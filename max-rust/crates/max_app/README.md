# max_app

Клиент M.A.X. на Rust: связка **`max_core`** (симуляция) + **`max_net`**
(Horizon gRPC / ENet) + **`max_assets`** (RES/WRL/спрайты) + **`max_render`**
(палитра/карта/спрайты) в один игровой цикл. **По умолчанию собирается GUI**:
`cargo run -p max_app` открывает игру (`default = ["sdl-from-source"]`, SDL3
3.4.20 собирается из исходников через CMake). Headless-режимы (`--demo`,
`--new-game`, `--connect` без `--sdl`), тесты, QA и CI работают без окна и без
SDL — они собираются с `--no-default-features`.

```
max_app (lib)  ── App::tick / apply / render_frame / digest
   ├── max_core   GameState, apply_command, setup::generate_map, crc
   ├── max_net    HorizonClient, SessionHandle, LobbyPacket, BarrierTracker
   ├── max_assets ResArchive, WrlMap, SpriteSheet/ShadowSheet
   └── max_render RgbaBuffer, Viewport, render_map_into, SpriteRenderer
max_app (bin)  ── CLI + SDL3-окно (feature `sdl`, в default — `sdl-from-source`)
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
# без флагов (или --ui) — GUI: меню, создать/выбрать игру, партия.
# SDL3 входит в default (фича sdl-from-source) — собирается из исходников
# через CMake, системный SDL3/vcpkg не нужен.
cargo run -p max_app

# headless: V70-сейв → N тиков → digest (SDL не тянется вовсе)
cargo run -p max_app --no-default-features -- --demo ../max-original/SAVE1.CAM --turns 5

# локальная партия: карта планеты + генерация ресурсов/обломков + кадр в PPM
cargo run -p max_app --no-default-features -- --new-game --world 12 --seed 42 --out frame.ppm

# справка
cargo run -p max_app --no-default-features -- --help
```

Режимы:

| Флаг | Поведение |
|---|---|
| *(без флагов)* | GUI по умолчанию: интерактивное меню в окне SDL3 — создать игру / выбрать игру из лобби / сессия. Feature `sdl` входит в `default = ["sdl-from-source"]`, поэтому `cargo run -p max_app` сразу открывает игру |
| `--demo <save> [--turns N]` | headless: загрузить V70-сейв, прокрутить N тиков (по умолчанию 5), напечатать digest |
| `--new-game` | `setup::generate_map` по параметрам P2 (`--seed`, `--raw/--fuel/--gold/--derelicts`, `--world`), печать отчёта и размеров кадра; `--out` пишет PPM |
| `--connect <host[:port]> --join <game> [--sdl]` | Horizon gRPC (по умолчанию `127.0.0.1:8082`) → ENet → лобби → игровой цикл; без `--sdl` — headless, с `--sdl` — окно |
| `--ui` | совместимый alias для GUI-режима по умолчанию |
| `--record <file.qa>` | запись ручной игры в `.qa`-сценарий для `qa_run` (работает в GUI; существующий файл не перезаписывается — суффикс `-1`) |
| `--player/--team/--clan/--password` | параметры лобби |
| `--reconnect-attempts N` | M6.1-заготовка: сколько раз переподключаться при разрыве ENet (по умолчанию 3; `0` — выключить) |
| `--reconnect-timeout S` | таймаут одной попытки переподключения (ENet connect + `TP 00`) в секундах, по умолчанию 10 |
| `--turn-wait N` | окно ввода в ходу сессии, секунды: `0`/нет — headless, ход закрывается сразу после тика (так идут live-smoke/тесты); `N > 0` в GUI/с `--sdl` — ход держится открытым до **End Turn** или до `N` секунд, чтобы игрок успел отдать приказы |
| `--data-dir`, `--assets-dir`, `--world-dir`, `--max-res`, `--patches`, `--palette` | где искать данные игры |

По умолчанию `MAX.RES`/`COLOR.PAL` берутся из `max-original/`, `PATCHES.RES` —
из `assets/`; если запуск идёт из `max-rust/`, CLI сам находит корень
репозитория (каталог, где есть `max-rust/` и `max-original/`).

