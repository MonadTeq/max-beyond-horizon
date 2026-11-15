//! Раскладка окна игры 1:1 по оригиналу (`max-cpp-port/src/window_manager.cpp`).
//!
//! Оригинал рисует окно 640×480: фон — картинка `FRAMEPIC` (в ней же нарисованы
//! панели и кнопки), карта партии — прямоугольник [`MAIN_MAP`] (7×7 клеток по
//! 64 px), слева панель с портретом/статистикой/миникартой, сверху — кнопки
//! Files/Preferences и таймеры, справа — End Turn.
//!
//! Координаты ниже — из таблицы `windows[]` C++-порта (`WINDOW_ITEM`), включая
//! `WINDOW_MAIN_MAP` = (180, 18) … (180 + 7·64 − 1, 18 + 7·64 − 1).

use crate::buffer::PixelRect;

/// Ширина окна оригинала (`WINDOW_WIDTH`).
pub const WINDOW_WIDTH: i32 = 640;
/// Высота окна оригинала (`WINDOW_HEIGHT`).
pub const WINDOW_HEIGHT: i32 = 480;

/// Область карты в окне: 7×7 клеток по 64 px (`WINDOW_MAIN_MAP`).
pub const MAIN_MAP: PixelRect = PixelRect::new(180, 18, 7 * 64, 7 * 64);

/// Миникарта в панели (`WINDOW_MINIMAP`).
pub const MINIMAP: PixelRect = PixelRect::new(15, 356, 112, 112);

/// Кнопка End Turn (`WINDOW_ENDTURN_BUTTON`).
pub const END_TURN_BUTTON: PixelRect = PixelRect::new(390, 2, 72, 21);
/// Кнопка Files (`WINDOW_FILES_BUTTON`).
pub const FILES_BUTTON: PixelRect = PixelRect::new(17, 4, 67, 18);
/// Кнопка Preferences (`WINDOW_PREFS_BUTTON`).
pub const PREFS_BUTTON: PixelRect = PixelRect::new(86, 4, 67, 18);

/// Окно со статистикой (4 строки) в левой панели (`WINDOW_STAT_WINDOW`).
pub const STAT_WINDOW: PixelRect = PixelRect::new(10, 173, 153, 48);
/// Окно портрета/анимации юнита (`WINDOW_CORNER_FLIC`).
pub const CORNER_FLIC: PixelRect = PixelRect::new(10, 29, 128, 128);

/// Кнопки SURVEY/STATUS/COLORS (`WINDOW_*_BUTTON`).
pub const SURVEY_BUTTON: PixelRect = PixelRect::new(4, 298, 55, 16);
/// Кнопка STATUS.
pub const STATUS_BUTTON: PixelRect = PixelRect::new(4, 314, 55, 16);
/// Кнопка COLORS.
pub const COLORS_BUTTON: PixelRect = PixelRect::new(4, 330, 55, 18);

/// Кнопки HITS/AMMO/RANGE.
pub const HITS_BUTTON: PixelRect = PixelRect::new(59, 298, 55, 16);
/// Кнопка AMMO.
pub const AMMO_BUTTON: PixelRect = PixelRect::new(59, 314, 55, 16);
/// Кнопка RANGE.
pub const RANGE_BUTTON: PixelRect = PixelRect::new(59, 330, 55, 18);

/// Кнопки SCAN/GRID/NAME.
pub const SCAN_BUTTON: PixelRect = PixelRect::new(114, 298, 55, 16);
/// Кнопка GRID.
pub const GRID_BUTTON: PixelRect = PixelRect::new(114, 314, 55, 16);
/// Кнопка NAME.
pub const NAME_BUTTON: PixelRect = PixelRect::new(114, 330, 55, 18);

/// Половинки левой панели (`PANELTOP`/`PANELBTM`): 171 px шириной.
pub const SIDE_PANEL_WIDTH: i32 = 171;
/// Высота `PANELTOP`.
pub const SIDE_PANEL_TOP_HEIGHT: i32 = 239;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_map_matches_original_seven_by_seven_tiles() {
        assert_eq!(MAIN_MAP.ulx, 180);
        assert_eq!(MAIN_MAP.uly, 18);
        assert_eq!(MAIN_MAP.width, 448);
        assert_eq!(MAIN_MAP.height, 448);
        assert!(MAIN_MAP.lrx() < WINDOW_WIDTH, "карта не вылезает за окно");
        assert!(MAIN_MAP.lry() < WINDOW_HEIGHT);
    }

    #[test]
    fn side_panel_is_split_at_239() {
        assert_eq!(SIDE_PANEL_WIDTH, 171);
        assert_eq!(
            SIDE_PANEL_TOP_HEIGHT + (WINDOW_HEIGHT - SIDE_PANEL_TOP_HEIGHT),
            WINDOW_HEIGHT
        );
        assert!(
            SIDE_PANEL_WIDTH < MAIN_MAP.ulx,
            "между панелью и картой есть зазор (скроллбар), как в оригинале"
        );
    }

    /// Пересечение прямоугольников (для проверки раскладки).
    fn overlaps(a: PixelRect, b: PixelRect) -> bool {
        a.ulx < b.lrx() && a.lrx() > b.ulx && a.uly < b.lry() && a.lry() > b.uly
    }

    #[test]
    fn side_panel_controls_do_not_overlap_main_map() {
        // End Turn в оригинале намеренно чуть заходит на карту (y=2..23 при
        // uly=18), поэтому он исключён: его рисуют поверх.
        for rect in [
            FILES_BUTTON,
            PREFS_BUTTON,
            SURVEY_BUTTON,
            STATUS_BUTTON,
            COLORS_BUTTON,
            HITS_BUTTON,
            AMMO_BUTTON,
            RANGE_BUTTON,
            SCAN_BUTTON,
            GRID_BUTTON,
            NAME_BUTTON,
            STAT_WINDOW,
            CORNER_FLIC,
            MINIMAP,
        ] {
            assert!(
                !overlaps(rect, MAIN_MAP),
                "элемент {rect:?} пересекается с областью карты"
            );
        }

        assert!(
            overlaps(END_TURN_BUTTON, MAIN_MAP),
            "End Turn в оригинале заходит на карту — если это изменится, поправить раскладку"
        );
    }
}
