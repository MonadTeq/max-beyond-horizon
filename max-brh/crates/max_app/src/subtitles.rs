//! `INTROSUB` subtitles of the `Introduction` movie (`moviesubtitle.cpp`).
//!
//! The original does not keep the text in `MAX.RES`: the `INTROSUB` resource body is an
//! ASCIIZ file name of a JSON document next to `MAX.RES`
//! (`ResourceManager_FilePathGameBase / filename`, `moviesubtitle.cpp:74-100`).
//! The document maps a language tag to a list of captions:
//!
//! ```json
//! { "subtitle": { "en-US": [ { "start": 144, "end": 245, "caption": "…" } ] } }
//! ```
//!
//! `start`/`end` are MVE frame numbers (half-open range, `end` excluded), exactly the
//! oracle's [`MovieSubtitle::GetSubtitleForFrame`] lookup: the entry with the largest
//! `start <= frame` wins, and only if `frame < end`.
//!
//! The text is only data here: drawing lives in [`crate::intro`] (the oracle also keeps
//! rendering in `moviesubtitle.cpp`, but our layer does not touch the screen).
//!
//! # Fallback
//!
//! Subtitles are optional in every path: a missing resource, a missing file, broken JSON
//! or an unknown language return [`SubtitleError`] — the movie keeps playing without
//! captions, and the caller reports the reason (requirement "fallback is visible").
//!
//! [`MovieSubtitle::GetSubtitleForFrame`]: https://github.com/klei1984/max/blob/master/src/moviesubtitle.cpp

use std::fmt;
use std::path::{Path, PathBuf};

use max_assets::ResArchive;
use serde_json::Value;

/// Resource whose body references the subtitle JSON file (`ResourceID` `SCRIPT_E`).
pub const SUBTITLE_RESOURCE: &str = "INTROSUB";
/// Language tag used when the requested one is absent (the oracle's `en-US` fallback).
pub const FALLBACK_LANGUAGE: &str = "en-US";
/// A language tag of the documents the port ships (`ResourceManager_GetSystemLocale`).
pub const DEFAULT_LANGUAGE: &str = "en-US";

/// One caption: frames `[start_frame, end_frame)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleEntry {
    /// First frame the caption is shown on.
    pub start_frame: usize,
    /// First frame after the caption (`end` in the JSON, exclusive).
    pub end_frame: usize,
    /// Caption text.
    pub caption: String,
}

impl SubtitleEntry {
    /// Whether the caption is shown on `frame`.
    pub fn covers(&self, frame: usize) -> bool {
        frame >= self.start_frame && frame < self.end_frame
    }
}

/// Parsed subtitle track of one movie.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleTrack {
    entries: Vec<SubtitleEntry>,
    language: String,
    source: Option<PathBuf>,
}

impl SubtitleTrack {
    /// Number of captions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the track has no captions.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Language actually loaded (the requested one or [`FALLBACK_LANGUAGE`]).
    pub fn language(&self) -> &str {
        &self.language
    }

    /// File the captions were read from (for diagnostics/tests).
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// The captions (sorted by `start_frame`).
    pub fn entries(&self) -> &[SubtitleEntry] {
        &self.entries
    }

    /// The caption of `frame`, if any — `MovieSubtitle::GetSubtitleForFrame`.
    pub fn entry_for_frame(&self, frame: usize) -> Option<&SubtitleEntry> {
        // `upper_bound` over `start_frame`, then one step back — like the oracle.
        let index = self
            .entries
            .partition_point(|entry| entry.start_frame <= frame);

        if index == 0 {
            return None;
        }

        let entry = &self.entries[index - 1];

        entry.covers(frame).then_some(entry)
    }

    /// The caption text of `frame`, if any.
    pub fn caption_for_frame(&self, frame: usize) -> Option<&str> {
        self.entry_for_frame(frame)
            .map(|entry| entry.caption.as_str())
    }

    /// Parses the subtitle JSON and picks the requested language.
    ///
    /// # Errors
    ///
    /// [`SubtitleError::Json`] — the document is not valid JSON or has no `subtitle`
    /// object; [`SubtitleError::NoLanguage`] — neither the requested language nor
    /// [`FALLBACK_LANGUAGE`] is present; [`SubtitleError::Empty`] — the list has no
    /// usable entry (`start < end`).
    pub fn parse_json(json: &str, language: &str) -> Result<Self, SubtitleError> {
        let document: Value =
            serde_json::from_str(json).map_err(|error| SubtitleError::Json(error.to_string()))?;
        let subtitle = document
            .get("subtitle")
            .and_then(Value::as_object)
            .ok_or_else(|| SubtitleError::Json("нет объекта \"subtitle\"".to_string()))?;

        let selected = if subtitle.contains_key(language) {
            language
        } else if subtitle.contains_key(FALLBACK_LANGUAGE) {
            FALLBACK_LANGUAGE
        } else {
            let mut available: Vec<&str> = subtitle.keys().map(String::as_str).collect();

            available.sort_unstable();

            return Err(SubtitleError::NoLanguage {
                language: language.to_string(),
                available: available.join(", "),
            });
        };

        let list = subtitle
            .get(selected)
            .and_then(Value::as_array)
            .ok_or_else(|| SubtitleError::Json(format!("язык {selected:?} — не массив")))?;
        let mut entries = Vec::with_capacity(list.len());

        for item in list {
            let start = item.get("start").and_then(Value::as_u64);
            let end = item.get("end").and_then(Value::as_u64);
            let caption = item.get("caption").and_then(Value::as_str);

            let (Some(start), Some(end), Some(caption)) = (start, end, caption) else {
                // The oracle logs and skips malformed entries (moviesubtitle.cpp:145-159).
                continue;
            };

            let (start_frame, end_frame) = (start as usize, end as usize);

            if start_frame >= end_frame {
                continue;
            }

            entries.push(SubtitleEntry {
                start_frame,
                end_frame,
                caption: caption.to_string(),
            });
        }

        entries.sort_by_key(|entry| entry.start_frame);

        if entries.is_empty() {
            return Err(SubtitleError::Empty);
        }

        Ok(Self {
            entries,
            language: selected.to_string(),
            source: None,
        })
    }

