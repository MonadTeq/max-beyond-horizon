# max_core — дизайн крейта детерминированной симуляции M.A.X. (PLAN v6.0)

> Статус: **дизайн для переписывания ядра на Rust**.
> Оракул поведения: `max-cpp-port/src/` (неизменяемый, C++).
> Спецификация состояния: `docs/state-model.md` (15 перечислений, 11 структур, V70).
> Сетевой контракт: `docs/protocol.md` (lockstep, пакеты 00–54).
> RE-каталог оригинала: `max-original-dasm/functions-named.csv` (имена исходных файлов игры: `task*.cpp`, `paths_manager.cpp`, `winloss.cpp`, …).
> Каркас для рефакторинга: `max-brh/crates/max_core/src/` (`lib.rs`, `map.rs`, `team.rs`, `unit.rs`, `order.rs`).

---

## 1. Цели и границы крейта

### 1.1 Цель

`max_core` — **чистая детерминированная модель M.A.X.**, исполняющая полную lockstep-симуляцию так, как её исполняет C++-порт (а значит — и оригинальная игра): карта и ресурсы, юниты, команды, приказы, ходы, бой, экономика, исследования, ИИ-задачи, условия победы/поражения, RNG бит-в-бит.

Функция крейта:

```
вход:  настройки партии (seed, правила) + поток команд/событий (пакеты 08/38/41/…, барьеры)
       + дискретные "тики" от сетевого слоя
выход: состояние партии (GameState) + события для UI/сети + CRC-дайджесты для десинк-проверок
```

### 1.2 Входит в крейт

| Область | Содержимое |
|---|---|
| Типы и константы | все перечисления/структуры state-model.md, таблицы атрибутов юнитов (93×14), кланы и их модификаторы |
| Карта | `SurfaceMap` 112×112, `ResourceMap` (количество/тип/видимость), генерация ресурсов, континенты, alien-здания |
| Юниты | `UnitInfo` (симуляционный набор полей), 5 списков-слоёв, хэш id→юнит и клетка→юниты, спавн/уничтожение/движение |
| Команды | `TeamInfo`/`CTInfo`, `TeamUnits` (базовые/текущие атрибуты, комплексы), экономика, исследования |
| Приказы | машина `UnitOrder` (32 типа) × `UnitOrderState` (47 состояний), `UnitsManager_ProcessOrders` |
| Ход | игровой цикл тика, конец хода, производство, снабжение, скоринг |
| Бой | расчёт урона, реактивный огонь, стелс, pending attacks, live attacks |
| Пути | ground/air/builder-поиск путей, заполнение, поля расстояний (все — синхронно, однопоточно) |
| ИИ | `TaskManager`, все `task*.cpp`, стратегии `AiPlayer`, вес-таблицы, threat/heat maps (симуляционная часть) |
| Победа/поражение | `winloss.cpp`, generic-условия и закодированные миссионные предикаты |
| Сериализация | V70 `GameState` ⇄ байты (`&[u8]`/`Vec<u8>`) — **без файлового I/O**; нужно для фикстур-тестов и реплеев; канонический байтовый дайджест состояния для CRC-паритета |

### 1.3 НЕ входит в крейт

| Область | Куда |
|---|---|
| Сеть (ENet/UDP, пакеты, барьеры, таймауты, реконнект) | `max_net` — он только доставляет **команды и тики** в max_core и забирает события |
| Файловое I/O, загрузка `.dta`/`.wrl`/`.res` | крейт-потребитель (Horizon/CLI/тесты); `max_core` работает с `&[u8]` |
| Время (`timer_get`, паузы, таймер хода как wall-clock) | таймеры — это **входные тики**; счётчики таймеров — состояние |
| Рендер, спрайты, анимация, звук | `max_render`/`max_assets`; UI-поля `UnitInfo`/`TeamInfo` (проекция `UI` state-model.md) в симуляцию не входят |
| Меню, лобби, выбор планеты | вне крейта; setup-данные приходят готовыми |
| Lua-скрипты миссий (`scripter.cpp`) | правила миссий — **данные** (см. §7, риск 8) |

### 1.4 Архитектурные принципы (иерархия приоритетов)

1. **Детерминизм бит-в-бит с C++** — важнее идиоматики: порядок обхода структур, wrapping-арифметика и порядок вызовов RNG воспроизводятся 1:1.
2. **Чистые функции без I/O** — запрещены: файлы, сеть, `SystemTime`, потоки, `HashMap`-итерация без упорядоченной обёртки, `f32/f64` в симуляции (единственные float-места C++-порта выносятся в build-time-таблицы, см. риск 1).
3. **Rust-идиоматика там, где это бесплатно**: `enum` вместо magic-чисел (с `#[repr(uN)]` и точными значениями), `Option<UnitId>` вместо нулевых указателей/`NO_ID`, newtype-id (`UnitId(u16)`, `TeamId(u8)`, `ComplexId(i16)`), итераторы по упорядоченным коллекциям, владение вместо `SmartPointer`-refcount.
4. **Ноль новых runtime-зависимостей**: `serde` (как в каркасе) — максимум; RNG собственный; никаких `rand`, `float`, `hashbrown`.
5. **Одна глобальная игра**: состояние — `GameState` (аналог глобалов `UnitsManager_*`/`GameManager_*`), мутирующий через `&mut GameState`; никакой глобальной мутации.

---

## 2. Дерево модулей

Предлагаемая структура `max-brh/crates/max_core/src/` (плоские модули для ядра, `ai/tasks/` — вложенные):

```
lib.rs            — корень: модули, реэкспорт, документация
enums.rs          — все перечисления и бит-флаги (см. §3)
geo.rs            — Point, Rect, ScreenLocation, Direction, DIRECTION_OFFSETS, целочисленная геометрия
rng.rs            — Randomizer: SplitMix64 + Xoshiro128** + Generate<T>
collections.rs    — SmartList<T> (порядок вставки), SmartObjectArray<T>, FixedArray-хелперы
map.rs            — Map, Surface (битмаска), ResourceMapEntry, видимость, тайлы
setup.rs          — генерация карты/ресурсов/континентов/alien-зданий, выбор места высадки
unit_values.rs    — UnitValues, ATTRIB_* индексация, таблицы атрибутов (const-данные)
team.rs           — TeamId, TeamKind, TeamClan, ResearchTopic, TeamInfo, TeamUnits
complex.rs        — Complex, Cargo, подключения, энергия/рабочие
unit.rs           — UnitId, UnitType, UnitFlags, UnitInfo (симуляционное подмножество), Path/PathStep
hash.rs           — UnitHash, MapHash (упорядоченные хэш-таблицы как в C++)
units_manager.rs  — спавн/уничтожение/движение, SetNewOrder, ProcessOrders (фикс. порядок списков)
unit_order.rs     — машина приказов: UnitOrder(32)×UnitOrderState(47), процессоры ORDER_*
combat.rs         — урон, реактивный огонь, стелс, AreaAttack, PendingAttacks, LiveAttacks
pathfinding.rs    — GroundPath/AirPath/BuilderPath, PathFill, TerrainDistanceField, CircumferenceWalker
access.rs         — проходимость (Access/AccessMap), поиск свободных клеток
game.rs           — GameState, tick-цикл, конец хода (ManageEconomy/ScoreGraph), фазы
victory.rs        — WinLoss_EvaluateStatus, generic + миссионные условия, ранжирование
events.rs         — очередь событий/команд, UnitEvents, delayed reactions (сетевая синхронизация)
input.rs          — модель входных команд (пакеты 08/38/41/43/50/20/10/11/12/14/06/45/53…)
save.rs           — V70: GameState ⇄ байты (чистая сериализация, для тестов/реплеев)
crc.rs            — CRC16 пакета 45 + канонический дайджест состояния
ai/mod.rs         — Ai_CheckReactions, реакции, флаги обработки
ai/strategy.rs    — AiPlayer: выбор стратегии, закупки, апгрейды, кланы
ai/weight.rs      — WeightTable (взвешенный выбор через RNG)
ai/threat.rs      — ThreatMap / HeatMap / damage-карты (целочисленные)
ai/defense.rs     — defense_manager
ai/task.rs        — базовый Task, флаги, Reminder
ai/task_manager.rs— TaskManager: очереди reminders (priority→normal→deferred), FindTaskForUnit
ai/tasks/*.rs     — по файлу на каждый класс задачи (49 классов, см. §2.2; плюс база `task.rs`)
```

### 2.1 Покрытие C++-файлов по модулям

