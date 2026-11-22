//! Packet 45 CRC16 and canonical state digests (P7).
//!
//! # Oracle
//!
//! * `max-cpp-port/src/crc16.c` — byte-wise CRC16 (polynomial `0x1021`, MSB
//!   first, no reflection, initial value chosen by the caller);
//! * `max-cpp-port/src/game_manager.cpp:2315` — `GameManager_GetCrc16`, a word-wise
//!   CRC step over a **16-bit** value (equivalent to two byte-wise steps with
//!   the high byte first — see [`get_crc16_word`]);
//! * `game_manager.cpp:2327` — `GameManager_GetUnitListChecksum` (which unit
//!   fields enter the CRC);
//! * `game_manager.cpp:2356` — `GameManager_CheckDesync` (order of lists,
//!   teams and initial value `0xFFFF`);
//! * `remote.cpp:1824` (`Remote_CheckDesync`), `remote.cpp:3556`
//!   (`Remote_SendNetPacket_45`), `remote.cpp:3568`
//!   (`Remote_ReceiveNetPacket_45`) — only the **transport** of packet 45
//!   (turn counter + `uint16_t crc_checksum`); the CRC itself is computed by
//!   `GameManager_CheckDesync` via `crc16()`/`GameManager_GetCrc16`;
//! * `max-brh-docs/protocol.md` §8.6 — packet 45 as the end-of-turn barrier.
//!
//! # Two digests
//!
//! * [`state_checksum`] — the **network** CRC of packet 45: exactly the fields and order
//!   that `GameManager_CheckDesync` iterates. This is the value sent in
//!   [`crate::input::Packet45CrcBarrier`] and compared by peers.
//! * [`replay_digest_bytes`] — the **canonical** digest for end-to-end
//!   parity tests (P7): map (CRC of both maps), teams, units in the order of the five
//!   lists and CRC16 of the whole digest. The fields are the simulation subset of
//!   [`crate::game::GameState::snapshot_bytes`] (without P6 AI tasks/reminders);
//!   the C++ generator prints exactly the same fields
//!   `max-brh/tools/sim_vectors/main.cpp`, so the comparison is bit for bit.
//! * [`state_digest`] — CRC16 "as in packet 45" over the full
//!   [`crate::game::GameState::snapshot_bytes`] (a desync detector for tests).
//!
//! All functions are pure, traversal order is fixed (DESIGN §6.2): no iteration
//! over `HashMap`.

use crate::enums::unit_flags::{EXPLODING, STATIONARY};
use crate::game::{GameState, PLAYER_TEAM_COUNT, TEAM_SLOTS};
use crate::team::{TeamId, TeamKind};
use crate::unit::UnitListKind;
use crate::unit_values::ATTRIB_COUNT;
use crate::victory::cargo_type_of;

/// CRC16 polynomial (CCITT): `x^16 + x^12 + x^5 + 1` — `crc16.c:38`.
pub const CRC16_POLYNOMIAL: u16 = 0x1021;
/// Initial CRC value of packet 45 — `game_manager.cpp:2362` (`0xFFFF`).
pub const CRC16_INITIAL: u16 = 0xFFFF;

/// One byte-wise CRC16 step — a verbatim port of `crc16()` (`crc16.c:24`).
///
/// ```c
/// for (i = 0; i < 8; i++) {
///     carry = *crc & 0x8000;
///     *crc <<= 1;
///     if (c & 0x80) (*crc)++;
///     c <<= 1;
///     if (carry) *crc ^= 0x1021;
/// }
/// ```
///
/// Extrapolation: the function is linear in `crc` and in the bits of `byte` over GF(2), so
/// 16 byte-wise steps with zero bytes give exactly `L^16(crc)`, and the contribution of
/// data bits is the 16-bit word itself (MSB first). This is
/// [`get_crc16_word`].
#[inline]
pub fn crc16_update_byte(mut crc: u16, byte: u8) -> u16 {
    let mut c = byte;

    for _ in 0..8 {
        let carry = crc & 0x8000;
        crc <<= 1;

        if c & 0x80 != 0 {
            crc = crc.wrapping_add(1);
        }
        c <<= 1;

        if carry != 0 {
            crc ^= CRC16_POLYNOMIAL;
        }
    }

    crc
}

/// CRC16 of a buffer starting from [`CRC16_INITIAL`] (streaming application
/// [`crc16_update_byte`]).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = CRC16_INITIAL;

    for &byte in data {
        crc = crc16_update_byte(crc, byte);
    }

    crc
}

