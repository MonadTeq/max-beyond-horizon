# ТЗ: GameUniverse — этап B (полная инкапсуляция)

> Статус: **передано Разработчику**.
> Архитектурное решение —
> [`../architecture/client/game-universe-facade.md`](../architecture/client/game-universe-facade.md)
> (§6 «Этап B», §6 «Риски и обязательные гейты», §7). Предыдущий этап —
> [`../development/game-universe-stage-a.md`](../development/game-universe-stage-a.md)
> (выполнен: фасад `GameUniverse<LocalDispatcher>` уже существует в
> `max-brh/crates/max_core/src/universe.rs`, `App` владеет `universe`).
>
> **Роль исполнителя:** Разработчик (DeepSeek v4 Flash). При расхождении с
> архитектурным документом или сомнении в поведении — стоп, вопрос Архитектору
> (эскалация), не домысливать. Комментарии в коде — строго на английском; `.md`
> — на русском.

## 0. Цель и границы

**Цель этапа B:** полная инкапсуляция `GameState` за фасадом `GameUniverse`.
Наружу — только фасад + read-only `StateRef`. Прямые мутации `GameState` из
`max_app` и из интеграционных тестов исчезают; `NetworkDispatcher` (в
`max_app`) берёт на себя барьерную FSM; `max_net` умеет кодировать
`NetworkCommand`/`RemoteCommand`.

**В этапе B делается** (ADR §6 «Этап B», пункты 1–6):

1. `GameState` → приватный в модуле `universe`; мутирующие методы →
   `pub(crate)`; наружу — фасад + `StateRef`.
2. `NetworkDispatcher` в `max_app` (реализует `Dispatcher`, переносит
   `TurnBarrierFsm` из `net.rs` **дословно**); `max_net` добавляет
   `encode`/`decode` для `NetworkCommand`/`RemoteCommand`.
3. `local.rs`/`net.rs` → чистые потребители `EventStream`/`drain_outgoing`/
   `submit_remote` (машина хода — у фасада с этапа A; барьерная FSM — у
   `NetworkDispatcher`).
4. Тесты фаз 1–5 переписываются на фасад: `spawn_unit`/`set_order`/
   `write_unit`/`tick`/`snapshot_bytes` → `submit`/`tick`/`snapshot`;
   внутренние `mod tests` остаются (видят `pub(crate)`); интеграционные
   `tests/*.rs` переезжают на публичный API.
5. Удаление `state_mut()` из `App`; `into_state` убирается.
6. Полевой diff `StateDelta` (`DirtySet<UnitId>`) — **опционально**, отдельным
   финальным шагом с пометкой «по требованию» (решение владельца §7.3).

**Обязательные гейты** (как отдельные шаги, «не проверить в конце»):

- **Гейт 5 (барьеры):** `TurnBarrierFsm` переносится **дословно** (retransmit
  3 с, timeout 30 с, resume-54); тесты таймаутов и resume — **до** удаления
  старого кода; степперы `step_*_barrier` остаются вызовами `GameState` без
  изменения поведения.
- **Гейт 3 (сдвиг на тик):** все сценарии `06`/lobby/hot-seat в двух режимах
  (старый `enqueue`+`tick` vs новый `submit`+`tick`) со сравнением
  `snapshot_bytes` и компенсацией (N+1 тиков).
- **Детерминизм:** golden `snapshot_bytes`/`replay_digest`/CRC-пиннинги без
  изменений.

### 0.1 Порядок и зависимости (обязательно к соблюдению)

Порядок минимизирует риск и учитывает два факта Rust:

1. **`pub(crate)` — граница крейта.** `pub(crate)`-методы `max_core` не видны из
   `max_app` (другой крейт). Поэтому степперы/мутаторы, которые нужны
   `max_app` (`NetworkDispatcher`, меню, QA), после инкапсуляции **должны**
   проходить через публичный фасадный passthrough.
2. **E0446 «private type in public interface».** Как только `GameState`
   перестанет реэкспортироваться, методы `GameUniverse::state()/state_mut()`
   (`universe.rs:649/654`) и делегаты `App::state()/state_mut()`
   (`lib.rs:879/886`) перестанут компилироваться. Поэтому **финальный срез
   видимости** (приватные поля + `pub(crate)` + удаление `state_mut()`) — один
   неразрывный последний шаг, выполняемый только после того, как все вызовы
   уже переведены на новый фасадный слой.

