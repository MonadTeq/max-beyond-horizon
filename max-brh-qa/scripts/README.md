# QA-скрипты клиента M.A.X. (слои 2–4)

Этот каталог — слой 2 (**скриптовый UI-драйвер**), слой 3 (**golden-кадры**) и
их обвязка в CI из `max-brh-docs/qa-plan.md`. Всё работает **in-process, без SDL, окна
и сети**: экраны и игровой цикл `max_app` — чистые функции от `InputEvent`,
поэтому скрипт нажимает те же кнопки, что игрок мышью, и получает те же кадры.

```
max-brh-qa/
├── scripts/sp-full.qa          # одиночная партия: New Game → Options → Planet → Players → 3 хода с ИИ
├── scripts/hs-2.qa             # hot-seat на двоих: окна «Begin turn», 2+ раунда, чередование команд
├── scripts/visual-screens.qa   # проход по всем экранам; кадры = эталоны golden/
├── scripts/victory.qa          # партия до исхода «победа» + экран победы
├── scripts/production.qa       # P2: конструктор → BuildMenu → TRAINHAL за 10 ходов → ADUMP + POWGEN/FDUMP/HABITAT в комплекс → завод → INFANTRY
├── scripts/save-load.qa        # P3: 2 хода → save_v70 → load в свежем App → те же ход/юниты
├── scripts/upgrade.qa          # P6: экран апгрейдов (U) → покупка → Done/Cancel, золото
├── scripts/research.qa         # P6: Research Center → O (питание) → 16 ходов → уровень 1
├── scripts/research-help.qa    # P7: ResearchMenu (распределение центров) + 4 Help-экрана
├── scripts/cargo-full.qa       # P7: полная мобилизация (15 типов, cargo, модернизация, апгрейд)
├── scripts/soak-100.qa         # P4: soak-партия 100 ходов (человек + компьютер), mem/кадры/ассерты
├── scripts/edge-cases.qa       # P4: TC-1106/1111/1112 — спам кликов, юникод, повторный старт
├── scripts/move-input.qa       # TC-0601/0603/0604: выделение кликом → RMB-приказ → End Turn → юнит сдвинулся
├── scripts/unit-menu.qa        # TC-0622–0626: меню команд юнита (повторный клик), Sentry, Esc, модальность, Build
├── scripts/allocate.qa         # TC-0627–0629: пункт allocate → диалог ALLOCFRM → set/row → Done (08/P5) → доход комплекса
├── scripts/xfer.qa             # TC-0632–0635: X-FER (TransferMenu) → Done (08/P3) → ProcessTransfer ↔ комплекс
├── scripts/preferences.qa      # TC-0935: диалог Preferences/Setup (PREFSPIC/SETUPPIC), 4 golden
├── scripts/ai-turn.qa          # A2/A3: мой ход → ход ИИ (End Turn нажат, таймер 00:45) → возврат хода
├── scripts/sp-tb.qa            # Turn Based SP: RED→GREEN→BLUE→RED, 2 компьютера, endturn-frames
├── scripts/combat-tb.qa        # Turn Based бой: выстрел ROCKET → след RKTSMOKE → взрыв → вспышка танка → End Turn
├── scripts/missile-line.qa     # снаряд летит ПО ПРЯМОЙ (диагональ −3/+3 после AI A.4): `assert missile-line`, след по линии
├── scripts/build-cancel.qa     # строителя нельзя увести (UNIT_NO_GO); отмена — только явный `stop` (CancelBuilding)
├── scripts/panels.qa           # кнопки 3×3 левой панели: переключатели и их оверлеи (overlay/panel)
├── golden/*.png                # эталоны visual-screens.qa (15 кадров, ~1.6 МиБ)
├── golden/production/*.png     # эталоны BuildMenu, стройплощадки, её роста и кадров строителя (production.qa)
├── golden/upgrade/*.png        # эталоны UpgradeMenu (upgrade.qa)
├── golden/research/*.png       # эталон партии после исследования (research.qa)
├── golden/research-help/*.png  # эталоны ResearchMenu + Help-экранов (research-help.qa)
├── golden/cargo/*.png          # эталоны CargoMenu: покупка/модернизация/после апгрейда
├── golden/move-input/*.png     # эталоны старта/выделения шахты/выделения Surveyor'а/предпросмотра/хода 2 (6)
├── golden/unit-menu/*.png      # эталоны меню команд юнита: танк/нажатый sentry/строитель/выделение (4)
├── golden/allocate/*.png       # эталоны диалога распределения добычи: выделение шахты/диалог/значения (3)
├── golden/xfer/*.png           # эталоны окна X-FER: меню/значение/помощь TRANSFER_SETUP (4)
├── golden/preferences/*.png    # эталоны диалога Preferences/Setup (4)
├── golden/ai-turn/*.png        # эталоны фаз End Turn: мой ход / ход ИИ / возврат (3)
├── golden/sp-tb/*.png          # эталоны Turn Based SP с двумя компьютерами (3)
├── golden/combat-tb/*.png      # эталоны боя: выстрел/снаряд/след/взрыв/вспышка танка (11)
├── golden/missile-line/*.png   # эталоны пиксельной прямой снаряда (3: два кадра в полёте + попадание)
├── golden/build-cancel/*.png   # эталоны NO_GO и явной отмены стройки (4)
└── scripts/README.md
```

Драйвер живёт в библиотеке (`max-brh/crates/max_app/src/qa/`), а примеры —
тонкие CLI-обёртки (команды — из `max-brh/`, пути к сценариям/эталонам — через
`../max-brh-qa/`):

```sh
cd max-brh

# слой 2: прогнать сценарий (log.txt + PNG в каталог вывода)
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-full.qa qa-out/sp-full

# слой 3: сверить кадры с эталонами
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden
```

Код выхода `qa_run`: **0** — все шаги выполнились, **1** — упал `assert` или
команда (текст в stderr и в `log.txt`), **2** — неверные аргументы CLI.

## Формат `.qa`-скрипта

Одна строка — одна команда. `#` начинает комментарий (внутри `"…"` — не
начинает), пустые строки игнорируются, регистр имён команд не важен.
**Координаты — пиксели кадра 640×480** (окно оригинала), это же окно рисуют
`shot`-кадры, поэтому скрипт одинаково работает на Windows и в Docker.

### Низкоуровневые команды (ровно как ввод игрока)

| Команда | Что делает |
|---|---|
| `click X Y` | левая кнопка в пикселях кадра → `InputEvent::MouseClick` |
| `click unit NAME` | левый клик по юниту игрока с таким именем **без** проверки выделения — для курсорных режимов (`x-fer`/`repair`/`reload`): камера центрируется, клик идёт в клетку юнита |
| `rclick X Y` | правая кнопка (приказ движения/отмена) |
| `key ENTER\|ESC\|TAB\|BACKSPACE\|UP\|DOWN\|LEFT\|RIGHT\|<символ>` | клавиша |
| `type "text"` | печатные символы по одному (поле имени, чат, лимит победы) |
| `wait N` | до N кадров, пока игра не будет готова к вводу (ход игрока, окно перехода или конец партии) |
| `tick N` | ровно N кадров игрового цикла (`App::local_update`) |
| `anim N` | прокрутить **только визуальные часы** на N мс реального времени (`dt > 0`): покадровая анимация строителя и «вырастание» постройки, без тиков симуляции — детерминированные кадры анимации |
| `mem` | текущий RSS процесса (и пик) в лог шага — P4 soak/TC-1108; `# mem start/end` пишутся автоматически |
| `shot NAME` | PNG кадра 640×480 в `<out_dir>/NAME.png` |
| `assert …` | проверка состояния (таблица ниже); падение → exit 1 |
| `save NAME` | контейнер локальной партии `MXLS` (`App::write_local_save`) |
| `save_v70 NAME` | **настоящий V70-сейв** текущей партии (`GameState::to_save_bytes`, P3): читается `SaveFile::parse`, C++-портом и командой `load` |
| `load NAME` | новая локальная партия из **V70**-сейва (`*.CAM`/`*.dta` — в т.ч. записанного `save_v70`) |
| `exit` | остановить скрипт (успех) |

