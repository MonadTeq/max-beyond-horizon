# QA-скрипты клиента M.A.X. (слои 2–4)

Этот каталог — слой 2 (**скриптовый UI-драйвер**), слой 3 (**golden-кадры**) и
их обвязка в CI из `docs/qa-plan.md`. Всё работает **in-process, без SDL, окна
и сети**: экраны и игровой цикл `max_app` — чистые функции от `InputEvent`,
поэтому скрипт нажимает те же кнопки, что игрок мышью, и получает те же кадры.

```
max-rust-qa/
├── scripts/sp-full.qa          # одиночная партия: New Game → Options → Planet → Players → 3 хода с ИИ
├── scripts/hs-2.qa             # hot-seat на двоих: окна «Begin turn», 2+ раунда, чередование команд
├── scripts/visual-screens.qa   # проход по всем экранам; кадры = эталоны golden/
├── scripts/victory.qa          # партия до исхода «победа» + экран победы
├── scripts/production.qa       # P2: конструктор → BuildMenu → TRAINHAL за 10 ходов → завод → INFANTRY
├── scripts/save-load.qa        # P3: 2 хода → save_v70 → load в свежем App → те же ход/юниты
├── scripts/upgrade.qa          # P6: экран апгрейдов (U) → покупка → Done/Cancel, золото
├── scripts/research.qa         # P6: Research Center → O (питание) → 16 ходов → уровень 1
├── scripts/cargo-full.qa       # P7: полная мобилизация (15 типов, cargo, модернизация, апгрейд)
├── scripts/soak-100.qa         # P4: soak-партия 100 ходов (человек + компьютер), mem/кадры/ассерты
├── scripts/edge-cases.qa       # P4: TC-1106/1111/1112 — спам кликов, юникод, повторный старт
├── scripts/move-input.qa       # TC-0601/0603/0604: выделение кликом → RMB-приказ → End Turn → юнит сдвинулся
├── golden/*.png                # эталоны visual-screens.qa (10 кадров, ~985 КиБ)
├── golden/production/*.png     # эталон BuildMenu (production.qa)
├── golden/upgrade/*.png        # эталоны UpgradeMenu (upgrade.qa)
├── golden/research/*.png       # эталон партии после исследования (research.qa)
├── golden/cargo/*.png          # эталоны CargoMenu: покупка/модернизация/после апгрейда
├── golden/move-input/*.png     # эталоны старта партии/выделения/предпросмотра/хода 2 (move-input.qa)
└── scripts/README.md
```

Драйвер живёт в библиотеке (`max-rust/crates/max_app/src/qa/`), а примеры —
тонкие CLI-обёртки (команды — из `max-rust/`, пути к сценариям/эталонам — через
`../max-rust-qa/`):

```sh
cd max-rust

# слой 2: прогнать сценарий (log.txt + PNG в каталог вывода)
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/sp-full.qa qa-out/sp-full

# слой 3: сверить кадры с эталонами
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-rust-qa/golden
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
| `mem` | текущий RSS процесса (и пик) в лог шага — P4 soak/TC-1108; `# mem start/end` пишутся автоматически |
| `shot NAME` | PNG кадра 640×480 в `<out_dir>/NAME.png` |
| `assert …` | проверка состояния (таблица ниже); падение → exit 1 |
| `save NAME` | контейнер локальной партии `MXLS` (`App::write_local_save`) |
| `save_v70 NAME` | **настоящий V70-сейв** текущей партии (`GameState::to_save_bytes`, P3): читается `SaveFile::parse`, C++-портом и командой `load` |
| `load NAME` | новая локальная партия из **V70**-сейва (`*.CAM`/`*.dta` — в т.ч. записанного `save_v70`) |
| `exit` | остановить скрипт (успех) |

### Высокоуровневые команды (читаемые сценарии)

