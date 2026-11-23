# ТЗ: GameUniverse — этап C (сетевая интеграция фасада, `App<D>`)

> Статус: **передано Разработчику**.
> Архитектурное решение —
> [`../architecture/client/game-universe-facade.md`](../architecture/client/game-universe-facade.md)
> (§7, решение владельца **8**; §2 — `Dispatcher`/`NetworkDispatcher`; §6 — риски).
> Предыдущие этапы — [`../development/game-universe-stage-a.md`](../development/game-universe-stage-a.md)
> (фасад поверх `GameState`) и [`../development/game-universe-stage-b.md`](../development/game-universe-stage-b.md)
> (полная инкапсуляция). Оба выполнены: `App` владеет `GameUniverse<LocalDispatcher>`,
> `NetworkDispatcher` реализован и покрыт тестами, но сетевой драйвер `net.rs` пока
> ходит через **временный мост** (`App::universe()`/`universe_mut()` + фасадные
> `step_*_barrier`/`submit_remote`/`drain_outgoing`).
>
> **Роль исполнителя:** Разработчик (DeepSeek v4 Flash). При расхождении с
> архитектурным документом или сомнении в поведении — стоп, вопрос Архитектору
> (эскалация), не домысливать. Комментарии в коде — строго на английском; `.md`
> — на русском.

## 0. Цель и границы

**Цель этапа C:** реализовать решение 8 ADR — перевести `App` на generic
`App<D: Dispatcher = LocalDispatcher>` с полем `universe: GameUniverse<D>`,
выделить локальную и сетевую поверхности, и переключить сетевой драйвер `net.rs`
с временного моста на фасадный путь, управляемый `NetworkDispatcher`.
Переключение режима = создание нового `App<D>` (ADR §7.8, мономорфизация, zero-cost).

**В этапе C делается** (ADR §7.8 + §2):

1. `App<D: Dispatcher = LocalDispatcher>`; поле `universe: GameUniverse<D>`.
2. Общая поверхность в `impl<D: Dispatcher> App<D>`: `state_ref`/`submit`/`tick`/
   `drain_outgoing`/`submit_remote`/`snapshot`/`digest` + доступ к конфиг-слою и
   `step_*_barrier` + вся UI/меню/рендер-поверхность, не зависящая от конкретного
   диспетчера.
3. Локальная поверхность в `impl App<LocalDispatcher>`: `new`/`new_game`/
   `new_game_on_surface`/`load_game`/`load_save`/`load_save_bytes`/`run_demo`/
   `end_local_turn`/`begin_local_round`/`resume_local_round`/`start_local_game*`.
4. Сетевая поверхность в `impl App<NetworkDispatcher>`: конструктор `new_network`,
   `initialize_game`, `run_game_loop` (+ `pump_events`/`handle_session_event`/
   `wait_barrier`/`broadcast_outgoing`/`send_frame_barrier`/`flush_hud_orders`/
   `send_barrier`).
5. `net.rs` строит `App<NetworkDispatcher>`; барьерный путь — только через фасад:
   `submit_remote` (00/52/45 → `ApplyBarrier`, 54 → `ApplyStateSync`, прочее →
   `Replay`) + `step_*_barrier` + `drain_outgoing` + `submit(Broadcast)` → вещание.
6. Удаление моста (`universe()`/`universe_mut()`/мост `submit_remote`/`apply`/
   `enqueue`/`flush_commands`) — **после** прохождения гейтов а/б/в.

**В этапе C НЕ делается** (guardrails, §7): изменение семантики барьерной FSM
(retransmit 3 с / timeout 30 с / resume-54), перевод `06`/`01` на `submit`,
перевод `13 Seed`/`12 MissionSupply` лобби через `NetworkDispatcher`, полевой
diff `StateDelta`, любые изменения `snapshot_bytes`/CRC.

### 0.1 Порядок и зависимости (обязательно к соблюдению)

1. **§2 (generic-рефактор, изолированный)** — `App<D>` с `D = LocalDispatcher`
   по умолчанию; поведение не меняется; мост **сохраняется** в пере-типизированном
   виде, поэтому все существующие call-sites компилируются без правок.
2. **§3 (фасадные добавки, аддитивно)** — `dispatcher()`/`dispatcher_mut()`,
   `with_setup_and_dispatcher`; тесты.
3. **§4 (сетевая поверхность + переключение драйвера)** — `App<NetworkDispatcher>`,
   перенос `net.rs`-драйвера на фасадный путь; гейты (а)/(б) здесь.
4. **§5 (удаление моста)** — только после гейтов (а)/(б).
5. **§6 (гейт (в) live-smoke + (г) детерминизм)** — полная проверка.

