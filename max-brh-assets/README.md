# max-brh-assets — изображения ресурсов M.A.X.

Каталог содержит **только** `README.md`: сами PNG регенерируются из
`max-original/MAX.RES` и в git не коммитятся (см. `.gitignore`,
`max-brh-assets/images/`). Манифест и отчёт тоже генерируются.

## Что здесь лежит

```
max-brh-assets/
├── README.md
└── images/                     # генерируется, в git не входит
    ├── ui/                     # 536 файлов: фон окна, панели, кнопки, поля
    ├── sprites/units/          # 170 файлов: юниты/здания/эффекты + тени
    ├── misc/                   # 363 файла: иконки, портреты, маркеры, курсоры
    ├── manifest.csv            # все 1828 записей: kind/category/offset/size/…
    └── EXPORT-REPORT.txt       # сводка последнего экспорта
```

Экспорт сделан утилитой
[`max-brh-qa-tools/max-resources-viewer`](../max-brh-qa-tools/max-resources-viewer/README.md).

## Как перегенерировать

Из корня репозитория:

```sh
cd max-brh-qa-tools/max-resources-viewer
cargo run --release -- --export \
  ../../max-original/MAX.RES \
  ../../max-original/COLOR.PAL \
  ../../max-brh-assets
```

`--export` работает **без SDL** (headless), поэтому достаточно Rust-тулчейна.
Повторный запуск побайтово перезаписывает те же PNG (кодек без времени и
случайности); это проверяет интеграционный тест `real_res.rs`.
Сводка экспорта сохраняется в `<OUT_ROOT>/export-log.txt`, полный отчёт по
записям — в `<OUT_ROOT>/images/EXPORT-REPORT.txt` (у готового `.exe` нет
консоли — подсистема GUI, поэтому stdout виден только при `cargo run` или
перенаправлении).

Быстрый отчёт без записи файлов:

```sh
cargo run --release -- --report ../../max-original/MAX.RES
```

## Статистика реального MAX.RES (2026-11-15)

| Показатель | Значение |
|---|---:|
| Всего записей в таблице | **1828** |
| Уникальных тегов | 1827 (`PRFDNE_D` встречается дважды) |
| Изображений (записей) | **1070** |
| Записано PNG (уникальных путей) | **1069** |
| Не-изображений | **758** |
| Объём PNG | ≈ 10,1 МБ |

### По категориям

| Категория | Записей | Файлов | Что это |
|---|---:|---:|---|
| `ui` | 537 | 536 | `FRAMEPIC`, `PANELTOP/BTM`, `MNUBTN*`, `BLANK_*`, `ENDGAME*`, `STAT_*`, `GRID_*`, `FILES_*`, … |
| `sprites/units` | 170 | 170 | 92 спрайта `COMMTWR`…`WALDO` + 78 теней `S_*` |
| `misc` | 363 | 363 | `I_*` иконки (70 sprite + 19 simple), `P_*`/`A_*` портреты, маркеры `RAWMSK*`/`FUELMK*`/`GOLDMK*`/`SMRWMK*`/`SMFLMK*`/`SMGDMK*`, `SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC`/`STAR_PIC`, `CLN*LOGO`, `CH_CN*`/`CH_TM*`, 29 курсоров, `E_DEPOT`/`E_DOCK`/`E_HANGAR` |

### По формату

| kind | Записей | Как читается |
|---|---:|---|
| `simple` | 649 | `ImageSimpleHeader` (8 байт) + палитра `COLOR.PAL` |
| `big` | 152 | `ImageBigHeader` (776 байт) + RLE + собственная палитра |
| `sprite` | 162 | мультикадровый RLE, кадры/строки по offset-таблицам |
| `shadow` | 78 | `S_*`: пары `{skip,count}` без пикселей → маска |
| `cursor` | 29 | `Cursor_Descriptor`: кадры уложены вертикально |

### Не-изображения (758)

