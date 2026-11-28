//! Distribution file inventory — the exact `data-inventory.json` manifest.
//!
//! Stage 1 of `max-brh-docs/development/distro-installers.md` §F. [`build`]
//! walks one game data directory and produces the file manifest of the
//! standalone distribution: every shipped file with its SHA-256 digest and its
//! distribution class ([`AssetClass`]). The manifest is deterministic: entries
//! are sorted by path and contain no timestamps, absolute paths or
//! directory-order dependencies, so two runs over the same data produce
//! byte-identical JSON.
//!
//! The file set is derived from the loaders, not from a directory listing
//! (plan §A — "exact inventory of runtime dependencies"):
//!
//! * the required core files `MAX.RES`, `PATCHES.RES`, `COLOR.PAL`;
//! * every file reference of `MAX.RES` and `PATCHES.RES`: such a record's body
//!   is an ASCIIZ file name (`max-original-dasm/formats/res.md` §5). The retail
//!   `MAX.RES` has 721 references to 567 distinct files: 607 `*.spw`,
//!   71 `*.flc`, 24 `*.wrl`, 16 `*.msc`, 3 `*.mve`; `PATCHES.RES` has one
//!   (`INTROSUB` → `maxint.json`);
//! * the mission set: files referenced by the `SC_*` mission registry, the
//!   `DESCR*` mission briefings and the campaign epilogue `WIN9.CAM`;
//! * the `maxint.json` subtitles sidecar.
//!
//! Everything else in the game directory (DOS binaries, DOSBox, GOG metadata,
//! `MAX.INI`, `MANUAL.PDF`, user saves) is deliberately not shipped and is
//! therefore absent from the manifest.
//!
//! # Explicitly allowlisted gaps
//!
//! The retail GOG data set does not ship three music tracks and two demo
//! mission files, although `MAX.RES`/`PATCHES.RES` still reference them. They
//! are reported under [`DataInventory::allowlisted_missing`] instead of
//! [`DataInventory::missing`]:
//!
//! * `BKG7_MSC`, `BKG8_MSC`, `BKG9_MSC` — see [`ALLOWLISTED_MISSING_MUSIC`];
//! * `SAVE2.DMO`, `SAVE3.DMO` — see [`ALLOWLISTED_MISSING_MISSIONS`].
//!
//! `MAX.RES` also keeps legacy sound tags whose `*.spw` files the retail data
//! set does not ship (182 distinct files). The game never opens a file it has
//! no tag for at runtime, and the v1.04 data set works without them, so
//! unresolved sound references are counted as
//! [`InventorySources::max_res_unresolved_references`] and do not make the
//! manifest incomplete. A missing `*.wrl`, `*.flc`, `*.mve`, `*.msc` (outside
//! the allowlist) or `*.json` reference is a required-file failure.
//!
//! # Layer rule
//!
//! The music track lists duplicate `max_app::music` (the inventory tool is a
//! `max_assets` example, and `max_assets` is the lower layer, so it cannot
//! depend on `max_app`). `max_app/tests/music.rs` pins both lists together, so
//! a playlist change cannot silently skip a track.
//!
//! # Oracle
//!
//! The reference and mission counts above are the documented ones
//! (`max-original-dasm/formats/spw.md` §1, `max-brh-docs/development/
//! campaign-missions.md`); `tests/asset_inventory.rs` pins them against the
//! real data set.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::mission::{MissionCategory, MissionRegistry};
use crate::res::{parse_file_ref, ResArchive, ResError};

/// Name of the generated manifest in the working directory.
pub const INVENTORY_FILE: &str = "data-inventory.json";
/// Manifest schema version (incremented on incompatible field changes).
pub const INVENTORY_SCHEMA: u32 = 1;

/// Required core archive with the game data.
const MAX_RES_FILE: &str = "MAX.RES";
/// Required mission/unit archive.
const PATCHES_RES_FILE: &str = "PATCHES.RES";
/// Required palette file.
const COLOR_PAL_FILE: &str = "COLOR.PAL";
/// Subtitles sidecar referenced by `PATCHES.RES:INTROSUB`.
const SUBTITLES_FILE: &str = "maxint.json";
/// Campaign epilogue (prose, the DOS-era ending screen).
const CAMPAIGN_EPILOGUE_FILE: &str = "WIN9.CAM";

