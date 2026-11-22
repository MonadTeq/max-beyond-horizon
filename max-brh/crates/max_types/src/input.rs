//! Входная модель команд — пакеты протокола как чистые данные.
//!
//! Оракул: `max-cpp-port/src/remote.cpp` (обработчики `Remote_ReceiveNetPacket_*`
//! и процессоры приказов `Remote_OrderProcessor{0..5}_Read`), `max-brh-docs/protocol.md`
//! §8.4 (форматы полей на проводе) и DESIGN.md §2.4.
//!
//! Сетевое кодирование (ENet, little-endian, `NetPacket`) — **не здесь**: это
//! `max_net`/`max_app`. `max_core` получает уже разобранные команды и применяет
//! их к `GameState` (`max_core::game`) в порядке поступления
//! (`max_core::events`). Поэтому все структуры модуля — plain data: их можно
//! строить напрямую из тестов, из max_net и из скриптов-реплеев (DESIGN §8.2).
//!
//! ## Процессоры пакета 08
//!
//! Как и в C++, формат payload'а выбирается по *новому* приказу юнита
//! (`Remote_OrderProcessors[unit->GetOrder()]`), процессоры вложены:
//! `P3 ⊃ P2 ⊃ P1`, `P4 ⊃ P3`, `P5 ⊃ P3`; [`OrderPayload`] повторяет вложенность
//! полями-структурами, чтобы сериализация на стороне сети шла в том же порядке
//! байт, что и в C++ (`Remote_OrderProcessor2_Read` сначала вызывает
//! `Remote_OrderProcessor1_Read` и т.д.).
//!
//! ## Расхождения с C++-портом (зафиксированы осознанно)
//!
//! * Пакет **41** в C++-порте несёт только `unit_id` и `mode` (см.
//!   `Remote_SendNetPacket_41`), а `max-brh-docs/protocol.md` §8.4 описывает ещё
//!   `grid_x/grid_y/speed/group_speed/shots/move_fraction`. В
//!   [`Packet41MoveFinished`] эти поля есть (протокол — контракт с max_net), но
//!   применяются только если отличаются от уже известного состояния; см.
//!   `max_core::events::apply_command`.
//! * Пакет **38** в C++-порте не несёт `grid_x/grid_y`, а `max-brh-docs/protocol.md`
//!   §8.4 их описывает; поле оставлено в структуре для совместимости и
//!   применяется как «текущая клетка владельца» (C++: `Remote_RelocateUnit`).
//! * Пакет **53** (итоги комплексов) в C++-порте не реализован вовсе; в Rust —
//!   данные + применение (DESIGN §2.4 указал его как часть входной модели).

use crate::enums::{PlayMode, UnitOrder, UnitOrderState, UnitType};
use crate::geo::{PathStep, Point};
use crate::team::TeamId;

/// Размер массива команд в сетевых структурах (`TRANSPORT_MAX_TEAM_COUNT`).
pub const MAX_TEAM_COUNT: usize = 4;

/// Число атрибутов, которые несут пакеты апгрейдов 10/20 (13 значений
/// `u16` в фиксированном порядке, `max-brh-docs/protocol.md` §8.4).
pub const UPGRADE_VALUE_COUNT: usize = 13;

/// Уровень процессора payload'а пакета 08 (`Remote_OrderProcessor{0..5}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PayloadLevel {
    /// `ORDER_17` — пусто (`Remote_OrderProcessor0_Read`).
    Empty = 0,
    /// `P1`: order/state/prior/disabled_reaction_fire.
    P1 = 1,
    /// `P2` = `P1` + parent/experience.
    P2 = 2,
    /// `P3` = `P2` + move/fire/enemy/transfer/stealth.
    P3 = 3,
    /// `P4` = `P3` + build fields.
    P4 = 4,
    /// `P5` = `P3` + mining fields.
    P5 = 5,
}

