//! M-HS: hot-seat (2–4 humans on one PC) — the turn-transition screen and texts.
//!
//! # What the original does (`max-cpp-port/src/game_manager.cpp`)
//!
//! Hot-seat in C++ is not a separate simulation mode but a consequence of there being
//! more than one «human» team:
//!
//! 1. `GameManager_UpdateHumanPlayerCount` (`:3267`) counts teams with
//!    `TEAM_TYPE_PLAYER`; if there are `> 1`, the counter stays non-zero and
//!    `GameManager_PlayMode` **forcibly** becomes `PLAY_MODE_TURN_BASED`
//!    (in a single game the counter is zeroed);
//! 2. `GameManager_TeamTurnTurnBased` (`:451`) walks the teams RED → GREEN →
//!    BLUE → GRAY and for each playing team calls
//!    `GameManager_UpdateGui(team, ...)` (`:2407`);
//! 3. in the `TEAM_TYPE_PLAYER` branch (`:2437`), when the team changes
//!    (`GameManager_PlayerTeam != team`) and `GameManager_HumanPlayerCount` is non-zero,
//!    it calls `GameManager_UpdateGuiControl(team)` (`:2262`). It
//!    **shows a modal message**:
//!
//!    ```c
//!    sprintf(message, _(c2eb), menu_team_names[team]);
//!    MessageManager_DrawMessage(message, MESSAGE_BOX_INFO, MESSAGE_BOX_MODAL, true);
//!    ```
//!
//!    `_(c2eb)` = `"%s:\nBegin turn."`, `menu_team_names[]` (`_(f394)`, `_(a8a6)`,
//!    `_(a3ee)`, `_(319d)`) = `Red Team`, `Green Team`, `Blue Team`, `Gray Team`.
//!    The modal (`MESSAGE_BOX_MODAL`) path of `MessageManager_DrawMessage`
//!    (`message_manager.cpp:144`) is `DialogMenu dialog(text, true); dialog.Run();`;
//! 4. `DialogMenu` (`dialogmenu.cpp:145`) draws the `HELPFRAM` window (301×231),
//!    centered in `WINDOW_MAIN_MAP` (`GameManager_GetDialogWindowCenterMode`,
//!    `dialog_center_mode = 0` by default → `(253,126)`), text `GNW_TEXT_FONT_5`
//!    in the color `GNW_TEXT_OUTLINE | 0xFF` in the area `(20,14)` 265×175 (centered
//!    horizontally, the block — vertically centered), the `HELPOK_U/D` 65×24 button
//!    at `(120,193)` labeled `OK`; exit — button click, **Enter** or **Esc**
//!    (`ProcessKey`, `:103`). While the dialog is open, the game loop sits in
//!    `DialogMenu::Run()`, and the turn timer is not started yet
//!    (`GameManager_UpdateGui` starts it after returning from `UpdateGuiControl`,
//!    `:2490`);
//! 5. after confirmation `GameManager_PlayerTeam = team`, `Access_UpdateMinimapFogOfWar(team, …)`
//!    and `GameManager_UpdatePanelButtons(team)` — input and panels switch to the
//!    new player.
//!
//! In other words: **the original does have a turn-transition screen**, but it is not
//! a separate screen but a `DialogMenu` modal window with the text «<Team>: Begin turn.».
//! Here it is reproduced 1:1 (resources, coordinates, font, color, keys);
//! the only difference from the oracle is the absence of scrolling (`BLDUP/BLDDWN`) for
//! texts longer than 17 lines: the message is two lines.
//!
//! # Setup
//!
//! `menu_custom_game_menu(false)` (`menu.cpp:2115`) — the hot-seat variant of the
//! setup menu: the same three screens (`GameConfigMenu` → `PlanetSelectMenu` →
//! `ChoosePlayerMenu`), but `ChoosePlayerMenu` gets `is_single_player =
//! false`, therefore:
//!
//! * the screen title — `_(55c8)` = `Hot Seat Menu` (instead of `_(5512)` =
//!   `Custom Game Menu`);
//! * «Human» on another slot does **not** copy the human flag
//!   (`EventSelectHuman`, `:79`), so several humans can be selected;
//! * initial values of `menu_choose_player_menu_loop` (`menu.cpp:1883`):
//!   RED — human, GREEN — human, BLUE/GRAY — empty;
//! * `EventDone` (`:212`) requires at least **two humans**: `_(06ec)` =
//!   `There must be at least two human players to begin a hot seat game.`;
//! * team names in the match — `Red Team`/`Green Team`/… (`menu.cpp:1941`:
//!   `player_name` is substituted only in a single game).
//!
//! The entry point in the original is `Multiplayer Menu` (`menu_multiplayer_menu_loop`,
//! `:2361`) → the `Hot Seat Game` button (`_(f731)`).

