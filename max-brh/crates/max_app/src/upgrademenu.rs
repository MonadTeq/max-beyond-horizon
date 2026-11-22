//! Purchasable upgrades screen — C++ `AbstractUpgradeMenu` / `UpgradeMenu`
//! (`abstractupgrademenu.cpp`, `upgrademenu.cpp`, `upgradecontrol.cpp`).
//!
//! # Oracle
//!
//! `UPGRADE` background 640×480 (own palette), the buttons are the same resources as in C++:
//! `BLDONE`, `BLDHLP`, `BLDCAN` (y=452), `BLDUP__`/`BLDDWN` (471/491, 387),
//! `BLDDES` (292,264), `SELGRD`/`SELAIR`/`SELSEA`/`SELBLD`/`SELCBT`
//! (467/500/533/565/598, 411), `UPGLFT`/`UPGRGT` (283/301, row 293+19·i),
//! and the `I_GOLD` icon.
//!
//! Layout:
//! * unit portrait — area (11,13) 280×240 (in C++ — `GetPortrait()`, FLIC;
//!   here — a 2× sprite, as in `BuildMenu`);
//! * statistics — (11,293) 250×174; price — (321,293) 40×174; upgrade
//!   arrows — x=283/301; gold — (361,276) 40×140;
//! * team type list — (482,80) 143×290, 9 rows of 32 px
//!   (`UnitTypeSelector`: `max_item_count = height/32`), the selected row —
//!   `page_max_index` (in C++ `GetLast()` = `unit_types[page_max_index]`);
//! * the title `_(0b85)` («Unit Upgrades») — `Text_TextBox(327,7,158,18)`,
//!   the `Cost`/`Gold` labels — (320,283)/(358,284).
//!
//! # Differences (deliberate)
//!
//! * `UnitStats_DrawStats` draws a grid of `EI_*` icons; here — a textual
//!   table of the same 13 attributes (font 5) in the same area. The values and
//!   formulas match, the picture does not; the golden frame records our version.
//! * The unit description (`BLDDES`/`GetDescription`) is not ported — the button
//!   exists, but there is no text in the Rust client data.
//! * The `KCARG0`/`NDONE0`/`NCANC0` sounds are not played (the audio layer is separate).
//! * The menu is opened by the `buy upg` item of the unit command menu (C++
//!   `UnitsManager_Popup_OnClick_BuyUpgrade`, `units_manager.cpp:956`) or
//!   by the `U` key as a shortcut for a selected unit of own team.
//! * Network: in C++, `CommitUpgradeChanges` sends packet 09 (team state) and
//!   packet 10 for each changed type. Packet 09 does not exist in the Rust protocol,
//!   so in the network only packets **10** are sent (`Command::UpgradeUnitType`),
//!   while gold/`gold_spent` are synchronized locally — the limitation is
//!   documented in `max-brh-docs/architecture/client/protocol.md` §8.4.

use max_assets::image::{BigImage, SimpleImage};
use max_core::access::unit_type_flags;
// Explicit import of the classification extension type (`UnitTypeClassify` — sealed in
// max_core); no glob re-exports (phase 5.2).
use max_core::UnitTypeClassify;
use max_core::enums::{unit_flags, ResearchTopic, UnitType, UNIT_END};
use max_core::game::GameState;
use max_core::production::BUILDABLE;
use max_core::research::{
    upgrade_packet_values, UpgradeQuote, RESEARCH_TOPIC_FACTORS, UPGRADE_COST_LIMIT,
};
use max_core::team::TeamId;
use max_core::unit_values::{
    UnitValues, ATTRIB_AGENT_ADJUST, ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK,
    ATTRIB_ATTACK_RADIUS, ATTRIB_FUEL, ATTRIB_HITS, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN,
    ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS, BASE_UNIT_VALUES,
};
use max_core::victory::{
    cargo_fuel_consumption_rate, cargo_life_consumption_rate, cargo_power_consumption_rate,
    cargo_raw_consumption_rate,
};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::assets::Assets;
use crate::buildmenu::unit_name;
use crate::menu::{blit_simple, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::Key;

/// Original frame width.
pub const SCREEN_WIDTH: i32 = 640;
/// Original frame height.
pub const SCREEN_HEIGHT: i32 = 480;

/// Portrait (C++ `unit_portrait_window` 11,13 + 280×240).
pub const PORTRAIT: PixelRect = PixelRect::new(11, 13, 280, 240);
/// Statistics area (`stats_background` 11,293,250,174).
pub const STATS: PixelRect = PixelRect::new(11, 293, 250, 174);
/// Price column (`cost_background` 321,293,40,174).
pub const COST: PixelRect = PixelRect::new(321, 293, 40, 174);
/// Upgrade arrows column (`button_background` 283,293,38,174).
pub const CONTROL_BUTTONS: PixelRect = PixelRect::new(283, 293, 38, 174);
/// Gold bar (`gold_background` 361,276,40,140).
pub const GOLD_BAR: PixelRect = PixelRect::new(361, 276, 40, 140);
/// Type list (`wininfo` 482,80 — 625,370).
pub const LIST_AREA: PixelRect = PixelRect::new(482, 80, 143, 290);
/// List row height (`UnitTypeSelector`: 32 px).
pub const LIST_ROW: i32 = 32;
/// Number of visible list rows (`height / 32`).
pub const LIST_ROWS: i32 = 9;
/// Scroll up (`BLDUP__U`, 18×17).
pub const SCROLL_UP: PixelRect = PixelRect::new(471, 387, 18, 17);
/// Scroll down (`BLDDWN_U`, 18×17).
pub const SCROLL_DOWN: PixelRect = PixelRect::new(491, 387, 18, 17);
/// Description (`BLDDES_U`, 17×17).
pub const DESCRIPTION: PixelRect = PixelRect::new(292, 264, 17, 17);
/// `Done` (`BLDONE_U`, 54×23).
pub const DONE: PixelRect = PixelRect::new(447, 452, 54, 23);
/// `Help` (`BLDHLP_U`, 26×23).
pub const HELP: PixelRect = PixelRect::new(420, 452, 26, 23);
/// `Cancel` (`BLDCAN_U`, 62×23).
pub const CANCEL: PixelRect = PixelRect::new(357, 452, 62, 23);
/// Title (`Text_TextBox` 327,7,158,18).
pub const TITLE: PixelRect = PixelRect::new(327, 7, 158, 18);

/// List filters (C++ static rest-state: land/air/sea/buildings on,
/// combat off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeFilter {
    /// Ground mobile.
    Ground,
    /// Air.
    Air,
    /// Sea.
    Sea,
    /// Stationary buildings.
    Building,
    /// Only units with an attack (C++ `button_combat_rest_state`).
    Combat,
}