/// `GameManager_GetCrc16` (`game_manager.cpp:2315`) — a word-wise step.
///
/// ```c
/// for (i = 0; i < 16; ++i) {
///     if (crc & 0x8000) crc = (crc << 1) ^ 0x1021u; else crc <<= 1;
/// }
/// return crc ^ data;
/// ```
///
/// Equivalent to two [`crc16_update_byte`] calls with the high, then low
/// byte of `data` (verified by the `word_step_matches_two_byte_steps` test).
#[inline]
pub fn get_crc16_word(data: u16, crc_checksum: u16) -> u16 {
    let mut crc = crc_checksum;

    for _ in 0..16 {
        if crc & 0x8000 != 0 {
            crc = (crc << 1) ^ CRC16_POLYNOMIAL;
        } else {
            crc <<= 1;
        }
    }

    crc ^ data
}

// ---------------------------------------------------------------------------
// Packet 45 CRC — the network desync detector
// ---------------------------------------------------------------------------

/// `GameManager_GetUnitListChecksum` (`game_manager.cpp:2327`).
///
/// Only units with `id != NO_ID`, without the `EXPLODING` flag, and only
/// of the requested team participate. Field order is verbatim from C++:
/// `team, unit_type, unit_id, grid_x, grid_y, hits, [speed if not STATIONARY],
/// shots, [storage if the type has a cargo type], ammo`.
///
/// The caller's `team` condition (`team_type != NONE || team == ALIEN`) is checked by
/// [`state_checksum`], as in `GameManager_CheckDesync`.
pub fn get_unit_list_checksum(
    state: &GameState,
    kind: UnitListKind,
    team: TeamId,
    mut crc_checksum: u16,
) -> u16 {
    for &id in state.unit_list(kind) {
        let Some(unit) = state.unit(id) else { continue };

        if unit.id.is_no_id() || unit.flags & EXPLODING != 0 || unit.team != team {
            continue;
        }

        crc_checksum = get_crc16_word(unit.team.0 as u16, crc_checksum);
        crc_checksum = get_crc16_word(unit.unit_type as u16, crc_checksum);
        crc_checksum = get_crc16_word(u16::from(unit.unit_id), crc_checksum);
        crc_checksum = get_crc16_word(unit.grid.x as u16, crc_checksum);
        crc_checksum = get_crc16_word(unit.grid.y as u16, crc_checksum);
        crc_checksum = get_crc16_word(unit.hits, crc_checksum);

        if unit.flags & STATIONARY == 0 {
            crc_checksum = get_crc16_word(unit.speed, crc_checksum);
        }

        crc_checksum = get_crc16_word(u16::from(unit.shots), crc_checksum);

        if cargo_type_of(unit.unit_type) as u8 != 0 {
            crc_checksum = get_crc16_word(unit.storage as u16, crc_checksum);
        }

        crc_checksum = get_crc16_word(u16::from(unit.ammo), crc_checksum);
    }

    crc_checksum
}

/// `GameManager_CheckDesync` (`game_manager.cpp:2356`) — CRC of packet 45.
///
/// The outer loop is teams `RED..ALIEN`, the inner loop is the four lists
/// (`GroundCover`, `MobileLandSea`, `Stationary`, `MobileAir`); the
/// `ParticleUnits` list does **not** enter the CRC (as in C++). Initial value `0xFFFF`.
pub fn state_checksum(state: &GameState) -> u16 {
    let mut crc_checksum = CRC16_INITIAL;

    for kind in [
        UnitListKind::GroundCover,
        UnitListKind::MobileLandSea,
        UnitListKind::Stationary,
        UnitListKind::MobileAir,
    ] {
        for team in 0..TEAM_SLOTS {
            let kind_of_team = state.teams[team].team_type;
            if kind_of_team == TeamKind::None && team != TeamId::ALIEN.0 as usize {
                continue;
            }

            crc_checksum = get_unit_list_checksum(state, kind, TeamId(team as u8), crc_checksum);
        }
    }

    crc_checksum
}

/// CRC16 of the full canonical digest [`GameState::snapshot_bytes`].
///
/// Used by determinism tests as a "desync detector": any change to a
/// simulation field changes the value.
pub fn state_digest(state: &GameState) -> u16 {
    crc16(&state.snapshot_bytes())
}

// ---------------------------------------------------------------------------
// Canonical replay digest (P7 parity)
// ---------------------------------------------------------------------------

