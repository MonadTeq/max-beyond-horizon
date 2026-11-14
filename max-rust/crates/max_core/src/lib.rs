//! max_core — чистая детерминированная модель M.A.X. без I/O.
//!
//! Крейт повторяет поведение C++-порта (`max-cpp-port/src`) бит-в-бит: типы и
//! константы оригинала (P0), детерминированный RNG (P1), далее — карта, юниты,
//! приказы, бой и ИИ (DESIGN.md §7).
//!
//! Текущее состояние:
//! * [`enums`] — все перечисления/битовые маски C++ с точными `#[repr]` и
//!   `TryFrom`-преобразованиями;
//! * [`geo`] — `Point`, `PathStep`, `Direction`, `DIRECTION_OFFSETS`, размеры
//!   карты;
//! * [`rng`] — `Randomizer` (SplitMix64 + Xoshiro128\*\*) с эталонными
//!   векторами из C++ (P1);
//! * [`map`] — `SurfaceMap` (112×112 битмаска `Surface`), `ResourceMapEntry`
//!   (u16-битфилд количества/типа/видимости) и `Map` (state-model §3.10);
//! * [`setup`] — детерминированная генерация карты при старте партии: вывод
//!   `SurfaceMap` из tile-data `.wrl`, ресурсы (`ResourceAllocator`), стартовые
//!   позиции и alien-обломки, с порядком RNG 1:1 к C++;
//! * [`order`], [`team`], [`unit`] — каркас, сохранённый для совместимости
//!   потребителей (`max_net`, `max_render`) и поэтапной замены по DESIGN.md §2.3.
//!
//! Запрещено (DESIGN.md §1.4, §6.4): файловый I/O, сеть, время, потоки,
//! `f32`/`f64` в симуляции, итерация по `HashMap`.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]

pub mod enums;
pub mod geo;
pub mod map;
pub mod order;
pub mod rng;
pub mod setup;
pub mod team;
pub mod unit;

pub use enums::{
    AiStrategy, CautionLevel, FileType, GameStateType, InvalidEnumValue, Opponent, OpponentType,
    Planet, PlanetType, PlayMode, ResearchTopic, ResearchTopicType, Surface, SurfaceType, TeamClan,
    TeamClanType, TeamIndex, TeamType, UnitOrder, UnitOrderState, UnitOrderStateType, UnitOrderType,
    UnitType, VictoryType, WinLossState,
};
pub use geo::{
    Direction, PathStep, Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_SIZE, MAP_TILES, MAP_WIDTH,
};
pub use map::{Map, MapSize, ResourceKind, ResourceMap, ResourceMapEntry, SurfaceMap};
pub use order::Order;
pub use rng::{GenerateValue, Randomizer};
pub use setup::{
    derive_surface_map, generate_map, AlienPlacement, AlienUnitCosts, GameSetup,
    GenerationSettings, ResourceLevel, SetupOutput, TeamStart,
};
pub use team::{Team, TeamId, TeamKind};
pub use unit::{Unit, UnitId, UnitKind};

#[cfg(test)]
mod tests {
    use crate::enums::{UnitOrder, UnitOrderState, UnitType};
    use crate::geo::{Direction, Point, DIRECTION_OFFSETS, MAP_TILES};
    use crate::rng::Randomizer;
    use crate::{Team, TeamId, TeamKind};

    #[test]
    fn core_smoke() {
        let team = Team::new(TeamId(0), TeamKind::Player);
        assert_eq!(team.kind, TeamKind::Player);
    }

    #[test]
    fn p0_constants_are_visible_from_crate_root() {
        assert_eq!(UnitType::COUNT, 93);
        assert_eq!(UnitOrder::COUNT, 32);
        assert_eq!(UnitOrderState::COUNT, 47);
        assert_eq!(MAP_TILES, 12_544);
        assert_eq!(Direction::COUNT, DIRECTION_OFFSETS.len());
        assert_eq!(Point::new(1, 2).x, 1);
    }

    #[test]
    fn p1_rng_is_deterministic() {
        let mut first = Randomizer::new(0xDEAD_BEEF);
        let mut second = Randomizer::new(0xDEAD_BEEF);
        for _ in 0..1000 {
            assert_eq!(first.next_u32(), second.next_u32());
        }
    }
}
