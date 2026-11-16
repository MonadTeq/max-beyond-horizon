## [Unreleased]

### Добавлено

- **P7 — предвысадочный экран мобилизации доведён до паритета с `CargoMenu`**
  (`docs/development/PLAN-production.md`, `max_app/src/mobilize.rs`):
  - **Изображения юнитов в списках.** Строки `Available`/`Purchased` рисуют
    ресурсы `I_*` (`Unit::GetIcon()`): 32×32, горячая точка в центре строки,
    масштаб как `ReportStats_DrawListItemIcon` (мобильные — 1:2, стационарные —
    1:4), перекраска командных слотов `ColorLut`. Загружаются из `MAX.RES`
    вместе с ассетами (`CargoMenuAssets::icons`, 71 тип).
  - **Индикаторы.** Полоса золота `gold_background` (361,276 40×140) с
    `VERTGOLD` снизу и числом кредитов сверху (`AbstractUpgradeMenu::DrawUnitStats`),
    cargo-слайдер `LimitedScrollbar` (421,301 20×115) с `VERTRAW`/`VERTFUEL`/
    `VERTGOLD` и счётчиком `5·N` (421,276 20×10); 1 кредит за 5 единиц cargo,
    ёмкость `storage/5` (у `GOLDTRCK` — 0), бесплатный cargo `SC_L0001` не
    снимается.
  - **Полный список покупки.** `CargoMenu::unit_types1` — 15 типов, 9 видимых
    строк по 32 px, прокрутка `BLDUP__`/`BLDDWN`, цена строки — ставка
    `CONSTRCT`/`TRAINHAL`/`LANDPLT` × `turns` (`PurchaseTypeSelector::Draw`),
    цена покупки — `3 × turns` текущих значений (`BuyUnit`), возврат при продаже
    — та же цена + `cargo/5`.
  - **Режим модернизации.** Переключатель `CRGTOG` (542,446; подписи
    `Buy Units`/`Upgrades` цветами `Fonts_DarkOrageColor`/`Fonts_GoldColor`),
    полный список `IsUnitFiltered`/`IsBetterUnit` (62 типа по умолчанию),
    фильтры `SELGRD`/`SELAIR`/`SELSEA`/`SELBLD`/`SELCBT` (в покупке — `CRGSELDX`),
    колонка апгрейдов `UPGLFT`/`UPGRGT` со степенями `UpgradeControl`
    (`Attack/Rounds/Range/Ammo/Armor/Hits/Scan/Speed`, шаг 1/2/5/10, лимит 800)
    и ценами `UpgradeControl_CalculateCost`; `Done` списывает кредиты и
    фиксирует `UnitValues` всех юнитов типа **до высадки** (`TeamDeployment::upgrades`
    → `TeamUnits.current_values` → `stats_gold_spent_on_upgrades`).
  - **Тесты.** `max_app/tests/cargo_vectors.rs` — бит-в-бит сверка с новым
    C++-генератором `tools/cargo_vectors` (через `tools/gen_cargo_data.py`):
    15 цен покупки, 93 цены списка, 7 наборов списка модернизации (фильтры),
    строки и первые стоимости апгрейдов всех 93 типов; `tests/cargo_render.rs` —
    отрисовка (иконки, бары, стрелки, `CRGTOG`, прокрутка); 19 UNIT-тестов
    `mobilize`; `start_phases::mobilization_upgrades_reach_the_deployed_units`.
  - **QA.** Новый сценарий `cargo-full.qa` (покупка из конца полного списка,
    cargo, переключение в модернизацию, фильтры, апгрейд, `Done`), команды
    `mobilize mode|filter|select|loadout|row|cargo`, проверки
    `assert cargo-mode/cargo-types/cargo-credits`; golden-кадры
    `golden/cargo/{cargo-buy,cargo-upgrade,cargo-upgraded}.png` и обновлённый
    `golden/mobilize.png`; `ci.sh`/GitHub CI прогоняют `cargo-full` и его golden.

## [1.1.0] — 2026-11-16

### Добавлено

