//! AI construction chain — regression tests for the "the AI builds nothing" and
//! "the AI built a radar on top of the light vehicle plant" defects.
//!
//! # What broke and why these tests exist
//!
//! 1. A finished building never reached the base manager
//!    (`TaskManageBuildings::units`): the port's `TaskCreateBuilding::Finish`
//!    skipped the oracle's `AddUnit(*building)` handover
//!    (`taskcreatebuilding.cpp:326-334`), and `AiPlayer::AddBuilding` runs only in
//!    `need_init` (`aiplayer.cpp:2343-2348`). `PlanNextBuildJob` returns at once
//!    while that list is empty (`taskmanagebuildings.cpp:1011`), so an AI base
//!    finished its first mining station and then planned **no factory at all** —
//!    every strategy lives behind that guard.
//! 2. `CheckMaterials` compared the team's **complex** stock with a fixed 10
//!    instead of the builder's own cargo against
//!    `turns_to_build * Cargo_GetRawConsumptionRate(builder, 1)`
//!    (`taskcreatebuilding.cpp:1033-1086`). A base whose complex was empty (or
//!    whose dump was full while the builder's cargo was not) kept the only
//!    builder in `CREATE_BUILDING_STATE_GETTING_MATERIALS` forever: it was
//!    attached to the stalled task, so no other construction could start. The
//!    oracle sends the builder for material (`TaskGetMaterials`) in that state —
//!    the port does the same now.
//! 3. `find_build_site` checked only accessibility, never the cell hash, and two
//!    plans of one turn could pick the same cells — the second building was later
//!    laid **on top of** the first (the manual "radar over the light vehicle
//!    plant"). The site search now skips occupied cells and the footprints of the
//!    base's other plans (`ClearBuildingAreas` + `ClearPlannedBuildings`,
//!    `taskmanagebuildings.cpp:261-374`), and the construction start re-checks
//!    the site before the build order is issued.
//!
//! The scenarios use the real turn machine (`next_team` → economy + `Ai_BeginTurn`
//! → packet 06 → `finish_turn`), see `support/phase5.rs`.

mod support;

use max_core::ai::TaskKind;
use max_core::enums::{AiStrategy, Opponent, UnitOrder, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::universe::GameUniverse;
use support::phase5::{
    count_units, green_base, play_rounds, play_rounds_observed, put_ore, red_base, spawn,
    strategy_state, AiObservation, ROUND_TICKS,
};

/// Buildings the AI can add to a base (the strategy factories and the support
/// buildings of `PlanNextBuildJob`).
const BUILDABLE: [UnitType; 12] = [
    UnitType::LightVehiclePlant,
    UnitType::HeavyVehiclePlant,
    UnitType::AirUnitsPlant,
    UnitType::Shipyard,
    UnitType::TrainingHall,
    UnitType::MiningStation,
    UnitType::Habitat,
    UnitType::EcoSphere,
    UnitType::ResearchCentre,
    UnitType::Radar,
    UnitType::Depot,
    UnitType::PowerGenerator,
];

/// A connected, powered AI base: power station → fuel dump → mining station →
/// material dump (all 2×2 and adjacent, one complex), ore under the mine, and a
/// builder carrying raw material. This is the shape of a mission's starting base
/// and the one the manual playtest used.
fn powered_base(strategy: AiStrategy, opponent: Opponent) -> GameUniverse {
    let mut state = strategy_state(strategy, opponent);
    red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    put_ore(&mut state, Point::new(8, 8), 40);
    put_ore(&mut state, Point::new(9, 8), 40);

    let station = spawn(&mut state, UnitType::PowerStation, TeamId::RED, Point::new(8, 4));
    state.unit_mut(station).expect("station").orders = UnitOrder::PowerOn;

    let fuel = spawn(&mut state, UnitType::FuelStorage, TeamId::RED, Point::new(8, 6));
    state.unit_mut(fuel).expect("fuel dump").storage = 120;

    let mine = spawn(
        &mut state,
        UnitType::MiningStation,
        TeamId::RED,
        Point::new(8, 8),
    );
    state.unit_mut(mine).expect("mine").orders = UnitOrder::PowerOn;

    let material = spawn(
        &mut state,
        UnitType::MaterialStorage,
        TeamId::RED,
        Point::new(10, 8),
    );
    state.unit_mut(material).expect("material dump").storage = 0;

    let builder = spawn(
        &mut state,
        UnitType::Constructor,
        TeamId::RED,
        Point::new(12, 8),
    );
    state.unit_mut(builder).expect("builder").storage = 60;

    state
}

/// Counts the base's buildings of the types the AI plans.
fn built(universe: &GameUniverse) -> Vec<UnitType> {
    let state = universe.snapshot();
    let mut types: Vec<UnitType> = state
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.unit(id))
        .filter(|unit| unit.team == TeamId::RED && BUILDABLE.contains(&unit.unit_type))
        .map(|unit| unit.unit_type)
        .collect();

    types.sort_by_key(|unit_type| *unit_type as u16);
    types.dedup();
    types
}

