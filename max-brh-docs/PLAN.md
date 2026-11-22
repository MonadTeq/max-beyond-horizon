# Единый план проекта — M.A.X. : Beyond Rust Horizon

Единственный источник истины по **целям**, **статусу** и **открытым работам**.
Детализация — по ссылкам на профильные документы; этот файл — общая картина и
разделение ответственности по агентам (см. [`AGENTS.md`](AGENTS.md)).

> Новые задачи фиксируются только в этом документе; детальные рабочие планы —
> по ссылкам в §6.

## 1. Состояние проекта

**Пре-альфа.** Режимы и механики реализованы в коде и покрыты тестами
(1217+ тестов, golden-гейт, CRC-барьеры бит-в-бит с C++-оракулом), но по
ручному тестированию игра ещё сырая и далека от альфы. Закрытие целей — только
после явного подтверждения владельца (правило 4, `qa/README.md`).

## 2. Цели (консолидированные)

1. **Играбельный Rust-клиент** с полной детерминированной симуляцией 1:1
   (режимы одиночная игра, hot-seat, сетевая).
2. **Сетевая игра**: выделенный сервер (Horizon + headless-арбитр) — **готово**;
   режим хоста (P2P) из оригинала — **не реализован**.
3. **Абсолютное сохранение визуала и механик оригинала** + исправление
   известных ошибок оригинала (осознанные расхождения — отдельно).

## 3. Готово (вехи)

- **Симуляция `max_core`**: P0–P7 — карта/ресурсы, юниты, команды, приказы,
  ходы, RNG 1:1, ИИ, victory/loss, V70 round-trip побайтно, CRC16; C++-мини-реплей
  совпал бит-в-бит.
- **Режимы**: M-SP (одиночная), M-HS (hot-seat), M-NET (сеть через Horizon) —
  реализованы; доводки ввода/старта/реконнекта завершены.
- **Выделенный сервер**: Horizon (`max-brh-server`) + C++-headless-арбитр,
  лобби/сессии/тикеты/админ, десинк-аудит, рестарт; направление A закрыто
  (0 десинков, 10 ходов).
- **Геймплей 1:1**: производство/экономика/комплексы/коннекторы/бой/исследования/
  мобилизация/сейвы — портированы по C++-оракулу.
- **Визуал 1:1**: меню/настройки/карта/панели/курсоры/CargoMenu/ResearchMenu/
  Preferences/Help — A1–A9, B1–B5, D1/D3, E1 сделаны.
- **Рефакторинг под новые стандарты (Ф0–Ф5.3) — завершён**: enum-классификация
  (`CombatRole`/`Mobility`/`Environment`/`Footprint`/`ObjectKind`/`CargoKind`),
  нижний crate `max_types`, слои выстроены, презентация (`UnitPresentation`)
  отделена от симуляции, ~230 проверок флагов мигрированы на enum-методы;
  CRC/V70 не изменены.
- **CI/QA/релиз**: `ci.sh` (check/test/QA/golden/soak), GitHub Actions, релизные
  артефакты Windows (UI + headless, нативный MSVC) и Linux, чек-лист релиза.

## 4. Открытые работы — по агентам

### Координатор (основной агент)

- Вести этот план; распределять задачи по компетенциям.
- Принимать эскалации и **сообщать владельцу**, не решая вне компетенций.
- Следить за гейтами: CRC/детерминизм/V70 — бит-в-бит на каждом изменении.

### Аналитик (домен/DSL/классификация; владелец `domain-map.md`, `object-classifier.md`)

- **Открытые имена**: `SHIELDGN`/`SUPRTPLT`/`RECCENTR` (C++ `ALIEN_BUILDING_1/2/3`)
  — ждут решения владельца.
- **Дистилляция DSL**: приказы/состояния/ресурсы — таблицы готовы, вносятся в
  `domain-map.md`; дальше cross-check «мануал ↔ C++ ↔ наши имена».
- **Синхронизация** `max-brh-qa-tools/max-resources-exporter/src/names.rs` с
  каноничной таблицей имён (после Ф1 устарела).

### Реверс-инженер (дизассемблер оригинала; `max-original-dasm/`)

- **R2**: `UnitOrderState::State10` (0x0A) и `State21` (0x15) — безымянны в C++;
  найти следы в оригинале (используются ли, какая семантика).
