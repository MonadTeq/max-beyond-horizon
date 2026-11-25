//! Command menu of the selected unit (popup `UNTBTN_U/D`) — 1:1 with the C++ port.
//!
//! Oracle: `max-cpp-port/src/game_manager.cpp`
//!
//! * `GameManager_InitPopupButtons` (`game_manager.cpp:3568`) —
//!   opening conditions, layout, button pictures/labels;
//! * `GameManager_DeinitPopupButtons` (`:3664`) — closing;
//! * `GameManager_ProcessPopupMenuInput` (`:2607`) — keys: the position character
//!   or the first letter of the label;
//! * `GameManager_ClickUnit` (`:6265`) / `GameManager_UnitSelect` (`:6246`) —
//!   trigger (a repeated click on an already selected unit);
//! * `game_manager.cpp:5455` (`Esc`) — closing the menu;
//! * `units_manager.cpp:543-1352` — the composition of the items:
//!   `UnitsManager_Popup_InitCommons` and `UnitsManager_Popup_Init*` by unit
//!   type (`UnitInfo::Init`, `unitinfo.cpp:454-582` — the table
//!   `UnitsManager_PopupCallbacks[0..22]`).
//!
//! # Trigger (exact, from C++)
//!
//! The menu opens on a **repeated click on an already selected own unit**:
//! `GameManager_ClickUnit` gets `unit1 == unit2` (a click on the same unit) and
//! calls `GameManager_InitPopupButtons`; if the menu is already open — closes it
//! (`DeinitPopupButtons`). The left button on another own unit only
//! re-selects. The right button does **not** open the menu: on the selected unit it
//! opens statistics (`UnitStats_Menu`), on another — selects it.
//!
//! `GameManager_InitPopupButtons` requires:
//!
//! * `unit->GetOrder() != ORDER_DISABLE`;
//! * the unit is inside the visible map area;
//! * for `MOBILE_LAND_UNIT` — the state is not `ORDER_STATE_UNIT_READY`;
//! * `popup->init` registered at least one item.
//!
//! # Layout (per C++)
//!
//! A vertical list of `UNTBTN_U` buttons (42×21, `BUTTON_WIDTH`×`BUTTON_HEIGHT`)
//! with a step of `height + 3`; the window is `image.width + 6` × `image.height * count +
//! (count + 1) * 3`, i.e. 3 px of margin and 3 px between buttons. Anchor:
//! `ulx = unit->x + 32` (the right edge of the unit cell), `uly = unit->y - height/2`
//! (the cell center by Y; `unit->x/y` — the cell center, `UnitInfo::SetPosition`).
//! If the menu goes beyond the right edge of the map window — shift left by
//! `width + Gfx_ZoomLevel` (one cell); vertically — within the map window.
//!
//! Label — `Button::SetCaption(caption, -1, 2)`: the `GNW_TEXT_FONT_2` font,
//! color `COLOR_GREEN` (`0x02`), centered in the button; the pressed item is drawn
//! with the `UNTBTN_D` picture (C++: `state[i]` → `image_up = UNTBTN_D`).
//!
//! # Input
//!
//! * mouse: a click on a button — the item (C++ `Button` with `r_func`);
//! * keyboard: `toupper(position)` or `toupper(first letter of the label)`
//!   (`GameManager_ProcessPopupMenuInput`); arrows/Enter in C++ **do not**
//!   work — neither here;
//! * `Esc` — close (C++ `game_manager.cpp:5469`);
//! * a second click on the unit — close (C++ `GameManager_ClickUnit`);
//! * a click outside the menu — close (the menu is modal: the map receives no order).
//!
//! # Item composition
//!
//! The order of the items is not the registration order but `UnitsManager_RegisterButton`:
//! insertion by ascending `key_code` (the position character, `'0'` → `':'`, so that
//! "remove"/"place" come last); equal positions keep the registration order
//! (stable insertion). Therefore the tank menu goes
//! `attack → x-fer → enter → stop → sentry → Done`, and the builder —
//! `Build → …`.
//!
//! # Implemented / disabled
//!
//! An item whose mechanics are not ported in our stack remains in the list
//! (as in C++), but is drawn gray and does not trigger — the reason is in
//! [`UnitMenuItem::note`]. Since 2026-11-20 (B1) `attack` (targeting_mode),
//! `load` (cursor mode 7), `disable`/`steal` (modes 9/8), bulldozer
//! (`ORDER_CLEAR`), `remove` (`UnitsManager_SelfDestructMenu`) and `Upg. All` are enabled.
//! Only `activate` is disabled (`RepairShopMenu` is not ported); the master builder
//! does not register the item (`FindValidPowerGeneratorPosition`). The list of reasons and
//! references — `max-brh-docs/analysis/visual-1to1-status.md` §33.
//!
//! # Deliberate divergences
//!
//! 1. For a stationary unit C++ opens the menu already on selection
//!    (`GameManager_UnitSelect` → `InitPopupButtons`). Here selection does not
//!    change the frame (golden `mine-selected` and others), so the menu opens only
//!    on a repeated click — like `GameManager_ClickUnit`, see the trigger above.
//! 2. `Done` (`UnitsManager_PerformAction`) — orders in our stack are
//!    executed immediately when issued, there is no separate "confirm" step: the item
//!    closes the menu and clears the selection (plus switches `Await` +
//!    `STATIONARY` to `ORDER_STATE_READY_TO_EXECUTE_ORDER`, like C++).
//! 3. C++ cursor modes (`unit->cursor`: x-fer 3, repair 4, reload 6,
//!    load 7) live in the UI state ([`crate::hud::HudState`]), not in
//!    `UnitInfo`: `UnitInfo::cursor` is not part of the snapshot, but must not
//!    affect the digest either. Switching a mode — like C++ (`cursor = N`).

use max_assets::image::SimpleImage;
use max_core::enums::{unit_flags, PlayMode, UnitOrder, UnitOrderState, UnitType};
use max_core::universe::StateRef;
use max_core::geo::Point;
use max_core::unit::{UnitId, UnitInfo};
use max_core::unit_values::{
    ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_HITS, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN,
    ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS,
};
use max_core::victory::{cargo_type_of, CargoType};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, GnwFontId};

use crate::panels::UiPalette;
use crate::ui::Key;
use crate::Assets;

/// Popup button width (`UNTBTN_U`/`UNTBTN_D`, `MAX.RES`): 42.
pub const BUTTON_WIDTH: i32 = 42;
/// Popup button height (`UNTBTN_U`/`UNTBTN_D`, `MAX.RES`): 21.
pub const BUTTON_HEIGHT: i32 = 21;
/// Window margin and gap between buttons (`(count + 1) * 3` in C++): 3.
pub const MARGIN: i32 = 3;

/// Label color — `COLOR_GREEN` (`color.h:38`, `SetCaption(..., -1, 2)`).
pub const CAPTION_GREEN: u8 = 0x02;
/// Color of a disabled item (in C++ a disabled button does not react and is drawn
/// with the same picture; we need a visible sign — gray `0xA2`).
pub const CAPTION_DISABLED: u8 = 0xA2;

/// Cursor mode `x-fer` — C++ `unit->cursor = 3` (`CURSOR_TRANSFER`).
pub const CURSOR_TRANSFER: u8 = 3;
/// Cursor mode `repair` — C++ `unit->cursor = 4` (`CURSOR_REPAIR`).
pub const CURSOR_REPAIR: u8 = 4;
/// Cursor mode `reload` — C++ `unit->cursor = 6` (`CURSOR_RELOAD`).
pub const CURSOR_RELOAD: u8 = 6;
/// Cursor mode `load` — C++ `unit->cursor = 7` (`CURSOR_LOAD`).
pub const CURSOR_LOAD: u8 = 7;

