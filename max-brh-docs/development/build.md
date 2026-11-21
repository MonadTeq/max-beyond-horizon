# Сборка, запуск, инструменты и библиотеки

## Термины (сборка и артефакты)

Документация держит один набор терминов; `headless`, `UI` и `Console` не переводятся.

| Термин | Что значит |
|---|---|
| **headless-сборка** | артефакт `max_app-<ver>-windows-x86_64` (+ `.zip`, `.zip.sha256`): клиент без окна и звука (`--no-default-features`, без фич `sdl*`/`sound`), PE **Subsystem=3** (Console); режимы `--demo`, `--new-game`, `--connect` |
| **UI-сборка** | артефакт `max_app-<ver>-windows-x86_64-ui` (+ `.zip`, `.zip.sha256`): клиент с окном SDL3 и звуком (фичи `sdl-vcpkg,sound`), PE **Subsystem=2** (Windows GUI) |
| **нативная MSVC-сборка** | сборка на Windows-хосте тулчейном MSVC (`vswhere` → `vcvars64`), target `x86_64-pc-windows-msvc`; Docker, mingw-w64 и wine не участвуют |
| **кросс-сборка** | сборка под чужую платформу; в актуальных инструкциях не используется — только в исторических записях (`x86_64-pc-windows-gnu`, mingw) и для Linux/Docker-сборок |
| **бинарь** | файл `max-brh.exe` (`max-brh` на Linux); каталог и архив — «сборка»/«артефакт», процесс — «сборка» |
| **GUI-режим** | поведение клиента по умолчанию (`default = ["sdl-from-source"]`): окно SDL3; не путать с «UI-сборкой» — это артефакт для Windows со `SDL3.dll` |
| **Subsystem=3 / 2** | поле PE `Subsystem`: 3 = Console (stdout в терминал), 2 = Windows GUI (консоли нет, `--help` виден только при перенаправлении) |

## Необходимые инструменты

### Разработка (хост, Windows)
| Инструмент | Версия | Зачем |
|---|---|---|
| Rust (rustup) | 1.94.0 | сборка max-brh |
| MSVC (VS Build Tools 2022+/VS 2026) + Windows SDK + CMake | — | Windows-сборка SDL3 (default `sdl-from-source`: `cargo run -p max_app` собирает SDL3 из исходников через CMake) |
| vcpkg + `sdl3:x64-windows` | SDL3 3.4.20 | опционально: окно на Windows без CMake-сборки SDL3 (`--features sdl-vcpkg --no-default-features`); нужен для `build-windows-ui.ps1` |
| Docker (Desktop) | — | Linux-сборки, сервер, CI |
| Python 3 | — | эталонные парсеры/утилиты |
| Git | — | репозиторий |

### Docker-образы
| Образ | Назначение |
|---|---|
| `rust:1.94.0-bookworm` | сборка/тесты max-brh и max-brh-server |
| `ubuntu:24.04` | рантайм сервера, radare2, вспомогательные |
| `max-port-build:latest` | сборка C++-headless (Linux) |
| `max-ghidra`, `max-dosbox` | RE (по необходимости) |

## Сборка

### Rust-клиент (все крейты + тесты)
```sh
# в Docker (без локального тулчейна): default max_app = ["sdl-from-source"],
# поэтому ставим dev-заголовки SDL3 (X11/Wayland/ALSA/GL) и собираем SDL3 из
# исходников через CMake; libSDL3.so.0 кладётся в target/debug, тестам нужен
# LD_LIBRARY_PATH.
docker run --rm -v "D:/dev/max-port-debug:/src" -w /src/max-brh rust:1.94.0-bookworm \
  bash -c "apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config \
    libasound2-dev libx11-dev libxext-dev libxrender-dev libxcursor-dev libxi-dev \
    libxrandr-dev libxinerama-dev libxss-dev libxtst-dev libxfixes-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols \
    libdecor-0-dev libegl-dev libgles-dev libgl-dev libgbm-dev libdrm-dev \
    libdbus-1-dev libudev-dev >/dev/null && \
    export LD_LIBRARY_PATH=/src/max-brh/target/debug && \
    cargo check && cargo test"

# на хосте (Windows, MSVC + CMake в PATH — проще всего из Developer PowerShell
# VS; см. §«Запуск без флагов» ниже):
cd max-brh
cargo build --release -p max_app              # GUI: SDL3 из исходников (default)
cargo build --release -p max_app --features sound            # + звук
cargo build --release -p max_app --features sdl-vcpkg,sound --no-default-features
                                             # Windows: SDL3 из vcpkg вместо сборки
cargo build --release -p max_app --no-default-features       # headless
```

### Windows UI-сборка (готовый путь)
```powershell
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-ui.ps1
# артефакт: bin\release\max_app-1.1.0-windows-x86_64-ui\max-brh.exe (+SDL3.dll)
```

