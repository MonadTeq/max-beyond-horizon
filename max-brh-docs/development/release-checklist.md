# Чек-лист релиза Rust-клиента M.A.X.

Версия: **1.1.0** · дата среза: **2026-11-16** · тег: **v1.1.0** (не создан —
создаётся релиз-менеджером; команда подготовлена в §v1.1.0 ниже).

Документ — исполняемая последовательность: каждый пункт либо команда с
ожидаемым результатом, либо ручная проверка с местом для свидетельства.
Релиз считается выпущенным, когда отмечены **все** пункты разделов 1–9 и
артефакты раздела 10 опубликованы.

> **Историческая часть.** Разделы 0–10 и приложения A–D описывают релиз
> **1.0.0** (2026-11-15) и остаются как свидетельства предыдущего среза.
> Для **1.1.0** актуальны раздел «v1.1.0 — итоговый чек-лист» (ниже) и
> артефакты `bin/release/max_app-1.1.0-*`.

## v1.1.0 — итоговый чек-лист (2026-11-16)

| # | Пункт | Команда/свидетельство | Результат |
|---|---|---|---|
| 1 | Версия workspace | `grep -n 'version = "1.1.0"' max-brh/Cargo.toml`; `Cargo.lock` обновлён `cargo check` | выполнено: `[workspace.package] version = "1.1.0"`, все 5 крейтов 1.1.0 |
| 2 | CHANGELOG | `CHANGELOG.md` | выполнено: `[1.1.0] — 2026-11-16` + состав с v1.0.0 (режимы, визуал, производство, сейвы, сеть, QA, rename, GUI-default) |
| 3 | Полный CI | `bash ci.sh --smoke --smoke3 --turns 10` (Docker, стадии 1–4) | выполнено: P8 — `CI OK за 566 с` (`build/ci-p8.log`): тесты 0 failed, QA/golden/soak PASS |
| 4 | Тесты max-brh | стадия 1/4 (`cargo check && cargo test`; `--features sound`; `--no-default-features`) | выполнено: **0 failed**; в P8 добавлено: `researchmenu` 10, `helpmenu` 9, `ui` +3, `qa_driver` +1 (264 lib-теста `max_app`, 14 `qa_driver`, 220 `max_core`, 86 `max_net`, 79 `max_render`) |
| 5 | QA-сценарии | `qa_run`: sp-full, hs-2, visual-screens, victory, production, save-load, **upgrade**, **research**, edge-cases, **cargo-full**, **research-help** | выполнено: все PASS; golden-дрейф 0 |
| 6 | Golden новых экранов | `golden/cargo/`, `golden/upgrade/`, `golden/research/`, `golden/research-help/` (9: Help-окна + `research-menu`) | выполнено: 0 пикселей отличий |
| 7 | Soak/память | `soak-100.qa` + `soak_headless --turns 120` (`MAX_TICK_PROFILE=1`) | выполнено: PASS; бюджет 60 Гц, RSS в пороге 64 МиБ |
| 8 | Live smoke 2×10 | `max-brh-cicd/scripts/live-smoke.sh 10` | выполнено: PASS — `turns=10`, `barrier_timeouts=0`, `data_hash=dd24703e(64)`, `state_checksum=0x7035` у A/B |
| 9 | Live smoke 3×10 | `max-brh-cicd/scripts/live-smoke-3.sh 10` | выполнено: PASS, `пиров=2`, `barrier_timeouts=0`, CRC `0x34D5` у трёх клиентов |
| 10 | Windows игровая сборка | `powershell -File max-brh-cicd\scripts\build-windows-ui.ps1` | выполнено: `bin/release/max_app-1.1.0-windows-x86_64-ui/` (2026-11-19, X-FER + хранилища): `max-brh.exe` 6 612 480 Б, sha256 `3a7994d531a2d469f2de6b89e26a89ce5e3250b3ae0eae81144bd4fa8e96bce9`; zip sha256 `d4cc8be4ddb2b4b1cef6cf0744ef8630aeae87742b8498f6a23c2f74858abcb3`; PE Subsystem=2 (GUI), MSVC + vcpkg SDL3 3.4.20, sound; `--help` OK |
| 11 | Windows консольная сборка клиента | `powershell -File max-brh-cicd\scripts\build-windows-console.ps1` (нативный MSVC) | выполнено: `bin/release/max_app-1.1.0-windows-x86_64/` (2026-11-21, пересобран нативно): `max-brh.exe` 4 433 408 Б, sha256 `2a36cd6e13e5e590778a98bea77015722ea9e62d7d6b6e6aa27112dc10a9807d`; zip 1 807 530 Б, sha256 `f5388c39c52c6d2a069f4bfdc6087cba8efa323e3e98d96207305542069f06f1`; PE Subsystem=3 (console); `--help` и `--demo SAVE1.CAM --turns 1` (`state_digest=0x733D`) — нативно, без wine |
| 12 | Linux | `bash release.sh` (rust:1.94.0-bookworm, `sdl-from-source,sound`) | частично выполнено: пересобраны Windows-сборки (UI и headless); Linux-бандл 1.1.0 остаётся прежним (X-FER/хранилища в него не входят) |
| 13 | SHA256SUMS | `release/*/SHA256SUMS`, `*.zip.sha256`, `*.tar.gz.sha256` | выполнено: Windows-сборки (UI и headless) пересобраны (sha256 в строках 10–11); Linux — прежний `*.tar.gz.sha256` |
| 14 | Документация | `README.md`, `max-brh/README.md`, `max-brh-docs/development/build.md`, `max-brh-docs/analysis/visual-1to1-status.md` §29–30, `max-brh-qa/suite/README.md` | выполнено: версия 1.1.0, X-FER (`TransferMenu`/`ProcessOrderTransfer`) и хранилища комплекса, матрица QA: 193 выполнено / 16 отложено |
| 15 | Оракулы не тронуты | `git status --short max-cpp-port max-original max-original-dasm` | проверено: пусто (C++-порт и оригинал не менялись) |
| 16 | Тег | `git tag -l 'v1.1.0'` | проверено: пусто — **не создан** по указанию; команда: `bash release.sh --tag` (чистое дерево) |

**Остаточные ограничения 1.1.0** (не блокируют релиз, отражены в QA как
`not planned` со статусом «отложено»): спектатор (нужна серверная поддержка `TEAM_TYPE_SPECTATOR`),
полная mixed-сессия Rust+C++ (headless `src/` поддерживает только `--server`),
реальный alt-tab/авто-масштаб (WINDOW-проверки), сквозной
бой/поражение в QA (нет команды урона), MXLS-загрузка (полный путь — V70),
пакет 09 (золото команды) в сети — апгрейды идут пакетом 10.

**P7 (предвысадочный `CargoMenu`)** добавил полный экран мобилизации: иконки
`I_*` в списках, полоса золота и cargo-слайдер, полный список покупки (15
типов), режим модернизации (62 типа по умолчанию, фильтры, апгрейды до 8
атрибутов), C++-векторы `tools/cargo_vectors` (0 расхождений), QA
`cargo-full.qa` и golden `golden/cargo/`. Доведено: портрет `P_*` 300×240
1:1 (big image, палитра окна, окно 280×240), описания юнитов
(`GetDescription()` → `SC_UNITS` + `language.json`, 93 записи в
`data/cargo_unit_data.json`), таблица `UnitStats_DrawStats` с сеткой `I_*`
pips, дельтой апгрейда и красными линиями; второй кадр башни/пушки иконок
(`D_*` → `angle_offsets[2] >> divisor`) и её отдельный кадр на карте/HUD/
в меню апгрейдов. Остаточные отличия CargoMenu: у 24 служебных типов описания
нет и в оригинале (в `CargoMenu` недостижимы); `Help` открывает окно
`CARGO_SETUP` (`HELPFRAM`), как и остальные `?`/`Help`-кнопки (P8).