- **P6 — исследования/апгрейды, финальный QA-свип и релиз v1.1.0**
  (`docs/development/PLAN-production.md`):
  - **Исследования и покупные апгрейды (последний геймплейный пробел).**
    `max_core::research` — порт `researchmenu.cpp`/`upgradecontrol.cpp`/
    `teamunits.cpp`: `ResearchMenu_CalculateFactor`, ходы исследования
    `pow(base+0.1,7.5)…`, дельты `ResearchMenu_ApplyUpgrades` для всех 93 типов,
    `UpgradeControl_CalculateCost`/`TeamUnits_UpgradeCost`. `pow(x,7.5)`
    считается **целочисленно** (Q48 + целочисленный `sqrt`), без `f64`;
    эталонные векторы `tools/upgrade_vectors` (C++ `std::pow`) сгенерированы для
    135 452 строк и сверяются бит-в-бит в `max_core/tests/research.rs`
    (58 086 стоимостей, 656 ходов, 61k дельт).
  - **Ход исследований в симуляции:** `GameState::advance_research` вызывается
    из `finish_turn` перед производством (как `ResearchMenu_NewTurn` в
    `GameManager_ProgressBuildState`); питание `RESEARCH`-центра
    (`Power On`, пакет 08/P1) делает `UpdateResearchProgress(+1)`, выключение и
    диверсант — `-1`. `manage_economy` инициализирует `turns_to_complete`
    нового уровня.
  - **Экран Upgrade 1:1:** `max_app::upgrademenu` — фон `UPGRADE` 640×480,
    портрет (11,13), статистика (11,293), цены (321,293), золото (361,276),
    стрелки `UPGLFT/UPGRGT` (283/301, шаг 19), список типов 482,80 (9×32),
    фильтры `SELGRD/AIR/SEA/BLD/CBT`, `Done/Help/Cancel`; клавиша `U`;
    модальный ввод; `Done` фиксирует `TeamUnits.current_values` и списывает
    золото, `Cancel` откатывает; в сети изменённые типы уходят пакетом **10**
    (пакета 09 в Rust-протоколе нет — ограничение задокументировано).
  - **Питание юнитов:** клавиша `O` — попап `Power On/Off` (пакет 08/P1),
    включает исследовательский центр.
  - **QA:** новые сценарии `upgrade.qa` (золото 150→142, значения типа,
    `Cancel` не меняет золото) и `research.qa` (Research Center за 12 ходов →
    питание → 16 ходов → `Attack` уровень 1); golden-кадры
    `golden/upgrade/`, `golden/research/`; QA-команды `upgrade …`,
    `assert upgrade=`, `assert gold`, `assert research <тема>=`.
  - **Финальный QA-свип:** все 🟡/❌ `suite/*.md` закрыты — 161 ✅, 0 🟡,
    18 ⏭ not planned с причинами (спектатор, mixed Rust+C++, FLIC, ручной
    WINDOW, сквозной урон); матрица покрытия в `max-rust-qa/suite/README.md`.
  - **Релиз:** workspace `max-rust` → **1.1.0**; пересобраны артефакты Windows
    UI/headless и Linux, обновлены `SHA256SUMS`/`release-checklist.md`.
  - **Тесты:** +16 `max_core/tests/research.rs`, +5
    `max_app/tests/upgrade.rs` (всего 780, 0 failed); полный `ci.sh`
    (тесты + QA + golden + soak) зелёный.