/// Menu item identifier (all items of the C++ popup).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemId {
    /// `Manual` (`_(b0b9)`) — the return fire toggle in simultaneous moves.
    Manual,
    /// `x-fer` (`_(886b)`) — the cargo transfer cursor mode.
    Transfer,
    /// `enter` (`_(e08d)`) — the boarding mode into a transport/platform.
    Enter,
    /// `stop` (`_(6c45)`) — cancels the route (`ORDER_AWAIT` + `CLEAR_PATH`).
    Stop,
    /// `upgrade` (`_(7fef)`) — the `ORDER_UPGRADE` order.
    Upgrade,
    /// `Upg. All` (`_(97a2)`) — upgrade all buildings of the complex.
    UpgradeAll,
    /// `sentry` (`_(0cf6)`/`_(524e)`) — `ORDER_SENTRY` / `ORDER_AWAIT`.
    Sentry,
    /// `Done` (`_(2b7d)`) — `UnitsManager_PerformAction`.
    Done,
    /// `remove` (`_(7e56)`) — building self-destruct (`SelfDestructMenu`).
    RemoveStationary,
    /// `Auto` (`_(0286)`) — surveyor auto-survey.
    Auto,
    /// `attack` (`_(e183)`/`_(5f6b)`) — the targeting mode.
    Attack,
    /// `disable` (`_(6d6a)`) — commando: disable the target.
    Disable,
    /// `steal` (`_(a0fd)`) — commando: capture the target.
    Steal,
    /// `reload` (`_(9d39)`) — the refuel/recharge cursor mode.
    Reload,
    /// `place` (`_(81df)`) — placing a mine.
    PlaceMine,
    /// `remove` (`_(bfe6)`) — removing a mine.
    RemoveMine,
    /// `repair` (`_(a1a1)`) — the repair cursor mode.
    Repair,
    /// `Build` (`_(7b6d)`/`_(b3ef)`) — the order screen.
    Build,
    /// `stop` (`_(bf85)`/`_(bcf7)`) — cancels construction (`GameManager_SelectBuildSite`).
    StopBuild,
    /// `start` (`_(cced)`) — resumes factory construction (`BuildOrder`).
    StartBuild,
    /// `start` (`_(e7b6)`/`_(283e)`/`_(98c9)`/`_(53c3)`/`_(bd97)`/`_(5fe9)`).
    PowerOn,
    /// `stop` (`_(7a07)`/`_(6889)`/`_(d9c8)`/`_(3f13)`/`_(df26)`).
    PowerOff,
    /// `resrch` (`_(2e00)`) — the research screen.
    Research,
    /// `buy upg` (`_(1da0)`) — the upgrades screen.
    BuyUpgrade,
    /// `allocate` (`_(33b7)`) — mine allocation (`AllocMenu`).
    Allocate,
    /// `activate` (`_(200c)`/`_(cec1)`) — `RepairShopMenu`.
    Activate,
    /// `load` (`_(8452)`) — the boarding cursor mode into a transport.
    Load,
    /// `stop` (`_(bc20)`) — cancels bulldozer clearing.
    StopRemove,
    /// `remove` (`_(9737)`) — clears rubble with a bulldozer.
    RemoveRubble,
    /// `start` (`_(2c2a)`) — master builder (`UnitsManager_FindValidPowerGeneratorPosition`).
    StartMaster,
}

impl ItemId {
    /// Item name for the log/QA (coincides with the en-US label, except
    /// `Upg. All`/`buy upg` — there the space is replaced with `_`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Transfer => "x-fer",
            Self::Enter => "enter",
            Self::Stop => "stop",
            Self::Upgrade => "upgrade",
            Self::UpgradeAll => "Upg. All",
            Self::Sentry => "sentry",
            Self::Done => "Done",
            Self::RemoveStationary => "remove",
            Self::Auto => "Auto",
            Self::Attack => "attack",
            Self::Disable => "disable",
            Self::Steal => "steal",
            Self::Reload => "reload",
            Self::PlaceMine => "place",
            Self::RemoveMine => "remove",
            Self::Repair => "repair",
            Self::Build => "Build",
            Self::StopBuild => "stop build",
            Self::StartBuild => "start",
            Self::PowerOn => "power on",
            Self::PowerOff => "power off",
            Self::Research => "resrch",
            Self::BuyUpgrade => "buy upg",
            Self::Allocate => "allocate",
            Self::Activate => "activate",
            Self::Load => "load",
            Self::StopRemove => "stop remove",
            Self::RemoveRubble => "remove rubble",
            Self::StartMaster => "master",
        }
    }
}

/// A menu item — the `UNTBTN_U/D` button with a label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitMenuItem {
    /// What the item does.
    pub id: ItemId,
    /// Label (en-US from `language.json`, like `_(...)` in C++).
    pub caption: String,
    /// Position character for the keyboard (`UnitsManager_RegisterButton::position`).
    pub hotkey: char,
    /// C++ `state[i]`: the pressed look (`UNTBTN_D`) — the mode is on.
    pub pressed: bool,
    /// The item triggers; `false` — the mechanics are not ported (gray).
    pub enabled: bool,
    /// Why the item is disabled (for the log/documentation); `""` — enabled.
    pub note: &'static str,
}

impl UnitMenuItem {
    fn new(id: ItemId, caption: &str, hotkey: char, pressed: bool) -> Self {
        Self {
            id,
            caption: caption.to_string(),
            hotkey,
            pressed,
            enabled: true,
            note: "",
        }
    }

    fn disabled(mut self, note: &'static str) -> Self {
        self.enabled = false;
        self.note = note;
        self
    }
}

/// C++ stable sort key (`'0'` → `':'`, so that "remove" is last).
pub fn key_code(position: char) -> char {
    if position == '0' {
        ':'
    } else {
        position
    }
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// Menu pictures: `UNTBTN_U` (released) and `UNTBTN_D` (pressed), 42×21.
#[derive(Debug, Clone)]
pub struct UnitMenuAssets {
    /// Released button.
    pub up: Option<SimpleImage>,
    /// Pressed button (C++: `state[i]` → `image_up = UNTBTN_D`).
    pub down: Option<SimpleImage>,
}

impl UnitMenuAssets {
    /// Loads `UNTBTN_U/D` from `MAX.RES` (missing — `None`, the menu is not drawn).
    pub fn load(max_res: &max_assets::ResArchive) -> Self {
        Self {
            up: max_res
                .read("UNTBTN_U")
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok()),
            down: max_res
                .read("UNTBTN_D")
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok()),
        }
    }

    /// Both pictures are loaded.
    pub fn is_ready(&self) -> bool {
        self.up.is_some() && self.down.is_some()
    }
}

// ---------------------------------------------------------------------------
// Item composition
// ---------------------------------------------------------------------------

