//! Victory/defeat screen — a port of `menu_draw_game_over_screen` (`menu.cpp:165`).
//!
//! 1:1 layout:
//! * background — one of `ENDGAME1`…`ENDGAME9` (`menu_briefing_backgrounds`);
//! * the **Game Over** title (`_(71f1)`) at (100,10), width 440, silver;
//! * team columns: `ulx` starts at 80 and grows by 110; the team number
//!   (1…4) at (ulx,44), the team name (`*_team_name`) at (ulx,61) with height 55;
//! * statistics rows: `Points` (116), `Factories Built` (166),
//!   `Mines Built` (198), `Buildings Built` (230), `Lost` buildings (246),
//!   `Units Built` (278), `Lost` mobile (294), `Upgrades` (326);
//! * places (`1st place`…`Eliminated`) — in the points row, for a generic game;
//! * `ENDARM` at (273, 480−22) and the `ENDOK` button at (293, 458) labelled **OK**.
//!
//! The numbers come from `GameState::teams` (`TeamInfo.team_points`,
//! `stats_*`, `casualties`), the places — from `WinLossStatus.team_rank`, which
//! is computed when the round is closed ([`crate::local::LocalOutcome`]).

use max_core::enums::{WinLossState, UNIT_END};
use max_core::team::{TeamId, TeamKind};
use max_core::universe::StateRef;
// Explicit import of the classification extension type (`UnitTypeClassify` — sealed in
// max_core); no glob re-exports (phase 5.2).
use max_core::UnitTypeClassify;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw_shadowed, GnwFontId};

use crate::local::{team_name, LocalGame, LOCAL_TEAM_COUNT};
use crate::menu::{blit_simple, draw_big_image, MenuAssets};

/// Text color `Fonts_BrightSilverColor` (`fonts.cpp`).
pub const SILVER: (u8, u8, u8) = (162, 172, 199);
/// Team number colors: `team_fonts[]` (`menu.cpp:169`).
pub const TEAM_FONTS: [(u8, u8, u8); LOCAL_TEAM_COUNT] = [
    (165, 230, 199),
    (165, 238, 199),
    (165, 245, 199),
    (165, 214, 199),
];

/// Place texts (`menu_game_over_screen_places`).
pub const PLACES: [&str; 5] = [
    "Eliminated",
    "1st place",
    "2nd place",
    "3rd place",
    "4th place",
];

/// Screen title.
pub const TITLE_RECT: PixelRect = PixelRect::new(100, 10, 440, 16);
/// Statistics rows: (label, y).
pub const ROWS: [(&str, i32); 7] = [
    ("Points", 116),
    ("Factories Built", 166),
    ("Mines Built", 198),
    ("Buildings Built", 230),
    ("Lost", 246),
    ("Units Built", 278),
    ("Lost", 294),
];
/// The `Upgrades` row (drawn separately — text with the credits amount).
pub const UPGRADES_ROW_Y: i32 = 326;
/// Width of the label column on the left.
pub const LABEL_WIDTH: i32 = 172;
/// OK button — `ENDOK`, (293,458).
pub const OK_RECT: PixelRect = PixelRect::new(293, 458, 72, 19);
/// `ENDARM` — (273, 480−22).
pub const ARM_POSITION: (i32, i32) = (273, 458);

/// Team name for the screen: for a human — the name from the settings, for the AI — `*_team_name`.
///
/// In hot-seat the player name is the same for everyone, so all teams there are `Red Team`,
/// `Green Team`, … (`menu.cpp:1941`: `player_name` is substituted only in
/// a single game). Renaming in game (`OptionsMenu` `PREFSPIC`) is stored in
/// [`crate::local::LocalGame::team_names`], so the name is taken from the game
/// rather than from the static settings.
pub fn display_name(local: &LocalGame, team: TeamId) -> String {
    local.display_name(team)
}

/// `ENDGAME1..9` background index.
///
/// C++: `Randomizer_Generate(9)`. The global RNG must not be touched (it is part of
/// the simulation and the CRC), so the background is chosen deterministically by turn and
/// player team — visually the same, but without affecting determinism.
pub fn background_index(local: &LocalGame) -> usize {
    let turn = local
        .outcome
        .as_ref()
        .map(|outcome| outcome.turn_counter)
        .unwrap_or(0)
        .max(0) as usize;
    let team = local
        .outcome
        .as_ref()
        .map(|outcome| outcome.team.0 as usize)
        .or_else(|| local.setup.human_team().map(|team| team.0 as usize))
        .unwrap_or(0);

    (turn + team) % 9
}

