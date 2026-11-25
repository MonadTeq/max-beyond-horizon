//! Pre-landing mobilization screen — a full port of `CargoMenu`.
//!
//! # Oracle
//!
//! * `cargomenu.cpp` — `CargoMenu` (`CARGOPIC` background): the "Available" and
//!   "Purchased" lists, cargo loading with a slider, buy/sell, the gold bar,
//!   the `CRGTOG` toggle (buy ↔ upgrade), the filters
//!   `SELGRD`/`SELAIR`/`SELSEA`/`SELBLD`/`SELCBT`, the buttons `CRGBUY`, `CRGDEL`,
//!   `BLDONE`, `BLDCAN`, `BLDHLP`, `BLDDES`, the list arrows;
//! * `abstractupgrademenu.cpp` — the upgrades column (`UpgradeControl`),
//!   `IsUnitFiltered`/`IsBetterUnit`, `DrawUnitStats` (the `VERTGOLD` gold bar),
//!   `VERTGOLD`), `CommitUpgradeChanges`;
//! * `purchaseTypeSelector.cpp` — prices in the type list
//!   (`Cargo_GetRawConsumptionRate(CONSTRCT|TRAINHAL|LANDPLT, 1) * turns`);
//! * `cargoselector.cpp` — "Purchased" rows (`I_*` icon, "Name N (Full)");
//! * `unittypeselector.cpp`/`scrollbar.cpp` — 32 px rows, `max_item_count`,
//!   `LoadVerticalBar` (the `VERTRAW`/`VERTFUEL`/`VERTGOLD` bars);
//! * `reportstats.cpp` — `ReportStats_DrawListItemIcon` (32×32 icon, scale
//!   1/2 for mobile and 1/4 for stationary), `DrawNumber`/`DrawText` (the number
//!   is right-aligned in the column).
//!
//! # Layout (`cargomenu.cpp` coordinates)
//!
//! | Area | Coordinates | Source |
//! |---|---|---|
//! | Portrait | (11,13) 280×240 | `unit_portrait_window` |
//! | Description | (11+16,13+17) 280×230 | `Text_TextBox` over the portrait |
//! | Lot icon / `Purchased` list | (337,45) 118×185, rows 32 px (5) | `wininfo2` |
//! | `Available` list | (482,80) 123×290, rows 32 px (9) | `wininfo1` |
//! | List price column | x 610..625, right edge 625 | `PurchaseTypeSelector` |
//! | Statistics | (11,293) 250×174 | `stats_background` |
//! | Upgrades column | (283,293) 38×174, row 19 px | `button_background` |
//! | Upgrade price column | (321,293) 40×174, number + `I_GOLD` | `cost_background` |
//! | Gold bar | (361,276) 40×140, `VERTGOLD` fill from the bottom | `gold_background` |
//! | Cargo slider | (421,301) 20×115, value 5·N | `LimitedScrollbar` |
//! | Cargo counter | (421,276) 20×10 | `xfer_amount_background` |
//! | Title | (473,7) 158×18 | `_(eb71)` «Purchase Menu» |
//! | `Buy` / `Delete` / `Cancel` / `Done` / `Help` / `Description` / `CRGTOG` | (590,386) 41×22; (412,240) 53×23; (357,452) 62×23; (447,452) 54×23; (420,452) 26×23; (292,264) 17×17; (542,446) 81×32 |
//!
//! # Behavior
//!
//! * `CargoMenu::unit_types1` — 15 purchasable types (the buy list); the upgrade
//!   mode shows the **full** filtered list
//!   (`IsUnitFiltered`: `UPGRADABLE`, built by the team, domain on,
//!   optionally "combat only"), sorted by `IsBetterUnit`.
//! * Buy price — `3 * ATTRIB_TURNS` of the **current** values
//!   (`unitvalues_actual`, after upgrades), the sale refund — the same price
//!   plus `cargo / 5`.
//! * Cargo: 1 credit per 5 units, capacity `ATTRIB_STORAGE / 5` (for `GOLDTRCK`
//!   capacity 0 — like `Select` in C++); the free mission cargo (`SC_L0001`)
//!   cannot be reduced below the starting one (`xfer_take_max`).
//! * Upgrades (`UpgradeControl`): `ATTRIB_ATTACK`, `ROUNDS`, `RANGE`, `AMMO`
//!   (only if there is a base attack), `ARMOR`, `HITS`, `SCAN`, `SPEED`; the price —
//!   [`max_core::research::UpgradeQuote`] (the integer
//!   `UpgradeControl_CalculateCost`, checked against the C++ vectors), the step — 1/2/5/10,
//!   the display limit — `UPGRADECONTROL_UPGRADE_COST_LIMIT` (800). `ATTRIB_TURNS`
//!   is not added by a control in the C++ port (`UPGRADE_CONTROL_9` is not
//!   used) — `CargoMenu::UpdateTeamGold` is dead code there.
//! * `Done` commits the credits/loadout/changed `UnitValues`
//!   (`CommitUpgradeChanges`): they go into `LocalSetup` and are applied to
//!   `TeamUnits.current_values` before landing.
//!
//! # Deliberate differences
//!
//! * The list icons — `I_*` resources (sprite format): body (frame 0) and
//!   a separate turret/gun frame (frame 1) for `TURRET_SPRITE`/
//!   `SPINNING_TURRET`, like `ReportStats_DrawListItemIcon`. The turret shift —
//!   `angle_offsets[2]` from `D_*` (`Unit::GetFrameInfo()`), divided by
//!   `divisor` (1 for mobile, 2 for stationary).
//! * Portrait — big image `P_*` 300×240 1:1, like `WindowManager_LoadBigImage`
//!   (`palette_from_image=false`, `ulx/uly` from the resource = 0): the indices are read
//!   with the `CARGOPIC` window palette. The C++ port writes 300 px into the dialog buffer **without
//!   clipping**, so its 20 right pixels spill onto the background; here the portrait
//!   is clipped by the `unit_portrait_window` window (280×240) — like the original window.
//! * Description — `Unit::GetDescription()`: the `language.json` (en-US) texts by
//!   the hex key from `SC_UNITS`, 93 records in `data/cargo_unit_data.json`
//!   ([`crate::cargo_unit_data`]). Drawn over the portrait
//!   (`Text_TextBox(16,17,280,230, GNW_TEXT_OUTLINE|0xA2)`, font 5, wrapping
//!   `Text_SplitText`). 24 service types (debris, explosions, projectiles,
//!   `HARVSTER`, `WALDO`) have no description in the original, but they are also unreachable in
//!   `CargoMenu` (no `UPGRADABLE`/not buildable).
//! * `UnitStats_DrawStats` — 1:1: the `I_*` icon grid (`I_HRDATK`, `I_SHOTS`,
//!   `I_RANGE`, `I_AMMO`, `I_RAW`/`I_RAWE`/`I_FUEL`/`I_GOLD`/`I_GOLDE`,
//!   `I_POWER`, `I_LIFE`, `I_ARMOR`, `I_HITS`, `I_SCAN`, `I_SPEED`), the pips step
//!   `UnitStats_DrawIcons`, the red row lines, the numbers/labels
//!   `ReportStats_DrawNumber`/`Text_Blit` (x=24/28, clip 49 px).
//! * The `Cost`/`Credit`/`Cargo` numbers under the lists: in the C++ port only the
//!   labels are drawn (the numbers were lost during porting), here the values are output over
//!   the labels (as in the original game).
//! * `Help` (`BLDHLP`) opens the `CARGO_SETUP` window (`HelpMenu_Menu`,
//!   `HELPFRAM`, P8) over `CARGOPIC`; `?` does the same.
//! * Sounds (`KCARG0`/`NDONE0`/…) are not played (the audio layer is separate).
//! * The AI still gets the standard `SC_L0001` loadout
//!   (`RollTeamMissionSupplies` is not ported) — this has not changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use max_assets::image::{BigImage, SimpleImage};
use max_assets::res::ResArchive;
use max_assets::sprite::{decode_sprite, SpriteFrame};
use max_core::access::unit_type_flags;
// Explicit import of the classification extension type (`UnitTypeClassify` — sealed in
// max_core); no glob re-exports (phase 5.2).
use max_core::UnitTypeClassify;
use max_core::deploy::{TeamSupply, DEFAULT_MISSION_LOADOUT};
use max_core::enums::{unit_flags, TeamClan, UnitType, UNIT_END};
use max_core::production::builder_type;
use max_core::research::UpgradeQuote;
use max_core::team::TeamId;
use max_core::unit_values::{
    team_base_values, UnitValues, ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK,
    ATTRIB_HITS, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN, ATTRIB_SPEED, ATTRIB_STORAGE,
    ATTRIB_TURNS,
};
#[cfg(test)]
use max_core::unit_values::base_unit_values;
use max_core::victory::{
    cargo_fuel_consumption_rate, cargo_gold_consumption_rate, cargo_life_consumption_rate,
    cargo_power_consumption_rate, cargo_raw_consumption_rate,
};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFont, GnwFontId};
use max_render::{ColorLut, TeamColors};

use crate::assets::Assets;
use crate::cargo_unit_data;
use crate::menu::{
    blit_simple, draw_big_image, draw_big_image_at, draw_image_button, wrap_gnw_text,
    MenuButtonPair,
};
use crate::panels::UiPalette;
use crate::ui::Key;
use crate::upgrademenu::{is_better_unit, is_buildable, UpgradeFilter};

// ---------------------------------------------------------------------------
// CargoMenu layout (cargomenu.cpp)
// ---------------------------------------------------------------------------

/// Original frame width.
pub const SCREEN_WIDTH: i32 = 640;
/// Original frame height.
pub const SCREEN_HEIGHT: i32 = 480;

/// Portrait (`unit_portrait_window`, 11,13 + 280×240).
pub const PORTRAIT: PixelRect = PixelRect::new(11, 13, 280, 240);
/// "Available" list (`wininfo1` 482,80 — 605,370).
pub const AVAILABLE_AREA: PixelRect = PixelRect::new(482, 80, 123, 290);
/// List row height (`unittypeselector.cpp:64`, `* 32`).
pub const LIST_ROW: i32 = 32;
/// Rows in "Available": `(370 - 80) / 32`.
pub const AVAILABLE_ROWS: usize = 9;
/// Price column of the "Available" list (`PurchaseTypeSelector`, x = 482 + 128, 15 px,
/// the number right-aligned to 625).
pub const PRICE_COLUMN: PixelRect = PixelRect::new(610, 80, 15, 288);
/// "Purchased" list (`wininfo2` 337,45 — 455,230).
pub const PURCHASED_AREA: PixelRect = PixelRect::new(337, 45, 118, 185);
/// Rows in "Purchased": `(230 - 45) / 32`.
pub const PURCHASED_ROWS: usize = 5;
/// "Purchased" title (`Text_TextBox(337,22,118,10)`).
pub const PURCHASED_HEADER: PixelRect = PixelRect::new(337, 22, 118, 10);
/// Separator under "Purchased" (`draw_line(337,34 — 455,34)`).
pub const PURCHASED_LINE_Y: i32 = 34;
/// "Available" title (`Text_TextBox(482,57,143,10)`).
pub const AVAILABLE_HEADER: PixelRect = PixelRect::new(482, 57, 143, 10);
/// Separator under "Available" (`draw_line(482,69 — 625,69)`).
pub const AVAILABLE_LINE_Y: i32 = 69;
/// "Cost" title above the price column (`Text_TextBox(625 - w, 57, w, 10)`).
pub const COST_HEADER: PixelRect = PixelRect::new(610, 57, 15, 10);
/// Window title (`Text_TextBox(473,7,158,18, COLOR_GREEN)`).
pub const TITLE: PixelRect = PixelRect::new(473, 7, 158, 18);

/// Statistics block (`stats_background` 11,293,250,174).
pub const STATS: PixelRect = PixelRect::new(11, 293, 250, 174);
/// Statistics row: `UnitStats_DrawText` — 19 px, the number/label centered.
pub const STAT_ROW: i32 = 19;
/// Where the pips start (`UnitStats_DrawStats`: `&buffer[76]`).
pub const STAT_ICON_X: i32 = 76;
/// How many pixels are allotted to the label (`Text_Blit(..., 49, ...)`).
pub const STAT_LABEL_WIDTH: i32 = 49;
/// Description area: `Text_TextBox(unit_portrait_window.buffer, ..., 16, 17,
/// 280, 230, GNW_TEXT_OUTLINE | 0xA2, false, false)` — from (11,13).
pub const DESCRIPTION: PixelRect = PixelRect::new(PORTRAIT.ulx + 16, PORTRAIT.uly + 17, 280, 230);
/// `ReportStats_DrawNumber`/`Text_Blit` in a statistics row: `COLOR_YELLOW`.
pub const STAT_COLOR_NUMBER: u8 = 0x04;
/// `UnitStats_DrawText(..., 0xA2)` — the label and description number color.
pub const STAT_COLOR_LABEL: u8 = 0xA2;
/// `COLOR_RED` — row separator lines and the upgrade delta.
pub const STAT_COLOR_LINE: u8 = 0x01;
/// `COLOR_BLACK` — description outline (`GNW_TEXT_OUTLINE` → `Color_RGB2Color(0)`).
pub const STAT_COLOR_OUTLINE: u8 = 0x00;

