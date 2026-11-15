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
| Содержимое панелей (портрет, статистика, кнопки, таймеры) | **1:1** (кроме портрета-анимации) | `FILES_OF/ON`, `PREF_OFF/ON`, `ENDTRN_U`, `TURNS`, `TIMER`, `XYPOS`, `UNITNAME`, `SURV_*`, `STAT_*`, `COLOR_*`, `HITS_*`, `AMMO_*`, `RANG_*`, `VISN_*`, `GRID_*`, `NAMES_*`, `ZOOMPNL1`, `ZOOMPTR`, `FIND_OFF`, `PREV_OF`, `UDONE_OF`, `NEXT_OF`, `HELP_OF`, `REPT_OFF`, `CHAT_OFF`, `MIN2X_OF`, `MINFL_OF` | `max_app::panels` |
| Миникарта | **1:1** (FOV-маска, рамка камеры, точки юнитов, `2x`/`TNT`; рамка — из `FRAMEPIC`) | `.WRL::minimap()`, палитра окна + `.WRL` 64…159, `RAWMSK*`/… | `max_app::panels::draw_minimap` |
| Шрифт | **1:1 (растровые GNW)** | `FONT1.FON`, `FONT2.FON`, `FONT5.FON` (копии — `max_render/data/`), сверены с FreeType-путём порта | `max_render::text::gnw` |
| Иконки ресурсов/обломков | **1:1** (`RAWMSK*`/`FUELMK*`/`GOLDMK*` + малые при `zoom ≤ 16`; `I_*` — fallback) | `MAX.RES`: 99 маркеров; обломки — спрайты `ALNTANK/ALNASGUN/ALNPLANE` + плита/мост/здания | `max_assets::markers`, `max_render::map_render::render_resource_icons_into` |
| Курсор мыши | **1:1 по формату и контексту** (SDL `Cursor::from_surface` за feature `sdl`) | `MAX.RES`: 30 курсоров (`HANDPTR`, `ARROW_*`, `PTR_PATH`…) | `max_assets::cursor`, `max_app::cursor` |
| Меню (главное, create/join) | **процедурное** | `MNUBTN*U/D`, `FRAMEPIC` | `max_app::ui` |
| Экран выбора планеты | **сетка 24 планет** на `SNOW_PIC + world/6` + имена; числовой ввод — fallback | `MAX.RES`: `SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC` (128×128) | `max_app::ui::{planet_grid_rect,draw_planet_picker}` |

Итог: **в игре** (сессия/`--new-game` при 640×480) окно, карта и панели уже
выглядят как оригинал: верхние кнопки/счётчики, портрет (статичный кадр
спрайта), четыре строки статистики, сетка переключателей, зум, координаты,
описание юнита, **миникарта с FOV-маской/точками юнитов/`2x`/`TNT`** и
**настоящие маркеры ресурсов** рисуются на координатах и ресурсах C++-порта,
текст — оригинальными растровыми шрифтами GNW, курсоры и сетка выбора планеты
подключены. Остались анимация портрета (FLIC), меню на `MNUBTN*` (E) и
масштабирование элементов (F).

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
  `max-cpp-port/src/resourcetable.cpp:118-119`). **На карте ресурсы рисуются не
  `I_*`**, а маркерами `RAWMSK*`/`FUELMK*`/`GOLDMK*` (большие 62×62, hotspot
  31,31) и `SMRWMK*`/`SMFLMK*`/`SMGDMK*` (малые 14×16, hotspot 7,8) —
  `Survey_RenderMarker` (`survey.cpp:32-82`): при `zoom > 16` кадр
  `min(amount,16)`, иначе `amount-1`; `I_RAW`/`I_FUEL`/`I_GOLD` — иконки
  интерфейса и запасной вариант рендера.
- 70 тегов `P_*` — крупные картинки юнитов/зданий 300×240 (big image),
  применяются в диалогах и информационных панелях (`resourcetable.cpp:223-225`);
  для `CORNER_FLIC` 128×128 в C++ используется отдельная анимация (FLIC), а не
  `P_*` — этот момент требует уточнения при реализации портрета.
- 30 ресурсов курсоров (`HIDDNPTR`, `HANDPTR`, `ARROW_N/NE/…/NW`, `MAP_PTR`,
  `MINI_PTR`, `PTR_PATH`, `PTR_HELP`…; `cursor.cpp:55-58`). Вопреки первому
  предположению, **отдельного `u16 frame_count` в файле нет**: курсор — это
  обычный `ImageSimpleHeader` (8 байт полей, 9-й байт — первый пиксель и он же
  прозрачный индекс), а кадры анимации уложены вертикально:

  ```
  i16 width, height, ulx, uly;      // 8 байт, ulx/uly — hotspot
  u8  pixels[width*height*frame_count]
  frame_count = 1;                  // C++: если width < height,
  if (width < height) {             // frame_count = height / width (нацело),
      frame_count = height / width; // height /= frame_count
      height /= frame_count;
  }
  ```

  Примеры: `ARROW_N` — 264 Б (16×16, hotspot 7,1), `HANDPTR` — 332 Б (18×18,
  0,1), `ENMY_PTR` — 1 229 Б (37×33, 18,13), `PTR_PATH` — 5 298 Б (10 кадров
  23×23, hotspot 13,13), `HIDDNPTR` — 9 Б (1×1).

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

