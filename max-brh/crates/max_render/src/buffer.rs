//! The frame RGBA buffer — the only format in which `max_render` hands
//! the finished picture outside (the SDL layer only outputs it to the screen).
//!
//! The buffer is simple: `width * height` pixels of 4 bytes (RGBA8), row-major,
//! without row alignment. All operations are pure functions over `&mut RgbaBuffer`,
//! so rendering is tested without a window and without SDL.

/// Bytes per pixel (RGBA8).
pub const BYTES_PER_PIXEL: usize = 4;

/// Pixel color in RGBA8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha: `0` — transparent, `255` — opaque.
    pub a: u8,
}

impl Rgba {
    /// Fully transparent pixel (frame background).
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    /// Opaque black.
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// Opaque white.
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    /// Opaque color from components.
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Color from the `[r, g, b]` array (alpha is opaque).
    #[inline]
    pub const fn from_rgb(rgb: [u8; 3]) -> Self {
        Self::rgb(rgb[0], rgb[1], rgb[2])
    }

    /// Color from the `[r, g, b, a]` array.
    #[inline]
    pub const fn from_rgba(rgba: [u8; 4]) -> Self {
        Self {
            r: rgba[0],
            g: rgba[1],
            b: rgba[2],
            a: rgba[3],
        }
    }

    /// The `[r, g, b, a]` array.
    #[inline]
    pub const fn to_rgba(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// The pixel is opaque.
    #[inline]
    pub const fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// The pixel is fully transparent.
    #[inline]
    pub const fn is_transparent(self) -> bool {
        self.a == 0
    }

    /// Darkening by the formula of the brightness tables `world.cpp:513-530` (§6.3 spw.md):
    /// `c' = c * (level * 32) / 224`, `level` 0..=7 (7 — unchanged).
    ///
    /// The original applies the "palette index -> palette index" LUT, but here
    /// the map is already in RGBA, so the same arithmetic is applied to the channels.
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

    /// Linear blending with `other`: `weight` is the weight of `other` in 0..=255.
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

/// Rectangular area in screen/buffer pixels: `(ulx, uly, width, height)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    /// Top-left corner along x.
    pub ulx: i32,
    /// Top-left corner along y.
    pub uly: i32,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}

impl PixelRect {
    /// Rectangle from a corner and a size.
    #[inline]
    pub const fn new(ulx: i32, uly: i32, width: i32, height: i32) -> Self {
        Self {
            ulx,
            uly,
            width,
            height,
        }
    }

    /// Rectangle from two corners.
    #[inline]
    pub const fn from_corners(ulx: i32, uly: i32, lrx: i32, lry: i32) -> Self {
        Self {
            ulx,
            uly,
            width: lrx - ulx,
            height: lry - uly,
        }
    }

    /// Bottom-right corner along x (exclusive).
    #[inline]
    pub const fn lrx(self) -> i32 {
        self.ulx + self.width
    }

    /// Bottom-right corner along y (exclusive).
    #[inline]
    pub const fn lry(self) -> i32 {
        self.uly + self.height
    }

    /// The rectangle is empty (zero or negative area).
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// The point is inside the rectangle.
    #[inline]
    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.ulx && y >= self.uly && x < self.lrx() && y < self.lry()
    }

    /// Intersection with another rectangle (`None` if empty).
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

/// Frame buffer in RGBA8.
///
/// Pixel `(x, y)` lies at the offset `(y * width + x) * 4`; the origin is the
/// top-left corner. Operations with coordinates outside the buffer do not panic:
/// [`RgbaBuffer::set`] returns `false`, [`RgbaBuffer::get`] returns `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaBuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaBuffer {
    /// Buffer of the given size filled with the transparent color.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * BYTES_PER_PIXEL],
        }
    }

    /// Buffer of the given size filled with the color `color`.
    pub fn filled(width: u32, height: u32, color: Rgba) -> Self {
        let mut buffer = Self::new(width, height);

        buffer.fill(color);
        buffer
    }

    /// Buffer from ready bytes; `None` if the length is not `width * height * 4`.
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

    /// Width in pixels.
    #[inline]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[inline]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Row size in bytes (`width * 4`).
    #[inline]
    pub const fn stride(&self) -> usize {
        self.width as usize * BYTES_PER_PIXEL
    }

    /// Number of pixels.
    #[inline]
    pub const fn len(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Buffer without pixels.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Rectangle of the whole buffer.
    #[inline]
    pub const fn bounds(&self) -> PixelRect {
        PixelRect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// Frame bytes (`width * height * 4`), row-major.
    #[inline]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Frame bytes for modification.
    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Frame bytes by value (e.g. for `Texture::update` in SDL).
    #[inline]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// Pixel color; `None` outside the buffer.
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

    /// Sets the pixel color; `false` if the point is outside the buffer.
    pub fn set(&mut self, x: i32, y: i32, color: Rgba) -> bool {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return false;
        }

        let offset = (y as usize * self.width as usize + x as usize) * BYTES_PER_PIXEL;

        self.pixels[offset..offset + BYTES_PER_PIXEL].copy_from_slice(&color.to_rgba());

        true
    }

    /// Fills the whole buffer with a color.
    pub fn fill(&mut self, color: Rgba) {
        let rgba = color.to_rgba();

        for pixel in self.pixels.chunks_exact_mut(BYTES_PER_PIXEL) {
            pixel.copy_from_slice(&rgba);
        }
    }

    /// Fills a rectangle; returns the number of filled pixels.
    ///
    /// The area is clipped by the buffer bounds, so a partially visible
    /// rectangle is safe.
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

    /// Changes the color of an already drawn pixel (for shadows): outside the buffer it is ignored.
    ///
    /// Returns `true` if the pixel was inside the buffer and was modified.
    pub fn modify(&mut self, x: i32, y: i32, change: impl FnOnce(Rgba) -> Rgba) -> bool {
        match self.get(x, y) {
            Some(color) => self.set(x, y, change(color)),
            None => false,
        }
    }
}

#[cfg(test)]
#[path = "buffer_tests.rs"]
mod tests;