/// Longest body accepted as an ASCIIZ file reference.
///
/// A reference record stores a bare file name (`"tank1.spw\0"`, 10 bytes;
/// `max-original-dasm/formats/res.md` §5); `spw.md` §1 counts the references
/// the same way. Anything longer is a payload, not a name.
const MAX_REFERENCE_BYTES: u32 = 64;

/// The match shuffle playlist — copy of `max_app::music::GAME_PLAYLIST`.
pub const MUSIC_PLAYLIST: [&str; 10] = [
    "MAIN_MSC", "BKG1_MSC", "BKG2_MSC", "BKG3_MSC", "BKG4_MSC", "BKG5_MSC", "BKG6_MSC", "BKG7_MSC",
    "BKG8_MSC", "BKG9_MSC",
];
/// Planet base tracks — copy of `max_app::music::PLANET_TRACKS`.
pub const PLANET_TRACKS: [&str; 4] = ["SNOW_MSC", "CRTR_MSC", "GREN_MSC", "DSRT_MSC"];
/// Victory/defeat tracks — copy of `max_app::music::{TRACK_WIN, TRACK_LOSE}`.
pub const OUTCOME_TRACKS: [&str; 2] = ["WINR_MSC", "LOSE_MSC"];

/// Every music resource tag the game can request.
pub const MUSIC_TRACKS: [&str; 16] = [
    "MAIN_MSC", "BKG1_MSC", "BKG2_MSC", "BKG3_MSC", "BKG4_MSC", "BKG5_MSC", "BKG6_MSC", "BKG7_MSC",
    "BKG8_MSC", "BKG9_MSC", "SNOW_MSC", "CRTR_MSC", "GREN_MSC", "DSRT_MSC", "WINR_MSC", "LOSE_MSC",
];

/// Music tags whose files the retail data set does not ship (plan §A).
///
/// `BKG8_MSC` and `BKG9_MSC` both reference `bkg9_msc.msc`, so the two tracks
/// share one absent file.
pub const ALLOWLISTED_MISSING_MUSIC: [&str; 3] = ["BKG7_MSC", "BKG8_MSC", "BKG9_MSC"];

/// Demo mission files referenced by `SC_D0002`/`SC_D0003` but not shipped by
/// the retail data set (only `SAVE1.DMO` exists; see
/// `max-brh-docs/development/campaign-missions.md`).
pub const ALLOWLISTED_MISSING_MISSIONS: [&str; 2] = ["SAVE2.DMO", "SAVE3.DMO"];

/// Categories whose briefings are stored as `DESCR<slot>.<extension>` text
/// files next to the mission saves.
const BRIEFING_EXTENSIONS: [(MissionCategory, &str); 4] = [
    (MissionCategory::Training, "TRA"),
    (MissionCategory::Campaign, "CAM"),
    (MissionCategory::Scenario, "SCE"),
    (MissionCategory::MultiPlayerScenario, "MPS"),
];

const MUSIC_ALLOWLIST_REASON: &str =
    "music track allowlisted: the retail data set does not ship BKG7_MSC/BKG8_MSC/BKG9_MSC";
const MISSION_ALLOWLIST_REASON: &str =
    "demo mission file allowlisted: the retail data set ships only SAVE1.DMO";

/// Distribution class of a shipped file (plan §A: data/mission/sound/flc/video/
/// music/subtitles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetClass {
    /// Core archives, palette and `.WRL` maps.
    Data,
    /// Mission saves, mission briefings and the campaign epilogue.
    Mission,
    /// `*.SPW` RIFF/WAVE sound clips.
    Sound,
    /// `*.FLC` Autodesk FLIC portraits/animations.
    Flc,
    /// `*.MVE` Interplay videos.
    Video,
    /// `*.MSC` music tracks.
    Music,
    /// `maxint.json` subtitles.
    Subtitles,
}

