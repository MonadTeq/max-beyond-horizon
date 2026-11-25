# QA-скрипты клиента M.A.X. (слои 2–4)

Этот каталог — слой 2 (**скриптовый UI-драйвер**), слой 3 (**golden-кадры**) и
их обвязка в CI из `max-brh-docs/qa-plan.md`. Всё работает **in-process, без SDL, окна
и сети**: экраны и игровой цикл `max_app` — чистые функции от `InputEvent`,
поэтому скрипт нажимает те же кнопки, что игрок мышью, и получает те же кадры.

```
max-brh-qa/
├── scripts/sp-full.qa          # одиночная партия: New Game → Options → Planet → Players → 3 хода с ИИ
├── scripts/hs-2.qa             # hot-seat на двоих: окна «Begin turn», 2+ раунда, чередование команд
├── scripts/visual-screens.qa   # проход по всем экранам; кадры = эталоны golden/
├── scripts/victory.qa          # партия до исхода «победа» + экран победы
├── golden/*.png                # эталоны visual-screens.qa (20 кадров, ~2.2 МиБ)
└── scripts/README.md
```

Драйвер живёт в библиотеке (`max-brh/crates/max_app/src/qa/`), а примеры —
тонкие CLI-обёртки (команды — из `max-brh/`, пути к сценариям/эталонам — через
`../max-brh-qa/`):

```sh
cd max-brh

# слой 2: прогнать сценарий (log.txt + PNG в каталог вывода)
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-full.qa qa-out/sp-full

# слой 3: сверить кадры с эталонами
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden
```

Код выхода `qa_run`: **0** — все шаги выполнились, **1** — упал `assert` или
команда (текст в stderr и в `log.txt`), **2** — неверные аргументы CLI.

## Формат `.qa`-скрипта

Одна строка — одна команда. `#` начинает комментарий (внутри `"…"` — не
начинает), пустые строки игнорируются, регистр имён команд не важен.
**Координаты — пиксели кадра 640×480** (окно оригинала), это же окно рисуют
`shot`-кадры, поэтому скрипт одинаково работает на Windows и в Docker.

### Низкоуровневые команды (ровно как ввод игрока)

| Команда | Что делает |
|---|---|
| `click X Y` | левая кнопка в пикселях кадра → `InputEvent::MouseClick` |
| `rclick X Y` | правая кнопка (приказ движения/отмена) |
| `key ENTER\|ESC\|TAB\|BACKSPACE\|UP\|DOWN\|LEFT\|RIGHT\|<символ>` | клавиша |
| `type "text"` | печатные символы по одному (поле имени, чат, лимит победы) |
| `wait N` | до N кадров, пока игра не будет готова к вводу (ход игрока, окно перехода или конец партии) |
| `tick N` | ровно N кадров игрового цикла (`App::local_update`) |
| `shot NAME` | PNG кадра 640×480 в `<out_dir>/NAME.png` |
| `assert …` | проверка состояния (таблица ниже); падение → exit 1 |
| `save NAME` | контейнер локальной партии `MXLS` (`App::write_local_save`) |
| `load NAME` | новая локальная партия из **V70**-сейва (`*.CAM`) |
| `exit` | остановить скрипт (успех) |

### Высокоуровневые команды (читаемые сценарии)

| Команда | Что делает |
|---|---|
| `menu new_game` | `New Game` главного меню → подменю New Game → `Custom Game` → экран Options |
| `menu multiplayer` | `Multiplayer Game` → `Multiplayer Menu` |
| `menu hot_seat` | `Multiplayer Menu` → `Hot Seat Game` → настройка hot-seat |
| `menu join` | `Multiplayer Menu` → `Join Network Game` → список игр (клиент лобби — заглушка QA, 2 детерминированные игры) |
| `setup done` | кнопка `Done` текущего экрана настройки (Options → Planet → Players → старт партии) |
| `setup cancel` | кнопка `Cancel` |
| `setup random` | кнопка `Random` на экране планет |
| `players SLOT human\|computer\|none` | клик по кнопке типа команды в слоте `0..3` (RED/GREEN/BLUE/GRAY) |
| `select X Y` | левый клик + проверка, что выделен юнит **своей** команды |
| `select unit N` | левый клик по N-му **подвижному** юниту игрока (камера центрируется на нём) |
| `move X Y` | правый клик + проверка, что приказ движения принят |
| `move grid X Y` | то же по клетке карты (0..111), если она видна в области карты |
| `move near` | приказ в соседнюю свободную клетку выделенного юнита (перебор 8 направлений) |
| `end_turn` | клик по кнопке `End Turn` + прокрутка кадров, пока игра не вернётся к вводу (ход ИИ/окно перехода) |

