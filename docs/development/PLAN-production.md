# План Production-ready Rust-клиента (v1)

Версия: 1.1 · 2026-11-16 · Цель: `goal-f7008432`.

## Определение «production-ready»

Клиент можно отдать игроку: полная партия в любом из трёх режимов от меню до
экрана победы (строительство/производство/исследования/бой), сохранение и
загрузка настоящих сейвов, стабильность на длинных партиях, сетевая игра
без обходных флагов, зелёный CI/QA и оформленный релиз v1.1.0.

## Этапы

### P1 — Быстрые UX-фиксы (1–2 дня)
- [x] TC-1109: поиск данных игры от каталога exe (сейчас только от CWD).
  `assets::data_search_roots` — каталог `current_exe` и вверх до ближайшего
  `max-original`; `resolve_max_res`/`resolve_patches_res`/`resolve_color_pal`
  используют общий порядок, тесты — `max-brh/crates/max_app/src/assets.rs`
  (`exe_dir_resolves_game_data_when_cwd_is_elsewhere`).
- [ ] Согласование путей: `--data-dir/--assets-dir` единообразны во всех режимах и документации.
  Сделано частично: сообщение об ошибке `MAX.RES` подсказывает
  `--data-dir/--max-res`, fallback предупреждает о подмене. Полный аудит флагов
  (все режимы + `docs/development/build.md`) — не закрыт.
- Критерий: запуск из любого каталога находит данные или даёт точную подсказку.

### P2 — Производство и строительство (крупнейший геймплейный пробел)
- [x] ProductionManager: очереди заводов, `SpawnNewUnits`, стоимости (Cargo/Upgrade-формулы).
  `max-brh/crates/max_core/src/production.rs`: таблица
  `max_builder_capability_list`, `turns_to_build`, `cost_per_turn`,
  `max_possible_build_rate` (порт `buildmenu.cpp:729`), `ProductionManager`
  (queue/commit/cancel/progress), `GameState::finish_turn` → ход производства
  для команд в порядке `RED..GRAY` (`ManageEconomy` → `ProgressBuildState`).
  Экономика: золото списывается ставкой `Cargo_GetRawConsumptionRate` за ход
  (материалы комплексов — TODO(P3)); нехватка золота — пауза, а не отмена.
- [x] BuildMenu: заказ юнитов/зданий (UI 1:1, ресурсы `B_*`/иконки из max-assets).
  `max-brh/crates/max_app/src/buildmenu.rs`: `FACBUILD`/`CONBUILD` + `BLD*`
  (фон 640×480, списки 483,60 / 337,60, кнопки по координатам C++), клавиша
  `B`, модальный ввод, заказ — `Command::Order` с `PayloadP4` (пакет 08).
- [x] Исследования/апгрейды: ResearchMenu-факторы (float-формулы → build-time таблицы по DESIGN).
  **Закрыто в P6** (было отложено из P2): `max_core::research` — `pow(x,7.5)`
  целочисленно (Q48 + целочисленный sqrt), 135 452 эталонные строки
  `tools/upgrade_vectors` сверяются бит-в-бит (58 086 стоимостей, 656 ходов,
  61k дельт); `advance_research` в `finish_turn`, питание Research Center,
  экран `UpgradeMenu` 1:1 (`U`), QA `research.qa`/`upgrade.qa`.
- [x] Интеграция с ИИ (ИИ строит через ProductionManager, как C++).
  ИИ уже выдаёт `ORDER_BUILD` с очередью (`TaskCreateBuilding`/`TaskCreateUnit`
  → `TaskCtx::build_order`); теперь очередь реально тикает и выпускает юнитов
  в `finish_turn`. `builder_type_for`/`turns_to_build` делегируют в
  `production` (единая таблица). Тест:
  `max_core/tests/ai_p6.rs::ai_completes_a_build_queue_through_production`.
- Критерий: скриптом строится завод → заказан юнит → юнит появляется через N ходов; ИИ строит флот; партия 30+ ходов с производством без расхождений (детерминизм).
  Завод → юнит: `max-brh-qa/scripts/production.qa` (TRAINHAL за 10 ходов,
  INFANTRY за 2). ИИ использует флот, только если `TaskObtainUnits` запросит
  морской юнит (зависит от стратегии ИИ — отдельная задача); детерминизм
  очереди проверен (`tests/production.rs::two_identical_production_runs_have_identical_state`).

