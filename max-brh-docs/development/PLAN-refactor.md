# План рефакторинга BRH под новые стандарты

Результат gap-анализа (Аналитик v4 Pro) и плана рефакторинга (Архитектор v4 Pro).
Источник канонов: `rust-programming-rules.md`, `architect-rules.md`,
`object-classifier.md`, `domain-map.md`, `unit-name-review.md`.

## 1. Gap-анализ (Аналитик)

### 1.1 Имена `UnitType` — 14 переименований не выполнены

В `enums.rs` остались старые имена (канон — `unit-name-review.md` «Переименования»):
`CommTower→GoldRefinery`, `ADump→MaterialStorage`, `FDump→FuelStorage`,
`GoldSmelter→GoldVault`, `ResearchCenter→ResearchCentre`, `Greenhouse→EcoSphere`,
`ArtilleryTurret→Artillery`, `AntiMissile→MissileLauncher`, `Block→ConcreteBlock`,
`PersonnelCarrier→ArmouredPersonnelCarrier`, `Commando→Infiltrator`,
`FastBoat→Escort`, `Battleship→Gunboat`, `MissileBoat→MissileCruiser`.

> Расхождение канонов: `domain-map.md` и `unit-name-review.md` расходятся по
> `SCANNER`, `ARTYTRRT`, `ANTIMSSL`, `COMMANDO` — требует решения владельца (§3.1).

### 1.2 Классификация — полностью на битовых флагах

- `unit_flags` (`enums.rs:852-909`) — «голые» `u32`-константы; типобезопасного слоя
  (enum/marker-traits) нет. Прямые проверки `flags & MOBILE_AIR_UNIT` и т.п. —
  151+ вхождений в логике (`access.rs`, `production.rs`, `victory.rs`, `ai/*` и др.).
- Маркерные типажи `AirUnit/GroundUnit/SeaUnit/HoveringUnit/Combat/NonCombat/
  Stationary/Mobile/Building/Producing/NonProducing` — **отсутствуют полностью**.
- **23 не-юнита** (эффекты/декорации/сюжет) сидят в `UnitType` (93 варианта):
  взрывы, снаряды, обломки, следы, ленты/плиты/конусы, `MasterBuilder/Harvester/
  DeadWaldo` — нарушение «эффект не юнит».

### 1.3 Слои

- Правило 4 — **нарушений нет**: `max_core`/`max_net`/`max_assets` не зависят от
  `max_render`/`max_app`.
- Правило 5 — формальное нарушение: `max_net → max_core`, `max_assets → max_core`
  (Infrastructure → Application вверх).
- UI-данные в прикладном слое: `UnitInfo` несёт презентационные поля (`x/y`,
  `sprite_bounds`, `turret_*`, `image_*`, `brightness`, `sound`, `scaler_adjust`,
  `Rect`, `frames`) — ради побайтного V70-round-trip.

### 1.4 Детерминизм/паритет (что нельзя ломать)

`flags: u32` входит в `replay_digest_bytes` (`crc.rs:291`) и в `state_digest`;
биты `EXPLODING`/`STATIONARY` влияют на выбор полей CRC45 (`crc.rs:147,158`).
Дискриминанты `UnitType` индексируют `UNIT_TYPE_PROPERTIES` и `BASE_UNIT_VALUES`.
`HASH_TEAM_*` — биты хэш-ведра, `BUILDING` — раскладка 2×2 в `MapHash`. Всё это —
жёсткие инварианты.

### 1.5 Прочее

`u8`-магические числа (`targeting_mode`, `enter_mode`, `ai_state_bits`) вместо
enum; `Surface` — ручная newtype-битмаска вместо `bitflags`; `UnitValues::
get_attribute(attribute: u8, value: i32)` повторяет C++-раскладку.

## 2. План рефакторинга (Архитектор)

Принцип (правило 2): меняем только внутреннюю структуру, поведение и CRC — 1:1.

| Фаза | Содержание | Риск |
|---|---|---|
| **Ф0** | Решения владельца (эскалации) — гейт | — |
| **Ф1** | Переименование `UnitType` (14) + `fn tag()` + `fn user_friendly_name()` | низкий |
| **Ф2** | Классифицирующие enum (`CombatRole`/`Mobility`/`Environment`/`CargoKind`) как вид над флагами; маркерные типажи НЕ используются (решение C) | низкий–средний |
| **Ф3** | Миграция логики с сырых флагов на enum-методы/поведенческие типажи (151+ мест) | средний |
| **Ф4** | Вынос эффектов/декораций из логического юнит-классификатора (`ObjectKind`) | высокий |
| **Ф5** | Разрешение слоёв (rule 4↔5) и вынос UI/V70-данных из `UnitInfo` | от решений Ф0 |

Стратегия Ф4: `ResourceId` (сырой `u16`) остаётся для CRC/V70; поверх —
`ObjectKind { Unit / Effect / Decoration / PlotProp }` с тотальной биекцией
0x00–0x5C; таблицы индексируются по сырому id.

Стратегия имён (Ф1): код = Rust-имя; ресурсный токен = `fn tag()`; User-Friendly
Name = данные вне `max_core` (рекомендуется `max_assets`/`max_app`).

## 3. Решения владельца (Ф0)

