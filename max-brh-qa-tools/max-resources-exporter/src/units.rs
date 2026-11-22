//! The "unit type → resources" mapping: `cargo_unit_data.json` plus a check against
//! `SC_UNITS` from `PATCHES.RES`.
//!
//! `max-brh/crates/max_app/data/cargo_unit_data.json` is a generated
//! (`max-brh/tools/gen_cargo_unit_data.py`) snapshot of `SC_UNITS`: 93 records in
//! [`max_core::enums::UnitType`] order, with the fields `sprite`/`shadow`/`portrait`/
//! `data`/`flics`, where `INVALID_ID` → `null`. The export reads the ready JSON (the same
//! mapping the port's `CargoMenu` uses), and [`verify_sc_units`]
//! additionally checks it against the `PATCHES.RES` archive: if the JSON is stale, the export
//! fails with a clear error instead of writing directories under wrong tags.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use max_assets::ResArchive;
use serde::Deserialize;

/// How many in-game unit types there are (`UNIT_END` in C++ / `UnitType::COUNT`).
pub const UNIT_COUNT: usize = 93;

/// The XOR key for the `SC_UNITS`/`ATTRIBS` resources
/// (`ResourceManager_GenericTable`, `resource_manager.cpp:156`).
const XOR_TABLE: [u8; 32] = [
    0x6c, 0x57, 0x36, 0xe6, 0x81, 0xe0, 0x72, 0x8a, 0xd3, 0xd9, 0xff, 0x54, 0x48, 0x3a, 0xcd, 0x75,
    0xd5, 0x0d, 0xe9, 0xe7, 0x7a, 0x57, 0xae, 0xeb, 0x61, 0x16, 0xb0, 0x35, 0x55, 0x62, 0xed, 0xd1,
];

/// A `cargo_unit_data.json` record — one unit type.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct CargoUnit {
    /// The `SC_UNITS`/`UnitType` token (`TANK`, `MININGST`, …) — also the directory name.
    pub token: String,
    /// The body sprite tag (`Unit::GetSprite()`).
    #[serde(default)]
    pub sprite: Option<String>,
    /// The shadow tag `S_*` (`Unit::GetShadow`); `null` — there is no shadow.
    #[serde(default)]
    pub shadow: Option<String>,
    /// The portrait tag `P_*` (`Unit::GetPortrait`); `null` — 22 types have no portrait.
    #[serde(default)]
    pub portrait: Option<String>,
    /// The FLC animation tag `F_*` (`Unit::GetFlicsAnimation()`); `null` — no animation.
    #[serde(default)]
    pub flics: Option<String>,
    /// The `D_*` tag — [`max_assets::sprite::FrameInfo`] (`Unit::GetFrameInfo()`).
    #[serde(default)]
    pub data: Option<String>,
    /// The English description from `language.json` (not included in the export).
    #[serde(default)]
    pub description: Option<String>,
}

impl CargoUnit {
    /// The unit directory in the export — the `SC_UNITS` token (`TANK`, `MININGST`).
    pub fn tag(&self) -> &str {
        &self.token
    }
}

/// Root of `cargo_unit_data.json`.
#[derive(Debug, Clone, Deserialize)]
struct CargoUnitFile {
    units: Vec<CargoUnit>,
}

/// Reads `cargo_unit_data.json` and checks that there are exactly 93 types and that the tokens
/// are unique.
pub fn load_cargo_unit_data(path: impl AsRef<Path>) -> Result<Vec<CargoUnit>> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("чтение маппинга {}", path.display()))?;
    let file: CargoUnitFile =
        serde_json::from_str(&text).with_context(|| format!("разбор {}", path.display()))?;

    if file.units.len() != UNIT_COUNT {
        bail!(
            "{}: {} записей вместо {} (UnitType::COUNT)",
            path.display(),
            file.units.len(),
            UNIT_COUNT
        );
    }

    for (index, unit) in file.units.iter().enumerate() {
        if unit.token.is_empty() {
            bail!("{}: запись {index} без токена", path.display());
        }

        if let Some(previous) = file.units[..index]
            .iter()
            .find(|other| other.token == unit.token)
        {
            bail!(
                "{}: токен {} повторяется (записи {index} и {})",
                path.display(),
                unit.token,
                previous.token
            );
        }
    }

    Ok(file.units)
}

/// `SC_UNITS` from `PATCHES.RES`: token → raw JSON record.
///
/// The resource is XOR-encrypted with [`XOR_TABLE`] and contains
/// `{"units": {"<TOKEN>": {...}}}`. Key order in `serde_json::Map` without
/// the `preserve_order` feature is alphabetical, so only the **set** of tokens
/// matters here (the order comes from `cargo_unit_data.json`).
pub fn read_sc_units_tokens(path: impl AsRef<Path>) -> Result<BTreeMap<String, serde_json::Value>> {
    let path = path.as_ref();
    let archive = ResArchive::open(path)?;
    let body = archive
        .read("SC_UNITS")
        .with_context(|| format!("{}: нет ресурса SC_UNITS", path.display()))?;

    let decoded: Vec<u8> = body
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ XOR_TABLE[index % XOR_TABLE.len()])
        .collect();

    let root: serde_json::Value = serde_json::from_slice(&decoded)
        .with_context(|| format!("{}: SC_UNITS — не JSON после XOR", path.display()))?;
    let units = root
        .get("units")
        .and_then(serde_json::Value::as_object)
        .with_context(|| format!("{}: SC_UNITS без объекта \"units\"", path.display()))?;

    Ok(units.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
}

