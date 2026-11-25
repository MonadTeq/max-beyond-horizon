use super::*;
use crate::enums::unit_flags::{ELECTRONIC_UNIT, MOBILE_LAND_UNIT, SELECTABLE, STATIONARY};
use crate::enums::TeamClan;
use crate::team::TeamKind;
use crate::unit_values::{ATTRIB_ARMOR, ATTRIB_SPEED, ATTRIB_TURNS};

fn state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn_tank(state: &mut GameState, team: TeamId, grid: Point) -> UnitId {
    state
        .spawn_unit(
            UnitType::Tank,
            team,
            grid,
            MOBILE_LAND_UNIT | SELECTABLE | crate::enums::unit_flags::UPGRADABLE,
        )
        .unwrap()
}

#[test]
fn damage_formula_matches_cpp() {
    // TANK: attack 16, TANK armor 10 → 6 damage.
    let attacker = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    let target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));
    assert_eq!(
        attacker.base_values.get_attribute(ATTRIB_ATTACK),
        16,
        "атрибуты танка из таблицы SC_ATTRI"
    );
    assert_eq!(target.base_values.get_attribute(ATTRIB_ARMOR), 10);
    assert_eq!(get_attack_damage(&attacker, &target, 0), 6);

    // Armor >= attack: minimum damage 1.
    let mut weak = attacker.clone();
    weak.base_values.set_attribute(ATTRIB_ATTACK, 4);
    assert_eq!(get_attack_damage(&weak, &target, 0), ATTACK_DAMAGE_MIN);

    // Area attack: attack * (5 - potential) / 4.
    assert_eq!(get_attack_damage(&attacker, &target, 2), 2); // 16*3/4=12 → 2
    assert_eq!(get_attack_damage(&attacker, &target, 3), 1); // 16*2/4=8 → 1
    assert_eq!(get_attack_damage(&attacker, &target, 5), 1); // 0 → 1

    // Submarine: a bomber hits at half strength.
    let bomber = UnitInfo::new(UnitType::Bomber, TeamId::RED, UnitId(3));
    let submarine = UnitInfo::new(UnitType::Submarine, TeamId::GREEN, UnitId(4));
    assert_eq!(get_attack_damage(&bomber, &submarine, 0), 7);
}

#[test]
fn attack_unit_applies_damage_and_explodes_target() {
    let mut state = state();
    let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
    let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(6, 5));
    // The target is visible to the attacker (in game — after scanning/reconnaissance).
    state.update_visibility_status(false);

    // Live attack pin: the target gets pin_count.
    let mut data = state.order_data(red);
    data.fire_on = Point::new(6, 5);
    state.set_order_data(red, data);
    state.pin_attack_targets(red);
    assert_eq!(state.unit(green).unwrap().pin_count, 1);
    assert!(state.live_attacks[0].targets.contains(&green));

    // Damage 6 with hits 5 — the target is destroyed.
    state.end_attack(red);
    state.unit_mut(green).unwrap().hits = 5;
    state.pending_attacks.push(red);
    attack_unit(&mut state, green, red, 0, 2);

    let target = state.unit(green).unwrap();
    assert_eq!(target.hits, 0);
    assert!(target.damaged_this_turn);
    assert_eq!(target.orders, UnitOrder::Explode);
    assert_eq!(target.state, UnitOrderState::Init);
    assert!(!state.pending_attacks.contains(&green));
    assert_eq!(target.pin_count, 0);
}

#[test]
fn area_attack_hits_all_targets_in_radius() {
    let mut state = state();
    let artillery = state
        .spawn_unit(
            UnitType::RocketLauncher,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    let near = spawn_tank(&mut state, TeamId::GREEN, Point::new(11, 10));
    let far = spawn_tank(&mut state, TeamId::GREEN, Point::new(12, 10));
    let outside = spawn_tank(&mut state, TeamId::GREEN, Point::new(14, 10));

    let attacker = state.unit(artillery).unwrap().clone();
    let targets = get_area_attack_targets(&state, &attacker, Point::new(10, 10));
    assert!(targets.iter().any(|(id, _)| *id == near));
    assert!(targets.iter().any(|(id, _)| *id == far));
    assert!(!targets.iter().any(|(id, _)| *id == outside));
    // Target uniqueness.
    let unique: std::collections::BTreeSet<_> = targets.iter().map(|(id, _)| *id).collect();
    assert_eq!(unique.len(), targets.len());

    attack_area_targets(&mut state, artillery, Point::new(10, 10));
    assert_eq!(state.unit(near).unwrap().hits, 23);
    assert_eq!(state.unit(far).unwrap().hits, 23);
    assert_eq!(state.unit(outside).unwrap().hits, 24);
}

#[test]
fn fire_order_runs_aim_prepare_and_damage_pipeline() {
    let mut state = state();
    let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
    let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(8, 5));

    let mut data = state.order_data(red);
    data.fire_on = Point::new(8, 5);
    data.move_to = Point::new(8, 5);
    state.set_order_data(red, data);
    state.set_order(red, UnitOrder::Fire, UnitOrderState::Init);

    // AIM (2 turns) + recoil (3 frames) + shot — on the 7th tick.
    for _ in 0..7 {
        state.tick();
    }

    let target = state.unit(green).unwrap();
    assert_eq!(target.hits, 18, "TANK 16 - armor 10 = 6 урона");
    let shooter = state.unit(red).unwrap();
    assert_eq!(shooter.ammo, 13, "--ammo");
    assert_eq!(shooter.shots, 1, "--shots");
    assert_eq!(shooter.live_attack, 0, "EndAttack после выстрела");
    assert_eq!(target.pin_count, 0);
}