### Окно SDL3

**GUI — режим по умолчанию**: `max_app` собирается с
`default = ["sdl-from-source"]`, то есть `cargo run -p max_app` (и обычная
`cargo build`) открывают игру: SDL3 3.4.20 берётся из vendored `sdl3-src` и
собирается CMake'ом из исходников — ни системного SDL3, ни vcpkg не нужно.
Требуются только C-компилятор и `cmake` (на Windows — MSVC + CMake из состава
Visual Studio, удобнее из Developer PowerShell).

```sh
# GUI: SDL3 из исходников (default)
cargo run -p max_app

# + звук (feature sound — НЕ default: тянет rodio/cpal → ALSA/WASAPI)
cargo run -p max_app --features sound

# headless: фичи SDL не нужны ни при сборке, ни в рантайме
cargo run -p max_app --no-default-features -- --demo ../max-original/SAVE1.CAM --turns 5
```

Другие способы получить SDL3 (явные фичи = `--no-default-features`, иначе
к ним добавится default `sdl-from-source`):

```sh
# системный SDL3 (Linux: libsdl3-dev; Windows: vcpkg)
cargo run -p max_app --no-default-features --features sdl -- \
  --connect 127.0.0.1:8082 --join <game_id> --sdl

# SDL3 из vcpkg (Windows)
cargo run -p max_app --no-default-features --features sdl-vcpkg -- \
  --connect 127.0.0.1:8082 --join <game_id> --sdl
```

Управление в окне: Esc или закрытие окна — корректное завершение сессии.
Кадры рисует библиотека (`render_frame`), окно только выводит `RgbaBuffer`
через `max_render::SdlWindow`.

#### Windows: без консольного окна

GUI-сборки (`sdl` — он же default `sdl-from-source`, `sdl-vcpkg`; все включают
`sdl`) собираются с
`#![cfg_attr(feature = "sdl", windows_subsystem = "windows")]` в `src/main.rs`:
у `max-rust.exe` подсистема **windows** (PE Subsystem=2), поэтому двойной клик
открывает только окно SDL3 — окно cmd не появляется. Обратная сторона: у этой
сборки нет stdout в терминале, `--help`/`--demo` печатают только при
перенаправлении (`max-rust.exe --help > help.txt`).

Headless-сборка (`--no-default-features`, **без** `sdl`) остаётся **console**
(Subsystem=3): `--demo`, `--new-game`, `--connect` печатают в терминал и CI,
как раньше. Так собираются `build/scripts/build-windows.sh`, CI и QA-прогоны.
Примеры (`qa_run`, `qa_golden_check`) — отдельные console-бинари и подсистемой
клиента не затрагиваются; в CI они идут с `--no-default-features`.

## UI (M4.1): меню, список игр, сессия

Запуск без флагов (или с `--ui`) открывает интерактивный клиент: главное
меню → создание игры или список игр Horizon → сессия. Вся логика переходов — в `max_app::ui`
(`Screen`, `InputEvent`, `MenuClient`), SDL-цикл — только в `main.rs`.
Меню не требует данных игры; если `MAX.RES`/`COLOR.PAL` не найдены, карта
рисуется упрощённо, а загрузка ассетов пишет предупреждение.

```sh
# GUI (default: SDL3 из исходников)
cargo run -p max_app

# headless-прогон против Horizon (SDL не нужен)
cargo run -p max_app --no-default-features -- \
    --connect 127.0.0.1:8082 --player rust --team 0
```

Horizon задаётся `--connect <host[:port]>` (по умолчанию `127.0.0.1:8082`),
остальные флаги (`--player`, `--team`, `--clan`, `--turns`, `--data-dir`,
`--assets-dir`, `--width/--height`, `--handshake-patches`) работают как в
`--connect`.

### Сетевая партия в окне: `--turn-wait`

По умолчанию сетевой цикл закрывает ход сам (`06 + 52` сразу после тика) — это
правильный режим для headless и live-smoke, но в окне ход длится один кадр
(~16 мс), и игрок ничего не успевает. Для интерактивной игры:

```sh
max-rust --turn-wait 60          # ход ждёт End Turn или 60 секунд
max-rust --turn-wait 0           # прежнее поведение (по умолчанию)
```

В фазе PLAY с окном и `--turn-wait N > 0` цикл крутит тики и кадры, клики
собирают приказы (`38` движение, `08` атака, `18` чат), а ход закрывается
кнопкой **End Turn** или по таймауту `N` секунд. Проверено двумя клиентами
(team 0/1) против Horizon + C++ headless: 6 ходов, приказы `38` доходят до
сервера, чат `18` доставляется пиру, барьеры `06/52/45/54` — без таймаутов и
без десинка.

Сеть работает **без обходных флагов** (M-NET-доводки):

* `data_hash` согласуется сам: без явного `--handshake-patches` клиент
  пробует полный хэш (`MAX.RES` + `assets/PATCHES.RES`), а после
  `TP 03 reason=GameData` один раз переподключается с хэшем только `MAX.RES`
  (так считает сервер, чей `game_data` без `PATCHES.RES`; тикет тот же). Если
  отвергнуты оба варианта — ошибка с подсказкой про `--handshake-patches`.
* `Create game` сразу входит в сессию: первый `SessionHandle::start`
  повторяется с backoff `1,2,3,5,8,11` с (окно ~30 с, статус
  «Session warming up — attempt N/M»), потому что сессия `max-port`
  поднимается 10–15 с. Параметры — `--join-retry-attempts` (6) и
  `--join-retry-timeout` (4 с на попытку); `--reconnect-*` остаются для
  разрыва уже идущей сессии.
* `--handshake-patches` по-прежнему работает как явный override (ровно один
  вариант хэша, без fallback) — нужен только для нестандартного стенда.

### Запись QA-сценария: `--record`

`--record <file.qa>` записывает ручную игру в скрипт слоя 2 (см.
`max-rust-qa/drivers/README.md`, раздел «Запись сценариев вручную»):

```sh
# из max-rust/; GUI (default): SDL3 из исходников или системный SDL3
cargo run -p max_app -- \
  --record ../max-rust-qa/scripts/my-run.qa
```

Запись начинается с главного меню и дописывается при выходе. В файл попадают
`click`/`rclick` в пикселях кадра 640×480 (масштаб окна пересчитывается
обратно), `key`, агрегированный `type` для текстового ввода в сессии и
`wait N` — паузы в кадрах с cap 120. Существующий файл не перезаписывается:
рядом появляется `my-run-1.qa`. Готовый скрипт сразу исполняется
`qa_run`; `assert`/`shot` QA-инженер добавляет вручную после записи.

Без SDL рекордер доступен как API (`max_app::qa::{Recorder, RecordContext}`):
вызывающий код даёт событие `InputEvent` и `begin_frame()` раз в кадр, а
`finish()` дописывает хвост. Ограничения (движение мыши, `Key::Other`, Backspace
внутри `type`, сетевые сессии) — в `drivers/README.md`.

Экраны и управление:

| Экран | Что делает | Клавиши/мышь |
|---|---|---|
| **MainMenu** | New game (M-SP) / Multiplayer / Quit | Up/Down/Tab + Enter; клик; `C`/`J`/`Q`; Esc — выход |
| **MultiplayerMenu** | `menu_multiplayer_menu_loop`: Host / Join / Hot Seat / Hot Seat Scenarios / Load Hot Seat / Cancel | клик; `H`/`J`/`O`/`T`/`L`/`C` (Shift-хоткеи оригинала); Esc — назад |
| **LocalSetup** | M-SP / M-HS 1:1: Options (`OPTNFRM`) → Planet (`PLANETSE`) → Players (`CHOSPLYR`) | клик по радиокнопкам/слотам/кнопкам; ввод имени и лимита победы; Enter — дальше; Esc — назад |
| **CreateForm** | сетевая игра: имя, планета (`0..=23`), таймеры, число противников, пароль (открывается клавишей `C` в списке игр) | Tab/Up/Down — поле; ввод; Left/Right — числа; Enter — создать; Esc — назад |
| **JoinList** | список игр из `ListGames`; выбор и вход | Up/Down + Enter; клик по строке; `R` — обновить; `C` — создать игру; Esc — назад |
| **InSession** | карта (`map_render`) + HUD (M4.2); в локальной партии — ходы ИИ, таймер, окно перехода хода (M-HS) | LMB — выбрать юнита, RMB — приказ движения, клик по врагу — атака, End Turn/Menu, `P` — пауза, `S` — Save Game, `L` — Load Game, клик по миникарте — камера, Esc — отмена/выход (в окне перехода — подтверждение) |
| **SaveLoad** (модальное окно P3) | `Save Game`/`Load Game` 1:1 по `saveloadmenu.cpp`: 10 слотов с превью, страницы, ввод имени | клик по слоту/кнопкам; Up/Down/Left/Right; PageUp/PageDown; ввод имени; Enter — Save/Load; Esc — Cancel |
| **GameOver** | экран победы/поражения 1:1 (`ENDGAME*`, `ENDARM`, `ENDOK`) | Enter/Esc/клик по OK — в меню |

### M-SP: одиночная игра против ИИ

«New Game» в главном меню открывает настройку локальной партии — три экрана
1:1 с C++-портом (`max_app/src/setup.rs`):

1. **Options** (`OPTNFRM`): сложность ИИ, таймеры хода и конца хода, режим
   игры, стартовые кредиты, уровни ресурсов, условие победы + поле имени;
2. **Planet** (`PLANETSE`): 3×2 мини-карты текущей группы планет (из `.WRL`),
   стрелки групп, имя/описание, Random/Cancel/?/Done;
3. **Players** (`CHOSPLYR`): четыре слота Human/Computer/None, флаги команд с
   эмблемой клана; Done проверяет «минимум два игрока» и стартует партию.

Локальный цикл (`max_app/src/local.rs`, порт `GameManager_TeamTurnTurnBased`):
`manage_economy` → `Ai_BeginTurn` для компьютеров → тики до
`end_turn_retry_counter > 120` → пакет `06` → следующая команда → после
последней команды `GameState::finish_turn` (условия победы, `turn_counter+1`).
End Turn и таймер хода закрывают ход человека, `P` — пауза. Победа/поражение —
экран `max_app/src/winloss.rs` (`menu_draw_game_over_screen` 1:1).

Сохранение (P3): партия пишется **настоящим V70-сейвом**
(`GameState::to_save()` → `max_core::save_write`, `App::save_v70_to`): окно
`Save Game`/`Load Game` (клавиши `S`/`L`) — 10 слотов на страницу, файлы
`save<slot>.<ext>` по категории, автосейв в конце раунда в слот **10**
(`save10.dta`, `App::autosave`). Загрузка V70 в локальную партию —
`App::start_local_game_from_save` / `App::load_v70_path`. В сети окно не
открывается: оригинал при сохранении рассылает пакет 16, которого в нашем
протоколе нет. Прежний контейнер `MXLS` (`App::local_save_bytes` /
`write_local_save` / `read_local_save`: настройки + фаза + `snapshot_bytes` +
CRC) сохранён для быстрых снапшотов QA.

Кадры-свидетельства:

```sh
cargo run -p max_app --no-default-features --example sp_frames -- ../max-original ../build
# → build/sp-{options,planets,players,game,victory,defeat}.ppm
```

Тесты: `cargo test -p max_app --test local_sp` (12).

### M-HS: hot-seat (2–4 человека на одном ПК)

Вход — **Multiplayer Game** → **Hot Seat Game** (`menu_multiplayer_menu_loop`).
В C++ это не отдельный режим симуляции: `GameManager_UpdateHumanPlayerCount`
при `> 1` человека принудительно включает turn-based, а
`GameManager_UpdateGuiControl` на смене человеческой команды показывает
модальный `DialogMenu` с текстом `_(c2eb)` = `"%s:\nBegin turn."` и именем
команды (`Red Team`, `Green Team`, …). Экран настройки тот же, но
`ChoosePlayerMenu` работает с `is_single_player = false`: заголовок
`Hot Seat Menu` (`_(55c8)`), «Human» на другом слоте не переносит флаг,
`Done` требует двух людей (`_(06ec)`).

