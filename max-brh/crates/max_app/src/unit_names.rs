//! User-Friendly Name (UFN) of unit types — the presentation layer.
//!
//! Owner decision, phase 0 item 4 (`max-brh-docs/development/PLAN-refactor.md` §3):
//! UFN strings are **not** part of `max_core` — only the resource token lives there
//! ([`UnitType::tag`]). The UFN table is application data:
//! [`UNIT_NAMES_JSON`] (`crates/max_app/data/unit_names.json`), the key is the token.
//!
//! The canon of values is `max-brh-docs/unit-name-review.md`, column
//! «User-Friendly Name» (an English string, as in the original manual/game).
//! 70 units/buildings/alien types are covered; 23 non-units (effects, projectiles, trails,
//! debris, decorations, `MASTER_BUILDER`, `HARVSTER`, `WALDO`) are not shown in the game
//! UI — for them [`unit_user_friendly_name`] returns `tag()`.
//!
//! C++: `ResourceManager_GetUnit(...).GetSingularName()`/`GetPluralName()`
//! (`resourcetable.hpp`, resource `SC_UNITS`); the strings are `assets/language.json`.

use std::collections::HashMap;
use std::sync::OnceLock;

use max_core::enums::UnitType;
use serde::Deserialize;

/// UFN JSON resource (70 records; generated/verified against the canon manually).
pub const UNIT_NAMES_JSON: &str = include_str!("../data/unit_names.json");

/// Contents of [`UNIT_NAMES_JSON`]: metadata (`$comment`/`$nonUnits`) and the
/// «token → UFN» table.
#[derive(Debug, Default, Deserialize)]
struct UnitNamesFile {
    /// [`UnitType::tag`] token → User-Friendly Name.
    #[serde(default)]
    units: HashMap<String, String>,
}

/// Parsed table (once; a parse error yields an empty table).
fn unit_names() -> &'static UnitNamesFile {
    static FILE: OnceLock<UnitNamesFile> = OnceLock::new();

    FILE.get_or_init(|| serde_json::from_str::<UnitNamesFile>(UNIT_NAMES_JSON).unwrap_or_default())
}

/// User-Friendly Name of a type — the string for the game UI.
///
/// `GoldRefinery` → `"Gold Refinery"`, `MiningStation` → `"Mining Station"`.
/// For the 23 non-units (effects/decorations) there is no record — it returns
/// [`UnitType::tag`] (`"LNDEXPLD"`, `"WALDO"`, …) so that the function is total
/// and a call from the debug UI never shows an empty string.
pub fn unit_user_friendly_name(unit: UnitType) -> &'static str {
    unit_names()
        .units
        .get(unit.tag())
        .map_or_else(|| unit.tag(), |name| name.as_str())
}

