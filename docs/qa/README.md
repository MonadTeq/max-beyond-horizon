# QA

Центр обеспечения качества — каталог **`max-rust-qa/`** (всё QA живёт там):

| Раздел | Где |
|---|---|
| Стратегия QA и формат .qa-скриптов | `max-rust-qa/plans/qa-plan.md` |
| Тест-кейсы (149 TC по 11 областям) | `max-rust-qa/suite/` |
| .qa-сценарии | `max-rust-qa/scripts/` |
| Golden-кадры (дрейф-гейт) | `max-rust-qa/golden/` |
| Драйверы (qa_run, qa_golden_check, запись вручную) | `max-rust-qa/drivers/README.md` |

Быстрый старт:

```sh
bash ci.sh                              # все тесты + QA-сценарии + golden
cargo run -p max_app --example qa_run -- max-rust-qa/scripts/sp-full.qa out
cargo run -p max_app --example qa_golden_check -- max-rust-qa/golden out
```

Запись сценария вручную: `max-rust.exe --record my.qa` — GUI открывается без
флагов, действия пишутся в скрипт (см. `max-rust-qa/drivers/README.md`).

Чек-лист релиза: `docs/development/release-checklist.md`.