impl UpgradeFilter {
    /// Button rectangle (32×31).
    pub const fn rect(self) -> PixelRect {
        match self {
            Self::Ground => PixelRect::new(467, 411, 32, 31),
            Self::Air => PixelRect::new(500, 411, 32, 31),
            Self::Sea => PixelRect::new(533, 411, 32, 31),
            Self::Building => PixelRect::new(565, 411, 32, 31),
            Self::Combat => PixelRect::new(598, 411, 32, 31),
        }
    }
}

/// Menu action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeAction {
    /// Nothing.
    None,
    /// `Cancel`/Esc.
    Cancel,
    /// `Done`/Enter — changes committed.
    Commit,
    /// `Help`/`?` — open `UPGRADES_SETUP` (`HelpMenu_Menu`).
    Help,
}

/// State of the open upgrade menu.
#[derive(Debug, Clone)]
pub struct UpgradeMenu {
    /// Team.
    pub team: TeamId,
    /// Available types (filtered, C++ `IsBetterUnit` order).
    pub unit_types: Vec<UnitType>,
    /// Selected index (`page_max_index`).
    pub selected: usize,
    /// First visible row (`page_min_index`).
    pub page_min: usize,
    /// Current values at the moment of opening (`unitvalues_base`).
    pub base: [UnitValues; UNIT_END as usize],
    /// Editable values (`unitvalues_actual`).
    pub actual: [UnitValues; UNIT_END as usize],
    /// Team gold in the menu.
    pub gold: u32,
    /// Gold at the moment of opening.
    pub start_gold: u32,
    /// Filters (C++ static rest-state).
    pub filters: [bool; 5],
    /// Description button.
    pub description: bool,
    /// Upgrade rows of the selected type: `(ATTRIB_*, uly)`.
    pub controls: Vec<(u8, i32)>,
    /// Status line (errors).
    pub status: String,
}

impl UpgradeMenu {
    /// Opens the menu for a team — C++ `UpgradeMenu::UpgradeMenu(team, complex)`
    /// (complex is not used in C++ either).
    pub fn new(state: &GameState, team: TeamId) -> Option<Self> {
        let team_index = team.0 as usize;
        let units = state.team_units.get(team_index)?;
        let base = units.current_values;
        let actual = base;

        let mut menu = Self {
            team,
            unit_types: Vec::new(),
            selected: 0,
            page_min: 0,
            base,
            actual,
            gold: units.gold,
            start_gold: units.gold,
            filters: [true, true, true, true, false],
            description: true,
            controls: Vec::new(),
            status: String::new(),
        };
        menu.rebuild_list(state, None);
        Some(menu)
    }

