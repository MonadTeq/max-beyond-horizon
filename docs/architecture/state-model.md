# Модель состояния партии M.A.X. — спецификация для `max_core`

> Статус: **рабочая спецификация** (заменяет прежний черновик-скелет).
> Источники: авторская спецификация формата сохранений v1.04 (V70) —
> [`max-port-site/save.md`](../max-port-site/save.md); заголовки и код C++-порта —
> [`max-cpp-port/src/`](../max-cpp-port/src/) (`enums.hpp`, `unitinfo.hpp`,
> `unitvalues.hpp`, `complex.hpp`, `teamunits.hpp`, `ctinfo.hpp`,
> `units_manager.hpp`, `game_manager.hpp`, `resourcetable.hpp`, `hash.hpp`,
> `saveload.cpp`, `unitinfo.cpp`); каркас Rust-крейта —
> [`max-rust/crates/max_core/src/`](../max-rust/crates/max_core/src/).

## 0. Обозначения

| Обозначение | Значение |
|---|---|
| **save.md** | `max-port-site/save.md` — спецификация бинарного формата V70 (главный источник раскладки байт). |
| **C++** | `max-cpp-port/src/` — заголовки/код порта; источник имён, семантики и уточнений типов. |
| **Rust-тип** | Предлагаемый тип для `max_core` (чистая модель, serde `Serialize/Deserialize`, без I/O). |
| **Проекция** | `S` — игровое состояние, входит в серверный снапшот; `UI` — состояние экрана клиента, в снапшот **не** входит; `T` — техническое поле сериализации сохранений; `—` — исключено из модели/серверная внутренняя деталь. |
| `[T; N]` | Массив фиксированного размера (как в исходном формате). |

Сетевой контракт клиент↔сервер здесь не определяется (см. «Открытые вопросы», п. 7.1);
этот документ задаёт только **форму состояния**, из которой снапшот будет собираться.

---

## 1. Обзор

### 1.1 Что такое состояние партии

Состояние партии M.A.X. — совокупность данных, полностью определяющих ход игры:

1. **Настройки партии (setup)** — неизменяемы после старта: планета, миссия,
   состав команд (тип/клан/имя), RNG-зерно, таймеры, режим игры, тип/лимит
   победы, уровни ресурсов. Источник: `SaveFormatHeader` + `IniOptions`.
2. **Игровое состояние (game state)** — изменяется каждый ход: карта
   (поверхность/ресурсы), команды (очки, исследования, счётчики, статистика),
   технологии и комплексы команд (`TeamUnits`), юниты (`UnitInfo`), ход/фаза
   (`GameManagerState`).
3. **UI-состояние экрана (screen state)** — положение камеры, зум, выбранный
   юнит, переключатели отображения. Хранится в оригинале в `TeamInfo`
   (поэтому попадает в файл сохранения), но **не является игровым состоянием**:
   оно не влияет на симуляцию и в сетевой игре у каждого клиента своё.

Сервер (`max-port` в headless-режиме, см. [`src/server.cpp`](../src/server.cpp))
держит слои 1–2 авторитетно; клиент получает их через снапшот и держит слой 3
локально. Крейт `max_core` описывает слои 1–2 + минимальные UI-поля (см. раздел 6
«Проекция»).

### 1.2 Числа, с которыми все согласны

- Карта всегда **112 × 112** тайлов (размер задаёт WRL-файл мира; в практике V70 — 112).
- Типов юнитов **93** (`UNIT_END` = 93 = 0x5D в C++).
- Команд **4** игровых (red/green/blue/gray) + **1** псевдокоманда
  «пришельцы» (alien/derelict) для нейтральных юнитов; порядок: red, green, blue, gray.
- Оригинальный формат — little-endian, без выравнивания (packed).

### 1.3 Имена C++-порта vs имена save.md

C++-порт местами переименовал структуры и поля (часто точнее по смыслу).
Канонические **значения** и **порядок полей** — из save.md (V70);
**семантика и имена** — из C++. Основные расхождения перечислены в таблицах и
собраны в разделе 7 «Открытые вопросы».

---

## 2. Перечисления

Все перечисления в `max_core` предлагаются как Rust-`enum` с
`#[repr(uN)]` и derive `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`
(стиль каркаса `max_core`).

### 2.1 TeamType (тип команды)

save.md: `enum TeamType : unsigned char`; C++: `enum PlayerType : uint8_t` (`enums.hpp`).
Rust: `TeamKind` — каркас уже имеет `team::TeamKind` с теми же значениями
(`Player` в C++ = `HUMAN` в save.md).

| Знач. | save.md (`TeamType`) | C++ (`PlayerType`) | Rust (`TeamKind`, каркас) | Смысл |
|--:|--|--|--|---|
| 0 | `TEAM_TYPE_NONE` | `TEAM_TYPE_NONE` | `None` | пустой слот |
| 1 | `TEAM_TYPE_HUMAN` | `TEAM_TYPE_PLAYER` | `Player` | человек за этим компьютером |
| 2 | `TEAM_TYPE_COMPUTER` | `TEAM_TYPE_COMPUTER` | `Computer` | ИИ |
| 3 | `TEAM_TYPE_REMOTE` | `TEAM_TYPE_REMOTE` | `Remote` | человек по сети |
| 4 | `TEAM_TYPE_ELIMINATED` | `TEAM_TYPE_ELIMINATED` | `Eliminated` | выбыла |

### 2.2 TeamClan (клан)

save.md: `enum TeamClan : unsigned char` (1–8); C++: `enum TeamClanType : uint8_t`
добавляет `TEAM_CLAN_RANDOM = 0` (`enums.hpp`). Rust: `#[repr(u8)] enum TeamClan`.

| Знач. | save.md | C++ | Знач. | save.md | C++ |
|--:|--|--|--:|--|--|
| 0 | — | `TEAM_CLAN_RANDOM` | 5 | `TEAM_CLAN_MUSASHI` | `TEAM_CLAN_MUSASHI` |
| 1 | `TEAM_CLAN_THE_CHOSEN` | `TEAM_CLAN_THE_CHOSEN` | 6 | `TEAM_CLAN_SACRED_EIGHTS` | `TEAM_CLAN_SACRED_EIGHTS` |
| 2 | `TEAM_CLAN_CRIMSON_PATH` | `TEAM_CLAN_CRIMSON_PATH` | 7 | `TEAM_CLAN_7_KNIGHTS` | `TEAM_CLAN_7_KNIGHTS` |
| 3 | `TEAM_CLAN_VON_GRIFFIN` | `TEAM_CLAN_VON_GRIFFIN` | 8 | `TEAM_CLAN_AXIS_INC` | `TEAM_CLAN_AXIS_INC` |
| 4 | `TEAM_CLAN_AYERS_HAND` | `TEAM_CLAN_AYERS_HAND` | | | |

### 2.3 TeamIndex (индекс команды)

save.md: `enum TeamIndex : unsigned char` (0–3); C++: `enum PlayerTeamType : uint8_t`
добавляет `PLAYER_TEAM_ALIEN = 4` и `PLAYER_TEAM_MAX = 5` (`enums.hpp`).
Rust: `team::TeamId(pub u8)` уже в каркасе; значения:

| Знач. | save.md | C++ | Цвет/роль |
|--:|--|--|---|
| 0 | `TEAM_INDEX_RED` | `PLAYER_TEAM_RED` | красные |
| 1 | `TEAM_INDEX_GREEN` | `PLAYER_TEAM_GREEN` | зелёные |
| 2 | `TEAM_INDEX_BLUE` | `PLAYER_TEAM_BLUE` | синие |
| 3 | `TEAM_INDEX_GRAY` | `PLAYER_TEAM_GRAY` | серые |
| 4 | — | `PLAYER_TEAM_ALIEN` | нейтральные юниты-«пришельцы» (не команда в списке `TeamInfo`) |

В AI-объектах save.md использует 16-битный вариант `TeamIndex16 : unsigned short` (те же значения).

### 2.4 OpponentType (сложность ИИ)

save.md: `enum OpponentType : unsigned char`; C++: `enum OpponentType : uint8_t`
(`OPPONENT_TYPE_*`). Rust: `#[repr(u8)] enum Opponent { Clueless=0, Apprentice, Average, Expert, Master, God }`.

| 0 `CLUELESS` | 1 `APPRENTICE` | 2 `AVERAGE` | 3 `EXPERT` | 4 `MASTER` | 5 `GOD` |

### 2.5 PlayMode (режим игры)

save.md: `enum PlayMode : unsigned char`; C++ добавляет `PLAY_MODE_UNKNOWN = 2` (`enums.hpp`).

| Знач. | save.md | C++ | Rust-вариант |
|--:|--|--|---|
| 0 | `PLAY_MODE_TURN_BASED` | `PLAY_MODE_TURN_BASED` | `TurnBased` |
| 1 | `PLAY_MODE_SIMULTANEOUS_MOVES` | `PLAY_MODE_SIMULTANEOUS_MOVES` | `SimultaneousMoves` |
| 2 | — | `PLAY_MODE_UNKNOWN` | `Unknown` (только C++-порт) |

### 2.6 VictoryType (тип победы)

save.md: `enum VictoryType : char { VICTORY_TYPE_DURATION = 0, VICTORY_TYPE_SCORE = 1 }`;
C++: анонимный enum `VICTORY_TYPE_DURATION / VICTORY_TYPE_SCORE`.
Rust: `#[repr(u8)] enum VictoryType { Duration = 0, Score = 1 }`.

- `DURATION`: игра на число ходов (лимит = `victory_limit` ходов; побеждает лидер по очкам).
- `SCORE`: игра на очки (первый достигший `victory_limit` очков побеждает).