#[test]
fn missile_prepare_fire_transfers_live_attack_to_particle() {
    let mut state = state();
    let launcher = state
        .spawn_unit(
            UnitType::MissileCrawler,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    let mut data = state.order_data(launcher);
    data.fire_on = Point::new(9, 9);
    state.set_order_data(launcher, data);

    let mut unit = state.unit(launcher).unwrap().clone();
    unit.live_attack = 7;
    prepare_fire(&mut state, &mut unit);
    assert_eq!(
        unit.ammo,
        unit.base_values
            .get_attribute(crate::unit_values::ATTRIB_AMMO) as u8
            - 1
    );
    assert_eq!(unit.state, UnitOrderState::FireInProgress);
    assert_eq!(unit.live_attack, 0);

    // The projectile — in the particle list, with target and parent.
    let particle = state
        .unit_list(UnitListKind::Particle)
        .iter()
        .copied()
        .find(|id| {
            state
                .unit(*id)
                .map(|unit| unit.parent_unit == Some(launcher))
                .unwrap_or(false)
        })
        .expect("particle");
    let particle_unit = state.unit(particle).unwrap();
    assert_eq!(particle_unit.live_attack, 7);
    assert_eq!(state.order_data(particle).fire_on, Point::new(9, 9));
}

/// `UnitInfo::PrepareFire` (`unitinfo.cpp:4749-4788`): the projectile carries the
/// **16-direction** firing angle (`UnitsManager_GetFiringAngle`,
/// `units_manager.cpp:1767`) — `D_ROCKET`/`D_TORPEDO` have 16 frames, `D_ALNMISSL`
/// 32. Masking it to the 8 hull directions (`angle & 0x07`) drew a missile fired
/// south (8) with the north frame: "missiles fly backwards".
#[test]
fn a_projectile_keeps_the_sixteen_direction_firing_angle() {
    // (dx, dy, expected firing angle) — the four cardinal directions and a diagonal.
    for (dx, dy, expected_angle) in [(4, 0, 4), (0, 4, 8), (-4, 0, 12), (0, -4, 0), (4, 4, 6)] {
        let mut state = state();
        let launcher = state
            .spawn_unit(
                UnitType::MissileCrawler,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        let fire_on = Point::new(10 + dx, 10 + dy);

        let mut data = state.order_data(launcher);
        data.fire_on = fire_on;
        state.set_order_data(launcher, data);

        let mut unit = state.unit(launcher).unwrap().clone();
        prepare_fire(&mut state, &mut unit);

        let particle = state
            .unit_list(UnitListKind::Particle)
            .iter()
            .copied()
            .find(|id| {
                state
                    .unit(*id)
                    .map(|unit| unit.parent_unit == Some(launcher))
                    .unwrap_or(false)
            })
            .expect("particle");
        let particle_unit = state.unit(particle).unwrap();

        assert_eq!(
            i32::from(particle_unit.angle),
            expected_angle,
            "снаряд в {fire_on:?}: направление выстрела, не маскированное до 8"
        );
    }

    // `ALNMISSL` doubles the firing angle (`unitinfo.cpp:4783-4785`, 32 frames).
    let mut state = state();
    let plane = state
        .spawn_unit(
            UnitType::AlienPlane,
            TeamId::GREEN,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    let mut data = state.order_data(plane);
    data.fire_on = Point::new(10, 14);
    state.set_order_data(plane, data);

    let mut unit = state.unit(plane).unwrap().clone();
    prepare_fire(&mut state, &mut unit);

    let particle = state
        .unit_list(UnitListKind::Particle)
        .iter()
        .copied()
        .find(|id| {
            state
                .unit(*id)
                .map(|unit| unit.parent_unit == Some(plane))
                .unwrap_or(false)
        })
        .expect("particle");
    let particle_unit = state.unit(particle).unwrap();
    assert_eq!(particle_unit.unit_type, UnitType::AlienMissile);
    assert_eq!(
        i32::from(particle_unit.angle),
        16,
        "ALNMISSL: угол выстрела ×2 (8 → 16), кадров в D_ALNMISSL — 32"
    );
}

#[test]
fn stealth_action_uses_dice_roll_and_reveals_on_failure() {
    let mut state = state();
    let tower = state
        .spawn_unit(
            UnitType::GoldRefinery,
            TeamId::GREEN,
            Point::new(5, 5),
            STATIONARY | ELECTRONIC_UNIT | crate::enums::unit_flags::BUILDING,
        )
        .unwrap();
    {
        let unit = state.unit_mut(tower).unwrap();
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    let commando = state
        .spawn_unit(
            UnitType::Infiltrator,
            TeamId::RED,
            Point::new(5, 6),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    {
        let unit = state.unit_mut(commando).unwrap();
        unit.parent_unit = Some(tower);
        unit.orders = UnitOrder::AwaitDisableUnit;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.stealth_dice_roll = 0;
    }

    let mut infiltrator = state.unit(commando).unwrap().clone();
    let chance = get_stealth_chance_percentage(
        &state,
        &infiltrator,
        state.unit(tower).unwrap(),
        infiltrator.orders,
    );
    assert!(chance > 0);
    let experience = infiltrator.experience;
    assert!(attempt_stealth_action(&mut state, &mut infiltrator));
    assert_eq!(infiltrator.experience, experience + 1);
    // `AttemptStealthAction` deducts `shots` (not `ammo`).
    assert_eq!(infiltrator.shots, 0);

    // Failure on a high roll: the saboteur is revealed.
    infiltrator.stealth_dice_roll = 100;
    assert!(!attempt_stealth_action(&mut state, &mut infiltrator));
    assert!(infiltrator.is_visible_to_team(TeamId::GREEN));
}

#[test]
fn reaction_fire_starts_fire_order() {
    let mut state = state();
    state.play_mode = PlayMode::TurnBased;
    let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
    let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(7, 5));
    state.update_visibility_status(false);
    {
        let unit = state.unit_mut(red).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    {
        let unit = state.unit_mut(green).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    assert!(check_reaction(&mut state, red, green));
    let shooter = state.unit(red).unwrap();
    assert_eq!(shooter.orders, UnitOrder::Fire);
    assert_eq!(shooter.state, UnitOrderState::Init);
    assert_eq!(state.order_data(red).fire_on, Point::new(7, 5));
    assert!(state.units_active[0].is_none());

    // `AddToDelayedReactionList` + `AssessAttacks` trigger a reaction in
    // the defending team (a green tank within the red one's radius).
    state.unit_mut(red).unwrap().orders = UnitOrder::Sentry;
    state.unit_mut(green).unwrap().orders = UnitOrder::Sentry;
    state.add_to_delayed_reaction_list(red);
    let triggered = assess_attacks(&mut state);
    assert!(triggered);
    assert_eq!(state.unit(green).unwrap().orders, UnitOrder::Fire);
}

#[test]
fn seeded_stealth_rolls_are_reproducible() {
    let mut first = state();
    let mut second = state();
    first.rng.set_seed(0xDEAD_BEEF);
    second.rng.set_seed(0xDEAD_BEEF);
    for _ in 0..64 {
        let a = roll_stealth_dice(&mut first);
        let b = roll_stealth_dice(&mut second);
        assert_eq!(a, b);
        assert!(a < 101);
    }
    assert_eq!(first.rng.state(), second.rng.state());
}

#[test]
fn angle_helpers_match_cpp_tables() {
    assert_eq!(get_target_angle(1, 0), 2);
    assert_eq!(get_target_angle(0, 1), 4);
    assert_eq!(get_target_angle(-1, 0), 6);
    assert_eq!(get_target_angle(0, -1), 0);
    assert_eq!(get_target_angle(1, 1), 3);
    assert_eq!(get_target_angle(-1, -1), 7);
    assert_eq!(get_firing_angle(1, 0), 4);
    assert_eq!(get_firing_angle(0, 1), 8);
    assert_eq!(get_firing_angle(0, -1), 0);
    assert_eq!(get_firing_angle(1, 1), 6);
}

#[test]
fn zone_walker_visits_circle_row_major() {
    let cells = zone_cells(Point::new(5, 5), 1);
    // A circle of radius 1 — 3×3 without corners? C++ checks distance² <= range²,
    // so corners (dx=1,dy=1 → 2 > 1) are excluded: 5 cells.
    assert_eq!(cells.len(), 5);
    assert!(cells.contains(&Point::new(5, 5)));
    assert!(!cells.contains(&Point::new(6, 6)));
    // Order: y ascending, within a row x ascending.
    let mut sorted = cells.clone();
    sorted.sort_by_key(|p| (p.y, p.x));
    assert_eq!(cells, sorted);
}

#[test]
fn capture_and_disable_change_target_state() {
    let mut state = state();
    let tower = state
        .spawn_unit(
            UnitType::GoldRefinery,
            TeamId::GREEN,
            Point::new(5, 5),
            STATIONARY | ELECTRONIC_UNIT | crate::enums::unit_flags::BUILDING,
        )
        .unwrap();
    {
        let unit = state.unit_mut(tower).unwrap();
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    let commando = state
        .spawn_unit(
            UnitType::Infiltrator,
            TeamId::RED,
            Point::new(5, 6),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();

    capture_unit(&mut state, commando, tower);
    let captured = state.unit(tower).unwrap();
    assert_eq!(captured.team, TeamId::RED);
    assert!(captured.is_visible_to_team(TeamId::RED));
    assert_eq!(captured.parent_unit, None);

    // Disable: the target receives ORDER_DISABLE for 1+ turn.
    let target = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(8, 8),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    disable_unit(&mut state, commando, target);
    let disabled = state.unit(target).unwrap();
    assert_eq!(disabled.orders, UnitOrder::Disable);
    assert!(disabled.disabled_turns_remaining >= 1);
    assert_eq!(disabled.path, None);
}

#[test]
fn damage_is_deterministic_between_runs() {
    fn run() -> (Vec<u8>, i32) {
        let mut state = state();
        let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
        let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(8, 5));
        let mut data = state.order_data(red);
        data.fire_on = Point::new(8, 5);
        state.set_order_data(red, data);
        state.set_order(red, UnitOrder::Fire, UnitOrderState::Init);
        for _ in 0..12 {
            state.tick();
        }
        (
            state.snapshot_bytes(),
            i32::from(state.unit(green).unwrap().hits),
        )
    }

    let first = run();
    let second = run();
    assert_eq!(first, second);
    assert!(first.1 < 24);
}

#[test]
fn disable_turns_follows_cpp_formula() {
    let mut infiltrator = UnitInfo::new(UnitType::Infiltrator, TeamId::RED, UnitId(1));
    infiltrator.experience = 4;
    let mut target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));
    target.speed = target.base_values.get_attribute(ATTRIB_SPEED) as u16;
    // turns(TANK)=4, speed=6: ((6*(4+8)) - (4*6)) / (4*6) = (72-24)/24 = 2.
    assert_eq!(access::disable_turns(&infiltrator, &target), 2);
    assert!(target.base_values.get_attribute(ATTRIB_TURNS) > 0);
}

// -----------------------------------------------------------------------
// Weapon classes, ranges, fog and experience boundaries
// -----------------------------------------------------------------------

/// `UnitsManager_GetAttackDamage` for every weapon class against the same
/// armored target: `max(1, attack - armor)` — armor, attacks and minimum
/// damage are taken from `SC_ATTRI`.
#[test]
fn damage_covers_all_weapon_classes() {
    let target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));
    let armor = target.base_values.get_attribute(ATTRIB_ARMOR);
    assert_eq!(armor, 10);

    for (unit_type, attack) in [
        (UnitType::Scout, 12),
        (UnitType::Tank, 16),
        (UnitType::AssaultGun, 18),
        (UnitType::RocketLauncher, 15),
        (UnitType::MissileCrawler, 22),
        (UnitType::GunTurret, 16),
        (UnitType::AntiAir, 16),
        (UnitType::MobileAntiAir, 22),
        (UnitType::Artillery, 18),
        (UnitType::Bomber, 18),
        (UnitType::Submarine, 20),
        (UnitType::AlienTank, 20),
        (UnitType::Infantry, 16),
        (UnitType::Fighter, 16),
    ] {
        let attacker = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
        assert_eq!(
            attacker.base_values.get_attribute(ATTRIB_ATTACK),
            attack,
            "{unit_type:?} attack from SC_ATTRI"
        );
        assert_eq!(
            get_attack_damage(&attacker, &target, 0),
            attack - armor,
            "{unit_type:?}"
        );
    }

    // Below armor — the minimum is one point of damage.
    let weak = UnitInfo::new(UnitType::Infiltrator, TeamId::RED, UnitId(3));
    assert_eq!(weak.base_values.get_attribute(ATTRIB_ATTACK), 6);
    assert_eq!(get_attack_damage(&weak, &target, 0), ATTACK_DAMAGE_MIN);
    let unarmed = UnitInfo::new(UnitType::DeadWaldo, TeamId::RED, UnitId(4));
    assert_eq!(get_attack_damage(&unarmed, &target, 0), ATTACK_DAMAGE_MIN);
}

/// The bomber/alien-plane half-damage rule applies only to submarines, and
/// only to those two attacker types (`UnitsManager_GetAttackDamage`).
#[test]
fn submarine_half_damage_rule_boundaries() {
    let submarine = UnitInfo::new(UnitType::Submarine, TeamId::GREEN, UnitId(2));
    let bomber = UnitInfo::new(UnitType::Bomber, TeamId::RED, UnitId(1));
    let alien_plane = UnitInfo::new(UnitType::AlienPlane, TeamId::RED, UnitId(3));
    let tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(4));

    // Submarine armor 2: 18/2 - 2 = 7, 25/2 - 2 = 10.
    assert_eq!(get_attack_damage(&bomber, &submarine, 0), 7);
    assert_eq!(get_attack_damage(&alien_plane, &submarine, 0), 10);
    // A tank-like attacker is halved nowhere near the rule.
    assert_eq!(get_attack_damage(&tank, &submarine, 0), 14);
    // The same half-damage rule does not apply to non-submarine targets.
    let tank_target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(5));
    assert_eq!(get_attack_damage(&bomber, &tank_target, 0), 8);
}

/// Area-attack scaling `attack * (5 - potential) / 4` is applied only for
/// `potential > 1`; below armor the result is still the minimum.
#[test]
fn area_attack_potential_boundaries() {
    let attacker = UnitInfo::new(UnitType::RocketLauncher, TeamId::RED, UnitId(1));
    let target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));

    // RocketLauncher attack 15, armor 10.
    assert_eq!(get_attack_damage(&attacker, &target, 0), 5);
    assert_eq!(
        get_attack_damage(&attacker, &target, 1),
        5,
        "1 is not scaled"
    );
    assert_eq!(get_attack_damage(&attacker, &target, 2), 1, "15*3/4=11");
    assert_eq!(get_attack_damage(&attacker, &target, 3), 1, "15*2/4=7");
    assert_eq!(get_attack_damage(&attacker, &target, 4), 1, "15*1/4=3");
    assert_eq!(get_attack_damage(&attacker, &target, 5), 1, "0");
    assert_eq!(
        get_attack_damage(&attacker, &target, 6),
        ATTACK_DAMAGE_MIN,
        "negative scaled attack"
    );
    // Negative potential is not an area distance — left unscaled.
    assert_eq!(get_attack_damage(&attacker, &target, -1), 5);
}

