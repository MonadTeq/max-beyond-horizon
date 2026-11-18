# Визуал M.A.X. Rust-клиента: статус 1:1 с оригиналом

**Дата среза:** 2026-11-19 (§27) · **Версия workspace:** 1.1.0 (разделы §1–§11 —
срез 2026-11-15/1.0.0, §12 — полное покрытие спрайтов) · **Задача:** довести визуал
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
| Здания (спрайты) | **1:1** (34 типа: все постройки и обломки) | `MAX.RES`: `HANGAR`, `DEPOT`, `POWERSTN`, … + тени `S_*` | `max_app::assets::UNIT_SPRITES` (все 93 типа `SC_UNITS`) |
| Анимация зданий (маяки, коннекторы) | **1:1** по DOS-съёмке: палитровые циклы (13–16/21–24 @6 fps и др.) + коннекторы по маске; корпус статичен, кадры дрели 1..15 выключены до съёмки добычи (§26) | оракул `World::InitColorCycles`/`UnitInfo::RenderWithConnectors` | `max_render::palette::ColorCycles`, `max_app::render::{connector_*, MINING_DRILL_FRAMES_ENABLED}` |
| Юниты (спрайты, тени, командные цвета) | **1:1** (92 типа спрайтами; маркер — только `MASTER`) | `MAX.RES`: `TANK`, `INFANTRY`, … + `S_*` + `D_*` (башня, `angle_offsets`) | `max_render::sprite_atlas`, `max_assets::sprite`, `max_app::render::draw_unit_sprites` |
| Окно и рамки интерфейса | **1:1** | `MAX.RES`: `FRAMEPIC` (640×480, своя палитра, RLE) | `max_app::chrome`, `max_render::layout` |
| Раскладка элементов (карта/миникарта/кнопки) | **1:1** по координатам C++-порта | `window_manager.cpp` `windows[]` | `max_render::layout` |
| Содержимое панелей (портрет, статистика, кнопки, таймеры) | **1:1** (портрет — FLC-анимация 15 FPS; 4 строки статистики — по типу юнита, §17) | `FILES_OF/ON`, `PREF_OFF/ON`, `ENDTRN_U`, `TURNS`, `TIMER`, `XYPOS`, `UNITNAME`, `SURV_*`, `STAT_*`, `COLOR_*`, `HITS_*`, `AMMO_*`, `RANG_*`, `VISN_*`, `GRID_*`, `NAMES_*`, `ZOOMPNL1`, `ZOOMPTR`, `FIND_OFF`, `PREV_OF`, `UDONE_OF`, `NEXT_OF`, `HELP_OF`, `REPT_OFF`, `CHAT_OFF`, `MIN2X_OF`, `MINFL_OF` | `max_app::panels` |
| Миникарта | **1:1** (FOV-маска, рамка камеры, точки юнитов, `2x`/`TNT`; рамка — из `FRAMEPIC`) | `.WRL::minimap()`, палитра окна + `.WRL` 64…159, `RAWMSK*`/… | `max_app::panels::draw_minimap` |
| Меню команд юнита (попап) | **1:1** (кнопки `UNTBTN_U/D`, раскладка/подписи/горячие клавиши; часть пунктов отключена — механика не портирована, §18) | `MAX.RES`: `UNTBTN_U`, `UNTBTN_D`; подписи — `language.json` | `max_app::unitmenu` |
| Шрифт | **1:1 (растровые GNW)** | `FONT1.FON`, `FONT2.FON`, `FONT5.FON` (копии — `max_render/data/`), сверены с FreeType-путём порта | `max_render::text::gnw` |
| Иконки ресурсов/обломков | **1:1** (`RAWMSK*`/`FUELMK*`/`GOLDMK*` + малые при `zoom ≤ 16`; `I_*` — fallback) | `MAX.RES`: 99 маркеров; обломки — спрайты `ALNTANK/ALNASGUN/ALNPLANE` + плита/мост/здания | `max_assets::markers`, `max_render::map_render::render_resource_icons_into` |
| Курсор мыши | **1:1 по формату и контексту** (SDL `Cursor::from_surface` за feature `sdl`) | `MAX.RES`: 30 курсоров (`HANDPTR`, `ARROW_*`, `PTR_PATH`…) | `max_assets::cursor`, `max_app::cursor` |
| Меню (главное, create/join) | **1:1 (пункт E)** | `MAINPIC`/`PLANETSE`/`MULTGAME`, `BLANK_*`/`SBLNK_*`, `MNUBTN1..6U/D`, `BTNHIDE`, `BGRSCRN*`, `P_*`; подписи — GNW `FONT1/5` | `max_app::menu`, `max_app::ui` |
| Диалог Preferences/Setup | **1:1 (пункт P8)** — панель `PREFSPIC` 400×422 в (120,29), заголовок, кнопки `PRFDNE/PRFCAN/PRFHLP`, слайдеры `PRFSLIT/PRFSLIDE`, чекбоксы `UNCHKED/CHECKED`, поля `PREFEDIT/PREFNAME`; открытие из `Setup` меню и кнопки `Preferences` панели; `Done/Cancel/Help`, модальность | `optionsmenu.cpp/.hpp` (`window_uly`-раскладка, 36 опций), `settings.cpp` defaults, `menu.cpp:2325/2331`, `saveload.cpp:547/920` (`IniPreferences`) | `max_app::preferences`, `max_app::ui`, `max_app::render::draw_preferences` |
| Масштабирование окна | **1:1 (пункт F)** | фиксированный кадр 640×480 → целочисленный nearest-neighbour в окно любого размера + letterbox; обратный пересчёт кликов | `max_render::frame_scale`, `max_render::sdl_window` |
| Экран выбора планеты | **8×3 в оформлении `PLANETSE`** на `SNOW_PIC + world/6` + имена; числовой ввод — fallback | `MAX.RES`: `SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC` (128×128) | `max_app::menu::draw_create_form`, `max_app::ui::{planet_grid_rect,draw_planet_picker}` |

Итог: **в игре** (сессия/`--new-game` при 640×480) окно, карта и панели уже
выглядят как оригинал: верхние кнопки/счётчики, портрет (FLC-анимация
юнита, 15 FPS, кадр 0 в headless), четыре строки статистики, сетка переключателей, зум, координаты,
описание юнита, **миникарта с FOV-маской/точками юнитов/`2x`/`TNT`** и
**настоящие маркеры ресурсов** рисуются на координатах и ресурсах C++-порта,
текст — оригинальными растровыми шрифтами GNW, курсоры и сетка выбора планеты
подключены. **Меню (E)** переведено на `MAINPIC`/`PLANETSE`/`MULTGAME` с
кнопками `BLANK_*`/`MNUBTN*` и GNW-подписями, **масштабирование (F)** —
фиксированный кадр 640×480, целочисленный апскейл в окно любого размера и
обратный пересчёт координат мыши, анимация портрета (FLC), а также экраны
`Introduction` (MVE-видео `maxint.mve`) и `Credits` (прокрутка титров) — см. §11 и §14.

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
- **Полное покрытие типов (2026-11-16).** `max_app::assets::UNIT_SPRITES` —
  построчный порт `SC_UNITS` (`assets/PATCHES.RES`, `Units::LoadResource`):
  93 записи «тип → `sprite`/`shadow`» в порядке `UnitType`. Загружаются 92
  корпуса из `MAX.RES` (все, кроме `MASTER`) и 75 теней (`SC_UNITS` объявляет
  77, но `S_MASTER`/`S_SUBMAR` в архивах отсутствуют); `D_*`
  (`Unit::GetFrameInfo()`) даёт базы/число кадров башни и `angle_offsets`
  для 11 типов с `TURRET_SPRITE`/`SPINNING_TURRET`. Общая отрисовка —
  `max_app::render::draw_unit_sprites`: тень → корпус → башня со сдвигом
  `angle_offsets[angle]`, масштаб `zoom/64`; её же использует HUD-портрет,
  поэтому панели автоматически показывают все типы.
- **Дыры самих данных игры.** `MASTER` (`Master Builder`) объявлен в
  `SC_UNITS`, но ресурса `MASTER`/`S_MASTER` нет ни в `MAX.RES`, ни в
  `PATCHES.RES` (в каталоге игры лежит осиротевший `MASTR17.SPW`, ссылки на
  него в архивах нет): C++ `UnitInfo::RenderSprite` проверяет
  `sprite_data != null` и не рисует ничего, Rust-клиент оставляет маркер
  команды — единственный такой тип. У `SUBMARNE` так же отсутствует
  объявленная `S_SUBMAR` — подлодка рисуется без тени, как в оригинале.
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
2. **Портрет** `CORNER_FLIC` (10,29) 128×128: чёрный фон и FLC-анимация
   выделенного юнита (`Unit::GetFlicsAnimation` → тег `F_*` → `*.FLC` рядом с
   `MAX.RES`), кадр 1:1 без масштабирования, индексы — палитрой окна
   (`FRAMEPIC`), как C++ (`load_flic_palette = 0`); смена кадра раз в 1/15 с
   (`GameManager_AdvanceFlic`), зацикливание (`animate = 2`), в headless/QA
   `dt = 0` — кадр 0. Снятие выделения — `MenuDeleteFlic` + чёрное окно; у 22
   типов без `F_*` (обломки, взрывы, ракеты, `HARVSTER`, `WALDO`) и у `MASTER`
   (нет `F_MASTER`) — fallback: спрайт ×2. Сверху — имя и статус
   (`GNW_TEXT_FONT_2`), как `MenuUnitSelect`/`UpdateInfoDisplay`.
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
  `SHIELDGN`/`SUPRTPLT`/`RECCENTR`; спрайты всех типов теперь в
  `UNIT_SPRITES` (93 типа), тест `alien_derelict_sprites_are_loaded`.
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
   для `CORNER_FLIC`: там в C++ отдельная FLIC-анимация (`flicsmgr`), формат
   разобран в `max_assets::flic` (см. §11).
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
3. ~~**Портрет юнита:** FLIC или статичная картинка?~~ Решено: **FLIC**
   (`PlayerCorner`/`flicsmgr`); реализовано — см. §11. Спрайт ×2 остался
   fallback'ом для типов без `F_*`.
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
`GameState → V70` (запись) отсутствует — локальный сейв это контейнер `MXLS`.
Кланы (`ClanSelectMenu`) не портированы; Help-экраны и `ResearchMenu`
портированы в P8 (§13).

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
| Выбор точки | область карты `MAIN_MAP` (180,18,448,448) = 112 × 4 px; **настоящие тайлы `.WRL`** (`Gfx_DecodeMapTile`, выборка 0/21/42/63); окно `"Select starting location."` (одна строка, затемнение карты по палитре, `Font5`); выделение точки — **курсор** `UNIT_GO`/`UNIT_NO_GO`/`HAND` (`GameManager_GetWindowCursor`), а не нарисованная рамка. `MASTER`-маркер/круг обзора/`BLDMRK*` в обычном выборе не рисуются: C++ спавнит `MASTER` только в ветке `GameManager_CheckLandingZones` | `max_app/src/landing.rs`, `max_render/src/terrain.rs` |
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
  `UnitStats_DrawStats` с сеткой иконок. Иконки турельных типов
  (`TURRET_SPRITE`/`SPINNING_TURRET`) рисуются двумя кадрами `I_*` со сдвигом
  `angle_offsets[2]` из `D_*` (`Unit::GetFrameInfo()`), как
  `ReportStats_DrawListItemIcon`; та же башня отдельным кадром
  (`SpriteRenderer::draw_turret_scaled`) на карте, в портрете HUD и меню
  апгрейдов. `BuildMenu` использует тот же `draw_icon`/`team_lut` для строк
  списка, а его портрет — big image `P_*` 300×240 в (11,13) без обрезки
  (§24). Отличия от оригинала:
  в `CargoMenu` портрет обрезан окном 280×240 (C++-порт `CargoMenu` пишет
  300 px без клипа — 20 правых пикселей уходят на фон; `BuildMenu` рисует все
  300 px, как его `local_window`); у 24 служебных типов (обломки, взрывы,
  снаряды, `HARVSTER`, `WALDO`) описания нет и в оригинале, но они не
  достижимы в `CargoMenu`; числа `Cost`/`Credit`/`Cargo` в C++-порте не
  рисуются (остались подписи), счётчики — на полосе золота и над слайдером;
  `Help` открывает `CARGO_SETUP` (P8, §13.2); звуки не
  проигрываются. Клановые бонусы применяются автоматически
  (`unit_values::team_base_values`), кредиты кланов `SC_CLANS` у всех = 0.
* Состав ИИ — стандартная загрузка `SC_L0001` (как человек, ничего не купивший);
  `AiPlayer::RollTeamMissionSupplies` не портирован.
* Экран высадки — **1:1** по карте, сообщению и курсору (настоящие тайлы
  `.WRL`, `MessageManager_DrawMessage`, `GameManager_GetWindowCursor`).
  Остались отличия: ветка проверки близости (`CheckLandingZones`/
  `HandleProximityOverlaps`) не портирована — вместе с ней нет спавна `MASTER`,
  его маркера `DrawUnitSelector`, круга обзора `DrawCircle` и `BLDMRK*`
  (в обычном выборе C++ их тоже нет, см. кадр C++-порта
  `build/tmp/oracle-MAX0002.png`); `Cancel`/`Done` убраны — клик по валидной
  клетке подтверждает выбор, `Enter`/`Esc` сохранены.
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
* `cargo test -p max_app --test landing_wrl` — кадр высадки содержит настоящие
  тайлы: >40 цветов в области карты, пиксели клеток сверены с сырым
  `SNOW_1.WRL` (формула `Gfx_DecodeMapTile`), курсор `UNIT_GO` рисуется в
  кадре; fallback без `.WRL` не падает.
* `cargo test -p max_core --test fog_of_war` — юнит виден только своей команде;
  скан открывает врага в FOV и не открывает вне; `AllVisible`; стелс вне FOV.
* Windows UI (`release/max_app-1.1.0-windows-x86_64-ui`, MSVC + vcpkg SDL3):
  реальный кадр `build/landing-ui.png` — настоящие тайлы `.WRL`, окно
  `"Select starting location."`, `CURSOR_UNIT_GO`; сборка пересобрана.

### 11.6. Туман войны на карте и миникарте — исправлено

До фикса `GameState::spawn_unit` повторял `UnitInfo::InitStealthStatus`
(`unitinfo.cpp:2796`) неточно: все не-стелс юниты получали
`visible_to_team = true` **для всех команд**. Поэтому на карте
(`DrawMap_RenderUnits`) и миникарте (`DrawMap_RenderMiniMapUnitList`) были
видны чужие базы и юниты без разведки/радара.

Теперь:

* `combat::init_stealth_status(unit, all_visible)` — точный порт: мины,
  диверсанты и подлодки невидимы всем, остальные — только при
  `GameManager_AllVisible`, своя команда видит всегда;
* `GameState::update_visibility_status(reset)` — порт
  `Access_UpdateVisibilityStatus` (`access.cpp:880`): `InitStealthStatus`,
  очистка тепловых карт и повторное покрытие сканированием всех юнитов;
  вызывается после высадки армий (`game_manager.cpp:1965`);
* `App::draw_units` дополнительно скрывает `ORDER_IDLE`
  (`UnitInfoGroup::IsRelevant`), чужие юниты — только при `is_visible_to_team`;
* миникарта использует те же флаги (`panels::paint_minimap_units`).

Проверки: `max_core/tests/fog_of_war.rs` (4 теста), тесты рендера карты и
миникарты; QA `sp-full`/`hs-2` PASS, в `game-start.png` и
`hs-green-turn.png` на миникарте остаются только свои точки; golden
`game.png`, `upgrade-done.png`, `research-done.png` пересняты (0 дрейфа).

---

## 12. Полное покрытие спрайтов юнитов — **сделано** (2026-11-16)

`UNIT_SPRITES` расширен с 43 до **93** записей — построчный порт `SC_UNITS`
(`assets/PATCHES.RES`, `Units::LoadResource`): поля `sprite`
(`Unit::GetSprite()`) и `shadow` (`Unit::GetShadow()`) в порядке `UnitType`.
Спрайтами на карте рисуются все здания, техника, пехота, флот, авиация,
пришельцы, мины, взрывы и снаряды; маркер команды остался только у `MASTER`.

| Что | Значение |
|---|---|
| Записей в таблице | **93** (все `UnitType` 0x00–0x5C, `UNIT_END`) |
| Корпусов загружено | **92** (`MASTER` — нет ресурса) |
| Теней загружено | **75** (`SC_UNITS` объявляет 77; у 16 типов `INVALID_ID`, `S_MASTER` и `S_SUBMAR` отсутствуют в архивах) |
| Башня `D_*` | **11** типов с `TURRET_SPRITE`/`SPINNING_TURRET`: `GUNTURRT`, `ANTIAIR`, `ARTYTRRT`, `ANTIMSSL`, `TANK`, `SP_FLAK`, `FASTBOAT`, `BATTLSHP`, `ALNTANK`, `SCANNER`, `AWAC` |
| Маркер | **1**: `MASTER` (`UNIT_TYPES_WITHOUT_SPRITE_DATA`) |

Добавлены 50 типов, которых не было в старой таблице:

* стационарные/обломки: `LRGRUBLE`, `SMLRUBLE`, `LRGTAPE`, `SMLTAPE`,
  `SMLSLAB`, `LRGCONES`, `SMLCONES`, `BLOCK`, `MININGST`, `LANDMINE`,
  `SEAMINE`, `LNDEXPLD`, `AIREXPLD`, `SEAEXPLD`, `BLDEXPLD`, `HITEXPLD`,
  `ARTYTRRT`, `ANTIMSSL`, `MASTER`;
* наземные: `ARTILLRY`, `ROCKTLCH`, `MISSLLCH`, `SP_FLAK`, `MINELAYR`,
  `SCANNER`, `REPAIR`, `FUELTRCK`, `CLNTRANS`, `COMMANDO`;
* флот: `FASTBOAT`, `CORVETTE`, `BATTLSHP`, `SUBMARNE`, `SEATRANS`,
  `MSSLBOAT`, `SEAMNLYR`, `CARGOSHP`;
* авиация/пришельцы: `BOMBER`, `AIRTRANS`, `AWAC`, `JUGGRNT`;
* снаряды/следы: `ROCKET`, `TORPEDO`, `ALNMISSL`, `ALNTBALL`, `ALNABALL`,
  `RKTSMOKE`, `TRPBUBLE`;
* `HARVSTER`, `WALDO`.

**Что осталось маркером и почему.** Только `MASTER` (`Master Builder`): в
`SC_UNITS` у него `sprite = "MASTER"`, `shadow = "S_MASTER"`, но ни одного из
этих ресурсов нет ни в `MAX.RES`, ни в `PATCHES.RES` (в каталоге игры лежит
осиротевший `MASTR17.SPW`, ссылки из архивов на него нет). C++
`UnitInfo::RenderSprite`/`RenderShadow` проверяют `sprite_data`/`shadow_data`
на `null` и для такого типа не рисуют ничего; Rust-клиент по своим правилам
показывает маркер команды. У `SUBMARNE` отсутствует объявленная `S_SUBMAR` —
подлодка рисуется без тени, как в оригинале (оба случая вынесены в
`UNIT_TYPES_WITHOUT_SPRITE_DATA`/`UNIT_TYPES_WITHOUT_SHADOW_DATA`).

**Код и тесты.**

* `max_app/src/assets.rs` — таблица 93 записей + `UNIT_TYPES_WITHOUT_*`;
  `assets::tests::unit_sprite_table_mirrors_sc_units` сверяет таблицу с
  `cargo_unit_data` (JSON генерируется из `SC_UNITS`),
  `every_unit_type_loads_its_body_and_shadow` — загрузка 92 корпусов и 75
  теней (77 объявлено, `S_MASTER`/`S_SUBMAR` отсутствуют),
  `turret_frames_cover_every_flagged_type` — 11 типов с башней и
  `angle_offsets`.
* `max_app/src/render.rs` — `draw_unit_sprites` (тень → корпус → башня,
  `zoom/64`); карта и HUD-портрет (`panels.rs`) используют один код.
* `max_app/tests/unit_sprites.rs` — 3 теста: непустой кадр у каждого типа
  (на zoom 32/64/128), башня ровно у флаговых типов, кадр карты 640×640 с
  юнитами всех 93 типов (`units_drawn=92`, `units_marker=1`), портрет HUD для
  всех типов.

