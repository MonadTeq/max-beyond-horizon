# Протокол клиент ↔ Horizon ↔ headless (спецификация для крейта max_net)

> Статус: ЗАПОЛНЕНО из кода (`src/transport_*.cpp`, `src/remote.cpp`, `src/ws_client.cpp`,
> `max-brh-server/crates/*`). По этому документу можно написать сетевую часть
> `max_net` без чтения C++/Rust-исходников. Все имена полей, порядок байтов и
> числовые константы взяты из кода; места, где код не даёт ответа, вынесены в
> «Открытые вопросы» (§12).
>
> ⚠️ **Ключевая поправка к прежнему каркасу:** игровая часть — это **НЕ**
> client/server со снапшотами и дельтами. M.A.X. — детерминированный **lockstep**:
> каждый участник (headless-сервер и все клиенты) симулирует весь мир, по сети
> идут только приказы/события и барьерные синхронизации. Пакетов
> «snapshot»/«delta»/«SNAP» в протоколе нет; ближайшие аналоги — пакет 54
> (авторитетная синхронизация состояния юнитов на каждой CRC-проверке) и пакет 23
> (состояние одного юнита при аудите). Модель честно описана в §8.1.

## 1. Топология

```
max_net (Rust) ──WS JSON (ws://, TCP 8081)──► Horizon (max-brh-server, Rust)
    │                                            │  spawn/stop/restart, settings.json, тикет
    │                                            ▼
    │                                   max-port.exe --server (headless, C++)
    │                                   симуляция-арбитр, свой UDP-порт сессии
    └────────ENet/UDP (игра)────────────► на порт из game_joined
```

- **Horizon** — управляющая плоскость: WS-слушатель (по умолчанию `0.0.0.0:8081`,
  JSON-фреймы, см. `max-brh-server/config.toml`), плагины `max_lobby`
  (лобби/сессии) и `max_admin` (администрирование). Игровых данных через себя
  **не гоняет** (ретрансляция UDP — только план, фаза 3).
- **Headless max-port** — авторитет: один процесс на сессию, в своём каталоге
  (`sessions/<uuid>/`), со своим UDP-портом из `MAX_PORT_RANGE`
  (по умолчанию в коде `32000-32100`, в поставляемом `config.toml` —
  `31554-31600`), запускается командой `<dir>/max-port(.exe) --server` с
  переписанным `settings.json` (правила игры, порт, тикет, `server_mode=1`,
  `server_min_players`). Watchdog перезапускает процесс при падении
  (по умолчанию 3 рестарта, экспоненциальный backoff от 2 с).
- **Клиент** подключается по UDP только к headless-серверу (звезда, не меш).
  См. важное замечание о ретрансляции в §11 и §12.
- Каждая WS-коннекция Horizon получает серверный `player_id` (UUID v4);
  аутентификация на уровне WS отсутствует (только тикет для UDP и
  admin-токены для админ-событий).

## 2. Фазы соединения

1. **Лобби (WS)**: `max_lobby:list_games` → `game_list`; `max_lobby:create_game`
   (создатель получает тикет) → `game_created`; остальные —
   `max_lobby:join_game` → `game_joined` (`game_id`, `host`, `port`, `ticket`).
2. **UDP-хендшейк (ENet, канал 0)**: клиент подключается на `port` из тикета;
   обе стороны обмениваются пакетом `TP 00` (версии игры/ENet/протокола,
   тикет клиента, хеш игровых данных). При несовпадении — отказ `TP 03`
   с причиной и разрыв.
3. **Внутриигровое лобби (ENet, канал 1)**: 28 (запрос входа) → 44 (анонс хоста)
   → 29 (регистрация у хоста) → 47+30 (идентификатор ноды и полная информация
   о сессии) → 36/35 (имя игрока, выбор команды, READY) → сервер сам стартует,
   когда `server_min_players` игроков нажали READY: 32 (setup) → 05 (подтверждение
   каждого клиента) → 34 (старт).
4. **Игра**: 13 (seed ГПСЧ) → 17 (настройки игры) → 12 (выбор точки высадки,
   раунды) → 14 (высадка юнитов) → приказы (08/38/41/50/…), барьеры
   (01 — покадровый, 00 — начало хода, 52 — конец хода, 45 — CRC),
   синхронизация состояния (54), чат (18), пауза (39/40/46).
5. **Выход/таймауты**: 07 (заявка на выход, ответ 48); при зависшем пире
   барьерные ожидания снимают его через 30 с (`TEAM_TYPE_ELIMINATED`).

## 3. Сообщения (сводная таблица)

Легенда направлений: **C** = клиент (max_net), **H** = Horizon, **S** = headless-сервер.
«Кодирование»: JSON — текстовый WS-фрейм; bin LE — сырой порядок байтов хоста
(на всех поддерживаемых платформах x86/x86-64 это little-endian), без префикса
длины (длина — это длина ENet-пакета/WS-фрейма).

### 3.1. WS: клиент → Horizon (JSON `{"namespace","event","data"}`)

| # | Сообщение | Направление | Поля `data` | Ответ | Когда шлётся |
|---|---|---|---|---|---|
| 1 | `{"type":"hello",…}` | C→H | `protocol_version: u16`, `client: string?`, `dictionary_id: u32?` | `hello_ack` (или close 4000/4002) | опционально, первым фреймом |
| 2 | `max_lobby:list_games` | C→H | `{}` | `game_list` | вход на экран join |
| 3 | `max_lobby:create_game` | C→H | `name?, world?, timer?, endturn?, play_mode?, start_gold?, victory_type?, victory_limit?, min_players?, password?` | `game_created` | создание игры |
| 4 | `max_lobby:join_game` | C→H | `game_id: string(UUID), password?` | `game_joined` | вход в игру |
| 5 | `max_admin:list_sessions` | C→H | `admin_token?` | `sessions_listed` | админка |
| 6 | `max_admin:session_status` | C→H | `game_id, admin_token?` | `session_status` | админка |
| 7 | `max_admin:stop_session` | C→H | `game_id, admin_token?` | `session_stopped` | админка |
| 8 | `max_admin:restart_session` | C→H | `game_id, admin_token?` | `session_restarted` | админка |
| 9 | `max_admin:start_session` | C→H | `port?, name?, world?, timer?, endturn?, play_mode?, start_gold?, victory_type?, victory_limit?, min_players?, admin_token?` | `session_started` | админка |
| 10 | `system:ping` | C→H | любой JSON | `{"timestamp","message":"pong"}` | проверка живости |

### 3.2. WS: Horizon → клиент

| # | Сообщение | Направление | Поля | В ответ на |
|---|---|---|---|---|
| 11 | `{"type":"hello_ack",…}` | H→C | `type, protocol_version, format("json"), compression("none"\|"zstd"\|"lz4"), dictionary_id?` | #1 |
| 12 | `game_list` | H→C | `event:"game_list", games:[{id,name,world,status,port,players,min_players,password_protected}]` | #2 |
| 13 | `game_created` | H→C | `event, game:{…как в game_list…}, ticket` | #3 |
| 14 | `game_joined` | H→C | `event:"game_joined", game_id, host, port, ticket` | #4 |
| 15 | `error` | H→C | `event:"error", error` | любая ошибка лобби |
| 16 | `sessions_listed` | H→C | `event, sessions:[{game_id,name,port,state,dir}]` | #5 |
| 17 | `session_status` | H→C | `event, game_id, status:{state}` | #6 |
| 18 | `session_stopped` | H→C | `event, game_id, ok, error?` | #7 |
| 19 | `session_restarted` | H→C | `event, game_id, ok, new_game_id?, port?, error?` | #8 |
| 20 | `session_started` | H→C | `event, game_id, port` либо `{ok:false, error}` | #9 |
| 21 | `admin_error` | H→C | `event:"admin_error", error` | ошибка админки |
| 22 | pong (`system`) | H→C | `{timestamp, message:"pong"}` | #10 |
| — | close 4000–4008 | H→C | WS-close с кодом и reason (§5.2) | ошибки соединения |

