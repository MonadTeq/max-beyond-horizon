## [Unreleased]

### Исправлено

- **Энергия комплекса: завод не стартует без power/life, здания отключаются при
  дефиците, сверху — сообщение о нехватке.** Оракул: `cargo.cpp:70-138`
  (`Cargo_GetNetProduction`/`Cargo_ApplyUnitConsumption` — завод тратит
  raw+power+life из котла), `unitinfo.cpp:2751-2794` (`StartBuilding` сразу
  списывает `complex->material/power/workers` и зовёт
  `GameManager_OptimizeProduction`), `game_manager.cpp:2555-2584`,
  `production_manager.cpp:84-197` (активные `POWERSTN`/`POWGEN`), `:481-562`
  (`OptimizePowerProduction`/`OptimizePowerConsumption`), `:642-664`
  (`CheckPowerNeed`: топливо), `:677-790` (`ValidateIndustry` и порядок
  отключения `RESEARCH` → `GREENHSE` → `COMMTWR` → стройки → `MININGST`),
  `:366-380` (`UpdateUnitLifeConsumption` — `HABITAT`), `:308-330/792-804`
  (`ComposeIndustryMessage`/`DrawResourceMessage`), `teamunits.cpp:170-175`
  (`OptimizeComplexes`), `game_manager.cpp:8183-8204` (`ProgressBuildState`).
  Порт раньше не проверял `power`/`workers` вообще: завод запускался и строил
  без энергии, потребители не отключались, сообщения не было.
  Новый `max_core::power` (`optimize_complex`, `complex_totals`, `Shortage`,
  `IndustryOutcome`): баланс энергии/рабочих по комплексу, автоматическое
  включение/выключение производителей по спросу и топливу, отключение
  потребителей в C++-порядке (генераторы восстанавливаются сами; выключенные
  потребители C++ сам не включает — только игрок/ИИ), пауза стройки вместо
  потери очереди. `ProductionManager::commit` проверяет первый ход завода
  (`BuildError::NotEnoughPower/Fuel/Workers/Materials`) и оставляет очередь;
  `progress`/`manage_economy` прогоняют оптимизацию, `GameState` копит
  `industry_message`, `App` выводит его в `hud.hint` (верх карты). Тесты:
  `max_core/tests/power_economy.rs` (10: старт без power/workers/raw, порядок
  отключения, пауза/возобновление, сообщение, снапшот/CRC, детерминизм),
  `max_core/src/power.rs` (2), обновлены `production`/`construction_economy`/
  `transfer` под энергоснабжение комплекса. QA: `production.qa`,
  `research.qa`, `research-help.qa` ставят `POWGEN`/`FDUMP`/`HABITAT`;
  новая QA-команда `complex fuel|gold N` (как `complex raw`), golden
  `production`/`research`/`research-help` перегенерированы (новые здания).
  Документация: `docs/architecture/game-mechanics.md` §3/§5,
  `docs/analysis/visual-1to1-status.md` §42.
- **Построенный в AIRPLT самолёт стоит НА заводе и выводится приказом игрока.**
  Оракул: `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3489-3520` — юнит на клетке
  завода, `ORDER_IDLE`/`ORDER_STATE_STORE`, `scaler_adjust = 4`, `parent` —
  завод, у завода `parent` — юнит и `storage = 1`),
  `UnitsManager_Popup_InitFactories`/`Popup_PlaceNewUnit`
  (`units_manager.cpp:833-886` — повторный клик по заводу выбирает готового
  юнита), `GameManager_GetAirUnitCursor` + `GameManager_ManagePlayerAction`
  (`game_manager.cpp:5937-5962`, `:6918-6926` — `CURSOR_ACTIVATE` →
  `ORDER_ACTIVATE` заводу), `UnitsManager_ActivateUnit`
  (`units_manager.cpp:3826-3913` — `--storage`, снятие родителя, старт
  следующего заказа очереди, самолёту `ORDER_MOVE`/`INIT` + `ScaleUnit(EXPAND)`
  → взлёт; наземному — клетка клика), `TaskActivate`
  (`taskactivate.cpp:65-201`) для ИИ. Порт раньше спавнил юнит в соседнюю
  клетку, завод сразу начинал следующий заказ, а `ORDER_ACTIVATE` возвращал
  прежний приказ. Новые `unit_order::activate_unit`/`issue_factory_exit`/
  `activation_exit_cell`, `hud::build_activate_order`/`build_air_move_order`
  (авиация ходит пакетом **08/P3**), `App::open_unit_menu` выбирает готового
  юнита, `TaskManager::find_task_for_unit` не даёт задач юниту «в трюме».
  Тесты: `max_core/tests/production.rs` (`air_plant_*`,
  `repeated_build_keeps_producing_after_the_unit_is_launched`),
  `max_app/tests/airplant_launch.rs` (6). QA: `production.qa` +
  `assert factory-ready`/`unit-stored`, команда `launch near|grid`, кадры
  `factory-unit-ready`/`factory-unit-launched`.
- **Ракеты летят по прямой в пикселях, а не по клеткам.** Оракул:
  `AirPath::Execute` (`airpath.cpp:175-295` — `m_step += m_delta`,
  `position = (m_step >> 16) + m_start`, `OffsetDrawZones`, `grid = pixel >> 6`,
  `Paths_DrawMissile`, `m_length`, `Paths_FinishMove`),
  `AirPath::AirPath` (`:46-62`), `Paths_GetMissilePath`
  (`units_manager.cpp:4759-4774`), `Paths_DrawMissile` (`:362-406`),
  `Paths_FinishMove` (`:408-420`). Порт вёл снаряд как юнит (клетка за тик по
  `DIRECTION_OFFSETS`), след ставил по центрам клеток. Новый
  `unit_order::missile_path_execute`, пиксельный `spawn_missile_trail`,
  `render::missile_pixel_anchor`. Тесты: `unit_order` (`missile_flies_a_pixel_straight_line`,
  `missile_trail_particles_lie_on_the_line`,
  `missile_impact_uses_the_engine_cell_and_destroys_the_round`). QA: новый
  `missile-line.qa` + golden, `assert missile-line`, команда `wait firing`.
- **Стрелка X-FER показывает направление перевода (от источника к приёмнику).**
  Оракул: `TransferMenu::UpdateIndicators` (`transfermenu.cpp:94-96` —
  `button_arrow->SetRestState(total >= 0)`), `Button(XFRLFARO, XFRRTARO, …)`
  (`:239` — `up` = влево, `down` = вправо), `win_set_button_rest_state`
  (`button.c:1022` — `rest_down` рисует `down`). Порт рисовал наоборот.
  `TransferMenu::arrow_points_right()` + тест
  `arrow_points_from_source_to_target`; golden `xfer/` перегенерирован.
- **Звуки выстрела, попадания и взрыва (орудие юнита, взрыв).** Оракул:
  `UnitInfo::PrepareFire` (`unitinfo.cpp:4725` — `SFX_TYPE_FIRE`),
  `UnitsManager_StartExplosion` (`units_manager.cpp:3922/3929` —
  `SFX_TYPE_HIT`/`SFX_TYPE_EXPLOAD`), `SoundManager::PlaySfx`
  (`sound_manager.cpp:413-490` — `volume / 100`), `ParseSoundEffects`
  (`units.cpp:228-290` — `units[TOKEN].sound_effects`, иначе
  `default_sound_effects`; у взрывов `LNDEXPLD`/`AIREXPLD`/`SEAEXPLD`/`BLDEXPLD`/
  `HITEXPLD` — `GEN_XPLD`, у `TANK` — `TANK14/15/16`). Симуляция копит
  `UnitSound` (`GameState::take_unit_sounds`, в CRC не входит), клиент берёт тег
  из `data/unit_sfx.json` (генератор `tools/gen_unit_sfx.py`) и играет через
  `AudioPlayer::play_gain` (`AudioBackend::play_scaled`, усиление `volume/100`);
  выключатель — `Preferences::disable_fx`. Тесты:
  `max_app/tests/unit_sounds.rs` (5, мок плеера), `audio.rs`, `sfx.rs`.
- **Комплексы и коннекторы портированы 1:1 — завод снова видит сырьё базы (AWAC
  достраивается).** Оракул: `UnitsManager_UpdateConnectors`
  (`units_manager.cpp:2051-2225`), `UnitInfo::AttachComplex`/`AttachToPrimaryComplex`/
  `TestConnections`/`GetFirstUntestedConnection`/`DetachComplex`
  (`unitinfo.cpp:1978-2313`), `UnitsManager_RemoveConnections`
  (`units_manager.cpp:1556-1709`), `Access_GetTeamBuilding` (`access.cpp:2157`),
  `Complex::Grow/Shrink`/`GetCargoInfo` (`complex.cpp:136-215`). Новый модуль
  `max_core/src/complex.rs`: восемь направлений `CONNECTOR_*` ставятся себе и
  соседу (у `CONNECTOR_UNIT`/`STANDALONE` — «левая/верхняя» пара, как в C++),
  `AttachToPrimaryComplex` сливает всё связное подграфство в комплекс с
  наименьшим id, `DetachComplex` при уничтожении/переносе здания разделяет граф
  на связные группы (временная привязка отделяемого здания — как в C++),
  `RemoveConnections` снимает парные биты у соседей. Ёмкость/запас комплекса —
  сумма складов подключённых `ADUMP`/`FDUMP`/`GOLDSM`/шахт (`Complex::GetCargoInfo`),
  x-fer и `Complex::Transfer` работают по общему котлу. Слияние идёт при
  спавне/раскрытии здания (`spawn_unit`, `ORDER_AWAIT_SCALING`/`EXPAND`) и при
  входе в новый приказ — как `UnitsManager_UpdateConnectors` в C++.
  `GameState::snapshot_bytes` (и CRC `state_digest`) теперь включают
  `UnitInfo::complex` и `TeamUnits::complexes` (id, material/fuel/gold/power/
  workers/buildings), поэтому детерминизм ловит и развал комплекса.
- **Экономика завода — материалы ЕГО комплекса, а не всей команды.** Оракул:
  `Cargo_GetNetProduction` (`cargo.cpp:70-130`, `Cargo_ApplyUnitConsumption`:
  `raw = Cargo_GetRawConsumptionRate(type, GetMaxAllowedBuildRate())`, fuel/power/
  life по ставкам, `gold = 0`), `UnitInfo::StartBuilding` (`unitinfo.cpp:2757`),
  `ProductionManager::ValidateIndustry` (`production_manager.cpp:673-707`).
  `production::complex_raw_materials`/`spend_complex_raw_materials` списывают
  ставку со складов членов комплекса в порядке `UnitsManager_StationaryUnits`;
  чужой склад (без коннекторов) завод не оплачивает — даже если у команды есть
  сырьё. Попутно `spawn_unit` повторяет `UnitsManager_DeployUnit`
  (`units_manager.cpp:2382-2384`): у нового юнита склад пуст (кроме фазы
  `GAME_STATE_12_DEPLOYING_UNITS`), поэтому только что построенный `ADUMP` не
  появляется полным. Прежняя привязка «ставка из общего склада команды»
  (`team_raw_materials`) удалена. Тесты: `max_core/tests/production.rs`
  (`awac_completes_from_the_merged_complex_pot`), `construction_economy.rs`
  (`factory_pays_the_connected_complex_materials...`,
  `factory_without_connected_storage_pauses...`), `transfer.rs`
  (`xfer_takes_materials_from_the_whole_connected_complex`), `complex.rs`.
- **Строителя нельзя сдвинуть без явной отмены — как в C++.** Оракул:
  `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5829-5935`: для
  `BUILD_IN_PROGRESS` — `CURSOR_UNIT_NO_GO`, приказ движения не создаётся),
  `UnitsManager_Popup_OnClick_BuildStop` → `ORDER_BUILD`/`BUILD_CANCEL` →
  `UnitInfo::CancelBuilding` (`unitinfo.cpp:4231`). Откат «автоотмены при уводе»
  из 2de7085: `cancel_build_for_new_order` удалён из пакетов 08/38 и
  `GameState::set_order`; RMB во время стройки в `ui.rs` не отдаёт приказ, а
  пишет «Cannot move while building … use stop». Отмена — только `stop` попапа
  (мобильный строитель — `AWAIT`, площадка снимается; завод — `HALT_BUILDING`,
  заказ жив и продолжается пунктом `start`). Готовая площадка
  (`ORDER_STATE_UNIT_READY`) не тронута: уход по приказу (A1). Тесты:
  `max_core/tests/builder_cancel.rs` (`move/packet 08/38 do not cancel`,
  `explicit_stop_cancels_the_build`), `max_app/tests/build_lifecycle_interactive.rs`
  (`right_click_while_building_is_no_go_and_keeps_the_site`), QA
  `build-cancel.qa` переписан на `move near refused` + `stop`.
- **QA/эталоны:** `production.qa` теперь строит ADUMP вплотную к цеху и шахте
  (клетка (30,21)) и кладёт 60 сырья в общий котёл через `complex raw`
  (`Complex::Transfer`; QA-команда теперь раскладывает разницу по складам),
  поэтому Infantry действительно выпускается из материалов комплекса.
  `build-cancel.qa` — NO_GO + явный `stop`; пересняты эталоны
  `golden/production/factory-work-*`, `production-done`, `golden/build-cancel/*` (4 кадра), `golden/xfer/menu|selected|set`.
  В `production.qa` и `xfer.qa` проверяется общий котёл комплекса: в `xfer.qa` добавлены `spawn at 29 22 ADump` вплотную к шахте и `complex raw 100` (склад получает 50 из общего запаса), для точной постановки появилась QA-команда `spawn at X Y NAME`.

- **Заводы производят: платят материалами базы, а не золотом команды.**
  Оракул — `Cargo_GetRawConsumptionRate` (`cargo.cpp:140`),
  `UnitInfo::StartBuilding` (`unitinfo.cpp:2757`: `complex->material -= rate`),
  `Cargo_GetNetProduction` (`cargo.cpp:88-95`: та же ставка в балансе каждый
  ход), `ProductionManager::ValidateIndustry` (`production_manager.cpp:673-707`),
  `GameManager_SpawnNewUnits` (`game_manager.cpp:8076-8094`), очередь завода —
  `FactoryBuildMenu::Build` (`buildmenu.cpp:1171-1203`). После правки
  «мобильный строитель платит сырьём, заводы — золотом» (011629f) завод
  вставал навсегда, как только игрок тратил кредиты на мобилизации (обычный
  случай): золото за производство C++ не берёт. Теперь
  `production::team_raw_materials`/`spend_team_raw_materials` списывают ставку
  из складов сырья стационарных зданий (материалы комплекса в C++; комплексы
  порта не сливаются по коннекторам — известное ограничение §20), нехватка —
  `PausedNoMaterials` без потери заказа, `build_time` не двигается; золото не
  тратится. Тесты: `max_core/tests/production.rs` (28),
  `construction_economy.rs` (6); QA `production.qa` ассертит, что после
  выпуска пехоты золото не изменилось, а материалы шахты на месте.
- **Анимация включённого завода — как в оригинале: корпус статичен, мигают
  палитровые циклы.** Убран промежуточный цикл кадров
  `WORKING_BUILDING_ANIM_FRAME_SECONDS = 1/30` (завод «дёргался» в 30 раз
  быстрее оригинала). C++ `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`)
  держит `image_base + 1`, `SpawnNewUnit`/`CancelBuilding` — `image_base`
  (`unitinfo.cpp:3507/4246`); у `TRAINHAL`/`LIGHTPLT`/`LANDPLT`/`AIRPLT` в
  `MAX.RES` ровно два кадра корпуса, а видимое мигание дают палитровые циклы
  `World::InitColorCycles` (`world.cpp:542`) — кадр `+1` использует слоты
  9–12 (9 fps), 13–16 (6), 17–20 (9), 21–24 (6), 25–30 (2). DOS-съёмка
  (`max-original-dasm/formats/unit-animation.md` §7.2/§7.3) подтверждает:
  корпус зданий по времени не листается. Тесты:
  `max_app/tests/building_animation.rs` (5, период 1 с =
  9/6/9/6/2/6/8/8/10/6/6 шагов, за 1/30 с кадр не меняется),
  `building_frames.rs` (9); QA `production.qa` (`factory-work-a/b`) + эталон.
- **COLORS рисует цвет команды (красный/зелёный/синий/серый), а не derelict.**
  Оракул — `DrawMap_RenderUnitDisplays` (`drawmap.cpp:581-598`): цвет рамки по
  битам `HASH_TEAM_*` в `unit->flags`; `UnitInfo::UnitInfo`
  (`unitinfo.cpp:179`) добавляет `team_units->hash_team_id`
  (`resource_manager.cpp:969-981`: RED 0x2000, GREEN 0x1000, BLUE 0x800,
  GRAY 0x400); сама рамка — `DrawMap_RenderColorsDisplay` (`drawmap.cpp:511`),
  только для `!(flags & STATIONARY)`. Порт эти биты не выставлял, поэтому все
  рамки были derelict-цвета 0xA9. Теперь `overlays::team_color_index` повторяет
  то же отображение слот→цвет; биты из сейвов honoured напрямую, стационарные
  здания пропускаются. `team_clan` в C++ к цвету рамки отношения не имеет (он
  задаёт клановые кадры шахты, `unitinfo.cpp:282`). Тесты:
  `max_app/tests/overlay_colors.rs` (2, покадрово: 4 разные рамки + derelict),
  `max_core` — соответствие `hash_team_id` константам C++; QA `panels.qa` +
  обновлённые эталоны `golden/panels/panel-colors.png`, `panels-combined.png`.
