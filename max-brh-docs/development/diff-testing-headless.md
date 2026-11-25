# Дифференциальное тестирование C++-headless: разведка

**Версия 0.1 (Разработчик) · 2026-11-25.**

Объект: `bin/linux-server-template/max-port` — C++-headless (Linux x64 ELF, MAX_HEADLESS),
57 994 768 Б, sha256 `31ad63198585e635ff9f987705d1f63c82640e1dffeeffaa91b7c16a0c4bb3ec0`,
BuildID `38b44c3086000c210e6d46a9e1fc8168a3f9867f`, с отладочной информацией
(путь сборки — `/src/src/server.cpp`).

Окружение разведки: Docker Desktop 29.4.1, образы `ubuntu:24.04` и `max-port-build:latest`,
WSL Ubuntu 22.04, нативная консольная сборка Rust `bin/target-headless/release/max-brh.exe`.

Задача — только разведка: корпус сценариев не строился, файлы кода и оракулы не изменялись.

> **Ограничение источника.** Исходники форка с `--server` в текущем checkout отсутствуют:
> каталог `src/` удалён коммитом `4a25dc8 Cleanup`; `max-cpp-port/` — чистый Оракул Klei
> (`711b3de`). Тот же `src/` восстановим из истории (`4478caa`, `23d8fce`) и совпадает с
> бинарём по строкам и символам; DWARF ссылается на `/src/src/server.cpp`. Поэтому интерфейс
> исследован по бинарю (символы `Server_Main`, дизассемблер `main`, строки) и по исходникам
> из истории; ссылки на «src/… (4478caa)» означают файл из этого коммита.

## Краткие ответы

| № | Вопрос | Ответ | Ключевое доказательство |
|---|---|---|---|
| 1 | Принимает ли headless `load <save> + N ticks`? | **Нет.** Единственный распознаваемый аргумент — `--server`; сейв и число тиков не принимаются, конфигурация — `settings.json` | `main()` (4478caa) перебирает `argv` и сравнивает только с `--server`; лишние аргументы игнорируются (прогоны ниже) |
| 2 | Детерминирован ли (два прогона → одинаковый digest)? | **В такой постановке — нет.** Ни загрузки сейва, ни счётчика тиков, ни дампа digest; сид = `time(nullptr)` хоста (пакет 13), поэтому два независимых запуска — разные партии. При одинаковом сиде генерация карты воспроизводится бит-в-бит | `networkmenu.cpp` (4478caa):197-201 и `game_manager.cpp`:3098-3103; `randomizer.cpp`:34-54; опыты с `faketime`: RNG-чекпоинты совпали у трёх прогонов с одним временем и разошлись с другим |
| 3 | Совпадает ли digest C++ с Rust на развёрнутом сценарии? | **Digest — нет:** C++-headless не выдаёт `state_digest`/`replay_digest` (`hex_digest` в бинаре — SHA256 игровых данных). **CRC — да:** пакет 45 (`GameManager_GetUnitListChecksum` = Rust `state_checksum`) арбитр сверяет каждый ход; на живом сценарии 2×Rust + C++ CRC совпал за 5 и 50 ходов, desync нет | `game_manager.cpp` (4478caa):2442–2530; `crc.rs`:174–198; прогоны `live-smoke-mixed.sh` 5/50 |
| 4 | Если не совпадает — в чём расхождение, локализуется? | Локализация есть по номеру хода и снимку юнитов (`desync-turn{N}.txt`, все поля CRC + `order`/`order_state`) и по RNG-чекпоинтам фаз генерации карты. Экономика и внутреннее состояние ИИ в CRC не входят — только косвенно, через состояние юнитов | `game_manager.cpp` (4478caa):2387–2440; `randomizer.cpp`:52–58; `remote.cpp` (4478caa):2207–2231 |

## 1. Вопрос 1 — интерфейс headless

### 1.1. Как объект запускается в проекте

