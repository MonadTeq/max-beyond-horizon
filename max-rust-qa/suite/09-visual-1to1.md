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
| TC-0918 | Паритет кадров с C++-порталом | сравнение с build/run/dbg.png, smoke-window.png | ⏭ not planned: сравнение с оракульными кадрами — ручное (WINDOW, требует GUI-прогона); автоматические golden покрывают наши экраны |
| TC-0919 | Портрет в CORNER_FLIC | FLIC-анимация (сейчас статичный спрайт) | ⏭ not planned: FLIC-декодер не портирован (в C++ `flicsmgr`), портрет рисуется статичным спрайтом 2× |
| TC-0920 | Эмблемы кланов на флагах/карточках | `Players` с кланом и экран `Clans` | `CLNnLOGO` с якорем (41,40): `ulx/uly` ресурса вычитаются, логотип по центру флага 93×89 и карточки 91×86 | ✅ golden players.png; UNIT menu/setup |
| TC-0921 | Центровка знака `?` | кнопка `MNUBTN5` 48×40 | `x = ulx + (48 − (Text_GetWidth+1))/2 = 484`, `y = uly + (40−16)/2 = 450` — как `Button::SetCaption`; сверено с оракульным кадром C++ | ✅ UNIT menu; сверка с C++ |
| TC-0922 | Экран мобилизации | после `Players` Done | `CARGOPIC`, 15 типов покупки с иконками `I_*` и ценами, списки Purchased/Available 32 px, `Buy/Delete/Cancel/Done`, `CRGTOG`, полоса золота и cargo-слайдер | ✅ golden mobilize.png, `golden/cargo/cargo-buy.png`; QA sp-full/cargo-full |
| TC-0923 | Экран выбора точки высадки | после мобилизации | карта 112×112 в `MAIN_MAP` (4 px/клетка), курсор 4×4, рамка «Select starting location.», `Cancel/Done` | ✅ golden landing.png; QA sp-full |
| TC-0924 | Экран BuildMenu | выделить конструктора/завод → `B` | `CONBUILD`/`FACBUILD` 640×480, список 483,60 (115×369, строки 32 px = 11), очередь завода 337,60 (116×220 = 6), кнопки `BLDBLD/2X/4X/ONE/CAN/UP__/DWN/DEL/REP` по координатам C++, портрет-спрайт 2×, цифры ходов/цены | ✅ golden `golden/production/buildmenu*.png`; QA production.qa |
| TC-0925 | Экран апгрейдов | выделить юнита → `U` | `UPGRADE` 640×480, портрет (11,13), статистика (11,293), цены (321,293), золото (361,276), стрелки `UPGLFT/UPGRGT` (283/301), список 482,80 (9×32), фильтры `SEL*` (411), `Done/Help/Cancel` (452) | ✅ golden `golden/upgrade/upgrade-menu.png`; QA upgrade.qa |
| TC-0926 | Исследование завершено | Research Center → `O`, 16 ходов | кадр партии после уровня 1 (оверлей «Research Complete» в C++ не портирован — статичный кадр) | ✅ golden `golden/research/research-done.png`; QA research.qa |
| TC-0927 | Мобилизация: модернизация | `CRGTOG` → полный список, фильтры, апгрейды | иконки `I_*` в обоих списках, стрелки `UPGLFT`/`UPGRGT`, цены (321..361) с `I_GOLD`, полоса золота `VERTGOLD`, cargo-слайдер, `Buy Units`/`Upgrades` | ✅ golden `golden/cargo/cargo-upgrade.png`, `cargo-upgraded.png`; QA cargo-full |

Итог: 27 TC, ✅ 25, 🟡 0, ⏭ 2 (TC-0918, TC-0919).
