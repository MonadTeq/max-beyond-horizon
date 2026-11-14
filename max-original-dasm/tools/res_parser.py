#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
res_parser.py — эталонный парсер ресурсных архивов M.A.X. (формат RES).

Формат описан в max-original-dasm/formats/res.md:
  * заголовок 12 байт: сигнатура "RES0" + u32 offset + u32 size;
  * данные ресурсов идут подряд сразу после заголовка (первый ресурс на 0x0C);
  * таблица из (size / 16) записей лежит по смещению offset;
  * запись 16 байт: тег 8 байт (C-строка, обрезается по первому NUL) +
    u32 data_offset + u32 data_size;
  * всё little-endian, структуры упакованы, выравнивания нет;
  * сжатия на уровне контейнера НЕТ — данные читаются как есть.

Использование:
    python res_parser.py <file.res> list
    python res_parser.py <file.res> extract <name> [out_path]
    python res_parser.py <file.res> extract-all <out_dir>
    python res_parser.py <file.res> verify          (доп. команда: проверка инвариантов)
"""

import os
import struct
import sys

# Константы формата (см. res.md §1.1, §1.2).
SIGNATURE = b"RES0"          # сигнатура заголовка, 4 байта, без NUL
HEADER_SIZE = 12             # sizeof(res_header)
ENTRY_SIZE = 16              # sizeof(res_index)
TAG_LEN = 8                  # длина поля tag в записи таблицы
FIRST_DATA_OFFSET = HEADER_SIZE  # данные начинаются сразу после заголовка (0x0C)


class ResError(Exception):
    """Ошибка чтения/разбора RES-файла (некорректный формат)."""


class ResEntry:
    """Одна запись таблицы ресурсов.

    Поля: name — тег как C-строка; offset — смещение данных от начала файла;
    size — размер данных; flags — в формате RES поля флагов нет, поэтому всегда 0
    (оставлено для совместимости с ожидаемым API list()).
    """

    __slots__ = ("index", "name", "offset", "size", "flags")

    def __init__(self, index, name, offset, size):
        self.index = index
        self.name = name
        self.offset = offset
        self.size = size
        self.flags = 0

    def __repr__(self):
        return "ResEntry(%d, %r, off=0x%X, size=%d)" % (
            self.index, self.name, self.offset, self.size)


class ResArchive:
    """Чтение и распаковка одного .RES-архива.

    Ресурсы адресуются по тегу (имя). Если в одном файле встречаются дубликаты
    тега, побеждает первая запись — как в оригинальном загрузчике (res.md §3).
    """

    def __init__(self, path):
        self.path = path
        self.file_size = os.path.getsize(path)
        self.table_offset = 0
        self.table_size = 0
        self.entries = []            # все записи в порядке таблицы
        self.by_name = {}            # тег -> первая запись с этим тегом
        self.warnings = []           # некритичные расхождения с форматом

        # Файл держим открытым: read() и extract_all() читают по смещениям.
        self._fp = open(path, "rb")
        try:
            self._read_header()
            self._read_table()
        except Exception:
            self._fp.close()
            raise

    # ------------------------------------------------------------------ разбор
    def _read_header(self):
        """Читает 12-байтный заголовок и проверяет сигнатуру."""
        self._fp.seek(0)
        raw = self._fp.read(HEADER_SIZE)
        if len(raw) < HEADER_SIZE:
            raise ResError("файл слишком короткий для заголовка RES (%d байт)" % len(raw))

        sig = raw[0:4]
        if sig != SIGNATURE:
            raise ResError(
                "некорректная сигнатура: ожидалось %r, получено %r"
                % (SIGNATURE.decode("ascii"), sig.decode("latin-1")))

        # Поля int32_t в оригинале; читаем как u32 и проверяем на разумность.
        table_offset, table_size = struct.unpack_from("<II", raw, 4)
        if table_size % ENTRY_SIZE != 0:
            # По спецификации size кратен 16; не падаем, но фиксируем расхождение.
            self.warnings.append(
                "размер таблицы %d не кратен %d — лишние байты отброшены"
                % (table_size, ENTRY_SIZE))
        if table_offset + table_size > self.file_size:
            raise ResError(
                "таблица выходит за конец файла: offset=0x%X size=0x%X, файл %d байт"
                % (table_offset, table_size, self.file_size))
        if table_offset < HEADER_SIZE:
            raise ResError("смещение таблицы 0x%X меньше заголовка (0x%X)"
                           % (table_offset, HEADER_SIZE))

        self.table_offset = table_offset
        self.table_size = table_size

    def _read_table(self):
        """Читает ровно header.size байт таблицы и разбирает её на записи."""
        count = self.table_size // ENTRY_SIZE
        self._fp.seek(self.table_offset)
        table = self._fp.read(self.table_size)
        if len(table) != self.table_size:
            raise ResError("таблица обрезана: прочитано %d из %d байт"
                           % (len(table), self.table_size))

        # Читаем строго offset..offset+size: за таблицей в PATCHES.RES лежат
        # 16 байт мусора (обрывок 85-й записи), их трогать нельзя (res.md §1.4).
        for i in range(count):
            pos = i * ENTRY_SIZE
            tag_raw = table[pos:pos + TAG_LEN]
            # tag — C-строка в фиксированном поле: режем по первому NUL,
            # хвостовой мусор после NUL игнорируется ("ILOGO\0TM" == "ILOGO").
            name = tag_raw.split(b"\x00", 1)[0].decode("latin-1")
            data_offset, data_size = struct.unpack_from("<II", table, pos + TAG_LEN)
            entry = ResEntry(i, name, data_offset, data_size)
            self.entries.append(entry)
            # "Первый выигрывает" — дубликаты тега внутри файла не перезаписывают.
            if name not in self.by_name:
                self.by_name[name] = entry

            # Мягкая проверка границ: данные должны лежать до таблицы и в файле.
            if data_offset + data_size > self.file_size:
                self.warnings.append(
                    "запись %d (%r) выходит за конец файла: 0x%X+%d > %d"
                    % (i, name, data_offset, data_size, self.file_size))
            elif data_offset + data_size > self.table_offset:
                self.warnings.append(
                    "запись %d (%r) заходит за начало таблицы: конец 0x%X > 0x%X"
                    % (i, name, data_offset + data_size, self.table_offset))

    # ------------------------------------------------------------- публичное API
    def list(self):
        """Возвращает список всех записей таблицы (ResEntry)."""
        return list(self.entries)

    def _read_entry(self, entry):
        """Читает сырые байты записи. Сжатия в контейнере нет (res.md §5)."""
        if entry.size == 0:
            return b""
        self._fp.seek(entry.offset)
        data = self._fp.read(entry.size)
        if len(data) != entry.size:
            raise ResError("ресурс %r обрезан: прочитано %d из %d байт"
                           % (entry.name, len(data), entry.size))
        return data

    def read(self, name):
        """Возвращает байты ресурса по имени (тегу)."""
        entry = self.by_name.get(name)
        if entry is None:
            # Подсказка: ближайшие по регистру совпадения.
            low = name.lower()
            close = [e.name for e in self.entries if e.name.lower() == low]
            hint = (" (есть с другим регистром: %s)" % ", ".join(close)) if close else ""
            raise KeyError("ресурс %r не найден в %s%s" % (name, self.path, hint))
        return self._read_entry(entry)

    def extract_all(self, out_dir):
        """Распаковывает все ресурсы в каталог out_dir.

        Дубликаты тегов пропускаются (первая запись выигрывает, res.md §3).
        Возвращает список (тег, путь, размер) для записанных файлов.
        """
        os.makedirs(out_dir, exist_ok=True)
        written = []
        seen = set()
        for entry in self.entries:
            if entry.name in seen:
                continue
            seen.add(entry.name)
            dest = os.path.join(out_dir, self._safe_name(entry.name, entry.index))
            with open(dest, "wb") as f:
                f.write(self._read_entry(entry))
            written.append((entry.name, dest, entry.size))
        return written

    def check_invariants(self):
        """Проверяет инварианты реальных файлов из res.md §1.2.

        Возвращает список строк с нарушениями (пустой список — всё сходится).
        """
        problems = list(self.warnings)
        if not self.entries:
            return problems

        prev_end = FIRST_DATA_OFFSET
        for e in self.entries:
            if e.offset != prev_end:
                problems.append(
                    "запись %d (%r): offset 0x%X != ожидаемого 0x%X (разрыв/наложение)"
                    % (e.index, e.name, e.offset, prev_end))
            prev_end = e.offset + e.size

        last = self.entries[-1]
        if last.offset + last.size != self.table_offset:
            problems.append(
                "конец данных 0x%X != начала таблицы 0x%X"
                % (last.offset + last.size, self.table_offset))
        return problems

    def close(self):
        if self._fp is not None:
            self._fp.close()
            self._fp = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        self.close()

    # ------------------------------------------------------------------ утилиты
    @staticmethod
    def _safe_name(name, index):
        """Имя файла для распаковки: без разделителей путей и запрещённых символов."""
        bad = '\\/:*?"<>|'
        cleaned = "".join(ch if (ch not in bad and 32 <= ord(ch) < 127) else "_"
                          for ch in name).strip()
        if not cleaned or cleaned in (".", ".."):
            cleaned = "resource_%04d" % index
        return cleaned


# ---------------------------------------------------------------- CLI-команды
def cmd_list(archive):
    """Печатает все записи: индекс, тег, смещение, размер (+ hex)."""
    entries = archive.list()
    print("Файл:      %s (%d байт)" % (archive.path, archive.file_size))
    print("Заголовок: RES0, таблица @ 0x%X, размер %d байт, записей %d"
          % (archive.table_offset, archive.table_size, len(entries)))
    print("")
    print("%4s  %-9s %12s %12s %10s" % ("#", "тег", "offset", "size", "size(hex)"))
    print("-" * 54)
    for e in entries:
        print("%4d  %-9s %12d %12d   0x%08X"
              % (e.index, e.name, e.offset, e.size, e.size))
    total = sum(e.size for e in entries)
    print("-" * 54)
    print("Итого записей: %d, суммарный размер данных: %d байт" % (len(entries), total))
    for w in archive.warnings:
        print("ПРЕДУПРЕЖДЕНИЕ: %s" % w, file=sys.stderr)


def cmd_extract(archive, name, out_path=None):
    """Пишет один ресурс в файл (по умолчанию — в текущий каталог)."""
    data = archive.read(name)
    if out_path is None:
        out_path = ResArchive._safe_name(name, archive.by_name[name].index)
    with open(out_path, "wb") as f:
        f.write(data)
    print("Извлечено %r: %d байт -> %s" % (name, len(data), out_path))


def cmd_extract_all(archive, out_dir):
    """Пишет все ресурсы в каталог."""
    written = archive.extract_all(out_dir)
    for name, path, size in written:
        print("  %-9s %8d байт  %s" % (name, size, path))
    print("Всего извлечено: %d файлов в %s" % (len(written), out_dir))


def cmd_verify(archive):
    """Проверяет инварианты и печатает сводку."""
    print("Файл:  %s (%d байт)" % (archive.path, archive.file_size))
    print("Таблица: offset=0x%X size=%d entries=%d"
          % (archive.table_offset, archive.table_size, len(archive.entries)))
    problems = archive.check_invariants()
    if problems:
        print("Инварианты нарушены (%d):" % len(problems))
        for p in problems:
            print("  - %s" % p)
        return 1
    print("Инварианты в порядке (§1.2 res.md)")
    return 0


def _force_utf8_streams():
    """Вывод в UTF-8: при перенаправлении в файл/канал Python берёт cp1251 и рвёт кириллицу."""
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
        print("  python res_parser.py <file.res> list", file=sys.stderr)
        print("  python res_parser.py <file.res> extract <name> [out_path]", file=sys.stderr)
        print("  python res_parser.py <file.res> extract-all <out_dir>", file=sys.stderr)
        print("  python res_parser.py <file.res> verify", file=sys.stderr)
        return 2

    path = argv[1]
    command = argv[2].lower()
    if not os.path.isfile(path):
        print("Ошибка: файл не найден: %s" % path, file=sys.stderr)
        return 1

    try:
        archive = ResArchive(path)
    except (ResError, OSError) as exc:
        print("Ошибка: %s" % exc, file=sys.stderr)
        return 1

    try:
        if command == "list":
            cmd_list(archive)
            return 0
        if command == "verify":
            return cmd_verify(archive)
        if command == "extract":
            if len(argv) < 4:
                print("Ошибка: укажите имя ресурса", file=sys.stderr)
                return 2
            out_path = argv[4] if len(argv) > 4 else None
            cmd_extract(archive, argv[3], out_path)
            return 0
        if command == "extract-all":
            if len(argv) < 4:
                print("Ошибка: укажите каталог назначения", file=sys.stderr)
                return 2
            cmd_extract_all(archive, argv[3])
            return 0
        print("Ошибка: неизвестная команда %r" % command, file=sys.stderr)
        return 2
    except KeyError as exc:
        print("Ошибка: %s" % exc.args[0], file=sys.stderr)
        return 1
    except (ResError, OSError) as exc:
        print("Ошибка: %s" % exc, file=sys.stderr)
        return 1
    finally:
        archive.close()


if __name__ == "__main__":
    sys.exit(main(sys.argv))
