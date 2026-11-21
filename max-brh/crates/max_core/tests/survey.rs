//! Знание ресурсов командой — порт `Survey_SurveyArea` (`survey.cpp:84-121`)
//! и `Survey_RenderMarkers` (`survey.cpp:123-155`).
//!
//! C++: `Access_UpdateMapStatus` (`access.cpp:762-768`) при добавлении покрытия
//! (`mode = true`) зовёт `Survey_SurveyArea(unit, 1)` для `SURVEYOR`/`MINELAYR`/
//! `SEAMNLYR`/`COMMANDO`. Внутри квадрата `grid ± 1` (3×3) клетки с
//! поверхностью `AIR` пропускаются, а **разведчик** ставит в
//! `ResourceManager_CargoMap` бит своей команды (`hash_team_id`): `RED = 0x2000`
//! (`enums.hpp:40-43`). Знание накопительное — `|=`, поэтому уход разведчика
//! его не снимает; иконки (`Survey_RenderMarkers`) показываются только на
//! помеченных клетках, а маска `GameManager_MaxSurvey` (чит `MAXSURVEY`) даёт
//! `0xFFFF`.

use max_core::access;
use max_core::enums::{Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::map::{ResourceKind, ResourceMapEntry};
use max_core::team::TeamId;
use max_core::team::TeamKind;
use max_core::GameState;

/// Партия: вся карта — суша, в каждой клетке 8 материалов, RED — человек,
/// GREEN — компьютер. `UNKNOWN` — ни одна команда не разведала.
fn land_state_with_resources() -> GameState {
    let mut state = GameState::new();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let _ = state.map.surface.set(x, y, Surface::LAND);
            state
                .map
                .resources
                .set(x, y, ResourceMapEntry::new(8, Some(ResourceKind::Raw), 0));
        }
    }

    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Computer;
    state.team_units[0] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    state.team_units[1] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);

    state
}

/// Разведана ли клетка командой (`ResourceManager_CargoMap & hash_team_id`).
fn known(state: &GameState, x: i16, y: i16, team: u8) -> bool {
    state.map.resources.get(x, y).is_visible_to(team)
}

#[test]
fn surveyor_marks_its_3x3_for_its_own_team_only() {
    let mut state = land_state_with_resources();
    // Клетка в квадрате разведки с поверхностью AIR — C++ её пропускает.
    let _ = state.map.surface.set(21, 21, Surface::AIR);

    let surveyor = state
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
        .expect("surveyor");

    // `spawn_unit` → `GameState::update_map_status(id, true)` — как
    // `Access_UpdateMapStatus` после `DeployUnit`.
    let _ = surveyor;

    for y in 19..=21 {
        for x in 19..=21 {
            if (x, y) == (20, 20) || (x, y) == (21, 21) {
                // Своя клетка разведчика разведана, AIR — пропущена.
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

    // Вне квадрата 3×3 знания нет.
    for (x, y) in [(22, 20), (18, 20), (20, 22), (20, 18)] {
        assert!(!known(&state, x, y, 0), "клетка ({x},{y}) вне 3×3");
    }

    // Бит RED — `HASH_TEAM_RED = 0x2000` (бит 13), а не младший бит.
    assert_eq!(
        state.map.resources.get(19, 19).visibility(),
        0b1000,
        "RED помечает клетку битом 13 (HASH_TEAM_RED)"
    );
    assert_eq!(
        state.map.resources.get(20, 20).bits() & 0x3C00,
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

    // Разведчик проехал на четыре клетки восточнее: `write_unit` пересчитывает
    // покрытие старой и новой клетки (`Access_UpdateMapStatus`).
    let mut unit = state.unit(surveyor).expect("surveyor").clone();
    unit.grid = Point::new(24, 20);
    state.write_unit(surveyor, unit);

    // Новое место разведано…
    for (x, y) in [(23, 19), (24, 20), (25, 21)] {
        assert!(known(&state, x, y, 0), "новая клетка ({x},{y})");
    }

    // …старое осталось разведанным (C++ `|=`, снятия нет).
    for (x, y) in [(19, 19), (20, 20), (21, 21)] {
        assert!(
            known(&state, x, y, 0),
            "прежняя клетка ({x},{y}) не снимается"
        );
    }

    // Промежуточные клетки маршрута не разведаны: C++ помечает только текущий
    // квадрат `grid ± 1`, телепорта по клеткам нет.
    assert!(
        !known(&state, 22, 20, 0),
        "клетка между позициями не разведана"
    );
}

#[test]
fn only_a_surveyor_marks_resources() {
    let mut state = land_state_with_resources();

    // `Access_UpdateMapStatus` зовёт `Survey_SurveyArea` и для мин/коммандо, но
    // **помечает** клетки только `SURVEYOR` (`survey.cpp:108-117`).
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
            state.map.resources.get(40, 40).visibility(),
            0,
            "{unit_type:?} не помечает ресурсы"
        );
        state.destroy_unit(id);
    }

    // А разведчик — помечает.
    let _ = state
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(40, 40), 0)
        .expect("surveyor");
    assert_eq!(state.map.resources.get(40, 40).visibility(), 0b1000);
}

#[test]
fn knowledge_is_part_of_the_state_and_deterministic() {
    fn run() -> GameState {
        let mut state = land_state_with_resources();
        let surveyor = state
            .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(30, 30), 0)
            .expect("surveyor");
        let mut unit = state.unit(surveyor).expect("surveyor").clone();
        unit.grid = Point::new(32, 30);
        state.write_unit(surveyor, unit);
        state
    }

    let first = run();
    let second = run();

    assert_eq!(
        first.map.resources.to_le_bytes(),
        second.map.resources.to_le_bytes(),
        "знание ресурсов — часть состояния и детерминировано"
    );
    assert_eq!(
        first.snapshot_bytes(),
        second.snapshot_bytes(),
        "снапшот включает карту ресурсов с битами знания"
    );

    // Снапшот/CRC меняются вместе со знанием: это не UI-поле.
    let mut learned = first.clone();
    let entry = learned.map.resources.get(50, 50).with_team_visible(0);
    learned.map.resources.set(50, 50, entry);
    assert_ne!(
        first.snapshot_bytes(),
        learned.snapshot_bytes(),
        "новое знание меняет снапшот (CRC/десинк-детектор)"
    );
}

