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

## Реализация этапа 3a (2026-11-25, Разработчик)

Прогрессия кампании (M5): `last_campaign`/`game_file_number` — **персистентное**
состояние настроек, а не UI-флаг; стори-экраны (M6) в этот этап не входят.

- **Оракул.** `settings.cpp:101-105` — `game_file_number` и `last_campaign` —
  `SETUP`-настройки со значением по умолчанию 1 (тот же файл настроек, что
  `preferences.ini` порта). `menu.cpp:518-545` — после **победы** в кампании
  менеджер миссий загружает следующую миссию списка, `game_file_number` получает
  её 1-based индекс, `last_campaign` поднимается до него (никогда не понижается),
  затем настройки сохраняются; победа в последней миссии (`next_it == end`)
  ничего не пишет. `menu.cpp:1968-1983` (`GameSetupMenu_Menu`) — `last_campaign`
  зажимается сверху по числу миссий, зажим пишется в настройки, и список
  открывается на `game_file_number = last_campaign`. `gamesetupmenu.cpp:257-298,
  402-423` — `DrawMissionList`/`FindNextValidFile`: миссия заблокирована, пока её
  0-based индекс `>= last_campaign`; заблокированная строка остаётся в списке, но
  неактивна и без заголовка, листание и выбор ограничены числом открытых миссий.
- **Где живёт состояние** (`max_app::preferences`): новые поля `Preferences::
  game_file_number`/`last_campaign` (`i32`, default 1), чтение/запись
  `preferences.ini`. Загрузка не зажимает значения — зажим является правилом
  меню (`campaign::menu_open`), поэтому файл round-trip'ится **байт-в-байт**
  (unit-тест `preferences.rs::campaign_progress_roundtrips_the_settings_file_byte_for_byte`
  + интеграционный повтор записи). В снапшот/CRC настройки не входят.
- **Модуль `max_app::campaign`** (M5): `menu_open(last_campaign, mission_count)
  -> CampaignMenuOpen` (`menu.cpp:1968-1983`; зажим только сверху, отрицательное
  значение не открывает ничего), `victory_advance(current_index, count, prefs)
  -> Option<CampaignAdvance>` (`menu.cpp:518-545`, `last_campaign` — high-water
  mark), `App::advance_campaign_after_victory` (вызывается из
  `App::leave_local_game`, т.е. в точке возврата C++-обёртки в главное меню после
  экранов победы), `App::apply_campaign_clamp` (зажим из меню),
  `App::set_campaign_mission_count` и `campaign_mission_count`.
- **Старт миссии** (`max_app::mission`): `start_mission_from_registry` фиксирует
  0-based позицию миссии в реестре (как `MissionManager::GetMissionIndex`) и число
  кампанийных миссий, а также `game_file_number = index + 1` (`menu.cpp:2058`,
  для любой категории — как в оракуле). Загрузка V70-сейва восстанавливает
  позицию из 1-based `mission_index` заголовка (`saveload.cpp:772`); установка
  нового мира (`adopt_universe`) её сбрасывает. Драйверы (`main.rs`,
  `qa/runner.rs`) сохраняют `preferences.ini` после старта миссии (условие
  владельца 1: `game_file_number` переживает перезапуск).
- **`GameSetupMenu` (Campaign)**: `MissionSelectForm` получил
  `campaign_unlocked` (это `last_campaign`; для остальных категорий — все миссии),
  `open_campaign`, `unlocked_count`, `is_locked`; заблокированные строки видны,
  но без заголовка, не выбираются кликом/стрелками, `Start` отказывает
  («Mission N is locked…»), страницы ограничены открытыми миссиями. Без реестра
  (`PATCHES.RES` недоступен) список пуст и **не** зажимает сохранённую
  прогрессию. Открытие кампании только *сообщает* о зажиме
  (`UiContext::campaign_clamp`), запись выполняет драйвер через
  `App::apply_campaign_clamp` (правило C3: UI не мутирует приложение); после
  возврата из партии драйвер синхронизирует зеркало `ctx.preferences` из `App`.
  `handle_session_input` и сетевой драйвер не менялись.