/// Statistics row labels — `language.json` en-US, like `_(...)` in
/// `unitstats.cpp` (`Attack` `_(98a8)`, `Shots` `_(5e40)`, `Range` `_(2540)`,
/// `Ammo` `_(d510)`, `Cargo` `_(054b)`, `Prod.` `_(527f)`/`_(3641)`,
/// `Uses` `_(9e61)`, `Armor` `_(49d1)`, `Hits` `_(6e4e)`, `Scan` `_(d10e)`,
/// `Speed` `_(6136)`, `Cost` `_(7dfb)`).
pub const STAT_ATTACK: &str = "Attack";
/// `_(5e40)`.
pub const STAT_SHOTS: &str = "Shots";
/// `_(2540)`.
pub const STAT_RANGE: &str = "Range";
/// `_(d510)`.
pub const STAT_AMMO: &str = "Ammo";
/// `_(054b)`.
pub const STAT_CARGO: &str = "Cargo";
/// `_(527f)` (production) and `_(3641)` (life support).
pub const STAT_PROD: &str = "Prod.";
/// `_(9e61)` — resource consumption.
pub const STAT_USES: &str = "Uses";
/// `_(49d1)`.
pub const STAT_ARMOR: &str = "Armor";
/// `_(6e4e)`.
pub const STAT_HITS: &str = "Hits";
/// `_(d10e)`.
pub const STAT_SCAN: &str = "Scan";
/// `_(6136)`.
pub const STAT_SPEED: &str = "Speed";
/// `_(7dfb)`.
pub const STAT_COST: &str = "Cost";
/// Upgrade price column (`cost_background` 321,293,40,174).
pub const COST: PixelRect = PixelRect::new(321, 293, 40, 174);
/// Upgrade arrows column (`button_background` 283,293,38,174).
pub const CONTROL_BUTTONS: PixelRect = PixelRect::new(283, 293, 38, 174);
/// Upgrade row step (`upgrade_control_next_uly += 19`).
pub const CONTROL_ROW: i32 = 19;
/// Gold bar (`gold_background` 361,276,40,140).
pub const GOLD_BAR: PixelRect = PixelRect::new(361, 276, 40, 140);
/// Cargo slider (`Scrollbar` 421,301 — 441,416).
pub const CARGO_BAR: PixelRect = PixelRect::new(421, 301, 20, 115);
/// Cargo counter above the slider (`xfer_amount_bounds` 421,276 — 441,286).
pub const CARGO_AMOUNT: PixelRect = PixelRect::new(421, 276, 20, 10);

/// "Description" label (`Text_TextBox(209,264,80,17)`).
pub const DESCRIPTION_LABEL: PixelRect = PixelRect::new(209, 264, 80, 17);
/// "Cost" label (`Text_TextBox(320,283,48,16)`).
pub const COST_LABEL: PixelRect = PixelRect::new(320, 283, 48, 16);
/// "Credit" label (`Text_TextBox(358,283,48,16)`).
pub const CREDIT_LABEL: PixelRect = PixelRect::new(358, 283, 48, 16);
/// "Cargo" label (`Text_TextBox(409,283,48,16)`).
pub const CARGO_LABEL: PixelRect = PixelRect::new(409, 283, 48, 16);

/// `Buy` button (`CRGBUY_U`, 41×22).
pub const BUY_BUTTON: PixelRect = PixelRect::new(590, 386, 41, 22);
/// `Delete` button (`CRGDEL_U`, 53×23).
pub const DELETE_BUTTON: PixelRect = PixelRect::new(412, 240, 53, 23);
/// `Cancel` button (`BLDCAN_U`, 62×23).
pub const CANCEL_BUTTON: PixelRect = PixelRect::new(357, 452, 62, 23);
/// `Done` button (`BLDONE_U`, 54×23).
pub const DONE_BUTTON: PixelRect = PixelRect::new(447, 452, 54, 23);
/// `Help` button (`BLDHLP_U`, 26×23).
pub const HELP_BUTTON: PixelRect = PixelRect::new(420, 452, 26, 23);
/// Description button (`BLDDES_U`, 17×17).
pub const DESCRIPTION_BUTTON: PixelRect = PixelRect::new(292, 264, 17, 17);
/// "Buy Units"/"Upgrades" toggle (`CRGTOG_U`, 81×32).
pub const TOGGLE_BUTTON: PixelRect = PixelRect::new(542, 446, 81, 32);
/// "Available" list scroll up (`BLDUP__U`, 18×17).
pub const LIST_UP: PixelRect = PixelRect::new(471, 387, 18, 17);
/// "Available" list scroll down (`BLDDWN_U`).
pub const LIST_DOWN: PixelRect = PixelRect::new(491, 387, 18, 17);
/// "Purchased" list scroll up (`BLDUP__U`).
pub const PURCHASE_UP: PixelRect = PixelRect::new(327, 240, 18, 17);
/// "Purchased" list scroll down (`BLDDWN_U`).
pub const PURCHASE_DOWN: PixelRect = PixelRect::new(348, 240, 18, 17);
/// Row icon size (`ReportStats_DrawListItemIcon`, 32×32).
pub const ICON_BOX: i32 = 32;

/// `CargoMenu::unit_types1` (`cargomenu.cpp:176`) — the buy list, the order of
/// the list rows.
pub const PURCHASE_UNITS: [UnitType; 15] = [
    UnitType::Constructor,
    UnitType::Engineer,
    UnitType::Surveyor,
    UnitType::Scout,
    UnitType::Scanner,
    UnitType::Tank,
    UnitType::AssaultGun,
    UnitType::RocketLauncher,
    UnitType::MissileCrawler,
    UnitType::MineLayer,
    UnitType::RepairUnit,
    UnitType::SupplyTruck,
    UnitType::FuelTruck,
    UnitType::MobileAntiAir,
    UnitType::GoldTruck,
];

/// `Cargo_GetRawConsumptionRate(LANDPLT, 1)` (`cargo.cpp:168`).
pub const LANDPLT_RAW_CONSUMPTION: u32 = 3;
/// Upper bound of the displayed cost (`UPGRADECONTROL_UPGRADE_COST_LIMIT`).
pub const UPGRADE_COST_LIMIT: i32 = 800;

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

/// Type list mode (the `CRGTOG` toggle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoMode {
    /// Buy list — `CargoMenu::unit_types1`.
    Buy,
    /// Full upgrade list (`AbstractUpgradeMenu::PopulateTeamUnitsList`).
    Upgrade,
}

/// Which list receives the arrows/PageUp/PageDown (`active_selector`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveList {
    /// "Available" (types).
    Types,
    /// "Purchased" (bought plus the free loadout).
    Loadout,
}

/// Screen action after a click/key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoAction {
    /// The screen stays open.
    None,
    /// `Cancel`/`Esc` — cancel the start.
    Cancel,
    /// `Done`/`Enter` — commit and continue.
    Done,
    /// `Help`/`?` — open the `CARGO_SETUP` window (`HelpMenu_Menu`).
    Help,
}

/// Icon (pips) group of a statistics row — a port of `UnitStats_DrawIcons`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatIcon {
    /// `I_*` resource (simple image) used to draw the "filled" pips.
    pub resource: &'static str,
    /// "Empty" pips resource (`I_*E`); for all rows except `Cost` — the same.
    pub empty_resource: &'static str,
    /// `current` in `UnitStats_DrawIcons` — how many pips are drawn with `resource`.
    pub current: i32,
    /// `original` — the total; `original - current` is drawn with
    /// `empty_resource` after the red line (the upgrade delta).
    pub original: i32,
}

impl StatIcon {
    /// A pair of identical icons (`UnitStats_DrawIcons(id, id, ...)`).
    pub const fn same(resource: &'static str, current: i32, original: i32) -> Self {
        Self {
            resource,
            empty_resource: resource,
            current,
            original,
        }
    }

    /// How many pips the group will draw (`max(0, current) + max(0, delta)`).
    pub fn count(&self) -> usize {
        self.current.max(0) as usize + (self.original - self.current).max(0) as usize
    }
}

/// A statistics table row — a port of `UnitStats_DrawText` + `UnitStats_DrawIcons`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatRow {
    /// Row `y` (the same 19 px step as the upgrades column).
    pub y: i32,
    /// Label from `language.json` (`_(98a8)` → `Attack`, …).
    pub label: &'static str,
    /// The number (`ReportStats_DrawNumber`); `None` — the row needs no number
    /// (`_(9e61)`: `resource_count == 0`).
    pub value: Option<i32>,
    /// Icon groups after the number; shifted by `STAT_ICON_X` and further.
    pub icons: Vec<StatIcon>,
    /// Red line under the row (`draw_line(2,18 — 248,18)`); not for `Cost`.
    pub separator: bool,
}

impl StatRow {
    /// How many pips are in the row (for QA/tests).
    pub fn icon_count(&self) -> usize {
        self.icons.iter().map(StatIcon::count).sum()
    }
}

/// Mobilization screen state of one team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mobilization {
    /// Whose mobilization.
    pub team: TeamId,
    /// Team clan (values and prices).
    pub clan: TeamClan,
    /// Credits at entry (`CargoMenu::start_gold`).
    pub credits_start: u32,
    /// Remaining credits (`CargoMenu::team_gold`).
    pub credits: u32,
    /// Free mission loadout (`SC_L0001`), cannot be deleted/reduced.
    pub free: Vec<TeamSupply>,
    /// Purchased units (in purchase order).
    pub bought: Vec<TeamSupply>,
    /// Type list mode.
    pub mode: CargoMode,
    /// Types in the "Available" list (`Buy` mode — 15).
    pub types: Vec<UnitType>,
    /// Selected row of the type list (`page_max_index`).
    pub type_selected: usize,
    /// First visible row of the type list (`page_min_index`).
    pub type_page: usize,
    /// Selected "Purchased" row (0..free+bought).
    pub loadout_selected: usize,
    /// First visible "Purchased" row.
    pub loadout_page: usize,
    /// Which list is active for the arrows.
    pub active: ActiveList,
    /// The type whose portrait/statistics/upgrades are shown (`unit_type`).
    pub info: Option<UnitType>,
    /// Base values at the moment of opening (`unitvalues_base`).
    pub base: [UnitValues; UNIT_END as usize],
    /// Editable values (`unitvalues_actual`).
    pub actual: [UnitValues; UNIT_END as usize],
    /// Upgrade rows of the selected type: `(ATTRIB_*, uly)`.
    pub controls: Vec<(u8, i32)>,
    /// Domain filters (`button_*_rest_state`): ground, air, sea, building, combat.
    pub filters: [bool; 5],
    /// Show the description (`button_description_rest_state`); there is no text in the data.
    pub description: bool,
    /// Status line.
    pub status: String,
}

impl Mobilization {
    /// `CargoMenu::CargoMenu(team)`: `start_gold + Clans::GetCredits(clan)`
    /// (all `SC_CLANS` clans have 0 credits), the base team values and
    /// the free mission loadout.
    pub fn new(team: TeamId, clan: TeamClan, start_gold: u32) -> Self {
        let base = team_base_values(clan);
        let free = DEFAULT_MISSION_LOADOUT.to_vec();
        let info = free.first().map(|supply| supply.unit_type);

        let mut mobilization = Self {
            team,
            clan,
            credits_start: start_gold,
            credits: start_gold,
            free,
            bought: Vec::new(),
            mode: CargoMode::Buy,
            types: PURCHASE_UNITS.to_vec(),
            type_selected: 0,
            type_page: 0,
            loadout_selected: 0,
            loadout_page: 0,
            active: ActiveList::Types,
            info,
            base,
            actual: base,
            controls: Vec::new(),
            filters: [true, true, true, true, false],
            description: true,
            status: String::new(),
        };

        mobilization.rebuild_controls();
        mobilization
    }

    /// Whether the phase is needed: in C++ `CargoMenu` does not open at a zero budget
    /// (`if (team_gold)` in `GameManager_PlayerMissionSetup`).
    pub fn needed(start_gold: u32) -> bool {
        start_gold > 0
    }

    /// Final loadout `TeamMissionSupplies::units` (free + purchased).
    pub fn loadout(&self) -> Vec<TeamSupply> {
        let mut loadout = self.free.clone();
        loadout.extend(self.bought.iter().copied());
        loadout
    }

    /// All "Purchased" list rows (`unit_types2`).
    pub fn loadout_rows(&self) -> Vec<TeamSupply> {
        self.loadout()
    }

    /// `CargoMenu::DrawUnitInfo`: the price of the current values, `3 * turns`.
    pub fn price(&self, unit: UnitType) -> u32 {
        unit_price(&self.actual, unit)
    }

    /// Type list row price (`PurchaseTypeSelector::Draw`):
    /// `STATIONARY` → `CONSTRCT` rate (2), `COMMANDO`/`INFANTRY` →
    /// `TRAINHAL` rate (1), otherwise `LANDPLT` (3); multiply by `turns`.
    pub fn list_price(&self, unit: UnitType) -> u32 {
        list_price(&self.actual, unit)
    }

    /// Price of the selected type list row (for the `Cost` label).
    pub fn selected_cost(&self) -> u32 {
        self.types
            .get(self.type_selected)
            .map(|unit| self.price(*unit))
            .unwrap_or(0)
    }

    /// Selected type of the "Available" list.
    pub fn selected_type(&self) -> Option<UnitType> {
        self.types.get(self.type_selected).copied()
    }

    /// "Purchased" row by index.
    pub fn loadout_row(&self, index: usize) -> Option<TeamSupply> {
        if index < self.free.len() {
            self.free.get(index).copied()
        } else {
            self.bought.get(index - self.free.len()).copied()
        }
    }

    /// How many cargo units are in row `index`.
    pub fn cargo(&self, index: usize) -> u16 {
        self.loadout_row(index).map_or(0, |supply| supply.cargo)
    }

    /// The row the slider can control (selected in "Purchased").
    fn cargo_row(&self) -> usize {
        self.loadout_selected
    }

    /// Cargo capacity in blocks of 5 (`Select`: `ATTRIB_STORAGE / 5`, for
    /// `GOLDTRCK` — 0).
    pub fn cargo_capacity(&self) -> i32 {
        let Some(supply) = self.loadout_row(self.cargo_row()) else {
            return 0;
        };
        cargo_capacity(&self.actual, supply.unit_type)
    }

    /// Slider value (`cargos[index] / 5`).
    pub fn cargo_value(&self) -> i32 {
        i32::from(self.cargo(self.cargo_row())) / 5
    }

    /// `xfer_take_max`: the free mission cargo cannot be reduced, the purchased ones —
    /// can be reduced to zero.
    fn cargo_take_max(&self) -> i32 {
        let index = self.cargo_row();
        if index < self.free.len() {
            i32::from(self.cargo(index)) / 5
        } else {
            0
        }
    }

    /// `xfer_give_max`: `cargos[page] / 5 + team_gold` (`DrawUnitStats`).
    fn cargo_give_max(&self) -> i32 {
        self.cargo_value() + self.credits as i32
    }

    /// Cargo bar material (`VERTFUEL`/`VERTGOLD`/`VERTRAW`).
    pub fn cargo_bar_kind(&self) -> CargoBarKind {
        match self
            .loadout_row(self.cargo_row())
            .map(|supply| supply.unit_type)
        {
            Some(UnitType::FuelTruck) => CargoBarKind::Fuel,
            Some(UnitType::GoldTruck) => CargoBarKind::Gold,
            _ => CargoBarKind::Raw,
        }
    }

