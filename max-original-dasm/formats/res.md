# Формат ресурсных файлов M.A.X.: RES / PATCHES.RES

Ревизия: 2026-11-14. Источники: байты реальных файлов (`assets/PATCHES.RES`, `max-original/MAX.RES`),
эталонная реализация загрузчика `max-cpp-port/src/resource_manager.cpp` + `resourcetable.cpp/.hpp`,
`smartfile.cpp/.hpp`, `enums.hpp`, `game_manager.cpp`; якоря RE — `max-original-dasm/01-strings.txt`.

Файлы-примеры:

| Файл | Размер | Назначение |
|---|---|---|
| `assets/PATCHES.RES` | 797 951 (0xC2CFF) | Патч 1.04 (шрифты, логотип, скрипты кампаний) |
| `max-original/MAX.RES` | 17 836 096 (0x1102840) | Базовая база ресурсов игры |
| `max-cpp-port/assets/PATCHES.RES` | 798 150 | Пересобранная портом копия патча (те же 84 записи) |

В дистрибутиве GOG PATCHES.RES лежит рядом с MAX.RES в каталоге игры.

---

## 1. Общая структура файла

Файл — контейнер без сжатия и без выравнивания. Три области:

```
+0x00  res_header (12 байт)                    — заголовок
+0x0C  данные ресурсов (подряд, без паддинга)  — каждый ресурс: ровно data_size байт
+offset таблица res_index[count]                — 16-байтные записи
```

- Данные начинаются **сразу после заголовка**: смещение первой записи в реальных файлах всегда `0x0C` (= `sizeof(res_header)`).
- Таблица лежит по произвольному смещению `header.offset` (в реальных файлах — в конце файла, но это **не** требование формата).
- Количество записей в таблице = `header.size / 16`. В заголовке **нет** счётчика записей и нет поля версии — только сигнатура, смещение и размер таблицы.
- Порядок байтов: **little-endian** (формат DOS/x86). Структуры **упакованы**, без паддинга (в порте это проверено `static_assert(sizeof(...))`).

### 1.1. Заголовок (12 байт), `struct res_header`

```c
struct res_header {
    char    id[4];    // +0x00  сигнатура "RES0" (4 байта, без NUL)
    int32_t offset;   // +0x04  LE: файловое смещение таблицы ресурсов
    int32_t size;     // +0x08  LE: размер таблицы в байтах (кратен 16)
};
```

Реальные значения:

```
PATCHES.RES (первые 16 байт):
00000000  52 45 53 30 AF 27 0C 00 40 05 00 00 00 01 00 00   RES0.'..@.......

  id     = "RES0"              (52 45 53 30)
  offset = 0x000C27AF = 796 591 (AF 27 0C 00, LE)
  size   = 0x00000540 = 1 344  (40 05 00 00, LE)  → 84 записи × 16 байт

MAX.RES (первые 16 байт):
00000000  52 45 53 30 00 B6 0F 01 40 72 00 00 00 01 00 00   RES0....@r......

  id     = "RES0"              (52 45 53 30)
  offset = 0x010FB600 = 17 806 848 (00 B6 0F 01, LE)
  size   = 0x00007240 = 29 248  (40 72 00 00, LE)  → 1 828 записей × 16 байт
```

Байты `00 01 00 00` на +0x0C — это уже начало первого ресурса (в PATCHES.RES первый
ресурс — TrueType-шрифт `FONT_1`, его sfnt-заголовок начинается с версии 0x00010000).

Проверка консистентности:
- PATCHES.RES: `offset + size = 796 591 + 1 344 = 797 935`, до конца файла остаётся **16 байт** «хвоста» (см. §1.4).
- MAX.RES: `offset + size = 17 806 848 + 29 248 = 17 836 096` — таблица заканчивается **точно в конце файла**.

### 1.2. Запись таблицы (16 байт), `struct res_index`

```c
struct res_index {
    char    tag[8];       // +0x00  имя ресурса, до 8 байт; NUL-терминировано,
                          //        после NUL может быть мусор
    int32_t data_offset;  // +0x08  LE: смещение данных ресурса от начала файла
    int32_t data_size;    // +0x0C  LE: размер данных в байтах
};
```

