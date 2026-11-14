#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Конвертер JSON-экспорта radare2 в CSV, который понимает join_functions.py.

Входы (по умолчанию относительно каталога max-original-dasm/):
  export/r2_functions.json  массив функций из r2 `aflj`
  export/r2_strings.json    массив строк: {addr, text, xrefs[]}

Выходы:
  export/functions.csv      address,name,size,calls,called_by
  export/strings.csv        address,string,xrefs

Формат совместим с tools/join_functions.py:
  * address - hex без 0x, минимум 8 цифр (например 0010c77c);
  * name и size берутся из aflj как есть;
  * calls/called_by намеренно пустые (каталогу они не нужны);
  * string - текст из r2_strings.json (CSV сам экранирует кавычки/запятые);
  * xrefs - hex-адреса `from` из axtj через ";".

Запуск:
  python tools/r2_to_csv.py                  # реальные export/*.json
  python tools/r2_to_csv.py --selftest       # самотест на синтетических JSON
  python tools/r2_to_csv.py --base-dir DIR   # другой каталог max-original-dasm
"""

import csv
import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DASM_DIR = os.path.dirname(HERE)

EXPORT_DIR = "export"
FUNCTIONS_JSON = "r2_functions.json"
STRINGS_JSON = "r2_strings.json"
FUNCTIONS_CSV = "functions.csv"
STRINGS_CSV = "strings.csv"

FUNCTIONS_HEADER = ["address", "name", "size", "calls", "called_by"]
STRINGS_HEADER = ["address", "string", "xrefs"]


class InputMissingError(Exception):
    """Нет обязательного входного JSON (или он пуст/не того типа)."""


# ---------------------------------------------------------------------------
# Разбор значений
# ---------------------------------------------------------------------------

def parse_int(value, base=10):
    """Разбирает целое: None -> None, 0x-префикс, иначе указанное основание."""
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    text = str(value).strip()
    if not text:
        return None
    try:
        if text[:2].lower() == "0x":
            return int(text, 16)
        return int(text, base)
    except ValueError:
        try:
            return int(text, 16)
        except ValueError:
            return None


def address_text(value):
    """Адрес в формате экспорта: hex без 0x, минимум 8 цифр."""
    return "%08x" % value


def load_json(path):
    with open(path, "r", encoding="utf-8-sig") as handle:
        return json.load(handle)


def as_list(data, key):
    """aflj - это массив; на всякий случай принимаем и {key: [...]}."""
    if isinstance(data, dict):
        data = data.get(key)
    return data if isinstance(data, list) else []


# ---------------------------------------------------------------------------
# Входные JSON
# ---------------------------------------------------------------------------

def read_functions(path):
    """r2_functions.json -> отсортированные функции (по одной на адрес)."""
    items = as_list(load_json(path), "functions")
    funcs = {}
    for item in items:
        if not isinstance(item, dict):
            continue
        raw_addr = item.get("offset")
        if raw_addr is None:
            raw_addr = item.get("addr")
        addr = parse_int(raw_addr, 16)
        if addr is None:
            continue
        size = parse_int(item.get("size"), 10)
        if size is None or size < 0:
            size = 0
        name = item.get("name")
        name = "" if name is None else str(name)
        old = funcs.get(addr)
        if old is None:
            funcs[addr] = {"address": addr, "name": name, "size": size}
        else:
            # Дубликат: оставляем максимальный размер и непустое имя.
            old["size"] = max(old["size"], size)
            if not old["name"] and name:
                old["name"] = name
    return sorted(funcs.values(), key=lambda func: func["address"])


def xref_from(ref):
    """Достаёт адрес источника из элемента axtj (dict) или голого адреса."""
    if isinstance(ref, dict):
        return parse_int(ref.get("from"), 16)
    return parse_int(ref, 16)


def read_strings(path):
    """r2_strings.json -> отсортированные строки с уникальными xref'ами."""
    items = as_list(load_json(path), "strings")
    strings = {}
    for item in items:
        if not isinstance(item, dict):
            continue
        addr = parse_int(item.get("addr"), 16)
        if addr is None:
            addr = parse_int(item.get("address"), 16)
        text = item.get("text")
        if addr is None or text is None:
            continue
        text = str(text)
        if not text:
            continue
        xrefs = []
        for ref in item.get("xrefs") or []:
            from_addr = xref_from(ref)
            if from_addr is not None and from_addr not in xrefs:
                xrefs.append(from_addr)
        old = strings.get(addr)
        if old is None:
            strings[addr] = {
                "address": addr,
                "text": text,
                "xrefs": sorted(xrefs),
            }
        else:
            # Та же строка пришла дважды: объединяем xref'ы, текст первый.
            merged = sorted(set(old["xrefs"]) | set(xrefs))
            old["xrefs"] = merged
    return sorted(strings.values(), key=lambda item: item["address"])


# ---------------------------------------------------------------------------
# Запись CSV
# ---------------------------------------------------------------------------