| Команда | Что делает |
|---|---|
| `menu new_game` | `New Game` главного меню → экран Options |
| `menu multiplayer` | `Multiplayer Game` → `Multiplayer Menu` |
| `menu hot_seat` | `Multiplayer Menu` → `Hot Seat Game` → настройка hot-seat |
| `menu join` | `Multiplayer Menu` → `Join Network Game` → список игр (клиент лобби — заглушка QA, 2 детерминированные игры) |
| `setup done` | кнопка `Done` текущего экрана настройки (Options → Planet → Players → открыть фазы старта) |
| `setup cancel` | кнопка `Cancel` |
| `setup random` | кнопка `Random` на экране планет |
| `players SLOT human\|computer\|none` | клик по кнопке типа команды в слоте `0..3` (RED/GREEN/BLUE/GRAY) |
| `mobilize buy` | кнопка `Buy` экрана мобилизации (покупает выбранную строку) |
| `mobilize buy N` | выбрать строку `N` списка `CargoMenu::unit_types1` (0..14) и купить её |
| `mobilize delete` | кнопка `Delete` (продать выбранный купленный юнит) |
| `mobilize step N` | сдвинуть выбор по списку (N > 0 — вниз, N < 0 — вверх) |
| `mobilize done` | кнопка `Done` мобилизации → выбор точки высадки |
| `mobilize cancel` | кнопка `Cancel` мобилизации → назад на `Players` |
| `landing click X Y` | клик по кадру 640×480: двигает курсор высадки |
| `landing grid X Y` | клик по клетке карты 112×112 (4 px/клетка) |
| `landing done` | `Enter` — подтвердить точку (`GameManager_IsValidStartingPosition`) |
| `landing cancel` | `Esc` — отменить выбор → назад на `Players` |
| `select X Y` | левый клик + проверка, что выделен юнит **своей** команды |
| `select unit N` | левый клик по N-му **подвижному** юниту игрока (камера центрируется на нём) |
| `select last` | левый клик по самому новому юниту игрока (в т.ч. стационарный завод) |
| `select type NAME` | клик по юниту игрока с именем из `BuildMenu` («Training Hall», «Tank», …) |
| `build open` | открыть `BuildMenu` для выделенного строителя/завода (клавиша `B`) |
| `build select N` | выбрать строку списка доступного |
| `build add` / `build delete` | `Build` (в очередь) / `Delete` завода |
| `build rate 1\|2\|4` | кнопки скорости 1×/2×/4× |
| `build repeat` | кнопка `Repeat` завода |
| `build step N` / `build queue step N` | стрелки списка / очереди (N > 0 — вниз) |
| `build done` | `Done`/Enter — заказ (`PayloadP4`, пакет 08) |
| `build cancel` | `Cancel`/Esc — закрыть без заказа |
| `upgrade open` | открыть экран апгрейдов (P6, клавиша `U`) для выделенного юнита |
| `upgrade select N` | выбрать строку списка типов |
| `upgrade row N up\|down` | нажать стрелку `UPGRGT`/`UPGLFT` строки N (цена/шаг — C++) |
| `upgrade done` | `Done`/Enter — `CommitUpgradeChanges` (золото списывается) |
| `upgrade cancel` | `Cancel`/Esc — закрыть без изменений |
| `mobilize mode buy\|upgrade` | переключатель `CRGTOG` (542,446) — список покупки/модернизации |
| `mobilize filter ground\|air\|sea\|building\|combat` | фильтр домена `SELGRD/SELAIR/SELSEA/SELBLD/SELCBT` |
| `mobilize select N` | строка N списка Available (прокручивает список стрелками, как игрок) |
| `mobilize loadout N` | строка N списка Purchased (бесплатная загрузка + покупки) |
| `mobilize row N up\|down` | стрелка `UPGRGT`/`UPGLFT` строки апгрейда N |
| `mobilize cargo N` | cargo-слайдер: N блоков по 5 единиц (1 кредит за блок) |
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
| `assert screen=InGame` | текущий экран: `MainMenu`, `JoinList`, `CreateForm`, `MultiplayerMenu`, `Options`, `Planet`, `Players`, `Clan`, `LocalSetup`, `Mobilize`, `Landing`, `GameOver`, `InGame` |
| `assert phase=PlayerTurn` | фаза локальной партии: `PlayerTurn`, `AiTurn`, `Advancing`, `Finished` |
| `assert notice=RED` / `notice=none` | окно «Begin turn» (M-HS) открыто для команды |
| `assert victory` / `assert defeat` | исход партии (`LocalOutcome`) |
| `assert selected` | выделен подвижный юнит игрока |
| `assert preview` | у выделенного юнита есть непустой предпросмотр пути (зелёный маршрут после RMB) |
| `assert unit-pos=X,Y` | выделенный юнит стоит в клетке `(X, Y)` |
| `assert log=ТЕКСТ` | в журнале HUD есть строка с `ТЕКСТ` («Move: N steps to (X, Y)») |
| `assert digest=0xBEEF` | CRC16 снапшота состояния (`App::digest`) |
| `assert build=open` / `build=closed` | открыт ли `BuildMenu` (P2) |
| `assert upgrade=open` / `upgrade=closed` | открыт ли экран апгрейдов (P6) |
| `assert gold>=N` (и `=`, `<=`, `>`, `<`) | золото локальной команды |
| `assert research attack>=1` | уровень темы исследований (`attack|shots|range|armor|hits|speed|scan|cost`) |
| `assert cargo-mode=buy\|upgrade` | режим списка `CargoMenu` (P7) |
| `assert cargo-types>=N` | длина списка типов (15 в покупке, 62 в модернизации по умолчанию) |
| `assert cargo-credits>=N` | остаток кредитов в `CargoMenu` |
| `assert unit=NAME` | у игрока есть юнит с именем из `BuildMenu` («Training Hall», «Infantry», …) |