**QA.** `qa_run` — все 10 сценариев PASS. Golden переснят после осознанного
визуального изменения: `golden/game.png`, `golden/upgrade/upgrade-done.png`,
`golden/research/research-done.png` (в кадре вместо маркеров появились
`MININGST`/`POWGEN` и другие типы); повторная сверка — **0 дрейфа**, остальные
кадры не изменились (`join-list`/`landing`/`main-menu`/`mobilize`/
`multiplayer`/`options`/`planet`/`players`/`winloss`, `production/*`,
`cargo/*` — 0 пикселей). `soak-100.qa` и `soak_headless --turns 120` — PASS.

**Полный гейт и артефакты.** `bash ci.sh` (Docker, `rust:1.94.0-bookworm`) —
**CI OK за 577 с**: `cargo check/test` default + `sound` + `--no-default-features`,
`max-horizon-server` (108 тестов), все 10 QA-сценариев PASS, golden без дрейфа,
soak PASS (RSS +172 КиБ). Пересобраны артефакты
`release/max_app-1.1.0-windows-x86_64/` (headless, mingw),
`release/max_app-1.1.0-windows-x86_64-ui/` (MSVC + SDL3.dll) и
`release/max_app-1.1.0-linux-x86_64/` (SDL3 из исходников).

---

## 13. Камера после высадки центрируется на шахте — **сделано** (2026-11-16)

До этого среза `App::focus_player` центрировал карту на **первом юните
канонического обхода** — `GROUND_COVER` (`SMLSLAB`/`LRGSLAB`), клетка на одну
вниз-влево от шахты. Визуально это давало сдвиг всей карты на 64 px
(центр `MAIN_MAP` приходился на плиту, а не на базу), а в игре — «юнит не
двигается»: клики, рассчитанные от базы, попадали на соседнюю клетку, где
стоит стационарная шахта; RMB отвечал «Unit cannot move (air/stationary)»,
предпросмотра не было.

Теперь `focus_player` — построчный порт `GameManager_GetFirstRelevantUnit`
(`game_manager.cpp:3140`): `Access_GetFirstMiningStation` (шахта своей команды
из `StationaryUnits`), затем первый не-`ORDER_IDLE` юнит в
`MobileLandSea` → `MobileAir` → `Stationary`. Центрирование —
`GameManager_CenterMainMapView`: центр клетки (`grid*64+32`) в центр области
карты, т.е. `MAIN_MAP` (180,18,448×448) → (404,242).

| Что | Значение |
|---|---|
| Кадр старта партии | шахта (22,22) в (404,242), `Surveyor` (23,21) в (468,178) |
| Юниты на старте | 7 спрайтов RED: `MININGST`, `POWGEN`, `SMLSLAB`, `LRGSLAB`, `SURVEYOR`, `ENGINEER`, `CONSTRCT` (`units_drawn=7`, `units_skipped=0`) |
| Предпросмотр пути | зелёная линия `PREVIEW_COLOR` (120,255,152) в буфере карты и при хроме (66 px на 1 шаг) |

**Свидетельства.** `max_app/tests/move_input.rs` (6 тестов), новый сценарий
`max-rust-qa/scripts/move-input.qa` (полный путь кликами, `assert selected`/
`unit-pos`/`preview`/`log=Move:`/`log=Cancelled`), эталоны
`golden/move-input/*` (5 кадров) и переснятые `golden/game.png` +
`golden/research/research-done.png`; `qa_golden_check` — 0 дрейфа.

---

## 11. Пункт H. FLIC-портрет, `Introduction` и `Credits` — **сделано** (2026-11-17)

Закрыты последние визуальные пробелы: анимация портрета юнита и экраны
`Introduction`/`Credits` главного меню.

### 11.1 FLC-декодер (`max_assets::flic`)

Формат — Autodesk FLIC (публичная спека): 128-байтный заголовок
(`type = 0xAF12`, `frames`, `width/height`, `speed`, `oframe1/oframe2`), кадры по
16-байтному заголовку (`0xF1FA`, число чанков), чанки `COLOR_256` (4),
`DELTA_FLC`/`SS2` (7, дельта по словам), `COLOR_64` (11), `BLACK` (13),
`BYTE_RUN` (15), `LITERAL` (16); `PSTAMP` (18) пропускается, `DELTA_FLI` (12)
реализован для полноты (в ресурсах не встречается). Оракул —
`max-cpp-port/src/flicsmgr.cpp`.

* Ресурсы: 71 запись `F_*` в `MAX.RES` — не изображения, а ASCIIZ-имена файлов
  (`fxagun.flc`, `tank03.flc`, …); сами `.FLC` лежат рядом с архивом. Декодер
  читает файлы по индексу каталога без учёта регистра.
* Кадры дельта-кодированы → `Flic::decode_frame(n)` раскладывает `0..=n`,
  `FlicPlayer` листает инкрементально (как `flicsmgr_advance_animation`,
  включая возврат на кадр 1 после последнего при `animate = 2`).
* Палитровые чанки копятся, но в портрете не применяются (C++
  `load_flic_palette = 0` → индексы идут через палитру окна `FRAMEPIC`).
  Байты палитры FLIC в этих файлах 8-битные (C++ сдвигает их на 2).
* Ошибки — значениями (`FlicError`), паник на битых данных нет.

**Проверка.** `max_assets/tests/flic_assets.rs`: все 71 `flc-ref` записи
`MAX.RES` (имена/смещения сверены с `max-assets/images/manifest.csv`), каждый
файл разобран и декодирован целиком — 1115 кадров, из них 36 анимированных
(30 кадров), 35 статичных; размеры 128×128 (кроме `SAVELOAD.FLC` 128×256),
`speed` 67/71, число кадров 1/30. `max_assets/tests/flic_vectors.rs` сверяет
**каждый кадр каждого файла** с C++ (`tools/flic_vectors`, FNV-1a-32) и 6
полных кадров бит-в-бит.

### 11.2 Портрет (`max_app::portrait`, `panels::draw_portrait`)

* Маппинг тип → FLC — из `SC_UNITS` (`flics_animation`, 71 тип) через
  `cargo_unit_data::unit_flic`; `Assets::load` грузит 70 FLC (у `MASTER` нет
  `F_MASTER`, как и спрайта — известный пробел данных).
* Анимация: `floor(elapsed · 15) % frames` (15 FPS как
  `TIMER_FPS_TO_MS(15)`), зацикливание; `dt = 0` в headless/QA — кадр 0
  (`golden` детерминированы), GUI идёт с `dt = 1/60`.
* Смена выделения пересоздаёт анимацию с кадра 0; снятие выделения — чёрное
  окно; fallback — старый спрайт ×2.
* Кадр рисуется 1:1 (128×128) палитрой `FRAMEPIC`; имя/статус — поверх, как
  `MenuUnitSelect` + `UpdateInfoDisplay`.

### 11.3 `Credits` (`max_app::credits`)

Построчный порт `menu_credits_menu_loop`: фон `CREDITS` 640×480 со своей
палитрой, область 450×230 в (95,125), буфер прокрутки 230+18 строк, строка
появляется каждые 20 шагов в строке 231 и уезжает вверх на 1 пиксель за шаг,
шаг — `1000/speed` мс (по умолчанию 10 мс, 100 шагов/с), скорость меняется на
±9 (клавиши `-`/`+`, в оригинале `KP-`/`KP+`), Esc/любая клавиша — выход,
автозакрытие когда `height_offset <= 125`. Таблица — 171 строка из `menu.cpp`
(65 заголовков `0xFF` + 106 имён `0x04`, 58 пустых разделителей), тексты
заголовков — `language.json` en-US; генератор `tools/gen_credits_data.py` →
`crates/max_app/data/credits.json`. Состояние — чистая функция от числа шагов,
поэтому QA-кадр детерминирован.

### 11.4 `Introduction` (`max_app::intro`) — MVE-видео

В C++ это MVE-видео: `Movie_PlayIntro` → `movie_run(INTROFLC, track, INTROSUB)`,
а `INTROFLC` — ASCIIZ-ссылка на `maxint.mve`; играет `mvelib32.c` (Interplay
MVE, свои видеокодек и звуковые дорожки). Декодер MVE портирован в
`max_assets::mve` (см. §14): экран декодирует кадр 0 при входе, крутит 15 FPS
по `dt`, масштабирует 432×320 в 640×474 с letterbox (как `movie_cb_show_frame`
+ `cscale`) и возвращается в меню по концу видео или Esc/клику (`mve_cb_ctl`).
Если `maxint.mve` рядом с `MAX.RES` нет, остаётся прежняя заглушка с фоном
`MLOGO`/`MAINPIC` и текстом — так юнит-тесты без данных не падают.

### 11.5 QA и артефакты

* `visual-screens.qa`: `menu intro` → `shot intro` (dt = 0, первый кадр MVE) →
  `tick 210` → `shot intro-logo` (логотип Interplay, ~кадр 52), `menu credits`
  → `tick 100` → `shot credits`, клик по своему Surveyor'у → `shot portrait`
  (14 кадров, эталоны `golden/intro.png`, `golden/intro-logo.png`,
  `golden/credits.png`, `golden/portrait.png`).
* Перебазированы только кадры с выделенным юнитом: `move-input/game-selected`,
  `move-input/move-preview`, `move-input/game-after`,
  `upgrade/upgrade-done` (портрет вместо спрайта); остальные 0 дрейфа.
* `cargo test -p max_assets` (7 UNIT `flic`, 3 `flic_assets`, 2 `flic_vectors`),
  `cargo test -p max_app` (`portrait` 6, `credits` 7).

---

## 13. ResearchMenu и Help-экраны — **сделано** (2026-11-17)

Закрыты два последних визуальных пробела клиента: экран исследований
(`RSRCHPIC`) и окно помощи `HELPFRAM` для секций `CARGO_SETUP`,
`PLANET_SETUP`, `NEW_GAME_SETUP`, `OPTIONS_SETUP` (а также `RESEARCH_SETUP`,
`UPGRADES_SETUP`, `GAME_SCREEN_SETUP`).

### 13.1 Экран исследований (`max_app::researchmenu`)

Оракул — `researchmenu.cpp`. Окно `RSRCHPIC` 360×331 отцентровано в
`WINDOW_MAIN_MAP`: `ulx = 180 + (447−360)/2 = 223`, `uly = 18 + (447−331)/2 = 76`
(`Window(RSRCHPIC, GameManager_GetDialogWindowCenterMode())`).

| Элемент | Координаты (окно) | Как в C++ |
|---|---|---|
| Заголовок `Research Topics` | (95,19) 170×13, центр, `COLOR_GREEN` | `Text_TextBox(buffer, width, …, 95,19,170,13, COLOR_GREEN, true)` (vertical_align → +2) |
| Подписи `Labs`/`Topics`/`Turns` | (23,52)/(184,52)/(282,52), ширины 125/90/60 | `Text_TextLine` (FONT1 в C++, здесь FONT5 — см. отличия) |
| 8 тем | `uly = i·28 + 70` | `ResearchMenu_TopicLabels`/`TopicIcons`: Attack, Shots, Range, Armor, Hits, Speed, Scan, Cost |
| Стрелки | `UPGLFT_U/D` (72,uly), `UPGRGT_U/D` (143,uly) 18×17 | disabled — `UPGLFT_X`/`UPGRGT_X`; правая только при `free_capacity > 0`, левая при `centers > 0` |
| Счётчик центров | окно (28,uly+3) 32×10, число справа | `ReportStats_DrawNumber(topic_value, 32, …)` |
| Ходы до завершения | окно (293,uly+3) 40×10 | `(centers + turns_to_complete − 1) / centers`; `turns` считается временным `UpdateResearchProgress(+1)` **на копии** темы, состояние не трогается |
| Слайдер | слот (91,uly) 48×17, `PRFSLIDE` 14×17 | `offset = 34·centers/active`; клик: `((x−91)·active + 24)/48` |
| Иконка/подпись/`+N%` | (170,uly+7) центр; (185,uly) 49×15; справа у x=278 | `+N%` = `research_level·10`; `Text_Blit` цветом `0xA2` |
| `Cancel`/`Help`/`Done` | (92,294)/(167,294)/(194,294) | `DPTBAYUP/DN` 75×25, `DPTHP_UP/DN` 31×26 |

**Распределение.** В C++ `free_capacity` стартует с 0: все включённые центры
уже закреплены за темами. Уменьшение темы освобождает центры
(`free_capacity += old − new`), увеличение — тратит. `Done`
(`ResearchMenu_OnClick_Done`) собирает желаемое число центров по темам и для
каждого включённого `RESEARCH`-центра: оставляет его, если в цели для его темы
есть место; иначе переводит на первую тему с местом (`allocation −1/+1`), а
если мест нет — `ORDER_POWER_OFF`. Порт — `GameState::set_research_allocation`
(работает и для локальной, и для сетевой партии). Выключение центра списывает
allocation не в момент `Done`, а на следующем тике
(`process_order_power_off` → `ResearchMenu_UpdateResearchProgress(-1)`) — как
в C++. `ResearchMenu_NewTurn` уже вызывается из `GameState::finish_turn`
(`advance_research`), поэтому экран и тик используют одни и те же
`research_topics`.

**Открытие.** В C++ путь — popup исследовательского центра
(`UnitsManager_Popup_OnClick_Research`, при `ORDER_POWER_ON`). Popup-меню юнита
в Rust-клиенте нет, поэтому экран открывается клавишей **`R`** для выделенного
включённого центра своей команды (как `U` для апгрейдов). `Help` экрана
открывает `RESEARCH_SETUP` поверх `RSRCHPIC`.

### 13.2 Help-экраны (`max_app::helpmenu`)

Оракул — `helpmenu.cpp`, данные — `assets/help.json` (`Help::LoadFile`) и
`language.json` (`_(ceb6)` для кнопки `Keys`).

* Окно `HELPFRAM` 301×231: в меню настройки — `(169,124)`
  (`((639−301)/2, (479−231)/2)`), в игре/`CargoMenu` — `(253,126)`
  (центр `WINDOW_MAIN_MAP`); это `GameManager_GetDialogWindowCenterMode()`.
* Текст: `FONT5` (10 px), `GNW_TEXT_OUTLINE | 0xFF` (белый + чёрная обводка),
  область `(20,20)` 265×160 → 16 строк на страницу; если текст короче
  страницы, блок центрируется по вертикали (`DrawText`).
* Прокрутка: `PageUp`/`PageDown` шагом 16 строк и кнопки `BLDUP__U`/`BLDDWN_U`
  18×17 в (20,197)/(40,197); кнопки есть только при `rows > 16`.
* `Done` — `HELPOK_U/D` 65×24 в (120,193), подпись `_(4bfa)`, закрывают
  `Enter`/`Esc`; в секции `GAME_SCREEN_SETUP` `Done` сдвинут в (85,193), а
  `Keys` (`_(f0d6)`) в (155,193) переключает текст на подсказку `_(ceb6)`.
* Тексты: генератор `max-rust/tools/gen_help_data.py` читает
  `max-cpp-port/assets/help.json` + `language.json` и пишет
  `max-rust/crates/max_app/data/help.json` — **32 секции, 661 запись**
  (en-US, ASCII). Rust-клиент грузит его `include_str!` (как `credits.json`).
* Кнопки, которые открывают окно (сверено с C++): `?` на `Options`/`Planet`/
  `Players`/`Clan` (`GameConfigMenu::EventHelp`, `PlanetSelectMenu::EventHelp`,
  `ChoosePlayerMenu::EventHelp`, `ClanSelectMenu::EventHelp`), `Help` в
  `CargoMenu` (`_(CARGO_SETUP)`), `Help` в `ResearchMenu` (`RESEARCH_SETUP`),
  `Help` в `UpgradeMenu` (`UPGRADES_SETUP`), панель `HELP_OF`
  (`GameManager_MenuClickHelpButton` → `GAME_SCREEN_SETUP`) и клавиша `?` в
  игре. Окно модально: `Screen::Help` хранит экран под ним, в игре —
  `App::help_screen` поверх игры/`ResearchMenu`/`UpgradeMenu`.

### 13.3 Отличия (осознанные)

* **Текст секции целиком, а не абзац под курсором.** В C++ `HelpMenu_Menu`
  включает «режим помощи»: игрок кликает по элементу экрана, и окно показывает
  один абзац, привязанный к координатам (`help.json::coordinates`). В
  Rust-клиенте кнопка `?`/`Help` сразу открывает окно, поэтому показывается
  **весь текст секции** в порядке файла (абзацы через пустую строку). Это
  сохраняет все тексты и делает длинные секции (`CARGO_SETUP` — 99 строк,
  `OPTIONS_SETUP` — 62) прокручиваемыми; координаты записей сохранены в
  `data/help.json` для будущего режима.
* Палитра окна — собственная палитра `HELPFRAM` (как у остальных big image
  Rust-клиента); C++ рисует `HELPFRAM` системной палитрой, но индексы
  UI-цветов в ней те же.
* Клавиши: в C++ у окна только `PageUp`/`PageDown`/`Enter`/`Esc`; здесь
  добавлены `↑`/`↓` (одна строка), `K` = `Keys`, `?` = закрыть. У
  `ResearchMenu` добавлены `↑/↓` (выбор темы), `←/→`, `−/+`,
  `PageUp/PageDown`, `Enter`/`Esc`, `H`/`?` — иначе headless-QA и
  клавиатурный игрок не могут распределить центры.
* `ResearchMenu` не отправляет пакет 09 (как и `UpgradeMenu`): в Rust-протоколе
  его нет; распределение меняет локальное состояние команды. Звуки
  `V_M093`/`NDONE0`/`KCARG0` не проигрываются (аудио-слой отдельный).
* Попап-меню юнита не портировано: `R`/`U` заменяют пункты «Research»/
  «Buy Upgrade».

### 13.4 Свидетельства

* `qa_run research-help.qa` → **PASS**: 131 шаг, 3430 кадров; кадры
  `help-options`, `help-planet`, `help-newgame`, `help-cargo`,
  `help-research`, `help-game`, `help-game-keys`, `research-menu`,
  `research-plan` — эталоны `golden/research-help/` (9 PNG), `qa_golden_check`
  — 0 дрейфа. Сценарий проверяет: `assert help=open/help-section/help-row`,
  прокрутку (`help-row=16/32`), закрытие и возврат экрана; открытие
  `ResearchMenu`, `research-alloc attack=1` → `row 0 down` (free) →
  `row 1 up` → `Done` → `attack=0`, `shots=1`; через 16 ходов
  `assert research shots>=1`, `attack=0`.
* `cargo test -p max_app`: `researchmenu` (10 UNIT: раскладка, порядок тем,
  подсчёт центров, `free_capacity`, слайдер, `Done`/PowerOff, turns,
  детерминизм, клавиши/клики), `helpmenu` (9 UNIT: тексты четырёх секций,
  прокрутка, центровка, позиции окон, `Keys`, клики), `ui` (+3: `?` в
  настройке → `Screen::Help` → Esc, `R` + распределение в сессии, панель
  `HELP_OF` → `GAME_SCREEN_SETUP`); `tests/qa_driver.rs` — сквозной прогон
  `research-help.qa`.
* `ci.sh`/GitHub CI: сценарий `research-help.qa` и golden
  `golden/research-help` добавлены в стадию QA.

**Остаточные отличия**: текст секции целиком (см. выше), звуки, пакет 09,
`Research Complete`-оверлей на центрах (как и в C++-порте — статичный кадр).

---

## 14. MVE-интро и второй кадр турелей — **сделано** (2026-11-17)

Закрыты два последних визуальных пункта: настоящее видео `Introduction`
(`INTROFLC` → `maxint.mve`) и подтверждён второй кадр иконок турелей в
`CargoMenu`/`UpgradeMenu`.

### 14.1 Откуда берутся `angle_offsets` (не из `.SPW`)

Проверка оракула (`unit.cpp`, `reportstats.cpp`) и ресурсов:

* `angle_offsets[8]` — это **не** часть `.SPW`-файла и не часть спрайта
  `I_*`/`TANK`. Они лежат в конце ресурса `D_*` (`Unit::GetFrameInfo()`):
  8 счётчиков (`image_base/count`, `turret_*`, `firing_*`, `connector_*`) +
  8 пар `(dx, dy): i8` = 24 байта. У 11 типов с `TURRET_SPRITE`/
  `SPINNING_TURRET`; у остальных ресурс короче (8 байт).
* `.SPW` в каталоге игры — **RIFF/WAVE-звуки**, а не спрайты
  (`max-original-dasm/formats/spw.md` §1); таблицы кадров спрайта лежат в
  самом `MAX.RES` (`u16 image_count` + `u32 frame_offset[]`).
* Второй кадр иконки — это кадр 1 ресурса `I_*` (не `D_*`). C++
  `ReportStats_DrawListItemIcon` рисует кадр 0, затем для турельных типов —
  кадр 1 со сдвигом `angle_offsets[2] >> divisor` (`divisor` 1 у мобильных,
  2 у стационарных).

