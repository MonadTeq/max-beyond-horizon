# Аудит качества тестов — M.A.X.: Beyond Rust Horizon

- **Дата:** 2026-11-25
- **Роль:** QA
- **Область:** workspace `max-brh` (6 крейтов)
- **Статус:** только чтение; код, тесты и оракулы не изменялись, коммит не выполнялся.
- **Правила приёмки:** `max-brh-docs/development/test-organization.md` — Q1 (падает ли
  тест при регрессе) и Q6 (проверяет свойство или «вызвать и не упасть»).

---

## 1. Резюме

| Категория | Тестов | Доля от 2114 |
|---|---:|---:|
| Тривиальные (нет проверки свойства / только «не упасть») | 2 | 0.1 % |
| Дубликаты (8 групп) | 18 | 0.9 % |
| Устаревшие (TODO/удалённое/переименованное поведение) | 0 | 0 % |
| `#[ignore]` без обоснования | 0 | 0 % |
| `#[should_panic]` без проверки причины | 0 | 0 % |
| Always-green: пропуск до первой проверки (silent skip) | 251 | 11.9 % |
| Always-green: тавтологии и feature-ветка без проверки | 4 | 0.2 % |
| **Итого затронуто (уникальных тестов)** | **275** | **13.0 %** |

Главный вывод: содержательных «пустых» тестов мало (2), но **251 тест (11.9 %) может
завершиться до первой проверки** и при этом отчитывается как passed — в JUnit
`skipped = 0`. Это системный дефект приёмки по Q1, а не разовые ошибки. Дубликаты
(18 тестов) и тавтологии (4) — локальные; `#[ignore]`/`#[should_panic]` и маркеры
устаревания в наборе отсутствуют.

---

## 2. Инвентаризация

| Показатель | Значение |
|---|---:|
| `#[test]` + `#[tokio::test]` функций | 2114 |
| `#[rstest]` функций (разворачиваются в кейсы) | 10 |
| Кейсов в nextest (JUnit `nextest-run`) | **2153** |
| Файлов с тестами | 254 |
| Unit (`src/*_tests.rs`, `#[cfg(test)]`) | 1304 |
| Интеграционные (`tests/*.rs`) | 810 |

| Крейт | Тестов | Крейт | Тестов |
|---|---:|---|---:|
| `max_core` | 901 | `max_net` | 106 |
| `max_app` | 874 | `max_assets` | 87 |
| `max_render` | 113 | `max_types` | 33 |

Источник фактического прогона: `max-brh/target/nextest/ci/junit.xml`
(`tests=2153, failures=0, skipped=0`, timestamp 2026-11-25T17:53). Цифра «~1978» из
постановки не воспроизводится ни в одном из доступных отчётов; аудит покрывает все
2114 функций. Серверный workspace `max-brh-server` (отдельный workspace, 381 функция)
в эту ревизию не входил.

Размещение: 1304 unit-теста в `src/` и 810 интеграционных в `tests/` — в целом
соответствует таблице `test-organization.md`; отдельное наблюдение по Q2 —
фасадные сценарии `src/universe/scenario_tests/` (gate3/turn/universe/load_resume)
живут в `src/`, хотя по типу это интеграционные тесты (в 5 категорий не включены).

---

## 3. Метод

1. **Статический разбор** всех `#[test]`/`#[tokio::test]`: имя, файл, тело с
   балансировкой скобок (строки/комментарии/raw-строки учитываются), атрибуты.
2. **Эвристики Q1/Q6:**
   - отсутствие `assert!/assert_eq!/assert_ne!/panic!/unreachable!/expect_err` и
     helper-проверок (`round_trip`, `assert_send` и т.п.);
   - `assert!(true)`, сравнение литерала с собой, `x == x`, `matches!(x, _)`;
   - `if let Some/Ok … { assert }` без `else` (условная проверка);
   - `return`/`else { return; }` **до** первой проверки (пропуск);
   - тавтологии вида `assert_eq!(Literal, Literal)` и `assert_eq!(expr(), expr())`;
   - дубликаты: точный хэш тела после нормализации, `difflib ≥ 0.82` внутри файла,
     пересечение assert-утверждений и токенов имён между файлами, затем ручная сверка.
3. **Ручной разбор** ~40 подозрительных тестов (все тривиальные, все 8 групп
   дубликатов, тавтологии, выборка silent skip из 10 файлов, 2 теста с условным
   `assert`).
4. **Сверка с инфраструктурой:** `cargo-nextest` JUnit, `.config/nextest.toml`,
   `.github/workflows/ci.yml`, `git ls-files` по файлам данных, README/`build.md`.

Ограничение: семантическую устарелость («тест проверяет уже не то поведение») нельзя
доказать статически без сверки с оракулом; здесь фиксируются только доказуемые
признаки (маркеры, мёртвые пути данных, тавтологии).

---

## 4. Результаты по категориям

### 4.1. Тривиальные тесты — 2

| Тест | Файл:строка | Категория | Обоснование | Действие |
|---|---|---|---|---|
| `select_starting_position_is_within_map` | `max-brh/crates/max_core/src/ai/ai_tests.rs:59` | тривиальный | Тело: `GameState::new()`, вызов `ai_select_starting_position`, `let _ = world.rng.state();`. Ни одного `assert`; имя обещает «в пределах карты», но позиция не проверяется. Тест проходит, если функция не паникует. Q6: «вызвать и не упасть». | усилить: проверить `is_inside_map`/допустимую зону старта (или удалить как дубль `ai_*` сценариев) |
| `enum_roundtrip_is_exhaustive` | `max-brh/crates/max_types/src/enums_tests.rs:168` | тривиальный | Цикл `0..=u8::MAX` и `let _ = TeamKind::try_from(raw);` для 16 enum'ов: результат отбрасывается, проверок нет; ловится только паника. Round-trip уже проверяется с `assert_eq!` в `all_u8_enums_roundtrip`/`all_enums_roundtrip_u16`. | усилить: `assert!(T::try_from(raw).is_ok())`/сверить `COUNT`; иначе удалить |

Остальные 11 кандидатов «без assert» — ложные срабатывания: `round_trip(...)`
(`wire_tests.rs:14`), `run_build_cycle(...)` (`build_lifecycle_interactive.rs:171`),
`roundtrip_u8/u16` (`enums_tests.rs:4,15`) сами содержат `assert!`; `assert_send::<T>()`
и `assert_serialize::<T>()` — compile-time «замки» (D7), регрессия ловится сборкой.

### 4.2. Дубликаты — 8 групп / 18 тестов

