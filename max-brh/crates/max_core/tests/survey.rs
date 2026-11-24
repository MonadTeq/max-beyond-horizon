//! Team resource knowledge — a port of `Survey_SurveyArea` (`survey.cpp:84-121`)
//! and `Survey_RenderMarkers` (`survey.cpp:123-155`).
//!
//! C++: `Access_UpdateMapStatus` (`access.cpp:762-768`) on adding coverage
//! (`mode = true`) calls `Survey_SurveyArea(unit, 1)` for `SURVEYOR`/`MINELAYR`/
//! `SEAMNLYR`/`COMMANDO`. Inside the square `grid ± 1` (3×3), cells with
//! surface `AIR` are skipped, and the **surveyor** sets its team's bit in
//! `ResourceManager_CargoMap` (`hash_team_id`): `RED = 0x2000`
//! (`enums.hpp:40-43`). Knowledge is cumulative — `|=`, so the surveyor leaving
//! does not remove it; icons (`Survey_RenderMarkers`) are shown only on
//! marked cells, and the `GameManager_MaxSurvey` mask (the `MAXSURVEY` cheat) gives
//! `0xFFFF`.

use max_core::access;
use max_core::enums::{PlayMode, Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::map::{ResourceKind, ResourceMapEntry};
use max_core::team::TeamId;
use max_core::team::TeamKind;
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

/// Game: the whole map is land, each cell has 8 materials, RED is a human,
/// GREEN is a computer. `UNKNOWN` — no team has surveyed.
fn land_state_with_resources() -> GameUniverse {
    let mut state = GameUniverse::default();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let _ = state.map_mut().surface.set(x, y, Surface::LAND);
            state.map_mut().resources.set(
                x,
                y,
                ResourceMapEntry::new(8, Some(ResourceKind::Raw), 0),
            );
        }
    }

    state.teams_mut()[0].team_type = TeamKind::Player;
    state.teams_mut()[1].team_type = TeamKind::Computer;
    state.team_units_table_mut()[0] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    state.team_units_table_mut()[1] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);

    state
}

/// Whether the cell is surveyed by the team (`ResourceManager_CargoMap & hash_team_id`).
fn known(state: &GameUniverse, x: i16, y: i16, team: u8) -> bool {
    state.snapshot().map().resources.get(x, y).is_visible_to(team)
}

#[test]
fn surveyor_marks_its_3x3_for_its_own_team_only() {
    let mut state = land_state_with_resources();
    // A cell in the survey square with surface AIR — C++ skips it.
    let _ = state.map_mut().surface.set(21, 21, Surface::AIR);

    let surveyor = state
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
        .expect("surveyor");

    // `spawn_unit` → `GameUniverse::update_map_status(id, true)` — like
    // `Access_UpdateMapStatus` after `DeployUnit`.
    let _ = surveyor;

    for y in 19..=21 {
        for x in 19..=21 {
            if (x, y) == (20, 20) || (x, y) == (21, 21) {
                // The surveyor's own cell is surveyed, AIR — skipped.
                assert_eq!(
                    known(&state, x, y, 0),
                    (x, y) == (20, 20),
                    "клетка ({x},{y}): AIR пропускается, остальные помечаются"
                );
                continue;
            }

            assert!(
                known(&state, x, y, 0),
                "клетка ({x},{y}) в 3×3 разведчика обязана быть разведана"
            );
            assert!(
                !known(&state, x, y, 1),
                "клетка ({x},{y}) не должна быть разведана чужой командой"
            );
        }
    }

    // Outside the 3×3 square there is no knowledge.
    for (x, y) in [(22, 20), (18, 20), (20, 22), (20, 18)] {
        assert!(!known(&state, x, y, 0), "клетка ({x},{y}) вне 3×3");
    }

    // The RED bit — `HASH_TEAM_RED = 0x2000` (bit 13), not the low bit.
    assert_eq!(
        state.snapshot().map().resources.get(19, 19).visibility(),
        0b1000,
        "RED помечает клетку битом 13 (HASH_TEAM_RED)"
    );
    assert_eq!(
        state.snapshot().map().resources.get(20, 20).bits() & 0x3C00,
        0x2000,
        "маска разведанности — ровно hash_team_id красных"
    );
}