/// `UnitsManager_ShouldAttack`: AA weapons reject ground targets, dead
/// targets are ignored, idle/disabled units do not react.
#[test]
fn should_attack_restrictions() {
    let mut state = state();
    state.ai.all_visible = true;
    state.play_mode = PlayMode::TurnBased;

    let anti_air = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
    {
        let unit = state.unit_mut(anti_air).unwrap();
        unit.unit_type = UnitType::AntiAir;
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    let ground = spawn_tank(&mut state, TeamId::GREEN, Point::new(6, 5));
    let air = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::GREEN,
            Point::new(6, 6),
            crate::enums::unit_flags::MOBILE_AIR_UNIT | SELECTABLE,
        )
        .unwrap();

    assert!(!should_attack(
        &state,
        state.unit(anti_air).unwrap(),
        state.unit(ground).unwrap()
    ));
    assert!(should_attack(
        &state,
        state.unit(anti_air).unwrap(),
        state.unit(air).unwrap()
    ));

    // Dead targets are never attacked.
    state.unit_mut(air).unwrap().hits = 0;
    assert!(!should_attack(
        &state,
        state.unit(anti_air).unwrap(),
        state.unit(air).unwrap()
    ));

    // Idle, disabled, exploding and mid-move attackers do not react.
    for (order, order_state) in [
        (UnitOrder::Idle, UnitOrderState::ExecutingOrder),
        (UnitOrder::Disable, UnitOrderState::ExecutingOrder),
        (UnitOrder::Move, UnitOrderState::InProgress),
        (UnitOrder::MoveToAttack, UnitOrderState::InProgress),
        (UnitOrder::Move, UnitOrderState::NewOrder),
    ] {
        let unit = state.unit_mut(anti_air).unwrap();
        unit.orders = order;
        unit.state = order_state;
        let target = state.unit(ground).unwrap();
        assert!(
            !should_attack(&state, state.unit(anti_air).unwrap(), target),
            "{order:?}/{order_state:?} must not react"
        );
    }
    state.unit_mut(anti_air).unwrap().orders = UnitOrder::Sentry;
    state.unit_mut(anti_air).unwrap().state = UnitOrderState::Explode;
    assert!(!should_attack(
        &state,
        state.unit(anti_air).unwrap(),
        state.unit(ground).unwrap()
    ));
}

