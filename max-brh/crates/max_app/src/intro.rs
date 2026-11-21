//! Экран `Introduction` главного меню — настоящее MVE-видео (`INTROFLC`).
//!
//! Что делает оригинал (`max-cpp-port/src/menu.cpp:2666` →
//! `Movie_PlayIntro`, `movie.cpp`):
//!
//! * `Movie_PlayIntro()` читает настройку `intro_movie` (0 — выключено) и
//!   вызывает `movie_run(INTROFLC, track, INTROSUB)`;
//! * ресурс `INTROFLC` в `MAX.RES` — не видео, а ASCIIZ-ссылка на файл
//!   (`maxint.mve`, 11 байт), поэтому `ResourceManager_OpenFileResource`
//!   открывает `.MVE`-контейнер рядом с архивом;
//! * `movie_run` крутит `MVE_rmStepMovie()` из `mvelib32.c` (Interplay MVE:
//!   собственный видеокодек + звуковые дорожки по языкам), кадр выводится
//!   `movie_cb_show_frame` с масштабированием под окно (letterbox: 432×320 →
//!   640×474 в кадре 640×480); любая клавиша прерывает (`mve_cb_ctl`).
//!
//! В Rust-клиенте видео декодирует [`max_assets::mve`] (точный порт
//! `nfPkDecomp`: 4391 кадр, 15 FPS, 432×320). Экран:
//!
//! * при входе открывает `maxint.mve` и декодирует кадр 0 — поэтому `shot`
//!   в QA детерминирован, а `dt = 0` оставляет стоп-кадр;
//! * в GUI продвигает кадры по реальному `dt` (15 FPS, как `TIMER_FPS_TO_MS`);
//! * по концу видео и по Esc/клику возвращает в главное меню;
//! * если данных нет (юнит-тесты без `MAX.RES`), показывает прежнюю
//!   заглушку с фоном `MLOGO`/`MAINPIC`, чтобы экран не был пустым.
//!
//! Аудио и субтитры (`INTROSUB`, `moviesubtitle.cpp`) не портированы — это
//! честно отмечено в `docs/analysis/visual-1to1-status.md`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use max_assets::image::BigImage;
use max_assets::mve::{Mve, MveDecoder, MveError};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::menu::{draw_big_image, MenuAssets, TEXT};
use crate::panels::UiPalette;

/// Заголовок экрана (как подпись кнопки главного меню).
pub const INTRO_TITLE: &str = "Introduction";
/// Область заголовка: по центру, верхняя четверть кадра.
pub const INTRO_TITLE_RECT: PixelRect = PixelRect::new(120, 120, 400, 20);
/// Область текста-заглушки.
pub const INTRO_TEXT_RECT: PixelRect = PixelRect::new(90, 176, 460, 150);

/// Строки заглушки: что играет оригинал и почему здесь не видео.
pub const INTRO_TEXT: [&str; 5] = [
    "The original plays an MVE movie: INTROFLC -> maxint.mve",
    "with subtitles from INTROSUB (Interplay MVE container).",
    "",
    "MVE data was not found next to MAX.RES -",
    "the video screen needs the original game files.",
];

/// Цвет заголовка: золото меню (`Fonts_GoldColor` 165/175/199).
pub const TITLE_COLORS: (u8, u8, u8) = (165, 175, 199);
/// Цвет основного текста: зелёный `COLOR_GREEN` (0x02) палитры меню.
pub const TEXT_COLOR: u8 = crate::menu::GREEN;

/// Состояние проигрывателя (общее у клонов [`IntroScreen`]).
#[derive(Debug)]
struct Playback {
    /// Декодер MVE (если файл открылся и первый кадр декодирован).
    decoder: Option<MveDecoder>,
    /// Текущий индексированный кадр (пусто до загрузки/при ошибке).
    frame: Vec<u8>,
    /// Палитра текущего кадра.
    palette: [[u8; 3]; 256],
    /// Ширина/высота видео в пикселях.
    width: u32,
    height: u32,
    /// Сколько кадров уже декодировано (для статуса/тестов).
    frame_index: usize,
    /// Накопленное время с последнего показанного кадра.
    elapsed: f32,
    /// Видео закончилось.
    finished: bool,
    /// Ошибка открытия/декодирования (тогда рисуется заглушка).
    error: Option<String>,
}

