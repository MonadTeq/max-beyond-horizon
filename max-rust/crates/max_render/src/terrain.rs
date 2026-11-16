//! Настоящие тайлы поверхности из `.WRL` (то, что рисует оригинал).
//!
//! В `max_render::map_render` тайл — процедурная заглушка: базовый цвет
//! поверхности + шахматный узор (это видно как «непонятно что» вместо земли).
//! Оригинал (и C++-порт, `Gfx_DecodeMapTile`) берёт **пиксели тайла** из карты:
//! `.WRL` хранит `tile_ids: u16[W*H]`, `tile_count` и буфер `tile_count × 64×64`
//! индексов палитры плюс саму палитру 256×RGB — всё это уже разбирает
//! [`max_assets::wrl::WrlMap`], но рендер его не использовал.
//!
//! Здесь — [`TerrainAtlas`] (палитра + тайлы + карта id) и блит видимой части
//! карты в [`RgbaBuffer`] с учётом зума/камеры [`Viewport`]. Масштабирование —
//! nearest-neighbour, как `Gfx_ZoomLevel` в оригинале.
//!
//! Палитра `.WRL` может быть 6-битной (VGA, значения `0..=63`) — такие каналы
//! растягиваются на весь диапазон, иначе карта выглядела бы почти чёрной.

use max_assets::wrl::{WrlMap, PALETTE_COLORS, PALETTE_STRIDE, TILE_PIXELS, TILE_SIZE};

use crate::buffer::{Rgba, RgbaBuffer};
use crate::viewport::Viewport;

/// Палитра `.WRL` в RGBA (индекс → цвет).
pub type PaletteRgba = [[u8; 4]; PALETTE_COLORS];

/// Тайлы поверхности планеты: палитра + пиксели тайлов + карта `tile_id`.
#[derive(Clone, PartialEq, Eq)]
pub struct TerrainAtlas {
    palette: PaletteRgba,
    /// `tile_count × TILE_PIXELS` индексов палитры (по 1 байту на пиксель).
    tiles: Vec<u8>,
    /// `W*H` идентификаторов тайлов (row-major).
    tile_ids: Vec<u16>,
    tile_count: u16,
    width: i16,
    height: i16,
}

impl TerrainAtlas {
    /// Собирает атлас из разобранной карты `.WRL`.
    pub fn from_wrl(wrl: &WrlMap) -> Self {
        Self {
            palette: palette_from_wrl(wrl.palette()),
            tiles: wrl.tiles().to_vec(),
            tile_ids: wrl.tile_map().to_vec(),
            tile_count: wrl.tile_count(),
            width: wrl.width(),
            height: wrl.height(),
        }
    }

    /// Палитра атласа.
    pub fn palette(&self) -> &PaletteRgba {
        &self.palette
    }

    /// Размер карты в клетках.
    pub fn size(&self) -> (i16, i16) {
        (self.width, self.height)
    }

    /// Число уникальных тайлов.
    pub fn tile_count(&self) -> u16 {
        self.tile_count
    }

    /// `true`, если тайлов нет (рисовать нечего).
    pub fn is_empty(&self) -> bool {
        self.tile_count == 0 || self.tiles.is_empty() || self.tile_ids.is_empty()
    }

    /// Пиксель тайла по координатам внутри тайла (0..64).
    pub fn tile_pixel(&self, tile_id: u16, x: u32, y: u32) -> Option<Rgba> {
        if tile_id >= self.tile_count || x as usize >= TILE_SIZE || y as usize >= TILE_SIZE {
            return None;
        }

        let index = tile_id as usize * TILE_PIXELS + y as usize * TILE_SIZE + x as usize;
        let color = *self.tiles.get(index)?;
        let entry = self.palette[color as usize];

        Some(Rgba::rgb(entry[0], entry[1], entry[2]))
    }

    /// `tile_id` клетки карты.
    pub fn tile_id_at(&self, grid_x: i16, grid_y: i16) -> Option<u16> {
        if grid_x < 0 || grid_y < 0 || grid_x >= self.width || grid_y >= self.height {
            return None;
        }

        let index = grid_y as usize * self.width as usize + grid_x as usize;

        self.tile_ids.get(index).copied()
    }

    /// Рисует видимые клетки карты в `dst`; возвращает число клеток.
    pub fn render_into(&self, dst: &mut RgbaBuffer, viewport: &Viewport) -> u32 {
        if self.is_empty() {
            return 0;
        }

        let Some(range) = viewport.visible_tiles() else {
            return 0;
        };

        let mut painted = 0;

        for (grid_x, grid_y) in range.cells() {
            let rect = viewport.tile_rect(grid_x, grid_y);

            if self.render_tile_into(dst, (grid_x, grid_y), rect) > 0 {
                painted += 1;
            }
        }

        painted
    }