/// Fog of war: an unseen target is not a reaction target until it is spotted;
/// stealth unit status follows `UnitInfo::InitStealthStatus`.
#[test]
fn fog_of_war_blocks_reaction_until_spotted() {
    let mut state = state();
    state.play_mode = PlayMode::TurnBased;
    let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
    // Beyond the tank's scan radius (4 cells), so the fog hides it.
    let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(20, 5));
    {
        let unit = state.unit_mut(red).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    state.update_visibility_status(false);

    assert!(!state.unit(green).unwrap().is_visible_to_team(TeamId::RED));
    assert!(!should_attack(
        &state,
        state.unit(red).unwrap(),
        state.unit(green).unwrap()
    ));

    spot_unit(&mut state, green, TeamId::RED);
    assert!(state.unit(green).unwrap().is_visible_to_team(TeamId::RED));
    assert!(should_attack(
        &state,
        state.unit(red).unwrap(),
        state.unit(green).unwrap()
    ));

    // Stealth types: own team always sees them, others only after spotting.
    for unit_type in [
        UnitType::LandMine,
        UnitType::SeaMine,
        UnitType::Infiltrator,
        UnitType::Submarine,
    ] {
        let mut stealth = UnitInfo::new(unit_type, TeamId::RED, UnitId(1));
        init_stealth_status(&mut stealth, true);
        assert!(stealth.is_visible_to_team(TeamId::RED), "{unit_type:?}");
        for raw in 1..=4u8 {
            assert!(
                !stealth.is_visible_to_team(TeamId(raw)),
                "{unit_type:?} visible to team {raw}"
            );
        }
    }

    // Non-stealth types are visible to everyone with `AllVisible`.
    for unit_type in [UnitType::Tank, UnitType::Fighter] {
        let mut ordinary = UnitInfo::new(unit_type, TeamId::RED, UnitId(2));
        init_stealth_status(&mut ordinary, true);
        assert_eq!(ordinary.visible_to_team, [true; 5], "{unit_type:?}");

        init_stealth_status(&mut ordinary, false);
        assert!(ordinary.is_visible_to_team(TeamId::RED));

        for raw in 1..=4u8 {
            assert!(
                !ordinary.is_visible_to_team(TeamId(raw)),
                "{unit_type:?} visible to team {raw} without AllVisible"
            );
        }
    }
}

