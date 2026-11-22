//! Construction cancellation is explicit only (`stop`); moving the builder away by an order does not cancel it.
//!
//! Oracle: `game_manager.cpp` `GameManager_GetBuilderUnitCursor` (`:5829-5935`)
//! and `GameManager_SelectBuildSite` (`:3529`), `units_manager.cpp`
//! `UnitsManager_Popup_OnClick_BuildStop` → `UnitsManager_SetNewOrder(unit,
//! ORDER_BUILD, ORDER_STATE_BUILD_CANCEL)` → `UnitInfo::CancelBuilding`
//! (`unitinfo.cpp:4231`, the `!STATIONARY` branch).
//!
//! In C++ a map movement order is not issued to a builder in `BUILD_IN_PROGRESS`
//! at all: for all states except `SELECT_SITE` and `UNIT_READY`,
//! `GameManager_GetBuilderUnitCursor` returns `CURSOR_UNIT_NO_GO`
//! (`:5927-5931`). An order arriving another way (packet 08/38, AI, group)
//! **does not cancel** construction: `CancelBuilding` is called only from
//! `UnitsManager_ProcessOrderHaltBuilding` on an explicit `stop`
//! (`units_manager.cpp:3142-3148`). The port repeats this behavior — the former
//! "auto-cancel on move-away" contradicted the oracle.
//!
//! A completed site (`ORDER_STATE_UNIT_READY`) is a separate case: the building is
//! already complete, the exit is allowed (`ORDER_ACTIVATE`) and does not cancel construction.

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::events::apply_command;
use max_core::geo::Point;
use max_core::input::{Command, Packet38Move};
use max_core::production::{self, ProductionManager};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::{TeamClan, UnitId};

fn state_with_gold(gold: u32) -> max_core::GameState {
    let mut state = max_core::GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[0].set_gold(gold);
    state
}

fn count_type(state: &max_core::GameState, unit_type: UnitType) -> usize {
    state
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == unit_type)
        })
        .count()
}