- **P5 — сетевая зрелость** (`docs/development/PLAN-production.md`):
  - **Три клиента.** `build/scripts/live-smoke-3.sh`: Horizon + C++-headless +
    3 Rust-клиента (`--min-players 3`, слоты RED/GREEN/BLUE), N ходов.
    Проверяет `пиров=2`, релей 00 от двух других команд каждому, прохождение
    00/52/54/45, `barrier_timeouts=0` и совпадение `state_checksum` у всех.
    Живой прогон 3×10 — **PASS**, CRC `0x34D5` у трёх клиентов.
    `horizon_probe` получил необязательный `min_players` (4-й аргумент).
  - **Реконнект без потери партии.** `max_app::net`: `SessionEvent::Disconnected`
    больше не завершает прогон после успешного ENet+TP00 — сессия продолжается;
    `TurnBarrierFsm::resume_state_sync` публикует 54 своей команды с последним
    `sync_index`, встречные 54 исправляют удалённые команды, барьеры 52/54/45
    доигрываются. `reconnect_pending_state=false` после resume; пределы (нет
    SNAP, 54 только своя команда/экономика, C++ ELIMINATED через 30 с) —
    `docs/architecture/protocol.md` §9.
  - **Тесты:** unit `resume_state_sync_publishes_the_last_completed_index`;
    in-process 3-клиентный мок-релей
    `three_clients_survive_disconnect_and_converge_via_state_sync` (B offline,
    копия его юнита испорчена, после возврата 54 владельца восстанавливает
    состояние; CRC/turn_counter совпадают у всех).
  - **C++-паритет — PARTIAL.** `build/scripts/live-smoke-mixed.sh`: Rust×2 + C++
    сервер как desync-оракул (Rust CRC `0x7035`, `Desync check failed` нет).
    Полная mixed-сессия заблокирована: headless `src/` умеет только `--server`,
    `--client`/`--connect` нет, GUI-клиенту нужен X11 и ручной ввод
    (`src/main.cpp`); блокер зафиксирован скриптом и документацией.
  - **Спектатор — невозможно без сервера:** нет `TEAM_TYPE_SPECTATOR`,
    `Remote_SetupPlayers` требует команду, поздний вход не получает снапшот
    (protocol.md §9.2). **MAX_LOG** (простой уровень/`target=level`) включает
    debug-логи для smoke без включения `env-filter`.
  - Документация: `protocol.md` §9/§11/§12, `barrier-flow.md` (P5-статус),
    `PLAN-production.md` P5, `build/scripts/README.md`,
    `max-rust-qa/suite/05-network.md`, `release-checklist.md` §3.1.
  - **Тесты:** 759 passed / 0 failed (`cargo test`, default; + sound и
    `--no-default-features` — зелёные).

- **P4 — стабильность и производительность клиента** (`docs/development/PLAN-production.md`):
  - **Soak 100+ ходов.** `max-rust-qa/scripts/soak-100.qa`: партия RED
    (человек, `end_turn` + `select`/`move`) против GREEN (Clueless, Duration
    200), 100 ходов, `assert turn=21/41/61/81/101`, `shot`/`mem` каждые
    20 ходов, `save_v70 soak-turn100.dta`. **PASS**: 12 208 кадров, V70-сейв
    157 921 байт, RSS 54 696 → 60 944 КиБ (+6.1 МиБ, плато после 20-го хода).
    Headless `max_app/examples/soak_headless.rs` (RED + 2 компьютера,
    `--turns 120`, `dt = 1/60`): **PASS**, 120/120 ходов, 362 502 кадра,
    21.8 с, RSS 53 548 → 53 708 КиБ (+160 КиБ, peak 53 884).
  - **Замер памяти** `max_app::mem`: RSS/пик без внешних крейтов (Linux
    `/proc/self/status` `VmRSS`/`VmHWM`, Windows `GetProcessMemoryInfo`);
    команда qa-драйвера `mem` + автоматические `# mem start/end` (с дельтой)
    в каждом `log.txt`; `soak_headless` пишет `soak.txt` со строкой на ход,
    итогом `SOAK RESULT`, `MEMORY` (порог стабильности 64 МиБ) и `PROFILE`.
  - **Профиль тика** `max_app::profile` (`MAX_TICK_PROFILE=1`): время каждого
    `App::local_update` и каждого `GameState::finish_turn`, среднее/максимум/
    число кадров сверх бюджета 60 Гц (16.67 мс) + номер хода максимума.
    QA-soak (debug): средний кадр 289 µs, max 15.69 мс, сверх бюджета 0;
    `finish_turn` на 100-м ходу 23.7 µs (max 56.6 µs). Headless 120 ходов
    (debug): средний кадр 59.9 µs, один планировочный выброс 121.9 мс (1 из
    362 502). Релизная сборка тех же 120 ходов: 2.47 с, средний кадр
    **6.76 µs**, max 5.21 мс, кадров сверх 16.67 мс — **0**, средний ход
    20.6 мс, RSS 51 544 → 51 824 КиБ (+280 КиБ); `finish_turn` avg 58 µs,
    max 5.19 мс. Порядок/арифметика симуляции не меняются — `Instant::now()`
    вокруг тех же вызовов.
  - **Оптимизация кадра без изменения детерминизма**:
    `App::update_visual_units` обновляет карту визуальных позиций на месте
    (`retain` + `insert`) вместо сборки `HashMap` каждый кадр; визуальные
    позиции не входят в CRC/снапшот — `AppDigest` и порядок симуляции те же.
  - **Краевые TC-11xx**: `max-rust-qa/scripts/edge-cases.qa` — 50 кликов по
    пустым областям главного меню + 50 по карте, не-ASCII `type` в чате и
    поле имени (отклоняется без паник, кадры `edge-unicode-*`), выход →
    New Game → Cancel → полный второй старт (кадр `edge-second-game`);
    TC-1112 — `tests/app.rs::app_can_be_created_twice_and_start_exit_twice`
    (`App::new` дважды, один и тот же файл сейва, свежие `turn=1`/`ticks=0`,
    одинаковый digest); TC-1107 — явная нейтральность SDL-событий
    alt-tab/сворачивания `max_render::sdl_window::window_event_is_input_neutral`
    (+тест). `suite/11-edge-cases.md`: **13 ✅ / 1 🟡** (живой alt-tab —
    ручной WINDOW-пункт).
  - **Тесты**: `tests/soak.rs` (детерминизм soak, профиль видит каждый кадр,
    бюджет 60 Гц, память), `tests/qa_driver.rs` (команда `mem`, проверка
    структуры `soak-100.qa` — 100 `end_turn`/кадры/финальный assert,
    прогон `edge-cases.qa`), unit-тесты `profile`/`mem`/`window_event_is_input_neutral`.
  - **CI**: `ci.sh` — стадия **4/4 SOAK** (`soak-100.qa` с
    `MAX_TICK_PROFILE=1` + `soak_headless --turns 120`); job `soak` в
    `.github/workflows/ci.yml` (`timeout-minutes: 45`, push/ручной запуск,
    не PR) с артефактами `soak.txt`/`log.txt` при падении; `edge-cases.qa`
    добавлен в стадию 3/4 и job `qa`.
  - Документация: `docs/development/PLAN-production.md` (P4 закрыт),
    `max-rust-qa/scripts/README.md` §P4 (команды, метрики, пороги),
    `max-rust-qa/plans/qa-plan.md`, `docs/development/release-checklist.md`
    §2.9 (свидетельства soak), `max-rust-qa/suite/11-edge-cases.md`.

