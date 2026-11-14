#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Каталог функций MAXRUN.EXE с именами C++ (связывание трёх CSV).

Входы (по умолчанию относительно каталога max-original-dasm/):
  export/functions.csv  address, name, size, calls, called_by
  export/strings.csv    address, string, xrefs          (необязателен: может отсутствовать)
  string-matches.csv    flat_offset, string, cpp_files

Логика:
  * для каждого xref строки из strings.csv находится функция, в тело которой
    попадает адрес: addr <= xref < addr + size (при вложенности выбирается
    самая узкая функция);
  * такая строка становится якорем функции;
  * каждая строка-якорь ищется в string-matches.csv: сначала точное совпадение,
    затем по подстроке в любую сторону (бинарная строка внутри строки matches
    или наоборот); из cpp_files берутся базовые имена файлов (без пути);
  * cpp_files функции - объединение файлов всех её якорей.

Выходы:
  functions-named.csv   address, name, size, cpp_files, anchors, purpose
  functions-named.md    таблица top-100 функций по размеру

  anchors: до 4 строк-якорей, "строка|файл1,файл2", якоря через ";".
  purpose: колонка намеренно пустая - назначения не выдумываются, её заполняет
  человек (строки-якоря сами по себе не дают достоверного назначения).

Запуск:
  python tools/join_functions.py             # реальные данные
  python tools/join_functions.py --selftest  # самотест на синтетике во временном каталоге
"""

import csv
import heapq
import itertools
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DASM_DIR = os.path.dirname(HERE)
DEFAULT_BASE_DIR = DASM_DIR

EXPORT_DIR = "export"
FUNCTIONS_CSV = "functions.csv"
STRINGS_CSV = "strings.csv"
MATCHES_CSV = "string-matches.csv"
OUT_CSV = "functions-named.csv"
OUT_MD = "functions-named.md"

MAX_ANCHORS = 4          # сколько якорей показывать в ячейке anchors
TABLE_TOP = 100          # сколько строк в markdown-таблице
MIN_SUBSTRING = 6        # минимальная длина для нечёткого совпадения по подстроке


class InputMissingError(Exception):
    """Нет обязательного входного файла (или он пуст)."""


# ---------------------------------------------------------------------------
# Разбор значений
# ---------------------------------------------------------------------------

def parse_int(value, base=10):
    """Разбирает целое: "" -> None, 0x-префикс, иначе указанное основание.

    Экспорт Ghidra пишет адреса без префикса 0x (например 00010c77c), поэтому
    для адресов везде передаётся base=16.
    """
    if value is None:
        return None
    text = str(value).strip().strip('"').strip()
    if not text:
        return None
    sign = 1
    if text[0] in "+-":
        if text[0] == "-":
            sign = -1
        text = text[1:].strip()
        if not text:
            return None
    try:
        if text[:2].lower() == "0x":
            return sign * int(text, 16)
        return sign * int(text, base)
    except ValueError:
        # На случай записи "по-другому" (hex без префикса в десятичном поле).
        try:
            return sign * int(text, 16)
        except ValueError:
            return None


def iter_rows(path, expected_header):
    """Читает CSV и выдаёт dict-строки по заголовку (utf-8-sig, кавычки, переносы)."""
    with open(path, "r", encoding="utf-8-sig", errors="replace", newline="") as handle:
        reader = csv.reader(handle)
        try:
            first = next(reader)
        except StopIteration:
            return
        header = [cell.strip().lower() for cell in first]
        if "address" in header or "flat_offset" in header:
            rows = reader
        else:
            # Заголовка нет - считаем, что первая строка уже данные.
            header = list(expected_header)
            rows = itertools.chain([first], reader)
        for row in rows:
            if not row or not any(cell.strip() for cell in row):
                continue
            yield {header[i]: (row[i] if i < len(row) else "") for i in range(len(header))}


def basename(path):
    """Базовое имя файла без каталога (пути в CSV - windows-стиль, 'max\\src\\x.cpp')."""
    text = (path or "").strip().strip('"').strip()
    text = text.replace("\\", "/")
    return text.rsplit("/", 1)[-1].strip()


def dedupe(items):
    seen = set()
    out = []
    for item in items:
        if item and item not in seen:
            seen.add(item)
            out.append(item)
    return out


# ---------------------------------------------------------------------------
# Чтение входов
# ---------------------------------------------------------------------------

def read_functions(path):
    """functions.csv -> список dict'ов, отсортированный по адресу."""
    funcs = {}
    for row in iter_rows(path, ("address", "name", "size", "calls", "called_by")):
        addr = parse_int(row.get("address"), 16)
        if addr is None:
            continue
        size = parse_int(row.get("size"), 10)
        if size is None or size < 0:
            size = 0
        name = (row.get("name") or "").strip()
        address_text = (row.get("address") or "").strip() or ("%08x" % addr)
        old = funcs.get(addr)
        if old is not None:
            old["size"] = max(old["size"], size)
            if not old["name"]:
                old["name"] = name
            continue
        funcs[addr] = {
            "address": addr,
            "address_text": address_text,
            "name": name,
            "size": size,
        }
    return sorted(funcs.values(), key=lambda func: func["address"])