### 2.5 Миникарта, FOV и маркеры (порт `game_manager.cpp`/`access.cpp`/`survey.cpp`)

- **Рамка миникарты — в `FRAMEPIC`**, а не отдельный ресурс. C++
  `ResourceManager_InitInGameAssets` сохраняет снимок окна `WINDOW_MINIMAP` в
  `ResourceManager_MinimapBgImage`, а `GameManager_RenderMinimap` кладёт его
  первым слоем. `RADAR` (60 449 Б) — это **спрайт здания-радара**: 20 кадров
  64×64 с hotspot (32,32), он рисуется как обычный юнит; к миникарте отношения
  не имеет. Проверка — тест `alien_derelict_sprites_are_loaded`.
- **Палитра миникарты** — системная палитра окна (`FRAMEPIC`), в которой
  индексы 64…159 заменены палитрой `.WRL` (`World::ApplyPaletteAndTints`,
  `world.cpp:329-334`, палитра `.WRL` делится на 4 и снова растягивается при
  выводе). Поэтому мини-карта и маркеры `I_*`/`RAWMSK*` используют палитру
  `FRAMEPIC`, а тайлы карты — палитру `.WRL`; смешивать их нельзя.
- **FOV** (`Access_UpdateMinimapFogOfWar`, `access.cpp:896-910`): слой
  `MinimapFov` = копия `Minimap`, затем клетки с `heat_map.cells[i].complete == 0`
  прогоняются через `ResourceManager_DarkeningColorIndexTable`. Таблица
  строится из палитры (`world.cpp:493-508`): `max(7, c/4) * 31 / 63`, затем
  `Color_MapColor` (пропуская индексы 9…31 и 96…127). В Rust heat map уже
  портирован (`GameState::ai.heat_maps[team].cells()[i].complete`), поэтому
  маска — точный порт, а не эвристика по юнитам.
- **Точки юнитов** (`DrawMap_RenderMiniMapUnitList`): только
  `visible_to_team`, `order != Idle`, `SELECTABLE`; `TNT` дополнительно
  оставляет юнитов с `HAS_FIRING_SPRITE | FIRES_MISSILES`. Цвета — индексы
  `1/2/3/0xFF/4` (red/green/blue/white/yellow), точка 2×2, в 2x у не-зданий —
  1 клетка.
- **`2x`** (`GameManager_RenderMinimap`): после отрисовки окна и рамки камеры
  область 56×56 от `offset/2 + GridCenterOffset` растягивается на 112×112
  (рамка камеры при этом тоже масштабируется); `GameManager_GetMinimapPosition`
  делит координату клика на 2 и прибавляет смещение. `TNT` — фильтр точек
  (см. выше), сами пиксели миникарты не меняет.

---

## 3. Что реализовано (код)

