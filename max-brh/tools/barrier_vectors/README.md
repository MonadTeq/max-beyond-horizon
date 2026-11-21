# Barrier wire reference vectors

Генератор эталонных байтов барьерных пакетов для
`max-brh/crates/max_net/src/wire` (53/54/45).

## Зачем

Канон — C++ `max-cpp-port/src/remote.cpp`:

* **53** — `Remote_SendNetPacket_53` (`remote.cpp:4243`): `type, team u16,
  turn i32, count u16, count × {complex_id u16, 6×i16}`; шесть значений —
  `Complex::WritePacket` (`complex.cpp:65`) в порядке
  `material, fuel, gold, power, workers, buildings`.
* **54** — `Remote_SendUnitStates` (`remote.cpp:4333`):
  `type, team u16, sync_index u8, is_last bool, count u16,
  count × {unit_id u16, Packet23Data}` и хвост последнего чанка
  `gold u32, gold_spent_on_upgrades u32, research[8] (i32×3),
  number_of_objects_created u16, unit_counters[93] (u32)`.
  `Packet23Data` пишется по полям (`Remote_ReadPacket23Data`,
  `remote.cpp:3298`), поэтому struct-паддинг C++ в пакет **не** попадает:
  wire-размер — 45 байт, а `sizeof(Packet23Data)` на ABI игры — 50.
* **45** — `Remote_SendNetPacket_45` (`remote.cpp:3556`):
  `type, team u16, next_turn_index u8, crc_checksum u16`.

`main.cpp` содержит самостоятельные копии этих сериализаторов; сам порт не
собирается и не меняется.

## Сборка и запуск

```bash
docker run --rm -v "D:/dev/max-port-debug:/src" \
  -w /src/max-brh/tools/barrier_vectors rust:1.94.0-bookworm \
  bash -c "g++ -O2 -std=c++17 -Wall -Wextra -Werror main.cpp -o gen && ./gen > vectors.txt"
```

## Формат `vectors.txt` (v1)

Строки `#` — комментарии, остальное:

```
packet <номер> <hex-байты кадра целиком, включая type>
```

Пакеты: 53, 54, 45. Вектор 54 содержит два юнита (разные значения, включая
отрицательные координаты/груз) и полный хвост: 8 тем исследований и 93
счётчика `unit_counters[i] = i*3+1` — так проверяется и раскладка хвоста, и
его длина.

`vectors.txt` закоммичен; тесты `max_net::wire::tests` содержат те же байты
hex-литералами и сверяют их с `vectors.txt`
(`barrier_literals_match_cpp_generated_vectors`).

## Перегенерация

Меняете набор/значения векторов — обновите `main.cpp`, перегенерируйте
`vectors.txt` командой выше и синхронизируйте hex-литералы в
`max_net/src/wire/mod.rs`. `cargo test -p max_net` обязан быть зелёным.