impl PayloadLevel {
    /// Уровень процессора для приказа — точная копия `switch` из
    /// `Remote_Init` (`remote.cpp:661-724`).
    pub const fn for_order(order: UnitOrder) -> Self {
        use UnitOrder::*;
        match order {
            // ORDER_17: «unused order slots»
            Order17 => Self::Empty,
            // P1
            PowerOn | PowerOff | Explode | Sentry | Land | TakeOff | Load | Idle | HaltBuilding
            | AwaitScaling | AwaitTapePositioning | LayMine | HaltBuilding2 => Self::P1,
            // P4 (содержит P3)
            Transform | Build | Clear | Disable => Self::P4,
            // P3
            Await | Move | Fire | Activate | Transfer | AwaitStealUnit | AwaitDisableUnit | MoveToUnit
            | MoveToAttack => Self::P3,
            // P5 (содержит P3)
            NewAllocate => Self::P5,
            // P2
            Unload | Repair | Reload | Upgrade => Self::P2,
        }
    }
}

/// Общие поля всех процессоров, кроме `P0` — C++ `Remote_OrderProcessor1_Read`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadP1 {
    /// Новый приказ (`< ORDER_COUNT_MAX`, иначе пакет отвергается).
    pub order: UnitOrder,
    /// Новое состояние приказа.
    pub state: UnitOrderState,
    /// Отложенный приказ (`< ORDER_COUNT_MAX`).
    pub prior_order: UnitOrder,
    /// Отложенное состояние.
    pub prior_state: UnitOrderState,
    /// Ответный огонь выключен (`bool` на проводе).
    pub disabled_reaction_fire: bool,
}

/// `P2` — C++ `Remote_OrderProcessor2_Read`: `P1` + родитель и опыт.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadP2 {
    /// Поля `P1`.
    pub p1: PayloadP1,
    /// `parent_unit_id` (на проводе `u32`, 0 = нет родителя); в модели —
    /// `Option<UnitId>`.
    pub parent_unit: Option<crate::unit::UnitId>,
    /// `experience i16`.
    pub experience: i16,
}

/// `P3` — C++ `Remote_OrderProcessor3_Read`: `P2` + цели и груз.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadP3 {
    /// Поля `P2`.
    pub p2: PayloadP2,
    /// `move_to_grid_x/y` (C++ runtime-поля, в V70 — один `target_grid`).
    pub move_to: Point,
    /// `fire_on_grid_x/y`.
    pub fire_on: Point,
    /// `enemy_unit_id` (0 = нет).
    pub enemy_unit: Option<crate::unit::UnitId>,
    /// `transfer_cargo i16`.
    pub transfer_cargo: i16,
    /// `stealth_dice_roll u8`.
    pub stealth_dice_roll: u8,
}

/// `P4` — C++ `Remote_OrderProcessor4_Read`: `P3` + поля постройки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadP4 {
    /// Поля `P3`.
    pub p3: PayloadP3,
    /// `repeat_build` (`bool`).
    pub repeat_build: bool,
    /// `build_time u8`.
    pub build_time: u8,
    /// `build_rate u16`.
    pub build_rate: u16,
    /// Очередь постройки (`unit_count u32` + `unit_count × ResourceID u16`).
    pub build_list: Vec<UnitType>,
}

/// `P5` — C++ `Remote_OrderProcessor5_Read`: `P3` + уровни добычи.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadP5 {
    /// Поля `P3`.
    pub p3: PayloadP3,
    /// `total_mining u8`.
    pub total_mining: u8,
    /// `raw_mining u8`.
    pub raw_mining: u8,
    /// `fuel_mining u8`.
    pub fuel_mining: u8,
    /// `gold_mining u8`.
    pub gold_mining: u8,
}

/// Payload пакета 08 (один из шести процессоров).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderPayload {
    /// `P0`: `ORDER_17` — пусто.
    Empty,
    /// `P1`.
    P1(PayloadP1),
    /// `P2`.
    P2(PayloadP2),
    /// `P3`.
    P3(PayloadP3),
    /// `P4`.
    P4(PayloadP4),
    /// `P5`.
    P5(PayloadP5),
}

