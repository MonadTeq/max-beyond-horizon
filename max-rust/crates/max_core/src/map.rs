//! Карта мира: размеры, тайлы, ресурсы. Заполняется из снапшота сервера.

use serde::{Deserialize, Serialize};

/// Размеры карты в тайлах.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapSize {
    pub width: i16,
    pub height: i16,
}

/// Тип поверхности тайла (по оригиналу M.A.X.: land/water/coast/air...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Surface {
    Land,
    Water,
    Coast,
    Air,
}

/// Ресурс на тайле.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceKind {
    Raw,
    Fuel,
    Gold,
}

/// Карта: поверхность + ресурсы по тайлам.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Map {
    pub size: MapSize,
    pub surface: Vec<Surface>,
    pub resources: Vec<Option<ResourceKind>>,
}

impl Map {
    pub fn new(size: MapSize) -> Self {
        let tiles = size.width as usize * size.height as usize;
        Self {
            size,
            surface: vec![Surface::Land; tiles],
            resources: vec![None; tiles],
        }
    }

    pub fn index(&self, x: i16, y: i16) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.size.width || y >= self.size.height {
            return None;
        }
        Some(y as usize * self.size.width as usize + x as usize)
    }
}