impl Playback {
    fn new() -> Self {
        Self {
            decoder: None,
            frame: Vec::new(),
            palette: [[0u8; 3]; 256],
            width: 0,
            height: 0,
            frame_index: 0,
            elapsed: 0.0,
            finished: false,
            error: None,
        }
    }

    /// Открывает MVE и декодирует кадр 0.
    fn load(&mut self, path: &Path) {
        let mve = match Mve::open(path) {
            Ok(mve) => mve,
            Err(error) => {
                self.error = Some(format!("{}: {error}", path.display()));
                return;
            }
        };

        let info = mve.info();
        let mut decoder = mve.decoder();

        match decoder.next_frame() {
            Ok(Some(frame)) => {
                let pixels = frame.to_vec();
                self.width = u32::from(info.width);
                self.height = u32::from(info.height);
                self.palette = *decoder.palette();
                self.frame = pixels;
                self.frame_index = 1;
                self.decoder = Some(decoder);
            }
            Ok(None) => {
                self.error = Some(format!("{}: в MVE нет кадров", path.display()));
            }
            Err(error) => {
                self.error = Some(format!("{}: {error}", path.display()));
            }
        }
    }

    /// Продвигает видео на `dt` секунд. `true` — видео закончилось.
    fn advance(&mut self, dt: f32) -> bool {
        // Ошибка декодирования — показываем заглушку/последний кадр, но не
        // выкидываем игрока из экрана автоматически.
        if self.error.is_some() {
            return false;
        }

        if self.finished {
            return true;
        }

        let Some(decoder) = self.decoder.as_mut() else {
            return false;
        };

        if dt <= 0.0 {
            return false;
        }

        // Не догоняем больше 8 кадров за один вызов (как пропуск кадров в
        // `sync_late`): иначе после паузы окно «прыгнет» через полфильма.
        self.elapsed += dt.min(0.5);
        let frame_delay = decoder.info().frame_delay_seconds().max(1.0 / 120.0);
        let mut decoded = 0;

        while self.elapsed >= frame_delay && decoded < 8 {
            match decoder.next_frame() {
                Ok(Some(frame)) => {
                    let pixels = frame.to_vec();
                    self.palette = *decoder.palette();
                    self.frame = pixels;
                    self.frame_index += 1;
                    self.elapsed -= frame_delay;
                    decoded += 1;
                }
                Ok(None) => {
                    self.finished = true;

                    return true;
                }
                Err(error) => {
                    self.error = Some(mve_error_line(error));
                    self.finished = true;

                    return true;
                }
            }
        }

        false
    }
}

fn mve_error_line(error: MveError) -> String {
    format!("MVE: {error}")
}

/// Экран `Introduction` с ленивым проигрывателем MVE.
#[derive(Debug, Clone)]
pub struct IntroScreen {
    path: Option<PathBuf>,
    state: Arc<Mutex<Playback>>,
}

impl PartialEq for IntroScreen {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for IntroScreen {}

impl IntroScreen {
    /// Создаёт экран и сразу декодирует кадр 0 (если файл есть).
    ///
    /// `path` — путь к `maxint.mve` рядом с `MAX.RES` (см.
    /// [`crate::assets::Assets::intro_mve`]); `None` — заглушка.
    pub fn new(path: Option<PathBuf>) -> Self {
        let state = Arc::new(Mutex::new(Playback::new()));

        if let Some(path) = path.as_deref() {
            state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .load(path);
        }

        Self { path, state }
    }

    /// Путь к открытому MVE (для тестов/диагностики).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Есть ли декодированный кадр (MVE открылся).
    pub fn is_loaded(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        state.decoder.is_some() && !state.frame.is_empty()
    }

    /// Сколько кадров декодировано (0 — только заглушка).
    pub fn frame_index(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .frame_index
    }

    /// Ошибка открытия/декодирования (для статуса/тестов).
    pub fn error(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .error
            .clone()
    }

    /// Один шаг видео (`dt` секунд; `dt <= 0` — стоп-кадр).
    ///
    /// `true` — видео закончилось, экран должен вернуться в меню.
    pub fn advance(&mut self, dt: f32) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .advance(dt)
    }