Окно перехода (`max_app/src/hotseat.rs`) — 1:1 `DialogMenu`: фон `HELPFRAM`
301×231 в (253,126) (центр области карты), текст `GNW_TEXT_FONT_5` цветом
`GNW_TEXT_OUTLINE | 0xFF` в (20,14) 265×175, кнопка `HELPOK_U/D` 65×24 в
(120,193) с подписью `OK`; закрывается кликом по `OK`, Enter или Esc. Пока окно
открыто, `local_update` не тикает и не крутит таймер хода (в C++ цикл стоит в
`DialogMenu::Run()`), а ввод принимает только подтверждение.

Ввод: `state.player_team` переключается на активного человека, выделение
сбрасывается при передаче хода, чужой юнит не выделяется и приказов не
получает; во время хода компьютера приказы заблокированы. `End Turn` и таймер
действуют на каждую человеческую команду.

Кадры: `build/hs-{players,turn,turn2,game,ai}.png`; тесты —
`cargo test -p max_app --test hot_seat` (13).

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
# default-фичи включают sdl-from-source (окно) — проверяется компиляцией;
# headless-сборка (без SDL) — отдельно:
cargo check -p max_app
cargo check -p max_app --no-default-features
```

## HUD (M4.2) и приказы мышью (M4.3)

Внутри сессии поверх карты рисуется HUD (`max_app::hud`), а мышь отдаёт
приказы в сессию. Всё — чистые функции от состояния, поэтому HUD рисуется и
тестируется без окна.

| Элемент | Содержимое |
|---|---|
| миникарта | `SurfaceMap` 112×112, сэмплируется в панель; рамка камеры и юниты своей команды |
| панель юнита | имя/тип, HP, ammo, приказ/состояние, клетка (без выделения — подсказка) |
| журнал | кольцевой буфер `MessageLog` (8 строк): выбор, приказы, ответы |
| полоса ресурсов | gold команды + суммы комплексов (`TeamUnits`): материал, топливо, энергия, рабочие |
| кнопки | **End Turn** и **Menu** |

Управление:

| Действие | Результат |
|---|---|
| **LMB по своему юниту** | выделение (панель юнита, крупная точка на миникарте) |
| **RMB по свободной клетке** | `Paths_RequestPath` → предпросмотр маршрута линией → пакет **38** (`Command::Move` с `GroundPath`) |
| **LMB по врагу** | приказ атаки — пакет **08**, процессор `P3` (`ORDER_MOVE_TO_ATTACK` + `enemy_unit`) |
| **RMB по юниту / Esc** | отмена выделения и предпросмотра (второй Esc — выход в меню) |
| **End Turn** | тот же путь, что и таймер: пакет **06** + закрытие хода (`52`/`finish_turn`) |
| **Menu** | выход в главное меню |
| клик по миникарте | камера на выбранную клетку |

Как приказы уходят в сеть: `ui::handle_session_input` собирает `Command`
через `hud::build_move_order`/`hud::build_attack_order` и кладёт в
`App::hud().pending`; игровой цикл (`net::flush_hud_orders`) применяет команду
локально (`events::apply_command` — как `Remote_ReceiveNetPacket_*` для своей
команды) и отправляет `Packet::Command` в `SessionHandle` (энкодеры
`max_net::wire::command`: 38 — путь шагами, 08 — `P3`). В демо-режиме
(`online = false`, headless-тесты) приказы применяются сразу через
`App::flush_orders` — без сети.

Выделение, журнал и предпросмотр живут в `App` (`hud::HudState`) и **не
входят** в `GameState::snapshot_bytes`/`AppDigest`: детерминизм тика и
CRC-барьер 45 от них не зависят.

## Реконнект (M6.1-заготовка)

`SessionEvent::Disconnected` больше не завершает сетевой прогон молча: клиент
запускает `ReconnectPolicy` из `max_app::net` — заново делает **ENet connect +
`TP 00`** с таймаутом на попытку и паузой между попытками.

```sh
# 5 попыток по 5 секунд; 0 попыток — выключить автоматический реконнект
cargo run -p max_app -- --connect 127.0.0.1:8082 --join <game_id> \
    --reconnect-attempts 5 --reconnect-timeout 5