| Модуль (Rust) | C++-порт (оракул) | Оригинал (dasm-каталог) | Комментарий |
|---|---|---|---|
| `enums.rs` | `enums.hpp` (все перечисления), `resourcetable.hpp` (`ResourceID`) | `enums.hpp`, `resourcetable.cpp/.hpp` | значения `#[repr]` строго как в C++ |
| `geo.rs` | `point.hpp`, `path.hpp` (`Direction`, `DIRECTION_OFFSETS`) | `path.hpp` | 8 направлений по часовой: N, NE, E, SE, S, SW, W, NW |
| `rng.rs` | `randomizer.cpp/.hpp` + `XoshiroCpp.hpp` (FetchContent) | `randomizer.cpp` | см. §4 |
| `collections.rs` | `smartlist.hpp`, `smartobjectarray.hpp`, `sortedarray.hpp` | `smartlist.*`, `smartobjectarray.*`, `sortedarray.*` | SmartList = двусвязный список с порядком вставки |
| `map.rs` | `resource_manager.hpp` (карта), `heatmap.hpp` (видимость), `maphashlist.hpp` | `resource_manager.*`, `heatmap.*` | `Surface` → битмаска (§3.2) |
| `setup.rs` | `resource_allocator.cpp/.hpp`, `continent.cpp/.hpp`, `continentfiller.*`, `game_manager.cpp` (alien-здания, зоны высадки) | `resource_allocator.*`, `continent.*`, `continentfiller.*` | RNG-тяжёлый модуль; порядок вызовов — канонический |
| `unit_values.rs` | `unitvalues.cpp/.hpp`, `resourcetable.cpp` (таблицы), `clans.cpp/.hpp` (клановые бонусы), `upgradecontrol.cpp` | `unitvalues.*`, `clans.*`, `upgradecontrol.*` | табл. 93×14 — const-данные; стоимость апгрейдов — см. риск 1 |
| `team.rs` | `ctinfo.hpp`, `teamunits.cpp/.hpp`, `researchmenu.cpp` (прогресс) | `ctinfo.hpp`, `teamunits.*` | `gold` — u32 (семантика порта) |
| `complex.rs` | `complex.cpp/.hpp`, `cargo.cpp/.hpp` | `complex.*`, `cargo.*` | |
| `unit.rs` | `unitinfo.cpp/.hpp`, `paths.hpp` (UnitPath), `unit.hpp/.cpp` (Unit-таблица) | `unitinfo.*`, `unitpath.*`, `unit.*` | только `S`-поля из state-model §3.9 |
| `hash.rs` | `hash.cpp/.hpp`, `maphashlist.cpp/.hpp` | `hash.*`, `maphashlist.*` | хэш-таблицы с упорядоченными ведёрками |
| `units_manager.rs` | `units_manager.cpp/.hpp` (списки, спавн, move, destroy, connectors) | `units_manager.*`, `unitevents.*` | |
| `unit_order.rs` | `units_manager.cpp` (`UnitsManager_ProcessOrder*`, 32 функции) | `unitinfo.cpp` (orders) | dispatch строго по порядку значений enum |
| `combat.rs` | `units_manager.cpp` (damage), `unitinfo.cpp` (`AttackUnit`, `Attack`, area), `aiattack.cpp/.hpp` | `taskattack.cpp`, `attack_census.*` | |
| `pathfinding.rs` | `groundpath.*`, `airpath.*`, `builderpath.*`, `pathfill.*`, `paths_manager.*` (только логика, без потоков), `pathrequest.*`, `path_worker.hpp` (свёрнут в sync) | `groundpath.*`, `airpath.*`, `builderpath.*`, `pathfill.*`, `paths_manager.*`, `terraindistancefield.*`, `circumferencewalker.*` | **синхронно**, порядок = порядок постановки запросов |
| `access.rs` | `access.cpp/.hpp`, `accessmap.cpp/.hpp` | `access.*`, `accessmap.*` | |
| `game.rs` | `game_manager.cpp/.hpp` (симуляционная часть), `order_log.cpp` (как тест-инструмент) | `game_manager.*` | tick/end-turn порядок — §5 |
| `victory.rs` | `winloss.cpp/.hpp`, `gameruleshandler.cpp/.hpp`, `mission.cpp/.hpp` (данные) | `winloss.cpp` | |
| `events.rs` | `unitevents.cpp/.hpp`, `remote.cpp` (применение пакетов) | `unitevents.*`, `remote.*` | |
| `input.rs` | `remote.cpp` (обработчики 08/38/41/…), `protocol.md` §8.4 | `transportorder.*`, `remote.*` | структуры P0–P5 для пакета 08 |
| `save.rs` | `saveload.cpp/.hpp`, `smartfile.*` (формат) | `saveload.*`, `smartfile.*` | V70, без I/O |
| `crc.rs` | `crc16.c/.h` | — | паритет пакета 45 |
| `ai/mod.rs` | `ai.cpp/.hpp` | `ai.*` | |
| `ai/strategy.rs` | `aiplayer.cpp/.hpp`, `clans.cpp` (выбор клана) | `aiplayer.*` | |
| `ai/weight.rs` | `weighttable.cpp/.hpp` | `weighttable.*` | |
| `ai/threat.rs` | `heatmap.cpp/.hpp`, `threatmap.hpp` (порт: `heatmap.hpp` покрывает оба) | `heatmap.*`, `threatmap.*` | |
| `ai/defense.rs` | `defense_manager.cpp/.hpp` | `defense_manager.*` | |
| `ai/task.rs`, `ai/task_manager.rs` | `task.cpp/.hpp`, `task_manager.cpp/.hpp`, `reminders.*` | `task.*`, `task_manager.*`, `reminders.*` | |
| `ai/tasks/*` | 51 файл `task*.cpp` порта (49 классов + `task.cpp` + `task_manager.cpp`) | одноимённые `task*.cpp` оригинала | см. §2.2 |

### 2.2 Модуль-таблица ИИ-задач (`ai/tasks/`)

Порт по одному файлу на класс (все имена совпадают с оригиналом — это доказано `functions-named.csv`):

| Rust-файл | C++ | Rust-файл | C++ |
|---|---|---|---|
| `task_move.rs` | `taskmove.cpp` | `task_kill_unit.rs` | `taskkillunit.cpp` |
| `task_attack.rs` | `taskattack.cpp` | `task_create_building.rs` | `taskcreatebuilding.cpp` |
| `task_attack_reserve.rs` | `taskattackreserve.cpp` | `task_create_unit.rs` | `taskcreateunit.cpp` |
| `task_support_attack.rs` | `tasksupportattack.cpp` | `task_manage_buildings.rs` | `taskmanagebuildings.cpp` |
| `task_frontal_attack.rs` | `taskfrontalattack.cpp` | `task_obtain_units.rs` | `taskobtainunits.cpp` |
| `task_wait_to_attack.rs` | `taskwaittoattack.cpp` | `task_repair.rs` | `taskrepair.cpp` |
| `task_check_assaults.rs` | `taskcheckassaults.cpp` | `task_reload.rs` | `taskreload.cpp` |
| `task_escort.rs` | `taskescort.cpp` | `task_transport.rs` | `tasktransport.cpp` |
| `task_rendezvous.rs` | `taskrendezvous.cpp` | `task_upgrade.rs` | `taskupgrade.cpp` |
| `task_retreat.rs` | `taskretreat.cpp` | `task_scavenge.rs` | `taskscavenge.cpp` |
| `task_clear_zone.rs` | `taskclearzone.cpp` | `task_place_mines.rs` | `taskplacemines.cpp` |
| `task_find_path.rs` | `taskfindpath.cpp` | `task_remove_mines.rs` | `taskremovemines.cpp` |
| `task_find_mines.rs` | `taskfindmines.cpp` | `task_remove_rubble.rs` | `taskremoverubble.cpp` |
| `task_get_materials.rs` | `taskgetmaterials.cpp` | `task_activate.rs` | `taskactivate.cpp` |
| `task_get_resource.rs` | `taskgetresource.cpp` | `task_move_home.rs` | `taskmovehome.cpp` |
| `task_search_destination.rs` | `tasksearchdestination.cpp` | `task_mine_assistant.rs` | `taskmineassistant.cpp` |
| `task_defense_reserve.rs` | `taskdefensereserve.cpp` | `task_power_assistant.rs` | `taskpowerassistant.cpp` |
| `task_assist_move.rs` | `taskassistmove.cpp` | `task_radar_assistant.rs` | `taskradarassistant.cpp` |
| `task_auto_survey.rs` | `taskautosurvey.cpp` | `task_habitat_assistant.rs` | `taskhabitatassistant.cpp` |
| `task_survey.rs` | `tasksurvey.cpp` | `task_connection_assistant.rs` | `taskconnectionassistant.cpp` |
| `task_abstract_search.rs` | `taskabstractsearch.cpp` | `task_defense_assistant.rs` | `taskdefenseassistant.cpp` |
| `task_explore.rs` | `taskexplore.cpp` | `task_frontier_assistant.rs` | `taskfrontierassistant.cpp` |
| `task_dump.rs` | `taskdump.cpp` | `task_create.rs` | `taskcreate.cpp` |
| `task_path_request.rs` | `taskpathrequest.cpp` | `task_update_terrain.rs` | `taskupdateterrain.cpp` |
| `task_debugger.rs` | `taskdebugger.cpp` | | |

