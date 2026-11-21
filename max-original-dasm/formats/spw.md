# Формат спрайтов M.A.X. — мультикадровые спрайты в MAX.RES (и что такое «.SPW» на самом деле)

Ревизия: 2026-11-14. Источники: код-оракул `max-cpp-port/src/` (`gfx.cpp/.hpp`,
`resource_manager.cpp/.hpp`, `unitinfo.cpp`, `units.cpp`, `world.cpp`, `color.h/.cpp`,
`enums.hpp`, `resourcetable.cpp/.hpp`), байты реальных файлов
(`max-original/MAX.RES`, 272 файла `*.SPW` из `max-original/`), сайт проекта
(`max-cpp-port-docs/`, в т.ч. `scripting.md`), якоря RE (`max-original-dasm/01-strings.txt`,
`functions-named.md`), `max-original-dasm/formats/res.md`.

---

## 0. TL;DR

| Вопрос | Ответ |
|---|---|
| Что такое `*.SPW` в каталоге игры? | **Звуки**: RIFF/WAVE, PCM 22050 Гц, моно, 16 бит. **Не спрайты** (§1). |
| Где лежат спрайты? | **Встроены в MAX.RES** как ресурсы с тегами `COMMTWR`…`WALDO` (юниты) и `S_*` (тени) (§2). |
| Контейнер спрайта | `u16 image_count` + `image_count`×`u32` смещений кадров; кадры подряд (§3). |
| Заголовок кадра | 4×`i16` (width, height, hotx, hoty) + `height`×`u32` смещений строк (§3). |
| Кодирование пикселей | Построчный RLE: прогоны `{u8 skip, u8 count, count байт палитровых индексов}`, терминатор строки `0xFF`. Прозрачность = пропуск пикселей (структурная, альфа-канала нет). Тени — только пары `{skip, count}` без байтов пикселей (§4). |
| Палитра | 256 цветов, 3 байта RGB (6 бит/компонент) на запись; индексы 32–39 и 48–63 — «командные цвета», перекрашиваются LUT в момент отрисовки (§6). |
| Файлов-примеров | `TANK` @ 0xCEA3F3 (35211 б), `S_TANK` @ 0xDE4137 (3604 б), `COMMTWR` @ 0x11803F (14153 б) в `max-original/MAX.RES`; 272 файла `.SPW` — WAV-звуки (§8). |

---

## 1. Терминологическое прояснение: `.SPW` ≠ спрайты

В каталоге игры (GOG-сборка `max-original/`, v1.02 + патч 1.04) лежат 272 файла
`*.SPW` (суммарно 25 737 626 байт). **Все 272 — RIFF/WAVE**, не спрайты:

```
TANK1.SPW   (58 442): 52 49 46 46 42 E4 00 00 57 41 56 45 66 6D 74 20  "RIFF....WAVEfmt "
                      10 00 00 00 01 00 01 00 22 56 00 00 44 AC 00 00  ....1...1."V..D..
                      02 00 10 00 64 61 74 61 ...                       ....data...
    → fmt: PCM (1), 1 канал, 22050 Гц, byte rate 44100, block align 2, 16 бит;
      data-чанк + smpl-чанк (60 б). Файл = один WAV-клип.
ACTIVATE.SPW (17 964), KBUY0.SPW (2 604), CANFIRE.SPW — тот же RIFF-заголовок.
```

Это подтверждается документацией автора порта
([`max-cpp-port-docs/scripting.md`](../../max-cpp-port-docs/scripting.md), строка 51):
оригинальные разработчики использовали механизм «ресурс-ссылка» RES-файла только
для того, чтобы **переименовывать RIFF `*.WAV` в `*.SPW`** на поздних тиражах CD
(`CD-ICD-082-SP`, `CD-ICD-082-1`). Отсюда и имя формата звука в проекте.

Как связаны `.SPW` и спрайты:

- В MAX.RES есть **721 ресурс-ссылка**: 607 → `*.spw`, 71 → `*.flc`, 24 → `*.wrl`,
  16 → `*.msc`, 3 → `*.mve`. Тело такого ресурса — ASCIIZ-имя файла
  (например, `TANK1` = `"tank1.spw\0"`, 10 байт; `TANK14` = `"canfire.spw\0"`).
  Порт открывает файл функцией `ResourceManager_OpenFileResource(id)` и проигрывает
  WAV через miniaudio (`sound_manager.cpp`).
- Имена с суффиксами `1`–`18` в таблице ресурсов (`TANK1`…`TANK16`, `SCOUT1`…,
  `CONST1`…, `*17`/`*18` и т.д.) — это **слотовые звуки юнитов** (движение, стрельба,
  попадание, взрыв), а не кадры спрайта. Один файл используется многими тегами
  (например, кнопки меню → `mengens4.spw`).
- **Сам спрайт юнита** — ресурс с тегом-именем юнита без суффикса (`TANK`, `SCOUT`,
  `COMMTWR`), тень — `S_<имя>` (`S_TANK`). Мастер-строитель вырезан из игры: в
  `defects.md` сайта перечислены отсутствующие `MASTER`, `S_MASTER`, `A_MASTER`,
  `I_MASTER`, `P_MASTER`, `F_MASTER`; в MAX.RES этих записей действительно нет.
- В оригинальном `MAXRUN.EXE` строки `".SPW"` нет вовсе (проверено по
  `01-strings.txt`): имена файлов берутся из RES-записей.

**Вывод:** «формат спрайтов SPW» — это фактически два независимых формата:
мультикадровый спрайт внутри RES (описан ниже) и RIFF/WAVE-звук во внешнем
файле `.SPW`. Эта спецификация описывает спрайт; WAV-контейнер описан в §8.5.

---

## 2. Расположение спрайтов и порядок загрузки

Загрузчик ресурсов описан в `formats/res.md`. Для спрайтов существенно:

1. `ResourceManager_BuildResourceTable` регистрирует записи PATCHES.RES, затем
   MAX.RES по тегу → `ResourceID` (индекс в `ResourceManager_ResourceIdList[]`,
   `resourcetable.cpp`; порядок задан `enum ResourceID` в `resourcetable.hpp`).
2. Юнит создаётся из данных скрипта `SC_UNITS` (PATCHES.RES, бинарный формат
   скриптов; порт конвертирует его схемой `SC_SCHEU`); поля юнита: `sprite`,
   `shadow`, `data`, `flics_animation`, `portrait`, `icon`, `armory_portrait`
   (`units.cpp: CreateUnitFromJson`) и `FrameInfo` (`unit.hpp: struct FrameInfo`):
   ```c
   struct FrameInfo {
       int16_t image_base;             // базовый индекс кадра в спрайте
       int16_t image_count;            // число кадров анимации
       int16_t turret_image_base;      // база кадров башни (TURRET_SPRITE)
       int16_t turret_image_count;
       int16_t firing_image_base;      // кадры выстрела (HAS_FIRING_SPRITE)
       int16_t firing_image_count;
       int16_t connector_image_base;   // кадры коннекторов (CONNECTOR_UNIT)
       int16_t connector_image_count;
       Point  angle_offsets[8];        // смещения кадров по 8 направлениям
   };
   ```
