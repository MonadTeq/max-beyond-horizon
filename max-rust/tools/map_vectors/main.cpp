// Reference-vector generator for max_core's map/resource generation (P2).
//
// The program contains a self-contained copy of the exact code paths the C++ oracle
// (max-cpp-port/src) executes while a match is being created:
//
//   * Randomizer (SplitMix64 + Xoshiro128**)              randomizer.cpp + XoshiroCpp.hpp
//   * menu_update_resource_levels()                      menu.cpp:866
//   * ResourceRange / ResourceAllocator                  resource_allocator.cpp
//   * Access_GetApproximateDistance / IsInsideBounds     access.cpp
//   * World::LoadTilesAndMetadata() surface-map loop      world.cpp:382-409
//   * GameManager_IsValidStartingPosition                game_manager.cpp:4196
//   * GameManager_FindValidStartingPosition              game_manager.cpp:4712
//   * GameManager_PopulateMapWithResources               game_manager.cpp:4730
//   * GameManager_FindSpot                               game_manager.cpp:4763
//   * GameManager_SpawnAlienDerelicts                    game_manager.cpp:4805
//   * GameManager_PopulateMapWithAlienUnits              game_manager.cpp:4942
//
// The oracle itself is NOT touched, is NOT built and needs no SDL.
//
// Two inputs cannot be reproduced from the repository and are therefore supplied as
// deterministic stand-ins (the Rust side receives them **as data** from this dump, it
// never regenerates them):
//
//   1. the planet's `.wrl` asset (tile ids + pass table) — the assets are not in the
//      repository, so `build_world_input()` synthesizes a fixed layout per planet id.
//      The pass-table -> SurfaceMap conversion that is applied to it *is* the oracle
//      code (`world.cpp:382-409`), so the Rust port of that loop is still verified
//      bit-for-bit.
//   2. `UnitValues[type].ATTRIB_TURNS` for the six alien derelict types — the unit
//      attribute tables (P3) are not ported yet, so each vector case carries its own
//      values on the `alien_costs` line.
//
// Build & run (Ubuntu 24.04, as in the task statement):
//   g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt
//
// Output format v1 (line oriented, '#' lines are comments):
//   case <name>
//   planet <u8>
//   levels <raw> <fuel> <gold> <derelicts>      (0..2 each, menu levels)
//   seed <u32 decimal>
//   start <team> <x> <y>                        (one line per active team)
//   pass_table <hex bytes>
//   tile_ids <hex u16 LE>
//   surface <hex bytes, MAP_CELLS>
//   alien_costs <alntank> <juggernaut> <alnasgun> <alnplane>
//   resources <hex u16 LE, MAP_CELLS>
//   placement <unit_type> <x> <y>               (one line per UnitsManager_DeployUnit)
//   rng_state <s0> <s1> <s2> <s3>               (decimal, state after generation)

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iterator>
#include <string>
#include <vector>

namespace {

constexpr int32_t MAP_W = 112;
constexpr int32_t MAP_H = 112;
constexpr int32_t MAP_CELLS = MAP_W * MAP_H;

enum : uint8_t {
    SURFACE_TYPE_NONE = 0x0,
    SURFACE_TYPE_LAND = 0x1,
    SURFACE_TYPE_WATER = 0x2,
    SURFACE_TYPE_COAST = 0x4,
    SURFACE_TYPE_AIR = 0x8,
};

enum : uint16_t {
    CARGO_FUEL = 0x20,
    CARGO_GOLD = 0x40,
    CARGO_MATERIALS = 0x80,
    CARGO_MASK = 0x1F,
};

// ---------------------------------------------------------------------------
// Randomizer — copied 1:1 from XoshiroCpp.hpp + randomizer.hpp
// ---------------------------------------------------------------------------

uint64_t splitmix64_next(uint64_t& state) {
    uint64_t z = (state += UINT64_C(0x9e3779b97f4a7c15));
    z = (z ^ (z >> 30)) * UINT64_C(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)) * UINT64_C(0x94d049bb133111eb);
    return z ^ (z >> 31);
}

std::array<uint32_t, 4> g_rng_state{};

void Randomizer_SetSeed(uint32_t seed) {
    uint64_t splitmix_state = static_cast<uint64_t>(seed);

    for (auto& slot : g_rng_state) {
        slot = static_cast<uint32_t>(splitmix64_next(splitmix_state));
    }
}

uint32_t Randomizer_GetNext() {
    auto rotate_left = [](uint32_t x, int s) { return static_cast<uint32_t>((x << s) | (x >> (32 - s))); };
    const uint32_t result = rotate_left(g_rng_state[1] * 5u, 7) * 9u;
    const uint32_t t = g_rng_state[1] << 9;

    g_rng_state[2] ^= g_rng_state[0];
    g_rng_state[3] ^= g_rng_state[1];
    g_rng_state[1] ^= g_rng_state[2];
    g_rng_state[0] ^= g_rng_state[3];
    g_rng_state[2] ^= t;
    g_rng_state[3] = rotate_left(g_rng_state[3], 11);

    return result;
}

// randomizer.hpp: static_cast<T>((static_cast<int64_t>(next) * value) >> 32)
template <typename T>
T Randomizer_Generate(const T value) {
    return static_cast<T>((static_cast<int64_t>(Randomizer_GetNext()) * value) >> 32);
}

// ---------------------------------------------------------------------------
// Point / Rect — point.hpp, rect.c
// ---------------------------------------------------------------------------

struct Point {
    int16_t x;
    int16_t y;

