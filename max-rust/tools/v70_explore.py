"""Ad-hoc V70 save explorer used to validate the Rust parser layout.

Reads a real .CAM save (V70) and walks every section, printing statistics.
Not part of the build; kept as a development aid.
"""
import struct
import sys

MAP_CELLS = 112 * 112
UNIT_END = 93
PLAYER_TEAM_MAX = 5

TYPES = {
    1: "AirPath",
    2: "BuilderPath",
    3: "Complex",
    4: "GroundPath",
    5: "UnitInfo",
    6: "UnitValues",
}


class Buf:
    def __init__(self, data):
        self.d = data
        self.p = 0
        self.objects = []  # (kind, offset)

    def read(self, n):
        b = self.d[self.p:self.p + n]
        assert len(b) == n, ("truncated", self.p, n)
        self.p += n
        return b

    def u8(self):
        return self.read(1)[0]

    def i8(self):
        return struct.unpack("<b", self.read(1))[0]

    def u16(self):
        return struct.unpack("<H", self.read(2))[0]

    def i16(self):
        return struct.unpack("<h", self.read(2))[0]

    def u32(self):
        return struct.unpack("<I", self.read(4))[0]

    def i32(self):
        return struct.unpack("<i", self.read(4))[0]

    def point(self):
        return (self.i16(), self.i16())

    def obj_ref(self, expected=None):
        """Returns (index, kind, is_new)."""
        index = self.u16()
        if index == 0:
            return (0, None, False)
        if index <= len(self.objects):
            kind = self.objects[index - 1][0]
            assert expected is None or kind == expected, (index, kind, expected)
            return (index, kind, False)
        assert index == len(self.objects) + 1, (index, len(self.objects))
        type_index = self.u16()
        kind = TYPES[type_index]
        assert expected is None or kind == expected, (index, type_index, expected)
        self.objects.append((kind, self.p))
        return (index, kind, True)


def parse_unit_values(buf):
    v = {}
    v["turns"] = buf.u16()
    v["hits"] = buf.u16()
    v["armor"] = buf.u16()
    v["attack"] = buf.u16()
    v["speed"] = buf.u16()
    v["range"] = buf.u16()
    v["rounds"] = buf.u16()
    v["move_and_fire"] = buf.u8()
    v["scan"] = buf.u16()
    v["storage"] = buf.u16()
    v["ammo"] = buf.u16()
    v["attack_radius"] = buf.u16()
    v["agent_adjust"] = buf.u16()
    v["version"] = buf.u16()
    v["units_built"] = buf.u8()
    return v


def parse_complex(buf):
    return {
        "material": buf.i16(),
        "fuel": buf.i16(),
        "gold": buf.i16(),
        "power": buf.i16(),
        "workers": buf.i16(),
        "buildings": buf.i16(),
        "id": buf.i16(),
    }


def parse_path(buf, kind):
    if kind == "GroundPath":
        end_x = buf.i16()
        end_y = buf.i16()
        step_index = buf.u16()
        count = buf.u16()
        steps = [(buf.i8(), buf.i8()) for _ in range(count)]
        return {"kind": kind, "end": (end_x, end_y), "step_index": step_index, "steps": steps}
    if kind == "AirPath":
        d = {}
        d["length"] = buf.i16()
        d["angle"] = buf.i8()
        d["start"] = (buf.i16(), buf.i16())
        d["end_x"] = buf.i16()
        d["end_y"] = buf.i16()
        d["step"] = (buf.i32(), buf.i32())
        d["delta"] = (buf.i32(), buf.i32())
        return d
    if kind == "BuilderPath":
        return {"kind": kind, "dir": (buf.i16(), buf.i16())}
    raise AssertionError(kind)


