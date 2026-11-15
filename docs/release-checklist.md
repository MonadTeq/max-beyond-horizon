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
- [ ] Для человеческого сценария: Linux-хост с SDL3 и аудиоустройством либо
      Windows-хост (см. §6), плюс запущенный Horizon + headless.
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
cargo run -p max_app --features sdl -- --ui
# со звуком:
cargo run -p max_app --features sdl,sound -- --ui
```

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

## 6. Windows x64: инструкция сборки (бинарь здесь не собирается)

В этом окружении нет Windows-Rust-toolchain, поэтому бинарь не приложен;
сборка выполняется на Windows-машине и проверяется по пунктам ниже.

```powershell
# 1. Rust 1.94.0 + MSVC target
rustup toolchain install 1.94.0
rustup default 1.94.0
rustup target add x86_64-pc-windows-msvc

# 2. Visual Studio Build Tools 2022: "Desktop development with C++"
#    (MSVC, Windows SDK, CMake) — обязательны для линковки.

# 3. SDL3 через vcpkg
git clone https://github.com/microsoft/vcpkg C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat
C:\vcpkg\vcpkg install sdl3:x64-windows
$env:VCPKG_ROOT = "C:\vcpkg"        # переменная окружения для sdl3-sys

# 4. Сборка (vcpkg-фича max_app/max_render)
cd max-rust
cargo build --release -p max_app --features sdl-vcpkg
# вариант без vcpkg: --features sdl-from-source (нужен CMake + MSVC)

# 5. Рядом с exe положить SDL3.dll (vcpkg: C:\vcpkg\installed\x64-windows\bin\SDL3.dll)
#    и запустить:
.\target\release\max_app.exe --ui
```

- [ ] `cargo build --release -p max_app --features sdl-vcpkg` — успешно.
- [ ] `SDL3.dll` лежит рядом с `max_app.exe`; `max_app.exe --help` работает.
- [ ] `--ui` открывает окно, меню и партия играются мышью; звук (если собрано
      с `sound`) слышен без дополнительных DLL (WASAPI).
- [ ] Бинарь и DLL заархивированы в `release/max_app-1.0.0-windows-x86_64.zip`
      (собирается на Windows; в git не коммитится).

## 7. Документация и лицензии

- [ ] `README.md` (корень): раздел про Rust-клиент ссылается на
      `docs/release-checklist.md`, `release.sh` и звук.
- [ ] `max-rust/README.md`: версия 1.0.0, feature `sound`, ссылка на чек-лист.
- [ ] `CHANGELOG.md`: 1.0.0, список возможностей и ограничений, ссылка на
      `LICENSE`.
- [ ] `docs/release-checklist.md` (этот файл) актуален: команды и пути
      совпадают с `release.sh`/`ci.sh`.
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
      `max_app-1.0.0-linux-x86_64.tar.gz` и Windows-zip (если собран).

## 9. Архив артефактов

- [ ] `release/max_app-1.0.0-linux-x86_64.tar.gz` + `SHA256SUMS` сохранены.
- [ ] Windows-zip (если собран) + его sha256 сохранены.
- [ ] Логи гейтов: `max-rust/target/cargo-test.log` (если писали),
      `build/smoke-logs/*.log`, скриншоты M4 — приложены к релизной записи.
- [ ] Артефакты **не** закоммичены в git (`release/` в `.gitignore`).

## 10. После релиза

- [ ] Обновить `PLAN-v7-release.md`: M6.2/M6.4 отмечены, известные хвосты
      (M6.1-финал, FLC, звуки юнитов) перенесены в следующий milestone.
- [ ] Открыть issue на ограничения из `CHANGELOG.md` §«Ограничения 1.0.0».
- [ ] Проверить, что `main` после тега собирается `bash ci.sh`.

---

## Приложение A. Быстрый прогон (минимальный релиз-гейт)

```sh
bash ci.sh                                  # 2.1 + 2.4
bash build/scripts/live-smoke.sh 10         # §3
bash release.sh --dry-run                   # §5, проверка плана
bash release.sh                             # §5, артефакты
```

## Приложение B. Свидетельства

| Гейт | Дата | Команда/стенд | Результат | Ссылка |
|---|---|---|---|---|
| Тесты max-rust | | | | |
| Тесты Horizon | | | | |
| Live smoke 2×10 | | | | |
| M4 вручную | | | | |
| Linux-бинарь | | | | |
| Windows-бинарь | | | | |
| Тег v1.0.0 | | | | |
