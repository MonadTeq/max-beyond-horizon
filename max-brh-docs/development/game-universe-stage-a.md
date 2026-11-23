# ТЗ: GameUniverse — этап A (фасад поверх `GameState`)

> Статус: **утверждено владельцем 2026-11-23; передано Разработчику**.
> Архитектурное решение —
> [`../architecture/client/game-universe-facade.md`](../architecture/client/game-universe-facade.md)
> (статус «утверждено 2026-11-23»). Здесь — пошаговая реализация этапа A с
> обязательными гейтами: риск 3, пункт 4 (одиночный цикл без барьеров), риск 5
> (предрегистрирован на этап B).
>
> **Роль исполнителя:** Разработчик (DeepSeek v4 Flash). При расхождении с
> архитектурным документом или сомнении в поведении — стоп, вопрос Архитектору
> (эскалация), не домысливать. Комментарии в коде — строго на английском; `.md`
> — на русском.

## 0. Цель и границы

**Цель этапа A:** фасад `GameUniverse` **поверх** существующего `GameState`,
без инкапсуляции. `GameState` остаётся `pub` со всеми `pub` полями/методами;
тесты фаз 1–5 продолжают звать его напрямую.

**В этапе A делается:**

1. Модуль `max_core::universe`: `GameUniverse<D>`, trait `Dispatcher`,
   `LocalDispatcher`, типы `StateRef`/`StateDelta`/`EventStream`/`EventBus`/
   `GameEvent`/`NetworkCommand`/`RemoteCommand`/`SubmitOutcome`/`BarrierFacts`/
   `RoutedCommand`/`RemoteHandling`/`RejectSource`.
2. Тесты фасада (эквивалентность `submit→tick→snapshot` ≡ `enqueue→tick→snapshot_bytes`).
3. Интеграция в `App`: `GameUniverse<LocalDispatcher>` + `snapshot()`/`StateRef`
   для рендера; `state()`/`state_mut()` — временные делегаты (не удалять).
4. Гейт 3 (обязательный): миграция `06`/lobby на `submit` (очередь), сверка
   `snapshot_bytes` с компенсацией сдвига на один тик.
5. Решение 1 владельца: `next_team()` — внутрь `tick()`; `local.rs` становится
   потребителем `TurnEvent` из `EventStream`.

**В этапе A НЕ делается** (guardrails, см. §7): инкапсуляция `GameState`,
`NetworkDispatcher`, перенос `TurnBarrierFsm`, полевой diff `StateDelta`,
удаление `state()`/`state_mut()`.

## 1. Обязательное чтение перед стартом

- [`../architecture/client/game-universe-facade.md`](../architecture/client/game-universe-facade.md) — §1–§5 полностью.
- `max-brh/crates/max_core/src/{game,events,crc}.rs` — точки опоры (§4 решения).
- `max-brh/crates/max_types/src/{input,unit}.rs` — DTO `Command`, `UnitPresentation`.
- `max-brh/crates/max_app/src/{lib,local,net}.rs` — текущие потребители.

## 2. Шаг 1 — модуль `max_core::universe`

Создать `max-brh/crates/max_core/src/universe.rs` (или `universe/` + `mod.rs`),
зарегистрировать `pub mod universe;` в `lib.rs` (после `pub mod unit_values;`).
Никаких изменений `GameState`/`events.rs`/`crc.rs` — фасад только вызывает их.

Реализовать точно по §1–§3 решения:

