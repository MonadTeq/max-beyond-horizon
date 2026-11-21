//! Растеризация ресурсов в RGBA: палитра, прозрачность, ленты кадров.
//!
//! Соглашения экспорта (подробно — в `max-brh-assets/README.md`):
//! * simple/sprite/cursor → `COLOR.PAL` (компоненты уже умножены на 4);
//! * big image → собственная палитра из заголовка (8-битные компоненты);
//! * прозрачные индексы (поле `transparent`) → альфа 0;
//! * тень — маска: затемняемые пиксели чёрные с альфой 160, остальные
//!   прозрачные;
//! * кадры спрайта/курсора/тени укладываются в горизонтальную ленту по
//!   горячей точке (`hot_x/hot_y`), поэтому анимация не «прыгает».

use anyhow::{anyhow, Result};
use max_assets::cursor::CursorDescriptor;
use max_assets::image::{BigImage, SimpleImage};
use max_assets::res::ResArchive;
use max_assets::sprite::{decode_shadow, decode_sprite, ShadowFrame, SpriteFrame};
use max_render::palette::Palette;

use crate::model::{Kind, ResourceInfo};
use crate::png::RgbaImage;

/// Цвет пикселя.
#[inline]
fn rgba(rgb: [u8; 3], alpha: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
}

/// Общий якорь ленты: максимум положительных горячих точек.
fn anchor(frames: &[(u16, u16, i16, i16)]) -> (u32, u32) {
    let x = frames
        .iter()
        .map(|(_, _, hot, _)| (*hot).max(0) as u32)
        .max()
        .unwrap_or(0);
    let y = frames
        .iter()
        .map(|(_, _, _, hot)| (*hot).max(0) as u32)
        .max()
        .unwrap_or(0);

    (x, y)
}

/// Насколько ячейка шире/выше кадра из-за общего якоря.
fn extra(anchor: u32, hot: i16) -> u32 {
    (i64::from(anchor) - i64::from(hot)).max(0) as u32
}

/// Габариты ленты кадров.
///
/// Каждый кадр занимает ячейку `width + anchor_x - hot_x`; общая ширина —
/// сумма ячеек, высота — максимум `anchor_y - hot_y + height`. Отрицательные
/// горячие точки (`BLDEXPLD`: `hotx = -13`) увеличивают ячейку и не обрезаются.
pub fn strip_dimensions(frames: &[(u16, u16, i16, i16)]) -> (u32, u32) {
    if frames.is_empty() {
        return (0, 0);
    }

    let (anchor_x, anchor_y) = anchor(frames);
    let width: u32 = frames
        .iter()
        .map(|(width, _, hot, _)| *width as u32 + extra(anchor_x, *hot))
        .sum();
    let height = frames
        .iter()
        .map(|(_, height, _, hot)| *height as u32 + extra(anchor_y, *hot))
        .max()
        .unwrap_or(0);

    (width, height)
}

/// Габариты ленты кадров конкретного спрайта/тени.
pub fn sprite_strip_dimensions(frames: &[SpriteFrame]) -> (u32, u32) {
    let meta: Vec<_> = frames
        .iter()
        .map(|frame| (frame.width, frame.height, frame.hotx, frame.hoty))
        .collect();

    strip_dimensions(&meta)
}

/// Габариты ленты кадров тени.
pub fn shadow_strip_dimensions(frames: &[ShadowFrame]) -> (u32, u32) {
    let meta: Vec<_> = frames
        .iter()
        .map(|frame| (frame.width, frame.height, frame.hotx, frame.hoty))
        .collect();

    strip_dimensions(&meta)
}

/// Раскладка кадра `index`: `(cell_x, origin_x, origin_y)`.
fn layout(frames: &[(u16, u16, i16, i16)], index: usize) -> (u32, u32, u32) {
    let (anchor_x, anchor_y) = anchor(frames);
    let mut cell_x = 0u32;

    for (current, (width, _, hot, hot_y)) in frames.iter().enumerate() {
        let cell_width = *width as u32 + extra(anchor_x, *hot);

        if current == index {
            return (
                cell_x,
                cell_x + extra(anchor_x, *hot),
                extra(anchor_y, *hot_y),
            );
        }

        cell_x += cell_width;
    }

    (0, 0, 0)
}

/// Simple image (`ImageSimpleHeader`) → RGBA по `COLOR.PAL`.
pub fn simple_image(image: &SimpleImage, palette: &Palette) -> RgbaImage {
    let width = u32::from(image.width as u16);
    let height = u32::from(image.height as u16);
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let Some(index) = image.pixel(x, y) else {
                continue;
            };
            let alpha = if index == image.transparent { 0 } else { 255 };
            out.set_pixel(x, y, rgba(palette.rgb(index), alpha));
        }
    }

    out
}

/// Big image (`ImageBigHeader`) → RGBA по собственной палитре.
pub fn big_image(image: &BigImage) -> RgbaImage {
    let width = u32::from(image.width as u16);
    let height = u32::from(image.height as u16);
    let mut out = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let Some(index) = image.pixel(x, y) else {
                continue;
            };
            let alpha = if index == image.transparent { 0 } else { 255 };
            out.set_pixel(x, y, rgba(image.color(index), alpha));
        }
    }

    out
}

