# Отчёт: R2 (UnitOrderState 0x0A/0x15) и R4 (Cargo power/life) по оригиналу M.A.X.

Метод: прямой статический анализ плоского образа `maxrun.flat` (base 0x10000) с помощью
capstone 5.0.7. Перед дизассемблированием к образу были применены 32-битные LE-фиксапы
(type 7) — в точности той же логикой, что зашита в `tools/r2_export.sh` (патч
`value = object_base + target_offset`; применено 24281 фиксапов). Без этого ссылки на
строки/адреса неразрешены, и по коду их не видно. Адресная формула:
**runtime = static + 0x160010** (из README).

Существующие экспорты (`functions-named.csv/.md`, `string-matches.csv`) для этих двух
вопросов оказались **недостаточны**: каталог функций построен по «плоскому» образу, где
radare2/Ghidra линейным свипом превратили строковые таблицы данных (диапазон 0x150000+)
в ложные «функции» (например `fcn.00162422` на самом деле строка "< MAXPOPUPBUTTONS").
Реальных данных о смещениях полей структур и о таблицах переходов в экспортах нет —
поэтому ниже приведены доказательства из прямого дизассемблирования.

---

## R2 — состояния приказа 0x0A и 0x15

### Поле состояния приказа (аналог UnitInfo::state / ProcessOrders)

В `UnitsManager_SetNewOrderInt` (static **0x102F96**, runtime **0x262FA6**) подтверждена
раскладка четырёх смежных байтов `UnitInfo`:

| Поле | Смещение | Источник (дизассембл 0x102F96) |
|---|---|---|
| `order` | **0x3D** | `mov [edx+0x3d], al` (новый order) |
| `state` (порядок-состояние) | **0x3E** | `mov [edx+0x3e], al` (новый state) |
| `prior_order` | **0x3F** | `mov [eax+0x3f], dl` (старый order) |
| `prior_state` | **0x40** | `mov [eax+0x40], dl` (старый state) |

Функции-сеттеры состояния (все инлайнят запись в `[unit+0x3E]`):
- `UnitsManager_SetNewOrderInt(unit, order=DL, state=BL)` — **0x102F96**.
- `UnitsManager_SetNewOrder(unit, order=DL, state=BL)` — **0x103053**.
- `UnitsManager_ScaleUnit(unit, state=DL)` — **0x102F0A** (фиксирует order=0x16=AWAIT_SCALING).

`UnitsManager_ProcessOrders` — функция с таблицей переходов по order (0..0x1F) по адресу
**0x102908** (runtime **0x262918**): `cmp [ebp-0x80], 0x1f; ja default; jmp [eax*4 + 0x102908]`.
Хендлеры приказов лежат в 0x101A41…0x1028F6, каждый со своим свитчем по `state`.

### Проверка: присваивается ли 0x0A или 0x15

Проверены **все три пути** задания состояния и **оба пути** проверки:

1. Прямая запись `mov byte [reg+0x3E], imm8` (raw-скан всего кода 0x10000..0x13D469,
   6 вариантов базового регистра): значения — 0x00,0x01,0x02,0x03,0x04,0x05,0x06,0x07,
   0x08,0x09,0x0B,0x0E,0x0F,0x10,0x11,0x12,0x13,0x14,0x16,0x17,0x1B,0x1D,0x1F,0x23,
   0x25,0x28,0x29,0x2A. **0x0A и 0x15 отсутствуют.**
2. Запись через сеттеры (аргумент state в BL/DL перед `call`):
   - SetNewOrderInt: 0x00,0x01,0x04,0x19,0x1E,0x22,0x24,0x26,0x27;
   - SetNewOrder: 0x00,0x01,0x05,0x06,0x0C,0x0D,0x18,0x1A,0x1B,0x1C,0x2A,0x2B,0x2C,0x2D,0x2E;
   - ScaleUnit: только 0x20 (EXPAND) / 0x21 (SHRINK).
   **0x0A и 0x15 отсутствуют.**
3. Свитчи по состоянию внутри ProcessOrders (`cmp byte [ebp-N], imm8` по всему диапазону
   хендлеров 0x101A40..0x1029AF): присутствуют 0x00,0x01,0x02,0x03,0x05,0x06,0x08,0x09,
   0x0B,0x0C,0x0D,0x0E,0x0F,0x10,0x11,0x12,0x13,0x14,0x16,0x17,0x18,0x19,0x1A,0x1B,
   0x1C,0x1D,0x1F,0x20,0x21,0x26,0x27,0x28,0x29,0x2A,0x2B,0x2C,0x2D,0x2E. Отсутствуют
   только 0x04,0x07,0x1E,0x22,0x23,0x24,0x25 — но эти значения **сравниваются напрямую**
   `cmp [reg+0x3E], imm` (проверено отдельно). **0x0A и 0x15 отсутствуют во всех свитчах.**