| Модуль / файл | Строк | Назначение |
|---|---|---|
| `max_assets/src/image.rs` | 394 | `SimpleImage`, `BigImage` (+ RLE с чанками через строку), `ImageError`; 8 unit-тестов |
| `max_assets/src/cursor.rs` | 360 | `CursorDescriptor`/`CursorSet`: 30 курсоров, кадры из `height/width`, hotspot; 4 теста (включая реальные `MAX.RES`) |
| `max_assets/src/markers.rs` | 190 | `ResourceMarkers`: 99 маркеров `RAWMSK*`/`FUELMK*`/`GOLDMK*` + малые + fallback `I_*`; 1 тест на реальном `MAX.RES` |
| `max_app/src/cursor.rs` | 350 | `CursorId` (30), `CursorContext`, `session_cursor`, `cursor_image` (палитра → RGBA/hotspot); 4 теста |
| `max_render/src/terrain.rs` | 282 | `TerrainAtlas`: палитра `.WRL` (6→8 бит), пиксели тайлов, блит с зумом; 4 теста |
| `max_render/src/layout.rs` | 125 | константы раскладки 640×480 из `windows[]`; 3 теста (в т.ч. непересечение с картой) |
| `max_app/src/chrome.rs` | 196 | `Chrome` (`FRAMEPIC` + `PANELTOP/PANELBTM`), `draw`, `blit_map`; 3 теста |
| `max_render/src/text/gnw.rs` | 788 | `GnwFont`/`GnwFontId`: декодер `.FON` GNW, `draw_text_gnw`, `draw_text_gnw_shadowed`, центрирование; 10 тестов (включая сверку с C++-эталоном) |
| `max_render/data/gnw_font_{1,2,5}.fnt` | 6932/4399/4868 | оригинальные `FONT1/2/5.FON` (растровые шрифты, вкомпилированы через `include_bytes!`) |
| `max_app/src/panels.rs` | 1700+ | `Panels`: 30+ картинок панелей из `MAX.RES`, 27 кнопок, портрет, статистика, миникарта (FOV/2x/TNT/точки/рамка), зум, координаты; `PanelState` в `HudState`; 15 тестов |
| `max_app/src/render.rs` | +60 | режим «окно 1:1»: хром + карта в `MAIN_MAP`; маркеры ресурсов вместо точек, `RenderStats::resource_icons`; `draw_units` вынесен |
| `max_render/src/map_render.rs` | +170 | `render_resource_icons_into`: большие/малые маркеры, масштаб `zoom/64` и `zoom/16`, fallback `I_*` |
| `max_render/src/sdl_window.rs` | +80 | `set_cursor_rgba`: кэш `Cursor` по ключу, `Cursor::from_surface`, hotspot |
| `max_app/src/ui.rs` | +330 | сетка 24 планет (`planet_grid_rect`/`planet_cell_rect`/`planet_cell_at`), `PLANET_NAMES`, отрисовка `SNOW_PIC`-миниатюр, клик по планете; числовой ввод — fallback; 3 теста |
| `max_app/examples/visual_cd.rs` | 230 | кадры C/D: игра (маркеры/FOV/2x), экран планет, контактный лист курсоров |
| `max-rust/tools/font_vectors/main.cpp` + `vectors.txt`/`render.txt` | 388 + 34 КБ | эталонный генератор: FreeType-путь C++-порта и `.FON`-декодер, побайтовое сравнение |
| `max_app/examples/panel_frame.rs` | 80 | кадр 640×480 с выбранным юнитом (доказательство пункта A) |
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

// шрифт GNW (оригинальный растровый)
GnwFontId::{Font1,Font2,Font5}.font() -> &'static GnwFont
GnwFont::decode(&[u8]) -> Result<GnwFont, GnwFontError>
GnwFont::text_width(&str) -> i32, GnwFont::line_height() -> i32
text::draw_text_gnw(&mut RgbaBuffer, x, y, font, text, color) -> i32
text::draw_text_gnw_shadowed(buf, x, y, font, text, base, outline, shadow) -> i32
// Старый 8×8 остался fallback'ом: draw_text / draw_text_line / wrap_text.

// панели поверх FRAMEPIC
Panels::load(&ResArchive) -> Option<Panels>
Panels::draw(&mut RgbaBuffer, &App) -> u32
panels::panel_button_at(x, y) -> Option<PanelButton>    // клики по хрому
panels::screen_to_map(x, y) -> Option<(i32, i32)>      // (180,18) → (0,0)
panels::minimap_grid_at(x, y, w, h, &Viewport, two_x)  // клик → клетка (2x учтён)
panels::minimap_view(w, h) -> MinimapView              // World::CalculateMinimapScaling
panels::darkening_table(&PaletteRgba) -> [u8; 256]     // FOV-затемнение
App::map_viewport() -> Viewport                        // 448×448 для карты
App::chrome_active() -> bool

// курсоры (формат и контекст)
CursorSet::load(&ResArchive) -> CursorSet              // 30 ресурсов
CursorDescriptor::decode(&[u8]) -> Result<...>         // 8-байтный заголовок + кадры
CursorId::ALL / CursorId::resource_name()
cursor_for_context(CursorContext) -> CursorId          // меню/панель/карта/приказ
cursor_image(&CursorSet, &PaletteRgba, CursorId, frame) -> Option<CursorImage>
window.set_cursor_rgba(key, rgba, w, h, hot_x, hot_y)  // SDL3, feature sdl

// ресурсы и планеты
ResourceMarkers::load(&ResArchive) -> ResourceMarkers  // 99 маркеров
map_render::render_resource_icons_into(dst, map, viewport, markers, palette, team, show_all)
Assets::planet_picture(Planet) -> Option<&BigImage>    // SNOW_PIC + world/6
ui::planet_cell_rect(w, h, index) / planet_cell_at(w, h, x, y) / planet_name(planet)

