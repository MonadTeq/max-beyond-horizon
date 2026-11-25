use super::*;

#[test]
fn roll_respects_weights_and_rng_order() {
    let mut table = WeightTable::new(0);
    table.add(UnitType::Tank, 1);
    table.add(UnitType::Scout, 3);
    // Total weight 4: rng.generate_i32(4) + 1 ∈ {1..4}.
    let mut rng = Randomizer::new(0);
    let first = table.roll_unit_type(&mut rng);
    let mut rng2 = Randomizer::new(0);
    let second = table.roll_unit_type(&mut rng2);
    assert_eq!(first, second);
    assert!(matches!(first, Some(UnitType::Tank) | Some(UnitType::Scout)));
}

#[test]
fn empty_or_zero_weight_table_rolls_nothing() {
    let mut table = WeightTable::new(0);
    let mut rng = Randomizer::new(1);
    assert_eq!(table.roll_unit_type(&mut rng), None);
    table.add(UnitType::Tank, 0);
    assert_eq!(table.roll_unit_type(&mut rng), None);
    table.compact();
    assert!(table.is_empty());
}

#[test]
fn disable_swaps_with_last_and_compact_keeps_order() {
    let mut table = WeightTable::new(0);
    table.add(UnitType::Scout, 1);
    table.add(UnitType::Tank, 2);
    table.add(UnitType::AssaultGun, 3);
    table.disable(0);
    assert_eq!(table.len(), 2);
    assert_eq!(table.entries()[0].unit_type, Some(UnitType::AssaultGun));
    assert_eq!(table.entries()[1].unit_type, Some(UnitType::Tank));

    let mut table = WeightTable::new(0);
    table.add(UnitType::Scout, 1);
    table.add(UnitType::Tank, 0);
    table.add(UnitType::AssaultGun, 3);
    table.compact();
    assert_eq!(table.len(), 2);
    assert_eq!(table.entries()[0].unit_type, Some(UnitType::Scout));
    assert_eq!(table.entries()[1].unit_type, Some(UnitType::AssaultGun));
}

#[test]
fn merge_adds_weights_by_type() {
    let mut first = WeightTable::new(0);
    first.add(UnitType::Tank, 2);
    let mut second = WeightTable::new(1);
    second.add(UnitType::Tank, 3);
    second.add(UnitType::Scout, 1);
    first.merge(&second);
    assert_eq!(first.get_weight(UnitType::Tank), 5);
    assert_eq!(first.get_weight(UnitType::Scout), 1);
    assert_eq!(first.len(), 2);
}

#[test]
fn rebuild_multiplies_weight() {
    let mut table = WeightTable::new(0);
    table.add(UnitType::Tank, 2);
    table.add(UnitType::Scout, 1);
    table.rebuild(UnitType::Tank, 10);
    assert_eq!(table.get_weight(UnitType::Tank), 20);
    assert_eq!(table.get_weight(UnitType::Scout), 1);
}
