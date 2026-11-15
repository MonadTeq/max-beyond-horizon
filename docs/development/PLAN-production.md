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
- [ ] Soak: 100+ ходов ИИ×ИИ и человек+ИИ через qa-драйвер — без паник/утечек (замер памяти).
- [ ] 60 Гц интерактив против lockstep: проверка дрейфа и стоимость тика (профилирование; при необходимости — бюджет).
- [ ] Краевые: спам кликов, alt-tab, битые файлы, юникод — по TC-11xx.
- Критерий: soak-сценарий в CI (ночной/релизный), память стабильна, время тика в норме.

### P5 — Сетевая зрелость
- [ ] 3+ клиента в сессии (релей всем, барьеры по всем командам) — live-smoke с 3 клиентами.
- [ ] Паритет Rust+C++-клиентов в одной сессии (бит-в-бит) — тест с C++-клиентом (headless-пара).
- [ ] Полный реконнект: восстановление состояния после разрыва (54/CRC) — по barrier-flow.
- [ ] Спектатор (клиент без команды) — если достижимо без изменения сервера.
- Критерий: smoke-скрипты 3-клиентов и Rust+C++ зелёные; реконнект переживает разрыв.

### P6 — QA-покрытие и релиз v1.1.0
- [ ] Перевод 🟡/❌ тест-кейсов suite в зелёные (или осознанное закрытие).
- [ ] Новые .qa-сценарии: производство, сейвы, долгая партия, 3-клиента.
- [ ] Golden-кадры новых экранов (BuildMenu и пр.).
- [ ] Релиз: версия 1.1.0, CHANGELOG, чек-лист, артефакты (UI/headless Windows + Linux), тег v1.1.0.
- Критерий: ci.sh (все стадии) + live-smoke зелёные; чек-лист полностью заполнен.

## Порядок

P1 → P2 → P3 → P4 → P5 → P6 (P1 сразу; P2/P3 параллелятся с P4 частично).
Каждый этап: коммит + тесты + свидетельства (логи/скриншоты/qa-out).
