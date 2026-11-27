# План ремастера под высокое разрешение (HD)

Версия 0.1 (проект Архитектора) · 2026-11-26. Отдельный трек.

## Ключевой принцип

**HD — только презентация.** Симуляция (`max_core`), сетевая модель, `snapshot_bytes`/
`state_digest`/CRC и golden-наборы 1:1 **не меняются ни на байт**. Ремастер —
опциональный слой подмены текстур поверх 1:1-рендера, по-ассетный fallback;
«пиксель-в-пиксель» остаётся режимом по умолчанию.

## Итог оценки

- Рендер уже разделён на чистый слой (`max_render`) + SDL-слой (`sdl_window`), презентация
  отделена от детерминизма (`presentation_gate`). Готовая точка встраивания HD без касания ядра.
- **Главный барьер — палитровая модель**: спрайты/тайлы/UI — индексы палитры; командные
  цвета (`ColorLut`), яркость (`BrightnessLut`) и циклы (`ColorCycles`) применяются в момент
  отрисовки к индексу. KTX2/Basis — RGBA, поэтому прямое «запекание» ломает реколор.
- **Второй барьер — бэкенд**: `SdlWindow::present` — SDL3 `Canvas` (2D, RGBA32), не грузит
  BC7/ASTC. Двухфазно: транскод KTX2→RGBA8 на CPU, затем (опц.) сырой GPU.

## Режимы презентации (`RenderScale`)

- `S=1` → **PixelPerfect** (сегодняшний 1:1), байт-в-байт неизменен.
- `S∈{2,3,4}` → **Hd**: логический кадр `640·S × 480·S`, HD-текстуры, линейная/трилинейная
  фильтрация при зуме ≠ 64. Координаты UI/клики/камера/симуляция остаются в 640×480
  (тот же паттерн `ScreenGeometry`, ещё одно звено `window→render→/S→logical`).

## Развилки (решение владельца 2026-11-26)

1. **Модель реколора**: **индекс-доменный HD + палитровая LUT на GPU** (командные слоты
   32–39/48–63 сохраняют семантику, `ColorLut`/`BrightnessLut`/циклы = перезапись LUT).
   Запечённые варианты — запасной путь, не основной.
2. **GPU-бэкенд**: **SDL3 Canvas + транскод KTX2→RGBA8 сейчас**, сырой GPU (wgpu/glow)
   — отдельной фазой после R7.
3. **Кратность**: **×2 и ×4**; дефолтный `S` и VRAM-квота — по измерениям R0.

## Ассет-пайплайн

`lossless → basisu (pin, одна CI-машина) → .ktx2 + манифест(content-hash) → target/remaster/
→ run-time транскодер pure-Rust → подмена в sdl_window → fallback на MAX.RES`.

Маппинг: тайлы `.WRL` (без team → RGBA), спрайты юнитов (system+team+brightness+cycles),
хром UI, меню/кнопки, маркеры, курсоры, шрифты. Fallback — `resolve(tag) -> Option<...>`,
отсутствие HD-ассета не ломает запуск.

Тайлы карты (R3): манифест-тег `terrain_tile_<WORLD>_<ID:03>_x<S>` (например,
`terrain_tile_GREEN_1_000_x2`); один тег — одна предзапечённая RGBA-текстура,
`scale` записи совпадает с `S`, тег строится `max_assets::remaster::terrain_tile_tag`.
Сэмплинг — та же формула `Gfx_DecodeMapTile`/`render_tile_into`, но от размера
HD-текстуры. Нет записи в манифесте, файла или декода — тайл рисуется из `.WRL`
(fallback по-ассетный); `S=1` идёт прежним путём и в этот слой не заходит.

