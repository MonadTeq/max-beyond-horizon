//! Растеризация ресурсов в RGBA: палитры, прозрачность, лента кадров.
//!
//! Соглашения — те же, что у `max-assets` (`tools/max-resource-editor`), чтобы
//! выгрузка юнитов стыковалась с уже существующей:
//!
//! * спрайты (`COMMTWR`…`WALDO`) индексируются `COLOR.PAL`, у которого
//!   компоненты уже умножены на 4 (`Palette::rgb`);
//! * структурная прозрачность спрайта (пропущенные RLE-пиксели) → альфа 0;
//! * тень `S_*` — маска: затемняемый пиксель чёрный с альфой 160, остальное
//!   прозрачно;
//! * `sheet.png` — горизонтальная лента кадров, выровненная по горячей точке
//!   (`hotx/hoty`), поэтому анимация не «прыгает» (раскладка 1:1 с
//!   `max-assets/README.md`, §«Лента кадров»);
//! * портрет `P_*` — big image 300×240: индексы картинки переводятся
//!   **палитрой окна** (`FRAMEPIC`), как это делает `CargoMenu`/`BuildMenu`
//!   (`WindowManager_LoadBigImage(..., palette_from_image = false)`), кадр
//!   полностью непрозрачен;
//! * кадры FLC — индексы кадра + палитра `COLOR_256` этого FLC, компоненты
//!   хранятся 6-битными, при выводе умножаются на 4.

use max_assets::flic::FlicFrame;
use max_assets::image::BigImage;
use max_assets::sprite::{ShadowFrame, SpriteFrame};
use max_render::palette::Palette;

use crate::png::RgbaImage;

/// Альфа тени в выгрузке (как в `max-assets`: `_mask`-PNG).
pub const SHADOW_ALPHA: u8 = 160;

/// Множитель 6-битной палитры в 8-битную (`Palette::SIX_BIT_SCALE`).
pub const SIX_BIT_SCALE: u16 = 4;

/// Размер портрета `P_*` в игре (big image, `CargoMenu`/`BuildMenu`).
pub const PORTRAIT_WIDTH: u32 = 300;
/// Высота портрета `P_*`.
pub const PORTRAIT_HEIGHT: u32 = 240;

/// 8-битный цвет из 6-битного компонента палитры (`×4`, с насыщением).
#[inline]
pub fn six_bit(value: u8) -> u8 {
    (u16::from(value) * SIX_BIT_SCALE).min(255) as u8
}

/// Палитра окна (`FRAMEPIC`) → 256 цветов RGB.
///
/// В C++ системная палитра окна — палитра фоновой big image (`UiPalette` в
/// порте); портреты `P_*` и кадры FLC рисуются именно ей (`palette_from_image =
/// false`). Своя палитра `P_*` отличается от неё на единицы в младших битах,
/// поэтому картинка совпадает (проверено на всех 71 портрете).
pub fn window_palette(image: &BigImage) -> [[u8; 3]; 256] {
    let mut colors = [[0u8; 3]; 256];

    for (index, color) in colors.iter_mut().enumerate() {
        *color = image.color(index as u8);
    }

    colors
}

/// 6-битная палитра FLC → 8-битная таблица цветов.
pub fn flic_palette(palette: &[[u8; 3]; 256]) -> [[u8; 3]; 256] {
    let mut colors = [[0u8; 3]; 256];

    for (index, color) in colors.iter_mut().enumerate() {
        let entry = palette[index];
        *color = [six_bit(entry[0]), six_bit(entry[1]), six_bit(entry[2])];
    }

    colors
}

/// Один кадр обычного спрайта → RGBA-картинка `width × height`.
pub fn sprite_frame_image(frame: &SpriteFrame, palette: &Palette) -> RgbaImage {
    let width = u32::from(frame.width);
    let height = u32::from(frame.height);
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let Some(index) = frame.pixel(x, y) else {
                continue;
            };
            let [red, green, blue] = palette.rgb(index);

            out.set_pixel(x, y, [red, green, blue, 255]);
        }
    }

    out
}

