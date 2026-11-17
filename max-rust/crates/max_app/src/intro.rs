//! Экран `Introduction` главного меню — честная заглушка MVE-видео.
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
//!   `movie_cb_show_frame` с масштабированием под окно; любая клавиша
//!   прерывает (`mve_cb_ctl`).
//!
//! Декодер MVE в Rust-клиенте **не портирован** (отдельная задача, как
//! `MSC`-музыка). Поэтому экран — заглушка, но не пустая: он показывает
//! реальный фон игры (`MLOGO` 640×480 — big image из `MAX.RES`, иначе
//! `MAINPIC`) и текст в стиле клиента, который честно говорит, что вместо
//! видео. Esc (или любая клавиша/клик) возвращает в главное меню.

use max_assets::image::BigImage;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::menu::{draw_big_image, MenuAssets, TEXT};
use crate::panels::UiPalette;

/// Заголовок экрана (как подпись кнопки главного меню).
pub const INTRO_TITLE: &str = "Introduction";
/// Область заголовка: по центру, верхняя четверть кадра.
pub const INTRO_TITLE_RECT: PixelRect = PixelRect::new(120, 120, 400, 20);
/// Область текста-заполнителя.
pub const INTRO_TEXT_RECT: PixelRect = PixelRect::new(90, 176, 460, 150);

/// Строки заглушки: что играет оригинал и почему здесь не видео.
pub const INTRO_TEXT: [&str; 5] = [
    "The original plays an MVE movie: INTROFLC -> maxint.mve",
    "with subtitles from INTROSUB (Interplay MVE container).",
    "",
    "MVE playback is not ported to the Rust client yet -",
    "it is a separate task (see docs/visual-1to1-status.md).",
];

/// Цвет заголовка: золото меню (`Fonts_GoldColor` 165/175/199).
pub const TITLE_COLORS: (u8, u8, u8) = (165, 175, 199);
/// Цвет основного текста: зелёный `COLOR_GREEN` (0x02) палитры меню.
pub const TEXT_COLOR: u8 = crate::menu::GREEN;

/// Рисует экран-заглушку. Возвращает число закрашенных пикселей.
///
/// `menu` даёт фон (`MLOGO`, иначе `MAINPIC`) и его палитру — системную в
/// главном меню (`WindowManager_LoadBigImage(..., palette_from_image = true)`).
pub fn draw(frame: &mut RgbaBuffer, menu: &MenuAssets) -> u32 {
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
