//! Перечисления, битовые маски и ключевые константы модели M.A.X.
//!
//! Источники значений:
//! * оракул `max-cpp-port/src/enums.hpp`, `resourcetable.hpp`, `planetselectmenu.hpp`;
//! * `max-cpp-port-max-brh-docs/save.md` (V70) — имена и значения сохраняемых перечислений;
//! * `max-brh-docs/state-model.md` §2 — предлагаемые Rust-имена.
//!
//! Правила модуля (DESIGN.md §3.2):
//! * каждое перечисление — `#[repr(uN)]` с точными значениями C++;
//! * derive: `Debug, Clone, Copy, PartialEq, Eq, Hash` (+ `Serialize, Deserialize`
//!   для снапшотов/дебага);
//! * обратное преобразование — `TryFrom<u8>`/`TryFrom<u16>`, ошибка
//!   [`InvalidEnumValue`] с именем типа и сырым значением;
//! * у каждого перечисления есть `ALL` и `COUNT` — тесты round-trip проходят по
//!   всем вариантам без «забытых» значений.

use core::fmt;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

use serde::{Deserialize, Serialize};

use crate::team::{TeamId, TeamKind};

// ---------------------------------------------------------------------------
// Ошибка преобразования
// ---------------------------------------------------------------------------

/// Сырое числовое значение не соответствует ни одному варианту перечисления.
///
/// Возвращается `TryFrom<u8>`/`TryFrom<u16>` всех перечислений модуля.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InvalidEnumValue {
    /// Имя перечисления, для которого выполнено преобразование.
    pub type_name: &'static str,
    /// Неизвестное сырое значение.
    pub value: u32,
}

impl InvalidEnumValue {
    /// Создаёт ошибку преобразования.
    #[inline]
    pub const fn new(type_name: &'static str, value: u32) -> Self {
        Self { type_name, value }
    }
}

impl fmt::Display for InvalidEnumValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid value {} for enum {}",
            self.value, self.type_name
        )
    }
}

impl std::error::Error for InvalidEnumValue {}

// ---------------------------------------------------------------------------
// Макросы объявления перечислений
// ---------------------------------------------------------------------------

/// Генерирует `From<Enum> for Repr` и `TryFrom<Repr> for Enum`.
macro_rules! impl_conversions {
    ($name:ty, $repr:ty, [$($variant:ident),+ $(,)?]) => {
        impl From<$name> for $repr {
            #[inline]
            fn from(value: $name) -> Self {
                value as $repr
            }
        }

        impl TryFrom<$repr> for $name {
            type Error = InvalidEnumValue;

            #[inline]
            fn try_from(raw: $repr) -> Result<Self, Self::Error> {
                $(
                    if raw == <$name>::$variant as $repr {
                        return Ok(<$name>::$variant);
                    }
                )+
                Err(InvalidEnumValue::new(stringify!($name), raw as u32))
            }
        }
    };
}

/// Добавляет `ALL` и `COUNT` — используется тестами round-trip.
macro_rules! impl_all {
    ($name:ty, [$($variant:ident),+ $(,)?]) => {
        impl $name {
            /// Все варианты в порядке объявления (совпадает с порядком C++).
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            /// Количество вариантов.
            pub const COUNT: usize = Self::ALL.len();
        }
    };
}