### Высокоуровневые команды (читаемые сценарии)

| Команда | Что делает |
|---|---|
| `menu new_game` | `New Game` главного меню → экран Options |
| `menu multiplayer` | `Multiplayer Game` → `Multiplayer Menu` |
| `menu hot_seat` | `Multiplayer Menu` → `Hot Seat Game` → настройка hot-seat |
| `menu join` | `Multiplayer Menu` → `Join Network Game` → список игр (клиент лобби — заглушка QA, 2 детерминированные игры) |
| `menu intro` | `Introduction` главного меню → экран-заглушка MVE (TC-0108) |
| `menu credits` | `Credits` главного меню → прокрутка титров `CREDITS` (TC-0108) |
| `menu setup` | `Setup` главного меню → диалог `OptionsMenu(SETUPPIC)` (`screen=Preferences`, TC-0935) |
| `setup done` | кнопка `Done` текущего экрана настройки (Options → Planet → Players → открыть фазы старта) |
| `setup cancel` | кнопка `Cancel` |
| `setup random` | кнопка `Random` на экране планет |
| `players SLOT human\|computer\|none` | клик по кнопке типа команды в слоте `0..3` (RED/GREEN/BLUE/GRAY) |
| `mobilize buy` | кнопка `Buy` экрана мобилизации (покупает выбранную строку) |
| `mobilize buy N` | выбрать строку `N` списка `CargoMenu::unit_types1` (0..14) и купить её |
| `mobilize delete` | кнопка `Delete` (продать выбранный купленный юнит) |
| `mobilize step N` | сдвинуть выбор по списку (N > 0 — вниз, N < 0 — вверх) |
| `mobilize done` | кнопка `Done` мобилизации → выбор точки высадки |
| `mobilize cancel` | кнопка `Cancel` мобилизации → назад на `Players` |
| `landing click X Y` | клик по кадру 640×480: двигает курсор высадки |
| `landing grid X Y` | клик по клетке карты 112×112 (4 px/клетка) |
| `landing done` | `Enter` — подтвердить точку (`GameManager_IsValidStartingPosition`) |
| `landing cancel` | `Esc` — отменить выбор → назад на `Players` |
| `select X Y` | левый клик + проверка, что выделен юнит **своей** команды |
| `select unit N` | левый клик по N-му **подвижному** юниту игрока (камера центрируется на нём) |
| `select last` | левый клик по самому новому юниту игрока (в т.ч. стационарный завод) |
| `select type NAME` | клик по юниту игрока с именем из `BuildMenu` («Training Hall», «Tank», …) |
| `build open` | открыть `BuildMenu` для выделенного строителя/завода (клавиша `B`) |
| `build select N` | выбрать строку списка доступного |
| `build add` / `build delete` | `Build` (в очередь) / `Delete` завода |
| `build rate 1\|2\|4` | кнопки скорости 1×/2×/4× |
| `build repeat` | кнопка `Repeat` завода |
| `build step N` / `build queue step N` | стрелки списка / очереди (N > 0 — вниз) |
| `build done` | `Done`/Enter — заказ (`PayloadP4`, пакет 08). Для **здания** у мобильного строителя заказ ещё не уходит: открывается выбор места (`ORDER_STATE_SELECT_SITE`, призрак `LRGTAPE`), как C++ `MobileBuildMenu::Build` |
| `build cancel` | `Cancel`/Esc — закрыть без заказа (в режиме выбора места — отменить его) |
| `build place X Y` | LMB по клетке карты: подтвердить место стройки (`GameManager_PathBuild`); площадка 2×2 встаёт в эту клетку (якорь 2×2, накрывающий её) |
| `build place near` | то же по клетке выделенного строителя (якорь всегда накрывает строителя) |
| `build cursor X Y` | навести мышь на клетку без клика: призрак переезжает, валидность (`build-site=valid\|invalid`) проверяется до подтверждения |
| `build path` | кнопка `Path` (`BLDPTH_U`) меню **инженера** (`buildmenu.cpp:836`): `Build()` с принудительным выбором места 1×1 (`event_click_path_build`), площадка — в клетке курсора, только в строке/столбце инженера (`GetBuilderUnitCursor`, `game_manager.cpp:5854`) |
| `upgrade open` | открыть экран апгрейдов (P6, клавиша `U`) для выделенного юнита |
| `upgrade select N` | выбрать строку списка типов |
| `upgrade row N up\|down` | нажать стрелку `UPGRGT`/`UPGLFT` строки N (цена/шаг — C++) |
| `upgrade done` | `Done`/Enter — `CommitUpgradeChanges` (золото списывается) |
| `upgrade cancel` | `Cancel`/Esc — закрыть без изменений |
| `research open` | открыть `ResearchMenu` (P7, клавиша `R`) для выделенного **включённого** `RESEARCH`-центра |
| `research select N` | выбрать тему (0 Attack … 7 Cost) |
| `research row N up\|down` | стрелка `UPGRGT`/`UPGLFT` темы N: `+1`/`−1` центр (свободные центры — `free_capacity`) |
| `research done` | `Done`/Enter — `ResearchMenu_OnClick_Done`: распределение пишется в `GameState` |
| `research cancel` | `Cancel`/Esc — закрыть без изменений |
| `help cargo\|planet\|newgame\|options\|research\|upgrades\|game\|hotseat\|clan\|preferences\|setup_menu` | включить **режим помощи** C++ (`HelpMenu_Menu`, `helpmenu.cpp:315`) кнопкой `?`/`Help` текущего экрана: окна ещё нет, курсор `CURSOR_HELP` |
| `help entry NAME` | клик по центру записи `NAME` из `help.json` (тот же путь, что у игрока: `HelpMenu_ScaleCursor` + `Help::GetEntry`); открывает окно с одной записью |
| `help click X Y` | клик по точке кадра в режиме помощи: открывает запись под точкой; пустое место режим не выключает |
| `help page up\|down` | `PageUp`/`PageDown` — страница текста окна (16 строк; записи обычно короче — прокручивается `Keys`) |
| `help up\|down` | одна строка вверх/вниз |
| `help keys` | кнопка `Keys` (`GAME_SCREEN_SETUP`): подсказка `_(ceb6)` |
| `help done` | `Done`/Enter — закрыть **окно**, вернуться в режим помощи (C++ `HelpMenu::Run`) |
| `help close` | выйти из режима (`Esc` в режиме/`Ctrl-Esc`) |
| `unit-menu open` | повторный клик по выделенному своему юниту — меню команд (`GameManager_ClickUnit` → `InitPopupButtons`); проверяет, что меню открылось |
| `unit-menu item NAME` | клик по кнопке пункта (подпись `sentry`, `Build`, `x-fer`, … или символ позиции); отключённый пункт — ошибка с причиной |
| `unit-menu key C` | горячая клавиша пункта (`GameManager_ProcessPopupMenuInput`) |
| `unit-menu close` | `Esc` — закрыть меню |
| `spawn NAME` / `spawn enemy NAME` | QA-only: поставить юнита на свободную клетку рядом с выделенным (или в центре карты) — для пунктов, недоступных в мобилизации (диверсант, транспорт, бульдозер, здание под `remove`) |
| `spawn at X Y NAME` | QA-only: поставить юнита **ровно в клетку** (клетки должны быть свободны) — например, `ADUMP` вплотную к шахте для проверки слияния комплекса |
| `selfdestruct arm\|ok\|cancel\|advance N` | диалог `UnitsManager_SelfDestructMenu`: `Arm`, `OK` после анимации (`SLFDOPN1..6`, `advance` — визуальные часы), `Cancel`/`Esc` |
| `complex raw N` | QA-only: запас сырья комплекса выделенного юнита (проверка `Upg. All`) |
| `complex fuel N` / `complex gold N` | QA-only: запас топлива/золота комплекса (`Complex::Transfer`) — нужен, чтобы включить `POWGEN`/`POWERSTN` и проверить энергетику |
| `teamvalues NAME ATTR VALUE` | QA-only: текущее значение атрибута команды (`hits`, `attack`, …) — апгрейд доступен, когда `current` выше `base` |
| `key ALT_P` | `Alt+P` — `PauseMenu_Menu` (диалог «Game Paused», `dialogmenu.cpp`) |
| `allocate open` | открыть диалог распределения добычи (`AllocMenu`, `ALLOCFRM`) для выделенной включённой шахты (`MININGST` с комплексом) |
| `allocate select raw\|fuel\|gold` | выбрать секцию (клавиатура Rust-клиента, как `↑/↓`) |
| `allocate set N` / `allocate set raw\|fuel\|gold N` | абсолютный уровень добычи (клик по полосе, `OnClick*Bar`; `N` обрезается по ёмкости шахты — 16 единиц) |
| `allocate row [raw\|fuel\|gold] up\|down` | стрелка `RTARO_OF`/`LFARO_OF`: `+1`/`−1` единица |
| `allocate done` / `allocate cancel` | `Done`/`Enter` или `Esc`: закрыть и разослать `ORDER_NEW_ALLOCATE` (пакет **08/P5**) шахтам комплекса (в C++ `Esc` и `KP_Enter` — один и тот же выход) |
| `transfer amount N` / `transfer set N` | значение полосы X-FER (как клик по полосе `LimitedScrollbar`): `N` обрезается по `[take_max, give_max]`; отрицательное — из комплекса в юнит |
| `transfer right` / `transfer left` | кнопки `XFRRGT`/`XFRLFT`: `+1`/`−1` (в пределах `give_max`/`take_max`) |
| `transfer done` / `transfer cancel` | `Done`/`Enter` (пакет **08/P3** `ORDER_TRANSFER`) или `Esc` без приказа |
| `transfer help` | `?`/`Shift+/`: окно помощи `TRANSFER_SETUP` (`HelpMenu_Menu`) |
| `prefs open` | открыть диалог `Preferences`: в игре — кликом по кнопке `PREF_OFF` панели (`GameManager_MenuClickPreferencesButton`), в главном меню — пунктом `Setup` (`menu_setup_window`) |
| `prefs toggle NAME` | клик по чекбоксу диалога: `effects`, `click_scroll`, `fast_movement`, `follow_unit`, `enemy_halt`, `auto_select`, `disable_music`, `disable_fx` (алиас `sound`), `disable_voice`, `disable_movie`, `auto_save`, `enhanced_graphics` |
| `prefs set NAME N` | клик по слайдеру в позицию значения (квантование 57 px как `UpdateSlider`): `music_level`, `fx_sound_level`, `voice_level`, `movie_level`, `quick_scroll` |
| `prefs done` / `prefs cancel` | `Done` (сохраняет в `preferences.ini`/`IniPreferences`) или `Cancel` (возвращает снимок открытия) |
| `prefs help` | кнопка `Help`/`?` диалога: `PREFS_MENU_SETUP`/`SETUP_MENU_SETUP` |
| `mobilize mode buy\|upgrade` | переключатель `CRGTOG` (542,446) — список покупки/модернизации |
| `mobilize filter ground\|air\|sea\|building\|combat` | фильтр домена `SELGRD/SELAIR/SELSEA/SELBLD/SELCBT` |
| `mobilize select N` | строка N списка Available (прокручивает список стрелками, как игрок) |
| `mobilize loadout N` | строка N списка Purchased (бесплатная загрузка + покупки) |
| `mobilize row N up\|down` | стрелка `UPGRGT`/`UPGLFT` строки апгрейда N |
| `mobilize cargo N` | cargo-слайдер: N блоков по 5 единиц (1 кредит за блок) |
| `move X Y` | правый клик + проверка, что приказ движения принят |
| `move grid X Y` | то же по клетке карты (0..111), если она видна в области карты |
| `move near` | приказ в соседнюю свободную клетку выделенного юнита (перебор 8 направлений) |
| `move near refused` | RMB по соседней клетке **обязан** не дать приказа (`UNIT_NO_GO`): проверяет отказ, а не приказ |
| `launch near` / `launch grid X Y` | вывод готового юнита из завода: RMB по клетке рядом даёт `ORDER_ACTIVATE` **заводу** (`CURSOR_ACTIVATE`, `game_manager.cpp:6918-6926`), `UnitsManager_ActivateUnit` выпускает юнита (`production.qa`) |
| `wait firing` | крутить кадры, пока выделенный юнит не покажет кадр выстрела (`firing_recoil_frames > 0`): убирает зависимость сценария от точного числа тиков наведения (`combat-tb.qa`, `missile-line.qa`) |
| `end_turn` | клик по кнопке `End Turn` + прокрутка кадров, пока игра не вернётся к вводу (ход ИИ/окно перехода) |
| `click enemy NAME` | клик по чужому юниту с таким именем; при выделенном своём — приказ атаки (`GameManager_ClickUnit` → `build_attack_order`) |