    constexpr Point() : x(0), y(0) {}
    constexpr Point(int16_t x_, int16_t y_) : x(x_), y(y_) {}

    Point& operator+=(const Point& other) {
        x = static_cast<int16_t>(x + other.x);
        y = static_cast<int16_t>(y + other.y);
        return *this;
    }
};

constexpr Point DIRECTION_OFFSETS[8] = {{0, -1}, {1, -1}, {1, 0},  {1, 1},
                                        {0, 1},  {-1, 1}, {-1, 0}, {-1, -1}};

struct Rect {
    int32_t ulx;
    int32_t uly;
    int32_t lrx;
    int32_t lry;
};

void rect_init(Rect* r, int32_t ulx, int32_t uly, int32_t lrx, int32_t lry) {
    r->ulx = ulx;
    r->uly = uly;
    r->lrx = lrx;
    r->lry = lry;
}

// ---------------------------------------------------------------------------
// Map state (globals mirror ResourceManager_CargoMap / World::GetSurfaceMap)
// ---------------------------------------------------------------------------

uint16_t g_cargo_map[MAP_CELLS];
uint8_t g_surface_map[MAP_CELLS];
bool g_bridge_map[MAP_CELLS];

// ---------------------------------------------------------------------------
// Access helpers — access.cpp
// ---------------------------------------------------------------------------

int32_t Access_GetApproximateDistance(int32_t distance_x, int32_t distance_y) {
    int32_t result;

    distance_x = labs(distance_x);
    distance_y = labs(distance_y);

    if (distance_x >= distance_y) {
        result = 2 * distance_x + distance_y;
    } else {
        result = 2 * distance_y + distance_x;
    }

    return result;
}

uint8_t Access_GetSurfaceType(int32_t grid_x, int32_t grid_y) {
    if (grid_x < 0 || grid_y < 0 || grid_x >= MAP_W || grid_y >= MAP_H) {
        return SURFACE_TYPE_NONE;
    }

    return g_surface_map[grid_y * MAP_W + grid_x];
}

// Access_GetModifiedSurfaceType(..., mode = false): a bridge turns water/coast into
// land. Other unit kinds are irrelevant for the generation phase.
uint8_t Access_GetModifiedSurfaceType(int32_t grid_x, int32_t grid_y) {
    uint8_t surface_type = Access_GetSurfaceType(grid_x, grid_y);

    if (surface_type == SURFACE_TYPE_WATER || surface_type == SURFACE_TYPE_COAST) {
        if (g_bridge_map[grid_y * MAP_W + grid_x]) {
            surface_type = SURFACE_TYPE_LAND;
        }
    }

    return surface_type;
}

bool Access_IsInsideBounds(Rect* bounds, Point* point) {
    return point->x >= bounds->ulx && point->x < bounds->lrx && point->y >= bounds->uly && point->y < bounds->lry;
}

// ---------------------------------------------------------------------------
// Settings — settings.cpp defaults + menu_update_resource_levels (menu.cpp:866)
// ---------------------------------------------------------------------------

struct Settings {
    int32_t raw_normal_low;
    int32_t raw_normal_high;
    int32_t raw_concentrate_low;
    int32_t raw_concentrate_high;
    int32_t raw_concentrate_seperation;
    int32_t raw_concentrate_diffusion;

    int32_t fuel_normal_low;
    int32_t fuel_normal_high;
    int32_t fuel_concentrate_low;
    int32_t fuel_concentrate_high;
    int32_t fuel_concentrate_seperation;
    int32_t fuel_concentrate_diffusion;

    int32_t gold_normal_low;
    int32_t gold_normal_high;
    int32_t gold_concentrate_low;
    int32_t gold_concentrate_high;
    int32_t gold_concentrate_seperation;
    int32_t gold_concentrate_diffusion;

    int32_t mixed_resource_seperation;
    int32_t min_resources;
    int32_t max_resources;

    int32_t alien_seperation;
    int32_t alien_unit_value;
};

