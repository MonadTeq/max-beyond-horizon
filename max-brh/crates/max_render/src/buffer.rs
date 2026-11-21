//! RGBA-буфер кадра — единственный формат, в котором `max_render` отдаёт
//! готовую картинку наружу (SDL-слой только выводит его на экран).
//!
//! Буфер простой: `width * height` пикселей по 4 байта (RGBA8), row-major,
//! без выравнивания строк. Все операции — чистые функции над `&mut RgbaBuffer`,
//! поэтому отрисовка тестируется без окна и без SDL.

/// Байт на пиксель (RGBA8).
pub const BYTES_PER_PIXEL: usize = 4;

/// Цвет пикселя в RGBA8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    /// Красный канал.
    pub r: u8,
    /// Зелёный канал.
    pub g: u8,
    /// Синий канал.
    pub b: u8,
    /// Альфа: `0` — прозрачность, `255` — непрозрачный.
    pub a: u8,
}

impl Rgba {
    /// Полностью прозрачный пиксель (фон кадра).
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    /// Непрозрачный чёрный.
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// Непрозрачный белый.
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    /// Непрозрачный цвет из компонентов.
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Цвет из массива `[r, g, b]` (альфа — непрозрачная).
    #[inline]
    pub const fn from_rgb(rgb: [u8; 3]) -> Self {
        Self::rgb(rgb[0], rgb[1], rgb[2])
    }

    /// Цвет из массива `[r, g, b, a]`.
    #[inline]
    pub const fn from_rgba(rgba: [u8; 4]) -> Self {
        Self {
            r: rgba[0],
            g: rgba[1],
            b: rgba[2],
            a: rgba[3],
        }
    }

    /// Массив `[r, g, b, a]`.
    #[inline]
    pub const fn to_rgba(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Пиксель непрозрачен.
    #[inline]
    pub const fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// Пиксель полностью прозрачен.
    #[inline]
    pub const fn is_transparent(self) -> bool {
        self.a == 0
    }

    /// Затемнение по формуле таблиц яркости `world.cpp:513-530` (§6.3 spw.md):
    /// `c' = c * (level * 32) / 224`, `level` 0..=7 (7 — без изменений).
    ///
    /// Оригинал применяет LUT «индекс палитры -> индекс палитры», но здесь
    /// карта уже в RGBA, поэтому та же арифметика применяется к каналам.
    pub fn darken(self, level: u8) -> Self {
        let level = u32::from(level.min(7));
        let factor = level * 32;
        let scale = |value: u8| ((u32::from(value) * factor) / (7 * 32)) as u8;

        Self {
            r: scale(self.r),
            g: scale(self.g),
            b: scale(self.b),
            a: self.a,
        }
    }

    /// Линейное смешивание с `other`: `weight` — вес `other` в 0..=255.
    pub fn mix(self, other: Self, weight: u8) -> Self {
        let weight = u32::from(weight);
        let rest = 255 - weight;
        let blend = |a: u8, b: u8| ((u32::from(a) * rest + u32::from(b) * weight) / 255) as u8;

        Self {
            r: blend(self.r, other.r),
            g: blend(self.g, other.g),
            b: blend(self.b, other.b),
            a: blend(self.a, other.a),
        }
    }
}

/// Прямоугольная область в пикселях экрана/буфера: `(ulx, uly, width, height)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    /// Левый верхний угол по x.
    pub ulx: i32,
    /// Левый верхний угол по y.
    pub uly: i32,
    /// Ширина в пикселях.
    pub width: i32,
    /// Высота в пикселях.
    pub height: i32,
}

impl PixelRect {
    /// Прямоугольник по углу и размеру.
    #[inline]
    pub const fn new(ulx: i32, uly: i32, width: i32, height: i32) -> Self {
        Self {
            ulx,
            uly,
            width,
            height,
        }
    }

    /// Прямоугольник по двум углам.
    #[inline]
    pub const fn from_corners(ulx: i32, uly: i32, lrx: i32, lry: i32) -> Self {
        Self {
            ulx,
            uly,
            width: lrx - ulx,
            height: lry - uly,
        }
    }

    /// Правый нижний угол по x (исключающий).
    #[inline]
    pub const fn lrx(self) -> i32 {
        self.ulx + self.width
    }

    /// Правый нижний угол по y (исключающий).
    #[inline]
    pub const fn lry(self) -> i32 {
        self.uly + self.height
    }