impl AssetClass {
    /// Every class in manifest order (the order of plan §A).
    pub const ALL: [Self; 7] = [
        Self::Data,
        Self::Mission,
        Self::Sound,
        Self::Flc,
        Self::Video,
        Self::Music,
        Self::Subtitles,
    ];

    /// Lower-case class name as it appears in the manifest.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Mission => "mission",
            Self::Sound => "sound",
            Self::Flc => "flc",
            Self::Video => "video",
            Self::Music => "music",
            Self::Subtitles => "subtitles",
        }
    }

    /// Class of a file with `extension` (case-insensitive), `None` for files
    /// the distribution never ships by reference.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "spw" => Some(Self::Sound),
            "flc" => Some(Self::Flc),
            "wrl" => Some(Self::Data),
            "msc" => Some(Self::Music),
            "mve" => Some(Self::Video),
            "json" => Some(Self::Subtitles),
            "cam" | "mps" | "sce" | "tra" | "dmo" => Some(Self::Mission),
            _ => None,
        }
    }

    /// Class of a bare file name (the extension after the last dot).
    fn from_file_name(name: &str) -> Option<Self> {
        let (_, extension) = name.rsplit_once('.')?;

        Self::from_extension(extension)
    }
}

/// One shipped file with its digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InventoryFile {
    /// File name relative to the data directory (case as on disk).
    pub path: String,
    /// Distribution class.
    pub class: AssetClass,
    /// File length in bytes.
    pub size: u64,
    /// Lower-case hex SHA-256 of the file contents.
    pub sha256: String,
}

/// An expected file that the data directory does not contain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingFile {
    /// Referenced/expected name relative to the data directory.
    pub path: String,
    /// Distribution class.
    pub class: AssetClass,
    /// `MAX.RES`/`PATCHES.RES` tags or registry tags that ask for the file.
    pub referenced_by: Vec<String>,
    /// Why the file is expected and what its absence means.
    pub reason: String,
}

/// Per-class file count and byte total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ClassCount {
    /// Distribution class.
    pub class: AssetClass,
    /// Number of manifest entries in the class.
    pub files: usize,
    /// Sum of `size` over the class.
    pub bytes: u64,
}

/// Manifest totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct InventoryTotals {
    /// Number of manifest entries.
    pub files: usize,
    /// Sum of all file sizes.
    pub bytes: u64,
}

/// What the scan read from the archives, the mission registry and the music
/// playlists. Stable counters for gate reports and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct InventorySources {
    /// Records in `MAX.RES` (1828 in the retail data).
    pub max_res_entries: usize,
    /// File-reference records in `MAX.RES` (721).
    pub max_res_file_references: usize,
    /// Distinct referenced files (567).
    pub max_res_unique_references: usize,
    /// Distinct referenced files absent from the data set (184: 182 sounds
    /// and the two allowlisted music files).
    pub max_res_unresolved_references: usize,
    /// Records in `PATCHES.RES` (84).
    pub patches_res_entries: usize,
    /// File-reference records in `PATCHES.RES` (1: `INTROSUB`).
    pub patches_res_file_references: usize,
    /// Distinct referenced files in `PATCHES.RES` (1).
    pub patches_res_unique_references: usize,
    /// Distinct `PATCHES.RES` references absent from the data set (0).
    pub patches_res_unresolved_references: usize,
    /// Decoded `SC_*` mission descriptors (66).
    pub mission_descriptors: usize,
    /// Mission files the registry references (63: 61 present + 2 allowlisted).
    pub mission_files: usize,
    /// Referenced mission files that exist (61).
    pub mission_files_present: usize,
    /// Mission files whose SHA-256 matches the descriptor hash (61).
    pub mission_hashes_verified: usize,
    /// Mission files whose hash does not match (0 in the retail data).
    pub mission_hashes_mismatched: usize,
    /// `DESCR*` briefings plus the campaign epilogue present (61).
    pub mission_briefing_files: usize,
    /// Music tags the game can request (16).
    pub music_tracks: usize,
    /// Music files in the manifest (13).
    pub music_files: usize,
    /// Music tags allowlisted as absent (3: `BKG7/8/9_MSC`).
    pub music_allowlisted_missing: usize,
    /// Music tags absent without an allowlist entry (0).
    pub music_missing: usize,
}

