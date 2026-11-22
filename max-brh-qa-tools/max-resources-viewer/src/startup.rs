//! Finding `MAX.RES`/`COLOR.PAL` at UI startup and picking the file in the dialog.
//!
//! Startup order (task items 1–2):
//!
//! 1. the path from the command line argument (`--res PATH` or a positional one);
//! 2. the default paths: next to the `.exe`, in `CWD`, up the directory tree
//!    (`max-original/MAX.RES` — that is how the repository root is found);
//! 3. the native `Укажите файл MAX.RES` dialog (SDL3) if nothing was found.
//!
//! The module deliberately does not depend on SDL: the dialog is passed in as a closure
//! ([`PickResult`]), so the search order and cancellation are covered by unit tests
//! without a window and without SDL3. The SDL dialog itself lives in the binary's `src/dialog.rs`.
//!
//! `COLOR.PAL` is looked up next to the selected `MAX.RES`; if the file is missing, the editor
//! keeps working with a stub palette (grayscale) and warns
//! about it in stderr and in the window's status line.

use std::fmt;
use std::path::{Path, PathBuf};

use max_render::palette::Palette;

/// The resource archive name.
pub const RES_FILE_NAME: &str = "MAX.RES";
/// The system palette name.
pub const PAL_FILE_NAME: &str = "COLOR.PAL";
/// The directory with the original assets in the repository.
pub const ORIGINAL_DIR: &str = "max-original";

/// The result of the native file dialog.
///
/// The SDL callback produces [`PickResult::Cancelled`] on cancellation and
/// [`PickResult::Failed`] on a platform error (then no path can be chosen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickResult {
    /// The user picked a file.
    Picked(PathBuf),
    /// The user closed the dialog without choosing.
    Cancelled,
    /// The dialog could not be shown/the result obtained.
    Failed(String),
}

/// A UI startup error (before the window opens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupError {
    /// The dialog was cancelled — there is nothing to work with.
    Cancelled,
    /// The dialog could not show the file (SDL/platform).
    Dialog(String),
    /// The dialog returned a path that does not exist on disk.
    PickedMissing(PathBuf),
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(
                formatter,
                "выбор MAX.RES отменён — редактор не открыт. \
                 Укажите файл аргументом: max-resources-viewer --res <MAX.RES>"
            ),
            Self::Dialog(error) => {
                write!(
                    formatter,
                    "не удалось показать диалог выбора MAX.RES: {error}"
                )
            }
            Self::PickedMissing(path) => {
                write!(formatter, "выбранный файл не найден: {}", path.display())
            }
        }
    }
}

impl std::error::Error for StartupError {}

/// The default candidates in priority order.
///
/// `exe` is the path to the executable, `cwd` the current directory. Both parameters are
/// needed for testability; [`default_candidates`] substitutes the real ones.
pub fn default_candidates_for(exe: Option<&Path>, cwd: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(exe_dir) = exe.and_then(Path::parent) {
        candidates.push(exe_dir.join(RES_FILE_NAME));
        candidates.push(exe_dir.join(ORIGINAL_DIR).join(RES_FILE_NAME));

        for ancestor in exe_dir.ancestors().skip(1) {
            candidates.push(ancestor.join(ORIGINAL_DIR).join(RES_FILE_NAME));
        }
    }

    if let Some(cwd) = cwd {
        candidates.push(cwd.join(RES_FILE_NAME));
        candidates.push(cwd.join(ORIGINAL_DIR).join(RES_FILE_NAME));

        for ancestor in cwd.ancestors().skip(1) {
            candidates.push(ancestor.join(ORIGINAL_DIR).join(RES_FILE_NAME));
        }
    }

    dedup(candidates)
}

/// The default candidates for the current process (`current_exe` + `current_dir`).
pub fn default_candidates() -> Vec<PathBuf> {
    let exe = std::env::current_exe().ok();
    let cwd = std::env::current_dir().ok();

    default_candidates_for(exe.as_deref(), cwd.as_deref())
}

fn dedup(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique: Vec<PathBuf> = Vec::with_capacity(paths.len());

    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }

    unique
}

/// The first existing candidate (the caller checks it with [`Path::is_file`]).
pub fn resolve_max_res(candidates: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    candidates.iter().find(|path| exists(path)).cloned()
}

