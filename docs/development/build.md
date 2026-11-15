# Сборка, запуск, инструменты и библиотеки

## Необходимые инструменты

### Разработка (хост, Windows)
| Инструмент | Версия | Зачем |
|---|---|---|
| Rust (rustup) | 1.94.0 | сборка max-rust |
| MSVC (VS Build Tools 2022+/VS 2026) + Windows SDK + CMake | — | Windows-сборка SDL3 |
| vcpkg + `sdl3:x64-windows` | SDL3 3.4.20 | окно на Windows |
| Docker (Desktop) | — | Linux-сборки, сервер, CI |
| Python 3 | — | эталонные парсеры/утилиты |
| Git | — | репозиторий |

### Docker-образы
| Образ | Назначение |
|---|---|
| `rust:1.94.0-bookworm` | сборка/тесты max-rust и max-horizon-server |
| `ubuntu:24.04` | рантайм сервера, radare2, вспомогательные |
| `max-port-build:latest` | сборка C++-headless (Linux) |
| `max-ghidra`, `max-dosbox` | RE (по необходимости) |

## Сборка

### Rust-клиент (все крейты + тесты)
```sh
# в Docker (без локального тулчейна):
docker run --rm -v "D:/dev/max-port-debug:/src" -w /src/max-rust rust:1.94.0-bookworm \
  bash -c "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test"

# на хосте (Windows, MSVC):
cd max-rust
cargo build --release -p max_app --features sdl-vcpkg,sound
```

### Windows UI-бинарь (готовый путь)
```powershell
powershell -ExecutionPolicy Bypass -File build\scripts\build-windows-ui.ps1
# артефакт: release\max_app-1.0.0-windows-x86_64-ui\max-rust.exe (+SDL3.dll)
```

### Windows headless-бинарь (кросс-сборка, без MSVC)
```sh
bash build/scripts/build-windows.sh
# артефакт: release\max_app-1.0.0-windows-x86_64\max-rust.exe
```

### Linux-бинарь + архив
```sh
bash release.sh          # release/max_app-1.0.0-linux-x86_64.tar.gz
```

### Horizon (сервер)
```sh
docker run --rm -v "D:/dev/max-port-debug/max-horizon-server:/src" -w /src rust:1.94.0-bookworm \
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
```powershell
cd D:\dev\max-port-debug
release\max_app-1.0.0-windows-x86_64-ui\max-rust.exe
```
По умолчанию (без флагов) клиент открывает GUI; `--ui` — совместимый alias.
В меню: **New Game** (одиночная: Options → Planet → Players, «Human/Computer» →
мобилизация (покупка стартовых юнитов) → выбор точки высадки на карте) или
**Multiplayer Game → Hot Seat Game** (2–4 человека на одном ПК; те же две фазы
проходит каждый человек по очереди).
Управление: LMB — выбрать юнит; RMB — приказ движения (зелёный предпросмотр);
клик по врагу — атака; **T**+Enter — чат; **End Turn** — конец хода; клик по
миникарте — камера; **P** — пауза.

### Сетевая игра
```sh
# 1. сервер:
docker compose up -d --build
# 2. клиенты (в меню: Multiplayer Game → Host/Join Network Game):
release\max_app-1.0.0-windows-x86_64-ui\max-rust.exe
```
Автоматический smoke: `bash build/scripts/live-smoke.sh 10` (2 клиента × 10 ходов).

### Headless-режимы (без окна)
```cmd
max-rust.exe --demo max-original\SAVE1.CAM --turns 5
max-rust.exe --new-game --world 12 --seed 42 --out frame.ppm
max-rust.exe --connect 127.0.0.1:8082 --join <GAME_ID> --turns 10
```

### Данные игры
Нужны оригинальные данные: `max-original/` (MAX.RES, COLOR.PAL, *.WRL, *.SPW, сейвы)
и `assets/PATCHES.RES`. Пути задаются `--data-dir/--assets-dir`, по умолчанию ищутся
от корня репозитория.

## Библиотеки (стек)

| Слой | Стек |
|---|---|
| Язык | Rust 1.94 (edition 2021) |
| Симуляция (max_core) | без внешних зависимостей (собственный RNG Xoshiro128**, детерминизм) |
| Сеть (max_net) | tonic+prost (gRPC), vendored ENet 1.3.18 (UDP), sha2 |
| Ассеты (max_assets) | собственные парсеры (RES/WRL/спрайты/WAV) |
| Рендер (max_render) | чистый софтверный рендер + SDL3 (за фичей sdl/sdl-vcpkg/sdl-from-source), rodio/cpal (звук за фичей sound) |
| Сервер Horizon | Rust: tokio, tonic, существующий стек Horizon |
| C++-headless | C++20 (MSVC/GCC), SDL3, ENet — из нашего `src/` |

## QA

`bash ci.sh` — тесты + QA-сценарии + golden. Детали: `max-rust-qa/README.md`.