Пример из PATCHES.RES (начало таблицы по 0xC27AF):

```
000C27AF  46 4F 4E 54 5F 31 00 00 0C 00 00 00 98 7C 00 00   FONT_1..........
   → tag "FONT_1", data_offset = 0x0000000C (12), data_size = 0x00007C98 (31 896)
000C27BF  46 4F 4E 54 5F 32 00 00 A4 7C 00 00 58 75 00 00   FONT_2...|..Xu..
   → tag "FONT_2", data_offset = 0x00007CA4 (31 908), data_size = 0x00007558 (30 040)
```

Запись №3 показывает семантику поля `tag`:

```
49 4C 4F 47 4F 00 54 4D ...   → байты "ILOGO" 00 "TM"
```

Тег — это **C-строка** в фиксированном поле 8 байт: сравнение идёт до первого NUL
(порт копирует тег в буфер `char buffer[9] = {0}` и делает
`strncmp(buffer, имя, 9)`), поэтому хвостовой мусор `"TM"` не мешает: `"ILOGO\0TM" == "ILOGO"`.
Парсер должен читать 8 байт и обрезать по первому `0x00`; тег может быть и без NUL
(8 символов впритык — тогда сравнение по всем 8 байтам).

Инварианты реальных файлов (полезно для валидации):
- записи идут в порядке возрастания `data_offset`; данные **плотно прилегают** друг к другу: `offset[i+1] == offset[i] + size[i]`;
- первая запись имеет `data_offset == 12`;
- последняя запись заканчивается ровно на `header.offset` (`offset[last] + size[last] == header.offset`);
- `offset[i] + size[i] <= header.offset` для всех записей.

### 1.3. Разбор таблицы (псевдокод)

```
read header (12 байт)
assert header.id == "RES0"
table_bytes  = header.size
entry_count  = table_bytes / 16
seek(header.offset)
for i in 0..entry_count:
    tag  = read 8 bytes, обрезать по NUL
    off  = read u32 LE
    size = read u32 LE
    запись {tag, off, size}
```

### 1.4. Edge-кейс: 16 «хвостовых» байт в PATCHES.RES

После конца таблицы в PATCHES.RES (смещения 0xC2CEF..0xC2CFE) лежат 16 байт:

```
EC 12 42 02 EC 12 42 02 8C 0E 00 00 53 43 5F 43
```

Это обрывок 85-й записи: тег начинается с `"SC_C"`, а поля смещения/размера содержат
значение `0x024212EC` (= 37 872 364 — похоже на линейный адрес памяти DOS, то есть мусор,
записанный утилитой сборки патча). Оба экземпляра PATCHES.RES (оригинальный и пересобранный
портом, там `0x024C12EC`) заканчиваются одинаковым паттерном `.. .. .. 02 .. .. .. 02 8C 0E 00 00 53 43 5F 43`.
Загрузчик хвост игнорирует, потому что читает ровно `header.size` байт таблицы.
Вывод: **нельзя** полагаться на «таблица до конца файла»; читать строго `offset..offset+size`.

---

## 2. Имена ресурсов: `Resource_ID` ↔ `ResourceManager_ResourceIdList`

Идентификатор ресурса — это **индекс** в статической таблице имён
`ResourceManager_ResourceIdList[]` (файл `max-cpp-port/src/resourcetable.cpp`, порядок
задан перечислением `enum ResourceID : uint16_t` в `resourcetable.hpp`). Формат файла
хранит **только теги-строки** (8 байт); соответствие тег ↔ ID устанавливает загрузчик:
для каждой записи таблицы он ищет её тег в списке имён и получает ID как позицию в списке.

Ключевые константы перечисления (порт):

