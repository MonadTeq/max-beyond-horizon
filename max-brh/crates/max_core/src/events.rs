//! Очередь команд, события юнитов и применение пакетов к [`GameState`].
//!
//! Оракул: `max-cpp-port/src/remote.cpp` (`Remote_ProcessNetPackets` и
//! обработчики `Remote_ReceiveNetPacket_*`, процессоры приказов
//! `Remote_OrderProcessor{0..5}_Read`), `unitevents.cpp`
//! (`UnitEventEmergencyStop`), DESIGN.md §5.1 (шаг 1) и §6.2.
//!
//! # Модель применения
//!
//! Сеть в `max_core` нет: команды приходят уже разобранными
//! ([`crate::input::Command`]) и кладутся в [`CommandQueue`]. Применяются они
//! **строго в порядке получения** в детерминированной точке цикла — в начале
//! тика ([`crate::game::GameState::tick`], шаг 2), аналог
//! `Remote_Synchronize()`/`Remote_ProcessNetPackets()`.
//!
//! # Соответствие C++-порту
//!
//! * Пакет **08** повторяет вложенность процессоров: сначала
//!   `Remote_OrderProcessor1_Read` как предварительная проверка
//!   (`Remote_ReceiveNetPacket_08` разбирает копию пакета), затем процессор
//!   уровня приказа (который снова применяет поля `P1`). Оба прохода
//!   выполняются, как в C++.
//! * Пакет **50** не меняет юнита сразу: C++ создаёт
//!   `UnitEventEmergencyStop` и кладёт его в `UnitEvent_UnitEvents`, а событие
//!   обрабатывается в `UnitsManager_ProcessOrders` — так же и здесь
//!   ([`UnitEvent::EmergencyStop`]).
//! * Пакеты **00/01/45/52** — барьерные факты: они только записываются в
//!   поля состояния и не влияют на юнитов (DESIGN §8.4/§8.6).
//! * Пакеты **10/20** апгрейдят атрибуты через копию `UnitValues`
//!   (`UpdateVersion`/`MarkAsInUse`), как `Remote_ReceiveNetPacket_10/20`.
//! * Пакет **14** (высадка) возвращает [`ApplyOutcome::Unsupported`]:
//!   `UnitsManager_DeployUnit` требует данных миссии/точки высадки, которые
//!   приходят из setup-слоя (P3); флаги типов уже есть в
//!   [`crate::access::unit_type_flags`], и вызывающий слой деплоит юнита через
//!   [`crate::game::GameState::spawn_unit`].
//! * Пакет **12** (снабжение миссии) сохраняется как данные
//!   ([`crate::game::GameState::mission_supplies`]) и обновляет
//!   `stats_gold_spent_on_upgrades` — как приёмник C++.
//! * Пакет **54** — не [`Command`], а авторитетное состояние команды:
//!   [`apply_state_sync`] применяет `Packet23Data` каждого юнита, удаляет
//!   лишних (`Remote_RemoveExtraUnits`) и выравнивает хвост экономики
//!   (`Remote_ReceiveNetPacket_54`).

use crate::enums::{ResearchTopic, UnitOrder, UnitOrderState, UnitType, RESEARCH_TOPIC_COUNT};
use crate::game::{GameState, OrderData};
use crate::geo::Point;
use crate::input::{
    is_valid_order, is_valid_team_index, Command, MovePathData, OrderPayload, Packet08Order,
    PayloadLevel, PayloadP1, PayloadP2, PayloadP3, PayloadP4, PayloadP5, UPGRADE_VALUE_COUNT,
};
use crate::team::{ResearchTopicInfo, TeamId, TeamKind};
use crate::unit::{AirPath, GroundPath, UnitId, UnitInfo, UnitListKind, UnitPath};
use crate::unit_order;
use crate::unit_values::{
    ATTRIB_AGENT_ADJUST, ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_ATTACK_RADIUS,
    ATTRIB_HITS, ATTRIB_MOVE_AND_FIRE, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN, ATTRIB_SPEED,
    ATTRIB_STORAGE, ATTRIB_TURNS, UNIT_COUNT,
};

// ---------------------------------------------------------------------------
// События юнитов (C++ `UnitEvent_UnitEvents`)
// ---------------------------------------------------------------------------

/// Событие юнита — C++ `class UnitEvent` (`unitevents.hpp`).
///
/// Единственный вариант оригинала — аварийный стоп (`UnitEventEmergencyStop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitEvent {
    /// `UnitEventEmergencyStop::Process` (`unitevents.cpp:38`).
    EmergencyStop {
        /// Юнит, которому пришёл пакет 50.
        unit: UnitId,
    },
}

impl UnitEvent {
    /// Юнит, к которому относится событие.
    pub const fn unit(&self) -> UnitId {
        match self {
            Self::EmergencyStop { unit } => *unit,
        }
    }
}

/// `UnitEvent::Process` — применяет событие к партии.
pub fn process_unit_event(state: &mut GameState, event: &UnitEvent) {
    match event {
        UnitEvent::EmergencyStop { unit } => {
            let Some(target) = state.unit(*unit).cloned() else {
                return;
            };
            if target.hits == 0 {
                return;
            }
            let mut target = target;
            unit_order::stop_movement(&mut target, state.disable_fire);
            state.write_unit(*unit, target);
        }
    }
}

// ---------------------------------------------------------------------------
// Очередь команд
// ---------------------------------------------------------------------------

/// Очередь входных команд в порядке получения (DESIGN §5.1 шаг 1).
///
/// `VecDeque` — FIFO без итерации по хэш-таблицам; порядок полностью
/// определяется порядком поступления пакетов.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandQueue {
    pending: std::collections::VecDeque<Command>,
    applied: u64,
}

impl CommandQueue {
    /// Пустая очередь.
    pub fn new() -> Self {
        Self::default()
    }

    /// Кладёт команду в конец (порядок получения).
    pub fn push(&mut self, command: Command) {
        self.pending.push_back(command);
    }

    /// Забирает самую старую команду.
    pub fn pop_front(&mut self) -> Option<Command> {
        let command = self.pending.pop_front()?;
        self.applied = self.applied.wrapping_add(1);
        Some(command)
    }

    /// Сколько команд ещё не применено.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Пуста ли очередь.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Сколько команд применено с момента создания (диагностика).
    pub const fn applied(&self) -> u64 {
        self.applied
    }

    /// Очищает очередь (реконнект/новый прогон).
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

// ---------------------------------------------------------------------------
// Результат применения
// ---------------------------------------------------------------------------

/// Результат применения команды.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Команда применена.
    Applied,
    /// Пакет относится к неизвестному юниту (`Remote_NetErrorUnknownUnit`).
    UnknownUnit,
    /// Пакет неверного формата (`NetPacket::SetMalformed` в C++).
    Malformed,
    /// Пакет валиден, но не применим на этой фазе (см. шапку модуля).
    Unsupported,
    /// Пакет проигнорирован (невалидный индекс команды, неизвестный комплекс).
    Ignored,
}

// ---------------------------------------------------------------------------
// Применение очереди
// ---------------------------------------------------------------------------

/// Применяет все накопленные команды в порядке получения.
///
/// Возвращает количество применённых команд (как `Remote_ProcessNetPackets`
/// вычерпывает очередь до `REMOTE_RECEIVE_EMPTY`).
pub fn apply_pending(state: &mut GameState) -> usize {
    let mut applied = 0;

    while let Some(command) = state.commands.pop_front() {
        apply_command(state, &command);
        applied += 1;
    }

    applied
}

