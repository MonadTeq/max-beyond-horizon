use super::*;

fn mobilization() -> Mobilization {
    Mobilization::new(TeamId::RED, TeamClan::Random, 150)
}

#[test]
fn purchase_list_matches_cargo_menu_unit_types1() {
    assert_eq!(PURCHASE_UNITS.len(), 15, "CargoMenu::unit_types1");
    assert_eq!(
        PURCHASE_UNITS,
        [
            UnitType::Constructor,
            UnitType::Engineer,
            UnitType::Surveyor,
            UnitType::Scout,
            UnitType::Scanner,
            UnitType::Tank,
            UnitType::AssaultGun,
            UnitType::RocketLauncher,
            UnitType::MissileCrawler,
            UnitType::MineLayer,
            UnitType::RepairUnit,
            UnitType::SupplyTruck,
            UnitType::FuelTruck,
            UnitType::MobileAntiAir,
            UnitType::GoldTruck,
        ]
    );
    assert_eq!(mobilization().types, PURCHASE_UNITS.to_vec());
}

#[test]
fn purchase_prices_are_three_times_turns() {
    // `CargoMenu::BuyUnit`: `Cargo_GetRawConsumptionRate(LANDPLT, 1) * turns`
    // (`SC_ATTRI`: CONSTRCT 8, ENGINEER 6, TANK 4, SURVEYOR 3).
    assert_eq!(unit_cost(UnitType::Constructor, TeamClan::Random), 24);
    assert_eq!(unit_cost(UnitType::Engineer, TeamClan::Random), 18);
    assert_eq!(unit_cost(UnitType::Tank, TeamClan::Random), 12);
    assert_eq!(unit_cost(UnitType::Surveyor, TeamClan::Random), 9);
    assert_eq!(LANDPLT_RAW_CONSUMPTION, 3);

    let mobilization = mobilization();
    for unit in PURCHASE_UNITS {
        assert_eq!(
            mobilization.price(unit),
            LANDPLT_RAW_CONSUMPTION
                * base_unit_values(unit).get_attribute(ATTRIB_TURNS).max(0) as u32,
                "{unit:?}"
        );
    }
}

#[test]
fn list_prices_follow_unit_flags() {
    let mobilization = mobilization();

    // Mobile — LANDPLT rate (3), buildings — CONSTRCT (2), infantry —
    // TRAINHAL (1).
    assert_eq!(mobilization.list_price(UnitType::Tank), 12);
    assert_eq!(mobilization.list_price(UnitType::Infiltrator), 9);
    assert_eq!(mobilization.list_price(UnitType::Infantry), 2);
    assert_eq!(mobilization.list_price(UnitType::MineLayer), 9);
    assert_eq!(
        mobilization.list_price(UnitType::PowerGenerator),
        8,
            "4 хода * 2"
    );
    assert_eq!(mobilization.list_price(UnitType::GoldRefinery), 24);
}

#[test]
fn clan_tradeoffs_change_the_price() {
    let base = unit_cost(UnitType::Tank, TeamClan::Random);
    let musashi = unit_cost(UnitType::Tank, TeamClan::Musashi);

    assert_eq!(
        musashi,
        LANDPLT_RAW_CONSUMPTION
            * team_base_values(TeamClan::Musashi)[UnitType::Tank as usize]
                .get_attribute(ATTRIB_TURNS)
                .max(0) as u32
    );
    assert!(base > 0);
}

