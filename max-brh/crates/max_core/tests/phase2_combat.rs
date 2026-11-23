//! Phase 2 integration: `combat.rs` — all weapon classes, missiles, area fire,
//! reaction fire and fog, through the tick loop.
//!
//! Oracle: `max-cpp-port/src/unitinfo.cpp` (`Attack`, `AttackUnit`,
//! `PrepareFire`, `ProgressFire`, `GetAreaAttackTargets`), `units_manager.cpp`
//! (`GetAttackDamage`, `CheckReaction`, `AssessAttacks`).
//!
//! Damages are checked against `UnitsManager_GetAttackDamage`
//! (`max(1, attack - armor)`, area scaling, submarine half damage).

mod support;

use max_core::combat;
use max_core::crc;
use max_core::enums::{Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::Point;
use max_core::team::TeamId;

use support::{fog_game, game, order_fire, run_until, set_hovering, spawn, ticks};

fn base_hits(state: &GameState, id: max_core::UnitId) -> u16 {
    state
        .unit(id)
        .expect("unit")
        .base_values
        .get_attribute(max_core::ATTRIB_HITS) as u16
}

fn expected_damage(state: &GameState, attacker: max_core::UnitId, target: max_core::UnitId) -> u16 {
    let damage = combat::get_attack_damage(
        state.unit(attacker).expect("attacker"),
        state.unit(target).expect("target"),
        0,
    );
    u16::try_from(damage).expect("positive damage")
}

fn water(state: &mut GameState, x: i16) {
    state.map.surface.set(x, 5, Surface::WATER);
}

/// Direct-fire weapon classes: each `FIRE` order over ticks deals exactly
/// `attack - armor`, spends one ammo and one round.
#[test]
fn direct_fire_weapon_classes() {
    // (attacker, target, target is flying)
    let cases = [
        (UnitType::Tank, UnitType::Tank, false),
        (UnitType::AssaultGun, UnitType::Tank, false),
        (UnitType::GunTurret, UnitType::Tank, false),
        (UnitType::AntiAir, UnitType::Fighter, true),
        (UnitType::MobileAntiAir, UnitType::Bomber, true),
    ];

    for (attacker_type, target_type, flying) in cases {
        let mut state = game();
        let attacker = spawn(&mut state, attacker_type, TeamId::RED, Point::new(5, 5));
        let target = spawn(&mut state, target_type, TeamId::GREEN, Point::new(9, 5));
        if flying {
            set_hovering(&mut state, target, true);
        }

        let hits_before = base_hits(&state, target);
        let damage = expected_damage(&state, attacker, target);
        let ammo_before = state.unit(attacker).unwrap().ammo;
        let shots_before = state.unit(attacker).unwrap().shots;
        assert!(damage > 0, "{attacker_type:?} has attack");

        order_fire(&mut state, attacker, Point::new(9, 5));
        assert!(
            run_until(&mut state, 60, |state| {
                state
                    .unit(target)
                    .is_some_and(|unit| unit.hits < hits_before)
            }),
            "{attacker_type:?} never damaged {target_type:?} ({:?})",
            state.unit(attacker).map(|unit| (unit.orders, unit.state))
        );

        assert_eq!(
            state.unit(target).unwrap().hits,
            hits_before - damage,
            "{attacker_type:?} → {target_type:?}"
        );
        assert_eq!(
            state.unit(attacker).unwrap().ammo,
            ammo_before.wrapping_sub(1),
            "{attacker_type:?} ammo"
        );
        assert_eq!(
            state.unit(attacker).unwrap().shots,
            shots_before.wrapping_sub(1),
            "{attacker_type:?} rounds"
        );
        assert_eq!(state.unit(attacker).unwrap().live_attack, 0);
    }
}

/// Missile weapon classes: the projectile travels for several ticks, then the
/// impact applies the same damage formula (including the submarine half damage).
#[test]
fn missile_weapon_classes_impact_after_flight() {
    // (attacker, target, water target, flying target)
    let cases = [
        (UnitType::MissileCrawler, UnitType::Tank, false, false),
        (UnitType::RocketLauncher, UnitType::AssaultGun, false, false),
        (UnitType::Submarine, UnitType::Corvette, true, false),
        (UnitType::Bomber, UnitType::Submarine, true, false),
    ];

    for (attacker_type, target_type, on_water, flying) in cases {
        let mut state = game();
        if on_water {
            // A water lane: the submarine line-of-fire check needs water.
            for x in 0..=20 {
                water(&mut state, x);
            }
        }
        let attacker = spawn(&mut state, attacker_type, TeamId::RED, Point::new(5, 5));
        let target = spawn(&mut state, target_type, TeamId::GREEN, Point::new(10, 5));
        if state.unit(attacker).unwrap().is_air() {
            // Aircraft attack only while flying (`Access_IsValidAttackTarget`).
            set_hovering(&mut state, attacker, true);
        }
        if flying {
            set_hovering(&mut state, target, true);
        }

        let hits_before = base_hits(&state, target);
        let damage = expected_damage(&state, attacker, target);
        assert!(damage > 0);

        order_fire(&mut state, attacker, Point::new(10, 5));
        assert!(
            run_until(&mut state, 120, |state| {
                state
                    .unit(target)
                    .is_some_and(|unit| unit.hits < hits_before)
            }),
            "{attacker_type:?} missile never hit {target_type:?}: {:?}",
            state
                .unit_ids_in_order()
                .into_iter()
                .map(|id| {
                    let unit = state.unit(id).unwrap();
                    (unit.unit_type, unit.orders, unit.state, unit.grid)
                })
                .collect::<Vec<_>>()
        );

        assert_eq!(
            state.unit(target).unwrap().hits,
            hits_before - damage,
            "{attacker_type:?} → {target_type:?}"
        );
    }
}

/// Area attack: a rocket launcher hits a cluster; the center takes full damage
/// and the adjacent cell takes the `potential = 2` scaled damage (minimum 1).
#[test]
fn area_attack_scales_damage_with_distance() {
    let mut state = game();
    let launcher = spawn(
        &mut state,
        UnitType::RocketLauncher,
        TeamId::RED,
        Point::new(5, 5),
    );
    let center = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(10, 5));
    let neighbor = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(11, 5));

    let center_damage = u16::try_from(combat::get_attack_damage(
        state.unit(launcher).unwrap(),
        state.unit(center).unwrap(),
        0,
    ))
    .unwrap();
    let neighbor_damage = u16::try_from(combat::get_attack_damage(
        state.unit(launcher).unwrap(),
        state.unit(neighbor).unwrap(),
        2,
    ))
    .unwrap();
    assert_eq!(center_damage, 5);
    assert_eq!(neighbor_damage, 1);

    order_fire(&mut state, launcher, Point::new(10, 5));
    assert!(
        run_until(&mut state, 120, |state| {
            state
                .unit(center)
                .is_some_and(|unit| unit.hits < base_hits(state, center))
        }),
        "the area rocket never detonated"
    );

    // The explosion may damage the neighbor in the same tick or on the next one.
    run_until(&mut state, 4, |state| {
        state
            .unit(neighbor)
            .is_some_and(|unit| unit.hits < base_hits(state, neighbor))
    });

    assert_eq!(
        state.unit(center).unwrap().hits,
        base_hits(&state, center) - center_damage
    );
    assert_eq!(
        state.unit(neighbor).unwrap().hits,
        base_hits(&state, neighbor) - neighbor_damage
    );
}

