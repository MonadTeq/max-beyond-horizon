//! Frame-level regression test for the missile (rocket) animation.
//!
//! # Oracle
//!
//! * `UnitInfo::PrepareFire` (`unitinfo.cpp:4777-4803`) deploys the projectile
//!   with the **16-direction** firing angle
//!   (`UnitsManager_GetFiringAngle`, `units_manager.cpp:1767`): `D_ROCKET` has
//!   16 frames, `D_ALNMISSL` 32. `AirPath::AirPath` (`airpath.cpp:52-59`) takes
//!   `m_angle = unit->angle`, so the projectile never turns
//!   (`Paths_UpdateAngle` matches immediately) and `UnitInfo::Render` draws the
//!   launch frame for the whole flight.
//! * `AirPath::Execute` (`airpath.cpp:219-242`) moves the projectile pixel by
//!   pixel along the line from the launch cell center to the target cell
//!   center; on every crossed cell it places the trail
//!   (`Paths_DrawMissile`, `airpath.cpp:362-406`).
//! * At arrival `Paths_FinishMove` (`airpath.cpp:414-420`) destroys the
//!   projectile **immediately** — there is no post-impact sprite.
//!
//! # Defect (fixed)
//!
//! The presentation-corpse pass (`App::update_visual_units`) treated every
//! unit with `hits == 0` as a unit that had just died. Projectiles
//! (`MISSILE_UNIT`) have `hits == 0` for their whole life by `SC_ATTRI`, so a
//! flying rocket got a "corpse" record as soon as its visual cell queue was
//! non-empty: the frame drew a **second** rocket sprite walking the cell
//! centers (the live sprite follows the pixel line), and the ghost kept
//! gliding cell-by-cell after the impact — the missile animation regression.
//! Projectiles and effects (`EXPLODING`) are now excluded from the corpse
//! walk; the oracle destroys them instantly without one.
//!
//! The test fires a Rocket (Missile Crawler) at a Tank and checks frame by
//! frame: the projectile never becomes a presentation corpse, keeps its
//! 16-direction launch angle for the whole flight, flies the straight line to
//! the target and disappears at the impact (the explosion appears instead).

use max_app::{App, AppConfig};
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::{TeamClan, UnitId};

const TILE: i32 = 64;

fn set_visible(app: &mut App, id: UnitId) {
    if let Some(mut unit) = app.state_ref().unit(id).cloned() {
        unit.visible_to_team = [true; 5];
        app.universe_mut().write_unit(id, unit);
    }
}

/// Distance from a point to the line `start -> end` (pixels).
fn distance_to_line(point: (i32, i32), start: (i32, i32), end: (i32, i32)) -> f64 {
    let dx = f64::from(end.0 - start.0);
    let dy = f64::from(end.1 - start.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length == 0.0 {
        return 0.0;
    }
    let px = f64::from(point.0 - start.0);
    let py = f64::from(point.1 - start.1);
    (px * dy - py * dx).abs() / length
}

fn rocket_of(app: &App) -> Option<UnitId> {
    app.state_ref().unit_ids_in_order().into_iter().find(|id| {
        app.state_ref()
            .unit(*id)
            .is_some_and(|unit| unit.unit_type == UnitType::Rocket)
    })
}

/// A Rocket flies from the launcher to the target: no presentation corpse at
/// any frame, the launch (16-direction) angle is kept for the whole flight,
/// the pixel path is a straight line, and the impact removes the projectile.
#[test]
fn a_flying_rocket_keeps_the_launch_angle_and_never_becomes_a_corpse() {
    let mut app = App::new(AppConfig::default());
    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().teams_mut()[1].team_type = TeamKind::Computer;
    app.universe_mut().team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
    app.universe_mut().team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);

    // The missile-line QA geometry: 3 west, 4 south of the target.
    let launcher_grid = Point::new(29, 18);
    let target_grid = Point::new(26, 22);

    let launcher = app
        .universe_mut()
        .spawn_unit(
            UnitType::MissileCrawler,
            TeamId::RED,
            launcher_grid,
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("launcher");
    let target = app
        .universe_mut()
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            target_grid,
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("target");
    set_visible(&mut app, launcher);
    set_visible(&mut app, target);

    let mut data = app.state_ref().order_data(launcher);
    data.fire_on = target_grid;
    data.move_to = target_grid;
    app.universe_mut().set_order_data(launcher, data);
    app.universe_mut()
        .set_order(launcher, UnitOrder::Fire, UnitOrderState::Init);
    app.update_visual_units(0.0);

    // Aim + recoil, then the projectile spawns.
    let mut rocket = None;
    for _ in 0..400 {
        app.tick();
        rocket = rocket_of(&app);
        if rocket.is_some() {
            break;
        }
    }
    let rocket = rocket.expect("PrepareFire deploys the ROCKET");

    let launch_unit = app.state_ref().unit(rocket).expect("rocket").clone();
    // `UnitsManager_GetFiringAngle` is 16-direction (0..15); an 8-direction
    // mask would collapse angle 10 (south-west-ish) to 2 and flip the sprite.
    assert!(
        launch_unit.angle > 7,
        "the projectile must keep the 16-direction firing angle, got {}",
        launch_unit.angle
    );
    let firing_angle = launch_unit.angle;
    let start = (
        i32::from(launcher_grid.x) * TILE + TILE / 2,
        i32::from(launcher_grid.y) * TILE + TILE / 2,
    );
    let end = (
        i32::from(target_grid.x) * TILE + TILE / 2,
        i32::from(target_grid.y) * TILE + TILE / 2,
    );

    let mut flew = false;
    let mut impact = false;
    let mut explosion = false;

    for frame in 0..600 {
        app.tick();
        app.update_visual_units(1.0 / 60.0);

        // The regression: the flying rocket must never get a presentation
        // corpse — the oracle destroys the projectile at the impact instantly.
        assert!(
            !app.presentation_corpses().contains_key(&rocket),
            "frame {frame}: a flying rocket became a presentation corpse"
        );

        if let Some(unit) = app.state_ref().unit(rocket) {
            assert_eq!(
                unit.angle, firing_angle,
                "frame {frame}: the projectile must keep the launch angle"
            );
            assert_eq!(
                app.visual_angle(rocket),
                Some(firing_angle),
                "frame {frame}: the visual segment must not override the launch angle"
            );
            let point = (i32::from(unit.x), i32::from(unit.y));
            let deviation = distance_to_line(point, start, end);
            assert!(
                deviation <= 1.5,
                "frame {frame}: the projectile left the line {start:?}->{end:?} \
                 ({deviation:.2} px)"
            );
            flew |= point != start;
        } else if flew {
            // After the impact the projectile is gone — no ghost keeps flying.
            impact = true;
        }

        explosion |= app.state_ref().unit_ids_in_order().into_iter().any(|id| {
            app.state_ref().unit(id).is_some_and(|unit| {
                matches!(
                    unit.unit_type,
                    UnitType::HitExplosion
                        | UnitType::LandExplosion
                        | UnitType::BuildingExplosion
                        | UnitType::AirExplosion
                        | UnitType::SeaExplosion
                )
            })
        });

        if impact && explosion {
            break;
        }
    }

    assert!(flew, "the rocket must fly from the launch cell");
    assert!(impact, "the rocket must be destroyed at the impact");
    assert!(explosion, "the impact must produce an explosion effect");
    assert!(
        app.presentation_corpses().is_empty(),
        "after the impact there are no presentation corpses: {:?}",
        app.presentation_corpses()
            .values()
            .map(|corpse| corpse.unit.unit_type)
            .collect::<Vec<_>>()
    );
}