### 2.7 FileType (тип файла сохранения) — контекст

save.md: `enum FileType : unsigned char` (0–9: Custom, Tutorial, Campaign, Hot_seat,
Multiplayer, Demo, Debug, Text, Scenario, Multi_scenario; расширения dta/tra/cam/hot/
mlt/dmo/dbg/txt/sce/mps). В `max_core` не нужен как модель; оставить только если
сервер сообщает клиенту категорию партии.

### 2.8 PlanetType (планета)

save.md: `enum PlanetType : unsigned char`, 24 планеты; Rust: `#[repr(u8)] enum Planet`.

| 0 Snowcrab (снег) | 1 Frigia (снег) | 2 Ice Berg (снег) | 3 The Cooler (снег) | 4 Ultima Thule (снег) | 5 Long Floes (снег) |
|---|---|---|---|---|---|
| 6 Iron Cross (кратер) | 7 Splatterscape | 8 Peak-a-boo | 9 Valentine's Planet | 10 Three Rings | 11 Great divide |
| 12 New Luzon (зелень) | 13 Middle Sea | 14 High Impact | 15 Sanctuary | 16 Islandia | 17 Hammerhead |
| 18 Freckles (пустыня) | 19 Sandspit | 20 Great Circle | 21 Long Passage | 22 Flash Point | 23 Bottleneck |

### 2.9 ResearchTopic (тема исследований)

save.md: `enum ResearchTopic : unsigned char`; C++: `enum ResearchTopicType : uint8_t`
(+ `RESEARCH_TOPIC_COUNT = 8`). Rust: `#[repr(u8)] enum ResearchTopic`.
Порядок значений совпадает с атрибутами юнитов (`ATTRIB_*` в `unitvalues.hpp`).

| 0 `ATTACK` (атака) | 1 `SHOTS` (выстрелы) | 2 `RANGE` (радиус) | 3 `ARMOR` (броня) |
|---|---|---|---|
| 4 `HITS` (прочность) | 5 `SPEED` (скорость) | 6 `SCAN` (обзор) | 7 `COST` (стоимость) |

### 2.10 SurfaceType (проходимость клетки)

save.md: `enum SurfaceType : unsigned char` — **битовая маска** (не исключающие значения);
C++: анонимный enum + `SURFACE_TYPE_NONE = 0x0` (`enums.hpp`).
Rust: `#[repr(u8)] struct Surface(u8)` (bitflags) или `enum` со значениями 1/2/4/8.

| Бит | Значение | Rust-вариант (каркас `map.rs` уже имеет `Surface`) |
|--:|--|---|
| 0x0 | `SURFACE_TYPE_NONE` (только C++) | — |
| 0x1 | `SURFACE_TYPE_LAND` | `Surface::Land` |
| 0x2 | `SURFACE_TYPE_WATER` | `Surface::Water` |
| 0x4 | `SURFACE_TYPE_COAST` | `Surface::Coast` |
| 0x8 | `SURFACE_TYPE_AIR` | `Surface::Air` |

Карта хранит по одному байту на клетку (`SurfaceMap[112][112]`).

### 2.11 UnitType — полный список 93 типов

save.md: `enum UnitType : unsigned short`; C++: `enum ResourceID : uint16_t`,
игровые типы — диапазон `UNIT_START (0x00) … UNIT_END (0x5D = 93)` в
`resourcetable.hpp`; строковая таблица имён — `resourcetable.cpp`.
Rust: `#[repr(u16)] enum UnitType { ... }` с 93 вариантами.

> Замена каркаса: `unit::UnitKind` (короткий список + `Other(u8)`) заменяется на
> этот полный `UnitType`. Значения и порядок — строго по C++/save.md.

| № | C++ токен | save.md (`UNIT_TYPE_*`) | Декодировка токена (не офиц. название) |
|--:|--|--|---|
| 0 | `COMMTWR` | `GOLD_REFINERY` | Command Tower (в save.md названа по функции — переработка золота) |
| 1 | `POWERSTN` | `POWER_STATION` | Power Station |
| 2 | `POWGEN` | `POWER_GENERATOR` | Power Generator |
| 3 | `BARRACKS` | `BARRACKS` | Barracks |
| 4 | `SHIELDGN` | `ALIEN_BUILDING_1` | Shield Generator |
| 5 | `RADAR` | `RADAR` | Radar |
| 6 | `ADUMP` | `STORAGE_UNIT` | хранилище материалов (raw dump) |
| 7 | `FDUMP` | `FUEL_TANK` | топливный бак (fuel dump) |
| 8 | `GOLDSM` | `GOLD_VAULT` | хранилище золота (gold smelter) |
| 9 | `DEPOT` | `DEPOT` | Depot |
| 10 | `HANGAR` | `HANGAR` | Hangar |
| 11 | `DOCK` | `DOCK` | Dock |
| 12 | `CNCT_4W` | `CONNECTOR` | Connector (4-way) |
| 13 | `LRGRUBLE` | `LARGE_RUBBLE_1` | Large rubble |
| 14 | `SMLRUBLE` | `SMALL_RUBBLE_1` | Small rubble |
| 15 | `LRGTAPE` | `LARGE_TAPE` | Large tape |
| 16 | `SMLTAPE` | `SMALL_TAPE` | Small tape |
| 17 | `LRGSLAB` | `LARGE_SLAB` | Large slab |
| 18 | `SMLSLAB` | `SMALL_SLAB` | Small slab |
| 19 | `LRGCONES` | `LARGE_CONES` | Large cones |
| 20 | `SMLCONES` | `SMALL_CONES` | Small cones |
| 21 | `ROAD` | `ROAD` | Road |
| 22 | `LANDPAD` | `LANDING_PAD` | Landing pad |
| 23 | `SHIPYARD` | `SHIPYARD` | Shipyard |
| 24 | `LIGHTPLT` | `LIGHT_VEHICLE_PLANT` | Light vehicle plant |
| 25 | `LANDPLT` | `HEAVY_VEHICLE_PLANT` | Heavy vehicle plant («land plant») |
| 26 | `SUPRTPLT` | `ALIEN_BUILDING_2` | Support plant |
| 27 | `AIRPLT` | `AIR_UNITS_PLANT` | Air units plant |
| 28 | `HABITAT` | `HABITAT` | Habitat |
| 29 | `RESEARCH` | `RESEARCH_CENTER` | Research center |
| 30 | `GREENHSE` | `ECOSPHERE` | Ecosphere («greenhouse») |
| 31 | `RECCENTR` | `ALIEN_BUILDING_3` | Recreation center |
| 32 | `TRAINHAL` | `TRAINING_HALL` | Training hall |
| 33 | `WTRPLTFM` | `WATER_PLATFORM` | Water platform |
| 34 | `GUNTURRT` | `GUN_TURRET` | Gun turret |
| 35 | `ANTIAIR` | `ANTI_AIRCRAFT` | Anti-aircraft |
| 36 | `ARTYTRRT` | `ARTILLERY` | Artillery turret (стационарная артиллерия) |
| 37 | `ANTIMSSL` | `MISSILE_LAUNCHER` | Missile launcher |
| 38 | `BLOCK` | `CONCRETE_BLOCK` | Concrete block |
| 39 | `BRIDGE` | `BRIDGE` | Bridge |
| 40 | `MININGST` | `MINING_STATION` | Mining station |
| 41 | `LANDMINE` | `LAND_MINE` | Land mine |
| 42 | `SEAMINE` | `SEA_MINE` | Sea mine |
| 43 | `LNDEXPLD` | `LAND_EXPLOSION` | Land explosion |
| 44 | `AIREXPLD` | `AIR_EXPLOSION` | Air explosion |
| 45 | `SEAEXPLD` | `SEA_EXPLOSION` | Sea explosion |
| 46 | `BLDEXPLD` | `BUILDING_EXPLOSION` | Building explosion |
| 47 | `HITEXPLD` | `HIT_EXPLOSION` | Hit explosion |
| 48 | `MASTER` | `MASTER_BUILDER` | Master builder |
| 49 | `CONSTRCT` | `CONSTRUCTOR` | Constructor |
| 50 | `SCOUT` | `SCOUT` | Scout |
| 51 | `TANK` | `TANK` | Tank |
| 52 | `ARTILLRY` | `ASSAULT_GUN` | Assault gun (в C++ токен «artillery» — мобильная артиллерия) |
| 53 | `ROCKTLCH` | `ROCKET_LAUNCHER` | Rocket launcher |
| 54 | `MISSLLCH` | `MISSILE_CRAWLER` | Missile crawler |
| 55 | `SP_FLAK` | `MOBILE_ANTI_AIRCRAFT` | Mobile anti-aircraft (self-propelled flak) |
| 56 | `MINELAYR` | `MINE_LAYER` | Mine layer |
| 57 | `SURVEYOR` | `SURVEYOR` | Surveyor |
| 58 | `SCANNER` | `SCANNER` | Scanner |
| 59 | `SPLYTRCK` | `SUPPLY_TRUCK` | Supply truck |
| 60 | `GOLDTRCK` | `GOLD_TRUCK` | Gold truck |
| 61 | `ENGINEER` | `ENGINEER` | Engineer |
| 62 | `BULLDOZR` | `BULLDOZER` | Bulldozer |
| 63 | `REPAIR` | `REPAIR_UNIT` | Repair unit |
| 64 | `FUELTRCK` | `FUEL_TRUCK` | Fuel truck |
| 65 | `CLNTRANS` | `PERSONNEL_CARRIER` | Personnel carrier («colonist transport») |
| 66 | `COMMANDO` | `INFILTRATOR` | Infiltrator (commando) |
| 67 | `INFANTRY` | `INFANTRY` | Infantry |
| 68 | `FASTBOAT` | `ESCORT` | Escort (fast boat) |
| 69 | `CORVETTE` | `CORVETTE` | Corvette |
| 70 | `BATTLSHP` | `GUNBOAT` | Gunboat (внутр. «battleship») |
| 71 | `SUBMARNE` | `SUBMARINE` | Submarine |
| 72 | `SEATRANS` | `SEA_TRANSPORT` | Sea transport |
| 73 | `MSSLBOAT` | `MISSILE_CRUISER` | Missile cruiser |
| 74 | `SEAMNLYR` | `SEA_MINE_LAYER` | Sea mine layer |
| 75 | `CARGOSHP` | `CARGO_SHIP` | Cargo ship |
| 76 | `FIGHTER` | `FIGHTER` | Fighter |
| 77 | `BOMBER` | `GROUND_ATTACK_PLANE` | Ground attack plane (bomber) |
| 78 | `AIRTRANS` | `AIR_TRANSPORT` | Air transport |
| 79 | `AWAC` | `AWAC` | AWAC |
| 80 | `JUGGRNT` | `ALIEN_GUNBOAT` | Alien gunboat (juggernaut) |
| 81 | `ALNTANK` | `ALIEN_TANK` | Alien tank |
| 82 | `ALNASGUN` | `ALIEN_ASSAULT_GUN` | Alien assault gun |
| 83 | `ALNPLANE` | `ALIEN_ATTACK_PLANE` | Alien attack plane |
| 84 | `ROCKET` | `MISSILE` | Missile (rocket) |
| 85 | `TORPEDO` | `TORPEDO` | Torpedo |
| 86 | `ALNMISSL` | `ALIEN_MISSILE` | Alien missile |
| 87 | `ALNTBALL` | `TANK_PLASMA_BALL` | Alien tank plasma ball |
| 88 | `ALNABALL` | `ARTILLERY_PLASMA_BALL` | Alien artillery plasma ball |
| 89 | `RKTSMOKE` | `SMOKE_TRAIL` | Smoke trail (rocket smoke) |
| 90 | `TRPBUBLE` | `BUBBLE_TRAIL` | Bubble trail (torpedo bubble) |
| 91 | `HARVSTER` | `HARVESTER` | Harvester |
| 92 | `WALDO` | `DEAD_WALDO` | Dead Waldo (пасхалка) |