`max-brh-cicd/scripts/live-smoke.sh` сам `max-port` не запускает: он поднимает Horizon с
`max-brh-server/smoke_config.toml`, а в секции `[max]` этого конфига указаны
`executable = "/src/bin/linux-server-template/max-port"`, `template_dir`, `session_root`,
`game_data` и диапазон UDP-портов. Horizon через крейт `max_session` создаёт на каждую игру
отдельный каталог-сессию и запускает процесс так
(`max-brh-server/crates/max_session/src/manager.rs:273-301`):

```text
Command::new(<session_dir>/max-port)
    .arg("--server")
    .current_dir(<session_dir>)
    .env("LD_LIBRARY_PATH", <session_dir>)
```

То есть **единственный аргумент — `--server`**, а вся конфигурация лежит в
`settings.json` каталога сессии. Прямые запуски в репозитории:
`max-brh-cicd/scripts/verify-linux-template.sh` и
`max-brh-cicd/scripts/run-linux-server-with-data.sh` — оба выполняют
`timeout ... env LD_LIBRARY_PATH=. ./max-port --server`.

### 1.2. Разбор аргументов командной строки

`main()` форка (4478caa) устроен так: перебор `argv[1..]`, `strcmp` с `--server`, при
совпадении — `Server_Main()`, иначе — обычный GUI-путь. Справки нет, других ключей нет.
Дизассемблер бинаря подтверждает: единственная строка-аргумент в бинаре — `--server`,
`Server_Main()` — отдельный символ по адресу `0x38b89b`.

Проверено прогонами (Docker, `ubuntu:24.04`, данные `max-original`):

| Команда | Результат |
|---|---|
| `./max-port` | GUI-путь; в контейнере без дисплея — exit 1, создаётся `crashes/` |
| `./max-port --help` | то же (GUI); текст справки не выводится |
| `./max-port --load /src/max-original/SAVE1.CAM --turns 5` | то же (GUI), аргументы не распознаны |
| `./max-port --server --load /src/max-original/SAVE1.CAM --turns 5` | сервер; вывод идентичен `--server` без лишних аргументов |
| `./max-port --server` | `SERVER: entry`, `SERVER: initializing resources`, `SERVER: resources ready`, далее ожидание |

Проверенная команда «load + ticks» (лишние ключи не распознаны):

```bash
cd /tmp/max-server && LD_LIBRARY_PATH=. ./max-port --server \
  --load /src/max-original/SAVE1.CAM --turns 50
# вывод идентичен `./max-port --server`; ни сейв, ни 50 тиков не применяются
```

`Server_Main()` (4478caa:39-92): выставляет `SDL_VIDEODRIVER=dummy` и
`SDL_AUDIODRIVER=dummy`, инициализирует ресурсы, транспорт, вызывает
`Remote_Lobby(true)`, затем `GameManager_GameLoop()`. Консоль (stdin) не читается:
никакого командного интерфейса у сервера нет.

**Ответ: нет, `load <save> + N ticks` headless не принимает.** Ни `--load`, ни позиционного
аргумента сейва, ни `--turns`; лишние аргументы молча игнорируются.

### 1.3. Конфигурация вместо аргументов

`settings.json` (в каталоге сессии; шаблон — `bin/linux-server-template/settings.json`):

| Секция | Ключ | Смысл |
|---|---|---|
| `SETUP` | `game_data` | каталог `MAX.RES` |
| `OPTIONS` | `world` | планета 0..23 |
| | `timer`, `endturn` | таймеры хода/конца хода, с |
| | `play_mode` | пошаговый/реальный режим |
| | `start_gold`, `victory_type`, `victory_limit` | экономика и условие победы |
| | `raw_resource`, `fuel_resource`, `gold_resource`, `alien_derelicts` | уровни ресурсов |
| `TEAMS` | `*_team_player` | тип команды: 0 NONE, 1 PLAYER, 2 COMPUTER, 3 REMOTE |
| `NETWORK_SETTINGS` | `host_address`, `host_port`, `server_session_ticket` | ENet-адрес и тикет сессии |
| `DEBUG` | `server_mode` | включает серверный режим (`remote.cpp`:794) |
| | `server_min_players` | сколько слотов ждать до старта (по умолчанию 2, `settings.cpp`:46) |
| | `server_desync_test` | тест-хук искусственного десинка (по умолчанию 0, `settings.cpp`:47) |
| | `auto_save` | автосейв слота 10 на каждом командном ходе |