- **Стройка: ресурс больше не уходит в минус, конструктор достраивает.**
  Оракул — `Cargo_GetRawConsumptionRate` (`cargo.cpp:140`),
  `BuildMenu_GetMaxPossibleBuildRate`/`MobileBuildMenu::Build`
  (`buildmenu.cpp:729/903`), `UnitInfo::UpdateProduction`
  (`unitinfo.cpp:2904-2915`), `GameManager_SpawnNewUnits`
  (`game_manager.cpp:8076-8094`). Мобильный строитель платит **сырьём со
  склада** (а не золотом команды), списание идёт только при
  `storage >= rate`, неоплаченный ход — `PausedNoResources` без движения
  `build_time` (новый транзиент `UnitInfo::build_paused`, вне сейва/CRC);
  завершение — ровно `build_time <= build_rate`; `commit` повторяет отказ
  `_(c529)` (`BuildError::NotEnoughMaterials`). Тесты:
  `max_core/tests/construction_economy.rs` (6).
- **Обломки после гибели наземного юнита.** Порт
  `UnitsManager_BuildClearing` (`units_manager.cpp:3646-3805`):
  `access::build_clearing` создаёт `SMLRUBLE` за юнитом и `LRGRUBLE` за
  зданием (груз — `GetNormalRateBuildCost()/2` + `storage` сырья, кадр из
  `Randomizer_Generate`), не оставляет обломков пехоте/диверсанту, минам, на
  воде и при удалении (`mode = false`). Вызов — из
  `UnitsManager_ProgressExplosion` (`units_manager.cpp:4021`) на середине
  анимации. Расчистка бульдозером — порт ветки `BULLDOZR`
  `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3441-3487`): обломки сносятся,
  груз переходит бульдозеру. Тесты: `max_core/tests/rubble.rs` (7).
- **Кнопки 3×3: центровка подписей и поведение каждой кнопки.**
  `Button::SetCaption` (`button.cpp:412-449`) центрирует текст в
  `width - x`/`height - y` — порт центровал в полном прямоугольнике (подписи
  уезжали на 1-2 px). Таблица смещений `GameManager_MenuItemLabelOffsets`
  (`game_manager.cpp:275-278`) и формула `Text_TextLine` (`text.cpp:772`)
  теперь 1:1 (`PanelButton::caption_offset`/`caption_position`). Новый модуль
  `max_app/src/overlays.rs` рисует все слои: GRID (сетка цветом поверхности,
  `drawmap.cpp:922/956`), COLORS (рамка команды, `:581`), STATUS (иконки
  `IL_DSBLD`/`IL_SPEED`/`IL_SHOTS`, `:444`), HITS/AMMO (полоски, `:605-669`),
  RANGE/SCAN (круги, `game_manager.cpp:933-960`), NAMES
  (`drawmap.cpp:747`); SURVEY остаётся иконками ресурсов. Тесты:
  `panels.rs` (+2), `overlays.rs` (+3), QA `panels.qa` + `golden/panels`.
- **Таблица курсоров 1:1.** `cursor::window_cursor` — полный порт
  `GameManager_GetWindowCursor` (`game_manager.cpp:4215-4564`) с
  `GetBuilderUnitCursor` (`:5829`), `GetAirUnitCursor` (`:5937`),
  `GetUnitActionCursor` (`:5976`) и `GetMilitaryCursor` (`:6150`): X-FER
  (`FRND_XFR`), repair (`FRND_FIX`), reload (`PTR_RLD`), load (`FRND_LOD`,
  включая `DEPOT`/`HANGAR`/`DOCK`/`BARRACKS`), `PTR_FTRG`, `WAY_PTR`,
  `ACTVTPTR`, `DISBLPTR`/`STEALPTR`. Режим берётся из `HudState::cursor_mode`
  (порт `UnitInfo::cursor`), добавлен `HudState::shift_pressed`
  (`GameManager_IsShiftKeyPressed`). `CURSOR_MAP` на карте больше не
  выбирается — в оракуле такой ветки нет. Тесты:
  `max_app/tests/cursor_table.rs` (12).

### Добавлено

- **B4/B5: настройки Preferences реально действуют (1:1 с C++).** Раньше они
  только сохранялись:
  * `click_scroll`/`quick_scroll` — новый модуль `max_app/src/scroll.rs`: окна
    `WINDOW_SCROLL_*` (`window_manager.cpp:90-101`) и
    `GameManager_SetGridOffset`/`ScrollMainMapView` (`game_manager.cpp:6351`,
    `:1652`); шаг камеры = `quick_scroll` экранных пикселей за кадр 30 Гц,
    кнопки панели приоритетнее скролла; в игре и сетевом кадре;
  * `fast_movement` — скорость спрайта ×2 (`UnitInfo::Move`,
    `unitinfo.cpp:1742`); `follow_unit` — камера за выделенным
    (`UnitInfo::FollowUnit`, `unitinfo.cpp:3597`);
  * `auto_select` — после завершения приказа/постройки выделяется следующий
    юнит (`GameManager_AutoSelectNext`/`SelectNextUnit`,
    `game_manager.cpp:1779/1544`, `Access_IsValidNextUnit`); в сетевой партии
    хук в `App::tick`;
  * `enemy_halt` — симуляция: `GameState::enemy_halt`, `access::target_class`
    и `UnitEventEmergencyStop` из `Access_UpdateMapStatus` (`access.cpp:827`);
  * `Turn Time`/`End Turn Time` из Preferences читаются в начале хода
    (`GameManager_UpdateGui`, `game_manager.cpp:2416-2476`), старт партии
    синхронизирует их с экраном настройки, сейв пишет текущие;
  * `Player Name` — имя команды из `menu.cpp:1944`, в игре `PREFSPIC` правит
    имя текущей команды (`optionsmenu.cpp:177/552`) и сразу видно в журнале
    HUD, на экране победы и в V70 (имена команд — `MXLS` v4).
  Тесты: `max_app/tests/preferences_behavior.rs` (8), `max_core/tests/
  enemy_halt.rs` (4), `scroll` (9). QA: `preferences.qa` — `prefs set/toggle`
  для всех настроек, `prefs-text`, `assert prefs-value NAME>=N`; новый кадр
  `golden/preferences/prefs-b4-settings.png`. Детали — §34
  `docs/analysis/visual-1to1-status.md`.
- **B1: оставшиеся пункты меню юнита — 1:1 с C++-оракулом.** `attack` теперь
  тумблер `targeting_mode` (`units_manager.cpp:617`): наведение, `CURSOR_ENEMY`
  и стрельба по пустой клетке в радиусе (`GameManager_SetUnitOrder`,
  `game_manager.cpp:6360` → `ORDER_FIRE`). `load` — курсорный режим 7
  (`:1011`, `GameManager_ProcessMouse` CURSOR_LOAD `:6807`): `ORDER_MOVE_TO_UNIT`
  получает пассажир, `ORDER_LAND`/`INIT` с `parent` — в клетке транспорта.
  `disable`/`steal` — режимы 9/8 (`:921/910`) с `ORDER_AWAIT_DISABLE/STEAL_UNIT`
  и `stealth_dice_roll`; порт `UnitsManager_IsValidStealthTarget` (`:1370`).
  Бульдозер: `Access_GetRemovableRubble` + `ORDER_CLEAR` (parent, `build_time`
  4/1). `remove` здания — порт `UnitsManager_SelfDestructMenu` (`:292`):
  `SELFDSTR`, `Arm`/`Cancel`, кадры `SLFDOPN1..6`, `OK` → `ORDER_EXPLODE`.
  `Upg. All` — `UnitsManager_Popup_OnClick_UpgradeAll` (`:414`): материалы
  комплекса, `ORDER_UPGRADE`/`EXECUTING_ORDER` с `parent = self`, сообщения с
  римской маркой. Остались disabled с причинами: `activate` (`RepairShopMenu`
  не портирован) и мастер-строитель.
- **B2: Help-пейджинг 1:1.** Кнопка `?`/`Help` включает режим помощи
  (`HelpMenu_Menu`, `helpmenu.cpp:315`): `CURSOR_HELP`, клик по элементу →
  `HelpMenu_ScaleCursor` (`:412-471`) и `Help::GetEntry` (`help.cpp:140`) →
  окно показывает **одну** запись `help.json` (а не всю секцию). Скролл,
  `Keys`, `Done`, возврат в режим по `Esc` — как в C++; добавлены QA-команды
  `help entry NAME`, `help click X Y`, `help close`, `assert help-entry`.
- **B3: курсор площадки инженера.** `GameManager_GetBuilderUnitCursor` для
  `ENGINEER` (`game_manager.cpp:5854-5909`): 1×1-лента в клетке курсора, только
  в строке/столбце строителя; кнопка `Path` (`BLDPTH_U`, `buildmenu.cpp:836`,
  `event_click_path_build`) в `BuildMenu` инженера; `BuildMenuAction::PathBuild`
  → выбор места `ORDER_STATE_SELECT_SITE`.
- **D1: звуки меню/попапов/стройки/транспорта.** Модуль `sfx.rs`: `KCARG0`,
  `NDONE0`, `NCANC0`, `MBUTT0`, `NHELP0`, `NHUMN0/NCOMP0/NNONE0/NCLAN0`,
  `V_M049` (постановка площадки), `V_M210/V_M085/V_M224` (ремонт/дозаправка/
  передача) и `GameManager_PlayUnitStatusVoice` (`game_manager.cpp:7943`) с
  `V_M093`. Очередь `App::play_sfx` проигрывается `main.rs` через `AudioPlayer`
  с учётом `disable_fx`/`disable_voice` и громкостей.
- **D3: `Alt+P` — пауза/настройки в игре.** `PauseMenu_Menu` (`menu.cpp:1747`)
  → `DialogMenu` (`dialogmenu.cpp:148-204`): окно `HELPFRAM`, «Game Paused.
  Click OK to continue.» (`_(f9bb)`), `OK`/`Enter`/`Esc`; тики и таймер
  локальной партии на время диалога стоят. `Alt` теперь доходит до UI
  (`WindowKey::Alt` через SDL `keymod` → `Key::Alt`).
- **QA/golden:** `unit-menu.qa` (новые пункты, `spawn`, `selfdestruct`,
  `cursor=…`, `targeting=…`), `research-help.qa` (режим помощи + запись по
  клику), `preferences.qa` (`PREFS_MENU_SETUP` через режим, `Alt+P`);
  пересняты `golden/research-help/*`, `golden/unit-menu/*`, добавлены
  `unit-menu-targeting/load/infiltrator`, `self-destruct-arm/armed`,
  `pause-dialog`, `help-options-playmode`.

- **A4: строитель остаётся в ЦЕНТРЕ ленты после достройки.** `SpawnNewUnit`
  (`unitinfo.cpp:3440-3592`) не двигает пиксель `x`/`y` строителя: сдвиг
  `PositionInTape` (+32 px к центру 2×2) снимается только в
  `TAPE_POSITIONING_DEINIT`/`LEAVE`, то есть по приказу игрока. Порт
  (`max_app/src/buildsite.rs::tape_center_offset`) для `ORDER_BUILD`/
  `ORDER_STATE_UNIT_READY` возвращал `0` и «выталкивал» спрайт в угол ленты —
  теперь там те же `(±32, ±32)`. Заодно `ORDER_ACTIVATE`/
  `ORDER_STATE_IN_TRANSITION` (кадр между кликом игрока и тиком обработки) тоже
  остаётся в центре: C++ ещё не менял `x`/`y`. Тесты:
  `tests/app.rs::the_builder_stays_in_the_tape_centre_after_completion` (кадр:
  маркер строителя в центре ленты и **не** в углу клетки),
  `buildsite::tests::tape_center_offset_keeps_the_builder_on_the_tape_while_waiting`
  (`BUILD_IN_PROGRESS`/`UNIT_READY`/`IN_TRANSITION`).
- **A5: маркеры «клеток с символом» `BLDMRK1..5` — ровно кольцо C++.** Сверено
  с `DrawMap_RenderBuildMarker` (`drawmap.cpp:353-390`) и `DrawMap_RedrawUnit`
  (`:304-337`): старт `(parent.x − 1, parent.y + 1)`, для `BUILDING` `+1` по `y`,
  затем N/E/S/W по `limit = 3` шага накопительно, фильтр
  `Access_IsAccessible(… SameClassBlocks)` — 12 клеток вокруг 2×2, **не** клетки
  самой площадки; картинка `BLDMRK*` 62×62 (hotspot 31,31) в центре клетки
  (`DrawMap_Callback2`, `:339-351`); кадр меняется каждые три отрисовки карты
  (`DrawMap_BuildMMarkDelayCounter`, 5 кадров). Порт уже рисовал это кольцо;
  добавлен точный тест `tests/app.rs::build_marker_cells_are_the_cpp_ring_around_a_two_by_two_site`
  (набор клеток = кольцо, ни одна клетка площадки не помечена, число маркеров =
  числу клеток) и `assert build-center=yes` в `production.qa`. В полноэкранном
  fallback-кадре маркеры теперь тоже рисуются (раньше только в оконном режиме).
- **A6: иконки ресурсов — только разведанные клетки, только SURVEY/разведчик и
  ПОВЕРХ зданий.** Порт `Survey_SurveyArea` (`survey.cpp:84-121`):
  `Access_UpdateMapStatus` (`access.cpp:762-768`) при добавлении покрытия для
  `SURVEYOR`/`MINELAYR`/`SEAMNLYR`/`COMMANDO` обходит 3×3 (`grid ± 1`), пропускает
  `AIR` и **разведчик** ставит в `ResourceManager_CargoMap` бит команды
  (`hash_team_id`, `RED = 0x2000`). Знание накопительное (`|=`) и **входит в
  состояние** (снапшот/CRC/сейв) — детерминизм сохранён. Исправлено
  соответствие битов: `HASH_TEAM_RED = 0x2000` (бит 13), `GREEN = 0x1000`,
  `BLUE = 0x0800`, `GRAY = 0x0400` — раньше `ResourceMapEntry::is_visible_to`
  считал биты в обратном порядке. Отрисовка
  (`GameManager_RenderSurveyIndicator`, `game_manager.cpp:991-995`) включается
  только кнопкой SURVEY или выделенным разведчиком
  (`Access_IsSurveyorOverlayActive`, `access.cpp:341`), маска —
  `CargoMap & hash_team_id` (`Survey_RenderMarkers`, `survey.cpp:137-151`), и
  идёт **после** юнитов (`DrawMap_RenderUnits` → `RenderSurveyIndicator`), поэтому
  иконки ложатся поверх зданий. Тесты:
  `max_core/tests/survey.rs` (5 тестов: 3×3 для своей команды, `AIR`,
  накопление, только `SURVEYOR`, состояние/детерминизм, `ORDER_DISABLE`),
  `tests/app.rs::resource_icons_are_survey_knowledge_only_and_above_buildings`
  (кадр: без оверлея 0 иконок; разведчик → только разведанная клетка; SURVEY →
  та же маска; пиксели клетки меняются ⇒ иконка поверх здания); QA
  `visual-screens.qa` (`assert survey-overlay=yes/no`, `assert resource-icons>=1`,
  кадры `survey`, `survey-button`).
- **A7: анимация RADAR после достройки.** `RADAR` (как турели и мины) рождается
  в `ORDER_SENTRY` (`units_manager.cpp:2412-2421`), а `SpawnNewUnit` переводит
  здание в `ORDER_IDLE`/`BUILDING_READY` через `SetNewOrderInt`
  (`unitinfo.cpp:3581` + `units_manager.cpp:1969-1975`), который сохраняет
  `prior_orders = ORDER_SENTRY`. После ухода строителя `BuildingReady` делает
  `RestoreOrders` (`units_manager.cpp:4240`, `unitinfo.cpp:3783`) → снова
  `ORDER_SENTRY`, и `ProcessOrderSentry` листает 16 кадров `D_RADAR`
  (`image_base = 0`, `image_count = 16`). Порт жёстко писал
  `prior_orders = AWAIT`, поэтому построенный радар замирал в `ORDER_AWAIT` —
  «анимация радара сломана». Теперь prior берётся из `set_new_order_int`; кадр
  считается как `image_base + (angle + step) % image_count` (C++ сбрасывает на
  `image_base`, а не на `image_base + angle`). Тесты:
  `max_core/tests/production.rs::a_built_radar_restores_its_sentry_order`,
  `tests/app.rs::a_built_radar_keeps_rotating_its_sixteen_frames` (кадр: после
  достройки и ухода строителя 16 шагов `dt = 1/30` дают другой кадр, полный
  оборот и `dt = 0` возвращает исходный — golden детерминированы).
- **A8: индикатор выделенного юнита на карте (`GameManager_DrawUnitSelector`).**
  Порт `game_manager.cpp:744-836` и вызова из `DrawMap_RenderUnit`
  (`drawmap.cpp:813-833`): три вложенные рамки-уголка (`double_marker = true`),
  `size1` 32/16, `size2` 128/64, «полуразмер» 64/32 в мировых пикселях,
  цвет — `GameManager_MarkerColor` (`0xFF`/`0x00`) с чередованием `color ^= 0xFF`;
  мигание каждые `TIMER_FPS_TO_MS(24) * 5 = 210 мс` (`game_manager.cpp:1189-1192`).
  У зданий (2×2) уголки вдвое длиннее, как в C++. `dt = 0` (QA/headless) даёт
  `0xFF`. Тесты:
  `tests/app.rs::the_selected_unit_gets_the_cpp_corner_selector_and_it_blinks`
  (кадр: пиксели рамок, смена цвета через 210 мс, снятие выделения),
  `assert unit-selector=yes` в `production.qa`/`visual-screens.qa`.
