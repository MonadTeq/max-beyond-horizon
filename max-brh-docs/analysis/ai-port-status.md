# Статус порта ИИ и план фазы A

Результат аудита 1.2.1 (сверка `max_core::ai` с C++-оракулом). Этот документ —
рабочая база для фазы **A** (полный порт ИИ 1:1) и перечень уже закрытых
багфиксов (фаза **B**).

## Итог аудита

Каркас ИИ портирован аккуратно, но **поведенческая часть — в основном
упрощения**, а не 1:1 с C++. Текущий ИИ — упрощённая эвристика.

## Что совпадает 1:1

- перечисление 10 стратегий (`AiStrategy`), маппинг клан→стратегия,
  `RollGreenhouseRatio`/`RollMinefieldDensity`;
- константы приоритетов задач, реестр типов задач (49 имён / 44 класса);
- RNG-алгоритм, диспетчеризация жизненного цикла задач.

## Критичные расхождения (влияют на поведение)

| # | Область | Статус | Что расходится |
|---|---|---|---|
| 1 | Вес юнитов / состав армии | частично (A.3) | таблицы заполняются и используются (`GetWeightTable`/`GetFilteredWeightTable`/`GetExtendedWeightTable`, `RollUnitType`); нет `DetermineResearchProjects`, `ChooseUpgrade` (A.5) |
| 2 | Выбор стратегии/высадки | закрыто (A.2) | — |
| 3 | Производство | закрыто (A.3), кроме резерва | эвристика заменена на `PlanNextBuildJob`/`UpdateMiningNeeds`/`CheckPower`/`CheckWorkers`/склады `EndTurn`; производство юнитов по весам; нет `RollTeamMissionSupplies` (стартовая загрузка) и `TaskAttackReserve` (A.4) |
| 4 | Бой | частично | точечная атака в радиусе; нет `EvaluateAssault`/наступления, `GetRequiredDamage`/`GetProjectedDamage` |
| 5 | Разведка | отсутствует | нет целенаправленного исследования карты |
| 6 | Флот | отсутствует | не строит/не использует морские юниты; нет `AttemptTransport` |
| 7 | Исследования/апгрейды | отсутствует | нет выбора исследований и апгрейдов юнитов |
| 8 | Детерминизм | не бит-в-бит | другой порядок решений → при том же seed другие ходы |

## Фаза B — закрытые багфиксы (коммит `b9eb781`)

1. `EndTurn` у Attack/KillUnit вызывал `BeginTurn` → исправлено (`kill_unit_end_turn`/`attack_end_turn`).
2. `create_building_activate` терял `building_id` → исправлено (используется, порт `Activate`/`Finish`/`AddUnit`).
3. `attack_priority` жёстко `ATTACK_DEFAULT` → заменено на `priority_of`; формула `TaskAttack::GetPriority` внедрена.
4. `TaskMineAssistant` получал `CHECK_ASSAULTS` → `FRONTIER` (`taskmineassistant.cpp:27`).
5. Морские мины убирал наземный `MineLayer` → `SeaMineLayer` по `Access_GetModifiedSurfaceType` (`taskremovemines.cpp:101-112`).
6. `TaskAttack::GetPriority`/`TaskGetMaterials::GetPriority` отсутствовали → добавлены (`taskattack.cpp:101-112`, `taskgetmaterials.cpp:39-50`).
7. `TaskUpdateTerrain` получал `CHECK_ASSAULTS` → `EMERGENCY` (`taskupdateterrain.cpp:35`).

CRC/replay без изменений; 385 тестов зелёные.

## Фаза A — полный порт ИИ (по убыванию влияния на геймплей)

1. **Вес-таблицы + состав армии** (`WeightTable` ×18, `UpdateWeightTables`,
   `RebuildWeightTables`) — база всех решений ИИ.
2. **Выбор стратегии/высадки** (`SelectStrategy`, континенты, `SelectLandingSite`,
   детерминированный RNG).
3. **Производство** (`ChooseStrategic/GenericBuildOrders`, `PlanNextBuildJob`,
   заводы/армия, `RollTeamMissionSupplies`). — **закрыто в A.3** (кроме
   `RollTeamMissionSupplies`; см. ниже).
4. **Бой** (`EvaluateAssault`, `GetRequiredDamage`/`GetProjectedDamage`,
   census целей, `IsReconUnitAvailable`, `IsAttackUnderControl`).
5. **Исследования/апгрейды** (`DetermineResearchProjects`, `ChooseUpgrade`).
6. **Разведка и флот** (`TaskExplore`, `AttemptTransport`, морские задачи).

Каждый пункт — отдельная задача Разработчику (v4 Flash) со сверкой по
`aiplayer.cpp`/`task*.cpp` и фиксацией CRC/replay.

## Фаза A.3 — производство и стройка (закрыто)

Портировано (новый модуль `ai/build_orders.rs`, `ai/tasks/build.rs`):

