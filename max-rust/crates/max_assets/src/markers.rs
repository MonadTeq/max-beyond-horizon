//! Маркеры ресурсов на карте — `RAWMSK*`/`FUELMK*`/`GOLDMK*` и малые
//! `SMRWMK*`/`SMFLMK*`/`SMGDMK*` из `MAX.RES`.
//!
//! Оракул — `max-cpp-port/src/survey.cpp` (`Survey_RenderMarker`):
//!
//! ```cpp
//! resource_value = min(material_type & CARGO_MASK, 16);
//! if (Gfx_ZoomLevel > 16) marker = RAWMSK0 + resource_value;   // большие 62×62
//! else                    marker = SMRWMK1 + resource_value - 1; // малые 14×16
//! ```
//!
//! Большие маркеры — simple image с hotspot `(31,31)` (центр клетки), малые —
//! `(7,8)`. При зуме не 1:1 C++ масштабирует картинку (`WindowManager_
//! RescaleSimpleImage`), это делает рендер. `I_RAW`/`I_FUEL`/`I_GOLD` — иконки
//! интерфейса (статистика/меню), а **не** маркеры карты; здесь они лежат только
//! запасным вариантом, если спрайт-маркер отсутствует в архиве.

use crate::image::{ImageError, SimpleImage};
use crate::res::ResArchive;
use max_core::map::ResourceKind;

/// Больших маркеров на каждый тип: `*MK0..*MK16` (0 — пустая клетка).
pub const BIG_MARKER_COUNT: usize = 17;
/// Малых маркеров на каждый тип: `SM*MK1..SM*MK16`.
pub const SMALL_MARKER_COUNT: usize = 16;

/// Индексы типов ресурсов в таблицах маркеров.
const RAW: usize = 0;
const FUEL: usize = 1;
const GOLD: usize = 2;

fn kind_index(kind: ResourceKind) -> usize {
    match kind {
        ResourceKind::Raw => RAW,
        ResourceKind::Fuel => FUEL,
        ResourceKind::Gold => GOLD,
    }
}

/// Загруженные маркеры ресурсов.
#[derive(Debug, Clone, Default)]
pub struct ResourceMarkers {
    big: [Vec<SimpleImage>; 3],
    small: [Vec<SimpleImage>; 3],
    /// `I_RAW`/`I_FUEL`/`I_GOLD` — fallback, если маркеров нет.
    icons: [Option<SimpleImage>; 3],
    loaded: usize,
}

impl ResourceMarkers {
    /// Грузит все маркеры, которые есть в архиве.
    pub fn load(archive: &ResArchive) -> Self {
        let read = |name: &str| -> Option<SimpleImage> {
            archive
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };

        let mut markers = Self::default();

        for (index, prefix) in [(RAW, "RAWMSK"), (FUEL, "FUELMK"), (GOLD, "GOLDMK")] {
            for frame in 0..BIG_MARKER_COUNT {
                if let Some(image) = read(&format!("{prefix}{frame}")) {
                    markers.big[index].push(image);
                    markers.loaded += 1;
                }
            }
        }

        for (index, prefix) in [(RAW, "SMRWMK"), (FUEL, "SMFLMK"), (GOLD, "SMGDMK")] {
            for frame in 1..=SMALL_MARKER_COUNT {
                if let Some(image) = read(&format!("{prefix}{frame}")) {
                    markers.small[index].push(image);
                    markers.loaded += 1;
                }
            }
        }

        for (index, name) in [(RAW, "I_RAW"), (FUEL, "I_FUEL"), (GOLD, "I_GOLD")] {
            markers.icons[index] = read(name);
        }

        markers
    }

    /// Сколько картинок маркеров загрузилось.
    pub fn loaded(&self) -> usize {
        self.loaded
    }

    /// Большой маркер для количества `amount` (C++: `min(amount, 16)`).
    pub fn big(&self, kind: ResourceKind, amount: u8) -> Option<&SimpleImage> {
        let amount = usize::from(amount.min(16));
        let table = &self.big[kind_index(kind)];

        // Если в архиве нет части кадров, ближайший меньший индекс — разумный
        // fallback (в эталонном MAX.RES есть все 17).
        table.get(amount).or_else(|| table.first())
    }

    /// Малый маркер для количества `amount` (C++: кадр `amount - 1`, 1..=16).
    pub fn small(&self, kind: ResourceKind, amount: u8) -> Option<&SimpleImage> {
        let amount = amount.clamp(1, 16);
        let table = &self.small[kind_index(kind)];
        let index = usize::from(amount - 1);

        table.get(index).or_else(|| table.first())
    }

    /// Иконка интерфейса `I_*` (fallback).
    pub fn icon(&self, kind: ResourceKind) -> Option<&SimpleImage> {
        self.icons[kind_index(kind)].as_ref()
    }

    /// Есть ли хоть один маркер/иконка для карты.
    pub fn is_ready(&self) -> bool {
        self.loaded > 0 || self.icons.iter().any(Option::is_some)
    }
}

/// Разбирает одну картинку маркера (для тестов/отчётов).
pub fn decode_marker(bytes: &[u8]) -> Result<SimpleImage, ImageError> {
    SimpleImage::decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_max_res_has_all_resource_markers() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../max-original/MAX.RES");

        if !path.is_file() {
            return;
        }

        let archive = ResArchive::open(&path).expect("MAX.RES");
        let markers = ResourceMarkers::load(&archive);

        assert_eq!(
            markers.loaded(),
            3 * BIG_MARKER_COUNT + 3 * SMALL_MARKER_COUNT,
            "17 больших и 16 малых маркеров на каждый тип"
        );

        for kind in [ResourceKind::Raw, ResourceKind::Fuel, ResourceKind::Gold] {
            let big = markers.big(kind, 1).expect("большой маркер 1");
            assert_eq!((big.width, big.height), (62, 62));
            assert_eq!((big.ulx, big.uly), (31, 31), "hotspot в центре клетки");
            assert_eq!(
                Some(big.transparent),
                big.pixel(0, 0),
                "прозрачный индекс — первый пиксель картинки"
            );

            let empty = markers.big(kind, 0).expect("*MK0");
            assert_eq!((empty.width, empty.height), (38, 38));

            let small = markers.small(kind, 1).expect("малый маркер 1");
            assert_eq!((small.width, small.height), (14, 16));
            assert_eq!((small.ulx, small.uly), (7, 8));

            let icon = markers.icon(kind).expect("I_*");
            assert!(icon.width > 0 && icon.height > 0);
        }

        // Количество сверх 16 клампится, как `min(material_type & CARGO_MASK, 16)`.
        assert_eq!(
            markers.big(ResourceKind::Raw, 31).map(|image| image.width),
            markers.big(ResourceKind::Raw, 16).map(|image| image.width)
        );
    }
}