    /// Loads the track through the `INTROSUB` resource of `MAX.RES`.
    ///
    /// `dir` is the game directory next to `MAX.RES` (the oracle's `FilePathGameBase`).
    ///
    /// # Errors
    ///
    /// [`SubtitleError::NoResource`] — `MAX.RES` has no `INTROSUB`;
    /// [`SubtitleError::NoReference`] — the body is not an ASCIIZ file name;
    /// [`SubtitleError::NoFile`] — the referenced file is absent next to `MAX.RES`;
    /// [`SubtitleError::Read`]/[`SubtitleError::Json`]/[`SubtitleError::NoLanguage`] —
    /// unreadable/broken content.
    pub fn load(res: &ResArchive, dir: &Path, language: &str) -> Result<Self, SubtitleError> {
        let Some(reference) = res.file_reference(SUBTITLE_RESOURCE) else {
            if res.read(SUBTITLE_RESOURCE).is_ok() {
                return Err(SubtitleError::NoReference);
            }

            return Err(SubtitleError::NoResource);
        };
        let file = dir.join(reference);
        let json = std::fs::read_to_string(&file).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SubtitleError::NoFile { file: file.clone() }
            } else {
                SubtitleError::Read {
                    file: file.clone(),
                    error: error.to_string(),
                }
            }
        })?;
        let mut track = Self::parse_json(&json, language)?;

        track.source = Some(file);

        Ok(track)
    }

    /// Loads a track from a JSON file directly (a sidecar next to `MAX.RES`).
    ///
    /// This is the Rust-side fallback for the game directories that do not ship the
    /// `INTROSUB` reference or the file: a `maxint.json` next to `MAXINT.MVE` is picked
    /// up when present (the oracle's asset layout).
    pub fn load_file(path: &Path, language: &str) -> Result<Self, SubtitleError> {
        let json = std::fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SubtitleError::NoFile {
                    file: path.to_path_buf(),
                }
            } else {
                SubtitleError::Read {
                    file: path.to_path_buf(),
                    error: error.to_string(),
                }
            }
        })?;
        let mut track = Self::parse_json(&json, language)?;

        track.source = Some(path.to_path_buf());

        Ok(track)
    }
}

/// Why subtitles are not available (the movie still plays).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleError {
    /// `MAX.RES` has no `INTROSUB` resource.
    NoResource,
    /// `INTROSUB` is not a file-name reference (regular data resource).
    NoReference,
    /// The referenced file does not exist next to `MAX.RES`.
    NoFile {
        /// Expected path.
        file: PathBuf,
    },
    /// The file exists but cannot be read.
    Read {
        /// Path that failed.
        file: PathBuf,
        /// OS reason.
        error: String,
    },
    /// Broken JSON / missing `subtitle` object / a language is not a list.
    Json(String),
    /// Neither the requested language nor the `en-US` fallback is present.
    NoLanguage {
        /// Requested language.
        language: String,
        /// Languages of the document (comma separated).
        available: String,
    },
    /// The language list has no usable caption.
    Empty,
}

impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoResource => write!(f, "{SUBTITLE_RESOURCE} нет в MAX.RES"),
            Self::NoReference => write!(
                f,
                "{SUBTITLE_RESOURCE} не является ссылкой на файл субтитров"
            ),
            Self::NoFile { file } => write!(f, "нет файла субтитров {}", file.display()),
            Self::Read { file, error } => {
                write!(f, "не удалось прочитать {}: {error}", file.display())
            }
            Self::Json(error) => write!(f, "JSON субтитров: {error}"),
            Self::NoLanguage {
                language,
                available,
            } => write!(
                f,
                "нет языка {language:?} и фолбэка {FALLBACK_LANGUAGE:?} (есть: {available})"
            ),
            Self::Empty => f.write_str("в субтитрах нет пригодных записей"),
        }
    }
}

impl std::error::Error for SubtitleError {}

#[cfg(test)]
mod tests {
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

        if !max_res.is_file() {
            return;
        }

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
}