    /// Rebuilds the list of available types (`PopulateTeamUnitsList`), keeping
    /// the selected type if it remains.
    pub fn rebuild_list(&mut self, state: &GameState, keep: Option<UnitType>) {
        let previous = keep.or_else(|| self.selected_type());

        let mut types: Vec<UnitType> = (0..UNIT_END)
            .filter_map(|index| UnitType::try_from(index).ok())
            .filter(|unit_type| self.is_unit_filtered(state, *unit_type))
            .collect();

        // C++ `PopulateTeamUnitsList`: bubble sort, swap when
        // `IsBetterUnit(unit_types[i], unit_types[j])`.
        if types.len() > 1 {
            for i in 0..types.len() - 1 {
                for j in i + 1..types.len() {
                    if is_better_unit(types[i], types[j]) {
                        types.swap(i, j);
                    }
                }
            }
        }

        self.unit_types = types;
        self.selected = previous
            .and_then(|unit_type| self.unit_types.iter().position(|item| *item == unit_type))
            .unwrap_or(0);
        self.ensure_visible();
        self.rebuild_controls(state);
    }

    /// Selected type.
    pub fn selected_type(&self) -> Option<UnitType> {
        self.unit_types.get(self.selected).copied()
    }

    /// `IsUnitFiltered` (`abstractupgrademenu.cpp:699`).
    fn is_unit_filtered(&self, state: &GameState, unit_type: UnitType) -> bool {
        let flags = unit_type_flags(unit_type);
        if flags & unit_flags::UPGRADABLE == 0 || !is_buildable(unit_type) {
            return false;
        }

        // `MOBILE_*` bits include amphibians (see `Environment`).
        let land = unit_type.is_ground() || unit_type.is_amphibious();
        let sea = unit_type.is_sea() || unit_type.is_amphibious();
        let class_matches = (land && self.filters[0])
            || (unit_type.is_air() && self.filters[1])
            || (sea && self.filters[2])
            || (!unit_type.is_mobile() && self.filters[3]);
        if !class_matches {
            return false;
        }

        if self.filters[4] {
            // C++: `UnitsManager_TeamInfo[0]` base values, not the menu team.
            let base = state
                .team_units
                .first()
                .map(|units| units.base_unit_values(unit_type as u16).attack)
                .unwrap_or(0);
            base > 0
        } else {
            true
        }
    }

    /// `IsBetterUnit` (`abstractupgrademenu.cpp:727`).
    fn ensure_visible(&mut self) {
        if self.selected < self.page_min {
            self.page_min = self.selected;
        }
        let max_page = self.unit_types.len().saturating_sub(LIST_ROWS as usize);
        self.page_min = self.page_min.min(max_page);
        if self.selected >= self.page_min + LIST_ROWS as usize {
            self.page_min = self.selected + 1 - LIST_ROWS as usize;
        }
    }

    /// Selection scrolling (`UnitTypeSelector::ProcessKeys` UP/DOWN).
    pub fn step(&mut self, delta: i32, state: &GameState) {
        if self.unit_types.is_empty() {
            return;
        }
        let last = self.unit_types.len() - 1;
        self.selected = (self.selected as i32 + delta).clamp(0, last as i32) as usize;
        self.ensure_visible();
        self.rebuild_controls(state);
    }

    /// Page scrolling (PageUp/PageDown — `max_item_count` rows).
    pub fn page(&mut self, delta: i32, state: &GameState) {
        self.step(delta * LIST_ROWS, state);
    }

    /// Toggles the filter and rebuilds the list.
    pub fn toggle_filter(&mut self, filter: UpgradeFilter, state: &GameState) {
        let index = filter as usize;
        self.filters[index] = !self.filters[index];
        self.rebuild_list(state, None);
    }

    /// Builds the upgrade controls for the selected type — C++
    /// `DrawUnitInfo` → `AddUpgradeMilitary/AdjustRow*/AddUpgradeGeneric/Mobile`.
    pub fn rebuild_controls(&mut self, state: &GameState) {
        self.controls.clear();
        let Some(unit_type) = self.selected_type() else {
            return;
        };
        let index = unit_type as usize;
        let true_base = &state.team_units[self.team.0 as usize].base_values[index];
        let actual = self.actual[index];
        let mut uly = COST.uly;
        let mut rows: Vec<(u8, i32)> = Vec::new();
        fn push(rows: &mut Vec<(u8, i32)>, uly: &mut i32, attribute: u8) {
            rows.push((attribute, *uly));
            *uly += 19;
        }

        // `AddUpgradeMilitary`.
        if true_base.get_attribute(ATTRIB_ATTACK) > 0 {
            push(&mut rows, &mut uly, ATTRIB_ATTACK);
            if true_base.get_attribute(ATTRIB_ROUNDS) > 0 {
                push(&mut rows, &mut uly, ATTRIB_ROUNDS);
            }
            if true_base.get_attribute(ATTRIB_RANGE) > 0 {
                push(&mut rows, &mut uly, ATTRIB_RANGE);
            }
            if true_base.get_attribute(ATTRIB_AMMO) > 0 {
                push(&mut rows, &mut uly, ATTRIB_AMMO);
            }
        }

        // `AdjustRowStorage`.
        if actual.get_attribute(ATTRIB_STORAGE) != 0
            && matches!(
                unit_type,
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
            uly += 19;
        }

        // `AdjustRowConsumptions`.
        let raw = cargo_raw_consumption_rate(unit_type, 1);
        let fuel = cargo_fuel_consumption_rate(unit_type);
        let power = cargo_power_consumption_rate(unit_type);
        let life = cargo_life_consumption_rate(unit_type);
        if power < 0 {
            uly += 19;
        }
        if life < 0 {
            uly += 19;
        }
        if raw > 0 || fuel > 0 || power > 0 || life > 0 {
            uly += 19;
        }

        // `AddUpgradeGeneric`.
        if true_base.get_attribute(ATTRIB_ARMOR) > 0 {
            push(&mut rows, &mut uly, ATTRIB_ARMOR);
        }
        if true_base.get_attribute(ATTRIB_HITS) > 0 {
            push(&mut rows, &mut uly, ATTRIB_HITS);
        }
        if true_base.get_attribute(ATTRIB_SCAN) > 0 {
            push(&mut rows, &mut uly, ATTRIB_SCAN);
        }

        // `AddUpgradeMobile`.
        if true_base.get_attribute(ATTRIB_SPEED) > 0 {
            push(&mut rows, &mut uly, ATTRIB_SPEED);
        }

        self.controls = rows;
    }

