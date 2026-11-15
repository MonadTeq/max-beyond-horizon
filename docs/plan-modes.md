# План: три режима игры с визуальным паритетом 1:1 (v1)

Версия: 1.2 · 2026-11-15 · Цель: `goal-094b619d`.

## Принцип

Клиент визуально **не отличается от оригинала** (каждый экран — по ресурсам и
раскладке C++-порта), под капотом — наша Rust-архитектура (max_core — полная
детерминированная симуляция с ИИ, max_net — сеть, max_app — UI/цикл).
Оракул: `max-cpp-port/` (не изменяется). Визуальный статус: `docs/visual-1to1-status.md`.

## Режимы

### M-SP — Одиночная игра (против ИИ) — **сделано (v1.1)**

Локальная партия: 1 человеческая команда + 1–3 компьютерные, без сети.

**Настройка 1:1** — три экрана в порядке C++ `menu_custom_game_menu`
(`menu.cpp:2115`), модуль `max_app/src/setup.rs`:

| Экран | Оракул | Что есть |
|---|---|---|
| Options (`OPTNFRM`) | `gameconfigmenu.cpp/.hpp` | все 6 панелей: сложность ИИ (6), таймеры хода/конца хода (7+7), режим игры (2), стартовые кредиты (6), уровни ресурсов (4×3), условие победы (2×3 + числовой лимит) |
| Planet (`PLANETSE`) | `planetselectmenu.cpp/.hpp` | 3×2 мини-карты планет (`.WRL` + палитра/4 + `Color_MapColor(...,true)`), стрелки групп `MNULAROU/MNURAROU`, большая картинка `SNOW_PIC+world/6`, имя и описание 24 планет, Random/Cancel/?/Done |
| Players (`CHOSPLYR`) | `chooseplayermenu.cpp/.hpp` | 4 слота × Human/Computer/None (`CH_HUM/CH_CMP/CH_NON`), флаги команд `CH_TM1..4` + эмблема клана `CLN0LOGO` в (41,40), Cancel/?/Done |

Единственное дополнение к раскладке: поле **Name** на свободной полосе
`OPTNFRM` слева от кнопок (в C++ имя задаётся в Preferences, экран ещё не
портирован).

**Локальный цикл** — `max_app/src/local.rs`, порт
`GameManager_GameLoop` + `GameManager_TeamTurnTurnBased`:

* раунд идёт по командам RED → GREEN → BLUE → GRAY, `None`/`Eliminated`
  пропускаются;
* начало хода команды — `GameState::manage_economy` (`ManageEconomy`) и
  `ProgressBuildState` в объёме max_core;
* компьютер: `Ai_BeginTurn(team)`, ход крутится тиками, пока
  `AiPlayer::CheckEndTurn` не дойдёт до `end_turn_retry_counter > 120`
  (в C++ на этом пороге `GameManager_RefreshOrders` закрывает ход) — затем
  пакет `06` и следующая команда; страховка — предел тиков;
* человек: ввод (мышь/HUD как в сети), **End Turn** → пакет `06`, таймер
  `settings["timer"]` (0 — без лимита) закрывает ход сам;
* пауза (`P`/кнопка Pause панели) останавливает тики и таймер;
* конец раунда — `GameState::finish_turn` (`GameManager_TeamTurnFinish`):
  условия победы, `turn_counter += 1`, сброс `finished_turn`.

**Экран победы/поражения** — `max_app/src/winloss.rs`, порт
`menu_draw_game_over_screen` (`menu.cpp:165`): фон `ENDGAME1..9`, заголовок
`Game Over`, колонки команд (номер, имя), места (`1st place`…`Eliminated`),
`Points`, `Factories Built`, `Mines Built`, `Buildings Built`, `Lost` (здания),
`Units Built`, `Lost` (мобильные), `Upgrades` (`N Credits`), `ENDARM` (273,458)
и `ENDOK` + `OK` (293,458); Enter/Esc/клик по OK — в меню.

