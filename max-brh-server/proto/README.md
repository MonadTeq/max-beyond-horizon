# proto/ — Protobuf-схемы протокола Horizon (gRPC)

Схемы лобби/админ-протокола Horizon (PLAN v6.0): `.proto` — единый источник
истины контракта, из которого генерируются сервер (Rust) и клиенты (C++/любой
язык). Описание решений и маппинг на старый WebSocket JSON-протокол — в
[`max-brh-docs/protocol-grpc.md`](../../max-brh-docs/protocol-grpc.md).

## Файлы

| Файл | Пакет | Содержимое |
|---|---|---|
| `types.proto` | `horizon.v1` | Общие типы: `GameInfo`, `GameStatus`, `AdminSession`, `SessionStatusInfo`, `Ticket`, `TicketKind`, `Error` |
| `horizon.proto` | `horizon.v1` | Сервис `Horizon`: `Hello`, `ListGames`, `CreateGame`, `JoinGame`, `AdminListSessions`, `AdminSessionStatus`, `AdminSessionStop`, `AdminSessionRestart`, `AdminSessionStart`, `Ping` |

Зависимостей на `google/protobuf/*.proto` нет — генерация работает с голым
`protoc` без include-путей к well-known types.

## Проверка синтаксиса (protoc)

```sh
protoc --proto_path=proto --descriptor_set_out=/dev/null \
  proto/horizon.proto proto/types.proto
```

## Генерация для Rust (tonic + prost)

1. В `Cargo.toml` крейта-сервера:

   ```toml
   [dependencies]
   tonic = "0.12"
   prost = "0.13"
   tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

   [build-dependencies]
   tonic-build = "0.12"
   ```

2. `build.rs`:

   ```rust
   fn main() -> Result<(), Box<dyn std::error::Error>> {
       tonic_build::configure()
           .build_server(true)   // серверная часть (tonic::transport)
           .build_client(true)   // клиентская часть (для тестов/tooling)
           .compile_protos(
               &["proto/horizon.proto", "proto/types.proto"],
               &["proto"],
           )?;
       Ok(())
   }
   ```

   Сгенерированный код попадёт в `OUT_DIR`; подключение:
   `tonic::include_proto!("horizon.v1")` (пакет `horizon.v1` → модуль
   `horizon::v1`). `tonic-build` умеет скачивать/вызывать `protoc` сам, но
   рекомендуется ставить его явно: `choco install protoc` / `apt install
   protobuf-compiler` или `cargo install protobuf-bin-vendored` (через
   `PROTOC` env — свой protoc).

3. Опциональные поля (`optional string ...`) требуют protoc ≥ 3.15 (в
   Ubuntu 24.04 — 3.21.x) и tonic/prost ≥ 0.9/0.11.

## Генерация для C++ (grpc++)

```sh
# сообщения
protoc -I proto --cpp_out=gen \
  proto/horizon.proto proto/types.proto

# сервис (нужен плагин grpc_cpp_plugin из пакета grpc)
protoc -I proto --grpc_out=gen --plugin=protoc-gen-grpc="$(which grpc_cpp_plugin)" \
  proto/horizon.proto proto/types.proto
```

CMake-потребитель (vcpkg/системный gRPC):

```cmake
find_package(Protobuf CONFIG REQUIRED)
find_package(gRPC CONFIG REQUIRED)
protobuf_generate(TARGET my_client PROTO_DIRS proto)
target_link_libraries(my_client PRIVATE gRPC::grpc++ protobuf::libprotobuf)
```

Сгенерированные классы: `horizon::v1::Horizon::Stub` (клиент),
`horizon::v1::Horizon::Service` (сервер).

## Альтернатива: buf

```sh
buf lint proto   # стилевые проверки
buf generate     # генерация по buf.gen.yaml (плагины remote/protoc-gen-prost,
                 # protoc-gen-cpp, protoc-gen-grpc-cpp)
```

## Конвенции

- Пакет `horizon.v1` — версия контракта. Ломающие изменения ⇒ новый пакет
  `horizon.v2` + параллельный сервис, старый остаётся на время перехода.
- Имена полей повторяют JSON-ключи WS-протокола (см. таблицу соответствия в
  `max-brh-docs/protocol-grpc.md`); номера полей фиксируются навсегда.
- `u16` JSON → `uint32` (в protobuf нет 16-битных целых), `i64` → `int64`,
  UUID → `string` (нижний регистр, с дефисами).
- Ошибки: стандартные gRPC-статусы + `horizon.v1.Error` только там, где
  WS-протокол отдаёт `error` инлайн (`ok:false` в админ-ответах).