4. Прямое сравнение `cmp byte [reg+0x3E], imm8`: 0x00,0x01,0x02,0x03,0x04,0x05,0x06,0x07,
   0x0B,0x0D,0x0E,0x19,0x1B,0x1C,0x1D,0x1E,0x1F,0x20,0x22,0x23,0x24,0x2A,0x2E.
   **0x0A и 0x15 отсутствуют.**

### Вывод R2

В оригинале значения состояния приказа **0x0A и 0x15 не используются вообще** — ни разу не
присваиваются (`mov [unit+0x3E]`) и ни разу не проверяются (ни прямым сравнением, ни в
свитчах ProcessOrders). Все остальные значения 0x00…0x2E так или иначе используются, то
есть 0x0A и 0x15 — единственные две «дыры» в перечислении. Это в точности соответствует
C++-порту (`ORDER_STATE_10` = 0x0A, `ORDER_STATE_21` = 0x15, grep=0).

Контекст «дыр» (из enums.hpp / order_log.cpp):
- 0x0A стоит между `FIRE_IN_PROGRESS` (0x09) и `BUILD_IN_PROGRESS` (0x0B);
- 0x15 стоит между `FINISH_TAKE_OFF` (0x14) и `PROGRESS_TRANSFORMING` (0x16).

Никакого скрытого смысла у этих двух значений нет — это просто пропущенные слоты
перечисления в оригинальном исходнике, от которого портирован C++.

---

## R4 — Cargo: семантика полей power и life

### Структура Cargo в оригинале (6 полей int16)

Восстановлена по `Cargo_GetNetProduction` (static **0x12388**, runtime **0x172398**) и
`Cargo_ApplyUnitConsumption` (static **0x12305**, runtime **0x172315**):

| Поле | Смещение | Кто пишет (доказательство) |
|---|---|---|
| `gold` | **0x00** | 0x12388: `add word [cargo+0], ...` от `unit->[0x49]` (gold_mining); 0x12305: `sub word [cargo+0], dx` (gold-расход) |
| `raw` | **0x02** | 0x12388: `add word [cargo+2], ...` от `unit->[0x47]` (raw_mining); 0x12305: `sub word [cargo+2], dx` (raw-расход) |
| `fuel` | **0x04** | 0x12388: `add word [cargo+4], ...` от `unit->[0x48]` (fuel_mining); 0x12305: `sub word [cargo+4], dx` (fuel-расход) |
| `power` | **0x06** | 0x12305: `sub word [cargo+6], dx` ← `Cargo_GetPowerConsumptionRate` |
| `life` | **0x08** | 0x12305: `sub word [cargo+8], dx` ← `Cargo_GetLifeConsumptionRate` |
| `free_capacity` | **0x0A** | 0x12388: `add word [cargo+0xa], ax` где `ax = 0x10 - unit->[0x46]` (total_mining) |

Это **байт-в-байт** совпадает с C++ `struct Cargo { gold, raw, fuel, power, life, free_capacity }`
(cargo.hpp:29-36).

### Где пишутся/читаются power и life

`Cargo_ApplyUnitConsumption` (0x12305) — единственное место, где `power`/`life` получают
значение. Дизассембл (полный):

```
call 0x11E00   ; Cargo_GetRawConsumptionRate(unit_type, mult)   -> eax
sub  [cargo+2], dx      ; raw   -= rate
call 0x11EC5   ; Cargo_GetFuelConsumptionRate(unit_type)        -> eax
sub  [cargo+4], dx      ; fuel  -= rate
call 0x11F2E   ; Cargo_GetPowerConsumptionRate(unit_type)       -> eax
sub  [cargo+6], dx      ; power -= rate      <-- power
call 0x11FF5   ; Cargo_GetLifeConsumptionRate(unit_type)        -> eax
sub  [cargo+8], dx      ; life  -= rate      <-- life
call 0x12073   ; Cargo_GetGoldConsumptionRate(unit_type)        -> eax
sub  [cargo+0], dx      ; gold  -= rate
```

То есть `power` и `life` — это **нормы потребления/производства** (дельты баланса), а не
груз. Геттеры норм (статический адрес → runtime):
- `Cargo_GetRawConsumptionRate` — **0x11E00** → 0x171E10
- `Cargo_GetFuelConsumptionRate` — **0x11EC5** → 0x171ED5
- `Cargo_GetPowerConsumptionRate` — **0x11F2E** → 0x171F3E
- `Cargo_GetLifeConsumptionRate` — **0x11FF5** → 0x172005
- `Cargo_GetGoldConsumptionRate` — **0x12073** → 0x172083

