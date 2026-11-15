# План: три режима игры с визуальным паритетом 1:1 (v1)

Версия: 1.1 · 2026-11-15 · Цель: `goal-094b619d`.

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

### M-HS — Hot-seat (2–4 игрока на одном ПК)
Локальная партия: 2–4 человеческие команды, ходы по очереди.
- Настройка: выбор числа игроков/команд (те же экраны, что M-SP);
  `ChoosePlayerMenu` уже поддерживает несколько `Player`-слотов (сейчас
  `LocalSetup::validate` ограничивает M-SP одним человеком — снять).
- Цикл: активная команда управляется, при передаче хода — экран перехода
  (как в оригинале: сообщение «Ход игрока N»? — сверить с C++ hotseat-логикой),
  ввод переключается на следующего игрока.
- Критерий: партия 2 человек hot-seat до победы; визуал 1:1.

### M-NET — Сетевая игра (Horizon)
Уже работает (2 клиента × 10 ходов, UI-партия 6 ходов). Доводки:
- Одиночный сетевой лобби: снять `min_players >= 2` ограничение Horizon для
  игр с ИИ-слотами (1 человек + ИИ через сеть — если нужно; иначе оставить 2+).
- Согласование data_hash по умолчанию (без `--handshake-patches`).
- Ретрай при Create game (сессия греется 10–15 с).
- Критерий: существующий smoke 2×10 ходов + партия в окне без обходных флагов.

## Порядок

M-SP ✅ → M-HS → M-NET-доводки. Каждый этап: коммит + тесты +
скриншот-свидетельство.

## Общее для всех режимов

- Визуал: все экраны по C++-ресурсам; регресс-сверка кадров с эталонами C++-порта
  (`build/run/dbg.png`, `build/smoke-window.png`).
- Тесты: max-rust весь workspace зелёный; смоук-скрипты повторяемы.
- Артефакт: Windows UI + Linux пересобираются после каждого этапа.
