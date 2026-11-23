//! Phase 4 — deterministic RNG in the simulation (I + D layer).
//!
//! The unit vectors (`tools/rng_vectors`, C++ generated) already pin the
//! generator itself; here the generator is checked **as part of the game
//! state**:
//!
//! * V70 does not store the `Randomizer` state — the state is re-seeded from the
//!   header (`GameState::load_state`, intentional divergence from C++, see the
//!   `load_state` documentation); a save/load cycle therefore restarts the
//!   stream, and two loads of the same file produce the same sequence;
//! * the RNG state is part of the canonical replay digest at fixed offsets, so a
//!   different seed is a different digest;
//! * ticks on two independently loaded states are identical, and the seed
//!   chosen by the header is preserved by the save metadata.
//!
//! Oracles: `randomizer.cpp` (`Randomizer_SetSeed`, `Randomizer_Generate`),
//! `saveload.cpp:776` (C++ does **not** touch the RNG on load).

use max_core::crc::{replay_digest_bytes, state_digest};
use max_core::rng::{Randomizer, DEFAULT_SEED};
use max_core::save::SaveFile;
use max_core::GameState;

/// `max-original/SAVE1.CAM` — header `rng_seed = 0x32A61291`.
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM` — header `rng_seed = 0x329B8D8E`.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

fn loaded_state(source: &[u8]) -> GameState {
    GameState::from_save(&SaveFile::parse(source).expect("reference save parses"))
}

#[test]
fn load_reseeds_the_generator_from_the_header() {
    // `GameState::load_state`: `self.rng = Randomizer::new(save.header.rng_seed)`.
    let state1 = loaded_state(SAVE1);
    assert_eq!(state1.rng.state(), Randomizer::new(0x32A6_1291).state());

    let state2 = loaded_state(SAVE2);
    assert_eq!(state2.rng.state(), Randomizer::new(0x329B_8D8E).state());
    assert_ne!(state1.rng.state(), state2.rng.state());
}

#[test]
fn save_metadata_keeps_the_header_seed() {
    let state = loaded_state(SAVE1);
    assert_eq!(state.save_meta.rng_seed, 0x32A6_1291);

    // The seed survives the "state -> file -> state" cycle.
    let bytes = state.to_save_bytes().expect("writes");
    let reloaded = GameState::from_save(&SaveFile::parse(&bytes).expect("parses"));
    assert_eq!(reloaded.save_meta.rng_seed, 0x32A6_1291);
    assert_eq!(reloaded.rng.state(), state.rng.state());
}

#[test]
fn two_loads_produce_the_same_stream() {
    let mut first = loaded_state(SAVE1);
    let mut second = loaded_state(SAVE1);

    for index in 0..1000 {
        assert_eq!(
            first.rng.next_u32(),
            second.rng.next_u32(),
            "value {index} of the stream diverged"
        );
    }
}

#[test]
fn v70_does_not_store_the_generator_state() {
    // C++ `SaveLoad_LoadFormatV70` reads the seed only for the lobby and does not
    // restore the stream (`saveload.cpp:776`). A saved game therefore starts the
    // stream from the seed again — this is the documented port behavior.
    let mut state = loaded_state(SAVE1);
    for _ in 0..17 {
        let _ = state.rng.next_u32();
    }
    let advanced = state.rng.state();
    assert_ne!(advanced, Randomizer::new(0x32A6_1291).state());

    let bytes = state.to_save_bytes().expect("writes");
    let reloaded = GameState::from_save(&SaveFile::parse(&bytes).expect("parses"));
    assert_eq!(
        reloaded.rng.state(),
        Randomizer::new(0x32A6_1291).state(),
        "the stream must restart from the header seed"
    );
}

#[test]
fn seed_drives_the_replay_digest() {
    let mut state = loaded_state(SAVE1);
    let reference = replay_digest_bytes(&state);

    // The RNG state is 4 little-endian words after the unit-id counter.
    let mut reseeded = state.clone();
    reseeded.rng.set_seed(0x32A6_1292);
    let digest = replay_digest_bytes(&reseeded);
    assert_ne!(digest, reference, "a different seed must change the digest");
    assert_ne!(
        &digest[24..40],
        &reference[24..40],
        "the RNG words are at digest offsets 24..40"
    );

    // Restoring the exact state restores the digest.
    state.rng.set_state(reseeded.rng.state());
    assert_ne!(replay_digest_bytes(&state), reference);
    state.rng.set_seed(0x32A6_1291);
    assert_eq!(replay_digest_bytes(&state), reference);
}

#[test]
fn ticks_are_deterministic_with_the_loaded_seed() {
    let mut first = loaded_state(SAVE1);
    let mut second = loaded_state(SAVE1);

    for _ in 0..20 {
        first.tick();
        second.tick();
        assert_eq!(first.rng.state(), second.rng.state());
    }

    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(replay_digest_bytes(&first), replay_digest_bytes(&second));
    assert_eq!(state_digest(&first), state_digest(&second));
}

#[test]
fn a_new_game_uses_the_cpp_default_seed() {
    // `XoshiroCpp::DefaultSeed = 1234567890`; `Randomizer::default()`.
    assert_eq!(DEFAULT_SEED, 1_234_567_890);
    assert_eq!(
        Randomizer::default().state(),
        Randomizer::new(DEFAULT_SEED).state()
    );

    let state = GameState::new();
    assert_eq!(state.rng.state(), Randomizer::new(0).state());
}

#[test]
fn simulation_ranges_are_fully_covered() {
    // Ranges actually used by the port: 8 directions (`angle`), 6 damage
    // buckets, 3 resource levels. A modulo-biased generator would still pass the
    // bound check but not the coverage check.
    let mut rng = Randomizer::new(0x32A6_1291);
    for range in [2u32, 3, 6, 8] {
        let mut seen = vec![false; range as usize];
        for _ in 0..10_000 {
            let value = rng.generate(range);
            assert!(value < range);
            seen[value as usize] = true;
        }
        assert!(
            seen.iter().all(|hit| *hit),
            "range {range} did not reach every value"
        );
    }
}

#[test]
fn generator_state_round_trip_in_a_loaded_game() {
    // `Randomizer::from_state` is the save/restore route; the sequence after a
    // restore must continue, not restart.
    let state = loaded_state(SAVE1);
    let snapshot = state.rng.state();

    let mut restored = Randomizer::from_state(snapshot);
    let mut original = state.rng;
    for index in 0..64 {
        assert_eq!(
            restored.next_u32(),
            original.next_u32(),
            "value {index} after restore"
        );
    }
    assert_eq!(Randomizer::from_state(snapshot).state(), snapshot);
}
