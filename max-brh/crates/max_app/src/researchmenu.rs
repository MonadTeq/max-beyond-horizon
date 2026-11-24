//! Research screen `ResearchMenu` — C++ `researchmenu.cpp` (window `RSRCHPIC`).
//!
//! # Oracle
//!
//! * `RSRCHPIC` window 360×331, centered in the map area
//!   (`Window(RSRCHPIC, GameManager_GetDialogWindowCenterMode())` → `(223,76)`);
//! * title `Research Topics` (`GNW_TEXT_FONT_5`, `COLOR_GREEN`,
//!   `Text_TextBox(95,19,170,13)`, centered);
//! * column labels `Labs`/`Topics`/`Turns` in FONT1 at (23,52)/(184,52)/
//!   (282,52) — centered in their widths 125/90/60;
//! * eight topic rows (step 28 px, the first `uly = 70`), C++ order:
//!   Attack (`I_HRDATK`), Shots, Range, Armor, Hits, Speed, Scan, Cost
//!   (`I_GOLD`); each has:
//!   - arrows `UPGLFT_U/D` (72,uly) and `UPGRGT_U/D` (143,uly) 18×17, and for an
//!     unavailable one — `UPGLFT_X`/`UPGRGT_X`;
//!   - a counter of topic centers (the number on the right in `Image(28,uly+3,32,10)`);
//!   - "turns to completion" (`Image(293,uly+3,40,10)`): C++ computes
//!     `turns = (centers + turns_to_complete − 1) / centers` by the
//!     `ResearchMenu_UpdateResearchProgress` formula (without changing the state);
//!   - the `PRFSLIDE` slider 14×17 in the `Image(91,uly,48,17)` slot: position
//!     `(48−14) * centers / active_centers`; a click on the slot recomputes
//!     `((x−91)·active + 24) / 48` topic centers;
//!   - the topic label (185,uly,49,15) and the icon at the center (170,uly+7);
//!   - `+N%` (`research_level * 10`) on the right at x=278;
//! * `Cancel`/`Help`/`Done` — `DPTBAYUP/DN` (75×25) at (92,294)/(194,294) with
//!   labels and `DPTHP_UP/DN` (31×26) at (167,294).
//!
//! # Allocation
//!
//! `free_capacity` — "free" centers: in C++ it starts at 0 because all
//! enabled centers are already assigned to topics. Decreasing a topic frees
//! centers (`free_capacity += old − new`), increasing spends them. Therefore
//! redistribution goes "first down on one topic, then up on another";
//! if the screen is closed with a non-zero `free_capacity`, the extra centers are turned off —
//! this is exactly how `ResearchMenu_OnClick_Done` works (see
//! [`GameState::set_research_allocation`]).
//!
//! # Keyboard (Rust client)
//!
//! In C++ the screen has only buttons (the keys are just `Esc` to exit and `Enter`
//! through the buttons). Here `↑/↓` (topic selection), `←/→` and `−/+`
//! (change the selected topic), `PageUp/PageDown`, `Enter` = `Done`, `Esc` =
//! `Cancel`, `H`/`?` = help are added. This is a documented difference: without it
//! headless QA and the keyboard player cannot distribute centers.
//!
//! # Wiring
//!
//! In C++ the screen is opened from the research center popup
//! (`UnitsManager_Popup_OnClick_Research`) with `ORDER_POWER_ON`. In the Rust client
//! there is no unit popup menu, so the screen is opened with the `R` key for the
//! selected powered-on `RESEARCH` center of own team (like `U` for
//! upgrades). `Done` writes the allocation into `GameState`, and
//! `ResearchMenu_NewTurn` (port `max_core::research::research_new_turn`) is already
//! called from `GameState::finish_turn` — the screen and the tick use the same
//! `research_topics`.

use max_assets::image::{BigImage, SimpleImage};
use max_core::enums::{UnitOrder, UnitType, RESEARCH_TOPIC_COUNT};
use max_core::universe::StateRef;
use max_core::research::{update_research_progress, RESEARCH_TOPIC_FACTORS};
use max_core::team::TeamId;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered_shadowed, GnwFontId};

use crate::assets::Assets;
use crate::menu::{blit_simple, draw_big_image_at, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};