### 3.3. UDP ENet, управляющий канал 0 (TP)

| # | Пакет | Направление | Поля (bin LE) | Назначение |
|---|---|---|---|---|
| 23 | `TP 00` (0x00) | C→S и S→C | `u8 0x00, u32 game_version, u32 enet_version, u16 protocol_version=0x0004, string ticket, string data_hash` | хендшейк версий/тикета/данных |
| 24 | `TP 03` (0x03) | S→C (и C→S) | `u8 0x03, u32 game_version, u32 enet_version, u16 protocol_version, u8 reason, string reason_text` | отказ с причиной, затем разрыв |

### 3.4. UDP ENet, игровой канал 1 (APPL), первый байт — тип

| # | Пакет | Направление | Кратко (полные раскладки в §8.7) |
|---|---|---|---|
| 25 | 00 | C→S (+пиры) | барьер начала хода: `team u16, counter u8` |
| 26 | 01 | C→S (+пиры) | покадровый sync: `team u16, counter u8` |
| 27 | 05 | C→S | подтверждение setup: `host_node u16` |
| 28 | 06 | C→S (+пиры) | завершил ход: `team u16` |
| 29 | 07 | C→S (+пиры) | выход из игры: `team u16, request_id u8, mode bool` |
| 30 | 08 | C→S (+пиры) | приказ юниту: `unit_id u16` + payload по типу приказа |
| 31 | 09 | C→S (+пиры) | состояние команды: `team u16, gold u32, gold_spent u32, research[8×12B], имя SmartString` |
| 32 | 10 | C→S (+пиры) | апгрейд типа юнита команды: `team u16, unit_type u16, 13×u16` |
| 33 | 11 | C→S (+пиры) | приказ базе (Complex): `team u16, complex_id u16, 6×i16` |
| 34 | 12 | C→S (+пиры) | снабжение миссии / точка высадки: `team u16, round u8, …` |
| 35 | 13 | S→C | seed ГПСЧ: `team u16=0, rng_seed u32` |
| 36 | 14 | C→S (+пиры) | высадка юнита: `team u16, unit_type u16, grid_x i32, grid_y i32` |
| 37 | 16 | C→S (+пиры) | запрос сейва: `team u16=0, file SmartString, title SmartString` |
| 38 | 17 | S→C (и хост) | настройки игры: `team u16=0, game_state u8, 38×i32` |
| 39 | 18 | C→S (unicast) | игровой чат: `sender_team u16, сообщение SmartString` |
| 40 | 20 | C→S (+пиры) | апгрейд конкретного юнита: `unit_id u16, 13×u16` |
| 41 | 23 | S→C (аудит) | состояние юнита: `unit_id u16` + Packet23Data |
| 42 | 24 | C→S (+пиры) | подтверждение аудита: `team u16, status bool` |
| 43 | 26 | C→S (+пиры) | выбор клана: `team u16, clan u8` |
| 44 | 28 | C→S (broadcast) | запрос входа в лобби: `node u16=0` |
| 45 | 29 | C→S (unicast) | регистрация у хоста: `node u16 (player_node хоста)` |
| 46 | 30 | S→C (unicast) | информация о сессии: узлы + настройки игры |
| 47 | 31 | C→S (+пиры) | уход хоста из лобби: `node u16` |
| 48 | 32 | S→C (broadcast) | setup/старт: `host_node u16` |
| 49 | 33 | C→S (+пиры) | чат лобби: `host_node u16, строка` |
| 50 | 34 | S→C (+пиры) | подтверждение старта: `host_node u16` |
| 51 | 35 | C→S (+пиры) | команда/READY: `host_node u16, player_node u16, team_slot u8, ready bool, имя string` |
| 52 | 36 | C→S (+пиры) | имя игрока: `player_node u16, имя string` |
| 53 | 37 | C→S (+пиры) | настройки игры (от хоста-человека): `player_node u16` + GameSettings |
| 54 | 38 | C→S (+пиры) | приказ движения с путём: `unit_id u16, …` |
| 55 | 39 | C→S (+пиры) | запрос паузы: `team u16, param u8` |
| 56 | 40 | C→S (+пиры) | снятие паузы: `team u16, param u8` |
| 57 | 41 | C→S (+пиры) | конец движения: `unit_id u16, mode bool, grid i16×2, speed u16, group_speed u16, shots u8, move_fraction u8` |
| 58 | 42 | C→S (+пиры) | отмена выбора точки: `team u16, param u8` |
| 59 | 43 | C→S (+пиры) | переименование юнита: `unit_id u16, имя SmartString` |
| 60 | 44 | S→C (unicast) | анонс хоста: `player_node u16, game_version u32, имя string` |
| 61 | 45 | C→S (+пиры) | конец хода + CRC: `team u16, next_turn_index u8, crc16 u16` |
| 62 | 46 | C→S (+пиры) | пауза/отложенные реакции: `team u16, state bool, counter u32` |
| 63 | 47 | S→C (+пиры) | регистрация ноды: `player_node u16` + NetNode |
| 64 | 48 | C→S (+пиры) | подтверждение выхода: `team u16, request_id u8` |
| 65 | 49 | S→C (+пиры) | аудит завершён: `team u16, param u8` |
| 66 | 50 | C→S (+пиры) | стоп юниту: `unit_id u16` |
| 67 | 51 | S→C (+пиры) | обнаружен десинк: `team u16, param u8` |
| 68 | 52 | C→S (+пиры) | барьер конца хода: `team u16, counter u8` |
| 69 | 53 | C→S (+пиры) | итоги баз на начало хода: `team u16, turn i32, count u16, count×(complex_id u16+6×i16)` |
| 70 | 54 | C→S (+пиры) | синхронизация состояния юнитов: `team u16, sync_index u8, is_last bool, count u16, count×(unit_id u16 + Packet23Data) [+ командная статистика]` |

Неиспользуемые/неопределённые номера APPL: 02–04, 15, 19, 21, 22, 25, 27
(в `remote.hpp` они существуют как константы, но ни отправки, ни обработчика нет).

## 4. Инварианты

- **Сервер — авторитет.** Все клиентские приказы применяются только после
  репликации; владелец команды — авторитет по состоянию **своих** юнитов
  (пакеты 23/54); headless-арбитр сравнивает CRC всех команд со своей
  симуляцией (пакет 45) и рассылает 51 при расхождении.
- **Детерминизм.** Каждый пир симулирует весь мир. По сети — только ввод и
  события; расхождение = десинк. Поэтому в хендшейке сверяется хеш игровых
  данных (`data_hash`), а версии игры/ENet/протокола обязаны совпадать.