/// The complete distribution manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DataInventory {
    schema: u32,
    sources: InventorySources,
    totals: InventoryTotals,
    classes: Vec<ClassCount>,
    files: Vec<InventoryFile>,
    missing: Vec<MissingFile>,
    allowlisted_missing: Vec<MissingFile>,
}

impl DataInventory {
    /// Manifest schema version.
    pub fn schema(&self) -> u32 {
        self.schema
    }

    /// Scan counters.
    pub fn sources(&self) -> &InventorySources {
        &self.sources
    }

    /// Manifest totals.
    pub fn totals(&self) -> &InventoryTotals {
        &self.totals
    }

    /// Per-class counts in [`AssetClass::ALL`] order.
    pub fn classes(&self) -> &[ClassCount] {
        &self.classes
    }

    /// Shipped files sorted by path.
    pub fn files(&self) -> &[InventoryFile] {
        &self.files
    }

    /// Required files absent from the data directory (empty for a complete
    /// distribution; makes the manifest incomplete).
    pub fn missing(&self) -> &[MissingFile] {
        &self.missing
    }

    /// Known retail gaps (allowlisted, not failures).
    pub fn allowlisted_missing(&self) -> &[MissingFile] {
        &self.allowlisted_missing
    }

    /// Number of manifest entries of `class`.
    pub fn class_count(&self, class: AssetClass) -> usize {
        self.classes
            .iter()
            .find(|count| count.class == class)
            .map_or(0, |count| count.files)
    }

    /// Number of required files of `class` that are absent.
    pub fn missing_count(&self, class: AssetClass) -> usize {
        self.missing
            .iter()
            .filter(|file| file.class == class)
            .count()
    }

    /// No required file is missing (allowlisted gaps do not count).
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }

    /// Deterministic JSON text with a trailing newline.
    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self)
            .expect("DataInventory is plain data and always serializes");

        json.push('\n');

        json
    }

    /// Writes [`DataInventory::to_json`] to `path`.
    pub fn write(&self, path: impl AsRef<Path>) -> io::Result<()> {
        fs::write(path, self.to_json())
    }
}

/// Inventory build error (returned as a value, never a panic).
#[derive(Debug)]
pub enum InventoryError {
    /// The data directory or one of its files cannot be read.
    Io {
        /// Path being read.
        path: PathBuf,
        /// OS reason.
        source: io::Error,
    },
    /// A `.RES` archive is malformed.
    Archive {
        /// Path of the archive.
        path: PathBuf,
        /// Parser reason.
        source: ResError,
    },
}

impl std::fmt::Display for InventoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::Archive { path, source } => {
                write!(f, "cannot parse {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for InventoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Archive { source, .. } => Some(source),
        }
    }
}

