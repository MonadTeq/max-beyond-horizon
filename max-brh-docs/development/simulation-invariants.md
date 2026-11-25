# Каталог симуляционных инвариантов — минимальное ядро (экономика + бой)

Версия 0.2 · 2026-11-25 · Аналитик (каталог), Разработчик (статус реализации)

Минимальное ядро проверяемых свойств, гарантируемых **Оракулом Klei**
(`max-cpp-port/`). Свойства сформулированы не из найденных дефектов, а из кода
оракула: каждая строка — конкретная ветка/формула, которую оракул гарантирует
безусловно. Ссылка `файл:строка` указывает на строку, где гарантия закреплена.

Правило отбора (см. `test-organization.md`, A5): **нет свойства — нет теста**;
каждая строка каталога обязана быть проверяемой property/unit/integration-тестом
с понятным сообщением падения. Заметки без проверяемого инварианта сюда не
входят.

Имена в таблице — идентификаторы оракула (`ORDER_BUILD`, `MININGST`, `SUBMARNE`
и т. д.); DSL-соответствия ведутся в `domain-map.md` (правило 1 аналитика).

## Каталог (14 свойств)

| # | Подсистема | Свойство (что должно выполняться; иначе — падение теста) | Оракул (файл:строка) | Тест |
|---|---|---|---|---|
| E1 | Экономика / производство | Завод (`SHIPYARD/LIGHTPLT/LANDPLT/TRAINHAL/AIRPLT`) списывает `raw` **только** когда приказ `ORDER_BUILD` и состояние ≠ `ORDER_STATE_UNIT_READY`; при `ORDER_POWER_OFF`, `ORDER_HALT_BUILDING`, `ORDER_DISABLE` или `UNIT_READY` списание равно 0. Падение: «factory in state <state> consumed <N> raw». | `max-cpp-port/src/cargo.cpp:88-95` | property |
| E2 | Экономика / потребление | Здания `COMMTWR/POWERSTN/POWGEN/HABITAT/RESEARCH/GREENHSE` списывают ресурсы (fuel/power/life/gold) **только** при `ORDER_POWER_ON`; в ином приказе — нулевое потребление. Падение: «building <type> consumed resources in order <order>». | `max-cpp-port/src/cargo.cpp:99-105` | property |
| E3 | Экономика / добыча | `MININGST` начисляет добычу только при `ORDER_POWER_ON` или `ORDER_NEW_ALLOCATE`, причём `gold/raw/fuel = (mining_rate × difficulty) / 4`, `difficulty ∈ {4,5,6}` (5 — Master, 6 — God для компьютера). Падение: «mining output <got> ≠ rate×difficulty/4 <expected>». | `max-cpp-port/src/cargo.cpp:106-124` | property |
| E4 | Экономика / добыча | Свободная мощность одной станции `free_capacity = 16 − total_mining`: суммарная добыча станции за ход не превышает 16. Падение: «mine total_mining <N> exceeds cap 16». | `max-cpp-port/src/cargo.cpp:122`, `allocmenu.cpp:485-500` | property ×2 (формула + cap) |
| E5 | Экономика / комплекс | Ресурсы комплекса агрегируются строго по принадлежности: юнит добавляет `inventory + net_production` только в **свой** комплекс (`GetComplex() == complex`); юниты чужого комплекса не влияют на баланс. Падение: «complex balance polluted by unit of another complex». | `max-cpp-port/src/access.cpp:912-934` | property/integration |
| E6 | Экономика / энергия | Генератор (`POWGEN/POWERSTN`) не включается, если резерв топлива меньше его нормы расхода (`Cargo_GetFuelConsumptionRate > total.fuel`); нормы: `POWGEN=2`, `POWERSTN=6`. Падение: «power plant <type> powered on without fuel reserve». | `max-cpp-port/src/production_manager.cpp:332-352`, `cargo.cpp:183-197` | property (границы резерва) |
| E7 | Экономика / баланс | После `ProductionManager_OptimizeProduction` (не-remote команда) баланс комплекса неотрицателен: цикл оптимизации завершается только при `total.{raw,fuel,gold,power,life} ≥ 0`, либо исчерпав все отключения (ранний выход). Падение: «complex deficit <field>=<N> after optimization». | `max-cpp-port/src/production_manager.cpp:830-901` | property/integration (границы баланса) |
| C1 | Бой / реакция (sentry) | Реакция (в т. ч. `ORDER_SENTRY`) срабатывает только при `shots > 0`, цели в пределах `ATTRIB_RANGE` (`Access_IsWithinAttackRange`), допустимой цели (`Access_IsValidAttackTarget`) и `ShouldAttack`. Вне диапазона или без выстрелов — реакции нет. Падение: «unit reacted out of range / with 0 shots». | `max-cpp-port/src/units_manager.cpp:4709-4735` | property + unit |
| C2 | Бой / видимость | Реакция/атака требуют видимости цели: `ShouldAttack` возвращает false, если `!target->IsVisibleToTeam(attacker->team)`. Невидимая цель не провоцирует реакцию. Падение: «unit reacted to an invisible target». | `max-cpp-port/src/units_manager.cpp:4643-4644` | property + unit |
| C3 | Бой / стелс | Подводная лодка (`SUBMARNE`, всегда под водой) невидима обычному скану: без морского стелс-покрытия (`GetStealthSea == 0`, даёт только `CORVETTE`) `DrawStealth` не открывает её команде; класс цели — `TARGET_CLASS_SEA_STEALTH`, пока не засвечена. Падение: «submarine revealed by ordinary scan». | `max-cpp-port/src/unitinfo.cpp:2872-2884`, `access.cpp:609-621`, `units_manager.cpp:1994-1998` | property ×2 + unit |
| C4 | Бой / боезапас | Каждый выстрел расходует боезапас: `PrepareFire` декрементирует `ammo` (всегда) и `shots` (в пошаговом режиме). После выстрела `shots` монотонно не возрастает. Падение: «shots/ammo not decremented on fire». | `max-cpp-port/src/unitinfo.cpp:4805,4813` | unit |
| C5 | Бой / движение | Воздушный юнит тратит `speed` за каждый шаг пути: `Paths_TakeStep` выполняет `unit->speed -= cost` (и пересчитывает `shots` для не-`MOVE_AND_FIRE`); `GameManager_RealTime` делает шаг бесплатным, `group_speed` не ниже 1. Падение: «air unit moved without spending speed». | `max-cpp-port/src/airpath.cpp:458-484` | unit |
| C6 | Бой / урон | Урон атаки `damage = max(1, effective_attack − armor)`, где `effective_attack = ((5 − attack_potential) × attack) / 4` при `attack_potential > 1`, иначе `attack`; против `SUBMARNE` от `BOMBER`/`ALNPLANE` — `effective_attack /= 2`. Броня никогда не делает урон нулевым. Падение: «damage <1 or != max(1, effective_attack−armor)». | `max-cpp-port/src/units_manager.cpp:4958-4985` | property |
| C7 | Бой / гибель | `AttackUnit` зажимает урон по `hits` (`hits = max(0, hits − damage)`), `hits` не уходит в отрицание; при `hits == 0` юнит переходит в `ORDER_EXPLODE`. Оракул также выставляет `ORDER_EXPLODE` любому повреждённому юниту вне `EXPLODE`/`DESTROY`; тест проверяет только нулевой случай. Падение: «hits negative / destroyed unit not in ORDER_EXPLODE». | `max-cpp-port/src/unitinfo.cpp:1566-1570,1614-1616` | unit |