#[test]
fn upgrade_list_is_the_full_filtered_set() {
    let mobilization = mobilization();
    let mut upgrade = mobilization.clone();
    upgrade.toggle_mode();

    assert_eq!(upgrade.mode, CargoMode::Upgrade);
    assert!(
        upgrade.types.len() > PURCHASE_UNITS.len(),
            "список модернизации шире списка покупки: {}",
        upgrade.types.len()
    );
    // Built by the team and upgradeable.
    for unit in [
        UnitType::Constructor,
        UnitType::Tank,
        UnitType::PowerGenerator,
        UnitType::Shipyard,
        UnitType::Fighter,
        UnitType::LandMine,
        UnitType::SeaMine,
        UnitType::Infiltrator,
        UnitType::Infantry,
    ] {
        assert!(upgrade.types.contains(&unit), "{unit:?}");
    }
    // Road is not UPGRADABLE, mines — through the deployer (already included), and
    // missiles/explosions/debris are not built.
    for unit in [
        UnitType::Road,
        UnitType::Rocket,
        UnitType::Torpedo,
        UnitType::LargeRubble,
        UnitType::LandExplosion,
        UnitType::DeadWaldo,
    ] {
        assert!(!upgrade.types.contains(&unit), "{unit:?}");
    }
    assert_eq!(
        upgrade.types.len(),
        upgrade_types(&mobilization.filters).len()
    );
}

#[test]
fn upgrade_filters_switch_the_list() {
    let mut mobilization = mobilization();
    mobilization.toggle_mode();
    let all = mobilization.types.len();

    // Ground only.
    mobilization.filters = [true, false, false, false, false];
    mobilization.rebuild_types();
    let ground = mobilization.types.len();
    assert!(ground > 0 && ground < all, "ground {ground} из {all}");

    // Combat only (with an attack).
    mobilization.filters = [true, true, true, true, true];
    mobilization.rebuild_types();
    assert!(mobilization
        .types
        .iter()
        .all(|unit| { base_unit_values(*unit).get_attribute(ATTRIB_ATTACK) > 0 }));
    assert!(mobilization.types.len() < all);

    // All domains off — empty (C++: class_matches false).
    mobilization.filters = [false, false, false, false, true];
    mobilization.rebuild_types();
    assert!(mobilization.types.is_empty());
}

#[test]
fn buying_and_selling_conserves_credits() {
    let mut mobilization = mobilization();

    assert!(mobilization.buy(0), "Constructor за 24 кредита");
    assert_eq!(mobilization.credits, 126);
    assert_eq!(mobilization.bought.len(), 1);
    assert_eq!(mobilization.loadout_selected, 3);
    assert_eq!(mobilization.info, Some(UnitType::Constructor));

    // Cargo is bought at 1 credit per 5 units: Constructor (storage 60)
    // can take 12 blocks.
    let cargo_row = mobilization.loadout_selected;
    assert_eq!(mobilization.cargo(cargo_row), 0);
    mobilization.set_cargo_value(4);
    assert_eq!(mobilization.cargo(cargo_row), 20);
    assert_eq!(mobilization.credits, 122);

    assert!(mobilization.delete_selected());
    // Refund: price (24) + cargo/5 (4).
    assert_eq!(mobilization.credits, 150);
    assert!(mobilization.bought.is_empty());
    assert_eq!(
        mobilization.loadout().len(),
        DEFAULT_MISSION_LOADOUT.len(),
            "бесплатная загрузка остаётся"
    );
}

#[test]
fn purchase_is_rejected_without_credits() {
    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 10);

    assert!(!mobilization.buy(0));
    assert!(mobilization.bought.is_empty());
    assert_eq!(mobilization.credits, 10);
    assert!(mobilization.status.contains("Not enough credits"));
}

#[test]
fn free_loadout_cannot_be_sold_or_unloaded() {
    let mut mobilization = mobilization();

    mobilization.select_loadout(0);
    assert!(!mobilization.delete_selected(), "бесплатная загрузка");
    assert_eq!(mobilization.loadout(), DEFAULT_MISSION_LOADOUT.to_vec());
    assert_eq!(mobilization.cargo_capacity(), 12, "60 / 5");
    mobilization.set_cargo_value(0);
    assert_eq!(
        mobilization.cargo(0),
        40,
            "xfer_take_max не даёт снять бесплатный cargo"
    );
}

#[test]
fn gold_truck_has_no_cargo_slider() {
    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 150);
    let index = PURCHASE_UNITS
        .iter()
        .position(|unit| *unit == UnitType::GoldTruck)
        .unwrap();
    assert!(mobilization.buy(index));
    assert_eq!(
        mobilization.cargo_capacity(),
        0,
            "GOLDTRCK: free_capacity 0"
    );
    mobilization.set_cargo_value(8);
    assert_eq!(mobilization.cargo(mobilization.loadout_selected), 0);
}

