//! Property tests for the combat invariants C1, C2, C3 and C6.
//!
//! These invariants are properties of a generated state (position, shot budget,
//! visibility, attack/armor values), so the suite generates cases and checks the
//! oracle guarantee instead of replaying one fixed engagement.
//!
//! The concrete C1/C2/C3/C4/C5/C7 scenarios live as `#[cfg(test)]` unit tests next
//! to the private combat functions (see `src/combat.rs`, `src/access.rs`,
//! `src/unit_order.rs`).
//!
//! Oracles are in `max-cpp-port/src` and are cited next to each test.

mod support;

use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::unit_values::{ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_RANGE};
use max_core::{get_attack_damage, UnitId, UnitInfo, ATTACK_DAMAGE_MIN};
use support::invariants::Lcg;
use support::{fog_game, game, spawn};

/// Invariant C1: reaction fire needs `shots > 0`, the target inside
/// `ATTRIB_RANGE`, a valid attack target and `ShouldAttack`; any missing
/// condition suppresses the reaction.
/// Oracle: `max-cpp-port/src/units_manager.cpp:4709-4735`
/// (`UnitsManager_CheckReaction`) and `:4623-4707` (`UnitsManager_ShouldAttack`).
#[test]
fn c1_reaction_requires_shots_range_and_a_valid_target() {
    // Boundary: the same in-range, visible target with 0/1/2 shots.
    for shots in 0..=2u8 {
        let mut state = game();
        let shooter = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(20, 20));
        let target = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(22, 20),
        );
        set_sentry(&mut state, shooter);
        state.unit_mut(shooter).expect("shooter").shots = shots;

        assert_eq!(
            state.check_reaction(shooter, target),
            shots > 0,
            "C1 boundary: an in-range target must react iff shots > 0 (shots {shots})"
        );
    }

    let mut rng = Lcg::new(0xC1_0001);

    for case in 0..48 {
        let mut state = game();
        let shooter = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(20, 20));
        let mut dx = rng.range(-8, 8);
        let dy = rng.range(-8, 8);
        if dx == 0 && dy == 0 {
            dx = 1;
        }
        let target = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(20 + dx as i16, 20 + dy as i16),
        );

        set_sentry(&mut state, shooter);
        let shots = rng.range(0, 2) as u8;
        state.unit_mut(shooter).expect("shooter").shots = shots;

        let shooter_unit = state.snapshot().unit(shooter).expect("shooter").clone();
        let target_unit = state.snapshot().unit(target).expect("target").clone();
        let range = shooter_unit.base_values.get_attribute(ATTRIB_RANGE);
        let in_range =
            state
                .snapshot()
                .is_within_attack_range(&shooter_unit, target_unit.grid, range);
        let valid = state
            .snapshot()
            .is_valid_attack_target(&shooter_unit, &target_unit);
        let expected = shots > 0 && in_range && valid;

        let reacted = state.check_reaction(shooter, target);
        assert_eq!(
            reacted, expected,
            "C1 case {case}: reaction {reacted}, expected {expected} \
             (shots {shots}, range {range}, target at ({dx},{dy}))"
        );
    }
}

/// Invariant C2: `ShouldAttack` rejects a target that is not visible to the
/// attacker's team, so an invisible enemy never triggers reaction fire.
/// Oracle: `max-cpp-port/src/units_manager.cpp:4643-4644`
/// (`unit2->IsVisibleToTeam(unit1->team)`).
#[test]
fn c2_invisible_target_does_not_trigger_reaction() {
    for offset in 5..=8i16 {
        let mut state = fog_game();
        // Missile crawler: range 10, scan 4. At distance `offset` the target is
        // inside the attack range for `offset <= 10`, but outside the scan.
        let shooter = spawn(
            &mut state,
            UnitType::MissileCrawler,
            TeamId::RED,
            Point::new(10, 10),
        );
        let target = spawn(
            &mut state,
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(10 + offset, 10),
        );
        set_sentry(&mut state, shooter);
        state.update_visibility_status(false);

        assert!(
            !state
                .snapshot()
                .unit(target)
                .expect("target")
                .is_visible_to_team(TeamId::RED),
            "C2: offset {offset}: the target must start invisible to RED"
        );
        assert!(
            !state.check_reaction(shooter, target),
            "C2: offset {offset}: an invisible target must not trigger reaction fire"
        );

        // Once reconnaissance reveals it, the same geometry does react.
        state.spot_unit(target, TeamId::RED);
        assert!(
            state
                .snapshot()
                .unit(target)
                .expect("target")
                .is_visible_to_team(TeamId::RED),
            "C2: offset {offset}: the target must be visible after SpotByTeam"
        );
        assert!(
            state.check_reaction(shooter, target),
            "C2: offset {offset}: a visible in-range target must trigger reaction fire"
        );
    }
}