Итоговый порядок шагов: **§2 (гейт 5, изолированный) → §3 (фасадный
конфиг-слой, аддитивно) → §4 (переезд тестов) → §5 (гейт 3) → §6 (перевод
`max_app` с `state_mut`) → §7 (финальный срез инкапсуляции) → §10 (опциональный
полевой diff)**.

### 0.2 Решения Архитектора по спорным местам ADR (фиксируется здесь)

- **А1 — видимость степперов.** ADR §2 (таблица «Где живёт барьерная логика»,
  строка 247–248) требует: на этапе B степперы `step_*_barrier` становятся
  `pub(crate)` и «доступны только фасаду», при этом FSM живёт в `max_app`
  (`NetworkDispatcher`). `pub(crate)` в `max_core` невидим из `max_app`, поэтому
  разрешение: фасад добавляет три **публичных passthrough**
  `GameUniverse::step_begin_turn_barrier` / `step_end_turn_barrier` /
  `step_crc_barrier` (возвращают новый счётчик), которые 1:1 оборачивают
  теперь-`pub(crate)` степперы. `NetworkDispatcher` шагает барьеры **только
  через фасад**, никогда не трогая `GameState`. Это единственное дополнение к
  мутирующей поверхности фасада, и оно — транспортный канал в том же классе,
  что `submit_remote`/`drain_outgoing`.
- **А2 — `Dispatcher::plan` vs асинхронная FSM.** `plan(&BarrierFacts)` —
  синхронный хук `tick()` (`universe.rs:579`); текущая FSM
  (`run_game_loop`, `net.rs:1824`) шагает барьеры **асинхронно** (await
  ретранов/таймаутов), вне `tick()`. Поэтому `NetworkDispatcher::plan()`
  возвращает `vec![]` (как `LocalDispatcher`); барьерные пакеты FSM доставляются
  транспорту драйвером `run_game_loop` (как сегодня, через `SessionHandle`), но
  собираются из DTO `NetworkCommand` и шагаются через фасад. `drain_outgoing`/
  `submit_remote` — канал **lockstep**-пути (`submit` → `tick` →
  `drain_outgoing`); предикаты согласия (`*_barrier_reached`) и счётчики
  читаются через `StateRef`/`BarrierFacts`.
- **А3 — конфигурационная поверхность фасада.** `spawn_unit`/`write_unit`/
  `set_order`/`set_order_data`/`write_presentation` и «настроечные» поля
  (`network_game`, `enemy_halt`, `victory_*`, `mission_*`, `play_mode`,
  `rng`-seed, `save_meta`) **не имеют** эквивалента в `Command` — это
  setup/конфиг, а не lockstep-мутация (ADR §6, риск 2). Они остаются `pub` как
  **документированный конфиг/тестовый слой фасада** (по аналогии с
  `GameState::begin_round`/`end_round`, оставленными `pub` для
  «конфигурационных сценариев», ADR §4). Настоящие lockstep-мутаторы (`tick`,
  `apply_pending_commands`, `enqueue`, `process_orders`, `manage_economy`,
  `next_team`, `begin_round`, `resume_round`, `end_round`, `enter_end_turn`,
  `finish_turn`, `step_*_barrier`, `check_end_turn`) — `pub(crate)`.

## 1. Обязательное чтение перед стартом

- [`../architecture/client/game-universe-facade.md`](../architecture/client/game-universe-facade.md) — §1–§7 полностью.
- `max-brh/crates/max_core/src/universe.rs` — текущий фасад (этап A), целиком.
- `max-brh/crates/max_core/src/game.rs` — `GameState`: структура (`:217-394`),
  мутаторы, `tick` (`:2738`), `snapshot_bytes` (`:2829`), барьеры (`:2502-2598`).
- `max-brh/crates/max_app/src/net.rs` — `TurnBarrierFsm` (`:1393-1583`),
  `BarrierPending` (`:1599-1630`), `run_game_loop` (`:1824-2069`),
  `handle_session_event` (`:2072`).
- `max-brh/crates/max_app/src/local.rs` — потребитель `TurnNotice`
  (`local_next_team`, `:822`), `start_local_game` (`:619`).