Спрайты юнитов (R4, развилка 1 — индекс-доменный HD): `.ktx2` несёт не картинку,
а **карту индексов** палитры высокого разрешения — `R` = индекс палитры (у тени —
уровень маски затемнения), `A` = покрытие тексела (`0` — прозрачность, `255` —
непрозрачность), `G`/`B` не используются; полезная нагрузка кодируется **без
потерь** (UASTC/без сжатия) — lossy ETC1S сдвинул бы индексы и, значит, цвета.
Теги: `unit_sprite_<RES>_<FRAME:03>_x<S>` и `unit_shadow_<RES>_<FRAME:03>_x<S>`
(`max_assets::remaster::sprite_frame_tag`/`sprite_shadow_tag`). Реколор —
на CPU: `palette[brightness[team[index]]]` (`IndexPaletteLut`, 256 записей
`Rgba`), циклизация — сначала `rotate_palette`/`ColorCycles`, затем та же LUT,
то есть ровно семантика `SpriteRenderer::color_for` (командные слоты, уровни
0..7, маяки/дрель). Перекрашенный RGBA8 кэшируется по
`(спрайт, кадр, команда, яркость, кадр циклов)`, кэш ограничен и вытесняет
самую старую запись. Геометрия: `zoom` — пиксели рендер-кадра на тайл из 64
текселей; кадр HD занимает `L·zoom/(64·S)` пикселей, хот-спот масштабируется
пропорционально разрешению ассета (`×S` для корректного ассета). Тень — маска:
уровень `0..255` смешивает `Rgba::darken(5)`, так что `255` — ровно исходное
затемнение. Силуэт (`zoom_level < 8`, `Gfx_TeamColorIndexBase`) рисуется по
покрытию карты индексов без обращения к палитре. Fallback — по-ассетный: кадр
без HD-ассета рисует `SpriteRenderer` с тем же `zoom`, что даёт `×S`-апскейл
исходной картинки 1:1; `S=1` в слой R4 не заходит (байт-в-байт прежний кадр).
Модуль: `max_render::hd_sprites` (`HdSpriteBank`, `HdSpriteRenderer`,
`HdSpriteCache`, `HdIndexMap`/`HdIndexFrame`/`HdShadowFrame`, `IndexPaletteLut`).

Хром UI (R5): `FRAMEPIC` (640×480, собственная палитра), `PANELTOP`/`PANELBTM`,
кнопки/меню/поля/списки — картинки `MAX.RES`; командные цвета, яркость и циклы к
ним не применяются, поэтому HD-ассет — предзапечённый RGBA8, один на картинку.
Теги: `chrome_<RES>_x<S>` (`chrome_FRAMEPIC_x2`, `chrome_PANELTOP_x4`,
`chrome_ENDTRN_U_x2` — имя ресурса `MAX.RES` дословно; так же адресуются кнопки
и фоны меню) и `ui_<SLOT>_x<S>` (`ui_button_x2`, `ui_button_hover_x2`,
`ui_panel_x2`, …) для процедурных рамок `max_render::ui`; строители —
`max_assets::remaster::chrome_image_tag`/`ui_skin_tag`. Альфа — прозрачность
исходной картинки: индекс прозрачности `SimpleImage` запекается как `A = 0`,
такие тексели не пишутся, поэтому кнопка/панель компонуется поверх хрома.
Раскладка — константы `max_render::layout` (640×480); `S` входит только на
выводе (`HdChrome::render_rect`/`scaled_rect`), а hit-test и клики остаются в
логических координатах (цепочка `ScreenGeometry::window_to_frame` делит на `S`).
Fallback по-ассетный: `draw_frame`/`draw_panel`/`draw_image` возвращают `None`,
и вызывающий рисует исходную палитровую картинку; вариант
`draw_*_with_fallback` делает это сам по `ChromeSource` — исходным
`BigImage`/`SimpleImage` из `MAX.RES` поверх оконной палитры `FRAMEPIC` (на
`S=1` — байт-в-байт картинка `max_app::chrome`/`panels`, на `S>1` — её
nearest-апскейл `×S`). Слот `UiBox` без HD-скина рисуется процедурно темой
(hover/disabled откатываются на базовый `ui_button`, чтобы одна текстура
держала кнопки HD). `S=1` (`RenderScale::PixelPerfect`) в слой не заходит:
`load` ничего не грузит, а рендер байт-в-байт прежний.
Модуль: `max_render::hd_chrome` (`HdChrome`, `ChromeSource`, `HdImage`,
`HdUiSkin`, `ChromePanel`, `stretch_into`); точка подмены UI-рамок —
`max_render::ui::render_ui_with_skin` (`UiBox`, `UiBoxPainter`).

