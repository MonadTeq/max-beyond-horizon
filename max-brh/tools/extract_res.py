"""Extract encrypted JSON resources (SC_ATTRI = unit attributes, SC_CLANS = clans)
from the original game's MAX.RES archive.  Development aid, not part of the build.

RES layout (see max-cpp-port/src/resource_manager.cpp: res_header / res_index):
    header { char id[4] == "RES0"; int32 offset; int32 size; }
    at `offset`: size/16 entries of { char tag[8]; int32 data_offset; int32 data_size; }
The game encrypts selected resources with a repeating 32-byte XOR table.
"""
import struct
import sys

XOR_TABLE = bytes([
    0x6c, 0x57, 0x36, 0xe6, 0x81, 0xe0, 0x72, 0x8a, 0xd3, 0xd9, 0xff,
    0x54, 0x48, 0x3a, 0xcd, 0x75, 0xd5, 0x0d, 0xe9, 0xe7, 0x7a, 0x57,
    0xae, 0xeb, 0x61, 0x16, 0xb0, 0x35, 0x55, 0x62, 0xed, 0xd1,
])


def decode(data):
    return bytes(b ^ XOR_TABLE[i % 32] for i, b in enumerate(data))


def main(path, names):
    data = open(path, "rb").read()
    magic, offset, size = struct.unpack_from("<4sii", data, 0)
    assert magic == b"RES0", magic
    count = size // 16
    items = []
    for i in range(count):
        tag, data_offset, data_size = struct.unpack_from("<8sii", data, offset + i * 16)
        items.append((tag.rstrip(b"\0").decode("latin-1"), data_offset, data_size))
    print(f"{path}: {count} items")
    starts = set()
    for name, off, sz in items:
        if name in names:
            starts.add(off)
            blob = data[off:off + sz]
            for attempt in ("plain", "xor"):
                text = blob if attempt == "plain" else decode(blob)
                try:
                    import json
                    js = json.loads(text.decode("utf-8"))
                except Exception:
                    continue
                out = f"{name}.{attempt}.json"
                json.dump(js, open(out, "w"), indent=1)
                print(f"  {name}: {sz} bytes, decoded as {attempt} -> {out}")
                break
            else:
                print(f"  {name}: {sz} bytes, could not decode")
    if not starts:
        for name, off, sz in items[:40]:
            print(f"    sample tag={name!r} offset={off} size={sz}")


if __name__ == "__main__":
    main(sys.argv[1], set(sys.argv[2:]))
