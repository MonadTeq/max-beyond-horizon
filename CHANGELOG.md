## [Unreleased]

### Добавлено

- **P2 — производство и строительство** (`docs/development/PLAN-production.md`):
  - `max_core::production` — порт `buildmenu.cpp`/`unitinfo.cpp`/`game_manager.cpp`:
    таблица `max_builder_capability_list` из `default.game_rules.lua`
    (`BUILDABLE`: 7 строителей, 45 типов), `turns_to_build`
    (`BuildMenu_GetTurnsToBuild` с множителем сложности ИИ), `cost_per_turn`
    (`Cargo_GetRawConsumptionRate`), `max_possible_build_rate` (1×/2×/4×,
    `buildmenu.cpp:729` 1:1), `ProductionManager` (queue/enqueue/commit/
    cancel/progress/`SpawnNewUnit`), площадка `find_build_site` и свободная
    клетка выпуска (детерминированный обход).
  - Экономика: за ход списывается ставка C++ с **золота команды** (сырьё
    комплексов — TODO(P3)); при 1× суммарная стоимость = `Cargo_GetRawConsumptionRate
    (builder, 1) × ATTRIB_TURNS` (= `GetNormalRateBuildCost`/`CargoMenu::BuyUnit`).
    Нехватка золота ставит постройку на паузу, заказ не теряется; отмена —
    `CancelBuilding` (очередь очищается, `build_time = 0`).
  - `GameState::finish_turn` теперь выполняет ход производства для играющих
    команд в порядке `RED..GRAY`: списание (`ManageEconomy`) → спавн по спискам
    `GroundCover → MobileLandSea → Stationary → MobileAir`
    (`GameManager_ProgressBuildState`). События — `GameState::take_production_events`
    (журнал HUD).
  - `max_app::buildmenu` — экран `BuildMenu` 1:1: фон `FACBUILD`/`CONBUILD`
    (640×480), списки 483,60 (11×32) и очередь завода 337,60 (6×32), кнопки
    `BLDBLD`/`BLD2X`/`BLD4X`/`BLDONE`/`BLDCAN`/`BLDUP__U`/`BLDDWN_U`/
    `BLDDEL_U`/`BLDREP_U`/`BLDDES_U` по координатам C++, портрет-спрайт,
    цифры «ходов/цены». Клавиша `B` открывает экран для выделенного
    конструктора/инженера/завода; экран модален (`GameManager_DisableMainMenu`).
    Заказ уходит `Command::Order` с `PayloadP4` (пакет 08) — та же схема, что в
    сети (`build_list`, `build_time`, `build_rate`, `repeat_build`).
  - ИИ: `builder_type_for`/`turns_to_build` делегируют в `production` (единая
    таблица `max_builder_capability_list`), очередь ИИ теперь доходит до спавна.
  - Тесты: `max_core/tests/production.rs` (13 тестов: таблицы, цены, очередь
    за N ходов, ускорение 2×, отмена, пауза без золота, детерминизм,
    `finish_turn`), `max_app/src/buildmenu.rs` (unit), AI-тест
    `ai_completes_a_build_queue_through_production`.
  - QA: новые команды `build open|select N|add|delete|rate N|repeat|step N|
    queue step N|done|cancel`, `select last`, `select type NAME`,
    `assert build=open|closed`, `assert unit=NAME`; сценарий
    `max-rust-qa/scripts/production.qa` (конструктор → BuildMenu → TRAINHAL за
    10 ходов → завод → INFANTRY за 2 хода), golden-кадр
    `max-rust-qa/golden/production/buildmenu.png`, прогоны в `ci.sh` и
    `.github/workflows/ci.yml`.

### Исправлено