    /// Прямоугольник пуст (нулевая или отрицательная площадь).
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// Точка внутри прямоугольника.
    #[inline]
    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.ulx && y >= self.uly && x < self.lrx() && y < self.lry()
    }

    /// Пересечение с другим прямоугольником (`None`, если пусто).
    pub fn intersect(self, other: Self) -> Option<Self> {
        let ulx = self.ulx.max(other.ulx);
        let uly = self.uly.max(other.uly);
        let lrx = self.lrx().min(other.lrx());
        let lry = self.lry().min(other.lry());

        if ulx >= lrx || uly >= lry {
            None
        } else {
            Some(Self::from_corners(ulx, uly, lrx, lry))
        }
    }
}

/// RGBA8-буфер кадра.
///
/// Пиксель `(x, y)` лежит по смещению `(y * width + x) * 4`; начало координат —
/// левый верхний угол. Операции с координатами вне буфера не паникуют:
/// [`RgbaBuffer::set`] возвращает `false`, [`RgbaBuffer::get`] — `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaBuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaBuffer {
    /// Буфер заданного размера, заполненный прозрачным цветом.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * BYTES_PER_PIXEL],
        }
    }

    /// Буфер заданного размера, заполненный цветом `color`.
    pub fn filled(width: u32, height: u32, color: Rgba) -> Self {
        let mut buffer = Self::new(width, height);

        buffer.fill(color);
        buffer
    }

    /// Буфер из готовых байтов; `None`, если длина не `width * height * 4`.
    pub fn from_pixels(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        if pixels.len() == width as usize * height as usize * BYTES_PER_PIXEL {
            Some(Self {
                width,
                height,
                pixels,
            })
        } else {
            None
        }
    }

    /// Ширина в пикселях.
    #[inline]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Высота в пикселях.
    #[inline]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Размер строки в байтах (`width * 4`).
    #[inline]
    pub const fn stride(&self) -> usize {
        self.width as usize * BYTES_PER_PIXEL
    }

    /// Число пикселей.
    #[inline]
    pub const fn len(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Буфер без пикселей.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Прямоугольник всего буфера.
    #[inline]
    pub const fn bounds(&self) -> PixelRect {
        PixelRect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// Байты кадра (`width * height * 4`), row-major.
    #[inline]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Байты кадра для изменения.
    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Байты кадра во владение (например, для `Texture::update` в SDL).
    #[inline]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// Цвет пикселя; `None` вне буфера.
    pub fn get(&self, x: i32, y: i32) -> Option<Rgba> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }

        let offset = (y as usize * self.width as usize + x as usize) * BYTES_PER_PIXEL;

        Some(Rgba::from_rgba([
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ]))
    }

    /// Устанавливает цвет пикселя; `false`, если точка вне буфера.
    pub fn set(&mut self, x: i32, y: i32, color: Rgba) -> bool {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return false;
        }

        let offset = (y as usize * self.width as usize + x as usize) * BYTES_PER_PIXEL;

        self.pixels[offset..offset + BYTES_PER_PIXEL].copy_from_slice(&color.to_rgba());

        true
    }

    /// Заливает весь буфер цветом.
    pub fn fill(&mut self, color: Rgba) {
        let rgba = color.to_rgba();

        for pixel in self.pixels.chunks_exact_mut(BYTES_PER_PIXEL) {
            pixel.copy_from_slice(&rgba);
        }
    }

    /// Заливает прямоугольник; возвращает число закрашенных пикселей.
    ///
    /// Область обрезается по границам буфера, поэтому частично видимый
    /// прямоугольник безопасен.
    pub fn fill_rect(&mut self, rect: PixelRect, color: Rgba) -> u32 {
        let Some(clipped) = rect.intersect(self.bounds()) else {
            return 0;
        };

        let mut painted = 0;

        for y in clipped.uly..clipped.lry() {
            for x in clipped.ulx..clipped.lrx() {
                if self.set(x, y, color) {
                    painted += 1;
                }
            }
        }

        painted
    }

    /// Меняет цвет уже нарисованного пикселя (для теней): вне буфера — игнор.
    ///
    /// Возвращает `true`, если пиксель был в буфере и изменён.
    pub fn modify(&mut self, x: i32, y: i32, change: impl FnOnce(Rgba) -> Rgba) -> bool {
        match self.get(x, y) {
            Some(color) => self.set(x, y, change(color)),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_stride_and_transparent_default() {
        let buffer = RgbaBuffer::new(3, 2);

        assert_eq!(buffer.width(), 3);
        assert_eq!(buffer.height(), 2);
        assert_eq!(buffer.stride(), 12);
        assert_eq!(buffer.len(), 6);
        assert_eq!(buffer.pixels().len(), 24);
        assert_eq!(buffer.bounds(), PixelRect::new(0, 0, 3, 2));
        assert!(buffer.pixels().iter().all(|&byte| byte == 0));
        assert_eq!(buffer.get(0, 0), Some(Rgba::TRANSPARENT));
    }

    #[test]
    fn set_get_and_out_of_bounds() {
        let mut buffer = RgbaBuffer::new(2, 2);
        let red = Rgba::rgb(255, 0, 0);

        assert!(buffer.set(1, 1, red));
        assert_eq!(buffer.get(1, 1), Some(red));
        assert!(!buffer.set(2, 1, red));
        assert!(!buffer.set(-1, 0, red));
        assert_eq!(buffer.get(2, 1), None);
        assert_eq!(buffer.get(-1, 0), None);

        // Смещение пикселя (1, 1) = (1*2 + 1) * 4.
        assert_eq!(&buffer.pixels()[12..16], &[255, 0, 0, 255]);
    }

    #[test]
    fn fill_rect_clips_to_buffer() {
        let mut buffer = RgbaBuffer::new(4, 4);
        let blue = Rgba::rgb(0, 0, 255);

        let painted = buffer.fill_rect(PixelRect::new(-2, -2, 4, 4), blue);

        assert_eq!(painted, 4);
        assert_eq!(buffer.get(0, 0), Some(blue));
        assert_eq!(buffer.get(1, 1), Some(blue));
        assert_eq!(buffer.get(2, 2), Some(Rgba::TRANSPARENT));

        assert_eq!(buffer.fill_rect(PixelRect::new(10, 10, 2, 2), blue), 0);
        assert_eq!(buffer.fill_rect(PixelRect::new(0, 0, 0, 0), blue), 0);
    }

    #[test]
    fn rect_geometry() {
        let rect = PixelRect::new(10, 20, 30, 40);

        assert_eq!(rect.lrx(), 40);
        assert_eq!(rect.lry(), 60);
        assert!(rect.contains(10, 20));
        assert!(!rect.contains(40, 20));
        assert!(!rect.is_empty());
        assert!(PixelRect::new(0, 0, 0, 5).is_empty());

        let clipped = rect.intersect(PixelRect::new(0, 0, 20, 30)).unwrap();

        assert_eq!(clipped, PixelRect::from_corners(10, 20, 20, 30));
        assert!(rect.intersect(PixelRect::new(0, 0, 5, 5)).is_none());
    }

    #[test]
    fn darken_and_mix_match_the_documented_formula() {
        let grey = Rgba::rgb(224, 112, 0);

        // level 7 (full brightness) ничего не меняет.
        assert_eq!(grey.darken(7), grey);
        // level 5 = 160/224 = 5/7 -> 160, 80, 0.
        assert_eq!(grey.darken(5), Rgba::rgb(160, 80, 0));
        // level 0 — чёрный.
        assert_eq!(grey.darken(0), Rgba::rgb(0, 0, 0));
        // Уровень выше 7 клампится.
        assert_eq!(grey.darken(200), grey);
        // Альфа не меняется.
        assert_eq!(Rgba::from_rgba([10, 20, 30, 0]).darken(3).a, 0);

        assert_eq!(Rgba::BLACK.mix(Rgba::WHITE, 255), Rgba::WHITE);
        assert_eq!(Rgba::BLACK.mix(Rgba::WHITE, 0), Rgba::BLACK);
        assert_eq!(Rgba::BLACK.mix(Rgba::WHITE, 128), Rgba::rgb(128, 128, 128));
    }

    #[test]
    fn modify_changes_existing_pixels_only() {
        let mut buffer = RgbaBuffer::filled(2, 2, Rgba::WHITE);

        assert!(buffer.modify(0, 0, |color| color.darken(5)));
        assert_eq!(buffer.get(0, 0), Some(Rgba::rgb(182, 182, 182)));
        assert!(!buffer.modify(5, 5, |color| color.darken(5)));
        assert_eq!(buffer.get(1, 1), Some(Rgba::WHITE));
    }
}