use max_core::team::{TeamId, TeamKind};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered_shadowed, draw_text_gnw_shadowed, GnwFontId};

use crate::local::LOCAL_TEAM_COUNT;
use crate::menu::{blit_simple, draw_big_image_at, wrap_gnw_text, MenuAssets};
use crate::panels::UiPalette;

/// Team screen title in a single game (`_(5512)`).
pub const TITLE_SINGLE: &str = "Custom Game Menu";
/// Team screen title in hot-seat (`_(55c8)`).
pub const TITLE_HOT_SEAT: &str = "Hot Seat Menu";

/// `_(bc94)`: at least two participants are required (`ChoosePlayerMenu::EventDone`).
pub const MSG_TWO_PLAYERS: &str = "There must be at least two players to begin a new game.";
/// `_(06ec)`: at least two humans are required (hot-seat).
pub const MSG_TWO_HUMANS: &str =
    "There must be at least two human players to begin a hot seat game.";

/// Team names in hot-seat — `menu_team_names[]` (`_(f394)`, `_(a8a6)`, `_(a3ee)`,
/// `_(319d)`).
pub const TEAM_NAMES: [&str; LOCAL_TEAM_COUNT] =
    ["Red Team", "Green Team", "Blue Team", "Gray Team"];

/// Turn-transition message (`_(c2eb)` = `"%s:\nBegin turn."`).
pub fn turn_message(team: TeamId) -> String {
    format!("{}:\nBegin turn.", team_display_name(team))
}

/// Team name for the message/screen.
pub fn team_display_name(team: TeamId) -> &'static str {
    TEAM_NAMES.get(usize::from(team.0)).copied().unwrap_or("")
}

// ---------------------------------------------------------------------------
// DialogMenu layout (`HELPFRAM` + `HELPOK`)
// ---------------------------------------------------------------------------

/// `HELPFRAM` — dialog background 301×231 (`MAX.RES`; in C++ `DLG_...`/`HELPFRM`).
pub const HELPFRAM_SIZE: (i32, i32) = (301, 231);
/// `WINDOW_MAIN_MAP` — the 448×448 map area in the 640×480 window (`window_manager.cpp`).
pub const MAIN_MAP_RECT: PixelRect = PixelRect::new(180, 18, 448, 448);
/// Dialog window: `HELPFRAM` centered in `WINDOW_MAIN_MAP`
/// (`Window(HELPFRM, GameManager_GetDialogWindowCenterMode())`).
pub const DIALOG_RECT: PixelRect = PixelRect::new(253, 126, 301, 231);
/// Text area: `canvas = Image(20, 14, 265, 175)` (`dialogmenu.cpp:159`).
pub const TEXT_RECT: PixelRect = PixelRect::new(20, 14, 265, 175);
/// `OK` button (`new Button(HELPOK_U, HELPOK_D, 120, 193)`, `:162`) in window coordinates.
pub const OK_RECT: PixelRect = PixelRect::new(120, 193, 65, 24);
/// Text `GNW_TEXT_OUTLINE | 0xFF` (`MessageManager_DrawMessageBoxText`).
pub const TEXT_COLOR: u8 = 0xFF;
/// `OK` button label color — `Fonts_GoldColor` (`SetCaption` default).
pub const CAPTION_GOLD: (u8, u8, u8) = (165, 175, 199);

