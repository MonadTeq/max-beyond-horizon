# QA

Центр обеспечения качества — каталог **`max-brh-qa/`** (всё QA живёт там):

| Раздел | Где |
|---|---|
| Стратегия QA и формат .qa-скриптов | `max-brh-qa/plans/qa-plan.md` |
| Тест-кейсы (205 TC по 11 областям) | `max-brh-qa/suite/` |
| .qa-сценарии | `max-brh-qa/scripts/` |
| Golden-кадры (дрейф-гейт) | `max-brh-qa/golden/` |
| Драйверы (qa_run, qa_golden_check, запись вручную) | `max-brh-qa/drivers/README.md` |

Быстрый старт:

```sh
bash ci.sh                              # все тесты + QA-сценарии + golden

cd max-brh                             # примеры живут в workspace max-brh
# (сценарий, каталог вывода); эталоны — ../max-brh-qa/golden
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-full.qa qa-out/sp
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual ../max-brh-qa/golden
```

`--no-default-features` — у `max_app` default-фичи включают GUI (`sdl-from-source`,
SDL3 через CMake); QA-драйвер работает без SDL, поэтому собирается без них.

Запись сценария вручную: `max-brh.exe --record my.qa` — GUI открывается без
флагов, действия пишутся в скрипт (см. `max-brh-qa/drivers/README.md`).

Чек-лист релиза: `docs/development/release-checklist.md`.
