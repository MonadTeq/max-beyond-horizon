# Визуал M.A.X. Rust-клиента: статус 1:1 с оригиналом

**Дата среза:** 2026-11-15 · **Версия workspace:** 1.0.0 · **Задача:** довести визуал
Rust-клиента до 1:1 с оригинальной M.A.X. (эталон — C++-порт в этой же рабочей
области).

Документ фиксирует: что уже сделано, какие форматы данных разобраны, что
проверено и чем именно, что осталось и в каком порядке делать. Предназначен для
анализа дальнейших шагов.

---

## 1. Краткий статус

| Элемент визуала | Состояние | Источник данных | Где в коде |
|---|---|---|---|
| Поверхность карты (тайлы 64×64) | **1:1** | `.WRL`: `tile_ids`, `tile_count`, пиксели тайлов, палитра | `max_render::terrain::TerrainAtlas` |
| Здания (спрайты) | **1:1** (спрайты грузятся и рисуются) | `MAX.RES`: `HANGAR`, `DEPOT`, `POWERSTN`, … + тени `S_*` | `max_app::assets::UNIT_SPRITES` (39 типов) |
| Юниты (спрайты, тени, командные цвета) | **1:1** | `MAX.RES`: `TANK`, `INFANTRY`, … + `S_*` | `max_render::sprite_atlas`, `max_assets::sprite` |
| Окно и рамки интерфейса | **1:1** | `MAX.RES`: `FRAMEPIC` (640×480, своя палитра, RLE) | `max_app::chrome`, `max_render::layout` |
| Раскладка элементов (карта/миникарта/кнопки) | **1:1** по координатам C++-порта | `window_manager.cpp` `windows[]` | `max_render::layout` |
| Содержимое панелей (портрет, статистика, кнопки, таймеры) | **заглушка/пусто** | `P_*`, `I_*`, `MNUBTN*`, `UNTBTN_U/D`, `BTNHIDE`, `MENU_ON/OFF`, `RADAR` | не подключено |
| Миникарта | **процедурная** (в режиме хрома пустая рамка) | `.WRL::minimap()` + `RADAR` | не подключено |
| Шрифт | **8×8 заглушка** | шрифты GNW (`GNW_TEXT_FONT_1/5`), **не** в `MAX.RES` | `max_render::text` |
| Иконки ресурсов/обломков | **цветные точки** | `I_*` (спрайты) | `map_render::render_resources_into` |
| Курсор мыши | **системный SDL** | `MAX.RES`: 30 курсоров (`HANDPTR`, `ARROW_*`, `PTR_PATH`…) | не подключено |
| Меню (главное, create/join) | **процедурное** | `MNUBTN*U/D`, `FRAMEPIC` | `max_app::ui` |
| Экран выбора планеты | **нет** | `SNOW_PIC + world/6` (big image 128×128) | — |

Итог: **в игре** (сессия/`--new-game` при 640×480) окно и карта уже выглядят как
оригинал; панели внутри окна пока пустые, шрифт и меню — процедурные.

---

## 2. Разобранные форматы данных

### 2.1 Карта планеты `.WRL`

Разбор: `max_assets::wrl` (уже был, рендер не использовал).

```
RES0/WRL:
  заголовок (сигнатура, слои, размеры, смещения)
  minimap                  u8[?]              — карта для миникарты (уже парсится)
  tile_ids                 u16[W×H]           — id тайла на клетку (row-major)
  tile_count               u16
  tiles                    u8[tile_count*4096] — пиксели тайлов 64×64, индекс палитры
  palette                  u8[768]            — 256×RGB (может быть 6-битной VGA)
  pass_table               u8[tile_count]     — surface[i] = PassData[pass_table[tile_ids[i]]]
```

- Размер тайла — `GFX_MAP_TILE_SIZE = 64` (`max-cpp-port/src/gfx.hpp:28`), рендер
  оригинала — `Gfx_DecodeMapTile` (`max-cpp-port/src/gfx.cpp:125`).