- **Общие правки паритета, выявленные этими пунктами.**
  `UnitInfo::new` теперь, как конструктор C++ (`unitinfo.cpp:201-204`),
  рождается в `ORDER_AWAIT`/`ORDER_STATE_EXECUTING_ORDER` (было `…/INIT`), из-за
  этого `RestoreOrders` возвращает здание в `AWAIT`/`EXECUTING_ORDER`; векторы
  `tools/sim_vectors` перегенерированы (`main.cpp`: `state`/`prior_state` = 1).
  `NewGameReport::resources_crc` считается сразу после генерации карты — до
  высадки разведчиков, чтобы не смешивать генерацию с накопленным знанием.
  QA: новые проверки `assert resource-icons[OP]N`, `assert survey-overlay=yes|no`,
  `assert unit-selector=yes|no`; golden переснят осознанно
  (`visual-screens`, `production`, `move-input`, `unit-menu`, `xfer`, `upgrade`,
  `allocate`, `research-help`, `mine-selected`/`portrait` + новые `survey`,
  `survey-button`), повторная сверка — 0 дрейфа.
- **A1: строитель уходит с достроенной площадки только по приказу игрока.** Убран
  «автовыход» (`TaskActivate`/`ActivateEngineer`, который порт вызывал сам в
  `UnitsManager_ProcessOrderBuild`/`UNIT_READY`). Теперь ровно как
  `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3440-3592`): здание встаёт на клетку
  ленты в `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY` и не рисуется
  (`UnitInfoGroup::IsRelevant`), строитель остаётся в `ORDER_BUILD`/
  `ORDER_STATE_UNIT_READY` на площадке (`ProcessOrderBuild`/`UNIT_READY` только
  анимирует), лента/конусы/плита на месте. Уход заказывает **игрок** кликом по
  клетке — `game_manager.cpp:6879-6890`: `move_to_grid` = клетка клика,
  `UnitsManager_SetNewOrder(ORDER_ACTIVATE, ORDER_STATE_IN_TRANSITION)` (пакет
  08/P3, `hud::build_builder_exit_order`); дальше `PositionInTape` DEINIT/LEAVE
  (доворот + 4×8 px), `ActivateEngineer` (лента/конусы снимаются), `ORDER_MOVE`;
  `UnitsManager_BuildingReady` раскрывает здание (`ScaleUnit(EXPAND)`), когда
  строитель вышел из габаритов 2×2 — плита (`remove_slabs = false`) остаётся
  фундаментом. ИИ делает то же задачей `TaskCreateBuilding::Activate` →
  `TaskActivate` (`taskactivate.cpp:104`, `unit_order::issue_builder_exit`), поэтому
  соло-ИИ не зависает. Бонус: мигающие маркеры `BLDMRK1..5`
  (`DrawMap_RenderBuildMarker`/`DrawMap_RedrawUnit`) вокруг площадки — та самая
  «клетка с символом» из подсказки `_(276c)`. QA `production.qa` (шаги:
  достройка → строитель ждёт → `move near` игроком → `assert unit-order=activate`
  → уход → здание), `research.qa`/`research-help.qa`; тесты
  `max_core/tests/production.rs` (30 тиков без приказа ничего не раскрывают),
  `max_core/tests/ai_p6.rs` (ИИ отдаёт `ORDER_ACTIVATE` и здание появляется),
  `max_app/tests/app.rs` (кадр с маркерами).
- **A2: таймер хода 1:1 (`GameManager_DrawTurnTimer`).** Панель показывает
  **остаток времени хода** в окне `WINDOW_TURN_TIMER_DISPLAY` (534,2…594,22,
  ресурс `TIMER` 61×21), `FONT5`, формат `"%2.2i:%2.2i"` (минуты:секунды с
  нулями слева), по центру по обеим осям (`Text_TextBox` с
  `horizontal_align`/`vertical_align = true`), цвет `0xA2`, а при `turn_time <= 20`
  — `COLOR_RED` (`0x01`); ноль (таймер выключен) тоже красный. Геометрия цифр
  сверена с кадром DOS-оригинала: «01:41» занимает x 551…575, y 9…15 — совпадает
  пиксель в пиксель. Значение раньше не приходило в панель вообще
  (`PanelState::timer_seconds` был мёртвым полем, всегда «00:00»); теперь
  `App::turn_timer_seconds()` отдаёт `GameManager_TurnTimerValue` локальной
  партии (`ceil`), а ход компьютера стартует с `settings["endturn"]`, но не
  меньше 45 с (`game_manager.cpp:2426-2475`) и тоже тикает. Тесты
  `panels::tests` (формат/цвет/геометрия), `app.rs`
  (`chrome_timer_shows_the_turn_timer_value_with_the_cpp_colors`).
- **A3: кнопка End Turn — нажатый вид и цвет активной команды.** Как
  `GameManager_MenuClickEndTurnButton` (`game_manager.cpp:3977`) +
  `GameManager_UpdateGui`: на ходу человека (`TEAM_TYPE_PLAYER`) кнопка отжата
  (`ENDTRN_U`) с подписью `FontColor(0xA0, 0xA2, 0xE6)`; на ходу компьютера
  (`TEAM_TYPE_COMPUTER`) — нажата (`SetRestState(true)`) ресурсом
  `R_ENDT_D/G_ENDT_D/B_ENDT_D/W_ENDT_D` (`CopyDown(R_ENDT_D + team)`) и подписью
  «End Turn» цветом команды `FontColor(team + 1, team + 1, 0x3F)`
  (`game_manager.cpp:7860-7883`, у GRAY `0xFF`). В hot-seat у обоих людей кнопка
  отжата. QA `ai-turn.qa` (мой ход → ход ИИ → возврат; `assert endturn=normal|pressed`),
  golden `golden/ai-turn`; тесты `panels::tests` и `app.rs`
  (`chrome_end_turn_is_pressed_with_the_team_color_on_an_ai_turn`, три фазы).

- **X-FER 1:1: `TransferMenu` + `ProcessOrderTransfer`.** Пункт `x-fer` меню
  юнита больше не серый: у юнитов с грузом `RAW..GOLD` (приказ ≠ `CLEAR`/`BUILD`)
  он включает `cursor = 3` (`UnitsManager_Popup_OnClick_Transfer`), а клик по
  своему юниту идёт в `GameManager_TransferCargo` — с подменой цели на здание
  комплекса с нужным складом (`GameManager_GetUnitWithCargoType`) и `SetParent`.
  Окно `XFERPIC` 308×162 в области карты (249,160) — 1:1 по `transfermenu.cpp`:
  иконки/имена юнитов, запас источника и цели (`Complex::GetCargoInfo` для чужого
  комплекса, иначе `storage`/`ATTRIB_STORAGE`), `LimitedScrollbar`
  (`SMBRRAW/FUEL/GOLD`, `give_max/take_max`, клик по полосе и `±1`), стрелка
  направления, `Cancel`/`?`/`Done`, `Transfer_SETUP` по `?`. `Done`/`Enter`
  отдаёт **08/P3** (`parent_unit`, `transfer_cargo`), `ProcessTransfer` переносит
  материалы/топливо/золото в обе стороны: `TransferRaw/Fuel/Gold`,
  `Complex::Transfer` при нехватке `storage`, `RestoreOrders`. `Esc`/нулевое
  количество запасы не меняют; после закрытия снимается курсорный режим (как
  `~TransferMenu` → `MenuUnitSelect`). QA `xfer.qa` (1332 кадра) + golden
  `golden/xfer`; `unit-menu-builder.png` переснят (x-fer зелёный).
- **Учёт хранилищ комплекса 1:1 (`ADUMP`/`FDUMP`/`GOLDSM`).** `UnitInfo::UpdateProduction`
  теперь разбирает запас комплекса по складам: `storage = min(complex.material,
  ATTRIB_STORAGE)` и `complex.material -= storage` (так же fuel/gold), поэтому
  добыча шахт (`Access_UpdateResourcesTotal`) попадает в `ADUMP` (50), `FDUMP`
  (50), `GOLDSM` (50) и склад самой шахты (25), а не остаётся кэшем комплекса.
  Излишек сверх суммы ёмкостей теряется на следующем пересчёте — как C++; без
  склада топливо/золото сохранить негде. Также 1:1: `COMMTWR` (+5 золота),
  `GREENHSE` (`++storage`/`++team_points`), сырьё мобильного строителя
  (`Cargo_GetRawConsumptionRate`), `Resupply`/`Reload` через
  `Complex::Transfer(-1,0,0)`. Тесты `max_core/tests/transfer.rs` (12 с/без
  хранилища, лимит, переполнение, детерминизм) + `xfer.qa`/`allocate.qa`.

- **Подсказка строящего строителя сверху карты (C++ `MessageManager_DrawMessage`).**
  Клик по строителю в `ORDER_BUILD` показывает окно `MESSAGE_BOX_INFO` над картой
  (`GameManager_UnitSelect` → `GameManager_DrawUnitStatusMessage` →
  `GameManager_DrawBuilderUnitStatusMessage`, `game_manager.cpp:7893/8038`):
  «Currently building: X.» / «Turns to completion: N.» (`_(ee24)`; ходы —
  `UnitInfo::GetTurnsToBuild`), «Was building X, with N turns …» (`_(cdf8)`) для
  остановленной стройки, «Finished building X. To move the Y out of the
  construction site, …» (`_(276c)`) для `ORDER_STATE_UNIT_READY`, «Number of turns
  to clear site: N.» (`_(af4a)`) для бульдозера. Фон — таблица затемнения
  `MESSAGE_BOX_INFO`, текст `FONT5` с обводкой, перенос по словам
  (`MessageManager_WrapText`), прямоугольник `(map.ulx, map.uly+10)`, высота
  `20 + 10·строк`; закрывается кликом по карте/сменой выделения
  (`MessageManager_ClearMessageBox`). QA: `assert hint=TEXT|none`, кадр
  `build-hint`.

- **Диалог Preferences/Setup 1:1 (C++ `OptionsMenu`, P8).** Порт `optionsmenu.cpp`:
  панель `PREFSPIC` 400×422 в (120,29), заголовок `Setup`/`Preferences`
  (`FONT5`, `COLOR_GREEN`), кнопки `PRFDNE/PRFCAN/PRFHLP` (Done/Cancel/Help),
  раскладка `window_uly` (секция +20, `ulx=25` +20, label −7), слайдеры
  `PRFSLIT`/`PRFSLIDE`, чекбоксы `UNCHKED`/`CHECKED`, поля `PREFEDIT`/`PREFNAME`.
  Открытие: пункт `Setup` главного меню (`menu_setup_window`, `SETUPPIC`) и
  кнопка `Preferences` игровой панели (`GameManager_MenuClickPreferencesButton`,
  `PREFSPIC`); диалог модален, `Done` сохраняет, `Cancel` возвращает снимок,
  `?`/`Help` — `PREFS_MENU_SETUP`/`SETUP_MENU_SETUP`.
  Реально действуют: `effects` (кадры строителя/`ANIMATED`/работающих заводов
  статичны), `disable_fx` (звук SFX вкл/выкл — `AudioPlayer`), `fx_sound_level`
  (громкость), `auto_save` (автосейв слота 10). Остальные опции
  (`click_scroll`, `quick_scroll`, `fast_movement`, `follow_unit`,
  `auto_select`, `enemy_halt`, `disable_music/voice/movie`, уровни
  Music/Voice/Video, `enhanced_graphics`, `host_address/port`,
  `Turn Time`/`End Turn Time`, `Player Name`) сохраняются, но в игру пока не
  идут — помечено в `docs/analysis/visual-1to1-status.md` §27.
  Сохранение: `IniPreferences` (7 × `signed int`) в V70-сейв
  (`SaveMeta::ini_preferences`, как `SaveLoad_Save`), чтение при загрузке
  (`SaveLoad_Load`), плюс `preferences.ini` рядом с сейвами. QA:
  `prefs open|toggle NAME|set NAME N|done|cancel|help`, `assert prefs=…`,
  `assert prefs-value NAME=N`, `prefs-effects/sound/audio`, сценарий
  `preferences.qa`, golden `golden/preferences`; unit/integration
  `preferences.rs` (9) и `tests/preferences.rs` (6).

### Исправлено

- **Три расхождения с C++-оракулом: увод строителя, след снарядов, вспышка
  выстрела.**
  * **Строитель уходил, а стройка продолжалась.** Любой новый приказ
    мобильному строителю (`ORDER_BUILD`, площадка стоит) теперь сначала снимает
    стройку — порт `UnitInfo::CancelBuilding` (`unitinfo.cpp:4231`, ветка
    `!STATIONARY`) через `unit_order::cancel_build_for_new_order`:
    `build_list.Clear()`, `build_time = 0`, `Access_DestroyUtilities(grid, true,
    false, false, false)` (лента, конусы и плита; дорога/обломки/коннекторы не
    трогаются), `path = nullptr`, `ORDER_AWAIT`/`EXECUTING_ORDER`, материалы не
    возвращаются. Вызывается из `GameState::set_order` (ИИ/группа/прямой путь) и
    из приёма пакетов 08/38 (сеть), поэтому состояние совпадает у всех пиров;
    `prior` после отмены — `AWAIT` (пакет 08 несёт старый `BUILD` в
    `prior_order`). Заодно исправлен набор флагов `Access_DestroyUtilities` в
    штатной отмене (`ProcessOrderBuild`/`BUILD_CANCEL`,
    `ProcessOrderHaltBuilding`): было `true,true,true,true`, стало C++-шное
    `true,false,false,false`. HUD пишет `Build cancelled at (x, y)`.
  * **У снарядов не было следа.** C++ `Paths_DrawMissile`
    (`airpath.cpp:362-406`) на каждом переходе снаряда в новую клетку ставит
    три юнита-эффекта в **покинутую** клетку: `RKTSMOKE` у `ROCKET` (5 кадров
    `D_RKTSMOKE`) и `TRPBUBLE` у `TORPEDO` (3 кадра `D_TRPBUBLE`), флаги из
    `SC_UNITS` (`EXPLODING|ANIMATED|MISSILE_UNIT`, у пузырей ещё
    `GROUND_COVER`), `ORDER_EXPLODE`/`DESTROY`; `UnitsManager_ProgressExplosion`
    листает кадр за два тика и удаляет на `image_index_max` (4/2);
    `OffsetDrawZones` разносит частицы на 0/⅓/⅔ отрезка. Порт —
    `unit_order::spawn_missile_trail`: три частицы в покинутой клетке, разнос
    хранится в пиксельных `x/y` (рендер эффектов берёт их через
    `render::effect_pixel_anchor`), видимость — как `InitStealthStatus` (своя
    команда), поэтому чужие следы не рисуются. Спавн не зависит от
    `GameManager_PlayerTeam` (локальное UI-поле развело бы состояние клиентов,
    DESIGN §6.1) — в одиночной игре результат 1:1.
  * **Не было вспышки выстрела.** C++ `UnitInfo::PrepareFire`
    (`unitinfo.cpp:4709-4747`) выставляет `firing_recoil_frames` (3; ×5 у
    `ANTIAIR`, ×2 у `SP_FLAK`/`FASTBOAT`; 8 у `COMMANDO`/`INFANTRY`) и рисует
    кадр `firing_image_base`: у башни `DrawSpriteTurretFrame(turret_angle +
    firing_image_base)`, у корпуса `DrawSpriteFrame(angle +
    firing_image_base)` (пехота — `104 + angle`, затем `+8` за тик);
    `ProgressFire` (`:4824`) уменьшает счётчик, у `ANTIAIR`/`SP_FLAK`/
    `FASTBOAT` переключает башню `±8`, на нуле возвращает обычный кадр.
    `max_app::render::{firing_body_frame_index, firing_turret_frame_index}` —
    порт выбора кадра из `D_*` (`firing_image_base`/`firing_image_count`,
    которые уже экспортируются в `max-rust-3d-models` как `firing/`), подключён
    в общую точку отрисовки `draw_unit_sprites_with_animation`; `dt = 0` кадры
    golden-эталонов не двигает.
  * Тесты: `max_core/tests/builder_cancel.rs` (4: `set_order`, пакеты 38/08,
    штатный `stop`), `max_core/tests/turn_based_combat.rs` (+2: три частицы на
    клетку, тип/кадры/удаление/детерминизм), `max_app/tests/
    build_lifecycle_interactive.rs` (+1: RMB-увод в живой партии Turn Based),
    `max_app/tests/unit_sprites.rs` (+1: кадры `firing_image_base` для
    `TANK`/`ARTILLRY`/`ANTIAIR`/`SP_FLAK`/`INFANTRY`). QA: `combat-tb.qa` —
    `assert firing=on`, `assert trail=on`, `assert unit-count RocketSmoke>=3`,
    кадры `combat-tb-trail`, `combat-tb-tank-fired`, `combat-tb-tank-restored`;
    новый `build-cancel.qa` (3 кадра); новые ассерты `firing`, `building`,
    `trail`, `unit-count NAME<оп>N`. Golden `combat-tb` перегенерирован
    (11 кадров), новый `golden/build-cancel` (3). Детали — §37
    `docs/analysis/visual-1to1-status.md`.
