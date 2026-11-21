//! Энергия комплекса: старт завода только при достаточном балансе,
//! автоматическое отключение зданий при дефиците и сообщение над картой.
//!
//! Оракул — `max-cpp-port`:
//! * `cargo.cpp:70-138` — `Cargo_GetNetProduction`/`Cargo_ApplyUnitConsumption`
//!   (стройка завода тратит raw+power+life из котла комплекса);
//! * `production_manager.cpp:481-562` — `OptimizePowerProduction`/
//!   `OptimizePowerConsumption` (генераторы включаются/выключаются по балансу);
//! * `production_manager.cpp:677-790` — `ValidateIndustry` (стройка
//!   останавливается при нехватке), `OptimizeAuxilaryIndustry`/
//!   `OptimizeMiningIndustry` (порядок отключения потребителей);
//! * `production_manager.cpp:308-330/792-804` — `ComposeIndustryMessage`/
//!   `DrawResourceMessage` (текст сообщения);
//! * `game_manager.cpp:2058-2076/8183-8204` — `ManageEconomy`/
//!   `ProgressBuildState` (когда оптимизация запускается).

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::power;
use max_core::production::{self, BuildError, ProductionEventKind, ProductionManager};
use max_core::team::{TeamId, TeamKind};
use max_core::{GameState, TeamClan, TeamUnits};

fn state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(state: &mut GameState, unit_type: UnitType, grid: Point) -> max_core::UnitId {
    state
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .expect("unit")
}

/// Завод `Training Hall` в (12,11) и склад сырья в (12,12): один комплекс.
fn factory_with_raw(state: &mut GameState, raw: i16) -> (max_core::UnitId, max_core::UnitId) {
    let factory = spawn(state, UnitType::TrainingHall, Point::new(12, 11));
    let dump = spawn(state, UnitType::MaterialStorage, Point::new(12, 13));
    state.unit_mut(dump).expect("MaterialStorage").storage = raw;

    assert_eq!(
        state.unit(factory).expect("factory").complex,
        state.unit(dump).expect("MaterialStorage").complex,
        "завод и MaterialStorage — один комплекс"
    );

    (factory, dump)
}

/// `POWGEN` (+1 энергия) и `FDUMP` с топливом — производитель энергии.
fn add_power(state: &mut GameState) -> (max_core::UnitId, max_core::UnitId) {
    let generator = spawn(state, UnitType::PowerGenerator, Point::new(14, 11));
    let fuel = spawn(state, UnitType::FuelStorage, Point::new(14, 12));
    state.unit_mut(fuel).expect("fuel dump").storage = 50;
    (generator, fuel)
}

/// `HABITAT` (+3 рабочих; сам энергию не потребляет).
fn add_workers(state: &mut GameState) -> max_core::UnitId {
    spawn(state, UnitType::Habitat, Point::new(11, 12))
}

fn production_turn(state: &mut GameState) -> Vec<max_core::ProductionEvent> {
    state.manage_economy(TeamId::RED);
    production::progress_build_state(state, TeamId::RED)
}

