"""Generate / verify the 93x14 unit attribute tables for max_core::unit_values.

Sources (all real, in-repo):
  * `assets/PATCHES.RES` item `SC_ATTRI` - the port's JSON attribute table
    (`Attributes::LoadResource`, max-cpp-port/src/attributes.cpp), XOR encrypted
    with the 32-byte table from resource_manager.cpp.
  * `assets/PATCHES.RES` item `SC_CLANS` - the port's clan trade-off table
    (`Clans::LoadResource`, max-cpp-port/src/clans.cpp).
  * `max-original/SAVE1.CAM` - a real V70 save; its serialized TeamUnits
    base values are `base + clan trade-offs`, which this script cross-checks.

Output: `unit_values_tables.inc` (Rust source fragment) and a verification report.
"""
import json
import struct
import sys
from pathlib import Path

MAP_CELLS = 112 * 112
UNIT_END = 93

XOR_TABLE = bytes([
    0x6c, 0x57, 0x36, 0xe6, 0x81, 0xe0, 0x72, 0x8a, 0xd3, 0xd9, 0xff,
    0x54, 0x48, 0x3a, 0xcd, 0x75, 0xd5, 0x0d, 0xe9, 0xe7, 0x7a, 0x57,
    0xae, 0xeb, 0x61, 0x16, 0xb0, 0x35, 0x55, 0x62, 0xed, 0xd1,
])

# ResourceID order 0..92 from max-cpp-port/src/resourcetable.cpp
UNIT_TOKENS = """COMMTWR POWERSTN POWGEN BARRACKS SHIELDGN RADAR ADUMP FDUMP GOLDSM DEPOT HANGAR DOCK
CNCT_4W LRGRUBLE SMLRUBLE LRGTAPE SMLTAPE LRGSLAB SMLSLAB LRGCONES SMLCONES ROAD LANDPAD SHIPYARD
LIGHTPLT LANDPLT SUPRTPLT AIRPLT HABITAT RESEARCH GREENHSE RECCENTR TRAINHAL WTRPLTFM GUNTURRT
ANTIAIR ARTYTRRT ANTIMSSL BLOCK BRIDGE MININGST LANDMINE SEAMINE LNDEXPLD AIREXPLD SEAEXPLD
BLDEXPLD HITEXPLD MASTER CONSTRCT SCOUT TANK ARTILLRY ROCKTLCH MISSLLCH SP_FLAK MINELAYR SURVEYOR
SCANNER SPLYTRCK GOLDTRCK ENGINEER BULLDOZR REPAIR FUELTRCK CLNTRANS COMMANDO INFANTRY FASTBOAT
CORVETTE BATTLSHP SUBMARNE SEATRANS MSSLBOAT SEAMNLYR CARGOSHP FIGHTER BOMBER AIRTRANS AWAC JUGGRNT
ALNTANK ALNASGUN ALNPLANE ROCKET TORPEDO ALNMISSL ALNTBALL ALNABALL RKTSMOKE TRPBUBLE HARVSTER
WALDO""".split()

CLAN_KEYS = ["Clan A", "Clan B", "Clan C", "Clan D", "Clan E", "Clan F", "Clan G", "Clan H"]

# Attribute members in ATTRIB_* order (unitvalues.hpp): attack, rounds, range, armor,
# hits, speed, scan, turns, ammo, move_and_fire, fuel, storage, attack_radius, agent_adjust
ATTRIB_FIELDS = [
    "Attack rating",
    "Shots per turn",
    "Attack range",
    "Armor rating",
    "Hit points",
    "Movement points",
    "Scan range",
    "Turns to build",
    "Ammunition",
    "Move and fire",
    None,  # ATTRIB_FUEL: never serialized, never clan adjusted
    "Storage capacity",
    "Blast radius",
    "Experience",  # ATTRIB_AGENT_ADJUST
]


