# Инструментарий и артефакты RE

Восстановлено 2026-11-14. Описывает, чем пользовались и где что лежит.

## Каталоги

| Путь | Содержимое |
|---|---|
| `max-original/` | Оригинальная игра (MAXRUN.EXE + данные); хранится в git целиком |
| `max-original-dasm/` | Артефакты дизассемблирования (этот документ) |
| `max-original-dasm/maxrun.flat` | Плоский образ игры (base 0x10000; `le_flat.py`) |
| `max-original-dasm/01-strings.txt` | 18 372 строки (radare2 `izz`) |
| `max-original-dasm/string-matches.csv` | 2583 совпадения «строка → файлы C++» |
| `max-original-dasm/export/` | Экспорт Ghidra: `functions.csv`, `strings.csv` (пересобираемый, в git нет) |
| `max-original-dasm/ghidra-project/` | Проект Ghidra (пересобираемый, в git нет) |
| `max-original-dasm/tools/` | Скрипты и исходники инструментов |
| `max-horizon-server/` | Horizon (Rust, оркестратор сессий) |

## Инструменты

### Статика
- **radare2** (Docker, ubuntu:24.04): строки и разметка. `r2 -q -c izz MAXRUN.EXE`.
- **Ghidra 11.3.2 headless** (Docker-образ `max-ghidra`, из `tools/ghidra/Dockerfile`):
  импорт `maxrun.flat` как raw x86 (base 0x10000, entry 0x10C704), preScript-ы
  `set_entry.py` + `define_strings.py`, экспорт `export_results.py`.
  Запуск: `docker run --rm --entrypoint bash -v "D:/dev/max-port-debug:/src" -w /src max-ghidra /src/max-original-dasm/tools/ghidra/analyze.sh`.
- **le_flat.py** (Python 3): LE → плоский образ (страницы file = 0x3AB00 +
  (page−1)·0x1000; объекты LE 1-базовые).
- **match_strings.py**: сопоставление строк бинаря с `../src` (C++-порт) —
  розеттский камень.

### Динамика
- **DOSBox-X 2022.09** (форк caiiiycuk/dosbox-x, `tools/dosbox-x/`, сборка
  `tools/dosbox/build.sh`, флаги `--enable-debug=heavy --enable-sdl2`; в git нет).
  Встроенный отладчик: команды BP/BPINT/RUN/GDT/SELINFO/PAGING/EMU MEM/DOS MCBS
  и клавиши F5/F9/F10/F11; вход — DOS-команда `DEBUGBOX` в autoexec.
- **Рантайм-образ `max-dosbox`** (`tools/dosbox/Dockerfile`): python3 + все
  .so-зависимости dosbox-x.
- **re_drive.py** (Python 3, pty.fork + TIOCSWINSZ 50×132): подаёт команды из
  cmds-файла (обычные строки, `WAIT <sec>`, пустая строка = Enter), пишет
  сырой вывод в `dosbox-out.txt`, таймаут 900 с.
- Рабочая цепочка поимки входа: `tools/dosbox/cmds_c.txt` (BP-цепочка от
  MZ-стаба до входа игры 0x20D714; после `GDT` обязателен `Q` — пейджер).

### Исходники для сверки
- **DOS/32A v9.1.2** (`tools/dos32a_src/`, зеркало abbec/dos32a) — экстендер,
  loader.asm/mode.asm/init.asm/sbind.asm.
- **C++-порт M.A.X.** (`../src`) — оригинальные имена функций и строки.

## Ключевые адреса (выверено)

- T (параграф загрузки стаба) = 0x54C (DOSBox-X, memsize=64).
- Статический вход игры: 0x10C704 (LE-объекты 1-базовые; 0x23C704 — ошибка
  индексации).
- Рантайм-вход: **0x20D714** = XMS-база 0x110010 + 0xFC704 (подтверждено
  брейкпоинтом; CS=1048 база 0x20B000, EIP=0x2714).
- Полная карта адресов загрузчика: `tools/dosbox/entry-strategy.md`.

## Процедура перегенерации

```
python tools/le_flat.py ../max-original/MAXRUN.EXE maxrun.flat
# radare2: r2 -q -c izz MAXRUN.EXE > 01-strings.txt
python tools/match_strings.py
# Ghidra: docker run ... max-ghidra ... analyze.sh
```
