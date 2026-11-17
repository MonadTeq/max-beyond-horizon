# QA

Центр обеспечения качества — каталог **`max-rust-qa/`** (всё QA живёт там):

| Раздел | Где |
|---|---|
| Стратегия QA и формат .qa-скриптов | `max-rust-qa/plans/qa-plan.md` |
| Тест-кейсы (192 TC по 11 областям) | `max-rust-qa/suite/` |
| .qa-сценарии | `max-rust-qa/scripts/` |
| Golden-кадры (дрейф-гейт) | `max-rust-qa/golden/` |
| Драйверы (qa_run, qa_golden_check, запись вручную) | `max-rust-qa/drivers/README.md` |

Быстрый старт:

```sh
bash ci.sh                              # все тесты + QA-сценарии + golden

cd max-rust                             # примеры живут в workspace max-rust
# (сценарий, каталог вывода); эталоны — ../max-rust-qa/golden
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/sp-full.qa qa-out/sp
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual ../max-rust-qa/golden
```

`--no-default-features` — у `max_app` default-фичи включают GUI (`sdl-from-source`,
SDL3 через CMake); QA-драйвер работает без SDL, поэтому собирается без них.

Запись сценария вручную: `max-rust.exe --record my.qa` — GUI открывается без
флагов, действия пишутся в скрипт (см. `max-rust-qa/drivers/README.md`).

Чек-лист релиза: `docs/development/release-checklist.md`.