def read_strings(path):
    """strings.csv -> список строк бинаря с их xref-адресами."""
    items = []
    for row in iter_rows(path, ("address", "string", "xrefs")):
        addr = parse_int(row.get("address"), 16)
        text = row.get("string") or ""
        if addr is None or not text:
            continue
        xrefs = []
        for part in (row.get("xrefs") or "").split(";"):
            xref = parse_int(part, 16)
            if xref is not None:
                xrefs.append(xref)
        if xrefs:
            items.append({"address": addr, "string": text, "xrefs": xrefs})
    return items


class StringMatches(object):
    """Индекс string-matches.csv: точное совпадение, затем подстрока."""

    def __init__(self, entries):
        self.exact = {}
        self.norm = {}
        self.entries = []
        for text, files in entries:
            if not text:
                continue
            files = dedupe(files)
            if not files:
                continue
            self.entries.append((text, text.strip(), files))
            self.exact.setdefault(text, []).extend(files)
            key = text.strip()
            if key:
                self.norm.setdefault(key, []).extend(files)
        for table in (self.exact, self.norm):
            for key in list(table):
                table[key] = dedupe(table[key])

    def lookup(self, anchor):
        """-> (файлы, способ: 'exact' | 'substring' | None)."""
        files = self.exact.get(anchor)
        if files:
            return list(files), "exact"
        stripped = anchor.strip()
        files = self.norm.get(stripped)
        if files:
            return list(files), "exact"
        if len(stripped) < MIN_SUBSTRING:
            return [], None
        best = None
        for _text, norm, entry_files in self.entries:
            if not norm or min(len(stripped), len(norm)) < MIN_SUBSTRING:
                continue
            if stripped in norm or norm in stripped:
                score = (abs(len(norm) - len(stripped)), -len(norm), norm)
                if best is None or score < best[0]:
                    best = (score, entry_files)
        if best is None:
            return [], None
        return list(best[1]), "substring"


def read_matches(path):
    """string-matches.csv -> StringMatches."""
    entries = []
    for row in iter_rows(path, ("flat_offset", "string", "cpp_files")):
        text = row.get("string") or ""
        if not text:
            continue
        files = [basename(part) for part in (row.get("cpp_files") or "").split(";")]
        files = [name for name in files if name]
        if files:
            entries.append((text, files))
    return StringMatches(entries)


# ---------------------------------------------------------------------------
# xref -> функция
# ---------------------------------------------------------------------------

