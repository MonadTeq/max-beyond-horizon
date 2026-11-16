// Генератор C++-векторов для экрана мобилизации (`CargoMenu`) — P7.
//
// Повторяет **формулы и условия C++-порта**, не Rust:
//   * `Cargo_GetRawConsumptionRate` (`max-cpp-port/src/cargo.cpp:140`);
//   * `CargoMenu::unit_types1`/`BuyUnit` (`cargomenu.cpp:176/546`);
//   * `PurchaseTypeSelector::Draw` — цена строки списка типов
//     (`purchasetypeselector.cpp:40`);
//   * `AbstractUpgradeMenu::IsBuildable/IsUnitFiltered/IsBetterUnit`
//     (`abstractupgrademenu.cpp:681/699/727`);
//   * `AbstractUpgradeMenu::DrawUnitInfo` + `AddUpgradeMilitary/Generic/Mobile`
//     (`abstractupgrademenu.cpp:224/296/320`) — состав строк апгрейда;
//   * `UpgradeControl::Init/CalculateCost` + `UpgradeControl_CalculateCost`
//     (`upgradecontrol.cpp:102/58/214`, `std::pow`).
//
// Данные (флаги/атрибуты/строители) — `data.inc`, сгенерирован
// `max-rust/tools/gen_cargo_data.py` из `assets/PATCHES.RES` (`SC_UNITS`,
// `SC_ATTRI`) и `max_builder_capability_list` (`default.game_rules.lua`).
//
// Запуск:
//   docker run --rm -v "D:/dev/max-port-debug:/src" \
//       -w /src/max-rust/tools/cargo_vectors max-port-build:latest \
//       bash -c 'g++ -O2 -std=c++17 main.cpp -o /tmp/cargo_vectors && \
//                /tmp/cargo_vectors > /src/max-rust/tools/cargo_vectors/vectors.txt'
//
// Формат (по одной записи на строку):
//   purchase     <index> <unit> <turns> <price>
//   listprice    <unit> <stationary|infantry|mobile> <price>
//   upgrade_list <mask> <count> <unit>...
//   rows         <unit> <attr> <cost> <attr> <cost> ...
//   stats        <unit> <rows> <attr>...          (порядок строк DrawUnitStats)

#include <cstdio>
#include <cstdint>
#include <cmath>
#include <vector>
#include <algorithm>

#include "data.inc"

// --- unit_flags (max-cpp-port/src/unit.hpp) ---------------------------------
enum {
    GROUND_COVER = 0x1,
    BUILDING = 0x10,
    MISSILE_UNIT = 0x20,
    MOBILE_AIR_UNIT = 0x40,
    MOBILE_SEA_UNIT = 0x80,
    MOBILE_LAND_UNIT = 0x100,
    STATIONARY = 0x200,
    UPGRADABLE = 0x4000,
    TURRET_SPRITE = 0x200000,
    SPINNING_TURRET = 0x400000,
};

// --- AttributeControl (upgradecontrol.hpp) ----------------------------------
enum {
    ATTRIB_ATTACK = 0,
    ATTRIB_ROUNDS = 1,
    ATTRIB_RANGE = 2,
    ATTRIB_ARMOR = 3,
    ATTRIB_HITS = 4,
    ATTRIB_SPEED = 5,
    ATTRIB_SCAN = 6,
    ATTRIB_TURNS = 7,
    ATTRIB_AMMO = 8,
};

static const int32_t UpgradeControl_Factors[9] = {4, 4, 8, 2, 2, 4, 8, 16, 2};

// UnitType ids (resourcetable.hpp): 0x31 CONSTRCT, 0x20 TRAINHAL,
// 0x42 COMMANDO, 0x43 INFANTRY.
enum {
    UNIT_CONSTRCT = 0x31,
    UNIT_TRAINHAL = 0x20,
    UNIT_COMMANDO = 0x42,
    UNIT_INFANTRY = 0x43,
};