/// Объявляет перечисление с `#[repr(u8)]` и полным набором преобразований.
macro_rules! enum_u8 {
    ($(#[$doc:meta])* $name:ident {
        $($(#[$vmeta:meta])* $variant:ident = $value:expr),+ $(,)?
    }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[repr(u8)]
        pub enum $name {
            $($(#[$vmeta])* $variant = $value),+
        }

        impl_all!($name, [$($variant),+]);
        impl_conversions!($name, u8, [$($variant),+]);
        impl_conversions!($name, u16, [$($variant),+]);
    };
}

/// Объявляет перечисление с `#[repr(u16)]` и полным набором преобразований.
macro_rules! enum_u16 {
    ($(#[$doc:meta])* $name:ident {
        $($(#[$vmeta:meta])* $variant:ident = $value:expr),+ $(,)?
    }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[repr(u16)]
        pub enum $name {
            $($(#[$vmeta])* $variant = $value),+
        }

        impl_all!($name, [$($variant),+]);
        impl_conversions!($name, u16, [$($variant),+]);
        impl_conversions!($name, u8, [$($variant),+]);
    };
}

// ---------------------------------------------------------------------------
// Команды
// ---------------------------------------------------------------------------

// `TeamKind` объявлен в `team.rs` (каркас сохраняет имя); здесь только
// добавляются преобразования C++-значений и идиоматичный алиас `TeamType`.
impl_all!(
    TeamKind,
    [None, Player, Computer, Remote, Eliminated]
);
impl_conversions!(
    TeamKind,
    u8,
    [None, Player, Computer, Remote, Eliminated]
);
impl_conversions!(
    TeamKind,
    u16,
    [None, Player, Computer, Remote, Eliminated]
);

/// Синоним C++/save.md `TeamType` / `PlayerType`.
///
/// Значения: `None=0`, `Player=1` (`TEAM_TYPE_HUMAN`), `Computer=2`,
/// `Remote=3`, `Eliminated=4`.
pub type TeamType = TeamKind;

enum_u8! {
    /// Индекс команды — C++ `PlayerTeamType`, save.md `TeamIndex`.
    ///
    /// Внутри крейта идентификатор команды — newtype [`TeamId`]; этот enum нужен
    /// для точного парсинга сейвов и сетевых пакетов.
    TeamIndex {
        Red = 0x00,
        Green = 0x01,
        Blue = 0x02,
        Gray = 0x03,
        Alien = 0x04,
    }
}

/// `PLAYER_TEAM_MAX` из `enums.hpp`: количество индексов команд (0..=4).
pub const PLAYER_TEAM_MAX: u8 = 5;

impl From<TeamIndex> for TeamId {
    #[inline]
    fn from(value: TeamIndex) -> Self {
        Self(value as u8)
    }
}

impl TryFrom<TeamId> for TeamIndex {
    type Error = InvalidEnumValue;

    #[inline]
    fn try_from(value: TeamId) -> Result<Self, Self::Error> {
        Self::try_from(value.0)
    }
}

impl TeamId {
    /// Красная команда (`PLAYER_TEAM_RED`).
    pub const RED: Self = Self(0);
    /// Зелёная команда (`PLAYER_TEAM_GREEN`).
    pub const GREEN: Self = Self(1);
    /// Синяя команда (`PLAYER_TEAM_BLUE`).
    pub const BLUE: Self = Self(2);
    /// Серая команда (`PLAYER_TEAM_GRAY`).
    pub const GRAY: Self = Self(3);
    /// Псевдокоманда нейтральных юнитов (`PLAYER_TEAM_ALIEN`).
    pub const ALIEN: Self = Self(4);
    /// Значение `PLAYER_TEAM_MAX` (не команда, а счётчик).
    pub const MAX: Self = Self(PLAYER_TEAM_MAX);
}

enum_u8! {
    /// Клан команды — C++ `TeamClanType`, save.md `TeamClan` (1–8, случайный — 0).
    TeamClan {
        Random = 0x00,
        TheChosen = 0x01,
        CrimsonPath = 0x02,
        VonGriffin = 0x03,
        AyersHand = 0x04,
        Musashi = 0x05,
        SacredEights = 0x06,
        SevenKnights = 0x07,
        AxisInc = 0x08,
    }
}

/// Синоним C++-имени `TeamClanType`.
pub type TeamClanType = TeamClan;

enum_u8! {
    /// Уровень сложности ИИ — C++ `OpponentType`, save.md `OpponentType`.
    Opponent {
        Clueless = 0,
        Apprentice = 1,
        Average = 2,
        Expert = 3,
        Master = 4,
        God = 5,
    }
}

/// Синоним C++/save.md `OpponentType`.
pub type OpponentType = Opponent;

enum_u8! {
    /// Режим игры — C++ анонимный `PLAY_MODE_*`, save.md `PlayMode`.
    PlayMode {
        TurnBased = 0,
        SimultaneousMoves = 1,
        Unknown = 2,
    }
}

enum_u8! {
    /// Тип победы — C++ анонимный `VICTORY_TYPE_*`, save.md `VictoryType`.
    VictoryType {
        Duration = 0,
        Score = 1,
    }
}

enum_u8! {
    /// Тип файла сохранения — save.md `FileType` (контекст партии/лобби).
    FileType {
        Custom = 0,
        Tutorial = 1,
        Campaign = 2,
        HotSeat = 3,
        Multiplayer = 4,
        Demo = 5,
        Debug = 6,
        Text = 7,
        Scenario = 8,
        MultiScenario = 9,
    }
}

enum_u8! {
    /// Планета — save.md `PlanetType`, C++ `PlanetType` (`planetselectmenu.hpp`).
    Planet {
        Snowcrab = 0,
        Frigia = 1,
        IceBerg = 2,
        TheCooler = 3,
        UltimaThule = 4,
        LongFloes = 5,
        IronCross = 6,
        Splatterscape = 7,
        PeakABoo = 8,
        ValentinesPlanet = 9,
        ThreeRings = 10,
        GreatDivide = 11,
        NewLuzon = 12,
        MiddleSea = 13,
        HighImpact = 14,
        Sanctuary = 15,
        Islandia = 16,
        Hammerhead = 17,
        Freckles = 18,
        Sandspit = 19,
        GreatCircle = 20,
        LongPassage = 21,
        FlashPoint = 22,
        Bottleneck = 23,
    }
}

/// Синоним C++/save.md `PlanetType`.
pub type PlanetType = Planet;

enum_u8! {
    /// Тема исследований — C++ `ResearchTopicType`, save.md `ResearchTopic`.
    ///
    /// Порядок совпадает с атрибутами `UnitValues` (`ATTRIB_*`).
    ResearchTopic {
        Attack = 0,
        Shots = 1,
        Range = 2,
        Armor = 3,
        Hits = 4,
        Speed = 5,
        Scan = 6,
        Cost = 7,
    }
}

/// Синоним C++-имени `ResearchTopicType`.
pub type ResearchTopicType = ResearchTopic;

/// `RESEARCH_TOPIC_COUNT` из `enums.hpp`.
pub const RESEARCH_TOPIC_COUNT: usize = ResearchTopic::COUNT;

enum_u8! {
    /// Состояние партии — C++ `GameStateType` (16 значений).
    GameStateType {
        Unused0 = 0x0,
        Unused1 = 0x1,
        Unused2 = 0x2,
        MainMenu = 0x3,
        Unused4 = 0x4,
        Unused5 = 0x5,
        GameSetup = 0x6,
        SiteSelect = 0x7,
        InGame = 0x8,
        EndTurn = 0x9,
        LoadGame = 0xA,
        TurnActive = 0xB,
        DeployingUnits = 0xC,
        SiteSelected = 0xD,
        ExitSiteSelect = 0xE,
        FatalError = 0xF,
    }
}

enum_u8! {
    /// Исход команды — C++ `WinLossState`.
    WinLossState {
        Generic = 0,
        Pending = 1,
        Won = 2,
        Lost = 3,
    }
}

enum_u8! {
    /// Стратегия ИИ — C++ `AiStrategyType` (0..=9, `AI_STRATEGY_MAX` = 10).
    AiStrategy {
        Random = 0,
        Defensive = 1,
        Missiles = 2,
        Air = 3,
        Sea = 4,
        ScoutHorde = 5,
        TankHorde = 6,
        FastAttack = 7,
        CombinedArms = 8,
        Espionage = 9,
    }
}

/// `AI_STRATEGY_MAX` — счётчик стратегий (не вариант).
pub const AI_STRATEGY_MAX: u8 = 10;

enum_u8! {
    /// Уровень осторожности ИИ — C++ `CautionLevelType`.
    CautionLevel {
        None = 0,
        AvoidReactionFire = 1,
        AvoidNextTurnsFire = 2,
        AvoidAllDamage = 3,
    }
}

// ---------------------------------------------------------------------------
// Приказы юнита
// ---------------------------------------------------------------------------

enum_u8! {
    /// Внутренний приказ юнита — C++ `UnitOrderType` / save.md `OrderType`.
    ///
    /// Не путать с `Order` (`max_core::order`) — клиентской командой серверу.
    UnitOrder {
        Await = 0x00,
        Transform = 0x01,
        Move = 0x02,
        Fire = 0x03,
        Build = 0x04,
        Activate = 0x05,
        NewAllocate = 0x06,
        PowerOn = 0x07,
        PowerOff = 0x08,
        Explode = 0x09,
        Unload = 0x0A,
        Clear = 0x0B,
        Sentry = 0x0C,
        Land = 0x0D,
        TakeOff = 0x0E,
        Load = 0x0F,
        Idle = 0x10,
        Repair = 0x11,
        /// `ORDER_17` (0x12): имя в C++ не восстановлено; save.md — `REFUEL`.
        Order17 = 0x12,
        Reload = 0x13,
        Transfer = 0x14,
        HaltBuilding = 0x15,
        AwaitScaling = 0x16,
        AwaitTapePositioning = 0x17,
        AwaitStealUnit = 0x18,
        AwaitDisableUnit = 0x19,
        Disable = 0x1A,
        MoveToUnit = 0x1B,
        Upgrade = 0x1C,
        LayMine = 0x1D,
        MoveToAttack = 0x1E,
        HaltBuilding2 = 0x1F,
    }
}

/// Синоним C++-имени `UnitOrderType`.
pub type UnitOrderType = UnitOrder;

/// `ORDER_COUNT_MAX` из `enums.hpp`.
pub const ORDER_COUNT_MAX: u8 = UnitOrder::COUNT as u8;

enum_u8! {
    /// Состояние исполнения приказа — C++ `UnitOrderStateType` (47 значений).
    UnitOrderState {
        Init = 0x00,
        ExecutingOrder = 0x01,
        ReadyToExecuteOrder = 0x02,
        Store = 0x03,
        PrepareStore = 0x04,
        InProgress = 0x05,
        InTransition = 0x06,
        IssuingPath = 0x07,
        ReadyToFire = 0x08,
        FireInProgress = 0x09,
        State10 = 0x0A,
        BuildInProgress = 0x0B,
        PathRequestCancel = 0x0C,
        BuildCancel = 0x0D,
        Destroy = 0x0E,
        LoadingInProgress = 0x0F,
        FinishLoading = 0x10,
        UnloadingInProgress = 0x11,
        FinishUnloading = 0x12,
        FinishLanding = 0x13,
        FinishTakeOff = 0x14,
        State21 = 0x15,
        ProgressTransforming = 0x16,
        FinishTransforming = 0x17,
        ClearPath = 0x18,
        SelectSite = 0x19,
        BuildClearing = 0x1A,
        Explode = 0x1B,
        MoveInit = 0x1C,
        MoveGettingPath = 0x1D,
        BuildingReady = 0x1E,
        UnitReady = 0x1F,
        Expand = 0x20,
        Shrink = 0x21,
        TapePositioningInit = 0x22,
        TapePositioningEnter = 0x23,
        TapePositioningDeinit = 0x24,
        TapePositioningLeave = 0x25,
        Elevate = 0x26,
        Lower = 0x27,
        AttackPending = 0x28,
        AttackBeginning = 0x29,
        NewOrder = 0x2A,
        Inactive = 0x2B,
        PlacingMines = 0x2C,
        RemovingMines = 0x2D,
        BuildAbort = 0x2E,
    }
}

/// Синоним C++-имени `UnitOrderStateType`.
pub type UnitOrderStateType = UnitOrderState;

// ---------------------------------------------------------------------------
// Тип юнита: 93 варианта ResourceID / UnitType
// ---------------------------------------------------------------------------

enum_u16! {
    /// Тип юнита — C++ `ResourceID` (диапазон `UNIT_START..UNIT_END`), save.md
    /// `UnitType`.
    ///
    /// 93 игровых типа, `#[repr(u16)]`, значения 0x00–0x5C; 0x5D (`UNIT_END`) —
    /// счётчик, а не вариант. Имена — смысловые (C++-токен / save.md указаны в
    /// комментариях); ресурсный токен типа отдаёт [`UnitType::tag`].
    UnitType {
        /// `COMMTWR` / `UNIT_TYPE_GOLD_REFINERY`.
        GoldRefinery = 0x00,
        /// `POWERSTN` / `POWER_STATION`.
        PowerStation = 0x01,
        /// `POWGEN` / `POWER_GENERATOR`.
        PowerGenerator = 0x02,
        /// `BARRACKS` / `BARRACKS`.
        Barracks = 0x03,
        /// `SHIELDGN` / `ALIEN_BUILDING_1`.
        ShieldGenerator = 0x04,
        /// `RADAR` / `RADAR`.
        Radar = 0x05,
        /// `ADUMP` / `STORAGE_UNIT`.
        MaterialStorage = 0x06,
        /// `FDUMP` / `FUEL_TANK`.
        FuelStorage = 0x07,
        /// `GOLDSM` / `GOLD_VAULT`.
        GoldVault = 0x08,
        /// `DEPOT` / `DEPOT`.
        Depot = 0x09,
        /// `HANGAR` / `HANGAR`.
        Hangar = 0x0A,
        /// `DOCK` / `DOCK`.
        Dock = 0x0B,
        /// `CNCT_4W` / `CONNECTOR`.
        Connector = 0x0C,
        /// `LRGRUBLE` / `LARGE_RUBBLE_1`.
        LargeRubble = 0x0D,
        /// `SMLRUBLE` / `SMALL_RUBBLE_1`.
        SmallRubble = 0x0E,
        /// `LRGTAPE` / `LARGE_TAPE`.
        LargeTape = 0x0F,
        /// `SMLTAPE` / `SMALL_TAPE`.
        SmallTape = 0x10,
        /// `LRGSLAB` / `LARGE_SLAB`.
        LargeSlab = 0x11,
        /// `SMLSLAB` / `SMALL_SLAB`.
        SmallSlab = 0x12,
        /// `LRGCONES` / `LARGE_CONES`.
        LargeCones = 0x13,
        /// `SMLCONES` / `SMALL_CONES`.
        SmallCones = 0x14,
        /// `ROAD` / `ROAD`.
        Road = 0x15,
        /// `LANDPAD` / `LANDING_PAD`.
        LandingPad = 0x16,
        /// `SHIPYARD` / `SHIPYARD`.
        Shipyard = 0x17,
        /// `LIGHTPLT` / `LIGHT_VEHICLE_PLANT`.
        LightVehiclePlant = 0x18,
        /// `LANDPLT` / `HEAVY_VEHICLE_PLANT`.
        HeavyVehiclePlant = 0x19,
        /// `SUPRTPLT` / `ALIEN_BUILDING_2`.
        SupportPlant = 0x1A,
        /// `AIRPLT` / `AIR_UNITS_PLANT`.
        AirUnitsPlant = 0x1B,
        /// `HABITAT` / `HABITAT`.
        Habitat = 0x1C,
        /// `RESEARCH` / `RESEARCH_CENTER`.
        ResearchCentre = 0x1D,
        /// `GREENHSE` / `ECOSPHERE`.
        EcoSphere = 0x1E,
        /// `RECCENTR` / `ALIEN_BUILDING_3`.
        RecreationCenter = 0x1F,
        /// `TRAINHAL` / `TRAINING_HALL`.
        TrainingHall = 0x20,
        /// `WTRPLTFM` / `WATER_PLATFORM`.
        WaterPlatform = 0x21,
        /// `GUNTURRT` / `GUN_TURRET`.
        GunTurret = 0x22,
        /// `ANTIAIR` / `ANTI_AIRCRAFT`.
        AntiAir = 0x23,
        /// `ARTYTRRT` / `ARTILLERY`.
        Artillery = 0x24,
        /// `ANTIMSSL` / `MISSILE_LAUNCHER`.
        MissileLauncher = 0x25,
        /// `BLOCK` / `CONCRETE_BLOCK`.
        ConcreteBlock = 0x26,
        /// `BRIDGE` / `BRIDGE`.
        Bridge = 0x27,
        /// `MININGST` / `MINING_STATION`.
        MiningStation = 0x28,
        /// `LANDMINE` / `LAND_MINE`.
        LandMine = 0x29,
        /// `SEAMINE` / `SEA_MINE`.
        SeaMine = 0x2A,
        /// `LNDEXPLD` / `LAND_EXPLOSION`.
        LandExplosion = 0x2B,
        /// `AIREXPLD` / `AIR_EXPLOSION`.
        AirExplosion = 0x2C,
        /// `SEAEXPLD` / `SEA_EXPLOSION`.
        SeaExplosion = 0x2D,
        /// `BLDEXPLD` / `BUILDING_EXPLOSION`.
        BuildingExplosion = 0x2E,
        /// `HITEXPLD` / `HIT_EXPLOSION`.
        HitExplosion = 0x2F,
        /// `MASTER` / `MASTER_BUILDER`.
        MasterBuilder = 0x30,
        /// `CONSTRCT` / `CONSTRUCTOR`.
        Constructor = 0x31,
        /// `SCOUT` / `SCOUT`.
        Scout = 0x32,
        /// `TANK` / `TANK`.
        Tank = 0x33,
        /// `ARTILLRY` / `ASSAULT_GUN`.
        AssaultGun = 0x34,
        /// `ROCKTLCH` / `ROCKET_LAUNCHER`.
        RocketLauncher = 0x35,
        /// `MISSLLCH` / `MISSILE_CRAWLER`.
        MissileCrawler = 0x36,
        /// `SP_FLAK` / `MOBILE_ANTI_AIRCRAFT`.
        MobileAntiAir = 0x37,
        /// `MINELAYR` / `MINE_LAYER`.
        MineLayer = 0x38,
        /// `SURVEYOR` / `SURVEYOR`.
        Surveyor = 0x39,
        /// `SCANNER` / `SCANNER`.
        Scanner = 0x3A,
        /// `SPLYTRCK` / `SUPPLY_TRUCK`.
        SupplyTruck = 0x3B,
        /// `GOLDTRCK` / `GOLD_TRUCK`.
        GoldTruck = 0x3C,
        /// `ENGINEER` / `ENGINEER`.
        Engineer = 0x3D,
        /// `BULLDOZR` / `BULLDOZER`.
        Bulldozer = 0x3E,
        /// `REPAIR` / `REPAIR_UNIT`.
        RepairUnit = 0x3F,
        /// `FUELTRCK` / `FUEL_TRUCK`.
        FuelTruck = 0x40,
        /// `CLNTRANS` / `PERSONNEL_CARRIER`.
        ArmouredPersonnelCarrier = 0x41,
        /// `COMMANDO` / `INFILTRATOR`.
        Infiltrator = 0x42,
        /// `INFANTRY` / `INFANTRY`.
        Infantry = 0x43,
        /// `FASTBOAT` / `ESCORT`.
        Escort = 0x44,
        /// `CORVETTE` / `CORVETTE`.
        Corvette = 0x45,
        /// `BATTLSHP` / `GUNBOAT`.
        Gunboat = 0x46,
        /// `SUBMARNE` / `SUBMARINE`.
        Submarine = 0x47,
        /// `SEATRANS` / `SEA_TRANSPORT`.
        SeaTransport = 0x48,
        /// `MSSLBOAT` / `MISSILE_CRUISER`.
        MissileCruiser = 0x49,
        /// `SEAMNLYR` / `SEA_MINE_LAYER`.
        SeaMineLayer = 0x4A,
        /// `CARGOSHP` / `CARGO_SHIP`.
        CargoShip = 0x4B,
        /// `FIGHTER` / `FIGHTER`.
        Fighter = 0x4C,
        /// `BOMBER` / `GROUND_ATTACK_PLANE`.
        Bomber = 0x4D,
        /// `AIRTRANS` / `AIR_TRANSPORT`.
        AirTransport = 0x4E,
        /// `AWAC` / `AWAC`.
        Awac = 0x4F,
        /// `JUGGRNT` / `ALIEN_GUNBOAT`.
        Juggernaut = 0x50,
        /// `ALNTANK` / `ALIEN_TANK`.
        AlienTank = 0x51,
        /// `ALNASGUN` / `ALIEN_ASSAULT_GUN`.
        AlienAssaultGun = 0x52,
        /// `ALNPLANE` / `ALIEN_ATTACK_PLANE`.
        AlienPlane = 0x53,
        /// `ROCKET` / `MISSILE`.
        Rocket = 0x54,
        /// `TORPEDO` / `TORPEDO`.
        Torpedo = 0x55,
        /// `ALNMISSL` / `ALIEN_MISSILE`.
        AlienMissile = 0x56,
        /// `ALNTBALL` / `TANK_PLASMA_BALL`.
        AlienTankBall = 0x57,
        /// `ALNABALL` / `ARTILLERY_PLASMA_BALL`.
        AlienArtilleryBall = 0x58,
        /// `RKTSMOKE` / `SMOKE_TRAIL`.
        RocketSmoke = 0x59,
        /// `TRPBUBLE` / `BUBBLE_TRAIL`.
        TorpedoBubble = 0x5A,
        /// `HARVSTER` / `HARVESTER`.
        Harvester = 0x5B,
        /// `WALDO` / `DEAD_WALDO`.
        DeadWaldo = 0x5C,
    }
}

/// `UNIT_END` из `resourcetable.hpp`: количество типов юнитов (0x5D = 93).
pub const UNIT_END: u16 = 0x5D;
/// Количество типов юнитов.
pub const UNIT_TYPE_COUNT: usize = UnitType::COUNT;

/// Ресурсный токен типа юнита — C++ `ResourceID` / ресурс `SC_UNITS`.
///
/// Токен (`COMMTWR`, `MININGST`, …) — это **данные** (совместимость с файлами
/// ресурсов), а не имя типа: правило 6.4 `rust-programming-rules.md`. Он же
/// первая часть doc-комментария каждого варианта (`/// TOKEN / C++_NAME`) и
/// ключ таблицы User-Friendly Name в слое представления (`max_app`,
/// `data/unit_names.json`).
///
/// Соответствие C++: `resourcetable.hpp` / `UNIT_TYPE_*`, `ResourceManager_GetUnit`.
impl UnitType {
    /// Токен ресурса типа (`COMMTWR` для [`UnitType::GoldRefinery`] и т.д.).
    ///
    /// Для всех 93 вариантов: значение — ровно первая часть doc-комментария
    /// варианта. Порядок и дискриминанты enum не затрагиваются.
    #[inline]
    pub fn tag(&self) -> &'static str {
        match self {
            UnitType::GoldRefinery => "COMMTWR",
            UnitType::PowerStation => "POWERSTN",
            UnitType::PowerGenerator => "POWGEN",
            UnitType::Barracks => "BARRACKS",
            UnitType::ShieldGenerator => "SHIELDGN",
            UnitType::Radar => "RADAR",
            UnitType::MaterialStorage => "ADUMP",
            UnitType::FuelStorage => "FDUMP",
            UnitType::GoldVault => "GOLDSM",
            UnitType::Depot => "DEPOT",
            UnitType::Hangar => "HANGAR",
            UnitType::Dock => "DOCK",
            UnitType::Connector => "CNCT_4W",
            // 0x0D–0x14: декорации — обломки, следы, плиты, конусы.
            UnitType::LargeRubble => "LRGRUBLE",
            UnitType::SmallRubble => "SMLRUBLE",
            UnitType::LargeTape => "LRGTAPE",
            UnitType::SmallTape => "SMLTAPE",
            UnitType::LargeSlab => "LRGSLAB",
            UnitType::SmallSlab => "SMLSLAB",
            UnitType::LargeCones => "LRGCONES",
            UnitType::SmallCones => "SMLCONES",
            // 0x15–0x2A: здания, дороги, мины.
            UnitType::Road => "ROAD",
            UnitType::LandingPad => "LANDPAD",
            UnitType::Shipyard => "SHIPYARD",
            UnitType::LightVehiclePlant => "LIGHTPLT",
            UnitType::HeavyVehiclePlant => "LANDPLT",
            UnitType::SupportPlant => "SUPRTPLT",
            UnitType::AirUnitsPlant => "AIRPLT",
            UnitType::Habitat => "HABITAT",
            UnitType::ResearchCentre => "RESEARCH",
            UnitType::EcoSphere => "GREENHSE",
            UnitType::RecreationCenter => "RECCENTR",
            UnitType::TrainingHall => "TRAINHAL",
            UnitType::WaterPlatform => "WTRPLTFM",
            UnitType::GunTurret => "GUNTURRT",
            UnitType::AntiAir => "ANTIAIR",
            UnitType::Artillery => "ARTYTRRT",
            UnitType::MissileLauncher => "ANTIMSSL",
            UnitType::ConcreteBlock => "BLOCK",
            UnitType::Bridge => "BRIDGE",
            UnitType::MiningStation => "MININGST",
            UnitType::LandMine => "LANDMINE",
            UnitType::SeaMine => "SEAMINE",
            // 0x2B–0x2F: эффекты взрывов.
            UnitType::LandExplosion => "LNDEXPLD",
            UnitType::AirExplosion => "AIREXPLD",
            UnitType::SeaExplosion => "SEAEXPLD",
            UnitType::BuildingExplosion => "BLDEXPLD",
            UnitType::HitExplosion => "HITEXPLD",
            // 0x30–0x53: юнит-типы (строитель, боевые, транспорт, alien).
            UnitType::MasterBuilder => "MASTER",
            UnitType::Constructor => "CONSTRCT",
            UnitType::Scout => "SCOUT",
            UnitType::Tank => "TANK",
            UnitType::AssaultGun => "ARTILLRY",
            UnitType::RocketLauncher => "ROCKTLCH",
            UnitType::MissileCrawler => "MISSLLCH",
            UnitType::MobileAntiAir => "SP_FLAK",
            UnitType::MineLayer => "MINELAYR",
            UnitType::Surveyor => "SURVEYOR",
            UnitType::Scanner => "SCANNER",
            UnitType::SupplyTruck => "SPLYTRCK",
            UnitType::GoldTruck => "GOLDTRCK",
            UnitType::Engineer => "ENGINEER",
            UnitType::Bulldozer => "BULLDOZR",
            UnitType::RepairUnit => "REPAIR",
            UnitType::FuelTruck => "FUELTRCK",
            UnitType::ArmouredPersonnelCarrier => "CLNTRANS",
            UnitType::Infiltrator => "COMMANDO",
            UnitType::Infantry => "INFANTRY",
            UnitType::Escort => "FASTBOAT",
            UnitType::Corvette => "CORVETTE",
            UnitType::Gunboat => "BATTLSHP",
            UnitType::Submarine => "SUBMARNE",
            UnitType::SeaTransport => "SEATRANS",
            UnitType::MissileCruiser => "MSSLBOAT",
            UnitType::SeaMineLayer => "SEAMNLYR",
            UnitType::CargoShip => "CARGOSHP",
            UnitType::Fighter => "FIGHTER",
            UnitType::Bomber => "BOMBER",
            UnitType::AirTransport => "AIRTRANS",
            UnitType::Awac => "AWAC",
            UnitType::Juggernaut => "JUGGRNT",
            UnitType::AlienTank => "ALNTANK",
            UnitType::AlienAssaultGun => "ALNASGUN",
            UnitType::AlienPlane => "ALNPLANE",
            // 0x54–0x5A: снаряды и следы (эффекты).
            UnitType::Rocket => "ROCKET",
            UnitType::Torpedo => "TORPEDO",
            UnitType::AlienMissile => "ALNMISSL",
            UnitType::AlienTankBall => "ALNTBALL",
            UnitType::AlienArtilleryBall => "ALNABALL",
            UnitType::RocketSmoke => "RKTSMOKE",
            UnitType::TorpedoBubble => "TRPBUBLE",
            // 0x5B–0x5C: harvester и «мёртвый WALDO».
            UnitType::Harvester => "HARVSTER",
            UnitType::DeadWaldo => "WALDO",
        }
    }
}

// ---------------------------------------------------------------------------
// SurfaceType — битовая маска проходимости
// ---------------------------------------------------------------------------

/// Проходимость клетки — C++ `SURFACE_TYPE_*`, save.md `SurfaceType`.
///
/// Это **битовая маска**, а не исключающий enum: клетка может быть
/// `LAND | COAST`, `WATER | COAST` и т.п. Один байт на клетку (`SurfaceMap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Surface(u8);

impl Surface {
    /// `SURFACE_TYPE_NONE`.
    pub const NONE: Self = Self(0x00);
    /// `SURFACE_TYPE_LAND`.
    pub const LAND: Self = Self(0x01);
    /// `SURFACE_TYPE_WATER`.
    pub const WATER: Self = Self(0x02);
    /// `SURFACE_TYPE_COAST`.
    pub const COAST: Self = Self(0x04);
    /// `SURFACE_TYPE_AIR`.
    pub const AIR: Self = Self(0x08);
    /// Все определённые биты (0x0F).
    pub const ALL: Self = Self(0x0F);

    /// Создаёт маску из битов; `None`, если установлены неизвестные биты.
    #[inline]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Создаёт маску, отбрасывая неизвестные биты.
    #[inline]
    pub const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// Сырое значение маски.
    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Маска пуста.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Содержит все биты `other`.
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Пересекается с `other`.
    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl From<Surface> for u8 {
    #[inline]
    fn from(value: Surface) -> Self {
        value.0
    }
}

impl TryFrom<u8> for Surface {
    type Error = InvalidEnumValue;

    #[inline]
    fn try_from(raw: u8) -> Result<Self, Self::Error> {
        Self::from_bits(raw).ok_or_else(|| InvalidEnumValue::new("Surface", raw as u32))
    }
}

impl BitOr for Surface {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Surface {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Surface {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for Surface {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl BitXor for Surface {
    type Output = Self;
    #[inline]
    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

impl BitXorAssign for Surface {
    #[inline]
    fn bitxor_assign(&mut self, rhs: Self) {
        self.0 ^= rhs.0;
    }
}

impl Not for Surface {
    type Output = Self;
    #[inline]
    fn not(self) -> Self {
        Self(!self.0 & Self::ALL.0)
    }
}

/// Синоним C++/save.md `SurfaceType`.
pub type SurfaceType = Surface;

// ---------------------------------------------------------------------------
// Байтовые константы (флаги, карго, коннекторы)
// ---------------------------------------------------------------------------

/// Флаги юнита — C++ анонимный enum `GROUND_COVER` … `REGENERATING_UNIT`.
///
/// В C++ это `uint32_t`-биты; здесь — константы без внешнего `bitflags`.
pub mod unit_flags {
    /// Наземное укрытие.
    pub const GROUND_COVER: u32 = 0x1;
    /// Идёт взрыв.
    pub const EXPLODING: u32 = 0x2;
    /// Анимированный юнит.
    pub const ANIMATED: u32 = 0x4;
    /// Юнит-коннектор.
    pub const CONNECTOR_UNIT: u32 = 0x8;
    /// Здание.
    pub const BUILDING: u32 = 0x10;
    /// Ракетный юнит.
    pub const MISSILE_UNIT: u32 = 0x20;
    /// Воздушный мобильный юнит.
    pub const MOBILE_AIR_UNIT: u32 = 0x40;
    /// Морской мобильный юнит.
    pub const MOBILE_SEA_UNIT: u32 = 0x80;
    /// Наземный мобильный юнит.
    pub const MOBILE_LAND_UNIT: u32 = 0x100;
    /// Стационарный юнит.
    pub const STATIONARY: u32 = 0x200;
    /// Хэш-бит команды GRAY.
    pub const HASH_TEAM_GRAY: u32 = 0x400;
    /// Хэш-бит команды BLUE.
    pub const HASH_TEAM_BLUE: u32 = 0x800;
    /// Хэш-бит команды GREEN.
    pub const HASH_TEAM_GREEN: u32 = 0x1000;
    /// Хэш-бит команды RED.
    pub const HASH_TEAM_RED: u32 = 0x2000;
    /// Юнит можно улучшать.
    pub const UPGRADABLE: u32 = 0x4000;
    /// Хэш-бит derelict/нейтральных.
    pub const HASH_TEAM_DERELICT: u32 = 0x8000;
    /// Парящий юнит.
    pub const HOVERING: u32 = 0x10000;
    /// Есть спрайт выстрела.
    pub const HAS_FIRING_SPRITE: u32 = 0x20000;
    /// Стреляет ракетами.
    pub const FIRES_MISSILES: u32 = 0x40000;
    /// Юнит-строитель.
    pub const CONSTRUCTOR_UNIT: u32 = 0x80000;
    /// Электронный юнит.
    pub const ELECTRONIC_UNIT: u32 = 0x200000;
    /// Выбираемый.
    pub const SELECTABLE: u32 = 0x400000;
    /// Автономный.
    pub const STANDALONE: u32 = 0x800000;
    /// Требует плиту.
    pub const REQUIRES_SLAB: u32 = 0x1000000;
    /// Спрайт башни.
    pub const TURRET_SPRITE: u32 = 0x2000000;
    /// Сторожевой юнит.
    pub const SENTRY_UNIT: u32 = 0x4000000;
    /// Вращающаяся башня.
    pub const SPINNING_TURRET: u32 = 0x8000000;
    /// Регенерирующий юнит.
    pub const REGENERATING_UNIT: u32 = 0x10000000;
}

/// Флаги груза — C++ `CARGO_*` (тип груза — младшие 5 бит).
pub const CARGO_FUEL: u8 = 0x20;
/// Флаг груза «золото».
pub const CARGO_GOLD: u8 = 0x40;
/// Флаг груза «материалы».
pub const CARGO_MATERIALS: u8 = 0x80;
/// Маска количества груза (младшие 5 бит).
pub const CARGO_MASK: u8 = 0x1F;

/// Направления коннекторов — C++ `ConnectorDirectionType` (8 бит).
pub const CONNECTOR_NORTH_LEFT: u16 = 0x1;
/// Север-право.
pub const CONNECTOR_NORTH_RIGHT: u16 = 0x2;
/// Восток-верх.
pub const CONNECTOR_EAST_TOP: u16 = 0x4;
/// Восток-низ.
pub const CONNECTOR_EAST_BOTTOM: u16 = 0x8;
/// Юг-лево.
pub const CONNECTOR_SOUTH_LEFT: u16 = 0x10;
/// Юг-право.
pub const CONNECTOR_SOUTH_RIGHT: u16 = 0x20;
/// Запад-верх.
pub const CONNECTOR_WEST_TOP: u16 = 0x40;
/// Запад-низ.
pub const CONNECTOR_WEST_BOTTOM: u16 = 0x80;
/// Соединение в процессе проверки (не направление).
pub const CONNECTION_BEING_TESTED: u16 = 0x400;

/// Бит `NO_RETREAT` в `UnitInfo::ai_state_bits`.
pub const AI_STATE_NO_RETREAT: u32 = 1;
/// Маска cooldown цели в `ai_state_bits`.
pub const AI_STATE_TARGET_COOLDOWN_MASK: u32 = 0xFE;
/// Напоминание «движение завершено».
pub const AI_STATE_MOVE_FINISHED_REMINDER: u32 = 0x100;
/// Напоминание «отступление».
pub const AI_STATE_RETREAT_REMINDER: u32 = 0x200;

/// Маска видимости команды в `ResourceMap` (фактическая, см. DESIGN риск 5).
pub const RESOURCE_MAP_VISIBILITY_MASK: u16 = 0x3C00;

/// Значения `InfoMapType` (AI): нет информации.
pub const INFO_MAP_NO_INFO: u8 = 0x00;
/// Исследовано.
pub const INFO_MAP_EXPLORED: u8 = 0x01;
/// Минное поле.
pub const INFO_MAP_MINE_FIELD: u8 = 0x02;
/// Фронтир.
pub const INFO_MAP_FRONTIER: u8 = 0x04;
/// Зона зачистки.
pub const INFO_MAP_CLEAR_OUT_ZONE: u8 = 0x08;

// ---------------------------------------------------------------------------
// Тесты: значения, размеры, round-trip всех вариантов
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unit::UnitId;

    fn roundtrip_u8<T>(variants: &[T])
    where
        T: Copy + fmt::Debug + PartialEq + TryFrom<u8, Error = InvalidEnumValue>,
        u8: From<T>,
    {
        for &variant in variants {
            let raw = u8::from(variant);
            assert_eq!(T::try_from(raw).unwrap(), variant, "u8 round-trip failed");
        }
    }

    fn roundtrip_u16<T>(variants: &[T])
    where
        T: Copy + fmt::Debug + PartialEq + TryFrom<u16, Error = InvalidEnumValue>,
        u16: From<T>,
    {
        for &variant in variants {
            let raw = u16::from(variant);
            assert_eq!(T::try_from(raw).unwrap(), variant, "u16 round-trip failed");
        }
    }

    #[test]
    fn enum_counts_match_cpp() {
        assert_eq!(TeamKind::COUNT, 5);
        assert_eq!(TeamIndex::COUNT, 5);
        assert_eq!(TeamClan::COUNT, 9);
        assert_eq!(Opponent::COUNT, 6);
        assert_eq!(PlayMode::COUNT, 3);
        assert_eq!(VictoryType::COUNT, 2);
        assert_eq!(FileType::COUNT, 10);
        assert_eq!(Planet::COUNT, 24);
        assert_eq!(ResearchTopic::COUNT, 8);
        assert_eq!(Surface::ALL.bits(), 0x0F);
        assert_eq!(UnitType::COUNT, 93);
        assert_eq!(GameStateType::COUNT, 16);
        assert_eq!(WinLossState::COUNT, 4);
        assert_eq!(AiStrategy::COUNT, 10);
        assert_eq!(CautionLevel::COUNT, 4);
        assert_eq!(UnitOrder::COUNT, 32);
        assert_eq!(UnitOrderState::COUNT, 47);

        assert_eq!(UNIT_END, 93);
        assert_eq!(UNIT_TYPE_COUNT, 93);
        assert_eq!(ORDER_COUNT_MAX, 32);
        assert_eq!(RESEARCH_TOPIC_COUNT, 8);
        assert_eq!(AI_STRATEGY_MAX, 10);
        assert_eq!(PLAYER_TEAM_MAX, 5);
    }

    #[test]
    fn key_values_match_cpp() {
        // resourcetable.hpp
        assert_eq!(UnitType::GoldRefinery as u16, 0);
        assert_eq!(UnitType::Tank as u16, 0x33);
        assert_eq!(UnitType::Tank as u16, 51);
        assert_eq!(UnitType::Harvester as u16, 0x5B);
        assert_eq!(UnitType::DeadWaldo as u16, 0x5C);
        assert_eq!(UnitType::DeadWaldo as u16, 92);
        // enums.hpp
        assert_eq!(UnitOrder::Await as u8, 0x00);
        assert_eq!(UnitOrder::Order17 as u8, 0x12);
        assert_eq!(UnitOrder::HaltBuilding2 as u8, 0x1F);
        assert_eq!(UnitOrder::MoveToAttack as u8, 0x1E);
        assert_eq!(UnitOrderState::Init as u8, 0x00);
        assert_eq!(UnitOrderState::BuildAbort as u8, 0x2E);
        assert_eq!(GameStateType::FatalError as u8, 0x0F);
        assert_eq!(GameStateType::InGame as u8, 0x08);
        assert_eq!(TeamKind::None as u8, 0);
        assert_eq!(TeamKind::Player as u8, 1);
        assert_eq!(TeamKind::Computer as u8, 2);
        assert_eq!(TeamKind::Remote as u8, 3);
        assert_eq!(TeamKind::Eliminated as u8, 4);
        assert_eq!(TeamClan::Random as u8, 0);
        assert_eq!(TeamClan::AxisInc as u8, 8);
        assert_eq!(Planet::Bottleneck as u8, 23);
        assert_eq!(FileType::MultiScenario as u8, 9);
        assert_eq!(ResearchTopic::Cost as u8, 7);
        // save.md
        assert_eq!(TeamIndex::Red as u8, 0);
        assert_eq!(TeamIndex::Alien as u8, 4);
    }

    #[test]
    fn unit_type_tags_match_resources() {
        // Токен — первая часть doc-комментария варианта (`/// TOKEN / C++_NAME`);
        // он же ключ UFN-таблицы в слое представления, поэтому у 93 вариантов
        // ровно 93 различных токена в формате ресурса.
        assert_eq!(UnitType::COUNT, 93);
        assert_eq!(UnitType::GoldRefinery.tag(), "COMMTWR");
        assert_eq!(UnitType::MaterialStorage.tag(), "ADUMP");
        assert_eq!(UnitType::FuelStorage.tag(), "FDUMP");
        assert_eq!(UnitType::ResearchCentre.tag(), "RESEARCH");
        assert_eq!(UnitType::EcoSphere.tag(), "GREENHSE");
        assert_eq!(UnitType::Artillery.tag(), "ARTYTRRT");
        assert_eq!(UnitType::MissileLauncher.tag(), "ANTIMSSL");
        assert_eq!(UnitType::ConcreteBlock.tag(), "BLOCK");
        assert_eq!(UnitType::ArmouredPersonnelCarrier.tag(), "CLNTRANS");
        assert_eq!(UnitType::Infiltrator.tag(), "COMMANDO");
        assert_eq!(UnitType::Escort.tag(), "FASTBOAT");
        assert_eq!(UnitType::Gunboat.tag(), "BATTLSHP");
        assert_eq!(UnitType::MissileCruiser.tag(), "MSSLBOAT");
        assert_eq!(UnitType::MiningStation.tag(), "MININGST");
        assert_eq!(UnitType::Harvester.tag(), "HARVSTER");
        assert_eq!(UnitType::DeadWaldo.tag(), "WALDO");

        let mut seen = std::collections::HashSet::new();
        for unit_type in UnitType::ALL {
            let tag = unit_type.tag();
            assert!(
                (4..=8).contains(&tag.len()),
                "{unit_type:?}: токен «{tag}» не в формате ресурса (4–8 символов)"
            );
            assert!(
                tag.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "{unit_type:?}: токен «{tag}» не в формате ресурса (A-Z, 0-9, _)"
            );
            assert!(seen.insert(tag), "повторяющийся токен «{tag}» ({unit_type:?})");
        }
        assert_eq!(seen.len(), UnitType::COUNT);
    }

    #[test]
    fn all_u8_enums_roundtrip() {
        roundtrip_u8(TeamKind::ALL);
        roundtrip_u8(TeamIndex::ALL);
        roundtrip_u8(TeamClan::ALL);
        roundtrip_u8(Opponent::ALL);
        roundtrip_u8(PlayMode::ALL);
        roundtrip_u8(VictoryType::ALL);
        roundtrip_u8(FileType::ALL);
        roundtrip_u8(Planet::ALL);
        roundtrip_u8(ResearchTopic::ALL);
        roundtrip_u8(GameStateType::ALL);
        roundtrip_u8(WinLossState::ALL);
        roundtrip_u8(AiStrategy::ALL);
        roundtrip_u8(CautionLevel::ALL);
        roundtrip_u8(UnitOrder::ALL);
        roundtrip_u8(UnitOrderState::ALL);
        roundtrip_u8(UnitType::ALL);
    }

    #[test]
    fn all_enums_roundtrip_u16() {
        roundtrip_u16(TeamKind::ALL);
        roundtrip_u16(TeamIndex::ALL);
        roundtrip_u16(TeamClan::ALL);
        roundtrip_u16(Opponent::ALL);
        roundtrip_u16(PlayMode::ALL);
        roundtrip_u16(VictoryType::ALL);
        roundtrip_u16(FileType::ALL);
        roundtrip_u16(Planet::ALL);
        roundtrip_u16(ResearchTopic::ALL);
        roundtrip_u16(GameStateType::ALL);
        roundtrip_u16(WinLossState::ALL);
        roundtrip_u16(AiStrategy::ALL);
        roundtrip_u16(CautionLevel::ALL);
        roundtrip_u16(UnitOrder::ALL);
        roundtrip_u16(UnitOrderState::ALL);
        roundtrip_u16(UnitType::ALL);
    }

    #[test]
    fn enum_roundtrip_is_exhaustive() {
        // Все сырые значения u8/u16 либо декодируются, либо дают ошибку.
        for raw in 0..=u8::MAX {
            let _ = TeamKind::try_from(raw);
            let _ = TeamIndex::try_from(raw);
            let _ = TeamClan::try_from(raw);
            let _ = Opponent::try_from(raw);
            let _ = PlayMode::try_from(raw);
            let _ = VictoryType::try_from(raw);
            let _ = FileType::try_from(raw);
            let _ = Planet::try_from(raw);
            let _ = ResearchTopic::try_from(raw);
            let _ = GameStateType::try_from(raw);
            let _ = WinLossState::try_from(raw);
            let _ = AiStrategy::try_from(raw);
            let _ = CautionLevel::try_from(raw);
            let _ = UnitOrder::try_from(raw);
            let _ = UnitOrderState::try_from(raw);
            let _ = Surface::try_from(raw);
        }
        for raw in 0..=u16::MAX {
            let _ = UnitType::try_from(raw);
        }
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(UnitType::try_from(93u16).is_err());
        assert!(UnitType::try_from(0x5Eu16).is_err());
        assert!(UnitType::try_from(u16::MAX).is_err());
        assert!(UnitOrder::try_from(32u8).is_err());
        assert!(UnitOrderState::try_from(0x2Fu8).is_err());
        assert!(GameStateType::try_from(16u8).is_err());
        assert!(TeamKind::try_from(5u8).is_err());
        assert!(TeamIndex::try_from(5u8).is_err());
        assert!(Planet::try_from(24u8).is_err());
        assert!(Surface::try_from(0x10u8).is_err());

        let err = UnitType::try_from(0x5Fu16).unwrap_err();
        assert_eq!(err, InvalidEnumValue::new("UnitType", 0x5F));
        assert!(err.to_string().contains("UnitType"));
    }

    #[test]
    fn surface_is_a_bitmask() {
        let land_coast = Surface::LAND | Surface::COAST;
        assert_eq!(land_coast.bits(), 0x05);
        assert!(land_coast.contains(Surface::LAND));
        assert!(land_coast.contains(Surface::COAST));
        assert!(!land_coast.contains(Surface::WATER));
        assert!(land_coast.intersects(Surface::COAST | Surface::AIR));
        assert!(Surface::NONE.is_empty());
        assert_eq!((!Surface::LAND).bits(), 0x0E);
        assert_eq!(
            Surface::from_bits_truncate(0xFF).bits(),
            Surface::ALL.bits()
        );

        // Все 16 допустимых комбинаций битов проходят round-trip.
        for bits in 0u8..=0x0F {
            let surface = Surface::from_bits(bits).unwrap();
            assert_eq!(Surface::try_from(surface.bits()).unwrap(), surface);
        }
        for bits in 0x10u8..=u8::MAX {
            assert!(Surface::from_bits(bits).is_none());
        }
    }

    #[test]
    fn team_ids_and_indices() {
        assert_eq!(TeamId::RED, TeamId(0));
        assert_eq!(TeamId::GREEN, TeamId(1));
        assert_eq!(TeamId::BLUE, TeamId(2));
        assert_eq!(TeamId::GRAY, TeamId(3));
        assert_eq!(TeamId::ALIEN, TeamId(4));
        assert_eq!(TeamId::MAX, TeamId(5));

        for index in TeamIndex::ALL {
            let team: TeamId = (*index).into();
            assert_eq!(TeamIndex::try_from(team).unwrap(), *index);
        }
        assert!(TeamIndex::try_from(TeamId::MAX).is_err());
    }

    #[test]
    fn unit_id_bitfield() {
        assert_eq!(UnitId::NO_ID.0, 0xFFFF);
        assert!(UnitId::NO_ID.is_no_id());
        assert!(!UnitId(0).is_no_id());

        let id = UnitId::new(TeamId::GREEN, 7);
        assert_eq!(id.0, (1 << 13) | 7);
        assert_eq!(id.team(), TeamId::GREEN);
        assert_eq!(id.object_index(), 7);

        // Индекс маскируется 13 битами, как в C++.
        let id = UnitId::new(TeamId::GRAY, 0xFFFF);
        assert_eq!(id.object_index(), 0x1FFF);
        assert_eq!(id.team(), TeamId::GRAY);
    }

    #[test]
    fn byte_constants_match_cpp() {
        assert_eq!(unit_flags::GROUND_COVER, 0x1);
        assert_eq!(unit_flags::BUILDING, 0x10);
        assert_eq!(unit_flags::MOBILE_AIR_UNIT, 0x40);
        assert_eq!(unit_flags::ELECTRONIC_UNIT, 0x200000);
        assert_eq!(unit_flags::REGENERATING_UNIT, 0x10000000);

        assert_eq!(CARGO_FUEL, 0x20);
        assert_eq!(CARGO_GOLD, 0x40);
        assert_eq!(CARGO_MATERIALS, 0x80);
        assert_eq!(CARGO_MASK, 0x1F);

        assert_eq!(CONNECTOR_NORTH_LEFT, 0x1);
        assert_eq!(CONNECTOR_WEST_BOTTOM, 0x80);
        assert_eq!(CONNECTION_BEING_TESTED, 0x400);

        assert_eq!(AI_STATE_NO_RETREAT, 1);
        assert_eq!(AI_STATE_MOVE_FINISHED_REMINDER, 0x100);
        assert_eq!(AI_STATE_RETREAT_REMINDER, 0x200);
        assert_eq!(RESOURCE_MAP_VISIBILITY_MASK, 0x3C00);
    }
}