Полный список сверяется по `glob(max-cpp-port/src/task*.cpp)` при реализации (51 файл: 49 классов таблицы + база `task.cpp` + менеджер `task_manager.cpp`).

### 2.3 Решения по существующему каркасу

| Элемент каркаса | Решение |
|---|---|
| `team::TeamId(pub u8)` | **сохранить**; константы `RED=0..GRAY=3`, `ALIEN=4`, `MAX=5` |
| `team::TeamKind` | **сохранить имя**; значения = `PlayerType` (`None/Player/Computer/Remote/Eliminated`); зафиксировать соответствие `Player` = save.md `TEAM_TYPE_HUMAN` |
| `unit::UnitId(pub u16)` | **сохранить**; добавить `NO_ID = UnitId(0xFFFF)`, методы `team()/object_index()` по битфилду §4 |
| `unit::UnitKind` (короткий список + `Other(u8)`) | **заменить** на полный `UnitType` (93 варианта, `#[repr(u16)]`, state-model §2.11) |
| `map::Surface` (исключающий enum) | **заменить** на битмаску (§3.2): клетка может быть `LAND\|COAST` и т.п. |
| `map::Map { surface, resources: Vec<Option<ResourceKind>> }` | **заменить** на `[Surface; MAP_TILES]` + `[ResourceMapEntry; MAP_TILES]` (state-model §3.10) |
| `map::MapSize { width: i16, height: i16 }` | **сохранить**; константа `MAP_SIZE: MapSize = {112, 112}`, `MAP_TILES = 12544` |
| `unit::Unit` (урезанный снапшот-юнит) | **заменить** полным `UnitInfo` (§3.6); старый тип удалить |
| `order::Order` (клиентские команды Move/Attack/…) | модуль сохраняется как **входной API**, но варианты пересобираются из пакетов протокола (§2.4); внутренняя машина приказов — отдельный модуль `unit_order.rs` (`UnitOrder`/`UnitOrderState`) |
| `SessionId` (uuid_like) | удалить из крейта (сеть — не наша забота) |
| `#[derive(Serialize, Deserialize)]` на типах состояния | сохранить (снапшот/дебаг), но **канонический байтовый дайджест** для CRC — отдельная явная сериализация (`save.rs`/`crc.rs`) |

### 2.4 Входная модель команд (`input.rs`)

`max_core` не знает про ENet, но знает про **события**, которые сеть в него вкладывает (protocol.md §8.4). Один enum `Command` с вариантами-пакетами:

| Вариант | Пакет | Поля (кратко) |
|---|---|---|
| `OrderUnit { unit_id, payload }` | 08 | `payload` — один из 6 процессоров P0–P5 (таблица ниже) |
| `MoveWithPath { unit_id, order, state, grid, move_to, group_speed, steps/air-data }` | 38 | наземный `PathStep{i8,i8}[]` или воздушный (1 шаг + `euclidean_distance i16`) |
| `MoveFinished { unit_id, mode, grid, speed, group_speed, shots, move_fraction }` | 41 | |
| `StopUnit { unit_id }` | 50 | |
| `Rename { unit_id, name }` | 43 | |
| `UpgradeUnitType { team, unit_type, 13×u16 }` | 10 | фиксированный порядок: `turns, hits, armor, attack, speed, range, rounds, move_and_fire, scan, storage, ammo, attack_radius, agent_adjust` |
| `UpgradeUnit { unit_id, 13×u16 }` | 20 | |
| `ComplexOrder { team, complex_id, 6×i16 }` | 11 | |
| `MissionSupply { team, round, … }` | 12 | выбор точки высадки |
| `DeployUnit { team, unit_type, grid }` | 14 | |
| `FinishedTurn { team }` | 06 | |
| `TurnBarrier / CrcBarrier { team, counter/crc }` | 00/52/45 | сеть передаёт их как факты для продвижения фаз |
| `ComplexSummary { team, turn, … }` | 53 | итоги баз на начало хода |
| `Seed { seed: u32 }` | 13 | инициализация RNG |

Процессоры пакета 08 (payload):

| Процессор | Порядки | Доп. поля |
|---|---|---|
| P1 | POWER_ON/OFF, EXPLODE, SENTRY, LAND, TAKE_OFF, LOAD, IDLE, HALT_BUILDING(_2), AWAIT_SCALING, AWAIT_TAPE_POSITIONING, LAY_MINE | `order, state, prior_order, prior_state, disabled_reaction_fire` |
| P2 (+P1) | UNLOAD, REPAIR, RELOAD, UPGRADE | `parent_unit_id u16 (0=нет), experience i16` |
| P3 (+P2) | AWAIT, MOVE, FIRE, ACTIVATE, TRANSFER, AWAIT_STEAL/DISABLE_UNIT, MOVE_TO_UNIT, MOVE_TO_ATTACK | `move_to x/y, fire_on x/y, enemy_unit_id, transfer_cargo, stealth_dice_roll` |
| P4 (+P3) | TRANSFORM, BUILD, CLEAR, DISABLE | `repeat_build, build_time, build_rate, build_count, unit_type[]` |
| P5 (+P3) | NEW_ALLOCATE | `total/raw/fuel/gold_mining` |
| P0 | ORDER_17 | пусто |

Применение команд происходит **только в детерминированной точке цикла** (вместо `Remote_ProcessNetPackets()` в C++ — §5.1): все команды, накопленные между тиками, применяются в порядке поступления из очереди (`events.rs`).

---

## 3. Таблица соответствия типов C++ → Rust

### 3.1 Скаляры и правила арифметики

| C++ | Rust | Примечания по арифметике |
|---|---|---|
| `uint8_t` | `u8` | инкременты счётчиков (`moved`, `shots`, `recoil`) — `wrapping_add` там, где C++ заворачивается; **проверить каждый сайт** |
| `int8_t` | `i8` | `ScreenLocation`, `turret_offset_*` (UI — не в симе), `PathStep` |
| `uint16_t` | `u16` | `hits/speed` (в порту u16, в V70 u8 — канон для сима: u16), `movement_wait_count` (лимит 512 — переполнения нет), `hash_size` |
| `int16_t` | `i16` | `grid_x/y`, `storage`, `transfer_cargo`, `experience`, `Complex.*` (6 полей), `score_graph[50]`, `Point.x/y`. **Сложение `i16+i16` в C++ идёт через `int`** (продвижение!), затем сужение при записи — в Rust: считать в `i32`, сужать `as i16` (wrapping) |
| `uint32_t` | `u32` | `flags`, `team_points`, `gold`, `unit_counters[93]`, `casualties[93]`, `stats_gold_spent_on_upgrades`, `live_attack`, `ai_state_bits`, `id` (внутр. `UnitId` u16 + `NO_ID=UINT32_MAX` — см. §3.3) |
| `int32_t` | `i32` | `turn_counter`, `research_level/turns_to_complete/allocation`, координаты-вычисления. C++-деление со знаком усекает к нулю; Rust `/` — так же. `>>` на отрицательных — arithmetic в обоих |
| `int64_t` | `i64` | только внутри `Randomizer_Generate` (§4.2) |
| `size_t` | `usize`/`u64` | **только** в `Randomizer_Generate(sizeof(...))` — отдельная ветка генерации (§4.2) |
| `bool` | `bool` | сериализация 1 байт |
| `char[30]`, `char[4][30]` | `String` / `[String;4]` | ≤30 байт, UTF-8 |
| указатели/`SmartPointer<T>` | `Option<UnitId>` / `Option<ComplexId>` / владение | см. §3.3 |

### 3.2 Перечисления (все с `#[repr]`, `Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize`)

