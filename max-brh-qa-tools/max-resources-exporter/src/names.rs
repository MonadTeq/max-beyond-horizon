//! Английские имена юнитов по индексу `UnitType` (0..93).
//!
//! Источник — таблица `unit_name` из `max-brh/crates/max_app/src/buildmenu.rs`
//! (в C++ это `ResourceManager_GetUnit(...).GetSingularName()`). В порте имена
//! есть только у 61 «настоящего» типа; для служебных (эффекты, ракеты, обломки,
//! инопланетная техника) `None` — в манифесте и `unit.json` берётся токен
//! `SC_UNITS`. Порядок совпадает с [`max_core::enums::UnitType::ALL`], значения
//! индексов — `ResourceID` из `SC_UNITS`.
#![allow(clippy::unreadable_literal)]

/// Имя типа или `None`, если в порте его нет.
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

/// Имя типа по индексу `UnitType`; для служебных типов — сам токен.
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
        // Служебный тип — имени нет, отдаётся токен.
        assert_eq!(display_name(0x2B, "LNDEXPLD"), "LNDEXPLD");
        assert_eq!(display_name(200, "OUT_OF_RANGE"), "OUT_OF_RANGE");
    }
}
