//! Mouse cursors: context (`Cursor_SetCursor`) and drawing/preparation for SDL.
//!
//! The format and loading of the 30 resources — [`max_assets::cursor`] (`Cursor_Descriptor`,
//! `Cursor_ResourceLut` from `cursor.cpp:55-58`). Here — what depends on
//! the game state:
//!
//! * [`CursorId`] — indices exactly as the `enum` in `cursor.hpp`;
//! * [`CursorContext`] — a simplified `Cursor_GetDefaultWindowCursor` +
//!   `GameManager_GetWindowCursor`: menu/panel/minimap → `HANDPTR`, map without
//!   a selected unit → `MAP_PTR`, map with an order → `UNIT_GO`/`UNIT_NGO`,
//!   over an own/foreign unit → `FRND_PTR`/`ENMY_PTR`;
//! * [`CursorImage`] — an RGBA frame with a hotspot for the SDL layer (`Cursor::
//!   from_surface`), headless tests work without SDL.
//!
//! The animation (`PTR_PATH`, 10 frames) is provided frame by frame: the SDL layer itself decides
//! when to switch the frame (`Cursor_AnimationTick`, 200 ms in C++).

use max_assets::{CursorSet, CURSOR_RESOURCES};
use max_core::access;
use max_core::classification::Environment;
use max_core::enums::{unit_flags, GameStateType, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::{UnitId, UnitInfo, UnitListKind};
use max_core::universe::StateRef;
use max_render::terrain::PaletteRgba;

/// Cursor index (`CursorId` from `cursor.hpp:34-65`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CursorId {
    /// `CURSOR_HIDDEN` — the cursor is hidden.
    Hidden = 0,
    /// `CURSOR_HAND` — the usual hand arrow (menus, panels).
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
    /// `CURSOR_MAP` — the map without selection.
    Map = 10,
    /// `CURSOR_MINI` — the minimap (not used in the original, the LUT gives HANDPTR).
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
    /// `CURSOR_FRIEND` — over an own unit.
    Friend = 17,
    /// `CURSOR_ENEMY` — over an enemy (with an attack power bar).
    Enemy = 18,
    /// `CURSOR_FAR_TARGET`.
    FarTarget = 19,
    /// `CURSOR_UNIT_GO` — the order is possible.
    UnitGo = 20,
    /// `CURSOR_UNIT_NO_GO` — the order is impossible.
    UnitNoGo = 21,
    /// `CURSOR_WAY` — a route point.
    Way = 22,
    /// `CURSOR_GROUP`.
    Group = 23,
    /// `CURSOR_ACTIVATE`.
    Activate = 24,
    /// `CURSOR_MAP2` (the second `MAP_PTR` in the LUT).
    Map2 = 25,
    /// `CURSOR_STEAL`.
    Steal = 26,
    /// `CURSOR_DISABLE`.
    Disable = 27,
    /// `CURSOR_PATH` — an animated path (10 frames).
    Path = 28,
    /// `CURSOR_HELP`.
    Help = 29,
}

impl CursorId {
    /// All cursors in `Cursor_ResourceLut` order.
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

    /// Index in the resource array.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Cursor by index (`None` outside 0..30).
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// Resource name in `MAX.RES`.
    pub fn resource_name(self) -> &'static str {
        CURSOR_RESOURCES[self.index()]
    }
}

/// What is under the cursor on the map (`GameManager_GetWindowCursor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorHover {
    /// Own unit (`CURSOR_FRIEND`).
    Friend,
    /// Visible enemy (`CURSOR_ENEMY`).
    Enemy,
    /// Empty cell.
    Empty,
}