| C++ (enums.hpp / save.md) | Rust | Repr/значения | Замечания |
|---|---|---|---|
| `PlayerType` / `TeamType` | `TeamKind` (каркас) | `#[repr(u8)]`: None=0, Player=1, Computer=2, Remote=3, Eliminated=4 | имя из каркаса, значения C++ |
| `TeamClanType` | `TeamClan` | `#[repr(u8)]`: Random=0, TheChosen=1, …, AxisInc=8 | |
| `PlayerTeamType` | `TeamId(u8)` newtype + константы | RED..GRAY=0..3, ALIEN=4, MAX=5 | newtype, не enum (state-model §2.3) |
| `OpponentType` | `Opponent` | `#[repr(u8)]` 0..5 | |
| `PlayMode` (anon) | `PlayMode` | `#[repr(u8)]`: TurnBased=0, SimultaneousMoves=1, Unknown=2 | |
| `VictoryType` (anon) | `VictoryType` | `#[repr(u8)]`: Duration=0, Score=1 | |
| `PlanetType` | `Planet` | `#[repr(u8)]` 0..23 (24 планеты) | |
| `ResearchTopicType` | `ResearchTopic` | `#[repr(u8)]` 0..7 + COUNT=8 | |
| `SurfaceType` (биты) | `struct Surface(u8)` | const: NONE=0, LAND=0x1, WATER=0x2, COAST=0x4, AIR=0x8 | **битмаска**, не enum |
| `ResourceID` (тип юнита) | `UnitType` | `#[repr(u16)]`, 93 варианта `CommTower=0 … DeadWaldo=92` | порядок = state-model §2.11; `from_u16` → `Option` |
| `UnitOrderType` | `UnitOrder` | `#[repr(u8)]`, 32 значения 0x00–0x1F | `Order17 = 0x12` (имя в C++ не восстановлено) |
| `UnitOrderStateType` | `UnitOrderState` | `#[repr(u8)]`, 47 значений 0x00–0x2E | |
| `GameStateType` | `GameStateType` | `#[repr(u8)]` 0x0–0xF | неигровые фазы — только для парсинга сейвов |
| `WinLossState` | `WinLossState` | `#[repr(u8)]`: Generic=0, Pending=1, Won=2, Lost=3 | |
| `AiStrategyType` | `AiStrategy` | `#[repr(u8)]` 0..9 + MAX | |
| `CautionLevelType` | `CautionLevel` | `#[repr(u8)]` 0..3 | |
| `ConnectorDirectionType` | consts в `complex.rs` | 8 бит + `CONNECTION_BEING_TESTED=0x400` | |
| `AccessModifierType` | consts в `access.rs` | 6 бит | |
| `InfoMapType` | consts в `ai/threat.rs` | `NO_INFO/EXPLORED/MINE_FIELD/FRONTIER/CLEAR_OUT_ZONE` | |
| `UnitAngleType` / `Direction` | `Direction` (8 значений) | N..NW = 0..7 | `UnitsManager_GetTargetAngle/GetFiringAngle` считают в i32, результат маскируется `& 7` — `as u8 & 7` |
| флаги юнитов (anon 0x1…0x10000000) | `UnitFlags` (const u32 + хелперы) | `GROUND_COVER=0x1 … REGENERATING_UNIT=0x10000000` | без крейта bitflags — consts, как в C++ |
| `CARGO_*` | consts в `map.rs`/`complex.rs` | `FUEL=0x20, GOLD=0x40, MATERIALS=0x80, MASK=0x1F` | |
| `AI_STATE_*` (UnitInfo) | consts | `NO_RETREAT=1, TARGET_COOLDOWN_MASK=0xFE, MOVE_FINISHED_REMINDER=0x100, RETREAT_REMINDER=0x200` | |
| `CheatCodeType` | не в симе | — | читы — серверная/клиентская логика вне max_core (или опциональный флаг) |

### 3.3 Идентификаторы, ссылки, Option

| C++ | Rust | Решение |
|---|---|---|
| `UnitInfo::id` (u32; 0xFFFF… нет id; битфилд team<<13\|index) | `UnitId(u16)`; `NO_ID = UnitId(0xFFFF)` | в V70 хранится u16; внутренняя константа `NO_ID` = 0xFFFF; битфилд: `team() = id>>13`, `object_index() = id & 0x1FFF` |
| `UnitsManager_UnitIdCounter` (глобальный u32) | `GameState.unit_id_counter: u32` | старт после загрузки V70: `PLAYER_TEAM_MAX << 13` (открытый вопрос 7.8 state-model) |
| `parent_unit`, `enemy_unit`, `selected_unit` (`SmartPointer`, null) | `Option<UnitId>` | |
| `complex` (`SmartPointer<Complex>`) | `Option<ComplexId>` (в скоупе команды) + доступ `state.complex(team, id)` | |
| `base_values` (`SmartPointer<UnitValues>`) | `Option<Box<UnitValues>>` либо `Box<UnitValues>` (всегда есть у игровых юнитов) | |
| `path` (`SmartPointer<UnitPath>`) | `Option<UnitPath>` | |
| `unit_list` (указатель на один из 5 списков) | enum `UnitListKind { GroundCover, MobileLandSea, Stationary, MobileAir, Particle }` в `UnitInfo` | |
| `TeamUnits* team_units` (CTInfo) | в `TeamUnits` не нужно: `GameState.teams[team].team_units` по индексу | |

### 3.4 Структуры

| C++ (порт) | Rust | Ключевые поля и типы | Option/wrapping |
|---|---|---|---|
| `Point` (`point.hpp`) | `Point { x: i16, y: i16 }` | | |
| `Rect` | `Rect { ulx: i32, uly: i32, lrx: i32, lry: i32 }` | UI (не в симе) | |
| `ScreenLocation` | `ScreenLocation { x: i8, y: i8 }` | UI | |
| `UnitValues` (`unitvalues.hpp`) | `UnitValues` | `turns,hits,armor,attack,speed,fuel,range,rounds: u16; move_and_fire: u8; scan,storage,ammo,attack_radius,agent_adjust,version: u16; in_use: bool` | `fuel` runtime-поле, не сериализуется; `GetAttribute/AddAttribute` — через индексы ATTRIB_* |
| `ResearchTopic` (`ctinfo.hpp`) | `ResearchTopic { research_level: u32, turns_to_complete: u32, allocation: u32 }` | | ходы факт.: `ceil(turns_to_complete / allocation)` |
| `CTInfo`/`TeamInfo` | `TeamInfo` | `team_type: TeamKind; finished_turn: bool; unit_counters: [u32;93]; team_clan: TeamClan; research_topics: [ResearchTopic;8]; team_points: u32; stats_*: i16×4; score_graph: [i16;50]; casualties: [u32;93]; stats_gold_spent_on_upgrades: u32` | UI-поля (`camera_*`, `display_*`, `selected_unit`, `zoom`) — исключены из сима (state-model §6.3); `number_of_objects_created` — в `GameState` |
| `TeamUnits` | `TeamUnits` | `gold: u32; base_values: [UnitValues;93]; current_values: [UnitValues;93]; complexes: SmartList<Complex>` | V70: gold i16, канон сима: u32 |
| `Complex` | `Complex` | `material,fuel,gold,power,workers,buildings: i16; id: i16` | сужение из i32 — `as i16` |
| `UnitInfo` | `UnitInfo` | полный набор `S`-полей state-model §3.9: `unit_type, flags: u32, grid_x/y: i16, team: TeamId, unit_id: u8, orders: UnitOrder, state: UnitOrderState, prior_orders/state, laying_state, target_grid: Point, build_time: u8, *_mining: u8×7, hits/speed: u16, shots: u8, move_and_fire: u8, storage: i16, experience: i16 (из storage V70), ammo: u8, targeting_mode, enter_mode, recoil→(firing_recoil_frames, disabled_turns_remaining): u8×2, delayed_reaction: u8, damaged_this_turn: bool, research_topic: ResearchTopic, moved: u8, repeat_build: bool, build_rate: u16, disabled_reaction_fire: bool, auto_survey: bool, ai_state_bits: u32, path: Option<UnitPath>, connectors: u16, build_list: Vec<UnitType>, visible_to_team/spotted_by_team: [bool;5], complex: Option<ComplexId>, parent_unit/enemy_unit: Option<UnitId>, movement_wait_count: u16, group_speed: u16, transfer_cargo: i16, stealth_dice_roll: u8, pin_count: i16, in_transit: bool, tasks_enabled: bool` + `tasks: SmartList<Task>` (ИИ) | UI-поля (`pixel x/y`, спрайты, `angle`, `velocity`, `cursor`, …) исключены; `comm/fuel_distance/energized` не восстанавливаются портом — исключены |
| `TeamMissionSupplies` | `TeamMissionSupplies` | `units: Vec<UnitType>; cargos: Vec<u16>; starting_position: Point; team_gold/start_gold: u16; unit_count: u16; loadout_cargos: Vec<u16>; proximity_alert_ack: u8` | |
| глобалы `GameManager_*` | поля `GameState` | `turn_counter: i32, turn_timer: u16, play_mode: PlayMode, active_turn_team/player_team: TeamId, game_state: GameStateType` | |
| `WinLoss_Status` | `WinLossStatus` | `mission_type: i32; team_type: [i32;4]; team_status: [WinLossState;4]; team_rank: [i32;4]; teams_*: i32` | |
| `UnitPath`/`PathStep` | `UnitPath { steps: Vec<PathStep> }`, `PathStep { x: i8, y: i8 }` | воздушный путь — 1 шаг + `distance_x/y: i32, euclidean_distance: i16` | |
| `HeatMap` | `HeatMap { complete: [u8;112×112]? , stealth_land/sea }` | типы уточнить по `heatmap.hpp` при P6 | |
| `WeightTable` | `WeightTable` | целочисленные веса + `Roll()` через RNG | |
| `Task` / `Reminder` | `Task` (trait + enum) / `Reminder` | поля — при P6 | |
| `Cargo` (`cargo.hpp`) | `Cargo { raw: i32, fuel: i32, gold: i32, life: i32 }` | уточнить по заголовку при P3 | |