| Тест(ы) | Файл:строка | Категория | Обоснование | Действие |
|---|---|---|---|---|
| `corvette_sea_stealth_coverage_reveals_the_submarine` ↔ `c3_corvette_sea_stealth_reveals_the_submarine` | `max-brh/crates/max_core/tests/submarine_stealth.rs:116` ↔ `max-brh/crates/max_core/tests/invariants_combat_property.rs:210` | дубликат | Один сценарий (Tank+Submarine на (10,10)/(12,10), Corvette на (14,10), `update_visibility_status`), одни и те же 3 проверки (`heat_map_stealth_sea > 0`, видимость RED, видимость GREEN). Property-версия — надмножество (6 смещений), одиночная не добавляет ничего, кроме `image_base == 8`. | удалить одиночный (при необходимости перенести `image_base` в property-тест) |
| `submarine_is_hidden_from_an_ordinary_scan` ↔ `c3_submarine_is_hidden_from_an_ordinary_scan` | `max-brh/crates/max_core/tests/submarine_stealth.rs:80` ↔ `max-brh/crates/max_core/tests/invariants_combat_property.rs:153` | дубликат | Те же 4 проверки (`ai_heat_map_complete > 0`, `stealth_sea == 0`, скрыт от RED, виден GREEN); property-тест повторяет их для 6 смещений. | удалить одиночный |
| `complex_transfer_negative_drains_members_in_order` ↔ `complex_transfer_negative_uses_buildings_in_stationary_order` | `max-brh/crates/max_core/src/transfer_tests.rs:131` ↔ `max-brh/crates/max_core/tests/transfer.rs:618` | дубликат | Одно свойство: `complex_transfer(..., -15)` списывает с двух `MaterialStorage` (по 10) в порядке стационарного списка → 0 и 5. Отличаются только способ сборки комплекса и наличие mine в интеграционной версии; ожидаемые значения те же. | оставить один (интеграционный), второй удалить/сократить до проверки `complex.raw` |
| `end_local_turn_and_submit_06_are_distinguishable` ↔ `end_local_turn_and_submit_06_are_distinguishable` | `max-brh/crates/max_core/src/universe/scenario_tests/gate3.rs:1062` ↔ `max-brh/crates/max_core/src/universe/scenario_tests/turn.rs:235` | дубликат | Одно имя и одно свойство: `end_local_turn` применяет 06 сразу и двигает машину, `submit(06)` только ставит в очередь. 4 идентичных assert; версия в `turn.rs` полнее (`round_team`, turn notices). | удалить копию в `gate3.rs`, оставить `turn.rs` (или слить в один сценарий) |
| `main_menu_opens_the_local_setup_and_escape_goes_back` ↔ `screen_routes_setup_to_start_local_and_back` | `max-brh/crates/max_app/src/ui_tests.rs:114` ↔ `max-brh/crates/max_app/tests/local_sp.rs:355` | дубликат (частичный) | Общее ядро: MainMenu → NewGameMenu → Custom Game → LocalSetup, два Esc назад; 3 идентичных assert. `local_sp` дополнительно доводит Players → PreGame. | оставить `local_sp`, из `ui_tests` убрать дублирующую маршрутизацию (оставить только специфичное для unit-уровня) |
| `render_map_into_paints_exactly_the_visible_tiles` ↔ `map_view_fills_the_whole_screen_with_visible_tiles` | `max-brh/crates/max_render/src/map_render_tests.rs:288` ↔ `max-brh/crates/max_render/tests/render_pure.rs:384` | дубликат (частичный) | Один и тот же Viewport 512×384 и одно и то же свойство `painted == range.count()`; различаются карта (uniform water vs sample) и цветовые проверки. | консолидировать: счётчик/покрытие — в одном тесте, цветовые проверки — отдельно; иначе оставить интеграционный |
| `parses_real_v70_save_completely` ↔ `save1_header_matches_original_file`; `save1_all_sections_are_present` | `max-brh/crates/max_core/src/save_tests.rs:9` ↔ `max-brh/crates/max_core/tests/v70_save.rs:25`, `max-brh/crates/max_core/tests/v70_save.rs:96` | дубликат (кластер из 3) | Один и тот же SAVE1.CAM: 6 идентичных assert по header/IniOptions с `save1_header…` и 20 идентичных — по teams/game/lists/hash с `save1_all_sections…`; интеграционные дополнительно проверяют отдельные поля (имена команд, таймеры), но ядро контракта фикстуры продублировано. | оставить один уровень (интеграционные как контракт фикстуры), unit сократить до структуры/парсера |
| `round_trip_is_byte_identical` ↔ `save1_round_trip_is_byte_identical`; `save2_round_trip_is_byte_identical` | `max-brh/crates/max_core/src/save_tests.rs:93` ↔ `max-brh/crates/max_core/tests/v70_save.rs:160`, `max-brh/crates/max_core/tests/v70_save.rs:259` | дубликат (кластер из 3) | Одно свойство: parse(SAVE1) → to_bytes() побайтово равен исходнику; интеграционная версия дополнительно гоняет SAVE2. | оставить интеграционный (2 фикстуры), unit удалить/сократить |

Примечание: перекрытие unit ↔ integration само по себе допустимо (разные уровни API),
но в перечисленных парах дублируются **те же константы фикстуры и те же ожидания**,
поэтому одна регрессия валит 2–3 теста, а сопровождение требует синхронной правки.
Пары в `invariants_combat_property.rs` — тот же уровень (`tests/`), т.е. классический
дубликат, а не «unit + E2E».

### 4.3. Устаревшие — 0

Проверено и не найдено:

- маркеров `TODO`/`FIXME`/`XXX`/`HACK`/`deprecated` в телах и doc-комментариях тестов — 0
  (единственное совпадение — историческая фраза `BuildNext/Build were TODOs` в
  `tests/path_build.rs:21`, тест актуален);
- `#[deprecated]`-элементов в `src/` — 0, ссылаться на удалённое не на что;
- строковых литералов-имён фикстур, которых нет в репозитории (кроме имён,
  собираемых тестом динамически), — 0;
- `legacy` в именах/комментариях (`gate3`, `turn`, `universe`) — это осознанная
  сверка фасада со старым путём (A/B), а не устаревший тест.

Оговорка: «семантически устаревший, но зелёный» тест статически неотличим от слабого;
такие случаи попадают в 4.5 (тавтологии/пропуски), а не в эту категорию.

### 4.4. `#[ignore]` и `#[should_panic]` — 0 и 0

- `#[ignore]` (в любой форме, включая `ignore = "…"`) — **0** на 2114 тестов.
- `#[should_panic]` — **0**.
- `#[cfg_attr(..., ignore)]` — **0**.
- `#[cfg(feature = …)]` — 2 теста (`main_tests.rs:198,214`), взаимоисключающие:
  в каждой конфигурации компилируется и выполняется ровно один (`sdl` / `not(sdl)`),
  это не пропуск, но при подсчёте по JUnit активна только одна ветка.

Это соответствует D6/Q1: игноров без backlog нет. Важно: `retries` в
`max-brh/.config/nextest.toml` (1 по умолчанию, 2 в `ci`, 3 для P2P-гейтов) — это не
`#[ignore]`; nextest помечает повтор как FLAKY, а не pass. Отдельный риск — флаки
P2P-гейтов, но это вне пяти категорий (Q3).

### 4.5. Always-green — 255

#### (а) Пропуск до первой проверки — 251 тест в 60 файлах

Типовой код (пример из `max-brh/crates/max_app/tests/build_site.rs:29-46`):

```rust
fn app_with_assets() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let patches = repo_path("assets/PATCHES.RES");
    if !max_res.is_file() || !patches.is_file() { return None; }   // нет данных — None
    ...
}
#[test]
fn some_ui_test() {
    let Some(mut app) = app_with_assets() else { return; };      // тест «прошёл», assert'ов не было
    ...
}
```

