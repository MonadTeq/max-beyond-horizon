# Save/load в одиночной игре — оценка

Версия 0.1 (Аналитик) · 2026-11-25.

## Резюме

Save/load в single-player **уже практически полностью портирован** (фаза P3). Посылка
«запись V70 / UI / автосейв отсутствуют» **не соответствует коду** — всё реализовано
и покрыто тестами. Реальный пробел только один: **прогрессия кампании**
(`last_campaign`/`game_file_number`) — это `campaign-missions.md` M5, не ядро save/load.

## Портировано (byte-exact)

| Компонент | Rust | Оракул |
|---|---|---|
| V70-чтение | `save.rs:623` `SaveFile::parse` | `saveload.cpp:748` |
| V70-запись | `save.rs:628/2096`, `save_write.rs:367/835` | `saveload.cpp:371` |
| `GameState→SaveFile` | `save_write.rs:367` `build_save_file` | `saveload.cpp:371-746` |
| `GameUniverse::from_save` | `universe.rs:2370`, `game.rs:518` | `saveload.cpp:1666` |
| Загрузка в партию | `local.rs:1840`, `lib.rs:3847` | `game_manager.cpp:3312` |
| Расширения/пути (8 категорий) | `saveload.rs:212/226/231` | `saveload.cpp:1695` |
| UI Save/Load | `saveload.rs:172/448` | `saveloadmenu.cpp:310` |
| Автосейв (слот 10) | `saveload.rs:987`, `local.rs:1217` | `game_manager.cpp:2507` |

**Важно:** Klei пишет V71, Rust сознательно пишет **V70** (Interplay) — byte-exact
round-trip тест `save1_load_to_save_is_byte_identical`.

## Parity-дельта (S1, мелкие, ~1 день)

- S1.1 Свежий RNG-сид при сохранении (решение владельца: детерминированный сид лучше для QA).
- S1.2 Бэкап перед перезаписью (`.bak`, минимум для автосейва).
- S1.3 Быстрый save/load в последний слот (`GameManager_SaveLoadGame`).
- S1.4 Split каталога сейвов (`FilePathGamePref` vs `FilePathGameData`).

## Реализация S1 (2026-11-25, Разработчик)

Каждый пункт сверен с оракулом; отклонения, принятые владельцем, отмечены явно.

### S1.1 RNG-сид — свежий сид при записи

- **Оракул:** `saveloadmenu.cpp:611-619` — C++ при записи берёт
  `rng_seed = time(nullptr)` (в сети — `Remote_RngSeed`) и прибавляет Σ байт
  **имени файла** слота; значение пишется в заголовок V70 (`saveload.cpp:495`).
  При загрузке оно сеет `Randomizer` (`saveload.cpp:776`), т. е. сид — это
  симуляционное состояние, а не косметика.
- **Порт:** `saveload.rs::save_seed` — та же формула, но вместо `time()` база —
  младшее слово живого состояния `Randomizer` (`rng().state()[0]`). **Решение
  владельца:** детерминированный сид предпочтительнее для QA; расхождение с C++
  зафиксировано здесь и в doc-комментарии. Следствия: два сохранения одного и
  того же состояния в тот же слот дают одинаковый сид, другое состояние/слот —
  другой (как «свежий» сид оригинала).
- **Байт-в-байт:** заголовок читается/пишется без потерь (`SaveFile::parse` /
  `to_bytes`, round-trip `save1_load_to_save_is_byte_identical`); тест
  `save_seed_is_byte_exact_and_reseeds_the_generator` проверяет точное значение
  в файле, повторную запись и пересев генератора.
- **Snapshot/CRC:** сам `SaveMeta::rng_seed` в `snapshot_bytes` не входит
  (`state-model.md` §3.12 — как в оригинале, где сид лежит только в заголовке),
  но после загрузки он определяет состояние `Randomizer`, а оно входит в
  `snapshot_bytes`/`state_digest`. Тест проверяет, что изменение сида в
  заголовке меняет CRC состояния.

### S1.2 `.bak` при перезаписи

- **Оракул:** `SaveLoadMenu_CreateBackup` (`saveloadmenu.cpp:734`) вызывается
  только при `backup = true`, а этот флаг передаёт лишь автосейв
  (`game_manager.cpp:2507`); меню и быстрый save пишут без бэкапа.