Rust-клиент делает то же: `mobilize::CargoMenuAssets` грузит `body = frame(0)`,
`turret = frame(1)` и `turret_offset = FrameInfo::turret_offset(2)` из `D_*`;
`mobilize::draw_icon` рисует обе иконки. В этом срезе путь уже был (коммит
`41d2d91`); добавлено покрытие **всех 11** турельных типов в
`tests/cargo_render.rs` (наличие кадра 1 и совпадение сдвига с `D_*`), плюс
существующий пиксельный тест «башня танка в списке» (`>20 px`).

### 14.2 Декодер MVE (`max_assets::mve`)

Формат (Interplay MVE, оракул `mvelib32.c` + публичная спека FFmpeg):

```text
файл: 20-байтная подпись "Interplay MVE File\x1A\0",
      u16 magic 0x001A, u16 version 0x0100, u16 checksum;
записи: u16 size, u16 type (0 init_audio, 1 audio_only, 2 init_video,
      3 video, 4 shutdown, 5 end);
внутри записи: u16 size, u8 opcode, u8 version, data.
```

Реализованы нужные интро опкоды: `CREATE_TIMER` (2, 15 FPS:
`rate*divider = 66 664 мкс`), `INIT_VIDEO_BUFFERS` (5, 432×320),
`SET_PALETTE` (12, 6-бит → `*4`), `SET_DECODING_MAP` (15, ниббл на блок),
`VIDEO_DATA_11` (17). `maxint.mve` — **4391 кадр**, 432×320, 8-бит
индексированные, 13 обновлений палитры по ходу фильма. Аудио-опкоды
читаются и пропускаются; субтитры `INTROSUB` не накладываются (честное
ограничение, отдельный слой `moviesubtitle.cpp`).

Видеокодек — точный порт `nfPkDecomp`: блоки 8×8, карта из нибблов
(младший — левый блок), коды 0x0…0xF: копия прошлого/позапрошлого кадра,
векторы движения, шаблоны 2/4/16/64 цветов. Таблицы `nf_pk_lut_4/5/6`
перенесены дословно; ветка `0xA` повторена построчно (в ней важен порядок
записи правой половины), поэтому кадры совпадают с C-обвязкой над
`nfPkDecomp` **бит-в-бит**. `flags & 1` переключает двойной буфер, как
`nfAdvance`; оба буфера стартуют нулями (детерминизм).

### 14.3 Экран `Introduction`

`IntroScreen::new(path)` открывает `maxint.mve` и сразу декодирует кадр 0 —
поэтому `shot intro` детерминирован, а `dt = 0` даёт стоп-кадр. В GUI
`advance(dt)` идёт с 15 FPS; кадр масштабируется как `movie_cb_show_frame` +
`cscale` (432×320 → 640×474, letterbox 3 px, nearest-neighbor). Esc/клик и
конец видео возвращают в главное меню. Если файла нет — прежняя заглушка
`MLOGO`/`MAINPIC`, чтобы тесты без данных работали.

### 14.4 Свидетельства

* `max_assets::mve` (3 UNIT + пример `mve_hashes`): метаданные 432×320/
  4391/15 FPS; FNV-1a кадров 0/50/100 = `74CDBDC5`/`22EB1E36`/`158D8E0D`
  (эталон снят C-обвязкой над `nfPkDecomp` оракула); полный проход 4391
  кадра; палитра первых кадров `[4,0,4]`, финальная `[0,4,44]`.
* `tests/intro.rs` (3): `INTROFLC` → `maxint.mve`, кадр 0 + letterbox,
  детерминированный повторный рендер, `advance` (15 FPS) и Esc → `MainMenu`.
* `visual-screens.qa`: `shot intro` (dt = 0) + `tick 210` → `shot intro-logo`
  (логотип Interplay); эталоны `golden/intro.png`, `golden/intro-logo.png`,
  `qa_golden_check` — 0 дрейфа (14 кадров).
* `cargo test -p max_app --test cargo_render` — иконки всех 11 турельных
  типов: кадр башни + сдвиг `angle_offsets[2]` из `D_*`.

---

## 15. Видимое движение, якорь зданий и кадры питания — **сделано** (2026-11-17)

Четыре регресса из ручной игры: юниты «не двигались» визуально, генератор
накладывался на шахту, у включённых генератора и шахты не было кадра
питания. Оракул — C++-порт (не менялся).

### 15.1 Почему движение не было видно

* Симуляция `max_core` по-прежнему делает **одну клетку за тик**
  (`unit_order::unit_move` — задокументированное расхождение с пиксельным
  `UnitInfo::Move`). В интерактивном локальном цикле тик идёт **каждый кадр**
  (`local_update` → `state.tick`), т.е. до 60 клеток/с.
* Старая `App::update_visual_units` подтягивала позицию к симуляционной
  максимум на 12 клеток/с и при расхождении > 2.5 клетки **снапала** к цели.
  Симуляция убегала на клетку за кадр, поэтому спрайт сначала стоял, а потом
  прыгал: визуально движения не было.
* В C++ скорость задаёт `Paths_CalculateStep` (`max_velocity = 8` px/тик) и
  тик ~30 Гц (`TickTimer`): 8 × 30 / 64 ≈ **3.75 клетки/с**; одна клетка
  проходится ~8 тиков, а не один.

Исправление (`max_app/src/lib.rs`): вместо «догоняния» кадр ведёт
**очередь фактического маршрута**. `update_visual_units` запоминает последнюю
симуляционную клетку (`visual_goals`), при её изменении дописывает пройденные
клетки в `visual_paths` и проходит их со скоростью
`VISUAL_CELLS_PER_SECOND` = 4 клетки/с с постоянным бюджетом `dt`.
Скачок > `VISUAL_TELEPORT_CELLS` (6) — телепорт/синхронизация: снап.
`dt = 0` (headless/QA) — снимок ровно по `grid` и сброс очереди, поэтому
golden-кадры детерминированы, а `snapshot_bytes`/CRC не меняются.
Визуальное состояние по-прежнему не входит в симуляцию.

`local_update` двигает визуал и в фазе `Advancing` и при открытом окне
перехода хода (симуляция стоит, маршрут доигрывается); на паузе — стоит.

### 15.2 Генератор и шахта

Раскладка `deploy.rs` **совпадает с C++**:
`GameManager_GetStartPositionMiningStation` → шахта, затем
`GameManager_GetStartingPositionPowerGenerator` (`point.x -= 1; point.y += 2`,
первый доступный кандидат на север). На стартовой карте `move-input` это
шахта `(22,22)` и генератор `(21,23)` — **разные клетки**; `SMLSLAB` стоит в
клетке генератора, `LRGSLAB` — в клетке шахты (как `DeployUnit`). Порядок
`spawn_unit`/RNG не менялся, `snapshot_bytes` раскладки тот же.

Наложение было **ошибкой отрисовки**: C++ `UnitInfo::SetPosition`
(`unitinfo.cpp:1301`) для `flags & BUILDING` сдвигает `x/y` на
`GFX_MAP_TILE_SIZE / 2 - 1 = +31` px от центра клетки — здание 2×2
(`MININGST`, `LRGSLAB`) рисуется в своём 2×2-прямоугольнике. Rust рисовал всё
от центра клетки, поэтому 116×108-спрайт шахты уезжал на полклетки вверх-влево
и заходил на клетку генератора. Теперь якорь — `render::unit_anchor_world`
(+31 для `BUILDING`), как `unit.x/y` в C++; спрайты шахты и генератора больше
не пересекаются (регресс-тест).

### 15.3 Кадры включённых зданий (генератор/шахта)

Оракул `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`) и
`UnitInfo::PowerDown` (`unitinfo.cpp:5123`):

* при `ORDER_POWER_ON` — `DrawSpriteFrame(image_base + 1)`;
* при выключении — `DrawSpriteFrame(image_base)`;
* у шахты `UnitInfo::UnitInfo` (`unitinfo.cpp:281`) задаёт
  `image_base = (team_clan - 1) * sizeof(uint16_t)` — 8 клановых пар кадров
  (в `MININGST` 24 кадра: 16 «клан × выкл/вкл» + 8 коннекторов), у `POWGEN`
  6 кадров: 0/1 — выкл/вкл, 2…5 — коннекторы.

Покадровой анимации по времени у **шахт и заводов** в C++ **нет**:
`UnitInfo::Animate` добавляет только draw bounds, `ProcessOrderPowerOn`
вызывает `DrawSpriteFrame` один раз. Поэтому «анимация» включённого
генератора/шахты — это **второй кадр состояния** (индикатор питания), который
Rust раньше не выбирал. `render::unit_frame_index` теперь: `image_base + angle`,
а при `orders == PowerOn` — `+1`; `render::image_base_for` берёт клановый
вариант шахты (консервативно: `Random`/нулевая база → клан 1). Сама симуляция
не меняется — `image_base` остаётся полем сейва, выбор кадра только на рендере.

При этом у **`RADAR`** (единственное здание с флагом `ANIMATED`) покадровая
анимация в C++ есть: `UnitsManager_ProcessOrderSentry` каждый тик делает
`image_index + 1` и после последнего кадра возвращается к `image_base`
(16 кадров из `D_RADAR`). Она портирована в §25 вместе с промежуточным полным
циклом кадров работающих шахт/заводов; `dt = 0` по-прежнему даёт статичный
кадр C++.

### 15.4 Свидетельства

* `tests/app.rs::movement_is_visible_between_frames_and_finishes_on_the_target_cell`
  — после тика два кадра с `dt = 1/60` различаются и визуальной позицией, и
  пикселями кадра; в конце маршрута позиция равна целевой клетке;
  `snapshot_bytes` между кадрами не меняется.
* `tests/local_sp.rs::local_update_with_real_dt_animates_move_orders` — тот же
  путь через интерактивный `local_update(dt)`: маршрут проходит промежуточные
  позиции и заканчивается в цели.
* `tests/move_input.rs::generator_and_mining_station_are_on_different_cells_without_sprite_overlap`
  — клетки разные, якорь шахты `+31` (`unit.x` в C++), спрайты не
  пересекаются, у обоих включённых зданий кадр `image_base + 1`.
* `max_core::deploy::tests::power_generator_starts_next_to_the_mining_station_but_not_on_it`
  — координаты генератора/плиты ровно по C++.
* `tests/building_frames.rs` (8) — `POWGEN`: вкл/выкл = кадры 1/0, пиксели
  различаются; `MININGST`: клан 7 → база 12, вкл = 13; `Random` → клан 1;
  работающая шахта листает полный блок кадров C++, выключенная статична;
  завод с очередью листается, без — нет; `RADAR` циклит 16 кадров `D_RADAR`
  (см. §25).
* QA: `move-input.qa` PASS; пересняты `golden/move-input/*`,
  `golden/visual-screens/{game,portrait}.png`, `golden/upgrade/upgrade-done.png`,
  `golden/research/research-done.png`, `golden/research-help/{help-game,
  help-game-keys,help-research,research-menu,research-plan}.png` — `qa_golden_check`
  **0 дрейфа**. Остальные кадры (меню/карго/загрузка) не изменились.

---

## 16. Живая диагностика ввода/движения и интерактивная высадка — **проверено** (2026-11-17)

Разбор жалобы «юниты не двигаются» по трассе **живого окна**
(`MAX_DEBUG_INPUT=1`, `eprintln` в `main.rs`/`ui.rs`) и живому прогону
`build/scripts/ui-test-move2.ps1` (`SendInput`, проверка фокуса). Оракул
`max-cpp-port` не менялся.

### 16.1 «Генератор стоит на клетке шахты» — это плита, а не генератор

Трасса печатала тип юнита (`unit_type as u8`), а не id, и в ней на клетке
высадки стояли `17@(22,22)` и `40@(22,22)`. Это **`LRGSLAB` (0x11) и
`MININGST` (0x28)**: C++ `GameManager_ProcessTeamMissionSupplyUnits`
(`game_manager.cpp:4996`) вызывает
`UnitsManager_DeployUnit(LRGSLAB, …, mining_station_location)`, т.е. плита
обязана делить клетку с шахтой. Генератор `POWGEN` (`2`) стоит отдельно:
`GameManager_GetStartingPositionPowerGenerator` (`game_manager.cpp:1833`) —
`point.x -= 1; point.y += 2`, затем первый доступный кандидат на север.

Живые значения (совпадают с `deploy.rs`/C++):

| Прогон | шахта `MININGST` | плита `LRGSLAB` | генератор `POWGEN` | Surveyor |
|---|---|---|---|---|
| `move-input` (QA/live, точка по умолчанию) | `40@(22,22)` | `17@(22,22)` | `2@(21,23)` | `57@(23,21)` |
| ручная партия из предыдущей сессии | `40@(72,40)` | `17@(72,40)` | `2@(71,41)` | `57@(73,39)` |

Поэтому «два юнита на клетке высадки» — это шахта + её плита (норма C++);
регресс-тест теперь проверяет и состав клетки, а не только `assert_ne!` клеток
генератора и шахты.

### 16.2 Интерактивный поток высадки

`App::new_game_on_surface` строит `TeamDeployment` из фазы высадки
(`start_positions` → `new_game_config`) и вызывает
`deploy_starting_units_with`. В C++ точка высадки перед высадкой проходит
`GameManager_PopulateMapWithResources` (`game_manager.cpp:4747`):
`FindValidStartingPosition` + `Optimize` материалов/топлива — для **каждой**
играющей команды, включая человека. Раньше Rust синхронизировал итоговую
позицию только для точек, добавленных ИИ; теперь — для всех `TeamDeployment`.
В обычном потоке точка из `LandingScreen` уже валидна (`GameManager_IsValidStartingPosition`),
поэтому раскладка и golden не изменились, но поток больше не расходится с C++
при любой будущей правке `Optimize`.

### 16.3 Почему «RMB не доходит» оказалось ложной тревогой

Путь `SDL MouseButtonDown(Right) → WindowInput::MouseRightClick →
InputEvent::RightClick → handle_session_input → session_chrome_right_click →
session_map_right_click` корректен. В живом прогоне событие было
(`[in] screen=InSession event=RightClick(532,178)`), но предыдущий сценарий
подтверждал высадку **кликом по карте** (`468,178`), а не `Enter`: точка
оказывалась в `(72,40)`, камера центрировалась там, и рассчитанные от
`(22,22)` пиксели попадали на шахту/инженера. RMB по своему юниту — это
**отмена** (`Cancelled`), поэтому приказ не появлялся.

Что добавлено/исправлено:

* `MAX_DEBUG_INPUT` теперь печатает реальные тики: локальный цикл
  (`local_update`) инкрементирует `App::ticks`, как `App::tick`; раньше
  трасса всегда показывала `tick 0` и не могла подтвердить движение.
* `[tick N]` содержит `turn`, `phase`, `sel`, `preview=N` и для каждого юнита
  `type@(grid)v(visual)`; `[rclick]` — `map/grid/viewport_origin/selected` и
  результат (`move` со `steps`, `cancel`, `cannot-move`, `no-path`, …).
  Без `MAX_DEBUG_INPUT` вывод не появляется.
* `session_map_right_click` не менял поведение: только диагностика.

### 16.4 Живой прогон `ui-test-move2.ps1` (SendInput)

Сценарий: меню → Options/Planet/Players → мобилизация `Done` → высадка
**`Enter`** (как `key ENTER` в `move-input.qa`) → LMB по Surveyor'у → RMB на
клетку в 2 клетках → End Turn → LMB по целевой клетке. Перед каждым кликом
проверяется `GetForegroundWindow() == hwnd`; кнопки — `SendInput`
(`MOUSEEVENTF_LEFTDOWN/UP`, `RIGHTDOWN/UP`).

Практические грабли (в скрипте учтены):

* В `INPUT` union начинается со смещения **8** (выравнивание `ULONG_PTR`), а не
  4: при `FieldOffset(4)` `SendInput` возвращает 2/`ERROR_SUCCESS`, но SDL3 не
  получает ни одного `MouseButton` (тип/флаги попадают не туда). При
  `FieldOffset(8)` (или `type, pad, MOUSEINPUT`) клики доходят.
* Трассу дочернего процесса нельзя читать `File.ReadAllText` (файл занят
  редиректом); нужен `FileStream` с `FileShare.ReadWrite`.
* `^\[tick` в `-match` не работает без `RegexOptions.Multiline`; в ожидании
  использовать `(?m)^\[tick`.

Результат последнего прогона (`build/ui-test-move2/`):

```
A: mining=40@(22,22)  generator=2@(21,23)  large-slab=17@(22,22)  surveyor=57@(23,21)
A: units at mining cell: 17,40
B: [in] … RightClick; [rclick] result=move selected=7 steps=2 to=(25,21)
B: green preview pixels in map area: 152
C: 57@(23,21) -> (25,21); vis 23,21 -> 23.8,21 -> 24.7,21 -> 25,21
PASS (трасса build/ui-test-move2/trace.log, кадры 01…11)
```

### 16.5 Свидетельства

* `tests/move_input.rs::interactive_landing_keeps_the_generator_off_the_mining_station_cell`
  — `PreGameFlow::commit_landing` → `start_positions` → `TeamDeployment` →
  `deploy_starting_units_with`: шахта `(20,20)`, генератор `(19,21)`, плиты по
  своим зданиям; на клетке шахты ровно `{MININGST, LRGSLAB}` (без `POWGEN`).
* `tests/move_input.rs::rmb_then_end_turn_moves_the_unit_through_the_path_in_the_local_loop`
  — реальные `handle_session_input` LMB/RMB в локальной партии
  (`start_local_game`), зелёный предпросмотр в chrome-кадре, `local_update(dt)`
  проходит промежуточные позиции и заканчивается в цели, End Turn кнопкой
  `END_TURN_BUTTON` не теряет приказ, после хода ИИ — `grid`/`vis` в цели и
  повторное выделение кликом.
* `build/scripts/ui-test-move2.ps1` — живой прогон с `SendInput`; PASS.
* QA: `move-input.qa` PASS; `golden/move-input/*` — 0 дрейфа (раскладка не
  менялась, менялись только диагностика и синхронизация точки высадки).

## 17. Строки статистики по типу юнита — **сделано** (2026-11-17)

Жалоба: у шахты в панели статистики (`WINDOW_STAT_WINDOW` (10,173) 153×48,
строки `WINDOW_STAT_ROW_1..4`) показывался `Speed` — бессмыслица для
стационарного здания. В Rust панель всегда рисовала фиксированные
`Hits/Ammo/Speed/Shots` из `UnitInfo`; оракул `max-cpp-port` не менялся.

### 17.1 Как это устроено в C++ (`game_manager.cpp`)

`GameManager_UpdateInfoDisplay` (`game_manager.cpp:7667`) **не** переключается
по классу юнита — строки выбираются по атрибутам `UnitInfo::base_values`, а
`GameManager_DrawInfoDisplayRow` (`:7370`) рисует строку только при
`base_value && factor`. Поэтому у шахты `ATTRIB_SPEED == 0` — строки `Speed`
просто нет (фон окна), а не «0/0».

Базовые строки (`:7682-7731`):

| Строка | Условие | Значение | База | factor |
|---|---|---|---|---|
| 1 | всегда | `unit->hits` | `ATTRIB_HITS` | 4 |
| 2 | `ATTRIB_ATTACK != 0` | `unit->ammo` | `ATTRIB_AMMO` (чужой — 0) | 1 |
| 2 | иначе | `unit->storage` | `ATTRIB_STORAGE` (чужой — factor 0) | 1/2/3, для `RAW/FUEL/GOLD` — 10 |
| 3 | — | `unit->speed` | `ATTRIB_SPEED` | 1 |
| 4 | — | `unit->shots` | `min(ATTRIB_ROUNDS, ATTRIB_AMMO)` | 1 |

Затем наложения (только своя команда):

* `DrawInfoDisplayType2` (`:7437`, при `ATTRIB_STORAGE == 0`): `Usage`/`Power`
  (строка 2), `Total`/`Usage` по потреблению и выработке комплекса (строки 3/4);
* `DrawInfoDisplayType3` (`:7581`, `GREENHSE`): `points` + `Total` очков партии;
* `DrawInfoDisplayType1` (`:7509`, иначе): `Usage`/`Teams` рабочих + `Total`/
  `Usage` по комплексу;
* запас комплекса (`:7744`: свой, `STATIONARY`, `RAW/FUEL/GOLD`, storage > 0):
  `Total` (сумма `storage` / сумма `ATTRIB_STORAGE` зданий комплекса) поверх
  строки 3.