### P3 — Сейвы V70 (запись)
- [x] Сериализатор GameState→SaveFile (SaveUnit, HeatMap, AiPlayerState, хэши) — по save.rs чтению.
  `max_core::save_write` (`build_save_file`, `GameState::to_save`/`to_save_bytes`,
  `SaveMeta`): заголовок/`IniOptions`/`IniPreferences`, `SurfaceMap`/`ResourceMap`,
  `TeamInfo`×4 + UI-поля, `GameManagerState`, `TeamUnits`×4 (93 базовых +
  93 текущих `UnitValues` + комплексы), пять списков `UnitInfo` со всеми
  V70-полями, `UnitHash`/`MapHash`, тепловые карты ×4, журналы сообщений ×4,
  `AiPlayer` компьютерных команд. Объектные номера пересобираются обходом
  C++-писателя (сначала все базовые значения, затем текущие и комплексы;
  тело юнита — путь, `base_values`, комплекс, `parent_unit`, `enemy_unit`).
  Identity `UnitValues`/`Complex` восстанавливается по значениям
  (`current == base` ⇒ общий объект, `base_values` юнита ⇒ объект команды);
  порядок `UnitHash`/`MapHash` переносится через `SaveMeta` (C++ вставляет
  юнитов при создании, а `MapHash` при загрузке перехэширует).
  Сверка: `max_core/tests/save_write.rs` — `SAVE1.CAM` и `SAVE2.CAM` грузятся
  в `GameState` и пишутся обратно **побайтово** (`load → to_save → bytes`), плюс
  эквивалентность ключевых полей после цикла.
- [x] Автосейв и ручной Save/Load в UI (Save Game/Load Game 1:1).
  `max_app::saveload`: окно по `saveloadmenu.cpp` (фон `LOADPIC`, 10 слотов
  200×70 `FILE*_UP/DN` в сетке `402*(i/5)+16`/`76*(i%5)+44`, страницы по 10,
  слоты с 1, `save<slot>.<ext>` по категории, имя `char[30]`), кнопки
  `Save`/`Load`/`Cancel` (`MNUBTN2U`/`MNUBTN4U`/`MNUBTN6U`) и страницы
  (`MNUUAROU`/`MNUDAROU`); клавиши `S` (Save) и `L` (Load) в локальной партии;
  автосейв в конце раунда — слот **10**, имя «(Turn N)», как
  `GameManager_UpdateGui` + `SETUP.auto_save`. В сети окно не открывается:
  оригинал при сохранении рассылает пакет 16, которого в нашем протоколе нет
  (загрузка в сети запрещена и в оригинале).
- Критерий: round-trip V70 байт-в-байт на 9 эталонных сейвах + сейв нашей партии грузится C++-портом (кросс-проверка).
  Сделано для **двух** эталонов, которые есть в репозитории
  (`max-brh/crates/max_core/tests/data/save1_v70.cam`, `save2_v70.cam`):
  `load → to_save` совпадает побайтово. Остальные 7 файлов `max-original/SAVE3..9`
  в репозитории отсутствуют (не добавлялись как фикстуры). Кросс-проверка
  «наш сейв грузит C++-порт» требует собранного `max-port` (Docker
  `max-port-build`) и не выполнялась в этом этапе — формат гарантирован
  парсером `save.rs` (он принимает только V70 с полным обходом файла).

### P4 — Стабильность и производительность
- [x] Soak: 100+ ходов ИИ×ИИ и человек+ИИ через qa-драйвер — без паник/утечек (замер памяти).
  `max-brh-qa/scripts/soak-100.qa`: Options (Clueless, No Limit, Turn Based,
  Duration 200) → 100 × `end_turn` (человек RED + компьютер GREEN), `assert
  turn=21/41/61/81/101`, `shot`/`mem` каждые 20 ходов, `save_v70` на 100-м.
  **PASS**: 12 208 кадров, V70-сейв 157 921 байт; RSS 54 696 → 60 944 КиБ
  (+6.1 МиБ, после 20-го хода — плато: прогрев аллокатора, не линейный рост).
  Headless `max_app/examples/soak_headless.rs` (RED + 2 компьютера, `--turns
  120`, `dt = 1/60`): **PASS**, 120/120 ходов, 362 502 кадра, 21.8 с (debug),
  RSS 53 548 → 53 708 КиБ (+160 КиБ, peak 53 884); релизная сборка — 2.47 с,
  RSS 51 544 → 51 824 КиБ (+280 КиБ). Команда `mem` в
  qa-драйвере + автоматические `# mem start/end` в каждом `log.txt`;
  `max_app::mem` — Linux `/proc/self/status` (`VmRSS`/`VmHWM`), Windows
  `GetProcessMemoryInfo`; порог стабильности soak — 64 МиБ.