### 2.12 GameStateType — 16 состояний игры

save.md: `game_state` (u16) в `GameManagerState`, «16 состояний, многие транзитные».
C++: `enum GameStateType : uint8_t` (`enums.hpp`) — полная расшифровка.
Rust: `#[repr(u8)] enum GameStateType`.

| Знач. | C++ | Смысл (по использованию в `game_manager.cpp`/`remote.cpp`) |
|--:|--|---|
| 0x0 | `GAME_STATE_0_UNUSED` | не используется |
| 0x1 | `GAME_STATE_1_UNUSED` | не используется |
| 0x2 | `GAME_STATE_2_UNUSED` | не используется |
| 0x3 | `GAME_STATE_3_MAIN_MENU` | главное меню |
| 0x4 | `GAME_STATE_4_UNUSED` | не используется |
| 0x5 | `GAME_STATE_5_UNUSED` | не используется |
| 0x6 | `GAME_STATE_6_GAME_SETUP` | настройка игры |
| 0x7 | `GAME_STATE_7_SITE_SELECT` | выбор места высадки |
| 0x8 | `GAME_STATE_8_IN_GAME` | игровой цикл (в ходу) |
| 0x9 | `GAME_STATE_9_END_TURN` | конец хода (доигрывание приказов) |
| 0xA | `GAME_STATE_10_LOAD_GAME` | загрузка сохранения |
| 0xB | `GAME_STATE_11_TURN_ACTIVE` | ход активен (раздача приказов) |
| 0xC | `GAME_STATE_12_DEPLOYING_UNITS` | расстановка юнитов |
| 0xD | `GAME_STATE_13_SITE_SELECTED` | место высадки выбрано |
| 0xE | `GAME_STATE_14_EXIT_SITE_SELECT` | выход из выбора места |
| 0xF | `GAME_STATE_15_FATAL_ERROR` | фатальная ошибка |

### 2.13 OrderType (текущий приказ юнита)

save.md: `enum OrderType : unsigned char`, 32 значения 0x00–0x1F; C++: `enum
UnitOrderType : uint8_t` (`ORDER_*`, `ORDER_COUNT_MAX`). Rust: `#[repr(u8)] enum UnitOrder`.

> Не путать с `order::Order` из каркаса `max_core` — это **клиентская команда
> серверу** (Move/Attack/Build/...). Здесь `OrderType` — **внутренний приказ
> юнита** в модели симуляции. Рекомендация: `OrderType` → `UnitOrder`,
> `OrderStateType` → `UnitOrderState`, чтобы имена не пересекались.

| 0x00 `ORDER_AWAIT` | 0x01 `ORDER_TRANSFORM` | 0x02 `ORDER_MOVE` | 0x03 `ORDER_FIRE` |
|---|---|---|---|
| 0x04 `ORDER_BUILD` | 0x05 `ORDER_ACTIVATE` | 0x06 `ORDER_NEW_ALLOCATE` | 0x07 `ORDER_POWER_ON` |
| 0x08 `ORDER_POWER_OFF` | 0x09 `ORDER_EXPLODE` | 0x0A `ORDER_UNLOAD` | 0x0B `ORDER_CLEAR` |
| 0x0C `ORDER_SENTRY` | 0x0D `ORDER_LAND` | 0x0E `ORDER_TAKE_OFF` | 0x0F `ORDER_LOAD` |
| 0x10 `ORDER_IDLE` | 0x11 `ORDER_REPAIR` | 0x12 `ORDER_17` (save.md: `ORDER_TYPE_REFUEL`; в C++ имя не восстановлено) | 0x13 `ORDER_RELOAD` |
| 0x14 `ORDER_TRANSFER` | 0x15 `ORDER_HALT_BUILDING` | 0x16 `ORDER_AWAIT_SCALING` | 0x17 `ORDER_AWAIT_TAPE_POSITIONING` |
| 0x18 `ORDER_AWAIT_STEAL_UNIT` | 0x19 `ORDER_AWAIT_DISABLE_UNIT` | 0x1A `ORDER_DISABLE` | 0x1B `ORDER_MOVE_TO_UNIT` |
| 0x1C `ORDER_UPGRADE` | 0x1D `ORDER_LAY_MINE` | 0x1E `ORDER_MOVE_TO_ATTACK` | 0x1F `ORDER_HALT_BUILDING_2` |

### 2.14 OrderStateType (состояние исполнения приказа)

save.md: `enum OrderStateType : unsigned char`, 47 значений 0x00–0x2E; C++:
`enum UnitOrderStateType : uint8_t` (`ORDER_STATE_*`). Rust: `#[repr(u8)] enum UnitOrderState`.

| 0x00 `ORDER_STATE_INIT` | 0x01 `ORDER_STATE_EXECUTING_ORDER` | 0x02 `ORDER_STATE_READY_TO_EXECUTE_ORDER` | 0x03 `ORDER_STATE_STORE` |
|---|---|---|---|
| 0x04 `ORDER_STATE_PREPARE_STORE` | 0x05 `ORDER_STATE_IN_PROGRESS` | 0x06 `ORDER_STATE_IN_TRANSITION` | 0x07 `ORDER_STATE_ISSUING_PATH` |
| 0x08 `ORDER_STATE_READY_TO_FIRE` | 0x09 `ORDER_STATE_FIRE_IN_PROGRESS` | 0x0A `ORDER_STATE_10` | 0x0B `ORDER_STATE_BUILD_IN_PROGRESS` |
| 0x0C `ORDER_STATE_PATH_REQUEST_CANCEL` | 0x0D `ORDER_STATE_BUILD_CANCEL` | 0x0E `ORDER_STATE_DESTROY` | 0x0F `ORDER_STATE_LOADING_IN_PROGRESS` |
| 0x10 `ORDER_STATE_FINISH_LOADING` | 0x11 `ORDER_STATE_UNLOADING_IN_PROGRESS` | 0x12 `ORDER_STATE_FINISH_UNLOADING` | 0x13 `ORDER_STATE_FINISH_LANDING` |
| 0x14 `ORDER_STATE_FINISH_TAKE_OFF` | 0x15 `ORDER_STATE_21` | 0x16 `ORDER_STATE_PROGRESS_TRANSFORMING` | 0x17 `ORDER_STATE_FINISH_TRANSFORMING` |
| 0x18 `ORDER_STATE_CLEAR_PATH` | 0x19 `ORDER_STATE_SELECT_SITE` | 0x1A `ORDER_STATE_BUILD_CLEARING` | 0x1B `ORDER_STATE_EXPLODE` |
| 0x1C `ORDER_STATE_MOVE_INIT` | 0x1D `ORDER_STATE_MOVE_GETTING_PATH` | 0x1E `ORDER_STATE_BUILDING_READY` | 0x1F `ORDER_STATE_UNIT_READY` |
| 0x20 `ORDER_STATE_EXPAND` | 0x21 `ORDER_STATE_SHRINK` | 0x22 `ORDER_STATE_TAPE_POSITIONING_INIT` | 0x23 `ORDER_STATE_TAPE_POSITIONING_ENTER` |
| 0x24 `ORDER_STATE_TAPE_POSITIONING_DEINIT` | 0x25 `ORDER_STATE_TAPE_POSITIONING_LEAVE` | 0x26 `ORDER_STATE_ELEVATE` | 0x27 `ORDER_STATE_LOWER` |
| 0x28 `ORDER_STATE_ATTACK_PENDING` | 0x29 `ORDER_STATE_ATTACK_BEGINNING` | 0x2A `ORDER_STATE_NEW_ORDER` | 0x2B `ORDER_STATE_INACTIVE` |
| 0x2C `ORDER_STATE_PLACING_MINES` | 0x2D `ORDER_STATE_REMOVING_MINES` | 0x2E `ORDER_STATE_BUILD_ABORT` | |