Цвет значения (`:7388-7407`): красный `≤ base/4`, жёлтый `≤ base/2`, зелёный
иначе (сохранено в [`StatRow::color`]); подписи — `language.json`
(`c02c` Hits, `5027` Ammo, `1274` Cargo, `c372` Speed, `2bce` Shots, `8d59/
3aea/81c5/160c` Usage, `848e` Power, `1e1a` Teams, `d28b/c9b3/d4ca/7be8/5f25`
Total, `c54a` points).

Осознанные ограничения порта: отладочный `GameManager_MaxSpy` (показ панели
чужих юнитов) не портирован — наложения рисуются только при
`unit.team == player_team`; в циклах `Type1/Type2` C++ на время считает
производство юнита в `ORDER_UPGRADE` по предыдущему приказу
(`SetOrder(prior)`/`SetPriorOrder`), здесь то же делается на клоне юнита без
мутации состояния.

### 17.2 Что показывает панель по классам (порт `panels::rows_for`)

| Класс (примеры) | Строка 1 | Строка 2 | Строка 3 | Строка 4 |
|---|---|---|---|---|
| Мобильные наземные/морские с атакой (`TANK`, `SCOUT`, `INFANTRY`, `ARTILLRY`) | Hits | **Ammo** | **Speed** | **Shots** |
| Мобильные без атаки (`SURVEYOR`, `SCANNER`, `MASTER`) | Hits | — (storage 0) | Speed | — |
| Строители/транспорт (`CONSTRCT`, `ENGINEER`, `BULLDOZR`, `SPLYTRCK`, `CARGOSHP`) | Hits | **Cargo** | Speed | — |
| Воздушные (`FIGHTER`, `BOMBER`, `AWAC`; `AIRTRANS` с грузом 3) | Hits | — / **Cargo** | Speed | Shots / — |
| Турели (`GUNTURRT`, `ANTIAIR`, `ARTYTRRT`, `ANTIMSSL`) | Hits | **Ammo** | — (speed 0) | **Shots** |
| Мины (`LANDMINE`, `SEAMINE`) | Hits | — (ammo 0) | — | — |
| Шахта, склады (`MININGST`, `ADUMP`, `FDUMP`, `GOLDSM`) | Hits | **Cargo** (`ATTRIB_STORAGE`) | **Total** (запас комплекса) | — |
| Казармы/доки (`BARRACKS`, `DEPOT`, `HANGAR`, `DOCK`) | Hits | **Cargo** (хранят юнитов) | — | — |
| Заводы «рабочие» (`TRAINHAL`, `RESEARCH`; `Type1`) | Hits | **Usage** (рабочие) | **Total** (рабочие) | — |
| Заводы «энергия» (`LIGHT/HEAVYPLT`, `AIRPLT`, `SHIPYARD`; `Type2`) | Hits | **Usage** (энергия) | **Total** (потребление) | — |
| Электростанции (`POWERSTN`, `POWGEN`) | Hits | **Power** (выработка) | **Total** (выработка) | **Usage** (потребление) |
| Потребители энергии (`COMMTWR`) | Hits | **Usage** (энергия) | **Total** (потребление) | — |
| Жилой блок (`HABITAT`) | Hits | **Teams** (рабочие) | **Total** (рабочие) | — |
| Теплица игрока (`GREENHSE`) | Hits | **points** | **Total** (очки партии) | — |
| Чужой юнит (любой) | Hits | — (`Ammo` с базой 0 / `Cargo` с factor 0) | Speed (если > 0) | Shots (если > 0) |

«—» — строка не рисуется (нулевая база/`factor`), как в C++. Строка 3 у
шахты — `Total` запаса комплекса, поэтому `Speed` у здания отсутствует.

### 17.3 Код и тесты

* `max_app::panels::rows_for` + `StatRow`/`StatLabel` (`crates/max_app/src/panels.rs`)
  — публичный порт выбора строк; `draw_stats` рисует только `Some(..)`,
  `StatRow::color` — пороги C++.
* 10 unit-тестов `panels::tests` (шахта/танк/турель/разведчик/завод/
  электростанция/жилой блок/чужой юнит/цвета + «Speed только при
  `ATTRIB_SPEED > 0`» на всех 93 типах), 2 кадровых
  `tests/stat_panel.rs` (шахта: 3 строки, танк: 4; завод: `Usage`/`Total`),
  регресс в `hud` (нижняя панель без Speed) и `mobilize` (`CargoMenu`:
  `UnitStats_DrawStats` не рисует Speed у зданий).
* QA: `visual-screens.qa` и `move-input.qa` — новый кадр `mine-selected`
  (Hits 56/56, Cargo 0/25, Total 0/25, строка 4 пустая), `portrait`
  (Surveyor: Hits 16/16, Speed 7/7, пустые строки 2/4) — golden обновлён;
  `production.qa`/`cargo-full.qa` не изменились (BuildMenu/CargoMenu
  закрывают панель/уже условны).

---

## 18. Меню команд выделенного юнита — **сделано** (2026-11-17)

Попап `UNTBTN_U`/`UNTBTN_D` поверх карты: C++
`GameManager_InitPopupButtons`/`DeinitPopupButtons`/`ProcessPopupMenuInput`
(`game_manager.cpp:3568/3664/2607`) + `UnitsManager_Popup_Init*`
(`units_manager.cpp:543-1352`) + таблица `UnitInfo::Init`
(`unitinfo.cpp:454-582`). Код — `max_app::unitmenu`, ввод — `ui.rs`
(`session_map_left_click`, `session_unit_menu_input`), отрисовка —
`render::draw_unit_menu`.

### 18.1 Триггер открытия (точный, из C++)

Меню открывается **повторным кликом по уже выделенному своему юниту**
(`GameManager_ClickUnit`: `unit1 == unit2` → `InitPopupButtons`). Первый клик
только выделяет. Правая кнопка меню не открывает (по выделенному — статистика
`UnitStats_Menu`, по другому — выделение). Условия `InitPopupButtons`: приказ не
`ORDER_DISABLE`, юнит в видимой части карты, для `MOBILE_LAND_UNIT` состояние не
`ORDER_STATE_UNIT_READY`, и `popup->init` зарегистрировал ≥ 1 пункт. Открытие
закрывает выделение-предпросмотр и перерисовывает портрет/статистику
(`GameManager_MenuUnitSelect` — у нас панель и так следует за выделением).

Закрытие — всё то, что в C++ вызывает `DeinitPopupButtons`: `Esc`
(`game_manager.cpp:5469`), повторный клик по юниту (`GameManager_ClickUnit`),
клик вне меню, правая кнопка по карте, смена хода
(`GameManager_UpdateGuiControl` → `hud.clear_interaction` + `close_unit_menu`).

**Осознанное расхождение:** C++ открывает попап уже при *выделении*
стационарного юнита (`GameManager_UnitSelect` → `InitPopupButtons`). У нас меню
открывается только повторным кликом — иначе менялись бы существующие
golden-кадры (`mine-selected` в `move-input.qa`/`visual-screens.qa`), где
здание выделено без попапа.

### 18.2 Раскладка (по C++, без «на глаз»)

| Элемент | C++ | Значение |
|---|---|---|
| картинка кнопки | `UNTBTN_U`/`UNTBTN_D` | 42×21 (`MAX.RES`) |
| ширина окна | `image->width + 6` | 48 |
| высота окна | `height * count + (count + 1) * 3` | `21n + 3(n+1)` |
| левый край | `ulx = (unit->x + 32)` | правый край клетки юнита |
| верх | `uly = unit->y - height/2` | центр клетки по Y |
| сдвиг у правого края | `ulx -= width + Gfx_ZoomLevel` | на ширину меню + одну клетку |
| шаг кнопок | `uly += image->height + 3` | 24 px |
| подпись | `SetCaption(caption, -1, 2)` | `GNW_TEXT_FONT_2`, `COLOR_GREEN` (0x02), по центру |
| нажатый пункт | `state[i]` → `image_up = UNTBTN_D` | `sentry` при `ORDER_SENTRY`, `repair`/`reload` в курсорном режиме, `Auto`, `Manual`, мины (`laying_state`) |

Фон окна не закрашивается: в C++ попап рисуется прямо над картой (окно
`WINDOW_POPUP_BUTTONS` ограничивает только область перерисовки), кнопки —
`42×21` картинки поверх карты. Подпись — та же, что в оригинале (en-US из
`language.json`: `_(b0b9)` Manual, `_(886b)` x-fer, `_(e08d)` enter,
`_(6c45)` stop, `_(7fef)` upgrade, `_(97a2)` Upg. All, `_(0cf6)`/`_(524e)`
sentry, `_(2b7d)` Done, `_(7e56)` remove, `_(0286)` Auto, `_(e183)` attack,
`_(9d39)` reload, `_(81df)` place, `_(a1a1)` repair, `_(b3ef)`/`_(7b6d)` Build,
`_(cced)` start, `_(7a07)`/`_(3f13)`/… stop, `_(2e00)` resrch, `_(1da0)` buy upg,
`_(33b7)` allocate, `_(200c)` activate, `_(8452)` load, `_(6d6a)` disable,
`_(a0fd)` steal).

Порядок пунктов — `UnitsManager_RegisterButton`: устойчивая вставка по
возрастанию символа позиции, `'0'` → `':'` (поэтому «remove»/«place» последние);
при равных позициях сохраняется порядок регистрации (`'5'`: enter, затем
upgrade).

### 18.3 Состав пунктов по типу (таблица `UnitInfo::Init`)

| Тип (примеры) | Пункты (кроме общих) |
|---|---|
| танк, турели, пехота, корабли, самолёты (`TANK`, `GUNTURRT`, `INFANTRY`, …) | `attack`, `sentry`, `Done` |
| строители (`CONSTRCT`, `ENGINEER`) | `Build` (или `stop`, если `ORDER_BUILD` в процессе) |
| заводы (`LIGHTPLT`, `LANDPLT`, `AIRPLT`, `SHIPYARD`, `TRAINHAL`) | `Build`, `start` (при `ORDER_HALT_BUILDING`) / `stop` |
| `SURVEYOR` | `Auto` |
| `SPLYTRCK`, `CARGOSHP` | `reload` |
| `MINELAYR`, `SEAMNLYR` | `place`, `remove` (мины) |
| `REPAIR` | `repair` |
| `CLNTRANS`, `SEATRANS`, `AIRTRANS`, `BARRACKS`, `DEPOT`, `HANGAR`, `DOCK` | `activate`, `load` |
| `MININGST` | `allocate`+`stop` (или `start` при выключенном питании) |
| `POWERSTN`, `POWGEN` | `start`/`stop` (питание) |
| `RESEARCH` | `resrch`+`stop` (или `start`) |
| `GOLDSM`, `COMMTWR` | `start`/`stop` + `buy upg` |
| `GREENHSE` | `start`/`stop` |
| `RECCENTR` | `start` (при `ORDER_POWER_OFF`) |
| `BULLDOZR` | `remove`/`stop` (расчистка) |
| `MASTER` | (пункта `start` нет: площадка мастера не проверяется) |
| `ADUMP`, `FDUMP`, `FUELTRCK` | — |
| `COMMANDO` | `disable`, `steal`, `attack` |

Общие (`UnitsManager_Popup_InitCommons`): `Manual` (только simultaneous moves,
ammo > 0, не диверсант), `x-fer` (груз `RAW..GOLD`, не `CLEAR`/`BUILD`),
`enter` (подвижные), `stop` (есть маршрут), `upgrade`+`Upg. All`
(`STATIONARY` + есть что улучшать: сравнение `base_values` с
`TeamUnits::current_values`), `sentry` (флаг `SENTRY_UNIT`, не диверсант),
`Done` (`SENTRY_UNIT`), `remove` (`STATIONARY`).

### 18.4 Что делает каждый пункт: реализовано / отключено

| Пункт | C++ | У нас |
|---|---|---|
| `Build` | `BuildMenu_Menu(unit)` | **реализовано**: `App::open_build_menu` |
| `start`/`stop` питания | `ORDER_POWER_ON`/`ORDER_POWER_OFF` (P1) | **реализовано**: `hud::build_power_order` |
| `buy upg` | `UpgradeMenu(team, complex)` | **реализовано**: `App::open_upgrade_menu` |
| `resrch` | `ResearchMenu_Menu(unit)` | **реализовано**: `App::open_research_menu` |
| `sentry` | `ORDER_SENTRY` → иначе `ORDER_AWAIT` (P1) | **реализовано**: пакет 08 P1 |
| `stop` | `ORDER_AWAIT` + `ORDER_STATE_CLEAR_PATH` (P3) | **реализовано** |
| `upgrade` | `SetParent(self)` + `ORDER_UPGRADE`/`INIT` (P2) | **реализовано** (процессор в ядре — TODO P3, как и `UnitInfo::UpgradeInt` в C++-порте) |
| `place`/`remove` мины | `ORDER_LAY_MINE` + `PLACING_MINES`/`REMOVING_MINES` (P1) | **реализовано** |
| `enter` | `enter_mode` + `ORDER_MOVE_TO_UNIT` (P3) | **реализовано** (режим в `HudState.enter_mode`, сбрасывается после клика, как C++) |
| `repair` | курсор `cursor = 4`, клик по своему → `ORDER_REPAIR` (P2) с родителем | **реализовано** (нужен `storage`, цель не `ORDER_DISABLE`) |
| `reload` | курсор `cursor = 6`, клик → `ORDER_RELOAD` (P2) | **реализовано** |
| `Auto` | тумблер `auto_survey` (+ `Ai_EnableAutoSurvey`) | **реализовано** (поле `UnitInfo::auto_survey`, как C++, локально) |
| `Manual` | тумблер `disabled_reaction_fire` | **реализовано** (идёт в P1-поле `disabled_reaction_fire`; в C++ настройка есть только в simultaneous moves — наш UI такой режим не предлагает, поэтому пункт практически не появляется) |
| `Done` | `UnitsManager_PerformAction` | **реализовано в объёме стека**: `Await`+`STATIONARY` → `ORDER_STATE_READY_TO_EXECUTE_ORDER`, снятие выделения (`auto_select`); «подтверждения хода» нет — приказы в нашем стеке исполняются сразу |
| `x-fer` | курсор `cursor = 3` → `GameManager_TransferCargo` → `TransferMenu_Menu` + `ORDER_TRANSFER` (P3) | **отключено**: `TransferMenu_Menu` (диалог количества) не портирован, а `ProcessOrderTransfer` в ядре — TODO P3 (`unit_order.rs:2120`). Приказ `Transfer` в протоколе есть (`P3.transfer_cargo`), поэтому пункт оставлен видимым (серым) |
| `attack` | тумблер `targeting_mode` | **отключено**: режим наведения не портирован, атака — кликом по врагу |
| `load` | курсор `cursor = 7` → `ORDER_LAND`/`MOVE_TO_UNIT` | **отключено**: `ORDER_LAND` — процессор P1 и не несёт `parent_unit`, поэтому посадка не доедет до сети/получателя; вся цепочка `GameManager_ProcessMouse` (CURSOR_LOAD) не портирована |
| `activate` | `RepairShopMenu_Menu` | **отключено**: меню ремонтного цеха не портировано |
| `allocate` | `AllocMenu_Menu` (окно `ALLOCFRM`) | **реализовано**: `App::open_alloc_menu` → `max_app::allocmenu` (см. §20) |
| `start` завод | `UnitInfo::BuildOrder` | **реализовано**: P1 `ORDER_BUILD`/`INIT` (очередь и `build_time` сохранены остановкой), см. §21 |
| `stop` стройки | `GameManager_SelectBuildSite` (`game_manager.cpp:3529`) | **реализовано**: P1 `ORDER_BUILD`/`BUILD_CANCEL` → `CancelBuilding` (мобильный строитель — `AWAIT` и чистая очередь; завод — `HaltBuilding`), см. §21 |
| бульдозер (`stop` расчистки) | `GameManager_SelectBuildSite`, `Access_GetRemovableRubble` | **отключено**: выбор площадки/расчистка из попапа не портированы |
| `remove` здания | `UnitsManager_SelfDestructMenu` (диалог Arm/Cancel) | **отключено**: диалог не портирован |
| `Upg. All` | `UnitsManager_Popup_OnClick_UpgradeAll` (комплексы/материалы) | **отключено**: учёт материалов комплекса не портирован |
| `disable`/`steal` | курсоры 8/9 диверсанта | **отключено**: стелс-режимы не портированы |
| мастер-строитель `start` | `FindValidPowerGeneratorPosition` | **отключено** (пункт не регистрируется, как при `is_placeable == false`) |

Отключённый пункт **остаётся в списке** (как в C++ кнопка без обработчика),
рисуется серым (`0xA2`) и не срабатывает; причина — в
`UnitMenuItem::note` и в журнале HUD при попытке активации (QA-команда).

### 18.5 Ввод

* мышь: клик по кнопке пункта; клик вне меню/правый клик — закрыть;
* клавиатура: `toupper(символ позиции)` или `toupper(первая буква подписи)` —
  `GameManager_ProcessPopupMenuInput`; **стрелки и `Enter` в C++ попапе не
  работают** (нет выбора/подсветки), поэтому здесь тоже молча игнорируются;
* `Esc` — закрыть (не выходить из партии);
* пока меню открыто — модально: карта, панели и чат ввод не получают.

### 18.6 Код, тесты, QA

* `max_app::unitmenu` — `ItemId`/`UnitMenuItem`/`UnitMenu`,
  `items_for` (порт `InitCommons` + `Init*` + сортировка
  `UnitsManager_RegisterButton`), `layout`/`button_rect`/`item_at`/`key`/
  `menu_event`/`draw`, приказы `p1_order`/`p2_order`/`p3_order`,
  `cursor_mode_click`/`enter_mode_click`.
* `Assets::unit_menu` грузит `UNTBTN_U`/`UNTBTN_D`; отсутствие — предупреждение
  в отчёте ассетов (меню не рисуется).
* 13 unit-тестов (`unitmenu::tests`): порядок/состав пунктов для танка,
  строителя, транспорта; `stop` только с маршрутом; нажатый `sentry`; геометрия
  раскладки и шаг кнопок; горячие клавиши; отключённый пункт не срабатывает;
  `p1`/`p2`/`p3` приказы и их применение; `repair`/`enter` клики; завод с
  готовым юнитом меню не открывает.
* 12 кадровых тестов (`tests/unit_menu.rs`): повторный клик открывает меню,
  пункты по типу, `Esc` закрывает (и не выходит из партии), модальность
  (клик по другому юниту закрывает меню и не меняет выделение), клик по пункту
  `sentry` даёт `ORDER_SENTRY`, горячая клавиша снимает его, `Build` открывает
  `BuildMenu`, отключённый пункт не срабатывает, меню заметно меняет кадр,
  уровни процессоров приказов (`P1/P2/P3`), курсорный `repair` и `enter`.
* QA `max-rust-qa/scripts/unit-menu.qa`: покупка танка и строителя в
  мобилизации → высадка → `select type Tank` → `unit-menu open` (повторный
  клик) → проверки пунктов → `unit-menu item sentry` (`assert unit-order=sentry`)
  → повторное открытие (нажатый пункт) → `key 8` (снять) → `Esc` → модальность →
  меню строителя → `Build` открывает `BuildMenu`. Кадры
  `unit-menu-tank-selected`, `unit-menu-tank`, `unit-menu-sentry-on`,
  `unit-menu-builder` — golden `max-rust-qa/golden/unit-menu` (0 дрейфа).

## 19. Угол юнита по ходу движения — **сделано** (2026-11-17)

Симптом: спрайт подвижного юнита мог смотреть на 45° (или 90°) в сторону от
направления, в котором он ехал. Причина была не в кадре спрайта, а в двух
местах: (1) угол на клетке поворота начинал меняться позже, чем в C++; (2)
визуальная очередь кадра отстаёт от симуляции, а кадр читал **симуляционный**
угол — к моменту, когда спрайт ещё ехал по старому отрезку, симуляция уже
повернула к следующей клетке.

### 19.1 Как это устроено в C++ (оракул)