/// Invariant C3: a `SUBMARNE` (always submerged) is invisible to an ordinary
/// scan; only sea-stealth coverage (`CORVETTE`) reveals it.
/// Oracle: `max-cpp-port/src/unitinfo.cpp:2872-2884` (`UnitInfo::DrawStealth`),
/// `access.cpp:691-724` (`Access_DrawUnit`, the sea-stealth branch) and
/// `units_manager.cpp:1994-1998` (`IsUnitUnderWater`).
#[test]
fn c3_submarine_is_hidden_from_an_ordinary_scan() {
    let offsets = [(1, 0), (2, 0), (2, 1), (3, 0), (1, 2), (2, 2)];

    for (dx, dy) in offsets {
        let mut state = fog_game();
        let _scanner = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
        let submarine = spawn(
            &mut state,
            UnitType::Submarine,
            TeamId::GREEN,
            Point::new(10 + dx, 10 + dy),
        );

        state.update_visibility_status(false);

        assert!(
            state.snapshot().ai_heat_map_complete(
                TeamId::RED,
                10 + i32::from(dx),
                10 + i32::from(dy)
            ) > 0,
            "C3: offset ({dx},{dy}): the ordinary scan must cover the submarine cell"
        );
        assert_eq!(
            state.snapshot().ai_heat_map_stealth_sea(
                TeamId::RED,
                10 + i32::from(dx),
                10 + i32::from(dy)
            ),
            0,
            "C3: offset ({dx},{dy}): no corvette covers the submarine cell"
        );
        assert!(
            !state
                .snapshot()
                .unit(submarine)
                .expect("submarine")
                .is_visible_to_team(TeamId::RED),
            "C3: offset ({dx},{dy}): a submarine must stay hidden from an ordinary scan"
        );
        assert!(
            state
                .snapshot()
                .unit(submarine)
                .expect("submarine")
                .is_visible_to_team(TeamId::GREEN),
            "C3: offset ({dx},{dy}): the owner always sees its own submarine"
        );
    }
}

/// Invariant C3 (sea-stealth layer): `CORVETTE` is the only source of
/// `GetStealthSea` coverage; its presence reveals the same submarine that the
/// ordinary scan failed to reveal.
/// Oracle: `max-cpp-port/src/access.cpp:609-621` (`Access_DrawUnit`) and
/// `unitinfo.cpp:2877-2880` (`DrawStealth`).
#[test]
fn c3_corvette_sea_stealth_reveals_the_submarine() {
    let mut state = fog_game();
    let _scanner = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    let submarine = spawn(
        &mut state,
        UnitType::Submarine,
        TeamId::GREEN,
        Point::new(12, 10),
    );

    state.update_visibility_status(false);
    assert!(!state
        .snapshot()
        .unit(submarine)
        .expect("submarine")
        .is_visible_to_team(TeamId::RED));

    let _corvette = spawn(
        &mut state,
        UnitType::Corvette,
        TeamId::RED,
        Point::new(14, 10),
    );
    state.update_visibility_status(false);

    assert!(
        state
            .snapshot()
            .ai_heat_map_stealth_sea(TeamId::RED, 12, 10)
            > 0,
        "C3: the corvette must feed the sea-stealth layer"
    );
    assert!(
        state
            .snapshot()
            .unit(submarine)
            .expect("submarine")
            .is_visible_to_team(TeamId::RED),
        "C3: sea-stealth coverage must reveal the submarine"
    );
    assert_eq!(
        state
            .snapshot()
            .unit(submarine)
            .expect("submarine")
            .image_base,
        8,
        "C3: a spotted submarine is surfaced (`image_base = 8`, unitinfo.cpp:2825)"
    );
}

/// Invariant C6: `damage = max(1, attack - armor)` (with the oracle's area and
/// submarine modifiers); armor can lower damage to one but never to zero.
/// Oracle: `max-cpp-port/src/units_manager.cpp:4958-4985`
/// (`UnitsManager_GetAttackDamage`).
#[test]
fn c6_damage_is_never_below_one() {
    let mut rng = Lcg::new(0xC6_0006);
    let attacker_types = [UnitType::Tank, UnitType::Bomber, UnitType::Submarine];
    let target_types = [UnitType::Tank, UnitType::Submarine];

    for case in 0..128 {
        let attack = rng.range(0, 40);
        let armor = rng.range(0, 40);
        let potential = rng.range(0, 5);
        let attacker_type = rng.pick(&attacker_types);
        let target_type = rng.pick(&target_types);

        let attacker = unit_with_attack(attacker_type, attack, UnitId(1));
        let target = unit_with_attack(target_type, 0, UnitId(2));
        let target = with_armor(target, armor);

        let mut scaled = if potential > 1 {
            ((5 - potential) * attack) / 4
        } else {
            attack
        };
        if target_type == UnitType::Submarine
            && (attacker_type == UnitType::Bomber || attacker_type == UnitType::AlienPlane)
        {
            scaled /= 2;
        }
        let expected = if armor >= scaled { 1 } else { scaled - armor };

        let damage = get_attack_damage(&attacker, &target, potential);
        assert!(
            damage >= ATTACK_DAMAGE_MIN,
            "C6 case {case}: damage {damage} < minimum {} (attack {attack}, armor {armor}, \
             potential {potential})",
            ATTACK_DAMAGE_MIN
        );
        assert_eq!(
            damage, expected,
            "C6 case {case}: damage {damage} != max(1, attack−armor) {expected} \
             (attack {attack}, armor {armor}, potential {potential}, \
             {attacker_type:?} -> {target_type:?})"
        );
    }
}

fn set_sentry(state: &mut max_core::universe::GameUniverse, id: UnitId) {
    state.set_order(id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
}

fn unit_with_attack(unit_type: UnitType, attack: i32, id: UnitId) -> UnitInfo {
    let mut unit = UnitInfo::new(unit_type, TeamId::RED, id);
    unit.base_values.set_attribute(ATTRIB_ATTACK, attack);
    unit
}

fn with_armor(mut unit: UnitInfo, armor: i32) -> UnitInfo {
    unit.base_values.set_attribute(ATTRIB_ARMOR, armor);
    unit
}
