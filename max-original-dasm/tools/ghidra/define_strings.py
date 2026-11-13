# -*- coding: utf-8 -*-
# preScript: определяет ASCII-строки по оффсетам из string-matches.csv, чтобы
# автоанализ построил xref'ы из кода на эти строки (розеттский камень).
# @category MAX.RE

import csv
import os

csv_path = "/src/max-original-dasm/string-matches.csv"
if not os.path.exists(csv_path):
    print("string-matches.csv not found; skipping string definition")
    raise SystemExit(0)

program = getCurrentProgram()
listing = program.getListing()
space = program.getAddressFactory().getDefaultAddressSpace()
base = 0x10000

defined = 0
with open(csv_path, "r") as f:
    for row in csv.reader(f):
        if not row or not row[0].isdigit():
            continue
        off = int(row[0])
        addr = space.getAddress(base + off)
        if listing.getUndefinedDataAt(addr) is None:
            continue
        try:
            createAsciiString(addr)
            defined += 1
        except Exception:
            pass

print("strings defined: %d" % defined)