/// The set of items by unit type — C++ `UnitsManager_PopupCallbacks[i].init`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitKind {
    /// `UnitsManager_Popup_InitCommons` — common items only.
    Commons,
    /// `InitSurveyor` — `Auto`.
    Surveyor,
    /// `InitMilitary` — `attack`.
    Military,
    /// `InitInfiltrator` — `disable`, `steal`, `attack`.
    Infiltrator,
    /// `InitReloaders` — `reload`.
    Reloaders,
    /// `InitMineLayers` — `place`, `remove`.
    MineLayers,
    /// `InitRepair` — `repair`.
    Repair,
    /// `InitBuilders` — `Build`/`stop`.
    Builders,
    /// `InitFactories` — `Build`, `start`/`stop`.
    Factories,
    /// `InitRecreationCenter` — `start` when the power is off.
    RecreationCenter,
    /// `InitMiningStation` — `allocate`/`start` + `stop`.
    MiningStation,
    /// `InitEcoSphere` — `start`/`stop`.
    EcoSphere,
    /// `InitResearch` — `resrch` + `stop`, or `start`.
    Research,
    /// `InitPowerGenerators` — `start`/`stop`.
    PowerGenerators,
    /// `InitRemove` — `remove`/`stop` (bulldozer).
    Remove,
    /// `InitMaster` — the master builder `start`.
    Master,
    /// `InitGoldRefinery` — `start`/`stop` + `buy upg` (for `COMMTWR` — the same
    /// function, `unitinfo.cpp:456`).
    GoldRefinery,
    /// `InitRepairShops` — `activate` + `load`.
    RepairShops,
}

/// The `UnitInfo::Init` table (`unitinfo.cpp:454-582`): type → item set.
fn init_kind(unit_type: UnitType) -> InitKind {
    use UnitType::*;

    match unit_type {
        // `COMMTWR` → `UnitsManager_PopupCallbacks[22]` (`unitinfo.cpp:456`).
        GoldRefinery => InitKind::GoldRefinery,
        PowerStation | PowerGenerator => InitKind::PowerGenerators,
        Barracks | Depot | Hangar | Dock => InitKind::RepairShops,
        MaterialStorage | FuelStorage => InitKind::Commons,
        Shipyard | LightVehiclePlant | HeavyVehiclePlant | AirUnitsPlant | TrainingHall => {
            InitKind::Factories
        }
        ResearchCentre => InitKind::Research,
        EcoSphere => InitKind::EcoSphere,
        RecreationCenter => InitKind::RecreationCenter,
        // `UnitsManager_PopupCallbacks[3]` (`InitMilitary`): the complete oracle
        // list — `GUNTURRT`…`ALNPLANE`, including `SCOUT` (`unitinfo.cpp:499-522`).
        // Without `Scout` here it fell through to `InitCommons` and its popup had
        // no `attack` item at all (manual-run defect: the Scout could not attack).
        // `SCANNER` is *not* military: the oracle maps it to callbacks `[1]`
        // (`InitCommons` with the `IsReady` test, `unitinfo.cpp:546-549`).
        GunTurret | AntiAir | Artillery | MissileLauncher | Scout | Tank | AssaultGun
        | RocketLauncher | MissileCrawler | MobileAntiAir | Infantry | Escort | Corvette
        | Gunboat | Submarine | MissileCruiser | Fighter | Bomber | Juggernaut | AlienTank
        | AlienAssaultGun | AlienPlane => InitKind::Military,
        // `SCANNER`/`GOLDTRCK` → `UnitsManager_PopupCallbacks[1]` — common items.
        Scanner | GoldTruck => InitKind::Commons,
        MiningStation => InitKind::MiningStation,
        MasterBuilder => InitKind::Master,
        Constructor | Engineer => InitKind::Builders,
        MineLayer | SeaMineLayer => InitKind::MineLayers,
        Surveyor => InitKind::Surveyor,
        GoldVault => InitKind::GoldRefinery,
        SupplyTruck | CargoShip => InitKind::Reloaders,
        Bulldozer => InitKind::Remove,
        RepairUnit => InitKind::Repair,
        FuelTruck => InitKind::Commons,
        ArmouredPersonnelCarrier | SeaTransport | AirTransport => InitKind::RepairShops,
        Infiltrator => InitKind::Infiltrator,
        // `default:` — `UnitsManager_PopupCallbacks[0]` (`InitCommons`).
        _ => InitKind::Commons,
    }
}

/// `UnitInfo::HasUpgradeableAttributes` (`unitinfo.cpp:1403`).
fn has_upgradeable_attributes(state: StateRef<'_>, unit: &UnitInfo) -> bool {
    let Some(team_units) = state.team_units().get(unit.team.0 as usize) else {
        return false;
    };
    let current = team_units.current_unit_values(unit.unit_type as u16);
    let base = &unit.base_values;

    let better = |attribute: u8| base.get_attribute(attribute) < current.get_attribute(attribute);

    better(ATTRIB_HITS)
        || better(ATTRIB_ATTACK)
        || better(ATTRIB_ARMOR)
        || better(ATTRIB_SPEED)
        || better(ATTRIB_RANGE)
        || better(ATTRIB_ROUNDS)
        || better(ATTRIB_SCAN)
        || better(ATTRIB_AMMO)
        // Fewer turns to build is also an upgrade (`ATTRIB_TURNS`).
        || (base.get_attribute(ATTRIB_TURNS) > current.get_attribute(ATTRIB_TURNS)
            && current.get_attribute(ATTRIB_TURNS) >= 1)
}

/// `UnitInfo::IsUpgradeAvailable` (`unitinfo.cpp:1423`): stationary only.
pub fn is_upgrade_available(state: StateRef<'_>, unit: &UnitInfo) -> bool {
    !unit.is_mobile() && has_upgradeable_attributes(state, unit)
}

/// Common items — `UnitsManager_Popup_InitCommons` (`units_manager.cpp:543`).
fn init_commons(state: StateRef<'_>, unit: &UnitInfo, items: &mut Vec<UnitMenuItem>) {
    let order = unit.orders;

    // `Manual`: simultaneous moves only, with ammo and not a commando.
    if state.play_mode() == PlayMode::SimultaneousMoves
        && unit.base_values.get_attribute(ATTRIB_AMMO) > 0
        && unit.unit_type != UnitType::Infiltrator
    {
        items.push(UnitMenuItem::new(
            ItemId::Manual,
            "Manual",
            '4',
            unit.disabled_reaction_fire,
        ));
    }

    // `x-fer`: for units with cargo `RAW..GOLD`, except `CLEAR`/`BUILD`.
    let cargo = cargo_type_of(unit.unit_type);
    if (cargo as u8) > (CargoType::None as u8)
        && (cargo as u8) <= (CargoType::Gold as u8)
        && order != UnitOrder::Clear
        && order != UnitOrder::Build
    {
        items.push(UnitMenuItem::new(ItemId::Transfer, "x-fer", '3', false));
    }

    // `enter`: for mobile ones, except `CLEAR`/`BUILD`.
    if unit.environment().is_some()
        && order != UnitOrder::Clear
        && order != UnitOrder::Build
    {
        items.push(UnitMenuItem::new(ItemId::Enter, "enter", '5', false));
    }

    // `stop`: only when there is a route.
    if unit.path.is_some() && order != UnitOrder::Clear && order != UnitOrder::Build {
        items.push(UnitMenuItem::new(ItemId::Stop, "stop", '7', false));
    }

    // `upgrade`/`Upg. All`.
    if is_upgrade_available(state, unit) {
        items.push(UnitMenuItem::new(ItemId::Upgrade, "upgrade", '5', false));
        items.push(UnitMenuItem::new(ItemId::UpgradeAll, "Upg. All", '6', false));
    }

    // `sentry` + `Done`.
    if unit.flags & unit_flags::SENTRY_UNIT != 0 {
        if unit.unit_type != UnitType::Infiltrator {
            match order {
                UnitOrder::Sentry => items.push(UnitMenuItem::new(
                    ItemId::Sentry,
                    "sentry",
                    '8',
                    true,
                )),
                UnitOrder::Await => items.push(UnitMenuItem::new(
                    ItemId::Sentry,
                    "sentry",
                    '8',
                    false,
                )),
                _ => {}
            }
        }

        items.push(UnitMenuItem::new(ItemId::Done, "Done", '9', false));
    }

    // `remove` — building self-destruct (`UnitsManager_SelfDestructMenu`).
    if !unit.is_mobile() {
        items.push(UnitMenuItem::new(ItemId::RemoveStationary, "remove", '0', false));
    }
}