`max_session` перезаписывает `settings.json` под конкретную сессию
(`max-brh-server/crates/max_session/src/settings.rs`): первые `min_players` слотов —
открытые (`team_player=0`, после входа клиента становятся `REMOTE=3`), остальные —
`COMPUTER=2`.

### 1.4. Точные команды запуска

Прямой запуск с данными игры (рекомендуемый способ разведки; шаблон копируется в
`/tmp`, `game_data` патчится на `max-original`):

```bash
docker run --rm \
  -v "D:/dev/max-port-debug:/src:ro" \
  -v "D:/dev/max-port-debug/bin/linux-server-template:/srv:ro" \
  ubuntu:24.04 bash /src/max-brh-cicd/scripts/run-linux-server-with-data.sh /srv
```

Минимальный эквивалент внутри контейнера (после копирования шаблона в рабочий каталог и
правки `SETUP.game_data`):

```bash
cd /tmp/max-server && LD_LIBRARY_PATH=. ./max-port --server
```

Через Horizon (боевой путь сессии; так запускает live-smoke):

```bash
docker run -d --name horizon-live -v "D:/dev/max-port-debug:/src" -w /src/max-brh-server \
  -p 8082:8082 -p 8080:8080 ubuntu:24.04 bash -c \
  "apt-get update -qq && apt-get install -y -qq libssl3t64 libgcc-s1 libstdc++6 >/dev/null && \
   ./target/release/horizon --config smoke_config.toml"
```

Дальше `max_session` сам запускает `max-port --server` в каталоге
`/tmp/horizon-sessions/<game_id>/`.

### 1.5. Что сервер умеет без клиентов

Если в `settings.json` поставить `server_min_players: 0` и все команды сделать
`COMPUTER` (`*_team_player: 2`), сервер стартует партию **сам**, без единого клиента:
идут ходы ИИ, пишется `ai_log.txt` и на каждом командном ходе — автосейв слота 10
(`game_manager.cpp` (4478caa):522-546). Проверено: в каталоге появляются
`ai_log.txt` (сотни КБ трассы ИИ) и `save10.mlt` (около 790 КБ). Остановить такую
партию извне можно только сигналом или условием победы `victory_limit`
(`menu.cpp`:582, Оракул Klei); счётчика тиков нет. Кампанийные миссии и произвольные
сейвы в этом режиме недоступны: сервер всегда создаёт новую партию по `settings.json`
и лобби, поэтому «миссия 1 + 50 тиков» на нём не запускается.

### 1.6. Сейвы: что пишет и что читает C++

Форк пишет **V71**: `saveload.cpp:371-500` — `uint32 version = LATEST (71)`, затем
категория, сжатый скрипт миссии, строки названия/мира/команд, типы, кланы, сложность,
`rng_seed`. Rust читает **только V70** (`max-brh/crates/max_core/src/save.rs:1534-1560`);
это уже зафиксировано в `save-load.md` §«Важно». Практическая проверка:

```text
max-brh.exe --demo build\headless-probe\det\same-s2.mlt --turns 0
  → demo: ... не удалось разобрать сейв: unsupported save version 71
```

Следствие: автосейв C++-headless нельзя загрузить в Rust для сравнения digest'ов, а
C++-сервер не имеет интерфейса загрузки сейва (единственная автоматическая загрузка —
рестарт с автосейва слота 10 при десинке, `game_manager.cpp` (4478caa):2498-2509).

## 2. Вопрос 2 — детерминизм

### 2.1. Постановка «тот же сейв + N тиков → одинаковый digest» не поддерживается