/// Применяет одну команду к состоянию — аналог `Remote_ReceiveNetPacket_*`.
pub fn apply_command(state: &mut GameState, command: &Command) -> ApplyOutcome {
    match command {
        Command::Order(order) => apply_packet_08(state, order),
        Command::Move(movement) => apply_packet_38(state, movement),
        Command::MoveFinished(finished) => apply_packet_41(state, finished),
        Command::Stop(stop) => apply_packet_50(state, stop),
        Command::FinishedTurn(finished) => apply_packet_06(state, finished),
        Command::EndTurnBarrier(barrier) => apply_packet_52(state, barrier),
        Command::CrcBarrier(barrier) => apply_packet_45(state, barrier),
        Command::TurnBarrier(barrier) => apply_packet_00(state, barrier),
        Command::FrameBarrier(barrier) => apply_packet_01(state, barrier),
        Command::Chat(chat) => apply_packet_18(state, chat),
        Command::ComplexOrder(order) => apply_packet_11(state, order),
        Command::ComplexSummary(summary) => apply_packet_53(state, summary),
        Command::UpgradeUnitType(upgrade) => apply_packet_10(state, upgrade),
        Command::UpgradeUnit(upgrade) => apply_packet_20(state, upgrade),
        Command::Rename(rename) => apply_packet_43(state, rename),
        Command::Seed(seed) => {
            // Пакет 13 — единственная точка сидирования симуляции
            // (DESIGN §6.1). C++ хранит seed в `Remote_RngSeed` и применяет его
            // при старте партии; `max_core` применяет сразу — других
            // потребителей seed'а в симуляции нет.
            state.rng.set_seed(seed.seed);
            ApplyOutcome::Applied
        }
        Command::Deploy(_deploy) => ApplyOutcome::Unsupported,
        Command::MissionSupply(supply) => apply_packet_12(state, supply),
    }
}

// ---------------------------------------------------------------------------
// 08 — приказ юниту
// ---------------------------------------------------------------------------

fn apply_packet_08(state: &mut GameState, packet: &Packet08Order) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };

    // `UnitsManager_NewOrderWhileScaling` (`units_manager.cpp:2529`): приказ,
    // пришедший на этапе `EXPAND`, в C++ сразу зовёт
    // `UnitsManager_UpdateConnectors` — иначе здание, переставленное приказом,
    // никогда не попало бы в комплекс (анимация уже не завершится).
    let connectors_due =
        unit.orders == UnitOrder::AwaitScaling && unit.state == UnitOrderState::Expand;

    // `Remote_ReceiveNetPacket_08`: предварительный проход `P1` по копии
    // пакета — валидация приказов до выбора процессора.
    if let Some(p1) = packet.payload.p1() {
        if !is_valid_order(p1.order) || !is_valid_order(p1.prior_order) {
            return ApplyOutcome::Malformed;
        }
        // Стройку нельзя отменить новым приказом: в C++ увод строителя с
        // недостроенной площадки через карту невозможен (`game_manager.cpp:5927-5931`
        // — `CURSOR_UNIT_NO_GO`), а отмена идёт только явным `stop`
        // (`UnitsManager_Popup_OnClick_BuildStop` → `ORDER_BUILD`/
        // `ORDER_STATE_BUILD_CANCEL` → `UnitInfo::CancelBuilding`).
        apply_payload_p1(&mut unit, p1);
    }

    // Выбор процессора по *новому* приказу юнита (`Remote_OrderProcessors[...]`).
    let Some(order) = packet.payload.order() else {
        // `ORDER_17` (P0): пустой payload, менять нечего.
        state.write_unit(packet.unit, unit);
        return ApplyOutcome::Applied;
    };

    // Процессор должен соответствовать приказу (иначе — malformed). В C++
    // процессор выбирается по уже применённому приказу юнита, поэтому
    // предварительный проход `P1` к этому моменту уже изменяет юнита — как и
    // здесь (сначала записываем копию, затем сообщаем о malformed).
    if packet.payload.level() != PayloadLevel::for_order(order) {
        state.write_unit(packet.unit, unit);
        return ApplyOutcome::Malformed;
    }

    let result = match &packet.payload {
        OrderPayload::Empty => ApplyOutcome::Applied,
        OrderPayload::P1(p1) => {
            apply_payload_p1(&mut unit, p1);
            ApplyOutcome::Applied
        }
        OrderPayload::P2(p2) => {
            apply_payload_p2(state, &mut unit, p2);
            ApplyOutcome::Applied
        }
        OrderPayload::P3(p3) => {
            apply_payload_p3(state, &mut unit, p3);
            ApplyOutcome::Applied
        }
        OrderPayload::P4(p4) => {
            apply_payload_p4(state, &mut unit, p4);
            ApplyOutcome::Applied
        }
        OrderPayload::P5(p5) => {
            apply_payload_p5(state, &mut unit, p5);
            ApplyOutcome::Applied
        }
    };

    state.write_unit(packet.unit, unit);

    if connectors_due {
        crate::complex::update_connectors(state, packet.unit);
    }

    // Порт ставит площадку в момент заказа (`DeployConstructionSiteUtilities` +
    // `PrepareConstructionSite`): C++ `StartBuilding` идёт на следующем тике
    // (`UnitsManager_ProcessOrderBuild`/`INIT`), но порт применяет P4 сразу и
    // плита (`LRGSLAB`/`SMLSLAB`) обязана появиться на кадре подтверждения
    // места — без задержки в тик. `deploy_construction_site` идемпотентна,
    // поэтому `start_building` на следующем тике вторую площадку не ставит.
    if let Some(unit) = state.unit(packet.unit) {
        if unit.orders == UnitOrder::Build
            && unit.state == UnitOrderState::Init
            && unit.build_time > 0
            && crate::production::is_mobile_builder(unit.unit_type)
        {
            if let Some(head) = unit.build_list.first().copied() {
                let site = unit.target_grid;
                let _ = crate::production::deploy_construction_site(
                    state,
                    packet.unit,
                    head,
                    site,
                );
            }
        }
    }

    result
}

/// `Remote_OrderProcessor1_Read` (`remote.cpp:379`).
fn apply_payload_p1(unit: &mut UnitInfo, p1: &PayloadP1) {
    if !is_valid_order(p1.order) || !is_valid_order(p1.prior_order) {
        return;
    }

    // Сброс незавершённой анимации масштаба до смены приказа.
    unit_order::new_order_while_scaling(unit);

    unit.disabled_reaction_fire = p1.disabled_reaction_fire;
    unit.orders = p1.order;
    unit.state = p1.state;
    unit.prior_orders = p1.prior_order;
    unit.prior_state = p1.prior_state;
}

/// `Remote_OrderProcessor2_Read` (`remote.cpp:426`).
fn apply_payload_p2(state: &mut GameState, unit: &mut UnitInfo, p2: &PayloadP2) {
    apply_payload_p1(unit, &p2.p1);

    unit.parent_unit = p2.parent_unit;
    unit.experience = p2.experience;
    let _ = state;
}

/// `Remote_OrderProcessor3_Read` (`remote.cpp:462`).
fn apply_payload_p3(state: &mut GameState, unit: &mut UnitInfo, p3: &PayloadP3) {
    apply_payload_p2(state, unit, &p3.p2);

    let mut data = state.order_data(unit.id);
    data.move_to = p3.move_to;
    data.fire_on = p3.fire_on;
    // V70-поле `target_grid` зеркалит `move_to` (см. `OrderData`).
    unit.target_grid = p3.move_to;
    state.set_order_data(unit.id, data);

    unit.enemy_unit = p3.enemy_unit;
    unit.transfer_cargo = p3.transfer_cargo;
    unit.stealth_dice_roll = p3.stealth_dice_roll;
}