    /// Control quote row for the selected type.
    pub fn quote(&self, state: &GameState, row: usize) -> Option<UpgradeQuote> {
        let (attribute, _) = *self.controls.get(row)?;
        let unit_type = self.selected_type()?;
        let index = unit_type as usize;
        let true_base = state.team_units[self.team.0 as usize].base_values[index];
        let team_base = true_base.get_attribute(attribute) as u16;

        let level = if attribute as usize >= RESEARCH_TOPIC_FACTORS.len() {
            0
        } else {
            match ResearchTopic::try_from(attribute).ok() {
                Some(topic) => {
                    state.teams[self.team.0 as usize].research_topics[topic as usize].research_level
                }
                None => 0,
            }
        };
        let factor = max_core::research::research_factor(i32::from(team_base), level) as u16;

        let mut quote = UpgradeQuote::new(
            attribute,
            team_base,
            self.base[index].get_attribute(attribute) as u16,
            factor,
        );
        quote.actual = self.actual[index].get_attribute(attribute) as u16;
        Some(quote)
    }

    /// Line (ATTRIB) -> control index.
    pub fn control_at(&self, x: i32, y: i32) -> Option<(usize, bool)> {
        for (row, (_, uly)) in self.controls.iter().enumerate() {
            let left = PixelRect::new(CONTROL_BUTTONS.ulx, *uly, 18, 17);
            let right = PixelRect::new(CONTROL_BUTTONS.ulx + 18, *uly, 18, 17);
            if left.contains(x, y) {
                return Some((row, false));
            }
            if right.contains(x, y) {
                return Some((row, true));
            }
        }
        None
    }

    /// Click on an upgrade arrow: `false` if the change is impossible.
    pub fn adjust(&mut self, state: &GameState, row: usize, increase: bool) -> bool {
        let Some(unit_type) = self.selected_type() else {
            return false;
        };
        let index = unit_type as usize;
        let Some(mut quote) = self.quote(state, row) else {
            return false;
        };

        let changed = if increase {
            quote.increase(&mut self.gold)
        } else {
            quote.decrease(&mut self.gold)
        };

        if changed {
            self.actual[index].set_attribute(quote.attribute, i32::from(quote.actual));
        }
        changed
    }

    /// Handles a click; returns the action.
    pub fn click(&mut self, state: &GameState, x: i32, y: i32) -> UpgradeAction {
        if DONE.contains(x, y) {
            return UpgradeAction::Commit;
        }
        if CANCEL.contains(x, y) {
            return UpgradeAction::Cancel;
        }
        if HELP.contains(x, y) {
            return UpgradeAction::Help;
        }
        if SCROLL_UP.contains(x, y) {
            self.step(-1, state);
            return UpgradeAction::None;
        }
        if SCROLL_DOWN.contains(x, y) {
            self.step(1, state);
            return UpgradeAction::None;
        }
        if DESCRIPTION.contains(x, y) {
            self.description = !self.description;
            return UpgradeAction::None;
        }
        for filter in [
            UpgradeFilter::Ground,
            UpgradeFilter::Air,
            UpgradeFilter::Sea,
            UpgradeFilter::Building,
            UpgradeFilter::Combat,
        ] {
            if filter.rect().contains(x, y) {
                self.toggle_filter(filter, state);
                return UpgradeAction::None;
            }
        }

        for row in 0..LIST_ROWS {
            let row_rect = PixelRect::new(
                LIST_AREA.ulx,
                LIST_AREA.uly + row * LIST_ROW,
                LIST_AREA.width,
                LIST_ROW,
            );
            if row_rect.contains(x, y) {
                let index = self.page_min + row as usize;
                if index < self.unit_types.len() {
                    self.selected = index;
                    self.ensure_visible();
                    self.rebuild_controls(state);
                }
                return UpgradeAction::None;
            }
        }

        if let Some((row, increase)) = self.control_at(x, y) {
            if !self.adjust(state, row, increase) {
                self.status = if increase {
                    "Cannot upgrade: gold or cost limit".to_string()
                } else {
                    "Already at base value".to_string()
                };
            }
        }

        UpgradeAction::None
    }