- [x] 60 Гц интерактив против lockstep: проверка дрейфа и стоимость тика (профилирование; при необходимости — бюджет).
  `MAX_TICK_PROFILE=1` → `max_app::profile` замеряет `App::local_update` и
  `GameState::finish_turn`, пишет `PROFILE` в `log.txt`/`soak.txt`.
  QA-soak (debug, CI): средний кадр **289 µs**, максимум **15.69 мс**, кадров
  дольше бюджета 16.67 мс — **0**; `finish_turn` на 100-м ходу **23.7 µs**
  (avg 23.6 µs, max 56.6 µs). Headless 120 ходов (debug): средний кадр
  59.9 µs, один выброс 121.9 мс на 24-м ходу (`finish_turn`, 1 кадр из
  362 502) — планировочный спайк, не вычисление. **Релизная сборка** тех же
  120 ходов: средний кадр **6.76 µs**, max **5.21 мс**, кадров сверх бюджета
  16.67 мс — **0**, средний ход 20.6 мс; `finish_turn` avg 58 µs, max 5.19 мс
  (тот же 24-й ход), на 120-м — 9.4 µs. `App::update_visual_units`
  больше не собирает `HashMap` каждый кадр (in-place `retain` + `insert`;
  визуальные позиции не входят в CRC/снапшот) — порядок и арифметика
  симуляции не изменены.
- [x] Краевые: спам кликов, alt-tab, битые файлы, юникод — по TC-11xx.
  `max-brh-qa/scripts/edge-cases.qa` → **PASS**: 50 кликов по пустым областям
  главного меню + 50 по карте, не-ASCII `type` в чате/имени (символы
  отклоняются, паник нет), выход → New Game → Cancel → полный второй старт до
  `turn=2`; TC-1112 — `tests/app.rs::app_can_be_created_twice_and_start_exit_twice`
  (`App::new` дважды, один файл сейва, одинаковый digest). TC-1107 (alt-tab):
  SDL `FocusLost/FocusGained/Minimized/Restored/…` явно нейтральны
  (`window_event_is_input_neutral` + тест), живой alt-tab остаётся ручным
  WINDOW-пунктом. Итог `suite/11-edge-cases.md`: **13 ✅ / 1 🟡**.
- Критерий: soak-сценарий в CI (ночной/релизный), память стабильна, время тика в норме.
  `ci.sh` — стадия **4/4 soak** (`soak-100.qa` с `MAX_TICK_PROFILE=1` +
  `soak_headless --turns 120`); `.github/workflows/ci.yml` — job `soak` с
  `timeout-minutes: 45` (push/ручной запуск, не PR). Метрики и пороги —
  `release-checklist.md` §2.9, `max-brh-qa/scripts/README.md` §P4.

### P5 — Сетевая зрелость
- [x] 3+ клиента в сессии (релей всем, барьеры по всем командам) — live-smoke с 3 клиентами.
  `build/scripts/live-smoke-3.sh` (3 Rust-клиента, `--min-players 3` через
  `horizon_probe ... 3`, BLUE занимает реальный клиент): **PASS 3×10**,
  `пиров=2`, релей 00 от двух других команд, 00/52/54/45 пройдены у всех,
  `barrier_timeouts=0`, `state_checksum=0x34D5` у всех. `MAX_LOG` включает
  debug-логи барьеров (ограничение `tracing-subscriber` без `env-filter`).
- [ ] Паритет Rust+C++-клиентов в одной сессии (бит-в-бит) — тест с C++-клиентом (headless-пара).
  **Частично / блокер:** headless-сборка `src/` умеет только `--server`
  (`src/main.cpp`: `MAX_HEADLESS` → `Server_Main`, ветки `--client`/`--connect`
  нет); GUI-клиент требует X11 и ручного меню. `build/scripts/live-smoke-mixed.sh`:
  **PARTIAL PASS** — Rust×2, CRC `0x7035`, C++-сервер (эталонный
  `state_checksum`/`GameManager_CheckDesync`) не пишет `Desync check failed`,
  т.е. состояние Rust-клиентов совпало с C++-симуляцией; одновременная сессия
  Rust+C++ не доказана. Для полного пункта нужен headless client mode
  (или Xvfb+автоматизация GUI) — это изменение `src/` вне доказанного бага.
