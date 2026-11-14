#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
spw_parser.py — эталонный парсер мультикадровых спрайтов M.A.X. (ImageMultiHeader).

ВАЖНО: имя «.SPW» обманчиво (см. max-original-dasm/formats/spw.md §1).
В каталоге игры 272 файла *.SPW — это RIFF/WAVE-звуки (PCM 22050 Гц, моно,
16 бит), а НЕ спрайты. Спрайты лежат внутри MAX.RES как ресурсы с тегами
COMMTWR..WALDO (юниты/здания) и S_* (тени). Поэтому парсер понимает два входа:

  1) RIFF/WAVE *.SPW — это звук. info печатает параметры WAV и явно сообщает,
     что кадров спрайта в файле нет; frames()/frame()/dump для него бросают
     SpwError (а не молча возвращают пустоту).
  2) Тело спрайта ImageMultiHeader — либо ресурс RES-архива, заданный как
     "MAX.RES:TAG" (или через --tag TAG), либо отдельный файл, полученный
     извлечением ресурса (res_parser.py extract). В последнем случае тип
     (спрайт/тень) определяется по содержимому — см. §4.3 и примечание ниже.

Формат (spw.md §3, §4):
  * u16 image_count; image_count × u32 frame_offset (от начала спрайта);
  * кадр: 4 × i16 (width, height, hotx, hoty) + height × u32 row_offset
    (тоже от начала спрайта); всё little-endian, без выравнивания;
  * строка спрайта: прогоны {u8 skip, u8 count, count байт палитровых
    индексов}, терминатор 0xFF; прозрачные пиксели в файле не хранятся;
  * строка тени: прогоны {u8 skip, u8 count} БЕЗ байтов пикселей;
  * сигнатуры и поля флагов у спрайта нет (§3.4).

Прозрачность в API: None. Тень не содержит палитровых индексов, поэтому её
затемняемые пиксели кодируются сентинелом SHADOW_PIXEL = -1 (None — пропуск).
Это единственное отличие от «обычного» кадра; подробности — в docstring frame().

Палитра для dump внешняя: max-original/COLOR.PAL (§6.1), первые 768 байт —
256 × RGB по 6 бит на компонент, канал превращается в 8-битный умножением на 4
(так делает порт: color.cpp:82-84). Если COLOR.PAL не найден, используется
встроенная серая шкала (0..255) с предупреждением. Командные цвета (слоты
32–39 и 48–63, §6.2) по умолчанию НЕ перекрашиваются; для перекраски задайте
--team red|green|blue|gray|derelict.

Использование:
    python spw_parser.py <input> info [--tag NAME] [--sprite|--shadow]
    python spw_parser.py <input> dump <frame_id> <out.ppm>
                                [--tag NAME] [--sprite|--shadow]
                                [--palette PATH|gray] [--team NAME]
                                [--bg RRGGBB] [--shadow-color RRGGBB]
    python spw_parser.py <input> verify [--tag NAME] [--sprite|--shadow]

<input> — путь к файлу; для спрайта из RES-архива: "MAX.RES:TAG".
Примеры:
    python spw_parser.py max-original/TANK1.SPW info
    python spw_parser.py max-original/MAX.RES:TANK info
    python spw_parser.py max-original/MAX.RES:TANK dump 0 tank0.ppm
    python spw_parser.py max-original/MAX.RES:S_TANK dump 0 s_tank0.ppm
    python spw_parser.py max-original/MAX.RES:TANK dump 0 tank0.ppm --team red

