# QA-стратегия клиента M.A.X. (playbook)

Цель: сборка проверяется **играбельной автоматически** до каждого релиза.
Проблема: ручной UI-тест мышью (Windows) медленный и не воспроизводимый; сборка
«далека от играбельной» — нужны регресс-барьеры на каждом уровне.

**Статус:** слои 1–4 и запись сценариев (2.5) реализованы, слой 5 —
ручной/полуавтоматический (Windows). Свидетельства и команды:
`max-rust-qa/scripts/README.md`, `build/qa-*.log`.

## Слои тестирования (пирамида)

| Слой | Что проверяет | Где | Частота | Статус |
|---|---|---|---|---|
| 1. Юнит/интеграционные (780+ тестов) | логика core/net/render/assets + парсер/драйвер QA | cargo test | каждый коммит | **готово** |
| 2. **Скриптовый UI-драйвер (in-process)** | полный путь игрока: меню → настройка → партия → приказы → ходы → победа, ассерты состояния | `qa_run` без SDL | каждый коммит | **готово** (`max_app/src/qa/`, 10 сценариев) |
| 2.5. **Запись ручной игры (recorder)** | окно → `.qa`-сценарий для слоя 2 (`click/rclick/key/type/wait`) | `max-rust --record` | при ручном прогоне | **готово** (`max_app/src/qa/recorder.rs`) |
| 3. **Golden-кадры экранов** | визуальный 1:1 (дрейф пикселей) | qa-скрипт + diff | каждый коммит | **готово** (`qa_golden_check`, 15 эталонов: visual 10 + production + upgrade + research) |
| 4. Живой сетевой smoke | 2 клиента × 10 ходов CRC (и 3 клиента) | live-smoke.sh / live-smoke-3.sh | релизный гейт | **готово** (P5: 2×10, 3×10 PASS) |
| 5. Реальный оконный прогон мышью (Windows) | SDL/ввод/звук на живом бинаре | ui-test-sp.ps1 и пр. | релизный гейт, вручную/полуавто | по плану |

Ключевая идея слоя 2: экраны и игровой цикл в max_app — **чистые функции от
InputEvent** (ui.rs/setup.rs/panels.rs/local.rs не требуют SDL). Значит весь путь
игрока воспроизводим без окна: скрипт действий → те же обработчики, что у мыши →
кадры в PNG + ассерты (turn_counter, active_team, units, digest, победа).

## Скриптовый драйвер (слой 2)

- Пример: `cargo run -p max_app --no-default-features --example qa_run -- <script.qa> <out_dir>`
  (`--data-dir`, `--assets-dir`, `--frame-limit` — опции).
- Реализация: `max_app/src/qa/` — `script.rs` (парсер), `runner.rs`
  (исполнение тем же `Screen::handle_input` / `handle_session_input` /
  `App::local_update`, что в `main.rs`), `png.rs` (PNG-кодек и компаратор без
  внешних крейтов).
- Язык скрипта (построчно), координаты — пиксели кадра 640×480:
  - `click X Y` / `rclick X Y` / `key ENTER|ESC|T|...` / `type "text"` — как реальный ввод;
  - `wait N` (до N кадров, пока игра не готова к вводу), `tick N` (ровно N кадров);
  - `shot name` — PNG кадра (640×480);
  - `assert turn=N` / `team=N` / `units>=N` / `digest=HEX` / `screen=InGame` /
    `phase=PlayerTurn` / `notice=RED` / `victory` / `defeat` / `selected`;
  - `save file` (`MXLS`) / `load file` (V70 `*.CAM`) / `exit`;
  - высокоуровневые: `menu new_game|multiplayer|hot_seat|join`,
    `setup done|cancel|random`, `players <slot> human|computer|none`,
    `select X Y` / `select unit N`, `move X Y` / `move grid X Y` / `move near`,
    `end_turn`.
- Покрытие сценариями (`max-rust-qa/scripts/*.qa`, все — PASS):
  1. `sp-full.qa` — New Game → Options → Planet → Players (RED человек,
     GREEN компьютер) → мобилизация (`mobilize buy` ×2 + Done) → выбор точки
     высадки (`landing grid 30 22` + Done; точка не совпадает со стартовой
     четвертью — кадр доказывает, что выбор применён) → выделение юнита,
     приказ движения,
     3 × End Turn с ходами ИИ, `assert turn>=3`, `save` (MXLS);
  2. `hs-2.qa` — hot-seat 2 человека: фазы старта для GREEN и RED по очереди
     (мобилизация + высадка), окна «Begin turn» для RED и GREEN,
     3 раунда, `assert team` чередуется, подтверждение окна Enter;
  3. `visual-screens.qa` — проход по ВСЕМ экранам с `shot` (эталоны golden,
     включая `mobilize.png` и `landing.png`);
  4. `victory.qa` — партия до победы (лимит ходов 1) + `assert victory`;
     элиминация ИИ скриптом недостижима — pending, покрыта тестом слоя 1;
  5. `production.qa` — P2: конструктор → BuildMenu → TRAINHAL за 10 ходов →
     завод → INFANTRY за 2 хода;
  6. `save-load.qa` — P3: два хода → `save_v70` (настоящий V70) → окно
     `Save Game` (`key s`, кадр `save-menu`) → `load` в свежем состоянии →
     `assert turn=3`, `assert units=14` → `end_turn` после загрузки;
  7. `soak-100.qa` — P4: 100 ходов человек+ИИ, снимки/`mem` каждые 20 ходов,
     `assert turn=101`, V70-сейв на 100-м ходу;
  8. `edge-cases.qa` — P4 TC-1106/1111/1112: 50+50 кликов (меню/карта),
     не-ASCII ввод в чате/имени, повторный старт.