/// Builds the distribution manifest for `data_dir`.
///
/// `MAX.RES` and `PATCHES.RES` must parse (a hard error otherwise); every other
/// problem (a missing `COLOR.PAL`, a missing mission file, ...) is recorded in
/// the manifest instead of failing the build.
pub fn build(data_dir: impl AsRef<Path>) -> Result<DataInventory, InventoryError> {
    let dir = data_dir.as_ref();
    let disk = DiskFiles::scan(dir)?;

    let max_res_path = dir.join(MAX_RES_FILE);
    let patches_path = dir.join(PATCHES_RES_FILE);
    let max_res = open_archive(&max_res_path)?;
    let patches_res = open_archive(&patches_path)?;

    let mut builder = Builder::new(dir, disk);

    // Core files: always shipped, even though the archives carry the payloads
    // themselves (the files are the network `data_hash` input too).
    builder.expect_file(
        MAX_RES_FILE,
        AssetClass::Data,
        &[MAX_RES_FILE],
        "required core data file is missing",
    )?;
    builder.expect_file(
        PATCHES_RES_FILE,
        AssetClass::Data,
        &[PATCHES_RES_FILE],
        "required mission/unit archive is missing",
    )?;
    builder.expect_file(
        COLOR_PAL_FILE,
        AssetClass::Data,
        &[COLOR_PAL_FILE],
        "required palette file is missing",
    )?;
    builder.expect_file(
        SUBTITLES_FILE,
        AssetClass::Subtitles,
        &["PATCHES.RES:INTROSUB"],
        "subtitles file referenced by PATCHES.RES:INTROSUB is missing",
    )?;

    // External files: every ASCIIZ file-name record of both archives.
    let max_references = ArchiveReferences::collect(&max_res);
    let patches_references = ArchiveReferences::collect(&patches_res);
    let max_unresolved = builder.ingest_references(&max_references, MAX_RES_FILE)?;
    let patches_unresolved = builder.ingest_references(&patches_references, PATCHES_RES_FILE)?;

    // Mission registry and the prose briefings next to the saves.
    let registry = MissionRegistry::from_archive(&patches_res, dir);
    let mission = builder.ingest_missions(&registry)?;

    // Playlist cross-check: every tag the director can request must have a
    // file, unless the tag is on the retail allowlist.
    let music = builder.check_music(&max_res);

    let mut files: Vec<InventoryFile> = builder.files.into_values().collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let mut missing: Vec<MissingFile> = builder.missing.into_values().collect();
    missing.sort_by(|left, right| left.path.cmp(&right.path));

    let mut allowlisted_missing: Vec<MissingFile> = builder.allowlisted.into_values().collect();
    allowlisted_missing.sort_by(|left, right| left.path.cmp(&right.path));

    let totals = InventoryTotals {
        files: files.len(),
        bytes: files.iter().map(|file| file.size).sum(),
    };
    let classes = AssetClass::ALL
        .iter()
        .map(|&class| {
            let selected: Vec<&InventoryFile> =
                files.iter().filter(|file| file.class == class).collect();

            ClassCount {
                class,
                files: selected.len(),
                bytes: selected.iter().map(|file| file.size).sum(),
            }
        })
        .collect();

    let sources = InventorySources {
        max_res_entries: max_res.len(),
        max_res_file_references: max_references.total,
        max_res_unique_references: max_references.unique.len(),
        max_res_unresolved_references: max_unresolved,
        patches_res_entries: patches_res.len(),
        patches_res_file_references: patches_references.total,
        patches_res_unique_references: patches_references.unique.len(),
        patches_res_unresolved_references: patches_unresolved,
        mission_descriptors: registry.len(),
        mission_files: mission.referenced,
        mission_files_present: mission.present,
        mission_hashes_verified: mission.verified,
        mission_hashes_mismatched: mission.mismatched,
        mission_briefing_files: mission.briefing,
        music_tracks: music.tracks,
        music_files: files
            .iter()
            .filter(|file| file.class == AssetClass::Music)
            .count(),
        music_allowlisted_missing: music.allowlisted,
        music_missing: music.missing,
    };

    Ok(DataInventory {
        schema: INVENTORY_SCHEMA,
        sources,
        totals,
        classes,
        files,
        missing,
        allowlisted_missing,
    })
}

fn open_archive(path: &Path) -> Result<ResArchive, InventoryError> {
    ResArchive::open(path).map_err(|source| InventoryError::Archive {
        path: path.to_path_buf(),
        source,
    })
}

/// File names of one directory, keyed case-insensitively for resolution.
struct DiskFiles {
    /// Lower-case name → on-disk name.
    by_lower: BTreeMap<String, String>,
}

