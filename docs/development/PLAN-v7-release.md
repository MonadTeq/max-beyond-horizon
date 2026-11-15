# Дорожная карта до рабочего релиза (v7)

Версия: 7.0 · 2026-11-15 · Цель: `goal-7c4a8d2c` — довести Rust-клиент до рабочего релиза.

## Определение «рабочего релиза»

Rust-клиент запускается готовым бинарником (Windows x64 + Linux), подключается к
Horizon (gRPC), создаёт/входит в игру, играет **полную партию** (настройка → ходы →
экран победы/поражения) против headless с AI-противниками; **два и более клиента
без расхождений** (CRC-барьеры); реконнект после обрыва; релизный чек-лист выполнен.

## Статус на 2026-11-15

- [x] **M5.3** — compose-развёртывание Horizon+headless, healthcheck, README.
- [x] **M6.3** — `ci.sh` + GitHub Actions, смоук-стадия за флагом `--smoke`.
- [x] **M6.2** — звук: `max_assets::wav` (RIFF/WAVE PCM16, фикстура
  `KBUY0.SPW`), `max_app::audio` (`AudioPlayer`, noop + rodio за feature
  `sound`), клики меню; базовые сборки не тянут ALSA/CPAL.
- [~] **M6.4 — артефакты готовы**: версия 1.0.0, `CHANGELOG.md`,
  `docs/release-checklist.md`, `release.sh` (Linux-бинарь в Docker, архив,
  подготовка тега `v1.0.0`). Осталось выполнить гейты чек-листа: тесты max-rust
  + Horizon, live-smoke 2×10, человеческий сценарий M4, Windows-бинарь, тег.
- [x] **M6.1 — реконнект** (P5): ENet+`TP 00`, продолжение lockstep и resume-54
  своей команды; пределы (нет SNAP/spectator) — `protocol.md` §9.

## M3 — Игровой цикл и сетевой геймплей (headless-first)

Текущее: лобби проходит (28→…→34, «Main game loop start» у headless), барьеры
00/45/52 не завершаются.

- **3.1 Барьеры через релей**: разобрать семантику 00/45/52 по `src/remote.cpp` и
  `src/transport_udp_default.cpp` (звезда: клиенты релеят через сервер; headless —
  транзит). Клиент корректно шлёт/ждёт барьеры и передаёт CRC (45).
- **3.2 Первый ход headless**: приказы (08/38/41/50) → симуляция → конец хода
  (06+52+45) → следующий ход; сверка RNG-чекпоинтов (DBG rng cp) с C++.
- **3.3 Два клиента × 10+ ходов**: CRC сходится; автотест-реплей двух App против
  headless в контейнере (повторяемый скрипт).
- **Критерий**: скрипт `build/scripts/live-smoke.sh` гоняет 2 клиента 10 ходов,
  расхождений нет (логи + exit 0).

## M4 — UI и ввод (человек может сыграть)

- **4.1 SDL-окно**: главное меню, создание/вход в игру (gRPC), настройка партии
  (планета/таймер/противники).
- **4.2 HUD**: карта, миникарма, панель юнита, журнал сообщений, кнопки
  (конец хода, меню, чат).
- **4.3 Ввод мышью**: выделение юнита, приказы move/attack/build, предпросмотр
  маршрута, хоткеи; пакеты 08/38/41/50 из UI.
- **4.4 Чат** (пакет 18).
- **Критерий**: одиночная партия против AI от меню до экрана победы только мышью.

## M5 — Серверная интеграция и эксплуатация

- **5.1 Horizon**: `host` из конфига/тикета (не захардкоженный 127.0.0.1);
  gRPC+WS параллельно — покрыто smoke-скриптом; фикс двух старых падающих тестов
  horizon_event_system (или осознанный exclude).
- **5.2 Headless**: обход/фикс падения при PATCHES.RES в game_data (патч в нашем
  `src/` или репорт автору); задокументировать.
- **5.3 Развёртывание — готово (M5.3)**: `docker-compose.yml` (Horizon +
  C++-headless), образ `ubuntu:24.04` + готовый Linux-бинарь
  `max-horizon-server/target/release/horizon` volume-маунтом (плюс optional
  `--profile build`), порты 8080/8082 + UDP 32000-32100, конфиг из
  `smoke_config.toml` через `deploy/horizon.toml.template` + `entrypoint.sh`
  (`HORIZON_ANNOUNCE_HOST`, пути, секреты), healthcheck по LISTEN-сокетам.
  Проверено: `docker compose config` валиден, `up` → `healthy`, gRPC-проба
  `horizon_probe` (hello/list/create/join/ping) проходит; документация —
  `max-horizon-server/README.Docker.md`.
- **Критерий**: `docker compose up` → два клиента играют по инструкции.

## M6 — Полировка и релиз

- **6.1 Реконнект — P5**: `ReconnectPolicy` (ENet+`TP 00` тем же тикетом) +
  продолжение lockstep; после возврата `TurnBarrierFsm::resume_state_sync`
  публикует 54 своей команды, встречные 54 исправляют удалённые, барьеры
  52/54/45 доигрываются. Пределы: SNAP нет, 54 — только юниты/экономика одной
  команды, C++-пиры ELIMINATED через 30 с (`protocol.md` §9.2). Спектатор
  невозможен без серверного `TEAM_TYPE_SPECTATOR`/снапшота. Тесты: 3-клиентный
  мок-релей, resume-54, retry; живой smoke `live-smoke-3.sh`.
- **6.2 Звук — выполнено (M6.2)**: WAV-декодер `max_assets::wav` (PCM16,
  моно/стерео; 272 реальных `*.SPW`), ASCIIZ-ссылки `MAX.RES`, `AudioPlayer` с
  noop-backend'ом и rodio за feature `sound`, клики меню в `--ui`; полный
  вывод — `docs/release-checklist.md` §2.2. FLC-ролики — по-прежнему не
  блокер.
- **6.3 CI — готово (M6.3)**: `ci.sh` (локальный CI в Docker: `cargo
  check`+`cargo test` max-rust + `cargo check/test --features sound`,
  `cargo test --workspace` Horizon, optional `--smoke` →
  `build/scripts/live-smoke.sh`) и `.github/workflows/ci.yml` (push/PR);
  использование описано в `README.md`, `max-horizon-server/README.Docker.md`,
  `max-rust/README.md`.
- **6.4 Релиз — артефакты готовы (M6.4)**: версия 1.0.0, `CHANGELOG.md`,
  `docs/release-checklist.md`, `release.sh` (сборка Linux-бинаря в Docker,
  `release/` с README/LICENSE/SHA256SUMS/архивом, тег `v1.0.0` по `--tag`).
  Бинарники Windows x64 + Linux, README и git tag выпускаются по чек-листу.

## Гейты релиза

1. Все тесты зелёные: workspace `max-rust` + `max-horizon-server` (с учётом 5.1).
2. Живой smoke: 2 клиента × 10+ ходов, CRC сходится (скрипт).
3. Человеческий сценарий: партия против AI от меню до экрана победы.
4. Документация: README, deploy, smoke, чек-лист релиза (`docs/release-checklist.md`).

## Порядок исполнения

M3 → M4 → M5 → M6, параллельно — фиксы, мешающие текущему milestone.
Каждый шаг: коммит + критерий.
