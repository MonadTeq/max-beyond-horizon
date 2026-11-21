# Драйверы и утилиты QA

## Драйверы (исполняемые примеры в `max-brh/crates/max_app/examples/`)

| Драйвер | Назначение | Запуск |
|---|---|---|
| `qa_run.rs` | исполнение .qa-скриптов через реальные обработчики ввода (без SDL) | `cargo run -p max_app --no-default-features --example qa_run -- <script.qa> <out_dir>` |
| `qa_golden_check.rs` | сверка кадров с эталонами, `--update` — перегенерация | `cargo run -p max_app --no-default-features --example qa_golden_check -- <shots_dir> <golden_dir> [--update]` |
| `sp_frames.rs` | генерация эталонных кадров экранов | `cargo run -p max_app --no-default-features --example sp_frames -- <data_dir> <out_dir>` |

## Оконные прогоны (Windows, реальный бинарь + мышь)

| Драйвер | Назначение |
|---|---|
| `max-brh-cicd/scripts/ui-test-sp.ps1` | клики мышью через user32: меню→настройка→партия→скриншоты |
| `max-brh-cicd/scripts/live-smoke.sh` | сетевой smoke: Horizon + headless + 2 клиента × N ходов |

## Утилиты

| Утилита | Назначение |
|---|---|
| `max-brh/tools/rng_vectors/` | C++-генератор эталонных векторов RNG |
| `max-brh/tools/map_vectors/` | C++-генератор эталонов генерации карты |
| `max-brh/tools/sim_vectors/` | C++ мини-реплей для паритета дайджеста |
| `max-brh/tools/lobby_vectors/`, `barrier_vectors/`, `font_vectors/` | байтовые эталоны пакетов/шрифтов |
| `max-original-dasm/tools/*_parser.py` | эталонные Python-парсеры форматов |

## Формат .qa-скрипта (кратко)

```
# комментарий
menu new_game|multiplayer|hot_seat|join
setup done|cancel|random
players <slot> human|computer|none
select X Y | select unit N
move X Y | move grid X Y | move near
end_turn
click X Y | rclick X Y
key ENTER|ESC|T|...
type "text"
wait N          # до N кадров до готовности (или мс?)
tick N
shot <name>     # PNG кадра
assert turn>=3 | assert team=0 | assert units>=14 | assert screen=InGame | assert victory | assert defeat
save <name>     # MXLS
load <file>     # V70 .CAM
exit
```

Полный формат и примеры — в `plans/qa-plan.md` и скриптах `../scripts/`.

## Запись сценариев вручную (`--record`)

Слой 2.5: играете в окне как обычно (GUI открывается без флагов; `--ui` —
совместимый alias), а клиент пишет ваши действия в готовый `.qa`-сценарий для
`qa_run`. Запись начинается при запуске (главное меню) и дописывается при
выходе.

```sh
# из max-brh/; GUI — default: SDL3 3.4.20 собирается из исходников (cmake)
cargo run -p max_app -- \
  --record ../max-brh-qa/scripts/my-run.qa

# системный SDL3 вместо сборки из исходников (явные фичи + --no-default-features)
cargo run -p max_app --no-default-features --features sdl -- \
  --record ../max-brh-qa/scripts/my-run.qa
```

При старте печатается фактический путь (`RECORD: …`), при выходе —
`RECORD: сохранено …`. Существующий файл **не перезаписывается**: `my-run.qa` →
`my-run-1.qa` → `my-run-2.qa` (суффикс выбирается автоматически).

### Что и как пишется

Файл начинается с комментариев: «записан вручную», дата (UTC), билд, размер
кадра, команда реплея. Дальше — обычные команды языка `.qa`:

| Действие игрока | Строка | Почему так |
|---|---|---|
| LMB | `click X Y` | координаты уже в кадре 640×480: `SdlWindow::poll_input` делает обратный пересчёт масштаба окна (`--scale 2/3/…`) |
| RMB | `rclick X Y` | приказ движения/отмена |
| Enter/Esc/Tab/Backspace/стрелки | `key ENTER`, `key ESC`, … | спецклавиши |
| буква вне текстового ввода | `key A` / `key t` | ровно нажатие (регистр сохраняется) |
| печатные символы в открытом текстовом вводе | одна `type "…"` | в сессии ввод открывают **T** или **Enter**, закрывает **Enter** (отправка) или **Esc** (отмена) |
| пауза между событиями | `wait N` | `N` — число кадров приложения; `N = 0` не пишется, длинные паузы режутся до **120** кадров (~2 c при 60 FPS), чтобы реплей не растягивался |

Рекордер видит только `InputEvent`, поэтому высокоуровневые `menu`/`setup`/
`players` он не угадывает: в файле будут `click` по тем же кнопкам. Это
эквивалентно — при желании замените клики на высокоуровневые команды вручную.