### 2.15 WinLossState (исход для команды)

C++: `enum WinLossState` (`enums.hpp`); Rust: `#[repr(u8)] enum WinLossState`.

| Знач. | C++ | Смысл |
|--:|--|---|
| 0 | `VICTORY_STATE_GENERIC` | партия идёт по общим правилам |
| 1 | `VICTORY_STATE_PENDING` | условие вот-вот сработает (проверка в конце хода) |
| 2 | `VICTORY_STATE_WON` | победа |
| 3 | `VICTORY_STATE_LOST` | поражение |

---

## 3. Структуры

Вспомогательные типы, общие для всех структур:

| Структура | C-типы (save.md) | Rust-тип | Назначение |
|---|---|---|---|
| `Point` | `{ signed short x, y }` | `struct Point { x: i16, y: i16 }` | координаты сетки/пикселей |
| `ScreenLocation` | `{ signed char x, y }` | `struct ScreenLocation { x: i8, y: i8 }` | сохранённая позиция экрана (UI) |
| `Rect` | `{ signed int ulx, uly, lrx, lry }` | `struct Rect { ulx: i32, uly: i32, lrx: i32, lry: i32 }` | габариты спрайта (UI) |

### 3.1 SaveFormatHeader (заголовок / настройки партии)

save.md: `struct __attribute__((packed)) SaveFormatHeader`. Неизменяемые настройки
партии; шлются клиенту один раз (при входе/первом снапшоте).

| Поле | C-тип | Rust-тип | Назначение | Источник | Проекция |
|---|---|---|---|---|---|
| `version` | `unsigned short` | `u16` | версия формата (V70) | save.md | `T` |
| `save_game_type` | `FileType` (u8) | `Option<FileType>` | категория партии | save.md | `S` |
| `save_game_name` | `char[30]` | `String` (≤30 байт) | имя партии | save.md | `S` |
| `planet` | `PlanetType` (u8) | `Planet` | индекс планеты | save.md | `S` |
| `mission_index` | `unsigned short` | `u16` | миссия (0 = generic; иначе жёстко закодированные условия) | save.md | `S` |
| `team_name[4]` | `char[4][30]` | `[String; 4]` (red, green, blue, gray) | имена команд | save.md | `S` |
| `team_type[5]` | `TeamType[5]` | `[TeamKind; 5]` | типы 4 команд + слот «пришельцев» | save.md | `S` |
| `team_clan[5]` | `TeamClan[5]` | `[TeamClan; 5]` | кланы команд | save.md | `S` |
| `rng_seed` | `unsigned int` | `u32` | зерно ГПСЧ (сетевая игра детерминирована) | save.md | `S` |
| `opponent` | `OpponentType` (u8) | `Opponent` | сложность ИИ | save.md | `S` |
| `turn_timer_time` | `unsigned short` | `u16` | таймер хода, сек (0 = без лимита) | save.md | `S` |
| `endturn_time` | `unsigned short` | `u16` | таймер конца хода, сек (0 = без лимита) | save.md | `S` |
| `play_mode` | `PlayMode` (u8) | `PlayMode` | пошаговый/одновременный | save.md | `S` |

### 3.2 IniOptions (настройки партии из INI)

save.md: `struct __attribute__((packed)) IniOptions` — 12 значений `signed int`.
Частично дублирует заголовок (в кампаниях/сценариях заголовок **перекрывает**
`opponent/timer/endturn/play_mode` из INI).

| Поле | C-тип | Rust-тип | Назначение | Проекция |
|---|---|---|---|---|
| `world` | `signed int` | `u32` | планета (= `planet`) | `S` (дублирует заголовок) |
| `turn_timer` | `signed int` | `u32` | таймер хода, сек | `S` |
| `endturn` | `signed int` | `u32` | таймер конца хода, сек | `S` |
| `start_gold` | `signed int` | `u32` | стартовые кредиты 0–250 | `S` |
| `play_mode` | `signed int` | `PlayMode` | режим игры | `S` |
| `victory_type` | `signed int` | `VictoryType` | тип победы | `S` |
| `victory_limit` | `signed int` | `u32` | лимит победы (ходов или очков) | `S` |
| `opponent` | `signed int` | `Opponent` | сложность ИИ | `S` |
| `raw_resource` | `signed int` | `u32` | уровень материалов 0–2 (Poor/Medium/Rich) | `S` |
| `fuel_resource` | `signed int` | `u32` | уровень топлива 0–2 | `S` |
| `gold_resource` | `signed int` | `u32` | уровень золота 0–2 | `S` |
| `alien_derelicts` | `signed int` | `u32` | обломки пришельцев 0–2 (None/Rare/Common) | `S` |

### 3.3 IniPreferences (настройки UI — вне модели)

save.md: `IniPreferences` — 7 × `signed int` (`effects`, `click_scroll`, `quick_scroll`,
`fast_movement`, `follow_unit`, `auto_select`, `enemy_halt`). Это **пользовательские
предпочтения клиента**, они не влияют на симуляцию и в снапшот не входят (`—`).
В `max_core` их хранить не нужно (конфигурация клиента, не модели).
С P8 (`docs/analysis/visual-1to1-status.md` §27) клиент держит полный набор
настроек (`max_app::preferences::Preferences`): `IniPreferences` пишутся в
`SaveMeta::ini_preferences` при сохранении и читаются при загрузке
(`SaveLoad_Load`, `saveload.cpp:920-934`); `effects` управляет только кадровыми
анимациями (`App::update_unit_animations`), звук — `AudioPlayer`. Ни `effects`,
ни звук не входят в CRC/снапшот: два клиента с разными настройками остаются
детерминированными.

### 3.4 TeamInfo (состояние команды)

save.md: `struct __attribute__((packed)) TeamInfo`; C++: `struct CTInfo`
(`ctinfo.hpp`), глобальный массив `UnitsManager_TeamInfo[5]`
(`units_manager.hpp`). Четыре экземпляра (red, green, blue, gray).

| Поле | C-тип (V70) | Rust-тип | Назначение | Источник | Проекция |
|---|---|---|---|---|---|
| `markers[10]` | `Point[10]` | — | маркеры на карте; фича вырезана, всегда {-1,-1} | save.md | `—` |
| `team_type` | `TeamType` (u8) | `TeamKind` | тип команды | save.md | `S` |
| `finished_turn` | `bool` (1 байт) | `bool` | команда закончила ход | save.md | `S` |
| `team_clan` | `TeamClan` (u8) | `TeamClan` | клан команды | save.md | `S` |
| `research_topics[8]` | `ResearchTopicInfo[8]` | `[ResearchTopicInfo; 8]` | прогресс исследований (см. ниже) | save.md | `S` |
| `team_points` | `unsigned int` | `u32` | победные очки (экосферы) | save.md | `S` |
| `number_of_objects_created` | `unsigned short` | `u16` | счётчик созданных объектов 1–8191 (база для hash id) | save.md | `S` (нужен серверу; клиенту — нет) |
| `unit_counters[93]` | `unsigned char[93]` | `[u8; 93]` (C++: `uint32_t[UNIT_END]`) | счётчики имён юнитов по типам | save.md / ctinfo.hpp | `S` (нужен серверу для имён; клиент получает имена в `UnitInfo.name`) |
| `camera_positions[6]` | `ScreenLocation[6]` | `[ScreenLocation; 4]` (C++) | сохранённые позиции камеры F5–F8 | save.md / ctinfo.hpp | `UI` |
| `score_graph[50]` | `signed short[50]` | `[i16; 50]` | очки команды за последние 50 ходов (график отчёта) | save.md | `S` (выводимо на клиенте из истории, см. 6.3) |
| `selected_unit` | `unsigned short` | `Option<UnitId>` | hash id выбранного юнита (0xFFFF = нет) | save.md | `UI` |
| `zoom_level` | `unsigned short` | `u16` | зум карты 4–64 | save.md | `UI` |
| `camera_position` | `Point` | `Point` | центр экрана (0–111) | save.md | `UI` |
| `display_button_range` | `signed char` | `bool` | показ радиуса атаки | save.md | `UI` |
| `display_button_scan` | `signed char` | `bool` | показ обзора | save.md | `UI` |
| `display_button_status` | `signed char` | `bool` | показ статусов юнитов | save.md | `UI` |
| `display_button_colors` | `signed char` | `bool` | цвета команд | save.md | `UI` |
| `display_button_hits` | `signed char` | `bool` | полоски прочности | save.md | `UI` |
| `display_button_ammo` | `signed char` | `bool` | индикация боезапаса | save.md | `UI` |
| `display_button_minimap_2x` | `signed char` | `bool` | миникарта ×2 | save.md | `UI` |
| `display_button_minimap_tnt` | `signed char` | `bool` | миникарта в режиме «взрывчатка» | save.md | `UI` |
| `display_button_grid` | `signed char` | `bool` | сетка | save.md | `UI` |
| `display_button_names` | `signed char` | `bool` | имена юнитов | save.md | `UI` |
| `display_button_survey` | `signed char` | `bool` | карта разведки ресурсов | save.md | `UI` |
| `stats_factories_built` | `signed short` | `i16` | заводов построено (статистика) | save.md | `S` |
| `stats_mines_built` | `signed short` | `i16` | шахт построено | save.md | `S` |
| `stats_buildings_built` | `signed short` | `i16` | зданий построено | save.md | `S` |
| `stats_units_built` | `signed short` | `i16` | мобильных юнитов построено | save.md | `S` |
| `casualties[93]` | `unsigned short[93]` | `[u16; 93]` (C++: `uint32_t[UNIT_END]`) | потери по типам; **используется в условиях победы/поражения** | save.md / ctinfo.hpp | `S` |
| `stats_gold_spent_on_upgrades` | `signed short` | `i16` (C++: `u32`) | золото, потраченное на апгрейды | save.md / ctinfo.hpp | `S` |

