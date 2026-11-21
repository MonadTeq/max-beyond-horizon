# RNG reference vectors (P1)

Генератор эталонных векторов для `max_core::rng` (`max-brh/crates/max_core/src/rng.rs`).

`main.cpp` содержит **самостоятельную копию** алгоритма оракула:

- `SplitMix64` — из `XoshiroCpp.hpp` (`SplitMix64::operator()`);
- `Xoshiro128**` — из `XoshiroCpp.hpp`
  (`Xoshiro128StarStar(uint64_t seed)` и `operator()`);
- `Randomizer_Generate<T>` — из `max-cpp-port/src/randomizer.hpp`
  (`static_cast<T>((static_cast<int64_t>(next) * value) >> 32)`).

Сам `max-cpp-port` при этом **не изменяется** и не собирается: копия сделана
намеренно, чтобы генератор зависел только от `g++`.

## Сборка и запуск

Рекомендуемый способ — Ubuntu 24.04 (как в описании задачи):

```bash
docker run --rm -v "D:/dev/max-port-debug:/src" \
  -w /src/max-brh/tools/rng_vectors ubuntu:24.04 \
  bash -c "apt-get update -qq && apt-get install -y -qq g++ >/dev/null && \
           g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt"
```

Локально (любой Linux/macOS с `g++`):

```bash
g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt
```

`vectors.txt` закоммичен и используется Rust-тестом через
`include_str!("../../../tools/rng_vectors/vectors.txt")`.

## Формат `vectors.txt` (v1)

Строки, начинающиеся с `#`, — комментарии. Остальное — поток токенов,
разделённых пробелами/переводами строк:

| Запись | Значение |
|---|---|
| `state <seed> <s0> <s1> <s2> <s3>` | состояние `[u32;4]` сразу после `set_seed(seed)` |
| `next <seed> <count>` + `count` значений | последовательность `next_u32()` |
| `gen_u16 <seed> <range> <count>` + `count` значений | `Randomizer_Generate<uint16_t>(range)` |
| `gen_i32 <seed> <range> <count>` + `count` значений | `Randomizer_Generate<int32_t>(range)` |
| `gen_u64 <seed> <range> <count>` + `count` значений | `Randomizer_Generate<uint64_t>(range)` (ветка `size_t`) |
| `reseed <seed> <count=64>` + 64 значения | 32 значения после `set_seed(seed)` и ещё 32 после повторного `set_seed(seed)` (блоки обязаны совпасть) |

Длины: `count = 1000` для всех последовательностей; `range` = `101` (`u16`),
`1000` (`i32`), `8` (`u64`). Сиды:

```
0, 1, 2, 0xDEADBEEF, 1234567890, 0x0BADF00D, 0x12345678, 0xFFFFFFFF
```

Пять первых заданы DESIGN.md §7 (P1); три последних — детерминированные
«случайные» сиды для дополнительного покрытия.

## Что проверяет Rust-тест

`rng::tests::matches_cpp_reference_vectors` парсит `vectors.txt` и сравнивает
**бит-в-бит**:

- начальное состояние для каждого сида (то есть SplitMix64-сидирование);
- 8 × 1000 значений `next_u32()`;
- 8 × 1000 значений `generate<u16>(101)`, `generate<i32>(1000)`,
  `generate<u64>(8)`;
- 8 пар reseed-блоков (идемпотентность `set_seed`).

Итого: **32 000** значений четырёх последовательностей + 512 значений reseed +
32 слова состояния.

## Перегенерация

Если меняется формат или набор сидов, обновите одновременно `main.cpp` и
Rust-парсер в `rng.rs`, затем перегенерируйте `vectors.txt` командой выше и
убедитесь, что `cargo test` зелёный.
