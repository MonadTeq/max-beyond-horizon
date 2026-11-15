# Развёртывание Horizon + headless M.A.X. (M5.3)

Готовый `docker-compose.yml` в корне репозитория поднимает **Horizon** (WS + gRPC)
и даёт ему спавнить **C++-headless `max-port`** на каждую игру. Rust-клиент
(`max-rust`) подключается к gRPC `:8082`, получает тикет и играет по UDP.

## Что внутри

| Файл | Назначение |
|---|---|
| `docker-compose.yml` | сервис `horizon`, опциональный `builder` (profile `build`) |
| `deploy/Dockerfile.runtime` | `ubuntu:24.04` + рантайм-зависимости, healthcheck |
| `deploy/entrypoint.sh` | рендер конфига из шаблона и запуск `horizon` |
| `deploy/horizon.toml.template` | обобщённый `smoke_config.toml`: `announce_host`, пути, порты |
| `deploy/healthcheck.sh` | проверка LISTEN `8080`/`8082` через `/proc/net/tcp` |

**Выбранный вариант развёртывания — прагматичный:** Horizon не собирается внутри
compose-образа. Монтируется готовый Linux-бинарь
`max-horizon-server/target/release/horizon`; образ содержит только glibc/OpenSSL
Ubuntu 24.04. Плюсы: `up` занимает секунды, не нужен Rust-образ на рантайме,
бинарь можно обновлять без пересборки. Минус: бинарь нужно собрать заранее
(ниже есть и профиль-сборщик).

## Предпосылки

- Docker с Linux-контейнерами (Docker Desktop / Linux).
- Собранный Linux-бинарь Horizon:
  ```sh
  # из корня репозитория; сборка в отдельном контейнере (rust:1.94.0-bookworm)
  docker run --rm -v "$PWD/max-horizon-server:/src" -w /src \
    rust:1.94.0-bookworm bash -c \
    "apt-get update -qq && apt-get install -y -qq pkg-config libssl-dev >/dev/null \
     && cargo build --release -p horizon"
  ```
  или через compose:
  ```sh
  docker compose --profile build run --rm builder
  ```
- `build/linux-server-template/` — headless `max-port` + `.so` + `PATCHES.RES`
  (собирается отдельно, см. `docs/architecture/dedicated-server-plan.md` §9).
- `max-original/` с `MAX.RES` (и `COLOR.PAL`, `.WRL`). **PATCHES.RES в этом
  каталоге быть не должно** — headless падает при загрузке ресурсов, если
  `PATCHES.RES` лежит в `game_data` (M5.2). `PATCHES.RES` берётся из
  `template_dir`.

## Быстрый старт

```sh
docker compose config -q          # проверка YAML/подстановок
docker compose up -d --build      # сборка runtime-образа + запуск
docker compose ps                 # STATUS: Up ... (healthy)
docker compose logs -f horizon
```

Порты:

| Порт | Протокол | Что |
|---|---|---|
| `8080` | TCP | WS-листенер Horizon (JSON-события) |
| `8082` | TCP | gRPC `horizon.v1.Horizon` — его использует Rust-клиент |
| `32000-32100` | UDP | сессии headless `max-port` (`max_session` port pool) |

Проверка gRPC (grpcurl не нужен) — пример `horizon_probe` из `max_app`:

```sh
docker run --rm --network container:max-horizon \
  -v "$PWD:/src" -w /src/max-rust rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null \
   && cargo run -q -p max_app --no-default-features --example horizon_probe -- 127.0.0.1 8082 smoke"
```

Ожидаемые строки:

```
[2] hello: protocol=1 format=protobuf compression=none
[4] create_game: id=<uuid> ...
[5] join_game: host=127.0.0.1 port=3200x ticket=v1... player=Some(...)
[6] ping: pong (<ms>)
```

`JoinGame` спавнит headless и первое время отвечает `game is not running
(status: starting)`, поэтому probe повторяет join до 30 с. Логи сессий остаются
на хосте: `build/docker-sessions/<uuid>/` (`stdout.txt`, `ai_log.txt`).

Клиент (headless-прогон 3 ходов):

```sh
cargo run --manifest-path max-rust/Cargo.toml -p max_app -- \
  --connect 127.0.0.1:8082 --join <game_id> --player rust --team 0 --turns 3 \
  --data-dir max-original --assets-dir assets --handshake-patches /tmp/nonexistent.res
```

Остановка:

```sh
docker compose down
```

## Конфиг и переменные

`deploy/entrypoint.sh` рендерит `deploy/horizon.toml.template` в
`/tmp/horizon/config.toml`. Менять конфиг — через `environment:` в
`docker-compose.yml` (или собственный `HORIZON_CONFIG`-файл volume-маунтом).