    /// Рисует текущий кадр видео (или заглушку, если данных нет).
    pub fn draw(&self, frame: &mut RgbaBuffer, menu: &MenuAssets) -> u32 {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if state.decoder.is_some() && !state.frame.is_empty() {
            draw_video(frame, &state)
        } else {
            draw_placeholder(frame, menu)
        }
    }
}

/// Масштабирует индексированный кадр в 640×480 с letterbox, как
/// `movie_cb_show_frame` + `cscale` (целочисленный nearest-neighbor без
/// сглаживания; фон вокруг кадра чёрный).
fn draw_video(frame: &mut RgbaBuffer, state: &Playback) -> u32 {
    frame.fill(Rgba::rgb(0, 0, 0));

    let source_width = state.width as i32;
    let source_height = state.height as i32;

    if source_width <= 0 || source_height <= 0 {
        return 0;
    }

    let screen_width = frame.width() as i32;
    let screen_height = frame.height() as i32;
    let scale_w = screen_width as f32 / source_width as f32;
    let scale_h = screen_height as f32 / source_height as f32;
    let scale = if scale_h >= scale_w { scale_w } else { scale_h };

    let mut target_width = (scale * source_width as f32) as i32;
    let mut target_height = (scale * source_height as f32) as i32;

    target_width = target_width.min(screen_width);
    target_height = target_height.min(screen_height);

    let offset_x = (screen_width - target_width) / 2;
    let offset_y = (screen_height - target_height) / 2;
    let mx = (target_width << 16) / source_width;
    let my = (target_height << 16) / source_height;
    let mut painted = 0u32;

    for source_y in 0..source_height {
        let start_y = (my * source_y) >> 16;
        let end_y = (my + my * source_y) >> 16;
        let row = source_y as usize * source_width as usize;

        for source_x in 0..source_width {
            let start_x = (mx * source_x) >> 16;
            let end_x = (mx + mx * source_x) >> 16;
            let index = state.frame[row + source_x as usize];
            let color = state.palette[usize::from(index)];
            let rgba = Rgba::rgb(color[0], color[1], color[2]);

            for y in start_y..end_y {
                for x in start_x..end_x {
                    if frame.set(offset_x + x, offset_y + y, rgba) {
                        painted += 1;
                    }
                }
            }
        }
    }

    painted
}

/// Прежняя заглушка: фон `MLOGO`/`MAINPIC` + заголовок и честный текст.
fn draw_placeholder(frame: &mut RgbaBuffer, menu: &MenuAssets) -> u32 {
    let background: &BigImage = menu.mlogo.as_ref().unwrap_or(&menu.mainpic);
    let painted = draw_big_image(frame, background);
    let palette = UiPalette::from_big_image(&menu.mainpic);

    // Затемнение области текста, чтобы надпись читалась на любом фоне
    // (в оригинале здесь кадр видео — заглушке нужна подложка).
    let shade = Rgba::rgb(8, 10, 14);
    let box_rect = PixelRect::new(80, 108, 480, 240);

    for y in box_rect.uly..box_rect.uly + box_rect.height {
        for x in box_rect.ulx..box_rect.ulx + box_rect.width {
            frame.set(x, y, shade);
        }
    }

    draw_text_gnw_centered_shadowed(
        frame,
        INTRO_TITLE_RECT,
        GnwFontId::Font1.font(),
        INTRO_TITLE,
        Rgba::rgb(TITLE_COLORS.0, TITLE_COLORS.1, TITLE_COLORS.2),
        palette.rgba(0x00),
        palette.rgba(0x00),
    );

    let font = GnwFontId::Font2.font();
    let line_height = font.line_height();
    let mut y = INTRO_TEXT_RECT.uly;

    for line in INTRO_TEXT {
        if !line.is_empty() {
            draw_text_gnw_centered(
                frame,
                PixelRect::new(
                    INTRO_TEXT_RECT.ulx,
                    y,
                    INTRO_TEXT_RECT.width,
                    font.height() + 1,
                ),
                font,
                line,
                palette.rgba(TEXT_COLOR),
            );
        }

        y += line_height + 6;
    }

    draw_text_gnw_centered(
        frame,
        PixelRect::new(
            INTRO_TEXT_RECT.ulx,
            304,
            INTRO_TEXT_RECT.width,
            font.height() + 1,
        ),
        font,
        "Press Esc to return to the main menu",
        palette.rgba(TEXT),
    );

    painted
}