- `max-brh/crates/max_net/src/wire/{command,state}.rs` — `decode_command`/
  `encode_command` (`command.rs:677/702`), `StateSync`/`to_core`/`state_sync_for`
  (`state.rs:218/577/600`).

## 2. Шаг 1 — Гейт 5 (обязательный): `NetworkDispatcher` + перенос `TurnBarrierFsm`

> Изолированный шаг: делается **до** инкапсуляции, пока `GameState` ещё `pub`.
> Цель — перенести FSM без изменения поведения и доказать эквивалентность
> старого и нового кода, не задевая 460 тестов фаз 1–5.

### 2.1 `max_net`: encode/decode для `NetworkCommand`/`RemoteCommand`

Файл `max-brh/crates/max_net/src/wire/command.rs` (или новый
`wire/network.rs`, зарегистрировать в `wire/mod.rs`):

1. `pub fn decode_network_command(kind: u8, r: &mut Reader<'_>) -> WireResult<NetworkCommand>`
   и `pub fn encode_network_command(cmd: &NetworkCommand, w: &mut Writer) -> WireResult<()>`
   — тонкая надстройка: `NetworkCommand::Command(c)` → существующие
   `decode_command`/`encode_command` (`command.rs:677/702`); `NetworkCommand::StateSync(p)` →
   существующие `decode_state_sync`/`encode_state_sync` (`state.rs:286/393`).
2. Аналогично `decode_remote_command`/`encode_remote_command` для
   `RemoteCommand`.
3. **Недостающее звено:** `StateSync` (wire) уже имеет `to_core() ->
   max_types::events::StateSyncPacket` (`state.rs:577`), но **нет** обратного
   `from_core(StateSyncPacket) -> StateSync`. Добавить его (или кодировать
   `StateSyncPacket` напрямую), чтобы encode-направление фасадного DTO работало.
   `StateSyncPacket` описан в `max-brh/crates/max_types/src/events.rs:119`.
4. Экспортировать новые функции из `max-brh/crates/max_net/src/lib.rs`
   (`:55-59`).

Критерий: `cargo test -p max_net` зелёный; round-trip
`decode(encode(x)) == x` для всех вариантов `NetworkCommand`/`RemoteCommand`
(включая `StateSync` с `is_last` и пустым `units`).

### 2.2 `NetworkDispatcher` + дословный перенос FSM

Новый файл `max-brh/crates/max_app/src/net/dispatcher.rs` (или struct в `net.rs`).

1. **Дословно** перенести в модуль `NetworkDispatcher`:
   - `BarrierPhase` (`net.rs:1360-1384`),
   - `TurnBarrierFsm` (`net.rs:1393-1583`) — поля и все методы
     (`begin`/`complex`/`note_complex`/`end`/`sync`/`resume_state_sync`/`crc`/
     `*_reached`/`crc_matches`/`reached`/`mark_local_finished`/`on_turn_finished`),
   - `WaitOutcome` (`net.rs:1586-1592`), `BarrierPending` (`net.rs:1599-1630`),
     `BarrierPoll` (`net.rs:1632-1635`),
   - константы `BARRIER_RETRY`/`BARRIER_TIMEOUT` (`net.rs:1354-1357`, значения
     из `max_net::session::DEFAULT_BARRIER_REPEAT`/`DEFAULT_BARRIER_TIMEOUT`).
2. **Единственное допустимое механическое изменение** (см. §0.2 А1): параметр
   `&mut GameState` у `begin`/`end`/`crc` заменяется на `&mut GameUniverse<D>`
   (или `&mut GameUniverse<LocalDispatcher>`), а вызовы
   `state.step_*_barrier(...)` — на фасадные passthrough
   `universe.step_*_barrier(...)`. Логика (счётчики, фазы, пакеты, retransmit
   3 с, timeout 30 с, resume-54) остаётся **байт-в-байт** прежней.
3. `impl Dispatcher for NetworkDispatcher` (ADR §2):
   - `route`: команда, влияющая на симуляцию → `Broadcast`; барьеры (`00`/`01`/
     `52`/`45`/`53`/`54`) и локальные настройки → `Local`; запрещённый порядок →
     `Drop`.
   - `receive`: `RemoteCommand::Command` → `Replay`; `RemoteCommand::StateSync` →
     `ApplyStateSync`.
   - `advances_turns` → `false` (ход продвигает барьерная FSM, `net.rs:2020-2024`).
   - `plan` → `vec![]` (см. §0.2 А2).

