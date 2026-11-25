//! `GameSetupMenu` — the mission list of one category (`gamesetupmenu.cpp`).
//!
//! Oracle: [`GameSetupMenu_Menu`] (`menu.cpp:1957-2113`) and
//! `GameSetupMenu::Init/DrawMissionList/EventSelectItem/EventStepButton/
//! EventScrollButton/EventBriefingButton/DrawDescriptionPanel`.
//!
//! The screen is **data + selection**: it shows the descriptors of
//! [`MissionRegistry::missions`] for one [`MissionCategory`] and turns the choice
//! into a start DTO ([`crate::ui::Transition::StartMission`]). It never touches a
//! match: the start itself is `App::start_mission_from_registry`
//! (`mission.cpp:425-455` → `menu.cpp:2052-2098`, campaign stage 1).
//!
//! Layout (`gamesetupmenu.hpp:59-71`):
//! * 12 mission rows (18, 184 + 20·i) 294×19 with a `COLOR_GREEN` separator under
//!   each of the first eleven rows (`DrawMissionList`);
//! * description panel: title (400,174) 180×21 over `SUBTITLE`, text (354,215)
//!   266×201 inside `BIGSCRNL` (342,202) + `BIGSCRNR` (482,202), 20 rows of
//!   `GNW_TEXT_FONT_5`;
//! * page arrows `MNUUAROU`/`MNUDAROU` at (16,438)/(48,438);
//! * briefing arrows `MNUUAROU`/`MNUDAROU` at (563,438)/(596,438);
//! * `Cancel` `MNUBTN4U` (200,438), `?` `MNUBTN5U` (312,438), `Start` `MNUBTN6U`
//!   (361,438).
//!
//! The mission index is 0-based inside the category (the C++ `game_file_number - 1`);
//! the displayed number is 1-based.
//!
//! Missing mission files (owner condition 5): the descriptor stays in the list
//! (`MissionRegistry` keeps it with a warning instead of dropping it like the C++
//! `Mission_ProcessMission` does), the row is drawn in red with a `[missing]`
//! marker, the description panel names the absent file, and `Start` refuses with a
//! visible status line instead of starting an empty world.
//!
//! Campaign progression (stage 3a): [`MissionSelectForm::campaign_unlocked`] is the
//! persistent `last_campaign` of the settings (`DrawMissionList`,
//! `gamesetupmenu.cpp:281`). A campaign row with a 0-based index `>= last_campaign`
//! stays in the list but is disabled and carries no title — exactly the oracle's
//! locked rows; [`MissionSelectForm::open_campaign`] applies the
//! `GameSetupMenu_Menu` clamp and selection (`menu.cpp:1968-1983`).

use max_assets::{MissionDescriptor, MissionRegistry};
use max_core::victory::MissionCategory;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_shadowed, GnwFontId};

use crate::campaign::CampaignMenuOpen;
use crate::helpmenu::HelpSection;
use crate::menu::{
    blit_simple, clean_status, draw_big_image, draw_image_button, draw_title, truncate_to_width,
    wrap_gnw_text, MenuAssets, GREEN, TEXT,
};

/// Missions per page (`GAME_SETUP_MENU_MISSION_COUNT`, `gamesetupmenu.cpp:35`).
pub const MISSIONS_PER_PAGE: usize = 12;
/// [`MissionSelectForm::campaign_unlocked`] of a category without progression:
/// every mission is selectable (the campaign uses `last_campaign` instead).
pub const ALL_MISSIONS_UNLOCKED: usize = usize::MAX;
/// Mission row height (`control->bounds.lry - ulx`, `gamesetupmenu.hpp:60-71`).
pub const ROW_HEIGHT: i32 = 20;
/// Description rows per page (`201 / Text_GetHeight()`, `LoadMissionDescription`).
pub const DESCRIPTION_ROWS_PER_PAGE: usize = 20;

/// Mission row rectangle (0 — the first row of the page).
pub fn row_rect(row: usize) -> PixelRect {
    PixelRect::new(18, 184 + row as i32 * ROW_HEIGHT, 294, 19)
}

/// Mission list row under the point.
pub fn row_at(x: i32, y: i32) -> Option<usize> {
    for row in 0..MISSIONS_PER_PAGE {
        if row_rect(row).contains(x, y) {
            return Some(row);
        }
    }

    None
}