fn power_items(unit: &UnitInfo, on_caption: &str, items: &mut Vec<UnitMenuItem>) {
    if unit.orders == UnitOrder::PowerOff {
        items.push(UnitMenuItem::new(ItemId::PowerOn, on_caption, '2', false));
    } else {
        items.push(UnitMenuItem::new(ItemId::PowerOff, "stop", '7', false));
    }
}

/// Items by unit type (`UnitsManager_Popup_Init*`).
///
/// `false` — do not add the common items: in C++ `UnitsManager_Popup_InitFactories`
/// for a factory with a ready unit calls `PlaceNewUnit` and exits without building the popup.
fn init_for_kind(kind: InitKind, unit: &UnitInfo, items: &mut Vec<UnitMenuItem>) -> bool {
    match kind {
        InitKind::Commons => {}
        InitKind::Surveyor => {
            items.push(UnitMenuItem::new(ItemId::Auto, "Auto", '1', unit.auto_survey));
        }
        InitKind::Military => {
            // `UnitsManager_Popup_OnClick_TargetingMode` (`units_manager.cpp:617`).
            items.push(UnitMenuItem::new(ItemId::Attack, "attack", '3', false));
        }
        InitKind::Infiltrator => {
            // `UnitsManager_Popup_InitInfiltrator` (`units_manager.cpp:932`):
            // `disable`/`steal` — cursor modes 9/8, `attack` — targeting.
            items.push(UnitMenuItem::new(
                ItemId::Disable,
                "disable",
                '1',
                false,
            ));
            items.push(UnitMenuItem::new(ItemId::Steal, "steal", '2', false));
            items.push(UnitMenuItem::new(ItemId::Attack, "attack", '3', false));
        }
        InitKind::Reloaders => {
            items.push(UnitMenuItem::new(ItemId::Reload, "reload", '1', false));
        }
        InitKind::MineLayers => {
            items.push(UnitMenuItem::new(
                ItemId::PlaceMine,
                "place",
                '1',
                unit.laying_state == 2,
            ));
            items.push(UnitMenuItem::new(
                ItemId::RemoveMine,
                "remove",
                '0',
                unit.laying_state == 1,
            ));
        }
        InitKind::Repair => {
            items.push(UnitMenuItem::new(ItemId::Repair, "repair", '1', false));
        }
        InitKind::Builders => {
            if unit.orders == UnitOrder::Build
                && unit.state != UnitOrderState::BuildCancel
                && unit.state != UnitOrderState::BuildAbort
            {
                // C++ `UnitsManager_Popup_OnClick_SelectBuildSite` →
                // `GameManager_SelectBuildSite`: cancels the current construction —
                // `ORDER_BUILD` + `ORDER_STATE_BUILD_CANCEL`
                // (`UnitInfo::CancelBuilding` for a mobile builder clears
                // `build_list`/`build_time` and returns `ORDER_AWAIT`).
                items.push(UnitMenuItem::new(ItemId::StopBuild, "stop", '7', false));
            } else {
                items.push(UnitMenuItem::new(ItemId::Build, "Build", '1', false));
            }
        }
        InitKind::Factories => {
            // C++: for a factory with a ready unit the popup is not built at all —
            // `UnitsManager_Popup_PlaceNewUnit` places the unit immediately.
            if unit.state == UnitOrderState::UnitReady {
                return false;
            }

            items.push(UnitMenuItem::new(ItemId::Build, "Build", '1', false));

            if unit.orders == UnitOrder::HaltBuilding || unit.orders == UnitOrder::HaltBuilding2 {
                // C++ `UnitsManager_Popup_OnClick_BuildOrder` → `UnitInfo::BuildOrder`:
                // `ORDER_BUILD` + `ORDER_STATE_INIT` (the queue and `build_time`
                // are preserved by the stop), then `StartBuilding`.
                items.push(UnitMenuItem::new(ItemId::StartBuild, "start", '2', false));
            } else if unit.orders == UnitOrder::Build
                && unit.state != UnitOrderState::BuildCancel
                && unit.state != UnitOrderState::BuildAbort
            {
                items.push(UnitMenuItem::new(ItemId::StopBuild, "stop", '7', false));
            }
        }
        InitKind::RecreationCenter => {
            if unit.orders == UnitOrder::PowerOff {
                items.push(UnitMenuItem::new(ItemId::PowerOn, "start", '2', false));
            }
        }
        InitKind::MiningStation => {
            if unit.orders == UnitOrder::PowerOff {
                items.push(UnitMenuItem::new(ItemId::PowerOn, "start", '2', false));
            } else {
                items.push(UnitMenuItem::new(ItemId::Allocate, "allocate", '1', false));
                items.push(UnitMenuItem::new(ItemId::PowerOff, "stop", '7', false));
            }
        }
        InitKind::EcoSphere => power_items(unit, "start", items),
        InitKind::Research => {
            if unit.orders == UnitOrder::PowerOn {
                items.push(UnitMenuItem::new(ItemId::Research, "resrch", '1', false));
                items.push(UnitMenuItem::new(ItemId::PowerOff, "stop", '7', false));
            } else {
                items.push(UnitMenuItem::new(ItemId::PowerOn, "start", '2', false));
            }
        }
        InitKind::PowerGenerators => power_items(unit, "start", items),
        InitKind::Remove => {
            // `UnitsManager_Popup_InitRemove` (`units_manager.cpp:1123`):
            // in `ORDER_CLEAR` — `stop` (`_(bc20)`), otherwise `remove` (`_(9737)`).
            if unit.orders == UnitOrder::Clear {
                items.push(UnitMenuItem::new(ItemId::StopRemove, "stop", '7', false));
            } else {
                items.push(UnitMenuItem::new(
                    ItemId::RemoveRubble,
                    "remove",
                    '0',
                    false,
                ));
            }
        }
        InitKind::Master => {
            // `UnitsManager_FindValidPowerGeneratorPosition` (the master builder site
            // check) is not ported — there is no item, as with
            // `is_placeable == false`.
        }
        InitKind::GoldRefinery => {
            power_items(unit, "start", items);
            items.push(UnitMenuItem::new(ItemId::BuyUpgrade, "buy upg", '1', false));
        }
        InitKind::RepairShops => {
            let caption = if unit.orders == UnitOrder::PowerOff {
                "start"
            } else {
                "activate"
            };
            // `UnitsManager_Popup_InitRepairShops` (`units_manager.cpp:910-921`):
            // `activate` opens `RepairShopMenu`. For an `AIRTRANS` the menu's
            // `Activate` unloads one stored passenger
            // (`RepairShopMenu::Activate`, `repairshopmenu.cpp:661-678`:
            // `repairshop->SetParent(target); ORDER_UNLOAD/INIT`), so the item is
            // live while the hold is not empty. Other repair shops need the
            // unported screen (`FindReachableSpot`/`FollowUnit`) and stay disabled.
            let activate = UnitMenuItem::new(ItemId::Activate, caption, '1', false);

            items.push(if unit.unit_type == UnitType::AirTransport {
                if unit.storage > 0 {
                    activate
                } else {
                    activate.disabled("пустой трюм: нечего выгружать")
                }
            } else {
                activate.disabled(
                    "RepairShopMenu_Menu не портирован: экран repairshopmenu.cpp:144-1141",
                )
            });
            // `UnitsManager_Popup_OnClick_Load` (`units_manager.cpp:1011`):
            // mode 7 toggle; boarding — `GameManager_ProcessMouse`
            // (CURSOR_LOAD, `game_manager.cpp:6807-6842`).
            items.push(UnitMenuItem::new(ItemId::Load, "load", '2', false));
        }
    }

    true
}

