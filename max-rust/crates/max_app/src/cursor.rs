//! Курсоры мыши: контекст (`Cursor_SetCursor`) и отрисовка/подготовка для SDL.
//!
//! Формат и загрузка 30 ресурсов — [`max_assets::cursor`] (`Cursor_Descriptor`,
//! `Cursor_ResourceLut` из `cursor.cpp:55-58`). Здесь — то, что зависит от
//! состояния игры:
//!
//! * [`CursorId`] — индексы ровно как `enum` в `cursor.hpp`;
//! * [`CursorContext`] — упрощённый `Cursor_GetDefaultWindowCursor` +
//!   `GameManager_GetWindowCursor`: меню/панель/миникарта → `HANDPTR`, карта без
//!   выделенного юнита → `MAP_PTR`, карта с приказом → `UNIT_GO`/`UNIT_NGO`,
//!   над своим/чужим юнитом → `FRND_PTR`/`ENMY_PTR`;
//! * [`CursorImage`] — RGBA-кадр с hotspot для SDL-слоя (`Cursor::
//!   from_surface`), headless-тесты работают без SDL.
//!
//! Анимация (`PTR_PATH`, 10 кадров) отдаётся покадрово: SDL-слой сам решает,
//! когда переключать кадр (`Cursor_AnimationTick`, 200 мс в C++).

use max_assets::{CursorSet, CURSOR_RESOURCES};
use max_core::enums::unit_flags;
use max_render::terrain::PaletteRgba;

/// Индекс курсора (`CursorId` из `cursor.hpp:34-65`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CursorId {
    /// `CURSOR_HIDDEN` — курсор скрыт.
    Hidden = 0,
    /// `CURSOR_HAND` — обычная стрелка-рука (меню, панели).
    Hand = 1,
    /// `CURSOR_ARROW_N`.
    ArrowN = 2,
    /// `CURSOR_ARROW_NE`.
    ArrowNe = 3,
    /// `CURSOR_ARROW_E`.
    ArrowE = 4,
    /// `CURSOR_ARROW_SE`.
    ArrowSe = 5,
    /// `CURSOR_ARROW_S`.
    ArrowS = 6,
    /// `CURSOR_ARROW_SW`.
    ArrowSw = 7,
    /// `CURSOR_ARROW_W`.
    ArrowW = 8,
    /// `CURSOR_ARROW_NW`.
    ArrowNw = 9,
    /// `CURSOR_MAP` — карта без выделения.
    Map = 10,
    /// `CURSOR_MINI` — миникарта (в оригинале не используется, LUT даёт HANDPTR).
    Mini = 11,
    /// `CURSOR_REPAIR`.
    Repair = 12,
    /// `CURSOR_TRANSFER`.
    Transfer = 13,
    /// `CURSOR_FUEL`.
    Fuel = 14,
    /// `CURSOR_RELOAD`.
    Reload = 15,
    /// `CURSOR_LOAD`.
    Load = 16,
    /// `CURSOR_FRIEND` — над своим юнитом.
    Friend = 17,
    /// `CURSOR_ENEMY` — над врагом (с полоской силы удара).
    Enemy = 18,
    /// `CURSOR_FAR_TARGET`.
    FarTarget = 19,
    /// `CURSOR_UNIT_GO` — приказ возможен.
    UnitGo = 20,
    /// `CURSOR_UNIT_NO_GO` — приказ невозможен.
    UnitNoGo = 21,
    /// `CURSOR_WAY` — точка маршрута.
    Way = 22,
    /// `CURSOR_GROUP`.
    Group = 23,
    /// `CURSOR_ACTIVATE`.
    Activate = 24,
    /// `CURSOR_MAP2` (второй `MAP_PTR` в LUT).
    Map2 = 25,
    /// `CURSOR_STEAL`.
    Steal = 26,
    /// `CURSOR_DISABLE`.
    Disable = 27,
    /// `CURSOR_PATH` — анимированный путь (10 кадров).
    Path = 28,
    /// `CURSOR_HELP`.
    Help = 29,
}

impl CursorId {
    /// Все курсоры в порядке `Cursor_ResourceLut`.
    pub const ALL: [Self; 30] = [
        Self::Hidden,
        Self::Hand,
        Self::ArrowN,
        Self::ArrowNe,
        Self::ArrowE,
        Self::ArrowSe,
        Self::ArrowS,
        Self::ArrowSw,
        Self::ArrowW,
        Self::ArrowNw,
        Self::Map,
        Self::Mini,
        Self::Repair,
        Self::Transfer,
        Self::Fuel,
        Self::Reload,
        Self::Load,
        Self::Friend,
        Self::Enemy,
        Self::FarTarget,
        Self::UnitGo,
        Self::UnitNoGo,
        Self::Way,
        Self::Group,
        Self::Activate,
        Self::Map2,
        Self::Steal,
        Self::Disable,
        Self::Path,
        Self::Help,
    ];