3. `UnitInfo::Init()` (`unitinfo.cpp:430`) грузит спрайт:
   ```c
   sprite_data = ResourceManager_LoadResource(unit.GetSprite());   // кэшируется
   shadow_data = ResourceManager_LoadResource(unit.GetShadow());
   if (ResourceManager_DisableEnhancedGraphics) {                  // классический режим
       sprite_data = Gfx_RescaleSprite(sprite_data, &data_size, 0, 2);  // ×1/2
       shadow_data = Gfx_RescaleSprite(shadow_data, &data_size, 1, 2);
   }
   unit.SetSpriteData(sprite_data);
   ```
   То есть в MAX.RES спрайты хранятся в «enhanced» (полном) разрешении; при
   выключенных enhanced graphics порт уменьшает их вдвое целочисленным
   субдискретизатором `Gfx_RescaleSprite` (§7.3).

### 2.1. Теги спрайтов

- Спрайты юнитов: `COMMTWR` (0x00) … `WALDO` (0x5C) — 93 тега; в MAX.RES есть
  все, кроме `MASTER` (0x30).
- Тени: `S_COMMTW` (0x5E) … `S_HARVST` (0xAB) — 78 тегов; нет `S_MASTER` (0x87)
  и `S_SUBMAR` (0x9E).
- Полный инвентарь с размерами — §8.1–8.2.

---

## 3. Структура спрайта

Всё **little-endian**, выравнивания нет, структуры упакованы. Спрайт = целиком
тело RES-ресурса (без сжатия).

```
ImageMultiHeader (начало спрайта):
+0x00  u16 image_count                     — число кадров
+0x02  u32 frame_offset[image_count]       — смещения кадров ОТ НАЧАЛА СПРАЙТА

Каждый кадр (ImageMultiFrameHeader) лежит по frame_offset[i]:
+0x00  i16 width                           — ширина кадра в пикселях
+0x02  i16 height                          — высота кадра в пикселях
+0x04  i16 hotx                            — «горячая точка» (якорь), x
+0x06  i16 hoty                            — «горячая точка» (якорь), y
+0x08  u32 row_offset[height]              — смещения строк ОТ НАЧАЛА СПРАЙТА
+0x08+height*4  данные строк (RLE, §4)
```

Типы в порте (`resource_manager.hpp`, `unitinfo.cpp: GetSpriteFrame`):

```c
struct __attribute__((packed)) ImageMultiFrameHeader {
    int16_t  width;
    int16_t  height;
    int16_t  hotx;
    int16_t  hoty;
    uint32_t* rows;                        // height × u32, смещения от начала спрайта
};
struct ImageMultiHeader {
    uint16_t image_count;
    struct ImageMultiFrameHeader* frames[]; // на самом деле u32-смещения
};

frame = (ImageMultiFrameHeader*)((uint8_t*)sprite +
         ((u32*)((uint8_t*)sprite + 2))[image_index]);   // GetSpriteFrame
```

### 3.1. Реальный hex: `TANK` (MAX.RES, offset 0x00CEA3F3, размер 35 211)

```
00000000  18 00 62 00 00 00 FD 08 00 00 E1 11 00 00 1C 1A  ..b.............      image_count = 0x0018 = 24
00000010  00 00 F1 22 00 00 5D 2B 00 00 29 34 00 00 7F 3C  ..."..]+..)4...<      frame[0] @ 0x000062 = 98
00000020  00 00 6B 45 00 00 BE 48 00 00 F0 4B 00 00 D5 4E  ..kE...H...K...N      frame[1] @ 0x0008FD = 2301
00000030  00 00 DA 51 00 00 31 55 00 00 64 58 00 00 45 5B  ...Q..1U..dX..E[      frame[2] @ 0x0011E1 = 4577
00000040  00 00 42 5E 00 00 AA 63 00 00 58 69 00 00 65 6E  ..B^...c..Xi..en      …
00000050  00 00 DB 73 00 00 58 79 00 00 1F 7F 00 00 EC 83  ...s..Xy........
```

Инвариант реальных файлов: `frame_offset[0] == 2 + 4 * image_count`
(смещения идут сразу за `image_count`; 98 = 2 + 24·4 ✓).

Кадр 0 по смещению +0x62:

```
00000062  2C 00 2D 00 14 00 17 00 1E 01 00 00 2D 01 00 00  ,.-.........      width  = 0x002C = 44
00000072  54 01 00 00 7F 01 00 00 AA 01 00 00 D5 01 00 00  T...............      height = 0x002D = 45
                                                                                    hotx   = 0x0014 = 20
                                                                                    hoty   = 0x0017 = 23
                                                                                    row[0] @ 0x00011E = 286
                                                                                    row[1] @ 0x00012D = 301  (286+15)
                                                                                    row[2] @ 0x000154 = 340 …
```

Проверка: кадр 0 начинается на 98, заголовок 8 байт + 45×4 = 188 байт смещений
строк → данные строк с 98+188 = 286 = 0x11E ✓. Смещения строк **абсолютные**
(от начала всего спрайта), а не от кадра — декодер считает
`row = &sprite[row_offset]` при `Gfx_ResourceBuffer = начало спрайта`.

Строка 0 кадра 0 (по +0x11E):

```
0000011E  04 05 CF CE CB CA F0 19 05 CF CA C7 D1 E8 FF     ................
   run 1: skip=4, count=5, пиксели: CF CE CB CA F0
   run 2: skip=25, count=5, пиксели: CF CA C7 D1 E8
   0xFF  — конец строки
```

Сумма `skip+count` по прогонам = 4+5+25+5 = 39, при `width` = 44 → **последние
5 пикселей строки прозрачны и в файле не хранятся** (ключевой edge-кейс, §4.2).

### 3.2. Тени: `S_TANK` (MAX.RES, offset 0x00DE4137, размер 3 604)

```
00000000  18 00 62 00 00 00 ED 01 00 00 AE 03 00 00 0D 05  ..b.............      image_count = 24
00000010  00 00 C9 06 00 00 39 08 00 00 EE 09 00 00 54 0B  ......9.......T.
…
00000062  (кадр 0) width=49, height=53, hot=(20,23), 53×4 смещения строк;
00000062+8+53*4 → строка 0: 04 05 19 05 FF
```

Строка тени — **пары счётчиков без байтов пикселей**: `{skip=4,count=5}`,
`{skip=25,count=5}`, терминатор `0xFF` (§4.3). Из-за этого кадры тени компактные
(у `S_TANK` 24 кадра занимают 3604 байта, а 24 кадра `TANK` — 35 211).

### 3.3. Здание: `COMMTWR` (offset 0x0011803F, размер 14 153)

```
00000000  0A 00 2A 00 00 00 46 0F 00 00 50 1D 00 00 C5 20  ..*...F...P....      image_count = 10
                                                                                    frame[0] @ 0x2A = 42 (= 2+10·4)
0000002A  (кадр 0) 70 00 3F 00 35 00 1D 00 …                p.?.5...              width=112, height=63, hot=(53,29)
```

Число кадров у зданий произвольное (не кратно 8): у юнитов кадры группируются
по 8 направлениям (§5.3), у зданий и эффектов — по собственным анимациям.

### 3.4. Поля-«флаги» отсутствуют

В формате спрайта **нет** поля флагов и нет сигнатуры. Признаки
`HAS_FIRING_SPRITE`, `TURRET_SPRITE`, `SPINNING_TURRET`, `CONNECTOR_UNIT` и т.п.
(значения 0x20000/0x2000000/0x8000000/0x8 в `enums.hpp`/`unit.hpp`) — это флаги
**данных юнита** (скрипт `SC_UNITS`), а не заголовка спрайта. Определять «теневой
или нет» ресурс по содержимому нельзя — только по тегу (`S_*`) или по тому, что
в строках нет байтов пикселей (оба способа дают одинаковый результат).

