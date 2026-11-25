//! Phase 4 — CRC16 and canonical state digests (D layer).
//!
//! What is proved here:
//!
//! * the byte-wise and word-wise CRC steps are **bit-identical** to an
//!   independent reference implementation translated from
//!   `max-cpp-port/src/crc16.c` / `game_manager.cpp:2315` (the reference uses
//!   `u32` accumulation, i.e. the C `int` promotion, instead of `u16`);
//! * whole real V70 files (`max-original/SAVE1.CAM`/`SAVE2.CAM`) hash to pinned
//!   values and are reproducible from the reference implementation;
//! * the canonical replay digest (`replay_digest_bytes`) and the packet 45 CRC
//!   (`state_checksum`) of states loaded from the reference saves are pinned
//!   ("snapshot lock": changes to the digest layout or to the simulation subset
//!   are caught here, while `p7` still checks the C++ ⇄ Rust vector);
//! * every field of the digest reacts as documented and the packet 45 CRC reacts
//!   only to the unit fields (it must not include `turn_counter` etc.).
//!
//! Oracles: `crc16.c`, `game_manager.cpp` (`GameManager_GetCrc16`,
//! `GameManager_CheckDesync`), `tools/sim_vectors/vectors.txt` (P7).

use max_core::crc::{
    self, get_crc16_word,
    CRC16_INITIAL, CRC16_POLYNOMIAL, REPLAY_DIGEST_MAGIC,
};
use max_core::enums::{PlayMode, UnitType};
use max_core::geo::Point;
use max_core::save::SaveFile;
use max_core::team::TeamId;
use max_core::universe::GameUniverse;
use max_core::Randomizer;