/// `RSRCHPIC` 360×331.
pub const WINDOW_SIZE: (i32, i32) = (360, 331);
/// Window in the frame: center of `WINDOW_MAIN_MAP` (180,18,448,448).
pub const WINDOW_RECT: PixelRect = PixelRect::new(223, 76, 360, 331);
/// Title `Text_TextBox(95,19,170,13)`.
pub const TITLE_RECT: PixelRect = PixelRect::new(95, 19, 170, 13);
/// `Labs` label (`Text_TextLine(23,52,125)`).
pub const LABS_LABEL: PixelRect = PixelRect::new(23, 52, 125, 10);
/// `Topics` label (`Text_TextLine(184,52,90)`).
pub const TOPICS_LABEL: PixelRect = PixelRect::new(184, 52, 90, 10);
/// `Turns` label (`Text_TextLine(282,52,60)`).
pub const TURNS_LABEL: PixelRect = PixelRect::new(282, 52, 60, 10);
/// First topic row (`uly_slider = topic_index * 28 + 70`).
pub const FIRST_ROW_Y: i32 = 70;
/// Row step.
pub const ROW_HEIGHT: i32 = 28;
/// Left arrow (`Button(UPGLFT_U, UPGLFT_D, 72, uly)`).
pub const LEFT_ARROW: (i32, i32) = (72, 0);
/// Right arrow (`Button(UPGRGT_U, UPGRGT_D, 143, uly)`).
pub const RIGHT_ARROW: (i32, i32) = (143, 0);
/// Arrow size (18×17).
pub const ARROW_SIZE: (i32, i32) = (18, 17);
/// Centers counter field (`Image(28, uly+3, 32, 10)`).
pub const LABS_COUNT_BG: PixelRect = PixelRect::new(28, 3, 32, 10);
/// "turns" field (`Image(293, uly+3, 40, 10)`).
pub const TURNS_BG: PixelRect = PixelRect::new(293, 3, 40, 10);
/// Slider slot (`Image(91, uly, 48, 17)`).
pub const SLIDER_BG: PixelRect = PixelRect::new(91, 0, 48, 17);
/// `PRFSLIDE` size (14×17).
pub const SLIDER_SIZE: (i32, i32) = (14, 17);
/// Topic label (`Text_TextBox(185, uly, 49, 15)`).
pub const TOPIC_LABEL: PixelRect = PixelRect::new(185, 0, 49, 15);
/// Icon center point (`WindowManager_DecodeSimpleImage(image, 170-w/2, uly+7-h/2)`).
pub const TOPIC_ICON: (i32, i32) = (170, 7);
/// Right edge of the `+N%` text (`278 - Text_GetWidth`).
pub const PERCENT_RIGHT: i32 = 278;
/// `Cancel` (`DPTBAYUP/DN` 75×25 at 92,294).
pub const CANCEL_RECT: PixelRect = PixelRect::new(92, 294, 75, 25);
/// `Help` (`DPTHP_UP/DN` 31×26 at 167,294).
pub const HELP_RECT: PixelRect = PixelRect::new(167, 294, 31, 26);
/// `Done` (`DPTBAYUP/DN` 75×25 at 194,294).
pub const DONE_RECT: PixelRect = PixelRect::new(194, 294, 75, 25);
/// Title color `COLOR_GREEN`.
pub const GREEN: u8 = 0x02;
/// Regular text color (`0xA2`).
pub const TEXT: u8 = 0xA2;
/// Button label/heading color — `Fonts_GoldColor (165,175,199)`.
pub const GOLD: (u8, u8, u8) = (165, 175, 199);

/// Topic labels in C++ `ResearchMenu_TopicLabels` order.
pub const TOPIC_LABELS: [&str; RESEARCH_TOPIC_COUNT] = [
    "Attack", "Shots", "Range", "Armor", "Hits", "Speed", "Scan", "Cost",
];

/// Topic icons in C++ `ResearchMenu_TopicIcons` order.
pub const TOPIC_ICONS: [&str; RESEARCH_TOPIC_COUNT] = [
    "I_HRDATK", "I_SHOTS", "I_RANGE", "I_ARMOR", "I_HITS", "I_SPEED", "I_SCAN", "I_GOLD",
];

/// Screen action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchAction {
    /// Nothing.
    None,
    /// `Cancel`/`Esc` — close without changes.
    Cancel,
    /// `Done`/`Enter` — apply the allocation.
    Done,
    /// `Help`/`H` — open `RESEARCH_SETUP`.
    Help,
}

/// Open research screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchMenu {
    /// Team.
    pub team: TeamId,
    /// Number of enabled research centers (`active_research_centers`).
    pub active_centers: u16,
    /// How many centers are not assigned yet (`free_capacity`).
    pub free_capacity: u16,
    /// Centers per topic (`topics[i].topic_value`, starting value).
    pub values: [u16; RESEARCH_TOPIC_COUNT],
    /// Selected topic (Rust client keyboard only).
    pub selected: usize,
    /// Status line (errors/hints).
    pub status: String,
}

impl ResearchMenu {
    /// Opens the screen for a team — C++ `ResearchMenu::ResearchMenu(team)`.
    ///
    /// `None` if the team has no enabled `RESEARCH` center:
    /// in C++ the `Research` popup exists only with `ORDER_POWER_ON`, so a screen with
    /// zero centers is unreachable.
    pub fn new(state: StateRef<'_>, team: TeamId) -> Option<Self> {
        let mut values = [0u16; RESEARCH_TOPIC_COUNT];
        let mut active_centers = 0u16;

        for id in state.unit_ids_in_order() {
            let Some(unit) = state.unit(id) else {
                continue;
            };
            if unit.team != team
                || unit.unit_type != UnitType::ResearchCentre
                || unit.orders != UnitOrder::PowerOn
            {
                continue;
            }
            let index = unit.research_topic as usize;
            if index < RESEARCH_TOPIC_COUNT {
                active_centers = active_centers.saturating_add(1);
                values[index] = values[index].saturating_add(1);
            }
        }

        if active_centers == 0 {
            return None;
        }

        Some(Self {
            team,
            active_centers,
            free_capacity: 0,
            values,
            selected: 0,
            status: String::new(),
        })
    }