/// How many buildings were lost (`LOST` buildings, as in C++: `flags & BUILDING` —
/// a 2×2 footprint, i.e. [`max_core::classification::Footprint`]).
fn building_casualties(state: StateRef<'_>, team: TeamId) -> u32 {
    let info = &state.teams()[team.0 as usize];
    let mut total = 0;

    for index in 0..UNIT_END {
        let unit_type = max_core::enums::UnitType::try_from(index as u8).ok();
        let Some(unit_type) = unit_type else {
            continue;
        };

        if unit_type.is_quad() {
            total += info.casualties_for(index as u16);
        }
    }

    total
}

/// Losses of mobile units (there is an environment flag — `MOBILE_*`).
fn mobile_casualties(state: StateRef<'_>, team: TeamId) -> u32 {
    let info = &state.teams()[team.0 as usize];
    let mut total = 0;

    for index in 0..UNIT_END {
        let unit_type = max_core::enums::UnitType::try_from(index as u8).ok();
        let Some(unit_type) = unit_type else {
            continue;
        };

        if unit_type.environment().is_some() {
            total += info.casualties_for(index as u16);
        }
    }

    total
}

/// Draws the victory/defeat screen.
pub fn draw_game_over(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    state: StateRef<'_>,
    local: &LocalGame,
    _mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let font = GnwFontId::Font5.font();

    // Background: ENDGAME1..9; if resources are missing — FRAMEPIC/MAINPIC (honest fallback).
    let index = background_index(local);
    let mut painted = if let Some(background) = menu.endgame_backgrounds.get(index) {
        draw_big_image(frame, background)
    } else if let Some(background) = menu.framepic.as_ref() {
        draw_big_image(frame, background)
    } else {
        draw_big_image(frame, &menu.mainpic)
    };

    let Some(outcome) = local.outcome.as_ref() else {
        return painted;
    };

    let silver = |frame: &mut RgbaBuffer, x: i32, y: i32, text: &str, align: i32, width: i32| {
        let text_width = font.text_width(text);
        let x = match align {
            1 => x + (width - text_width).max(0),
            2 => x + ((width - text_width) / 2).max(0),
            _ => x,
        };
        draw_text_gnw_shadowed(
            frame,
            x,
            y,
            font,
            text,
            palette.rgba(SILVER.0),
            palette.rgba(SILVER.1),
            palette.rgba(SILVER.2),
        )
    };

    // Title `_(71f1)` = "Game Over".
    silver(
        frame,
        TITLE_RECT.ulx,
        TITLE_RECT.uly,
        "Game Over",
        0,
        TITLE_RECT.width,
    );

    // C++ passes the status of the **player's team** to `menu_draw_game_over_screen`
    // (`status.team_status[GameManager_PlayerTeam]`), not of each team.
    let player_team = local
        .outcome
        .as_ref()
        .map(|outcome| outcome.team)
        .or_else(|| local.setup.human_team())
        .unwrap_or(TeamId::RED);
    let generic = outcome.status.team_status[player_team.0 as usize] == WinLossState::Generic;

    // Team columns: only `team_type != NONE`.
    let mut ulx = 80;

    for team in 0..LOCAL_TEAM_COUNT {
        if state.teams()[team].team_type == TeamKind::None {
            continue;
        }

        ulx += 110;
        let lrx = ulx + 90;
        let team_id = TeamId(team as u8);
        let number = format!("{}", team + 1);
        let color = TEAM_FONTS[team];

        // Team number (165/230/199, 165/238/199, …).
        let number_width = font.text_width(&number);
        draw_text_gnw_shadowed(
            frame,
            ulx + ((90 - number_width) / 2).max(0),
            44,
            font,
            &number,
            palette.rgba(color.0),
            palette.rgba(color.1),
            palette.rgba(color.2),
        );

        // Team name — `Text_TextBox(..., horizontal_align = true)`.
        let name = display_name(local, team_id);
        silver(frame, ulx, 61, &name, 2, lrx - ulx);

        let info = &state.teams()[team];
        let rank = outcome.status.team_rank[team];

        // Points, then the place (generic) or Mission Success/Failed (script).
        if generic {
            silver(frame, ulx, 116, &info.team_points.to_string(), 2, lrx - ulx);

            let place = rank.clamp(0, 4) as usize;
            silver(frame, ulx, 132, PLACES[place], 2, lrx - ulx);
        } else if team_id == TeamId::RED {
            let text = if outcome.status.team_status[team] == WinLossState::Won {
                "Mission Success"
            } else {
                "Mission Failed"
            };
            silver(frame, ulx, 116, text, 2, lrx - ulx);
        }

        silver(
            frame,
            ulx,
            166,
            &info.stats_factories_built.to_string(),
            2,
            lrx - ulx,
        );
        silver(
            frame,
            ulx,
            198,
            &info.stats_mines_built.to_string(),
            2,
            lrx - ulx,
        );
        silver(
            frame,
            ulx,
            230,
            &info.stats_buildings_built.to_string(),
            2,
            lrx - ulx,
        );
        silver(
            frame,
            ulx,
            246,
            &building_casualties(state, team_id).to_string(),
            2,
            lrx - ulx,
        );
        silver(
            frame,
            ulx,
            278,
            &info.stats_units_built.to_string(),
            2,
            lrx - ulx,
        );
        silver(
            frame,
            ulx,
            294,
            &mobile_casualties(state, team_id).to_string(),
            2,
            lrx - ulx,
        );

        let credits = info.stats_gold_spent_on_upgrades;
        let credits_word = if credits == 1 { "Credit" } else { "Credits" };
        silver(
            frame,
            ulx,
            UPGRADES_ROW_Y,
            &format!("{credits} {credits_word}"),
            2,
            lrx - ulx,
        );
    }

    // Row labels on the left (x=0, width 172, right-aligned).
    // «Lost» occurs twice: building losses (246) and mobile unit losses (294).
    for (label, y) in ROWS {
        silver(frame, 0, y, label, 1, LABEL_WIDTH);
    }
    silver(frame, 0, UPGRADES_ROW_Y, "Upgrades", 1, LABEL_WIDTH);

    // `ENDARM` (273, 480−22) and the `ENDOK` button (293,458) labelled "OK".
    if let Some(arm) = menu.endarm.as_ref() {
        painted += blit_simple(frame, palette, arm, ARM_POSITION.0, ARM_POSITION.1, true);
    }
    if let Some(pair) = menu.endok.as_ref() {
        painted += blit_simple(
            frame,
            palette,
            pair.image(false),
            OK_RECT.ulx,
            OK_RECT.uly,
            true,
        );
        silver(frame, OK_RECT.ulx, OK_RECT.uly + 5, "OK", 2, OK_RECT.width);
    }

    painted
}