`ResearchTopicInfo` (save.md; C++: `struct ResearchTopic` в `ctinfo.hpp`):

| Поле | C-тип | Rust-тип | Назначение |
|---|---|---|---|
| `research_level` | `unsigned int` | `u32` | завершённые циклы исследования |
| `turns_to_complete` | `unsigned int` | `u32` | ходов на цикл при 1 центре (0 = не начато) |
| `allocation` | `unsigned int` | `u32` | сколько исследовательских центров выделено |

Фактические ходы: `turns = ceil(turns_to_complete / allocation)`.

### 3.5 UnitValues (атрибуты типа юнита)

save.md: `struct __attribute__((packed)) UnitValues`; C++: `class UnitValues : FileObject`
(`unitvalues.hpp`, `unitvalues.cpp`). В файле каждый экземпляр начинается с
`object_index` (u16) и `class_type` (u16) — это механика классового сериализатора,
**не состояние** (`T`). Таблица ниже — содержательные поля.

| Поле | C-тип | Rust-тип | Назначение | Источник | Проекция |
|---|---|---|---|---|---|
| `turns` | `unsigned short` | `u16` | ходов на постройку | save.md / unitvalues.hpp | `S` |
| `hits` | `unsigned short` | `u16` | прочность | save.md | `S` |
| `armor` | `unsigned short` | `u16` | броня | save.md | `S` |
| `attack` | `unsigned short` | `u16` | атака | save.md | `S` |
| `speed` | `unsigned short` | `u16` | скорость | save.md | `S` |
| `range` | `unsigned short` | `u16` | дальность | save.md | `S` |
| `rounds` | `unsigned short` | `u16` | выстрелы за атаку | save.md | `S` |
| `move_and_fire` | `unsigned char` | `u8` | может стрелять на ходу | save.md | `S` |
| `scan` | `unsigned short` | `u16` | радиус обзора | save.md | `S` |
| `storage` | `unsigned short` | `u16` | вместимость груза; у `COMMANDO`/`REGENERATING_UNIT` хранит **опыт** | save.md / unitinfo.cpp | `S` |
| `ammo` | `unsigned short` | `u16` | боезапас | save.md | `S` |
| `attack_radius` | `unsigned short` | `u16` | радиус поражения по площади | save.md | `S` |
| `agent_adjust` | `unsigned short` | `u16` | бонус агента (диверсанта) | save.md | `S` |
| `version` | `unsigned short` | `u16` | «марка» (уровень апгрейда), старт = 1 | save.md | `S` |
| `units_built` | `unsigned char` (V70) | `bool` (C++: `m_is_in_use`) | занят ли слот значений (в V70 — счётчик построенных) | save.md / unitvalues.cpp | `S` (сервер) |

Примечания C++: поле `fuel` объявлено в `unitvalues.hpp`, но **не сериализуется**
(только runtime); в V70 последний байт читается как `legacy_units_built` →
`m_is_in_use = (units_built != 0)` (`unitvalues.cpp`).

### 3.6 Complex (комплекс зданий)

save.md: `struct __attribute__((packed)) Complex`; C++: `class Complex : FileObject`
(`complex.hpp`). Комплекс — связная группа зданий, делящих энергию/грузы.

| Поле | C-тип | Rust-тип | Назначение | Источник | Проекция |
|---|---|---|---|---|---|
| `material` | `short` | `i16` | материалы (запас + добыча) | save.md / complex.hpp | `S` |
| `fuel` | `short` | `i16` | топливо | save.md | `S` |
| `gold` | `short` | `i16` | золото | save.md | `S` |
| `power` | `short` | `i16` | излишек энергии | save.md | `S` |
| `workers` | `short` | `i16` | свободная рабочая сила | save.md | `S` |
| `buildings` | `short` | `i16` | зданий в комплексе | save.md | `S` |
| `id` | `short` | `i16` | уникальный индекс комплекса (монотонный в команде) | save.md | `S` |
| `object_index`/`class_type` | `unsigned short` ×2 | — | механика сериализатора | save.md | `T` |

`power`/`workers` — не склады, а **баланс** членов комплекса: каждый ход
`Access_UpdateResourcesTotal` (`access.cpp:912-934`) кладёт сюда
`Cargo_GetNetProduction` (`cargo.cpp:70-130`), а
[`crate::power::optimize_complex`](../max-rust/crates/max_core/src/power.rs)
включает производителей (`POWERSTN`/`POWGEN`) и гасит потребителей при
дефиците (`production_manager.cpp:481-790`). Кэш входит в снапшот/CRC; приказы
`PowerOn/PowerOff` — часть `UnitInfo::orders`. Сообщение о нехватке
(`ProductionManager::DrawResourceMessage`) — отчёт `GameState::industry_message`,
в дайджест не входит.

### 3.7 TeamUnits (экономика и технологии команды)

save.md: `struct __attribute__((packed)) TeamUnits`; C++: `class TeamUnits`
(`teamunits.hpp`), экземпляры `ResourceManager_TeamUnitsRed/Green/Blue/Gray`.

| Поле | C-тип | Rust-тип | Назначение | Проекция |
|---|---|---|---|---|
| `gold` | `signed short` (V70) | `i32` (C++: `uint32_t`) | кредиты команды | `S` |
| `base_unit_values[93]` | `UnitValues[93]` | `[UnitValues; 93]` | базовые атрибуты (с учётом клана) | `S` |
| `current_unit_values[93]` | `UnitValues[93]` | `[UnitValues; 93]` | текущие атрибуты (после исследований) — по ним создаются новые юниты | `S` |
| `complex_count` | `unsigned short` | `u16` (→ `Vec` длина) | число комплексов | `T` |
| `complexes[]` | `Complex[complex_count]` | `Vec<Complex>` | комплексы команды | `S` |

### 3.8 GameManagerState (глобальное состояние партии)

save.md: `struct __attribute__((packed)) GameManagerState`; в C++ это не структура,
а глобалы (`GameManager_ActiveTurnTeam`, `GameManager_PlayerTeam`,
`GameManager_TurnCounter`, `GameManager_GameState`, `GameManager_TurnTimerValue` —
`game_manager.hpp`).

| Поле | C-тип (V70) | Rust-тип | Назначение | Проекция |
|---|---|---|---|---|
| `active_turn_team` | `TeamIndex` (u8) | `TeamId` | чей ход (кто управляет UI; актуально и в simultaneous) | `S` |
| `player_team` | `TeamIndex` (u8) | `TeamId` | команда игрока (hot-seat: активный человек) | `S` |
| `turn_counter` | `signed int` | `i32` | номер текущего хода | `S` |
| `game_state` | `unsigned short` (C++: `uint8_t`) | `GameStateType` | состояние игры (16 значений, §2.12) | `S` |
| `turn_timer` | `unsigned short` | `u16` | остаток таймера хода, сек | `S` |

### 3.9 UnitInfo (юнит — полная структура)

V70-раскладка: save.md `struct __attribute__((packed)) UnitInfo`; runtime-модель C++:
`class UnitInfo` (`unitinfo.hpp`), семантика полей V70 уточнена в `unitinfo.cpp`
(`UnitInfo::FileLoad`, ветка `SmartFileFormat::V70`). Юниты хранятся в 5 списках
(см. §3.11); для клиента — плоский список.

Колонка «Проекция»: `S` — в снапшот; `UI` — визуальное/клиентское (клиент
предсказывает или не хранит); `—` — серверная внутренняя деталь/не слать.