---

## 4. Кодирование строк (RLE)

### 4.1. Обычный спрайт

Строка = последовательность прогонов, завершённая байтом `0xFF`:

```
run   = u8 transparent_count   — сколько прозрачных пикселей пропустить слева
        u8 pixel_count         — сколько непрозрачных пикселей следует дальше
        u8 pixel[pixel_count]  — палитровые индексы (0..255)
row   = run* , 0xFF
```

- Прозрачность **структурная**: прозрачные пиксели просто отсутствуют в файле.
  Отдельного прозрачного цвета/маски нет (в отличие от «простых» изображений
  `ImageSimpleHeader`/`ImageBigHeader`, где есть байт `transparent_color`, §6.4).
- `transparent_count` не может быть `0xFF` (байт 0xFF в начале прогона = конец
  строки), т.е. максимальный пропуск — 254. Пиксели внутри полезной нагрузки
  могут иметь значение 0xFF (декодер не смотрит на них: он читает ровно
  `pixel_count` байт после счётчика).
- `pixel_count` может быть 0 (прогон «только пропуск» — допустимо, хотя в
  реальных файлах не встречалось).
- Ограничение декодера: строка обрабатывается до первого `0xFF`; длина буфера
  не проверяется, поэтому парсер должен доверять `row_offset`-ам и проверять
  границы ресурса.

### 4.2. Строка короче width — это норма

Сумма `Σ(transparent_count + pixel_count)` по прогонам строки может быть **меньше**
`width` (пример TANK §3.1: 39 из 44). Хвост строки в этом случае — прозрачные
пиксели, которые в файл не записаны. Декодер их просто не рисует (в
`Gfx_DecodeSprite` цикл заканчивается на терминаторе; буфер карты для этих
пикселей не трогается). При валидации: `Σ ≤ width` обязательно, равенство —
частое, но не обязательное. Строка **не** может выходить за `width` по суммарному
счётчику (в реальных файлах не выходит; рисовальщик всё равно клиппирует по
масштабированной ширине).

### 4.3. Тень

Строка тени = пары счётчиков без пиксельных байтов:

```
run   = u8 transparent_count   — пропустить
        u8 shadow_count        — затемнить столько пикселей карты
row   = run* , 0xFF
```

Декодер тени (`Gfx_DecodeShadow`, `gfx.cpp:346`) не читает байтов пикселей вовсе:
`Gfx_PixelCount += buf[0]; shadow_count = buf[1]; buf += 2;` и затем применяет
затемнение к уже отрисованной карте (LUT уровня яркости 5, §6.3). Те же правила
хвоста строки, что и в §4.2.

### 4.4. Цвета «командных» слотов

Палитровые индексы в пиксельных байтах — обычные значения 0..255, но диапазоны
**32–39 и 48–63 зарезервированы под командные цвета** юнита и перекрашиваются в
момент отрисовки через LUT команды (§6.2). В исходном спрайте эти индексы —
«заглушки» художника; реальный цвет определяется командой владельца.

---

## 5. Как кадр рисуется

### 5.1. Позиционирование (hot spot)

`Gfx_DecodeSpriteSetup(point, frame, divisor, bounds)` (`gfx.cpp:66`):

```
width, height, hotx, hoty = 4 × i16 из заголовка кадра
scaled.ulx = point.x - (hotx * 2) / divisor
scaled.uly = point.y - (hoty * 2) / divisor
scaled.lrx = scaled.ulx + (width * 2) / divisor
scaled.lry = scaled.uly + (height * 2) / divisor
```

- `point` — позиция юнита в «картографических» координатах; при `divisor = 2`
  (нормальный масштаб) спрайт рисуется 1:1, и `point` попадает ровно в hot spot
  (например, у TANK в точку (20,23) кадра — визуальный «центр» машины).
- `divisor = scaling_factor = 1 << (scaler_adjust + 1)` (у юнита по умолчанию
  `scaler_adjust = 0` → 2); при выключенных enhanced graphics делится пополам.
  `scaler_adjust` увеличивается для «высоты» (взлёт/посадка, `units_manager.cpp`).
- `zoom_level = 2 * Gfx_ZoomLevel / scaling_factor` (`Gfx_ZoomLevel` — текущий
  масштаб карты; при `zoom_level >= 4` спрайт вообще рисуется; при
  `zoom_level < 8` — сплошным силуэтом командного цвета, §5.2).
- Затем координаты переводятся в окно через `Gfx_ScaleInt32(x) = (x << 16) /
  Gfx_MapScalingFactor` (16.16 fixed point) и клипаются по `bounds`.

### 5.2. Построчный декодер `Gfx_DecodeSprite` (`gfx.cpp:212`)

Итерация по строкам с субдискретизацией при уменьшении:

```
scale_x = (((width  - 1) << 16) / (scaled_width  - 1)) + 8   // 16.16
scale_y = (((height - 1) << 16) / (scaled_height - 1)) + 8
for row_index = scaled_offset.y * scale_y; ; row_index += scale_y:
    row = &sprite[row_offsets[row_index >> 16]]               // пропуск строк при зуме
    x_count = 0
    for each run in row:                                      // до 0xFF
        x_count += run.skip
        // позиция в масштабированном окне: фаза (x_count << 16) / scale_x
        x_count += run.count
        если попали в видимую зону окна:
            if Gfx_TeamColorIndexBase != 0:                   // зум < 8: силуэт
                memset(окно + offset, brightness_table[TeamColorIndexBase], n)
            else:                                             // полный цвет
                color = brightness_table[ team_lut[ pixel ] ]
                window[offset + i] = color                    // i с шагом фазы
    offset += WindowWidth
```

- Порядок индексации: **сначала LUT команды** (`Gfx_ColorIndices` =
  `color_cycling_lut` юнита, §6.2), **потом LUT яркости** (7 уровней,
  `Gfx_UnitBrightnessBase & ~31`).
- `Gfx_TeamColorIndexBase` ≠ 0 означает режим «силуэта»: все непрозрачные пиксели
  заливаются одним цветом `brightness_table[base]`. База выбирается по флагам
  команды: красный `COLOR_RED`=1, зелёный `COLOR_GREEN`=2, синий `COLOR_BLUE`=3,
  серый `0xFF`, жёлтый `COLOR_YELLOW`=4 (по умолчанию), `COLOR_BLACK`=0 — полный
  цвет (`unitinfo.cpp:3111-3130`).
- Результат пишется в экранный буфер индексов (`Gfx_MapWindowBuffer`); в RGB
  индексы превращаются системной палитрой на финальном этапе вывода.

### 5.3. Выбор кадра и раскладка по направлениям

`UnitInfo::DrawSpriteFrame(image_index)`; кадр = `GetSpriteFrame(sprite,
image_index)`; границы зон обновления — `UpdateSpriteFrameBounds` с
`x - hotx*scale, y - hoty*scale, ширина×scale` (`unitinfo.cpp:842-869`).

Направления (`UnitAngleType`): N=0, NE=1, E=2, SE=3, S=4, SW=5, W=6, NW=7.
Раскладка кадров юнита — **по 8 на фазу**: `кадр = база + фаза×8 + угол`.
Из кода `unitinfo.cpp`:

- обычный кадр: `image_base + angle`; смена фазы анимации — `+8`
  (`DrawSpriteFrame(image_index + 8)`);