    /// Whether `Delete` can sell the selected row (`index >= unit_count`).
    pub fn can_delete(&self) -> bool {
        self.loadout_selected >= self.free.len()
            && self.loadout_selected < self.loadout_rows().len()
    }

    /// Whether `Buy` can buy the selected type (`DrawUnitInfo`).
    pub fn can_buy(&self) -> bool {
        self.selected_type()
            .is_some_and(|unit| PURCHASE_UNITS.contains(&unit) && self.price(unit) <= self.credits)
    }

    // -----------------------------------------------------------------------
    // Lists
    // -----------------------------------------------------------------------

    /// `CargoMenu::PopulateTeamUnitsList`: `Buy` mode — `unit_types1`, `Upgrade`
    /// mode — the filtered full list in `IsBetterUnit` order.
    pub fn rebuild_types(&mut self) {
        let previous = self.selected_type();
        self.types = match self.mode {
            CargoMode::Buy => PURCHASE_UNITS.to_vec(),
            CargoMode::Upgrade => upgrade_types(&self.filters),
        };

        self.type_selected = previous
            .and_then(|unit| self.types.iter().position(|candidate| *candidate == unit))
            .unwrap_or(0);
        self.ensure_type_visible();

        if previous.is_some() && self.selected_type() != previous {
            // C++ `AddItems`: `if (GetLast() != unit_type) Select(0)` →
            // `DrawUnitInfo(unit_type)`.
            self.info = self.selected_type();
            self.rebuild_controls();
        }
    }

    /// `UnitTypeSelector::ScrollTo` for the type list.
    fn ensure_type_visible(&mut self) {
        let count = self.types.len() as i64;
        let visible = AVAILABLE_ROWS as i64;
        let mut page = self.type_page as i64;
        let selected = self.type_selected as i64;

        if selected < page {
            page = selected;
        }
        if page + visible <= selected {
            page = selected - visible + 1;
        }
        if page + visible > count {
            page = count - visible;
        }
        if page < 0 {
            page = 0;
        }

        self.type_page = page.max(0) as usize;
    }

    /// The same for the "Purchased" list.
    fn ensure_loadout_visible(&mut self) {
        let count = self.loadout_rows().len() as i64;
        let visible = PURCHASED_ROWS as i64;
        let mut page = self.loadout_page as i64;
        let selected = self.loadout_selected as i64;

        if selected < page {
            page = selected;
        }
        if page + visible <= selected {
            page = selected - visible + 1;
        }
        if page + visible > count {
            page = count - visible;
        }
        if page < 0 {
            page = 0;
        }

        self.loadout_page = page.max(0) as usize;
    }