Ни один из элементов этой постановки у C++-headless не существует:

- нет аргумента/команды загрузки сейва (см. §1);
- нет счётчика тиков/ходов (остановка — только `victory_limit`, сигнал или конец партии);
- нет вывода `state_digest`/`replay_digest` (см. §3): единственный «digest» в бинаре —
  `hex_digest` — это SHA256 файлов данных (`resource_manager.cpp`:1594-1620, `world.cpp`:846),
  а не состояния.

### 2.2. Сид — системное время хоста, распространяемое через лобби

Источник сида — системное время, но в сетевой игре он передаётся пирам через лобби:

- `randomizer.cpp` (4478caa):34-54 — `Randomizer_Init()` сеет
  `Xoshiro128** (SplitMix64(time(nullptr))())`; `Randomizer_SetSeed(seed)`
  переинициализирует генератор, сбрасывает счётчик/хеш отрисовок и печатает `DBG rng cp`.
- `networkmenu.cpp` (4478caa):197-201 — после выхода из лобби хост (`is_host_mode`,
  в том числе `--server`) выполняет `Remote_RngSeed = time(nullptr)` и рассылает
  пакет 13; клиент принимает значение в `remote.cpp`:2769 (`packet >> Remote_RngSeed`).
- `game_manager.cpp` (4478caa):3098-3103 — если игра сетевая или `Remote_RngSeed != 0`,
  сид = `Remote_RngSeed`; иначе (локальная партия) `Remote_RngSeed = time(nullptr)`.
- `game_manager.cpp` (4478caa):3199-3203 — генерация карты и стартовых армий сеет
  тем же `Remote_RngSeed`, поэтому у всех пиров карта одна.

Параметра «сид» нет ни в CLI, ни в `settings.json`. Два независимых запуска в разные
секунды получают разные сиды и, следовательно, разные партии; в одной живой сессии
все пиры получают один сид через пакет 13, поэтому сверка CRC между ними корректна.

### 2.3. Опыт: одинаковый сид → одинаковые RNG-чекпоинты

Форк печатает в stdout/stderr через `SDL_Log` RNG-чекпоинты (`randomizer.cpp`:52-58):
`DBG rng cp <фаза> draws=<N> hash=<16 hex>`. Печать не закрыта `NDEBUG` — это
диагностика форка, попадающая и в релизную сборку. В опыте время фиксировалось утилитой
`faketime` (пакет `faketime` из Ubuntu, подмена через `LD_PRELOAD`; монотонное время
не подменялось, чтобы таймеры ходов шли):

```bash
FAKETIME_DONT_FAKE_MONOTONIC=1 timeout -k 5 20 \
  faketime '2026-01-01 00:00:00' env LD_LIBRARY_PATH=. ./max-port --server
```

Результаты (4 прогона, одинаковые `settings.json`; r1/r2/r4 — одно и то же фейковое время,
r3 — другое):

| Прогон | Фейковое время | `after-strategies` | `after-supplies` |
|---|---|---|---|
| r1 | 2026-01-01 | `draws=61 hash=2C0C50C84E432D8C` | `draws=19253 hash=A7356ACCA2B2AA21` |
| r2 | 2026-01-01 | `draws=61 hash=2C0C50C84E432D8C` | `draws=19253 hash=A7356ACCA2B2AA21` |
| r4 | 2026-01-01 | `draws=61 hash=2C0C50C84E432D8C` | `draws=19253 hash=A7356ACCA2B2AA21` |
| r3 | 2027-01-01 | `draws=52 hash=4AF5C8CF081C608C` | `draws=19232 hash=40B1DAD137094B09` |

r1 = r2 = r4 побайтово, r1 ≠ r3. Значит, **при одинаковом сиде конвейер генерации карты
детерминирован**; различие между обычными запусками даёт именно `time(nullptr)`.

### 2.4. Что это значит для дифференциального теста