- отдача при выстреле: `±8` (соседняя фаза);
- кадры выстрела: `angle + 104` (фаза 13), у башен `turret_angle + 104`;
- подбитый коммандос: `angle + 200` (фаза 25);
- коннекторы: `connector_image_base + 0..7` (N/S/E/W × top/bottom);
- `INFANTRY`/`COMMANDO`: `image_index_max = 103` (фазы 0–12, 13×8);
- `MININGST`: `image_base = (clan - 1) * 2` — клановые варианты кадров.

Примеры из MAX.RES: `TANK` 24 кадра = 3 фазы × 8; `SCOUT` 32 = 4×8; `INFANTRY`
200 = 25×8; `COMMANDO` 208 = 26×8; `BLDEXPLD` 28 (взрыв); `LRGCONES` 1 кадр.
Размеры кадров различаются по направлениям (у TANK кадр 0 — 44×45, кадр 1 —
56×57): каждый кадр имеет собственные `width/height/hot`, обрезку по
ограничивающему прямоугольнику.

Зеркалирования нет: все 8 направлений — готовые кадры. Таблица
`Gfx_DirectionCorrections[8]` в `gfx.cpp` (парные перестановки координат для
поворотов) определена, но в текущем порте не используется (мёртвый код).

### 5.4. Порядок отрисовки юнита

`UnitInfo::RenderWithConnectors` (`unitinfo.cpp:3151`): тень → спрайт →
коннекторы (для каждого — своя тень+спрайт) → башня (тень+спрайт со смещением
`turret_offset_*`). Тень рисуется по тем же кадрам `S_*`-спрайта со смещением
`shadow_offset` и **затемняет карту** (не рисует пиксели — §4.3). Зоны
перерисовки учитывают и спрайт, и тень, и башню (`UpdateSpriteFrameBounds` +
`GameManager_AddDrawBounds`).

---

## 6. Палитра

`PALETTE_SIZE = 256`, `PALETTE_STRIDE = 3` (RGB, 6 бит на компонент; SDL-порт
умножает на 4 при записи в системную палитру). `ColorIndex = u8`.

### 6.1. COLOR.PAL

Файл `max-original/COLOR.PAL` (33 536 байт) читается в `Color_Init`
(`color.cpp:94`):

```
+0x0000  768 байт      — палитра: 256 × RGB (6 бит/компонент)
+0x0300  32768 байт    — таблица RGB555 → ColorIndex (2^15 записей, RGB555_COLOR_COUNT)
```

### 6.2. Командные цвета (перекраска спрайтов)

Зарезервированные слоты пикселей: **32–39 и 48–63** (24 слота, «заглушки»).
`ResourceManager_BuildColorTables` (`resource_manager.cpp:919`) строит LUT 256
записей для каждой команды: слоты 32–39 и 48–63 заменяются на 8 цветов команды
(циклически, `j & 7`), остальные индексы — identity:

| Команда | Слоты → реальные индексы |
|---|---|
| Красная | 56–63 (`{56,57,58,59,60,61,62,63}`) |
| Зелёная | 32–39 (`{32,33,34,35,36,37,38,39}`) |
| Синяя | 48–55 (`{48,49,50,51,52,53,54,55}`) |
| Серая | `{255,161,172,169,216,213,212,207}` |
| Брошенная (derelict) | `{216,215,214,213,212,211,210,209}` |

LUT команды выбирается в `hash.cpp` по флагу команды и подключается в декодер
как `Gfx_ColorIndices`. Фиксированные цвета UI (`color.h`): 0 = чёрный,
1 = красный, 2 = зелёный, 3 = синий, 4 = жёлтый, 5 = хром-жёлтый,
6 = римское серебро, 7 = красно-оранжевый, 8 = пастельно-жёлтый.

### 6.3. Яркость и затемнение

- `ResourceManager_BrightnessColorIndexTable`: 7 уровней × 256 записей
  (`world.cpp:513-530`): уровень `l`: `rgb' = rgb · (l·32) / 224`, индекс 31
  принудительно = 31. Индекс уровня в декодере: `Gfx_UnitBrightnessBase & ~31`
  (яркость юнита, 0..255).
- Тень: `BrightnessColorIndexTable[5 * 256]` — уровень затемнения 5
  (`gfx.cpp:426`).
- `ResourceManager_DarkeningColorIndexTable`: `max(7, c) · 31 / 63`
  (`world.cpp:496-508`) — затемнение карты (не спрайтов).

### 6.4. Родственные форматы «простых» изображений (НЕ спрайты)

Для полноты (эти структуры встречаются в тех же RES-файлах):

```c
struct __attribute__((packed)) ImageSimpleHeader {   // sizeof == 9
    int16_t width; int16_t height; int16_t ulx; int16_t uly;
    uint8_t transparent_color;   // явный прозрачный индекс; пиксели — сырой
                                 // массив width*height байт сразу за заголовком
};
struct __attribute__((packed)) ImageBigHeader {      // sizeof == 777
    int16_t ulx; int16_t uly; int16_t width; int16_t height;
    Color palette[3 * PALETTE_SIZE];                 // собственная палитра 256×3
    uint8_t transparent_color;                       // + width*height байт пикселей
};
```

У мультикадровых спрайтов ни байта прозрачного цвета, ни встроенной палитры нет —
прозрачность кодируется прогонами, палитра глобальная (COLOR.PAL).

---

## 7. Псевдокод загрузчика и декодера

### 7.1. Чтение спрайта из RES

```
LoadSprite(res_id):
    item = ResMetaTable[res_id]                       # см. formats/res.md
    data = read(item.file, item.offset, item.size)    # без сжатия
    return data                                       # интерпретируется как ImageMultiHeader

GetFrame(sprite, index):
    count = u16_le(sprite[0])
    assert index < count
    off   = u32_le(sprite[2 + 4*index])
    return sprite + off                               # ImageMultiFrameHeader
```

### 7.2. Декодирование кадра в RGBA-буфер (каноническая семантика)

```
DecodeFrame(sprite, frame_index, team_lut, brightness_level):
    frame  = GetFrame(sprite, frame_index)
    w,h    = i16_le(frame+0), i16_le(frame+2)
    hot    = (i16_le(frame+4), i16_le(frame+6))
    rows   = u32_le(frame+8) ...                     # h смещений от начала спрайта
    out    = new byte[w*h]                           # заполнить "прозрачным" сентинелом
    for y in 0..h:
        p  = sprite + rows[y]
        x  = 0
        loop:
            skip = p[0]
            if skip == 0xFF: break                   # конец строки
            cnt  = p[1]; p += 2
            x += skip                                # прозрачные — пропуск
            for i in 0..cnt:
                idx = p[i]
                out[y*w + x + i] = brightness_table[ team_lut[idx] ]
            p += cnt
            x += cnt
        # x может быть < w: хвост строки прозрачен
    return (out, hot)

DecodeShadow(sprite, frame_index, level = 5):        # применяется к буферу карты
    frame  = GetFrame(sprite, frame_index)
    for y in 0..h:
        p = sprite + rows[y]; x = 0
        loop:
            skip = p[0]; if skip == 0xFF: break
            cnt  = p[1]; p += 2                      # байтов пикселей НЕТ
            x += skip
            darken(map, x, cnt, level)               # затемнить cnt пикселей карты
            x += cnt
```

### 7.3. Целочисленный субдискретизатор `Gfx_RescaleSprite` (`gfx.cpp:442`)