/// `ZoneWalker`: a radius-0 zone is the cell itself, negative radii are empty,
/// and edge zones are clipped to the map and emitted row-major without repeats.
#[test]
fn zone_walker_boundaries() {
    use crate::geo::{MAP_HEIGHT, MAP_WIDTH};

    assert_eq!(zone_cells(Point::new(5, 5), 0), vec![Point::new(5, 5)]);
    assert!(zone_cells(Point::new(5, 5), -1).is_empty());
    assert_eq!(zone_cells(Point::new(5, 5), 2).len(), 13, "r=2 disk");

    for grid in [
        Point::new(0, 0),
        Point::new(MAP_WIDTH - 1, MAP_HEIGHT - 1),
        Point::new(0, MAP_HEIGHT - 1),
    ] {
        let cells = zone_cells(grid, 3);
        assert!(!cells.is_empty());
        let mut unique = cells.clone();
        unique.sort_by_key(|p| (p.y, p.x));
        unique.dedup();
        assert_eq!(unique.len(), cells.len(), "duplicate cell in {grid:?}");
        for point in &cells {
            assert!(
                point.x >= 0 && point.y >= 0 && point.x < MAP_WIDTH && point.y < MAP_HEIGHT,
                "cell outside the map: {point:?} from {grid:?}"
            );
        }
        let mut sorted = cells.clone();
        sorted.sort_by_key(|p| (p.y, p.x));
        assert_eq!(cells, sorted, "row-major order for {grid:?}");
    }
}

