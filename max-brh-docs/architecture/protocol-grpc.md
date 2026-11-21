# Протокол Horizon: лобби/админ поверх gRPC + protobuf

> Версия: v1 (проект схемы) · Статус: СПРОЕКТИРОВАНО, реализация — следующий этап.
> Источник истины: [`max-brh-server/proto/`](../max-brh-server/proto/) —
> `horizon.proto` (сервис) + `types.proto` (общие типы).
> Базовый контракт (WS JSON): [`max-brh-docs/protocol.md`](protocol.md).

## 1. Цель

Сегодня клиенты общаются с Horizon (max-brh-server) по **WebSocket + JSON**
(`max-brh-docs/protocol.md` §5). По PLAN v6.0 лобби и админка переводятся на
**gRPC + protobuf**:

- **бинаризация и типизация** — нет ручного парсинга JSON, нет расхождений
  «код пишет одно, клиент читает другое»;
- **`.proto` как единый источник истины** — сервер (Rust), клиенты (C++, любой
  язык), документация и тесты генерируются из одного контракта;
- **игровая сессия (UDP/ENet) остаётся как есть** — тикеты, порты и
  хендшейки `TP 00`/`TP 03` не меняются, `.proto` их только *описывает*
  (тип `Ticket`), не заменяет.

Этот документ фиксирует решения по схеме и её соответствие старому
WS-контракту.

## 2. Структура схемы

Пакет **`horizon.v1`**, два файла:

| Файл | Содержимое |
|---|---|
| [`types.proto`](../max-brh-server/proto/types.proto) | Общие типы: `GameStatus`, `GameInfo`, `AdminSession`, `SessionStatusInfo`, `Ticket`, `TicketKind`, `Error` |
| [`horizon.proto`](../max-brh-server/proto/horizon.proto) | Сервис `Horizon` + все `*Request`/`*Response` |

Разнесение выбрано (а не один файл), потому что `types.proto` переиспользуем
будущими сервисами Horizon (метрики, чат-мост, события GORC для лобби) без
копирования, а `horizon.proto` остаётся коротким и читаемым как оглавление
протокола. Пакет у обоих файлов один — код генерируется в один модуль
(`horizon::v1` в Rust, `horizon::v1::` в C++).

Зависимостей на `google/protobuf/*.proto` (well-known types) **нет**: схема
компилируется голым `protoc` без include-путей (см.
[`proto/README.md`](../max-brh-server/proto/README.md)).

## 3. Сервис и методы

Один сервис `Horizon`, десять методов — ровно по одному на каждую
WS-пару «запрос→ответ». Метод **заменяет** пару `{namespace, event}` и поле
`event` ответа: имя RPC — это и есть событие.

| # | RPC | WS-эквивалент | Запрос | Ответ |
|---|---|---|---|---|
| 1 | `Hello` | `{"type":"hello"}` | `protocol_version`, `client`, `dictionary_id?` | `protocol_version`, `format`, `compression`, `dictionary_id?` |
| 2 | `ListGames` | `max_lobby:list_games` | `{}` | `games: GameInfo[]` |
| 3 | `CreateGame` | `max_lobby:create_game` | `name? world? timer? endturn? play_mode? start_gold? victory_type? victory_limit? min_players? password?` | `game: GameInfo`, `ticket: Ticket` |
| 4 | `JoinGame` | `max_lobby:join_game` | `game_id`, `password?` | `game_id`, `host`, `port`, `ticket: Ticket` |
| 5 | `AdminListSessions` | `max_admin:list_sessions` | `admin_token?` | `sessions: AdminSession[]` |
| 6 | `AdminSessionStatus` | `max_admin:session_status` | `game_id`, `admin_token?` | `game_id`, `status: {state}` |
| 7 | `AdminSessionStop` | `max_admin:stop_session` | `game_id`, `admin_token?` | `game_id`, `ok`, `error?` |
| 8 | `AdminSessionRestart` | `max_admin:restart_session` | `game_id`, `admin_token?` | `game_id`, `ok`, `new_game_id?`, `port?`, `error?` |
| 9 | `AdminSessionStart` | `max_admin:start_session` | `admin_token?`, `port?`, те же опции игры, что в `CreateGameRequest` | `game_id?`, `port?`, `ok`, `error?` |
| 10 | `Ping` | `system:ping` | `payload?` (bytes) | `timestamp`, `message:"pong"` |

