//! Mission descriptors and the mission registry.
//!
//! Oracle: `missionregistry.cpp:28-82` (scan `SC_MIS_S..SC_MIS_E`, XOR decode,
//! category buckets, mission index order), `mission.cpp:425-455`
//! (`MissionObject` JSON fields), `resourcetable.hpp:1974-2041` (`SC_*` tags).
//!
//! A mission descriptor is a JSON document XOR-obfuscated with
//! [`GENERIC_TABLE`] and stored in `PATCHES.RES` under one of
//! [`MISSION_RESOURCE_TAGS`]. [`MissionRegistry`] decodes every descriptor,
//! resolves the referenced mission file (a V70 save `SAVE*.CAM`/`.SCE`/`.TRA`)
//! in the data directory and groups the descriptors by [`MissionCategory`].
//!
//! Ordering is 1:1 with the C++ port: descriptors are pushed in resource-id
//! order (`SC_C0001`, `SC_C0002`, …), and a position in
//! [`MissionRegistry::missions`] is the mission index used by the setup menu,
//! `mission_index` and the win/loss rules. The C++ filesystem scan of
//! `*.mission.json` (a port-only extension, `missionregistry.cpp:56-71`) is not
//! part of this stage.
//!
//! Deliberate deviations, both non-blocking:
//! * a missing or mismatched mission file is a warning, not a reason to drop the
//!   descriptor (C++ throws and skips the mission);
//! * the JSON schema (`SC_SCHEM`) is not enforced; unknown fields are ignored,
//!   and a resource whose JSON has an unexpected shape is skipped with a warning.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub use max_types::victory::MissionCategory;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::res::{ResArchive, ResError};

/// XOR table of the compressed JSON resources
/// (`ResourceManager_GenericTable`, `resource_manager.cpp:156`).
pub const GENERIC_TABLE: [u8; 32] = [
    0x6c, 0x57, 0x36, 0xe6, 0x81, 0xe0, 0x72, 0x8a, 0xd3, 0xd9, 0xff, 0x54, 0x48, 0x3a, 0xcd, 0x75,
    0xd5, 0x0d, 0xe9, 0xe7, 0x7a, 0x57, 0xae, 0xeb, 0x61, 0x16, 0xb0, 0x35, 0x55, 0x62, 0xed, 0xd1,
];

/// Default locale of the text blocks (`Mission_GetText`, `mission.cpp:113-116`).
pub const DEFAULT_LANGUAGE: &str = "en-US";

/// Number of categories (`MISSION_CATEGORY_COUNT`, `mission.hpp:40`).
pub const MISSION_CATEGORY_COUNT: usize = 8;

/// Mission descriptor tags in resource-id order (`SC_MIS_S..SC_MIS_E`,
/// `resourcetable.hpp:1974-2041`).
///
/// The order is the C++ `for (id = SC_MIS_S; id < SC_MIS_E; ++id)` order and is
/// what makes a position inside a category the mission index.
pub const MISSION_RESOURCE_TAGS: [&str; 66] = [
    "SC_CUST1", "SC_HOTS1", "SC_MULT1", "SC_C0001", "SC_C0002", "SC_C0003", "SC_C0004", "SC_C0005",
    "SC_C0006", "SC_C0007", "SC_C0008", "SC_C0009", "SC_M0001", "SC_M0002", "SC_M0003", "SC_M0004",
    "SC_M0005", "SC_M0006", "SC_M0007", "SC_M0008", "SC_M0009", "SC_M0010", "SC_M0011", "SC_M0012",
    "SC_S0001", "SC_S0002", "SC_S0003", "SC_S0004", "SC_S0005", "SC_S0006", "SC_S0007", "SC_S0008",
    "SC_S0009", "SC_S0010", "SC_S0011", "SC_S0012", "SC_S0013", "SC_S0014", "SC_S0015", "SC_S0016",
    "SC_S0017", "SC_S0018", "SC_S0019", "SC_S0020", "SC_S0021", "SC_S0022", "SC_S0023", "SC_S0024",
    "SC_T0001", "SC_T0002", "SC_T0003", "SC_T0004", "SC_T0005", "SC_T0006", "SC_T0007", "SC_T0008",
    "SC_T0009", "SC_T0010", "SC_T0011", "SC_T0012", "SC_T0013", "SC_T0014", "SC_T0015", "SC_D0001",
    "SC_D0002", "SC_D0003",
];

