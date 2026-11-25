use super::*;
use max_core::enums::{unit_flags, Planet, UnitType, UNIT_END};

/// Game assets from the repository; `None` if the data is not nearby.
fn real_assets() -> Option<Assets> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("tests/fixtures/assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        return None;
    }

    Some(
        Assets::load(&AppConfig {
            max_res,
            color_pal,
            patches_res: patches,
            ..Default::default()
        })
        .expect("ассеты игры"),
    )
}

#[test]
fn alien_derelict_sprites_are_loaded() {
    use max_core::enums::UnitType;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("tests/fixtures/assets/PATCHES.RES");

    assert!(max_res.is_file() && color_pal.is_file() && patches.is_file(), concat!("required test fixture is missing: ", stringify!(!max_res.is_file() || !color_pal.is_file() || !patches.is_file())));

    let config = AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        ..Default::default()
    };
    let assets = Assets::load(&config).expect("ассеты игры");

    // Alien base debris: three units (`ALN*`) + slab/bridge and buildings.
    for unit_type in [
        UnitType::AlienTank,
        UnitType::AlienAssaultGun,
        UnitType::AlienPlane,
        UnitType::LargeSlab,
        UnitType::Bridge,
        UnitType::MonopoleMine,
        UnitType::SupportPlant,
        UnitType::RecreationCenter,
    ] {
        let sprite = assets
            .sprite(unit_type)
            .unwrap_or_else(|| panic!("нет спрайта {unit_type:?}"));
        assert!(!sprite.sheet.is_empty(), "{unit_type:?}: нет кадров");
    }

    // `RADAR` is the radar building sprite (20 frames 64×64, 60 449 B), not
    // the minimap frame: the frame is drawn in `FRAMEPIC`
    // (`ResourceManager_MinimapBgImage` — a snapshot of the minimap window).
    let radar = assets.sprite(UnitType::Radar).expect("RADAR");
    assert_eq!(radar.sheet.len(), 20);
    assert_eq!(
        (radar.sheet.frames[0].width, radar.sheet.frames[0].height),
        (64, 64)
    );
    assert_eq!(
        (radar.sheet.frames[0].hotx, radar.sheet.frames[0].hoty),
        (32, 32)
    );
}

#[test]
fn planet_pictures_cover_all_four_groups() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("tests/fixtures/assets/PATCHES.RES");

    assert!(max_res.is_file() && color_pal.is_file() && patches.is_file(), concat!("required test fixture is missing: ", stringify!(!max_res.is_file() || !color_pal.is_file() || !patches.is_file())));

    let assets = Assets::load(&AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        ..Default::default()
    })
    .expect("ассеты игры");

    assert_eq!(assets.planet_pictures.len(), 4);

    for planet in [
        Planet::Snowcrab,
        Planet::IronCross,
        Planet::NewLuzon,
        Planet::Bottleneck,
    ] {
        let picture = assets.planet_picture(planet).expect("картинка планеты");
        assert_eq!((picture.width, picture.height), (128, 128));
    }
}

#[test]
fn every_planet_maps_to_a_distinct_world_asset() {
    let mut names = Vec::new();

    for index in 0..WORLD_ASSETS.len() {
        let planet = Planet::try_from(index as u8).expect("planet id in range");
        let name = world_asset_name(planet);
        assert!(!name.is_empty());
        names.push(name);
    }

    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), WORLD_ASSETS.len());
    assert_eq!(world_asset_name(Planet::Snowcrab), "SNOW_1");
    assert_eq!(world_asset_name(Planet::Bottleneck), "DESERT_6");
}

#[test]
fn sprite_table_covers_distinct_unit_types() {
    let mut types: Vec<u16> = UNIT_SPRITES
        .iter()
        .map(|(unit_type, _, _)| *unit_type as u16)
        .collect();
    types.sort_unstable();
    types.dedup();
    assert_eq!(types.len(), UNIT_SPRITES.len());
    assert_eq!(UNIT_SPRITES.len(), UNIT_END as usize);
}