**P8 (ResearchMenu + Help-экраны)** закрыл последние визуальные пробелы:
`ResearchMenu` (`RSRCHPIC`, 8 тем, распределение центров, `R`) и окно
`HELPFRAM` для `OPTIONS_SETUP`/`PLANET_SETUP`/`NEW_GAME_SETUP`/`CARGO_SETUP`
(+`RESEARCH_SETUP`/`UPGRADES_SETUP`/`GAME_SCREEN_SETUP`+`Keys`); тексты —
`data/help.json` (32 секции, 661 запись). QA `research-help.qa`, 9 golden
`golden/research-help/`, команды `research …`/`help …` и проверки
`research-menu`/`research-alloc`/`help-section`/`help-row`. Отличие: окно
показывает текст секции целиком, а не абзац под курсором (C++ help-режим);
пакет 09 не шлётся, звуки не проигрываются.

---


Обозначения:

- `[ ]` — не проверено; `[x]` — проверено; `[!]` — осознанное исключение с
  причиной (записать рядом).
- `$ROOT` — корень репозитория (каталог с `max-brh/`, `assets/`,
  `max-original/`).

## 0. Предпосылки и данные

- [ ] Docker с Linux-контейнерами (`docker info`), образ
      `rust:1.94.0-bookworm` доступен (или `RUST_IMAGE` указывает на него).
- [ ] Данные оригинальной игры на месте: `max-original/MAX.RES`,
      `max-original/COLOR.PAL`, `assets/PATCHES.RES`, звуки
      `max-original/*.SPW`, карты `*.WRL` (или `--world-dir`).
- [ ] Для live-smoke: собран `max-brh-server/target/release/horizon` и
      C++-headless `bin/linux-server-template/max-port`.
- [ ] Для Windows-сборок (§6 и §6.1): **Windows-хост** с VS (workload
      «Desktop development with C++»: MSVC + Windows SDK + CMake) и Rust 1.94.0
      (`x86_64-pc-windows-msvc`) — headless- и UI-сборки делаются нативной
      MSVC-сборкой; Docker/mingw для Windows не нужны. На текущем хосте всё
      это уже стоит (см. таблицу в §6.1); MSVC ставится только установщиком VS
      и требует прав администратора.
- [ ] Для UI-сборки (§6.1) дополнительно: vcpkg с `sdl3:x64-windows`.
- [ ] Для человеческого сценария: Linux-хост с SDL3 и аудиоустройством либо
      Windows-хост (см. §6.1), плюс запущенный Horizon + headless.
- [ ] Нет незакоммиченных изменений в оракулах: `max-cpp-port/`,
      `max-original/`, `max-original-dasm/` (релиз их не меняет).

## 1. Заморозка версии

- [ ] `git status --short` — рабочее дерево чистое (кроме `release/`, он в
      `.gitignore`).