- **P3 — запись V70-сейвов из живой партии** (`docs/development/PLAN-production.md`):
  - `max_core::save_write`: `build_save_file(&GameState) -> SaveFile`,
    `GameState::to_save()`/`to_save_bytes()`, `SaveMeta` — мост
    `GameState → SaveFile` для **всех** структур V70: `SaveFormatHeader`
    (тип/имя/планета/миссия/имена и типы команд/клан/seed/таймеры/`PlayMode`),
    `IniOptions` (12 полей), `IniPreferences`, `SurfaceMap`/`ResourceMap`,
    `TeamInfo`×4 (+UI-поля `markers`/`screen_locations`/`selected_unit`/зум/
    камера/кнопки), `GameManagerState`, `TeamUnits`×4 (золото, 93 базовых +
    93 текущих `UnitValues`, комплексы), пять списков `UnitInfo` со всеми
    V70-полями (включая упаковку `experience` в `storage` у
    `COMMANDO`/`REGENERATING_UNIT` и `recoil_delay`), `UnitHash`, `MapHash`,
    тепловые карты ×4, журналы сообщений ×4, `AiPlayer` компьютерных команд
    (стратегия, spotted units, `info_map`/`mine_map`, `target_location`).
  - Объектные номера V70 пересобираются обходом C++-писателя (`TeamUnits` —
    сначала все базовые значения, затем текущие и комплексы; далее списки
    юнитов, тело юнита — путь → `base_values` → комплекс → `parent_unit` →
    `enemy_unit`), identity `UnitValues`/`Complex` — по значениям,
    `UnitHash`/`MapHash` — порядок из загруженного файла (`SaveMeta`).
    Чего рантайм не хранит — переносится в `SaveMeta` как V70-поля
    (`AiPlayer::unused_v70`/`target_team`, сырой
    `stats_gold_spent_on_upgrades`, наличие `info_map`/`mine_map`), UI-поля
    получают дефолты `CTInfo::Reset`, тепловые карты пишутся как `i8`
    (C++ клампит отрицательные при загрузке).
  - **Побайтовая сверка**: `SAVE1.CAM` и `SAVE2.CAM` → `GameState` →
    `to_save` → байты **идентичны** исходным файлам
    (`max_core/tests/save_write.rs`), плюс эквивалентность всех ключевых полей
    после цикла и стабильность «save → load → save».
  - `max_app::saveload` — окно сохранения/загрузки 1:1 по
    `saveloadmenu.cpp`: фон `LOADPIC`, 10 слотов 200×70 (`FILE1_UP`…`FILE9_UP`,
    `FILE10UP`) в сетке `402*(i/5)+16, 76*(i%5)+44`, страницы по
    10 (`MNUUAROU`/`MNUDAROU`), кнопки `Save`/`Load`/`Cancel`
    (`MNUBTN2U`/`MNUBTN4U`/`MNUBTN6U`), превью слота (имя, ход, планета,
    команды, число юнитов), ввод имени `char[30]`. Имена файлов — как в
    `SaveLoad_GetSaveFileName` (`save<slot>.dta|cam|hot|…`), каталог —
    `--save-dir` либо каталог `MAX.RES`.
  - Автосейв: конец раунда (`local_finish_round` → `App::autosave`), слот 10,
    имя «(Turn N)», отключается `AppConfig::with_autosave(false)` — как
    `SETUP.auto_save` + `GameManager_UpdateGui`. Ручные `S`/`L` в локальной
    партии (C++ `Alt+F`/`Alt+L`); в сети окно не открывается (пакет 16
    сохранения не портирован, загрузка запрещена и в оригинале).
  - QA: команда `save_v70 NAME` (настоящий V70, в отличие от контейнера
    `MXLS` команды `save`), сценарий `max-rust-qa/scripts/save-load.qa`
    (2 хода → `save_v70` → `load` в свежем состоянии → те же ход/юниты),
    кадры `save-menu`/`after-load`; прогон в `ci.sh`.
  - Тесты: `max_core/tests/save_write.rs` (6 тестов, включая побайтовые
    SAVE1/SAVE2), `max_app/tests/save_load.rs` (4 теста: сейв→загрузка в
    свежем `App`, автосейв слота 10, окно слотов/страниц/имени, горячие
    клавиши и запрет в сети), unit-тесты `save_write.rs`/`saveload.rs`.
  - Артефакты пересобраны: Windows headless (`build-windows.sh --no-wine`) и
    Linux (`release.sh`, features `sdl-from-source,sound`); актуальные размер и
    sha256 — в `SHA256SUMS`/`*.zip.sha256`/`*.tar.gz.sha256`. Windows UI
    (`max_app-*-windows-x86_64-ui`) **не** пересобран: `build-windows-ui.ps1`
    требует Windows-хост с MSVC, vcpkg `sdl3:x64-windows` и rustup — в этом
    окружении их нет (кросс-сборка SDL3 из Linux не поддерживается, см.
    README артефакта).

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

