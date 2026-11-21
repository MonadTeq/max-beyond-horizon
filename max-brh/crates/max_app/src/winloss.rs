//! Экран победы/поражения — порт `menu_draw_game_over_screen` (`menu.cpp:165`).
//!
//! Раскладка 1:1:
//! * фон — один из `ENDGAME1`…`ENDGAME9` (`menu_briefing_backgrounds`);
//! * заголовок **Game Over** (`_(71f1)`) в (100,10), ширина 440, серебряный;
//! * колонки команд: `ulx` начинается с 80 и растёт на 110; номер команды
//!   (1…4) в (ulx,44), имя команды (`*_team_name`) в (ulx,61) высотой 55;
//! * строки статистики: `Points` (116), `Factories Built` (166),
//!   `Mines Built` (198), `Buildings Built` (230), `Lost` зданий (246),
//!   `Units Built` (278), `Lost` мобильных (294), `Upgrades` (326);
//! * места (`1st place`…`Eliminated`) — в строке очков, для generic-партии;
//! * `ENDARM` в (273, 480−22) и кнопка `ENDOK` в (293, 458) с подписью **OK**.
//!
//! Числа берутся из `GameState::teams` (`TeamInfo.team_points`,
//! `stats_*`, `casualties`), места — из `WinLossStatus.team_rank`, который
//! считается на момент закрытия раунда ([`crate::local::LocalOutcome`]).

use max_core::enums::{unit_flags, WinLossState, UNIT_END};
use max_core::team::{TeamId, TeamKind};
use max_core::GameState;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw_shadowed, GnwFontId};

use crate::local::{team_name, LocalGame, LOCAL_TEAM_COUNT};
use crate::menu::{blit_simple, draw_big_image, MenuAssets};

/// Цвет текста `Fonts_BrightSilverColor` (`fonts.cpp`).
pub const SILVER: (u8, u8, u8) = (162, 172, 199);
/// Цвета номеров команд: `team_fonts[]` (`menu.cpp:169`).
pub const TEAM_FONTS: [(u8, u8, u8); LOCAL_TEAM_COUNT] = [
    (165, 230, 199),
    (165, 238, 199),
    (165, 245, 199),
    (165, 214, 199),
];

/// Тексты мест (`menu_game_over_screen_places`).
pub const PLACES: [&str; 5] = [
    "Eliminated",
    "1st place",
    "2nd place",
    "3rd place",
    "4th place",
];

/// Заголовок экрана.
pub const TITLE_RECT: PixelRect = PixelRect::new(100, 10, 440, 16);
/// Строки статистики: (подпись, y).
pub const ROWS: [(&str, i32); 7] = [
    ("Points", 116),
    ("Factories Built", 166),
    ("Mines Built", 198),
    ("Buildings Built", 230),
    ("Lost", 246),
    ("Units Built", 278),
    ("Lost", 294),
];
/// Строка `Upgrades` (рисуется отдельно — текст с числом кредитов).
pub const UPGRADES_ROW_Y: i32 = 326;
/// Ширина левой колонки подписей.
pub const LABEL_WIDTH: i32 = 172;
/// Кнопка OK — `ENDOK`, (293,458).
pub const OK_RECT: PixelRect = PixelRect::new(293, 458, 72, 19);
/// `ENDARM` — (273, 480−22).
pub const ARM_POSITION: (i32, i32) = (273, 458);

/// Имя команды для экрана: у человека — имя из настроек, у ИИ — `*_team_name`.
///
/// В hot-seat имя игрока одно на всех, поэтому там все команды — `Red Team`,
/// `Green Team`, … (`menu.cpp:1941`: `player_name` подставляется только в
/// одиночной игре). Переименование в игре (`OptionsMenu` `PREFSPIC`) хранится в
/// [`crate::local::LocalGame::team_names`], поэтому имя берётся из партии, а не
/// из статичных настроек.
pub fn display_name(local: &LocalGame, team: TeamId) -> String {
    local.display_name(team)
}

/// Индекс фона `ENDGAME1..9`.
///
/// C++: `Randomizer_Generate(9)`. Глобальный ГПСЧ трогать нельзя (он часть
/// симуляции и CRC), поэтому фон детерминированно выбирается по ходу и
/// команде игрока — визуально так же, но без влияния на детерминизм.
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