#[test]
fn cargo_slider_material_depends_on_type() {
    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 500);

    assert_eq!(
        mobilization.cargo_bar_kind(),
        CargoBarKind::Raw,
            "Constructor"
    );
    mobilization.select_loadout(1);
    assert_eq!(mobilization.cargo_bar_kind(), CargoBarKind::Raw, "Engineer");

    let fuel = PURCHASE_UNITS
        .iter()
        .position(|unit| *unit == UnitType::FuelTruck)
        .unwrap();
    assert!(mobilization.buy(fuel));
    assert_eq!(mobilization.cargo_bar_kind(), CargoBarKind::Fuel);

    let gold = PURCHASE_UNITS
        .iter()
        .position(|unit| *unit == UnitType::GoldTruck)
        .unwrap();
    assert!(mobilization.buy(gold));
    assert_eq!(mobilization.cargo_bar_kind(), CargoBarKind::Gold);
}

#[test]
fn cargo_slider_click_matches_scrollbar_formula() {
    let mut mobilization = mobilization();
    mobilization.select_loadout(0); // Constructor, cargo 40/60 (8/12 blocks)

    // A click at the very bottom — 0 blocks, but the free cargo is not removed.
    assert!(mobilization.click_cargo(CARGO_BAR.ulx + 2, CARGO_BAR.lry() - 1));
    assert_eq!(mobilization.cargo(0), 40);

    // A click at the very top — capacity (12 blocks).
    assert!(mobilization.click_cargo(CARGO_BAR.ulx + 2, CARGO_BAR.uly));
    assert_eq!(mobilization.cargo(0), 60);
    assert_eq!(mobilization.credits, 150 - 4, "4 блока * 1 кредит");
}

#[test]
fn upgrades_change_all_units_of_the_type() {
    let mut mobilization = mobilization();
    mobilization.select_loadout(0); // Constructor

    // Constructor: armor, hits, scan, speed (after skipping storage and
    // consumption).
    let rows: Vec<u8> = mobilization
        .controls
        .iter()
        .map(|(attribute, _)| *attribute)
        .collect();
    assert_eq!(
        rows,
        vec![ATTRIB_ARMOR, ATTRIB_HITS, ATTRIB_SCAN, ATTRIB_SPEED]
    );
    // Two skipped rows before the Common block: storage (Constructor in
    // the `AdjustRowStorage` list) and consumption (`AdjustRowConsumptions`).
    assert_eq!(mobilization.controls[0].1, COST.uly + 2 * CONTROL_ROW);
    assert_eq!(mobilization.controls[1].1, COST.uly + 3 * CONTROL_ROW);

    let before = mobilization.credits;
    let armor_base = mobilization.base[UnitType::Constructor as usize].armor;

    assert!(mobilization.adjust_upgrade(0, true));
    assert!(mobilization.credits < before, "апгрейд списывает кредиты");
    assert_eq!(
        mobilization.actual[UnitType::Constructor as usize].armor,
        armor_base + 1,
            "шаг 1 при базе < 10"
    );

    // Refund — the inverse change.
    assert!(mobilization.adjust_upgrade(0, false));
    assert_eq!(mobilization.credits, before);
    assert_eq!(
        mobilization.actual[UnitType::Constructor as usize].armor,
        armor_base
    );

    // Upgrade, then buy: the price is taken from the current turns.
    mobilization.adjust_upgrade(1, true); // hits (+2 at base 24)
    let changed = mobilization.changed_upgrades();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].0, UnitType::Constructor);
    assert_eq!(changed[0].1.hits, armor_base_hits() + 2);
}

fn armor_base_hits() -> u16 {
    base_unit_values(UnitType::Constructor).hits
}

