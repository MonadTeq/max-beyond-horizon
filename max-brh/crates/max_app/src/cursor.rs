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
use max_core::access;
use max_core::enums::{unit_flags, GameStateType, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{UnitId, UnitInfo, UnitListKind};
use max_core::GameState;
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
    /// Курсорный режим `load` (`unit->cursor = 7`): `CURSOR_LOAD` над целью,
    /// которая может сесть в транспорт (`GameManager_GetWindowCursor`).
    Load {
        /// Цель под курсором принимается транспортом.
        valid: bool,
    },
    /// Курсорный режим диверсанта «захват» (`cursor = 8`) — `CURSOR_STEAL`.
    Steal {
        /// Цель — валидная цель захвата.
        valid: bool,
    },
    /// Курсорный режим диверсанта «отключение» (`cursor = 9`) — `CURSOR_DISABLE`.
    Disable {
        /// Цель — валидная цель отключения.
        valid: bool,
    },
    /// Режим наведения (`targeting_mode`): `CURSOR_ENEMY` над целью,
    /// `CURSOR_UNIT_NO_GO` вне радиуса/без выстрелов.
    Targeting {
        /// Попадает ли клетка в радиус и есть ли выстрелы.
        valid: bool,
    },
    /// Режим выбора места стройки (`ORDER_STATE_SELECT_SITE`):
    /// `CURSOR_MAP2`/`CURSOR_UNIT_NO_GO` (`GameManager_GetBuilderUnitCursor`).
    BuildSite {
        /// Попадает ли курсор в 2×2 и проходима ли площадка.
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
        CursorContext::Load { valid: true } => CursorId::Load,
        CursorContext::Load { valid: false } => CursorId::UnitNoGo,
        CursorContext::Steal { valid: true } => CursorId::Steal,
        CursorContext::Steal { valid: false } => CursorId::UnitNoGo,
        CursorContext::Disable { valid: true } => CursorId::Disable,
        CursorContext::Disable { valid: false } => CursorId::UnitNoGo,
        CursorContext::Targeting { valid: true } => CursorId::Enemy,
        CursorContext::Targeting { valid: false } => CursorId::UnitNoGo,
        CursorContext::BuildSite { valid: true } => CursorId::Map2,
        CursorContext::BuildSite { valid: false } => CursorId::UnitNoGo,
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

/// `GameManager_IsUnitNextToPosition` (`game_manager.cpp:3208`): клетка в
/// окрестности юнита (для здания 2×2 — расширенной на 1 во все стороны).
pub fn is_unit_next_to_position(unit: &UnitInfo, grid: Point) -> bool {
    let size = if unit.is_quad() { 2 } else { 1 };

    i32::from(unit.grid.x) - 1 <= i32::from(grid.x)
        && i32::from(unit.grid.x) + size >= i32::from(grid.x)
        && i32::from(unit.grid.y) - 1 <= i32::from(grid.y)
        && i32::from(unit.grid.y) + size >= i32::from(grid.y)
}

/// `Task_IsReadyToTakeOrders` (`task.cpp:105`).
///
/// `Access_GetActiveLoader(unit) == nullptr` в порте моделируется
/// [`max_core::access::is_engaged`] (юнит, которого грузят/ведут, приказов не
/// принимает).
pub fn ready_to_take_orders(state: &GameState, unit: &UnitInfo) -> bool {
    if unit.hits == 0 {
        return false;
    }

    let order_ok = matches!(unit.orders, UnitOrder::Await | UnitOrder::Sentry)
        || (matches!(unit.orders, UnitOrder::Move | UnitOrder::MoveToUnit)
            && unit.state == UnitOrderState::ExecutingOrder);

    order_ok && !access::is_engaged(state, unit)
}

/// `Access_GetEnemyUnit(team, x, y, flags)` (`access.cpp:1731`): видимый враг в
/// клетке (сначала обычные юниты, затем ground cover/мины).
pub fn get_enemy_unit(
    state: &GameState,
    team: TeamId,
    grid: Point,
    flags: u32,
) -> Option<UnitId> {
    let mut fallback = None;

    for id in state.units_at(grid).iter().copied() {
        let Some(other) = state.unit(id) else {
            continue;
        };

        if other.team == team
            || !other.is_visible_to_team(team)
            || other.orders == UnitOrder::Idle
            || other.flags & flags == 0
            || other.orders == UnitOrder::Explode
            || other.state == UnitOrderState::Destroy
        {
            continue;
        }

        let ground_cover = other.flags & unit_flags::GROUND_COVER != 0;

        if !ground_cover
            || matches!(other.unit_type, UnitType::LandMine | UnitType::SeaMine)
        {
            return resolve_tape_parent(state, id);
        }

        if fallback.is_none() {
            fallback = Some(id);
        }
    }

    fallback.and_then(|id| resolve_tape_parent(state, id))
}

/// Хвост `Access_GetTeamUnit`/`Access_GetEnemyUnit`: лента принадлежит
/// строителю (`unit = unit->GetParent()`).
fn resolve_tape_parent(state: &GameState, id: UnitId) -> Option<UnitId> {
    let unit = state.unit(id)?;

    if matches!(unit.unit_type, UnitType::LargeTape | UnitType::SmallTape) {
        unit.parent_unit
    } else {
        Some(id)
    }
}

/// `GameManager_IsValidTransferTarget` (`game_manager.cpp:6084`).
pub fn is_valid_transfer_target(state: &GameState, unit1: &UnitInfo, unit2: &UnitInfo) -> bool {
    if unit1.id == unit2.id {
        return false;
    }

    if let Some(complex) = unit2.complex {
        for id in state.unit_list(UnitListKind::Stationary) {
            let Some(other) = state.unit(*id) else {
                continue;
            };

            if other.complex == Some(complex) && is_unit_next_to_position(other, unit1.grid) {
                return true;
            }
        }

        false
    } else {
        is_unit_next_to_position(unit2, unit1.grid)
    }
}

/// `GameManager_IsGroupOrderInterrupted` (`access.cpp:2390`): в группе есть
/// юнит, который ещё идёт.
pub fn is_group_order_interrupted(state: &GameState, unit: &UnitInfo) -> bool {
    let Some(members) = access::group_members_of(state, unit.id) else {
        return false;
    };

    members.iter().any(|id| {
        state.unit(*id).is_some_and(|other| {
            matches!(other.orders, UnitOrder::Move | UnitOrder::MoveToAttack)
                && matches!(
                    other.state,
                    UnitOrderState::InProgress
                        | UnitOrderState::InTransition
                        | UnitOrderState::NewOrder
                )
        })
    })
}

/// `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5829`).
///
/// Ветка выбора площадки (`SELECT_SITE`) приходит сюда только из
/// `GameManager_GetWindowCursor`; в порте призрак площадки живёт в
/// [`crate::buildsite`] и проверяется до вызова этой функции
/// ([`session_cursor`]).
pub fn builder_unit_cursor(
    state: &GameState,
    player_team: TeamId,
    unit1: &UnitInfo,
    grid: Point,
    team_unit: Option<&UnitInfo>,
) -> CursorId {
    if unit1.state == UnitOrderState::UnitReady {
        let next_to_parent = unit1
            .parent_unit
            .and_then(|id| state.unit(id))
            .is_some_and(|parent| is_unit_next_to_position(parent, grid));

        if next_to_parent
            && access::is_accessible(
                state,
                unit1.unit_type,
                player_team,
                grid,
                access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) > 0
        {
            CursorId::UnitGo
        } else if team_unit.is_some() {
            CursorId::Friend
        } else {
            CursorId::UnitNoGo
        }
    } else if team_unit.is_some() {
        CursorId::Friend
    } else {
        CursorId::UnitNoGo
    }
}

/// `GameManager_GetAirUnitCursor` (`game_manager.cpp:5937`): приказ
/// `ORDER_IDLE` у юнита (лента/внутри здания) — `CURSOR_ACTIVATE`.
pub fn air_unit_cursor(
    state: &GameState,
    player_team: TeamId,
    unit1: &UnitInfo,
    grid: Point,
    team_unit: Option<&UnitInfo>,
) -> CursorId {
    let parent = unit1.parent_unit.and_then(|id| state.unit(id));

    if let Some(parent) = parent {
        if is_unit_next_to_position(parent, grid)
            && access::is_accessible(
                state,
                unit1.unit_type,
                player_team,
                grid,
                access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) > 0
        {
            let inside = unit1.is_air()
                && i32::from(parent.grid.x) <= i32::from(grid.x)
                && i32::from(parent.grid.x) + 1 >= i32::from(grid.x)
                && i32::from(parent.grid.y) <= i32::from(grid.y)
                && i32::from(parent.grid.y) + 1 >= i32::from(grid.y);

            return if inside {
                CursorId::Friend
            } else {
                CursorId::Activate
            };
        }
    }

    if team_unit.is_some() {
        CursorId::Friend
    } else {
        CursorId::UnitNoGo
    }
}

/// `GameManager_GetUnitActionCursor` (`game_manager.cpp:5976`): курсор для
/// наземного/морского юнита — движение, X-FER (`CURSOR_TRANSFER`),
/// `repair` (`CURSOR_REPAIR`), `reload` (`CURSOR_RELOAD`), `load` (`CURSOR_LOAD`).
///
/// `cursor` — режим `unit->cursor` C++; в порте он живёт в
/// [`crate::hud::HudState::cursor_mode`] и передаётся сюда явно.
pub fn unit_action_cursor(
    state: &GameState,
    player_team: TeamId,
    unit1: &UnitInfo,
    grid: Point,
    team_unit: Option<&UnitInfo>,
    shift_pressed: bool,
    cursor: u8,
) -> CursorId {
    let accessible = access::is_accessible(
        state,
        unit1.unit_type,
        player_team,
        grid,
        access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
    ) > 0;

    if accessible {
        if team_unit.is_some_and(|team| team.is_air()) {
            CursorId::Friend
        } else if unit1.orders == UnitOrder::Clear {
            CursorId::UnitNoGo
        } else if shift_pressed {
            CursorId::Way
        } else {
            CursorId::UnitGo
        }
    } else if let Some(team_unit) = team_unit {
        let capacity = unit1
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_STORAGE);

        if cursor == crate::unitmenu::CURSOR_TRANSFER
            || cursor == crate::unitmenu::CURSOR_LOAD
            || cursor == crate::unitmenu::CURSOR_REPAIR
            || cursor == crate::unitmenu::CURSOR_RELOAD
        {
            // C++: `unit1->cursor == CURSOR_ARROW_SW` — 7, то есть режим `load`
            // (и `repair`/`reload`/`x-fer` попадают в `else` ниже).
            if cursor == crate::unitmenu::CURSOR_LOAD && i32::from(unit1.storage) < capacity {
                let land = access::get_team_unit(
                    state,
                    grid,
                    player_team,
                    unit_flags::MOBILE_LAND_UNIT,
                )
                .and_then(|id| state.unit(id));

                if let Some(land) = land {
                    if ready_to_take_orders(state, land) {
                        if unit1.unit_type == UnitType::ArmouredPersonnelCarrier
                            && matches!(land.unit_type, UnitType::Infiltrator | UnitType::Infantry)
                        {
                            return CursorId::Load;
                        }

                        if unit1.unit_type == UnitType::SeaTransport {
                            return CursorId::Load;
                        }
                    }
                }

                return CursorId::Friend;
            }

            if cursor != crate::unitmenu::CURSOR_LOAD {
                // Общая ветка C++: `CURSOR_ARROW_NE` — X-FER, `E` — repair,
                // `S` — reload; проверяются только для соседней цели.
                if is_unit_next_to_position(unit1, grid)
                    && unit1.id != team_unit.id
                    && team_unit.hits > 0
                {
                    return match cursor {
                        crate::unitmenu::CURSOR_TRANSFER => {
                            if transfer_target_accepts(state, unit1, team_unit) {
                                CursorId::Transfer
                            } else {
                                CursorId::Friend
                            }
                        }
                        crate::unitmenu::CURSOR_REPAIR => {
                            if i32::from(team_unit.hits)
                                < team_unit
                                    .base_values
                                    .get_attribute(max_core::unit_values::ATTRIB_HITS)
                            {
                                CursorId::Repair
                            } else {
                                CursorId::Friend
                            }
                        }
                        crate::unitmenu::CURSOR_RELOAD => {
                            if i32::from(team_unit.ammo)
                                < team_unit
                                    .base_values
                                    .get_attribute(max_core::unit_values::ATTRIB_AMMO)
                            {
                                CursorId::Reload
                            } else {
                                CursorId::Friend
                            }
                        }
                        _ => CursorId::Friend,
                    };
                }
            }
        }

        CursorId::Friend
    } else {
        CursorId::UnitNoGo
    }
}

/// Проверка `CURSOR_ARROW_NE` (`x-fer`) из `GameManager_GetUnitActionCursor`:
/// та же категория груза, цель не в `CLEAR`/`BUILD`, либо у цели-здания в
/// комплексе есть приёмник нужного груза.
fn transfer_target_accepts(state: &GameState, unit1: &UnitInfo, target: &UnitInfo) -> bool {
    let cargo = max_core::victory::cargo_type_of(unit1.unit_type);

    if max_core::victory::cargo_type_of(target.unit_type) == cargo
        && target.orders != UnitOrder::Clear
        && target.orders != UnitOrder::Build
    {
        return true;
    }

    if unit1.is_mobile() && !target.is_mobile() {
        if let Some(complex) = target.complex {
            return max_core::transfer::find_unit_with_cargo_type(
                state, target.team, complex, cargo,
            )
            .is_some();
        }
    }

    false
}

/// `GameManager_GetMilitaryCursor` (`game_manager.cpp:6150`): курсор по
/// валидной цели — `CURSOR_ENEMY`, `CURSOR_FAR_TARGET`, `CURSOR_DISABLE`,
/// `CURSOR_STEAL`, иначе `CURSOR_UNIT_NO_GO`.
pub fn military_cursor(
    state: &GameState,
    selected: &UnitInfo,
    grid: Point,
    cursor: u8,
) -> CursorId {
    if selected.delayed_reaction != 0 {
        return CursorId::UnitNoGo;
    }

    if selected.unit_type != UnitType::Infiltrator {
        if selected.weapon != 2 {
            return CursorId::UnitNoGo;
        }

        if selected.ammo == 0 && access::group_members_of(state, selected.id).is_none() {
            return CursorId::UnitNoGo;
        }
    }

    let Some(target_id) = access::get_attack_target(state, selected, grid, false) else {
        return CursorId::UnitNoGo;
    };
    let Some(target) = state.unit(target_id) else {
        return CursorId::UnitNoGo;
    };

    // Диверсант без режима наведения: рядом — steal/disable по направлению.
    if selected.unit_type == UnitType::Infiltrator && selected.targeting_mode == 0 {
        if is_unit_next_to_position(selected, grid) {
            let disable = cursor == crate::unitmenu::CURSOR_DISABLE || cursor == 0;
            let steal = cursor == crate::unitmenu::CURSOR_STEAL || cursor == 0;

            if disable
                && crate::unitmenu::is_valid_stealth_target(
                    state,
                    selected,
                    target,
                    UnitOrder::AwaitDisableUnit,
                )
            {
                return CursorId::Disable;
            }

            if steal
                && crate::unitmenu::is_valid_stealth_target(
                    state,
                    selected,
                    target,
                    UnitOrder::AwaitStealUnit,
                )
            {
                return CursorId::Steal;
            }
        }
    }

    if selected.weapon != 2 {
        return CursorId::UnitNoGo;
    }

    if selected.ammo == 0 && access::group_members_of(state, selected.id).is_none() {
        return CursorId::UnitNoGo;
    }

    let range = selected
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_RANGE);

    if access::is_within_attack_range(state, selected, grid, range) {
        if selected.ammo > 0 {
            CursorId::Enemy
        } else {
            CursorId::UnitNoGo
        }
    } else if !selected.is_mobile() {
        CursorId::UnitNoGo
    } else {
        CursorId::FarTarget
    }
}