При падении в `log.txt` пишется строка шага и **фактическое** значение
(`assert turn=3: фактически turn=1`), прогон останавливается, `qa_run` → exit 1.

## Сценарии и что они проверяют

| Скрипт | Покрытие | Результат |
|---|---|---|
| `sp-full.qa` | главное меню → Options (No Limit, Turn Based) → Planet → Players (`RED` человек, `GREEN` компьютер) → экран клана (`clan`) → мобилизация (`mobilize buy` ×2 + Done) → выбор точки высадки (`landing grid 30 22`, не совпадает со стартовой четвертью, + Done) → партия: `select unit`, `move near`, 3 × `end_turn` с ходами ИИ, `assert turn>=3`, сохранение `MXLS` | **PASS** (55 шагов, 14 кадров, ~380 кадров цикла) |
| `hs-2.qa` | `Hot Seat Game` → 2 человека → фазы старта для GREEN и RED по очереди (мобилизация + высадка) → окно «Begin turn» для RED и GREEN, `assert team` чередуется, 3 раунда, подтверждение окна `key ENTER` | **PASS** (64 шага, 14 кадров) |
| `visual-screens.qa` | 10 экранов только с `shot` (main menu, options, planet, players, mobilize, landing, game, winloss, multiplayer, join list); партия заканчивается честно лимитом ходов Duration=1 | **PASS** (45 шагов, 10 кадров = эталоны) |
| `victory.qa` | партия до исхода: лимит ходов 1 → фазы старта → `assert victory`, `assert phase=Finished`, экран победы | **PASS** (29 шагов, 3 кадра) |
| `production.qa` | P2: главное меню → Options → Planet → Players (`RED` человек, `GREEN` компьютер) → мобилизация без покупок → высадка → `select unit 0` (конструктор) → `B`/`build open` → TRAINHAL (10 ходов) → здание появилось (`assert unit=Training Hall`) → `select type Training Hall` → `build open` завода → INFANTRY (2 хода) → `assert unit=Infantry`; кадры `buildmenu`/`buildmenu-factory` | **PASS** (golden `production/buildmenu.png`) |
| `save-load.qa` | P3: локальная партия → два `end_turn` (ход ИИ и новый раунд) → `save_v70 turn2.dta` (настоящий V70) → `load turn2.dta` в свежем состоянии → `assert turn=3`, `assert units>=10` → `end_turn` после загрузки; кадры `save-menu`/`after-load` | **PASS** |
| `upgrade.qa` | P6: локальная партия → `select type Constructor` → `upgrade open` (клавиша `U`) → `upgrade row 0 up` → `Done`: золото 150 → 142, `current_values` типа изменены; повтор с `Cancel` не меняет золото; кадры `upgrade-menu`/`upgrade-done` | **PASS** (golden `upgrade/`) |
| `research.qa` | P6: конструктор строит Research Center (12 ходов) → `select type` + `key o` (питание, пакет 08/P1) → 16 × `end_turn` → `assert research attack>=1`; кадр `research-done` | **PASS** (golden `research/`) |
| `soak-100.qa` | P4 soak: Options (Clueless, No Limit, Turn Based, Duration 200) → мобилизация без покупок → высадка → `select unit 0`/`move near` в начале и на 41-м ходу → 100 × `end_turn` (ходы ИИ) → `assert turn=21/41/61/81/101`, `assert screen=InGame`, `shot` и `mem` каждые 20 ходов, `save_v70 soak-turn100.dta`; в `log.txt` — `# mem start/end` и (при `MAX_TICK_PROFILE=1`) строка `PROFILE` | **PASS** (100 ходов, 12 208 кадров, 7 PNG; V70-сейв 157 921 Б; RSS 54 696 → 60 944 КиБ) |
| `edge-cases.qa` | P4 TC-1106/1111/1112: 50 кликов по пустым областям главного меню + 50 по карте (без паник), не-ASCII `type` в чате и в поле имени, выход из партии → New Game → Cancel → полный второй старт до `turn=2`; кадры `edge-menu-spam`/`edge-game-spam`/`edge-unicode-*`/`edge-repeat-start`/`edge-second-game` | **PASS** |
| `cargo-full.qa` | P7: мобилизация целиком — `assert cargo-types=15`, покупка Gold Truck (14-я строка, прокрутка списка), Mine Layer и Constructor, cargo-слайдер (+20 единиц), `CRGTOG` → список модернизации (62 типа), фильтры домена, апгрейд armor Constructor'а, `Done` → `assert gold<150`; кадры `cargo-buy`/`cargo-upgrade`/`cargo-upgraded` | **PASS** (golden `cargo/`) |
| `move-input.qa` | TC-0601/0603/0604 сквозь весь путь **кликами мыши**: меню → Options → Planet → Players → мобилизация (`mobilize done`) → высадка (`key ENTER`, `assert screen=InGame`) → `assert units>=7` (стартовые юниты в партии) → клик по своему Surveyor'у (`assert selected`, `assert unit-pos=23,21`) → RMB на соседнюю клетку (`assert preview`, `assert log=Move:`) → `end_turn` → повторный клик по новой клетке (`assert unit-pos=24,21`); кадры `game-start`/`game-selected`/`move-preview`/`game-turn2`/`game-after` | **PASS** (134 кадра, 5 PNG = эталоны `move-input/`; регресс «юнит не двигается» — камера на шахте, а не на плите) |