| Константа | Значение | Смысл |
|---|---|---|
| `UNIT_START` | 0x000 | здания/юниты: `COMMTWR`, `POWERSTN`, …, `WALDO` |
| `UNIT_END` | 0x05D | конец блока юнитов (не ресурс) |
| `MEM_END` | 0x38E | граница кэшируемых ресурсов (§4) |
| `V_START` | 0x38F | голосовые сэмплы `V_M###`/`V_F###` |
| `FXS_STRT` | 0x51C | звуковые эффекты `GEN_*`, `MONOP##`… |
| `FXS_END` | 0x6A0 | конец звуковых эффектов |
| `SCRIPT_S` | 0x78C | скрипты: `SC_L0001` … `SC_D0003`, `INTROSUB`, `SC_SCHEE` |
| `RESOURCE_E` | 0x7DC (2012) | размер таблицы имён в порте |
| `INVALID_ID` | 0xFFFF | «ресурс не найден» |

Примеры: `FONT_1 = 0x788`, `FONT_2 = 0x789`, `FONT_5 = 0x78A`,
`ILOGO = 0x6E7`, `INTROSUB = 0x7DA`, `SC_ATTRI = 0x78E`, `SC_UNITS = 0x795`,
`SNOW_1 = 0x76E`, `FRAMEPIC = 0x6BE`, `INTROFLC = 0x6B4`, `D_DEFALT = 0x2DE`.

### 2.1. Оригинал v1.04: таблица имён короче

В оригинальном `MAXRUN.EXE` (GOG, v1.04 — строка
`"Copyright 1996 Interplay Productions. v1.04"`) таблица имён лежит в данных
(obj.3) единым куском и заканчивается именем **`WORLD_E`**:

| Строка | Адрес (flat) | RVA | Примечание |
|---|---|---|---|
| `resrcmgr.cpp` | 0x001724A4 | 0x001589A4 | имя исходника менеджера ресурсов |
| `DIRnameID < END_MEMORY_RESOURCES` | 0x001724B1 | 0x001589B1 | assert из LoadResource |
| `id > END_MEMORY_RESOURCES` | 0x001724DF | 0x001589DF | assert из ReadResource |
| `patches.res` | 0x001724FC | 0x001589FC | имя файла патча |
| `max.res` | 0x00172508 | 0x00158A08 | имя базового файла |
| `COMMTWR` | 0x0017252E | 0x00158A2E | **начало** таблицы имён |
| `WORLD_E` | 0x001762B7 | 0x0015C7B7 | **конец** таблицы имён (0x787 имён) |
| `RES0` | 0x00185018 | 0x0016B518 | сигнатура заголовка RES |

Внутри этого диапазона имена идут ровно в том же порядке, что и в
`resourcetable.cpp` порта: `COMMTWR, POWERSTN, POWGEN, BARRACKS, …` и далее
`…, ATTRIBS, CLANATRB, SOUNDVOL, TIPS, HELP_ENG, HELP_FRE, HELP_GRM, SNOW_PIC, …,
SNOW_1..6, CRATER_1..6, GREEN_1..6, DESERT_1..6, WORLD_E` (проверено по дампу строк;
`ILOGO` в бинарнике тоже есть, 0x00175D73).

То есть **оригинальная таблица имён содержит 0x787 = 1927 записей и не содержит
имён `FONT_S/FONT_1/FONT_2/FONT_5/FONT_E` и `SC_*`** (их нет нигде в бинарнике), хотя
PATCHES.RES 1.04 хранит именно такие теги. Отсюда рабочая гипотеза: оригинальная игра
адресует «новые» патч-ресурсы **по индексу записи в PATCHES.RES** (порядок записей
в патче фиксирован: `FONT_1, FONT_2, FONT_5, ILOGO, INTROSUB, SC_ATTRI, SC_C0001…SC_UNITS`),
а не по имени; в порте этим записям присвоены имена (`FONT_S…SC_SCHEE`), добавленные
в конец перечисления. Для парсера-экстрактора это несущественно: достаточно ключевать
записи по тегу, дубликаты разрешать «первый выигрывает» (§3).

---

## 3. Выбор базы и разрешение конфликтов (PATCHES.RES поверх MAX.RES)

Порядок загрузки в порте (и, судя по соседству строк в бинарнике, в оригинале):

1. `ResourceManager_PreloadPatches()` — грузит **PATCHES.RES** из базового каталога
   (рядом с исполняемым файлом; в оригинале — каталог игры). Файл **обязателен**:
   при отсутствии игра не стартует.