// Copied 1:1 from menu_update_resource_levels(); only the settings object access is
// replaced by direct field stores.
void menu_update_resource_levels(Settings& settings, int32_t resource_raw, int32_t resource_fuel,
                                 int32_t resource_gold, int32_t resource_derelicts) {
    int32_t resource_min;
    int32_t resource_max;

    resource_min = 1;
    resource_max = 15;

    switch (resource_raw) {
        case 0: {
            settings.raw_normal_low = 0;
            settings.raw_normal_high = 3;
            settings.raw_concentrate_low = 8;
            settings.raw_concentrate_high = 12;
            settings.raw_concentrate_seperation = 28;
            settings.raw_concentrate_diffusion = 6;

            resource_min = 4;
            resource_max = 16;
        } break;

        case 1: {
            settings.raw_normal_low = 0;
            settings.raw_normal_high = 5;
            settings.raw_concentrate_low = 13;
            settings.raw_concentrate_high = 16;
            settings.raw_concentrate_seperation = 23;
            settings.raw_concentrate_diffusion = 5;

            resource_min = 6;
            resource_max = 17;
        } break;

        case 2: {
            settings.raw_normal_low = 1;
            settings.raw_normal_high = 5;
            settings.raw_concentrate_low = 16;
            settings.raw_concentrate_high = 16;
            settings.raw_concentrate_seperation = 19;
            settings.raw_concentrate_diffusion = 4;

            resource_min = 7;
            resource_max = 18;
        } break;

        default: {
        } break;
    }

    switch (resource_fuel) {
        case 0: {
            settings.fuel_normal_low = 1;
            settings.fuel_normal_high = 2;
            settings.fuel_concentrate_low = 8;
            settings.fuel_concentrate_high = 12;
            settings.fuel_concentrate_seperation = 29;
            settings.fuel_concentrate_diffusion = 6;

            resource_min += 2;
            resource_max += 1;
        } break;

        case 1: {
            settings.fuel_normal_low = 2;
            settings.fuel_normal_high = 3;
            settings.fuel_concentrate_low = 12;
            settings.fuel_concentrate_high = 16;
            settings.fuel_concentrate_seperation = 24;
            settings.fuel_concentrate_diffusion = 5;

            resource_min += 3;
            resource_max += 2;
        } break;

        case 2: {
            settings.fuel_normal_low = 2;
            settings.fuel_normal_high = 4;
            settings.fuel_concentrate_low = 16;
            settings.fuel_concentrate_high = 16;
            settings.fuel_concentrate_seperation = 20;
            settings.fuel_concentrate_diffusion = 4;

            resource_min += 4;
            resource_max += 3;
        } break;

        default: {
        } break;
    }

    switch (resource_raw + resource_fuel) {
        case 0: {
            settings.mixed_resource_seperation = 49;
        } break;

        case 1: {
            settings.mixed_resource_seperation = 44;
        } break;

        case 2: {
            settings.mixed_resource_seperation = 40;
        } break;

        case 3: {
            settings.mixed_resource_seperation = 36;
        } break;

        case 4: {
            settings.mixed_resource_seperation = 33;
        } break;

        default: {
        } break;
    }

    switch (resource_gold) {
        case 0: {
            settings.gold_normal_low = 0;
            settings.gold_normal_high = 0;
            settings.gold_concentrate_low = 5;
            settings.gold_concentrate_high = 9;
            settings.gold_concentrate_seperation = 39;
            settings.gold_concentrate_diffusion = 6;

            resource_min += 0;
            resource_max += 0;
        } break;

        case 1: {
            settings.gold_normal_low = 0;
            settings.gold_normal_high = 0;
            settings.gold_concentrate_low = 8;
            settings.gold_concentrate_high = 12;
            settings.gold_concentrate_seperation = 32;
            settings.gold_concentrate_diffusion = 5;

            resource_min += 0;
            resource_max += 1;
        } break;

        case 2: {
            settings.gold_normal_low = 0;
            settings.gold_normal_high = 1;
            settings.gold_concentrate_low = 12;
            settings.gold_concentrate_high = 16;
            settings.gold_concentrate_seperation = 26;
            settings.gold_concentrate_diffusion = 4;

            resource_min += 1;
            resource_max += 2;
        } break;

        default: {
        } break;
    }

    settings.min_resources = resource_min;
    settings.max_resources = resource_max;

    switch (resource_derelicts) {
        case 0: {
            settings.alien_seperation = 0;
            settings.alien_unit_value = 0;
        } break;

        case 1: {
            settings.alien_seperation = 65;
            settings.alien_unit_value = 20;
        } break;

        case 2: {
            settings.alien_seperation = 50;
            settings.alien_unit_value = 30;
        } break;

        default: {
        } break;
    }
}

Settings g_settings{};

// ---------------------------------------------------------------------------
// ResourceAllocator — copied 1:1 from resource_allocator.cpp
// ---------------------------------------------------------------------------

struct ResourceRange {
    int32_t min;
    int32_t max;

    int32_t GetValue() const {
        return ((Randomizer_Generate(max - min + 1) + min) + (Randomizer_Generate(max - min + 1) + min) + 1) / 2;
    }
};

class ResourceAllocator {
private:
    uint16_t material_type;
    Point m_point;
    ResourceRange normal;
    ResourceRange concentrate;
    int32_t concentrate_seperation;
    int32_t concentrate_variance;
    int32_t concentrate_diffusion;
    int32_t max_resources;
    int32_t mixed_resource_seperation;
    int32_t mixed_resource_seperation_variance;

public:
    explicit ResourceAllocator(uint16_t material_type_) {
        const auto& settings = g_settings;

        material_type = material_type_;
        max_resources = settings.max_resources;
        mixed_resource_seperation = settings.mixed_resource_seperation;
        mixed_resource_seperation_variance = mixed_resource_seperation / 5;

        switch (material_type) {
            case CARGO_MATERIALS: {
                m_point.x = 0;
                m_point.y = 0;
                normal = ResourceRange{settings.raw_normal_low, settings.raw_normal_high};
                concentrate = ResourceRange{settings.raw_concentrate_low, settings.raw_concentrate_high};
                concentrate_seperation = settings.raw_concentrate_seperation;
                concentrate_variance = concentrate_seperation / 5;
                concentrate_diffusion = settings.raw_concentrate_diffusion;
            } break;

            case CARGO_GOLD: {
                m_point.x = 1;
                m_point.y = 0;
                normal = ResourceRange{settings.gold_normal_low, settings.gold_normal_high};
                concentrate = ResourceRange{settings.gold_concentrate_low, settings.gold_concentrate_high};
                concentrate_seperation = settings.gold_concentrate_seperation;
                concentrate_variance = concentrate_seperation / 5;
                concentrate_diffusion = settings.gold_concentrate_diffusion;
            } break;

            case CARGO_FUEL: {
                m_point.x = 1;
                m_point.y = 1;
                normal = ResourceRange{settings.fuel_normal_low, settings.fuel_normal_high};
                concentrate = ResourceRange{settings.fuel_concentrate_low, settings.fuel_concentrate_high};
                concentrate_seperation = settings.fuel_concentrate_seperation;
                concentrate_variance = concentrate_seperation / 5;
                concentrate_diffusion = settings.fuel_concentrate_diffusion;
            } break;

            default: {
                m_point.x = 0;
                m_point.y = 0;
                normal = ResourceRange{0, 0};
                concentrate = ResourceRange{0, 0};
                concentrate_seperation = 1;
                concentrate_variance = 0;
                concentrate_diffusion = 1;
            } break;
        }
    }

