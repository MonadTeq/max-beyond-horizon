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
| 1 | Вес юнитов / состав армии | частично (A.3) | таблицы заполняются и используются (`GetWeightTable`/`GetFilteredWeightTable`/`GetExtendedWeightTable`, `RollUnitType`); `ChooseUpgrade` портирован в A.5 |
| 2 | Выбор стратегии/высадки | закрыто (A.2) | — |
| 3 | Производство | закрыто (A.3), кроме резерва | эвристика заменена на `PlanNextBuildJob`/`UpdateMiningNeeds`/`CheckPower`/`CheckWorkers`/склады `EndTurn`; производство юнитов по весам; нет `RollTeamMissionSupplies` (стартовая загрузка) и `TaskAttackReserve` (A.4) |
| 4 | Бой | частично (A.4) | наступление, резерв, ценз целей, `EvaluateAttackReadiness` и `AiAttack_GetTargetFlags` портированы; нет `TransporterMap`/марша, слияния атак и `TaskSupportAttack`; см. ниже |
| 5 | Разведка | закрыто (A.6) | `TaskExplore` целенаправленно исследует карту (`TaskSearchDestination`), запрашивает разведчиков по типам |
| 6 | Флот | закрыто (A.6) | `AttemptTransport`/`TaskTransport`: переправа наземных юнитов морским транспортом; морская разведка (`FASTBOAT`/`SUBMARNE`) и `SEATRANS` запрашиваются |
| 7 | Исследования/апгрейды | закрыто (A.5) | `DetermineResearchProjects`, `ChooseUpgrade`, `UpgradeUnitType`, `ChooseInitialUpgrades` портированы |
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
5. **Исследования/апгрейды** (`DetermineResearchProjects`, `ChooseUpgrade`). —
   **закрыто в A.5** (см. ниже).
6. **Разведка и флот** (`TaskExplore`, `AttemptTransport`, морские задачи). —
   **закрыто в A.6** (см. ниже).

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


## Фаза A.4 — бой: резерв и наступление (в работе)

Портировано (модули `ai/attack.rs`, `ai/tasks/combat.rs`,
`ai/strategy.rs`, `ai/tasks/mod.rs`):

- **`TaskAttackReserve`**: пул `units`, `IsUnitUsable`, `AddUnit`
  (накопление и передача в ближайшую подходящую задачу — `Attack`/`KillUnit`),
  `Execute` (`AiAttack_EvaluateAssault` → `evaluate_attack` + `ChaseAttacker`),
  `EndTurn` (reload/repair/upgrade), `RemoveSelf`/`RemoveUnit`; задача создаётся
  в `AiPlayer::init_tasks` вместе с менеджером и оборонным резервом
  (`aiplayer.cpp:2183-2187`); резерв принимает только вооружённые мобильные
  юниты. Фабричная/строительная ветка `AddUnit`
  (`taskattackreserve.cpp:136-228`) оставлена в `TaskManageBuildings`
  (`plan_unit_production`, A.3), чтобы не дублировать производство.
- **`TaskAttack`**: `AssessEnemyUnits` (релевантность целей, добавление
  защитников через `IsDefenderDangerous`), `ChooseFirstTarget` (с ротацией
  у зоны атаки), `CopyTargets`/`RemoveTask`-очистка, `Init`/`GetCautionLevel`.
- **`EvaluateAttackReadiness`** и составные части: ценз вторичных целей
  (`GetRequiredDamage`/`GetProjectedDamage`), `IsReconUnitAvailable`,
  `IsAnyTargetInRange`, `IsAttackUnderControl`, `IsTargetGroupInSight`,
  `GetSafeDistances`, `UpdateReconUnit`/`FindReconUnit`/`RequestReconUnit`,
  `DetermineLeader`/`MoveUnits`, переходы
  WAIT → GATHER_FORCES → ADVANCE → NORMAL_SEARCH → ATTACK и обратно.
- **`TaskKillUnit`**: `UpdateDemand` (`required_damage`, `hits` с запасом 5/4,
  огонь защитников через `GetDefendersFire`), `GetProjectedDamage(attacker,
  target)` = `AiAttack_GetAttackPotential * ATTRIB_ROUNDS` + бонус за
  неподвижную/непоражаемую цель, `unit_damages` фиксируется при входе юнита,
  `GetNewUnits` (ближайшие подходящие юниты + запросы `TaskObtainUnits` по
  весам), `GetPriority` = база + `AiAttack_GetTargetFlags(nullptr, target,
  team)`.
- **`AiAttack_GetTargetFlags`** получил `Option<&UnitInfo>` (C++ `nullptr` для
  приоритета) и подключён в `TaskKillUnit::GetPriority`.