Коды возврата: 0 — успех; 1 — ошибка файла/формата; 2 — ошибка аргументов.
"""

import os
import re
import struct
import sys

# --------------------------------------------------------------- константы (§3, §6)
RES_SIGNATURE = b"RES0"          # сигнатура RES-контейнера (formats/res.md §1.1)
RES_HEADER_SIZE = 12             # sizeof(res_header)
RES_ENTRY_SIZE = 16              # sizeof(res_index)

WAVE_RIFF = b"RIFF"
WAVE_FORM = b"WAVE"

PALETTE_SIZE = 256               # PALETTE_SIZE (color.h)
PALETTE_STRIDE = 3               # RGB
PALETTE_BYTES = PALETTE_SIZE * PALETTE_STRIDE  # 768
COLOR_PAL_SIZE = 33536           # COLOR.PAL: 768 палитры + 32768 RGB555-таблицы (§6.1)

SHADOW_PIXEL = -1                # «затемняемый» пиксель тени (палитрового индекса нет)
ROW_TERMINATOR = 0xFF
MAX_IMAGE_COUNT = 4096           # предохранитель от мусора; реально максимум 208
MAX_DIMENSION = 4096             # предохранитель; реально максимум 260×192
MAX_FRAME_PIXELS = 16_000_000    # предохранитель от гигантских кадров

# Командные цвета (§6.2, ResourceManager_BuildColorTables).
TEAM_COLORS = {
    "red": (56, 57, 58, 59, 60, 61, 62, 63),
    "green": (32, 33, 34, 35, 36, 37, 38, 39),
    "blue": (48, 49, 50, 51, 52, 53, 54, 55),
    "gray": (255, 161, 172, 169, 216, 213, 212, 207),
    "derelict": (216, 215, 214, 213, 212, 211, 210, 209),
}


class SpwError(Exception):
    """Ошибка чтения/разбора спрайта или аргументов (некорректный формат)."""


# ---------------------------------------------------------------- структуры (§3)
class SpriteFrame:
    """Заголовок одного кадра ImageMultiFrameHeader + предел его данных.

    Поля: index — номер кадра; offset — смещение заголовка кадра от начала
    спрайта; width/height/hotx/hoty — 4 × i16; row_offsets — height абсолютных
    смещений строк от начала спрайта; limit — конец области кадра (начало
    следующего кадра или конец ресурса) — предел, за который декодер не читает.
    """

    __slots__ = ("index", "offset", "width", "height", "hotx", "hoty",
                 "row_offsets", "limit")

    def __init__(self, index, offset, width, height, hotx, hoty,
                 row_offsets, limit):
        self.index = index
        self.offset = offset
        self.width = width
        self.height = height
        self.hotx = hotx
        self.hoty = hoty
        self.row_offsets = row_offsets
        self.limit = limit

    def __repr__(self):
        return ("SpriteFrame(%d, @0x%X, %dx%d, hot=(%d,%d), rows=%d)"
                % (self.index, self.offset, self.width, self.height,
                   self.hotx, self.hoty, len(self.row_offsets)))


# -------------------------------------------------------- чтение ресурса из RES
def _read_res_resource(path, tag):
    """Читает тело ресурса tag из RES-архива (formats/res.md §1).

    Возвращает (data, info, warnings), где info — словарь с полями archive,
    tag, offset, size, index. Тег сравнивается как C-строка (обрезка по NUL);
    при отсутствии точного совпадения допускается одно регистронезависимое
    совпадение (с предупреждением), как подсказка пользователю.
    """
    file_size = os.path.getsize(path)
    warnings = []
    with open(path, "rb") as fp:
        header = fp.read(RES_HEADER_SIZE)
        if len(header) < RES_HEADER_SIZE:
            raise SpwError("файл слишком короткий для заголовка RES (%d байт)"
                           % len(header))
        if header[0:4] != RES_SIGNATURE:
            raise SpwError(
                "файл %s не является RES-архивом: ожидалась сигнатура 'RES0', "
                "получено %r" % (path, header[0:4].decode("latin-1")))
        table_offset, table_size = struct.unpack_from("<II", header, 4)
        if table_size % RES_ENTRY_SIZE:
            warnings.append(
                "размер таблицы RES %d не кратен %d — хвост отброшен"
                % (table_size, RES_ENTRY_SIZE))
        if table_offset < RES_HEADER_SIZE or \
                table_offset + table_size > file_size:
            raise SpwError(
                "таблица RES выходит за границы файла: offset=0x%X size=0x%X, "
                "файл %d байт" % (table_offset, table_size, file_size))
        fp.seek(table_offset)
        table = fp.read(table_size)
        if len(table) != table_size:
            raise SpwError("таблица RES обрезана: прочитано %d из %d байт"
                           % (len(table), table_size))

        entry = None
        names = []
        for i in range(table_size // RES_ENTRY_SIZE):
            pos = i * RES_ENTRY_SIZE
            name = table[pos:pos + 8].split(b"\x00", 1)[0].decode("latin-1")
            data_offset, data_size = struct.unpack_from("<II", table, pos + 8)
            names.append(name)
            if name == tag and entry is None:
                entry = (i, name, data_offset, data_size)

        if entry is None:
            low = tag.lower()
            close = [n for n in names if n.lower() == low]
            if len(close) == 1:
                idx = names.index(close[0])
                pos = idx * RES_ENTRY_SIZE
                data_offset, data_size = struct.unpack_from("<II", table, pos + 8)
                entry = (idx, close[0], data_offset, data_size)
                warnings.append(
                    "тег %r не найден точно; использован %r (регистр отличается)"
                    % (tag, close[0]))
            else:
                hint = ""
                if close:
                    hint = " (есть варианты: %s)" % ", ".join(close[:4])
                raise SpwError("ресурс %r не найден в %s%s"
                               % (tag, path, hint))

        index, real_tag, data_offset, data_size = entry
        if data_offset + data_size > file_size:
            raise SpwError(
                "ресурс %r (%d) выходит за конец файла: 0x%X+%d > %d"
                % (real_tag, index, data_offset, data_size, file_size))
        fp.seek(data_offset)
        data = fp.read(data_size)
        if len(data) != data_size:
            raise SpwError("ресурс %r обрезан: прочитано %d из %d байт"
                           % (real_tag, len(data), data_size))

    info = {
        "archive": path,
        "tag": real_tag,
        "index": index,
        "offset": data_offset,
        "size": data_size,
    }
    return data, info, warnings


# ------------------------------------------------------------------- палитра §6
def _read_palette_file(path):
    """Читает COLOR.PAL: первые 768 байт, 6 бит/компонент → ×4 (color.cpp:82)."""
    with open(path, "rb") as fp:
        raw = fp.read()
    if len(raw) < PALETTE_BYTES:
        raise SpwError(
            "палитра %s слишком короткая: %d байт (нужно минимум %d)"
            % (path, len(raw), PALETTE_BYTES))
    palette = [
        (raw[i * 3] * 4, raw[i * 3 + 1] * 4, raw[i * 3 + 2] * 4)
        for i in range(PALETTE_SIZE)
    ]
    note = ""
    if len(raw) >= COLOR_PAL_SIZE:
        note = "; RGB555-таблица (%d байт) не используется" % (len(raw) - PALETTE_BYTES)
    elif len(raw) > PALETTE_BYTES:
        note = "; лишние %d байт не используются" % (len(raw) - PALETTE_BYTES)
    description = ("%s (256×RGB, 6 бит/компонент → ×4, §6.1%s)"
                   % (path, note))
    return palette, description


def _gray_palette():
    """Резервная палитра: серая шкала 0..255 (не палитра игры, но валидный PPM)."""
    return [(i, i, i) for i in range(PALETTE_SIZE)]


def _palette_candidates(spw):
    """Кандидаты COLOR.PAL: рядом с входом/архивом, в CWD, в каталоге игры."""
    here = os.path.dirname(os.path.abspath(
        spw.resource["archive"] if spw.resource else spw.path))
    script_dir = os.path.dirname(os.path.abspath(__file__))
    return [
        os.path.join(here, "COLOR.PAL"),
        os.path.join(os.getcwd(), "COLOR.PAL"),
        os.path.join(script_dir, "..", "..", "max-original", "COLOR.PAL"),
    ]


def resolve_palette(spw, explicit=None):
    """Возвращает (palette, description) для dump.

    Явный путь: --palette PATH; --palette gray — встроенная серая шкала.
    Иначе ищем COLOR.PAL рядом с файлом/архивом, в текущем каталоге, затем
    в <репозиторий>/max-original/COLOR.PAL. Если не нашли — серая шкала с
    предупреждением (см. docstring модуля).
    """
    if explicit:
        if explicit.lower() in ("gray", "grey"):
            return _gray_palette(), "встроенная серая шкала (--palette gray)"
        if not os.path.isfile(explicit):
            raise SpwError("файл палитры не найден: %s" % explicit)
        return _read_palette_file(explicit)

    for candidate in _palette_candidates(spw):
        if os.path.isfile(candidate):
            return _read_palette_file(candidate)

    print("ПРЕДУПРЕЖДЕНИЕ: COLOR.PAL не найден (искали: %s); используется "
          "встроенная серая шкала. Для правильных цветов укажите "
          "--palette max-original/COLOR.PAL (§6.1)."
          % ", ".join(_palette_candidates(spw)), file=sys.stderr)
    return _gray_palette(), "встроенная серая шкала (COLOR.PAL не найден)"


def build_team_lut(team):
    """LUT команды (§6.2): слоты 32–39 и 48–63 → 8 цветов команды, цикл j&7.

    Возвращает список 256 индексов или None для 'none'/пустого значения.
    """
    if not team or team.lower() in ("none", "off"):
        return None
    key = team.lower()
    if key not in TEAM_COLORS:
        raise ValueError(
            "неизвестный цвет команды %r (допустимо: none, %s)"
            % (team, ", ".join(sorted(TEAM_COLORS))))
    colors = TEAM_COLORS[key]
    lut = list(range(PALETTE_SIZE))
    # Как ResourceManager_BuildColorTables: j=0..7 → слоты 32..39,
    # j=8..23 → слоты 48..63 (16 слотов, цвета повторяются дважды, j & 7).
    for j in range(8):
        lut[32 + j] = colors[j & 7]
    for j in range(16):
        lut[48 + j] = colors[j & 7]
    return lut


def parse_rgb(text, what):
    """'#RRGGBB' или 'RRGGBB' -> (r, g, b)."""
    raw = text.strip().lstrip("#")
    if not re.fullmatch(r"[0-9A-Fa-f]{6}", raw):
        raise ValueError("%s: ожидался цвет RRGGBB, получено %r" % (what, text))
    return (int(raw[0:2], 16), int(raw[2:4], 16), int(raw[4:6], 16))


# ------------------------------------------------------------------ SpwFile
class SpwFile:
    """Спрайт M.A.X. (ImageMultiHeader) или RIFF/WAVE-файл с расширением .SPW.

    Конструктор:
        SpwFile(path)                  — отдельный файл (тело спрайта или WAV)
        SpwFile(path, tag="TANK")      — ресурс TANK из RES-архива path
        SpwFile(path, mode="shadow")   — принудительный режим строк

    Атрибуты:
        kind            "sprite" | "wav";
        image_count     число кадров (0 для WAV);
        frames_meta     list[SpriteFrame] (для WAV пуст);
        warnings        некритичные расхождения с форматом;
        mode            "sprite" | "shadow" (для спрайта);
        mode_source     как определён режим;
        resource        словарь источника в RES или None;
        wav             параметры WAV или None;
        signature       b"RIFF/WAVE" для WAV, иначе None (у спрайта сигнатуры нет).

    API:
        frames()        -> [(id, w, h, pixels), ...]; pixels — list[list[int|None]];
        frame(id)       -> pixels одного кадра;
        frame_details(id) -> (pixels, stats);
        verify()        -> список проблем (пустой — всё в порядке).
    """

    def __init__(self, path, tag=None, mode=None):
        self.path = os.fspath(path)
        self.resource_tag = tag
        self.resource = None
        self._mode_arg = self._normalize_mode(mode)

        self.warnings = []
        self._warning_set = set()

        load_warnings = []
        if tag is not None:
            data, info, load_warnings = _read_res_resource(self.path, tag)
            self.resource = info
            self.resource_tag = info["tag"]
        else:
            with open(self.path, "rb") as fp:
                head = fp.read(4)
                if head == RES_SIGNATURE:
                    raise SpwError(
                        "это RES-архив, а не спрайт: укажите ресурс как "
                        "%s:<TAG> (например, %s:TANK) или --tag TAG"
                        % (self.path, os.path.basename(self.path)))
                data = head + fp.read()

        self._init_body(data)
        for warning in load_warnings:
            self._warn(warning)

    @classmethod
    def from_bytes(cls, data, name="<memory>", mode=None):
        """Создаёт SpwFile из готового тела ресурса (без файла)."""
        obj = cls.__new__(cls)
        obj.path = name
        obj.resource_tag = None
        obj.resource = None
        obj._mode_arg = cls._normalize_mode(mode)
        obj._init_body(bytes(data))
        return obj

    # ------------------------------------------------------------------ разбор
    @staticmethod
    def _normalize_mode(mode):
        if mode is None:
            return None
        mode = mode.lower()
        if mode in ("auto", ""):
            return None
        if mode not in ("sprite", "shadow"):
            raise ValueError("режим должен быть sprite, shadow или auto")
        return mode

    def _init_body(self, data):
        self.data = data
        self.file_size = len(data)
        self.warnings = []
        self._warning_set = set()
        self.kind = None
        self.signature = None
        self.wav = None
        self.image_count = 0
        self.frames_meta = []
        self.mode = None
        self.mode_source = None
        self.first_offset_ok = False
        self.offsets_sorted = False
        self._cache = {}
        self._cache_stats = {}

        if data[0:4] == WAVE_RIFF:
            if len(data) < 12:
                raise SpwError(
                    "RIFF-файл короче 12 байт (%d): нет формы WAVE" % len(data))
            if data[8:12] != WAVE_FORM:
                raise SpwError(
                    "RIFF-файл, но форма %r, а не 'WAVE'"
                    % data[8:12].decode("latin-1"))
            self._parse_wav()
        else:
            self._parse_sprite()

    def _warn(self, message):
        if message not in self._warning_set:
            self._warning_set.add(message)
            self.warnings.append(message)

    # ------------------------------------------------------------------ WAV §8.5
    def _parse_wav(self):
        self.kind = "wav"
        self.signature = b"RIFF/WAVE"
        data = self.data
        size = self.file_size

        riff_size = struct.unpack_from("<I", data, 4)[0]
        if riff_size + 8 != size:
            self._warn(
                "RIFF size=%d (конец 0x%X) не совпадает с размером файла %d — "
                "файл обрезан или дополнен" % (riff_size, riff_size + 8, size))

        chunks = []
        fmt = None
        data_chunk = None
        smpl_size = None
        pos = 12
        while pos + 8 <= size:
            chunk_id = data[pos:pos + 4]
            chunk_size = struct.unpack_from("<I", data, pos + 4)[0]
            payload = pos + 8
            if payload + chunk_size > size:
                self._warn(
                    "чанк %r на 0x%X обрезан: заявлено %d байт, до конца файла %d"
                    % (chunk_id.decode("latin-1"), pos, chunk_size, size - payload))
                chunk_size = size - payload
            chunks.append((chunk_id, chunk_size, payload))
            if chunk_id == b"fmt ":
                if chunk_size < 16:
                    self._warn("fmt-чанк короче 16 байт (%d)" % chunk_size)
                else:
                    (audio_format, channels, sample_rate, byte_rate,
                     block_align, bits) = struct.unpack_from("<HHIIHH", data, payload)
                    fmt = {
                        "format": audio_format,
                        "channels": channels,
                        "sample_rate": sample_rate,
                        "byte_rate": byte_rate,
                        "block_align": block_align,
                        "bits": bits,
                    }
            elif chunk_id == b"data":
                data_chunk = (payload, chunk_size)
            elif chunk_id == b"smpl":
                smpl_size = chunk_size
            pos = payload + chunk_size + (chunk_size & 1)

        if fmt is None:
            self._warn("в WAV нет fmt-чанка")
        if data_chunk is None:
            self._warn("в WAV нет data-чанка")
        if pos < size:
            self._warn("после последнего чанка %d лишних байт" % (size - pos))

        data_size = data_chunk[1] if data_chunk else 0
        block_align = fmt["block_align"] if fmt else 0
        sample_frames = data_size // block_align if block_align else 0
        duration = (sample_frames / float(fmt["sample_rate"])
                    if fmt and fmt["sample_rate"] else 0.0)

        self.wav = {
            "riff_size": riff_size,
            "fmt": fmt,
            "data_size": data_size,
            "data_offset": data_chunk[0] if data_chunk else None,
            "smpl_size": smpl_size,
            "chunks": [(cid.decode("latin-1"), csize) for cid, csize, _ in chunks],
            "sample_frames": sample_frames,
            "duration": duration,
        }

    # -------------------------------------------------------------- спрайт §3–4
    def _parse_sprite(self):
        data = self.data
        size = self.file_size
        if size < 2:
            raise SpwError("файл слишком короткий для ImageMultiHeader: %d байт"
                           % size)

        # Ресурс-ссылка (например, TANK1 = "tank1.spw\0", res.md §5) — не спрайт.
        if size <= 64 and data.endswith(b"\x00") and \
                all(32 <= byte < 127 for byte in data[:-1]):
            raise SpwError(
                "это ресурс-ссылка на внешний файл %r, а не спрайт (spw.md §1, "
                "res.md §5)" % data[:-1].decode("latin-1"))

        self.image_count = struct.unpack_from("<H", data, 0)[0]
        if self.image_count == 0:
            raise SpwError("image_count = 0: в спрайте нет кадров (§9.13)")
        if self.image_count > MAX_IMAGE_COUNT:
            raise SpwError(
                "image_count = %d неправдоподобно велико (максимум формата "
                "в реальных файлах — 208; предохранитель %d)"
                % (self.image_count, MAX_IMAGE_COUNT))

        table_end = 2 + 4 * self.image_count
        if table_end > size:
            raise SpwError(
                "таблица смещений кадров выходит за конец файла: нужно %d байт, "
                "файл %d" % (table_end, size))
        offsets = list(struct.unpack_from("<%dI" % self.image_count, data, 2))
        self.first_offset_ok = (offsets[0] == table_end)
        if not self.first_offset_ok:
            self._warn(
                "frame_offset[0] = 0x%X != 2 + 4*image_count = 0x%X — это "
                "инвариант реальных файлов, а не формата (§9.3); доверяем таблице"
                % (offsets[0], table_end))
        self.offsets_sorted = offsets == sorted(offsets)
        if not self.offsets_sorted:
            self._warn("смещения кадров не возрастают: %s" % offsets[:8])
        if len(set(offsets)) != self.image_count:
            self._warn("в таблице кадров есть повторяющиеся смещения")

        frames = []
        for index, offset in enumerate(offsets):
            if offset + 8 > size:
                raise SpwError(
                    "кадр %d: заголовок по смещению 0x%X выходит за конец файла "
                    "(0x%X)" % (index, offset, size))
            width, height, hotx, hoty = struct.unpack_from("<hhhh", data, offset)
            if width <= 0 or height <= 0:
                raise SpwError(
                    "кадр %d: некорректный размер %d×%d (должны быть > 0)"
                    % (index, width, height))
            if width > MAX_DIMENSION or height > MAX_DIMENSION or \
                    width * height > MAX_FRAME_PIXELS:
                raise SpwError(
                    "кадр %d: неправдоподобный размер %d×%d (предохранитель "
                    "%d×%d / %d пикселей)"
                    % (index, width, height, MAX_DIMENSION, MAX_DIMENSION,
                       MAX_FRAME_PIXELS))

            rows_end = offset + 8 + 4 * height
            if rows_end > size:
                raise SpwError(
                    "кадр %d: таблица смещений строк (%d × 4 байта) выходит за "
                    "конец файла (конец 0x%X, файл 0x%X)"
                    % (index, height, rows_end, size))
            row_offsets = list(
                struct.unpack_from("<%dI" % height, data, offset + 8))
            for row_index, row_offset in enumerate(row_offsets):
                if row_offset >= size:
                    raise SpwError(
                        "кадр %d, строка %d: смещение 0x%X за концом файла (0x%X)"
                        % (index, row_index, row_offset, size))
                if row_offset < rows_end:
                    raise SpwError(
                        "кадр %d, строка %d: смещение 0x%X указывает внутрь "
                        "заголовка кадра (данные строк начинаются с 0x%X)"
                        % (index, row_index, row_offset, rows_end))
            if row_offsets != sorted(row_offsets):
                self._warn(
                    "кадр %d: смещения строк не возрастают" % index)

            if self.offsets_sorted and index + 1 < self.image_count:
                limit = offsets[index + 1]
                if limit <= offset:
                    limit = size
            else:
                limit = size
            frames.append(SpriteFrame(index, offset, width, height, hotx, hoty,
                                      row_offsets, limit))

        self.frames_meta = frames
        self.kind = "sprite"

        if self._mode_arg is not None:
            self.mode = self._mode_arg
            self.mode_source = "задан явно (--sprite/--shadow)"
        elif self.resource_tag and self.resource_tag.upper().startswith("S_"):
            self.mode = "shadow"
            self.mode_source = "тег ресурса S_* (§3.4, §9.6)"
        elif self.resource_tag:
            self.mode = "sprite"
            self.mode_source = "тег ресурса (не S_*, §3.4)"
        else:
            self._detect_mode()

    def _detect_mode(self):
        """Определяет режим по содержимому: полный разбор в sprite, затем shadow.

        Для всех 168 реальных спрайтов MAX.RES проверка однозначна: теневые
        строки не проходят проверку sprite-режима (Σ(skip+count) > width), а
        обычные — не проходят shadow-режим. Тем не менее формат не хранит флаг
        (§3.4), поэтому для ресурсов S_* всё равно надёжнее тег, а для отдельного
        файла можно задать --sprite/--shadow.
        """
        sprite_error = self._probe_mode("sprite")
        if sprite_error is None:
            self.mode = "sprite"
            self.mode_source = ("автоопределение по содержимому: все строки имеют "
                                "байты палитровых индексов")
            return
        shadow_error = self._probe_mode("shadow")
        if shadow_error is None:
            self.mode = "shadow"
            self.mode_source = ("автоопределение по содержимому: строки — пары "
                                "счётчиков без пиксельных байтов")
            return
        raise SpwError(
            "не удалось прочитать кадры ни как спрайт, ни как тень. "
            "sprite: %s; shadow: %s. Файл повреждён или это не ImageMultiHeader"
            % (sprite_error, shadow_error))

    def _probe_mode(self, mode):
        """Возвращает None, если весь спрайт декодируется в mode, иначе текст ошибки."""
        for index in range(self.image_count):
            try:
                self._decode_frame(index, mode=mode, collect_warnings=False)
            except SpwError as exc:
                return str(exc)
        return None

    # --------------------------------------------------------------- декодер §4
    def _decode_row(self, row_offset, frame, mode):
        """Декодирует одну строку. Возвращает (row, runs, x, opaque_count)."""
        width = frame.width
        limit = frame.limit
        row = [None] * width
        pos = row_offset
        x = 0
        runs = 0
        opaque = 0

        while True:
            if pos >= limit:
                raise SpwError(
                    "строка не завершена 0xFF: позиция 0x%X дошла до предела "
                    "кадра 0x%X" % (pos, limit))
            skip = self.data[pos]
            if skip == ROW_TERMINATOR:
                return row, runs, x, opaque
            if pos + 1 >= limit:
                raise SpwError(
                    "строка обрывается после байта skip=0x%02X (позиция 0x%X, "
                    "предел 0x%X)" % (skip, pos, limit))
            count = self.data[pos + 1]
            pos += 2
            runs += 1

            if x + skip > width:
                raise SpwError(
                    "пропуск %d при x=%d выходит за width=%d"
                    % (skip, x, width))
            x += skip

            if mode == "shadow":
                if x + count > width:
                    raise SpwError(
                        "затемнение %d пикселей при x=%d выходит за width=%d"
                        % (count, x, width))
                for i in range(count):
                    row[x + i] = SHADOW_PIXEL
                x += count
                opaque += count
            else:
                if x + count > width:
                    raise SpwError(
                        "непрозрачных пикселей %d при x=%d выходит за width=%d"
                        % (count, x, width))
                if pos + count > limit:
                    raise SpwError(
                        "полезная нагрузка прогона (%d байт с 0x%X) выходит за "
                        "предел кадра 0x%X" % (count, pos, limit))
                for i in range(count):
                    row[x + i] = self.data[pos + i]
                pos += count
                x += count
                opaque += count

    def _decode_frame(self, index, mode=None, collect_warnings=True):
        """Декодирует кадр; возвращает (pixels, stats)."""
        mode = mode or self.mode
        frame = self.frames_meta[index]
        pixels = []
        runs = 0
        opaque = 0
        short_rows = 0
        full_rows = 0
        min_sum = frame.width
        max_sum = 0

        for y, row_offset in enumerate(frame.row_offsets):
            try:
                row, row_runs, x, row_opaque = self._decode_row(
                    row_offset, frame, mode)
            except SpwError as exc:
                raise SpwError("кадр %d, строка %d: %s" % (index, y, exc))
            pixels.append(row)
            runs += row_runs
            opaque += row_opaque
            if x < frame.width:
                short_rows += 1
            else:
                full_rows += 1
            if x < min_sum:
                min_sum = x
            if x > max_sum:
                max_sum = x

        if len(pixels) != frame.height:
            raise SpwError("кадр %d: декодировано %d строк вместо %d"
                           % (index, len(pixels), frame.height))

        stats = {
            "width": frame.width,
            "height": frame.height,
            "runs": runs,
            "opaque": opaque,
            "short_rows": short_rows,
            "full_rows": full_rows,
            "min_sum": min_sum,
            "max_sum": max_sum,
        }
        if collect_warnings:
            if max_sum == 0:
                self._warn("кадр %d: все строки пустые (0 пикселей)" % index)
        return pixels, stats

    # -------------------------------------------------------------- публичное API
    def frame(self, index):
        """Пиксели кадра: list[list[int|None]], row-major.

        None — прозрачный пиксель (структурная прозрачность, §4.1/§4.2).
        Для кадра тени (mode == "shadow") затемняемые пиксели равны
        SHADOW_PIXEL (-1): палитрового индекса у тени нет (§4.3).
        Возвращаемый список кэшируется — не модифицируйте его.
        """
        if self.kind != "sprite":
            raise SpwError(
                "%s — это RIFF/WAVE-звук, кадров спрайта в нём нет (§1); "
                "спрайт лежит в MAX.RES: укажите MAX.RES:<TAG>" % self.path)
        index = int(index)
        if not 0 <= index < self.image_count:
            raise IndexError("кадр %d вне диапазона 0..%d"
                             % (index, self.image_count - 1))
        if index not in self._cache:
            pixels, stats = self._decode_frame(index)
            self._cache[index] = pixels
            self._cache_stats[index] = stats
        return self._cache[index]

    def frame_details(self, index):
        """Возвращает (pixels, stats) — пиксели и статистика декодирования кадра."""
        pixels = self.frame(index)
        return pixels, dict(self._cache_stats[int(index)])

    def frames(self):
        """Все кадры: [(id, width, height, pixels), ...] (см. frame())."""
        if self.kind != "sprite":
            raise SpwError(
                "%s — это RIFF/WAVE-звук, кадров спрайта в нём нет (§1)" % self.path)
        result = []
        for index, frame in enumerate(self.frames_meta):
            result.append((index, frame.width, frame.height, self.frame(index)))
        return result

    def verify(self):
        """Проверяет инварианты §3/§9.13. Возвращает список проблем."""
        problems = []
        if self.kind != "sprite":
            problems.append("файл не является спрайтом (kind=%r)" % self.kind)
            return problems
        if self.image_count < 1:
            problems.append("image_count=%d < 1" % self.image_count)
        if not self.first_offset_ok:
            problems.append(
                "frame_offset[0]=0x%X != 2+4*image_count=0x%X (§9.3)"
                % (self.frames_meta[0].offset, 2 + 4 * self.image_count))
        if not self.offsets_sorted:
            problems.append("смещения кадров не возрастают")
        for index in range(self.image_count):
            try:
                self.frame_details(index)
            except SpwError as exc:
                problems.append(str(exc))
        return problems

    # ------------------------------------------------------------------ утилиты
    def input_name(self):
        if self.resource:
            return "%s:%s" % (self.path, self.resource["tag"])
        return self.path


# -------------------------------------------------------------------- PPM
def write_ppm(out_path, pixels, palette, team_lut=None,
              background=(255, 0, 255), shadow_color=(0, 0, 0)):
    """Пишет кадр в бинарный PPM (P6).

    None — прозрачный пиксель → background (по умолчанию пурпурный chroma-key);
    SHADOW_PIXEL — затемняемый пиксель тени → shadow_color (по умолчанию чёрный);
    иначе палитровый индекс (после необязательного LUT команды) → palette[idx].
    """
    height = len(pixels)
    width = len(pixels[0]) if height else 0
    out = bytearray()
    out += ("P6\n%d %d\n255\n" % (width, height)).encode("ascii")
    for row in pixels:
        if len(row) != width:
            raise SpwError("внутренняя ошибка: строка не равна %d пикселям" % width)
        for value in row:
            if value is None:
                rgb = background
            elif value == SHADOW_PIXEL:
                rgb = shadow_color
            else:
                index = team_lut[value] if team_lut is not None else value
                rgb = palette[index]
            out += bytes(rgb)
    with open(out_path, "wb") as fp:
        fp.write(out)
    return width, height


# ---------------------------------------------------------------- форматирование
def _fmt_int(value):
    """1234567 -> '1 234 567'."""
    return "%d" % value if abs(value) < 1000 else \
        "{:,}".format(value).replace(",", " ")


def _hex_bytes(raw):
    return " ".join("%02X" % b for b in raw)


def _fmt_duration(seconds):
    return "%.4f с" % seconds


AUDIO_FORMAT_NAMES = {
    1: "PCM",
    3: "IEEE float",
    0xFFFE: "WAVE_FORMAT_EXTENSIBLE",
}


# ---------------------------------------------------------------- CLI-команды
def cmd_info(spw, palette_hint=None):
    """Печатает заголовок/кадры/проверку спрайта либо параметры WAV."""
    _force_utf8_streams()
    print("Файл:        %s" % spw.input_name())
    if spw.resource:
        info = spw.resource
        print("Источник:    RES-архив %s, запись #%d, тег %r, offset 0x%X, %s байт"
              % (info["archive"], info["index"], info["tag"], info["offset"],
                 _fmt_int(info["size"])))
    print("Размер:      %s байт (0x%X)" % (_fmt_int(spw.file_size), spw.file_size))

    if spw.kind == "wav":
        wav = spw.wav
        fmt = wav["fmt"]
        print("Тип:         RIFF/WAVE — звуковой файл, НЕ спрайт (spw.md §1, §8.5)")
        print("Сигнатура:   %r + %r (%s / %s)"
              % (WAVE_RIFF.decode("ascii"), WAVE_FORM.decode("ascii"),
                 _hex_bytes(WAVE_RIFF), _hex_bytes(WAVE_FORM)))
        print("RIFF size:   %s байт (конец 0x%X), файл %s байт — %s"
              % (_fmt_int(wav["riff_size"]), wav["riff_size"] + 8,
                 _fmt_int(spw.file_size),
                 "сходится" if wav["riff_size"] + 8 == spw.file_size
                 else "НЕ СХОДИТСЯ"))
        if fmt:
            format_name = AUDIO_FORMAT_NAMES.get(fmt["format"], "?")
            print("fmt:         %s (%d), каналов %d, %s Гц, %d бит, "
                  "block align %d, byte rate %s"
                  % (format_name, fmt["format"], fmt["channels"],
                     _fmt_int(fmt["sample_rate"]), fmt["bits"],
                     fmt["block_align"], _fmt_int(fmt["byte_rate"])))
        else:
            print("fmt:         отсутствует")
        print("data:        %s байт" % _fmt_int(wav["data_size"]))
        print("Кадров (сэмплов): %s (%s / %d), длительность %s"
              % (_fmt_int(wav["sample_frames"]),
                 _fmt_int(wav["data_size"]),
                 fmt["block_align"] if fmt else 0,
                 _fmt_duration(wav["duration"])))
        print("smpl:        %s"
              % ("%s байт" % _fmt_int(wav["smpl_size"])
                 if wav["smpl_size"] is not None else "нет"))
        print("Чанки:       %s"
              % ", ".join("%s (%s)" % (cid.strip() or "?", _fmt_int(csize))
                          for cid, csize in wav["chunks"]))
        print("Кадров спрайта: 0 — ImageMultiHeader в файле нет (§1)")
        print("")
        print("Примечание:  для спрайтов используйте ресурсы MAX.RES "
              "(например, MAX.RES:TANK), а не *.SPW.")
        for warning in spw.warnings:
            print("ПРЕДУПРЕЖДЕНИЕ: %s" % warning, file=sys.stderr)
        return 0

    # ------------------------------------------------------------- спрайт
    print("Тип:         мультикадровый спрайт (ImageMultiHeader, §3)")
    first_bytes = spw.data[:8]
    print("Сигнатура:   отсутствует (§3.4); первые 8 байт: %s"
          % _hex_bytes(first_bytes))
    print("Кадров:      %d (u16 image_count; §3)" % spw.image_count)
    print("Режим строк: %s — %s"
          % ("shadow (маска затемнения, без байтов пикселей)"
             if spw.mode == "shadow" else "sprite (байты палитровых индексов)",
             spw.mode_source))
    print("frame_offset[0]: 0x%X (2 + 4×%d = 0x%X) — %s"
          % (spw.frames_meta[0].offset, spw.image_count,
             2 + 4 * spw.image_count,
             "инвариант выполняется" if spw.first_offset_ok else "НЕ СХОДИТСЯ"))

    show = min(8, spw.image_count)
    print("")
    print("Кадры (первые %d из %d):" % (show, spw.image_count))
    errors = []
    widths = []
    heights = []
    hotx_values = []
    hoty_values = []
    total_opaque = 0
    total_short = 0
    total_rows = 0
    for index in range(spw.image_count):
        frame = spw.frames_meta[index]
        widths.append(frame.width)
        heights.append(frame.height)
        hotx_values.append(frame.hotx)
        hoty_values.append(frame.hoty)
        try:
            _, stats = spw.frame_details(index)
        except SpwError as exc:
            errors.append((index, str(exc)))
            if index < show:
                print("  кадр %-3d: %d×%d, hot=(%d,%d) — ОШИБКА: %s"
                      % (index, frame.width, frame.height, frame.hotx,
                         frame.hoty, exc))
            continue
        total_opaque += stats["opaque"]
        total_short += stats["short_rows"]
        total_rows += stats["height"]
        if index < show:
            print("  кадр %-3d: %3d×%-3d hot=(%4d,%4d)  %s=%-6d  прогонов=%-5d  "
                  "строк короче width: %d/%d"
                  % (index, frame.width, frame.height, frame.hotx, frame.hoty,
                     "затемнено" if spw.mode == "shadow" else "непрозр.",
                     stats["opaque"], stats["runs"],
                     stats["short_rows"], stats["height"]))
    if spw.image_count > show:
        print("  …")
        print("  (остальные кадры: %d — проверены, см. сводку ниже)"
              % (spw.image_count - show))

    print("")
    print("Диапазон размеров: %d×%d … %d×%d; hot x %d..%d, y %d..%d"
          % (min(widths), min(heights), max(widths), max(heights),
             min(hotx_values), max(hotx_values),
             min(hoty_values), max(hoty_values)))
    print("Всего %s пикселей: %s; коротких строк (Σ(skip+count) < width): "
          "%s из %s — норма (§4.2)"
          % ("затемняемых" if spw.mode == "shadow" else "непрозрачных",
             _fmt_int(total_opaque), _fmt_int(total_short), _fmt_int(total_rows)))

    print("")
    if errors:
        print("Декодирование: ОШИБКИ в %d из %d кадров"
              % (len(errors), spw.image_count))
        for index, message in errors[:10]:
            print("  %s" % message)
        if len(errors) > 10:
            print("  … ещё %d" % (len(errors) - 10))
    else:
        print("Декодирование: все кадры (%d) прочитаны без ошибок"
              % spw.image_count)

    if palette_hint is not None:
        print("Палитра dump: %s" % palette_hint)
    else:
        print("Палитра dump: по умолчанию COLOR.PAL (max-original/COLOR.PAL, §6.1); "
              "индексы 32–39/48–63 — командные (§6.2).")

    for warning in spw.warnings:
        print("ПРЕДУПРЕЖДЕНИЕ: %s" % warning, file=sys.stderr)
    return 1 if errors else 0


def cmd_dump(spw, frame_id, out_path, palette_path=None,
             background=(255, 0, 255), shadow_color=(0, 0, 0), team=None):
    """Пишет кадр в PPM с внешней палитрой; печатает сводку."""
    _force_utf8_streams()
    if spw.kind != "sprite":
        raise SpwError(
            "%s — RIFF/WAVE-звук (§1), кадров спрайта нет; dump применим только "
            "к ресурсам MAX.RES (например, MAX.RES:TANK)" % spw.path)
    pixels, stats = spw.frame_details(frame_id)
    palette, palette_description = resolve_palette(spw, palette_path)
    team_lut = build_team_lut(team)
    width, height = write_ppm(out_path, pixels, palette, team_lut,
                              background, shadow_color)

    frame = spw.frames_meta[int(frame_id)]
    print("Файл:      %s" % spw.input_name())
    print("Кадр:      %d из %d, %d×%d, hot=(%d,%d)"
          % (frame_id, spw.image_count, frame.width, frame.height,
             frame.hotx, frame.hoty))
    print("Режим:     %s" % ("shadow (маска затемнения)" if spw.mode == "shadow"
                             else "sprite (палитровые индексы)"))
    print("Палитра:   %s" % palette_description)
    print("Команда:   %s"
          % ("перекраска LUT: %s (§6.2)" % team if team_lut is not None
             else "без перекраски (--team не задан)"))
    print("Прозрачных пикселей: %s → цвет #%02X%02X%02X"
          % (_fmt_int(stats["height"] * stats["width"] - stats["opaque"]),
             background[0], background[1], background[2]))
    if spw.mode == "shadow":
        print("Тень:      %s затемняемых пикселей → цвет #%02X%02X%02X "
              "(в игре это затемнение карты, §4.3)"
              % (_fmt_int(stats["opaque"]), shadow_color[0], shadow_color[1],
                 shadow_color[2]))
    header_len = len(("P6\n%d %d\n255\n" % (width, height)).encode("ascii"))
    print("PPM:       %d×%d, %s байт → %s"
          % (width, height, _fmt_int(header_len + width * height * 3), out_path))
    for warning in spw.warnings:
        print("ПРЕДУПРЕЖДЕНИЕ: %s" % warning, file=sys.stderr)


def cmd_verify(spw):
    """Проверяет инварианты и декодирование; печатает результат."""
    _force_utf8_streams()
    print("Файл:  %s (%s байт)" % (spw.input_name(), _fmt_int(spw.file_size)))
    if spw.kind != "sprite":
        print("Тип:   %s" % ("RIFF/WAVE-звук (§1), спрайтовых инвариантов нет"
                             if spw.kind == "wav" else str(spw.kind)))
        print("OK: файл корректен как WAV; кадров спрайта в нём нет.")
        return 0

    print("Тип:   ImageMultiHeader, image_count=%d, режим=%s"
          % (spw.image_count, spw.mode))
    problems = spw.verify()
    if problems:
        print("Проблемы (%d):" % len(problems))
        for problem in problems[:20]:
            print("  - %s" % problem)
        if len(problems) > 20:
            print("  … ещё %d" % (len(problems) - 20))
        return 1
    print("Инварианты §3/§9.13: в порядке; все кадры (%d) декодированы"
          % spw.image_count)
    for warning in spw.warnings:
        print("ПРЕДУПРЕЖДЕНИЕ: %s" % warning, file=sys.stderr)
    return 0


def _force_utf8_streams():
    """Вывод в UTF-8: при перенаправлении в файл/канал Python берёт cp1251."""
    for stream in (sys.stdout, sys.stderr):
        try:
            if (stream.encoding or "").lower().replace("-", "") != "utf8":
                stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass


USAGE = """Использование:
  python spw_parser.py <input> info [--tag NAME] [--sprite|--shadow]
  python spw_parser.py <input> dump <frame_id> <out.ppm>
                              [--tag NAME] [--sprite|--shadow]
                              [--palette PATH|gray] [--team NAME]
                              [--bg RRGGBB] [--shadow-color RRGGBB]
  python spw_parser.py <input> verify [--tag NAME] [--sprite|--shadow]