Высокоуровневые команды не «обходят» UI: каждая сводится к тому же
`InputEvent`, что отправляет `main.rs`, и затем проверяет результат
(выделение, приказ, смена экрана) — опечатка в координатах падает, а не
молчит.

### Проверки (`assert`)

| Проверка | Смысл |
|---|---|
| `assert turn=3`, `turn>=3`, `turn<=3`, `turn>3`, `turn<3` | `GameState::turn_counter` |
| `assert endturn-frames<2000` | кадров, которые занял последний `end_turn` (переход + ходы ИИ); зависший ход ИИ упирается в 12 000 (`LOCAL_AI_TICK_LIMIT`) |
| `assert team=1` / `team=GREEN` | `state.active_turn_team` (0..3 или RED/GREEN/BLUE/GRAY) |
| `assert units>=4` | число юнитов в состоянии |
| `assert unit-count NAME>=N` | юнитов типа `NAME` на карте (имя — как у `spawn`: `RocketSmoke`, `Rocket`, `TorpedoBubble`, `Training Hall`, …) |
| `assert screen=InGame` | текущий экран: `MainMenu`, `JoinList`, `CreateForm`, `MultiplayerMenu`, `Introduction`, `Credits`, `Options`, `Planet`, `Players`, `Clan`, `LocalSetup`, `Preferences`, `Mobilize`, `Landing`, `GameOver`, `InGame` |
| `assert phase=PlayerTurn` | фаза локальной партии: `PlayerTurn`, `AiTurn`, `Advancing`, `Finished` |
| `assert notice=RED` / `notice=none` | окно «Begin turn» (M-HS) открыто для команды |
| `assert victory` / `assert defeat` | исход партии (`LocalOutcome`) |
| `assert selected` | выделен подвижный юнит игрока |
| `assert preview` | у выделенного юнита есть непустой предпросмотр пути (зелёный маршрут после RMB) |
| `assert unit-pos=X,Y` | выделенный юнит стоит в клетке `(X, Y)` |
| `assert angle=N` | угол корпуса выделенного юнита `0..7` (`0` = север, по часовой): симуляционное `UnitInfo::angle`, меняется по ходу движения (`Paths_GetAngle`) |
| `assert log=ТЕКСТ` | в журнале HUD есть строка с `ТЕКСТ` («Move: N steps to (X, Y)») |
| `assert digest=0xBEEF` | CRC16 снапшота состояния (`App::digest`) |
| `assert build=open` / `build=closed` | открыт ли `BuildMenu` (P2) |
| `assert upgrade=open` / `upgrade=closed` | открыт ли экран апгрейдов (P6) |
| `assert research-menu=open` / `research-menu=closed` | открыт ли `ResearchMenu` (P7) |
| `assert research-alloc attack=N` (и `>=`, `<=`, `>`, `<`) | сколько центров закреплено за темой (`attack\|shots\|range\|armor\|hits\|speed\|scan\|cost`) |
| `assert help=open` / `help=closed` | открыто ли **окно** помощи (режим помощи — `help-section`, но окна нет) |
| `assert help-section=CARGO_SETUP` (короткие `cargo\|planet\|newgame\|options\|research\|upgrades\|game\|preferences\|setup_menu`) | секция режима/окна помощи |
| `assert help-entry=NAME` | какая запись `help.json` показана в окне |
| `assert help-row>=N` | первая видимая строка окна помощи (проверка прокрутки) |
| `assert unit-menu-item=NAME` / `unit-menu-enabled=NAME` / `unit-menu-disabled=NAME` | пункт меню юнита есть / включён / отключён (с причиной) |
| `assert cursor=load\|steal\|disable\|repair\|reload\|transfer\|none` | курсорный режим выделенного юнита (`UnitInfo::cursor`) |
| `assert targeting=on\|off` | режим наведения (`targeting_mode`, пункт `attack`) |
| `assert self-destruct=open\|closed` | диалог `UnitsManager_SelfDestructMenu` |
| `assert pause=open\|closed` | диалог паузы `Alt+P` (`PauseMenu_Menu`) |
| `assert gold>=N` (и `=`, `<=`, `>`, `<`) | золото локальной команды |
| `assert research attack>=1` | уровень темы исследований (`attack|shots|range|armor|hits|speed|scan|cost`) |
| `assert cargo-mode=buy\|upgrade` | режим списка `CargoMenu` (P7) |
| `assert cargo-types>=N` | длина списка типов (15 в покупке, 62 в модернизации по умолчанию) |
| `assert cargo-credits>=N` | остаток кредитов в `CargoMenu` |
| `assert unit=NAME` | у игрока есть юнит с именем из `BuildMenu` («Training Hall», «Infantry», …) |
| `assert site=large\|small\|none` | стройплощадка в мире: `LRG/SML-TAPE`+`CONES` (+`SLAB`) в одной клетке; `none` — ни ленты, ни конусов у команды игрока (плита под готовым зданием остаётся и не мешает) |
| `assert site=drawn` | декали площадки **нарисованы** в текущем кадре: каждая (лента, конусы, плита) даёт свои пиксели — проверка `IsRelevant` (`ORDER_IDLE` не рисуется) |
| `assert site-pos=X,Y` | клетка ленты стройплощадки (`LRG/SML-TAPE` команды игрока) |
| `assert build-site=open` / `build-site=closed` | идёт ли выбор места стройки (`ORDER_STATE_SELECT_SITE`, призрак `GameManager_TempTape`) |
| `assert build-site=valid` / `build-site=invalid` | валидна ли клетка под курсором в режиме выбора (`CURSOR_MAP2` / `CURSOR_UNIT_NO_GO`: 2×2 накрывает курсор и проходима) |
| `assert build-center=yes` / `build-center=no` | спрайт строителя стоит ровно в центре ленты 2×2 (`UnitInfo::PositionInTape` + `OffsetDrawZones`: доворот `Paths_UpdateAngle`, затем 8 px × 4 тика = 32 px по `DIRECTION_OFFSETS[angle]`) |
| `assert builder=inside` / `builder=outside` | стоит ли строитель (`CONSTRCT`) в габаритах ближайшего своего здания 2×2: `inside` — площадка ещё занята им, `outside` — он сошёл с неё (`ORDER_ACTIVATE` → `PositionInTape` LEAVE → `ActivateEngineer` → `ORDER_MOVE`) |
| `assert build-ready=yes` / `build-ready=no` | есть ли у игрока достроенное здание, ждущее ухода строителя (`ORDER_IDLE`/`ORDER_STATE_BUILDING_READY`: `SpawnNewUnit` уже поставил его на площадку, но `UnitInfoGroup::IsRelevant` его не рисует) |
| `assert factory-ready=yes` / `factory-ready=no` | выделенный **завод** ждёт вывода готового юнита (`ORDER_BUILD`/`ORDER_STATE_UNIT_READY`, `UnitInfo::SpawnNewUnit`, `unitinfo.cpp:3489-3520`) |
| `assert unit-stored=NAME` / `unit-stored=none` | внутри выделенного завода стоит юнит этого типа (`ORDER_IDLE`/`ORDER_STATE_STORE`, `parent` — завод); `none` — готового юнита нет |
| `assert missile-line` | летящий снаряд стоит на **пиксельной прямой** «центр клетки пуска → центр клетки цели» (отклонение ≤ 1.5 px): `AirPath::Execute` (`airpath.cpp:219-242`), ловит «снаряд идёт по клеткам» |
| `assert resource-icons>=N` (и `=`, `<=`, `>`, `<`) | сколько **клеток** с иконкой ресурса нарисовано в текущем кадре (`Survey_RenderMarkers`): только разведанные клетки (`CargoMap & hash_team_id`), `0` — оверлей выключен |
| `assert survey-overlay=yes` / `survey-overlay=no` | включён ли показ иконок ресурсов (`GameManager_RenderSurveyIndicator`: кнопка SURVEY `GameManager_DisplayButtonSurvey` **или** выделенный разведчик `GameManager_IsSurveyorSelected`) |
| `assert panel=NAME:on` / `panel=NAME:off` | переключатель 3×3 левой панели (`CTInfo::display_button_*`, `GameManager_MenuClick*Button`, `game_manager.cpp:3800-3990`): `survey`, `status`, `colors`, `hits`, `ammo`, `range`, `scan`, `grid`, `names` |
| `assert overlay>=N` (и `=`, `<=`, `>`, `<`) | сколько пикселей оверлеев кнопок 3×3 нарисовано в текущем кадре (`RenderStats::overlay_pixels`, `max_app::overlays`): `0` — ни один слой не включён |
| `assert unit-selector=yes` / `unit-selector=no` | нарисован ли индикатор выделенного юнита — три вложенные рамки-уголка `GameManager_DrawUnitSelector` (`double_marker`, `GameManager_MarkerColor` с миганием 210 мс) |
| `assert hint=TEXT` / `hint=none` | подсказка сверху карты (`MessageManager_DrawMessage`, `MESSAGE_BOX_INFO`): для строящего — «Currently building: X. / Turns to completion: N.» (`_(ee24)`), для достроившего — «Finished building X. …» (`_(276c)`); `TEXT` — подстрока без учёта регистра |
| `assert alloc=open` / `alloc=closed` | открыт ли диалог распределения добычи (C++ `AllocMenu`) |
| `assert alloc-raw=N` (и `>=`, `<=`, `>`, `<`) | добыча сырья в комплексе шахты (`production.raw`, сумма `raw_mining`); так же `alloc-fuel`, `alloc-gold` |
| `assert alloc-free=N` | свободные единицы добычи комплекса (`Σ(16 − total_mining)`) |
| `assert complex-raw=N` | запас сырья комплекса (`Complex::raw`, после `manage_economy`); так же `complex-fuel`, `complex-gold` — комплекс выделенного юнита, иначе первый комплекс команды |
| `assert unit-storage=N` | `storage` выделенного юнита (груз по его типу; `ATTRIB_STORAGE` — предел) |
| `assert transfer=open` / `transfer=closed` | открыто ли окно X-FER (`TransferMenu`, `XFERPIC`) |
| `assert transfer-amount=N` | текущее значение полосы X-FER (знаковое) |
| `assert unit-menu=open` / `unit-menu=closed` | открыто ли меню команд юнита |
| `assert unit-menu-item=NAME` | пункт есть в меню и **включён** |
| `assert unit-menu-disabled=NAME` | пункт есть, но отключён (механика не портирована — см. `unitmenu::UnitMenuItem::note`) |
| `assert unit-order=sentry` (и любое `ORDER_*`) | текущий приказ выделенного юнита (`await`, `move`, `sentry`, `poweron`, …) |
| `assert firing=on\|off` | выделенный юнит показывает кадр выстрела: `firing_recoil_frames > 0` (`UnitInfo::PrepareFire`/`ProgressFire`) **и** рендер выбрал `firing_image_base` из `D_*` |
| `assert building=on\|off` | у выделенного мобильного строителя идёт стройка (`ORDER_BUILD`/`AWAIT_TAPE_POSITIONING`, площадка стоит) |
| `assert trail=on\|off` | на карте есть частицы следа снаряда (`RKTSMOKE`/`TRPBUBLE`, `Paths_DrawMissile`) |
| `assert prefs=open` / `prefs=closed` | открыт ли диалог `Preferences`/`Setup` (в меню — `Screen::Preferences`, в игре — модальное окно `App::preferences_dialog`) |
| `assert prefs-value NAME=N` | значение опции в открытом диалоге (чекбокс — `0/1`; `effects`, `disable_fx`, `quick_scroll`, `timer`, …) |
| `assert prefs-effects=on\|off` | `settings["effects"]` (пока диалог открыт — его черновик) |
| `assert prefs-sound=on\|off` | флаг звука `!disable_fx` |
| `assert prefs-audio=on\|off` | фактическое состояние аудио-слоя (`AudioPlayer::enabled`) — флаг дошёл до аудио |