### Windows headless-сборка (MSVC, нативно)
```powershell
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-headless.ps1
# артефакт: bin\release\max_app-1.1.0-windows-x86_64\max-brh.exe
```
Нативная MSVC-сборка (`x86_64-pc-windows-msvc`), без Docker и mingw. Скрипт
поднимает `vcvars64`, собирает
`cargo build --release --locked -p max_app --no-default-features`, проверяет
PE (Subsystem=3, Console) и прогоняет `--help`/`--demo SAVE1.CAM --turns 1`
нативно, затем упаковывает каталог + `.zip` + `.zip.sha256`.

### Linux-сборка + архив
```sh
bash release.sh          # bin/release/max_app-1.1.0-linux-x86_64.tar.gz
```

### Horizon (сервер)
```sh
docker run --rm -v "D:/dev/max-port-debug/max-brh-server:/src" -w /src rust:1.94.0-bookworm \
  bash -c "apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libssl-dev >/dev/null && cargo build --release -p horizon"
# или: docker compose up -d --build
```

### C++-headless (наша развилка, Linux)
```sh
docker run --rm -v "D:/dev/max-port-debug:/src" -w /src max-port-build:latest \
  bash /src/build/docker-build-linux-server.sh
# бинарь: build/linux-server-template/max-port
```

## Запуск (как играть)

### Одиночная игра / hot-seat (окно)
```sh
# из исходников: default features — GUI, SDL3 собирается из исходников (cmake)
cd max-brh
cargo run -p max_app
# со звуком:
cargo run -p max_app --features sound
```
`cargo run -p max_app` **без флагов** открывает окно игры: меню → New Game /
Multiplayer Game → Hot Seat. `--ui` — совместимый alias. Чтобы окно собралось,
нужен CMake и C-компилятор (на Windows — MSVC/Windows SDK из состава VS
«Desktop development with C++»; проще запускать из Developer PowerShell, где
`cl.exe`/`cmake` уже в PATH, либо добавить cmake в PATH):

```powershell
# 1. Developer PowerShell for VS — cmake/cl.exe уже в PATH. Либо в обычном
#    PowerShell добавьте cmake из состава Visual Studio:
$vs = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" `
    -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath
$env:Path = "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;$env:Path"

