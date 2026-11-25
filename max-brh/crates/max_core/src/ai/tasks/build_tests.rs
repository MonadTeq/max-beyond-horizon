use super::*;
use crate::enums::{Surface, TeamClan};
use crate::map::Map;
use crate::team::TeamKind;
use crate::game::GameState;

fn land_map() -> Map {
    let mut map = Map::new();

    for y in 0..crate::geo::MAP_HEIGHT {
        for x in 0..crate::geo::MAP_WIDTH {
            map.surface.set(x, y, Surface::LAND);
        }
    }

    map
}

fn computer_state() -> GameState {
    let mut state = GameState::with_setup(
        land_map(),
        [
            TeamKind::Computer,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    for team in 0..2 {
        state.teams[team].team_type = TeamKind::Computer;
        state.teams[team].team_clan = TeamClan::Musashi;
    }
    crate::ai::ai_init(&mut state);
    state.ai.settings.opponent = crate::enums::Opponent::Expert as u8;

    state
}

fn manager_task(state: &mut GameState, site: Point) -> u32 {
    state.ai_append_task(
        TeamId::RED,
        None,
        TASK_PRIORITY_MANAGE_BUILDINGS,
        TaskKind::manage_buildings(site),
    )
}

fn with_manager<R>(
    state: &mut GameState,
    task_id: u32,
    f: impl FnOnce(&mut TaskKind, &mut TaskCtx<'_>, TaskHeader) -> R,
) -> R {
    state.with_task_manager(|task_manager, world| {
        let header = task_manager.task(task_id).expect("manager task").header();
        let mut kind = task_manager
            .task(task_id)
            .expect("manager task")
            .kind
            .clone();

        let result = {
            let mut ctx = TaskCtx::new(task_manager, world);
            f(&mut kind, &mut ctx, header)
        };

        task_manager.task_mut(task_id).expect("manager task").kind = kind;

        result
    })
}

#[test]
fn plan_next_build_job_starts_with_the_light_plant() {
    let mut state = computer_state();
    let site = Point::new(10, 10);

    // A base with one building: the C++ `if (units.GetCount())` guard.
    let mine = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, site, 0)
        .expect("mining station");
    let manager = manager_task(&mut state, site);

    let planned = with_manager(&mut state, manager, |kind, ctx, header| {
        if let TaskKind::ManageBuildings { units, .. } = kind {
            units.push(mine);
        }

        plan_next_build_job(kind, ctx, header)
    });

    assert!(planned, "the light plant must be the first factory");

    let create_types: Vec<UnitType> = state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter_map(|task| match &task.kind {
            TaskKind::CreateBuilding { unit_type, .. } => Some(*unit_type),
            _ => None,
        })
        .collect();

    assert_eq!(create_types, vec![UnitType::LightVehiclePlant]);
}

#[test]
fn plan_unit_production_rolls_a_factory_type_from_the_weight_table() {
    let mut state = computer_state();
    let site = Point::new(10, 10);

    let factory = state
        .spawn_unit(
            UnitType::LightVehiclePlant,
            TeamId::RED,
            Point::new(8, 8),
            0,
        )
        .expect("light plant");
    let mut unit = state.unit(factory).expect("light plant").clone();
    unit.orders = UnitOrder::PowerOn;
    state.write_unit(factory, unit);

    let complex = state
        .unit(factory)
        .and_then(|unit| unit.complex)
        .expect("factory complex");
    let complex = state.team_units[TeamId::RED.0 as usize]
        .get_complex_mut(complex)
        .expect("factory complex");
    complex.raw = 100;
    complex.fuel = 100;

    let manager = manager_task(&mut state, site);

    with_manager(&mut state, manager, |kind, ctx, header| {
        plan_unit_production(kind, ctx, header);
    });

    let obtain: Vec<UnitType> = state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter_map(|task| match &task.kind {
            TaskKind::ObtainUnits { units, .. } => units.first().copied(),
            _ => None,
        })
        .collect();

    assert_eq!(obtain.len(), 1, "one unit request per factory");
    let requested = obtain[0];
    assert_eq!(
        crate::production::builder_type(requested),
        Some(UnitType::LightVehiclePlant),
            "the rolled type must be buildable by the idle factory"
    );
    assert_eq!(
        crate::access::unit_type_flags(requested) & crate::enums::unit_flags::STATIONARY,
        0,
            "stationary types are filtered out (flags = 3)"
    );
}

#[test]
fn check_needs_orders_a_mining_station_for_a_negative_cargo_demand() {
    let mut state = computer_state();
    let site = Point::new(10, 10);
    let manager = manager_task(&mut state, site);

    // An empty manager: `PlanNextBuildJob` bails out on `units.GetCount()`,
    // and the cargo demand (no builders, no mines) is negative.
    let planned = with_manager(&mut state, manager, |kind, ctx, header| {
        check_needs(kind, ctx, header)
    });

    assert!(planned, "a negative demand must order a mining station");

    let create_types: Vec<UnitType> = state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter_map(|task| match &task.kind {
            TaskKind::CreateBuilding { unit_type, .. } => Some(*unit_type),
            _ => None,
        })
        .collect();

    assert_eq!(create_types, vec![UnitType::MiningStation]);
}

#[test]
fn plan_unit_production_needs_materials_and_fuel() {
    let mut state = computer_state();
    let site = Point::new(10, 10);

    let factory = state
        .spawn_unit(
            UnitType::LightVehiclePlant,
            TeamId::RED,
            Point::new(8, 8),
            0,
        )
        .expect("light plant");
    let mut unit = state.unit(factory).expect("light plant").clone();
    unit.orders = UnitOrder::PowerOn;
    state.write_unit(factory, unit);

    let complex = state
        .unit(factory)
        .and_then(|unit| unit.complex)
        .expect("factory complex");
    let complex = state.team_units[TeamId::RED.0 as usize]
        .get_complex_mut(complex)
        .expect("factory complex");
    complex.raw = 10;
    complex.fuel = 10;

    let manager = manager_task(&mut state, site);

    with_manager(&mut state, manager, |kind, ctx, header| {
        plan_unit_production(kind, ctx, header);
    });

    assert!(
        !state
            .ai
            .task_manager
            .tasks()
            .iter()
            .any(|task| matches!(task.kind, TaskKind::ObtainUnits { .. })),
            "the C++ threshold is material > 10 and fuel > 10"
    );
}