- **Барьеры.** Покадровая синхронизация (пакет 01, раз в ~2 мс), барьеры начала
  хода (00), конца хода (52), CRC (45), синхронизации состояния (54): пир ждёт
  всех удалённых команд; повтор отправки каждые 3 с; таймаут ожидания 30 с,
  после чего зависшая команда помечается ELIMINATED (сервер сам никого не
  исключает — только ждёт и логирует).
- **Порядок.** ENet-каналы 0 и 1 с флагом RELIABLE: доставка гарантирована и
  упорядочена в рамках канала. Пакет ≤ 1440 байт (`TRANSPORT_MAX_PACKET_SIZE`).
- **Приём только после верификации.** APPL-пакеты от неверифицированного пира
  отбрасываются (до успешного TP-хендшейка). Минимальный размер APPL-пакета —
  3 байта (тип + u16-идентификатор).
- **Клиентские предпросмотры не меняют состояние:** подтверждение приказа —
  только его применение в локальной симуляции (и, косвенно, CRC 45 / синк 54).

## 5. WS-протокол Horizon (JSON)

### 5.1. Транспорт и оболочка

- Обычный RFC 6455 WebSocket. URL: `ws://<host>:<port>/` (root path).
  TLS (`wss://`) — только если в `config.toml` заданы `cert_path`/`key_path`
  (`[server.websocket]`). По умолчанию слушается `0.0.0.0:8081`, формат `json`,
  без сжатия.
- Клиентские фреймы маскируются (RFC 6455), серверные — нет.
  JSON-сообщения идут текстовыми фреймами (opcode 1); на листенере со сжатием
  каждый фрейм начинается с заголовочного байта: биты 0–1 = кодек
  (0 none, 1 zstd, 2 lz4), бит 2 = «сжат со словарём», биты 3–7 = 0.
  Листенер по умолчанию сжатия не имеет ⇒ заголовочного байта нет.
- Событие от клиента: `{"namespace": "max_lobby", "event": "list_games",
  "data": {…}}` → имя обработчика `namespace:event`.
- Опциональный handshake **первым** фреймом:
  `{"type":"hello","protocol_version":1,"client":"max_net/0.1","dictionary_id":null}`
  → ответ `{"type":"hello_ack","protocol_version":1,"format":"json",
  "compression":"none"}`. `protocol_version` не совпал → close 4000; `dictionary_id`
  не совпал с серверным (при сжатии со словарём) → close 4002. Hello можно не
  слать (тогда первое событие считается валидным сразу), но с ним раньше
  узнаёшь о несовместимости.
- Ответы плагинов — обычные JSON-объекты (см. таблицы ниже); оболочки
  `{namespace, event, data}` у ответов **нет** (плагины шлют свой JSON
  напрямую через `respond_json`).
- Ответов-подтверждений нет: «подтверждение» — это сам ответ события.
  Таймаута на уровне сервера нет; клиенту разумно держать свой (C++-клиент
  ждёт 5 с). Пушей нет: лобби — только запрос/ответ (свежесть — повторный
  `list_games`).
- Ограничения: фрейм ≤ `max_frame_size` (по умолчанию 64 КиБ) → иначе close
  4004; более `max_decode_errors` (5) неразбираемых фреймов → close 4003.

### 5.2. Коды закрытия (WS close, диапазон 4000–4999)

| Код | Причина |
|---|---|
| 4000 | protocol version not supported |
| 4001 | frames do not match the listener's settings |
| 4002 | compression dictionary mismatch |
| 4003 | too many invalid frames |
| 4004 | message too large |
| 4005 | kicked by server |
| 4006 | idle timeout |
| 4007 | server full |
| 4008 | client not reading fast enough |

### 5.3. `max_lobby`

Запросы (все поля `data`):

`max_lobby:create_game` — все поля опциональны; незаданные берутся из
`SessionConfig::default()`:

| Поле | Тип | По умолчанию |
|---|---|---|
| `name` | string | первые 8 символов UUID сессии |
| `world` | i64 | 0 (индекс мира, 0..=23) |
| `timer` | i64 | 30 (секунды на ход) |
| `endturn` | i64 | 15 |
| `play_mode` | i64 | 1 (1 = пошаговый, 0 = одновременный) |
| `start_gold` | i64 | 200 |
| `victory_type` | i64 | 0 |
| `victory_limit` | i64 | 100 |
| `min_players` | u32 | 2 (меньше 2 → `error` «a network game needs at least 2 players») |
| `password` | string | — (если задан: игра защищена, пароль = тикет сессии) |

`max_lobby:join_game`:

| Поле | Тип | Обязательность |
|---|---|---|
| `game_id` | string, UUID | да (не UUID → `error` «game not found») |
| `password` | string | только для защищённых игр (иначе `error` «invalid password») |

Ответы:

`game_list` / `game_created.game` — один элемент игры:

| Поле | Тип | Значение |
|---|---|---|
| `id` | string | UUID сессии |
| `name` | string | имя игры |
| `world` | i64 | индекс мира |
| `status` | string | `starting` \| `running` \| `exited` \| `stopped` |
| `port` | u16 | UDP-порт headless-процесса |
| `players` | число | сколько игроков зарегистрировано (считая создателя) |
| `min_players` | u32 | лимит/минимум |
| `password_protected` | bool | есть пароль |

`game_created`: `{"event":"game_created","game":{…},"ticket":"<пароль или "">"}`
— тикет выдаётся **создателю**.
`game_joined`: `{"event":"game_joined","game_id":"<uuid>","host":"127.0.0.1",
"port":<u16>,"ticket":"<…>"}` — выдаётся входящему. ⚠️ `host` сейчас
захардкожен `127.0.0.1` (см. §12).
`error`: `{"event":"error","error":"…"}`. Возможные тексты: `max_lobby is not
configured`, `game not found`, `invalid password`,
`game is not running (status: <…>)`, `a network game needs at least 2 players`,
ошибки старта процесса.
`join_game` допускается только когда `manager.health(game_id) == Running`.

### 5.4. `max_admin`

Токен: `admin_token = hex(HMAC-SHA256(admin_secret, "max-admin"))` — 64
строчных hex-символа, сравнение константное по времени. Без настроенного
`MAX_ADMIN_SECRET`/`admin_secret` **все** админ-события отклоняются
(fail-closed): `{"event":"admin_error","error":"admin authorization is
disabled: no admin_secret is configured"}`. Другие ошибки:
`missing admin_token`, `invalid admin_token`, `max_admin is not configured`,
`invalid max_admin:<event> payload: …` (неизвестные поля тоже ошибка —
`deny_unknown_fields`).

| Запрос | `data` | Ответ | Поля ответа |
|---|---|---|---|
| `list_sessions` | `{}` | `sessions_listed` | `sessions: [{game_id, name, port, state, dir}]`; `state` = starting/running/exited/stopped |
| `session_status` | `game_id` | `session_status` | `game_id`, `status: {state}`; неизвестная сессия → `state: "unknown"` |
| `stop_session` | `game_id` | `session_stopped` | `game_id`, `ok: bool`, `error?` |
| `restart_session` | `game_id` | `session_restarted` | `game_id`, `ok`, `new_game_id?`, `port?`, `error?` (id и порт меняются!) |
| `start_session` | как `create_game` + `port?` | `session_started` | `game_id`, `port` либо `ok:false, error` |

`port` в `start_session` — только предпроверка (должен лежать в
`MAX_PORT_RANGE` и не совпадать с работающей сессией); реальный порт всё равно
выделяет менеджер.