- **Наведение/штурм**: подход к цели идёт через
  `AiAttack_ChooseSiteForAttacker` (TaskAttack::MoveCombatUnit), а не шагом на
  клетку цели — иначе pathfinder не находил путь и отряд стоял.
- Исправлены два препятствия, без которых наступление не запускалось:
  `AiPlayer::DetermineAttack` читал замеченные юниты из `world.ai.players`,
  хотя на время `BeginTurn` игрок оттуда изъят (теперь из `self`);
  `TaskWaitToAttack` получил проверку дубликатов и условия
  `CheckReactions` (`taskwaittoattack.cpp:41-82`) — без этого штурм порождал
  тысячи задач ожидания.

Осознанные отклонения: нет `TransporterMap` (марш/перевозка),
`AiAttack_EvaluateAssault` целиком и слияния атак (`CopyTargets`);
`attack_zone_reached` определяется как «лидер в пределах дальности стрельбы»,
а не прибытие на клетку атаки; `TaskSupportAttack` при >5 вторичных целях не
создаётся; `GetTotalProjectedDamage` в `IsAttackUnderControl` использует
урон, зафиксированный при входе юнитов.

Контрольный сценарий (20 ходов, карта 24×24, красная база + 3 танка + скаут,
у зелёного здания 500 hits; `ai_init` + `ai_setup_strategy`):

| Показатель | До A.4 | После A.4 |
|---|---|---|
| `state_checksum` (пакет 45) | `0x0C63` | `0x0C63` |
| `replay_digest_crc` | `0x4592` | `0x47FC` |
| CRC снапшота (`crc16`) | `0x9971` | `0x63CC` |
| Задач ИИ в конце | 24 | 129 |
| Создано `TaskAttack` | 0 | 1 |

Причина: до A.4 `TaskAttack` в этом сценарии не создавался вовсе
(изъятый слот игрока в `DetermineAttack`), теперь создаётся, ведёт ценз целей
и запрашивает юнитов по весам — меняются состав задач и RNG-путь (броски
`RollUnitType` в запросах). `state_checksum` не изменился: пакет 45 несёт
состояние юнитов, а не задачи ИИ; `replay_digest`/снапшот включают AI-состояние.
`p7` (`mini_replay_matches_cpp_digest_bit_for_bit`) не изменился. Тесты:
279 unit + интеграционные, все зелёные.

## Фаза A.5 — исследования и апгрейды юнитов (закрыто)

Портировано (новый модуль `ai/research.rs`, подключение в `ai/strategy.rs`):

- **`DetermineResearchProjects`**: тема выбирается по минимальному
  `turns_to_complete` команды с поправками C++ — `RANGE` делится на 2,
  `SHOTS` умножается на `5 - (research_level % 5)`, `SPEED` — на 2; начальная
  тема — `ATTACK` (без поправок), `COST` не рассматривается. Каждый
  включённый исследовательский центр (`RESEARCH`, `ORDER_POWER_ON`, состояние
  не `ORDER_STATE_INIT`) переводится на выбранную тему через
  `ResearchMenu_UpdateResearchProgress(-1/+1)`: обновляются `allocation` и
  `turns_to_complete`. Ставки — константы `RESEARCH_TOPIC_FACTORS` (как и в
  остальном `max_core`).
- **`ChooseUpgrade`**: по стратегическим заказам A.3 выбирается самый дешёвый
  апгрейд (`TeamUnits_GetUpgradeCost`); generic-заказ заменяет выбор только
  если он вдвое дешевле, а после первой замены — строго дешевле текущего.
  Апгрейды `HITS`/`SPEED` пропускаются при текущем значении `UINT8_MAX`
  (узкая проверка C++); `upgrade_cost` — `INT16_MAX`, если ничего не выбрано.
- **`UpgradeUnitType`**: шаг — `TeamUnits_UpgradeOffsetFactor` по **базовому**
  значению атрибута (1/2/5/10); `HITS`/`SPEED` обрезаются до `UINT16_MAX`;
  копия текущих `UnitValues` получает `UpdateVersion`, `MarkAsNotInUse` и
  `AddAttribute`, затем заменяет `current_values` команды.
- **`ChooseInitialUpgrades`**: стартовая закупка (`ChooseStrategicBuildOrders(true)`
  + пустой generic-список, золото из `TeamMissionSupplies::team_gold`) — для
  будущего `RollTeamMissionSupplies`.