Все методы **unary** — обоснование в §4.1.

## 4. Ключевые решения

### 4.1. Unary vs streaming

**Решение: в v1 все методы unary.** Список игр отдаётся **снапшотом** в
`ListGames`, отдельного `ListGamesStream` нет.

Обоснование:

1. **Паритет с текущим контрактом.** WS-лобби сегодня — строго
   запрос/ответ: «Пушей нет: лобби — только запрос/ответ (свежесть —
   повторный `list_games`)» (`protocol.md` §5.1). Unary-снапшот воспроизводит
   это 1:1; streaming был бы *изменением семантики*, а не заменой транспорта.
2. **Масштаб данных.** Реестр лобби — десятки записей, каждая ~150 байт.
   Снапшот влезает в один фрейм, поллинг раз в 2–5 секунд копеечный.
   Server-streaming не экономит ничего существенного, но заставляет сервер
   держать открытый поток и состояние на **каждого** idle-клиента, а в
   Horizon `max_connections` считается тысячами — это заметная цена.
3. **Простота клиентов.** Unary — простейший и самый переносимый паттерн
   gRPC (grpc++, tonic, grpc-web, любой load-balancer). Для C++-клиента
   проекта это важный фактор.
4. **Обратимость.** Добавить `rpc ListGamesStream(...) returns (stream
   GameInfo)` в `horizon.v1` (или v1.1-сервис) можно **без каких-либо
   изменений** существующих методов — gRPC это явно разрешает. Плюс в
   Horizon уже есть GORC-подписки (`gorc:*`): если понадобятся живые
   обновления лобби, их правильнее строить на нём, а не дублировать в
   лобби-сервисе.

### 4.2. Модель ошибок: стандартные gRPC-статусы + инлайн-`Error`

**Решение — гибрид, повторяющий двуслойность WS-контракта:**

1. **Основной канал — стандартный gRPC status (code + message).** Все ошибки,
   которые WS отдавал фреймами `error` / `admin_error`, становятся
   gRPC-статусами: `status.message` несёт **точный текст** старого поля
   `error`, `status.code` — машиночитаемую категорию. Клиент получает обе
   вещи в каждом фрейме, без проверки «а есть ли поле error в ответе».
2. **`horizon.v1.Error` — только там, где WS отдавал ошибку *инлайн*** внутри
   нормального ответа: `session_stopped.ok:false.error`,
   `session_restarted…`, `session_started…`. В этих трёх ответах сохранены
   поля `ok`/`error` (JSON-ключи дословно), чтобы админ-тулинг, читающий
   `ok`, работал без изменений. `Error` = `{code: gRPC-код int32,
   legacy_close_code: 4000–4008 или 0, message: string}`.

Почему **не** свой enum вместо gRPC-статусов:

- статусы понимает вся экосистема: `grpcurl`, перехватчики, ретраи, health-
  check'и, grpc-web — кастомный enum в поле ответа они бы не увидели, и
  клиенту пришлось бы проверять ошибки в двух местах;
- транспортные ошибки (обрыв, deadline) в gRPC всё равно приходят статусом —
  один канал ошибок неизбежен, два канала — только запутывают.

Детальные таблицы маппинга — в §6.

### 4.3. Версионирование: `horizon.v1`

- Версия контракта зашита в **пакет** (`horizon.v1`), как принято в
  protobuf-мире. Поле `protocol_version` остаётся только в `Hello` (паритет
  с WS-хендшейком): сервер сверяет его как раньше и отдаёт
  `FAILED_PRECONDITION` при несовпадении.
