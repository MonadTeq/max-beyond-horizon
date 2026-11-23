//! Rubble: the death of a ground unit leaves `SMLRUBLE`/`LRGRUBLE` — 1:1 C++.
//!
//! Oracle `UnitsManager_BuildClearing` (`units_manager.cpp:3646-3805`):
//! * rubble is created only with `mode = true` (the unit was destroyed), not for
//!   `COMMANDO`/`INFANTRY`, not for `CONNECTOR_UNIT`/`HOVERING`, and only on
//!   a **fully dry** cell (`Access_IsFullyLandCovered`, `access.cpp:1049`);
//! * type: `LRGRUBLE` for `BUILDING`, otherwise `SMLRUBLE`;
//! * rubble cargo — `GetNormalRateBuildCost() / 2` (+ `storage` for raw material);
//! * frame — `Randomizer_Generate(image_count)`;
//! * mines leave no rubble (`units_manager.cpp:3720`);
//! * called from `UnitsManager_ProgressExplosion` (`units_manager.cpp:4021`) at
//!   the middle of the explosion animation.

use max_core::enums::{unit_flags, Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{Point, MAP_WIDTH};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::GameUniverse;
use max_core::TeamClan;
use std::collections::BTreeMap;

fn state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.set_play_mode(max_core::enums::PlayMode::TurnBased);
    state.set_game_state(max_core::enums::GameStateType::InGame);
    state.teams_mut()[0].team_type = TeamKind::Player;
    state.teams_mut()[1].team_type = TeamKind::Computer;
    state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);
    state.ai_mut().all_visible = true;

    // The whole map is land: `Access_IsFullyLandCovered` requires `SURFACE_TYPE_LAND`.
    for tile in state.map_mut().surface.tiles_mut().iter_mut() {
        *tile = Surface::LAND;
    }

    state
}

fn spawn(
    state: &mut GameUniverse,
    unit_type: UnitType,
    team: TeamId,
    grid: Point,
) -> max_core::UnitId {
    state.spawn_unit(unit_type, team, grid, 0).expect("spawn")
}

/// Kill the unit: `hits = 0` + `ORDER_EXPLODE`/`STATE_EXPLODE` (as
/// `UnitsManager_ProcessOrderExplode`, `units_manager.cpp:2972`).
fn kill(state: &mut GameUniverse, id: max_core::UnitId) {
    let mut unit = state.snapshot().unit(id).expect("unit").clone();
    unit.hits = 0;
    state.write_unit(id, unit);
    state.set_order(id, UnitOrder::Explode, UnitOrderState::Explode);
}

fn tick_until<F>(state: &mut GameUniverse, done: F) -> bool
where
    F: Fn(&GameUniverse) -> bool,
{
    for _ in 0..120 {
        state.tick();

        if done(state) {
            return true;
        }
    }

    false
}

fn units_of(state: &GameUniverse, unit_type: UnitType) -> Vec<max_core::UnitId> {
    state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
                .snapshot()
                .unit(*id)
                .map(|unit| unit.unit_type == unit_type)
                .unwrap_or(false)
        })
        .collect()
}

#[test]
fn destroyed_ground_unit_leaves_small_rubble() {
    let mut state = state();
    let tank = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    state.update_visibility_status(false);

    kill(&mut state, tank);

    assert!(
        tick_until(&mut state, |state| !units_of(state, UnitType::SmallRubble)
            .is_empty()),
        "обломок должен появиться после половины анимации взрыва"
    );

    let rubble = state
        .snapshot()
        .unit(units_of(&state, UnitType::SmallRubble)[0])
        .expect("rubble");

    assert_eq!(rubble.grid, Point::new(10, 10), "обломок на месте гибели");
    assert_eq!(rubble.team, TeamId::RED, "владелец — команда погибшего");
    assert_eq!(
        i32::from(rubble.storage),
        max_core::victory::get_normal_rate_build_cost(
            &state.snapshot().unit(tank).cloned().unwrap_or_else(|| {
                let mut unit = max_core::UnitInfo::new(UnitType::Tank, TeamId::RED, tank);
                unit.flags = unit_flags::MOBILE_LAND_UNIT;
                unit
            })
        ) / 2,
        "груз обломка — половина стоимости постройки (GetNormalRateBuildCost / 2)"
    );
    assert!(
        (0..5).contains(&rubble.angle),
        "кадр обломка из Randomizer_Generate(SMLRUBLE.image_count = 5), получено {}",
        rubble.angle
    );
}

#[test]
fn infantry_leaves_no_rubble() {
    let mut state = state();
    let infantry = spawn(
        &mut state,
        UnitType::Infantry,
        TeamId::RED,
        Point::new(12, 12),
    );
    state.update_visibility_status(false);
    kill(&mut state, infantry);

    assert!(
        tick_until(&mut state, |state| state
            .snapshot()
            .unit(infantry)
            .is_none()),
        "пехота исчезает после своей анимации"
    );
    assert!(
        units_of(&state, UnitType::SmallRubble).is_empty(),
        "C++ `BuildClearing`: INFANTRY обломков не оставляет"
    );
}