impl OrderPayload {
    /// Уровень процессора этого payload'а.
    pub const fn level(&self) -> PayloadLevel {
        match self {
            Self::Empty => PayloadLevel::Empty,
            Self::P1(_) => PayloadLevel::P1,
            Self::P2(_) => PayloadLevel::P2,
            Self::P3(_) => PayloadLevel::P3,
            Self::P4(_) => PayloadLevel::P4,
            Self::P5(_) => PayloadLevel::P5,
        }
    }

    /// Общие поля `P1`, если они есть у процессора.
    pub const fn p1(&self) -> Option<&PayloadP1> {
        match self {
            Self::Empty => None,
            Self::P1(p1) => Some(p1),
            Self::P2(p2) => Some(&p2.p1),
            Self::P3(p3) => Some(&p3.p2.p1),
            Self::P4(p4) => Some(&p4.p3.p2.p1),
            Self::P5(p5) => Some(&p5.p3.p2.p1),
        }
    }

    /// Новый приказ (если payload его несёт).
    pub fn order(&self) -> Option<UnitOrder> {
        self.p1().map(|p1| p1.order)
    }

    /// Проверка «процессор соответствует приказу» — как в C++: формат payload'а
    /// выбирается по новому приказу юнита (`Remote_ReceiveNetPacket_08`), поэтому
    /// несовпадение уровня и приказа считается malformed-пакетом.
    pub fn matches_order(&self, order: UnitOrder) -> bool {
        self.level() == PayloadLevel::for_order(order) && self.order() == Some(order)
    }
}

/// Пакет **08** — приказ юниту заказа 08 (`C→S multicast`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet08Order {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// Payload процессора.
    pub payload: OrderPayload,
}

/// Данные пути в пакете 38.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MovePathData {
    /// Наземный путь: `steps_count × PathStep { i8, i8 }`.
    Ground(Vec<PathStep>),
    /// Воздушный путь (шагов ровно 1): линейный отрезок в пикселях.
    Air {
        /// `end_x i16`.
        end_x: i16,
        /// `end_y i16`.
        end_y: i16,
        /// `distance_x i32`.
        distance_x: i32,
        /// `distance_y i32`.
        distance_y: i32,
        /// `euclidean_distance i16`.
        euclidean_distance: i16,
        /// `speed u16` (остаток хода на момент отправки).
        speed: u16,
        /// `move_fraction u8`.
        move_fraction: u8,
        /// `max_velocity u8`.
        max_velocity: u8,
    },
}

/// Пакет **38** — движение с готовым путём (владелец пути рассылает его пирам).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet38Move {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// `order u8` (новый приказ).
    pub order: UnitOrder,
    /// `order_state u8`.
    pub state: UnitOrderState,
    /// `grid_x/grid_y` — клетка владельца на момент отправки (перемещение
    /// юнита у получателя, `Remote_RelocateUnit`).
    pub grid: Point,
    /// `move_to_grid_x/y`.
    pub move_to: Point,
    /// `group_speed u16`.
    pub group_speed: u16,
    /// Путь (`steps_count`; 0 — пути нет).
    pub path: Option<MovePathData>,
}

/// Пакет **41** — завершение движения (`BlockedOnPathRequest` у получателя).
///
/// В C++-порте передаются только `unit_id` + `mode`; остальные поля — из
/// `max-brh-docs/protocol.md` §8.4 (см. шапку модуля). Они `Option`, чтобы отправитель
/// мог собрать «портовую» (короткую) форму пакета: `None` = поле не передано и
/// не применяется.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet41MoveFinished {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// `mode u8(bool)` — «реакции после движения» (`MoveFinished(mode)`).
    pub mode: bool,
    /// `grid_x i16`.
    pub grid: Option<Point>,
    /// `speed u16`.
    pub speed: Option<u16>,
    /// `group_speed u16`.
    pub group_speed: Option<u16>,
    /// `shots u8`.
    pub shots: Option<u8>,
    /// `move_fraction u8`.
    pub move_fraction: Option<u8>,
}