- **Turn Based: зависание хода ИИ после выстрела/гибели юнита (три порта
  оракула) и настоящая анимация снарядов/взрывов.**
  * **Снаряд навсегда оставался в `ORDER_MOVE`.** `UnitInfo::PrepareFire`
    создаёт `ROCKET`/`TORPEDO`/`ALNMISSL` как юнит с `speed = 0` из `SC_ATTRI`
    (это эффект, а не машина). C++ `AirPath::Execute` проверяет скорость только
    **внутри** `if (unit->flags & MOBILE_AIR_UNIT)` (`airpath.cpp:186-199`), а
    `unit_order::air_path_execute` применял проверку ко всем: снаряд переходил в
    `EXECUTING_ORDER` на стартовой клетке и больше не двигался. Каждый тик
    `UnitsManager_ProcessOrder` выставляет `OrdersPending` для `MISSILE_UNIT`
    (`units_manager.cpp:3268-3271`), поэтому `Ai_CheckEndTurn` не вызывался и
    ход ИИ не закрывался до аварийного `LOCAL_AI_TICK_LIMIT` (12 000 кадров).
    Это и был «фриз на ходу»: `combat-tb.qa` крутил 12 000 кадров вместо 169.
  * **Взрыв не становился взрывом.** `spawn_unit` проверял `ANIMATED` по
    флагам вызывающего слоя, а `UnitsManager_StartExplosion` передаёт только
    класс (`MOBILE_LAND`/`MOBILE_AIR`/`MOBILE_SEA`/`STATIONARY`): `ANIMATED`
    приходит из `SC_UNITS`. Эффекты `LNDEXPLD`/`AIREXPLD`/`SEAEXPLD`/
    `BLDEXPLD`/`HITEXPLD` рождались в `AWAIT/EXECUTING_ORDER` и оставались на
    карте навсегда. Теперь флаги берутся из `merged_flags`, как `UnitInfo` +
    `UnitsManager_DeployUnit` (`units_manager.cpp:2390`).
  * **Погибший юнит не удалялся.** `UnitsManager_ProgressExplosion`
    (`units_manager.cpp:4006-4032`) на половине анимации отпускает родителя и
    уничтожает его, если `hits == 0`. Порт этого не делал: убитый юнит вечно
    висел в `ORDER_STATE_DESTROY`, `OrdersPending` не сбрасывался — ход ИИ
    снова не закрывался (ровно этот путь воспроизводит `combat-tb.qa`:
    ИИ-танк убивает ракетчика). Порт — `progress_explosion` +
    `GameState::destroy_unit`.
  * **Кадры эффектов.** Добавлен `max_core::frames` — порт
    `Unit::GetFrameInfo()` (93 записи `D_*`: `image_base`, `image_count`,
    кадры башни, данные из `MAX.RES`/`PATCHES.RES`). `UnitInfo::new` теперь
    выставляет `image_index = image_base + angle`,
    `image_index_max = image_count + image_index - 1` (у `COMMANDO`/`INFANTRY`
    — `103`, у вращающейся башни — по кадрам башни, `units_manager.cpp:2443`).
    Взрыв листает `image_index 0..13` (LND/AIR/SEA), `0..27` (BLD), `0..4`
    (HIT) по кадру за два тика и удаляется; у пехоты/диверсанта — своя
    8-кадровая смерть `image_base = 168 + (angle/2)*8`
    (`units_manager.cpp:3932`). Рендер `EXPLODING`-юнитов рисует
    `image_index` (`UnitInfo::Render`), а не статичный кадр.
  * **Видимость эффекта.** Взрыв принадлежит команде погибшего; видимость
    цели копируется на эффект, иначе атакующий не видел попадание
    (в C++ `DeployUnit(..., skip_map_status_update = true)` не пересчитывает
    heat map).
  Тесты: `max_core/tests/turn_based_combat.rs` (3: ход ИИ закрывается после
  попадания ракеты, типы взрывов для land/air/sea/building + кадры и удаление,
  промах → `HITEXPLD` и удаление), `max_app/tests/turn_based.rs` (3: ракета в
  полёте на End Turn первого хода → возврат за <3000 кадров без остатков-
  эффектов; горячий-seat 2 человека + компьютер; 4 раунда SP),
  `max_app/tests/unit_sprites.rs::frame_info_table_matches_game_data`
  (таблица `D_*` сверена с ассетами). QA: `combat-tb.qa` (выстрел ракетой,
  снаряд, взрыв по кадрам, `assert endturn-frames<2000`), `sp-tb.qa` (Turn
  Based с двумя компьютерами, `RED→GREEN→BLUE→RED`, `endturn-frames`), golden
  `golden/sp-tb/`, `golden/combat-tb/`. Soak по умолчанию — Turn Based
  (`SoakOptions::play_mode`, `soak_headless --mode tb`), `soak-100.qa` уже
  Turn Based. Детали — §36 `docs/analysis/visual-1to1-status.md`.

- **Клик по достроенной площадке выделяет СТРОИТЕЛЯ (маркеры `BLDMRK*`,
  подсказка «Finished building …» и уход с площадки снова работают).**
  Регрессия после A1/A4. `PositionInTape` держит строителя в центре ленты
  (`tape_center_offset` = ±32 px) и после достройки, поэтому его спрайт стоит в
  угле четырёх клеток 2×2, а не в своей `grid`. Игрок кликал по нарисованному
  строителю, попадал в соседнюю клетку площадки, и `hud::unit_at_screen` (порт
  `Access_GetTeamUnit`, `access.cpp:1804`) отдавал **декаль**: конусы/плиту —
  они не `ORDER_IDLE`, а фильтра `SELECTABLE` и правила «лента → родитель» в
  порте не было (декали не `SELECTABLE` — в C++ их клик не берёт вообще).
  Следствия: не рисовались маркеры `BLDMRK1..5` и не появлялась подсказка
  (`render::build_marker_cells`/`panels::builder_status_message` требуют
  выделенного строителя в `ORDER_BUILD`/`UNIT_READY`); правый клик по карте с
  выделенной декалью отменял приказ («Unit cannot move (air/stationary)»),
  поэтому строителя нельзя было увести, а `UnitsManager_BuildingReady`
  (`units_manager.cpp:4183`) не раскрывал здание. Теперь `unit_at_screen` —
  дословно C++: `Access_GetTeamUnit` (свои: `SELECTABLE` + не `ORDER_IDLE`,
  сперва без `GROUND_COVER`/лент, затем любой `SELECTABLE`) и
  `Access_GetEnemyUnit` (`access.cpp:1731`: чужие, мины — исключение из
  `GROUND_COVER`), а найденная лента возвращает `unit->GetParent()` —
  строителя. Плюс закрыт путь игрока из C++: LMB по клетке «рядом со зданием»
  (`GameManager_IsUnitNextToPosition`, `game_manager.cpp:3208` — 4×4 вокруг
  2×2, то есть кольцо с символами) отдаёт `ORDER_ACTIVATE`/`IN_TRANSITION`
  (`hud::builder_exit_zone_contains`/`build_builder_exit_click_order`,
  `:5832-5843` + `:6879-6890`), как и обещает подсказка `_(276c)`.
  `dt` тут не при чём: `local_update` тикает один раз за кадр и при `dt = 0`, и
  при `dt > 0` — разница интерактивного пути только во вводе (QA выделяет
  строителя командой `select type`, а не кликом по спрайту).
  Тесты: `max_app/tests/build_lifecycle_interactive.rs` (4; два полных цикла
  `local_update(1/60)` — маркеры после клика по нарисованному строителю, затем
  уход и раскрытие здания по LMB и по RMB; клик по декали возвращает
  строителя; зона 4×4 `IsUnitNextToPosition`), `hud::tests`
  (`ground_cover_slab_under_a_building_does_not_steal_the_click`). Golden не
  менялся (отрисовка не тронута).

- **Пункт `x-fer` больше не disabled; добыча шахт учитывается в лимите хранилищ.**
  Раньше `unitmenu` строил пункт с `.disabled("… не портированы (TODO P3)")`, а
  `GameState::update_production`/`resupply` были заглушками: запас комплекса
  (`Complex::material/fuel/gold`) не раскладывался по `storage` зданий, поэтому
  `ADUMP`/`FDUMP`/`GOLDSM` оставались пустыми, а добыча шахт «висела» в кэше
  комплекса без лимита. Теперь пункт включён (см. выше), а `update_production`
  выполняет шаг 5 C++ (`storage = min(complex.field, ATTRIB_STORAGE)`) — запас
  ограничен ёмкостями складов, излишек теряется на следующем
  `Access_UpdateResourcesTotal`, как в оригинале. `Reload` больше не TODO:
  боезапас берётся из комплекса через `Complex::Transfer(-1,0,0)`.

- **Достройка 1:1: строитель сначала УХОДИТ с площадки, потом появляется здание.**
  Раньше `ProductionManager::spawn_new_unit` сразу ставил готовое здание и
  снимал ленту — строитель оставался стоять внутри и был перекрыт зданием.
  Теперь порядок C++: `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3440`) ставит
  здание на клетку ленты в `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY`
  (не рисуется — `UnitInfoGroup::IsRelevant`) и переводит строителя в
  `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` (взаимный `SetParent`); строитель
  сходит с площадки (`TaskActivate`/`TaskCreateBuilding::Activate` →
  `ORDER_ACTIVATE`/`IN_TRANSITION` → `PositionInTape` DEINIT/LEAVE: доворот и
  4 × 8 px, клетка пересчитывается от пиксельной позиции `x / 64` →
  `ActivateEngineer` → `ORDER_MOVE` к свободной клетке вокруг здания), и только
  после этого `UnitsManager_BuildingReady` (`units_manager.cpp:4183`) снимает
  ленту/конусы (`remove_slabs = false` — плита остаётся), восстанавливает
  приказы и раскрывает здание `ScaleUnit(EXPAND)`. Добавлены
  `production::builder_exit_cell` (поиск клетки схода как `TaskActivate`),
  `production::unit_bounds_contains` (`GetBounds` 2×2/1×1),
  `unit_order::building_ready`/`return_to_construction_site`/`start_leaving_site`;
  `tape_center_offset` показывает и съезд с ленты. Тесты:
  `max_core/tests/production.rs::completion_waits_for_the_builder_to_leave_before_the_building_appears`,
  `completion_sequence_is_deterministic`; QA `production.qa` — кадры до/после.

- **Плита стройплощадки появляется мгновенно при подтверждении места.**
  `ProductionManager::commit` ставил площадку сразу, а путь «клик по карте»
  (`build place` → пакет 08/P4) ждал следующего тика (`ORDER_BUILD`/`INIT` →
  `StartBuilding`). Теперь `events::apply_command` ставит площадку
  (`DeployConstructionSiteUtilities` + `PrepareConstructionSite`) в момент
  применения заказа: `LRGTAPE`+`LRGCONES`+`LRGSLAB` видны на кадре
  подтверждения; `start_building` на следующем тике идемпотентен.

- **Зависание игры на 8-м ходу (ход ИИ не заканчивался 12 000 кадров).**
  Причина: `AiPlayer::CheckEndTurn` считал «есть работа» по статической
  проверке `TaskManager::has_pending_work` («есть незавершённые задачи»).
  Задача `CreateBuilding` ждала юнита, у которого кончилась скорость хода,
  поэтому флаг не сбрасывался никогда и ход ИИ не закрывался (QA
  `production.qa`: `end_turn` на 8-м ходу крутил 12 000 кадров). Теперь
  используется тот же флаг, что в C++ — `tasks_pending`
  (`Ai_SetTasksPendingFlag` из `UnitsManager_ProcessOrder*`/`ExecuteMove`/
  `PositionInTape`, сброс в `UnitsManager_ProcessOrders`): юнит, который не
  может ничего сделать в этом ходу, флаг не выставляет, и ход закрывается
  (`AiPlayer::CheckEndTurn`, `aiplayer.cpp:2828`). QA `production.qa` проходит
  ход 8 за 516 кадров, soak-100 (100 ходов) и soak_headless (120 ходов) — PASS.

- **Здание, ждущее ухода строителя, больше не выбирается кликом.**
  `ORDER_IDLE`-юнит не рисуется (`IsRelevant`), поэтому и `unit_at_screen` его
  пропускает: иначе игрок мог отдать приказ «недостроенному» зданию и навсегда
  оставить строителя в `ORDER_STATE_UNIT_READY` (так падал `research.qa`:
  подсказка «Finished building» и `tick` до раскрытия здания добавлены в
  сценарий).

- **Golden: три устаревших кадра `visual-screens` (0.25 % дрейфа).**
  `golden/game.png`, `golden/mine-selected.png` и `golden/portrait.png` были сняты
  до палитрового фикса `FRAMEPIC+WRL` (commit 04237e9) и не попали в его
  пересъёмку: у `POWGEN` оставались magenta-пиксели hot-point/палитры. Дрейф
  771 px (0.251 %) проходил порог 1 %, поэтому CI был зелёным; кадры пересняты
  текущим детерминированным выводом (два прогона `visual-screens.qa` совпадают
  побитово) — `qa_golden_check` даёт **0 пикселей на всех 15 кадрах**.

- **Палитровые циклы WRL (маяки зданий) и коннекторы комплексов — по DOS-съёмке.**
  DOS-съёмка (`max-original-dasm/formats/unit-animation.md` §7.3) показала, что
  мигание маяков — это **палитровые циклы**, а не кадры спрайта:
  `World::InitColorCycles` (`world.cpp:542`) вращает 11 диапазонов слотов DAC
  (9–12 @9 fps, 13–16 @6 fps, 17–20 @9 fps, 21–24 @6 fps, 25–30 @2 fps,
  31 @6 fps, 96–102/103–109 @8 fps, 110–116 @10 fps, 117–122/123–127 @6 fps).
  Порт переносит их 1:1 в `max_render::palette::{ColorCycles, rotate_palette,
  COLOR_CYCLES}`: часы копят `dt` по каждому циклу и вращают диапазон на шаг
  (`rotate_direction` 0/1, специальный одиночный слот 31 — зелёный канал
  63→31→15→7→63). Применение — **в рендере**, не в симуляции:
  `App::update_color_cycles(dt)` строит снимок системной палитры
  (`Assets::system_palette`), `App::animated_palette()` отдаёт его отрисовке
  юнитов; `dt = 0` сбрасывает часы, палитра исходная — golden не дрейфуют.
  Системная палитра — как в оригинале (`World::ApplyPaletteAndTints`): слоты
  0–63/160–255 из `FRAMEPIC`, 64–159 из `.WRL`; это же исправило пиксели
  спрайтов в слотах 2–31 (раньше они брались из `COLOR.PAL`, где 9–31 —
  маджента-заглушки). Маяки шахты (слоты 13–16, кадр `image_base + 1`) теперь
  мигают жёлтый → оранжевый → красный → оранжевый с фазой 166.7 мс и полным
  циклом 666.7 мс, индикатор дрели (21–24) вспыхивает красным 166.7 мс каждые
  666.7 мс — как в снятом AVI.
  Кадры `16..23` шахты и `2..5` у `POWGEN` — **не частицы бурения и не дым**, а
  коннекторы комплекса (`D_MINING.connector_image_base = 16/count = 8`,
  `D_SMLBLD.connector_image_base = 2/count = 4`). Порт переносит
  `UnitInfo::RenderWithConnectors` (`unitinfo.cpp:3151`): кадры
  `connector_image_base + n` (порядок N_L/N_R/S_L/S_R/E_T/E_B/W_T/W_B, смещения
  0/4/2/6/1/5/3/7) со своей тенью в том же якоре (горячая точка кадра задаёт
  сторону). Маска — `UnitInfo::connectors` (сохраняется в V70/CRC) ∪
  вычисленная для кадра копия `UnitsManager_UpdateConnectors`
  (`render::connector_mask_for`), пока симуляционный апдейт — заглушка; в
  плотной базе у шахты выставляются все 8 битов (DOS-съёмка §7.1 видела все
  восемь «частиц» одновременно), у `POWGEN` — четыре трубы N/E/S/W.
  **Шахта в idle больше не листает корпус** (§7.2: за ~118 с всегда кадр 0,
  включённая — `+1`): `working_building_animation_active` для шахты выключен,
  кадры дрели 1..15 отключены константой `render::MINING_DRILL_FRAMES_ENABLED =
  false` (нужна съёмка активной добычи). Цикл остался только у завода с
  очередью. Тесты: `max_render::palette` (7 новых: таблица/периоды/направления/
  слот 31/`dt = 0`), `tests/building_animation.rs` (маяки мигают, корпус
  статичен, коннекторы по маске, снапшот не меняется), `tests/building_frames.rs`
  (шахта статична, 8 коннекторов, 4 трубы `POWGEN`). QA/golden пересняты (35
  кадров в `allocate`/`production`/`move-input`/`unit-menu`/`upgrade`/`research`/
  `research-help`: +0.2–0.4 % пикселей — маяки и трубы), `qa_golden_check` —
  **0 дрейфа** на всех наборах.