#[test]
fn upgrade_cost_is_deterministic() {
    let run = || {
        let mut mobilization = mobilization();
        mobilization.select_loadout(0);
        let mut log = Vec::new();
        for row in 0..mobilization.controls.len() {
            for _ in 0..3 {
                let credits = mobilization.credits;
                mobilization.adjust_upgrade(row, true);
                log.push((row, credits, mobilization.credits));
            }
        }
        (log, mobilization.changed_upgrades())
    };

    assert_eq!(run(), run());
}

#[test]
fn upgrades_raise_purchase_prices_of_the_type() {
    let mut mobilization = mobilization();
    // The "turns" upgrade is unavailable in CargoMenu, but the buy price depends on
    // the current turns: check it via a direct change of actual (as the
    // turns attribute upgrade does in other menus).
    assert_eq!(mobilization.price(UnitType::Tank), 12);
    mobilization.actual[UnitType::Tank as usize].turns = 8;
    assert_eq!(mobilization.price(UnitType::Tank), 24, "3 * turns");
}

#[test]
fn zero_budget_skips_the_phase() {
    assert!(!Mobilization::needed(0));
    assert!(Mobilization::needed(150));
}

#[test]
fn long_names_are_shortened_with_dots() {
    let font = GnwFontId::Font5.font();

    assert_eq!(fit_label("Tank", 100, font), "Tank");
    let short = fit_label("Rocket Launcher", 80, font);

    assert!(short.ends_with(".."), "{short}");
    assert!(font.text_width(&short) <= 80, "{short}");

    let mobilization = mobilization();
    assert_eq!(
        mobilization.selected_cost(),
        unit_cost(UnitType::Constructor, TeamClan::Random)
    );
}

#[test]
fn stats_rows_follow_the_cpp_blocks() {
    let mut mobilization = mobilization();
    mobilization.select_loadout(0); // Constructor

    let rows = mobilization.stats_rows();
    let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();
    assert_eq!(
        labels,
        vec!["Cargo", "Uses", "Armor", "Hits", "Scan", "Speed", "Cost"],
            "UnitStats_DrawStats: cargo, usage, common, speed, cost"
    );
    // The step is the same as the upgrades column (`UnitStats_DrawText` — 19 px).
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.y, STATS.uly + STAT_ROW * index as i32);
    }

    // `Cargo`: the number — the current capacity, the pips — `storage / 10`.
    assert_eq!(rows[0].value, Some(60));
    assert_eq!(rows[0].icons, vec![StatIcon::same("I_RAW", 6, 6)]);
    // `Uses`: no number (`resource_count == 0`), raw consumption 2 — two pips.
    assert_eq!(rows[1].value, None);
    assert_eq!(rows[1].icons, vec![StatIcon::same("I_RAW", 2, 2)]);
    // `Armor`/`Hits`/`Scan`/`Speed` — 6/24/3/6.
    assert_eq!(rows[2].value, Some(6));
    assert_eq!(rows[3].value, Some(24));
    assert_eq!(rows[3].icons[0], StatIcon::same("I_HITS", 24, 24));
    assert_eq!(rows[4].value, Some(3));
    assert_eq!(rows[5].value, Some(6));
    // `Cost`: `Cargo_GetRawConsumptionRate(LANDPLT,1) * turns = 3 * 8 = 24`,
    // icons — `I_GOLD`/`I_GOLDE` (`interface_icon_full/empty`), without a line.
    assert_eq!(rows[6].value, Some(24));
    assert_eq!(
        rows[6].icons,
        vec![StatIcon {
            resource: "I_GOLD",
            empty_resource: "I_GOLDE",
            current: 24,
            original: 24,
        }]
    );
    assert!(!rows[6].separator, "Cost — единственная строка без линии");
    assert!(rows[..6].iter().all(|row| row.separator));

    assert!(mobilization.stats_icon_count() >= 6 + 2 + 6 + 24 + 3 + 6 + 24);
}

