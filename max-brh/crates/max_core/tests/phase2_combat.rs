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
use max_core::enums::{Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::universe::GameUniverse;

use support::{fog_game, game, order_fire, run_until, set_hovering, spawn, ticks};

fn base_hits(state: &GameUniverse, id: max_core::UnitId) -> u16 {
    state
        .snapshot()
        .unit(id)
        .expect("unit")
        .base_values
        .get_attribute(max_core::ATTRIB_HITS) as u16
}

fn expected_damage(
    state: &GameUniverse,
    attacker: max_core::UnitId,
    target: max_core::UnitId,
) -> u16 {
    let damage = combat::get_attack_damage(
        state.snapshot().unit(attacker).expect("attacker"),
        state.snapshot().unit(target).expect("target"),
        0,
    );
    u16::try_from(damage).expect("positive damage")
}

fn water(state: &mut GameUniverse, x: i16) {
    state.map_mut().surface.set(x, 5, Surface::WATER);
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
        let ammo_before = state.snapshot().unit(attacker).unwrap().ammo;
        let shots_before = state.snapshot().unit(attacker).unwrap().shots;
        assert!(damage > 0, "{attacker_type:?} has attack");

        order_fire(&mut state, attacker, Point::new(9, 5));
        assert!(
            run_until(&mut state, 60, |state| {
                state
                    .snapshot()
                    .unit(target)
                    .is_some_and(|unit| unit.hits < hits_before)
            }),
            "{attacker_type:?} never damaged {target_type:?} ({:?})",
            state
                .snapshot()
                .unit(attacker)
                .map(|unit| (unit.orders, unit.state))
        );

        assert_eq!(
            state.snapshot().unit(target).unwrap().hits,
            hits_before - damage,
            "{attacker_type:?} → {target_type:?}"
        );
        assert_eq!(
            state.snapshot().unit(attacker).unwrap().ammo,
            ammo_before.wrapping_sub(1),
            "{attacker_type:?} ammo"
        );
        assert_eq!(
            state.snapshot().unit(attacker).unwrap().shots,
            shots_before.wrapping_sub(1),
            "{attacker_type:?} rounds"
        );
        assert_eq!(state.snapshot().unit(attacker).unwrap().live_attack, 0);
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
        if state.snapshot().unit(attacker).unwrap().is_air() {
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
                    .snapshot()
                    .unit(target)
                    .is_some_and(|unit| unit.hits < hits_before)
            }),
            "{attacker_type:?} missile never hit {target_type:?}: {:?}",
            state
                .snapshot()
                .unit_ids_in_order()
                .into_iter()
                .map(|id| {
                    let unit = state.snapshot().unit(id).unwrap();
                    (unit.unit_type, unit.orders, unit.state, unit.grid)
                })
                .collect::<Vec<_>>()
        );

        assert_eq!(
            state.snapshot().unit(target).unwrap().hits,
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
        state.snapshot().unit(launcher).unwrap(),
        state.snapshot().unit(center).unwrap(),
        0,
    ))
    .unwrap();
    let neighbor_damage = u16::try_from(combat::get_attack_damage(
        state.snapshot().unit(launcher).unwrap(),
        state.snapshot().unit(neighbor).unwrap(),
        2,
    ))
    .unwrap();
    assert_eq!(center_damage, 5);
    assert_eq!(neighbor_damage, 1);

    order_fire(&mut state, launcher, Point::new(10, 5));
    assert!(
        run_until(&mut state, 120, |state| {
            state
                .snapshot()
                .unit(center)
                .is_some_and(|unit| unit.hits < base_hits(state, center))
        }),
        "the area rocket never detonated"
    );

    // The explosion may damage the neighbor in the same tick or on the next one.
    run_until(&mut state, 4, |state| {
        state
            .snapshot()
            .unit(neighbor)
            .is_some_and(|unit| unit.hits < base_hits(state, neighbor))
    });

    assert_eq!(
        state.snapshot().unit(center).unwrap().hits,
        base_hits(&state, center) - center_damage
    );
    assert_eq!(
        state.snapshot().unit(neighbor).unwrap().hits,
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

    assert!(state.check_reaction(defender,
        attacker
    ));
    assert_eq!(
        state.snapshot().unit(defender).unwrap().orders,
        UnitOrder::Fire
    );
    assert_eq!(
        state.snapshot().unit(defender).unwrap().state,
        UnitOrderState::Init
    );

    assert!(
        run_until(&mut state, 40, |state| {
            state
                .snapshot()
                .unit(attacker)
                .is_some_and(|unit| unit.hits < base_hits(state, attacker))
        }),
        "reaction fire never resolved"
    );
    assert_eq!(
        state.snapshot().unit(attacker).unwrap().hits,
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
    assert!(!state
        .snapshot()
        .unit(hidden)
        .unwrap()
        .is_visible_to_team(TeamId::RED));

    state.update_visibility_status(false);
    assert!(!state
        .snapshot()
        .unit(hidden)
        .unwrap()
        .is_visible_to_team(TeamId::RED));
    assert!(!state.check_reaction(defender, hidden));
    assert_eq!(
        state.snapshot().unit(defender).unwrap().orders,
        UnitOrder::Sentry
    );

    state.spot_unit(hidden, TeamId::RED);
    assert!(state.check_reaction(defender, hidden));
    assert_eq!(
        state.snapshot().unit(defender).unwrap().orders,
        UnitOrder::Fire
    );
}

/// A combat round with several weapon classes is deterministic: identical
/// checksums and replay digests on two independent runs.
#[test]
fn combat_round_is_deterministic() {
    fn run() -> GameUniverse {
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
                .snapshot()
                .unit(target)
                .is_some_and(|unit| unit.hits < base_hits(state, target))
                && state
                    .snapshot()
                    .unit(cluster)
                    .is_some_and(|unit| unit.hits < base_hits(state, cluster))
        }));
        // Let the particles/explosions finish so the digest covers their removal.
        ticks(&mut state, 60);
        state
    }

    let first = run();
    let second = run();
    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes()
    );
    assert_eq!(
        first.snapshot().state_checksum(),
        second.snapshot().state_checksum()
    );
    assert_eq!(
        first.snapshot().replay_digest_bytes(),
        second.snapshot().replay_digest_bytes()
    );
    assert_ne!(
        first.snapshot().state_checksum(),
        game().snapshot().state_checksum()
    );
}
