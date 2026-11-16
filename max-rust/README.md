# max-rust — клиент M.A.X. на Rust под Horizon

Клиент оригинальной M.A.X., написанный заново на Rust. **Не порт 1:1**: архитектура
осмыслена под клиент-сервер, где игровой авторитет — headless-сервер C++ под Horizon.

## Принципы архитектуры

1. **Чистые границы модулей.** `max_core` не знает про сеть и рендер; `max_net` не знает
   про рендер; UI — тонкий слой над событиями.
2. **Сервер — единственный авторитет.** Клиент отправляет *ввод* (приказы, выбор точки
   высадки, конец хода, чат), получает *авторитетное состояние* (настройки, снапшоты,
   дельты) и рендерит его с интерполяцией. Локальное предсказание — только для отзывчивости,
   истина всегда серверная (см. PLAN.md v2, раздел 4).
3. **Два транспортных канала:**
   - WS к Horizon (`max_lobby:list_games/create_game/join_game`, `max_admin:*`) — JSON-события,
     тикеты сессий;
   - UDP к headless-серверу сессии — хендшейк (версия + тикет + data-hash), приказы вверх,
     состояние вниз.
4. **Детерминизм не требуется от клиента.** Клиент не обязан идеально воспроизводить
   симуляцию: расхождение чинится снапшотом сервера. Игровая логика в `max_core` нужна для
   отображения, интерполяции и предпросмотра приказов, а не как второй арбитр.

## Крейты

| Крейт        | Назначение                                                        |
|--------------|-------------------------------------------------------------------|
| `max_core`   | Чистая игровая модель: карта, юниты, команды, приказы, правила. Без I/O. |
| `max_net`    | Wire-протокол: WS-события Horizon + UDP-пакеты сессии (join/state/orders), тикеты. |
| `max_assets` | Чтение ресурсов оригинальной игры: RES/PATCHES, карты (WRL/SCE/MPS), спрайты (RES-теги), звуки (`*.SPW`, RIFF/WAVE PCM16), FLC/FON — в планах. |
| `max_render` | Окно и отрисовка (SDL3), UI-слой над событиями core.              |
| `max_app`    | Библиотека клиента + бинарь `max-rust`: инициализация, запуск net+render, игровой цикл. |

## Статус

- **Версия 1.1.0** (2026-11-16): production-ready клиент — три режима (одиночная,
  hot-seat, сеть Horizon), производство/строительство, исследования и покупные
  апгрейды (`U`/`O`), V70-сейвы с автосейвом, визуал 1:1, QA-фабрика (179 TC,
  golden, soak) и зелёный `ci.sh`.
- **GUI — режим по умолчанию**: `default = ["sdl-from-source"]` в `max_app` —
  `cargo run -p max_app` открывает игру (SDL3 3.4.20 собирается из исходников
  через CMake), флаги не нужны; headless-сборки идут с `--no-default-features`.
- Релизный чек-лист (гейты, live-smoke 2×10, человеческий сценарий M4,
  Linux/Windows-бинарники, тег) — `../docs/development/release-checklist.md`; история —
  `../CHANGELOG.md`.
- Полный детальный план: `../PLAN.md` (v4.0, фазы R2–R4 и milestones M3–M6).

## Звук (M6.2)

`*.SPW` в каталоге игры — обычные WAV (PCM, 22050 Гц, моно, 16 бит), а теги
`MAX.RES` (`TANK1`, `KBUY0`, …) — ASCIIZ-ссылки на файлы рядом с `MAX.RES`.
Декодер работает всегда и не тянет внешних зависимостей:

```rust
use max_assets::wav::WavClip;

let clip = WavClip::open("max-original/KBUY0.SPW")?; // samples: Vec<i16>
println!("{} Гц, {} каналов, {} кадров", clip.sample_rate, clip.channels, clip.frames());
```

Воспроизведение — `max_app::audio::AudioPlayer` за feature **`sound`**
(rodio/cpal); без фичи используется тихий `NoopBackend`, поэтому базовые
сборки, тесты и Docker не требуют ALSA/CPAL:

```sh
# GUI — default: SDL3 3.4.20 собирается из исходников через CMake
cargo run -p max_app

# со звуком (Linux: apt install pkg-config libasound2-dev); sound — не default
cargo run -p max_app --features sound

# headless: SDL не тянется вовсе
cargo run -p max_app --no-default-features -- --demo ../max-original/SAVE1.CAM --turns 5
```

`AudioPlayer::play_once("KBUY0")`/`play_loop("TANK1")` принимает тег или имя
файла; поиск регистронезависимый, клипы кэшируются, вывод неблокирующий.
Клики меню (`ui.rs`) озвучены; звуки юнитов/выстрелов пока только декодируются.

## CI и развёртывание