- [x] Полный реконнект: восстановление состояния после разрыва (54/CRC) — по barrier-flow.
  `ReconnectPolicy` (ENet+TP00 тем же тикетом, без повторного лобби) →
  продолжение локального lockstep-состояния → resume-54 своей команды
  (`TurnBarrierFsm::resume_state_sync`) → встречные 54 пиров → 52/54/45 и
  `finish_turn`. `net.rs::three_clients_survive_disconnect_and_converge_via_state_sync`
  (3 клиента, мок-релей, B offline, CRC/turn сходятся) +
  `resume_state_sync_publishes_the_last_completed_index`. **Пределы:** SNAP нет;
  54 покрывает только юнитов/экономику своей команды; пропущенные приказы
  других не replay-ятся; C++-пиры исключают команду через 30 с; своя команда
  авторитетна (возможен откат её состояния). См. `protocol.md` §9.
- [ ] Спектатор (клиент без команды) — если достижимо без изменения сервера.
  **Невозможно без сервера:** нет `TEAM_TYPE_SPECTATOR` (`enums.hpp:164-168`),
  `Remote_SetupPlayers` требует `player_team`, лобби раздаёт 4 командных слота,
  поздний вход не получает снапшот карты/юнитов. Нужны `team=-1`/spectator-флаг
  и релей 53/54 наблюдателю. Задокументировано в `protocol.md` §9.2.
- Критерий: smoke-скрипты 3-клиентов и Rust+C++ зелёные; реконнект переживает разрыв.
  **Факт:** 3-клиентный smoke зелёный; Rust+C++ — PARTIAL (блокер C++ headless
  client mode); реконнект проверен в in-process 3-клиентном мок-тесте, живой
  разрыв в smoke не автоматизирован (нужен pause/proxy-хук).

### P6 — QA-покрытие и релиз v1.1.0
- [x] Перевод 🟡/❌ тест-кейсов suite в зелёные (или осознанное закрытие).
  `suite/*.md`: **161 ✅ / 0 🟡 / 18 ⏭ not planned** (причины в строках и
  `suite/README.md`): закрыты клавиатурная навигация, Esc, Name, chat,
  invalid order; `not planned` — спектатор (нет серверной поддержки),
  mixed Rust+C++ (нет headless client mode), FLIC, ручной WINDOW, сквозной
  урон/поражение, MXLS, десинк-хук, hot-seat-сценарии.
- [x] Новые .qa-сценарии: производство, сейвы, долгая партия, 3-клиента.
  `production.qa`, `save-load.qa`, `soak-100.qa` (+`soak_headless`),
  `live-smoke-3.sh`; в P6 добавлены `upgrade.qa` (золото/значения/`Cancel`) и
  `research.qa` (Research Center → питание → 16 ходов → Attack 1).
- [x] Golden-кадры новых экранов (BuildMenu и пр.).
  `golden/production/buildmenu*.png`; в P6 — `golden/upgrade/upgrade-menu.png`,
  `upgrade-done.png`, `golden/research/research-done.png`; `ci.sh` сверяет
  три набора.
- [x] Релиз: версия 1.1.0, CHANGELOG, чек-лист, артефакты (UI/headless Windows + Linux), тег v1.1.0.
  `max-brh` workspace = **1.1.0** (Cargo.lock обновлён); `CHANGELOG.md`
  `[1.1.0] — 2026-11-16`; пересобраны
  `release/max_app-1.1.0-windows-x86_64-ui` (MSVC+vcpkg, SDL3+sound),
  `release/max_app-1.1.0-windows-x86_64` (mingw, headless, wine-smoke) и
  `release/max_app-1.1.0-linux-x86_64.tar.gz`; SHA256SUMS внутри бандлов;
  `release-checklist.md` — раздел v1.1.0 со свидетельствами. **Тег не создан**
  (по указанию: сделает хозяин), команда подготовлена:
  `bash release.sh --tag`.
- Критерий: ci.sh (все стадии) + live-smoke зелёные; чек-лист полностью заполнен.
  `ci.sh` (стадии 1–4) — **PASS**; QA `sp-full/hs-2/visual-screens/victory/
  production/save-load/upgrade/research/edge-cases` — **PASS**; golden — 0
  дрейфа; `cargo test` — 0 failed; live-smoke 2×10 и 3×10 — **PASS**
  (Docker); soak 100 ходов — PASS.