- Пиксели и `tile_ids` в C++: `World::GetTileBuffer()` / `GetTileIds()`
  (`max-cpp-port/src/world.hpp:178-179`).
- Проверено на `SNOW_1.WRL`: 112×112 клеток, **337 уникальных тайлов**, 1 380 352
  байт пикселей; палитра — 6-битная (каналы ≤ 63), растягивается на 8 бит.

### 2.2 Картинки интерфейса `MAX.RES`

Разбор: `max_assets::image` (новый модуль, 394 строки).

**Simple image** (без своей палитры, палитра `COLOR.PAL`):

```
u16 width, u16 height, u16 ulx, u16 uly   (8 байт)
u8  pixels[width*height]                  (индексы палитры)
```

- В C++ `struct ImageSimpleHeader` имеет размер 9 (`resource_manager.hpp:44-52`):
  8 байт полей + `uint8_t transparent_color`. Но `WindowManager_DecodeSimpleImage`
  (`window_manager.cpp:755`) берёт данные с `&image->transparent_color`, т.е. с
  **8-го байта**: девятый байт структуры — это первый пиксель, отдельного поля
  прозрачности в файле нет.
- Проверка арифметикой (все сходится точно):
  | тег | размер | разбор |
  |---|---|---|
  | `PANELTOP` | 40 877 | 8 + 171×239 |
  | `PANELBTM` | 41 219 | 8 + 171×241 |
  | `MNUBTN1U/D` | 4 368 | 8 + 109×40 |
  | `MNUBTN5U/D` | 1 928 | 8 + 48×40 |
  | `UNTBTN_U/D` | 890 | 8 + 42×21 |
  | `BTNHIDE` | 806 | 8 + 38×21 |
  | `MENU_ON/OFF` | 624 | 8 + 44×14 |

**Big image** (своя палитра + RLE):

```
u16 ulx, u16 uly, u16 width, u16 height    (8 байт)
u8  palette[3*256]                          (768 байт, RGB)
u8  transparent_color                       (1 байт)
RLE по строкам: i16 n;
    n > 0  -> следующие n байт — литеральные пиксели
    n < 0  -> повторить следующий 1 байт (-n) раз
```

- `sizeof(ImageBigHeader) == 777` (`resource_manager.hpp:54-63`), декодер —
  `WindowManager_DecodeBigImage` (`window_manager.cpp:615`), загрузка —
  `WindowManager_LoadBigImage` (`window_manager.cpp:659`).
- **Ключевой нюанс (без него картинка не собирается):** кодировщик выпускает
  чанки **шире строки**. У `FRAMEPIC` первая строка: литерал 255 + повтор 1616
  при ширине 640 → 1871 пиксель на строку. C++ пишет чанк подряд по буферу
  (переливая в следующие строки), а следующая строка начинается со своего начала
  и **перекрывает хвост**. Rust-декодер воспроизводит это 1:1 (клиппинг по концу
  буфера, счётчик строки += длина чанка). Проверено: поток `FRAMEPIC` читается
  ровно до конца записи (212 750 байт данных на 640×480), последние 2 байта
  C++ читает уже за границей ресурса — у нас остаток кадра заполняется нулями.
- Разобранные big image: `FRAMEPIC` 640×480, `DEPOTFRM`/`HANGRFRM` 640×480
  (фоны диалогов), `P_DEPOT`/`P_HANGAR` 300×240 (картинки юнитов в диалогах),
  `SNOW_PIC` 128×128 (миниатюра планеты; в C++ — `SNOW_PIC + world/6`,
  `planetselectmenu.cpp:94`).

### 2.3 Спрайты зданий/юнитов

- Мультикадровые RLE-спрайты с `hotx/hoty`: `HANGAR` (10×42, 24 613 Б),
  `DEPOT` (10×42, 20 594 Б), `RADAR` (sprite sheet), тени — отдельные ресурсы
  `S_*` (78 тегов).
- Правило имён теней: `S_` + первые 6 символов имени спрайта
  (`SPLYTRCK → S_SPLYTR`, `INFANTRY → S_INFANT`).