2. `ResourceManager_LoadMaxResources()` — грузит **MAX.RES** из каталога данных игры
   (в порте — путь из настройки `game_data`, проверяется интерактивной настройкой).

Реализация (`ResourceManager_BuildResourceTable`):

- записи обоих файлов складываются в **один глобальный массив** `ResourceManager_ResItemTable`
  (индекс записи глобальный и монотонный: сначала 84 записи патча, затем 1828 записей MAX.RES);
- у каждой записи сохраняется `res_file_id` — номер файла (0 = PATCHES.RES, 1 = MAX.RES,
  массив `res_file_handle_array[2]` — **максимум два RES-файла**);
- для каждого ID, чей тег найден в таблице имён, регистрируется
  `ResMetaTable[id].res_file_item_index` (глобальный индекс записи) и
  `res_file_id`, **только если ID ещё не зарегистрирован** (`== INVALID_ID`).

Итог: **первый загруженный файл выигрывает**. Поскольку PATCHES.RES грузится раньше,
его записи переопределяют одноимённые записи MAX.RES (например, `ILOGO` в патче
заменяет `ILOGO` из MAX.RES), а записи MAX.RES с тегами, которых нет в патче,
подхватываются из базы. Дубликаты внутри одного файла также разрешаются в пользу
первой записи. Записи с тегами, отсутствующими в таблице имён, молча пропускаются.

---

## 4. Схема загрузки ресурса по ID

Инициализация (`ResourceManager_InitResources`, вызывается из `main()`):

```
InitLocale
InitResourceHandler        → выделить ResMetaTable[RESOURCE_E], всё в INVALID_ID
PreloadPatches             → BuildResourceTable(PATCHES.RES)   // res_file_id 0
InitLanguageManager        // локализованные строки уже доступны
InitSettings, InitSDL, InitGameDataPath
TestMemory, TestDiskSpace
LoadMaxResources           → BuildResourceTable(MAX.RES)       // res_file_id 1
InitUnitAttributes, InitClans, InitUnits, …
InitInternals → InitResManager → BuildColorTables, Cursor_Init
```

```
BuildResourceTable(path):
    fp = fopen(path, "rb")
    res_file_handle_array[ResFileCount] = fp          # открытый FILE* живёт всю игру
    header = read 12 байт
    if header.id != "RES0":  fail
    ResItemTable = realloc(ResItemTable,
                           header.size + ResItemCount * 16)   # дописать к глобальной таблице
    fseek(header.offset); read header.size байт в хвост ResItemTable
    new_count = ResItemCount + header.size / 16
    for i in ResItemCount .. new_count:
        id = имя_в_ID(ResItemTable[i].tag)            # поиск тега в ResourceIdList
        if id != INVALID_ID and ResMetaTable[id].res_file_item_index == INVALID_ID:
            ResMetaTable[id].res_file_item_index = i
            ResMetaTable[id].res_file_id              = ResFileCount
    ResItemCount = new_count; ResFileCount += 1
```

Два режима чтения — граница `MEM_END = 0x38E`:

```
LoadResource(id)                  # id < MEM_END — «резидентные» ресурсы
    if id == INVALID_ID или запись не найдена:  return null
    if ResMetaTable[id].resource_buffer:  return кэш        # кэш на всю игру
    fp = res_file_handle_array[res_file_id]
    fseek(fp, ResItemTable[index].data_offset)
    buf = malloc(data_size); fread(buf, data_size, fp)
    ResMetaTable[id].resource_buffer = buf                  # кэшировать
    return buf

ReadResource(id)                  # id >= MEM_END — «потоковые» ресурсы
    if id < MEM_END или id >= RESOURCE_E:  return null      # защита
    fp = …; fseek(data_offset); buf = malloc(data_size + 1)
    fread(buf, data_size); buf[data_size] = '\0'            # + NUL-терминатор
    return buf                                              # без кэша
```

Вспомогательные:
- `ResourceManager_GetResourceSize(id)` → `data_size` записи;
- `ResourceManager_GetFileOffset(id)` → `data_offset`;
- `ResourceManager_ReadImageHeader(id, &hdr)` — читает первые 777 байт ресурса
  (`ImageBigHeader`: 4×int16 + палитра 3×256×`Color` + `transparent_color`) без выделения всего ресурса;