Применяется портом при выключенных enhanced graphics (фактор 2) — и к спрайтам
(mode 0), и к теням (mode 1). Строит **новый спрайт того же формата**:

```
RescaleSprite(buffer, factor, mode):
    count = u16_le(buffer[0])
    for each frame i:
        frame = buffer + u32_le(buffer[2 + 4*i])
        w,h,hx,hy = 4×u16_le(frame)
        new_w = (w + factor - 1) / factor            # округление вверх
        new_h = (h + factor - 1) / factor
        new_hx = hx / factor; new_hy = hy / factor
        for j in 0..new_h:                           # строки: каждый factor-й исходный ряд
            RescaleRow(buffer + rows[j*factor], out, mode, factor)
```

`RescaleRow` перепаковывает прогоны: накапливает `transparent_count`, при переходе
через «сохраняемую» позицию (фаза `factor_max`, маска `factor-1`) пишет новый
прогон `{((factor_max & mask) + transparent)/factor, ((factor_max & mask) +
pixel_count)/factor}`; в режиме спрайта (mode 0) сохраняет каждый `factor`-й
пиксель (первый пиксель прогона всегда сохраняется), в режиме тени (mode 1) байты
пикселей не читаются вообще. Прогоны, целиком попадающие между сохраняемыми
позициями, схлопываются в `transparent_count` (для теней — в счётчик затемнения).
Строка завершается `0xFF`; выходной формат идентичен входному (смещения
пересчитываются от начала нового буфера). Точную арифметику см.
`gfx.cpp:576-633` — для пиксельной совместимости Rust-реализации с портом
алгоритм стоит переносить построчно, а не «улучшать».

---

## 8. Файлы и ресурсы

### 8.1. Спрайты (теги 0x00–0x5C в MAX.RES; `MASTER` отсутствует)

| Тег | Байт | Тег | Байт | Тег | Байт | Тег | Байт |
|---|---|---|---|---|---|---|---|
| COMMTWR | 14 153 | POWERSTN | 26 177 | POWGEN | 6 987 | BARRACKS | 15 588 |
| SHIELDGN | 13 294 | RADAR | 60 449 | ADUMP | 8 014 | FDUMP | 6 601 |
| GOLDSM | 8 356 | DEPOT | 20 594 | HANGAR | 24 613 | DOCK | 17 731 |
| CNCT_4W | 2 006 | LRGRUBLE | 18 923 | SMLRUBLE | 9 495 | LRGTAPE | 5 880 |
| SMLTAPE | 2 961 | LRGSLAB | 83 337 | SMLSLAB | 4 320 | LRGCONES | 1 032 |
| SMLCONES | 495 | ROAD | 4 320 | LANDPAD | 3 305 | SHIPYARD | 27 916 |
| LIGHTPLT | 24 610 | LANDPLT | 20 835 | SUPRTPLT | 20 810 | AIRPLT | 23 030 |
| HABITAT | 31 538 | RESEARCH | 25 356 | GREENHSE | 21 789 | RECCENTR | 20 867 |
| TRAINHAL | 23 097 | WTRPLTFM | 4 528 | GUNTURRT | 31 481 | ANTIAIR | 40 749 |
| ARTYTRRT | 31 105 | ANTIMSSL | 36 285 | BLOCK | 7 959 | BRIDGE | 23 313 |
| MININGST | 124 662 | LANDMINE | 307 | SEAMINE | 330 | LNDEXPLD | 31 525 |
| AIREXPLD | 23 933 | SEAEXPLD | 34 519 | BLDEXPLD | 284 918 | HITEXPLD | 1 931 |
| **MASTER** | **нет** | CONSTRCT | 65 450 | SCOUT | 25 187 | TANK | 35 211 |
| ARTILLRY | 42 655 | ROCKTLCH | 38 922 | MISSLLCH | 38 167 | SP_FLAK | 21 537 |
| MINELAYR | 12 394 | SURVEYOR | 18 916 | SCANNER | 20 650 | SPLYTRCK | 13 896 |
| GOLDTRCK | 17 567 | ENGINEER | 60 301 | BULLDOZR | 29 666 | REPAIR | 12 962 |
| FUELTRCK | 15 141 | CLNTRANS | 31 838 | COMMANDO | 137 182 | INFANTRY | 177 415 |
| FASTBOAT | 22 676 | CORVETTE | 25 309 | BATTLSHP | 31 697 | SUBMARNE | 16 792 |
| SEATRANS | 21 756 | MSSLBOAT | 47 112 | SEAMNLYR | 18 343 | CARGOSHP | 21 155 |
| FIGHTER | 27 534 | BOMBER | 33 779 | AIRTRANS | 9 996 | AWAC | 24 712 |
| JUGGRNT | 40 102 | ALNTANK | 22 375 | ALNASGUN | 35 268 | ALNPLANE | 40 746 |
| ROCKET | 4 585 | TORPEDO | 2 799 | ALNMISSL | 11 100 | ALNTBALL | 457 |
| ALNABALL | 873 | RKTSMOKE | 1 606 | TRPBUBLE | 429 | HARVSTER | 2 207 |
| WALDO | 1 055 | | | | | | |

### 8.2. Тени (теги 0x5E–0xAB; нет `S_MASTER`, `S_SUBMAR`)

| Тег | Байт | Тег | Байт | Тег | Байт | Тег | Байт |
|---|---|---|---|---|---|---|---|
| S_COMMTW | 4 602 | S_POWERS | 4 325 | S_POWGEN | 1 992 | S_BARRAC | 3 533 |
| S_SHIELD | 4 289 | S_RADAR | 12 946 | S_ADUMP | 1 654 | S_FDUMP | 1 610 |
| S_GOLDSM | 1 538 | S_DEPOT | 2 882 | S_HANGAR | 3 509 | S_DOCK | 3 604 |
| S_CNCT4W | 1 031 | S_LRGRBL | 4 563 | S_SMLRBL | 4 537 | S_LRGSLA | 5 074 |
| S_LRGCON | 779 | S_SMLSLA | 584 | S_SMLCON | 379 | S_ROAD | 584 |
| S_LANDPA | 480 | S_SHIPYA | 3 286 | S_LIGHTP | 3 147 | S_LANDPL | 4 071 |
| S_SUPRTP | 2 473 | S_AIRPLT | 4 370 | S_HABITA | 2 768 | S_RESEAR | 3 721 |
| S_GREENH | 3 391 | S_RECCEN | 3 360 | S_TRAINH | 3 365 | S_WTRPLT | 550 |
| S_GUNTUR | 9 697 | S_ANTIAI | 16 344 | S_ARTYTR | 10 805 | S_ANTIMS | 11 453 |
| S_BLOCK | 1 650 | S_BRIDGE | 4 636 | S_MINING | 16 641 | S_LANDMI | 147 |
| S_SEAMIN | 358 | **S_MASTER** | **нет** | S_CONSTR | 16 142 | S_SCOUT | 8 842 |
| S_TANK | 3 604 | S_ARTILL | 7 780 | S_ROCKTL | 5 260 | S_MISSLL | 4 352 |
| S_FLAK | 10 150 | S_MINELA | 2 372 | S_SURVEY | 6 820 | S_SCANNE | 2 901 |
| S_SPLYTR | 2 507 | S_GOLDTR | 3 046 | S_ENGINE | 12 334 | S_BULLDO | 5 932 |
| S_REPAIR | 4 246 | S_FUELTR | 2 699 | S_CLNTRA | 3 196 | S_COMMAN | 64 252 |
| S_INFANT | 73 574 | S_FASTBO | 3 382 | S_CORVET | 5 976 | S_BATTLS | 3 395 |
| **S_SUBMAR** | **нет** | S_SEATRA | 2 443 | S_MSSLBO | 4 662 | S_SEAMNL | 5 607 |
| S_CARGOS | 3 300 | S_FIGHTE | 6 546 | S_BOMBER | 6 996 | S_AIRTRA | 3 148 |
| S_AWAC | 3 718 | S_JUGGRN | 7 144 | S_ALNTAN | 4 095 | S_ALNASG | 8 764 |
| S_ALNPLA | 8 218 | S_HARVST | 534 | | | | |