/// Один кадр тени → RGBA-маска (чёрный, альфа [`SHADOW_ALPHA`]).
pub fn shadow_frame_image(frame: &ShadowFrame) -> RgbaImage {
    let width = u32::from(frame.width);
    let height = u32::from(frame.height);
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            if frame.darkened(x, y) {
                out.set_pixel(x, y, [0, 0, 0, SHADOW_ALPHA]);
            }
        }
    }

    out
}

/// Кадр FLC → RGBA по 8-битной таблице цветов (все пиксели непрозрачны).
pub fn flic_frame_image(frame: &FlicFrame, colors: &[[u8; 3]; 256]) -> RgbaImage {
    let width = frame.width as u32;
    let height = frame.height as u32;
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let index = frame.pixels[(y as usize) * frame.width + x as usize];
            let [red, green, blue] = colors[usize::from(index)];

            out.set_pixel(x, y, [red, green, blue, 255]);
        }
    }

    out
}

/// Big image (`P_*`) → RGBA по палитре окна; кадр полностью непрозрачен.
pub fn big_image_image(image: &BigImage, colors: &[[u8; 3]; 256]) -> RgbaImage {
    let width = u32::from(image.width as u16);
    let height = u32::from(image.height as u16);
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let Some(index) = image.pixel(x, y) else {
                continue;
            };
            let [red, green, blue] = colors[usize::from(index)];

            out.set_pixel(x, y, [red, green, blue, 255]);
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Лента кадров (как `max-assets/README.md`, §«Лента кадров»)
// ---------------------------------------------------------------------------

/// Общий якорь ленты: максимум положительных горячих точек по каждой оси.
fn anchor(frames: &[(u16, u16, i16, i16)]) -> (u32, u32) {
    let x = frames
        .iter()
        .map(|(_, _, hot, _)| u32::from((*hot).max(0) as u16))
        .max()
        .unwrap_or(0);
    let y = frames
        .iter()
        .map(|(_, _, _, hot)| u32::from((*hot).max(0) as u16))
        .max()
        .unwrap_or(0);

    (x, y)
}

/// Насколько ячейка шире/выше кадра из-за общего якоря (≥ 0).
fn extra(anchor: u32, hot: i16) -> u32 {
    (i64::from(anchor) - i64::from(hot)).max(0) as u32
}

/// Габариты ленты кадров: сумма ячеек по X, максимум высот по Y.
pub fn strip_dimensions(frames: &[(u16, u16, i16, i16)]) -> (u32, u32) {
    if frames.is_empty() {
        return (0, 0);
    }

    let (anchor_x, anchor_y) = anchor(frames);
    let width: u32 = frames
        .iter()
        .map(|(width, _, hot, _)| u32::from(*width) + extra(anchor_x, *hot))
        .sum();
    let height = frames
        .iter()
        .map(|(_, height, _, hot)| u32::from(*height) + extra(anchor_y, *hot))
        .max()
        .unwrap_or(0);

    (width, height)
}

/// Метаданные кадров спрайта для раскладки.
pub fn sprite_meta(frames: &[&SpriteFrame]) -> Vec<(u16, u16, i16, i16)> {
    frames
        .iter()
        .map(|frame| (frame.width, frame.height, frame.hotx, frame.hoty))
        .collect()
}

/// Лента кадров спрайта (RGBA, выравнивание по горячей точке).
pub fn sprite_strip(frames: &[&SpriteFrame], palette: &Palette) -> RgbaImage {
    let meta = sprite_meta(frames);
    let (width, height) = strip_dimensions(&meta);
    let mut out = RgbaImage::new(width, height);
    let (anchor_x, anchor_y) = anchor(&meta);
    let mut cell_x = 0u32;

    for (frame, (frame_width, _, hot, hot_y)) in frames.iter().zip(meta.iter()) {
        let origin_x = cell_x + extra(anchor_x, *hot);
        let origin_y = extra(anchor_y, *hot_y);

        for y in 0..u32::from(frame.height) {
            for x in 0..u32::from(frame.width) {
                let Some(index) = frame.pixel(x, y) else {
                    continue;
                };
                let [red, green, blue] = palette.rgb(index);

                out.set_pixel(origin_x + x, origin_y + y, [red, green, blue, 255]);
            }
        }

        cell_x += u32::from(*frame_width) + extra(anchor_x, *hot);
    }

    out
}

/// Fallback-портрет: первый кадр спрайта, увеличенный целым множителем и
/// вписанный в 300×240 на чёрном фоне (как `FillOrRestoreWindow(..., BLACK)`).
///
/// Множитель — наибольший целый, при котором кадр влезает в портрет
/// (`max(1, min(300 / w, 240 / h))`).
pub fn portrait_fallback(frame: &SpriteFrame, palette: &Palette) -> RgbaImage {
    let source = sprite_frame_image(frame, palette);
    let mut out = RgbaImage::new(PORTRAIT_WIDTH, PORTRAIT_HEIGHT);

    // Чёрный фон окна портрета.
    for pixel in out.rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[0, 0, 0, 255]);
    }

    let scale = (PORTRAIT_WIDTH / source.width.max(1))
        .min(PORTRAIT_HEIGHT / source.height.max(1))
        .max(1);
    let scaled_width = source.width * scale;
    let scaled_height = source.height * scale;
    let offset_x = (PORTRAIT_WIDTH - scaled_width) / 2;
    let offset_y = (PORTRAIT_HEIGHT - scaled_height) / 2;

    for y in 0..scaled_height {
        for x in 0..scaled_width {
            let Some(color) = source.pixel(x / scale, y / scale) else {
                continue;
            };

            if color[3] == 0 {
                continue;
            }

            out.set_pixel(offset_x + x, offset_y + y, color);
        }
    }

    out
}