При падении в `log.txt` пишется строка шага и **фактическое** значение
(`assert turn=3: фактически turn=1`), прогон останавливается, `qa_run` → exit 1.

## Сценарии и что они проверяют

| Скрипт | Покрытие | Результат |
|---|---|---|
| `sp-full.qa` | главное меню → Options (No Limit, Turn Based) → Planet → Players (`RED` человек, `GREEN` компьютер) → экран клана (`clan`) → мобилизация (`mobilize buy` ×2 + Done) → выбор точки высадки (`landing grid 30 22`, не совпадает со стартовой четвертью, + Done) → партия: `select unit`, `move near`, 3 × `end_turn` с ходами ИИ, `assert turn>=3`, сохранение `MXLS` | **PASS** (55 шагов, 14 кадров, ~380 кадров цикла) |
| `hs-2.qa` | `Hot Seat Game` → 2 человека → фазы старта для GREEN и RED по очереди (мобилизация + высадка) → окно «Begin turn» для RED и GREEN, `assert team` чередуется, 3 раунда, подтверждение окна `key ENTER` | **PASS** (64 шага, 14 кадров) |
| `visual-screens.qa` | 15 кадров только с `shot` (main menu, introduction, credits, options, planet, players, mobilize, landing, game, mine-selected, portrait, winloss, multiplayer, join list); Introduction — заглушка MVE, Credits — прокрутка после `tick 100`, `mine-selected` — панель статистики шахты (Hits/Cargo/Total, без Speed), portrait — FLC выделенного Surveyor'а (TC-0919); партия заканчивается честно лимитом ходов Duration=1 | **PASS** (442 кадра, 15 кадров = эталоны) |
| `victory.qa` | партия до исхода: лимит ходов 1 → фазы старта → `assert victory`, `assert phase=Finished`, экран победы | **PASS** (29 шагов, 3 кадра) |
| `production.qa` | P2: главное меню → Options → Planet → Players (`RED` человек, `GREEN` компьютер) → мобилизация без покупок → высадка → `select type Constructor` → `B`/`build open` → TRAINHAL → **выбор места (C++ `MobileBuildMenu::Build` → `SELECT_SITE`):** `build done` → `assert build-site=open` (призрак `LRGTAPE`, приказа и площадки в мире ещё нет) → `shot build-site-select` → `build cursor 30 21` → `assert build-site=invalid` → `shot build-site-invalid` → `key ESC` (отмена) → `build open`/`build select 13`/`build done` → `build place 28 20` (LMB; площадка **в указанной клетке**) → `assert site=large` + `assert site-pos=28,20` + `assert site=drawn` + `shot build-placed` (**плита и лента появляются мгновенно**, без тика: `PrepareConstructionSite` в момент заказа) → `select type Constructor` → `assert hint=Currently building: Training Hall` + `shot build-hint` (подсказка сверху карты) → `tick 1` → **скольжение под углом (`PositionInTape`):** `tick 1` → `assert angle=7` + `assert build-center=no` → `shot build-slide-turn` → `tick 1`/`shot build-slide-1` (`−8,−8`) → `tick 1`/`shot build-slide-2` → `tick 1`/`shot build-slide-3` → `tick 1`/`shot build-slide-4` + `assert build-center=yes` (`−32,−32`) → **цикл отмены (TC-0630):** `tick 1` → `assert unit-order=build` + `assert angle=0` (C++ доворот `(angle+1)&7`) → `unit-menu open` (во время стройки в попапе только `stop`) → `unit-menu item stop` → `tick 1` → `assert unit-order=await` → `assert unit-menu-item=Build` (снова включён) → повторный заказ TRAINHAL через `build place near` → `tick 1` → `assert site=large`/`site-pos=28,20` → `assert site=drawn` (каждая декаль даёт пиксели) → `shot build-site` → `anim 60`/`shot build-site-grow` → `anim 1000`/`shot build-site-grown` (вырастание плиты) → `tick 6` → `assert unit-order=build` + `assert build-center=yes` (строитель в центре ленты) → `shot build-work-a` → `anim 150`/`shot build-work-b` (цикл кадров конструктора `+16`/`+24`) → `end_turn` → `assert hint=Currently building` (подсказка на ходе 2) → ещё 9 ходов → **достройка 1:1:** `assert build-ready=yes` + `assert site=large` + `assert builder=inside` + `shot build-done-waiting` (здание ждёт ухода строителя и не рисуется) → `tick 1` → `assert unit-order=activate` + `shot build-done-leaving` → `tick 16` → `assert builder=outside` + `assert site=none` + `assert build-ready=no` + `shot build-done` (здание раскрылось, плита осталась) → `select type Constructor` → `spawn MaterialStorage` (клетка (30,21) примыкает к цеху и шахте) → `select type Training Hall` → `complex raw 60` (`Complex::Transfer` в общий котёл; QA-команда теперь раскладывает сырьё по складам членов) → `assert complex-raw>=50` → `select type MaterialStorage` + `assert unit-storage>=1` (коннекторы слили комплекс) → `build open` завода → INFANTRY (2 хода) → `assert unit=Infantry` (завод платит из общего котла); кадры `buildmenu` (иконки `I_*` 1:4 + `P_*` портрет + характеристики `UnitStats_DrawStats` + `Turns`/`Cost` по `GetTurnsToBuild`: 12/24 и 8/40)/`build-site-select`/`build-site-invalid`/`build-ordered`/`build-slide-turn`/`build-slide-1..4`/`build-site`/`build-site-grow`/`build-site-grown`/`build-work-a`/`build-work-b`/`buildmenu-factory` (характеристики Infiltrator 6/1/4/12/2/8/4/5/9, цена 9/9) | **PASS** (golden `production/` — 22 кадра, 0 дрейфа) |
| `save-load.qa` | P3: локальная партия → два `end_turn` (ход ИИ и новый раунд) → `save_v70 turn2.dta` (настоящий V70) → `load turn2.dta` в свежем состоянии → `assert turn=3`, `assert units=20` (5 боевых + 2 плиты RED и 5 боевых + 2 плиты + 2 стройплощадки GREEN: `ai A.3` 73b9d38 начинает стройку на ходу ИИ; в сейве те же 20 — загрузка ничего не теряет) → `end_turn` после загрузки; кадры `save-menu`/`after-load` | **PASS** |
| `upgrade.qa` | P6: локальная партия → `select type Constructor` → `upgrade open` (клавиша `U`) → `upgrade row 0 up` → `Done`: золото 150 → 142, `current_values` типа изменены; повтор с `Cancel` не меняет золото; кадры `upgrade-menu`/`upgrade-done` | **PASS** (golden `upgrade/`) |
| `research.qa` | P6: конструктор строит Research Center (12 ходов) — заказ здания идёт через выбор места (`build done` → `assert build-site=open` → `build place near` → `assert build-site=closed`) → `select type` + `key o` (питание, пакет 08/P1) → 16 × `end_turn` → `assert research attack>=1`; кадр `research-done` | **PASS** (golden `research/`) |
| `research-help.qa` | P8: `?` на Options/Planet/Players + `Help` мобилизации → окна `OPTIONS_SETUP`/`PLANET_SETUP`/`NEW_GAME_SETUP`/`CARGO_SETUP` (PageDown/PageUp/Done); `R` → `ResearchMenu` (`RSRCHPIC`, 8 тем, центры, `+N%`, ходы), `Help` экрана → `RESEARCH_SETUP`, панель `HELP_OF` → `GAME_SCREEN_SETUP` + `Keys`; `UPGLFT` Attack → `free_capacity` → `UPGRGT` Shots → `Done`; за 16 ходов Shots=1, Attack=0 (Research Center тоже ставится через выбор места, `build place near`) | **PASS** (3430 кадров, 9 PNG; golden `research-help/`) |
| `soak-100.qa` | P4 soak: Options (Clueless, No Limit, Turn Based, Duration 200) → мобилизация без покупок → высадка → `select unit 0`/`move near` в начале и на 41-м ходу → 100 × `end_turn` (ходы ИИ) → `assert turn=21/41/61/81/101`, `assert screen=InGame`, `shot` и `mem` каждые 20 ходов, `save_v70 soak-turn100.dta`; в `log.txt` — `# mem start/end` и (при `MAX_TICK_PROFILE=1`) строка `PROFILE` | **PASS** (100 ходов, 12 208 кадров, 7 PNG; V70-сейв 157 921 Б; RSS 54 696 → 60 944 КиБ) |
| `edge-cases.qa` | P4 TC-1106/1111/1112: 50 кликов по пустым областям главного меню + 50 по карте (без паник), не-ASCII `type` в чате и в поле имени, выход из партии → New Game → Cancel → полный второй старт до `turn=2`; кадры `edge-menu-spam`/`edge-game-spam`/`edge-unicode-*`/`edge-repeat-start`/`edge-second-game` | **PASS** |
| `cargo-full.qa` | P7: мобилизация целиком — `assert cargo-types=15`, покупка Gold Truck (14-я строка, прокрутка списка), Mine Layer и Constructor, cargo-слайдер (+20 единиц), `CRGTOG` → список модернизации (62 типа), фильтры домена, апгрейд armor Constructor'а, `Done` → `assert gold<150`; кадры `cargo-buy`/`cargo-upgrade`/`cargo-upgraded` | **PASS** (golden `cargo/`) |
| `move-input.qa` | TC-0601/0603/0604 сквозь весь путь **кликами мыши**: меню → Options → Planet → Players → мобилизация (`mobilize done`) → высадка (`key ENTER`, `assert screen=InGame`) → `assert units>=7` (стартовые юниты в партии) → клик по шахте (`assert unit-pos=22,22`, панель Hits/Cargo/Total) → клик по своему Surveyor'у (`assert selected`, `assert unit-pos=23,21`) → RMB на соседнюю клетку (`assert preview`, `assert log=Move:`) → `end_turn` → повторный клик по новой клетке (`assert unit-pos=24,21`, `assert angle=2` — угол по ходу движения, TC-0933); кадры `game-start`/`mine-selected`/`game-selected`/`move-preview`/`game-turn2`/`game-after` | **PASS** (134 кадра, 6 PNG = эталоны `move-input/`; регресс «юнит не двигается» — камера на шахте, а не на плите) |
| `unit-menu.qa` | TC-0622–0626: меню команд юнита — покупка танка и строителя → высадка → `select type Tank` → `unit-menu open` (повторный клик, `assert unit-menu=open`) → пункты танка (`sentry`/`enter`/`Done` включены, `attack` отключён) → `unit-menu item sentry` (`assert unit-order=sentry`) → повторное открытие (пункт нажат) → `key 8` (снять) → `Esc` (`assert unit-menu=closed`, `assert screen=InGame`) → модальность (клик по карте закрывает меню, выделение не меняется) → меню строителя → `Build` открывает `BuildMenu`; кадры `unit-menu-tank-selected`/`unit-menu-tank`/`unit-menu-sentry-on`/`unit-menu-builder` | **PASS** (golden `unit-menu/`) |
| `allocate.qa` | TC-0627–0629: распределение добычи шахты — `select type MiningStation` → `unit-menu open` → `unit-menu item allocate` (`assert alloc=open`, пункт **включён**) → значения из комплекса (`alloc-raw=12`, `alloc-fuel=4`, `alloc-gold=0`, `alloc-free=0`) → `allocate select raw` → `allocate set 4` (`assert alloc-raw=4`) → `row raw up`/`down` (`5`/`4`) → `set fuel 2`, `set gold 1` (`assert alloc-free=9`) → `set raw 20` упирается в 16 → `set raw 4` → модальность (клик по карте) → `allocate done` (`assert unit-order=newallocate`, `tick 1` → `poweron`) → `end_turn` → `assert complex-raw>=4` (добыча стала запасом комплекса) → повторное открытие помнит `raw=4` → `cancel`; кадры `allocate-selected`/`allocate-menu`/`allocate-set` | **PASS** (128 кадров; golden `allocate/`) |
| `xfer.qa` | TC-0632–0635: X-FER — конструктор `storage=40` + 3 хода экономики (шахта накопила ≥5) → **общий котёл:** `spawn at 29 22 MaterialStorage` вплотную к шахте (30,22) + `complex raw 100` (`Complex::Transfer`; QA-команда теперь раскладывает сырьё по складам членов) → `key ESC` → `select type MaterialStorage` + `assert unit-storage>=30` (склад в комплексе шахты получил груз) → `select type MiningStation` → `unit-menu open` → `unit-menu-item=x-fer` (пункт **включён**) → `unit-menu item x-fer` (курсор 3) → `click unit Constructor` (`assert transfer=open`) → `assert transfer-amount>=1` (по умолчанию цель заполняется) → `transfer amount`/`right`/`left`/`999` (лимит `give_max`) → модальность (клик по карте) → `transfer help` (`assert help=open`, `help-section=transfer`) → `help done` → `transfer done` → `tick 1` (`assert unit-order=poweron`) → `select type Constructor`/`assert unit-storage=45` → обратный перенос `transfer amount -3` → `assert unit-storage=42` → `transfer cancel` → запасы не меняются; кадры `xfer-selected`/`xfer-menu`/`xfer-set`/`xfer-help` | **PASS** (1332 кадра; golden `xfer/`, 4 эталона) |
| `preferences.qa` | TC-0935: `menu setup` → `SETUPPIC` (`setup-dialog`, `prefs-value disable_fx=0`, toggle → `prefs-sound=off`, Cancel не применяет, Done применяет); партия → кнопка `Preferences` панели → `PREFSPIC` (`prefs-dialog`, все секции, `quick_scroll=16`, `timer=180`), `prefs set quick_scroll 36` → 35, `prefs toggle effects` → `effects=off`, `anim 200` → `prefs-effects-off`, Done, значение помнится, `save_v70 prefs-ini`; звук `prefs-audio=off/on`; Cancel возвращает снимок; `Help` → `help-section=preferences`; 4 golden-кадра | **PASS** (1 кадр цикла, 0 дрейфа) |
| `ai-turn.qa` | A2/A3: конец хода человека и ход компьютера — Options (No Limit + Turn Based) → мобилизация → высадка → `assert endturn=normal` (мой ход: кнопка отжата, таймер 00:00 красным) → `click 426 12` (End Turn) → `tick 1` (`assert team=GREEN`, `assert endturn=pressed`: нажатый `G_ENDT_D` с подписью цветом команды, таймер 00:45) → `wait 2000` (`assert endturn=normal`, `assert team=RED`, `turn>=2`); кадры `ai-turn-human-normal`/`ai-turn-ai-pressed`/`ai-turn-human-again` | **PASS** (298 кадров; golden `ai-turn/`) |
| `sp-tb.qa` | Turn Based SP: Options (No Limit + Turn Based) → Planet → Players (RED человек, GREEN+BLUE компьютеры, GRAY пусто) → мобилизация → высадка → `assert turn=1`/`team=RED` → `select unit 0`/`move near` → `end_turn` → `assert turn=2`/`team=RED`/`endturn-frames<3000` (два хода ИИ: 658 кадров) → ещё 2 раунда (1022/1029) → `save sp-tb.mxs`; кадры `sp-tb-turn1/turn2/turn4` | **PASS** (2719 кадров; golden `sp-tb/`) |