### 8.3. Звуковые ссылки `.SPW` в resourcetable (примеры)

607 записей MAX.RES ссылаются на внешние `.SPW`. Паттерны имён (тег ↔ файл):

- `TANK1/5/7` → `tank1/5/7.spw` (движение/остановка танка), `TANK14` →
  `canfire.spw` (выстрел), `ASGUN14` → `asgun14.spw`,
  `RLNCH14`/`MSLNC14` → `mislfire.spw`, `TANK15/16` — записей нет;
- `GEN_IDLE` → `tanka_1.spw`, `GEN_DRVE` → `tanka_5.spw`, `GEN_STOP` →
  `tanka_7.spw` — общие «танковые» сэмплы движка; те же сэмплы под именами
  `tankc_1/5/7.spw` используют `MLAYER1/5/7` (минный заградитель);
- `*15` → звуки попадания (`CONST15` → `hitlarge.spw`), `*16` → взрывы
  (`CONST16` → `expllrge.spw`), `*17/18` → варианты зданий
  (`MASTR17`, `AUNIT17`, `HANGR17`…);
- интерфейс: `MBUTT0`/`MENUOP`/`NHUMN0`… → `mengens4.spw`, `NCANC0` →
  `mengens3.spw`, `NDONE0` → `menu38.spw`, `KBUY0`/`KCARG0` → `kbuy0.spw`,
  `KSELE0` → `ksel0.spw`, `IOPEN0` → `iopen0.spw`, `ICLOS0` → `iclos0l.spw`,
  `ISTAT0`/`ICOLO0` → `ihits0.spw`, `PING`, `ACTIVATE`, `LOAD`, `FQUIT`.

Полный маппинг получается `python max-original-dasm/tools/res_parser.py
max-original/MAX.RES list` (записи размером < 64 байт с ASCIIZ-именем —
ссылки).

### 8.4. Файлы `*.SPW` в `max-original/` (272 шт., все — WAV-звуки)