- Повторить «сейв + N тиков» у C++-headless нельзя: нет входа (сейв) и нет выхода (digest).
- Сравнение состояния возможно только в живой сессии: арбитр каждый ход считает CRC
  пакета 45 и сверяет с CRC клиентов; при равенстве состояние юнитов совпадает
  (в пределах полей CRC, §4).
- Живой прогон `live-smoke-mixed.sh` (2 Rust-клиента + C++-сервер-арбитр, планета 0,
  `min_players=2`, 2 AI-команды) прошёл 5 и 50 ходов: `barrier_timeouts=0`,
  `desyncs=0`, в `ai_log.txt` сервера нет `Desync check failed`; CRC 45 у обоих
  клиентов одинаков (`0x7035`) на всех ходах.
- Автосейв форка пишется на каждом командном ходе и привязан к моменту реального
  времени; внешнего «номера тика» у него нет, поэтому использовать его как эталон
  «после N тиков» без доработок нельзя. Дополнительный опыт (`victory_limit=2`, тот же
  фейковый сид, 300 с): RNG-чекпоинты у двух прогонов идентичны, но автосейвы разные
  (782 831 и 784 343 Б) — за одинаковое реальное время прогоны оказались на разных
  командных ходах. Это не доказывает недетерминизм, но показывает, что автосейв нельзя
  использовать как срез «после N тиков» без номера хода.

**Ответ: в постановке «load save + N ticks → одинаковый digest» — проверить нельзя,
интерфейса нет. Детерминизм симуляции при фиксированном сиде подтверждается косвенно
(RNG-чекпоинты и CRC-барьер), но два запуска по умолчанию не воспроизводимы, так как
сид — системное время.**

## 3. Вопрос 3 — digest C++ против Rust

### 3.1. Что выдаёт Rust

`max-brh.exe --demo <save> --turns N` (`max-brh/crates/max_app/src/main.rs:576-608`)
печатает строку digest'а. Примеры (нативная консольная сборка, `max-original`):

| Прогон | units | `state_digest` | `state_checksum` | `replay_crc` |
|---|---|---|---|---|
| `SAVE1.CAM --turns 5` | 312 | `0xDBD0` | `0x408A` | `0x5561` |
| `SAVE1.CAM --turns 50` | 312 | `0x3797` | `0x408A` | `0x5561` |
| `SAVE3.CAM --turns 50` | 75 | `0xB681` | `0x6A23` | `0x50DC` |
| `SAVE5.CAM --turns 50` | 362 | `0xF7E2` | `0xE3EE` | `0x3D32` |

Формат строки:

```text
ticks=50 turn=1 state=InGame units=312 state_digest=0x3797 \
  state_checksum=0x408A replay_crc=0x5561 snapshot=788740B
```

`state_digest` — CRC16 канонического снапшота, `replay_crc` — CRC канонического
реплей-дайджеста, `state_checksum` — CRC пакета 45 (`max-brh/crates/max_core/src/crc.rs:174-242`).

### 3.2. Что выдаёт C++-headless

Ничего из перечисленного. В форке нет ни снапшот-дайджеста, ни реплей-дайджеста, ни
дампа состояния: поиск по `src/` (4478caa) и по строкам бинаря даёт только
`hex_digest` — SHA256 `MAX.RES`/`PATCHES.RES` (`resource_manager.cpp`:1594-1620) для
хендшейка. Единственная величина состояния — CRC пакета 45, и она **не логируется на
успехе**: сервер считает её, ждёт CRC всех REMOTE-команд и сравнивает
(`remote.cpp` (4478caa):2207-2231). Своё значение арбитр печатает только при
расхождении:

```text
Desync check failed on the server, local <CRC>, peers: team <i> <CRC>, ...
```

### 3.3. Что сопоставимо

| Величина | C++-headless | Rust | Сопоставимо напрямую |
|---|---|---|---|
| `state_digest` (CRC снапшота) | нет | да | нет |
| `replay_digest_crc` | нет | да | нет |
| CRC пакета 45 | `GameManager_GetUnitListChecksum` | `crc::state_checksum` | **да** — тот же алгоритм, поля и порядок |