    /// Индекс в массиве ресурсов.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Курсор по индексу (`None` вне 0..30).
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// Имя ресурса в `MAX.RES`.
    pub fn resource_name(self) -> &'static str {
        CURSOR_RESOURCES[self.index()]
    }
}

/// Что происходит под курсором на карте (`GameManager_GetWindowCursor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorHover {
    /// Свой юнит (`CURSOR_FRIEND`).
    Friend,
    /// Видимый враг (`CURSOR_ENEMY`).
    Enemy,
    /// Пустая клетка.
    Empty,
}

/// Контекст курсора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorContext {
    /// Меню/форма создания (обычная стрелка).
    Menu,
    /// Карта без выделенного юнита (`CURSOR_MAP`).
    MapIdle,
    /// Карта, выделен юнит: наведение на юнит/пустую клетку.
    MapSelected(CursorHover),
    /// Панель/кнопка (`HANDPTR`).
    Panel,
    /// Миникарта (в C++ default — `HANDPTR`).
    Minimap,
    /// Приказ возможен/невозможен.
    Order {
        /// Можно ли выполнить приказ.
        valid: bool,
    },
    /// Справка (`CURSOR_HELP`).
    Help,
}

/// Курсор для контекста — как `Cursor_SetCursor` в `game_manager.cpp:6449-6468`.
pub const fn cursor_for_context(context: CursorContext) -> CursorId {
    match context {
        CursorContext::Menu | CursorContext::Panel | CursorContext::Minimap => CursorId::Hand,
        CursorContext::MapIdle => CursorId::Map,
        CursorContext::MapSelected(CursorHover::Friend) => CursorId::Friend,
        CursorContext::MapSelected(CursorHover::Enemy) => CursorId::Enemy,
        CursorContext::MapSelected(CursorHover::Empty) => CursorId::UnitGo,
        CursorContext::Order { valid: true } => CursorId::UnitGo,
        CursorContext::Order { valid: false } => CursorId::UnitNoGo,
        CursorContext::Help => CursorId::Help,
    }
}

/// RGBA-кадр курсора с hotspot — для `Cursor::from_surface` (SDL3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorImage {
    /// Ширина кадра.
    pub width: u32,
    /// Высота кадра.
    pub height: u32,
    /// Hotspot по x (`Cursor_Descriptor::ulx`).
    pub hot_x: i32,
    /// Hotspot по y (`Cursor_Descriptor::uly`).
    pub hot_y: i32,
    /// RGBA8-пиксели (`width * height * 4`), прозрачные — alpha 0.
    pub rgba: Vec<u8>,
}

impl CursorImage {
    /// Пиксель RGBA по координатам кадра.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let offset = (y as usize * self.width as usize + x as usize) * 4;

        self.rgba
            .get(offset..offset + 4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
    }
}

/// Готовит кадр курсора к SDL: индексы палитры окна → RGBA.
///
/// Возвращает `None`, если курсор не загрузился или кадра нет.
pub fn cursor_image(
    cursors: &CursorSet,
    palette: &PaletteRgba,
    id: CursorId,
    frame: usize,
) -> Option<CursorImage> {
    let cursor = cursors.get(id.index())?;
    let pixels = cursor.frame(frame)?;
    let width = cursor.width as u32;
    let height = cursor.height as u32;
    let mut rgba = vec![0u8; width as usize * height as usize * 4];

    for (index, &color_index) in pixels.iter().enumerate() {
        let offset = index * 4;

        if color_index == cursor.transparent {
            rgba[offset + 3] = 0;
            continue;
        }

        let [r, g, b, _] = palette[usize::from(color_index)];

        rgba[offset] = r;
        rgba[offset + 1] = g;
        rgba[offset + 2] = b;
        rgba[offset + 3] = 255;
    }

    Some(CursorImage {
        width,
        height,
        hot_x: cursor.hot_x,
        hot_y: cursor.hot_y,
        rgba,
    })
}

/// Курсор для точки окна в партии — порт `GameManager_GetWindowCursor` в той
/// части, которую покрывает клиент.
pub fn session_cursor(app: &crate::App, mouse: Option<(i32, i32)>) -> (CursorId, usize) {
    let Some((x, y)) = mouse else {
        return (CursorId::Hand, 0);
    };

    if crate::panels::panel_button_at(x, y).is_some() || max_render::layout::MINIMAP.contains(x, y)
    {
        return (CursorId::Hand, 0);
    }

    let Some((map_x, map_y)) = crate::panels::screen_to_map(x, y) else {
        return (CursorId::Hand, 0);
    };

    let viewport = crate::panels::map_viewport(app.viewport());
    let (grid_x, grid_y) = viewport.screen_to_grid(map_x, map_y);
    let player_team = app.state().player_team;
    let selected = app.selected_unit().is_some();
    let mut hover = None;

    for id in app
        .state()
        .units_at(max_core::geo::Point::new(grid_x, grid_y))
    {
        let Some(unit) = app.state().unit(*id) else {
            continue;
        };

        if unit.team == player_team {
            hover = Some(CursorHover::Friend);
            break;
        }

        if unit.is_visible_to_team(player_team) {
            hover = Some(CursorHover::Enemy);
        }
    }

    let context = if app.hud().preview.is_some() {
        CursorContext::Order { valid: true }
    } else if selected {
        CursorContext::MapSelected(hover.unwrap_or(CursorHover::Empty))
    } else {
        CursorContext::MapIdle
    };

    (cursor_for_context(context), 0)
}