/// The table is a line-by-line port of `SC_UNITS`: the type, sprite and shadow of
/// each of the 93 types match `cargo_unit_data` (the generator from `SC_UNITS`).
#[test]
fn unit_sprite_table_mirrors_sc_units() {
    assert_eq!(UNIT_SPRITES.len(), UNIT_END as usize);

    for (index, &(unit_type, sprite, shadow)) in UNIT_SPRITES.iter().enumerate() {
        let expected = UnitType::try_from(index as u16).expect("тип 0..92");
        assert_eq!(unit_type, expected, "порядок UNIT_SPRITES ≠ UnitType");
        assert_eq!(
            Some(sprite),
            crate::cargo_unit_data::unit_sprite(expected),
                "{expected:?}: спрайт не совпал с SC_UNITS"
        );
        assert_eq!(
            shadow,
            crate::cargo_unit_data::unit_shadow(expected),
                "{expected:?}: тень не совпала с SC_UNITS"
        );
    }

    // Documented holes in the game data (not the table).
    assert_eq!(
        UNIT_TYPES_WITHOUT_SPRITE_DATA,
        [UnitType::MasterBuilder],
            "SC_UNITS ссылается на MASTER, но ресурса нет в архивах"
    );
    assert_eq!(
        UNIT_TYPES_WITHOUT_SHADOW_DATA,
        [UnitType::Submarine],
            "SC_UNITS ссылается на S_SUBMAR, но ресурса нет в архивах"
    );
}

/// All 92 types with data load: body, 8 angles, shadow (if in
/// `SC_UNITS`), and `MASTER` — the only marker.
#[test]
fn every_unit_type_loads_its_body_and_shadow() {
    let assets = real_assets().expect(concat!("test fixture is missing: ", stringify!(real_assets())));

    let mut loaded = 0usize;
    let mut shadows = 0usize;

    for (unit_type, sprite_name, shadow_name) in UNIT_SPRITES {
        if UNIT_TYPES_WITHOUT_SPRITE_DATA.contains(&unit_type) {
            assert!(
                assets.sprite(unit_type).is_none(),
                    "{unit_type:?}: ресурс {sprite_name} появился — обновите таблицу"
            );
            assert!(
                assets
                    .warnings()
                    .iter()
                    .any(|warning| warning.contains(sprite_name)),
                    "{unit_type:?}: ожидалось предупреждение о {sprite_name}: {:?}",
                assets.warnings()
            );
            continue;
        }

        let sprite = assets
            .sprite(unit_type)
            .unwrap_or_else(|| panic!("{unit_type:?}: не загрузился {sprite_name}"));
        assert!(!sprite.sheet.is_empty(), "{unit_type:?}: нет кадров");

        for angle in 0..8u8 {
            assert!(
                sprite.frame_for(0, angle).is_some(),
                    "{unit_type:?}: нет кадра корпуса для угла {angle}"
            );
        }

        match shadow_name {
            Some(name) if UNIT_TYPES_WITHOUT_SHADOW_DATA.contains(&unit_type) => {
                assert!(
                    sprite.shadow.is_none(),
                        "{unit_type:?}: тень {name} вдруг загрузилась"
                );
                assert!(
                    assets
                        .warnings()
                        .iter()
                        .any(|warning| warning.contains(name)),
                        "{unit_type:?}: ожидалось предупреждение о тени {name}: {:?}",
                    assets.warnings()
                );
            }
            Some(name) => {
                assert!(
                    sprite.shadow.is_some(),
                        "{unit_type:?}: не загрузилась тень {name}"
                );
                for angle in 0..8u8 {
                    assert!(
                        sprite.shadow_for(0, angle).is_some(),
                            "{unit_type:?}: нет кадра тени для угла {angle}"
                    );
                }

                shadows += 1;
            }
            None => assert!(
                sprite.shadow.is_none(),
                    "{unit_type:?}: в SC_UNITS тени нет, а она загрузилась"
            ),
        }

        loaded += 1;
    }

    assert_eq!(
        loaded,
        UNIT_SPRITES.len() - UNIT_TYPES_WITHOUT_SPRITE_DATA.len()
    );
    assert_eq!(loaded, 92, "ожидались спрайты 92 типов из 93");
    assert_eq!(
        shadows, 75,
            "SC_UNITS объявляет 77 теней, S_MASTER/S_SUBMAR в архивах отсутствуют"
    );
}