Пример «меню → настройка → партия» после записи выглядит так:

```qa
click 485 189      # New Game
wait 2
click 516 440      # Options → Done
...
click 404 242      # выделить юнита
rclick 468 306     # приказ движения
click 426 12       # End Turn
wait 96            # ход компьютера (кадры, а не секунды)
```

Клавиша в сессии пишется отдельным `key`, а набранный текст — одной `type`:
`key ENTER`, `type "hello"`, `key ENTER` (открыть чат → набрать → отправить).
Если ввод закрыли Esc: `key T`, `type "hello"`, `key ESC`.

### Ограничения

* **Backspace внутри `type` не выражается.** Накопленный текст сбрасывается
  отдельной `type`, а Backspace пишется как `key BACKSPACE`: порядок ввода
  сохраняется, но удаление в середине набора — отдельная команда, а не правка
  внутри `type`.
* **Движение мыши не пишется** (`MouseMove` не меняет состояние игры, только
  подсветку) — иначе файл распухал бы на тысячи строк.
* **Клик вне кадра** (чёрные поля при масштабировании) отбрасывается: парсер
  не принимает координаты вне 640×480.
* **Запись рассчитана на кадр 640×480** (значение по умолчанию). С
  `--width/--height` координаты остаются в пикселях фактического кадра, а
  `qa_run` понимает только 640×480 — используйте `--record` со стандартным
  кадром.
* **`Key::Other`** (модификаторы, F-клавиши) и **пробел вне текстового ввода**
  не выражаются командой `key` — пропускаются; внутри `type` пробел пишется.
* **Сеть**: GUI умеет создавать сетевую игру, но `qa_run` не поднимает сеть.
  Воспроизводимы локальные партии (New Game); сетевую запись можно хранить как
  справочный артефакт, но не как регресс-скрипт.
* **Выход при открытом вводе**: файл закончится `type` без закрывающего `key`.
  Допишите `key ENTER`/`key ESC` вручную, если нужен завершённый сценарий.
* **`"` и `#` в набираемом тексте**: `type "…"` сохраняет кавычки/решётку
  внутри внешних кавычек, но сочетание `"…#` может обрезать строку
  комментарием. Для регресс-сценариев лучше не использовать такие сообщения.

### Как добавить asserts и shot после записи

Рекордер намеренно пишет **только действия**: что именно проверять — решает
QA-инженер. Типичные точки — сразу после перехода на экран и перед `End Turn`:

```qa
# в начало файла, до первого клика
shot main-menu
assert screen=MainMenu

# после клика New Game
assert screen=Options
shot options

# ... после Done на Players
assert screen=InGame
assert turn=1
assert team=RED
shot game-start

# перед End Turn / после хода ИИ
assert turn=2
shot game-turn2
```

1. Вставьте `shot NAME` (PNG 640×480 в каталог вывода) и `assert …` в нужные
   места файла; синтаксис проверок — `plans/qa-plan.md`.
2. Прогоните:
   ```sh
   cd max-brh
   cargo run -p max_app --no-default-features --example qa_run -- \
     ../max-brh-qa/scripts/my-run.qa ../max-brh-qa/out/my-run
   ```
   Упавший `assert` печатает фактическое значение и номер строки (`log.txt`).
3. Если кадр должен стать эталоном — добавьте `shot` в `visual-screens.qa`
   (а не в новый файл) и перегенерируйте `golden/` (см. `scripts/README.md`).
4. Сценарий в CI — добавьте прогон в `ci.sh` (стадия 3/3) и
   `.github/workflows/ci.yml`.

Реплей идёт с тем же стартовым состоянием: та же планета/seed/настройки,
что были при записи. Паузы — приближение (`wait` останавливается, когда игра
готова к вводу), поэтому `wait` не гарантирует «столько же кадров», а только
«не дольше N»; на детерминизм экранов и ассертов это не влияет.

### API без SDL

`--record` доступен только в GUI (по умолчанию или с `--ui`), но сам рекордер — обычный модуль
библиотеки (без feature `sdl`), поэтому его можно использовать в своих
headless-обвязках и тестах:

```rust
use max_app::qa::{RecordContext, Recorder};
use max_app::ui::InputEvent;

let mut recorder = Recorder::new("run.qa")?;      // существующий файл → run-1.qa
recorder.begin_frame();                            // раз в кадр приложения
recorder.record(
    InputEvent::MouseClick(485, 189),
    RecordContext { in_session: false, text_open: false },
);
let path = recorder.finish()?;                     // хвостовой type/wait
```

`RecordContext::text_open` — это `App::hud().chat.is_active()` **до** события:
по нему рекордер решает, агрегировать ли символы в `type`.