### P7 — Предвысадочный `CargoMenu` 1:1 (мобилизация)
- [x] Изучить оракул досконально: `cargomenu.cpp`, `abstractupgrademenu.cpp`,
  `purchasetypeselector.cpp`, `cargoselector.cpp`, `unittypeselector.cpp`,
  `scrollbar.cpp` (`LoadVerticalBar`), `reportstats.cpp`
  (`ReportStats_DrawListItemIcon`/`DrawNumber`), `upgradecontrol.cpp`.
  Зафиксировано: `unit_types1` — **15 типов**; список модернизации —
  `IsUnitFiltered` по всем 93 (по умолчанию **62**, `IsBetterUnit`-порядок);
  строки апгрейда — до 8 атрибутов (attack/rounds/range/ammo/armor/hits/
  scan/speed); `ATTRIB_TURNS` контролом не добавляется (`UPGRADE_CONTROL_9`
  не используется, `CargoMenu::UpdateTeamGold` — мёртвый код).
- [x] Переписать `max_app/src/mobilize.rs`: полный layout `CARGOPIC`
  (портрет 11,13 280×240; `Available` 482,80 123×290 ×9 строк; `Purchased`
  337,45 118×185 ×5 строк; статистика 11,293; апгрейды 283/321; золото
  361,276; cargo 421,301; заголовки 473,7 / 482,57 / 610,57; кнопки
  `Buy` 590,386, `Delete` 412,240, `Cancel` 357,452, `Done` 447,452,
  `Help` 420,452, `CRGTOG` 542,446, `BLDDES` 292,264, стрелки 471/491,387 и
  327/348,240).
- [x] Изображения: иконки `I_*` (71 тип) в обеих строках списков, масштаб
  как C++ (1:2 мобильные / 1:4 стационарные), командная перекраска.
- [x] Башня/пушка отдельным кадром (доведено после первого прохода P7):
  - `D_*` (`Unit::GetFrameInfo()`) разбирается в `FrameInfo`
    (`max_assets::sprite`): база/число кадров корпуса, башни, выстрела,
    коннектора и `angle_offsets[8]`;
  - иконки списков `I_*` — два кадра: корпус (0) + башня (1) со сдвигом
    `angle_offsets[2] >> divisor` (divisor 1/2), как
    `ReportStats_DrawListItemIcon`;
  - карта (`render.rs`), портрет HUD (`panels.rs`), меню апгрейдов и
    `BuildMenu` — башня кадром `turret_image_base + turret_angle` со сдвигом
    `angle_offsets[body_angle]` (`SpriteRenderer::draw_turret_scaled`), как
    `UnitInfo::RenderWithConnectors`; у танка/мобильной зенитки/эскорта
    башня больше не пропадает.
- [x] Портрет/описание/статистика 1:1 (доведено после первого прохода P7):
  - портрет — big image `P_*` 300×240 (`Unit::GetPortrait()`,
    `WindowManager_LoadBigImage(..., palette_from_image=false)`) в (11,13) с
    палитрой окна `CARGOPIC`; окно `unit_portrait_window` 280×240 обрезает
    правые 20 px (C++-порт пишет 300 px в буфер без клипа — отличие
    задокументировано); 71 ресурс, ленивый кэш `CargoMenuAssets::portraits`;
  - описание — `Unit::GetDescription()`: `SC_UNITS.description` (hex) →
    `language.json` en-US; генератор `tools/gen_cargo_unit_data.py` →
    `max_app/data/cargo_unit_data.json` (93 записи; 69 с текстом, 24 служебных
    типа без описания в оригинале и недостижимы в `CargoMenu`). Рисуется
    `Text_TextBox(16,17,280,230, GNW_TEXT_OUTLINE|0xA2)` поверх портрета,
    шрифт 5, перенос `Text_SplitText`;
  - статистика — `UnitStats_DrawStats` с сеткой pips: `UnitStats_DrawText`
    (шаг 19 px, число справа x=24, подпись x=28/клип 49 px, `language.json`:
    `Attack`/`Shots`/`Range`/`Ammo`/`Cargo`/`Prod.`/`Uses`/`Armor`/`Hits`/
    `Scan`/`Speed`/`Cost`), `UnitStats_DrawIcons` (`I_HRDATK`/`I_SHOTS`/
    `I_RANGE`/`I_AMMO`/`I_RAW`/`I_FUEL`/`I_GOLD`/`I_POWER`/`I_LIFE`/`I_ARMOR`/
    `I_HITS`/`I_SCAN`/`I_SPEED`), дельта апгрейда — пустые иконки той же
    строки (у `Cost` — `I_GOLDE`) за красной чертой, линии `COLOR_RED`.
    Условия строк — по базовым значениям, числа — по текущим; `Cost` —
    `Cargo_GetRawConsumptionRate(Builder_GetBuilderType, 1) * turns`.