### 5.5. `system:ping`

Запрос: `{"namespace":"system","event":"ping","data":{}}`.
Ответ: `{"timestamp":<u64 ms>,"message":"pong"}`.

### 5.6. Bitcode-режим (необязателен для max_net)

Если листенер сконфигурирован `protocol = "bitcode"`, фреймы кодируются
`bitcode`-крейтом (схема зафиксирована `PROTOCOL_VERSION = 1`; крит. зависимость
от точной версии bitcode). Клиент **обязан** первым фреймом прислать
`ClientFrame::Hello {protocol_version, client, dictionary_id}`, сервер отвечает
`ServerFrame::HelloAck {protocol_version, codec, dictionary_id, event_table:
Vec<(u16 id, String name)>}`. Дальше: `Event {key: Id(u16)|Name(String),
payload: Vec<u8>}` (payload — bitcode `data` события), `GorcEvent {handle u32,
channel u8, event: EventKey, payload}`, `Ping {nonce u32}` → `Pong`; сервер
шлёт `ZoneEnter {handle, object_id [u8;16], object_type, channel, zone_data}`,
`GorcEvent {handle, channel, seq u16, event, payload}`,
`ZoneExit {handle, channel}`, `Batch {frames}`, `Disconnect {code u16, reason}`.
C++-клиент говорит только JSON — max_net для совместимости достаточно JSON.

## 6. Тикет

Тикет — это строка, которую клиент предъявляет в UDP-хендшейке (`TP 00`) в поле
`ticket`; headless-сервер сравнивает её со значением настройки
`server_session_ticket` из своего `settings.json` (пустая строка = сессия
открыта, проверки нет; иначе — точное строковое равенство).

Два формата (что вернул `game_joined.ticket`, то и шли):

1. **Legacy** (без `MAX_SESSION_SECRET`): пароль защищённой игры (для открытых —
   пустая строка `""`).
2. **Подписанный** (настроен `MAX_SESSION_SECRET`):
   ```
   v1.<session_id>.<player_id>.<unix_expiry>.<hex_hmac>
   ```
   - `session_id`, `player_id` — UUID в нижнем регистре, через дефис;
   - `unix_expiry` — целые Unix-секунды истечения (лобби выдаёт TTL 24 ч);
   - `hex_hmac` — 64 символа, нижний регистр, hex:
     `HMAC-SHA256(secret, "v1.<session_id>.<player_id>.<unix_expiry>")`;
   - проверка: форма → подпись (constant-time) → совпадение `session_id` →
     срок (`now >= expiry` ⇒ просрочен). Принимается и верхний регистр hex.

Что тикет даёт клиенту: идентификатор сессии (поле 1), привязку к игроку
(поле 2), ограничение срока; адрес/порт для подключения тикет **не** несёт —
они приходят отдельными полями `game_joined` (`host`, `port`). Безопасность:
тикет передаётся в открытом виде по UDP и лежит в `settings.json` сессии
(`server_session_ticket`) — см. §11.

## 7. UDP-хендшейк (ENet)

- **Библиотека: ENet 1.3.18** (`ENET_VERSION = 0x010312 = (1<<16)|(3<<8)|18`).
  Совместимая реализация обязательна: `renet` — другой протокол, не подходит;
  в Rust это крейт `enet` (или аналог ENet 1.3.x). `enet_initialize()` перед
  работой.
- Хост сервера: `enet_host_create(&addr{ENET_HOST_ANY, port}, 32, 2, 0, 0)`;
  хост клиента: `enet_host_create(nullptr, 32, 2, 0, 0)` + `enet_host_connect`
  на `host:port` из тикета. **2 канала**: 0 — TP (управление), 1 — APPL (игра).
  Все пакеты — `ENET_PACKET_FLAG_RELIABLE`. ENet-таймауты — стандартные
  (лимит 32, минимум 5000 мс, максимум 30000 мс).
- Порт по умолчанию для ручного запуска — `31554` (`host_port`); порт сессии
  Horizon — из `game_joined`.
- **Обе стороны** сразу после `CONNECT` шлют `TP 00` и принимают его от пира.

`TP 00` (все поля little-endian):

| Смещ. | Поле | Тип | Значение |
|---|---|---|---|
| 0 | type | u8 | `0x00` |
| 1 | game_version | u32 | `GAME_VERSION = (major<<16)\|(minor<<8)\|patch` из сборки сервера |
| 5 | enet_version | u32 | `0x010312` (ENet 1.3.18) |
| 9 | protocol_version | u16 | `0x0004` |
| 11 | ticket | string | клиент: из `game_joined`; сервер: пустая строка |
| … | data_hash | string | см. ниже |

- `string` = `u32` (длина в байтах, LE) + сырые байты без терминатора.
- `data_hash` = склейка двух hex-строк без разделителя:
  `SHA256(файл MAX.RES)` + `SHA256(файл PATCHES.RES)` (итого 128 hex-символов).
  Оба файла должны побайтово совпадать с серверными, иначе — отказ (это
  проверка детерминизма симуляции).
- Проверки на приёме (обе стороны): `game_version`, `enet_version`,
  `protocol_version`, `data_hash`; **сервер дополнительно** сверяет `ticket`
  с `server_session_ticket`, если он непустой. Любое несовпадение → отправить
  `TP 03` и `disconnect_later`:

`TP 03`:

| Смещ. | Поле | Тип | Значение |
|---|---|---|---|
| 0 | type | u8 | `0x03` |
| 1 | game_version | u32 | локальная версия (для диагностики) |
| 5 | enet_version | u32 | локальная |
| 9 | protocol_version | u16 | локальная |
| 11 | reason | u8 | `1` = version, `2` = session ticket, `3` = game data |
| 12 | reason_text | string | `"version"` / `"session ticket"` / `"game data"` |

Клиент по `TP 03` (если причина от сервера) выставляет код ошибки
(`TICKET_REJECTED` / `DATA_MISMATCH` / `VERSION_MISMATCH`) и **не
переподключается**; при чистом разрыве клиент переподключается до 5 попыток
(`MaximumConnectAttempts`), потом сдаётся с `PEER_CONNECT_FAILED`.

После успешной верификации: пир помечается VERIFIED, APPL-пакеты от него
принимаются; сервер регистрирует клиента в списке пиров, клиент переводит
состояние в CONNECTED. APPL-пакет от неверифицированного пира отбрасывается.

## 8. Игровой протокол UDP (канал 1)

### 8.1. Модель и сериализация

- **Модель: детерминированный lockstep.** Сервер и клиенты симулируют один и
  тот же мир; APPL-пакеты переносят приказы/события/барьеры, а не состояние
  мира. Единственное исключение — пакеты 23/54 (авторитетное состояние юнитов
  для аудита и коррекции при десинке; полноценного «снапшота» нет).
- **Фрейминг:** длина пакета = длина ENet-фрейма; первый байт — тип
  (`0x50…0x86` = `TRANSPORT_APPL_PACKET_ID + номер`). Затем поля подряд,
  **little-endian, сырой дамп памяти структур** (без выравнивания по
  struct-паддингу — пишется поле за полем через `operator<<`/`>>`).
  Единственное исключение — `NetAddress` в NetNode: он передаётся **целиком**
  (`packet << node.address`), поэтому в пакет попадает `sizeof(NetAddress)` =
  8 байт, включая 2 хвостовых байта struct-паддинга после `port` (см. §8.2).
