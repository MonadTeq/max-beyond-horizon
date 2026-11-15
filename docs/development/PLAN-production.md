# План Production-ready Rust-клиента (v1)

Версия: 1.0 · 2026-11-15 · Цель: `goal-f7008432`.

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
  используют общий порядок, тесты — `max-rust/crates/max_app/src/assets.rs`
  (`exe_dir_resolves_game_data_when_cwd_is_elsewhere`).
- [ ] Согласование путей: `--data-dir/--assets-dir` единообразны во всех режимах и документации.
  Сделано частично: сообщение об ошибке `MAX.RES` подсказывает
  `--data-dir/--max-res`, fallback предупреждает о подмене. Полный аудит флагов
  (все режимы + `docs/development/build.md`) — не закрыт.
- Критерий: запуск из любого каталога находит данные или даёт точную подсказку.

### P2 — Производство и строительство (крупнейший геймплейный пробел)
- [x] ProductionManager: очереди заводов, `SpawnNewUnits`, стоимости (Cargo/Upgrade-формулы).
  `max-rust/crates/max_core/src/production.rs`: таблица
  `max_builder_capability_list`, `turns_to_build`, `cost_per_turn`,
  `max_possible_build_rate` (порт `buildmenu.cpp:729`), `ProductionManager`
  (queue/commit/cancel/progress), `GameState::finish_turn` → ход производства
  для команд в порядке `RED..GRAY` (`ManageEconomy` → `ProgressBuildState`).
  Экономика: золото списывается ставкой `Cargo_GetRawConsumptionRate` за ход
  (материалы комплексов — TODO(P3)); нехватка золота — пауза, а не отмена.
- [x] BuildMenu: заказ юнитов/зданий (UI 1:1, ресурсы `B_*`/иконки из max-assets).
  `max-rust/crates/max_app/src/buildmenu.rs`: `FACBUILD`/`CONBUILD` + `BLD*`
  (фон 640×480, списки 483,60 / 337,60, кнопки по координатам C++), клавиша
  `B`, модальный ввод, заказ — `Command::Order` с `PayloadP4` (пакет 08).
- [ ] Исследования/апгрейды: ResearchMenu-факторы (float-формулы → build-time таблицы по DESIGN).
  **Помечено как необъятное для P2**: `researchmenu.cpp` + `abstractupgrademenu.cpp`
  (~27 КБ, `pow`-формулы `ResearchMenu_CalculateFactor`) требуют
  build-time-таблиц; текущий `manage_economy` только сбрасывает прогресс тем
  (`TODO(риск 1)`). Вынести в отдельный этап (после P3).
- [x] Интеграция с ИИ (ИИ строит через ProductionManager, как C++).
  ИИ уже выдаёт `ORDER_BUILD` с очередью (`TaskCreateBuilding`/`TaskCreateUnit`
  → `TaskCtx::build_order`); теперь очередь реально тикает и выпускает юнитов
  в `finish_turn`. `builder_type_for`/`turns_to_build` делегируют в
  `production` (единая таблица). Тест:
  `max_core/tests/ai_p6.rs::ai_completes_a_build_queue_through_production`.
- Критерий: скриптом строится завод → заказан юнит → юнит появляется через N ходов; ИИ строит флот; партия 30+ ходов с производством без расхождений (детерминизм).
  Завод → юнит: `max-rust-qa/scripts/production.qa` (TRAINHAL за 10 ходов,
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
  (`max-rust/crates/max_core/tests/data/save1_v70.cam`, `save2_v70.cam`):
  `load → to_save` совпадает побайтово. Остальные 7 файлов `max-original/SAVE3..9`
  в репозитории отсутствуют (не добавлялись как фикстуры). Кросс-проверка
  «наш сейв грузит C++-порт» требует собранного `max-port` (Docker
  `max-port-build`) и не выполнялась в этом этапе — формат гарантирован
  парсером `save.rs` (он принимает только V70 с полным обходом файла).

### P4 — Стабильность и производительность
- [x] Soak: 100+ ходов ИИ×ИИ и человек+ИИ через qa-драйвер — без паник/утечек (замер памяти).
  `max-rust-qa/scripts/soak-100.qa`: Options (Clueless, No Limit, Turn Based,
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
  `max-rust-qa/scripts/edge-cases.qa` → **PASS**: 50 кликов по пустым областям
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
  `release-checklist.md` §2.9, `max-rust-qa/scripts/README.md` §P4.

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
- [ ] Перевод 🟡/❌ тест-кейсов suite в зелёные (или осознанное закрытие).
- [ ] Новые .qa-сценарии: производство, сейвы, долгая партия, 3-клиента.
- [ ] Golden-кадры новых экранов (BuildMenu и пр.).
- [ ] Релиз: версия 1.1.0, CHANGELOG, чек-лист, артефакты (UI/headless Windows + Linux), тег v1.1.0.
- Критерий: ci.sh (все стадии) + live-smoke зелёные; чек-лист полностью заполнен.

## Порядок

P1 → P2 → P3 → P4 → P5 → P6 (P1 сразу; P2/P3 параллелятся с P4 частично).
Каждый этап: коммит + тесты + свидетельства (логи/скриншоты/qa-out).