/// Отрисовка иконок (`Survey_RenderMarkers`) — только для помеченных клеток;
/// маска команды берётся из `hash_team_id`.
#[test]
fn render_mask_matches_the_cpp_hash_team_id() {
    let mut state = land_state_with_resources();

    // RED помечает (10,10), GREEN — нет.
    let entry = state.map.resources.get(10, 10).with_team_visible(0);
    state.map.resources.set(10, 10, entry);

    let mask_red = max_core::map::team_visibility_bit(0) as u16;
    let mask_green = max_core::map::team_visibility_bit(1) as u16;

    assert_eq!(mask_red, 0b1000, "HASH_TEAM_RED = 0x2000 (бит 13)");
    assert_eq!(mask_green, 0b0100, "HASH_TEAM_GREEN = 0x1000 (бит 12)");
    assert!(state.map.resources.get(10, 10).is_visible_to(0));
    assert!(!state.map.resources.get(10, 10).is_visible_to(1));

    // `Survey_RenderMarkers` дополнительно требует `surface != AIR`.
    let _ = state.map.surface.set(10, 10, Surface::AIR);
    assert_eq!(
        access::modified_surface_type(&state, Point::new(10, 10), false),
        access::SURFACE_TYPE_AIR,
        "иконка на AIR-клетке не рисуется (проверка в рендере)"
    );

    // Разведчик, стоящий в `ORDER_DISABLE`, не обновляет знание
    // (`Access_UpdateMapStatus`: `if (unit->GetOrder() != ORDER_DISABLE)`).
    // Спавн в (60,60) разведал 3×3; переводим в `ORDER_DISABLE` и двигаем —
    // новая клетка обязана остаться неразведанной.
    let surveyor = state
        .spawn_unit(UnitType::Surveyor, TeamId::GREEN, Point::new(60, 60), 0)
        .expect("surveyor");
    assert!(known(&state, 60, 60, 1), "спавн разведал клетку");

    let mut unit = state.unit(surveyor).expect("surveyor").clone();
    unit.orders = UnitOrder::Disable;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.grid = Point::new(64, 64);
    state.write_unit(surveyor, unit);
    assert!(
        !known(&state, 64, 64, 1),
        "ORDER_DISABLE не обновляет карту ресурсов"
    );
}