def find_functions(funcs, xrefs):
    """Отображает xref -> функция, содержащая адрес (самая узкая при вложенности).

    Разметка sweep line по отсортированным интервалам [addr, addr+size):
    активные интервалы лежат в куче по size, поэтому вершина - самая узкая
    функция, накрывающая текущий адрес.
    """
    result = {}
    if not funcs or not xrefs:
        return result
    intervals = sorted(
        (func["address"], func["address"] + func["size"], func["size"], index)
        for index, func in enumerate(funcs)
        if func["size"] > 0
    )
    heap = []
    position = 0
    total = len(intervals)
    for xref in sorted(set(xrefs)):
        while position < total and intervals[position][0] <= xref:
            start, end, size, index = intervals[position]
            heapq.heappush(heap, (size, start, index, end))
            position += 1
        # Выбрасываем истёкшие интервалы, пока они на вершине (ленивое удаление).
        while heap and heap[0][3] <= xref:
            heapq.heappop(heap)
        if heap:
            result[xref] = funcs[heap[0][2]]
    return result


# ---------------------------------------------------------------------------
# Сборка каталога
# ---------------------------------------------------------------------------

def sanitize_anchor_text(text):
    """Делает строку-якорь пригодной для ячейки CSV с разделителями ';' и '|'."""
    text = (text or "").replace("\r", " ").replace("\n", " ").replace("\t", " ")
    text = text.replace(";", " ").replace("|", " ")
    return " ".join(text.split())


def build(base_dir=DEFAULT_BASE_DIR, out_dir=None, quiet=False):
    """Строит функции-named.csv/.md. Возвращает (stats, rows, csv_path, md_path)."""
    base_dir = os.path.abspath(base_dir)
    out_dir = os.path.abspath(out_dir) if out_dir else base_dir

    functions_path = os.path.join(base_dir, EXPORT_DIR, FUNCTIONS_CSV)
    strings_path = os.path.join(base_dir, EXPORT_DIR, STRINGS_CSV)
    matches_path = os.path.join(base_dir, MATCHES_CSV)

    if not os.path.isfile(functions_path):
        raise InputMissingError(
            "export files missing: %s\n"
            "  сначала собери экспорт Ghidra: tools/ghidra/analyze.sh" % functions_path
        )
    if not os.path.isfile(matches_path):
        raise InputMissingError(
            "input files missing: %s\n"
            "  перегенерируй его: python tools/match_strings.py" % matches_path
        )

    funcs = read_functions(functions_path)
    if not funcs:
        raise InputMissingError("export files empty: no functions in %s" % functions_path)

    strings = []
    if os.path.isfile(strings_path):
        strings = read_strings(strings_path)
        if not strings and not quiet:
            print("note: %s пуст - якорей не будет" % strings_path)
    elif not quiet:
        print("note: %s отсутствует - якорей не будет" % strings_path)

    matches = read_matches(matches_path)

    # 1) xref -> функция; строки-якоря в разрезе функций.
    lookup = find_functions(funcs, [xref for item in strings for xref in item["xrefs"]])
    per_function = {}
    for item in strings:
        text = item["string"]
        for xref in item["xrefs"]:
            func = lookup.get(xref)
            if func is None:
                continue
            per_function.setdefault(func["address"], {}).setdefault(text, None)

    # 2) якорь -> файлы C++ (один раз на уникальный текст).
    cache = {}
    anchor_total = 0
    anchor_exact = 0
    anchor_substring = 0
    rows = []
    with_files = 0
    with_anchors = 0

    for func in funcs:
        bucket = per_function.get(func["address"], {})
        anchor_infos = []
        files_all = []
        for text in bucket:
            if text in cache:
                files, how = cache[text]
            else:
                files, how = matches.lookup(text)
                cache[text] = (files, how)
            anchor_infos.append((text, files))
            files_all.extend(files)
            anchor_total += 1
            if how == "exact":
                anchor_exact += 1
            elif how == "substring":
                anchor_substring += 1

        cpp_files = sorted(set(files_all))

        # Приоритет в показе: якоря с файлами, затем более длинные строки.
        ordered = sorted(
            anchor_infos,
            key=lambda item: (0 if item[1] else 1, -len(item[0]), item[0]),
        )[:MAX_ANCHORS]
        anchor_parts = []
        for text, files in ordered:
            label = sanitize_anchor_text(text)
            if files:
                anchor_parts.append("%s|%s" % (label, ",".join(files)))
            else:
                anchor_parts.append(label)

        row = {
            "address": func["address"],
            "address_text": func["address_text"],
            "name": func["name"],
            "size": func["size"],
            "cpp_files": ";".join(cpp_files),
            "anchors": ";".join(anchor_parts),
            "purpose": "",  # назначение не выдумывается
        }
        if row["cpp_files"]:
            with_files += 1
        if row["anchors"]:
            with_anchors += 1
        rows.append(row)

    rows.sort(key=lambda row: row["address"])

    stats = {
        "total": len(rows),
        "with_files": with_files,
        "with_anchors": with_anchors,
        "anchor_total": anchor_total,
        "anchor_exact": anchor_exact,
        "anchor_substring": anchor_substring,
        "functions_path": functions_path,
        "strings_path": strings_path,
        "matches_path": matches_path,
    }
    csv_path, md_path = write_catalog(rows, stats, out_dir)
    return stats, rows, csv_path, md_path