/// `Remote_OrderProcessor4_Read` (`remote.cpp:502`).
fn apply_payload_p4(state: &mut GameState, unit: &mut UnitInfo, p4: &PayloadP4) {
    apply_payload_p3(state, unit, &p4.p3);

    unit.repeat_build = p4.repeat_build;
    unit.build_time = p4.build_time;
    unit.build_rate = p4.build_rate;

    unit.build_list.clear();
    unit.build_list.extend_from_slice(&p4.build_list);
}

/// `Remote_OrderProcessor5_Read` (`remote.cpp:535`).
fn apply_payload_p5(state: &mut GameState, unit: &mut UnitInfo, p5: &PayloadP5) {
    apply_payload_p3(state, unit, &p5.p3);

    unit.total_mining = p5.total_mining;
    unit.raw_mining = p5.raw_mining;
    unit.fuel_mining = p5.fuel_mining;
    unit.gold_mining = p5.gold_mining;
}

// ---------------------------------------------------------------------------
// 38 — движение с путём
// ---------------------------------------------------------------------------

fn apply_packet_38(state: &mut GameState, packet: &crate::input::Packet38Move) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };

    // `Remote_ReceiveNetPacket_38`: снять прежний путь, завершить масштаб,
    // затем выставить приказ/состояние.
    if unit.path.is_some() {
        unit.path = None;
    }

    unit_order::new_order_while_scaling(&mut unit);

    // Приказ движения стройку не отменяет: отмена — только явный `stop`
    // (`UnitInfo::CancelBuilding`), см. `apply_packet_08`.

    unit.orders = packet.order;
    unit.state = packet.state;

    let mut data = state.order_data(packet.unit);
    data.move_to = packet.move_to;
    unit.target_grid = packet.move_to;
    state.set_order_data(packet.unit, data);

    unit.group_speed = packet.group_speed;

    // `grid_x/grid_y` (протокол §8.4, `Remote_RelocateUnit`): у получателя
    // юнит стоит на клетке владельца. C++-порт отправляет 38 без этих полей,
    // поэтому применяется только реальное расхождение.
    if unit.grid != packet.grid {
        unit.grid = packet.grid;
    }

    match &packet.path {
        Some(MovePathData::Ground(steps)) if !steps.is_empty() => {
            unit.path = Some(UnitPath::Ground(GroundPath {
                end_x: packet.move_to.x,
                end_y: packet.move_to.y,
                step_index: 0,
                steps: steps.clone(),
            }));
        }
        Some(MovePathData::Air {
            end_x,
            end_y,
            distance_x,
            distance_y,
            euclidean_distance,
            speed,
            move_fraction,
            max_velocity,
        }) => {
            let angle = unit_order::paths_get_angle(
                i32::from(*end_x) - i32::from(unit.grid.x),
                i32::from(*end_y) - i32::from(unit.grid.y),
            );
            unit.path = Some(UnitPath::Air(AirPath {
                length: *euclidean_distance,
                angle: (angle & 7) as i8,
                start_x: unit.grid.x,
                start_y: unit.grid.y,
                end_x: *end_x,
                end_y: *end_y,
                step_x: *distance_x,
                step_y: *distance_y,
                delta_x: 0,
                delta_y: 0,
            }));
            unit.speed = *speed;
            unit.move_fraction = *move_fraction;
            unit.max_velocity = *max_velocity;
        }
        _ => {}
    }

    let path_is_missing = unit.path.is_none();
    let needs_path = matches!(
        unit.orders,
        UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
    ) && matches!(
        unit.state,
        UnitOrderState::Init
            | UnitOrderState::MoveInit
            | UnitOrderState::NewOrder
            | UnitOrderState::MoveGettingPath
    );

    state.write_unit(packet.unit, unit);
    state.set_order_data(packet.unit, data);

    // Пакет 38 без шагов (короткая форма C++-порта): путь запрашивается у
    // синхронного pathfinding, а не строится линейно (DESIGN §5.1, риск 2).
    if path_is_missing && needs_path {
        crate::pathfinding::request_path(
            state,
            packet.unit,
            crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
        );
    }

    ApplyOutcome::Applied
}

/// Вспомогательная проверка «путь без шагов» (см. ветвление 38).
#[allow(dead_code)]
fn steps_is_empty(packet: &crate::input::Packet38Move) -> bool {
    match &packet.path {
        None => true,
        Some(MovePathData::Ground(steps)) => steps.is_empty(),
        Some(MovePathData::Air { .. }) => false,
    }
}

// ---------------------------------------------------------------------------
// 41 / 50 — завершение и аварийный стоп
// ---------------------------------------------------------------------------

fn apply_packet_41(
    state: &mut GameState,
    packet: &crate::input::Packet41MoveFinished,
) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };

    // Поля протокола §8.4 (в короткой форме C++-порта отсутствуют).
    if let Some(grid) = packet.grid {
        unit.grid = grid;
    }
    if let Some(speed) = packet.speed {
        unit.speed = speed;
    }
    if let Some(group_speed) = packet.group_speed {
        unit.group_speed = group_speed;
    }
    if let Some(shots) = packet.shots {
        unit.shots = shots;
    }
    if let Some(move_fraction) = packet.move_fraction {
        unit.move_fraction = move_fraction;
    }

    // `Remote_ReceiveNetPacket_41`: `unit->BlockedOnPathRequest(mode, true)`.
    // `MoveFinished(mode)` добавляет юнита в список отложенных реакций только
    // при `mode` и включённом ответном огне.
    unit_order::blocked_on_path_request(&mut unit, packet.mode, state.disable_fire);
    state.write_unit(packet.unit, unit);
    if packet.mode && !state.disable_fire {
        state.add_to_delayed_reaction_list(packet.unit);
    }
    ApplyOutcome::Applied
}

fn apply_packet_50(state: &mut GameState, packet: &crate::input::Packet50StopUnit) -> ApplyOutcome {
    if !state.has_unit(packet.unit) {
        return ApplyOutcome::UnknownUnit;
    }

    // `Remote_ReceiveNetPacket_50`: событие ставится в очередь и выполняется
    // в `UnitsManager_ProcessOrders`.
    state
        .unit_events
        .push(UnitEvent::EmergencyStop { unit: packet.unit });
    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// 06 / 52 / 45 / 00 / 01 — конец хода и барьеры
// ---------------------------------------------------------------------------

fn apply_packet_06(
    state: &mut GameState,
    packet: &crate::input::Packet06FinishedTurn,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    state.teams[packet.team.0 as usize].finished_turn = true;

    if state.play_mode == crate::enums::PlayMode::TurnBased {
        state.game_state = crate::enums::GameStateType::EndTurn;
    } else {
        // `GameManager_HandleTurnTimer()` — UI; проверка «есть ли ещё
        // незавершённые команды» — для логов/сообщений.
        let _pending = !state.are_teams_finished_turn();
    }

    ApplyOutcome::Applied
}

fn apply_packet_52(
    state: &mut GameState,
    packet: &crate::input::Packet52EndTurnBarrier,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }
    state.turn_indices[packet.team.0 as usize] = packet.counter;
    ApplyOutcome::Applied
}

fn apply_packet_45(
    state: &mut GameState,
    packet: &crate::input::Packet45CrcBarrier,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }
    state.next_turn_indices[packet.team.0 as usize] = packet.next_turn_index;
    state.team_data_crc16[packet.team.0 as usize] = packet.crc16;
    ApplyOutcome::Applied
}

