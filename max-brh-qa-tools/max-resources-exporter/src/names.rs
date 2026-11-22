//! English unit names by `UnitType` index (0..93).
//!
//! Source — the `unit_name` table from `max-brh/crates/max_app/src/buildmenu.rs`
//! (in C++ it is `ResourceManager_GetUnit(...).GetSingularName()`). In the port the names
//! exist only for the 61 "real" types; for the service ones (effects, rockets, debris,
//! alien hardware) it is `None` — the manifest and `unit.json` use the
//! `SC_UNITS` token instead. The order matches [`max_core::enums::UnitType::ALL`], and the
//! index values are the `ResourceID`s from `SC_UNITS`.
#![allow(clippy::unreadable_literal)]

/// The type name, or `None` if the port does not have one.
pub const UNIT_NAMES: [Option<&str>; 93] = [
    Some("Comm Tower"),
    Some("Power Station"),
    Some("Power Generator"),
    Some("Barracks"),
    None,
    Some("Radar"),
    Some("Ammo Dump"),
    Some("Fuel Tank"),
    Some("Gold Vault"),
    Some("Depot"),
    Some("Hangar"),
    Some("Dock"),
    Some("Connector"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some("Road"),
    Some("Landing Pad"),
    Some("Shipyard"),
    Some("Light Vehicle Plant"),
    Some("Heavy Vehicle Plant"),
    None,
    Some("Air Units Plant"),
    Some("Habitat"),
    Some("Research Center"),
    Some("Greenhouse"),
    None,
    Some("Training Hall"),
    Some("Water Platform"),
    Some("Gun Turret"),
    Some("Anti Aircraft"),
    Some("Artillery Turret"),
    Some("Missile Launcher"),
    Some("Concrete Block"),
    Some("Bridge"),
    Some("Mining Station"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some("Constructor"),
    Some("Scout"),
    Some("Tank"),
    Some("Assault Gun"),
    Some("Rocket Launcher"),
    Some("Missile Crawler"),
    Some("Mobile AA"),
    Some("Mine Layer"),
    Some("Surveyor"),
    Some("Scanner"),
    Some("Supply Truck"),
    Some("Gold Truck"),
    Some("Engineer"),
    Some("Bulldozer"),
    Some("Repair Unit"),
    Some("Fuel Truck"),
    Some("Personnel Carrier"),
    Some("Infiltrator"),
    Some("Infantry"),
    Some("Escort"),
    Some("Corvette"),
    Some("Gunboat"),
    Some("Submarine"),
    Some("Sea Transport"),
    Some("Missile Cruiser"),
    Some("Sea Mine Layer"),
    Some("Cargo Ship"),
    Some("Fighter"),
    Some("Ground Attack Plane"),
    Some("Air Transport"),
    Some("AWAC"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
];

/// The type name by `UnitType` index; for service types — the token itself.
pub fn display_name<'a>(index: usize, token: &'a str) -> &'a str {
    UNIT_NAMES.get(index).copied().flatten().unwrap_or(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_covers_every_unit_type() {
        assert_eq!(UNIT_NAMES.len(), max_core::enums::UnitType::COUNT);
        assert_eq!(UNIT_NAMES.len(), 93);
        assert_eq!(display_name(0x33, "TANK"), "Tank");
        assert_eq!(display_name(0x00, "COMMTWR"), "Comm Tower");
        assert_eq!(display_name(49, "CONSTRCT"), "Constructor");
        // Service type — there is no name, the token is returned.
        assert_eq!(display_name(0x2B, "LNDEXPLD"), "LNDEXPLD");
        assert_eq!(display_name(200, "OUT_OF_RANGE"), "OUT_OF_RANGE");
    }
}