- **TC-1109 — поиск данных игры от каталога exe**: `max_app::assets` получил
  общий порядок корней (`data_search_roots`): каталог `MAX.RES` и его родитель →
  текущий каталог и его родитель → каталог запущенного exe
  (`std::env::current_exe`) и вверх по дереву до ближайшего каталога с
  `max-original`. Резолверы `resolve_max_res` (новый), `resolve_patches_res`,
  `resolve_color_pal` ходят по этому списку; запуск `release\max_app.exe` из
  чужого CWD находит данные, а ошибка `MAX.RES` теперь подсказывает
  `--data-dir/--max-res`. Тесты: `assets::tests::exe_dir_resolves_game_data_when_cwd_is_elsewhere`,
  `exe_ancestors_are_searched_up_to_the_data_root`; `absent_everywhere…`
  сделан герметичным (пустые корни).

- **Фазы старта партии M-SP (порт `GameManager_PlayerMissionSetup`)**: после
  экрана `Players` партия больше не начинается сразу — человека ждут две фазы
  оригинала, для каждой человеческой команды в порядке C++ (убывание слота):
  - **мобилизация** (`CargoMenu`, фон `CARGOPIC`): список 15 стартовых юнитов
    (`CargoMenu::unit_types1`), цена `Cargo_GetRawConsumptionRate(LANDPLT, 1) =
    3` × `ATTRIB_TURNS` текущих значений команды, кредиты `start_gold`,
    покупка/продажа, `Buy`/`Delete`/`Cancel`/`Done`; бесплатная загрузка
    `SC_L0001` (`CONSTRCT` 40, `ENGINEER` 20, `SURVEYOR` 0) не удаляется. При
    `start_gold = 0` фазы нет (как C++). `max_app/src/mobilize.rs`.
  - **выбор точки высадки** (`GameManager_SelectSite`,
    `GAME_STATE_7_SITE_SELECT`): карта 112×112 в области 448×448 (4 px на
    клетку, зум 4 C++), курсор 4×4, проверка `GameManager_IsValidStartingPosition`
    (4×4 чистой суши), `Enter`/`Done` — подтвердить, `Esc`/`Cancel` — отмена,
    стрелки двигают курсор. `max_app/src/landing.rs`.
  - результаты фаз уходят в `LocalSetup::start_positions`/`team_loadouts`/
    `team_gold` → `NewGameConfig::deployments` → новая
    `deploy::deploy_starting_units_with`: база встаёт у выбранной точки,
    кредиты и состав — как куплено. ИИ-команды выбирают точку сами
    (`setup::ai_choose_starting_position` = `Ai_SelectStartingPosition`: 5×5
    суши, ≥ `AI_SAFE_ENEMY_DISTANCE`); их состав — стандартная загрузка.
  - QA: `mobilize buy|delete|step|done|cancel`, `landing click|grid|done|cancel`,
    `assert screen=Mobilize|Landing|Clan`; сценарии `sp-full.qa`/`hs-2.qa`/
    `victory.qa`/`visual-screens.qa` обновлены, новые golden-кадры
    `mobilize.png`, `landing.png`.
- **Выбор клана в настройке партии** (`ClanSelectMenu`): экран `CLANSEL` с
  8 карточками `CH_CN1..8_U/D` и логотипами `CLN1..8LOGO`, Random/Cancel/?/Done;
  выбранный клан пишется в `LocalSetup::team_clans`, попадает в
  `NewGameConfig::clans` и меняет стартовые атрибуты юнитов команды
  (`ResourceManager_InitClanUnitValues`). Контейнер `MXLS` — версия 3
  (`team_clans`), старые v2 читаются с кланами Random.