impl Packet41MoveFinished {
    /// Короткая форма C++-порта: только `unit_id` и `mode`.
    pub const fn port_form(unit: crate::unit::UnitId, mode: bool) -> Self {
        Self {
            unit,
            mode,
            grid: None,
            speed: None,
            group_speed: None,
            shots: None,
            move_fraction: None,
        }
    }
}

/// Пакет **50** — аварийный стоп юнита (создаёт `UnitEventEmergencyStop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet50StopUnit {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
}

/// Пакет **06** — команда закончила ход.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet06FinishedTurn {
    /// `team u16`.
    pub team: TeamId,
}

/// Пакет **52** — барьер конца хода (`turn index` команды).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet52EndTurnBarrier {
    /// `team u16`.
    pub team: TeamId,
    /// `counter u8`.
    pub counter: u8,
}

/// Пакет **45** — CRC-барьер (номер следующего хода + CRC16 команды).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet45CrcBarrier {
    /// `team u16`.
    pub team: TeamId,
    /// `next_turn_index u8`.
    pub next_turn_index: u8,
    /// `crc16 u16`.
    pub crc16: u16,
}

/// Пакет **00** — барьер начала хода (покадровый счётчик команды).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet00TurnBarrier {
    /// `team u16`.
    pub team: TeamId,
    /// `counter u8`.
    pub counter: u8,
}

/// Пакет **01** — покадровый барьер (real-time pacing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet01FrameBarrier {
    /// `team u16`.
    pub team: TeamId,
    /// `counter u8` (6 бит).
    pub counter: u8,
}

/// Пакет **18** — чат в игре.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet18Chat {
    /// `sender_team u16`.
    pub sender: TeamId,
    /// `message SmartString`.
    pub message: String,
}

/// Пакет **11** — приказ комплексу (поля `Complex::WritePacket`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet11ComplexOrder {
    /// `team u16`.
    pub team: TeamId,
    /// `complex_id u16`.
    pub complex_id: u16,
    /// `material i16`.
    pub material: i16,
    /// `fuel i16`.
    pub fuel: i16,
    /// `gold i16`.
    pub gold: i16,
    /// `power i16`.
    pub power: i16,
    /// `workers i16`.
    pub workers: i16,
    /// `buildings i16`.
    pub buildings: i16,
}

/// Итоги одного комплекса в пакете 53.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComplexSummaryEntry {
    /// `complex_id u16`.
    pub complex_id: u16,
    /// Шесть полей `i16` в порядке пакета 11/53
    /// (`material, fuel, gold, power, workers, buildings`).
    pub values: [i16; 6],
}

/// Пакет **53** — итоги комплексов команды на начало хода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet53ComplexSummary {
    /// `team u16`.
    pub team: TeamId,
    /// `turn i32`.
    pub turn: i32,
    /// `complex_count u16` + записи.
    pub complexes: Vec<ComplexSummaryEntry>,
}

/// Пакет **10** — апгрейд типа юнита команды.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet10UpgradeUnitType {
    /// `team u16`.
    pub team: TeamId,
    /// `unit_type u16` (`ResourceID`).
    pub unit_type: UnitType,
    /// 13 значений в порядке `turns, hits, armor, attack, speed, range, rounds,
    /// move_and_fire, scan, storage, ammo, attack_radius, agent_adjust`.
    pub values: [u16; UPGRADE_VALUE_COUNT],
}

/// Пакет **20** — апгрейд конкретного юнита.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet20UpgradeUnit {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// 13 значений (порядок как в пакете 10).
    pub values: [u16; UPGRADE_VALUE_COUNT],
}

