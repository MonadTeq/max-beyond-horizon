# max-rust-3d-models — ресурсы M.A.X. по юнитам

Выгрузка «один каталог на тип юнита»: спрайты корпуса, башня, выстрел,
коннекторы, тень, портрет `P_*` и FLC-анимация — всё, что игра берёт для
конкретного типа через `SC_UNITS` (`sprite` / `shadow` / `portrait` / `data` /
`flics_animation`).

```text
max-rust-3d-models/
├── README.md            этот файл (в git)
├── manifest.csv         строка на тип: теги, кадры, портрет, FLC (в git)
└── assets-base/         САМА ВЫГРУЗКА — в git не коммитится (.gitignore)
    ├── TANK/
    │   ├── sprites/       frame_XXX.png (корпус), sheet.png (лента), sprite.json
    │   ├── turret/        кадры башни (если есть)
    │   ├── firing/        кадры выстрела (если есть)
    │   ├── connectors/    кадры коннекторов (если есть)
    │   ├── shadow/        кадры тени S_*
    │   ├── portrait.png   P_* 300×240 (палитра окна) или fallback
    │   ├── animation/     frame_000.png…, animation.gif, копия .FLC
    │   └── unit.json      тип/имя/флаги/источники/размеры/анимация
    ├── MININGST/
    └── …                  всего 93 каталога (все UnitType)
```

## Как перегенерировать

Из корня репозитория:

```sh
cd max-brh-qa-tools/max-resources-exporter
cargo run --release
```

Пути по умолчанию — относительно корня репозитория
(`../../max-original/MAX.RES`, `../../max-original/COLOR.PAL`,
`../../max-original/*.FLC`,
`../../max-brh/crates/max_app/data/cargo_unit_data.json`,
`../../assets/PATCHES.RES` для сверки `SC_UNITS`, вывод —
`../../max-rust-3d-models`). Любой путь переопределяется флагом:

```sh
cargo run --release -- \
  --res ../../max-original/MAX.RES \
  --pal ../../max-original/COLOR.PAL \
  --flc-dir ../../max-original \
  --units ../../max-brh/crates/max_app/data/cargo_unit_data.json \
  --patches ../../assets/PATCHES.RES \
  --out ../../max-rust-3d-models \
  --only TANK,MININGST
```

Без `--patches` сверка с `SC_UNITS` пропускается (маппинг всё равно
читается из `cargo_unit_data.json`). Повторный прогон **идемпотентен**: файлы
перезаписываются теми же байтами, а всё лишнее в каталогах юнитов (и каталоги
типов, которых нет среди 93) удаляется — мусора не остаётся.

## Статистика последнего экспорта

| Показатель | Значение |
|---|---:|
| Типов юнитов | **93** |
| — зданий / мобильных | 22 / 71 |
| Каталогов в `assets-base/` | 93 |
| PNG всего | **4131** |
| Объём каталогов | 35.2 МБ |
| Кадров корпуса | 975 |
| Кадров башни | 118 |
| Кадров выстрела | 224 |
| Кадров коннекторов | 188 |
| Кадров тени | 1357 |
| Портретов `P_*` (300×240) | **70** |
| Портретов-fallback (кадр спрайта) | 22 |
| Типов без `portrait.png` вовсе | 1 |
| Типов с FLC-анимацией | **70** (70 разных `.FLC`) |
| Типов без анимации | 23 |
| Сверка с `SC_UNITS` | ✔ 93 записей, 465 полей |

### У кого нет портрета `P_*`

Служебные и «общие» типы (обломки, покрытия, взрывы, ракеты, `WALDO`): в
`SC_UNITS` у них `portrait = INVALID_ID`. Для них `portrait.png` — fallback из
первого кадра спрайта, увеличенного целым множителем на чёрном фоне
(`unit.json` → `portrait.source = "fallback"`, `portrait.scale`).