| Файл | Байт | Файл | Байт | Файл | Байт |
|---|---|---|---|---|---|
| ACTIVATE.SPW | 17 964 | ALNTK1.SPW | 123 760 | ALNTK5.SPW | 258 160 |
| ALNTK7.SPW | 32 556 | APC1.SPW | 89 644 | APC17.SPW | 34 152 |
| APC18.SPW | 32 722 | APC5.SPW | 90 480 | APC7.SPW | 80 172 |
| APLAN1.SPW | 131 952 | APLAN5.SPW | 143 442 | APLAN7.SPW | 43 308 |
| ASGUN14.SPW | 76 844 | ATANK1.SPW | 159 856 | ATANK5.SPW | 258 160 |
| ATANK7.SPW | 32 556 | ATRANS1.SPW | 150 384 | ATRANS5.SPW | 264 304 |
| ATRANS7.SPW | 65 964 | ATTACK1.SPW | 131 952 | ATTACK5.SPW | 157 088 |
| ATTACK7.SPW | 43 308 | AUNIT17.SPW | 297 072 | AUNIT18.SPW | 144 428 |
| AWAC1.SPW | 52 208 | AWAC5.SPW | 264 048 | AWAC7.SPW | 55 340 |
| BLDEXPLG.SPW | 241 452 | BLOCK16.SPW | 132 830 | BOATEXP1.SPW | 141 868 |
| BULL1.SPW | 153 712 | BULL17.SPW | 196 976 | BULL5.SPW | 141 168 |
| BULL7.SPW | 27 436 | CANFIRE.SPW | 86 060 | CBLDEXP1.SPW | 201 230 |
| CBLDEXP2.SPW | 199 148 | CMINE16.SPW | 144 684 | CONST1.SPW | 120 790 |
| CONST2.SPW | 123 504 | CONST5.SPW | 102 256 | CONST6.SPW | 224 624 |
| CONST7.SPW | 38 700 | CONST8.SPW | 31 532 | CORVT14.SPW | 44 844 |
| ENGIN1.SPW | 132 208 | ENGIN2.SPW | 132 464 | ENGIN5.SPW | 86 128 |
| ENGIN6.SPW | 85 872 | ENGIN7.SPW | 27 948 | ENGIN8.SPW | 32 556 |
| EPLOWET1.SPW | 91 692 | EPLOWET2.SPW | 70 956 | EXPBULD5.SPW | 302 778 |
| EXPBULD6.SPW | 265 500 | EXPLBLD1.SPW | 167 080 | EXPLBLD2.SPW | 164 392 |
| EXPLBULD.SPW | 196 058 | EXPLLRGE.SPW | 150 072 | EXPLMED.SPW | 122 240 |
| EXPLSMAL.SPW | 123 326 | EXPSDIRT.SPW | 74 138 | F001.SPW | 23 852 |
| F004.SPW | 53 804 | F005.SPW | 44 076 | F006.SPW | 56 364 |
| F007.SPW | 56 108 | F010.SPW | 51 756 | F012.SPW | 93 484 |
| F013.SPW | 45 868 | F025.SPW | 93 740 | F026.SPW | 106 284 |
| F031.SPW | 88 876 | F032.SPW | 100 652 | F037.SPW | 89 644 |
| F038.SPW | 97 068 | F043.SPW | 87 084 | F044.SPW | 100 396 |
| F049.SPW | 148 268 | F050.SPW | 73 004 | F053.SPW | 29 484 |
| F057.SPW | 64 044 | F061.SPW | 96 044 | F066.SPW | 72 492 |
| F070.SPW | 56 620 | F071.SPW | 47 916 | F075.SPW | 79 660 |
| F080.SPW | 105 004 | F081.SPW | 99 628 | F085.SPW | 34 604 |
| F089.SPW | 83 756 | F093.SPW | 63 276 | F094.SPW | 79 916 |
| F095.SPW | 44 332 | F098.SPW | 80 172 | F103.SPW | 88 620 |
| F108.SPW | 77 356 | F113.SPW | 86 060 | F118.SPW | 112 684 |
| F122.SPW | 111 660 | F126.SPW | 108 844 | F130.SPW | 104 492 |
| F138.SPW | 65 836 | F142.SPW | 56 876 | F145.SPW | 66 348 |
| F150.SPW | 54 060 | F151.SPW | 62 508 | F154.SPW | 54 316 |
| F155.SPW | 59 948 | F158.SPW | 42 284 | F162.SPW | 66 348 |
| F163.SPW | 63 276 | F164.SPW | 54 572 | F165.SPW | 52 012 |
| F166.SPW | 58 156 | F169.SPW | 47 404 | F171.SPW | 48 940 |
| F176.SPW | 55 084 | F177.SPW | 43 052 | F181.SPW | 52 524 |
| F182.SPW | 51 244 | F186.SPW | 54 316 | F187.SPW | 55 340 |
| F191.SPW | 37 932 | F192.SPW | 33 324 | F196.SPW | 33 324 |
| F198.SPW | 61 996 | F201.SPW | 67 628 | F206.SPW | 95 276 |
| F207.SPW | 47 660 | F210.SPW | 52 524 | F211.SPW | 62 508 |
| F215.SPW | 93 740 | F216.SPW | 37 932 | F217.SPW | 45 100 |
| F219.SPW | 56 620 | F220.SPW | 32 556 | F224.SPW | 57 644 |
| F229.SPW | 53 804 | F230.SPW | 48 684 | F231.SPW | 33 068 |
| F232.SPW | 51 244 | F234.SPW | 49 964 | F236.SPW | 48 684 |
| F239.SPW | 45 612 | F242.SPW | 43 564 | F243.SPW | 92 460 |
| F244.SPW | 50 988 | F247.SPW | 66 092 | F249.SPW | 64 044 |
| F250.SPW | 50 988 | F251.SPW | 68 908 | F255.SPW | 52 268 |
| F256.SPW | 69 164 | F265.SPW | 56 620 | F270.SPW | 59 948 |
| F271.SPW | 50 988 | F272.SPW | 56 620 | F273.SPW | 80 172 |
| F275.SPW | 77 356 | F276.SPW | 49 964 | F278.SPW | 60 972 |
| F279.SPW | 89 644 | F280.SPW | 92 972 | F281.SPW | 90 156 |
| F282.SPW | 95 276 | F283.SPW | 59 180 | F284.SPW | 23 340 |
| FANTI14.SPW | 149 036 | FARTY14.SPW | 115 884 | FGUN14.SPW | 65 534 |
| FIGHT1.SPW | 172 656 | FIGHT5.SPW | 175 162 | FIGHT7.SPW | 47 916 |
| FQUIT.SPW | 12 844 | FTRUCK17.SPW | 49 916 | GBOATIDL.SPW | 301 424 |
| GBOATMVE.SPW | 166 256 | GBOATSTP.SPW | 65 324 | GOLDR17.SPW | 466 032 |
| GOLDR18.SPW | 192 044 | HANGR17.SPW | 32 556 | HANGR18.SPW | 33 836 |
| HITLARGE.SPW | 98 886 | HITMED.SPW | 69 420 | HITSMAL.SPW | 55 898 |
| HVP17.SPW | 218 352 | HVP18.SPW | 79 148 | ICLOS0L.SPW | 47 842 |
| IHITS0.SPW | 9 260 | INFAN14.SPW | 75 018 | INFAN15.SPW | 60 238 |
| INFAN16.SPW | 67 888 | INFIL14.SPW | 35 226 | INFIL15.SPW | 50 790 |
| INFIL16.SPW | 72 338 | IOPEN0.SPW | 38 564 | JUGGR1.SPW | 82 416 |
| JUGGR5.SPW | 81 344 | JUGGR7.SPW | 26 412 | KBUY0.SPW | 2 604 |
| KSEL0.SPW | 10 412 | LOAD.SPW | 17 964 | LVP17.SPW | 303 088 |
| LVP18.SPW | 190 164 | MANMOVE.SPW | 212 864 | MANTI14.SPW | 80 940 |
| MASTR17.SPW | 107 632 | MBOATIDL.SPW | 209 520 | MBOATMVE.SPW | 163 440 |
| MBOATSTP.SPW | 43 052 | MENGENS3.SPW | 5 420 | MENGENS4.SPW | 4 908 |
| MENU38.SPW | 6 444 | MISLFIRE.SPW | 73 132 | MLAYER17.SPW | 16 172 |
| MLAYER18.SPW | 13 954 | MONOP17.SPW | 454 768 | MONOP18.SPW | 196 652 |
| MSLCR14.SPW | 41 538 | MSTAT18.SPW | 132 462 | PING.SPW | 12 972 |
| PLANLAND.SPW | 96 556 | PLANOFF.SPW | 47 148 | POWGN17.SPW | 237 614 |
| POWGN18.SPW | 106 656 | RADAR13.SPW | 123 760 | REPAIR1.SPW | 58 736 |
| REPAIR17.SPW | 150 840 | REPAIR5.SPW | 82 396 | REPAIR7.SPW | 19 028 |
| RESEAR17.SPW | 414 832 | RESEAR18.SPW | 172 844 | SBOATIDL.SPW | 261 232 |
| SBOATMVE.SPW | 254 064 | SBOATSTP.SPW | 121 644 | SCALE.SPW | 89 174 |
| SCAN1.SPW | 58 736 | SCAN5.SPW | 69 488 | SCAN7.SPW | 25 078 |
| SCOUT1.SPW | 110 192 | SCOUT14.SPW | 98 630 | SCOUT2.SPW | 138 864 |
| SCOUT5.SPW | 124 528 | SCOUT6.SPW | 124 784 | SCOUT7.SPW | 23 084 |
| SCOUT8.SPW | 46 892 | SMINE17.SPW | 46 380 | SMINE18.SPW | 82 268 |
| SUB14.SPW | 52 524 | SUB16.SPW | 147 810 | SUB2.SPW | 310 508 |
| SUB6.SPW | 191 136 | SUB8.SPW | 45 100 | SURVY1.SPW | 86 140 |
| SURVY2.SPW | 88 676 | SURVY5.SPW | 59 760 | SURVY6.SPW | 97 848 |
| SURVY7.SPW | 43 986 | SURVY8.SPW | 73 040 | TANK1.SPW | 58 442 |
| TANK5.SPW | 148 592 | TANK7.SPW | 22 188 | TANKA_1.SPW | 95 946 |
| TANKA_5.SPW | 76 656 | TANKA_7.SPW | 29 214 | TANKC_1.SPW | 95 946 |
| TANKC_5.SPW | 76 656 | TANKC_7.SPW | 29 214 | TRUCKIDL.SPW | 206 704 |
| TRUCKMVE.SPW | 205 168 | TRUCKSTP.SPW | 35 372 | | |

Примечания: `F###` — реплики/сэмплы кампании (цифра — ID ресурса `V_F###`);
`TANKA_*`/`TANKC_*` — общие «танковые» сэмплы движка (используются тегами
`GEN_IDLE`/`GEN_DRVE`/`GEN_STOP` и `MLAYER1/5/7`); `*IDL/*MVE/*STP` —
idle/move/stop кораблей и грузовиков.

### 8.5. Формат звукового файла `.SPW`

RIFF/WAVE: чанки `fmt ` (PCM, 1 канал, 22050 Гц, block align 2, 16 бит) + `data`
(+ опционально `smpl`, 60 б). Уникальных структур нет — стандартный WAV с
переименованным расширением. Порт играет их через miniaudio
(`sound_manager.cpp:940`).

---

## 9. Замечания для Rust max_render (edge-кейсы)

1. **Не путать `.SPW` и спрайты**: `.SPW` в каталоге игры — WAV-звуки (272 шт.);
   спрайты — ресурсы MAX.RES с тегами `COMMTWR..WALDO` и `S_*`. Ресурсы-ссылки
   (`TANK1` → `"tank1.spw\0"`) — имена звуковых файлов, их НЕ надо парсить как
   спрайты.
2. **Все числа LE, без выравнивания**: `u16`/`i16`/`u32` читать по отдельности;
   структуры не выровнены.
3. **Смещения абсолютные**: `frame_offset[]` и `row_offset[]` отсчитываются от
   начала спрайта (тела RES-ресурса), а не от кадра. Первый кадр в реальных
   файлах начинается на `2 + 4*image_count`, но это инвариант данных, а не
   формата — доверять таблице.
