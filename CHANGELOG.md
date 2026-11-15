# Changelog

Все заметные изменения Rust-клиента M.A.X. Формат — [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/),
нумерация — [Semantic Versioning](https://semver.org/lang/ru/).

Проект распространяется по лицензии [MIT](LICENSE); оригинальные данные игры
(`MAX.RES`, `PATCHES.RES`, `COLOR.PAL`, `*.SPW`, `*.WRL`, сейвы) в репозиторий
не входят как свободные ассеты и остаются собственностью Interplay Productions
(см. `README.md`).

## [Unreleased]

### Изменено

- **Бинарь клиента переименован `max_app` → `max-rust`** (Windows:
  `max-rust.exe`). Крейт и библиотека остаются `max_app`, примеры
  `qa_run`/`qa_golden_check` не тронуты; `cargo run -p max_app` по-прежнему
  работает (бинарь в пакете один), а артефакты теперь `max-rust(.exe)`.
  Каталоги артефактов (`max_app-<version>-…`) сохранены.
- **Запуск без флагов открывает GUI** (меню SDL3): headless-режимы доступны
  только по явным `--demo`, `--new-game`, `--connect`; `--ui` оставлен
  совместимым alias. В headless-сборке без feature `sdl` запуск без флагов
  даёт понятную ошибку с подсказкой про `--features sdl` и headless-флаги.
- Help-текст, build/release-скрипты и документация переведены на
  `max-rust.exe`; Windows UI-артефакт содержит `max-rust.exe` + `SDL3.dll`.

### Добавлено

- **Выбор клана в настройке партии** (`ClanSelectMenu`): экран `CLANSEL` с
  8 карточками `CH_CN1..8_U/D` и логотипами `CLN1..8LOGO`, Random/Cancel/?/Done;
  выбранный клан пишется в `LocalSetup::team_clans`, попадает в
  `NewGameConfig::clans` и меняет стартовые атрибуты юнитов команды
  (`ResourceManager_InitClanUnitValues`). Контейнер `MXLS` — версия 3
  (`team_clans`), старые v2 читаются с кланами Random.
- **M-SP — одиночная игра против ИИ** (`docs/architecture/plan-modes.md`):
  - экран настройки 1:1 из трёх экранов C++-порта (`max_app/src/setup.rs`):
    `GameConfigMenu` (`OPTNFRM`: сложность ИИ, таймеры, режим игры, кредиты,
    уровни ресурсов, условие победы), `PlanetSelectMenu` (`PLANETSE`: 3×2
    мини-карты из `.WRL`, стрелки групп, описания 24 планет),
    `ChoosePlayerMenu` (`CHOSPLYR`: 4 слота Human/Computer/None, флаги команд
    с эмблемой клана);
  - локальный цикл (`max_app/src/local.rs`, порт `GameManager_TeamTurnTurnBased`):
    `manage_economy` → `Ai_BeginTurn` → тики ИИ до `end_turn_retry_counter > 120`
    → пакет `06` → следующая команда → `finish_turn`; End Turn, таймер хода,
    пауза; 1 человек + 1–3 компьютера, V70 читается в локальную партию;
  - экран победы/поражения 1:1 (`max_app/src/winloss.rs`):
    `menu_draw_game_over_screen` — `ENDGAME1..9`, места, статистика,
    `ENDARM`/`ENDOK`;
  - контейнер сохранения локальной партии `MXLS`
    (`App::local_save_bytes`/`write_local_save`/`read_local_save`): настройки +
    фаза + снимок состояния с CRC (запись `GameState → V70` — открытый пункт);
  - 12 сквозных тестов `max_app/tests/local_sp.rs`, кадры
    `cargo run -p max_app --example sp_frames`.

### Исправлено

- **Юниты плавно двигаются в кадре**: `App::update_visual_units` догоняет
  симуляционную клетку (`UnitInfo::Move` в оригинале двигает спрайт
  пиксельно); `dt = 0` (headless/QA) оставляет снимок без интерполяции, поэтому
  golden-кадры не дрейфуют.
- **Спрайты юнитов масштабируются вместе с картой** (`zoom / 64`): при
  удалении/приближении камеры юнит и тень уменьшаются/увеличиваются, горячая
  точка остаётся в центре клетки (`SpriteRenderer::draw_unit_scaled`).
- **Кнопки больше не выглядят нажатыми при наведении**: `*D`/тёмная подпись
  остаются только у выбранных radio/toggle-кнопок; hover не нажимает
  (`draw_image_button`, экраны команд, hot-seat, победа/поражение).
- **Декодер big image**: RLE начинается с байта 776, а не 777 (C++
  `WindowManager_DecodeBigImage` читает с `&image->transparent_color`).
  До исправления `FRAMEPIC` и `ENDGAME1..9` декодировались со сдвигом на байт
  («scanline»-артефакты); исправление затронуло хром игрового окна, фоны
  экрана победы и все big image меню.

## [1.0.1] — 2026-11-15

- Исправлено зеркальное отображение текста UI (порядок битов глифов font8x8 — LSB-first); пересобраны Windows UI- и Linux-артефакты.

## [1.0.0] — 2026-11-15

Первый релизный срез Rust-клиента: самостоятельный клиент M.A.X. под Horizon,
который собирается без SDL/ALSA для headless-проверок и включает SDL3-окно и
звук отдельными feature-флагами.

### Добавлено

- **M6.2 — звук**:
  - `max_assets::wav` — чистый stdlib-парсер RIFF/WAVE PCM16 (моно/стерео),
    возвращает `WavClip { sample_rate, channels, samples: Vec<i16> }`;
    пропускает `smpl`/`fact`/неизвестные чанки, не паникует на битых данных;
  - `max_assets::res::parse_file_ref` и `ResArchive::file_reference` — чтение
    ASCIIZ-ссылок `MAX.RES` (`TANK1` → `"tank1.spw"`);
  - `max_app::audio` — `AudioBackend` (`NoopBackend` всегда, `RodioBackend` за
    feature `sound`), `AudioPlayer` с кэшем, `play_once`/`play_loop` и
    регистронезависимым поиском файлов рядом с `MAX.RES`;
  - клики меню в `--ui` озвучиваются (`KBUY0` → `kbuy0.spw`); звук не блокирует
    игровой цикл и молча деградирует до no-op без устройства/файлов.
- **M6.4 — релиз**:
  - версия workspace `max-rust` — **1.0.0** (все пять крейтов наследуют её);
  - `docs/development/release-checklist.md` — полный чек-лист релиза;
  - `release.sh` — сборка Linux-бинаря в Docker, упаковка `release/` с
    `README.txt`, `LICENSE`, `CHANGELOG.md`, `SHA256SUMS` и `.tar.gz`;
    подготовка/создание тега `v1.0.0` по флагу `--tag`;
  - `ci.sh` и GitHub Actions проверяют сборку с feature `sound`.

### Клиент умеет

- Загружать данные оригинальной игры: `MAX.RES`, `PATCHES.RES`, `COLOR.PAL`,
  карты `*.WRL`/ресурсы `SNOW_1`…`DESERT_6`, спрайты и тени юнитов.
- Играть локальную партию: `setup::generate_map`, высадка, ходы, приказы,
  победа/поражение (`max_core` — детерминированная модель).
- Работать по сети: Horizon gRPC → билет → ENet-сессия, лобби (создание/список/
  вход), приказы 08/38/41/50, чат 18, CRC-барьеры 45/52, реконнект транспорта
  (ENet + `TP 00`) с настраиваемыми попытками.
- Показывать SDL3-окно с меню, картой, HUD, миникартой, панелью юнита,
  журналом и предпросмотром маршрута (feature `sdl` / `sdl-from-source`).
- Играть звуки меню (feature `sound`).

### Ограничения 1.0.0

- Реконнект восстанавливает только транспорт; повторный вход в идущую партию
  (SNAP/54-восстановление) и спектатор — после 1.0.0 (M6.1-финал).
- Видеоролики FLC/MVE не воспроизводятся.
- Озвучены только клики меню; звуки юнитов/выстрелов/взрывов декодируются
  (`WavClip`), но точки вызова в игровом цикле появятся позже.
- Windows-бинарь в этом репозитории не собирается автоматически: инструкция —
  в `docs/development/release-checklist.md` §6 (нужен MSVC + vcpkg SDL3).
- Человеческий сценарий M4 и live-smoke 2×10 — гейты релиза, а не свойство
  кода: они проверяются по чек-листу на машине с SDL/данными игры.

### Этапы

| Этап | Итог |
|---|---|
| R1 | Каркас workspace, чистые границы крейтов, CI-заготовка. |
| R2 | Парсеры RES/WRL/спрайтов, WS-лобби Horizon, UDP-сессия, рендер карты. |
| M3 | Игровой цикл: тики, приказы, CRC-барьеры, два клиента против headless. |
| M4 | SDL3-окно, меню, создание/вход в игру, HUD, мышь/хоткеи, чат (18). |
| M5 | Horizon из конфига, docker-compose (Horizon + C++-headless), healthcheck. |
| M6 | Реконнект-заготовка, звук (feature), CI, версия 1.0.0, чек-лист, релиз. |

[1.0.0]: https://github.com/klei1984/max/releases/tag/v1.0.0