/// Plural name of a type — the presentation form of C++ `Unit::GetPluralName()`
/// (`units.cpp:314`, `SC_UNITS` `plural_name` → `language.json`).
///
/// C++ needs it for the turn-start report (`GameManager_ReportNewUnitsMessage`,
/// `game_manager.cpp:8123-8135`: `%i new %s`) and for the built-in unit report.
/// The port's canon name is the UFN ([`unit_user_friendly_name`]), so the plural is
/// derived from the UFN itself instead of the oracle's `plural_name` table (which
/// is keyed by the original singular name and disagrees with the UFN for
/// `ADUMP`/`FDUMP`/`RESEARCH`/`JUGGRNT`/…).
///
/// English rules: the uncountable nouns used by the game keep the singular form
/// (`Infantry`, `Barracks`, `Artillery` — the same in the oracle table), a
/// consonant + `y` becomes `ies` (`Gold Refineries`), a sibilant takes `es`,
/// everything else takes `s`.
pub fn unit_plural_name(unit: UnitType) -> String {
    let singular = unit_user_friendly_name(unit);

    // `plural_name` of `INFANTRY`/`BARRACKS`/`ARTYTRRT` in the oracle table.
    if matches!(
        unit,
        UnitType::Infantry | UnitType::Barracks | UnitType::Artillery
    ) {
        return singular.to_string();
    }

    let last = singular.chars().last();
    let before_last = singular.chars().rev().nth(1);

    if last == Some('y') && !matches!(before_last, Some('a' | 'e' | 'i' | 'o' | 'u')) {
        format!("{}ies", &singular[..singular.len() - 1])
    } else if singular.ends_with('s')
        || singular.ends_with('x')
        || singular.ends_with("ch")
        || singular.ends_with("sh")
    {
        format!("{singular}es")
    } else {
        format!("{singular}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 23 non-units: effects, projectiles/trails, debris/decorations, service types.
    /// The manual does not describe them, they are not shown in the UI (no UFN).
    const NON_UNITS: [UnitType; 23] = [
        UnitType::LargeRubble,
        UnitType::SmallRubble,
        UnitType::LargeTape,
        UnitType::SmallTape,
        UnitType::LargeSlab,
        UnitType::SmallSlab,
        UnitType::LargeCones,
        UnitType::SmallCones,
        UnitType::LandExplosion,
        UnitType::AirExplosion,
        UnitType::SeaExplosion,
        UnitType::BuildingExplosion,
        UnitType::HitExplosion,
        UnitType::MasterBuilder,
        UnitType::Rocket,
        UnitType::Torpedo,
        UnitType::AlienMissile,
        UnitType::AlienTankBall,
        UnitType::AlienArtilleryBall,
        UnitType::RocketSmoke,
        UnitType::TorpedoBubble,
        UnitType::Harvester,
        UnitType::DeadWaldo,
    ];

    #[test]
    fn table_matches_canon() {
        assert_eq!(UnitType::GoldRefinery.tag(), "COMMTWR");
        assert_eq!(
            unit_user_friendly_name(UnitType::GoldRefinery),
            "Gold Refinery"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::MaterialStorage),
            "Material Storage"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::FuelStorage),
            "Fuel Storage"
        );
        assert_eq!(unit_user_friendly_name(UnitType::GoldVault), "Gold Vault");
        assert_eq!(
            unit_user_friendly_name(UnitType::ResearchCentre),
            "Research Centre"
        );
        assert_eq!(unit_user_friendly_name(UnitType::EcoSphere), "Eco-Sphere");
        assert_eq!(
            unit_user_friendly_name(UnitType::MissileLauncher),
            "Missile Launcher"
        );
        assert_eq!(unit_user_friendly_name(UnitType::Artillery), "Artillery");
        assert_eq!(
            unit_user_friendly_name(UnitType::ConcreteBlock),
            "Concrete Block"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::ArmouredPersonnelCarrier),
            "Armoured Personnel Carrier"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::Infiltrator),
            "Infiltrator"
        );
        assert_eq!(unit_user_friendly_name(UnitType::Escort), "Escort");
        assert_eq!(unit_user_friendly_name(UnitType::Gunboat), "Gunboat");
        assert_eq!(
            unit_user_friendly_name(UnitType::MissileCruiser),
            "Missile Cruiser"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::Bomber),
            "Ground Attack Plane"
        );
        assert_eq!(unit_user_friendly_name(UnitType::Awac), "AWAC");
        assert_eq!(
            unit_user_friendly_name(UnitType::SeaTransport),
            "Sea Transport"
        );
        assert_eq!(unit_user_friendly_name(UnitType::CargoShip), "Cargo Ship");
    }

    #[test]
    fn table_covers_70_units_and_23_non_units() {
        let table = unit_names();
        assert_eq!(table.units.len(), 70);
        assert_eq!(NON_UNITS.len() + table.units.len(), UnitType::COUNT);

        // Every key is a token of an existing type (no "dangling" records).
        for token in table.units.keys() {
            assert!(
                UnitType::ALL.iter().any(|unit_type| unit_type.tag() == token),
                "ключ «{token}» не соответствует ни одному UnitType::tag()"
            );
        }

        // Units: a record exists, the UFN is non-empty (`Awac` is a canon exception:
        // its User-Friendly Name «AWAC» coincides with the `AWAC` token).
        for unit_type in UnitType::ALL {
            if NON_UNITS.contains(unit_type) {
                continue;
            }
            assert!(
                table.units.contains_key(unit_type.tag()),
                "{unit_type:?}: нет записи UFN (токен {})",
                unit_type.tag()
            );
            let name = unit_user_friendly_name(*unit_type);
            assert!(!name.is_empty(), "{unit_type:?}: пустая UFN");
        }

        // Non-units: no record, the function returns tag().
        for unit_type in NON_UNITS {
            assert!(
                !table.units.contains_key(unit_type.tag()),
                "{unit_type:?}: у эффекта/декорации не должно быть UFN"
            );
            assert_eq!(unit_user_friendly_name(unit_type), unit_type.tag());
        }
    }
}
