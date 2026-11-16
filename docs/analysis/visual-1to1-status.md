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
| Меню (главное, create/join) | **1:1 (пункт E)** | `MAINPIC`/`PLANETSE`/`MULTGAME`, `BLANK_*`/`SBLNK_*`, `MNUBTN1..6U/D`, `BTNHIDE`, `BGRSCRN*`, `P_*`; подписи — GNW `FONT1/5` | `max_app::menu`, `max_app::ui` |
| Масштабирование окна | **1:1 (пункт F)** | фиксированный кадр 640×480 → целочисленный nearest-neighbour в окно любого размера + letterbox; обратный пересчёт кликов | `max_render::frame_scale`, `max_render::sdl_window` |
| Экран выбора планеты | **8×3 в оформлении `PLANETSE`** на `SNOW_PIC + world/6` + имена; числовой ввод — fallback | `MAX.RES`: `SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC` (128×128) | `max_app::menu::draw_create_form`, `max_app::ui::{planet_grid_rect,draw_planet_picker}` |

Итог: **в игре** (сессия/`--new-game` при 640×480) окно, карта и панели уже
выглядят как оригинал: верхние кнопки/счётчики, портрет (статичный кадр
спрайта), четыре строки статистики, сетка переключателей, зум, координаты,
описание юнита, **миникарта с FOV-маской/точками юнитов/`2x`/`TNT`** и
**настоящие маркеры ресурсов** рисуются на координатах и ресурсах C++-порта,
текст — оригинальными растровыми шрифтами GNW, курсоры и сетка выбора планеты
подключены. **Меню (E)** переведено на `MAINPIC`/`PLANETSE`/`MULTGAME` с
кнопками `BLANK_*`/`MNUBTN*` и GNW-подписями, **масштабирование (F)** —
фиксированный кадр 640×480, целочисленный апскейл в окно любого размера и
обратный пересчёт координат мыши. Осталась анимация портрета (FLIC).

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

**Уточнение (M-SP, 2026-11-15): RLE начинается с байта 776, а не 777.**
В C++ `sizeof(ImageBigHeader) == 777` (`resource_manager.hpp:54-63`), но
`WindowManager_DecodeBigImage` берёт данные с `&image->transparent_color`
(`window_manager.cpp:626`) — то есть первое слово RLE читается **из** последнего
байта структуры и следующего за ним. Проверено на реальных ресурсах
(`FRAMEPIC`, `ENDGAME1..9`): декод со смещением 776 даёт целый кадр, со
смещением 777 — сдвиг на байт, «scanline»-артефакты и обрыв потока.
`max_assets::image::BIG_HEADER` исправлен на 776 (тест
`chrome_draws_frame_and_reports_ready` и новый `sp-*`-набор кадров зелёные);
это исправило и хром игрового окна, и фоны экрана победы `ENDGAME*`.

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

### 2.6 Меню: ресурсы, координаты и поведение (эталон — C++-порт)

**Главное меню** (`menu.cpp::main_menu`):

| Элемент | Ресурс | Координаты/размер | Примечание |
|---|---|---|---|
| фон | `MAINPIC` | 640×480, своя палитра | логотип M.A.X. уже нарисован; `palette_from_image=true` задаёт системную палитру |
| заголовок | GNW `FONT5`, `COLOR_GREEN` | (236,145) 172×15 | `draw_menu_title(window, _(35f9))` = «Main Menu» |
| портрет | `P_*` (`P_TRANSP + Randomizer(67)`) | (16,182) 300×240 | в статичном кадре — `P_TANK`; индексы читаются **палитрой окна**, не своей |
| рамка портрета | `BGRSCRNL`/`BGRSCRNR` | (6,173)/(166,173), 160×259 | сначала рамки, потом портрет поверх; `has_transparency=false` |
| кнопки | `BLANK_UP/DN` 200×29 | x=385, y=175+35·i (6 шт.) | ещё `SBLNK_UP/DN` 100×30 для `Exit` в (435,420) |
| подписи | GNW `FONT1` | по центру кнопки | `Fonts_GoldColor (165,175,199)`, нажатие — `(5,56,199)` |
| копирайт | GNW `FONT5`, чёрный | (10,469) 620×10 | `draw_copyright_label` |

Порядок/подписи из `language.json` (en-US): `New Game`, `Load Game`,
`Multiplayer Game`, `Setup`, `Introduction`, `Credits`, `Exit`. В Rust-клиенте
действия есть у `New Game` → `CreateForm`, `Multiplayer Game` → `JoinList` и
`Exit` → выход; остальные четыре пишут строку статуса «screen is not ported
yet».

**Форма создания игры** (`planetselectmenu.cpp` + `gamesetupmenu.cpp`):

- фон `PLANETSE` 640×480 (палитра совпадает с `MAINPIC` побайтово);
- заголовок `Planet Select Menu` (230,6) 180×20, имя выбранной планеты
  (16,61) 149×19 — `FONT5`, зелёный;
- большая картинка планеты `SNOW_PIC + world/6` в (26,98) 128×128
  (`ulx/uly` из заголовка ресурса);
- сетка 24 планет 8×3 вписана в область 3×2 слотов `PLANETSE`
  (192,51) 388×249, ячейка 48×83; подписи — `FONT5`, выбранная — красная
  рамка 2 px;