- `ResourceManager_GetFileHandle(id)` — возвращает `FILE*`, установленный на `data_offset`
  (так читаются «тяжёлые» ресурсы без полной загрузки).

Потребители: изображения грузятся как `ImageSimpleHeader*`
(`width, height, ulx, uly, transparent_color` — 9 байт, дальше пиксельные данные),
спрайты — `LoadResource`, скрипты/атрибуты — `ReadResource` (`SC_*`), миры и кино —
через ресурсы-ссылки (см. §5).

---

## 5. Сжатие и внутренние форматы

**На уровне контейнера сжатия нет.** Ресурсы хранятся как есть: `data_size` байт
«сырых» данных, читаются одним `fread`. В порте нет ни одного вызова распаковки
(grep `lzss|decompress|inflate|unpack` по `max-cpp-port/src` пуст).

Внутренние форматы полезной нагрузки (для понимания, не часть формата RES):

- **Шрифты** (`FONT_1`, `FONT_2`, `FONT_5`): обычные TrueType-шрифты (первый ресурс
  PATCHES.RES начинается с sfnt-заголовка `00 01 00 00 00 0E …`), порт отдаёт их
  FreeType напрямую из буфера (`text.cpp: Text_LoadFontTTF` → `FT_New_Memory_Face`).
- **Изображения**: собственные заголовки M.A.X. — `ImageBigHeader` (777 байт) /
  `ImageSimpleHeader` (9 байт) + пиксели; звук — сэмплы (VOC-подобные); музыка — `.MSC`.
- **Ресурс-ссылка**: некоторые записи содержат просто **ASCIIZ-имя внешнего файла**,
  который грузится с диска. Например, в MAX.RES: `SNOW_1` (11 байт) = `"snow_1.wrl\0"`,
  `INTROFLC` (11 байт) = `"maxint.mve\0"`. Такие ресурсы валидны только как имена файлов
  в каталоге данных игры.
- **SmartFile** (`smartfile.cpp/.hpp`) — **не имеет отношения к RES**. Это формат
  сериализации состояний/сейвов: первые 2 байта файла — LE-версия формата
  (`70` или `71`, `SmartFileFormat::V70/V71/LATEST=71`), дальше индекс объектов
  (uint32 счётчик + `FileObject`-записи). См. `saveload.cpp`.

---

## 6. Проверка парсера

Ожидаемые результаты для эталонных файлов:

**PATCHES.RES (797 951 байт):**
- заголовок: `RES0`, offset = 0xC27AF, size = 0x540 → **84 записи**;
- область данных: 0x0C .. 0xC27AF (последняя запись `SC_UNITS` кончается ровно на offset);
- после таблицы — 16 байт мусора (§1.4).

**MAX.RES (17 836 096 байт):**
- заголовок: `RES0`, offset = 0x10FB600, size = 0x7240 → **1828 записей**;
- первая запись `D_DEFALT` @ 0x0C (8 байт), последняя `CLANATRB` @ 0x10FAE6D (1939 байт,
  кончается ровно на 0x10FB600), таблица — ровно до конца файла.

PowerShell-проверка (выводит и сверяет инварианты):

```powershell
$f='D:\dev\max-port-debug\assets\PATCHES.RES'
$b=[System.IO.File]::ReadAllBytes($f)
function U32($o){ [BitConverter]::ToInt32($b,$o) }
$id=[Text.Encoding]::ASCII.GetString($b,0,4)
$tab=U32(4); $sz=U32(8); $n=$sz/16
"$id off=$tab size=$sz entries=$n"
$prevEnd=12; $ok=$true
for($i=0;$i -lt $n;$i++){
  $o=$tab+$i*16
  $tag=[Text.Encoding]::ASCII.GetString($b,$o,8).TrimEnd([char]0)
  $doff=U32($o+8); $dsz=U32($o+12)
  if($doff -ne $prevEnd -or $doff+$dsz -gt $tab){ $ok=$false }
  $prevEnd=$doff+$dsz
  "{0,2} {1,-9} off=0x{2:X8} size={3,8}" -f $i,$tag,$doff,$dsz
}
"invariants ok: $ok ; last_end=0x$('{0:X}' -f $prevEnd) (table at 0x$('{0:X}' -f $tab))"
```