/// Reaction fire: a sentry in `TURN_BASED` fires at a visible target after
/// `CheckReaction`; the whole reaction goes through the normal FIRE pipeline.
#[test]
fn sentry_reaction_fires_at_a_visible_target() {
    let mut state = game();
    let defender = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    let attacker = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(12, 10),
    );
    for id in [defender, attacker] {
        let unit = state.unit_mut(id).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    assert!(combat::check_reaction(&mut state, defender, attacker));
    assert_eq!(state.unit(defender).unwrap().orders, UnitOrder::Fire);
    assert_eq!(state.unit(defender).unwrap().state, UnitOrderState::Init);

    assert!(
        run_until(&mut state, 40, |state| {
            state
                .unit(attacker)
                .is_some_and(|unit| unit.hits < base_hits(state, attacker))
        }),
        "reaction fire never resolved"
    );
    assert_eq!(
        state.unit(attacker).unwrap().hits,
        base_hits(&state, attacker) - expected_damage(&state, defender, attacker)
    );
}

/// Fog blocks reaction fire until the target is spotted.
#[test]
fn fog_blocks_reaction_fire() {
    let mut state = fog_game();
    // ARTILLRY: range 8 (within reach at distance 6), scan 4 (fog still hides the cell).
    let defender = spawn(
        &mut state,
        UnitType::Artillery,
        TeamId::RED,
        Point::new(10, 10),
    );
    let hidden = spawn(
        &mut state,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(16, 10),
    );
    {
        let unit = state.unit_mut(defender).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    assert!(!state.unit(hidden).unwrap().is_visible_to_team(TeamId::RED));

    state.update_visibility_status(false);
    assert!(!state.unit(hidden).unwrap().is_visible_to_team(TeamId::RED));
    assert!(!combat::check_reaction(&mut state, defender, hidden));
    assert_eq!(state.unit(defender).unwrap().orders, UnitOrder::Sentry);

    combat::spot_unit(&mut state, hidden, TeamId::RED);
    assert!(combat::check_reaction(&mut state, defender, hidden));
    assert_eq!(state.unit(defender).unwrap().orders, UnitOrder::Fire);
}

/// A combat round with several weapon classes is deterministic: identical
/// checksums and replay digests on two independent runs.
#[test]
fn combat_round_is_deterministic() {
    fn run() -> GameState {
        let mut state = game();
        let tank = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
        let target = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(8, 5));
        let launcher = spawn(
            &mut state,
            UnitType::RocketLauncher,
            TeamId::RED,
            Point::new(5, 7),
        );
        let cluster = spawn(
            &mut state,
            UnitType::AssaultGun,
            TeamId::GREEN,
            Point::new(9, 7),
        );

        order_fire(&mut state, tank, Point::new(8, 5));
        order_fire(&mut state, launcher, Point::new(9, 7));

        assert!(run_until(&mut state, 120, |state| {
            state
                .unit(target)
                .is_some_and(|unit| unit.hits < base_hits(state, target))
                && state
                    .unit(cluster)
                    .is_some_and(|unit| unit.hits < base_hits(state, cluster))
        }));
        // Let the particles/explosions finish so the digest covers their removal.
        ticks(&mut state, 60);
        state
    }

    let first = run();
    let second = run();
    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(crc::state_checksum(&first), crc::state_checksum(&second));
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
    assert_ne!(crc::state_checksum(&first), crc::state_checksum(&game()));
}
