# 02 — Настройка игры (TC-02xx)

Экраны Options / Planet / Players (одиночная и hot-seat).

| ID | Название | Шаги | Ожидание | Автоматизация |
|---|---|---|---|---|
| TC-0201 | Options — раскладка 1:1 | открыть New Game | OPTNFRM, 6 панелей 200×190 (6,38/216,38/426,38/6,233/216,233/426,233), 71 опция + 51 элемент | ✅ golden `options.png` |
| TC-0202 | Options — изменение опций | менять сложность/таймер/кредиты/ресурсы/условие победы | значения сохраняются в настройки партии | ✅ UNIT (setup тесты); QA sp-full (по умолчаниям) |
| TC-0203 | Options — границы значений | таймер/кредиты/лимит победы в крайние значения | кламп как в C++, без паники | ✅ UNIT |
| TC-0204 | Options — Cancel | Cancel из Options | возврат в меню, настройки не применены | ✅ UNIT |
| TC-0205 | Planet — раскладка 1:1 | перейти на Planet | PLANETSE, большая картинка (26,98), сетка 3×2 мини-карт (192,51…), имя/описание, стрелки групп | ✅ golden `planet.png` |
| TC-0206 | Planet — выбор кликом | клик по мини-карте | выбранная планета подсвечена, world меняется | ✅ QA; UNIT |
| TC-0207 | Planet — стрелки групп | стрелки групп 0↔18 | смена группы планет | ✅ UNIT |
| TC-0208 | Planet — Random | Random | случайная планета, сразу Done | ✅ UNIT |
| TC-0209 | Players — раскладка 1:1 | перейти на Players | CHOSPLYR, 4 слота × Human/Computer/None (x=175/285/394, y=67+92r), флаги CH_TM* | ✅ golden `players.png` |
| TC-0210 | Players — 1 человек + ИИ | RED=Human, GREEN=Computer | Done допустим, партия стартует | ✅ QA sp-full.qa |
| TC-0211 | Players — только 1 игрок | только RED=Human, остальные None | Done отклоняется сообщением «нужно два участника» (как C++) | ✅ UNIT |
| TC-0212 | Players — смена Human | Human на другом слоте | флаг переносится с прежнего человека (одиночная) | ✅ UNIT |
| TC-0213 | Players — hot-seat флаги | hot-seat: два Human | флаг НЕ переносится; Done требует 2 человек | ✅ UNIT; QA hs-2.qa |
| TC-0214 | Players — Cancel/Дone | Cancel и Done | как C++ | ✅ UNIT |
| TC-0215 | Настройка: Name | ввести имя | имя игрока сохраняется | ✅ UNIT: поле имени LocalSetup (`GameSetupForm::push_char/backspace`, лимит 24), `player_name` уходит в `LocalSetup`/V70 `SaveMeta`; тест `app.rs::non_ascii_player_name_renders_with_the_fallback_glyph` |
| TC-0216 | Кнопка `?` нижней строки | сравнить кадр Players/Options с C++ | знак `?` по центру кнопки 48×40: `x = ulx + (48 − (Text_GetWidth+1))/2`, `y = uly + (40 − 16)/2` (`Text_TextLine`/`Button::SetCaption`) | ✅ golden players.png / options.png; UNIT `menu::draw_image_button` |
| TC-0217 | Эмблемы кланов на флагах | Players с выбранным кланом | `CLNnLOGO` рисуется с якорем (41,40) внутри флага 93×89: `ulx/uly` ресурса вычитаются (`WindowManager_DecodeSimpleImage`) | ✅ golden players.png; UNIT setup |
| TC-0218 | Экран выбора клана | клик по флагу команды | CLANSEL, 8 карточек 91×86, логотипы по центру (якорь 41,40), выбранная карточка — down-состояние, имя зелёным | ✅ QA sp-full (кадр `clan.png`); UNIT setup (clan screen) |
| TC-0219 | Options → Credits управляет мобилизацией | Options: «Credits» ≠ 0 → Players Done | после Players открывается `CargoMenu` (`start_gold + Clans::GetCredits`); при 0 — фазы нет, сразу точка высадки (`GameManager_PlayerMissionSetup`) | ✅ QA cargo-full (`assert screen=Mobilize`); UNIT `start_phases::zero_start_gold_skips_mobilization` |
| TC-0220 | Help-экраны настройки | кнопка `?` (465,438 48×40) на Options/Planet/Players | окно `HELPFRAM` 301×231 по центру окна `(169,124)`, текст секций `OPTIONS_SETUP`/`PLANET_SETUP`/`NEW_GAME_SETUP` (в hot-seat — `HOT_SEAT_SETUP`), прокрутка `PageUp`/`PageDown`/стрелки, `Done`/`Enter`/`Esc` закрывает и возвращает экран | ✅ QA `research-help.qa` (`help options/planet/newgame`, `assert help-section`, `help-row`); golden `golden/research-help/help-{options,planet,newgame}.png` |
| TC-0221 | Подменю New Game и выбор миссии (этап 2 кампании) | `New Game` в главном меню | подменю «New Game Menu» (`menu_new_game_menu_loop`): 7 кнопок Training Missions / Stand Alone Missions / Custom Game / Custom Scenarios / Campaign Game / Tips / Cancel по `menu.cpp:2181-2189` (200×29, x=385, шаг 35; Cancel 100×30), горячие клавиши `Shift+T/S/U/M/A/I/C`; Training/Scenario/Custom Scenarios/Campaign открывают `GameSetupMenu` — 12 строк миссий (18,184+20·i, 294×19), панель описания (`SUBTITLE`/`BIGSCRNL`/`BIGSCRNR`), страницы/брифинг (`MNUUAROU`/`MNUDAROU`), `Cancel`/`?`/`Start`; `Start` стартует миссию через `App::start_mission_from_registry` (тот же `snapshot_bytes`, что прямой load); `Custom` — прежняя настройка партии; отсутствующий файл миссии помечен `[missing]`, описание называет файл, старт отклоняется; `Tips` открывает окно `_(e81f)` (23 строки/страница) | ✅ QA `visual-screens.qa` (кадры `new-game`, `new-game-tips`, `mission-select`); UNIT `max_app/tests/game_setup_menu.rs` (12) |

Итог: 21 TC, ✅ 21, 🟡 0.