fn count_type(state: &GameState, unit_type: UnitType) -> usize {
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

// ---------------------------------------------------------------------------
// 1. Завод не стартует без необходимых ресурсов
// ---------------------------------------------------------------------------

#[test]
fn factory_start_is_refused_without_power() {
    let mut state = state();
    let (factory, _dump) = factory_with_raw(&mut state, 20);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");

    // `Cargo_GetNetProduction(TRAINHAL)` = raw 1 + power 1 + life 1. Энергии
    // нет (нет POWGEN/POWERSTN) — `StartBuilding` в C++ сразу ушла бы в
    // минус, `ValidateIndustry` прервала бы заказ. Порт не стартует и не
    // теряет очередь.
    assert_eq!(
        manager.commit(factory),
        Err(BuildError::NotEnoughPower),
        "без энергии заказ не выполняется"
    );
    assert_eq!(manager.queue(factory), vec![UnitType::Infantry]);
    drop(manager);

    let unit = state.unit(factory).expect("factory");
    assert_eq!(unit.build_time, 0, "очередь не стартует");
    assert_ne!(unit.orders, UnitOrder::Build);
}

#[test]
fn factory_start_is_refused_without_workers() {
    let mut state = state();
    let (factory, _dump) = factory_with_raw(&mut state, 20);
    add_power(&mut state);

    // Энергии хватает (POWGEN +1, TRAINHAL −1), но `Training Hall` требует
    // ещё и рабочего (`Cargo_GetLifeConsumptionRate(TRAINHAL) = 1`).
    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    assert_eq!(
        manager.commit(factory),
        Err(BuildError::NotEnoughWorkers),
        "без HABITAT рабочих нет"
    );
    drop(manager);

    add_workers(&mut state);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager
        .commit(factory)
        .expect("с рабочими и энергией заказ стартует");
}

#[test]
fn factory_start_is_refused_without_raw_materials() {
    let mut state = state();
    let (factory, _dump) = factory_with_raw(&mut state, 0);
    add_power(&mut state);
    add_workers(&mut state);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    assert_eq!(
        manager.commit(factory),
        Err(BuildError::NotEnoughMaterials),
        "первый ход нечем оплатить"
    );
}

// ---------------------------------------------------------------------------
// 2. Дефицит энергии выключает здания, восстановление — по правилу C++
// ---------------------------------------------------------------------------

#[test]
fn power_deficit_powers_off_research_center_before_the_mine() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let research = spawn(&mut state, UnitType::ResearchCentre, Point::new(12, 11));
    let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(9, 11));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(8, 11));
    state.unit_mut(fuel).expect("fuel").storage = 50;

    // `DeployUnit` ставит RESEARCH в POWER_OFF; включаем вручную.
    let _ = state.set_order(research, UnitOrder::PowerOn, UnitOrderState::Init);

    let complex = state.unit(mine).expect("mine").complex.expect("complex");
    for id in [research, generator, fuel] {
        assert_eq!(state.unit(id).expect("unit").complex, Some(complex));
    }

    // Спрос: шахта 1 + RESEARCH 1 = 2; генератор даёт 1 → дефицит 1.
    let outcome = power::optimize_complex(&mut state, TeamId::RED, complex, None, None, true);

    assert_eq!(
        state.unit(research).expect("research").orders,
        UnitOrder::PowerOff,
        "C++ `OptimizeAuxilaryIndustry(RESEARCH)` — первый на отключение"
    );
    assert_eq!(
        state.unit(mine).expect("mine").orders,
        UnitOrder::PowerOn,
        "шахта отключается только после RESEARCH (`OptimizeMiningIndustry`)"
    );
    assert_eq!(outcome.powered_off, vec![research]);
    let message = outcome.message.expect("сообщение о нехватке энергии");
    assert!(
        message.contains("More power needed, Research Center turned off."),
        "текст ComposeIndustryMessage: {message:?}"
    );
    assert!(message.starts_with("Adjustments made:"), "{message:?}");
}

#[test]
fn power_deficit_powers_off_the_mine_when_no_other_consumer_is_left() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let complex = state.unit(mine).expect("mine").complex.expect("complex");

    // У шахты нет производителя энергии: она включена (`ORDER_POWER_ON`),
    // значит её собственное потребление 1 > 0. C++ `OptimizeMiningIndustry`
    // выключает шахту (`production_manager.cpp:729-774`).
    let _ = state.set_order(mine, UnitOrder::PowerOn, UnitOrderState::Init);
    let outcome = power::optimize_complex(&mut state, TeamId::RED, complex, None, None, true);

    assert_eq!(state.unit(mine).expect("mine").orders, UnitOrder::PowerOff);
    assert_eq!(outcome.powered_off, vec![mine]);
    let message = outcome.message.expect("message");
    assert!(message.contains("More power needed, Mining Station turned off."));
}

#[test]
fn generators_are_switched_on_and_off_automatically() {
    let mut state = state();
    let mine = spawn(&mut state, UnitType::MiningStation, Point::new(10, 10));
    let generator = spawn(&mut state, UnitType::PowerGenerator, Point::new(9, 11));
    let fuel = spawn(&mut state, UnitType::FuelStorage, Point::new(8, 11));
    state.unit_mut(fuel).expect("fuel").storage = 50;

    let complex = state.unit(mine).expect("mine").complex.expect("complex");
    assert_eq!(state.unit(generator).expect("gen").complex, Some(complex));

    // POWGEN после `DeployUnit` выключен; шахта требует 1 энергию — генератор
    // включается автоматически (`OptimizePowerProduction`).
    assert_eq!(
        state.unit(generator).expect("generator").orders,
        UnitOrder::PowerOff
    );
    power::optimize_complex(&mut state, TeamId::RED, complex, None, None, false);
    assert_eq!(
        state.unit(generator).expect("generator").orders,
        UnitOrder::PowerOn,
        "генератор включён под спрос шахты"
    );

    // Если топлива нет, генератор выключается, а шахта — как потребитель.
    state.unit_mut(fuel).expect("fuel").storage = 0;
    power::optimize_complex(&mut state, TeamId::RED, complex, None, None, false);
    assert_eq!(
        state.unit(generator).expect("generator").orders,
        UnitOrder::PowerOff,
        "без топлива генератор не работает (`CheckPowerNeed`)"
    );
    assert_eq!(
        state.unit(mine).expect("mine").orders,
        UnitOrder::PowerOff,
        "шахта без энергии выключается"
    );

    // Восстановление топлива снова включает генератор — но потребителя C++
    // сам обратно не включает (`ProductionManager_OptimizeProduction` трогает
    // только производителей), поэтому шахту включаем как игрок.
    state.unit_mut(fuel).expect("fuel").storage = 50;
    let _ = state.set_order(mine, UnitOrder::PowerOn, UnitOrderState::Init);
    power::optimize_complex(&mut state, TeamId::RED, complex, None, None, false);
    assert_eq!(
        state.unit(generator).expect("generator").orders,
        UnitOrder::PowerOn
    );
    assert_eq!(state.unit(mine).expect("mine").orders, UnitOrder::PowerOn);
}

