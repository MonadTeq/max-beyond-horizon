//! DTO авторитетного состояния команды (пакет 54) — чистые данные.
//!
//! Здесь только протокольные структуры, не читающие данных симуляции:
//! [`UnitStatePacket`], [`TeamStateSyncTail`], [`StateSyncPacket`]. Их
//! применение (`apply_unit_state`, `apply_state_sync`) остаётся в
//! `max_core::events`, потому что работает с `GameState`.
//!
//! Оракул: `max-cpp-port/src/remote.cpp` (`Remote_SendUnitStates`,
//! `Remote_ReceiveNetPacket_54`, `Remote_ApplyUnitState`),
//! `max-brh-docs/protocol.md` §8.5.

use crate::enums::RESEARCH_TOPIC_COUNT;
use crate::team::{ResearchTopicInfo, TeamId};
use crate::unit::UnitId;
use crate::UNIT_COUNT;

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

/// Разобранный пакет 54 в терминах модели (`max_core`, без зависимости от `max_net`).
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