- поля формы (`Name`, `World`, `Timer`, `End Turn`, `Opponents`, `Password`)
  — 3×2 в описании `PLANETSE` (41,350) 559×59;
- нижняя строка кнопок `MNUBTN4U` (243,438) 109×40 «Random»,
  `MNUBTN4U` (354,438) 109×40 «Cancel», `MNUBTN5U` (465,438) 48×40 «?»,
  `MNUBTN6U` (514,438) 109×40 «Done» — координаты `planetselectmenu.hpp`.

**Список игр** (`networkmenu.cpp::DrawJoinScreen`):

- фон `MULTGAME` 640×480 (палитра = `MAINPIC`);
- строки хостов — окно `MENU_ITEM_JOIN_TEXT_WINDOW` (359,28) 209×160,
  по 20 px, текст `FONT5` `COLOR_GREEN`, разделители `draw_line(...,
  COLOR_GREEN)`; выделенная строка помечена `>` и подчёркиванием;
- рамка списка — чёрная панель (355,24) 217×168 с обводкой (в оригинале это
  тёмное окно `MULTGAME`); `BTNHIDE` (38×21) в (528,4) — кнопка «скрыть
  список»/назад (в C++-порте ресурс не используется: `grep BTNHIDE` пуст);
- кнопки `MNUBTN3U` (243,438) 109×40 «Options» → обновить,
  `MNUBTN4U` (354,438) 109×40 «Cancel» → назад, `MNUBTN5U` (465,438)
  48×40 «?» → помощь, `MNUBTN6U` (514,438) 109×40 «Ready» → войти
  (`networkmenu.hpp`, y=438).

**Палитра и шрифт.** Все simple image меню (`BLANK_*`, `MNUBTN*`,
`BGRSCRN*`, `BTNHIDE`) индексируются системной палитрой окна, то есть
палитрой `MAINPIC`; big image `PLANETSE`/`MULTGAME` имеют ту же палитру.
У `P_TANK` своя палитра отличается (индекс 19 — маджента), поэтому портрет
рисуется индексами через `MAINPIC` (как `palette_from_image=false` в C++).
Заголовки — `GNW_TEXT_FONT_5`, подписи кнопок — `GNW_TEXT_FONT_1`.

### 2.7 Масштабирование окна (пункт F)

Выбран **вариант 1**: приложение всегда рисует кадр 640×480 (все координаты
UI — константы C++-порта), а в окно он выводится целочисленным
nearest-neighbour:

```text
scale    = min(window_w / 640, window_h / 480)   // 1..8
content  = 640·scale × 480·scale
offset   = ((window_w - 640·scale) / 2, (window_h - 480·scale) / 2)
frame_x  = (window_x - offset_x) / scale
frame_y  = (window_y - offset_y) / scale
```

При `scale = N` каждый пиксель кадра — ровно блок `N×N` (никакой
интерполяции), вокруг — чёрные поля; клик по полю не попадает в кадр. Окно
SDL3 создаётся ресайзимым; `--scale N` задаёт множитель, без флага масштаб
подбирается под рабочий стол (целый, 1..4). В отличие от C++
(`WindowManager_GetScale` + по-элементное `WindowManager_ScaleWindow`/
`RescaleSimpleImage`), это даёт строгий критерий «пиксель-в-пиксель» без
пересчёта координат элементов.

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
| `max_app/src/menu.rs` | 1400+ | **пункт E:** `MenuAssets` (`MAINPIC`/`PLANETSE`/`MULTGAME`, `BLANK_*`/`SBLNK_*`, `MNUBTN1..6U/D`, `BTNHIDE`, `BGRSCRN*`, `P_TANK`), отрисовка главного меню/формы/списка в координатах `menu.cpp`/`planetselectmenu.hpp`/`networkmenu.hpp`, hit-test кнопок/строк/полей; 7 тестов |
| `max_render/src/frame_scale.rs` | 390 | **пункт F:** `FrameScale` — целочисленный `fit` 640×480 в окно, `upscale` (блоки `N×N` + поля), `window_to_frame`; 6 тестов |
| `max_render/src/sdl_window.rs` | +110 | ресайзимое окно, `new_scaled` (авто-масштаб под рабочий стол), `present` nearest-neighbour в `content_rect`, `window_to_frame`, `poll_input` в координатах кадра |
| `max_app/src/main.rs` | +25 | `--scale N`, окно = кадр × масштаб, логи размера/масштаба |
| `max_app/src/assets.rs` | +12 | `Assets.menu` (`MenuAssets::load`), предупреждение, если меню не собралось |
| `max_assets/src/image.rs` | +50 | ресинхронизация RLE при литерале за границей записи (иначе `SNOW_PIC` декодировался на 20 строк); тест `snow_pic_decodes_all_rows` |
| `max_app/examples/visual_ef.rs` | 190 | кадры E/F: главное меню, форма, список игр, апскейл 2× |
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