/// A turret exists exactly for the types with `TURRET_SPRITE`/`SPINNING_TURRET` in
/// `SC_UNITS` (11 types), the `D_*` frames cover all 8 angles, and the offset
/// matches `angle_offsets`.
#[test]
fn turret_frames_cover_every_flagged_type() {
    let assets = real_assets().expect(concat!("test fixture is missing: ", stringify!(real_assets())));

    let mut flagged = Vec::new();

    for index in 0..UNIT_END {
        let unit_type = UnitType::try_from(index).expect("тип");
        let needs_turret = access_flags_for_tests(unit_type)
            & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET)
            != 0;
        let Some(sprite) = assets.sprite(unit_type) else {
            continue;
        };

        if !needs_turret {
            assert!(
                sprite.turret_frame(0).is_none() || sprite.frame_info.is_none(),
                    "{unit_type:?}: башня без флага TURRET_SPRITE"
            );
            continue;
        }

        let info = sprite
            .frame_info
            .unwrap_or_else(|| panic!("{unit_type:?}: нет D_* для башни"));
        assert!(
            info.turret_image_count > 0,
                "{unit_type:?}: turret_image_count = 0"
        );

        for angle in 0..8u8 {
            assert!(
                sprite.turret_frame(angle).is_some(),
                    "{unit_type:?}: нет кадра башни для угла {angle}"
            );
            assert_eq!(
                sprite.turret_offset(angle),
                info.turret_offset(angle),
                    "{unit_type:?}: сдвиг башни для угла {angle}"
            );
        }

        flagged.push(unit_type);
    }

    assert_eq!(
        flagged.len(),
        11,
            "TURRET_SPRITE/SPINNING_TURRET: {flagged:?}"
    );
    for required in [
        UnitType::GunTurret,
        UnitType::AntiAir,
        UnitType::Artillery,
        UnitType::MissileLauncher,
        UnitType::Tank,
        UnitType::MobileAntiAir,
        UnitType::Escort,
        UnitType::Gunboat,
        UnitType::AlienTank,
        UnitType::Scanner,
        UnitType::Awac,
    ] {
        assert!(flagged.contains(&required), "{required:?}");
    }
}

/// Type flags from `SC_UNITS` (`access::unit_type_flags`).
fn access_flags_for_tests(unit_type: UnitType) -> u32 {
    max_core::access::unit_type_flags(unit_type)
}

/// Unique temporary directory of the test.
fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
            "max_app_assets_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Path inside the repository (`max-brh/crates/max_app/../../..`).