/// Мультикадровый спрайт → горизонтальная лента RGBA по `COLOR.PAL`.
pub fn sprite_sheet(frames: &[SpriteFrame], palette: &Palette) -> RgbaImage {
    let meta: Vec<_> = frames
        .iter()
        .map(|frame| (frame.width, frame.height, frame.hotx, frame.hoty))
        .collect();
    let (width, height) = strip_dimensions(&meta);
    let mut out = RgbaImage::new(width, height);

    for (index, frame) in frames.iter().enumerate() {
        let (_, origin_x, origin_y) = layout(&meta, index);

        for y in 0..u32::from(frame.height) {
            for x in 0..u32::from(frame.width) {
                let Some(pixel) = frame.pixel(x, y) else {
                    continue;
                };
                let color = palette.rgb(pixel);
                out.set_pixel(origin_x + x, origin_y + y, rgba(color, 255));
            }
        }
    }

    out
}

/// Мультикадровая тень → горизонтальная лента: чёрный с альфой 160.
pub fn shadow_sheet(frames: &[ShadowFrame]) -> RgbaImage {
    let meta: Vec<_> = frames
        .iter()
        .map(|frame| (frame.width, frame.height, frame.hotx, frame.hoty))
        .collect();
    let (width, height) = strip_dimensions(&meta);
    let mut out = RgbaImage::new(width, height);

    for (index, frame) in frames.iter().enumerate() {
        let (_, origin_x, origin_y) = layout(&meta, index);

        for y in 0..u32::from(frame.height) {
            for x in 0..u32::from(frame.width) {
                if frame.darkened(x, y) {
                    out.set_pixel(origin_x + x, origin_y + y, [0, 0, 0, 160]);
                }
            }
        }
    }

    out
}

/// Курсор → горизонтальная лента RGBA по `COLOR.PAL`.
pub fn cursor_strip(cursor: &CursorDescriptor, palette: &Palette) -> RgbaImage {
    let frame_width = u32::try_from(cursor.width).unwrap_or(0);
    let frame_height = u32::try_from(cursor.height).unwrap_or(0);
    let frames = usize::from(cursor.frame_count);
    let meta: Vec<_> = (0..frames)
        .map(|_| {
            (
                cursor.width as u16,
                cursor.height as u16,
                cursor.hot_x as i16,
                cursor.hot_y as i16,
            )
        })
        .collect();
    let (width, height) = strip_dimensions(&meta);
    let mut out = RgbaImage::new(width, height);

    for index in 0..frames {
        let Some(pixels) = cursor.frame(index) else {
            continue;
        };
        let (_, origin_x, origin_y) = layout(&meta, index);

        for y in 0..frame_height {
            for x in 0..frame_width {
                let pixel = pixels[(y * frame_width + x) as usize];
                if pixel == cursor.transparent {
                    continue;
                }
                out.set_pixel(origin_x + x, origin_y + y, rgba(palette.rgb(pixel), 255));
            }
        }
    }

    out
}

/// Декодирует и растеризует запись архива.
pub fn render_resource(
    info: &ResourceInfo,
    archive: &ResArchive,
    palette: &Palette,
) -> Result<RgbaImage> {
    let bytes = archive
        .read(&info.name)
        .map_err(|error| anyhow!("{}: {error}", info.name))?;

    match info.kind {
        Kind::Simple => {
            let image =
                SimpleImage::decode(bytes).map_err(|error| anyhow!("{}: {error}", info.name))?;
            Ok(simple_image(&image, palette))
        }
        Kind::Big => {
            let image =
                BigImage::decode(bytes).map_err(|error| anyhow!("{}: {error}", info.name))?;
            Ok(big_image(&image))
        }
        Kind::Sprite => {
            let sheet = decode_sprite(bytes).map_err(|error| anyhow!("{}: {error}", info.name))?;
            Ok(sprite_sheet(&sheet.frames, palette))
        }
        Kind::Shadow => {
            let sheet = decode_shadow(bytes).map_err(|error| anyhow!("{}: {error}", info.name))?;
            Ok(shadow_sheet(&sheet.frames))
        }
        Kind::Cursor => {
            let cursor = CursorDescriptor::decode(bytes)
                .map_err(|error| anyhow!("{}: {error}", info.name))?;
            Ok(cursor_strip(&cursor, palette))
        }
        Kind::NonImage => Err(anyhow!("{} — не изображение", info.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_packs_frames_by_hot_spot() {
        // Два TANK-подобных кадра: ячейки 44+6=50 и 56, высота max(50,57)=57.
        let frames = [(44, 45, 20, 23), (56, 57, 26, 28)];
        let (width, height) = strip_dimensions(&frames);

        assert_eq!(width, 50 + 56);
        assert_eq!(height, 57);

        let (cell, origin_x, origin_y) = layout(&frames, 0);
        assert_eq!((cell, origin_x, origin_y), (0, 6, 5));
        let (cell, origin_x, origin_y) = layout(&frames, 1);
        assert_eq!((cell, origin_x, origin_y), (50, 50, 0));
    }

    #[test]
    fn negative_hot_spot_extends_the_cell() {
        let (width, height) = strip_dimensions(&[(6, 8, -13, -28), (28, 25, 3, -16)]);

        assert_eq!(width, 22 + 28);
        assert_eq!(height, 41);
    }
}