### 2.3 Переключение драйвера сети на фасадный путь

В `max-brh/crates/max_app/src/net.rs`:

1. `run_game_loop` (`:1824-2069`), `pump_events` (`:1638`),
   `handle_session_event` (`:2072`) перестают звать
   `fsm.begin/end/crc(app.state_mut())` и `apply_state_sync(app.state_mut(), …)`
   (`:2118`). Входящие пакеты идут через `app.submit_remote(RemoteCommand::…)`;
   исходящие барьерные DTO — через фасад (`drain_outgoing` для lockstep-части;
   FSM-пакеты — в `SessionHandle`, как сегодня).
2. `initialize_game` (`net.rs:1188`, прямые `state_mut` в `:1256/1262`) и
   `run_lobby` (`:930`) — только через конфиг-слой фасада (см. §3).

### 2.4 Гейт 5 (обязательный, «не проверить в конце»)

**До удаления старого `TurnBarrierFsm`:**

1. Добавить тесты **таймаутов и resume** (сейчас покрытие отсутствует для
   `BarrierPending`): retransmit через `BARRIER_RETRY` (3 с), timeout-лог через
   `BARRIER_TIMEOUT` (30 с, один раз, с последующим ожиданием — как
   `net.rs:1618-1630`), resume-54 (`resume_state_sync`, существующий тест
   `net.rs:2848` — сохранить и перенести).
2. Прогнать старый `TurnBarrierFsm` и новый `NetworkDispatcher` на **одних и тех
   же** сценариях (`synthetic_state`, как `net.rs:2531/2875`) и сравнить:
   одинаковые счётчики после каждого `step_*_barrier`, одинаковые пакеты
   (`00`/`52`/`54`/`45`), одинаковые `reached()`/`crc_matches()`.
3. **Критерий:** пакет-в-пакет и счётчик-в-счётчик идентичны; таймауты
   (30 с) и resume-54 зелёные. Только после этого удалять старый
   `TurnBarrierFsm` (`net.rs:1393-1583`).

Критерий шага: `cargo test -p max_net` и `cargo test -p max_app` зелёные;
существующие тесты FSM (`net.rs:2219-2920`) проходят либо против нового
`NetworkDispatcher`.

## 3. Шаг 2 — Фасадный конфиг/тестовый слой и полнота `StateRef` (аддитивно)

> Пока `GameState` ещё `pub`; ничего не ломается. Готовит поверхность, на
> которую в §4 переедут тесты, а в §6 — `max_app`.

Файл `max-brh/crates/max_core/src/universe.rs`:

1. **Конфиг/тестовый слой `GameUniverse` (pub)** — passthrough к setup-мутаторам,
   которых нет в `Command` (ADR §6, риск 2): `spawn_unit` (`game.rs:1311`),
   `destroy_unit` (`:1562`), `write_unit` (`:880`), `set_order` (`:1626`),
   `set_order_data` (`:868`), `write_presentation` (`:841`), `unit_mut` (`:818`),
   `presentation_mut` (`:835`), `load_state` (`:547`), `set_save_meta`, а также
   сеттеры «настроечных» полей: `network_game` (`game.rs:350`), `enemy_halt`
   (`:373`), `play_mode` (`:258`), `game_state` (`:256`), `victory_type`/
   `victory_limit` (`:392/393`), `mission_index`/`mission_category`
   (`:387/389`), `settings_effects`/`disable_fire`/`real_time` (`:355-360`),
   `rng`-seed (`:226`), `save_meta` (`:383`), `team_units`/`teams` (`:222/224`).
   Конструкторы: `with_setup(map, team_kinds)` (обёртка `game.rs:486`),
   `Default`, `from_save` (уже есть, `universe.rs:687`).
2. **Полнота `StateRef`** (`universe.rs:366-456`): добавить read-only
   passthrough, которые нужны `max_app`/тестам: `order_data` (`game.rs:861`),
   `is_active_turn` (`:2634`), `upcoming_team` (`:2319`),
   `are_teams_finished_turn` (`:2293`), `begin/end/crc_turn_barrier` (`:2511/2502/2526`),
   `*_barrier_reached` (`:2576/2582/2588`), барьерные счётчики
   (`turn_indices`/`frame_sync_counter`/`next_turn_indices`/`state_sync_indices`/
   `team_data_crc16`, `game.rs:319-333`), `chat_log` (`:338`), `mission_supplies`
   (`:340`), `replay_digest_bytes` (`crc.rs:242`).