fn repo_path(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

#[test]
fn configured_patches_res_wins_over_any_fallback() {
    let dir = temp_dir("direct");
    let patches = dir.join("PATCHES.RES");
    std::fs::write(&patches, b"RES0").expect("write PATCHES.RES");

    let config = AppConfig {
        patches_res: patches.clone(),
        ..Default::default()
    };

    let (path, warning) = resolve_patches_res(&config);
    assert_eq!(path, patches);
    assert!(warning.is_none(), "без fallback предупреждения нет");

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn missing_patches_res_falls_back_to_the_server_template() {
    let dir = temp_dir("template");
    let data = dir.join("max-original");
    std::fs::create_dir_all(&data).expect("data dir");
    let max_res = data.join("MAX.RES");
    std::fs::write(&max_res, b"RES0").expect("write MAX.RES");

    let fallback = dir.join("bin/linux-server-template/PATCHES.RES");
    std::fs::create_dir_all(fallback.parent().expect("parent")).expect("template dir");
    std::fs::write(&fallback, b"RES0fallback").expect("write fallback");

    let missing = dir.join("nowhere/PATCHES.RES");
    let config = AppConfig {
        max_res,
        patches_res: missing.clone(),
        ..Default::default()
    };

    let (path, warning) = resolve_patches_res(&config);
    assert_eq!(path, fallback, "шаблон headless-сервера — первый fallback");
    assert!(
        warning.as_deref().unwrap_or("").contains("fallback"),
            "о подмене нужно предупредить: {warning:?}"
    );
    assert_eq!(
        config.patches_res, missing,
            "config не меняется (хэш отдельно)"
    );

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn missing_patches_res_falls_back_to_the_repository_assets_dir() {
    let dir = temp_dir("assets");
    let data = dir.join("max-original");
    std::fs::create_dir_all(&data).expect("data dir");
    let max_res = data.join("MAX.RES");
    std::fs::write(&max_res, b"RES0").expect("write MAX.RES");

    let fallback = dir.join("assets/PATCHES.RES");
    std::fs::create_dir_all(fallback.parent().expect("parent")).expect("assets dir");
    std::fs::write(&fallback, b"RES0assets").expect("write fallback");

    let config = AppConfig {
        max_res,
        patches_res: dir.join("nowhere/PATCHES.RES"),
        ..Default::default()
    };

    let (path, warning) = resolve_patches_res(&config);
    assert_eq!(path, fallback, "без шаблона берётся assets/PATCHES.RES");
    assert!(warning.is_some());

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn absent_everywhere_keeps_the_configured_path() {
    // The test is hermetic: with TC-1109 the ambient resolver also looks next to the exe,
    // and the test binary lies inside the checkout, where `assets/PATCHES.RES`
    // really exists. "Nowhere" is checked on empty roots.
    let dir = temp_dir("nowhere");
    let config = AppConfig {
        max_res: dir.join("missing/MAX.RES"),
        patches_res: dir.join("missing/PATCHES.RES"),
        color_pal: dir.join("missing/COLOR.PAL"),
        ..Default::default()
    };
    let roots = vec![dir.clone()];

    let (path, warning) = resolve_patches_res_at(&config, &roots);
    assert_eq!(path, config.patches_res);
    assert!(warning.is_none());

    let (path, warning) = resolve_max_res_at(&config, &roots);
    assert_eq!(path, config.max_res);
    assert!(warning.is_none());

    let (path, warning) = resolve_color_pal_at(&config, &roots);
    assert_eq!(path, config.color_pal);
    assert!(warning.is_none());

    std::fs::remove_dir_all(dir).ok();
}

/// TC-1109: `MAX.RES`/`PATCHES.RES`/`COLOR.PAL` are found from the exe directory
/// when the CWD is a foreign empty directory (launching `bin\release\max_app.exe`).
#[test]
fn exe_dir_resolves_game_data_when_cwd_is_elsewhere() {
    let root = temp_dir("exe");
    let cwd = root.join("cwd");
    let exe_dir = root.join("release");
    let data = exe_dir.join(DATA_DIR_NAME);
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&data).expect("data dir");
    std::fs::create_dir_all(exe_dir.join("assets")).expect("assets dir");

    let max_res = data.join(MAX_RES_NAME);
    let palette = data.join(COLOR_PAL_NAME);
    let patches = exe_dir.join("assets/PATCHES.RES");
    std::fs::write(&max_res, b"RES0").expect("write MAX.RES");
    std::fs::write(&palette, b"palette").expect("write COLOR.PAL");
    std::fs::write(&patches, b"RES0").expect("write PATCHES.RES");

    let config = AppConfig {
        max_res: cwd.join("max-original/MAX.RES"),
        color_pal: cwd.join("max-original/COLOR.PAL"),
        patches_res: cwd.join("assets/PATCHES.RES"),
        ..Default::default()
    };
    let exe = exe_dir.join("max_app.exe");
    let roots = data_search_roots_from(&config, Some(&cwd), Some(&exe));

    let (path, warning) = resolve_max_res_at(&config, &roots);
    assert_eq!(path, max_res, "MAX.RES от каталога exe");
    assert!(
        warning.as_deref().unwrap_or("").contains("MAX.RES"),
            "предупреждение о подмене: {warning:?}"
    );

    let (path, warning) = resolve_color_pal_at(&config, &roots);
    assert_eq!(path, palette, "COLOR.PAL от каталога exe");
    assert!(warning.is_some());

    let (path, warning) = resolve_patches_res_at(&config, &roots);
    assert_eq!(path, patches, "PATCHES.RES от каталога exe");
    assert!(warning.is_some());

    // The config is not mutated: the handshake computes the path from `config`.
    assert_eq!(config.max_res, cwd.join("max-original/MAX.RES"));

    std::fs::remove_dir_all(root).ok();
}

/// TC-1109: if the exe lies deeper (`bin/`), the data is searched upward to the root
/// with `max-original/` — but not above it (without a find we do not go up).
#[test]
fn exe_ancestors_are_searched_up_to_the_data_root() {
    let root = temp_dir("exe-up");
    let bin = root.join("release/bin");
    let data = root.join(DATA_DIR_NAME);
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::create_dir_all(&data).expect("data dir");
    std::fs::write(data.join(MAX_RES_NAME), b"RES0").expect("write MAX.RES");

    let exe = bin.join("max_app.exe");
    let roots = exe_search_roots(&exe);

    assert_eq!(roots.first(), Some(&bin), "первый корень — сам каталог exe");
    assert!(
        roots.iter().any(|candidate| *candidate == root),
            "корень с max-original должен войти в поиск: {roots:?}"
    );

    let config = AppConfig {
        max_res: bin.join("max-original/MAX.RES"),
        ..Default::default()
    };
    let (path, _) = resolve_max_res_at(&config, &roots);
    assert_eq!(path, data.join(MAX_RES_NAME));

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn missing_color_pal_falls_back_next_to_max_res() {
    let dir = temp_dir("palette");
    let data = dir.join("max-original");
    std::fs::create_dir_all(&data).expect("data dir");
    let max_res = data.join("MAX.RES");
    std::fs::write(&max_res, b"RES0").expect("write MAX.RES");
    let palette = data.join("COLOR.PAL");
    std::fs::write(&palette, b"palette").expect("write COLOR.PAL");

    let config = AppConfig {
        max_res,
        color_pal: dir.join("nowhere/COLOR.PAL"),
        ..Default::default()
    };

    let (path, warning) = resolve_color_pal(&config);
    assert_eq!(path, palette);
    assert!(warning.as_deref().unwrap_or("").contains("COLOR.PAL"));
    assert_eq!(config.color_pal, dir.join("nowhere/COLOR.PAL"));

    std::fs::remove_dir_all(dir).ok();
}

/// Assets load from the fallback, while `patches_res` remains the handshake path.
///
/// Requires the game data next to the repository (like
/// `assets_integration_smoke` in `tests/app.rs`); a missing data set fails the test.
#[test]
fn assets_load_from_fallback_when_configured_patches_res_is_missing() {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");

    assert!(max_res.is_file() && color_pal.is_file(), concat!("required test fixture is missing: ", stringify!(!max_res.is_file() || !color_pal.is_file())));

    let missing = repo_path("build/does-not-exist/PATCHES.RES");
    let config = AppConfig {
        max_res,
        color_pal: repo_path("max-original/does-not-exist.COLOR.PAL"),
        patches_res: missing.clone(),
        ..Default::default()
    };

    let assets = Assets::load(&config).expect("ассеты из fallback");
    let report = assets.report();

    assert!(
        report.patches_res_entries > 0,
            "PATCHES.RES должен загрузиться из fallback"
    );
    assert_eq!(
        config.patches_res, missing,
            "путь хэндшейка не подменяется fallback'ом"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("fallback")),
            "предупреждение о fallback должно попасть в отчёт: {:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("COLOR.PAL")),
            "палитра тоже должна резолвиться рядом с MAX.RES: {:?}",
        report.warnings
    );
}