```rust
pub struct GameUniverse<D: Dispatcher = LocalDispatcher> {
    state: GameState,
    dispatcher: D,
    seq: u64,
    outgoing: Vec<NetworkCommand>,
    bus: EventBus,
}

impl<D: Dispatcher> GameUniverse<D> {
    pub fn new(state: GameState, dispatcher: D) -> Self;
    pub fn submit(&mut self, cmd: Command) -> SubmitOutcome;
    pub fn tick(&mut self);
    pub fn snapshot(&self) -> StateRef<'_>;
    pub fn delta_since(&self, seq: u64) -> Option<StateDelta<'_>>;
    pub fn seq(&self) -> u64;
    pub fn subscribe(&self) -> EventStream;
    pub fn drain_outgoing(&mut self) -> Vec<NetworkCommand>;
    pub fn submit_remote(&mut self, cmd: RemoteCommand);
    // этап A only — временные доступы, убрать на этапе B:
    pub fn state(&self) -> &GameState;
    pub fn state_mut(&mut self) -> &mut GameState;
}

pub trait Dispatcher {
    fn route(&mut self, cmd: Command) -> RoutedCommand;
    fn receive(&mut self, cmd: RemoteCommand) -> RemoteHandling;
    fn plan(&mut self, facts: &BarrierFacts) -> Vec<NetworkCommand>;
}

pub struct LocalDispatcher;            // + impl Dispatcher
pub enum RoutedCommand { Local(Command), Broadcast(Command), Drop }
pub enum RemoteHandling { Replay(Command), ApplyStateSync(StateSyncPacket), Ignore }
pub struct BarrierFacts { /* begin_turn, end_turn, crc, state_sync, local_team, play_mode */ }
pub struct StateRef<'a> { /* &GameState + presentation(id) */ }
pub struct StateDelta<'a> { pub from_seq: u64, pub seq: u64, pub checksum: u16, pub state: StateRef<'a> }
pub struct EventStream { cursor: u64 }
pub enum GameEvent { Turn(TurnEvent), Production(ProductionEvent), Research(ResearchEvent),
    Sound(UnitSound), Industry(String), Chat(TeamId, String),
    CommandRejected { cmd: Command, outcome: ApplyOutcome, tick: u64, source: RejectSource } }
pub enum RejectSource { DroppedByRoute, RejectedBySimulation }
pub enum NetworkCommand { Command(Command), StateSync(StateSyncPacket) }
pub enum RemoteCommand { Command(Command), StateSync(StateSyncPacket) }
pub enum SubmitOutcome { Accepted, Dropped }
```

Поведенческие требования (обязательные):

- `submit` → `dispatcher.route(cmd)`: `Local(c)`/`Broadcast(c)` кладут `c` в
  `state.commands.push` (через `GameState::enqueue`, `game.rs:2700`); `Broadcast`
  дополнительно `outgoing.push(NetworkCommand::Command(c))`; `Drop` → `Dropped`.
  **Мутации нет** — только очередь.
- `submit_remote`: `RemoteHandling::Replay(c)` → `state.enqueue(c)`;
  `ApplyStateSync(p)` → `events::apply_state_sync(&mut state, &p)` немедленно
  (`events.rs:787`) и `seq += 1`; `Ignore` → ничего.
- `tick()`: обернуть `GameState::tick` (`game.rs:2659`) в порядке фаз §5 решения;
  **после** симуляции — `seq += 1`, push `GameEvent`s в `bus`, `outgoing =
  dispatcher.plan(&BarrierFacts{…})`. `BarrierFacts` строится **после фазы 3,
  до фазы 4**.
- **Барьерные степперы в `tick()` НЕ вызывать** (см. исправление в §2 решения:
  степперы — только `net.rs:1434/1474/1524`; иначе одиночный `snapshot_bytes`
  разойдётся). `LocalDispatcher::plan` → `vec![]`.
- `seq`/`bus`/`outgoing` **не** входят в `snapshot_bytes`/CRC (§3 решения).
- `#![forbid(unsafe_code)]` сохраняется; без `f32`/I/O/threads в фасаде.

## 3. Шаг 2 — тесты фасада

Новый файл(ы) `max-brh/crates/max_core/tests/universe.rs` (интеграционные) +
при необходимости inline-тесты в `universe.rs`. Минимальный набор:

1. **Эквивалентность** (ключевой): для скрипта команд `submit`→`tick` даёт
   `snapshot()`/`state().snapshot_bytes()` байт-в-байт равный прогону
   `enqueue`→`tick` того же скрипта. Эталон — существующий
   `scripted_replay_is_deterministic` (`game.rs:3458`): перенести его сценарий на
   фасад и сравнить со старым прогоном.
