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
use max_core::enums::{Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::map::{ResourceKind, ResourceMapEntry};
use max_core::team::TeamId;
use max_core::team::TeamKind;
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