#[test]
fn military_rows_follow_min_rounds_ammo_and_base_values() {
    let mut mobilization = mobilization();
    mobilization.info = Some(UnitType::Tank);

    let rows = mobilization.stats_rows();
    let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();
    assert_eq!(
        labels,
        vec!["Attack", "Shots", "Range", "Ammo", "Armor", "Hits", "Scan", "Speed", "Cost"]
    );
    assert_eq!(rows[0].value, Some(16));
    assert_eq!(rows[0].icons[0], StatIcon::same("I_HRDATK", 16, 16));
    // `std::min(ROUNDS, AMMO)` = min(2, 14).
    assert_eq!(rows[1].value, Some(2));
    assert_eq!(rows[2].value, Some(4));
    assert_eq!(rows[3].value, Some(14));
    // Constructo has no military rows (ATTACK = 0).
    mobilization.info = Some(UnitType::Constructor);
    assert!(mobilization
        .stats_rows()
        .iter()
        .all(|row| !matches!(row.label, "Attack" | "Shots" | "Range" | "Ammo")));
}

#[test]
fn usage_and_storage_rows_follow_unit_stats_switches() {
    let mut mobilization = mobilization();

    // `POWGEN`: production -1 (`Prod.` + `I_POWER`), `Cost` 2 * 4.
    mobilization.info = Some(UnitType::PowerGenerator);
    let rows = mobilization.stats_rows();
    assert_eq!(rows[0].label, "Prod.");
    assert_eq!(rows[0].value, Some(1));
    assert_eq!(rows[0].icons, vec![StatIcon::same("I_POWER", 1, 1)]);
    assert_eq!(rows.last().unwrap().label, "Cost");
    assert_eq!(rows.last().unwrap().value, Some(8), "CONSTRCT 2 * 4 turns");

    // `HABITAT`: life support -3 (`Prod.` + `I_LIFE`); no production —
    // `Cargo_GetPowerConsumptionRate` for HABITAT = 0.
    mobilization.info = Some(UnitType::Habitat);
    let rows = mobilization.stats_rows();
    assert_eq!(rows[0].label, "Prod.");
    assert_eq!(rows[0].icons, vec![StatIcon::same("I_LIFE", 3, 3)]);

    // `MINELAYR`: storage 16, but `UnitStats_DrawCargo` does not draw it
    // (default switch) — there is no `Cargo` row; LIGHTPLT rate 3 * 3.
    mobilization.info = Some(UnitType::MineLayer);
    let rows = mobilization.stats_rows();
    assert!(rows.iter().all(|row| row.label != "Cargo"));
    assert_eq!(rows.last().unwrap().value, Some(9));
    assert_eq!(rows.last().unwrap().icons[0].resource, "I_GOLD");

    // `LANDMINE`: `Builder_GetBuilderType` = INVALID_ID → rate 0,
    // the level is clamped to 1 (`UnitStats_DrawCost`).
    mobilization.info = Some(UnitType::LandMine);
    let rows = mobilization.stats_rows();
    assert_eq!(rows.last().unwrap().value, Some(1));
    assert_eq!(rows.last().unwrap().icons[0].current, 1);
    assert_eq!(mobilization.build_rate(UnitType::LandMine), 0);
}

/// `UnitStats_DrawStats`: `Speed` — only if `ATTRIB_SPEED > 0`
/// (`if (unit_values2->GetAttribute(ATTRIB_SPEED))`), so buildings
/// (mine/factory/turret) do not show the `Speed` row in `CargoMenu`.
#[test]
fn cargo_menu_buildings_never_show_speed() {
    let mut mobilization = mobilization();

    for unit_type in [
        UnitType::MiningStation,
        UnitType::TrainingHall,
        UnitType::GunTurret,
    ] {
        mobilization.info = Some(unit_type);
        let rows = mobilization.stats_rows();

        assert!(
            rows.iter().all(|row| row.label != "Speed"),
                "{unit_type:?}: {:?}",
            rows.iter().map(|row| row.label).collect::<Vec<_>>()
        );
    }

    mobilization.info = Some(UnitType::Tank);
    assert!(
        mobilization
            .stats_rows()
            .iter()
            .any(|row| row.label == "Speed"),
            "у танка Speed должен остаться"
    );
}