/// `TaskManageBuildings::units` of the team — the list `PlanNextBuildJob` needs.
fn managed_buildings(universe: &GameUniverse) -> usize {
    let state = universe.snapshot();
    state
        .ai()
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == TeamId::RED)
        .filter_map(|task| match &task.kind {
            TaskKind::ManageBuildings { units, .. } => Some(units.len()),
            _ => None,
        })
        .sum()
}

/// Regression 1: the base manager learns about a completed building, so the
/// strategy branch of `PlanNextBuildJob` runs and the AI adds a factory.
///
/// Before the fix the manager's `units` stayed empty forever (`Finish` did not
/// hand the building over) and no strategy ever planned anything.
#[test]
fn ai_registers_finished_buildings_and_builds_a_factory() {
    let mut state = powered_base(AiStrategy::TankHorde, Opponent::Average);
    let (_, over) = play_rounds(&mut state, 20, ROUND_TICKS);
    assert!(!over, "the match must stay open");

    assert!(
        managed_buildings(&state) >= 2,
        "the manager must own the started base and the buildings it finished, \
         owned: {}",
        managed_buildings(&state)
    );

    let types = built(&state);
    assert!(
        types.contains(&UnitType::LightVehiclePlant),
        "every strategy starts with a light vehicle plant, built: {types:?}"
    );

    assert!(
        count_units(&state, TeamId::RED, UnitType::MiningStation) >= 2,
        "the AI must keep expanding its mines, built: {types:?}"
    );
}

/// Regression 2: a builder with an empty cargo refills it from the base instead
/// of blocking the whole base.
///
/// The scenario starts with a full material dump and a builder that can pay for
/// the first construction but not for the next one: the oracle's
/// `TaskGetMaterials` (`taskcreatebuilding.cpp:1070-1078`) sends the builder for
/// material. The previous port never created that task, so the builder stayed in
/// `CREATE_BUILDING_STATE_GETTING_MATERIALS` with the dump full next to it.
#[test]
fn ai_fetches_material_from_the_base_and_keeps_building() {
    let mut state = powered_base(AiStrategy::TankHorde, Opponent::Average);

    // The dump starts filling: the mine produces into it, the builder must take
    // material from it once its own cargo runs out.
    {
        let snapshot = state.snapshot();
        let dump = snapshot
            .unit_ids_in_order()
            .into_iter()
            .find(|id| {
                snapshot
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::MaterialStorage)
            })
            .expect("material dump");
        state.unit_mut(dump).expect("dump").storage = 40;
    }

    let mut observation = AiObservation::default();
    let mut fetch_seen = false;

    for _ in 0..24 {
        let (_, over) =
            play_rounds_observed(&mut state, 1, ROUND_TICKS, TeamId::RED, &mut observation);
        assert!(!over, "the match must stay open");

        let snapshot = state.snapshot();
        fetch_seen |= snapshot
            .ai()
            .task_manager
            .tasks()
            .iter()
            .any(|task| matches!(task.kind, TaskKind::GetMaterials { .. }));
    }

    assert!(
        fetch_seen,
        "the builder must be sent for raw material once its own cargo runs low"
    );

    let types = built(&state);
    assert!(
        types.contains(&UnitType::LightVehiclePlant),
        "the base must keep growing, built: {types:?}"
    );
    assert!(
        types.contains(&UnitType::Habitat) || types.contains(&UnitType::HeavyVehiclePlant),
        "a powered base also adds its support block, built: {types:?}"
    );
}