| Тип | Кол-во | Примеры |
|---|---:|---|
| `wav-ref` | 607 | `TANK1` → `tank1.spw`, `KBUY0` → `kbuy0.spw` |
| `flc-ref` | 71 | `F_TANK` → `tank03.flc` |
| `wrl-ref` | 24 | `SNOW_1` → `snow_1.wrl`, `DESERT_6` → `desert_6.wrl` |
| `msc-ref` | 16 | `MAIN_MSC` → `main_msc.msc` |
| `mve-ref` | 3 | `INTROFLC` → `maxint.mve` |
| `script` | 3 | `ATTRIBS`, `SOUNDVOL`, `CLANATRB` (текст `;`/`[`) |
| `text` | 2 | `HELP_ENG`, `TIPS` |
| `data` | 32 | `D_*` (8/24 байта двоичных таблиц) |

`FONT_1`/`FONT_2`/`FONT_5` (TrueType) лежат не в `MAX.RES`, а в
`assets/PATCHES.RES`; классификатор и редактор их также понимают (`ttf`).
TTF/WAV/карты/скрипты **не экспортируются** — только перечисляются в
`manifest.csv` и отчёте.

## Формат имён и раскладка

`<TAG>_<W>x<H>[_<N>f][_ownpal|_mask].png`

* `WxH` — первый кадр (у simple/big — вся картинка);
* `_Nf` — число кадров; кадры лежат **горизонтальной лентой**;
* `_ownpal` — big image со своей палитрой (иначе `COLOR.PAL`);
* `_mask` — тень `S_*`.

Примеры:

```
images/ui/FRAMEPIC_640x480_ownpal.png
images/ui/PANELTOP_171x239.png
images/sprites/units/TANK_44x45_24f.png
images/sprites/units/S_TANK_49x53_24f_mask.png
images/misc/P_TANK_300x240_ownpal.png
images/misc/HANDPTR_18x18.png
```

### Как рисуются пиксели

* `simple`, `sprite`, `cursor` → `COLOR.PAL`; компоненты уже умножены на 4
  (`6 бит → 8 бит`, как в SDL-порте);
* `big` → 768-байтная палитра из заголовка (8-битные компоненты);
* индекс `transparent` (simple/big/cursor) → **альфа 0**;
* структурная прозрачность спрайта (пропущенные пиксели RLE) → альфа 0;
* тень → чёрный `(0,0,0)` с альфой **160** на прозрачном фоне;
* полностью непрозрачные кадры пишутся RGB (индексированный/truecolor PNG),
  кадры с прозрачностью — RGBA (color type 6). Оба варианта читаются
  стандартными просмотрщиками.

### Лента кадров

Кадры выравниваются по горячей точке (`hot_x/hot_y`), поэтому анимация не
«прыгает». Ячейка кадра расширяется на `anchor - hot`, если горячая точка
отрицательная (`BLDEXPLD`: `hotx = -13`). Ширина/высота ленты — сумма ячеек и
максимум высот; подробности — в `src/render.rs`.

## Категории: правила

* `sprites/units` — `S_*` (тени) и 92 имени из `model::UNIT_SPRITES`
  (`COMMTWR`…`WALDO`, включая взрывы/ракеты/мины). `RADAR`, `GUNTURRT` и т.п. —
  это спрайты зданий, поэтому они здесь, а не в `ui`.
* `misc` — курсоры, `I_*`, `P_*`, `A_*`, `E_*`, маркеры, картинки планет,
  `CLN*LOGO`, `CH_CN*`/`CH_TM*`, `ILOGO`.
* `ui` — все остальные изображения (хром окна, панели, кнопки, поля,
  счётчики, `SI_*`/`EI_*` и т.п.).

## Проверки

* 1828/1828 записей получают `kind` и категорию;
* известные теги: `FRAMEPIC → ui/big 640×480`, `TANK → sprites/units/sprite
  24 кадра`, `S_TANK → sprites/units/shadow`, `HANDPTR → misc/cursor 18×18`,
  `SNOW_PIC → misc/big 128×128`, `TANK1 → wav-ref`, `FONT_1 → ttf`;
* повторный экспорт избранного (`TANK`/`FRAMEPIC`/`SNOW_PIC`) даёт те же байты.

См. `max-brh-qa-tools/max-resources-viewer/tests/real_res.rs`.
