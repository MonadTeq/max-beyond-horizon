# Чек-лист релиза Rust-клиента M.A.X.

Версия: **1.0.0** · дата среза: **2026-11-15** · тег: **v1.0.0**

Документ — исполняемая последовательность: каждый пункт либо команда с
ожидаемым результатом, либо ручная проверка с местом для свидетельства.
Релиз считается выпущенным, когда отмечены **все** пункты разделов 1–9 и
артефакты раздела 10 опубликованы.

Обозначения:

- `[ ]` — не проверено; `[x]` — проверено; `[!]` — осознанное исключение с
  причиной (записать рядом).
- `$ROOT` — корень репозитория (каталог с `max-rust/`, `assets/`,
  `max-original/`).

## 0. Предпосылки и данные

- [ ] Docker с Linux-контейнерами (`docker info`), образ
      `rust:1.94.0-bookworm` доступен (или `RUST_IMAGE` указывает на него).
- [ ] Данные оригинальной игры на месте: `max-original/MAX.RES`,
      `max-original/COLOR.PAL`, `assets/PATCHES.RES`, звуки
      `max-original/*.SPW`, карты `*.WRL` (или `--world-dir`).
- [ ] Для live-smoke: собран `max-horizon-server/target/release/horizon` и
      C++-headless `build/linux-server-template/max-port`.
- [ ] Для Windows-бинаря (кросс-сборка, §6): сеть до apt
      (`gcc-mingw-w64-x86-64`, `wine`) и crates.io; запуск из Git Bash
      (`cygpath` + `MSYS_NO_PATHCONV` — иначе Docker получает пути вида
      `C:/Program Files/Git/src/...`).
- [ ] Для Windows UI-бинаря (§6.1): VS с workload «Desktop development with
      C++» (MSVC + Windows SDK + CMake), Rust 1.94.0 и vcpkg с
      `sdl3:x64-windows`. На текущем хосте всё это уже стоит (см. таблицу в
      §6.1); MSVC ставится только установщиком VS и требует прав администратора.
- [ ] Для человеческого сценария: Linux-хост с SDL3 и аудиоустройством либо
      Windows-хост (см. §6.1), плюс запущенный Horizon + headless.
- [ ] Нет незакоммиченных изменений в оракулах: `max-cpp-port/`,
      `max-original/`, `max-original-dasm/` (релиз их не меняет).

## 1. Заморозка версии

- [ ] `git status --short` — рабочее дерево чистое (кроме `release/`, он в
      `.gitignore`).
