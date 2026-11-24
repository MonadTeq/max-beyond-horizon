# План: сетевой режим хоста (P2P) — цель 2

Версия: 0.2 (проект Архитектора) · 2026-11-24. Заменяет черновик v0.1 §3–§6
пошаговым ТЗ H1–H5.

Ключевой принцип: **не ломать `NetworkDispatcher`** (покрыт тестами), **не менять
барьерную FSM**, релей хоста — **детерминированный** (не менять/не переупорядочивать
пакеты).

## 1. Исходное состояние

- Фасад `GameUniverse` + `App<D: Dispatcher>` + `NetworkDispatcher` (FSM
  00→53→06→52→54→45, retransmit 3 с / timeout 30 с, resume-54) — готово (этапы A/B/C).
- Транспорт `max_net`: `Host::client()` и `Host::server(...)` уже есть
  (`enet.rs:279-412`); `Session` — только клиентский (один Peer, TP 00, без accept/
  мультипиров/релея).
- Оракул (Klei): хост = `TRANSPORT_SERVER`, гость = `TRANSPORT_CLIENT`
  (`remote.cpp:928-994`); лобби-авторитет 28…34 (`remote.cpp:2930-3014`); релей
  multicast (`remote.cpp:826-862`); барьеры/десинк (`remote.cpp:1558-1878`).
- **Решение `PLAN-host-mode.md §5` уже принято: первый шаг — «звезда через хост»,
  а не меш** (меш — фаза 2, при необходимости).

## 2. Архитектура

- **Хост = игрок + авторитет сессии**: `App<NetworkDispatcher>` (играет за свою
  команду) + `max_app::host` (драйвер-композиция: лобби-авторитет + релей +
  кворум/миграция) + `max_net::host::HostSession` (ENet-сервер, accept/TP/релей).
- **Гость = тот же клиент, что сегодня**, только без Horizon: `SessionHandle` к
  `--connect <host-ip:port>` напрямую.
- **Граничный принцип:** `max_net::host` — чистый транспорт (accept/TP/релей/
  сердцебиение), не знает `GameState`/`Dispatcher`; вся игровая политика — в
  `max_app::host.rs`. Релей **не интерпретирует** пакеты — copy-forward.
- **Детерминизм релея:** порядок релея хоста = глобальный порядок; ENet RELIABLE
  FIFO + пересылка в порядке приёма; свои команды — через `submit(Broadcast)`;
  релей не меняет байты/порядок, не дропает валидные APPL.

## 3. Миграция хоста (по sequence number)

- **Преемник = `min(player_node)`** среди достижимых выживших (player_node =
  порядок входа, разделяем всеми через пакет 30). Доп. `host_epoch: u32`
  (монотонный) для разрешения «двух хостов» после раздела.
- Протокол: гость не слышит heartbeat > `host_timeout` → кандидат P поднимает
  сервер → `Takeover{epoch+1, turn_counter, phase, counters, roster}` (пакет 56) →
  валидация позиции (не совпала = STOP) → ре-коннект TP 00 без нового лобби →
  resume-54 → возобновление барьеров.
- **Восстановление = возобновление с той же точки** (lockstep ⇒ состояние байт-в-байт
  у выживших). Полного SNAP нет ⇒ отставший из миноритарного раздела **не догоняет**
  ⇒ при отсутствии кворума — **полная пауза**, а не продолжение миноритарным разделом.

## 4. Кворум и split-brain

- `quorum = floor(N_active/2) + 1` (N=2→2, N=3→2, N=4→3).
- Хост закрывает барьер только когда (а) все Remote прислали счётчик и (б)
  `reachable_peers + 1 >= quorum`.
- Split-brain **не расходится по построению**: барьер требует единогласия всех
  Remote, без релея хоста гость не продвигается. Элиминация — host-authoritative и
  кворум-гейтированная.
- Детекция: guest молчит → `SessionPolicy` (`Pause` — дефолт; `EliminateAfter{ticks}`
  при кворуме, элиминация по тикам); host молчит → миграция; раздел → обе стороны
  стоят, после слияния — resume; два хоста → побеждает больший epoch (ничья — меньший
  player_node).

**`SessionPolicy`** (конфигурация сессии, сериализуется, передаётся гостям):
`Pause` (дефолт) | `EliminateAfter { ticks: u32 }` — счётчик `ticks_without_heartbeat`,
элиминация эмитит детерминированное `PlayerEliminated{team, sequence}` (не метку
времени).

## 5. Пошаговое ТЗ (H1–H5)

**Guardrails (все этапы):** не трогать `NetworkDispatcher`/FSM/таймауты; не менять
CRC/`snapshot_bytes`; релей copy-forward; `max_net::host` не импортирует `max_core`;
мономорфизация; комментарии английские; новые APPL-пакеты 55/56 — только транспортные
(не через `Dispatcher::route`).

### H1 — `max_net::host`: ENet-сервер, TP, релей (транспорт)
`HostSession::bind` (accept-цикл), per-peer TP 00/03, `HostHandle`/`HostEvent{sender,kind}`,
`send(PeerId|Broadcast, Packet)`, релей multicast-exclude-source, heartbeat-пакет 55.
UNIT-тесты: 2–3 ENet-пира (accept, TP, релей всем-кроме-источника, FIFO, drop неверифицированного).
**Гейт H1:** `cargo test -p max_net` зелёный; `NetworkDispatcher`/`max_app` не тронуты.