def write_functions_csv(rows, path):
    with open(path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, lineterminator="\n")
        writer.writerow(FUNCTIONS_HEADER)
        for row in rows:
            # calls/called_by не заполняем: конвейеру нужны адрес, имя и размер.
            writer.writerow([address_text(row["address"]), row["name"], row["size"], "", ""])


def write_strings_csv(rows, path):
    with open(path, "w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, lineterminator="\n")
        writer.writerow(STRINGS_HEADER)
        for row in rows:
            xrefs = ";".join(address_text(xref) for xref in row["xrefs"])
            writer.writerow([address_text(row["address"]), row["text"], xrefs])


# ---------------------------------------------------------------------------
# Сборка
# ---------------------------------------------------------------------------

def build(base_dir=DASM_DIR, quiet=False):
    """Читает export/*.json, пишет export/functions.csv и export/strings.csv."""
    base_dir = os.path.abspath(base_dir)
    export_dir = os.path.join(base_dir, EXPORT_DIR)
    functions_json = os.path.join(export_dir, FUNCTIONS_JSON)
    strings_json = os.path.join(export_dir, STRINGS_JSON)
    functions_csv = os.path.join(export_dir, FUNCTIONS_CSV)
    strings_csv = os.path.join(export_dir, STRINGS_CSV)

    if not os.path.isfile(functions_json):
        raise InputMissingError(
            "missing input: %s\n"
            "  сначала собери экспорт radare2: tools/r2_export.sh" % functions_json
        )
    if not os.path.isfile(strings_json):
        raise InputMissingError(
            "missing input: %s\n"
            "  сначала собери экспорт radare2: tools/r2_export.sh" % strings_json
        )

    functions = read_functions(functions_json)
    if not functions:
        raise InputMissingError("no functions in %s" % functions_json)
    strings = read_strings(strings_json)

    write_functions_csv(functions, functions_csv)
    write_strings_csv(strings, strings_csv)

    stats = {
        "functions": len(functions),
        "strings": len(strings),
        "strings_with_xrefs": sum(1 for item in strings if item["xrefs"]),
        "functions_csv": functions_csv,
        "strings_csv": strings_csv,
    }
    if not quiet:
        print("Функций: %d; строк: %d (с xref'ами: %d)"
              % (stats["functions"], stats["strings"], stats["strings_with_xrefs"]))
        print("Записано: %s" % functions_csv)
        print("Записано: %s" % strings_csv)
    return stats


# ---------------------------------------------------------------------------
# Самотест
# ---------------------------------------------------------------------------

def run_selftest():
    """Синтетические JSON во временном каталоге; проверка CSV и ошибок."""
    checks = [0]

    def check(condition, message):
        checks[0] += 1
        if not condition:
            raise AssertionError(message)

    tmp = tempfile.mkdtemp(prefix="r2_to_csv_selftest_")
    try:
        export_dir = os.path.join(tmp, EXPORT_DIR)
        os.makedirs(export_dir)

        functions_json = os.path.join(export_dir, FUNCTIONS_JSON)
        with open(functions_json, "w", encoding="utf-8", newline="") as handle:
            handle.write(
                "[\n"
                '  {"offset": 1099644, "name": "fcn.0010c77c", "size": 42, "type": "fcn"},\n'
                '  {"offset": 65536, "name": "entry0", "size": 10},\n'
                '  {"offset": 65536, "name": "entry0.dup", "size": 4}\n'
                "]\n"
            )

        strings_json = os.path.join(export_dir, STRINGS_JSON)
        with open(strings_json, "w", encoding="utf-8", newline="") as handle:
            handle.write(
                "[\n"
                '  {"addr": "0x150120", "text": "Maintain an attack reserve.",\n'
                '   "xrefs": [{"from": 104541, "to": 1376544, "type": "DATA"},\n'
                '             {"from": "0x1ae76", "type": "DATA"},\n'
                '             {"from": 104541, "type": "DATA"}]},\n'
                '  {"addr": "0x150160", "text": "Dump unit task, null move.", "xrefs": []},\n'
                '  {"addr": "0x150200", "text": "a \\"quoted\\", value", "xrefs": [110197]},\n'
                '  {"addr": 1376832, "text": " leading space kept", "xrefs": [69200]}\n'
                "]\n"
            )

        stats = build(tmp, quiet=True)

        check(stats["functions"] == 2, "ожидалось 2 функции, получено %d" % stats["functions"])
        check(stats["strings"] == 4, "ожидалось 4 строки, получено %d" % stats["strings"])
        check(stats["strings_with_xrefs"] == 3,
              "строк с xref'ами: ожидалось 3, получено %d" % stats["strings_with_xrefs"])

        functions_csv = os.path.join(export_dir, FUNCTIONS_CSV)
        strings_csv = os.path.join(export_dir, STRINGS_CSV)
        check(os.path.isfile(functions_csv), "нет functions.csv")
        check(os.path.isfile(strings_csv), "нет strings.csv")

        with open(functions_csv, "r", encoding="utf-8", newline="") as handle:
            rows = list(csv.reader(handle))
        check(rows[0] == FUNCTIONS_HEADER,
              "неверный заголовок functions.csv: %r" % (rows[0],))
        check(len(rows) == 3, "в functions.csv ожидалось 3 строки, есть %d" % len(rows))
        check(rows[1][0] == "00010000", "адрес entry0: %r" % rows[1][0])
        check(rows[1][1] == "entry0", "имя entry0: %r" % rows[1][1])
        check(rows[1][2] == "10", "дубликат должен сохранить max(size): %r" % rows[1][2])
        check(rows[1][3] == "" and rows[1][4] == "", "calls/called_by должны быть пустыми")
        check(rows[2][0] == "0010c77c", "адрес 0x10c77c: %r" % rows[2][0])
        check(rows[2][2] == "42", "размер fcn.0010c77c: %r" % rows[2][2])

        with open(strings_csv, "r", encoding="utf-8", newline="") as handle:
            strings = list(csv.reader(handle))
        check(strings[0] == STRINGS_HEADER,
              "неверный заголовок strings.csv: %r" % (strings[0],))
        check(len(strings) == 5, "в strings.csv ожидалось 5 строк, есть %d" % len(strings))
        check(strings[1][0] == "00150120", "адрес первой строки: %r" % strings[1][0])
        check(strings[1][1] == "Maintain an attack reserve.",
              "текст первой строки: %r" % strings[1][1])
        check(strings[1][2] == "0001985d;0001ae76",
              "xref'ы первой строки (уникальные, по возрастанию): %r" % strings[1][2])
        check(strings[2][1] == "Dump unit task, null move.",
              "строка с запятой: %r" % strings[2][1])
        check(strings[2][2] == "", "пустые xref'ы: %r" % strings[2][2])
        check(strings[3][1] == 'a "quoted", value',
              "строка с кавычками: %r" % strings[3][1])
        check(strings[3][2] == "0001ae75", "xref строки с кавычками: %r" % strings[3][2])
        check(strings[4][0] == "00150240", "адрес из int: %r" % strings[4][0])
        check(strings[4][1] == " leading space kept",
              "ведущий пробел не должен теряться: %r" % strings[4][1])

        # Нет functions.json -> понятная ошибка.
        empty_dir = os.path.join(tmp, "empty")
        os.makedirs(os.path.join(empty_dir, EXPORT_DIR))
        try:
            build(empty_dir, quiet=True)
        except InputMissingError as exc:
            check("missing input" in str(exc), "сообщение об ошибке: %r" % str(exc))
        else:
            raise AssertionError("отсутствие r2_functions.json не обработано")

        # Есть functions.json, но нет strings.json -> тоже ошибка.
        partial_dir = os.path.join(tmp, "partial")
        os.makedirs(os.path.join(partial_dir, EXPORT_DIR))
        with open(os.path.join(partial_dir, EXPORT_DIR, FUNCTIONS_JSON),
                  "w", encoding="utf-8", newline="") as handle:
            handle.write('[{"offset": 65536, "name": "entry0", "size": 1}]')
        try:
            build(partial_dir, quiet=True)
        except InputMissingError as exc:
            check("r2_strings.json" in str(exc),
                  "сообщение об отсутствии strings: %r" % str(exc))
        else:
            raise AssertionError("отсутствие r2_strings.json не обработано")

        # Пустой список функций -> ошибка, а не пустой CSV.
        empty_funcs = os.path.join(tmp, "empty_funcs")
        os.makedirs(os.path.join(empty_funcs, EXPORT_DIR))
        with open(os.path.join(empty_funcs, EXPORT_DIR, FUNCTIONS_JSON),
                  "w", encoding="utf-8", newline="") as handle:
            handle.write("[]")
        with open(os.path.join(empty_funcs, EXPORT_DIR, STRINGS_JSON),
                  "w", encoding="utf-8", newline="") as handle:
            handle.write("[]")
        try:
            build(empty_funcs, quiet=True)
        except InputMissingError as exc:
            check("no functions" in str(exc), "пустые функции: %r" % str(exc))
        else:
            raise AssertionError("пустой aflj не обработан")
    finally:
        import shutil
        shutil.rmtree(tmp, ignore_errors=True)

    print("selftest OK: %d проверок" % checks[0])


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def setup_stdout():
    """В конвейере Windows Python пишет в кодировке локали - принудительно utf-8."""
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


def usage():
    print("usage: r2_to_csv.py [--selftest] [--base-dir DIR]")


def main(argv=None):
    setup_stdout()
    argv = list(sys.argv[1:] if argv is None else argv)
    base_dir = DASM_DIR

    if "--selftest" in argv:
        argv.remove("--selftest")
        if argv:
            usage()
            return 2
        try:
            run_selftest()
        except AssertionError as exc:
            print("selftest FAILED: %s" % exc)
            return 1
        return 0

    if "--base-dir" in argv:
        index = argv.index("--base-dir")
        if index + 1 >= len(argv):
            usage()
            return 2
        base_dir = argv[index + 1]
        del argv[index:index + 2]

    unknown = [arg for arg in argv if arg.startswith("-")]
    if unknown or argv:
        usage()
        return 2

    try:
        build(base_dir)
    except InputMissingError as exc:
        print(str(exc))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
