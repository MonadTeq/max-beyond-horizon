//! Хром окна игры 1:1: `FRAMEPIC` (фон 640×480 со своей палитрой) и половинки
//! левой панели `PANELTOP`/`PANELBTM` (171×239/241, системная палитра).
//!
//! Оригинал (`ResourceManager_InitInGameAssets`) делает
//! `WindowManager_LoadBigImage(FRAMEPIC, window, …, palette_from_image=true)` —
//! то есть весь интерфейс партии (панели, кнопки, рамки) уже нарисован в
//! `FRAMEPIC`, а карта выводится в прямоугольник [`crate::layout::MAIN_MAP`].
//! Здесь это воспроизведено: [`Chrome::draw`] заливает фон, [`blit_map`]
//! кладёт карту в область карты.

use max_assets::image::{BigImage, SimpleImage};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout;

/// Загруженный хром окна.
#[derive(Debug, Clone)]
pub struct Chrome {
    /// Фон окна 640×480 (своя палитра, RLE).
    pub frame: BigImage,
    /// Верхняя половина левой панели (для отчёта/проверок).
    pub panel_top: Option<SimpleImage>,
    /// Нижняя половина левой панели.
    pub panel_bottom: Option<SimpleImage>,
}

impl Chrome {
    /// Собирает хром из архива `MAX.RES`.
    ///
    /// `FRAMEPIC` обязателен (без него нет 1:1 окна), панели — опционально:
    /// они нужны только для экранов высадки, в партии всё уже в `FRAMEPIC`.
    pub fn load(max_res: &max_assets::ResArchive) -> Option<Self> {
        let frame = max_res
            .read("FRAMEPIC")
            .ok()
            .and_then(|bytes| BigImage::decode(bytes).ok())?;

        let read_simple = |name: &str| {
            max_res
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };

        Some(Self {
            frame,
            panel_top: read_simple("PANELTOP"),
            panel_bottom: read_simple("PANELBTM"),
        })
    }

    /// `true`, если фон окна пригоден для 1:1 (640×480).
    pub fn is_ready(&self) -> bool {
        self.frame.width as i32 == layout::WINDOW_WIDTH
            && self.frame.height as i32 == layout::WINDOW_HEIGHT
    }

    /// Заливает `dst` фоном окна (палитра берётся из самой картинки).
    ///
    /// Возвращает число закрашенных пикселей.
    pub fn draw(&self, dst: &mut RgbaBuffer) -> u32 {
        if !self.is_ready() {
            return 0;
        }

        let mut painted = 0;

        for y in 0..self.frame.height as u32 {
            for x in 0..self.frame.width as u32 {
                let Some([r, g, b]) = self.frame.rgb(x, y) else {
                    continue;
                };

                if dst.set(x as i32, y as i32, Rgba::rgb(r, g, b)) {
                    painted += 1;
                }
            }
        }

        painted
    }

    /// Пиксель фона (для тестов/диагностики).
    pub fn frame_rgb(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        self.frame.rgb(x, y)
    }
}

/// Копирует буфер карты в область [`layout::MAIN_MAP`] окна.
///
/// Карта рисуется в собственный буфер размером 448×448 (7×7 клеток), поэтому
/// она автоматически обрезается краями области — как `WINDOW_MAIN_MAP` в
/// оригинале. Возвращает число скопированных пикселей.
pub fn blit_map(dst: &mut RgbaBuffer, map: &RgbaBuffer) -> u32 {
    let area = layout::MAIN_MAP;
    let mut painted = 0;

    for y in 0..area.height.min(map.height() as i32) {
        for x in 0..area.width.min(map.width() as i32) {
            let Some(color) = map.get(x, y) else {
                continue;
            };

            if dst.set(area.ulx + x, area.uly + y, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// Прямоугольник области карты (для перевода координат мыши).
pub fn map_area() -> PixelRect {
    layout::MAIN_MAP
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Мини-кадр 640×480: по одному цвету, чтобы проверить блит.
    fn fake_frame() -> BigImage {
        let width = max_render::layout::WINDOW_WIDTH as i16;
        let height = max_render::layout::WINDOW_HEIGHT as i16;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());

        let mut palette = [0u8; max_assets::image::PALETTE_BYTES];
        palette[3..6].copy_from_slice(&[1, 2, 3]);
        bytes.extend_from_slice(&palette);

        // Каждая строка: литерал из width байт индекса 1 (RLE с n > 0).
        let row = vec![1u8; width as usize];
        for _ in 0..height {
            bytes.extend_from_slice(&(width).to_le_bytes());
            bytes.extend_from_slice(&row);
        }

        BigImage::decode(&bytes).expect("мини-кадр должен разбираться")
    }

    #[test]
    fn chrome_draws_frame_and_reports_ready() {
        let chrome = Chrome {
            frame: fake_frame(),
            panel_top: None,
            panel_bottom: None,
        };
        let mut frame = RgbaBuffer::new(640, 480);

        assert!(chrome.is_ready());
        assert_eq!(chrome.draw(&mut frame), 640 * 480);
        assert_eq!(frame.get(0, 0), Some(Rgba::rgb(1, 2, 3)));
        assert_eq!(frame.get(639, 479), Some(Rgba::rgb(1, 2, 3)));
    }

    #[test]
    fn blit_map_lands_at_main_map_rect() {
        let mut frame = RgbaBuffer::new(640, 480);
        let mut map = RgbaBuffer::new(448, 448);
        map.set(0, 0, Rgba::rgb(9, 9, 9));
        map.set(447, 447, Rgba::rgb(7, 7, 7));

        assert_eq!(blit_map(&mut frame, &map), 448 * 448);
        assert_eq!(
            frame.get(layout::MAIN_MAP.ulx, layout::MAIN_MAP.uly),
            Some(Rgba::rgb(9, 9, 9))
        );
        assert_eq!(
            frame.get(layout::MAIN_MAP.lrx() - 1, layout::MAIN_MAP.lry() - 1),
            Some(Rgba::rgb(7, 7, 7))
        );
        assert_ne!(
            frame.get(layout::MAIN_MAP.ulx - 1, layout::MAIN_MAP.uly),
            Some(Rgba::rgb(9, 9, 9)),
            "за границей области карты буфер не тронут"
        );
    }

    #[test]
    fn blit_map_clips_smaller_buffer() {
        let mut frame = RgbaBuffer::new(640, 480);
        let mut map = RgbaBuffer::new(32, 16);
        map.set(31, 15, Rgba::rgb(5, 5, 5));

        assert_eq!(blit_map(&mut frame, &map), 32 * 16);
        assert_eq!(
            frame.get(layout::MAIN_MAP.ulx + 31, layout::MAIN_MAP.uly + 15),
            Some(Rgba::rgb(5, 5, 5))
        );
    }
}
