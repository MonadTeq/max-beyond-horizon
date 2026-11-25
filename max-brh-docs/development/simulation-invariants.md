# Каталог симуляционных инвариантов — минимальное ядро (экономика + бой)

Версия 0.1 · 2026-11-25 · Аналитик

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
| E1 | Экономика / производство | Завод (`SHIPYARD/LIGHTPLT/LANDPLT/TRAINHAL/AIRPLT`) списывает `raw` **только** когда приказ `ORDER_BUILD` и состояние ≠ `ORDER_STATE_UNIT_READY`; при `ORDER_POWER_OFF`, `ORDER_HALT_BUILDING`, `ORDER_DISABLE` или `UNIT_READY` списание равно 0. Падение: «factory in state <state> consumed <N> raw». | `max-cpp-port/src/cargo.cpp:88` | unit + property |
| E2 | Экономика / потребление | Здания `COMMTWR/POWERSTN/POWGEN/HABITAT/RESEARCH/GREENHSE` списывают ресурсы (fuel/power/life/gold) **только** при `ORDER_POWER_ON`; в ином приказе — нулевое потребление. Падение: «building <type> consumed resources in order <order>». | `max-cpp-port/src/cargo.cpp:99` | unit + property |
| E3 | Экономика / добыча | `MININGST` начисляет добычу только при `ORDER_POWER_ON` или `ORDER_NEW_ALLOCATE`, причём `gold/raw/fuel = (mining_rate × difficulty) / 4`, `difficulty ∈ {4,5,6}` (5 — Master, 6 — God для компьютера). Падение: «mining output <got> ≠ rate×difficulty/4 <expected>». | `max-cpp-port/src/cargo.cpp:106-121` | unit + property |
| E4 | Экономика / добыча | Свободная мощность одной станции `free_capacity = 16 − total_mining`: суммарная добыча станции за ход не превышает 16. Падение: «mine total_mining <N> exceeds cap 16». | `max-cpp-port/src/cargo.cpp:122` | unit |
| E5 | Экономика / комплекс | Ресурсы комплекса агрегируются строго по принадлежности: юнит добавляет `inventory + net_production` только в **свой** комплекс (`GetComplex() == complex`); юниты чужого комплекса не влияют на баланс. Падение: «complex balance polluted by unit of another complex». | `max-cpp-port/src/access.cpp:921-933` | integration |
| E6 | Экономика / энергия | Генератор (`POWGEN/POWERSTN`) не включается, если резерв топлива меньше его нормы расхода (`Cargo_GetFuelConsumptionRate > total.fuel`); нормы: `POWGEN=2`, `POWERSTN=6`. Падение: «power plant <type> powered on without fuel reserve». | `max-cpp-port/src/production_manager.cpp:335`, `cargo.cpp:187-197` | unit |
| E7 | Экономика / баланс | После `ProductionManager_OptimizeProduction` (не-remote команда) баланс комплекса неотрицателен: цикл оптимизации завершается только при `total.{raw,fuel,gold,power,life} ≥ 0`, либо исчерпав все отключения (ранний выход). Падение: «complex deficit <field>=<N> after optimization». | `max-cpp-port/src/production_manager.cpp:847-886` | integration |
| C1 | Бой / реакция (sentry) | Реакция (в т. ч. `ORDER_SENTRY`) срабатывает только при `shots > 0`, цели в пределах `ATTRIB_RANGE` (`Access_IsWithinAttackRange`), допустимой цели (`Access_IsValidAttackTarget`) и `ShouldAttack`. Вне диапазона или без выстрелов — реакции нет. Падение: «unit reacted out of range / with 0 shots». | `max-cpp-port/src/units_manager.cpp:4712-4715` | unit + integration |
| C2 | Бой / видимость | Реакция/атака требуют видимости цели: `ShouldAttack` возвращает false, если `!target->IsVisibleToTeam(attacker->team)`. Невидимая цель не провоцирует реакцию. Падение: «unit reacted to an invisible target». | `max-cpp-port/src/units_manager.cpp:4643` | unit + integration |
| C3 | Бой / стелс | Подводная лодка (`SUBMARNE`, всегда под водой) невидима обычному скану: без морского стелс-покрытия (`GetStealthSea == 0`, даёт только `CORVETTE`) `DrawStealth` не открывает её команде; класс цели — `TARGET_CLASS_SEA_STEALTH`, пока не засвечена. Падение: «submarine revealed by ordinary scan». | `max-cpp-port/src/unitinfo.cpp:2877-2880`, `access.cpp:609-621`, `units_manager.cpp:1997-1998` | unit + integration |
| C4 | Бой / боезапас | Каждый выстрел расходует боезапас: `PrepareFire` декрементирует `ammo` (всегда) и `shots` (в пошаговом режиме). После выстрела `shots` монотонно не возрастает. Падение: «shots/ammo not decremented on fire». | `max-cpp-port/src/unitinfo.cpp:4805,4813` | unit |
| C5 | Бой / движение | Воздушный юнит тратит `speed` за каждый шаг пути: `Paths_TakeStep` выполняет `unit->speed -= cost` (и пересчитывает `shots` для не-`MOVE_AND_FIRE`). Падение: «air unit moved without spending speed». | `max-cpp-port/src/airpath.cpp:463` | unit |
| C6 | Бой / урон | Урон атаки ограничен снизу единицей и сверху разностью: при `armor ≥ attack` урон = 1, иначе `attack − armor`; броня никогда не делает урон нулевым. Падение: «damage <0 or != max(1, attack−armor)». | `max-cpp-port/src/units_manager.cpp:4972-4977` | unit + property |
| C7 | Бой / гибель | `AttackUnit` зажимает урон по `hits` (`hits = max(0, hits − damage)`), `hits` не уходит в отрицание; при `hits == 0` юнит переходит в `ORDER_EXPLODE`. Падение: «hits negative / destroyed unit not in ORDER_EXPLODE». | `max-cpp-port/src/unitinfo.cpp:1566-1570,1614-1616` | unit |

## Почему выбраны именно эти 14 первыми

1. **Покрытие обеих критичных осей.** Экономика (E1–E7) и бой (C1–C7) — две
   подсистемы, где регрессия ломает игровой баланс целиком; остальные подсистемы
   (пути, интерфейс, сеть) зависят от этих двух и покрываются позже.

2. **Каждая строка — безусловная гарантия оракула, а не наблюдение.** Свойства
   сформулированы по конкретным веткам (`if`/формуле), а не по «найденному
   дефекту»: регресс в порте даёт детерминированно падающий тест с точным
   сообщением, а не «примерно похожее» поведение.

3. **Минимальная связность → максимальная ценность.** E1–E4 и C1, C4, C6 — чистые
   функции (`Cargo_GetNetProduction`, `UnitsManager_CheckReaction`,
   `UnitsManager_GetAttackDamage`, `PrepareFire`), их дёшево тестировать unit-
   тестами без подъёма `GameUniverse`; E5, E7 и C2, C3 — интеграционные, они
   ловят ошибки композиции (агрегация по комплексу, баланс после оптимизации,
   стелс-карты).

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