3. **Барьерные passthrough** (см. §0.2 А1): `step_begin_turn_barrier` /
   `step_end_turn_barrier` / `step_crc_barrier` (обёртки `game.rs:2542/2553/2564`,
   возвращают новый счётчик).

Критерий: `cargo check -p max_core` зелёный; существующие тесты фаз 1–5 **не
изменились** (фасад — аддитивно).

## 4. Шаг 3 — Переезд интеграционных тестов фаз 1–5 на фасад

> Самый объёмный шаг (ADR §6, риск 1: ~460 тестов). Разбить на подшаги;
> `GameState` в этот момент ещё `pub`, поэтому тесты можно переносить порциями,
> оставляя зелёным после каждой.

### 4.1 Хелперы `tests/support/mod.rs`

Файл `max-brh/crates/max_core/tests/support/mod.rs` (`:20-136`):

1. `game()` (`:20`) и `fog_game()` (`:37`) → возвращать
   `GameUniverse<LocalDispatcher>` (конфигурация через конфиг-слой §3).
2. `spawn` (`:44`), `order_move` (`:51`), `order_fire` (`:63`), `set_hovering`
   (`:128`), `start_turn` (`:99`) → переписать на фасадные passthrough.
3. `run_until` (`:75`), `ticks` (`:90`), `run_turns` (`:105`) → на
   `universe.tick()`.
4. `support/phase5.rs` (AI-harness) — аналогично.

### 4.2 Механический переезд `tests/*.rs`

Файлы `max-brh/crates/max_core/tests/*.rs` (≈50 файлов: `phase1_*`, `phase2_*`,
`phase3_*`, `phase4_*`, `phase5_*`, `ai_*`, `p7.rs`, `builder_cancel.rs`, …):

1. `GameState::new()/with_setup()/from_save()` → фасадные конструкторы §3;
   `state.spawn_unit(...)` → `universe.spawn_unit(...)`; `state.set_order(...)` →
   `universe.set_order(...)`; `state.write_unit(...)` → `universe.write_unit(...)`;
   `state.tick()` → `universe.tick()`; `state.snapshot_bytes()` →
   `universe.snapshot().snapshot_bytes()`.
2. Сценарии, где команда шла через `state.enqueue(...)` → `universe.submit(...)`
   **с компенсацией сдвига** (см. §5, гейт 3). Порядок-чувствительные тесты
   (`phase1_events.rs:81-222`) сохраняют порядок поступления.
3. **Внутренние `mod tests`** (inline `#[cfg(test)]` в `game.rs`, `events.rs`,
   `crc.rs` и др.) **не трогаются** — они видят `pub(crate)` после §7.

### 4.3 Golden-пиннинги

- `phase4_crc.rs`, `phase4_hash.rs`, `p7.rs`, `universe.rs`,
  `universe_turn.rs`, `universe_gate3.rs`, `universe_load_resume.rs` — golden
  `snapshot_bytes`/`replay_digest`/`state_checksum`/`state_digest` **не меняются**
  ни на байт. Любое расхождение — стоп → Архитектор.

Критерий: `cargo test -p max_core` зелёный (все фазы 1–5 **плюс** фасадные
тесты); golden-дайджесты без изменений.

## 5. Шаг 4 — Гейт 3 (обязательный): сдвиг на тик — все сценарии

**Задача:** доказать байт-в-байт эквивалентность старого и нового путей для
всех командных сценариев, с явной компенсацией сдвига.

1. Прогнать **все** сценарии `06`/lobby/hot-seat в двух режимах:
   - старый: `enqueue`/`apply` + `tick` (эталонный код до переезда);
   - новый: `submit` + `tick`.