- **Жизненный цикл**: `AiPlayer::BeginTurn` строит `strategic_build_orders` и
  `generic_build_orders`, вызывает `ChooseUpgrade` и цикл покупки
  (`upgrade_cost <= team_gold`: списание золота, `stats_gold_spent_on_upgrades`,
  `UpgradeUnitType`, повторный выбор), затем `RegisterIdleUnits` и
  `DetermineResearchProjects` — порядок `aiplayer.cpp:2503-2525`. Сетевые
  пакеты 09/10 остаются за транспортным слоем (как и в остальном порте).

Контрольные сценарии (20 ходов; «research-only» — один `BeginTurn` без
экономики, чтобы центр оставался включённым):

| Сценарий | `state_checksum` до → после | `replay_digest_crc` до → после | CRC снапшота до → после |
|---|---|---|---|
| research-only (центр `SPEED` → `SHOTS`) | `0x2670` → `0x2670` | `0x50BA` → `0x081F` | `0x4C7F` → `0x8F34` |
| research + upgrades (центр + танк в стройке, 500 золота) | `0xDDD4` → `0xDDD4` | `0x139B` → `0x74D9` | `0xF63F` → `0xE837` |
| шахта + конструктор (контроль) | `0x8F32` → `0x8F32` | `0x68D2` → `0x68D2` | `0x6D79` → `0x6D79` |
| армия + замеченный враг (контроль) | `0xD2F1` → `0xD2F1` | `0x002C` → `0x002C` | `0x94D2` → `0x94D2` |

Причина: `state_checksum` (пакет 45) несёт только поля юнитов и не меняется —
апгрейды живут в текущих значениях команды, исследования — в темах команды;
`replay_digest`/снапшот включают золото команды,
`stats_gold_spent_on_upgrades`, темы исследований и `research_topic` юнитов.
В сценарии «research + upgrades» за 20 ходов потрачено 474 золота, 26
осталось; тема центра в нём не меняется (сценарий проверяет вклад апгрейдов в
CRC), переназначение темы покрыто сценарием «research-only» и интеграционным
тестом `begin_turn_moves_powered_research_centres_to_the_fastest_topic`.
`p7` (`mini_replay_matches_cpp_digest_bit_for_bit`) не изменился: в replay нет
ходов ИИ. Тесты: 289 unit + интеграционные, все зелёные (10 unit и 3
интеграционных — новые).

## Фаза A.6 — разведка и флот: `TaskExplore`/`TaskTransport` (закрыто)

Портировано (модули `ai/tasks/search.rs`, `ai/tasks/move_tasks.rs`,
`ai/tasks/mod.rs`, подключение в `ai/strategy.rs`; последняя фаза полного порта
ИИ).

### `TaskExplore` и `TaskSearchDestination`

- **`TaskAbstractSearch::EndTurn`** (`taskabstractsearch.cpp:79`) — общий
  жизненный цикл: готовым юнитам ставится `RemindMoveFinished`, повреждённые
  (`hits <= base_hits / 2`, противник `APPRENTICE`+) уходят в `TaskRepair`
  (`RemoveTasks` + `TaskRepair`), а при пустом списке юнитов и нулевом
  `requestors` создаётся `TaskObtainUnits` (`requestors++`).
  `BeginTurn` — это `EndTurn()`; `ChildComplete` уменьшает `requestors`.
- **`TaskExplore`**: флаги запросов — `SCOUT`; `AWAC`, `FASTBOAT` (Rust
  `Escort`), `SUBMARNE` при доступности постройки; `SUBMARNE` — с
  `OPPONENT_TYPE_AVERAGE`, `COMMANDO` (Rust `Infiltrator`) — с
  `OPPONENT_TYPE_EXPERT` (`taskexplore.cpp:36-46`). `IsUnitUsable` берёт только
  запрошенные типы, `SCOUT`/`FASTBOAT` — до трёх, остальные по одному.
  `ObtainUnit` выставляет запросы в порядке C++ (`SCOUT`, `AWAC`, `FASTBOAT`,
  `SUBMARNE`, `COMMANDO`).
- **Поиск цели** (`TaskSearchDestination`) выполняется синхронно в `Execute`:
  кольца Чебышёва вокруг юнита, первый подходящий сайт, затем продолжение по
  направлению от `point` задачи на `radius` шагов (`ATTRIB_SCAN` для
  разведки, `1` для `TaskSurvey`/`TaskFindMines`). Сайт фильтруется по
  `IsVisited` подкласса, доступности
  (`AccessModifier_EnemySameClassBlocks`) и карте урона
  (`damage_potential_map < 1`). Найденный сайт отдаётся `TaskMove`.
- **`TaskExplore::IsVisited`** (`taskexplore.cpp:108`): вода/земля по
  `SurfaceType` (для `FASTBOAT`/`SUBMARNE` — только вода, для остальных —
  только земля) и `INFO_MAP_EXPLORED` в `AiPlayer::info_map`; юнит, которому
  некуда идти, освобождается (`OnUnitReleased`: снятие флага типа +
  `ClearUnitTasksAndRemindAvailable`).