**Тесты:** `576 passed, 0 failed` (`cargo test` в `rust:1.94.0-bookworm`,
лог `build/cargo-docker-ef-final3.log`); `cargo check --all-targets` — 0
ошибок; `cargo check -p max_app --features sdl-from-source` — 0 ошибок
(Linux, полный набор X11/ALSA dev-пакетов).
Из них новые (пункт E/F): 7 тестов `max_app::menu` (`main_menu_layout`,
`menu_assets_load`, отрисовка `MAINPIC`/`PLANETSE`/`MULTGAME` + `MNUBTN`,
hit-test прямоугольников, сетка 8×3), 3 теста `max_app::ui`
(`native_frame_menus_are_drawn_from_max_res`,
`main_menu_clicks_use_the_cpp_button_rectangles`,
`window_click_at_2x_lands_in_the_same_map_cell`), 6 тестов
`max_render::frame_scale` (2×2-блоки, letterbox, обратный клик 1280×960,
кроп при окне меньше кадра), 1 тест `max_assets::image`
(`snow_pic_decodes_all_rows`), плюс `max_app::assets`/`planet`-тесты.

**Артефакт (Windows UI):** `release/max_app-1.0.0-windows-x86_64-ui/`
- `max-rust.exe` — 5 423 616 Б,
  sha256 `279e472dcc41e1747548a7e38d9c1b98738e861db913b9f1d1228ac858ff23c9`
- `zip` — 3 065 885 Б,
  sha256 `d7415687a5d70381d30bf7ccd1bb2f67dc8afcabea993775fe22aca22b91f0d0`
- `SHA256SUMS` по 5 файлам — `OK`; `max-rust.exe --help` работает,
  `--ui` открывается (окно 640×480 при `--scale 1`, 1280×960 при
  `--scale 2`), клик `New Game` в окне 2× открывает форму создания
  (`build/frame-ef-main-windows*.png`,
  `build/frame-ef-create-windows*.png`).

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
| `build/frame-windows-artifact.png` | тот же кадр, отрисованный собранным `max-rust.exe` (Windows UI-артефакт); пиксель-в-пиксель с Linux-кадром |
| `build/frame-cd-game.png` | **пункт C/D:** 640×480: маркеры ресурсов `RAWMSK*`/`GOLDMK*` (не точки), миникарта с FOV-маской, рамкой камеры, точками юнитов, кнопки `2x`/`TNT` |
| `build/frame-cd-game-2x.png` | тот же кадр в режиме `2x` (миникарта 56×56 → 112×112, кнопка нажата) |
| `build/frame-cd-game-minimap-4x.png` | кроп миникарты 4× (виден перепад FOV и красная рамка камеры) |
| `build/frame-cd-planets.png` | **пункт D:** форма создания игры — сетка 24 планет на `SNOW_PIC + world/6`, имя под каждой, красная рамка выбранной, поле `World` как fallback |
| `build/frame-cd-cursors.png` | все 30 курсоров из `MAX.RES` (контактный лист: hotspot/кадры/палитра) |
| `build/frame-cd-windows.png` | тот же игровой кадр, отрисованный собранным `max-rust.exe` (Windows UI) |
| `build/frame-ef-main.png` | **пункт E:** главное меню 640×480 на `MAINPIC`: 6 кнопок `BLANK_*` + `Exit` `SBLNK_*`, портрет `P_TANK` в рамках `BGRSCRN*`, `FONT1`-подписи, наведение на `New Game` |
| `build/frame-ef-create.png` | **пункт E:** форма создания на `PLANETSE`: большая картинка планеты (26,98), сетка 8×3, поля 3×2, кнопки `MNUBTN4/5/6` в y=438 |
| `build/frame-ef-join.png` | **пункт E:** список игр на `MULTGAME`: рамка (359,28) 209×160, строки по 20 px, `BTNHIDE`, кнопки `MNUBTN3/4/5/6` |
| `build/frame-ef-main-2x.png` | **пункт F:** тот же кадр главного меню после `FrameScale` 2× — 1280×960, каждый пиксель блоком 2×2 |
| `build/frame-ef-main-windows.png`, `build/frame-ef-create-windows.png` | те же экраны из живого `max-rust.exe --ui --scale 1` (клиент 640×480) |
| `build/frame-ef-main-windows-2x.png`, `build/frame-ef-create-windows-2x.png` | живое окно `--ui --scale 2` (1280×960); клик в окне 2× открывает форму — обратный пересчёт координат работает |
| `build/ui-chrome-session-final.png` | живая сессия `--ui`: хром + карта + панели |
| `build/rle-FRAMEPIC.bmp` | независимая (Python) эмуляция RLE — сверка с Rust-декодером |

**Измерения раскладки** (по пикселям кадра `frame-chrome2.png`): карта занимает
x 180…627, y 18…465 (448×448), левее — панель (`FRAMEPIC`), правее/ниже — рамка
окна; всё совпадает с `windows[]` C++-порта.

---

## 5. Пункты плана: статус (A–F сделаны)

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

### Пункт E. Меню на оригинальных ресурсах — **сделано**

Реализация — `max_app::menu` (`MenuAssets::load`, `draw_main_menu`,
`draw_create_form`, `draw_join_list`, hit-test `main_menu_at`/
`create_button_at`/`create_field_at`/`join_button_at`/`join_row_at`),
экраны в `max_app::ui` переключаются на неё при кадре 640×480; без ассетов
остаётся процедурный fallback (headless-тесты и малые кадры).