### 3.5 Где нужны wrapping-операции (сводно)

| Место | C++ поведение | Rust |
|---|---|---|
| `Randomizer_Generate` | i64-умножение + arithmetic `>> 32` | `((next as i64).wrapping_mul(v as i64) >> 32)`; для `T=size_t`: `((next as u64).wrapping_mul(v) >> 32)` |
| Xoshiro128** / SplitMix64 | беззнаковое заворачивание | `wrapping_mul/wrapping_add/wrapping_shl/rotl` |
| `moved`/`shots`/`ammo`-инкременты (u8) | `x++` = mod 256 | `wrapping_add(1)` (после аудита каждого сайта) |
| `movement_wait_count` (u16) | инкремент с лимитом 512 (`MOVEMENT_WAIT_LIMIT`) | без переполнения, но `wrapping_add` для единообразия |
| `i16`-поля (`Complex`, `storage`, `transfer_cargo`, `score_graph`) | `short` сужение из `int` | считать в i32 → `as i16` |
| `angle` (`GetTargetAngle/GetFiringAngle`) | i32 → u8 | `as u8 & 7` (после сверки с C++) |
| `hash_id`-битфилд | `(team<<13)|index`, `NO_ID=0xFFFF` | u16-операции |
| урон/броня | i32-математика, деления со знаком | i32, `/` (усечение к нулю — совпадает с C++) |
| `team_points`, `gold`, `casualties` | u32 | `wrapping_add` (практически не переполняются) |
| CRC16 | табличный/побитовый | порт `crc16.c` 1:1 |

---

## 4. RNG: алгоритм, состояние, точки вызова

### 4.1 Порождающие функции (порт 1:1)

`max-cpp-port/src/randomizer.cpp` использует **Xoshiro128\*\*** из `XoshiroCpp.hpp` с сидированием через **SplitMix64**:

```
SplitMix64 (u64-состояние s):
    s += 0x9E3779B97F4A7C15          (wrapping)
    z = s
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9
    z = (z ^ (z >> 27)) * 0x94D049BB133111EB
    вернуть z ^ (z >> 31)

set_seed(seed: u32):
    splitmix = SplitMix64(seed)
    state[0..4] = low32(splitmix()), low32(splitmix()), low32(splitmix()), low32(splitmix())

next_u32() — Xoshiro128StarStar:
    result = rotl(state[1] * 5, 7) * 9        (u32, wrapping)
    t      = state[1] << 9
    state[2] ^= state[0]; state[3] ^= state[1]; state[1] ^= state[2]; state[0] ^= state[3]
    state[2] ^= t; state[3] = rotl(state[3], 11)
    вернуть result
```

Rust: `struct Randomizer { s: [u32; 4] }` с `set_seed(u32)`, `next_u32()`, `generate_i32/generate_u32/generate_u64` (две ветки семантики — §3.5). Крейт-зависимости: **нет** (никакого `rand`).

### 4.2 Семантика `Randomizer_Generate<T>`

C++: `static_cast<T>((static_cast<int64_t>(next) * value) >> 32)`.

- `T` = int/u32/u16/u8/счётчики (`101`, `2`, `GetCount()`, …): произведение в int64, сдвиг arithmetic (для неотрицательного произведения = logical). Rust: `generate_i64(value)`.
- `T` = `size_t` (`Randomizer_Generate(sizeof(ExpResearchTopics))` — `unitinfo.cpp:1485/1495/1518`): `int64_t * uint64_t` → **беззнаковое** u64-умножение, logical `>> 32`. Rust: `generate_u64(value)`.
- Все сайты с аргументом-константой фиксируются в таблице сайтов (P1).

### 4.3 Точки вызова RNG в симуляции (полный аудит — фаза P1/P6)

Из `grep Randomizer_*` по `max-cpp-port/src` (сим-релевантные сайты; меню/звук/рендер исключены):

| Файл | Сайты | Что рандомизируется |
|---|---|---|
| `ai.cpp:109-110` | позиция базы ИИ | стартовая точка |
| `aiattack.cpp:421,732` | стелс-бросок (101), ветвление | атака ИИ |
| `aiplayer.cpp` (~30 сайтов: 1114–1244, 1351–1632, 2808, 3944, 4061) | стратегия, клан, снабжение, минные поля, апгрейды, скоринг | ИИ-настройки |
| `continent.cpp:139-140` | позиция континента | генерация карты |
| `game_manager.cpp:723, 4818–5010` | размещение alien-зданий/юнитов, тайлы, стартовые позиции | setup |
| `resource_allocator.cpp:30, 227–271` | концентрации ресурсов, смещения | генерация ресурсов |
| `resource_manager.cpp:1286` | выбор клана | setup |
| `taskattack.cpp:282` | ветвление атаки | ИИ |
| `unitinfo.cpp:1485–1518, 4139` | тема апгрейда (через `sizeof` → ветка u64), тайл | апгрейды |
| `units_manager.cpp:3495, 3783` | тайлы, кадры | юниты |
| `weighttable.cpp:92` | взвешенный выбор | ИИ |

Правила: `Randomizer_SetSeed(Remote_RngSeed)` (пакет 13) — единственная точка сидирования сима; `Randomizer_Init/SetSeed(time(...))` — только меню/локальные режимы, в max_core не переносятся. Порядок вызовов определяется порядком исполнения кода; аудит фиксируется тестом-счётчиком (§6.2).

---

## 5. Состояние игры и порядок исполнения (канонический)

### 5.1 Tick-цикл (`game.rs::tick`)

Воспроизводит `GameManager_ProcessTick` → симуляционную часть, с сетью, заменённой на очереди:

1. применить накопленные входные команды (`events.rs`) — аналог `Remote_Synchronize`/`Remote_ProcessNetPackets` в **порядке поступления**;
2. если фаза ≠ `TURN_ACTIVE`: `units_manager::process_orders(&mut state)` — полный прогон §5.2;
3. `Ai_CheckReactions` **через флаг чередования** `update_flag` (в C++ реакции зовутся только на чётных тиках — `GameManager_UpdateFlag = !GameManager_UpdateFlag`);
4. `pathfinding::poll_and_dispatch` — результаты путей применяются в **порядке постановки запросов** (см. риск 2);
5. проверка конца хода (`Ai_CheckEndTurn`, `finished_turn`, барьерные факты из `input.rs`).

**Запрещено**: тайм-бюджеты (`TickTimer_HaveTimeToThink`, лимиты `ExecuteReminders`) — в C++ они управляются wall-clock; в max_core очередь ИИ-напоминаний обрабатывается **полностью и детерминированно** за тик (риск 16).

### 5.2 `UnitsManager_ProcessOrders` — фиксированный порядок

Порт `units_manager.cpp:1860-1943` дословно:

