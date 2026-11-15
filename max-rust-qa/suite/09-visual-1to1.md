# 09 — Визуал 1:1 (TC-09xx)

Паритет каждого экрана с оригиналом (оракул C++-порт, эталоны `build/`).

| ID | Название | Эталон/проверка | Автоматизация |
|---|---|---|---|
| TC-0901 | Главное меню | MAINPIC + BLANK_* + портрет P_TANK + подписи GNW | ✅ golden main.png |
| TC-0902 | Multiplayer Menu | MAINPIC + 5 кнопок + Cancel | ✅ golden |
| TC-0903 | Options | OPTNFRM + 71 опция + панели | ✅ golden options.png |
| TC-0904 | Planet | PLANETSE + мини-карты + описание | ✅ golden planet.png |
| TC-0905 | Players | CHOSPLYR + радиогруппы + флаги | ✅ golden players.png |
| TC-0906 | Join list | MULTGAME + строки + кнопки | ✅ golden |
| TC-0907 | Окно игры (хром) | FRAMEPIC + панели + раскладка windows[] | ✅ golden game.png |
| TC-0908 | Карта тайлами | WRL-тайлы 64×64, палитра 6→8 бит, зум | ✅ UNIT terrain; golden |
| TC-0909 | Спрайты юнитов/зданий | SPW-декодер, 39 типов + тени, 8 направлений | ✅ UNIT sprite |
| TC-0910 | Миникарта + FOV | minimap WRL + затемнение + рамка + точки | ✅ UNIT; golden |
| TC-0911 | Маркеры ресурсов | RAWMSK/FUELMK/GOLDMK, кадр по количеству | ✅ UNIT |
| TC-0912 | Шрифты | GNW FONT1/2/5, метрики, тень — как C++ | ✅ UNIT (font vectors) |
| TC-0913 | Курсоры | Cursor_Descriptor, 30 курсоров, контексты | ✅ UNIT |
| TC-0914 | Экран Game Over | ENDGAME1..9 + статистика | ✅ golden |
| TC-0915 | Окно перехода хода | HELPFRAM+HELPOK «Begin turn» | ✅ golden |
| TC-0916 | Выбор планеты в форме | SNOW_PIC+world/6, рамка выбранной | ✅ golden |
| TC-0917 | Дрейф-гейт | golden-сверка всех кадров, порог 1% | ✅ CI |
| TC-0918 | Паритет кадров с C++-порталом | сравнение с build/run/dbg.png, smoke-window.png | 🟡 выборочно (вручную) |
| TC-0919 | Портрет в CORNER_FLIC | FLIC-анимация (сейчас статичный спрайт) | ⬜ FLC не разобран |

Итог: 19 TC, ✅ 17, 🟡 1, ⬜ 1.