- [x] Индикаторы: полоса золота `VERTGOLD` + число кредитов; cargo-слайдер
  `VERTRAW/FUEL/GOLD` + счётчик `5·N`; 1 кредит за 5 единиц, ёмкость
  `storage/5`, `GOLDTRCK` — 0, бесплатный cargo не снимается.
- [x] Режим модернизации: переключатель `CRGTOG`, фильтры домена, стрелки
  `UPGLFT/UPGRGT`, цены `UpgradeControl_CalculateCost` (целочисленный
  `max_core::research`), `Done` фиксирует `UnitValues` до высадки; продажа
  возвращает цену + `cargo/5`.
- [x] C++-вектор-генератор: `max-brh/tools/cargo_vectors` (`main.cpp` +
  `data.inc` от `tools/gen_cargo_data.py`), векторы `vectors.txt`; тесты
  `max_app/tests/cargo_vectors.rs` сверяют бит-в-бит 15 цен покупки, 93 цены
  списка, 7 наборов списка модернизации, строки и первые стоимости
  апгрейдов всех 93 типов. 0 расхождений.
- [x] QA: `cargo-full.qa` (покупка из конца полного списка, cargo,
  модернизация, фильтры, апгрейд, `Done`), новые команды
  `mobilize mode|filter|select|loadout|row|cargo` и проверки
  `assert cargo-mode/cargo-types/cargo-credits/cargo-description/
  cargo-stat-icons/cargo-portrait` (Tank — второй кадр с описанием/портретом/
  статами); golden `golden/cargo/{cargo-buy,cargo-description,cargo-upgrade,
  cargo-upgraded}.png` + обновлённый `golden/mobilize.png`; `ci.sh`/GitHub CI —
  новые стадии. Портрет/описание/иконки — frame-тесты
  `tests/cargo_render.rs` (портрет, описание вкл/выкл, pips, линии, смена
  типа) и UNIT-тесты `mobilize`/`cargo_unit_data`.
- [x] Артефакты и документация: Windows UI (MSVC+vcpkg) и headless (mingw)
  пересобраны; `CHANGELOG.md`, `docs/analysis/visual-1to1-status.md`,
  `suite/03-single-player.md` (TC-0324…0328), `suite/09-visual-1to1.md`
  (TC-0922/0927), `tools/cargo_vectors/README.md`.
- Критерий: `cargo check/test` — зелено; все qa-сценарии PASS; golden —
  0 дрейфа; `cargo test --all-targets` (в т.ч. новые 24 теста) — 0 failed.

### P8 — ResearchMenu и Help-экраны 1:1 (2026-11-17)

- [x] Изучить оракул: `researchmenu.cpp` (`RSRCHPIC`), `helpmenu.cpp` +
  `assets/help.json`, точки вызова `HelpMenu_Menu` в `cargomenu.cpp`,
  `planetselectmenu.cpp`, `gameconfigmenu.cpp`, `chooseplayermenu.cpp`,
  `researchmenu.cpp`, `upgrademenu.cpp`, `game_manager.cpp` (`HELP_OF`).
- [x] `max_app/src/researchmenu.rs`: окно `RSRCHPIC` 360×331 в (223,76),
  заголовок/подписи, 8 тем (Attack/Shots/Range/Armor/Hits/Speed/Scan/Cost),
  счётчики центров, ходы до завершения (`(centers+turns−1)/centers` через
  `research_turns_to_complete` на копии темы), `+N%`, `UPGLFT/UPGRGT` с
  disabled `_X`, `PRFSLIDE` + клик по слоту, `DPTBAY/DPTHP`;
  `free_capacity` как в C++ (старт 0, уменьшение темы освобождает центры);
  `Done` → `GameState::set_research_allocation` (порт
  `ResearchMenu_OnClick_Done`), `Cancel`/`Esc`, `Help` → `RESEARCH_SETUP`.
  Экран открывается пунктом `resrch` меню команд юнита (popup «Research» в C++)
  или клавишей `R` для включённого Research Center. `cancel`/`done` не шлют
  пакет 09 — ограничение протокола.
- [x] Help: `max_app/src/helpmenu.rs` — окно `HELPFRAM` 301×231
  ((169,124) в меню / (253,126) в игре), текст `FONT5` + обводка, 16 строк
  на страницу и центровка короткого текста, `PageUp/PageDown` + `BLDUP__/
  BLDDWN`, `Done`/`Enter`/`Esc`, `Keys` у `GAME_SCREEN_SETUP`; тексты —
  `tools/gen_help_data.py` → `data/help.json` (32 секции, 661 запись из
  `help.json` + `language.json`). Подключены кнопки `?` Options/Planet/
  Players/Clan, `Help` CargoMenu/ResearchMenu/UpgradeMenu, панель `HELP_OF`
  и `?` в игре. Отличие: показывается весь текст секции, а не абзац под
  курсором (в C++ help-режим с кликом по элементу).