    /// Рисует одну клетку в прямоугольник `rect` (учитывает зум растяжением).
    pub fn render_tile_into(
        &self,
        dst: &mut RgbaBuffer,
        grid: (i16, i16),
        rect: crate::buffer::PixelRect,
    ) -> u32 {
        if rect.width <= 0 || rect.height <= 0 || rect.is_empty() {
            return 0;
        }

        let Some(tile_id) = self.tile_id_at(grid.0, grid.1) else {
            return 0;
        };

        if tile_id >= self.tile_count {
            return 0;
        }

        let base = tile_id as usize * TILE_PIXELS;
        let Some(pixels) = self.tiles.get(base..base + TILE_PIXELS) else {
            return 0;
        };

        // Карты координат пикселей считаем один раз на клетку: `rect` может быть
        // меньше/больше 64 (зум), а внутри клетки масштаб одинаковый.
        let rows: Vec<usize> = (0..rect.height as usize)
            .map(|local_y| (local_y * TILE_SIZE / rect.height as usize) * TILE_SIZE)
            .collect();
        let columns: Vec<usize> = (0..rect.width as usize)
            .map(|local_x| local_x * TILE_SIZE / rect.width as usize)
            .collect();

        let mut painted = 0;

        for (local_y, row) in rows.iter().enumerate() {
            for (local_x, column) in columns.iter().enumerate() {
                let color_index = pixels[row + column] as usize;
                let entry = self.palette[color_index];

                if dst.set(
                    rect.ulx + local_x as i32,
                    rect.uly + local_y as i32,
                    Rgba::rgb(entry[0], entry[1], entry[2]),
                ) {
                    painted += 1;
                }
            }
        }

        painted
    }

    /// Рисует клетку выборкой **ровно как `Gfx_DecodeMapTile`** оригинала
    /// (`gfx.cpp:125-210`) — это нужно для экрана высадки (зум 4), где C++
    /// сводит 64×64 тайл к 4×4 пикселям.
    ///
    /// Отличие от [`TerrainAtlas::render_tile_into`]: там каждый экранный
    /// пиксель берёт начало равного блока (`x * 64 / width`), а оригинал
    /// растягивает тайл по краям:
    ///
    /// ```text
    /// factor = ((64 - 1) << 16) / (size - 1) + 8;
    /// source = (i * factor) >> 16;          // size = 4 -> 0, 21, 42, 63
    /// ```
    ///
    /// При `size == 64` обе формулы дают тождественную выборку.
    pub fn render_tile_cpp_into(
        &self,
        dst: &mut RgbaBuffer,
        grid: (i16, i16),
        rect: crate::buffer::PixelRect,
    ) -> u32 {
        if rect.width <= 0 || rect.height <= 0 || rect.is_empty() {
            return 0;
        }

        let Some(tile_id) = self.tile_id_at(grid.0, grid.1) else {
            return 0;
        };

        if tile_id >= self.tile_count {
            return 0;
        }

        let base = tile_id as usize * TILE_PIXELS;
        let Some(pixels) = self.tiles.get(base..base + TILE_PIXELS) else {
            return 0;
        };

        let columns = cpp_tile_sample_offsets(rect.width);
        let rows: Vec<usize> = cpp_tile_sample_offsets(rect.height)
            .into_iter()
            .map(|row| row * TILE_SIZE)
            .collect();
        let mut painted = 0;

        for (local_y, row) in rows.iter().enumerate() {
            for (local_x, column) in columns.iter().enumerate() {
                let color_index = pixels[row + column] as usize;
                let entry = self.palette[color_index];

                if dst.set(
                    rect.ulx + local_x as i32,
                    rect.uly + local_y as i32,
                    Rgba::rgb(entry[0], entry[1], entry[2]),
                ) {
                    painted += 1;
                }
            }
        }

        painted
    }
}

/// Смещения исходных пикселей тайла для `size` экранных пикселей —
/// дословно `map_tile_zoom_factor` из `Gfx_DecodeMapTile` (`gfx.cpp:128`).
///
/// `size == 1` — единственный пиксель тайла (C++ делил бы на ноль).
pub fn cpp_tile_sample_offsets(size: i32) -> Vec<usize> {
    let size = size.max(1) as usize;

    if size == 1 {
        return vec![0];
    }

    let factor = (((TILE_SIZE as i32 - 1) << 16) / (size as i32 - 1) + 8) as u32;

    (0..size)
        .map(|index| ((index as u32 * factor) >> 16) as usize)
        .collect()
}

impl std::fmt::Debug for TerrainAtlas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerrainAtlas")
            .field("size", &(self.width, self.height))
            .field("tile_count", &self.tile_count)
            .field("tiles_bytes", &self.tiles.len())
            .finish()
    }
}