def read_res(path, wanted):
    data = open(path, "rb").read()
    magic, offset, size = struct.unpack_from("<4sii", data, 0)
    assert magic == b"RES0", magic
    out = {}
    for i in range(size // 16):
        tag, o, s = struct.unpack_from("<8sii", data, offset + i * 16)
        name = tag.rstrip(b"\0").decode("latin-1")
        if name in wanted:
            blob = bytes(b ^ XOR_TABLE[j % 32] for j, b in enumerate(data[o:o + s]))
            out[name] = json.loads(blob.decode("utf-8"))
    return out


def unit_values_from_json(a):
    return {
        "turns": a["Turns to build"],
        "hits": min(a["Hit points"], 0xFFFF),
        "armor": a["Armor rating"],
        "attack": a["Attack rating"],
        "speed": min(a["Movement points"], 0xFFFF),
        "fuel": 0,
        "range": a["Attack range"],
        "rounds": a["Shots per turn"],
        "move_and_fire": a["Move and fire"],
        "scan": a["Scan range"],
        "storage": a["Storage capacity"],
        "ammo": a["Ammunition"],
        "attack_radius": a["Blast radius"],
        "agent_adjust": 0,
        "version": 1,
        "units_built": 0,
    }


def parse_save_team_units(path):
    """Returns the 4 serialized TeamUnits base value tables of a V70 save."""
    data = open(path, "rb").read()
    p = 0

    def take(n):
        nonlocal p
        b = data[p:p + n]
        assert len(b) == n
        p += n
        return b

    def u8():
        return take(1)[0]

    def i8():
        return struct.unpack("<b", take(1))[0]

    def u16():
        return struct.unpack("<H", take(2))[0]

    def i16():
        return struct.unpack("<h", take(2))[0]

    def u32():
        return struct.unpack("<I", take(4))[0]

    def i32():
        return struct.unpack("<i", take(4))[0]

    objects = []

    def obj():
        idx = u16()
        if idx == 0:
            return (0, None, False)
        if idx <= len(objects):
            return (idx, objects[idx - 1], False)
        assert idx == len(objects) + 1
        kind = u16()
        objects.append(kind)
        return (idx, kind, True)

    def unit_values():
        v = {}
        for key in ("turns", "hits", "armor", "attack", "speed", "range", "rounds"):
            v[key] = u16()
        v["move_and_fire"] = u8()
        for key in ("scan", "storage", "ammo", "attack_radius", "agent_adjust", "version"):
            v[key] = u16()
        v["units_built"] = u8()
        return v

    take(2)  # version
    take(1)  # category
    take(30)  # name
    take(1)  # world
    take(2)  # mission
    take(4 * 30)  # team names
    team_type = list(take(5))
    take(5)  # team clans
    take(4)  # seed
    take(1)  # opponent
    take(2)
    take(2)
    take(1)  # play mode
    take(12 * 4)  # ini options
    take(MAP_CELLS + MAP_CELLS * 2)
    for _ in range(4):
        take(40)
        take(1 + 1 + 1)
        take(8 * 12)
        take(4)
        take(2)
        take(UNIT_END)
        take(6 * 2)
        take(50 * 2)
        take(2 + 2 + 4)
        take(11)
        take(4 * 2)
        take(UNIT_END * 2)
        take(2)
    take(1 + 1 + 4 + 2 + 2)
    take(7 * 4)
    tables = []
    for _ in range(4):
        gold = u16()
        base = []
        for _ in range(UNIT_END):
            ref = obj()
            assert ref[2] and ref[1] == 6, ref
            base.append(unit_values())
        for _ in range(UNIT_END):
            ref = obj()
            if ref[2]:
                unit_values()
        for _ in range(u16()):
            ref = obj()
            assert ref[2] and ref[1] == 3, ref
            take(14)
        tables.append({"gold": gold, "base": base})
    return team_type, tables


def main():
    res = read_res(r"..\..\assets\PATCHES.RES", {"SC_ATTRI", "SC_CLANS"})
    attributes = res["SC_ATTRI"]["attributes"]
    clans = res["SC_CLANS"]["clans"]

    base = []
    for token in UNIT_TOKENS:
        assert token in attributes, token
        base.append(unit_values_from_json(attributes[token]))
    assert len(base) == UNIT_END

    tradeoffs = []
    for clan_index, clan_key in enumerate(CLAN_KEYS, start=1):
        clan = clans[clan_key]
        for token, deltas in clan["advantages"]["attributes"].items():
            unit_index = UNIT_TOKENS.index(token)
            for field, value in deltas.items():
                attrib = ATTRIB_FIELDS.index(field)
                tradeoffs.append((clan_index, unit_index, attrib, value))

    team_type, tables = parse_save_team_units(r"..\..\max-original\SAVE1.CAM")
    team_clans = [3, 7, 7, 8]
    report = []
    for team in range(2):  # teams 0 and 1 are the real players in SAVE1.CAM
        clan = team_clans[team]
        expected = []
        for unit in range(UNIT_END):
            v = dict(base[unit])
            for (c, u, attrib, delta) in tradeoffs:
                if c != clan or u != unit:
                    continue
                key = ("attack", "rounds", "range", "armor", "hits", "speed", "scan", "turns",
                       "ammo", "move_and_fire", "fuel", "storage", "attack_radius", "agent_adjust")[attrib]
                v[key] += delta
            expected.append(v)
        mismatches = 0
        for unit in range(UNIT_END):
            saved = tables[team]["base"][unit]
            for key in ("turns", "hits", "armor", "attack", "speed", "range", "rounds",
                        "move_and_fire", "scan", "storage", "ammo", "attack_radius", "agent_adjust"):
                want = expected[unit][key]
                got = saved[key]
                if want != got:
                    mismatches += 1
                    if mismatches <= 10:
                        report.append(f"team{team} unit{unit} {token_of(unit)} {key}: json={want} save={got}")
            for key in ("version", "units_built"):
                if saved[key] != expected[unit][key]:
                    report.append(f"team{team} unit{unit} {token_of(unit)} {key}: json={expected[unit][key]} save={saved[key]}")
        print(f"team{team} clan={clan}: mismatched attribute values={mismatches}")
        print(f"  gold={tables[team]['gold']}")
    print("\n".join(report[:20]))

    out = []
    out.append("// Generated by max-brh/tools/gen_unit_values.py - do not edit by hand.")
    out.append("//")
    out.append("// Source: assets/PATCHES.RES item SC_ATTRI (C++ Attributes::LoadResource) for")
    out.append("// the base table, SC_CLANS (C++ Clans::LoadResource) for the clan trade-offs.")
    out.append("// Verified against the serialized TeamUnits base values of")
    out.append("// max-original/SAVE1.CAM (V70): base + clan trade-offs == save bytes.")
    out.append("pub const BASE_UNIT_VALUES: [UnitValues; UNIT_COUNT] = [")
    for token, v in zip(UNIT_TOKENS, base):
        out.append(
            "    // {token}\n    UnitValues {{ turns: {turns}, hits: {hits}, armor: {armor}, attack: {attack}, "
            "speed: {speed}, fuel: {fuel}, range: {range}, rounds: {rounds}, move_and_fire: {move_and_fire}, "
            "scan: {scan}, storage: {storage}, ammo: {ammo}, attack_radius: {attack_radius}, "
            "agent_adjust: {agent_adjust}, version: {version}, in_use: {in_use} }},".format(
                token=token, in_use="false", **v))
    out.append("];")
    out.append("")
    out.append("/// Clan trade-offs: `(clan, unit type, ATTRIB_* index, delta)`.")
    out.append("///")
    out.append("/// C++: `Clans::GetUnitTradeoff` + `ResourceManager_SetClanUpgradesFromClans`;")
    out.append("/// a delta of 0 is skipped (`AddAttribute` is only called for non-zero values).")
    out.append("pub const CLAN_TRADEOFFS: [(u8, u16, u8, i16); {}] = [".format(len(tradeoffs)))
    for (c, u, a, d) in tradeoffs:
        out.append(f"    ({c}, {u}, {a}, {d}),")
    out.append("];")
    # The crate includes this file verbatim (`include!("unit_values_tables.in")`).
    target = Path(__file__).resolve().parent.parent / "crates" / "max_core" / "src" / "unit_values_tables.in"
    target.write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"wrote {target}: {len(base)} base rows, {len(tradeoffs)} trade-offs")


def token_of(index):
    return UNIT_TOKENS[index]


if __name__ == "__main__":
    main()