1. **Главное меню** — `MAINPIC`, заголовок `Main Menu` (236,145) `FONT5`
   зелёным, портрет `P_TANK` 300×240 в (16,182) в рамках `BGRSCRNL/R`
   (6,173)/(166,173), шесть кнопок `BLANK_UP/DN` 200×29 на x=385 с шагом 35
   (y=175…350), `Exit` `SBLNK_UP/DN` 100×30 в (435,420), копирайт
   (10,469) 620×10 `FONT5` чёрным. Подписи — `GNW_TEXT_FONT_1`,
   `Fonts_GoldColor`; нажатие (а не наведение) — `*_DN` и
   `Fonts_DarkOrageColor`; выбранные radio/toggle остаются `*_DN`.
   Действия Rust-клиента: `New Game`, `Multiplayer Game`, `Exit`; четыре
   остальные кнопки оригинала пишут «screen is not ported yet».
2. **Форма создания игры** — `PLANETSE`, заголовок и имя планеты `FONT5`
   зелёным, большая картинка `SNOW_PIC + world/6` в (26,98) 128×128, сетка
   8×3 в области (192,51) 388×249 (ячейка 48×83, подписи `FONT5`, выбранная
   — красная рамка 2 px), поля 3×2 в описании (41,350) 559×59, кнопки
   `MNUBTN4U`/`MNUBTN4U`/`MNUBTN5U`/`MNUBTN6U` в (243/354/465/514, 438).
   Клик по кнопке Random выбирает детерминированную «случайную» планету;
   числовой ввод `World` остаётся fallback'ом.
3. **Список игр** — `MULTGAME`, чёрная панель (355,24) 217×168 с обводкой,
   строки (359,28) 209×160 по 20 px (`FONT5`, `COLOR_GREEN`, разделители
   линиями), выбранная строка — `>` + подчёркивание, `BTNHIDE` в (528,4) =
   назад, кнопки `MNUBTN3U` (243,438) «Options» → refresh,
   `MNUBTN4U` (354,438) «Cancel» → back, `MNUBTN5U` (465,438) «?»,
   `MNUBTN6U` (514,438) «Ready» → join.
4. **Мышь/клавиатура** сохранены: Up/Down/Tab/Enter, `c`/`j`/`q`, Esc —
   как раньше; клики в 640×480 идут по оригинальным прямоугольникам,
   в остальных кадрах — по процедурной раскладке (fallback).

*Отличия от формулировки плана (сверено с оракулом):* фон главного меню —
`MAINPIC`, а не `FRAMEPIC`; кнопки главного меню — `BLANK_*`/`SBLNK_*`, а не
`MNUBTN*`; `MNUBTN*` используются в нижних строках формы/списка ровно как в
`planetselectmenu.hpp`/`networkmenu.hpp`; `BTNHIDE` в C++-порте не
используется (`grep` пуст) и здесь применён как кнопка «скрыть список».

**Критерий:** `build/frame-ef-main.png`, `build/frame-ef-create.png`,
`build/frame-ef-join.png`; те же экраны из живого Windows-артефакта —
`build/frame-ef-main-windows.png`, `build/frame-ef-create-windows.png`.

### Пункт F. Масштабирование окна — **сделано (вариант 1)**

Выбран **вариант 1**: фиксированный кадр 640×480 + целочисленный
nearest-neighbour апскейл всего кадра; по-элементное масштабирование C++
(`WindowManager_GetScale`/`ScaleWindows`/`RescaleSimpleImage`) не нужно,
потому что все координаты UI остаются константами 640×480, а критерий
«пиксель-в-пиксель» достигается строго: `scale = min(w/640, h/480)`, каждый
пиксель → блок `N×N`, поля чёрные.

- `max_render::frame_scale::FrameScale` — `fit`/`native`, `upscale` (блоки
  `N×N` + letterbox), `frame_to_window`, `window_to_frame` (обратный
  коэффициент, `None` на полях);
- `SdlWindow` — ресайзимое окно, `new_scaled` (авто-масштаб 1..4 под рабочий
  стол), `--scale N`, `present` выводит текстуру nearest в
  `content_rect`, `poll_input` отдаёт координаты **кадра**, а не окна;
- при `--scale 2` окно 1280×960, клик в нём пересчитывается в кадр 640×480
  и попадает в ту же клетку карты; клик по чёрному полю игнорируется;
- окно меньше кадра не уменьшает картинку, а обрезает её по центру
  (сохраняя 1:1).

**Тесты:** `double_upscale_replicates_every_pixel_as_a_two_by_two_block`
(640×480 → 1280×960, каждый пиксель 2×2, без промежуточных цветов),
`clicks_in_a_1280x960_window_map_to_the_same_map_cell` (окно 2× → та же
клетка (56,55)), `odd_window_letterboxes_and_centres_the_frame`,
`window_smaller_than_frame_crops_instead_of_shrinking`,
`window_click_at_2x_lands_in_the_same_map_cell` (`max_app::ui`).

**Критерий:** `build/frame-ef-main-2x.png` (1280×960) и живые окна
`build/frame-ef-main-windows-2x.png` / `create-windows-2x.png`; клик
`New Game` в окне 2× открывает форму.

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
13. **Меню — не `FRAMEPIC`.** В C++ фон главного меню — `MAINPIC`
    (`palette_from_image=true` задаёт системную палитру), формы — `PLANETSE`,
    списка игр — `MULTGAME`; `FRAMEPIC` — хром партии. В `MenuAssets` `FRAMEPIC`
    оставлен только fallback'ом, если какого-то фона нет. Не подменять
    `MAINPIC` на `FRAMEPIC`: другой рисунок и палитра simple image.
