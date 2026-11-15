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

- [x] 2.1 `cargo check` и `cargo test` — зелёные, 0 failed.
  Свидетельство: **615 тестов, 0 failed**, `cargo check` — 0 ошибок и 0
  предупреждений (2026-11-15, `rust:1.94.0-bookworm`, M-SP 12 +
  M-HS 13 в `tests/hot_seat.rs` + 5 модульных в `hotseat.rs`); лог
  `build/cargo-docker-hotseat-final.log`.
- [ ] 2.2 `--features sound` — `check` и `test` зелёные. Тест
      `max_assets::wav::tests::parses_real_kbuy0_spw` проходит на реальном
      `KBUY0.SPW`; `all_real_spw_files_are_pcm16_22050_mono` проверяет все 272
      файла (`cargo test -p max_assets wav::`).
  Свидетельство: ______________________
- [x] 2.3 `--features sdl-from-source` — зелёный `cargo check`.
  Свидетельство: 0 ошибок (Linux, полный X11/ALSA dev-набор, 2026-11-15,
  `max_app` с `frame_scale`/`SdlWindow::new_scaled`/`window_to_frame`);
  лог `build/cargo-docker-ef-sdl-final.log`.
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

- [ ] Главное меню: видны кнопки оригинала `New Game`, `Load Game`,
      `Multiplayer Game`, `Setup`, `Introduction`, `Credits`, `Exit` на фоне
      `MAINPIC` (кнопки `BLANK_*`/`SBLNK_*`, подписи GNW `FONT1`); клик по
      `New Game`/`Multiplayer Game`/`Exit` срабатывает (со звуком — слышен клик
      `KBUY0`, если собрано с `sound`); остальные кнопки пишут статус «screen is
      not ported yet».
