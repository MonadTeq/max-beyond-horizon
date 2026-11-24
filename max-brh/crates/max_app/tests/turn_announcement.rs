//! Turn-start announcement — `GameManager_ReportNewUnitsMessage`
//! (`game_manager.cpp:8103-8181`).
//!
//! In the original, the start of a player's turn opens a modeless
//! `MESSAGE_BOX_INFO` window: «Begin turn N.» followed by what the turn produced —
//! `a new %s` / `%i new %s` per type, joined with ` and ` / `, `, then
//! ` is/are available this turn.`. The chunks are collected while walking
//! `unit_type` **downwards** and prepended, so the lowest type comes first.
//!
//! These tests pin the text; the `production.qa` script pins the window itself.

use max_app::local::new_units_message;
use max_app::unit_names::unit_plural_name;
use max_core::enums::UnitType;
use max_core::unit::UnitId;
use max_core::{ProductionEvent, ProductionEventKind};

fn event(unit_type: UnitType, kind: ProductionEventKind) -> ProductionEvent {
    ProductionEvent {
        builder: UnitId(1),
        unit_type,
        spawned: None,
        kind,
    }
}

/// No production: the window still opens with the turn number alone.
#[test]
fn empty_turn_reports_only_the_turn_number() {
    assert_eq!(new_units_message(3, &[]), "Begin turn 3.\n");
}

/// One unit — `_(c64e)` `a new %s` + `_(a8bd)` ` is`.
#[test]
fn one_new_unit_is_singular() {
    let events = [event(UnitType::Infantry, ProductionEventKind::Spawned)];

    assert_eq!(
        new_units_message(5, &events),
        "Begin turn 5.\nA new Infantry is available this turn."
    );
}

/// Several units of one type — `_(5e9c)` `%i new %s` with the plural name
/// (`Unit::GetPluralName`) + `_(2cd0)` ` are`.
#[test]
fn several_new_units_use_the_plural_name() {
    let events = [
        event(UnitType::Tank, ProductionEventKind::Spawned),
        event(UnitType::Tank, ProductionEventKind::Spawned),
    ];

    assert_eq!(
        new_units_message(7, &events),
        "Begin turn 7.\n2 new Tanks are available this turn."
    );
}

/// `Infantry` is uncountable in the oracle's `plural_name` table: the sentence keeps
/// the singular form.
#[test]
fn uncountable_names_stay_in_the_singular_form() {
    let events = [
        event(UnitType::Infantry, ProductionEventKind::Spawned),
        event(UnitType::Infantry, ProductionEventKind::Spawned),
    ];

    assert_eq!(
        new_units_message(2, &events),
        "Begin turn 2.\n2 new Infantry are available this turn."
    );
}

/// Two types: C++ walks the types downwards and prepends, so the **lowest**
/// `unit_type` ends up first (`TRAINHAL` 0x20 before `INFANTRY` 0x43), and the
/// second chunk is glued with `_(e3ee)` ` and `.
#[test]
fn different_types_are_joined_lowest_first() {
    let events = [
        event(UnitType::Infantry, ProductionEventKind::Spawned),
        event(UnitType::TrainingHall, ProductionEventKind::BuildingDone),
    ];

    assert_eq!(
        new_units_message(12, &events),
        "Begin turn 12.\nA new Training Hall and a new Infantry are available this turn."
    );
}

/// Three types: the extra chunks use `_(0a07)` `, `; the final order is ascending by
/// `unit_type` (`TRAINHAL` 0x20, `CONSTRCT` 0x31, `INFANTRY` 0x43).
#[test]
fn three_types_use_the_comma_separator() {
    let events = [
        event(UnitType::Infantry, ProductionEventKind::Spawned),
        event(UnitType::TrainingHall, ProductionEventKind::BuildingDone),
        event(UnitType::Constructor, ProductionEventKind::Spawned),
    ];

    assert_eq!(
        new_units_message(9, &events),
        "Begin turn 9.\nA new Training Hall, a new Constructor and a new Infantry are available this turn."
    );
}

/// Only `SpawnNewUnit` completions count: pauses and the bulldozer's rubble clearing
/// are not part of the C++ `unit_counters`.
#[test]
fn paused_and_bulldozer_events_are_not_announced() {
    let events = [
        event(UnitType::Tank, ProductionEventKind::PausedNoMaterials),
        event(UnitType::Tank, ProductionEventKind::NoSpawnCell),
        event(UnitType::Tank, ProductionEventKind::RubbleCleared),
    ];

    assert_eq!(new_units_message(4, &events), "Begin turn 4.\n");
}

/// `Unit::GetPluralName()` equivalents used by the announcement.
#[test]
fn plural_names_follow_the_canon_names() {
    assert_eq!(unit_plural_name(UnitType::Tank), "Tanks");
    assert_eq!(unit_plural_name(UnitType::Infantry), "Infantry");
    assert_eq!(unit_plural_name(UnitType::Barracks), "Barracks");
    assert_eq!(unit_plural_name(UnitType::Artillery), "Artillery");
    assert_eq!(unit_plural_name(UnitType::GoldRefinery), "Gold Refineries");
    assert_eq!(unit_plural_name(UnitType::TrainingHall), "Training Halls");
    assert_eq!(
        unit_plural_name(UnitType::MaterialStorage),
        "Material Storages"
    );
}