/// Description title (`game_setup_menu_titles[0]`, (400,174) … (580,195)).
pub const DESCRIPTION_TITLE: PixelRect = PixelRect::new(400, 174, 180, 21);
/// `SUBTITLE` picture origin (`GameSetupMenu_LoadSubtitleControl`).
pub const SUBTITLE_ORIGIN: (i32, i32) = (400, 174);
/// Description text area (`game_setup_menu_titles[1]`, (354,215) … (620,416)).
pub const DESCRIPTION_TEXT: PixelRect = PixelRect::new(354, 215, 266, 201);
/// Left info panel `BIGSCRNL` origin (`GameSetupMenu_DrawBigInfoPanel`).
pub const INFO_PANEL_LEFT: (i32, i32) = (342, 202);
/// Right info panel `BIGSCRNR` origin.
pub const INFO_PANEL_RIGHT: (i32, i32) = (482, 202);

/// Page scroll up (`MNUUAROU`, (16,438)).
pub const SCROLL_UP: PixelRect = PixelRect::new(16, 438, 28, 29);
/// Page scroll down (`MNUDAROU`, (48,438)).
pub const SCROLL_DOWN: PixelRect = PixelRect::new(48, 438, 28, 29);
/// Briefing scroll up (`MNUUAROU`, (563,438)).
pub const BRIEF_UP: PixelRect = PixelRect::new(563, 438, 28, 29);
/// Briefing scroll down (`MNUDAROU`, (596,438)).
pub const BRIEF_DOWN: PixelRect = PixelRect::new(596, 438, 28, 29);
/// `Cancel` (`MNUBTN4U`, (200,438)).
pub const CANCEL: PixelRect = PixelRect::new(200, 438, 109, 40);
/// Help `?` (`MNUBTN5U`, (48×40 at (312,438))).
pub const HELP: PixelRect = PixelRect::new(312, 438, 48, 40);
/// `Start` (`MNUBTN6U`, (361,438)).
pub const START: PixelRect = PixelRect::new(361, 438, 109, 40);
/// Status/fallback line under the mission list (the original has no such line).
pub const STATUS: PixelRect = PixelRect::new(19, 426, 296, 12);
/// `COLOR_YELLOW` of the selected row (`DrawSaveFileTitle(…, COLOR_YELLOW)`).
pub const YELLOW: u8 = 0x04;
/// `COLOR_RED` of an unavailable mission row.
pub const RED: u8 = 0x01;

/// What a click/key on the mission screen asks the screen machine to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionSelectAction {
    /// Nothing.
    None,
    /// Click on row `0..MISSIONS_PER_PAGE` of the page.
    Select(usize),
    /// `Start`/`Enter`.
    Start,
    /// `Cancel`/Esc — back to the `New Game` menu.
    Cancel,
    /// `?` — the category help window (`EventHelp`).
    Help,
    /// Previous page (`EventScrollButton`, key 12).
    PageUp,
    /// Next page (`EventScrollButton`, key 13).
    PageDown,
    /// Briefing text up (`EventBriefingButton`, key 14).
    BriefUp,
    /// Briefing text down (key 15).
    BriefDown,
}

/// Screen/button under the point (rows first, then the bottom row).
pub fn mission_select_at(x: i32, y: i32) -> MissionSelectAction {
    if let Some(row) = row_at(x, y) {
        return MissionSelectAction::Select(row);
    }

    if SCROLL_UP.contains(x, y) {
        MissionSelectAction::PageUp
    } else if SCROLL_DOWN.contains(x, y) {
        MissionSelectAction::PageDown
    } else if BRIEF_UP.contains(x, y) {
        MissionSelectAction::BriefUp
    } else if BRIEF_DOWN.contains(x, y) {
        MissionSelectAction::BriefDown
    } else if CANCEL.contains(x, y) {
        MissionSelectAction::Cancel
    } else if HELP.contains(x, y) {
        MissionSelectAction::Help
    } else if START.contains(x, y) {
        MissionSelectAction::Start
    } else {
        MissionSelectAction::None
    }
}