Про `victory.qa` — **осознанный pending** по элиминации: в языке `.qa` нет
команды «уничтожить юнит», а честный бой с ИИ занимает много ходов, поэтому
победа здесь по условию партии (`Duration`), а не по элиминации. Путь
«элиминация ИИ → победа» покрыт тестом
`crates/max_app/tests/local_sp.rs::eliminating_the_ai_wins_and_the_game_over_screen_renders`
(слой 1). Слой 2 закроет его, когда в языке появится команда урона/разрушения.

## P4: soak и производительность (`soak-100.qa`, `soak_headless`)

Два независимых прогона P4 (`docs/development/PLAN-production.md`):

1. **QA-сценарий** `soak-100.qa` — тот же UI-путь игрока (100 × `end_turn` с
   ходами ИИ), поэтому он ловит паники/десинк экранов и заодно пишет
   `# mem start`/`# mem end` в `log.txt` (RSS процесса до/после), а команда
   `mem` — промежуточные замеры каждые 20 ходов.
2. **Headless-пример** `soak_headless` — 120 ходов ИИ×ИИ без UI:

   ```sh
   cd max-rust
   cargo run -q -p max_app --no-default-features --example soak_headless -- \
       --turns 120 --out qa-out/soak-headless
   # qa-out/soak-headless/soak.txt: строка на ход (wall/frames/units/rss),
   # SOAK RESULT, PROFILE, MEMORY
   ```

   Опции: `--turns`, `--out`, `--ai 1..3`, `--world`, `--seed`, `--opponent`,
   `--dt-ms`, `--data-dir/--assets-dir`, `--quiet`. Сложность по умолчанию 0
   (Clueless), условие победы Duration 200 — иначе партия закрылась бы
   лимитом раньше soak-окна.