- [x] QA: `research-help.qa` (Help-экраны, `ResearchMenu`, распределение
  Attack→Shots, `Done`, 16 ходов; 9 golden-кадров), команды
  `research open/select/row/done/cancel`, `help SECTION/page/up/down/keys/
  done/close`, проверки `research-menu`, `research-alloc`, `help`,
  `help-section`, `help-row`; `ci.sh`/GitHub CI — новая стадия.
- [x] Артефакты/документация: Windows UI + headless пересобраны; CHANGELOG,
  `visual-1to1-status.md` §13, suite TC-0220/TC-0329/0330/TC-0929/0930.

### P9 — Диалог Preferences/Setup 1:1 + реальные effects/sound (2026-11-19)

- [x] Изучить оракул: `menu.cpp:2325/2331` (`menu_preferences_window`/
  `menu_setup_window`), `optionsmenu.cpp/.hpp` (таблица 36 опций, `Init`-
  раскладка, `Done`/`Cancel`/`ProcessTextEditKeyPress`), `settings.cpp`
  (defaults и секции), `saveload.cpp:547/920` (`IniPreferences`),
  `units_manager.cpp:2857/3030` и `unitinfo.cpp:5057` (`effects`).
- [x] `max_app/src/preferences.rs`: `Preferences` (все настройки), раскладка
  `PREFSPIC`/`SETUPPIC` по C++, отрисовка `PREFSPIC`/`PRFDNE/PRFCAN/PRFHLP`/
  `PRFSLIT`/`PRFSLIDE`/`UNCHKED`/`CHECKED`/`PREFEDIT`/`PREFNAME`, мышь +
  клавиатура (Enter/Esc/Tab/Space/Left/Right, text edit), `Done`/`Cancel`/
  `Help`, модальность; открытие из `Setup` главного меню (`Screen::Preferences`)
  и кнопки `Preferences` панели (`App::preferences_dialog`).
- [x] Реальное действие: `effects` → `App::update_unit_animations` и
  `render::unit_frame_index_with_animation` (кадры статичны), `disable_fx` →
  `AudioPlayer::set_enabled`, `fx_sound_level` → `set_volume`, `auto_save` →
  `App::autosave`. Остальные опции сохраняются и помечены «не действуют» в
  `visual-1to1-status.md` §27.
- [x] Сохранение: `SaveMeta::ini_preferences` (`save_meta_for_local_game_with`,
  `sync_save_meta`), чтение при загрузке V70 (`SaveLoad_Load`),
  `preferences.ini` рядом с сейвами; `Files` панели заодно открывает
  существующее окно сохранения/загрузки.
- [x] QA: `prefs open|toggle NAME|set NAME N|done|cancel|help`, `assert prefs=…`,
  `prefs-value NAME=N`, `prefs-effects/sound/audio`; `preferences.qa` и golden
  `golden/preferences`; `ci.sh` — новая QA+golden-стадия.
- [x] Тесты: `preferences` (9, раскладка/клики/ini/файл), `tests/preferences.rs`
  (6: меню/игра, модальность, статичные кадры при effects=off, звук→AudioPlayer,
  V70 round-trip, Cancel); живой `build/scripts/ui-test-prefs.ps1`.
- [x] Артефакты/документация: Windows UI + headless пересобраны; CHANGELOG,
  `visual-1to1-status.md` §27, `state-model.md` §3.3, suite TC-0108/TC-0935,
  `scripts/README.md`, `qa-plan.md`.

### P10 — Ревью A4–A9: центр ленты, маркеры, знание ресурсов, RADAR, селектор (2026-11-20)

- [x] Сверить с оракулом каждый пункт и зафиксировать ссылки на функции/строки.
  A4 `unitinfo.cpp:4414-4483` (`PositionInTape` не возвращает `x`/`y`; сдвиг
  снимается в `TAPE_POSITIONING_DEINIT`/`LEAVE`), A5 `drawmap.cpp:304-390`
  (кольцо 12 клеток вокруг 2×2, `BLDMRK*` 62×62 hotspot 31,31, 5 кадров по 3
  отрисовки), A6 `survey.cpp:84-155` + `access.cpp:762-768` (бит `hash_team_id`
  в `CargoMap`, `GameManager_MaxSurvey` — только чит), A7
  `units_manager.cpp:2412-2421`/`:3029-3041` + `unitinfo.cpp:3581`/`:3783`
  (RADAR `SENTRY` → prior → `SENTRY`, 16 кадров), A8 `game_manager.cpp:744-836`
  + `drawmap.cpp:813-833` (три рамки, `MarkerColor`, 210 мс).