1. `bob_effect_quota = 5`; сброс флагов задач ИИ (`Ai_ClearTasksPendingFlags`);
2. обход **пяти списков строго по порядку**: `GroundCoverUnits` → `MobileLandSeaUnits` → `StationaryUnits` → `MobileAirUnits` → `ParticleUnits`; внутри списка — **порядок вставки SmartList** (с учётом `MoveToFrontInUnitList` и т.п. — операции портируются как есть);
3. `pending_attacks`: обработка головы списка (проверка `hits == 0` / `state == DESTROY` / `order != FIRE` / `shots > 0`), иначе `AssessAttacks()`;
4. `pending_air_group_leader` → `Access_ProcessNewGroupOrder`;
5. очередь `UnitEvents` (`Process()` каждого, затем `Clear()`);
6. `reset_bob_state = bob_effect_quota`.

Процессор приказа (`unit_order.rs`) — dispatch по значению `UnitOrder` в порядке 0x00→0x1F (как `switch` в `UnitsManager_ProcessOrder`, `units_manager.cpp:3257-3396`), включая «странные» маппинги: `ORDER_MOVE_TO_UNIT` → `ProcessOrderMove`, `ORDER_HALT_BUILDING_2` → `ProcessOrderHaltBuilding` и т.п. — **переносятся как есть, с комментариями**.

### 5.3 Конец хода (`game.rs::end_turn`)

Порт `GameManager_ManageEconomy` + окружения (`game_manager.cpp:2042-2087`):

1. `team_units.optimize_complexes(team)` (`TeamUnits::OptimizeComplexes`);
2. `UpdateProduction` по спискам: `MobileLandSeaUnits` → `StationaryUnits` → `MobileAirUnits` (внутри — порядок вставки);
3. `ResupplyUnits` по тем же трём спискам в том же порядке (пропуск `ORDER_DISABLE`);
4. исследования: цикл `i in 0..RESEARCH_TOPIC_COUNT` — если `allocation && turns_to_complete == 0` → пересчёт прогресса (риск 1: формула с `std::pow`);
5. `UpdateScoreGraph`: сдвиг `score_graph` на 1 влево (`memmove`), последний элемент = `team_points`;
6. проверка победы/поражения (`victory.rs`, §5.4);
7. счётчик хода, смена `active_turn_team`, барьеры — фактами из `input.rs`.

### 5.4 Победа/поражение (`victory.rs`)

Порт `winloss.cpp` дословно:

- фильтр команд: `team_type != NONE`, статус через `TestWinLossConditions` → `WinLossState`;
- сортировка `teams_in_play` (пузырёк по убыванию `team_points`; при равенстве — по `WinLoss_GetTotalWorthOfAssets` (сумма `GetNormalRateBuildCost` по `MobileLandSea + MobileAir + Stationary`); стабильность C++-цикла `i<j` сохраняется);
- ранги `team_rank = i+1`;
- generic-условия (state-model §5.1): SCORE/DURATION лимиты, «все соперники проиграли», ничьи;
- миссионные предикаты (state-model §5.2) — таблица «mission_index → список предикатов», данные (риск 8).

---

## 6. Чек-лист детерминизма

### 6.1 RNG

- [ ] алгоритм Xoshiro128** + SplitMix64 — порт 1:1 (§4.1), юнит-тесты на эталонных векторах (§7.1);
- [ ] состояние: `[u32;4]`, сериализуемо, входит в CRC-дайджест;
- [ ] единственная точка сидирования в симе — `Command::Seed`;
- [ ] **все** точки вызова (таблица §4.3) перенесены в том же порядке; тест «счётчик вызовов RNG на тик» сверяется с C++-эталоном;
- [ ] `Generate` для `T≤u32` — ветка i64; `T=u64` — ветка u64 (§4.2).

### 6.2 Порядки обхода

- [ ] 5 списков юнитов в фиксированной последовательности (§5.2), внутри — порядок вставки;
- [ ] хэш-ведёрки (`UnitHash`/`MapHash`) — упорядоченные списки с той же хэш-функцией и размером, что в `hash.cpp`; итерация по ведёрку = порядок вставки;
- [ ] команды — всегда `0..4` (не по `HashMap`);
- [ ] `research_topics` — `0..8`, `unit_counters/casualties` — `0..93`, атрибуты `UnitValues` — фиксированный порядок полей;
- [ ] `SmartList`-операции (`PushBack/PushFront/InsertAfter/MoveToFrontInUnitList/Remove`) портированы с той же результирующей последовательностью;
- [ ] очереди ИИ: priority-reminders → normal → deferred, внутри — порядок вставки; `SmartList<Task>` — порядок регистрации;
- [ ] `UnitEvents`, `PendingAttacks`, `DelayedAttackTargets[5]`, `UnitsManager_LiveAttacks` — порядок обработки как в C++;
- [ ] результаты pathfinding применяются в порядке постановки запросов (риск 2).

### 6.3 Арифметика

- [ ] все u8/u16/u32-инкременты — `wrapping_*` после аудита (таблица §3.5);
- [ ] i16-поля: счёт в i32, сужение `as i16`;
- [ ] деление со знаком — усечение к нулю (Rust `/` = C++ `/`);
- [ ] сдвиги: `>>` на i64 в `Generate` — arithmetic; на беззнаковых — logical;
- [ ] ни одной `f32/f64` в runtime-симуляции (clippy `float_arithmetic` в `#![deny]` для модулей сима); float-формулы C++ (`std::pow`) — build-time-таблицы (риск 1);
- [ ] нет `as`-кастов с плавающей точкой;
- [ ] `abs`/`labs` — `abs_diff`/`wrapping_abs` с проверкой MIN-значений (в C++ `labs(INT_MIN)` = UB; искать такие сайты).

### 6.4 Запрещено в крейте

| Запрет | Обоснование/замена |
|---|---|
| `f32`/`f64` в симуляции | платформенная нестабильность `std::pow`/libm; замена — целочисленные формулы + build-time-таблицы |
| `HashMap`/`HashSet` **итерация** | нестабильный порядок; разрешён только lookup; итерация — по `Vec`/`SmartList`/упорядоченным ведёркам |
| `SystemTime`/`Instant`/`thread_rng`/`rand` | недетерминизм; время — входные тики |
| потоки (`std::thread`/rayon) в симуляции | pathfinding синхронный |
| чтение env/файлов | чистота крейта |
| итерация по указателям/адресам | в Rust адресов нет — только явные коллекции |
| `serde_json`/float-сериализация для CRC | канонический дайджест — ручной байтовый writer (`crc.rs`) |

### 6.5 Время и случайность из ОС

- [ ] таймеры хода/конца хода — поля состояния, декрементируются только внешними тиковыми событиями (сервер — авторитет времени);
- [ ] паузы (`39/40/46`) — факты из `input.rs`, на состояние сима влияют только через задержку тиков;
- [ ] `Randomizer_Init(time(nullptr))` — не портируется;
- [ ] `NetNode`-id (31991) — вне сима (сетевой крейт).

---

## 7. Порядок портирования (фазы с критериями)

Общее правило: каждая фаза завершается только когда **все** её критерии зелёные; C++-порт не трогается, кроме папки `test/` и нового таргета-генератора.

### P0 — Фундамент: типы и константы

| Шаг | Содержимое |
|---|---|
| 1 | Переписать каркас: `lib.rs` (модули), `enums.rs`, `geo.rs`, `collections.rs`; выпилить `uuid_like`, старый `Unit` |
| 2 | `UnitType` (93), `UnitOrder` (32), `UnitOrderState` (47), `TeamClan`, `PlayMode`, `VictoryType`, `Planet`, `Opponent`, `ResearchTopic`, `GameStateType`, `WinLossState`, `Surface`-битмаска, `UnitFlags`, `CARGO_*`, `CONNECTOR_*` |
| 3 | `UnitId/TeamId/ComplexId`, `Point`, `SmartList` с порядком вставки |

**Критерии**: `cargo test`; тесты констант: значения `#[repr]` == C++ (`#[repr(u16)] UnitType::Tank as u16 == 51` и т.д. — таблица генерируется из `enums.hpp`), `MAP_TILES == 12544`, размеры структур, `UnitId::NO_ID == 0xFFFF`; `#![deny(clippy::float_arithmetic)]` компилируется.

### P1 — RNG + эталонные векторы

| Шаг | Содержимое |
|---|---|
| 1 | `rng.rs` — SplitMix64 + Xoshiro128** + `generate_i64/generate_u64` |
| 2 | **Генератор эталонов** `max-cpp-port/test/rng_vector_generator.cpp`: компилирует только `randomizer.cpp` + `XoshiroCpp.hpp` (зависимостей нет), печатает CSV: для seeds {0, 1, 2, 0xDEADBEEF, 1234567890, 3 случайных} — 100k `next()` и `Generate(x)` для `x ∈ {1,2,5,7,21,24,100,101,31991, size_t(8)}` |
| 3 | CSV-фикстуры в `max-brh/crates/max_core/testdata/rng/` + тест сверки; таргет генератора в `test/CMakeLists.txt` |
| 4 | Таблица сайтов вызова RNG (§4.3) с пометкой «перенесён/нет» — живой аудит в DESIGN/issue-трекере |

