# План: три режима игры с визуальным паритетом 1:1 (v1)

Версия: 1.3 · 2026-11-15 · Цель: `goal-094b619d`.

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
| Players (`CHOSPLYR`) | `chooseplayermenu.cpp/.hpp` | 4 слота × Human/Computer/None (`CH_HUM/CH_CMP/CH_NON`), флаги команд `CH_TM1..4` + эмблема выбранного клана `CLN0..8LOGO` в (41,40), Cancel/?/Done |
| Clan (`CLANSEL`) | `clanselectmenu.cpp/.hpp` | 8 карточек `CH_CN1..8_U/D` 91×86, логотипы `CLN1..8LOGO`, имена, Random/Cancel/?/Done; выбор пишется в `team_clans` |

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

**Тесты**: `max_app/tests/local_sp.rs` (14) — настройка из UI → `team_types`,
5 ходов против ИИ (чередование, `Ai_BeginTurn`, задачи/приказы ИИ), таймер,
пауза, победа при элиминации ИИ + отрисовка экрана победы без паник,
round-trip `MXLS` после хода, загрузка V70 в локальную партию;
`max_app/tests/start_phases.rs` (10) — мобилизация, выбор точки высадки,
порядок фаз hot-seat, ИИ-точки, детерминизм.

**Кадры**: `cargo run -p max_app --no-default-features --example sp_frames -- ../max-original ../build`
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
* Производство (`ProgressBuildState`: `SpawnNewUnits`, `ResearchMenu_NewTurn`)
  портировано: P2 — очередь `ProductionManager`/спавн, P6 — исследования
  (`max_core::research`, питание Research Center) и покупные апгрейды
  (`UpgradeMenu`).

### M-SP+: фазы старта — мобилизация и выбор точки высадки — **сделано (v1.4, 2026-11-15)**

В C++ после `ChoosePlayerMenu` партия не начинается сразу:
`menu_custom_game_menu` (`menu.cpp:2166`) вызывает
`GameManager_GameLoop(GAME_STATE_6_GAME_SETUP)`, а `GameManager_InitGame`
(`game_manager.cpp:3056`) для каждой человеческой команды **в порядке убывания
слота** (`for (i = PLAYER_TEAM_MAX - 1; i >= PLAYER_TEAM_RED; --i)`) делает:

1. `GameManager_CargoSelection(team)` → `CargoMenu` (`cargomenu.cpp`) —
   мобилизация: покупка стартовых юнитов за `start_gold` (только если
   `team_gold != 0`; `team_gold = start_gold + Clans::GetCredits(clan)`, у всех
   кланов `SC_CLANS` кредиты = 0);
2. `GameManager_SelectSite(team)` → `GAME_STATE_7_SITE_SELECT` — карта 112×112
   на зуме 4 (клетка 4 px), клик выбирает точку, `Enter`/валидный клик —
   подтвердить, `Esc` — отмена; `GameManager_IsValidStartingPosition` требует
   4×4 чистой суши;
3. `GameManager_HandleProximityOverlaps` — проверка близости высадок.

Затем компьютерные команды выбирают точки сами (`Ai_SelectStartingPosition`:
5×5 суши, ≥ `AI_SAFE_ENEMY_DISTANCE` от уже выбравших), и только после этого
`GameManager_PopulateMapWithResources` генерирует ресурсы, а
`GameManager_ProcessTeamMissionSupplyUnits` высаживает армии.

Порт: `max_app/src/pregame.rs` (очередь фаз), `mobilize.rs` (CargoMenu),
`landing.rs` (SelectSite); результаты уходят в `LocalSetup::start_positions` /
`team_loadouts` / `team_gold` → `NewGameConfig::deployments` →
`deploy_starting_units_with`. ИИ-точки — `setup::ai_choose_starting_position`.
Подробности и упрощения: `docs/analysis/visual-1to1-status.md` §11.

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

**Кадры**: `cargo run -p max_app --no-default-features --example sp_frames -- ../max-original ../build`
→ `build/hs-players.png` (Hot Seat Menu, 2 человека + ИИ),
`build/hs-turn.png` (окно «Red Team: Begin turn.»), `build/hs-turn2.png`
(окно GREEN), `build/hs-game.png` (ход второго человека), `build/hs-ai.png`
(окно RED после хода компьютера).

**Открытые вопросы M-HS**

* Экран `Hot Seat Scenarios` (`MISSION_CATEGORY_MULTI_PLAYER_SCENARIO`) и
  `Load Hot Seat Game` (`SaveLoadMenu_MenuLoop(MISSION_CATEGORY_HOT_SEAT)`) не
  портированы — кнопки пишут статус.
* Кланы (`ClanSelectMenu`) реализованы: 8 карточек с логотипами, Random/
  Cancel/?/Done, выбранный клан пишется в `LocalSetup::team_clans` и
  применяется к стартовым атрибутам юнитов (`NewGameConfig::clans`).
  Полные описания/таблица бонусов из `SC_CLANS`/`clans.json` пока не
  загружаются — на экране имя и подсказка.
* Полная запись V70 для hot-seat не появилась (общий пункт M-SP).