| Шаг | Код | Что делает |
|---|---|---|
| таблица направлений | `path.hpp:51` `DIRECTION_OFFSETS` | N, NE, E, SE, S, SW, W, NW = `(0,-1) (1,-1) (1,0) (1,1) (0,1) (-1,1) (-1,0) (-1,-1)` — индексы угла 0..7 |
| `(dx,dy) → angle` | `Paths_GetAngle` (`paths.cpp:219`) | 8 направлений по знакам `x`/`y`; диагонали — когда оба ненулевые |
| поворот | `Paths_UpdateAngle` (`paths.cpp:258`) | сравнивает с текущим, выбирает короткую сторону (`delta_a >= delta_b` → −1), делает **ровно один шаг 45°** и возвращает `true` |
| перед шагом | `GroundPath::Execute` (`groundpath.cpp:236/260`) | угол = направление текущего `PathStep`; если `Paths_UpdateAngle` вернул `true` — движение в этот тик **не начинается** (юнит доворачивает на месте) |
| на завершении клетки | `UnitInfo::Move` (`unitinfo.cpp:1835`) → `GroundPath::UpdateUnitAngle` (`groundpath.cpp:430`) | после `moved == GFX_MAP_TILE_SIZE`: если есть следующий шаг, `Paths_UpdateAngle` к его направлению — один шаг 45° начинается **на границе клетки** |
| движение | `UnitInfo::Move` (`unitinfo.cpp:1781`) | `step_x/y = DIRECTION_OFFSETS[angle]` — юнит физически едет только туда, куда смотрит |
| башня | `UnitInfo::UpdateAngle` (`unitinfo.cpp:1161`) | при повороте корпуса на `image_diff` у `TURRET_SPRITE` башня поворачивается на тот же шаг: `turret_angle = (turret_angle + image_diff) & 7` |
| кадр | `RenderWithConnectors` (`unitinfo.cpp:4838`) | корпус `image_base + angle`; башня `turret_image_base + turret_angle` со сдвигом `angle_offsets[angle]` |

Пример на 8 клетках: юнит смотрит на север (0), маршрут на восток (2):
`0 → 1 → 2`, затем шаг; после остановки угол остаётся 2 (последнее
направление). На L-маршруте (восток ×2, затем север) после второй восточной
клетки корпус уже `E → NE`, следующий `Execute` доводит `NE → N` и только потом
начинает шаг на север.

### 19.2 Что портировано

* `max_core::unit_order::paths_update_angle` — `Paths_UpdateAngle` 1:1 плюс
  связка башни из `UnitInfo::UpdateAngle` (`TURRET_SPRITE`).
* `max_core::unit_order::ground_path_update_unit_angle` — `GroundPath::UpdateUnitAngle`
  (один шаг к следующему шагу маршрута); вызывается из `unit_move` после
  списания текущего `PathStep` — как `path->UpdateUnitAngle(this)` в C++.
* `ground_path_execute` уже повторял `GroundPath::Execute`: сначала
  `paths_update_angle`, при повороте — `return false` без движения.
* `max_app::App::visual_angle` + `VisualStep.angle`: визуальная очередь хранит
  направление **отрезка** (из фактического смещения клеток, `Paths_GetAngle`),
  а не читает симуляционный угол в момент кадра. Пока спрайт идёт по отрезку,
  кадр берёт его угол; когда очередь пуста — текущий `UnitInfo.angle`.
* `max_app::render::draw_unit_sprites_with_angle` /
  `unit_frame_index_for_angle`: карта рисует корпус `image_base + visual_angle` и
  башню со сдвигом `angle_offsets[visual_angle]`; HUD-портрет и тесты — по
  `unit.angle` (поведение старых вызовов сохранено).
* Детерминизм: `angle` входит в `GameState::snapshot_bytes` (CRC состояния), но
  **не** входит в сетевой CRC пакета 45 — ровно как `GameManager_GetUnitListChecksum`
  в C++; снапшоты двух прогонов совпадают побайтово, сторона поворота и число
  шагов детерминированы.

### 19.3 Тесты и QA

* `max_core::unit_order::tests`: `paths_angle_and_turn_match_cpp`,
  `ground_path_update_unit_angle_matches_cpp`,
  `body_turn_rotates_turret_like_cpp_update_angle`,
  `all_eight_directions_move_facing_the_step` (8 направлений: поворот, шаг
  только «по ходу», после остановки — последнее направление),
  `turn_at_cell_boundary_precedes_the_next_step` (L-маршрут и граница клетки),
  `angle_updates_deterministically_in_snapshot` (2 прогона, снапшот каждого тика
  совпадает; изменение `angle` меняет снапшот).
* `max_app::tests`: `visual_angle_stays_on_the_segment_being_animated` (пока
  спрайт смещается по оси, `visual_angle` равен направлению смещения; на
  повороте симуляция уже `NE`, а спрайт ещё смотрит `E`);
  `unit_sprites::explicit_body_angle_selects_the_corresponding_sprite_frame`
  (явный угол = кадр `unit.angle`, разные углы — разные кадры);
  `render::tests::frame_index_uses_the_given_body_angle_and_power_on_offset`.
* QA: `move-input.qa` после перехода на восток проверяет `assert angle=2`
  (новая команда драйвера `assert angle=N`, 0..7); golden `move-input/*` —
  **0 дрейфа** (сценарии с `dt = 0` кадр не интерполируют, поэтому эталоны не
  переснимались).
* Windows UI: `build/scripts/ui-test-move2.ps1` (живое окно, SendInput,
  скриншоты `build/ui-test-move2/*.png`) — прогон движения после сборки
  `build/scripts/build-windows-ui.ps1`; headless-кадры — `qa-out/move-input/*`.
* Headless-артефакт: `max_app --example angle_demo -- qa-out/angle` снимает
  `01-east-with-sim-ne.png` (симуляция уже NE, спрайт ещё едет на восток и
  смотрит E), `02-turn.png` (поворот на клетке) и `03-stopped-north.png`
  (остановка, угол 0); пример сам проверяет «угол = направлению смещения» и
  завершается кодом 1 при расхождении.

## 20. Распределение добычи шахты (`Allocate`) — **сделано** (2026-11-18)

Пункт `allocate` меню команд `MININGST` раньше был серым («`AllocMenu_Menu` не
портирован»). Теперь он открывает диалог распределения добычи — порт
`allocmenu.cpp` 1:1.

### 20.1 Как это устроено в C++ (оракул)

| Что | Код | Значение |
|---|---|---|
| окно | `AllocMenu::AllocMenu` (`allocmenu.cpp:153`) | `ALLOCFRM` 640×480, `Window(ALLOCFRM, GetDialogWindowCenterMode())`; при `dialog_center_mode = 0` это центр `WINDOW_MAIN_MAP`, а `win_move` (`gnw.c:663`) прижимает окно 640×480 к `(0,0)` — диалог занимает весь экран |
| заголовок | `_(c771)` | `Allocation Menu`, `Text_TextBox(230, 6, 180, 20)`, `FONT5`, `COLOR_GREEN`, по центру |
| подписи | `_(8481)`, `_(1e19)`, `_(5ae6)`, `_(a80f)`, `_(5432)`, `_(04dd)`, `_(dff1)`, `_(e187)`, `_(89a3)` | `Raw`/`Usage`/`Reserve`, `Fuel`/`Usage`/`Reserve`, `Gold`/`Usage`/`Reserve` — по 78×19 в x = 42, `FONT5`, `COLOR_GREEN` |
| секции | `MENU_CONTROL_DEF(…, 70/190/310, …)` | три секции (сырьё/топливо/золото), шаг полос 37 px |
| полосы | `CargoBar::Draw`/`CargoBarData::Draw` (`:234-359`) | в секции три полосы 240×30 в (174, y): **добыча** (заливка `BARRAW`/`BARFUEL`/`BARGOLD` на `production·240/capacity` px + `BARTAPE` справа на `(capacity − balance)·240/capacity`), **потребление** (`usage`, подпись `%i (%+i / turn)`), **запас комплекса** (`complex_materials`, подпись `%i`); подпись — `FONT1`, цвет `0xFF`, обводка `0x00`, подложка перекрашена `Color_GenerateIntensityTable1(0, 0, 25, 1, 1)` (`Color_RecolorPixels`) |
| стрелки | `LFARO_OF`/`RTARO_OF` 28×29 в (139, y)/(421, y) | `−1`/`+1` единица (case `1002`/`1001`, `1005`/`1004`, `1008`/`1007`) |
| клик по полосе | `AllocMenu::OnClick*Bar` + `GetHorizontalBarClickValue` (`:592`) | `target = (capacity · (mouse_x − 174) + 120) / 240`; `materials = target − production`, обрезается `production.free_capacity`; `< 0` → `ReduceProduction`, `> 0` → `Optimize` |
| кнопки | `MNUBTN5U/D` 48×40 в (465,438) и `MNUBTN6U/D` 109×40 в (514,438) | `?` → `HelpMenu_Menu("ALLOCATE_SETUP")`, `Done` (`GNW_KB_KEY_RETURN`) и `Esc`/`KP_Enter` — закрыть; **`Cancel` нет** |
| модальность | `GameManager_DisableMainMenu`/`EnableMainMenu` вокруг `AllocMenu::Run` | карта приказов не получает |

**Где хранится значение.** Не в `TeamUnits` и не в процентах: у каждого юнита
`MININGST` три счётчика `uint8_t` — `raw_mining`, `fuel_mining`, `gold_mining`
и их сумма `total_mining` (лимит `16` единиц), плюс максимумы клетки
`raw/fuel/gold_mining_max` (`unitinfo.hpp:434-440`). Диалог меняет их **живо**
(`UpdateRawAllocation`: `production += material; complex->material += material`),
`Done`/`Esc` только закрывают окно. На выходе `AllocMenu::Run` в сети вызывает
`Remote_SendNetPacket_11` (`remote.cpp:2195`), который каждой шахте комплекса
ставит `ORDER_NEW_ALLOCATE` (пакет **08**, процессор **P5** — четыре поля
добычи) и рассылает состояние комплекса.

**Экономика.** `Cargo_GetNetProduction(MININGST)` (`cargo.cpp:105-126`) при
`ORDER_POWER_ON`/`ORDER_NEW_ALLOCATE` даёт `raw += raw_mining`,
`fuel += fuel_mining`, `gold += gold_mining` (у компьютера — `×5/4`/`×6/4` по
сложности). `Access_UpdateResourcesTotal` (`access.cpp:912`) суммирует это по
зданиям комплекса в `Complex::{material,fuel,gold,power,workers}`, а
`GameManager_ManageEconomy` (`game_manager.cpp:2058`) вызывает пересчёт каждый
ход через `TeamUnits::OptimizeComplexes`. Поэтому доля шахты = добыча
комплекса за ход.

### 20.2 Что портировано

* `max_core::alloc` — `AllocMenu_Optimize`/`AllocMenu_ReduceProduction`/
  `AllocMenu::Balance` (`Allocator` с маской `cargo_material_type`),
  `Complex::GetCargoMining`/`GetCargoMinable`/`GetCargoInfo`,
  `Cargo_GetInventory`/`Cargo_GetCargoCapacity`, `Access_UpdateResourcesTotal`,
  `Survey_GetResourcesInArea(…, radius 1, limit 16)` (`mining_survey`).
  Замер клетки — как в C++: сумма 4 клеток, лимит **16** (а не 255).
* `GameState::manage_economy` — `OptimizeComplexes` в объёме экономики:
  `update_complex_resources(team)` → `Access_UpdateResourcesTotal` для каждого
  комплекса команды (раньше в этом месте был TODO, и `Complex::material`
  оставался нулём).
* `GameState::spawn_unit` — `UnitInfo::UnitInfo` (`unitinfo.cpp:316`): здание
  сразу получает **свой** комплекс (`AttachComplex(CreateComplex(team))`,
  `Complex::Grow`); `destroy_unit` — `DetachComplex` (`Complex::Shrink`,
  пустой комплекс удаляется). Без этого у шахты не было `unit->GetComplex()`,
  и `allocate` не мог открыться.
* `max_app::allocmenu` — диалог: раскладка/полосы/стрелки/кнопки, клики,
  клавиши (Rust-расширение: `↑/↓` — секция, `←/→`, `−/+`, `H`/`?`), `Done`/
  `Esc`; `App::open_alloc_menu`/`close_alloc_menu`/`alloc_menu_set`/
  `alloc_menu_adjust`/`queue_alloc_orders`.
* Пункт `allocate` меню команд включён (`UnitMenuItem::enabled = true`).
* Модальность: пока диалог открыт, карта/панели приказов не получают; строка
  статуса партии не рисуется поверх полноэкранного `ALLOCFRM`.
* `Done`/`Esc` кладут в очередь `ORDER_NEW_ALLOCATE` (**08/P5**) на каждую
  шахту комплекса — как `Remote_SendNetPacket_11`; офлайн они применяются сразу
  (`App::flush_orders`), в сети уходят пакетом.
* Детерминизм: значение живёт в полях юнита, а они уже входят в
  `GameState::snapshot_bytes`/CRC (`crc.rs`) и в V70-сейв — отдельного поля в
  снапшоте не появилось, два прогона дают одинаковый CRC.

**Осознанные расхождения.** (1) Коннекторы зданий не моделируются, поэтому
комплекс в Rust = одно здание; в C++ шахта, генератор и плиты слились бы в один
комплекс, и `Total`/`Usage` считались бы по ним всем. (2) `Balance` (вторая
«лента» полосы) вызывается при открытии и после каждого изменения, а не на
каждой перерисовке окна — суммарная добыча от этого не меняется (C++ тоже не
меняет итог, только раскладывает единицы по шахтам). (3) Звук `KCARG0` на
клике стрелки не проигрывается (в headless-стеке звука нет).

### 20.3 Тесты и QA

* `max_core::alloc::tests` (9): биты `CARGO_*` и имена, лимит замера 16,
  `Optimize` до максимума клетки и до 16 единиц, `ReduceProduction` держит
  `total_mining` в синхроне, `Cargo_GetNetProduction` учитывает добычу только у
  включённой шахты, `UpdateResourcesTotal` кладёт добычу в комплекс, смена доли
  меняет доход (сырьё ↔ золото), коэффициент сложности `×5/4`/`×6/4`.
* `max_app::allocmenu::tests` (12): раскладка/подписи из `language.json`,
  условия открытия (только активная шахта с комплексом), `±1` и абсолютный
  уровень, формула `GetHorizontalBarClickValue`, попадание кликов в `?`/`Done`/
  стрелки/полосу, клавиши, формат подписей (`%i (%+i / turn)`), геометрия
  полос, детерминизм двух одинаковых диалогов (включая `state_checksum` и
  `snapshot_bytes`).
* `tests/alloc_menu.rs` (9 кадровых): пункт включён, диалог открывается из меню
  и совпадает с раскладкой C++, значения меняются и переживают закрытие,
  стрелки `±1`, модальность, влияние на доход комплекса на следующем ходу,
  детерминизм двух прогонов, полноэкранный кадр, `ORDER_NEW_ALLOCATE` → P5.
* QA `max-rust-qa/scripts/allocate.qa` (128 кадров): шахта → меню → `allocate`
  → `allocate set`/`row` → `Done` → `ORDER_NEW_ALLOCATE` → тик возвращает
  `POWER_ON` → `end_turn` → `assert complex-raw>=4` → повторное открытие
  помнит значения. Кадры `allocate-selected`, `allocate-menu`, `allocate-set` —
  golden `max-rust-qa/golden/allocate` (0 дрейфа).
* Windows UI (живое окно): `build/scripts/ui-test-allocate.ps1` — меню → партия,
  клик по шахте ×2 (выделение + меню), пункт `allocate` (`alloc=open` в трассе
  `MAX_DEBUG_INPUT`), полноэкранный диалог (зелёный заголовок в кадре окна),
  модальность (клик по карте), `RTARO_OF` (`+1`), `Done` (`alloc=closed`);
  скриншоты `build/ui-test-allocate/*.png`. Сборка клиента —
  `build/scripts/build-windows-ui.ps1 -NoPackage`.
* Существующие golden (visual-screens/production/upgrade/research/cargo/
  move-input/unit-menu/research-help) — **0 дрейфа**.

## 21. Строительство: `stop` отменяет стройку, `start` продолжает — **сделано** (2026-11-18)

### 21.1 Как в C++ (оракул)

| Что | Код | Поведение |
|---|---|---|
| состав попапа строителя | `UnitsManager_Popup_InitBuilders` (`units_manager.cpp:716`) | если `ORDER_BUILD` и состояние не `BUILD_CANCEL`/`BUILD_ABORT` — регистрируется **только** `stop` (`_(bf85)`, позиция `7`); иначе — `Build` (`_(b3ef)`, позиция `1`). Оба пункта вызывают один обработчик `UnitsManager_Popup_OnClick_BuildStop` |
| `Build` | `BuildMenu_Menu(unit)` | экран заказа |
| `stop` | `GameManager_SelectBuildSite` (`game_manager.cpp:3529`) | если юнит в `ORDER_STATE_SELECT_SITE` — снять «ленту» площадки, очистить очередь, вернуть прежний приказ; иначе `move_to = grid` и `UnitsManager_SetNewOrder(unit, ORDER_BUILD, ORDER_STATE_BUILD_CANCEL)` |
| отмена | `UnitInfo::CancelBuilding` (`unitinfo.cpp:4231`) | мобильный строитель: `build_list.Clear()`, `build_time = 0`, путь снят, `ORDER_AWAIT`/`EXECUTING_ORDER`; завод: `HaltBuilding`/`HaltBuilding2` + `EXECUTING_ORDER` (очередь и `build_time` сохранены) |
| `start` завода | `UnitsManager_Popup_OnClick_BuildOrder` → `UnitInfo::BuildOrder` | `ORDER_BUILD` + `ORDER_STATE_INIT` + `StartBuilding` — продолжение остановленной стройки |

### 21.2 Что было сломано у нас

Пункты `stop` (у строителя и завода) и `start` (у завода) были **серыми**
(`.disabled("GameManager_SelectBuildSite не портирован")` /
`("UnitInfo::BuildOrder ... не портирован")`). Сам приказ постройки при этом
работал — но по C++-логике `Build` во время `ORDER_BUILD` из попапа исчезает и
остаётся только `stop`: серый пункт ⇒ у инженера/конструктора после **первого
же** заказа попап навсегда превращался в `stop` + `Done`, и построить что-то
ещё (или отменить заказ) было нельзя. Симптом игрока — «инженер и конструктор
не могут ничего построить».

### 21.3 Что сделано

* `unitmenu.rs`: у строителя и завода `stop`/`start` теперь **включены**
  (состав и подписи — как в C++).
* `App::activate_unit_menu_item`: `stop` → P1 `ORDER_BUILD` +
  `ORDER_STATE_BUILD_CANCEL`; `start` → P1 `ORDER_BUILD` + `ORDER_STATE_INIT`
  (приказ идёт тем же путём пакета 08, что и остальные пункты).
* Ядро уже умело: `cancel_building` (`UnitInfo::CancelBuilding`) и
  `start_building` (`UnitInfo::StartBuilding`) — новых функций не понадобилось.
* Тесты: 2 unit (`unitmenu::tests`), кадровый
  `tests/unit_menu.rs::builder_stop_cancels_the_build_and_build_returns`
  (заказ → `stop` → `AWAIT`, очередь пуста, `Build` вернулся и включён).
* QA `production.qa` дополнен циклом «заказ → `stop` → `AWAIT` → `Build` →
  повторный заказ → постройка доходит до конца»; golden `production/*` —
  **0 дрейфа**.

### 21.4 Что проверено и что осталось

Проверено headless (QA `production.qa` + probe-сценарии, затем полный
docker-гейт): строительство работает у **конструктора** (пункт `Build` из
попапа и клавиша `B`, высадка `(30,22)`, `(23,45)` и позиция по умолчанию,
Turn Based и Simultaneous Moves) и у **инженера**; приказ `ORDER_BUILD`
применяется, здание появляется через `ATTRIB_TURNS` ходов, `stop` его отменяет,
`Build` возвращается.

Осознанные расхождения, которые остаются: (1) площадка постройки выбирается
автоматически (`find_build_site`, радиус 8) — клик по карте площадку не задаёт;
(2) `Alt-P`/звуки попапа не портированы. Стройплощадка (`PrepareConstructionSite`
/ `DeployConstructionSiteUtilities`) портирована отдельным этапом — см. §22.

> **Обновление (2026-11-18, §23).** Пункт (1) закрыт: после `Done` для здания у
> мобильного строителя включается режим выбора места (`ORDER_STATE_SELECT_SITE`),
> призрак-лента `GameManager_TempTape` двигается за курсором, LMB ставит
> площадку в выбранную клетку, Esc/RMB отменяют; после подтверждения строитель
> через `PositionInTape` встаёт в центр ленты. `find_build_site` остался
> запасным путём для ИИ/тестов (`BuildMenu::command` без явной клетки).

## 22. Стройплощадка: площадка стоит на карте до готового здания — **сделано** (2026-11-18)

### 22.1 Как в C++ (оракул)