    /// Keyboard.
    pub fn key(&mut self, state: &GameState, key: Key) -> UpgradeAction {
        match key {
            Key::Escape => UpgradeAction::Cancel,
            Key::Enter => UpgradeAction::Commit,
            Key::Up => {
                self.step(-1, state);
                UpgradeAction::None
            }
            Key::Down => {
                self.step(1, state);
                UpgradeAction::None
            }
            Key::PageUp => {
                self.page(-1, state);
                UpgradeAction::None
            }
            Key::PageDown => {
                self.page(1, state);
                UpgradeAction::None
            }
            // `UpgradeMenu::ProcessKey`: `GNW_KB_KEY_SHIFT_DIVIDE` (`?`) and the Help
            // button code (1001) open `UPGRADES_SETUP`.
            Key::Char('?') => UpgradeAction::Help,
            _ => UpgradeAction::None,
        }
    }

    /// `CommitUpgradeChanges`: write the changed types into `TeamUnits`, debit
    /// gold and `stats_gold_spent_on_upgrades`.
    ///
    /// Returns the list of changed types for packet 10 (network).
    pub fn commit(&mut self, state: &mut GameState) -> Vec<(UnitType, [u16; 13])> {
        let mut changed = Vec::new();
        let team_index = self.team.0 as usize;

        for index in 0..UNIT_END as usize {
            if self.actual[index] == self.base[index] {
                continue;
            }
            let mut values = self.actual[index];
            values.update_version();
            values.mark_as_not_in_use();
            state.team_units[team_index].current_values[index] = values;
            if let Ok(unit_type) = UnitType::try_from(index as u16) {
                changed.push((unit_type, upgrade_packet_values(&values)));
            }
        }

        state.team_units[team_index].gold = self.gold;
        let spent = self.start_gold.saturating_sub(self.gold);
        state.teams[team_index].stats_gold_spent_on_upgrades = state.teams[team_index]
            .stats_gold_spent_on_upgrades
            .saturating_add(spent);

        changed
    }