    /// `ResearchControl::GetValue`.
    pub fn value(&self, index: usize) -> u16 {
        self.values.get(index).copied().unwrap_or(0)
    }

    /// `ResearchControl::Update(value)` for topic `index`.
    ///
    /// `value` is clamped below by zero and above by `topic_value + free_capacity`;
    /// `free_capacity` changes by `old − new` (C++ `UpdateTopics`).
    pub fn update(&mut self, index: usize, value: i32) -> bool {
        let Some(topic_value) = self.values.get(index).copied() else {
            return false;
        };
        let old = i32::from(topic_value);
        let mut value = value.max(0);

        if value - old > i32::from(self.free_capacity) {
            value = old + i32::from(self.free_capacity);
        }
        if value == old {
            return false;
        }

        let difference = old - value;
        self.values[index] = value as u16;
        self.free_capacity =
            (i32::from(self.free_capacity) + difference).clamp(0, i32::from(u16::MAX)) as u16;
        self.selected = index;

        true
    }

    /// `ResearchMenu_OnClick_Left/Right`: `topic_value ± 1`.
    pub fn adjust(&mut self, index: usize, increase: bool) -> bool {
        let value = i32::from(self.value(index));
        self.update(index, value + if increase { 1 } else { -1 })
    }

    /// Click on the slider of topic `index` in 640×480 **frame** coordinates.
    ///
    /// C++ `ResearchMenu_OnClick_Slider`:
    /// `((mouse_x − slider_ulx) * active + width/2) / width`.
    pub fn slider_value(&self, index: usize, x: i32) -> i32 {
        if self.active_centers == 0 {
            return i32::from(self.value(index));
        }
        let width = i32::from(SLIDER_BG.width);
        let mouse_x = x - (WINDOW_RECT.ulx + i32::from(SLIDER_BG.ulx));

        (mouse_x * i32::from(self.active_centers) + width / 2) / width
    }

    /// Sets the slider of topic `index` by a click on the frame.
    pub fn slider(&mut self, index: usize, x: i32) -> bool {
        let value = self.slider_value(index, x);
        self.update(index, value)
    }

    /// Position of `PRFSLIDE` inside the slot (`offset = (48−14)·value/active`).
    pub fn slider_offset(&self, index: usize) -> i32 {
        let range = i32::from(SLIDER_BG.width) - SLIDER_SIZE.0;

        if self.active_centers == 0 {
            0
        } else {
            (range * i32::from(self.value(index))) / i32::from(self.active_centers)
        }
    }

    /// Topic level in percent (`research_level * 10`).
    pub fn topic_percent(&self, state: StateRef<'_>, index: usize) -> u32 {
        state
            .teams()
            .get(self.team.0 as usize)
            .and_then(|team| team.research_topics.get(index))
            .map(|topic| topic.research_level * 10)
            .unwrap_or(0)
    }

    /// "Turns to completion" for the current number of topic centers.
    ///
    /// Repeats `ResearchControl::RefreshScreen`: temporarily applies
    /// `ResearchMenu_UpdateResearchProgress(team, topic, +1)`, computes
    /// `(centers + turns_to_complete − 1) / centers` and rolls back `−1`. Here
    /// the change is computed on a copy of `ResearchTopic` — the game state is not
    /// touched.
    pub fn turns_to_complete(&self, state: StateRef<'_>, index: usize) -> Option<u32> {
        let centers = i32::from(self.value(index));
        if centers <= 0 || index >= RESEARCH_TOPIC_COUNT {
            return None;
        }

        let mut topic = *state
            .teams()
            .get(self.team.0 as usize)?
            .research_topics
            .get(index)?;
        update_research_progress(&mut topic, RESEARCH_TOPIC_FACTORS[index], 1);

        let turns = (centers + topic.turns_to_complete as i32 - 1) / centers;
        Some(turns.max(0) as u32)
    }