# ---------------------------------------------------------------------------
# Вывод
# ---------------------------------------------------------------------------

def md_cell(text):
    return (text or "").replace("|", "\\|").replace("\r", " ").replace("\n", " ")


def write_catalog(rows, stats, out_dir):
    os.makedirs(out_dir, exist_ok=True)
    csv_path = os.path.join(out_dir, OUT_CSV)
    md_path = os.path.join(out_dir, OUT_MD)

    with open(csv_path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, lineterminator="\n")
        writer.writerow(["address", "name", "size", "cpp_files", "anchors", "purpose"])
        for row in rows:
            writer.writerow([
                row["address_text"],
                row["name"],
                row["size"],
                row["cpp_files"],
                row["anchors"],
                row["purpose"],
            ])

    top = sorted(rows, key=lambda row: (-row["size"], row["address"]))[:TABLE_TOP]
    lines = [
        "# Каталог функций MAXRUN.EXE (имена C++ из порта)",
        "",
        "Всего функций: %d; с файлами C++: %d; с якорями: %d."
        % (stats["total"], stats["with_files"], stats["with_anchors"]),
        "",
        "| # | address | name | size | cpp_files | anchors |",
        "|---:|---|---|---:|---|---|",
    ]
    for index, row in enumerate(top, 1):
        lines.append(
            "| %d | %s | %s | %d | %s | %s |"
            % (
                index,
                md_cell(row["address_text"]),
                md_cell(row["name"]),
                row["size"],
                md_cell(row["cpp_files"]),
                md_cell(row["anchors"]),
            )
        )
    with open(md_path, "w", encoding="utf-8", newline="") as handle:
        handle.write("\n".join(lines) + "\n")

    return csv_path, md_path


def print_summary(stats, csv_path, md_path):
    print("Функций: %d; с cpp_files: %d; с anchors: %d"
          % (stats["total"], stats["with_files"], stats["with_anchors"]))
    print("Якорей: %d (точно: %d, по подстроке: %d)"
          % (stats["anchor_total"], stats["anchor_exact"], stats["anchor_substring"]))
    print("Записано: %s" % csv_path)
    print("Записано: %s (top-%d по размеру)" % (md_path, TABLE_TOP))


# ---------------------------------------------------------------------------
# Самотест
# ---------------------------------------------------------------------------

