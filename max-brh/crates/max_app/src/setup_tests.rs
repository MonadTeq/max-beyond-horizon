use super::*;

use max_core::team::TeamId;

#[test]
fn config_table_matches_the_cpp_oracle() {
    assert_eq!(CONFIG_ITEMS.len(), 71);
    assert_eq!(CONFIG_ITEMS[0].text, "Options Menu");
    assert_eq!(CONFIG_ITEMS[13].rect, PixelRect::new(227, 85, 80, 20));
    assert_eq!(CONFIG_ITEMS[29].text, "Turn Based");
    assert_eq!(CONFIG_ITEMS[30].text, "Simultaneous Moves");
    assert_eq!(CONFIG_ITEMS[70].text, "points");
}

#[test]
fn planet_slots_match_the_cpp_controls() {
    assert_eq!(PLANET_SLOTS[0], PixelRect::new(192, 51, 112, 112));
    assert_eq!(PLANET_SLOTS[5], PixelRect::new(469, 189, 112, 112));
    assert_eq!(PLANET_ARROW_LEFT, PixelRect::new(60, 248, 30, 22));
    assert_eq!(PLANET_BUTTONS[3].0, PixelRect::new(514, 438, 109, 40));
    assert_eq!(PLANET_NAMES[0], "Snowcrab");
    assert_eq!(PLANET_NAMES[23], "Bottleneck");
}

#[test]
fn player_buttons_match_the_cpp_controls() {
    assert_eq!(
        player_type_button_rect(0, 0),
        PixelRect::new(175, 67, 55, 71)
    );
    assert_eq!(
        player_type_button_rect(3, 2),
        PixelRect::new(394, 343, 55, 71)
    );
    assert_eq!(player_banner_rect(0), PixelRect::new(495, 61, 93, 89));
    assert_eq!(PLAYER_BUTTONS[2].0, PixelRect::new(514, 438, 109, 40));
}

#[test]
fn radio_indices_map_settings_to_cpp_rows() {
    let mut form = GameSetupForm::default();
    assert_eq!(form.timer_index(), 3); // 180 → the string "180"
    assert_eq!(form.endturn_index(), 3); // 45 → the string "45"
    assert_eq!(form.credit_index(), 3); // 150 → "150"

    form.setup.timer = 0;
    assert_eq!(form.timer_index(), 0, "No Limit");
    form.setup.timer = 90;
    assert_eq!(form.timer_index(), 1, "60 seconds");

    form.setup.victory_type = VictoryType::Score;
    form.setup.victory_limit = 400;
    assert_eq!(form.score_index(), 1);
}

#[test]
fn single_player_human_selection_moves_the_flag() {
    let mut form = GameSetupForm::default();
    form.setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];

    form.select_team_type(1, TeamKind::Player);

    assert_eq!(form.setup.team_types[1], TeamKind::Player);
    assert_eq!(
        form.setup.team_types[0],
        TeamKind::Computer,
            "прежний человек получает тип нового слота"
    );
    assert_eq!(form.setup.human_team(), Some(TeamId(1)));
}

#[test]
fn clan_selection_screen_applies_to_team() {
    use crate::ui::InputEvent;

    let mut form = GameSetupForm::default();
    form.screen = SetupScreen::Players;
    let banner = player_banner_rect(1);
    form.handle_input(
        InputEvent::MouseClick(
            banner.ulx + banner.width / 2,
            banner.uly + banner.height / 2,
        ),
        640,
        480,
    );
    assert_eq!(form.screen, SetupScreen::Clan);
    assert_eq!(form.clan_team, 1);

    let card = CLAN_SLOTS[4];
    form.handle_input(
        InputEvent::MouseClick(card.ulx + card.width / 2, card.uly + card.height / 2),
        640,
        480,
    );
    assert_eq!(
        form.clan_selection,
        max_core::enums::TeamClan::Musashi,
            "карточка 5 — Musashi"
    );

    let done = CLAN_BUTTONS[3].0;
    form.handle_input(
        InputEvent::MouseClick(done.ulx + done.width / 2, done.uly + done.height / 2),
        640,
        480,
    );

    assert_eq!(form.screen, SetupScreen::Players);
    assert_eq!(
        form.setup.team_clans[1],
        max_core::enums::TeamClan::Musashi,
            "клан применён к команде GREEN"
    );
    assert_eq!(
        form.setup.team_clans[0],
        max_core::enums::TeamClan::Random,
            "другие команды не тронуты"
    );
    assert_eq!(
        form.setup.new_game_config().clans[1],
        max_core::enums::TeamClan::Musashi,
            "кланы уходят в NewGameConfig"
    );
}