/// Menu item list of a unit — C++ `popup->init(unit, &GameManager_PopupButtons)`.
///
/// An empty list — the menu does not open (C++ `popup_count == 0`).
pub fn items_for(state: StateRef<'_>, unit: &UnitInfo) -> Vec<UnitMenuItem> {
    let mut items = Vec::new();

    if init_for_kind(init_kind(unit.unit_type), unit, &mut items) {
        init_commons(state, unit, &mut items);
    }
    // `UnitsManager_RegisterButton`: stable insertion by ascending key.
    items.sort_by_key(|item| key_code(item.hotkey));

    items
}

/// Opening condition — C++ `GameManager_InitPopupButtons` (`game_manager.cpp:3576`).
pub fn can_open(unit: &UnitInfo) -> bool {
    unit.orders != UnitOrder::Disable
        && (!(unit.is_ground() || unit.is_amphibious())
            || unit.state != UnitOrderState::UnitReady)
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

/// Open command menu of one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitMenu {
    /// Whose unit.
    pub unit: UnitId,
    /// Items in drawing order (top to bottom).
    pub items: Vec<UnitMenuItem>,
    /// Menu window in frame pixels (`GameManager_PopupButtons` ulx/uly/width/height).
    pub rect: PixelRect,
    /// Index of the item under the cursor (C++ has no highlight, needed for QA/debug).
    pub hover: Option<usize>,
}

impl UnitMenu {
    /// Menu layout — `GameManager_InitPopupButtons` (`game_manager.cpp:3586-3617`).
    ///
    /// `anchor` — `(unit->x + 32, unit->y)` in map area pixels, i.e.
    /// the right edge of the unit cell by X and its center by Y; `area` — the map window,
    /// `zoom` — `Gfx_ZoomLevel` (pixels per cell).
    pub fn layout(anchor: (i32, i32), count: usize, area: PixelRect, zoom: u32) -> PixelRect {
        let count = count.max(1) as i32;
        let width = BUTTON_WIDTH + 2 * MARGIN;
        let height = BUTTON_HEIGHT * count + (count + 1) * MARGIN;

        let mut ulx = anchor.0;
        let mut uly = anchor.1 - height / 2;

        if ulx + width > area.lrx() {
            ulx -= width + zoom as i32;
        }

        ulx = ulx.max(area.ulx);

        if uly < area.uly {
            uly = area.uly;
        } else if uly + height > area.lry() {
            uly = area.lry() - height;
        }

        PixelRect::new(ulx, uly, width, height)
    }

    /// Opens the menu for a unit; `None` — no items/the menu is not allowed.
    ///
    /// `anchor`/`area` — in frame pixels (already with the map window offset).
    pub fn new(
        state: StateRef<'_>,
        unit: &UnitInfo,
        anchor: (i32, i32),
        area: PixelRect,
        zoom: u32,
    ) -> Option<Self> {
        if !can_open(unit) {
            return None;
        }

        let items = items_for(state, unit);

        if items.is_empty() {
            return None;
        }

        let rect = Self::layout(anchor, items.len(), area, zoom);

        Some(Self {
            unit: unit.id,
            items,
            rect,
            hover: None,
        })
    }

    /// Rectangle of button `index` (C++: `ulx += 3`, `uly += image->height + 3`).
    pub fn button_rect(&self, index: usize) -> PixelRect {
        PixelRect::new(
            self.rect.ulx + MARGIN,
            self.rect.uly + MARGIN + index as i32 * (BUTTON_HEIGHT + MARGIN),
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
        )
    }

    /// Index of the item under the frame point.
    pub fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.items.len()).find(|index| self.button_rect(*index).contains(x, y))
    }

    /// Index of the item by identifier.
    pub fn index_of(&self, id: ItemId) -> Option<usize> {
        self.items.iter().position(|item| item.id == id)
    }

    /// Index of the item by label (case-insensitive) or by the position character.
    ///
    /// C++ searches the same way: `toupper(position)` or `toupper(caption[0])`.
    pub fn index_by_text(&self, text: &str) -> Option<usize> {
        let wanted = text.trim().to_ascii_lowercase();
        let mut chars = wanted.chars();

        if let (Some(character), None) = (chars.next(), chars.next()) {
            if let Some(index) = self
                .items
                .iter()
                .position(|item| item.hotkey.eq_ignore_ascii_case(&character))
            {
                return Some(index);
            }
        }

        self.items.iter().position(|item| {
            item.caption.eq_ignore_ascii_case(&wanted)
                || item.id.name().eq_ignore_ascii_case(&wanted)
        })
    }

    /// Key in the open menu — `GameManager_ProcessPopupMenuInput`.
    ///
    /// First the comparison with the position character, then with the first letter of
    /// the label (`toupper`); arrows/Enter are not handled, `Esc` — close.
    pub fn key(&self, key: Key) -> Option<MenuKey> {
        match key {
            Key::Escape => Some(MenuKey::Close),
            Key::Char(character) => {
                let upper = character.to_ascii_uppercase();
                let index = self
                    .items
                    .iter()
                    .position(|item| item.hotkey.to_ascii_uppercase() == upper)
                    .or_else(|| {
                        self.items.iter().position(|item| {
                            item.caption
                                .chars()
                                .next()
                                .is_some_and(|first| first.to_ascii_uppercase() == upper)
                        })
                    });

                index.map(MenuKey::Item)
            }
            _ => None,
        }
    }

    /// Draws the menu over the frame (`UNTBTN_U/D` + `FONT2` label in `GREEN`).
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets, palette: &UiPalette) -> u32 {
        let Some(menu_assets) = assets.unit_menu.as_ref() else {
            return 0;
        };
        let (Some(up), Some(down)) = (menu_assets.up.as_ref(), menu_assets.down.as_ref()) else {
            return 0;
        };
        let font = GnwFontId::Font2.font();
        let mut painted = 0;

        for (index, item) in self.items.iter().enumerate() {
            let rect = self.button_rect(index);
            let image = if item.pressed { down } else { up };
            painted +=
                crate::menu::blit_simple(frame, palette, image, rect.ulx, rect.uly, true);

            let color = if item.enabled {
                palette.rgba(CAPTION_GREEN)
            } else {
                palette.rgba(CAPTION_DISABLED)
            };
            let drawn = draw_text_gnw_centered(frame, rect, font, &item.caption, color);

            if drawn > 0 {
                painted += 1;
            }
        }

        painted
    }
}

/// Result of a key in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    /// `Esc` — close.
    Close,
    /// Item `index` (position or first letter of the label).
    Item(usize),
}

/// What to do with an input event in the open menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEvent {
    /// The event does not concern the menu (mouse movement etc.).
    None,
    /// Hover over an item (for debug/frame highlight).
    Hover(Option<usize>),
    /// Close the menu (Esc, click outside, repeated click on the unit).
    Close,
    /// Activate item `index`.
    Item(usize),
}