/// Hit on the OK button.
pub fn ok_at(x: i32, y: i32) -> bool {
    OK_RECT.contains(x, y)
}

/// Name of the winning team for the log (by ranks).
pub fn winner_line(local: &LocalGame) -> String {
    let Some(outcome) = local.outcome.as_ref() else {
        return String::new();
    };

    let winner = (0..LOCAL_TEAM_COUNT)
        .find(|team| outcome.status.team_rank[*team] == 1)
        .map(|team| team_name(TeamId(team as u8)))
        .unwrap_or("nobody");

    format!(
        "{}: {}",
        if outcome.victory { "Victory" } else { "Defeat" },
        winner
    )
}

/// Silver text color in the window palette (`Rgba` of index 162).
pub fn silver_color(palette: &crate::panels::UiPalette) -> Rgba {
    palette.rgba(SILVER.0)
}

#[cfg(test)]
mod tests {
    use max_core::universe::GameUniverse;

    use super::*;

    #[test]
    fn background_index_is_bounded() {
        let mut local = LocalGame::new(crate::local::LocalSetup::default());
        local.outcome = Some(crate::local::LocalOutcome {
            victory: true,
            team: TeamId::RED,
            status: GameUniverse::default().snapshot().evaluate_status(),
            turn_counter: 17,
            player_name: "Tester".to_string(),
        });

        assert!(background_index(&local) < 9);
    }

    #[test]
    fn places_match_the_cpp_table() {
        assert_eq!(PLACES[0], "Eliminated");
        assert_eq!(PLACES[1], "1st place");
        assert_eq!(PLACES[4], "4th place");
    }

    #[test]
    fn ok_button_matches_cpp_rectangle() {
        assert_eq!(OK_RECT, PixelRect::new(293, 458, 72, 19));
        assert!(ok_at(300, 460));
        assert!(!ok_at(10, 10));
    }
}
