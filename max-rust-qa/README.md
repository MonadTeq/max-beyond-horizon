# max-rust-qa — центр обеспечения качества клиента M.A.X.

Всё QA в одном каталоге: планы, тест-кейсы, скрипты, golden-кадры, драйверы, утилиты.

## Структура

```
max-rust-qa/
  README.md               этот файл
  plans/                  планы QA и стратегия (qa-plan.md и пр.)
  suite/                  каталог тест-кейсов (по одному файлу на область, TC-IDs)
  scripts/                .qa-сценарии драйвера (sp-full.qa, hs-2.qa, …)
  golden/                 эталонные кадры экранов (для сверки на дрейф)
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

## Соответствие «Test Case → автоматизация»

Каждый TC в `suite/` имеет поле `Автоматизация`: ссылку на `.qa`-скрипт, юнит/интеграционный
тест (`cargo test`), живой smoke (`build/scripts/live-smoke.sh`), оконный прогон
(`drivers/ui-test-*.ps1`) или `manual` с шагами для человека.

## Быстрый старт

```sh
# все qa-скрипты + golden (в docker):
bash ci.sh

# один скрипт:
cargo run -p max_app --example qa_run -- max-rust-qa/scripts/sp-full.qa max-rust-qa/out/sp

# golden-сверка / перегенерация:
cargo run -p max_app --example qa_golden_check -- max-rust-qa/golden max-rust-qa/out/visual
cargo run -p max_app --example qa_golden_check -- --update max-rust-qa/golden max-rust-qa/out/visual
```