    /// `ResearchMenu_OnClick_Done`: applies the allocation to `GameState`.
    ///
    /// Returns the actual allocation (C++ may also turn centers off
    /// if the target has fewer slots than enabled centers).
    ///
    /// **B2 audit — documented exception (protocol gap).** This is one of the two
    /// UI writes that stayed on the facade writer
    /// (`GameUniverse::set_research_allocation`) instead of the command channel:
    /// C++ sends packet **09** (`researchmenu.cpp:558-560`,
    /// `Remote_SendNetPacket_09`), and the Rust protocol has no packet 09
    /// (`max-brh-docs/architecture/client/protocol.md` §8.4), so there is no
    /// `Command` that carries `research_topics[].allocation`. Closing this path
    /// needs a protocol decision (a team-state command), not a menu refactor.
    pub fn commit<D: max_core::universe::Dispatcher>(
        &mut self,
        state: &mut max_core::universe::GameUniverse<D>,
    ) -> [u32; RESEARCH_TOPIC_COUNT] {
        let target = self.values.map(u32::from);
        let actual = state.set_research_allocation(self.team, target);

        for (index, value) in self.values.iter_mut().enumerate() {
            *value = actual[index].min(u32::from(u16::MAX)) as u16;
        }
        self.free_capacity = 0;
        self.status = format!(
            "Research: {} center(s) assigned",
            actual.iter().sum::<u32>()
        );

        actual
    }

    /// Handles a click in the frame; returns the action.
    pub fn click(&mut self, x: i32, y: i32) -> ResearchAction {
        let window_x = x - WINDOW_RECT.ulx;
        let window_y = y - WINDOW_RECT.uly;

        if CANCEL_RECT.contains(window_x, window_y) {
            return ResearchAction::Cancel;
        }
        if HELP_RECT.contains(window_x, window_y) {
            return ResearchAction::Help;
        }
        if DONE_RECT.contains(window_x, window_y) {
            return ResearchAction::Done;
        }

        for index in 0..RESEARCH_TOPIC_COUNT {
            let uly = FIRST_ROW_Y + index as i32 * ROW_HEIGHT;
            let left = PixelRect::new(LEFT_ARROW.0, uly, ARROW_SIZE.0, ARROW_SIZE.1);
            let right = PixelRect::new(RIGHT_ARROW.0, uly, ARROW_SIZE.0, ARROW_SIZE.1);
            let slider = PixelRect::new(
                SLIDER_BG.ulx,
                uly + SLIDER_BG.uly,
                SLIDER_BG.width,
                SLIDER_BG.height,
            );

            if left.contains(window_x, window_y) {
                if !self.adjust(index, false) {
                    self.status = format!("{}: no centers to release", TOPIC_LABELS[index]);
                }
                return ResearchAction::None;
            }
            if right.contains(window_x, window_y) {
                if !self.adjust(index, true) {
                    self.status = format!("{}: no free research centers", TOPIC_LABELS[index]);
                }
                return ResearchAction::None;
            }
            if slider.contains(window_x, window_y) {
                self.slider(index, x);
                return ResearchAction::None;
            }
        }

        ResearchAction::None
    }

    /// Keyboard (Rust client; see the module header).
    pub fn key(&mut self, key: Key) -> ResearchAction {
        match key {
            Key::Escape => ResearchAction::Cancel,
            Key::Enter => ResearchAction::Done,
            Key::Up => {
                self.selected = self
                    .selected
                    .checked_sub(1)
                    .unwrap_or(RESEARCH_TOPIC_COUNT - 1);
                ResearchAction::None
            }
            Key::Down | Key::Tab => {
                self.selected = (self.selected + 1) % RESEARCH_TOPIC_COUNT;
                ResearchAction::None
            }
            Key::Left | Key::Char('-') | Key::Char('_') => {
                self.adjust(self.selected, false);
                ResearchAction::None
            }
            Key::Right | Key::Char('+') | Key::Char('=') => {
                self.adjust(self.selected, true);
                ResearchAction::None
            }
            Key::PageUp => {
                self.adjust(self.selected, false);
                ResearchAction::None
            }
            Key::PageDown => {
                self.adjust(self.selected, true);
                ResearchAction::None
            }
            Key::Char('h' | 'H' | '?') => ResearchAction::Help,
            Key::Backspace | Key::Char(_) | Key::Alt(_) | Key::Other => ResearchAction::None,
        }
    }

    /// Input event.
    pub fn input(&mut self, event: InputEvent) -> ResearchAction {
        match event {
            InputEvent::MouseClick(x, y) => self.click(x, y),
            InputEvent::Key(key) => self.key(key),
            _ => ResearchAction::None,
        }
    }

    /// Draws the screen over the 640×480 frame (`ResearchControl::Init/RefreshScreen`
    /// + `win_draw`).
    pub fn draw(&self, frame: &mut RgbaBuffer, assets: &Assets, state: StateRef<'_>) -> u32 {
        if frame.width() != 640 || frame.height() != 480 {
            return 0;
        }

        let Some(menu_assets) = assets.research.as_ref() else {
            return 0;
        };
        let background = menu_assets.background.as_ref();
        let palette = background
            .map(UiPalette::from_big_image)
            .or_else(|| UiPalette::from_assets(assets))
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return 0;
        };
        let font = GnwFontId::Font5.font();
        let green = palette.rgba(GREEN);
        let text = palette.rgba(TEXT);
        let (gold_base, gold_outline, gold_shadow) = GOLD;
        let mut painted = 0;