2. **`seq`/`delta_since`**: `delta_since(seq_after)` → `None`; `delta_since(0)`
   после первого тика → `Some` с `from_seq==0`, `seq==1`,
   `checksum == crc::state_digest(&state)`.
3. **`EventStream`**: два `subscribe()` дают независимые курсоры; `drain` второго
   не «съедает» события первого; события накапливаются за тик и вычитываются по
   запросу.
4. **`submit_remote(StateSync)`**: применяется сразу и инкрементирует `seq`;
   `submit_remote(Command)` — в очередь (применяется на следующем тике).
5. **`LocalDispatcher`**: `route` всегда `Local`; `plan` пуст; `drain_outgoing`
   пуст; барьерные счётчики после `tick` **не** изменились (см. §2).
6. **`SubmitOutcome`**: `submit` запрещённой команды → `Dropped` (через
   `RoutedCommand::Drop`), валидной → `Accepted`.
7. **Гейт пункта 4 (обязательный):** одиночная игра (`LocalDispatcher`) проходит
   полный цикл `submit(06) → tick → finish_turn → turn_counter+1` **без** вызова
   `step_*_barrier` (барьерные счётчики остаются `0`), и её `snapshot_bytes`
   идентичен сетевому прогону с выключенными барьерами.

Критерий: `cargo test -p max_core` зелёный, включая все существующие тесты фаз
1–5 (они не должны измениться).

## 4. Шаг 3 — интеграция в `App`

Файл `max-brh/crates/max_app/src/lib.rs`:

1. Поле `state: GameState` заменить на `universe: GameUniverse<LocalDispatcher>`
   (или добавить как новое владение — но состояние должно быть одно, не два).
2. Временные делегаты (этап A only, пометить `// Stage A only`):
   - `pub fn state(&self) -> &GameState { self.universe.state() }` (`lib.rs:852`);
   - `pub fn state_mut(&mut self) -> &mut GameState { self.universe.state_mut() }` (`lib.rs:857`).
   Это сохраняет компиляцию всех существующих вызовов `state()`/`state_mut()`
   без изменений.
3. Добавить `pub fn snapshot(&self) -> StateRef<'_>` и перевести рендер
   (`update_visual_units`, `hud`, чтение `presentation`) на `snapshot()`/`StateRef`.
   **`state()` НЕ удалять до этапа B** (риск 7 решения).
4. `digest()`/`snapshot()`/`apply()`/`enqueue()` (`lib.rs:3430-3455`) пока
   оставить как есть — они работают через делегаты.

Критерий: `cargo check -p max_app` и `cargo test -p max_app` зелёные; рендер
читает состояние через `snapshot()` без изменения вывода.

## 5. Шаг 4 — Гейт 3 (обязательный): миграция `06`/lobby на `submit`

**Задача:** унифицировать путь команд: `local_end_turn` (`local.rs:1032`) и
приём lobby-команд (`net.rs:1049`, `run_lobby`) переводятся с немедленного
`apply` на `submit` (очередь → применение в начале следующего тика).

**Гейт 3 (обязательный, не «проверить в конце»):**

1. Прогнать сценарии `06`/lobby/hot-seat в двух режимах:
   - старый: `enqueue`/`apply` + `tick` (текущий код);
   - новый: `submit` + `tick`.
2. Сравнить `snapshot_bytes` **с компенсацией сдвига** — новый прогон делает
   `N+1` тиков вместо `N` (команда применяется на тик позже).
3. Критерий: байт-в-байт совпадение после компенсации **для каждого** сценария
   (`06` в пошаговом, `06` в одновременном, hot-seat, lobby-команды 13/12).
4. Если хоть один сценарий не сходится — это не «приемлемый сдвиг», а баг
   классификации `RoutedCommand::Local` vs `Broadcast`, либо неявная немедленная
   мутация в `local_end_turn`. Её вынести в отдельный конфигурационный метод
   фасада (не возвращать немедленный `apply`). Стоп → Архитектор.