/// Replay digest magic (`replay_digest_bytes`).
pub const REPLAY_DIGEST_MAGIC: &[u8; 8] = b"MAXRPL1\0";

/// Canonical state digest for end-to-end parity tests.
///
/// The format is an explicit little-endian writer (DESIGN §6.4: no `serde`, no
/// `HashMap` iteration). Field order is fixed and matches
/// `max-brh/tools/sim_vectors/main.cpp` (`write_state_digest`):
///
/// | Field | Type |
/// |---|---|
/// | `REPLAY_DIGEST_MAGIC` | 8 bytes |
/// | CRC16 `SurfaceMap` | u16 |
/// | CRC16 `ResourceMap` (u16 LE) | u16 |
/// | `turn_counter` | i32 |
/// | `game_state`, `active_turn_team`, `player_team`, `play_mode` | u8×4 |
/// | `unit_id_counter` | u32 |
/// | `rng.state()` | u32×4 |
/// | per team `0..PLAYER_TEAM_MAX`: `team_type`, `finished_turn`, `team_points`, `gold`, 8×(`research_level`,`turns_to_complete`,`allocation`), 93×`unit_counters`, 93×`casualties` | see code |
/// | `unit_count` | u32 |
/// | units in [`GameState::unit_ids_in_order`] order (5 lists + outside lists) | see code |
/// | CRC16 of all preceding bytes | u16 |
///
/// Unit fields: `id, unit_type, team, unit_id, flags, grid_x, grid_y,
/// orders/state/prior_orders/prior_state, hits, speed, group_speed, shots,
/// move_and_fire, storage, experience, ammo, build_time, total_mining,
/// raw/fuel/gold_mining, raw/fuel/gold_mining_max, build_rate, ai_state_bits,
/// connectors, 14 `base_values` attributes, path (kind + data), parent/enemy,
/// build_list`.
pub fn replay_digest_bytes(state: &GameState) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 * 1024);

    out.extend_from_slice(REPLAY_DIGEST_MAGIC);
    put_u16(&mut out, crc16(&state.map.surface.to_bytes()));
    put_u16(&mut out, crc16(&state.map.resources.to_le_bytes()));

    put_i32(&mut out, state.turn_counter);
    out.push(state.game_state as u8);
    out.push(state.active_turn_team.0);
    out.push(state.player_team.0);
    out.push(state.play_mode as u8);
    put_u32(&mut out, state.unit_id_counter);
    for word in state.rng.state() {
        put_u32(&mut out, word);
    }

    for team in 0..TEAM_SLOTS {
        let info = &state.teams[team];
        out.push(info.team_type as u8);
        out.push(u8::from(info.finished_turn));
        put_u32(&mut out, info.team_points);
        put_u32(&mut out, state.team_units[team].gold);

        for topic in &info.research_topics {
            put_u32(&mut out, topic.research_level);
            put_u32(&mut out, topic.turns_to_complete);
            put_u32(&mut out, topic.allocation);
        }

        for value in &info.unit_counters {
            put_u32(&mut out, *value);
        }

        for value in &info.casualties {
            put_u32(&mut out, *value);
        }
    }

    let ids = state.unit_ids_in_order();
    put_u32(&mut out, ids.len() as u32);

    for id in ids {
        let Some(unit) = state.unit(id) else { continue };

        put_u16(&mut out, unit.id.0);
        put_u16(&mut out, unit.unit_type as u16);
        out.push(unit.team.0);
        out.push(unit.unit_id);
        put_u32(&mut out, unit.flags);
        put_i16(&mut out, unit.grid.x);
        put_i16(&mut out, unit.grid.y);
        out.push(unit.orders as u8);
        out.push(unit.state as u8);
        out.push(unit.prior_orders as u8);
        out.push(unit.prior_state as u8);
        put_u16(&mut out, unit.hits);
        put_u16(&mut out, unit.speed);
        put_u16(&mut out, unit.group_speed);
        out.push(unit.shots);
        out.push(unit.move_and_fire);
        put_i16(&mut out, unit.storage);
        put_i16(&mut out, unit.experience);
        out.push(unit.ammo);
        out.push(unit.build_time);
        out.push(unit.total_mining);
        out.push(unit.raw_mining);
        out.push(unit.fuel_mining);
        out.push(unit.gold_mining);
        out.push(unit.raw_mining_max);
        out.push(unit.gold_mining_max);
        out.push(unit.fuel_mining_max);
        put_u16(&mut out, unit.build_rate);
        put_u32(&mut out, unit.ai_state_bits);
        put_u16(&mut out, unit.connectors);

        for attribute in 0..ATTRIB_COUNT {
            put_i16(&mut out, unit.base_values.get_attribute(attribute) as i16);
        }

        match &unit.path {
            None => out.push(0),
            Some(crate::unit::UnitPath::Ground(path)) => {
                out.push(1);
                put_i16(&mut out, path.end_x);
                put_i16(&mut out, path.end_y);
                put_u32(&mut out, path.step_index);
                put_u32(&mut out, path.steps.len() as u32);
                for step in &path.steps {
                    out.push(step.x as u8);
                    out.push(step.y as u8);
                }
            }
            Some(crate::unit::UnitPath::Air(path)) => {
                out.push(2);
                put_i16(&mut out, path.end_x);
                put_i16(&mut out, path.end_y);
                put_i16(&mut out, path.length);
                out.push(path.angle as u8);
            }
            Some(crate::unit::UnitPath::Builder(path)) => {
                out.push(3);
                put_i16(&mut out, path.direction_x);
                put_i16(&mut out, path.direction_y);
            }
        }

        put_u16(&mut out, unit.parent_unit.map(|p| p.0).unwrap_or(u16::MAX));
        put_u16(&mut out, unit.enemy_unit.map(|p| p.0).unwrap_or(u16::MAX));
        put_u32(&mut out, unit.build_list.len() as u32);
        for unit_type in &unit.build_list {
            put_u16(&mut out, *unit_type as u16);
        }
    }

    let crc = crc16(&out);
    put_u16(&mut out, crc);
    out
}