/// Пакет **43** — переименование юнита.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet43Rename {
    /// `unit_id u16`.
    pub unit: crate::unit::UnitId,
    /// `name SmartString`.
    pub name: String,
}

/// Пакет **13** — seed RNG (единственная точка сидирования симуляции).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet13Seed {
    /// `rng_seed u32`.
    pub seed: u32,
}

/// Пакет **14** — высадка юнита (quick build / deployment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet14DeployUnit {
    /// `team u16`.
    pub team: TeamId,
    /// `unit_type u16`.
    pub unit_type: UnitType,
    /// `grid_x i32`.
    pub grid: Point,
}

/// Снабжение миссии — запись пакета 12.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissionSupplyEntry {
    /// `unit_type u16`.
    pub unit_type: UnitType,
    /// `cargo u16`.
    pub cargo: u16,
}

/// Пакет **12** — выбор точки высадки и снабжение миссии.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet12MissionSupply {
    /// `team u16`.
    pub team: TeamId,
    /// `round u8`.
    pub round: u8,
    /// `start_gold u16`.
    pub start_gold: u16,
    /// `team_gold u16`.
    pub team_gold: u16,
    /// `unit_count u32` + записи.
    pub units: Vec<MissionSupplyEntry>,
    /// `gold_spent_on_upgrades u32`.
    pub gold_spent_on_upgrades: u32,
    /// `starting_position Point`.
    pub starting_position: Point,
    /// `proximity_alert_ack u8`.
    pub proximity_alert_ack: u8,
}

/// Идентификатор типа пакета (`max-brh-docs/protocol.md` §8.4, номера после
/// `TRANSPORT_APPL_PACKET_ID`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum PacketKind {
    /// Барьер начала хода.
    BarrierTurn = 0x00,
    /// Покадровый барьер.
    BarrierFrame = 0x01,
    /// Конец хода команды.
    FinishedTurn = 0x06,
    /// Приказ юниту.
    OrderUnit = 0x08,
    /// Апгрейд типа юнита.
    UpgradeUnitType = 0x0A,
    /// Приказ комплексу.
    ComplexOrder = 0x0B,
    /// Точка высадки и снабжение миссии.
    MissionSupply = 0x0C,
    /// Seed RNG.
    Seed = 0x0D,
    /// Высадка юнита.
    DeployUnit = 0x0E,
    /// Чат.
    Chat = 0x12,
    /// Апгрейд конкретного юнита.
    UpgradeUnit = 0x14,
    /// Движение с путём.
    MoveWithPath = 0x26,
    /// Завершение движения.
    MoveFinished = 0x29,
    /// Переименование.
    Rename = 0x2B,
    /// CRC-барьер.
    CrcBarrier = 0x2D,
    /// Аварийный стоп.
    StopUnit = 0x32,
    /// Барьер конца хода.
    EndTurnBarrier = 0x34,
    /// Итоги комплексов.
    ComplexSummary = 0x35,
}

/// Одна входная команда — результат разбора одного APPL-пакета.
///
/// Применение — `max_core::events::apply_command`, строго в порядке получения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// 08.
    Order(Packet08Order),
    /// 38.
    Move(Packet38Move),
    /// 41.
    MoveFinished(Packet41MoveFinished),
    /// 50.
    Stop(Packet50StopUnit),
    /// 06.
    FinishedTurn(Packet06FinishedTurn),
    /// 52.
    EndTurnBarrier(Packet52EndTurnBarrier),
    /// 45.
    CrcBarrier(Packet45CrcBarrier),
    /// 00.
    TurnBarrier(Packet00TurnBarrier),
    /// 01.
    FrameBarrier(Packet01FrameBarrier),
    /// 18.
    Chat(Packet18Chat),
    /// 11.
    ComplexOrder(Packet11ComplexOrder),
    /// 53.
    ComplexSummary(Packet53ComplexSummary),
    /// 10.
    UpgradeUnitType(Packet10UpgradeUnitType),
    /// 20.
    UpgradeUnit(Packet20UpgradeUnit),
    /// 43.
    Rename(Packet43Rename),
    /// 13.
    Seed(Packet13Seed),
    /// 14.
    Deploy(Packet14DeployUnit),
    /// 12.
    MissionSupply(Packet12MissionSupply),
}