- **M-SP — одиночная игра против ИИ** (`docs/architecture/plan-modes.md`):
  - экран настройки 1:1 из трёх экранов C++-порта (`max_app/src/setup.rs`):
    `GameConfigMenu` (`OPTNFRM`: сложность ИИ, таймеры, режим игры, кредиты,
    уровни ресурсов, условие победы), `PlanetSelectMenu` (`PLANETSE`: 3×2
    мини-карты из `.WRL`, стрелки групп, описания 24 планет),
    `ChoosePlayerMenu` (`CHOSPLYR`: 4 слота Human/Computer/None, флаги команд
    с эмблемой клана);
  - локальный цикл (`max_app/src/local.rs`, порт `GameManager_TeamTurnTurnBased`):
    `manage_economy` → `Ai_BeginTurn` → тики ИИ до `end_turn_retry_counter > 120`
    → пакет `06` → следующая команда → `finish_turn`; End Turn, таймер хода,
    пауза; 1 человек + 1–3 компьютера, V70 читается в локальную партию;
  - экран победы/поражения 1:1 (`max_app/src/winloss.rs`):
    `menu_draw_game_over_screen` — `ENDGAME1..9`, места, статистика,
    `ENDARM`/`ENDOK`;
  - контейнер сохранения локальной партии `MXLS`
    (`App::local_save_bytes`/`write_local_save`/`read_local_save`): настройки +
    фаза + снимок состояния с CRC (запись `GameState → V70` — открытый пункт);
  - 12 сквозных тестов `max_app/tests/local_sp.rs`, кадры
    `cargo run -p max_app --example sp_frames`.

### Исправлено

- **Эмблемы кланов на экране `Players` и `Clans`**: логотипы `CLN0..8LOGO`
  рисуются с якорем (41,40) внутри кнопки, как `Button::Copy(id, 41, 40)`:
  собственные `ulx/uly` ресурса вычитаются
  (`WindowManager_DecodeSimpleImage`: `ulx -= image->ulx`). Раньше картинка
  рисовалась от `(41,40)` без поправки и уезжала вправо-вниз, вылезая за
  флаг/карточку (`max_app/src/menu.rs::blit_simple_anchored`).
- **Центровка знака `?`**: сверено с оракульным кадром C++-порта (Alt+C,
  `Players` 640×480) — подпись `?` в кнопке 48×40 совпадает пиксель-в-пиксель
  с `Button::SetCaption`/`Text_TextLine`; формула закреплена тестом.
- **Юниты плавно двигаются в кадре**: `App::update_visual_units` догоняет
  симуляционную клетку (`UnitInfo::Move` в оригинале двигает спрайт
  пиксельно); `dt = 0` (headless/QA) оставляет снимок без интерполяции, поэтому
  golden-кадры не дрейфуют.
- **Спрайты юнитов масштабируются вместе с картой** (`zoom / 64`): при
  удалении/приближении камеры юнит и тень уменьшаются/увеличиваются, горячая
  точка остаётся в центре клетки (`SpriteRenderer::draw_unit_scaled`).
- **Кнопки больше не выглядят нажатыми при наведении**: `*D`/тёмная подпись
  остаются только у выбранных radio/toggle-кнопок; hover не нажимает
  (`draw_image_button`, экраны команд, hot-seat, победа/поражение).
- **Декодер big image**: RLE начинается с байта 776, а не 777 (C++
  `WindowManager_DecodeBigImage` читает с `&image->transparent_color`).
  До исправления `FRAMEPIC` и `ENDGAME1..9` декодировались со сдвигом на байт
  («scanline»-артефакты); исправление затронуло хром игрового окна, фоны
  экрана победы и все big image меню.

### Изменено

- **GUI — режим по умолчанию: `default = ["sdl-from-source"]` в `max_app`**.
  `cargo run -p max_app` (и обычная `cargo build`) больше не требуют флагов:
  SDL3 3.4.20 берётся из vendored `sdl3-src` и собирается через CMake из
  исходников — системный SDL3 и vcpkg не нужны, нужны только C-компилятор и
  `cmake`. Раньше `default = []`, и запуск без флагов падал с
  «GUI-режим … требует сборки с feature sdl». Headless-сборки явно собираются
  с `--no-default-features` (`build/scripts/build-windows.sh`, `ci.sh`, QA);
  явные оконные фичи (`sdl`, `sdl-vcpkg`) комбинировать с default нельзя —
  их тоже нужно включать через `--no-default-features`, иначе добавится
  `sdl-from-source` (`build/scripts/build-windows-ui.ps1`, `release.sh`).
  Звук (`sound`) **не** входит в default: rodio/cpal тянет ALSA/WASAPI —
  включается явно `--features sound`.