// раскладка (max_render::layout)
WINDOW_WIDTH = 640, WINDOW_HEIGHT = 480, MAIN_MAP = (180,18,448,448)
MINIMAP = (15,356,112,112), END_TURN_BUTTON = (390,2,72,21), …
TURN_COUNTER = (468,2,61,21), TURN_TIMER, ZOOM_SLIDER, COORDINATES, …
```

---

## 4. Свидетельства проверки

**Тесты:** `559 passed, 0 failed` (`cargo test` в `rust:1.94.0-bookworm`);
`cargo check --all-targets` — 0 ошибок; `cargo check -p max_app --features
sdl-from-source` — 0 ошибок (Linux, полный набор X11/ALSA dev-пакетов).
Из них новые (пункты C/D): 4 теста `max_assets::cursor` (включая реальные
размеры/hotspot/кадры всех 30 курсоров `MAX.RES`), 1 тест
`max_assets::markers` (99 маркеров), 4 теста `max_app::cursor` (индексы/контекст/
RGBA/сессия), 9 тестов `max_app::panels` (FOV-таблица, масштаб миникарты, клик 1x/2x,
точки юнитов/TNT), 1 тест `max_render::map_render` (маркеры из `MAX.RES`),
3 теста `max_app::ui` (клик по планете, сетка, картинки `SNOW_PIC`) и
2 теста `max_app::assets` (обломки `ALN*` + `RADAR`, 4 картинки планет).

**Артефакт (Windows UI):** `release/max_app-1.0.0-windows-x86_64-ui/`
- `max_app.exe` — 5 402 112 Б,
  sha256 `6d17aeafcb1851e2e8383303016d1ad409f539ee3dcc8aa6e544bbbf6b5827c0`
- `zip` — 3 054 781 Б,
  sha256 `021667b6adf59ada1cf1a2c62e635b04f941c3e721c903b2a6796e26bf19f594`
- `SHA256SUMS` по 5 файлам — `OK`; `max_app.exe --help` и `--new-game
  --width 640 --height 480` отрабатывают (кадр `build/frame-cd-windows.png`).

**Изображения (в `build/`, не коммитятся):**

| Файл | Что показывает |
|---|---|
| `build/ui-framepic.png` | `FRAMEPIC`, декодированный из `MAX.RES` (640×480) |
| `build/wrl-snow-map.png` | вся карта `SNOW_1` настоящими тайлами (337 тайлов, 12 544 клетки) |
| `build/wrl-snow-detail.png` | фрагмент 1:1 (64 px/клетка): снег, скальный берег, вода |
| `build/frame-original-terrain.png` | кадр 512×384: настоящие тайлы + спрайт шахты |
| `build/frame-chrome2.png` | кадр 640×480: `FRAMEPIC` + карта ровно в `MAIN_MAP` |
| `build/frame-panels.png` | **пункт A:** кадр 640×480 с выбранным юнитом: верхние кнопки/End Turn/счётчики, портрет, 4 строки статистики, сетка, зум, координаты/описание, миникарта |
| `build/frame-newgame-panels.png` | тот же кадр из `--new-game --width 640 --height 480` (выбор первого юнита игрока) |
| `build/frame-panels-left.png` | левая панель `frame-panels.png` в 2× (детали подписей/статистики) |
| `build/frame-windows-artifact.png` | тот же кадр, отрисованный собранным `max_app.exe` (Windows UI-артефакт); пиксель-в-пиксель с Linux-кадром |
| `build/frame-cd-game.png` | **пункт C/D:** 640×480: маркеры ресурсов `RAWMSK*`/`GOLDMK*` (не точки), миникарта с FOV-маской, рамкой камеры, точками юнитов, кнопки `2x`/`TNT` |
| `build/frame-cd-game-2x.png` | тот же кадр в режиме `2x` (миникарта 56×56 → 112×112, кнопка нажата) |
| `build/frame-cd-game-minimap-4x.png` | кроп миникарты 4× (виден перепад FOV и красная рамка камеры) |
| `build/frame-cd-planets.png` | **пункт D:** форма создания игры — сетка 24 планет на `SNOW_PIC + world/6`, имя под каждой, красная рамка выбранной, поле `World` как fallback |
| `build/frame-cd-cursors.png` | все 30 курсоров из `MAX.RES` (контактный лист: hotspot/кадры/палитра) |
| `build/frame-cd-windows.png` | тот же игровой кадр, отрисованный собранным `max_app.exe` (Windows UI) |
| `build/ui-chrome-session-final.png` | живая сессия `--ui`: хром + карта + панели |
| `build/rle-FRAMEPIC.bmp` | независимая (Python) эмуляция RLE — сверка с Rust-декодером |

**Измерения раскладки** (по пикселям кадра `frame-chrome2.png`): карта занимает
x 180…627, y 18…465 (448×448), левее — панель (`FRAMEPIC`), правее/ниже — рамка
окна; всё совпадает с `windows[]` C++-порта.

---

## 5. Пункты плана: статус (A–D сделаны)

### Пункт A. Наполнение панелей и верхней строки — **сделано**

Реализация — `max_app::panels` (`Panels::load`/`draw`), координаты — из
`windows[]`, картинки — из `MAX.RES`, подписи — из `language.json`, цвета —
индексы палитры `FRAMEPIC` (в игре системная палитра — её, а не `COLOR.PAL`;
поэтому `UiPalette` в `panels.rs`).

1. **Верхние кнопки и счётчики:** `FILES_OF/ON` (17,4) 67×18,
   `PREF_OFF/ON` (86,4) 67×18, `ENDTRN_U` (390,2) 72×21, `TURNS` (468,2)
   61×21, `TIMER` (534,2) 61×21. Подписи — `FontColor(0xA2,0xA7,0xE6)`,
   у End Turn — `(0xA0,0xA2,0xE6)` и `GNW_TEXT_FONT_5`.
   *Уточнение против плана:* `Files`/`Preferences` — это `FILES_OF/ON` и
   `PREF_OFF/ON` (67×18, ровно окно), а не `MENU_ON/OFF`; `UNTBTN_U/D` (42×21)
   и `BTNHIDE` (38×21) в C++-порте для End Turn не используются.
2. **Портрет** `CORNER_FLIC` (10,29) 128×128: чёрный фон, кадр спрайта
   выбранного юнита (`Assets::sprite` → `frame_for` + тень), увеличенный ×2;
   сверху — имя и статус (`GNW_TEXT_FONT_2`). FLIC-анимация (`PlayerCorner`)
   остаётся неразобранной.
3. **Статистика** `STAT_WINDOW` (10,173) 153×48: четыре строки по 12 px
   (`WINDOW_STAT_ROW_1..4`), значения `Hits/Ammo/Speed/Shots` из `UnitInfo`
   (`ATTRIB_HITS/AMMO/SPEED/ROUNDS`), цвета — как
   `GameManager_DrawInfoDisplayRow` (красный ≤ ¼, жёлтый ≤ ½, зелёный иначе).
4. **Сетка переключателей** 3×3 на (4,298)…(168,348): `SURV_*`, `STAT_*`,
   `COLOR_*`, `HITS_*`, `AMMO_*`, `RANG_*`, `VISN_*`, `GRID_*`, `NAMES_UP/DN`
   (55×16/18), нажатое состояние — `*_ON`/`*_DN`, локальные флаги
   `HudState::panels: PanelState`. *Уточнение против плана:* это ресурсы
   `SURV_OFF/ON` и т.п., а не `MNUBTN*U/D` (те — кнопки главного меню 109×40).
5. **Зум** `ZOOMPNL1` + `ZOOMPTR` в (20,275) 130×19 (формула смещения
   `GameManager_UpdateZoomSlider`), кнопки `+`/`−` (2,275) и (150,275) меняют
   `Viewport::set_zoom` на шаг 8 в диапазоне 8…64; клик по ползунку —
   `zoom_from_slider_x`.
6. **Нижняя строка:** `XYPOS` (242,456) 90×21 — `%03d-%03d` под курсором
   (или выбранный юнит/центр камеры), `UNITNAME` (338,456) 222×21 — имя
   выбранного/наведённого юнита. Шрифт — `GNW_TEXT_FONT_5`.
7. **Бонусом:** `PLAY_OF`/`PAUSE_OF`, `FIND_OFF`/`LOCK_OF`/`PREV_OF`/
   `UDONE_OF`/`NEXT_OF`, `HELP_OF`/`REPT_OFF`/`CHAT_OFF`, `MIN2X_*`/`MINFL_*`
   и подписи `Prev/Done/Next/Reports/Chat`; клики по ним пока пишут в журнал
   «экран не портирован».

**Критерий:** живой кадр сессии сравнивается с `build/smoke-window.png` по
расположению и наполнению панелей — `build/frame-newgame-panels.png` (кадр
`--new-game --width 640 --height 480`, первый юнит выбран автоматически),
`build/frame-panels.png` (тот же кадр из примера `panel_frame`, камера на
юните) и `build/frame-panels-left.png` (кроп левой панели 2×).

### Пункт B. Шрифт оригинала — **сделано**

- В C++-порте `Text_SetFont(GNW_TEXT_FONT_1/2/5)` действительно выбирает
  GNW-шрифт, но битмапы в `max-cpp-port/src` не лежат: порт грузит
  **TTF-ресурсы** `FONT_1`/`FONT_2`/`FONT_5` из `PATCHES.RES` (не из
  `MAX.RES`) и растеризует их FreeType (`Text_LoadFontTTF`/`Text_BlitTTF`,
  `text.cpp:194-298`, `:485-544`). TTF — конверсия оригинальных растровых
  `FONT1.FON`/`FONT2.FON`/`FONT5.FON` (файлы рядом с `MAX.RES`).
- Найдены и разобраны **оригинальные `.FON`**: заголовок 28 байт, 255 записей
  `{u32 width; u32 offset}`, битмапы `ceil(width/8) × height`, MSB-first.
  Декодер — `max_render::text::gnw::GnwFont::decode`; сами файлы вкомпилированы
  в `max_render/data/README.md`.
- Инструмент `max-rust/tools/font_vectors` собрал эталон C++/FreeType и
  независимый декодер `.FON`: для ASCII **оба пути дают побайтово одинаковые
  битмапы** строк `M.A.X.`, `Hits 15/15`, `End Turn`, `SURVEY STATUS COLORS`,
  `044-034` (все три шрифта). Метрики: высота 16/9/10, `ascender` 13/7/8,
  `advance` совпадает с шириной глифа (у `FONT_1` 4 символа расходятся на 1 px
  из-за хинтинга TTF).
- API: `GnwFontId::Font1/2/5`, `GnwFont::text_width/line_height`,
  `draw_text_gnw`, `draw_text_gnw_shadowed` (тень/обводка как
  `Text_TextLine`), `draw_text_gnw_centered`. Старый 8×8 `draw_text` оставлен
  как fallback и не удалён.
- Тест `text::gnw::tests::rendering_matches_the_cpp_reference_bitmaps` сверяет
  каждый пиксель с `tools/font_vectors/render.txt`; тест
  `embedded_fonts_match_the_original_files` — с `max-original/FONT*.FON`.

**Критерий:** начертание/метрики совпадают с оригиналом — подтверждено
побайтовой сверкой с C++-путём (FreeType) для ASCII.

### Пункт C. Миникарта — **сделано**

- [x] `minimap`-слой `.WRL` рисуется в `WINDOW_MINIMAP` (15,356) 112×112,
  масштаб/смещение — `World::CalculateMinimapScaling`; палитра — системная
  палитра окна с индексами 64…159 из `.WRL`.
- [x] **FOV-маска** — точный порт `Access_UpdateMinimapFogOfWar`: неисследованные
  клетки (`heat_map.cells[i].complete == 0`) затемняются таблицей
  `DarkeningColorIndexTable` (`max(7, c/4)*31/63` + `Color_MapColor`).
- [x] **Точки юнитов** (`DrawMap_RenderMiniMapUnitList`): цвет команды, 2×2,
  `TNT`-фильтр по `HAS_FIRING_SPRITE | FIRES_MISSILES`, в 2x не-здания — 1
  клетка.
- [x] **`2x`**: область 56×56 вокруг `GridCenterOffset` растягивается на всё окно,
  клик переводится через `GameManager_GetMinimapPosition`; рамка камеры
  масштабируется вместе с областью (как в C++).
- [x] **Рамка камеры** — красный `draw_box` по видимым клеткам (`.9` на правом/
  нижнем крае, как в C++).
- [x] **Рамка/фон миникарты** — из `FRAMEPIC` (`MinimapBgImage`); кнопки
  `MIN2X_OF/ON`, `MINFL_OF/ON` рисуются поверх. `RADAR` разобран как
  20-кадровый спрайт 64×64 здания-радара (не рамка миникарты); тест
  `alien_derelict_sprites_are_loaded` фиксирует это.
- [x] Клик по миникарте центрирует камеру (в 1x и 2x), тесты
  `minimap_click_maps_to_grid_and_accounts_for_2x`.

**Критерий:** `build/frame-cd-game.png` и `build/frame-cd-game-2x.png`:
FOV-перепад, красная рамка камеры, точки юнитов, кнопки `2x`/`TNT`;
`build/frame-cd-game-minimap-4x.png` — кроп 4×.

### Пункт D. Иконки, курсоры и выбор планеты — **сделано**

- **Иконки ресурсов** — не цветные точки и не `I_*` на карте: `Survey_RenderMarker`
  использует `RAWMSK*`/`FUELMK*`/`GOLDMK*` (62×62, hotspot 31,31) при
  `zoom > 16` и `SMRWMK*`/`SMFLMK*`/`SMGDMK*` (14×16) при `zoom ≤ 16`;
  кадр — `min(amount,16)` / `amount-1`. `I_RAW/I_FUEL/I_GOLD` оставлены
  fallback'ом, если маркер отсутствует. Видимость — `entry.is_visible_to(team)`
  или кнопка `SURVEY` (`GameManager_MaxSurvey`). Тест
  `resource_markers_are_drawn_from_max_res`.
- **Обломки alien** — это настоящие юниты `ALNTANK/ALNASGUN/ALNPLANE` команды
  `ALIEN` (derelict LUT), плюс `LRGSLAB`/`BRIDGE` и здания
  `SHIELDGN`/`SUPRTPLT`/`RECCENTR`; спрайты `ALNASGUN`/`ALNPLANE` добавлены
  в `UNIT_SPRITES` (43 типа), тест `alien_derelict_sprites_are_loaded`.
- **Курсоры** — `max_assets::cursor` (декодер `Cursor_Descriptor`: 8-байтный
  заголовок, `frame_count = height/width`, hotspot), загрузка 30 ресурсов
  (`HIDDNPTR`…`PTR_HELP`), `max_app::cursor` (индексы `CursorId`, контекст
  `CursorContext`, `session_cursor`, `cursor_image`). В SDL — `set_cursor_rgba`
  (`Cursor::from_surface`, кэш по ключу) за feature `sdl`; headless — только
  логика контекста. Тесты: размеры/hotspot/кадры всех 30 курсоров
  (`real_max_res_cursors_match_the_cpp_descriptors`), контекст
  (`context_switches_cursor_like_cursor_setcursor`), RGBA/анимация.
- **Экран выбора планеты** — в `CreateForm` вместо числового поля сетка 24
  планет 8×3: картинка `SNOW_PIC + world/6` (128×128, 4 группы), имя планеты под
  ней (`planetselectmenu.cpp:120-138`), красная рамка выбранной, клик мышью
  меняет `world`. Числовой ввод в поле `World` сохранён как fallback (Tab →
  цифры/стрелки). Тесты `planet_click_changes_world_and_keeps_numeric_fallback`,
  `planet_grid_is_eight_by_three_and_inside_the_form`,
  `create_form_paints_planet_pictures_from_max_res`.

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
4. **Координаты мыши (закрыт).** При хроме карта занимает (180,18) 448×448, а
   `Viewport` приложения — 640×480. Теперь ввод сессии при активном хроме
   переводит экранные координаты в локальные координаты карты
   (`panels::screen_to_map`), а клики по панелям обрабатываются до карты —
   включая End Turn, который намеренно заходит на карту. Камера карты
   (448×448) для рендера и ввода — одна и та же (`App::map_viewport`), поэтому
   клик в центр окна карты попадает в ожидаемую клетку (тест
   `chrome_click_on_the_map_centre_selects_the_unit_under_it`), а клик по
   End Turn не уходит в карту (`chrome_end_turn_click_is_a_button_not_a_map_click`).
5. **`hud.rs` (74 КБ) рисует процедурный HUD**; в сессии при активном хроме он
   отключён. Включать его обратно нельзя, пока не перенесём его элементы на
   оригинальные координаты — иначе снова «два HUD».
6. **Палитра `FRAMEPIC` своя** (не `COLOR.PAL`), 8-битные каналы. Не смешивать с
   палитрой спрайтов: спрайты берут `COLOR.PAL` + brightness LUT, а **весь UI**
   (картинки панелей и текст) — системную палитру окна, то есть палитру
   `FRAMEPIC` (`UiPalette` в `panels.rs`). Проверка: индекс `0x02` в `COLOR.PAL`
   — пурпурный, в `FRAMEPIC` — зелёный (цвет полоски Hits), `0xA2` — текст.
7. **`P_*` 300×240** — крупные картинки юнитов/зданий для диалогов и
   информационных панелей (`resourcetable.cpp:223-225`), а не портрет 128×128
   для `CORNER_FLIC`: там в C++ отдельная FLIC-анимация (`PlayerCorner`), формат
   FLC пока не разобран.
8. **Курсоры — не `SimpleImage`-семантика в чистом виде**: это
   `ImageSimpleHeader` + вертикально уложенные кадры, а `frame_count` **не
   хранится**, а выводится как `height / width` (нацело). У `MINI_PTR` (15×16)
   это даёт 1 кадр высотой 16, хотя высота не кратна ширине; строгая проверка
   кратности ломает загрузку 29/30. Лежат в `MAX.RES` (30 ресурсов), палитра —
   системная палитра окна, не `COLOR.PAL`.
9. **`RADAR` — не рамка миникарты.** Это спрайт здания-радара: 20 кадров 64×64,
   hotspot (32,32), 60 449 Б. Рамка миникарты нарисована в `FRAMEPIC`
   (`ResourceManager_MinimapBgImage` — снимок окна `WINDOW_MINIMAP`). Не
   подменять рамку спрайтом `RADAR` и не искать отдельный ресурс рамки.
10. **Палитра миникарты — гибрид.** Индексы 64…159 берутся из `.WRL` (каналы
    `/4`, затем `*4` при выводе), остальные — из `FRAMEPIC`; индексы точек
    юнитов (`1/2/3/4/0xFF`) — из `FRAMEPIC`. Если рисовать миникарту целиком
    палитрой `.WRL`, индексы 6/177…232 (края/неизвестные клетки) и цвета точек
    станут неверными. Тест — `minimap_view_matches_calculate_minimap_scaling` +
    `darkening_table_halves_brightness_like_cpp`.
11. **Кнопки миникарты в `FRAMEPIC` уже нарисованы**, а ресурс `MIN2X_OF`/
    `MINFL_OF` лежит на 4–5 px ниже: если блитить `*_OF` поверх хрома, видны
    два «2x»/«TNT». Отжатое состояние берётся из `FRAMEPIC`, `*_ON` рисуется
    при нажатии; без хрома (headless) рисуется и `*_OF`.
12. **FOV не эвристика по юнитам.** `World::GetMinimapFov` — лишь стартовый
    слой; затемнение делает `Access_UpdateMinimapFogOfWar` по
    `heat_map.cells[i].complete`. В Rust heat map уже есть, поэтому маска — 1:1;
    попытка вывести «известные клетки» из позиций юнитов даст другой рисунок.

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

# 4) кадр 640×480 с панелями и выбранным юнитом (пункт A)
cargo run -p max_app --example panel_frame -- ../build/frame-panels.ppm
powershell -File build/ppm2png.ps1 -Ppm build/frame-panels.ppm -Png build/frame-panels.png

# 4.1) кадры C/D: маркеры/FOV/2x, экран планет, лист курсоров
cargo run -p max_app --example visual_cd -- ../build/frame-cd
# → build/frame-cd-{game,game-2x,planets,cursors}.ppm (PPM → PNG любым конвертером)

# 5) эталон шрифтов: FreeType-путь C++-порта против .FON (пункт B)
docker run --rm -v "D:/dev/max-port-debug:/src" \
    -w /src/max-rust/tools/font_vectors rust:1.94.0-bookworm bash run.sh

# 6) тесты (ровно релизный гейт)
docker run --rm -v "D:/dev/max-port-debug:/src" -w /src/max-rust \
    rust:1.94.0-bookworm bash -c \
    "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test"

# 6.1) SDL-сборка (окно/курсоры) — полный X11/ALSA dev-набор, см. release-checklist §2.3

# 7) живая сессия в окне
docker compose up -d --build                     # Horizon + headless
release/max_app-1.0.0-windows-x86_64-ui/max_app.exe --ui \
    --handshake-patches build/nonexistent.res --turn-wait 45
```