**Профиль тика.** Переменная `MAX_TICK_PROFILE=1` включает замеры в
`max_app::profile`: время каждого `App::local_update` (кадр) и каждого
`GameState::finish_turn` (закрытие раунда). Итоговая строка пишется в
`log.txt` QA и в `soak.txt`:

```
PROFILE: local_update n=… avg=…us max=…us over_16.67ms=… (…%);
         finish_turn n=… avg=…us max=… last=…@turn100 max_at=turn…
```

Бюджет кадра 60 Гц — 16 666 667 нс (`profile::FRAME_BUDGET_NS`); счётчик
`over_16.67ms` показывает, сколько кадров в него не уложились. Замер не меняет
порядок/арифметику симуляции: `Instant::now()` стоит вокруг тех же вызовов,
CRC/снапшоты не затрагиваются.

**Память.** `max_app::mem` читает RSS и пик: Linux — `/proc/self/status`
(`VmRSS`/`VmHWM`), Windows — `GetProcessMemoryInfo`; на прочих ОС
`rss=unavailable`. `soak_headless` печатает рост RSS и считает его стабильным
при `growth ≤ 64 МиБ` (порог от грубых утечек; реальный рост за 120 ходов —
единицы МиБ). Цифры прогона P4 (2026-11-16, `rust:1.94.0-bookworm`, debug):

| Прогон | Ходы | Кадры | Время | RSS старт → конец | Профиль кадра |
|---|---|---|---|---|---|
| `soak-100.qa` | 100/100 | 12 208 | ≈3.5 c (12208 × avg 289 µs) | 54 696 → 60 944 КиБ (+6.1 МиБ, плато после 20-го хода) | avg 289 µs, max 15.69 мс, over 16.67 мс = 0 |
| `soak_headless` (debug) | 120/120 | 362 502 | 21.8 c | 53 548 → 53 708 КиБ (+160 КиБ, peak 53 884) | avg 59.9 µs; 1 выброс 121.9 мс (планировщик, 1 кадр из 362 502) |
| `soak_headless` (`--release`) | 120/120 | 362 502 | 2.47 c | 51 544 → 51 824 КиБ (+280 КиБ) | avg **6.76 µs**, max 5.21 мс, over 16.67 мс = **0** |

`finish_turn` на 100-м ходу QA-партии — 23.7 µs (avg 23.6 µs, max 56.6 µs
на 48-м ходу); в релизной сборке headless — avg 58 µs, max 5.19 мс (24-й ход),
на 120-м ходу 9.4 µs. Память QA-партии растёт только на прогреве аллокатора в
первые 20 ходов, дальше — плато, то есть линейного роста нет. Бюджет кадра
60 Гц (16.67 мс) в релизной сборке не превышен ни на одном кадре из 362 502.

**Оптимизация без изменения детерминизма.** `App::update_visual_units`
(интерполяция кадра) больше не собирает `HashMap` заново каждый кадр, а
обновляет её на месте (`retain` + `insert`); визуальные позиции не входят в
CRC/снапшот, порядок симуляции не меняется. Симуляция и `finish_turn` не
тронуты (только замеры).

## Golden-кадры (слой 3)

Эталоны — `../max-rust-qa/golden/*.png`, ровно те кадры, которые снимает
`visual-screens.qa` (сравнение по имени файла: `golden/NAME.png` ↔
`<out>/NAME.png`). Порог дрейфа (как в `docs/qa-plan.md`):

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
cd max-rust