    /// `PopulateTeamUnitsList` by the toggle (`1100`/`1101`).
    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            CargoMode::Buy => CargoMode::Upgrade,
            CargoMode::Upgrade => CargoMode::Buy,
        };
        self.rebuild_types();
        self.status = match self.mode {
            CargoMode::Buy => "Buy units".to_string(),
            CargoMode::Upgrade => "Upgrade units".to_string(),
        };
    }

    /// Toggles the domain filter (`1003`…`1012`); in buy mode the buttons
    /// are off (C++ `Disable`).
    pub fn toggle_filter(&mut self, index: usize) {
        if self.mode != CargoMode::Upgrade || index >= self.filters.len() {
            return;
        }
        self.filters[index] = !self.filters[index];
        let previous = self.selected_type();
        self.types = upgrade_types(&self.filters);
        self.type_selected = previous
            .and_then(|unit| self.types.iter().position(|candidate| *candidate == unit))
            .unwrap_or(0);
        self.ensure_type_visible();
        if self.selected_type() != previous {
            self.info = self.selected_type();
            self.rebuild_controls();
        }
    }

    // -----------------------------------------------------------------------
    // Selection and loading
    // -----------------------------------------------------------------------

    /// `CargoMenu::Select(index)` for the "Purchased" list.
    pub fn select_loadout(&mut self, index: usize) {
        let count = self.loadout_rows().len();
        if index >= count {
            return;
        }

        self.loadout_selected = index;
        self.ensure_loadout_visible();

        if let Some(supply) = self.loadout_row(index) {
            self.info = Some(supply.unit_type);
            self.rebuild_controls();
        }
    }

    /// Selecting an "Available" list row (a click on it).
    pub fn select_type(&mut self, index: usize) {
        if index >= self.types.len() {
            return;
        }

        self.type_selected = index;
        self.active = ActiveList::Types;
        self.ensure_type_visible();
        self.info = self.selected_type();
        self.rebuild_controls();
    }

    /// Arrow/PageUp/PageDown over the active list.
    pub fn step(&mut self, delta: i32) {
        match self.active {
            ActiveList::Types => {
                if self.types.is_empty() {
                    return;
                }
                let last = self.types.len() as i32 - 1;
                self.type_selected = (self.type_selected as i32 + delta).clamp(0, last) as usize;
                self.ensure_type_visible();
                self.info = self.selected_type();
                self.rebuild_controls();
            }
            ActiveList::Loadout => {
                let count = self.loadout_rows().len();
                if count == 0 {
                    return;
                }
                let last = count as i32 - 1;
                self.loadout_selected =
                    (self.loadout_selected as i32 + delta).clamp(0, last) as usize;
                self.ensure_loadout_visible();
                if let Some(supply) = self.loadout_row(self.loadout_selected) {
                    self.info = Some(supply.unit_type);
                    self.rebuild_controls();
                }
            }
        }
    }

    /// List page (by `max_item_count` rows).
    pub fn page(&mut self, delta: i32) {
        let rows = match self.active {
            ActiveList::Types => AVAILABLE_ROWS as i32,
            ActiveList::Loadout => PURCHASED_ROWS as i32,
        };
        self.step(delta * rows);
    }

    /// List scrolling without changing the selection (the `BLDUP__`/`BLDDWN` arrows).
    pub fn scroll(&mut self, list: ActiveList, delta: i32) {
        match list {
            ActiveList::Types => {
                let rows = self.types.len().saturating_sub(AVAILABLE_ROWS);
                self.type_page = (self.type_page as i32 + delta).clamp(0, rows as i32) as usize;
            }
            ActiveList::Loadout => {
                let rows = self.loadout_rows().len().saturating_sub(PURCHASED_ROWS);
                self.loadout_page =
                    (self.loadout_page as i32 + delta).clamp(0, rows as i32) as usize;
            }
        }
    }

    /// `CargoMenu::BuyUnit`: buy the selected type.
    pub fn buy_selected(&mut self) -> bool {
        self.buy(self.type_selected)
    }

    /// Buying by the "Available" list index.
    pub fn buy(&mut self, index: usize) -> bool {
        let Some(&unit) = self.types.get(index) else {
            return false;
        };

        if !PURCHASE_UNITS.contains(&unit) {
            self.status = format!("{} is not for sale", unit_label(unit));
            return false;
        }

        let cost = self.price(unit);
        if cost > self.credits {
            self.status = format!("Not enough credits: {} costs {cost}", unit_label(unit));
            return false;
        }

        self.credits -= cost;
        self.bought.push(TeamSupply {
            unit_type: unit,
            cargo: 0,
        });
        self.loadout_selected = self.loadout_rows().len() - 1;
        self.ensure_loadout_visible();
        self.info = Some(unit);
        self.rebuild_controls();
        self.status = format!("Bought {} for {cost} credits", unit_label(unit));
        true
    }

    /// `CargoMenu::DeleteUnit`: sell the selected "Purchased" row (except
    /// the free mission loadout).
    pub fn delete_selected(&mut self) -> bool {
        if !self.can_delete() {
            self.status = if self.loadout_selected < self.free.len() {
                "The free mission loadout cannot be sold".to_string()
            } else {
                "Select a purchased unit to delete".to_string()
            };
            return false;
        }

        let index = self.loadout_selected - self.free.len();
        let supply = self.bought.remove(index);
        let refund = self.price(supply.unit_type) + u32::from(supply.cargo) / 5;

        self.credits = self.credits.saturating_add(refund);
        self.loadout_selected = self.loadout_selected.min(self.loadout_rows().len() - 1);
        self.ensure_loadout_visible();
        if let Some(supply) = self.loadout_row(self.loadout_selected) {
            self.info = Some(supply.unit_type);
        }
        self.rebuild_controls();
        self.status = format!("Sold {} for {refund} credits", unit_label(supply.unit_type));
        true
    }

    /// Click on the cargo slider (`Scrollbar::ProcessKey`, `pcode 1106`).
    pub fn click_cargo(&mut self, x: i32, y: i32) -> bool {
        if !CARGO_BAR.contains(x, y) {
            return false;
        }

        let capacity = self.cargo_capacity();
        if capacity <= 0 {
            return false;
        }

        let from_bottom = CARGO_BAR.lry() - y;
        let value = (CARGO_BAR.height / 2 + capacity * from_bottom) / CARGO_BAR.height;
        let value = value
            .max(self.cargo_take_max())
            .min(capacity)
            .min(self.cargo_give_max());
        self.set_cargo_value(value);
        true
    }

    /// Sets the slider value (`cargos[index] = value * 5`), debiting
    /// `new - old` credits (1 credit per 5 units).
    pub fn set_cargo_value(&mut self, value: i32) {
        let capacity = self.cargo_capacity();
        if capacity < 0 {
            return;
        }
        let value = value
            .max(self.cargo_take_max())
            .min(capacity)
            .min(self.cargo_give_max())
            .max(0);
        let old = self.cargo_value();
        let delta = value - old;

        if delta > 0 {
            self.credits = self.credits.saturating_sub(delta as u32);
        } else {
            self.credits = self.credits.saturating_add((-delta) as u32);
        }

        let cargo = (value * 5) as u16;
        let index = self.cargo_row();
        if index < self.free.len() {
            self.free[index].cargo = cargo;
        } else if let Some(supply) = self.bought.get_mut(index - self.free.len()) {
            supply.cargo = cargo;
        }
    }

    // -----------------------------------------------------------------------
    // Upgrades
    // -----------------------------------------------------------------------

    /// `AbstractUpgradeMenu::DrawUnitInfo` → `AddUpgradeMilitary`,
    /// `AdjustRowStorage`, `AdjustRowConsumptions`, `AddUpgradeGeneric`,
    /// `AddUpgradeMobile`. `ATTRIB_TURNS` is not added by a control (in the C++ port
    /// `UPGRADE_CONTROL_9` is not used anywhere).
    pub fn rebuild_controls(&mut self) {
        self.controls.clear();
        let Some(unit) = self.info else {
            return;
        };
        let true_base = self.base[unit as usize];
        let actual = self.actual[unit as usize];
        let mut y = COST.uly;
        let mut rows: Vec<(u8, i32)> = Vec::new();

        let push = |rows: &mut Vec<(u8, i32)>, y: &mut i32, attribute: u8| {
            rows.push((attribute, *y));
            *y += CONTROL_ROW;
        };

        if true_base.get_attribute(ATTRIB_ATTACK) > 0 {
            push(&mut rows, &mut y, ATTRIB_ATTACK);
            if true_base.get_attribute(ATTRIB_ROUNDS) > 0 {
                push(&mut rows, &mut y, ATTRIB_ROUNDS);
            }
            if true_base.get_attribute(ATTRIB_RANGE) > 0 {
                push(&mut rows, &mut y, ATTRIB_RANGE);
            }
            if true_base.get_attribute(ATTRIB_AMMO) > 0 {
                push(&mut rows, &mut y, ATTRIB_AMMO);
            }
        }

        if actual.get_attribute(ATTRIB_STORAGE) != 0
            && matches!(
                unit,
                UnitType::MaterialStorage
                    | UnitType::FuelStorage
                    | UnitType::GoldVault
                    | UnitType::Engineer
                    | UnitType::Constructor
                    | UnitType::CargoShip
                    | UnitType::SupplyTruck
                    | UnitType::FuelTruck
                    | UnitType::GoldTruck
            )
        {
            y += CONTROL_ROW;
        }

        let raw = cargo_raw_consumption_rate(unit, 1);
        let fuel = cargo_fuel_consumption_rate(unit);
        let power = cargo_power_consumption_rate(unit);
        let life = cargo_life_consumption_rate(unit);
        if power < 0 {
            y += CONTROL_ROW;
        }
        if life < 0 {
            y += CONTROL_ROW;
        }
        if raw > 0 || fuel > 0 || power > 0 || life > 0 {
            y += CONTROL_ROW;
        }

        if true_base.get_attribute(ATTRIB_ARMOR) > 0 {
            push(&mut rows, &mut y, ATTRIB_ARMOR);
        }
        if true_base.get_attribute(ATTRIB_HITS) > 0 {
            push(&mut rows, &mut y, ATTRIB_HITS);
        }
        if true_base.get_attribute(ATTRIB_SCAN) > 0 {
            push(&mut rows, &mut y, ATTRIB_SCAN);
        }
        if true_base.get_attribute(ATTRIB_SPEED) > 0 {
            push(&mut rows, &mut y, ATTRIB_SPEED);
        }

        self.controls = rows;
    }

    /// Row control (a port of `UpgradeControl::Init`): `team_base_value` —
    /// the type base value, `control_base_value` — the value at menu entry,
    /// the research contribution before the match start is zero
    /// (`ResearchMenu_CalculateFactor` at level 0).
    pub fn quote(&self, row: usize) -> Option<UpgradeQuote> {
        let (attribute, _) = *self.controls.get(row)?;
        let unit = self.info?;
        let base_value = self.base[unit as usize].get_attribute(attribute) as u16;

        let mut quote = UpgradeQuote::new(attribute, base_value, base_value, 0);
        quote.actual = self.actual[unit as usize].get_attribute(attribute) as u16;
        Some(quote)
    }

    /// `UpgradeControl::Increase`/`Decrease`; `true` if the value changed.
    pub fn adjust_upgrade(&mut self, row: usize, increase: bool) -> bool {
        let Some(mut quote) = self.quote(row) else {
            return false;
        };
        let Some(unit) = self.info else {
            return false;
        };

        let changed = if increase {
            quote.increase(&mut self.credits)
        } else {
            quote.decrease(&mut self.credits)
        };

        if changed {
            self.actual[unit as usize].set_attribute(quote.attribute, i32::from(quote.actual));
            self.status = format!(
                "{} {} = {}",
                unit_label(unit),
                attribute_name(quote.attribute),
                quote.actual
            );
        } else {
            self.status = if increase {
                "Cannot upgrade: credits, base or limit".to_string()
            } else {
                "Already at the base value".to_string()
            };
        }

        changed
    }

    /// Arrow rectangles of row `row` (left/right).
    pub fn control_arrows(&self, row: usize) -> Option<(PixelRect, PixelRect)> {
        let (_, y) = *self.controls.get(row)?;
        Some((
            PixelRect::new(CONTROL_BUTTONS.ulx, y, 18, 17),
            PixelRect::new(CONTROL_BUTTONS.ulx + 18, y, 18, 17),
        ))
    }

    /// `Cargo` row icon resource — the `UnitStats_DrawCargo` switch.
    ///
    /// For other types C++ does not draw the row at all (`default: return
    /// buffer`), even if `ATTRIB_STORAGE` > 0 (barracks, dock, repair, …).
    fn storage_icon(unit: UnitType) -> Option<&'static str> {
        match unit {
            UnitType::MaterialStorage
            | UnitType::CargoShip
            | UnitType::SupplyTruck
            | UnitType::Engineer
            | UnitType::Constructor => Some("I_RAW"),
            UnitType::FuelTruck | UnitType::FuelStorage => Some("I_FUEL"),
            UnitType::GoldVault | UnitType::GoldTruck => Some("I_GOLD"),
            _ => None,
        }
    }

    /// `Cargo_GetRawConsumptionRate(Builder_GetBuilderType(team, unit), 1)` —
    /// rate of the `Cost` row (`UnitStats_DrawCost`). For mines and non-buildings
    /// `Builder_GetBuilderType` = `INVALID_ID`, rate 0 (like the C++ default).
    pub fn build_rate(&self, unit: UnitType) -> i32 {
        build_rate_for(unit)
    }

    /// `Unit::GetDescription()` of the selected type (`""` — no description).
    pub fn description(&self) -> &'static str {
        self.info.map_or("", cargo_unit_data::unit_description)
    }

    /// Whether the selected type has a portrait `P_*` (`Unit::GetPortrait()`).
    pub fn has_portrait(&self) -> bool {
        self.info.and_then(cargo_unit_data::unit_portrait).is_some()
    }

    /// How many pips icons are in the statistics table (for QA checks).
    pub fn stats_icon_count(&self) -> usize {
        self.stats_rows().iter().map(StatRow::icon_count).sum()
    }

    /// Statistics table rows — a port of the `UnitStats_DrawStats` blocks.
    ///
    /// The order is exactly as in C++: military (`Attack`/`Shots`/`Range`/`Ammo`),
    /// cargo (`Cargo`), consumption (`Prod.`/`Uses`), common (`Armor`/`Hits`/`Scan`),
    /// `Speed`, `Cost`. The display conditions — by the **base** values
    /// (`unit_values2 = team_units->GetBaseUnitValues`), the numbers — by the current ones.
    pub fn stats_rows(&self) -> Vec<StatRow> {
        let Some(unit) = self.info else {
            return Vec::new();
        };

        // `CargoMenu` overrides `interface_icon_full/empty` with `I_GOLD`/`I_GOLDE`
        // (`cargomenu.cpp:45-46`): the purchase `Cost` row is paid in credits.
        Self::stats_rows_for(&self.base, &self.actual, unit, "I_GOLD", "I_GOLDE")
    }

    /// [`Self::stats_rows`] by explicit value sets — needed by `BuildMenu`:
    /// C++ `AbstractBuildMenu::Draw` calls `UnitStats_DrawStats` with
    /// `team_units->GetCurrentUnitValues(team, unit_type)` and the same
    /// `base_values` (clan modifiers and upgrades are already inside).
    ///
    /// `cost_icon`/`cost_empty_icon` are the `UnitStats_DrawStats(..., icon_full,
    /// icon_empty)` pair for the `Cost` row: `CargoMenu` passes `I_GOLD`/`I_GOLDE`,
    /// while `AbstractBuildMenu::Draw` (`buildmenu.cpp:536`) and `UnitStats_Menu`
    /// (`unitstats.cpp:516`) pass `I_RAW`/`I_RAWE` — the production cost is paid in
    /// raw materials. The number itself is always `UnitStats_DrawCost`
    /// (`Cargo_GetRawConsumptionRate(builder, 1) * ATTRIB_TURNS`), independent of
    /// the icon pair.
    pub fn stats_rows_for(
        base_values: &[UnitValues],
        current_values: &[UnitValues],
        unit: UnitType,
        cost_icon: &'static str,
        cost_empty_icon: &'static str,
    ) -> Vec<StatRow> {
        let base = base_values[unit as usize];
        let actual = current_values[unit as usize];
        let mut rows = Vec::new();
        let mut y = STATS.uly;

        #[allow(clippy::too_many_arguments)]
        let push = |rows: &mut Vec<StatRow>,
                    y: &mut i32,
                    label: &'static str,
                    value: Option<i32>,
                    icons: Vec<StatIcon>,
                    separator: bool| {
            rows.push(StatRow {
                y: *y,
                label,
                value,
                icons,
                separator,
            });
            *y += STAT_ROW;
        };

        let base_attack = base.get_attribute(ATTRIB_ATTACK);
        let actual_attack = actual.get_attribute(ATTRIB_ATTACK);

        if base_attack > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_ATTACK,
                Some(actual_attack),
                vec![StatIcon::same("I_HRDATK", base_attack, actual_attack)],
                true,
            );

            if base.get_attribute(ATTRIB_ROUNDS) > 0 {
                // The number of shots is limited by ammo (`std::min(ROUNDS, AMMO)`).
                let shots = actual
                    .get_attribute(ATTRIB_ROUNDS)
                    .min(actual.get_attribute(ATTRIB_AMMO));

                push(
                    &mut rows,
                    &mut y,
                    STAT_SHOTS,
                    Some(shots),
                    vec![StatIcon::same(
                        "I_SHOTS",
                        base.get_attribute(ATTRIB_ROUNDS),
                        shots,
                    )],
                    true,
                );
            }
            if base.get_attribute(ATTRIB_RANGE) > 0 {
                push(
                    &mut rows,
                    &mut y,
                    STAT_RANGE,
                    Some(actual.get_attribute(ATTRIB_RANGE)),
                    vec![StatIcon::same(
                        "I_RANGE",
                        base.get_attribute(ATTRIB_RANGE),
                        actual.get_attribute(ATTRIB_RANGE),
                    )],
                    true,
                );
            }
            if base.get_attribute(ATTRIB_AMMO) > 0 {
                push(
                    &mut rows,
                    &mut y,
                    STAT_AMMO,
                    Some(actual.get_attribute(ATTRIB_AMMO)),
                    vec![StatIcon::same(
                        "I_AMMO",
                        base.get_attribute(ATTRIB_AMMO),
                        actual.get_attribute(ATTRIB_AMMO),
                    )],
                    true,
                );
            }
        }

        let base_storage = base.get_attribute(ATTRIB_STORAGE);
        let actual_storage = actual.get_attribute(ATTRIB_STORAGE);

        if base_storage > 0 {
            if let Some(icon) = Self::storage_icon(unit) {
                push(
                    &mut rows,
                    &mut y,
                    STAT_CARGO,
                    Some(actual_storage),
                    vec![StatIcon::same(icon, base_storage / 10, actual_storage / 10)],
                    true,
                );
            }
        }

        let raw = cargo_raw_consumption_rate(unit, 1);
        let fuel = cargo_fuel_consumption_rate(unit);
        let power = cargo_power_consumption_rate(unit);
        let life = cargo_life_consumption_rate(unit);
        let gold = cargo_gold_consumption_rate(unit);

        if power < 0 {
            push(
                &mut rows,
                &mut y,
                STAT_PROD,
                Some(-power),
                vec![StatIcon::same("I_POWER", -power, -power)],
                true,
            );
        }
        if life < 0 {
            push(
                &mut rows,
                &mut y,
                STAT_PROD,
                Some(-life),
                vec![StatIcon::same("I_LIFE", -life, -life)],
                true,
            );
        }

        if raw > 0 || fuel > 0 || power > 0 || life > 0 {
            // `UnitStats_DrawUsage`: the number is not drawn (resource_count = 0),
            // the consumption icons go in a row one after another.
            let mut icons = Vec::new();

            for (resource, value) in [
                ("I_RAW", raw),
                ("I_FUEL", fuel),
                ("I_POWER", power),
                ("I_LIFE", life),
                ("I_GOLD", gold),
            ] {
                if value > 0 {
                    icons.push(StatIcon::same(resource, value, value));
                }
            }

            push(&mut rows, &mut y, STAT_USES, None, icons, true);
        }

        if base.get_attribute(ATTRIB_ARMOR) > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_ARMOR,
                Some(actual.get_attribute(ATTRIB_ARMOR)),
                vec![StatIcon::same(
                    "I_ARMOR",
                    base.get_attribute(ATTRIB_ARMOR),
                    actual.get_attribute(ATTRIB_ARMOR),
                )],
                true,
            );
        }
        if base.get_attribute(ATTRIB_HITS) > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_HITS,
                Some(actual.get_attribute(ATTRIB_HITS)),
                vec![StatIcon::same(
                    "I_HITS",
                    base.get_attribute(ATTRIB_HITS),
                    actual.get_attribute(ATTRIB_HITS),
                )],
                true,
            );
        }
        if base.get_attribute(ATTRIB_SCAN) > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_SCAN,
                Some(actual.get_attribute(ATTRIB_SCAN)),
                vec![StatIcon::same(
                    "I_SCAN",
                    base.get_attribute(ATTRIB_SCAN),
                    actual.get_attribute(ATTRIB_SCAN),
                )],
                true,
            );
        }
        if base.get_attribute(ATTRIB_SPEED) > 0 {
            push(
                &mut rows,
                &mut y,
                STAT_SPEED,
                Some(actual.get_attribute(ATTRIB_SPEED)),
                vec![StatIcon::same(
                    "I_SPEED",
                    base.get_attribute(ATTRIB_SPEED),
                    actual.get_attribute(ATTRIB_SPEED),
                )],
                true,
            );
        }
        if base.get_attribute(ATTRIB_TURNS) > 0 {
            // `UnitStats_DrawCost`: level = builder rate * turns, minimum 1;
            // filled — the current turns, "empty" — the base ones (the upgrade delta).
            let rate = build_rate_for(unit);
            let base_level = (rate * base.get_attribute(ATTRIB_TURNS)).max(1);
            let actual_level = (rate * actual.get_attribute(ATTRIB_TURNS)).max(1);

            push(
                &mut rows,
                &mut y,
                STAT_COST,
                Some(actual_level),
                vec![StatIcon {
                    resource: cost_icon,
                    empty_resource: cost_empty_icon,
                    current: actual_level,
                    original: base_level,
                }],
                false,
            );
        }

        rows
    }

    // -----------------------------------------------------------------------
    // Commit (CommitUpgradeChanges + CommitUpgradeChanges override)
    // -----------------------------------------------------------------------

    /// Changed `UnitValues` (`*unitvalues_actual != *unitvalues_base`).
    pub fn changed_upgrades(&self) -> Vec<(UnitType, UnitValues)> {
        let mut changed = Vec::new();

        for index in 0..UNIT_END as usize {
            if self.actual[index] == self.base[index] {
                continue;
            }
            let Ok(unit) = UnitType::try_from(index as u16) else {
                continue;
            };
            let mut values = self.actual[index];
            values.update_version();
            values.mark_as_not_in_use();
            changed.push((unit, values));
        }

        changed
    }

    /// `units_built`/`stats_gold_spent_on_upgrades`: `(start_gold - team_gold)`
    /// minus the current prices of the purchased units and paid cargo
    /// (`CargoMenu::CommitUpgradeChanges`).
    pub fn gold_spent_on_upgrades(&self) -> u32 {
        let mut spent = self.credits_start.wrapping_sub(self.credits);

        for supply in &self.bought {
            spent = spent.wrapping_sub(self.price(supply.unit_type));
        }

        let total: u32 = self
            .loadout()
            .iter()
            .map(|supply| u32::from(supply.cargo))
            .sum();
        let free: u32 = self.free.iter().map(|supply| u32::from(supply.cargo)).sum();
        spent.wrapping_sub((total.saturating_sub(free)) / 5)
    }

    // -----------------------------------------------------------------------
    // Input
    // -----------------------------------------------------------------------

    /// Click on the screen (the same rectangles as the C++ buttons).
    pub fn handle_click(&mut self, x: i32, y: i32) -> CargoAction {
        self.status.clear();

        if DONE_BUTTON.contains(x, y) {
            return CargoAction::Done;
        }
        if CANCEL_BUTTON.contains(x, y) {
            return CargoAction::Cancel;
        }
        if BUY_BUTTON.contains(x, y) {
            self.buy_selected();
            return CargoAction::None;
        }
        if DELETE_BUTTON.contains(x, y) {
            self.delete_selected();
            return CargoAction::None;
        }
        if HELP_BUTTON.contains(x, y) {
            return CargoAction::Help;
        }
        if DESCRIPTION_BUTTON.contains(x, y) {
            self.description = !self.description;
            return CargoAction::None;
        }
        if TOGGLE_BUTTON.contains(x, y) {
            self.toggle_mode();
            return CargoAction::None;
        }
        for (index, filter) in [
            UpgradeFilter::Ground,
            UpgradeFilter::Air,
            UpgradeFilter::Sea,
            UpgradeFilter::Building,
            UpgradeFilter::Combat,
        ]
        .into_iter()
        .enumerate()
        {
            if filter.rect().contains(x, y) {
                self.toggle_filter(index);
                return CargoAction::None;
            }
        }
        if LIST_UP.contains(x, y) {
            self.scroll(ActiveList::Types, -1);
            return CargoAction::None;
        }
        if LIST_DOWN.contains(x, y) {
            self.scroll(ActiveList::Types, 1);
            return CargoAction::None;
        }
        if PURCHASE_UP.contains(x, y) {
            self.scroll(ActiveList::Loadout, -1);
            return CargoAction::None;
        }
        if PURCHASE_DOWN.contains(x, y) {
            self.scroll(ActiveList::Loadout, 1);
            return CargoAction::None;
        }

        if let Some((row, increase)) = self.control_at(x, y) {
            self.adjust_upgrade(row, increase);
            return CargoAction::None;
        }
        if self.click_cargo(x, y) {
            return CargoAction::None;
        }

        if AVAILABLE_AREA.contains(x, y) {
            let row = ((y - AVAILABLE_AREA.uly) / LIST_ROW) as usize;
            let index = self.type_page + row;
            if index < self.types.len() {
                // A repeated click on the selected row — `Select(1)` →
                // `OnUnitTypeConfirmed` → `BuyUnit`.
                if index == self.type_selected && self.info == self.selected_type() {
                    self.buy(index);
                } else {
                    self.select_type(index);
                }
                return CargoAction::None;
            }
        }

        if PURCHASED_AREA.contains(x, y) {
            let row = ((y - PURCHASED_AREA.uly) / LIST_ROW) as usize;
            let index = self.loadout_page + row;
            if index < self.loadout_rows().len() {
                if index == self.loadout_selected
                    && self.info == self.loadout_row(index).map(|supply| supply.unit_type)
                {
                    self.delete_selected();
                } else {
                    self.select_loadout(index);
                }
                return CargoAction::None;
            }
        }

        CargoAction::None
    }

    /// Keyboard (`CargoMenu::ProcessKey`/`AbstractUpgradeMenu::ProcessKey`).
    pub fn handle_key(&mut self, key: Key) -> CargoAction {
        match key {
            Key::Escape => CargoAction::Cancel,
            Key::Enter => CargoAction::Done,
            Key::Up => {
                self.step(-1);
                CargoAction::None
            }
            Key::Down => {
                self.step(1);
                CargoAction::None
            }
            Key::PageUp => {
                self.page(-1);
                CargoAction::None
            }
            Key::PageDown => {
                self.page(1);
                CargoAction::None
            }
            Key::Char(character) => {
                match character.to_ascii_lowercase() {
                    'b' => {
                        self.buy_selected();
                    }
                    'd' => {
                        self.delete_selected();
                    }
                    'u' => self.toggle_mode(),
                    // `CargoMenu::ProcessKey`: `GNW_KB_KEY_SHIFT_DIVIDE` (`?`)
                    // and the Help button code (1001) open `CARGO_SETUP`.
                    '?' => return CargoAction::Help,
                    _ => {}
                }
                CargoAction::None
            }
            _ => CargoAction::None,
        }
    }

    /// Upgrade row under the point (`(row, increase)`).
    pub fn control_at(&self, x: i32, y: i32) -> Option<(usize, bool)> {
        for row in 0..self.controls.len() {
            let (left, right) = self.control_arrows(row)?;
            if left.contains(x, y) {
                return Some((row, false));
            }
            if right.contains(x, y) {
                return Some((row, true));
            }
        }
        None
    }
}