impl DiskFiles {
    /// Reads the regular file names of `dir`.
    ///
    /// Names are sorted before the map is built, so a case-insensitive
    /// collision resolves the same way on every run and every filesystem.
    fn scan(dir: &Path) -> Result<Self, InventoryError> {
        let entries = fs::read_dir(dir).map_err(|source| InventoryError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let mut names = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|source| InventoryError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
            let is_file = entry
                .file_type()
                .map_err(|source| InventoryError::Io {
                    path: entry.path(),
                    source,
                })?
                .is_file();

            if !is_file {
                continue;
            }

            // Game data names are ASCII; skip anything that is not UTF-8
            // instead of failing the whole inventory on an unrelated file.
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_owned());
            }
        }

        names.sort();

        let mut by_lower = BTreeMap::new();

        for name in names {
            by_lower
                .entry(name.to_ascii_lowercase())
                .or_insert_with(|| name);
        }

        Ok(Self { by_lower })
    }

    /// On-disk name of `name` when the data directory contains it, ignoring
    /// ASCII case (the references are lower-case, the files are upper-case).
    fn resolve(&self, name: &str) -> Option<&str> {
        self.by_lower
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// Distinct file references of one archive.
#[derive(Debug, Default)]
struct ArchiveReferences {
    /// Number of reference records (a file may be referenced by several tags).
    total: usize,
    /// Referenced name → tags asking for it.
    unique: BTreeMap<String, BTreeSet<String>>,
}

impl ArchiveReferences {
    /// Collects the ASCIIZ file-name records of `archive`.
    fn collect(archive: &ResArchive) -> Self {
        let mut references = Self::default();

        for entry in archive.list() {
            if entry.size == 0 || entry.size > MAX_REFERENCE_BYTES {
                continue;
            }

            // Read this record's own body (not the first record of the tag):
            // a duplicate tag must not hide a reference.
            let start = entry.offset as usize;
            let Some(end) = start
                .checked_add(entry.size as usize)
                .filter(|&end| end <= archive.data().len())
            else {
                continue;
            };
            let Some(name) = parse_file_ref(&archive.data()[start..end]) else {
                continue;
            };

            // Only files of a known distribution class are references; this
            // keeps short text payloads from turning into file names.
            if AssetClass::from_file_name(name).is_none() {
                continue;
            }

            references.total += 1;
            references
                .unique
                .entry(name.to_owned())
                .or_default()
                .insert(entry.name.clone());
        }

        references
    }
}

/// Mission counters gathered while the files are added.
struct MissionStats {
    referenced: usize,
    present: usize,
    verified: usize,
    mismatched: usize,
    briefing: usize,
}

/// Music counters gathered while the playlist is checked.
struct MusicStats {
    tracks: usize,
    allowlisted: usize,
    missing: usize,
}

/// Accumulates unique files and missing entries for [`build`].
struct Builder<'a> {
    dir: &'a Path,
    disk: DiskFiles,
    files: BTreeMap<String, InventoryFile>,
    missing: BTreeMap<String, MissingFile>,
    allowlisted: BTreeMap<String, MissingFile>,
}

impl<'a> Builder<'a> {
    fn new(dir: &'a Path, disk: DiskFiles) -> Self {
        Self {
            dir,
            disk,
            files: BTreeMap::new(),
            missing: BTreeMap::new(),
            allowlisted: BTreeMap::new(),
        }
    }

    /// Adds `name` as a required file or records it as missing.
    fn expect_file(
        &mut self,
        name: &str,
        class: AssetClass,
        referenced_by: &[&str],
        reason: &str,
    ) -> Result<(), InventoryError> {
        match self.disk.resolve(name) {
            Some(actual) => {
                let actual = actual.to_owned();

                self.add_file(&actual, class)
            }
            None => {
                self.record_missing(name, class, referenced_by, reason);

                Ok(())
            }
        }
    }

    /// Adds a file (deduplicated by case-insensitive name) and hashes it.
    fn add_file(&mut self, name: &str, class: AssetClass) -> Result<(), InventoryError> {
        let key = name.to_ascii_lowercase();

        if let Some(existing) = self.files.get(&key) {
            debug_assert_eq!(
                existing.class, class,
                "a shipped file must have exactly one class"
            );

            return Ok(());
        }

        let path = self.dir.join(name);
        let (size, sha256) = hash_file(&path)?;

        self.files.insert(
            key,
            InventoryFile {
                path: name.to_owned(),
                class,
                size,
                sha256,
            },
        );

        Ok(())
    }

    /// Records a required file that the data directory does not contain.
    fn record_missing(
        &mut self,
        path: &str,
        class: AssetClass,
        referenced_by: &[&str],
        reason: &str,
    ) {
        let key = path.to_ascii_lowercase();

        if self.files.contains_key(&key) {
            return;
        }

        if let Some(mut entry) = self.allowlisted.remove(&key) {
            entry.reason = reason.to_owned();
            push_tags(&mut entry.referenced_by, referenced_by);
            self.missing.insert(key, entry);

            return;
        }

        let entry = self.missing.entry(key).or_insert_with(|| MissingFile {
            path: path.to_owned(),
            class,
            referenced_by: Vec::new(),
            reason: reason.to_owned(),
        });

        push_tags(&mut entry.referenced_by, referenced_by);
    }