    /// Draws the screen over the frame (C++ `DrawUnitStats` + `win_draw`).
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets, state: &GameState) -> u32 {
        if frame.width() != SCREEN_WIDTH as u32 || frame.height() != SCREEN_HEIGHT as u32 {
            return 0;
        }

        let Some(menu_assets) = assets.upgrade.as_ref() else {
            return 0;
        };
        let background = menu_assets.background.as_ref();
        frame.fill_rect(
            PixelRect::new(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT),
            max_render::buffer::Rgba::rgb(0, 0, 0),
        );
        let mut painted = match background {
            Some(background) => crate::menu::draw_big_image(frame, background),
            None => 0,
        };
        let palette = background
            .map(UiPalette::from_big_image)
            .or_else(|| UiPalette::from_assets(assets))
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return painted;
        };
        let font = GnwFontId::Font5.font();
        let green = palette.rgba(crate::setup::GREEN);
        let text = palette.rgba(crate::setup::TEXT);
        let yellow = palette.rgba(crate::setup::YELLOW.0);
        let dim = palette.rgba(0x5A);

        // Title.
        painted += draw_text_gnw_centered(frame, TITLE, font, "Unit Upgrades", green).max(0) as u32;
        painted += draw_text_gnw(frame, COST.ulx - 1, 283, font, "Cost", text).max(0) as u32;
        painted += draw_text_gnw(frame, GOLD_BAR.ulx - 3, 284, font, "Gold", text).max(0) as u32;

        // Portrait 2× (C++ `GetPortrait`, FLIC not ported).
        if let Some(unit_type) = self.selected_type() {
            if let Some(sprite) = assets.sprite(unit_type) {
                if let Some(sprite_frame) = sprite.frame_for(0, 0) {
                    let colors = max_render::TeamColors::ALL
                        .get(self.team.0 as usize)
                        .copied()
                        .unwrap_or(max_render::TeamColors::Derelict);
                    let renderer = max_render::SpriteRenderer::new(
                        &assets.palette,
                        &assets.brightness,
                        max_render::ColorLut::for_team(colors),
                        max_render::FULL_BRIGHTNESS_LEVEL,
                    );
                    renderer.draw_unit_scaled(
                        frame,
                        sprite_frame,
                        None,
                        (PORTRAIT.ulx + 140, PORTRAIT.uly + 120),
                        (0, 0),
                        128,
                    );

                    // Turret/gun as a separate frame (as on the map): C++
                    // `GetPortrait()` — a big image where the vehicle already has the turret.
                    if let Some(turret) = sprite.turret_frame(0) {
                        renderer.draw_turret_scaled(
                            frame,
                            turret,
                            None,
                            (PORTRAIT.ulx + 140, PORTRAIT.uly + 120),
                            (0, 0),
                            sprite.turret_offset(0),
                            128,
                        );
                    }

                    painted += 1;
                }
            }

            painted += draw_text_gnw(
                frame,
                STATS.ulx + 2,
                STATS.uly + 2,
                font,
                unit_name(unit_type),
                green,
            )
            .max(0) as u32;

            // Attribute table (C++ `UnitStats_DrawStats`, here as text).
            let values = &self.actual[unit_type as usize];
            let labels: [(u8, &str); 13] = [
                (ATTRIB_ATTACK, "Attack"),
                (ATTRIB_ROUNDS, "Rounds"),
                (ATTRIB_RANGE, "Range"),
                (ATTRIB_AMMO, "Ammo"),
                (ATTRIB_ARMOR, "Armor"),
                (ATTRIB_HITS, "Hits"),
                (ATTRIB_SPEED, "Speed"),
                (ATTRIB_SCAN, "Scan"),
                (ATTRIB_TURNS, "Turns"),
                (ATTRIB_STORAGE, "Storage"),
                (ATTRIB_FUEL, "Fuel"),
                (ATTRIB_ATTACK_RADIUS, "Blast"),
                (ATTRIB_AGENT_ADJUST, "Agent"),
            ];
            for (row, (attribute, label)) in labels.iter().enumerate() {
                let y = STATS.uly + 16 + row as i32 * 12;
                if y + 12 > STATS.lry() {
                    break;
                }
                painted += draw_text_gnw(frame, STATS.ulx + 2, y, font, label, text).max(0) as u32;
                let value = values.get_attribute(*attribute).to_string();
                let width = font.text_width(&value);
                painted += draw_text_gnw(frame, STATS.ulx + 120 - width, y, font, &value, yellow)
                    .max(0) as u32;
            }
        }

        // Upgrade rows: arrows, price.
        for (row, (attribute, uly)) in self.controls.iter().enumerate() {
            let quote = self.quote(state, row);
            let left_rect = PixelRect::new(CONTROL_BUTTONS.ulx, *uly, 18, 17);
            let right_rect = PixelRect::new(CONTROL_BUTTONS.ulx + 18, *uly, 18, 17);

            let can_up = quote
                .as_ref()
                .is_some_and(|quote| quote.can_increase(self.gold));
            let can_down = quote.as_ref().is_some_and(UpgradeQuote::can_decrease);

            painted += draw_upgrade_arrow(
                frame,
                &palette,
                menu_assets.left.as_ref(),
                menu_assets.left_disabled.as_ref(),
                left_rect,
                can_down,
            );
            painted += draw_upgrade_arrow(
                frame,
                &palette,
                menu_assets.right.as_ref(),
                menu_assets.right_disabled.as_ref(),
                right_rect,
                can_up,
            );

            if let Some(quote) = quote {
                let cost = quote.cost();
                if cost > 0 && cost < UPGRADE_COST_LIMIT {
                    if let Some(icon) = menu_assets.gold_icon.as_ref() {
                        painted += blit_simple(frame, &palette, icon, COST.ulx + 2, *uly + 4, true);
                    }
                    let label = cost.to_string();
                    let width = font.text_width(&label);
                    painted += draw_text_gnw(
                        frame,
                        COST.ulx + COST.width - width - 2,
                        *uly + 4,
                        font,
                        &label,
                        if !can_up { dim } else { yellow },
                    )
                    .max(0) as u32;
                } else if cost >= UPGRADE_COST_LIMIT {
                    painted += draw_text_gnw(frame, COST.ulx + 2, *uly + 4, font, "max", dim).max(0)
                        as u32;
                }
            }

            // Attribute label (in C++ the column already does it; here it helps
            // navigate between the table and the price rows).
            let label = match *attribute {
                ATTRIB_ATTACK => "ATK",
                ATTRIB_ROUNDS => "RND",
                ATTRIB_RANGE => "RNG",
                ATTRIB_AMMO => "AMMO",
                ATTRIB_ARMOR => "ARM",
                ATTRIB_HITS => "HITS",
                ATTRIB_SCAN => "SCAN",
                ATTRIB_SPEED => "SPD",
                _ => "",
            };
            painted +=
                draw_text_gnw(frame, STATS.ulx + 140, *uly + 4, font, label, dim).max(0) as u32;
        }

        // Team type list.
        for row in 0..LIST_ROWS as usize {
            let index = self.page_min + row;
            let Some(unit_type) = self.unit_types.get(index).copied() else {
                break;
            };
            let y = LIST_AREA.uly + row as i32 * LIST_ROW + 8;
            let selected = index == self.selected;
            let color = if selected { green } else { text };
            let name = fit_label(unit_name(unit_type), LIST_AREA.width - 6, font);
            painted += draw_text_gnw(frame, LIST_AREA.ulx + 3, y, font, &name, color).max(0) as u32;
        }

        // Filters.
        for (filter, pair) in [
            (UpgradeFilter::Ground, menu_assets.filter_ground.as_ref()),
            (UpgradeFilter::Air, menu_assets.filter_air.as_ref()),
            (UpgradeFilter::Sea, menu_assets.filter_sea.as_ref()),
            (
                UpgradeFilter::Building,
                menu_assets.filter_building.as_ref(),
            ),
            (UpgradeFilter::Combat, menu_assets.filter_combat.as_ref()),
        ] {
            let active = self.filters[filter as usize];
            if let Some(pair) = pair {
                painted += blit_simple(
                    frame,
                    &palette,
                    pair.image(!active),
                    filter.rect().ulx,
                    filter.rect().uly,
                    true,
                );
            }
        }

        // Description button.
        if let Some(pair) = menu_assets.description.as_ref() {
            painted += blit_simple(
                frame,
                &palette,
                pair.image(!self.description),
                DESCRIPTION.ulx,
                DESCRIPTION.uly,
                true,
            );
        }

        // Done/Help/Cancel buttons.
        if let Some(pair) = menu_assets.done.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                DONE,
                "Done",
                false,
            );
        }
        if let Some(pair) = menu_assets.cancel.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                CANCEL,
                "Cancel",
                false,
            );
        }
        if let Some(pair) = menu_assets.help.as_ref() {
            painted += blit_simple(frame, &palette, pair.image(false), HELP.ulx, HELP.uly, true);
        }

        // List arrows.
        if let Some(pair) = menu_assets.scroll_up.as_ref() {
            painted += blit_simple(
                frame,
                &palette,
                pair.image(false),
                SCROLL_UP.ulx,
                SCROLL_UP.uly,
                true,
            );
        }
        if let Some(pair) = menu_assets.scroll_down.as_ref() {
            painted += blit_simple(
                frame,
                &palette,
                pair.image(false),
                SCROLL_DOWN.ulx,
                SCROLL_DOWN.uly,
                true,
            );
        }

        // Gold bar: C++ `LoadVerticalBar(..., VERTGOLD)`.
        let bar_top = GOLD_BAR.uly + 25;
        let bar_height = GOLD_BAR.height - 25;
        let fill = if self.start_gold > 0 {
            ((bar_height as i64 * i64::from(self.gold)) / i64::from(self.start_gold))
                .clamp(0, i64::from(bar_height)) as i32
        } else {
            0
        };
        frame.fill_rect(
            PixelRect::new(GOLD_BAR.ulx, bar_top, GOLD_BAR.width, bar_height),
            palette.rgba(0x18),
        );
        frame.fill_rect(
            PixelRect::new(
                GOLD_BAR.ulx,
                bar_top + (bar_height - fill),
                GOLD_BAR.width,
                fill,
            ),
            yellow,
        );
        painted += 1;

        let gold_label = self.gold.to_string();
        let width = font.text_width(&gold_label);
        painted += draw_text_gnw(
            frame,
            GOLD_BAR.ulx + (GOLD_BAR.width - width) / 2,
            GOLD_BAR.uly,
            font,
            &gold_label,
            yellow,
        )
        .max(0) as u32;

        if !self.status.is_empty() {
            painted += draw_text_gnw(
                frame,
                STATS.ulx + 2,
                STATS.lry() - 12,
                font,
                &fit_label(&self.status, STATS.width - 4, font),
                green,
            )
            .max(0) as u32;
        }

        painted
    }
}

