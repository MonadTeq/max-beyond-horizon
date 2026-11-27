# remaster-encode — build-time KTX2-энкодер (R2, HD-ремастер)

Инструмент гейта **R2** трека `max-brh-docs/development/remaster-hd.md`:
lossless-исходники → `.ktx2` (ETC1S + UASTC) + `remaster-manifest.json` в
`target/remaster/` (вне git). Отдельный Cargo-проект со своим `Cargo.lock`,
**не входит** в workspace `max-brh` и не меняет прод-код игры: читает
`max-original/` через `max_assets`/`max_render` (те же декодеры, что у клиента)
и пишет только в `target/`.

## Команды

```sh
cd max-brh-qa-tools/remaster-encode

# 1. Lossless-исходники из реальных данных: тайл GREEN_1.WRL (64×64 и ×4 256×256),
#    FRAMEPIC 640×480, кадр TANK → target/remaster-sources/*.png
cargo run --release -- sample

# 2. Кодирование каталога исходников → target/remaster/*.ktx2 + remaster-manifest.json
cargo run --release -- encode

# 3. Тот же прогон с самопроверкой детерминизма (каждый файл кодируется дважды)
cargo run --release -- encode --verify

# 4. Гейт «два прогона байт-в-байт»: два каталога и diff
cargo run --release -- encode --out ../../target/remaster-run1
cargo run --release -- encode --out ../../target/remaster-run2
```

`--root` — корень репозитория (каталог с `max-original/MAX.RES`); без него
ищется вверх от текущего каталога. `--sources`/`--out` переопределяют каталоги,
`--primary etc1s|uastc` — кодек, на который указывает `assets[].path`.

## Формат исходников

Плоский каталог (без подкаталогов), файлы обрабатываются в порядке имён —
манифест не зависит от порядка перечисления каталога:

| файл | смысл |
|---|---|
| `<tag>.png` | lossless PNG, `scale = 1` |
| `<tag>@4x.png` | то же, исходник сделан под презентационный масштаб 4 |
| `<tag>_<W>x<H>.rgba` | сырые RGBA8-байты (`W*H*4`), размеры из имени |
| `<tag>@2x_<W>x<H>.rgba` | то же + масштаб |

Тег — имя файла без `@<N>x` и `_<W>x<H>`; теги уникальны (иначе — ошибка:
один тег в манифесте соответствует одному ассету).

## Что делает `encode`

1. Для каждого исходника кодирует **оба** кодека энкодером `ktx-rs` (чистый Rust,
   порт Khronos KTX-Software / libktx, версия закреплена `=4.4.4`):
   ETC1S (BasisLZ) и UASTC LDR 4×4 — те же параметры, что измерялись в R0:
   `quality_level = 128`, `uastc_flags = LEVEL_DEFAULT | FAVOR_UASTC_ERROR`.
2. Пишет `<tag>.etc1s.ktx2` и `<tag>.uastc.ktx2` в `--out`.
3. Пишет `remaster-manifest.json`: схема загрузчика R1
   (`max_assets::remaster`) плюс build-поля R2.

Старые `*.ktx2` в каталоге вывода удаляются перед прогоном, поэтому каталог
всегда соответствует каталогу исходников.

## Детерминизм

Гейт R2 — «два прогона на одной машине → байт-в-байт». Обеспечено так:

- `ktx-rs = "=4.4.4"`, `Cargo.lock` закоммичен (пин всех транзитивных версий);
- все параметры энкодера — константы `src/encode.rs` (`QUALITY_LEVEL`,
  `UASTC_FLAGS`) и **`THREAD_COUNT = 1`**: вывод не зависит от планировщика;
- исходники обходятся в порядке имён, манифест сериализуется из `BTreeMap`
  (ключи отсортированы), JSON пишется с завершающим переводом строки;
- в манифест пишется SHA-256 каждого `.ktx2` и каждого исходника, поэтому
  сравнение двух прогонов не требует доверия к самому инструменту.

Проверка: `encode --verify` (двойное кодирование в процессе) и diff двух
каталогов `--out` (внешняя проверка гейта).

## Манифест

```json
{
  "version": 1,
  "encoder": {
    "tool": "remaster-encode",
    "tool_version": "0.1.0",
    "library": "ktx-rs",
    "library_version": "4.4.4",
    "thread_count": 1,
    "quality_level": 128,
    "uastc_flags": 10,
    "primary_codec": "etc1s"
  },
  "assets": {
    "terrain_tile": {
      "path": "terrain_tile.etc1s.ktx2",
      "scale": 1,
      "hash": "sha256:<hex>",
      "source": {
        "path": "terrain_tile.png",
        "width": 64,
        "height": 64,
        "sha256": "<hex>"
      },
      "variants": {
        "etc1s": {"path": "terrain_tile.etc1s.ktx2", "bytes": 1152, "sha256": "<hex>"},
        "uastc": {"path": "terrain_tile.uastc.ktx2", "bytes": 4288, "sha256": "<hex>"}
      }
    }
  }
}
```

- `assets[tag].path`/`hash`/`scale` — ровно то, что читает R1-загрузчик
  (`path` — основной кодек, `hash` в его конвенции `sha256:<hex>`);
- `source.sha256` — хеш **файла** исходника: изменённый исходник меняет запись;
- `variants` — оба кодека с размером и хешем (R2);
- незнакомые R1-загрузчику поля он игнорирует (`serde` без `deny_unknown_fields`),
  совместимость закреплена тестами (`cargo test`).

## Тесты

`cargo test` — unit-тесты модулей (`src/*.rs`) и интеграционные в `tests/`:
схема манифеста парсится настоящим `max_assets::remaster::RemasterManifest`,
энкодер даёт одинаковые байты при повторном вызове, .ktx2 → RGBA8 через
штатный рантайм-транскодер даёт те же пиксели.
