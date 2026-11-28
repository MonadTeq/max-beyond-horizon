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
use max_core::unit::UnitInfo;
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

// ---------------------------------------------------------------------------
// Instance names (`UnitInfo::GetName` / `GetDisplayName`)
// ---------------------------------------------------------------------------

/// One Roman digit — an exact port of `UnitInfo::CalcRomanDigit`
/// (`unitinfo.cpp:805-826`).
///
/// `value` is one decimal digit of the "mark" level: `0..=9` in normal play,
/// but the C++ loop also accepts larger values (it appends `one` that many
/// times after subtracting 5/4), so the port keeps the same arithmetic instead
/// of a `0..=9` match — `version / 100` for a level above 999 takes this path.
///
/// C++ appends into a fixed 20-byte buffer (`Text_AppendUtf8(..., size)`);
/// the port builds a `String` — the C++ call sites already limit the level to
/// `GET_VERSION`-sized values (`game_manager.cpp:2864` caps the upgrade loop
/// at 30), so the truncation never triggers.
pub fn calc_roman_digit(out: &mut String, mut value: u32, one: &str, five: &str, ten: &str) {
    if value == 9 {
        out.push_str(one);
        out.push_str(ten);
        return;
    }

    if value >= 5 {
        out.push_str(five);
        value -= 5;
    }

    if value >= 4 {
        out.push_str(one);
        out.push_str(five);
        value -= 4;
    }

    for _ in 0..value {
        out.push_str(one);
    }
}

/// `UnitInfo::GetVersion` (`unitinfo.cpp:915-925`): the upgrade "mark" level in
/// Roman numerals — `1` → `I`, `2` → `II`, `4` → `IV`, `9` → `IX`, `30` → `XXX`.
///
/// The level is `UnitValues::version` (starts at 1 and grows on upgrades,
/// `unitvalues.cpp`); the port keeps the C++ three-digit decomposition.
pub fn roman_version(mut version: u32) -> String {
    let mut out = String::new();

    calc_roman_digit(&mut out, version / 100, "C", "D", "M");
    version %= 100;
    calc_roman_digit(&mut out, version / 10, "X", "L", "C");
    version %= 10;
    calc_roman_digit(&mut out, version, "I", "V", "X");

    out
}

/// `UnitInfo::GetName` (`unitinfo.cpp:757-779`): the user name if set,
/// otherwise `"%s %i"` — the type name and the instance number
/// ([`UnitInfo::unit_id`]).
///
/// `None` → `"Scout 7"`, `Some("Oddball")` → `"Oddball"` (the number is not
/// appended to a user name — C++ prints the stored string as is).
pub fn unit_name(unit: &UnitInfo) -> String {
    match &unit.name {
        Some(name) => name.clone(),
        None => format!(
            "{} {}",
            unit_user_friendly_name(unit.unit_type),
            unit.unit_id
        ),
    }
}

/// `UnitInfo::GetDisplayName` (`unitinfo.cpp:781-803`): `"Mk %s %s"` — the
/// Roman "mark" level of the unit's values and [`unit_name`].
///
/// `Mk I Scout 7` for an unnamed Scout at level 1; a named unit keeps the prefix:
/// `Mk II Oddball`. The level comes from the unit's own `base_values` (upgrades
/// copy the values into the instance), default 1 → `Mk I`.
pub fn unit_display_name(unit: &UnitInfo) -> String {
    format!(
        "Mk {} {}",
        roman_version(u32::from(unit.base_values.version)),
        unit_name(unit)
    )
}

#[cfg(test)]
#[path = "unit_names_tests.rs"]
mod tests;