- Драйвер пишет `out/log.txt` (шаги, фактические значения, итог) и `out/*.png`;
  ассерт-фейл → exit 1 с текстом (`строка N: assert …: фактически …`).
- Тесты драйвера: `src/qa/script.rs` (все команды парсера),
  `tests/qa_driver.rs` (полный прогон `sp-full.qa`, падение ассерта → exit 1,
  `load` V70, PNG/golden round-trip).

## Запись сценария вручную (слой 2.5)

Проблема слоя 2: ручной сценарий приходится писать по координатам из кадров.
Решение — рекордер в клиенте: QA-инженер играет в окне, а `main.rs` пишет тот
же `InputEvent`, что уходит в `Screen::handle_input`/`handle_session_input`.

```sh
cd max-rust
# GUI — default (SDL3 из исходников через CMake)
cargo run -p max_app -- \
  --ui --record ../max-rust-qa/scripts/my-run.qa
```

- Старт — при запуске (MainMenu), финал — при выходе; файл пишется построчно
  и не перезаписывается (`run.qa` → `run-1.qa`).
- В файл попадают `click X Y` / `rclick X Y` (**пиксели кадра 640×480** после
  обратного пересчёта масштаба окна), `key …`, агрегированный `type "…"`
  (текстовый ввод в сессии: T/Enter → символы → Enter/Esc) и `wait N` —
  паузы в кадрах, с cap 120.
- Движение мыши, клики вне кадра и `Key::Other` не пишутся; Backspace внутри
  `type` выражается отдельной `key BACKSPACE` (см. ограничения в
  `drivers/README.md`).
- Пост-обработка не нужна: файл сразу исполняется `qa_run` с тем же стартовым
  состоянием. **Ассерты и `shot` добавляет QA-инженер вручную** — типично
  после каждого экрана (`assert screen=…`, `shot name`) и перед `End Turn`.
- Тесты: unit-тесты рекордера (строки команд, cap пауз, `type`/Backspace) и
  round-trip `tests/qa_driver.rs` — записанный поток `InputEvent` → `.qa` →
  `qa_run` → тот же `AppDigest`.

## Golden-кадры (слой 3)

- Эталонные кадры экранов — в `max-rust-qa/golden/*.png` (10 кадров, ~980 КиБ;
  индексированный PNG при ≤256 цветах), генерируются из `visual-screens.qa`.
- Сверка: `cargo run -p max_app --no-default-features --example qa_golden_check -- <out> ../max-rust-qa/golden` —
  попиксельный diff с порогом: пиксель изменён при `max|Δканал| > 30`, кадр
  «дрейфнул» при > 1 % таких пикселей; превышение → exit 1, список файлов и
  `diff-*.png` (белый фон, красные — расхождения). `--update` перезаписывает
  эталоны после осознанного визуального изменения.
- Детерминизм подтверждён: два прогона `visual-screens.qa` дают 0 отличающихся
  пикселей.
- Сверка игровых кадров с эталонами C++-порта (`build/run/dbg.png`,
  `build/smoke-window.png`) — отдельный ориентир 1:1, в CI не входит.

## Интеграция в CI (слой 4)

- `ci.sh`: стадия **3/4** после `cargo test` и тестов Horizon — прогон
  `sp-full.qa`, `hs-2.qa`, `visual-screens.qa`, `victory.qa`, `production.qa`,
  `save-load.qa`, `edge-cases.qa` и golden-сверка; стадия **4/4** — soak
  (`soak-100.qa` с `MAX_TICK_PROFILE=1` + `soak_headless --turns 120`);
  `--smoke` дополнительно запускает `live-smoke.sh`.
- `.github/workflows/ci.yml`: job `qa` с теми же командами и job `soak`
  (`timeout-minutes: 45`); при падении логи и кадры уходят артефактом
  `qa-artifacts`.
- Docker (rust:1.94.0-bookworm + gcc/cmake) — всё headless, без SDL; PNG-кодек
  свой, поэтому новые крейты и сеть для картинок не нужны.

## Playbook запуска (для QA вручную)

1. `bash ci.sh` — всё автоматическое (тесты + QA-сценарии + golden).
2. Ручной оконный прогон: `powershell -File build/scripts/ui-test-sp.ps1` (реальный
   бинарь, мышь, скриншоты) — перед релизом.
3. Живой smoke: `bash build/scripts/live-smoke.sh 10`.
4. Разбор QA-падения: `max-rust-qa/out/<сценарий>/log.txt` + `diff-*.png`;
   формат и перегенерация эталонов — `max-rust-qa/scripts/README.md`.

## Метрики готовности релиза

- ci.sh зелёный (слои 1–3) и live-smoke (слой 4);
- QA-сценарии `sp-full`, `hs-2`, `visual-screens`, `victory` — PASS, golden без
  дрейфа;
- оконный прогон (слой 5) снят и сверен;
- `docs/release-checklist.md` — все пункты с датами/свидетельствами.