### Состав релиза 1.1.0 (всё с v1.0.0)

- **Режимы:** одиночная партия (человек + 1–3 ИИ), hot-seat 2–4 человека,
  сетевая игра через Horizon (2 и 3 клиента, реконнект), мобилизация,
  выбор точки высадки, победа/поражение (generic + миссии).
- **Визуал 1:1:** MAINPIC-меню, Options/Planet/Players/Clan, хром `FRAMEPIC`,
  тайлы `.WRL`, спрайты/тени, миникарта, курсоры, шрифты GNW, BuildMenu,
  Save/Load, UpgradeMenu, Game Over, golden-кадры в CI.
- **Геймплей:** приказы/движение/бой/реактивный огонь, экономика и
  производство (очереди заводов, `SpawnNewUnit`, ставки C++), исследования и
  покупные апгрейды (P6), ИИ (задачи/стратегия/флот).
- **Сейвы:** V70 чтение/запись байт-в-байт (SAVE1/SAVE2), автосейв, окно
  Save/Load 1:1, V70-сейв нашей партии грузится обратно.
- **Сеть:** лобби Horizon, TP00/03, релей 00/52/54/45, CRC-барьеры, 3 клиента,
  реконнект через 54; C++ mixed — PARTIAL (блокер headless client).
- **QA/стабильность:** 100-ходовый soak, бюджет 60 Гц, память, edge-кейсы,
  golden-дрейф, полный `ci.sh`; детерминизм RNG/карт/симуляции.
- **Сборка/поставка:** rename проекта в **max-rust**; GUI-сборка по умолчанию
  (`cargo run -p max_app` открывает игру, bundled SDL3); Windows UI/headless и
  Linux-артефакты, `SHA256SUMS`, `release-checklist.md`.

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