/// `UnitsManager_GetTargetAngle` (8 sectors) and `GetFiringAngle` (16)
/// cover every direction and stay in range.
#[test]
fn angle_sectors_cover_all_directions() {
    let expected_target = [
        ((1, 0), 2),
        ((1, 1), 3),
        ((0, 1), 4),
        ((-1, 1), 5),
        ((-1, 0), 6),
        ((-1, -1), 7),
        ((0, -1), 0),
        ((1, -1), 1),
    ];
    for ((dx, dy), angle) in expected_target {
        assert_eq!(get_target_angle(dx, dy), angle, "target ({dx},{dy})");
        assert_eq!(
            get_target_angle(-dx, -dy),
            (angle + 4) & 7,
            "opposite target ({dx},{dy})"
        );
    }

    let expected_firing = [
        ((1, 0), 4),
        ((0, 1), 8),
        ((-1, 0), 12),
        ((0, -1), 0),
        ((1, 1), 6),
        ((-1, 1), 10),
        ((-1, -1), 14),
        ((1, -1), 2),
    ];
    for ((dx, dy), angle) in expected_firing {
        assert_eq!(get_firing_angle(dx, dy), angle, "firing ({dx},{dy})");
        assert_eq!(
            get_firing_angle(-dx, -dy),
            (angle + 8) & 15,
            "opposite firing ({dx},{dy})"
        );
    }

    for dy in -5..=5 {
        for dx in -5..=5 {
            let target = get_target_angle(dx, dy);
            assert!((0..=7).contains(&target), "target ({dx},{dy}) = {target}");
            let firing = get_firing_angle(dx, dy);
            assert!((0..=15).contains(&firing), "firing ({dx},{dy}) = {firing}");
        }
    }
}

