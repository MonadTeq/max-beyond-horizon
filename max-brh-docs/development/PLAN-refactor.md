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
| **Ф2** | Маркерные типажи + классифицирующие enum (вид над флагами) | низкий–средний |
| **Ф3** | Миграция логики с сырых флагов на типажи/enum (151+ мест) | средний |
| **Ф4** | Вынос эффектов/декораций из логического юнит-классификатора (`ObjectKind`) | высокий |
| **Ф5** | Разрешение слоёв (rule 4↔5) и вынос UI/V70-данных из `UnitInfo` | от решений Ф0 |

Стратегия Ф4: `ResourceId` (сырой `u16`) остаётся для CRC/V70; поверх —
`ObjectKind { Unit / Effect / Decoration / PlotProp }` с тотальной биекцией
0x00–0x5C; таблицы индексируются по сырому id.

Стратегия имён (Ф1): код = Rust-имя; ресурсный токен = `fn tag()`; User-Friendly
Name = данные вне `max_core` (рекомендуется `max_assets`/`max_app`).

## 3. Эскалации владельцу (Ф0 — блокируют старт)

1. **Авторитет имён:** `domain-map.md` vs `unit-name-review.md` расходятся по
   `SCANNER` (`Scanner`/`MobileScanner`), `ARTYTRRT` (`ArtilleryTurret`/`Artillery`),
   `ANTIMSSL` (`AntiMissile`/`MissileLauncher`), `COMMANDO` (`Commando`/`Infiltrator`).
   Какой документ канон?
2. **Открытые имена:** `SHIELDGN`/`SUPRTPLT`/`RECCENTR` (C++ `ALIEN_BUILDING_1/2/3`);
   UFN для `BOMBER` (решено «Ground Attack Plane»), `SEATRANS`/`CARGOSHP`
   (коллизия «Supply Boats» в списке верфи).
3. **Rule 4 ↔ Rule 5:** `max_net`/`max_assets` зависят от `max_core`. Варианты:
   (а) считать их Application-слоем; (б) выделить общий нижний crate в
   Infrastructure. Рекомендация Архитектора — (б).
4. **UI/V70-данные в `UnitInfo`** и место UFN-таблицы (вынести из `max_core` или
   оставить как «чистые данные»). Рекомендация — вынести.
