## [Unreleased]

### Исправлено

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
