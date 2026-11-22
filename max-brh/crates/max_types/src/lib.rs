//! max_types — общие доменные и протокольные типы M.A.X. (Infrastructure).
//!
//! Крейт выделен на Ф5.2 (решение владельца, вариант «б»: `PLAN-refactor.md`
//! §3.3) как общий нижний слой, от которого зависят `max_core` (Application),
//! `max_net` (Infrastructure) и `max_assets` (Infrastructure). Это снимает
//! формальное нарушение правила 5 (`max_assets → max_core`,
//! `max_net → max_core`) для **чистых** типов.
//!
//! ## Что здесь лежит
//!
//! * [`enums`] — все перечисления/битовые маски C++ с точными `#[repr]`,
//!   дискриминантами и `TryFrom`-преобразованиями; `UnitType` (93 типа);
//! * [`geo`] — `Point`, `PathStep`, `Direction`, `DIRECTION_OFFSETS`, размеры
//!   карты;
//! * [`map`] — `SurfaceMap`, `ResourceMapEntry`, `ResourceMap`, `Map`;
//! * [`team`] — листовые типы команд: `TeamId`, `TeamKind`, `Team`,
//!   `ResearchTopicInfo`, `Complex`, `Cargo`;
//! * [`unit`] — `UnitId` (16-битный id с командой в старших битах) и
//!   `UnitListKind`;
//! * [`input`] — протокольные DTO входящих пакетов (`Command` и payload'ы);
//! * [`events`] — DTO авторитетного состояния команды (пакет 54);
//! * [`victory`] — `MissionCategory` (категория миссии).
//!
//! ## Чего здесь быть не должно
//!
//! Типы, которые читают данные `max_core` (`UnitValues`, таблица
//! `UNIT_TYPE_PROPERTIES`, `TeamInfo`/`TeamUnits`, `UnitInfo`) остаются в
//! `max_core`; методы `UnitType`, читающие эти данные, живут в extension-типаже
//! `max_core::UnitTypeClassify` (sealed), чтобы правило сироты соблюдалось.
//!
//! Принцип релокации (правило 2): типы перенесены **verbatim** — `#[repr]`,
//! дискриминанты, порядок полей, derive и значения битов не менялись, поэтому
//! CRC/детерминизм/V70 не затрагиваются.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]

pub mod enums;
pub mod team;
pub mod unit;
