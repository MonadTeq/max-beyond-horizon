#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Disassemble regions of maxrun.flat with capstone.
Usage: python dis_region.py <start_flat_off> <end_flat_off> <out_file> [base_override]
Offsets are FLAT offsets (vaddr = flat + 0x10000)."""
import sys
import traceback
from capstone import Cs, CS_ARCH_X86, CS_MODE_32

FLAT = r"D:\dev\max-port-debug\max-original-dasm\maxrun.flat"


def main():
    try:
        start = int(sys.argv[1], 0)
        end = int(sys.argv[2], 0)
        out = sys.argv[3]
        base = int(sys.argv[4], 0) if len(sys.argv) > 4 else 0x10000
        data = open(FLAT, "rb").read()
        md = Cs(CS_ARCH_X86, CS_MODE_32)
        md.skipdata = True
        lines = []
        for ins in md.disasm(data[start:end], base + start):
            lines.append("0x%08X  %-10s %s" % (ins.address, ins.mnemonic, ins.op_str))
        with open(out, "w", encoding="utf-8") as f:
            f.write("\n".join(lines))
        print("wrote %d instructions to %s" % (len(lines), out))
    except Exception:
        traceback.print_exc()
        sys.exit(2)


if __name__ == "__main__":
    main()
