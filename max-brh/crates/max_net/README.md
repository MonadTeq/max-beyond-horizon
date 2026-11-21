# max_net

Сетевая часть Rust-клиента M.A.X.: **Horizon по gRPC** и **игровая сессия по
UDP/ENet 1.3.18**. Контракты — `max-brh-docs/protocol-grpc.md`, `max-brh-docs/protocol.md`,
схемы — `max-brh-server/proto/`.

## Модули

| Модуль | Назначение |
|---|---|
| `horizon` | Типизированный async-клиент Horizon: `connect`, `hello`, `list_games`, `create_game`, `join_game` (→ `GameJoined` с `ticket`), `ping`. Таймауты на connect и на каждый вызов. Ошибки — стандартные gRPC-статусы (`HorizonError::Status { code, message }`). |
| `wire` | Байтовые (де)сериализаторы игрового протокола (`max-brh-docs/protocol.md` §8): конверт `0x50 + n`, команды 00/01/06/08/10/11/12/13/14/18/20/38/41/43/45/50/52/53, лобби 05/26/28/29/30/31/32/33/34/35/36/37/44/47, состояние 09/16/17, аудит/синк 23/24/49/51/54, управление 07/39/40/42/46/48. Пакеты ≤ 1440 байт. |
| `handshake` | `TP 00` / `TP 03` (канал 0), `data_hash = SHA256(MAX.RES)‖SHA256(PATCHES.RES)` (128 hex), проверки версий/тикета/данных. |
| `session` | ENet-подключение, `TP 00`-обмен, асинхронный цикл «команды → события», барьеры 00/45/52 (повтор 3 с, таймаут 30 с). |
| `enet` | Единственный `unsafe`-модуль: минимальная обёртка над вендоренным C-ENet. |
| `lobby`, `protocol` | Легаси WS-JSON клиент. Оставлен, чтобы `max_app` собирался до завершения миграции; новый код должен использовать `horizon`. |

### Переиспользование `max_core`

Пакеты-команды **не дублируются**: `wire::command` пишет/читает
`max_core::input::{Packet08Order, Packet38Move, …}` и возвращает
`max_core::input::Command`, который сразу применим через
`max_core::events::apply_command`. Собственные wire-DTO есть только у того,
чего нет в `max_core`: NetNode/лобби, `UnitState` (Packet23Data), `StateSync`
(54), `GameOptions` (17).

## Сборка

### Proto (gRPC)

`build.rs` вызывает `tonic-prost-build` для
`../../../max-brh-server/proto` относительно `CARGO_MANIFEST_DIR`
(из `max-brh/crates/max_net` это `<repo>/max-brh-server/proto`). Путь
зафиксирован и проверяется ассертом; `protoc` берётся из крейта
`protoc-bin-vendored`, поэтому `apt-get install protobuf-compiler` не нужен.

Сгенерированный код доступен как `max_net::horizon::proto` (`horizon.v1`).
При изменении `.proto` достаточно `cargo check` — `cargo:rerun-if-changed`
перегенерирует стабы.

### ENet 1.3.18 (vendor + cc)

Исходники `enet-1.3.18` скопированы из `dependencies/enet-1.3.18.tar.gz` в
[`vendor/enet/`](vendor/enet); `build.rs` собирает их `cc::Build` в статическую
библиотеку и линкует (`winmm`/`ws2_32` на Windows). Это ровно та же версия,
которую собирает C++-порт (`CMakeLists.txt` → `include(enet)`,
`ENET_VERSION = 0x010312`), поэтому `TP 00` сверяет одинаковое значение.
Системный ENet и `apt`-пакеты не нужны; для сборки достаточно C-компилятора
(`gcc`/`cl`).

Почему `unsafe` допустим и локализован — см. docs модуля `enet`: один
`ENetHost*` на владельца, `Send` без `Sync`, все вызовы через `&mut self`,
пакеты копируются и уничтожаются по правилам владения ENet.

### TLS: выключен по умолчанию (фича `tls`)

Horizon — plaintext `http://host:port` (`horizon.rs` собирает `Endpoint` из
`format!("http://{host}:{port}")`), поэтому TLS-стек крейту не нужен.
`tonic` подключён с `default-features = false` и явным набором
`router,transport,codegen`. У tonic 0.14 дефолт и так без TLS, но набор
зафиксирован явно: rustls/ring/aws-lc (C/asm-зависимости, которые ломают
лёгкую кросс-сборку) не появятся в собираемом графе при смене дефолтов
`tonic`.