        let window = WINDOW_RECT;
        if let Some(background) = background {
            painted += draw_big_image_at(
                frame,
                background,
                &palette,
                (window.ulx, window.uly),
                window,
            );
        } else {
            frame.fill_rect(window, palette.rgba(0x22));
            painted += 1;
        }

        // `Text_TextBox(buffer, …, "Research Topics", 95, 19, 170, 13,
        // COLOR_GREEN, true)`; vertical_align=true → `19 + ((13+1)−10)/2 = 21`.
        painted += draw_text_gnw_centered_shadowed(
            frame,
            PixelRect::new(
                window.ulx + TITLE_RECT.ulx,
                window.uly + TITLE_RECT.uly + 2,
                TITLE_RECT.width,
                font.height(),
            ),
            font,
            "Research Topics",
            green,
            palette.rgba(0x00),
            palette.rgba(0x00),
        )
        .max(0) as u32;

        // Column labels: `Text_TextLine` with Fonts_GoldColor.
        for (rect, label) in [
            (LABS_LABEL, "Labs"),
            (TOPICS_LABEL, "Topics"),
            (TURNS_LABEL, "Turns"),
        ] {
            painted += draw_text_gnw_centered_shadowed(
                frame,
                PixelRect::new(
                    window.ulx + rect.ulx,
                    window.uly + rect.uly,
                    rect.width,
                    font.height(),
                ),
                font,
                label,
                palette.rgba(gold_base),
                palette.rgba(gold_outline),
                palette.rgba(gold_shadow),
            )
            .max(0) as u32;
        }

        for index in 0..RESEARCH_TOPIC_COUNT {
            let y = window.uly + FIRST_ROW_Y + index as i32 * ROW_HEIGHT;

            // `UPGLFT`/`UPGRGT`: the left one is available when centers > 0, the right one when
            // `free_capacity > 0` (`UpdateButtons`).
            painted += draw_arrow(
                frame,
                &palette,
                menu_assets.left.as_ref(),
                menu_assets.left_disabled.as_ref(),
                PixelRect::new(window.ulx + LEFT_ARROW.0, y, ARROW_SIZE.0, ARROW_SIZE.1),
                self.value(index) > 0,
            );
            painted += draw_arrow(
                frame,
                &palette,
                menu_assets.right.as_ref(),
                menu_assets.right_disabled.as_ref(),
                PixelRect::new(window.ulx + RIGHT_ARROW.0, y, ARROW_SIZE.0, ARROW_SIZE.1),
                self.free_capacity > 0,
            );

            // `ReportStats_DrawNumber(topic_value, 32, …)`.
            let centers = self.value(index).to_string();
            let width = font.text_width(&centers);
            painted += draw_text_gnw(
                frame,
                window.ulx + i32::from(LABS_COUNT_BG.ulx + LABS_COUNT_BG.width) - 1 - width,
                y + LABS_COUNT_BG.uly,
                font,
                &centers,
                text,
            )
            .max(0) as u32;

            // `turns = (topic_value + turns_to_complete − 1) / topic_value`.
            if let Some(turns) = self.turns_to_complete(state, index) {
                let label = turns.to_string();
                let width = font.text_width(&label);
                painted += draw_text_gnw(
                    frame,
                    window.ulx + i32::from(TURNS_BG.ulx + TURNS_BG.width) - 1 - width,
                    y + TURNS_BG.uly,
                    font,
                    &label,
                    text,
                )
                .max(0) as u32;
            }

            // Slider: the slot is already drawn by the background, `PRFSLIDE` on top.
            if let Some(slide) = menu_assets.slider.as_ref() {
                painted += blit_simple(
                    frame,
                    &palette,
                    slide,
                    window.ulx + SLIDER_BG.ulx + self.slider_offset(index),
                    y + SLIDER_BG.uly,
                    true,
                );
            }

            // Topic label: centered 49×15, `vertical_align=true` → y+2.
            painted += draw_text_gnw_centered_shadowed(
                frame,
                PixelRect::new(
                    window.ulx + TOPIC_LABEL.ulx,
                    y + 2,
                    TOPIC_LABEL.width,
                    font.height(),
                ),
                font,
                TOPIC_LABELS[index],
                palette.rgba(gold_base),
                palette.rgba(gold_outline),
                palette.rgba(gold_shadow),
            )
            .max(0) as u32;

            // Icon: `WindowManager_DecodeSimpleImage(image, 170−w/2, uly+7−h/2)`.
            if let Some(icon) = menu_assets.icons[index].as_ref() {
                painted += blit_simple(
                    frame,
                    &palette,
                    icon,
                    window.ulx + TOPIC_ICON.0 - i32::from(icon.width) / 2,
                    y + TOPIC_ICON.1 - i32::from(icon.height) / 2,
                    true,
                );
            }

            // `+N%` at the right edge 278, color 0xA2, no outline.
            let percent = format!("+{}%", self.topic_percent(state, index));
            let width = font.text_width(&percent);
            painted += draw_text_gnw(
                frame,
                window.ulx + PERCENT_RIGHT - width,
                y + 3,
                font,
                &percent,
                text,
            )
            .max(0) as u32;
        }