Шрифты (R6): текст игры рисуется **растровыми** GNW-шрифтами `FONT1.FON`/
`FONT2.FON`/`FONT5.FON`, и ремастер оставляет их растровыми (TTF не
используется): только так сохраняются пиксельный облик, метрики и рельеф
`Text_TextLine`. HD-ассет — один предзапечённый RGBA8-атлас на шрифт и масштаб,
тег `font_<FONT>_x<S>` (`font_FONT1_x2`; `<FONT>` — стем исходного файла,
`FONT1`/`FONT2`/`FONT5`), строитель — `max_assets::remaster::font_tag`.
Раскладка атласа выводится из метрик `.FON`: глифы `0…255` подряд в одну строку,
ячейка `advance·S × height·S`, смещение — префиксная сумма `advance·S`; `A` —
покрытие («чернила» при `A ≥ 128`), `R`/`G`/`B` не используются, поэтому один
атлас обслуживает любой цвет текста, обводку и тень; кодировать без потерь
(UASTC), паддинг текстуры до кратности 4 справа/снизу допустим. Метрики в
рендер-пикселях кадра `640·S × 480·S` масштабируются **точно на S**: `advance`,
высота ячейки, межстрочный интервал `(height+1)·S` и ширина строки
(`Text_GetWidth`); логическая раскладка остаётся 640×480 (`render_point`).
Рельеф сохранён: `Text_TextLine` — заливка, затем сдвиг вниз-вправо на `S`
рендер-пикселей (обводка на диагонали штриха, тень снаружи), запас `+S` вместо
`+1`; `Text_Blit` с `GNW_TEXT_OUTLINE` — копия со сдвигом `S`. Следствие: если
HD-атлас — точный nearest-апскейл исходного битмапа, HD-текст байт-в-байт равен
`×S`-апскейлу 1:1-текста (golden-инвариант R6). Fallback по-шрифтовый: нет тега,
файла, декода, альфа-канала или размеры атласа меньше метрик (в т.ч. ×1-атлас при
`S > 1`) — рисуется исходный GNW nearest-апскейлом `×S`; 8×8-служебный шрифт —
`draw_text_line_hd`. `S=1` в слой не заходит: `load` ничего не резолвит, а
рендер байт-в-байт прежний. Модуль: `max_render::hd_text`
(`HdGlyphAtlas`, `HdFontAtlas`, `HdFonts`, `draw_text_hd`,
`draw_text_hd_shadowed`, `draw_text_hd_outlined`, `draw_text_line_hd`).

## Пошаговое ТЗ (R0–R7)

- **R0** — измерения (VRAM, объём HD, выбор транскодера) + решения развилок.
- **R1** — загрузчик/манифест (`max_assets::remaster`) + fallback.
- **R2** — build-time `basisu`→`.ktx2` в CI (детерминизм: одна машина, hash).
- **R3** — HD-тайлы карты (предзапечённый RGBA).
- **R4** — HD-спрайты юнитов (recolor/brightness/cycles по развилке 1).
- **R5** — HD-хром UI (FRAMEPIC/панели/кнопки/меню).
- **R6** — HD-шрифты (растровый битмап-атлас GNW ×2/×4; TTF не используется).
- **R7** — режим-переключатель, детерминизм, DoD (golden 1:1 байт-в-байт, HD-golden, soak).

## Интеграция R7: переключатель и HD-кадр

R3–R6 — слои подмены; R7 собирает их в один кадр и включает режимом.

**Режим.** `max_render::render_scale::PresentationMode::{PixelPerfect, Hd}` +
`RenderScale` (`S = 2/4`); CLI — `--hd` (по умолчанию `S = 2` — рекомендация R0),
`--hd-scale N` (`N ∈ {1,2,4}`, `N = 1` — pixel-perfect через тот же
переключатель; `×3`/`×8` отвергаются при разборе аргументов, ассет-тега для них
нет), `--remaster-dir DIR` (по умолчанию `max-brh/target/remaster`).
`PresentationMode::from_flags` — единственная нормализация флагов;
по умолчанию режим `PixelPerfect`, и `HdPresenter::load` в нём **ничего не
резолвит**.

**Владелец слоёв.** `max_app::hd::HdPresenter` строит один раз на запуск
`HdTerrainAtlas` (теги `terrain_tile_<WORLD>_<ID>_x<S>`), `HdChrome`
(`chrome_*_x<S>`, fallback-источник — `ChromeSource` из `FRAMEPIC`),
`HdFonts` (`font_*_x<S>`) и по `HdSpriteBank` на каждый загруженный спрайт
(`unit_sprite_*`/`unit_shadow_*`), плюс общий ограниченный `HdSpriteCache`.
`RemasterCache::resolve` Fallback — по-ассетный; `HdPresenter::stats()` считает,
сколько ассетов реально подставлено.

