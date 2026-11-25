use super::*;

/// All 8 categories map to distinct buckets in the C++ enum order.
#[test]
fn categories_match_cpp_enum_order() {
    let names = [
            "Custom",
            "Training",
            "Campaign",
            "Hot-seat",
            "Multiplayer",
            "Demo",
            "Single Player Scenario",
            "Multiplayer Scenario",
    ];

    for (index, name) in names.iter().enumerate() {
        let category = mission_category(name).expect("category");
        assert_eq!(category as usize, index, "{name}");
    }

    assert_eq!(mission_category("Unknown"), None);
}

/// The tag array is the C++ `SC_MIS_S..SC_MIS_E` sequence.
#[test]
fn mission_tags_are_in_resource_order() {
    assert_eq!(MISSION_RESOURCE_TAGS.len(), 66);
    assert_eq!(MISSION_RESOURCE_TAGS[0], "SC_CUST1");
    assert_eq!(MISSION_RESOURCE_TAGS[3], "SC_C0001");
    assert_eq!(MISSION_RESOURCE_TAGS[11], "SC_C0009");
    assert_eq!(MISSION_RESOURCE_TAGS[12], "SC_M0001");
    assert_eq!(MISSION_RESOURCE_TAGS[23], "SC_M0012");
    assert_eq!(MISSION_RESOURCE_TAGS[24], "SC_S0001");
    assert_eq!(MISSION_RESOURCE_TAGS[47], "SC_S0024");
    assert_eq!(MISSION_RESOURCE_TAGS[48], "SC_T0001");
    assert_eq!(MISSION_RESOURCE_TAGS[62], "SC_T0015");
    assert_eq!(MISSION_RESOURCE_TAGS[63], "SC_D0001");
    assert_eq!(MISSION_RESOURCE_TAGS[65], "SC_D0003");
}

/// `xor_decode` is an involution with the 32-byte period.
#[test]
fn xor_decode_round_trips_with_period_32() {
    let source: Vec<u8> = (0..100u8).collect();
    let decoded = xor_decode(&source);

    assert_eq!(decoded[0], source[0] ^ GENERIC_TABLE[0]);
    assert_eq!(decoded[32], source[32] ^ GENERIC_TABLE[0]);
    assert_eq!(decoded[33], source[33] ^ GENERIC_TABLE[1]);
    assert_eq!(xor_decode(&decoded), source);
}

/// Text blocks fall back to `en-US` (`Mission_GetText`).
#[test]
fn text_block_falls_back_to_default_language() {
    let block = TextBlock {
        text: BTreeMap::from([
            ("en-US".to_owned(), "Default".to_owned()),
            ("ru-RU".to_owned(), "Русский".to_owned()),
        ]),
    };

    assert_eq!(block.get("ru-RU"), "Русский");
    assert_eq!(block.get("de-DE"), "Default");
    assert_eq!(block.default_text(), "Default");
    assert!(!block.is_empty());
}
