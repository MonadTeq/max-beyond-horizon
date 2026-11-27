//! Unit tests of the remaster module (private invariants and fallback
//! semantics). The real `.ktx2` decode golden lives in `tests/remaster.rs`
//! (public API), per `max-brh-docs/development/test-organization.md`.

use super::*;

fn manifest(json: &str) -> RemasterManifest {
    RemasterManifest::parse(json).unwrap()
}

#[test]
fn manifest_round_trips_through_serde() {
    let json = r#"{
        "version": 1,
        "assets": {
            "terrain_tile": {
                "path": "terrain_tile_ETC1S.ktx2",
                "scale": 2,
                "palette": "GREEN_1",
                "recolor": null,
                "shadow": null,
                "hash": "sha256:deadbeef"
            }
        }
    }"#;

    let parsed = manifest(json);
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.assets.len(), 1);

    let entry = parsed.get("terrain_tile").expect("tag must be listed");
    assert_eq!(entry.path, "terrain_tile_ETC1S.ktx2");
    assert_eq!(entry.scale, 2);
    assert_eq!(entry.palette.as_deref(), Some("GREEN_1"));
    assert_eq!(entry.recolor, None);
    assert_eq!(entry.shadow, None);
    assert_eq!(entry.hash.as_deref(), Some("sha256:deadbeef"));

    // Serialize → parse must be the identity (stable manifest round-trip).
    let reserialized = serde_json::to_string_pretty(&parsed).unwrap();
    assert_eq!(manifest(&reserialized), parsed);
}