    /// Records a known retail gap; it does not make the manifest incomplete.
    fn record_allowlisted(
        &mut self,
        path: &str,
        class: AssetClass,
        referenced_by: &[&str],
        reason: &str,
    ) {
        let key = path.to_ascii_lowercase();

        if self.files.contains_key(&key) {
            return;
        }

        if let Some(entry) = self.missing.get_mut(&key) {
            push_tags(&mut entry.referenced_by, referenced_by);

            return;
        }

        let entry = self.allowlisted.entry(key).or_insert_with(|| MissingFile {
            path: path.to_owned(),
            class,
            referenced_by: Vec::new(),
            reason: reason.to_owned(),
        });

        push_tags(&mut entry.referenced_by, referenced_by);
    }

    /// Adds every resolvable reference and returns how many distinct names were
    /// absent from the data directory.
    fn ingest_references(
        &mut self,
        references: &ArchiveReferences,
        source: &str,
    ) -> Result<usize, InventoryError> {
        let mut unresolved = 0;

        for (name, tags) in &references.unique {
            let class = AssetClass::from_file_name(name)
                .expect("ArchiveReferences stores classified names only");

            if let Some(actual) = self.disk.resolve(name).map(str::to_owned) {
                self.add_file(&actual, class)?;

                continue;
            }

            unresolved += 1;

            let tags: Vec<&str> = tags.iter().map(String::as_str).collect();

            match class {
                // Legacy sound tags whose files the retail data set does not
                // ship: not part of the distribution (see the module docs).
                AssetClass::Sound => {}
                AssetClass::Music if music_is_allowlisted(&tags) => {
                    self.record_allowlisted(name, class, &tags, MUSIC_ALLOWLIST_REASON);
                }
                _ => {
                    let reason = format!("file referenced by {source} is missing");

                    self.record_missing(name, class, &tags, &reason);
                }
            }
        }

        Ok(unresolved)
    }

    /// Adds the registry mission files plus the `DESCR*` briefings and the
    /// campaign epilogue.
    fn ingest_missions(
        &mut self,
        registry: &MissionRegistry,
    ) -> Result<MissionStats, InventoryError> {
        let mut stats = MissionStats {
            referenced: 0,
            present: 0,
            verified: 0,
            mismatched: 0,
            briefing: 0,
        };

        for &category in &MISSION_CATEGORIES {
            for descriptor in registry.missions(category) {
                let Some(name) = descriptor.mission_name.as_deref() else {
                    continue;
                };

                stats.referenced += 1;

                if descriptor.mission_found {
                    let resolved = descriptor
                        .mission_path()
                        .and_then(Path::file_name)
                        .and_then(|name| name.to_str())
                        .unwrap_or(name);
                    // The registry may resolve a lower-case candidate
                    // (`save1.cam`) on a case-insensitive filesystem; the
                    // manifest always names the file as it is on disk.
                    let actual = self.disk.resolve(resolved).unwrap_or(resolved).to_owned();

                    self.add_file(&actual, AssetClass::Mission)?;
                    stats.present += 1;
                } else if mission_is_allowlisted(name) {
                    self.record_allowlisted(
                        name,
                        AssetClass::Mission,
                        &[descriptor.tag.as_str()],
                        MISSION_ALLOWLIST_REASON,
                    );
                } else {
                    self.record_missing(
                        name,
                        AssetClass::Mission,
                        &[descriptor.tag.as_str()],
                        "mission file referenced by the registry is missing",
                    );
                }

                match descriptor.hash_verified {
                    Some(true) => stats.verified += 1,
                    Some(false) => stats.mismatched += 1,
                    None => {}
                }
            }
        }

        // Briefings mirror the mission slots of the categories that have them.
        for (category, extension) in BRIEFING_EXTENSIONS {
            let slots = registry.missions(category).len();

            for slot in 1..=slots {
                let name = format!("DESCR{slot}.{extension}");

                match self.disk.resolve(&name).map(str::to_owned) {
                    Some(actual) => {
                        self.add_file(&actual, AssetClass::Mission)?;
                        stats.briefing += 1;
                    }
                    None => self.record_missing(
                        &name,
                        AssetClass::Mission,
                        &[category_tag(category)],
                        "mission briefing file is missing",
                    ),
                }
            }
        }

        match self.disk.resolve(CAMPAIGN_EPILOGUE_FILE).map(str::to_owned) {
            Some(actual) => {
                self.add_file(&actual, AssetClass::Mission)?;
                stats.briefing += 1;
            }
            None => self.record_missing(
                CAMPAIGN_EPILOGUE_FILE,
                AssetClass::Mission,
                &["campaign epilogue"],
                "campaign epilogue file is missing",
            ),
        }

        Ok(stats)
    }