/// Input event in the open menu — like C++ `Button` in the `WINDOW_MAIN_MAP` window.
///
/// A click **outside** the menu closes it (modality: the map receives no order).
pub fn menu_event(menu: &UnitMenu, event: crate::ui::InputEvent) -> MenuEvent {
    match event {
        crate::ui::InputEvent::MouseMove(x, y) => MenuEvent::Hover(menu.item_at(x, y)),
        crate::ui::InputEvent::MouseClick(x, y) => match menu.item_at(x, y) {
            Some(index) if menu.items[index].enabled => MenuEvent::Item(index),
            Some(_) => MenuEvent::None,
            None => MenuEvent::Close,
        },
        crate::ui::InputEvent::RightClick(..) => MenuEvent::Close,
        crate::ui::InputEvent::Key(key) => match menu.key(key) {
            Some(MenuKey::Close) => MenuEvent::Close,
            Some(MenuKey::Item(index)) => {
                if menu.items[index].enabled {
                    MenuEvent::Item(index)
                } else {
                    MenuEvent::None
                }
            }
            None => MenuEvent::None,
        },
    }
}

// ---------------------------------------------------------------------------
// Orders from the menu (packets 08, processors P1/P2/P3)
// ---------------------------------------------------------------------------

/// `P1` order (packet 08) — C++ `Remote_SendNetPacket_08` with processor 1.
pub fn p1_order(
    state: StateRef<'_>,
    id: UnitId,
    order: UnitOrder,
    order_state: UnitOrderState,
) -> Option<max_core::input::Command> {
    let unit = state.unit(id)?;

    Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: id,
            payload: max_core::input::OrderPayload::P1(max_core::input::PayloadP1 {
                order,
                state: order_state,
                prior_order: unit.orders,
                prior_state: unit.state,
                disabled_reaction_fire: unit.disabled_reaction_fire,
            }),
        },
    ))
}

/// `P2` order (packet 08) — with a parent (`ORDER_REPAIR`/`ORDER_RELOAD`).
pub fn p2_order(
    state: StateRef<'_>,
    id: UnitId,
    order: UnitOrder,
    order_state: UnitOrderState,
    parent: Option<UnitId>,
) -> Option<max_core::input::Command> {
    let unit = state.unit(id)?;

    Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: id,
            payload: max_core::input::OrderPayload::P2(max_core::input::PayloadP2 {
                p1: max_core::input::PayloadP1 {
                    order,
                    state: order_state,
                    prior_order: unit.orders,
                    prior_state: unit.state,
                    disabled_reaction_fire: unit.disabled_reaction_fire,
                },
                parent_unit: parent,
                experience: unit.experience,
            }),
        },
    ))
}

/// `P3` order (packet 08) — with a destination cell (`ORDER_AWAIT`/`MOVE_TO_UNIT`).
pub fn p3_order(
    state: StateRef<'_>,
    id: UnitId,
    order: UnitOrder,
    order_state: UnitOrderState,
    move_to: Point,
    enemy: Option<UnitId>,
) -> Option<max_core::input::Command> {
    let unit = state.unit(id)?;
    let data = state.order_data(id);

    Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: id,
            payload: max_core::input::OrderPayload::P3(max_core::input::PayloadP3 {
                p2: max_core::input::PayloadP2 {
                    p1: max_core::input::PayloadP1 {
                        order,
                        state: order_state,
                        prior_order: unit.orders,
                        prior_state: unit.state,
                        disabled_reaction_fire: unit.disabled_reaction_fire,
                    },
                    parent_unit: unit.parent_unit,
                    experience: unit.experience,
                },
                move_to,
                fire_on: data.fire_on,
                enemy_unit: enemy,
                transfer_cargo: unit.transfer_cargo,
                stealth_dice_roll: unit.stealth_dice_roll,
            }),
        },
    ))
}

/// Cursor click on an own unit with an active `cursor` mode (C++ `CURSOR_*`
/// branches of `GameManager_ProcessMouse`).
///
/// `repair` (`game_manager.cpp:4617`) and `reload` (`:4600`): cargo is required on
/// the selected one (`unit1->storage`), the target is not `ORDER_DISABLE`; the parent
/// becomes the target, the order — `ORDER_REPAIR`/`ORDER_RELOAD`, state `INIT`.
///
/// `x-fer` (`cursor = 3`) is handled in the UI by the
/// [`crate::transfermenu`] (`GameManager_TransferCargo` → `TransferMenu_Menu`):
/// here it does not turn into an order. `load` — [`load_mode_click`],
/// `steal`/`disable` — [`stealth_mode_click`] (a click on an enemy, see `ui.rs`).
pub fn cursor_mode_click(
    state: StateRef<'_>,
    selected: UnitId,
    target: UnitId,
    cursor: u8,
) -> Result<Option<max_core::input::Command>, String> {
    let Some(unit) = state.unit(selected) else {
        return Ok(None);
    };
    let Some(target_unit) = state.unit(target) else {
        return Ok(None);
    };

    if selected == target || target_unit.team != unit.team {
        return Ok(None);
    }

    let (order, label) = match cursor {
        CURSOR_REPAIR => (UnitOrder::Repair, "Repair"),
        CURSOR_RELOAD => (UnitOrder::Reload, "Reload"),
        _ => return Ok(None),
    };

    if target_unit.orders == UnitOrder::Disable {
        return Err(format!("{label}: цель отключена"));
    }

    if unit.storage == 0 {
        return Err(format!("{label}: у юнита нет груза (storage = 0)"));
    }

    Ok(p2_order(
        state,
        selected,
        order,
        UnitOrderState::Init,
        Some(target),
    ))
}

/// "Board the transport" order by click (C++ `CURSOR_UNIT_GO` + `enter_mode`):
/// `ORDER_MOVE_TO_UNIT` into the target cell, like `GameManager_SetUnitOrder`.
pub fn enter_mode_click(
    state: StateRef<'_>,
    selected: UnitId,
    target: UnitId,
) -> Option<max_core::input::Command> {
    let unit = state.unit(selected)?;
    let target_unit = state.unit(target)?;

    if selected == target || target_unit.team != unit.team {
        return None;
    }

    p3_order(
        state,
        selected,
        UnitOrder::MoveToUnit,
        UnitOrderState::Init,
        target_unit.grid,
        None,
    )
}

/// Commando "capture" cursor mode — C++ `unit->cursor = 8`
/// (`CURSOR_ARROW_W`, `UnitsManager_Popup_OnClick_Steal`).
pub const CURSOR_STEAL: u8 = 8;
/// Commando "disable" cursor mode — C++ `unit->cursor = 9`
/// (`CURSOR_ARROW_NW`, `UnitsManager_Popup_OnClick_Disable`).
pub const CURSOR_DISABLE: u8 = 9;

/// `UnitsManager_IsValidStealthTarget` (`units_manager.cpp:1370`) — what in general
/// can be a commando target: an electronic unit, not own, alive, not in flight,
/// not `IDLE`/`TRANSFORM`/`FIRE`/`EXPLODE`, adjacent and valid as an attack
/// target. For `ORDER_AWAIT_STEAL_UNIT` — also not stationary/not occupied
/// by a transport with cargo.
pub fn is_valid_stealth_target(
    state: StateRef<'_>,
    unit: &UnitInfo,
    target: &UnitInfo,
    order: UnitOrder,
) -> bool {
    let target_order = target.orders;

    if target.team == unit.team
        || target.hits == 0
        || target.flags & unit_flags::ELECTRONIC_UNIT == 0
        || (target.is_air() && target.flags & unit_flags::HOVERING != 0)
        || target_order == UnitOrder::Idle
        || (target_order == UnitOrder::AwaitScaling && target.prior_orders == UnitOrder::Idle)
        || target_order == UnitOrder::Transform
        || target_order == UnitOrder::Fire
        || target_order == UnitOrder::Explode
        || target.state == UnitOrderState::Destroy
        || !max_core::access::is_adjacent_to_grid(target, unit.grid)
        || !state.is_valid_attack_target( unit, target)
    {
        return false;
    }

    if order == UnitOrder::AwaitStealUnit {
        let transport_with_cargo = matches!(
            target.unit_type,
            UnitType::ArmouredPersonnelCarrier | UnitType::SeaTransport | UnitType::AirTransport
        ) && target.storage != 0;

        target.is_mobile() && !transport_with_cargo
    } else {        target_order != UnitOrder::Disable
    }
}