- **Скалярные типы:** u8/i8 — 1 байт; u16/i16 — 2; u32/i32 — 4; bool — 1 байт;
  `UnitOrderType`, `UnitOrderStateType`, `TEAM_TYPE` — 1 байт; `ResourceID`
  (тип юнита) — 2 байта; `Point` — `i16 x, i16 y`; `PathStep` — `i8 x, i8 y`.
- **Строки:**
  - `std::string` → `u32 len` + байты (без NUL), лимит приёма 4096;
  - `SmartString` → `u16 len` + байты **включая завершающий NUL** (лимит 4096);
  - `std::vector<std::string>` → `u32 count` (≤1024) + элементы.
- **Массивы фикс. размера** (пишутся сырым дампом): `char[30]` — 30 байт,
  `char[4][30]` — 120 байт, `uint16_t[4]` — 8 байт, `ResearchTopic[8]` —
  96 байт (элемент = `i32 research_level, i32 turns_to_complete, i32
  allocation`), `uint32_t[UNIT_END=93]` — 372 байта.
- **Адресация:** у пакета есть транспортная таблица получателей (не на проводе;
  на приёме сервер кладёт адрес отправителя первым). Клиент шлёт всё серверу;
  получатель команды определяется по полю `team`/`unit_id` внутри пакета.
  Клиент распознаёт отправителя по таблице адресов (`Remote_GetSenderTeam`).
- Валидация: team-индекс обязан быть < 4 (`Remote_IsValidTeamIndex`), порядки
  юнитов < `ORDER_COUNT_MAX`, число элементов ≤ 1024, строки ≤ 4096; иначе
  пакет помечается malformed и игнорируется.

### 8.2. Внутриигровое лобби (после UDP-хендшейка)

| Шаг | Пакет | Кто | Поля |
|---|---|---|---|
| 1 | 28 | C→S broadcast | `type, node u16` (=0 от клиента) |
| 2 | 44 | S→C unicast | `type, player_node u16 (хоста), game_version u32, player_name string` — клиент принимает только при равенстве `game_version` |
| 3 | 29 | C→S unicast | `type, node u16` (= player_node хоста из 44) |
| 4a | 47 | S→C multicast | `type, player_node u16` + NetNode нового клиента |
| 4b | 30 | S→C unicast | см. ниже |
| 5 | 36 | C→S multicast | `type, player_node u16, player_name string` |
| 6 | 35 | C→S multicast | `type, host_node u16, player_node u16, team_slot u8 (0–3), ready bool, team_name string` |
| 7 | 33 | C→S multicast | чат лобби: `type, host_node u16, текст string` («имя: текст») |
| 8 | 26 | C→S multicast | `type, team u16, clan u8` |
| 9 | 32 | S→C broadcast | `type, host_node u16` — сервер стартует, когда `server_min_players` игроков заняли слоты и все READY |
| 10 | 05 | C→S unicast | `type, host_node u16` — каждый клиент подтверждает 32 |
| 11 | 34 | S→C multicast | `type, host_node u16` — старт игры |
| — | 31 | C→S multicast | `type, node u16` — хост-человек покинул лобби (клиенты сбрасывают сессию) |

**NetNode** (в 47 и 30): `entity_id u16, address {host u32, port u16, pad u8[2]},
name char[30] (сырые 30 байт), is_host bool`. `address` — сырой C++-агрегат
`NetAddress`: `sizeof` = **8** байт, то есть после `port` идут **2 байта
struct-паддинга**. Их значение на отправителе не определено (неинициализированная
память), но приёмник обязан их прочитать и проигнорировать — иначе все
следующие поля сдвигаются на 2 байта на каждый узел. Идентификатор ноды сервер
генерирует как `Randomizer(31991)+10` — это **не** UUID и не WS player_id.

**Пакет 30** (ответ на регистрацию; «добро пожаловать» в лобби):

| Поле | Тип |
|---|---|
| type | u8 (0x6E) |
| player_node | u16 — id, выданный этому клиенту |
| host_node | u16 — id ноды хоста (сервера) |
| team_nodes | u16[4] (сырой массив, 8 байт) |
| team_names | char[4][30] (120 байт) |
| node_count | u32 (≤4) |
| nodes | node_count × NetNode |
| game settings | см. GameSettings ниже |

**GameSettings** (в пакетах 30 и 37):

| Поле | Тип |
|---|---|
| mission_category | u32 (`MULTI`/`MULTI_PLAYER_SCENARIO`) |
| если MULTI: `multi_scenario_id` i32; если SCENARIO: `mission_hashes` vector\<string\> (SHA256-хеши файлов миссии) | |
| world_name | char[30] |
| ini_world_index, ini_play_mode, ini_timer, ini_endturn, ini_opponent, ini_start_gold, ini_raw_resource, ini_fuel_resource, ini_gold_resource, ini_alien_derelicts, ini_victory_type, ini_victory_limit | i32 × 12 |
| is_map_changed, is_multi_scenario | char × 2 (по 1 байту) |
| default_team_names | char[4][30] (120 байт) |
| rng_seed | u32 |

### 8.3. Старт игры, seed и выбор точки высадки

- **13** `S→C` (сразу после лобби; в headless-режиме шлёт сервер):
  `type, team u16 (=0), rng_seed u32` — `rng_seed = time(nullptr)` хоста.
- **17** `S→C`: `type, team u16 (=0), game_state u8,` затем **38 полей i32**:
  `world, game_file_number, play_mode, all_visible, quick_build, real_time,
  log_file_debug, disable_fire, fast_movement, timer, endturn, start_gold,
  auto_save, victory_type, victory_limit,
  raw_normal_low, raw_normal_high, raw_concentrate_low,
  raw_concentrate_high, raw_concentrate_seperation, raw_concentrate_diffusion,
  fuel_normal_low, fuel_normal_high, fuel_concentrate_low,
  fuel_concentrate_high, fuel_concentrate_seperation, fuel_concentrate_diffusion,
  gold_normal_low, gold_normal_high, gold_concentrate_low,
  gold_concentrate_high, gold_concentrate_seperation, gold_concentrate_diffusion,
  mixed_resource_seperation, min_resources, max_resources, alien_seperation,
  alien_unit_value`. Приёмник применяет настройки к своему settings.json
  (детерминизм!). Хост-человек шлёт 37 (то же самое, но `player_node` вместо
  `team=0`, и без `game_state`).
- **Выбор точки высадки (site select) — пакет 12** `C→S multicast`:

| Поле | Тип | Примечание |
|---|---|---|
| type | u8 (0x5C) | |
| team | u16 | |
| round | u8 | раунд перевыбора; пакеты чужого раунда буферизуются/отбрасываются |
| start_gold | u16 | стартовое золото снабжения |
| team_gold | u16 | |
| unit_count | u32 | число пар ниже |
| gold_spent_on_upgrades | u32 | |
| starting_position | Point (i16×2) | выбранная точка высадки |
| proximity_alert_ack | u8 | |
| unit_count × | {unit_type u16 (ResourceID), cargo u16} | снабжение миссии |

  Сервер/пиры ждут пакет 12 от **каждой** удалённой команды за текущий раунд;
  при конфликте близости точек — раунд увеличивается и все выбирают заново.
  Отмена выбора: **42** (`type, team u16, param u8`).