/// `UPGLFT`/`UPGRGT` arrow (disabled variant `UPGLFT_X`/`UPGRGT_X`).
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

/// Clipping of a long label (like `buildmenu::fit_label`).
fn fit_label(text: &str, max_width: i32, font: &max_render::text::GnwFont) -> String {
    if font.text_width(text) <= max_width {
        return text.to_string();
    }
    let mut result = String::new();
    for character in text.chars() {
        let mut candidate = result.clone();
        candidate.push(character);
        if font.text_width(&candidate) > max_width {
            break;
        }
        result = candidate;
    }
    result
}

/// `AbstractUpgradeMenu::IsBuildable` (`abstractupgrademenu.cpp:681`).
///
/// Game rules `max_is_buildable` do not depend on the team; mines are a special
/// case: they are not built but are upgraded if their deployer is buildable.
pub fn is_buildable(unit_type: UnitType) -> bool {
    match unit_type {
        UnitType::LandMine => is_buildable(UnitType::MineLayer),
        UnitType::SeaMine => is_buildable(UnitType::SeaMineLayer),
        _ => in_builder_lists(unit_type),
    }
}

fn in_builder_lists(unit_type: UnitType) -> bool {
    BUILDABLE
        .iter()
        .any(|(_, units)| units.contains(&unit_type))
}