**Сохранение**: V70 **читается** в локальную партию
(`App::start_local_game_from_save` → обычный локальный цикл). Запись
`GameState → V70` не реализована (в V70 объектное дерево `SaveUnit` +
`HeatMap` + `AiPlayerState` + хэши, которого в `GameState` нет в таком виде).
Быстрое сохранение локальной партии — контейнер `MXLS`
(`LocalSave`: настройки + фаза + `GameState::snapshot_bytes()` + CRC), см.
`App::local_save_bytes`/`write_local_save`/`read_local_save`.

**Тесты**: `max_app/tests/local_sp.rs` (12) — настройка из UI → `team_types`,
5 ходов против ИИ (чередование, `Ai_BeginTurn`, задачи/приказы ИИ), таймер,
пауза, победа при элиминации ИИ + отрисовка экрана победы без паник,
round-trip `MXLS` после хода, загрузка V70 в локальную партию.

**Кадры**: `cargo run -p max_app --example sp_frames -- ../max-original ../build`
→ `build/sp-options.png`, `sp-planets.png`, `sp-players.png`, `sp-game.png`,
`sp-victory.png`, `sp-defeat.png`.

**Открытые вопросы M-SP**

* `GameState → SaveFile` (полная запись V70) — отдельная задача; до неё
  «сейв» локальной партии — контейнер `MXLS` (снимок без восстановления
  симуляции) + чтение V70.
* `PlayMode::SimultaneousMoves` поддержан циклом (ходы команд идут
  последовательно, а не одновременно); для 1–3 ИИ поведение совпадает, но
  конкурентный `GameManager_TeamTurnConcurrent` (общий `GameState_9_END_TURN`
  и барьеры) не портирован 1:1.
* Кланы (`ClanSelectMenu`) и экраны Help (`PLANET_SETUP`,
  `NEW_GAME_SETUP`, `OPTIONS_SETUP`) не портированы — кнопки `?`/флаг команды
  пишут статус.
* Производство (`ProgressBuildState`: `SpawnNewUnits`,
  `ResearchMenu_NewTurn`) в max_core всё ещё P3/TODO — ИИ строит через задачи,
  но очередь производства юнитов из `ProductionManager` не спавнится.

### M-HS — Hot-seat (2–4 игрока на одном ПК) — **сделано (v1.2)**

Локальная партия: 2–4 человеческие команды, ходы по очереди. В C++ это **не
отдельный режим симуляции**, а следствие того, что людей больше одного:

* `GameManager_UpdateHumanPlayerCount` (`game_manager.cpp:3267`) считает команды
  `TEAM_TYPE_PLAYER`; при `> 1` принудительно ставит
  `GameManager_PlayMode = PLAY_MODE_TURN_BASED` (в одиночной игре счётчик
  обнуляется);
* `GameManager_TeamTurnTurnBased` (`:451`) идёт по командам RED → GREEN → BLUE →
  GRAY и на каждую вызывает `GameManager_UpdateGui(team, ...)`;
* в ветке `TEAM_TYPE_PLAYER` (`:2437`) при смене команды
  (`GameManager_PlayerTeam != team`) вызывается `GameManager_UpdateGuiControl(team)`
  (`:2262`), который показывает **модальное сообщение**
  `MessageManager_DrawMessage(sprintf(_(c2eb), menu_team_names[team]), INFO, MODAL, true)`,
  то есть `DialogMenu dialog("Red Team:\nBegin turn.", true); dialog.Run()`.
  `GameManager_UpdateTurnTimer` включается **после** возврата из диалога (`:2490`),
  поэтому таймер хода начинает идти только после подтверждения;
* после подтверждения `GameManager_PlayerTeam = team`,
  `Access_UpdateMinimapFogOfWar(team, …)`, `GameManager_UpdatePanelButtons(team)` —
  ввод, туман войны и панели переключаются на нового игрока;
* первым окно видит и RED: `GameManager_InitGame` ставит
  `GameManager_PlayerTeam = PLAYER_TEAM_ALIEN` (или первую компьютерную команду,
  `:3076`), поэтому на первом ходу `PlayerTeam != team` тоже;
