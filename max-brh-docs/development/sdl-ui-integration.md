# План: подключение SDL-рендеринга/UI к игре (поверх фасада)

Версия: 0.1 (проект Архитектора) · 2026-11-24.

## 0. Итог оценки

Фасад `GameUniverse` подключён **правильно и почти полностью**: рендер читает состояние
только через `StateRef`, сетевой цикл замкнут (`ввод→submit→tick→drain_outgoing→render`),
локальная партия идёт через локальную машину фасада, `GameState` не выходит за фасад.
Оставшиеся разрывы — **«подключено через второй, широкий канал»**, а не «не подключено»:
UI-мутации меню пишут в `&mut universe` напрямую, локальный/сетевой циклы — два разных
скелета (`online: bool`), «издать приказ» идёт разными путями (`apply_now` vs `submit`).

## 1. Статус компонентов

| Компонент | Статус |
|---|---|
| Рендер карты/юнитов (`render.rs`) | ✅ читает только `state_ref()` |
| HUD/панели/оверлеи/курсор | ✅ читают `state_ref()` |
| `Screen::render`/`render_session` | ✅ |
| События/окно SDL (`run_ui`) | ✅ |
| Меню→локальная/сетевая партия | ✅ |
| Сетевой игровой цикл | ✅ замкнут |
| Локальный игровой цикл | ⚠️ замкнут, но вторым каналом (прямой `universe.tick`, приказы `apply_now`) |
| Ввод в игру | ⚠️ два пути (`online=false` → `apply_now`; `online=true` → `submit`) |
| End Turn | ⚠️ локальный ✅; demo-путь прямой `set_game_state`+`finish_turn` |
| Меню-мутации | ⚠️ `UpgradeMenu`/`ResearchMenu` — пакетами 09/10 (командный канал); alloc/parent/auto_survey/build_time — аудированные исключения (отдельный трек, §3 B2-таблица) |
| Аудио | ✅ из событий фасада |
| Рекордер/QA | ✅ |
| Детерминизм (digest/golden) | ✅ |

## 2. Ключевые «разобщённые» места

1. **Широкий публичный канал фасада** (`apply_now`+`set_*`+`*_mut`) + `App.universe` = `pub`.
2. **UI-мутации мимо командного канала** (меню-коммиты) — для `UpgradeMenu` (золото +
   `gold_spent_on_upgrades`) и `ResearchMenu` (распределение исследований) **закрыто
   packet 09**; остальные пути аудированы и задокументированы в §3 (B2-таблица).
3. **`end_turn` demo-путь** — прямой `set_game_state`+`finish_turn`.
4. **Два скелета цикла** (локальный vs сетевой) + `online: bool`.
5. **Конфигурация мира** — легитимный install-путь, но не отделён от UI-пути.

## 3. План (решение владельца 2026-11-24: **A→B→C1/C2→D**; C3 — отдельный трек после D)

### Stage A — Базовая линия и границы
- **A1 Baseline-гейт:** `--demo`, `--new-game`, `qa_run`, `qa_golden_check`, `--connect --sdl`, `soak` — зафиксировать digests/golden.
- **A2 Guardrail на чтение рендера:** инвариант «рендер/HUD/панели не берут `&mut`» (тест/lint).
- **A3 Канонизировать контракт фасада:** два канала — lockstep (`submit/tick/drain_outgoing/submit_remote`) и локальная машина (`apply_now`/`begin_local_round`/`resume_local_round`/`end_local_turn`/`tick`); UI — только командный канал, install — только узкий install-API.

### Stage B — Убрать прямые мутации UI (узкий командный канал)
- **B1** `end_turn` demo-путь → **`GameUniverse::end_local_turn(team)`** (проверка решения 1,
  НЕ `submit(06)` — отложенный 06 снова войдёт в EndTurn).
- **B2** меню-коммиты → генерация `Command` в `hud.pending`; **grep по всем
  `*_mut`/`set_*`/`apply_now`/`state_mut` в UI-слое** — каждый найденный либо переведён
  на командный канал, либо явно задокументирован «install-only, не мутирует симуляцию».
- **B3** централизовать локальный `tick` (единственный `App::tick`); **гейт эквивалентности
  `apply_now` vs `submit`+`tick`**: для каждого пути, где был `apply_now`, прогнать N+1
  тиков и сравнить `snapshot_bytes` — расхождение = баг классификации команд, не «сдвиг».
- Гейты: QA-скрипты `alloc/xfer/upgrade/research/unitmenu` + golden без изменений; digest стабилен.
- **Требование детерминизма (не гигиена):** B закрывает **все** пути мутации мимо
  `submit`/`apply_now` — не только видимые в меню. Если после B останется хоть один путь
  мутации вне командного канала, детерминизм не гарантирован.
- **Вне B:** C1/C2 (структура цикла), `handle_session_input` (протокол), C3 (install-API).

#### B2 — статус аудита (2026-11-24, PACKET 09)

Packet **09** (состояние команды: `gold`, `gold_spent_on_upgrades`,
`research_topics`) добавлен как `Command::TeamState` — broadcast, бит-в-байт по
оракулу (`remote.cpp:2067/2088`, раскладка в
[`../architecture/client/protocol.md`](../architecture/client/protocol.md) §8.4).
Пп.1–2 закрыты, остальные пути проклассифицированы:

| № | Путь | Где | Статус |
|---|---|---|---|
| 1 | `UpgradeMenu` commit: `gold` + `gold_spent_on_upgrades` | `App::commit_upgrade_menu` | **закрыт** — `Command::TeamState` (09) |
| 2 | `ResearchMenu` commit: `research_topics[].allocation` + пересборка центров | `App::commit_research_menu` | **закрыт** — `Command::TeamState` (09) |
| 3 | `AllocMenu` per-click: добыча/сдача комплекса, `balance`, `complex_mut` | `allocmenu.rs:288`, `App::optimize_allocation`/`reduce_production_allocation`/`allocation_balance` | **отдельный трек**: носителя нет — 11 несёт только итоги комплекса, 08/P5 — четыре поля шахты, у `Balance` пакета нет; C++ пишет так же напрямую (`allocmenu.cpp:790-792`) |
| 4 | `parent_unit` при открытии X-FER | `lib.rs:1631` | **отдельный трек**: пакета «только parent» нет (поле едет в 08/P3 при `Done`), C++ пишет при открытии окна и сохраняет после `Cancel` |
| 5 | `auto_survey` (тумблер `Auto`) | `lib.rs:1909` | **отдельный трек**: поля нет в 08 (только V70 save), C++ тоже переключает локально |
| 6 | `build_time` (обломки `ORDER_CLEAR`) | `lib.rs:2014` | **отдельный трек**: `build_time` есть только в P4, а `clear` собирает P3 |
| 7 | `enemy_halt` | `preferences.rs:1897` | **install/preferences**: не входит в `snapshot_bytes`, пишется из `Preferences` |
| 8 | save meta (V70 header/preferences/camera) | `saveload.rs:950` | **не симуляция**: заголовок сейва, вне snapshot/CRC |
| 9 | RNG-ролл `stealth_dice_roll` | `ui.rs:3199` | **не мутация**: значение едет в 08/P3, RNG расходуется как в C++ |
| 10 | install мира (карта/ресурсы/стартовые юниты, `--new-game`, load) | `local.rs:693-706`, `net.rs:2142`, `App::load_save_bytes` | **install-API**, не UI (Stage C3) |

Гейты packet 09:

- раскладка бит-в-байт — `max_net/src/wire/mod.rs::packet_09_matches_the_oracle_byte_layout`;
- класс маршрута (broadcast, не local) —
  `net/dispatcher.rs::dispatcher_routes_simulation_broadcast_barriers_local_and_seed_drop`;
- семантика применения (золото/траты/пересборка/выключение центров) —
  `max_core/tests/team_state.rs`;
- **гейт эквивалентности** «одиночная игра == broadcast → пир» —
  `max_app/tests/channel_equivalence.rs::upgrade_menu_commit_matches_the_broadcast_apply_on_a_peer`
  и `research_menu_commit_matches_the_broadcast_apply_on_a_peer` (сравнение
  `snapshot_bytes`).

### Stage C — Единый скелет цикла (C1/C2)
- **C1** общий шаг сессии `poll input → приказ → tick → render` (dispatcher-generic).
- **C2** убрать `online: bool` (capability из dispatcher).
- Гейты: локальная/сетевая/хост-партия один скелет; `PacketHistogram`/`barrier_timeouts`/`desyncs`/`digest` без изменений.
- **C2 — обязательный гейт перед C3** (иначе C3 меняет контракт в двух местах одновременно).

### Stage D — Детерминизм и регрессия (сетка безопасности, ДО C3)
- **D1** эквивалентность каналов тестом (`apply_now` ≡ `submit` для одной симуляции).
- **D2** golden-регрессия.
- **D3** CI/soak.
- **DoD:** меню → партия → ходы → рендер → ввод интерактивно играбельны в SDL-окне;
  рендер читает только `StateRef`; UI пишет только командами; единый цикл; golden/qa/digest зелёные.

### Stage C3 — узкий install-API (**отдельный трек**, после D)
> Отдельное ТЗ с собственными гейтами; приёмка A→B→C1/C2→D **не зависит от C3**.
- `App.universe` → `pub(crate)`; install через `install_world`/`adopt_universe`.
- **`handle_session_input` — вне объёма C3** (изменение протокола требует отдельного
  решения владельца).
- **Риск C3:** меняет публичную поверхность `App.universe`; QA-раннер и сетевой драйвер
  могут потребовать правок; регрессия ловится golden/CI (стадия D), а не вручную.

## 4. Вне блока (не трогаем)

- MVE-плеер (C3/C4), музыка (D2) — медиа, не потребители состояния (в очереди 1.4.1).
- Анимации (C1/C2) — визуально-нейтральны к CRC.
- debug-Preferences (B6) — чисто UI-настройки, вне снапшота.

## 5. Риск для владельца

Stage C2/C3 меняет публичную поверхность `App.universe` и протокол `handle_session_input`
(трогает сетевой драйвер + QA-раннер) — обязательны гейты D1/D3. Если узкий install-API
рискован на текущем темпе — C3 можно вынести отдельным треком после C1/C2.