4. **Строка короче `width`**: хвост строки — неявная прозрачность
   (`Σ(skip+count) ≤ width`). Выделять кадр по `width×height` и не паниковать на
   недозаполненных строках; также не выходить за `width` при отрисовке.
5. **Терминатор `0xFF`**: `transparent_count` не бывает 0xFF; пиксель со значением
   0xFF допустим только внутри полезной нагрузки прогона. Парсер строки читает
   ровно `pixel_count` байт и не интерпретирует их как структуру.
6. **Тени**: строки без байтов пикселей (пары `{skip, count}`). Режим нельзя
   определить по содержимому — только по тегу ресурса (`S_*` / поле `shadow`
   юнита). Затемнение применяется к уже отрисованной карте (LUT яркости, уровень
   5), а не рисуется пикселями.
7. **Пустые/отсутствующие спрайты**: `MASTER`, `S_MASTER`, `S_SUBMAR` отсутствуют
   в MAX.RES; `LoadResource` возвращает null — обрабатывать как «невидимый юнит»,
   не паниковать (в оригинале это источник багов, `defects.md` №105/№233).
8. **Командные цвета**: индексы 32–39 и 48–63 перекрашивать LUT команды
   (таблица в §6.2); индекс 255 — серый командный. Без LUT спрайты выглядят
   «радужно» — это известная ошибка ранних портов.
9. **Масштаб/силуэт**: при `zoom_level < 8` юнит рисуется сплошным командным
   цветом (`Gfx_TeamColorIndexBase ≠ 0`), при `≥ 8` — полными пикселями.
   Фиксированная точка 16.16 (`(x << 16) / factor`), фазы строк/столбцов — для
   пиксельной идентичности переносить алгоритм `Gfx_DecodeSprite` как есть.
10. **Enhanced graphics**: при выключенном режиме спрайт предварительно
    уменьшается вдвое `Gfx_RescaleSprite` (фактор 2, субдискретизация с фазой).
    Выходной буфер — спрайт того же формата; если Rust-рендер будет
    поддерживать этот режим, алгоритм из `gfx.cpp:576-633` переносить дословно.
11. **Кадры**: размеры/горячие точки у кадров разные; позиция = `point −
    (hot·2)/divisor`; раскладка по 8 направлений (`база + фаза×8 + угол`),
    спецфазы: отдача ±8, выстрел +104, подбитый коммандос +200; базы фаз — из
    данных юнита (скрипт `SC_UNITS` → `FrameInfo`), а не из файла спрайта.
12. **Палитра**: 256×3 байт RGB (6 бит/компонент) из `COLOR.PAL` (+32768 байт
    RGB555-таблицы); у спрайтов собственной палитры нет (в отличие от
    `ImageBigHeader`-картинок с палитрой 777-байтного заголовка).
13. **Валидация**: `image_count ≥ 1`; каждый `frame_offset` и `row_offset` —
    в границах ресурса; строки заканчиваются 0xFF до конца данных кадра; не
    читать за конец ресурса (декодер оригинала не проверяет границ).

---

## 10. Якоря RE в MAXRUN.EXE (Ghidra/radare2)

Адреса — как в `01-strings.txt` (линейные в `maxrun.flat`, base 0x10000; рантайм =
flat + 0x160010, см. `README.md` дазма).

| Адрес (flat) | Что |
|---|---|
| 0x0016B834 … 0x0016B995 | assert-строки `drawmap.cpp` (`vertical_side == SIDE_IS_LOWER`, `rect.ulx >= 0 && … < 640 && … < 480` и т.д.) — исходник, где в оригинале живёт декодер спрайтов (в порте он разнесён в `gfx.cpp` + `drawmap.cpp`) |
| 0x0016DAFA | `enhanced_graphics` — ключ настроек; управляет режимом rescale ×1/2 |
| 0x00177013 / 0x0017758D | `scaler_adjust` — ключ данных юнита (масштаб спрайта) |
| 0x0017705B … 0x001770BE | ключи `total_images`, `image_base`, `turret_image_base`, `firing_image_base`, `connector_image_base`, `image_index`, `image_index_max` (сериализация/скрипт UnitInfo) |
| 0x00176E18 / 0x0017738C | `turret_sprite`, `has_firing_sprite` — флаги данных юнита |
| 0x001724A4 | `resrcmgr.cpp` — имя исходника менеджера ресурсов; рядом asserts `DIRnameID < END_MEMORY_RESOURCES` (0x001724B1), `id > END_MEMORY_RESOURCES` (0x001724DF), строки `patches.res` (0x001724FC), `max.res` (0x00172508) |
| 0x0017252E … 0x001762B7 | таблица имён ресурсов `COMMTWR` … `WORLD_E` |
| 0x00185018 | `RES0` — сигнатура RES-контейнера |
| 0x0016B2FE | `sprite != 0` (commo.cpp — окно связи, не формат) |

Кандидаты функций по `functions-named.md`: `fcn.001019cc` (4068 б; xref из
`drawmap.cpp`, `gfx.cpp`, `game_manager.cpp` — общий хелпер), `fcn.0015e59a`
(3123 б; `drawmap.cpp`/`game_manager.cpp`/`unitinfo.cpp` — связка коннекторов).
Строки `".SPW"` в бинарнике нет — расширения файлов приходят из RES-записей.

---

## 11. Источники

- `max-cpp-port/src/gfx.cpp` — `Gfx_DecodeSpriteSetup`, `Gfx_DecodeSprite`,
  `Gfx_DecodeShadow`, `Gfx_RescaleSprite`/`Gfx_RescaleSpriteRow` (эталонный
  декодер и субдискретизатор).
- `max-cpp-port/src/resource_manager.hpp` — `ImageSimpleHeader`/`ImageBigHeader`/
  `ImageMultiHeader`/`ImageMultiFrameHeader`; `.cpp` — `LoadResource`,
  `OpenFileResource`, `BuildColorTables` (LUT команд).
- `max-cpp-port/src/unitinfo.cpp` — `Init` (загрузка спрайта), `GetSpriteFrame`,
  `Render*`, `DrawSpriteFrame`, `UpdateSpriteFrameBounds`, выбор кадров.
- `max-cpp-port/src/units.cpp`/`unit.hpp` — поля юнита (`sprite`, `shadow`,
  `FrameInfo`), звуковые слоты; `units_manager.cpp` — `scaler_adjust`.
- `max-cpp-port/src/color.h`/`color.cpp`, `world.cpp` — палитра, LUT яркости,
  COLOR.PAL.
- `max-cpp-port/src/enums.hpp`, `resourcetable.cpp/.hpp` — идентификаторы ресурсов.
- Реальные файлы: `max-original/MAX.RES` (спрайты `TANK` @ 0xCEA3F3, `S_TANK` @
  0xDE4137, `COMMTWR` @ 0x11803F), 272×`*.SPW` (RIFF/WAVE), `COLOR.PAL`,
  `assets/PATCHES.RES`.
- `max-original-dasm/01-strings.txt`, `functions-named.md`, `formats/res.md`,
  `tools/res_parser.py` — якоря и инструмент извлечения ресурсов.
- `max-cpp-port-docs/scripting.md` (переименование WAV→SPW), `defects.md` (№105, №233
  — отсутствующие спрайты), `_posts/2019-12-21-Sprites-finally-render-correctly.md`.
