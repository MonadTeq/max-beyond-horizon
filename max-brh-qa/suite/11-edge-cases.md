# 11 — Edge-кейсы и стабильность (TC-11xx)

Граничные ситуации, ошибки, ресурсы, производительность.

| ID | Название | Шаги | Ожидание | Автоматизация |
|---|---|---|---|---|
| TC-1101 | Пустой каталог данных | запуск без MAX.RES | fallback-UI, предупреждение, без паники | ✅ UNIT |
| TC-1102 | Битый MAX.RES/PATCHES.RES | повреждённые файлы | понятная ошибка/деградация, без паники | ✅ UNIT res/wrl ошибки |
| TC-1103 | Битые .qa-скрипты | синтаксис/неизвестная команда | exit 2 с текстом | ✅ UNIT qa parser |
| TC-1104 | Очень маленькое окно | 64×48 | кадр масштабируется/letterbox, панели не падают | ✅ UNIT hud |
| TC-1105 | Очень большое окно | 4K | апскейл, клики корректны | ✅ UNIT frame_scale |
| TC-1106 | Быстрый ввод/спам кликов | серия кликов подряд | без паник, состояние консистентно | ✅ QA `edge-cases.qa`: 50 кликов по пустым областям главного меню + 50 по карте партии, затем `assert screen=InGame`, `assert turn=1`, End Turn работает; `tests/qa_driver.rs::edge_cases_script_passes_click_spam_and_unicode` |
| TC-1107 | Alt-Tab/сворачивание | свернуть/развернуть | окно живо, рендер продолжается | ⏭ not planned (WINDOW): SDL-события `FocusLost/FocusGained/Minimized/Restored/Shown/Hidden/Exposed/Occluded` классифицированы как нейтральные (`window_event_is_input_neutral`, тест), ввод по ним не идёт; живой alt-tab требует реального окна |
| TC-1108 | Утечки/рост памяти | длинный прогон (100+ ходов) | память стабильна | ✅ SOAK: `scripts/soak-100.qa` (100 ходов, команда `mem` на старте/каждые 20/в конце) + `examples/soak_headless.rs` (120 ходов ИИ×ИИ, RSS старт/конец/каждые 10 ходов, порог роста 64 МиБ); цифры — в `qa-out/*/log.txt`, `qa-out/soak-headless/soak.txt` и §2.9 release-checklist |
| TC-1109 | Запуск из каталога exe | запустить exe из release\ (CWD — чужой каталог) | данные находятся от каталога exe: `MAX.RES`/`PATCHES.RES`/`COLOR.PAL` ищутся в каталоге `current_exe` и вверх до ближайшего `max-original` (плюс прежние CWD/конфиг); если ничего нет — точная подсказка про `--data-dir/--max-res` | ✅ UNIT assets (`exe_dir_resolves_game_data_when_cwd_is_elsewhere`, `exe_ancestors_are_searched_up_to_the_data_root`) |
| TC-1110 | Долгая партия | 50+ ходов ИИ против ИИ | партия доходит до конца без паники | ✅ QA `soak-100.qa`: 100 × `end_turn`, `assert turn=21/41/61/81/101`, `assert screen=InGame`, кадры каждые 20 ходов, V70-сейв на 100-м; headless `soak_headless --turns 120` |
| TC-1111 | Юникод/не-ASCII ввод | символы вне ASCII в чате/имени | корректная обработка (fallback глифы) | ✅ QA `edge-cases.qa`: `type "Привет, мир! 汉字 🚀"` в чате и `"Командир 汉字"` в поле имени — не-ASCII отклоняется без паники (ASCII-часть принимается), кадры `edge-unicode-chat`/`edge-unicode-name`; UNIT `max_render::text::glyph('Ж') == '?'` (fallback-глиф) |
| TC-1112 | Повторный запуск/выход | старт-выход-старт | чистое состояние, нет блокировок портов/файлов | ✅ UNIT `tests/app.rs::app_can_be_created_twice_and_start_exit_twice` (`App::new` дважды, один и тот же файл сейва, свежие `turn=1`/`ticks=0`, одинаковый digest); QA `edge-cases.qa`: выход из партии → New Game → Cancel → полный второй старт до хода 2 |
| TC-1113 | Логи | прогон с логами | ключевые события в логе, без спама | ✅ QA+UNIT: `log.txt` — строка на шаг с фактическим значением и итогом (`RESULT: PASS`), строки `# mem start/end`, `PROFILE` при `MAX_TICK_PROFILE=1`; журнал HUD — кольцевой буфер на 8 строк (`hud::MESSAGE_LOG_CAPACITY`, тест `message_log_is_a_ring_buffer`) |
| TC-1114 | CI-стабильность | повторный ci.sh | зелёный повторяемо (нет флаки) | ✅ (два прогона) |

Итог: 14 TC, ✅ 13, 🟡 0, ⏭ 1 (TC-1107), ❌ 0.

Проверки P4:

```sh
cd max-brh
# soak (слой 2 QA) + память/профиль в log.txt
MAX_TICK_PROFILE=1 cargo run -q -p max_app --no-default-features --example qa_run -- \
    ../max-brh-qa/scripts/soak-100.qa qa-out/soak-100
# headless ИИ×ИИ 120 ходов (время кадра/хода, RSS)
cargo run -q -p max_app --no-default-features --example soak_headless -- \
    --turns 120 --out qa-out/soak-headless
# спам кликов/юникод/повторный старт
cargo run -q -p max_app --no-default-features --example qa_run -- \
    ../max-brh-qa/scripts/edge-cases.qa qa-out/edge-cases
```