fn apply_packet_00(
    state: &mut GameState,
    packet: &crate::input::Packet00TurnBarrier,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }
    state.frame_sync_counter[packet.team.0 as usize] = packet.counter;
    ApplyOutcome::Applied
}

fn apply_packet_01(
    state: &mut GameState,
    packet: &crate::input::Packet01FrameBarrier,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }
    state.frame_sync_counter2[packet.team.0 as usize] = packet.counter;
    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// 18 / 11 / 53 — чат, комплексы
// ---------------------------------------------------------------------------

fn apply_packet_18(state: &mut GameState, packet: &crate::input::Packet18Chat) -> ApplyOutcome {
    if !is_valid_team_index(packet.sender) {
        return ApplyOutcome::Ignored;
    }
    state.chat_log.push((packet.sender, packet.message.clone()));
    ApplyOutcome::Applied
}

fn apply_packet_11(
    state: &mut GameState,
    packet: &crate::input::Packet11ComplexOrder,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    let Some(complex) = state.complex_mut(packet.team, packet.complex_id) else {
        return ApplyOutcome::Ignored;
    };

    complex.material = packet.material;
    complex.fuel = packet.fuel;
    complex.gold = packet.gold;
    complex.power = packet.power;
    complex.workers = packet.workers;
    complex.buildings = packet.buildings;
    ApplyOutcome::Applied
}

fn apply_packet_53(
    state: &mut GameState,
    packet: &crate::input::Packet53ComplexSummary,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    // Применяем все записи; отсутствующие комплексы пропускаются (как
    // `TeamUnits::ReadComplexPacket` с `GetComplex(id)` == nullptr в порту).
    for entry in &packet.complexes {
        if let Some(complex) = state.complex_mut(packet.team, entry.complex_id) {
            complex.material = entry.values[0];
            complex.fuel = entry.values[1];
            complex.gold = entry.values[2];
            complex.power = entry.values[3];
            complex.workers = entry.values[4];
            complex.buildings = entry.values[5];
        }
    }

    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// 54 — авторитетное состояние команды (P23-запись + хвост экономики)
// ---------------------------------------------------------------------------

/// Каноническое состояние юнита — `struct Packet23Data` (`max-brh-docs/protocol.md`
/// §8.5); используется пакетами 23 и 54.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnitStatePacket {
    /// Текущий приказ (`UnitOrderType`, сырой `u8`).
    pub orders: u8,
    /// Состояние приказа (`UnitOrderStateType`, сырой `u8`).
    pub state: u8,
    /// Отложенный приказ.
    pub prior_orders: u8,
    /// Состояние отложенного приказа.
    pub prior_state: u8,
    /// Ответный огонь запрещён.
    pub disabled_reaction_fire: bool,
    /// Родительский юнит (`0` — нет).
    pub parent_unit_id: u16,
    /// Опыт.
    pub experience: i16,
    /// Точка движения X.
    pub move_to_grid_x: i16,
    /// Точка движения Y.
    pub move_to_grid_y: i16,
    /// Цель огня X.
    pub fire_on_grid_x: i16,
    /// Цель огня Y.
    pub fire_on_grid_y: i16,
    /// Юнит-цель (`0` — нет).
    pub enemy_unit_id: u16,
    /// Перегружаемый груз.
    pub transfer_cargo: i16,
    /// Бросок маскировки.
    pub stealth_dice_roll: u8,
    /// Добыча: всего.
    pub total_mining: u8,
    /// Добыча материалов.
    pub raw_mining: u8,
    /// Добыча топлива.
    pub fuel_mining: u8,
    /// Добыча золота.
    pub gold_mining: u8,
    /// Таймер постройки.
    pub build_time: u8,
    /// Темп постройки.
    pub build_rate: u16,
    /// Тип юнита (`ResourceID`).
    pub unit_type: u16,
    /// `UnitInfo::unit_id` (номер в команде, один байт).
    pub unit_id: u8,
    /// Клетка X.
    pub grid_x: i16,
    /// Клетка Y.
    pub grid_y: i16,
    /// Команда.
    pub team: u8,
    /// Хиты.
    pub hits: u16,
    /// Скорость.
    pub speed: u16,
    /// Выстрелы.
    pub shots: u8,
    /// Груз в хранилище.
    pub storage: i16,
    /// Боезапас.
    pub ammo: u8,
}

/// Хвост последнего чанка пакета 54 — экономика команды на момент проверки.
///
/// Поля повторяют `Remote_SendUnitStates`/`Remote_ReceiveNetPacket_54`
/// (`remote.cpp:4368-4373`, `4601-4648`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamStateSyncTail {
    /// Кредиты команды (`TeamUnits::gold`).
    pub gold: u32,
    /// Золото, потраченное на апгрейды.
    pub gold_spent_on_upgrades: u32,
    /// Прогресс восьми тем исследований.
    pub research: [ResearchTopicInfo; RESEARCH_TOPIC_COUNT],
    /// Счётчик созданных объектов.
    pub number_of_objects_created: u16,
    /// Счётчики имён по типам (`uint32_t[UNIT_END]`).
    pub unit_counters: [u32; UNIT_COUNT],
}

impl Default for TeamStateSyncTail {
    fn default() -> Self {
        Self {
            gold: 0,
            gold_spent_on_upgrades: 0,
            research: [ResearchTopicInfo::default(); RESEARCH_TOPIC_COUNT],
            number_of_objects_created: 0,
            unit_counters: [0; UNIT_COUNT],
        }
    }
}

/// Разобранный пакет 54 в терминах `max_core` (без зависимости от `max_net`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSyncPacket {
    /// Команда-владелец состояния.
    pub team: TeamId,
    /// Номер проверки (отличает раунды).
    pub sync_index: u8,
    /// Последний чанк команды (несёт [`TeamStateSyncTail`]).
    pub is_last: bool,
    /// Юниты чанка в порядке владельца.
    pub units: Vec<(UnitId, UnitStatePacket)>,
    /// Хвост экономики; обязателен при `is_last`.
    pub tail: Option<TeamStateSyncTail>,
}