Высокоуровневые команды не «обходят» UI: каждая сводится к тому же
`InputEvent`, что отправляет `main.rs`, и затем проверяет результат
(выделение, приказ, смена экрана) — опечатка в координатах падает, а не
молчит.

### Проверки (`assert`)

| Проверка | Смысл |
|---|---|
| `assert turn=3`, `turn>=3`, `turn<=3`, `turn>3`, `turn<3` | `GameState::turn_counter` |
| `assert team=1` / `team=GREEN` | `state.active_turn_team` (0..3 или RED/GREEN/BLUE/GRAY) |
| `assert units>=4` | число юнитов в состоянии |
| `assert screen=InGame` | текущий экран: `MainMenu`, `JoinList`, `CreateForm`, `MultiplayerMenu`, `Options`, `Planet`, `Players`, `LocalSetup`, `GameOver`, `InGame` |
| `assert phase=PlayerTurn` | фаза локальной партии: `PlayerTurn`, `AiTurn`, `Advancing`, `Finished` |
| `assert notice=RED` / `notice=none` | окно «Begin turn» (M-HS) открыто для команды |
| `assert victory` / `assert defeat` | исход партии (`LocalOutcome`) |
| `assert selected` | выделен подвижный юнит игрока |
| `assert digest=0xBEEF` | CRC16 снапшота состояния (`App::digest`) |

При падении в `log.txt` пишется строка шага и **фактическое** значение
(`assert turn=3: фактически turn=1`), прогон останавливается, `qa_run` → exit 1.

## Сценарии и что они проверяют

| Скрипт | Покрытие | Результат |
|---|---|---|
| `sp-full.qa` | главное меню → Options (No Limit, Turn Based) → Planet → Players (`RED` человек, `GREEN` компьютер) → партия: `select unit`, `move near`, 3 × `end_turn` с ходами ИИ, `assert turn>=3`, сохранение `MXLS` | **PASS** (38 шагов, 9 кадров, ~380 кадров цикла) |
| `hs-2.qa` | `Hot Seat Game` → 2 человека → окно «Begin turn» для RED и GREEN, `assert team` чередуется, 3 раунда, подтверждение окна `key ENTER` | **PASS** (50 шагов, 10 кадров) |
| `visual-screens.qa` | 20 снимков по всем экранам (main menu, New Game submenu, Tips window, GameSetupMenu mission list, introduction, credits, options, planet, players, mobilize, landing, game, mine-selected, portrait, survey, winloss, multiplayer, join list); партия заканчивается честно лимитом ходов Duration=1 | **PASS** (451 кадр, 20 кадров = эталоны) |
| `victory.qa` | партия до исхода: лимит ходов 1 → `assert victory`, `assert phase=Finished`, экран победы | **PASS** (26 шагов, 3 кадра) |
| `sp-tb.qa` | Turn Based SP: RED человек + GREEN/BLUE компьютеры, `end_turn` три раза, `assert turn`, `endturn-frames<3000`, сохранение `MXLS` | **PASS** (2719 кадров, 3 кадра) |
| `combat-tb.qa` | Turn Based бой: `spawn` ракетчика и танка, `click enemy` (атака), снаряд `ROCKET`, взрыв по кадрам `image_index`, `assert unit-order`, `endturn-frames<2000` | **PASS** (193 кадра, 8 кадров; до фикса 12 000) |

Про `victory.qa` — **осознанный pending** по элиминации: в языке `.qa` нет
команды «уничтожить юнит», а честный бой с ИИ занимает много ходов, поэтому
победа здесь по условию партии (`Duration`), а не по элиминации. Путь
«элиминация ИИ → победа» покрыт тестом
`crates/max_app/tests/local_sp.rs::eliminating_the_ai_wins_and_the_game_over_screen_renders`
(слой 1). Слой 2 закроет его, когда в языке появится команда урона/разрушения.

## Golden-кадры (слой 3)

Эталоны — `../max-brh-qa/golden/*.png`, ровно те кадры, которые снимает
`visual-screens.qa` (сравнение по имени файла: `golden/NAME.png` ↔
`<out>/NAME.png`). Порог дрейфа (как в `max-brh-docs/qa-plan.md`):

* пиксель «изменился», если `max(|ΔR|, |ΔG|, |ΔB|) > 30`;
* кадр «дрейфнул», если изменилось больше **1 %** пикселей (3072 из 307 200);
* для каждого дрейфнувшего кадра `qa_golden_check` пишет `diff-NAME.png`
  (белый фон, красные — разошедшиеся пиксели) и завершается кодом 1 со
  списком файлов.