1. **Авторитет имён — РЕШЕНО:** `unit-name-review.md` — канон; `domain-map.md`
   приведён в соответствие (`SCANNER=Scanner`, `ARTYTRRT=Artillery`,
   `ANTIMSSL=MissileLauncher`, `COMMANDO=Infiltrator`).
2. **Имена — РЕШЕНО (частично):** `SEATRANS` = «Sea Transport»,
   `CARGOSHP` = «Cargo Ship». Остаются открытыми `SHIELDGN`/`SUPRTPLT`/`RECCENTR`
   (C++ `ALIEN_BUILDING_1/2/3`) — не блокируют Ф1, решаются до Ф2/UFN.
3. **Rule 4 ↔ Rule 5 — РЕШЕНО (вариант б):** выделить общий нижний crate в
   Infrastructure (доменные типы `UnitType`/`ResourceId`/протокольные типы), от
   которого зависят `max_core`, `max_net`, `max_assets`. Уточнить имя crate и
   границу на Ф5.
4. **UI/V70-данные — РЕШЕНО:** вынести из `max_core` (презентационные поля
   `UnitInfo`, `Rect`, `frames`, `UnitSound`, UFN-таблица) — в отдельный слой
   представления/данных с сохранением побайтного V70-round-trip.

Ф0 закрыт → передать Разработчику пофазное ТЗ, начиная с Ф1.

## 4. Ф5.3 — граница `max_net → max_core` и `UnitPresentation` (выполнено)

### 4.1 Решение владельца: read-only зависимость (вариант 2)

`max_net → max_core` оставлена осознанно — только как **read-only проекция
симуляции в провод** (пакеты 23/45/53/54). Разрешены ровно пять функций:
`UnitState::from_unit`, `state_sync_for`, `state_sync_tail`, `crc_barrier_for`,
`complex_summary_for` — и импорты данных `max_core::unit::UnitInfo`,
`max_core::team::TeamUnits`. Вызов `apply_*` или мутация `GameState` из
`max_net` аннулируют решение. Замок:
`max-brh-cicd/scripts/check_maxnet_boundary.sh` (шаг 0/4 в `ci.sh`); пометки в
`wire/mod.rs`, `session.rs`, `max_net/Cargo.toml`.

### 4.2 B0 — верификация полей B.1.2

Правило: поле переносится, только если его не читает ни один сим-путь
(`snapshot_bytes`, `replay_digest_bytes`, CRC пакета 45, приказы/движение/
наведение/взрывы). Иначе оно остаётся в `UnitInfo` и фиксируется здесь.

| Поле | Вердикт | Обоснование |
|---|---|---|
| `x`, `y` | остались в `UnitInfo` | сим: `Access_IsWithinAttackRange` (линия огня, `access.rs`), попиксельный `AirPath` (`pathfinding.rs`), `unit_order::unit_move` пишет `x`/`y` |
| `shadow_offset` | → `UnitPresentation` | только `save.rs` + рендер; в симе/CRC нет |
| `brightness` | → `UnitPresentation` | только `save.rs` + рендер; в симе/CRC нет |
| `sound` | → `UnitPresentation` | только `save.rs`; звук приложения идёт событиями `UnitSound`, не полем |
| `sprite_bounds`, `shadow_bounds` | → `UnitPresentation` | только `save.rs` |
| `turret_offset_x`, `turret_offset_y` | → `UnitPresentation` | только `save.rs` (рендер берёт `turret_offset` из `D_*`) |
| `total_images` | → `UnitPresentation` | только `save.rs` |
| `firing_image_base`, `connector_image_base` | → `UnitPresentation` | только `save.rs` (рендер берёт те же поля из `FrameInfo` ассетов) |
| `cursor` | → `UnitPresentation` | только `save.rs`; UI ведёт `hud.cursor_mode` |
| `bobbed` | → `UnitPresentation` | только `save.rs` |

`x` значился в задании как презентационное, но B0 показал, что `x`/`y` —
симуляционные: оставлены в `UnitInfo` (список не «дотягивался»).

### 4.3 Реализация и гейт

* `max_types::unit::UnitPresentation` — только `Clone, Debug, PartialEq` +
  конструктор `new()`/`Default` (brightness 255 как в C++), без логики;
  `Rect` переехал в `max_types::unit` (реэкспорт `max_core::unit::Rect`
  сохранён).
* `GameState` держит `presentations: Vec<Option<UnitPresentation>>` параллельно
  слотам `units`: `presentation`/`presentation_mut`/`write_presentation`;
  спавн, загрузка и уничтожение синхронизируют таблицу.
* `save.rs::read_unit_body`/`write_unit_body` — единственный владелец
  V70-порядка: сим-поля из `UnitInfo`, презентационные из `UnitPresentation`;
  порядок байтов не менялся.
* Гейт: `save1/save2_round_trip_is_byte_identical` и
  `round_trip_is_byte_identical` зелёные; `base_values_match_real_v70_save`,
  `mini_replay_matches_cpp_digest_bit_for_bit`,
  `demo_save1_five_ticks_matches_reference_digest` зелёные; `snapshot_bytes`,
  `state_checksum` и `replay_digest` на SAVE1/SAVE2 до и после переноса — 0
  расхождений (`SAVE1: snapshot=0xfd67, checksum=0x408a, replay=0x5561`;
  `SAVE2: snapshot=0xd3f1, checksum=0x20eb, replay=0x4bc7`).