Ожидаемый вывод первой строки: `RES0 off=796591 size=1344 entries=84`, и
`invariants ok: True; last_end=0xC27AF`.

### Полный список записей PATCHES.RES (84)

| # | Тег | offset (hex) | size | | # | Тег | offset (hex) | size |
|---|---|---|---|---|---|---|---|---|
| 0 | FONT_1 | 0x0000000C | 31 896 | | 42 | SC_S0007 | 0x0007F52F | 5 453 |
| 1 | FONT_2 | 0x00007CA4 | 30 040 | | 43 | SC_S0008 | 0x00080A7C | 6 036 |
| 2 | FONT_5 | 0x0000F1FC | 32 644 | | 44 | SC_S0009 | 0x00082210 | 7 476 |
| 3 | ILOGO | 0x00017180 | 234 467 | | 45 | SC_S0010 | 0x00083F44 | 3 820 |
| 4 | INTROSUB | 0x00050563 | 12 | | 46 | SC_S0011 | 0x00084E30 | 5 254 |
| 5 | SC_ATTRI | 0x0005056F | 21 114 | | 47 | SC_S0012 | 0x000862B6 | 5 519 |
| 6 | SC_C0001 | 0x000557E9 | 16 469 | | 48 | SC_S0013 | 0x00087845 | 5 601 |
| 7 | SC_C0002 | 0x0005983E | 6 399 | | 49 | SC_S0014 | 0x00088E26 | 5 985 |
| 8 | SC_C0003 | 0x0005B13D | 7 018 | | 50 | SC_S0015 | 0x0008A587 | 6 772 |
| 9 | SC_C0004 | 0x0005CCA7 | 5 779 | | 51 | SC_S0016 | 0x0008BFFB | 6 335 |
| 10 | SC_C0005 | 0x0005E33A | 8 104 | | 52 | SC_S0017 | 0x0008D8BA | 5 139 |
| 11 | SC_C0006 | 0x000602E2 | 4 883 | | 53 | SC_S0018 | 0x0008ECCD | 5 057 |
| 12 | SC_C0007 | 0x000615F5 | 5 678 | | 54 | SC_S0019 | 0x0009008E | 4 212 |
| 13 | SC_C0008 | 0x00062C23 | 9 103 | | 55 | SC_S0020 | 0x00091102 | 9 762 |
| 14 | SC_C0009 | 0x00064FB2 | 17 624 | | 56 | SC_S0021 | 0x00093724 | 4 528 |
| 15 | SC_CLANS | 0x0006948A | 8 389 | | 57 | SC_S0022 | 0x000948D4 | 4 867 |
| 16 | SC_CUST1 | 0x0006B54F | 249 | | 58 | SC_S0023 | 0x00095BD7 | 4 140 |
| 17 | SC_D0001 | 0x0006B648 | 412 | | 59 | SC_S0024 | 0x00096C03 | 4 697 |
| 18 | SC_D0002 | 0x0006B7E4 | 345 | | 60 | SC_SCHEA | 0x00097E5C | 1 239 |
| 19 | SC_D0003 | 0x0006B93D | 345 | | 61 | SC_SCHEC | 0x00098333 | 1 805 |
| 20 | SC_HOTS1 | 0x0006BA96 | 251 | | 62 | SC_SCHEE | 0x00098A40 | 1 665 |
| 21 | SC_L0001 | 0x0006BB91 | 2 516 | | 63 | SC_SCHEH | 0x000990C1 | 1 280 |
| 22 | SC_L0002 | 0x0006C565 | 336 | | 64 | SC_SCHEL | 0x000995C1 | 799 |
| 23 | SC_M0001 | 0x0006C6B5 | 3 067 | | 65 | SC_SCHEM | 0x000998E0 | 2 402 |
| 24 | SC_M0002 | 0x0006D2B0 | 3 686 | | 66 | SC_SCHES | 0x0009A242 | 7 927 |
| 25 | SC_M0003 | 0x0006E116 | 3 079 | | 67 | SC_SCHEU | 0x0009C139 | 2 414 |
| 26 | SC_M0004 | 0x0006ED1D | 3 140 | | 68 | SC_T0001 | 0x0009CAA7 | 3 240 |
| 27 | SC_M0005 | 0x0006F961 | 3 138 | | 69 | SC_T0002 | 0x0009D74F | 3 273 |
| 28 | SC_M0006 | 0x000705A3 | 3 146 | | 70 | SC_T0003 | 0x0009E418 | 5 079 |
| 29 | SC_M0007 | 0x000711ED | 3 269 | | 71 | SC_T0004 | 0x0009F7EF | 5 124 |
| 30 | SC_M0008 | 0x00071EB2 | 3 667 | | 72 | SC_T0005 | 0x000A0BF3 | 2 725 |
| 31 | SC_M0009 | 0x00072D05 | 3 650 | | 73 | SC_T0006 | 0x000A1698 | 8 393 |
| 32 | SC_M0010 | 0x00073B47 | 3 847 | | 74 | SC_T0007 | 0x000A3761 | 8 205 |
| 33 | SC_M0011 | 0x00074A4E | 3 710 | | 75 | SC_T0008 | 0x000A576E | 5 854 |
| 34 | SC_M0012 | 0x000758CC | 3 178 | | 76 | SC_T0009 | 0x000A6E4C | 8 246 |
| 35 | SC_MULT1 | 0x00076536 | 254 | | 77 | SC_T0010 | 0x000A8E82 | 7 568 |
| 36 | SC_S0001 | 0x00076634 | 5 797 | | 78 | SC_T0011 | 0x000AAC12 | 10 727 |
| 37 | SC_S0002 | 0x00077CD9 | 6 427 | | 79 | SC_T0012 | 0x000AD5F9 | 6 255 |
| 38 | SC_S0003 | 0x000795F4 | 7 028 | | 80 | SC_T0013 | 0x000AEE68 | 7 473 |
| 39 | SC_S0004 | 0x0007B168 | 4 327 | | 81 | SC_T0014 | 0x000B0B99 | 7 396 |
| 40 | SC_S0005 | 0x0007C24F | 5 536 | | 82 | SC_T0015 | 0x000B287D | 3 899 |
| 41 | SC_S0006 | 0x0007D7EF | 7 488 | | 83 | SC_UNITS | 0x000B37B8 | 61 431 |