- 89 тегов `I_*` — иконки построек (спрайт-листы: `I_DEPOT`, `I_RADAR`,
  `I_POWSTN`…; используются в меню строительства/апгрейдов, см.
  `max-cpp-port/src/resourcetable.cpp:118-119`).
- 70 тегов `P_*` — крупные картинки юнитов/зданий 300×240 (big image),
  применяются в диалогах и информационных панелях (`resourcetable.cpp:223-225`);
  для `CORNER_FLIC` 128×128 в C++ используется отдельная анимация (FLIC), а не
  `P_*` — этот момент требует уточнения при реализации портрета.
- 30 ресурсов курсоров (`HIDDNPTR`, `HANDPTR`, `ARROW_N/NE/…/NW`, `MAP_PTR`,
  `MINI_PTR`, `PTR_PATH`, `PTR_HELP`…; `cursor.cpp:55-58`):

  ```
  Cursor_Descriptor (10 байт): i16 width, height, ulx, uly; u16 frame_count
  далее u8 data[width*height*frame_count] — индексы палитры
  ```

  Например `ARROW_N` — 264 байта, `HANDPTR` — 332, `PTR_PATH` — 5 298 (анимация).

### 2.4 Раскладка окна (эталон — `window_manager.cpp:51-110`)

Окно **640×480** (`window_manager.hpp:27-28`). Координаты `WINDOW_RECT(ulx, uly, lrx, lry)`:

| Окно | Прямоугольник | Примечание |
|---|---|---|
| `WINDOW_MAIN_WINDOW` | 0,0 – 639,479 | всё окно |
| `WINDOW_MAIN_MAP` | 180,18 – 627,465 | **область карты 448×448** (7×7 клеток по 64) |
| `WINDOW_INTERFACE_PANEL_TOP` | 0,0 – 178,238 | левая панель (в ней `PANELTOP`) |
| `WINDOW_INTERFACE_PANEL_BOTTOM` | 0,239 – 178,479 | левая панель (в ней `PANELBTM`) |
| `WINDOW_ENDTURN_BUTTON` | 390,2 – 461,22 | намеренно заходит на карту по y |
| `WINDOW_TURN_COUNTER_DISPLAY` | 468,2 – 528,22 | номер хода |
| `WINDOW_TURN_TIMER_DISPLAY` | 534,2 – 594,22 | таймер |
| `WINDOW_TOP_INSTRUMENTS_WINDOW` | 384,0 – 639,24 | верхний блок инструментов |
| `WINDOW_FILES_BUTTON` / `PREFS_BUTTON` | 17,4–83,21 / 86,4–152,21 | меню сверху слева |
| `WINDOW_CORNER_FLIC` | 10,29 – 137,156 | анимация/портрет юнита |
| `WINDOW_PLAY_BUTTON` / `PAUSE_BUTTON` | 146,123–164,140 / 146,143–164,161 | |
| `WINDOW_STAT_WINDOW` (+4 строки) | 10,173 – 162,220 | статистика (строки по 10–11 px) |
| `WINDOW_CENTER/LOCK/PRE/DONE/NXT_BUTTON` | 4,227–23,249 … 125,228–162,249 | |
| `WINDOW_HELP/REPORTS/CHAT_BUTTON` | 21,250–45,273 / 51,252–98,270 / 102,252–149,270 | |
| Зум: `ZOOM_PLUS/SLIDER/MINUS` | 2,275–19,288 / 20,275–149,288 / 150,275–169,288 | |
| `SURVEY/STATUS/COLORS` | 4,298–58,313 / 4,314–58,329 / 4,330–58,347 | левый столбец |
| `HITS/AMMO/RANGE` | 59,298–113,313 / 59,314–113,329 / 59,330–113,347 | средний столбец |
| `SCAN/GRID/NAME` | 114,298–168,313 / 114,314–168,329 / 114,330–168,347 | правый столбец |
| `WINDOW_MINIMAP` | 15,356 – 126,467 | миникарта 112×112 |
| `WINDOW_2X_MINIMAP` / `TNT_MINIMAP` | 136,387–162,412 / 136,413–162,439 | |
| `WINDOW_COORDINATES_DISPLAY` | 242,456 – 331,476 | координаты клетки |
| `WINDOW_UNIT_DESCRIPTION_DISPLAY` | 338,456 – 559,476 | описание юнита |
| `WINDOW_BOTTOM_INSTRUMENTS_WINDOW` | 240,456 – 585,479 | нижний блок |
| Скролл-полосы (16 окон) | по краям: y 0–8, y 472–479, x 0–8, x 632–639 | прокрутка карты |