- **Полный цикл кадров работающих зданий вместо «пульса» (промежуточное
  правило до DOS-съёмки).** Прежний индикатор работы (`activity_step`: два
  кадра состояния `image_base` ↔ `image_base + 1` раз в 0.5 с) заменён полным
  циклом кадров C++: у включённой шахты (`ORDER_POWER_ON`) и завода с непустой
  очередью (`ORDER_BUILD` + `build_list`) кадр листается по блоку
  `image_base + angle .. image_index_max`, где
  `image_index_max = image_count + image_index - 1` (`unitinfo.cpp:293`), по
  кругу со скоростью `RADAR`/`ProcessOrderSentry` — 1 кадр за 1/30 с
  (`render::WORKING_BUILDING_ANIM_FRAME_SECONDS`). Блок считает
  `render::working_building_frame_block`; он не выходит за кадры корпуса `D_*`
  (у `MININGST` 24-кадровая лента: 16 кадров корпуса + 8 частиц бурения, у
  заводов 2 кадра состояния + 8 коннекторов). `render::FrameAnimation::
  work_step` — номер кадра; шаг 0 даёт ровно статичный кадр C++ (`POWER_ON` →
  `+1`, завод → `image_base`), поэтому `dt = 0` (QA/golden) не дрейфует.
  Включение/скорость — одна строка: `WORKING_BUILDING_ANIMATION_ENABLED` /
  `WORKING_BUILDING_ANIM_FRAME_SECONDS`; `effects = false` тоже возвращает
  статичный кадр. Цикл строителя (`+16/+24/+32`) и `RADAR` (16 кадров
  `D_RADAR`) не менялись; симуляция/CRC/сейв не затронуты. Тесты:
  `tests/building_frames.rs` (8: 16-кадровый цикл шахты, 2-кадровый завод,
  `RADAR`, строитель), `tests/building_animation.rs` (4, на данных игры:
  тик 1/30 с, полный оборот, `dt = 0`, снапшот), `construction_animation` (3);
  QA `allocate.qa`/`production.qa` — `mine-work-a/b`, `factory-work-a/b`,
  `qa_golden_check` — **0 дрейфа** (правки ниже порога 1 %, пересъёмка не
  понадобилась); Windows UI пересобран. *(Цикл шахты в этой записи позже
  отменён DOS-съёмкой — см. запись выше.)*

- **Покадровая анимация работающих зданий и строителей.** В C++ есть ровно один
  потиковый цикл кадров зданий — `UnitsManager_ProcessOrderSentry`
  (`units_manager.cpp:3029-3041`): у юнитов с флагом `ANIMATED` каждый тик
  `image_index + 1`, после последнего кадра — `image_index = image_base`.
  Из зданий `ANIMATED` есть только у `RADAR` (16 кадров из `D_RADAR`, хотя в
  спрайте 20 — последние 4 коннекторы). У шахт/заводов покадровой анимации в
  C++ **нет**: есть два кадра состояния (`image_base`/`image_base + 1`),
  выставляемые `ProcessOrderPowerOn`/`PowerDown`; `UnitInfo::Animate` только
  добавляет draw bounds. Rust-рендер вообще не читал `image_index` и всегда
  рисовал `image_base + angle`, поэтому `RADAR` стоял. Теперь
  `render::FrameAnimation` (строитель `+8`/`-16`, `sentry_step` для
  `ANIMATED`) обновляется `App::update_unit_animations(dt)`; включённые шахта
  (`ORDER_POWER_ON`) и завод с непустой очередью (`ORDER_BUILD`) дополнительно
  чередуют два кадра состояния (0.5 с) — визуальный индикатор работы, которого
  в C++ нет. `dt = 0` (QA/headless) даёт статичный кадр C++, часы не входят в
  снапшот/CRC/сейв. Тесты: `tests/building_animation.rs` (4, на данных игры),
  `tests/building_frames.rs` (8, +4: шахта/завод/`RADAR`), 3 прежних
  `construction_animation`; QA `production.qa` — `factory-work-a/b`,
  `allocate.qa` — `mine-work-a/b`, golden `production/`/`allocate/` — 0 дрейфа;
  `bash ci.sh` — **754 с, все стадии PASS**.

- **`BuildMenu`: характеристики выбранного здания с иконками.** В C++
  `AbstractBuildMenu::Draw` (`buildmenu.cpp:531-536`) после портрета пишет
  `stats_background` (11,293,250,174) и вызывает `UnitStats_DrawStats` для
  **выбранной** строки (у завода — активного списка/очереди): военные строки,
  `Cargo`, `Prod./Uses`, `Armor/Hits/Scan`, `Speed`, `Cost`; числа и дельты
  апгрейдов — по текущим значениям команды, условия показа — по базовым;
  иконки — pips `I_*`, `Cost` — `I_GOLD`/`I_GOLDE`. В порте там был краткий
  текст строителя («Constructor / Hits / Armor / Build rate / Order»).
  Теперь `BuildMenu` переиспользует `CargoMenu`-код:
  `Mobilization::stats_rows_for(&team_units.base_values,
  &team_units.current_values, unit_type)` + `mobilize::draw_stats` в тех же
  координатах; клановые модификаторы и апгрейды уже внутри `current_values`.
  Тесты: `tests/build_menu_render.rs` (5, +1: таблица `STATS` нарисована,
  `Hits`/`Armor`/`Cost`, `Cost = 2 × 10 = 20` с `I_GOLD`/`I_GOLDE`), unit
  `buildmenu::stats_rows_for_a_building_carry_icons_and_the_cpp_cost`; QA
  `production.qa` — кадры `buildmenu`/`buildmenu-factory` с характеристиками.

- **Стоимость постройки по скорости — как `UnitInfo::GetTurnsToBuild`, с
  клановыми модификаторами и складом.** В C++ (`buildmenu.cpp:545-660`,
  `unitinfo.cpp:3393`) колонки `Turns`/`Cost` считаются так: 1× —
  `turns = GetTurnsToBuild(unit_type)` и `turns * Cargo_GetRawConsumptionRate(
  builder, 1)`; 2×/4× — цикл `while (turns > 0)`: `min(rate,
  BuildMenu_GetMaxPossibleBuildRate(builder, turns, storage))`, сумма ставок,
  `storage -= cost`, `turns -= rate`. То есть `Cost` — **полная** цена, а не
  ставка за ход, и ходы могут не делиться на скорость (при исчерпании склада
  скорость падает 2× → 1×). `ATTRIB_TURNS` берётся из
  `UnitsManager_GetCurrentUnitValues`, поэтому клановые `CLAN_TRADEOFFS`
  (например, CrimsonPath: SHIPYARD −5 ходов) сразу меняют и ходы, и цену.
  В порте `rate_numbers` показывал ставку за ход и `ceil(turns/rate)`, а
  `charge_units` не уменьшал `storage`, из-за чего фактические ходы/цена
  расходились с меню. Теперь `max_core::production::turns_and_consumption` —
  точный порт `GetTurnsToBuild`; `charge_units` уменьшает `storage` мобильного
  строителя на ставку (C++ `UpdateProduction`), поэтому реальная постройка
  совпадает с колонками (пример: 40 сырья, MININGST 2× → 4×8 + 4×2 = 40
  золота за 8 ходов). `BuildMenu::max_rate`: у `TRAINHAL` 2×/4× выключены
  (C++), у мобильного строителя строки 2×/4× скрываются при нехватке склада.
  Тесты: `max_core/tests/production.rs` 22 (+2: `turns_and_consumption` для
  1×/2×/4×, остаток `build_time`, Training Hall, клан SHIPYARD; реальный 2×
  с 40 сырья — 8 ходов/40 золота/`storage=0`), unit `buildmenu` 10 (+5:
  `(4,12)/(2,24)/(1,36)`, `TRAINHAL` 1×, CrimsonPath 40→30, `Cost`-строка, цифры активного селектора завода).
  QA `production.qa`/golden `production/*` — 0 дрейфа (игровые кадры не
  изменились; пересняты только `buildmenu*.png`).

- **Строитель въезжает в центр ленты под своим углом: сначала доворот, потом
  скольжение по `DIRECTION_OFFSETS[angle]`.** В C++
  (`UnitInfo::PositionInTape`, `unitinfo.cpp:4414-4482`) цикл такой:
  `if (moved || !Paths_UpdateAngle(this, target_angle))` — пока строитель не
  довернулся к `GetTargetUnitAngle` (`unitinfo.cpp:4147`), `moved` **не растёт**
  и `OffsetDrawZones` не вызывается; затем четыре тика спрайт едет на
  `DIRECTION_OFFSETS[target_angle] * 8` (32 px — в центр 2×2), а на завершении
  C++ доворачивает юнита ещё на шаг (`(angle + 1) & 0x07`) под рабочую позу.
  В порте доворота не было: спрайт ехал по диагонали, глядя в третью сторону
  (обычно на север), из-за чего движение читалось как прямой/вертикальный сдвиг.
  Теперь `max_core::unit_order::get_target_unit_angle` портирует C++
  (SE/NE/SW/NW по клетке ленты; для `LEAVE` — к `target_grid`),
  `position_in_tape` сначала крутит на месте и только после совпадения угла
  наращивает `moved`, а `max_app::buildsite::tape_center_offset` берёт
  направление из **угла юнита** (`DIRECTION_OFFSETS[unit.angle]`), а не из
  фиксированной диагонали; на `BUILD_IN_PROGRESS` — угол въезда
  (`(angle - 1) & 7`, т.к. C++ довернул на шаг). Тесты: unit `buildsite` 9 (+3:
  направление = угол, включая (1,0)*8 для взгляда на восток; четыре угла 2×2 →
  SE/NE/SW/NW и центр ленты; тик поворота + четыре шага ровно по 8 px и доворот
  `NW→N`), кадровый `tests/build_site.rs::
  builder_turns_to_the_tape_and_slides_diagonally_by_eight_pixels_per_tick`
  (bbox спрайта смещается ровно (−8,−8) за тик). QA `production.qa`: новые
  кадры `build-slide-turn`/`build-slide-1..4` + `assert angle=7`, `angle=0`,
  `build-center=no/yes`; golden `production/*` — 0 дрейфа (игровые кадры
  `build-ordered`/`build-site*`/`build-work-*` не изменились).

- **`BuildMenu`: иконки `I_*` в строках списка и портрет выбранного здания
  (`P_*`).** В C++ строки рисует `ReportStats_DrawListItem` →
  `ReportStats_DrawListItemIcon` (иконка `I_*` с горячей точкой в (16,16)
  строки; здания — масштаб 1:4, `divisor = 2`; для
  `TURRET_SPRITE`/`SPINNING_TURRET` — второй кадр со сдвигом
  `angle_offsets[2] >> divisor`; перекраска командных слотов
  `color_index_table`), имя — `Text_TextBox(ulx + 35, uly, 80, 32)`, ходы —
  `ReportStats_DrawNumber` справа (483 + 135 + 20 = 618; `0xA2`, красные, если
  склада строителя не хватает), выделение — белые уголки
  `GameManager_DrawUnitSelector`; портрет — `AbstractBuildMenu::Draw`
  (`buildmenu.cpp:509-529`): `GetPortrait()` big image `P_*` 300×240 в (11,13),
  палитра окна, **без** обрезки 280×240 (в `CargoMenu` своё окно). В порте
  рисовался спрайт **строителя** 2× и не было иконок. Теперь `buildmenu`
  переиспользует `mobilize::{draw_icon, team_lut, center_y, draw_text_right}`
  (`CargoMenu`-паттерн), `CargoMenuAssets::portrait` (ленивый кэш `P_*`), рисует
  иконку/имя/ходы/уголки в строках списка и очереди завода, а портрет — для
  выбранной строки (у завода — активного селектора: список или очередь).
  Тесты: `tests/build_menu_render.rs` (4): иконка в каждой видимой строке
  (diff от фона > 20 px), RED/GREEN перекраска иконок, портрет совпадает с
  `P_MINING` попиксельно (>80 %) в окне (11,13) 300×240, смена строки/очереди
  меняет портрет. QA `production.qa`: `buildmenu`/`buildmenu-factory` пересняты
  с иконками и `P_*`; golden — 0 дрейфа.

- **Призрак выбора места и центр ленты доходили до игрока: упакованный Windows
  UI был старым; оба пути `Build` теперь проверяются живым тестом.** Прошлый
  `ui-test-construction.ps1` брал `build/win-msvc-target/release/max-rust.exe`,
  а в `release/max_app-1.1.0-windows-x86_64-ui/` лежал exe, собранный **до**
  `SELECT_SITE`: он сразу ставил площадку в автоматически найденную клетку
  (`find_build_site`), поэтому игрок не видел призрака, площадка не накрывала
  строителя, а `PositionInTape` не центрировал его. Теперь скрипт по умолчанию
  проверяет **упакованный** exe (build-target — fallback), `-OpenVia key|popup`
  прогоняет оба пути открытия `BuildMenu` (клавиша `B` и пункт `Build` попапа —
  оба идут через один `open_build_menu`), в трассу добавлено `site=open@x,y
  cursor=x,y valid=…`. Живой тест проверяет: призрак нарисован, до LMB в мире нет
  `LRGTAPE`/`LRGCONES`, якорь 2×2 накрывает строителя, после LMB площадка его
  накрывает, а bbox-проверка пикселей кадра даёт центр спрайта строителя в центре
  жёлто-чёрной ленты (допуск 10 px; прогоны key/popup — ≤1.5 px). В коде
  инвариант закреплён: `BuildSiteSelect::is_valid_at` отвергает якорь, не
  накрывающий строителя, а `tape_center_offset` не сдвигает спрайт к ленте,
  которая его не накрывает. Тесты: `buildsite` unit 6 (+2), `tests/build_site.rs`
  7 (+2: попап-путь и призрак после обоих путей). Упакованный Windows UI
  пересобран; QA `production.qa` PASS, golden `production/` — 0 дрейфа. Живые
  кадры: `build/ui-test-construction/` (key) и
  `build/ui-test-construction-popup/` (popup).

- **Строитель больше не «застревает»: `stop` отменяет стройку, `start` её
  продолжает (TC-0630).** В C++ пункт `stop` попапа строителя (`_(bf85)`,
  позиция `7`) — это `UnitsManager_Popup_OnClick_BuildStop` →
  `GameManager_SelectBuildSite` (`game_manager.cpp:3529`): `ORDER_BUILD` +
  `ORDER_STATE_BUILD_CANCEL`, затем `UnitInfo::CancelBuilding` (у мобильного
  строителя очищает `build_list`/`build_time` и возвращает `ORDER_AWAIT`).
  У нас пункт был **серым** («`GameManager_SelectBuildSite` не портирован»),
  поэтому инженер/конструктор, начав стройку, оставался с попапом `stop`+`Done`
  без возможности что-либо строить: `Build` в C++ во время `ORDER_BUILD`
  заменяется на `stop` (`UnitsManager_Popup_InitBuilders:716`).
  Теперь `stop` включён и отменяет постройку (`max_app::App::
  activate_unit_menu_item` → P1 `ORDER_BUILD`/`BUILD_CANCEL`), а у
  остановленного завода включён `start` — `UnitInfo::BuildOrder`
  (`ORDER_BUILD`/`INIT`, очередь и `build_time` сохранены остановкой).
  Тесты: 2 unit (`unitmenu::tests::{builder_mid_build_offers_enabled_stop_
  instead_of_build, halted_factory_offers_enabled_start}`), кадровый
  `tests/unit_menu.rs::builder_stop_cancels_the_build_and_build_returns`,
  QA `production.qa` (заказ → `stop` → `assert unit-order=await` → `Build`
  снова в меню → повторный заказ → постройка доходит до конца; golden
  `production/*` — 0 дрейфа).

### Добавлено

- **Режим выбора места стройки (`ORDER_STATE_SELECT_SITE`) и строитель в центре
  ленты (`PositionInTape`).** Раньше заказ здания у мобильного строителя сразу
  ставил площадку в автоматически найденную клетку (`find_build_site`, радиус 8),
  а конструктор оставался на своей клетке — игрок не мог выбрать место, как в
  оригинале. Теперь после `Done` в `BuildMenu` для здания (`MobileBuildMenu::Build`,
  `buildmenu.cpp:921`: `(base_unit.GetFlags() & BUILDING)`) заказ **не уходит**:
  `UnitsManager_SetNewOrderInt(unit, ORDER_BUILD, ORDER_STATE_SELECT_SITE)` и
  призрак-лента `GameManager_TempTape` (`LRGTAPE`/`SMLTAPE`) двигаются за
  курсором — `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5829`)
  привязывает 2×2 к строителю и возвращает `CURSOR_MAP2`/`CURSOR_UNIT_NO_GO`
  по проходимости (`UnitsManager_MoveUnitAndParent`/`IsAccessible`) и попаданию
  курсора в 2×2. LMB — `GameManager_PathBuild` (`game_manager.cpp:4566`):
  `move_to = TempTape->grid`, приказ `ORDER_BUILD`/`INIT` через пакет 08/P4,
  площадка (`LRGTAPE`+`LRGCONES`+`LRGSLAB`) встаёт в выбранную клетку на
  следующем тике (`UnitInfo::StartBuilding`). RMB/Esc (и LMB по невалидной
  клетке, как `ManagePlayerAction`) отменяют выбор без заказа. После
  `StartBuilding` конструктор проходит `ORDER_AWAIT_TAPE_POSITIONING`:
  `UnitInfo::PositionInTape` (`unitinfo.cpp:4414`) четыре тика двигает спрайт на
  8 px (`OffsetDrawZones`) — 32 px, половина клетки, ровно в центр 2×2;
  сетка строителя не меняется. Ядро: `production::is_site_accessible[_ignoring]`,
  `initial_build_site`, `site_anchor_for_cursor`, `site_covers`,
  `access::is_accessible_ignoring` (C++ временно вынимает строителя из хэша);
  UI: `max_app::buildsite` (режим, призрак, `tape_center_offset`), курсор
  `CursorContext::BuildSite`, `BuildMenu::command_at`/`build_order_command`.
  Завод — как раньше, сразу в очередь. Тесты: unit `buildsite` (4: якорь/валидность/
  смещение центра), `max_app/tests/build_site.rs` (5: курсор+валидность, призрак в
  кадре, LMB ставит площадку в указанной клетке и строитель работает в центре,
  LMB по невалидной клетке отменяет, детерминизм digest),
  `unit_menu::builder_stop_cancels_the_build_and_build_returns`
  (заказ через подтверждение клетки). QA `production.qa` продлён: `build done` →
  `assert build-site=open/valid` → `build cursor 30 21` (`invalid`) → `key ESC`
  (отмена) → `build place 28 20` → `assert site-pos=28,20` → … →
  `assert build-center=yes`; новые проверки `build-site=…`, `build-center`,
  `site-pos=X,Y` и команды `build place X Y|near`, `build cursor X Y`.
  `research.qa`/`research-help.qa` обновлены (`build place near`). Golden
  `production/` переснят (10 кадров: `build-site-select`, `build-site-invalid`,
  `build-ordered` + обновлённые `build-site*`/`build-work-*`), `research/` и
  `research-help/` пересняты из-за нового места постройки — 0 дрейфа.
  Живой Windows-прогон — `build/scripts/ui-test-construction.ps1` (призрак,
  выбор клетки, строитель в центре).