#[test]
fn upgraded_values_show_delta_pips() {
    let mut mobilization = mobilization();
    mobilization.info = Some(UnitType::Tank);
    let base_rows = mobilization.stats_rows();

    mobilization.actual[UnitType::Tank as usize].attack = 20;
    let rows = mobilization.stats_rows();
    let attack = &rows[0];

    // `UnitStats_DrawIcons(current = base, original = actual)`: 4 empty
    // delta pips (`original - current`) after the red line.
    assert_eq!(attack.value, Some(20), "число — текущее значение");
    assert_eq!(attack.icons[0].current, 16);
    assert_eq!(attack.icons[0].original, 20);
    assert_eq!(attack.icons[0].count(), 20);
    assert_eq!(attack.icon_count(), 20);
    assert_eq!(base_rows[0].icon_count(), 16);

    // Decreasing turns: the current price is below the base — `original > current`,
    // so 3 "empty" pips show the base cost.
    mobilization.actual[UnitType::Tank as usize].turns = 3;
    let rows = mobilization.stats_rows();
    let cost = rows.last().unwrap();
    assert_eq!(cost.value, Some(9), "LANDPLT 3 * 3");
    assert_eq!(cost.icons[0].current, 9);
    assert_eq!(cost.icons[0].original, 12);
    assert_eq!(cost.icons[0].count(), 12, "9 заполненных + 3 пустых");

    // Increasing turns: `original < current` — no empty pips.
    mobilization.actual[UnitType::Tank as usize].turns = 5;
    let rows = mobilization.stats_rows();
    let cost = rows.last().unwrap();
    assert_eq!(cost.value, Some(15));
    assert_eq!(cost.icons[0].current, 15);
    assert_eq!(cost.icons[0].original, 12);
    assert_eq!(cost.icons[0].count(), 15);
}

#[test]
fn description_and_portrait_come_from_the_generated_resource() {
    assert!(cargo_unit_data::is_loaded());

    let mobilization = mobilization();
    assert_eq!(mobilization.info, Some(UnitType::Constructor));
    assert!(mobilization
        .description()
        .starts_with("Large construction vehicle"));
    assert!(mobilization.has_portrait());

    // Every type of the buy and upgrade lists is described and has a
    // portrait — for every type openable in `CargoMenu`.
    let mut all = PURCHASE_UNITS.to_vec();
    all.extend(upgrade_types(&[true, true, true, true, false]));
    all.sort_by_key(|unit| *unit as u16);
    all.dedup();

    assert!(all.len() >= 60, "типов в `CargoMenu`: {}", all.len());

    for unit in all {
        assert!(
            !cargo_unit_data::unit_description(unit).is_empty(),
                "{unit:?}: описание в `language.json`"
        );
        assert!(
            cargo_unit_data::unit_portrait(unit).is_some(),
                "{unit:?}: `P_*` портрет"
        );
    }
}

#[test]
fn only_unreachable_types_have_no_description() {
    let mut reachable = PURCHASE_UNITS.to_vec();
    reachable.extend(upgrade_types(&[true, true, true, true, false]));

    for index in 0..UNIT_END {
        let unit = UnitType::try_from(index).expect("UnitType 0..92");

        if cargo_unit_data::unit_description(unit).is_empty() {
            // 24 service types (debris, explosions, projectiles, HARVSTER,
            // WALDO) — without a description in the original too; in `CargoMenu` they cannot
            // be opened, so `assert cargo-description` always sees text.
            assert!(
                !reachable.contains(&unit),
                    "{unit:?}: без описания, но достижим в CargoMenu"
            );
        } else {
            assert!(
                cargo_unit_data::unit_portrait(unit).is_some(),
                    "{unit:?}: описание есть, портрета нет"
            );
        }
    }
}