| Поле (V70) | C-тип | Rust-тип | Назначение / примечание | Проекция |
|---|---|---|---|---|
| `object_index`, `class_type` | `unsigned short` ×2 | — | механика сериализатора | `T` |
| `unit_type` | `UnitType` (u16) | `UnitType` | тип юнита (§2.11) | `S` |
| `hash_id` | `unsigned short` | `UnitId(u16)` (каркас) | уникальный id юнита (0xFFFF = нет id; см. §4) | `S` |
| `flags` | `unsigned int` | `u32` (bitflags: `GROUND_COVER`…`REGENERATING_UNIT`, `enums.hpp`) | класс юнита, поведение, команда-владелец в хэше | `S` |
| `pixel_position` | `Point` | `Point` | пиксельная позиция (анимация) | `UI` |
| `grid_position` | `Point` | `Point` | клетка карты (C++: `grid_x/grid_y : int16_t`) | `S` |
| `name_length` + `name` | `u16` + `char[]` | `String` | имя юнита (переменной длины, без терминатора в V70) | `S` |
| `shadow_offset` | `Point` | `Point` | смещение тени | `UI` |
| `team` | `TeamIndex` (u8) | `TeamId` | команда-владелец (в т.ч. «пришельцы» = 4) | `S` |
| `unit_id` | `unsigned char` | `u8` | порядковый номер типа в команде (для имён) | `S` (необязательно клиенту) |
| `brightness` | `unsigned char` | `u8` | яркость (визуал повреждения) | `UI` |
| `angle` | `unsigned char` | `u8` | угол корпуса 0..7 — **направление шага** (`UnitInfo::Move`: `DIRECTION_OFFSETS[angle]`); меняется `Paths_UpdateAngle`/`GroundPath::UpdateUnitAngle` | `S` (снапшот; в сетевой CRC не входит — как C++ `GameManager_GetUnitListChecksum`) |
| `visible_to_team[5]` | `unsigned char[5]` | `[bool; 5]` | видимость для каждой команды (туман войны) | `S` (см. 7.5) |
| `spotted_by_team[5]` | `unsigned char[5]` | `[bool; 5]` | обнаружен командами (не только виден) | `S` (см. 7.5) |
| `max_velocity` | `unsigned char` | `u8` | максимальная скорость анимации | `UI` |
| `velocity` | `unsigned char` | `u8` | текущая скорость анимации | `UI` |
| `sound` | `unsigned char` | `u8` | звуковой эффект юнита | `UI` |
| `scaler_adjust` | `unsigned char` | `u8` | масштаб спрайта | `UI` |
| `sprite_bounds`, `shadow_bounds` | `Rect` ×2 | `Rect` ×2 | кэш габаритов спрайта | `UI` |
| `turret_angle`, `turret_offset_x/y` | `u8` + `i8`×2 | `u8` + `i8`×2 | анимация башни | `UI` |
| `total_images`, `image_base`, `turret_image_base`, `firing_image_base`, `connector_image_base` | `signed short` ×5 | `i16` ×5 | индексы спрайтов | `UI` |
| `image_index`, `turret_image_index`, `image_index_max` | `signed short` ×3 | `i16` ×3 | текущий кадр анимации | `UI` |
| `orders` | `OrderType` (u8) | `UnitOrder` | текущий приказ | `S` |
| `state` | `OrderStateType` (u8) | `UnitOrderState` | состояние исполнения приказа | `S` |
| `prior_orders`, `prior_state` | `u8` ×2 | `UnitOrder`, `UnitOrderState` | отложенный приказ (возврат после завершения) | `S` |
| `laying_state` | `unsigned char` | `u8` | маскировка/постановка мин (подлодка/миноукладчик) | `S` |
| `target_grid` | `Point` | `Point` | целевая клетка (C++: `move_to_grid_x/y` = `fire_on_grid_x/y` в V70) | `S` |
| `build_time` | `unsigned char` | `u8` | осталось ходов до конца постройки | `S` |
| `total_mining`, `raw_mining`, `fuel_mining`, `gold_mining` | `unsigned char` ×4 | `u8` ×4 | суммарная и поресурсная добыча | `S` |
| `raw_mining_max`, `gold_mining_max`, `fuel_mining_max` | `unsigned char` ×3 | `u8` ×3 | максимумы добычи | `S` |
| `hits` | `unsigned char` (V70) | `u8` (C++: `u16`) | прочность | `S` |
| `speed` | `unsigned char` (V70) | `u8` (C++: `u16`) | скорость | `S` |
| `shots` | `unsigned char` | `u8` | выстрелы в этом ходу | `S` |
| `move_and_fire` | `unsigned char` | `u8` | стрельба на ходу | `S` |
| `storage` | `signed short` | `i16` | груз; у `COMMANDO`/`REGENERATING_UNIT` — **опыт** (C++ выделяет `experience`) | `S` |
| `ammo` | `unsigned char` | `u8` | боезапас | `S` |
| `targeting_mode` | `unsigned char` | `u8` | режим наведения | `S` |
| `enter_mode` | `unsigned char` | `u8` | посадка/высадка | `S` |
| `cursor` | `unsigned char` | `u8` | курсор над юнитом | `UI` |
| `recoil_delay` | `signed char` | `u8` ×2 (C++ split) | отдача выстрела **или** оставшиеся ходы отключения (если `ORDER_DISABLE`) → C++: `firing_recoil_frames` + `disabled_turns_remaining` | `S` |
| `delayed_reaction` | `unsigned char` | `u8` | отложенная реакция (сетевая синхронизация) | `S` |
| `damaged_this_turn` | `bool` | `bool` | повреждён в этом ходу | `S` |
| `research_topic` | `unsigned char` | `ResearchTopic` | тема исследования (для зданий) | `S` |
| `moved` | `unsigned char` | `u8` | счётчик движения (лимит на ход) | `S` |
| `bobbed` | `bool` | `bool` | покачивание на воде (визуал) | `UI` |
| `shake_effect_state` | `unsigned char` | `u8` | эффект тряски (визуал) | `UI` |
| `engine`, `weapon` | `unsigned char` ×2 | `u8` ×2 | состояние анимации двигателя/оружия | `UI` |
| `comm` | `unsigned char` | — | не используется C++ при загрузке V70 | `—` (см. 7.6) |
| `fuel_distance` | `unsigned char` | — | не используется C++ при загрузке V70 | `—` (см. 7.6) |
| `move_fraction` | `unsigned char` | `u8` | доля движения между клетками (анимация) | `UI` |
| `energized` | `bool` | — | не используется C++ при загрузке V70 | `—` (см. 7.6) |
| `repeat_build` | `unsigned char` | `bool` | повторная постройка (завод) | `S` |
| `build_rate` | `unsigned short` | `u16` | скорость постройки (множитель) | `S` |
| `disabled_reaction_fire` | `bool` | `bool` | ответный огонь выключен | `S` |
| `auto_survey` | `bool` | `bool` | авто-разведка | `S` |
| `field_221` | `unsigned int` | — (C++: `ai_state_bits : u32`) | AI-состояние (`AI_STATE_*` в `unitinfo.hpp`) | `—` (только сервер) |
| `path` | `Path` (класс) | `Vec<PathStep>` (упрощённо) | маршрут движения | `S` (упрощённая форма) |
| `connectors` | `unsigned short` | `u16` | битфилд подключений комплекса (`CONNECTOR_*`) | `S` |
| `base_values` | `UnitValues` (объект) | `Box<UnitValues>` | атрибуты этого юнита (с апгрейдами) | `S` |
| `complex` | `Complex` (объект) | `i16` (id комплекса) | принадлежность к комплексу | `S` |
| `parent_unit` | `UnitInfo` (объект) | `Option<UnitId>` | родитель (здание-завод/транспорт) | `S` |
| `enemy_unit` | `UnitInfo` (объект) | `Option<UnitId>` | текущая цель-враг | `S` |
| `build_list` | `UnitTypeArray` | `Vec<UnitType>` | очередь постройки завода | `S` |

Runtime-поля C++ без аналога в V70 (не сериализуются): `group_speed`,
`transfer_cargo` (= `move_to_grid_x` в V70), `stealth_dice_roll`,
`movement_wait_count`, `live_attack`, `in_transit`, `pin_count`, `tasks`,
`ai_state_bits` (в V70 — `field_221`), `experience` (из `storage` в V70).

### 3.10 Карта: SurfaceMap и ResourceMap

**SurfaceMap** (проходимость): `SurfaceType[112][112]`, байт на клетку —
битовая маска из §2.10. Rust: `[Surface; 12544]` (каркас `map::Map.surface`
использует `Vec<Surface>`; предложение — `[Surface; MAP_TILES]`,
`MAP_TILES = 112*112`).

**ResourceMap** (ресурсы и разведка): `short GridResourceMapEntry[112][112]`,
u16-битфилд на клетку (save.md; C++: `ResourceManager_CargoMap`, `CARGO_*` в `enums.hpp`):

| Биты | Маска | Поле | Значение |
|--:|--:|---|---|
| 0–4 | `0x001F` | `RESOURCE_AMOUNT_MASK` | количество ресурса (игра ограничивает 16) |
| 5 | `0x0020` | `CARGO_FUEL` | тип = топливо |
| 6 | `0x0040` | `CARGO_GOLD` | тип = золото |
| 7 | `0x0080` | `CARGO_MATERIALS` | тип = материалы (должен быть установлен ровно один бит типа) |
| 5–7 | `0x00E0` | `RESOURCE_TYPE_MASK` | маска типа |
| 10 | `0x0400` | `TEAM_VISIBILITY_GRAY` | клетка разведана серыми |
| 11 | `0x0800` | `TEAM_VISIBILITY_BLUE` | разведана синими |
| 12 | `0x1000` | `TEAM_VISIBILITY_GREEN` | разведана зелёными |
| 13 | `0x2000` | `TEAM_VISIBILITY_RED` | разведана красными |
| 10–12 | `0x1C00` | `TEAM_VISIBILITY_MASK` | **⚠ в save.md не накрывает красных (0x2000); фактическая маска всех четырёх — `0x3C00`** (см. 7.3) |

