# M.A.X. — Rust-клиент под Horizon

Современный клиент игры **M.A.X.: Mechanized Assault & Exploration** (Interplay, 1996),
написанный заново на **Rust**, с серверной инфраструктурой **Horizon** и
визуальным паритетом **1:1 с оригиналом**.

M.A.X. COPYRIGHT (C) 1996 INTERPLAY PRODUCTIONS. ALL RIGHTS RESERVED.
Для игры требуются оригинальные данные (`MAX.RES`, `PATCHES.RES`, `COLOR.PAL`, карты `.WRL`, звуки `.SPW`) — они не входят в репозиторий.

---

## Чем это отличается от M.A.X. Port

[M.A.X. Port](https://github.com/klei1984/max) (klei1984) — нативный порт оригинального
движка на C++ (та же логика, тот же сетевой P2P/lockstep-стек). Этот проект —
**переосмысление клиента**, а не порт 1:1:

| Аспект | M.A.X. Port | Наш проект |
|---|---|---|
| Язык клиента | C++ (порт оригинального кода) | **Rust**, чистая модульная архитектура |
| Архитектура | монолит порта | `max_core` (симуляция) / `max_net` / `max_assets` / `max_render` / `max_app` — строгие границы |
| Сеть | P2P-меш, ENet | **серверная звезда через Horizon**: gRPC-лобби (protobuf) + headless-арбитр, релей пакетов, подписанные тикеты |
| Серверная инфраструктура | — | **Horizon** (Rust): лобби/админ, сессии, масштабирование (docker compose) |
| Согласованность | lockstep как есть | lockstep с **бит-в-бит верификацией** (RNG-векторы, CRC-барьеры, C++-эталоны) |
| Качество | ручное | **QA-фабрика**: Test-Suite (149 TC), скриптовый драйвер, golden-кадры, CI, запись сценариев вручную |
| RE-база | частная IDA-база автора | собственная RE-линия (`max-original-dasm/`) + оракулы C++ |

M.A.X. Port используется как **оракул** (канонический референс логики и визуала) и
не изменяется; C++-headless из нашей развилки остаётся игровым арбитром для сетевых сессий.

## Принципиально новое

1. **Rust-переписывание с детерминизмом**: вся симуляция (карта, юниты, приказы,
   бой, пути, ИИ с 44 классами задач, победа/поражение) портирована бит-в-бит с
   C++ (RNG Xoshiro128\*\* 1:1, CRC16 барьеров, мини-реплей совпал побайтово).
2. **Три режима игры**: одиночная против ИИ, hot-seat 2–4 игрока на одном ПК,
   сетевая через Horizon.
3. **Бинарные протоколы**: protobuf/gRPC для Horizon, плоские LE-пакеты для сессии.
4. **Визуальный паритет 1:1**: тайлы карт, спрайты, окно FRAMEPIC, панели,
   оригинальные шрифты GNW, курсоры, меню — из оригинальных ресурсов.
5. **Автоматическое QA**: 650+ тестов, скриптовые UI-сценарии, golden-кадры,
   живой сетевой smoke — в каждом CI-прогоне.

## Структура репозитория

| Каталог | Назначение |
|---|---|
| `max-rust/` | Rust-клиент (workspace: core/net/assets/render/app) |
| `max-horizon-server/` | Horizon: оркестратор сессий (Rust, gRPC+WS) |
| `src/` | наша развилка C++-порта: headless-сервер-арбитр |
| `max-cpp-port/` | оракул: канонический M.A.X. Port (git-субмодуль, неизменяемый) |
| `max-port-site/` | документация автора (формат сейвов V70 и пр.) |
| `max-original/`, `max-original-dasm/` | оригинальная игра и RE-артефакты |
| `max-rust-qa/` | QA: Test-Suite, сценарии, golden, драйверы |
| `docs/` | документация по направлениям: `architecture/`, `analysis/`, `qa/`, `development/` |
| `build/`, `deploy/`, `release/` | сборки, развёртывание, артефакты |

## Собрать

Кратко — в [docs/development/build.md](docs/development/build.md). Основное:

```sh
bash ci.sh                                   # тесты + QA + golden (Docker)
powershell -File build\scripts\build-windows-ui.ps1   # Windows-бинарь с окном (MSVC+vcpkg)
bash release.sh                              # Linux-бинарь + архив
docker compose up -d --build                 # сервер Horizon
```

Необходимые инструменты и библиотеки — там же (Rust 1.94, MSVC+vcpkg/SDL3, Docker,
Python 3; стек: tonic/prost, vendored ENet 1.3.18, SDL3, rodio).

## Запустить (как играть)

```powershell
# одиночная / hot-seat (окно; без флагов — GUI, --ui — совместимый alias):
release\max_app-1.0.0-windows-x86_64-ui\max-rust.exe
#   New Game — одиночная против ИИ; Multiplayer Game → Hot Seat Game — 2–4 игрока.
#   После экрана Players: мобилизация (покупка юнитов) → выбор точки высадки.
#   LMB — выбрать юнит; RMB — движение; клик по врагу — атака; T — чат; End Turn.

# сетевая: сначала docker compose up -d --build, затем в меню Host/Join Network Game.
```

Подробно: [docs/development/build.md](docs/development/build.md) (раздел «Запуск»).

## Документация

- **Architecture**: [docs/architecture/](docs/architecture/) — протоколы, модель состояния, режимы игры, барьеры.
- **Analysis**: [docs/analysis/](docs/analysis/) — визуальный паритет, реверс-инжиниринг.
- **QA**: [docs/qa/](docs/qa/) и [max-rust-qa/](max-rust-qa/) — Test-Suite, драйверы, golden.
- **Development**: [docs/development/](docs/development/) — планы, сборка, чек-лист релиза.
- История изменений: [CHANGELOG.md](CHANGELOG.md). Лицензия: [LICENSE](LICENSE).


### Быстрая сборка на Windows (автокопирование SDL3.dll)

`powershell
powershell -ExecutionPolicy Bypass -File build.ps1          # release + DLL в target\\release\\ и dist\\
powershell -ExecutionPolicy Bypass -File build.ps1 -Debug   # debug-сборка
``r

Скрипт сам находит SDL3 (vcpkg → build\\sdl3-3.4.20 → бандл клиента) и копирует SDL3.dll рядом с exe — отдельный запуск с ошибкой «SDL3.dll not found» не повторится.
