# План: Кампания + одиночные миссии (сценарии)

Версия 0.1 (проект Аналитика) · 2026-11-24.

## TL;DR

Симуляционная часть миссий **уже готова** (V70-читатель, `mission_category`/`mission_index`,
`mission_win_loss`, `start_local_game_from_save`). Отсутствует **только слой подачи**:
реестр дескрипторов миссий (JSON из `PATCHES.RES`), меню выбора, подменю New Game,
прогрессия кампании, экраны брифинга/истории. **~4 этапа, ~70% — UI-порт + 1 реестр.**

## Что уже есть vs отсутствует

**Есть:** `MissionCategory` (8), V70-парсер + `GameUniverse::from_save`,
`mission_category`/`mission_index` в `GameState`, win/loss уникальных миссий
(`victory.rs::mission_win_loss`), экран победы/поражения, `start_local_game_from_save`,
расширения сейвов cam/sce/tra/mps, `Packet12MissionSupply`+landing.

**Отсутствует:**

| # | Что | Оракул |
|---|---|---|
| M1 | Реестр миссий (JSON `SC_*` → XOR → разбор по категориям) | `mission.cpp:425-455`, `missionregistry.cpp:28-82` |
| M2 | Подменю New Game (7 кнопок) | `menu.cpp:2171` |
| M3 | Экран выбора миссии (`GameSetupMenu`) | `gamesetupmenu.cpp` |
| M4 | Поток старта миссии (дескриптор → SAVE*.cam → `start_local_game_from_save`) | `menu.cpp:2052-2098` |
| M5 | Прогрессия кампании (победа → следующая миссия) | `menu.cpp:518-545` |
| M6 | Экраны истории (intro/victory/defeat) | `menu.cpp:766` |
| M7 | (условно) game-rules buildability | `gameruleshandler.cpp:126-175` |

## Формат миссий

Миры миссий = **обычные V70-сейвы** (`SAVE1.CAM` → заголовок 0x46=70). Дескриптор =
JSON (XOR-обфусцирован) в `PATCHES.RES` (`SC_C0001…SC_D0003`): title/description/
hash/mission-file/winloss_conditions(LUA)/game_rules(LUA)/story. Rust не встраивает
LUA — win/loss уже захардкожено; game-rules — по категории/индексу при необходимости.

| Категория | Ресурсы | Сейвы | Кол-во |
|---|---|---|---|
| Campaign | SC_C0001–9 | SAVE1..9.CAM | 9 |
| Scenario | SC_S0001–24 | SAVE1..24.SCE | 24 |
| Training | SC_T0001–15 | SAVE1..15.TRA | 15 |
| MultiPlayerScenario | SC_M0001–12 | SAVE1..12.MPS | 12 |
| Demo | SC_D0001–3 | SAVE1.DMO | 1–3 |

## План (4 этапа с гейтами)

- **Этап 0 — реестр миссий** (без UI): `MissionDescriptor`+`MissionRegistry`,
  XOR-декод `SC_*`, разбор JSON, разложение по категориям. Гейт: `len(Campaign)==9`,
  `[0].title=="Islands in the Sun"`.
- **Этап 1 — поток старта миссии** (headless): реестр → `SAVE*.cam` → `start_local_game_from_save`.
  Гейт: SAVE1.CAM 5 тиков детерминизм; победа Campaign 1 по условиям.
- **Этап 2 — меню**: подменю New Game (7 кнопок) + `GameSetupMenu` (список+описание+Start).
  Гейт: кадры выбора (golden).
- **Этап 3 — прогрессия + стори-экраны**: победа → следующая миссия (`last_campaign`),
  экраны intro/victory/defeat. Гейт: end-to-end Campaign 1→2.
- **Этап 4 (условный) — game-rules buildability** по категории/индексу.

## Риски

1. Порядок проверок win/loss для всех миссий — перепроверить таблицей.
2. Стори-экраны ссылаются на фон (MAX.RES) и видео (.MVE) — подключить.
3. `hash`-верификация — warning, не блокер.
4. Прогрессия — нужен персистентный settings (`last_campaign`/`game_file_number`).
5. Hot Seat Scenarios / MultiPlayerScenario — вне этого объёма.

## Реализация этапа 1 (2026-11-25, Разработчик)

