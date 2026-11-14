#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
wrl_parser.py — эталонный парсер карт M.A.X. (формат WRL).

Формат описан в max-original-dasm/formats/wrl.md:
  * файл — линейная последовательность секций без сжатия и выравнивания,
    little-endian, структуры упакованы (§1);
  * заголовок: u8[3] сигнатура + u16 layer_count + Point[layer_count]
    (Point = {int16 x; int16 y}, 4 байта) — §1.1;
  * миникарта: ровно один слой W*H байт, индекс палитры (§1.2);
  * карта tile-id: W*H × u16 LE, row-major (§1.3);
  * u16 tile_count + tile_count × 64×64 байт пикселей (§1.4);
  * палитра 256×3 байта RGB, 8 бит на канал (§1.5);
  * pass-таблица: tile_count байт, значения 0..3 (§1.6).

SurfaceMap (§3.1):
    PassData = {1, 2, 4, 8} = {LAND, WATER, COAST, AIR}
    surface_map[i] = PassData[pass_table[tile_ids[i]]]

Особенности, унаследованные от эталонного загрузчика:
  * сигнатура информативна, но не является строгим признаком формата:
    у SNOW_1.WRL она битая — "DMO" (44 4D 4F), файл при этом полностью
    валиден (§1.1, §7.2). Парсер принимает "WRL" и известное исключение
    "DMO" (с предупреждением); любые другие 3 байта — ошибка с exit 1;
  * читается ровно один слой миникарты (W*H), независимо от layer_count;
  * используется только dims[0]; реальные файлы — layer_count=1, 112×112;
  * значения tile_id >= tile_count и байты pass-таблицы > 3 в C++ дают
    чтение за границей массива (UB). Здесь такие клетки получают
    SURFACE_TYPE_NONE (0), как в Rust-порте (setup.rs: derive_surface_map),
    а расхождение фиксируется в warnings/инвариантах (§7.6).

Использование:
    python wrl_parser.py <file.wrl> info
    python wrl_parser.py <file.wrl> dump-surface <out.txt> [hex|dec]
    python wrl_parser.py <file.wrl> verify          (доп. команда: инварианты §6)