Семантика норм (из C++-оракула, которым я сверял имена): `GetPowerConsumptionRate`
возвращает −6 (POWERSTN) / −1 (POWGEN) / +1 (потребители); `GetLifeConsumptionRate`
возвращает −3 (HABITAT) / +1 (TRAINHAL/GREENHSE/RESEARCH). Отрицательное значение = чистое
**производство**, положительное = **потребление**.

Результат (Cargo с power/life) далее читается в баланс комплекса. В оригинале
`ProductionManager::ValidateAuxilaryIndustry` (static **0x41984**, строка-якорь
`"debugging :P"` из production_manager.cpp:428) сравнивает поля производственного Cargo
и вычитает его из агрегата баланса; строки-якоря перечисляют ровно пять видов ресурса:
`raw material`, `fuel`, `gold`, `power`, `workers` (адреса 0x150AB8, 0x150AC5, 0x150ACA,
0x150ACF, 0x150AD5). Обратите внимание: в оригинале ресурс называется **«workers»**, а не
«life» — в C++ `Cargo.life` отображается на `Complex.workers` (complex.hpp:67,
cargo.cpp:279), и это же имя («workers») используют строки оригинала.

### Входят ли power/life в груз юнита?

Нет. Транспортный груз (`Cargo_GetInventory` / `Cargo_GetCargoCapacity`, cargo.cpp:30-68)
заполняет **только raw/fuel/gold** (по `unit->storage` / атрибуту STORAGE в зависимости от
типа груза). `power` и `life` в этих функциях не участвуют ни в оригинале, ни в C++-порте.

### Вывод R4

`power` (смещение 6) и `life` (смещение 8) — **это поля структуры Cargo** в оригинале, но
по семантике они **не груз транспорта, а дельты баланса комплекса**: пишутся только в
`Cargo_ApplyUnitConsumption` из норм power/life-расхода, читаются в баланс комплекса
(`Complex.power`, `Complex.workers`). Транспортный груз — только `raw/fuel/gold`.
Сужение Rust-порта (`Cargo = { raw, fuel, gold, free_capacity }`, `power/workers` живут в
`Complex`) **семантически точно**: power и life никогда не были физическим грузом, они —
комплексные величины. Единственная оговорка: в C++ поле названо `life`, а в оригинале и в
`Complex` это «workers» — стоит придерживаться термина `workers` (глоссарий).

---

## Что требовало бы живого прогона и зачем (честная оценка)

Для самих выводов R2 и R4 **живой запуск не потребовался** — всё закрыто статически по
`maxrun.flat` с применёнными LE-фиксапами (точные адреса и смещения приведены выше).
Исходные экспорты (`functions-named.csv`) были недостаточны: в них код-область
0x10000..0x13D469 смешана с ложными «функциями» из области данных 0x150000+, и нет
смещений полей структур. Поэтому пришлось дизассемблировать напрямую.

Команды для воспроизведения/верификации (по артефактам `tools/`):

```
# 1) плоский образ (уже есть): python tools/le_flat.py ../max-original/MAXRUN.EXE maxrun.flat
# 2) применение LE-фиксапов (логика из tools/r2_export.sh, embedded-скрипт) — даёт образ с
#    разрешёнными ссылками; на его основе выполнены все сканы capstone.
# 3) живой radare2 (если нужны xref'ы/графы в GUI):
docker run --rm --entrypoint bash -v "D:/dev/max-port-debug:/src" -w /src ubuntu:24.04 \
  /src/max-original-dasm/tools/r2_export.sh
#    (перегенерирует export/r2_functions.json + r2_strings.json с xref'ами по строкам)
# 4) живой Ghidra (если нужен декомпилятор для ProcessOrders 0x101A41..0x1029AF и
#    Cargo-функций 0x12305/0x12388):
docker run --rm --entrypoint bash -v "D:/dev/max-port-debug:/src" -w /src max-ghidra \
  /src/max-original-dasm/tools/ghidra/analyze.sh
```

Рекомендуемые точки для живого просмотра (если потребуется сверка человеком):
- `0x102F96` (SetNewOrderInt) — подтверждение смещений order/state/prior_order/prior_state.
- `0x102908` (таблица переходов ProcessOrders по order) + хендлеры `0x101A41..0x1028F6`.
- `0x12305` (Cargo_ApplyUnitConsumption) — единственная запись power/life.
- `0x12388` (Cargo_GetNetProduction) — полная раскладка Cargo (в т.ч. free_capacity=0x0A).