- **Высадка юнитов — пакет 14** `C→S multicast`: `type, team u16, unit_type u16,
  grid_x i32, grid_y i32` (координаты клетки карты).

### 8.4. Ввод: приказы юнитам, базам, апгрейды, конец хода, чат

**08 — приказ юниту** (`C→S multicast`): `type, unit_id u16` + payload, формат
которого зависит от *нового* порядка юнита (первые поля payload — сам порядок):

| Процессор (порядки) | Поля payload (по порядку) |
|---|---|
| P1: POWER_ON/OFF, EXPLODE, SENTRY, LAND, TAKE_OFF, LOAD, IDLE, HALT_BUILDING(_2), AWAIT_SCALING, AWAIT_TAPE_POSITIONING, LAY_MINE | `order u8, order_state u8, prior_order u8, prior_order_state u8, disabled_reaction_fire u8(bool)` |
| P2 (+ к P1): UNLOAD, REPAIR, RELOAD, UPGRADE | `+ parent_unit_id u16 (0 = нет), experience i16` |
| P3 (+ к P2): AWAIT, MOVE, FIRE, ACTIVATE, TRANSFER, AWAIT_STEAL_UNIT, AWAIT_DISABLE_UNIT, MOVE_TO_UNIT, MOVE_TO_ATTACK | `+ move_to_grid_x i16, move_to_grid_y i16, fire_on_grid_x i16, fire_on_grid_y i16, enemy_unit_id u16 (0 = нет), transfer_cargo i16, stealth_dice_roll u8` |
| P4 (+ к P3): TRANSFORM, BUILD, CLEAR, DISABLE | `+ repeat_build u8(bool), build_time u8, build_rate u16, build_count u32, build_count × unit_type u16` |
| P5 (+ к P3): NEW_ALLOCATE | `+ total_mining u8, raw_mining u8, fuel_mining u8, gold_mining u8` |
| P0: ORDER_17 | пусто |

  Приёмник валидирует `order`/`prior_order` < ORDER_COUNT_MAX и применяет
  приказ к локальной симуляции.

**38 — движение с путём**: `type, unit_id u16, order u8, order_state u8,
grid_x i16, grid_y i16, move_to_grid_x i16, move_to_grid_y i16,
group_speed u16, steps_count u16`, затем:
- воздушный юнит (шагов ровно 1): `end_x i16, end_y i16, distance_x i32,
  distance_y i32, euclidean_distance i16, speed u16, move_fraction u8,
  max_velocity u8`;
- наземный: `steps_count × PathStep {x i8, y i8}`.
  (Приёмник переставляет юнит на клетку владельца — `Remote_RelocateUnit`.)

**41 — завершение движения**: `type, unit_id u16, mode u8(bool),
grid_x i16, grid_y i16, speed u16, group_speed u16, shots u8, move_fraction u8`.

**50 — аварийный стоп**: `type, unit_id u16`.

**43 — переименование**: `type, unit_id u16, name SmartString`.

**11 — приказ базе (Complex)**: `type, team u16, complex_id u16,
material i16, fuel i16, gold i16, power i16, workers i16, buildings i16`.

**53 — итоги баз на начало хода** (владелец команды оптимизирует производство,
пиры реплицируют): `type, team u16, turn i32, complex_count u16, complex_count ×
{complex_id u16, 6×i16}`.

**10 — апгрейд типа юнита команды**: `type, team u16, unit_type u16`, затем
13 значений `u16` в фиксированном порядке: `turns, hits, armor, attack, speed,
range, rounds, move_and_fire, scan, storage, ammo, attack_radius, agent_adjust`.
**20 — апгрейд конкретного юнита**: `type, unit_id u16` + те же 13 × u16.

**Конец хода**: клиент шлёт **06** (`type, team u16`) и барьерный **52**
(`type, team u16, counter u8` — увеличивается каждый ход; повтор каждые 3 с,
ожидание остальных до 30 с). Сервер-арбитр барьер сам не шлёт: он ждёт
согласия всех команд (все прислали 52 с одинаковым счётчиком). Затем
синхронизация состояния (54) и CRC-барьер (**45**: `type, team u16,
next_turn_index u8, crc16 u16`; каждый владелец шлёт CRC своих юнитов;
сервер сравнивает со своей симуляцией; несовпадение → **51**
`type, team u16, param u8` — «десинк», клиенты пишут desync-отчёты).

**Чат (в игре)**: **18** unicast адресату: `type, sender_team u16,
message SmartString`.

**Пауза**: **39** (запрос, `type, team u16, param u8`), **40** (снятие, то же),
**46** (`type, team u16, state u8(bool), counter u32` — синхронизация
отложенных реакций).

**Сейв**: **16** `type, team u16(=0), file_name SmartString, file_title
SmartString`; имя обязано матчить `save<1-3 цифры>.<dta|tra|cam|hot|mlt|dmo|sce|mps>`
(иначе отклоняется).

**Выход**: **07** `type, team u16, request_id u8, mode u8(bool)`; получатель
отвечает **48** `type, team u16, request_id u8`. Разрыв без предупреждения:
через 30 с барьерных таймаутов команда помечается ELIMINATED (сервер сам не
исключает, но перестаёт её учитывать).

### 8.5. Состояние: аудит и синхронизация (аналог «снапшота»)

**Packet23Data** — каноническая сериализация состояния юнита (используется в
23 и 54):

| Поле | Тип |
|---|---|
| orders | u8 |
| state | u8 |
| prior_orders | u8 |
| prior_state | u8 |
| disabled_reaction_fire | u8 (bool) |
| parent_unit_id | u16 (0 = нет) |
| experience | i16 |
| move_to_grid_x, move_to_grid_y, fire_on_grid_x, fire_on_grid_y | i16 × 4 |
| enemy_unit_id | u16 (0 = нет) |
| transfer_cargo | i16 |
| stealth_dice_roll | u8 |
| total_mining, raw_mining, fuel_mining, gold_mining | u8 × 4 |
| build_time | u8 |
| build_rate | u16 |
| unit_type | u16 (ResourceID) |
| unit_id | u8 |
| grid_x, grid_y | i16 × 2 |
| team | u8 |
| hits, speed | u16 × 2 |
| shots | u8 |
| storage | i16 |
| ammo | u8 |

**23** `S→C` (десинк-аудит, по одному юниту): `type, unit_id u16` +
Packet23Data. Клиент сверяет со своей копией и отвечает **24**
`type, team u16, status u8(bool)`; по завершении аудита — **49**
`type, team u16, param u8`.

**54 — синхронизация состояния команды** (владелец шлёт авторитетное состояние
своих юнитов на каждой десинк-проверке; пиры применяют поверх своего мира —
это ближайший аналог «снапшота»; полного состояния карты нет):

| Поле | Тип |
|---|---|
| type | u8 (0x86) |
| team | u16 |
| sync_index | u8 — номер проверки (отличает раунды) |
| is_last | u8 (bool) — последний чанк команды |
| count | u16 (≤ 32 юнитов на чанк) |
| count × | {unit_id u16, Packet23Data} |
| если is_last: gold u32, gold_spent_on_upgrades u32, research_topics 96 B (8×{i32×3}), number_of_objects_created u16, unit_counters 372 B (93 × u32) | |

Приёмник применяет различия (приказы, позиции, родитель/цель, hp и т.д.),
удаляет юнитов, которых нет у владельца, и выравнивает счётчики id.