---

## 8. Состояние репозитория

Изменено (не закоммичено): `max-rust/crates/max_app/src/{assets,cursor,lib,main,render,ui}.rs`,
`max_app/src/hud.rs` (`Panels`-состояние), `max_render/src/{lib,layout,map_render,sdl_window,text}.rs`,
`max_assets/src/{image,lib}.rs`, `docs/{visual-1to1-status,release-checklist}.md`.

Новые файлы: `max_assets/src/{cursor,markers}.rs`, `max_app/src/cursor.rs`,
`max_render/src/text/gnw.rs` (+ `max_render/data/gnw_font_*.fnt`),
`max_app/src/panels.rs`, `max_app/examples/{panel_frame,visual_cd}.rs`,
`max-rust/tools/font_vectors/{main.cpp,run.sh,README.md,vectors.txt,render.txt}`.

Ранее закоммичено: `6295a8e` (Windows-кросс-сборка + `build-windows.sh`),
`ba8279a` (нативная Windows UI-сборка + `build-windows-ui.ps1`), плюс правки
`--turn-wait` и шрифта (коммиты делались между итерациями).

---

## 9. Вопросы, которые надо решить перед следующим этапом

1. **Масштаб:** фиксированные 640×480 с апскейлом кадра или масштабирование
   элементов как в C++ (пункт F)? От этого зависит, где хранить координаты
   (константы 640×480 или пересчёт).
2. ~~**Шрифт:** тащить битмапы GNW в репозиторий или рисовать свой?~~ Решено:
   оригинальные `FONT*.FON` вкомпилированы (12 КБ) и сверены с FreeType-путём
   порта; 8×8 остался fallback'ом.
3. ~~**Портрет юнита:** FLIC или статичная картинка?~~ Решено для первого шага:
   статичный кадр спрайта ×2; FLIC-анимация — отдельная задача.
4. ~~**Приоритет:** панели (A) или меню (E)?~~ Панели A, шрифт B, миникарта/
   иконки/курсоры/планеты C/D сделаны; следующий визуальный шаг — меню (E) на
   `MNUBTN*U/D` + `FRAMEPIC` и FLIC-портрет.
5. ~~**Миникарта:** нужна ли FOV-маска и поведение `2x`/`TNT`?~~ Решено: FOV —
   точный порт `Access_UpdateMinimapFogOfWar` по heat map, `2x`/`TNT` — как
   `GameManager_RenderMinimap`/`DrawMap_RenderMiniMapUnitList` (см. §2.5).