/// The full UI startup order: argument → defaults → dialog.
///
/// `pick` is called only if there is no argument and no candidate was found.
/// Cancellation/dialog errors are returned as [`StartupError`]; the calling code
/// prints a message and exits with code 1. The path from the argument is not
/// checked here: a failure to read the archive will be shown when `MAX.RES` is loaded.
pub fn resolve_startup_res<F>(
    arg: Option<&Path>,
    candidates: &[PathBuf],
    exists: impl Fn(&Path) -> bool,
    mut pick: F,
) -> Result<PathBuf, StartupError>
where
    F: FnMut() -> PickResult,
{
    if let Some(arg) = arg {
        return Ok(arg.to_path_buf());
    }

    if let Some(found) = resolve_max_res(candidates, &exists) {
        return Ok(found);
    }

    match pick() {
        PickResult::Picked(path) => {
            if exists(&path) {
                Ok(path)
            } else {
                Err(StartupError::PickedMissing(path))
            }
        }
        PickResult::Cancelled => Err(StartupError::Cancelled),
        PickResult::Failed(error) => Err(StartupError::Dialog(error)),
    }
}

/// The path to `COLOR.PAL` next to the selected `MAX.RES`.
pub fn palette_path(res: &Path) -> PathBuf {
    res.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(PAL_FILE_NAME)
}

/// The result of the palette search/load.
#[derive(Debug)]
pub struct PaletteLoad {
    /// The palette for rendering: the real one or [`stub_palette`].
    pub palette: Palette,
    /// Where the palette was read from (`None` — the stub).
    pub path: Option<PathBuf>,
    /// A warning for stderr/the status line if there is no palette.
    pub warning: Option<String>,
}

/// Loads the palette: an explicit `--pal` → `COLOR.PAL` next to `MAX.RES` → the stub.
///
/// An explicitly given palette has priority; if it could not be read,
/// we try the one next to `MAX.RES`. Any failure is a warning, not an error:
/// the editor stays usable.
pub fn load_palette(explicit: Option<&Path>, res: &Path) -> PaletteLoad {
    load_palette_with(explicit, res, |path| {
        Palette::load(path).map_err(|error| error.to_string())
    })
}

/// The testable part of [`load_palette`]: the loader is replaced by a closure.
pub fn load_palette_with<F>(explicit: Option<&Path>, res: &Path, mut load: F) -> PaletteLoad
where
    F: FnMut(&Path) -> Result<Palette, String>,
{
    let adjacent = palette_path(res);
    let mut notes: Vec<String> = Vec::new();

    if let Some(explicit) = explicit {
        match load(explicit) {
            Ok(palette) => {
                return PaletteLoad {
                    palette,
                    path: Some(explicit.to_path_buf()),
                    warning: None,
                };
            }
            Err(error) => notes.push(format!("--pal {}: {error}", explicit.display())),
        }
    }

    if explicit != Some(adjacent.as_path()) {
        match load(&adjacent) {
            Ok(palette) => {
                let warning = if notes.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{}; использую палитру {}",
                        notes.join("; "),
                        adjacent.display()
                    ))
                };

                return PaletteLoad {
                    palette,
                    path: Some(adjacent),
                    warning,
                };
            }
            Err(error) => notes.push(error),
        }
    }

    let warning = format!(
        "COLOR.PAL не найден рядом с {} ({}). Превью рисуется заглушкой-палитрой \
         (градации серого), редактор работает. Положите COLOR.PAL рядом с MAX.RES \
         или укажите --pal PATH.",
        res.display(),
        notes.join("; ")
    );

    PaletteLoad {
        palette: stub_palette(),
        path: None,
        warning: Some(warning),
    }
}