- Правила совместимости **внутри** v1 (зафиксированы комментарием в
  `types.proto`):
  - номера полей и имена сервисов/методов не меняются и не переиспользуются;
  - новые поля — только добавлением (новый номер, `optional`/значение по
    умолчанию); неизвестные поля proto3 сохраняет и игнорирует, поэтому
    старый клиент переживёт новый сервер и наоборот;
  - новые значения enum — только добавлением; новые методы — только
    добавлением;
  - **ломающее** изменение ⇒ новый пакет `horizon.v2` и параллельная
    регистрация сервиса на время перехода (тот же приём, что с WS, §4.4).

### 4.4. Обратная совместимость: параллельный запуск WS и gRPC

**Решение: WS (порт 8081, как сегодня) и gRPC (новый порт, по умолчанию
8082) работают одновременно** весь период перехода; WS выключается флагом
конфигурации после миграции клиентов (а не в день релиза).

- gRPC-сервис — тонкий адаптер **поверх тех же** `max_lobby` / `max_admin` /
  `max_session` / `max_auth`: бизнес-логика, реестр игр, менеджер сессий и
  секреты общие, различается только сериализация. Сейчас каждый плагин
  держит свой `SessionManager`; при реализации gRPC-адаптера стоит вынести
  состояние плагинов в общий хендл, чтобы WS и gRPC видели один и тот же
  реестр игр (иначе `ListGames` и WS `list_games` разойдутся). Это
  рефакторинг реализации, не схемы.
- Конфигурация (предложение): секция `[server.grpc]` в `config.toml`:
  `enabled = true`, `bind_address = "0.0.0.0:8082"`, TLS-ключи опционально
  (как `[server.websocket]`). Старый `[server]`/`[max]` не трогается.
- WS-клиенты (max_net текущей версии, `horizon-smoke.py`) не замечают
  изменений до своего собственного перехода.

### 4.5. Имена полей и типы: паритет с JSON-ключами

- **Имена полей .proto = JSON-ключи** везде, где ключ существует:
  `game_id`, `name`, `world`, `status`, `port`, `players`, `min_players`,
  `password_protected`, `state`, `dir`, `ok`, `error`, `new_game_id`,
  `timestamp`, `message`, `protocol_version`, `client`, `dictionary_id`,
  `host`, `ticket`, `admin_token`, `password`. Исключения осознанные:
  - поле `event` ответов — не переносится, его роль играет имя RPC;
  - оболочка `{namespace, event, data}` запроса — не переносится, `data`
    становится телом сообщения запроса;
  - `ticket` — строка → сообщение `Ticket` (`.value` = строка), см. §4.6.
- **Несовпадение типов JSON/ЯП — задокументировано в схеме:**
  - `u16` (порты, версия) → `uint32`: в protobuf нет 16-битных целых,
    сервер валидирует ≤ 65535 (комментарии в .proto);
  - `i64` (`world`, `timer`, …) → `int64` как в Rust-структурах;
  - `players` (usize) → `uint32`;
  - UUID → `string` (нижний регистр, с дефисами). `bytes` (16 байт) было бы
    компактнее, но поломало бы паритет и читаемость; при желании это
    отдельная оптимизация v2;
  - состояния → `enum GameStatus` со значениями `STARTING/RUNNING/EXITED/
    STOPPED/UNKNOWN` (+ обязательный в proto3 нулевой
    `GAME_STATUS_UNSPECIFIED`, который сервер не шлёт). «Кривой» нулевой
    элемент — плата за proto3; для C++/Rust это не проблема, а
    JSON-гейтвею достаточно маппинга строк.
- **Осознанно сохранённая асимметрия WS:** лобби зовёт статус `status`
  (`GameInfo.status`), админка — `state` (`AdminSession.state`). Поля в
  .proto повторяют ключи соответствующих WS-фреймов, чтобы маппинг был
  механическим и «grep-абельным» в обоих кодовых базах.
- **`hello_ack`**: `format` всегда `"protobuf"`, `compression` — сжатие
  сообщений, договорённое gRPC (`"none"`, `"gzip"`, `"zstd"`, …);
  `dictionary_id` всегда unset. Поля оставлены ради паритета ключей; их
  реальная роль на gRPC-транспорте — индикатор того, что клиент разговаривает
  по gRPC.

