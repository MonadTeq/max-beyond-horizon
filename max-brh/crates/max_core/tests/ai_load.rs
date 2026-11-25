//! AI state after a V70 mission load — `Ai_FileLoad` (`ai.cpp:164`) and the
//! `GAME_STATE_10_LOAD_GAME` entry (`game_manager.cpp:1929-1961`).
//!
//! Checked:
//!
//! 1. A loaded save leaves the AI **without** a task queue and with `need_init`
//!    set: `AiPlayer::FileSave` (`aiplayer.cpp:4091`) serializes no task lists, so
//!    C++ `Ai_FileLoad` starts with `Ai_Init()` and the first `Ai_BeginTurn`
//!    rebuilds the tasks (`aiplayer.cpp:2242`). `need_init == false` after a load
//!    would leave the computer with an empty queue forever — the mission blocker.
//! 2. The `Ai_Init` weight tables (`aiplayer.cpp:2951-3305`) are filled on load,
//!    so the first `BeginTurn` can choose build orders.
//! 3. The load must **not** run `Ai_SetupStrategy` (`game_manager.cpp:3098` is
//!    called only by `GameManager_InitGame`): that would re-roll the team clan and
//!    call `ResourceManager_InitClanUnitValues`, which clears the loaded
//!    `TeamUnits` complexes (`TeamUnits::ClearComplexes`) — the AI then has no
//!    building/resource managers and never builds from the mission's base.
//!
//! Fixture: `data/save2_v70.cam` — campaign 2 ("Heart of the Matter"), RED human,
//! GREEN and BLUE computer, each with a saved complex.

use max_core::ai::AiPlayer;
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::save::SaveFile;
use max_core::team::{TeamId, TeamKind};
use max_core::universe::GameUniverse;

/// `max-original/SAVE2.CAM`.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

fn parse() -> SaveFile {
    SaveFile::parse(SAVE2).expect("save2 parses as V70")
}

fn loaded() -> GameUniverse {
    GameUniverse::from_save(&parse())
}

/// Computer teams of the fixture (GREEN and BLUE; `save.header.team_type`).
fn computer_teams(save: &SaveFile) -> Vec<TeamId> {
    (0..PLAYER_TEAM_COUNT)
        .filter(|team| save.teams[*team].team_type == TeamKind::Computer)
        .map(|team| TeamId(team as u8))
        .collect()
}

// ---------------------------------------------------------------------------
// 1. `Ai_FileLoad`: no tasks in the file, `need_init` set, tables filled
// ---------------------------------------------------------------------------

#[test]
fn v70_load_leaves_the_ai_tasks_for_the_first_begin_turn() {
    let save = parse();
    let universe = loaded();
    let view = universe.snapshot();

    assert_eq!(
        view.ai().task_manager.task_count(),
        0,
        "V70 stores no task lists (`AiPlayer::FileSave` writes none): the loaded queue is empty"
    );

    for team in 0..PLAYER_TEAM_COUNT {
        let player: &AiPlayer = &view.ai().players[team];
        let label = format!("team {team}");

        // `Ai_Init` (`ai.cpp:154`) → `AiPlayer::Init` (`aiplayer.cpp:2902`).
        assert!(
            player.need_init,
            "{label}: `need_init` must stay set after a load — the first \
             `Ai_BeginTurn` (`aiplayer.cpp:2242`) rebuilds the task list"
        );
        assert!(
            player.tasks_pending,
            "{label}: `Ai_Init` marks the team as having work"
        );
        assert!(
            !player.weight_table_tanks.is_empty() && !player.weight_table_scout.is_empty(),
            "{label}: the `Ai_Init` weight tables must be filled on load \
             (`aiplayer.cpp:2951-3305`)"
        );
    }

    // The saved `AiPlayer::FileLoad` fields are kept, not re-rolled: the fixture
    // stores `strategy = 0` (RANDOM) for both computers and no spotted units.
    for team in computer_teams(&save) {
        let saved = save.ai_players[team.0 as usize]
            .as_deref()
            .expect("the fixture stores the computer team's AI state");
        assert_eq!(
            view.ai().players[team.0 as usize].strategy as u8,
            saved.strategy,
            "{team:?}: the strategy from the file must survive the load"
        );
        assert_eq!(
            view.ai().players[team.0 as usize].greenhouse_ratio,
            saved.greenhouse_ratio,
            "{team:?}: greenhouse_ratio from `AiPlayer::FileLoad`"
        );
        assert_eq!(
            view.ai().players[team.0 as usize].target_location,
            saved.target_location,
            "{team:?}: target_location from `AiPlayer::FileLoad`"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. The load does not run `Ai_SetupStrategy`: clans and complexes stay
// ---------------------------------------------------------------------------

#[test]
fn v70_load_keeps_the_saved_team_complexes_and_clans() {
    let save = parse();
    let universe = loaded();
    let view = universe.snapshot();

    for team in 0..PLAYER_TEAM_COUNT {
        let loaded_units = &view.team_units()[team];
        let saved_units = &save.team_units[team];

        assert_eq!(
            loaded_units.complexes().len(),
            saved_units.complexes.len(),
            "team {team}: `TeamUnits::FileLoad` restores the complexes; \
             `Ai_SetupStrategy` → `apply_clan` would clear them"
        );

        assert_eq!(
            view.teams()[team].team_clan,
            save.teams[team].team_clan,
            "team {team}: the clan comes from the save header (`saveload.cpp:847`); \
             a load must not re-roll it"
        );
    }

    // Every unit's complex reference resolves in its team's complex list. With the
    // cleared complexes this is the observable "AI does nothing" state: the
    // builders carry ids of complexes that no longer exist.
    for id in view.unit_ids_in_order() {
        let Some(unit) = view.unit(id) else {
            continue;
        };

        if unit.team == TeamId::ALIEN {
            continue;
        }

        let Some(complex) = unit.complex else {
            continue;
        };

        assert!(
            view.team_units()[unit.team.0 as usize]
                .complexes()
                .iter()
                .any(|entry| entry.id == complex.0 as i16),
            "{id:?} references complex {complex:?} which is missing from team {:?}",
            unit.team
        );
    }

    // The computer teams of this fixture really do have complexes to lose.
    for team in computer_teams(&save) {
        assert!(
            !view.team_units()[team.0 as usize].complexes().is_empty(),
            "{team:?}: the fixture's AI team has a saved complex"
        );
    }
}