/// The stub palette: 256 grayscale steps (6-bit components, as in COLOR.PAL).
///
/// Needed so that the simple/sprite previews do not fail without `COLOR.PAL`. Index
/// `i` gives `rgb = (i / 4) * 4`: from black to almost white.
pub fn stub_palette() -> Palette {
    let mut bytes = [0u8; 768];

    for (index, entry) in bytes.chunks_exact_mut(3).enumerate() {
        let level = (index as u8) >> 2;
        entry[0] = level;
        entry[1] = level;
        entry[2] = level;
    }

    Palette::from_color_pal(&bytes).expect("768 байт — валидный COLOR.PAL")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }

    #[test]
    fn default_candidates_start_next_to_exe_and_include_cwd() {
        let exe = Path::new("/repo/tools/editor/target/release/max-resources-viewer.exe");
        let cwd = Path::new("/repo/tools/editor");
        let candidates = default_candidates_for(Some(exe), Some(cwd));

        assert_eq!(
            candidates[0],
            path("/repo/tools/editor/target/release/MAX.RES")
        );
        assert_eq!(
            candidates[1],
            path("/repo/tools/editor/target/release/max-original/MAX.RES")
        );
        assert_eq!(
            candidates[2],
            path("/repo/tools/editor/target/max-original/MAX.RES"),
            "сразу за exe идёт поиск вверх по дереву от каталога exe"
        );
        assert!(
            candidates.contains(&path("/repo/tools/editor/MAX.RES")),
            "CWD/MAX.RES должен быть кандидатом: {candidates:?}"
        );
        assert!(
            candidates.contains(&path("/repo/tools/editor/max-original/MAX.RES")),
            "CWD/max-original/MAX.RES должен быть кандидатом: {candidates:?}"
        );
    }

    #[test]
    fn default_candidates_walk_up_to_repository_root() {
        let exe = Path::new("/repo/tools/editor/target/release/max-resources-viewer.exe");
        let cwd = Path::new("/repo/tools/editor");
        let candidates = default_candidates_for(Some(exe), Some(cwd));

        let repo_root = path("/repo/max-original/MAX.RES");
        assert!(
            candidates.contains(&repo_root),
            "вверх по дереву должен находиться корень репозитория: {candidates:?}"
        );
        assert!(
            candidates.contains(&path("/max-original/MAX.RES")),
            "поиск идёт до корня файловой системы: {candidates:?}"
        );
    }

    #[test]
    fn default_candidates_are_deduplicated() {
        let same = Path::new("/repo/MAX.RES");
        let candidates = default_candidates_for(Some(same), Some(same));
        let mut sorted = candidates.clone();
        sorted.sort();
        sorted.dedup();

        assert_eq!(candidates.len(), sorted.len(), "{candidates:?}");
    }

    #[test]
    fn resolve_max_res_skips_missing_candidates_in_order() {
        let candidates = vec![
            path("/exe/MAX.RES"),
            path("/work/max-original/MAX.RES"),
            path("/work/MAX.RES"),
        ];
        let present = path("/work/max-original/MAX.RES");

        assert_eq!(
            resolve_max_res(&candidates, |candidate| candidate == present),
            Some(present)
        );
        assert_eq!(resolve_max_res(&candidates, |_| false), None);
    }

    #[test]
    fn repo_root_default_is_found_before_the_dialog() {
        let exe = Path::new("/repo/tools/editor/target/release/max-resources-viewer.exe");
        let cwd = Path::new("/repo/tools/editor");
        let candidates = default_candidates_for(Some(exe), Some(cwd));
        let repo_res = path("/repo/max-original/MAX.RES");

        let resolved = resolve_startup_res(
            None,
            &candidates,
            |candidate| candidate == repo_res,
            || panic!("диалог не нужен: MAX.RES найден в корне репозитория"),
        )
        .unwrap();

        assert_eq!(resolved, repo_res);
    }

    #[test]
    fn explicit_argument_wins_over_defaults() {
        let candidates = vec![path("/exe/MAX.RES")];
        let picked = std::cell::Cell::new(false);

        let resolved = resolve_startup_res(
            Some(Path::new("/data/MAX.RES")),
            &candidates,
            |_| true,
            || {
                picked.set(true);
                PickResult::Cancelled
            },
        )
        .unwrap();

        assert_eq!(resolved, path("/data/MAX.RES"));
        assert!(!picked.get(), "диалог не должен открываться при аргументе");
    }

    #[test]
    fn first_existing_default_is_selected_in_order() {
        let candidates = vec![
            path("/exe/MAX.RES"),
            path("/repo/max-original/MAX.RES"),
            path("/work/max-original/MAX.RES"),
        ];
        let present = path("/work/max-original/MAX.RES");

        let resolved = resolve_startup_res(
            None,
            &candidates,
            |candidate| candidate == present,
            || panic!("диалог не нужен: дефолт найден"),
        )
        .unwrap();

        assert_eq!(resolved, present);
    }

    #[test]
    fn dialog_is_opened_when_nothing_is_found() {
        let candidates = vec![path("/exe/MAX.RES")];
        let picked_path = path("/chosen/MAX.RES");
        let mut calls = 0;

        let resolved = resolve_startup_res(
            None,
            &candidates,
            |candidate| candidate == picked_path,
            || {
                calls += 1;
                PickResult::Picked(picked_path.clone())
            },
        )
        .unwrap();

        assert_eq!(resolved, picked_path);
        assert_eq!(calls, 1);
    }

    #[test]
    fn dialog_cancel_is_a_clear_error() {
        let error = resolve_startup_res(
            None,
            &[path("/exe/MAX.RES")],
            |_| false,
            || PickResult::Cancelled,
        )
        .unwrap_err();

        assert_eq!(error, StartupError::Cancelled);
        let text = error.to_string();
        assert!(text.contains("отмен"), "{text}");
        assert!(text.contains("--res"), "{text}");
    }

    #[test]
    fn dialog_failure_is_propagated() {
        let error = resolve_startup_res(
            None,
            &[],
            |_| false,
            || PickResult::Failed("SDL: нет видеодрайвера".to_string()),
        )
        .unwrap_err();

        assert_eq!(
            error,
            StartupError::Dialog("SDL: нет видеодрайвера".to_string())
        );
        assert!(error.to_string().contains("нет видеодрайвера"));
    }

    #[test]
    fn dialog_can_return_a_missing_file() {
        let missing = path("/chosen/MAX.RES");
        let error =
            resolve_startup_res(None, &[], |_| false, || PickResult::Picked(missing.clone()))
                .unwrap_err();

        assert_eq!(error, StartupError::PickedMissing(missing));
        assert!(error.to_string().contains("не найден"));
    }

    #[test]
    fn palette_is_looked_up_next_to_res() {
        let res = Path::new("/repo/max-original/MAX.RES");
        let expected = path("/repo/max-original/COLOR.PAL");
        let stub = stub_palette();
        let mut loaded_from = None;

        let load = load_palette_with(None, res, |candidate| {
            loaded_from = Some(candidate.to_path_buf());
            Ok(stub.clone())
        });

        assert_eq!(loaded_from, Some(expected.clone()));
        assert_eq!(load.path, Some(expected));
        assert_eq!(load.palette, stub);
        assert!(load.warning.is_none());
    }

    #[test]
    fn explicit_palette_has_priority() {
        let res = Path::new("/repo/max-original/MAX.RES");
        let explicit = Path::new("/opt/palettes/COLOR.PAL");
        let stub = stub_palette();
        let mut attempts = Vec::new();

        let load = load_palette_with(Some(explicit), res, |candidate| {
            attempts.push(candidate.to_path_buf());
            if candidate == explicit {
                Ok(stub.clone())
            } else {
                Err("не должен читаться".to_string())
            }
        });

        assert_eq!(attempts, vec![explicit.to_path_buf()]);
        assert_eq!(load.path.as_deref(), Some(explicit));
        assert!(load.warning.is_none());
    }

    #[test]
    fn broken_explicit_palette_falls_back_to_adjacent() {
        let res = Path::new("/repo/max-original/MAX.RES");
        let explicit = Path::new("/opt/palettes/COLOR.PAL");
        let adjacent = path("/repo/max-original/COLOR.PAL");
        let stub = stub_palette();

        let load = load_palette_with(Some(explicit), res, |candidate| {
            if candidate == adjacent {
                Ok(stub.clone())
            } else {
                Err("нет файла".to_string())
            }
        });

        assert_eq!(load.path, Some(adjacent));
        let warning = load.warning.expect("должно быть предупреждение");
        assert!(warning.contains("--pal"), "{warning}");
        assert!(warning.contains("COLOR.PAL"), "{warning}");
    }

    #[test]
    fn missing_palette_warns_but_editor_gets_a_stub() {
        let res = Path::new("/repo/max-original/MAX.RES");
        let load = load_palette_with(None, res, |_| Err("No such file".to_string()));

        assert_eq!(load.path, None);
        assert_eq!(load.palette, stub_palette());

        let warning = load.warning.expect("должно быть предупреждение");
        assert!(warning.contains("COLOR.PAL"), "{warning}");
        assert!(warning.contains("заглушк"), "{warning}");
        assert!(warning.contains("редактор работает"), "{warning}");
    }

    #[test]
    fn stub_palette_is_a_grayscale_ramp() {
        let palette = stub_palette();

        assert_eq!(palette.rgb(0), [0, 0, 0]);
        assert_eq!(palette.rgb(255), [252, 252, 252]);
        assert!(palette.rgb(128)[0] > palette.rgb(64)[0]);
        assert_eq!(palette.rgb(128), [128, 128, 128]);
    }
}