**Критерии**: все векторы совпадают; тест «счётчик вызовов» готов к использованию следующими фазами.

### P2 — Карта, террейн, генерация

| Шаг | Содержимое |
|---|---|
| 1 | `map.rs`: `Map`, `ResourceMapEntry { amount: u8, kind: ResourceKind, visibility: u8 }`, видимость команд (маска `0x3C00` — см. риск 5) |
| 2 | `access.rs` + `maxfloodfill` (заливка, поиск свободных клеток) |
| 3 | `setup.rs`: `resource_allocator` (нормальные/концентрированные залежи, сепарация, диффузия), континенты, alien-здания/юниты, стартовые позиции — **с точным порядком RNG-вызовов** |

**Критерии**: для фиксированного seed карта (поверхность+ресурсы+континенты) побайтово равна дампу C++-генератора карты (тот же `test/`-таргет, расширенный флагом `--dump-map`); тесты свойств: количество/типы ресурсов в лимитах.

### P3 — Юниты, команды, комплексы, экономика

| Шаг | Содержимое |
|---|---|
| 1 | `unit_values.rs` + const-таблицы атрибутов 93×14 (из `resourcetable.cpp`/данных юнитов) + клановые модификаторы (`clans.cpp`) |
| 2 | `team.rs`, `complex.rs`, `unit.rs`, `hash.rs` |
| 3 | `units_manager.rs` (спавн/деплой/уничтожение/движение/коннекторы) — без боя |
| 4 | `save.rs` (V70 чтение/запись) — минимально для фикстур |

**Критерии**: юнит-тесты констант (атрибуты каждого из 93 типов == C++-таблицам); загрузка `.dta`-фикстуры (V70) и выгрузка обратно даёт байтово идентичный файл (round-trip); спавн юнита меняет состояние так же, как C++ (сравнение дампов Packet23Data).

### P4 — Приказы и ход

| Шаг | Содержимое |
|---|---|
| 1 | `unit_order.rs`: 32 процессора + `SetNewOrder(Int)` со всеми ветками (AWAKEN_SCALING, NEW_ORDER, FIRE-удержание) |
| 2 | `game.rs`: tick-цикл §5.1, `process_orders` §5.2, конец хода §5.3 |
| 3 | `events.rs` + `input.rs` (команды P0–P5, 38/41/50/06/53) |

**Критерии**: скрипт-реплей (общий формат скриптов с C++-эталоном, §8.2): одиночный юнит получает приказ `ORDER_MOVE` и доходит до цели; пошаговый дамп состояния (тик/ход) совпадает с C++-эталоном для всех базовых приказов; порядок обхода списков зафиксирован тестом с «канарейками».

### P5 — Бой, движение, пути

| Шаг | Содержимое |
|---|---|
| 1 | `combat.rs`: `CalculateAttackDamage`, `GetAttackDamage`, реактивный огонь, стелс, area-attack, `PendingAttacks` |
| 2 | движение: `StepMoveUnit`, `MoveFinished`, `AttemptSideStep`, задержки (`movement_wait_count`) |
| 3 | `pathfinding.rs`: GroundPath/AirPath/BuilderPath, PathFill, TerrainDistanceField, CircumferenceWalker — синхронно, порядок = очередь запросов |

**Критерии**: сценарии «танк атакует танк», «артиллерия по площади», «подлодка-стелс» — дампы урона/позиций == C++-эталону; пути совпадают пошагово (включая спорные узлы, выбор из равнозначных — сравнение полных путей); юнит-тесты формул урона на крайних значениях (0 hp, атака < броня, деление на 0 — паритет с C++).

### P6 — ИИ-задачи

| Шаг | Содержимое |
|---|---|
| 1 | `ai/task.rs`, `ai/task_manager.rs` (очереди reminders, `FindTaskForUnit`, `RemoveDestroyedUnit`) |
| 2 | `ai/strategy.rs` (AiPlayer), `ai/weight.rs`, `ai/threat.rs`, `ai/defense.rs` |
| 3 | 51 класс задач `ai/tasks/*` — в порядке зависимости (сначала `taskmove/taskattack/taskcreatebuilding/taskcreateunit`, затем остальные) |

**Критерии**: AI-vs-AI партия на 30 ходов с фиксированным seed: хэш состояния после каждого хода совпадает с C++-эталоном; «счётчик вызовов RNG» совпадает; счётчики построек/юнитов/потерь идентичны.

### P7 — Победа/поражение и сквозные тесты

| Шаг | Содержимое |
|---|---|
| 1 | `victory.rs` (§5.4): generic-условия + предикаты миссий (данные) |
| 2 | `crc.rs`: CRC16 как в `crc16.c` + канонический дайджест состояния |
| 3 | Сквозные тесты на `.dta`-фикстурах: загрузка → N ходов по скрипту → сравнение полного состояния с C++-эталоном (и/или save → compare) |

**Критерии**: фикстура «сохранение на 10-м ходу реальной партии» проигрывается в Rust и C++ и даёт идентичное состояние + идентичный CRC; условия победы срабатывают на тех же ходах.

---

## 8. Стратегия тестирования

### 8.1 Юнит-тесты констант и типов

- значения `#[repr]` всех enum против таблиц из `enums.hpp`/state-model.md (генерируются скриптом, тесты — литеральные, читаемые);
- размеры/раскладки: `size_of::<UnitValues>()`, счётчики 93/8/50, `MAP_TILES`;
- семантика `SmartList`: порядок при push/pop/insert/move-to-front, `Remove` по значению, копирование (order-preserving);
- `UnitId`-битфилд: `(team, index) ⇄ u16`, `NO_ID`;
- исследование: `turns = ceil(turns_to_complete / allocation)` — граничные случаи (0 allocation, 0 turns).

### 8.2 Эталонные векторы и C++-headless сравнение

1. **RNG-векторы** (P1) — CSV от `test/rng_vector_generator.cpp` (§7, P1).
2. **Карта** (P2) — тот же таргет с `--dump-map <seed>`: сырой дамп `SurfaceMap`+`ResourceMap`+континентов.
3. **Скрипт-реплей** (P4+) — новый таргет `test/reference_runner` в `max-cpp-port/test/`:
   - вход: `.dta`-фикстура или чистая партия (seed/правила) + текстовый скрипт команд (`deploy`, `order 08 <unit> <payload>`, `path 38 …`, `end_turn`, `tick N`);
   - работа: headless-прогон (флаги порта `--server`, без рендера), вывод построчно: номер тика/хода, CRC16 состояния, счётчик вызовов RNG, список `(unit_id, Packet23Data)` изменённых юнитов, `TeamInfo`, `WinLoss_Status`;
   - Rust-сторона: тот же скрипт через `max_core` → идентичный текстовый дамп → `diff`. Скрипты и эталонные дампы коммитятся в `max-brh/crates/max_core/testdata/replay/`.
4. **Order-log** — в порту уже есть `order_log.cpp` (лог приказов/переходов): добавить режим дампа канонического лога → сравнение с логом max_core (отладочный `order_log`-модуль в Rust, `#[cfg(feature)]`).
5. **Сейв-паритет** (P7) — загрузка `.dta`, N ходов, выгрузка: побайтовое сравнение с эталонным сейвом C++ (в рамках полей, которые порт реально пишет).

### 8.3 Фикстуры из `.dta`-сейвов

- формат V70 (state-model.md + `max-port-site/save.md`); `save.rs` — чистый парсер/писатель `&[u8]`;
- фикстуры: реальные сейвы (из ресурсов игры) + синтетические (сгенерированные headless-портом на N-м ходу скрипта);
- каталог: `testdata/saves/{name}.dta` + метаданные `{name}.json` (seed, правила, описание сценария);
- запрещено коммитить сейвы, полученные недетерминированным путём.

### 8.4 Property-тесты (dev-dependency `proptest`, фиксированные сиды)

- RNG: `generate(v) ∈ [0, v)`, равномерность грубая (χ² по 10k выборок), монотонность seed-различия;
- pathfinding: путь валиден (каждый шаг — сосед, поверхности допустимы), детерминирован (двойной прогон = один путь), `PathFill` — связность области;
- `SmartList`: инварианты после случайных последовательностей операций (сравнение с эталонным `Vec`-моделированием);
- урон: `0 ≤ damage`, монотонность по атаке/броне (где C++ монотонен);
- экономика: комплексы не уходят в отрицательные запасы.