/// `UnitsManager_GetStealthChancePercentage`: zero turns fall back to 90,
/// the formula result is capped at 90, and `AWAIT_STEAL_UNIT` divides by four.
#[test]
fn stealth_chance_boundaries() {
    let mut state = state();
    let target = spawn_tank(&mut state, TeamId::GREEN, Point::new(5, 5));
    let infiltrator = state
        .spawn_unit(
            UnitType::Infiltrator,
            TeamId::RED,
            Point::new(5, 6),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();

    let mut saboteur = state.unit(infiltrator).unwrap().clone();
    let target_unit = state.unit(target).unwrap().clone();

    // turns(TANK) = 4: (experience * 96 + 640) / 4, capped at 90.
    saboteur.experience = 0;
    assert_eq!(
        get_stealth_chance_percentage(&state, &saboteur, &target_unit, UnitOrder::AwaitDisableUnit),
        90
    );
    assert_eq!(
        get_stealth_chance_percentage(&state, &saboteur, &target_unit, UnitOrder::AwaitStealUnit),
        40,
        "steal is a quarter of the base chance"
    );

    saboteur.experience = 100;
    assert_eq!(
        get_stealth_chance_percentage(&state, &saboteur, &target_unit, UnitOrder::AwaitDisableUnit),
        90,
        "capped"
    );

    // A team without values yields `turns == 0`; the function guards the division.
    let empty = GameState::new();
    assert_eq!(
        get_stealth_chance_percentage(&empty, &saboteur, &target_unit, UnitOrder::AwaitDisableUnit),
        90
    );
}

/// `UnitInfo::GainExperience` only touches `REGENERATING_UNIT` units, adds
/// with `i16` wrapping and ignores zero gain.
#[test]
fn gain_experience_only_for_regenerating_units() {
    let mut state = state();
    let alien = state
        .spawn_unit(
            UnitType::AlienTank,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    assert_ne!(
        state.unit(alien).unwrap().flags & crate::enums::unit_flags::REGENERATING_UNIT,
        0,
        "ALNTANK carries REGENERATING_UNIT in SC_UNITS"
    );

    gain_experience(&mut state, alien, 0);
    assert_eq!(state.unit(alien).unwrap().experience, 0);

    gain_experience(&mut state, alien, 3);
    assert_eq!(state.unit(alien).unwrap().experience, 3);

    state.unit_mut(alien).unwrap().experience = i16::MAX;
    gain_experience(&mut state, alien, 1);
    assert_eq!(state.unit(alien).unwrap().experience, i16::MIN, "wraps");

    let tank = spawn_tank(&mut state, TeamId::RED, Point::new(6, 6));
    gain_experience(&mut state, tank, 5);
    assert_eq!(state.unit(tank).unwrap().experience, 0, "non-regenerating");

    // A missing unit is a no-op.
    gain_experience(&mut state, UnitId(0x7FFF), 1);
}

/// Oracle check for the experience formula (`unitinfo.cpp:1622`):
/// `target.TURNS * damage * 5 / (attacker.TURNS * target.HITS)`.
#[test]
fn experience_gain_uses_cpp_turns_operands() {
    let mut state = state();
    state.ai.all_visible = true;
    let attacker = state
        .spawn_unit(
            UnitType::AlienTank,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    let target = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::GREEN,
            Point::new(8, 5),
            crate::enums::unit_flags::MOBILE_AIR_UNIT | SELECTABLE,
        )
        .unwrap();
    state.update_visibility_status(false);

    // ALNTANK attack 20 vs FIGHTER armor 4 → 16, capped to 12 hits.
    attack_unit(&mut state, target, attacker, 0, 2);

    // C++: FIGHTER turns 8 * damage 12 * 5 / (ALNTANK turns 4 * FIGHTER hits 12) = 10.
    assert_eq!(state.unit(attacker).unwrap().experience, 10);
}

/// `unitinfo.cpp:1620-1625`: the attacker earns experience only when **its**
/// team is neither `Remote` nor `Eliminated`; player, computer and empty
/// (`None`) teams still earn it.
#[test]
fn experience_is_skipped_for_remote_and_eliminated_attacker_teams() {
    let cases = [
        (TeamKind::Player, 10),
        (TeamKind::Computer, 10),
        (TeamKind::None, 10),
        (TeamKind::Remote, 0),
        (TeamKind::Eliminated, 0),
    ];

    for (kind, expected) in cases {
        let mut state = state();
        state.ai.all_visible = true;
        state.teams[0].team_type = kind;
        let attacker = state
            .spawn_unit(
                UnitType::AlienTank,
                TeamId::RED,
                Point::new(5, 5),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        let target = state
            .spawn_unit(
                UnitType::Fighter,
                TeamId::GREEN,
                Point::new(8, 5),
                crate::enums::unit_flags::MOBILE_AIR_UNIT | SELECTABLE,
            )
            .unwrap();
        state.update_visibility_status(false);

        // ALNTANK attack 20 vs FIGHTER armor 4 → 16, capped to 12 hits;
        // `target.TURNS 8 * damage 12 * 5 / (attacker.TURNS 4 * target.HITS 12)` = 10.
        attack_unit(&mut state, target, attacker, 0, 2);

        assert_eq!(
            state.unit(attacker).unwrap().experience,
            expected,
            "attacker team {kind:?}"
        );
    }
}

/// Invariant C1 (concrete scenario): reaction fire requires a shot budget
/// (`shots > 0`) and a target inside `ATTRIB_RANGE`; without either the
/// sentry stays in `ORDER_SENTRY`.
///
/// Oracle: `max-cpp-port/src/units_manager.cpp:4709-4735`
/// (`UnitsManager_CheckReaction`).
#[test]
fn invariant_c1_reaction_needs_shots_and_range() {
    let mut state = state();
    state.play_mode = PlayMode::TurnBased;
    // Visibility is an explicit input here; C2 covers the visibility gate.
    state.ai.all_visible = true;

    let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
    let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(7, 5));
    {
        let unit = state.unit_mut(red).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    state.update_visibility_status(false);

    assert!(
        check_reaction(&mut state, red, green),
        "C1: in-range target with shots must trigger a reaction"
    );

    // Reset the fire order and take the shot budget away.
    {
        let unit = state.unit_mut(red).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.shots = 0;
    }
    assert!(
        !check_reaction(&mut state, red, green),
        "C1: a unit with shots == 0 must not react"
    );

    // Restore shots and move the target out of the 4-cell range.
    let rounds = state
        .unit(red)
        .unwrap()
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
    {
        let unit = state.unit_mut(red).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.shots = rounds as u8;
    }
    let mut target = state.unit(green).unwrap().clone();
    target.grid = Point::new(12, 5);
    state.write_unit(green, target);

    assert!(
        !check_reaction(&mut state, red, green),
        "C1: an out-of-range target must not trigger a reaction"
    );
}

/// Invariant C2 (concrete scenario): `ShouldAttack` rejects a target that is
/// not visible to the attacker's team; after `SpotByTeam` the same geometry
/// reacts.
///
/// Oracle: `max-cpp-port/src/units_manager.cpp:4623-4700`
/// (`UnitsManager_ShouldAttack`, the `IsVisibleToTeam` branch).
#[test]
fn invariant_c2_reaction_needs_target_visibility() {
    let mut state = state();
    state.play_mode = PlayMode::TurnBased;
    // MISSILE CRAWLER: range 10, scan 4. Distance 6 is inside the attack
    // range but outside the scan coverage, so the target starts unseen.
    let red = state
        .spawn_unit(
            UnitType::MissileCrawler,
            TeamId::RED,
            Point::new(5, 5),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .unwrap();
    let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(11, 5));
    {
        let unit = state.unit_mut(red).unwrap();
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    state.update_visibility_status(false);

    assert!(
        !state.unit(green).unwrap().is_visible_to_team(TeamId::RED),
        "C2: the target must start outside RED's scan coverage"
    );
    assert!(
        !check_reaction(&mut state, red, green),
        "C2: an invisible target must not trigger reaction fire"
    );

    state.spot_by_team(green, TeamId::RED);
    assert!(
        state.unit(green).unwrap().is_visible_to_team(TeamId::RED),
        "C2: SpotByTeam must reveal the target"
    );
    assert!(
        check_reaction(&mut state, red, green),
        "C2: a visible in-range target must trigger reaction fire"
    );
}

/// Invariant C4: every shot spends one `ammo` and, outside the real-time
/// debug setting, one `shots`; in real-time `shots` is capped by the
/// remaining `ammo`.
///
/// Oracle: `max-cpp-port/src/unitinfo.cpp:4805` (`--ammo`) and `:4813`
/// (`--shots` / the real-time `shots = ammo` branch).
#[test]
fn invariant_c4_prepare_fire_spends_ammo_and_shots() {
    let mut state = state();
    let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));

    for &(shots, ammo, real_time) in &[
        (2u8, 10u8, false),
        (1, 1, false),
        (2, 5, true),
        (1, 3, true),
    ] {
        {
            let unit = state.unit_mut(red).unwrap();
            unit.shots = shots;
            unit.ammo = ammo;
        }
        state.real_time = real_time;

        let mut unit = state.unit(red).unwrap().clone();
        prepare_fire(&mut state, &mut unit);

        assert_eq!(
            unit.ammo,
            ammo.wrapping_sub(1),
            "C4: a shot must always spend one ammo (shots {shots}, ammo {ammo})"
        );
        let expected_shots = if real_time {
            shots.min(ammo.wrapping_sub(1))
        } else {
            shots.wrapping_sub(1)
        };
        assert_eq!(
            unit.shots, expected_shots,
            "C4: shots after firing (turn-based decrement / real-time cap), \
                 real_time={real_time}"
        );
        assert_eq!(
            unit.state,
            UnitOrderState::FireInProgress,
            "C4: the shot must enter ORDER_STATE_FIRE_IN_PROGRESS"
        );
    }
}

/// Invariant C7: damage is clamped by the remaining `hits`; `hits` never goes
/// below zero and a unit that reaches zero enters `ORDER_EXPLODE`.
///
/// Oracle: `max-cpp-port/src/unitinfo.cpp:1566-1570` (clamp and
/// `std::max(0, ...)`) and `:1614-1616` (`SetNewOrderInt(ORDER_EXPLODE)`).
#[test]
fn invariant_c7_hits_never_go_negative_and_zero_explodes() {
    let mut state = state();
    state.ai.all_visible = true;
    let attacker = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
    let target = spawn_tank(&mut state, TeamId::GREEN, Point::new(8, 5));

    // TANK attack 16 − TANK armor 10 = 6 damage.
    for &(hits, expected_hits) in &[(24u16, 18u16), (5, 0), (1, 0)] {
        {
            let unit = state.unit_mut(target).unwrap();
            unit.hits = hits;
            unit.orders = UnitOrder::Await;
            unit.state = UnitOrderState::ExecutingOrder;
        }

        attack_unit(&mut state, target, attacker, 0, 2);

        let unit = state.unit(target).unwrap();
        assert_eq!(
            unit.hits, expected_hits,
            "C7: {hits} hits minus 6 damage must clamp to {expected_hits}"
        );
        assert!(
            unit.hits < u16::MAX,
            "C7: hits must never wrap into the unsigned maximum"
        );
        if expected_hits == 0 {
            assert_eq!(
                unit.orders,
                UnitOrder::Explode,
                "C7: a destroyed unit must enter ORDER_EXPLODE"
            );
            assert_eq!(unit.state, UnitOrderState::Init);
        }
    }
}