- **`TaskSurvey`/`TaskFindMines`**: та же механика `FindDestination`, свои
  `IsVisited` — бит команды в `CargoMap` и `AiPlayer::mine_map`
  (`tasksurvey.cpp:131`, `taskfindmines.cpp:92`); `TaskFindMines::BeginTurn`
  оценивает число «ценных» клеток с минами (карта урона) и просит
  дополнительных сапёров.

### `AttemptTransport` и `TaskTransport`

- **`AttemptTransport`** (`taskmove.cpp:370`): наземный юнит, которому
  `find_ground_path` не даёт маршрута, делит путь по воде. `transport_route`
  идёт по прямой от юнита к цели, находит последнюю проходимую клетку перед
  водой (`transport_waypoint` — посадка) и первую проходимую за ней
  (`passenger_destination` — высадка). Транспорт выбирается как в C++: `APC`
  (`CLNTRANS`) для пехоты/коммандос, если он есть у команды и клетка посадки
  не под наблюдением врага; иначе `SEATRANS`.
- **`TaskMove`**: после посадки юнит идёт к точке посадки и ждёт; признак
  «в трюме» — `ORDER_IDLE` + `parent_unit` транспорта (события
  `EventUnitLoaded/Unloaded` в порте нет). После высадки `TaskMove`
  продолжает путь к исходной цели.
- **`TaskTransport`** (`tasktransport.cpp`): `Init` создаёт
  `TaskObtainUnits` на тип транспорта (это и есть заказ `SEATRANS`/`APC`),
  `AddUnit` принимает транспорт, `ChooseNewTask` выбирает ближайшего клиента,
  `Execute` ведёт транспорт к воде у посадки, грузит пассажира
  (`ORDER_MOVE_TO_UNIT` + одношаговый путь, как `LoadUnit`) и к воде у
  высадки, выгружает (`ORDER_ACTIVATE` + `move_to`, как
  `TaskTransport_FinishTransport`), после чего освобождает транспорт и
  удаляет задачу. `IsNeeded` — «нет транспорта, но есть клиенты».
- **Флот**: `TaskExplore` запрашивает `FASTBOAT`/`SUBMARNE` (морская
  разведка), `TaskTransport` — `SEATRANS`; строительство идёт через
  `TaskCreateBuilding` (`SHIPYARD` — из `PlanNextBuildJob` для `SEA`), наём
  боевых кораблей — через весовые таблицы (`GetFilteredWeightTable`,
  `TaskAttackReserve`, `TaskKillUnit`), морская разведка атак — `FASTBOAT`
  (`UpdateReconUnit`).

Осознанные отклонения:

- кольцевой обход вместо спиральных курсоров `TaskSearchDestination` (набор
  кандидатов и порядок по радиусу те же);
- нет `TransporterMap`: точка остановки транспорта — ближайшая проходимая для
  него клетка у посадки/высадки, а не клетка, найденная `TransporterMap`;
- `AIRTRANS` не используется потоком переправы (в C++ он предпочтительнее,
  если построен);
- при занятой клетке высадки подбирается соседняя свободная (в C++ —
  `AiPlayer::ClearZone`);
- `FASTBOAT` добавлен в запросы `TaskExplore`: в C++ конструктор его читает в
  `ObtainUnit`, но никогда не выставляет (`taskexplore.cpp:35-45`) — здесь
  реализовано намерение, иначе морская разведка не запрашивается.

### Проверка

| Сценарий (20 ходов) | `state_checksum` до → после | `replay_digest_crc` до → после | CRC снапшота до → после |
|---|---|---|---|
| «море»: канал, танк + `SEATRANS`, цель за водой | `0x9D30` → `0x0101` | `0x4E4E` → `0x5D84` | `0x925F` → `0xCDE8` |
| «земля»: конструктор + скаут, 24×24 | `0x2684` → `0x45C9` | `0x6D92` → `0x0815` | `0x0798` → `0xA9D1` |

Причина: `TaskExplore` больше не удаляет себя при пустом списке юнитов, а
запрашивает разведчиков (`SCOUT`/`AWAC`/`FASTBOAT`/`SUBMARNE`), поэтому
меняются состав задач (62 → 85 и 36 → 40 в сценариях), стройка (заводы под
разведчиков) и путь решений ИИ; в «земля»-сценарии скаут уходит с (11,9) на
(3,2). `p7` (`mini_replay_matches_cpp_digest_bit_for_bit`) не изменился: в
replay нет ходов ИИ. Тесты: 294 unit (5 новых — `transport_route` и выбор
транспорта) + интеграционные, все зелёные (2 новых в `ai_a6.rs`: разведка и
переправа). Два утверждения `ai_p6.rs` расширены под поведение порта:
строитель может стоять на площадке (без `TaskMove`) и `AwaitTapePositioning`
считается фазой стройки.

