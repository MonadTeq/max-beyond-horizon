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

## Прогрессия кампании (S2 = campaign-missions.md M5)

- `last_campaign`/`game_file_number` (settings) + связка «победа → следующая миссия».
- Выносится в `campaign-missions.md` (M1 реестр + M5 прогрессия).

## Рекомендация

Если цель — «работающий save/load в SP» — **уже закрыт** (нужна верификация S0 + опц. S1).
Если цель — «победа открывает следующую миссию» — это `campaign-missions.md` (M1+M5).
