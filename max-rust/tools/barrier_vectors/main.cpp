// Reference-vector generator for the barrier wire format
// (max_net::wire state/command codecs: packets 53/54/45).
//
// The C++ headless (max-cpp-port/src/remote.cpp) is the canon. This program is a
// self-contained copy of the serializers used by
// `Remote_SendNetPacket_53` (remote.cpp:4243), `Remote_SendUnitStates`
// (remote.cpp:4333) and `Remote_SendNetPacket_45` (remote.cpp:3556), with the
// exact `struct Packet23Data` field order from `Remote_ReadPacket23Data`
// (remote.cpp:3298) and the `Complex::WritePacket` order (complex.cpp:65).
//
// Build & run (Ubuntu 24.04 / Debian bookworm):
//   g++ -O2 -std=c++17 -Wall -Wextra main.cpp -o gen && ./gen > vectors.txt
//
// Output format v1 (one packet per line, '#' lines are comments):
//   packet <number> <hex bytes, lowercase, no separators>
// plus a comment block with the C++ sizes used by the serializers.

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {

// ---------------------------------------------------------------------------
// NetPacket's write side (net_packet.cpp): only `operator<<` for scalars and
// arrays is used by packets 53/54/45.
// ---------------------------------------------------------------------------
class Packet {
    std::vector<std::uint8_t> buffer;

public:
    void write(const void* data, std::size_t length) {
        const auto* bytes = static_cast<const std::uint8_t*>(data);
        buffer.insert(buffer.end(), bytes, bytes + length);
    }

    template <typename T>
    void put(const T& data) {
        write(&data, sizeof(T));
    }

    const std::vector<std::uint8_t>& bytes() const { return buffer; }
};

// `struct Packet23Data` (docs/protocol.md §8.5). Field order is the wire order
// of `Remote_ReadPacket23Data`; these fields are packed by the game's ABI
// (uint8_t/bool/int16_t/uint16_t only), so `sizeof` is 45.
struct Packet23Data {
    std::uint8_t orders;
    std::uint8_t state;
    std::uint8_t prior_orders;
    std::uint8_t prior_state;
    bool disabled_reaction_fire;
    std::uint16_t parent_unit_id;
    std::int16_t experience;
    std::int16_t move_to_grid_x;
    std::int16_t move_to_grid_y;
    std::int16_t fire_on_grid_x;
    std::int16_t fire_on_grid_y;
    std::uint16_t enemy_unit_id;
    std::int16_t transfer_cargo;
    std::uint8_t stealth_dice_roll;
    std::uint8_t total_mining;
    std::uint8_t raw_mining;
    std::uint8_t fuel_mining;
    std::uint8_t gold_mining;
    std::uint8_t build_time;
    std::uint16_t build_rate;
    std::uint16_t unit_type;
    std::uint8_t unit_id;
    std::int16_t grid_x;
    std::int16_t grid_y;
    std::uint8_t team;
    std::uint16_t hits;
    std::uint16_t speed;
    std::uint8_t shots;
    std::int16_t storage;
    std::uint8_t ammo;
};

// `struct ResearchTopic` (ctinfo.hpp): three int32_t.
struct ResearchTopic {
    std::int32_t research_level;
    std::int32_t turns_to_complete;
    std::int32_t allocation;
};

constexpr std::uint32_t RESEARCH_TOPIC_COUNT = 8;
constexpr std::uint32_t UNIT_END = 93;

static_assert(sizeof(ResearchTopic) == 12, "ResearchTopic must be 12 bytes");
static_assert(sizeof(bool) == 1, "C++ bool is one byte on this ABI");
// `Remote_ReadPacket23Data` writes the fields one by one, so struct padding
// (`sizeof(Packet23Data)` is 50 on this ABI) is NOT on the wire. The wire size is
// the sum of the field widths: 45 bytes — the same as
// `max_net::wire::state::UnitState` writes.

// Remote_WritePacket23Data (the `packet << data.field` chain of
// `Remote_CreateNetPacket_23`, remote.cpp:3298).
void write_packet23_data(Packet& packet, const Packet23Data& data) {
    packet.put(data.orders);
    packet.put(data.state);
    packet.put(data.prior_orders);
    packet.put(data.prior_state);
    packet.put(data.disabled_reaction_fire);
    packet.put(data.parent_unit_id);
    packet.put(data.experience);
    packet.put(data.move_to_grid_x);
    packet.put(data.move_to_grid_y);
    packet.put(data.fire_on_grid_x);
    packet.put(data.fire_on_grid_y);
    packet.put(data.enemy_unit_id);
    packet.put(data.transfer_cargo);
    packet.put(data.stealth_dice_roll);
    packet.put(data.total_mining);
    packet.put(data.raw_mining);
    packet.put(data.fuel_mining);
    packet.put(data.gold_mining);
    packet.put(data.build_time);
    packet.put(data.build_rate);
    packet.put(data.unit_type);
    packet.put(data.unit_id);
    packet.put(data.grid_x);
    packet.put(data.grid_y);
    packet.put(data.team);
    packet.put(data.hits);
    packet.put(data.speed);
    packet.put(data.shots);
    packet.put(data.storage);
    packet.put(data.ammo);
}

// `Complex::WritePacket` (complex.cpp:65) plus the complex id
// (`TeamUnits::WriteComplexPacket`, teamunits.cpp:120).
struct Complex {
    std::uint16_t id;
    std::int16_t material;
    std::int16_t fuel;
    std::int16_t gold;
    std::int16_t power;
    std::int16_t workers;
    std::int16_t buildings;
};

void write_complex(Packet& packet, const Complex& complex) {
    packet.put(complex.id);
    packet.put(complex.material);
    packet.put(complex.fuel);
    packet.put(complex.gold);
    packet.put(complex.power);
    packet.put(complex.workers);
    packet.put(complex.buildings);
}

// Remote_SendNetPacket_53 (remote.cpp:4243):
//   type, team u16, turn i32, count u16, count × (complex_id u16, 6×i16).
std::vector<std::uint8_t> packet_53() {
    Packet packet;
    const std::uint16_t team = 1;
    const std::int32_t turn = 7;
    const Complex complexes[] = {
        {3, -1, 2, 300, -4, 5, 6},
    };

    packet.put(static_cast<std::uint8_t>(0x35 + 0x50));  // REMOTE_PACKET_53
    packet.put(team);
    packet.put(turn);
    packet.put(static_cast<std::uint16_t>(sizeof(complexes) / sizeof(complexes[0])));
    for (const auto& complex : complexes) {
        write_complex(packet, complex);
    }
    return packet.bytes();
}

// Remote_SendUnitStates (remote.cpp:4333):
//   type, team u16, sync_index u8, is_last bool, count u16,
//   count × (unit_id u16, Packet23Data),
//   if is_last: gold u32, gold_spent u32, research[8] (i32×3), objects u16,
//               unit_counters[93] (u32).
std::vector<std::uint8_t> packet_54() {
    Packet packet;
    const std::uint16_t team = 2;
    const std::uint8_t sync_index = 3;
    const bool is_last = true;

    Packet23Data first{};
    first.orders = 2;   // ORDER_MOVE
    first.state = 1;    // ORDER_STATE_EXECUTING_ORDER
    first.prior_orders = 0;
    first.prior_state = 1;
    first.disabled_reaction_fire = true;
    first.parent_unit_id = 0;
    first.experience = 42;
    first.move_to_grid_x = 10;
    first.move_to_grid_y = 11;
    first.fire_on_grid_x = 12;
    first.fire_on_grid_y = 13;
    first.enemy_unit_id = 0;
    first.transfer_cargo = -2;
    first.stealth_dice_roll = 3;
    first.total_mining = 1;
    first.raw_mining = 2;
    first.fuel_mining = 3;
    first.gold_mining = 4;
    first.build_time = 5;
    first.build_rate = 6;
    first.unit_type = 0x33;  // TANK
    first.unit_id = 7;
    first.grid_x = 8;
    first.grid_y = 9;
    first.team = 2;  // GREEN
    first.hits = 100;
    first.speed = 11;
    first.shots = 2;
    first.storage = -300;
    first.ammo = 9;

    Packet23Data second{};
    second.orders = 0x0C;  // ORDER_SENTRY
    second.state = 1;
    second.prior_orders = 0x10;  // ORDER_IDLE
    second.prior_state = 1;
    second.disabled_reaction_fire = false;
    second.parent_unit_id = 0x1234;
    second.experience = -5;
    second.move_to_grid_x = 20;
    second.move_to_grid_y = 21;
    second.fire_on_grid_x = 22;
    second.fire_on_grid_y = 23;
    second.enemy_unit_id = 0x4321;
    second.transfer_cargo = 7;
    second.stealth_dice_roll = 8;
    second.total_mining = 9;
    second.raw_mining = 10;
    second.fuel_mining = 11;
    second.gold_mining = 12;
    second.build_time = 13;
    second.build_rate = 14;
    second.unit_type = 1;
    second.unit_id = 15;
    second.grid_x = -1;
    second.grid_y = -2;
    second.team = 2;
    second.hits = 200;
    second.speed = 16;
    second.shots = 17;
    second.storage = 18;
    second.ammo = 19;

    const std::uint16_t unit_ids[] = {0x0102, 0x0304};

    packet.put(static_cast<std::uint8_t>(0x36 + 0x50));  // REMOTE_PACKET_54
    packet.put(team);
    packet.put(sync_index);
    packet.put(is_last);
    packet.put(static_cast<std::uint16_t>(2));

    packet.put(unit_ids[0]);
    write_packet23_data(packet, first);
    packet.put(unit_ids[1]);
    write_packet23_data(packet, second);

    if (is_last) {
        packet.put(static_cast<std::uint32_t>(1234));
        packet.put(static_cast<std::uint32_t>(56));

        for (std::uint32_t i = 0; i < RESEARCH_TOPIC_COUNT; ++i) {
            ResearchTopic topic{};
            topic.research_level = static_cast<std::int32_t>(i);
            topic.turns_to_complete = static_cast<std::int32_t>(10 + i);
            topic.allocation = static_cast<std::int32_t>(20 + i);
            packet.put(topic);
        }

        packet.put(static_cast<std::uint16_t>(7));

        for (std::uint32_t i = 0; i < UNIT_END; ++i) {
            packet.put(static_cast<std::uint32_t>(i * 3 + 1));
        }
    }
    return packet.bytes();
}

// Remote_SendNetPacket_45 (remote.cpp:3556):
//   type, team u16, next_turn_index u8, crc_checksum u16.
std::vector<std::uint8_t> packet_45() {
    Packet packet;

    packet.put(static_cast<std::uint8_t>(0x2D + 0x50));  // REMOTE_PACKET_45
    packet.put(static_cast<std::uint16_t>(3));
    packet.put(static_cast<std::uint8_t>(4));
    packet.put(static_cast<std::uint16_t>(0xABCD));
    return packet.bytes();
}

void print_packet(const char* label, const std::vector<std::uint8_t>& bytes) {
    std::printf("packet %s ", label);
    for (const auto byte : bytes) {
        std::printf("%02x", byte);
    }
    std::printf("  # %zu bytes\n", bytes.size());
}

}  // namespace

int main() {
    std::printf("# barrier wire vectors generated from max-cpp-port (remote.cpp)\n");
    std::printf("# sizeof(Packet23Data)=%zu, sizeof(ResearchTopic)=%zu\n", sizeof(Packet23Data),
                sizeof(ResearchTopic));

    print_packet("53", packet_53());
    print_packet("54", packet_54());
    print_packet("45", packet_45());
    return 0;
}