/// Dialog window in frame coordinates.
pub const fn dialog_rect_frame() -> PixelRect {
    DIALOG_RECT
}

/// Text area in frame coordinates.
pub fn text_rect_frame() -> PixelRect {
    PixelRect::new(
        DIALOG_RECT.ulx + TEXT_RECT.ulx,
        DIALOG_RECT.uly + TEXT_RECT.uly,
        TEXT_RECT.width,
        TEXT_RECT.height,
    )
}

/// `OK` button in frame coordinates.
pub fn ok_rect_frame() -> PixelRect {
    PixelRect::new(
        DIALOG_RECT.ulx + OK_RECT.ulx,
        DIALOG_RECT.uly + OK_RECT.uly,
        OK_RECT.width,
        OK_RECT.height,
    )
}

/// Hit on the `OK` button (640×480 frame coordinates).
pub fn ok_at(x: i32, y: i32) -> bool {
    ok_rect_frame().contains(x, y)
}

// ---------------------------------------------------------------------------
// Turn-transition screen
//
// The state's «modal window»: `LocalGame::notice` stores a `TurnNotice` record,
// the game loop does not tick it, and input accepts only confirmation.
// ---------------------------------------------------------------------------

/// Turn-transition screen: «<Team>: Begin turn.» (`GameManager_UpdateGuiControl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnNotice {
    /// Which team starts the turn.
    pub team: TeamId,
}

impl TurnNotice {
    /// Record for team `team`.
    pub const fn new(team: TeamId) -> Self {
        Self { team }
    }

    /// Window text (`_(c2eb)` + `menu_team_names[team]`).
    pub fn message(&self) -> String {
        turn_message(self.team)
    }

    /// Window lines: `Text_SplitText` splits by `\n`, then by width 265.
    pub fn lines(&self) -> Vec<String> {
        wrap_gnw_text(&self.message(), TEXT_RECT.width)
    }
}

/// Draws the turn-transition window over the frame.
///
/// Layout — 1:1 `DialogMenu::DrawText` (`dialogmenu.cpp:31`):
/// * `HELPFRAM` background at `(253,126)`;
/// * lines in `GNW_TEXT_FONT_5`, color `0xFF` with a black outline,
///   step 10 px, the block centered in the 265×175 area, each line — centered;
/// * `HELPOK_U/D` button 65×24 at `(373,319)` labeled `OK`.
pub fn draw_turn_notice(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    palette: &UiPalette,
    notice: &TurnNotice,
    _mouse: Option<(i32, i32)>,
) -> u32 {
    let mut painted = 0;

    if let Some(helpfram) = menu.helpfram.as_ref() {
        painted += draw_big_image_at(
            frame,
            helpfram,
            palette,
            (DIALOG_RECT.ulx, DIALOG_RECT.uly),
            DIALOG_RECT,
        );
    } else {
        // No resource (trimmed `MAX.RES`): an honest frame instead of the window.
        frame.fill_rect(DIALOG_RECT, palette.rgba(0x22));
    }

    let font = GnwFontId::Font5.font();
    let row_height = font.height();
    let lines = notice.lines();
    let block = row_height * lines.len() as i32;
    let text = text_rect_frame();
    let mut y = text.uly + ((TEXT_RECT.height - block).max(0)) / 2;

    for line in &lines {
        let width = font.text_width(line);
        let x = text.ulx + ((TEXT_RECT.width - width).max(0)) / 2;

        painted += draw_text_gnw_shadowed(
            frame,
            x,
            y,
            font,
            line,
            palette.rgba(TEXT_COLOR),
            palette.rgba(0),
            palette.rgba(0),
        )
        .max(0) as u32;
        y += row_height;
    }

    // `OK` button: `HELPOK_U`/`HELPOK_D`, label `_(ae0b)` = "OK".
    let ok = ok_rect_frame();

    if let Some(pair) = menu.helpok.as_ref() {
        painted += blit_simple(frame, palette, pair.image(false), ok.ulx, ok.uly, true);
    } else {
        frame.fill_rect(ok, palette.rgba(0x17));
    }

    // `Button::SetCaption(caption, 2, 2)` → area (0,0) 63×22, text centered
    // at `((22 - 10) / 2) = 6` from the top of the button.
    let caption_rect = PixelRect::new(ok.ulx, ok.uly + 6, ok.width - 2, font.height());
    let (base, outline, shadow) = CAPTION_GOLD;

    painted += draw_text_gnw_centered_shadowed(
        frame,
        caption_rect,
        font,
        "OK",
        palette.rgba(base),
        palette.rgba(outline),
        palette.rgba(shadow),
    )
    .max(0) as u32;

    painted
}

