# Глоссарий проекта

Канонические названия, термины и сокращения проекта. Если термин встречается в
другом написании — править сюда и ссылаться отсюда.

## 1. Название проекта

**M.A.X. : Beyond Rust Horizon** — полное название ремейка игры
M.A.X. (Mechanized Assault & Exploration, Interplay 1996).

Сокращение: **BRH** = *Beyond Rust Horizon*.

## 2. Соглашение об именах (префикс `max-brh`)

Префикс `max-brh` (в URL/путях/именах пакетов — `max-brh`, в бинарнике —
`max-brh`) происходит от BRH и означает «часть проекта Beyond Rust Horizon».

| Имя | Назначение |
|---|---|
| `max-brh/` | игровой клиент (Rust-workspace: `max_core`/`max_net`/`max_assets`/`max_render`/`max_app`) |
| `max-brh-server/` | выделенный игровой сервер (Horizon) — отдельный проект |
| `max-brh-docs/` | проектная документация |
| `max-brh-qa/` | тестовый сьют (сценарии, golden, suite, планы) |
| `max-brh-qa-tools/` | QA-утилиты (`max-resources-viewer`, `max-resources-exporter`) |
| `max-brh-assets/` | референс/ремастер: выгруженные ресурсы, описания |
| `max-brh-cicd/` | CI/CD: `ci.sh` + `scripts/` |
| `max-brh.exe` | бинарник клиента (игровая и консольная сборки) |

Не относятся к `max-brh*` (сторонние/исходные артефакты, не наш код):
`max-cpp-port/` (оракул), `max-cpp-port-docs/`, `max-original/`,
`max-original-dasm/`, `max-unity-client/`.

## 3. Термины сборки и артефактов

| Термин | Значение |
|---|---|
| **игровая сборка** | артефакт клиента с окном: `max_app-<ver>-windows-x86_64-ui` (SDL3+звук, PE Subsystem=2). Единственный «продуктовый» артефакт клиента. |
| **консольная сборка клиента** | клиент без окна (`--no-default-features`), `max_app-<ver>-windows-x86_64` (PE Subsystem=3): `--demo`/`--new-game`/`--connect`. Тестовый/smoke-артефакт, не релиз. |
| **нативная MSVC-сборка** | способ сборки Windows: MSVC на хосте (`vswhere`→`vcvars64`), `x86_64-pc-windows-msvc`; без Docker/mingw/wine. |
| **бинарь** | только файл `max-brh(.exe)`; каталог/архив — «сборка»/«артефакт». |
| **headless** | **только контекст сервера** (нет UI по определению): `max-brh-server`, C++-арбитр `max-port --server`. К игровому клиенту не применяется. |

## 4. Термины серверной части

| Термин | Значение |
|---|---|
| **Horizon** | серверный протокол/инфраструктура (gRPC + WS, protobuf) выделенного сервера |
| **UI-лобби** | отдельная поверхность сервера (игровой лобби + админ-консоль), **не игра** — см. [architecture/lobby-ui.md](architecture/lobby-ui.md) |
| **арбитр** | C++-headless `max-port --server`: игровой авторитет, релей пакетов и CRC-барьеры — см. [architecture/dedicated-server-plan.md](architecture/dedicated-server-plan.md) |
| **тикет** | выданный `JoinGame` ключ/адрес для входа клиента в ENet-сессию |

## 5. Прочие термины

- **оракул** — неизменяемый канонический код/данные для сверки (`max-cpp-port/`, `max-original/`, `max-original-dasm/`).
- **golden** — эталонный кадр/вектор для регрессионного сравнения (QA).
- **lockstep** — детерминированная синхронная симуляция, по сети идут только приказы/события.
- **RE** — reverse engineering (обратная разработка) оригинальной игры.