/// Click on an enemy unit in commando cursor mode (8/9) —
/// `GameManager_ProcessMouse` → `GameManager_StealUnit`/`GameManager_DisableUnit`
/// (`game_manager.cpp:4680/4696`).
///
/// Sets `parent = target`, `stealth_dice_roll = Randomizer_Generate(101)` and
/// `ORDER_AWAIT_STEAL_UNIT`/`ORDER_AWAIT_DISABLE_UNIT` + `INIT` (packet 08/P3).
/// `Err` — the commando has no action left for this turn (`_(f37a)`/`_(3e56)`).
pub fn stealth_mode_click(
    state: StateRef<'_>,
    selected: UnitId,
    target: UnitId,
    cursor: u8,
    stealth_dice_roll: u8,
) -> Result<Option<max_core::input::Command>, String> {
    let (order, message) = match cursor {
        CURSOR_STEAL => (
            UnitOrder::AwaitStealUnit,
            "Infiltrator has already used his action this turn. Try again next turn.",
        ),
        CURSOR_DISABLE => (
            UnitOrder::AwaitDisableUnit,
            "Infiltrator has already used his action this turn. Try again next turn.",
        ),
        _ => return Ok(None),
    };

    let Some(unit) = state.unit(selected) else {
        return Ok(None);
    };
    if selected == target || unit.unit_type != UnitType::Infiltrator || unit.shots == 0 {
        return Err(message.to_string());
    }

    // `Access_GetAttackTarget(unit1, target->grid)` (e.g. a platform target).
    let Some(target_unit) = state.unit(target) else {
        return Ok(None);
    };
    let target_id = if is_valid_stealth_target(state, unit, target_unit, order) {
        target
    } else if let Some(other) =
        state.get_attack_target( unit, target_unit.grid, false)
    {
        let Some(other_unit) = state.unit(other) else {
            return Ok(None);
        };
        if !is_valid_stealth_target(state, unit, other_unit, order) {
            return Ok(None);
        }
        other
    } else {
        return Ok(None);
    };

    let unit = state.unit(selected).expect("юнит проверен");
    let payload = max_core::input::PayloadP3 {
        p2: max_core::input::PayloadP2 {
            p1: max_core::input::PayloadP1 {
                order,
                state: UnitOrderState::Init,
                prior_order: unit.orders,
                prior_state: unit.state,
                disabled_reaction_fire: unit.disabled_reaction_fire,
            },
            parent_unit: Some(target_id),
            experience: unit.experience,
        },
        move_to: unit.target_grid,
        fire_on: state.order_data(selected).fire_on,
        enemy_unit: None,
        transfer_cargo: unit.transfer_cargo,
        stealth_dice_roll,
    };

    Ok(Some(max_core::input::Command::Order(
        max_core::input::Packet08Order {
            unit: selected,
            payload: max_core::input::OrderPayload::P3(payload),
        },
    )))
}

/// Conditions for showing `CURSOR_LOAD` over the target — `GameManager_GetWindowCursor`
/// (`game_manager.cpp:4416-4518`): the selected one has room, and the target is the type
/// that this transport/bay accepts.
pub fn load_mode_applies(selected: &UnitInfo, target: &UnitInfo) -> bool {
    if selected.id == target.id || selected.team != target.team || target.hits == 0 {
        return false;
    }

    let storage = selected.base_values.get_attribute(ATTRIB_STORAGE);
    if i32::from(selected.storage) >= storage {
        return false;
    }

    // `MOBILE_*` bits include amphibians: `is_ground()/is_sea()` — pure ones only.
    let target_land = target.is_ground() || target.is_amphibious();
    let target_air = target.is_air();
    let target_sea = target.is_sea() || target.is_amphibious();
    let infantry = matches!(target.unit_type, UnitType::Infiltrator | UnitType::Infantry);

    match selected.unit_type {
        UnitType::AirTransport => target_land,
        UnitType::ArmouredPersonnelCarrier => infantry,
        UnitType::SeaTransport => target_land,
        UnitType::Depot => target_land && !infantry,
        UnitType::Hangar => target_air,
        UnitType::Dock => target_sea && !target_land,
        UnitType::Barracks => infantry,
        _ => false,
    }
}

/// Click in cursor mode `load` (`cursor = 7`) —
/// `GameManager_ProcessMouse`, the `CURSOR_LOAD` branch (`game_manager.cpp:6807-6842`).
///
/// * `AIRTRANS`: the transport flies to the target cell (`parent = target`);
/// * the target is disabled — message `_(9897)`;
/// * the target is already in the transport cell and ready — `ORDER_LAND`/`INIT` with
///   `parent = transport` (the passenger "folds" into cargo);
/// * otherwise — the **target** gets `ORDER_MOVE_TO_UNIT` into the transport cell
///   (`GameManager_Unit` in C++, not the selected unit).
pub fn load_mode_click(
    state: StateRef<'_>,
    selected: UnitId,
    target: UnitId,
) -> Result<Option<max_core::input::Command>, String> {
    let Some(unit) = state.unit(selected) else {
        return Ok(None);
    };
    let Some(target_unit) = state.unit(target) else {
        return Ok(None);
    };

    if selected == target || unit.team != target_unit.team || target_unit.hits == 0 {
        return Ok(None);
    }

    if i32::from(unit.storage) >= unit.base_values.get_attribute(ATTRIB_STORAGE) {
        return Ok(None);
    }

    if target_unit.orders == UnitOrder::Disable {
        return Err("Unable to load disabled units.".to_string());
    }

    if unit.unit_type == UnitType::AirTransport {
        let payload = max_core::input::PayloadP3 {
            p2: max_core::input::PayloadP2 {
                p1: max_core::input::PayloadP1 {
                    order: UnitOrder::Move,
                    state: UnitOrderState::Init,
                    prior_order: unit.orders,
                    prior_state: unit.state,
                    disabled_reaction_fire: unit.disabled_reaction_fire,
                },
                parent_unit: Some(target),
                experience: unit.experience,
            },
            move_to: target_unit.grid,
            fire_on: state.order_data(selected).fire_on,
            enemy_unit: None,
            transfer_cargo: unit.transfer_cargo,
            stealth_dice_roll: unit.stealth_dice_roll,
        };

        return Ok(Some(max_core::input::Command::Order(
            max_core::input::Packet08Order {
                unit: selected,
                payload: max_core::input::OrderPayload::P3(payload),
            },
        )));
    }

    let same_cell = unit.grid == target_unit.grid
        && target_unit.state == UnitOrderState::ExecutingOrder;

    if same_cell {
        return Ok(p2_order(
            state,
            target,
            UnitOrder::Land,
            UnitOrderState::Init,
            Some(selected),
        ));
    }

    Ok(p3_order(
        state,
        target,
        UnitOrder::MoveToUnit,
        UnitOrderState::Init,
        unit.grid,
        None,
    ))
}