- Локальный CI (M6.3): `bash ci.sh` из корня репозитория — `cargo check` +
  `cargo test` всего `max-rust` (default = GUI: SDL3 из исходников через CMake;
  ставит X11/Wayland/ALSA/GL dev-заголовки, `cargo test` — с
  `LD_LIBRARY_PATH=target/debug`), `--features sound`, `--no-default-features`
  (headless) и `cargo test --workspace` Horizon в `rust:1.94.0-bookworm`;
  `bash ci.sh --smoke` добавляет живой `build/scripts/live-smoke.sh`
  (2 клиента × N ходов). GitHub-эквивалент — `../.github/workflows/ci.yml`.
- Релиз (M6.4): `bash ../release.sh` собирает Linux-бинарь с
  `--no-default-features --features sdl-from-source,sound`, упаковывает
  `release/` и готовит тег `v1.1.0`; полный порядок —
  `../docs/development/release-checklist.md`.
- Windows x64, headless (M6.4): `bash ../build/scripts/build-windows.sh` —
  кросс-компиляция `x86_64-pc-windows-gnu` (mingw-w64) в Docker, без
  Windows-тулчейна на хосте: `cargo build --release --target
  x86_64-pc-windows-gnu -p max_app --no-default-features` → PE32+ x86-64
  **console** (`max-rust.exe` + `release/max_app-<ver>-windows-x86_64.zip` и
  wine-smoke `--help`, `--demo SAVE1.CAM`): без feature `sdl` подсистема
  остаётся 3, вывод `--demo`/`--new-game`/`--connect` виден в терминале/CI.
- Windows x64, интерактивный (окно SDL3 + звук): нативная сборка на
  Windows-хосте через `powershell -File build\scripts\build-windows-ui.ps1`
  (`--no-default-features --features sdl-vcpkg,sound`, SDL3 из vcpkg) →
  `release/max_app-<ver>-windows-x86_64-ui.zip` (`max-rust.exe` + `SDL3.dll`).
  Подсистема — **windows** (Subsystem=2): двойной клик открывает окно SDL3 без
  консоли; `--help`/`--demo` этой сборки видны только при перенаправлении
  (`max-rust.exe --help > help.txt`). Проверено на MSVC 14.51 + SDL3 3.4.20:
  окно `M.A.X. — Rust client (M4.2 HUD)` открывается, звук — `rodio/cpal`
  (WASAPI, без отдельных DLL). Требования к тулчейну —
  `../docs/development/release-checklist.md` §6.1, разбор грабель —
  `../build/scripts/README.md`.
- Horizon + C++-headless в Docker (M5.3): `docker compose up --build` из корня;
  подробности — `../max-horizon-server/README.Docker.md`.
- Реконнект (M6.1-заготовка): `--reconnect-attempts`/`--reconnect-timeout`,
  восстановление только транспорта ENet+`TP 00`; полное восстановление партии —
  M6.1-финал, см. `crates/max_app/README.md` и `docs/architecture/protocol.md` §9.

## Визуал 1:1 с оригиналом (в работе)

Эталон — C++-порт в этой же рабочей области: `build/run/dbg.png` (главное
меню), `build/smoke-window.png` (партия: снег, здание-ангар, панели HUD).

Сделано:

- **Поверхность — настоящие тайлы.** `.WRL` хранит `tile_ids[112×112]`,
  `tile_count` и пиксели тайлов `64×64` (индекс палитры) + палитру
  (`max_assets::wrl` это уже разбирал, но рендер их не использовал). Теперь
  `max_render::terrain::TerrainAtlas` блитит их с зумом камеры
  (nearest-neighbour, как `Gfx_ZoomLevel`), 6-битные каналы палитры
  растягиваются на 8 бит. Процедурный «шахматный» `TileStyle` остался только
  как fallback без данных.
  Проверка одного файла: `cargo run -p max_render --example terrain --
  ../max-original/SNOW_1.WRL out` → `out-map.bmp` (вся карта, 8 px/клетка) и
  `out-detail.bmp` (фрагмент 1:1, 64 px/клетка). Для SNOW_1 это 337 тайлов,
  12 544 клетки; снимки — `build/wrl-snow-map.png`, `build/wrl-snow-detail.png`.
  Карта партии берётся из того же `WRL` (`App::new_game` →
  `assets.world.surface_map()`), поэтому проходимость и картинка согласованы.
- **Спрайты всех 93 типов юнитов.** `UNIT_SPRITES` — построчный порт
  `SC_UNITS`: все постройки (`HANGAR`, `DEPOT`, `POWERSTN`, `RADAR`…), техника,
  пехота, флот, авиация, пришельцы, мины, взрывы и снаряды грузятся из
  `MAX.RES` вместе с тенями `S_*`, а корпус, тень и отдельная башня `D_*`
  (`TURRET_SPRITE`/`SPINNING_TURRET`, сдвиг `angle_offsets`) рисуются на карте
  тем же кодом, что и HUD-портрет. Маркером команды остаётся только `MASTER`:
  его ресурса нет ни в `MAX.RES`, ни в `PATCHES.RES` (у `SUBMARNE` так же нет
  тени `S_SUBMAR`). В кадре `--new-game` счётчик спрайтов виден в
  `render_frame_with_stats` (`units_drawn`/`units_marker`).