#[test]
fn survey_knowledge_accumulates_and_is_not_removed_by_movement() {
    let mut state = land_state_with_resources();
    let surveyor = state
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
        .expect("surveyor");

    // The surveyor drove four cells east: `write_unit` recomputes
    // the coverage of the old and new cells (`Access_UpdateMapStatus`).
    let mut unit = state.snapshot().unit(surveyor).expect("surveyor").clone();
    unit.grid = Point::new(24, 20);
    state.write_unit(surveyor, unit);

    // The new place is surveyed…
    for (x, y) in [(23, 19), (24, 20), (25, 21)] {
        assert!(known(&state, x, y, 0), "новая клетка ({x},{y})");
    }

    // …the old one remains surveyed (C++ `|=`, no removal).
    for (x, y) in [(19, 19), (20, 20), (21, 21)] {
        assert!(
            known(&state, x, y, 0),
            "прежняя клетка ({x},{y}) не снимается"
        );
    }

    // Intermediate route cells are not surveyed: C++ marks only the current
    // square `grid ± 1`, there is no teleport by cells.
    assert!(
        !known(&state, 22, 20, 0),
        "клетка между позициями не разведана"
    );
}

#[test]
fn only_a_surveyor_marks_resources() {
    let mut state = land_state_with_resources();

    // `Access_UpdateMapStatus` calls `Survey_SurveyArea` for mines/commandos too, but
    // **marks** cells only `SURVEYOR` (`survey.cpp:108-117`).
    for unit_type in [
        UnitType::MineLayer,
        UnitType::SeaMineLayer,
        UnitType::Infiltrator,
        UnitType::Tank,
        UnitType::Engineer,
    ] {
        let id = state
            .spawn_unit(unit_type, TeamId::RED, Point::new(40, 40), 0)
            .expect("unit");
        assert_eq!(
            state.snapshot().map().resources.get(40, 40).visibility(),
            0,
            "{unit_type:?} не помечает ресурсы"
        );
        state.destroy_unit(id);
    }

    // And the surveyor — marks.
    let _ = state
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(40, 40), 0)
        .expect("surveyor");
    assert_eq!(state.snapshot().map().resources.get(40, 40).visibility(), 0b1000);
}

#[test]
fn knowledge_is_part_of_the_state_and_deterministic() {
    fn run() -> GameUniverse {
        let mut state = land_state_with_resources();
        let surveyor = state
            .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(30, 30), 0)
            .expect("surveyor");
        let mut unit = state.snapshot().unit(surveyor).expect("surveyor").clone();
        unit.grid = Point::new(32, 30);
        state.write_unit(surveyor, unit);
        state
    }

    let first = run();
    let second = run();

    assert_eq!(
        first.snapshot().map().resources.to_le_bytes(),
        second.snapshot().map().resources.to_le_bytes(),
        "знание ресурсов — часть состояния и детерминировано"
    );
    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes(),
        "снапшот включает карту ресурсов с битами знания"
    );

    // Snapshot/CRC change together with knowledge: it is not a UI field.
    let mut learned = first.clone();
    let entry = learned
        .snapshot()
        .map()
        .resources
        .get(50, 50)
        .with_team_visible(0);
    learned.map_mut().resources.set(50, 50, entry);
    assert_ne!(
        first.snapshot().snapshot_bytes(),
        learned.snapshot().snapshot_bytes(),
        "новое знание меняет снапшот (CRC/десинк-детектор)"
    );
}