## Статус реализации

Все 14 инвариантов реализованы. Property-тесты генерируют состояния и
граничные значения (детерминированный LCG + полные таблицы приказ/состояние);
unit-тесты лежат в `#[cfg(test)]` рядом с приватными функциями. Негативная
проверка: 17 временных мутаций прод-кода (по одной на проверяемую ветку) —
каждый соответствующий тест падал; прод-код восстановлен. Реальных дефектов
инварианты не вскрыли: `cargo nextest run -p max_core --no-default-features` —
940/940 passed.

| # | Статус | Тест (файл::имя) |
|---|---|---|
| E1 | реализован | `max-brh/crates/max_core/tests/invariants_economy_property.rs::e1_factory_raw_consumption_requires_build_and_not_unit_ready` |
| E2 | реализован | `max-brh/crates/max_core/tests/invariants_economy_property.rs::e2_power_buildings_consume_only_when_powered_on` |
| E3 | реализован | `max-brh/crates/max_core/tests/invariants_economy_property.rs::e3_mining_output_follows_order_and_difficulty` |
| E4 | реализован | `.../invariants_economy_property.rs::e4_free_capacity_is_sixteen_minus_total_mining`; `.../e4_allocation_saturates_at_sixteen` |
| E5 | реализован | `.../invariants_economy_property.rs::e5_complex_balance_is_isolated_by_complex_membership` |
| E6 | реализован | `.../invariants_economy_property.rs::e6_power_plant_requires_its_fuel_reserve` |
| E7 | реализован | `.../invariants_economy_property.rs::e7_optimization_leaves_no_negative_balance` |
| C1 | реализован | `.../invariants_combat_property.rs::c1_reaction_requires_shots_range_and_a_valid_target`; `max-brh/crates/max_core/src/combat.rs::tests::invariant_c1_reaction_needs_shots_and_range` |
| C2 | реализован | `.../invariants_combat_property.rs::c2_invisible_target_does_not_trigger_reaction`; `max-brh/crates/max_core/src/combat.rs::tests::invariant_c2_reaction_needs_target_visibility` |
| C3 | реализован | `.../invariants_combat_property.rs::c3_submarine_is_hidden_from_an_ordinary_scan`; `.../c3_corvette_sea_stealth_reveals_the_submarine`; `max-brh/crates/max_core/src/access.rs::tests::invariant_c3_submarine_target_class_is_sea_stealth_until_spotted` |
| C4 | реализован | `max-brh/crates/max_core/src/combat.rs::tests::invariant_c4_prepare_fire_spends_ammo_and_shots` |
| C5 | реализован | `max-brh/crates/max_core/src/unit_order.rs::tests::invariant_c5_air_step_spends_speed` |
| C6 | реализован | `.../invariants_combat_property.rs::c6_damage_is_never_below_one` |
| C7 | реализован | `max-brh/crates/max_core/src/combat.rs::tests::invariant_c7_hits_never_go_negative_and_zero_explodes` |

