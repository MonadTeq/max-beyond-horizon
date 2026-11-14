# SIM reference vectors (P7)

Генератор эталонных векторов для финальной фазы `max_core` (P7):
CRC16 пакета 45, канонический дайджест состояния и мини-реплей.

`main.cpp` — самостоятельная программа на `g++`, которая исполняет **код оракула**:

| Что | Источник в `max-cpp-port/src` |
|---|---|
| `crc16()` | `crc16.c:24` (дословная копия) |
| `get_crc16_word()` | `game_manager.cpp:2315` (`GameManager_GetCrc16`) |
| генерация карты/ресурсов/обломков | `../map_vectors/main.cpp` (**включён** в тот же TU; `#define main` переименовывает его `main`), т.е. копии `resource_allocator.cpp`, `game_manager.cpp`, `world.cpp`, `access.cpp` |
| правила победы/поражения | `winloss.cpp`, `scripter.cpp`, `menu.cpp` — перенесены в `max_core::victory` |
| скрипты миссий | `assets/PATCHES.RES` → `SC_L0002`, `SC_T0001..15`, `SC_S0001..24`, `SC_C0001..09` |

## Две секции `vectors.txt`

### 1. CRC16 (`crc_bytes`, `crc_word`)

`crc_bytes <hex payload> <crc16 hex>` — `crc16()` от буфера (init `0xFFFF`,
полином `0x1021`, старший бит вперёд).

`crc_word <data> <state> <crc>` — `GameManager_GetCrc16`: 16 сдвигов с
полиномом, затем XOR с данными. Эквивалент двух побайтовых шагов
(старший байт, затем младший) — это свойство проверяет
`max_core::crc::tests::word_step_matches_two_byte_steps`.

⚠️ Это **не** CRC-16/CCITT-FALSE: реализация `crc16.c` вносит бит данных в
младший разряд **после** сдвига, поэтому «123456789» даёт `0xa69d`
(у CCITT-FALSE было бы `0x29b1`). Эталон — только этот файл.

### 2. Мини-реплей (`replay_*`)

Сценарий: карта строится теми же параметрами, что case `p00_l111_d0_s0`
из `../map_vectors/vectors.txt` (planet 0, уровни 1/1/1/0, seed 0, старты
`(0,20,20)` и `(2,70,70)`, alien costs `4 8 5 6`), затем:

1. `UnitsManager_DeployUnit`: красный `ENGINEER` в `(20,20)`, синий `TANK`
   в `(24,20)`;
2. 10 тиков (`UnitsManager_ProcessOrders` над двумя списками);
3. печать канонического дайджеста `max_core::crc::replay_digest_bytes`
   (магия `MAXRPL1\0`, CRC карт, команды, юниты в порядке пяти списков,
   CRC16 в конце).

Записи:

| Запись | Значение |
|---|---|
| `replay_case <name>` | case в `../map_vectors/vectors.txt` (источник tile_ids/pass_table) |
| `replay_levels <raw> <fuel> <gold> <derelicts>` | уровни меню |
| `replay_seed <u32>` | seed (совпадает с P2-case) |
| `replay_start <team> <x> <y>` | стартовые позиции |
| `replay_alien_costs <...>` | `ATTRIB_TURNS` alien-юнитов (вход, как в P2) |
| `replay_deploy <unit_type> <team> <x> <y>` | порядок `DeployUnit` |
| `replay_ticks <n>` | число тиков |
| `replay_checksum <hex>` | CRC пакета 45 на этом состоянии (`GameManager_CheckDesync`, `game_manager.cpp:2356`) |
| `replay_digest <hex>` | дайджест (little-endian поля) |

Константы двух юнитов — данные P3: `ENGINEER` (`SC_ATTRI`: turns 6, hits 14,
armor 4, speed 6, scan 3, storage 40; `SC_UNITS`: flags `0x04684180`) и `TANK`
(turns 4, hits 24, armor 10, attack 16, speed 6, range 4, rounds 2, scan 4,
ammo 14; flags `0x06624100`). Ассетов в репозитории нет, поэтому значения
вписаны константами — ровно как `alien_costs` в P2-генераторе.

### Что доказывает дайджест

Совпадение байт-в-байт между C++-генератором и
`max_core` (`tests/p7.rs::mini_replay_matches_cpp_digest_bit_for_bit`)
означает: одинаковую карту (CRC поверхности и ресурсов), одинаковое состояние
RNG после генерации, одинаковые id/счётчики/флаги/атрибуты юнитов, одинаковый
порядок пяти списков и одинаковый канонический формат дайджеста. Дополнительно
`replay_checksum` сверяет **сетевой** CRC пакета 45: `GameManager_GetUnitListChecksum`
по четырём спискам и пяти командам (порядок полей: команда, тип, `unit_id`,
клетка, `hits`, `speed` у не-стационарных, `shots`, `storage` при наличии типа
груза, `ammo`). В этом сценарии оба юнита бездействуют (AWAIT/INIT,
команды-люди), поэтому 10 тиков
не меняют ни одного поля дайджеста — тест фиксирует сам цикл тика и отсутствие
дрейфа. Более «живой» реплей (движение/бой) требует полного headless-порта
(`max-port --server`), см. «Открытые вопросы» в README крейта.

## Сборка

Ubuntu 24.04 (как в задании):

```bash
docker run --rm -v "D:/dev/max-port-debug:/src" \
  -w /src/max-rust/tools/sim_vectors ubuntu:24.04 \
  bash -c "apt-get update -qq && apt-get install -y -qq g++ >/dev/null && \
           g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt"
```

`vectors.txt` закоммичен; `max_core` читает его через
`include_str!("../../../tools/sim_vectors/vectors.txt")`.

## Перегенерация

`vectors.txt` нельзя править вручную: обновите `main.cpp` и перегенерируйте
командой выше, затем прогоните `cargo test -p max_core`.