/// CRC16 of the canonical replay digest (the last two bytes
/// [`replay_digest_bytes`]).
pub fn replay_digest_crc(state: &GameState) -> u16 {
    let bytes = replay_digest_bytes(state);
    u16::from_le_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]])
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Number of teams participating in network barriers (`PLAYER_TEAM_MAX - 1`).
pub const NETWORK_TEAM_COUNT: usize = PLAYER_TEAM_COUNT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
    use crate::game::GameState;
    use crate::geo::Point;
    use crate::team::{TeamKind, TeamUnits};

    /// Vectors of the `crc` section of `max-brh/tools/sim_vectors/vectors.txt`
    /// (generated by the C++ generator from `crc16.c` in ubuntu:24.04).
    const VECTORS: &str = include_str!("../../../tools/sim_vectors/vectors.txt");

    fn hex_to_bytes(text: &str) -> Vec<u8> {
        let bytes = text.as_bytes();
        assert_eq!(bytes.len() % 2, 0, "hex string has odd length");

        (0..bytes.len() / 2)
            .map(|index| {
                let pair =
                    std::str::from_utf8(&bytes[index * 2..index * 2 + 2]).expect("ascii hex");
                u8::from_str_radix(pair, 16).expect("valid hex byte")
            })
            .collect()
    }

    /// `crc16.c` on known inputs.
    ///
    /// Note: this is **not** CRC-16/CCITT-FALSE (whose "123456789" = 0x29B1)
    /// and not CRC-16/XMODEM: `crc16()` injects the data bit into the least significant
    /// bit **after** the shift, so the stream differs from the classical variants.
    /// The reference is the C++ vectors from `crc16.c` (see `crc16_matches_cpp_vectors`),
    /// and here the basic properties are pinned (initial value and one byte).
    #[test]
    fn crc16_basics() {
        assert_eq!(crc16(b""), CRC16_INITIAL);
        assert_eq!(crc16(&[0x00]), 0xE1F0);
        assert_eq!(crc16(&[0xFF]), 0xE10F);
        assert_eq!(crc16(&[0x01, 0x02, 0x03, 0x04]), 0x94B7);
    }

    /// The word-wise `GameManager_GetCrc16` step is equivalent to two byte-wise steps
    /// (high byte first) — over all 16-bit inputs and different states.
    #[test]
    fn word_step_matches_two_byte_steps() {
        let states = [0x0000u16, 0xFFFF, 0x1234, 0x8000, 0x29B1];

        for state in states {
            for data in [0x0000u16, 0x0001, 0x00FF, 0xFF00, 0xFFFF, 0x1234, 0xABCD] {
                let mut bytes = state;
                bytes = crc16_update_byte(bytes, (data >> 8) as u8);
                bytes = crc16_update_byte(bytes, data as u8);

                assert_eq!(
                    get_crc16_word(data, state),
                    bytes,
                    "state {state:#06x} data {data:#06x}"
                );
            }
        }
    }

    /// Comparison with the C++ vectors: byte-wise CRCs and word-wise steps.
    #[test]
    fn crc16_matches_cpp_vectors() {
        let mut checked_bytes = 0;
        let mut checked_words = 0;

        for line in VECTORS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();

            match parts.next().expect("record key") {
                "crc_bytes" => {
                    let payload = hex_to_bytes(parts.next().expect("payload"));
                    let expected =
                        u16::from_str_radix(parts.next().expect("crc"), 16).expect("hex crc");
                    assert_eq!(crc16(&payload), expected, "crc_bytes {line}");
                    checked_bytes += 1;
                }
                "crc_word" => {
                    let data =
                        u16::from_str_radix(parts.next().expect("data"), 16).expect("hex data");
                    let state =
                        u16::from_str_radix(parts.next().expect("state"), 16).expect("hex state");
                    let expected =
                        u16::from_str_radix(parts.next().expect("crc"), 16).expect("hex crc");
                    assert_eq!(get_crc16_word(data, state), expected, "crc_word {line}");
                    checked_words += 1;
                }
                // Other sections (replay) are checked in `tests/p7_replay.rs`.
                _ => {}
            }
        }

        assert!(checked_bytes >= 5, "expected byte CRC vectors");
        assert!(checked_words >= 4, "expected word CRC vectors");
    }

    fn replay_state() -> GameState {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Computer;
        state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
        state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
        state.play_mode = PlayMode::TurnBased;

        let red = state
            .spawn_unit(UnitType::Engineer, TeamId::RED, Point::new(20, 20), 0)
            .expect("engineer");
        let green = state
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(24, 20), 0)
            .expect("tank");
        state.set_order(red, UnitOrder::Await, UnitOrderState::ExecutingOrder);
        state.set_order(green, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
        state
    }

    #[test]
    fn checksum_visits_only_units_of_the_team() {
        let state = replay_state();
        let checksum = state_checksum(&state);

        // The red team is counted in the CRC; an empty blue team adds nothing.
        let mut expected = CRC16_INITIAL;
        for kind in [
            UnitListKind::GroundCover,
            UnitListKind::MobileLandSea,
            UnitListKind::Stationary,
            UnitListKind::MobileAir,
        ] {
            for team in 0..TEAM_SLOTS {
                let kind_of_team = state.teams[team].team_type;
                if kind_of_team == TeamKind::None && team != TeamId::ALIEN.0 as usize {
                    continue;
                }
                expected = get_unit_list_checksum(&state, kind, TeamId(team as u8), expected);
            }
        }
        assert_eq!(checksum, expected);
    }

    #[test]
    fn checksum_reacts_to_every_tracked_field() {
        let base = replay_state();
        let reference = state_checksum(&base);
        let id = base.unit_ids_in_order()[0];

        let mut moved = base.clone();
        if let Some(unit) = moved.unit(id).cloned() {
            let mut unit = unit;
            unit.grid.x += 1;
            moved.write_unit(id, unit);
        }
        assert_ne!(state_checksum(&moved), reference);

        let mut damaged = base.clone();
        if let Some(unit) = damaged.unit(id).cloned() {
            let mut unit = unit;
            unit.hits = unit.hits.wrapping_sub(1);
            damaged.write_unit(id, unit);
        }
        assert_ne!(state_checksum(&damaged), reference);
    }

    #[test]
    fn replay_digest_is_stable_and_deterministic() {
        let state = replay_state();
        let first = replay_digest_bytes(&state);
        let second = replay_digest_bytes(&state.clone());

        assert_eq!(first, second);
        assert_eq!(&first[..8], REPLAY_DIGEST_MAGIC);
        assert_eq!(replay_digest_crc(&state), crc16(&first[..first.len() - 2]));

        let mut changed = state;
        changed.turn_counter += 1;
        assert_ne!(replay_digest_bytes(&changed), first);
    }

    #[test]
    fn particle_list_does_not_take_part_in_network_crc() {
        let mut state = replay_state();
        // A missile/particle must not affect the packet 45 CRC.
        let reference = state_checksum(&state);
        state
            .spawn_unit(UnitType::Rocket, TeamId::RED, Point::new(30, 30), 0)
            .expect("rocket");
        assert_eq!(state_checksum(&state), reference);
    }
}