def run_selftest():
    """Синтетические данные во временном каталоге; проверка ожидаемых строк."""
    checks = [0]

    def check(condition, message):
        checks[0] += 1
        if not condition:
            raise AssertionError(message)

    tmp = tempfile.mkdtemp(prefix="join_functions_selftest_")
    try:
        export_dir = os.path.join(tmp, EXPORT_DIR)
        os.makedirs(export_dir)

        functions_csv = os.path.join(export_dir, FUNCTIONS_CSV)
        with open(functions_csv, "w", encoding="utf-8", newline="") as handle:
            handle.write(
                "address,name,size,calls,called_by\n"
                "00010000,FUN_00010000,64,,\n"
                "00010100,TaskKillUnit_Update,48,,\n"
                "00010200,FUN_00010200,32,,\n"
                "00010300,FUN_00010300,128,,\n"
            )

        strings_csv = os.path.join(export_dir, STRINGS_CSV)
        with open(strings_csv, "w", encoding="utf-8", newline="") as handle:
            handle.write(
                "address,string,xrefs\n"
                "00012000,Completed Kill Unit task.,00010110\n"
                "00012040,Maintain an attack reserve.,00010020;00010120\n"
                "00012080,Defense Reserve,00010200\n"
                "000120c0,Outside any function,00010fff\n"
                "00012100,Alpha anchor string one,00010310\n"
                "00012120,Bravo anchor string two,00010320\n"
                "00012140,Charlie anchor string three,00010330\n"
                "00012160,Delta anchor string four,00010340\n"
                "00012180,Echo anchor string five,00010350\n"
                "000121a0,Foxtrot anchor string six,00010360\n"
            )

        matches_csv = os.path.join(tmp, MATCHES_CSV)
        with open(matches_csv, "w", encoding="utf-8", newline="") as handle:
            handle.write(
                "flat_offset,string,cpp_files\n"
                "1241404,Completed Kill Unit task.,max\\src\\taskkillunit.cpp\n"
                "1241405,Completed Kill Unit task. (done),max\\src\\bogus.cpp\n"
                "1241376,Maintain an attack reserve.,"
                "max\\src\\taskattackreserve.cpp;max\\src\\taskkillunit.cpp\n"
                "1241512,Defense Reserve (idle),max\\src\\taskdefensereserve.cpp\n"
            )

        stats, rows, csv_path, md_path = build(tmp, quiet=True)

        check(stats["total"] == 4, "ожидалось 4 функции, получено %d" % stats["total"])
        check(stats["with_files"] == 3, "с cpp_files: ожидалось 3, получено %d" % stats["with_files"])
        check(stats["with_anchors"] == 4, "с anchors: ожидалось 4, получено %d" % stats["with_anchors"])

        addresses = [row["address"] for row in rows]
        check(addresses == sorted(addresses), "строки не отсортированы по адресу")
        check(addresses[0] == 0x10000 and addresses[-1] == 0x10300,
              "адреса функций разобраны неверно: %r" % addresses)

        by_name = {row["name"]: row for row in rows}

        row = by_name["FUN_00010000"]
        check(row["cpp_files"] == "taskattackreserve.cpp;taskkillunit.cpp",
              "FUN_00010000 cpp_files = %r" % row["cpp_files"])
        check(row["anchors"] == "Maintain an attack reserve.|taskattackreserve.cpp,taskkillunit.cpp",
              "FUN_00010000 anchors = %r" % row["anchors"])

        row = by_name["TaskKillUnit_Update"]
        check(row["cpp_files"] == "taskattackreserve.cpp;taskkillunit.cpp",
              "TaskKillUnit_Update cpp_files = %r" % row["cpp_files"])
        check(row["anchors"].count(";") == 1, "TaskKillUnit_Update: ожидалось 2 якоря")
        check("Completed Kill Unit task.|taskkillunit.cpp" in row["anchors"],
              "точное совпадение якоря не найдено: %r" % row["anchors"])
        check("bogus.cpp" not in row["cpp_files"], "точный якорь должен побеждать подстроку")

        row = by_name["FUN_00010200"]
        check(row["cpp_files"] == "taskdefensereserve.cpp",
              "подстрочный якорь Defense Reserve: %r" % row["cpp_files"])
        check(row["anchors"] == "Defense Reserve|taskdefensereserve.cpp",
              "FUN_00010200 anchors = %r" % row["anchors"])

        row = by_name["FUN_00010300"]
        check(row["cpp_files"] == "", "FUN_00010300 не должен иметь файлов")
        anchor_count = len([part for part in row["anchors"].split(";") if part])
        check(anchor_count == MAX_ANCHORS,
              "FUN_00010300: ожидалось %d якорей, получено %d" % (MAX_ANCHORS, anchor_count))

        check(all(row["purpose"] == "" for row in rows),
              "purpose должен оставаться пустым")
        check(all("Outside any function" not in row["anchors"] and
                  "Outside any function" not in row["cpp_files"] for row in rows),
              "строка вне функций не должна попадать в каталог")
        check(all("/" not in row["cpp_files"] and "\\" not in row["cpp_files"] for row in rows),
              "в cpp_files остались пути вместо базовых имён")

        with open(csv_path, "r", encoding="utf-8", newline="") as handle:
            reader = csv.reader(handle)
            header = next(reader)
            written = list(reader)
        check(header == ["address", "name", "size", "cpp_files", "anchors", "purpose"],
              "неверный заголовок functions-named.csv: %r" % (header,))
        check(len(written) == 4, "в CSV должно быть 4 строки данных, есть %d" % len(written))

        with open(md_path, "r", encoding="utf-8") as handle:
            markdown = handle.read()
        check(markdown.startswith("# "), "markdown не начинается с заголовка")
        check("| # | address | name | size | cpp_files | anchors |" in markdown,
              "в markdown нет шапки таблицы")
        check("FUN_00010300" in markdown, "в markdown нет крупнейшей функции")
        check("TaskKillUnit_Update" in markdown, "в markdown нет функции с якорями")

        # Нет обязательных входов -> понятное сообщение и код 1.
        empty_dir = os.path.join(tmp, "empty")
        os.makedirs(empty_dir)
        try:
            build(empty_dir, quiet=True)
        except InputMissingError as exc:
            check("export files missing" in str(exc),
                  "сообщение об отсутствии export: %r" % str(exc))
        else:
            raise AssertionError("отсутствие export/functions.csv не обработано")
        check(not os.path.exists(os.path.join(empty_dir, OUT_CSV)),
              "при отсутствии входов не должно быть выхода")

        # Есть functions.csv, но нет string-matches.csv -> тоже ошибка.
        partial_dir = os.path.join(tmp, "partial")
        os.makedirs(os.path.join(partial_dir, EXPORT_DIR))
        with open(os.path.join(partial_dir, EXPORT_DIR, FUNCTIONS_CSV),
                  "w", encoding="utf-8", newline="") as handle:
            handle.write("address,name,size,calls,called_by\n00010000,FUN_00010000,16,,\n")
        try:
            build(partial_dir, quiet=True)
        except InputMissingError as exc:
            check("missing" in str(exc), "сообщение об отсутствии matches: %r" % str(exc))
        else:
            raise AssertionError("отсутствие string-matches.csv не обработано")
    finally:
        import shutil
        shutil.rmtree(tmp, ignore_errors=True)

    print("selftest OK: %d проверок" % checks[0])


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def setup_stdout():
    """В конвейере Windows Python пишет в кодировке локали - принудительно utf-8.

    Интерактивную консоль не трогаем: там Python сам использует Unicode API.
    """
    stream = sys.stdout
    if stream is None or stream.isatty():
        return
    reconfigure = getattr(stream, "reconfigure", None)
    if reconfigure is None:
        return
    try:
        reconfigure(encoding="utf-8", errors="replace")
    except (ValueError, OSError):
        pass


def main(argv=None):
    setup_stdout()
    argv = list(sys.argv[1:] if argv is None else argv)
    base_dir = DEFAULT_BASE_DIR
    selftest = False

    if "--selftest" in argv:
        selftest = True
        argv.remove("--selftest")
    if "--base-dir" in argv:
        index = argv.index("--base-dir")
        if index + 1 >= len(argv):
            print("usage: join_functions.py [--selftest] [--base-dir DIR]")
            return 2
        base_dir = argv[index + 1]
        del argv[index:index + 2]
    unknown = [arg for arg in argv if arg.startswith("-")]
    if unknown:
        print("unknown arguments: %s" % " ".join(unknown))
        print("usage: join_functions.py [--selftest] [--base-dir DIR]")
        return 2

    if selftest:
        try:
            run_selftest()
        except AssertionError as exc:
            print("selftest FAILED: %s" % exc)
            return 1
        return 0

    try:
        stats, _rows, csv_path, md_path = build(base_dir)
    except InputMissingError as exc:
        print(str(exc))
        return 1
    print_summary(stats, csv_path, md_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