/// Cargo bar material (`Select`: `SetMaterialBar(vbar)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoBarKind {
    /// `VERTRAW`.
    Raw,
    /// `VERTFUEL`.
    Fuel,
    /// `VERTGOLD`.
    Gold,
}

// ---------------------------------------------------------------------------
// Formulas (checked against the C++ vectors, tools/cargo_vectors)
// ---------------------------------------------------------------------------

/// Unit price — `CargoMenu::BuyUnit`: `Cargo_GetRawConsumptionRate(LANDPLT, 1)`
/// (3) × `ATTRIB_TURNS` of the **current** values.
pub fn unit_price(values: &[UnitValues; UNIT_END as usize], unit: UnitType) -> u32 {
    let turns = values[unit as usize].get_attribute(ATTRIB_TURNS).max(0) as u32;

    LANDPLT_RAW_CONSUMPTION * turns
}

/// Type list row price (`PurchaseTypeSelector::Draw`, the `if` parentheses).
pub fn list_price(values: &[UnitValues; UNIT_END as usize], unit: UnitType) -> u32 {
    let turns = values[unit as usize].get_attribute(ATTRIB_TURNS).max(0) as u32;
    let rate = if !unit.is_mobile() {
        cargo_raw_consumption_rate(UnitType::Constructor, 1) as u32
    } else if unit == UnitType::Infiltrator || unit == UnitType::Infantry {
        cargo_raw_consumption_rate(UnitType::TrainingHall, 1) as u32
    } else {
        LANDPLT_RAW_CONSUMPTION
    };

    rate * turns
}

/// Cargo capacity in blocks of 5 (`CargoMenu::Select`): `storage / 5`, for
/// `GOLDTRCK` — 0.
pub fn cargo_capacity(values: &[UnitValues; UNIT_END as usize], unit: UnitType) -> i32 {
    if unit == UnitType::GoldTruck {
        return 0;
    }
    values[unit as usize].get_attribute(ATTRIB_STORAGE) / 5
}

/// Buy price for the original (non-upgraded) values — a wrapper for tests and
/// `CargoMenu` without state.
pub fn unit_cost(unit: UnitType, clan: TeamClan) -> u32 {
    unit_price(&team_base_values(clan), unit)
}

/// Displayed type name — User-Friendly Name (`ResourceManager_GetUnit(...).GetSingularName()`).
pub fn unit_label(unit: UnitType) -> &'static str {
    crate::buildmenu::unit_name(unit)
}

/// `ATTRIB_*` attribute name (for the status).
pub fn attribute_name(attribute: u8) -> &'static str {
    match attribute {
        ATTRIB_ATTACK => "attack",
        ATTRIB_ROUNDS => "rounds",
        ATTRIB_RANGE => "range",
        ATTRIB_AMMO => "ammo",
        ATTRIB_ARMOR => "armor",
        ATTRIB_HITS => "hits",
        ATTRIB_SCAN => "scan",
        ATTRIB_SPEED => "speed",
        ATTRIB_TURNS => "turns",
        ATTRIB_STORAGE => "storage",
        _ => "attribute",
    }
}

/// Full upgrade list: `AbstractUpgradeMenu::PopulateTeamUnitsList` —
/// `IsUnitFiltered` over all 93 types and `IsBetterUnit` sorting.
pub fn upgrade_types(filters: &[bool; 5]) -> Vec<UnitType> {
    let mut types: Vec<UnitType> = (0..UNIT_END)
        .filter_map(|index| UnitType::try_from(index).ok())
        .filter(|unit| is_unit_filtered(*unit, filters))
        .collect();

    if types.len() > 1 {
        for i in 0..types.len() - 1 {
            for j in i + 1..types.len() {
                if is_better_unit(types[i], types[j]) {
                    types.swap(i, j);
                }
            }
        }
    }

    types
}

/// `AbstractUpgradeMenu::IsUnitFiltered` (`abstractupgrademenu.cpp:699`).
pub fn is_unit_filtered(unit: UnitType, filters: &[bool; 5]) -> bool {
    let flags = unit_type_flags(unit);

    if flags & unit_flags::UPGRADABLE == 0 || !is_buildable(unit) {
        return false;
    }

    // `MOBILE_*` bits include amphibians (see `Environment`).
    let class_matches = ((unit.is_ground() || unit.is_amphibious()) && filters[0])
        || (unit.is_air() && filters[1])
        || ((unit.is_sea() || unit.is_amphibious()) && filters[2])
        || (!unit.is_mobile() && filters[3]);
    if !class_matches {
        return false;
    }

    if filters[4] {
        // C++: `UnitsManager_TeamInfo[0].team_units->GetBaseUnitValues(...)`.
        unit.is_combat()
    } else {
        true
    }
}

/// Clan bonus row (for the hint on the mobilization screen).
pub fn clan_summary(clan: TeamClan) -> String {
    if clan == TeamClan::Random {
        return "Clan: random".to_string();
    }

    let mut bonuses = Vec::new();

    for unit in [
        UnitType::Scout,
        UnitType::Tank,
        UnitType::Scanner,
        UnitType::Constructor,
        UnitType::Engineer,
    ] {
        let attack = max_core::unit_values::clan_tradeoff(clan, unit, ATTRIB_ATTACK);
        let speed = max_core::unit_values::clan_tradeoff(clan, unit, ATTRIB_SPEED);
        let turns = max_core::unit_values::clan_tradeoff(clan, unit, ATTRIB_TURNS);

        if attack != 0 {
            bonuses.push(format!("{} {:+} atk", unit_label(unit), attack));
        }
        if speed != 0 {
            bonuses.push(format!("{} {:+} spd", unit_label(unit), speed));
        }
        if turns != 0 {
            bonuses.push(format!("{} {:+} turns", unit_label(unit), turns));
        }
    }

    if bonuses.is_empty() {
        format!("Clan: {}", crate::setup::clan_name(clan))
    } else {
        format!(
            "Clan {}: {}",
            crate::setup::clan_name(clan),
            bonuses.join(", ")
        )
    }
}

// ---------------------------------------------------------------------------
// Screen resources
// ---------------------------------------------------------------------------

/// Type icon in the lists (`I_*`): body and a separate turret/gun frame.
///
/// In C++ `ReportStats_DrawListItemIcon` draws frame 0, and for
/// `TURRET_SPRITE`/`SPINNING_TURRET` — also frame 1 with the shift
/// `angle_offsets[2] >> divisor` (`divisor` = 1 for mobile, 2 for stationary).
#[derive(Debug, Clone)]
pub struct UnitIcon {
    /// Body — frame 0 of the `I_*` resource.
    pub body: SpriteFrame,
    /// Turret/gun — frame 1 (for turret types).
    pub turret: Option<SpriteFrame>,
    /// `angle_offsets[2]` from `D_*` — the turret shift before scaling.
    pub turret_offset: (i32, i32),
}

impl UnitIcon {
    /// Whether there is a separate turret frame.
    pub fn has_turret(&self) -> bool {
        self.turret.is_some()
    }
}

/// `CargoMenu` resources from `MAX.RES`.
#[derive(Debug, Clone)]
pub struct CargoMenuAssets {
    /// `CARGOPIC` background 640×480.
    pub background: Option<BigImage>,
    /// `Buy` (`CRGBUY_U/D`).
    pub buy: Option<MenuButtonPair>,
    /// `CRGBUY_X` — unavailable.
    pub buy_disabled: Option<SimpleImage>,
    /// `Delete` (`CRGDEL_U/D`).
    pub delete: Option<MenuButtonPair>,
    /// `CRGDEL_X`.
    pub delete_disabled: Option<SimpleImage>,
    /// `Cancel` (`BLDCAN_U/D`).
    pub cancel: Option<MenuButtonPair>,
    /// `Done` (`BLDONE_U/D`).
    pub done: Option<MenuButtonPair>,
    /// `Help` (`BLDHLP_U/D`).
    pub help: Option<MenuButtonPair>,
    /// Toggle (`CRGTOG_U/D`).
    pub toggle: Option<MenuButtonPair>,
    /// Description (`BLDDES_U/D`).
    pub description: Option<MenuButtonPair>,
    /// Scroll up (`BLDUP__U/D`).
    pub scroll_up: Option<MenuButtonPair>,
    /// `BLDUP__X`.
    pub scroll_up_disabled: Option<SimpleImage>,
    /// Scroll down (`BLDDWN_U/D`).
    pub scroll_down: Option<MenuButtonPair>,
    /// `BLDDWN_X`.
    pub scroll_down_disabled: Option<SimpleImage>,
    /// Upgrade left arrow (`UPGLFT_U/D`).
    pub left: Option<SimpleImage>,
    /// `UPGLFT_X`.
    pub left_disabled: Option<SimpleImage>,
    /// Upgrade right arrow (`UPGRGT_U/D`).
    pub right: Option<SimpleImage>,
    /// `UPGRGT_X`.
    pub right_disabled: Option<SimpleImage>,
    /// Gold icon `I_GOLD` (upgrade price column).
    pub gold_icon: Option<SimpleImage>,
    /// Unavailable filter (`CRGSELDX`).
    pub filter_disabled: Option<SimpleImage>,
    /// Domain filters (`SELGRD`/`SELAIR`/`SELSEA`/`SELBLD`/`SELCBT`).
    pub filters: [Option<MenuButtonPair>; 5],
    /// `VERTRAW`/`VERTFUEL`/`VERTGOLD` bars (index — [`CargoBarKind`]).
    pub bars: [Option<SimpleImage>; 3],
    /// Type icons (`I_*`): body (frame 0) and turret (frame 1).
    pub icons: Vec<(UnitType, UnitIcon)>,
    /// Statistics row icons (`I_*` simple images, `UnitStats_DrawStats`).
    pub stat_icons: Vec<(&'static str, SimpleImage)>,
    /// Portrait cache `P_*` (`Unit::GetPortrait`, big image 300×240).
    ///
    /// Loaded lazily when a type is selected: 71 big images are not needed at start.
    /// `Arc<Mutex<…>>` — `CargoMenuAssets` remains `Clone`/`Sync`.
    pub portraits: Arc<Mutex<HashMap<UnitType, Option<Arc<BigImage>>>>>,
}

impl CargoMenuAssets {
    /// Loads resources; a missing part is not an error (the button is drawn with a frame).
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<SimpleImage> {
            archive
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };
        let icon = |unit: UnitType, name: &str| -> Option<(UnitType, UnitIcon)> {
            let bytes = archive.read(name).ok()?;
            let sheet = decode_sprite(bytes).ok()?;
            let body = sheet.frame(0)?.clone();
            let turret = sheet.frame(1).cloned();
            // `ReportStats_DrawListItemIcon`: the turret shift — `angle_offsets[2]`.
            let turret_offset = cargo_unit_data::unit_frame_data(unit)
                .and_then(|data| archive.read(data).ok())
                .and_then(|bytes| max_assets::sprite::FrameInfo::decode(&bytes))
                .map(|info| info.turret_offset(2))
                .unwrap_or((0, 0));

            Some((
                unit,
                UnitIcon {
                    body,
                    turret,
                    turret_offset,
                },
            ))
        };

        let mut icons = Vec::new();
        for (unit, name) in UNIT_ICONS {
            if let Some(entry) = icon(unit, name) {
                icons.push(entry);
            }
        }

        let mut stat_icons = Vec::new();
        for name in STAT_ICONS {
            if let Some(image) = simple(name) {
                stat_icons.push((name, image));
            }
        }

