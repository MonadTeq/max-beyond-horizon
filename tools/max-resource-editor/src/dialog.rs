//! Нативный диалог выбора `MAX.RES` через SDL3 (только под feature `sdl`).
//!
//! # Почему raw FFI, а не `sdl3::dialog`
//!
//! Крейт `sdl3` 0.22.1 оборачивает только `SDL_ShowOpenFileDialog`, у которого
//! нет параметра заголовка (заголовок всегда системный «Open File»). Начиная с
//! SDL 3.2.0 есть `SDL_ShowFileDialogWithProperties` с
//! `SDL_PROP_FILE_DIALOG_TITLE_STRING`, `..._ACCEPT_STRING` и т. п. — именно он
//! нужен для заголовка «Укажите файл MAX.RES». Слой `sdl3-sys` 0.7.3
//! (`SDL 3.4.20`) объявляет эту функцию и константы свойств, а `SDL3.dll`
//! 3.4.20 экспортирует её (проверено `dumpbin /exports`). Поэтому здесь
//! небольшой `unsafe`-вызов напрямую через `sdl3::sys`, а не Windows-FFI
//! `GetOpenFileNameW`: один код работает на Windows и Linux, диалог остаётся
//! нативным, а `unsafe` изолирован в бинарнике (в библиотеке
//! `forbid(unsafe_code)` сохраняется).
//!
//! # Асинхронность
//!
//! SDL-диалог асинхронный: `SDL_ShowFileDialogWithProperties` возвращается
//! сразу, результат приходит в [`SDL_DialogFileCallback`] — возможно, из
//! другого потока. Мы держим канал `mpsc`, крутим `poll_iter()` (на Linux это
//! нужно XDG-порталу) и ждём результата: окно редактора не открывается, пока
//! пользователь не выберет файл или не отменит диалог.
//!
//! Все C-строки и массив фильтров живут до конца функции, то есть дольше
//! колбэка (SDL требует, чтобы массив фильтров был валиден до колбэка).

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use sdl3::sys;

use max_resource_editor::startup::PickResult;

/// Заголовок диалога (пункт 1 задачи).
const DIALOG_TITLE: &str = "Укажите файл MAX.RES";
/// Подпись фильтра в списке типов файлов.
const FILTER_NAME: &str = "MAX.RES (*.RES)";
/// Расширение без точки, как требует SDL.
const FILTER_PATTERN: &str = "RES";

/// Как часто просыпаться, чтобы прокачать события SDL, пока диалог открыт.
const PUMP_INTERVAL: Duration = Duration::from_millis(10);

/// Владение колбэком: SDL получает указатель на эту структуру и вызывает
/// колбэк ровно один раз, после чего [`dialog_callback`] освобождает `Box`.
struct CallbackData {
    sender: mpsc::Sender<PickResult>,
}

/// C-колбэк SDL: разбирает `filelist` и отправляет результат в канал.
///
/// # Safety
///
/// `userdata` должен быть указателем на `Box<CallbackData>`, созданный в
/// [`pick_max_res_file`]; SDL вызывает колбэк один раз на диалог. `filelist`
/// либо NULL (ошибка), либо указывает на NULL-terminated массив C-строк.
unsafe extern "C" fn dialog_callback(
    userdata: *mut c_void,
    filelist: *const *const c_char,
    _filter: c_int,
) {
    // SAFETY: см. контракт функции; Box возвращается ровно один раз.
    let data = unsafe { Box::from_raw(userdata as *mut CallbackData) };

    let result = if filelist.is_null() {
        PickResult::Failed(format!("SDL: {}", sdl3::get_error()))
    } else {
        // SAFETY: filelist не NULL и содержит как минимум завершающий NULL.
        let first = unsafe { *filelist };

        if first.is_null() {
            PickResult::Cancelled
        } else {
            // SAFETY: первый элемент — валидная NULL-terminated C-строка.
            match unsafe { CStr::from_ptr(first) }.to_str() {
                Ok(path) => PickResult::Picked(PathBuf::from(path)),
                Err(error) => PickResult::Failed(format!("путь не в UTF-8: {error}")),
            }
        }
    };

    let _ = data.sender.send(result);
}

/// Показывает нативный диалог выбора `MAX.RES` и ждёт решения пользователя.
///
/// `sdl` — уже инициализированный контекст (диалог создаётся на главном
/// потоке). Видеоподсистема нужна платформам, которые привязывают диалог к
/// активному окну; здесь окна ещё нет, поэтому `window = NULL`.
pub fn pick_max_res_file(sdl: &sdl3::Sdl) -> PickResult {
    // Видео инициализируем до диалога: на части платформ SDL показывает
    // file dialog только при живом video subsystem. Держим до конца выбора.
    let _video = match sdl.video() {
        Ok(video) => video,
        Err(error) => return PickResult::Failed(format!("SDL video: {error}")),
    };

    let mut events = match sdl.event_pump() {
        Ok(events) => events,
        Err(error) => return PickResult::Failed(format!("SDL event pump: {error}")),
    };

    // C-строки: SDL копирует строковые свойства, но массив фильтров читает
    // вплоть до колбэка, поэтому всё живёт до конца функции.
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

    // SAFETY: свойства создаются и уничтожаются в этой функции; указатели на
    // строки/массив фильтров живут дольше вызова SDL (см. выше). userdata —
    // Box<CallbackData>, который SDL вернёт ровно один раз в колбэк.
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

        // Ждём ровно один колбэк; попутно прокачиваем события (Linux/XDG portal).
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

/// Выставляет строковое свойство диалога; неудача не критична (платформа может
/// не поддерживать конкретное свойство, тогда останется системный текст).
///
/// # Safety
///
/// `props` должен быть валидным `SDL_PropertiesID`.
unsafe fn set_string_property(
    props: sys::properties::SDL_PropertiesID,
    name: *const c_char,
    value: *const c_char,
) {
    // SAFETY: контракт функции — валидные props и NULL-terminated строки.
    unsafe {
        sys::properties::SDL_SetStringProperty(props, name, value);
    }
}