14. **Кнопки главного меню — `BLANK_*`/`SBLNK_*`, а не `MNUBTN*`.** `MNUBTN1..6`
    (109×40, `MNUBTN5` 48×40) — нижние строки `planetselectmenu`/
    `networkmenu`/`gamesetupmenu` (y=438). `BTNHIDE` в C++-порте не
    используется вовсе (`grep BTNHIDE` пуст), это ресурс оригинала; здесь он
    стал кнопкой «скрыть список».
15. **Палитра big image меню.** `PLANETSE`/`MULTGAME`/`SNOW_PIC` совпадают с
    `MAINPIC` побайтово, но `P_TANK` — нет (индекс 19 в собственной палитре
    маджента, в `MAINPIC` — голубой). C++ рисует `P_*` с
    `palette_from_image=false`, то есть системной палитрой; поэтому
    `draw_big_image_at` использует `menu.palette`, а не `image.color`.
16. **RLE `SNOW_PIC`.** В его потоке есть литерал 29744 за границей записи;
    старый декодер обрывал на нём **все** строки, и планета оставалась почти
    чёрной (это было видно в старой версии `build/frame-cd-planets.png`).
    Теперь на таком чанке обрывается только текущая строка и следующая
    начинается со следующего слова (ресинхронизация); для остальных big image
    результат побайтово тот же, тест `snow_pic_decodes_all_rows`. Не
    «оптимизировать» обратно.
17. **GNW-шрифт — Latin-1.** Длинное тире `—`, `…`, `’` рисуются как `?`;
    `menu::clean_status` заменяет их ASCII-эквивалентами для строк статуса
    меню. Новые строки статуса писать ASCII или через `clean_status`.
18. **Масштаб — только целый.** `FrameScale` не интерполирует: при окне не
    кратном 640×480 остаются чёрные поля; клики по полям не попадают в кадр.
    Окно меньше кадра обрезается по центру, а не уменьшается. Не менять
    `scale` на дробный без пересмотра критерия «пиксель-в-пиксель».
19. **`--scale`/авто-масштаб.** Без флага окно = 640×480×N, где N — максимум
    целых, влезающих в рабочую область основного монитора (1..4); `--scale 2`
    форсирует 1280×960. Клики SDL-слой пересчитывает в кадр
    (`SdlWindow::window_to_frame`), поэтому в `Screen`/HUD координаты всегда
    640×480.

---

## 7. Как воспроизвести проверки

```sh
# 1) окно 1:1 в кадре (без SDL): FRAMEPIC + карта
release/max_app-1.0.0-windows-x86_64-ui/max-rust.exe --new-game --world 0 --seed 42 \
    --width 640 --height 480 --out build/frame-chrome2.ppm
powershell -File build/ppm2png.ps1 -Ppm build/frame-chrome2.ppm -Png build/frame-chrome2.png

# 2) дамп UI-картинок из MAX.RES (сверка с оригиналом)
cargo run -p max_assets --example ui_images -- ../max-original/MAX.RES build/ui

# 3) вся карта и фрагмент 1:1 настоящими тайлами
cargo run -p max_render --example terrain -- ../max-original/SNOW_1.WRL build/wrl-snow [cx cy]

# 4) кадр 640×480 с панелями и выбранным юнитом (пункт A)
cargo run -p max_app --no-default-features --example panel_frame -- ../build/frame-panels.ppm
powershell -File build/ppm2png.ps1 -Ppm build/frame-panels.ppm -Png build/frame-panels.png

# 4.1) кадры C/D: маркеры/FOV/2x, экран планет, лист курсоров
cargo run -p max_app --no-default-features --example visual_cd -- ../build/frame-cd
# → build/frame-cd-{game,game-2x,planets,cursors}.ppm (PPM → PNG любым конвертером)

# 4.2) кадры E/F: главное меню, форма на PLANETSE, список на MULTGAME, 2×
cargo run -p max_app --no-default-features --example visual_ef -- ../build/frame-ef
# → build/frame-ef-{main,create,join,main-2x}.ppm
powershell -File build/ppm2png.ps1 -Ppm build/frame-ef-main.ppm -Png build/frame-ef-main.png
powershell -File build/ppm2png.ps1 -Ppm build/frame-ef-main-2x.ppm -Png build/frame-ef-main-2x.png

# 4.3) живые кадры Windows-артефакта: --ui --scale 1 и 2, клик New Game
powershell -File build/capture-ef.ps1
# → build/frame-ef-{main,create}-windows{,-2x}.png

# 5) эталон шрифтов: FreeType-путь C++-порта против .FON (пункт B)
docker run --rm -v "D:/dev/max-port-debug:/src" \
    -w /src/max-rust/tools/font_vectors rust:1.94.0-bookworm bash run.sh

# 6) тесты (ровно релизный гейт)
docker run --rm -v "D:/dev/max-port-debug:/src" -w /src/max-rust \
    rust:1.94.0-bookworm bash -c \
    "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test"

# 6.1) SDL-сборка (окно/курсоры) — полный X11/ALSA dev-набор, см. release-checklist §2.3

# 7) живая сессия в окне (--scale 2 — окно 1280×960; без флага — авто)
docker compose up -d --build                     # Horizon + headless
release/max_app-1.0.0-windows-x86_64-ui/max-rust.exe --ui --scale 2 \
    --handshake-patches build/nonexistent.res --turn-wait 45
```

---

## 8. Состояние репозитория