// `CargoMenu::unit_types1` (cargomenu.cpp:176).
static const int kPurchase[] = {
    0x31, 0x3D, 0x39, 0x32, 0x3A, 0x33, 0x34, 0x35,
    0x36, 0x38, 0x3F, 0x3B, 0x40, 0x37, 0x3C,
};

/// `Cargo_GetRawConsumptionRate` (cargo.cpp:140) — дословно.
static int32_t Cargo_GetRawConsumptionRate(const int unit_type, const int32_t speed_multiplier) {
    int32_t multiplier;
    int32_t result;

    switch (speed_multiplier) {
        case 2:
            multiplier = 4;
            break;
        case 4:
            multiplier = 12;
            break;
        default:
            multiplier = speed_multiplier;
            break;
    }

    switch (unit_type) {
        case UNIT_TRAINHAL:
            result = multiplier;
            break;
        case 0x3D:  // ENGINEER
        case UNIT_CONSTRCT:
            result = multiplier * 2;
            break;
        case 0x17:  // SHIPYARD
        case 0x19:  // LANDPLT
        case 0x1B:  // AIRPLT
        case 0x18:  // LIGHTPLT
            result = multiplier * 3;
            break;
        default:
            result = 0;
            break;
    }

    return result;
}

/// `UpgradeControl_CalculateCost` (upgradecontrol.cpp:214) — `std::pow`.
static int32_t UpgradeControl_CalculateCost(int32_t id, uint16_t current_value, uint16_t factor,
                                            uint16_t base_value) {
    int32_t result;

    if (id == ATTRIB_TURNS) {
        result = std::pow((double)base_value / (double)((int32_t)current_value - factor), 7.5) *
                 UpgradeControl_Factors[id];
    } else if (base_value > 0) {
        result = std::pow((double)((int32_t)current_value - factor) / (double)base_value, 7.5) *
                 UpgradeControl_Factors[id];
    } else {
        result = 0;
    }

    return result;
}

/// `UpgradeControl::GetLevelCost`.
static int32_t GetLevelCost(int32_t id, uint16_t value, uint16_t research_factor, uint16_t base_value) {
    return UpgradeControl_CalculateCost(id, value, research_factor, base_value);
}

/// `UpgradeControl::Init` — шаг атрибута.
static uint16_t UpgradeAmount(int32_t id, int32_t team_base_value) {
    if (id == ATTRIB_TURNS) {
        return 1;
    } else if (team_base_value >= 10) {
        if (team_base_value >= 25) {
            if (team_base_value >= 50) {
                return 10;
            }
            return 5;
        }
        return 2;
    }
    return 1;
}

/// `UpgradeControl::CalculateCost` (upgradecontrol.cpp:58) для первого шага.
static int32_t FirstStepCost(int32_t id, int32_t team_base_value) {
    uint16_t amount = UpgradeAmount(id, team_base_value);
    uint16_t current = (uint16_t)team_base_value;
    int32_t result;

    if (id == ATTRIB_TURNS) {
        if (current > amount) {
            result = GetLevelCost(id, (uint16_t)(current - amount), 0, (uint16_t)team_base_value) -
                     GetLevelCost(id, current, 0, (uint16_t)team_base_value);
            if (result < 1) {
                result = 1;
            }
        } else {
            result = 0;
        }
    } else if (team_base_value > 0) {
        result = GetLevelCost(id, (uint16_t)(current + amount), 0, (uint16_t)team_base_value) -
                 GetLevelCost(id, current, 0, (uint16_t)team_base_value);
        if (result < 1) {
            result = 1;
        }
    } else {
        result = 0;
    }

    return result;
}

/// `Builder_IsBuildable` — таблица `default.game_rules.lua`.
static bool IsBuildable(int unit) {
    for (int i = 0; i < kBuildableCount; ++i) {
        if (kBuildable[i].unit == unit) {
            return true;
        }
    }
    return false;
}