        Self {
            background: archive
                .read("CARGOPIC")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            buy: pair("CRGBUY_U", "CRGBUY_D"),
            buy_disabled: simple("CRGBUY_X"),
            delete: pair("CRGDEL_U", "CRGDEL_D"),
            delete_disabled: simple("CRGDEL_X"),
            cancel: pair("BLDCAN_U", "BLDCAN_D"),
            done: pair("BLDONE_U", "BLDONE_D"),
            help: pair("BLDHLP_U", "BLDHLP_D"),
            toggle: pair("CRGTOG_U", "CRGTOG_D"),
            description: pair("BLDDES_U", "BLDDES_D"),
            scroll_up: pair("BLDUP__U", "BLDUP__D"),
            scroll_up_disabled: simple("BLDUP__X"),
            scroll_down: pair("BLDDWN_U", "BLDDWN_D"),
            scroll_down_disabled: simple("BLDDWN_X"),
            left: simple("UPGLFT_U"),
            left_disabled: simple("UPGLFT_X"),
            right: simple("UPGRGT_U"),
            right_disabled: simple("UPGRGT_X"),
            gold_icon: simple("I_GOLD"),
            filter_disabled: simple("CRGSELDX"),
            filters: [
                pair("SELGRD_U", "SELGRD_D"),
                pair("SELAIR_U", "SELAIR_D"),
                pair("SELSEA_U", "SELSEA_D"),
                pair("SELBLD_U", "SELBLD_D"),
                pair("SELCBT_U", "SELCBT_D"),
            ],
            bars: [simple("VERTRAW"), simple("VERTFUEL"), simple("VERTGOLD")],
            icons,
            stat_icons,
            portraits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Type icon (`I_*`, frame 0).
    pub fn icon(&self, unit: UnitType) -> Option<&UnitIcon> {
        self.icons
            .iter()
            .find(|(candidate, _)| *candidate == unit)
            .map(|(_, icon)| icon)
    }

    /// Statistics row icon (`I_*` simple image).
    pub fn stat_icon(&self, resource: &str) -> Option<&SimpleImage> {
        self.stat_icons
            .iter()
            .find(|(name, _)| *name == resource)
            .map(|(_, image)| image)
    }

    /// Type portrait (`Unit::GetPortrait()` → big image `P_*` 300×240).
    ///
    /// Lazily reads and decodes the resource, caches the result (including
    /// the absence of the resource). Returns `Arc` so as not to copy 72 KiB of the frame
    /// on every draw call.
    pub fn portrait(&self, archive: &ResArchive, unit: UnitType) -> Option<Arc<BigImage>> {
        let mut cache = self
            .portraits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(entry) = cache.get(&unit) {
            return entry.clone();
        }

        let image = cargo_unit_data::unit_portrait(unit)
            .and_then(|name| archive.read(name).ok())
            .and_then(|bytes| BigImage::decode(bytes).ok())
            .map(Arc::new);

        cache.insert(unit, image.clone());
        image
    }

    /// Whether the background and key buttons are present.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.done.is_some() && self.cancel.is_some()
    }
}

/// `UnitStats_DrawStats` row icons: attack/shots/range/ammo,
/// cargo, consumption, armor/hits/scan/speed and the `Cost` pips. Both empty
/// variants are here because the `Cost` pair depends on the screen:
/// `I_GOLD`/`I_GOLDE` in `CargoMenu`, `I_RAW`/`I_RAWE` in the `BuildMenu` and the
/// unit statistics window (`UnitStats_DrawStats(..., icon_full, icon_empty)`).
pub const STAT_ICONS: [&str; 15] = [
    "I_HRDATK", "I_SHOTS", "I_RANGE", "I_AMMO", "I_RAW", "I_RAWE", "I_FUEL", "I_GOLD", "I_GOLDE",
    "I_POWER", "I_LIFE", "I_ARMOR", "I_HITS", "I_SCAN", "I_SPEED",
];

/// `Unit::GetIcon()` — `I_*` resources from `SC_UNITS` (units.json), in [`UnitType`]
/// order; types without an icon (`INVALID_ID`) are skipped.
pub const UNIT_ICONS: [(UnitType, &str); 71] = [
    (UnitType::GoldRefinery, "I_TRANSP"),
    (UnitType::PowerStation, "I_POWSTN"),
    (UnitType::PowerGenerator, "I_POWGEN"),
    (UnitType::Barracks, "I_BARRCK"),
    (UnitType::MonopoleMine, "I_SHIELD"),
    (UnitType::Radar, "I_RADAR"),
    (UnitType::MaterialStorage, "I_SMSTOR"),
    (UnitType::FuelStorage, "I_SMFUEL"),
    (UnitType::GoldVault, "I_SMVLT"),
    (UnitType::Depot, "I_DEPOT"),
    (UnitType::Hangar, "I_HANGAR"),
    (UnitType::Dock, "I_DOCK"),
    (UnitType::Connector, "I_CONNEC"),
    (UnitType::Road, "I_ROAD"),
    (UnitType::LandingPad, "I_LANDPD"),
    (UnitType::Shipyard, "I_SHIPYD"),
    (UnitType::LightVehiclePlant, "I_LGHTPL"),
    (UnitType::HeavyVehiclePlant, "I_HVYPLT"),
    (UnitType::SupportPlant, "I_LIFESP"),
    (UnitType::AirUnitsPlant, "I_AIRPLT"),
    (UnitType::Habitat, "I_HABITA"),
    (UnitType::ResearchCentre, "I_RESEAR"),
    (UnitType::EcoSphere, "I_GREENH"),
    (UnitType::RecreationCenter, "I_RECCTR"),
    (UnitType::TrainingHall, "I_TRNHLL"),
    (UnitType::WaterPlatform, "I_WATER"),
    (UnitType::GunTurret, "I_GUNTUR"),
    (UnitType::AntiAir, "I_FXAA"),
    (UnitType::Artillery, "I_ARTYTR"),
    (UnitType::MissileLauncher, "I_FXROCK"),
    (UnitType::ConcreteBlock, "I_BLOCK"),
    (UnitType::Bridge, "I_BRIDGE"),
    (UnitType::MiningStation, "I_MINING"),
    (UnitType::LandMine, "I_LANDMN"),
    (UnitType::SeaMine, "I_SEAMIN"),
    (UnitType::MasterBuilder, "I_MASTER"),
    (UnitType::Constructor, "I_CONTRC"),
    (UnitType::Scout, "I_SCOUT"),
    (UnitType::Tank, "I_TANK"),
    (UnitType::AssaultGun, "I_ARTY"),
    (UnitType::RocketLauncher, "I_ROCKET"),
    (UnitType::MissileCrawler, "I_MISSIL"),
    (UnitType::MobileAntiAir, "I_AA"),
    (UnitType::MineLayer, "I_MNELAY"),
    (UnitType::Surveyor, "I_SURVEY"),
    (UnitType::Scanner, "I_SCANNR"),
    (UnitType::SupplyTruck, "I_SPLYTR"),
    (UnitType::GoldTruck, "I_GOLDTR"),
    (UnitType::Engineer, "I_ENGINR"),
    (UnitType::Bulldozer, "I_BULLDZ"),
    (UnitType::RepairUnit, "I_REPAIR"),
    (UnitType::FuelTruck, "I_FUELTR"),
    (UnitType::ArmouredPersonnelCarrier, "I_COLNST"),
    (UnitType::Infiltrator, "I_COMMAN"),
    (UnitType::Infantry, "I_INFANT"),
    (UnitType::Escort, "I_ESCORT"),
    (UnitType::Corvette, "I_CORVET"),
    (UnitType::Gunboat, "I_GUNBT"),
    (UnitType::Submarine, "I_SUB"),
    (UnitType::SeaTransport, "I_SEATRN"),
    (UnitType::MissileCruiser, "I_MSLCR"),
    (UnitType::SeaMineLayer, "I_SEAMNL"),
    (UnitType::CargoShip, "I_CARGOS"),
    (UnitType::Fighter, "I_FIGHTR"),
    (UnitType::Bomber, "I_BOMBER"),
    (UnitType::AirTransport, "I_AIRTRN"),
    (UnitType::Awac, "I_AWAC"),
    (UnitType::Juggernaut, "I_JUGGER"),
    (UnitType::AlienTank, "I_ALNTAN"),
    (UnitType::AlienAssaultGun, "I_ALNASG"),
    (UnitType::AlienPlane, "I_ALNPLA"),
];

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

impl Mobilization {
    /// Draws the screen (1:1 `cargomenu.cpp` coordinates) into the 640×480 frame.
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        assets: &Assets,
        status: &str,
        mouse: Option<(i32, i32)>,
    ) -> u32 {
        if frame.width() != SCREEN_WIDTH as u32 || frame.height() != SCREEN_HEIGHT as u32 {
            return 0;
        }
        let Some(cargo) = assets.cargo.as_ref() else {
            return 0;
        };
        let Some(background) = cargo.background.as_ref() else {
            return 0;
        };

        let palette = UiPalette::from_big_image(background);
        let mut painted = draw_big_image(frame, background);

        let font = GnwFontId::Font5.font();
        let green = palette.rgba(crate::setup::GREEN);
        let text = palette.rgba(crate::setup::TEXT);
        let yellow = palette.rgba(0x04);
        let chrome = palette.rgba(0x05);
        let gold_font = palette.rgba(165);

        // Titles and labels.
        painted += draw_text_gnw_centered(frame, TITLE, font, "Purchase Menu", green).max(0) as u32;
        painted += draw_text_gnw(
            frame,
            AVAILABLE_HEADER.ulx,
            center_y(AVAILABLE_HEADER, font),
            font,
            "Available",
            text,
        )
        .max(0) as u32;
        painted += draw_text_right(
            frame,
            PRICE_COLUMN.lrx(),
            center_y(AVAILABLE_HEADER, font),
            font,
            "Cost",
            yellow,
        );
        draw_hline(frame, palette.rgba(0x05), 482, 625, AVAILABLE_LINE_Y);

        painted +=
            draw_text_gnw_centered(frame, PURCHASED_HEADER, font, "Purchased", text).max(0) as u32;
        draw_hline(frame, chrome, 337, 455, PURCHASED_LINE_Y);

        painted += draw_text_gnw(
            frame,
            DESCRIPTION_LABEL.ulx + 1,
            center_y(DESCRIPTION_LABEL, font),
            font,
            "Description",
            gold_font,
        )
        .max(0) as u32;
        for (rect, label) in [
            (COST_LABEL, "Cost"),
            (CREDIT_LABEL, "Credit"),
            (CARGO_LABEL, "Cargo"),
        ] {
            painted += draw_text_gnw_centered(frame, rect, font, label, gold_font).max(0) as u32;
        }

        // The Cost/Credit/Cargo values are not drawn in the C++ port (only the
        // labels remain); the credits counter — the number on the gold bar (`gold_background`),
        // the cargo counter — above the slider. Adding numbers over the labels is impossible:
        // they fall onto the gold bar (361,276) and the cargo counter (421,276).

        // Portrait/statistics of the selected type.
        painted += self.draw_info(frame, assets, cargo, &palette);

        // Lists.
        painted += self.draw_type_list(frame, cargo, &palette);
        painted += self.draw_loadout_list(frame, cargo, &palette);

        // Upgrades column: arrows + prices.
        painted += self.draw_controls(frame, cargo, &palette);

        // Cargo slider and gold bar.
        painted += self.draw_gold_bar(frame, cargo, &palette);
        painted += self.draw_cargo_bar(frame, cargo, &palette);

        // Buttons.
        painted += draw_button(
            frame,
            &palette,
            cargo.buy.as_ref(),
            cargo.buy_disabled.as_ref(),
            BUY_BUTTON,
            "Buy",
            mouse,
            self.can_buy(),
        );
        painted += draw_button(
            frame,
            &palette,
            cargo.delete.as_ref(),
            cargo.delete_disabled.as_ref(),
            DELETE_BUTTON,
            "Delete",
            mouse,
            self.can_delete(),
        );
        painted += draw_button(
            frame,
            &palette,
            cargo.cancel.as_ref(),
            None,
            CANCEL_BUTTON,
            "Cancel",
            mouse,
            true,
        );
        painted += draw_button(
            frame,
            &palette,
            cargo.done.as_ref(),
            None,
            DONE_BUTTON,
            "Done",
            mouse,
            true,
        );
        painted += draw_button(
            frame,
            &palette,
            cargo.help.as_ref(),
            None,
            HELP_BUTTON,
            "",
            mouse,
            true,
        );
        painted += draw_filter_button(
            frame,
            cargo.description.as_ref(),
            &palette,
            DESCRIPTION_BUTTON,
            self.description,
        );

        // Mode toggle: both labels, the color depends on the state
        // (`SetCaption(834a, 20,0,80,15, DarkOrange, Gold)` and
        // `SetCaption(e02a, 20,15,80,31)`).
        painted += self.draw_toggle(frame, cargo, &palette);

        // Domain filters.
        for (index, filter) in [
            UpgradeFilter::Ground,
            UpgradeFilter::Air,
            UpgradeFilter::Sea,
            UpgradeFilter::Building,
            UpgradeFilter::Combat,
        ]
        .into_iter()
        .enumerate()
        {
            painted += draw_filter(
                frame,
                cargo,
                &palette,
                filter,
                index,
                self.filters[index],
                self.mode,
            );
        }

        // List arrows.
        painted += draw_scroll(
            frame,
            cargo.scroll_up.as_ref(),
            cargo.scroll_up_disabled.as_ref(),
            &palette,
            LIST_UP,
            self.type_page > 0,
            mouse,
        );
        painted += draw_scroll(
            frame,
            cargo.scroll_down.as_ref(),
            cargo.scroll_down_disabled.as_ref(),
            &palette,
            LIST_DOWN,
            self.type_page + AVAILABLE_ROWS < self.types.len(),
            mouse,
        );
        painted += draw_scroll(
            frame,
            cargo.scroll_up.as_ref(),
            cargo.scroll_up_disabled.as_ref(),
            &palette,
            PURCHASE_UP,
            self.loadout_page > 0,
            mouse,
        );
        painted += draw_scroll(
            frame,
            cargo.scroll_down.as_ref(),
            cargo.scroll_down_disabled.as_ref(),
            &palette,
            PURCHASE_DOWN,
            self.loadout_page + PURCHASED_ROWS < self.loadout_rows().len(),
            mouse,
        );

        // Clan bonuses (our addition) and the status line.
        let clan_line = clan_summary(self.clan);
        painted += draw_text_gnw(frame, 11, 466, font, &clan_line, green).max(0) as u32;

        let line = if status.is_empty() {
            self.status.as_str()
        } else {
            status
        };
        painted += draw_text_gnw(
            frame,
            11,
            451,
            font,
            &line.chars().take(44).collect::<String>(),
            text,
        )
        .max(0) as u32;

        painted
    }