### M-NET — Сетевая игра (Horizon) — работает; доводки **сделано (v1.3)**

Уже работает: 2 клиента × 10 ходов headless (`live-smoke.sh`) и партия в окне
(6 ходов, приказы `38`, чат `18`, барьеры `45/52/53/54` без таймаутов).
Доводки M-NET (v1.3) убрали два UX-обхода, без которых Create не доходил до
игры:

**1. Ретрай первого подключения (`Create game → Join → SessionHandle::start`).**
Сессия headless греется 10–15 с, и первый `ENet connect` падал с
`connect timed out after 10s`. Теперь `max_app/src/net.rs`:
`JoinRetryPolicy { attempts, timeout, budget }` и `retry_join` — те же
принципы, что у `ReconnectPolicy` (транспорт + resume-54, см.
`protocol.md` §9), но **для первого** подключения:

* задержки `1, 2, 3, 5, 8, 11` с (`JOIN_RETRY_BACKOFF`), окно 30 с;
* таймаут одной попытки — 4 с (ENet connect + `TP 00`), по умолчанию
  6 попыток; `--join-retry-attempts N` / `--join-retry-timeout S`
  (только `--connect`/`--ui`; `0` попыток = одна попытка без ретраев);
* статус в окне: `Session warming up — attempt N/M` через
  `FrameSink::join_status`; в логе — `join: сессия разогревается — попытка N`;
* `CreateForm`: `create_game` → сразу `InSession` с ретраем, в список игр не
  возвращает; `Horizon JoinGame` тоже повторяется, если сервер ещё не
  зарегистрировал игру (`join_horizon_with_retry`).

**2. `data_hash` по умолчанию (без `--handshake-patches`).**
Сервер без `PATCHES.RES` в `game_data` считает хэш как
`SHA256(MAX.RES) + ""` (64 hex). Клиент без явного флага
(`handshake_data_hashes`) пробует по порядку:

1. полный хэш `SHA256(MAX.RES) ‖ SHA256(PATCHES.RES)` (128 hex);
2. после `TP 03 reason=GameData` — один раз переподключается с хэшем только
   `MAX.RES` (64 hex), **тикет тот же** (повторного `JoinGame` нет —
   `start_with_hash_fallback`);
3. повторный отказ `GameData` или отказ другой причины (`Version`,
   `SessionTicket`, `Unknown`) — ошибка; для `GameData` с подсказкой
   `--handshake-patches <PATCHES.RES сервера>`, без fallback.

Явный `--handshake-patches` сохраняет старое поведение: ровно один вариант
хэша, fallback выключен.

**Тесты** (`max_app/src/net.rs`, 7 модульных + UI-тест):

* `join_backoff_is_one_two_three_five_eight_eleven` — расписание и `0`;
* `join_retry_succeeds_on_the_second_attempt_after_backoff` — мок-коннектор:
  1-я попытка падает, 2-я проходит; `start_paused` фиксирует ровно 1 с
  backoff;
* `join_retry_attempt_timeout_is_bounded_by_the_policy` — тайм-контроль:
  2 попытки по 2 с + 1 с backoff = 5 с, без зависания;
* `game_data_reject_falls_back_to_the_max_only_hash` — мок отвергает полный
  хэш, принимает MAX-only; сессия стартует, MAX-часть та же;
* `version_reject_is_neither_retried_nor_fallen_back` — другой `TP 03` не
  запускает fallback;
* `handshake_hashes_try_full_then_max_only_by_default` — варианты хэша,
  явный флаг и отсутствующий локальный `PATCHES.RES`;
* `join_progress_status_names_the_attempt_and_the_fallback` — строка статуса;
* UI: `ui::tests::create_form_creates_a_game_and_enters_the_session_with_mock_client`
  — Create сразу `InSession`, `CreateGame` один раз, `ListGames` не
  повторяется.

**Живой smoke** — `bash build/scripts/live-smoke.sh 10` **без**
`--handshake-patches`: оба клиента получают `TP 03 GameData`, переподключаются
с 64-символьным хэшем и проходят 10 ходов; скрипт проверяет
`data_hash=<hex>(64)` и отсутствие `Session failed` в `a.log`/`b.log`.

Открытые вопросы: spectator (нет `TEAM_TYPE_SPECTATOR`/снапшота) и полный
SNAP для реконнекта — см. `protocol.md` §9.2/§12. Реконнект в идущую партию
реализован через ENet+`TP 00`, продолжение lockstep и resume-54 (без SNAP:
54 покрывает юнитов/экономику своей команды). `--handshake-patches` остаётся
аварийным override'ом. Три клиента проверены `live-smoke-3.sh`.

## Порядок

M-SP ✅ → M-HS ✅ → M-NET-доводки ✅. Каждый этап: коммит + тесты +
скриншот-свидетельство.

## Общее для всех режимов

- Визуал: все экраны по C++-ресурсам; регресс-сверка кадров с эталонами C++-порта
  (`build/run/dbg.png`, `build/smoke-window.png`).
- Тесты: max-rust весь workspace зелёный; смоук-скрипты повторяемы.
- Артефакт: Windows UI + Linux пересобираются после каждого этапа.