CRC пакета 45 покрывает юниты четырёх списков (ground cover, mobile land/sea, stationary,
mobile air) всех команд: `team, unit_type, unit_id, grid_x, grid_y, hits, [speed если не
stationary], shots, [storage при типе груза], ammo` (`game_manager.cpp` (4478caa):2358-2385;
`crc.rs`:130-198). Стартовое значение `0xFFFF`.

### 3.4. Живое сравнение на развёрнутом сценарии

Сценарий: новая сетевая партия (планета 0), 2 Rust-клиента (`--turns 5` и `--turns 50`)
и C++-сервер-арбитр, который симулирует те же 2 AI-команды. Команда:

```bash
bash max-brh-cicd/scripts/live-smoke-mixed.sh 50
```

Результаты (логи `build/smoke-logs/mixed-a.log`, `mixed-b.log`, `mixed-ai_log.txt`):

| Прогон | ходов | CRC 45 (оба клиента) | `state_digest` клиента A / B | desync |
|---|---|---|---|---|
| 5 ходов | 5 | `0x7035` | `0x49D1` / `0xD78E` | нет |
| 50 ходов | 50 | `0x7035` | `0x3998` / `0x1707` | нет |

Сервер за 50 ходов не записал ни одного `Desync check failed` и не создал
`desync-turn*.txt`; у обоих клиентов `barrier_timeouts=0`, `desyncs=0`. То есть
**CRC-эквивалентность C++ и Rust на этом сценарии подтверждена**, а «digest»-эквивалентность
проверить нечем.

Замечание: `state_digest` двух клиентов одной партии **различается** (`0x49D1` против
`0xD78E`), хотя CRC 45 и размер снапшота совпадают. Снапшот включает локальные поля
(как минимум `player_team`), поэтому `state_digest` — величина на клиента, а не
межклиентская/межреализационная метрика; для сверки C++↔Rust пригоден только CRC 45.

Слабое место сценария: 14 юнитов, CRC 45 не менялся за 50 ходов (юниты двух «человеческих»
команд простаивают, AI-команды в этом сценарии не меняют отслеживаемые поля). Это проверка
транспорта/барьеров/CRC-конвейера, но не боевого и не экономического расхождения. Для
насыщенного сценария нужны приказы от клиентов (QA-сценарий) или загрузка сейва с боем —
последнее упирается в отсутствие интерфейса (§1) и формат V71 (§1.6).

**Ответ: digest C++ против Rust сравнить нельзя — у C++-headless его нет. Сравнивать можно
CRC пакета 45; на живом сценарии он совпал за 5 и 50 ходов.**

## 4. Вопрос 4 — расхождение и его локализация

### 4.1. Гранулярность CRC

CRC 45 считается на барьере конца хода по всем командам и четырём спискам юнитов, в
фиксированном порядке (сначала по спискам, внутри — по командам red..alien). Не входят:

- золото/экономика команды, исследования, счётчики производства;
- внутреннее состояние ИИ (задачи, цели, веса);
- порядок приказов за пределами полей `order`/`order_state` (в CRC их нет);
- частицы/ракеты (список `ParticleUnits`);
- туман войны и локальные представления (поэтому CRC одинаков у клиентов, а digest — нет).

### 4.2. Что пишется при расхождении

1. **Лог сервера:** `Desync check failed on the server, local <CRC>, peers: team <i> <CRC>, ...`
   (`remote.cpp` (4478caa):2227) — локализует ход и то, какая команда-клиент разошлась;
   на клиенте — `Desync check failed, unit checksums: ...` (там же:2296).
2. **Снимок юнитов** `desync-turn<N>.txt` (`game_manager.cpp` (4478caa):2387-2440):
   каждый пир пишет его на одном и том же барьере; сравнение снимков разных игроков
   показывает, какие юниты разошлись. Формат:

```text
turn <N>, player team <T>, checksum <XXXX>

[<список>]
team type unit_id id grid_x grid_y hits speed shots storage ammo order order_state
...
```

   Списки: `ground cover`, `mobile land and sea`, `stationary`, `mobile air`; поля — те же,
   что в CRC, плюс `order`/`order_state` для контекста.
3. **`save.dbg`** — отладочный сейв, который пишется по завершении desync-аудита
   (`remote.cpp` (4478caa):1717).
4. **Политика арбитра:** пакет 51 «рестарт» и загрузка автосейва слота 10
   (`game_manager.cpp` (4478caa):2498-2509) — партия продолжается с последнего
   командного хода, а не с сейва, из которого её начали.

### 4.3. Искусственный десинк как проверка локализации

В форке есть тест-хук: при `DEBUG.server_desync_test = 1` арбитр один раз портит `hits`
первого мобильного юнита и печатает
`Desync test hook: corrupted unit state on the arbiter (turn <N>)`
(`game_manager.cpp` (4478caa):2454-2472). Это единственный способ увидеть в логе
**собственное CRC арбитра** (`local`) рядом с CRC клиентов и получить
`desync-turn<N>.txt` на живом сценарии. В разведке хук не запускался (требует правки
`settings.json` шаблона сессии); путь проверен по исходнику. `max_session` этот ключ
сейчас не выставляет — его можно задать только в шаблоне `settings.json`.

### 4.4. RNG-чекпоинты — локализация генерации карты

`DBG rng cp <фаза> draws=<N> hash=<16 hex>` печатается на фазах: `seed`,
`before-strategies`, `after-strategies`, `after-cargomaps`, `after-optimize`,
`after-resources`, `supplies-t0..t3`, `after-supplies` (`randomizer.cpp`:52-58;
вызовы — `game_manager.cpp` (4478caa):3243-3285, 4911-4922). Совпадение хешей до
первой расходящейся фазы локализует расхождение генерации карты/ресурсов, а разное
количество `draws` показывает, на каком шаге ветвление разошлось. Пример из §2.3:
одинаковый сид — `after-supplies draws=19253 hash=A7356ACCA2B2AA21`, другой сид —
`draws=19232 hash=40B1DAD137094B09`.

### 4.5. Что не локализуется

- Экономическое расхождение (доход/золото/производство), если оно ещё не изменило
  поля юнитов из CRC: обнаружится позже, когда проявится в `hits`/позиции/патронах;
  в самом CRC экономики нет.
- Расхождение ИИ, не приведшее к изменению отслеживаемых полей юнитов, — невидимо.
- Поле-в-поле diff состояния без снимков `desync-turn` невозможен: дампа состояния нет.
- Порядок приказов/очередей проверяется только косвенно (через `order`/`order_state`
  в снимке и через последующее состояние).

**Ответ: при расхождении локализация есть на уровне «ход + команда + юнит и его поля»
(CRC-барьер + `desync-turn{N}.txt`), а также «фаза генерации карты» (RNG-чекпоинты).
Экономика и внутреннее состояние ИИ напрямую не локализуются — только через вызванные
ими изменения юнитов.**

## 5. Выводы и рекомендации

1. **Дифференциальный тест C++↔Rust возможен только как живая сессия** (C++-сервер-арбитр
   + Rust-клиенты) и только по CRC пакета 45; «сейв + N тиков → digest» у C++-headless
   отсутствует. Для регулярного прогона пригоден `live-smoke-mixed.sh` (5/50 ходов
   проходят, CRC совпадает, desync нет).
2. **Сценарий нужно усилить приказами** (движение/бой/строительство от Rust-клиентов):
   текущий прогон на 14 юнитах не меняет CRC и не проверяет боевые/экономические ветки.
3. **Минимальная диагностическая доработка** (в разведке не выполнялась): в форк
   добавить вывод CRC 45 на каждом барьере и/или канонического digest'а состояния в
   stdout/файл (одна функция + ключ `settings.json`). Это даст прямой diff C++↔Rust и
   снимет зависимость от `server_desync_test`. Исходники форка для патча берутся из
   `4478caa` (`src/`), в текущем дереве их нет.
