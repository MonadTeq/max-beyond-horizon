# max_app

Клиент M.A.X. на Rust: связка **`max_core`** (симуляция) + **`max_net`**
(Horizon gRPC / ENet) + **`max_assets`** (RES/WRL/спрайты) + **`max_render`**
(палитра/карта/спрайты) в один игровой цикл. Крейт собран так, чтобы
**headless-режим был основным**: тесты, CI и demo-прогон не требуют SDL, окна и
дисплея.

```
max_app (lib)  ── App::tick / apply / render_frame / digest
   ├── max_core   GameState, apply_command, setup::generate_map, crc
   ├── max_net    HorizonClient, SessionHandle, LobbyPacket, BarrierTracker
   ├── max_assets ResArchive, WrlMap, SpriteSheet/ShadowSheet
   └── max_render RgbaBuffer, Viewport, render_map_into, SpriteRenderer
max_app (bin)  ── CLI + SDL3-окно (только под feature `sdl`)
```

## Библиотека

```rust
use max_app::{App, AppConfig, AppMode, NewGameConfig};

let mut app = App::new(AppConfig::default().with_save("max-original/SAVE1.CAM"));
app.init_assets()?;            // MAX.RES, PATCHES.RES, COLOR.PAL, SNOW_1.WRL, спрайты
app.load_game()?;              // V70 → GameState (нормализации SaveLoad_LoadFormatV70)
for _ in 0..5 { app.tick(); }  // GameManager_ProcessTick
let digest = app.digest();     // CRC снапшота + CRC пакета 45 + CRC реплея
println!("{digest}");

let frame = app.render_frame();  // RgbaBuffer без SDL
println!("{}x{}", frame.width(), frame.height());
```

| Метод | Что делает |
|---|---|
| `App::new(AppConfig)` | приложение без файлов игры (ассеты подгружаются явно) |
| `init_assets()` | `ResArchive` для `MAX.RES`/`PATCHES.RES`, `COLOR.PAL`, `.WRL` планеты (`SNOW_1`…`DESERT_6`), `SpriteSheet`/`ShadowSheet` из `assets::UNIT_SPRITES` |
| `load_game()` / `load_save(path)` / `load_save_bytes(&[u8])` | V70-сейв → `GameState::from_save`; счётчики тиков обнуляются |
| `new_game(&NewGameConfig)` | `setup::generate_map` + `GameState::with_setup` на карте планеты |
| `new_game_on_surface(&cfg, &SurfaceMap)` | то же на готовой поверхности (тесты без файлов игры) |
| `tick()` | один тик симуляции |
| `apply(Command)` / `enqueue(Command)` | команда сразу в порядке вызова / в очередь `GameState` |
| `render_frame()` / `render_frame_with_stats()` | кадр `RgbaBuffer` + статистика (клетки, спрайты, маркеры) |
| `write_ppm(path, &frame)` | кадр в бинарный PPM (`P6`) без кодеков |
| `digest()` | `AppDigest { state_digest, state_checksum, replay_crc, snapshot_len, … }` |
| `run_demo(save, turns)` | headless-сценарий `--demo` целиком |

`AppDigest` печатается одной строкой:

```text
ticks=5 turn=1 state=InGame units=312 state_digest=0xC354 state_checksum=0x408A replay_crc=0x5561 snapshot=788026B
```

Эталон `SAVE1.CAM + 5 тиков` зафиксирован в `tests/app.rs`
(`SAVE1_FIVE_TICK_*`): изменение загрузки сейва или симуляции ломает тест
явно, а не «тихо» меняет дайджест.

## CLI

```sh
# из max-rust/; пути по умолчанию — относительно корня репозитория
cargo run -p max_app -- --demo ../max-original/SAVE1.CAM --turns 5

# локальная партия: карта планеты + генерация ресурсов/обломков + кадр в PPM
cargo run -p max_app -- --new-game --world 12 --seed 42 --out frame.ppm

# справка
cargo run -p max_app -- --help
```

Режимы:

| Флаг | Поведение |
|---|---|
| `--demo <save> [--turns N]` | headless: загрузить V70-сейв, прокрутить N тиков (по умолчанию 5), напечатать digest |
| `--new-game` | `setup::generate_map` по параметрам P2 (`--seed`, `--raw/--fuel/--gold/--derelicts`, `--world`), печать отчёта и размеров кадра; `--out` пишет PPM |
| `--connect <host[:port]> --join <game> [--sdl]` | Horizon gRPC (по умолчанию `127.0.0.1:8082`) → ENet → лобби → игровой цикл; без `--sdl` — headless, с `--sdl` — окно |
| `--ui` | интерактивное меню в окне SDL3: создать игру / выбрать игру из лобби / сессия (нужна сборка `--features sdl`) |
| `--player/--team/--clan/--password` | параметры лобби |
| `--data-dir`, `--assets-dir`, `--world-dir`, `--max-res`, `--patches`, `--palette` | где искать данные игры |

