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

/// Номер хода (`WINDOW_TURN_COUNTER_DISPLAY`), ресурс `TURNS` 61×21.
pub const TURN_COUNTER: PixelRect = PixelRect::new(468, 2, 61, 21);
/// Таймер хода (`WINDOW_TURN_TIMER_DISPLAY`), ресурс `TIMER` 61×21.
pub const TURN_TIMER: PixelRect = PixelRect::new(534, 2, 61, 21);

/// Кнопка `+` зума (`WINDOW_ZOOM_PLUS_BUTTON`).
pub const ZOOM_PLUS: PixelRect = PixelRect::new(2, 275, 18, 14);
/// Кнопка-ползунок зума (`WINDOW_ZOOM_SLIDER_BUTTON`) — область клика.
pub const ZOOM_SLIDER_BUTTON: PixelRect = PixelRect::new(20, 275, 130, 14);
/// Окно ползунка зума (`WINDOW_ZOOM_SLIDER_WINDOW` / `ZOOMPNL1` 130×19).
pub const ZOOM_SLIDER: PixelRect = PixelRect::new(20, 275, 130, 19);
/// Кнопка `−` зума (`WINDOW_ZOOM_MINUS_BUTTON`).
pub const ZOOM_MINUS: PixelRect = PixelRect::new(150, 275, 20, 14);

/// Строка 1 статистики юнита (`WINDOW_STAT_ROW_1`).
pub const STAT_ROW_1: PixelRect = PixelRect::new(10, 173, 153, 12);
/// Строка 2 статистики юнита.
pub const STAT_ROW_2: PixelRect = PixelRect::new(10, 185, 153, 12);
/// Строка 3 статистики юнита.
pub const STAT_ROW_3: PixelRect = PixelRect::new(10, 197, 153, 12);
/// Строка 4 статистики юнита.
pub const STAT_ROW_4: PixelRect = PixelRect::new(10, 209, 153, 12);

/// Координаты клетки (`WINDOW_COORDINATES_DISPLAY`), `XYPOS` 90×21.
pub const COORDINATES: PixelRect = PixelRect::new(242, 456, 90, 21);
/// Описание юнита (`WINDOW_UNIT_DESCRIPTION_DISPLAY`), `UNITNAME` 222×21.
pub const UNIT_DESCRIPTION: PixelRect = PixelRect::new(338, 456, 222, 21);

/// Кнопка Play (`WINDOW_PLAY_BUTTON`), `PLAY_OF` 19×18.
pub const PLAY_BUTTON: PixelRect = PixelRect::new(146, 123, 19, 18);
/// Кнопка Pause (`WINDOW_PAUSE_BUTTON`), `PAUSE_OF` 19×19.
pub const PAUSE_BUTTON: PixelRect = PixelRect::new(146, 143, 19, 19);

/// Кнопка Center (`WINDOW_CENTER_BUTTON`), `FIND_OFF` 20×23.
pub const CENTER_BUTTON: PixelRect = PixelRect::new(4, 227, 20, 23);
/// Кнопка Lock (`WINDOW_LOCK_BUTTON`), `LOCK_OF` 20×22.
pub const LOCK_BUTTON: PixelRect = PixelRect::new(32, 227, 20, 22);
/// Кнопка Prev (`WINDOW_PRE_BUTTON`), `PREV_OF` 38×22.
pub const PREV_BUTTON: PixelRect = PixelRect::new(60, 228, 38, 22);
/// Кнопка Done (`WINDOW_DONE_BUTTON`), `UDONE_OF` 26×23.
pub const DONE_BUTTON: PixelRect = PixelRect::new(99, 228, 26, 22);
/// Кнопка Next (`WINDOW_NXT_BUTTON`), `NEXT_OF` 38×22.
pub const NEXT_BUTTON: PixelRect = PixelRect::new(125, 228, 38, 22);

/// Кнопка Help (`WINDOW_HELP_BUTTON`), `HELP_OF` 25×24.
pub const HELP_BUTTON: PixelRect = PixelRect::new(21, 250, 25, 24);
/// Кнопка Reports (`WINDOW_REPORTS_BUTTON`), `REPT_OFF` 48×19.
pub const REPORTS_BUTTON: PixelRect = PixelRect::new(51, 252, 48, 19);
/// Кнопка Chat (`WINDOW_CHAT_BUTTON`), `CHAT_OFF` 48×19.
pub const CHAT_BUTTON: PixelRect = PixelRect::new(102, 252, 48, 19);

/// Кнопка `2x` миникарты (`WINDOW_2X_MINIMAP`), `MIN2X_OF` 27×26.
pub const MINIMAP_2X: PixelRect = PixelRect::new(136, 387, 27, 26);
/// Кнопка `TNT` миникарты (`WINDOW_TNT_MINIMAP`), `MINFL_OF` 27×27.
pub const MINIMAP_TNT: PixelRect = PixelRect::new(136, 413, 27, 27);

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