/// XOR-decodes `data` with [`GENERIC_TABLE`] (period 32), like the C++ port
/// (`file_base[i] ^ ResourceManager_GenericTable[i % 32]`).
pub fn xor_decode(data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(index, byte)| byte ^ GENERIC_TABLE[index % GENERIC_TABLE.len()])
        .collect()
}

/// A localized text block (`TextBlock`, `mission.cpp:54-56`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBlock {
    /// Texts keyed by language tag (`en-US`, `ru-RU`, …).
    pub text: BTreeMap<String, String>,
}

impl TextBlock {
    /// Text for `language`, with the C++ fallback to [`DEFAULT_LANGUAGE`]; `""`
    /// when neither is present.
    pub fn get(&self, language: &str) -> &str {
        self.text
            .get(language)
            .or_else(|| self.text.get(DEFAULT_LANGUAGE))
            .map_or("", String::as_str)
    }

    /// Default (`en-US`) text.
    pub fn default_text(&self) -> &str {
        self.get(DEFAULT_LANGUAGE)
    }

    /// Whether the block has no texts at all.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// A media reference (`Media`, `mission.cpp:44-48`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// Resource reference; `None` when the descriptor has neither `id` nor `file`.
    pub resource: Option<MediaResource>,
    /// Copyright notice (`Media::copyright`).
    pub copyright: String,
    /// License notice (`Media::license`).
    pub license: String,
}

/// The `resource` of a [`Media`] block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaResource {
    /// `resource.id` — resource tags such as `ENDGAME1` or `DEMO2FLC`.
    ///
    /// The original JSON schema declares an array, but live descriptors also use
    /// a single string (the video of `SC_C0001`), so both forms are accepted.
    Ids(Vec<String>),
    /// `resource.file` — paths relative to the game data directory.
    Files(Vec<String>),
}

/// A story block (`StoryBlock`, `mission.cpp:62-67`) of `intro`/`victory`/`defeat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryBlock {
    /// Localized story text.
    pub text: TextBlock,
    /// Background images (`StoryBlock::background`).
    pub background: Option<Media>,
    /// Intro videos keyed by language (`StoryBlock::video`).
    pub video: BTreeMap<String, Media>,
    /// Background music (`StoryBlock::music`).
    pub music: Option<Media>,
}

impl StoryBlock {
    /// Default (`en-US`) story text.
    pub fn default_text(&self) -> &str {
        self.text.default_text()
    }
}

/// One decoded mission descriptor (`MissionObject`, `mission.cpp:94-111`).
///
/// The position inside [`MissionRegistry::missions`] is the C++ mission index
/// (`MissionManager::GetMissionIndex`), used by `mission_index` and win/loss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionDescriptor {
    /// Resource tag (`SC_C0001`) the descriptor was decoded from.
    pub tag: String,
    /// `author`.
    pub author: String,
    /// `copyright`.
    pub copyright: String,
    /// `license`.
    pub license: String,
    /// `category` — the registry bucket.
    pub category: MissionCategory,
    /// Declared mission file name (`SAVE1.CAM`); `None` for
    /// `Custom`/`Hot-seat`/`Multiplayer` (`Mission_ProcessMission`,
    /// `mission.cpp:272-291`).
    pub mission_name: Option<String>,
    /// Mission file path resolved in the data directory. When the file is absent
    /// this is `data_dir/mission_name`, so the descriptor stays usable for a
    /// later retry (C++ would have dropped it).
    pub mission: Option<PathBuf>,
    /// Whether [`MissionDescriptor::mission`] exists as a regular file.
    pub mission_found: bool,
    /// Declared SHA-256 hashes (`hash`) of the mission file.
    pub hash: Vec<String>,
    /// `Some(true)` — the file hash matches one of [`MissionDescriptor::hash`];
    /// `Some(false)` — mismatch; `None` — not checked (no file or no hashes).
    pub hash_verified: Option<bool>,
    /// `title`.
    pub title: TextBlock,
    /// `description`.
    pub description: TextBlock,
    /// `intro` story block.
    pub intro: Option<StoryBlock>,
    /// `victory` story block.
    pub victory: Option<StoryBlock>,
    /// `defeat` story block.
    pub defeat: Option<StoryBlock>,
    /// `winloss_conditions` Lua script; hardcoded in `max_core::victory` for now.
    pub winloss_conditions: Option<String>,
    /// `game_rules` Lua script (stage 4 of the campaign plan).
    pub game_rules: Option<String>,
}