> **Энергия в сценариях.** `production.qa`, `research.qa` и `research-help.qa`
> строят завод/центр отдельным комплексом и поэтому ставят рядом
> `Power Generator` + `Fuel Tank` (+ `Habitat` для `TRAINHAL`/`RESEARCH`),
> а топливо кладут QA-командой `complex fuel N`. Это не обход механики:
> ровно так C++ `ValidateIndustry` (`production_manager.cpp:677-711`) требует
> покрыть `power`/`life` из котла комплекса — без генератора заказ теперь
> корректно не стартует (`BuildError::NotEnoughPower`). Golden этих трёх
> наборов перегенерирован (в кадре появились новые здания, исследовательский
> центр рисуется включённым).
| `combat-tb.qa` | Turn Based бой: `tick 5` → `select unit 0` → `spawn Missile Crawler` + `spawn enemy Tank` → `select type Missile Crawler` → `click enemy Tank` (клик по чужому юниту = атака) → `assert log=Attack:` → `tick 2` (`assert unit-order=fire`) → `tick 4` (`assert firing=on`: кадр `firing_image_base` у ракетчика; `assert unit-order=fire`) → кадры `fired`/`rocket` → `tick 2` (`assert trail=on`, `assert unit-count RocketSmoke>=3`, кадр `trail`) → `impact`/`explosion`/`explosion-mid`/`explosion-end` (кадры `image_index` `HitExplosion` 0→4) → `assert unit-order=await` → танк: `spawn Tank`/`spawn enemy Tank`/`click enemy Tank` → `tick 15` (`assert unit-order=fire`) → `tick 5` (`assert firing=on`, кадры `tank-fired`/`tank-restored`, `assert firing=off`) → `end_turn` → `assert turn>=2`, `assert endturn-frames<2000` → `combat-tb-after-ai` | **PASS** (299 кадров; golden `combat-tb/`; кадр `after-ai` — снимок хода ИИ, перегенерирован после AI-порта A.2/A.4: 1.544 % → 2.99 % дрейфа, кадры самого боя не дрейфуют) |
| `panels.qa` | Кнопки 3×3 левой панели (`CTInfo::display_button_*`, `game_manager.cpp:3800-3990`): Options (No Limit + Turn Based) → Planet → Players → мобилизация Tank (есть RANGE/SCAN/HITS/AMMO) → высадка → `select type Tank` → все переключатели `off` (`assert overlay=0`) → поочерёдное включение кликом по панели: GRID (141,322), COLORS (31,339), HITS (86,306), AMMO (86,322), RANGE (86,339), SCAN (141,306), NAMES (141,339), STATUS (31,322), SURVEY (31,306) с `assert panel=NAME:on`, `assert overlay>0`, `shot panel-*` и повторным кликом (`assert overlay=0`) → SURVEY дополнительно `assert survey-overlay=yes|no` → комбинация GRID+HITS+COLORS (кадр `panels-combined`): **PASS** (5 кадров; golden `panels/`, 12 PNG) |
| `build-cancel.qa` | Строителя **нельзя увести** с недостроенной площадки (Turn Based, No Limit): `select type Constructor` → `build open` → `build select 13` → `build done` → `build place 28 20` → `assert unit-order=build`, `assert site=large`, `assert building=on`, `assert unit-count Training Hall=0` (кадр `build-cancel-placed`) → `move near refused` (RMB по соседней клетке обязан дать `UNIT_NO_GO`: приказ не создаётся, курсор `CURSOR_UNIT_NO_GO`) → `assert unit-order=build`, `assert site=large`, `assert building=on`, очередь/плита целы (кадр `build-cancel-no-go`) → явная отмена `unit-menu open`/`assert unit-menu-item=stop`/`unit-menu item stop` (`ORDER_BUILD`+`BUILD_CANCEL` → `CancelBuilding`) → `tick 1` → `assert unit-order=await`, `assert site=none`, `assert building=off` (кадр `build-cancel-stopped`) → `end_turn` → `assert turn>=2`, `assert unit-count Training Hall=0` (кадр `build-cancel-after-turn`): ход производства отменённую стройку не достраивает | **PASS** (299 кадров; golden `build-cancel/`, 4 PNG) |