23 тип(ов): `LRGRUBLE`, `SMLRUBLE`, `LRGTAPE`, `SMLTAPE`, `LRGSLAB`, `SMLSLAB`, `LRGCONES`, `SMLCONES`, `LNDEXPLD`, `AIREXPLD`, `SEAEXPLD`, `BLDEXPLD`, `HITEXPLD`, `MASTER`, `ROCKET`, `TORPEDO`, `ALNMISSL`, `ALNTBALL`, `ALNABALL`, `RKTSMOKE`, `TRPBUBLE`, `HARVSTER`, `WALDO`

Совсем без `portrait.png` (нет ни `P_*`, ни спрайта для fallback —
вырезанный из игры тип): `MASTER` — в каталоге только `unit.json`.

### У кого нет FLC-анимации

У большинства из них `flics_animation = INVALID_ID` (эффекты, ракеты,
обломки, `HARVSTER`, `WALDO`); у `MASTER` тег `F_MASTER` есть, но самого
ресурса (и файла) в данных игры нет. Каталога `animation/` у таких типов нет
(в `unit.json` → `animation: null`).

23 тип(ов): `LRGRUBLE`, `SMLRUBLE`, `LRGTAPE`, `SMLTAPE`, `LRGSLAB`, `SMLSLAB`, `LRGCONES`, `SMLCONES`, `LNDEXPLD`, `AIREXPLD`, `SEAEXPLD`, `BLDEXPLD`, `HITEXPLD`, `MASTER`, `ROCKET`, `TORPEDO`, `ALNMISSL`, `ALNTBALL`, `ALNABALL`, `RKTSMOKE`, `TRPBUBLE`, `HARVSTER`, `WALDO`

## Как это устроено

* **Спрайты** — `max_assets::sprite`, мультикадровый RLE из `MAX.RES`.
  Кадры корпуса берутся по `image_base..+image_count` из `D_*`
  (`max_assets::sprite::FrameInfo`), башня/выстрел/коннекторы — по
  `turret_image_base`/`firing_image_base`/`connector_image_base` из того же
  `D_*`. Пиксели — индексы `COLOR.PAL` (6-бит → ×4), пропущенные RLE-пиксели
  прозрачны.
* **Лента** `sprites/sheet.png` — горизонтальная укладка кадров корпуса по
  горячей точке, как в `max-brh-assets/README.md` §«Лента кадров» (у
  `INFANTRY`/`COMMANDO` она до ~25 000 px в ширину: 200 кадров, ячейка
  расширяется по горячей точке).
* **Тень** `S_*` — маска: чёрный, альфа 160.
* **Портрет** `P_*` — big image 300×240; индексы переводятся **палитрой окна**
  (`FRAMEPIC`), как `CargoMenu`/`BuildMenu` порта
  (`WindowManager_LoadBigImage(..., palette_from_image = false)`). Своя палитра
  `P_*` отличается от палитры окна на единицы в младших битах — картинка та же.
* **FLC** — `F_*` из `MAX.RES` это ссылка на файл (`tank03.flc`); кадры
  декодируются `max_assets::flic` и пишутся PNG по собственной палитре
  `COLOR_256` кадра, `animation.gif` собирается своим GIF89a-кодировщиком
  (`src/gif.rs`: LZW + локальные палитры кадров), исходный `.FLC` копируется
  рядом.
* **`sprite.json`** — 1:1 с `FrameInfo`: `image_base`, `image_count`,
  `turret_image_*`, `firing_image_*`, `connector_image_*`, `angle_offsets`,
  плюс размеры/горячие точки каждого выгруженного кадра и габариты ленты.

## Проверки

`cargo test` в `max-brh-qa-tools/max-resources-exporter` (числа — см. таблицу
«Статистика последнего экспорта» выше):

* счётчики: 93 каталога (все `UnitType`), число портретов `P_*`,
  fallback-портретов и типов с FLC сверяется с реальными данными;
* размеры PNG совпадают с кадрами спрайта, `portrait.png` — 300×240;
* `sprite.json` соответствует `D_*` (`image_base`/`image_count`/
  `turret_*`/`firing_*`/`connector_*`/`angle_offsets`) и числу выгруженных
  кадров у каждого из 92 типов со спрайтом;
* `animation.gif` — настоящий GIF89a (разбор блоков + число кадров FLC);
* повторный экспорт даёт те же байты и не оставляет мусора.