/// Human teams in slot order (`GameManager_UpdateHumanPlayerCount`).
pub fn human_teams(team_types: &[TeamKind; LOCAL_TEAM_COUNT]) -> Vec<TeamId> {
    team_types
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == TeamKind::Player)
        .map(|(index, _)| TeamId(index as u8))
        .collect()
}

/// Transition line for the log/status.
pub fn notice_log_line(notice: &TurnNotice) -> String {
    format!("{}: begin turn", team_display_name(notice.team))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_is_centered_in_the_main_map() {
        // `((627 - 180 - 301) / 2) + 180 = 253`, `((465 - 18 - 231) / 2) + 18 = 126`.
        assert_eq!(DIALOG_RECT, PixelRect::new(253, 126, 301, 231));
        assert_eq!(
            MAIN_MAP_RECT.ulx + (MAIN_MAP_RECT.width - HELPFRAM_SIZE.0) / 2,
            253
        );
        assert_eq!(
            MAIN_MAP_RECT.uly + (MAIN_MAP_RECT.height - HELPFRAM_SIZE.1) / 2,
            126
        );
    }

    #[test]
    fn ok_button_matches_the_dialog_menu_controls() {
        // `Button(HELPOK_U, HELPOK_D, 120, 193)` 65×24 inside the dialog window.
        assert_eq!(OK_RECT, PixelRect::new(120, 193, 65, 24));
        assert_eq!(ok_rect_frame(), PixelRect::new(373, 319, 65, 24));
        assert!(ok_at(380, 325));
        assert!(!ok_at(253, 126), "угол окна — не кнопка");
    }

    #[test]
    fn turn_message_matches_the_oracle_string() {
        assert_eq!(turn_message(TeamId::RED), "Red Team:\nBegin turn.");
        assert_eq!(turn_message(TeamId::GREEN), "Green Team:\nBegin turn.");
        assert_eq!(turn_message(TeamId::BLUE), "Blue Team:\nBegin turn.");
        assert_eq!(turn_message(TeamId::GRAY), "Gray Team:\nBegin turn.");

        let notice = TurnNotice::new(TeamId::GREEN);
        assert_eq!(notice.lines(), vec!["Green Team:", "Begin turn."]);
    }

    #[test]
    fn text_block_is_vertically_centered() {
        // Two lines of 10 px → `(175 - 20) / 2 = 77` from the top of the text area.
        let text = text_rect_frame();
        let block = 10 * TurnNotice::new(TeamId::RED).lines().len() as i32;
        let y = text.uly + (TEXT_RECT.height - block).max(0) / 2;

        assert_eq!(text, PixelRect::new(273, 140, 265, 175));
        assert_eq!(y, 140 + 77);
    }

    #[test]
    fn human_teams_follow_the_team_slot_order() {
        let teams = human_teams(&[
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::Player,
            TeamKind::None,
        ]);

        assert_eq!(teams, vec![TeamId::RED, TeamId::BLUE]);
    }
}