### H2 — серверное лобби хоста (28…34) + `--host/--listen` CLI
Порт серверного лобби (28→44; 29→entity_id по порядку входа + 47+30; 36/35/26→team_nodes;
READY≥min_players→32; 05→34). Флаги `--host`/`--listen`/`--host-port`/`--min-players`/
`--host-policy`. Гость: `--connect <host-ip:port>` без `--join`.
**Гейт H2:** два headless-процесса доходят до 34; `host_node`/`team_nodes`/`rng_seed` совпадают.

### H3 — хост как игрок + релей-арбитр (lockstep)
Игровой цикл хоста: композиция `App<NetworkDispatcher>` + `HostHandle`; generic-форма
драйвера `I: HostIo` (SessionHandle для гостя, HostHandle для хоста); семантика FSM дословно.
**Гейт (а):** host+guest 10 ходов нативно, 0 desync, digest совпадают, CRC 45 сходится.
**Гейт (б):** 3 клиента (host + 2 гостя) 10 ходов, гистограммы пакетов совпадают.

### H4 — миграция хоста + кворум/split-brain + SessionPolicy
Heartbeat/`host_timeout` (конфиг 5–300 с, дефолт 90 с, валидация), выборы
`min(player_node)`, `Takeover` (56), ре-коннект, resume-54, кворум-гейт,
`SessionPolicy` (Pause/EliminateAfter{ticks}), host-authoritative `TeamEliminated`.
**Гейт (в):** kill хоста → преемник, партия до 10 ходов, 0 desync.
**Гейт (г):** split-brain → обе стороны пауза, после слияния resume.
**Гейт (д):** элиминация при 3 (quorum 2) продолжает; при 2 (quorum 2) пауза.
**Гейт (детерминизм элиминации):** два клиента с разным дрейфом/задержкой heartbeat
элиминируют игрока в **один и тот же тик** (иначе `EliminateAfter` не годится).

### H5 — десинк-аудит, нативный smoke (UI — отдельно, в последний момент)
1. Реконнект (resume-54) — уже покрыт в H4; проверить гостевой реконнект в host-режиме.
2. Десинк-аудит 23/24/49 (`remote.cpp:880-1428`) на хосте; `--host` пишет `desync-turn*.txt`.
3. **UI `Host Network Game`/`Join Network Game` — ОТЛОЖЕНО (делается в самый последний
   момент, отдельная задача).**
4. **Нативный smoke** `host-smoke.sh` (+ `.ps1`): спавнит N нативных `max-brh` (headless)
   на 127.0.0.1, `--turns 10`, 2 и 3 клиента; assert 0 десинков + равные digest. Без
   Horizon/Docker/`live-smoke.sh`.
5. Паритет с C++ — опционально (гистограммы/порядок барьеров по protocol.md/barrier-flow.md).

**Гейт (е) DoD (без UI):** `cargo test` (все крейты) + H1–H4 гейты + `host-smoke.sh`
2× и 3× клиента = PASS + партия host+guest против ИИ 10 ходов 0 десинков.

## 6. Решения владельца (2026-11-24) и оставшиеся риски

**Решения (до старта H1):**

1. **Элиминация vs пауза.** Пауза по умолчанию (детерминизм, соответствие выделенному
   серверу). `SessionPolicy::EliminateAfter { ticks: u32 }` — опция сессии для
   соревновательных режимов; элиминация **по тикам, не по секундам** (секунды —
   недетерминированны). Политика сериализуется и передаётся гостям; элиминация
   эмитит детерминированное `PlayerEliminated{team, sequence}`.
2. **`host_timeout`** — `Duration`, дефолт 90 с, диапазон 5–300 с с валидацией;
   **отдельно от `elimination_timeout`** (это разные таймеры: пауза при потере хоста
   ≠ элиминация покинувшего игрока).

**Обязательный тест детерминизма элиминации (если `EliminateAfter`):** два клиента
с разным дрейфом часов/задержкой heartbeat элиминируют игрока **в один и тот же тик**;
иначе политика не годится.

**Оставшиеся риски:**

- Смена хоста = рестарт ENet-топологии (ре-коннект гостей без нового лобби) — гейт (в).
- Звезда-через-хост (первый шаг) vs меш оригинала (фаза 2) — решение принято (звезда).
- NAT/UPnP — в MVP только LAN/localhost.

> **P2P/host (H1–H5) — отдельный трек**: не влияет на приёмку этапа C (NetworkDispatcher)
> и на релизный гейт Horizon-smoke. Три параллельные линии; приёмку не смешивать.

## 7. Сводка ссылок

- Наши: `max_app/src/net/dispatcher.rs:351-563` (NetworkDispatcher), `:87-290` (FSM);
  `max_app/src/net.rs:1312-1508,1634-2076` (драйвер); `max_net/src/enet.rs:279-412`
  (Host::server), `session.rs:362-711` (Session/payload_team); `max_app/src/main.rs:147-186`;
  `max-brh-docs/architecture/client/{game-universe-facade,protocol,barrier-flow,plan-modes}.md`;
  `development/game-universe-stage-c.md` (шаблон ТЗ).
- Оракул Klei: `remote.cpp:928-994` (host/client), `:544-591` (SetupPlayers),
  `:826-862` (multicast), `:1558-1878` (барьеры/десинк), `:2930-3014,3528-3554` (лобби),
  `:225-245` (entity_id/sequence), `:880-1428` (десинк-аудит); `transport_udp_default.cpp:234-244,718-829`.