- **Гейт** (`max_app/tests/campaign_progress.rs`): победа Campaign 1 (через экран
  победы → `Enter`/`LeaveSession`, как `main.rs`/`QaRunner`) →
  `last_campaign=2`, `game_file_number=2`, файл настроек на диске, свежий `App`
  (`load_preferences_from_disk`) читает 2 (перезапуск), повторное сохранение даёт
  те же байты; поражение и последняя миссия прогрессию не двигают; список
  кампании открывает ровно `last_campaign` миссий (заблокированная строка видна,
  не выбирается, не стартует); end-to-end Campaign 1→2 через меню даёт
  побайтово тот же `snapshot_bytes`/digest, что прямой
  `start_local_game_from_save` `SAVE2.CAM` (включая 5 тиков); `last_campaign`
  выше списка зажимается и пишется; прогрессия не входит в снапшот.
- **Валидация:** `cargo check --workspace` — зелёный; `cargo test -p max_app
  --no-default-features` — зелёный; `qa_run save-load.qa` и
  `qa_golden_check` — PASS без дрейфа.

## Реализация этапа 3b (2026-11-25, Разработчик)

Стори-экраны кампании (M6): intro/victory/defeat по `menu_draw_mission_story_screen`
(`menu.cpp:766-864`). Прогрессия этапа 3a не менялась: стори-экран только
откладывает действие драйвера (вход в партию / возврат в меню), а
`App::leave_local_game` вызывается там же, где раньше.

- **Оракул и порядок.** `menu.cpp:2065-2071` — интро показывается после выбора
  миссии и перед стартом партии; `menu.cpp:495-512` — victory/defeat после
  экрана победы/поражения и **до** кампанийного wrap-up `menu.cpp:514-545`.
  Экран: текст `GNW_TEXT_FONT_1`, ширина 600 px, область (20,20) 600×400,
  цвет `Fonts_BrightSilverColor`; `ENDOK` 72×19 в (293,458), Enter/Esc
  закрывают текст; далее `story.video` проигрывается `Movie_Play` (ресурс
  `MAX.RES` — ASCIIZ-имя `*.MVE` рядом с архивом); `story.music` ставится до
  текстового окна; фон — `menu_briefing_backgrounds` (`ENDGAME1`…`ENDGAME9`)
  или список `story.background`.
- **Модуль `max_app::story`** — презентация без `GameState`: `StoryScreen`
  (текст + декодированный фон `BigImage` + пути видео + тег музыки + видимые
  диагностики), `StoryKind::{Intro,Victory,Defeat}`, `StoryPhase::{Text,Video}`,
  `StoryContinuation::{EnterSession,LeaveSession}`.
  `StoryScreen::for_mission(descriptor, kind, seed, max_res, data_dir)` берёт
  `StoryBlock` из `MissionDescriptor` (готов в `max_assets`) и возвращает
  `None`, если блока нет (`HasIntroInfo`/`HasVictoryInfo`/`HasDefeatInfo`).
  Видео-ID разрешается через `ResArchive::file_reference` (`DEMO2FLC` →
  `maxmve2.mve`), файловые `Media` — относительно каталога данных; музыка-ID
  уходит в `MusicDirector`, файловая форма — диагностика (в оракуле
  `/// \todo`). Фон выбирается детерминированно из `seed` (1-based
  `mission_index`); RNG симуляции не трогается (как у фона экрана победы).
- **Драйверы** (`main.rs`, `qa/runner.rs`): старт миссии выполняется
  **до** интро-экрана (мир уже загружен, `Screen::MissionStory` — слой подачи
  перед `Screen::InSession`), поэтому отсутствие стори-файла физически не
  может заблокировать старт; после закрытия экрана драйвер входит в партию
  (`StoryFinished`). После `GameOver` `LeaveSession` сначала показывает
  victory/defeat, затем возвращает в главное меню и вызывает
  `App::leave_local_game` (прогрессия — как в C++, после стори). Дескриптор
  запоминается на время партии; для миссии, загруженной из сейва, он ищется по
  заголовку V70 (`mission_category` + 1-based `mission_index`). Во время
  стори-экрана `local_update` не вызывается: `snapshot_bytes`/CRC не меняются.
  `handle_session_input` не менялся; в сетевом UI-sink (`UiSessionSink`) добавлена
  только пустая ветка нового `Transition::StoryFinished` для исчерпывающего
  `match` — логика сети/сессии не менялась.