- **Порт:** `SaveLoadMenu::create_backup` — один `.bak` рядом со слотом
  (`save10.dta` → `save10.bak`), только для автосейва. **Решение владельца:**
  без ротации `.bak` → `.ba1` … `.ba5`, которая есть в Оракле Klei; содержимое
  `.bak` — предыдущий слот. Ошибка бэкапа логируется и не валит запись (как
  `SDL_Log` в C++).
- Тесты: `overwritten_autosave_keeps_the_previous_slot_as_bak` (перезапись →
  `.bak` = предыдущий слот, `.ba1`/`.bak.1` нет), `regular_save_does_not_create_a_backup`.

### S1.3 Быстрый save/load в последний слот

- **Оракул:** `GameManager_SaveLoadGame` (`game_manager.cpp:4032`), горячие
  клавиши `Alt+S`/`Alt+L` (`game_manager.cpp:5546-5550`). Слот —
  `SaveLoadMenu_SaveSlot`: 10 на старте партии (`game_manager.cpp:3053`),
  обновляется успешной записью в окне (`saveloadmenu.cpp:440`) и загрузкой
  не-кампании (`game_manager.cpp:3359-3361`). При наличии файла — `OKCancelMenu`
  с подписью «OK to save/load file?\n<файл>\n"<имя>"»; `OK` пишет слот с именем
  из файла, `Cancel` ничего не делает; нет файла — открывается полное окно.
- **Порт:** `App::last_save_slot`/`set_last_save_slot`,
  `App::quick_save`/`quick_load`, `SaveConfirm` (раскладка `OKCancelMenu`:
  `HELPFRAM`, подпись (20,14,265,175), `Cancel` (85,193), `OK` (155,193);
  `Enter`/`Esc`/клики). `Alt+S`/`Alt+L` в `ui::handle_session_input`.
- **Эквивалентность формата:** быстрый save идёт через тот же
  `App::save_game_to_slot` → `save_v70_to`, поэтому байты совпадают с окном
  (тест `quick_save_writes_the_last_slot_in_the_window_format`).

### S1.4 Split каталога `FilePathGamePref` / `FilePathGameData`

- **Оракул:** `SaveLoad_GetFilePath` (`saveload.cpp:72-78`) —
  `Custom`/`HotSeat`/`Multi` → `FilePathGamePref`, остальные категории
  (кампания, тренировка, демо, сценарий, мульти-сценарий) → `FilePathGameData`.
- **Порт:** `MissionCategory::uses_pref_dir`, `App::save_root`, запись и
  загрузка слота используют один и тот же корень; `AppConfig::save_dir` —
  `FilePathGamePref` (там же `preferences.ini`), `AppConfig::save_data_dir` —
  `FilePathGameData` (по умолчанию каталог `MAX.RES`).
- **Упрощение:** у порта нет SDL preference-каталога, поэтому по умолчанию оба
  корня совпадают (каталог данных игры). Наблюдаемое поведение — как в
  оригинале при явно заданных путях; тест
  `save_directory_follows_the_mission_category` проверяет
  custom→pref, campaign→data для записи и загрузки.
- Замечание: Оракл Klei в `SaveLoadMenu_Save` (`saveloadmenu.cpp:599`) всегда
  пишет в pref-каталог; порт, по требованию владельца (S1.4), применяет split и
  к записи — иначе кампания не читалась бы из data-каталога.

### Валидация S1

- `cargo check --workspace`, `cargo test -p max_core -p max_app --no-default-features` — зелёные;
- `qa_run save-load.qa` — PASS (добавлены шаги быстрого save/load), `qa_golden_check`
  на `golden/save-load` — 0 дрейфа.

## Прогрессия кампании (S2 = campaign-missions.md M5)

- `last_campaign`/`game_file_number` (settings) + связка «победа → следующая миссия».
- Выносится в `campaign-missions.md` (M1 реестр + M5 прогрессия).

## Рекомендация

Если цель — «работающий save/load в SP» — **уже закрыт** (нужна верификация S0 + опц. S1).
Если цель — «победа открывает следующую миссию» — это `campaign-missions.md` (M1+M5).