/// `TRAINHAL` order by the constructor: the site stands, the order is `ORDER_BUILD`.
fn start_training_hall_build(state: &mut max_core::GameState) -> (UnitId, Point) {
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    // Landing gives the constructor raw material (`DEFAULT_MISSION_LOADOUT`, 40);
    // `UnitsManager_DeployUnit` zeroes the new unit's storage.
    state.unit_mut(constructor).expect("constructor").storage = 40;

    let mut manager = ProductionManager::new(state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("constructor builds training halls");
    let report = manager.commit(constructor).expect("site found");
    drop(manager);

    (constructor, report.site)
}

/// Signs of a **live** construction: queue, progress and site utilities are in place.
fn assert_build_intact(state: &max_core::GameState, builder: UnitId, site: Point) {
    let unit = state.unit(builder).expect("builder alive");

    assert_eq!(
        unit.build_list,
        vec![UnitType::TrainingHall],
        "очередь стройки не тронута"
    );
    assert!(unit.build_time > 0, "прогресс стройки сохранён");
    assert!(
        production::construction_utility(state, TeamId::RED, site).is_some(),
        "лента площадки на месте"
    );
}

/// Signs of a cancelled construction (C++ `CancelBuilding` mobile branch).
fn assert_build_cancelled(state: &max_core::GameState, builder: UnitId, site: Point) {
    let unit = state.unit(builder).expect("builder alive");

    assert!(unit.build_list.is_empty(), "очередь стройки очищена");
    assert_eq!(unit.build_time, 0, "прогресс сброшен");
    assert!(
        unit.path.is_none(),
        "путь к площадке снят (`path = nullptr`)"
    );
    assert!(
        production::construction_utility(state, TeamId::RED, site).is_none(),
        "лента площадки снята с карты"
    );
    for id in state.units_at(site).iter().copied() {
        let unit_type = state.unit(id).map(|unit| unit.unit_type);
        assert!(
            !matches!(
                unit_type,
                Some(UnitType::LargeTape)
                    | Some(UnitType::SmallTape)
                    | Some(UnitType::LargeCones)
                    | Some(UnitType::SmallCones)
                    | Some(UnitType::LargeSlab)
                    | Some(UnitType::SmallSlab)
            ),
            "утилиты площадки должны быть удалены, осталось {unit_type:?}"
        );
    }
}

/// `UnitsManager_SetNewOrder` (AI, group, direct call) does not cancel construction.
#[test]
fn set_order_move_does_not_cancel_the_unfinished_build() {
    let mut state = state_with_gold(100);
    let (builder, site) = start_training_hall_build(&mut state);

    state.set_order(builder, UnitOrder::Move, UnitOrderState::InProgress);

    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(
        (unit.orders, unit.state),
        (UnitOrder::Move, UnitOrderState::InProgress),
        "приказ применён, но стройка не снята"
    );
    assert_build_intact(&state, builder, site);
}

/// Packet **38** (movement with a path) — the network path of a player order.
#[test]
fn packet_38_move_does_not_cancel_the_unfinished_build() {
    let mut state = state_with_gold(100);
    let (builder, site) = start_training_hall_build(&mut state);
    let grid = state.unit(builder).expect("builder").grid;

    let packet = Packet38Move {
        unit: builder,
        order: UnitOrder::Move,
        state: UnitOrderState::InProgress,
        grid,
        move_to: Point::new(20, 20),
        group_speed: 0,
        path: None,
    };
    apply_command(&mut state, &Command::Move(packet));

    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(
        (unit.orders, unit.state),
        (UnitOrder::Move, UnitOrderState::InProgress)
    );
    assert_eq!(unit.target_grid, Point::new(20, 20));
    assert_build_intact(&state, builder, site);
}

/// Packet **08** (`UnitsManager_SetNewOrder` of a remote player): `prior` from
/// the packet is preserved, construction is not cancelled.
#[test]
fn packet_08_move_does_not_cancel_the_unfinished_build() {
    use max_core::input::{OrderPayload, Packet08Order, PayloadP1};

    let mut state = state_with_gold(100);
    let (builder, site) = start_training_hall_build(&mut state);

    let order = Packet08Order {
        unit: builder,
        payload: OrderPayload::P1(PayloadP1 {
            order: UnitOrder::Move,
            state: UnitOrderState::Init,
            prior_order: UnitOrder::Build,
            prior_state: UnitOrderState::BuildInProgress,
            disabled_reaction_fire: false,
        }),
    };
    apply_command(&mut state, &Command::Order(order));

    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(
        (unit.orders, unit.state),
        (UnitOrder::Move, UnitOrderState::Init)
    );
    assert_eq!(unit.prior_orders, UnitOrder::Build);
    assert_eq!(unit.prior_state, UnitOrderState::BuildInProgress);
    assert_build_intact(&state, builder, site);
}

/// A completed site: the builder's exit (`UNIT_READY` → `ORDER_ACTIVATE` →
/// `AWAIT_TAPE_POSITIONING`/`TAPE_POSITIONING_DEINIT`) does **not** cancel construction
/// and does not remove the slab under the completed building.
#[test]
fn leaving_a_finished_site_keeps_the_foundation() {
    let mut state = state_with_gold(100);
    let (builder, site) = start_training_hall_build(&mut state);

    for _ in 0..10 {
        state.manage_economy(TeamId::RED);
        production::progress_build_state(&mut state, TeamId::RED);
    }

    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(
        (unit.orders, unit.state),
        (UnitOrder::Build, UnitOrderState::UnitReady)
    );
    assert!(unit.build_list.is_empty(), "очередь очищена достройкой");

    let slab_at_site = |state: &max_core::GameState| {
        state.units_at(site).iter().any(|id| {
            state.unit(*id).is_some_and(|unit| {
                matches!(unit.unit_type, UnitType::LargeSlab | UnitType::SmallSlab)
            })
        })
    };
    assert!(slab_at_site(&state), "плита под зданием стоит");

    // Exit order: `ORDER_ACTIVATE`/`IN_TRANSITION` → `PositionInTape` LEAVE.
    state.set_order(builder, UnitOrder::Activate, UnitOrderState::InTransition);
    state.tick();
    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(unit.orders, UnitOrder::AwaitTapePositioning);
    assert_eq!(unit.state, UnitOrderState::TapePositioningDeinit);

    // A new order during the exit does not "cancel construction".
    state.set_order(builder, UnitOrder::Move, UnitOrderState::Init);
    assert!(
        slab_at_site(&state),
        "плита не должна сниматься при уходе с готовой площадки"
    );
    assert_eq!(
        count_type(&state, UnitType::TrainingHall),
        1,
        "здание остаётся на карте"
    );
}

/// An explicit `stop` (`ORDER_BUILD`/`BUILD_CANCEL`) goes through the normal order
/// machine path: `ProcessOrderHaltBuilding` → `CancelBuilding` removes the site
/// exactly once.
#[test]
fn explicit_stop_cancels_the_build() {
    let mut state = state_with_gold(100);
    let (builder, site) = start_training_hall_build(&mut state);

    // `UnitsManager_Popup_OnClick_BuildStop` → `GameManager_SelectBuildSite`
    // → `UnitsManager_SetNewOrder(unit, ORDER_BUILD, BUILD_CANCEL)`.
    state.set_order(builder, UnitOrder::Build, UnitOrderState::BuildCancel);
    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(unit.orders, UnitOrder::Build);
    assert_eq!(unit.state, UnitOrderState::BuildCancel);

    // Order machine: `ProcessOrderBuild`/`BUILD_CANCEL` → `CancelBuilding`.
    for _ in 0..4 {
        state.tick();
    }

    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(
        (unit.orders, unit.state),
        (UnitOrder::Await, UnitOrderState::ExecutingOrder)
    );
    assert_build_cancelled(&state, builder, site);
}
