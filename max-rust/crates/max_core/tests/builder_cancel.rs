//! Увод строителя с недостроенной площадки отменяет стройку.
//!
//! Оракул: `unitinfo.cpp` `UnitInfo::CancelBuilding` (`:4231`, ветка
//! `!STATIONARY`), `game_manager.cpp` `GameManager_SelectBuildSite` (`:3529`) /
//! `GameManager_GetBuilderUnitCursor` (`:5829`), `units_manager.cpp`
//! `UnitsManager_SetNewOrderInt` (`:1945`). В C++ приказ движения по карте
//! строителю в `BUILD_IN_PROGRESS` невозможен (`CURSOR_UNIT_NO_GO`), а `stop`
//! попапа идёт через `ORDER_BUILD`/`ORDER_STATE_BUILD_CANCEL` →
//! `CancelBuilding`: `build_list.Clear()`, `build_time = 0`,
//! `Access_DestroyUtilities(grid, true, false, false, false)` (лента, конусы,
//! плита), `ORDER_AWAIT`/`EXECUTING_ORDER`; материалы не возвращаются.
//!
//! Порт обязан снимать стройку и тогда, когда приказ пришёл другим путём
//! (пакет 38/08, ИИ, группа): иначе строитель уходил, а `ProgressBuildState`
//! продолжал тратить сырьё и достраивал здание «само».

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
        .filter(|id| state.unit(*id).is_some_and(|unit| unit.unit_type == unit_type))
        .count()
}

/// Заказ `TRAINHAL` конструктором: площадка стоит, приказ — `ORDER_BUILD`.
fn start_training_hall_build(state: &mut max_core::GameState) -> (UnitId, Point) {
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");

    let mut manager = ProductionManager::new(state, TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("constructor builds training halls");
    let report = manager.commit(constructor).expect("site found");
    drop(manager);

    (constructor, report.site)
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

/// `UnitsManager_SetNewOrder` (ИИ, группа, прямой вызов) — тоже новый приказ.
#[test]
fn set_order_to_move_cancels_the_unfinished_build() {
    let mut state = state_with_gold(100);
    let (builder, site) = start_training_hall_build(&mut state);

    state.set_order(builder, UnitOrder::Move, UnitOrderState::InProgress);

    let unit = state.unit(builder).expect("builder alive");
    assert_eq!(
        (unit.orders, unit.state),
        (UnitOrder::Move, UnitOrderState::InProgress),
        "новый приказ применён после отмены стройки"
    );
    // `CancelBuilding` оставляет `prior` на `AWAIT`: новый приказ не должен
    // «восстанавливаться» в пустую стройку при `RestoreOrders`.
    assert_eq!(unit.prior_orders, UnitOrder::Await);
    assert_eq!(unit.prior_state, UnitOrderState::ExecutingOrder);
    assert_build_cancelled(&state, builder, site);

    // Достройки «сама собой» больше нет: ни одного хода производства.
    for _ in 0..12 {
        state.manage_economy(TeamId::RED);
        production::progress_build_state(&mut state, TeamId::RED);
    }
    assert_eq!(
        count_type(&state, UnitType::TrainingHall),
        0,
        "отменённая стройка не должна выпускать здание"
    );
}

/// Пакет **38** (движение с путём) — сетевой путь приказа игрока.
#[test]
fn packet_38_move_cancels_the_unfinished_build() {
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
    assert_build_cancelled(&state, builder, site);
}

/// Пакет **08** (`UnitsManager_SetNewOrder` удалённого игрока): `prior_order`
/// в пакете — старый `ORDER_BUILD`, после отмены он обязан стать `AWAIT`.
#[test]
fn packet_08_move_cancels_the_unfinished_build_and_clears_prior_build() {
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
    assert_build_cancelled(&state, builder, site);
}

/// Достроенная площадка: уход строителя (`UNIT_READY` → `ORDER_ACTIVATE` →
/// `AWAIT_TAPE_POSITIONING`/`TAPE_POSITIONING_DEINIT`) **не** отменяет стройку
/// и не снимает плиту под готовым зданием (`CancelBuilding` в C++ вызывается
/// только для `ORDER_BUILD`/`ORDER_CLEAR`, а не для ухода).
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

/// `stop` (`ORDER_BUILD`/`BUILD_CANCEL`) идёт штатным путём машины приказов и
/// не должен попадать под «отмену перед новым приказом» дважды: площадка
/// снимается ровно один раз, завода-очередь это не трогает.
#[test]
fn build_cancel_order_still_goes_through_the_order_machine() {
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
