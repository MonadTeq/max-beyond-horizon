#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Сопоставляет строки MAXRUN.EXE (из radare2 izz) со строками исходников C++.

Вход: 01-strings.txt (radare2), каталог исходников C++ (max-port-debug/src)
Выход: string-matches.csv — строка, оффсет в плоском образе, файлы C++.
"""

import csv
import os

HERE = os.path.dirname(os.path.abspath(__file__))
DASM_DIR = os.path.normpath(os.path.join(HERE, ".."))
STRINGS = os.path.join(DASM_DIR, "01-strings.txt")
CPP_ROOT = os.path.normpath(os.path.join(HERE, "..", "..", "..", "src"))

# Плоский образ начинается с 0x3AB00 файла (конец MZ-стаба).
FLAT_BASE = 0x3AB00


def parse_izz_line(line):
    # Формат radare2 izz: nth paddr vaddr len size section type string
    # (колонка section может быть пустой — тип может быть на позиции 5 или 6).
    parts = line.split()
    if len(parts) < 7 or not parts[1].startswith("0x"):
        return None
    try:
        paddr = int(parts[1], 16)
    except ValueError:
        return None
    types = {"ascii", "utf8", "utf16le", "utf32le", "utf16be", "utf32be"}
    if parts[5] in types:
        text = " ".join(parts[6:])
    elif len(parts) > 6 and parts[6] in types:
        text = " ".join(parts[7:])
    else:
        text = " ".join(parts[5:])
    if paddr < FLAT_BASE:
        return None  # внутри MZ-стаба DOS/32A
    return paddr - FLAT_BASE, text


def main():
    cpp_strings = {}
    for root, _dirs, files in os.walk(CPP_ROOT):
        for name in files:
            if not name.endswith((".cpp", ".hpp", ".h", ".c")):
                continue
            path = os.path.join(root, name)
            rel = os.path.relpath(path, CPP_ROOT)
            try:
                content = open(path, "r", encoding="utf-8", errors="replace").read()
            except OSError:
                continue
            cpp_strings[content] = rel

    rows = []
    for line in open(STRINGS, encoding="utf-8", errors="replace"):
        parsed = parse_izz_line(line)
        if not parsed:
            continue
        off, text = parsed
        if len(text) < 6:
            continue
        hits = [rel for content, rel in cpp_strings.items() if text in content]
        if hits:
            rows.append((off, text, ";".join(hits)))

    out = os.path.join(DASM_DIR, "string-matches.csv")
    with open(out, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["flat_offset", "string", "cpp_files"])
        w.writerows(rows)
    print("matched %d strings -> %s" % (len(rows), out))


if __name__ == "__main__":
    main()