| Что | Код | Поведение |
|---|---|---|
| площадка | `UnitInfo::StartBuilding` (`unitinfo.cpp:2751`) | мобильный строитель: `DeployConstructionSiteUtilities(unit_to_build)`; конструктор затем `ORDER_AWAIT_TAPE_POSITIONING`, инженер — `ORDER_STATE_BUILD_IN_PROGRESS` + `DrawSpriteFrame(image_index + 16)` |
| типы/раскладка утилит | `UnitInfo::DeployConstructionSiteUtilities` (`unitinfo.cpp:3802`) | здание (`flags & BUILDING`) → `LRGTAPE`+`LRGCONES`, иначе `SMLTAPE`+`SMLCONES`; конусы и лента в клетке `move_to`, угол 0; на `SURFACE_TYPE_WATER|COAST` конусы **не** ставятся, лента получает угол 1; `tape->SetParent(builder)` |
| плита | `UnitInfo::PrepareConstructionSite` (`unitinfo.cpp:4122`) | если `flags & REQUIRES_SLAB`: `LRGSLAB` (здание) / `SMLSLAB`, кадр `Randomizer_Generate(image_count)` (5/1), `scaler_adjust = 4`, `UnitsManager_ScaleUnit(EXPAND)`; заодно убирает с площадки `COMMANDO`/`SUBMARNE`/`CLNTRS` |
| рост | `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856`) | у конструктора `+8` каждые 2 кадра (`image_index + 8`, `>= 40` → `-16`), у инженера рабочая поза постоянна; отдельного кадра «процент постройки» у здания **нет** — прогресс виден только по площадке и позе строителя |
| завершение | `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3520`) + `UnitsManager_BuildingReady` (`units_manager.cpp:4183`) | здание `DeployUnit(GetConstructedUnitType(), …, utility_unit->x/y)`; `ORDER_IDLE`/`BUILDING_READY`; лента/конусы снимаются `Access_DestroyUtilities(..., remove_slabs = false, …)` — **плита остаётся**; здание `ScaleUnit(EXPAND)`, родитель-строитель отвязывается |
| отмена | `UnitInfo::CancelBuilding` (`unitinfo.cpp:4269`) | `Access_DestroyUtilities(grid, true, …)` — лента, конусы и плита удаляются |

### 22.2 Что сделано

* `max_core::production` — `deploy_construction_site` (порт обеих функций):
  большие/малые декали, вода/побережье, родитель ленты, кадр плиты из RNG,
  `scaler_adjust = 4` + `ScaleUnit(EXPAND)`. Заказ ставит площадку синхронно
  (`ProductionManager::commit`), `start_building` (машина приказов, путь P4/ИИ)
  зовёт её же; повторный вызов идемпотентен и не двигает RNG.
* `ProductionManager::spawn_new_unit` — завершение: здание в клетке площадки,
  `scale_unit(EXPAND)`, `access::destroy_utilities(..., remove_slabs = false)`;
  лента/конусы исчезают, плита остаётся. Отмена (`BuildCancel`/`HaltBuilding`)
  убирает всю площадку из `target_grid` (порт не двигает строителя на клетку).
* `max_app::render::unit_frame_index` — рабочая поза строителя
  (`+16`, `ORDER_BUILD`/`BUILD_IN_PROGRESS`) и возврат к базовому кадру после
  достройки.
* `max_app::render::unit_frame_index_for_animation` +
  `App::build_animation_step` — покадровый цикл конструктора
  (`UnitsManager_ProcessOrderBuild`): `index + 8`, а при `index + 8 >= 40` —
  `index - 16`, то есть `+16 → +24 → +32 → +16`. Кадр листается **реальным
  `dt`** (100 мс/кадр, [`max_app::BUILDER_ANIM_FRAME_SECONDS`]; C++ переключает
  на каждом втором тике); `dt = 0` (QA/headless) — кадр начала стройки `+16`,
  поэтому golden-кадры детерминированы. У инженера цикл не листается: он держит
  `+16`, как `StartBuilding`.
* `App::unit_growth_scale`/`App::unit_render_zoom` — «вырастание» постройки
  (`UnitsManager_ProcessOrderAwaitScaling` + `UnitInfo::RenderSprite`): масштаб
  `2^-scaler_adjust` (1/16 … 1) применяется к спрайту, тени и башне через
  уменьшенный `zoom` (в C++ — делитель `Gfx_DecodeSpriteSetup`). Начальный
  масштаб берётся из **симуляционного** `scaler_adjust`; переход к полному
  размеру сглаживается по реальному `dt` (`GROWTH_ANIM_SECONDS = 1 с`),
  `dt = 0` — полный размер.
* `max_app::hud::unit_at_screen` — плита под зданием (`GROUND_COVER`) не
  забирает клик: сначала ищется не-`GROUND_COVER`, как `Access_GetTeamUnit`.
* Отрисовка площадки — общий цикл спрайтов: декали `ORDER_AWAIT` (не
  `ORDER_IDLE`) и рисуются настоящими `LRGTAPE`/`LRGSLAB`/`LRGCONES`.
  `App::draw_units` повторяет `IsRelevant` (`unitinfogroup.cpp:59`) полностью:
  `ORDER_IDLE` не рисуется, а `GROUND_COVER` — не рисуется на `zoom < 8`.

### 22.3 Тесты и QA

* `max_core/tests/production.rs` (6): большая площадка конструктора (типы,
  клетка, углы, родитель, `scaler_adjust`, RNG), малая у инженера без плиты,
  вода/побережье без конусов и с углом ленты 1, завершение (лента/конусы
  удалены, плита и здание на месте, `EXPAND`), отмена (вся площадка удалена),
  детерминизм раскладки и снапшота.
* `max_app/src/render.rs` (unit): цикл `unit_frame_index_for_animation`
  (`+16 → +24 → +32 → +16`, заворот `+8 >= 40` → `-16`, у инженера
  постоянный `+16`) и `scaler_scale` (`2^-n`).
* `max_app/tests/construction_animation.rs` (3, на данных игры): два кадра с
  `dt > 0` различаются кадром спрайта конструктора, а `dt = 0` даёт ту же
  рабочую позу `+16`; плита с `scaler_adjust = 4` в первом кадре `dt > 0`
  меньше полной (`< 1/2` пикселей), через `GROWTH_ANIM_SECONDS` совпадает с
  полной, `dt = 0` — сразу полная; юнит без `scaler_adjust` не масштабируется.
* `max_app/tests/building_frames.rs` (+2): `ENGINEER`/`CONSTRCT` рисуют
  `image_base + angle + 16` в `BUILD_IN_PROGRESS` и базовый кадр после;
  все шесть декалей (`LRG/SML` × `TAPE/SLAB/CONES`) имеют непустой спрайт.
* `max_app/tests/construction_site.rs`: кадр 640×640 до/после заказа —
  площадка добавляет ровно 3 нарисованных юнита и >500 изменённых пикселей;
  **каждая** декаль проверяется отдельно (скрыть → кадр меняется: лента,
  конусы и плита дают свои пиксели); после достройки ленты/конусов нет, кадр
  меняется снова.
* QA `production.qa`: `tick 1` после повторного заказа → `assert site=large`
  (лента+конусы+плита в одной клетке) → `assert site=drawn` (декали реально
  нарисованы: `LargeCones ~564 px`, `LargeTape ~1790 px`, `LargeSlab ~14043 px`)
  → `shot build-site`; затем `anim 60` → `shot build-site-grow` (плита
  стартует с `2^-4`) и `anim 1000` → `shot build-site-grown` (полный размер);
  `tick 6` → `assert unit-order=build` → `shot build-work-a`, `anim 150` →
  `shot build-work-b` (рабочая поза конструктора `+16` против `+24`).
  `stop` → `assert site=none`; после 10 ходов → `assert unit=Training Hall` +
  `assert site=none`. Golden `production/`: 7 кадров (`build-site`,
  `build-site-grow`, `build-site-grown`, `build-work-a`, `build-work-b`,
  `buildmenu`, `buildmenu-factory`) — 0 дрейфа; остальные эталоны тоже
  0 дрейфа (font/golden `research*` не тронуты).
* Windows UI (живое окно): `build/scripts/ui-test-construction.ps1` —
  меню → партия → конструктор найден по трассе `MAX_DEBUG_INPUT` (тип `49`) →
  камера кликом по миникарте → `B` → Training Hall стрелками списка → `Done`;
  в трассе появились `15@` (LRGTAPE), `17@` (LRGSLAB), `19@` (LRGCONES),
  скриншоты `03-construction-site.png` (площадка 2×2 с лентой и плитой) и
  `04-construction-grown.png` (после роста плиты; кадры различаются).
  Сборка клиента — `build/scripts/build-windows-ui.ps1 -NoPackage`.

### 22.4 Осознанные расхождения

* Площадка ставится в `ProductionManager::commit`/`start_building`, а не
  отдельным тиком `TAPE_POSITIONING`; для конструктора плита появляется сразу
  (в C++ — после 4 кадров позиционирования). Типы, клетки, углы и RNG-порядок
  совпадают. У игрока заказ здания теперь идёт через выбор места (§23):
  `BuildMenu::commit` площадку не ставит, её ставит `start_building` на тике
  после LMB — как C++ `PathBuild` → `ORDER_BUILD`/`INIT` → `StartBuilding`.
* «Вырастание»: C++ уменьшает `scaler_adjust` на 1 за тик обработки приказов
  (5 шагов, ~5 кадров), порт берёт **начальный** масштаб `2^-scaler_adjust` из
  состояния и доводит его до 1 по реальному `dt` за `GROWTH_ANIM_SECONDS`
  (1 с) — визуально это то же вырастание, но сглаженное по времени интерфейса.
  `dt = 0` (QA/headless) сразу рисует полный размер, поэтому golden-кадры не
  зависят от числа тиков до `shot`.
* Покадровая анимация конструктора: C++ переключает кадр на каждом втором
  тике (`moved & 1`), порт — по реальному `dt` (10 кадров/с). Последовательность
  кадров и формула заворота (`+8`, при `image_index + 8 >= 40` — `-16`) точные;
  `dt = 0` — рабочий кадр `+16`. Анимация не зависит от настройки `effects`
  (она в порте не моделируется как UI-опция).

## 23. Выбор места стройки (`SELECT_SITE`): призрак, LMB и центр ленты — **сделано** (2026-11-18)

### 23.1 Как в C++ (оракул)

| Что | Код | Поведение |
|---|---|---|
| заказ здания | `MobileBuildMenu::Build` (`buildmenu.cpp:872`) | `Builder_IsBuildable` + `UnitsManager_IssueBuildOrder(unit, &x, &y, type)`: четыре якоря 2×2 вокруг строителя — `(x,y)`, `(x,y-1)`, `(x-1,y-1)`, `(x-1,y)` (все накрывают его клетку); если здание (`base_unit.GetFlags() & BUILDING`) или `event_click_path_build` — `UnitsManager_SetNewOrderInt(unit, ORDER_BUILD, ORDER_STATE_SELECT_SITE)`, `GameManager_TempTape = UnitsManager_SpawnUnit(LRGTAPE или SMLTAPE, team, grid_x, grid_y, unit)` и сообщение `_(06cb)`; иначе — сразу `BuildOrder` в клетке строителя |
| призрак | то же | Временная **лента стройплощадки** (`LRGTAPE`/`SMLTAPE`) — настоящий юнит в мире, родитель — строитель; спавнится в текущем якоре и двигается `UnitsManager_MoveUnit` при движении мыши. Призрака здания, рамки или `BLDMRK*` нет |
| курсор/валидность | `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5829`) | Конструктор: якорь по каждой оси — курсор правее → `builder.x`, левее → `builder.x-1`, ровно на линии → прежний якорь (по `y` так же). Валидно (`CURSOR_MAP2`), если `UnitsManager_MoveUnitAndParent` дал проходимость `UnitsManager_IsAccessible` (2×2, `AccessModifier_SameClassBlocks`, строитель и лента вынуты из хэша) **и** 2×2 накрывает клетку курсора; иначе `CURSOR_UNIT_NO_GO` |
| подтверждение | `GameManager_PathBuild` (`game_manager.cpp:4566`) | LMB по `CURSOR_MAP2`: `move_to = TempTape->grid`, лента уничтожается, `UnitsManager_StartBuild` → `BuildOrder` (`ORDER_BUILD`/`INIT`); на следующем тике `UnitInfo::StartBuilding` ставит `LRGTAPE`+`LRGCONES`+`LRGSLAB` в `move_to` |
| отмена | `GameManager_ManagePlayerAction` → `GameManager_SelectBuildSite` (`game_manager.cpp:3529`) | RMB по карте и LMB по невалидной клетке: лента уничтожается, `BuildList` очищается, `SetOrder(prior)`/`SetOrderState(prior)` — заказа нет |
| центр ленты | `UnitInfo::PositionInTape` (`unitinfo.cpp:4414`), `GetTargetUnitAngle` (`4147`), `Paths_UpdateAngle` (`paths.cpp:258`) | После `StartBuilding` конструктор идёт `ORDER_AWAIT_TAPE_POSITIONING`/`INIT → ENTER`: на каждом тике `GetTargetUnitAngle` даёт диагональ к центру 2×2 (по клетке ленты: обе оси совпали — `SE`, только `x` — `NE`, только `y` — `SW`, иначе `NW`). Пока `moved == 0`, `Paths_UpdateAngle` крутит юнита на месте (до 4 шагов по 45°); как только угол совпал, `moved` 1..4 и на каждом шаге `OffsetDrawZones(DIRECTION_OFFSETS[target_angle] * 8)` — спрайт едет **по направлению своего угла** на 8 px за тик (всего 32 px = половина клетки) в центр 2×2. На завершении: `RestoreOrders` в `ORDER_BUILD`/`BUILD_IN_PROGRESS`, `Paths_UpdateAngle(this, (angle + 1) & 7)` — доворот на 45° под рабочую позу, `DrawSpriteFrame(image_index + 16)`. Сетка строителя (`grid_x/grid_y`) не меняется |

Клавиши: RMB и LMB по невалидной клетке — отмена (как выше); Esc в C++ открывает
меню выхода (`OKCancelMenu`), а не отменяет выбор.

### 23.2 Что сделано

* `max_core::production` — `is_site_accessible[_ignoring]` (порт
  `UnitsManager_IsAccessible`, 2×2/1×1, без требования «клетка пуста»:
  C++ проверяет только `Access_IsAccessible`), `initial_build_site`
  (четыре якоря `IssueBuildOrder` в том же порядке), `site_anchor_for_cursor`
  (привязка к строителю с сохранением оси), `site_covers`.
* `max_core::access::is_accessible_ignoring` — C++ на время проверки вынимает
  строителя из `Hash_MapHash` (`MoveUnitAndParent`/`IssueBuildOrder`); порт
  пропускает его id в цикле, не меняя состояние.
* `max_app::buildsite` — `BuildSiteSelect` (строитель, тип, скорость, якорь,
  курсор), `valid`/`cursor_id`/`status_line`, призрак `draw_ghost` (та же декаль
  `LRGTAPE`/`SMLTAPE`, кадр 0), `tape_center_offset` (8 px × `moved` до 4,
  направление — **угол юнита** `DIRECTION_OFFSETS[unit.angle]`, после
  `RestoreOrders` — полные 32 px по углу въезда с поправкой на C++-доворот
  `+1`), `App::{open_build_site, confirm_build_site, cancel_build_site}`; заказ
  идёт общим `buildmenu::build_order_command` (пакет 08/P4 с `move_to` = якорь).
* `max_core::unit_order::get_target_unit_angle` — точный порт
  `UnitInfo::GetTargetUnitAngle`: для `TAPE_POSITIONING_ENTER` ищет ленту в
  клетке строителя (`production::construction_utility`) и возвращает SE/NE/SW/NW
  по якорю 2×2; иначе — направление к центру `target_grid`. `position_in_tape`
  повторяет C++-цикл `if (moved || !Paths_UpdateAngle(this, target_angle))`:
  доворот без сдвига, затем четыре шага `moved`, на завершении — доворот
  `(angle + 1) & 7` (рабочая поза).
* `BuildMenu::site_selection_target` + `commit_build_menu`: для здания у
  мобильного строителя `Done` открывает режим (статус «choose a site»), при
  отсутствии проходимого якоря меню остаётся открытым с `NoSite` (C++ —
  «Invalid square»); `BuildMenu::command_at` принимает явную клетку, без неё —
  прежний `find_build_site` (ИИ/тесты).
* Ввод: `handle_session_input` → `session_build_site_input` — MouseMove двигает
  призрак (`GetBuilderUnitCursor`), LMB по карте подтверждает (по невалидной —
  отменяет, как C++), RMB/Esc отменяют; клики по панелям уходят обычным путём
  (C++ не выключает главное меню). Курсор окна — `CursorContext::BuildSite`
  (`CURSOR_MAP2`/`CURSOR_UNIT_NO_GO`).
* Кадр: `App::draw_build_site_ghost` (после юнитов, в обеих ветках рендера) и
  сдвиг спрайта строителя в `draw_units` — `tape_center_offset` добавляется к
  мировому якорю до камеры, поэтому масштаб/зум учитываются как у спрайта.
* Режим — UI-состояние (`App::build_site`): в снапшот/CRC/сейв не входит, при
  выходе из партии/загрузке сбрасывается.

### 23.3 Тесты и QA

* `max_app/src/buildsite.rs` (unit, 9): якорь 2×2 всегда накрывает строителя и
  курсор; якорь вдали от строителя **невалиден** (нельзя строить дистанционно);
  начальный якорь — первый кандидат `IssueBuildOrder`; курсор/валидность
  (вода → `CURSOR_UNIT_NO_GO`); `tape_center_offset` = (−32,−32) для правого
  нижнего угла и ровно центр ленты; направление сдвига — **угол юнита**
  (`DIRECTION_OFFSETS[angle]` для всех 8 углов, включая (1,0)*8 при взгляде на
  восток), а не фиксированная диагональ; четыре угла 2×2 дают SE/NE/SW/NW и
  `DIRECTION_OFFSETS[angle] * 32` ровно в центр; `position_in_tape` сначала
  доворачивает (moved = 0, сдвига нет), затем 4 шага по 8 px и `NW→N` на
  завершении; лента, не накрывающая строителя, не даёт сдвига.
* `max_app/tests/build_site.rs` (8): призрак двигается за мышью и меняет
  валидность; призрак **нарисован** в кадре после **обоих** путей открытия
  `BuildMenu` (клавиша `B` и пункт `Build` попапа); Esc отменяет без приказа и
  без площадки; LMB ставит площадку **в указанной клетке** (`site-pos`),
  строитель работает в центре ленты — кадровый тест по bbox спрайта; то же для
  попапа; та же последовательность → тот же digest и та же площадка;
  `builder_turns_to_the_tape_and_slides_diagonally_by_eight_pixels_per_tick` —
  кадры позиционирования: после доворота `angle=7`, bbox спрайта каждый тик
  смещается ровно (−8,−8), после четырёх шагов центр ленты, затем `angle=0` и
  рабочая поза.
* `unit_menu::builder_stop_cancels_the_build_and_build_returns` — заказ через
  подтверждение клетки (здание у конструктора).
* QA `production.qa`: `build done` → `assert build-site=open/valid` →
  `shot build-site-select` → `build cursor 30 21` → `assert build-site=invalid` →
  `shot build-site-invalid` → `key ESC` (отмена) → заказ → `build place 28 20` →
  `tick 1` → `assert site=large` + `assert site-pos=28,20` → покадровое
  позиционирование: `build-slide-turn` (`assert angle=7`,
  `build-center=no`), `build-slide-1..4` (сдвиг −8/−16/−24/−32,
  `build-center=no/yes`) → отмена `stop` → повторный заказ через
  `build place near` → `assert site=drawn` → `assert build-center=yes` →
  `anim`/`shot` `build-work-a`/`build-work-b`. `research.qa`/`research-help.qa`
  добавлен `build place near`.
* Новые команды QA: `build place X Y|near`, `build cursor X Y`; проверки
  `build-site=open|closed|valid|invalid`, `build-center=yes|no`,
  `site-pos=X,Y`, `angle=N` (`max-rust-qa/scripts/README.md`).
* Golden: `production/` — 15 кадров, из них новые `build-slide-turn` +
  `build-slide-1..4` и переснятые `buildmenu`/`buildmenu-factory` (иконки и
  `P_*`), 0 дрейфа; игровые `build-ordered`/`build-site*`/`build-work-*` не
  изменились. `research/` (1 кадр) и `research-help/` (9 кадров) — 0 дрейфа.
* Живой Windows-прогон: `build/scripts/ui-test-construction.ps1` по умолчанию
  проверяет **упакованный** exe (`release/max_app-*-windows-x86_64-ui/max-rust.exe`),
  `-OpenVia key|popup` — оба пути открытия `BuildMenu`. Проверки: `site=open`
  в трассе, якорь накрывает строителя (`valid=1`), до LMB в мире нет `LRGTAPE`
  (15) / `LRGCONES` (19), после LMB площадка накрывает строителя, `05-builder-centered`
  — bbox-проверка: центр спрайта строителя против центра жёлто-чёрной ленты
  (допуск 10 px; на прогонах ≤1.5 px). Кадры — `build/ui-test-construction/`
  (key) и `build/ui-test-construction-popup/` (popup), призрак — `03-site-select`,
  скольжение под углом — `03-slide-a..d`, BuildMenu с иконками/портретом —
  `02-buildmenu`.