/// `GameSetupMenu` state: the category, the selected mission and the pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionSelectForm {
    /// Category of the list (`GameSetupMenu::m_mission_category`).
    pub category: MissionCategory,
    /// Selected mission, 0-based inside the category (`game_file_number - 1`).
    pub selected: usize,
    /// First mission index of the page, a multiple of [`MISSIONS_PER_PAGE`]
    /// (`game_index_first_on_page - 1`).
    pub page: usize,
    /// First visible briefing row (`string_row_index`).
    pub row_index: usize,
    /// Single-player entry: the `Custom Scenarios` title of the oracle
    /// (`_(884f)` instead of `_(f27a)`).
    pub single_player: bool,
    /// Unlocked missions of the category.
    ///
    /// The campaign value is the persistent `last_campaign` (`settings.cpp:105`), a
    /// 1-based count: `DrawMissionList` (`gamesetupmenu.cpp:281`) disables a row
    /// while its 0-based index is `>= last_campaign`. Every other category unlocks
    /// all missions ([`ALL_MISSIONS_UNLOCKED`]). Defaults to the `settings.cpp`
    /// value of `last_campaign` = 1.
    pub campaign_unlocked: usize,
}

impl MissionSelectForm {
    /// Opens the category on the first mission (`GameSetupMenu_Menu` sets
    /// `game_file_number` to 1 for every category except `Campaign`, where the
    /// progression value is used — [`Self::open_campaign`]).
    pub fn new(category: MissionCategory) -> Self {
        Self {
            category,
            selected: 0,
            page: 0,
            row_index: 0,
            single_player: true,
            campaign_unlocked: if category == MissionCategory::Campaign {
                // `settings.cpp:105`: `last_campaign` defaults to 1.
                1
            } else {
                ALL_MISSIONS_UNLOCKED
            },
        }
    }

    /// `GameSetupMenu_Menu` (`menu.cpp:1968-1983`) for the campaign list: the
    /// persistent `last_campaign` is clamped to the mission count, the list opens on
    /// `game_file_number = last_campaign`, and the returned value is the clamp the
    /// oracle writes back to the settings file (`menu.cpp:1972-1977`).
    ///
    /// `None` — the registry is unavailable: the visible fallback of the stage-2
    /// menu (owner condition 5). Nothing is unlocked and the stored progression is
    /// left untouched: clamping it to an unknown zero would destroy it.
    pub fn open_campaign(
        &mut self,
        last_campaign: i32,
        registry: Option<&MissionRegistry>,
    ) -> Option<CampaignMenuOpen> {
        self.row_index = 0;

        let Some(registry) = registry else {
            self.campaign_unlocked = 0;
            self.selected = 0;
            self.page = 0;

            return None;
        };

        let open = crate::campaign::menu_open(last_campaign, self.mission_count(Some(registry)));

        self.campaign_unlocked = open.last_campaign.max(0) as usize;
        self.selected = open.selected;
        self.page = open.selected / MISSIONS_PER_PAGE * MISSIONS_PER_PAGE;

        Some(open)
    }

    /// Unlocked missions of the category — the C++ `game_count` of
    /// `DrawMissionList`: every mission outside the campaign, `last_campaign`
    /// clamped to the list for the campaign.
    pub fn unlocked_count(&self, registry: Option<&MissionRegistry>) -> usize {
        let count = self.mission_count(registry);

        if self.category == MissionCategory::Campaign {
            self.campaign_unlocked.min(count)
        } else {
            count
        }
    }

    /// Whether `index` is a locked campaign mission (`DrawMissionList`: the row is
    /// still drawn by the oracle, but it is disabled and carries no title).
    pub fn is_locked(&self, registry: Option<&MissionRegistry>, index: usize) -> bool {
        index >= self.unlocked_count(registry)
    }