    static int32_t GetZoneResoureLevel(int32_t grid_x, int32_t grid_y) {
        int32_t result = 0;
        Rect bounds;

        rect_init(&bounds, grid_x, grid_y, grid_x + 2, grid_y + 2);

        if (bounds.ulx < 0) {
            bounds.ulx = 0;
        }

        if (bounds.uly < 0) {
            bounds.uly = 0;
        }

        if (MAP_W < bounds.lrx) {
            bounds.lrx = MAP_W;
        }

        if (MAP_H < bounds.lry) {
            bounds.lry = MAP_H;
        }

        for (int32_t i = bounds.ulx; i < bounds.lrx; ++i) {
            for (int32_t j = bounds.uly; j < bounds.lry; ++j) {
                result += g_cargo_map[MAP_W * j + i] & 0x1F;
            }
        }

        return result;
    }

    static int32_t OptimizeResources(int32_t grid_x, int32_t grid_y, int32_t level_min, int32_t level_max) {
        int32_t zone_resource_level = GetZoneResoureLevel(grid_x, grid_y);

        if ((zone_resource_level + level_min) > level_max) {
            level_min = level_max - zone_resource_level;
        }

        if (level_min < 0) {
            level_min = 0;
        }

        return level_min;
    }

    void Optimize(Point point, int32_t resource_level, int32_t resource_value) {
        Rect bounds;
        int32_t map_resource_amount;
        int32_t material_value;
        int32_t distance_factor;
        int32_t grid_x;
        int32_t grid_y;

        point.x = ((((point.x + 1) - m_point.x) / 2) * 2) + m_point.x;
        point.y = ((((point.y + 1) - m_point.y) / 2) * 2) + m_point.y;

        bounds.lry = point.y - 4;

        if (bounds.lry < 0) {
            bounds.lry = 0;
        }

        bounds.lry = ((bounds.lry / 2) * 2) + m_point.y;

        bounds.lrx = point.x - 4;

        if (bounds.lrx < 0) {
            bounds.lrx = 0;
        }

        bounds.lrx = ((bounds.lrx / 2) * 2) + m_point.x;

        bounds.uly = point.y + 5;

        if (bounds.uly > MAP_H) {
            bounds.uly = MAP_H;
        }

        bounds.ulx = point.x + 5;

        if (bounds.ulx > MAP_W) {
            bounds.ulx = MAP_W;
        }

        const int32_t map_index = point.y * MAP_W + point.x;

        // The centre of a deposit is not clamped into the map, so this index may leave the cargo map. The
        // deposit loop below cannot, as bounds.lrx and bounds.lry never fall below m_point and the loops stay
        // empty once the centre is past the far edge.
        if (map_index >= 0 && map_index < MAP_W * MAP_H) {
            map_resource_amount = g_cargo_map[map_index] & 0x1F;

        } else {
            map_resource_amount = 0;
        }

        if (point.x > 0) {
            if (point.y > 0) {
                resource_level = OptimizeResources(point.x - 1, point.y - 1, resource_level,
                                                   resource_value + map_resource_amount);
            }

            if (point.y < MAP_H - 1) {
                resource_level =
                    OptimizeResources(point.x - 1, point.y, resource_level, resource_value + map_resource_amount);
            }
        }

        if (point.x < MAP_W - 1) {
            if (point.y > 0) {
                resource_level =
                    OptimizeResources(point.x, point.y - 1, resource_level, resource_value + map_resource_amount);
            }

            if (point.y < MAP_H - 1) {
                resource_level =
                    OptimizeResources(point.x, point.y, resource_level, resource_value + map_resource_amount);
            }
        }

        resource_level *= 100;

        for (grid_x = bounds.lrx; grid_x < bounds.ulx; grid_x += 2) {
            for (grid_y = bounds.lry; grid_y < bounds.uly; grid_y += 2) {
                distance_factor =
                    ((Access_GetApproximateDistance(point.x - grid_x, point.y - grid_y) * 10) / concentrate_diffusion) +
                    10;
                material_value = resource_level / (distance_factor * distance_factor);

                if (material_value > (g_cargo_map[grid_y * MAP_W + grid_x] & 0x1F)) {
                    g_cargo_map[grid_y * MAP_W + grid_x] = material_type + material_value;
                }
            }
        }
    }

    void PopulateCargoMap() const {
        for (int32_t i = m_point.x; i < MAP_W; i += 2) {
            for (int32_t j = m_point.y; j < MAP_H; j += 2) {
                g_cargo_map[MAP_W * j + i] = material_type + normal.GetValue();
            }
        }
    }