/// `AbstractUpgradeMenu::IsBuildable` (abstractupgrademenu.cpp:681).
static bool MenuIsBuildable(int unit) {
    const unsigned flags = kUnits[unit].flags;
    bool buildable = IsBuildable(unit);

    if (!buildable && (flags & GROUND_COVER) && (flags & STATIONARY) && (unit != -1)) {
        // Мины апгрейдятся, если строится постановщик.
        if (unit == 0x29) {  // LANDMINE
            buildable = IsBuildable(0x38);  // MINELAYR
        } else if (unit == 0x2A) {  // SEAMINE
            buildable = IsBuildable(0x4A);  // SEAMNLYR
        }
    }

    return buildable;
}

/// `AbstractUpgradeMenu::IsUnitFiltered` (abstractupgrademenu.cpp:699).
static bool IsUnitFiltered(int unit, bool ground, bool air, bool sea, bool building, bool combat) {
    const unsigned flags = kUnits[unit].flags;

    if (!(flags & UPGRADABLE) || !MenuIsBuildable(unit)) {
        return false;
    }

    if (((flags & MOBILE_LAND_UNIT) && ground) || ((flags & MOBILE_AIR_UNIT) && air) ||
        ((flags & MOBILE_SEA_UNIT) && sea) || ((flags & STATIONARY) && building)) {
        if (!combat || kUnits[unit].attack > 0) {
            return true;
        }
    }

    return false;
}

/// `AbstractUpgradeMenu::IsBetterUnit` (abstractupgrademenu.cpp:727).
static bool IsBetterUnit(int unit1, int unit2) {
    const unsigned flags1 = kUnits[unit1].flags;
    const unsigned flags2 = kUnits[unit2].flags;

    if ((flags1 & MOBILE_LAND_UNIT) != (flags2 & MOBILE_LAND_UNIT)) {
        return !(flags1 & MOBILE_LAND_UNIT);
    }
    if ((flags1 & MOBILE_SEA_UNIT) != (flags2 & MOBILE_SEA_UNIT)) {
        return !(flags1 & MOBILE_SEA_UNIT);
    }
    if ((flags1 & MOBILE_AIR_UNIT) != (flags2 & MOBILE_AIR_UNIT)) {
        return !(flags1 & MOBILE_AIR_UNIT);
    }

    const int attack1 = kUnits[unit1].attack;
    const int attack2 = kUnits[unit2].attack;

    if (!attack1 || attack2) {
        if (attack1 || !attack2) {
            if (kUnits[unit1].turns == kUnits[unit2].turns) {
                return unit1 > unit2;
            }
            return kUnits[unit1].turns > kUnits[unit2].turns;
        }
        return true;
    }

    return false;
}

/// `AbstractUpgradeMenu::PopulateTeamUnitsList`.
static std::vector<int> UpgradeList(bool ground, bool air, bool sea, bool building, bool combat) {
    std::vector<int> types;

    for (int unit = 0; unit < kUnitCount; ++unit) {
        if (IsUnitFiltered(unit, ground, air, sea, building, combat)) {
            types.push_back(unit);
        }
    }

    if (types.size() > 1) {
        for (size_t i = 0; i + 1 < types.size(); ++i) {
            for (size_t j = i + 1; j < types.size(); ++j) {
                if (IsBetterUnit(types[i], types[j])) {
                    std::swap(types[i], types[j]);
                }
            }
        }
    }

    return types;
}

/// `AbstractUpgradeMenu::AddUpgradeMilitary/Generic/Mobile` —
/// порядок и условия строк.
static std::vector<int> UpgradeRows(int unit) {
    std::vector<int> rows;

    if (kUnits[unit].attack) {
        rows.push_back(ATTRIB_ATTACK);
        if (kUnits[unit].rounds) {
            rows.push_back(ATTRIB_ROUNDS);
        }
        if (kUnits[unit].range) {
            rows.push_back(ATTRIB_RANGE);
        }
        if (kUnits[unit].ammo) {
            rows.push_back(ATTRIB_AMMO);
        }
    }

    // `AdjustRowStorage`/`AdjustRowConsumptions` пропускают строки, но новых
    // апгрейд-строк не добавляют, поэтому на состав `rows` не влияют.

    if (kUnits[unit].armor) {
        rows.push_back(ATTRIB_ARMOR);
    }
    if (kUnits[unit].hits) {
        rows.push_back(ATTRIB_HITS);
    }
    if (kUnits[unit].scan) {
        rows.push_back(ATTRIB_SCAN);
    }
    if (kUnits[unit].speed) {
        rows.push_back(ATTRIB_SPEED);
    }

    return rows;
}