Обоснование категории: такой тест **не может упасть** в окружении без данных
(условие пропуска тривиально выполнено), т.е. по Q1 он не доказывает свойство, а по Q6
является «вызвать и не упасть» в ещё более слабой форме — он даже не вызывает.

| Тест (пример) | Файл:строка | Категория | Обоснование | Действие |
|---|---|---|---|---|
| `assets_integration_smoke` | `max-brh/crates/max_app/tests/app.rs:659` | always-green (условно) | `if !max_res.is_file() \|\| !patches.is_file() { eprintln!("пропуск"); return; }` до первого assert; при отсутствии `assets/PATCHES.RES` тест зелёный без проверок | явный skip с backlog / данные в CI |
| `played_game_saves_as_v70_and_loads_in_a_fresh_app` | `max-brh/crates/max_app/tests/save_load.rs:119` | always-green (условно) | `let Some(mut app) = local_app(&dir) else { return; };` — при отсутствии `MAX.RES` возврат до ассертов | то же |
| `attack_item_toggles_targeting_mode` | `max-brh/crates/max_app/tests/unit_menu_actions.rs:63` | always-green (условно) | `if !app.local_active() { return; }` — при неудавшемся старте локальной партии тест не проверяет переключение targeting mode | то же + падать, если старт обязателен |
| `debug_layout_matches_the_cpp_window_uly_algorithm` | `max-brh/crates/max_app/src/preferences_tests.rs:63` | always-green (условно) | `if !DEBUG_SECTION_BUILT { return; }` — в release-прогоне проверок нет; текущий CI — debug, поэтому там тест выполняется | не критично; при release-прогонах — явный skip |
| `increase_and_decrease_move_the_mine_counters` | `max-brh/crates/max_app/src/allocmenu_tests.rs:126` | always-green (условно) | `if menu.capacity_of(MiningKind::Raw) == 0 { return; }` — при нулевой ёмкости сценарий пропускается молча | сделать предусловие assert'ом фикстуры |

Все 251 тест используют один из этих паттернов; поимённый список — Приложение A.

Доказательства системности:

1. JUnit текущего прогона: `skipped = 0` — пропуски не видны ни в отчёте, ни в CI-артефактах.
2. `.github/workflows/ci.yml` чекаутит только сабмодуль `max-brh-vendor/SDL`; сабмодуль
   `max-cpp-port` (а с ним `max-cpp-port/assets/PATCHES.RES`) в GitHub CI отсутствует.
3. `git ls-files`: `max-original/MAX.RES` **отслеживается**, а `assets/PATCHES.RES` —
   **нет** (README: оригинальные данные не входят в репозиторий; `build.md` ждёт
   `assets/PATCHES.RES` в корне).
4. В текущем рабочем каталоге `MAX.RES` есть, а `assets/PATCHES.RES` — нет: до ~125
   тестов из 23 файлов с «жёстким» `repo_path("assets/PATCHES.RES")` (верхняя оценка)
   уже возвращаются до ассертов. В GitHub CI `MAX.RES` присутствует (отслеживается), а
   `assets/PATCHES.RES` и `max-cpp-port` — нет, поэтому там срабатывает подмножество,
   зависящее от этих путей. Полные 251 — верхняя граница по всем окружениям без данных
   (локальный checkout без `max-original`, кастомные data-каталоги, release-прогон).

Разбивка по причинам (по guard-условию): данные/ассеты — 229, `local_active()`/
неудавшийся старт партии — 10, `menu.capacity_* == 0` — 7, `DEBUG_SECTION_BUILT`
(только release-сборка) — 5.

**Действие (без правки кода в этом аудите):** сделать пропуск явным и учитываемым —
`#[ignore = "requires MAX.RES/PATCHES.RES; backlog: …"]` с записью в `qa-backlog.md`
(D6), либо отдельный data-профиль nextest/CI-джоб с данными и `skipped` в JUnit; в
идеале — общий фикстур-хелпер, который возвращает `Result` и превращается в явный
`skip`-отчёт, а не в `return`.

Полная таблица по файлам и поимённый список — Приложение A.

#### (б) Тавтологические `assert` и feature-ветка без проверки — 4 теста

| Тест | Файл:строка | Категория | Обоснование | Действие |
|---|---|---|---|---|
| `order_values_preserve_unit_and_boundaries` | `max-brh/crates/max_core/src/order_tests.rs:42` | always-green (часть assert) | `assert_eq!(order, order.clone())` — для derive-`Clone` не падает; `Order::Build{…} == Order::Build{…}` (два одинаковых литерала), `Order::Mode{…} == Order::Mode{…}`, `Order::EndTurn == Order::EndTurn` — сравнение константы с собой. Реальны только `match` и `assert_ne!(order, empty-route)`. | удалить тавтологические `assert_eq!` (оставить match/`assert_ne!`), либо заменить на round-trip через wire |
| `attack_targets_distinguish_unit_from_cell` | `max-brh/crates/max_core/src/order_tests.rs:93` | always-green (часть assert) | `assert_eq!(Target::Unit(NO_ID), Target::Unit(NO_ID))` и `Target::Cell(i16::MIN, i16::MAX)` с собой — не могут упасть. Содержательны `assert_ne!(Unit, Cell)` и `match` с `is_no_id()`. | удалить два self-сравнения |
| `state_ref_is_the_canonical_state_of_the_app` | `max-brh/crates/max_app/tests/universe_app.rs:44` | always-green (часть assert) | Два `assert_eq!(app.state_ref().turn_counter(), app.state_ref().turn_counter())` и то же для `active_turn_team()` — один и тот же вызов слева и справа. Содержательна только первая проверка `snapshot_bytes() == snapshot()`. | удалить два self-сравнения, оставить проверку канонических байт |
| `default_backend_without_sound_feature_is_silent` | `max-brh/crates/max_app/src/audio_tests.rs:176` | always-green (под `--features sound`) | Проверки «silent» обёрнуты в `#[cfg(not(feature = "sound"))]`; в sound-прогоне блок вырезан, остаётся `let _ = player.is_audible();` — тест не может упасть (кроме паники). CI гоняет обе конфигурации. | навесить `#[cfg(not(feature = "sound"))]` на весь тест (или проверять инвариант в обеих конфигурациях) |

Отдельно (не в счётчике): `menu_tests.rs:206` и `menu_tests.rs:245` проверяют фон
`PLANETSE`/`MULTGAME` внутри `if let Some(...)` без `else` — при отсутствии
опциональной картинки соответствующее свойство не проверяется, тест продолжается.
Действие: `expect("PLANETSE")` или явный пропуск.

---

## 5. Итог и приоритеты

| Приоритет | Что | Объём | Почему |
|---|---|---|---|
| P0 | Сделать 251 silent skip видимыми (данные в CI или `#[ignore]` + backlog) | 251 тест / 60 файлов | Q1: до ~12 % набора могут быть зелёными без единой проверки; JUnit рапортует 0 skipped |
| P1 | Удалить/слить 8 групп дубликатов | 18 тестов | Одна регрессия валит 2–3 теста; синхронная правка фикстур |
| P1 | Убрать тавтологические assert и вакуумную feature-ветку (плюс 2 меню-теста) | 4 + 2 теста | Q1/Q6: проверки не могут упасть |
| P2 | Усилить 2 тривиальных теста | 2 теста | Имена обещают свойства, которых нет в проверках |
| P2 | Негативные тесты проверяют только `is_err()` без варианта ошибки | 9 тестов (наблюдение) | Оракул ошибки слабый: подойдёт любая ошибка |