/// Порт `Remote_ApplyUnitState` (`remote.cpp:4383-4512`) для одного юнита.
///
/// Принимает только юнита своей команды и типа; несуществующие/взорванные
/// юниты и приказы вне `ORDER_COUNT_MAX` игнорируются — как в C++.
pub fn apply_unit_state(
    state: &mut GameState,
    team: TeamId,
    unit_id: UnitId,
    data: &UnitStatePacket,
) -> ApplyOutcome {
    let Some(mut unit) = state.unit(unit_id).cloned() else {
        // `Remote_ApplyUnitState`: «unit ... does not exist locally».
        return ApplyOutcome::UnknownUnit;
    };

    if unit.team != team || unit.unit_type as u16 != data.unit_type {
        return ApplyOutcome::Ignored;
    }
    if unit.flags & crate::enums::unit_flags::EXPLODING != 0 {
        return ApplyOutcome::Ignored;
    }

    let (Ok(orders), Ok(order_state), Ok(prior_orders), Ok(prior_state)) = (
        UnitOrder::try_from(data.orders),
        UnitOrderState::try_from(data.state),
        UnitOrder::try_from(data.prior_orders),
        UnitOrderState::try_from(data.prior_state),
    ) else {
        return ApplyOutcome::Malformed;
    };

    unit.orders = orders;
    unit.state = order_state;
    unit.prior_orders = prior_orders;
    unit.prior_state = prior_state;
    unit.disabled_reaction_fire = data.disabled_reaction_fire;

    // `SetParent`/`SetEnemy` в C++ берут юнита из `Hash_UnitHash`; отсутствующий
    // id даёт `nullptr`, поэтому здесь — `None`.
    unit.parent_unit = if data.parent_unit_id == 0 {
        None
    } else {
        state
            .unit(UnitId(data.parent_unit_id))
            .map(|parent| parent.id)
    };
    unit.enemy_unit = if data.enemy_unit_id == 0 {
        None
    } else {
        state.unit(UnitId(data.enemy_unit_id)).map(|enemy| enemy.id)
    };

    unit.experience = data.experience;
    unit.transfer_cargo = data.transfer_cargo;
    unit.stealth_dice_roll = data.stealth_dice_roll;
    unit.total_mining = data.total_mining;
    unit.raw_mining = data.raw_mining;
    unit.fuel_mining = data.fuel_mining;
    unit.gold_mining = data.gold_mining;
    unit.build_time = data.build_time;
    unit.build_rate = data.build_rate;
    unit.unit_id = data.unit_id;
    unit.hits = data.hits;
    unit.speed = data.speed;
    unit.shots = data.shots;
    unit.storage = data.storage;
    unit.ammo = data.ammo;

    // Точки движения/огня в Rust живут в `OrderData` (`unit.target_grid`
    // зеркалит `move_to`, как в пакете 08 P3).
    let mut order_data = state.order_data(unit_id);
    order_data.move_to = Point::new(data.move_to_grid_x, data.move_to_grid_y);
    order_data.fire_on = Point::new(data.fire_on_grid_x, data.fire_on_grid_y);
    unit.target_grid = order_data.move_to;
    state.set_order_data(unit_id, order_data);

    // `Remote_RelocateUnit`: у получателя юнит встаёт на клетку владельца;
    // `write_unit` переносит его в `Hash_MapHash`.
    unit.grid = Point::new(data.grid_x, data.grid_y);

    state.write_unit(unit_id, unit);
    ApplyOutcome::Applied
}

/// Порт `Remote_ReceiveNetPacket_54` (`remote.cpp:4547-4652`).
///
/// Применяет авторитетное состояние удалённой команды, удаляет юнитов,
/// которых нет у владельца (кроме зданий — как `Remote_RemoveExtraUnits`), и
/// выравнивает счётчики id/золото/исследования на последнем чанке. Индекс
/// раунда записывается в [`GameState::state_sync_indices`], по нему закрывается
/// 54-барьер.
pub fn apply_state_sync(state: &mut GameState, packet: &StateSyncPacket) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    let slot = packet.team.0 as usize;
    // «only the owner may dictate the state of its units» — своё состояние и
    // команды без remote-типа не применяются.
    if state.teams[slot].team_type != TeamKind::Remote {
        return ApplyOutcome::Ignored;
    }

    if state.state_sync_receiving_indices[slot] != packet.sync_index {
        state.state_sync_receiving_indices[slot] = packet.sync_index;
        state.state_sync_unit_ids[slot].clear();
    }

    for (unit_id, data) in &packet.units {
        state.state_sync_unit_ids[slot].push(*unit_id);
        apply_unit_state(state, packet.team, *unit_id, data);
    }

    if packet.is_last {
        let Some(tail) = &packet.tail else {
            return ApplyOutcome::Malformed;
        };

        // Счётчики id — до удаления лишних юнитов, как в C++.
        state.teams[slot].number_of_objects_created = tail.number_of_objects_created;
        state.teams[slot].unit_counters = tail.unit_counters;

        remove_extra_units(state, packet.team);
        state.state_sync_unit_ids[slot].clear();

        state.team_units[slot].gold = tail.gold;
        state.teams[slot].stats_gold_spent_on_upgrades = tail.gold_spent_on_upgrades;
        state.teams[slot].research_topics = tail.research;
        state.state_sync_indices[slot] = packet.sync_index;
    }

    ApplyOutcome::Applied
}

/// Порт `Remote_RemoveExtraUnits` (`remote.cpp:4517-4545`).
///
/// Юниты команды, которых нет у владельца, удаляются; здания/наземное
/// покрытие (`STATIONARY | GROUND_COVER`) не удаляются — как в C++.
fn remove_extra_units(state: &mut GameState, team: TeamId) {
    use crate::enums::unit_flags::{EXPLODING, GROUND_COVER, STATIONARY};

    let mut owner_unit_ids = state.state_sync_unit_ids[team.0 as usize].clone();
    owner_unit_ids.sort_unstable();

    let mut extra = Vec::new();

    for kind in [
        UnitListKind::GroundCover,
        UnitListKind::MobileLandSea,
        UnitListKind::Stationary,
        UnitListKind::MobileAir,
    ] {
        for &id in state.unit_list(kind) {
            let Some(unit) = state.unit(id) else {
                continue;
            };
            if unit.team != team
                || unit.id.is_no_id()
                || unit.flags & EXPLODING != 0
                || owner_unit_ids.binary_search(&unit.id).is_ok()
            {
                continue;
            }
            if unit.flags & (STATIONARY | GROUND_COVER) != 0 {
                continue;
            }
            extra.push(unit.id);
        }
    }

    for id in extra {
        state.destroy_unit(id);
    }
}

// ---------------------------------------------------------------------------
// 10 / 20 / 43 / 12 — апгрейды, имя, снабжение
// ---------------------------------------------------------------------------

/// Порядок 13 атрибутов пакетов 10/20 (`max-brh-docs/protocol.md` §8.4).
///
/// Совпадает с порядком присваиваний в `Remote_ReceiveNetPacket_10/20`.
pub const UPGRADE_ATTRIBUTE_ORDER: [u8; UPGRADE_VALUE_COUNT] = [
    ATTRIB_TURNS,
    ATTRIB_HITS,
    ATTRIB_ARMOR,
    ATTRIB_ATTACK,
    ATTRIB_SPEED,
    ATTRIB_RANGE,
    ATTRIB_ROUNDS,
    ATTRIB_MOVE_AND_FIRE,
    ATTRIB_SCAN,
    ATTRIB_STORAGE,
    ATTRIB_AMMO,
    ATTRIB_ATTACK_RADIUS,
    ATTRIB_AGENT_ADJUST,
];

fn apply_packet_10(
    state: &mut GameState,
    packet: &crate::input::Packet10UpgradeUnitType,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    let index = packet.unit_type as usize;
    if index >= crate::unit_values::UNIT_COUNT {
        return ApplyOutcome::Malformed;
    }

    let Some(team_units) = state.team_units.get_mut(packet.team.0 as usize) else {
        return ApplyOutcome::Ignored;
    };

    let mut values = team_units.current_values[index];
    values.update_version();
    values.mark_as_not_in_use();

    for (attribute, value) in UPGRADE_ATTRIBUTE_ORDER.iter().zip(packet.values.iter()) {
        values.set_attribute(*attribute, i32::from(*value));
    }

    team_units.current_values[index] = values;
    ApplyOutcome::Applied
}

fn apply_packet_20(
    state: &mut GameState,
    packet: &crate::input::Packet20UpgradeUnit,
) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };

    // Апгрейд ложится на копию: базовые значения общие для типа до первого
    // копирования (`Remote_ReceiveNetPacket_20`).
    let mut values = unit.base_values;
    values.update_version();
    values.mark_as_in_use();

    for (attribute, value) in UPGRADE_ATTRIBUTE_ORDER.iter().zip(packet.values.iter()) {
        values.set_attribute(*attribute, i32::from(*value));
    }

    unit.base_values = values;
    state.write_unit(packet.unit, unit);
    ApplyOutcome::Applied
}