    /// Counts the music tags that resolve and records the ones that do not.
    fn check_music(&mut self, max_res: &ResArchive) -> MusicStats {
        let mut stats = MusicStats {
            tracks: MUSIC_TRACKS.len(),
            allowlisted: 0,
            missing: 0,
        };

        for tag in MUSIC_TRACKS {
            let reference = max_res.file_reference(tag);

            if reference.and_then(|name| self.disk.resolve(name)).is_some() {
                continue;
            }

            if ALLOWLISTED_MISSING_MUSIC.contains(&tag) {
                stats.allowlisted += 1;

                continue;
            }

            stats.missing += 1;

            let path = reference.unwrap_or(tag);

            self.record_missing(
                path,
                AssetClass::Music,
                &[tag],
                "music track is missing from the data set",
            );
        }

        stats
    }
}

/// All mission categories (`MISSION_CATEGORY_*`), in the C++ enum order.
const MISSION_CATEGORIES: [MissionCategory; 8] = [
    MissionCategory::Custom,
    MissionCategory::Training,
    MissionCategory::Campaign,
    MissionCategory::HotSeat,
    MissionCategory::Multiplayer,
    MissionCategory::Demo,
    MissionCategory::Scenario,
    MissionCategory::MultiPlayerScenario,
];

fn category_tag(category: MissionCategory) -> &'static str {
    match category {
        MissionCategory::Custom => "Custom",
        MissionCategory::Training => "Training",
        MissionCategory::Campaign => "Campaign",
        MissionCategory::HotSeat => "Hot-seat",
        MissionCategory::Multiplayer => "Multiplayer",
        MissionCategory::Demo => "Demo",
        MissionCategory::Scenario => "Scenario",
        MissionCategory::MultiPlayerScenario => "Multiplayer Scenario",
    }
}

fn push_tags(entry: &mut Vec<String>, tags: &[&str]) {
    for tag in tags {
        if !entry.iter().any(|existing| existing == tag) {
            entry.push((*tag).to_owned());
        }
    }

    entry.sort();
}

fn music_is_allowlisted(tags: &[&str]) -> bool {
    !tags.is_empty()
        && tags
            .iter()
            .all(|tag| ALLOWLISTED_MISSING_MUSIC.contains(tag))
}

fn mission_is_allowlisted(name: &str) -> bool {
    ALLOWLISTED_MISSING_MISSIONS
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(name))
}

/// Length and lower-case hex SHA-256 of a file.
fn hash_file(path: &Path) -> Result<(u64, String), InventoryError> {
    let mut file = fs::File::open(path).map_err(|source| InventoryError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| InventoryError::Io {
                path: path.to_path_buf(),
                source,
            })?;

        if read == 0 {
            break;
        }

        hasher.update(&buffer[..read]);
        size += read as u64;
    }

    Ok((size, hex_digest(&hasher.finalize())))
}

fn hex_digest(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut hex = String::with_capacity(digest.len() * 2);

    for &byte in digest {
        hex.push(HEX[usize::from(byte >> 4)] as char);
        hex.push(HEX[usize::from(byte & 0x0F)] as char);
    }

    hex
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
