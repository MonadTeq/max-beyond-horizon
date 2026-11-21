# max-brh-qa — центр обеспечения качества клиента M.A.X.

Всё QA в одном каталоге: планы, тест-кейсы, скрипты, golden-кадры, драйверы, утилиты.

## Структура

```
max-brh-qa/
  README.md               этот файл
  plans/                  планы QA и стратегия (qa-plan.md и пр.)
  suite/                  каталог тест-кейсов (по одному файлу на область, TC-IDs)
  scripts/                .qa-сценарии драйвера (sp-full.qa, hs-2.qa, upgrade.qa, research.qa, …)
  golden/                 эталонные кадры экранов (для сверки на дрейф; production/, upgrade/, research/, cargo/, research-help/)
  drivers/                тестовые драйверы и инструкции (qa_run, qa_golden_check, ui-тесты)
  tools/                  утилиты (генераторы векторов, компараторы)
  out/                    рабочие результаты прогонов (gitignored)
```

## Как это работает

1. **Драйвер `qa_run`** (пример в `max-rust/crates/max_app/examples/qa_run.rs`) исполняет
   `.qa`-скрипты через реальные обработчики ввода клиента (без SDL): команды
   `click/rclick/key/type/wait/tick/shot/assert …`, высокоуровневые `menu/setup/players/
   mobilize/landing/select/move/end_turn`. Формат — в `plans/qa-plan.md` и
   `drivers/README.md`.
2. **Скрипты** лежат в `scripts/`; каждый скрипт покрывает один или несколько TC из `suite/`.
3. **Golden** в `golden/` — эталоны кадров; дрейф ловится `qa_golden_check` (порог 1 %).
4. **CI**: `ci.sh` гоняет все скрипты + golden; падение = красный билд.
5. **P6 (v1.1.0)**: `scripts/upgrade.qa` — экран `UpgradeMenu` (`U`),
   `scripts/research.qa` — питание Research Center (`O`) и уровень темы;
   golden — `golden/upgrade/`, `golden/research/`. **P7**: `scripts/cargo-full.qa`
   — полный `CargoMenu` (15 типов покупки, cargo-слайдер, режим модернизации,
   апгрейд до высадки), golden — `golden/cargo/`; C++-векторы —
   `max-rust/tools/cargo_vectors`. **P8**: `scripts/research-help.qa` —
   `ResearchMenu` (`R`, распределение центров Attack→Shots и `Done`) и окна
   `HELPFRAM` (`OPTIONS_SETUP`/`PLANET_SETUP`/`NEW_GAME_SETUP`/`CARGO_SETUP`/
   `RESEARCH_SETUP`/`GAME_SCREEN_SETUP`), golden — `golden/research-help/`.
   **P9**: `scripts/unit-menu.qa` — меню команд юнита (`GameManager_PopupButtons`),
   `scripts/allocate.qa` — распределение добычи шахты (`AllocMenu`, `ALLOCFRM`,
   `Done` → `ORDER_NEW_ALLOCATE` 08/P5 → доход комплекса),
   `scripts/xfer.qa` — X-FER (`TransferMenu` + `ProcessOrderTransfer`: перенос
   материалов/топлива/золота, `Done` → `ORDER_TRANSFER` 08/P3), golden —
   `golden/unit-menu/`, `golden/allocate/`, `golden/xfer/`.
   Итоговая матрица — `suite/README.md` (**193 ✅, 1 🟡, 16 ⏭ not planned**).

## Соответствие «Test Case → автоматизация»

Каждый TC в `suite/` имеет поле `Автоматизация`: ссылку на `.qa`-скрипт, юнит/интеграционный
тест (`cargo test`), живой smoke (`build/scripts/live-smoke.sh`), оконный прогон
(`drivers/ui-test-*.ps1`) или `manual` с шагами для человека.

## Быстрый старт

```sh
# все qa-скрипты + golden (в docker):
bash ci.sh

cd max-rust   # примеры собираются в workspace max-rust

# один скрипт (сценарии — в ../max-brh-qa/scripts/, вывод — в qa-out/):
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-full.qa qa-out/sp

# golden-сверка / перегенерация (<shots_dir> <golden_dir>):
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual ../max-brh-qa/golden
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual ../max-brh-qa/golden --update
```
