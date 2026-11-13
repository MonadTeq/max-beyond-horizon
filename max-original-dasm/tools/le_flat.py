#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Разворачивает DOS/4GW (DOS/32A bound) LE-исполняемый файл в плоский образ.

Использование: python le_flat.py MAXRUN.EXE out.flat
"""

import struct
import sys


def u16(b, o):
    return struct.unpack_from("<H", b, o)[0]


def u32(b, o):
    return struct.unpack_from("<I", b, o)[0]


def main():
    src, dst = sys.argv[1], sys.argv[2]
    data = open(src, "rb").read()

    e_lfanew = u32(data, 0x3C)
    assert data[e_lfanew:e_lfanew + 2] == b"LE", "no LE magic at %#x" % e_lfanew

    cpu = u16(data, e_lfanew + 8)
    os_type = u16(data, e_lfanew + 0x0A)
    page_count = u32(data, e_lfanew + 0x14)
    eip_object = u32(data, e_lfanew + 0x18)
    eip = u32(data, e_lfanew + 0x1C)
    esp_object = u32(data, e_lfanew + 0x20)
    esp = u32(data, e_lfanew + 0x24)
    page_size = u32(data, e_lfanew + 0x28)
    page_shift = u32(data, e_lfanew + 0x2C)
    obj_table_off = u32(data, e_lfanew + 0x40)
    obj_count = u32(data, e_lfanew + 0x44)
    page_table_off = u32(data, e_lfanew + 0x48)

    print("LE header @%#x: cpu=%d os=%d pages=%d page_size=%#x shift=%d objs=%d"
          % (e_lfanew, cpu, os_type, page_count, page_size, page_shift, obj_count))

    objects = []
    min_base = 0xFFFFFFFF
    max_end = 0
    for i in range(obj_count):
        o = e_lfanew + obj_table_off + i * 24
        vsize = u32(data, o)
        base = u32(data, o + 4)
        flags = u32(data, o + 8)
        first_page = u32(data, o + 12)
        pages = u32(data, o + 16)
        objects.append((vsize, base, flags, first_page, pages))
        if base and base < min_base:
            min_base = base
        if base + vsize > max_end:
            max_end = base + vsize
        print("  obj %d: vsize=%#x base=%#x flags=%#x pages=%d first=%#x"
              % (i, vsize, base, flags, pages, first_page))

    # В bound-DOS/4GW номера объектов в entry/esp-полях 1-базовые: eip_object=1
    # означает obj0 (доказательство: esp_object=3 -> obj2.base+esp = вершина образа).
    entry = objects[eip_object - 1][1] + eip if 0 < eip_object <= obj_count else 0xFFFFFFFF
    print("entry: obj %d (0-based %d) eip %#x -> linear %#x, esp obj %d esp %#x"
          % (eip_object, eip_object - 1, eip, entry, esp_object, esp))

    image = bytearray(max_end - min_base)
    # Эмпирическая раскладка bound-DOS/4GW: линейные страницы начинаются со
    # смещения 0x3AB00 (конец MZ-стаба; там сигнатура стаба CC EB FD),
    # каждая следующая страница объекта — +0x1000. Номера страниц в таблице
    # 1-базовые (obj.0 начинается с 1, obj.2 с 0x130).
    pages_start = 0x3AB00
    assert data[pages_start:pages_start + 3] == b"\xCC\xEB\xFD", "stub signature mismatch"
    for i, (vsize, base, flags, first_page, pages) in enumerate(objects):
        for p in range(pages):
            file_off = pages_start + (first_page + p - 1) * page_size
            va = base + p * page_size
            chunk = data[file_off:file_off + page_size]
            start = va - min_base
            image[start:start + len(chunk)] = chunk

    open(dst, "wb").write(image)
    with open(dst + ".info", "w") as f:
        f.write("base=%#x\n" % min_base)
        f.write("entry=%#x\n" % entry)
        f.write("size=%#x\n" % len(image))
    print("flat image: %s (%d bytes), base %#x, entry %#x"
          % (dst, len(image), min_base, entry))


if __name__ == "__main__":
    main()