- **Ресурсы** остаются точками, но рисуются поверх настоящих тайлов
  (`map_render::render_resources_into`), а не поверх заглушки.
- **Окно 1:1 из `FRAMEPIC` + раскладка оригинала.** `max_assets::image`
  разбирает оба формата UI-графики:
  `ImageSimpleHeader` (8 байт `w,h,ulx,uly` + пиксели; так устроены
  `PANELTOP`/`PANELBTM`, 171×239/241) и `ImageBigHeader` (777 байт: `ulx,uly,
  w,h`, своя палитра 256×RGB, прозрачный индекс, затем **RLE по строкам**:
  `i16 n`, `n>0` — `n` литеральных байт, `n<0` — повтор следующего байта).
  У `FRAMEPIC` RLE выпускает чанки шире строки — воспроизведено поведение C++
  (`WindowManager_DecodeBigImage` пишет подряд, следующая строка перекрывает
  хвост), поэтому картинка совпадает с оригиналом пиксель в пиксель.
  `max_render::layout` — координаты из таблицы `windows[]` C++-порта:
  окно 640×480, область карты `MAIN_MAP` = (180, 18) 448×448 (7×7 клеток по
  64 px), миникарта (15, 356) 112×112, кнопки End Turn/Files/Preferences и
  сетка SURVEY…/HITS…/SCAN… — с тестами на непересечение с областью карты.
  `max_app::chrome` заливает фон и кладёт карту в область карты; `max_app::panels`
  поверх него рисует верхние кнопки/End Turn/счётчики, портрет (статичный кадр
  спрайта ×2), четыре строки статистики, сетку переключателей SURVEY…/SCAN…,
  зум `ZOOMPNL1`+`ZOOMPTR`, координаты/описание юнита и миникарту из
  `minimap`-слоя `.WRL`. Текст — оригинальные растровые шрифты GNW
  (`max_render::text::gnw`, файлы `FONT1/2/5.FON` в `max_render/data/`),
  встроенный 8×8 оставлен fallback'ом. Размер кадра по умолчанию 640×480, в
  сессии процедурный HUD не рисуется.
  Проверено: `cargo run -p max_app --example panel_frame -- ../build/frame-panels.ppm`
  даёт окно оригинала с выбранным юнитом (`build/frame-panels.png`), в живом
  окне — `build/ui-chrome-session-final.png`, Windows-артефакт рисует тот же
  кадр (`build/frame-windows-artifact.png`).

Меню (пункт E) и масштабирование окна (пункт F) подключены:

- `max_app::menu` рисует главное меню на `MAINPIC` (кнопки `BLANK_*`/`SBLNK_*`,
  портрет `P_*` в рамках `BGRSCRN*`), форму создания игры на `PLANETSE`
  (сетка 8×3 + `MNUBTN4/5/6U`) и список игр на `MULTGAME` (рамка, строки
  20 px, `BTNHIDE`, `MNUBTN3/4/5/6U`) — координаты из `menu.cpp`,
  `planetselectmenu.*`, `networkmenu.*`; подписи — GNW `FONT1`, заголовки —
  `FONT5`, цвета `Fonts_GoldColor`/`DarkOrageColor`;
- `max_render::frame_scale` разворачивает фиксированный кадр 640×480 в окно
  любого размера целочисленным nearest-neighbour (`N×N` на пиксель, чёрные
  поля), а `SdlWindow::window_to_frame` переводит клики обратно; окно
  ресайзится, `--scale N` задаёт множитель (по умолчанию — под рабочий стол).

Осталось: **анимация портрета** — `CORNER_FLIC` сейчас статичный кадр
спрайта, в оригинале — FLIC (`FLC`/`PlayerCorner`).

Раскладка ресурсов снята в `build/res_list.txt` (1828 тегов, offset/size) — по
нему видно, что для 1:1 данных хватает: графика лежит в `MAX.RES`, карта — в
`.WRL`, звук — в `*.SPW`.


## Лицензия

Код клиента — MIT, см. [`../LICENSE`](../LICENSE) и [`../CHANGELOG.md`](../CHANGELOG.md).
Данные оригинальной M.A.X. (`MAX.RES`, `PATCHES.RES`, `COLOR.PAL`, `*.SPW`,
`*.WRL`, сейвы) лицензией клиента не покрываются: M.A.X. (C) 1996 Interplay
Productions, все права сохранены.