```

В логах:

```
WARN ... reconnect: сервер разорвал ENet-сессию
WARN ... reconnect: retrying — попытка 1/5 (таймаут 5s)
...
INFO ... reconnect: ENet+TP00 восстановлены (попытка 2/5)
WARN ... reconnect: транспорт восстановлен ..., но восстановление состояния партии — M6.1-финал
```

Что **не** делает заготовка (это M6.1-финал): вход в идущую партию и
восстановление состояния. В протоколе нет SNAP/повторного входа в сессию
(`docs/protocol.md` §9); пакет **54** — синхронизация состояния *своей команды*
на CRC-проверке, а не запрос снапшота. Поэтому после успешного транспортного
реконнекта прогон завершается, а `NetworkReport` несёт
`reconnects`/`reconnect_attempts`/`reconnect_pending_state = true` (их печатает
`--connect`). Разрыв в лобби тоже пытается восстановить транспорт, но пере-
вход в лобби не выполняется.

Тесты retry-логики не требуют сети: `ReconnectTracker` + `reconnect_with`
прогоняются с мок-коннектором (`net::tests::disconnect_retry_*`,
`reconnect_zero_attempts_is_disabled_without_connecting`,
`reconnect_attempt_timeout_is_reported`); отдельно проверяется, что
`Disconnected` в обработчике событий не паникует и корректно завершает цикл.

## Тесты

```sh
# из max-rust/; default = GUI (SDL3 из исходников через CMake) — для Docker
# нужны dev-заголовки X11/Wayland/ALSA/GL, см. docs/development/release-checklist.md §2.3;
# libSDL3.so.0 кладётся в target/debug, тестам нужен LD_LIBRARY_PATH.
docker run --rm -v "D:/dev/max-port-debug:/src" -w /src/max-rust \
  rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config \
   libasound2-dev libx11-dev libxext-dev libxrender-dev libxcursor-dev libxi-dev \
   libxrandr-dev libxinerama-dev libxss-dev libxtst-dev libxfixes-dev \
   libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols \
   libdecor-0-dev libegl-dev libgles-dev libgl-dev libgbm-dev libdrm-dev \
   libdbus-1-dev libudev-dev >/dev/null && \
   export LD_LIBRARY_PATH=/src/max-rust/target/debug && \
   cargo check && cargo test"

# только приложение; headless-сборка (без SDL) — --no-default-features
cargo test -p max_app
cargo check -p max_app --no-default-features
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

Запись ручной игры проверяется отдельно: `qa/recorder.rs` — строки команд
(`click`/`rclick`/`key`/`type`/`wait`, cap пауз, суффикс `-1`), а
`tests/qa_driver.rs::recorded_manual_session_round_trips_through_qa_run` гоняет
поток `InputEvent` → `.qa` → `qa_run` и сверяет `AppDigest` с исходным.

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
   cargo run -p max_app --no-default-features -- --connect 127.0.0.1:8082 --join <game_id> \
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
* HUD отдаёт приказы движения только наземным/морским юнитам
  (`hud::unit_can_move`): путь считается наземным `find_ground_path`, воздушным
  нужен `Paths_GetAirPath` с пиксельной геометрией — это следующий шаг.
* В сетевой сессии кнопка **End Turn** только фиксирует намерение
  (`HudState::end_turn_requested`) и логирует его: ход закрывает игровой цикл
  тем же путём (`06` + `52`), что и таймер, поэтому отдельной отправки из HUD
  нет; в демо-режиме `App::end_turn` проходит этот путь локально.
