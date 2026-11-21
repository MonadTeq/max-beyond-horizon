//! Поиск `MAX.RES`/`COLOR.PAL` при запуске UI и выбор файла в диалоге.
//!
//! Порядок старта (пункты 1–2 задачи):
//!
//! 1. путь из аргумента командной строки (`--res PATH` или позиционный);
//! 2. дефолтные пути: рядом с `.exe`, в `CWD`, вверх по дереву каталогов
//!    (`max-original/MAX.RES` — так находится корень репозитория);
//! 3. нативный диалог `Укажите файл MAX.RES` (SDL3), если ничего не найдено.
//!
//! Модуль намеренно не зависит от SDL: диалог передаётся замыканием
//! [`PickResult`], поэтому порядок поиска и отмена проверяются unit-тестами
//! без окна и без SDL3. Сам SDL-диалог живёт в `src/dialog.rs` бинарника.
//!
//! `COLOR.PAL` ищется рядом с выбранным `MAX.RES`; если файла нет, редактор
//! продолжает работать с заглушкой-палитрой (градации серого) и предупреждает
//! об этом в stderr и в статусной строке окна.

use std::fmt;
use std::path::{Path, PathBuf};

use max_render::palette::Palette;

/// Имя архива ресурсов.
pub const RES_FILE_NAME: &str = "MAX.RES";
/// Имя системной палитры.
pub const PAL_FILE_NAME: &str = "COLOR.PAL";
/// Каталог с оригинальными ассетами в репозитории.
pub const ORIGINAL_DIR: &str = "max-original";

/// Результат нативного диалога выбора файла.
///
/// SDL-колбэк вызывает [`PickResult::Cancelled`] при отмене и
/// [`PickResult::Failed`] при ошибке платформы (тогда путь выбрать нельзя).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickResult {
    /// Пользователь выбрал файл.
    Picked(PathBuf),
    /// Пользователь закрыл диалог без выбора.
    Cancelled,
    /// Диалог не удалось показать/получить результат.
    Failed(String),
}

/// Ошибка старта UI (до открытия окна).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupError {
    /// Диалог отменён — работать не с чем.
    Cancelled,
    /// Диалог не смог показать файл (SDL/платформа).
    Dialog(String),
    /// Диалог вернул путь, которого нет на диске.
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

/// Кандидаты по умолчанию в порядке приоритета.
///
/// `exe` — путь к исполняемому файлу, `cwd` — текущий каталог. Оба параметра
/// нужны для тестируемости; [`default_candidates`] подставляет реальные.
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

/// Кандидаты по умолчанию для текущего процесса (`current_exe` + `current_dir`).
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

/// Первый существующий кандидат (проверка — [`Path::is_file`] у вызывающего).
pub fn resolve_max_res(candidates: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    candidates.iter().find(|path| exists(path)).cloned()
}

/// Полный порядок старта UI: аргумент → дефолты → диалог.
///
/// `pick` вызывается только если аргумента нет и ни один кандидат не найден.
/// Отмена/ошибка диалога возвращаются как [`StartupError`] — вызывающий код
/// печатает сообщение и завершается с кодом 1. Путь из аргумента не
/// проверяется здесь: ошибку чтения архива покажет загрузка `MAX.RES`.
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

/// Путь к `COLOR.PAL` рядом с выбранным `MAX.RES`.
pub fn palette_path(res: &Path) -> PathBuf {
    res.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(PAL_FILE_NAME)
}

/// Результат поиска/загрузки палитры.
#[derive(Debug)]
pub struct PaletteLoad {
    /// Палитра для рендера: настоящая или [`stub_palette`].
    pub palette: Palette,
    /// Откуда прочитана палитра (`None` — заглушка).
    pub path: Option<PathBuf>,
    /// Предупреждение для stderr/статусной строки, если палитры нет.
    pub warning: Option<String>,
}

/// Загружает палитру: явный `--pal` → `COLOR.PAL` рядом с `MAX.RES` → заглушка.
///
/// Явно заданная палитра имеет приоритет; если её не удалось прочитать,
/// пробуем соседнюю с `MAX.RES`. Любая неудача — предупреждение, но не ошибка:
/// редактор остаётся работоспособным.
pub fn load_palette(explicit: Option<&Path>, res: &Path) -> PaletteLoad {
    load_palette_with(explicit, res, |path| {
        Palette::load(path).map_err(|error| error.to_string())
    })
}

/// Тестируемая часть [`load_palette`]: загрузчик подменяется замыканием.
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

/// Заглушка-палитра: 256 градаций серого (6-битные компоненты, как в COLOR.PAL).
///
/// Нужна, чтобы превью simple/спрайтов не падало без `COLOR.PAL`. Индекс
/// `i` даёт `rgb = (i / 4) * 4`: от чёрного до почти белого.
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
