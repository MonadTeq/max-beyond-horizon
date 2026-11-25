use super::*;

#[test]
fn default_config_points_at_the_repository_layout() {
    let config = AppConfig::default();

    assert_eq!(config.mode, AppMode::Demo);
    assert_eq!(config.demo_turns, 5);
    assert_eq!(config.horizon_port, 8082);
    assert_eq!(config.max_res, PathBuf::from("max-original/MAX.RES"));
    assert_eq!(config.save, None);
}

#[test]
fn p2_reference_matches_the_vector_case() {
    let config = NewGameConfig::p2_reference();

    assert_eq!(config.planet, Planet::Snowcrab);
    assert_eq!(config.seed, 0);
    assert_eq!(config.raw.value(), 1);
    assert_eq!(config.fuel.value(), 1);
    assert_eq!(config.gold.value(), 1);
    assert_eq!(config.derelicts.value(), 0);
    assert_eq!(
        config.teams,
        vec![
            TeamStart::new(TeamId::RED, Point::new(20, 20)),
            TeamStart::new(TeamId::BLUE, Point::new(70, 70)),
        ]
    );
}

#[test]
fn digest_line_has_the_documented_fields() {
    let digest = AppDigest::capture(GameUniverse::default().snapshot(), 0);
    let line = digest.to_line();

    assert!(line.contains("ticks=0"));
    assert!(line.contains("state_digest=0x"));
    assert!(line.contains("replay_crc=0x"));
    assert_eq!(
        digest,
        AppDigest::capture(GameUniverse::default().snapshot(), 0)
    );
}

/// Unique temporary test directory (without external crates).
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
            "max_app_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn displayed_speed_follows_the_visual_route_not_the_tick() {
    // The panel's `Speed` row must fall with the sprite, not with the fast
    // simulation (`Paths_CalculateStep` decrements the value as C++ walks a cell).
    let mut app = App::new(AppConfig::default());
    let mut setup = LocalSetup::default();
    setup.world = 0;
    setup.timer = 0;
    setup.team_types = [
        max_core::team::TeamKind::Player,
        max_core::team::TeamKind::Computer,
        max_core::team::TeamKind::None,
        max_core::team::TeamKind::None,
    ];
    if app.start_local_game(&setup).is_err() {
        eprintln!("displayed_speed: no map data");
        return;
    }

    let team = app.player_team();
    let surveyor = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref().unit(*id).is_some_and(|unit| {
                unit.team == team && unit.unit_type == max_core::enums::UnitType::Surveyor
            })
        })
        .expect("surveyor");
    let start = app.state_ref().unit(surveyor).expect("surveyor").grid;

    let mut target = None;
    'search: for dy in -3..=3 {
        for dx in -3..=3 {
            let point = max_core::geo::Point::new(start.x + dx, start.y + dy);
            if point == start || !app.state_ref().units_at(point).is_empty() {
                continue;
            }
            if crate::hud::build_move_order(app.state_ref(), surveyor, point).is_some() {
                target = Some(point);
                break 'search;
            }
        }
    }
    let target = target.expect("reachable cell");
    let order = crate::hud::build_move_order(app.state_ref(), surveyor, target).expect("order");

    app.hud_mut().select(Some(surveyor));
    app.hud_mut().pending.push(order.command);
    app.flush_orders();
    app.update_visual_units(0.0);

    let speed0 = app.state_ref().unit(surveyor).expect("surveyor").speed;
    assert!(speed0 > 1, "the surveyor starts with movement points: {speed0}");

    // One simulation step (the unit moves a whole cell in a tick) — the frame
    // has not walked it yet.
    let mut ticks = 0;
    while app.state_ref().unit(surveyor).map(|unit| unit.grid) == Some(start) && ticks < 20 {
        app.tick();
        ticks += 1;
    }
    assert_ne!(
        app.state_ref().unit(surveyor).map(|unit| unit.grid),
        Some(start),
        "the order must move the unit in the simulation"
    );
    app.update_visual_units(1.0 / 60.0);

    let sim_speed = app.state_ref().unit(surveyor).expect("surveyor").speed;
    assert!(sim_speed < speed0, "the tick spends movement points");
    assert_eq!(
        app.displayed_speed(surveyor),
        Some(speed0),
        "the panel still shows the points of the cells the sprite has not walked"
    );

    // Walk the frame: the displayed value decreases and ends exactly at the
    // simulation value when the sprite arrives.
    let mut last = app.displayed_speed(surveyor).expect("speed");
    let mut frames = 0;
    loop {
        app.update_visual_units(1.0 / 60.0);
        let shown = app.displayed_speed(surveyor).expect("speed");
        assert!(
            shown <= last,
            "the displayed speed must not grow while walking: {last} -> {shown}"
        );
        last = shown;
        frames += 1;

        if shown == app.state_ref().unit(surveyor).expect("surveyor").speed {
            break;
        }
        assert!(frames < 1200, "the sprite did not finish the route: {shown}");
    }
}

#[test]
fn handshake_patches_default_to_the_assets_path() {
    let config = AppConfig::default();

    assert_eq!(config.handshake_patches, None);
    assert_eq!(
        config.handshake_patches_path(),
        config.patches_res.as_path(),
            "None → хэш из assets-пути patches_res, как раньше"
    );
}

#[test]
fn handshake_patches_override_only_the_hash_path() {
    let config = AppConfig::default().with_handshake_patches("/tmp/nonexistent.res");

    assert_eq!(
        config.handshake_patches_path(),
        Path::new("/tmp/nonexistent.res")
    );
    assert_eq!(
        config.patches_res,
        PathBuf::from("assets/PATCHES.RES"),
            "ассетный путь не меняется"
    );
}

#[test]
fn data_hash_uses_handshake_patches_and_leaves_the_patches_part_empty_when_missing() {
    let dir = temp_dir("handshake_hash");
    let max_res = dir.join("MAX.RES");
    let patches = dir.join("PATCHES.RES");
    std::fs::write(&max_res, b"max-res-bytes").expect("write MAX.RES");
    std::fs::write(&patches, b"patches-bytes").expect("write PATCHES.RES");

    // None → the PATCHES part of the hash is computed by the assets path.
    let assets_only = AppConfig {
        max_res: max_res.clone(),
        patches_res: patches.clone(),
        ..Default::default()
    };
    let hash = max_net::DataHash::from_files(
        &assets_only.max_res,
        assets_only.handshake_patches_path(),
    )
    .expect("hash files");
    assert!(hash.max_res.is_some());
    assert!(
        hash.patches_res.is_some(),
            "None → хэш из assets-пути patches_res"
    );

    // Some(nonexistent path) → an empty PATCHES part, but the assets remain.
    let split = AppConfig {
        max_res,
        patches_res: patches,
        handshake_patches: Some(dir.join("nonexistent.res")),
        ..Default::default()
    };
    let hash = max_net::DataHash::from_files(&split.max_res, split.handshake_patches_path())
        .expect("hash files");
    assert!(hash.max_res.is_some());
    assert!(
        hash.patches_res.is_none(),
            "путь хендшейка не существует → хэш без PATCHES-части (семантика сервера)"
    );
    assert_eq!(
        split.handshake_patches_path(),
        dir.join("nonexistent.res").as_path()
    );

    std::fs::remove_dir_all(dir).ok();
}