Загрузка фона в игре: `ResourceManager_InitInGameAssets` →
`WindowManager_LoadBigImage(FRAMEPIC, window, …, palette_from_image=true)`
(`resource_manager.cpp:1137`, `:1169`) — то есть **весь хром интерфейса уже
нарисован в `FRAMEPIC`**, а карта выводится в `WINDOW_MAIN_MAP`.

---

## 3. Что реализовано (код)

| Модуль / файл | Строк | Назначение |
|---|---|---|
| `max_assets/src/image.rs` | 394 | `SimpleImage`, `BigImage` (+ RLE с чанками через строку), `ImageError`; 8 unit-тестов |
| `max_render/src/terrain.rs` | 282 | `TerrainAtlas`: палитра `.WRL` (6→8 бит), пиксели тайлов, блит с зумом; 4 теста |
| `max_render/src/layout.rs` | 125 | константы раскладки 640×480 из `windows[]`; 3 теста (в т.ч. непересечение с картой) |
| `max_app/src/chrome.rs` | 196 | `Chrome` (`FRAMEPIC` + `PANELTOP/PANELBTM`), `draw`, `blit_map`; 3 теста |
| `max_app/src/render.rs` | +82 | режим «окно 1:1»: хром + карта в `MAIN_MAP`; `draw_units` вынесен |
| `max_app/src/ui.rs` | +25 | в сессии при активном хроме процедурный HUD не рисуется — только строка статуса в углу карты |
| `max_app/src/assets.rs` | +7 | `Assets.terrain` и `Assets.chrome` |
| `max_app/src/lib.rs` | +5 | `AppConfig::default()` → **640×480** (окно оригинала) |
| `max_assets/examples/ui_images.rs` | 99 | дамп `FRAMEPIC`/`PANELTOP`/`PANELBTM` в BMP для сверки |
| `max_render/examples/terrain.rs` | 120 | рендер всей карты и фрагмента 1:1 из `.WRL` в BMP |

Ключевые API:

```rust
// карта
TerrainAtlas::from_wrl(&WrlMap) -> TerrainAtlas
TerrainAtlas::render_into(&mut RgbaBuffer, &Viewport) -> u32
TerrainAtlas::tile_pixel(tile_id, x, y) -> Option<Rgba>

// UI-картинки
SimpleImage::decode(&[u8]) -> Result<SimpleImage, ImageError>
BigImage::decode(&[u8])    -> Result<BigImage, ImageError>
BigImage::rgb(x, y)        -> Option<[u8; 3]>   // палитра из самой картинки

// окно
Chrome::load(&ResArchive) -> Option<Chrome>
Chrome::draw(&mut RgbaBuffer) -> u32
chrome::blit_map(&mut RgbaBuffer, &RgbaBuffer) -> u32   // в layout::MAIN_MAP

// раскладка (max_render::layout)
WINDOW_WIDTH = 640, WINDOW_HEIGHT = 480, MAIN_MAP = (180,18,448,448)
MINIMAP = (15,356,112,112), END_TURN_BUTTON = (390,2,72,21), …
```

---

## 4. Свидетельства проверки

**Тесты:** `516 passed, 0 failed` (`cargo test` в `rust:1.94.0-bookworm`);
`cargo check --all-targets` — 0 ошибок.