// ---------------------------------------------------------------------------
// 3. Пауза уже идущей стройки и сообщение
// ---------------------------------------------------------------------------

#[test]
fn running_build_pauses_on_power_loss_and_resumes() {
    let mut state = state();
    let (factory, dump) = factory_with_raw(&mut state, 20);
    let (_generator, fuel) = add_power(&mut state);
    add_workers(&mut state);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    manager.commit(factory).expect("power/workers/raw enough");
    drop(manager);

    // Ход 1: оплачен, build_time 2 → 1.
    production_turn(&mut state);
    assert_eq!(state.unit(factory).expect("factory").build_time, 1);
    assert_eq!(state.unit(dump).expect("dump").storage, 19);

    // Топливо кончилось — производитель энергии не может работать.
    state.unit_mut(fuel).expect("fuel").storage = 0;

    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::PausedNoPower),
        "стройка встала из-за энергии: {events:?}"
    );
    assert_eq!(
        state.unit(factory).expect("factory").build_time,
        1,
        "пауза не двигает build_time"
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 0);
    assert_eq!(
        state.unit(dump).expect("dump").storage,
        19,
        "пауза не списывает сырьё"
    );

    // Сообщение над карты — C++ `ComposeIndustryMessage`/
    // `DrawResourceMessage`: ход производства идёт от лица команды без
    // «выделенного» юнита, поэтому форма «More power needed, X turned off.»
    // с префиксом «Adjustments made:».
    let message = state
        .take_industry_message()
        .expect("сообщение о нехватке энергии");
    assert!(
        message.contains("More power needed, Training Hall turned off."),
        "текст сообщения: {message:?}"
    );
    assert!(message.starts_with("Adjustments made:"), "{message:?}");

    // Подвоз топлива: генератор снова включается, стройка доходит до конца.
    state.unit_mut(fuel).expect("fuel").storage = 50;
    let events = production_turn(&mut state);
    assert!(
        events
            .iter()
            .any(|event| event.kind == ProductionEventKind::Spawned),
        "после восстановления энергии завод выпускает юнита: {events:?}"
    );
    assert_eq!(count_type(&state, UnitType::Infantry), 1);
}

#[test]
fn commit_message_names_the_missing_power() {
    let mut state = state();
    let (factory, _dump) = factory_with_raw(&mut state, 20);

    let mut manager = ProductionManager::new(&mut state, TeamId::RED);
    manager.enqueue(factory, UnitType::Infantry).expect("queue");
    assert_eq!(manager.commit(factory), Err(BuildError::NotEnoughPower));
    drop(manager);

    let message = state.take_industry_message().expect("message");
    assert_eq!(message, "Cannot turn Training Hall on, power needed.");
}

// ---------------------------------------------------------------------------
// Детерминизм и снапшот
// ---------------------------------------------------------------------------

#[test]
fn energy_state_is_part_of_snapshot_and_crc() {
    fn build(generator_on: bool) -> GameState {
        let mut state = state();
        let _ = factory_with_raw(&mut state, 20);
        let (generator, _fuel) = add_power(&mut state);
        add_workers(&mut state);
        if generator_on {
            let _ = state.set_order(generator, UnitOrder::PowerOn, UnitOrderState::Init);
        }
        state
    }

    let on = build(true);
    let off = build(false);

    assert_ne!(
        on.snapshot_bytes(),
        off.snapshot_bytes(),
        "приказ генератора входит в снапшот"
    );
    assert_ne!(
        max_core::crc::state_digest(&on),
        max_core::crc::state_digest(&off),
        "CRC состояния меняется вместе с энергетикой"
    );

    let same = build(true);
    assert_eq!(on.snapshot_bytes(), same.snapshot_bytes());
    assert_eq!(
        max_core::crc::state_digest(&on),
        max_core::crc::state_digest(&same)
    );
}

#[test]
fn optimization_is_deterministic() {
    fn run() -> (Vec<u8>, u16) {
        let mut state = state();
        let (factory, _dump) = factory_with_raw(&mut state, 20);
        add_power(&mut state);
        add_workers(&mut state);
        spawn(&mut state, UnitType::ResearchCentre, Point::new(9, 12));

        let mut manager = ProductionManager::new(&mut state, TeamId::RED);
        manager.enqueue(factory, UnitType::Infantry).expect("queue");
        let _ = manager.commit(factory);
        drop(manager);

        production_turn(&mut state);
        production_turn(&mut state);
        (state.snapshot_bytes(), max_core::crc::state_digest(&state))
    }

    assert_eq!(run(), run(), "два прогона энергии совпадают");
}