Если Horizon когда-нибудь переедет на `https://`:

```sh
cargo build -p max_app --features tls      # max_net/tls → tonic/tls-ring + tls-webpki-roots
```

### Windows-сборка (`x86_64-pc-windows-msvc`, нативно; `windows-gnu` — тоже работает)

`build.rs` выбирает платформенный файл ENet по **`CARGO_CFG_TARGET_OS`**, а не
по `cfg!(target_os = "windows")`: в build-скрипте этот `cfg!` раскрывается для
**хоста**, поэтому при кросс-сборке в Windows компилировался `unix.c` (он весь
под `#ifndef _WIN32` → пустой объект) и линковка падала на
`undefined reference to enet_socket_create`. Тот же env-переменной управляются
`winmm`/`ws2_32`. На нативном MSVC `cfg!` и `CARGO_CFG_TARGET_OS` совпадают,
правило нужно для любого кросс-таргета.

Сборка: `powershell -ExecutionPolicy Bypass -File
max-brh-cicd\scripts\build-windows-headless.ps1` (нативный MSVC, headless:
`--no-default-features`, без `sdl*`/`sound`), см. `max-brh-cicd/scripts/README.md`.

## Проверка

```sh
# из корня репозитория; gcc/g++/cmake нужны для cc/vendor enet
docker run --rm -v "$PWD:/src" -w /src/max-brh rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test"
```

Тесты не требуют сети:

* gRPC: mock-сервер `horizon.v1` на `127.0.0.1:0` (реальные tonic-вызовы
  `Hello`/`ListGames`/`CreateGame`/`JoinGame`/`Ping`, маппинг gRPC-статусов,
  таймаут);
* wire: точные байтовые литералы для `TP 00`, `TP 03`, 08 P0–P5, 38 (земля и
  воздух), 41 (полная и короткая формы), 50, 06, 52, 45, 12, 18 и round-trip
  остальных пакетов;
* `data_hash`: известные SHA-256 векторы (`"abc"`/пустой файл), чтение
  `MAX.RES`/`PATCHES.RES` из каталога;
* ENet: версия линкованной библиотеки, создание client/server host, порядок
  байтов адреса;
* барьеры: повтор/таймаут на синтетическом `Instant`.

## Живой smoke (Docker)

1. Собрать и запустить Horizon + headless по
   `max-brh-server/README.Docker.md` / `Dockerfile`, прописав в
   `config.toml` секцию `[max]` (бинарь headless, `template_dir`, диапазон
   UDP-портов) и gRPC-листенер (`max-brh-docs/protocol-grpc.md` §4.4, порт 8082).
2. Убедиться, что в лобби есть игра (`ListGames`), либо создать её
   `CreateGame`.
3. `JoinGame` → `GameJoined { host, port, ticket }`.
4. Собрать `DataHash::from_data_dir(<каталог с MAX.RES/PATCHES.RES>)`; важно,
   чтобы файлы побайтово совпадали с серверными (иначе `TP 03` reason 3).
5. `SessionConfig::from_joined(&joined, data_hash)` (при необходимости
   `with_game_version(...)` — `GAME_VERSION` конкретной сборки), затем
   `SessionHandle::start(config)`.
6. Дождаться `SessionEvent::HandshakeOk`, отправить `LobbyPacket::EnterLobby`,
   пройти 44 → 29 → 30 → 36 → 35 → 32 → 05 → 34, затем слать приказы
   (`Command::Order`/`Move`/…) и барьеры.
7. Маркеры в логах headless (`sessions/<uuid>/ai_log.txt`): вход клиента,
   `Main game loop`, отсутствие `Desync check failed` / `desync-*.txt`.

Ограничения, зафиксированные в контракте: игрового реконнекта/SNAP нет
(`max-brh-docs/protocol.md` §9), ретрансляция «сервер → остальные клиенты» в текущем
снапшоте не подтверждена (§12.1), `host` в `JoinGameResponse` пока
`127.0.0.1` (§12.2) — для удалённого сервера подставляйте адрес из конфига.