### 4.6. Тикет и `host`

- **`Ticket`** (в `types.proto`): `value` — точная строка для ENet-хендшейка
  `TP 00`; `kind` (`LEGACY`/`SIGNED`); для подписанного тикета дополнительно
  разобранные `session_id`, `player_id`, `expires_at_unix`. Оба формата из
  `protocol.md` §6 сохранены дословно: legacy (`""` для открытых игр, пароль
  для защищённых) и `v1.<session_id>.<player_id>.<unix_expiry>.<hex_hmac>`
  (TTL 24 ч, constant-time проверка в `max_auth`). Схема ничего не меняет в
  криптографии — только типизирует то, что уже есть на проводе.
- **`host` — отдельное поле `JoinGameResponse.host`, значение поправимо без
  изменения схемы.** Сейчас в коде захардкожен `127.0.0.1` (`protocol.md`
  §12); поле зафиксировано в контракте именно для того, чтобы позже
  заполнять его из конфигурации (публикуемый адрес сервера) без ломки
  клиентов.

## 5. Таблица соответствия JSON ↔ proto

Легенда: `opt` = proto3-`optional` (присутствие отслеживается, как
`Option<T>` в Rust). «—» = поля в контракте нет.

### 5.1. hello / hello_ack

| WS JSON | Proto (`HelloRequest`) |
|---|---|
| `protocol_version` (u16) | `uint32 protocol_version = 1` |
| `client` (string?) | `string client = 2` |
| `dictionary_id` (u32?) | `optional uint32 dictionary_id = 3` |

| WS JSON | Proto (`HelloAck`) |
|---|---|
| `type: "hello_ack"` | — (имя RPC `Hello`) |
| `protocol_version` | `uint32 protocol_version = 1` |
| `format: "json"` | `string format = 2` = всегда `"protobuf"` |
| `compression` | `string compression = 3` = сжатие gRPC-канала |
| `dictionary_id?` | `optional uint32 dictionary_id = 4` (всегда unset) |

### 5.2. game_list / game_created.game → `GameInfo`

| WS JSON | Proto (`GameInfo`) |
|---|---|
| `id` (UUID string) | `string id = 1` |
| `name` | `string name = 2` |
| `world` (i64) | `int64 world = 3` |
| `status` ("starting"\|…) | `GameStatus status = 4` |
| `port` (u16) | `uint32 port = 5` (≤ 65535) |
| `players` | `uint32 players = 6` |
| `min_players` (u32) | `uint32 min_players = 7` |
| `password_protected` | `bool password_protected = 8` |

### 5.3. create_game → `CreateGameRequest` / `CreateGameResponse`

| WS `data` | Proto |
|---|---|
| `name?` | `opt string name = 1` |
| `world?` (i64) | `opt int64 world = 2` |
| `timer?` (i64) | `opt int64 timer = 3` |
| `endturn?` (i64) | `opt int64 endturn = 4` |
| `play_mode?` (i64) | `opt int64 play_mode = 5` |
| `start_gold?` (i64) | `opt int64 start_gold = 6` |
| `victory_type?` (i64) | `opt int64 victory_type = 7` |
| `victory_limit?` (i64) | `opt int64 victory_limit = 8` |
| `min_players?` (u32) | `opt uint32 min_players = 9` |
| `password?` | `opt string password = 10` |

| WS ответ | Proto (`CreateGameResponse`) |
|---|---|
| `event: "game_created"` | — (имя RPC) |
| `game: {…}` | `GameInfo game = 1` |
| `ticket: "<…>"` | `Ticket ticket = 2` (строка в `ticket.value`) |

### 5.4. join_game → `JoinGameRequest` / `JoinGameResponse`

| WS `data` | Proto |
|---|---|
| `game_id` | `string game_id = 1` |
| `password?` | `opt string password = 2` |