impl Command {
    /// Тип пакета команды.
    pub const fn kind(&self) -> PacketKind {
        match self {
            Self::Order(_) => PacketKind::OrderUnit,
            Self::Move(_) => PacketKind::MoveWithPath,
            Self::MoveFinished(_) => PacketKind::MoveFinished,
            Self::Stop(_) => PacketKind::StopUnit,
            Self::FinishedTurn(_) => PacketKind::FinishedTurn,
            Self::EndTurnBarrier(_) => PacketKind::EndTurnBarrier,
            Self::CrcBarrier(_) => PacketKind::CrcBarrier,
            Self::TurnBarrier(_) => PacketKind::BarrierTurn,
            Self::FrameBarrier(_) => PacketKind::BarrierFrame,
            Self::Chat(_) => PacketKind::Chat,
            Self::ComplexOrder(_) => PacketKind::ComplexOrder,
            Self::ComplexSummary(_) => PacketKind::ComplexSummary,
            Self::UpgradeUnitType(_) => PacketKind::UpgradeUnitType,
            Self::UpgradeUnit(_) => PacketKind::UpgradeUnit,
            Self::Rename(_) => PacketKind::Rename,
            Self::Seed(_) => PacketKind::Seed,
            Self::Deploy(_) => PacketKind::DeployUnit,
            Self::MissionSupply(_) => PacketKind::MissionSupply,
        }
    }

    /// Влияет ли команда на игровую симуляцию (а не только на барьеры/UI).
    ///
    /// Используется тестами детерминизма и диагностикой: барьерные пакеты
    /// (00/01/45/52) — это факты сети для продвижения фаз, они не меняют
    /// состояние юнитов.
    pub const fn affects_simulation(&self) -> bool {
        !matches!(
            self,
            Self::EndTurnBarrier(_)
                | Self::CrcBarrier(_)
                | Self::TurnBarrier(_)
                | Self::FrameBarrier(_)
                | Self::Chat(_)
        )
    }
}

/// Проверка `Remote_IsValidTeamIndex` (`remote.cpp`: `entity_id <
/// PLAYER_TEAM_MAX`), но без ALIEN-слота: сетевые пакеты адресуются только
/// игровым командам 0..3.
pub fn is_valid_team_index(team: TeamId) -> bool {
    (team.0 as usize) < MAX_TEAM_COUNT
}

/// Проверка диапазона приказа (`order < ORDER_COUNT_MAX`, включая prior).
///
/// В C++ приказ на проводе — сырой `u8`, и приёмник отвергает значения
/// `>= ORDER_COUNT_MAX` (`remote.cpp:394`). В Rust-модели приказ —
/// `#[repr(u8)]`-enum, поэтому значение вне диапазона существовать не может;
/// функция сохранена как явная точка паритета с проверкой C++.
pub fn is_valid_order(order: UnitOrder) -> bool {
    (order as u8) < crate::enums::ORDER_COUNT_MAX
}

/// Режим игры, при котором весь ход одной команды обрабатывается до конца
/// (`PLAY_MODE_TURN_BASED`).
pub const PLAY_MODE_TURN_BASED: PlayMode = PlayMode::TurnBased;

/// Режим одновременных ходов (`PLAY_MODE_SIMULTANEOUS_MOVES`).
pub const PLAY_MODE_SIMULTANEOUS_MOVES: PlayMode = PlayMode::SimultaneousMoves;

#[cfg(test)]
mod tests {
    use super::*;

    fn p1(order: UnitOrder) -> PayloadP1 {
        PayloadP1 {
            order,
            state: UnitOrderState::Init,
            prior_order: UnitOrder::Await,
            prior_state: UnitOrderState::ExecutingOrder,
            disabled_reaction_fire: false,
        }
    }