* имена команд в hot-seat — `Red Team`/`Green Team`/`Blue Team`/`Gray Team`
  (`menu.cpp:1941`: `player_name` подставляется только в одиночной игре).

**Экран перехода — 1:1** (`max_app/src/hotseat.rs`):

| Элемент | Оракул | Реализация |
|---|---|---|
| фон окна | `Window(HELPFRM, GameManager_GetDialogWindowCenterMode())`, ресурс `HELPFRAM` 301×231, `dialog_center_mode = 0` → центр `WINDOW_MAIN_MAP` (180,18) 448×448 | `DIALOG_RECT = (253,126,301,231)`, `HELPFRAM` палитрой окна (`palette_from_image = false`, как в C++) |
| текст | `DialogMenu::DrawText`: `GNW_TEXT_FONT_5`, `GNW_TEXT_OUTLINE \| 0xFF`, область `(20,14)` 265×175, строки по центру, блок по центру по вертикали, шаг 10 px | `TEXT_RECT`, `hotseat::turn_message()` = `"<Команда>:\nBegin turn."` (`_(c2eb)` + `_(f394/a8a6/a3ee/319d)`) |
| кнопка | `Button(HELPOK_U, HELPOK_D, 120, 193)` 65×24, подпись `_(ae0b)` = `OK` (`Fonts_GoldColor`) | `OK_RECT`/`ok_rect_frame()`, `HELPOK_U/D` |
| закрытие | `ProcessKey`: `GNW_KB_KEY_RETURN`, `GNW_KB_KEY_ESCAPE`, клик по `OK` | Enter/Esc/клик по `OK` (`hotseat::ok_at`) |

Отличие: нет прокрутки `BLDUP`/`BLDDWN` (`row_count > 17`) — сообщение две
строки.

**Точка входа** — `Multiplayer Menu` (`menu_multiplayer_menu_loop`, `:2361`):
«Multiplayer Game» главного меню → `Host Network Game` / `Join Network Game` /
**`Hot Seat Game`** / `Hot Seat Scenarios` / `Load Hot Seat Game` / `Cancel`
(новый экран `Screen::MultiplayerMenu` в `max_app/src/ui.rs`, раскладка
1:1: `MAINPIC`, заголовок `Multiplayer Menu` в (236,145), кнопки `BLANK_*` 200×29
в x=385 y=175/210/280/315/350 и `Cancel` `SBLNK_*` 100×30 в (435,420)).
`Host Network Game` открывает существующую форму создания игры Horizon,
`Join Network Game` — список игр; сценарии и загрузка hot-seat ещё не
портированы (кнопки пишут статус).

**Настройка** (`max_app/src/setup.rs`) — те же три экрана, но
`is_single_player = false` (`GameSetupForm::hot_seat()`):

* заголовок экрана команд — `Hot Seat Menu` (`_(55c8)`), а не `Custom Game Menu`;
* «Human» на другом слоте **не** переносит флаг человека
  (`ChoosePlayerMenu::EventSelectHuman`, `:79`) — людей можно выбрать 2–4;
* стартовые значения `menu_choose_player_menu_loop` (`:1883`): RED — человек,
  GREEN — человек, BLUE/GRAY — пусто;
* `EventDone` требует двух людей: `There must be at least two human players to
  begin a hot seat game.` (`_(06ec)`); компьютеры в смеси допустимы.

**Цикл и ввод** (`max_app/src/local.rs`):

* `State.player_team` переключается на активного человека в начале его хода —
  выделение, приказы, панели, туман и курсор смотрят на него (`GameManager_PlayerTeam`);
  в начале хода выделение сбрасывается (`hud.clear_interaction`);
* при смене человеческой команды открывается `LocalGame::notice`
  (`hotseat::TurnNotice`); пока он открыт, `local_update` **не тикает и не
  крутит таймер**, а `handle_session_input` принимает только подтверждение
  (клик по `OK`/Enter/Esc) — как цикл, стоящий в `DialogMenu::Run()`;
