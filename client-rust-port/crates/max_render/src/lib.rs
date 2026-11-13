//! max_render — окно, отрисовка и UI.
//!
//! Слой рендера — тонкий потребитель `max_core`-состояния: рисует карту, юнитов,
//! меню и собирает ввод. Планируется на SDL3 (как C++-порт); до фазы R2 — заглушка.

use max_core::Map;

/// Состояние отрисовки.
#[derive(Debug, Default)]
pub struct Renderer {
    pub view_center: (i16, i16),
    pub zoom: u8,
}

impl Renderer {
    pub fn draw_map(&mut self, _map: &Map) {
        tracing::debug!("draw map at {:?}, zoom {}", self.view_center, self.zoom);
    }
}