# 2. Запуск (сборка SDL3 из исходников идёт один раз, дальше — из кэша)
cd D:\dev\max-port-debug\max-brh
cargo run -p max_app
```

```powershell
# готовый Windows-артефакт (SDL3 из vcpkg, звук), см. build-windows-ui.ps1:
cd D:\dev\max-port-debug
bin\release\max_app-1.1.0-windows-x86_64-ui\max-brh.exe
```
По умолчанию (без флагов) клиент открывает GUI; `--ui` — совместимый alias.
В меню: **New Game** (одиночная: Options → Planet → Players, «Human/Computer» →
мобилизация (покупка стартовых юнитов) → выбор точки высадки на карте) или
**Multiplayer Game → Hot Seat Game** (2–4 человека на одном ПК; те же две фазы
проходит каждый человек по очереди).
Управление: LMB — выбрать юнит; RMB — приказ движения (зелёный предпросмотр);
клик по врагу — атака; **T**+Enter — чат; **End Turn** — конец хода; клик по
миникарте — камера; **P** — пауза; **S** — Save Game, **L** — Load Game;
**B** — BuildMenu; **U** — экран покупных апгрейдов; **O** — питание юнита
(у Research Center включает исследование темы Attack).

### Сохранения (V70, P3)
Локальная партия пишется **настоящим V70-сейвом** (`GameState::to_save_bytes`,
`max_core::save_write`) — тем же форматом, что `max-original/SAVE1.CAM`:
- ручное сохранение/загрузка — окно `Save Game`/`Load Game` (клавиши **S**/**L**),
  10 слотов на страницу, файлы `save<slot>.<ext>`; расширение — по категории
  партии (custom `.dta`, hot-seat `.hot`, кампания `.cam`, сеть `.mlt`), как
  `SaveLoad_GetSaveFileName` в C++;
- автосейв — конец каждого раунда, слот **10** (`save10.dta`), имя «(Turn N)»
  (`SETUP.auto_save`); отключается `AppConfig::with_autosave(false)`;
- каталог — `AppConfig::save_dir` (`--save-dir`/`with_save_dir`), по умолчанию
  каталог `MAX.RES` (`max-original/`); QA пишет сейвы в `qa-out/...`;
- в сетевой сессии окно не открывается: оригинал при сохранении рассылает
  пакет 16, которого в нашем протоколе нет (загрузка в сети запрещена и в
  оригинале).

### Сетевая игра
```sh
# 1. сервер:
docker compose up -d --build
# 2. клиенты (в меню: Multiplayer Game → Host/Join Network Game):
bin\release\max_app-1.1.0-windows-x86_64-ui\max-brh.exe
```
Автоматический smoke: `bash max-brh-cicd/scripts/live-smoke.sh 10` (2 клиента × 10 ходов).

### Headless-режимы (без окна)
Headless-сборка собирается **без** default-фич — SDL3/ALSA тогда не нужны ни
при сборке, ни в рантайме:
```sh
cd max-brh
cargo run -p max_app --no-default-features -- --demo ../max-original/SAVE1.CAM --turns 5
cargo run -p max_app --no-default-features -- --new-game --world 12 --seed 42 --out frame.ppm
cargo run -p max_app --no-default-features -- --connect 127.0.0.1:8082 --join <GAME_ID> --turns 10
```
Готовая headless-сборка (Windows, MSVC): `bin\release\max_app-1.1.0-windows-x86_64\max-brh.exe`
(`--demo`, `--new-game`, `--connect`; окна в нём нет).

В debug-сборке на Windows главный поток получает 8 МиБ стека через
`max-brh/.cargo/config.toml` (`/STACK:8388608`): дефолтный 1 МиБ не вмещает
загрузку V70-сейва (`--demo`/`--new-game` падали бы с `has overflowed its
stack`). Release-сборки и Linux работали и без этого.

### Данные игры
Нужны оригинальные данные: `max-original/` (MAX.RES, COLOR.PAL, *.WRL, *.SPW, сейвы)
и `assets/PATCHES.RES`. Пути задаются `--data-dir/--assets-dir`, по умолчанию ищутся
от корня репозитория.

## Фичи `max_app`

| Фича | Что включает | В default |
|---|---|---|
| `sdl-from-source` | окно SDL3; SDL3 3.4.20 собирается из vendored `sdl3-src` через CMake (нужны C-компилятор + cmake) | **да** |
| `sdl` | окно с системным SDL3 (`libsdl3-dev` / vcpkg) — база для остальных sdl-фич | нет (включается ими) |
| `sdl-vcpkg` | окно с SDL3 из vcpkg | нет |
| `sound` | звук (rodio/cpal → ALSA на Linux, WASAPI на Windows) | нет |
| `tls` | TLS для Horizon (https://) | нет |

`default = ["sdl-from-source"]`, поэтому `cargo run -p max_app` открывает игру.
Явные фичи сочетаются с `--no-default-features`
(`cargo run -p max_app --no-default-features --features sdl-vcpkg`), иначе к ним
добавится ещё и `sdl-from-source`. Headless-сборки — `--no-default-features`.

## Библиотеки (стек)


| Слой | Стек |
|---|---|
| Язык | Rust 1.94 (edition 2021) |
| Симуляция (max_core) | без внешних зависимостей (собственный RNG Xoshiro128**, детерминизм) |
| Сеть (max_net) | tonic+prost (gRPC), vendored ENet 1.3.18 (UDP), sha2 |
| Ассеты (max_assets) | собственные парсеры (RES/WRL/спрайты/WAV) |
| Рендер (max_render) | чистый софтверный рендер + SDL3 (фичи `sdl`/`sdl-vcpkg`/`sdl-from-source`; в `max_app` **default — `sdl-from-source`**, т.е. SDL3 3.4.20 собирается из исходников через CMake), rodio/cpal (звук за фичей `sound`, не в default) |
| Сервер Horizon | Rust: tokio, tonic, существующий стек Horizon |
| C++-headless | C++20 (MSVC/GCC), SDL3, ENet — из нашего `src/` |

## QA

`bash ci.sh` — тесты + QA-сценарии + golden. Детали: `max-brh-qa/README.md`.

### C++-векторы (эталоны формул)

`max-brh/tools/*_vectors` — самостоятельные C++-программы, повторяющие
формулы оракула; компилируются в образе `max-port-build:latest` (`g++`) и
пишут закоммиченные `vectors.txt`, которые Rust-тесты сверяют бит-в-бит:

```sh
# стоимости апгрейдов/исследований (P6)
docker run --rm -v "D:/dev/max-port-debug:/src" \
    -w /src/max-brh/tools/upgrade_vectors max-port-build:latest \
    bash -c 'g++ -O2 -std=c++17 main.cpp -o /tmp/v && /tmp/v > /src/max-brh/tools/upgrade_vectors/vectors.txt'

# экран мобилизации (P7): сначала данные из PATCHES.RES, затем векторы
python max-brh/tools/gen_cargo_data.py
docker run --rm -v "D:/dev/max-port-debug:/src" \
    -w /src/max-brh/tools/cargo_vectors max-port-build:latest \
    bash -c 'g++ -O2 -std=c++17 main.cpp -o /tmp/v && /tmp/v > /src/max-brh/tools/cargo_vectors/vectors.txt'
```