- [ ] Версия workspace `max-brh` = `1.0.0`:

      grep -n 'version = "1.0.0"' max-brh/Cargo.toml
      grep -rn 'version.workspace = true' max-brh/crates/*/Cargo.toml

  Ожидается: `[workspace.package] version = "1.0.0"`; все пять крейтов
  (`max_core`, `max_net`, `max_assets`, `max_render`, `max_app`) наследуют её и
  не переопределяют `version`.
- [ ] `max-brh/Cargo.lock` обновлён (`cargo check` меняет версии локальных
      пакетов на 1.0.0); закоммичен вместе с релизом.
- [ ] `CHANGELOG.md` содержит секцию `[1.0.0]` с датой и ограничениями.
- [ ] `LICENSE` (MIT) на месте; ссылки на лицензию есть в `CHANGELOG.md` и
      `release/…/README.txt`.

## 2. Автоматические проверки

Все команды — через Docker, чтобы не зависеть от хостового toolchain:

```sh
# 2.1 Базовые тесты max-brh. default max_app = ["sdl-from-source"]: SDL3 3.4.20
# собирается из исходников через CMake, поэтому нужны dev-заголовки
# X11/Wayland/ALSA/GL/KMSDRM/dbus/udev, а тестам — LD_LIBRARY_PATH на
# target/debug (туда sdl3-sys кладёт libSDL3.so.0).
docker run --rm -v "$ROOT:/src" -w /src/max-brh rust:1.94.0-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libasound2-dev libx11-dev libxext-dev libxrender-dev libxcursor-dev libxi-dev libxrandr-dev libxinerama-dev libxss-dev libxtst-dev libxfixes-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols libdecor-0-dev libegl-dev libgles-dev libgl-dev libgbm-dev libdrm-dev libdbus-1-dev libudev-dev >/dev/null && export LD_LIBRARY_PATH=/src/max-brh/target/debug && cargo check && cargo test'

# 2.2 Звук: WAV-декодер + rodio за feature sound (нужен libasound2-dev;
# default-фичи GUI остаются включены)
docker run --rm -v "$ROOT:/src" -w /src/max-brh rust:1.94.0-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libasound2-dev libx11-dev libxext-dev libxrender-dev libxcursor-dev libxi-dev libxrandr-dev libxinerama-dev libxss-dev libxtst-dev libxfixes-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols libdecor-0-dev libegl-dev libgles-dev libgl-dev libgbm-dev libdrm-dev libdbus-1-dev libudev-dev >/dev/null && export LD_LIBRARY_PATH=/src/max-brh/target/debug && cargo check --features sound && cargo test --features sound'

# 2.3 Headless-путь (--no-default-features): SDL не тянется ни при сборке, ни в
# рантайме — так собираются Windows консольная сборка клиента, QA и серверные артефакты; этот
# прогон проверяет и cfg-ветки main.rs без feature sdl.
docker run --rm -v "$ROOT:/src" -w /src/max-brh rust:1.94.0-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config >/dev/null && cargo test --no-default-features'

# 2.4 Horizon + C++-адаптер
docker run --rm -v "$ROOT/max-brh-server:/src" -w /src rust:1.94.0-bookworm \
  bash -c 'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libssl-dev >/dev/null && cargo test --workspace'

# 2.5 Полный локальный CI (2.1 + 2.4 + QA-сценарии/эталоны; с --smoke добавляет §3)
bash ci.sh

# 2.6 QA-слои 2–3 вручную (то же, что стадия 3/3 ci.sh; подробности — max-brh-qa/scripts/README.md)
#     примеры идут с --no-default-features: QA-драйверу окно не нужно;
#     сценарии/эталоны лежат в ../max-brh-qa/ (рабочий каталог — /src/max-brh)
docker run --rm -v "$ROOT:/src" -w /src/max-brh rust:1.94.0-bookworm bash -c '
  set -e
  apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null
  rm -rf qa-out
  cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-full.qa qa-out/sp-full
  cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/hs-2.qa qa-out/hs-2
  cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/visual-screens.qa qa-out/visual-screens
  cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/victory.qa qa-out/victory
  cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden'
```

- [ ] 2.1 `cargo check` и `cargo test` — зелёные, 0 failed (default-фичи =
  GUI: SDL3 3.4.20 собирается из исходников через CMake; тесты идут с
  `LD_LIBRARY_PATH=target/debug`).
  Свидетельство (прежний прогон, когда SDL был не-default): **705 тестов,
  0 failed**, `cargo check` — 0 ошибок и 0 предупреждений (2026-11-15,
  `rust:1.94.0-bookworm`); M-SP 14 в `tests/local_sp.rs`, фазы старта 10 в
  `tests/start_phases.rs`, M-HS 13 в `tests/hot_seat.rs`, M-NET 7 в `net.rs`.
  Повторный прогон с default `sdl-from-source`: ______________________
- [ ] 2.2 `--features sound` — `check` и `test` зелёные. Тест
      `max_assets::wav::tests::parses_real_kbuy0_spw` проходит на реальном
      `KBUY0.SPW`; `all_real_spw_files_are_pcm16_22050_mono` проверяет все 272
      файла (`cargo test -p max_assets wav::`).
  Свидетельство: ______________________
- [x] 2.3 Headless-путь (`--no-default-features`) — зелёные `cargo test`
  (сборка с SDL, теперь default, проверяется в 2.1, окно — §4).
  Свидетельство: SDL3 3.4.20 собирается из исходников без системного SDL3 и
  vcpkg — 0 ошибок (Linux, полный X11/ALSA dev-набор, 2026-11-15,
  `max_app` с `frame_scale`/`SdlWindow::new_scaled`/`window_to_frame`);
  лог `build/cargo-docker-ef-sdl-final.log` (тогда проверялось
  `--features sdl-from-source` — сейчас это default).
- [ ] 2.4 Horizon: `cargo test --workspace` — зелёный (известные сбои
      `horizon_event_system` должны быть исправлены или явно исключены — M5.1).
  Свидетельство: ______________________
- [ ] 2.5 `bash ci.sh` — `CI OK` за разумное время.
  Свидетельство: ______________________
- [x] 2.6 QA-слои 2–3 (`max-brh-qa/scripts/`): сценарии `sp-full`, `hs-2`,
      `visual-screens`, `victory` — PASS, golden-кадры без дрейфа (порог:
      1 % пикселей с Δ>30). Свидетельство: 2026-11-15,
      `rust:1.94.0-bookworm`; `qa-out/*/log.txt` (`RESULT: PASS`),
      `qa_golden_check: сверено 10 кадров (980 КиБ эталонов)` — 0 дрейфа;
      повторный прогон `visual-screens.qa` даёт 0 отличающихся пикселей;
      `cargo test -p max_app --test qa_driver` — 11 тестов, 0 failed (P4:
      команда `mem`, структура `soak-100.qa`, прогон `edge-cases.qa`).
- [x] 2.7 Golden-эталоны проверены визуально (каждый `diff-*.png` после
      осознанного изменения рендера объяснён и закоммичен вместе с ним).
  Свидетельство: 2026-11-15 — обновлены `players.png` (эмблемы кланов теперь
  с якорем 41,40 — сверено с кадром C++-порта), `game.png` (ИИ выбирает точку
  сам → иначе расставлены маркеры ресурсов; 148 px, 0.05 %) и `winloss.png`
  (32 px, 0.01 % — мелкая статистика партии), добавлены `mobilize.png` и
  `landing.png` (новые экраны фаз старта).
- [x] 2.8 Фазы старта M-SP: `sp-full.qa` проходит мобилизацию и выбор точки
      высадки; `hs-2.qa` — фазы для GREEN и RED по очереди (порядок C++
      GRAY→RED); `cargo test -p max_app --test start_phases` — 10 тестов
      (точка → база, покупки → состав/кредиты, ИИ-точки, старый путь без фаз).
  Свидетельство: 2026-11-15, `rust:1.94.0-bookworm`; кадры
  `qa-out/*/mobilize.png`, `landing.png`; golden `players.png` обновлён после
  исправления эмблем кланов (якорь 41,40), знак `?` сверен с кадром самого
  C++-порта (Alt+C) — совпадение.
- [x] 2.9 **Soak и производительность P4** (ночной/релизный гейт):
      `ci.sh` стадия 4/4 — `soak-100.qa` (100 ходов человек+ИИ,
      `MAX_TICK_PROFILE=1`) и `soak_headless --turns 120` (ИИ×ИИ); стадия 3/4
      дополнительно запускает `edge-cases.qa` (многократные клики/юникод/повторный
      старт). Критерии: партия доходит до конца, память без линейного роста,
      кадр укладывается в бюджет 60 Гц (16.67 мс).
  Свидетельство: 2026-11-16, `rust:1.94.0-bookworm`, debug-сборка CI:
  * `soak-100.qa` — **PASS**, 100 ходов, 12 208 кадров, V70-сейв 157 921 Б;
    RSS 54 696 → 60 944 КиБ (**+6.1 МиБ**, после 20-го хода плато — прогрев
    аллокатора, не линейный рост); `PROFILE`: средний кадр **289 µs**,
    max **15.69 мс**, кадров дольше 16.67 мс — **0**; `finish_turn` на 100-м
    ходу **23.7 µs** (avg 23.6 µs, max 56.6 µs, ход 48).
  * `soak_headless` (debug) — **PASS**, 120/120 ходов, 362 502 кадра, 21.8 с,
    avg хода 181.7 мс; RSS 53 548 → 53 708 КиБ (**+160 КиБ**, peak 53 884);
    средний кадр 59.9 µs; один планировочный выброс 121.9 мс (1 кадр из
    362 502 — не вычисление).
  * `soak_headless` **`--release`** — **PASS**, 120/120 ходов, 362 502 кадра,
    2.47 с, avg хода 20.6 мс; RSS 51 544 → 51 824 КиБ (**+280 КиБ**);
    средний кадр **6.76 µs**, max 5.21 мс, кадров дольше бюджета 60 Гц —
    **0**; `finish_turn` avg 58 µs, max 5.19 мс (24-й ход), на 120-м 9.4 µs.
  * `edge-cases.qa` — **PASS**, 125 кадров, 8 снимков, паник нет.
  * Логи: `max-brh/qa-out/soak-100/log.txt`,
    `max-brh/qa-out/soak-headless/soak.txt`,
    `max-brh/qa-out/edge-cases/log.txt`.
  Повторный прогон перед тегом (после последних изменений кода): __________.

## 3. Smoke на реальном стенде: 2 клиента × 10 ходов

Скрипт: `max-brh-cicd/scripts/live-smoke.sh` (Docker: Horizon + C++-headless + два
Rust-клиента в общем network namespace).

```sh
# Git Bash: MSYS_NO_PATHCONV=1 — иначе -w /src/... станет C:/Program Files/Git/src/...
MSYS_NO_PATHCONV=1 bash max-brh-cicd/scripts/live-smoke.sh 10
# опционально: bash ci.sh --smoke --turns 10
```

`--handshake-patches` скрипт **не** передаёт: клиент сам пробует полный
`data_hash`, получает `TP 03 GameData` и переподключается с хэшем только
`MAX.RES`, а скрипт проверяет `data_hash=<hex>(64)` и отсутствие
`Session failed` в логах.

- [x] Horizon поднялся (`horizon-live` healthy), `horizon_probe` создал игру,
      `GAME_ID` непустой. _(2026-11-15: `83621ef3-6153-4da7-9b0b-ca05320ee2c5`)_
- [x] Оба клиента (`client-a`, `client-b`) завершились с exit 0, в логах
      `build/smoke-logs/a.log` и `b.log` нет паник/таймаутов. _(2026-11-15)_
- [x] В логах обоих клиентов одинаковые CRC-барьеры (`45`) на каждом ходу;
      расхождений (desync) нет. _(2026-11-15: `state_checksum=0x7035` у обоих,
      `barrier_timeouts=0`)_
- [x] Пройдено ≥ 10 ходов, `turns_finished`/`ticks` в отчёте соответствуют
      ожиданию. _(2026-11-15: `turns=10`, `ticks=10`, `join_attempts=1`,
      `data_hash=dd24703e(64)` у A и B — fallback сработал)_
- [x] Скрипт воспроизводим: повторный запуск даёт тот же результат (не
      обязательно тот же CRC между разными seed, но без desync внутри партии).
      _(2026-11-15: два прогона — `83621ef3-…` и `87b1c4dc-…`; оба 10 ходов,
      `data_hash=dd24703e(64)`, `barrier_timeouts=0`)_

Свидетельство (дата, GAME_ID, хвосты логов): 2026-11-15,
`GAME_ID=87b1c4dc-9e71-4256-b4b0-39b475c7c44e` (повтор: `83621ef3-…`),
`build/live-smoke-mnet-final.log` (+ `build/live-smoke-mnet.log`),
`build/smoke-logs/a.log`/`b.log`; оба клиента
`handshake=true lobby=true turns=10 barrier_timeouts=0`, fallback data_hash
подтверждён (`live-smoke: fallback data_hash (MAX.RES-only) подтверждён`).

### 3.1 P5: три клиента и Rust+C++

```sh
bash max-brh-cicd/scripts/live-smoke-3.sh 5
bash max-brh-cicd/scripts/live-smoke-mixed.sh 5
```

- [x] `live-smoke-3.sh`: 3 Rust-клиента (RED/GREEN/BLUE), `--min-players 3`;
      `пиров=2`, релей 00 от двух других каждой команде, 00/52/54/45 пройдены,
      `barrier_timeouts=0`, `state_checksum` совпал у всех.
      _(2026-11-16: 3×10 — PASS, `0x34D5` у трёх; логи `build/smoke-logs/live3-*.log`)_
- [x] `live-smoke-mixed.sh`: Rust×2 ×10, CRC `0x7035`, C++-сервер без
      `Desync check failed`. _(2026-11-16: PARTIAL PASS)_
- [ ] Полная mixed-сессия Rust-клиент + C++-клиент: **блокер** — headless
      `src/` поддерживает только `--server`, GUI-клиенту нужен X11 и ручной ввод
      (`src/main.cpp`). См. `max-brh-docs/architecture/client/protocol.md` §9/§12.
- [x] Реконнект-состояние: in-process 3-клиентный мок-тест (B offline → resume-54
      → CRC/turn сходятся); реальный pause/proxy smoke не автоматизирован.
      Пределы — `protocol.md` §9.2.

## 4. Человеческий сценарий M4 (SDL, мышь)

Выполняется на машине с SDL3 и (для звука) аудиоустройством. Сервер — по
`max-brh-server/README.Docker.md`:

```sh
docker compose up -d --build          # Horizon + headless
# клиент из исходников: GUI — default (SDL3 из исходников, нужен cmake)
cargo run -p max_app -- --turn-wait 60
# со звуком:
cargo run -p max_app --features sound -- --turn-wait 60
```

`--turn-wait 60` обязателен для интерактивной игры: без него сетевой цикл закрывает ход
сразу после тика (~16 мс) и клики не успевают попасть в партию — это режим для
headless/live-smoke. **Обходные флаги не нужны (M-NET-доводки):** если каталог
данных headless-сервера без `PATCHES.RES` (как в `max-brh-server/docker-compose.yml`), клиент
сам после `TP 03 GameData` переподключается с хэшем только `MAX.RES`; пока
сессия инициализируется 10–15 с, `SessionHandle::start` повторяется с backoff
(статус `Session warming up — attempt N/M`), поэтому `Create game` сразу ведёт
в партию. `--handshake-patches <файл>` остался явным override'ом (один вариант
хэша, без fallback) — нужен только для нестандартного стенда. Подробности —
`max_app/README.md`, раздел «Сетевая партия в окне».

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
      при приказе — `UNIT_GO`/`UNIT_NGO` (SDL; в default-сборке окно включено).
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
      игра не завершается аварийно.
- [ ] Меню → выход из сессии → главное меню; повторный вход в игру работает.
- [ ] Окно закрывается/Esc — корректное завершение процесса (exit 0).

Свидетельство (скриншоты/лог, дата, ОС): ______________________

### 4.1. Hot-seat (M-HS) в интерактивном режиме, без сети

Локальная партия, сервер не нужен (`cargo run -p max_app` — GUI по умолчанию,
`--ui` — alias).

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
- [ ] Партия доходит до экрана победы/поражения без аварийных завершений.

Свидетельство: `build/hs-players.png`, `build/hs-turn.png`, `build/hs-game.png`,
`build/hs-ai.png`; дата/ОС: ______________________

## 5. Linux-сборка

`release.sh` собирает в Docker **интерактивную** сборку с
`--no-default-features --features sdl-from-source,sound` (решение
зафиксировано здесь: релизный Linux-клиент — с окном и звуком; явно указанные features
плюс `--no-default-features`, чтобы default-набор не добавлялся; консольная сборка клиента
для серверов — `bash release.sh --features ''`, то есть
`cargo build --release -p max_app --no-default-features`). Скрипт кладёт всё в
`bin/release/max_app-<version>-linux-x86_64/` и `.tar.gz`.

```sh
bash release.sh --dry-run     # что и где будет собрано, без запуска Docker
bash release.sh               # реальная сборка + упаковка
# потом (после проверки артефактов):
bash release.sh --tag         # сборка + аннотированный тег v1.0.0
```

- [ ] `bash release.sh --dry-run` завершается 0 и печатает команду сборки,
      список файлов и путь `release/…`.
- [ ] `bash release.sh` завершается 0; в `release/` есть каталог и `.tar.gz`.
- [ ] В каталоге: `max-brh`, `run.sh`, `README.txt`, `CHANGELOG.md`, `LICENSE`,
      `SHA256SUMS` и `lib/` (если SDL3 не статический).
- [ ] `ldd release/…/max-brh` с `LD_LIBRARY_PATH=release/…/lib` не содержит
      `not found`; `libSDL3.so.0` лежит в `lib/` (или залинкован статически).
- [ ] `release/…/max-brh --help` через `run.sh` запускается на чистом
      Linux-хосте (или в `ubuntu:24.04` c `libasound2`) и печатает справку:
      сырой `max-brh` требует `LD_LIBRARY_PATH=…/lib`, это делает `run.sh`.
- [ ] `release/…/run.sh --demo max-original/SAVE1.CAM --turns 1` печатает
      digest (`ticks=1 … units=312`), exit 0 — headless-путь реального бинаря.
- [ ] `release/…/run.sh` (без флагов) открывает окно и обрабатывает клики (на хосте с
      дисплеем и звуком); без дисплея — понятная ошибка SDL, не паника.
      `run.sh --ui` — совместимый alias.
- [ ] `sha256sum -c release/…/SHA256SUMS` — все файлы `OK`.
- [ ] `tar -tzf bin/release/max_app-1.0.0-linux-x86_64.tar.gz` содержит тот же
      набор файлов (без посторонних файлов/`target`).

Свидетельство (размер, sha256, дата): ______________________

### 5.1. Runtime-зависимости Linux-артефакта

- `libasound2` (ALSA) — для feature `sound`; без неё клиент не запустится,
  поэтому звук в релизе включается осознанно. Для headless-сборок sound не
  включается.
- `libSDL3.so.0` — из `lib/` рядом с бинарём (`run.sh` добавляет
  `LD_LIBRARY_PATH`); системный SDL3 не требуется.
- X11/Wayland — для окна; headless-режимы (`--demo`, `--new-game`,
  `--connect` без `--sdl`) работают без дисплея.

## 6. Windows x64: headless- и UI-сборки нативной MSVC-сборкой

Обе Windows-сборки делаются **на Windows-хосте тулчейном MSVC**; Docker,
mingw-w64 и wine для Windows больше не используются (кросс-сборка
`x86_64-pc-windows-gnu` в контейнере удалена вместе с
`max-brh-cicd/scripts/build-windows.sh`). консольная сборка клиента — этот раздел, скрипт
`max-brh-cicd/scripts/build-windows-console.ps1` (описание —
`max-brh-cicd/scripts/README.md`); игровая сборка — §6.1
(`build-windows-ui.ps1`).

Термины (консольная сборка клиента, игровая сборка, нативная MSVC-сборка) — в
`max-brh-docs/development/build.md`, раздел «Термины (сборка и артефакты)».

Собирается **headless**-вариант (`--no-default-features`: без `sdl*`, без
`sound`) — он закрывает `--demo`, `--new-game` и `--connect` (headless-цикл
Horizon gRPC + ENet). Окно SDL3 и звук в этот бинарь не входят: они собираются
отдельным артефактом §6.1 (vcpkg + feature `sound`).

```powershell
# Windows-хост: VS (MSVC + Windows SDK) и Rust 1.94.0 уже стоят
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1
# только собрать и проверить, без упаковки:
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1 -NoPackage
# без данных игры (пропустить --help/--demo):
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1 -SkipSmoke
```

Скрипт находит VS через `vswhere`, поднимает окружение `vcvars64.bat`, задаёт
`CARGO_TARGET_DIR=<repo>\bin\target-console` и выполняет (target по умолчанию —
нативный `x86_64-pc-windows-msvc`, без `--target`):

```sh
cargo build --release --locked -p max_app --no-default-features
```

Артефакты:

```
bin/release/max_app-<ver>-windows-x86_64/            max-brh.exe, README.txt (нативная
                                                 MSVC-сборка, зависимости), LICENSE,
                                                 CHANGELOG.md, SHA256SUMS
bin/release/max_app-<ver>-windows-x86_64.zip         архив каталога
bin/release/max_app-<ver>-windows-x86_64.zip.sha256  sha256 архива
```

- [x] **Нативная сборка MSVC (2026-11-21)**: `build-windows-console.ps1` →
      `bin/release/max_app-1.1.0-windows-x86_64/`; `max-brh.exe` **4 433 408 Б**,
      sha256 `2a36cd6e13e5e590778a98bea77015722ea9e62d7d6b6e6aa27112dc10a9807d`;
      zip **1 807 530 Б**, sha256
      `f5388c39c52c6d2a069f4bfdc6087cba8efa323e3e98d96207305542069f06f1`;
      Docker/mingw/wine не использовались.
- [x] PE-заголовок: `PE32+ x64`, первые байты `MZ`; сборка прошла без правок
      кода — `max_net/build.rs` уже выбирает платформенный файл ENet по
      `CARGO_CFG_TARGET_OS`, `tonic` подключён без `default-features`
      (`router,transport,codegen`; TLS — opt-in фича `tls`). _(2026-11-21)_
- [x] Подсистема PE — **Console (Subsystem=3)**; проверка зашита в
      `max-brh-cicd/scripts/build-windows-console.ps1` (консольная сборка клиента не
      должна быть GUI, иначе `--help`/`--demo` теряют stdout в терминале/CI).
      _(2026-11-21)_
- [x] Импорты (`dumpbin /dependents`, `build/msvc-console/imports.txt`):
      `KERNEL32`, `ws2_32`, `WINMM`, `ntdll`, `bcryptprimitives`,
      `api-ms-win-core-synch-l1-2-0`, `api-ms-win-crt-*`, `VCRUNTIME140.dll` —
      mingw-рантайма (`libgcc_s_seh-1.dll`/`libwinpthread-1.dll`) и `SDL3.dll`
      нет. Единственная не-системная зависимость — `vcruntime140.dll` (Visual
      C++ Redistributable 2015-2022 x64), это указано в `README.txt` архива.
      _(2026-11-21)_
- [x] `max-brh.exe --help` и `max-brh.exe --demo max-original\SAVE1.CAM --turns 1`
      — **нативно на хосте** (не wine): справка печатается (`ИСПОЛЬЗОВАНИЕ`,
      rc=0), demo даёт `ticks=1 … units=312 state_digest=0x733D
      state_checksum=0x408A`, rc=0; лог —
      `build/smoke-logs/windows-msvc-console-smoke.log`. _(2026-11-21)_
- [x] `bin/release/max_app-1.1.0-windows-x86_64/` содержит `max-brh.exe`,
      `README.txt`, `LICENSE`, `CHANGELOG.md`, `SHA256SUMS`; рядом
      `.zip` + `.zip.sha256`; хеши в `SHA256SUMS` совпадают с файлами.
      _(2026-11-21; sha256 exe зависит от PE-таймстампа линковки, а zip — от
      mtime файлов: при пересборке они меняются; сверяются `SHA256SUMS` и
      `.zip.sha256` в самом архиве)_
- [ ] `--connect HOST:8082 --join GAME_ID` с этим exe против работающего Horizon
      (headless-цикл gRPC+ENet на Windows-бинаре).
- [ ] `--new-game` этим exe (кадр в PPM) — визуальная проверка на Windows-хосте.

> **История (до 2026-11-21).** Windows headless собирался кросс-компиляцией
> `x86_64-pc-windows-gnu` (mingw-w64) в контейнере `rust:1.94.0-bookworm`,
> проверки шли под wine 8.0 (у него нет `bcryptprimitives.dll`, поэтому
> подкладывался тестовый шим `ProcessPrng`). Свидетельства тех прогонов —
> в `CHANGELOG.md` и `max-brh-docs/analysis/visual-1to1-status.md`; скрипт
> `max-brh-cicd/scripts/build-windows.sh` удалён, артефакты пересобраны нативно.

### 6.1. Игровая сборка: окно SDL3 и звук (Windows-хост)

Скрипт: `max-brh-cicd/scripts/build-windows-ui.ps1` (PowerShell 5.1+, без прав
администратора). Он сам находит VS через `vswhere`, vcpkg с `sdl3:x64-windows`,
поднимает окружение `vcvars64` и собирает

```sh
cargo build --release --locked -p max_app --no-default-features --features sdl-vcpkg,sound
```

```powershell
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-ui.ps1
# только собрать, без упаковки:
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-ui.ps1 -NoPackage
```

Артефакты: `bin/release/max_app-1.0.0-windows-x86_64-ui/` (`max-brh.exe`,
`SDL3.dll`, `README.txt`, `LICENSE`, `CHANGELOG.md`, `SHA256SUMS`) и
`bin/release/max_app-1.0.0-windows-x86_64-ui.zip` + `.zip.sha256`. Сборка идёт в
`bin/target/` (`CARGO_TARGET_DIR`), чтобы не смешиваться с Linux/Docker-артефактами
`max-brh/target/`; консольная сборка клиента §6 использует `bin/target-console/`.

> Примечание: с появлением default `sdl-from-source` скрипт и ручная команда
> собирают с `--no-default-features` — иначе к `sdl-vcpkg` добавилась бы
> CMake-сборка SDL3. Записи ниже — исторические (тогда SDL не входил в
> default), результат артефактов от этого не меняется: в архив идёт
> `SDL3.dll` из vcpkg.

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

Две особенности, из-за которых команда `cargo build` в cmd не работает
(учтены в скрипте):

- `set VAR=value && …` в cmd записывает в значение **пробел** перед `&&`
  (`CARGO_TARGET_DIR=…\bin\target ` → «The system cannot find the path»);
  нужно `set "VAR=value"`.
- `link.exe` не находит `SDL3.lib`: `sdl3-sys` с vcpkg линкует `SDL3.lib`, но не
  добавляет путь поиска, поэтому в `LIB` дописывается
  `<vcpkg>\installed\x64-windows\lib`. Плюс `SDL3.dll` должен лежать рядом с
  exe (скрипт копирует).

- [x] `cargo build --release -p max_app --features sdl-vcpkg,sound` — успешно
      (MSVC; `max-brh.exe` 5 423 616 байт,
      sha256 `279e472dcc41e1747548a7e38d9c1b98738e861db913b9f1d1228ac858ff23c9` —
      сборка с меню E на `MAINPIC`/`PLANETSE`/`MULTGAME`, масштабом F
      (`FrameScale`, ресайз, `--scale`), панелями 1:1, GNW-шрифтами,
      маркерами C/D и курсорами). _(2026-11-15)_
- [x] `SDL3.dll` лежит рядом с `max-brh.exe`; `max-brh.exe --help` работает
      (rc=0, справка читается через `cmd` с `>` в файл — у UI-сборки нет
      консоли), в справке есть `--scale N`. _(2026-11-15)_
- [x] Подсистема PE — **Windows GUI (Subsystem=2)**: `Start-Process max-brh.exe`
      открывает окно SDL3 без консольного окна; проверка зашита в
      `build-windows-ui.ps1` (`max-brh-cicd/scripts/pe-subsystem.ps1 -Expected 2`).
      `--help`/`--demo` этой сборки видны только при перенаправлении —
      для терминала/CI есть Console-сборка
      `max_app-1.0.0-windows-x86_64.zip`. _(2026-11-15)_
- [x] `--ui` открывает окно: при `--scale 1` клиент 640×480, при `--scale 2` —
      1280×960 (кадр целочисленно увеличен, каждый пиксель 2×2); клик
      `New Game` в окне 2× открывает форму (обратный пересчёт координат).
      Живые кадры: `build/frame-ef-main-windows.png`,
      `build/frame-ef-create-windows.png`,
      `build/frame-ef-main-windows-2x.png`,
      `build/frame-ef-create-windows-2x.png`; скрипт `build/capture-ef.ps1`.
      _(2026-11-15)_
- [x] `--ui` открывает окно: процесс остаётся активным, `MainWindowTitle` =
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
- [x] **Панели 1:1 и шрифты GNW (пункты A/B `max-brh-docs/visual-1to1-status.md`)**:
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
      `bin/release/max_app-1.0.0-windows-x86_64-ui.zip` (3 065 885 Б, sha256
      `d7415687a5d70381d30bf7ccd1bb2f67dc8afcabea993775fe22aca22b91f0d0`,
      `.zip.sha256`) — отдельно от headless-zip из §6. _(2026-11-15)_
- [x] **M-HS: пересборка Windows UI с hot-seat** (окно `DialogMenu` на
      `HELPFRAM`, `Hot Seat Menu`, `Multiplayer Menu`):
      `cargo build --release -p max_app --features sdl-vcpkg,sound` —
      `max-brh.exe` 5 572 096 Б, sha256
      `7af62ac1175886ee775b77e0417736b51acaa36f3ca60ae8703e7467ef11bc42`;
      zip `bin/release/max_app-1.0.0-windows-x86_64-ui.zip` sha256
      `9ce01c0a03eeb49557055882baca8e27b49129bb1c4c92844dd2ac362a6a2c0e`
      (меняется от mtime упаковки; exe-хеш стабилен);
      `max-brh.exe --help` — rc=0; в бинаре есть строки `Hot Seat Game`,
      `Hot Seat Menu`, `Begin turn.`, `HELPFRAM`, `HELPOK_U` (проверка
      `build/exe_hs_probe.py`). Лог `build/win-ui-hotseat-final.log`.
      _(2026-11-15)_
- [x] **M-NET: пересборка Windows UI с ретраем первого подключения и
      fallback `data_hash`**:
      `cargo build --release -p max_app --features sdl-vcpkg,sound`
      (`max-brh-cicd\scripts\build-windows-ui.ps1`) — `max-brh.exe` 5 540 864 Б,
      sha256 `136ff7743019702d94c3c7ad364138d1f97e640b3c1342d5e3ee9e6855e8c3c3`;
      zip `bin/release/max_app-1.0.0-windows-x86_64-ui.zip` sha256
      `40d644ff4d4ab00a972a03845de4ebe570467a7efa30a5f7419dea974b6b71ca`
      (меняется от mtime упаковки); `max-brh.exe --help` — rc=0, в справке
      `--join-retry-attempts`/`--join-retry-timeout` и необязательный
      `--handshake-patches`. Лог `build/win-ui-mnet-final.log`. _(2026-11-15)_
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
- [x] **M-NET-доводки: оба обходных пути удалены из клиента** _(2026-11-15)_:
      (1) `Create game` больше не завершается ошибкой `connect timed out after 10s`:
      первый `SessionHandle::start` повторяется с backoff 1,2,3,5,8,11 с
      (окно 30 с, статус `Session warming up — attempt N/M`), `CreateForm`
      сразу ведёт в `InSession`; (2) без `--handshake-patches` клиент сам
      пробует полный `data_hash`, после `TP 03 GameData` один раз
      переподключается с хэшем только `MAX.RES` (тикет тот же), при повторном
      отказе — ошибка с подсказкой. Тесты: 7 модульных в `net.rs`
      (тайм-контролируемый ретрай, fallback, отказ другого `TP 03`) + UI
      `create_form_creates_a_game_and_enters_the_session_with_mock_client`;
      `max_app/README.md` обновлён.
- [x] Партия в окне без обходных флагов: `Create game` → сессия сразу
      (ретрай/прогрев в клиенте) и дефолтный `data_hash`, совпадающий с
      сервером. Свидетельство: UI-переход Create→InSession (тест), реальный smoke
      2×10 без `--handshake-patches` (§3), прежний прогон окна 2 клиентов
      (6 ходов). _(2026-11-15)_
- [ ] Полный сценарий §4 (меню → create → бой/чат/End Turn до победы) на работающем
      сервере, без ручных флагов совместимости.

## 7. Документация и лицензии

- [ ] `README.md` (корень): раздел про Rust-клиент ссылается на
      `max-brh-docs/release-checklist.md`, `release.sh` и звук.
- [ ] `max-brh/README.md`: версия 1.0.0, feature `sound`, ссылка на чек-лист.
- [ ] `CHANGELOG.md`: 1.0.0, список возможностей и ограничений, ссылка на
      `LICENSE`.
- [ ] `max-brh-docs/release-checklist.md` (этот файл) актуален: команды и пути
      совпадают с `release.sh`/`ci.sh`/`max-brh-cicd/scripts/build-windows-console.ps1`.
- [ ] `max-brh-cicd/scripts/README.md` описывает релизные скрипты
      (`build-windows-console.ps1` — нативная Windows-сборка на MSVC,
      `build-windows-ui.ps1`, `live-smoke.sh`) и преодолённые блокеры
      (ENet-платформа в `build.rs`, TLS-фича `tonic`).
- [ ] `LICENSE` (MIT) присутствует в корне и в архиве релиза; в `README.txt`
      архива — копирайт Interplay на данные игры.

## 8. Git tag v1.0.0

Тег создаётся только после зелёных §2–§5 (и §6, если Windows-сборки входят в
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

- [ ] `bin/release/max_app-1.0.0-linux-x86_64.tar.gz` + `SHA256SUMS` сохранены.
- [ ] `bin/release/max_app-1.0.0-windows-x86_64.zip` + `.zip.sha256` сохранены
      (продукт `max-brh-cicd/scripts/build-windows-console.ps1`); headless-каталог
      `bin/release/max_app-1.0.0-windows-x86_64/` тоже несёт свой `SHA256SUMS`.
- [ ] `bin/release/max_app-1.0.0-windows-x86_64-ui.zip` + `.zip.sha256` сохранены
      (продукт `max-brh-cicd/scripts/build-windows-ui.ps1`; в архиве `max-brh.exe` +
      `SDL3.dll`).
- [ ] Логи гейтов: `max-brh/target/cargo-test.log` (если записывался),
      `build/smoke-logs/*.log`, `build/smoke-logs/windows-msvc-console-smoke.log`,
      скриншоты M4 — приложены к релизной записи.
- [ ] Артефакты **не** закоммичены в git (`release/` в `.gitignore`).

## 10. После релиза

- [ ] Актуализировать единый план `../PLAN.md` §3–§4 (вехи и backlog) после релиза.
- [ ] Открыть issue на ограничения из `CHANGELOG.md` §«Ограничения 1.0.0».
- [ ] Проверить, что `main` после тега собирается `bash ci.sh`.
- [ ] Пост-релизная правка, попавшая в Windows UI-сборку и **ещё не** в
      Linux-артефакт: `max_render/src/text.rs` — бит 0 = левый пиксель
      (текст был зеркальным во всём UI). Нужно: запись в `CHANGELOG.md` для
      1.0.1 и пересборка `bin/release/max_app-1.0.0-linux-x86_64.tar.gz`
      (`bash release.sh`) — в текущем Linux-архиве текст ещё зеркальный.
      Headless-сборки (Windows) текст не рисуют, их пересборка не нужна.

---

## Приложение A. Быстрый прогон (минимальный релиз-гейт)

```sh
bash ci.sh                                  # 2.1 + 2.4
bash max-brh-cicd/scripts/live-smoke.sh 10         # §3
powershell -ExecutionPolicy Bypass -File max-brh-cicd\scripts\build-windows-console.ps1  # §6, Windows консольная сборка клиента: прогон сборки + --help/--demo + zip
bash release.sh --dry-run                   # §5, проверка плана
bash release.sh                             # §5, артефакты
```

## Приложение B. Свидетельства

| Гейт | Дата | Команда/стенд | Результат | Ссылка |
|---|---|---|---|---|
| Тесты max-brh | 2026-11-15 | `docker run --rm -v "$ROOT:/src" -w /src/max-brh rust:1.94.0-bookworm bash -c "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test"`; плюс `cargo check -p max_app --features sdl-from-source` (полный X11/ALSA dev-набор) | **622 теста, 0 failed**; `cargo check` — 0 ошибок и 0 предупреждений; сборка с SDL — 0 ошибок | `build/cargo-docker-mnet-final.log`, `build/cargo-docker-ef-sdl-final.log` |
| M-HS (hot-seat) | 2026-11-15 | та же команда Docker; `cargo test -p max_app --test hot_seat`; `cargo run -p max_app --example sp_frames -- ../max-original ../build`; `max-brh-cicd\scripts\build-windows-ui.ps1` | 13 тестов hot-seat (2 и 4 человека, ИИ между людьми, окно `DialogMenu` 1:1, таймер до подтверждения, только свои юниты); 5 кадров; Windows UI exe 5 572 096 Б sha256 `7af62ac1…` | `build/hs-{players,turn,turn2,game,ai}.png`, `build/cargo-docker-hotseat-final.log`, `bin/release/max_app-1.0.0-windows-x86_64-ui.zip` |
| M-NET-доводки (ретрай + data_hash fallback) | 2026-11-15 | `docker run … rust:1.94.0-bookworm … cargo check && cargo test`; `MSYS_NO_PATHCONV=1 bash max-brh-cicd/scripts/live-smoke.sh 10`; `max-brh-cicd\scripts\build-windows-ui.ps1` | **622 теста, 0 failed** (+7: тайм-контролируемый ретрай, fallback MAX-only, отказ `Version` без fallback, варианты хэша, статус, backoff; UI Create→InSession); smoke 2×10 без `--handshake-patches`: `data_hash=dd24703e(64)`, `barrier_timeouts=0`, fallback подтверждён; Windows UI exe 5 540 864 Б sha256 `136ff774…`, zip `40d644ff…` | `build/cargo-docker-mnet-final.log`, `build/live-smoke-mnet-final.log`, `build/smoke-logs/a.log`/`b.log`, `build/win-ui-mnet-final.log` |
| Тесты Horizon | | | | |
| Live smoke 2×10 | 2026-11-15 | `MSYS_NO_PATHCONV=1 bash max-brh-cicd/scripts/live-smoke.sh 10` без `--handshake-patches`, два прогона | `GAME_ID=87b1c4dc-…` (повтор `83621ef3-…`); A/B: `turns=10`, `barrier_timeouts=0`, `data_hash=dd24703e(64)`, fallback подтверждён | `build/live-smoke-mnet-final.log`, `build/live-smoke-mnet.log`, `build/smoke-logs/a.log`/`b.log` |
| M4 вручную | | | | |
| Linux-сборка | | | | |
| Windows консольная сборка клиента (MSVC) | 2026-11-21 | `powershell -File max-brh-cicd\scripts\build-windows-console.ps1` (MSVC 14.x + Windows SDK, Rust 1.94.0 `x86_64-pc-windows-msvc`, без Docker/mingw/wine) | PE32+ x64, 4 433 408 Б, sha256 `2a36cd6e…`; `--help` и `--demo SAVE1.CAM --turns 1` нативно — rc=0, `ticks=1 … state_digest=0x733D`; zip 1 807 530 Б + `.zip.sha256` | `bin/release/max_app-1.1.0-windows-x86_64.zip`, `build/smoke-logs/windows-msvc-console-smoke.log` |
| Windows консольная сборка клиента (mingw-кросс — история) | 2026-11-15 | `bash max-brh-cicd/scripts/build-windows.sh` (Docker, `rust:1.94.0-bookworm`, mingw-w64 + wine 8.0; скрипт удалён 2026-11-21) | PE32+ x86-64, 8 908 438 Б, sha256 `15cd9fa2…`; `--help`/`--demo` под wine 8.0 — exit 0; zip 3 255 566 Б + `.zip.sha256` | `bin/release/max_app-1.0.0-windows-x86_64.zip`, `build/smoke-logs/windows-wine-smoke.log` |
| Windows игровая сборка (SDL3 + звук, нативная MSVC) | 2026-11-15 | `powershell -File max-brh-cicd\scripts\build-windows-ui.ps1` (MSVC 14.51 + Windows SDK 10.0.26100, SDL3 3.4.20 из vcpkg) | `max-brh.exe` 5 423 616 Б, sha256 `279e472d…`; окно открывается, `--scale 1/2` = 640×480/1280×960, клик 2× работает, звук `backend=rodio/cpal`; zip 3 065 885 Б, sha256 `d7415687…` + `.zip.sha256`; `--new-game --width 640 --height 480` отработал | `bin/release/max_app-1.0.0-windows-x86_64-ui.zip`, `build/frame-ef-create-windows-2x.png` |
| Визуал: меню E и масштаб F | 2026-11-15 | `cargo run -p max_app --example visual_ef -- ../build/frame-ef`; `max-brh.exe --ui --scale 1/2` + `build/capture-ef.ps1`; `cargo test -p max_app -p max_render -p max_assets` | главное меню `MAINPIC` + `BLANK_*`, форма `PLANETSE` + `MNUBTN4/5/6` + сетка 8×3, список `MULTGAME` + `BTNHIDE`; `FrameScale` 2× — каждый пиксель блоком 2×2, клик в окне 1280×960 попадает в ту же клетку карты (56,55); декодер `SNOW_PIC` исправлен (планета 0..5 больше не чёрная); +17 тестов (576 всего) | `build/frame-ef-main.png`, `build/frame-ef-create.png`, `build/frame-ef-join.png`, `build/frame-ef-main-2x.png`, `build/frame-ef-*-windows*.png` |
| Визуал: окно 1:1 (FRAMEPIC) | 2026-11-15 | `max-brh.exe --new-game --width 640 --height 480 --out frame.ppm`; реальная сессия `--ui` против Horizon+headless | FRAMEPIC (640×480, своя палитра, RLE с чанками через строку) + раскладка C++-порта: карта (180,18) 448×448, миникарта (15,356), кнопки End Turn/Files/Prefs; в сессии процедурный HUD не рисуется | `build/frame-chrome2.png`, `build/ui-chrome-session-final.png`, `build/ui-framepic.png` |
| Визуал: панели и шрифт GNW (A/B) | 2026-11-15 | `max-brh.exe --new-game --width 640 --height 480 --out build/frame-windows-artifact.ppm`; `cargo run -p max_app --example panel_frame -- ../build/frame-panels.ppm`; `tools/font_vectors/run.sh` | верхние кнопки/счётчики, портрет (спрайт ×2), 4 строки статистики, 9 переключателей, зум, координаты/описание, миникарта из `.WRL`; `FONT1/2/5.FON` совпали с FreeType-путём C++-порта побайтово для ASCII; Windows- и Linux-кадры идентичны | `build/frame-newgame-panels.png`, `build/frame-panels.png`, `build/frame-panels-left.png`, `build/frame-windows-artifact.png`, `max-brh/tools/font_vectors/render.txt` |
| Визуал: тайлы и здания | 2026-11-15 | `cargo run -p max_render --example terrain -- ../max-original/SNOW_1.WRL out`; `max-brh.exe --new-game --out frame.ppm` | 337 тайлов/12 544 клетки SNOW_1 рендерятся палитрой `.WRL` (снимки `build/wrl-snow-*.png`), спрайтов грузится 43 (здания и alien-обломки видны вместо маркеров) | `build/wrl-snow-map.png`, `build/frame-original-terrain.png` |
| Визуал: миникарта/иконки/курсоры/планеты (C/D) | 2026-11-15 | `cargo run -p max_app --example visual_cd -- ../build/frame-cd`; `cargo test -p max_assets -p max_app --lib` | 30/30 курсоров декодированы (размеры/hotspot/кадры сверены с C++), 99 маркеров ресурсов, миникарта с FOV-маской/рамкой камеры/точками юнитов/2x/TNT, сетка 24 планет `SNOW_PIC + world/6` с числовым fallback; +20 тестов (576 всего) | `build/frame-cd-game.png`, `build/frame-cd-game-2x.png`, `build/frame-cd-game-minimap-4x.png`, `build/frame-cd-planets.png`, `build/frame-cd-cursors.png` |
| Сетевая партия 2×UI | 2026-11-15 | 2× `max-brh.exe --ui --turn-wait 45` (MSVC-сборка) + Horizon + C++ headless (`docker compose up -d --build`) | 6 ходов в lockstep, приказы `38` и чат `18` доходят до сервера и пира, `End Turn` → `06+52`, таймаутов/десинков 0 | `build/ui-alice.log`, `build/ui-bob.log`, `build/final-a.png` |
| Windows игровая сборка (SDL3 + звук) | | | | |
| Тег v1.0.0 | | | | |

## Приложение C. M-SP (локальная партия против ИИ)

| Гейт | Команда | Результат | Ссылка |
|---|---|---|---|
| Настройка 1:1 | `cargo run -p max_app --example sp_frames` | `OPTNFRM`/`PLANETSE`/`CHOSPLYR` кадры | `build/sp-*.png` |
| Локальный цикл | `cargo test -p max_app --test local_sp` | 12 тестов | — |
| Windows UI | `max-brh-cicd\scripts\build-windows-ui.ps1` | zip + sha256 | `bin/release/max_app-1.0.0-windows-x86_64-ui.zip` |
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
| Windows UI | `max-brh-cicd\scripts\build-windows-ui.ps1` | zip + sha256 | `bin/release/max_app-1.0.0-windows-x86_64-ui.zip` |
| Вход в режим | `Screen::MultiplayerMenu` (`menu.cpp:2361`) | «Multiplayer Game» → `Hot Seat Game` → Options → Planet → Players | `build/hs-players.png` |

Ограничения M-HS: `Hot Seat Scenarios` и `Load Hot Seat Game` не портированы
(кнопки пишут статус); кланы выбираются экраном `ClanSelectMenu` (8 карточек,
бонусы применяются к стартовым юнитам), но описания/таблица бонусов из
`clans.json` ещё не загружаются; запись V70 отсутствует (общий пункт M-SP),
контейнер `MXLS` — версия 3 (флаг `hot_seat` + `team_clans`).