/// Regression 3: no two plans of the base share or overlap cells, and no
/// construction starts on a cell that is already taken.
///
/// This is the "radar on top of the light vehicle plant" defect: the site search
/// returned an occupied cell (it checked accessibility only) and the construction
/// start did not re-validate the site.
#[test]
fn ai_never_plans_two_buildings_on_the_same_cells() {
    let mut state = powered_base(AiStrategy::CombinedArms, Opponent::Expert);
    let (_, over) = play_rounds(&mut state, 28, ROUND_TICKS);
    assert!(!over);

    let state_ref = state.snapshot();

    // The footprints of the planned constructions.
    let planned: Vec<(UnitType, Point, i16)> = state_ref
        .ai()
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == TeamId::RED)
        .filter_map(|task| match &task.kind {
            TaskKind::CreateBuilding { unit_type, site, .. } => {
                Some((*unit_type, *site, max_core::production::footprint(*unit_type)))
            }
            _ => None,
        })
        .collect();

    let overlaps = |first: (Point, i16), second: (Point, i16)| {
        first.0.x < second.0.x + second.1
            && second.0.x < first.0.x + first.1
            && first.0.y < second.0.y + second.1
            && second.0.y < first.0.y + first.1
    };

    for (index, (first_type, first_site, first_size)) in planned.iter().enumerate() {
        for (second_type, second_site, second_size) in planned.iter().skip(index + 1) {
            assert!(
                !overlaps((*first_site, *first_size), (*second_site, *second_size)),
                "{first_type:?} at {first_site:?} overlaps the plan \
                 {second_type:?} at {second_site:?}"
            );
        }

        // A plan may stand on its own construction utilities (the site is being
        // prepared there) but never on another building.
        for id in state_ref.unit_ids_in_order() {
            let Some(unit) = state_ref.unit(id) else {
                continue;
            };

            if unit.team != TeamId::RED || !BUILDABLE.contains(&unit.unit_type) {
                continue;
            }

            let unit_size = max_core::production::footprint(unit.unit_type);

            assert!(
                !overlaps((*first_site, *first_size), (unit.grid, unit_size)),
                "the plan {first_type:?} at {first_site:?} overlaps the built \
                 {:?} at {:?}",
                unit.unit_type,
                unit.grid
            );
        }
    }
}

/// A base whose only builder carries material but whose complex stock is zero
/// (an unpowered mine, no dump income) must still build: `CheckMaterials` is
/// about the builder's cargo, not the complex stock.
///
/// The previous port compared the complexes' raw with a fixed 10, so the builder
/// waited in `GETTING_MATERIALS` forever and the whole base stopped after its
/// first mining station — the manual "the AI builds nothing" symptom.
#[test]
fn a_builder_with_its_own_material_builds_without_a_complex_stock() {
    let mut state = strategy_state(AiStrategy::TankHorde, Opponent::Average);
    red_base(&mut state, Point::new(8, 8), 500);
    green_base(&mut state, Point::new(32, 32));

    // A complex with **zero** material: the mine is switched off (no power) and
    // the dump is empty, so `Complex::raw` stays 0 for the whole match.
    put_ore(&mut state, Point::new(8, 8), 40);
    let mine = spawn(
        &mut state,
        UnitType::MiningStation,
        TeamId::RED,
        Point::new(8, 8),
    );
    state.unit_mut(mine).expect("mine").orders = UnitOrder::PowerOff;
    let dump = spawn(
        &mut state,
        UnitType::MaterialStorage,
        TeamId::RED,
        Point::new(10, 8),
    );
    state.unit_mut(dump).expect("dump").storage = 0;

    let builder = spawn(
        &mut state,
        UnitType::Constructor,
        TeamId::RED,
        Point::new(12, 8),
    );
    state.unit_mut(builder).expect("builder").storage = 60;

    let mut observation = AiObservation::default();
    let (_, over) =
        play_rounds_observed(&mut state, 16, ROUND_TICKS, TeamId::RED, &mut observation);
    assert!(!over);

    assert!(
        observation.saw_constructor_build_order,
        "the builder must receive ORDER_BUILD from its own cargo"
    );

    let types = built(&state);
    assert!(
        types.contains(&UnitType::LightVehiclePlant),
        "the base must build its first factory despite the empty complex, built: {types:?}"
    );
}