impl MissionDescriptor {
    /// Localized title (`Mission::GetTitle`), `en-US` by default.
    pub fn title_text(&self) -> &str {
        self.title.default_text()
    }

    /// Localized description (`Mission::GetDescription`), `en-US` by default.
    pub fn description_text(&self) -> &str {
        self.description.default_text()
    }

    /// Resolved mission file (`Mission::GetMission`).
    pub fn mission_path(&self) -> Option<&Path> {
        self.mission.as_deref()
    }

    /// `Mission::HasIntroInfo`.
    pub fn has_intro(&self) -> bool {
        self.intro.is_some()
    }

    /// `Mission::HasVictoryInfo`.
    pub fn has_victory(&self) -> bool {
        self.victory.is_some()
    }

    /// `Mission::HasDefeatInfo`.
    pub fn has_defeat(&self) -> bool {
        self.defeat.is_some()
    }

    /// `Mission::HasWinLossConditions`.
    pub fn has_winloss_conditions(&self) -> bool {
        self.winloss_conditions.is_some()
    }

    /// `Mission::HasGameRules`.
    pub fn has_game_rules(&self) -> bool {
        self.game_rules.is_some()
    }
}

/// Mission descriptors grouped by category (`MissionRegistry`,
/// `missionregistry.cpp:28-82`).
///
/// [`MissionRegistry::missions`] returns the C++ `m_categories[category]`
/// vector; the index inside it is the mission index (0-based,
/// `game_file_number - 1`).
#[derive(Debug, Clone)]
pub struct MissionRegistry {
    categories: [Vec<MissionDescriptor>; MISSION_CATEGORY_COUNT],
    warnings: Vec<String>,
}

impl MissionRegistry {
    /// Reads `PATCHES.RES` and builds the registry.
    ///
    /// `data_dir` is the game data directory where mission files
    /// (`SAVE1.CAM`, …) are searched for.
    pub fn open(
        patches_res: impl AsRef<Path>,
        data_dir: impl AsRef<Path>,
    ) -> Result<Self, ResError> {
        let archive = ResArchive::open(patches_res)?;

        Ok(Self::from_archive(&archive, data_dir))
    }

    /// Builds the registry from an already parsed `PATCHES.RES`.
    pub fn from_archive(archive: &ResArchive, data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref();
        let mut categories: [Vec<MissionDescriptor>; MISSION_CATEGORY_COUNT] =
            std::array::from_fn(|_| Vec::new());
        let mut warnings = Vec::new();

        for tag in MISSION_RESOURCE_TAGS {
            let bytes = match archive.read(tag) {
                Ok(bytes) => bytes,
                Err(_) => {
                    warnings.push(format!("{tag}: resource not found in PATCHES.RES"));
                    continue;
                }
            };

            // `missionregistry.cpp:35` requires a non-empty resource.
            if bytes.is_empty() {
                continue;
            }

            let decoded = xor_decode(bytes);
            let raw: RawDescriptor = match serde_json::from_slice(&decoded) {
                Ok(raw) => raw,
                Err(error) => {
                    warnings.push(format!("{tag}: JSON parse error: {error}"));
                    continue;
                }
            };

            match descriptor_from_raw(tag, raw, data_dir, &mut warnings) {
                Ok(descriptor) => {
                    categories[descriptor.category as usize].push(descriptor);
                }
                Err(warning) => warnings.push(warning),
            }
        }

        Self {
            categories,
            warnings,
        }
    }

    /// Descriptors of `category` in C++ mission-index order
    /// (`MissionRegistry::GetMissions`).
    pub fn missions(&self, category: MissionCategory) -> &[MissionDescriptor] {
        &self.categories[category as usize]
    }

    /// First mission of `category` carrying `hash` (`MissionRegistry::GetMission`).
    pub fn get_mission(&self, category: MissionCategory, hash: &str) -> Option<&MissionDescriptor> {
        self.missions(category)
            .iter()
            .find(|mission| mission.hash.iter().any(|declared| declared == hash))
    }