### 8.6. Барьеры — сводка

| Барьер | Пакет | Поля | Период повтора | Таймаут ожидания |
|---|---|---|---|---|
| Покадровый (real-time pacing) | 01 | `team u16, counter u8 (6 бит)` | каждый тик (~2 мс) | 30 с |
| Начало хода | 00 | `team u16, counter u8` | 3 с | 30 с |
| Конец хода | 52 | `team u16, counter u8` | 3 с | 30 с |
| CRC/десинк | 45 | `team u16, next_turn_index u8, crc16 u16` | 3 с | 30 с |
| Синхронизация состояния | 54 | `team u16, sync_index u8, …` | 3 с | 30 с |
| Итоги баз (старт хода) | 53 | `team u16, turn i32, …` | — | 30 с |

Сервер-арбитр в барьерах не участвует как команда: он ждёт, пока **все**
удалённые команды пришлют одинаковое значение счётчика, строго большее
значения предыдущего завершённого барьера (`Remote_ServerWaitBarrier`).
Покадровый барьер сервер не ждёт вовсе (сам тикает по игровому таймеру).

## 9. Реконнект / SNAP

Честно: **SNAP-пакета в протоколе нет**, а C++-эталон игрового реконнекта не
реализует: `game_manager.cpp` не содержит `HandleClientDisconnect`, сервер не
хранит per-client снапшот и не шлёт его на повторный `TP 00`. Но транспортный
разрыв можно пережить, потому что lockstep-барьеры не дают партии уйти вперёд
без команды вернувшегося клиента.

### 9.1 Что делает Rust-клиент (`max_net/session.rs`, `max_app/net.rs`)

1. `SessionEvent::Disconnected` → `ReconnectPolicy` (`--reconnect-attempts`,
   `--reconnect-timeout`): новый ENet connect + `TP 00` **с тем же тикетом и
   тем же `data_hash`**, которыми сервер уже принял клиента. Horizon
   `JoinGame` повторно не вызывается; лобби 28–34 не повторяется.
2. Состояние не сбрасывается. Это корректно для lockstep: пока клиент был
   offline, пиры/сервер ждали его барьер (00/52/54/45), поэтому локальные
   `turn_counter`, индексы и юниты — точка продолжения.
3. После `TP 00` клиент сразу публикует **resume-54** — полное состояние
   **своей** команды (`state_sync_for`, последний завершённый `sync_index`,
   юниты + хвост экономики/счётчиков). Сервер и пиры применяют его как
   авторитетное (`Remote_ReceiveNetPacket_54`); встречные 54 пиров исправляют
   удалённые команды у вернувшегося. Барьерный пакет текущей фазы
   переотправляется штатным 3-секундным трекером; после 52/54/45 ход
   продолжается, `turn_counter` и `state_checksum` должны совпасть.
4. `reconnect_pending_state=false` в итоговом отчёте означает, что сессия
   **продолжена**, а не только транспорт восстановлен.

### 9.2 Пределы (важно)

- **Нет полного SNAP.** 54 содержит только юнитов/золото/исследования/счётчики
  id **одной команды**. Карта, здания-комплексы, очередь производства и
  пропущенные приказы 38/08 отдельным снимком не пересылаются. 53 восстанавливает
  итоги комплексов на ближайшем начале хода, 54 — юнитов и экономику.
- **Своя команда — авторитетна.** Resume-54 может откатить на сервере/пирах
  изменения своей команды, которые те успели применить по пропущенным приказам.
  Это следствие контракта «владелец диктует состояние своих юнитов»; потерянные
  команды **другой** команды не воспроизводятся, их видимый эффект снимается
  коррекцией 54.
- **Окно ~30 с.** C++-пиры после `REMOTE_RESPONSE_TIMEOUT` помечают команду
  `ELIMINATED` и перестают ждать её барьеры; тогда 54 от вернувшегося уже не
  принимается. Rust-пиры команду не исключают и ждут дольше, но для mixed-сессии
  с C++ окно ограничено 30 с.
- **Разрыв в лобби** не восстанавливается: сервер уже не в `Remote_GameState=1`;
  клиент завершает прогон с диагностикой. Это не игровой реконнект.
- **Нет spectator.** Типа `TEAM_TYPE_SPECTATOR` нет (`enums.hpp:164-168`),
  `Remote_SetupPlayers` требует `GameManager_PlayerTeam = player_team`, лобби
  раздаёт только 4 командных слота, а поздний вход не получает снапшот. Клиент
  без команды либо займёт чужой/компьютерный слот, либо не сможет
  инициализировать мир. Нужны серверные изменения (`team=-1`, отдельный
  spectator-флаг, релей 54/53 наблюдателю) — без них пункт нереализуем.

### 9.3 Чем проверено

- `max_app/src/net.rs::three_clients_survive_disconnect_and_converge_via_state_sync`
  — три клиента, мок-релей: B отключён, A/C меняют копию его юнита, после
  возврата B публикует 54, A/C принимают, встречный 54 исправляет B; 52/54/45
  сходятся, CRC и `turn_counter` совпадают.
- `resume_state_sync_publishes_the_last_completed_index` — индекс/состав 54.
- Живые `build/scripts/live-smoke-3.sh` (3 клиента × N, релей, 00/52/54/45,
  CRC) и `live-smoke-mixed.sh` (Rust×2 + C++ desync-детектор; C++-клиент
  headless недоступен — см. §12).

## 10. Проверка контракта

**Smoke через Horizon + headless (Docker или локально):**

1. Собрать/запустить Horizon (см. `max-brh-server/README.Docker.md`,
   `Dockerfile`, `compose.yaml`). По умолчанию он слушает WS на `0.0.0.0:8081`,
   грузит плагины `max_admin` и `max_lobby`, читает `config.toml` (секция
   `[max]`: путь к headless-бинари, `template_dir`, диапазон портов, секреты).
2. WS-драйвер: `python build/horizon-smoke.py create <name> [world]
   [min_players]`, `python build/horizon-smoke.py list`, `python build/
   horizon-smoke.py join <game_id> [password]` — печатает JSON-ответы
   (`game_created`, `game_list`, `game_joined`). Админ-драйвер:
   `python build/horizon-admin.py`. Пример успешного прогона —
   `max-brh-server/horizon-smoke.log`.
3. Из `game_joined` взять `port` и `ticket`, подключиться ENet к
   `<host>:<port>` и выполнить §7 + §8.2. Маркеры успеха в логах headless
   (`sessions/<uuid>/ai_log.txt`, `stdout.txt`): `SERVER: entry`, вход клиента
   (пакеты 28/44/29/30/35/32/05/34), `Main game loop`.
4. Автоматизация GUI-клиента (опционально): `build/horizon-smoke-driver.ps1`
   (один клиент через экран join), `build/horizon-pairtest.ps1` (два клиента,
   ходы до N), `build/horizon-turns.ps1`, `build/server-pairtest.ps1`,
   `build/server-disconnecttest.ps1`, `build/server-aitest.ps1`.
5. Негативные проверки: неверный `ticket` → `TP 03` reason 2 («session
   ticket»), клиент показывает «The server rejected the session ticket»;
   неверная версия → reason 1; чужие `MAX.RES/PATCHES.RES` → reason 3.
6. Гейт-тест Rust: `max_session` (crates/max_session/src/lib.rs) запускает
   реальный headless при `MAX_SESSION_TEST_EXE` и ждёт `ai_log.txt` до 60 с.