/// `max-original/SAVE1.CAM` — the training campaign "Islands in the Sun".
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM` — the "Heart of the Matter" campaign.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

/// One byte-wise CRC step, translated literally from `crc16.c:24`.
///
/// Deliberately a different structure from the port: the accumulator is
/// `u32` (the C `int` promotion of `uint16_t`), truncated only on return, and
/// the input byte is `i32` as in C.
fn reference_crc16_byte(crc: u16, byte: u8) -> u16 {
    let mut crc = u32::from(crc);
    let mut c = i32::from(byte);

    for _ in 0..8 {
        let carry = crc & 0x8000;
        crc <<= 1;

        if c & 0x80 != 0 {
            crc += 1;
        }
        c <<= 1;

        if carry != 0 {
            crc ^= 0x1021;
        }
    }

    (crc & 0xFFFF) as u16
}

/// Full buffer CRC through the reference step.
fn reference_crc16(data: &[u8]) -> u16 {
    let mut crc = CRC16_INITIAL;
    for &byte in data {
        crc = reference_crc16_byte(crc, byte);
    }
    crc
}

/// `GameManager_GetCrc16` reference: 16 shift/XOR rounds, then `^ data`.
fn reference_crc16_word(data: u16, crc: u16) -> u16 {
    let mut value = crc;
    for _ in 0..16 {
        value = if value & 0x8000 != 0 {
            (value << 1) ^ 0x1021
        } else {
            value << 1
        };
    }
    value ^ data
}

/// Deterministic pseudo-random bytes (LCG, independent of `Randomizer`) so the
/// test does not depend on the module under test.
fn pseudo_random_bytes(seed: u32, length: usize) -> Vec<u8> {
    let mut state = seed | 1;
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 16) as u8
        })
        .collect()
}

fn loaded_state(source: &[u8]) -> GameUniverse {
    GameUniverse::from_save(&SaveFile::parse(source).expect("reference save parses"))
}

// ---------------------------------------------------------------------------
// Parity with the C++ step (independent implementation)
// ---------------------------------------------------------------------------

#[test]
fn byte_step_matches_cpp_for_every_input_byte_and_state() {
    for state in [0x0000u16, 0x0001, 0x1021, 0x7FFF, 0x8000, 0xFFFF, 0xE1F0] {
        for byte in 0..=u8::MAX {
            assert_eq!(
                crc::crc16_update_byte(state, byte),
                reference_crc16_byte(state, byte),
                "state {state:#06x}, byte {byte:#04x}"
            );
        }
    }
}

#[test]
fn word_step_matches_cpp_for_every_word_and_state() {
    for state in [0x0000u16, 0xFFFF, 0x8001] {
        for data in 0..=u16::MAX {
            assert_eq!(
                get_crc16_word(data, state),
                reference_crc16_word(data, state),
                "state {state:#06x}, data {data:#06x}"
            );
        }
    }
}

#[test]
fn every_two_byte_buffer_matches_the_reference() {
    for first in 0..=u8::MAX {
        for second in 0..=u8::MAX {
            let buffer = [first, second];
            assert_eq!(
                crc::crc16(&buffer),
                reference_crc16(&buffer),
                "buffer {first:#04x}{second:#04x}"
            );
        }
    }
}

#[test]
fn deterministic_buffers_match_the_reference() {
    // Lengths around the byte/word boundaries and the C++ vector sizes.
    for length in [
        0usize, 1, 2, 3, 4, 7, 8, 15, 16, 31, 32, 63, 64, 127, 128, 255, 256, 1000, 8192,
    ] {
        let buffer = pseudo_random_bytes(0x1234_5678 ^ length as u32, length);
        assert_eq!(
            crc::crc16(&buffer),
            reference_crc16(&buffer),
            "length {length}"
        );
    }
}

#[test]
fn polynomial_and_initial_value_match_cpp() {
    // `crc16.c` uses 0x1021; `GameManager_CheckDesync` starts at 0xFFFF.
    assert_eq!(CRC16_POLYNOMIAL, 0x1021);
    assert_eq!(CRC16_INITIAL, 0xFFFF);
    // The CCITT polynomial is irreducible: a single high bit never returns to
    // the initial state within one byte step.
    for byte in 0..=u8::MAX {
        assert_ne!(crc::crc16_update_byte(CRC16_INITIAL, byte), CRC16_INITIAL);
    }
}

// ---------------------------------------------------------------------------
// Whole real files (reference bytes, not generated)
// ---------------------------------------------------------------------------

#[test]
fn real_save_files_hash_to_pinned_crc16() {
    // Values computed from the reference implementation over the exact
    // `max-original/SAVE1.CAM`/`SAVE2.CAM` bytes; pinned as a regression lock.
    let save1 = crc::crc16(SAVE1);
    let save2 = crc::crc16(SAVE2);
    assert_eq!(save1, reference_crc16(SAVE1));
    assert_eq!(save2, reference_crc16(SAVE2));
    assert_eq!((save1, save2), (SAVE1_CRC16, SAVE2_CRC16));
}

/// CRC16 of `SAVE1.CAM` (183,047 bytes) — reference lock.
const SAVE1_CRC16: u16 = 0xCF79;
/// CRC16 of `SAVE2.CAM` (216,410 bytes) — reference lock.
const SAVE2_CRC16: u16 = 0xB439;

#[test]
fn packet_45_crc_of_reference_saves_is_pinned() {
    let state1 = loaded_state(SAVE1);
    let state2 = loaded_state(SAVE2);

    // Packet 45 only covers units of teams that play; it is the desync barrier.
    assert_eq!(state1.snapshot().state_checksum(), SAVE1_CHECKSUM);
    assert_eq!(state2.snapshot().state_checksum(), SAVE2_CHECKSUM);
    // Units exist, so the value is not the initial one.
    assert_ne!(state1.snapshot().state_checksum(), CRC16_INITIAL);
    assert_ne!(state2.snapshot().state_checksum(), CRC16_INITIAL);
}

/// Packet 45 CRC of the SAVE1.CAM state — reference lock.
const SAVE1_CHECKSUM: u16 = 0x408A;
/// Packet 45 CRC of the SAVE2.CAM state — reference lock.
const SAVE2_CHECKSUM: u16 = 0x20EB;

#[test]
fn replay_digest_of_reference_saves_is_pinned() {
    let state1 = loaded_state(SAVE1);
    let state2 = loaded_state(SAVE2);

    let digest1 = state1.snapshot().replay_digest_bytes();
    let digest2 = state2.snapshot().replay_digest_bytes();

    assert_eq!(state1.snapshot().replay_digest_crc(), SAVE1_DIGEST_CRC);
    assert_eq!(state2.snapshot().replay_digest_crc(), SAVE2_DIGEST_CRC);
    assert_eq!(digest1.len(), SAVE1_DIGEST_LEN);
    assert_eq!(digest2.len(), SAVE2_DIGEST_LEN);
    assert_eq!(digest1.len() % 2, 0);
    assert_eq!(digest2.len() % 2, 0);

    // The trailer is the CRC of everything before it.
    assert_eq!(
        state1.snapshot().replay_digest_crc(),
        reference_crc16(&digest1[..digest1.len() - 2])
    );
    assert_eq!(
        state2.snapshot().replay_digest_crc(),
        reference_crc16(&digest2[..digest2.len() - 2])
    );
}

/// Replay digest CRC of the SAVE1.CAM state — reference lock.
const SAVE1_DIGEST_CRC: u16 = 0x5561;
/// Replay digest length of the SAVE1.CAM state — reference lock.
const SAVE1_DIGEST_LEN: usize = 30_504;
/// Replay digest CRC of the SAVE2.CAM state — reference lock.
const SAVE2_DIGEST_CRC: u16 = 0x4BC7;
/// Replay digest length of the SAVE2.CAM state — reference lock.
const SAVE2_DIGEST_LEN: usize = 28_236;

#[test]
fn state_digest_of_reference_saves_is_pinned() {
    let state1 = loaded_state(SAVE1);
    let state2 = loaded_state(SAVE2);

    assert_eq!(state1.snapshot().state_digest(), SAVE1_STATE_DIGEST);
    assert_eq!(state2.snapshot().state_digest(), SAVE2_STATE_DIGEST);
    assert_eq!(
        state1.snapshot().state_digest(),
        crc::crc16(&state1.snapshot().snapshot_bytes())
    );
}

/// `state_digest` of the SAVE1.CAM state — reference lock.
///
/// Re-pinned after the mission-AI load fix: `state_digest` covers the AI block,
/// and the V70 loader now reproduces `Ai_FileLoad` (`ai.cpp:164`) — `need_init`
/// stays set and the `Ai_Init` weight tables are filled
/// (`aiplayer.cpp:2951-3305`).
/// The packet-45 checksum and the replay digest deliberately do not include the
/// AI bookkeeping, so their locks above are unchanged.
const SAVE1_STATE_DIGEST: u16 = 0x1381;
/// `state_digest` of the SAVE2.CAM state — reference lock (see SAVE1 above).
const SAVE2_STATE_DIGEST: u16 = 0xF9A6;

// ---------------------------------------------------------------------------
// Digest layout (the C++ generator `tools/sim_vectors/main.cpp` layout)
// ---------------------------------------------------------------------------

#[test]
fn replay_digest_layout_is_stable() {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = max_core::team::TeamKind::Player;
    state.set_play_mode(PlayMode::SimultaneousMoves);
    state.set_turn_counter(0x0102_0304);
    state.set_active_turn_team(TeamId::GREEN);
    state.set_player_team(TeamId::BLUE);
    state.set_unit_id_counter(0x0506_0708);
    *state.rng_mut() = Randomizer::from_state([0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444]);

    let digest = state.snapshot().replay_digest_bytes();
    assert_eq!(&digest[..8], REPLAY_DIGEST_MAGIC);

    // SurfaceMap CRC, ResourceMap CRC, turn counter, 4 state bytes,
    // unit id counter, then 4 RNG words — all little-endian.
    assert_eq!(
        le_u16(&digest[8..10]),
        crc::crc16(&state.snapshot().map().surface.to_bytes())
    );
    assert_eq!(
        le_u16(&digest[10..12]),
        crc::crc16(&state.snapshot().map().resources.to_le_bytes())
    );
    assert_eq!(le_i32(&digest[12..16]), 0x0102_0304);
    assert_eq!(
        &digest[16..20],
        &[
            state.snapshot().game_state() as u8,
            TeamId::GREEN.0,
            TeamId::BLUE.0,
            PlayMode::SimultaneousMoves as u8,
        ]
    );
    assert_eq!(le_u32(&digest[20..24]), 0x0506_0708);
    for (index, word) in state.snapshot().rng().state().iter().enumerate() {
        assert_eq!(
            le_u32(&digest[24 + index * 4..28 + index * 4]),
            *word,
            "rng word {index}"
        );
    }
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le_i32(bytes: &[u8]) -> i32 {
    i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

// ---------------------------------------------------------------------------
// Sensitivity: every tracked field must move the digest
// ---------------------------------------------------------------------------

/// One mutation of the loaded SAVE1 state; the replay digest must change and the
/// packet 45 CRC must change only when a CRC-tracked unit field is touched.
struct Mutation {
    name: &'static str,
    apply: fn(&mut GameUniverse),
    packet_45_changes: bool,
}

fn mutations() -> Vec<Mutation> {
    vec![
        Mutation {
            name: "turn_counter",
            apply: |state| state.set_turn_counter(state.snapshot().turn_counter() + 1),
            packet_45_changes: false,
        },
        Mutation {
            name: "active_turn_team",
            apply: |state| state.set_active_turn_team(TeamId::GREEN),
            packet_45_changes: false,
        },
        Mutation {
            name: "player_team",
            apply: |state| state.set_player_team(TeamId::GREEN),
            packet_45_changes: false,
        },
        Mutation {
            name: "play_mode",
            apply: |state| state.set_play_mode(PlayMode::TurnBased),
            packet_45_changes: false,
        },
        Mutation {
            name: "unit_id_counter",
            apply: |state| {
                state.set_unit_id_counter(state.snapshot().unit_id_counter().wrapping_add(1000))
            },
            packet_45_changes: false,
        },
        Mutation {
            name: "rng_state",
            apply: |state| {
                let mut words = state.snapshot().rng().state();
                words[0] ^= 1;
                state.rng_mut().set_state(words);
            },
            packet_45_changes: false,
        },
        Mutation {
            name: "team_points",
            apply: |state| state.teams_mut()[0].team_points ^= 0x8000_0000,
            packet_45_changes: false,
        },
        Mutation {
            name: "team_gold",
            apply: |state| state.team_units_table_mut()[0].gold ^= 0xFFFF,
            packet_45_changes: false,
        },
        Mutation {
            name: "research_level",
            apply: |state| state.teams_mut()[0].research_topics[0].research_level ^= 1,
            packet_45_changes: false,
        },
        Mutation {
            name: "unit_counters",
            apply: |state| state.teams_mut()[0].unit_counters[0] ^= 1,
            packet_45_changes: false,
        },
        Mutation {
            name: "casualties",
            apply: |state| state.teams_mut()[0].casualties[0] ^= 1,
            packet_45_changes: false,
        },
        Mutation {
            name: "unit_hits",
            apply: |state| {
                let id = state.snapshot().unit_ids_in_order()[0];
                let mut unit = state.snapshot().unit(id).expect("unit").clone();
                unit.hits = unit.hits.wrapping_add(1);
                state.write_unit(id, unit);
            },
            packet_45_changes: true,
        },
    ]
}

#[test]
fn replay_digest_reacts_to_every_tracked_group() {
    let base = loaded_state(SAVE1);
    let reference = base.snapshot().replay_digest_bytes();

    for mutation in mutations() {
        let mut state = base.clone();
        (mutation.apply)(&mut state);
        assert_ne!(
            state.snapshot().replay_digest_bytes(),
            reference,
            "replay digest ignored `{}`",
            mutation.name
        );
    }
}

#[test]
fn packet_45_crc_reacts_only_to_unit_fields() {
    let base = loaded_state(SAVE1);
    let reference = base.snapshot().state_checksum();

    for mutation in mutations() {
        let mut state = base.clone();
        (mutation.apply)(&mut state);
        if mutation.packet_45_changes {
            assert_ne!(
                state.snapshot().state_checksum(),
                reference,
                "packet 45 CRC ignored `{}`",
                mutation.name
            );
        } else {
            assert_eq!(
                state.snapshot().state_checksum(),
                reference,
                "packet 45 CRC must not include `{}`",
                mutation.name
            );
        }
    }
}

#[test]
fn packet_45_crc_tracks_each_unit_list() {
    let mut state = loaded_state(SAVE1);
    let reference = state.snapshot().state_checksum();

    // Every list except `Particle` participates (`GameManager_CheckDesync`).
    for kind in [
        max_core::UnitListKind::GroundCover,
        max_core::UnitListKind::MobileLandSea,
        max_core::UnitListKind::Stationary,
        max_core::UnitListKind::MobileAir,
    ] {
        let mut changed = state.clone();
        let id = changed.snapshot().unit_list(kind)[0];
        let mut unit = changed.snapshot().unit(id).expect("unit").clone();
        unit.grid.x = unit.grid.x.wrapping_add(1);
        changed.write_unit(id, unit);
        assert_ne!(
            changed.snapshot().state_checksum(),
            reference,
            "list {kind:?} is not covered by packet 45 CRC"
        );
    }

    // Removing a unit changes the CRC.
    let removed = state
        .snapshot()
        .unit_list(max_core::UnitListKind::GroundCover)[0];
    state.destroy_unit(removed);
    assert_ne!(state.snapshot().state_checksum(), reference);
}

/// `GameManager_GetUnitListChecksum` skips units with `EXPLODING` and units
/// without an id: the packet 45 barrier must not depend on a unit that is
/// already being removed.
fn fresh_crc_state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.teams_mut()[0].team_type = max_core::team::TeamKind::Player;
    state.team_units_table_mut()[0] =
        max_core::team::TeamUnits::with_clan(max_core::enums::TeamClan::Random);
    state
}

#[test]
fn packet_45_crc_skips_exploding_units() {
    let mut state = fresh_crc_state();
    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("tank");
    let with_tank = state.snapshot().state_checksum();
    assert_ne!(with_tank, CRC16_INITIAL, "the tank must be counted");

    let mut exploding = state.clone();
    let mut unit = exploding.snapshot().unit(tank).expect("unit").clone();
    unit.flags |= max_core::enums::unit_flags::EXPLODING;
    exploding.write_unit(tank, unit);
    assert_eq!(
        exploding.snapshot().state_checksum(),
        CRC16_INITIAL,
        "an exploding unit must be skipped like C++"
    );

    // `NO_ID` units are skipped as well (C++ `if (unit->id != NO_ID)`).
    let mut no_id = state.clone();
    let mut unit = no_id.snapshot().unit(tank).expect("unit").clone();
    unit.id = max_core::UnitId::NO_ID;
    no_id.write_unit(tank, unit);
    assert_eq!(no_id.snapshot().state_checksum(), CRC16_INITIAL);
}

#[test]
fn digests_are_independent_of_clone_and_load_order() {
    let first = loaded_state(SAVE1);
    let second = loaded_state(SAVE1);
    let clone = first.clone();

    for state in [&second, &clone] {
        assert_eq!(
            state.snapshot().replay_digest_bytes(),
            first.snapshot().replay_digest_bytes()
        );
        assert_eq!(state.snapshot().state_checksum(), first.snapshot().state_checksum());
        assert_eq!(state.snapshot().state_digest(), first.snapshot().state_digest());
        assert_eq!(
            state.snapshot().snapshot_bytes(),
            first.snapshot().snapshot_bytes()
        );
    }
}

#[test]
fn state_checksum_ignores_particles_and_empty_teams() {
    let mut state = fresh_crc_state();
    let reference = state.snapshot().state_checksum();
    assert_eq!(reference, CRC16_INITIAL, "no units yet");

    // A missile lives in the `Particle` list, which `CheckDesync` does not
    // iterate — spawning/firing it must not desync the barrier.
    state
        .spawn_unit(UnitType::Rocket, TeamId::RED, Point::new(4, 4), 0)
        .expect("rocket");
    assert_eq!(state.snapshot().state_checksum(), reference);
    assert_eq!(
        state
            .snapshot()
            .unit(state.snapshot().unit_list(max_core::UnitListKind::Particle)[0])
            .map(|unit| unit.unit_type),
        Some(UnitType::Rocket)
    );

    // A live unit of a team that is not playing (`team_type == NONE`, not
    // `ALIEN`) is not enumerated by `CheckDesync` either.
    let mut state = fresh_crc_state();
    let green = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(5, 5), 0)
        .expect("green tank");
    assert_eq!(
        state.snapshot().state_checksum(),
        CRC16_INITIAL,
        "green is not playing"
    );
    state.teams_mut()[1].team_type = max_core::team::TeamKind::Computer;
    assert_ne!(
        state.snapshot().state_checksum(),
        CRC16_INITIAL,
        "green now plays"
    );
    let _ = green;

    // The `ALIEN` slot is always enumerated, even with `team_type == NONE`
    // (`team == PLAYER_TEAM_ALIEN` condition in `GameManager_CheckDesync`).
    let mut state = fresh_crc_state();
    state
        .spawn_unit(UnitType::AlienTank, TeamId::ALIEN, Point::new(6, 6), 0)
        .expect("alien tank");
    assert_ne!(state.snapshot().state_checksum(), CRC16_INITIAL);
}
