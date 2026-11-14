// Reference-vector generator for max_core's P7 (victory/CRC/cross-cutting tests).
//
// Two sections, both produced by the *oracle* code, not by the Rust port:
//
//  1. `crc_bytes` / `crc_word` — CRC16 пакета 45.
//     `crc16()` is copied 1:1 from `max-cpp-port/src/crc16.c`; `get_crc16_word()`
//     is copied 1:1 from `GameManager_GetCrc16` (`game_manager.cpp:2315`).
//
//  2. `replay_*` — мини-реплей: карта строится теми же параметрами, что case
//     `p00_l111_d0_s0` в P2-векторах, расставляются два юнита
//     (`UnitsManager_DeployUnit` в объёме полей, которые попадают в дайджест),
//     выполняются 10 тиков и печатается канонический дайджест состояния
//     (`max_core::crc::replay_digest_bytes`), посчитанный независимым писателем.
//
// The map/resource generation is NOT duplicated: `map_vectors/main.cpp` is
// included (its `main` is renamed), so the exact same oracle-copied functions
// run here. The two inputs P2 already had to stand in for (the `.wrl` asset and
// the alien `ATTRIB_TURNS` values) come from the same place as in P2.
//
// Unit attributes/flags for the two deployed units are the SC_ATTRI/SC_UNITS
// values that P3 extracted and verified (`unit_values_tables.in`,
// `access.rs::UNIT_TYPE_PROPERTIES`); the asset itself is not in the repo.
//
// Build & run (Ubuntu 24.04):
//   g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt
//
// Output format v1 (line oriented, '#' lines are comments):
//   crc_bytes <hex payload> <crc16 hex>
//   crc_word <data hex> <state hex> <crc hex>
//   replay_case <name>                 (case from tools/map_vectors/vectors.txt)
//   replay_levels <raw> <fuel> <gold> <derelicts>
//   replay_seed <u32>
//   replay_start <team> <x> <y>
//   replay_alien_costs <alntank> <juggrnt> <alnasgun> <alnplane>
//   replay_deploy <unit_type> <team> <grid_x> <grid_y>
//   replay_ticks <n>
//   replay_checksum <crc hex>          (GameManager_CheckDesync, пакет 45)
//   replay_digest <hex bytes>          (canonical state digest, LE fields)

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

// --- P2-генератор карты/ресурсов: те же функции оракула в этом же TU. ---
#define main max_map_vectors_disabled_main
#include "../map_vectors/main.cpp"
#undef main

namespace {

// ---------------------------------------------------------------------------
// crc16.c — дословная копия
// ---------------------------------------------------------------------------

void crc16(uint16_t* crc, uint8_t c) {
    int32_t i;
    int32_t carry;

    for (i = 0; i < 8; i++) {
        carry = *crc & 0x8000;
        *crc <<= 1;

        if (c & 0x80) {
            (*crc)++;
        }
        c <<= 1;

        if (carry) {
            *crc ^= 0x1021;
        }
    }
}

uint16_t crc16_buf(const uint8_t* data, size_t size) {
    uint16_t crc = 0xFFFF;

    for (size_t i = 0; i < size; ++i) {
        crc16(&crc, data[i]);
    }

    return crc;
}

// game_manager.cpp:2315 — GameManager_GetCrc16
uint16_t get_crc16_word(uint16_t data, uint16_t crc_checksum) {
    for (int32_t i = 0; i < 16; ++i) {
        if (crc_checksum & 0x8000) {
            crc_checksum = static_cast<uint16_t>((crc_checksum << 1) ^ 0x1021u);
        } else {
            crc_checksum <<= 1;
        }
    }

    return static_cast<uint16_t>(crc_checksum ^ data);
}

// ---------------------------------------------------------------------------
// Канонический дайджест (совпадает с max_core::crc::replay_digest_bytes)
// ---------------------------------------------------------------------------

struct DigestWriter {
    std::vector<uint8_t> bytes;

    void u8(uint8_t value) { bytes.push_back(value); }

    void u16(uint16_t value) {
        bytes.push_back(static_cast<uint8_t>(value & 0xFF));
        bytes.push_back(static_cast<uint8_t>(value >> 8));
    }

