use super::*;
use crate::AppConfig;

/// `UnitInfo::RenderShadow` (`unitinfo.cpp:3067`: `point -= shadow_offset`):
/// the presentation keeps the C++ sign, the sprite renderer adds the offset,
/// so a hovering aircraft (C++ `(-64, -64)`) draws its shadow one cell
/// **down-right** of the hull.
#[test]
fn the_air_shadow_is_drawn_below_and_right_of_the_hull() {
    use max_core::geo::Point;

    let mut presentation = UnitPresentation::new();
    assert_eq!(
        shadow_render_offset(&presentation),
        (0, 0),
            "shadow under the hull"
    );

    // `UnitInfo::Redraw` for `MOBILE_AIR_UNIT | HOVERING`.
    presentation.shadow_offset = Point::new(-64, -64);
    assert_eq!(
        shadow_render_offset(&presentation),
        (64, 64),
            "a flying aircraft's shadow is one cell down-right (airborne look)"
    );

    // A takeoff step of `Take()`: `shadow_offset -= 16` → a smaller shift.
    presentation.shadow_offset = Point::new(-32, -32);
    assert_eq!(shadow_render_offset(&presentation), (32, 32));
}

/// `UnitInfo::ShakeAir` (`unitinfo.cpp:4636-4678`): the hull swings at most
/// two pixels along the rotating direction, the offsets cancel in each 8-tick
/// block, and the shadow moves three times as far (`shadow_offset -= 2 * offset`
/// on top of the hull's `OffsetDrawZones(offset)`).
#[test]
fn the_flight_bob_follows_shake_air() {
    // First block: north. Two even ticks out, two even ticks back.
    assert_eq!(shake_air_offset(0), (0, -1));
    assert_eq!(shake_air_offset(1), (0, -1));
    assert_eq!(shake_air_offset(2), (0, -2));
    assert_eq!(shake_air_offset(3), (0, -2));
    assert_eq!(shake_air_offset(4), (0, -1));
    assert_eq!(shake_air_offset(5), (0, -1));
    assert_eq!(shake_air_offset(6), (0, 0));
    assert_eq!(shake_air_offset(7), (0, 0));
    // Second block: north-east.
    assert_eq!(shake_air_offset(8), (1, -1));
    assert_eq!(shake_air_offset(10), (2, -2));
    assert_eq!(shake_air_offset(14), (0, 0));
    // Last block: north-west — the `moved / 8` direction index wraps.
    assert_eq!(shake_air_offset(56), (-1, -1));
    assert_eq!(shake_air_offset(62), (0, 0));

    // The shadow: the hull anchor carries `offset`, the stored value takes
    // `-2 * offset`, so the shadow ends `3 * offset` from its base.
    let mut presentation = UnitPresentation::new();
    presentation.shadow_offset = Point::new(-64, -64);
    assert_eq!(
        bobbed_shadow_offset(&presentation, (0, -2)),
        Point::new(-64, -60),
            "shadow_offset -= 2 * offset"
    );
    let bobbing = bobbed_shadow_offset(&presentation, (0, -2));
    assert_eq!(
        (-bobbing.x, -bobbing.y),
        (64, 60),
            "the shadow moves 3 × 2 px north of its base"
    );
    assert_eq!(
        bobbed_shadow_offset(&presentation, (0, 0)),
        presentation.shadow_offset,
            "no bob — the stored shadow offset is unchanged"
    );
}