| WS ответ | Proto (`JoinGameResponse`) |
|---|---|
| `event: "game_joined"` | — (имя RPC) |
| `game_id` | `string game_id = 1` |
| `host` (сейчас "127.0.0.1") | `string host = 2` |
| `port` (u16) | `uint32 port = 3` |
| `ticket: "<…>"` | `Ticket ticket = 4` |

### 5.5. sessions_listed / session_status / stop / restart / start

| WS | Proto |
|---|---|
| `data: {}` / `admin_token?` | `AdminListSessionsRequest { opt string admin_token = 1; }` |
| ответ `sessions: [{…}]` | `AdminListSessionsResponse { repeated AdminSession sessions = 1; }` |
| запись `{game_id, name, port, state, dir}` | `AdminSession { string game_id=1; string name=2; uint32 port=3; GameStatus state=4; string dir=5; }` |

| WS | Proto |
|---|---|
| `data: {game_id, admin_token?}` | `AdminSessionStatusRequest/AdminSessionStopRequest/AdminSessionRestartRequest { string game_id=1; opt string admin_token=2; }` |
| `{game_id, status:{state}}` | `AdminSessionStatusResponse { string game_id=1; SessionStatusInfo status=2; }` (`state` внутри) |
| `{game_id, ok, error?}` | `AdminSessionStopResponse { string game_id=1; bool ok=2; opt Error error=3; }` |
| `{game_id, ok, new_game_id?, port?, error?}` | `AdminSessionRestartResponse { string game_id=1; bool ok=2; opt string new_game_id=3; opt uint32 port=4; opt Error error=5; }` |
| `data: {admin_token?, port?, name?, world?, …}` | `AdminSessionStartRequest { opt string admin_token=1; opt uint32 port=2; opt string name=3; opt int64 world=4; opt int64 timer=5; opt int64 endturn=6; opt int64 play_mode=7; opt int64 start_gold=8; opt int64 victory_type=9; opt int64 victory_limit=10; opt uint32 min_players=11; }` |
| успех `{game_id, port}` / фейл `{ok:false, error}` | `AdminSessionStartResponse { opt string game_id=1; opt uint32 port=2; bool ok=3; opt Error error=4; }` — успех ставит `ok=true` явно (в WS успешном фрейме `ok` нет) |

### 5.6. ping

| WS | Proto |
|---|---|
| `data`: любой JSON | `PingRequest { opt bytes payload = 1; }` (сырые байты, не интерпретируются) |
| `{timestamp (u64 ms), message:"pong"}` | `PingResponse { uint64 timestamp = 1; string message = 2; }` |

## 6. Модель ошибок: таблицы маппинга

### 6.1. Тексты `error` / `admin_error` → gRPC-код

`status.message` = старый текст дословно (инструменты, грепающие логи/ответы
по тексту, продолжат работать).

| Старый текст (WS) | gRPC-код | Комментарий |
|---|---|---|
| `max_lobby is not configured` | `UNAVAILABLE` (14) | сервис не поднят (нет `MAX_SERVER_EXE`/`MAX_TEMPLATE_DIR`) |
| `game not found` (лобби: не UUID / нет в реестре) | `NOT_FOUND` (5) | |
| `invalid password` | `PERMISSION_DENIED` (7) | пароль защищённой игры не сошёлся |
| `game is not running (status: …)` / `game is not running` | `FAILED_PRECONDITION` (9) | join только в `Running` |
| `a network game needs at least 2 players` | `INVALID_ARGUMENT` (3) | |
| ошибки старта процесса (кроме порта), напр. сбой спавна | `INTERNAL` (13) / `UNAVAILABLE` (14) | по месту возникновения |
| `no free UDP port left in the configured range` | `RESOURCE_EXHAUSTED` (8) | |
| `max_admin is not configured` | `UNAVAILABLE` (14) | |
| `admin authorization is disabled: no admin_secret is configured` | `UNAUTHENTICATED` (16) | fail-closed |
| `missing admin_token` | `UNAUTHENTICATED` (16) | |
| `invalid admin_token` | `UNAUTHENTICATED` (16) | неверные креденшалы |
| `invalid max_admin:<event> payload: …` | `INVALID_ARGUMENT` (3) | |
| `invalid game_id …: expected a UUID` (админ) | `INVALID_ARGUMENT` (3) | |
| `requested port … is outside the configured MAX_PORT_RANGE …` | `INVALID_ARGUMENT` (3) | (в WS это `admin_error`) |
| `port … is already used by session …` | `ALREADY_EXISTS` (6) / `INVALID_ARGUMENT` (3) | предложение: `ALREADY_EXISTS`; если важно 1:1 со старым фреймом — см. ниже |