- **Анимация стройки: кадры конструктора и «вырастание» постройки.** C++
  `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856`) во время
  `ORDER_BUILD`/`ORDER_STATE_BUILD_IN_PROGRESS` переключает кадр конструктора
  на каждом втором тике: `DrawSpriteFrame(image_index + 8)`, а при
  `image_index + 8 >= 40` — `image_index - 16`, то есть цикл
  `+16 → +24 → +32 → +16`; инженер держит постоянную рабочую позу `+16`
  (`UnitInfo::StartBuilding`, `unitinfo.cpp:2789`). Раньше порт рисовал только
  постоянную позу. Теперь `max_app::render::unit_frame_index_for_animation` +
  `App::build_animation_step` листают кадры по **реальному `dt`**
  (`BUILDER_ANIM_FRAME_SECONDS = 100 мс`; C++ переключает каждые 2 тика), а
  `dt = 0` (QA/headless) даёт детерминированный кадр начала стройки `+16`.
  «Вырастание» (`UnitsManager_ProcessOrderAwaitScaling` +
  `UnitInfo::RenderSprite`/`RenderShadow`): масштаб `2^-scaler_adjust`
  (1/16 … 1) берётся из симуляционного `scaler_adjust` и доводится до полного
  по `dt` за `GROWTH_ANIM_SECONDS = 1 с`; `dt = 0` — сразу полный размер.
  Оба визуальных часа применяет `App::draw_units` (спрайт, тень и башня
  масштабируются одним уменьшенным `zoom`, как делитель
  `Gfx_DecodeSpriteSetup`), а `IsRelevant` теперь повторён полностью:
  `GROUND_COVER` не рисуется на `zoom < 8`. Тесты:
  `max_app/tests/construction_animation.rs` (3: два кадра с `dt > 0`
  различаются кадром строителя, `dt = 0` — поза `+16`; плита растёт от 1/16
  до полной; юнит без `scaler_adjust` не масштабируется), unit-тесты
  `render.rs` (цикл `+16→+24→+32` и `scaler_scale`), per-decal проверка
  пикселей в `tests/construction_site.rs`. QA `production.qa`:
  `assert site=drawn` (лента/конусы/плита дают свои пиксели) + `anim 60`/
  `anim 1000` (`shot build-site-grow`/`build-site-grown`) и
  `tick 6`/`anim 150` (`shot build-work-a`/`build-work-b`); golden
  `production/` — 7 кадров, 0 дрейфа. Живой Windows-прогон
  `build/scripts/ui-test-construction.ps1` пишет `03-construction-site` и
  `04-construction-grown` (кадры различаются).

- **Стройплощадка на карте (`PrepareConstructionSite` /
  `DeployConstructionSiteUtilities`).** Раньше после заказа стройки на карте не
  было ничего, а в конце ходов появлялось сразу готовое здание. C++ ставит
  площадку из **настоящих юнитов-декалей** (`DeployConstructionSiteUtilities`,
  `unitinfo.cpp:3802`, и `PrepareConstructionSite`, `unitinfo.cpp:4122`):
  конусы `LRGCONES`/`SMLCONES` и лента `LRGTAPE`/`SMLTAPE` — в клетке заказа
  (угол 0 на земле; на воде/побережье конусы не ставятся, лента — угол 1),
  лента с родителем-строителем; если у типа `REQUIRES_SLAB` — плита
  `LRGSLAB`/`SMLSLAB` с кадром `Randomizer_Generate(image_count)` (5/1),
  `scaler_adjust = 4` и `ScaleUnit(EXPAND)`. Порт:
  `max_core::production::{deploy_construction_site, construction_utility,
  construction_utility_types, construction_slab_type}` (идемпотентно: заказ
  ставит площадку сразу, `start_building` повторно её не дублирует и не жжёт
  RNG); `ProductionManager::spawn_new_unit` по завершении повторяет
  `SpawnNewUnit` + `UnitsManager_BuildingReady`: лента/конусы удаляются, плита
  остаётся фундаментом, здание встаёт в ту же клетку и «вырастает». Рендер
  рабочих кадров — `max_app::render::unit_frame_index`: во время
  `ORDER_BUILD`/`BUILD_IN_PROGRESS` у `ENGINEER`/`CONSTRCT` кадр
  `image_base + angle + 16` (C++ `DrawSpriteFrame(image_index + 16)`), после
  достройки — снова базовый. Клик по готовому зданию больше не забирает плита
  (`hud::unit_at_screen` предпочитает не-`GROUND_COVER`, как
  `Access_GetTeamUnit`). Тесты: `max_core/tests/production.rs` (6: большая/
  малая площадка, вода без конусов, уборка при завершении и отмене,
  детерминизм/снапшот), `max_app/tests/building_frames.rs` (+2: кадр `+16`
  и спрайты всех декалей), `max_app/tests/construction_site.rs` (площадка
  видна в кадре до/после стройки), QA `production.qa`
  (`assert site=large|none`, `shot build-site`, golden
  `golden/production/build-site.png`), `research.qa` golden переснят (у
  Research Center теперь плита-фундамент, как в C++),
  `research-help/research-plan.png` — тоже (здание на плите), живой
  Windows-прогон `build/scripts/ui-test-construction.ps1` (скриншоты
  `build/ui-test-construction/`, `03-construction-site.png` — площадка в окне).
  Полный docker-гейт — зелёный, golden — 0 дрейфа.

- **Распределение добычи шахты (`Allocate`, `AllocMenu`, TC-0627–0629).**
  Пункт `allocate` меню команд `MININGST` больше не серый: `max_app::allocmenu`
  — порт `allocmenu.cpp` 1:1. Окно `ALLOCFRM` 640×480 (`Window(ALLOCFRM,
  GetDialogWindowCenterMode())`, `win_move` прижимает его к `(0,0)`), заголовок
  `Allocation Menu` (`_(c771)`, `FONT5`, `COLOR_GREEN`), три секции
  `Raw`/`Fuel`/`Gold` с подписями `Usage`/`Reserve` (`_(8481)`, `_(1e19)`,
  `_(5ae6)`, `_(a80f)`, `_(5432)`, `_(04dd)`, `_(dff1)`, `_(e187)`, `_(89a3)`),
  по три полосы 240×30 на секцию: добыча (`BARRAW`/`BARFUEL`/`BARGOLD` +
  «лента» `BARTAPE`), потребление (`%i (%+i / turn)`), запас комплекса;
  подпись — `FONT1`, цвет `0xFF`, обводка `0x00`, подложка перекрашена
  `Color_GenerateIntensityTable1(0, 0, 25, 1, 1)`. Стрелки `LFARO_OF`/
  `RTARO_OF` — `±1` единица (`case 1001/1002/1004/1005/1007/1008`), клик по
  полосе — абсолютный уровень `(capacity·(x−174)+120)/240` (`OnClick*Bar`),
  `?` → `ALLOCATE_SETUP`, `Done`/`Esc`/`KP_Enter` — закрыть (отдельного
  `Cancel` в C++ нет: значения применяются в момент клика). Клавиатура
  Rust-клиента (задокументированное отличие): `↑/↓`/`Tab` — секция, `←/→` и
  `−/+` — изменение, `H`/`?` — помощь. Диалог модален (`GameManager_
  DisableMainMenu`), полноэкранный `ALLOCFRM` не перекрывается строкой статуса.
  Значение — **не** поле `TeamUnits`, а три счётчика каждого `MININGST`
  (`raw_mining`/`fuel_mining`/`gold_mining`, сумма ≤ 16, `total_mining`) — как
  в C++ (`unitinfo.hpp:434-440`); они уже в снапшоте/CRC и в V70-сейве.
  `Done`/`Esc` ставят каждой шахте комплекса `ORDER_NEW_ALLOCATE` — пакет **08**,
  процессор **P5** (`payload P5` в `docs/architecture/protocol.md`), как
  `Remote_SendNetPacket_11` (`remote.cpp:2195`).
  Экономика: `max_core::alloc` — `AllocMenu_Optimize`/`AllocMenu_
  ReduceProduction`/`Balance` (`Allocator` с маской `cargo_material_type`),
  `Complex::GetCargoMining`/`GetCargoMinable`/`GetCargoInfo`,
  `Cargo_GetInventory`/`Cargo_GetCargoCapacity`, `Access_UpdateResourcesTotal`;
  `GameState::manage_economy` теперь вызывает `update_complex_resources`
  (раньше был TODO и `Complex::material` оставался нулём), поэтому доля шахты
  становится добычей комплекса за ход (`raw += raw_mining` и т.д. — формулы
  `cargo.cpp:105-126`, с коэффициентом сложности `×5/4`/`×6/4`).
  `GameState::spawn_unit` — `UnitInfo::UnitInfo` (`unitinfo.cpp:316`): здание
  сразу получает свой комплекс (`AttachComplex(CreateComplex(team))`),
  `destroy_unit` — `DetachComplex`/`Complex::Shrink`; без этого у шахты не было
  `GetComplex()` и `allocate` не открывался. Тесты: 9 `max_core::alloc`,
  12 `max_app::allocmenu`, 9 кадровых `tests/alloc_menu.rs`, QA
  `allocate.qa` (128 кадров) + golden `golden/allocate` (0 дрейфа; остальные
  эталоны — тоже 0 дрейфа), живой Windows-прогон
  `build/scripts/ui-test-allocate.ps1` (скриншоты `build/ui-test-allocate/`).
  Осознанное расхождение: коннекторы зданий не
  моделируются, поэтому комплекс = одно здание.

- **Меню команд выделенного юнита (`GameManager_PopupButtons`, TC-0622–0626).**
  `max_app::unitmenu` — порт `GameManager_InitPopupButtons`/`DeinitPopupButtons`/
  `ProcessPopupMenuInput` (`game_manager.cpp:3568/3664/2607`) и
  `UnitsManager_Popup_Init*` (`units_manager.cpp:543-1352`): попап открывается
  **повторным кликом** по уже выделенному своему юниту (`GameManager_ClickUnit`,
  `unit1 == unit2`; первый клик только выделяет), закрывается `Esc`, повторным
  кликом по юниту, кликом вне меню и правой кнопкой; пока меню открыто, карта
  приказов не получает (как `GameManager_DisableMainMenu` в C++). Раскладка —
  по C++: вертикальный список кнопок `UNTBTN_U`/`UNTBTN_D` (42×21) с шагом
  `height + 3`, окно `image.width + 6` × `height * count + (count+1) * 3`,
  якорь `ulx = unit->x + 32` (правый край клетки), `uly = unit->y - height/2`
  (центр клетки), сдвиг влево на `width + Gfx_ZoomLevel` у правого края карты;
  подпись — `GNW_TEXT_FONT_2` цветом `COLOR_GREEN` по центру кнопки, нажатый
  пункт рисуется `UNTBTN_D`; порядок пунктов — как `UnitsManager_RegisterButton`
  (устойчивая сортировка по символу позиции, `'0'` → `':'`). Состав пунктов —
  таблица `UnitInfo::Init` (`unitinfo.cpp:454-582`): танк
  `attack/enter/sentry/Done` (+ `x-fer` при грузе, `stop` при маршруте),
  строитель `Build`, завод
  `Build/start/stop`, транспорт `activate/load`, шахта `allocate/stop`, генератор
  `start/stop`, исследовательский центр `resrch/stop`, золотой завод
  `start|stop + buy upg`, и т.д. Клавиатура: символ позиции или первая буква
  подписи (`GameManager_ProcessPopupMenuInput`); стрелки/`Enter` в C++ попапе не
  работают — здесь тоже. Пункты вызывают существующие обработчики: `Build` →
  `BuildMenu`, `buy upg` → `UpgradeMenu`, `resrch` → `ResearchMenu`, `start/stop`
  → `build_power_order`, `sentry` → `ORDER_SENTRY`/`ORDER_AWAIT` (пакет 08, P1),
  `stop` → `ORDER_AWAIT`+`CLEAR_PATH` (P3), `upgrade` → `ORDER_UPGRADE` (P2),
  `place`/`remove` мины → `ORDER_LAY_MINE` (P1), `enter` → `enter_mode` +
  `ORDER_MOVE_TO_UNIT` (P3), `repair`/`reload` → курсорный режим (`cursor` 4/6) +
  `ORDER_REPAIR`/`ORDER_RELOAD` с родителем (P2), `Auto` → `auto_survey`,
  `Done` → `UnitsManager_PerformAction` (в объёме стека: `Await`+`STATIONARY` →
  `READY_TO_EXECUTE_ORDER`, снятие выделения). Честно **отключены** (серый пункт,
  причина в `UnitMenuItem::note` и `docs/analysis/visual-1to1-status.md`):
  `x-fer` (`TransferMenu_Menu`/`ProcessOrderTransfer` — TODO P3, хотя приказ
  `Transfer` в протоколе есть), `attack` (`targeting_mode`; атака — кликом по
  врагу), `load` (погрузка в транспорт: `Land` — P1 и не несёт родителя),
  `activate` (`RepairShopMenu`), `start` завода
  (`BuildOrder`), `stop` стройки (`GameManager_SelectBuildSite`), `remove` здания
  (`UnitsManager_SelfDestructMenu`), бульдозер (`Access_GetRemovableRubble`),
  `Upg. All` (комплексы/материалы), мастер-строитель (проверка площадки), а также
  `disable`/`steal` диверсанта. Тесты: 13 unit (`unitmenu`), 12 кадровых
  (`tests/unit_menu.rs`), QA `unit-menu.qa` (+golden `unit-menu-*`). Расхождение
  с C++ (осознанное): у стационарного юнита меню не открывается уже при
  выделении — иначе менялись бы существующие эталоны (`mine-selected`).
  Пункт `allocate` (`AllocMenu`) с этого среза **включён** — см. запись выше.

- **Панель статистики по типу юнита (`WINDOW_STAT_WINDOW`, TC-0601).**
  `max_app::panels::rows_for` — порт `GameManager_UpdateInfoDisplay` +
  `GameManager_DrawInfoDisplayRow`/`DrawInfoDisplayType1/2/3` из
  `game_manager.cpp`: четыре строки выбираются по атрибутам и типу, а не
  фиксированным набором `Hits/Ammo/Speed/Shots`. Мобильные наземные/морские/
  воздушные: `Hits`, `Ammo` (при `ATTRIB_ATTACK != 0`), `Speed`, `Shots`;
  турели: `Hits/Ammo/Shots` (Speed = 0 — строки нет); шахта и склады:
  `Hits`, `Cargo` (`ATTRIB_STORAGE`), `Total` (запас комплекса, `_(d28b)`);
  заводы: `Hits`, `Usage`, `Total` рабочих (`DrawInfoDisplayType1`); электро-
  станции/генераторы: `Hits`, `Power`, `Total`, `Usage` (`Type2`); жилой блок:
  `Hits`, `Teams`, `Total`; теплица игрока: `points`, `Total` (`Type3`).
  Строку с нулевой базой или нулевым factor C++ не рисует вовсе
  (`if (base_value && factor)`) — поэтому у шахты больше нет `Speed`/`Shots`,
  а строки без данных остаются фоном окна. Цвета порогов сохранены
  (`COLOR_RED` ≤ 1/4, `COLOR_YELLOW` ≤ 1/2, `COLOR_GREEN` иначе), подписи —
  значения `en-US` из `language.json` (`Hits/Ammo/Cargo/Speed/Shots/Usage/
  Power/Teams/Total/points`). `CargoMenu` (`UnitStats_DrawStats`) и нижняя
  панель юнита уже были условными — добавлены регресс-тесты «у здания нет
  Speed». Тесты: 10 unit в `panels` (шахта/танк/турель/разведчик/завод/
  электростанция/жилой блок/чужой юнит/цвета + все 93 типа), 2 кадровых
  `tests/stat_panel.rs`, +1 `hud`, +1 `mobilize`; QA `visual-screens.qa` и
  `move-input.qa` снимают `mine-selected` (шахта без Speed), golden обновлён.