/// Есть ли у курсора анимация (для SDL-тика).
pub fn frame_count(cursors: &CursorSet, id: CursorId) -> usize {
    cursors
        .get(id.index())
        .map_or(0, |cursor| usize::from(cursor.frame_count))
}

/// Условие TNT-фильтра миникарты — вынесено для тестов и переиспользования.
pub fn has_firing_sprite(flags: u32) -> bool {
    flags & (unit_flags::HAS_FIRING_SPRITE | unit_flags::FIRES_MISSILES) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_ids_follow_the_cpp_enum_and_resource_table() {
        assert_eq!(CursorId::Hidden.index(), 0);
        assert_eq!(CursorId::Hand.resource_name(), "HANDPTR");
        assert_eq!(CursorId::Map.resource_name(), "MAP_PTR");
        assert_eq!(CursorId::Map2.resource_name(), "MAP_PTR");
        assert_eq!(CursorId::Path.resource_name(), "PTR_PATH");
        assert_eq!(CursorId::Help.index(), 29);
        assert_eq!(CursorId::from_index(30), None);

        for (index, id) in CursorId::ALL.into_iter().enumerate() {
            assert_eq!(id.index(), index);
            assert_eq!(id.resource_name(), CURSOR_RESOURCES[index]);
        }
    }

    #[test]
    fn context_switches_cursor_like_cursor_setcursor() {
        assert_eq!(cursor_for_context(CursorContext::Menu), CursorId::Hand);
        assert_eq!(cursor_for_context(CursorContext::Panel), CursorId::Hand);
        assert_eq!(cursor_for_context(CursorContext::Minimap), CursorId::Hand);
        assert_eq!(cursor_for_context(CursorContext::MapIdle), CursorId::Map);
        assert_eq!(
            cursor_for_context(CursorContext::MapSelected(CursorHover::Friend)),
            CursorId::Friend
        );
        assert_eq!(
            cursor_for_context(CursorContext::MapSelected(CursorHover::Enemy)),
            CursorId::Enemy
        );
        assert_eq!(
            cursor_for_context(CursorContext::MapSelected(CursorHover::Empty)),
            CursorId::UnitGo
        );
        assert_eq!(
            cursor_for_context(CursorContext::Order { valid: false }),
            CursorId::UnitNoGo
        );
        assert_eq!(cursor_for_context(CursorContext::Help), CursorId::Help);
    }

    #[test]
    fn session_cursor_switches_between_panel_and_map() {
        let app = crate::App::new(crate::AppConfig::default());

        assert_eq!(session_cursor(&app, None).0, CursorId::Hand);
        // Миникарта (15,356) — HANDPTR, как default-window-cursor C++.
        assert_eq!(session_cursor(&app, Some((20, 360))).0, CursorId::Hand);
        // Карта без выделения — MAP_PTR.
        assert_eq!(session_cursor(&app, Some((300, 200))).0, CursorId::Map);
    }

    #[test]
    fn cursor_image_converts_palette_indices_and_hotspot() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAX.RES");

        if !path.is_file() {
            return;
        }

        let archive = max_assets::ResArchive::open(&path).expect("MAX.RES");
        let cursors = CursorSet::load(&archive);
        let mut palette = [[0u8, 0, 0, 255]; 256];

        for (index, entry) in palette.iter_mut().enumerate() {
            *entry = [index as u8, 1, 2, 255];
        }

        let hand = cursor_image(&cursors, &palette, CursorId::Hand, 0).expect("HANDPTR");
        assert_eq!((hand.width, hand.height), (18, 18));
        assert_eq!((hand.hot_x, hand.hot_y), (0, 1));
        assert_eq!(hand.rgba.len(), 18 * 18 * 4);

        let path_cursor = cursor_image(&cursors, &palette, CursorId::Path, 0).expect("PTR_PATH");
        assert_eq!((path_cursor.width, path_cursor.height), (23, 23));
        assert_eq!((path_cursor.hot_x, path_cursor.hot_y), (13, 13));
        assert_eq!(frame_count(&cursors, CursorId::Path), 10);
        assert_eq!(frame_count(&cursors, CursorId::Hand), 1);

        // Прозрачный индекс не попадает в кадр как цвет.
        assert!(frame_count(&cursors, CursorId::Hidden) == 1);
        assert!(cursor_image(&cursors, &palette, CursorId::Hidden, 1).is_none());
    }
}