**Инлайн-ответы сохраняются** только для `ok:false` в
`AdminSessionStopResponse` / `AdminSessionRestartResponse` /
`AdminSessionStartResponse`: там заполняется `Error { code, legacy_close_code=0,
message }` — это в точности старая пара `{ok:false, error:"…"}` плюс код.

### 6.2. WS close-коды 4000–4008 → gRPC

Это коды уровня **соединения**; у gRPC их аналог — транспорт HTTP/2 (GOAWAY,
trailers) и статусы. Таблица нужна гейтвеям/адаптерам, чтобы старый
клиентский код, понимающий 4000–4008, получил то же поведение:

| WS close | Причина (старая) | gRPC-эквивалент |
|---|---|---|
| 4000 | protocol version not supported | `FAILED_PRECONDITION` (9) — в ответе `Hello` |
| 4001 | frames do not match the listener's settings | `INTERNAL` (13) — неверная конфигурация сервера |
| 4002 | compression dictionary mismatch | `FAILED_PRECONDITION` (9) — в ответе `Hello` |
| 4003 | too many invalid frames | `ABORTED` (10) — транспорт разрывает соединение |
| 4004 | message too large | `RESOURCE_EXHAUSTED` (8) — gRPC сам ограничивает размер сообщений |
| 4005 | kicked by server | `PERMISSION_DENIED` (7) |
| 4006 | idle timeout | `DEADLINE_EXCEEDED` (4) — keepalive/таймауты gRPC |
| 4007 | server full | `RESOURCE_EXHAUSTED` (8) / `UNAVAILABLE` (14) |
| 4008 | client not reading fast enough | `RESOURCE_EXHAUSTED` (8) — flow-control HTTP/2 |

Прямого аналога «закрыть соединение с кодом» в gRPC нет; на уровне
приложения эти условия транслируются в статусы выше, а сам разрыв (если
нужен) делает транспорт.

## 7. Примеры: JSON ↔ proto

### 7.1. hello

```json
{"type":"hello","protocol_version":1,"client":"max_net/0.1","dictionary_id":null}
```

```proto
HelloRequest { protocol_version: 1  client: "max_net/0.1" }
HelloAck     { protocol_version: 1  format: "protobuf"  compression: "none" }
```

### 7.2. list_games

```json
{"event":"game_list","games":[{"id":"5e2f9f3a-8b4d-4c1e-9a7b-2d0e6f8a1c3b",
  "name":"Friday 2v2","world":3,"status":"running","port":31554,
  "players":2,"min_players":4,"password_protected":false}]}
```

```proto
ListGamesResponse {
  games: [ GameInfo { id: "5e2f9f3a-8b4d-4c1e-9a7b-2d0e6f8a1c3b"
                      name: "Friday 2v2"  world: 3  status: RUNNING
                      port: 31554  players: 2  min_players: 4
                      password_protected: false } ]
}
```

### 7.3. create_game (защищённая игра)

```json
// запрос: {"namespace":"max_lobby","event":"create_game",
//          "data":{"name":"2v2","world":3,"password":"sesame"}}
{"event":"game_created","game":{/* GameInfo, см. 7.2 */},
 "ticket":"sesame"}
```

```proto
CreateGameRequest  { name: "2v2"  world: 3  password: "sesame" }
CreateGameResponse { game: GameInfo{...}
                     ticket: Ticket { value: "sesame"  kind: LEGACY } }
```

### 7.4. join_game (подписанный тикет, настроен `MAX_SESSION_SECRET`)