Изменено (не закоммичено): `max-rust/crates/max_app/src/{assets,cursor,lib,main,render,ui}.rs`,
`max_app/src/{hud,menu}.rs`, `max_render/src/{lib,frame_scale,layout,map_render,sdl_window,text}.rs`,
`max_assets/src/{image,lib}.rs`, `max-rust/README.md`,
`docs/{visual-1to1-status,release-checklist}.md`.

Новые файлы (эта итерация): `max_render/src/frame_scale.rs`,
`max_app/src/menu.rs`, `max_app/examples/visual_ef.rs`,
`build/capture-ef.ps1` (тестовый скрипт, `build/**` в `.gitignore`).
Ранее добавленные: `max_assets/src/{cursor,markers}.rs`, `max_app/src/cursor.rs`,
`max_render/src/text/gnw.rs` (+ `max_render/data/gnw_font_*.fnt`),
`max_app/src/panels.rs`, `max_app/examples/{panel_frame,visual_cd}.rs`,
`max-rust/tools/font_vectors/{main.cpp,run.sh,README.md,vectors.txt,render.txt}`.

Ранее закоммичено: `6295a8e` (Windows-кросс-сборка + `build-windows.sh`),
`ba8279a` (нативная Windows UI-сборка + `build-windows-ui.ps1`), плюс правки
`--turn-wait` и шрифта (коммиты делались между итерациями).

---

## 9. Вопросы, которые надо решить перед следующим этапом

1. ~~**Масштаб:** фиксированные 640×480 с апскейлом кадра или масштабирование
   элементов как в C++?~~ Решено: **вариант 1** — фиксированный кадр 640×480,
   целочисленный nearest-neighbour в окно любого размера и обратный пересчёт
   кликов (`max_render::frame_scale`); координаты UI остаются константами.
2. ~~**Шрифт:** тащить битмапы GNW в репозиторий или рисовать свой?~~ Решено:
   оригинальные `FONT*.FON` вкомпилированы (12 КБ) и сверены с FreeType-путём
   порта; 8×8 остался fallback'ом.
3. ~~**Портрет юнита:** FLIC или статичная картинка?~~ Решено для первого шага:
   статичный кадр спрайта ×2; FLIC-анимация — отдельная задача.
4. ~~**Приоритет:** панели (A) или меню (E)?~~ Панели A, шрифт B, миникарта/
   иконки/курсоры/планеты C/D, меню E и масштаб F сделаны; остался FLIC-портрет.
5. ~~**Миникарта:** нужна ли FOV-маска и поведение `2x`/`TNT`?~~ Решено: FOV —
   точный порт `Access_UpdateMinimapFogOfWar` по heat map, `2x`/`TNT` — как
   `GameManager_RenderMinimap`/`DrawMap_RenderMiniMapUnitList` (см. §2.5).

## 10. Пункт G. M-SP: настройка, локальная партия, экран победы — **сделано** (2026-11-15)

Одиночная игра против ИИ (1 человек + 1–3 компьютера), визуал по C++-ресурсам.

| Экран | Ресурсы | Раскладка (C++) | Код |
|---|---|---|---|
| Options | `OPTNFRM` 640×480 | `game_config_menu_items[71]` + `game_config_menu_controls[51]`: 6 панелей 200×190 в (6/216/426, 38/233), подписи 15 px, `draw_box(0xFF)` вокруг выбранного | `max_app/src/setup.rs::draw_options` |
| Planet | `PLANETSE`, `SNOW_PIC+world/6`, `MNULAROU/MNURAROU`, `MNUBTN4/5/6` | слоты 112×112 в (192/330/469, 51/189), имя (16,61,150,20), описание (41,350,561,60), Random/Cancel/?/Done y=438 | `draw_planet` |
| Players | `CHOSPLYR`, `CH_HUM/CMP/NON_U/D`, `CH_TM1..4_U/D`, `CLN0..8LOGO` | 4×3 кнопки 55×71 (175/285/394, 67+92·i), флаги 93×89 (495, 61+92·i), эмблема выбранного клана (41,40), заголовки колонок (61/162/272/380/497, 31, 79/80×20) | `draw_players` |
| Clan | `CLANSEL`, `CH_CN1..8_U/D`, `CLN1..8LOGO`, `MNUBTN3..6U/D` | 8 карточек 91×86 (46/201/355/510, 44/194), имена (32/188/344/497, 140/290), Random/Cancel/?/Done y=438 | `draw_clan` |

**Локальная партия**: цикл `GameManager_TeamTurnTurnBased` (`manage_economy` →
`Ai_BeginTurn` → тики до `end_turn_retry_counter > 120` → `06` → следующая
команда → `finish_turn`), таймер хода, пауза, End Turn. Кадр — штатный
`render_frame` с хромом `FRAMEPIC` (карта 180,18 448×448, панели, миникарта).

**Экран победы/поражения**: `menu_draw_game_over_screen` 1:1 — фон
`ENDGAME1..9`, `Game Over` (100,10), 4 колонки (80+110·i), строки 116…326,
`1st place`…`Eliminated`, `ENDARM` (273,458), `ENDOK` (293,458) + `OK`.

**Свидетельства**

* `cargo run -p max_app --no-default-features --example sp_frames -- ../max-original ../build` →
  `build/sp-options.png`, `build/sp-planets.png`, `build/sp-players.png`,
  `build/sp-game.png`, `build/sp-victory.png`, `build/sp-defeat.png`;