Формат хранения: PNG, который пишет `qa::png` (свой кодек без внешних
крейтов). Кадры M.A.X. — 8-битная палитровая графика, поэтому при ≤256 цветах
пишется индексированный PNG (1 байт/пиксель + `PLTE`), иначе truecolor.
Итог: **8 эталонов ≈ 865 КиБ** (сырые кадры были бы ~7 МБ), самый крупный —
`game.png` (~183 КиБ, 257 цветов: палитра хрома + цвет текста статуса).

### Перегенерация эталонов (после осознанного визуального изменения)

```sh
cd max-brh

# 1. снять кадры скриптом
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/visual-screens.qa qa-out/visual-screens

# 2. посмотреть, что изменилось (без --update эталоны не трогаются)
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden
#    → ДРЕЙФ name.png: N пикселей (M %), diff qa-out/visual-screens/diff-name.png

# 3. глазами проверить diff-*.png: изменение ожидаемое?

# 4. зафиксировать новые эталоны и закоммитить их вместе с изменением
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden --update
git add ../max-brh-qa/golden && git commit -m "..."
```

Правило: эталоны **не** обновляются «чтобы CI стал зелёным». Сначала
объясняется дрейф (баг это или осознанное изменение), потом обновляются
эталоны — в том же коммите, что и изменение рендера.

Опции `qa_golden_check`: `--threshold N` (порог по каналу, по умолчанию 30),
`--ratio R` (доля пикселей, по умолчанию 0.01), `--diff-dir DIR`, `--update`.

## CI (слой 4)

`ci.sh` (Docker `rust:1.94.0-bookworm`) и `.github/workflows/ci.yml` содержат
одну и ту же стадию **qa** (`3/3` в `ci.sh`):

```sh
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-full.qa qa-out/sp-full
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/hs-2.qa qa-out/hs-2
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/visual-screens.qa qa-out/visual-screens
cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/victory.qa qa-out/victory
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden
```

`set -e` + exit 1 у `qa_run`/`qa_golden_check` = красный CI. Каталог `qa-out/`
(логи, кадры, diff, `*.mxs`) в git не попадает. В GitHub Actions при падении
стадии логи и кадры уходят артефактом `qa-artifacts`.

## Тесты драйвера (слой 1 для слоя 2)

* `src/qa/script.rs` (unit) — парсер: все команды, операторы, координаты,
  ошибки с номером строки;
* `src/qa/png.rs` (unit) — PNG round-trip, палитровый и truecolor режимы,
  CRC/Adler, компаратор;
* `src/qa/runner.rs` (unit) — конфиг, клиент лобби, имена экранов/фаз;
* `tests/qa_driver.rs` (integration) — все команды парсера, прогон
  `../max-brh-qa/scripts/sp-full.qa` целиком, падение `assert` → `passed=false` (в CLI → exit 1),
  `load` V70-сейва, PNG/golden round-trip.

```sh
cd max-brh && cargo test -p max_app --lib qa:: && cargo test -p max_app --test qa_driver
```

## Ограничения

* **`load` понимает только V70** (`*.CAM`): у контейнера `MXLS` (команда
  `save`) нет десериализатора состояния — см. `max-brh-docs/plan-modes.md`; `save`
  проверяется round-trip'ом в `tests/local_sp.rs` и здесь пишется как артефакт.
* **Сеть не поднимается**: `menu join` использует заглушку клиента лобби
  (`QaMenuClient`, 2 детерминированные игры) — слой 4 (`live-smoke.sh`)
  остаётся отдельным релизным гейтом.
* **PNG-декодер** понимает `stored` и `fixed Huffman` блоки deflate — то, что
  пишет `qa::png`. Динамический Huffman (вывод `libpng`/`optipng`) отклоняется:
  эталоны перегенерируются только через `qa_run`, не сторонними утилитами.
* **Элиминация ИИ** скриптом недостижима (нет команды урона) — `victory.qa`
  побеждает по условию партии, см. выше.
* Ходы ИИ прокручиваются рамками кадров (`App::local_update(0.0)`), поэтому
  прогон детерминирован: два прогона `visual-screens.qa` дают 0 отличающихся
  пикселей (проверено).

## Как добавить сценарий

1. Создать `../max-brh-qa/scripts/NAME.qa` (формат выше), координаты брать из кадров
   (`shot` + просмотр PNG).
2. Прогнать локально: `cargo run -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/NAME.qa qa-out/NAME`.
3. Если сценарий проверяет экран, который должен быть в golden — добавить
   `shot` в `visual-screens.qa` (не в новый файл), перегенерировать эталоны.
4. Добавить прогон в `ci.sh` (стадия 3/3) и `.github/workflows/ci.yml` (job `qa`).
