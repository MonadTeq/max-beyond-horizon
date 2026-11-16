//! Бит-в-бит сверка `CargoMenu` с C++-векторами (`tools/cargo_vectors`).
//!
//! Векторы генерирует `max-rust/tools/cargo_vectors/main.cpp` из формул и
//! условий C++-порта (`cargo.cpp`, `cargomenu.cpp`,
//! `purchasetypeselector.cpp`, `abstractupgrademenu.cpp`,
//! `upgradecontrol.cpp`) по данным `assets/PATCHES.RES` (`SC_UNITS`,
//! `SC_ATTRI`) и `default.game_rules.lua`; данные выгружает
//! `max-rust/tools/gen_cargo_data.py`.
//!
//! Перегенерация:
//!
//! ```sh
//! python max-rust/tools/gen_cargo_data.py
//! docker run --rm -v "D:/dev/max-port-debug:/src" \
//!     -w /src/max-rust/tools/cargo_vectors max-port-build:latest \
//!     bash -c 'g++ -O2 -std=c++17 main.cpp -o /tmp/cargo_vectors && \
//!              /tmp/cargo_vectors > /src/max-rust/tools/cargo_vectors/vectors.txt'
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use max_app::mobilize::{list_price, unit_price, upgrade_types, Mobilization, PURCHASE_UNITS};
use max_core::enums::UnitType;
use max_core::team::TeamId;

fn vectors_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../max-rust/tools/cargo_vectors/vectors.txt")
}

fn read_vectors() -> Vec<String> {
    let path = vectors_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("нет {}: {error}", path.display()));

    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

fn unit(index: u32) -> UnitType {
    UnitType::try_from(index as u16).expect("unit index")
}

#[test]
fn purchase_list_matches_cpp_vectors() {
    let lines = read_vectors();
    let purchase: Vec<(u32, u32)> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("purchase "))
        .map(|rest| {
            let mut parts = rest.split_whitespace();
            let _index = parts.next().unwrap();
            let id: u32 = parts.next().unwrap().parse().unwrap();
            let _turns: u32 = parts.next().unwrap().parse().unwrap();
            let price: u32 = parts.next().unwrap().parse().unwrap();
            (id, price)
        })
        .collect();

    assert_eq!(purchase.len(), 15, "CargoMenu::unit_types1");
    assert_eq!(
        purchase.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        PURCHASE_UNITS.iter().map(|unit| *unit as u32).collect::<Vec<_>>()
    );

    let mobilization = Mobilization::new(TeamId::RED, max_core::enums::TeamClan::Random, 150);
    for (id, price) in purchase {
        assert_eq!(
            mobilization.price(unit(id)),
            price,
            "{:?}: цена покупки",
            unit(id)
        );
        assert_eq!(
            unit_price(&mobilization.actual, unit(id)),
            price,
            "{:?}: unit_price",
            unit(id)
        );
    }
}

#[test]
fn list_prices_match_cpp_vectors() {
    let lines = read_vectors();
    let mobilization = Mobilization::new(TeamId::RED, max_core::enums::TeamClan::Random, 150);
    let mut seen = 0;

    for line in &lines {
        let Some(rest) = line.strip_prefix("listprice ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let id: u32 = parts.next().unwrap().parse().unwrap();
        let kind = parts.next().unwrap();
        let price: u32 = parts.next().unwrap().parse().unwrap();

        assert_eq!(
            mobilization.list_price(unit(id)),
            price,
            "{:?} ({kind})",
            unit(id)
        );
        assert_eq!(
            list_price(&mobilization.actual, unit(id)),
            price,
            "{:?} ({kind})",
            unit(id)
        );
        seen += 1;
    }

    assert_eq!(seen, max_core::enums::UNIT_END as usize);
}

#[test]
fn upgrade_lists_match_cpp_vectors() {
    let lines = read_vectors();
    let masks: BTreeMap<u32, [bool; 5]> = [
        (0, [true, true, true, true, false]),
        (1, [true, true, true, true, true]),
        (2, [true, false, false, false, false]),
        (3, [false, true, false, false, false]),
        (4, [false, false, true, false, false]),
        (5, [false, false, false, true, false]),
        (6, [false, false, false, false, true]),
    ]
    .into_iter()
    .collect();

    let mut seen = 0;
    for line in &lines {
        let Some(rest) = line.strip_prefix("upgrade_list ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let mask: u32 = parts.next().unwrap().parse().unwrap();
        let count: usize = parts.next().unwrap().parse().unwrap();
        let ids: Vec<u32> = parts.map(|part| part.parse().unwrap()).collect();

        let filters = masks[&mask];
        let actual: Vec<u32> = upgrade_types(&filters)
            .into_iter()
            .map(|unit| unit as u32)
            .collect();

        assert_eq!(actual.len(), count, "маска {mask}: число типов");
        assert_eq!(actual, ids, "маска {mask}: порядок IsBetterUnit");
        seen += 1;
    }

    assert_eq!(seen, masks.len());
}

#[test]
fn upgrade_rows_and_costs_match_cpp_vectors() {
    let lines = read_vectors();
    let mut seen = 0;

    for line in &lines {
        let Some(rest) = line.strip_prefix("rows ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let id: u32 = parts.next().unwrap().parse().unwrap();
        let count: usize = parts.next().unwrap().parse().unwrap();
        let mut expected = Vec::new();
        for _ in 0..count {
            let attribute: u8 = parts.next().unwrap().parse().unwrap();
            let cost: i32 = parts.next().unwrap().parse().unwrap();
            expected.push((attribute, cost));
        }

        let mut mobilization = Mobilization::new(TeamId::RED, max_core::enums::TeamClan::Random, 150);
        mobilization.info = Some(unit(id));
        mobilization.rebuild_controls();

        let actual_attributes: Vec<u8> = mobilization
            .controls
            .iter()
            .map(|(attribute, _)| *attribute)
            .collect();
        let expected_attributes: Vec<u8> = expected.iter().map(|(attribute, _)| *attribute).collect();

        assert_eq!(
            actual_attributes, expected_attributes,
            "{:?}: состав строк",
            unit(id)
        );

        for (row, (_, cost)) in expected.iter().enumerate() {
            let quote = mobilization.quote(row).expect("строка");
            assert_eq!(quote.cost(), *cost, "{:?} строка {row}: цена", unit(id));
        }

        seen += 1;
    }

    assert_eq!(seen, max_core::enums::UNIT_END as usize);
}
