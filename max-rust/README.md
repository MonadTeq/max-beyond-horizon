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
| `max_app`    | Бинарь: инициализация, запуск net+render, игровой цикл.           |

## Статус

- **Версия 1.0.0** (2026-11-15): workspace собирается, тесты зелёные; клиент
  проходит лобби, играет партию, показывает SDL3-меню/HUD, поддерживает
  реконнект транспорта (M6.1-заготовка) и звук меню (feature `sound`).
- Релизный чек-лист (гейты, live-smoke 2×10, человеческий сценарий M4,
  Linux/Windows-бинарники, тег) — `../docs/release-checklist.md`; история —
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
# со звуком (Linux: apt install pkg-config libasound2-dev)
cargo run -p max_app --features sdl,sound -- --ui

# без звука (по умолчанию): окно работает, клики беззвучны
cargo run -p max_app --features sdl -- --ui
```

`AudioPlayer::play_once("KBUY0")`/`play_loop("TANK1")` принимает тег или имя
файла; поиск регистронезависимый, клипы кэшируются, вывод неблокирующий.
Клики меню (`ui.rs`) озвучены; звуки юнитов/выстрелов пока только декодируются.

## CI и развёртывание

- Локальный CI (M6.3): `bash ci.sh` из корня репозитория — `cargo check` +
  `cargo test` всего `max-rust` (включая feature `sound`) и
  `cargo test --workspace` Horizon в `rust:1.94.0-bookworm`; `bash ci.sh --smoke`
  добавляет живой `build/scripts/live-smoke.sh` (2 клиента × N ходов).
  GitHub-эквивалент — `../.github/workflows/ci.yml`.
- Релиз (M6.4): `bash ../release.sh` собирает Linux-бинарь с
  `sdl-from-source,sound`, упаковывает `release/` и готовит тег `v1.0.0`;
  полный порядок — `../docs/release-checklist.md`.
- Windows x64, headless (M6.4): `bash ../build/scripts/build-windows.sh` —
  кросс-компиляция `x86_64-pc-windows-gnu` (mingw-w64) в Docker, без
  Windows-тулчейна на хосте: `cargo build --release --target
  x86_64-pc-windows-gnu -p max_app --no-default-features` → PE32+ x86-64
  `max_app.exe` + `release/max_app-<ver>-windows-x86_64.zip` и wine-smoke
  (`--help`, `--demo SAVE1.CAM`).
- Windows x64, интерактивный (окно SDL3 + звук): нативная сборка на
  Windows-хосте через `powershell -File build\scripts\build-windows-ui.ps1`
  (`--features sdl-vcpkg,sound`, SDL3 из vcpkg) →
  `release/max_app-<ver>-windows-x86_64-ui.zip` (`max_app.exe` + `SDL3.dll`).
  Проверено на MSVC 14.51 + SDL3 3.4.20: окно `M.A.X. — Rust client (M4.2 HUD)`
  открывается, звук — `rodio/cpal` (WASAPI, без отдельных DLL). Требования к
  тулчейну — `../docs/release-checklist.md` §6.1, разбор грабель —
  `../build/scripts/README.md`.
- Horizon + C++-headless в Docker (M5.3): `docker compose up --build` из корня;
  подробности — `../max-horizon-server/README.Docker.md`.
- Реконнект (M6.1-заготовка): `--reconnect-attempts`/`--reconnect-timeout`,
  восстановление только транспорта ENet+`TP 00`; полное восстановление партии —
  M6.1-финал, см. `crates/max_app/README.md` и `docs/protocol.md` §9.

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
- **Здания.** `UNIT_SPRITES` расширен с 12 до **39** типов: все постройки
  (`HANGAR`, `DEPOT`, `POWERSTN`, `RADAR`, `BARRACKS`, `AIRPLT`, `LIGHTPLT`,
  `GUNTURRT`… ) грузятся из `MAX.RES` вместе с тенями `S_*`, а не рисуются
  квадратным маркером команды. В кадре `--new-game` это видно по счётчику:
  было `sprite=3 marker=4`, стало `sprite=4 marker=3` (шахта — настоящий спрайт).
- **Ресурсы** остаются точками, но рисуются поверх настоящих тайлов
  (`map_render::render_resources_into`), а не поверх заглушки.

Осталось (по убыванию визуального эффекта):

1. **HUD/окно оригинала.** В `MAX.RES` есть весь хром: `FRAMEPIC` (фон окна),
   `PANELTOP`/`PANELBTM` (верхняя/нижняя панели), `MNUBTN1U..6D`,
   `UNTBTN_U/D`, `BTNHIDE`, `MENU_ON/OFF`, `RADAR` (миникарта-рамка). Сейчас
   HUD (`max_app::hud`, `max_render::ui`) рисуется процедурно и на оригинал не
   похож — это пункт №1 задачи «1:1».
2. **Шрифт оригинала** вместо встроенного 8×8 (`FON`/RES-шрифт пока не
   разобран) — от него зависит вся типографика меню и HUD.
3. **Миникарта** из `.WRL` (`WrlMap::minimap()` уже парсится) вместо
   процедурного канваса; рамка и кнопки `2x`/`1x`.
4. **Иконки** ресурсов/обломков (`I_*`) вместо цветных точек, курсор мыши,
   панель юнита с портретом (`P_*`, `I_*`), экран выбора планеты (`SNOW_PIC` +
   `world/6`).
5. Меню (главное, create/join) — на `MNUBTN*U/D` и `FRAMEPIC`, как в оригинале.

Раскладка ресурсов снята в `build/res_list.txt` (1828 тегов, offset/size) — по
нему видно, что для 1:1 данных хватает: графика лежит в `MAX.RES`, карта — в
`.WRL`, звук — в `*.SPW`.


## Лицензия

Код клиента — MIT, см. [`../LICENSE`](../LICENSE) и [`../CHANGELOG.md`](../CHANGELOG.md).
Данные оригинальной M.A.X. (`MAX.RES`, `PATCHES.RES`, `COLOR.PAL`, `*.SPW`,
`*.WRL`, сейвы) лицензией клиента не покрываются: M.A.X. (C) 1996 Interplay
Productions, все права сохранены.