- **MVE-интро (`Introduction`, `INTROFLC` → `maxint.mve`, TC-0108).**
  `max_assets::mve` — декодер видео Interplay MVE: контейнер (подпись,
  записи `u16 size/type`, опкоды), `CREATE_TIMER` (15 FPS), `SET_PALETTE`
  (6-бит → 8-бит), `SET_DECODING_MAP` (ниббл на блок 8×8) и `VIDEO_DATA_11`;
  видеокодек — точный порт `nfPkDecomp` (копии кадров/векторы движения/
  шаблоны 2–64 цветов, таблицы `nf_pk_lut_4/5/6`, ветка `0xA` построчно).
  `maxint.mve`: 432×320, **4391 кадр**, 13 обновлений палитры; FNV-1a кадров
  0/50/100 совпадает с C-обвязкой над `nfPkDecomp` (`74CDBDC5`/`22EB1E36`/
  `158D8E0D`), полный проход 4391 кадра — в тесте. `max_app::intro` декодирует
  кадр 0 при входе (детерминированный `dt = 0`), крутит 15 FPS, масштабирует
  432×320 → 640×474 с letterbox (`cscale`), Esc/клик/конец видео — в меню.
  Аудио и субтитры `INTROSUB` не портированы (честное ограничение).
  QA: `shot intro` (dt = 0) + `tick 210` → `shot intro-logo` (логотип
  Interplay), эталоны `golden/intro.png`/`golden/intro-logo.png`.
- **Второй кадр иконок турелей — проверено и покрыто тестами.** `angle_offsets`
  лежат не в `.SPW` (это WAV-звуки), а в 24-байтном ресурсе `D_*`
  (`Unit::GetFrameInfo`: 8 счётчиков + 8 пар `(dx,dy)`); кадр башни — кадр 1
  ресурса `I_*`. `CargoMenu`/`UpgradeMenu` рисуют корпус + башню со сдвигом
  `angle_offsets[2] >> divisor`; тест расширен на все **11** турельных типов
  (кадр 1 есть, сдвиг совпадает с `D_*`) плюс пиксельная проверка «башня
  танка в списке».
- **Экран исследований `ResearchMenu` (`RSRCHPIC`) и Help-экраны
  (`HELPFRAM`).** `max_app::researchmenu` — порт `researchmenu.cpp`: окно
  `RSRCHPIC` 360×331 в (223,76), заголовок `Research Topics`, подписи
  `Labs/Topics/Turns`, 8 тем (Attack/Shots/Range/Armor/Hits/Speed/Scan/Cost) с
  иконками `I_*`, счётчики центров, «ходов до завершения»
  (`(centers+turns−1)/centers`), `+N%`, стрелки `UPGLFT/UPGRGT` (disabled
  `_X`), слайдер `PRFSLIDE` с кликом по слоту, `Cancel/Help/Done`
  (`DPTBAY/DPTHP`). Распределение — как C++: `free_capacity` стартует с 0,
  уменьшение темы освобождает центры, `Done` (`set_research_allocation`)
  переназначает/выключает центры; `ResearchMenu_NewTurn` в `finish_turn` уже
  тикает те же `research_topics`. Экран открывается клавишей `R` для
  выделенного включённого Research Center (popup «Research» в C++; попап не
  портирован). `max_app::helpmenu` — окно `HELPFRAM` 301×231 ((169,124) в
  меню, (253,126) в игре), шрифт 5 + обводка, 16 строк на страницу,
  `PageUp/PageDown` + `BLDUP__/BLDDWN`, `Done`/`Enter`/`Esc`, `Keys` у
  `GAME_SCREEN_SETUP`. Тексты — генератор `tools/gen_help_data.py` →
  `data/help.json` (32 секции, 661 запись из `help.json` + `language.json`).
  Подключены `?` на Options/Planet/Players/Clan, `Help` CargoMenu/
  ResearchMenu/UpgradeMenu, панель `HELP_OF` и `?` в игре. Тесты:
  `researchmenu` 10, `helpmenu` 9, `ui` +3, `qa_driver` +1.
- **QA `research-help.qa`** — окна `OPTIONS_SETUP`/`PLANET_SETUP`/
  `NEW_GAME_SETUP`/`CARGO_SETUP` (прокрутка, `Done`), `ResearchMenu`,
  `RESEARCH_SETUP`/`GAME_SCREEN_SETUP`+`Keys`, распределение Attack→Shots и
  проверка через 16 ходов (Shots=1, Attack=0); новые команды
  `research open/select/row/done/cancel`, `help SECTION/page/up/down/keys/
  done/close` и проверки `research-menu`, `research-alloc`, `help`,
  `help-section`, `help-row`; 9 golden-кадров `golden/research-help/` в
  `ci.sh`/GitHub CI.

- **FLIC-анимация портрета юнита (`WINDOW_CORNER_FLIC`, TC-0919).**
  `max_assets::flic` — декодер Autodesk FLIC по публичной спеке (заголовок
  128 байт, кадры `0xF1FA`, чанки `COLOR_256`/`DELTA_FLC`(SS2)/`COLOR_64`/
  `BLACK`/`BYTE_RUN`/`LITERAL`, `PSTAMP` пропускается, `DELTA_FLI` для полноты;
  ошибки — значениями, без паник). В `MAX.RES` 71 запись `F_*` — не картинки, а
  ASCIIZ-имена `*.FLC` рядом с архивом; все 71 файл (1115 кадров, 36
  анимированных × 30 кадров, 35 статичных; 128×128, `SAVELOAD.FLC` 128×256)
  разобраны и сверены с C++ (`tools/flic_vectors`): 6 полных кадров бит-в-бит +
  FNV-1a-32 **каждого** кадра. `max_app::portrait`: маппинг тип → `F_*` из
  `SC_UNITS` (`flics_animation`, 71 тип), анимация 15 FPS
  (`GameManager_AdvanceFlic`), зацикливание (`animate = 2`), кадр 1:1 палитрой
  окна (`FRAMEPIC`, `load_flic_palette = 0` как в C++), `dt = 0` в headless/QA —
  кадр 0 (детерминизм golden), снятие выделения — чёрное окно, fallback —
  спрайт ×2 (22 типа без `F_*` и `MASTER` без `F_MASTER`).
  Тесты: `max_assets` (`flic` 7, `flic_assets` 3 — все 71 записи MAX.RES и
  сверка с `manifest.csv`, `flic_vectors` 2), `max_app/tests/portrait.rs` (6).

- **Экраны `Introduction` и `Credits` главного меню (TC-0108).**
  `Credits` — построчный порт `menu_credits_menu_loop`: фон `CREDITS` 640×480 со
  своей палитрой, область 450×230 в (95,125), прокрутка 1 px за шаг по 10 мс
  (`1000/speed`), новая строка каждые 20 шагов, `-`/`+` меняют скорость на 9,
  Esc/любая клавиша — в меню, автозакрытие по концу прокрутки; таблица — 171
  строка из `menu.cpp` (65 заголовков + 106 имён), тексты заголовков из
  `language.json` (`tools/gen_credits_data.py` → `data/credits.json`).
  `Introduction` — MVE-видео `INTROFLC` → `maxint.mve` через
  `max_assets::mve` (см. выше); если файла нет — заглушка с фоном `MLOGO`
  640×480 и текстом, Esc — выход.

### Изменено

- Кнопки `Help`/`?` больше не пишут «экран не портирован»: открывают окно
  `HELPFRAM` (`Screen::Help` для меню/мобилизации, `App::help_screen` в игре);
  `R` в партии открывает `ResearchMenu`, `?` — `GAME_SCREEN_SETUP`; в
  `ResearchMenu`/`UpgradeMenu` `Help` открывает соответствующие секции.
- Портрет `CORNER_FLIC` рисует FLC-кадр вместо статичного спрайта ×2; экраны
  меню: новые пункты в фокусе клавиатуры (`Introduction`, `Credits`), горячие
  клавиши `I`/`R`, в QA — команды `menu intro`/`menu credits` и экраны
  `assert screen=Introduction|Credits`.
- Перебазированы golden-кадры только там, где виден портрет выделенного юнита:
  `golden/portrait.png`, `golden/intro.png`, `golden/credits.png` (новые),
  `golden/move-input/{game-selected,move-preview,game-after}.png`,
  `golden/upgrade/upgrade-done.png`; остальные эталоны — 0 дрейфа.
- Новые golden-кадры P8 — `golden/research-help/` (9 PNG: Help-окна,
  `research-menu`, `research-plan`); существующие наборы не переснимались
  (0 дрейфа).

### Исправлено

- **Угол юнита всегда по ходу движения (TC-0933, 2026-11-17).**
  Спрайт подвижного юнита мог смотреть на 45°/90° в сторону от фактического
  движения: угол на клетке поворота менялся позже C++-оракула, а кадр читал
  симуляционный `angle`, хотя визуальная очередь ещё доигрывала предыдущий
  отрезок.
  - **Оракул.** `Paths_GetAngle` (`paths.cpp:219`, `DIRECTION_OFFSETS` из
    `path.hpp:51` — N/NE/E/SE/S/SW/W/NW → 0..7), `Paths_UpdateAngle`
    (`paths.cpp:258`, один шаг 45° за вызов), `GroundPath::Execute`
    (`groundpath.cpp:236/260`: сначала доворот, движение только при совпадении),
    `GroundPath::UpdateUnitAngle` (`groundpath.cpp:430`: на завершении клетки —
    один шаг к следующему шагу), `UnitInfo::UpdateAngle`
    (`unitinfo.cpp:1161`: башня поворачивается вместе с корпусом),
    `UnitInfo::Move` (`unitinfo.cpp:1781`), `RenderWithConnectors`
    (`unitinfo.cpp:4838`).
  - **Порт.** `max_core::unit_order::paths_update_angle` дополнен связкой
    `turret_angle` (`TURRET_SPRITE`); добавлен
    `ground_path_update_unit_angle`; `unit_move` вызывает его после списания
    `PathStep` — как `path->UpdateUnitAngle(this)`. Порядок «сначала поворот,
    потом шаг» сохранён, после остановки `angle` = последнему направлению.
  - **Кадр.** Визуальная очередь (`App::update_visual_units`) хранит
    `VisualStep.angle` — направление отрезка из фактического смещения клеток
    (`Paths_GetAngle`); `App::visual_angle` отдаёт угол текущего отрезка (или
    `unit.angle`, когда очередь пуста). `render::draw_unit_sprites_with_angle` /
    `unit_frame_index_for_angle` рисуют корпус `image_base + angle` и башню со
    сдвигом `angle_offsets[angle]`; HUD-портрет/тесты используют `unit.angle`.
  - **Детерминизм.** `angle` входит в `GameState::snapshot_bytes` (CRC
    состояния), но не в сетевой CRC пакета 45 — как в C++
    (`GameManager_GetUnitListChecksum`); два одинаковых прогона дают побайтово
    одинаковые снапшоты на каждом тике.
  - **Тесты.** `max_core::unit_order::tests` — 8 направлений, L-поворот,
    граница клетки, башня, детерминизм; `max_app` —
    `visual_angle_stays_on_the_segment_being_animated`,
    `explicit_body_angle_selects_the_corresponding_sprite_frame`,
    `frame_index_uses_the_given_body_angle_and_power_on_offset`. QA
    `move-input.qa` — `assert angle=2` (новая команда `assert angle=N`);
    golden `move-input/*` — 0 дрейфа. Windows UI — `ui-test-move2.ps1`
    (`build/ui-test-move2/*.png`), headless — `qa-out/move-input/*` и
    `--example angle_demo` (`qa-out/angle/{01-east-with-sim-ne,02-turn,03-stopped-north}.png`).

- **Видимое движение юнитов, наложение генератора на шахту, кадры питания
  зданий (2026-11-17).**
  - **Движение.** `App::update_visual_units` больше не «догоняет» симуляцию с
    потолком 12 клеток/с и снапом при расхождении > 2.5. Кадр ведёт очередь
    фактического маршрута (`visual_goals`/`visual_paths`) и проходит её
    клетка за клеткой со скоростью оригинала: `UnitInfo::Move` 8 px/тик ×
    ~30 Гц / 64 px ≈ 4 клетки/с. В интерактивном окне юнит с `Move` теперь
    визуально едет после End Turn; `dt = 0` (headless/QA) — снимок по `grid`,
    `snapshot_bytes`/CRC не меняются. `local_update` доигрывает маршрут в
    фазе `Advancing`/при окне перехода хода.
  - **Раскладка.** `deploy.rs` сверен с
    `GameManager_GetStartingPositionPowerGenerator`: шахта `(22,22)`,
    генератор `(21,23)` — **разные клетки** (RNG/порядок `spawn_unit` не
    трогали). Наложение было ошибкой рендера: C++ `UnitInfo::SetPosition` для
    `flags & BUILDING` сдвигает якорь на `+31` px (`MININGST`, `LRGSLAB` —
    2×2). Добавлен `render::unit_anchor_world`; спрайт шахты больше не заходит
    на клетку генератора.
  - **Кадры зданий.** `render::unit_frame_index` выбирает `image_base + 1`
    при `ORDER_POWER_ON` и `image_base` при выключении (C++
    `ProcessOrderPowerOn`/`PowerDown`); `image_base_for` даёт клановый вариант
    шахты `(team_clan - 1) * 2`. Покадровой анимации по времени у зданий в
    оракуле нет — «анимация» включённой шахты/генератора это второй кадр
    состояния (индикатор питания), который раньше не рисовался.
  - **Тесты.** `tests/app.rs::movement_is_visible_between_frames_and_finishes_on_the_target_cell`,
    `tests/local_sp.rs::local_update_with_real_dt_animates_move_orders`,
    `tests/move_input.rs::generator_and_mining_station_are_on_different_cells_without_sprite_overlap`,
    `max_core::deploy::tests::power_generator_starts_next_to_the_mining_station_but_not_on_it`,
    `tests/building_frames.rs` (3). Пересняты golden `move-input/*`,
    `visual-screens/{game,portrait}`, `upgrade/upgrade-done`,
    `research/research-done`, `research-help/*` (help-game/help-game-keys/
    help-research/research-menu/research-plan); `qa_golden_check` — 0 дрейфа.

- **«Юнит не двигается»: камера после высадки вставала не на базу**
  (`max_app/src/lib.rs` — `App::focus_player`, `max_app/src/render.rs` —
  `render_with_chrome`):
  - **Корень.** После высадки камера центрировалась на первом юните
    канонического обхода — а это `GROUND_COVER` (`SMLSLAB`/`LRGSLAB`), клетка
    на одну вниз-влево от шахты. В C++ вид ставит
    `GameManager_GetFirstRelevantUnit` (`game_manager.cpp:3140`):
    `Access_GetFirstMiningStation` → первый активный наземный/морской →
    воздушный → стационарный. Из-за сдвига на клетку клики, рассчитанные «от
    базы», попадали на соседнюю клетку: вместо `Surveyor`'а выделялась
    стационарная шахта (`sel-6` в прогоне `move-debug` — портрет Mining
    Station), RMB отвечал «Unit cannot move (air/stationary)», зелёного
    предпросмотра не было, и ход ничего не двигал. **Юниты при этом рисовались**
    — FOW/`ORDER_IDLE`-фильтр (`UnitInfoGroup::IsRelevant`) ни при чём.
  - **Камера.** `App::focus_player` повторяет `GameManager_GetFirstRelevantUnit`
    1:1: шахта своей команды из `StationaryUnits`, иначе первый не-`IDLE` юнит
    в `MobileLandSea` → `MobileAir` → `Stationary`.
    `Viewport::center_on_grid` ставит центр клетки (`grid*64+32`) в центр
    `MAIN_MAP`, как `GameManager_CenterMainMapView`.
  - **Предпросмотр маршрута в оконном режиме.** `render_with_chrome` рисует
    зелёную линию RMB поверх юнитов в буфере карты: при хроме `render_hud` не
    вызывается (панели приходят из `FRAMEPIC`), поэтому раньше предпросмотр
    был виден только в полноэкранном fallback (0.04 % зелёных пикселей в
    `move-1` против целой линии теперь).
  - **Enter на экране высадки работает** (проверено новым сценарием:
    `key ENTER` → `assert screen=InGame`). «Enter не подтверждает посадку» в
    прогоне `move-debug` — артефакт самого сценария: клик `501 472` промахивался
    мимо `Done` мобилизации на 1 px (`DONE_BUTTON` = 447,452,54×23,
    `contains` — правая граница исключающая), `Enter` уходил в `CargoMenu`
    (`Key::Enter` = Done) и переводил на экран высадки — поэтому
    `game-before`/`sel-1`/`sel-2` были ещё экраном высадки.
  - **Тесты.** `max_app/tests/move_input.rs` (6): камера на шахте (центр кадра);
    `Enter` подтверждает точку (`Transition::StartLocal` + `start_positions`);
    стартовые юниты рисуются спрайтами (`units_drawn >= 7`, `units_skipped = 0`);
    координата сценария `(468,178)` попадает в клетку `Surveyor`'а;
    клик выделяет (`hud.selected`), RMB даёт предпросмотр + «Move: N steps» +
    зелёные пиксели в кадре; после тиков юнит стоит в целевой клетке.
  - **QA.** Новый `max-rust-qa/scripts/move-input.qa` — весь путь **кликами**
    (меню → мобилизация → высадка `Enter` → клик по своему `Surveyor`'у →
    RMB → `End Turn` → позиция изменилась) с ассертами `selected`/`unit-pos`/
    `preview`/`log=Move:`/`log=Cancelled`; добавлен в `ci.sh` (стадия 3/4).
    Отладочные `move-debug.qa`/`tmp-probe.qa` удалены. Golden: новые
    `golden/move-input/*` (5 кадров: старт с юнитами, выделение, предпросмотр,
    ход 2, ход после движения), пересняты `golden/game.png` и
    `golden/research/research-done.png` (камера на базе вместо плиты);
    `qa_golden_check` — 0 дрейфа по всем наборам.
