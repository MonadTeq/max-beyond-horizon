//! max_core — чистая детерминированная модель M.A.X. без I/O.
//!
//! Крейт повторяет поведение C++-порта (`max-cpp-port/src`) бит-в-бит: типы и
//! константы оригинала (P0), детерминированный RNG (P1), карта и генерация
//! (P2), далее — юниты, команды, комплексы, экономика (P3), приказы, бой и ИИ
//! (DESIGN.md §7).
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
//! * [`unit_values`] — `UnitValues`, индексы `ATTRIB_*`, таблица базовых
//!   значений 93 типов (`SC_ATTRI`) и клановые модификаторы (`SC_CLANS`);
//! * [`team`] — `TeamInfo`/`ResearchTopicInfo`/`TeamUnits`, `Complex`, `Cargo`;
//! * [`unit`] — `UnitId`, `UnitInfo` (симуляционные `S`-поля), пути юнита;
//! * [`hash`] — `UnitHash`/`MapHash` с упорядоченными ведёрками;
//! * [`save`] — V70 `&[u8]` ⇄ [`save::SaveFile`] (полный разбор и запись);
//! * [`order`] — каркас клиентских команд (входной API, DESIGN §2.4).
//!
//! Запрещено (DESIGN.md §1.4, §6.4): файловый I/O, сеть, время, потоки,
//! `f32`/`f64` в симуляции, итерация по `HashMap`.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]

pub mod enums;
pub mod geo;
pub mod hash;
pub mod map;
pub mod order;
pub mod rng;
pub mod save;
pub mod setup;
pub mod team;
pub mod unit;
pub mod unit_values;

pub use enums::{
    AiStrategy, CautionLevel, FileType, GameStateType, InvalidEnumValue, Opponent, OpponentType,
    Planet, PlanetType, PlayMode, ResearchTopic, ResearchTopicType, Surface, SurfaceType, TeamClan,
    TeamClanType, TeamIndex, TeamType, UnitOrder, UnitOrderState, UnitOrderStateType, UnitOrderType,
    UnitType, VictoryType, WinLossState,
};
pub use geo::{
    Direction, PathStep, Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_SIZE, MAP_TILES, MAP_WIDTH,
};
pub use hash::{MapHash, MapHashObject, UnitHash};
pub use map::{Map, MapSize, ResourceKind, ResourceMap, ResourceMapEntry, SurfaceMap};
pub use order::Order;
pub use rng::{GenerateValue, Randomizer};
pub use save::{
    AiPlayerState, GameManagerState, HeatMap, IniOptions, IniPreferences, MessageLogEntry,
    SaveError, SaveFile, SaveFormatHeader, SaveTeamUnits, SaveUnit, SavedComplex, SavedUnitValues,
    SpottedUnit, SAVE_VERSION_V70,
};
pub use setup::{
    derive_surface_map, generate_map, AlienPlacement, AlienUnitCosts, GameSetup,
    GenerationSettings, ResourceLevel, SetupOutput, TeamStart,
};
pub use team::{
    Cargo, Complex, ComplexId, ResearchTopicInfo, ScreenLocation, Team, TeamId, TeamInfo, TeamInfoUi,
    TeamKind, TeamUnits,
};
pub use unit::{
    AirPath, BuilderPath, GroundPath, Rect, UnitId, UnitInfo, UnitListKind, UnitPath,
};
pub use unit_values::{
    apply_clan_tradeoffs, base_unit_values, base_unit_values_by_index, clan_tradeoff,
    team_base_values, upgrade_offset_factor, UnitValues, ATTRIB_AGENT_ADJUST, ATTRIB_AMMO,
    ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_ATTACK_RADIUS, ATTRIB_COUNT, ATTRIB_FUEL, ATTRIB_HITS,
    ATTRIB_MOVE_AND_FIRE, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN, ATTRIB_SPEED, ATTRIB_STORAGE,
    ATTRIB_TURNS, BASE_UNIT_VALUES, CLAN_TRADEOFFS, UNIT_COUNT,
};

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

    #[test]
    fn p3_types_are_visible_from_crate_root() {
        // Таблица атрибутов, доменные команды и сейв-слой доступны с корня.
        assert_eq!(crate::BASE_UNIT_VALUES.len(), crate::UNIT_COUNT);
        assert_eq!(crate::UNIT_COUNT, 93);
        let units = crate::TeamUnits::with_clan(crate::TeamClan::Musashi);
        assert_eq!(units.complexes().len(), 0);
        assert_eq!(crate::SaveFile::parse(SAVE_BYTES).map(|save| save.header.version), Ok(70));
        assert_eq!(
            crate::SaveFile::parse(SAVE_BYTES)
                .expect("fixture parses")
                .units
                .len(),
            312
        );
    }

    const SAVE_BYTES: &[u8] = include_bytes!("../tests/data/save1_v70.cam");
}
