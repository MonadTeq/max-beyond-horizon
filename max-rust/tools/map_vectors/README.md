# MAP reference vectors (P2)

Генератор эталонных векторов для `max_core::map` / `max_core::setup`
(`max-rust/crates/max_core/src/map.rs`, `.../src/setup.rs`).

`main.cpp` — **самостоятельная копия** тех путей оракула (`max-cpp-port/src`),
которые реально исполняются при создании партии. Сам `max-cpp-port` не
изменяется и не собирается: копия сделана намеренно, чтобы генератор зависел
только от `g++` (ни SDL, ни ассетов).

## Что скопировано из оракула

| Код в `main.cpp` | Источник в `max-cpp-port/src` |
|---|---|
| `splitmix64_next`, `Randomizer_GetNext`, `Randomizer_Generate<T>` | `randomizer.cpp` / `randomizer.hpp` + `XoshiroCpp.hpp` |
| `menu_update_resource_levels()` | `menu.cpp:866` (уровни `raw/fuel/gold/derelicts` 0–2 → числовые настройки) |
| `ResourceRange::GetValue`, `ResourceAllocator` (`PopulateCargoMap`, `Optimize`, `OptimizeResources`, `GetZoneResoureLevel`, `ConcentrateResources`, `SeparateResources`, `SettleMinimumResourceLevels`) | `resource_allocator.cpp` (дословно) |
| `Access_GetApproximateDistance`, `Access_IsInsideBounds`, `Access_GetSurfaceType`, `Access_GetModifiedSurfaceType` | `access.cpp:386`, `:2409`, `:936`, `:950` |
| `GameManager_IsValidStartingPosition`, `GameManager_FindValidStartingPosition` | `game_manager.cpp:4196`, `:4712` |
| `GameManager_PopulateMapWithResources` | `game_manager.cpp:4730` |
| `GameManager_FindSpot`, `GameManager_SpawnAlienDerelicts`, `GameManager_PopulateMapWithAlienUnits` | `game_manager.cpp:4763`, `:4805`, `:4942` |
| `World_BuildSurfaceMap` (`PassData[tile_ids]`) | `world.cpp:382-409` |

`UnitsManager_DeployUnit()` заменён на `Tool_DeployUnit()`, который только
записывает `(unit_type, x, y)` и помечает клетки мостов: по
`units_manager.cpp:2354` и `unitinfo.cpp:1301` развёртывание не пишет ни в
`CargoMap`, ни в `SurfaceMap`, поэтому такое замещение эквивалентно.

## Два входа, которых нет в репозитории

Зафиксировано честно, чтобы вектор не выглядел «настоящей игровой картой»:

1. **Ассет `.wrl` планеты.** Процедурной генерации поверхности в порте нет:
   `ResourceManager_InitInGameAssets(settings["world"])` читает файл планеты, а
   `World::LoadTilesAndMetadata()` переводит `tile_ids` + `pass_table` в байты
   `SurfaceMap`. Файлов `.wrl` в репозитории нет, поэтому `build_world_input()`
   синтезирует фиксированный layout для каждого `planet` (блок суши 12..99 и
   вода/побережье/воздух по краям). **Преобразование** `pass_table → SurfaceMap`
   при этом — настоящий код оракула, и именно оно проверяется Rust-тестом.
2. **`ATTRIB_TURNS` alien-юнитов.** Таблицы атрибутов 93×14 — это P3, поэтому у
   каждого case своя строка `alien_costs` (`ALNTANK`, `JUGGRNT`, `ALNASGUN`,
   `ALNPLANE`). Для команды пришельцев `current_values == base_values`, так что
   одной таблицы достаточно для обоих чтений в `GameManager_SpawnAlienDerelicts`.

## Сборка и запуск

Ubuntu 24.04 с `g++` (как в описании задачи):