/// `AirPath::Execute` (`airpath.cpp:219-242`) and `UnitsManager_StartExplosion`
/// (`units_manager.cpp:3991-3994`): both are drawn at the stored **pixel**
/// position, not at the cell anchor — the air path is a straight pixel segment and
/// the blast takes the dying unit's `x/y`.
#[test]
fn aircraft_and_explosions_are_drawn_on_their_pixel() {
    use max_core::enums::{unit_flags, UnitType};
    use max_core::geo::Point;
    use max_core::team::TeamId;
    use max_core::unit::{UnitId, UnitInfo};

    let mut fighter = UnitInfo::new(UnitType::Fighter, TeamId::RED, UnitId(1));
    fighter.flags |= unit_flags::MOBILE_AIR_UNIT;
    fighter.grid = Point::new(10, 10);
    // Twelve pixels right and three down of the cell center: a point of the air
    // path between two cells.
    fighter.x = (10 * 64 + 32 + 12) as u16;
    fighter.y = (10 * 64 + 32 + 3) as u16;

    let visual = (f32::from(fighter.x) / 64.0, f32::from(fighter.y) / 64.0);
    assert_eq!(
        air_pixel_anchor(&fighter, visual),
        Some((i32::from(fighter.x), i32::from(fighter.y))),
        "the frame point (cells) × 64 is the air-path pixel, without the +32 center"
    );

    let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
    tank.flags |= unit_flags::MOBILE_LAND_UNIT;
    tank.grid = Point::new(10, 10);
    assert_eq!(air_pixel_anchor(&tank, (10.0, 10.0)), None);
    assert_eq!(
        unit_anchor_world(&tank, (10.0, 10.0)),
        (10 * 64 + 32, 10 * 64 + 32),
        "a ground unit stays on the cell-center anchor"
    );

    let mut blast = UnitInfo::new(UnitType::LandExplosion, TeamId::RED, UnitId(3));
    blast.flags |= unit_flags::EXPLODING;
    blast.x = 700;
    blast.y = 690;
    assert_eq!(explosion_pixel_anchor(&blast), Some((700, 690)));
    assert_eq!(explosion_pixel_anchor(&tank), None);
}

#[test]
fn frame_index_uses_the_given_body_angle_and_power_on_offset() {
    use max_core::enums::UnitType;
    use max_core::team::TeamId;
    use max_core::unit::{UnitId, UnitInfo};

    let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    unit.angle = 7;

    for angle in 0..8u8 {
        assert_eq!(
            unit_frame_index_for_angle(&unit, 10, 100, angle),
            10 + usize::from(angle),
                "кадр корпуса = image_base + angle"
        );
    }
    assert_eq!(unit_frame_index(&unit, 10, 100), 17);

    // `ORDER_POWER_ON` — the next frame of a powered-on building.
    unit.orders = UnitOrder::PowerOn;
    assert_eq!(unit_frame_index_for_angle(&unit, 0, 100, 2), 3);
    // The last frame does not overflow.
    assert_eq!(unit_frame_index_for_angle(&unit, 0, 3, 2), 2);
}

#[test]
fn elevated_bridge_draws_its_stored_elevation_frame() {
    use max_core::enums::UnitType;
    use max_core::team::TeamId;
    use max_core::unit::{UnitId, UnitInfo};

    // `UnitInfo::IsBridgeElevated` (`unitinfo.cpp:3030`): the `BRIDGE` sprite's
    // frames 0..4 are elevation states; `ORDER_STATE_ELEVATE`/`LOWER` change
    // `image_index`, and C++ renders exactly that index
    // (`UnitInfo::Render`, `unitinfo.cpp:3141`). Without this the bridge looked
    // lowered while a ship passed under it.
    let mut bridge = UnitInfo::new(UnitType::Bridge, TeamId::RED, UnitId(1));
    bridge.image_base = 0;
    bridge.image_index = 0;
    assert_eq!(unit_frame_index_for_angle(&bridge, 0, 5, 0), 0);

    for elevation in 1..=3 {
        bridge.image_index = elevation;
        for angle in 0..8u8 {
            assert_eq!(
                unit_frame_index_for_angle(&bridge, 0, 5, angle),
                elevation as usize,
                "the raised bridge draws frame {elevation}, not base + angle"
            );
        }
    }

    // A non-bridge unit keeps the base + angle frame even if `image_index` differs
    // (e.g. a stale save field).
    let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(2));
    tank.image_index = 3;
    assert_eq!(unit_frame_index_for_angle(&tank, 8, 16, 2), 10);
}

