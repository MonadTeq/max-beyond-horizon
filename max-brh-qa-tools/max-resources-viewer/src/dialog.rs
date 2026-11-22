//! The native `MAX.RES` selection dialog through SDL3 (only under the `sdl` feature).
//!
//! # Why raw FFI instead of `sdl3::dialog`
//!
//! The `sdl3` 0.22.1 crate wraps only `SDL_ShowOpenFileDialog`, which has
//! no title parameter (the title is always the system "Open File"). Since
//! SDL 3.2.0 there is `SDL_ShowFileDialogWithProperties` with
//! `SDL_PROP_FILE_DIALOG_TITLE_STRING`, `..._ACCEPT_STRING` and so on — exactly what is
//! needed for the `Укажите файл MAX.RES` title. The `sdl3-sys` 0.7.3 layer
//! (`SDL 3.4.20`) declares that function and the property constants, and `SDL3.dll`
//! 3.4.20 exports it (verified with `dumpbin /exports`). That is why there is a
//! small `unsafe` call directly through `sdl3::sys` here instead of the Windows FFI
//! `GetOpenFileNameW`: one code path works on Windows and Linux, the dialog stays
//! native, and the `unsafe` is isolated in the binary (in the library
//! `forbid(unsafe_code)` stays in place).
//!
//! # Asynchrony
//!
//! The SDL dialog is asynchronous: `SDL_ShowFileDialogWithProperties` returns
//! immediately and the result arrives in [`SDL_DialogFileCallback`] — possibly from
//! another thread. We keep an `mpsc` channel, pump `poll_iter()` (on Linux this is
//! required by the XDG portal) and wait for the result: the editor window does not open until
//! the user picks a file or cancels the dialog.
//!
//! All C strings and the filter array live until the end of the function, that is, longer than
//! the callback (SDL requires the filter array to be valid until the callback).

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use sdl3::sys;

use max_resources_viewer::startup::PickResult;

/// The dialog title (task item 1).
const DIALOG_TITLE: &str = "Укажите файл MAX.RES";
/// The filter caption in the file type list.
const FILTER_NAME: &str = "MAX.RES (*.RES)";
/// The extension without a dot, as SDL requires.
const FILTER_PATTERN: &str = "RES";

/// How often to wake up to pump SDL events while the dialog is open.
const PUMP_INTERVAL: Duration = Duration::from_millis(10);

/// Callback ownership: SDL receives a pointer to this structure and calls
/// the callback exactly once, after which [`dialog_callback`] frees the `Box`.
struct CallbackData {
    sender: mpsc::Sender<PickResult>,
}

/// The SDL C callback: parses `filelist` and sends the result into the channel.
///
/// # Safety
///
/// `userdata` must be a pointer to a `Box<CallbackData>` created in
/// [`pick_max_res_file`]; SDL calls the callback once per dialog. `filelist` is
/// either NULL (an error) or points to a NULL-terminated array of C strings.
unsafe extern "C" fn dialog_callback(
    userdata: *mut c_void,
    filelist: *const *const c_char,
    _filter: c_int,
) {
    // SAFETY: see the function contract; the Box is returned exactly once.
    let data = unsafe { Box::from_raw(userdata as *mut CallbackData) };

    let result = if filelist.is_null() {
        PickResult::Failed(format!("SDL: {}", sdl3::get_error()))
    } else {
        // SAFETY: filelist is not NULL and contains at least the terminating NULL.
        let first = unsafe { *filelist };

        if first.is_null() {
            PickResult::Cancelled
        } else {
            // SAFETY: the first element is a valid NULL-terminated C string.
            match unsafe { CStr::from_ptr(first) }.to_str() {
                Ok(path) => PickResult::Picked(PathBuf::from(path)),
                Err(error) => PickResult::Failed(format!("путь не в UTF-8: {error}")),
            }
        }
    };

    let _ = data.sender.send(result);
}