/// `CargoMenu::DrawUnitInfo`: `3 * turns` (LANDPLT = 3).
static int PurchasePrice(int unit) {
    return Cargo_GetRawConsumptionRate(0x19, 1) * kUnits[unit].turns;
}

/// `PurchaseTypeSelector::Draw` — цена строки списка типов.
static int ListPrice(int unit) {
    int cost;

    if (kUnits[unit].flags & STATIONARY) {
        cost = Cargo_GetRawConsumptionRate(UNIT_CONSTRCT, 1) * kUnits[unit].turns;
    } else if (unit == UNIT_COMMANDO || unit == UNIT_INFANTRY) {
        cost = Cargo_GetRawConsumptionRate(UNIT_TRAINHAL, 1) * kUnits[unit].turns;
    } else {
        cost = Cargo_GetRawConsumptionRate(0x19, 1) * kUnits[unit].turns;
    }

    return cost;
}

int main() {
    std::printf("# CargoMenu vectors - generated by max-rust/tools/cargo_vectors/main.cpp\n");
    std::printf("# source: max-cpp-port cargomenu/purchasetypeselector/abstractupgrademenu/"
                "upgradecontrol\n");

    int index = 0;
    for (int unit : kPurchase) {
        std::printf("purchase %d %d %d %d\n", index, unit, kUnits[unit].turns, PurchasePrice(unit));
        ++index;
    }

    for (int unit = 0; unit < kUnitCount; ++unit) {
        const char* kind = (kUnits[unit].flags & STATIONARY) ? "stationary"
                           : (unit == UNIT_COMMANDO || unit == UNIT_INFANTRY) ? "infantry"
                                                                              : "mobile";
        std::printf("listprice %d %s %d\n", unit, kind, ListPrice(unit));
    }

    // Маски фильтров: бит 0 ground, 1 air, 2 sea, 3 building, 4 combat.
    const bool masks[][5] = {
        {true, true, true, true, false},   // 0  — умолчание CargoMenu
        {true, true, true, true, true},    // 1  — + combat
        {true, false, false, false, false},// 2  — только земля
        {false, true, false, false, false},// 3  — только воздух
        {false, false, true, false, false},// 4  — только море
        {false, false, false, true, false},// 5  — только здания
        {false, false, false, false, true},// 6  — только боевые (без домена)
    };

    for (int mask = 0; mask < 7; ++mask) {
        std::vector<int> list =
            UpgradeList(masks[mask][0], masks[mask][1], masks[mask][2], masks[mask][3], masks[mask][4]);
        std::printf("upgrade_list %d %zu", mask, list.size());
        for (int unit : list) {
            std::printf(" %d", unit);
        }
        std::printf("\n");
    }

    for (int unit = 0; unit < kUnitCount; ++unit) {
        std::vector<int> rows = UpgradeRows(unit);
        std::printf("rows %d %zu", unit, rows.size());
        for (int id : rows) {
            const int base = (id == ATTRIB_ATTACK)      ? kUnits[unit].attack
                             : (id == ATTRIB_ROUNDS)    ? kUnits[unit].rounds
                             : (id == ATTRIB_RANGE)     ? kUnits[unit].range
                             : (id == ATTRIB_AMMO)      ? kUnits[unit].ammo
                             : (id == ATTRIB_ARMOR)     ? kUnits[unit].armor
                             : (id == ATTRIB_HITS)      ? kUnits[unit].hits
                             : (id == ATTRIB_SCAN)      ? kUnits[unit].scan
                             : (id == ATTRIB_SPEED)     ? kUnits[unit].speed
                                                        : 0;
            std::printf(" %d %d", id, FirstStepCost(id, base));
        }
        std::printf("\n");
    }

    return 0;
}