2. Компенсация:
   - **queue-able** команды (lobby 13/12, hot-seat order-команды 08/38/41/…):
     новый прогон делает `N+1` тиков (команда применяется на тик позже);
     сравнить `snapshot_bytes` **с компенсацией**.
   - **`06` (завершение хода):** НЕ мигрирует на `submit` — применяется
     немедленно через `end_local_turn(team)` (`universe.rs:718`), без сдвига;
     сравнить байт-в-байт с `apply(06)+enter_end_turn+local_next_team`
     (решение §5 этапа A; ADR §1/§5). Отдельно проверить различимость
     `end_local_turn` (немедленный `06`) и `submit(06)` (очередь) — гейт
     различимости путей этапа A сохраняется.
3. **Критерий:** байт-в-байт совпадение после компенсации для всех сценариев.
   Если хоть один не сходится — это не «приемлемый сдвиг», а баг классификации
   `RoutedCommand::Local` vs `Broadcast` или неявная немедленная мутация в
   `local_end_turn`. Стоп → Архитектор.

## 6. Шаг 5 — Перевод `max_app` с `state()`/`state_mut()`

> Пока `GameState` ещё `pub`; механическая миграция вызовов с полной
> тип-информацией. После этого `App::state()/state_mut()` можно удалять.

Файлы `max-brh/crates/max_app/src/{lib,local,net,hud,ui,render,qa/runner,
saveload,preferences,soak,menu*}.rs` (≈298 обращений, см. `lib.rs`):

1. Чтения `self.state().X` → `self.state_ref().X` (или `EventStream` для
   событий); `self.state().snapshot_bytes()` (`lib.rs:3555`) → `state_ref().
   snapshot_bytes()`.
2. Прямые мутации через `state_mut()` (`lib.rs:886`) → конфиг-слой фасада (§3)
   или `submit(...)`. Особое внимание «настроечным» полям (ADR §6, риск 2):
   `start_local_game` (`local.rs:642`), `start_local_game_from_save`
   (`local.rs:1683`), `initialize_game` (`net.rs:1262`), `load_save`
   (`lib.rs:2763`), `preferences.rs:1889`.
3. `local.rs`: `local_next_team` (`:822`) уже потребитель `TurnNotice`;
   добить оставшиеся `state_mut().take_*` (`:979/1019`) и прямые `tick()`
   (`:1198/1231`) → через фасад/`EventStream`.
4. `net.rs`: после §2 не остаётся прямых `state_mut()` (кроме тестов; тесты
   `net.rs:2531-2919` перевести на фасад/`NetworkDispatcher`).
5. `App::flush_commands` (`lib.rs:912-914`, использует
   `universe.state_mut().apply_pending_commands()`) → фасадный конфиг-метод
   (например `GameUniverse::flush_commands`).

Критерий: `cargo check -p max_app` зелёный; `App::state()/state_mut()`
(`lib.rs:879/886`) больше **не вызываются** нигде (остаются только как
временные делегаты до §7).

## 7. Шаг 6 — Финальный срез инкапсуляции (последний)

> Один неразрывный шаг: приватность + `pub(crate)` + удаление
> `state()/state_mut()/into_state`. Выполняется только после зелёного §6.

Файл `max-brh/crates/max_core/src/game.rs`:

1. Поля `GameState` (`:217-394`) → `pub` снять; структура `GameState` —
   больше **не** реэкспортируется из `lib.rs` (`:167-171`); наружу — фасад.
2. Lockstep-мутаторы → `pub(crate)` (видны фасаду и inline `#[cfg(test)]`):
   `tick` (`:2738`), `apply_pending_commands` (`:2774`), `enqueue` (`:2779`),
   `process_orders` (`:1780`), `manage_economy` (`:1886`), `next_team` (`:2400`),
   `begin_round` (`:2336`), `resume_round` (`:2361`), `end_round` (`:2486`),
   `enter_end_turn` (`:2607`), `finish_turn` (`:2649`), `check_end_turn`
   (`:2809`), `step_*_barrier` (`:2542/2553/2564`), `take_*_events`
   (`:2695/2075/2083/2721`). Setup-мутаторы, покрытые конфиг-слоем §3, —
   `pub(crate)`. Read-only (`unit`/`unit_list`/`units_at`/`presentation`/
   `*_barrier`/`*_barrier_reached`/`state_checksum`/`state_digest`/
   `snapshot_bytes`) — остаются `pub` (их видит `StateRef`, `universe.rs:366`).
3. `max-brh/crates/max_core/src/universe.rs`: удалить `state()` (`:649`),
   `state_mut()` (`:654`), `into_state()` (`:698`); убедиться, что фасад — единственный
   публичный вход.
