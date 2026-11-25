#!/usr/bin/env python3
"""Generate the minimal synthetic PATCHES.RES fixture.

The fixture is deliberately NOT a copy of the game's PATCHES.RES: it holds two
synthetic mission descriptors (XOR-obfuscated JSON, the same encoding the game
uses for `SC_*` records) and nothing else.  It is a valid RES archive, so it can
be opened by `max_assets::ResArchive`, `MissionRegistry` and the asset loader,
while every archive-specific assertion in the tests uses only the data declared
here.

Usage (from the repository root):
    python tests/fixtures/generate_patches_res.py
"""

import json
import pathlib
import struct

# `ResourceManager_GenericTable` (`resource_manager.cpp:156`), see
# `max_assets/src/mission.rs::GENERIC_TABLE`.
GENERIC_TABLE = bytes(
    [
        0x6C, 0x57, 0x36, 0xE6, 0x81, 0xE0, 0x72, 0x8A,
        0xD3, 0xD9, 0xFF, 0x54, 0x48, 0x3A, 0xCD, 0x75,
        0xD5, 0x0D, 0xE9, 0xE7, 0x7A, 0x57, 0xAE, 0xEB,
        0x61, 0x16, 0xB0, 0x35, 0x55, 0x62, 0xED, 0xD1,
    ]
)

HEADER_SIZE = 12
ENTRY_SIZE = 16
TAG_LEN = 8


def xor_encode(data: bytes) -> bytes:
    return bytes(
        byte ^ GENERIC_TABLE[index % len(GENERIC_TABLE)]
        for index, byte in enumerate(data)
    )


def mission_descriptor(tag: str, category: str, title: str) -> bytes:
    """One mission descriptor: XOR-obfuscated JSON with the required fields."""
    payload = {
        "author": "synthetic fixture",
        "copyright": "synthetic fixture",
        "license": "synthetic fixture",
        "category": category,
        "title": {"text": {"en-US": title}},
        "description": {"text": {"en-US": f"Synthetic descriptor {tag}."}},
    }
    raw = json.dumps(payload, separators=(",", ":"), sort_keys=True).encode("utf-8")
    return xor_encode(raw)


def build_archive(entries: list[tuple[str, bytes]]) -> bytes:
    """Write a RES archive: signature, data, then the 16-byte record table."""
    data = bytearray(HEADER_SIZE)
    table = bytearray()

    for tag, payload in entries:
        encoded_tag = tag.encode("ascii")
        assert len(encoded_tag) <= TAG_LEN, tag
        table += encoded_tag.ljust(TAG_LEN, b"\x00")
        table += struct.pack("<II", len(data), len(payload))
        data += payload

    table_offset = len(data)
    data += table
    data[0:4] = b"RES0"
    data[4:8] = struct.pack("<I", table_offset)
    data[8:12] = struct.pack("<I", len(table))
    return bytes(data)


def main() -> None:
    fixture = pathlib.Path(__file__).with_name("assets") / "PATCHES.RES"
    entries = [
        ("SC_CUST1", mission_descriptor("SC_CUST1", "Custom", "Synthetic Custom")),
        ("SC_HOTS1", mission_descriptor("SC_HOTS1", "Hot-seat", "Synthetic Hot-seat")),
    ]
    archive = build_archive(entries)
    fixture.parent.mkdir(parents=True, exist_ok=True)
    fixture.write_bytes(archive)
    print(f"wrote {fixture} ({len(archive)} bytes, {len(entries)} records)")


if __name__ == "__main__":
    main()