#[test]
fn manifest_defaults_version_scale_and_optional_fields() {
    let parsed = manifest(r#"{"assets":{"t":{"path":"a.ktx2"}}}"#);

    assert_eq!(parsed.version, 1, "missing `version` defaults to MANIFEST_VERSION");
    let entry = parsed.get("t").unwrap();
    assert_eq!(entry.scale, 1, "missing `scale` defaults to 1");
    assert_eq!(entry.palette, None);
    assert_eq!(entry.recolor, None);
    assert_eq!(entry.shadow, None);
    assert_eq!(entry.hash, None);

    let empty = manifest(r#"{"assets":{}}"#);
    assert!(empty.is_empty());
}

#[test]
fn manifest_ignores_unknown_fields_for_forward_compatibility() {
    // R2–R4 may add fields; an older loader must not reject them.
    let parsed = manifest(r#"{"version":1,"future":"x","assets":{"t":{"path":"a.ktx2","mipmap":true}}}"#);

    assert!(parsed.get("t").is_some());
}

#[test]
fn manifest_rejects_invalid_json_and_wrong_types() {
    assert!(matches!(
        RemasterManifest::parse("not json"),
        Err(RemasterError::InvalidManifest(_))
    ));
    // `scale` must be a number, not a string.
    assert!(matches!(
        RemasterManifest::parse(r#"{"assets":{"t":{"path":"a.ktx2","scale":"2"}}}"#),
        Err(RemasterError::InvalidManifest(_))
    ));
    // A non-object `assets` is a schema violation.
    assert!(matches!(
        RemasterManifest::parse(r#"{"assets":[]}"#),
        Err(RemasterError::InvalidManifest(_))
    ));
}

#[test]
fn decode_ktx2_rejects_garbage_without_panicking() {
    let entry = RemasterEntry {
        path: "garbage.ktx2".to_string(),
        scale: 1,
        palette: None,
        recolor: None,
        shadow: None,
        hash: None,
    };

    for bytes in [&[][..], b"not a basis container", b"\xAB\xBA\xB1\x1C KTX 20"] {
        assert!(
            matches!(decode_ktx2(bytes, &entry), Err(RemasterError::Transcode(_))),
            "garbage input must be a Transcode error, not a panic"
        );
    }
}

#[test]
fn open_on_missing_root_yields_empty_cache() {
    // A directory that does not exist (hence no manifest) must not fail.
    let cache = RemasterCache::open(std::path::Path::new("__no_such_remaster_root__"));
    assert!(cache.manifest().is_empty());
    assert_eq!(cache.resolve("anything"), None);
}

#[test]
fn terrain_tile_tag_is_stable_and_scale_specific() {
    // The R3 contract: the renderer resolves exactly this string from the
    // manifest, and a ×2 and a ×4 bank cannot collide.
    assert_eq!(
        terrain_tile_tag("GREEN_1", 0, 2),
        "terrain_tile_GREEN_1_000_x2"
    );
    assert_eq!(
        terrain_tile_tag("GREEN_1", 411, 4),
        "terrain_tile_GREEN_1_411_x4"
    );
    assert_ne!(
        terrain_tile_tag("GREEN_1", 7, 2),
        terrain_tile_tag("GREEN_1", 7, 4),
        "a ×2 and a ×4 asset must be different manifest entries"
    );
    // Two maps with the same tile bank are different palettes, hence different
    // textures (R0 §2: the unit is the `(palette, tile pixels)` pair).
    assert_ne!(
        terrain_tile_tag("GREEN_1", 7, 2),
        terrain_tile_tag("SNOW_1", 7, 2)
    );
}

#[test]
fn terrain_tile_tag_is_usable_as_a_manifest_key_and_file_stem() {
    let tag = terrain_tile_tag("SNOW_1", 337, 4);

    assert!(
        tag.chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_'),
        "the tag becomes a manifest key and a file name: {tag}"
    );

    let manifest = manifest(&format!(
        r#"{{"assets":{{"{tag}":{{"path":"{tag}.etc1s.ktx2","scale":4}}}}}}"#
    ));
    assert_eq!(manifest.get(&tag).map(|entry| entry.scale), Some(4));
}

#[test]
fn resolve_missing_entry_and_missing_file_are_none() {
    let root = std::path::Path::new("__no_such_remaster_root__");
    let manifest = RemasterManifest::parse(
        r#"{"assets":{"ghost":{"path":"ghost.ktx2","scale":2}}}"#,
    )
    .unwrap();

    // Unknown tag → None.
    let cache = RemasterCache::with_manifest(root, manifest.clone());
    assert_eq!(cache.resolve("unknown_tag"), None);
    // Known tag but the `.ktx2` file does not exist → None (no panic).
    assert_eq!(cache.resolve("ghost"), None);
}

#[test]
fn chrome_image_tags_are_stable_and_scale_specific() {
    // The R5 contract: `max_render::hd_chrome` resolves exactly these strings,
    // so a ×2 and a ×4 chrome texture must not collide.
    assert_eq!(chrome_image_tag("FRAMEPIC", 2), "chrome_FRAMEPIC_x2");
    assert_eq!(chrome_image_tag("PANELTOP", 4), "chrome_PANELTOP_x4");
    assert_eq!(
        chrome_image_tag("ENDTURN_U", 2),
        "chrome_ENDTURN_U_x2",
        "buttons and menu images use the MAX.RES name verbatim"
    );
    assert_ne!(
        chrome_image_tag("FRAMEPIC", 2),
        chrome_image_tag("FRAMEPIC", 4),
        "a ×2 and a ×4 chrome texture must be different manifest entries"
    );
    assert_ne!(
        chrome_image_tag("PANELTOP", 2),
        chrome_image_tag("PANELBTM", 2),
        "the two panel halves are separate images"
    );
}

#[test]
fn ui_skin_tags_are_stable_and_scale_specific() {
    // The R5 contract: every `UiBox::name()` maps to `ui_<part>_x<S>`.
    assert_eq!(ui_skin_tag("button", 2), "ui_button_x2");
    assert_eq!(ui_skin_tag("button_hover", 4), "ui_button_hover_x4");
    assert_ne!(
        ui_skin_tag("button", 2),
        ui_skin_tag("button", 4),
        "a ×2 and a ×4 skin must be different manifest entries"
    );
    assert_ne!(
        ui_skin_tag("button", 2),
        ui_skin_tag("button_hover", 2),
        "the button states are separate skins"
    );
}

#[test]
fn chrome_and_ui_tags_are_usable_as_manifest_keys_and_file_stems() {
    for tag in [
        chrome_image_tag("FRAMEPIC", 2),
        chrome_image_tag("PANELBTM", 4),
        ui_skin_tag("panel", 2),
    ] {
        assert!(
            tag.chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_'),
            "the tag becomes a manifest key and a file name: {tag}"
        );

        let manifest = manifest(&format!(
            r#"{{"assets":{{"{tag}":{{"path":"{tag}.uastc.ktx2","scale":2}}}}}}"#
        ));
        assert_eq!(manifest.get(&tag).map(|entry| entry.scale), Some(2));
    }
}

#[test]
fn sprite_tags_are_stable_and_scale_and_frame_specific() {
    // The R4 contract: `max_render::hd_sprites` resolves exactly these strings,
    // so a ×2 and a ×4 bank must not collide, and neither must two frames.
    assert_eq!(sprite_frame_tag("TANK", 0, 2), "unit_sprite_TANK_000_x2");
    assert_eq!(sprite_frame_tag("TANK", 104, 4), "unit_sprite_TANK_104_x4");
    assert_eq!(sprite_shadow_tag("TANK", 0, 2), "unit_shadow_TANK_000_x2");
    assert_ne!(
        sprite_frame_tag("TANK", 7, 2),
        sprite_frame_tag("TANK", 7, 4),
        "a ×2 and a ×4 asset must be different manifest entries"
    );
    assert_ne!(
        sprite_frame_tag("TANK", 7, 2),
        sprite_frame_tag("TANK", 8, 2),
        "every frame is a separate index map"
    );
    assert_ne!(
        sprite_frame_tag("TANK", 7, 2),
        sprite_shadow_tag("TANK", 7, 2),
        "the body and its shadow are separate assets"
    );
    assert_ne!(
        sprite_frame_tag("TANK", 7, 2),
        sprite_frame_tag("COMMTWR", 7, 2),
        "different resources are different sprites"
    );
}

#[test]
fn sprite_tags_are_usable_as_manifest_keys_and_file_stems() {
    for tag in [
        sprite_frame_tag("SUBMARNE", 15, 2),
        sprite_shadow_tag("SUBMARNE", 15, 4),
    ] {
        assert!(
            tag.chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_'),
            "the tag becomes a manifest key and a file name: {tag}"
        );

        let manifest = manifest(&format!(
            r#"{{"assets":{{"{tag}":{{"path":"{tag}.uastc.ktx2","scale":2}}}}}}"#
        ));
        assert_eq!(manifest.get(&tag).map(|entry| entry.scale), Some(2));
    }
}