/// `Access_GetRemovableRubble` (`access.cpp:1817`): rubble in the cell that
/// the bulldozer can remove. `LRGRUBLE` requires the other three cells of the 2×2
/// to be passable for `BULLDOZR` (`Access_IsAccessible` with
/// `AccessModifier_SameClassBlocks`).
pub fn removable_rubble(state: StateRef<'_>, team: max_core::team::TeamId, grid: Point) -> Option<UnitId> {
    for id in state.units_at(grid).iter().copied() {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        match unit.unit_type {
            UnitType::SmallRubble => return Some(id),
            UnitType::LargeRubble => {
                for y in unit.grid.y..=unit.grid.y + 1 {
                    for x in unit.grid.x..=unit.grid.x + 1 {
                        if x == grid.x && y == grid.y {
                            continue;
                        }

                        if state.is_accessible(
                            UnitType::Bulldozer,
                            team,
                            Point::new(x, y),
                            max_core::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                        ) == 0
                        {
                            return None;
                        }
                    }
                }

                return Some(id);
            }
            _ => {}
        }
    }

    None
}

/// `ORDER_CLEAR` order for the bulldozer — `UnitsManager_Popup_OnClick_StopRemove`
/// (`units_manager.cpp:1082-1121`): the parent is the rubble, `build_time` = 4 for
/// a building and 1 for other rubble.
pub fn clear_order(
    state: StateRef<'_>,
    selected: UnitId,
    rubble: UnitId,
) -> Option<max_core::input::Command> {
    p3_order(
        state,
        selected,
        UnitOrder::Clear,
        UnitOrderState::Init,
        state.unit(rubble).map(|unit| unit.grid).unwrap_or(Point::ZERO),
        None,
    )
}

/// The "mark number" line — `UnitInfo::GetVersion` (`unitinfo.cpp:2930`):
/// Roman numerals of the `UnitValues` version (`Mk I`, `Mk II`, …).
pub fn roman_version(mut version: u16) -> String {
    fn digit(out: &mut String, count: u16, one: &str, five: &str, ten: &str) {
        match count {
            0 => {}
            1 => out.push_str(one),
            2 => out.push_str(&one.repeat(2)),
            3 => out.push_str(&one.repeat(3)),
            4 => {
                out.push_str(one);
                out.push_str(five);
            }
            5 => out.push_str(five),
            6 => {
                out.push_str(five);
                out.push_str(one);
            }
            7 => {
                out.push_str(five);
                out.push_str(&one.repeat(2));
            }
            8 => {
                out.push_str(five);
                out.push_str(&one.repeat(3));
            }
            _ => {
                out.push_str(one);
                out.push_str(ten);
            }
        }
    }

    let mut out = String::new();
    digit(&mut out, version / 100, "C", "D", "M");
    version %= 100;
    digit(&mut out, version / 10, "X", "L", "C");
    version %= 10;
    digit(&mut out, version, "I", "V", "X");

    out
}

/// Result of `Upg. All` — orders and the message (`UnitsManager_Popup_OnClick_UpgradeAll`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeAllReport {
    /// `ORDER_UPGRADE`/`EXECUTING_ORDER` (P2) to each upgraded building.
    pub commands: Vec<max_core::input::Command>,
    /// How many buildings were upgraded.
    pub count: usize,
    /// Raw material spent by complexes.
    pub material_cost: i32,
    /// Message to the player (`_(a0ee)`/`_(8967)`/`_(2693)`).
    pub message: String,
}

/// `UnitsManager_Popup_OnClick_UpgradeAll` (`units_manager.cpp:414-496`):
/// upgrade **all** buildings of own team of the same type as the selected one, by
/// complexes, debiting the cost from the complex materials (`GetCargoInfo`).
///
/// `get_normal_rate_build_cost(unit) / 4` — `UnitInfo::GetUpgradeCost()`.
pub fn upgrade_all_orders(state: StateRef<'_>, unit_id: UnitId) -> Option<UpgradeAllReport> {
    let unit = state.unit(unit_id)?;
    let team = unit.team;
    let unit_type = unit.unit_type;
    let upgrade_cost = max_core::victory::get_normal_rate_build_cost(unit) / 4;

    let ids: Vec<UnitId> = state
        .unit_list(max_core::unit::UnitListKind::Stationary)
        .to_vec();
    let mut complexes: Vec<max_core::team::ComplexId> = Vec::new();
    let mut costs: Vec<i32> = Vec::new();
    let mut commands = Vec::new();
    let mut material_cost = 0;
    let mut count = 0;
    let mut last: Option<UnitId> = None;

    for id in ids {
        let Some(other) = state.unit(id) else {
            continue;
        };

        if other.team != team
            || other.unit_type != unit_type
            || !is_upgrade_available(state, other)
            || other.state == UnitOrderState::UnitReady
        {
            continue;
        }

        let Some(complex) = other.complex else {
            continue;
        };

        let index = match complexes.iter().position(|known| *known == complex) {
            Some(index) => index,
            None => {
                // `(*it).GetComplex()->GetCargoInfo(materials, capacity)`: in
                // the port the complex aggregate is already recomputed by `OptimizeComplexes`
                // (`GameState::update_complex_resources`), so we take its
                // `raw` field without re-summing the storages.
                let materials = state
                    .complex(team, complex.0 as u16)
                    .map(|complex| i32::from(complex.raw))
                    .unwrap_or(0);

                complexes.push(complex);
                costs.push(materials);

                complexes.len() - 1
            }
        };

        if costs[index] >= upgrade_cost {
            if let Some(command) = p2_order(
                state,
                id,
                UnitOrder::Upgrade,
                UnitOrderState::ExecutingOrder,
                Some(id),
            ) {
                commands.push(command);
            }

            costs[index] -= upgrade_cost;
            material_cost += upgrade_cost;
            count += 1;
            last = Some(id);
        }
    }

    let version = roman_version(
        state
            .team_units()
            .get(team.0 as usize)
            .map(|units| units.current_unit_values(unit_type as u16).version)
            .unwrap_or(1),
    );
    let name = crate::buildmenu::unit_name(unit_type);

    let message = if count == 0 {
        format!("{upgrade_cost} raw material needed to upgrade.")
    } else if count == 1 {
        format!("{name} upgraded to mark {version} for {material_cost} raw material.")
    } else {
        format!("{count} {name} upgraded to mark {version} for {material_cost} raw material.")
    };

    let _ = last;

    Some(UpgradeAllReport {
        commands,
        count,
        material_cost,
        message,
    })
}

/// Building self-destruct order — `UnitsManager_SetNewOrder(unit,
/// ORDER_EXPLODE, ORDER_STATE_EXPLODE)` (`units_manager.cpp:506-511`).
pub fn explode_order(
    state: StateRef<'_>,
    unit_id: UnitId,
) -> Option<max_core::input::Command> {
    p1_order(state, unit_id, UnitOrder::Explode, UnitOrderState::Explode)
}

/// A log line for an item that does nothing (disabled).
pub fn disabled_line(item: &UnitMenuItem) -> String {
    format!("{}: {}", item.caption, item.note)
}

/// A log line about choosing an item (`App::activate_unit_menu_item`).
pub fn activation_line(item: &UnitMenuItem) -> String {
    format!("{} — {}", item.caption, item.id.name())
}

/// Check "the item exists and is enabled" (QA `assert unit-menu-enabled`).
pub fn item_enabled(menu: &UnitMenu, text: &str) -> Option<bool> {
    menu.index_by_text(text).map(|index| menu.items[index].enabled)
}

#[cfg(test)]
#[path = "unitmenu_tests.rs"]
mod tests;
