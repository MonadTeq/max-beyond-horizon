# 01 — Общее и меню (TC-01xx)

Главное меню, экран Multiplayer, запуск/выход, окно, звук.

| ID | Название | Шаги | Ожидание | Автоматизация |
|---|---|---|---|---|
| TC-0101 | Запуск с данными | запустить `max-rust.exe` (без флагов — GUI) из корня репозитория | окно 640×480 (или scaled), главное меню с MAINPIC и 6 кнопками | ✅ WINDOW ui-test-sp.ps1 (shot 01) |
| TC-0102 | Запуск без данных | запустить из чужого каталога без `--data-dir` | окно открывается, меню в fallback-режиме (процедурное), предупреждение о данных, без паники | ✅ UNIT (assets fallback тесты); поиск от каталога exe — TC-1109 |
| TC-0103 | Главное меню — раскладка 1:1 | сравнить кадр с эталоном C++ (`build/frame-ef-main.png`) | MAINPIC, заголовок (236,145), 6 кнопок BLANK_UP 200×29 x=385 y=175…350, Exit (435,420) | ✅ golden `main.png` |
| TC-0104 | Навигация меню мышью | hover по кнопкам, клик | подсветка (BLANK_DN) при наведении, переход по клику | ✅ QA visual-screens.qa; UNIT (menu tests) |
| TC-0105 | Навигация клавиатурой | стрелки/Tab + Enter; хоткеи (первая буква?) | выбор и активация как мышью | ✅ UNIT: Up/Down/Tab+Enter в главном меню (`ui.rs::main_menu_opens_the_local_setup_and_escape_goes_back`), хоткеи `C`/`J`/`Q`; Join/Create/Setup-формы — стрелки/Tab |
| TC-0106 | Кнопка New Game | клик | переход на экран Options (одиночная игра) | ✅ QA sp-full.qa |
| TC-0107 | Кнопка Multiplayer Game | клик | экран Multiplayer Menu (Host/Join/Hot Seat/… 1:1) | ✅ QA hs-2.qa; golden |
| TC-0108 | Кнопки Load Game / Setup / Introduction / Credits | клик | соответствующие экраны; Setup — настройки; Introduction/Credits — экраны или статус | 🟡 частично: **Introduction** — настоящее MVE-видео `INTROFLC` → `maxint.mve` (`max_assets::mve`: 432×320, 4391 кадр, 15 FPS, кадр 0 при входе, letterbox 640×474; аудио/субтитры `INTROSUB` — ограничение), Esc/клик/конец — в меню; **Credits** — построчный порт `menu_credits_menu_loop` (фон `CREDITS` 640×480, 171 строка из `menu.cpp` + `language.json`, прокрутка 10 мс/пиксель, шаг строки 20, скорость `-`/`+`, Esc/любая клавиша — в меню, автозакрытие по концу прокрутки); `Load Game`/`Setup` — по-прежнему статус «screen is not ported yet» (загрузка партии доступна в игре: `L`, V70) | ✅ QA visual-screens.qa (`menu intro`/`menu credits`, golden `intro.png`/`intro-logo.png`/`credits.png`); UNIT `credits` (7), `intro` (3), `mve` (3) |
| TC-0109 | Exit | клик/хоткей | корректный выход, код 0 | ✅ UNIT; WINDOW |
| TC-0110 | Esc на верхнем уровне | Esc в главном меню | выход (как Exit) или бездействие — по C++ | ✅ UNIT: `Key::Escape` в главном меню → `Transition::Quit` (`ui.rs:1122`); `ui.rs::main_menu_opens_the_local_setup_and_escape_goes_back` |
| TC-0111 | Звук кликов меню | включить sound, кликать кнопки | проигрывается `kbuy0.spw` (WASAPI) | ✅ WINDOW (запуск с backend=rodio); прослушивание — ручное, вне CI |
| TC-0112 | Курсор в меню | двигать мышь над меню | курсор HANDPTR (оригинальный, не системный) | ✅ UNIT (cursor context); WINDOW визуально |
| TC-0113 | Масштабирование окна (F) | `--scale 2`, `--scale 3`, ресайз окна | кадр 640×480 апскейлится nearest-neighbour; клики пересчитываются | ✅ UNIT (frame_scale тесты) |
| TC-0114 | Авто-масштаб под рабочий стол | запуск без --scale на большом экране | окно масштабируется под экран, чёрные поля (letterbox) | ⏭ not planned (WINDOW): `--scale 0`/нет передаёт `None` в `SdlWindow::new_scaled`, масштаб подбирается по рабочему столу; проверка требует реального дисплея — вне headless CI |

Итог: 14 TC, ✅ 12, 🟡 1 (TC-0108 — Introduction/Credits портированы, Load/Setup — нет), ⏭ 1 (TC-0114).
