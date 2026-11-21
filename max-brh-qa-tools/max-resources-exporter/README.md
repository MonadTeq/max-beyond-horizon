# max-resources-exporter — ресурсы M.A.X. по каждому типу юнита

Отдельный Cargo-проект (свой `Cargo.lock`, не входит в workspace `max-brh`),
как [`max-brh-qa-max-brh-qa-tools/max-resources-viewer`](../max-resources-viewer/README.md). Обходит все
**93 типа юнитов** (`UnitType::ALL`) и раскладывает их ресурсы по каталогам
`max-brh-assets/units/<TAG>/`:

```text
units/TANK/
├── sprites/            frame_XXX.png (корпус), sheet.png (лента кадров), sprite.json
├── turret/             кадры башни (если у типа есть)
├── firing/             кадры выстрела (если есть)
├── connectors/         кадры коннекторов (если есть)
├── shadow/             кадры тени S_*
├── portrait.png        P_* 300×240 (палитра окна) либо fallback из спрайта
├── animation/          frame_000.png…, animation.gif, копия исходного .FLC
└── unit.json           тип/имя/флаги/источники/размеры/наличие анимации
```

Рядом с каталогами юнитов (в `max-brh-assets/units`) пишутся `manifest.csv`
(строка на тип) и `README.md` со статистикой; сами каталоги в git не входят
(`.gitignore` → `max-brh-assets/units/*/`), потому что регенерируются из данных
игры.

## Запуск

Из корня репозитория (нужны `max-original/MAX.RES`, `COLOR.PAL`, `*.FLC`,
`max-cpp-port/assets/PATCHES.RES` и `max-brh/crates/max_app/data/cargo_unit_data.json`):

```sh
cd max-brh-qa-tools/max-resources-exporter
cargo run --release
```

Переопределение путей и частичный экспорт:

```sh
cargo run --release -- \
  --res ../../max-original/MAX.RES \
  --pal ../../max-original/COLOR.PAL \
  --flc-dir ../../max-original \
  --units ../../max-brh/crates/max_app/data/cargo_unit_data.json \
  --patches ../../max-cpp-port/assets/PATCHES.RES \
  --out ../../max-brh-assets/units \
  --only TANK,MININGST
```

`--help` печатает справку, `--no-patches-check` отключает сверку с `SC_UNITS`.

## Откуда берётся маппинг

`max-brh/crates/max_app/data/cargo_unit_data.json` — снимок `SC_UNITS` из
`assets/PATCHES.RES`, который генерирует
[`max-brh/tools/gen_cargo_unit_data.py`](../../max-brh/tools/gen_cargo_unit_data.py):
для каждого из 93 типов `sprite` / `shadow` / `portrait` / `data` / `flics`
(`INVALID_ID` → `null`). Экспорт читает этот JSON, а по `--patches` ещё и
сверяет его с самим архивом (`SC_UNITS` XOR-зашифрован таблицей
`ResourceManager_GenericTable`, разбор — в `src/units.rs`); расхождение —
ошибка, а не «тихая» выгрузка не по тем тегам.

## Как рисуются пиксели

* спрайты — `COLOR.PAL` (6-битные компоненты `×4`), структурная прозрачность
  RLE → альфа 0;
* тень `S_*` — маска: чёрный, альфа 160;
* `P_*` — big image, индексы через **палитру окна** (`FRAMEPIC`), как
  `CargoMenu`/`BuildMenu` (`palette_from_image = false`);
* FLC — индексы кадра через палитру `COLOR_256` самого FLC (6-бит `×4`);
  `animation.gif` пишется своим GIF89a-кодировщиком (`src/gif.rs`: LZW,
  локальные палитры кадров при смене, зацикливание);
* `sheet.png` — горизонтальная лента кадров **корпуса**, выровненная по горячей
  точке, как в [`max-brh-assets/README.md`](../../max-brh-assets/README.md) §«Лента
  кадров» (у `INFANTRY`/`COMMANDO` она до ~25 000 px в ширину — это нормально
  для 200 кадров).

## Проверки

```sh
cargo test          # 24 юнит-теста + 7 интеграционных на реальных данных
```

Интеграционные тесты (`tests/export.rs`) фиксируют реальные числа: 93 каталога,
70 портретов `P_*`, 22 fallback (+`MASTER` без ресурсов), 70 типов с FLC,
975/118/224/188/1357 кадров корпуса/башни/выстрела/коннекторов/тени, а также
проверяют размеры PNG, соответствие `sprite.json` ↔ `D_*` (`FrameInfo`),
валидность `animation.gif` и идемпотентность повторного экспорта (те же байты,
без мусора). Без данных игры (`max-original/MAX.RES`) интеграционные тесты
пропускаются.

## Модули

| Файл | Что внутри |
|---|---|
| `src/main.rs` | CLI: пути, `--only`, `--help`, сводка в stdout |
| `src/units.rs` | `cargo_unit_data.json` + сверка `SC_UNITS` (XOR + JSON) |
| `src/export.rs` | обход 93 типов, кадры/портрет/FLC, `manifest.csv`, `README.md`, идемпотентная запись |
| `src/render.rs` | палитры, спрайты/тени/портреты, лента кадров по горячей точке |
| `src/gif.rs` | GIF89a: LZW + локальные палитры + зацикливание |
| `src/png.rs` | PNG-кодек (копия `max-brh/crates/max_app/src/qa/png.rs` + RGBA) |
| `src/names.rs` | английские имена типов (`buildmenu::unit_name`) |