/// Shows the native `MAX.RES` selection dialog and waits for the user's decision.
///
/// `sdl` is an already initialized context (the dialog is created on the main
/// thread). The video subsystem is needed by platforms that attach the dialog to the
/// active window; here there is no window yet, so `window = NULL`.
pub fn pick_max_res_file(sdl: &sdl3::Sdl) -> PickResult {
    // We initialize video before the dialog: on some platforms SDL shows a
    // file dialog only with a live video subsystem. We keep it until the selection ends.
    let _video = match sdl.video() {
        Ok(video) => video,
        Err(error) => return PickResult::Failed(format!("SDL video: {error}")),
    };

    let mut events = match sdl.event_pump() {
        Ok(events) => events,
        Err(error) => return PickResult::Failed(format!("SDL event pump: {error}")),
    };

    // C strings: SDL copies the string properties, but it reads the filter array
    // up to the callback, so everything lives until the end of the function.
    let title = CString::new(DIALOG_TITLE).expect("в заголовке нет NUL");
    let filter_name = CString::new(FILTER_NAME).expect("в имени фильтра нет NUL");
    let filter_pattern = CString::new(FILTER_PATTERN).expect("в шаблоне фильтра нет NUL");
    let accept = CString::new("Открыть").expect("в подписи кнопки нет NUL");
    let cancel = CString::new("Отмена").expect("в подписи кнопки нет NUL");
    let location = std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .and_then(|path| CString::new(path).ok());

    let filters = [sys::dialog::SDL_DialogFileFilter {
        name: filter_name.as_ptr(),
        pattern: filter_pattern.as_ptr(),
    }];

    let (sender, receiver) = mpsc::channel();

    // SAFETY: the properties are created and destroyed in this function; the pointers to the
    // strings/filter array outlive the SDL call (see above). userdata is a
    // `Box<CallbackData>` that SDL returns exactly once to the callback.
    unsafe {
        let props = sys::properties::SDL_CreateProperties();

        if props.0 == 0 {
            return PickResult::Failed(format!("SDL_CreateProperties: {}", sdl3::get_error()));
        }

        set_string_property(
            props,
            sys::dialog::SDL_PROP_FILE_DIALOG_TITLE_STRING,
            title.as_ptr(),
        );
        set_string_property(
            props,
            sys::dialog::SDL_PROP_FILE_DIALOG_ACCEPT_STRING,
            accept.as_ptr(),
        );
        set_string_property(
            props,
            sys::dialog::SDL_PROP_FILE_DIALOG_CANCEL_STRING,
            cancel.as_ptr(),
        );

        if let Some(location) = &location {
            set_string_property(
                props,
                sys::dialog::SDL_PROP_FILE_DIALOG_LOCATION_STRING,
                location.as_ptr(),
            );
        }

        sys::properties::SDL_SetPointerProperty(
            props,
            sys::dialog::SDL_PROP_FILE_DIALOG_FILTERS_POINTER,
            filters.as_ptr() as *mut c_void,
        );
        sys::properties::SDL_SetNumberProperty(
            props,
            sys::dialog::SDL_PROP_FILE_DIALOG_NFILTERS_NUMBER,
            filters.len() as i64,
        );

        let data = Box::into_raw(Box::new(CallbackData { sender }));

        sys::dialog::SDL_ShowFileDialogWithProperties(
            sys::dialog::SDL_FILEDIALOG_OPENFILE,
            Some(dialog_callback),
            data as *mut c_void,
            props,
        );

        // We wait for exactly one callback; along the way we pump events (Linux/XDG portal).
        let result = loop {
            match receiver.recv_timeout(PUMP_INTERVAL) {
                Ok(result) => break result,
                Err(RecvTimeoutError::Timeout) => for _event in events.poll_iter() {},
                Err(RecvTimeoutError::Disconnected) => {
                    break PickResult::Failed(
                        "диалог завершился без результата (колбэк не вызван)".to_string(),
                    );
                }
            }
        };

        sys::properties::SDL_DestroyProperties(props);

        result
    }
}

/// Sets a string property of the dialog; a failure is not critical (a platform may
/// not support a particular property, in which case the system text remains).
///
/// # Safety
///
/// `props` must be a valid `SDL_PropertiesID`.
unsafe fn set_string_property(
    props: sys::properties::SDL_PropertiesID,
    name: *const c_char,
    value: *const c_char,
) {
    // SAFETY: the function contract — valid props and NULL-terminated strings.
    unsafe {
        sys::properties::SDL_SetStringProperty(props, name, value);
    }
}