#[test]
fn destroyed_building_leaves_large_rubble() {
    let mut state = state();
    let building = spawn(
        &mut state,
        UnitType::TrainingHall,
        TeamId::RED,
        Point::new(20, 20),
    );
    state.update_visibility_status(false);
    kill(&mut state, building);

    assert!(
        tick_until(&mut state, |state| !units_of(state, UnitType::LargeRubble)
            .is_empty()),
        "здание оставляет большой обломок"
    );
    assert!(
        units_of(&state, UnitType::SmallRubble).is_empty(),
        "маленького обломка быть не должно"
    );
    assert!(
        state.snapshot().unit(building).is_none(),
        "само здание уничтожено"
    );
}

#[test]
fn sea_unit_on_water_leaves_no_rubble() {
    let mut state = state();
    let boat = spawn(
        &mut state,
        UnitType::Gunboat,
        TeamId::RED,
        Point::new(30, 30),
    );

    for y in 29..=31 {
        for x in 29..=31 {
            let index = y * MAP_WIDTH as i16 + x;
            state.map_mut().surface.tiles_mut()[index as usize] = Surface::WATER;
        }
    }

    state.update_visibility_status(false);
    kill(&mut state, boat);

    assert!(
        tick_until(&mut state, |state| state.snapshot().unit(boat).is_none()),
        "корабль исчезает"
    );
    assert!(
        units_of(&state, UnitType::SmallRubble).is_empty(),
        "`Access_IsFullyLandCovered` ложно: на воде обломков нет"
    );
}

#[test]
fn quick_build_removal_does_not_leave_rubble() {
    // `UnitsManager_BuildClearing(unit, false)` — the unit was removed, not destroyed.
    let mut state = state();
    let tank = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(14, 14));

    let rubble = state.build_clearing(tank, false);

    assert!(rubble.is_none(), "mode = false — обломка нет");
    assert!(
        state.snapshot().unit(tank).is_none(),
        "юнит всё равно убран"
    );
    assert!(units_of(&state, UnitType::SmallRubble).is_empty());
}

#[test]
fn rubble_is_deterministic_between_runs() {
    fn run() -> (BTreeMap<String, u8>, Vec<u8>) {
        let mut state = state();
        let tank = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
        let infantry = spawn(
            &mut state,
            UnitType::Infantry,
            TeamId::RED,
            Point::new(11, 10),
        );
        state.update_visibility_status(false);
        kill(&mut state, tank);
        kill(&mut state, infantry);
        assert!(tick_until(&mut state, |state| !units_of(
            state,
            UnitType::SmallRubble
        )
        .is_empty()));

        let mut rubble = BTreeMap::new();

        for id in units_of(&state, UnitType::SmallRubble) {
            let unit = state.snapshot().unit(id).expect("rubble");
            rubble.insert(
                format!("{id:?}@{},{}", unit.grid.x, unit.grid.y),
                unit.angle,
            );
        }

        let digest = state.snapshot().replay_digest_bytes();

        (rubble, digest)
    }

    let (first, first_digest) = run();
    let (second, second_digest) = run();

    assert_eq!(first, second, "кадры обломков не меняются между партиями");
    assert_eq!(
        first_digest, second_digest,
        "дайджест состояния детерминирован"
    );
}

#[test]
fn bulldozer_clears_rubble_and_takes_its_cargo() {
    let mut state = state();
    let rubble = spawn(
        &mut state,
        UnitType::SmallRubble,
        TeamId::RED,
        Point::new(16, 16),
    );
    let bulldozer = spawn(
        &mut state,
        UnitType::Bulldozer,
        TeamId::RED,
        Point::new(16, 16),
    );

    // `UnitsManager_Popup_OnClick_StopRemove` (`units_manager.cpp:1092-1113`):
    // `bulldozer->SetParent(rubble)`, `build_time = 1` (4 for a building),
    // `ORDER_CLEAR`/`INIT`.
    {
        let mut unit = state.snapshot().unit(bulldozer).expect("bulldozer").clone();
        unit.parent_unit = Some(rubble);
        unit.build_time = 1;
        unit.storage = 3;
        unit.orders = UnitOrder::Clear;
        unit.state = UnitOrderState::Init;
        state.write_unit(bulldozer, unit);
    }
    {
        let mut unit = state.snapshot().unit(rubble).expect("rubble").clone();
        unit.storage = 7;
        state.write_unit(rubble, unit);
    }

    // `GameManager_SpawnNewUnits` → `UnitInfo::SpawnNewUnit` (BULLDOZR branch).
    state.progress_build_state(TeamId::RED);

    assert!(state.snapshot().unit(rubble).is_none(), "обломок снесён");
    let unit = state.snapshot().unit(bulldozer).expect("bulldozer");
    assert_eq!(
        (unit.orders, unit.state),
        (UnitOrder::Await, UnitOrderState::ExecutingOrder),
        "бульдозер вернулся в AWAIT/EXECUTING_ORDER"
    );
    assert_eq!(unit.build_time, 0);
    assert_eq!(
        unit.storage, 10,
        "груз обломка (7) перешёл бульдозеру (3 + 7)"
    );
    assert!(unit.parent_unit.is_none(), "родитель сброшен");
}