    void ConcentrateResources() {
        Point point1;
        Point point2;
        bool flag = false;

        point1.x = Randomizer_Generate((concentrate_seperation * 4) / 5 + 1);
        point1.y = Randomizer_Generate(concentrate_seperation / 2 + 1);

        for (int32_t i = point1.x; i < MAP_W; i += (concentrate_seperation * 4) / 5) {
            for (int32_t j = flag ? (concentrate_seperation / 2 + point1.y) : point1.y; j < MAP_H;
                 j += concentrate_seperation) {
                point2.x = Randomizer_Generate(concentrate_variance * 2 + 1) - concentrate_variance + i;
                point2.y = Randomizer_Generate(concentrate_variance * 2 + 1) - concentrate_variance + j;

                Optimize(point2, concentrate.GetValue(), max_resources);
            }

            flag = !flag;
        }
    }

    static void SettleMinimumResourceLevels(int32_t min_level) {
        for (int32_t i = 0; i < MAP_W; ++i) {
            for (int32_t j = 0; j < MAP_H; ++j) {
                if (GetZoneResoureLevel(i - 1, j - 1) < min_level && GetZoneResoureLevel(i - 1, j) < min_level &&
                    GetZoneResoureLevel(i, j - 1) < min_level && GetZoneResoureLevel(i, j) < min_level) {
                    g_cargo_map[MAP_W * j + i] = CARGO_MATERIALS;
                }
            }
        }
    }

    void SeparateResources(ResourceAllocator* allocator) {
        Point point1;
        Point point2;
        bool flag = false;

        point1.x = Randomizer_Generate(((mixed_resource_seperation * 4) / 5) + 1);
        point1.y = Randomizer_Generate(mixed_resource_seperation / 2 + 1);

        for (int32_t i = point1.x; i < MAP_W; i += (mixed_resource_seperation * 4) / 5) {
            for (int32_t j = flag ? (mixed_resource_seperation / 2 + point1.y) : point1.y; j < MAP_H;
                 j += mixed_resource_seperation) {
                point2.x = Randomizer_Generate(mixed_resource_seperation_variance * 2 + 1) -
                           mixed_resource_seperation_variance + i;
                point2.y = Randomizer_Generate(mixed_resource_seperation_variance * 2 + 1) -
                           mixed_resource_seperation_variance + j;

                Optimize(point2, Randomizer_Generate(5) + 8, max_resources);
                allocator->Optimize(point2, Randomizer_Generate(5) + 8, max_resources);
            }

            flag = !flag;
        }
    }
};

// ---------------------------------------------------------------------------
// Teams and starting positions — game_manager.cpp
// ---------------------------------------------------------------------------

constexpr uint16_t PLAYER_TEAM_RED = 0;
constexpr uint16_t PLAYER_TEAM_MAX = 5;

bool g_team_active[PLAYER_TEAM_MAX] = {};
Point g_starting_position[PLAYER_TEAM_MAX] = {};

bool GameManager_IsValidStartingPosition(int32_t grid_x, int32_t grid_y) {
    Point point;

    if (grid_x < 1 || grid_y < 1 || (grid_x + 3) > MAP_W || (grid_y + 3) > MAP_H) {
        return false;
    }

    for (point.x = grid_x - 1; point.x < grid_x + 3; ++point.x) {
        for (point.y = grid_y - 1; point.y < grid_y + 3; ++point.y) {
            if (Access_GetModifiedSurfaceType(point.x, point.y) != SURFACE_TYPE_LAND) {
                return false;
            }
        }
    }

    return true;
}

void GameManager_FindValidStartingPosition(Point* position) {
    if (!GameManager_IsValidStartingPosition(position->x, position->y)) {
        for (int32_t range = 2;; range += 2) {
            --position->x;
            ++position->y;

            for (int32_t direction = 0; direction < 8; direction += 2) {
                for (int32_t i = 0; i < range; ++i) {
                    *position += DIRECTION_OFFSETS[direction];

                    if (GameManager_IsValidStartingPosition(position->x, position->y)) {
                        return;
                    }
                }
            }
        }
    }
}

void GameManager_PopulateMapWithResources() {
    ResourceAllocator allocator_materials(CARGO_MATERIALS);
    ResourceAllocator allocator_fuel(CARGO_FUEL);
    ResourceAllocator allocator_gold(CARGO_GOLD);

    const int32_t max_resources{g_settings.max_resources};
    const int32_t minimum_fuel{(max_resources * 8 + 10) / 20};
    const int32_t minimum_materials{(max_resources * 12 + 10) / 20};

    // Remote_RngSeed seeding happens in run_case() right before this call, exactly as
    // GameManager_PopulateMapWithResources() seeds the stream at its start.

    allocator_materials.PopulateCargoMap();
    allocator_fuel.PopulateCargoMap();
    allocator_gold.PopulateCargoMap();

    for (int32_t team = PLAYER_TEAM_RED; team < PLAYER_TEAM_MAX - 1; ++team) {
        if (g_team_active[team]) {
            GameManager_FindValidStartingPosition(&g_starting_position[team]);
            allocator_materials.Optimize(g_starting_position[team], minimum_materials, 100);
            allocator_fuel.Optimize(g_starting_position[team], minimum_fuel, 100);
        }
    }

    allocator_materials.SeparateResources(&allocator_fuel);

    allocator_materials.ConcentrateResources();
    allocator_fuel.ConcentrateResources();
    allocator_gold.ConcentrateResources();

    ResourceAllocator::SettleMinimumResourceLevels(g_settings.min_resources);
}

// ---------------------------------------------------------------------------
// Alien derelicts — game_manager.cpp
// ---------------------------------------------------------------------------