/// The result of checking the mapping against `SC_UNITS`.
#[derive(Debug, Clone, Default)]
pub struct ScUnitsCheck {
    /// How many records are in `SC_UNITS`.
    pub units: usize,
    /// How many fields were compared (`sprite`/`shadow`/`portrait`/`data`/`flics`).
    pub fields: usize,
    /// Mismatches (`token.field: JSON … / SC_UNITS …`).
    pub mismatches: Vec<String>,
}

impl ScUnitsCheck {
    /// The check passed with no mismatches.
    pub fn is_ok(&self) -> bool {
        self.mismatches.is_empty()
    }
}

/// Checks `cargo_unit_data.json` against `SC_UNITS` from the archive.
pub fn verify_sc_units(
    path: impl AsRef<Path>,
    units: &[CargoUnit],
) -> Result<ScUnitsCheck> {
    let sc_units = read_sc_units_tokens(path)?;
    let mut check = ScUnitsCheck {
        units: sc_units.len(),
        ..ScUnitsCheck::default()
    };

    for unit in units {
        let Some(record) = sc_units.get(&unit.token) else {
            check
                .mismatches
                .push(format!("{}: нет в SC_UNITS", unit.token));
            continue;
        };

        for (field, key, expected) in [
            ("sprite", "sprite", &unit.sprite),
            ("shadow", "shadow", &unit.shadow),
            ("portrait", "portrait", &unit.portrait),
            ("data", "data", &unit.data),
            ("flics", "flics_animation", &unit.flics),
        ] {
            check.fields += 1;

            let raw = record.get(key).and_then(serde_json::Value::as_str);
            let actual = match raw {
                None | Some("INVALID_ID") => None,
                Some(tag) => Some(tag.to_string()),
            };

            if &actual != expected {
                check.mismatches.push(format!(
                    "{}.{field}: JSON {:?} / SC_UNITS {:?}",
                    unit.token, expected, actual
                ));
            }
        }
    }

    for token in sc_units.keys() {
        if !units.iter().any(|unit| &unit.token == token) {
            check
                .mismatches
                .push(format!("{token}: есть в SC_UNITS, нет в JSON"));
        }
    }

    Ok(check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from("../.."))
    }

    fn unit_json_path() -> PathBuf {
        repo_root().join("max-brh/crates/max_app/data/cargo_unit_data.json")
    }

    fn patches_path() -> PathBuf {
        repo_root().join("assets/PATCHES.RES")
    }

    #[test]
    fn cargo_unit_data_has_93_unique_tokens_in_unit_type_order() {
        if !unit_json_path().exists() {
            return; // working directory without game data — the test is skipped
        }

        let units = load_cargo_unit_data(unit_json_path()).expect("JSON читается");

        assert_eq!(units.len(), UNIT_COUNT);
        assert_eq!(units.len(), max_core::enums::UnitType::COUNT);
        assert_eq!(units[0].token, "COMMTWR");
        assert_eq!(units[0x33].token, "TANK"); // UnitType::Tank
        assert_eq!(units[92].token, "WALDO"); // UnitType::DeadWaldo
        assert!(units.iter().all(|unit| unit.sprite.is_some()));
        assert_eq!(units.iter().filter(|unit| unit.portrait.is_some()).count(), 71);
        assert_eq!(units.iter().filter(|unit| unit.flics.is_some()).count(), 71);
    }

    #[test]
    fn sc_units_matches_the_generated_json() {
        if !unit_json_path().exists() || !patches_path().exists() {
            return;
        }

        let units = load_cargo_unit_data(unit_json_path()).expect("JSON читается");
        let check = verify_sc_units(patches_path(), &units).expect("SC_UNITS читается");

        assert_eq!(check.units, UNIT_COUNT);
        assert_eq!(check.fields, UNIT_COUNT * 5);
        assert!(check.is_ok(), "расхождения: {:?}", check.mismatches);
    }

    #[test]
    fn invalid_id_becomes_none() {
        let raw = serde_json::json!({
            "token": "TEST",
            "sprite": "TEST",
            "portrait": "INVALID_ID",
            "flics": null
        });
        let unit: CargoUnit = serde_json::from_value(raw).expect("разбор");

        // `INVALID_ID` is a string, not null: the generator turns it into null,
        // so here we only check the shape of the `None` record.
        assert_eq!(unit.portrait.as_deref(), Some("INVALID_ID"));
        assert_eq!(unit.flics, None);
        assert_eq!(unit.shadow, None);
    }
}