    /// Total number of decoded descriptors.
    pub fn len(&self) -> usize {
        self.categories.iter().map(Vec::len).sum()
    }

    /// No descriptors were decoded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Non-fatal discrepancies collected while decoding (missing mission files,
    /// hash mismatches, malformed descriptors).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

// ---------------------------------------------------------------------------
// JSON representation (`mission.cpp:425-455`)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawDescriptor {
    author: String,
    copyright: String,
    license: String,
    category: String,
    #[serde(default)]
    mission: Option<String>,
    #[serde(default)]
    hash: Vec<String>,
    title: RawTextBlock,
    description: RawTextBlock,
    #[serde(default)]
    intro: Option<RawStoryBlock>,
    #[serde(default)]
    victory: Option<RawStoryBlock>,
    #[serde(default)]
    defeat: Option<RawStoryBlock>,
    #[serde(default)]
    winloss_conditions: Option<String>,
    #[serde(default)]
    game_rules: Option<String>,
}

#[derive(Deserialize)]
struct RawTextBlock {
    text: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RawStoryBlock {
    /// Localized story texts directly (`from_json(StoryBlock)` reads the `text`
    /// key of the story object, unlike `title`/`description` which are wrapped
    /// in `TextBlock`'s own `text` object).
    text: BTreeMap<String, String>,
    #[serde(default)]
    background: Option<RawMedia>,
    #[serde(default)]
    video: BTreeMap<String, RawMedia>,
    #[serde(default)]
    music: Option<RawMedia>,
}

#[derive(Deserialize)]
struct RawMedia {
    #[serde(default)]
    resource: Option<RawResource>,
    #[serde(default)]
    copyright: String,
    #[serde(default)]
    license: String,
}

#[derive(Deserialize)]
struct RawResource {
    #[serde(default)]
    id: Option<StringOrList>,
    #[serde(default)]
    file: Option<StringOrList>,
}

/// `resource.id`/`resource.file` as one string or an array of strings.
#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrList {
    One(String),
    Many(Vec<String>),
}