Тег записи №3 в файле: байты `49 4C 4F 47 4F 00 54 4D` («ILOGO» + NUL + мусор «TM»).

---

## 7. Замечания для Rust max_assets

1. **Порядок байтов**: всё little-endian. Читать заголовок как `[u8;4]` + `u32` + `u32`
   (или `i32` с проверкой `>= 0`); структуры без паддинга, `#[repr(C, packed)]` не нужен,
   если читать поля по отдельности.
2. **Выравнивание**: в файле его нет. Первый ресурс на `0x0C`, данные идут вплотную;
   смещения произвольные. Никаких align-окружений не применять.
3. **Теги**: 8 байт, не обязательно NUL-терминированы в конце — обрезать по первому
   `0x00` (`split(|&b| b == 0).next()`), сравнивать как C-строки. Тег длиной ровно 8
   допустим. Ключевать записи тегом, а не порядковым номером.
4. **Конфликты**: загружать PATCHES.RES, затем MAX.RES; при повторе тега оставлять
   первую (из патча). Один глобальный индекс записей не нужен — достаточно
   `HashMap<tag, (file, offset, size)>`.
5. **Границы**: валидировать `offset + size <= header.offset` (в идеале) и
   `<= file_len` (минимум); таблицу читать строго `offset..offset+size` — за ней в
   PATCHES.RES лежит 16 байт мусора, а MAX.RES таблица впритык к EOF.
6. **Кэширование**: для совместимости семантики порта: `id < 0x38E` (MEM_END) —
   кэшируемые, `0x38E <= id < 0x7DC` — читаются заново каждый раз; `ReadResource`
   добавляет к буферу NUL-байт. Для простого экстрактора необязательно, но полезно
   задокументировать.