/// Cursor context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorContext {
    /// Menu/creation form (the usual arrow).
    Menu,
    /// Map without a selected unit (`CURSOR_MAP`).
    MapIdle,
    /// Map with a unit selected: hovering over a unit/an empty cell.
    MapSelected(CursorHover),
    /// Panel/button (`HANDPTR`).
    Panel,
    /// Minimap (in C++ default — `HANDPTR`).
    Minimap,
    /// The order is possible/impossible.
    Order {
        /// Whether the order can be executed.
        valid: bool,
    },
    /// Cursor mode `load` (`unit->cursor = 7`): `CURSOR_LOAD` over a target
    /// that can board a transport (`GameManager_GetWindowCursor`).
    Load {
        /// The target under the cursor is accepted by the transport.
        valid: bool,
    },
    /// Commando "capture" cursor mode (`cursor = 8`) — `CURSOR_STEAL`.
    Steal {
        /// The target is a valid capture target.
        valid: bool,
    },
    /// Commando "disable" cursor mode (`cursor = 9`) — `CURSOR_DISABLE`.
    Disable {
        /// The target is a valid disable target.
        valid: bool,
    },
    /// Targeting mode (`targeting_mode`): `CURSOR_ENEMY` over the target,
    /// `CURSOR_UNIT_NO_GO` outside the radius/without shots.
    Targeting {
        /// Whether the cell is within the radius and whether there are shots.
        valid: bool,
    },
    /// Build site selection mode (`ORDER_STATE_SELECT_SITE`):
    /// `CURSOR_MAP2`/`CURSOR_UNIT_NO_GO` (`GameManager_GetBuilderUnitCursor`).
    BuildSite {
        /// Whether the cursor is inside the 2×2 and whether the site is passable.
        valid: bool,
    },
    /// Help (`CURSOR_HELP`).
    Help,
}

/// Cursor for a context — like `Cursor_SetCursor` in `game_manager.cpp:6449-6468`.
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

/// RGBA cursor frame with a hotspot — for `Cursor::from_surface` (SDL3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorImage {
    /// Frame width.
    pub width: u32,
    /// Frame height.
    pub height: u32,
    /// Hotspot x (`Cursor_Descriptor::ulx`).
    pub hot_x: i32,
    /// Hotspot y (`Cursor_Descriptor::uly`).
    pub hot_y: i32,
    /// RGBA8 pixels (`width * height * 4`), transparent — alpha 0.
    pub rgba: Vec<u8>,
}