/// `AbstractUpgradeMenu::IsBetterUnit` (`abstractupgrademenu.cpp:727`).
pub fn is_better_unit(unit_type1: UnitType, unit_type2: UnitType) -> bool {
    // `MOBILE_*` bits include amphibians (see `Environment`).
    let land1 = unit_type1.is_ground() || unit_type1.is_amphibious();
    let land2 = unit_type2.is_ground() || unit_type2.is_amphibious();

    if land1 != land2 {
        return !land1;
    }

    let sea1 = unit_type1.is_sea() || unit_type1.is_amphibious();
    let sea2 = unit_type2.is_sea() || unit_type2.is_amphibious();

    if sea1 != sea2 {
        return !sea1;
    }

    let air1 = unit_type1.is_air();
    let air2 = unit_type2.is_air();

    if air1 != air2 {
        return !air1;
    }

    let values1 = BASE_UNIT_VALUES[unit_type1 as usize];
    let values2 = BASE_UNIT_VALUES[unit_type2 as usize];
    let (combat1, combat2) = (unit_type1.is_combat(), unit_type2.is_combat());

    if !combat1 || combat2 {
        if combat1 || !combat2 {
            if values1.turns == values2.turns {
                (unit_type1 as u16) > (unit_type2 as u16)
            } else {
                values1.turns > values2.turns
            }
        } else {
            true
        }
    } else {
        false
    }
}

/// Upgrade screen assets from `MAX.RES`.
#[derive(Debug, Clone)]
pub struct UpgradeMenuAssets {
    /// `UPGRADE` background 640×480.
    pub background: Option<BigImage>,
    /// Left arrow `UPGLFT_U/D`.
    pub left: Option<SimpleImage>,
    /// `UPGLFT_X` — unavailable.
    pub left_disabled: Option<SimpleImage>,
    /// Right arrow `UPGRGT_U/D`.
    pub right: Option<SimpleImage>,
    /// `UPGRGT_X`.
    pub right_disabled: Option<SimpleImage>,
    /// Gold icon `I_GOLD`.
    pub gold_icon: Option<SimpleImage>,
    /// `BLDUP__U/D`.
    pub scroll_up: Option<MenuButtonPair>,
    /// `BLDDWN_U/D`.
    pub scroll_down: Option<MenuButtonPair>,
    /// `BLDDES_U/D`.
    pub description: Option<MenuButtonPair>,
    /// `BLDONE_U/D`.
    pub done: Option<MenuButtonPair>,
    /// `BLDHLP_U/D`.
    pub help: Option<MenuButtonPair>,
    /// `BLDCAN_U/D`.
    pub cancel: Option<MenuButtonPair>,
    /// `SELGRD_U/D`.
    pub filter_ground: Option<MenuButtonPair>,
    /// `SELAIR_U/D`.
    pub filter_air: Option<MenuButtonPair>,
    /// `SELSEA_U/D`.
    pub filter_sea: Option<MenuButtonPair>,
    /// `SELBLD_U/D`.
    pub filter_building: Option<MenuButtonPair>,
    /// `SELCBT_U/D`.
    pub filter_combat: Option<MenuButtonPair>,
}

impl UpgradeMenuAssets {
    /// Loads the resources; a missing part is a warning, not an error.
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<SimpleImage> {
            let bytes = archive.read(name).ok()?;
            SimpleImage::decode(bytes).ok()
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };

        Self {
            background: archive
                .read("UPGRADE")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            left: simple("UPGLFT_U"),
            left_disabled: simple("UPGLFT_X"),
            right: simple("UPGRGT_U"),
            right_disabled: simple("UPGRGT_X"),
            gold_icon: simple("I_GOLD"),
            scroll_up: pair("BLDUP__U", "BLDUP__D"),
            scroll_down: pair("BLDDWN_U", "BLDDWN_D"),
            description: pair("BLDDES_U", "BLDDES_D"),
            done: pair("BLDONE_U", "BLDONE_D"),
            help: pair("BLDHLP_U", "BLDHLP_D"),
            cancel: pair("BLDCAN_U", "BLDCAN_D"),
            filter_ground: pair("SELGRD_U", "SELGRD_D"),
            filter_air: pair("SELAIR_U", "SELAIR_D"),
            filter_sea: pair("SELSEA_U", "SELSEA_D"),
            filter_building: pair("SELBLD_U", "SELBLD_D"),
            filter_combat: pair("SELCBT_U", "SELCBT_D"),
        }
    }

    /// Whether the background and the key buttons are present.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.done.is_some() && self.cancel.is_some()
    }
}