<input> — файл *.SPW (это WAV-звук, §1) или RES-архив с тегом: MAX.RES:TANK.
"""

VALUE_OPTIONS = {
    "--tag": "tag",
    "--palette": "palette",
    "--team": "team",
    "--bg": "bg",
    "--shadow-color": "shadow_color",
}
FLAG_OPTIONS = {
    "--sprite": "sprite",
    "--shadow": "shadow",
}


def _split_input(spec):
    """'MAX.RES:TANK' -> ('MAX.RES', 'TANK'); обычный путь -> (path, None)."""
    if os.path.isfile(spec):
        return spec, None
    if ":" in spec:
        base, tag = spec.rsplit(":", 1)
        if re.fullmatch(r"[A-Za-z0-9_]{1,8}", tag) and os.path.isfile(base):
            return base, tag
    return spec, None


def _parse_options(args):
    """Разбирает хвост argv: опции и позиционные аргументы."""
    options = {}
    positionals = []
    i = 0
    while i < len(args):
        arg = args[i]
        if arg in VALUE_OPTIONS:
            if i + 1 >= len(args):
                raise ValueError("у опции %s нет значения" % arg)
            options[VALUE_OPTIONS[arg]] = args[i + 1]
            i += 2
            continue
        if arg.startswith("--") and "=" in arg:
            name, value = arg.split("=", 1)
            if name in VALUE_OPTIONS:
                options[VALUE_OPTIONS[name]] = value
                i += 1
                continue
            raise ValueError("неизвестная опция %r" % name)
        if arg in FLAG_OPTIONS:
            if FLAG_OPTIONS[arg] in options:
                raise ValueError("опция %s уже задана" % arg)
            options[FLAG_OPTIONS[arg]] = True
            i += 1
            continue
        if arg.startswith("--"):
            raise ValueError("неизвестная опция %r" % arg)
        positionals.append(arg)
        i += 1
    if options.get("sprite") and options.get("shadow"):
        raise ValueError("нельзя одновременно задать --sprite и --shadow")
    return options, positionals


def main(argv):
    _force_utf8_streams()
    if len(argv) < 3:
        print(USAGE, file=sys.stderr)
        return 2

    spec = argv[1]
    command = argv[2].lower()
    try:
        options, positionals = _parse_options(argv[3:])
    except ValueError as exc:
        print("Ошибка аргументов: %s" % exc, file=sys.stderr)
        print(USAGE, file=sys.stderr)
        return 2

    path, spec_tag = _split_input(spec)
    if not os.path.isfile(path):
        print("Ошибка: файл не найден: %s" % path, file=sys.stderr)
        return 1

    tag = options.get("tag", spec_tag)
    if options.get("tag") and spec_tag and options["tag"] != spec_tag:
        print("ПРЕДУПРЕЖДЕНИЕ: тег %r из --tag перекрывает %r из имени входа"
              % (options["tag"], spec_tag), file=sys.stderr)
    mode = None
    if options.get("sprite"):
        mode = "sprite"
    elif options.get("shadow"):
        mode = "shadow"

    try:
        spw = SpwFile(path, tag=tag, mode=mode)
    except (SpwError, OSError) as exc:
        print("Ошибка: %s" % exc, file=sys.stderr)
        return 1

    try:
        if command == "info":
            if positionals:
                print("Ошибка: у info нет позиционных аргументов", file=sys.stderr)
                return 2
            if any(key in options for key in ("team", "bg", "shadow_color")):
                print("Ошибка: --team/--bg/--shadow-color относятся только к dump",
                      file=sys.stderr)
                return 2
            palette_hint = None
            if options.get("palette"):
                _, palette_hint = resolve_palette(spw, options["palette"])
            return cmd_info(spw, palette_hint)

        if command == "dump":
            if len(positionals) != 2:
                print("Ошибка: dump требует <frame_id> <out.ppm>", file=sys.stderr)
                return 2
            try:
                frame_id = int(positionals[0], 0)
            except ValueError:
                print("Ошибка: frame_id должен быть целым числом, получено %r"
                      % positionals[0], file=sys.stderr)
                return 2
            try:
                bg = parse_rgb(options.get("bg", "FF00FF"), "--bg")
                shadow_color = parse_rgb(options.get("shadow-color", "000000"),
                                         "--shadow-color")
            except ValueError as exc:
                print("Ошибка аргументов: %s" % exc, file=sys.stderr)
                return 2
            team = options.get("team")
            if team and team.lower() not in ("none", "off") and \
                    team.lower() not in TEAM_COLORS:
                print("Ошибка: неизвестный --team %r (допустимо: none, %s)"
                      % (team, ", ".join(sorted(TEAM_COLORS))), file=sys.stderr)
                return 2
            cmd_dump(spw, frame_id, positionals[1],
                     palette_path=options.get("palette"),
                     background=bg, shadow_color=shadow_color, team=team)
            return 0

        if command == "verify":
            if positionals:
                print("Ошибка: у verify нет позиционных аргументов", file=sys.stderr)
                return 2
            if any(key in options for key in ("team", "bg", "shadow_color",
                                              "palette")):
                print("Ошибка: verify не принимает --team/--bg/--shadow-color/"
                      "--palette", file=sys.stderr)
                return 2
            return cmd_verify(spw)

        print("Ошибка: неизвестная команда %r (info, dump, verify)" % command,
              file=sys.stderr)
        return 2
    except (SpwError, OSError, IndexError) as exc:
        print("Ошибка: %s" % exc, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