### 23.4 Осознанные расхождения

* C++ спавнит временную ленту в мире (`GameManager_TempTape`); порт рисует её
  как UI-призрак — счётчик юнитов, CRC, сейв и ИИ не видят выбор до LMB.
* Esc отменяет выбор (в C++ Esc открывает `OKCancelMenu`); RMB и LMB по
  невалидной клетке отменяют как в C++.
* `GameManager_GetBuilderUnitCursor` для `ENGINEER` (лента 1×1, выравнивание по
  строке/столбцу, линейный путь) не портирован: инженер ставит 1×1-постройки
  сразу в своей клетке, как ветка C++ без `SELECT_SITE`.
* `GameManager_AutoSelectNext` после подтверждения не вызывается (как и в
  остальных приказах порта); звуки `V_M049`/`SFX` в режиме не проигрываются.
* После достройки здания C++ оставляет сдвинутый `OffsetDrawZones`, пока
  строитель не сдвинется с места (`Redraw`/`UpdateGridPosition`); порт
  сбрасывает сдвиг сразу, когда приказ перестаёт быть `ORDER_BUILD`.

### 23.5 Почему пользователь не видел призрак и центр (2026-11-18)

Симптомы пользователя: при выборе места призрака не видно, конструктор стоит не
в центре площадки. В исходниках оба пути уже сходились в один
`App::open_build_menu` → `BuildMenu` → `open_build_site`, а `GetBuilderUnitCursor`
держал якорь 2×2 вокруг строителя. Расхождение было в **артефакте**: прошлый
живой тест запускал `build/win-msvc-target/release/max-rust.exe` (свежая сборка),
а игрок — упакованный `release/max_app-1.1.0-windows-x86_64-ui/max-rust.exe`,
собранный **до** режима `SELECT_SITE`. Старый exe сразу ставил площадку в
автоматически найденную клетку (`find_build_site`, 2 клетки от строителя),
поэтому «призрака» не было, а площадка не накрывала строителя и `PositionInTape`
не центрировал его.

Исправления:

* `ui-test-construction.ps1` выбирает **упакованный** exe (build-target — только
  fallback), поэтому живой тест больше не может «зеленеть» на другой сборке;
* проверяется инвариант якоря и в коде: `BuildSiteSelect::is_valid_at` требует,
  чтобы 2×2 накрывал клетку строителя; `tape_center_offset` не сдвигает спрайт,
  если лента его не накрывает;
* живой тест проверяет оба пути (`key`/`popup`), отсутствие площадки до LMB,
  накрытие строителя после LMB и bbox-центр спрайта;
* упакованный Windows UI пересобран (`build-windows-ui.ps1`), прогоны
  `key`/`popup` на нём — PASS.

### 23.6 Скольжение под углом (2026-11-18)

Симптом: конструктор «въезжал» в центр площадки прямым/вертикальным сдвигом,
глядя не туда, куда ехал. Причина — в порте `PositionInTape` наращивал `moved`
сразу, без `Paths_UpdateAngle`, а `tape_center_offset` считал направление по
разнице клеток, не поворачивая спрайт. C++ (`unitinfo.cpp:4472-4482`) сначала
доворачивает строителя к `GetTargetUnitAngle`, и только потом
`OffsetDrawZones(DIRECTION_OFFSETS[target_angle] * 8)` двигает спрайт по его
углу. Теперь `position_in_tape` повторяет эту последовательность, а
`tape_center_offset` берёт `DIRECTION_OFFSETS[unit.angle]`; на завершении
портирован доворот `(angle + 1) & 7`. Штатно у 2×2-ленты `GetTargetUnitAngle`
даёт диагональ (строитель всегда внутри ленты), поэтому кадрово это (−8,−8) за
тик; функция `tape_center_offset` при этом честно следует любому углу — тест
проверяет и (1,0)*8 для взгляда на восток. Golden: `build-slide-turn` (после
доворота, сдвиг 0), `build-slide-1..4` (−8/−16/−24/−32) — 0 дрейфа.

## 24. `BuildMenu`: иконки, портрет, характеристики и цена — **сделано** (2026-11-18)

### 24.1 Как в C++ (оракул)

| Что | Код | Поведение |
|---|---|---|
| строки списка | `UnitTypeSelector::Draw` (`unittypeselector.cpp:269`), `BuildUnitTypeSelector::Draw` (`buildmenu.cpp:165`) | `ReportStats_DrawListItem(buffer, width, type, 0, i*32, width, 0xA2)`: иконка `I_*` с горячей точкой в (16,16) строки, имя `Text_TextBox(ulx + 35, uly, full - 35, 32)` по центру по вертикали; ходы — `ReportStats_DrawNumber(... + 135, turns, 20)`, т.е. правая граница 618 |
| иконка | `ReportStats_DrawListItemIcon` (`reportstats.cpp`) | `data.divisor = 2` для `STATIONARY` (здания — `1 << 2 = 1:4`), иначе 1 (`1:2`); кадр 0 `GetIcon()` (`I_*`); для `TURRET_SPRITE`/`SPINNING_TURRET` — кадр 1 со сдвигом `angle_offsets[2] >> divisor`; перекраска `color_index_table` команды |
| цвет ходов | `BuildUnitTypeSelector::Draw` | `0xA2`, если строитель `STATIONARY` или `storage >= turns * Cargo_GetRawConsumptionRate(builder, 1)`, иначе `COLOR_RED` |
| выделение | `GameManager_DrawUnitSelector` (`game_manager.cpp`) | белые уголки 8 px в 32×32-ячейке (offsetx=0, `scaled_size2 = 30`); не заливка и не цвет текста |
| портрет | `AbstractBuildMenu::Draw` (`buildmenu.cpp:509-529`) | `WindowManager_LoadBigImage(base_unit.GetPortrait(), &local_window, window.width, false, true, -1, -1, false)`; `local_window.buffer = &window.buffer[11 + width*13]`, big image `P_*` 300×240 в (11,13), палитра окна (`CONBUILD`/`FACBUILD`), **без** обрезки 280×240 (в `CargoMenu` — своё окно `unit_portrait_window` 280×240); при отсутствии — `buf_fill` 300×240 чёрным |
| характеристики | `AbstractBuildMenu::Draw` (`buildmenu.cpp:531-536`), `UnitStats_DrawStats` (`unitstats.cpp:98`) | `stats_background` 11,293,250,174: `UnitStats_DrawStats(buffer, width, unit_type, unit->team, *UnitsManager_GetCurrentUnitValues(team, unit_type), 250, I_RAW, I_RAWE)` — военные строки, `Cargo`, `Prod./Uses`, `Armor/Hits/Scan`, `Speed`, `Cost`; условия по базовым значениям, числа/дельты — по текущим; иконки — `I_*` (pips), `Cost` — `I_GOLD`/`I_GOLDE` |
| ходы/цена | `AbstractBuildMenu::Draw` (`buildmenu.cpp:545-660`), `UnitInfo::GetTurnsToBuild` (`unitinfo.cpp:3393`) | 1×: `turns = GetTurnsToBuild(unit_type)`, `cost = turns * Cargo_GetRawConsumptionRate(builder, 1)`; 2×/4×: `unit->GetTurnsToBuild(unit_type, rate, &turns)` — на каждом ходу `min(rate, BuildMenu_GetMaxPossibleBuildRate(builder, remaining, storage))`, сумма ставок и число ходов; `ATTRIB_TURNS` — **текущие** значения (клановые `CLAN_TRADEOFFS` и исследования). У `TRAINHAL` кнопки 2×/4× выключены, строки не рисуются; у мобильного строителя строки скрыты, если `max_rate < rate` |
| выбор портрета/статистики | `AbstractBuildMenu::Select` / `FactoryBuildMenu::Select` | `Draw(selector->GetLast())` — выбранная строка списка; у завода — активного селектора (список или очередь `cargo_selector`) |

### 24.2 Что сделано

* `max_app::buildmenu` переиспользует `CargoMenu`-паттерн:
  `mobilize::{draw_icon, team_lut, center_y, draw_text_right}` (`draw_icon`
  рисует кадр 0 + кадр башни со сдвигом и LUT команды — ровно
  `ReportStats_DrawListItemIcon`), `CargoMenuAssets::icons` (71 `I_*`) и
  `CargoMenuAssets::portrait` (ленивый кэш `P_*` 300×240).
* Строка списка: иконка 1:4 у зданий в (483+16, 60+i*32+16), имя с x=518
  (ширина 80, по центру строки), ходы справа (`LIST_TURNS_RIGHT = 618`,
  `0xA2`/`COLOR_RED`), белые уголки на выбранной строке. Очередь завода — те же
  иконка+имя и уголки (`UnitTypeSelector::Draw`).
* Портрет — `P_*` 300×240 в (11,13) (`PORTRAIT`), палитра `CONBUILD`/`FACBUILD`,
  без обрезки; тип — выбранная строка (у завода — активный список/очередь).
  Спрайт строителя 2×, который рисовался раньше, убран.
* Характеристики — `Mobilization::stats_rows_for(&base, &current, unit_type)` +
  `mobilize::draw_stats` в `STATS` (11,293,250,174): ровно
  `UnitStats_DrawStats` с иконками `I_*` и строкой `Cost`
  (`I_GOLD`/`I_GOLDE`). Это тот же код, что рисует `CargoMenu`; база/текущие
  значения берутся из `state.team_units[team]`, поэтому клановые модификаторы
  и исследования уже учтены.
* `Turns`/`Cost` — `max_core::production::turns_and_consumption` (точный порт
  `UnitInfo::GetTurnsToBuild`): 1× — ходы и `turns * rate1`, 2×/4× — сумма
  ставок с падением скорости при нехватке склада; `build_time` активной
  постройки используется как остаток. `BuildMenu::max_rate` для `TRAINHAL`
  = 1, недоступные строки 2×/4× не рисуются.
* `charge_units` теперь уменьшает `storage` мобильного строителя на ставку
  (C++ `UpdateProduction`), поэтому реальные ходы/цена совпадают с колонками:
  пример из QA — 40 сырья, MININGST 2×: 4 хода по 8 + 4 хода по 2 = (8, 40).

### 24.3 Тесты и QA

* `max_app/tests/build_menu_render.rs` (5): иконка в каждой видимой строке
  (diff от чистого `CONBUILD` > 20 px, в т.ч. выбранная строка с уголками);
  RED/GREEN перекраска иконок (diff > 50 px — командные слоты как в
  `CargoMenu`); портрет попиксельно совпадает с `P_MINING` (>80 % пикселей,
  пропущенных через палитру окна) и отличается от фона; смена выбранной
  строки/выбранного элемента очереди меняет портрет; таблица
  `STATS` (11,293,250,174) нарисована, содержит `Hits`/`Armor`/`Cost`,
  `Cost = 2 × 10 = 20` с `I_GOLD`/`I_GOLDE`.
* `max_app::buildmenu` unit (10, +5): `rate_numbers` = `(4,12)/(2,24)/(1,36)`
  для HVP→TANK; `TRAINHAL` `max_rate=1`; CrimsonPath сокращает SHIPYARD
  20→15 ходов и цену 40→30; цифры — активного селектора завода (выбранная очередь/список), а не всегда головы очереди; `stats_rows_for` для здания — `Hits`/`Armor`/
  `Cost`, иконки `I_GOLD`/`I_GOLDE`.
* `max_core/tests/production.rs` (22, +2): `turns_and_consumption` для
  1×/2×/4×, остаток `build_time`, Training Hall, мобильный строитель с пустым
  складом; клановый модификатор SHIPYARD; реальный прогон 2× с 40 сырья —
  8 ходов, 40 золота, `storage=0`.
* QA `production.qa`: `shot buildmenu` (конструктор, `Mining Station` — иконки
  всех 15 строк, `P_MINING`, характеристики, 12/24, 8/40) и
  `shot buildmenu-factory` (завод, `Infiltrator` — `P_COMMAN`, характеристики
  6/1/4/12/2/8/4/5/9, 9/9, 2×/4× пусты); golden
  `production/buildmenu*.png` пересняты, 0 дрейфа. `research/`/`research-help/`
  — 0 дрейфа.

### 24.4 Осознанные расхождения

* Описание (`BLDDES`) по умолчанию выключено (`show_description=false`), в
  C++ статический `button_description_rest_state` = `true`; оверлей описания
  поверх портрета реализован (`mobilize::draw_description`, 16,17,290×230).
* Недоступные кнопки 2×/4× рисуются обычной картинкой (`BLD2X_U`/`BLD4X_U`):
  отдельного disabled-ресурса (`BLD2X_X`/`BLD4X_X`) в `BuildMenuAssets` нет;
  числа/строки при этом скрыты, клик/клавиша не срабатывают.
* Экономика — золото вместо сырья комплекса (см. `max_core::production`):
  дополнительно склад мобильного строителя уменьшается, как в C++, чтобы
  скорость и ходы совпадали; C++-ветка «компьютер и `opponent >= MASTER`»
  (`UpdateProduction` не списывает склад) не моделируется.
* Фон и кнопки — `CONBUILD`/`FACBUILD`; `Help`/`Build path` инженера не
  портированы (как и раньше).

---

## 25. Покадровая анимация зданий и строителей — **сделано** (2026-11-18)

Разбор жалобы «работающие шахты, заводы и строители выглядят статично».
Оракул `max-cpp-port` не менялся; ниже — что в нём действительно есть.

### 25.1 Что C++ анимирует по времени

| Механизм | Оракул | Формула / интервал | Условие |
|---|---|---|---|
| `ANIMATED`-здание (`RADAR`) | `UnitsManager_ProcessOrderSentry` (`units_manager.cpp:3029-3041`), вызов из `UnitsManager_ProcessOrder` (`3323`) | каждый тик: `if (image_index + 1 <= image_index_max) image_index += 1; else image_index = image_base;` — один кадр на тик `UnitsManager_ProcessOrders` (~30 Гц), блок `image_base .. image_base + image_count - 1` | `flags & ANIMATED` **и** `settings["effects"]` (по умолчанию 1), приказ `ORDER_SENTRY` |
| конструктор | `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856-2868`) | на каждом **втором** тике (`moved & 1`): `+8`; если `image_index + 8 >= 40`, вместо этого `-16`. Старт — `DrawSpriteFrame(image_index + 16)` из `StartBuilding` (`unitinfo.cpp:2789` / `PositionInTape` `4451`), цикл `+16 → +24 → +32 → +16` | `CONSTRCT`, `ORDER_BUILD` / `ORDER_STATE_BUILD_IN_PROGRESS`, `effects` |
| инженер | `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`) | постоянная рабочая поза `image_index + 16`; цикла нет | `ENGINEER`, `BUILD_IN_PROGRESS` |
| пехота/коммандос в движении | `UnitInfo::Move` (`unitinfo.cpp:1826-1833`) | `+8` за тик, затем `image_base + angle` | `COMMANDO`/`INFANTRY` |

Ключевой вывод: **у шахт и заводов покадровой анимации в C++ нет**. В
`MININGST` 24 кадра ленты = 16 кадров корпуса (8 клановых пар `выкл`/`вкл`:
0/1, 2/3, … 14/15) + 8 мелких кадров частиц бурения (16…23); `D_MINING.image_count
= 16`. У `HANGAR`/`DEPOT`/`DOCK`/`TRAINHAL` — 2 кадра состояния + 8 коннекторов
(`D_LRGBLD.image_count = 2`). `ProcessOrderPowerOn` (`units_manager.cpp:2945`)
один раз рисует `image_base + 1`, `PowerDown` — `image_base`; `UnitInfo::Animate`
(`unitinfo.cpp:5056`) только добавляет draw bounds для shake-эффектов.
`ANIMATED` из 93 типов есть только у `RADAR`, взрывов и ракетного дыма.

При этом C++ определяет **блок анимации** любого юнита:
`image_index_max = image_count + image_index - 1`, где `image_index = image_base
+ angle` (`unitinfo.cpp:293`). Именно этот блок и листает промежуточное правило
порта для работающих шахт/заводов (§25.2).

### 25.2 Что портировано

* **`RADAR`** — точный потиковый цикл `ProcessOrderSentry`: 16 кадров
  (`D_RADAR.image_count`, а не 20 кадров спрайта — последние 4 коннекторы).
  Раньше Rust-рендер вообще не читал `image_index` и всегда рисовал
  `image_base + angle`, поэтому анимация терялась.
* **Полный цикл кадров работающего завода** — **ПРОМЕЖУТОЧНОЕ правило**
  (до съёмки работающего завода). Прежний «пульс» двух кадров состояния
  (`activity_step`, 0.5 с) **заменён**: пока завод работает
  (`ORDER_BUILD` + непустая `build_list`), кадр листается по блоку C++
  `image_base + angle .. image_index_max` по кругу со скоростью `RADAR` —
  `render::WORKING_BUILDING_ANIM_FRAME_SECONDS = 1/30` с на кадр, как
  `ProcessOrderSentry`. **Шахта этот цикл больше не использует**: DOS-съёмка
  показала статичный корпус в idle, см. §26.
  * `render::working_building_frame_block(image_base, image_count,
    sentry_frames, angle)` считает `(start, len)`: `image_count` здесь —
    `D_*.image_count` (16 у `MININGST`, 2 у `D_LRGBLD`), а длина ленты —
    отдельный аргумент. Блок никогда не выходит за кадры корпуса
    `D_*` (у `MININGST` кадры 16…23 — частицы, у заводов 2…9 — коннекторы),
    поэтому у старших кланов он укорачивается (клан 8: 14…15).
  * `render::FrameAnimation::work_step` — номер кадра цикла; шаг `0` —
    ровно статичный кадр C++ (`POWER_ON` → `image_base + 1`, завод →
    `image_base`), поэтому `dt = 0` (QA/golden) не двигает эталоны.
  * Ручки «одна строка»: `WORKING_BUILDING_ANIM_FRAME_SECONDS` (скорость) и
    `WORKING_BUILDING_ANIMATION_ENABLED` (выключение → прежний статичный кадр
    C++), обе — `max_app::render`, реэкспорт в корне `max_app`.
* **Строитель** — прежний цикл `+8`/`-16` (`BUILDER_ANIM_FRAME_SECONDS`),
  инженер держит `+16`. Он не дублируется циклом зданий и не сбрасывается при
  появлении других анимаций.
* Кадровые часы живут в `App::frame_anims` (`HashMap<UnitId, FrameAnim>`),
  обновляются в `App::update_unit_animations(dt)`; `dt` клампится до 0.1 с,
  `dt = 0` очищает часы. `snapshot_bytes`/CRC/сейв не содержат ни одного поля
  часов; `tests/building_animation.rs::animation_clock_does_not_change_the_snapshot`
  это фиксирует.
* `settings['effects']` в Rust-порте — симуляционное поле и по умолчанию
  `false`; визуальные часы считают эффекты включёнными (как дефолт оригинала
  `effects=1`), чтобы анимация шла и в QA/headless.
* `dt = 0` (QA/headless) даёт ровно статичный кадр C++: включённая шахта —
  `+1`, завод с очередью — базовый кадр, `RADAR` — `image_base`, строитель —
  `+16`. Поэтому golden не дрейфуют без пересъёмки.

### 25.3 Тесты и QA

* `max_app/tests/building_frames.rs` (8): включённая шахта листает блок
  `D_MINING` (16 кадров): шаги 1…14 — следующие кадры, 15-й — первый кадр
  блока, 16-й — снова статичный кадр C++; выключенная шахта статична,
  `effects=false` — статичный кадр; завод с очередью листает блок из двух
  кадров, без очереди статичен; `RADAR` циклит ровно 16 кадров `D_RADAR`;
  строитель держит `+16` / цикл `+24`/`+32`.
* `max_app/tests/building_animation.rs` (4, на данных игры): один тик
  `1/30` с двигает кадр включённой шахты, второй — ещё один, полный оборот
  (16 кадров) и `dt = 0` возвращают статичный кадр; завод с очередью
  листается, без очереди — нет; `RADAR` меняет кадр за 1/30 с;
  `snapshot_bytes` до/после `dt` идентичны.
* `max_app/tests/construction_animation.rs` (3) — прежний цикл конструктора
  и «вырастание» постройки остаются зелёными (строитель не затронут).
