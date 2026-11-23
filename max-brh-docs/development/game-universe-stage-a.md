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
5. Решение 1 (уточнённое): `next_team()` — внутрь фасада, гейтирован
   `Dispatcher::advances_turns()`; триггер — `end_local_turn(team)`, а не каждый `tick()`;
   `local.rs` становится потребителем `GameEvent::Turn(TurnNotice)` из `EventStream`.

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

## 5. Шаг 4 — Гейт 3 (обязательный): миграция queue-able команд на `submit`

**Задача:** унифицировать путь команд НА ОЧЕРЕДЬ `submit` для тех команд, что можно
сдвинуть на один тик — **но НЕ для завершения хода.** `local_end_turn` (`local.rs:1032`)
**НЕ** переводится на `submit(06)`: он становится фасадным методом `end_local_turn(team)`
(§6), который применяет `06` **немедленно**. На `submit` переводятся lobby-команды
(`net.rs:1049`, `run_lobby`) и прочие queue-able команды (применение в начале следующего тика).

**Гейт 3 (обязательный, не «проверить в конце»):**

1. Прогнать queue-able сценарии (lobby 13/12) в двух режимах:
   - старый: `enqueue`/`apply` + `tick` (текущий код);
   - новый: `submit` + `tick`.
2. Сравнить `snapshot_bytes` **с компенсацией сдвига** — новый прогон делает
   `N+1` тиков вместо `N` (команда применяется на тик позже).
3. Критерий: байт-в-байт совпадение после компенсации для queue-able сценариев
   (lobby 13/12).
4. Если хоть один сценарий не сходится — это не «приемлемый сдвиг», а баг
   классификации `RoutedCommand::Local` vs `Broadcast`. Стоп → Архитектор.
5. **`06` (завершение хода) НЕ мигрирует на `submit` (решение Архитектора; ADR §1/§5).**
   В одновременном режиме `06` ставит только `finished_turn` без `EndTurn`
   (`events.rs:574-580`); раунд закрывается через `are_teams_finished_turn`
   (`game.rs:2277`), которому нужен **синхронный** `finished_turn`. Отложенный `06` на
   следующем тике снова войдёт в `EndTurn` и закроет ход новой команды. Сценарии `06`
   валидируются в §6 (гейт «без сдвига», байт-в-байт), а не здесь.

## 6. Шаг 5 — Решение 1 (уточнённое): машина хода внутрь фасада, гейтированная политикой

После зелёного гейта 3:

1. В `Dispatcher` добавить `fn advances_turns(&mut self) -> bool` (default `false`);
   `LocalDispatcher` → `true` (ADR §2).
2. В `GameUniverse<LocalDispatcher>` добавить `end_local_turn(team)` (ADR §1):
   немедленный `events::apply_command(command_finished_turn(team))` + `enter_end_turn()`
   + прогон `next_team()` до `Human`/`Computer`/`GameOver` (по `RoundClosed` —
   продолжение), эмитируя каждый шаг как `GameEvent::Turn(TurnNotice)`. Peek-данные
   (`previous_player_team` до смены, `evaluate_status` до `finish_turn` при
   `upcoming_team().is_none()`) вычисляются **до** перехода и кладутся в `TurnNotice` (ADR §3).
3. `local.rs`: `local_next_team` (`local.rs:784`) перестаёт звать `state_mut().next_team()`
   и становится потребителем `TurnNotice` из `EventStream`; `local_end_turn` (`local.rs:1032`)
   становится `universe.end_local_turn(team)`. Тот же переход для AI-пути
   (`local.rs:1235-1247`: `apply(06)+enter_end_turn+local_next_team` → `end_local_turn`).
4. `begin_round`/`end_round` на этапе A допустимо оставить `pub` для конфигурационных
   сценариев (`start_local_game`, `local.rs:615`); `next_team` — только внутри фасада.

**Регресс-гейт (риск 4):** hot-seat «Begin turn»-окно, AI-тики,
`finish_turn`/`evaluate_status` идентичны; golden `hs-2.qa`/`sp-tb.qa` без изменений кадров;
`snapshot_bytes` детерминирован; порядок строк HUD-лога (RoundClosed → production/research →
следующий Human) сохранён. Гейт «06 без сдвига»: `end_local_turn` даёт байт-в-байт тот же
`snapshot_bytes`, что текущий `apply(06)+enter_end_turn+local_next_team`.

**Гейт различимости путей (уточнение владельца):** `end_local_turn(team)` — единственный
триггер машины хода в одиночной игре; он применяет `06` немедленно. Отдельно: `submit(06)`
в одиночной игре **не** должен закрывать ход немедленно (идёт в очередь, применяется в
начале следующего `tick()`). Два пути должны быть **различимы в тесте**: если их поведение
неотличимо — `end_local_turn` и `submit` недостаточно разделены.

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
- Гейт 3 пройден: `snapshot_bytes` идентичен после компенсации сдвига для queue-able
  сценариев (lobby 13/12); `06` (завершение хода) НЕ мигрирует на `submit` и валидируется
  в §6 без сдвига.
- Гейт 4 (пункт 4) пройден: одиночный цикл `submit → tick → finish_turn →
  turn_counter+1` без `step_*_barrier`; `snapshot_bytes` идентичен сетевому
  прогону с выключенными барьерами.
- Решение 1 (уточнённое) пройдено: `next_team` вызывается только фасадом и только через
  `end_local_turn`, гейтированный `advances_turns()`; `local.rs`/`net.rs` не зовут его
  напрямую; `TurnEvent` доставляется только как `GameEvent::Turn(TurnNotice)` через `EventStream`.
- Гейт различимости путей пройден: `end_local_turn` (немедленный `06`) и `submit(06)`
  (очередь) дают **различимое** поведение в тесте.
- Детерминизм не нарушен: golden `snapshot_bytes`/`replay_digest` не изменились
  (кроме задокументированного сдвига, закрытого гейтом 3).

**Полная проверка** (как в [`../build.md`](../build.md)): `bash ci.sh`
(`max-brh-cicd/ci.sh`). Быстрая итерация — `cargo test -p max_core`.

---

После зелёного этапа A — вернуться к Архитектору за ТЗ этапа B (инкапсуляция,
`NetworkDispatcher`, полевой diff).