/// Палитра `.WRL` (768 байт RGB) в RGBA, с растяжением 6-битных каналов.
pub fn palette_from_wrl(raw: &[u8]) -> PaletteRgba {
    let mut palette = [[0u8, 0, 0, 255]; PALETTE_COLORS];
    let usable = raw.len().min(PALETTE_COLORS * PALETTE_STRIDE);

    if usable < PALETTE_COLORS * PALETTE_STRIDE {
        return palette;
    }

    // VGA-палитры хранят 6 бит на канал; если весь файл в 0..=63 — растягиваем.
    let six_bit = raw[..usable].iter().copied().max().unwrap_or(0) <= 63;

    for (index, entry) in palette.iter_mut().enumerate() {
        let r = raw[index * PALETTE_STRIDE];
        let g = raw[index * PALETTE_STRIDE + 1];
        let b = raw[index * PALETTE_STRIDE + 2];

        let scale = |value: u8| -> u8 {
            if six_bit {
                ((u16::from(value) * 255 + 31) / 63) as u8
            } else {
                value
            }
        };

        *entry = [scale(r), scale(g), scale(b), 255];
    }

    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas_with_one_tile(tiles: Vec<u8>, palette: PaletteRgba) -> TerrainAtlas {
        TerrainAtlas {
            palette,
            tiles,
            tile_ids: vec![0],
            tile_count: 1,
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn palette_scales_six_bit_channels() {
        let mut raw = vec![0u8; PALETTE_COLORS * PALETTE_STRIDE];
        raw[0] = 63;
        raw[1] = 32;

        let palette = palette_from_wrl(&raw);

        assert_eq!(palette[0][0], 255, "63 → 255");
        assert_eq!(palette[0][1], 130, "32 → 32*255/63 ≈ 130");
        assert_eq!(palette[0][3], 255, "альфа всегда 255");
    }

    #[test]
    fn palette_keeps_eight_bit_values() {
        let mut raw = vec![0u8; PALETTE_COLORS * PALETTE_STRIDE];
        raw[0] = 200;
        raw[1] = 120;
        raw[2] = 7;

        let palette = palette_from_wrl(&raw);

        assert_eq!(&palette[0][..3], &[200, 120, 7]);
    }

    #[test]
    fn tile_pixels_use_palette() {
        let mut palette = [[0u8, 0, 0, 255]; PALETTE_COLORS];
        palette[3] = [10, 20, 30, 255];

        let mut tiles = vec![0u8; TILE_PIXELS];
        tiles[0] = 3; // (0,0) тайла
        tiles[TILE_SIZE + 5] = 3; // (5,1)

        let atlas = atlas_with_one_tile(tiles, palette);

        assert_eq!(atlas.tile_pixel(0, 0, 0), Some(Rgba::rgb(10, 20, 30)));
        assert_eq!(atlas.tile_pixel(0, 5, 1), Some(Rgba::rgb(10, 20, 30)));
        assert_eq!(atlas.tile_pixel(0, 1, 0), Some(Rgba::rgb(0, 0, 0)));
        assert_eq!(atlas.tile_pixel(1, 0, 0), None, "нет такого тайла");
        assert_eq!(atlas.tile_pixel(0, 64, 0), None, "за пределами тайла");
    }

    #[test]
    fn cpp_sampling_matches_gfx_decode_map_tile() {
        // `map_tile_zoom_factor = ((64-1)<<16)/(size-1) + 8`;
        // источник = `(i*factor)>>16` (`gfx.cpp:128,196`).
        assert_eq!(cpp_tile_sample_offsets(4), vec![0, 21, 42, 63]);
        assert_eq!(
            cpp_tile_sample_offsets(8),
            vec![0, 9, 18, 27, 36, 45, 54, 63]
        );
        assert_eq!(
            cpp_tile_sample_offsets(64),
            (0..64).collect::<Vec<_>>(),
            "зум 1:1 — тождественная выборка"
        );
        assert_eq!(cpp_tile_sample_offsets(1), vec![0]);
    }

    #[test]
    fn cpp_tile_render_samples_tile_corners() {
        let mut palette = [[0u8, 0, 0, 255]; PALETTE_COLORS];
        palette[1] = [10, 20, 30, 255];
        palette[2] = [40, 50, 60, 255];

        let mut tiles = vec![0u8; TILE_PIXELS];
        tiles[0] = 1; // (0,0)
        tiles[TILE_SIZE * TILE_SIZE - 1] = 2; // (63,63)

        let atlas = atlas_with_one_tile(tiles, palette);
        let mut frame = RgbaBuffer::new(4, 4);

        assert_eq!(
            atlas.render_tile_cpp_into(
                &mut frame,
                (0, 0),
                crate::buffer::PixelRect::new(0, 0, 4, 4)
            ),
            16
        );
        assert_eq!(frame.get(0, 0), Some(Rgba::rgb(10, 20, 30)));
        assert_eq!(frame.get(3, 3), Some(Rgba::rgb(40, 50, 60)));
    }

    #[test]
    fn empty_atlas_draws_nothing() {
        let atlas = TerrainAtlas {
            palette: [[0u8, 0, 0, 255]; PALETTE_COLORS],
            tiles: Vec::new(),
            tile_ids: Vec::new(),
            tile_count: 0,
            width: 0,
            height: 0,
        };
        let mut frame = RgbaBuffer::new(8, 8);
        let viewport = Viewport::new(8, 8);

        assert!(atlas.is_empty());
        assert_eq!(atlas.render_into(&mut frame, &viewport), 0);
    }
}