constexpr uint16_t LRGSLAB = 0x11;
constexpr uint16_t BRIDGE = 0x27;
constexpr uint16_t SHIELDGN = 0x4;
constexpr uint16_t SUPRTPLT = 0x1A;
constexpr uint16_t RECCENTR = 0x1F;
constexpr uint16_t JUGGRNT = 0x50;
constexpr uint16_t ALNTANK = 0x51;
constexpr uint16_t ALNASGUN = 0x52;
constexpr uint16_t ALNPLANE = 0x53;

const uint16_t GameManager_AlienBuildings[] = {SHIELDGN, SUPRTPLT, RECCENTR};
const uint16_t GameManager_AlienUnits[] = {ALNTANK, ALNASGUN, ALNPLANE};

struct AlienCosts {
    int32_t alntank;
    int32_t juggrnt;
    int32_t alnasgun;
    int32_t alnplane;
};

AlienCosts g_alien_costs{};

// UnitValues[type].GetAttribute(ATTRIB_TURNS) for the alien team (current == base).
int32_t AlienTurns(uint16_t unit_type) {
    switch (unit_type) {
        case ALNTANK: {
            return g_alien_costs.alntank;
        }

        case JUGGRNT: {
            return g_alien_costs.juggrnt;
        }

        case ALNASGUN: {
            return g_alien_costs.alnasgun;
        }

        case ALNPLANE: {
            return g_alien_costs.alnplane;
        }

        default: {
            std::fprintf(stderr, "unexpected alien unit type %u\n", static_cast<unsigned>(unit_type));
            std::exit(1);
        }
    }
}

struct Placement {
    uint16_t unit_type;
    int16_t x;
    int16_t y;
};

std::vector<Placement> g_placements;

// Stand-in for UnitsManager_DeployUnit(): records the placement; bridges additionally
// change the modified surface type (access.cpp:950).
void Tool_DeployUnit(uint16_t unit_type, Point position) {
    g_placements.push_back(Placement{unit_type, position.x, position.y});

    if (unit_type == BRIDGE && position.x >= 0 && position.y >= 0 && position.x < MAP_W && position.y < MAP_H) {
        g_bridge_map[position.y * MAP_W + position.x] = true;
    }
}

void GameManager_FindSpot(Point* point) {
    for (int32_t range = 2;; range += 2) {
        --point->x;
        ++point->y;

        for (int32_t direction = 0; direction < 8; direction += 2) {
            for (int32_t i = 0; i < range; ++i) {
                *point += DIRECTION_OFFSETS[direction];

                if (point->x >= 1 && point->y >= 1 && point->x + 3 < MAP_W && point->y + 3 < MAP_H) {
                    bool is_found;

                    is_found = true;

                    for (int32_t team = PLAYER_TEAM_RED; team < PLAYER_TEAM_MAX - 1; ++team) {
                        if (g_team_active[team]) {
                            const int32_t distance_x =
                                static_cast<int32_t>(g_starting_position[team].x) - point->x;
                            const int32_t distance_y =
                                static_cast<int32_t>(g_starting_position[team].y) - point->y;

                            if ((distance_x * distance_x + distance_y * distance_y) < 256) {
                                is_found = false;
                            }
                        }
                    }

                    for (int32_t grid_x = point->x; grid_x < point->x + 2 && is_found; ++grid_x) {
                        for (int32_t grid_y = point->y; grid_y < point->y + 2 && is_found; ++grid_y) {
                            if (Access_GetModifiedSurfaceType(grid_x, grid_y) == SURFACE_TYPE_AIR) {
                                is_found = false;
                            }
                        }
                    }

                    if (is_found) {
                        return;
                    }
                }
            }
        }
    }
}

void GameManager_SpawnAlienDerelicts(Point point, int32_t alien_unit_value) {
    Rect bounds;
    Point position;
    uint8_t surface_type;

    rect_init(&bounds, 0, 0, MAP_W, MAP_H);

    GameManager_FindSpot(&point);

    Tool_DeployUnit(LRGSLAB, point);
    Tool_DeployUnit(GameManager_AlienBuildings[Randomizer_Generate(std::size(GameManager_AlienBuildings))], point);

    position.x = point.x - 1;
    position.y = point.y + 2;

    for (int32_t direction = 0; direction < 8; direction += 2) {
        for (int32_t step_count = 0; step_count < 3; ++step_count) {
            position += DIRECTION_OFFSETS[direction];

            if (Access_IsInsideBounds(&bounds, &position)) {
                surface_type = Access_GetSurfaceType(position.x, position.y);

                if (surface_type == SURFACE_TYPE_WATER || surface_type == SURFACE_TYPE_COAST) {
                    Tool_DeployUnit(BRIDGE, position);
                }
            }
        }
    }

    {
        std::vector<Point> land_tiles;
        std::vector<Point> water_tiles;
        int32_t cost;
        int32_t value;
        uint32_t index;

        cost = AlienTurns(ALNTANK);

        for (int32_t offset = 2; alien_unit_value >= cost && offset > 0; --offset) {
            land_tiles.clear();
            water_tiles.clear();

            position.x = point.x - offset;
            position.y = point.y + offset + 1;

            for (int32_t direction = 0; direction < 8; direction += 2) {
                for (int32_t step_count = 0; step_count < 2 * offset + 1; ++step_count) {
                    position += DIRECTION_OFFSETS[direction];

                    if (Access_IsInsideBounds(&bounds, &position)) {
                        surface_type = Access_GetModifiedSurfaceType(position.x, position.y);

                        if (surface_type == SURFACE_TYPE_WATER) {
                            water_tiles.push_back(position);

                        } else if (surface_type == SURFACE_TYPE_LAND) {
                            land_tiles.push_back(position);
                        }
                    }
                }
            }

            if (alien_unit_value < AlienTurns(JUGGRNT)) {
                value = 1;

            } else {
                value = 0;
            }

            if (alien_unit_value < value) {
                water_tiles.clear();
            }

            while (alien_unit_value >= cost && (land_tiles.size() + water_tiles.size()) > 0) {
                uint16_t deployed_unit;

                if ((Randomizer_Generate(static_cast<uint32_t>(land_tiles.size() + water_tiles.size()))) + 1 <=
                    water_tiles.size()) {
                    index = Randomizer_Generate(static_cast<uint32_t>(water_tiles.size()));

                    position = water_tiles[index];
                    water_tiles.erase(water_tiles.begin() + index);

                    Tool_DeployUnit(JUGGRNT, position);
                    deployed_unit = JUGGRNT;

                } else {
                    int32_t alien_unit_index;

                    index = Randomizer_Generate(static_cast<uint32_t>(land_tiles.size()));

                    position = land_tiles[index];
                    land_tiles.erase(land_tiles.begin() + index);

                    do {
                        alien_unit_index = static_cast<int32_t>(Randomizer_Generate(std::size(GameManager_AlienUnits)));
                    } while (AlienTurns(GameManager_AlienUnits[alien_unit_index]) > alien_unit_value);

                    deployed_unit = GameManager_AlienUnits[alien_unit_index];

                    Tool_DeployUnit(deployed_unit, position);
                }

                alien_unit_value -= AlienTurns(deployed_unit);

                if (alien_unit_value < AlienTurns(JUGGRNT)) {
                    value = 1;

                } else {
                    value = 0;
                }

                if (alien_unit_value < value) {
                    water_tiles.clear();
                }
            }
        }
    }
}