* `cargo test -p max_app --test local_sp` — 12 тестов: настройка из UI
  (team_types/планета/таймер), 5 ходов против ИИ (чередование, задачи и
  приказы ИИ), таймер/пауза, победа при элиминации ИИ + отрисовка экрана,
  round-trip контейнера `MXLS` после хода, загрузка V70 в локальную партию;
* найдено и исправлено по пути: **декодер big image читал RLE с 777 вместо
  776** (см. §2.2) — исправило хром `FRAMEPIC` и фоны `ENDGAME*`.

**Ограничения**: Simultaneous Moves идёт последовательными ходами команд;
кланы (`ClanSelectMenu`) и Help-экраны не портированы; `GameState → V70`
(запись) отсутствует — локальный сейв это контейнер `MXLS`.

## 11. Пункт H. Фазы старта M-SP и выравнивание логотипов/`?` — **сделано** (2026-11-15)

### 11.1. Цепочка экранов из C++

`menu_custom_game_menu` (`menu.cpp:2115`) после `ChoosePlayerMenu` вызывает
`GameManager_GameLoop(GAME_STATE_6_GAME_SETUP)` → `GameManager_GameSetup` →
`GameManager_InitGame` (`game_manager.cpp:3056`). Там для каждой **человеческой**
команды в порядке убывания слота (`for (i = PLAYER_TEAM_MAX - 1; i >=
PLAYER_TEAM_RED; --i)`, то есть GRAY→BLUE→GREEN→RED) идёт
`GameManager_PlayerMissionSetup(team)`:

| # | Экран C++ | Что делает | Условие |
|---|---|---|---|
| 1 | `CargoMenu` (`cargomenu.cpp`, фон `CARGOPIC`) — `GameManager_CargoSelection` | мобилизация: покупка стартовых юнитов за `team_gold = start_gold + Clans::GetCredits(clan)` | `team_gold != 0` (иначе сразу `GetMissionLoadout`) |
| 2 | `GAME_STATE_7_SITE_SELECT` — `GameManager_SelectSite` | выбор точки высадки: карта 112×112 на зуме 4 (4 px/клетку), клик/`Enter`, `Esc` — отмена | всегда |
| 3 | `GameManager_HandleProximityOverlaps` | предупреждения о близких высадках | люди |

После людей компьютерные команды выбирают точки сами
(`Ai_SelectStartingPosition`: 5×5 суши, ≥ `AI_SAFE_ENEMY_DISTANCE` от уже
выбравших) и состав (`AiPlayer::RollTeamMissionSupplies`). Только затем
`GameManager_PopulateMapWithResources` генерирует ресурсы (поэтому на экране
высадки ресурсов ещё нет), а `GameManager_ProcessTeamMissionSupplyUnits`
высаживает армии.

### 11.2. Что портировано

| Экран | Ресурсы/раскладка | Код |
|---|---|---|
| Мобилизация | `CARGOPIC` 640×480; списки Available (482,80,123,290, строки 32 px ×9) / Purchased (337,45,118,185, ×5) с иконками `I_*` (1:2 мобильные, 1:4 здания); портрет `P_*` 300×240 в (11,13) 1:1 (окно 280×240) с описанием `GetDescription()` (16,17 280×230, шрифт 5 + обводка) и таблицей `UnitStats_DrawStats` (11,293,250,174, строки 19 px, pips `I_*`, красные линии); цены списка — колонка 610..625; колонка апгрейдов (283,293) и цен (321,293) с `I_GOLD`; полоса золота (361,276,40,140, `VERTGOLD`); cargo-слайдер (421,301,20,115 `VERTRAW/FUEL/GOLD`) и счётчик (421,276,20,10); `Cost`/`Credit`/`Cargo` (320/358/409,283,48×16); `Buy` (590,386,41,22), `Delete` (412,240,53,23), `Cancel` (357,452,62,23), `Done` (447,452,54,23), `Help` (420,452,26,23), `CRGTOG` (542,446,81,32), `BLDDES` (292,264,17,17), стрелки 471/491,387 и 327/348,240 | `max_app/src/mobilize.rs`, `max_app/src/cargo_unit_data.rs` |
| Выбор точки | область карты `MAIN_MAP` (180,18,448,448) = 112 × 4 px; курсор 4×4; рамка «Select starting location.»; `Cancel`/`Done` нижней строки | `max_app/src/landing.rs` |
| Очередь фаз | люди в порядке C++ (убывание слота), мобилизация → высадка на каждого; `start_gold = 0` пропускает мобилизацию | `max_app/src/pregame.rs` |

Формулы (байт-совместимы с C++):
* цена юнита — `Cargo_GetRawConsumptionRate(LANDPLT, 1)` (для `LANDPLT` = 3,
  `cargo.cpp:168`) × `ATTRIB_TURNS` **текущих** значений команды
  (`Clans`-модификаторы учтены, `unit_values::team_base_values`);
* цена строки списка — ставка `CONSTRCT` (2) для `STATIONARY`, `TRAINHAL` (1)
  для `COMMANDO`/`INFANTRY`, иначе `LANDPLT` (3) × `turns`
  (`purchasetypeselector.cpp:40`);
* cargo — 1 кредит за 5 единиц, ёмкость `storage/5` (у `GOLDTRCK` — 0),
  `xfer_take_max` бесплатной загрузки = её cargo/5 (`CargoMenu::Select`);