/// Icon rendering (`Survey_RenderMarkers`) — only for marked cells;
/// the team mask is taken from `hash_team_id`.
#[test]
fn render_mask_matches_the_cpp_hash_team_id() {
    let mut state = land_state_with_resources();

    // RED marks (10,10), GREEN does not.
    let entry = state.snapshot().map().resources.get(10, 10).with_team_visible(0);
    state.map_mut().resources.set(10, 10, entry);

    let mask_red = max_core::map::team_visibility_bit(0) as u16;
    let mask_green = max_core::map::team_visibility_bit(1) as u16;

    assert_eq!(mask_red, 0b1000, "HASH_TEAM_RED = 0x2000 (бит 13)");
    assert_eq!(mask_green, 0b0100, "HASH_TEAM_GREEN = 0x1000 (бит 12)");
    assert!(state.snapshot().map().resources.get(10, 10).is_visible_to(0));
    assert!(!state.snapshot().map().resources.get(10, 10).is_visible_to(1));

    // `Survey_RenderMarkers` additionally requires `surface != AIR`.
    let _ = state.map_mut().surface.set(10, 10, Surface::AIR);
    assert_eq!(
        state.snapshot().modified_surface_type(Point::new(10, 10), false),
        access::SURFACE_TYPE_AIR,
        "иконка на AIR-клетке не рисуется (проверка в рендере)"
    );

    // A surveyor in `ORDER_DISABLE` does not update knowledge
    // (`Access_UpdateMapStatus`: `if (unit->GetOrder() != ORDER_DISABLE)`).
    // Spawning at (60,60) surveyed 3×3; switch to `ORDER_DISABLE` and move —
    // the new cell must remain unsurveyed.
    let surveyor = state
        .spawn_unit(UnitType::Surveyor, TeamId::GREEN, Point::new(60, 60), 0)
        .expect("surveyor");
    assert!(known(&state, 60, 60, 1), "спавн разведал клетку");

    let mut unit = state.snapshot().unit(surveyor).expect("surveyor").clone();
    unit.orders = UnitOrder::Disable;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.grid = Point::new(64, 64);
    state.write_unit(surveyor, unit);
    assert!(
        !known(&state, 64, 64, 1),
        "ORDER_DISABLE не обновляет карту ресурсов"
    );
}

// ---------------------------------------------------------------------------
// `TaskAutoSurvey` (`taskautosurvey.cpp`) — the Auto mode of a surveyor
// ---------------------------------------------------------------------------

/// The same playable state as [`land_state_with_resources`], but with an
/// **active turn** and `PlayMode::TurnBased`: `UnitsManager_PerformAutoSurvey`
/// (`units_manager.cpp:2585-2587`) needs both (`GameManager_PlayMode !=
/// PLAY_MODE_UNKNOWN`, `GameManager_IsActiveTurn`).
fn auto_survey_state() -> GameUniverse {
    let mut state = land_state_with_resources();
    state.set_play_mode(PlayMode::TurnBased);
    state.set_active_turn_team(TeamId::RED);
    state
}

/// Land map with a single deposit at `(24..26, 24..26)` and no other materials —
/// the `TaskAutoSurvey` scenario (the C++ surveyor looks for a **known** deposit,
/// not for any surveyed cell).
fn deposit_state() -> GameUniverse {
    let mut state = GameUniverse::default();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let _ = state.map_mut().surface.set(x, y, Surface::LAND);
            state.map_mut().resources.set(x, y, ResourceMapEntry::NONE);
        }
    }

    for y in 24..=26 {
        for x in 24..=26 {
            state.map_mut().resources.set(
                x,
                y,
                ResourceMapEntry::new(8, Some(ResourceKind::Raw), 0),
            );
        }
    }

    state.teams_mut()[0].team_type = TeamKind::Player;
    state.teams_mut()[1].team_type = TeamKind::Computer;
    state.team_units_table_mut()[0] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    state.team_units_table_mut()[1] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    state.set_play_mode(PlayMode::TurnBased);
    state.set_active_turn_team(TeamId::RED);
    state
}

/// Enables the `Auto` unit-menu toggle (`UnitInfo::auto_survey`).
fn enable_auto_survey(state: &mut GameUniverse, id: UnitId) {
    let Some(unit) = state.unit_mut(id) else {
        panic!("unit");
    };

    unit.auto_survey = true;
}

/// Number of AI tasks (`TaskManager::GetTaskCount`).
fn task_count(state: &GameUniverse) -> usize {
    state.snapshot().ai().task_manager.task_count()
}

