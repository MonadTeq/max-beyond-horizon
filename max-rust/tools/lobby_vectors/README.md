# Lobby wire reference vectors

Генератор эталонных байтов лобби-пакетов для `max-rust/crates/max_net/src/wire/lobby.rs`.

## Зачем

`Remote_WriteNetNode` (`max-cpp-port/src/remote.cpp:772`) сериализует `NetNode`
не целиком, а по полям — но адрес пишет как **агрегат**:

```cpp
packet << node.entity_id;
packet << node.address;          // <-- sizeof(NetAddress), не 6!
packet.Write(node.name, sizeof(node.name));
packet << node.is_host;
```

`NetAddress` объявлен в `max-cpp-port/src/net_address.hpp` как
`{ uint32_t host; uint16_t port; }`, поэтому на ABI игры
`sizeof(NetAddress) == 8`: после `port` идут **два байта struct-паддинга**.
`operator<<` копирует `sizeof(T)` байт, то есть паддинг тоже уходит в пакет.
Его значение на отправителе не определено (в C++ это неинициализированная
память), но его **длина** — часть wire-формата.

Rust-парсер читал только 6 байт адреса. На пакете 30 (один NetNode перед
блоком GameSettings) это сдвигало чтение на 2 байта, и `mission_category`
читался как `name[29] is_host 04 00` = `0x000400FC` — то самое
`invalid MissionCategory value 262396` из живого smoke.

## Сборка и запуск

```bash
docker run --rm -v "D:/dev/max-port-debug:/src" \
  -w /src/max-rust/tools/lobby_vectors rust:1.94.0-bookworm \
  bash -c "g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt"
```

`main.cpp` подключает **настоящий** `net_address.hpp` из `max-cpp-port/src`
(только он, без SDL-зависимостей) и содержит самостоятельную копию
`Remote_WriteNetNode` / `Remote_WriteGameSettings` /
`Remote_SendNetPacket_30/35/37/44/47`. Сам порт не собирается и не меняется.

`static_assert(sizeof(NetAddress) == 8)` в генераторе падает при сборке, если
ABI или заголовок изменятся.

## Формат `vectors.txt` (v1)

Строки `#` — комментарии, остальное:

```
packet <номер> <hex-байты кадра целиком, включая type>
```

Пакеты: 28, 29, 30, 35, 37, 44, 47. Вектор 30 содержит два NetNode (хост и
клиент) — так проверяется, что паддинг пропускается на каждом узле, а не
только на первом. В векторе 47 паддинг адреса намеренно заполнен `0xCD`:
парсер, который просто предполагает нули, на нём упадёт.

`vectors.txt` закоммичен; Rust-тесты `max_net::wire::lobby::tests` содержат те
же байты hex-литералами и дополнительно сверяют их с `vectors.txt`
(`literals_match_cpp_generated_vectors`).

## Перегенерация

Меняете набор/значения векторов — обновите `main.cpp`, перегенерируйте
`vectors.txt` командой выше и синхронизируйте hex-литералы в
`max_net/src/wire/lobby.rs`. `cargo test -p max_net` обязан быть зелёным.