void GameManager_PopulateMapWithAlienUnits(int32_t alien_seperation, int32_t alien_unit_value) {
    if (alien_unit_value > 0) {
        Point point1;
        Point point2;
        bool flag = false;

        point1.x = Randomizer_Generate((alien_seperation * 4) / 5 + 1);
        point1.y = Randomizer_Generate(alien_seperation / 2 + 1);

        for (int32_t i = point1.x; i < MAP_W; i += (alien_seperation * 4) / 5) {
            for (int32_t j = flag ? (alien_seperation / 2 + point1.y) : point1.y; j < MAP_H; j += alien_seperation) {
                point2.x = Randomizer_Generate(((alien_seperation / 5) - ((-alien_seperation) / 5)) + 1) +
                           ((-alien_seperation) / 5) + i;
                point2.y = Randomizer_Generate(((alien_seperation / 5) - ((-alien_seperation) / 5)) + 1) +
                           ((-alien_seperation) / 5) + j;

                GameManager_SpawnAlienDerelicts(point2, alien_unit_value);
            }

            flag = !flag;
        }
    }
}

// ---------------------------------------------------------------------------
// World surface map — world.cpp:382-409 (Load pass table and build surface map)
// ---------------------------------------------------------------------------

void World_BuildSurfaceMap(const std::vector<uint16_t>& tile_ids, const std::vector<uint8_t>& pass_table) {
    static const uint8_t PassData[] = {SURFACE_TYPE_LAND, SURFACE_TYPE_WATER, SURFACE_TYPE_COAST, SURFACE_TYPE_AIR};
    std::vector<uint8_t> table(pass_table);

    for (size_t i = 0; i < table.size(); ++i) {
        table[i] = PassData[table[i]];
    }

    for (int32_t i = 0; i < MAP_CELLS; ++i) {
        g_surface_map[i] = (i < static_cast<int32_t>(tile_ids.size()) && tile_ids[i] < table.size())
                               ? table[tile_ids[i]]
                               : static_cast<uint8_t>(SURFACE_TYPE_NONE);
    }
}

// ---------------------------------------------------------------------------
// Deterministic stand-in for the `.wrl` asset (see the file header)
// ---------------------------------------------------------------------------

void build_world_input(uint8_t planet, std::vector<uint16_t>& tile_ids, std::vector<uint8_t>& pass_table) {
    const int32_t tile_count = 16;

    pass_table.resize(static_cast<size_t>(tile_count));

    for (int32_t i = 0; i < tile_count; ++i) {
        // Values are indices into PassData {LAND, WATER, COAST, AIR}.
        pass_table[i] = static_cast<uint8_t>(i % 4);
    }

    tile_ids.resize(static_cast<size_t>(MAP_CELLS));

    for (int32_t y = 0; y < MAP_H; ++y) {
        for (int32_t x = 0; x < MAP_W; ++x) {
            uint32_t h = static_cast<uint32_t>(x) * 0x9E3779B1u ^ static_cast<uint32_t>(y) * 0x85EBCA77u ^
                         static_cast<uint32_t>(planet) * 0xC2B2AE3Du;

            h ^= h >> 15;
            h *= 0x2545F491u;
            h ^= h >> 13;

            uint16_t tile;

            if (x >= 12 && x < 100 && y >= 12 && y < 100) {
                // Mostly land (tile 0 and tile 4 both map to pass index 0 = LAND),
                // sprinkled with coast so the alien spawn logic sees mixed terrain.
                tile = static_cast<uint16_t>((h % 5) == 0 ? 8 : 0);
            } else {
                tile = static_cast<uint16_t>(1 + h % 3);
            }

            tile_ids[static_cast<size_t>(y * MAP_W + x)] = tile;
        }
    }
}