#[test]
fn auto_survey_drives_the_scout_to_a_known_deposit_and_reveals_it() {
    let mut state = deposit_state();

    // The deposit centre is already surveyed by RED — the "known deposit" of
    // `TaskAutoSurvey::Execute` (`taskautosurvey.cpp:189`); its arms are not.
    let entry = state
        .snapshot()
        .map()
        .resources
        .get(25, 25)
        .with_team_visible(0);
    state.map_mut().resources.set(25, 25, entry);

    let surveyor = state
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
        .expect("surveyor");
    enable_auto_survey(&mut state, surveyor);

    // The scout starts away from the deposit: its 3×3 does not cover it.
    assert!(known(&state, 25, 25, 0), "the deposit centre is known");
    assert!(!known(&state, 24, 26, 0), "the deposit arms are unknown");

    // One turn of the Auto mode: `PerformAutoSurvey` creates the task, the task
    // issues `ORDER_MOVE` to the known deposit and the scout drives there.
    let mut moved = false;

    for _ in 0..40 {
        state.tick();

        let unit = state.snapshot().unit(surveyor).expect("surveyor").clone();

        if unit.grid != Point::new(20, 20) {
            moved = true;
        }

        if unit.speed == 0 {
            break;
        }
    }

    assert!(
        moved,
        "the surveyor must drive to the known deposit (Auto mode)"
    );

    let position = state.snapshot().unit(surveyor).expect("surveyor").grid;
    assert!(
        (i32::from(position.x) - 25).abs() <= 2 && (i32::from(position.y) - 25).abs() <= 2,
        "the scout moves toward the deposit (20,20) → (25,25), now {position:?}"
    );

    // The deposit's arms are now surveyed, so the resource icons appear there.
    assert!(
        known(&state, 24, 26, 0) || known(&state, 26, 24, 0) || known(&state, 24, 25, 0),
        "the trip must reveal the deposit cells around (25,25)"
    );

    // Exactly one task owns the scout: C++ `if (!unit->GetTask())
    // Ai_EnableAutoSurvey(unit)` (`units_manager.cpp:2594-2596`). The port used to
    // create a new task on every tick.
    assert_eq!(task_count(&state), 1, "one AutoSurvey task, no task leak");
}

#[test]
fn auto_survey_ends_when_everything_is_surveyed_and_clears_the_flag() {
    let mut state = auto_survey_state();

    // The whole map is already surveyed: the spiral finds nothing to look at and
    // `TaskAutoSurvey::Execute` gives up (`taskautosurvey.cpp:115-122`).
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let entry = state.snapshot().map().resources.get(x, y).with_team_visible(0);
            state.map_mut().resources.set(x, y, entry);
        }
    }

    let surveyor = state
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
        .expect("surveyor");
    enable_auto_survey(&mut state, surveyor);

    for _ in 0..8 {
        state.tick();

        if !state.snapshot().unit(surveyor).expect("surveyor").auto_survey {
            break;
        }
    }

    let unit = state.snapshot().unit(surveyor).expect("surveyor");
    assert!(!unit.auto_survey, "the Auto mode turns itself off");
    assert_eq!(unit.grid, Point::new(20, 20), "nothing left to survey — no move");
    assert_eq!(task_count(&state), 0, "the task is removed with the Auto mode");
}

#[test]
fn auto_survey_needs_the_surveyors_speed() {
    let mut state = auto_survey_state();

    let surveyor = state
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
        .expect("surveyor");
    enable_auto_survey(&mut state, surveyor);

    {
        let unit = state.unit_mut(surveyor).expect("surveyor");
        unit.speed = 0;
    }

    // `UnitsManager_PerformAutoSurvey` (`units_manager.cpp:2587`): `unit->speed`
    // must be non-zero, so no task is created and the scout stays put.
    for _ in 0..5 {
        state.tick();
    }

    assert_eq!(task_count(&state), 0, "no task without speed");
    assert_eq!(
        state.snapshot().unit(surveyor).expect("surveyor").grid,
        Point::new(20, 20)
    );
}