### 8.5 Дифференциальное фаззинг (P5+, опционально)

- маленькие случайные скрипты (2–4 юнита, 1–3 хода) → сравнение дампов Rust/C++ (как §8.2.3); при расхождении — минимизация скрипта (ddmin);
- раннер C++ собирается в CI (Linux x64 — эталонная платформа).

### 8.6 CI и линты

- `cargo test` всех фаз; nightly + `miri` на симуляционном подмножестве (проверка отсутствия UB);
- `#![forbid(unsafe_code)]` в max_core (пока не доказана необходимость);
- `#![deny(clippy::float_arithmetic)]` в модулях сима;
- запрет `std::collections::HashMap`-итерации — clippy-кастомный lint или code-review правило + grep в CI.

---

## 9. Риски и открытые вопросы

| # | Риск/вопрос | Влияние | План/митигация |
|---|---|---|---|
| 1 | **Float в C++-порта**: `researchmenu.cpp:113-122` (`std::pow(base+0.1, 7.5) * 256.0 / factor`) и `upgradecontrol.cpp:219-222` (`std::pow(...,7.5) * Factors[id]`) — состояние сима (ходы исследований, стоимость апгрейдов) | высокое: без точного воспроизведения — десинк | **build-time-таблица**: `build.rs` компилирует микро-C++-программу из формул порта и генерирует `research_table.rs`/`upgrade_table.rs` (целые результаты для достижимой области `research_level × factor`); на случай отсутствия C++-тулчейна — закоммиченная таблица; выборочная верификация против живого C++. Доп. риск: `std::pow` не бит-стабилен между libm — фиксируем эталон = Linux x64 сборка порта |
| 2 | **Pathfinding многопоточный** в порту (`path_worker.hpp`, `DispatchJobs`, `std::unordered_map m_dispatched_requests`): порядок применения результатов = порядок завершения воркеров | среднее: порядок применения путей может влиять на состояние | max_core применяет результаты **в порядке постановки запросов**; на P5 доказываем (тестами на C++ в однопоточном режиме/при `#if !defined(NDEBUG)`-таймер-моде), что порядок применения независимых путей не влияет на состояние; иначе — расхождение документируем как осознанное (наш lockstep — полностью Rust) |
| 3 | `UnitsManager_LiveAttacks` — `std::unordered_map<uint32_t, SmartList<UnitInfo>>` (`units_manager.cpp:197`) | среднее: если C++ где-то итерирует её, порядок нестабилен даже у оракула | P5: аудит использования (ожидается только lookup по `live_attack`); Rust — lookup-map + упорядоченный Vec для обхода |
| 4 | **Тайм-бюджеты ИИ**: `ExecuteReminders` «до исчерпания бюджета кадра», `TickTimer_HaveTimeToThink` | среднее: разное число напоминаний за тик у разных пиров | max_core обрабатывает очередь полностью за тик (детерминированно); расхождение с C++ возможно только в «нагруженных» кадрах — проверяется в P6 сценарием с массовым ИИ; при необходимости — режим «N напоминаний за тик» с N из конфигурации, синхронизированной через барьер |
| 5 | Маска видимости ресурсов: save.md `0x1C00` не накрывает `RED=0x2000` (state-model §7.3) | низкое | принять фактическую `0x3C00`; сверить по оригиналу (RE); зафиксировать в тестах |
| 6 | `ORDER_17` (0x12) и невосстановленные V70-поля `comm/fuel_distance/energized` (state-model §7.6) | низкое | порт держит заглушку — Rust держит заглушку; для сейв-совместимости — сохранение байтов как есть |
| 7 | Типы V70 ≠ типы порта (`hits/speed` u8 vs u16, `gold` i16 vs u32, `recoil_delay` i8 → пара u8) | среднее | канон сима = семантика порта; V70-конвертация — в `save.rs` с тестами round-trip (state-model §7.7) |
| 8 | **Lua-скрипты миссий** (`scripter.cpp`, `gameruleshandler.cpp`) | высокое для кампаний | max_core не встраивает Lua. Generic-правила и жёстко закодированные миссии — таблицами данных; скриптовые миссии — «предкомпилированный» байткод правил или внешний движок (max_app/Horizon), который дёргает чистые предикаты max_core. Область P7 для кампаний уточняется |
| 9 | Схема id юнитов: глобальный `UnitIdCounter` (порт) vs per-team `number_of_objects_created` (V70) (state-model §7.8) | низкое | воспроизводим порт: глобальный счётчик; при загрузке V70 — старт `PLAYER_TEAM_MAX << 13`; тест round-trip |
| 10 | Таймеры хода/конца хода — wall-clock в оригинале (`Remote_SyncTurnTimer`, `timer_get`) | среднее | в max_core — явные тики времени от сети (сервер — авторитет); поведение при паузе/таймауте — на сетевом слое |
| 11 | `Randomizer_Init/SetSeed(time)` в локальных режимах | низкое | не портируется; seed всегда входит |
| 12 | Порядок `SmartList::Get(index)`: C++ идёт с начала или с конца списка | отсутствует (логический результат тот же) | портируем эквивалент для O(1)-хвоста необязательно; важно только значение |
| 13 | `score_graph` — сдвиг `memmove` на 1 | низкое | `VecDeque`-эквивалент или `copy_within`; тест раскладки |
| 14 | Чередование `GameManager_UpdateFlag` для `Ai_CheckReactions` | низкое | переносится как поле `update_flag: bool` в `GameState` |
| 15 | Деление/остаток на 0 в боевых формулах | низкое | C++ UB/особые случаи; каждый сайт аудируется и портируется с теми же ветками (P5) |
| 16 | Семантика `UnitInfo::storage` = «опыт» у COMMANDO/REGENERATING (state-model §3.9) | низкое | отдельные аксессоры `experience()/storage()` как в C++ |
| 17 | Производительность: полный обход 5 списков каждый тик + AI-задачи | среднее | замер на сценарии 4×AI×1000 юнитов; оптимизации только сохраняющие порядок (индексы, кэши) |
| 18 | Воспроизводимость `std::sort`-эквивалентов: C++ `SmartList::Sort` — quicksort с конкретной схемой разбиения | среднее | порт точно этого quicksort (Lomuto, pivot = последний), не `sort_unstable` из std |
| 19 | `Ai_CheckReactions` лимиты/флаги (`tasks_pending`, `end_turn_retry_counter` с шагами 10/20/30/60/120) | низкое | порт констант как есть (`aiplayer.cpp:2839-2883`) |
| 20 | Меню/скрипты задают `ini_setting_*` (victory_type/limit и пр.) | низкое | `GameState.setup: Setup` — неизменяемый блок настроек (state-model §3.1/3.2) |

---

## 10. Итоговые ключевые решения (резюме)

1. **Оракул = max-cpp-port** (не оригинальный бинарник): где порт разошёлся с V70 (`hits/speed u16`, `gold u32`, `recoil → пара полей`), канон сима — порт; V70 — только слой `save.rs`.
2. **RNG**: Xoshiro128** + SplitMix64, состояние `[u32;4]`, порт 1:1; `Generate` двумя ветками (i64 / u64 для `sizeof`-сайтов); seed только из сети.
3. **Коллекции**: `SmartList`-эквивалент (порядок вставки) — единственная итерируемая коллекция юнитов/задач; `HashMap` — только lookup; хэш-таблицы юнитов — упорядоченные ведёрки как в `hash.cpp`.
4. **Без float в runtime**: `std::pow`-формулы исследований/апгрейдов — build-time-таблицы, сгенерированные из C++ (риск 1).
5. **Без времени и потоков**: тики времени — вход; pathfinding — синхронный, порядок = очередь запросов; бюджеты ИИ — детерминированная полная обработка.
6. **Совместимость с каркасом**: сохранены `TeamId/UnitId/TeamKind/MapSize/Point`-идиомы; заменены `UnitKind`→`UnitType`, `Surface`→битмаска, `Map.resources`→`ResourceMapEntry`, `Unit`→полный `UnitInfo`; `order::Order` остаётся входным API, машина приказов — `unit_order.rs`.
7. **Слои портирования**: P0 типы → P1 RNG+векторы → P2 карта → P3 юниты/экономика → P4 приказы/ход → P5 бой/пути → P6 ИИ → P7 победа/сквозные сейв-тесты; каждая фаза закрывается сравнением с эталонами, генерируемыми из C++ (генератор векторов/карты + `reference_runner` для скрипт-реплеев).