        // `Cancel`/`Help`/`Done` buttons.
        if let Some(pair) = menu_assets.cancel.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                frame_rect(CANCEL_RECT),
                "Cancel",
                false,
            );
        }
        if let Some(pair) = menu_assets.help.as_ref() {
            painted += blit_simple(
                frame,
                &palette,
                pair.image(false),
                window.ulx + HELP_RECT.ulx,
                window.uly + HELP_RECT.uly,
                true,
            );
        }
        if let Some(pair) = menu_assets.done.as_ref() {
            painted += draw_image_button(
                frame,
                &palette,
                pair.image(false),
                pair.image(true),
                frame_rect(DONE_RECT),
                "Done",
                false,
            );
        }

        painted
    }
}

/// Button rectangle in frame coordinates.
fn frame_rect(rect: PixelRect) -> PixelRect {
    PixelRect::new(
        WINDOW_RECT.ulx + rect.ulx,
        WINDOW_RECT.uly + rect.uly,
        rect.width,
        rect.height,
    )
}

/// Arrow with a disabled variant (`UPGLFT_X`/`UPGRGT_X` 16×16).
fn draw_arrow(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: Option<&MenuButtonPair>,
    disabled: Option<&SimpleImage>,
    rect: PixelRect,
    enabled: bool,
) -> u32 {
    let chosen = if enabled {
        image.map(|pair| pair.image(false))
    } else {
        disabled.or_else(|| image.map(|pair| pair.image(false)))
    };

    match chosen {
        Some(chosen) => blit_simple(frame, palette, chosen, rect.ulx, rect.uly, true),
        None => 0,
    }
}

/// Research screen assets.
#[derive(Debug, Clone)]
pub struct ResearchMenuAssets {
    /// `RSRCHPIC` background 360×331.
    pub background: Option<BigImage>,
    /// `I_HRDATK`/`I_SHOTS`/`I_RANGE`/`I_ARMOR`/`I_HITS`/`I_SPEED`/
    /// `I_SCAN`/`I_GOLD`.
    pub icons: [Option<SimpleImage>; RESEARCH_TOPIC_COUNT],
    /// `UPGLFT_U/D` 18×17.
    pub left: Option<MenuButtonPair>,
    /// `UPGLFT_X` 16×16.
    pub left_disabled: Option<SimpleImage>,
    /// `UPGRGT_U/D` 18×17.
    pub right: Option<MenuButtonPair>,
    /// `UPGRGT_X` 16×16.
    pub right_disabled: Option<SimpleImage>,
    /// `PRFSLIDE` 14×17.
    pub slider: Option<SimpleImage>,
    /// `DPTBAYUP/DN` 75×25 (Cancel/Done).
    pub cancel: Option<MenuButtonPair>,
    /// `DPTBAYUP/DN` 75×25 (Done; the same resource in C++).
    pub done: Option<MenuButtonPair>,
    /// `DPTHP_UP/DN` 31×26.
    pub help: Option<MenuButtonPair>,
}

impl ResearchMenuAssets {
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
                .read("RSRCHPIC")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            icons: TOPIC_ICONS.map(simple),
            left: pair("UPGLFT_U", "UPGLFT_D"),
            left_disabled: simple("UPGLFT_X"),
            right: pair("UPGRGT_U", "UPGRGT_D"),
            right_disabled: simple("UPGRGT_X"),
            slider: simple("PRFSLIDE"),
            cancel: pair("DPTBAYUP", "DPTBAYDN"),
            done: pair("DPTBAYUP", "DPTBAYDN"),
            help: pair("DPTHP_UP", "DPTHP_DN"),
        }
    }

    /// Whether the background and the key buttons are present.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.done.is_some() && self.slider.is_some()
    }
}

#[cfg(test)]
mod tests {
    /// Applies the menu allocation through the facade.
    fn commit(
        menu: &mut ResearchMenu,
        mut universe: max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    ) -> (
        max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
        [u32; RESEARCH_TOPIC_COUNT],
    ) {
        let actual = menu.commit(&mut universe);
        (universe, actual)
    }

    use super::*;
    use max_core::enums::ResearchTopic;
    use max_core::geo::Point;
    use max_core::research::research_turns_to_complete;
    use max_core::team::TeamId;

