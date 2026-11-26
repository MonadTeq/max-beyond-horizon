//! Unit record data for the mobilization screen (`CargoMenu`).
//!
//! Oracle: `cargomenu.cpp`/`abstractupgrademenu.cpp` —
//! `Unit::GetPortrait()` (`P_*` big image 300×240) and
//! `Unit::GetDescription()` (`language.json` by the hex key from `SC_UNITS`).
//! The C++ port does not keep this data in code: it is read from the `SC_UNITS`
//! resource (`assets/PATCHES.RES`, `Units::LoadResource`) and the
//! `assets/language.json` catalog (`Language::GetEntry`; `0xffff` → empty string).
//!
//! The generator `max-brh/tools/gen_cargo_unit_data.py` decodes
//! `SC_UNITS` (the XOR table `ResourceManager_GenericTable`) and puts
//! 93 records into [`CARGO_UNIT_DATA_JSON`] (the file
//! `crates/max_app/data/cargo_unit_data.json`, committed). The table goes in
//! [`UnitType`] order (ResourceID 0..92), so a type record is
//! `units[unit as usize]`.
//!
//! Texts — `en-US`, like the other client labels (`assets/language.json`,
//! the default locale). 24 types have no description in the original — debris,
//! explosions, missiles, `HARVSTER`, `WALDO` — and are unreachable in `CargoMenu`
//! (`IsUnitFiltered`: no `UPGRADABLE`/not buildable), i.e. every type
//! openable by the player has a non-empty description.

use std::sync::OnceLock;

use max_core::enums::{UnitType, UNIT_END};
use serde::Deserialize;

/// JSON resource with 93 records (generated from `SC_UNITS` + `language.json`).
pub const CARGO_UNIT_DATA_JSON: &str = include_str!("../data/cargo_unit_data.json");

/// One `SC_UNITS` record: the C++ token, sprite/shadow, portrait, turret frames and
/// the description (en-US).
#[derive(Debug, Clone, Deserialize)]
pub struct CargoUnitData {
    /// C++ resource token (`TANK`, `LANDPLT`, …) — for order verification.
    pub token: String,
    /// Body resource — `Unit::GetSprite()` (`TANK`, `GUNTURRT`, …);
    /// `None` — `INVALID_ID` (in the original it is set for all 93 types).
    pub sprite: Option<String>,
    /// Shadow resource `S_*` — `Unit::GetShadow()`; `None` — `INVALID_ID`
    /// (missiles, explosions, films and `WALDO` have no shadow).
    pub shadow: Option<String>,
    /// Portrait resource `P_*` (`Unit::GetPortrait`); `None` — `INVALID_ID`.
    pub portrait: Option<String>,
    /// Armory portrait `A_*` (`Unit::GetArmoryPortrait`), drawn in the
    /// `RepairShopMenu` slots (`repairshopmenu.cpp:243-245`); `None` — `INVALID_ID`.
    pub armory_portrait: Option<String>,
    /// Description text (`Unit::GetDescription`); empty if there is none in the original.
    pub description: String,
    /// `D_*` resource — `Unit::GetFrameInfo()`: body/turret frame bases,
    /// the frame count and `angle_offsets` for the turret overlay.
    pub data: Option<String>,
    /// `F_*` resource — `Unit::GetFlicsAnimation()`: the portrait FLC animation
    /// `WINDOW_CORNER_FLIC` (`GameManager_MenuCreateFlic`); `None` — `INVALID_ID`.
    pub flics: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CargoUnitFile {
    units: Vec<CargoUnitData>,
}

/// Parsed data (index — [`UnitType`] as `usize`).
fn units() -> &'static [CargoUnitData] {
    static UNITS: OnceLock<Vec<CargoUnitData>> = OnceLock::new();

    UNITS
        .get_or_init(|| {
            serde_json::from_str::<CargoUnitFile>(CARGO_UNIT_DATA_JSON)
                .map(|file| file.units)
                .unwrap_or_default()
        })
        .as_slice()
}

/// Type record; `None` if the resource did not parse or the index is outside the table.
pub fn unit_data(unit: UnitType) -> Option<&'static CargoUnitData> {
    units().get(unit as usize)
}

/// C++ type token (`TANK`, …); `""` if there is no data.
pub fn unit_token(unit: UnitType) -> &'static str {
    unit_data(unit).map_or("", |data| data.token.as_str())
}

/// `Unit::GetSprite()` — the body resource name (`TANK`, `GUNTURRT`, …).
///
/// The source is `SC_UNITS`, not a guess: `UNIT_SPRITES` is checked against this
/// function by a test. `None` — `INVALID_ID` in the original.
pub fn unit_sprite(unit: UnitType) -> Option<&'static str> {
    unit_data(unit).and_then(|data| data.sprite.as_deref())
}

/// `Unit::GetShadow()` — the `S_*` shadow resource name; `None` — no shadow.
pub fn unit_shadow(unit: UnitType) -> Option<&'static str> {
    unit_data(unit).and_then(|data| data.shadow.as_deref())
}

/// `Unit::GetDescription()` for `CargoMenu`; `""` — no description.
///
/// Corresponds to `ResourceManager_GetLanguageEntry(0xffff)` → `std::string()`
/// and `Language::GetEntry` for the hex key from `SC_UNITS`.
pub fn unit_description(unit: UnitType) -> &'static str {
    unit_data(unit).map_or("", |data| data.description.as_str())
}

/// `Unit::GetPortrait()` — the `P_*` resource name (big image 300×240).
pub fn unit_portrait(unit: UnitType) -> Option<&'static str> {
    unit_data(unit).and_then(|data| data.portrait.as_deref())
}

/// `Unit::GetArmoryPortrait()` — the `A_*` resource name (128×128; aircraft
/// 200×128) of the `RepairShopMenu` slot portrait (`repairshopmenu.cpp:243-245`).
///
/// Every holdable unit (a mobile land/sea/air unit with a `cargo_type`) has one;
/// `None` — `INVALID_ID` (buildings and the misc types, which are never stored).
pub fn unit_armory_portrait(unit: UnitType) -> Option<&'static str> {
    unit_data(unit).and_then(|data| data.armory_portrait.as_deref())
}

/// `Unit::GetFrameInfo()` — the `D_*` resource name (body/turret frames,
/// `angle_offsets`); `None` — `INVALID_ID`.
pub fn unit_frame_data(unit: UnitType) -> Option<&'static str> {
    unit_data(unit).and_then(|data| data.data.as_deref())
}

/// `Unit::GetFlicsAnimation()` — the `F_*` resource tag with the portrait FLC animation
/// (`WINDOW_CORNER_FLIC`, 128×128); `None` — the type has no animation (`INVALID_ID`).
///
/// The resource itself is an ASCIIZ name of a `*.FLC` next to `MAX.RES`: the file is loaded by
/// [`crate::assets::Assets`], the frames are decoded by `max_assets::flic`.
pub fn unit_flic(unit: UnitType) -> Option<&'static str> {
    unit_data(unit).and_then(|data| data.flics.as_deref())
}

/// Whether all [`UNIT_END`] records parsed (diagnostics/tests).
pub fn is_loaded() -> bool {
    units().len() == UNIT_END as usize
}

#[cfg(test)]
#[path = "cargo_unit_data_tests.rs"]
mod tests;