impl CursorImage {
    /// RGBA pixel by frame coordinates.
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

/// Prepares the cursor frame for SDL: window palette indices → RGBA.
///
/// Returns `None` if the cursor was not loaded or there is no frame.
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

/// `GameManager_IsUnitNextToPosition` (`game_manager.cpp:3208`): a cell in
/// the neighborhood of a unit (for a 2×2 building — expanded by 1 in all directions).
pub fn is_unit_next_to_position(unit: &UnitInfo, grid: Point) -> bool {
    let size = if unit.is_quad() { 2 } else { 1 };

    i32::from(unit.grid.x) - 1 <= i32::from(grid.x)
        && i32::from(unit.grid.x) + size >= i32::from(grid.x)
        && i32::from(unit.grid.y) - 1 <= i32::from(grid.y)
        && i32::from(unit.grid.y) + size >= i32::from(grid.y)
}

/// `Task_IsReadyToTakeOrders` (`task.cpp:105`).
///
/// `Access_GetActiveLoader(unit) == nullptr` is modeled in the port by
/// [`max_core::access::is_engaged`] (a unit that is being loaded/led does not accept
/// orders).
pub fn ready_to_take_orders(state: StateRef<'_>, unit: &UnitInfo) -> bool {
    if unit.hits == 0 {
        return false;
    }

    let order_ok = matches!(unit.orders, UnitOrder::Await | UnitOrder::Sentry)
        || (matches!(unit.orders, UnitOrder::Move | UnitOrder::MoveToUnit)
            && unit.state == UnitOrderState::ExecutingOrder);

    order_ok && !state.is_engaged(unit)
}

/// `Access_GetEnemyUnit(team, x, y, flags)` (`access.cpp:1731`): a visible enemy in
/// the cell (first regular units, then ground cover/mines).
pub fn get_enemy_unit(
    state: StateRef<'_>,
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

/// The tail of `Access_GetTeamUnit`/`Access_GetEnemyUnit`: the tape belongs to
/// the builder (`unit = unit->GetParent()`).
fn resolve_tape_parent(state: StateRef<'_>, id: UnitId) -> Option<UnitId> {
    let unit = state.unit(id)?;

    if matches!(unit.unit_type, UnitType::LargeTape | UnitType::SmallTape) {
        unit.parent_unit
    } else {
        Some(id)
    }
}

/// `GameManager_IsValidTransferTarget` (`game_manager.cpp:6084`).
///
/// C++ compares `Complex*` pointers; the port's [`max_core::team::ComplexId`] is
/// unique only within a team, so the team of the complex member is compared too —
/// otherwise an enemy complex with the same numeric id would accept a transfer.
pub fn is_valid_transfer_target(state: StateRef<'_>, unit1: &UnitInfo, unit2: &UnitInfo) -> bool {
    if unit1.id == unit2.id {
        return false;
    }

    if let Some(complex) = unit2.complex {
        for id in state.unit_list(UnitListKind::Stationary) {
            let Some(other) = state.unit(*id) else {
                continue;
            };

            if other.complex == Some(complex)
                && other.team == unit2.team
                && is_unit_next_to_position(other, unit1.grid)
            {
                return true;
            }
        }

        false
    } else {
        is_unit_next_to_position(unit2, unit1.grid)
    }
}

/// `GameManager_IsGroupOrderInterrupted` (`access.cpp:2390`): the group has
/// a unit that is still moving.
pub fn is_group_order_interrupted(state: StateRef<'_>, unit: &UnitInfo) -> bool {
    let Some(members) = state.group_members_of(unit.id) else {
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
/// The site selection branch (`SELECT_SITE`) comes here only from
/// `GameManager_GetWindowCursor`; in the port the site ghost lives in
/// [`crate::buildsite`] and is checked before this function is called
/// ([`session_cursor`]).
pub fn builder_unit_cursor(
    state: StateRef<'_>,
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
            && state.is_accessible(
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

/// `GameManager_GetAirUnitCursor` (`game_manager.cpp:5937`): the order
/// `ORDER_IDLE` of the unit (tape/inside a building) — `CURSOR_ACTIVATE`.
pub fn air_unit_cursor(
    state: StateRef<'_>,
    player_team: TeamId,
    unit1: &UnitInfo,
    grid: Point,
    team_unit: Option<&UnitInfo>,
) -> CursorId {
    let parent = unit1.parent_unit.and_then(|id| state.unit(id));

    if let Some(parent) = parent {
        if is_unit_next_to_position(parent, grid)
            && state.is_accessible(
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

/// `GameManager_GetUnitActionCursor` (`game_manager.cpp:5976`): the cursor for
/// a ground/sea unit — movement, X-FER (`CURSOR_TRANSFER`),
/// `repair` (`CURSOR_REPAIR`), `reload` (`CURSOR_RELOAD`), `load` (`CURSOR_LOAD`).
///
/// `cursor` — the C++ `unit->cursor` mode; in the port it lives in
/// [`crate::hud::HudState::cursor_mode`] and is passed here explicitly.
pub fn unit_action_cursor(
    state: StateRef<'_>,
    player_team: TeamId,
    unit1: &UnitInfo,
    grid: Point,
    team_unit: Option<&UnitInfo>,
    shift_pressed: bool,
    cursor: u8,
) -> CursorId {
    let accessible = state.is_accessible(
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
            // C++: `unit1->cursor == CURSOR_ARROW_SW` — 7, i.e. the `load` mode
            // (and `repair`/`reload`/`x-fer` fall into `else` below).
            if cursor == crate::unitmenu::CURSOR_LOAD && i32::from(unit1.storage) < capacity {
                let land = state.get_team_unit_by_environments(
                    grid,
                    player_team,
                    &[Environment::Ground, Environment::Amphibious],
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
                // The common C++ branch: `CURSOR_ARROW_NE` — X-FER, `E` — repair,
                // `S` — reload; checked only for an adjacent target.
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

/// `CURSOR_ARROW_NE` (`x-fer`) check from `GameManager_GetUnitActionCursor`:
/// the same cargo category, the target is not in `CLEAR`/`BUILD`, or the target building
/// has a receiver of the needed cargo in the complex.
fn transfer_target_accepts(state: StateRef<'_>, unit1: &UnitInfo, target: &UnitInfo) -> bool {
    let cargo = max_core::victory::cargo_type_of(unit1.unit_type);

    if max_core::victory::cargo_type_of(target.unit_type) == cargo
        && target.orders != UnitOrder::Clear
        && target.orders != UnitOrder::Build
    {
        return true;
    }

    if unit1.is_mobile() && !target.is_mobile() {
        if let Some(complex) = target.complex {
            return state.find_unit_with_cargo_type( target.team, complex, cargo,
            )
            .is_some();
        }
    }

    false
}

/// `GameManager_GetMilitaryCursor` (`game_manager.cpp:6150`): the cursor on
/// a valid target — `CURSOR_ENEMY`, `CURSOR_FAR_TARGET`, `CURSOR_DISABLE`,
/// `CURSOR_STEAL`, otherwise `CURSOR_UNIT_NO_GO`.
pub fn military_cursor(
    state: StateRef<'_>,
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

        if selected.ammo == 0 && state.group_members_of(selected.id).is_none() {
            return CursorId::UnitNoGo;
        }
    }

    let Some(target_id) = state.get_attack_target( selected, grid, false) else {
        return CursorId::UnitNoGo;
    };
    let Some(target) = state.unit(target_id) else {
        return CursorId::UnitNoGo;
    };

    // A commando without targeting mode: nearby — steal/disable by direction.
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

    if selected.ammo == 0 && state.group_members_of(selected.id).is_none() {
        return CursorId::UnitNoGo;
    }

    let range = selected
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_RANGE);

    if state.is_within_attack_range( selected, grid, range) {
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

/// `GameManager_GetWindowCursor` (`game_manager.cpp:4215-4564`) — the exact
/// table of cursors by map context.
///
/// The branch order is 1:1 C++ (the first suitable one wins):
/// 1. a cell outside the map → `HANDPTR`;
/// 2. end of turn or not the player's turn → `FRND_PTR`/`UNIT_NGO`;
/// 3. the unit is not "ready for orders" (no selection, moving, firing, exploding,
///    disabled, loading, scaling, foreign) → `FRND_PTR`/`UNIT_NGO`;
/// 4. an interrupted group order → `FRND_PTR`/`UNIT_NGO`;
/// 5. `ORDER_BUILD`/`AWAIT_TAPE_POSITIONING` of a ground builder →
///    [`builder_unit_cursor`];
/// 6. `ORDER_IDLE` → [`air_unit_cursor`] (`ACTVTPTR`);
/// 7. `ORDER_STATE_EXECUTING_ORDER` and a valid target → [`military_cursor`];
/// 8. `targeting_mode` → `FRND_PTR`/`UNIT_NGO`/`ENMY_PTR`;
/// 9. `enter_mode` → `UNIT_GO`/`UNIT_NGO`;
/// 10. by class: air — `LOAD`/`FRND_PTR`/`WAY`/`UNIT_GO`;
///     ground/sea — [`unit_action_cursor`]; stationary — `LOAD` (by the building
///     type), `TRANSFER`/`RELOAD` by the cursor mode, otherwise
///     `FRND_PTR`/`UNIT_NGO`.
pub fn window_cursor(app: &crate::App, grid: Point) -> CursorId {
    let state = app.state_ref();
    let player_team = state.player_team();
    // C++ `unit->cursor` (X-FER/repair/reload/load/steal/disable modes).
    let cursor_mode = app.hud().cursor_mode;
    // C++ `targeting_mode`/`enter_mode` live in `UnitInfo`; in the port — in the HUD
    // (`HudState::targeting_mode`/`enter_mode`, `unitmenu` toggles them).
    let targeting_mode = app.hud().targeting_mode;
    let enter_mode = app.hud().enter_mode;
    let team_unit_id = state.get_team_unit( grid, player_team, unit_flags::SELECTABLE);
    let team_unit = team_unit_id.and_then(|id| state.unit(id));
    let active_kind = state
        .teams()
        .get(usize::from(state.active_turn_team().0))
        .map(|team| team.team_type)
        .unwrap_or(TeamKind::None);

    if grid.x < 0
        || grid.y < 0
        || grid.x >= max_core::geo::MAP_WIDTH as i16
        || grid.y >= max_core::geo::MAP_HEIGHT as i16
    {
        return CursorId::Hand;
    }

    if state.game_state() == GameStateType::EndTurn || active_kind != TeamKind::Player {
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

    // C++ branch 3: the unit does not accept orders.
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
                    && state.is_valid_attack_target( selected, enemy)
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

        if !state.is_within_attack_range( selected, grid, range) {
            return CursorId::UnitNoGo;
        }

        if let Some(team_unit) = team_unit {
            if !state.is_valid_attack_target( selected, team_unit) {
                return CursorId::UnitNoGo;
            }
        }

        // `Cursor_DrawAttackPowerCursor` in the SDL layer (the attack power bar).
        return CursorId::Enemy;
    }

    if enter_mode {
        if Some(selected.id) == team_unit_id {
            return CursorId::Friend;
        }

        return if state.get_receiver_unit( selected, grid).is_some() {
            CursorId::UnitGo
        } else {
            CursorId::UnitNoGo
        };
    }

    // The class selects the cursor branch (`Environment` + shape); the order is 1:1 C++.
    if selected.is_air() {
        // Loading: a transport in `load` mode (`CURSOR_ARROW_SW`) with room.
        if cursor_mode == crate::unitmenu::CURSOR_LOAD
            && selected.state == UnitOrderState::ExecutingOrder
            && i32::from(selected.storage)
                < selected
                    .base_values
                    .get_attribute(max_core::unit_values::ATTRIB_STORAGE)
        {
            let land = state.get_team_unit_by_environments(
                grid,
                player_team,
                &[Environment::Ground, Environment::Amphibious],
            )
            .and_then(|id| state.unit(id));

            if let Some(land) = land {
                if land.orders != UnitOrder::Clear
                    && land.orders != UnitOrder::Build
                    && state.get_team_unit_by_environments(
                        grid,
                        player_team,
                        &[Environment::Air],
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
        // Loading into a building (`CURSOR_ARROW_SW` = `load` mode).
        if cursor_mode == crate::unitmenu::CURSOR_LOAD
            && i32::from(selected.storage)
                < selected
                    .base_values
                    .get_attribute(max_core::unit_values::ATTRIB_STORAGE)
        {
            // C++ bits: `Depot`/`Barracks` — `MOBILE_LAND_UNIT` (land +
            // amphibians), `Hangar` — `MOBILE_AIR_UNIT`, `Dock` —
            // `MOBILE_SEA_UNIT` (sea + amphibians).
            let (environments, only_special): (&[Environment], bool) = match selected.unit_type {
                UnitType::Depot => (&[Environment::Ground, Environment::Amphibious], false),
                UnitType::Hangar => (&[Environment::Air], false),
                UnitType::Dock => (&[Environment::Sea, Environment::Amphibious], false),
                UnitType::Barracks => (&[Environment::Ground, Environment::Amphibious], true),
                _ => (&[], false),
            };

            if !environments.is_empty() {
                let candidate =
                    state.get_team_unit_by_environments( grid, player_team, environments)
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
            let target = state.get_team_unit( grid, player_team, unit_flags::SELECTABLE)
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

/// Cursor for a window point in the match — `GameManager_ProcessInput`
/// (`game_manager.cpp:6359-6468`): menu/panel/minimap — `HANDPTR`
/// (`Cursor_GetDefaultWindowCursor`), map — [`window_cursor`].
///
/// Before the map table there are two port branches that C++ does not have in this
/// form: the help mode (`HelpMenu_Menu`, `helpmenu.cpp:318`) and the build site
/// ghost (`GameManager_TempTape`; in C++ it is handled by
/// `GameManager_GetBuilderUnitCursor`/`SELECT_SITE`, in the port — [`crate::buildsite`],
/// because the order has not been issued yet).
pub fn session_cursor(app: &crate::App, mouse: Option<(i32, i32)>) -> (CursorId, usize) {
    let Some((x, y)) = mouse else {
        return (CursorId::Hand, 0);
    };

    // Help mode (`HelpMenu_Menu`): `Cursor_SetCursor(CURSOR_HELP)`
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

    // The build site selection mode is more important than hovering over units:
    // `GameManager_GetBuilderUnitCursor` returns `CURSOR_MAP2`/`UNIT_NO_GO`.
    if let Some(site) = app.build_site() {
        let hovered = Point::new(grid_x, grid_y);
        let anchor = site.anchor_at(app.state_ref(), hovered);
        let valid = site.is_valid_at(app.state_ref(), anchor, hovered);

        return (cursor_for_context(CursorContext::BuildSite { valid }), 0);
    }

    (
        window_cursor(app, Point::new(grid_x, grid_y)),
        0,
    )
}

/// Whether the cursor has an animation (for the SDL tick).
pub fn frame_count(cursors: &CursorSet, id: CursorId) -> usize {
    cursors
        .get(id.index())
        .map_or(0, |cursor| usize::from(cursor.frame_count))
}

/// The minimap TNT filter condition — extracted for tests and reuse.
pub fn has_firing_sprite(flags: u32) -> bool {
    flags & (unit_flags::HAS_FIRING_SPRITE | unit_flags::FIRES_MISSILES) != 0
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