| Переменная | По умолчанию | Смысл |
|---|---|---|
| `HORIZON_ANNOUNCE_HOST` | `127.0.0.1` | host, который `JoinGame` отдаёт клиентам (`[server.grpc].announce_host`) |
| `HORIZON_WS_BIND` | `0.0.0.0:8080` | bind WS |
| `HORIZON_GRPC_BIND` | `0.0.0.0:8082` | bind gRPC |
| `MAX_EXECUTABLE` | `/opt/max/template/max-port` | headless-бинарь |
| `MAX_TEMPLATE_DIR` | `/opt/max/template` | шаблон сессии (`build/linux-server-template`) |
| `MAX_SESSION_ROOT` | `/var/lib/horizon/sessions` | рабочие каталоги сессий |
| `MAX_GAME_DATA` | `/opt/max/game-data` | каталог `MAX.RES` (без `PATCHES.RES`) |
| `MAX_PORT_START`/`MAX_PORT_END` | `32000`/`32100` | UDP-диапазон сессий |
| `MAX_RESTARTS` | `0` | перезапуски watchdog (0 — выкл.) |
| `MAX_SESSION_SECRET` | `smoke-secret` | HMAC-секрет тикетов сессий |
| `MAX_ADMIN_SECRET` | `smoke-admin` | HMAC-секрет админ-токенов |

Переменные `MAX_*` имеют приоритет над секцией `[max]` в TOML (так устроен
`max_session`), а `HORIZON_*` подставляет entrypoint до старта.

### Публичный сервер

```sh
HORIZON_ANNOUNCE_HOST=203.0.113.10 docker compose up -d
```

`announce_host` должен быть адресом, по которому клиенты реально достучатся до
UDP-портов `32000-32100` (NAT/firewall). Для TLS перед gRPC — reverse proxy
(фаза 1 отдаёт plaintext h2c).

## CI и smoke (M6.3)

`ci.sh` в корне — локальный эквивалент GitHub Actions:

```sh
bash ci.sh                  # max-rust: cargo check+test; Horizon: cargo test --workspace
bash ci.sh --smoke          # + build/scripts/live-smoke.sh (2 клиента × 5 ходов)
bash ci.sh --smoke --turns 10
```

Шаги выполняются в `rust:1.94.0-bookworm` с `gcc/g++/cmake` (как в проверках
M5/M6); для Horizon дополнительно `pkg-config libssl-dev`. Workflow —
`.github/workflows/ci.yml` (push/PR). Живой smoke в GitHub-hosted CI не
запускается: нужны лицензионные `MAX.RES` и собранный C++-headless — гоняйте
`bash ci.sh --smoke` на машине с данными.

`build/scripts/live-smoke.sh` также обновлён: корень репозитория и путь для
Docker вычисляются от расположения скрипта (не захардкожен `D:/...`), контейнеры
убираются по `trap`.

## Ограничения и безопасность

- Секреты `smoke-secret`/`smoke-admin` — **только для разработки**; для
  публичного сервера задайте свои и не храните в git.
- Контейнер работает от root (bind-каталоги на Linux проще). Для продакшена:
  non-root UID + права на `build/docker-sessions`, TLS/reverse proxy, firewall.
- `[plugins] auto_load = false`: gRPC-путь самодостаточен (`horizon_grpc` +
  `max_session`), WS-плагины `max_lobby`/`max_admin` в этом compose не грузятся.
- Реконнект: M6.1-заготовка в клиенте (`--reconnect-attempts`,
  `--reconnect-timeout`) восстанавливает только транспорт ENet+`TP 00`; полное
  восстановление партии (SNAP/54) — M6.1-финал, см. `docs/architecture/protocol.md` §9.
- `max-original` и `build/linux-server-template` содержат данные игры и не
  предназначены для распространения.

## Troubleshooting

| Симптом | Причина/решение |
|---|---|
| `entrypoint: нет бинаря /opt/horizon/horizon` | не собран `max-horizon-server/target/release/horizon` |
| `game is not running (status: starting)` | headless ещё стартует; probe ретраит, клиенту — повторить join |
| `TP 03 reason 3` у клиента | не совпал `data_hash` (`MAX.RES`/`PATCHES.RES`), проверьте `--assets-dir`/`--handshake-patches` |
| headless падает на старте | `PATCHES.RES` попал в `MAX_GAME_DATA` (M5.2) |
| `docker compose ps` — `unhealthy` | смотрите `docker logs max-horizon`; healthcheck ищет LISTEN 8080/8082 |