Коды возврата: 0 — успех; 1 — ошибка файла/формата; 2 — ошибка аргументов.
"""

import hashlib
import os
import struct
import sys

# --------------------------------------------------------------- константы (§1)
SIGNATURE = b"WRL"                   # штатная сигнатура, 3 байта, без NUL
KNOWN_SIGNATURES = (b"WRL", b"DMO")  # DMO — известное исключение SNOW_1.WRL (§1.1)
SIGNATURE_LEN = 3

TILE_SIZE = 64                       # GFX_MAP_TILE_SIZE (gfx.hpp:28)
TILE_PIXELS = TILE_SIZE * TILE_SIZE  # 4096 байт на тайл
PALETTE_COLORS = 256                 # PALETTE_SIZE (color.h)
PALETTE_STRIDE = 3                   # RGB, 8 бит на канал
PALETTE_BYTES = PALETTE_COLORS * PALETTE_STRIDE  # 768

# Индексы pass-таблицы -> SURFACE_TYPE_* (enums.hpp:60-66, §3.1).
PASS_SURFACES = (0x1, 0x2, 0x4, 0x8)  # LAND, WATER, COAST, AIR

SURFACE_NAMES = {
    0x0: "NONE",
    0x1: "LAND",
    0x2: "WATER",
    0x4: "COAST",
    0x8: "AIR",
}

# Штатные размеры и число слоёв реальных файлов (§1.1, §6).
STANDARD_WIDTH = 112
STANDARD_HEIGHT = 112
MAX_CPP_LAYERS = 12   # Point map_layer_dimensions[12] в C++-порте (§1.1)

# Формула размера для штатного случая layer_count=1, W=H=112 (§1):
#   size = 3 + 2 + 4*1 + W*H + 2*W*H + 2 + 4096*tile_count + 768 + tile_count
STANDARD_BASE = 38_411
STANDARD_PER_TILE = 4_097


class WrlError(Exception):
    """Ошибка чтения/разбора WRL-файла (некорректный формат)."""


class WrlMap:
    """Чтение одной карты .wrl и производные структуры.

    После __init__ в памяти лежат: заголовок, миникарта, карта tile-id,
    палитра, pass-таблица и сырые пиксели тайлов (для tile_pixels()).
    Размеры и смещения секций доступны через атрибуты width/height/offsets.
    """

    def __init__(self, path):
        self.path = path
        self.warnings = []       # некритичные расхождения с форматом
        self._data = b""         # сырые байты файла (нужны для пикселей тайлов)
        self._surface_flat = None

        with open(path, "rb") as fp:
            self._data = fp.read()
        self.file_size = len(self._data)

        try:
            self._read_header()
            self._read_sections()
        except Exception:
            self._data = b""
            raise

    # ------------------------------------------------------------------ разбор
    def _fail(self, message):
        raise WrlError(message)

    def _read_header(self):
        """Читает сигнатуру, layer_count и dims[layer_count] (§1.1)."""
        data = self._data
        size = self.file_size
        if size < SIGNATURE_LEN + 2:
            self._fail(
                "файл слишком короткий для заголовка WRL: %d байт (нужно минимум %d)"
                % (size, SIGNATURE_LEN + 2))

        self.signature = data[0:SIGNATURE_LEN]
        if self.signature not in KNOWN_SIGNATURES:
            self._fail(
                "некорректная сигнатура WRL: ожидалось %r (известное допустимое "
                "исключение — %r у SNOW_1.WRL, §1.1), получено %r (%s)"
                % (SIGNATURE, b"DMO", self.signature,
                   " ".join("%02X" % b for b in self.signature)))
        if self.signature == b"DMO":
            self.warnings.append(
                "сигнатура %r вместо %r — известное исключение SNOW_1.WRL (§1.1); "
                "файл читается как валидный" % (self.signature, SIGNATURE))

        self.layer_count = struct.unpack_from("<H", data, SIGNATURE_LEN)[0]
        if self.layer_count == 0:
            self._fail("layer_count = 0: в файле нет dims[0], карта повреждена (§1.1)")
        if self.layer_count > MAX_CPP_LAYERS:
            self.warnings.append(
                "layer_count=%d > %d: C++-порт переполнил бы стековый массив "
                "Point[12] (§1.1); читаем все записи в список"
                % (self.layer_count, MAX_CPP_LAYERS))

        dims_offset = SIGNATURE_LEN + 2
        dims_end = dims_offset + 4 * self.layer_count
        if size < dims_end:
            self._fail(
                "файл обрезан: таблица размеров слоёв (%d × 4 байта) выходит "
                "за конец файла (конец 0x%X, файл 0x%X)"
                % (self.layer_count, dims_end, size))

        self.layer_dimensions = []
        for i in range(self.layer_count):
            x, y = struct.unpack_from("<hh", data, dims_offset + 4 * i)
            self.layer_dimensions.append((x, y))

        self.width, self.height = self.layer_dimensions[0]
        if self.width <= 0 or self.height <= 0:
            self._fail(
                "некорректные размеры слоя 0: (%d, %d) — W и H должны быть > 0"
                % (self.width, self.height))
        if (self.width, self.height) != (STANDARD_WIDTH, STANDARD_HEIGHT):
            self.warnings.append(
                "нестандартный размер карты %d×%d (реальные файлы — 112×112); "
                "формат допускает любые dims[0] (§7.4)"
                % (self.width, self.height))
        if self.layer_count != 1:
            self.warnings.append(
                "layer_count=%d: используется только dims[0], миникарта читается "
                "как один слой W*H (§1.1, §1.2, §7.3)" % self.layer_count)

        self.cells = self.width * self.height

    def _read_sections(self):
        """Читает миникарту, tile-id, tile_count, палитру и pass-таблицу (§1.2–1.6).

        Пиксели тайлов не копируются: они лежат в self._data и читаются
        по требованию в tile_pixels(); границы проверяются здесь.
        """
        data = self._data
        size = self.file_size
        cells = self.cells

        header_end = SIGNATURE_LEN + 2 + 4 * self.layer_count

        minimap_offset = header_end
        tile_ids_offset = minimap_offset + cells
        tile_count_offset = tile_ids_offset + 2 * cells
        if size < tile_count_offset + 2:
            self._fail(
                "файл обрезан: нет поля tile_count (нужно минимум 0x%X байт, "
                "файл 0x%X)" % (tile_count_offset + 2, size))

        self.tile_count = struct.unpack_from("<H", data, tile_count_offset)[0]
        tiles_offset = tile_count_offset + 2
        palette_offset = tiles_offset + TILE_PIXELS * self.tile_count
        pass_offset = palette_offset + PALETTE_BYTES
        data_end = pass_offset + self.tile_count

        if size < palette_offset:
            self._fail(
                "файл обрезан: пиксели тайлов (tile_count=%d × %d байт) выходят "
                "за конец файла (конец 0x%X, файл 0x%X)"
                % (self.tile_count, TILE_PIXELS, palette_offset, size))
        if size < pass_offset:
            self._fail(
                "файл обрезан: палитра (%d байт) выходит за конец файла "
                "(конец 0x%X, файл 0x%X)" % (PALETTE_BYTES, pass_offset, size))
        if size < data_end:
            self._fail(
                "файл обрезан: pass-таблица (tile_count=%d байт) выходит за конец "
                "файла (конец 0x%X, файл 0x%X)"
                % (self.tile_count, data_end, size))
        if size > data_end:
            self.warnings.append(
                "после pass-таблицы %d лишних байт (0x%X..0x%X); в реальных "
                "файлах pass-таблица заканчивается ровно на EOF (§6.7)"
                % (size - data_end, data_end, size))

        self.offsets = {
            "signature": 0,
            "layer_count": SIGNATURE_LEN,
            "dims": SIGNATURE_LEN + 2,
            "minimap": minimap_offset,
            "tile_ids": tile_ids_offset,
            "tile_count": tile_count_offset,
            "tiles": tiles_offset,
            "palette": palette_offset,
            "pass_table": pass_offset,
            "end": data_end,
        }
        self.trailing_bytes = max(0, size - data_end)

        # --- миникарта: ровно один слой, индекс палитры (§1.2) ---
        self._minimap = data[minimap_offset:minimap_offset + cells]

        # --- карта tile-id: W*H u16 LE, row-major (§1.3) ---
        self._tile_ids = list(struct.unpack_from("<%dH" % cells, data, tile_ids_offset))

        # --- палитра: 256 × RGB (§1.5) ---
        self._palette = data[palette_offset:palette_offset + PALETTE_BYTES]

        # --- pass-таблица: tile_count байт, значения 0..3 (§1.6) ---
        self._pass_table = list(data[pass_offset:data_end])

        self._check_values()

    def _check_values(self):
        """Мягко проверяет диапазоны значений (§6.4, §6.5, §7.6)."""
        bad_ids = [i for i, tid in enumerate(self._tile_ids)
                   if tid >= self.tile_count]
        if bad_ids:
            self.warnings.append(
                "%d клеток ссылаются на tile_id >= tile_count=%d (например, "
                "клетка %d: id=%d); такие клетки получат SURFACE_TYPE_NONE"
                % (len(bad_ids), self.tile_count, bad_ids[0],
                   self._tile_ids[bad_ids[0]]))

        bad_pass = [i for i, v in enumerate(self._pass_table) if v > 3]
        if bad_pass:
            self.warnings.append(
                "%d байт pass-таблицы вне 0..3 (например, тайл %d: %d); такие "
                "тайлы получат SURFACE_TYPE_NONE"
                % (len(bad_pass), bad_pass[0], self._pass_table[bad_pass[0]]))

        if self.tile_count == 0:
            self.warnings.append("tile_count = 0: тайлов и pass-таблицы нет (§7.11)")

    # ------------------------------------------------------------- публичное API
    def signature_text(self):
        """Сигнатура как ASCII-строка (для отчётов; может быть битой)."""
        return self.signature.decode("latin-1")

    def minimap(self):
        """Плоский список W*H байт миникарты (индексы палитры, row-major)."""
        return list(self._minimap)

    def minimap_rows(self):
        """Миникарта как список строк W×H."""
        return self._to_rows(list(self._minimap))

    def tile_map(self):
        """Карта tile-id как список строк H×W (каждая строка — W int).

        Требование задания: «список/массив 112×112 tile-id».
        """
        return self._to_rows(self._tile_ids)

    def tile_map_flat(self):
        """Карта tile-id плоским списком W*H (row-major)."""
        return list(self._tile_ids)

    def pass_table(self):
        """Pass-таблица: список tile_count сырых байт 0..3 (индексы PassData)."""
        return list(self._pass_table)

    def pass_surfaces(self):
        """Pass-таблица, отображённая в SURFACE_TYPE_*: 0→1, 1→2, 2→4, 3→8.

        Некорректный индекс (> 3) даёт 0 (SURFACE_TYPE_NONE), как в Rust-порте.
        """
        return [PASS_SURFACES[v] if 0 <= v < len(PASS_SURFACES) else 0
                for v in self._pass_table]

    def surface_map(self):
        """Карта поверхностей как список строк H×W (значения — SURFACE_TYPE_*).

        Формула §3.1: surface[i] = PassData[pass_table[tile_id[i]]].
        """
        return self._to_rows(self.surface_map_flat())

    def surface_map_flat(self):
        """Карта поверхностей плоским списком W*H (row-major), §3.1."""
        if self._surface_flat is None:
            surfaces = self.pass_surfaces()
            tile_count = self.tile_count
            result = []
            for tid in self._tile_ids:
                if 0 <= tid < tile_count:
                    result.append(surfaces[tid])
                else:
                    result.append(0x0)  # SURFACE_TYPE_NONE (§7.6)
            self._surface_flat = result
        return list(self._surface_flat)

    def palette(self):
        """Сырые 768 байт палитры (256 × RGB, 8 бит на канал)."""
        return bytes(self._palette)

    def palette_rgb(self):
        """Палитра списком 256 кортежей (R, G, B), §1.5."""
        p = self._palette
        return [(p[i * 3], p[i * 3 + 1], p[i * 3 + 2])
                for i in range(PALETTE_COLORS)]

    def tile_pixels(self, tile_id):
        """Сырые 64×64 байта (4096) пикселей одного тайла по его id (§1.4)."""
        if not 0 <= tile_id < self.tile_count:
            raise IndexError(
                "tile_id=%d вне диапазона 0..%d" % (tile_id, self.tile_count - 1))
        offset = self.offsets["tiles"] + tile_id * TILE_PIXELS
        return self._data[offset:offset + TILE_PIXELS]

    # ------------------------------------------------------------------ утилиты
    def _to_rows(self, flat):
        """Плоский row-major список -> список строк H×W (копии срезов)."""
        w = self.width
        return [flat[y * w:(y + 1) * w] for y in range(self.height)]

    def pass_table_sha256(self):
        """SHA-256 сырых байт pass-таблицы — отпечаток для сравнения карт."""
        return hashlib.sha256(bytes(self._pass_table)).hexdigest()

    def check_invariants(self):
        """Проверяет инварианты реальных файлов из §6.

        Возвращает список строк с нарушениями (пустой список — всё сходится).
        Предупреждения (warnings) сюда не входят: это не нарушения формата.
        """
        problems = []
        size = self.file_size
        expected_end = self.offsets["end"]

        if self.layer_count != 1:
            problems.append("layer_count=%d != 1 (§6.2)" % self.layer_count)
        if (self.width, self.height) != (STANDARD_WIDTH, STANDARD_HEIGHT):
            problems.append("dims[0]=(%d, %d) != (112, 112) (§6.2)"
                            % (self.width, self.height))

        if self.layer_count == 1 and \
                (self.width, self.height) == (112, 112):
            formula = STANDARD_BASE + STANDARD_PER_TILE * self.tile_count
            if size != formula:
                problems.append(
                    "размер %d != 38411 + 4097*%d = %d (§1, §6.1)"
                    % (size, self.tile_count, formula))
        elif size != expected_end:
            problems.append(
                "размер %d != концу данных 0x%X = %d (§1)"
                % (size, expected_end, expected_end))

        if self.signature not in KNOWN_SIGNATURES:
            problems.append("сигнатура %r не WRL/DMO (§6.3)"
                            % (self.signature,))

        if self.tile_count:
            bad_ids = sum(1 for tid in self._tile_ids
                          if tid >= self.tile_count)
            if bad_ids:
                problems.append(
                    "%d клеток с tile_id >= tile_count=%d (§6.4)"
                    % (bad_ids, self.tile_count))
            used = set(self._tile_ids)
            missing = [tid for tid in range(self.tile_count)
                       if tid not in used]
            if missing:
                problems.append(
                    "%d id из 0..%d не используются (например, %s) (§6.4)"
                    % (len(missing), self.tile_count - 1,
                       ", ".join(str(t) for t in missing[:8])))
            if max(self._tile_ids) != self.tile_count - 1:
                problems.append(
                    "максимальный tile_id=%d != tile_count-1=%d (§6.4)"
                    % (max(self._tile_ids), self.tile_count - 1))

        bad_pass = sum(1 for v in self._pass_table if v > 3)
        if bad_pass:
            problems.append(
                "%d байт pass-таблицы вне 0..3 (§6.5)" % bad_pass)

        if len(self._palette) != PALETTE_BYTES:
            problems.append("палитра %d байт != %d (§6.6)"
                            % (len(self._palette), PALETTE_BYTES))

        if size != expected_end:
            problems.append(
                "pass-таблица не заканчивается на EOF: конец 0x%X, файл 0x%X (§6.7)"
                % (expected_end, size))

        return problems

    def close(self):
        """Освобождает сырые байты файла."""
        self._data = b""
        self._surface_flat = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.close()


# ---------------------------------------------------------------- форматирование
def _fmt_int(value):
    """1234567 -> '1 234 567' (узкий пробел не нужен, обычный читаемее)."""
    return "%d" % value if abs(value) < 1000 else \
        "{:,}".format(value).replace(",", " ")


def _hex_bytes(raw):
    return " ".join("%02X" % b for b in raw)


def _histogram(values, limit=None):
    """Гистограмма значений -> список пар (значение, количество) по возрастанию."""
    counts = {}
    for v in values:
        counts[v] = counts.get(v, 0) + 1
    items = sorted(counts.items())
    if limit is not None and len(items) > limit:
        items = items[:limit]
    return items


# ---------------------------------------------------------------- CLI-команды
def cmd_info(wrl):
    """Печатает заголовок, секции, статистику карты и результат инвариантов."""
    print("Файл:        %s" % wrl.path)
    print("Размер:      %s байт (0x%X)" % (_fmt_int(wrl.file_size), wrl.file_size))
    note = ""
    if wrl.signature == b"DMO":
        note = "  — известное исключение SNOW_1.WRL (§1.1), файл читается"
    print("Сигнатура:   %r (%s)%s"
          % (wrl.signature_text(), _hex_bytes(wrl.signature), note))
    print("Заголовок:   layer_count=%d, dims[0]=(%d, %d) → %s клеток"
          % (wrl.layer_count, wrl.width, wrl.height, _fmt_int(wrl.cells)))
    if wrl.layer_count > 1:
        print("Слои:        %s"
              % ", ".join("(%d, %d)" % d for d in wrl.layer_dimensions))

    o = wrl.offsets
    print("")
    print("Секции:")
    sections = [
        ("сигнатура", o["signature"], SIGNATURE_LEN),
        ("layer_count", o["layer_count"], 2),
        ("dims", o["dims"], 4 * wrl.layer_count),
        ("миникарта", o["minimap"], wrl.cells),
        ("карта tile-id", o["tile_ids"], 2 * wrl.cells),
        ("tile_count", o["tile_count"], 2),
        ("пиксели тайлов", o["tiles"], TILE_PIXELS * wrl.tile_count),
        ("палитра", o["palette"], PALETTE_BYTES),
        ("pass-таблица", o["pass_table"], wrl.tile_count),
    ]
    for label, offset, length in sections:
        extra = "  = %d" % wrl.tile_count if label == "tile_count" else ""
        print("  %-14s 0x%06X  %8d%s" % (label, offset, length, extra))
    print("  %-14s 0x%06X" % ("конец данных", o["end"]))

    if wrl.layer_count == 1 and \
            (wrl.width, wrl.height) == (STANDARD_WIDTH, STANDARD_HEIGHT):
        formula = STANDARD_BASE + STANDARD_PER_TILE * wrl.tile_count
        status = "сходится" if formula == wrl.file_size else "НЕ СХОДИТСЯ"
        print("Формула §1:  38 411 + 4 097 × %d = %s — %s"
              % (wrl.tile_count, _fmt_int(formula), status))
    else:
        print("Конец данных: 0x%X, размер файла 0x%X — %s"
              % (o["end"], wrl.file_size,
                 "сходится" if o["end"] == wrl.file_size else "НЕ СХОДИТСЯ"))

    # --- карта tile-id (§1.3, §6.4) ---
    tiles = wrl.tile_map_flat()
    used = set(tiles)
    bad_ids = sum(1 for tid in tiles if tid >= wrl.tile_count)
    print("")
    print("Карта tile-id: min=%d max=%d, использовано %d из %d id, "
          "ссылок вне набора: %d"
          % (min(tiles) if tiles else -1, max(tiles) if tiles else -1,
             len(used), wrl.tile_count, bad_ids))
    print("  первые tile-id (16): %s"
          % ", ".join(str(v) for v in tiles[:16]))
    missing = [tid for tid in range(wrl.tile_count) if tid not in used]
    if missing:
        print("  не встречаются id: %s%s"
              % (", ".join(str(t) for t in missing[:16]),
                 " …" if len(missing) > 16 else ""))

    # --- pass-таблица (§1.6) ---
    pass_raw = wrl.pass_table()
    pass_hist = _histogram(pass_raw)
    print("")
    print("Pass-таблица: %d байт, уникальных значений %d%s"
          % (len(pass_raw), len(pass_hist),
             (": " + ", ".join("%d×%d" % (v, n) for v, n in pass_hist))
             if pass_hist else ""))
    print("  значения вне 0..3: %d"
          % sum(1 for v in pass_raw if v > 3))
    print("  sha256: %s" % wrl.pass_table_sha256())

    # --- производная карта поверхностей (§3.1) ---
    surface = wrl.surface_map_flat()
    surf_hist = _histogram(surface)
    surf_text = ", ".join(
        "%s(%02X)=%d" % (SURFACE_NAMES.get(v, "?"), v, n)
        for v, n in surf_hist)
    print("")
    print("SurfaceMap: %d клеток; %s" % (len(surface), surf_text or "пусто"))
    print("  первые поверхности (16): %s"
          % ", ".join("%02X" % v for v in surface[:16]))

    # --- палитра (§1.5) ---
    palette = wrl.palette()
    colors = wrl.palette_rgb()
    print("")
    print("Палитра: %d байт (256 × RGB); цвет 0=%s, цвет 1=%s, цвет 96=%s"
          % (len(palette), colors[0], colors[1], colors[96]))

    problems = wrl.check_invariants()
    print("")
    if problems:
        print("Инварианты §6 (эталонные файлы): НЕ ВЫПОЛНЕНЫ (%d)" % len(problems))
        for p in problems:
            print("  - %s" % p)
    else:
        print("Инварианты §6: в порядке")
    for w in wrl.warnings:
        print("ПРЕДУПРЕЖДЕНИЕ: %s" % w, file=sys.stderr)


def cmd_dump_surface(wrl, out_path, base=16):
    """Пишет surface_map в текстовый файл: hex (по умолчанию) или десятичные числа."""
    surface = wrl.surface_map()
    lines = [
        "# surface_map %s  %dx%d (row-major)" % (
            os.path.basename(wrl.path), wrl.width, wrl.height),
        "# 01=LAND 02=WATER 04=COAST 08=AIR 00=NONE",
        "# format=%s, file_size=%d, tile_count=%d"
        % ("hex" if base == 16 else "dec", wrl.file_size, wrl.tile_count),
    ]
    for row in surface:
        if base == 16:
            lines.append(" ".join("%02X" % v for v in row))
        else:
            lines.append(" ".join(str(v) for v in row))
    text = "\n".join(lines) + "\n"
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print("SurfaceMap %d×%d записана: %s (%s, %d строк)"
          % (wrl.width, wrl.height, out_path,
             "hex" if base == 16 else "dec", len(surface)))


def cmd_verify(wrl):
    """Проверяет инварианты §6 и печатает результат."""
    print("Файл:  %s (%s байт)" % (wrl.path, _fmt_int(wrl.file_size)))
    print("Заголовок: sig=%r layers=%d dims=%s tile_count=%d"
          % (wrl.signature_text(), wrl.layer_count,
             (wrl.width, wrl.height), wrl.tile_count))
    problems = wrl.check_invariants()
    if problems:
        print("Не выполнены инварианты эталонных файлов §6 (%d):" % len(problems))
        for p in problems:
            print("  - %s" % p)
        return 1
    print("Инварианты §6 в порядке")
    return 0


def _force_utf8_streams():
    """Вывод в UTF-8: при перенаправлении в файл/канал Python берёт cp1251."""
    for stream in (sys.stdout, sys.stderr):
        try:
            if (stream.encoding or "").lower().replace("-", "") != "utf8":
                stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass


def main(argv):
    _force_utf8_streams()
    if len(argv) < 3:
        print("Использование:", file=sys.stderr)
        print("  python wrl_parser.py <file.wrl> info", file=sys.stderr)
        print("  python wrl_parser.py <file.wrl> dump-surface <out.txt> [hex|dec]",
              file=sys.stderr)
        print("  python wrl_parser.py <file.wrl> verify", file=sys.stderr)
        return 2

    path = argv[1]
    command = argv[2].lower()
    if not os.path.isfile(path):
        print("Ошибка: файл не найден: %s" % path, file=sys.stderr)
        return 1

    try:
        wrl = WrlMap(path)
    except (WrlError, OSError) as exc:
        print("Ошибка: %s" % exc, file=sys.stderr)
        return 1

    try:
        if command == "info":
            cmd_info(wrl)
            return 0
        if command == "verify":
            return cmd_verify(wrl)
        if command == "dump-surface":
            if len(argv) < 4:
                print("Ошибка: укажите путь выходного файла", file=sys.stderr)
                return 2
            out_path = argv[3]
            base = 16
            if len(argv) > 4:
                fmt = argv[4].lower()
                if fmt in ("hex", "16", "h"):
                    base = 16
                elif fmt in ("dec", "10", "d"):
                    base = 10
                else:
                    print("Ошибка: неизвестный формат %r (ожидалось hex или dec)"
                          % argv[4], file=sys.stderr)
                    return 2
            cmd_dump_surface(wrl, out_path, base)
            return 0
        print("Ошибка: неизвестная команда %r" % command, file=sys.stderr)
        return 2
    except (WrlError, OSError) as exc:
        print("Ошибка: %s" % exc, file=sys.stderr)
        return 1
    finally:
        wrl.close()


if __name__ == "__main__":
    sys.exit(main(sys.argv))
