use super::*;

const DOC: &str = r#"{
        "subtitle": {
            "en-US": [
                { "start": 144, "end": 245, "caption": "First" },
                { "start": 249, "end": 322, "caption": "Second" },
                { "start": 400, "end": 400, "caption": "Skipped: empty range" },
                { "start": 500, "caption": "Skipped: no end" }
            ],
            "ru-RU": [
                { "start": 10, "end": 20, "caption": "Первый" }
            ]
        }
    }"#;

#[test]
fn looks_up_captions_like_the_oracle() {
    let track = SubtitleTrack::parse_json(DOC, "en-US").expect("track");

    assert_eq!(track.len(), 2);
    assert_eq!(track.language(), "en-US");
    assert_eq!(track.caption_for_frame(0), None);
    assert_eq!(track.caption_for_frame(143), None);
    assert_eq!(track.caption_for_frame(144), Some("First"));
    assert_eq!(track.caption_for_frame(244), Some("First"));
    // `end` is exclusive.
    assert_eq!(track.caption_for_frame(245), None);
    assert_eq!(track.caption_for_frame(250), Some("Second"));
    assert_eq!(track.caption_for_frame(322), None);
}

#[test]
fn picks_the_requested_language_and_falls_back_to_english() {
    let russian = SubtitleTrack::parse_json(DOC, "ru-RU").expect("ru");
    assert_eq!(russian.language(), "ru-RU");
    assert_eq!(russian.caption_for_frame(15), Some("Первый"));

    let unknown = SubtitleTrack::parse_json(DOC, "de-DE").expect("fallback");
    assert_eq!(unknown.language(), "en-US");
}

#[test]
fn rejects_documents_without_a_usable_language() {
    let no_subtitle = r#"{ "author": {} }"#;
    assert!(matches!(
        SubtitleTrack::parse_json(no_subtitle, "en-US"),
        Err(SubtitleError::Json(_))
    ));

    let other_language =
            r#"{ "subtitle": { "fr-FR": [ { "start": 1, "end": 2, "caption": "x" } ] } }"#;
    assert!(matches!(
        SubtitleTrack::parse_json(other_language, "en-US"),
        Err(SubtitleError::NoLanguage { .. })
    ));

    let empty = r#"{ "subtitle": { "en-US": [] } }"#;
    assert!(matches!(
        SubtitleTrack::parse_json(empty, "en-US"),
        Err(SubtitleError::Empty)
    ));

    assert!(matches!(
        SubtitleTrack::parse_json("{", "en-US"),
        Err(SubtitleError::Json(_))
    ));
}

#[test]
fn missing_file_is_reported_and_does_not_panic() {
    let path = std::env::temp_dir().join("max_app_subtitles_missing/maxint.json");
    let error = SubtitleTrack::load_file(&path, "en-US").expect_err("no file");

    assert!(matches!(error, SubtitleError::NoFile { .. }));
    assert!(error.to_string().contains("maxint.json"));
}

#[test]
fn loads_a_sidecar_json_file() {
    let dir = std::env::temp_dir().join(format!(
            "max_app_subtitles_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("maxint.json");
    std::fs::write(&path, DOC).expect("write");

    let track = SubtitleTrack::load_file(&path, "en-US").expect("track");

    assert_eq!(track.len(), 2);
    assert_eq!(track.source(), Some(path.as_path()));

    std::fs::remove_dir_all(dir).ok();
}

/// The real `MAX.RES` does not ship `INTROSUB` (the port keeps the JSON next to the
/// game data), so the loader must report the absence instead of failing the movie.
#[test]
fn real_max_res_without_intro_sub_reports_the_fallback() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original");
    let max_res = dir.join("MAX.RES");

    assert!(max_res.is_file(), concat!("required test fixture is missing: ", stringify!(!max_res.is_file())));

    let res = ResArchive::open(&max_res).expect("MAX.RES");

    // Either no resource at all, or a reference whose file is absent — both are a
    // visible fallback, never a panic.
    match SubtitleTrack::load(&res, &dir, DEFAULT_LANGUAGE) {
        Ok(track) => assert!(!track.is_empty()),
        Err(error) => assert!(
            matches!(
                error,
                SubtitleError::NoResource
                    | SubtitleError::NoReference
                    | SubtitleError::NoFile { .. }
            ),
                "{error}"
        ),
    }
}