#[test]
fn builder_work_cycle_matches_the_cpp_plus_eight_minus_sixteen_loop() {
    use max_core::enums::UnitType;
    use max_core::team::TeamId;
    use max_core::unit::{UnitId, UnitInfo};

    let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
    builder.orders = UnitOrder::Build;
    builder.state = UnitOrderState::BuildInProgress;

    // CONSTRCT: 40 frames, angle 3 → image_index = 3 + 16 = 19.
    // C++ ProcessOrderBuild: +8, +8, then 35 + 8 >= 40 → 35 - 16 = 19.
    let cycle: Vec<usize> = (0..7)
        .map(|step| unit_frame_index_for_animation(&builder, 0, 40, 3, step % 3))
        .collect();
    assert_eq!(cycle, vec![19, 27, 35, 19, 27, 35, 19]);

    // Angle 0: construction start frames +16 = 16, then 24 and 32.
    let cycle: Vec<usize> = (0..3)
        .map(|step| unit_frame_index_for_animation(&builder, 0, 40, 0, step))
        .collect();
    assert_eq!(cycle, vec![16, 24, 32]);

    // The engineer has no cycle: the working pose +16 is constant (StartBuilding).
    let engineer = {
        let mut unit = UnitInfo::new(UnitType::Engineer, TeamId::RED, UnitId(2));
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        unit
    };
    for step in 0..3 {
        assert_eq!(
            unit_frame_index_for_animation(&engineer, 0, 32, 3, step),
            19,
                "ENGINEER держит DrawSpriteFrame(image_index + 16)"
        );
    }

    // After construction the frame returns to the base regardless of the step.
    builder.orders = UnitOrder::Await;
    builder.state = UnitOrderState::ExecutingOrder;
    for step in 0..3 {
        assert_eq!(unit_frame_index_for_animation(&builder, 0, 40, 3, step), 3);
    }
}

/// `SPINNING_TURRET` radar: `UnitInfo::SpinningTurretAdvanceAnimation`
/// (`unitinfo.cpp:3891`) advances `turret_image_index` by one on every
/// `UnitsManager_ProcessOrder` tick and wraps at
/// `turret_image_base + turret_image_count - 1` back to `turret_image_base`
/// (`UnitsManager_DeployUnit`, `units_manager.cpp:2446-2449`).
#[test]
fn spinning_turret_walks_through_its_d_resource_frames() {
    use max_core::enums::UnitType;
    use max_core::team::TeamId;
    use max_core::unit::{UnitId, UnitInfo};

    let info = |base: u8, count: u8| max_assets::sprite::FrameInfo {
        turret_image_base: base,
        turret_image_count: count,
        ..max_assets::sprite::FrameInfo::default()
    };
    let frame = |info: &max_assets::sprite::FrameInfo, unit: &UnitInfo, step: u32| {
        spinning_turret_frame_index(
            info,
            unit,
            FrameAnimation {
                turret_step: step,
                ..FrameAnimation::default()
            },
        )
    };

    // `AWAC`: `D_AWAC` turret base 8, 30 frames (`max_core::frames`).
    let mut awac = UnitInfo::new(UnitType::Awac, TeamId::RED, UnitId(1));
    awac.flags |= unit_flags::SPINNING_TURRET;
    awac.orders = UnitOrder::Sentry;
    assert!(spinning_turret_animation_active(&awac));

    let awac_info = info(8, 30);
    assert_eq!(frame(&awac_info, &awac, 0), Some(8));
    assert_eq!(frame(&awac_info, &awac, 1), Some(9));
    assert_eq!(frame(&awac_info, &awac, 29), Some(37));
    assert_eq!(
        frame(&awac_info, &awac, 30),
        Some(8),
            "wrap to turret_image_base"
    );
    assert_eq!(frame(&awac_info, &awac, 31), Some(9));

    // `SCANNER`: `D_SCANNER` turret base 8, 16 frames.
    let mut scanner = UnitInfo::new(UnitType::Scanner, TeamId::RED, UnitId(2));
    scanner.flags |= unit_flags::SPINNING_TURRET;
    scanner.orders = UnitOrder::Await;
    assert!(spinning_turret_animation_active(&scanner));

    let scanner_info = info(8, 16);
    assert_eq!(frame(&scanner_info, &scanner, 15), Some(23));
    assert_eq!(frame(&scanner_info, &scanner, 16), Some(8));

    // The start phase is the C++ stored frame `turret_image_base + turret_angle`
    // (`UnitInfo::UpdateTurretAngle`, `unitinfo.cpp:2690-2696`).
    awac.turret_angle = 3;
    assert_eq!(frame(&awac_info, &awac, 0), Some(11));
    assert_eq!(frame(&awac_info, &awac, 27), Some(8));

    // `ORDER_DISABLE` — `UnitsManager_ProcessOrder` skips the animation
    // (`units_manager.cpp:3263`); a turret-less type and an empty `D_*` row
    // have no cycle either.
    awac.orders = UnitOrder::Disable;
    assert!(!spinning_turret_animation_active(&awac));

    let tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(3));
    assert!(
        !spinning_turret_animation_active(&tank),
            "TANK has TURRET_SPRITE, not SPINNING_TURRET"
    );
    assert_eq!(frame(&info(8, 0), &scanner, 5), None);
}