#[test]
fn clan_cancel_and_random_are_mechanics() {
    use crate::ui::InputEvent;

    let mut form = GameSetupForm::default();
    form.setup.team_clans[0] = max_core::enums::TeamClan::TheChosen;
    form.screen = SetupScreen::Players;
    let banner = player_banner_rect(0);
    form.handle_input(
        InputEvent::MouseClick(
            banner.ulx + banner.width / 2,
            banner.uly + banner.height / 2,
        ),
        640,
        480,
    );
    let card = CLAN_SLOTS[7];
    form.handle_input(
        InputEvent::MouseClick(card.ulx + card.width / 2, card.uly + card.height / 2),
        640,
        480,
    );
    let cancel = CLAN_BUTTONS[1].0;
    form.handle_input(
        InputEvent::MouseClick(
            cancel.ulx + cancel.width / 2,
            cancel.uly + cancel.height / 2,
        ),
        640,
        480,
    );
    assert_eq!(
        form.setup.team_clans[0],
        max_core::enums::TeamClan::TheChosen,
            "Cancel не меняет клан"
    );

    let banner = player_banner_rect(0);
    form.handle_input(
        InputEvent::MouseClick(
            banner.ulx + banner.width / 2,
            banner.uly + banner.height / 2,
        ),
        640,
        480,
    );
    let random = CLAN_BUTTONS[0].0;
    form.handle_input(
        InputEvent::MouseClick(
            random.ulx + random.width / 2,
            random.uly + random.height / 2,
        ),
        640,
        480,
    );
    assert_eq!(
        form.setup.team_clans[0],
        max_core::enums::TeamClan::Random,
            "Random сбрасывает клан"
    );
}

/// `ClanSelectMenu::DrawClanUpgrades` (`clanselectmenu.cpp:165-220`): the
/// archetype line is `Clans::GetDescription` of the `SC_CLANS` entry, and the
/// bonus list walks `UNIT_START..UNIT_END` in resource order with the
/// `ATTRIB_TURNS_TO_BUILD..ATTRIB_COUNT` attributes.
#[test]
fn clan_bonuses_are_the_sc_clans_tradeoffs_in_oracle_order() {
    use max_core::enums::TeamClan;

    assert_eq!(clan_description(TeamClan::TheChosen), "Better Air Units:");
    assert_eq!(clan_description(TeamClan::CrimsonPath), "Better Sea Units:");
    assert_eq!(clan_description(TeamClan::VonGriffin), "Improved Espionage:");
    assert_eq!(
        clan_description(TeamClan::AyersHand),
        "Better Missile Units:"
    );
    assert_eq!(
        clan_description(TeamClan::Musashi),
        "Better Armored Vehicles:"
    );
    assert_eq!(clan_description(TeamClan::SacredEight), "Better Fast Units:");
    assert_eq!(clan_description(TeamClan::SevenKnights), "Better Defense:");
    assert_eq!(
        clan_description(TeamClan::AxisInc),
        "Extra constructors and engineers (depends on budget):"
    );

    // `TeamClan::Random` has no `SC_CLANS` entry: the oracle draws no upgrade
    // text; the port explains the random pick instead.
    assert_eq!(clan_description(TeamClan::Random), MSG_NO_CLAN_CHOSEN);
    assert!(clan_bonus_text(TeamClan::Random).is_empty());

    // The Chosen: `AIRPLT` (0x1B) first, then the aircraft (`FIGHTER` 0x4C …).
    // The attribute order is `Clans::ATTRIB_TURNS_TO_BUILD..ATTRIB_COUNT`
    // (`clanselectmenu.cpp:179`), i.e. turns before range/attack.
    assert_eq!(
        clan_bonus_text(TeamClan::TheChosen),
        "Air Units Plant: -5 turns. \
         Fighter: -1 turns, +1 range. \
         Ground Attack Plane: -1 turns, +4 attack. \
         Air Transport: +4 speed. \
         AWAC: +2 scan."
    );

    // Axis Inc. — production plants only.
    assert_eq!(
        clan_bonus_text(TeamClan::AxisInc),
        "Light Vehicle Plant: -2 turns. Heavy Vehicle Plant: -4 turns."
    );

    // Musashi — heavy armour: `GUNTURRT` (0x22), `TANK` (0x33),
    // `CLNTRANS` (0x41), `BATTLSHP` (0x46); hits → armor → attack (the
    // `Clans::ATTRIB_*` order), speed and range last.
    let musashi = clan_bonus_text(TeamClan::Musashi);
    assert_eq!(
        musashi,
        "Gun Turret: +4 hits, +2 armor, +4 attack. \
         Tank: +4 hits, +2 armor, +4 attack, +1 speed. \
         Armoured Personnel Carrier: +4 hits, +2 armor, +1 speed. \
         Gunboat: +8 hits, +2 armor, +4 attack, +1 range."
    );

    // Every bonus line fits the three-line block of the screen area.
    let lines = crate::menu::wrap_gnw_text(&clan_bonus_text(TeamClan::VonGriffin), CLAN_TEXT_WIDTH);
    assert!(!lines.is_empty());
    assert!(
        lines.iter().all(|line| line.len() > 1),
        "no empty wrap lines: {lines:?}"
    );
}

