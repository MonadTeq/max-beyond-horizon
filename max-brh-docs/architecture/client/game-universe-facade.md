# GameUniverse — фасад ядра над `max_core`

> Статус: **УТВЕРЖДЕНО владельцем 2026-11-23** (решения §7 приняты в полном
> объёме; пункт 4 — в скорректированной формулировке §2, взамен исходной, которая
> смешивала `turn_counter` и барьерные счётчики). Проект архитектурного решения;
> реализация — этапами (A)→(B), см. §6.
>
> Источники: `max-brh/crates/max_core/src/{game,events,crc,lib}.rs`,
> `max-brh/crates/max_types/src/{input,unit,lib}.rs`,
> `max-brh/crates/max_net/src/wire/command.rs`,
> `max-brh/crates/max_app/src/{lib,local,net}.rs`.
> Границы слоёв — `max_types::lib.rs:1-33`, `max_core::lib.rs:1-104`,
> `../README.md` (схема слоёв).

## 0. Суть решения

`GameUniverse` — **тонкая владеющая обёртка** над `GameState`, которая:

- скрывает всё мутирующее (`submit`/`tick`/`submit_remote`) и весь ввод-вывод
  состояния/событий за pull-интерфейсом;
- **не знает** рендеринг и сеть: рендеринг читает `snapshot()`, транспорт
  общается DTO `NetworkCommand`/`RemoteCommand`;
- держит `seq` (версию состояния) и `EventBus` (push-буфер, вычитываемый по запросу);
- делегирует «что на другой стороне submit» и «барьерную FSM» через trait `Dispatcher`.

Философия: минимализм, инверсия зависимостей, тестируемость, детерминизм внутри.
Четыре столпа + транспорт:

1. `submit(cmd: Command)` — единственная точка изменений;
2. `tick()` — единственная точка продвижения времени (шаг, не цикл);
3. `snapshot() -> StateRef` + `delta_since(seq) -> Option<StateDelta>` — pull-выход состояния;
4. `subscribe() -> EventStream` — pull-выход событий (push-канал, забирается по запросу);
5. `drain_outgoing() -> Vec<NetworkCommand>` + `submit_remote(cmd: RemoteCommand)` —
   единственный контакт с транспортом (DTO, не протокол).

---

## 1. Структура `GameUniverse` и точные сигнатуры

```rust
// Новый модуль max_core::universe (Application; НЕ зависит от max_net/max_app).

pub struct GameUniverse<D: Dispatcher = LocalDispatcher> {
    state: GameState,               // владение, однопоточный lockstep (Q20)
    dispatcher: D,                  // инверсия: локально / сеть
    seq: u64,                       // монотонная версия состояния (для delta_since)
    outgoing: Vec<NetworkCommand>,  // буфер, наполняется в tick, отдаётся drain_outgoing (Q24)
    bus: EventBus,                  // push-сторона событий (Q4)
}

impl<D: Dispatcher> GameUniverse<D> {
    // — единственная точка изменений —
    pub fn submit(&mut self, cmd: Command) -> SubmitOutcome;

    // — единственная точка продвижения времени (шаг, не цикл; Q23) —
    pub fn tick(&mut self);

    // — pull-выход состояния —
    pub fn snapshot(&self) -> StateRef<'_>;                  // Q20: &State, без Arc
    pub fn delta_since(&self, seq: u64) -> Option<StateDelta<'_>>;
    pub fn seq(&self) -> u64;

    // — pull-выход событий —
    pub fn subscribe(&self) -> EventStream;                  // push-канал, забирается по запросу

    // — единственный контакт с транспортом (DTO, не протокол) —
    pub fn drain_outgoing(&mut self) -> Vec<NetworkCommand>; // Q24: синхронный Vec в конце tick
    pub fn submit_remote(&mut self, cmd: RemoteCommand);

    // — конструкторы / загрузка (мутация состояния вне tick, тоже учитывается в seq) —
    pub fn from_save(save: &crate::save::SaveFile) -> Self;
    pub fn into_state(self) -> GameState;                    // только этап (A), для миграции
}
```