def parse_unit_info(buf):
    u = {}
    u["unit_type"] = buf.u16()
    u["id"] = buf.u16()
    u["flags"] = buf.u32()
    u["x"] = buf.u16()
    u["y"] = buf.u16()
    u["grid_x"] = buf.i16()
    u["grid_y"] = buf.i16()
    name_len = buf.u16()
    u["name"] = buf.read(name_len).decode("utf-8", "replace") if name_len else ""
    u["shadow_offset"] = buf.point()
    u["team"] = buf.u8()
    u["unit_id"] = buf.u8()
    u["brightness"] = buf.u8()
    u["angle"] = buf.u8()
    u["visible_to_team"] = list(buf.read(5))
    u["spotted_by_team"] = list(buf.read(5))
    u["max_velocity"] = buf.u8()
    u["velocity"] = buf.u8()
    u["sound"] = buf.u8()
    u["scaler_adjust"] = buf.u8()
    u["sprite_bounds"] = struct.unpack("<4i", buf.read(16))
    u["shadow_bounds"] = struct.unpack("<4i", buf.read(16))
    u["turret_angle"] = buf.u8()
    u["turret_offset_x"] = buf.i8()
    u["turret_offset_y"] = buf.i8()
    for key in ("total_images", "image_base", "turret_image_base", "firing_image_base",
                "connector_image_base", "image_index", "turret_image_index", "image_index_max"):
        u[key] = buf.i16()
    u["orders"] = buf.u8()
    u["state"] = buf.u8()
    u["prior_orders"] = buf.u8()
    u["prior_state"] = buf.u8()
    u["laying_state"] = buf.u8()
    u["target_grid"] = buf.point()
    for key in ("build_time", "total_mining", "raw_mining", "fuel_mining", "gold_mining",
                "raw_mining_max", "gold_mining_max", "fuel_mining_max"):
        u[key] = buf.u8()
    u["hits"] = buf.u8()
    u["speed"] = buf.u8()
    u["shots"] = buf.u8()
    u["move_and_fire"] = buf.u8()
    u["storage"] = buf.i16()
    u["ammo"] = buf.u8()
    u["targeting_mode"] = buf.u8()
    u["enter_mode"] = buf.u8()
    u["cursor"] = buf.u8()
    u["recoil_delay"] = buf.u8()
    u["delayed_reaction"] = buf.u8()
    u["damaged_this_turn"] = buf.u8()
    u["research_topic"] = buf.u8()
    u["moved"] = buf.u8()
    u["bobbed"] = buf.u8()
    u["shake_effect_state"] = buf.u8()
    u["engine"] = buf.u8()
    u["weapon"] = buf.u8()
    u["comm"] = buf.u8()
    u["fuel_distance"] = buf.u8()
    u["move_fraction"] = buf.u8()
    u["energized"] = buf.u8()
    u["repeat_build"] = buf.u8()
    u["build_rate"] = buf.u16()
    u["disabled_reaction_fire"] = buf.u8()
    u["auto_survey"] = buf.u8()
    u["ai_state_bits"] = buf.u32()
    path_ref = buf.obj_ref()
    if path_ref[2]:
        u["path"] = parse_path(buf, path_ref[1])
    else:
        u["path"] = None
    u["path_ref"] = path_ref[0]
    u["connectors"] = buf.u16()
    bv_ref = buf.obj_ref("UnitValues")
    if bv_ref[2]:
        u["base_values"] = parse_unit_values(buf)
    else:
        u["base_values"] = None
    u["base_values_ref"] = bv_ref[0]
    cx_ref = buf.obj_ref("Complex")
    if cx_ref[2]:
        u["complex"] = parse_complex(buf)
    else:
        u["complex"] = None
    u["complex_ref"] = cx_ref[0]
    parent = buf.obj_ref("UnitInfo")
    u["parent_ref"] = parent[0]
    if parent[2]:
        # nested objects are serialized in full the first time they are referenced
        u["parent_inline"] = parse_unit_info(buf)
    enemy = buf.obj_ref("UnitInfo")
    u["enemy_ref"] = enemy[0]
    if enemy[2]:
        u["enemy_inline"] = parse_unit_info(buf)
    count = buf.u16()
    u["build_list"] = [buf.u16() for _ in range(count)]
    return u


