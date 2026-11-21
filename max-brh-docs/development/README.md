# Разработка

Планы, сборка, инструменты, чек-лист релиза.

| Документ | Содержимое |
|---|---|
| [build.md](build.md) | Сборка, запуск, необходимые инструменты и библиотеки |
| [PLAN.md](PLAN.md) | Генеральный план проекта (v5) |
| [PLAN-v7-release.md](PLAN-v7-release.md) | Дорожная карта до рабочего релиза |
| [release-checklist.md](release-checklist.md) | Чек-лист релиза со свидетельствами |

## Быстрый старт

```sh
# Все тесты + QA + сборка (Docker):
bash ci.sh

# Собрать Windows UI-бинарь (хост: MSVC + vcpkg SDL3):
powershell -File max-brh-cicd/scripts/build-windows-ui.ps1

# Собрать Linux-бинарь:
bash release.sh
```

Подробности — в [build.md](build.md).