    fn state_with_center(
        topic: ResearchTopic,
        level: u32,
    ) -> (
        max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
        TeamId,
    ) {
        let mut state = max_core::universe::GameUniverse::default();
        state
            .team_mut(TeamId::RED)
            .expect("RED slot")
            .research_topics[topic as usize]
            .research_level = level;
        let id = state
            .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(20, 20), 0)
            .expect("research center");
        let mut unit = state.snapshot().unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.research_topic = topic;
        state.write_unit(id, unit);
        state.update_research_progress(TeamId::RED, topic, 1);
        (state, TeamId::RED)
    }

    #[test]
    fn layout_matches_the_oracle() {
        assert_eq!(WINDOW_RECT, PixelRect::new(223, 76, 360, 331));
        // `180 + (448-360)/2 = 224`? C++ integer: (447-360)/2 = 43 → 223.
        assert_eq!(WINDOW_RECT.ulx, 180 + (447 - 360) / 2);
        assert_eq!(WINDOW_RECT.uly, 18 + (447 - 331) / 2);
        assert_eq!(FIRST_ROW_Y, 70);
        assert_eq!(ROW_HEIGHT, 28);
        assert_eq!(
            PixelRect::new(
                LEFT_ARROW.0,
                FIRST_ROW_Y + 7 * ROW_HEIGHT,
                ARROW_SIZE.0,
                ARROW_SIZE.1
            ),
            PixelRect::new(72, 266, 18, 17)
        );
        assert_eq!(
            frame_rect(DONE_RECT),
            PixelRect::new(223 + 194, 76 + 294, 75, 25)
        );
    }

    #[test]
    fn topic_order_matches_cpp() {
        let topics = [
            ResearchTopic::Attack,
            ResearchTopic::Shots,
            ResearchTopic::Range,
            ResearchTopic::Armor,
            ResearchTopic::Hits,
            ResearchTopic::Speed,
            ResearchTopic::Scan,
            ResearchTopic::Cost,
        ];

        for (index, topic) in topics.iter().enumerate() {
            assert_eq!(*topic as usize, index);
        }
        assert_eq!(TOPIC_LABELS[0], "Attack");
        assert_eq!(TOPIC_LABELS[7], "Cost");
        assert_eq!(TOPIC_ICONS[0], "I_HRDATK");
        assert_eq!(TOPIC_ICONS[7], "I_GOLD");
    }

    #[test]
    fn opening_counts_powered_centers_per_topic() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");

        assert_eq!(menu.active_centers, 1);
        assert_eq!(menu.values[ResearchTopic::Attack as usize], 1);
        assert_eq!(menu.free_capacity, 0, "в C++ free_capacity стартует с 0");
        assert_eq!(menu.slider_offset(ResearchTopic::Attack as usize), 48 - 14);
    }

    #[test]
    fn can_only_redistribute_released_centers() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");

        // Cannot go up: there are no free centers.
        assert!(!menu.adjust(ResearchTopic::Shots as usize, true));
        assert_eq!(menu.values[ResearchTopic::Shots as usize], 0);

        // Down on Attack frees a center...
        assert!(menu.adjust(ResearchTopic::Attack as usize, false));
        assert_eq!(menu.free_capacity, 1);
        assert_eq!(menu.values[ResearchTopic::Attack as usize], 0);

        // ...and it can be given to Shots.
        assert!(menu.adjust(ResearchTopic::Shots as usize, true));
        assert_eq!(menu.free_capacity, 0);
        assert_eq!(menu.values[ResearchTopic::Shots as usize], 1);
    }

    #[test]
    fn slider_maps_pixels_to_centers_like_cpp() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
        let left = WINDOW_RECT.ulx + SLIDER_BG.ulx;
        let right = left + SLIDER_BG.width;

        assert_eq!(menu.slider_value(0, left), 0);
        assert_eq!(menu.slider_value(0, right), 1);
    }

    #[test]
    fn done_reassigns_the_unit_and_keeps_allocation_consistent() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
        menu.adjust(ResearchTopic::Attack as usize, false);
        menu.adjust(ResearchTopic::Shots as usize, true);
        let (state, actual) = commit(&mut menu, state);

        assert_eq!(actual[ResearchTopic::Attack as usize], 0);
        assert_eq!(actual[ResearchTopic::Shots as usize], 1);

        let allocation = state.snapshot().research_allocation(team);
        assert_eq!(allocation[ResearchTopic::Attack as usize], 0);
        assert_eq!(allocation[ResearchTopic::Shots as usize], 1);

        let unit = state
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.snapshot().unit(id))
            .find(|unit| unit.unit_type == UnitType::ResearchCentre)
            .expect("center");
        assert_eq!(unit.research_topic, ResearchTopic::Shots);
    }

    #[test]
    fn done_with_released_centers_powers_them_off() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
        menu.adjust(ResearchTopic::Attack as usize, false);
        let (mut state, actual) = commit(&mut menu, state);

        assert_eq!(
            actual.iter().sum::<u32>(),
            1,
            "центр ещё числится до Power Off"
        );
        let unit = state
            .snapshot()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| state.snapshot().unit(id))
            .find(|unit| unit.unit_type == UnitType::ResearchCentre)
            .expect("center");
        assert_eq!(unit.orders, UnitOrder::PowerOff);

        // On the next tick `process_order_power_off` writes off the allocation
        // (`ResearchMenu_UpdateResearchProgress(-1)`), as in C++.
        state.tick();
        assert_eq!(state.snapshot().research_allocation(team).iter().sum::<u32>(), 0);
    }

    #[test]
    fn turns_to_complete_shrink_with_more_centers() {
        let (mut state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
        let one = menu
            .turns_to_complete(StateRef::from(&state), ResearchTopic::Attack as usize)
            .unwrap();
        assert!(one > 0);
        assert_eq!(
            one,
            research_turns_to_complete(0, RESEARCH_TOPIC_FACTORS[0]) as u32
        );

        // Second center: the topic speed changes (a deterministic formula).
        let second = state
            .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(21, 20), 0)
            .expect("second center");
        let mut unit = state.snapshot().unit(second).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.research_topic = ResearchTopic::Attack;
        state.write_unit(second, unit);
        state.update_research_progress(TeamId::RED, ResearchTopic::Attack, 1);

        menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
        assert_eq!(menu.value(ResearchTopic::Attack as usize), 2);
        let two = menu
            .turns_to_complete(StateRef::from(&state), ResearchTopic::Attack as usize)
            .unwrap();
        assert!(two < one, "два центра быстрее одного: {two} < {one}");
    }

    #[test]
    fn research_new_turn_uses_the_same_distribution() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
        menu.adjust(ResearchTopic::Attack as usize, false);
        menu.adjust(ResearchTopic::Shots as usize, true);
        let (mut state, _) = commit(&mut menu, state);

        // `finish_turn`/`advance_research` tick exactly the topic to which
        // the centers were redistributed: Attack stays 0, Shots grows.
        for _ in 0..16 {
            state.advance_research(team);
        }
        assert_eq!(
            state.snapshot().teams()[team.0 as usize].research_topics[ResearchTopic::Attack as usize]
                .research_level,
            0
        );
        assert!(
            state.snapshot().teams()[team.0 as usize].research_topics[ResearchTopic::Shots as usize]
                .research_level
                >= 1
        );
    }

    #[test]
    fn action_keys_and_clicks() {
        let (state, team) = state_with_center(ResearchTopic::Attack, 0);
        let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");

        assert_eq!(menu.key(Key::Enter), ResearchAction::Done);
        assert_eq!(menu.key(Key::Escape), ResearchAction::Cancel);
        assert_eq!(menu.key(Key::Char('h')), ResearchAction::Help);

        // Buttons in frame coordinates: Help (167..198) overlaps the left edge of
        // Done (194..269), so we click the button centers.
        assert_eq!(
            menu.click(WINDOW_RECT.ulx + 92 + 37, WINDOW_RECT.uly + 294 + 12),
            ResearchAction::Cancel
        );
        assert_eq!(
            menu.click(WINDOW_RECT.ulx + 167 + 15, WINDOW_RECT.uly + 294 + 13),
            ResearchAction::Help
        );
        assert_eq!(
            menu.click(WINDOW_RECT.ulx + 194 + 37, WINDOW_RECT.uly + 294 + 12),
            ResearchAction::Done
        );

        // The right arrow of Attack does nothing at free_capacity = 0.
        let right = PixelRect::new(
            WINDOW_RECT.ulx + RIGHT_ARROW.0,
            WINDOW_RECT.uly + FIRST_ROW_Y,
            ARROW_SIZE.0,
            ARROW_SIZE.1,
        );
        assert_eq!(
            menu.click(right.ulx + 1, right.uly + 1),
            ResearchAction::None
        );
        assert_eq!(menu.values[0], 1);

        // The left arrow frees a center.
        let left = PixelRect::new(
            WINDOW_RECT.ulx + LEFT_ARROW.0,
            WINDOW_RECT.uly + FIRST_ROW_Y,
            ARROW_SIZE.0,
            ARROW_SIZE.1,
        );
        assert_eq!(menu.click(left.ulx + 1, left.uly + 1), ResearchAction::None);
        assert_eq!(menu.values[0], 0);
        assert_eq!(menu.free_capacity, 1);
    }

    #[test]
    fn determinism_of_two_identical_menus() {
        let (a, team) = state_with_center(ResearchTopic::Attack, 0);
        let (b, _) = state_with_center(ResearchTopic::Attack, 0);

        let mut menu_a = ResearchMenu::new(StateRef::from(&a), team).unwrap();
        let mut menu_b = ResearchMenu::new(StateRef::from(&b), team).unwrap();
        for menu in [&mut menu_a, &mut menu_b] {
            menu.adjust(ResearchTopic::Attack as usize, false);
            menu.adjust(ResearchTopic::Hits as usize, true);
            menu.adjust(ResearchTopic::Hits as usize, false);
            menu.adjust(ResearchTopic::Scan as usize, true);
        }
        assert_eq!(menu_a.values, menu_b.values);
        assert_eq!(menu_a.free_capacity, menu_b.free_capacity);
        let (a, actual_a) = commit(&mut menu_a, a);
        let (b, actual_b) = commit(&mut menu_b, b);
        assert_eq!(actual_a, actual_b);
        assert_eq!(a.snapshot().research_allocation(team), b.snapshot().research_allocation(team));
    }
}