Поток старта миссии без UI (M4): реестр → дескриптор → файл миссии → локальная партия.

- **API** (`max_app::mission`):
  `App::start_mission_from_registry(registry, category, index, setup)` — вход
  «категория + индекс» из `GameSetupMenu` (`index` 0-based, как C++
  `game_file_number - 1`, `menu.cpp:2052-2056`), и
  `App::start_mission_from_descriptor(descriptor, setup)` — тот же поток по уже
  разрешённому дескриптору. Оба читают файл миссии и вызывают
  `App::start_local_game_from_save`, поэтому путь реестра не может разойтись с
  прямым сейв-лоадом. Отчёт `MissionStart` (tag/title/category/mission_index/path)
  понадобится экрану выбора (этап 2).
- **Идентичность миссии — из заголовка V70**: `mission_category` и 1-based
  `mission_index` попадают в `GameState` через `GameState::load_state`
  (`game.rs:562-564`, `saveload.cpp:772`, `SaveLoad_TranslateSaveFileCategory`
  `saveload.cpp:1609-1664`) — как C++ восстанавливает дескриптор по заголовку при
  загрузке (`saveload.cpp:196`). Реестр выбирает только файл; `mission_index` не
  перезаписывается нулём (`0` молча включил бы generic-правила, `victory.rs:26`).
  Расхождение заголовка и дескриптора — предупреждение (уровень hash-warning,
  `mission.cpp:263`), не ошибка старта.
- **`start_local_game_from_save` не форсирует `Custom`/`HotSeat`**: категория
  загруженного сейва сохраняется — `SaveLoad_Load(filepath, mission->GetCategory(), …)`
  (`game_manager.cpp:1930-1936`); портовое M-HS-поведение (несколько людей в
  *custom*-сейве → hot-seat) осталось только для `Custom`. `SaveMeta::save_game_type`
  пишется обратной таблицей категории (`local.rs::file_type_for_mission`), поэтому
  повторный сейв загруженной миссии остаётся сейвом миссии.
- **Headless:** загрузка идёт через `App::load_save_bytes` → `App::adopt_universe`
  (install-API C3), `CargoMenu`/`SelectSite` не открываются; после старта партия
  сразу turn-active (`GAME_STATE_11_TURN_ACTIVE` → `GAME_STATE_8_IN_GAME`),
  `build_menu`/`build_site` — `None`.
- **Взаимодействие с save/load (S1):** `App::load_v70_path` и quick load идут через
  тот же `start_local_game_from_save`, поэтому загруженная кампания теперь сохраняет
  категорию — автосейв и quick save пишут `save10.cam` в data-каталог, а не
  `save10.dta` в pref-каталог (`SaveLoad_GetFilePath`, `saveload.cpp:72-78`).
- **Гейт:** `max_app/tests/mission_start.rs` — для всех четырёх файловых категорий
  (Campaign/Scenario/Training/MultiPlayerScenario) старт из реестра даёт побайтово
  те же `snapshot_bytes`, что прямой `start_local_game_from_save` (на старте и после
  5 тиков; digest совпадает), партия turn-active, `mission_win_loss` для Campaign 1
  отдаёт `Pending`, а не generic `Generic`. Победа Campaign 1 по условиям
  (теплицы/исследовательские центры) покрыта `max_core/tests/p7.rs`.
- **Валидация:** `cargo check --workspace` — зелёный; `cargo test -p max_core
  -p max_app --no-default-features` — 1689 тестов, 0 failed.

## Реализация этапа 2 (2026-11-25, Разработчик)

Слой подачи миссий (M2/M3): подменю New Game и `GameSetupMenu`. UI остаётся
**презентацией**: он собирает выбор и отдаёт DTO, старт идёт потоком этапа 1
(`App::start_mission_from_registry`), `App.universe` не мутируется из UI.