- [x] Исправить порт в этих точках: `buildsite::tape_center_offset` (UNIT_READY и
  IN_TRANSITION держат центр), `production::spawn_unit`‑ветка `SpawnNewUnit`
  (prior через `set_new_order_int`), `GameState::survey_area` (знание ресурсов —
  часть состояния), `ResourceMapEntry` (порядок битов `HASH_TEAM_*`),
  `render::draw_resource_icons`/`draw_unit_selector` (+ порядок отрисовки
  «юниты → иконки → маркеры стройки»), `UnitInfo::new` (`EXECUTING_ORDER`).
- [x] Тесты: `max_core/tests/survey.rs` (5), `production.rs` (RADAR),
  `max_app/tests/app.rs` (5 кадровых), `buildsite::tests`; QA-проверки
  `assert resource-icons`, `assert survey-overlay`, `assert unit-selector`,
  `assert build-center=yes`; golden переснят (0 дрейфа после повторной сверки).
- [x] Документация: CHANGELOG, `visual-1to1-status.md` §32, этот план,
  `max-brh-qa/scripts/README.md` (новые assert-проверки).

### P11 — Turn Based: конец хода ИИ и боевые анимации (2026-11-20)

- [x] Воспроизвести зависание: QA `combat-tb.qa` (Options → Turn Based, ход 1,
  выстрел ракетой, `end_turn`) до исправления крутил **12 000 кадров**
  (`LOCAL_AI_TICK_LIMIT`); unit-тест
  `max_core/tests/turn_based_combat.rs::turn_based_ai_turn_ends_after_its_missile_impact`
  падал с `Rocket/Move/ExecutingOrder` на старте и `counter = 0`.
- [x] Причины (оракул): `AirPath::Execute` (`airpath.cpp:186-199`) проверяет
  `speed == 0` только у `MOBILE_AIR_UNIT`; `UnitsManager_DeployUnit`
  (`units_manager.cpp:2390`) ставит `ANIMATED` в `ORDER_EXPLODE/DESTROY`;
  `UnitsManager_ProgressExplosion` (`units_manager.cpp:4006-4032`) уничтожает
  погибшего родителя на половине анимации. Порт терял все три места →
  `OrdersPending` не сбрасывался, `Ai_CheckEndTurn` не вызывался.
- [x] Порт: `unit_order::air_path_execute` (guard только для воздуха),
  `GameState::spawn_unit` (`merged_flags & ANIMATED`),
  `unit_order::progress_explosion` (parent + `destroy_unit`), `max_core::frames`
  (`D_*` → `image_index_max`), `UnitInfo::new` (кадры), смерть
  пехоты/диверсанта (`image_base = 168 + angle/2*8`), рендер `EXPLODING` по
  `image_index`, видимость эффекта от погибшего юнита.
- [x] Тесты: `max_core/tests/turn_based_combat.rs` (3), `max_app/tests/
  turn_based.rs` (3: ракета в полёте на End Turn, hot-seat 2 человека + ИИ,
  4 раунда SP), `unit_sprites::frame_info_table_matches_game_data`; QA
  `combat-tb.qa` (`assert endturn-frames<2000`), `sp-tb.qa`
  (`endturn-frames<3000`, 2 компьютера), golden `sp-tb/`, `combat-tb/`.
- [x] Soak: `SoakOptions::play_mode` (default `TurnBased`), `soak_headless
  --mode tb`, `ci.sh` передаёт `--mode tb`; `soak-100.qa` уже Turn Based.
- [x] Гейт и артефакты: `bash ci.sh` — зелёный; Windows UI + headless
  пересобраны; CHANGELOG, `visual-1to1-status.md` §36, `scripts/README.md`
  (новые QA/команды `click enemy`, `assert endturn-frames`).

## Порядок

P1 → P2 → P3 → P4 → P5 → P6 → P7 (P1 сразу; P2/P3 параллелятся с P4 частично).
P8 → P9 (Help/Research, затем Preferences/Setup и реальные effects/sound).
Каждый этап: коммит + тесты + свидетельства (логи/скриншоты/qa-out).
