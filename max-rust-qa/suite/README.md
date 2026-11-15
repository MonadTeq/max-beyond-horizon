# Каталог тест-кейсов клиента M.A.X.

Обозначения автоматизации:
- `QA:<script>` — .qa-скрипт в `max-rust-qa/scripts/`;
- `UNIT` — юнит/интеграционные тесты (`cargo test`);
- `SMOKE` — живой сетевой smoke (`build/scripts/live-smoke.sh`);
- `WINDOW` — оконный прогон на реальном бинаре (`max-rust-qa/drivers/`);
- `MANUAL` — ручной сценарий.

Статусы: ✅ покрыто и зелёное; 🟡 покрыто частично/с оговоркой; ⬜ не покрыто/запланировано; ❌ известно сломанное.

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

| Область | Всего TC | ✅ | 🟡 | ⬜/❌ |
|---|---|---|---|---|
| 01 Меню | см. файл | | | |
| 02 Настройка | | | | |
| 03 Одиночная | | | | |
| 04 Hot-seat | | | | |
| 05 Сеть | | | | |
| 06 Ввод | | | | |
| 07 Сейвы | | | | |
| 08 Победа | | | | |
| 09 Визуал | | | | |
| 10 Детерминизм | | | | |
| 11 Edge | | | | |

(Матрица заполняется по мере прохождения; актуальные числа — в каждом файле области.)