/// `GameManager_GetWindowCursor` (`game_manager.cpp:4215-4564`) — точная
/// таблица курсоров по контексту карты.
///
/// Порядок веток 1:1 C++ (первая подходящая выигрывает):
/// 1. клетка вне карты → `HANDPTR`;
/// 2. конец хода или ход не игрока → `FRND_PTR`/`UNIT_NGO`;
/// 3. юнит не «готов к приказам» (нет выбора, движется, стреляет, взрывается,
///    отключён, грузится, масштабируется, чужой) → `FRND_PTR`/`UNIT_NGO`;
/// 4. прерванный групповой приказ → `FRND_PTR`/`UNIT_NGO`;
/// 5. `ORDER_BUILD`/`AWAIT_TAPE_POSITIONING` наземного строителя →
///    [`builder_unit_cursor`];
/// 6. `ORDER_IDLE` → [`air_unit_cursor`] (`ACTVTPTR`);
/// 7. `ORDER_STATE_EXECUTING_ORDER` и валидная цель → [`military_cursor`];
/// 8. `targeting_mode` → `FRND_PTR`/`UNIT_NGO`/`ENMY_PTR`;
/// 9. `enter_mode` → `UNIT_GO`/`UNIT_NGO`;
/// 10. по классу: воздух — `LOAD`/`FRND_PTR`/`WAY`/`UNIT_GO`;
///     земля/море — [`unit_action_cursor`]; стационарные — `LOAD` (по типу
///     здания), `TRANSFER`/`RELOAD` по курсорному режиму, иначе
///     `FRND_PTR`/`UNIT_NGO`.
pub fn window_cursor(app: &crate::App, grid: Point) -> CursorId {
    let state = app.state();
    let player_team = state.player_team;
    // `unit->cursor` C++ (режимы X-FER/repair/reload/load/steal/disable).
    let cursor_mode = app.hud().cursor_mode;
    // `targeting_mode`/`enter_mode` C++ живут в `UnitInfo`; в порте — в HUD
    // (`HudState::targeting_mode`/`enter_mode`, `unitmenu` их переключает).
    let targeting_mode = app.hud().targeting_mode;
    let enter_mode = app.hud().enter_mode;
    let team_unit_id = access::get_team_unit(state, grid, player_team, unit_flags::SELECTABLE);
    let team_unit = team_unit_id.and_then(|id| state.unit(id));
    let active_kind = state
        .teams
        .get(usize::from(state.active_turn_team.0))
        .map(|team| team.team_type)
        .unwrap_or(TeamKind::None);

    if grid.x < 0
        || grid.y < 0
        || grid.x >= max_core::geo::MAP_WIDTH as i16
        || grid.y >= max_core::geo::MAP_HEIGHT as i16
    {
        return CursorId::Hand;
    }

    if state.game_state == GameStateType::EndTurn || active_kind != TeamKind::Player {
        return if team_unit.is_some() {
            CursorId::Friend
        } else {
            CursorId::UnitNoGo
        };
    }

    let Some(selected_id) = app.hud().selected else {
        return if team_unit.is_some() {
            CursorId::Friend
        } else {
            CursorId::UnitNoGo
        };
    };
    let Some(selected) = state.unit(selected_id) else {
        return CursorId::UnitNoGo;
    };

    // Ветка 3 C++: юнит не принимает приказы.
    let not_ready = matches!(
        selected.orders,
        UnitOrder::Fire
            | UnitOrder::Explode
            | UnitOrder::Disable
            | UnitOrder::AwaitDisableUnit
            | UnitOrder::AwaitStealUnit
            | UnitOrder::Load
            | UnitOrder::AwaitScaling
    ) || selected.state == UnitOrderState::Destroy
        || (matches!(
            selected.orders,
            UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
        ) && selected.state != UnitOrderState::ExecutingOrder)
        || selected.team != player_team;

    if not_ready {
        if Some(selected.id) == team_unit_id && selected.orders != UnitOrder::Disable {
            return CursorId::UnitNoGo;
        }

        if let Some(team_unit) = team_unit {
            if selected.unit_list == team_unit.unit_list && selected.orders != UnitOrder::Idle {
                return CursorId::UnitNoGo;
            }
        }

        return if team_unit.is_some_and(|unit| unit.flags & unit_flags::GROUND_COVER == 0) {
            CursorId::Friend
        } else {
            CursorId::UnitNoGo
        };
    }

    if is_group_order_interrupted(state, selected) {
        if let Some(team_unit) = team_unit {
            if team_unit.unit_list == selected.unit_list && selected.orders != UnitOrder::Idle {
                return CursorId::UnitNoGo;
            }
        }

        return if team_unit.is_some_and(|unit| unit.flags & unit_flags::GROUND_COVER == 0) {
            CursorId::Friend
        } else {
            CursorId::UnitNoGo
        };
    }

    if matches!(
        selected.orders,
        UnitOrder::Build | UnitOrder::AwaitTapePositioning
    ) && (selected.is_ground() || selected.is_amphibious())
    {
        return builder_unit_cursor(state, player_team, selected, grid, team_unit);
    }

    if selected.orders == UnitOrder::Idle {
        return air_unit_cursor(state, player_team, selected, grid, team_unit);
    }

    if selected.state == UnitOrderState::ExecutingOrder {
        if let Some(enemy) = get_enemy_unit(state, player_team, grid, unit_flags::SELECTABLE) {
            if let Some(enemy) = state.unit(enemy) {
                if enemy.unit_type != UnitType::Connector
                    && (enemy.flags & unit_flags::GROUND_COVER == 0
                        || matches!(enemy.unit_type, UnitType::LandMine | UnitType::SeaMine))
                    && access::is_valid_attack_target(state, selected, enemy)
                {
                    let result = military_cursor(state, selected, grid, cursor_mode);

                    if result != CursorId::UnitNoGo || team_unit.is_none() {
                        return result;
                    }
                }
            }
        }
    }

    if targeting_mode {
        if Some(selected.id) == team_unit_id {
            return CursorId::Friend;
        }

        if selected.shots == 0 {
            return CursorId::UnitNoGo;
        }

        let range = selected
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_RANGE);

        if !access::is_within_attack_range(state, selected, grid, range) {
            return CursorId::UnitNoGo;
        }

        if let Some(team_unit) = team_unit {
            if !access::is_valid_attack_target(state, selected, team_unit) {
                return CursorId::UnitNoGo;
            }
        }

        // `Cursor_DrawAttackPowerCursor` в SDL-слое (полоска силы удара).
        return CursorId::Enemy;
    }

    if enter_mode {
        if Some(selected.id) == team_unit_id {
            return CursorId::Friend;
        }

        return if access::get_receiver_unit(state, selected, grid).is_some() {
            CursorId::UnitGo
        } else {
            CursorId::UnitNoGo
        };
    }

    // Класс выбирает ветку курсора (`Environment` + форма); порядок 1:1 C++.
    if selected.is_air() {
        // Погрузка: транспорт в режиме `load` (`CURSOR_ARROW_SW`) с местом.
        if cursor_mode == crate::unitmenu::CURSOR_LOAD
            && selected.state == UnitOrderState::ExecutingOrder
            && i32::from(selected.storage)
                < selected
                    .base_values
                    .get_attribute(max_core::unit_values::ATTRIB_STORAGE)
        {
            let land = access::get_team_unit(state, grid, player_team, unit_flags::MOBILE_LAND_UNIT)
                .and_then(|id| state.unit(id));

            if let Some(land) = land {
                if land.orders != UnitOrder::Clear
                    && land.orders != UnitOrder::Build
                    && access::get_team_unit(
                        state,
                        grid,
                        player_team,
                        unit_flags::MOBILE_AIR_UNIT,
                    )
                    .is_none()
                {
                    return CursorId::Load;
                }
            }
        }

        return if team_unit.is_some_and(|unit| {
            unit.unit_type != UnitType::LandingPad
                && unit.flags & unit_flags::GROUND_COVER == 0
        }) {
            CursorId::Friend
        } else if app.shift_pressed() {
            CursorId::Way
        } else {
            CursorId::UnitGo
        };
    }

    if selected.is_ground() || selected.is_sea() || selected.is_amphibious() {
        return unit_action_cursor(
            state,
            player_team,
            selected,
            grid,
            team_unit,
            app.shift_pressed(),
            cursor_mode,
        );
    }

    if !selected.is_mobile() {
        // Погрузка в здание (`CURSOR_ARROW_SW` = режим `load`).
        if cursor_mode == crate::unitmenu::CURSOR_LOAD
            && i32::from(selected.storage)
                < selected
                    .base_values
                    .get_attribute(max_core::unit_values::ATTRIB_STORAGE)
        {
            let (class_flags, only_special) = match selected.unit_type {
                UnitType::Depot => (unit_flags::MOBILE_LAND_UNIT, false),
                UnitType::Hangar => (unit_flags::MOBILE_AIR_UNIT, false),
                UnitType::Dock => (unit_flags::MOBILE_SEA_UNIT, false),
                UnitType::Barracks => (unit_flags::MOBILE_LAND_UNIT, true),
                _ => (0, false),
            };

            if class_flags != 0 {
                let candidate =
                    access::get_team_unit(state, grid, player_team, class_flags)
                        .and_then(|id| state.unit(id));

                if let Some(candidate) = candidate {
                    let special = matches!(
                        candidate.unit_type,
                        UnitType::Infiltrator | UnitType::Infantry
                    );

                    let ready = if selected.unit_type == UnitType::Barracks {
                        special
                    } else {
                        ready_to_take_orders(state, candidate)
                    };

                    let sea_ok = selected.unit_type != UnitType::Dock
                        || !(candidate.is_ground() || candidate.is_amphibious());

                    if ready && sea_ok && (!only_special || special) {
                        return CursorId::Load;
                    }
                }
            }
        }

        if cursor_mode != 0 {
            let target = access::get_team_unit(state, grid, player_team, unit_flags::SELECTABLE)
                .and_then(|id| state.unit(id));

            if let Some(target) = target {
                if is_valid_transfer_target(state, &target, selected) {
                    match cursor_mode {
                        crate::unitmenu::CURSOR_TRANSFER => {
                            let cargo =
                                max_core::victory::cargo_type_of(selected.unit_type);

                            if max_core::victory::cargo_type_of(target.unit_type) == cargo
                                && target.orders != UnitOrder::Clear
                                && target.orders != UnitOrder::Build
                            {
                                return CursorId::Transfer;
                            }
                        }
                        crate::unitmenu::CURSOR_RELOAD => {
                            if i32::from(target.ammo)
                                < target
                                    .base_values
                                    .get_attribute(max_core::unit_values::ATTRIB_AMMO)
                            {
                                return CursorId::Reload;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        return if team_unit.is_some() {
            CursorId::Friend
        } else {
            CursorId::UnitNoGo
        };
    }

    CursorId::UnitNoGo
}

/// Курсор для точки окна в партии — `GameManager_ProcessInput`
/// (`game_manager.cpp:6359-6468`): меню/панель/миникарта — `HANDPTR`
/// (`Cursor_GetDefaultWindowCursor`), карта — [`window_cursor`].
///
/// Перед таблицей карты стоят две портовые ветки, которых нет в C++ в таком
/// виде: режим помощи (`HelpMenu_Menu`, `helpmenu.cpp:318`) и призрак
/// стройплощадки (`GameManager_TempTape`; в C++ его обрабатывает
/// `GameManager_GetBuilderUnitCursor`/`SELECT_SITE`, в порте — [`crate::buildsite`],
/// т.к. приказ ещё не отдан).
pub fn session_cursor(app: &crate::App, mouse: Option<(i32, i32)>) -> (CursorId, usize) {
    let Some((x, y)) = mouse else {
        return (CursorId::Hand, 0);
    };

    // Режим помощи (`HelpMenu_Menu`): `Cursor_SetCursor(CURSOR_HELP)`
    // (`helpmenu.cpp:318`).
    if app.help_mode().is_some() {
        return (CursorId::Help, 0);
    }

    if crate::panels::panel_button_at(x, y).is_some() || max_render::layout::MINIMAP.contains(x, y)
    {
        return (CursorId::Hand, 0);
    }

    let Some((map_x, map_y)) = crate::panels::screen_to_map(x, y) else {
        return (CursorId::Hand, 0);
    };

    let viewport = crate::panels::map_viewport(app.viewport());
    let (grid_x, grid_y) = viewport.screen_to_grid(map_x, map_y);

    // Режим выбора места стройки важнее наведения на юнитов:
    // `GameManager_GetBuilderUnitCursor` возвращает `CURSOR_MAP2`/`UNIT_NO_GO`.
    if let Some(site) = app.build_site() {
        let hovered = Point::new(grid_x, grid_y);
        let anchor = site.anchor_at(app.state(), hovered);
        let valid = site.is_valid_at(app.state(), anchor, hovered);

        return (cursor_for_context(CursorContext::BuildSite { valid }), 0);
    }

    (
        window_cursor(app, Point::new(grid_x, grid_y)),
        0,
    )
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
        // Карта без выделения — `CURSOR_UNIT_NO_GO`: `GameManager_GetWindowCursor`
        // (`game_manager.cpp:4305-4335`) не имеет ветки `MAP_PTR`; `CURSOR_MAP`
        // остаётся только записью `Cursor_DefaultWindowCursorLut`, которую
        // `GameManager_ProcessInput` всегда перекрывает.
        assert_eq!(
            session_cursor(&app, Some((300, 200))).0,
            CursorId::UnitNoGo
        );
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