* цена апгрейда — `UpgradeControl_CalculateCost` (`upgradecontrol.cpp:214`,
  целочисленный `pow(x,7.5)`; сверено с C++-векторами `tools/upgrade_vectors`
  и `tools/cargo_vectors`), шаг 1/2/5/10, лимит показа 800;
* валидность точки — `GameManager_IsValidStartingPosition`: 4×4 клетки
  `x-1…x+2`, `y-1…y+2` — ровно `LAND`;
* высадка — `deploy_starting_units_with` (позиция/кредиты/загрузка/апгрейды на
  команду) с тем же порядком ГПСЧ (две плиты на команду).

### 11.3. Визуальные исправления (задача A)

* **Эмблемы кланов**: `Button::Copy(CLNnLOGO, 41, 40)` кладёт картинку в точку
  **якоря**, а `WindowManager_DecodeSimpleImage` вычитает её собственные
  `ulx/uly` (`window_manager.cpp:784`). Добавлен
  `menu::blit_simple_anchored`; применён на экране `Players` (флаги 93×89) и
  `Clans` (карточки 91×86). До фикса логотип рисовался от `(41,40)` и вылезал
  вправо-вниз за кнопку.
* **Знак `?`**: сверен с оракульным кадром C++-порта (снят самим портом по
  Alt+C, 640×480: `Players`) — подпись `?` в кнопке `MNUBTN5` (48×40)
  совпадает с `Button::SetCaption`/`Text_TextLine`: `x = ulx + (48 −
  (Text_GetWidth+1))/2 = 484`, `y = uly + (40 − 16)/2 = 450`, тень/обводка —
  как `Text_TextLine`. Наш кадр совпал с оракулом в этой области
  пиксель-в-пиксель; формула закреплена юнит-тестом
  `menu::tests::quest_caption_is_centered_like_cpp`.

### 11.4. Упрощения (честно)

* Мобилизация — полный `CargoMenu` (P7): списки, иконки, cargo-слайдер, полоса
  золота, апгрейды, режим модернизации, портрет `P_*` 1:1, описания юнитов
  (`GetDescription()`, 93 записи в `data/cargo_unit_data.json`) и таблица
  `UnitStats_DrawStats` с сеткой иконок. Отличия от оригинала:
  иконки турелей без второго кадра (`angle_offsets[2]` из `.SPW` не разобран);
  портрет обрезан окном 280×240 (C++-порт пишет 300 px без клипа — 20 правых
  пикселей уходят на фон); у 24 служебных типов (обломки, взрывы, снаряды,
  `HARVSTER`, `WALDO`) описания нет и в оригинале, но они не достижимы в
  `CargoMenu`; числа `Cost`/`Credit`/`Cargo` в C++-порте не
  рисуются (остались подписи), счётчики — на полосе золота и над слайдером;
  `Help` пишет строку статуса (экран `CARGO_SETUP` не портирован); звуки не
  проигрываются. Клановые бонусы применяются автоматически
  (`unit_values::team_base_values`), кредиты кланов `SC_CLANS` у всех = 0.
* Состав ИИ — стандартная загрузка `SC_L0001` (как человек, ничего не купивший);
  `AiPlayer::RollTeamMissionSupplies` не портирован.
* Экран высадки рисует **поверхностную** карту цветами `TileStyle`, а не
  изометрические тайлы `.WRL`; зоны близости (`CheckLandingZones`) и
  `HandleProximityOverlaps` не портированы.
* `Cancel` фаз возвращает на экран `Players` (в C++ отмена прерывает старт и
  уводит в главное меню).

### 11.5. Свидетельства

* `qa_run cargo-full.qa` → кадры `cargo-buy.png` (покупка: иконки, полосы,
  цены), `cargo-upgrade.png` (режим модернизации: 62 типа, фильтры, стрелки и
  цены апгрейдов), `cargo-upgraded.png` (после armor +1 и загрузки cargo);
  `assert cargo-mode/cargo-types/cargo-credits`; эталоны —
  `max-rust-qa/golden/cargo/*.png`.
* `qa_run sp-full.qa` → кадры `clan.png` (логотипы 8 кланов по центру
  карточек), `mobilize.png`, `mobilize-bought.png`, `landing.png`,
  `landing-cursor.png`; база в `game-start.png` стоит у выбранной клетки
  (30,22), а не в стартовой четверти; `hs-2.qa` — фазы для GREEN и RED по
  очереди; `victory.qa`/`visual-screens.qa` обновлены (`golden/mobilize.png`
  переснят с иконками и полосами).
* `cargo test -p max_app --test start_phases` — точка применяется (база у
  выбранной клетки), мобилизация меняет состав/кредиты/апгрейды (высаженный
  конструктор получает `armor + 1`), порядок фаз hot-seat, ИИ-точки, старый
  путь без фаз не изменился.
* `cargo test -p max_app --test cargo_vectors` — бит-в-бит с C++-векторами
  (`tools/cargo_vectors`): 15 цен покупки, 93 цены списка, 7 наборов списка
  модернизации, строки и стоимости апгрейдов всех 93 типов; 0 расхождений.
* `cargo test -p max_app --test cargo_render` — отрисовка иконок (оба списка),
  полосы золота/cargo, стрелок, `CRGTOG`, фильтров и прокрутки по
  изменившимся пикселям кадра.