// ---------------------------------------------------------------------------
// Vector cases
// ---------------------------------------------------------------------------

struct CaseTeam {
    uint8_t team;
    int16_t x;
    int16_t y;
};

struct CaseConfig {
    const char* name;
    uint8_t planet;
    uint8_t raw;
    uint8_t fuel;
    uint8_t gold;
    uint8_t derelicts;
    uint32_t seed;
    CaseTeam teams[4];
    int32_t team_count;
    AlienCosts alien_costs;
};

const CaseConfig CASES[] = {
    {"p00_l111_d0_s0", 0, 1, 1, 1, 0, 0u, {{0, 20, 20}, {2, 70, 70}, {0, 0, 0}, {0, 0, 0}}, 2, {4, 8, 5, 6}},
    {"p03_l000_d1_s1", 3, 0, 0, 0, 1, 1u, {{0, 24, 30}, {1, 60, 30}, {0, 0, 0}, {0, 0, 0}}, 2, {5, 10, 6, 7}},
    {"p07_l222_d1_sdeadbeef",
     7,
     2,
     2,
     2,
     1,
     0xDEADBEEFu,
     {{1, 30, 24}, {3, 80, 80}, {0, 0, 0}, {0, 0, 0}},
     2,
     {3, 9, 4, 5}},
    {"p12_l201_d2_s1234567890",
     12,
     2,
     0,
     1,
     2,
     1'234'567'890u,
     {{0, 18, 18}, {1, 55, 18}, {2, 18, 55}, {3, 90, 90}},
     4,
     {6, 12, 7, 8}},
    {"p17_l022_d2_s42", 17, 0, 2, 2, 2, 42u, {{0, 40, 60}, {2, 64, 40}, {0, 0, 0}, {0, 0, 0}}, 2, {2, 6, 3, 4}},
    {"p23_l120_d0_s7fffffff",
     23,
     1,
     2,
     0,
     0,
     0x7FFFFFFFu,
     {{1, 22, 76}, {3, 76, 22}, {0, 0, 0}, {0, 0, 0}},
     2,
     {7, 14, 8, 9}},
};

void print_hex(const std::vector<uint8_t>& bytes) {
    for (uint8_t byte : bytes) {
        std::printf("%02x", byte);
    }

    std::printf("\n");
}

void print_hex_u16(const uint16_t* words, size_t count) {
    for (size_t i = 0; i < count; ++i) {
        std::printf("%02x%02x", static_cast<unsigned>(words[i] & 0xFFu), static_cast<unsigned>(words[i] >> 8));
    }

    std::printf("\n");
}

void run_case(const CaseConfig& config) {
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

    // GameManager_InitGame() order. Remote_RngSeed is applied at the start of
    // GameManager_PopulateMapWithResources().
    Randomizer_SetSeed(config.seed);

    GameManager_PopulateMapWithResources();

    // The alien phase never writes the cargo map, but the vector is the resource map
    // right after the resource phase (that is what the port returns).
    uint16_t resources[MAP_CELLS];

    std::memcpy(resources, g_cargo_map, sizeof(resources));

    GameManager_PopulateMapWithAlienUnits(g_settings.alien_seperation, g_settings.alien_unit_value);

    std::vector<uint8_t> surface_bytes(static_cast<size_t>(MAP_CELLS));

    std::memcpy(surface_bytes.data(), g_surface_map, surface_bytes.size());

    std::printf("case %s\n", config.name);
    std::printf("planet %u\n", static_cast<unsigned>(config.planet));
    std::printf("levels %u %u %u %u\n", static_cast<unsigned>(config.raw), static_cast<unsigned>(config.fuel),
                static_cast<unsigned>(config.gold), static_cast<unsigned>(config.derelicts));
    std::printf("seed %u\n", config.seed);

    for (int32_t i = 0; i < config.team_count; ++i) {
        std::printf("start %u %d %d\n", static_cast<unsigned>(config.teams[i].team),
                    static_cast<int>(config.teams[i].x), static_cast<int>(config.teams[i].y));
    }

    std::printf("pass_table ");
    print_hex(pass_table);

    std::printf("tile_ids ");
    print_hex_u16(tile_ids.data(), tile_ids.size());

    std::printf("surface ");
    print_hex(surface_bytes);

    std::printf("alien_costs %d %d %d %d\n", config.alien_costs.alntank, config.alien_costs.juggrnt,
                config.alien_costs.alnasgun, config.alien_costs.alnplane);

    std::printf("resources ");
    print_hex_u16(resources, MAP_CELLS);

    for (const auto& placement : g_placements) {
        std::printf("placement %u %d %d\n", static_cast<unsigned>(placement.unit_type),
                    static_cast<int>(placement.x), static_cast<int>(placement.y));
    }

    std::printf("rng_state %u %u %u %u\n", g_rng_state[0], g_rng_state[1], g_rng_state[2], g_rng_state[3]);
}

}  // namespace

int main() {
    std::printf("# MAP reference vectors (P2) for max_core::setup / max_core::map.\n");
    std::printf("# Generated by max-rust/tools/map_vectors/main.cpp (see README).\n");
    std::printf("# Format v1: 'case' blocks with planet/levels/seed/start/pass_table/tile_ids/\n");
    std::printf("# surface/alien_costs/resources/placement/rng_state records.\n");

    for (const auto& config : CASES) {
        run_case(config);
    }

    return 0;
}