#[test]
fn scaler_scale_matches_the_cpp_divisor() {
    assert_eq!(scaler_scale(0), 1.0);
    assert_eq!(scaler_scale(1), 0.5);
    assert_eq!(scaler_scale(2), 0.25);
    assert_eq!(scaler_scale(3), 0.125);
    assert_eq!(scaler_scale(4), 0.0625);
    // `scaler_adjust` in C++ — uint8_t, but really 0..4; we clamp it.
    assert_eq!(scaler_scale(9), 0.0625);
}

#[test]
fn frame_without_assets_has_screen_size_and_is_opaque() {
    let app = App::new(AppConfig::default().with_screen(64, 48));
    let (frame, stats) = app.render_frame_with_stats();

    assert_eq!(frame.width(), 64);
    assert_eq!(frame.height(), 48);
    assert_eq!(frame.len(), 64 * 48);
    assert!(stats.map_tiles > 0, "карта должна рисоваться без ассетов");
    assert_eq!(stats.units(), stats.units_drawn + stats.units_marker);
    assert!(frame.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));
}

#[test]
fn enemy_units_are_rendered_only_when_visible() {
    use max_core::enums::UnitType;
    use max_core::geo::Point;
    use max_core::team::TeamId;

    let mut app = App::new(AppConfig::default());
    app.universe = max_core::universe::GameUniverse::default();

    let own = app
        .universe
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(55, 55), 0)
        .expect("свой танк");
    let enemy = app
        .universe
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(56, 55), 0)
        .expect("чужой танк");

    assert!(app
        .state_ref()
        .unit(own)
        .unwrap()
        .is_visible_to_team(TeamId::RED));
    assert!(
        !app.state_ref()
            .unit(enemy)
            .unwrap()
            .is_visible_to_team(TeamId::RED),
            "чужой юнит вне FOV не виден"
    );

    // Without assets units are drawn with markers: own — one.
    let (frame, stats) = app.render_frame_with_stats();
    assert_eq!(stats.units_marker, 1, "виден только свой юнит");
    assert_eq!(stats.units_drawn, 0);
    assert_eq!(
        frame.get(
            55 * 64 + 32 - app.viewport().origin().0,
            55 * 64 + 32 - app.viewport().origin().1
        ),
        Some(MARKER_COLORS[0]),
            "свой маркер на карте"
    );

    // Appeared in the FOV (survey/radar) — a foreign one is drawn too.
    app.universe
        .unit_mut(enemy)
        .unwrap()
        .set_visible_to_team(TeamId::RED, true);
    let (_, stats) = app.render_frame_with_stats();
    assert_eq!(stats.units_marker, 2, "видимый чужой юнит рисуется");
}

#[test]
fn ppm_header_is_p6_with_frame_dimensions() {
    let path = std::env::temp_dir().join("max_app_render_test.ppm");
    let frame = RgbaBuffer::filled(3, 2, Rgba::rgb(1, 2, 3));

    write_ppm(&path, &frame).expect("ppm written");
    let bytes = std::fs::read(&path).expect("ppm read back");
    let header = b"P6\n3 2\n255\n";

    assert_eq!(&bytes[..header.len()], header);
    assert_eq!(bytes.len(), header.len() + 3 * 2 * 3);
    assert_eq!(&bytes[header.len()..header.len() + 3], &[1, 2, 3]);

    let _ = std::fs::remove_file(&path);
}
