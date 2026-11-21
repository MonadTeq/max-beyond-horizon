# FLC reference vectors (C++ эталон декодера)

Генератор эталонных пикселей и хэшей кадров для `max-brh/crates/max_assets/src/flic.rs`.

## Зачем

Канон — C++-порт `max-cpp-port/src/flicsmgr.cpp` (загрузка кадров
`Flicsmgr_load_frame`, диспетчер `flicsmgr_decode_frame`, декодеры
`flicsmgr_decode_{color,byte_run,delta_flc}`) и `game_manager.cpp`
(`GameManager_MenuCreateFlic` создаёт 128×128 буфер `GameManager_Flic.sw` и
передаёт его в `flicsmgr_construct`, `GameManager_AdvanceFlic` листает кадры с
частотой 15 FPS).

`main.cpp` — самостоятельная копия этих функций: без SDL, без ресурсного
менеджера, буфер `width × height` с `full = width` (как окно `sw`). Поэтому
эталон можно regenerировать одной командой, не собирая порт.

## Сборка и запуск

```bash
docker run --rm -v "D:/dev/max-port-debug:/src" \
  -w /src/max-brh/tools/flic_vectors rust:1.94.0-bookworm \
  bash -c "g++ -O2 -std=c++17 -Wall -Wextra -Werror main.cpp -o gen && \
           ./gen /src/max-original > vectors.txt"
```

Без аргументов после каталога декодируются все `*.FLC` в нём; можно передать
конкретные имена (`./gen ../../../max-original AGT.FLC`).

## Формат `vectors.txt` (v1)

```
# строки '#' — комментарии
vector <file> <frame> <width> <height> <hex-пиксели, width*height*2 символов>
digests <file> <frames> <fnv1a-32 кадра 0> <fnv1a-32 кадра 1> …
files <число файлов> <всего кадров>
```

* `vector` — 6 полных кадров: `AGT.FLC` кадры 0/1/29 (BRUN, первая и последняя
  дельта), `AA_ISO.FLC` 0 (статичный ресурс, 35 таких), `COMMTWR.FLC` 0,
  `SAVELOAD.FLC` 0 (128×256 — единственный FLC другого размера);
* `digests` — по строке на каждый из 71 FLC-ресурса MAX.RES: FNV-1a-32
  (`h = 2166136261; h ^= b; h *= 16777619`, переполнение по модулю 2³²) от
  пикселей каждого кадра. Это покрывает **все 1115 кадров**, а не только первый.

## Проверка в Rust

`max-brh/crates/max_assets/tests/flic_vectors.rs`:

* `vectors_match_cpp_flicsmgr_for_every_frame` — пиксели выбранных кадров
  бит-в-бит и FNV-1a каждого кадра каждого файла (через `FlicPlayer`, т.е. тот же
  инкрементальный путь, что у C++), плюс сверка `decode_frame` с нуля;
* `vectors_cover_every_flc_resource_referenced_by_max_res` — набор файлов в
  эталоне совпадает с 71 `flc-ref` записью `MAX.RES`.

```bash
cd max-brh
cargo test -p max_assets --test flic_vectors
```

## Перегенерация

Изменили `main.cpp` (новые векторы/кадры) — соберите заново командой выше и
закоммитьте `vectors.txt` вместе с кодом. `cargo test -p max_assets` должен
остаться зелёным; расхождение означает, что Rust-декодер перестал совпадать с
C++.