4. **Для файлового diff'а** нужен либо приём V71 в Rust (`save.rs`), либо запись V70
   из headless (сейчас пишется `LATEST = 71`). До этого автосейвы C++ не читаются Rust,
   а C++-сервер не грузит сейв по интерфейсу.
5. **`server_desync_test=1`** — готовый способ проверить ветку локализации
   (`desync-turn{N}.txt`, `local/peers` CRC, `save.dbg`, рестарт с автосейва); ключ
   стоит прокинуть через `max_session`, чтобы включать его без правки шаблона.

## Приложение A. Команды разведки

```bash
# 1. Прямой запуск headless с данными игры (25 с, затем SIGKILL)
docker run --rm \
  -v "D:/dev/max-port-debug:/src:ro" \
  -v "D:/dev/max-port-debug/bin/linux-server-template:/srv:ro" \
  ubuntu:24.04 bash /src/max-brh-cicd/scripts/run-linux-server-with-data.sh /srv

# 2. Проверка аргументов (GUI/сервер, лишние ключи)
docker run --rm \
  -v "D:/dev/max-port-debug:/src:ro" \
  -v "D:/dev/max-port-debug/bin/linux-server-template:/srv:ro" \
  ubuntu:24.04 bash /src/build/headless-probe/probe.sh

# 3. Одинаковый сид: RNG-чекпоинты
docker run --rm \
  -v "D:/dev/max-port-debug:/src:ro" \
  -v "D:/dev/max-port-debug/bin/linux-server-template:/srv:ro" \
  -v "D:/dev/max-port-debug/build/headless-probe/det:/out" \
  ubuntu:24.04 bash /src/build/headless-probe/probe7-seed.sh

# 4. Rust-digest на сейвах
bin\target-headless\release\max-brh.exe --demo max-original\SAVE1.CAM --turns 50

# 5. Живая смешанная сессия (Rust×2 + C++-арбитр)
bash max-brh-cicd/scripts/live-smoke-mixed.sh 50

# 6. Попытка загрузить автосейв C++ в Rust (ожидаемо V71)
bin\target-headless\release\max-brh.exe --demo build\headless-probe\det\same-s2.mlt --turns 0
```

## Приложение B. Артефакты прогонов

| Путь | Содержимое |
|---|---|
| `build/smoke-logs/mixed-a.log`, `mixed-b.log` | Rust-клиенты: `network: ... turns=50 ... desyncs=0`, `digest: ... state_digest=... state_checksum=0x7035` |
| `build/smoke-logs/mixed-ai_log.txt` | `ai_log.txt` C++-сессии (сеть, пакеты 45/54, без `Desync check failed`) |
| `build/smoke-logs/mixed-horizon.log` | Horizon: `session process started session=<uuid>` |
| `build/headless-probe/det/same-s2.mlt` | автосейв C++ (V71), не читается Rust |
| `build/headless-probe/det/seed-r*.txt` | RNG-чекпоинты четырёх прогонов (`DBG rng cp`) |
| `build/headless-probe/det/stdout-*.txt` | stdout сервера (маркеры, RNG-чекпоинты) |
| `build/headless-probe/probe11.log` | опыт `victory_limit=2`: RNG-чекпоинты и автосейвы двух прогонов |

Временные диагностические скрипты разведки лежат в `build/headless-probe/` (вне
отслеживаемого дерева); в коммит не входят.

## Вывод (решение владельца 2026-11-25)

Diff-корпус через headless «load save + N ticks + dump» НЕВОЗМОЖЕН: headless — сервер
(--server), сейв не загружает, digest'а нет. Альтернатива и текущий релизный слой —
**CRC-45 (state_checksum) в живых multi-client прогонах** (Horizon-smoke, 0 desync);
локализация — desync-turn<N>.txt + RNG-чекпоинты. Расширение покрытия — через живые
прогоны с экономикой/боем, НЕ через headless load+ticks.