/// Сколько зданий потеряно (`LOST` зданий, как в C++: `flags & BUILDING`).
fn building_casualties(state: &GameState, team: TeamId) -> u32 {
    let info = &state.teams[team.0 as usize];
    let mut total = 0;

    for index in 0..UNIT_END {
        let unit_type = max_core::enums::UnitType::try_from(index as u8).ok();
        let Some(unit_type) = unit_type else {
            continue;
        };
        let flags = max_core::access::unit_type_flags(unit_type);

        if flags & unit_flags::BUILDING != 0 {
            total += info.casualties_for(index as u16);
        }
    }

    total
}

/// Потери мобильных юнитов (`MOBILE_AIR_UNIT | MOBILE_SEA_UNIT | MOBILE_LAND_UNIT`).
fn mobile_casualties(state: &GameState, team: TeamId) -> u32 {
    let info = &state.teams[team.0 as usize];
    let mut total = 0;

    for index in 0..UNIT_END {
        let unit_type = max_core::enums::UnitType::try_from(index as u8).ok();
        let Some(unit_type) = unit_type else {
            continue;
        };
        let flags = max_core::access::unit_type_flags(unit_type);

        if flags
            & (unit_flags::MOBILE_AIR_UNIT
                | unit_flags::MOBILE_SEA_UNIT
                | unit_flags::MOBILE_LAND_UNIT)
            != 0
        {
            total += info.casualties_for(index as u16);
        }
    }

    total
}

/// Рисует экран победы/поражения.
pub fn draw_game_over(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    state: &GameState,
    local: &LocalGame,
    _mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let font = GnwFontId::Font5.font();

    // Фон: ENDGAME1..9; если ресурсов нет — FRAMEPIC/MAINPIC (честный fallback).
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

    // Заголовок `_(71f1)` = "Game Over".
    silver(
        frame,
        TITLE_RECT.ulx,
        TITLE_RECT.uly,
        "Game Over",
        0,
        TITLE_RECT.width,
    );

    // C++ передаёт в `menu_draw_game_over_screen` статус **команды игрока**
    // (`status.team_status[GameManager_PlayerTeam]`), а не каждой команды.
    let player_team = local
        .outcome
        .as_ref()
        .map(|outcome| outcome.team)
        .or_else(|| local.setup.human_team())
        .unwrap_or(TeamId::RED);
    let generic = outcome.status.team_status[player_team.0 as usize] == WinLossState::Generic;

    // Колонки команд: только `team_type != NONE`.
    let mut ulx = 80;

    for team in 0..LOCAL_TEAM_COUNT {
        if state.teams[team].team_type == TeamKind::None {
            continue;
        }

        ulx += 110;
        let lrx = ulx + 90;
        let team_id = TeamId(team as u8);
        let number = format!("{}", team + 1);
        let color = TEAM_FONTS[team];

        // Номер команды (165/230/199, 165/238/199, …).
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

        // Имя команды — `Text_TextBox(..., horizontal_align = true)`.
        let name = display_name(local, team_id);
        silver(frame, ulx, 61, &name, 2, lrx - ulx);

        let info = &state.teams[team];
        let rank = outcome.status.team_rank[team];

        // Очки, затем место (generic) или Mission Success/Failed (скрипт).
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

    // Подписи строк слева (x=0, ширина 172, выравнивание вправо).
    // «Lost» встречается дважды: потери зданий (246) и мобильных юнитов (294).
    for (label, y) in ROWS {
        silver(frame, 0, y, label, 1, LABEL_WIDTH);
    }
    silver(frame, 0, UPGRADES_ROW_Y, "Upgrades", 1, LABEL_WIDTH);

    // `ENDARM` (273, 480−22) и кнопка `ENDOK` (293,458) с подписью "OK".
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

/// Попадание в кнопку OK.
pub fn ok_at(x: i32, y: i32) -> bool {
    OK_RECT.contains(x, y)
}

/// Имя победившей команды для журнала (по рангам).
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

/// Цвет серебряного текста в палитре окна (`Rgba` индекса 162).
pub fn silver_color(palette: &crate::panels::UiPalette) -> Rgba {
    palette.rgba(SILVER.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_index_is_bounded() {
        let mut local = LocalGame::new(crate::local::LocalSetup::default());
        local.outcome = Some(crate::local::LocalOutcome {
            victory: true,
            team: TeamId::RED,
            status: max_core::victory::evaluate_status(&GameState::new(), 0),
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