# 1. снять кадры скриптом
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/visual-screens.qa qa-out/visual-screens

# 2. посмотреть, что изменилось (без --update эталоны не трогаются)
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-rust-qa/golden
#    → ДРЕЙФ name.png: N пикселей (M %), diff qa-out/visual-screens/diff-name.png

# 3. глазами проверить diff-*.png: изменение ожидаемое?

# 4. зафиксировать новые эталоны и закоммитить их вместе с изменением
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-rust-qa/golden --update
git add ../max-rust-qa/golden && git commit -m "..."
```

Правило: эталоны **не** обновляются «чтобы CI стал зелёным». Сначала
объясняется дрейф (баг это или осознанное изменение), потом обновляются
эталоны — в том же коммите, что и изменение рендера.

Опции `qa_golden_check`: `--threshold N` (порог по каналу, по умолчанию 30),
`--ratio R` (доля пикселей, по умолчанию 0.01), `--diff-dir DIR`, `--update`.

## CI (слой 4)

`ci.sh` (Docker `rust:1.94.0-bookworm`) содержит стадии `1/4` (cargo
check/test), `2/4` (Horizon), `3/4` — QA-сценарии и golden, `4/4` — soak:

```sh
# 3/4: сценарии + golden
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/sp-full.qa qa-out/sp-full
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/hs-2.qa qa-out/hs-2
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/visual-screens.qa qa-out/visual-screens
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/victory.qa qa-out/victory
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/production.qa qa-out/production
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/save-load.qa qa-out/save-load
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/edge-cases.qa qa-out/edge-cases
cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/move-input.qa qa-out/move-input
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-rust-qa/golden
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/production ../max-rust-qa/golden/production
cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/move-input ../max-rust-qa/golden/move-input

# 4/4: soak (100 ходов QA + 120 ходов headless; профиль и память в лог)
MAX_TICK_PROFILE=1 cargo run -q -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/soak-100.qa qa-out/soak-100
cargo run -q -p max_app --no-default-features --example soak_headless -- --turns 120 --out qa-out/soak-headless
```

`set -e` + exit 1 у `qa_run`/`soak_headless`/`qa_golden_check` = красный CI.
Каталог `qa-out/` (логи, кадры, diff, `*.mxs`, `soak.txt`) в git не попадает.
В GitHub Actions `soak` — отдельный job (`timeout-minutes: 45`, не на PR); при
падении стадии логи и кадры уходят артефактом `qa-artifacts`.

## Тесты драйвера (слой 1 для слоя 2)

* `src/qa/script.rs` (unit) — парсер: все команды, операторы, координаты,
  ошибки с номером строки;
* `src/qa/png.rs` (unit) — PNG round-trip, палитровый и truecolor режимы,
  CRC/Adler, компаратор;
* `src/qa/runner.rs` (unit) — конфиг, клиент лобби, имена экранов/фаз;
* `tests/qa_driver.rs` (integration) — все команды парсера, прогон
  `../max-rust-qa/scripts/sp-full.qa` целиком, падение `assert` → `passed=false` (в CLI → exit 1),
  `load` V70-сейва, PNG/golden round-trip.

```sh
cd max-rust && cargo test -p max_app --lib qa:: && cargo test -p max_app --test qa_driver
```

## Ограничения

* **`load` понимает только V70** (`*.CAM`): у контейнера `MXLS` (команда
  `save`) нет десериализатора состояния — см. `docs/plan-modes.md`; `save`
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

1. Создать `../max-rust-qa/scripts/NAME.qa` (формат выше), координаты брать из кадров
   (`shot` + просмотр PNG).
2. Прогнать локально: `cargo run -p max_app --no-default-features --example qa_run -- ../max-rust-qa/scripts/NAME.qa qa-out/NAME`.
3. Если сценарий проверяет экран, который должен быть в golden — добавить
   `shot` в `visual-screens.qa` (не в новый файл), перегенерировать эталоны.
4. Добавить прогон в `ci.sh` (стадия 3/4 — обычные сценарии, 4/4 — soak) и
   `.github/workflows/ci.yml` (job `qa` / `soak`).