- **Туман войны: чужие юниты больше не видны без разведки**
  (`max_core/src/game.rs`, `max_core/src/combat.rs`, `max_core/src/ai/threat.rs`,
  `max_app/src/render.rs`):
  - **Корень бага.** `GameState::spawn_unit` повторял `UnitInfo::InitStealthStatus`
    неточно: все обычные (не стелс) юниты помечались `visible_to_team = true`
    **всем** командам. Из-за этого на карте и миникарте были видны чужие базы и
    юниты вне FOV. Теперь как в C++ (`unitinfo.cpp:2796`): невидимые типы
    (`LANDMINE`/`SEAMINE`/`COMMANDO`/`SUBMARNE`) скрыты, остальные видны только
    при `GameManager_AllVisible`, а своя команда видит всегда.
  - **Пересчёт после высадки.** Добавлен
    `GameState::update_visibility_status` — порт `Access_UpdateVisibilityStatus`
    (`access.cpp:880`): `InitStealthStatus`, очистка тепловых карт и повторное
    покрытие сканированием каждого юнита. Вызывается после
    `deploy_starting_units*` (как `game_manager.cpp:1965`), поэтому видимость не
    зависит от порядка высадки команд.
  - **Отрисовка.** `App::draw_units` дополнительно требует
    `orders != ORDER_IDLE` (`UnitInfoGroup::IsRelevant`, `unitinfogroup.cpp:59`);
    чужие юниты по-прежнему рисуются только при `is_visible_to_team`
    (`DrawMap_RenderUnits`). Миникарта (`panels::paint_minimap_units`) уже
    фильтровала по видимости — теперь фильтр получает корректные флаги.
  - **Тесты.** `max_core/tests/fog_of_war.rs` (4): спавн виден только своей
    команде; скан открывает врага в FOV и не открывает вне; `AllVisible`;
    стелс-юнит вне FOV скрыт. `render::tests` — чужой юнит не рисуется на
    карте, пока не виден, и появляется после `set_visible_to_team`;
    `panels::tests` — то же для точек миникарты.
  - **QA.** `sp-full`/`hs-2` PASS; в кадрах `game-start`/`hs-green-turn`
    миникарта показывает только свои точки. Golden `game.png`,
    `upgrade-done.png`, `research-done.png` пересняты (0 дрейфа).
- **Живая диагностика «юниты не двигаются» и интерактивная высадка
  (2026-11-17).** Оракулы (`max-cpp-port`) не менялись.
  - **«Генератор на клетке шахты» — ошибка чтения трассы.** `[tick]` печатал
    `unit_type as u8`, а не id: `17@(22,22)` — это `LRGSLAB`, `40@(22,22)` —
    `MININGST`; плита ставится в клетку шахты по C++
    (`UnitsManager_DeployUnit(LRGSLAB, …, mining_station_location)`), а генератор
    `2@(21,23)` — отдельная клетка (`GameManager_GetStartingPositionPowerGenerator`).
    В живом прогоне на клетке высадки ровно `17,40`, генератора там нет.
  - **Интерактивный поток.** После `generate_map` (порт
    `GameManager_PopulateMapWithResources`: `FindValidStartingPosition` +
    `Optimize`) позиция синхронизируется для **всех** `TeamDeployment`, а не
    только для добавленных ИИ: высадка идёт по итоговой
    `TeamMissionSupplies[team].starting_position`, как в C++. Golden не
    изменились (точка из фазы высадки валидна, поэтому сдвига нет).
  - **Трасса `MAX_DEBUG_INPUT`.** Локальный цикл теперь инкрементирует
    `App::ticks` (раньше трасса вечно показывала `tick 0`), `[tick]` печатает
    `grid` и визуальную `vX.Y` каждого юнита + `preview=N`; добавлен
    `[rclick]` с grid/selected/результатом (`move`/`cancel`/`no-path`/…).
    Без переменной окружения ничего не печатается.
  - **Живой тест.** `build/scripts/ui-test-move2.ps1` — `SendInput`
    (`MOUSEEVENTF_*`; union `INPUT` — на смещении 8, как в Win32), проверка
    фокуса перед каждым кликом, `Enter` на высадке, координаты берутся из
    трассы. Проверяет `[in] … RightClick`, `[rclick] result=move`,
    `preview`, зелёные пиксели chrome-кадра (**152 px**) и после End Turn
    `grid=25,21` с визуальным проходом `23,21 → 23.8,21 → 24.7,21 → 25,21`.
  - **Тесты.** `tests/move_input.rs`:
    `interactive_landing_keeps_the_generator_off_the_mining_station_cell`
    (`PreGameFlow` → `TeamDeployment` → разные клетки; на клетке шахты только
    `MININGST` + `LRGSLAB`) и
    `rmb_then_end_turn_moves_the_unit_through_the_path_in_the_local_loop`
    (реальные LMB/RMB → `local_update(dt)` → End Turn кнопкой панели →
    `grid`/`vis` в цели, повторное выделение на новом ходу).
  - **QA.** `move-input.qa` PASS, `golden/move-input/*` — **0 дрейфа**
    (раскладка не менялась).

### Изменено

- **Экран высадки — 1:1 с C++: настоящие тайлы `.WRL`, маркер/круг/маркеры
  строительства, без плоских цветов** (`max_app/src/landing.rs`,
  `max_render/src/terrain.rs`, `max_app/src/pregame.rs`, `max_app/src/menu.rs`):
  - **Тайлы.** Вместо заливки `TileStyle` рисуется `TerrainAtlas` (настоящие
    пиксели `.WRL`) с точной выборкой `Gfx_DecodeMapTile` (`gfx.cpp:125-210`):
    `factor = ((64-1)<<16)/(size-1)+8`, источник `(i*factor)>>16` — на зуме 4
    это `0, 21, 42, 63`, а не `0,16,32,48`. Добавлен
    `TerrainAtlas::render_tile_cpp_into` + `cpp_tile_sample_offsets`; палитра
    окна дополняется цветами `.WRL` для индексов 64…159
    (`UiPalette::with_wrl_palette`, как `World::Load`). `landing::load` читает
    `WRL` один раз (`load_parts`), `PreGameFlow` хранит атлас.
  - **Курсор/выделение.** В обычном выборе C++ `MASTER` ещё не существует:
    `GameManager_SelectSite` запускает интерактивный `GameManager_ProgressTurn`
    до спавна `MASTER` (он нужен только ветке проверки близости), поэтому на
    карте нет ни рамки 4×4, ни `DrawUnitSelector`, ни круга обзора
    `DrawCircle`, ни `BLDMRK*` — это подтверждает кадр C++-порта
    `build/tmp/oracle-MAX0002.png`. Выделение точки — курсор мыши
    (`GameManager_GetWindowCursor`): `CURSOR_UNIT_GO` над валидной клеткой,
    `CURSOR_UNIT_NO_GO` над невалидной, `HANDPTR` вне карты. Он ставится в
    SDL-слое (`main.rs`) и рисуется в кадр для headless QA
    (`Cursor_RenderToBuffer`, как в `screendump.cpp`).
  - **Сообщение.** `MessageManager_DrawMessage` (`_(dfe1)`):
    `"Select starting location."` одной строкой (`Font5`, обводка) в левом
    верхнем углу области карты (`+0,+10`), фон — затемнение пикселей карты
    через палитру (`ResourceManager_DarkeningColorIndexTable` +
    `Color_MapColor`), высота `20 + 10`. Лишняя подсказка и кнопки
    `Cancel`/`Done` убраны (в C++ их нет). Клик по валидной клетке сразу
    подтверждает выбор (`game_manager.cpp:6718-6727`); Esc/Enter сохранены.
  - **Тесты.** `max_app/tests/landing_wrl.rs` — кадр содержит >40 цветов и
    пиксели клеток совпадают с сырым `WRL` (независимо от рендера), курсор
    `UNIT_GO` рисуется в кадре; fallback без `.WRL` не падает.
    `terrain::tests` — формула выборки `Gfx_DecodeMapTile`.
  - **QA/артефакты.** `sp-full`/`hs-2`/`visual-screens` и остальные 10
    сценариев PASS; golden `landing.png` переснят (реальные тайлы вместо
    плоских цветов), `game.png`/`upgrade-done.png`/`research-done.png` — после
    исправления тумана войны; `qa_golden_check` — 0 дрейфа. Пересобраны
    Windows-артефакты `release/max_app-1.1.0-windows-x86_64-ui` (MSVC/SDL3,
    живой кадр — `build/landing-ui.png`) и
    `release/max_app-1.1.0-windows-x86_64` (mingw, headless); `bash ci.sh` —
    CI OK (496 с).

### Добавлено

- **Спрайты всех 93 типов юнитов на карте и в HUD** (`max_app/src/assets.rs`,
  `max_app/src/render.rs`, `max_app/src/panels.rs`, `max_app/src/cargo_unit_data.rs`,
  `max-rust/tools/gen_cargo_unit_data.py`):
  - **Таблица.** `UNIT_SPRITES` расширен с 43 до **93** записей — построчный
    порт `SC_UNITS` (`assets/PATCHES.RES`, `Units::LoadResource`): `sprite`
    (`Unit::GetSprite()`) и `shadow` (`Unit::GetShadow()`), порядок `UnitType`.
    Генератор `gen_cargo_unit_data.py` теперь кладёт в
    `data/cargo_unit_data.json` ещё `sprite`/`shadow`; тест
    `assets::tests::unit_sprite_table_mirrors_sc_units` сверяет таблицу
    построчно с оракулом. Грузятся 92 корпуса и 75 теней (в `SC_UNITS`
    объявлено 77, но `S_MASTER`/`S_SUBMAR` в архивах отсутствуют); у 11 типов с
    `TURRET_SPRITE`/`SPINNING_TURRET` из `D_*` берутся `turret_image_base`,
    `turret_image_count` и `angle_offsets`.
  - **Рендер.** `render::draw_unit_sprites` — единая точка «тень → корпус →
    башня со сдвигом `angle_offsets`» с масштабом `zoom/64`. Её используют
    карта (`App::draw_units`), HUD-портрет (`panels::draw_portrait`, тот же
    путь вместо копии кода), а через `assets.sprite()` — `BuildMenu`/
    `UpgradeMenu`. Маркером команды остаётся только `MASTER`: ресурса
    `MASTER`/`S_MASTER` нет ни в `MAX.RES`, ни в `PATCHES.RES` (C++
    `UnitInfo::RenderSprite` для него тоже ничего не рисует; в каталоге игры
    лежит осиротевший `MASTR17.SPW` без ссылки из архивов). У `SUBMARNE`
    отсутствует объявленная в `SC_UNITS` тень `S_SUBMAR` — подлодка без тени,
    как в оригинале.
  - **Тесты.** Новый `tests/unit_sprites.rs`: у каждого из 93 типов кадр
    отрисовки не пуст (92 спрайта + маркер `MASTER`), башня — ровно у 11
    флаговых типов, кадр карты 640×640 с юнитами всех 93 типов
    (`units_drawn=92`, `units_marker=1`), HUD-портрет для всех типов;
    `assets::tests` проверяют загрузку всех корпусов/теней и башенных кадров
    `D_*`.
  - **QA/артефакты.** `qa_run` — все 10 сценариев PASS; golden пересняты:
    `golden/game.png`, `golden/upgrade/upgrade-done.png`,
    `golden/research/research-done.png` (0 дрейфа после пересъёмки);
    `soak-100.qa` и `soak_headless --turns 120` PASS. `bash ci.sh` — CI OK
    (577 с): cargo check/test default + sound + no-default, Horizon, все QA,
    golden 0 дрейфа, soak. Пересобраны артефакты Windows headless
    (`max_app-1.1.0-windows-x86_64`), Windows UI (`…-windows-x86_64-ui`) и
    Linux (`max_app-1.1.0-linux-x86_64`).

- **P7 — мобилизация: портрет `P_*` 1:1, описания юнитов и сетка иконок
  статистики** (`max_app/src/mobilize.rs`, `max_app/src/cargo_unit_data.rs`,
  `docs/development/PLAN-production.md`):
  - **Описания.** Источник — `SC_UNITS` (`description` → hex-ключ) +
    `assets/language.json` (en-US), как `Unit::GetDescription()`. Генератор
    `max-rust/tools/gen_cargo_unit_data.py` расшифровывает XOR-ресурс и
    складывает 93 записи в `max_app/data/cargo_unit_data.json` (читается
    `include_str!` + serde_json). Текстов 69: у 24 служебных типов (обломки,
    взрывы, снаряды, `HARVSTER`, `WALDO`) описания в оригинале нет, но они и
    не достижимы в `CargoMenu` (`IsUnitFiltered`). Рисуется как
    `AbstractUpgradeMenu::DrawUnitInfo`: `Text_TextBox(16,17,280,230,
    GNW_TEXT_OUTLINE | 0xA2)` поверх портрета, шрифт 5, перенос
    `Text_SplitText` (ширина 280, 20 строк, интервал 11 px, чёрная обводка
    `Text_BlitTTF`).
  - **Портрет.** C++ рисует `Unit::GetPortrait()` — big image `P_*` 300×240,
    `WindowManager_LoadBigImage(..., palette_from_image=false)` 1:1 в (11,13);
    у нас палитра окна `CARGOPIC`, обрезка окном `unit_portrait_window`
    280×240 (C++-порт пишет 300 px без клипа — 20 правых пикселей залезают на
    фон). 71 ресурс `P_*`, ленивый кэш `CargoMenuAssets::portraits`.
  - **Иконки статистики.** `UnitStats_DrawStats` 1:1: строки 19 px, число по
    правому краю x=24 (`ReportStats_DrawNumber`), подпись x=28 (клип 49 px) из
    `language.json` (`Attack`, `Shots`, `Range`, `Ammo`, `Cargo`, `Prod.`,
    `Uses`, `Armor`, `Hits`, `Scan`, `Speed`, `Cost`), pips
    `UnitStats_DrawIcons` из `I_HRDATK`/`I_SHOTS`/`I_RANGE`/`I_AMMO`/`I_RAW`/
    `I_FUEL`/`I_GOLD`/`I_POWER`/`I_LIFE`/`I_ARMOR`/`I_HITS`/`I_SCAN`/`I_SPEED`,
    дельта апгрейда — пустые иконки той же строки (у `Cost` — `I_GOLDE`) за
    красной чертой, красные линии под строками. Строка `Cost` — ставка
    строителя (`Builder_GetBuilderType`) × ходы, а не цена покупки.
  - **Тесты.** 4 UNIT-теста `cargo_unit_data` (93 записи, 71 портрет, 69
    описаний, все открываемые типы описаны), 6 UNIT-тестов `mobilize`
    (Cargo/Uses/`Prod.`, `min(ROUNDS, AMMO)`, ставка строителя, дельта pips),
    frame-тесты `tests/cargo_render.rs` (портрет, описание вкл/выкл, pips,
    линии, смена типа), `qa_driver` — кадр `cargo-description`.
  - **QA.** `assert cargo-description/cargo-stat-icons/cargo-portrait`;
    `cargo-full.qa` открывает Tank и проверяет описание/портрет/иконки; новый
    golden `golden/cargo/cargo-description.png`, переснятые
    `cargo-buy`/`cargo-upgrade`/`cargo-upgraded` и `golden/mobilize.png`
    (0 дрейфа).

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
    отрисовка (иконки, бары, стрелки, `CRGTOG`, прокрутка); 25 UNIT-тестов
    `mobilize` + 4 `cargo_unit_data`; `start_phases::mobilization_upgrades_reach_the_deployed_units`.
  - **QA.** Новый сценарий `cargo-full.qa` (покупка из конца полного списка,
    cargo, переключение в модернизацию, фильтры, апгрейд, `Done`), команды
    `mobilize mode|filter|select|loadout|row|cargo`, проверки
    `assert cargo-mode/cargo-types/cargo-credits`; golden-кадры
    `golden/cargo/{cargo-buy,cargo-upgrade,cargo-upgraded}.png` и обновлённый
    `golden/mobilize.png`; `ci.sh`/GitHub CI прогоняют `cargo-full` и его golden.

### Исправлено

- **Мобилизация: у танка, мобильной зенитки и эскорта на иконках списков
  пропадала башня** (`max_app/src/mobilize.rs`, `max_app/src/assets.rs`,
  `max_assets/src/sprite.rs`). В C++ `ReportStats_DrawListItemIcon` для
  `TURRET_SPRITE`/`SPINNING_TURRET` рисует **второй кадр** `I_*` со сдвигом
  `angle_offsets[2] >> divisor` (divisor 1 — мобильные, 2 — стационарные).
  Теперь `D_*` (`Unit::GetFrameInfo()`) разбирается в `FrameInfo`, иконка
  хранит корпус (кадр 0) и башню (кадр 1), `draw_icon` накладывает её.
  Тот же `FrameInfo` подключён к карте (`render.rs`), портрету HUD
  (`panels.rs`), меню апгрейдов и `BuildMenu`: башня рисуется отдельным кадром
  `turret_image_base + turret_angle` со сдвигом `angle_offsets[body_angle]`
  (`SpriteRenderer::draw_turret_scaled`) — как `UnitInfo::RenderWithConnectors`.
  Тесты: `FrameInfo::decode` (D_TANK/D_FIRING), `turret_icons_carry_*`,
  `turret_is_painted_over_the_tank_icon_in_the_list`,
  `sprite_renderer_draws_the_tank_turret_as_a_separate_frame`; golden
  `cargo-*`/`mobilize` пересняты (0 дрейфа).

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
