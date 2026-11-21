# Каталог тест-кейсов клиента M.A.X.

Обозначения автоматизации:
- `QA:<script>` — .qa-скрипт в `max-brh-qa/scripts/`;
- `UNIT` — юнит/интеграционные тесты (`cargo test`);
- `SMOKE` — живой сетевой smoke (`build/scripts/live-smoke.sh`);
- `WINDOW` — оконный прогон на реальном бинаре (`max-brh-qa/drivers/`);
- `MANUAL` — ручной сценарий.

Статусы: ✅ покрыто и зелёное; 🟡 покрыто частично/с оговоркой (в P6 не осталось); ⏭ not planned — осознанно не закрывается сейчас, причина указана в кейсе; ⬜ не покрыто/запланировано; ❌ известно сломанное.

В P6 (2026-11-16) все 🟡/❌ переведены в ✅ или ⏭: добавлены `upgrade.qa` (покупные апгрейды), `research.qa` (питание Research Center → уровень темы), golden-кадры `golden/upgrade`/`golden/research`; закрыты клавиатурная навигация, Esc, Name, chat, invalid order, MXLS/десинк/спектатор/FLIC помечены `not planned` с причиной.

В P7 (2026-11-16) закрыт полный `CargoMenu`: TC-0324…0328 (изображения/индикаторы, полный список покупки, режим модернизации, апгрейд до высадки, cargo) и TC-0927; QA `cargo-full.qa`, golden `golden/cargo`, C++-векторы `tools/cargo_vectors`.

В P8 (2026-11-17) закрыты `ResearchMenu` (`RSRCHPIC`) и Help-экраны: TC-0220, TC-0329/0330, TC-0929/0930; QA `research-help.qa`, golden `golden/research-help`, тексты — `data/help.json` (32 секции, 661 запись из `help.json` + `language.json`).

В P9 (2026-11-19) закрыт диалог `Preferences`/`Setup` (C++ `OptionsMenu`): TC-0108 (Setup больше не заглушка — остался только `Load Game`) и TC-0935; QA `preferences.qa`, golden `golden/preferences`, `effects`/`disable_fx`/`auto_save` реально действуют, `IniPreferences` пишутся в V70 (`max-brh-docs/analysis/visual-1to1-status.md` §27).

## Области и файлы

| # | Область | Файл | TC |
|---|---|---|---|
| 01 | Общее и меню | [suite/01-common-and-menus.md](suite/01-common-and-menus.md) | TC-01xx |
| 02 | Настройка игры | [suite/02-game-setup.md](suite/02-game-setup.md) | TC-02xx |
| 03 | Одиночная игра | [suite/03-single-player.md](suite/03-single-player.md) | TC-03xx |
| 04 | Hot-seat | [suite/04-hot-seat.md](suite/04-hot-seat.md) | TC-04xx |
| 05 | Сетевая игра | [suite/05-network.md](suite/05-network.md) | TC-05xx |
| 06 | Ввод в игре | [suite/06-in-game-input.md](suite/06-in-game-input.md) | TC-06xx |
| 07 | Сохранение/загрузка | [suite/07-save-load.md](suite/07-save-load.md) | TC-07xx |
| 08 | Победа/поражение | [suite/08-victory-defeat.md](suite/08-victory-defeat.md) | TC-08xx |
| 09 | Визуал 1:1 | [suite/09-visual-1to1.md](suite/09-visual-1to1.md) | TC-09xx |
| 10 | Детерминизм и сеть | [suite/10-determinism-and-net.md](suite/10-determinism-and-net.md) | TC-10xx |
| 11 | Edge-кейсы и стабильность | [suite/11-edge-cases.md](suite/11-edge-cases.md) | TC-11xx |

## Сводная матрица покрытия

| Область | Всего TC | ✅ | 🟡 | ⏭ | ⬜/❌ |
|---|---|---|---|---|---|
| 01 Меню | 14 | 12 | 1 | 1 | 0 |
| 02 Настройка | 20 | 20 | 0 | 0 | 0 |
| 03 Одиночная | 30 | 28 | 0 | 2 | 0 |
| 04 Hot-seat | 13 | 11 | 0 | 2 | 0 |
| 05 Сеть | 15 | 12 | 0 | 3 | 0 |
| 06 Ввод | 35 | 34 | 0 | 1 | 0 |
| 07 Сейвы | 12 | 11 | 0 | 1 | 0 |
| 08 Победа | 9 | 7 | 0 | 2 | 0 |
| 09 Визуал | 34 | 33 | 0 | 1 | 0 |
| 10 Детерминизм | 14 | 12 | 0 | 2 | 0 |
| 11 Edge | 14 | 13 | 0 | 1 | 0 |
| **Итого** | **210** | **193** | **1** | **16** | **0** |

`⏭ not planned` (16): TC-0108 (меню Load Game — заглушка; Setup/Introduction/Credits портированы, см. 🟡),
TC-0114 (авто-масштаб — WINDOW), TC-0309/TC-0314 (сквозной бой/поражение —
нужна QA-команда урона), TC-0411/TC-0412 (hot-seat до конца / Load Hot Seat),
TC-0511 (хук десинка), TC-0514 (спектатор — блокер сервера), TC-0515 (живой
выход из сессии), TC-0611 (часть display-фильтров), TC-0706 (MXLS-загрузка),
TC-0807/TC-0809 (сквозное поражение/снятие угрозы), TC-0918 (ручное сравнение
с C++-кадром), TC-1012 (десинк-хук), TC-1014
(mixed-сессия — блокер C++ headless client), TC-1107 (живой alt-tab).
Причины — в соответствующих строках.