Про `victory.qa` — **осознанный pending** по элиминации: в языке `.qa` нет
команды «уничтожить юнит», а честный бой с ИИ занимает много ходов, поэтому
победа здесь по условию партии (`Duration`), а не по элиминации. Путь
«элиминация ИИ → победа» покрыт тестом
`crates/max_app/tests/local_sp.rs::eliminating_the_ai_wins_and_the_game_over_screen_renders`
(слой 1). Слой 2 закроет его, когда в языке появится команда урона/разрушения.

## P4: soak и производительность (`soak-100.qa`, `soak_headless`)

Два независимых прогона P4 (`max-brh-docs/development/PLAN-production.md`):

1. **QA-сценарий** `soak-100.qa` — тот же UI-путь игрока (100 × `end_turn` с
   ходами ИИ), поэтому он ловит паники/десинк экранов и заодно пишет
   `# mem start`/`# mem end` в `log.txt` (RSS процесса до/после), а команда
   `mem` — промежуточные замеры каждые 20 ходов.
2. **Headless-пример** `soak_headless` — 120 ходов ИИ×ИИ без UI:

   ```sh
   cd max-brh
   cargo run -q -p max_app --no-default-features --example soak_headless -- \
       --turns 120 --out qa-out/soak-headless
   # qa-out/soak-headless/soak.txt: строка на ход (wall/frames/units/rss),
   # SOAK RESULT, PROFILE, MEMORY
   ```

   Опции: `--turns`, `--out`, `--ai 1..3`, `--world`, `--seed`, `--opponent`,
   `--dt-ms`, `--data-dir/--assets-dir`, `--quiet`. Сложность по умолчанию 0
   (Clueless), условие победы Duration 200 — иначе партия закрылась бы
   лимитом раньше soak-окна.