    void i16(int16_t value) { u16(static_cast<uint16_t>(value)); }

    void u32(uint32_t value) {
        for (int i = 0; i < 4; ++i) {
            bytes.push_back(static_cast<uint8_t>((value >> (8 * i)) & 0xFF));
        }
    }

    void i32(int32_t value) { u32(static_cast<uint32_t>(value)); }

    void raw(const uint8_t* data, size_t size) { bytes.insert(bytes.end(), data, data + size); }
};

// ATTRIB_* порядок: attack, rounds, range, armor, hits, speed, scan, turns,
// ammo, move_and_fire, fuel, storage, attack_radius, agent_adjust.
struct Attributes {
    int16_t attack, rounds, range, armor, hits, speed, scan, turns;
    int16_t ammo, move_and_fire, fuel, storage, attack_radius, agent_adjust;
};

constexpr Attributes kEngineerAttributes{0, 0, 0, 4, 14, 6, 3, 6, 0, 0, 0, 40, 0, 0};
constexpr Attributes kTankAttributes{16, 2, 4, 10, 24, 6, 4, 4, 14, 0, 0, 0, 0, 0};

constexpr uint16_t UNIT_ENGINEER = 61;  // UNIT_START + 0x3D
constexpr uint16_t UNIT_TANK = 51;      // UNIT_START + 0x33

// SC_UNITS flags (access.rs::UNIT_TYPE_PROPERTIES, P3-verified).
constexpr uint32_t kEngineerFlags = 0x0468'4180u;  // MOBILE_SEA|LAND, SENTRY, ELECTRONIC, CONSTRUCTOR, UPGRADABLE
constexpr uint32_t kTankFlags = 0x0662'4100u;      // MOBILE_LAND, SENTRY, TURRET, ELECTRONIC, HAS_FIRING, UPGRADABLE

struct MiniUnit {
    uint16_t unit_type;
    uint16_t team;
    int16_t grid_x;
    int16_t grid_y;
    uint32_t flags;
    Attributes values;
    uint16_t hits;
    uint16_t speed;
    uint8_t shots;
    uint8_t move_and_fire;
    uint8_t ammo;
    int16_t storage;
    uint8_t unit_id;
    uint16_t id;
};

std::vector<MiniUnit> g_units;
uint32_t g_unit_id_counter = PLAYER_TEAM_MAX << 13;
uint16_t g_unit_counters[PLAYER_TEAM_MAX - 1][93];

// UnitsManager_DeployUnit в объёме полей дайджеста (unitinfo.cpp:
// `UnitInfo::UnitInfo` + `UnitsManager_DeployUnit`).
void deploy_unit(uint16_t unit_type, uint16_t team, int16_t grid_x, int16_t grid_y,
                 const Attributes& values, uint32_t flags) {
    MiniUnit unit{};

    ++g_unit_id_counter;
    unit.id = static_cast<uint16_t>((team << 13) | (g_unit_id_counter & 0x1FFF));
    unit.unit_type = unit_type;
    unit.team = team;
    unit.grid_x = grid_x;
    unit.grid_y = grid_y;
    unit.flags = flags;
    unit.values = values;
    unit.hits = static_cast<uint16_t>(values.hits);
    unit.speed = static_cast<uint16_t>(values.speed);
    unit.shots = static_cast<uint8_t>(values.rounds);
    unit.move_and_fire = static_cast<uint8_t>(values.move_and_fire);
    unit.ammo = static_cast<uint8_t>(values.ammo);
    unit.storage = values.storage;
    // `DeployUnit`: счётчик имён типа в команде (стартует с 1).
    unit.unit_id = static_cast<uint8_t>(g_unit_counters[team][unit_type]++);

    g_units.push_back(unit);
}

// Десять тиков: в этом сценарии (две команды-люди, юниты в AWAIT/INIT без
// задач) `UnitsManager_ProcessOrders` не меняет ни одного поля дайджеста —
// меняется только `GameManager_UpdateFlag` (в дайджест не входит). Функция
// фиксирует сам цикл тика, чтобы вектор назывался «10 тиков», а не «deploy».
void tool_tick() {
    static bool update_flag = false;
    update_flag = !update_flag;
}

void write_state_digest(DigestWriter& out, const uint16_t* cargo_map) {
    static const char magic[8] = {'M', 'A', 'X', 'R', 'P', 'L', '1', '\0'};
    out.raw(reinterpret_cast<const uint8_t*>(magic), 8);

    // CRC16 карты поверхности (12544 байта) и карты ресурсов (u16 LE).
    out.u16(crc16_buf(g_surface_map, static_cast<size_t>(MAP_CELLS)));

    std::vector<uint8_t> resources(static_cast<size_t>(MAP_CELLS) * 2);
    for (int i = 0; i < MAP_CELLS; ++i) {
        resources[static_cast<size_t>(i) * 2] = static_cast<uint8_t>(cargo_map[i] & 0xFF);
        resources[static_cast<size_t>(i) * 2 + 1] = static_cast<uint8_t>(cargo_map[i] >> 8);
    }
    out.u16(crc16_buf(resources.data(), resources.size()));

    out.i32(0);  // turn_counter
    out.u8(0x08);  // GameStateType::InGame (state-model §2.12)
    out.u8(static_cast<uint8_t>(PLAYER_TEAM_RED));  // active_turn_team
    out.u8(static_cast<uint8_t>(PLAYER_TEAM_RED));  // player_team
    out.u8(0);  // PlayMode::TurnBased
    out.u32(g_unit_id_counter);

    for (int i = 0; i < 4; ++i) {
        out.u32(g_rng_state[static_cast<size_t>(i)]);
    }

    const uint8_t team_types[5] = {1, 0, 1, 0, 2};  // Player, None, Player, None, Computer

    for (uint16_t team = 0; team < PLAYER_TEAM_MAX; ++team) {
        out.u8(team_types[team]);
        out.u8(0);  // finished_turn
        out.u32(0);  // team_points
        out.u32(0);  // gold

        for (int topic = 0; topic < 8; ++topic) {
            out.u32(0);  // research_level
            out.u32(0);  // turns_to_complete
            out.u32(0);  // allocation
        }

        for (int unit_type = 0; unit_type < 93; ++unit_type) {
            const uint32_t counter = (team < PLAYER_TEAM_MAX - 1)
                                         ? g_unit_counters[team][unit_type]
                                         : 1u;
            out.u32(counter);
        }

        for (int unit_type = 0; unit_type < 93; ++unit_type) {
            out.u32(0);  // casualties
        }
    }

    // Пять списков: MobileLandSea — PushFront, поэтому обратный порядок спавна.
    std::vector<const MiniUnit*> ordered;
    for (auto it = g_units.rbegin(); it != g_units.rend(); ++it) {
        ordered.push_back(&(*it));
    }
    out.u32(static_cast<uint32_t>(ordered.size()));

    for (const MiniUnit* unit : ordered) {
        out.u16(unit->id);
        out.u16(unit->unit_type);
        out.u8(static_cast<uint8_t>(unit->team));
        out.u8(unit->unit_id);
        out.u32(unit->flags);
        out.i16(unit->grid_x);
        out.i16(unit->grid_y);
        out.u8(0);  // orders: AWAIT
        out.u8(0);  // state: INIT
        out.u8(0);  // prior_orders: AWAIT
        out.u8(0);  // prior_state: INIT
        out.u16(unit->hits);
        out.u16(unit->speed);
        out.u16(0);  // group_speed
        out.u8(unit->shots);
        out.u8(unit->move_and_fire);
        out.i16(unit->storage);
        out.i16(0);  // experience
        out.u8(unit->ammo);
        out.u8(0);  // build_time
        out.u8(0);  // total_mining
        out.u8(0);  // raw_mining
        out.u8(0);  // fuel_mining
        out.u8(0);  // gold_mining
        out.u8(0);  // raw_mining_max
        out.u8(0);  // gold_mining_max
        out.u8(0);  // fuel_mining_max
        out.u16(1);  // build_rate (C++ нормализует 0 → 1)
        out.u32(0);  // ai_state_bits
        out.u16(0);  // connectors

        out.i16(unit->values.attack);
        out.i16(unit->values.rounds);
        out.i16(unit->values.range);
        out.i16(unit->values.armor);
        out.i16(unit->values.hits);
        out.i16(unit->values.speed);
        out.i16(unit->values.scan);
        out.i16(unit->values.turns);
        out.i16(unit->values.ammo);
        out.i16(unit->values.move_and_fire);
        out.i16(unit->values.fuel);
        out.i16(unit->values.storage);
        out.i16(unit->values.attack_radius);
        out.i16(unit->values.agent_adjust);

        out.u8(0);  // path: нет
        out.u16(0xFFFF);  // parent_unit
        out.u16(0xFFFF);  // enemy_unit
        out.u32(0);  // build_list
    }

    const uint16_t crc = crc16_buf(out.bytes.data(), out.bytes.size());
    out.u16(crc);
}

// `GameManager_GetUnitListChecksum` (`game_manager.cpp:2327`) для одного списка.
uint16_t unit_list_checksum(const std::vector<const MiniUnit*>& units, uint16_t team,
                            uint16_t crc) {
    for (const MiniUnit* unit : units) {
        if (unit->team != team) {
            continue;
        }

        crc = get_crc16_word(unit->team, crc);
        crc = get_crc16_word(unit->unit_type, crc);
        crc = get_crc16_word(unit->unit_id, crc);
        crc = get_crc16_word(static_cast<uint16_t>(unit->grid_x), crc);
        crc = get_crc16_word(static_cast<uint16_t>(unit->grid_y), crc);
        crc = get_crc16_word(unit->hits, crc);

        if ((unit->flags & 0x200u) == 0) {  // !STATIONARY
            crc = get_crc16_word(unit->speed, crc);
        }

        crc = get_crc16_word(unit->shots, crc);

        // ENGINEER — CARGO_TYPE_RAW (storage учитывается), TANK — NONE.
        if (unit->unit_type == UNIT_ENGINEER) {
            crc = get_crc16_word(static_cast<uint16_t>(unit->storage), crc);
        }

        crc = get_crc16_word(unit->ammo, crc);
    }

    return crc;
}

// `GameManager_CheckDesync` (`game_manager.cpp:2356`): команды во внешнем цикле,
// четыре списка (Particle не входит), старт 0xFFFF.
uint16_t state_checksum() {
    std::vector<const MiniUnit*> mobile_land_sea;  // списки в порядке ProcessOrders
    for (auto it = g_units.rbegin(); it != g_units.rend(); ++it) {
        mobile_land_sea.push_back(&(*it));
    }

    const std::vector<const MiniUnit*> empty;

    uint16_t crc = 0xFFFF;

    const std::vector<const MiniUnit*>* lists[4] = {&empty, &mobile_land_sea, &empty, &empty};
    const uint8_t team_types[5] = {1, 0, 1, 0, 2};

    for (const auto* list : lists) {
        for (uint16_t team = 0; team < PLAYER_TEAM_MAX; ++team) {
            if (team_types[team] == 0 && team != 4) {
                continue;
            }
            crc = unit_list_checksum(*list, team, crc);
        }
    }

    return crc;
}

void print_hex_bytes(const std::vector<uint8_t>& bytes) {
    for (uint8_t byte : bytes) {
        std::printf("%02x", byte);
    }
}

void crc_section() {
    std::printf("# CRC16 пакета 45: crc16.c (init 0xFFFF, poly 0x1021) и\n");
    std::printf("# GameManager_GetCrc16 (пословный шаг).\n");

    const std::vector<std::vector<uint8_t>> payloads = {
        {0x00},
        {0xFF},
        {0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39},  // "123456789"
        {0x01, 0x02, 0x03, 0x04},
        {0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x7F, 0x80, 0xFF},
    };

    for (const auto& payload : payloads) {
        std::printf("crc_bytes ");
        print_hex_bytes(payload);
        std::printf(" %04x\n", crc16_buf(payload.data(), payload.size()));
    }

    const uint16_t word_cases[][2] = {
        {0x0000, 0x0000}, {0x0000, 0xFFFF}, {0x1234, 0xFFFF},
        {0xABCD, 0x29B1}, {0xFFFF, 0x1234}, {0x00FF, 0x8000},
    };

    for (const auto& word_case : word_cases) {
        std::printf("crc_word %04x %04x %04x\n", word_case[0], word_case[1],
                    get_crc16_word(word_case[0], word_case[1]));
    }
}

void replay_section() {
    const CaseConfig& config = CASES[0];  // p00_l111_d0_s0

    std::vector<uint16_t> tile_ids;
    std::vector<uint8_t> pass_table;

    build_world_input(config.planet, tile_ids, pass_table);

    std::memset(g_cargo_map, 0, sizeof(g_cargo_map));
    std::memset(g_bridge_map, 0, sizeof(g_bridge_map));
    g_placements.clear();

    for (int32_t team = 0; team < PLAYER_TEAM_MAX; ++team) {
        g_team_active[team] = false;
        g_starting_position[team] = Point(0, 0);
    }

    for (int32_t i = 0; i < config.team_count; ++i) {
        g_team_active[config.teams[i].team] = true;
        g_starting_position[config.teams[i].team] = Point(config.teams[i].x, config.teams[i].y);
    }

    menu_update_resource_levels(g_settings, config.raw, config.fuel, config.gold, config.derelicts);
    g_alien_costs = config.alien_costs;

    World_BuildSurfaceMap(tile_ids, pass_table);

    Randomizer_SetSeed(config.seed);

    GameManager_PopulateMapWithResources();
    GameManager_PopulateMapWithAlienUnits(g_settings.alien_seperation, g_settings.alien_unit_value);

    // Юниты: красный инженер и синий танк (обе команды — «люди»).
    for (uint16_t team = 0; team < PLAYER_TEAM_MAX - 1; ++team) {
        for (int unit_type = 0; unit_type < 93; ++unit_type) {
            g_unit_counters[team][unit_type] = 1;
        }
    }
    g_units.clear();
    g_unit_id_counter = PLAYER_TEAM_MAX << 13;

    deploy_unit(UNIT_ENGINEER, PLAYER_TEAM_RED, 20, 20, kEngineerAttributes, kEngineerFlags);
    deploy_unit(UNIT_TANK, 2, 24, 20, kTankAttributes, kTankFlags);

    for (int tick = 0; tick < 10; ++tick) {
        tool_tick();
    }

    DigestWriter out;
    write_state_digest(out, g_cargo_map);

    std::printf("# Мини-реплей: карта как в P2 case p00_l111_d0_s0, два юнита, 10 тиков.\n");
    std::printf("replay_case %s\n", config.name);
    std::printf("replay_levels %u %u %u %u\n", static_cast<unsigned>(config.raw),
                static_cast<unsigned>(config.fuel), static_cast<unsigned>(config.gold),
                static_cast<unsigned>(config.derelicts));
    std::printf("replay_seed %u\n", config.seed);

    for (int32_t i = 0; i < config.team_count; ++i) {
        std::printf("replay_start %u %d %d\n", static_cast<unsigned>(config.teams[i].team),
                    static_cast<int>(config.teams[i].x), static_cast<int>(config.teams[i].y));
    }

    std::printf("replay_alien_costs %d %d %d %d\n", config.alien_costs.alntank,
                config.alien_costs.juggrnt, config.alien_costs.alnasgun,
                config.alien_costs.alnplane);
    std::printf("replay_deploy %u %u %d %d\n", static_cast<unsigned>(UNIT_ENGINEER),
                static_cast<unsigned>(PLAYER_TEAM_RED), 20, 20);
    std::printf("replay_deploy %u %u %d %d\n", static_cast<unsigned>(UNIT_TANK), 2u, 24, 20);
    std::printf("replay_ticks 10\n");
    std::printf("replay_checksum %04x\n", state_checksum());
    std::printf("replay_digest ");
    print_hex(out.bytes);
}

}  // namespace

int main() {
    std::printf("# P7 reference vectors for max_core (crc + mini replay).\n");
    std::printf("# Generated by max-rust/tools/sim_vectors/main.cpp (see README).\n");

    crc_section();
    replay_section();

    return 0;
}
