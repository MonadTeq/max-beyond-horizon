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
mod tests {
    use super::*;

    /// All 93 types are present in the resource, the order matches `UnitType`.
    #[test]
    fn resource_carries_all_unit_types_in_enum_order() {
        assert!(is_loaded(), "JSON не разобрался: {} записей", units().len());

        for index in 0..UNIT_END {
            let unit = UnitType::try_from(index).expect("UnitType 0..92");
            let data = unit_data(unit).expect("запись типа");
            assert!(!data.token.is_empty(), "{unit:?}");
        }

        // C++ tokens of special types (verification of `SC_UNITS`/`UnitType` order).
        assert_eq!(unit_token(UnitType::GoldRefinery), "COMMTWR");
        assert_eq!(unit_token(UnitType::HeavyVehiclePlant), "LANDPLT");
        assert_eq!(unit_token(UnitType::MineLayer), "MINELAYR");
        assert_eq!(unit_token(UnitType::DeadWaldo), "WALDO");
    }

    /// A portrait exists for all except the 22 non-units of the original (`INVALID_ID`).
    #[test]
    fn portraits_match_sc_units() {
        let with_portrait = (0..UNIT_END)
            .map(|index| UnitType::try_from(index).unwrap())
            .filter(|unit| unit_portrait(*unit).is_some())
            .count();

        assert_eq!(with_portrait, 71, "SC_UNITS: 71 ресурс P_*");
        assert_eq!(unit_portrait(UnitType::Tank), Some("P_TANK"));
        assert_eq!(unit_portrait(UnitType::GoldRefinery), Some("P_TRANSP"));
        assert_eq!(unit_portrait(UnitType::HeavyVehiclePlant), Some("P_HVYPLT"));
        assert_eq!(unit_portrait(UnitType::Constructor), Some("P_CONTRC"));
        // Non-units: `portrait` = `INVALID_ID` in `SC_UNITS`.
        for unit in [
            UnitType::Rocket,
            UnitType::Torpedo,
            UnitType::Harvester,
            UnitType::DeadWaldo,
            UnitType::LargeRubble,
        ] {
            assert_eq!(unit_portrait(unit), None, "{unit:?}");
        }
    }

    /// Descriptions: 69 non-empty; every type reachable in `CargoMenu` is described.
    #[test]
    fn descriptions_cover_every_cargo_menu_unit() {
        let described = (0..UNIT_END)
            .map(|index| UnitType::try_from(index).unwrap())
            .filter(|unit| !unit_description(*unit).is_empty())
            .count();
        assert_eq!(described, 69, "69 описаний в language.json");

        // The texts are verbatim `language.json` (en-US).
        assert!(unit_description(UnitType::Tank).starts_with("Heavily armored fighting vehicle."));
        assert!(unit_description(UnitType::Scout)
            .starts_with("High speed scouting vehicle for exploration and spotting enemy units."));
        assert_eq!(
            unit_description(UnitType::Road),
            "Units move twice as fast over road squares."
        );

        // 24 types without a description — debris/explosions/projectiles/`HARVSTER`/`WALDO`;
        // none passes `IsUnitFiltered` (checked in `mobilize`).
        for unit in [
            UnitType::Rocket,
            UnitType::Torpedo,
            UnitType::LandExplosion,
            UnitType::LargeRubble,
            UnitType::Harvester,
            UnitType::DeadWaldo,
            UnitType::SupportPlant,
            UnitType::RecreationCenter,
        ] {
            assert_eq!(unit_description(unit), "", "{unit:?}");
        }
    }

    /// `Unit::GetSprite()`/`GetShadow()`: all 93 types have a body,
    /// shadow — for 77 (missiles/explosions/films/`WALDO` have `INVALID_ID` in `SC_UNITS`).
    #[test]
    fn sprite_and_shadow_follow_sc_units() {
        let sprites = (0..UNIT_END)
            .map(|index| UnitType::try_from(index).unwrap())
            .filter(|unit| unit_sprite(*unit).is_some())
            .count();
        let shadows = (0..UNIT_END)
            .map(|index| UnitType::try_from(index).unwrap())
            .filter(|unit| unit_shadow(*unit).is_some())
            .count();

        assert_eq!(sprites, 93, "SC_UNITS: sprite у всех 93 типов");
        assert_eq!(shadows, 77, "SC_UNITS: shadow у 77 типов");

        assert_eq!(unit_sprite(UnitType::Tank), Some("TANK"));
        assert_eq!(unit_shadow(UnitType::Tank), Some("S_TANK"));
        assert_eq!(unit_sprite(UnitType::GunTurret), Some("GUNTURRT"));
        assert_eq!(unit_shadow(UnitType::GunTurret), Some("S_GUNTUR"));
        assert_eq!(unit_sprite(UnitType::MasterBuilder), Some("MASTER"));
        assert_eq!(unit_shadow(UnitType::MasterBuilder), Some("S_MASTER"));
        // Submarine: `SC_UNITS` references `S_SUBMAR`, which is absent from
        // MAX.RES/PATCHES.RES — the asset loader checks the resource.
        assert_eq!(unit_shadow(UnitType::Submarine), Some("S_SUBMAR"));
        // Films/missiles/explosions and `WALDO` — without a shadow in the original.
        for unit in [
            UnitType::LargeTape,
            UnitType::SmallTape,
            UnitType::SeaMine,
            UnitType::Rocket,
            UnitType::AlienMissile,
            UnitType::DeadWaldo,
        ] {
            assert_eq!(unit_shadow(unit), None, "{unit:?}");
        }
    }

    /// `Unit::GetFrameInfo()`: all 93 types have a `D_*` resource; the tank,
    /// the mobile AA and the escort — a separate turret.
    #[test]
    fn frame_data_covers_all_units() {
        let with_data = (0..UNIT_END)
            .map(|index| UnitType::try_from(index).unwrap())
            .filter(|unit| unit_frame_data(*unit).is_some())
            .count();
        assert_eq!(with_data, 93, "SC_UNITS: 93 ресурса D_*");

        assert_eq!(unit_frame_data(UnitType::Tank), Some("D_TANK"));
        assert_eq!(unit_frame_data(UnitType::MobileAntiAir), Some("D_SP_FLK"));
        assert_eq!(unit_frame_data(UnitType::Escort), Some("D_ESCORT"));
        assert_eq!(unit_frame_data(UnitType::GunTurret), Some("D_FIXED"));
        assert_eq!(unit_frame_data(UnitType::Scanner), Some("D_SCANNR"));
    }

    /// The table is non-empty even on a JSON failure — but then `is_loaded()` is false.
    #[test]
    fn json_resource_is_the_generated_file() {
        assert!(CARGO_UNIT_DATA_JSON.contains("\"COMMTWR\""));
        assert!(CARGO_UNIT_DATA_JSON.contains("\"P_CONTRC\""));
        assert!(CARGO_UNIT_DATA_JSON.contains("\"D_TANK\""));
        assert!(CARGO_UNIT_DATA_JSON.len() > 15_000);
    }
}
