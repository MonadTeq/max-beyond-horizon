# Разработка

Планы, сборка, инструменты, чек-лист релиза.

> **Единый план проекта — [`../PLAN.md`](../PLAN.md)** (цели, статус, открытые
> работы по агентам). Планы ниже — детализация/история; новые задачи фиксируются
> только в мастер-плане.

| Документ | Содержимое |
|---|---|
| [../PLAN.md](../PLAN.md) | **Единый мастер-план** (цели, статус, backlog по агентам) |
| [build.md](build.md) | Сборка, запуск, необходимые инструменты и библиотеки |
| [PLAN.md](PLAN.md) | Исторический генеральный план (v6) — консолидирован в `../PLAN.md` |
| [PLAN-v7-release.md](PLAN-v7-release.md) | Дорожная карта до релиза (историческая) |
| [PLAN-host-mode.md](PLAN-host-mode.md) | Сетевой режим хоста (P2P) — цель 2 (актуальный детальный план) |
| [PLAN-refactor.md](PLAN-refactor.md) | Рефакторинг BRH (Ф0–Ф5.3) — выполнен, история |
| [agent-programmer.md](agent-programmer.md) | Руководство агента-программиста (правила и порядок работы) |
| [rust-programming-rules.md](rust-programming-rules.md) | Канонические правила программирования на Rust |
| [analyst-rules.md](analyst-rules.md) | Правила аналитика |
| [architect-rules.md](architect-rules.md) | Правила архитектора |
| [release-checklist.md](release-checklist.md) | Чек-лист релиза со свидетельствами |

## Быстрый старт

```sh
# Все тесты + QA + сборка (Docker):
bash ci.sh

# Собрать Windows игровой бинарь (хост: MSVC + vcpkg SDL3):
powershell -File max-brh-cicd/scripts/build-windows-ui.ps1

# Собрать Linux-бинарь:
bash release.sh
```

Подробности — в [build.md](build.md).