```json
{"event":"game_joined",
 "game_id":"5e2f9f3a-8b4d-4c1e-9a7b-2d0e6f8a1c3b",
 "host":"127.0.0.1","port":31554,
 "ticket":"v1.5e2f9f3a-8b4d-4c1e-9a7b-2d0e6f8a1c3b.7d0f1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b.1772341200.0a1b…ff"}
```

```proto
JoinGameResponse {
  game_id: "5e2f9f3a-8b4d-4c1e-9a7b-2d0e6f8a1c3b"
  host: "127.0.0.1"  port: 31554
  ticket: Ticket {
    value: "v1.5e2f9f3a-….0a1b…ff"
    session_id: "5e2f9f3a-8b4d-4c1e-9a7b-2d0e6f8a1c3b"
    player_id: "7d0f1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b"
    expires_at_unix: 1772341200
    kind: SIGNED
  }
}
```

### 7.5. Ошибки

Лобби-ошибка (WS: `{"event":"error","error":"invalid password"}`):

```proto
// не "ответ", а gRPC-статус:
//   code    = PERMISSION_DENIED (7)
//   message = "invalid password"
```

Админ-операция упала (WS: `{"event":"session_started","ok":false,
"error":"no free UDP port left in the configured range"}`):

```proto
AdminSessionStartResponse {
  ok: false
  error: Error { code: 8 /* RESOURCE_EXHAUSTED */
                 legacy_close_code: 0
                 message: "no free UDP port left in the configured range" }
}
```

Админ не авторизован (WS: `{"event":"admin_error","error":"invalid
admin_token"}`):

```proto
// gRPC-статус: code = UNAUTHENTICATED (16), message = "invalid admin_token"
```

## 8. План перехода (обратная совместимость)

| Фаза | Содержание | Что ломается |
|---|---|---|
| 0 (сделано этим документом) | `.proto` + README + этот документ | ничего |
| 1 | Сервер: `tonic`-сервис `Horizon` в крейте `horizon` — адаптер над `max_lobby`/`max_admin`/`max_session`/`max_auth`; общий хендл состояния плагинов (WS и gRPC видят один реестр); `[server.grpc]` в `config.toml`, порт 8082; grpc-рефлексия и health-сервис (`grpc.health.v1.Health`) для tooling'а | ничего: WS работает как раньше |
| 2 | Пилот: `horizon-smoke.py`/`grpcurl` гоняют оба транспорта, сверка ответов 1:1 | ничего |
| 3 | Клиенты (max_net и др.) переходят на gRPC; WS остаётся включённым | ничего до flag-day |
| 4 | Flag-day: WS-листенер выключается флагом; при необходимости — удаление | старые WS-клиенты |

Замечания:

- тикеты и UDP-часть не трогаются ни на одной фазе — gRPC отдаёт те же
  `host`/`port`/`ticket.value`, что и WS;
- grpc-web (браузерные админки) поддержит tonic-web/Envoy-прокси без
  изменения `.proto`;
- `Hello` опционален и в gRPC — сервер обязан отвечать на любой метод без
  предварительного `Hello` (как WS принимает первое событие без hello).

## 9. Открытые вопросы

1. **`host`**: публикуемый адрес сервера (конфиг) вместо `127.0.0.1` —
   решается в фазе 1, схема готова (поле уже есть).
2. **`AdminSessionStartResponse.ok`**: в WS успешный фрейм `ok` не содержит;
   в proto он всегда `true` на успехе. Если строгий 1:1 нужен инструментам —
   обсудить, но менять схему не нужно (это правило кодеков).
3. **Метаданные vs поле `admin_token`**: схема оставляет токен полем (паритет);
   переход на gRPC-metadata — аддитивное улучшение позже.
4. **`ListGamesStream`**: не в v1 (см. §4.1); при появлении живых обновлений
   лобби — сначала оценить GORC-подписки, иначе добавить метод в v1 без
   ломки.
5. **grpc-рефлексия/health**: не часть контракта, но рекомендуется в фазе 1
   для отладки (`grpcurl list horizon`).
