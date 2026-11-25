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