7. **Отсутствующие ID**: ресурса может не быть ни в одном файле — возвращать
   `Option`/`Result`, а не паниковать. `INVALID_ID = 0xFFFF`.
8. **Теги вне таблицы имён**: не отбрасывать при экстракции (все 84 тега PATCHES.RES
   известны порту, но парсер должен быть устойчив к неизвестным тегам — в оригинале
   v1.04 такие записи просто игнорировались при регистрации ID).
9. **Максимум 2 файла** RES в оригинальной схеме (`res_file_handle_array[2]`);
   в общем случае формат допускает сколько угодно — решается порядком загрузки.
10. **Полезная нагрузка непрозрачна**: шрифты — TTF, изображения — M.A.X.-заголовки
    (`ImageBigHeader` 777 байт / `ImageSimpleHeader` 9 байт, LE), скрипты — текстовые
    данные SmartFile-подобного вида, ресурсы-ссылки — ASCIIZ-имя внешнего файла
    (`SNOW_1` → `"snow_1.wrl"`). Не пытаться «разжать» содержимое — сжатия в контейнере нет.
11. **Числа**: `data_size` может быть 0 (теоретически) и маленьким (INTROSUB = 12 байт);
    int32-поля читать как `u32` после проверки на неотрицательность.
12. Для проверки: прогнать §6 на обоих файлах и сравнить с таблицей выше (84 записи
    PATCHES.RES, 1828 записей MAX.RES).

---

## 8. Якоря RE в MAXRUN.EXE (для Ghidra/radare2)

Адреса — как в `01-strings.txt` (линейные в `maxrun.flat`, base 0x10000);
рантайм-маппинг из `README.md`: obj#1 `runtime = flat + 0x160010`,
obj#3 (данные) `runtime = flat + 0x14D610` (база obj#3 0x29D610 − 0x150000).
Например, `"patches.res"` в рантайме ≈ `0x1724FC + 0x14D610 = 0x2BFB0C`.

| Адрес (flat) | Что |
|---|---|
| 0x001724A4 | строка `resrcmgr.cpp` — имя исходника; рядом assert-сообщения ниже |
| 0x001724B1 | `DIRnameID < END_MEMORY_RESOURCES` — assert в функции загрузки по ID (аналог `LoadResource`) |
| 0x001724DF | `id > END_MEMORY_RESOURCES` — assert в функции чтения без кэша (аналог `ReadResource`) |
| 0x001724FC | `patches.res` |
| 0x00172508 | `max.res` |
| 0x0017252E | начало таблицы имён ресурсов (`COMMTWR`) |
| 0x001762B7 | конец таблицы имён (`WORLD_E`) — 0x787 имён |
| 0x00185018 | `RES0` — сигнатура заголовка RES |

Перекрёстные ссылки на `patches.res` / `max.res` / `RES0` из кода obj#1 дают адреса
оригинальных `res_init`-подобных функций и процедуру чтения таблицы (fseek/fread по
`header.offset`/`header.size`).

---

## 9. Источники

- `max-cpp-port/src/resource_manager.cpp` — `res_header`/`res_index`,
  `ResourceManager_BuildResourceTable`, `LoadResource`/`ReadResource`, порядок загрузки файлов.
- `max-cpp-port/src/resourcetable.hpp` / `.cpp` — перечисление `ResourceID` и таблица имён.
- `max-cpp-port/src/resource_manager.hpp` — `ImageBigHeader`/`ImageSimpleHeader`.
- `max-cpp-port/src/smartfile.hpp/.cpp` — формат SmartFile (не RES), версии 70/71.
- `max-cpp-port/src/enums.hpp` — коды ошибок `EXIT_CODE_RES_FILE_*`.
- `max-cpp-port/src/text.cpp`, `world.cpp`, `window_manager.cpp` — потребители ресурсов.
- `assets/PATCHES.RES`, `max-original/MAX.RES` — эталонные файлы (дампы в §1, §6).
- `max-original-dasm/01-strings.txt` — якоря-строки (§8).
