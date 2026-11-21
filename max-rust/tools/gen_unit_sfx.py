"""Генератор таблицы звуков боя (орудие/попадание/взрыв) для Rust-клиента.

Источник — `assets/PATCHES.RES` item `SC_UNITS` (`Units::LoadResource`,
`max-cpp-port/src/units.cpp`), поля:

  * `sfx_types` — `persistent` / `infinite_looping` каждого `SfxType`;
  * `default_sound_effects` — `resource_id` + `volume` по умолчанию;
  * `units[TOKEN].sound_effects` — переопределения конкретного типа.

Порт повторяет `ParseSoundEffects` (`units.cpp:228-290`): для каждого из 18
`SfxType` берётся звук юнита, иначе `default_sound_effects`, иначе
`INVALID_ID`; признак `default` = «юнит своего звука не задавал»
(`info.is_default = !has_unit_specific`).

Для клиента достаточно трёх транзиентных эффектов, которые C++ проигрывает в
бою (`sound_manager.cpp:413-490`, `unitinfo.cpp:4725`,
`units_manager.cpp:3922/3929`):

  * `fire`    — `UnitInfo::PrepareFire` (`unitinfo.cpp:4718-4726`);
  * `hit`     — `UnitsManager_StartExplosion` при `hits > 0` (`units_manager.cpp:3922`);
  * `expload` — `UnitsManager_StartExplosion` при `hits == 0` (`units_manager.cpp:3929`).

Вывод — `max-rust/crates/max_app/data/unit_sfx.json` (коммитится, читается
крейтом через `include_str!`).

Запуск (из корня репозитория):

    python max-rust/tools/gen_unit_sfx.py
"""

import json
import struct
import sys
from pathlib import Path

XOR_TABLE = bytes([
    0x6c, 0x57, 0x36, 0xe6, 0x81, 0xe0, 0x72, 0x8a, 0xd3, 0xd9, 0xff,
    0x54, 0x48, 0x3a, 0xcd, 0x75, 0xd5, 0x0d, 0xe9, 0xe7, 0x7a, 0x57,
    0xae, 0xeb, 0x61, 0x16, 0xb0, 0x35, 0x55, 0x62, 0xed, 0xd1,
])

# Порядок `UnitType` (max-cpp-port/src/resourcetable.cpp).
UNIT_TOKENS = """COMMTWR POWERSTN POWGEN BARRACKS SHIELDGN RADAR ADUMP FDUMP GOLDSM DEPOT HANGAR DOCK
CNCT_4W LRGRUBLE SMLRUBLE LRGTAPE SMLTAPE LRGSLAB SMLSLAB LRGCONES SMLCONES ROAD LANDPAD SHIPYARD
LIGHTPLT LANDPLT SUPRTPLT AIRPLT HABITAT RESEARCH GREENHSE RECCENTR TRAINHAL WTRPLTFM GUNTURRT
ANTIAIR ARTYTRRT ANTIMSSL BLOCK BRIDGE MININGST LANDMINE SEAMINE LNDEXPLD AIREXPLD SEAEXPLD
BLDEXPLD HITEXPLD MASTER CONSTRCT SCOUT TANK ARTILLRY ROCKTLCH MISSLLCH SP_FLAK MINELAYR SURVEYOR
SCANNER SPLYTRCK GOLDTRCK ENGINEER BULLDOZR REPAIR FUELTRCK CLNTRANS COMMANDO INFANTRY FASTBOAT
CORVETTE BATTLSHP SUBMARNE SEATRANS MSSLBOAT SEAMNLYR CARGOSHP FIGHTER BOMBER AIRTRANS AWAC JUGGRNT
ALNTANK ALNASGUN ALNPLANE ROCKET TORPEDO ALNMISSL ALNTBALL ALNABALL RKTSMOKE TRPBUBLE HARVSTER
WALDO""".split()

# Транзиентные звуки боя, которые нужны клиенту (ключи SC_UNITS).
SFX_KINDS = ["fire", "hit", "expload"]


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
    missing = wanted - set(out)
    assert not missing, f"нет ресурсов: {missing}"
    return out


def resolve(unpacked, sfx_types, defaults, unit_sfx, key):
    """`ParseSoundEffects` для одного `SfxType` (`units.cpp:228-290`)."""
    flags = sfx_types[key]
    info = {
        "resource": None,
        "volume": 0,
        "persistent": bool(flags["persistent"]),
        "looping": bool(flags["infinite_looping"]),
        "default": True,
    }

    has_unit_specific = key in unit_sfx
    if has_unit_specific:
        info["resource"] = unit_sfx[key]["resource_id"]
        info["volume"] = unit_sfx[key]["volume"]
    elif key in defaults:
        info["resource"] = defaults[key]["resource_id"]
        info["volume"] = defaults[key]["volume"]

    info["default"] = not has_unit_specific
    return info


def main():
    root = Path(__file__).resolve().parents[2]
    resources = read_res(root / "assets" / "PATCHES.RES", {"SC_UNITS"})
    sc_units = resources["SC_UNITS"]
    units = sc_units["units"]
    sfx_types = sc_units["sfx_types"]
    defaults = sc_units["default_sound_effects"]

    assert len(units) == len(UNIT_TOKENS), (len(units), len(UNIT_TOKENS))
    assert list(units) == UNIT_TOKENS, "порядок SC_UNITS не совпал с UnitType"

    records = []
    for token in UNIT_TOKENS:
        unit_sfx = units[token].get("sound_effects", {})
        record = {"token": token}
        for kind in SFX_KINDS:
            resolved = resolve(units, sfx_types, defaults, unit_sfx, kind)
            if resolved["resource"] in (None, "INVALID_ID"):
                record[kind] = None
            else:
                record[kind] = {
                    "resource": resolved["resource"],
                    "volume": resolved["volume"],
                    "default": resolved["default"],
                }
        records.append(record)

    with_unit_specific = sum(
        1 for r in records for kind in SFX_KINDS
        if r[kind] is not None and not r[kind]["default"]
    )
    with_any = sum(
        1 for r in records for kind in SFX_KINDS if r[kind] is not None
    )

    payload = {
        "$comment": "Generated by max-rust/tools/gen_unit_sfx.py from "
        "assets/PATCHES.RES:SC_UNITS (sfx_types/default_sound_effects/"
        "units[].sound_effects) — порт ParseSoundEffects (units.cpp). "
        "Do not edit by hand.",
        "kinds": SFX_KINDS,
        "units": records,
    }

    out = (
        Path(__file__).resolve().parents[1]
        / "crates"
        / "max_app"
        / "data"
        / "unit_sfx.json"
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"written {out} ({out.stat().st_size} bytes)")
    print(
        f"{len(records)} unit types x {len(SFX_KINDS)} sounds: "
        f"{with_any} resolved ({with_unit_specific} unit-specific, "
        f"{with_any - with_unit_specific} from default_sound_effects)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