- [ ] Create game: форма на `PLANETSE`, большая картинка выбранной планеты
      (26,98), **сетка планет 8×3** (картинка `SNOW_PIC + world/6` + имя под
      ней; числовое поле World остаётся fallback'ом), поля 3×2, кнопки
      `Random`/`Cancel`/`?`/`Done` (`MNUBTN4/5/6`) в y=438; Enter/`Done`
      создаёт игру.
- [ ] Масштаб окна: при `--scale 2` окно 1280×960, картинка целочисленно
      увеличена (каждый пиксель 2×2, без мыла), клик по `New Game` в окне
      открывает форму (обратный пересчёт координат); окно можно ресайзить,
      при некратном размере — чёрные поля.
- [ ] Игра создалась в Horizon (в логе `Created "<name>" (<id>)`), клиент
      вошёл в сессию; видна карта, HUD, миникарта, ресурсы.
- [ ] Миникарта: видны **маркеры ресурсов** (`RAWMSK*`/`FUELMK*`/`GOLDMK*`, не
      точки), **FOV-маска** (неисследованные клетки темнее), красная **рамка
      камеры**, точки юнитов; кнопка `2x` увеличивает область вокруг центра,
      `TNT` оставляет только стреляющих юнитов; клик по миникарте центрирует
      камеру (в 1x и 2x).
- [ ] Курсор мыши: над меню/панелью — `HANDPTR`, над картой без выделения —
      `MAP_PTR`, над своим юнитом — `FRND_PTR`, над врагом — `ENMY_PTR`,
      при приказе — `UNIT_GO`/`UNIT_NGO` (SDL, `--features sdl`).
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

### 4.1. Hot-seat (M-HS) руками, без сети

Локальная партия, сервер не нужен (`cargo run -p max_app --features sdl -- --ui`).

- [ ] Главное меню → `Multiplayer Game` → экран `Multiplayer Menu` (фон
      `MAINPIC`, заголовок (236,145), пять больших кнопок `BLANK_*` и `Cancel`
      `SBLNK_*`); кнопка `Hot Seat Game` открывает настройку.
- [ ] Экран команд озаглавлен `Hot Seat Menu`; по умолчанию RED — `Human`,
      GREEN — `Human`, BLUE/GRAY — `None`; клик `Human` на BLUE добавляет
      третьего человека (флаг у RED **не** снимается).
- [ ] `Done` при одном человеке — сообщение `There must be at least two human
      players to begin a hot seat game.`; при двух и более — партия стартует.
- [ ] Первый кадр партии — поверх карты модальное окно `HELPFRAM` с текстом
      `Red Team:` / `Begin turn.` и кнопкой `OK` (позиция (253,126), кнопка
      (373,319)); клик по карте/панелям игнорируется, таймер хода стоит.
- [ ] Enter / Esc / клик по `OK` закрывает окно; таймер хода пошёл.
- [ ] Ввод работает только для активной команды: клик по чужому юниту его не
      выделяет, приказ чужому не уходит; `End Turn` передаёт ход следующему
      человеку и снова показывает окно (`Green Team:` и т.д.).
- [ ] Если в смеси есть компьютер, его ход проходит сам (без окна), после него
      окно показывается следующему человеку.
- [ ] Таймер `Turn Limit` действует на каждую человеческую команду отдельно и
      закрывает ход сам.
- [ ] Партия доходит до экрана победы/поражения без падений.

Свидетельство: `build/hs-players.png`, `build/hs-turn.png`, `build/hs-game.png`,
`build/hs-ai.png`; дата/ОС: ______________________

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
      (MSVC; `max_app.exe` 5 423 616 байт,
      sha256 `279e472dcc41e1747548a7e38d9c1b98738e861db913b9f1d1228ac858ff23c9` —
      сборка с меню E на `MAINPIC`/`PLANETSE`/`MULTGAME`, масштабом F
      (`FrameScale`, ресайз, `--scale`), панелями 1:1, GNW-шрифтами,
      маркерами C/D и курсорами). _(2026-11-15)_
- [x] `SDL3.dll` лежит рядом с `max_app.exe`; `max_app.exe --help` работает
      (rc=0), в справке есть `--scale N`. _(2026-11-15)_
- [x] `--ui` открывает окно: при `--scale 1` клиент 640×480, при `--scale 2` —
      1280×960 (кадр целочисленно увеличен, каждый пиксель 2×2); клик
      `New Game` в окне 2× открывает форму (обратный пересчёт координат).
      Живые кадры: `build/frame-ef-main-windows.png`,
      `build/frame-ef-create-windows.png`,
      `build/frame-ef-main-windows-2x.png`,
      `build/frame-ef-create-windows-2x.png`; скрипт `build/capture-ef.ps1`.
      _(2026-11-15)_
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
- [x] **Панели 1:1 и шрифты GNW (пункты A/B `docs/visual-1to1-status.md`)**:
      `FILES_OF/ON`, `PREF_OFF/ON`, `ENDTRN_U`, `TURNS`, `TIMER`, `XYPOS`,
      `UNITNAME`, 9 переключателей `*_OFF/ON`, `ZOOMPNL1`/`ZOOMPTR`, портрет
      (кадр спрайта ×2), миникарта из `.WRL`, миникарта `2x`/`TNT`; текст —
      оригинальные `FONT1/2/5.FON`, сверенные с FreeType-путём C++-порта
      побайтово для ASCII. Кадр: `build/frame-panels.png`; сборка артефакта
      отрисовывает его же (`build/frame-windows-artifact.png`). _(2026-11-15)_
- [x] **Координаты мыши (риск 4)**: при хроме клик переводится из окна в
      область карты (`panels::screen_to_map`, −180/−18), камера карты 448×448
      одна для рендера и ввода (`App::map_viewport`); клики по панелям
      обрабатываются до карты, End Turn не проваливается в карту. Тесты:
      `chrome_click_on_the_map_centre_selects_the_unit_under_it`,
      `chrome_end_turn_click_is_a_button_not_a_map_click`,
      `chrome_toggle_and_zoom_clicks_work_on_the_panels`. _(2026-11-15)_
- [x] Бинарь и DLL заархивированы в
      `release/max_app-1.0.0-windows-x86_64-ui.zip` (3 065 885 Б, sha256
      `d7415687a5d70381d30bf7ccd1bb2f67dc8afcabea993775fe22aca22b91f0d0`,
      `.zip.sha256`) — отдельно от headless-zip из §6. _(2026-11-15)_
- [x] **M-HS: пересборка Windows UI с hot-seat** (окно `DialogMenu` на
      `HELPFRAM`, `Hot Seat Menu`, `Multiplayer Menu`):
      `cargo build --release -p max_app --features sdl-vcpkg,sound` —
      `max_app.exe` 5 572 096 Б, sha256
      `7af62ac1175886ee775b77e0417736b51acaa36f3ca60ae8703e7467ef11bc42`;
      zip `release/max_app-1.0.0-windows-x86_64-ui.zip` sha256
      `9ce01c0a03eeb49557055882baca8e27b49129bb1c4c92844dd2ac362a6a2c0e`
      (меняется от mtime упаковки; exe-хеш стабилен);
      `max_app.exe --help` — rc=0; в бинаре есть строки `Hot Seat Game`,
      `Hot Seat Menu`, `Begin turn.`, `HELPFRAM`, `HELPOK_U` (проверка
      `build/exe_hs_probe.py`). Лог `build/win-ui-hotseat-final.log`.
      _(2026-11-15)_
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
| Тесты max-rust | 2026-11-15 | `docker run --rm -v "$ROOT:/src" -w /src/max-rust rust:1.94.0-bookworm bash -c "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test"`; плюс `cargo check -p max_app --features sdl-from-source` (полный X11/ALSA dev-набор) | **615 тестов, 0 failed**; `cargo check` — 0 ошибок; SDL-сборка — 0 ошибок | `build/cargo-docker-hotseat-final.log`, `build/cargo-docker-ef-sdl-final.log` |
| M-HS (hot-seat) | 2026-11-15 | та же команда Docker; `cargo test -p max_app --test hot_seat`; `cargo run -p max_app --example sp_frames -- ../max-original ../build`; `build\scripts\build-windows-ui.ps1` | 13 тестов hot-seat (2 и 4 человека, ИИ между людьми, окно `DialogMenu` 1:1, таймер до подтверждения, только свои юниты); 5 кадров; Windows UI exe 5 572 096 Б sha256 `7af62ac1…` | `build/hs-{players,turn,turn2,game,ai}.png`, `build/cargo-docker-hotseat-final.log`, `release/max_app-1.0.0-windows-x86_64-ui.zip` |
| Тесты Horizon | | | | |
| Live smoke 2×10 | | | | |
| M4 вручную | | | | |
| Linux-бинарь | | | | |
| Windows-бинарь (headless, кросс-сборка) | 2026-11-15 | `bash build/scripts/build-windows.sh` (Docker, `rust:1.94.0-bookworm`, mingw-w64 + wine 8.0) | PE32+ x86-64, 8 908 438 Б, sha256 `15cd9fa2…`; `--help`/`--demo` под wine 8.0 — exit 0; zip 3 255 566 Б + `.zip.sha256` | `release/max_app-1.0.0-windows-x86_64.zip`, `build/smoke-logs/windows-wine-smoke.log` |
| Windows-бинарь (SDL3 + звук, хост) | 2026-11-15 | `powershell -File build\scripts\build-windows-ui.ps1` (MSVC 14.51 + Windows SDK 10.0.26100, SDL3 3.4.20 из vcpkg) | `max_app.exe` 5 423 616 Б, sha256 `279e472d…`; окно открывается, `--scale 1/2` = 640×480/1280×960, клик 2× работает, звук `backend=rodio/cpal`; zip 3 065 885 Б, sha256 `d7415687…` + `.zip.sha256`; `--new-game --width 640 --height 480` отработал | `release/max_app-1.0.0-windows-x86_64-ui.zip`, `build/frame-ef-create-windows-2x.png` |
| Визуал: меню E и масштаб F | 2026-11-15 | `cargo run -p max_app --example visual_ef -- ../build/frame-ef`; `max_app.exe --ui --scale 1/2` + `build/capture-ef.ps1`; `cargo test -p max_app -p max_render -p max_assets` | главное меню `MAINPIC` + `BLANK_*`, форма `PLANETSE` + `MNUBTN4/5/6` + сетка 8×3, список `MULTGAME` + `BTNHIDE`; `FrameScale` 2× — каждый пиксель блоком 2×2, клик в окне 1280×960 попадает в ту же клетку карты (56,55); декодер `SNOW_PIC` исправлен (планета 0..5 больше не чёрная); +17 тестов (576 всего) | `build/frame-ef-main.png`, `build/frame-ef-create.png`, `build/frame-ef-join.png`, `build/frame-ef-main-2x.png`, `build/frame-ef-*-windows*.png` |
| Визуал: окно 1:1 (FRAMEPIC) | 2026-11-15 | `max_app.exe --new-game --width 640 --height 480 --out frame.ppm`; живая сессия `--ui` против Horizon+headless | FRAMEPIC (640×480, своя палитра, RLE с чанками через строку) + раскладка C++-порта: карта (180,18) 448×448, миникарта (15,356), кнопки End Turn/Files/Prefs; в сессии процедурный HUD не рисуется | `build/frame-chrome2.png`, `build/ui-chrome-session-final.png`, `build/ui-framepic.png` |
| Визуал: панели и шрифт GNW (A/B) | 2026-11-15 | `max_app.exe --new-game --width 640 --height 480 --out build/frame-windows-artifact.ppm`; `cargo run -p max_app --example panel_frame -- ../build/frame-panels.ppm`; `tools/font_vectors/run.sh` | верхние кнопки/счётчики, портрет (спрайт ×2), 4 строки статистики, 9 переключателей, зум, координаты/описание, миникарта из `.WRL`; `FONT1/2/5.FON` совпали с FreeType-путём C++-порта побайтово для ASCII; Windows- и Linux-кадры идентичны | `build/frame-newgame-panels.png`, `build/frame-panels.png`, `build/frame-panels-left.png`, `build/frame-windows-artifact.png`, `max-rust/tools/font_vectors/render.txt` |
| Визуал: тайлы и здания | 2026-11-15 | `cargo run -p max_render --example terrain -- ../max-original/SNOW_1.WRL out`; `max_app.exe --new-game --out frame.ppm` | 337 тайлов/12 544 клетки SNOW_1 рендерятся палитрой `.WRL` (снимки `build/wrl-snow-*.png`), спрайтов грузится 43 (здания и alien-обломки видны вместо маркеров) | `build/wrl-snow-map.png`, `build/frame-original-terrain.png` |
| Визуал: миникарта/иконки/курсоры/планеты (C/D) | 2026-11-15 | `cargo run -p max_app --example visual_cd -- ../build/frame-cd`; `cargo test -p max_assets -p max_app --lib` | 30/30 курсоров декодированы (размеры/hotspot/кадры сверены с C++), 99 маркеров ресурсов, миникарта с FOV-маской/рамкой камеры/точками юнитов/2x/TNT, сетка 24 планет `SNOW_PIC + world/6` с числовым fallback; +20 тестов (576 всего) | `build/frame-cd-game.png`, `build/frame-cd-game-2x.png`, `build/frame-cd-game-minimap-4x.png`, `build/frame-cd-planets.png`, `build/frame-cd-cursors.png` |
| Сетевая партия 2×UI | 2026-11-15 | 2× `max_app.exe --ui --turn-wait 45` (MSVC-сборка) + Horizon + C++ headless (`docker compose up -d --build`) | 6 ходов в lockstep, приказы `38` и чат `18` доходят до сервера и пира, `End Turn` → `06+52`, таймаутов/десинков 0 | `build/ui-alice.log`, `build/ui-bob.log`, `build/final-a.png` |
| Windows-бинарь (SDL, хост) | | | | |
| Тег v1.0.0 | | | | |

## Приложение C. M-SP (локальная партия против ИИ)

| Гейт | Команда | Результат | Ссылка |
|---|---|---|---|
| Настройка 1:1 | `cargo run -p max_app --example sp_frames` | `OPTNFRM`/`PLANETSE`/`CHOSPLYR` кадры | `build/sp-*.png` |
| Локальный цикл | `cargo test -p max_app --test local_sp` | 12 тестов | — |
| Windows UI | `build\scripts\build-windows-ui.ps1` | zip + sha256 | `release/max_app-1.0.0-windows-x86_64-ui.zip` |
| Декодер big image | `cargo test -p max_assets` | RLE с 776 | — |

## Приложение D. M-HS (hot-seat, 2–4 человека на одном ПК)

Оракул: `GameManager_UpdateHumanPlayerCount` (`game_manager.cpp:3267`) — при
`> 1` человека игра принудительно turn-based; `GameManager_UpdateGui` (`:2437`)
на смене человеческой команды показывает модальный `DialogMenu`
(`GameManager_UpdateGuiControl`, `:2262`): текст `_(c2eb)` = `"%s:\nBegin turn."`
с именем команды `_(f394)`…, фон `HELPFRAM` (301×231, центр `WINDOW_MAIN_MAP` →
(253,126)), шрифт `GNW_TEXT_FONT_5` цвет `GNW_TEXT_OUTLINE | 0xFF`, кнопка
`HELPOK_U/D` 65×24 в (120,193) с подписью `OK`; закрытие — клик по `OK`,
Enter или Esc. **Другого экрана перехода в оригинале нет.**

| Гейт | Команда | Результат | Ссылка |
|---|---|---|---|
| Окно перехода 1:1 | `cargo test -p max_app --test hot_seat` | 13 тестов: форма `Hot Seat Menu`, `_(06ec)`, окно у первого игрока и на каждой смене, таймер стоит до подтверждения | `build/cargo-docker-hotseat-final.log` |
| Ходы 2 человек | там же (`two_humans_alternate_with_the_begin_turn_dialog`) | RED → GREEN, `turn_counter` 1 → 2, `player_team` переключается | — |
| Ходы 4 человек | там же (`four_humans_all_play_in_team_order`) | RED → GREEN → BLUE → GRAY | — |
| ИИ между людьми | там же (`ai_plays_between_two_humans`, `input_is_ignored_during_the_ai_turn`) | ход компьютера идёт сам, без окна; ввод заблокирован | — |
| Управление только своей командой | там же (`only_the_active_human_controls_their_units`) | чужой юнит не выделяется, приказы чужой команды не меняются, выделение сбрасывается при передаче хода | — |
| Кадры | `cargo run -p max_app --example sp_frames -- ../max-original ../build` | `Hot Seat Menu`, окно «Red Team: Begin turn.», окно GREEN, ход GREEN, окно RED после хода ИИ | `build/hs-players.png`, `build/hs-turn.png`, `build/hs-turn2.png`, `build/hs-game.png`, `build/hs-ai.png` |
| Windows UI | `build\scripts\build-windows-ui.ps1` | zip + sha256 | `release/max_app-1.0.0-windows-x86_64-ui.zip` |
| Вход в режим | `Screen::MultiplayerMenu` (`menu.cpp:2361`) | «Multiplayer Game» → `Hot Seat Game` → Options → Planet → Players | `build/hs-players.png` |

Ограничения M-HS: `Hot Seat Scenarios` и `Load Hot Seat Game` не портированы
(кнопки пишут статус), кланы — `CLN0LOGO`; запись V70 отсутствует (общий пункт
M-SP), контейнер `MXLS` — версия 2 с флагом `hot_seat`.