**Артефакт (Windows UI):** `release/max_app-1.0.0-windows-x86_64-ui/`
- `max_app.exe` — 5 270 016 Б,
  sha256 `d4d68f8427a8d075f88a1741d0ff615aaebca044b46c904d4b203159b3933a18`
- `zip` — sha256 `1d84cba546495814c732316ee16449311fd83627a41c1ac1b154f70921c6cc5b`
- `SHA256SUMS` по 5 файлам — `OK`

**Изображения (в `build/`, не коммитятся):**

| Файл | Что показывает |
|---|---|
| `build/ui-framepic.png` | `FRAMEPIC`, декодированный из `MAX.RES` (640×480) |
| `build/wrl-snow-map.png` | вся карта `SNOW_1` настоящими тайлами (337 тайлов, 12 544 клетки) |
| `build/wrl-snow-detail.png` | фрагмент 1:1 (64 px/клетка): снег, скальный берег, вода |
| `build/frame-original-terrain.png` | кадр 512×384: настоящие тайлы + спрайт шахты |
| `build/frame-chrome2.png` | кадр 640×480: `FRAMEPIC` + карта ровно в `MAIN_MAP` |
| `build/ui-chrome-session-final.png` | живая сессия `--ui`: хром + карта + статус |
| `build/rle-FRAMEPIC.bmp` | независимая (Python) эмуляция RLE — сверка с Rust-декодером |

**Измерения раскладки** (по пикселям кадра `frame-chrome2.png`): карта занимает
x 180…627, y 18…465 (448×448), левее — панель (`FRAMEPIC`), правее/ниже — рамка
окна; всё совпадает с `windows[]` C++-порта.

---

## 5. Что осталось — план по приоритетам

### Пункт A. Наполнение панелей и верхней строки (наибольший визуальный эффект)

Данные готовы, нужен код отрисовки поверх `FRAMEPIC`:

1. **Верхние кнопки и счётчики** (правый верх): `End Turn` (390,2) 72×21,
   счётчик хода (468,2) 60×21, таймер (534,2) 60×21; кнопки `Files` (17,4) 67×18
   и `Preferences` (86,4) 67×18 слева.
   Ресурсы: `MNUBTN1U..4D` (109×40) и `MNUBTN5U/D..6` (48×40), `UNTBTN_U/D` (42×21), `BTNHIDE` (38×21),
   `MENU_ON/OFF` (44×14) — все уже декодируются как `SimpleImage`.
2. **Портрет/анимация юнита** — `WINDOW_CORNER_FLIC` (10,29) 128×128.
   Для статичного портрета: `P_*` (300×240, big image) или `I_*` (спрайты);
   для анимации — FLIC (`FLC`), пока не разобран.
3. **Статистика юнита** — `WINDOW_STAT_WINDOW` (10,173) 153×48, четыре строки
   (HP/ammo/speed…), значения — из `HudState`/`UnitInfo` (уже есть в
   `max_app::hud`).
4. **Сетка кнопок** SURVEY/STATUS/COLORS, HITS/AMMO/RANGE, SCAN/GRID/NAME
   (по 55×16/18, координаты в §2.4) — это переключатели режимов HUD; сейчас
   логики нет, нужны как минимум состояния «нажата» через `MNUBTN*D`.
5. **Зум-слайдер** (20,275) 130×14 и кнопки `+`/`−` — привязать к
   `Viewport::set_zoom` (API уже есть).
6. **Нижняя строка** координат (242,456) 90×21 и описания юнита (338,456)
   222×21 — текст потребует шрифта (пункт B).

**Критерий готовности:** скриншот сессии совпадает с `build/smoke-window.png`
по расположению всех подписей/кнопок (пиксельная сверка панелей).

### Пункт B. Шрифт оригинала

- В C++-порте текст рисуется через `Text_SetFont(GNW_TEXT_FONT_1/5)` — это
  растровые шрифты библиотеки **GNW**, а не ресурс `MAX.RES` (проверено: среди
  1 828 тегов нет `FONT`/`FON`; есть только `FRAMEPIC` и иконки).