    fn p2(order: UnitOrder) -> PayloadP2 {
        PayloadP2 {
            p1: p1(order),
            parent_unit: None,
            experience: 0,
        }
    }

    fn p3(order: UnitOrder) -> PayloadP3 {
        PayloadP3 {
            p2: p2(order),
            move_to: Point::new(1, 2),
            fire_on: Point::new(3, 4),
            enemy_unit: None,
            transfer_cargo: 0,
            stealth_dice_roll: 0,
        }
    }

    #[test]
    fn processor_levels_match_cpp_init_table() {
        use UnitOrder::*;
        // Точная копия switch из Remote_Init (remote.cpp:661-724).
        assert_eq!(PayloadLevel::for_order(Order17), PayloadLevel::Empty);
        for order in [
            PowerOn,
            PowerOff,
            Explode,
            Sentry,
            Land,
            TakeOff,
            Load,
            Idle,
            HaltBuilding,
            AwaitScaling,
            AwaitTapePositioning,
            LayMine,
            HaltBuilding2,
        ] {
            assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P1, "{order:?}");
        }
        for order in [Transform, Build, Clear, Disable] {
            assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P4, "{order:?}");
        }
        for order in [
            Await,
            Move,
            Fire,
            Activate,
            Transfer,
            AwaitStealUnit,
            AwaitDisableUnit,
            MoveToUnit,
            MoveToAttack,
        ] {
            assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P3, "{order:?}");
        }
        assert_eq!(
            PayloadLevel::for_order(NewAllocate),
            PayloadLevel::P5
        );
        for order in [Unload, Repair, Reload, Upgrade] {
            assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P2, "{order:?}");
        }
        // Все 32 приказа распределены (нет забытых вариантов).
        for order in UnitOrder::ALL {
            let _ = PayloadLevel::for_order(*order);
        }
    }

    #[test]
    fn payload_nesting_exposes_p1_fields() {
        let payload = OrderPayload::P4(PayloadP4 {
            p3: p3(UnitOrder::Build),
            repeat_build: true,
            build_time: 3,
            build_rate: 2,
            build_list: vec![UnitType::Tank],
        });
        assert_eq!(payload.level(), PayloadLevel::P4);
        assert_eq!(payload.order(), Some(UnitOrder::Build));
        assert!(payload.matches_order(UnitOrder::Build));
        assert!(!payload.matches_order(UnitOrder::Move));

        let empty = OrderPayload::Empty;
        assert_eq!(empty.level(), PayloadLevel::Empty);
        assert_eq!(empty.order(), None);
        assert!(!empty.matches_order(UnitOrder::Order17));
    }

    #[test]
    fn command_kinds_match_protocol_numbers() {
        let stop = Command::Stop(Packet50StopUnit {
            unit: crate::unit::UnitId(1),
        });
        assert_eq!(stop.kind(), PacketKind::StopUnit);
        assert_eq!(PacketKind::StopUnit as u8, 0x32);
        assert!(stop.affects_simulation());

        let chat = Command::Chat(Packet18Chat {
            sender: TeamId(0),
            message: "hello".into(),
        });
        assert!(!chat.affects_simulation());
        assert_eq!(PacketKind::Chat as u8, 0x12);
    }

    #[test]
    fn team_and_order_validation() {
        assert!(is_valid_team_index(TeamId(0)));
        assert!(is_valid_team_index(TeamId(3)));
        assert!(!is_valid_team_index(TeamId(4)));
        assert!(!is_valid_team_index(TeamId(5)));
        assert!(is_valid_order(UnitOrder::HaltBuilding2));
        assert_eq!(PLAY_MODE_TURN_BASED, PlayMode::TurnBased);
        assert_eq!(PLAY_MODE_SIMULTANEOUS_MOVES, PlayMode::SimultaneousMoves);
    }
}