fn apply_packet_43(state: &mut GameState, packet: &crate::input::Packet43Rename) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };
    unit.name = packet.name.clone();
    state.write_unit(packet.unit, unit);
    ApplyOutcome::Applied
}

fn apply_packet_12(
    state: &mut GameState,
    packet: &crate::input::Packet12MissionSupply,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    state.teams[packet.team.0 as usize].stats_gold_spent_on_upgrades =
        packet.gold_spent_on_upgrades;
    state.mission_supplies[packet.team.0 as usize] = Some(packet.clone());
    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// Помощники для тестов и сетевого слоя
// ---------------------------------------------------------------------------

/// Собирает payload `P1` с приказом, сохранив остальные поля как у `unit`.
pub fn payload_p1_for(
    order: UnitOrder,
    state: UnitOrderState,
    prior_order: UnitOrder,
    prior_state: UnitOrderState,
) -> PayloadP1 {
    PayloadP1 {
        order,
        state,
        prior_order,
        prior_state,
        disabled_reaction_fire: false,
    }
}

/// Собирает команду 08 с payload уровня `P1`.
pub fn command_order_p1(unit: UnitId, p1: PayloadP1) -> Command {
    Command::Order(Packet08Order {
        unit,
        payload: OrderPayload::P1(p1),
    })
}

/// Собирает команду 38 «движение по линейному маршруту» (как
/// `GroundPath::AppendLinearSteps`).
pub fn command_move_linear(
    unit: UnitId,
    from: Point,
    to: Point,
    order: UnitOrder,
    state: UnitOrderState,
) -> Command {
    let mut steps = Vec::new();
    let mut x = i32::from(to.x) - i32::from(from.x);
    let mut y = i32::from(to.y) - i32::from(from.y);

    while x != 0 || y != 0 {
        let step_x = x.signum() as i8;
        let step_y = y.signum() as i8;
        steps.push(crate::geo::PathStep::new(step_x, step_y));
        x -= i32::from(step_x);
        y -= i32::from(step_y);
    }

    Command::Move(crate::input::Packet38Move {
        unit,
        order,
        state,
        grid: from,
        move_to: to,
        group_speed: 0,
        path: Some(MovePathData::Ground(steps)),
    })
}

/// Собирает команду 06 (конец хода команды).
pub fn command_finished_turn(team: TeamId) -> Command {
    Command::FinishedTurn(crate::input::Packet06FinishedTurn { team })
}

/// Собирает команду 52 (барьер конца хода).
pub fn command_end_turn_barrier(team: TeamId, counter: u8) -> Command {
    Command::EndTurnBarrier(crate::input::Packet52EndTurnBarrier { team, counter })
}

/// Порядок данных приказа, пришедших с пакетом (для тестов и отладки).
pub fn order_data_of(unit: &UnitInfo, state: &GameState) -> OrderData {
    state.order_data(unit.id)
}

/// Тема исследования по индексу — проверка формы пакетов 10/20 в тестах.
pub fn research_topic_at(index: u8) -> Option<ResearchTopic> {
    RESEARCH_TOPICS.get(index as usize).copied()
}

const RESEARCH_TOPICS: [ResearchTopic; 8] = [
    ResearchTopic::Attack,
    ResearchTopic::Shots,
    ResearchTopic::Range,
    ResearchTopic::Armor,
    ResearchTopic::Hits,
    ResearchTopic::Speed,
    ResearchTopic::Scan,
    ResearchTopic::Cost,
];

/// Тип юнита из `u16` (для сборки пакетов) — `None` вне `UNIT_END`.
pub fn unit_type_from_raw(raw: u16) -> Option<UnitType> {
    UnitType::try_from(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
    use crate::enums::TeamClan;
    use crate::input::ComplexSummaryEntry;
    use crate::input::{
        Packet13Seed, Packet41MoveFinished, Packet45CrcBarrier, Packet50StopUnit,
        Packet52EndTurnBarrier,
    };
    use crate::team::TeamKind;
    use crate::unit_values::ATTRIB_HITS;

    fn state_with_tank() -> (GameState, UnitId) {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        let id = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .expect("spawn");
        (state, id)
    }

    #[test]
    fn command_queue_is_fifo() {
        let (mut state, id) = state_with_tank();
        state
            .commands
            .push(Command::Stop(Packet50StopUnit { unit: id }));
        state.commands.push(Command::Seed(Packet13Seed { seed: 7 }));
        assert_eq!(state.commands.len(), 2);

        assert_eq!(state.apply_pending_commands(), 2);
        assert!(state.commands.is_empty());
        assert_eq!(state.commands.applied(), 2);
        assert_eq!(state.unit_events.len(), 1);
        assert_eq!(state.rng, crate::rng::Randomizer::new(7));
    }

    #[test]
    fn packet_08_p1_sets_orders_and_validates_range() {
        let (mut state, id) = state_with_tank();
        let p1 = payload_p1_for(
            UnitOrder::Sentry,
            UnitOrderState::ExecutingOrder,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        );
        assert_eq!(
            apply_command(&mut state, &command_order_p1(id, p1)),
            ApplyOutcome::Applied
        );
        let unit = state.unit(id).unwrap();
        assert_eq!(unit.orders, UnitOrder::Sentry);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert_eq!(unit.prior_orders, UnitOrder::Await);
    }

    #[test]
    fn packet_08_p3_applies_targets_and_payload_level_mismatch_is_malformed() {
        let (mut state, id) = state_with_tank();
        let p1 = payload_p1_for(
            UnitOrder::Move,
            UnitOrderState::Init,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        );
        let p3 = PayloadP3 {
            p2: PayloadP2 {
                p1,
                parent_unit: None,
                experience: 5,
            },
            move_to: Point::new(12, 13),
            fire_on: Point::new(14, 15),
            enemy_unit: None,
            transfer_cargo: 3,
            stealth_dice_roll: 9,
        };
        let command = Command::Order(Packet08Order {
            unit: id,
            payload: OrderPayload::P3(p3),
        });
        assert_eq!(apply_command(&mut state, &command), ApplyOutcome::Applied);

        let unit = state.unit(id).unwrap();
        assert_eq!(unit.experience, 5);
        assert_eq!(unit.transfer_cargo, 3);
        assert_eq!(unit.stealth_dice_roll, 9);
        assert_eq!(unit.target_grid, Point::new(12, 13));
        let data = state.order_data(id);
        assert_eq!(data.move_to, Point::new(12, 13));
        assert_eq!(data.fire_on, Point::new(14, 15));

        // P3-payload с приказом уровня P1 — malformed.
        let malformed = Command::Order(Packet08Order {
            unit: id,
            payload: OrderPayload::P3(PayloadP3 {
                p2: PayloadP2 {
                    p1: payload_p1_for(
                        UnitOrder::Sentry,
                        UnitOrderState::Init,
                        UnitOrder::Await,
                        UnitOrderState::ExecutingOrder,
                    ),
                    parent_unit: None,
                    experience: 0,
                },
                move_to: Point::ZERO,
                fire_on: Point::ZERO,
                enemy_unit: None,
                transfer_cargo: 0,
                stealth_dice_roll: 0,
            }),
        });
        assert_eq!(
            apply_command(&mut state, &malformed),
            ApplyOutcome::Malformed
        );
    }

    #[test]
    fn packet_38_builds_ground_path() {
        let (mut state, id) = state_with_tank();
        let command = command_move_linear(
            id,
            Point::new(10, 10),
            Point::new(13, 10),
            UnitOrder::Move,
            UnitOrderState::Init,
        );
        assert_eq!(apply_command(&mut state, &command), ApplyOutcome::Applied);
        let unit = state.unit(id).unwrap();
        match unit.path.as_ref().expect("path") {
            UnitPath::Ground(path) => {
                assert_eq!(path.end_x, 13);
                assert_eq!(path.end_y, 10);
                assert_eq!(path.steps.len(), 3);
                assert_eq!(path.step_index, 0);
            }
            other => panic!("ожидался наземный путь, получено {other:?}"),
        }
        assert_eq!(state.order_data(id).move_to, Point::new(13, 10));
    }

    #[test]
    fn packet_41_stops_movement_and_restores_await() {
        let (mut state, id) = state_with_tank();
        let command = command_move_linear(
            id,
            Point::new(10, 10),
            Point::new(20, 10),
            UnitOrder::Move,
            UnitOrderState::InProgress,
        );
        apply_command(&mut state, &command);
        assert!(state.unit(id).unwrap().path.is_some());

        let stop = Command::MoveFinished(Packet41MoveFinished {
            unit: id,
            mode: true,
            grid: Some(Point::new(11, 10)),
            speed: Some(5),
            group_speed: Some(0),
            shots: Some(2),
            move_fraction: Some(1),
        });
        assert_eq!(apply_command(&mut state, &stop), ApplyOutcome::Applied);

        let unit = state.unit(id).unwrap();
        assert!(unit.path.is_none());
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert_eq!(unit.grid, Point::new(11, 10));
        assert_eq!(unit.speed, 5);
        assert_eq!(unit.shots, 2);
        assert_eq!(unit.delayed_reaction, 1);
    }

    #[test]
    fn packet_50_creates_emergency_stop_event_processed_by_cycle() {
        let (mut state, id) = state_with_tank();
        // Начнём движение.
        let command = command_move_linear(
            id,
            Point::new(10, 10),
            Point::new(10, 15),
            UnitOrder::Move,
            UnitOrderState::InProgress,
        );
        apply_command(&mut state, &command);
        state.unit_mut(id).unwrap().state = UnitOrderState::InProgress;

        assert_eq!(
            apply_command(&mut state, &Command::Stop(Packet50StopUnit { unit: id })),
            ApplyOutcome::Applied
        );
        assert_eq!(state.unit_events.len(), 1);
        assert!(
            state.unit(id).unwrap().path.is_some(),
            "событие ещё не обработано"
        );

        state.tick();
        assert!(
            state.unit_events.is_empty(),
            "очередь событий очищается за тик"
        );
        assert!(state.unit(id).unwrap().path.is_none());
    }

    #[test]
    fn packet_06_sets_finished_turn_and_end_state() {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.play_mode = crate::enums::PlayMode::TurnBased;
        assert_eq!(
            apply_command(&mut state, &command_finished_turn(TeamId::RED)),
            ApplyOutcome::Applied
        );
        assert!(state.teams[0].finished_turn);
        assert_eq!(state.game_state, crate::enums::GameStateType::EndTurn);

        // Невалидный индекс команды (ALIEN=4) отвергается.
        assert_eq!(
            apply_command(&mut state, &command_finished_turn(TeamId::ALIEN)),
            ApplyOutcome::Ignored
        );
    }

    #[test]
    fn barriers_are_recorded_not_simulated() {
        let mut state = GameState::new();
        apply_command(&mut state, &command_end_turn_barrier(TeamId::GREEN, 7));
        assert_eq!(state.turn_indices[1], 7);
        apply_command(
            &mut state,
            &Command::CrcBarrier(Packet45CrcBarrier {
                team: TeamId::GREEN,
                next_turn_index: 3,
                crc16: 0xABCD,
            }),
        );
        assert_eq!(state.next_turn_indices[1], 3);
        assert_eq!(state.team_data_crc16[1], 0xABCD);
        assert!(!Command::EndTurnBarrier(Packet52EndTurnBarrier {
            team: TeamId::RED,
            counter: 1
        })
        .affects_simulation());
    }

    #[test]
    fn upgrade_packets_update_attributes_in_protocol_order() {
        let (mut state, id) = state_with_tank();
        let mut values = [0u16; UPGRADE_VALUE_COUNT];
        values[1] = 99; // hits — второй атрибут в порядке пакета.

        apply_command(
            &mut state,
            &Command::UpgradeUnit(crate::input::Packet20UpgradeUnit { unit: id, values }),
        );
        assert_eq!(
            state
                .unit(id)
                .unwrap()
                .base_values
                .get_attribute(ATTRIB_HITS),
            99
        );

        apply_command(
            &mut state,
            &Command::UpgradeUnitType(crate::input::Packet10UpgradeUnitType {
                team: TeamId::RED,
                unit_type: UnitType::Tank,
                values,
            }),
        );
        assert_eq!(
            state.team_units[0]
                .current_unit_values(UnitType::Tank as u16)
                .get_attribute(ATTRIB_HITS),
            99
        );
    }

    #[test]
    fn complex_packets_apply_six_fields() {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        let complex_id = state.team_units[0].create_complex();
        apply_command(
            &mut state,
            &Command::ComplexOrder(crate::input::Packet11ComplexOrder {
                team: TeamId::RED,
                complex_id: complex_id.0 as u16,
                material: 5,
                fuel: 6,
                gold: 7,
                power: 8,
                workers: 9,
                buildings: 10,
            }),
        );
        let complex = state
            .complex(TeamId::RED, complex_id.0 as u16)
            .expect("complex");
        assert_eq!(complex.material, 5);
        assert_eq!(complex.buildings, 10);

        apply_command(
            &mut state,
            &Command::ComplexSummary(crate::input::Packet53ComplexSummary {
                team: TeamId::RED,
                turn: 3,
                complexes: vec![ComplexSummaryEntry {
                    complex_id: complex_id.0 as u16,
                    values: [1, 2, 3, 4, 5, 6],
                }],
            }),
        );
        let complex = state
            .complex(TeamId::RED, complex_id.0 as u16)
            .expect("complex");
        assert_eq!(complex.material, 1);
        assert_eq!(complex.buildings, 6);
    }

    #[test]
    fn deploy_is_reported_unsupported_until_unit_flag_data_exists() {
        let mut state = GameState::new();
        let outcome = apply_command(
            &mut state,
            &Command::Deploy(crate::input::Packet14DeployUnit {
                team: TeamId::RED,
                unit_type: UnitType::Tank,
                grid: Point::new(4, 4),
            }),
        );
        assert_eq!(outcome, ApplyOutcome::Unsupported);
    }

    #[test]
    fn rename_and_unknown_unit() {
        let (mut state, id) = state_with_tank();
        apply_command(
            &mut state,
            &Command::Rename(crate::input::Packet43Rename {
                unit: id,
                name: "Tank 1".into(),
            }),
        );
        assert_eq!(state.unit(id).unwrap().name, "Tank 1");

        assert_eq!(
            apply_command(
                &mut state,
                &Command::Rename(crate::input::Packet43Rename {
                    unit: UnitId(0x7FFF),
                    name: "x".into(),
                })
            ),
            ApplyOutcome::UnknownUnit
        );
    }

    #[test]
    fn chat_is_logged() {
        let mut state = GameState::new();
        apply_command(
            &mut state,
            &Command::Chat(crate::input::Packet18Chat {
                sender: TeamId::BLUE,
                message: "hello".into(),
            }),
        );
        assert_eq!(state.chat_log.len(), 1);
        assert_eq!(state.chat_log[0].0, TeamId::BLUE);
    }

    #[test]
    fn unit_event_helpers_are_consistent() {
        let event = UnitEvent::EmergencyStop { unit: UnitId(5) };
        assert_eq!(event.unit(), UnitId(5));
        assert_eq!(unit_type_from_raw(0x33), Some(UnitType::Tank));
        assert_eq!(unit_type_from_raw(0xFFFF), None);
        assert_eq!(research_topic_at(0), Some(ResearchTopic::Attack));
        assert_eq!(research_topic_at(8), None);
    }

    // -----------------------------------------------------------------------
    // 54 — применение авторитетного состояния команды
    // -----------------------------------------------------------------------

    fn state_with_remote_team() -> (GameState, UnitId, UnitId, UnitId) {
        let mut state = GameState::new();
        let green = TeamId::GREEN.0 as usize;
        state.teams[green].team_type = TeamKind::Remote;
        state.team_units[green] = crate::team::TeamUnits::with_clan(TeamClan::Random);

        let tank = state
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(5, 5), 0)
            .expect("tank");
        let scout = state
            .spawn_unit(UnitType::Scout, TeamId::GREEN, Point::new(6, 5), 0)
            .expect("scout");
        let tower = state
            .spawn_unit(UnitType::GoldRefinery, TeamId::GREEN, Point::new(8, 8), 0)
            .expect("tower");
        (state, tank, scout, tower)
    }

    fn unit_state_for_tank() -> UnitStatePacket {
        UnitStatePacket {
            orders: UnitOrder::Sentry as u8,
            state: UnitOrderState::ExecutingOrder as u8,
            prior_orders: UnitOrder::Await as u8,
            prior_state: UnitOrderState::ExecutingOrder as u8,
            disabled_reaction_fire: true,
            parent_unit_id: 0,
            experience: 11,
            move_to_grid_x: 50,
            move_to_grid_y: 51,
            fire_on_grid_x: 52,
            fire_on_grid_y: 53,
            enemy_unit_id: 0,
            transfer_cargo: -4,
            stealth_dice_roll: 3,
            total_mining: 1,
            raw_mining: 2,
            fuel_mining: 3,
            gold_mining: 4,
            build_time: 5,
            build_rate: 6,
            unit_type: UnitType::Tank as u16,
            unit_id: 7,
            grid_x: 40,
            grid_y: 41,
            team: TeamId::GREEN.0,
            hits: 77,
            speed: 8,
            shots: 2,
            storage: -9,
            ammo: 10,
        }
    }

    fn tail_for_green() -> TeamStateSyncTail {
        let mut tail = TeamStateSyncTail::default();
        tail.gold = 500;
        tail.gold_spent_on_upgrades = 60;
        tail.number_of_objects_created = 12;
        tail.unit_counters[UnitType::Tank as usize] = 13;
        tail.research[0] = ResearchTopicInfo {
            research_level: 1,
            turns_to_complete: 2,
            allocation: 3,
        };
        tail
    }

    #[test]
    fn packet_54_applies_state_and_removes_extra_units() {
        let (mut state, tank, scout, tower) = state_with_remote_team();
        let packet = StateSyncPacket {
            team: TeamId::GREEN,
            sync_index: 1,
            is_last: true,
            units: vec![(tank, unit_state_for_tank())],
            tail: Some(tail_for_green()),
        };

        assert_eq!(apply_state_sync(&mut state, &packet), ApplyOutcome::Applied);

        let unit = state.unit(tank).expect("tank survives");
        assert_eq!(unit.orders, UnitOrder::Sentry);
        assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
        assert!(unit.disabled_reaction_fire);
        assert_eq!(unit.grid, Point::new(40, 41));
        assert_eq!(unit.hits, 77);
        assert_eq!(unit.speed, 8);
        assert_eq!(unit.shots, 2);
        assert_eq!(unit.storage, -9);
        assert_eq!(unit.ammo, 10);
        assert_eq!(unit.experience, 11);
        assert_eq!(unit.build_rate, 6);
        assert_eq!(unit.unit_id, 7);
        let data = state.order_data(tank);
        assert_eq!(data.move_to, Point::new(50, 51));
        assert_eq!(data.fire_on, Point::new(52, 53));
        assert_eq!(unit.target_grid, Point::new(50, 51));

        // Лишний мобильный юнит удалён, здание — нет (`Remote_RemoveExtraUnits`).
        assert!(
            !state.has_unit(scout),
            "scout отсутствует у владельца — удалён"
        );
        assert!(state.has_unit(tower), "здание не удаляется");

        // Хвост последнего чанка выровнял экономику и счётчики id.
        let green = TeamId::GREEN.0 as usize;
        assert_eq!(state.team_units[green].gold, 500);
        assert_eq!(state.teams[green].stats_gold_spent_on_upgrades, 60);
        assert_eq!(state.teams[green].number_of_objects_created, 12);
        assert_eq!(
            state.teams[green].unit_counters[UnitType::Tank as usize],
            13
        );
        assert_eq!(state.teams[green].research_topics[0].research_level, 1);
        assert_eq!(state.teams[green].research_topics[0].turns_to_complete, 2);
        assert_eq!(state.teams[green].research_topics[0].allocation, 3);
        assert_eq!(state.state_sync_index(TeamId::GREEN), 1);
    }

    #[test]
    fn packet_54_chunks_only_complete_on_last() {
        let (mut state, tank, _scout, _tower) = state_with_remote_team();
        let first = StateSyncPacket {
            team: TeamId::GREEN,
            sync_index: 3,
            is_last: false,
            units: vec![(tank, unit_state_for_tank())],
            tail: None,
        };
        assert_eq!(apply_state_sync(&mut state, &first), ApplyOutcome::Applied);
        assert_eq!(
            state.state_sync_index(TeamId::GREEN),
            0,
            "не последний чанк"
        );

        let last = StateSyncPacket {
            team: TeamId::GREEN,
            sync_index: 3,
            is_last: true,
            units: Vec::new(),
            tail: Some(TeamStateSyncTail::default()),
        };
        assert_eq!(apply_state_sync(&mut state, &last), ApplyOutcome::Applied);
        assert_eq!(state.state_sync_index(TeamId::GREEN), 3);
    }

    #[test]
    fn packet_54_ignores_non_remote_teams_and_out_of_range_orders() {
        // Своя (Player) команда: пакет 54 не применяется.
        let (mut state, tank, _scout, _tower) = state_with_remote_team();
        state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
        let packet = StateSyncPacket {
            team: TeamId::RED,
            sync_index: 1,
            is_last: true,
            units: vec![(tank, unit_state_for_tank())],
            tail: Some(TeamStateSyncTail::default()),
        };
        assert_eq!(apply_state_sync(&mut state, &packet), ApplyOutcome::Ignored);
        assert_eq!(state.state_sync_index(TeamId::RED), 0);

        // Приказ вне ORDER_COUNT_MAX не применяется (`Remote_ApplyUnitState`).
        let before = state.unit(tank).expect("tank").orders;
        let mut bad = unit_state_for_tank();
        bad.orders = crate::enums::ORDER_COUNT_MAX;
        let packet = StateSyncPacket {
            team: TeamId::GREEN,
            sync_index: 4,
            is_last: true,
            units: vec![(tank, bad)],
            tail: Some(TeamStateSyncTail::default()),
        };
        assert_eq!(apply_state_sync(&mut state, &packet), ApplyOutcome::Applied);
        let unit = state.unit(tank).expect("tank");
        assert_eq!(unit.orders, before, "невалидный приказ не применён");
        assert_eq!(state.state_sync_index(TeamId::GREEN), 4);
    }
}