Код не менялся; перечисленные действия — предложения для Разработчика/QA, а не
выполненные правки.

---

## 6. Ограничения аудита

- Статические эвристики не доказывают отсутствие смысловых дубликатов за пределами
  выбранных порогов (например, два теста одного свойства через разные API без общих
  литералов). Для 8 групп дубликатов выполнена ручная сверка.
- rstest-кейсы считались как отдельные кейсы nextest; пофункциональный разбор
  покрывает 2114 функций, включая шаблоны rstest.
- Серверный workspace `max-brh-server` (381 функция) не входил в ревизию.
- Семантическая устарелость требует сверки с оракулом; здесь её признаки не найдены.

---

## Приложение A. Silent skip: 251 тест по файлам

Пути указаны относительно `max-brh/`; `:NNN` — строка объявления теста.

| Файл | Пропусков | Причина | Примеры тестов |
|---|---:|---|---|
| `crates\max_app\src\allocmenu_tests.rs` | 4 | menu capacity == 0 | `increase_and_decrease_move_the_mine_counters`, `level_change_is_absolute_like_a_bar_click`, `bar_captions_follow_cpp_format` и ещё 1 |
| `crates\max_app\src\assets_tests.rs` | 5 | PATCHES.RES/assets, fixture | `alien_derelict_sprites_are_loaded`, `planet_pictures_cover_all_four_groups`, `every_unit_type_loads_its_body_and_shadow` и ещё 2 |
| `crates\max_app\src\audio_tests.rs` | 1 | MAX.RES (game data) | `real_game_data_resolves_menu_click_and_unit_sounds` |
| `crates\max_app\src\credits_tests.rs` | 1 | MAX.RES (game data) | `draw_paints_background_and_text_only_inside_the_region` |
| `crates\max_app\src\cursor_tests.rs` | 1 | PATCHES.RES/assets | `cursor_image_converts_palette_indices_and_hotspot` |
| `crates\max_app\src\menu_tests.rs` | 5 | MAX.RES (game data), fixture | `clan_logos_are_drawn_at_their_anchor`, `menu_assets_load_all_button_images_from_max_res`, `main_menu_draws_mainpic_title_and_gold_captions` и ещё 2 |
| `crates\max_app\src\panels_tests.rs` | 1 | PATCHES.RES/assets | `panels_load_and_paint_every_region` |
| `crates\max_app\src\preferences_tests.rs` | 1 | DEBUG_SECTION_BUILT (release build) | `debug_layout_matches_the_cpp_window_uly_algorithm` |
| `crates\max_app\src\subtitles_tests.rs` | 1 | MAX.RES (game data) | `real_max_res_without_intro_sub_reports_the_fallback` |
| `crates\max_app\src\ui_tests.rs` | 2 | PATCHES.RES/assets | `create_form_paints_planet_pictures_from_max_res`, `native_frame_menus_are_drawn_from_max_res` |
| `crates\max_app\tests\ai_turn_timer.rs` | 6 | fixture | `real_time_ai_turn_runs_for_the_endturn_setting`, `endturn_minimum_is_fifteen_seconds`, `the_next_ai_turn_reads_the_live_preferences` и ещё 3 |
| `crates\max_app\tests\air_transport_load.rs` | 1 | fixture | `air_transport_load_click_takes_the_passenger_aboard` |
| `crates\max_app\tests\alloc_menu.rs` | 8 | fixture, menu capacity == 0 | `allocation_item_is_enabled_for_a_mining_station`, `allocate_opens_the_dialog_and_matches_the_cpp_layout`, `values_change_and_survive_closing_the_dialog` и ещё 5 |
| `crates\max_app\tests\app.rs` | 12 | MAX.RES (game data), PATCHES.RES/assets, fixture | `assets_integration_smoke`, `chrome_click_on_the_map_centre_selects_the_unit_under_it`, `chrome_end_turn_click_is_a_button_not_a_map_click` и ещё 9 |
| `crates\max_app\tests\build_lifecycle_interactive.rs` | 1 | fixture | `right_click_while_building_is_no_go_and_keeps_the_site` |
| `crates\max_app\tests\build_menu_render.rs` | 5 | PATCHES.RES/assets, fixture | `list_rows_draw_the_i_star_icons_of_every_visible_building`, `list_icons_are_recolored_for_the_team_like_cargo_menu`, `portrait_is_the_selected_buildings_p_star_big_image` и ещё 2 |
| `crates\max_app\tests\build_site.rs` | 7 | fixture | `ghost_cursor_moves_with_the_mouse_and_marks_validity`, `ghost_is_drawn_in_the_frame_and_esc_cancels_without_an_order`, `ghost_is_drawn_after_both_build_menu_paths` и ещё 4 |
| `crates\max_app\tests\building_animation.rs` | 5 | fixture | `mining_station_body_is_static_and_only_the_palette_cycles_with_dt`, `factory_with_a_queue_holds_its_on_frame_and_blinks_only_via_the_palette`, `radar_sentry_animation_advances_with_real_dt` и ещё 2 |
| `crates\max_app\tests\building_frames.rs` | 7 | PATCHES.RES/assets, fixture | `powered_generator_uses_the_on_frame_and_power_off_uses_the_off_frame`, `powered_mining_station_uses_its_clan_variant_and_on_frame`, `mining_station_body_frames_are_eight_clan_pairs_not_a_drill_cycle` и ещё 4 |
| `crates\max_app\tests\cargo_render.rs` | 11 | PATCHES.RES/assets, fixture | `cargo_assets_carry_icons_for_every_purchase_type`, `turret_icons_carry_the_second_frame_and_d2_offset`, `turret_is_painted_over_the_tank_icon_in_the_list` и ещё 8 |
| `crates\max_app\tests\construction_animation.rs` | 3 | fixture | `builder_sprite_cycles_through_work_frames_with_real_dt`, `scaling_unit_grows_to_full_size_over_real_time`, `unit_without_scaler_is_never_scaled` |
| `crates\max_app\tests\construction_site.rs` | 1 | fixture | `construction_site_is_drawn_on_the_map_until_the_building_is_ready` |
| `crates\max_app\tests\enemy_unit_panel.rs` | 3 | PATCHES.RES/assets, fixture | `clicking_a_visible_enemy_selects_it_instead_of_ordering_an_attack`, `clicking_an_invisible_enemy_does_not_select_or_order`, `an_attackable_enemy_still_receives_the_attack_order` |
| `crates\max_app\tests\grid_coverage.rs` | 1 | PATCHES.RES/assets | `grid_covers_all_four_edges_of_the_map_window` |
| `crates\max_app\tests\hot_seat.rs` | 9 | fixture | `two_humans_alternate_with_the_begin_turn_dialog`, `four_humans_all_play_in_team_order`, `ai_plays_between_two_humans` и ещё 6 |
| `crates\max_app\tests\intro.rs` | 6 | PATCHES.RES/assets | `intro_asset_points_to_maxint_mve`, `intro_decodes_frame_zero_and_scales_to_letterbox`, `intro_advances_by_real_time_and_esc_closes` и ещё 3 |
| `crates\max_app\tests\landing_wrl.rs` | 2 | MAX.RES (game data), fixture | `landing_frame_contains_real_wrl_tiles`, `landing_without_wrl_falls_back_to_flat_map` |
| `crates\max_app\tests\local_sp.rs` | 11 | PATCHES.RES/assets, fixture, local_active / setup failed | `local_update_with_real_dt_animates_move_orders`, `clan_choice_changes_starting_team_values`, `local_save_roundtrips_clans` и ещё 8 |
| `crates\max_app\tests\media_presentation.rs` | 1 | PATCHES.RES/assets | `snapshot_bytes_do_not_depend_on_the_loaded_content` |
| `crates\max_app\tests\mission_ai.rs` | 3 | fixture | `mission_load_keeps_the_ai_complexes_and_clan`, `mission_load_ai_turn_rebuilds_tasks_and_moves_units`, `mission_load_ai_produces_units_after_load` |
| `crates\max_app\tests\mission_map.rs` | 1 | PATCHES.RES/assets | `mission_one_terrain_matches_the_interplay_planet_map` |
| `crates\max_app\tests\mission_story.rs` | 5 | MAX.RES (game data), PATCHES.RES/assets, fixture | `a_missing_story_file_is_visible_and_does_not_block_the_mission`, `the_campaign_intro_uses_the_real_story_block`, `the_last_campaign_mission_has_a_victory_story_with_music` и ещё 2 |
| `crates\max_app\tests\mission_unit_idle.rs` | 1 | PATCHES.RES/assets | `loaded_mission_unit_renders_the_idle_frame_and_still_flashes_when_firing` |
| `crates\max_app\tests\move_input.rs` | 6 | PATCHES.RES/assets, fixture | `generator_and_mining_station_are_on_different_cells_without_sprite_overlap`, `start_units_are_drawn_after_landing`, `base_is_centred_and_the_qa_click_lands_on_the_surveyor` и ещё 3 |
| `crates\max_app\tests\music.rs` | 2 | MAX.RES (game data), fixture | `real_max_res_references_the_music_tracks`, `real_msc_tracks_are_stereo_pcm16_wav` |
| `crates\max_app\tests\overlay_colors.rs` | 2 | fixture | `colors_frame_uses_the_team_color`, `alien_units_fall_back_to_derelict_and_stationary_is_skipped` |
| `crates\max_app\tests\portrait.rs` | 6 | PATCHES.RES/assets, fixture | `every_unit_type_with_flics_maps_to_a_loaded_128x128_animation`, `portrait_draws_the_flc_first_frame_and_dt_zero_keeps_it_still`, `portrait_advances_at_fifteen_fps_and_loops` и ещё 3 |
| `crates\max_app\tests\preferences.rs` | 9 | DEBUG_SECTION_BUILT (release build), PATCHES.RES/assets, fixture | `in_game_panel_button_opens_preferences_and_done_saves_effects`, `effects_off_freezes_building_animation_frames`, `sound_flag_reaches_the_audio_layer` и ещё 6 |
| `crates\max_app\tests\preferences_behavior.rs` | 6 | fixture | `fast_movement_doubles_the_visual_unit_speed`, `follow_unit_centers_the_camera_on_the_selected_unit`, `auto_select_picks_the_next_unit_after_a_move` и ещё 3 |
| `crates\max_app\tests\presentation_gate.rs` | 3 | fixture | `snapshot_bytes_and_digest_do_not_depend_on_the_presentation_profile`, `alt_tab_does_not_reset_or_unpause_the_match`, `session_step_is_not_gated_on_focus` |
| `crates\max_app\tests\qa_driver.rs` | 8 | MAX.RES (game data), fixture | `sp_full_script_passes_through_the_real_input_path`, `cargo_full_script_passes_through_the_real_input_path`, `move_input_script_passes_through_the_real_input_path` и ещё 5 |
| `crates\max_app\tests\save_load.rs` | 13 | MAX.RES (game data) | `played_game_saves_as_v70_and_loads_in_a_fresh_app`, `autosave_is_written_at_the_end_of_a_round`, `save_menu_lists_slots_with_preview_and_edits_the_name` и ещё 10 |
| `crates\max_app\tests\soak.rs` | 2 | MAX.RES (game data) | `short_headless_soak_completes_and_is_deterministic`, `soak_budget_helper_reports_a_fraction_of_the_frame` |
| `crates\max_app\tests\spinning_turret_animation.rs` | 3 | PATCHES.RES/assets, fixture | `awac_radar_turns_through_thirty_frames_and_wraps`, `mobile_scanner_radar_turns_through_sixteen_frames`, `turret_clock_is_frame_only_and_follows_the_order_tick` |
| `crates\max_app\tests\stat_panel.rs` | 3 | PATCHES.RES/assets | `mine_panel_frame_skips_speed_and_empty_rows_tank_keeps_them`, `stat_rows_draw_the_pip_icons`, `factory_panel_frame_shows_raw_usage_only_while_producing` |
| `crates\max_app\tests\survey_circles.rs` | 2 | fixture | `a_surveyed_cell_without_a_resource_shows_the_circle`, `at_small_zoom_only_known_deposits_get_markers` |
| `crates\max_app\tests\transfer.rs` | 5 | fixture, menu capacity == 0 | `xfer_item_is_enabled_for_a_mining_station`, `xfer_opens_on_target_click_and_moves_cargo_on_done`, `xfer_cancel_and_zero_amount_do_not_change_storage` и ещё 2 |
| `crates\max_app\tests\turn_based.rs` | 3 | fixture | `first_turn_end_turn_with_a_missile_in_flight_returns_to_the_player`, `hot_seat_turn_based_covers_humans_and_the_computer`, `several_turn_based_rounds_close_without_hanging` |
| `crates\max_app\tests\unit_menu.rs` | 12 | fixture | `repeat_click_on_selected_unit_opens_menu`, `menu_items_follow_unit_type`, `escape_closes_menu_and_does_not_leave_session` и ещё 9 |
| `crates\max_app\tests\unit_menu_actions.rs` | 9 | local_active / setup failed | `attack_item_toggles_targeting_mode`, `load_mode_orders_move_to_unit_and_lands_on_arrival`, `infiltrator_orders_steal_and_disable_with_parent` и ещё 6 |
| `crates\max_app\tests\unit_ready_label.rs` | 6 | fixture | `factory_with_a_produced_unit_shows_the_label`, `finished_building_gets_the_label_from_its_builder`, `foreign_unit_has_no_label` и ещё 3 |
| `crates\max_app\tests\unit_sprites.rs` | 6 | PATCHES.RES/assets, fixture | `every_unit_type_draws_a_non_empty_frame_and_turret_where_flagged`, `firing_frames_come_from_the_d_star_firing_block`, `map_render_loop_covers_all_93_unit_types` и ещё 3 |
| `crates\max_assets\src\cursor_tests.rs` | 1 | MAX.RES (game data) | `real_max_res_cursors_match_the_cpp_descriptors` |
| `crates\max_assets\src\image_tests.rs` | 1 | MAX.RES (game data) | `snow_pic_decodes_all_rows` |
| `crates\max_assets\src\markers_tests.rs` | 1 | MAX.RES (game data) | `real_max_res_has_all_resource_markers` |
| `crates\max_assets\src\mve_tests.rs` | 3 | fixture | `real_maxint_audio_track_matches_the_oracle_decoder`, `real_maxint_metadata_and_reference_frames`, `real_maxint_reaches_the_end` |
| `crates\max_assets\src\res_tests.rs` | 1 | MAX.RES (game data) | `real_max_res_exposes_ascii_file_references` |
| `crates\max_assets\src\wav_tests.rs` | 1 | fixture | `all_real_spw_files_are_pcm16_22050_mono` |
| `crates\max_assets\tests\flic_assets.rs` | 3 | MAX.RES (game data), PATCHES.RES/assets | `max_res_has_71_flc_reference_records`, `max_res_flc_records_match_the_generated_manifest`, `all_71_flc_resources_parse_and_decode_every_frame` |
| `crates\max_render\src\map_render_tests.rs` | 2 | PATCHES.RES/assets | `resource_markers_are_drawn_from_max_res`, `a_surveyed_empty_cell_gets_the_circle_marker` |