**Кадр.** `App::render_frame_hd` (и `ui::render_session_hd` для игрового экрана)
рисует `640·S × 480·S`:

1. фон окна — HD `FRAMEPIC` (`draw_frame_with_fallback`: нет ассета — исходная
   палитровая картинка nearest-апскейлом `×S`);
2. карта в области `MAIN_MAP` — с `×S`-камерой (`Viewport::scaled`: экран и
   `zoom` умножаются, мировые координаты и видимые клетки те же), поэтому тайлы
   из `HdTerrainAtlas`, юниты из `HdSpriteBank` (`HdSpriteRenderer` с тем же
   кадром/командой/яркостью/циклом, что и 1:1), а наложения (сетка, селектор,
   путь, метки) масштабируются геометрией камеры сами;
3. слои интерфейса, разложенные в 640×480 (панели, модальные окна), рисуются
   1:1 в пустой буфер и композитируются nearest-апскейлом `×S` — это и есть
   по-ассетный fallback для UI-картинок (`A = 0` — «не тронуто», HD-кадр
   остаётся виден);
4. рамка сообщения (`MessageManager_DrawMessage`) размечается и рисуется сразу
   в `×S` с HD-атласом `Font5` (`panels::draw_status_message_hd`);
   остальной текст интерфейса — `×S`-апскейл 1:1 (по инварианту R6 он совпадает
   с HD-атласом, если атлас — точный апскейл).

`PixelPerfect` в этот слой не заходит: `render_frame` остаётся прежним
байт-в-байт, HD-кадр строится только по явному `--hd`.

**Презентация.** `SdlWindow::set_render_scale` (R3) включает `×S`-кадр и
линейную фильтрацию финального аплоада; окно создаётся размером рендер-кадра
(`640·S × 480·S` точек) с сохранением целочисленного nearest-масштаба;
клики/мышь остаются в 640×480 (`ScreenGeometry::window_to_frame` делит на `S`).
Меню (не игровой экран) в HD-режиме — nearest-апскейл 1:1-кадра.

**SSAA (`--aa`).** Сглаживание — ортогональная ось поверх `S`
(`development/ssaa-design.md`, решение владельца 2026-11-27): множитель
`aa ∈ {Off, 2, 4}` (`AaMode`), кадр рисуется в `640·S·aa × 480·S·aa` и
сводится **целочисленным box-фильтром** `aa×aa` обратно в `640·S × 480·S`
(`max_render::frame_scale::downscale_box`, CPU, детерминирован). Ключевое:

- `aa` — только презентация: `SdlWindow`/`max_core`/snapshot/CRC/golden 1:1 не
  трогаются; при `S = 1` множитель **принудительно Off** (golden 1:1
  байт-в-байт);
- геометрия (`viewport.zoom = 64·S·aa`, `MAIN_MAP` и слои UI) считается в
  `S·aa`, а `×S`-ассеты HD-слоёв (тайлы/спрайты/хром/шрифты) раскладываются
  своим equal-block маппингом — каждый тексел становится блоком `aa×aa`
  (nearest `×aa`). Поэтому сплошные участки не меняются, а края, попавшие
  внутрь box-блока, получают промежуточный цвет;
- **uniform (whole-frame resolve)**: в SSAA-буфер `640·S·aa × 496·S·aa`
  композитится весь кадр (игра + полоса меню), затем один box-фильтр сводит его
  в `present_buffer`; окно по-прежнему показывает `640·S × 496·S` и не
  переоткрывается при смене `aa`;
- `settings.jsonc` — `display.aa` (`0`/`1` = off, `2`, `4`; прочее — ошибка
  запуска), CLI — `--aa 0|off|1|2|4`, GUI — `Settings → Antialiasing`
  (`WindowMenuAction::SetAa`, без переоткрытия окна); приоритет
  **CLI > jsonc > дефолт Off**;
- буфер SSAA — CPU RAM, не VRAM: квота `HdStats` остаётся `×S`-слоями, а
  `hd_frame_bench --aa N` печатает отдельно время рендера и резолва;