По умолчанию `MAX.RES`/`COLOR.PAL` берутся из `max-original/`, `PATCHES.RES` —
из `assets/`; если запуск идёт из `max-rust/`, CLI сам находит корень
репозитория (каталог, где есть `max-rust/` и `max-original/`).

### Окно SDL3

SDL3 не входит в обязательные зависимости: без feature `sdl` окна нет, а
`render_frame()` по-прежнему работает.

```sh
# системный SDL3 (Linux: libsdl3-dev; Windows: vcpkg)
cargo run -p max_app --features sdl -- \
  --connect 127.0.0.1:8082 --join <game_id> --sdl

# SDL3 собирается из исходников (нужен cmake; docker/CI) — как в max_render
cargo run -p max_app --features sdl-from-source -- \
  --connect 127.0.0.1:8082 --join <game_id> --sdl
```

Управление в окне: Esc или закрытие окна — корректное завершение сессии.
Кадры рисует библиотека (`render_frame`), окно только выводит `RgbaBuffer`
через `max_render::SdlWindow`.

## UI (M4.1): меню, список игр, сессия

`--ui` открывает интерактивный клиент: главное меню → создание игры или
список игр Horizon → сессия. Вся логика переходов — в `max_app::ui`
(`Screen`, `InputEvent`, `MenuClient`), SDL-цикл — только в `main.rs`.
Меню не требует данных игры; если `MAX.RES`/`COLOR.PAL` не найдены, карта
рисуется упрощённо, а загрузка ассетов пишет предупреждение.

```sh
# системный SDL3 (Linux: libsdl3-dev; Windows: vcpkg)
cargo run -p max_app --features sdl -- --ui

# SDL3 из исходников (нужен cmake; docker/CI)
cargo run -p max_app --features sdl-from-source -- --ui \
    --connect 127.0.0.1:8082 --player rust --team 0
```

Horizon задаётся `--connect <host[:port]>` (по умолчанию `127.0.0.1:8082`),
остальные флаги (`--player`, `--team`, `--clan`, `--turns`, `--data-dir`,
`--assets-dir`, `--width/--height`) работают как в `--connect`.

Экраны и управление:

| Экран | Что делает | Клавиши/мышь |
|---|---|---|
| **MainMenu** | Create game / Join game / Quit | Up/Down/Tab + Enter; клик; `C`/`J`/`Q`; Esc — выход |
| **CreateForm** | имя, планета (`0..=23`), таймеры, число противников, пароль | Tab/Up/Down — поле; ввод; Left/Right — числа; Enter — создать; Esc — назад |
| **JoinList** | список игр из `ListGames`; выбор и вход | Up/Down + Enter; клик по строке; `R` — обновить; Esc — назад |
| **InSession** | карта (`map_render`) и строка статуса | Esc — выйти из сессии и вернуться в меню |

Кнопка **Create** вызывает `CreateGame`; при успехе экран сразу переходит в
**InSession**, и `main.rs` запускает существующий сетевой путь
(`run_network_with_sink`). **Join** входит в выбранную игру (пароль
защищённой игры можно задать в форме создания; для готовой игры пароль пока
не запрашивается). Ошибки Horizon показываются строкой статуса, экран при
этом остаётся тем же.

Тесты UI не требуют сети и SDL: `MenuClient` подменяется мок-клиентом в
`max_app/src/ui.rs` (переходы, выбор в списке, отрисовка всех экранов).
SDL-часть проверяется компиляцией:

```sh
cargo check -p max_app --features sdl-from-source
```

## Тесты

```sh
# из max-rust/
docker run --rm -v "D:/dev/max-port-debug:/src" -w /src/max-rust \
  rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test"

# только приложение
cargo test -p max_app
```

`tests/app.rs`:

1. **`demo_save1_five_ticks_matches_reference_digest`** — `tests/data/save1_v70.cam`
   (копия фикстуры `max_core`) → 5 тиков → digest совпадает с эталонными
   константами и стабилен при повторном запросе.