### 0.2 Решения Архитектора (А1–А6)

- **А1 — generic-границы borrow-checker.** FSM-методы `begin/end/crc` сплитятся на
  **шаг счётчика** (фасад `step_*_barrier`) и **чистую запись+пакет**
  (`record_begin/record_end/record_crc`), чтобы не было двойного `&mut`-заимствования
  (`self.universe.dispatcher_mut().begin(&mut self.universe)`).
- **А2 — доступ к диспетчеру.** `GameUniverse<D>` получает `dispatcher()`/
  `dispatcher_mut()` — документированная точка входа асинхронного драйвера (пара к
  `drain_outgoing`/`submit_remote`).
- **А3 — конструктор сети и общий генератор мира.** `with_setup_and_dispatcher(map,
  team_kinds, dispatcher)`; генератор мира выносится в generic `build_world`.
- **А4 — лобби остаётся локальным.** `12`/`13` не проходят через
  `NetworkDispatcher::route`; `run_lobby` собирает setup-данные в `LobbyInfo`.
- **А5 — `06`/`01` остаются немедленными** через `apply_now` + `send_untracked`,
  не через `submit`/`drain_outgoing`.
- **А6 — семантика `submit_remote`** становится пассквозом `universe.submit_remote`
  (Replay/ApplyBarrier/ApplyStateSync), заменяя немедленный мост.

## 1. Обязательное чтение перед стартом

- [`../architecture/client/game-universe-facade.md`](../architecture/client/game-universe-facade.md) — §2, §5, §7 (п. 8).
- [`../development/game-universe-stage-b.md`](../development/game-universe-stage-b.md) — §0.2, §6, §7.
- `max-brh/crates/max_core/src/universe.rs` — целиком.
- `max-brh/crates/max_app/src/lib.rs` — `App` `:544-689`, `impl App` `:812-3658`, мост `:879-943`, `new_game*` `:2822/2864`.
- `max-brh/crates/max_app/src/net.rs` — `run_network_with_sink` `:1105`, `run_lobby` `:936`, `initialize_game` `:1194`, `run_game_loop` `:1577`, `handle_session_event` `:1830`, `wait_barrier` `:1529`, `broadcast_outgoing` `:1487`, `flush_hud_orders` `:1465`.
- `max-brh/crates/max_app/src/net/dispatcher.rs` — `TurnBarrierFsm` `:79-281`, `NetworkDispatcher` `:342-554`.
- `max-brh/crates/max_app/src/local.rs` — потребитель `TurnNotice` `:828`, `local_begin_round`/`local_resume_round`/`local_close_turn`.

## 2. Шаг 1 — Generic-рефактор `App<D>` (изолированный)

> Только типизация, без смены поведения. `App` по-прежнему `App<LocalDispatcher>`
> (default). Мост пере-типизируется, не удаляется.

Файл `max-brh/crates/max_app/src/lib.rs`:

1. `pub struct App` (`:544`) → `pub struct App<D: Dispatcher = LocalDispatcher>`;
   поле `universe: GameUniverse<LocalDispatcher>` (`:551`) → `universe: GameUniverse<D>`.
2. `impl App` (`:812`) расщепляется:
   - `impl<D: Dispatcher> App<D>` — поверхность, не требующая конкретного
     диспетчера: `state_ref`, `submit`, `tick`, `drain_outgoing`, `snapshot`,
     `digest`, меню/рендер/HUD/UI + **временный мост** в пере-типизированном виде
     (`universe() -> &GameUniverse<D>`, `universe_mut() -> &mut GameUniverse<D>`,
     мост `submit_remote`/`apply`/`enqueue`/`flush_commands` остаются до §5).
   - `impl App<LocalDispatcher>` — `new`, `new_game`, `new_game_on_surface`,
     `load_game`, `load_save`, `load_save_bytes`, `run_demo`.
3. `local.rs` (`start_local_game*`, `local_begin_round`/`local_resume_round`/
   `local_close_turn`) — компилируются без изменений (тип `App<LocalDispatcher>`).
4. Все call-sites в тестах/бинарях остаются `App<LocalDispatcher>` без правок
   (default). Компилятор — оракул типов.

Критерий: `cargo check -p max_app` и `cargo test -p max_app` зелёные; `max_core`/`max_net` не тронуты.

## 3. Шаг 2 — Фасадные добавки в `max_core` (аддитивно)

1. **А2:** `pub fn dispatcher(&self) -> &D` и `dispatcher_mut(&mut self) -> &mut D`.
2. **А3:** `pub fn with_setup_and_dispatcher(map, team_kinds, dispatcher) -> Self`;
   `with_setup` делегирует ему с `LocalDispatcher`.