- 60 fps: при `S = 2` + `aa` CPU-рендер выходит за бюджет, поэтому `aa` —
  опциональный пресет качества (скриншоты), дефолт `Off`.

**Гейты (DoD R7).**

- `PixelPerfect` — байт-в-байт: golden 1:1 и `presentation_gate` не меняются;
  `--hd` не входит в 1:1-путь (и в тестах, и в коде).
- HD-golden «по источникам, не по окну»: HD-кадр есть функция исходных ассетов
  и `S`; при слоях, являющихся точным nearest-апскейлом исходников, HD-кадр
  **байт-в-байт равен `×S`-апскейлу 1:1-кадра** (включая спрайты:
  `hd_unit_sprites_equal_the_upscaled_1to1_drawing`), и не зависит от окна/DPI.
- Детерминизм: `snapshot_bytes`/`state_digest` при рендере в HD совпадают с
  1:1-прогоном (`presentation_gate::hd_presentation_does_not_change_the_snapshot_or_digest`).
- VRAM/soak: `HdStats::fits_quota` против квоты R0 (105 MiB при `S=2`, 420 MiB
  при `S=4`), ограниченный кэш реколора, прогон HD-кадров не трогает состояние;
  замер кадра — `examples/hd_frame_bench` (`cargo run -p max_app
  --no-default-features --release --example hd_frame_bench -- --scale 2 [--aa 2]`).
- SSAA: `aa = Off` — байт-в-байт текущий HD-кадр (`aa_off_is_the_current_hd_frame`),
  box-резолв `S·aa`-кадра равен `S`-кадру (`the_box_resolve_of_the_ssaa_frame_is_the_hd_frame`),
  `aa > 1` сглаживает края и не меняет сплошные участки
  (`ssaa_smooths_edges_and_keeps_flat_interiors`), `snapshot_bytes`/`state_digest`
  не зависят от `aa` (`presentation_gate::ssaa_does_not_change_the_snapshot_or_digest`).
  `S = 1` принудительно Off (`settings::resolve`), поэтому golden 1:1 не меняется.
- Тесты: `max_app/tests/hd_presentation.rs` (HD-golden/VRAM/soak/SSAA),
  `max_app/tests/presentation_gate.rs` (+HD/SSAA-профили), unit-тесты
  `max_render::render_scale`/`frame_scale`/`hd_text`/`viewport` и `max_app::hd`.

**Ограничение фазы.** Интерфейс (панели, меню, диалоги) остаётся 640×480 и
попадает в HD-кадр nearest-апскейлом: покадровый HD-UI — отдельная фаза (GPU/
атласная раскладка), как и сырой GPU-бэкенд после R7. Живая проверка «60 fps»
зависит от машины: R0 измерил CPU-рендерер `×2` в ~21 мс (47 fps) и `×4` в
~83 мс (12 fps) на референсной машине, поэтому `S=4` в CPU-пути остаётся
неиграбельным до GPU-фазы.

## Guardrails (сводно)

1. Оракул read-only; ремастер — удаляемый кэш + отдельный манифест.
2. HD не импортирует/не мутирует `max_core`.
3. `PixelPerfect` (S=1) байт-в-байт — расхождение = блокер.
4. По-ассетный fallback.
5. Режим фильтрации — глобальный, не точечный.
6. Слои однонаправленные.
7. Энкодер детерминирован (pin basisu + одна машина + hash).
8. Тесты по типам; комментарии — английские.

## Риски

Реколор/яркость/циклы vs RGBA (развилка 1); бэкенд BC7/ASTC (SDL не грузит); VRAM
(HD-спрайты × кадры); сдвиг фильтрации (линейный только в Hd); семантика слотов палитры
(контракт художника); офф-бай-ван при S=2,3; затемнение теней; метрики шрифта.

## Сводка ссылок

`max_render/src/{buffer,frame_scale,presentation,sdl_window,palette,sprite_atlas,terrain,map_render,viewport,text,ui,layout}.rs`;
`max_assets/src/{res,sprite,image,wrl,markers,cursor}.rs`; `max_app/tests/presentation_gate.rs`;
`development/asset-storage.md` (§Ремастер), `render-modern-systems.md`, `sdl-ui-integration.md`;
оракул `max-cpp-port/src/{gfx,window_manager,resource_manager,world,text}.cpp`.