- Варианты: (1) извлечь битмапы шрифтов GNW из исходников C++-порта и перенести
  в `max_render::text`; (2) пока оставить 8×8 и подключить текст позже.
- От шрифта зависят: таймер/счётчик хода, статистика, координаты, описания,
  меню, чат, журнал.

**Критерий:** текст в окне совпадает с оригиналом по начертанию и метрикам
(ширина/высота глифа, межстрочный интервал).

### Пункт C. Миникарта

- В `.WRL` уже есть `minimap` (парсится как `WrlMap::minimap()`), в C++ —
  `World::GetMinimap()` / `GetMinimapFov()` (fov-маска), размеры/смещение окна
  миникарты берутся из `WrlMap`.
- Рамка миникарты: `RADAR` (спрайт-лист, 60 449 Б); кнопки `2x` (136,387) и
  `TNT` (136,413) — 26×26.
- Нужно: нарисовать миникарту в `WINDOW_MINIMAP` (15,356) 112×112, подсветить
  прямоугольник камеры и FOV.

### Пункт D. Иконки, курсоры и мелочи

- Иконки ресурсов/обломков (`I_*`) вместо цветных точек (сейчас
  `render_resources_into` рисует точки).
- **Курсоры** — в `MAX.RES` 30 ресурсов (`HANDPTR`, `ARROW_*`, `MAP_PTR`,
  `MINI_PTR`, `PTR_PATH`…), формат `Cursor_Descriptor` (10 байт + кадры, см.
  §2.3). Нужен декодер и переключение по контексту (карта/меню/приказ), как
  `Cursor_SetCursor` в C++ (`cursor.cpp`).
- Экран выбора планеты: `SNOW_PIC + world/6` (big image 128×128) —
  `planetselectmenu.cpp:94`.

### Пункт E. Меню на оригинальных ресурсах

- Главное меню/список игр — сейчас процедурные (`max_app::ui::build_main_menu`
  и `build_join_list` на `max_render::ui`).
- Заменить на `FRAMEPIC` + `MNUBTN*U/D` + `BTNHIDE`, координаты кнопок брать из
  C++-порта (`menu.cpp`, `abstractmenu.hpp`).

### Пункт F. Масштабирование окна (для «настоящего 1:1» на любом мониторе)

- Сейчас 1:1 работает только при кадре ровно 640×480 (`AppConfig::default`).
  C++-порт умеет масштабировать: `WindowManager_GetScale()`
  (`window_manager.cpp:872`), `WindowManager_ScaleWindow` (`:122`),
  `WindowManager_RescaleSimpleImage`, `WindowManager_ScaleButtonResource`
  (`:134`), `WindowManager_ScaleWindows` (`:200`).
- Нужно решить: (1) фиксированные 640×480 с апскейлом всего кадра (простое,
  пиксель-в-пиксель); (2) масштабирование каждого элемента (как в C++, дороже).

---

## 6. Риски и подводные камни (уже наступили или известны)

1. **RLE big image с чанками через строку** — при наивном «построчном» декодере
   `FRAMEPIC` не собирается (`RLE-строка 0: позиция 255 вне строки`). Решение
   воспроизводит C++ (см. §2.2). Не менять без сверки с `build/ui-framepic.png`.
2. **8-байтный заголовок simple image**, а не 9: `9 + w*h` не сходится на
   `PANELTOP/PANELBTM` (`40878 ≠ 40877`). Не «исправлять» обратно.
3. **Хвост потока big image**: последние 2 байта C++ читает за границей записи;
   у нас остаток кадра = нули. Если появится рассинхрон картинки — смотреть сюда.
4. **Координаты мыши**: при хроме карта занимает (180,18) 448×448, а `Viewport`
   приложения — 448×448. Клики окна **не смещены** на (180,18): в
   `main.rs::UiSessionSink` координаты из SDL идут как есть. Это надо учесть при
   подключении кликов по панелям (пункт A) и при кликах по карте (сейчас карта
   в сессии рисуется через `render_with_chrome`, но ввод `handle_session_input`
   считает экран 640×480 — требует отдельной проверки/правки).