## Фаза 5 — интеграционные сценарии ИИ (AI CRC)

Закрепление порта A сквозными сценариями (`max-brh-qa/plans/engine-coverage-plan.md`
§3, фаза 5): `max_core/tests/phase5_ai_*.rs` и общий харнесс
`max_core/tests/support/phase5.rs`. Каждый сценарий: база ИИ (RED,
`TEAM_TYPE_COMPUTER`) + пассивная база противника (GREEN, `TEAM_TYPE_PLAYER`) →
N ходов через машинку ходов ядра (`begin_round` → `next_team` → `end_round`;
`next_team` сам выполняет `GameManager_ManageEconomy` и `Ai_BeginTurn`, поэтому
`ai_begin_turn` повторно не вызывается) → ожидаемые задачи/юниты/CRC. Задачи,
живущие внутри хода (`TaskAttack`, `TaskKillUnit`, `TaskMove`, резервы),
сэмплируются на каждом тике (`AiObservation`); устойчивое состояние (юниты,
золото, темы исследований, digest'ы) читается на границе хода.

### Сценарии по областям ИИ

| Файл | Область | Что доказывает |
|---|---|---|
| `phase5_ai_strategy.rs` | A.2, A.1 | маппинг клан→стратегия в `init_tasks` (все 8 кланов); force-стратегия (`menu_team_strategy_setting`) применяется только при ненулевом score, иначе — взвешенный бросок; `SelectStrategy` расходует RNG (тот же seed → та же стратегия и тот же хвост RNG); все 18 вес-таблиц заполнены, `UpdateWeightTables` умножает веса ровно своих типов во всех таблицах (`RebuildWeightTables`, факторы 10/5/3/2); `GetFilteredWeightTable(0x01)` убирает `STATIONARY`; стратегия и таблицы входят в снапшот |
| `phase5_ai_production.rs` | A.3 | TankHorde с LIGHTPLT планирует `TaskCreateBuilding(HeavyVehiclePlant)` и не спешит с AIRPLT/SHIPYARD/TRAINHAL; голая база за 14 ходов отдаёт `ORDER_BUILD` строителю и достраивает MININGST; `plan_unit_production` заказывает TANK с LANDPLT по весам (`GetWeightTable(INVALID_ID)` + таблица замеченного танка) и молчит при пустом складе комплекса |
| `phase5_ai_combat.rs` | A.4 | замеченный ключевой объект → `TaskAttack` + `TaskKillUnit` + `TaskAttackReserve`, `TaskDefenseReserve` накапливает армию (3 танка), объект получает урон; без засветки атак нет и урона нет; `TaskAttack::target_team` — команда противника |
| `phase5_ai_research.rs` | A.5 | `ChooseUpgrade` + цикл покупки тратят золото команды (`gold` падает, `stats_gold_spent_on_upgrades` растёт, ATTACK танка в `current_values` растёт); без золота не покупается ничего; `DetermineResearchProjects` переводит включённый центр на быстрейшую тему (RANGE) и ведёт `allocation`; выключенный центр тему не меняет |
| `phase5_ai_recon.rs` | A.6 | `TaskExplore` создаётся, запрашивает SCOUT/AWAC/FASTBOAT/SUBMARNE, отдаёт `TaskMove`, разведчик уходит с места и увеличивает `info_map`; доступный разведчик принимается задачей |
| `phase5_ai_determinism.rs` | D | каждый сценарий прогоняется дважды: `snapshot_bytes`, `state_checksum`, `replay_digest_crc`, `state_digest` совпадают и равны закреплённым значениям; прогон продолжается с `clone` бит-в-бит; шесть сценариев дают разные digest'ы |

### Детерминизм (D)

Для одного seed прогон **бит-в-бит**: два прогона сценария дают одинаковый
`snapshot_bytes` (включая задачи, напоминания и карты ИИ), `state_checksum`
(пакет 45), `replay_digest_crc` и `state_digest`. Прогон, продолженный с `clone`,
совпадает с непрерывным. Вопрос владельца о «той же стратегии, другом
seed-пути» остаётся открытым: тесты фиксируют только равенство для одного seed.

### AI CRC сценариев

| Сценарий | Ходы | `state_checksum` | `replay_digest_crc` | `state_digest` (снапшот) |
|---|---|---|---|---|
| `strategy` (A.2: клан AyersHand → MISSILES) | 6 | `0x4C90` | `0xD94E` | `0xB2DB` |
| `production` (A.3: голая база, стройка) | 12 | `0x7682` | `0x0404` | `0x4901` |
| `factory` (A.1/A.3: замеченный танк → LANDPLT заказывает TANK) | 4 | `0xEDDD` | `0x6357` | `0x4DD1` |
| `combat` (A.4: замеченный TRAINHAL, штурм) | 6 | `0x5D58` | `0xC2D6` | `0xD081` |
| `research` (A.5: апгрейды + включённый центр) | 4 | `0xCC41` | `0x6407` | `0x38C5` |
| `recon` (A.6: разведчик) | 8 | `0x122B` | `0x54BB` | `0xF5E7` |

При изменении ИИ таблицу обновлять здесь; значения закреплены константами
`*_CRCS` в `phase5_ai_determinism.rs`.

**2026-11-26, перепин темпа анимации взрывов (дефект 1 ручного прогона):** `progress_explosion` (unit_order.rs) теперь листает кадр раз в **пять** шагов симуляции вместо двух (EXPLOSION_FRAME_STEPS), как в оракуле `UnitsManager_ProgressExplosion` (units_manager.cpp:4030-4036) при тике 42 мс (game_manager.cpp:5103, 	imer.h:27) и шаге порта 60 Гц. Следствия: эффекты (HITEXPLD/LNDEXPLD/…, частицы `RKTSMOKE`/`TRPBUBLE`) живут дольше в шагах, поэтому `ORDER_STATE_ATTACK_PENDING` дольше ждёт окончания анимации попадания — в `factory` и `combat` сдвигаются все три дайджеста (`state_checksum` тоже: `moved`/`image_index` входят в снапшот). Валлидность по `max_core/tests/phase2_frames.rs` (13 кадров `LNDEXPD` = 65 шагов).

**2026-11-25, перепин видимости (дефект sentry-реакции):** `update_map_status_unit`
(`ai/threat.rs`) теперь выполняет пропущенную часть `Access_UpdateMapStatus`
(`access.cpp:762`) — проход видимости самого юнита: `Access_DrawUnit`
(`access.cpp:691`) со `UnitInfo::Draw` (`unitinfo.cpp:2842`) для клетки вне скана.
Юнит, оказавшийся в уже просканированной клетке (перемещение,
высадка), в C++ засвечивается сразу; без этого прохода `visible_to_team`
оставался `false`, и `UnitsManager_ShouldAttack` (`units_manager.cpp:4623`)
отклонял цель — sentry не реагировал. Следствия: `factory` и `combat`
переснимают `state_digest` (враг попадает в `Ai_UnitSpotted` на шаге развёртывания,
а не на явном `Access_UpdateVisibilityStatus`), `state_checksum` и
`replay_digest_crc` не изменились. Регресс-тест:
`max_core/tests/sentry_reaction.rs`.

**2026-11-24, перепин экономики (дефект 3 ручного прогона):** экономика команды
перенесена на **начало её хода** в порядке оракула
`GameManager_ManageEconomy` → `GameManager_ProgressBuildState`
(`game_manager.cpp:2058`, `:8183`): сначала начисляется добыча/распределяется
запас комплекса и списывается сырьё мобильных строителей, затем завод платит из
**уже начисленного** запаса и продвигает/завершает стройку. Раньше
`progress_build_state` для всех команд шёл в `finish_turn` (закрытие раунда) —
**до** `manage_economy` следующего хода, поэтому нехватка определялась по
прошлому запасу. Дополнительно `GameManager_RefreshOrders` (начисление добычи +
проверка нехватки) теперь выполняется при применении пакета 06 — в момент
окончания хода. Плюс порт **Path-постройки** инженера (`GameManager_PathBuild` →
`UnitsManager_BuildNext`): цепочка построек идёт по клеткам маршрута, что меняет
число юнитов в сценариях ИИ. Отсюда перепин `state_checksum`/digest'ов;
`research` не изменился (в его сценарии экономика не доходит до порога).

**2026-11-24, перепин `research`:** порт `UnitsManager_SetInitialMining` перенесён
в `GameState::spawn_unit` (`units_manager.cpp:2452-2454`): теперь **любая**
развёрнутая шахта — в том числе построенная в партии — получает стартовую
аллокацию и `POWER_ON`, а не остаётся с `0/0/0` (дефект 2 ручного прогона 1.2).
В сценарии `research` шахта исследовательского комплекса впервые начинает давать
топливо с первого хода, поэтому `replay_digest_crc` и `state_digest` изменились
(`0x0CA2 → 0x6407`, `0xADC0 → 0x38C5`). `state_checksum` (пакет 45) не зависит
от счётчиков добычи и остался `0xCC41`. Фикстура `add_research_lab` теперь
пишет `raw/fuel/gold/total_mining` согласованно (10 топлива, сумма 10).

**2026-11-24, перепин `production` — починка стройки ИИ (БЛОКЕР ручного
прогона):** см. раздел «Починка стройки ИИ» ниже. `state_checksum` (пакет 45) и
`replay_digest_crc` не изменились — набор юнитов в сценарии тот же; `state_digest`
изменился (`0x2804 → 0x4901`), потому что снапшот несёт список задач ИИ: база
теперь регистрирует свои здания в `TaskManageBuildings::units` и планирует
LIGHTPLT через `PlanNextBuildJob`.

## Починка стройки ИИ (2026-11-24)

Ручной прогон: «ИИ не строит ничего (сломаны все стратегии)» и «построил радар
поверх завода лёгких машин». Диагностика — сценарий 20–30 ходов (база ИИ +
пассивная база противника, `support/phase5.rs`) с печатью `TaskManageBuildings`
и планов `TaskCreateBuilding` на каждой границе хода.

### Что именно было сломано

1. **Здание не доходило до менеджера базы.** `TaskManageBuildings::units` —
   единственный вход стратегической ветки `PlanNextBuildJob`
   (`taskmanagebuildings.cpp:1011`: при пустом списке функция сразу возвращает
   `false`). Список наполняется только `AiPlayer::AddBuilding` в `need_init`
   (`aiplayer.cpp:2343-2348`) и передачей из `TaskCreateBuilding::Finish` —
   `AddUnit(*building)` (`taskcreatebuilding.cpp:326-334`). В порте второго
   вызова не было: `create_building_finish` чистил задачу и удалял себя.
   Итог: ИИ доводил до конца первую шахту и **больше не планировал ничего** —
   ни LIGHTPLT, ни LANDPLT, ни AIRPLT/SHIPYARD/TRAINHAL, ни HABITAT/GREENHSE/
   RESEARCH; «сломаны все стратегии» = недостижима общая ветка после этого
   стража.
2. **`CheckMaterials` считал не то.** Оракул требует, чтобы **собственный груз
   строителя** покрывал всю постройку: `BuildMenu_GetTurnsToBuild(unit_type) *
   Cargo_GetRawConsumptionRate(builder, 1)`, плюс ветку резерва
   `TaskManager_NeedToReserveRawMaterials` (`taskcreatebuilding.cpp:1033-1086`).
   Порт сравнивал **сумму raw комплексов** с фиксированными 10. Пока комплекс
   пуст (нет склада, шахта без питания) — строитель навсегда застревал в
   `CREATE_BUILDING_STATE_GETTING_MATERIALS`, оставаясь привязанным к задаче:
   другие постройки не могли получить строителя, и база вставала. Оракул в этом
   состоянии отправляет строителя за сырьём (`TaskGetMaterials`,
   `:1070-1078`) — в порте этого вызова не было.
   Родственная ошибка: `need_to_reserve_raw_materials` использовал собственную
   таблицу «потребления» (шахта «потребляет» 2), а не
   `Cargo_GetRawConsumptionRate(type, 1)` (`ai.cpp:434`).
3. **Площадка стройки не проверяла занятость.** `find_build_site` в
   `ai/tasks/create.rs` проверял только проходимость (`is_valid_building_site`),
   но не хеш клеток: клетка с юнитом возвращалась как свободная. Вторая ошибка —
   два плана одного хода могли выбрать одни и те же клетки. Оракул для этого
   строит карту стройки: `ClearBuildingAreas` (стационарные здания) +
   `ClearPlannedBuildings` (футпринты других `TaskCreateBuilding`,
   `taskmanagebuildings.cpp:261-374`). Итог ручного прогона: радар планировался
   на клетку будущего LIGHTPLT и после его постройки вставал сверху; порядок
   проверок на старте стройки занятую клетку тоже не отсекал.

### Когда сломалось

Регрессии от `3ab9aec` (тайминг ресурсов) и `bef1592` (резерв энергии) **нет**:
на родителе `3ab9aec` (`02939f1`) сценарий ведёт себя так же — ИИ строит первую
шахту и встаёт (проверено прогоном того же сценария на обеих ревизиях). Обе
причины живут в порте ИИ с начального порта: `check_materials`-упрощение и
отсутствие передачи здания появились до переименования `max-rust → max-brh`
(`bb3c0cd`). Правки экономики лишь сдвинули тайминг: на `02939f1` шахта
достраивается на 7-м ходу, на `6aee166` — на 8-м (завод платит из уже
начисленного запаса), поэтому CRC ИИ и перепинивался — это законное изменение,
а не поломка стратегий. Уровень сложности на блокер не влияет: на
`Apprentice`/`Average`/`Expert` картина одинаковая (после починки все 9
стратегий строят LIGHTPLT, часть — LANDPLT/HABITAT).

### Исправления (файл:строка)

| Что | Оракул | Порт |
|---|---|---|
| передача готового здания менеджеру | `taskcreatebuilding.cpp:326-334` (`Finish` → `AddUnit`) | `ai/tasks/create.rs::create_building_finish` |
| `CheckMaterials` по грузу строителя + ветка резерва | `taskcreatebuilding.cpp:1033-1086` | `ai/tasks/create.rs::check_materials` |
| ставка потребления raw | `cargo.cpp:140` (`Cargo_GetRawConsumptionRate`) | `ai/tasks/create.rs::need_to_reserve_raw_materials` |
| запрос сырья строителю | `taskcreatebuilding.cpp:1070-1078` (`TaskGetMaterials`) | `ai/tasks/create.rs::create_building_request_materials` |
| доставка сырья со склада/грузовика | `taskgetresource.cpp:24-135`, `taskgetmaterials.cpp:99-211` | `ai/tasks/misc.rs::get_materials_end_turn` / `do_transfer` / `find_source_building` / `find_truck` |
| площадка: занятые клетки + планы базы | `taskmanagebuildings.cpp:261-374` (`ClearBuildingAreas`/`ClearPlannedBuildings`) | `production.rs::find_build_site_excluding` / `site_is_free_except` / `construction_utilities_at`, `ai/tasks/create.rs::find_build_site_for_task` / `planned_build_footprints` |
| проверка площадки на старте стройки | `taskcreatebuilding.cpp:822-845` (пере-поиск площадки) | `ai/tasks/create.rs::create_building_begin` |

Тесты: `max_core/tests/ai_build_chain.rs` (4 сценария: регистрация зданий и
LIGHTPLT; доставка сырья со склада; отсутствие пересечений планов и построек;
стройка при пустом комплексе). Все четыре падают на ревизии до правок и
проходят после.

### Отладка: `--no-fow`

Наблюдение за ИИ «в покое» — режим без тумана войны: флаг `--no-fow` (UI) или
строка `no_fow=1` в `preferences.ini`. Это **только представление**: рисуются
все юниты (`render`, `overlays`), миникарта без затемнения неисследованных
клеток (`panels::draw_minimap`), точки юнитов на миникарте — все. Состояние
симуляции и CRC не меняются (в отличие от `AiState::all_visible`, который
входит в дайджест).

### Наблюдения, зафиксированные сценариями (не правки порта)

1. `Access_UpdateMapStatus` не сканирует `ORDER_IDLE` (`access.cpp:762`), поэтому
   сценарии ставят наблюдателя в `ORDER_SENTRY` и пересчитывают обзор
   `Access_UpdateVisibilityStatus` — как после высадки. Заспавненный без приказа
   юнит не даёт засветки; это поведение C++, а не дефект порта.
2. `plan_unit_production` катит тип из составной таблицы
   `GetFilteredWeightTable(INVALID_ID, 3)` (air defense + bomber + submarine +
   таблицы замеченных типов): без замеченного врага у тяжёлого завода нет
   кандидатов на постройку, и производство юнитов не стартует. Сценарий `factory`
   фиксирует это как предусловие (замеченный танк → таблица `tanks` → TANK ×10
   для TankHorde).
3. `ProductionManager_OptimizePowerProduction` включает POWERSTN по формуле
   `(demand + 6 - 3) / 6`, т.е. при спросе меньше трёх станция не включается;
   одному исследовательскому центру нужны генераторы (в сценарии — три), иначе
   экономика выключает центр, и `DetermineResearchProjects` его пропускает
   (негативный контроль в `phase5_ai_research.rs`).
4. `GameManager_ManageEconomy` пересчитывает склад комплекса из его состава
   (`Access_UpdateResourcesTotal`), поэтому ручная запись `Complex::raw/fuel`
   перед ходом бесполезна: сценарии ставят в комплекс реальные склады
   (ADUMP/FDUMP) либо руду/топливо под шахту.

## Открытые вопросы владельца

- Требование к детерминизму бит-в-бит ИИ (п. 8) — цель или допустимо «та же
  стратегия, но другой seed-путь» до полного порта?
- Порядок фаз A: рекомендуемый — по списку выше (вес-таблицы → стратегия →
  производство → бой → исследования → разведка/флот).