- **CI/Docker: системные зависимости для сборки SDL3 из исходников**.
  `ci.sh` и `.github/workflows/ci.yml` ставят dev-заголовки X11/Wayland/ALSA/
  GL/KMSDRM/dbus/udev (набор из `docs/development/release-checklist.md` §2.3);
  `cargo test` идёт с `LD_LIBRARY_PATH=target/debug` (там `libSDL3.so.0`).
  QA-примеры (`qa_run`, `qa_golden_check`) собираются с
  `--no-default-features` — окно им не нужно. Добавлен `cargo test
  --no-default-features` (headless-путь и cfg-ветки без feature `sdl`) и
  `cargo build … --no-default-features` в `release.sh` для headless-релиза.
  Заодно исправлены пути QA-сценариев в
  `ci.sh`/`ci.yml`/`release-checklist` (`../max-rust-qa/scripts/…`,
  `../max-rust-qa/golden` — каталога `max-rust/qa/` давно нет, из-за чего
  стадия 3/3 не находила скрипты) и путь артефактов QA в workflow.
  `ci.sh`/`release.sh` экспортируют `MSYS_NO_PATHCONV` (иначе в Git Bash
  `docker run -w /src/max-rust` ломался на `C:/Program Files/Git/...`).
- **`max-rust/.cargo/config.toml`: 8 МиБ стека для Windows-целей**.
  У главного потока PE-стек 1 МиБ, а debug-сборка headless-режимов
  (`--demo`/`--new-game`) в него не влезала — `thread 'main' has overflowed its
  stack`; release-сборка работала. `-C link-arg=/STACK:8388608` (msvc) и
  `-Wl,--stack,8388608` (gnu) выравнивают поведение с Linux.
- **Windows GUI-сборки без консольного окна**: `max_app/src/main.rs` —
  `#![cfg_attr(feature = "sdl", windows_subsystem = "windows")]`; фичи
  `sdl-vcpkg`/`sdl-from-source` включают `sdl`, поэтому все оконные сборки
  получают **PE Subsystem=2** (двойной клик — только окно SDL3), а headless
  без `sdl` остаётся **Console (3)**: `--demo`/`--new-game`/`--connect` и
  пример `qa_run` по-прежнему печатают в терминал/CI. `max-resource-editor` —
  безусловный `#![windows_subsystem = "windows"]`; чтобы headless-режимы не
  теряли вывод, итог и ошибки дублируются в файлы
  (`<OUT_ROOT>/export-log.txt`, `./report.txt`,
  `./max-resource-editor-error.txt`). Сборки проверяют подсистему PE
  (`build/scripts/pe-subsystem.ps1`), `build-windows.sh` — console для
  headless; README/чек-лист обновлены.
- **Бинарь клиента переименован `max_app` → `max-rust`** (Windows:
  `max-rust.exe`). Крейт и библиотека остаются `max_app`, примеры
  `qa_run`/`qa_golden_check` не тронуты; `cargo run -p max_app` по-прежнему
  работает (бинарь в пакете один), а артефакты теперь `max-rust(.exe)`.
  Каталоги артефактов (`max_app-<version>-…`) сохранены.
- **Запуск без флагов открывает GUI** (меню SDL3): headless-режимы доступны
  только по явным `--demo`, `--new-game`, `--connect`; `--ui` оставлен
  совместимым alias. В headless-сборке (`--no-default-features`, без feature
  `sdl`) запуск без флагов даёт понятную ошибку с подсказкой собрать без
  `--no-default-features` и headless-флаги.
- Help-текст, build/release-скрипты и документация переведены на
  `max-rust.exe`; Windows UI-артефакт содержит `max-rust.exe` + `SDL3.dll`.

## [1.0.1] — 2026-11-15

- Исправлено зеркальное отображение текста UI (порядок битов глифов font8x8 — LSB-first); пересобраны Windows UI- и Linux-артефакты.

## [1.0.0] — 2026-11-15

Первый релизный срез Rust-клиента: самостоятельный клиент M.A.X. под Horizon,
который собирается без SDL/ALSA для headless-проверок и включает SDL3-окно и
звук отдельными feature-флагами.

