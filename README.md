# M.A.X. : Beyond Rust Horizon

**M.A.X. : Beyond Rust Horizon** — ремейк игры **M.A.X.: Mechanized Assault & Exploration**
(Interplay, 1996), переписанный на **Rust**, с серверной инфраструктурой **Horizon** и
визуальным паритетом **1:1 с оригиналом**.

В основе ремейка — два **референса**, которые в процессе разработки **не меняются** и
используются для сверки игровых механик:

- оригинальная игра **M.A.X.** (Interplay, 1996);
- порт **M.A.X. Port** (klei1984) — канонический C++-код логики и визуала.

**Проект называется «M.A.X. : Beyond Rust Horizon»** (сокращённо **BRH** — от
*Beyond Rust Horizon*). Отсюда префикс каталогов `max-brh*` и имя бинарника
`max-brh(.exe)`. Полный глоссарий терминов и сокращений —
[max-brh-docs/glossary.md](max-brh-docs/glossary.md).

M.A.X. COPYRIGHT (C) 1996 INTERPLAY PRODUCTIONS. ALL RIGHTS RESERVED.
Для игры требуются оригинальные данные (`MAX.RES`, `PATCHES.RES`, `COLOR.PAL`, карты `.WRL`, звуки `.SPW`) — они не входят в репозиторий.

---

## Ремейк на базе оригинала и M.A.X. Port