## Почему выбраны именно эти 14 первыми

1. **Покрытие обеих критичных осей.** Экономика (E1–E7) и бой (C1–C7) — две
   подсистемы, где регрессия ломает игровой баланс целиком; остальные подсистемы
   (пути, интерфейс, сеть) зависят от этих двух и покрываются позже.

2. **Каждая строка — безусловная гарантия оракула, а не наблюдение.** Свойства
   сформулированы по конкретным веткам (`if`/формуле), а не по «найденному
   дефекту»: регресс в порте даёт детерминированно падающий тест с точным
   сообщением, а не «примерно похожее» поведение.

3. **Минимальная связность → максимальная ценность.** E1–E4 и C1, C4, C6 —
   чистые функции (`Cargo_GetNetProduction`, `UnitsManager_CheckReaction`,
   `UnitsManager_GetAttackDamage`, `PrepareFire`); их инварианты проверяются
   property-тестами на сгенерированных состояниях и границах, а C1/C2/C3, C4,
   C5, C7 — ещё и unit-тестами у приватных функций. E5, E7 и C2, C3 —
   интеграционные по природе: они ловят ошибки композиции (агрегация по
   комплексу, баланс после оптимизации, стелс-карты).

4. **Перечисленные в задании примеры закрыты явно.** «Выключенный/паузный завод»
   → E1; «станция от резерва» → E6; «добыча по приказу» → E3; «аллокация по
   комплексу» → E5; «списание по ORDER_BUILD, не UNIT_READY» → E1; «sentry в
   range+scan» → C1; «подлодка невидима обычному скану» → C3; «shots за выстрел» →
   C4; «air тратит speed» → C5; «реакция требует видимости» → C2.

5. **Инварианты замкнуты и не пересекаются.** Каждый тест проверяет ровно одно
   свойство с одной причиной падения; нет дублирующих пар, которые при регрессе
   падали бы «за компанию» и размывали диагностику.

## Границы ядра (что сознательно НЕ включено)

- Пути/поиск маршрута, save/load-детерминизм, golden/CRC, сеть — отдельные оси,
  не входят в минимальное ядро экономики и боя.
- Точные таблицы ставок (`Cargo_GetRawConsumptionRate` по типам юнитов) — это
  данные (golden-таблицы), а не инвариант; здесь фиксируются только сами
  гарантии-формулы (E3) и пороги (E4, E6).
- AI-стратегии и порядок обработки отрядов — не детерминированный инвариант,
  отдельная задача реверс-инженера.

## Покрытие ручных дефектов (ноябрь 2026)

4 дефекта ручного прогона — распределение по слоям:
- **Подлодки (стелс)** → C3 (в каталоге), property c3_submarine_is_hidden_from_an_ordinary_scan.
- **Бесконечные выстрелы** → C4 (в каталоге), unit invariant_c4_prepare_fire_spends_ammo_and_shots.
- **Радар AWAC/сканера (анимация)** → ПРЕЗЕНТАЦИЯ, не симуляционный инвариант;
  покрыт кадровыми тестами spinning_turret_animation.rs + golden, вне этого каталога.
- **Статы видимого врага** → ПРЕЗЕНТАЦИЯ/UI, покрыт enemy_unit_panel.rs, вне каталога.