def parse_list(buf):
    count = buf.u16()
    out = []
    for _ in range(count):
        ref = buf.obj_ref("UnitInfo")
        if ref[2]:
            out.append(parse_unit_info(buf))
        else:
            out.append({"ref": ref[0]})
    return out


def main(path, dump_base=False):
    data = open(path, "rb").read()
    buf = Buf(data)
    version = buf.u16()
    save_game_type = buf.u8()
    save_name = buf.read(30)
    world_index = buf.u8()
    mission_index = buf.u16()
    team_names = [buf.read(30) for _ in range(4)]
    team_type = list(buf.read(5))
    team_clan = list(buf.read(5))
    rng_seed = buf.u32()
    opponent = buf.i8()
    turn_timer = buf.u16()
    endturn = buf.u16()
    play_mode = buf.i8()
    ini = [buf.i32() for _ in range(12)]
    print(f"file={path} size={len(data)}")
    print(f"version={version} category={save_game_type} name={save_name.rstrip(bytes([0]))!r} world={world_index} mission={mission_index}")
    print(f"team_names={[n.rstrip(bytes([0])) for n in team_names]}")
    print(f"team_type={team_type} team_clan={team_clan} seed={rng_seed:#x} opponent={opponent} timer={turn_timer} endturn={endturn} play_mode={play_mode}")
    print(f"ini_options={ini}")

    surface = buf.read(MAP_CELLS)
    cargo = buf.read(MAP_CELLS * 2)
    print(f"after maps offset={buf.p} (expect 37856)")
    print(f"surface values={sorted(set(surface))} cargo nonzero={sum(1 for i in range(0, len(cargo), 2) if cargo[i] or cargo[i+1])}")

    teams = []
    for t in range(4):
        markers = [buf.point() for _ in range(10)]
        info = {}
        info["team_type"] = buf.u8()
        info["finished_turn"] = buf.u8()
        info["team_clan"] = buf.u8()
        info["research_topics"] = [tuple(buf.u32() for _ in range(3)) for _ in range(8)]
        info["team_points"] = buf.u32()
        info["number_of_objects_created"] = buf.u16()
        info["unit_counters"] = list(buf.read(UNIT_END))
        info["screen_locations"] = [(buf.i8(), buf.i8()) for _ in range(6)]
        info["score_graph"] = [buf.i16() for _ in range(50)]
        info["selected_unit"] = buf.u16()
        info["zoom_level"] = buf.u16()
        info["camera_position"] = buf.point()
        info["display_buttons"] = list(buf.read(11))
        info["stats"] = [buf.i16() for _ in range(4)]
        info["casualties"] = [buf.u16() for _ in range(UNIT_END)]
        info["stats_gold_spent_on_upgrades"] = buf.i16()
        info["markers"] = markers
        teams.append(info)
    for t, info in enumerate(teams):
        print(f"team{t}: type={info['team_type']} clan={info['team_clan']} points={info['team_points']} objs={info['number_of_objects_created']} research={info['research_topics']} sel={info['selected_unit']:#x} zoom={info['zoom_level']} cam={info['camera_position']} stats={info['stats']} gold_up={info['stats_gold_spent_on_upgrades']}")
        print(f"       counters={info['unit_counters'][:12]}... casualties_sum={sum(info['casualties'])}")
    active = buf.u8()
    player = buf.u8()
    turn_counter = buf.i32()
    game_state = buf.u16()
    turn_timer_val = buf.u16()
    prefs = [buf.i32() for _ in range(7)]
    print(f"game: active={active} player={player} turn_counter={turn_counter} game_state={game_state} timer_value={turn_timer_val} prefs={prefs}")
    print(f"offset after game manager={buf.p}")

    team_units = []
    for t in range(4):
        gold = buf.u16()
        base = []
        refs = []
        for _ in range(UNIT_END):
            ref = buf.obj_ref("UnitValues")
            assert ref[2], f"base value {t} {len(base)} not new"
            base.append(parse_unit_values(buf))
        cur = []
        for _ in range(UNIT_END):
            ref = buf.obj_ref("UnitValues")
            if ref[2]:
                cur.append(parse_unit_values(buf))
            else:
                cur.append(None)
        ncomplex = buf.u16()
        complexes = []
        for _ in range(ncomplex):
            ref = buf.obj_ref("Complex")
            assert ref[2]
            complexes.append(parse_complex(buf))
        team_units.append({"gold": gold, "base": base, "current": cur, "complexes": complexes})
        print(f"team_units{t}: gold={gold} complexes={len(complexes)} current_unique={sum(1 for c in cur if c)}")
    print(f"offset after team units={buf.p}")

    lists = {}
    for name in ("ground_cover", "mobile_land_sea", "stationary", "mobile_air", "particles"):
        lists[name] = parse_list(buf)
        print(f"list {name}: {len(lists[name])}")
    print(f"offset after unit lists={buf.p}")

    unit_hash_size = buf.u16()
    total_units = 0
    for _ in range(unit_hash_size):
        total_units += len(parse_list(buf))
    print(f"unit hash size={unit_hash_size} units={total_units} objects_total={len(buf.objects)}")
    print(f"offset after unit hash={buf.p}")

    map_hash_size = buf.u16()
    map_hash_shift = buf.u16()
    map_objects = 0
    for _ in range(map_hash_size):
        count = buf.u16()
        for _ in range(count):
            x = buf.u16()
            y = buf.u16()
            entries = parse_list(buf)
            map_objects += 1
    print(f"map hash size={map_hash_size} shift={map_hash_shift} objects={map_objects}")
    print(f"offset after map hash={buf.p}")

    for t in range(4):
        if teams[t]["team_type"] == 0:
            print(f"heat map {t}: skipped (empty team)")
            continue
        buf.read(MAP_CELLS)
        buf.read(MAP_CELLS)
        buf.read(MAP_CELLS)
        print(f"heat map {t}: 3 arrays")
    print(f"offset after heat maps={buf.p}")

    logs = 0
    for t in range(4):
        count = buf.u16()
        for _ in range(count):
            length = buf.u16()
            text = buf.read(length)
            ref = buf.obj_ref()
            point = buf.point()
            alert = buf.u8()
            mid = buf.u16()
            logs += 1
        print(f"message log {t}: {count}")
    print(f"offset after message logs={buf.p}")

    for t in range(4):
        if teams[t]["team_type"] == 2:  # computer
            info = {}
            info["player_team"] = buf.u16()
            info["strategy"] = buf.u8()
            info["unused"] = buf.i16()
            info["greenhouse_ratio"] = buf.i16()
            info["minefield_density"] = buf.i16()
            info["target_team"] = buf.i16()
            n = buf.u16()
            for _ in range(n):
                buf.obj_ref("UnitInfo")
                team = buf.u16()
                vis = buf.u8()
                pos = buf.point()
            has_info = buf.u16()
            if has_info:
                buf.read(MAP_CELLS)
            has_mine = buf.u16()
            if has_mine:
                buf.read(MAP_CELLS)
            target = buf.point()
            print(f"ai team{t}: {info} spotted={n} info_map={has_info} mine_map={has_mine} target={target}")
    print(f"final offset={buf.p} / {len(data)} (rest={len(data) - buf.p})")

    if dump_base:
        import json
        json.dump(team_units[0]["base"], open("v70_base.json", "w"), indent=0)
        json.dump([[t["team_clan"], [c for c in team_units[t]["base"]]] for t in range(4)],
                  open("v70_base_all.json", "w"), indent=0)


if __name__ == "__main__":
    main(sys.argv[1], dump_base="--dump" in sys.argv)