- **Музыка** (`max_app::music`): `MusicDirector::play_story(tag, audio)` —
  презентационный оверрайд: музыка сцены приостанавливается, на закрытии
  `end_story` возвращает трек сцены; `disable_music` — no-op. Трек истории
  `WINR_MSC` (`SC_C0009`) идёт через тот же `AudioPlayer`, что и остальная
  музыка.
- **Видимые fallback'и (условие владельца 2).** Отсутствующий фон/видео/музыка
  попадают в `StoryScreen::diagnostics` (панель внизу экрана с чёрной
  подложкой), в `ctx.status`/`eprintln!`/`tracing`, экран продолжает
  показывать текст, а миссия уже стартовала. Без `MAX.RES` текст рисуется на
  чёрном фоне (не пустой кадр). Отсутствие самого стори-блока — не ошибка:
  экрана нет, миссия идёт как раньше.
- **Отличия подачи (задокументированы в модуле).** (1) Текстовый бокс оракула
  обрезается до 23 строк (`Text_SplitText`); порт листает текст
  Up/Down/PageUp/PageDown со счётчиком страниц — сценарий `SC_C0001` полностью
  укладывается в первую страницу после отбрасывания хвостовых пустых строк.
  (2) Панель диагностик внизу — добавлена для видимости fallback'а.
  (3) Печатающая машинка (`Text_TypeWriter`) не анимируется: страница
  рисуется целиком, чтобы `dt = 0` в QA был детерминирован.
- **Гейт** (`max_app/tests/mission_story.rs`, 7 тестов): старт Campaign 1 с
  показом/закрытием стори-экрана и без него даёт побайтово тот же
  `snapshot_bytes`/digest (на старте и после 5 тиков); сквозной прогон реального
  драйвера (`QaRunner`: сценарий New Game → Campaign → intro-story → InGame)
  даёт то же состояние, что прямой `start_local_game_from_save`; отсутствие
  `maxmve2.mve` видно в диагностиках, экран рисуется, миссия стартует; реальные
  блоки `SC_C0001` (текст/`ENDGAME2`/`DEMO2FLC`) и `SC_C0009`
  (текст/фон/`WINR_MSC`) декодируются; миссия без блока не открывает экран;
  кадр `dt = 0` детерминирован, Enter запускает видео, Esc завершает стори.
  Плюс 7 unit-тестов модуля (перенос текста, пагинация, диагностики, кадр без
  ассетов) и тест `MusicDirector` на приостановку/возврат трека сцены.
- **Валидация:** `cargo check --workspace` — зелёный; `cargo test -p max_app
  --no-default-features` — зелёный (483 lib-теста + интеграционные, 0 failed);
  `qa_run visual-screens.qa` — PASS, `qa_golden_check` — 20/20, 0 дрейфа;
  новый `qa_run story-screens.qa` — PASS, 3 новых эталона
  (`story-mission-select.png`, `story-intro.png`, `story-intro-video.png` в
  `max-brh-qa/golden/story-screens/`), сверка 3/3 без дрейфа.


## Замечание о коммитах этапа 3

Этапы 3a (прогрессия) и 3b (стори-экраны) выполнялись параллельно на общих файлах
проводки (ui.rs/main.rs/qa/runner.rs), поэтому чистый split на два коммита был
невозможен — закоммичены одним коммитом 863c61f. Атрибуция сохранена раздельными
тест-файлами: прогрессия — campaign_progress.rs, стори-экраны — mission_story.rs.
При будущем рефакторинге проводки UI — разделить.