2. **`demo_is_bit_identical_across_two_runs`** — два независимых `App` дают
   одинаковый `AppDigest` и одинаковый `snapshot_bytes`.
3. **`new_game_with_p2_parameters_is_deterministic_and_renders`** — вход
   эталонного вектора P2 `p00_l111_d0_s0` (`tools/map_vectors/vectors.txt`):
   поверхность `derive_surface_map(tile_ids, pass_table)`, генерация ресурсов
   сверяется с `resources` вектора по CRC; `report`/digest детерминированы,
   `render_frame` даёт буфер нужного размера и не паникует. Отдельно
   проверяется уровень обломков 2 (непустая расстановка).
4. **`move_command_replays_through_app`** — move-команда из тестов `max_core`
   (`command_move_linear`, пакет 38) применяется через `App::apply`, выставляет
   путь, и три тика дают одинаковый digest в двух прогонах.
5. **`assets_integration_smoke`** — если рядом есть `max-original/MAX.RES`,
   грузит настоящие `MAX.RES`/`PATCHES.RES`/`COLOR.PAL`/`SNOW_1.WRL`, рисует
   кадр с настоящим спрайтом; без данных игры тест печатает «пропуск» и
   завершается успешно.

Тесты **не требуют сети и SDL**; `max_net` проверяется своими модульными
тестами (mock gRPC + байтовые вектора), сетевой цикл `max_app` — компиляцией и
юнит-тестами конфигурации.

## Живой smoke с Horizon + headless (опционально)

Полный прогон **не требуется** для R2.4 и здесь не выполнялся; шаги — из
`docs/dedicated-server-plan.md` (§9) и `max_net/README.md`:

1. Собрать headless-сервер оригинального порта (`MAX_HEADLESS`, `--server`) по
   `build/make-server-run.ps1` / `build/docker-build-linux-server.sh`; он
   арбитр и сам командой не играет.
2. Поднять Horizon (`max-horizon-server`) с секцией `[max]` в `config.toml`
   (путь к headless-бинари, `template_dir`, диапазон UDP-портов) и gRPC
   листенером (по умолчанию `0.0.0.0:8082`, `docs/protocol-grpc.md` §4.4).
3. Убедиться, что сессия видна в лобби (`ListGames`), либо создать её
   (`CreateGame`); получить `game_id`.
4. Проверить, что `MAX.RES`/`PATCHES.RES` клиента побайтово совпадают с
   серверными (иначе `TP 03` reason 3) — `DataHash::from_files` считается
   автоматически.
5. Запустить клиента:
   ```sh
   cargo run -p max_app -- --connect 127.0.0.1:8082 --join <game_id> \
       --player rust --team 0 --turns 3
   ```
   Без `--sdl` клиент headless: проходит `28→44→29→30→36→35→32→05→34`,
   применяет команды сервера, тикает, закрывает ходы барьерами `00/01/52` и
   печатает `network: …` + digest.
6. Признаки успеха: `handshake=true lobby=true`, рост `ticks`, `turns=N`,
   отсутствие `barrier_timeouts`; в логах headless-сессии
   (`sessions/<uuid>/ai_log.txt`) — вход клиента, `Main game loop`, отсутствие
   `Desync check failed`/`desync-*.txt`.

## Ограничения R2.4

* **Alien-обломки не разворачиваются** в `GameState`: `GameState::spawn_unit`
  для `TeamId::ALIEN` индексирует heat-map слой `max_core`, у которого длина
  `PLAYER_TEAM_COUNT == 4`, и паникует. `new_game` возвращает число размещений
  из `setup::generate_map` (`NewGameReport::aliens`), а состояние остаётся без
  юнитов. Это ограничение ядра, а не приложения; рендер карты/ресурсов и
  demo-прогон на сейвах от него не зависят.
* Спрайты грузятся для типов из `assets::UNIT_SPRITES` (12 типов: техника,
  пехота, инженеры, воздух, башня, пришельцы). Остальные типы рисуются
  маркерами командного цвета — осознанное упрощение, расширяется таблицей.
* Сетевой клиент реализует клиентскую половину протокола: лобби, применение
  команд, барьеры и закрытие хода. Полный реконнект/SNAP (`protocol.md` §9) и
  ретрансляция «сервер → остальные клиенты» (§12.1) остаются вне R2.4.
* DMO/WRL-карты читаются из отдельного `.WRL`-файла рядом с `MAX.RES`;
  запасной путь — ресурс `MAX.RES` (если сборка пакует карты внутрь архива).