Пример из save.md: `0x208C` = 12 материалов, разведано только красными.
Rust-предложение (вместо `map::Map.resources: Vec<Option<ResourceKind>>` из каркаса):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceMapEntry {
    pub amount: u8,        // bits 0..4
    pub kind: ResourceKind, // Fuel | Gold | Raw (from bits 5..7)
    pub visibility: u8,    // bitmask 0x0F: bit i = команда i разведала
}
pub struct Map {
    pub size: MapSize,           // width: i16, height: i16 (каркас)
    pub surface: [Surface; MAP_TILES],
    pub resources: [ResourceMapEntry; MAP_TILES],
}
```

`ResourceKind` каркаса (`Raw/Fuel/Gold`) совпадает по смыслу — сохранить.

### 3.11 Служебные структуры сохранения (вне модели ядра)

Эти структуры есть в файле V70, но не переносятся в клиентскую модель 1:1:

| Структура | Что это | Решение для max_core |
|---|---|---|
| 5 списков `UnitInfoList` (ground_cover / mobile_land_sea / stationary / mobile_air / particles) | организация юнитов по слоям отрисовки и логики | клиенту — плоский `Vec<UnitInfo>`; разбиение на слои — выводится из `flags` (`GROUND_COVER`, `MOBILE_AIR_UNIT`, …) |
| `Hash_UnitInfo` / `Hash_MapHash` | хэш-таблицы id→юнит и клетка→юниты | серверная инфраструктура; на клиенте — `HashMap<UnitId, usize>` / пересборка по позициям |
| `Path` / `PathStep` | маршрут (air/builder/ground) | `Vec<Point>` в снапшоте |
| `TeamHeatMaps` (×4) | 3 карты 112×112: `heatmap_complete`, `heatmap_stealth_sea`, `heatmap_stealth_land` (туман войны для мини-карты) | см. 7.5: либо серверная (фильтровать снапшот по команде), либо клиент пересчитывает из `visible_to_team` |
| `MessageLogList` (×4) | журнал сообщений (`text`, `unit`, `coordinates`, `is_alert_message`, `resource_id`) | не снапшот; отдельный поток событий (сообщения/алерты) |
| `AiPlayer` (×4, только `TEAM_TYPE_COMPUTER`) | состояние ИИ: стратегия, карты целей/мин, `info_map`/`mine_map` 112×112 | `—` только сервер |

### 3.12 `SaveMeta` — что переносится ради записи V70 (P3)

`GameState` хранит слои 1–2 (см. §1.1), но в V70 есть поля, которых в
симуляции нет. Для записи сейва 1:1 они живут в `GameState::save_meta`
(`max_core::save_write::SaveMeta`); в канонический дайджест
(`GameState::snapshot_bytes`) **не входят**, потому что не влияют на игру:

| Поле `SaveMeta` | Источник в V70 | Почему не выводится из состояния |
|---|---|---|
| `save_game_type`, `save_name`, `world`, `team_names`, `rng_seed`, `opponent`, `turn_timer_time`, `endturn_time` | `SaveFormatHeader` | настройки партии; `Randomizer` хранит только состояние, не seed |
| `ini_options`, `ini_preferences` | `IniOptions`/`IniPreferences` | 12 + 7 значений INI, часть дублирует заголовок |
| `team_ui` (×4) | `markers[10]`, `camera_positions[6]`, `selected_unit`, `zoom_level`, `camera_position`, `display_button_*` | UI-состояние (`CTInfo`), у нас — в `hud`/`Viewport`; запись — из снимка на момент save |
| `team_gold_spent_raw` (×4) | `stats_gold_spent_on_upgrades` (`int16_t`) | C++ клампит ≥ 0 при загрузке; сырое значение нужно для побайтового round-trip |
| `alien_team_type`, `alien_team_clan` | `team_type[4]`/`team_clan[4]` заголовка | V70 пишет 5-й слот, `CTInfo` его не хранит; `GameState.teams[4]` — служебный ALIEN |
| `ai_unused_v70`, `ai_target_team_v70` (×4) | `AiPlayer::FileLoad` (V70-поля) | C++ **читает и выбрасывает**; нужны только для байтов |
| `ai_has_info_map`, `ai_has_mine_map` (×4) | наличие блоков `info_map`/`mine_map` | в эталонных V70-сейвах карт ИИ нет (`item_count = 0`), а рантайм всегда держит 112×112 |
| `message_logs` (×4) | `MessageLogList` | журнал — UI-поток событий; `max_core` хранит как данные для записи/чтения |
| `turn_timer` | `GameManagerState::turn_timer` | остаток таймера живёт в `LocalGame`, снимок берётся при save |
| `unit_hash_size`, `map_hash_size`, `map_hash_x_shift`, `unit_hash_order`, `map_hash_cells` | заголовки и порядок `UnitHash`/`MapHash` | C++ вставляет юнитов в ведро при создании, а `MapHash` при загрузке перехэширует; порядок из состояния не выводится — переносится из файла |

Запись: `GameState::to_save()` → `SaveFile` → `GameState::to_save_bytes()`.
Для `SAVE1.CAM`/`SAVE2.CAM` цикл «файл → `GameState` → файл» даёт **те же
байты** (`max_core/tests/save_write.rs`).

---

## 4. Unit hash id (битфилд)

Единый 16-битный идентификатор юнита, производный от `number_of_objects_created`
и индекса команды (save.md; C++: `UnitsManager_UnitIdCounter`, старт
`PLAYER_TEAM_MAX << 13` после загрузки V70 — `saveload.cpp`, `units_manager.cpp`).

| Биты | Содержимое | Диапазон |
|--:|---|---|
| 0–12 | порядковый индекс объекта команды (из `number_of_objects_created`) | 1–8191 (2^13) |
| 13–15 | индекс команды (5 команд: red…gray + «пришельцы») | 0–4 |
| всё 0xFFFF | `NO_ID` — «нет id» (юниты вне хэша; `selected_unit` при пустом выборе) | — |

- `hash_id = (team << 13) | object_index`.
- В C++ `UnitInfo::NO_ID = UINT32_MAX`; в V70-файле хранится 16-битный `0xFFFF` и
  преобразуется при загрузке (`unitinfo.cpp`).
- Rust: каркас уже использует `unit::UnitId(pub u16)` — совпадает; константа
  `UnitId::NO_ID = UnitId(0xFFFF)`.

---

## 5. Условия победы и поражения

### 5.1 Generic (`mission_index == 0`)

**Победа** (по `victory_type` + `victory_limit`, save.md):

1. `VICTORY_TYPE_SCORE`: первый игрок, достигший `victory_limit` очков — победитель.
2. `VICTORY_TYPE_DURATION`: при достижении лимита ходов побеждает лидер по очкам.
3. Независимо от типа: если все соперники выполнили условия поражения —
   оставшийся игрок побеждает.
4. Ничьи: сначала сравнивается «стоимость базы» (сумма ходов постройки уцелевших
   зданий при скорости 1×, с учётом клановых бонусов и текущих атрибутов команд —
   чем меньше, тем «ценнее»); затем — меньший индекс команды (red < green < blue < gray).

**Поражение** (generic loss, диаграмма `loss_conditions.svg` в save.md): команда
проигрывает, когда одновременно теряет способность строить, добывать и воевать
(нет строителей/зданий-производителей и нет военной мощи); известные угловые
случаи (шахта без залежей под ней не считается «работающей», стелс-юниты не
считаются военной мощью, истребители — считаются) описаны в save.md.

### 5.2 Уникальные условия (`mission_index != 0`)

Правила **жёстко закодированы** в оригинале по `mission_index` (обучение,
сценарии, кампания) и сводятся к комбинациям предикатов над состоянием:

| Класс предиката | Примеры (save.md) |
|---|---|
| количество юнитов типа у команды (`unit_counters`/списки) | «Gun Turret > 0» (тренировка 2), «Mining Station == 0» (тренировка 5), «Ecosphere == 0» (кампания 1) |
| суммарные потери (`casualties[]`) | «потери Research Center > 0» (кампания 2) |
| ресурсы | «Raw Materials > 0 и Fuel > 0» (тренировка 1), «Gold > 0» (тренировка 8) |
| очки команды и прогноз | «Team Points > 0» (тренировка 10), «прогноз очков красных ≥ очки зелёных» (сценарий 20; прогноз = очки + число экосфер) |
| исследования/апгрейды | «Armor Research Level > 0» (тренировка 15), «Upgraded Tank > 0» (тренировка 9) |
| ходы | «Turn Counter == 30» (кампания 2), «Turn Counter == 100» (кампания 8) |
| добыча/потребление | «Total Raw Mining > 21» (тренировка 4), «Fuel mining − consumption > 0» (тренировка 7) |
| состояние юнитов | «Damaged Tank == 0» (тренировка 13), «Missing Ammo Tank == 0» (тренировка 14), «Infiltrator c Experience > 0» (тренировка 11) |

**Моделирование в max_core**: сами правила считает **сервер** (в оригинале они
зашиты в коде; в порту — см. ветки проверки победы в `game_manager.cpp`).
Клиентской модели достаточно знать `mission_index`, `victory_type`,
`victory_limit` и получать результат: `WinLossState` (§2.15) на команду в
снапшоте. Предикаты выше — спецификация для реализации серверной проверки.

---

## 6. Проекция: что входит в серверный снапшот клиенту

### 6.1 Принципы

1. **Снапшот = игровое состояние (слои 1–2 из §1.1), не состояние экрана.**
   В оригинале UI-поля попали в `TeamInfo` только потому, что сохранение
   снимает слепок всего процесса, включая экран. Для клиент-сервера их слать
   нельзя: у каждого клиента своя камера, свой зум и свои переключатели
   отображения; в сетевой игре оригинала эти поля не синхронизируются и не
   участвуют в сверке рассинхронизации.
2. **Сервер — единственный авторитет**; клиент предсказывает визуальное
   (позиции анимации, спрайты, угол), а игровые значения получает из снапшота
   (`docs/protocol.md`, инварианты).
3. **Технические поля формата** (`object_index`, `class_type`,
   `complex_count`, счётчики сериализатора) в модель не переносятся.
4. **Ссылки на объекты** в снапшоте — по id (`UnitId`, id комплекса), а не
   вложенные структуры (в отличие от V70, где объекты вкладываются рекурсивно).

### 6.2 Сводная таблица проекции по структурам

| Структура | Проекция | Комментарий |
|---|---|---|
| `SaveFormatHeader` | `S` (без `version`) | неизменяемые настройки; один раз (присоединение/первый снапшот) |
| `IniOptions` | `S` | настройки партии; дубли заголовка можно не слать дважды |
| `IniPreferences` | `—` | клиентская конфигурация |
| `TeamInfo` | `S` за вычетом UI-полей | см. 6.3 |
| `UnitValues` (base/current) | `S` | технологии/апгрейды влияют на бой и UI покупки апгрейдов |
| `Complex` | `S` | ресурсы/энергия/рабочие показываются в UI и нужны для предпросмотра |
| `TeamUnits` | `S` | экономика команды |
| `GameManagerState` | `S` | фаза/ход/таймер — базовое состояние партии |
| `UnitInfo` | `S` за вычетом `UI`-полей | см. 6.4 |
| `SurfaceMap` | `S` | статична в партии; можно один раз |
| `ResourceMap` | `S` (тип+количество — всем; биты видимости — по п. 7.5) | |
| `Hash_UnitInfo`/`Hash_MapHash`, списки | `—` | клиент пересобирает индексы сам |
| `TeamHeatMaps` | открытый вопрос (7.5) | предложение: не слать, клиент строит туман из `visible_to_team` |
| `MessageLog` | `—` в снапшоте | события отдельным каналом |
| `AiPlayer` | `—` | только сервер |

### 6.3 Аргументация по UI-полям `TeamInfo`

**Не входят в снапшот** (`UI`), потому что это состояние конкретного экрана, а
не партии: `camera_positions` (F5–F8), `selected_unit`, `zoom_level`,
`camera_position`, все 11 `display_button_*`, `markers`.

- `camera_position`/`zoom_level`/`camera_positions`: в оригинале — где игрок
  смотрит; два клиента в одной партии видят разные места карты. Передавать
  серверу их не нужно (и незачем); каждый клиент хранит свои.
- `selected_unit`: выбор — клиентское действие; серверу выбранный юнит не нужен
  (приказы приходят с явным `UnitId` в `order::Order` каркаса). В сохранении
  поле живёт только ради восстановления экрана после загрузки.
- `display_button_*`: переключатели слоёв отображения (сетка/имена/обзор и
  т.д.) — чисто визуальные, на симуляцию не влияют.
- `markers`: фича вырезана из игры (всегда {-1,-1}) — исключаем полностью.
- Спорные: `score_graph[50]` и `stats_*`/`casualties` — **включаем** (`S`):
  `casualties` напрямую используется условиями победы/поражения; `stats_*` —
  итоговая статистика партии (сервер должен её авторитетно знать); `score_graph`
  — история очков, технически выводима на клиенте из дельт, но массив в 50
  значений дешевле слать, чем восстанавливать после реконнекта.

### 6.4 Проекция полей `UnitInfo`

Полная разметка — в таблице §3.9. Суть:

- **`S` (в снапшот)**: идентичность (`hash_id`, `unit_type`, `team`, `name`),
  позиция (`grid_position`), боевые/экономические атрибуты (`hits`, `speed`,
  `shots`, `ammo`, `storage`, `move_and_fire`, mining-поля, `build_time`,
  `build_rate`, `repeat_build`), приказы (`orders`, `state`, `prior_*`,
  `target_grid`, `research_topic`, `moved`, `targeting_mode`, `enter_mode`,
  `recoil/disable`, `delayed_reaction`, `damaged_this_turn`,
  `disabled_reaction_fire`, `auto_survey`, `laying_state`), структура
  (`flags`, `connectors`, `complex`→id, `parent_unit`→id, `enemy_unit`→id,
  `build_list`), путь (`path` → `Vec<Point>`), видимость (`visible_to_team`,
  `spotted_by_team`), атрибуты юнита (`base_values`), угол корпуса (`angle`:
  от него зависит направление шага в `UnitInfo::Move`, а не только кадр).
- **`UI` (не слать)**: всё спрайтово-анимационное (`pixel_position`,
  `velocity`, `max_velocity`, `brightness`, `scaler_adjust`, `sprite_bounds`,
  `shadow_bounds`, `shadow_offset`, `turret_*`, `image_*`, `bobbed`,
  `shake_effect_state`, `engine`, `weapon`, `cursor`, `move_fraction`). Всё это
  выводится из `grid_position` + параметров движения/повреждения либо вообще не
  нужно без рендера оригинала.
- **`—`**: `field_221`/`ai_state_bits` (AI только на сервере), `comm`/
  `fuel_distance`/`energized` (C++-порт не восстанавливает — см. 7.6),
  `object_index`/`class_type` (`T`).

### 6.5 Что не входит ни в модель, ни в снапшот

- Механика классового сериализатора (`object_index`, `class_type`,
  `last_object_index`, рекурсивные вложения объектов).
- Состояние меню/экранов (не-игровые `GAME_STATE_3_MAIN_MENU` и т.п. сервер
  просто не будет слать как «партию»; в снапшоте фазы — только игровые).
- `IniPreferences` и любые настройки клиента.

---

## 7. Открытые вопросы

1. **Кодирование снапшота.** Форма состояния зафиксирована здесь; конкретное
   кодирование сообщений (`snapshot`/`delta`/`welcome`) — в
   [`docs/protocol.md`](protocol.md) (пока таблица-каркас) и в реализации
   headless-сервера [`src/server.cpp`](../src/server.cpp)
   (+ [`docs/dedicated-server-plan.md`](dedicated-server-plan.md)). Нужно решить:
   binary vs JSON/MessagePack, полный снапшот vs дельта, частота (ход/кадр),
   формат `Path` и ссылок на юниты.
2. **Согласование с каркасом max_core.** Заменить `unit::UnitKind` на полный
   `UnitType` (§2.11); `map::Map.resources: Vec<Option<ResourceKind>>` — на
   `ResourceMapEntry` с количеством и видимостью (§3.10); добавить
   `GameStateType`, `UnitOrder(State)`, `ResearchTopicInfo`, `UnitValues`,
   `Complex`, `TeamUnits`, `TeamInfo`, `UnitInfo`; `TeamKind::Player` vs
   `TEAM_TYPE_HUMAN` — выбрать одно имя.
3. **Маска видимости ресурсов.** В save.md `TEAM_VISIBILITY_MASK = 0x1C00`
   (биты 10–12) не накрывает `TEAM_VISIBILITY_RED = 0x2000` (бит 13);
   фактическая маска всех четырёх команд — `0x3C00`. Уточнить по оригиналу
   (вероятно, опечатка в спецификации).
4. **`camera_positions`: 6 в V70 против 4 в C++.** Оригинал хранит 6
   `ScreenLocation`, но клавиши F5–F8 дают 4 слота; C++-порт держит 4.
   Уточнить назначение 5-го/6-го элемента (возможно, вторая пара — зум/вид).
5. **Туман войны в клиент-сервере.** В оригинале каждый клиент имеет полное
   состояние и сам фильтрует по `visible_to_team`/`spotted_by_team`/heat maps
   (lockstep). В клиент-серверной модели полный снапшот = тривиальный чит.
   Варианты: (а) сервер фильтрует снапшот по команде (дорого, но честно);
   (б) слать всё + пометить поля видимости (как в оригинале, доверие клиенту);
   (в) слать `heatmap_complete` команды + видимые юниты. Требует решения в
   `docs/protocol.md`.
6. **Не восстановленные поля V70.** `comm`, `fuel_distance`, `energized` в
   `UnitInfo` читаются, но не используются C++-портом (`unitinfo.cpp`) — их
   семантика не подтверждена. Порядок `OrderType` 0x12: save.md называет
   `REFUEL`, C++ держит заглушку `ORDER_17`. Уточнить по RE оригинального
   бинарника, если понадобится полная совместимость сохранений.
7. **Типы, различающиеся между V70 и C++.** `TeamUnits.gold` (i16 vs u32),
   `hits`/`speed` юнита (u8 vs u16), `unit_counters`/`casualties` (u8/u16 vs
   u32), `game_state` (u16 vs u8), `stats_gold_spent_on_upgrades` (i16 vs u32),
   `recoil_delay` (i8 → пара `firing_recoil_frames`/`disabled_turns_remaining`).
   Для формата сохранений канон — V70 (save.md); для сетевого снапшота —
   предложенные Rust-типы (расширенные, как в C++).
8. **`number_of_objects_created`/`unit_counters`.** В оригинале счётчик на
   команду (u16, переполнение имён при >255 юнитов типа); в C++-порту единый
   глобальный `UnitsManager_UnitIdCounter`. Для сервера зафиксировать схему
   присвоения id (§4) и переполнение имён.
9. **Условия победы/поражения.** Список предикатов (§5.2) — сводка; полный
   перечень на каждый `mission_index` и порядок проверок (`WinLossState`
   `PENDING`/`WON`/`LOST`) следует выписать отдельной таблицей при реализации
   серверной проверки (источник: save.md §Victory and Loss Conditions +
   `game_manager.cpp`).
10. **Score graph.** Слать массивом (предложение §6.3) или пересчитывать на
    клиенте из дельт очков — выбрать при проектировании дельт.
11. **Уникальность `name`.** Имя юнита переменной длины в V70; в снапшоте
    предложение — слать с юнитом при создании/переименовании, а не в каждом
    кадре (уточнить в протоколе).