3. Тесты: `dispatcher()`/`dispatcher_mut()`; `with_setup_and_dispatcher(NetworkDispatcher)`
   даёт world с `snapshot_bytes` байт-в-байт равным `with_setup(LocalDispatcher)`.

Критерий: `cargo test -p max_core` зелёный; аддитивно, golden не меняются.

## 4. Шаг 3 — Сетевая поверхность и переключение драйвера на фасад

### 4.1 Общий генератор мира (А3)
Вынести тело `new_game_on_surface` в generic `build_world<D>(config, surface,
dispatcher) -> (GameUniverse<D>, NewGameReport)`; локальный `new_game_on_surface`
вызывает `build_world(..., LocalDispatcher)`; сетевой `initialize_game` —
`build_world(..., NetworkDispatcher::new(...))` без входа в раунд.

### 4.2 Конструктор и поверхность `App<NetworkDispatcher>`
`impl App<NetworkDispatcher>`: `new_network(config, local_team, play_mode)`;
перенос `initialize_game`/`run_game_loop`/`pump_events`/`handle_session_event`/
`wait_barrier`/`broadcast_outgoing`/`send_frame_barrier`/`flush_hud_orders`/
`send_barrier` в этот impl. `run_network`/`run_network_with_sink` — для лобби
(`App<LocalDispatcher>`), после `initialize_game` строят сетевой App.

### 4.3 `run_lobby` — сбор setup-данных вместо `app.submit` (А4)
`LobbyInfo` получает `mission_supplies`; `12` → `info.mission_supplies[team]`,
`13` — игнорировать; `initialize_game` читает `LobbyInfo` напрямую.

### 4.4 Барьерный путь через фасад (А1/А2/А5/А6)
FSM-сплит `begin/end/crc` → `step_*_barrier` + `record_*`; `handle_session_event`
→ `universe.submit_remote` (Replay/ApplyBarrier/ApplyStateSync); `flush_hud_orders`
→ `submit(Broadcast)` + `broadcast_outgoing`→`drain_outgoing`; `06`/`01` остаются
на `apply_now` + `send_untracked`.

### 4.5 Миграция тестов драйвера
`net.rs` тесты → `App::<NetworkDispatcher>::new_network`; `dispatcher.rs` тесты →
`record_*`-сплит.

Критерий: `cargo check`/`cargo test -p max_app` зелёные; гейты (а)/(б).

## 5. Шаг 4 — Удаление моста (после гейтов а/б)

Удалить `universe()`/`universe_mut()`/мост `submit_remote`/`apply`/`enqueue`/
`flush_commands` из `App`; пометить `apply_now`/`flush_commands`/`apply_state_sync`/
`apply_barrier` в `universe.rs` как FSM-канал.

## 6. Гейты (обязательные)

- **(а) барьерный путь идентичен legacy-мосту:** пакет-в-пакет/счётчик-в-счётчик
  на одних сценариях; таймауты/resume-54 зелёные; только после — удалять мост.
- **(б) вещание Broadcast:** `submit(Broadcast)` → `drain_outgoing` → пирам;
  `Local`/`Drop` не вещаются; `outgoing` накапливается.
- **(в) live-smoke + net-тесты:** `cargo test -p max_app` + `ci.sh --smoke`/`--smoke3`.
- **(г) детерминизм/golden без изменений.**

## 7. Guardrails

1. Не удалять мост до §4 и гейтов (а)/(б).
2. Не менять таймауты/счётчики/фазы FSM (сплит — только механический).
3. Не переводить `06`/`01` на `submit`/`drain_outgoing` (А5); не переводить `13`/`12` лобби через `NetworkDispatcher` (А4).
4. Не делать `Box<dyn Dispatcher>`/enum — мономорфизация `App<D>`.
5. Не добавлять `dispatcher`/`seq`/`outgoing`/`bus` в `snapshot_bytes`/CRC.
6. Не переносить `begin_local_round`/`resume_local_round`/`end_local_turn` в `impl<D: Dispatcher>`.
7. Комментарии в коде — английский; `#![forbid(unsafe_code)]`; без `f32`/I/O/threads в `max_core`.

## 8. Критерии приёмки (DoD)

- `cargo check`/`cargo test` (все крейты) — зелёные.
- Гейты (а)/(б)/(в)/(г) пройдены.
- Мост удалён: `rg '\b(universe|universe_mut)\(' crates/max_app/src` → пусто;
  `App` нигде не упоминает `LocalDispatcher` в `impl<D: Dispatcher>`.
- Полная проверка: `bash ci.sh` + `bash ci.sh --smoke --smoke3`.

---

После зелёного §8 — вернуться к Архитектору за приёмкой этапа C; дальнейшие этапы —
по решению владельца.