* QA `production.qa`: `factory-work-a`/`factory-work-b` (кадры состояния
  завода, второй — через один кадр цикла), `build-work-a/b` (строитель,
  без изменений); QA `allocate.qa`: `mine-work-a`/`mine-work-b`.
  `qa_golden_check` — **0 дрейфа** на всех эталонах: правки задевают только
  работающие здания, а их вклад ниже порога 1 % (максимум 0.664 % у
  `factory-work-b`), поэтому пересъёмка golden **не потребовалась**.
* Артефакты: headless-кадры — `max-rust/qa-out/production/`,
  `max-rust/qa-out/allocate/`; Windows UI
  `max_app-1.1.0-windows-x86_64-ui` пересобран (см. §25.5).

### 25.4 Осознанные расхождения и открытые вопросы

* C++ не анимирует шахты/заводы по времени; полный цикл — решение порта по
  запросу пользователя. Он использует только реальные кадры ленты (никаких
  выдуманных спрайтов), выключен при `dt = 0` и не влияет на
  симуляцию/CRC/сейв.
* **Открытый вопрос (решён DOS-съёмкой, §26):** у `MININGST` блок
  `D_MINING.image_count = 16` — это 8 *клановых* пар `выкл`/`вкл` (кадры 0/1,
  2/3, … 14/15; логотипы кланов), а не покадровая анимация одной шахты; за ними
  идут 8 кадров частиц бурения (16…23). Съёмка показала, что в idle корпус
  статичен (кадр 0, включённая — `+1`), а кадры 16…23 — **коннекторы
  комплекса**, которые рисуются по маске `connectors` (не «всегда 8 частиц»).
  Цикл шахты отменён, кадры дрели 1..15 выключены константой
  `render::MINING_DRILL_FRAMES_ENABLED` до съёмки активной добычи.
* Визуальные часы идут по `dt`, а не по номеру симуляционного тика: в
  интерактиве `local_update` тикает каждый кадр, поэтому темп совпадает с
  C++ (~30 Гц для `ProcessOrderSentry`), а QA-команда `anim` может крутить
  только кадр, не трогая состояние партии.

### 25.5 Артефакт Windows UI (пересобран)

`build/scripts/build-windows-ui.ps1` (MSVC + SDL3 из vcpkg, feature
`sdl-vcpkg,sound`), упаковка `release/max_app-1.1.0-windows-x86_64-ui`:

* `max-rust.exe` — 6 441 472 Б, sha256 `91d10f26284efe47e1589690be3872c9cbafa2b4dfd149d4ad425791679bbe88`
  (PE32+ x64, Subsystem=2, `--help` отработал);
* zip — sha256 `0da39fcddae96727664d50cc7091662d58b5cf0d7865ec94fdc035ee30621731`;
* рядом `SDL3.dll`, `README.txt`, `LICENSE`, `CHANGELOG.md`, `SHA256SUMS`.


---

## 26. Палитровые циклы, статичный корпус шахты и коннекторы — **сделано** (2026-11-19)

По результатам DOS-съёмки (`max-original-dasm/formats/unit-animation.md` §7)
переделана анимация зданий: кадры корпуса больше не листаются, мигание маяков
делают **палитровые циклы**, а кадры «частиц» шахты и «дыма» `POWGEN` —
**коннекторы комплекса**.

### 26.1 Что показала съёмка (и что уточнено при переносе)

| Вопрос | Ответ |
|---|---|
| Маяки шахты | Кадр `image_base + 1` содержит 4 пикселя в слотах **13–16**, ещё несколько в **21–24**. Палитра окна в игре — `FRAMEPIC` (слоты 0–63/160–255) + `.WRL` (64–159); `FRAMEPIC[13..16]` = жёлтый/оранжевый/красный/оранжевый. `World::InitColorCycles` вращает 13–16 @6 fps → фаза 166.7 мс, полный цикл 666.7 мс. Индикатор дрели — слоты 21–24, тот же 6 fps (тёмный/оранжевый/чёрный; в AVI в этой зоне видна красная вспышка) |
| Корпус шахты | Статичен: выключенная — `image_base`, включённая — `+1`; за ~118 с ни одного перехода `0 → 1 → … → 15` (§7.2) |
| «8 частиц» шахты | Кадры **16…23** = `D_MINING.connector_image_base = 16`, `connector_image_count = 8`. Это коннекторы: рисуются `UnitInfo::RenderWithConnectors` по маске `UnitInfo::connectors`. В плотной базе у шахты выставлены все 8 битов — отсюда «все 8 одновременно» (§7.1). Горячие точки кадров точно соответствуют сторонам: `(35,63)` — север-лево, `(-43,35)` — восток-верх и т.д. |
| «4 дымовых клуба» `POWGEN` | Кадры **2…5** = `D_SMLBLD.connector_image_base = 2`, `connector_image_count = 4` (трубы N/E/S/W). **Уточнение:** «дым» в §7.3 — это **рамка выделения инженера** (`proof/powgen_smoke.png` — два кропа с `Mk I Engineer 1` и уголками выделения), а не дым; настоящий `POWGEN` в захвате f4 стоит в (276,308) и его трубы не анимируются. Анимации дыма в данных игры нет |
| Кадры дрели 1..15 | В idle не играют; включаются константой `render::MINING_DRILL_FRAMES_ENABLED = false` (нужна съёмка активной добычи/обработки хода) |

### 26.2 Что портировано

* **`max_render::palette::{ColorCycle, COLOR_CYCLES, ColorCycles, rotate_palette,
  single_slot_value}`** — дословный порт `World::InitColorCycles`
  (`world.cpp:542`) и `World::UpdateColorAnimations` (`world.cpp:190`):
  11 диапазонов (9–12 @9, 13–16 @6, 17–20 @9, 21–24 @6, 25–30 @2, 31 @6,
  96–102 @8, 103–109 @8, 110–116 @10, 117–122 @6, 123–127 @6), вращение
  влево/вправо по `rotate_direction`, специальный одиночный слот 31
  (зелёный канал `63 → 31 → 15 → 7 → 63`). Часы копят `dt` на цикл и делают
  столько шагов, сколько периодов уложилось (тот же темп, без дрейфа
  округления).
* **Применение — только в рендере.** `App::color_cycles`/`App::cycle_palette`
  живут вне снапшота/CRC; `App::update_color_cycles(dt)` вызывается из
  `App::update_visual_units`, `App::animated_palette()` отдаёт снимок
  отрисовке юнитов. `dt = 0` (QA/headless) сбрасывает часы — палитра исходная,
  golden не дрейфуют.
* **Системная палитра спрайтов** (`Assets::system_palette`): слоты 0–63 и
  160–255 — `FRAMEPIC`, 64–159 — `.WRL` (компоненты `.WRL` усечены до 6 бит,
  как `palette[i] / 4` в `World::ApplyPaletteAndTints`). Раньше спрайты
  рисовались `COLOR.PAL`, где слоты 9–31 — маджента-заглушки: маяки были
  невидимы, а пиксели в этих слотах давали маджента-артефакты (видно в старых
  golden `POWGEN`). `COLOR.PAL` остался источником таблиц яркости.
* **Коннекторы** (`render::{CONNECTOR_FRAMES, connector_frame_indices,
  connector_mask_for, drawn_connector_mask}`): точный порядок
  `UnitInfo::RenderWithConnectors` (N_L, N_R, S_L, S_R, E_T, E_B, W_T, W_B;
  смещения 0, 4, 2, 6, 1, 5, 3, 7), тень коннектора из того же `S_*`, якорь —
  как у корпуса (горячая точка кадра задаёт сторону). Маска —
  `unit.connectors` (симуляционное поле, V70/CRC) ∪ вычисленная для кадра
  копия `UnitsManager_UpdateConnectors` (`units_manager.cpp:2051`), пока
  `max_core::unit_order::update_connectors` — заглушка. В состоянии партии
  ничего не пишется.
* **Шахта**: `working_building_animation_active` для `MININGST` выключен,
  корпус — статичный кадр состояния; кадры дрели 1..15 — константа
  `render::MINING_DRILL_FRAMES_ENABLED` (false). Цикл остался только у завода
  с очередью (`WORKING_BUILDING_ANIMATION_ENABLED`).
* `UnitSpriteStats::connectors` — диагностика: сколько пикселей коннекторов
  нарисовано.

### 26.3 Тесты и QA

* `max_render::palette` +7 тестов: таблица `COLOR_CYCLES` (порядок/направления/
  111/166/500/125/100 мс), `dt = 0` не двигает палитру, фаза 6 fps = +1 слот
  за 166 мс, полный цикл 4 шага возвращает слоты 13–16, у каждого цикла свой
  темп и направление (9 fps влево), слот 31 (63→31→15→7), `from_rgb8`.
* `tests/building_animation.rs` (5): маяки шахты меняют цвет от `dt` и не
  меняются при `dt = 0`, корпус статичен при любом `dt`, `dt = 0` возвращает
  кадр побайтово, коннекторы по маске соседей (шахта/POWGEN) и их
  отсутствие у одиночного здания, снапшот не меняется.
* `tests/building_frames.rs` (9): шахта статична (все `work_step` → `+1`/0,
  клановый сдвиг), 8 коннекторов `D_MINING` в порядке C++, 4 трубы
  `D_SMLBLD`, завод с очередью по-прежнему листает 2 кадра, `RADAR` 16.
* QA: все 14 скриптов `qa_run` — PASS; `qa_golden_check` — **0 дрейфа после
  пересъёмки**: 35 кадров в `allocate` (3), `production` (15), `move-input`
  (6), `unit-menu` (4), `upgrade`/`research` (2), `research-help` (5)
  обновлены — изменение 0.21–0.42 % пикселей (маяки + трубы `POWGEN` +
  исправление палитры слотов 2–31), ниже порога 1 %, но переснято осознанно.
  Docker-гейт `bash ci.sh` — **CI OK за 871 с**, все 4 стадии PASS; в контейнере
  `qa_golden_check` даёт **0 пикселей** на всех наборах (пересъёмка
  воспроизводится на Linux побайтово), soak `soak-100`/`soak_headless` (120
  ходов) — PASS, RSS стабилен (+156 КиБ).
* Артефакты: headless-кадры — `max-rust/qa-out/allocate/mine-work-a.png`,
  `qa-out/production/factory-work-a.png`; Windows UI — см. §26.4.

### 26.4 Артефакты

* **Headless, фазы палитрового цикла:** `cargo run -q -p max_app
  --no-default-features --example building_cycles -- qa-out/building-cycles`
  сохраняет 5 PNG 640×480: `mine-phase-0-static.png` (`dt = 0`, статичный
  кадр) и `mine-phase-1..4.png` (шаги 6 fps). Каждая фаза меняет 251 пиксель
  (маяки шахты 13–16 + индикатор дрели 21–24 + огни `POWGEN`), 4-я фаза
  побайтово равна статичному кадру, `dt = 0` — тоже. Сцена намеренно ставит
  `POWGEN` вплотную к шахте: видны трубы-коннекторы `D_MINING`/`D_SMLBLD`.
* **Headless, QA:** `max-rust/qa-out/allocate/mine-work-a.png` — включённая
  шахта: 4 маяка по углам и корпус `image_base + 1`;
  `qa-out/allocate/allocate-selected.png` — `POWGEN` с трубами.
* **Windows UI:** `build/scripts/build-windows-ui.ps1` (MSVC + SDL3 из vcpkg) →
  `release/max_app-1.1.0-windows-x86_64-ui`: `max-rust.exe` — 6 442 496 Б,
  sha256 `10e9e30e75e943d00a66aa26515d2dbbf0f77c76c49b23bbd611b4684893defc`
  (PE32+ x64, Subsystem=2, `--help` — OK); zip —
  sha256 `a85555e2f6ba17d6f6c1a943a1f6306dcecfd33335b708662d42201cac0487b6`.
* **Windows headless** (`build/scripts/build-windows.sh --no-wine`, mingw-w64
  x86_64-pc-windows-gnu, `--no-default-features`): `max-rust.exe` — 10 702 092 Б,
  sha256 `5f6f89f73cc056d2099cd5288a6659b37c06151eb29cf4af24845bdf1dbac01b`
  (PE32+ console, Subsystem=3, без mingw-рантайма); zip —
  sha256 `855ea275ef4f38d109be73fb3a63190b7e0656dc922008638ee9e4ab89066038`.
  Сводные хеши — `release/SHA256SUMS-1.1.0.txt`.

## 27. Диалог Preferences/Setup (`OptionsMenu`) — **сделано** (2026-11-19)

**Оракул.** `max-cpp-port/src/menu.cpp:2325` `menu_preferences_window(team)` →
`OptionsMenu(team, PREFSPIC).Run()` (кнопка `Preferences` панели,
`GameManager_MenuClickPreferencesButton`, `game_manager.cpp:3918`);
`menu.cpp:2331` `menu_setup_window()` → `OptionsMenu(PLAYER_TEAM_RED, SETUPPIC)`
(пункт `Setup` главного меню). Раскладка и поведение — `optionsmenu.hpp/.cpp`,
значения по умолчанию — `settings.cpp` (секции `SETUP`/`PREFERENCES`/`OPTIONS`),
`IniPreferences` — `save.md` и `saveload.cpp:547/920`.

**Окно.** Обе версии рисуются одной панелью `PREFSPIC` 400×422; `Window(id)` в
C++ центрирует её: `ulx = (640−400)/2 = 120`, `uly = (480−422)/2 = 29`.
Заголовок — `GNW_TEXT_FONT_5` в (108,12) 184×17 зелёным (`COLOR_GREEN`), текст
`Setup` (`_(7013)`) или `Preferences` (`_(f70f)`). Кнопки в нижней строке:
`Done` — `PRFDNE_U/D` 62×23 в (215,383), `Cancel` — `PRFCAN_U/D` в (125,383),
`Help` — `PRFHLP_U/D` 25×23 в (188,383); подписи — `FONT5`,
`Fonts_GoldColor (165,175,199)` (в конструкторе `Text_SetFont(FONT5)` стоит до
создания кнопок). В игре диалог модален (`OptionsMenu::Run` блокирует цикл),
поэтому `App::local_update` не тикает, а карта/панели ввод не получают.

**Раскладка** — дословно `OptionsMenu::Init`: `window_uly` стартует с 20,
секция даёт `+20`, каждая опция с `ulx = 25` — `+20`, `LABEL` после отрисовки
отнимает `20 − (Text_GetHeight() + 3) = 7` (`FONT5` — 10 px); скрытые контролы
строку не занимают. `SETUPPIC` выходит на секции `preferences`, поэтому в меню
только setup-часть; `PREFSPIC` показывает setup + preferences + options
(debug-секция `invalid` не портирована). Контролы: подпись `Text_TextBox`
(`ulx`, `uly`, 185×20), жёлоб `PRFSLIT` 57×3 в
`x = ulx + Text_GetWidth(caption) + 10`, `y = uly + 8`, ползунок `PRFSLIDE`
14×17 в `x = track + value·57/max − 7`; чекбокс `UNCHKED`/`CHECKED` 18×17 в
`(ulx, uly)`, подпись в `(ulx+25, uly−2) 155×24`; поле `PREFEDIT` 60×19
(`PREFNAME` 187×17 для имени) в `(ulx + width(caption) + 10, uly + (20−h)/2)`,
значение по центру цветом `0xA2`.

**Опции (все из оригинала).** `setup`: `Volume:`, `Enhanced graphics
(requires 16MB)` (только Setup), `Music`/`Disable Music`,
`FX`/`FX Disabled`, `Voice`/`Voice Disabled`, `Video`/`Disable Video`,
`Auto-Save Enabled`, `Host Address:`/`Host Port:` (только Setup),
`Player Name:`. `preferences`: `Animate Effects`, `Click to Scroll`,
`Double Unit Steps`, `Track Selected Unit`,
`Halt movement when enemy is detected`, `Auto-Select Next Unit`,
`Scroll Speed`. `options`: `Turn Time:`/`End Turn Time:` (edit int),
`Play Mode: %s`, `Computer Player(s): %s`, `Game ends at %i %s.` (read-only).
Подписи — `language.json` (en-US), значения по умолчанию — `settings.cpp`.

**Что реально действует.**
* `effects` («Animate Effects») — `App::update_unit_animations` не листает
  кадры строителя, `ANIMATED`-зданий (`RADAR`) и работающих заводов;
  `render::unit_frame_index_with_animation` получает `effects = false`, кадр
  статичен — как `units_manager.cpp:2857/3030` и `unitinfo.cpp:5057`. Палитровые
  циклы (маяки) в C++ не гейтятся `effects` и продолжают идти (проверено
  `World::UpdateColorAnimations`, `game_manager.cpp:5161`).
* `disable_fx` («FX Disabled») — звук SFX вкл/выкл: `AudioPlayer::set_enabled`
  (в C++ `HaltSfxPlayback`); `fx_sound_level` — громкость
  (`AudioPlayer::set_volume`, `AUDIO_TYPE_SFX2`).
* `auto_save` — `App::autosave` (слот 10).
**Что только хранится** (честно помечено, в игру не идёт): `click_scroll`,
`quick_scroll`, `fast_movement`, `follow_unit`, `auto_select`, `enemy_halt`
(управление камерой/выделением ещё не портировано), `disable_music`,
`disable_voice`, `disable_movie` и уровни Music/Voice/Video (музыки/голоса/видео
в звуковом слое нет), `enhanced_graphics` (рендер всегда «enhanced»),
`host_address`/`host_port` (сеть задаётся CLI), `Turn Time`/`End Turn Time`
(таймеры берутся с экрана настройки), `Player Name` (в C++ `PREFSPIC` правит
имя команды; у нас — общее имя игрока для следующей партии).

**Сохранение.** `IniPreferences` (7 × `signed int`: `effects`, `click_scroll`,
`quick_scroll`, `fast_movement`, `follow_unit`, `auto_select`, `enemy_halt`)
пишется в `SaveMeta::ini_preferences` при создании партии
(`save_meta_for_local_game_with`) и при каждом сохранении/автосейве
(`App::sync_save_meta`, как `SaveLoad_Save`); при загрузке V70 применяется
обратно (`SaveLoad_Load`, `saveload.cpp:920-934`). Полный набор — в
`preferences.ini` рядом с сейвами (`<save_dir>/preferences.ini`, аналог
`settings.json` в `FilePathGamePref`); интерактивный клиент читает его на
старте, QA — нет (детерминизм).

**Отличие от C++ в `Cancel`.** `OptionsMenu::ProcessKeyPress` при `Esc`
откатывает только громкости и `disable_music/fx/voice/movie` (`last_rest_state`),
а остальные чекбоксы уже записаны в `settings` и остаются до перезапуска (но не
сохраняются, пока не нажат `Done`). Здесь `Cancel` возвращает **весь** снимок
открытия — предсказуемее для игрока и совпадает с тем, что видит пользователь;
`Done` — единственный путь записи `preferences.ini`/`IniPreferences`.

**Проверка.** QA `preferences.qa`: `menu setup` → `SETUPPIC`, `prefs
open/toggle/set/done/cancel/help`, `assert prefs=…`, `prefs-value NAME=N`,
`prefs-effects/sound/audio`, `save_v70`; golden `golden/preferences`
(`main-menu`, `setup-dialog`, `prefs-dialog`, `prefs-effects-off`) — 0 дрейфа.
Unit/integration: `max_app::preferences` (9 тестов: раскладка `PREFSPIC`/
`SETUPPIC`, клики/клавиатура, `IniPreferences`, файл) и
`tests/preferences.rs` (6: открытие из меню и из игры, модальность,
`effects=off` → статичные кадры `RADAR`, флаг звука → `AudioPlayer`,
`IniPreferences` в V70 round-trip, Cancel).

**Артефакты (2026-11-19).**
* **Windows UI:** `release/max_app-1.1.0-windows-x86_64-ui.zip` —
  sha256 `42f7be834946cf9dbecfea8e7f5557db4d402936d930f56debbfe664cfb68f15`;
  `max-rust.exe` 6 597 120 Б, sha256
  `edcda45d00d0a99093e2367eeddf630d135eff046e9a2865748aeb3845caef57`
  (PE32+ x64, Subsystem=2, `--help` OK).
* **Windows headless:** `release/max_app-1.1.0-windows-x86_64.zip` —
  sha256 `bee54053dbc1230eb5e6bb9e282f1b9e03e57e5bcac1ab911007e65200dbc2d7`;
  `max-rust.exe` 10 867 626 Б, sha256
  `54a8fefe4199d1ad4343f26fdc0eb048edc5c233693780dfe51735331103f0a5`
  (PE32+ console, Subsystem=3).
* Живой тест `build/scripts/ui-test-prefs.ps1` (нужен интерактивный рабочий
  стол; в headless-сессии агента скриншот `CopyFromScreen` чёрный — не
  считается свидетельством).


