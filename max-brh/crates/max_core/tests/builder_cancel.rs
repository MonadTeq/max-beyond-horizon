//! Отмена стройки — только явная (`stop`), увод строителя приказом её не снимает.
//!
//! Оракул: `game_manager.cpp` `GameManager_GetBuilderUnitCursor` (`:5829-5935`)
//! и `GameManager_SelectBuildSite` (`:3529`), `units_manager.cpp`
//! `UnitsManager_Popup_OnClick_BuildStop` → `UnitsManager_SetNewOrder(unit,
//! ORDER_BUILD, ORDER_STATE_BUILD_CANCEL)` → `UnitInfo::CancelBuilding`
//! (`unitinfo.cpp:4231`, ветка `!STATIONARY`).
//!
//! В C++ приказ движения по карте строителю в `BUILD_IN_PROGRESS` не отдаётся
//! вовсе: для всех состояний, кроме `SELECT_SITE` и `UNIT_READY`,
//! `GameManager_GetBuilderUnitCursor` возвращает `CURSOR_UNIT_NO_GO`
//! (`:5927-5931`). Приказ, пришедший другим путём (пакет 08/38, ИИ, группа),
//! стройку **не отменяет**: `CancelBuilding` вызывается только из
//! `UnitsManager_ProcessOrderHaltBuilding` по явному `stop`
//! (`units_manager.cpp:3142-3148`). Порт повторяет это поведение — прежняя
//! «автоотмена при уводе» противоречила оракулу.
//!
//! Готовая площадка (`ORDER_STATE_UNIT_READY`) — отдельный случай: здание уже
//! достроено, уход разрешён (`ORDER_ACTIVATE`) и стройку не отменяет.

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

/// Заказ `TRAINHAL` конструктором: площадка стоит, приказ — `ORDER_BUILD`.
fn start_training_hall_build(state: &mut max_core::GameState) -> (UnitId, Point) {
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    // Высадка даёт конструктору сырьё (`DEFAULT_MISSION_LOADOUT`, 40);
    // `UnitsManager_DeployUnit` обнуляет склад нового юнита.
    state.unit_mut(constructor).expect("constructor").storage = 40;

    let mut manager = ProductionManager::new(state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("constructor builds training halls");
    let report = manager.commit(constructor).expect("site found");
    drop(manager);

    (constructor, report.site)
}

/// Признаки **живой** стройки: очередь, прогресс и утилиты площадки на месте.
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

/// Признаки отменённой стройки (C++ `CancelBuilding` мобильной ветки).
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

/// `UnitsManager_SetNewOrder` (ИИ, группа, прямой вызов) стройку не отменяет.
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

/// Пакет **38** (движение с путём) — сетевой путь приказа игрока.
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

/// Пакет **08** (`UnitsManager_SetNewOrder` удалённого игрока): `prior` из
/// пакета сохраняется, стройка не снимается.
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

/// Достроенная площадка: уход строителя (`UNIT_READY` → `ORDER_ACTIVATE` →
/// `AWAIT_TAPE_POSITIONING`/`TAPE_POSITIONING_DEINIT`) **не** отменяет стройку
/// и не снимает плиту под готовым зданием.
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

    // Приказ уйти: `ORDER_ACTIVATE`/`IN_TRANSITION` → `PositionInTape` LEAVE.
    state.set_order(builder, UnitOrder::Activate, UnitOrderState::InTransition);
    state.tick();
    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(unit.orders, UnitOrder::AwaitTapePositioning);
    assert_eq!(unit.state, UnitOrderState::TapePositioningDeinit);

    // Новый приказ во время ухода не «отменяет стройку».
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

/// Явный `stop` (`ORDER_BUILD`/`BUILD_CANCEL`) идёт штатным путём машины
/// приказов: `ProcessOrderHaltBuilding` → `CancelBuilding` снимает площадку
/// ровно один раз.
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

    // Машина приказов: `ProcessOrderBuild`/`BUILD_CANCEL` → `CancelBuilding`.
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