4. `max-brh/crates/max_app/src/lib.rs`: удалить `App::state()` (`:879`),
   `App::state_mut()` (`:886`); поля `universe`/`universe_events`/`turn_events`
   (`:549/552/559`) — единственные точки доступа.

Критерий: `cargo check` и `cargo test` (все крейты) зелёные; `GameState` не
упоминается ни в одном публичном API `max_core`/`max_app`; `state_mut()` нет ни
в `App`, ни в фасаде; `into_state` отсутствует.

## 8. Guardrails (запрещено / не раньше)

1. Не удалять старый `TurnBarrierFsm` до прохождения гейта 5 (§2.4).
2. Не менять таймауты/счётчики/фазы FSM (retransmit 3 с, timeout 30 с,
   resume-54) — перенос **дословно**.
3. Не шагать `step_*_barrier` в `tick()` (остаются за FSM/`NetworkDispatcher`;
   ADR §2, исправление к пункту 4).
4. Не переводить `06` на `submit` (немедленный путь `end_local_turn`; ADR §1/§5).
5. Не добавлять `seq`/`EventBus`/`outgoing` в `snapshot_bytes`/CRC; golden
   `snapshot_bytes`/`replay_digest`/`state_checksum`/`state_digest` не менять.
6. Не делать полевой diff `StateDelta` до §10 (решение владельца §7.3).
7. Финальный срез видимости (§7) — только после §6; не делать «частями».
8. Комментарии в коде — английский; `#![forbid(unsafe_code)]`; без
   `f32`/I/O/threads в `max_core`.

## 9. Критерии приёмки (DoD) и полная проверка

- `cargo check` — зелёный (все крейты).
- `cargo test -p max_core` — зелёный: фазы 1–5 (переехавшие на фасад) **плюс**
  фасадные тесты; golden-дайджесты без изменений.
- `cargo test -p max_app` — зелёный (включая перенесённые FSM-тесты и
  timeout/resume).
- `cargo test -p max_net` — зелёный (encode/decode round-trip).
- **Гейт 5** пройден: FSM перенесён дословно; timeout (30 с)/retransmit (3 с)/
  resume-54 покрыты до удаления старого кода; `step_*_barrier` — вызовы
  `GameState` без изменения поведения (через фасадные passthrough).
- **Гейт 3** пройден: `06`/lobby/hot-seat сходятся байт-в-байт после
  компенсации (N+1 для queue-able; без сдвига для `06`); пути
  `end_local_turn` vs `submit(06)` различимы.
- **Инкапсуляция** завершена: `GameState` вне публичного API; мутаторы
  `pub(crate)`; `state_mut()`/`into_state` удалены.
- **Детерминизм** не нарушен: golden `snapshot_bytes`/`replay_digest`/CRC-пиннинги
  идентичны эталону.

**Полная проверка** (как в [`../build.md`](../build.md)):
`bash ci.sh` (`max-brh-cicd/ci.sh`). Быстрая итерация —
`cargo test -p max_core` / `cargo test -p max_app`.

## 10. Шаг 7 (ОПЦИОНАЛЬНО, «по требованию») — полевой diff `StateDelta`

> Выполнять **только** если профилирование покажет, что передача полного
> `State` по сети дорога (решение владельца §7.3; ADR §3 «грубая дельта
> сейчас»). Не входит в обязательный объём.

1. В `max-brh/crates/max_core/src/universe.rs` расширить `StateDelta`
   (`:463-472`) полем `dirty: DirtySet<UnitId>`.
2. `DirtySet<UnitId>` — change-tracking по точкам мутации юнитов:
   `spawn_unit` (`game.rs:1311`), `write_unit` (`:880`), `destroy_unit`
   (`:1562`) — помечают затронутые `UnitId`; сбрасывается при взятии дельты.
3. `delta_since(seq)` заполняет `dirty`; `seq`/`checksum` по-прежнему **не**
   входят в CRC.
4. Критерий: золотые дайджесты без изменений; `delta_since` отдаёт только
   изменённые юниты; 460 тестов зелёные.

---

После зелёного §7 (и опционально §10) — вернуться к Архитектору за приёмкой
этапа B и планом на этап C (если применимо).