    /// Screen title (`GameSetupMenu::Init`): `_(9b46)`, `_(5d49)`, `_(385b)`,
    /// `_(884f)`/`_(f27a)`.
    pub fn title(&self) -> &'static str {
        match self.category {
            MissionCategory::Training => "Training Missions",
            MissionCategory::Scenario => "Stand Alone Missions",
            MissionCategory::Campaign => "Campaign Game",
            MissionCategory::MultiPlayerScenario => {
                if self.single_player {
                    "Custom Scenarios"
                } else {
                    "Multiplayer Scenarios"
                }
            }
            _ => "Multiplayer Scenarios",
        }
    }

    /// Help section of the `?` button (`GameSetupMenu::EventHelp`).
    pub fn help_section(&self) -> HelpSection {
        match self.category {
            MissionCategory::Training => HelpSection::TrainingMenu,
            MissionCategory::Scenario => HelpSection::StandAloneMenu,
            MissionCategory::Campaign => HelpSection::CampaignMenu,
            _ => HelpSection::MultiScenarioMenu,
        }
    }

    /// How many missions the category holds (`GetMissions(category).size()`).
    pub fn mission_count(&self, registry: Option<&MissionRegistry>) -> usize {
        registry.map_or(0, |registry| registry.missions(self.category).len())
    }

    /// Descriptor at `index` of the category.
    pub fn descriptor<'a>(
        &self,
        registry: Option<&'a MissionRegistry>,
        index: usize,
    ) -> Option<&'a MissionDescriptor> {
        registry?.missions(self.category).get(index)
    }

    /// Descriptor of the current selection.
    pub fn selected_descriptor<'a>(
        &self,
        registry: Option<&'a MissionRegistry>,
    ) -> Option<&'a MissionDescriptor> {
        self.descriptor(registry, self.selected)
    }

    /// Descriptor of a row of the current page.
    pub fn row_descriptor<'a>(
        &self,
        registry: Option<&'a MissionRegistry>,
        row: usize,
    ) -> Option<&'a MissionDescriptor> {
        self.descriptor(registry, self.page + row)
    }

    /// Whether the mission can start: it is unlocked and the descriptor exists and
    /// its file is on disk (`Mission::GetMission`, `mission.cpp:272-291`).
    ///
    /// A locked campaign mission never starts (`DrawMissionList` disables the row;
    /// the selection can only hold an unlocked mission).
    pub fn can_start(&self, registry: Option<&MissionRegistry>) -> bool {
        !self.is_locked(registry, self.selected)
            && self
                .selected_descriptor(registry)
                .is_some_and(|descriptor| {
                    descriptor.mission_found && descriptor.mission_path().is_some()
                })
    }

    /// Why the selection cannot start (visible fallback, owner condition 5).
    pub fn start_error(&self, registry: Option<&MissionRegistry>) -> String {
        if registry.is_none() {
            return "Mission registry unavailable (PATCHES.RES)".to_string();
        }

        if self.category == MissionCategory::Campaign && self.is_locked(registry, self.selected) {
            return format!(
                "Mission {} is locked: win the previous campaign mission first",
                self.selected + 1
            );
        }

        match self.selected_descriptor(registry) {
            None => "No mission selected".to_string(),
            Some(descriptor) if !descriptor.mission_found => format!(
                "{} is unavailable: mission file {} is missing",
                descriptor.tag,
                descriptor
                    .mission_name
                    .as_deref()
                    .unwrap_or("(not declared)")
            ),
            Some(descriptor) => format!(
                "{}: mission file is not readable",
                descriptor
                    .mission_name
                    .as_deref()
                    .unwrap_or(descriptor.tag.as_str())
            ),
        }
    }

    /// `EventSelectItem`: a click on row `row` of the page.
    ///
    /// A locked campaign row is a disabled button in the oracle: the click is
    /// ignored and the selection stays where it was. Returns `true` when the row is
    /// the already selected mission — the second click starts it
    /// (`event_click_done`).
    pub fn select_row(&mut self, registry: Option<&MissionRegistry>, row: usize) -> bool {
        let index = self.page + row;

        if self.is_locked(registry, index) {
            return false;
        }

        if self.selected == index {
            return true;
        }

        self.selected = index;
        self.row_index = 0;

        false
    }

    /// `EventStepButton`: up/down moves the selection and keeps the page in sync.
    ///
    /// The oracle stops on the last **unlocked** mission: `game_slot` is clamped to
    /// `game_count + game_index_first_on_page - 1` (`gamesetupmenu.cpp:455-457`).
    pub fn step(&mut self, registry: Option<&MissionRegistry>, delta: isize) {
        let count = self.unlocked_count(registry);

        if count == 0 {
            return;
        }

        let next = (self.selected as isize + delta).clamp(0, count as isize - 1) as usize;

        if next == self.selected {
            return;
        }

        self.selected = next;
        self.page = next / MISSIONS_PER_PAGE * MISSIONS_PER_PAGE;
        self.row_index = 0;
    }

    /// Last page index that holds missions.
    fn last_page(&self, registry: Option<&MissionRegistry>) -> usize {
        let count = self.unlocked_count(registry);

        if count == 0 {
            0
        } else {
            (count - 1) / MISSIONS_PER_PAGE * MISSIONS_PER_PAGE
        }
    }

    /// `EventScrollButton`: `direction` — `-1` up, `1` down.
    ///
    /// The C++ paging only browses the list: `game_file_number` (the selection) is
    /// not changed, and the description panel keeps showing the selected mission.
    /// The port keeps that behavior, so `Start` always starts the mission whose
    /// description is on screen.
    pub fn scroll_page(&mut self, registry: Option<&MissionRegistry>, direction: isize) {
        let last = self.last_page(registry);

        self.page = if direction < 0 {
            self.page.saturating_sub(MISSIONS_PER_PAGE)
        } else {
            (self.page + MISSIONS_PER_PAGE).min(last)
        };
    }

    /// Whether the page arrows can move (`FindNextValidFile`).
    pub fn can_scroll_page(&self, registry: Option<&MissionRegistry>, direction: isize) -> bool {
        if direction < 0 {
            self.page > 0
        } else {
            self.page < self.last_page(registry)
        }
    }

    /// Wrapped description of the current selection (`Text_SplitText`).
    pub fn description_lines(&self, registry: Option<&MissionRegistry>) -> Vec<String> {
        if registry.is_none() {
            return vec!["Mission registry unavailable (PATCHES.RES)".to_string()];
        }

        let Some(descriptor) = self.selected_descriptor(registry) else {
            return vec!["No mission selected".to_string()];
        };

        let mut lines = wrap_gnw_text(descriptor.description_text(), DESCRIPTION_TEXT.width);

        if !descriptor.mission_found {
            lines.push(String::new());
            lines.push(format!(
                "Mission file {} is missing — unavailable.",
                descriptor
                    .mission_name
                    .as_deref()
                    .unwrap_or("(not declared)")
            ));
        }

        lines
    }

    /// `EventBriefingButton`: scroll the briefing text one page.
    pub fn scroll_briefing(&mut self, registry: Option<&MissionRegistry>, direction: isize) {
        let lines = self.description_lines(registry).len();
        let last = lines.saturating_sub(1);

        self.row_index = if direction < 0 {
            self.row_index.saturating_sub(DESCRIPTION_ROWS_PER_PAGE)
        } else {
            (self.row_index + DESCRIPTION_ROWS_PER_PAGE).min(last)
        };
    }

    /// Last mission index of the current page (exclusive).
    pub fn page_end(&self, registry: Option<&MissionRegistry>) -> usize {
        let count = self.mission_count(registry);

        (self.page + MISSIONS_PER_PAGE).min(count)
    }
}