    /// Portrait + description + statistics table (`AbstractUpgradeMenu::DrawUnitInfo`).
    fn draw_info(
        &self,
        frame: &mut RgbaBuffer,
        assets: &Assets,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let Some(unit) = self.info else {
            return 0;
        };
        let mut painted = 0;

        // Portrait: `WindowManager_LoadBigImage(GetPortrait(), ...)` — big image
        // `P_*` 300×240, `ulx/uly` = 0, scale 1:1, window palette (`CARGOPIC`).
        // The `unit_portrait_window` window 280×240 clips the right 20 px.
        if let Some(portrait) = cargo.portrait(&assets.max_res, unit) {
            painted += draw_big_image_at(
                frame,
                &portrait,
                palette,
                (PORTRAIT.ulx, PORTRAIT.uly),
                PORTRAIT,
            );
        } else {
            // C++ `buf_fill(unit_portrait_window.buffer, 300, 240, width, 0)`.
            frame.fill_rect(PORTRAIT, palette.rgba(STAT_COLOR_OUTLINE));
        }

        // Description — over the portrait, `button_description_rest_state`.
        if self.description {
            painted += draw_description(frame, palette, unit);
        }

        painted += draw_stats(frame, cargo, palette, &self.stats_rows());

        painted
    }

    /// "Available" list with icons and prices.
    fn draw_type_list(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let text = palette.rgba(crate::setup::TEXT);
        let yellow = palette.rgba(0x04);
        let white = palette.rgba(0xFF);
        let lut = team_lut(self.team);
        let mut painted = 0;

        for row in 0..AVAILABLE_ROWS {
            let index = self.type_page + row;
            let Some(&unit) = self.types.get(index) else {
                break;
            };
            let y = AVAILABLE_AREA.uly + row as i32 * LIST_ROW;

            painted += draw_text_gnw(
                frame,
                AVAILABLE_AREA.ulx + 35,
                center_y(PixelRect::new(0, y, 0, LIST_ROW), font),
                font,
                &fit_label(unit_label(unit), 88, font),
                text,
            )
            .max(0) as u32;
            painted += draw_text_right(
                frame,
                PRICE_COLUMN.lrx(),
                center_y(PixelRect::new(0, y, 0, LIST_ROW), font),
                font,
                &self.list_price(unit).to_string(),
                yellow,
            );

            if let Some(icon) = cargo.icon(unit) {
                let stationary = !unit.is_mobile();
                let zoom = if stationary { 16 } else { 32 };
                painted += draw_icon(
                    frame,
                    palette,
                    unit,
                    icon,
                    (AVAILABLE_AREA.ulx + 16, y + 16),
                    zoom,
                    &lut,
                );
            }

            if index == self.type_selected {
                painted += draw_selection(
                    frame,
                    white,
                    AVAILABLE_AREA.ulx,
                    y,
                    AVAILABLE_AREA.width,
                    LIST_ROW,
                    1,
                );
            }
        }

        painted
    }

    /// "Purchased" list (`CargoSelector::Draw`).
    fn draw_loadout_list(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let text = palette.rgba(crate::setup::TEXT);
        let white = palette.rgba(0xFF);
        let lut = team_lut(self.team);
        let rows = self.loadout_rows();
        let mut painted = 0;

        for row in 0..PURCHASED_ROWS {
            let index = self.loadout_page + row;
            let Some(supply) = rows.get(index).copied() else {
                break;
            };
            let y = PURCHASED_AREA.uly + row as i32 * LIST_ROW;
            let mut count = 1;
            for earlier in rows.iter().take(index) {
                if earlier.unit_type == supply.unit_type {
                    count += 1;
                }
            }
            let mut label = format!("{} {count}", unit_label(supply.unit_type));
            let storage = self.actual[supply.unit_type as usize].get_attribute(ATTRIB_STORAGE);
            if storage > 0 {
                if supply.cargo == 0 {
                    label.push_str(" (Empty)");
                } else if i32::from(supply.cargo) < storage {
                    label.push_str(&match supply.unit_type {
                        UnitType::FuelTruck => format!(" ({} Fuel)", supply.cargo),
                        UnitType::GoldTruck => format!(" ({} Gold)", supply.cargo),
                        _ => format!(" ({} Mat.)", supply.cargo),
                    });
                } else {
                    label.push_str(" (Full)");
                }
            }

            painted += draw_wrapped(
                frame,
                PixelRect::new(
                    PURCHASED_AREA.ulx + 32,
                    y,
                    PURCHASED_AREA.width - 32,
                    LIST_ROW,
                ),
                font,
                &label,
                text,
                3,
            );

            if let Some(icon) = cargo.icon(supply.unit_type) {
                let stationary = !supply.unit_type.is_mobile();
                let zoom = if stationary { 16 } else { 32 };
                painted += draw_icon(
                    frame,
                    palette,
                    supply.unit_type,
                    icon,
                    (PURCHASED_AREA.ulx + 16, y + 16),
                    zoom,
                    &lut,
                );
            }

            if index == self.loadout_selected {
                painted += draw_selection(
                    frame,
                    white,
                    PURCHASED_AREA.ulx,
                    y,
                    PURCHASED_AREA.width,
                    LIST_ROW,
                    2,
                );
            }
        }

        painted
    }

    /// Upgrades column: arrows and prices (`AbstractUpgradeMenu::DrawUnitStats`).
    fn draw_controls(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let chrome = palette.rgba(0x05);
        let mut painted = 0;

        for row in 0..self.controls.len() {
            let (left, right) = match self.control_arrows(row) {
                Some(rects) => rects,
                None => break,
            };
            let quote = self.quote(row);
            let can_up = quote
                .as_ref()
                .is_some_and(|quote| quote.can_increase(self.credits));
            let can_down = quote.as_ref().is_some_and(UpgradeQuote::can_decrease);

            painted += draw_upgrade_arrow(
                frame,
                palette,
                cargo.left.as_ref(),
                cargo.left_disabled.as_ref(),
                left,
                can_down,
            );
            painted += draw_upgrade_arrow(
                frame,
                palette,
                cargo.right.as_ref(),
                cargo.right_disabled.as_ref(),
                right,
                can_up,
            );

            if let Some(quote) = quote {
                let cost = quote.cost();
                if cost > 0 && cost < UPGRADE_COST_LIMIT {
                    // `ReportStats_DrawNumber` in `cost_background`: the number
                    // is right-aligned at x=345, the icon — x=346.
                    painted += draw_text_right(
                        frame,
                        COST.ulx + 24,
                        left.uly + 5,
                        font,
                        &cost.to_string(),
                        chrome,
                    );
                    if let Some(icon) = cargo.gold_icon.as_ref() {
                        painted +=
                            blit_simple(frame, palette, icon, COST.ulx + 25, left.uly + 3, true);
                    }
                }
            }
        }

        painted
    }

    /// Gold bar (`gold_background` + `VERTGOLD`).
    fn draw_gold_bar(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let mut painted = 0;
        let inner = GOLD_BAR.height - 25;
        let fill = if self.credits_start > 0 {
            ((i64::from(inner) * i64::from(self.credits)) / i64::from(self.credits_start))
                .clamp(0, i64::from(inner)) as i32
        } else {
            0
        };

        if let Some(bar) = cargo.bars[2].as_ref() {
            painted += draw_vertical_bar(
                frame,
                palette,
                bar,
                GOLD_BAR.ulx,
                GOLD_BAR.uly + 25,
                GOLD_BAR.width,
                inner,
                fill,
            );
        }

        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(GOLD_BAR.ulx, GOLD_BAR.uly, GOLD_BAR.width, font.height()),
            font,
            &self.credits.to_string(),
            palette.rgba(0x05),
        )
        .max(0) as u32;

        painted
    }

    /// Cargo slider (`LimitedScrollbar::RefreshScreen`).
    fn draw_cargo_bar(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let capacity = self.cargo_capacity();
        let value = self.cargo_value();
        let fill = if capacity != 0 {
            (value * CARGO_BAR.height) / capacity
        } else {
            0
        }
        .clamp(0, CARGO_BAR.height);
        let kind = self.cargo_bar_kind();
        let mut painted = 0;

        if let Some(bar) = cargo.bars[kind as usize].as_ref() {
            painted += draw_vertical_bar(
                frame,
                palette,
                bar,
                CARGO_BAR.ulx,
                CARGO_BAR.uly,
                CARGO_BAR.width,
                CARGO_BAR.height,
                fill,
            );
        }

        painted += draw_text_gnw_centered(
            frame,
            CARGO_AMOUNT,
            font,
            &(value * 5).to_string(),
            palette.rgba(0xFF),
        )
        .max(0) as u32;

        painted
    }

    /// `CRGTOG` toggle: both labels, the colors — like C++ `SetCaption`.
    fn draw_toggle(
        &self,
        frame: &mut RgbaBuffer,
        cargo: &CargoMenuAssets,
        palette: &UiPalette,
    ) -> u32 {
        let font = GnwFontId::Font5.font();
        let down = self.mode == CargoMode::Upgrade;
        let mut painted = 0;

        if let Some(pair) = cargo.toggle.as_ref() {
            painted += blit_simple(
                frame,
                palette,
                pair.image(down),
                TOGGLE_BUTTON.ulx,
                TOGGLE_BUTTON.uly,
                true,
            );
        }

        // up: Buy Units — DarkOrange (5), Upgrades — Gold (165);
        // down: the opposite.
        let (buy_color, upgrade_color) = if down {
            (palette.rgba(165), palette.rgba(0x05))
        } else {
            (palette.rgba(0x05), palette.rgba(165))
        };
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(TOGGLE_BUTTON.ulx + 20, TOGGLE_BUTTON.uly, 80, 15),
            font,
            "Buy Units",
            buy_color,
        )
        .max(0) as u32;
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(TOGGLE_BUTTON.ulx + 20, TOGGLE_BUTTON.uly + 15, 80, 16),
            font,
            "Upgrades",
            upgrade_color,
        )
        .max(0) as u32;

        painted
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

/// Team color (the team-slot recolor LUT).
pub(crate) fn team_lut(team: TeamId) -> ColorLut {
    ColorLut::for_team(
        TeamColors::ALL
            .get(team.0 as usize)
            .copied()
            .unwrap_or(TeamColors::Derelict),
    )
}

/// `Cargo_GetRawConsumptionRate(Builder_GetBuilderType(team, unit), 1)` —
/// rate of the `Cost` row (`UnitStats_DrawCost`); see [`Mobilization::build_rate`].
pub(crate) fn build_rate_for(unit: UnitType) -> i32 {
    builder_type(unit).map_or(0, |builder| cargo_raw_consumption_rate(builder, 1))
}

/// Vertical coordinate of single-line text centered in the rectangle
/// (`Text_TextBox`: `((height + 1) - font_height) / 2`).
pub(crate) fn center_y(rect: PixelRect, font: &max_render::text::GnwFont) -> i32 {
    rect.uly + ((rect.height + 1) - font.height()).max(0) / 2
}

/// Horizontal line (C++ `draw_line`).
fn draw_hline(frame: &mut RgbaBuffer, color: Rgba, x1: i32, x2: i32, y: i32) {
    for x in x1..=x2 {
        frame.set(x, y, color);
    }
}

/// Text right-aligned to `right` (`ReportStats_DrawText`).
pub(crate) fn draw_text_right(
    frame: &mut RgbaBuffer,
    right: i32,
    y: i32,
    font: &max_render::text::GnwFont,
    text: &str,
    color: Rgba,
) -> u32 {
    let width = font.text_width(text);
    draw_text_gnw(frame, right - width, y, font, text, color).max(0) as u32
}

/// Clips a string by width like `Text_Blit` (`swidth`): the glyph is drawn while
/// its end has not crossed the boundary.
fn clip_gnw_text<'a>(text: &'a str, max_width: i32, font: &GnwFont) -> &'a str {
    let mut width = 0;

    for (index, character) in text.char_indices() {
        width += font.char_width(character);

        if width > max_width {
            return &text[..index];
        }
    }

    text
}

/// `ReportStats_DrawNumber`: `> 1e6` → `%.1fM`, `> 1000` → `%.1fk`, otherwise
/// an integer (`%lld`).
fn report_stats_number(value: i32) -> String {
    let number = i64::from(value);

    if number > 1_000_000 {
        format!("{:.1}M", number as f64 / 1_000_000.0)
    } else if number > 1_000 {
        format!("{:.1}k", number as f64 / 1_000.0)
    } else {
        number.to_string()
    }
}

/// Unit description over the portrait.
///
/// `Text_TextBox(unit_portrait_window.buffer, dialog_window.width, description,
/// 16, 17, 280, 230, GNW_TEXT_OUTLINE | 0xA2, false, false)`:
/// font 5, line spacing `Text_GetHeight + 1` (outline), 20 lines,
/// a black copy at `(x+1, y+1)` (`Text_BlitTTF` + `GNW_TEXT_OUTLINE`).
pub(crate) fn draw_description(frame: &mut RgbaBuffer, palette: &UiPalette, unit: UnitType) -> u32 {
    let text = cargo_unit_data::unit_description(unit);

    if text.is_empty() {
        return 0;
    }

    let font = GnwFontId::Font5.font();
    let color = palette.rgba(STAT_COLOR_LABEL);
    let outline = palette.rgba(STAT_COLOR_OUTLINE);
    let line_height = font.height() + 1;
    let max_rows = (DESCRIPTION.height / line_height).max(0) as usize;
    let lines = wrap_gnw_text(text, DESCRIPTION.width);
    let mut painted = 0;

    for (row, line) in lines.iter().take(max_rows).enumerate() {
        let y = DESCRIPTION.uly + row as i32 * line_height;

        painted +=
            draw_text_gnw(frame, DESCRIPTION.ulx + 1, y + 1, font, line, outline).max(0) as u32;
        painted += draw_text_gnw(frame, DESCRIPTION.ulx, y, font, line, color).max(0) as u32;
    }

    painted
}

