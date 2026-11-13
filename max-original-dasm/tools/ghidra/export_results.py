# -*- coding: utf-8 -*-
# Экспорт результатов анализа Ghidra для MAXRUN.EXE: функции, строки, xrefs.
# Запускается из analyzeHeadless как -postScript. Jython (Python 2) совместимость.
# @category MAX.RE

import csv
import os

out_dir = str(getScriptArgs()[0])
if not os.path.exists(out_dir):
    os.makedirs(out_dir)

program = getCurrentProgram()
listing = program.getListing()
fm = program.getFunctionManager()

# --- functions ---
with open(os.path.join(out_dir, "functions.csv"), "wb") as f:
    w = csv.writer(f)
    w.writerow(["address", "name", "size", "calls", "called_by"])
    for func in fm.getFunctions(True):
        addr = func.getEntryPoint()
        calls = []
        for ref in getReferencesFrom(addr):
            if fm.getFunctionContaining(ref.getToAddress()) is not None:
                calls.append(str(ref.getToAddress()))
        called_by = []
        for ref in getReferencesTo(addr):
            f2 = fm.getFunctionContaining(ref.getFromAddress())
            if f2 is not None:
                called_by.append(str(f2.getEntryPoint()))
        w.writerow([str(addr), func.getName(), func.getBody().getNumAddresses(),
                    ";".join(calls), ";".join(called_by)])

# --- strings with xrefs ---
with open(os.path.join(out_dir, "strings.csv"), "wb") as f:
    w = csv.writer(f)
    w.writerow(["address", "string", "xrefs"])
    data = listing.getDefinedData(True)
    for d in data:
        if d is None or not d.hasStringValue():
            continue
        addr = d.getAddress()
        xrefs = [str(r.getFromAddress()) for r in getReferencesTo(addr)]
        if not xrefs:
            continue
        w.writerow([str(addr), d.getValue(), ";".join(xrefs)])

print("MAX.RE export done: %d functions" % fm.getFunctionCount())