- **Подменю New Game** (`menu_new_game_menu_loop`, `menu.cpp:2171-2320`;
  модуль `max_app::newgame`): семь кнопок оракула — Training Missions / Stand
  Alone Missions / Custom Game / Custom Scenarios / Campaign Game / Tips / Cancel
  (`_(182b)`/`_(02c9)`/`_(3d29)`/`_(884f)`/`_(878a)`/`_(3838)`/`_(7eec)`),
  геометрия `MENU_BUTTON_DEF`: 200×29 при x=385, шаг 35, Cancel 100×30 (435,420);
  заголовок `_(04b5)` «New Game Menu» (236,145); портрет `BGRSCRN*` + `P_*`.
  Горячие клавиши оракула `Shift+T/S/U/M/A/I/C`. `Custom` открывает прежнюю
  настройку партии (`menu_custom_game_menu`), `Cancel`/Esc возвращает в главное
  меню, а отмена самой настройки — в подменю (как цикл оракула).
- **Tips** (`menu_draw_tips`, `menu.cpp:1114-1204`): текст `_(e81f)` из
  `max_app/data/tips.txt`, обёртка 296 px в области (19,185) 296×236,
  `GNW_TEXT_FONT_5` (23 строки на страницу), листание `MNUUAROU`/`MNUDAROU`
  (16,438)/(48,438). Отличие подачи: под текстом заливается чёрная панель
  (в C++ текст блитится поверх случайного `P_*`-портрета и нечитаем).
- **GameSetupMenu** (`gamesetupmenu.cpp`; модуль `max_app::gamesetupmenu`):
  12 строк миссий (18, 184+20·i) 294×19 с зелёными разделителями
  (`gamesetupmenu.hpp:59-71`), панель описания (`SUBTITLE` (400,174),
  `BIGSCRNL`/`BIGSCRNR` (342,202)/(482,202), текст 266×201, 20 строк),
  стрелки страниц (16,438)/(48,438), стрелки брифинга (563,438)/(596,438),
  `Cancel` `MNUBTN4U` (200,438), `?` `MNUBTN5U` (312,438), `Start` `MNUBTN6U`
  (361,438). Заголовки категорий и help-секции — как `Init/EventHelp`
  (`TRAINING_MENU_SETUP`, `STAND_ALONE_MENU_SETUP`, `CAMPAIGN_MENU`,
  `MULTI_SCENARIO_MENU`). Выбор — 0-based индекс (`game_file_number - 1`),
  повторный клик по выбранной строке или `Start`/Enter отдаёт
  `Transition::StartMission { category, index }`; `main.rs`/QA-драйвер исполняют
  его через `App::start_mission_from_registry` с `LocalSetup` (имя игрока из
  настроек). Реестр строится один раз из `PATCHES.RES` (файлы миссий — данные
  рядом с `MAX.RES`).
- **Fallback отсутствующих файлов (условие владельца 5):** `MissionRegistry`
  оставляет дескриптор с предупреждением; строка списка рисуется красным с
  маркером `[missing]`, панель описания называет отсутствующий файл, `Start`
  остаётся на экране со статусом (`SAVE1.TRA is unavailable…`) и **не** стартует
  пустой мир. При отсутствии `PATCHES.RES` экран показывает «registry
  unavailable» и также отказывает в старте.
- **Гейты (тесты `max_app/tests/game_setup_menu.rs`, 12):** (1) исходники новых
  UI-модулей не содержат `universe_mut`/`_mut(`/`set_`; (2) DTO несёт только
  категорию и индекс; (3) все четыре категории стартуют из меню и дают
  побайтово тот же `snapshot_bytes`, что прямой `start_local_game_from_save`
  (включая Scenario 18 через листание страниц, старт и после 5 тиков);
  (4) 0/1/3/7 циклов открытия-закрытия меню не меняют `snapshot_bytes`;
  (5) отсутствующий файл виден и старт отклонён; Custom ведёт в настройку
  партии, Tips открывает окно, Cancel возвращает в меню.
- **Валидация:** `cargo check --workspace` — зелёный; `cargo test -p max_app
  --no-default-features` — 471 lib + все интеграционные, 0 failed; `qa_run
  visual-screens.qa` — PASS (451 кадр, 20 снимков), `qa_golden_check` — 20/20,
  0 дрейфа (новые эталоны `new-game.png`, `mission-select.png`; пересняты
  `options/planet/players` из-за текста статуса «Custom game…», edge-cases —
  из-за возврата отмены настройки в подменю). Скрипт `menu new_game` сохранён:
  драйвер проходит New Game → Custom, поэтому остальные сценарии не менялись.