- [ ] Версия workspace `max-rust` = `1.0.0`:

      grep -n 'version = "1.0.0"' max-rust/Cargo.toml
      grep -rn 'version.workspace = true' max-rust/crates/*/Cargo.toml

  Ожидается: `[workspace.package] version = "1.0.0"`; все пять крейтов
  (`max_core`, `max_net`, `max_assets`, `max_render`, `max_app`) наследуют её и
  не переопределяют `version`.
- [ ] `max-rust/Cargo.lock` обновлён (`cargo check` меняет версии локальных
      пакетов на 1.0.0); закоммичен вместе с релизом.
- [ ] `CHANGELOG.md` содержит секцию `[1.0.0]` с датой и ограничениями.
- [ ] `LICENSE` (MIT) на месте; ссылки на лицензию есть в `CHANGELOG.md` и
      `release/…/README.txt`.

## 2. Автоматические проверки

Все команды — через Docker, чтобы не зависеть от хостового toolchain:

```sh
# 2.1 Базовые тесты max-rust (без SDL/ALSA)
docker run --rm -v "$ROOT:/src" -w /src/max-rust rust:1.94.0-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test'

# 2.2 Звук: WAV-декодер + rodio за feature sound (нужен libasound2-dev)
docker run --rm -v "$ROOT:/src" -w /src/max-rust rust:1.94.0-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libasound2-dev >/dev/null && cargo check --features sound && cargo test --features sound'

# 2.3 SDL3 из исходников (проверка feature-сборки окна)
docker run --rm -v "$ROOT:/src" -w /src/max-rust rust:1.94.0-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libasound2-dev libx11-dev libxext-dev libxrender-dev libxcursor-dev libxi-dev libxrandr-dev libxinerama-dev libxss-dev libxtst-dev libxfixes-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols libdecor-0-dev libegl-dev libgles-dev libgl-dev libgbm-dev libdrm-dev libdbus-1-dev libudev-dev >/dev/null && cargo check --features sdl-from-source'

# 2.4 Horizon + C++-адаптер
docker run --rm -v "$ROOT/max-horizon-server:/src" -w /src rust:1.94.0-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libssl-dev >/dev/null && cargo test --workspace'

# 2.5 Полный локальный CI (2.1 + 2.4; с --smoke добавляет §3)
bash ci.sh
```

- [ ] 2.1 `cargo check` и `cargo test` — зелёные, 0 failed.
  Свидетельство: ______________________
- [ ] 2.2 `--features sound` — `check` и `test` зелёные. Тест
      `max_assets::wav::tests::parses_real_kbuy0_spw` проходит на реальном
      `KBUY0.SPW`; `all_real_spw_files_are_pcm16_22050_mono` проверяет все 272
      файла (`cargo test -p max_assets wav::`).
  Свидетельство: ______________________
- [ ] 2.3 `--features sdl-from-source` — зелёный `cargo check`.
  Свидетельство: ______________________
- [ ] 2.4 Horizon: `cargo test --workspace` — зелёный (известные падения
      `horizon_event_system` должны быть исправлены или явно исключены — M5.1).
  Свидетельство: ______________________
- [ ] 2.5 `bash ci.sh` — `CI OK` за разумное время.
  Свидетельство: ______________________

## 3. Живой smoke: 2 клиента × 10 ходов

Скрипт: `build/scripts/live-smoke.sh` (Docker: Horizon + C++-headless + два
Rust-клиента в общем network namespace).

```sh
bash build/scripts/live-smoke.sh 10
# опционально: bash ci.sh --smoke --turns 10
```

- [ ] Horizon поднялся (`horizon-live` healthy), `horizon_probe` создал игру,
      `GAME_ID` непустой.
- [ ] Оба клиента (`client-a`, `client-b`) завершились с exit 0, в логах
      `build/smoke-logs/a.log` и `b.log` нет паник/таймаутов.
- [ ] В логах обоих клиентов одинаковые CRC-барьеры (`45`) на каждом ходу;
      расхождений (desync) нет.
- [ ] Пройдено ≥ 10 ходов, `turns_finished`/`ticks` в отчёте соответствуют
      ожиданию.
- [ ] Скрипт воспроизводим: повторный запуск даёт тот же результат (не
      обязательно тот же CRC между разными seed, но без desync внутри партии).

Свидетельство (дата, GAME_ID, хвосты логов): ______________________

## 4. Человеческий сценарий M4 (SDL, мышь)

Выполняется на машине с SDL3 и (для звука) аудиоустройством. Сервер — по
`max-horizon-server/README.Docker.md`:

```sh
docker compose up -d --build          # Horizon + headless
# клиент из исходников (SDL3 из системы) либо релизный бинарь из §5:
cargo run -p max_app --features sdl -- --ui --turn-wait 60
# со звуком:
cargo run -p max_app --features sdl,sound -- --ui --turn-wait 60
```

`--turn-wait 60` обязателен для игры руками: без него сетевой цикл закрывает ход
сразу после тика (~16 мс) и клики не успевают попасть в партию — это режим для
headless/live-smoke. Если каталог данных headless-сервера без `PATCHES.RES`
(как в `docker-compose.yml`), добавьте `--handshake-patches <несуществующий
файл>`, иначе handshake вернёт `game data`. Подробности — `max_app/README.md`,
раздел «Сетевая партия в окне».

Шаги (все — только мышью, кроме ввода имени игры):

- [ ] Главное меню: видны «Create game», «Join game», «Quit»; клик по кнопке
      срабатывает (со звуком — слышен клик `KBUY0`, если собрано с `sound`).
- [ ] Create game: задать имя, планету (0–23), таймер/конец хода, число
      противников (1–3), пароль (опционально); Enter/кнопка создаёт игру.
- [ ] Игра создалась в Horizon (в логе `Created "<name>" (<id>)`), клиент
      вошёл в сессию; видна карта, HUD, миникарта, ресурсы.
- [ ] LMB по своему юниту — выделение, панель юнита заполняется.
- [ ] LMB по врагу при выделенном своём — приказ атаки (пакет 08), в журнале
      `Attack: …`.
- [ ] RMB по свободной клетке — маршрут-предпросмотр и приказ движения
      (пакет 38), юнит идёт; RMB по юниту/Esc — отмена.
- [ ] End Turn завершает ход (06+52/45), начинается следующий; таймер хода
      работает.
- [ ] Чат: Enter/T, ввод, Enter — сообщение уходит (пакет 18) и видно в
      журнале; Esc — отмена.
- [ ] Партия против AI идёт до экрана победы/поражения; HUD показывает итог,
      игра не падает.
- [ ] Меню → выход из сессии → главное меню; повторный вход в игру работает.
- [ ] Окно закрывается/Esc — корректное завершение процесса (exit 0).

Свидетельство (скриншоты/лог, дата, ОС): ______________________

## 5. Linux-бинарник

`release.sh` собирает в Docker **интерактивный** бинарь с
`--features sdl-from-source,sound` (решение зафиксировано здесь: релизный
Linux-клиент — с окном и звуком; headless-сборка для серверов остаётся
`cargo build --release -p max_app` без фич). Скрипт кладёт всё в
`release/max_app-<version>-linux-x86_64/` и `.tar.gz`.

```sh
bash release.sh --dry-run     # что и где будет собрано, без запуска Docker
bash release.sh               # реальная сборка + упаковка
# потом (после проверки артефактов):
bash release.sh --tag         # сборка + аннотированный тег v1.0.0
```

- [ ] `bash release.sh --dry-run` завершается 0 и печатает команду сборки,
      список файлов и путь `release/…`.
- [ ] `bash release.sh` завершается 0; в `release/` есть каталог и `.tar.gz`.
- [ ] В каталоге: `max_app`, `run.sh`, `README.txt`, `CHANGELOG.md`, `LICENSE`,
      `SHA256SUMS` и `lib/` (если SDL3 не статический).
- [ ] `ldd release/…/max_app` с `LD_LIBRARY_PATH=release/…/lib` не содержит
      `not found`; `libSDL3.so.0` лежит в `lib/` (или залинкован статически).
- [ ] `release/…/max_app --help` через `run.sh` запускается на чистом
      Linux-хосте (или в `ubuntu:24.04` c `libasound2`) и печатает справку:
      сырой `max_app` требует `LD_LIBRARY_PATH=…/lib`, это делает `run.sh`.
- [ ] `release/…/run.sh --demo max-original/SAVE1.CAM --turns 1` печатает
      digest (`ticks=1 … units=312`), exit 0 — headless-путь живого бинаря.
- [ ] `release/…/run.sh --ui` открывает окно и играет клики (на хосте с
      дисплеем и звуком); без дисплея — понятная ошибка SDL, не паника.
- [ ] `sha256sum -c release/…/SHA256SUMS` — все файлы `OK`.
- [ ] `tar -tzf release/max_app-1.0.0-linux-x86_64.tar.gz` содержит тот же
      набор файлов (без вложенного мусора/`target`).

Свидетельство (размер, sha256, дата): ______________________

### 5.1. Runtime-зависимости Linux-артефакта

- `libasound2` (ALSA) — для feature `sound`; без неё клиент не запустится,
  поэтому звук в релизе включается осознанно. Для headless-сборок sound не
  включается.
- `libSDL3.so.0` — из `lib/` рядом с бинарём (`run.sh` добавляет
  `LD_LIBRARY_PATH`); системный SDL3 не требуется.
- X11/Wayland — для окна; headless-режимы (`--demo`, `--new-game`,
  `--connect` без `--sdl`) работают без дисплея.

## 6. Windows x64: headless-бинарь кросс-сборкой + полный SDL на хосте

Windows-бинарник **собирается здесь** — кросс-компиляцией
`x86_64-pc-windows-gnu` (mingw-w64) в Linux-контейнере; Windows-тулчейн на
хосте не нужен. Скрипт: `build/scripts/build-windows.sh`
(описание и разбор блокеров — `build/scripts/README.md`).

Собирается **headless**-вариант (`--no-default-features`: без `sdl*`, без
`sound`) — он закрывает `--demo`, `--new-game` и `--connect` (headless-цикл
Horizon gRPC + ENet). Интерактивное окно SDL3 и звук кросс-сборкой не
получаются: `sdl3-sys` собирает SDL3 через CMake и линкует Windows-импорты, а
`rodio`/`cpal` на Windows требуют WASAPI-заголовков, — поэтому **полная
SDL-сборка остаётся на Windows-хосте** (см. ниже).

```sh
# из Git Bash / WSL / Linux; Docker + сеть (apt: mingw-w64, wine; crates.io)
bash build/scripts/build-windows.sh --dry-run   # план: команды, пути, артефакты
bash build/scripts/build-windows.sh             # сборка + проверки + упаковка
bash build/scripts/build-windows.sh --no-wine   # без wine-smoke (быстрее)
```

Внутри контейнера `rust:1.94.0-bookworm`: `gcc/binutils-mingw-w64-x86-64`
(для vendored ENet 1.3.18), `rustup target add x86_64-pc-windows-gnu` и

```sh
cargo build --release --locked --target x86_64-pc-windows-gnu \
            -p max_app --no-default-features
```

Артефакты:

```
release/max_app-1.0.0-windows-x86_64/            max_app.exe, README.txt (инструкция
                                                 SDL-сборки), LICENSE, CHANGELOG.md,
                                                 SHA256SUMS
release/max_app-1.0.0-windows-x86_64.zip         архив каталога
release/max_app-1.0.0-windows-x86_64.zip.sha256  sha256 архива
```

- [x] `bash build/scripts/build-windows.sh --dry-run` — exit 0, печатает команду
      сборки и пути артефактов. _(2026-11-15)_
- [x] Сборка `-p max_app --no-default-features` под `x86_64-pc-windows-gnu` —
      успешна. Понадобились правки **только в сборке** (игровой код не
      менялся): `max_net/build.rs` выбирает платформенный файл ENet по
      `CARGO_CFG_TARGET_OS` (а не по `cfg!(target_os)` хоста) и `tonic`
      подключён без `default-features` с явными `router,transport,codegen`
      (TLS — opt-in фича `tls`). _(2026-11-15)_
- [x] PE-заголовок: `file` → `PE32+ executable (console) x86-64`,
      `objdump -f` → `file format pei-x86-64`, первые байты `MZ`.
      Размер **8 908 438 байт** (8.5 МиБ), 10 секций,
      SHA256 `15cd9fa29dbec8cb930819708f0ea85a60b4fc3aff059c26941718bf7e8a2234`.
      _(2026-11-15; хеш зависит от снапшота `rust:1.94.0-bookworm` — mingw-версии
      из apt влияют на код ENet)_
- [x] Импорты только системные: `kernel32`, `msvcrt`, `ntdll`, `ws2_32`,
      `winmm`, `userenv`, `bcryptprimitives`,
      `api-ms-win-core-synch-l1-2-0` — нет `libgcc_s_seh-1.dll` и
      `libwinpthread-1.dll`, то есть mingw-рантайм рядом с exe не нужен.
      _(2026-11-15)_
- [x] `max_app.exe --help` и `max_app.exe --demo SAVE1.CAM --turns 1` под
      **wine 8.0** в контейнере: справка печатается, demo даёт
      `ticks=1 … units=312`, оба exit 0 (лог:
      `build/smoke-logs/windows-wine-smoke.log`, вердикт
      `WINE: ok (…; шим bcryptprimitives: да)`). _(2026-11-15)_
      Оговорка: `std` Rust импортирует `bcryptprimitives!ProcessPrng`, а в wine
      8.0 из Debian bookworm этой DLL нет — первый запуск падает на загрузке
      (exit 53, `Importing dlls failed, status c0000135`). Скрипт сначала
      пробует exe «как есть», и только если не вышло — подкладывает рядом с
      копией exe **тестовый** шим `ProcessPrng` → `advapi32!SystemFunction036`.
      В архив шим не входит: на настоящей Windows `bcryptprimitives.dll`
      системная (поэтому отдельный пункт ниже — прогон на реальной Windows).
- [x] `release/max_app-1.0.0-windows-x86_64/` содержит `max_app.exe`,
      `README.txt`, `LICENSE`, `CHANGELOG.md`, `SHA256SUMS`;
      `release/max_app-1.0.0-windows-x86_64.zip` (3 255 566 байт) и
      `.zip.sha256` собраны, `sha256sum -c SHA256SUMS` в каталоге — все файлы
      `OK`. _(2026-11-15; sha256 zip зависит от mtime файлов, актуальный — в
      `.zip.sha256`)_
- [ ] `--connect HOST:8082 --join GAME_ID` с этим exe против живого Horizon
      (headless-цикл gRPC+ENet на Windows-бинаре) — прогон на Windows-хосте
      или wine с работающим Horizon в docker compose.
- [ ] Headless-бинарь запущен на **настоящей** Windows 10/11 x64 (`--help`,
      `--demo`, `--new-game`) — подтверждение, что mingw-сборка не зависит от
      wine-шимов.

### 6.1. Полная сборка с окном SDL3 и звуком (Windows-хост)

Скрипт: `build/scripts/build-windows-ui.ps1` (PowerShell 5.1+, без прав
администратора). Он сам находит VS через `vswhere`, vcpkg с `sdl3:x64-windows`,
поднимает окружение `vcvars64` и собирает

```sh
cargo build --release --locked -p max_app --features sdl-vcpkg,sound
```

```powershell
powershell -ExecutionPolicy Bypass -File build\scripts\build-windows-ui.ps1
# только собрать, без упаковки:
powershell -ExecutionPolicy Bypass -File build\scripts\build-windows-ui.ps1 -NoPackage
```

Артефакты: `release/max_app-1.0.0-windows-x86_64-ui/` (`max_app.exe`,
`SDL3.dll`, `README.txt`, `LICENSE`, `CHANGELOG.md`, `SHA256SUMS`) и
`release/max_app-1.0.0-windows-x86_64-ui.zip` + `.zip.sha256`. Сборка идёт в
`build/win-msvc-target/`, чтобы не смешиваться с Linux/Docker-артефактами
`max-rust/target/`.

Тулчейн на этом хосте **уже установлен** (всё, кроме VS, ставится без
администратора):

| Компонент | Где | Как проверено |
|---|---|---|
| Visual Studio Community 2026 (MSVC 14.51, Windows SDK 10.0.26100, CMake) | `D:\Program Files\Microsoft Visual Studio\18\Community`, SDK `D:\Windows Kits\10` | `vswhere -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64` → путь; `cl.exe`, `link.exe`, `vcvars64.bat`, `kernel32.lib` на месте |
| Rust 1.94.0 + target `x86_64-pc-windows-msvc` | `%USERPROFILE%\.cargo\bin` (rustup, `--no-modify-path`) | `rustc -vV` → host `x86_64-pc-windows-msvc` |
| vcpkg + `sdl3:x64-windows` 3.4.20 | `D:\dev\toolchain\vcpkg` | `installed\x64-windows\lib\SDL3.lib`, `bin\SDL3.dll` |

Переустановка при необходимости:

```powershell
# Rust (user-level, без администратора)
Invoke-WebRequest https://win.rustup.rs/x86_64 -OutFile $env:TEMP\rustup-init.exe
& $env:TEMP\rustup-init.exe -y --default-toolchain 1.94.0 --profile minimal

# SDL3 (vcpkg тоже user-level)
git clone --depth 1 https://github.com/microsoft/vcpkg D:\dev\toolchain\vcpkg
D:\dev\toolchain\vcpkg\bootstrap-vcpkg.bat -disableMetrics
D:\dev\toolchain\vcpkg\vcpkg.exe install sdl3:x64-windows

# MSVC + Windows SDK: только через установщик Visual Studio
# (workload "Desktop development with C++") — требует прав администратора
```

Два подводных камня, из-за которых «просто cargo build» в cmd не работает
(учтены в скрипте):

- `set VAR=value && …` в cmd записывает в значение **пробел** перед `&&`
  (`CARGO_TARGET_DIR=…\win-msvc-target ` → «The system cannot find the path»);
  нужно `set "VAR=value"`.
- `link.exe` не находит `SDL3.lib`: `sdl3-sys` с vcpkg линкует `SDL3.lib`, но не
  добавляет путь поиска, поэтому в `LIB` дописывается
  `<vcpkg>\installed\x64-windows\lib`. Плюс `SDL3.dll` должен лежать рядом с
  exe (скрипт копирует).

- [x] `cargo build --release -p max_app --features sdl-vcpkg,sound` — успешно
      (MSVC; `max_app.exe` 5 256 192 байта,
      sha256 `816d90e2162ba227093feeccc70923162b0fee03cb5de7fb06cf4bee653722f5` —
      сборка с `--turn-wait`). _(2026-11-15)_
- [x] `SDL3.dll` лежит рядом с `max_app.exe`; `max_app.exe --help` работает
      (rc=0). _(2026-11-15)_
- [x] `--ui` открывает окно: процесс живёт, `MainWindowTitle` =
      `M.A.X. — Rust client (M4.2 HUD)`, в логе `UI: окно 512x384; меню — …`.
      Звук — `backend=rodio/cpal` (WASAPI, отдельных DLL не нужно), 272 файла
      `*.SPW`, 607 ссылок. _(2026-11-15)_
- [x] Текст в окне читается слева направо. При первом запуске он был
      **зеркальным** ('N' → 'И', 'e' → 'ǝ', 't' → 'ʇ'): `max_render/src/text.rs`
      читал биты глифа MSB-первым, а таблица font8x8 — LSB-первая (бит 0 =
      левый пиксель). Исправлено (`0x01 << column`), добавлен тест
      `text::tests::glyph_bits_are_lsb_first`; проверено визуально по скриншоту
      окна: `NEW GAME ? settings`, `Name: …`, `World: 23`, `Back`/`Create`,
      `Configure the game and press Enter`. _(2026-11-15)_
- [x] Бинарь и DLL заархивированы в
      `release/max_app-1.0.0-windows-x86_64-ui.zip` (sha256 в `.zip.sha256`) —
      отдельно от headless-zip из §6. _(2026-11-15)_
- [x] **Сетевая партия 2 игрока в окне** (`--ui --turn-wait 45`,
      `docker compose up -d --build` + игра создана `horizon_probe`): два клиента,
      team 0/1, `local_team` и `team_types=[Player, Remote, None, None]`;
      **6 ходов** в lockstep (`turn_counter=1…6` совпадают у обоих),
      приказы движения `38` доходят до сервера (`ai_log.txt`),
      чат `18` доставляется пиру (`RED: hi bob` в логе bob),
      `End Turn` → `06 + 52`, барьеры `45/53/54` без таймаутов,
      `barrier_timeouts=0`, десинков (`CRC 45 … десинк`) — 0.
      Отмечено: `--turn-wait` добавлен именно по итогам этого прогона — без него
      ход закрывался за один кадр (~16 мс). _(2026-11-15)_
- [x] Найдено и обойдено при прогоне: (1) UI `Create game` получает
      `connect timed out after 10s`, пока сессия `max-port` поднимается
      (~10–15 с) — лечится «создать → подождать → Join»; (2) без
      `--handshake-patches <несуществующий>` сервер отвечает
      `handshake rejected: game data (reason GameData)` — в данных headless нет
      `PATCHES.RES`. Оба пункта описаны в `max_app/README.md`. _(2026-11-15)_
- [ ] Партия в окне без обходных флагов: `Create game` → сессия сразу (нужен
      ретрай/прогрев в клиенте) и дефолтный `data_hash`, совпадающий с сервером.
- [ ] Полный сценарий §4 (меню → create → бой/чат/End Turn до победы) на живом
      сервере, без ручных флагов совместимости.

## 7. Документация и лицензии

- [ ] `README.md` (корень): раздел про Rust-клиент ссылается на
      `docs/release-checklist.md`, `release.sh` и звук.
- [ ] `max-rust/README.md`: версия 1.0.0, feature `sound`, ссылка на чек-лист.
- [ ] `CHANGELOG.md`: 1.0.0, список возможностей и ограничений, ссылка на
      `LICENSE`.
- [ ] `docs/release-checklist.md` (этот файл) актуален: команды и пути
      совпадают с `release.sh`/`ci.sh`/`build/scripts/build-windows.sh`.
- [ ] `build/scripts/README.md` описывает релизные скрипты (`build-windows.sh`
      — Windows-кросс-сборка, `live-smoke.sh`) и обойдённые блокеры
      (ENet-платформа в `build.rs`, TLS-фича `tonic`).
- [ ] `LICENSE` (MIT) присутствует в корне и в архиве релиза; в `README.txt`
      архива — копирайт Interplay на данные игры.

## 8. Git tag v1.0.0

Тег создаётся только после зелёных §2–§5 (и §6, если Windows-бинарь входит в
релиз). `release.sh --tag` делает это автоматически; вручную:

```sh
git status --short                  # пусто
git tag -l 'v1.0.0'                 # пусто — тег ещё не создан
git tag -a v1.0.0 -m "M.A.X. Rust client 1.0.0"
git show --stat v1.0.0
git push origin v1.0.0              # публикация тега (по решению релиз-менеджера)
```

- [ ] Тег `v1.0.0` аннотированный, указывает на релизный коммит.
- [ ] `git push origin v1.0.0` выполнен (или релиз готов к пушу).
- [ ] GitHub Release создан из тега; к нему приложены
      `max_app-1.0.0-linux-x86_64.tar.gz`,
      `max_app-1.0.0-windows-x86_64.zip` (+ `.sha256`) и, если собран на
      Windows-хосте, `…-windows-x86_64-ui.zip`.

## 9. Архив артефактов

- [ ] `release/max_app-1.0.0-linux-x86_64.tar.gz` + `SHA256SUMS` сохранены.
- [ ] `release/max_app-1.0.0-windows-x86_64.zip` + `.zip.sha256` сохранены
      (продукт `build/scripts/build-windows.sh`); headless-каталог
      `release/max_app-1.0.0-windows-x86_64/` тоже несёт свой `SHA256SUMS`.
- [ ] `release/max_app-1.0.0-windows-x86_64-ui.zip` + `.zip.sha256` сохранены
      (продукт `build/scripts/build-windows-ui.ps1`; в архиве `max_app.exe` +
      `SDL3.dll`).
- [ ] Логи гейтов: `max-rust/target/cargo-test.log` (если писали),
      `build/smoke-logs/*.log`, `build/smoke-logs/windows-wine-smoke.log`,
      скриншоты M4 — приложены к релизной записи.
- [ ] Артефакты **не** закоммичены в git (`release/` в `.gitignore`).

## 10. После релиза

- [ ] Обновить `PLAN-v7-release.md`: M6.2/M6.4 отмечены, известные хвосты
      (M6.1-финал, FLC, звуки юнитов) перенесены в следующий milestone.
- [ ] Открыть issue на ограничения из `CHANGELOG.md` §«Ограничения 1.0.0».
- [ ] Проверить, что `main` после тега собирается `bash ci.sh`.
- [ ] Пост-релизная правка, попавшая в Windows UI-бинарь и **ещё не** в
      Linux-артефакт: `max_render/src/text.rs` — бит 0 = левый пиксель
      (текст был зеркальным во всём UI). Нужно: запись в `CHANGELOG.md` для
      1.0.1 и пересборка `release/max_app-1.0.0-linux-x86_64.tar.gz`
      (`bash release.sh`) — в текущем Linux-архиве текст ещё зеркальный.
      Headless-сборки (Windows-gnu) текст не рисуют, их пересборка не нужна.

---

## Приложение A. Быстрый прогон (минимальный релиз-гейт)

```sh
bash ci.sh                                  # 2.1 + 2.4
bash build/scripts/live-smoke.sh 10         # §3
bash build/scripts/build-windows.sh         # §6, Windows headless + zip + wine-smoke
bash release.sh --dry-run                   # §5, проверка плана
bash release.sh                             # §5, артефакты
```

## Приложение B. Свидетельства

| Гейт | Дата | Команда/стенд | Результат | Ссылка |
|---|---|---|---|---|
| Тесты max-rust | 2026-11-15 | `cargo check && cargo test` в `rust:1.94.0-bookworm` после кросс-правок; плюс `cargo check --features sound` и `cargo check -p max_net --features tls` | 500 тестов, 0 failed; check'и OK | `max-rust/target/cargo-test.log` |
| Тесты Horizon | | | | |
| Live smoke 2×10 | | | | |
| M4 вручную | | | | |
| Linux-бинарь | | | | |
| Windows-бинарь (headless, кросс-сборка) | 2026-11-15 | `bash build/scripts/build-windows.sh` (Docker, `rust:1.94.0-bookworm`, mingw-w64 + wine 8.0) | PE32+ x86-64, 8 908 438 Б, sha256 `15cd9fa2…`; `--help`/`--demo` под wine 8.0 — exit 0; zip 3 255 566 Б + `.zip.sha256` | `release/max_app-1.0.0-windows-x86_64.zip`, `build/smoke-logs/windows-wine-smoke.log` |
| Windows-бинарь (SDL3 + звук, хост) | 2026-11-15 | `powershell -File build\scripts\build-windows-ui.ps1` (MSVC 14.51 + Windows SDK 10.0.26100, SDL3 3.4.20 из vcpkg) | `max_app.exe` 5 256 192 Б, sha256 `816d90e2…`; окно `M.A.X. — Rust client (M4.2 HUD)` открывается, текст не зеркальный, звук `backend=rodio/cpal`; zip + `.zip.sha256` | `release/max_app-1.0.0-windows-x86_64-ui.zip` |
| Сетевая партия 2×UI | 2026-11-15 | 2× `max_app.exe --ui --turn-wait 45` (MSVC-сборка) + Horizon + C++ headless (`docker compose up -d --build`) | 6 ходов в lockstep, приказы `38` и чат `18` доходят до сервера и пира, `End Turn` → `06+52`, таймаутов/десинков 0 | `build/ui-alice.log`, `build/ui-bob.log`, `build/final-a.png` |
| Windows-бинарь (SDL, хост) | | | | |
| Тег v1.0.0 | | | | |