Оригинальная **M.A.X.** и порт **M.A.X. Port** ([klei1984/max](https://github.com/klei1984/max)) —
два референса, которые не изменяются в ходе разработки и служат для сверки игровых
механик. Ремейк **не является переносом** 1:1, а представляет собой переосмысление
клиента:

| Аспект | M.A.X. Port (референс) | Ремейк (BRH) |
|---|---|---|
| Язык клиента | C++ (порт оригинального кода) | **Rust**, чистая модульная архитектура |
| Архитектура | монолит порта | `max_core` (симуляция) / `max_net` / `max_assets` / `max_render` / `max_app` — строгие границы |
| Сеть | P2P-меш, ENet | **серверная звезда через Horizon**: gRPC-лобби (protobuf) + headless-арбитр, релей пакетов, подписанные тикеты |
| Серверная инфраструктура | — | **Horizon** (Rust): лобби и администрирование, сессии, масштабирование (docker compose) |
| Согласованность | lockstep в исходном виде | lockstep с **бит-в-бит верификацией** (RNG-векторы, CRC-барьеры, C++-эталоны) |
| Качество | ручное | **QA-инфраструктура**: Test-Suite (179 TC), скриптовый драйвер, golden-кадры, CI, запись сценариев вручную |
| RE-база | частная IDA-база автора | собственная RE-база (`max-original-dasm/`) + оракулы C++ |

Оригинал и M.A.X. Port используются как **референсы** (каноническая логика и визуал) и
**не изменяются**; headless-арбитр для сетевых сессий собирается из `max-cpp-port` (режим `--server`).

## Принципиально новое

1. **Rust-переписывание с детерминизмом**: вся симуляция (карта, юниты, приказы,
   бой, пути, ИИ с 44 классами задач, победа/поражение) портирована бит-в-бит с
   C++ (RNG Xoshiro128\*\* 1:1, CRC16 барьеров, мини-реплей совпал побайтово).
2. **Три режима игры**: одиночная против ИИ, hot-seat 2–4 игрока на одном ПК,
   сетевая через Horizon.
3. **Бинарные протоколы**: protobuf/gRPC для Horizon, плоские LE-пакеты для сессии.
4. **Визуальный паритет 1:1**: тайлы карт, спрайты, окно FRAMEPIC, панели,
   оригинальные шрифты GNW, курсоры, меню — из оригинальных ресурсов.
5. **Автоматическое QA**: 780+ тестов, скриптовые UI-сценарии (включая
   `upgrade.qa`/`research.qa`), golden-кадры, сетевой smoke-тест в CI.

## Структура репозитория

| Каталог | Назначение |
|---|---|
| `max-brh/` | Rust-клиент (workspace: core/net/assets/render/app) |
| `max-brh-server/` | выделенный игровой сервер (gRPC+WS) |
| `max-brh-docs/` | проектная документация: `architecture/`, `analysis/`, `qa/`, `development/` |
| `max-brh-qa/` | QA: Test-Suite, сценарии, golden, драйверы |
| `max-brh-qa-tools/` | QA-утилиты: `max-resources-viewer`, `max-resources-exporter` |
| `max-brh-assets/` | референс/ремастер: `images/`, `units/`, `text/` |
| `max-brh-cicd/` | CI/CD: `ci.sh` + `scripts/` |
| `max-cpp-port/` | оракул: канонический M.A.X. Port (git-субмодуль, неизменяемый) |
| `max-cpp-port-docs/` | документация автора (сайт, формат сохранений V70) |
| `max-original/`, `max-original-dasm/` | оригинальная игра и RE-артефакты |
| `max-unity-client/` | Unity-прототип |
| `build/`, `bin/` | временные сборки, артефакты (`bin/` можно удалить) |

## Собрать

Краткое руководство — в [max-brh-docs/development/build.md](max-brh-docs/development/build.md). Основные команды:

```sh
bash ci.sh                                   # тесты + QA + golden (Docker)
powershell -File max-brh-cicd\scripts\build-windows-console.ps1   # Windows консольная сборка клиента (нативный MSVC, без Docker/mingw)
powershell -File max-brh-cicd\scripts\build-windows-ui.ps1   # Windows игровая сборка (MSVC+vcpkg)
bash release.sh                              # Linux-сборка + архив
docker compose up -d --build                 # сервер Horizon
```

Docker для Windows-сборок не требуется: обе сборки выполняются нативной MSVC-сборкой
на хосте (консольная сборка клиента —
`bin\release\max_app-1.1.0-windows-x86_64.zip`, игровая сборка —
`…-windows-x86_64-ui.zip`).

Необходимые инструменты и библиотеки — там же (Rust 1.94, MSVC+vcpkg/SDL3, Docker,
Python 3; стек: tonic/prost, vendored ENet 1.3.18, SDL3, rodio).

## Запустить (как играть)

```powershell
# одиночная / hot-seat (окно; без флагов — GUI, --ui — совместимый alias):
bin\release\max_app-1.1.0-windows-x86_64-ui\max-brh.exe
#   New Game — одиночная против ИИ; Multiplayer Game → Hot Seat Game — 2–4 игрока.
#   После экрана Players: мобилизация (покупка юнитов, cargo, апгрейды —
#   переключатель Buy Units/Upgrades) → выбор точки высадки.
#   LMB — выбрать юнит; RMB — движение; клик по врагу — атака; T — чат; End Turn.
#   U — покупные апгрейды (форма, золото); O — питание юнита (Research Center
#   включает исследование темы); R — ResearchMenu (распределение центров по
#   темам); ? — помощь (GAME_SCREEN_SETUP); B — BuildMenu.
#   Кнопки ?/Help в меню и мобилизации открывают окна помощи (OPTIONS_SETUP,
#   PLANET_SETUP, NEW_GAME_SETUP, CARGO_SETUP).

# сетевая: сначала docker compose up -d --build, затем в меню Host/Join Network Game.
```

Игровая сборка (`bin\release\max_app-*-windows-x86_64-ui\max-brh.exe`) — подсистема
Windows (GUI): двойной щелчок открывает окно без консоли; параметры `--help`/`--demo`
отображаются только при перенаправлении вывода (`max-brh.exe --help > help.txt`).
Вывод для терминала и CI обеспечивает консольная сборка клиента
`bin\release\max_app-*-windows-x86_64.zip` (подсистема Console).

Подробное описание: [max-brh-docs/development/build.md](max-brh-docs/development/build.md) (раздел «Запуск»).

## Документация

- **Architecture**: [max-brh-docs/architecture/](max-brh-docs/architecture/) — протоколы, модель состояния, режимы игры, барьеры.
- **Analysis**: [max-brh-docs/analysis/](max-brh-docs/analysis/) — визуальный паритет, реверс-инжиниринг.
- **QA**: [max-brh-docs/qa/](max-brh-docs/qa/) и [max-brh-qa/](max-brh-qa/) — Test-Suite, драйверы, golden.
- **Development**: [max-brh-docs/development/](max-brh-docs/development/) — планы, сборка, чек-лист релиза.
- История изменений: [CHANGELOG.md](CHANGELOG.md). Лицензия: [LICENSE](LICENSE).