#[test]
fn stat_icon_resource_names_match_unitstats_resources() {
    // `UnitStats_DrawStats`: exactly the `I_*` that C++ reads.
    for name in [
            "I_HRDATK", "I_SHOTS", "I_RANGE", "I_AMMO", "I_RAW", "I_FUEL", "I_GOLD", "I_POWER",
            "I_LIFE", "I_ARMOR", "I_HITS", "I_SCAN", "I_SPEED",
    ] {
        assert!(STAT_ICONS.contains(&name), "{name}");
    }
    // `CargoMenu` passes `interface_icon_full/empty = I_GOLD/I_GOLDE`,
    // `BuildMenu` — `I_RAW`/`I_RAWE` (`buildmenu.cpp:536`).
    assert!(STAT_ICONS.contains(&"I_GOLDE"));
    assert!(STAT_ICONS.contains(&"I_RAWE"));

    let mut mobilization = mobilization();
    for unit in upgrade_types(&[true, true, true, true, false]) {
        mobilization.info = Some(unit);
        let rows = mobilization.stats_rows();
        assert!(!rows.is_empty(), "{unit:?}: строки статистики");
        for row in &rows {
            for icon in &row.icons {
                assert!(STAT_ICONS.contains(&icon.resource), "{}", icon.resource);
                assert!(
                    STAT_ICONS.contains(&icon.empty_resource),
                        "{}",
                    icon.empty_resource
                );
            }
        }
    }
}

#[test]
fn clicks_follow_cargo_menu_buttons_and_double_click() {
    let mut mobilization = mobilization();

    // A click on the already selected list row — `Select(1)` → `BuyUnit`.
    assert_eq!(
        mobilization.handle_click(AVAILABLE_AREA.ulx + 4, AVAILABLE_AREA.uly + 4),
        CargoAction::None
    );
    assert_eq!(mobilization.bought.len(), 1, "двойной клик покупает");
    assert_eq!(mobilization.credits, 126);

    // A click on a purchased unit row: after `BuyUnit` it is already selected
    // (`PushBack` → `Select`), so a repeated click — a sale.
    let bought_row = mobilization.free.len();
    let y = PURCHASED_AREA.uly + bought_row as i32 * LIST_ROW + 4;
    mobilization.handle_click(PURCHASED_AREA.ulx + 4, y);
    assert!(mobilization.bought.is_empty(), "повторный клик продаёт");
    assert_eq!(mobilization.credits, 150);

    // Selecting the free loadout rows — the information follows the list.
    mobilization.handle_click(PURCHASED_AREA.ulx + 4, PURCHASED_AREA.uly + 4);
    assert_eq!(mobilization.info, Some(UnitType::Constructor));
    mobilization.handle_click(PURCHASED_AREA.ulx + 4, PURCHASED_AREA.uly + LIST_ROW + 4);
    assert_eq!(mobilization.info, Some(UnitType::Engineer));

    // Done/Cancel buttons.
    assert_eq!(
        mobilization.handle_click(DONE_BUTTON.ulx + 2, DONE_BUTTON.uly + 2),
        CargoAction::Done
    );
    assert_eq!(
        mobilization.handle_click(CANCEL_BUTTON.ulx + 2, CANCEL_BUTTON.uly + 2),
        CargoAction::Cancel
    );

    // Mode toggle and upgrade arrow.
    assert_eq!(
        mobilization.handle_click(TOGGLE_BUTTON.ulx + 2, TOGGLE_BUTTON.uly + 2),
        CargoAction::None
    );
    assert_eq!(mobilization.mode, CargoMode::Upgrade);
    let (_, right) = mobilization.control_arrows(0).expect("строка");
    let before = mobilization.credits;
    mobilization.handle_click(right.ulx + 2, right.uly + 2);
    assert!(mobilization.credits < before, "правая стрелка — Increase");

    // Keys.
    assert_eq!(mobilization.handle_key(Key::Enter), CargoAction::Done);
    assert_eq!(mobilization.handle_key(Key::Escape), CargoAction::Cancel);
    mobilization.handle_key(Key::Char('u'));
    assert_eq!(mobilization.mode, CargoMode::Buy);
    mobilization.handle_key(Key::Char('b'));
    assert_eq!(mobilization.bought.len(), 1, "b — BuyUnit");
}