```bash
docker run --rm -v "D:/dev/max-port-debug:/src" \
  -w /src/max-rust/tools/map_vectors ubuntu:24.04 \
  bash -c "apt-get update -qq && apt-get install -y -qq g++ >/dev/null && \
           g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt"
```

Локально (любой Linux/macOS с `g++`):

```bash
g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt
```

`vectors.txt` закоммичен и читается Rust-тестом через
`include_str!("../../../tools/map_vectors/vectors.txt")`.

## Формат `vectors.txt` (v1)

Построчный; строки, начинающиеся с `#`, — комментарии. Блок начинается с `case`
и содержит:

| Запись | Значение |
|---|---|
| `case <name>` | имя вектора |
| `planet <u8>` | id планеты (`settings["world"]`), выбирает вход `.wrl` |
| `levels <raw> <fuel> <gold> <derelicts>` | уровни меню 0–2 (`menu_update_resource_levels`) |
| `seed <u32>` | seed (`Remote_RngSeed`) |
| `start <team> <x> <y>` | стартовая позиция команды (вход; `FindValidStartingPosition` может её сдвинуть) |
| `pass_table <hex>` | сырая pass-таблица `.wrl` (индексы в `PassData`) |
| `tile_ids <hex u16 LE>` | tile-id карта `.wrl` |
| `surface <hex, 12544 байта>` | ожидаемый `SurfaceMap` (порт `world.cpp:382-409`) |
| `alien_costs <alntank> <juggrnt> <alnasgun> <alnplane>` | `ATTRIB_TURNS` (вход) |
| `resources <hex u16 LE, 12544>` | ожидаемый `ResourceManager_CargoMap` после ресурсной фазы |
| `placement <unit_type> <x> <y>` | порядок `UnitsManager_DeployUnit` в alien-фазе |
| `rng_state <s0> <s1> <s2> <s3>` | состояние RNG после всей генерации (decimal) |

Шесть векторов покрывают `planet × levels × seed`:

| case | planet | raw/fuel/gold/derelicts | seed |
|---|--:|---|---|
| `p00_l111_d0_s0` | 0 | 1/1/1/0 | `0` |
| `p03_l000_d1_s1` | 3 | 0/0/0/1 | `1` |
| `p07_l222_d1_sdeadbeef` | 7 | 2/2/2/1 | `0xDEADBEEF` |
| `p12_l201_d2_s1234567890` | 12 | 2/0/1/2 | `1234567890` |
| `p17_l022_d2_s42` | 17 | 0/2/2/2 | `42` |
| `p23_l120_d0_s7fffffff` | 23 | 1/2/0/0 | `0x7FFFFFFF` |

## Что проверяет Rust-тест

`setup::tests::matches_cpp_reference_vectors` для каждого case сверяет
**бит-в-бит**:

1. `derive_surface_map(tile_ids, pass_table)` — `SurfaceMap` (12 544 байта);
2. `populate_map_with_resources` — `ResourceMap` (12 544 × u16 LE);
3. `populate_map_with_alien_units` — полный список `AlienPlacement`
   (186 записей суммарно по всем case, включая плиты/здания/мосты/юнитов);
4. `rng.state()` после генерации — ловит любое расхождение в числе или порядке
   вызовов RNG (а не только в результате).

Дополнительно: `generation_is_deterministic` (двойной прогон),
`generated_resources_stay_within_bitfield` (границы битфилда, видимость не
выставляется), `menu_levels_match_cpp_settings`, `derive_surface_map_follows_pass_data_order`,
`find_valid_starting_position_walks_spiral`, `starting_position_requires_pure_land`,
`alien_population_disabled_consumes_no_rng`, `resource_bits_are_written_like_cpp`.

## Перегенерация

Если меняется набор case, формат или копируемый код оракула — обновите
`main.cpp`, перегенерируйте `vectors.txt` командой выше и убедитесь, что
`cargo test` зелёный. `vectors.txt` нельзя править вручную: он должен быть
результатом сборки этого файла.