/// Mission list row text (`DrawSaveFileTitle`: `"%i. %s"`), 1-based number.
pub fn row_title(index: usize, descriptor: &MissionDescriptor) -> String {
    let mut title = format!("{}. {}", index + 1, descriptor.title_text());

    if !descriptor.mission_found {
        title.push_str(" [missing]");
    }

    title
}

/// Draws the `GameSetupMenu` screen.
pub fn draw_mission_select(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    registry: Option<&MissionRegistry>,
    form: &MissionSelectForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, &menu.mainpic);

    draw_title(frame, palette, crate::newgame::NEW_GAME_TITLE, form.title());
    crate::menu::draw_menu_portrait_frame(frame, menu);

    let font = GnwFontId::Font5.font();

    for row in 0..MISSIONS_PER_PAGE {
        let rect = row_rect(row);
        let index = form.page + row;
        let descriptor = form.row_descriptor(registry, row);

        if row + 1 < MISSIONS_PER_PAGE {
            // `draw_line(..., COLOR_GREEN)` under the row.
            let line_y = rect.uly + rect.height - 1;
            for x in rect.ulx..rect.ulx + rect.width {
                frame.set(x, line_y, palette.rgba(GREEN));
            }
        }

        let Some(descriptor) = descriptor else {
            continue;
        };

        // `DrawMissionList` (`gamesetupmenu.cpp:281-296`): a campaign mission above
        // `last_campaign` is a disabled button and `mission_titles[i]` stays empty —
        // the row is visible, but it carries neither title nor selection color.
        if form.is_locked(registry, index) {
            continue;
        }

        let selected = index == form.selected;
        let color = if !descriptor.mission_found {
            RED
        } else if selected {
            YELLOW
        } else {
            GREEN
        };

        let text = truncate_to_width(&row_title(index, descriptor), rect.width - 4, font);
        let y = rect.uly + (rect.height - font.height()).max(0) / 2;

        painted +=
            draw_text_gnw(frame, rect.ulx + 2, y, font, &text, palette.rgba(color)).max(0) as u32;
    }

    // Description panel: `GameSetupMenu_LoadSubtitleControl` +
    // `GameSetupMenu_DrawBigInfoPanel`.
    if let Some(subtitle) = &menu.subtitle {
        painted += blit_simple(
            frame,
            palette,
            subtitle,
            SUBTITLE_ORIGIN.0,
            SUBTITLE_ORIGIN.1,
            false,
        );
    }
    if let Some(left) = &menu.info_panel_left {
        painted += blit_simple(
            frame,
            palette,
            left,
            INFO_PANEL_LEFT.0,
            INFO_PANEL_LEFT.1,
            false,
        );
    }
    if let Some(right) = &menu.info_panel_right {
        painted += blit_simple(
            frame,
            palette,
            right,
            INFO_PANEL_RIGHT.0,
            INFO_PANEL_RIGHT.1,
            false,
        );
    }

    if let Some(descriptor) = form.selected_descriptor(registry) {
        draw_title(frame, palette, DESCRIPTION_TITLE, descriptor.title_text());
    } else {
        let placeholder = if registry.is_some() {
            "No mission"
        } else {
            "No registry"
        };
        draw_title(frame, palette, DESCRIPTION_TITLE, placeholder);
    }

    let lines = form.description_lines(registry);
    let end = (form.row_index + DESCRIPTION_ROWS_PER_PAGE).min(lines.len());

    if form.row_index < end {
        for (offset, line) in lines[form.row_index..end].iter().enumerate() {
            if line.is_empty() {
                continue;
            }

            let text = truncate_to_width(line, DESCRIPTION_TEXT.width, font);
            let y = DESCRIPTION_TEXT.uly + offset as i32 * font.height();

            painted += draw_text_gnw(
                frame,
                DESCRIPTION_TEXT.ulx,
                y,
                font,
                &text,
                palette.rgba(TEXT),
            )
            .max(0) as u32;
        }
    }

    // Bottom row: page arrows, briefing arrows, Cancel, ?, Start.
    painted += draw_arrow_button(frame, menu, SCROLL_UP, true);
    painted += draw_arrow_button(frame, menu, SCROLL_DOWN, false);
    painted += draw_arrow_button(frame, menu, BRIEF_UP, true);
    painted += draw_arrow_button(frame, menu, BRIEF_DOWN, false);

    if let Some(pair) = menu.mnubtn_pair(4) {
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            CANCEL,
            "Cancel",
            false,
        );
    }
    if let Some(pair) = menu.mnubtn_pair(5) {
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            HELP,
            "?",
            false,
        );
    }
    if let Some(pair) = menu.mnubtn_pair(6) {
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            START,
            "Start",
            false,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw_shadowed(
            frame,
            STATUS.ulx,
            STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(GREEN),
            palette.rgba(0xA7),
            palette.rgba(0xE6),
        );
    }

    // Unused parameters: the mouse is not used for row highlighting (the C++ list
    // has no hover state), kept in the signature for the screen renderer.
    let _ = mouse;

    painted
}

/// Page/briefing arrow at `rect` (up — `MNUUAROU`, down — `MNUDAROU`).
fn draw_arrow_button(frame: &mut RgbaBuffer, menu: &MenuAssets, rect: PixelRect, up: bool) -> u32 {
    let pair = if up {
        menu.arrow_up.as_ref()
    } else {
        menu.arrow_down.as_ref()
    };

    match pair {
        Some(pair) => blit_simple(
            frame,
            &menu.palette,
            pair.image(false),
            rect.ulx,
            rect.uly,
            true,
        ),
        None => {
            // Missing resource: a visible frame instead of a silent hole.
            for x in rect.ulx..rect.ulx + rect.width {
                frame.set(x, rect.uly, Rgba::rgb(60, 60, 60));
                frame.set(x, rect.uly + rect.height - 1, Rgba::rgb(60, 60, 60));
            }
            for y in rect.uly..rect.uly + rect.height {
                frame.set(rect.ulx, y, Rgba::rgb(60, 60, 60));
                frame.set(rect.ulx + rect.width - 1, y, Rgba::rgb(60, 60, 60));
            }

            0
        }
    }
}