### Полный поимённый список

- `crates\max_app\src\allocmenu_tests.rs` — `increase_and_decrease_move_the_mine_counters`:126, `level_change_is_absolute_like_a_bar_click`:147, `bar_captions_follow_cpp_format`:244, `bar_geometry_follows_cpp`:261
- `crates\max_app\src\assets_tests.rs` — `alien_derelict_sprites_are_loaded`:27, `planet_pictures_cover_all_four_groups`:80, `every_unit_type_loads_its_body_and_shadow`:178, `turret_frames_cover_every_flagged_type`:268, `assets_load_from_fallback_when_configured_patches_res_is_missing`:576
- `crates\max_app\src\audio_tests.rs` — `real_game_data_resolves_menu_click_and_unit_sounds`:193
- `crates\max_app\src\credits_tests.rs` — `draw_paints_background_and_text_only_inside_the_region`:161
- `crates\max_app\src\cursor_tests.rs` — `cursor_image_converts_palette_indices_and_hotspot`:62
- `crates\max_app\src\menu_tests.rs` — `clan_logos_are_drawn_at_their_anchor`:53, `menu_assets_load_all_button_images_from_max_res`:131, `main_menu_draws_mainpic_title_and_gold_captions`:163, `create_form_draws_planetse_grid_and_mnubtn_buttons`:206, `join_list_draws_multgame_frame_rows_and_buttons`:245
- `crates\max_app\src\panels_tests.rs` — `panels_load_and_paint_every_region`:527
- `crates\max_app\src\preferences_tests.rs` — `debug_layout_matches_the_cpp_window_uly_algorithm`:63
- `crates\max_app\src\subtitles_tests.rs` — `real_max_res_without_intro_sub_reports_the_fallback`:104
- `crates\max_app\src\ui_tests.rs` — `create_form_paints_planet_pictures_from_max_res`:257, `native_frame_menus_are_drawn_from_max_res`:493
- `crates\max_app\tests\ai_turn_timer.rs` — `real_time_ai_turn_runs_for_the_endturn_setting`:235, `endturn_minimum_is_fifteen_seconds`:266, `the_next_ai_turn_reads_the_live_preferences`:296, `endturn_15_and_45_spend_the_same_shots_and_ammo`:324, `turn_based_keeps_the_oracle_counter_trigger`:474, `missing_setting_falls_back_to_the_counter`:510
- `crates\max_app\tests\air_transport_load.rs` — `air_transport_load_click_takes_the_passenger_aboard`:151
- `crates\max_app\tests\alloc_menu.rs` — `allocation_item_is_enabled_for_a_mining_station`:145, `allocate_opens_the_dialog_and_matches_the_cpp_layout`:164, `values_change_and_survive_closing_the_dialog`:185, `arrows_bump_one_unit_like_the_cpp_buttons`:242, `dialog_is_modal_for_the_map`:273, `allocation_changes_the_complex_income_next_turn`:289, `two_identical_dialogs_stay_deterministic`:366, `frame_shows_the_full_screen_dialog`:392
- `crates\max_app\tests\app.rs` — `assets_integration_smoke`:659, `chrome_click_on_the_map_centre_selects_the_unit_under_it`:788, `chrome_end_turn_click_is_a_button_not_a_map_click`:833, `chrome_timer_shows_the_turn_timer_value_with_the_cpp_colors`:899, `chrome_end_turn_is_pressed_with_the_team_color_on_an_ai_turn`:947, `chrome_build_markers_are_drawn_around_the_finished_site`:1018, `chrome_toggle_and_zoom_clicks_work_on_the_panels`:1084, `app_can_be_created_twice_and_start_exit_twice`:1123, `non_ascii_player_name_renders_with_the_fallback_glyph`:1223, `build_marker_cells_are_the_cpp_ring_around_a_two_by_two_site`:1435, `resource_icons_are_survey_knowledge_only_and_above_buildings`:1524, `a_built_radar_keeps_rotating_its_sixteen_frames`:1688
- `crates\max_app\tests\build_lifecycle_interactive.rs` — `right_click_while_building_is_no_go_and_keeps_the_site`:389
- `crates\max_app\tests\build_menu_render.rs` — `list_rows_draw_the_i_star_icons_of_every_visible_building`:107, `list_icons_are_recolored_for_the_team_like_cargo_menu`:147, `portrait_is_the_selected_buildings_p_star_big_image`:176, `portrait_follows_the_selected_row_and_the_queue_selection`:242, `stats_table_shows_the_building_characteristics_with_icons`:281
- `crates\max_app\tests\build_site.rs` — `ghost_cursor_moves_with_the_mouse_and_marks_validity`:249, `ghost_is_drawn_in_the_frame_and_esc_cancels_without_an_order`:296, `ghost_is_drawn_after_both_build_menu_paths`:358, `builder_turns_to_the_tape_and_slides_diagonally_by_eight_pixels_per_tick`:510, `lmb_on_an_invalid_cell_cancels_the_selection`:632, `same_input_gives_the_same_site_and_digest`:679, `engineer_path_attaches_the_route_and_builds_at_its_own_cell`:719
- `crates\max_app\tests\building_animation.rs` — `mining_station_body_is_static_and_only_the_palette_cycles_with_dt`:82, `factory_with_a_queue_holds_its_on_frame_and_blinks_only_via_the_palette`:195, `radar_sentry_animation_advances_with_real_dt`:295, `animation_clock_does_not_change_the_snapshot`:342, `adjacent_buildings_get_connector_frames_and_isolated_ones_do_not`:399
- `crates\max_app\tests\building_frames.rs` — `powered_generator_uses_the_on_frame_and_power_off_uses_the_off_frame`:99, `powered_mining_station_uses_its_clan_variant_and_on_frame`:148, `mining_station_body_frames_are_eight_clan_pairs_not_a_drill_cycle`:203, `power_generator_body_is_static_and_its_four_frames_are_pipes`:384, `working_factory_holds_the_on_frame_and_an_idle_factory_is_off`:428, `working_builder_uses_the_plus_sixteen_frame`:561, `construction_site_decals_have_sprites`:605
- `crates\max_app\tests\cargo_render.rs` — `cargo_assets_carry_icons_for_every_purchase_type`:83, `turret_icons_carry_the_second_frame_and_d2_offset`:110, `turret_is_painted_over_the_tank_icon_in_the_list`:175, `sprite_renderer_draws_the_tank_turret_as_a_separate_frame`:206, `purchase_screen_draws_icons_bars_and_buttons`:245, `upgrade_screen_shows_more_of_the_frame`:298, `upgrade_costs_are_shown_within_the_limit`:329, `full_purchase_list_fits_nine_rows_with_scroll`:359, `portrait_is_the_p_star_big_image_for_every_selectable_type`:389, `purchase_screen_draws_portrait_description_and_stat_pips`:418, `stat_rows_change_with_the_selected_unit`:475
- `crates\max_app\tests\construction_animation.rs` — `builder_sprite_cycles_through_work_frames_with_real_dt`:72, `scaling_unit_grows_to_full_size_over_real_time`:129, `unit_without_scaler_is_never_scaled`:180
- `crates\max_app\tests\construction_site.rs` — `construction_site_is_drawn_on_the_map_until_the_building_is_ready`:63
- `crates\max_app\tests\enemy_unit_panel.rs` — `clicking_a_visible_enemy_selects_it_instead_of_ordering_an_attack`:173, `clicking_an_invisible_enemy_does_not_select_or_order`:286, `an_attackable_enemy_still_receives_the_attack_order`:339
- `crates\max_app\tests\grid_coverage.rs` — `grid_covers_all_four_edges_of_the_map_window`:66
- `crates\max_app\tests\hot_seat.rs` — `two_humans_alternate_with_the_begin_turn_dialog`:223, `four_humans_all_play_in_team_order`:310, `ai_plays_between_two_humans`:346, `notice_freezes_the_turn_timer_until_acknowledged`:403, `only_the_active_human_controls_their_units`:448, `input_is_ignored_during_the_ai_turn`:580, `orders_are_blocked_while_the_transition_notice_is_open`:627, `begin_turn_dialog_renders_over_the_game_frame`:681, `screens_and_setup_render_with_hot_seat_forms`:751
- `crates\max_app\tests\intro.rs` — `intro_asset_points_to_maxint_mve`:46, `intro_decodes_frame_zero_and_scales_to_letterbox`:61, `intro_advances_by_real_time_and_esc_closes`:87, `intro_audio_track_is_decoded_into_chunks`:114, `intro_draws_the_caption_of_the_current_frame`:146, `a_missing_intro_and_missing_subtitles_are_a_placeholder`:215
- `crates\max_app\tests\landing_wrl.rs` — `landing_frame_contains_real_wrl_tiles`:35, `landing_without_wrl_falls_back_to_flat_map`:130
- `crates\max_app\tests\local_sp.rs` — `local_update_with_real_dt_animates_move_orders`:143, `clan_choice_changes_starting_team_values`:220, `local_save_roundtrips_clans`:264, `five_turns_against_the_ai_alternate_and_the_ai_acts`:442, `simultaneous_moves_mode_also_plays_turns`:535, `turn_timer_expires_and_closes_the_turn`:555, `pause_stops_ticks_and_timer`:579, `human_on_a_later_slot_starts_with_the_ai_turns`:603, `eliminating_the_ai_wins_and_the_game_over_screen_renders`:649, `local_save_roundtrips_after_a_turn`:733, `v70_save_loads_into_the_local_game`:786
- `crates\max_app\tests\media_presentation.rs` — `snapshot_bytes_do_not_depend_on_the_loaded_content`:88
- `crates\max_app\tests\mission_ai.rs` — `mission_load_keeps_the_ai_complexes_and_clan`:167, `mission_load_ai_turn_rebuilds_tasks_and_moves_units`:245, `mission_load_ai_produces_units_after_load`:307
- `crates\max_app\tests\mission_map.rs` — `mission_one_terrain_matches_the_interplay_planet_map`:175
- `crates\max_app\tests\mission_story.rs` — `a_missing_story_file_is_visible_and_does_not_block_the_mission`:181, `the_campaign_intro_uses_the_real_story_block`:245, `the_last_campaign_mission_has_a_victory_story_with_music`:284, `the_story_frame_is_deterministic_and_the_video_sequence_finishes`:339, `the_qa_driver_shows_the_intro_story_and_starts_the_same_mission`:395
- `crates\max_app\tests\mission_unit_idle.rs` — `loaded_mission_unit_renders_the_idle_frame_and_still_flashes_when_firing`:150
- `crates\max_app\tests\move_input.rs` — `generator_and_mining_station_are_on_different_cells_without_sprite_overlap`:126, `start_units_are_drawn_after_landing`:305, `base_is_centred_and_the_qa_click_lands_on_the_surveyor`:334, `move_order_moves_the_unit_on_following_ticks`:378, `click_selects_own_unit_and_rmb_shows_path_preview`:406, `rmb_then_end_turn_moves_the_unit_through_the_path_in_the_local_loop`:594
- `crates\max_app\tests\music.rs` — `real_max_res_references_the_music_tracks`:190, `real_msc_tracks_are_stereo_pcm16_wav`:224
- `crates\max_app\tests\overlay_colors.rs` — `colors_frame_uses_the_team_color`:81, `alien_units_fall_back_to_derelict_and_stationary_is_skipped`:177
- `crates\max_app\tests\portrait.rs` — `every_unit_type_with_flics_maps_to_a_loaded_128x128_animation`:100, `portrait_draws_the_flc_first_frame_and_dt_zero_keeps_it_still`:160, `portrait_advances_at_fifteen_fps_and_loops`:217, `clearing_the_selection_paints_the_portrait_black`:245, `type_without_flic_falls_back_to_the_sprite`:265, `switching_units_rebuilds_the_animation_from_frame_zero`:288
- `crates\max_app\tests\preferences.rs` — `in_game_panel_button_opens_preferences_and_done_saves_effects`:101, `effects_off_freezes_building_animation_frames`:161, `sound_flag_reaches_the_audio_layer`:223, `preferences_are_written_to_and_read_from_v70_ini_preferences`:270, `debug_switches_install_through_the_facade_when_the_dialog_closes`:402, `debug_team_field_installs_the_team_control_slot`:462, `debug_gate_keeps_the_snapshot_and_the_installed_switch_moves_it`:533, `debug_cancel_restores_the_installed_values`:590, `debug_section_is_painted_above_the_dialog_buttons`:656
- `crates\max_app\tests\preferences_behavior.rs` — `fast_movement_doubles_the_visual_unit_speed`:198, `follow_unit_centers_the_camera_on_the_selected_unit`:281, `auto_select_picks_the_next_unit_after_a_move`:336, `enemy_halt_reaches_the_simulation`:456, `turn_timers_from_preferences_apply_to_the_local_game`:488, `player_name_applies_to_the_game_and_the_v70_save`:537
- `crates\max_app\tests\presentation_gate.rs` — `snapshot_bytes_and_digest_do_not_depend_on_the_presentation_profile`:199, `alt_tab_does_not_reset_or_unpause_the_match`:273, `session_step_is_not_gated_on_focus`:373
- `crates\max_app\tests\qa_driver.rs` — `sp_full_script_passes_through_the_real_input_path`:165, `cargo_full_script_passes_through_the_real_input_path`:220, `move_input_script_passes_through_the_real_input_path`:259, `dt_zero_frames_are_byte_identical_across_runs`:319, `research_help_script_passes_through_the_real_input_path`:396, `v70_save_is_loaded_by_the_script_command`:498, `recorded_manual_session_round_trips_through_qa_run`:750, `edge_cases_script_passes_click_spam_and_unicode`:937
- `crates\max_app\tests\save_load.rs` — `played_game_saves_as_v70_and_loads_in_a_fresh_app`:119, `autosave_is_written_at_the_end_of_a_round`:173, `save_menu_lists_slots_with_preview_and_edits_the_name`:216, `save_hotkey_opens_and_closes_the_menu_in_a_local_game`:295, `local_load_resumes_without_replaying_the_turn_economy`:334, `save_seed_is_byte_exact_and_reseeds_the_generator`:441, `save_seed_is_deterministic_across_saves`:502, `overwritten_autosave_keeps_the_previous_slot_as_bak`:533, `regular_save_does_not_create_a_backup`:573, `quick_save_writes_the_last_slot_in_the_window_format`:600, `quick_save_opens_the_window_for_an_empty_last_slot`:652, `quick_load_loads_the_last_slot`:674, `save_directory_follows_the_mission_category`:709
- `crates\max_app\tests\soak.rs` — `short_headless_soak_completes_and_is_deterministic`:34, `soak_budget_helper_reports_a_fraction_of_the_frame`:100
- `crates\max_app\tests\spinning_turret_animation.rs` — `awac_radar_turns_through_thirty_frames_and_wraps`:120, `mobile_scanner_radar_turns_through_sixteen_frames`:256, `turret_clock_is_frame_only_and_follows_the_order_tick`:307
- `crates\max_app\tests\stat_panel.rs` — `mine_panel_frame_skips_speed_and_empty_rows_tank_keeps_them`:100, `stat_rows_draw_the_pip_icons`:177, `factory_panel_frame_shows_raw_usage_only_while_producing`:224
- `crates\max_app\tests\survey_circles.rs` — `a_surveyed_cell_without_a_resource_shows_the_circle`:113, `at_small_zoom_only_known_deposits_get_markers`:166
- `crates\max_app\tests\transfer.rs` — `xfer_item_is_enabled_for_a_mining_station`:179, `xfer_opens_on_target_click_and_moves_cargo_on_done`:199, `xfer_cancel_and_zero_amount_do_not_change_storage`:267, `xfer_help_opens_transfer_setup`:300, `xfer_frame_draws_the_window_over_the_map`:342
- `crates\max_app\tests\turn_based.rs` — `first_turn_end_turn_with_a_missile_in_flight_returns_to_the_player`:113, `hot_seat_turn_based_covers_humans_and_the_computer`:242, `several_turn_based_rounds_close_without_hanging`:309
- `crates\max_app\tests\unit_menu.rs` — `repeat_click_on_selected_unit_opens_menu`:195, `menu_items_follow_unit_type`:217, `escape_closes_menu_and_does_not_leave_session`:263, `menu_is_modal_for_the_map`:282, `hotkey_and_click_activate_sentry`:306, `build_item_opens_build_menu`:343, `disabled_item_does_not_act`:363, `builder_stop_cancels_the_build_and_build_returns`:387, `frame_shows_menu_over_map`:467, `cursor_mode_repairs_own_unit`:543, `enter_mode_moves_to_unit`:590, `done_item_deselects_and_keeps_game_running`:619
- `crates\max_app\tests\unit_menu_actions.rs` — `attack_item_toggles_targeting_mode`:63, `load_mode_orders_move_to_unit_and_lands_on_arrival`:99, `infiltrator_orders_steal_and_disable_with_parent`:191, `bulldozer_remove_orders_clear_with_parent_and_build_time`:277, `upgrade_all_spends_complex_materials_and_sends_orders`:351, `self_destruct_dialog_arms_and_orders_explode`:400, `alt_p_opens_pause_dialog_and_resumes`:461, `ui_events_queue_cpp_sounds`:491, `auto_item_enables_auto_survey_and_leaves_sentry`:557
- `crates\max_app\tests\unit_ready_label.rs` — `factory_with_a_produced_unit_shows_the_label`:73, `finished_building_gets_the_label_from_its_builder`:145, `foreign_unit_has_no_label`:183, `finished_research_gets_the_label`:215, `label_color_blinks_with_the_palette_cycle`:250, `stored_unit_itself_has_no_label`:284
- `crates\max_app\tests\unit_sprites.rs` — `every_unit_type_draws_a_non_empty_frame_and_turret_where_flagged`:87, `firing_frames_come_from_the_d_star_firing_block`:188, `map_render_loop_covers_all_93_unit_types`:309, `hud_portrait_draws_every_unit_type`:376, `explicit_body_angle_selects_the_corresponding_sprite_frame`:408, `frame_info_table_matches_game_data`:474
- `crates\max_assets\src\cursor_tests.rs` — `real_max_res_cursors_match_the_cpp_descriptors`:77
- `crates\max_assets\src\image_tests.rs` — `snow_pic_decodes_all_rows`:129
- `crates\max_assets\src\markers_tests.rs` — `real_max_res_has_all_resource_markers`:4
- `crates\max_assets\src\mve_tests.rs` — `real_maxint_audio_track_matches_the_oracle_decoder`:330, `real_maxint_metadata_and_reference_frames`:417, `real_maxint_reaches_the_end`:472
- `crates\max_assets\src\res_tests.rs` — `real_max_res_exposes_ascii_file_references`:172
- `crates\max_assets\src\wav_tests.rs` — `all_real_spw_files_are_pcm16_22050_mono`:184
- `crates\max_assets\tests\flic_assets.rs` — `max_res_has_71_flc_reference_records`:106, `max_res_flc_records_match_the_generated_manifest`:172, `all_71_flc_resources_parse_and_decode_every_frame`:212
- `crates\max_render\src\map_render_tests.rs` — `resource_markers_are_drawn_from_max_res`:104, `a_surveyed_empty_cell_gets_the_circle_marker`:156

---

## Приложение B. Воспроизводимость

```text
# инвентаризация
grep -R --include='*.rs' -E '^\s*#\[(tokio::)?test' max-brh/crates | wc -l        # 2114
grep -R --include='*.rs' -E '#\[(ignore|should_panic)' max-brh/crates | wc -l    # 0

# always-green: return до первой проверки
# (скрипт: разбор тел с балансировкой скобок; guard-позиция < позиции первого assert)

# JUnit
max-brh/target/nextest/ci/junit.xml  # tests=2153 skipped=0 failures=0

# данные
git ls-files --error-unmatch max-original/MAX.RES        # OK (отслеживается)
git ls-files --error-unmatch assets/PATCHES.RES          # ошибка: не отслеживается
```

Скрипты аудита и промежуточные выгрузки не добавлялись в репозиторий (работали во
временном каталоге).
