# План: Кампания + одиночные миссии (сценарии)

Версия 0.1 (проект Аналитика) · 2026-11-24.

## TL;DR

Симуляционная часть миссий **уже готова** (V70-читатель, `mission_category`/`mission_index`,
`mission_win_loss`, `start_local_game_from_save`). Отсутствует **только слой подачи**:
реестр дескрипторов миссий (JSON из `PATCHES.RES`), меню выбора, подменю New Game,
прогрессия кампании, экраны брифинга/истории. **~4 этапа, ~70% — UI-порт + 1 реестр.**

## Что уже есть vs отсутствует

**Есть:** `MissionCategory` (8), V70-парсер + `GameUniverse::from_save`,
`mission_category`/`mission_index` в `GameState`, win/loss уникальных миссий
(`victory.rs::mission_win_loss`), экран победы/поражения, `start_local_game_from_save`,
расширения сейвов cam/sce/tra/mps, `Packet12MissionSupply`+landing.

**Отсутствует:**

| # | Что | Оракул |
|---|---|---|
| M1 | Реестр миссий (JSON `SC_*` → XOR → разбор по категориям) | `mission.cpp:425-455`, `missionregistry.cpp:28-82` |
| M2 | Подменю New Game (7 кнопок) | `menu.cpp:2171` |
| M3 | Экран выбора миссии (`GameSetupMenu`) | `gamesetupmenu.cpp` |
| M4 | Поток старта миссии (дескриптор → SAVE*.cam → `start_local_game_from_save`) | `menu.cpp:2052-2098` |
| M5 | Прогрессия кампании (победа → следующая миссия) | `menu.cpp:518-545` |
| M6 | Экраны истории (intro/victory/defeat) | `menu.cpp:766` |
| M7 | (условно) game-rules buildability | `gameruleshandler.cpp:126-175` |

## Формат миссий

Миры миссий = **обычные V70-сейвы** (`SAVE1.CAM` → заголовок 0x46=70). Дескриптор =
JSON (XOR-обфусцирован) в `PATCHES.RES` (`SC_C0001…SC_D0003`): title/description/
hash/mission-file/winloss_conditions(LUA)/game_rules(LUA)/story. Rust не встраивает
LUA — win/loss уже захардкожено; game-rules — по категории/индексу при необходимости.

| Категория | Ресурсы | Сейвы | Кол-во |
|---|---|---|---|
| Campaign | SC_C0001–9 | SAVE1..9.CAM | 9 |
| Scenario | SC_S0001–24 | SAVE1..24.SCE | 24 |
| Training | SC_T0001–15 | SAVE1..15.TRA | 15 |
| MultiPlayerScenario | SC_M0001–12 | SAVE1..12.MPS | 12 |
| Demo | SC_D0001–3 | SAVE1.DMO | 1–3 |

## План (4 этапа с гейтами)

- **Этап 0 — реестр миссий** (без UI): `MissionDescriptor`+`MissionRegistry`,
  XOR-декод `SC_*`, разбор JSON, разложение по категориям. Гейт: `len(Campaign)==9`,
  `[0].title=="Islands in the Sun"`.
- **Этап 1 — поток старта миссии** (headless): реестр → `SAVE*.cam` → `start_local_game_from_save`.
  Гейт: SAVE1.CAM 5 тиков детерминизм; победа Campaign 1 по условиям.
- **Этап 2 — меню**: подменю New Game (7 кнопок) + `GameSetupMenu` (список+описание+Start).
  Гейт: кадры выбора (golden).
- **Этап 3 — прогрессия + стори-экраны**: победа → следующая миссия (`last_campaign`),
  экраны intro/victory/defeat. Гейт: end-to-end Campaign 1→2.
- **Этап 4 (условный) — game-rules buildability** по категории/индексу.

## Риски

1. Порядок проверок win/loss для всех миссий — перепроверить таблицей.
2. Стори-экраны ссылаются на фон (MAX.RES) и видео (.MVE) — подключить.
3. `hash`-верификация — warning, не блокер.
4. Прогрессия — нужен персистентный settings (`last_campaign`/`game_file_number`).
5. Hot Seat Scenarios / MultiPlayerScenario — вне этого объёма.
