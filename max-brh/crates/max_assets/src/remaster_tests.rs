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