### Добавлено

- **M6.2 — звук**:
  - `max_assets::wav` — чистый stdlib-парсер RIFF/WAVE PCM16 (моно/стерео),
    возвращает `WavClip { sample_rate, channels, samples: Vec<i16> }`;
    пропускает `smpl`/`fact`/неизвестные чанки, не паникует на битых данных;
  - `max_assets::res::parse_file_ref` и `ResArchive::file_reference` — чтение
    ASCIIZ-ссылок `MAX.RES` (`TANK1` → `"tank1.spw"`);
  - `max_app::audio` — `AudioBackend` (`NoopBackend` всегда, `RodioBackend` за
    feature `sound`), `AudioPlayer` с кэшем, `play_once`/`play_loop` и
    регистронезависимым поиском файлов рядом с `MAX.RES`;
  - клики меню в `--ui` озвучиваются (`KBUY0` → `kbuy0.spw`); звук не блокирует
    игровой цикл и молча деградирует до no-op без устройства/файлов.
- **M6.4 — релиз**:
  - версия workspace `max-rust` — **1.0.0** (все пять крейтов наследуют её);
  - `docs/development/release-checklist.md` — полный чек-лист релиза;
  - `release.sh` — сборка Linux-бинаря в Docker, упаковка `release/` с
    `README.txt`, `LICENSE`, `CHANGELOG.md`, `SHA256SUMS` и `.tar.gz`;
    подготовка/создание тега `v1.0.0` по флагу `--tag`;
  - `ci.sh` и GitHub Actions проверяют сборку с feature `sound`.

### Клиент умеет

- Загружать данные оригинальной игры: `MAX.RES`, `PATCHES.RES`, `COLOR.PAL`,
  карты `*.WRL`/ресурсы `SNOW_1`…`DESERT_6`, спрайты и тени юнитов.
- Играть локальную партию: `setup::generate_map`, высадка, ходы, приказы,
  победа/поражение (`max_core` — детерминированная модель).
- Работать по сети: Horizon gRPC → билет → ENet-сессия, лобби (создание/список/
  вход), приказы 08/38/41/50, чат 18, CRC-барьеры 45/52, реконнект транспорта
  (ENet + `TP 00`) с настраиваемыми попытками.
- Показывать SDL3-окно с меню, картой, HUD, миникартой, панелью юнита,
  журналом и предпросмотром маршрута (feature `sdl` / `sdl-from-source`).
- Играть звуки меню (feature `sound`).

### Ограничения 1.0.0

- Реконнект восстанавливает только транспорт; повторный вход в идущую партию
  (SNAP/54-восстановление) и спектатор — после 1.0.0 (M6.1-финал).
- Видеоролики FLC/MVE не воспроизводятся.
- Озвучены только клики меню; звуки юнитов/выстрелов/взрывов декодируются
  (`WavClip`), но точки вызова в игровом цикле появятся позже.
- Windows-бинарь в этом репозитории не собирается автоматически: инструкция —
  в `docs/development/release-checklist.md` §6 (нужен MSVC + vcpkg SDL3).
- Человеческий сценарий M4 и live-smoke 2×10 — гейты релиза, а не свойство
  кода: они проверяются по чек-листу на машине с SDL/данными игры.

### Этапы

| Этап | Итог |
|---|---|
| R1 | Каркас workspace, чистые границы крейтов, CI-заготовка. |
| R2 | Парсеры RES/WRL/спрайтов, WS-лобби Horizon, UDP-сессия, рендер карты. |
| M3 | Игровой цикл: тики, приказы, CRC-барьеры, два клиента против headless. |
| M4 | SDL3-окно, меню, создание/вход в игру, HUD, мышь/хоткеи, чат (18). |
| M5 | Horizon из конфига, docker-compose (Horizon + C++-headless), healthcheck. |
| M6 | Реконнект-заготовка, звук (feature), CI, версия 1.0.0, чек-лист, релиз. |

[1.0.0]: https://github.com/klei1984/max/releases/tag/v1.0.0