/// Statistics table (`UnitStats_DrawStats`): rows of 19 px, the number to the right
/// from x=24, the label from x=28 (clip 49 px), the pips from x=76, the red line under the row.
pub(crate) fn draw_stats(
    frame: &mut RgbaBuffer,
    cargo: &CargoMenuAssets,
    palette: &UiPalette,
    rows: &[StatRow],
) -> u32 {
    let font = GnwFontId::Font5.font();
    let number_color = palette.rgba(STAT_COLOR_NUMBER);
    let label_color = palette.rgba(STAT_COLOR_LABEL);
    let line_color = palette.rgba(STAT_COLOR_LINE);
    let mut painted = 0;

    for row in rows {
        // `UnitStats_DrawText`: `((19 - Text_GetHeight()) / 2)`.
        let text_y = row.y + (STAT_ROW - font.height()).max(0) / 2;

        if let Some(value) = row.value {
            let text = report_stats_number(value);
            painted += draw_text_right(frame, STATS.ulx + 24, text_y, font, &text, number_color);
        }

        let label = clip_gnw_text(row.label, STAT_LABEL_WIDTH, font);
        painted +=
            draw_text_gnw(frame, STATS.ulx + 28, text_y, font, label, label_color).max(0) as u32;

        // `draw_line(buffer, sw, 2, 18, line_length - 2, 18, COLOR_RED)`.
        if row.separator {
            draw_hline(
                frame,
                line_color,
                STATS.ulx + 2,
                STATS.ulx + STATS.width - 2,
                row.y + 18,
            );
        }

        let mut offset = STAT_ICON_X;
        for icons in &row.icons {
            offset += draw_stat_icons(
                frame,
                palette,
                cargo,
                STATS.ulx + offset,
                row.y,
                STATS.width - offset,
                icons,
            );
        }
    }

    painted
}

/// One pips group — `UnitStats_DrawIcons`: the step scale fits the width, the upgrade
/// delta (`original - current`) is drawn with empty icons after the red line.
#[allow(clippy::too_many_arguments)]
fn draw_stat_icons(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    cargo: &CargoMenuAssets,
    base_x: i32,
    row_y: i32,
    max_width: i32,
    icons: &StatIcon,
) -> i32 {
    let Some(normal) = cargo.stat_icon(icons.resource) else {
        return 0;
    };
    let empty = cargo.stat_icon(icons.empty_resource).unwrap_or(normal);

    // `original -= current` — the upgrade delta (may be negative).
    let original = icons.original - icons.current;
    let mut max_width = max_width;

    if original != 0 {
        max_width -= 5;
    }

    let image_width = i32::from(normal.width);
    let image_height = i32::from(normal.height);
    let row_step = if icons.current >= 6 {
        (18 - image_height) / 4
    } else {
        0
    };
    // `address = &buffer[(((18 - (height * 4)) - image_height) / 2) * sw]`.
    let top = row_y + ((18 - row_step * 4) - image_height) / 2;

    let current_scaled = (icons.current + 4) / 5;
    let current_offset = icons.current - (current_scaled - 1) * 5;
    let original_scaled = (original + 4) / 5;
    let original_offset = original - (original_scaled - 1) * 5;
    let mut full_scaled = current_scaled + original_scaled;

    max_width -= image_width * full_scaled;

    // `var_2C` — the total width of the steps that would remain at the full
    // icon scale.
    let mut full_step_width = full_scaled * 4 - (5 - current_offset);

    if original_scaled != 0 {
        full_step_width -= 5 - original_offset;
    }

    let (spacing, shrink) = if full_step_width != 0 {
        if max_width < full_step_width {
            if original_scaled != 0 {
                full_scaled -= 1;
            }

            let denominator = full_scaled - 1;
            let shrink = if denominator > 0 {
                (full_scaled + (full_step_width - max_width) - 2) / denominator
            } else {
                0
            };

            (1, shrink.max(0))
        } else {
            ((max_width / full_step_width).min(image_width), 0)
        }
    } else {
        (image_width, 0)
    };

    let stride = spacing * 4 + image_width - shrink;
    let mut advance = draw_stat_pips(
        frame,
        palette,
        base_x,
        top,
        normal,
        icons.current,
        spacing,
        stride,
        row_step,
    );

    if original != 0 {
        // `draw_line(&buffer[var_24 + 2], sw, 0, 2, 0, 16, COLOR_RED)` —
        // a vertical line over the full row height.
        let x = base_x + advance + 2;

        for y in (row_y + 2)..=(row_y + 16) {
            frame.set(x, y, palette.rgba(STAT_COLOR_LINE));
        }

        advance += 5;
        advance += draw_stat_pips(
            frame,
            palette,
            base_x + advance,
            top,
            empty,
            original,
            spacing,
            stride,
            row_step,
        );
    }

    advance
}

/// `UnitStats_DrawIcon`: up to `count` icons in blocks of 5, with the shift
/// `reminder * row_step` vertically.
#[allow(clippy::too_many_arguments)]
fn draw_stat_pips(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    base_x: i32,
    base_y: i32,
    image: &SimpleImage,
    count: i32,
    spacing: i32,
    stride: i32,
    row_step: i32,
) -> i32 {
    let mut offset = 0;

    for index in 0..count.max(0) {
        let reminder = index % 5;
        offset = stride * (index / 5) + spacing * reminder;

        draw_stat_image(
            frame,
            palette,
            image,
            base_x + offset,
            base_y + reminder * row_step,
        );
    }

    i32::from(image.width) + offset
}

/// `UnitStats_DrawImage`: copies `SimpleImage` pixels, skipping the
/// transparent index (the first pixel of the resource).
fn draw_stat_image(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    x: i32,
    y: i32,
) -> u32 {
    let mut painted = 0;

    for py in 0..image.height as u32 {
        for px in 0..image.width as u32 {
            let Some(index) = image.pixel(px, py) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            if frame.set(x + px as i32, y + py as i32, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Text centered in a rectangle with wrapping and vertical centering
/// (`Text_TextBox`), at most `max_rows` lines.
fn draw_wrapped(
    frame: &mut RgbaBuffer,
    rect: PixelRect,
    font: &max_render::text::GnwFont,
    text: &str,
    color: Rgba,
    max_rows: usize,
) -> u32 {
    let mut lines = wrap_gnw_text(text, rect.width);
    if lines.len() > max_rows {
        lines.truncate(max_rows);
        if let Some(last) = lines.last_mut() {
            last.push_str("..");
        }
    }
    let rows = lines.len() as i32;
    let mut y = rect.uly + ((rect.height + 1) - font.height() * rows).max(0) / 2;
    let mut painted = 0;

    for line in &lines {
        painted += draw_text_gnw(frame, rect.ulx, y, font, line, color).max(0) as u32;
        y += font.height();
    }

    painted
}

/// Selected row frame (a port of `GameManager_DrawUnitSelector`: 1 px, color
/// 0xFF; for the "Purchased" list — double).
fn draw_selection(
    frame: &mut RgbaBuffer,
    color: Rgba,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    thickness: i32,
) -> u32 {
    let mut painted = 0;

    for step in 0..thickness {
        let left = x + step;
        let top = y + step;
        let right = x + width - 1 - step;
        let bottom = y + height - 1 - step;

        for px in left..=right {
            if frame.set(px, top, color) {
                painted += 1;
            }
            if frame.set(px, bottom, color) {
                painted += 1;
            }
        }
        for py in top..=bottom {
            if frame.set(left, py, color) {
                painted += 1;
            }
            if frame.set(right, py, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// Button: picture (or an honest frame), when unavailable — `_X`.
#[allow(clippy::too_many_arguments)]
fn draw_button(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    pair: Option<&MenuButtonPair>,
    disabled: Option<&SimpleImage>,
    rect: PixelRect,
    label: &str,
    _mouse: Option<(i32, i32)>,
    enabled: bool,
) -> u32 {
    let image = if enabled {
        pair.map(|pair| pair.image(false))
    } else {
        disabled.or_else(|| pair.map(|pair| pair.image(false)))
    };

    match image {
        Some(image) => {
            let down = pair.map(|pair| pair.image(true));
            let down = down.unwrap_or(image);
            draw_image_button(frame, palette, image, down, rect, label, false)
        }
        None => {
            let mut painted = frame.fill_rect(rect, palette.rgba(crate::setup::TEXT));
            if !label.is_empty() {
                painted += draw_text_gnw_centered(
                    frame,
                    rect,
                    GnwFontId::Font1.font(),
                    label,
                    palette.rgba(crate::setup::GREEN),
                )
                .max(0) as u32;
            }
            painted
        }
    }
}

/// Domain filter: `_D` (pressed) for the active one, `CRGSELDX` in buy mode.
fn draw_filter(
    frame: &mut RgbaBuffer,
    cargo: &CargoMenuAssets,
    palette: &UiPalette,
    filter: UpgradeFilter,
    index: usize,
    active: bool,
    mode: CargoMode,
) -> u32 {
    let rect = filter.rect();

    if mode == CargoMode::Buy {
        if let Some(disabled) = cargo.filter_disabled.as_ref() {
            return blit_simple(frame, palette, disabled, rect.ulx, rect.uly, true);
        }
        return 0;
    }

    match cargo.filters.get(index).and_then(|pair| pair.as_ref()) {
        Some(pair) => blit_simple(frame, palette, pair.image(active), rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// Toggle button (`BLDDES`): `_D` when the description is on.
fn draw_filter_button(
    frame: &mut RgbaBuffer,
    pair: Option<&MenuButtonPair>,
    palette: &UiPalette,
    rect: PixelRect,
    active: bool,
) -> u32 {
    match pair {
        Some(pair) => blit_simple(frame, palette, pair.image(active), rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// List scroll arrow with an `_X` variant.
fn draw_scroll(
    frame: &mut RgbaBuffer,
    pair: Option<&MenuButtonPair>,
    disabled: Option<&SimpleImage>,
    palette: &UiPalette,
    rect: PixelRect,
    enabled: bool,
    _mouse: Option<(i32, i32)>,
) -> u32 {
    if enabled {
        match pair {
            Some(pair) => blit_simple(frame, palette, pair.image(false), rect.ulx, rect.uly, true),
            None => 0,
        }
    } else if let Some(disabled) = disabled {
        blit_simple(frame, palette, disabled, rect.ulx, rect.uly, true)
    } else {
        0
    }
}

/// Upgrade arrow (`UPGLFT`/`UPGRGT`, the unavailable one — `*_X`).
fn draw_upgrade_arrow(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: Option<&SimpleImage>,
    disabled: Option<&SimpleImage>,
    rect: PixelRect,
    enabled: bool,
) -> u32 {
    let chosen = if enabled { image } else { disabled.or(image) };
    match chosen {
        Some(chosen) => blit_simple(frame, palette, chosen, rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// Bar (`LoadVerticalBar`): a 20-pixel texture from bottom to top.
#[allow(clippy::too_many_arguments)]
fn draw_vertical_bar(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    bar: &SimpleImage,
    x: i32,
    y: i32,
    width: i32,
    capacity: i32,
    fill: i32,
) -> u32 {
    if fill <= 0 || capacity <= 0 {
        return 0;
    }

    let sprite_width = bar.width as i32;
    let sprite_height = bar.height as i32;
    let fill = fill.min(sprite_height).min(capacity);
    let offset = (width - sprite_width).max(0) / 2;
    let mut painted = 0;

    for row in 0..fill {
        let target_y = y + capacity - fill + row;
        for column in 0..sprite_width {
            let Some(index) = bar.pixel(column as u32, row as u32) else {
                continue;
            };
            if index == bar.transparent {
                continue;
            }
            if frame.set(x + offset + column, target_y, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Unit icon (`ReportStats_DrawListItemIcon`): the frame with the hotspot in
/// the center, scale `zoom` (64 — 1:1), team slot recoloring.
pub(crate) fn draw_icon(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    unit: UnitType,
    icon: &UnitIcon,
    anchor: (i32, i32),
    zoom: u32,
    lut: &ColorLut,
) -> u32 {
    let mut painted = draw_icon_frame(frame, palette, &icon.body, anchor, zoom, lut);

    // `ReportStats_DrawListItemIcon`: for turret types — the second `I_*` frame
    // with the `angle_offsets[2] >> divisor` shift.
    if unit_type_flags(unit) & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
        if let Some(turret) = icon.turret.as_ref() {
            let divisor = if !unit.is_mobile() {
                2
            } else {
                1
            };
            let offset = (
                icon.turret_offset.0 >> divisor,
                icon.turret_offset.1 >> divisor,
            );

            painted += draw_icon_frame(
                frame,
                palette,
                turret,
                (anchor.0 + offset.0, anchor.1 + offset.1),
                zoom,
                lut,
            );
        }
    }

    painted
}

/// One `I_*` icon frame with the `zoom` scale (64 — 1:1) and team recoloring.
fn draw_icon_frame(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    icon: &SpriteFrame,
    anchor: (i32, i32),
    zoom: u32,
    lut: &ColorLut,
) -> u32 {
    let zoom = zoom.max(1);
    let width = ((u32::from(icon.width) * zoom) / 64).max(1) as i32;
    let height = ((u32::from(icon.height) * zoom) / 64).max(1) as i32;
    let ulx = anchor.0 - ((i32::from(icon.hotx) * zoom as i32) / 64);
    let uly = anchor.1 - ((i32::from(icon.hoty) * zoom as i32) / 64);
    let mut painted = 0;

    for y in 0..height {
        let source_y = ((y as i64 * 64) / i64::from(zoom)) as u32;
        let source_y = source_y.min(u32::from(icon.height).saturating_sub(1));
        for x in 0..width {
            let source_x = ((x as i64 * 64) / i64::from(zoom)) as u32;
            let source_x = source_x.min(u32::from(icon.width).saturating_sub(1));
            let Some(index) = icon.pixel(source_x, source_y) else {
                continue;
            };
            if frame.set(ulx + x, uly + y, palette.rgba(lut.map(index))) {
                painted += 1;
            }
        }
    }

    painted
}

/// Name clipping by width (like `Text_Blit` with `swidth`).
fn fit_label(text: &str, max_width: i32, font: &max_render::text::GnwFont) -> String {
    if font.text_width(text) <= max_width {
        return text.to_string();
    }

    let mut out = String::new();
    for character in text.chars() {
        let mut candidate = out.clone();
        candidate.push(character);
        if font.text_width(&format!("{candidate}..")) > max_width {
            break;
        }
        out = candidate;
    }

    format!("{out}..")
}

#[cfg(test)]
#[path = "mobilize_tests.rs"]
mod tests;