Ключевые семантические решения:

1. **`submit` ≠ `apply_command`.** `submit` — *приём и упорядочивание*
   (маршрутизация `Dispatcher`'ом → `state.commands.push`). Реальная мутация
   происходит в `tick` через `events::apply_pending` → `apply_command`, строго
   в порядке поступления (`events.rs:8-14`, `game.rs:2695`). Это сохраняет
   существующий контракт «команды применяются в начале тика».
2. **`submit` — только очередь; `StateSync` (пакет 54) — немедленно** (решение владельца).
   `RemoteCommand::StateSync` применяется сразу (`events::apply_state_sync`,
   `events.rs:787`), потому что это авторитетное состояние, а не «намерение».
   Сдвиг пакета `06`/лобби-команд на один тик — принятая цена за единый путь
   (гейт — §6, риск 3).
3. **`seq` — фасадная, не симуляционная величина.** Её **нельзя** добавлять в
   `snapshot_bytes()`/CRC (`game.rs:2750`), иначе сломается детерминизм /
   `crc::state_digest`. `seq` инкрементируется на каждое наблюдаемое изменение
   состояния: `tick`, `submit_remote(RemoteCommand::StateSync)`, `from_save`.
4. **`snapshot() -> StateRef`** — заимствование, привязанное к `&self`;
   заимствование заканчивается до следующего `submit`/`tick` (borrow-checker это
   гарантирует). Никакого клона, `Arc`, буфера (Q20).

---

## 2. `Dispatcher` trait + `LocalDispatcher` / `NetworkDispatcher`

### Принцип: `Dispatcher` — чистая политика, не трогает `GameState`

Фасад делает **всю** мутацию; `Dispatcher` только *классифицирует* команды и
*планирует* барьерный поток. Так `max_core` остаётся транспортно-агностичным.

```rust
pub trait Dispatcher {
    /// Маршрут для локально поданной команды.
    fn route(&mut self, cmd: Command) -> RoutedCommand;

    /// Что делать с командой, пришедшей от пира.
    fn receive(&mut self, cmd: RemoteCommand) -> RemoteHandling;

    /// Барьерный план после тика — какие DTO отправить.
    /// `facts` — read-only срез барьерных счётчиков (фасад строит из GameState).
    fn plan(&mut self, facts: &BarrierFacts) -> Vec<NetworkCommand>;
}

pub enum RoutedCommand {
    Local(Command),      // применить только себе (барьер 00/52/45, одиночная игра)
    Broadcast(Command),  // применить себе и разослать пирам (08/38/41/50/06/11/53/18/43…)
    Drop,                // отбросить (запрещённый порядок неактивной команды и т.п.)
}

pub enum RemoteHandling {
    Replay(Command),                // положить в очередь в порядке поступления
    ApplyStateSync(StateSyncPacket),// применить авторитетное состояние сразу (пакет 54)
    Ignore,
}

/// Read-only срез барьерных счётчиков (значения — из GameState, CRC-релевантны).
pub struct BarrierFacts {
    pub begin_turn: Option<u8>,   // GameState::begin_turn_barrier (game.rs:2432)
    pub end_turn:   Option<u8>,   // GameState::end_turn_barrier   (game.rs:2423)
    pub crc:        Option<u8>,   // GameState::crc_barrier        (game.rs:2447)
    pub state_sync: [u8; TEAM_SLOTS], // GameState::state_sync_indices
    pub local_team: TeamId,
    pub play_mode: PlayMode,
}
```

### `LocalDispatcher` (в `max_core`)

Zero-sized, детерминированный. `route` → всегда `Local` (сети нет); `receive` →
`Ignore`; `plan` → `vec![]`.

> **Барьерные степперы — только сетевой путь (уточнение к пункту 4 владельца).**
> `step_begin_turn_barrier`/`step_end_turn_barrier`/`step_crc_barrier`
> (`game.rs:2463/2474/2485`) **не вызываются в `tick()`** — их единственные
> вызовы в проде — `max_app/net.rs:1434/1474/1524` (FSM `TurnBarrierFsm`); в
> `local.rs` их нет. В одиночной игре (`network_game = false`) барьерные
> счётчики остаются `0`, и это правильно: `Remote_IsNetworkGame = false` в C++
> пропускает барьерный путь. `finish_turn` (`game.rs:2570`) гейтится фазой
> `game_state == EndTurn`, а `manage_economy` (`game.rs:1865`) вызывается из
> `next_team` (`game.rs:2373`) — ни то, ни другое **не зависит** от барьерных
> счётчиков. Отсюда:
>
> * `LocalDispatcher::plan()` пуст и степперы **не** шагает — верно;
> * фасадный `tick()` **не** шагает барьеры (повторяет `GameState::tick`,
>   `game.rs:2659`), иначе `snapshot_bytes` одиночной игры разойдётся с текущим;
> * степперы остаются за `NetworkDispatcher` (этап B), как сейчас в `net.rs`
>   (гейт 5 — перенос без изменения поведения).
>
> **Гейт пункта 4 (обязательный, этап A):** тест, что одиночная игра
> (`LocalDispatcher`) проходит полный цикл `submit → tick → finish_turn →
> turn_counter+1` **без** вызова `step_*_barrier`, и её `snapshot_bytes` идентичен
> сетевому прогону с выключенными барьерами.

### `NetworkDispatcher` (в `max_app`)

Реализует trait **на стороне приложения** (владеет `SessionHandle` и переносит
нынешнюю `TurnBarrierFsm` из `net.rs`). `route`: команда, влияющая на симуляцию →
`Broadcast`; барьеры → `Local`. `receive`: `RemoteCommand::Command` → `Replay`
(в порядке поступления); `RemoteCommand::StateSync` → `ApplyStateSync`. `plan`:
пре-инкремент барьерных счётчиков и генерация
`NetworkCommand::Command(TurnBarrier/EndTurnBarrier/CrcBarrier)` + resume-54.

### Где живёт барьерная логика (точное разделение)

| Слой | Что живёт | Где сейчас |
|---|---|---|
| **Счётчики** (CRC-состояние) | `turn_indices`, `next_turn_indices`, `frame_sync_counter`, `frame_sync_counter2`, `team_data_crc16`, `state_sync_*` | остаются в `GameState` (`game.rs:300-317`) |
| **Степперы** (пре-инкремент) | `step_begin_turn_barrier`/`step_end_turn_barrier`/`step_crc_barrier` | остаются в `GameState` (`game.rs:2463/2474/2485`) |
| **Предикаты согласия** | `*_barrier()` / `*_barrier_reached()` | остаются в `GameState` (`game.rs:2423-2511`) |
| **FSM ожидания/повтора/таймаута** | `TurnBarrierFsm` (retransmit 3 с, timeout 30 с, resume) | переносится в `NetworkDispatcher` (`max_app::net`, header 16-30) |
| **`seq`** (версия для delta) | `u64` | новый, в `GameUniverse` |

Степперы остаются `pub` (этап A) и вызываются фасадом/`NetworkDispatcher`; на
этапе B — `pub(crate)` и доступны только фасаду.

---

## 3. Типы `StateRef` / `StateDelta` / `EventStream` / `NetworkCommand` / `RemoteCommand`

### `StateRef<'a>` — заимствованный снапшот (Q20, Q22)

```rust
pub struct StateRef<'a> { state: &'a GameState }

impl<'a> StateRef<'a> {
    // Q22: UnitPresentation — ЧАСТЬ снапшота.
    pub fn presentation(&self, id: UnitId) -> Option<&UnitPresentation>; // game.rs:807
    // + пас-сквозь к read-only методам GameState:
    // unit(id), unit_list(kind), units_at(grid), turn_counter, active_turn_team,
    // game_state, play_mode, teams, map, state_checksum(), state_digest(), …
}
```

#### Что входит в `StateRef`, что нет

| Входит (читается рендером/QA/сетью) | Не входит (фасадная/UI-не-состояние) |
|---|---|
| `map`, `teams`, `team_units`, `rng` | `seq` (версия, не в `snapshot_bytes`) |
| юниты `UnitInfo` (5 списков, `game.rs:790`) | `EventBus` / курсоры событий |
| **`UnitPresentation`** (Q22, `game.rs:807`) | `outgoing` (транспортный буфер) |
| `turn_counter`, `game_state`, `active_turn_team`, `play_mode`, `player_team` | `save_meta` (V70-only, `game.rs:359-364`) |
| барьерные счётчики (CRC-состояние, `game.rs:300-317`) | `production_events`/`research_events`/`industry_message`/`unit_sounds` (UI-отчёты) |
| `state_checksum()`/`state_digest()` (`crc.rs:179/205`) | — |

`UnitPresentation` не входит в `snapshot_bytes`/CRC (`max_types::unit.rs:135-143`,
`game.rs:212-214`), но входит в снапшот фасада по Q22 — рендерер читает его через
`StateRef::presentation`. Клонирования нет.

### `StateDelta<'a>` — конверт изменений (грубая дельта)

С учётом Q20 (запрет буферизации старых снапшотов) честная минимальная дельта —
*грубая*:

```rust
pub struct StateDelta<'a> {
    pub from_seq: u64,           // от какой версии (исключающая базовая)
    pub seq: u64,                // до какой версии
    pub checksum: u16,           // crc::state_digest на seq (crc.rs:205)
    pub state: StateRef<'a>,     // полный текущий снапшот (заимствованный, не клон)
}

// delta_since(seq) -> None ⇔ seq >= self.seq  (вызывающий уже актуален)
```

> **Фасадные, не в CRC (уточнение владельца, пункт 2):** `from_seq`/`seq`/
> `checksum` — это **фасадные** величины. Они не входят в `snapshot_bytes()` и
> ни в какой CRC. `checksum` — это `crc::state_digest(&state)` (`crc.rs:205`),
> то есть CRC16 состояния **без учёта `seq`**. Никто не должен добавлять `seq`
> в дайджест «для порядка» — это сломает парность с `crc.rs` и C++-генератором.

`state` внутри дельты позволяет за один вызов получить и версию, и свежее
состояние (без второго `snapshot()`). Поле-уровневый diff юнитов
(`DirtySet<UnitId>` через `write_unit`/`spawn_unit`/`destroy_unit`,
`game.rs:859/1290/1541`) — **только этап B** и только если профилирование
покажет, что передача полного `State` по сети дорога. **Не делать сразу**
(решение владельца).

### `EventStream` — pull-канал событий (Q4)

```rust
pub struct EventStream { cursor: u64 }      // монотонный курсор

impl GameUniverse {
    pub fn subscribe(&self) -> EventStream; // свежий курсор с текущей позиции
}
impl EventStream {
    pub fn drain(&mut self, universe: &mut GameUniverse) -> Vec<GameEvent>;
    pub fn try_drain(&mut self, universe: &GameUniverse) -> &[GameEvent]; // без мутации (опц.)
}

pub enum GameEvent {
    Turn(TurnEvent),                    // Human/Computer/RoundClosed/GameOver (game.rs:176)
    Production(ProductionEvent),        // spawn/interrupt (game.rs:281)
    Research(ResearchEvent),            // уровни тем (game.rs:284)
    Sound(UnitSound),                   // боевые звуки (game.rs:153)
    Industry(String),                   // сообщение комплекса (game.rs:292)
    Chat(TeamId, String),               // чат (game.rs:319)
    CommandRejected {                   // диагностика: НЕ терять контекст (решение владельца)
        cmd: Command,                   // какая команда
        outcome: ApplyOutcome,          // полный enum (events.rs:161), не только kind
        tick: u64,                      // на каком тике отклонена
        source: RejectSource,           // DroppedByRoute | RejectedBySimulation
    },
}

pub enum RejectSource { DroppedByRoute, RejectedBySimulation }
```

**Буфер и курсоры (решение владельца, пункт 6):** `EventBus` (private) хранит
**один** append-only буфер с монотонными номерами; `subscribe()` возвращает
**независимый** курсор. Рендер, QA/реплей и логирование вычитывают свои
диапазоны, не конкурируя за один курсор. Мультикаст не нужен. Обрезка — по
**минимальному** активному курсору (или по K тиков, как `unit_sounds`,
`game.rs:2663-2667`). События — UI/QA-отчёт, не состояние, поэтому обрезка не
влияет на детерминизм.

`TurnEvent` попадает в поток при продвижении машины хода (см. §4).

### `NetworkCommand` / `RemoteCommand` — транспортные DTO (не протокол)

```rust
pub enum NetworkCommand {
    Command(Command),                 // 00/52/45/08/38/41/50/06/11/53/18/43/…
    StateSync(StateSyncPacket),       // пакет 54 (max_types::events, реэкспорт events.rs:694)
}

pub enum RemoteCommand {
    Command(Command),
    StateSync(StateSyncPacket),
}
```

Связь с существующим: `Command` уже чистый DTO (`max_types::input::Command`,
`input.rs:541`), `StateSyncPacket` — уже чистый DTO (`max_types::events`,
`events.rs:694`). `max_net` уже умеет `Command ⇄ wire`
(`max_net/wire/command.rs:677/702`) и `state_sync_for`. То есть **новых
дублирующих структур не нужно** — только тонкие обёртки над ними на границе
фасада. «DTO, не протокол»: фасад отдаёт типизированные DTO, `max_net` кодирует
их в байты; протокольные форматы в фасад не протекают.

---

## 4. Маппинг: существующий модуль/функция → роль в фасаде

| Существующее | Роль в фасаде |
|---|---|
| `max_types::input::Command` (`input.rs:541`) | вход `submit`; выход `NetworkCommand::Command` |
| `events::CommandQueue` (`events.rs:111`) | внутренняя очередь; `submit` → `push`, tick → `pop_front` |
| `events::apply_pending` (`events.rs:182`) | фаза `apply` внутри tick |
| `events::apply_command` (`events.rs:194`) | **внутренняя** мутация одного пакета (не публичная точка входа) |
| `events::apply_unit_state`/`apply_state_sync` (`events.rs:700/787`) | `submit_remote(RemoteCommand::StateSync)` — немедленно |
| `events::UnitEvent`/`process_unit_event` (`events.rs:68/86`) | **не** событие наружу; обрабатывается в tick (`game.rs:1791`) |
| `GameState::tick` (`game.rs:2659`) | тело фазы `tick` фасада |
| `GameState::process_orders` (`game.rs:1759`) | фаза «правила» (машина приказов, 5 списков) |
| `GameState::manage_economy` (`game.rs:1865`) | фаза «правила/счётчики», триггерится **машиной хода** (`next_team`, `game.rs:2373`), а не каждым тиком |
| `production::progress_build_state` + `advance_research` (`game.rs:2043/2608`) | фазы `finish_turn` → события `Production`/`Research` |
| AI: `ai_begin_turn`/`ai_check_reactions`/`ai_check_end_turn`/`dispatch_path_jobs` (`game.rs:2682-2690`, `next_team:2392`) | фазы внутри tick + машины хода |
| `TurnEvent`/`begin_round`/`next_team`/`end_round`/`finish_turn` (`game.rs:176/2311/2345/2407/2570`) | машина хода → эмитит `GameEvent::Turn` в поток |
| `step_*_barrier` + `*_barrier(_reached)` (`game.rs:2423-2511`) | барьерная логика: степперы в `GameState`, FSM в `NetworkDispatcher` |
| `crc::state_checksum`/`state_digest`/`replay_digest_bytes` (`crc.rs:179/205/242`) | фаза «hash/CRC»; `state_digest` — поле `StateDelta::checksum` |
| `GameState::snapshot_bytes` (`game.rs:2750`) | основа `StateRef`/контроля детерминизма (не `seq`!) |
| `GameState::presentation` (`game.rs:807`) | `StateRef::presentation` (Q22) |
| `take_production_events`/`take_research_events`/`take_unit_sounds`/`take_industry_message` (`game.rs:2616/2054/2062/2642`) | заменяются: push в `EventBus` → `EventStream::drain` |
| `max_net/wire/command.rs:677/702` | кодирование/декодирование `NetworkCommand`/`RemoteCommand` (тонкая надстройка) |
| `max_app::App::apply`/`enqueue`/`state_mut` (`lib.rs:3430/3441/857`) | заменяются на `submit`/`snapshot` (два пути → один) |
| `max_app::local.rs`/`net.rs` (TurnEvent-потребители) | мигрируют на `EventStream` + `drain_outgoing`/`submit_remote` |

**Особо: машина хода внутри `tick` (решение владельца, пункт 1).** `next_team()`
и эмиссия `TurnEvent` в `EventStream` — **внутри `tick()`**. Владелец (рендер,
`local.rs`, `net.rs`) не зовёт `next_team()` сам. Логика из
`local.rs:790-804` (peek до перехода: `evaluate_status` до `finish_turn`, сравнение
`player_team` до смены команды) и hot-seat «Begin turn»-окно **переносится в ядро**,
а не остаётся снаружи. `manage_economy` остаётся фазой внутри `next_team`
(`game.rs:2373`). На этапе A допустимо временно оставить `begin_round`/`end_round`
публичными для конфигурационных сценариев, но `next_team` — только внутри `tick()`.

---

## 5. Порядок фаз внутри `tick()` (фиксированный)

Концептуальные фазы владельца → конкретные вызовы:

| # | Фаза владельца | Конкретно в фасаде |
|---|---|---|
| 1 | **команды** | `state.poll_path_results()` (P5, `game.rs:2670`) + приём `submit` уже в очереди |
| 2 | **apply** | `state.apply_pending_commands()` → `events::apply_command` в порядке поступления (`game.rs:2673`) |
| 3 | **правила/счётчики** | `state.process_orders()` (если фаза ≠ `TurnActive`, `game.rs:2676`); затем AI: `check_reactions`/`dispatch_path_jobs`/`check_reactions`/`check_end_turn` (если `!orders_pending`, `game.rs:2681-2690`); при закрытии хода — машина хода `next_team`/`manage_economy`/`finish_turn` (счётчики ходов, `game.rs:2585-2610`) |
| 4 | **hash/CRC** | `seq += 1`; `checksum = crc::state_digest(&state)` (`crc.rs:205`) — **после** мутаций |
| 5 | **события** | push `GameEvent::{Production,Research,Sound,Industry,Chat,Turn,CommandRejected}` в `EventBus` |
| 6 | **outgoing** | `self.outgoing = dispatcher.plan(&BarrierFacts{…})` (Q24: синхронный Vec, отдаётся `drain_outgoing`) |

Порядок фаз 1–3 **бит-в-бит** повторяет существующий `GameState::tick`
(`game.rs:2659-2692`) — фасад лишь добавляет 4–6 (версию, события, outgoing)
*после* симуляции, не трогая детерминизм. `seq`/`bus`/`outgoing` в `snapshot_bytes`
не входят.

> **Момент построения `BarrierFacts` (уточнение владельца, пункт 3):**
> `BarrierFacts` строится **после фазы 3, до фазы 4**. Тогда `dispatcher.plan()`
> видит **свежие** барьерные счётчики (степперы уже шагнули, `game.rs:2463-2485`),
> а `checksum` ещё не посчитан — что корректно: `plan()` не нуждается в CRC
> состояния, он оперирует только счётчиками. Порядок «правила → барьеры →
> checksum → события → outgoing» фиксируется.

---

## 6. План миграции (A)→(B)

### Этап A — фасад ПОВЕРХ `GameState`

1. Новый модуль `max_core::universe` (или файл `universe.rs`), типы §1–§3,
   `LocalDispatcher`.
2. `GameState` остаётся `pub` со всеми `pub` полями/методами; `max_core::lib.rs`
   продолжает реэкспортировать как сейчас.
3. Добавить `GameUniverse<LocalDispatcher>` как **дополнительную** точку входа;
   `App` начинает пользоваться `submit`/`tick`/`snapshot` параллельно со старым
   `state()`/`state_mut()`/`apply()`/`enqueue()` (два пути сосуществуют,
   детерминизм сохраняется — фасад вызывает те же методы `GameState`).
4. **Заимствование (риск 7):** на этапе A ввести `snapshot()` **рядом** со старым
   `state()` (`lib.rs:852`), перевести рендер на `snapshot()`, но **не удалять**
   `state()` до этапа B. Тогда borrow-checker-рефактор `update_visual_units`/`hud`
   делается постепенно, а не одним куском.
5. Тесты фаз 1–5 **не трогаются** — они зовут `GameState` напрямую и продолжают
   проходить; фасад покрывается новым набором тестов (`submit`→`tick`→`snapshot`
   идентичен `enqueue`→`tick`→`snapshot_bytes`).
6. В `App` вводится `GameUniverse`; рендерер переводится на `snapshot()`/`StateRef`
   (Q21 pull), но `state_mut()` временно остаётся.

### Этап B — полная инкапсуляция

1. `GameState` → private в модуле `universe`; мутирующие методы → `pub(crate)`
   (доступны фасаду и `#[cfg(test)]`).
2. Перенос владения машиной хода и барьерной FSM (§4): `local.rs`/`net.rs`
   становятся чистыми потребителями `EventStream`/`drain_outgoing`/`submit_remote`.
3. `NetworkDispatcher` в `max_app` реализует `Dispatcher` (перенос `TurnBarrierFsm`),
   `max_net` добавляет `encode/decode` для `NetworkCommand`/`RemoteCommand`.
4. Тесты фаз 1–5 переписываются на фасад: `spawn_unit`/`set_order`/`write_unit`/
   `tick`/`snapshot_bytes` → `submit`/`tick`/`snapshot`; внутренние `mod tests`
   остаются (видят `pub(crate)`), интеграционные (`tests/*.rs`) переезжают на
   публичный API фасада.
5. Удаление `state_mut()` из `App`; `into_state` убирается.

### Риски и обязательные гейты

1. **~460 тестов фаз 1–5** (интеграционные `tests/*.rs`) завязаны на прямые методы
   `GameState` (`spawn_unit`, `set_order`, `write_unit`, `tick`, `enqueue`,
   `snapshot_bytes`). Переезд — самый объёмный шаг этапа B; тесты фаз 1–5
   переносятся механически, сохранив золотые `snapshot_bytes`.
2. **`max_app` прямые мутации** через `state_mut()` (`start_local_game`
   `local.rs:615`, `initialize_game` `net.rs:1184`, `local_end_turn` `local.rs:1032`,
   и десятки других) — каждая становится `submit`/конфигурационным методом фасада;
   риск потери одного из «настроечных» полей (`network_game`, `enemy_halt`,
   `victory_*`, `mission_*`).
3. **Сдвиг на один тик — самый опасный (обязательный гейт).** Два пути команд
   (немедленный `apply` vs `enqueue`) унифицируются в один (`submit` → очередь).
   **Гейт:** прогнать **все** сценарии `06`/lobby/hot-seat в двух режимах
   (старый `enqueue`+`tick` vs новый `submit`+`tick`) и сравнить `snapshot_bytes`
   **с компенсацией сдвига** (прогон N+1 тиков вместо N). Если хоть один сценарий
   не сходится — это не «приемлемый сдвиг», а баг в классификации команд
   (`RoutedCommand::Local` vs `Broadcast`), либо `local_end_turn` содержит неявную
   немедленную мутацию, которую надо вынести в отдельный конфигурационный метод
   фасада. Это отдельный шаг ТЗ, а не «проверить в конце».
4. **Машина хода** переезжает из `local.rs`/`net.rs` внутрь фасада — регрессия в
   hot-seat («Begin turn»-окно), AI-тиках, `finish_turn`/`evaluate_status`
   (peek до перехода, `local.rs:790-804`).
5. **Барьеры (обязательный гейт).** `TurnBarrierFsm` в `net.rs` (header 16-30)
   переносится **дословно**, с теми же таймаутами (retransmit 3 с, timeout 30 с)
   и тем же resume-54. Любое «улучшение» при переносе — регрессия 45/54. Требование
   ТЗ: «перенос без изменения поведения; покрыть тестами на таймауты и resume **до**
   удаления старого кода».
6. **`delta_since`**: запрет буферизации (Q20) ⇒ грубая дельта; полевой diff требует
   change-tracking (риск для 460 тестов, если делать сразу).
7. **Заимствование `StateRef`**: компилятор поймает, но рефактор
   `update_visual_units`/`hud` в `App` может оказаться объёмнее, чем кажется —
   см. этап A, пункт 4 (не удалять `state()` до этапа B).
8. **Детерминизм**: `seq`, `EventBus`, `outgoing` не должны попасть в
   `snapshot_bytes`/CRC; любая перестановка фаз должна давать байт-в-байт тот же
   снапшот.

---

## 7. Решения владельца (зафиксировано)

1. **Машина хода — внутри фасада.** `next_team()` и эмиссия `TurnEvent` — внутри
   `tick()`. `begin_round`/`end_round` на этапе A допустимо оставить публичными для
   конфигурационных сценариев; `next_team` — только внутри `tick()`.
2. **`submit` — только очередь; `StateSync` — немедленно.** Сдвиг `06`/лобби-команд
   на один тик — принятая цена; гейт — §6, риск 3.
3. **`StateDelta` — грубая сейчас, полевой diff — этап B.** Не делать сразу.
4. **`Dispatcher` — generic `GameUniverse<D = LocalDispatcher>`.** Мономорфизация,
   zero-cost; смена типа — создание нового `GameUniverse`, а не подмена `Box`.
5. **`NetworkDispatcher` — в `max_app`; `max_core` — только trait + `LocalDispatcher`.**
   `TurnBarrierFsm` живёт в `max_app::net`.
6. **`EventStream` — один буфер, N курсоров; обрезка по `min(cursor)` или по K тиков.**
   Мультикаст не нужен.
7. **`submit -> SubmitOutcome`; `ApplyOutcome` — в `GameEvent::CommandRejected`.**
   `CommandRejected` несёт полный контекст: `cmd`, `outcome` (enum целиком),
   `tick`, `source` (DroppedByRoute / RejectedBySimulation).

---

## 8. Передача в реализацию

После фиксации этого документа — пошаговое ТЗ Разработчику на **этап A**, с
обязательными гейтами:

- риск 3 (сдвиг на один тик) — как отдельный шаг с сравнением `snapshot_bytes` в
  двух режимах;
- риск 5 (перенос `TurnBarrierFsm` без изменения поведения) — тесты на таймауты и
  resume до удаления старого кода.

Этап B начинается только после зелёного этапа A.