- **Весовые потребители**: `GetWeightTable` (18 таблиц; `INVALID_ID` —
  композиция air defense + bomber + submarine + таблицы всех замеченных типов +
  `COMMANDO` на `OPPONENT_TYPE_EXPERT`), `GetFilteredWeightTable`
  (`0x01` — `STATIONARY`, `0x02` — `REGENERATING_UNIT`), `GetExtendedWeightTable`
  (alien + expert → только `COMMANDO`; прочий alien → таблица `ADUMP`; вода у
  стационарной цели → `CORVETTE`, на `AVERAGE`+ ещё `SUBMARNE`; недостижимые
  LAND/WATER-типы обнуляются через `IsSurfaceTypePresent`/`ZoneWalker`).
- **Заказы на стройку**: `AddBuildOrder`, `CheckReconnaissanceNeeds`,
  `GetMaxSpottedScan`, `ChooseStrategicBuildOrders`, `ProcessBuildOrders`,
  `ChooseGenericBuildOrders`. `AiPlayer::BeginTurn` строит
  `strategic_build_orders` и `generic_build_orders` (`aiplayer.cpp:2503-2504`);
  потребитель — `ChooseUpgrade` (A.5).
- **`PlanNextBuildJob`**: `GetUnitCount` (units + планы по приоритету),
  `CreateBuildings`, `CreateBuilding` (buildability, время миссии, ценз
  строителей `builder_count * 2 + 1`, один план на тип — замена
  `ClearPlannedBuildings`, поиск площадки), заводы по стратегии
  (`DEFENSIVE`/`SCOUT_HORDE` → LIGHTPLT, `TANK_HORDE`/`COMBINED_ARMS` → LANDPLT,
  `AIR` → AIRPLT, `SEA` → SHIPYARD, `ESPIONAGE` → TRAINHAL), гринхаусы/исследования,
  ремонтные цеха (`DEPOT`/`HANGAR`/`DOCK`/`BARRACKS`) и их ёмкость.
- **`CheckNeeds`** = `PlanNextBuildJob` → `UpdateMiningNeeds` (потребность в
  raw/fuel/gold по ставкам `cargo.cpp`, ёмкость шахт, `COMMTWR`) → `MININGST`.
  `EndTurn` менеджера — склады `ADUMP`/`FDUMP`/`GOLDSM` по заполнению.
- **Ассистенты**: `CheckPower` (баланс энергии, `POWERSTN`/`POWGEN`) и
  `CheckWorkers` (`HABITAT`); `TaskRadarAssistant` — порог времени миссии +
  «радара нет» (полный `EvaluateNeedForRadar` требует `AccessMap`).
- **Юниты по весам**: ветка фабрики из `TaskAttackReserve::AddUnit`
  (`taskattackreserve.cpp:136-164`) выполняется менеджером
  (`plan_unit_production`: `GetFilteredWeightTable(INVALID_ID, 3)` →
  `RollUnitType` → `TaskObtainUnits`); сам резерв (`TaskAttackReserve`) — A.4.
  `TaskKillUnit::FindVaildTypes` использует `GetExtendedWeightTable(target, 0x01)`.
- **Стартовые здания**: `AiPlayer::BeginTurn` регистрирует в менеджере
  платформы/мосты, все не-idle стационарные здания и строящих строителей
  (`aiplayer.cpp:2335-2355`); без этого `PlanNextBuildJob` не видел базу.

Осознанные отклонения (те же, что и раньше): площадки ищет `find_build_site`
(`AccessMap`-разметка P5/P7), `FindSiteForRadar`/`FindDefenseSite`/коннекторы не
портированы; производство юнитов размещено в менеджере до A.4.

CRC/дайджесты изменились (ожидаемо — ИИ строит иначе). Контрольные сценарии
(20 ходов, карта 28×28):

| Сценарий | `state_checksum` до → после | `replay_digest_crc` до → после |
|---|---|---|
| шахта + конструктор | `0x83cf` → `0x5853` | `0x3bf0` → `0x032f` |
| армия + замеченный враг | `0x3a33` → `0xde48` | `0x4816` → `0xd955` |
| завод (average) | `0xa88f` → `0xb557` | `0x9324` → `0xfc1e` |
| завод (expert) | `0xa88f` → `0xb557` | `0xaeef` → `0x59f0` |

Причина: другой состав задач (ассистенты, `CreateBuilding` заводов и шахт),
другие решения о стройке (`PlanNextBuildJob`/`UpdateMiningNeeds` вместо
эвристики по ходам) и новые вызовы RNG в `RollUnitType`; при одинаковом seed
путь решений расходится, как и ожидалось на этом этапе. `p7`/replay-векторы
(без ИИ) не изменились. Тесты: 274 unit + 136 интеграционных, все зелёные.


## Открытые вопросы владельца

- Требование к детерминизму бит-в-бит ИИ (п. 8) — цель или допустимо «та же
  стратегия, но другой seed-путь» до полного порта?
- Порядок фаз A: рекомендуемый — по списку выше (вес-таблицы → стратегия →
  производство → бой → исследования → разведка/флот).