* ход ИИ (если он есть между людьми) идёт сам, без окна: во время него приказы
  и выделение заблокированы (`LocalGame::input_blocked`), панели/миникарта
  остаются только просмотром;
* `End Turn` и таймер `settings["timer"]` работают **на каждую человеческую
  команду** отдельно (`timer_left` переинициализируется в начале хода команды);
* hot-seat всегда turn-based (`LocalSetup::effective_play_mode`), как
  `GameManager_UpdateHumanPlayerCount`;
* `mission_category = MISSION_CATEGORY_HOT_SEAT` (`HotSeat` в max_core),
  имена команд — `LocalSetup::display_name` (в hot-seat всегда `* Team`);
* `player_team` на старте партии — `PLAYER_TEAM_ALIEN`, чтобы окно увидела и
  первая команда; итог партии (`LocalOutcome::team`) — команда, на чьём ходу
  закрылся раунд, как `GameManager_PlayerTeam` в C++;
* сохранение `MXLS` — версия **2** (добавлен флаг `hot_seat`), файлы версии 1
  отклоняются.

**Тесты**: `max_app/tests/hot_seat.rs` (13) — форма hot-seat (флаг человека не
переезжает, заголовок, `_(06ec)`), принудительный turn-based, окно перехода у
первого и последующих игроков, чередование RED ↔ GREEN, ходы всех четырёх
человек по порядку, ИИ между людьми, «окно морозит таймер», «управление только
своими юнитами» (чужой не выделяется, приказы чужой команды не меняются, при
смене игрока выделение сброшено), блокировка ввода на ходу ИИ и на окне
перехода, отрисовка окна (`HELPFRAM` + кнопка `OK` + текст), `Hot Seat Menu` в
кадре 640×480 и `Multiplayer Menu`.

**Кадры**: `cargo run -p max_app --example sp_frames -- ../max-original ../build`
→ `build/hs-players.png` (Hot Seat Menu, 2 человека + ИИ),
`build/hs-turn.png` (окно «Red Team: Begin turn.»), `build/hs-turn2.png`
(окно GREEN), `build/hs-game.png` (ход второго человека), `build/hs-ai.png`
(окно RED после хода компьютера).

**Открытые вопросы M-HS**

* Экран `Hot Seat Scenarios` (`MISSION_CATEGORY_MULTI_PLAYER_SCENARIO`) и
  `Load Hot Seat Game` (`SaveLoadMenu_MenuLoop(MISSION_CATEGORY_HOT_SEAT)`) не
  портированы — кнопки пишут статус.
* Кланы (`ClanSelectMenu`) не портированы: флаг команды рисуется с `CLN0LOGO`,
  как при `player_clan = 0`.
* Полная запись V70 для hot-seat не появилась (общий пункт M-SP).

### M-NET — Сетевая игра (Horizon)
Уже работает (2 клиента × 10 ходов, UI-партия 6 ходов). Доводки:
- Одиночный сетевой лобби: снять `min_players >= 2` ограничение Horizon для
  игр с ИИ-слотами (1 человек + ИИ через сеть — если нужно; иначе оставить 2+).
- Согласование data_hash по умолчанию (без `--handshake-patches`).
- Ретрай при Create game (сессия греется 10–15 с).
- Критерий: существующий smoke 2×10 ходов + партия в окне без обходных флагов.

## Порядок

M-SP ✅ → M-HS ✅ → M-NET-доводки. Каждый этап: коммит + тесты +
скриншот-свидетельство.

## Общее для всех режимов

- Визуал: все экраны по C++-ресурсам; регресс-сверка кадров с эталонами C++-порта
  (`build/run/dbg.png`, `build/smoke-window.png`).
- Тесты: max-rust весь workspace зелёный; смоук-скрипты повторяемы.
- Артефакт: Windows UI + Linux пересобираются после каждого этапа.
