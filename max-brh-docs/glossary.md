# Глоссарий проекта

Канонические названия, термины и сокращения проекта. Если термин встречается в
другом написании — править сюда и ссылаться отсюда. Соответствие доменного языка
и C++-оракула (имена объектов кода) — в [domain-map.md](domain-map.md); системная
таксономия игровых объектов — в [object-classifier.md](object-classifier.md);
свойства производственных юнитов — в [unit-properties.md](unit-properties.md);
сверка имён юнитов — в [unit-name-review.md](unit-name-review.md).

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
| **UI-лобби** | отдельная поверхность сервера (игровой лобби + админ-консоль), **не игра** — см. [architecture/server/lobby-ui.md](architecture/server/lobby-ui.md) |
| **арбитр** | C++-headless `max-port --server`: игровой авторитет, релей пакетов и CRC-барьеры — см. [architecture/server/dedicated-server-plan.md](architecture/server/dedicated-server-plan.md) |
| **тикет** | выданный `JoinGame` ключ/адрес для входа клиента в ENet-сессию |

## 5. Прочие термины

- **оракул** — неизменяемый канонический код/данные для сверки (`max-cpp-port/`, `max-original/`, `max-original-dasm/`).
- **golden** — эталонный кадр/вектор для регрессионного сравнения (QA).
- **lockstep** — детерминированная синхронная симуляция, по сети идут только приказы/события.
- **RE** — reverse engineering (обратная разработка) оригинальной игры.

## 6. Игровые ресурсы (DSL)

Каноничное имя ресурса — английский термин оригинала (и `Rust`-идентификатор);
русский — глосса. C++-токены с «ложными друзьями» помечены отдельно.

| Канон (DSL) | Русская глосса | Rust | C++-токен | Примечание |
|---|---|---|---|---|
| **raw** | сырьё / материалы | `Cargo.raw`, `Complex.material` | `CARGO_TYPE_RAW`, `raw`≡`material` | один ресурс; мануал равно пишет «Raw Materials»/«Materials» |
| **fuel** | топливо | `Cargo.fuel`, `Complex.fuel` | `CARGO_TYPE_FUEL` | не путать с `ATTRIB_FUEL` (собственный запас топлива машины) |
| **gold** | золото (руда) | `Cargo.gold`, `Complex.gold` | `CARGO_TYPE_GOLD` | не путать с `credits` |
| **power** | энергия / мощность | `Complex.power` | `Complex.power` | поток комплекса, не хранится в складах |
| **workers** | рабочие | `Complex.workers` | `Cargo.life` (ложный друг) | канон по оригиналу «workers»; C++ `life` ≠ прочность `hits` |
| **credits** | кредиты | `TeamUnits::gold` | `TeamUnits::gold` | очищенное золото на счёте команды; не путать с `gold` |