/// Множитель fallback-портрета для кадра (для `unit.json`).
pub fn fallback_scale(frame: &SpriteFrame) -> u32 {
    (PORTRAIT_WIDTH / u32::from(frame.width).max(1))
        .min(PORTRAIT_HEIGHT / u32::from(frame.height).max(1))
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u16, height: u16, hotx: i16, hoty: i16) -> SpriteFrame {
        let mut pixels = vec![None; usize::from(width) * usize::from(height)];

        pixels[0] = Some(1);

        SpriteFrame {
            width,
            height,
            hotx,
            hoty,
            rows: Vec::new(),
            pixels,
        }
    }

    #[test]
    fn strip_packs_frames_by_hot_spot() {
        // Как в тесте max-resource-editor: ячейки 44+6=50 и 56, высота max(50,57).
        let (width, height) = strip_dimensions(&[(44, 45, 20, 23), (56, 57, 26, 28)]);

        assert_eq!(width, 50 + 56);
        assert_eq!(height, 57);
    }

    #[test]
    fn negative_hot_spot_extends_the_cell() {
        let (width, height) = strip_dimensions(&[(6, 8, -13, -28), (28, 25, 3, -16)]);

        assert_eq!(width, 22 + 28);
        assert_eq!(height, 41);
    }

    #[test]
    fn six_bit_scales_to_eight_bit_without_overflow() {
        assert_eq!(six_bit(0), 0);
        assert_eq!(six_bit(63), 252);
        assert_eq!(six_bit(64), 255);
        assert_eq!(six_bit(255), 255);
    }

    #[test]
    fn shadow_image_is_black_with_alpha() {
        let shadow = ShadowFrame {
            width: 2,
            height: 1,
            hotx: 0,
            hoty: 0,
            rows: Vec::new(),
            mask: vec![true, false],
        };
        let image = shadow_frame_image(&shadow);

        assert_eq!(image.pixel(0, 0), Some([0, 0, 0, SHADOW_ALPHA]));
        assert_eq!(image.pixel(1, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn fallback_portrait_fits_into_300x240() {
        let palette = Palette::from_color_pal(&[0u8; 768]).expect("палитра");
        let source = frame(44, 45, 20, 23);
        let portrait = portrait_fallback(&source, &palette);

        assert_eq!((portrait.width, portrait.height), (300, 240));
        assert_eq!(fallback_scale(&source), 5); // 44*5=220, 45*5=225 — влезает
    }

    #[test]
    fn sprite_strip_is_as_wide_as_the_sum_of_cells() {
        let palette = Palette::from_color_pal(&[0u8; 768]).expect("палитра");
        let frames = [frame(10, 4, 2, 2), frame(12, 6, 6, 3)];
        let strip = sprite_strip(&[&frames[0], &frames[1]], &palette);

        // Ячейки: 10 + (6-2) = 14 и 12 + 0 = 12.
        assert_eq!(strip.width, 26);
        assert_eq!(strip.height, 6);
    }
}
