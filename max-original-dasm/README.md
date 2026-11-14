# MAXRUN.EXE — реверс-инжиниринг оригинальной M.A.X.

Восстановлено 2026-11-14 после аварии с удалением (см. историю сессии).

## Структура

- `tools/` — инструменты: LE→flat конвертер, сопоставление строк, Ghidra headless, DOSBox-X отладчик.
- `01-overview.md` — сводка установленных фактов.
- `entry-strategy.md` (tools/dosbox/) — полная карта адресов загрузчика и стратегия поимки.
- `runtime-mapping-analysis.md` (tools/dosbox/) — рантайм-маппинг и формулы адресов.
- `functions.csv` / `string-matches.csv` — каталог функций и розеттский камень строк (перегенерируются).
- `maxrun.flat` — плоский образ (перегенерируется `le_flat.py`).
- `ghidra-project/`, `export/`, `tools/dosbox-x/`, `tools/dos32a_src/` — пересобираемые, в git не хранятся.

## Ключевые установленные факты

- `max-original/MAXRUN.EXE` — MZ + DOS/32A v9.1.2 (DOS/4GW-совместимый, bound) + LE.
- LE-заголовок по смещению 0x6B90; объекты (нумерация с 1): #1 код 0x10000 (0x12D469),
  #2 ISR COM2 0x140000 (16-бит), #3 данные 0x150000 (0x6F170).
- Статический вход игры: 0x10C704 = base(#1) + LE.eip(0xFC704); там `EB 76` через
  баннер WATCOM.
- Рантайм (динамика, DOSBox-X): T (параграф загрузки стаба) = 0x54C; XMS-блок
  DOS/32A = 0x170000 (62.5 МБ). **Рантайм-размещение РЕШЕНО** (декодирован стек
  цикла apply_fixups загрузчика): obj#1 база **0x170010**, obj#2 0x29D490,
  obj#3 0x29D610; **вход игры = 0x26C714** (= 0x170010 + LE.eip 0xFC704),
  формула `runtime = static + 0x160010`; main() = 0x20884E (CS приложения —
  плоский 1038). Стоп 1048:2714 — цикл apply_fixups, а не INT21-клиент.
  Детали: `tools/dosbox/runtime-placement.md`.
- Каталог функций: radare2 (с применением LE-fixups) — 6857 функций, 428 строк
  с xref'ами; `functions-named.csv/.md` — 159 функций с именами файлов C++
  (`tools/r2_export.sh` + `tools/r2_to_csv.py` + `tools/join_functions.py`).
- Проектные документы: `PLAN.md` (v5.0), `TOOLS.md`, `docs/protocol.md`,
  `docs/state-model.md`.
