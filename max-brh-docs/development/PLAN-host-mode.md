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
- Детекция: guest молчит → `Pause` (default) / `Eliminate` (при кворуме); host
  молчит → миграция; раздел → обе стороны стоят, после слияния — resume; два хоста →
  побеждает больший epoch (ничья — меньший player_node).

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

### H4 — миграция хоста + кворум/split-brain
Heartbeat/host_timeout, выборы `min(player_node)`, `Takeover` (56), ре-коннект, resume-54,
кворум-гейт, host-authoritative `TeamEliminated`, политика `pause|eliminate`.
**Гейт (в):** kill хоста → преемник, партия до 10 ходов, 0 desync.
**Гейт (г):** split-brain → обе стороны пауза, после слияния resume.
**Гейт (д):** элиминация при 3 (quorum 2) продолжает; при 2 (quorum 2) пауза.

### H5 — паритет, реконнект, UI, нативный smoke 2–3 клиента
Реконнект (resume-54), десинк-аудит 23/24/49, UI `Host`/`Join`, **нативный smoke**
`host-smoke.sh` (без Docker/Horizon, спавнит N нативных headless-клиентов, 2 и 3 клиента,
assert 0 desync + равные digest).
**Гейт (е) DoD:** все гейты H1–H4 + host-smoke 2×/3× + партия host+guest vs ИИ 10 ходов
0 desync + UI формы.

## 6. Риски и открытые вопросы (владельцу)

1. Смена хоста = рестарт ENet-топологии (ре-коннект гостей без нового лобби) — гейт (в).
2. `host_timeout` vs `REMOTE_RESPONSE_TIMEOUT` (90 с паузы) — конфиг `--host-timeout`.
3. Звезда-через-хост (первый шаг) vs меш оригинала (фаза 2) — решение уже принято (звезда).
4. **Элиминация vs пауза** (по умолчанию `pause`, оригинал элиминировал через 30 с) —
   решение владельца.
5. NAT/UPnP — в MVP только LAN/localhost.

## 7. Сводка ссылок

- Наши: `max_app/src/net/dispatcher.rs:351-563` (NetworkDispatcher), `:87-290` (FSM);
  `max_app/src/net.rs:1312-1508,1634-2076` (драйвер); `max_net/src/enet.rs:279-412`
  (Host::server), `session.rs:362-711` (Session/payload_team); `max_app/src/main.rs:147-186`;
  `max-brh-docs/architecture/client/{game-universe-facade,protocol,barrier-flow,plan-modes}.md`;
  `development/game-universe-stage-c.md` (шаблон ТЗ).
- Оракул Klei: `remote.cpp:928-994` (host/client), `:544-591` (SetupPlayers),
  `:826-862` (multicast), `:1558-1878` (барьеры/десинк), `:2930-3014,3528-3554` (лобби),
  `:225-245` (entity_id/sequence), `:880-1428` (десинк-аудит); `transport_udp_default.cpp:234-244,718-829`.