5. **`hud.rs` (74 КБ) рисует процедурный HUD**; в сессии при активном хроме он
   отключён. Включать его обратно нельзя, пока не перенесём его элементы на
   оригинальные координаты — иначе снова «два HUD».
6. **Палитра `FRAMEPIC` своя** (не `COLOR.PAL`), 8-битные каналы. Не смешивать с
   палитрой спрайтов: спрайты берут `COLOR.PAL` + brightness LUT.
7. **`P_*` 300×240** — крупные картинки юнитов/зданий для диалогов и
   информационных панелей (`resourcetable.cpp:223-225`), а не портрет 128×128
   для `CORNER_FLIC`: там в C++ отдельная FLIC-анимация (`PlayerCorner`), формат
   FLC пока не разобран.
8. **Курсоры — отдельный формат** (`Cursor_Descriptor`, 10 байт + кадры), не
   `SimpleImage`/`BigImage`; лежат в `MAX.RES` (30 ресурсов), не в `COLOR.PAL`.

---

## 7. Как воспроизвести проверки

```sh
# 1) окно 1:1 в кадре (без SDL): FRAMEPIC + карта
release/max_app-1.0.0-windows-x86_64-ui/max_app.exe --new-game --world 0 --seed 42 \
    --width 640 --height 480 --out build/frame-chrome2.ppm
powershell -File build/ppm2png.ps1 -Ppm build/frame-chrome2.ppm -Png build/frame-chrome2.png

# 2) дамп UI-картинок из MAX.RES (сверка с оригиналом)
cargo run -p max_assets --example ui_images -- ../max-original/MAX.RES build/ui

# 3) вся карта и фрагмент 1:1 настоящими тайлами
cargo run -p max_render --example terrain -- ../max-original/SNOW_1.WRL build/wrl-snow [cx cy]

# 4) тесты
cargo check --all-targets && cargo test          # 516 passed

# 5) живая сессия в окне
docker compose up -d --build                     # Horizon + headless
release/max_app-1.0.0-windows-x86_64-ui/max_app.exe --ui \
    --handshake-patches build/nonexistent.res --turn-wait 45
```

---

## 8. Состояние репозитория

Изменено (не закоммичено): `max-rust/crates/max_app/src/{assets,lib,main,render,ui}.rs`,
`max-rust/crates/max_assets/src/lib.rs`, `max-rust/crates/max_render/src/lib.rs`,
`max-rust/README.md`, `docs/release-checklist.md`.

Новые файлы: `max_assets/src/image.rs`, `max_assets/examples/ui_images.rs`,
`max_render/src/{layout,terrain}.rs`, `max_render/examples/terrain.rs`,
`max_app/src/chrome.rs`.

Ранее закоммичено: `6295a8e` (Windows-кросс-сборка + `build-windows.sh`),
`ba8279a` (нативная Windows UI-сборка + `build-windows-ui.ps1`), плюс правки
`--turn-wait` и шрифта (коммиты делались между итерациями).

---

## 9. Вопросы, которые надо решить перед следующим этапом

1. **Масштаб:** фиксированные 640×480 с апскейлом кадра или масштабирование
   элементов как в C++ (пункт F)? От этого зависит, где хранить координаты
   (константы 640×480 или пересчёт).
2. **Шрифт:** тащить битмапы GNW в репозиторий (лицензия GNW?) или рисовать
   свой шрифт, близкий к оригиналу?
3. **Портрет юнита:** FLIC-анимация (нужен разбор FLC) или статичная картинка
   (`P_*`/`I_*`) как первый шаг?
4. **Приоритет:** панели (A) или меню (E)? Меню проще (кнопки+рамка уже
   декодируются), но в игре чаще смотрят на панели.
5. **Миникарта:** только `.WRL::minimap()` или ещё FOV-маска (`GetMinimapFov`)?