**Профиль тика.** Переменная `MAX_TICK_PROFILE=1` включает замеры в
`max_app::profile`: время каждого `App::local_update` (кадр) и каждого
`GameState::finish_turn` (закрытие раунда). Итоговая строка пишется в
`log.txt` QA и в `soak.txt`:

```
PROFILE: local_update n=… avg=…us max=…us over_16.67ms=… (…%);
         finish_turn n=… avg=…us max=… last=…@turn100 max_at=turn…
```

Бюджет кадра 60 Гц — 16 666 667 нс (`profile::FRAME_BUDGET_NS`); счётчик
`over_16.67ms` показывает, сколько кадров в него не уложились. Замер не меняет
порядок/арифметику симуляции: `Instant::now()` стоит вокруг тех же вызовов,
CRC/снапшоты не затрагиваются.

**Память.** `max_app::mem` читает RSS и пик: Linux — `/proc/self/status`
(`VmRSS`/`VmHWM`), Windows — `GetProcessMemoryInfo`; на прочих ОС
`rss=unavailable`. `soak_headless` печатает рост RSS и считает его стабильным
при `growth ≤ 64 МиБ` (порог от грубых утечек; реальный рост за 120 ходов —
единицы МиБ). Цифры прогона P4 (2026-11-16, `rust:1.94.0-bookworm`, debug):

| Прогон | Ходы | Кадры | Время | RSS старт → конец | Профиль кадра |
|---|---|---|---|---|---|
| `soak-100.qa` | 100/100 | 12 208 | ≈3.5 c (12208 × avg 289 µs) | 54 696 → 60 944 КиБ (+6.1 МиБ, плато после 20-го хода) | avg 289 µs, max 15.69 мс, over 16.67 мс = 0 |
| `soak_headless` (debug) | 120/120 | 362 502 | 21.8 c | 53 548 → 53 708 КиБ (+160 КиБ, peak 53 884) | avg 59.9 µs; 1 выброс 121.9 мс (планировщик, 1 кадр из 362 502) |
| `soak_headless` (`--release`) | 120/120 | 362 502 | 2.47 c | 51 544 → 51 824 КиБ (+280 КиБ) | avg **6.76 µs**, max 5.21 мс, over 16.67 мс = **0** |