**Самопроверка max_net:** реализовать ENet-хендшейк (§7) и лобби (§8.2) и
прогнать против живого headless; затем полный флоу до первого хода
(пакеты 12/14/08/52/45) и сверить, что в `ai_log.txt` сервера нет
«Desync check failed» и не появилось `desync-*.txt`.

## 11. Замечания для max_net

- **Порядок байтов:** весь UDP-протокол — little-endian (сырые дампы полей;
  на x86/x86-64 это совпадает с нативным). Никаких конверсий в коде нет —
  max_net на любом хосте обязан писать LE явно.
- **Нет префикса длины на уровне игры:** длина = размер ENet-фрейма; парсить
  строго последовательно, любое чтение за концом = malformed (C++ помечает
  пакет и игнорирует). Строки: `std::string` — u32-длина без NUL,
  `SmartString` — u16-длина **с** NUL; не перепутать (в пакете 33/35/36/44 —
  `std::string`; в 09/16/18/43 — `SmartString`).
- **ENet обязателен, и именно 1.3.18-совместимый:** в хендшейке сверяется
  `enet_version == 0x010312`. renet/WebRTC-подобное не подойдёт. Каналы:
  0 — управление, 1 — игра; всё RELIABLE. MTU/пакет: ≤ 1440 байт — большие
  данные (состояние команды в 54) дробятся на чанки по 32 юнита.
- **Версии и данные:** `game_version` (u32 из сборки), `protocol_version`
  (0x0004), `data_hash` (SHA256(MAX.RES)‖SHA256(PATCHES.RES), hex) обязаны
  совпасть — иначе жёсткий отказ. Для max_net нужны те же файлы данных, что у
  сервера; хеш вычислять один раз при старте.
- **Тикет:** поле `ticket` пакета TP 00 шлёт только клиент; сервер отвечает
  пустой строкой. Открытая сессия — тикет `""`. Подписанный тикет не парсить
  жёстко: формат `v1.<uuid>.<uuid>.<unix>.<64hex>` (TTL 24 ч); клиенту
  достаточно хранить строку целиком и пересылать её.
- **Безопасность:** (а) тикет идёт открытым текстом по UDP — канал до
  headless должен быть доверенным (VPN/локальная сеть); (б) пароль защищённой
  игры и есть тикет — не логировать; (в) `game_joined.host` сейчас
  захардкожен `127.0.0.1` — для удалённого сервера брать host из адреса
  Horizon-коннекции/конфига, порт — из ответа; (г) replay-защиты у тикета нет
  (только TTL); (д) admin-токен — HMAC-SHA256(secret, "max-admin") в hex,
  без секрета админка недоступна.
- **Звезда и реле:** клиент шлёт все пакеты серверу (транспорт сам стирает
  таблицу адресов у клиента). Сервер релеит игровые пакеты всем остальным
  верифицированным пирам (`TransportUdpDefault_RelayApplPacket`), не эхо-шлёт
  отправителю и не релеит лобби-пакеты. Проверено `live-smoke.sh` (2 клиента)
  и `live-smoke-3.sh` (3 клиента).
- **Барьеры:** после приказа ждать соответствующий барьер/подтверждение;
  повторять свои барьерные пакеты каждые 3 с; не ждать от сервера собственного
  барьерного пакета (арбитр их не шлёт); таймауты 30 с — после них команда
  выбывает.
- **Идентификаторы:** team ∈ [0,4); `entity_id` ноды — u16 от сервера;
  `unit_id` юнита — u16; `player_id` Horizon (UUID) в UDP-протоколе нигде не
  фигурирует.
- **Детерминизм клиента:** max_net обязан применять приказы в том же порядке
  и так же, как C++-клиент, иначе — CRC-десинк (45/51). Сверка CRC — лучший
  интеграционный оракул.
- **Падения сессии:** headless может упасть/перезапуститься (watchdog):
  порт сохранится, но игровое состояние начнётся заново (новая сессия) —
  клиент должен переживать разрыв и заново проходить §8.2.
- **Тайминги клиента-эталона:** сервис-тик ENet 10 мс; покадровый барьер ~2 мс
  (24 FPS-тик); повторы барьеров 3 с; индикатор ожидания — с 3 с; таймаут
  барьера 30 с; таймаут WS-запроса (C++-клиент) 5 с.

## 12. Открытые вопросы

1. **Ретрансляция сервера (мультиплеер через headless) — реализована.**
   `TransportUdpDefault_RelayApplPacket` пересылает игровые пакеты всем
   верифицированным пирам, кроме отправителя, и не релеит лобби 05/26/28..37/44/47.
   Проверено `live-smoke.sh` (2×N) и `live-smoke-3.sh` (3×N: у каждого клиента
   видны 00 двух других команд, CRC сходится).
2. **`game_joined.host` захардкожен `127.0.0.1`** в `max_lobby` — для
   удалённых серверов нужно решение (адрес из конфига/хедера запроса).
3. **Игровой реконнект — частично, без SNAP.** Транспорт + resume-54 и
   продолжение барьеров реализованы (§9.1). Полного снимка карты/экономики и
   replay пропущенных приказов нет; C++-клиент после 30 с помечает команду
   ELIMINATED. Для «идеального» реконнекта нужен новый SNAP-пакет (полный
   `GameState`/снапшот всех команд) или серверный буфер релея на время разрыва.
4. **Нет пушей лобби:** список игр статичен до следующего `list_games`;
   GORC-репликация сессий из плана не реализована.
   Смежная проблема: **нет headless C++-клиента** (`src/main.cpp` умеет только
   `--server`; GUI-клиент требует X11 и ручного меню), поэтому mixed-сессия
   Rust+C++ сейчас не автоматизируется и проверяется через C++-сервер как
   desync-оракул (`live-smoke-mixed.sh`, PARTIAL PASS).
5. **Формат `Complex` в 11/53** задокументирован по коду (id + 6×i16), но
   семантика «итогов» (материал/топливо/золото/энергия/рабочие/здания) и
   валидация приёмника — игровая логика `teamunits.cpp`; для приказов
   строительства max_net должен воспроизвести её точно (иначе десинк).
6. **Пакет 17 — 38 полей i32 без версии схемы:** добавление/перестановка поля в
   C++ молча ломает совместимость; констант `RESEARCH_TOPIC_COUNT=8`,
   `UNIT_END=93` нет в проводе — они «зашиты» в размеры массивов.
7. **`GAME_VERSION` конкретной сборки** не зафиксирован в репо (CMake-кэш);
   для max_net нужно значение из собранного сервера.
8. **Порт по умолчанию расходится:** код — 32000–32100, поставляемый
   `config.toml` — 31554–31600; дефолт ручного запуска — 31554.
9. **Тикет-легаси = пароль в открытом виде** в WS-ответе и `settings.json`
   сессии; подписанные тикеты включаются только при `MAX_SESSION_SECRET`.
10. **Bitcode-режим Horizon** (zstd/lz4, event table) для max_net не нужен
    (JSON достаточно), но если включать — потребуется точная версия крейта
    `bitcode` и словарь сжатия.
11. **Один клиент на команду?** Пакет 35 позволяет занять слот только если он
    свободен; поведение при повторном 35 от того же игрока — перевыбор слота
    (не проверено на двух клиентах одной команды).