impl StringOrList {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

impl From<RawTextBlock> for TextBlock {
    fn from(raw: RawTextBlock) -> Self {
        Self { text: raw.text }
    }
}

fn convert_media(raw: RawMedia) -> Media {
    Media {
        resource: raw.resource.and_then(convert_resource),
        copyright: raw.copyright,
        license: raw.license,
    }
}

fn convert_resource(raw: RawResource) -> Option<MediaResource> {
    // `from_json(Resource)` prefers `file` when both keys are present.
    if let Some(files) = raw.file {
        Some(MediaResource::Files(files.into_vec()))
    } else {
        raw.id.map(|ids| MediaResource::Ids(ids.into_vec()))
    }
}

fn convert_story(raw: RawStoryBlock) -> StoryBlock {
    StoryBlock {
        text: TextBlock { text: raw.text },
        background: raw.background.map(convert_media),
        video: raw
            .video
            .into_iter()
            .map(|(language, media)| (language, convert_media(media)))
            .collect(),
        music: raw.music.map(convert_media),
    }
}

/// `Mission_ProcessCategory` (`mission.cpp:173-195`).
fn mission_category(name: &str) -> Option<MissionCategory> {
    Some(match name {
        "Custom" => MissionCategory::Custom,
        "Training" => MissionCategory::Training,
        "Campaign" => MissionCategory::Campaign,
        "Hot-seat" => MissionCategory::HotSeat,
        "Multiplayer" => MissionCategory::Multiplayer,
        "Demo" => MissionCategory::Demo,
        "Single Player Scenario" => MissionCategory::Scenario,
        "Multiplayer Scenario" => MissionCategory::MultiPlayerScenario,
        _ => return None,
    })
}

fn descriptor_from_raw(
    tag: &str,
    raw: RawDescriptor,
    data_dir: &Path,
    warnings: &mut Vec<String>,
) -> Result<MissionDescriptor, String> {
    let category = mission_category(&raw.category)
        .ok_or_else(|| format!("{tag}: unknown mission category {:?}", raw.category))?;

    let title = TextBlock::from(raw.title);
    let description = TextBlock::from(raw.description);

    // `from_json(TextBlock)` (`mission.cpp:327-333`) requires the default locale.
    if !title.text.contains_key(DEFAULT_LANGUAGE) {
        return Err(format!("{tag}: title has no {DEFAULT_LANGUAGE} text"));
    }

    if !description.text.contains_key(DEFAULT_LANGUAGE) {
        return Err(format!("{tag}: description has no {DEFAULT_LANGUAGE} text"));
    }

    let mut hash = raw.hash;
    let mut mission_name = None;
    let mut mission = None;
    let mut mission_found = false;
    let mut hash_verified = None;

    match category {
        MissionCategory::Custom | MissionCategory::HotSeat | MissionCategory::Multiplayer => {
            // `Mission_ProcessMission` (`mission.cpp:277-291`) does not read
            // `mission`/`hash` for these categories.
            hash.clear();
        }
        _ => {
            let name = raw
                .mission
                .ok_or_else(|| format!("{tag}: mission file name is missing"))?;
            let (path, found) = resolve_mission_file(data_dir, &name);

            if found {
                hash_verified = verify_mission_hash(&path, &hash, tag, warnings);
            } else {
                warnings.push(format!(
                    "{tag}: mission file {name:?} not found in {}",
                    data_dir.display()
                ));
            }

            if hash.is_empty() {
                warnings.push(format!("{tag}: no mission file hashes declared"));
            }

            mission_name = Some(name);
            mission = Some(path);
            mission_found = found;
        }
    }

    Ok(MissionDescriptor {
        tag: tag.to_owned(),
        author: raw.author,
        copyright: raw.copyright,
        license: raw.license,
        category,
        mission_name,
        mission,
        mission_found,
        hash,
        hash_verified,
        title,
        description,
        intro: raw.intro.map(convert_story),
        victory: raw.victory.map(convert_story),
        defeat: raw.defeat.map(convert_story),
        winloss_conditions: raw.winloss_conditions,
        game_rules: raw.game_rules,
    })
}

/// Resolves the declared mission file name in `data_dir`.
///
/// Tries the lowercase and the uppercase form first — the C++
/// `Mission_ProcessMission` checks `StringToLowerCase` before
/// `StringToUpperCase` — then the exact name and finally any file whose name
/// matches case-insensitively. Returns `data_dir/name` when nothing is found.
fn resolve_mission_file(data_dir: &Path, name: &str) -> (PathBuf, bool) {
    let lowered = data_dir.join(name.to_ascii_lowercase());

    if lowered.is_file() {
        return (lowered, true);
    }

    let uppered = data_dir.join(name.to_ascii_uppercase());

    if uppered.is_file() {
        return (uppered, true);
    }

    let exact = data_dir.join(name);

    if exact.is_file() {
        return (exact, true);
    }

    if let Some(found) = find_case_insensitive(data_dir, name) {
        return (found, true);
    }

    (exact, false)
}

/// First regular file in `data_dir` whose name equals `name` ignoring ASCII case.
///
/// Matches are sorted so that the result does not depend on directory order.
fn find_case_insensitive(data_dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(data_dir).ok()?;
    let mut matches: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
        .map(|entry| entry.path())
        .collect();

    matches.sort();

    matches.into_iter().next()
}

/// SHA-256 check of the mission file (`Mission_TestMissionFile`,
/// `mission.cpp:263-266`). A mismatch is reported as a warning only.
fn verify_mission_hash(
    path: &Path,
    hashes: &[String],
    tag: &str,
    warnings: &mut Vec<String>,
) -> Option<bool> {
    if hashes.is_empty() {
        return None;
    }

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            warnings.push(format!(
                "{tag}: cannot read mission file {}: {error}",
                path.display()
            ));
            return None;
        }
    };

    let digest = sha256_hex(&bytes);
    let matched = hashes
        .iter()
        .any(|declared| declared.eq_ignore_ascii_case(&digest));

    if !matched {
        warnings.push(format!(
            "{tag}: mission file {} hash mismatch (declared {}, actual {digest})",
            path.display(),
            hashes.join(", ")
        ));
    }

    Some(matched)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);

    for byte in digest {
        hex.push(HEX[usize::from(byte >> 4)] as char);
        hex.push(HEX[usize::from(byte & 0x0F)] as char);
    }

    hex
}

#[cfg(test)]
#[path = "mission_tests.rs"]
mod tests;
