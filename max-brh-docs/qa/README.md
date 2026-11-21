# QA

Центр обеспечения качества — каталог **`max-brh-qa/`** (все материалы QA расположены там):

## Правила процесса QA

1. **Цели не закрываются без подтверждения человека.** Статус цели
   («выполнено») в документации устанавливается только после того, как человек
   явно подтвердил, что найденные ошибки исправлены и цель действительно можно
   закрыть. Автоматические прогоны (тесты, QA-сценарии, golden, smoke) являются
   необходимым, но **не достаточным** условием: решающее слово — за ручным
   тестированием и явным согласием человека.
2. **Ошибки из ручного тестирования фиксируются отдельно** и остаются открытыми
   до подтверждения исправления.

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

Чек-лист релиза: `max-brh-docs/development/release-checklist.md`.