- **R4**: семантика `Cargo.power`/`Cargo.life` в оригинале — подтвердить или
  опровергнуть сужение нашего `Cargo` (`raw/fuel/gold/free_capacity`).
- **RE-стенд** (фоновая линия, по потребности): расширение каталога функций.

### Архитектор (слои/границы; владелец архитектурных решений)

- **Граница `max_net → max_core`** зафиксирована (read-only, 5 проекций, CI-замок
  `check_maxnet_boundary.sh`): мониторить; пересмотр — при появлении `apply_*`/
  мутаций `GameState` из `max_net`.
- Новые архитектурные задачи — по запросу владельца/координатора.

### Разработчик (код; правила `rust-programming-rules.md`)

- **Цель 2 — режим хоста (P2P)**: этапы M-NET-HOST (локальный хост-арбитр),
  M-NET-JOIN (подключение гостя), M-NET-HOST-POLISH (реконнект/2–4 игрока) —
  [`development/PLAN-host-mode.md`](development/PLAN-host-mode.md).
- **Визуал — остатки**: C1/C2 (анимации шахты/завода по DOS-съёмке), C3/C4
  (MVE аудио/субтитры/форматы 0x06/0x10), D2 (музыка), B6 (debug-секция
  Preferences) — [`analysis/visual-1to1-plan.md`](analysis/visual-1to1-plan.md).
- **Спектатор** и **смешанная сессия Rust+C++** — блокированы (нет
  `TEAM_TYPE_SPECTATOR`/снапшота и headless client mode; см.
  [`architecture/client/protocol.md`](architecture/client/protocol.md) §9/§12).
- **Полная запись V70** (`GameState → SaveFile`): расширить охват с 2 эталонных
  сейвов на все сейвы `max-original/` + кросс-проверка C++-портом.
- **Хвосты рефакторинга**: call-sites `is_producing()`/`is_combat()`/`object_kind()`
  (появятся при разборе `Cargo_GetNetProduction`/логики).

### QA (тесты/качество; цели не закрывает без человека)

- **Ручной прогон пре-альфа → альфа**: составить список дефектов по режимам
  (строительство/бой/сеть/UI), фиксировать в `qa/` до подтверждения владельца.
- **Golden/QA** для новых экранов (host mode, оставшиеся анимации/звук).
- **Релизные гейты**: чек-лист `development/release-checklist.md`, live-smoke,
  soak — перед каждым релизом.

## 5. Порядок эскалации

1. Агент не может решить задачу в своей компетенции → **координатор**.
2. Координатор определяет, какая роль/компетенция нужна для решения вопроса, и
   **делегирует агенту соответствующей роли**.
3. Если роль определить не удалось, или делегат не справился и компетентных
   агентов больше нет → **вопрос владельцу (человеку)**; решение ожидается, не
   домысливается.

## 6. Карта документов

| Область | Документ |
|---|---|
| Агенты и правила | [`AGENTS.md`](AGENTS.md), [`development/rust-programming-rules.md`](development/rust-programming-rules.md), [`development/architect-rules.md`](development/architect-rules.md), [`development/analyst-rules.md`](development/analyst-rules.md), [`development/agent-programmer.md`](development/agent-programmer.md) |
| Термины/стиль | [`glossary.md`](glossary.md), [`style-guide.md`](style-guide.md) |
| Домен/классификация | [`domain-map.md`](domain-map.md), [`object-classifier.md`](object-classifier.md), [`unit-properties.md`](unit-properties.md), [`unit-name-review.md`](unit-name-review.md) |
| Клиент (цели/режимы) | [`architecture/client/README.md`](architecture/client/README.md), [`architecture/client/plan-modes.md`](architecture/client/plan-modes.md) |
| Сеть/протокол | [`architecture/client/protocol.md`](architecture/client/protocol.md), [`architecture/client/barrier-flow.md`](architecture/client/barrier-flow.md), [`architecture/server/`](architecture/server/) |
| Визуал 1:1 | [`analysis/visual-1to1-status.md`](analysis/visual-1to1-status.md), [`analysis/visual-1to1-plan.md`](analysis/visual-1to1-plan.md) |
| Сборка/CI/релиз | [`development/build.md`](development/build.md), [`development/release-checklist.md`](development/release-checklist.md), `max-brh-cicd/` |
| Исторический (сервер) | `architecture/server/dedicated-server-plan.md` |