## 6. Шаг 5 — Решение 1 владельца: `next_team()` внутрь `tick()`

После зелёного гейта 3:

1. В `GameUniverse::tick()` после симуляционного шага: если
   `game_state == EndTurn` (`game.rs:2528`), прогонять `next_team()`
   (`game.rs:2345`) до `Human`/`Computer`/`GameOver`, эмитя каждый `TurnEvent` в
   `EventBus` как `GameEvent::Turn`.
2. `local.rs`: `local_next_team` (`local.rs:784`) перестаёт звать
   `state_mut().next_team()`; `local_end_turn` (`local.rs:1032`) становится
   `submit(06)`; `TurnEvent` читается из `EventStream`. Peek-логика
   (`local.rs:790-804`: `evaluate_status` до `finish_turn`, сравнение
   `player_team` до смены) переносится в ядро, как в решении.
3. `begin_round`/`end_round` на этапе A допустимо оставить `pub` для
   конфигурационных сценариев (`start_local_game`, `local.rs:615`);
   `next_team` — только внутри `tick()`.

**Регресс-гейт (риск 4):** hot-seat «Begin turn»-окно, AI-тики,
`finish_turn`/`evaluate_status` должны вести себя идентично; прогон golden
`hs-2.qa`/`sp-tb.qa` без изменений кадров, `snapshot_bytes` детерминирован.

## 7. Guardrails (запрещено в этапе A)

1. Не инкапсулировать `GameState` (остаётся `pub`, поля `pub`).
2. Не удалять `state()`/`state_mut()` из `App` (до этапа B).
3. Не реализовывать `NetworkDispatcher` (этап B).
4. Не переносить `TurnBarrierFsm` (этап B; гейт 5 ниже).
5. Не делать полевой diff `StateDelta` (`DirtySet`) — грубая дельта только.
6. Не добавлять `seq`/`EventBus`/`outgoing` в `snapshot_bytes`/CRC.
7. Не шагать `step_*_barrier` в `tick()`.
8. Комментарии в коде — английский; `#![forbid(unsafe_code)]`; без `f32`/I/O/threads.

## 8. Гейт 5 (предрегистрирован на этап B, не выполнять в этапе A)

При переносе `TurnBarrierFsm` из `net.rs` (header 16-30) в `NetworkDispatcher`:

- перенос **дословно**, без «улучшений» (retransmit 3 с, timeout 30 с, resume-54);
- покрыть тестами таймауты и resume **до** удаления старого кода;
- степперы `step_*_barrier` (`net.rs:1434/1474/1524`) остаются вызовами
  `GameState` и не меняют поведение.

## 9. Критерии приёмки (DoD) и валидация

- `cargo check` — зелёный.
- `cargo test -p max_core` — зелёный (существующие фазы 1–5 **плюс** новые тесты фасада).
- `cargo test -p max_app` — зелёный.
- Гейт 3 пройден: `snapshot_bytes` идентичен после компенсации сдвига для всех
  сценариев `06`/lobby/hot-seat.
- Гейт 4 (пункт 4) пройден: одиночный цикл `submit → tick → finish_turn →
  turn_counter+1` без `step_*_barrier`; `snapshot_bytes` идентичен сетевому
  прогону с выключенными барьерами.
- Решение 1 пройдено: `next_team` вызывается только из `tick()`; `local.rs` не
  зовёт его напрямую; `TurnEvent` — только через `EventStream`.
- Детерминизм не нарушен: golden `snapshot_bytes`/`replay_digest` не изменились
  (кроме задокументированного сдвига, закрытого гейтом 3).

**Полная проверка** (как в [`../build.md`](../build.md)): `bash ci.sh`
(`max-brh-cicd/ci.sh`). Быстрая итерация — `cargo test -p max_core`.

---

После зелёного этапа A — вернуться к Архитектору за ТЗ этапа B (инкапсуляция,
`NetworkDispatcher`, полевой diff).