`finish_turn` на 100-м ходу QA-партии — 23.7 µs (avg 23.6 µs, max 56.6 µs
на 48-м ходу); в релизной сборке headless — avg 58 µs, max 5.19 мс (24-й ход),
на 120-м ходу 9.4 µs. Память QA-партии растёт только на прогреве аллокатора в
первые 20 ходов, дальше — плато, то есть линейного роста нет. Бюджет кадра
60 Гц (16.67 мс) в релизной сборке не превышен ни на одном кадре из 362 502.

**Оптимизация без изменения детерминизма.** `App::update_visual_units`
(интерполяция кадра) больше не собирает `HashMap` заново каждый кадр, а
обновляет её на месте (`retain` + `insert`); визуальные позиции не входят в
CRC/снапшот, порядок симуляции не меняется. Симуляция и `finish_turn` не
тронуты (только замеры).

## Golden-кадры (слой 3)

Эталоны — `../max-brh-qa/golden/*.png`, ровно те кадры, которые снимает
`visual-screens.qa` (сравнение по имени файла: `golden/NAME.png` ↔
`<out>/NAME.png`). Порог дрейфа (как в `max-brh-docs/qa-plan.md`):

* пиксель «изменился», если `max(|ΔR|, |ΔG|, |ΔB|) > 30`;
* кадр «дрейфнул», если изменилось больше **1 %** пикселей (3072 из 307 200);
* для каждого дрейфнувшего кадра `qa_golden_check` пишет `diff-NAME.png`
  (белый фон, красные — разошедшиеся пиксели) и завершается кодом 1 со
  списком файлов.

Формат хранения: PNG, который пишет `qa::png` (свой кодек без внешних
крейтов). Кадры M.A.X. — 8-битная палитровая графика, поэтому при ≤256 цветах
пишется индексированный PNG (1 байт/пиксель + `PLTE`), иначе truecolor.
Итог: **8 эталонов ≈ 865 КиБ** (сырые кадры были бы ~7 МБ), самый крупный —
`game.png` (~183 КиБ, 257 цветов: палитра хрома + цвет текста статуса).

### Перегенерация эталонов (после осознанного визуального изменения)

```sh
cd max-brh

# 1. снять кадры скриптом
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/visual-screens.qa qa-out/visual-screens

# 2. посмотреть, что изменилось (без --update эталоны не трогаются)
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden
#    → ДРЕЙФ name.png: N пикселей (M %), diff qa-out/visual-screens/diff-name.png

# 3. глазами проверить diff-*.png: изменение ожидаемое?

# 4. зафиксировать новые эталоны и закоммитить их вместе с изменением
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden --update
git add ../max-brh-qa/golden && git commit -m "..."
```

Правило: эталоны **не** обновляются «чтобы CI стал зелёным». Сначала
объясняется дрейф (баг это или осознанное изменение), потом обновляются
эталоны — в том же коммите, что и изменение рендера.

Опции `qa_golden_check`: `--threshold N` (порог по каналу, по умолчанию 30),
`--ratio R` (доля пикселей, по умолчанию 0.01), `--diff-dir DIR`, `--update`.

## CI (слой 4)

`ci.sh` (Docker `rust:1.94.0-bookworm`) содержит стадии `1/4` (cargo
check/test), `2/4` (Horizon), `3/4` — QA-сценарии и golden, `4/4` — soak:

```sh
# 3/4: сценарии + golden
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-full.qa qa-out/sp-full
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/hs-2.qa qa-out/hs-2
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/visual-screens.qa qa-out/visual-screens
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/victory.qa qa-out/victory
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/production.qa qa-out/production
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/save-load.qa qa-out/save-load
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/edge-cases.qa qa-out/edge-cases
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/move-input.qa qa-out/move-input
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/unit-menu.qa qa-out/unit-menu
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/allocate.qa qa-out/allocate
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/xfer.qa qa-out/xfer
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/preferences.qa qa-out/preferences
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/combat-tb.qa qa-out/combat-tb
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/missile-line.qa qa-out/missile-line
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/build-cancel.qa qa-out/build-cancel
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/production ../max-brh-qa/golden/production
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/move-input ../max-brh-qa/golden/move-input
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/unit-menu ../max-brh-qa/golden/unit-menu
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/allocate ../max-brh-qa/golden/allocate
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/xfer ../max-brh-qa/golden/xfer
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/preferences ../max-brh-qa/golden/preferences
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/combat-tb ../max-brh-qa/golden/combat-tb
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/missile-line ../max-brh-qa/golden/missile-line
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/build-cancel ../max-brh-qa/golden/build-cancel

# 4/4: soak (100 ходов QA + 120 ходов headless; профиль и память в лог)
MAX_TICK_PROFILE=1 cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/soak-100.qa qa-out/soak-100
cargo run -q -p max_app --no-default-features --example soak_headless -- --turns 120 --out qa-out/soak-headless
```

`set -e` + exit 1 у `qa_run`/`soak_headless`/`qa_golden_check` = красный CI.
Каталог `qa-out/` (логи, кадры, diff, `*.mxs`, `soak.txt`) в git не попадает.
В GitHub Actions `soak` — отдельный job (`timeout-minutes: 45`, не на PR); при
падении стадии логи и кадры уходят артефактом `qa-artifacts`.

## Тесты драйвера (слой 1 для слоя 2)

* `src/qa/script.rs` (unit) — парсер: все команды, операторы, координаты,
  ошибки с номером строки;
* `src/qa/png.rs` (unit) — PNG round-trip, палитровый и truecolor режимы,
  CRC/Adler, компаратор;
* `src/qa/runner.rs` (unit) — конфиг, клиент лобби, имена экранов/фаз;
* `tests/qa_driver.rs` (integration) — все команды парсера, прогон
  `../max-brh-qa/scripts/sp-full.qa` целиком, падение `assert` → `passed=false` (в CLI → exit 1),
  `load` V70-сейва, PNG/golden round-trip.

```sh
cd max-brh && cargo test -p max_app --lib qa:: && cargo test -p max_app --test qa_driver
```

## Ограничения

* **`load` понимает только V70** (`*.CAM`): у контейнера `MXLS` (команда
  `save`) нет десериализатора состояния — см. `max-brh-docs/plan-modes.md`; `save`
  проверяется round-trip'ом в `tests/local_sp.rs` и здесь пишется как артефакт.
* **Сеть не поднимается**: `menu join` использует заглушку клиента лобби
  (`QaMenuClient`, 2 детерминированные игры) — слой 4 (`live-smoke.sh`)
  остаётся отдельным релизным гейтом.
* **PNG-декодер** понимает `stored` и `fixed Huffman` блоки deflate — то, что
  пишет `qa::png`. Динамический Huffman (вывод `libpng`/`optipng`) отклоняется:
  эталоны перегенерируются только через `qa_run`, не сторонними утилитами.
* **Элиминация ИИ** скриптом недостижима (нет команды урона) — `victory.qa`
  побеждает по условию партии, см. выше.
* Ходы ИИ прокручиваются рамками кадров (`App::local_update(0.0)`), поэтому
  прогон детерминирован: два прогона `visual-screens.qa` дают 0 отличающихся
  пикселей (проверено).

## Как добавить сценарий

1. Создать `../max-brh-qa/scripts/NAME.qa` (формат выше), координаты брать из кадров
   (`shot` + просмотр PNG).
2. Прогнать локально: `cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/NAME.qa qa-out/NAME`.
3. Если сценарий проверяет экран, который должен быть в golden — добавить
   `shot` в `visual-screens.qa` (не в новый файл), перегенерировать эталоны.
4. Добавить прогон в `ci.sh` (стадия 3/4 — обычные сценарии, 4/4 — soak) и
   `.github/workflows/ci.yml` (job `qa` / `soak`).
