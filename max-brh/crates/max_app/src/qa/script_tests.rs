use super::*;

fn one(source: &str) -> Command {
    let steps = parse_script(source).expect("скрипт разбирается");
    assert_eq!(steps.len(), 1, "ожидалась одна команда: {source}");
    steps.into_iter().next().expect("шаг").command
}

#[test]
fn every_low_level_command_is_parsed() {
    assert_eq!(one("click 10 20"), Command::Click(10, 20));
    assert_eq!(one("rclick 639 479"), Command::RightClick(639, 479));
    assert_eq!(one("key ENTER"), Command::Key(Key::Enter));
    assert_eq!(one("key esc"), Command::Key(Key::Escape));
    assert_eq!(one("key t"), Command::Key(Key::Char('t')));
    assert_eq!(
        one("type \"Hello, world\""),
        Command::Type("Hello, world".to_string())
    );
    assert_eq!(one("wait 40"), Command::Wait(40));
    assert_eq!(one("tick 3"), Command::Tick(3));
    // `anim N` — visual clock only (dt > 0), without simulation ticks.
    assert_eq!(one("anim 150"), Command::Anim(150));
    assert_eq!(one("anim 0"), Command::Anim(0));
    assert!(parse_script("anim -5\n").is_err());
    assert_eq!(one("mem"), Command::Mem);
    assert_eq!(
        one("shot main-menu"),
        Command::Shot("main-menu".to_string())
    );
    assert_eq!(one("save sp.mxs"), Command::Save("sp.mxs".to_string()));
    assert_eq!(
        one("load SAVE1.CAM"),
        Command::Load("SAVE1.CAM".to_string())
    );
    assert_eq!(one("exit"), Command::Exit);
}

#[test]
fn every_assertion_is_parsed() {
    assert_eq!(
        one("assert turn=3"),
        Command::Assert(Assertion::Turn(Cmp::Eq, 3))
    );
    assert_eq!(
        one("assert turn>=3"),
        Command::Assert(Assertion::Turn(Cmp::Ge, 3))
    );
    assert_eq!(
        one("assert turn<=3"),
        Command::Assert(Assertion::Turn(Cmp::Le, 3))
    );
    assert_eq!(
        one("assert units>4"),
        Command::Assert(Assertion::Units(Cmp::Gt, 4))
    );
    assert_eq!(
        one("assert unit-count RocketSmoke>=3"),
        Command::Assert(Assertion::UnitCount("rocketsmoke".to_string(), Cmp::Ge, 3))
    );
    assert_eq!(
        one("assert firing=on"),
        Command::Assert(Assertion::Firing(true))
    );
    assert_eq!(
        one("assert building=off"),
        Command::Assert(Assertion::Building(false))
    );
    assert_eq!(
        one("assert trail=on"),
        Command::Assert(Assertion::Trail(true))
    );
    assert_eq!(one("assert team=1"), Command::Assert(Assertion::Team(1)));
    assert_eq!(one("assert team=RED"), Command::Assert(Assertion::Team(0)));
    assert_eq!(
        one("assert screen=InGame"),
        Command::Assert(Assertion::Screen(ScreenName::InGame))
    );
    assert_eq!(
        one("assert phase=PlayerTurn"),
        Command::Assert(Assertion::Phase(PhaseName::PlayerTurn))
    );
    assert_eq!(
        one("assert notice=GREEN"),
        Command::Assert(Assertion::Notice(Some(1)))
    );
    assert_eq!(
        one("assert notice=none"),
        Command::Assert(Assertion::Notice(None))
    );
    assert_eq!(one("assert victory"), Command::Assert(Assertion::Victory));
    assert_eq!(one("assert defeat"), Command::Assert(Assertion::Defeat));
    assert_eq!(one("assert selected"), Command::Assert(Assertion::Selected));
    assert_eq!(
        one("assert digest=0xBEEF"),
        Command::Assert(Assertion::Digest(0xBEEF))
    );
    assert_eq!(one("assert angle=2"), Command::Assert(Assertion::Angle(2)));
    assert!(parse_script("assert angle=8\n").is_err(), "угол 0..7");
    assert!(parse_script("assert angle=восток\n").is_err());
    assert_eq!(
        one("assert endturn=pressed"),
        Command::Assert(Assertion::EndTurnPressed(true))
    );
    assert_eq!(
        one("assert endturn=normal"),
        Command::Assert(Assertion::EndTurnPressed(false))
    );
    assert_eq!(
        one("assert site=large"),
        Command::Assert(Assertion::ConstructionSite(Some(true)))
    );
    assert_eq!(
        one("assert site=small"),
        Command::Assert(Assertion::ConstructionSite(Some(false)))
    );
    assert_eq!(
        one("assert site=none"),
        Command::Assert(Assertion::ConstructionSite(None))
    );
    assert_eq!(
        one("assert site=drawn"),
        Command::Assert(Assertion::ConstructionSiteDrawn)
    );
    assert_eq!(
        one("assert site=visible"),
        Command::Assert(Assertion::ConstructionSiteDrawn)
    );
    assert!(parse_script("assert site=huge\n").is_err());
    // A4–A9: frame checks of the SURVEY overlay and the selector.
    assert_eq!(
        one("assert resource-icons>=1"),
        Command::Assert(Assertion::ResourceIcons(Cmp::Ge, 1))
    );
    assert_eq!(
        one("assert resource-icons=0"),
        Command::Assert(Assertion::ResourceIcons(Cmp::Eq, 0))
    );
    assert_eq!(
        one("assert survey-overlay=yes"),
        Command::Assert(Assertion::SurveyOverlay(true))
    );
    assert_eq!(
        one("assert survey-overlay=no"),
        Command::Assert(Assertion::SurveyOverlay(false))
    );
    assert_eq!(
        one("assert unit-selector=yes"),
        Command::Assert(Assertion::UnitSelector(true))
    );
    assert_eq!(
        one("assert unit-selector=no"),
        Command::Assert(Assertion::UnitSelector(false))
    );
    assert!(parse_script("assert survey-overlay=maybe\n").is_err());
    // P9: the Preferences/Setup dialog.
    assert_eq!(
        one("assert prefs=open"),
        Command::Assert(Assertion::PrefsOpen(true))
    );
    assert_eq!(
        one("assert prefs=closed"),
        Command::Assert(Assertion::PrefsOpen(false))
    );
    assert_eq!(
        one("assert prefs-value effects=0"),
        Command::Assert(Assertion::PrefsValue(
            crate::preferences::PrefKey::Effects,
            Cmp::Eq,
            0
        ))
    );
    assert_eq!(
        one("assert prefs-value quick_scroll=35"),
        Command::Assert(Assertion::PrefsValue(
            crate::preferences::PrefKey::QuickScroll,
            Cmp::Eq,
            35
        ))
    );
    assert_eq!(
        one("assert prefs-effects=off"),
        Command::Assert(Assertion::PrefsEffects(false))
    );
    assert_eq!(
        one("assert prefs-sound=on"),
        Command::Assert(Assertion::PrefsSound(true))
    );
    assert_eq!(
        one("assert prefs-audio=off"),
        Command::Assert(Assertion::PrefsAudio(false))
    );
    assert!(parse_script("assert prefs-value bogus=1\n").is_err());
    assert!(parse_script("assert prefs-value effects\n").is_err());
    assert!(parse_script("assert prefs-effects=maybe\n").is_err());
}

#[test]
fn high_level_commands_are_parsed() {
    assert_eq!(one("menu new_game"), Command::Menu(MenuTarget::NewGame));
    assert_eq!(
        one("menu multiplayer"),
        Command::Menu(MenuTarget::Multiplayer)
    );
    assert_eq!(one("menu hot_seat"), Command::Menu(MenuTarget::HotSeat));
    assert_eq!(one("menu join"), Command::Menu(MenuTarget::Join));
    assert_eq!(one("menu credits"), Command::Menu(MenuTarget::Credits));
    assert_eq!(one("menu intro"), Command::Menu(MenuTarget::Introduction));
    assert_eq!(one("menu setup"), Command::Menu(MenuTarget::Setup));
    assert_eq!(one("setup done"), Command::Setup(SetupAction::Done));
    assert_eq!(one("setup cancel"), Command::Setup(SetupAction::Cancel));
    assert_eq!(one("setup random"), Command::Setup(SetupAction::Random));
    assert_eq!(
        one("players 2 none"),
        Command::Players {
            slot: 2,
            kind: TeamKind::None
        }
    );
    assert_eq!(
        one("players 1 computer"),
        Command::Players {
            slot: 1,
            kind: TeamKind::Computer
        }
    );
    assert_eq!(
        one("players 0 human"),
        Command::Players {
            slot: 0,
            kind: TeamKind::Player
        }
    );
    assert_eq!(one("select 404 242"), Command::Select(404, 242));
    assert_eq!(one("select unit 2"), Command::SelectUnit(2));
    assert_eq!(one("move 468 306"), Command::Move(468, 306));
    assert_eq!(one("move grid 24 25"), Command::MoveGrid(24, 25));
    assert_eq!(one("move near"), Command::MoveNear);
    assert_eq!(one("end_turn"), Command::EndTurn);
}

#[test]
fn upgrade_and_research_commands_parse() {
    let steps = parse_script(
            "upgrade open\n\
             upgrade select 3\n\
             upgrade row 0 up\n\
             upgrade row 1 down\n\
             upgrade done\n\
             upgrade cancel\n\
             assert upgrade=open\n\
             assert gold<150\n\
             assert research attack>=1\n",
    )
    .expect("P6-команды разбираются");

    assert_eq!(steps[0].command, Command::Upgrade(UpgradeCommand::Open));
    assert_eq!(
        steps[1].command,
        Command::Upgrade(UpgradeCommand::Select(3))
    );
    assert_eq!(
        steps[2].command,
        Command::Upgrade(UpgradeCommand::Adjust {
            row: 0,
            increase: true
        })
    );
    assert_eq!(
        steps[3].command,
        Command::Upgrade(UpgradeCommand::Adjust {
            row: 1,
            increase: false
        })
    );
    assert_eq!(
        steps[6].command,
        Command::Assert(Assertion::UpgradeOpen(true))
    );
    assert_eq!(
        steps[7].command,
        Command::Assert(Assertion::Gold(Cmp::Lt, 150))
    );
    assert_eq!(
        steps[8].command,
        Command::Assert(Assertion::ResearchLevel(
            max_core::enums::ResearchTopic::Attack,
            Cmp::Ge,
            1
        ))
    );
}

#[test]
fn cargo_menu_commands_and_assertions_parse() {
    let steps = parse_script(
            "mobilize mode upgrade\n\
             mobilize mode buy\n\
             mobilize filter ground\n\
             mobilize filter combat\n\
             mobilize select 3\n\
             mobilize loadout 0\n\
             mobilize row 0 up\n\
             mobilize row 1 down\n\
             mobilize cargo 4\n\
             mobilize buy 14\n\
             mobilize delete\n\
             assert cargo-mode=upgrade\n\
             assert cargo-types=15\n\
             assert cargo-credits>=100\n\
             assert cargo-description>=100\n\
             assert cargo-stat-icons>=10\n\
             assert cargo-portrait=yes\n",
    )
    .expect("P7-команды разбираются");

    assert_eq!(
        steps[0].command,
        Command::Mobilize(MobilizeAction::Mode(false))
    );
    assert_eq!(
        steps[1].command,
        Command::Mobilize(MobilizeAction::Mode(true))
    );
    assert_eq!(
        steps[2].command,
        Command::Mobilize(MobilizeAction::Filter(FilterName::Ground))
    );
    assert_eq!(
        steps[3].command,
        Command::Mobilize(MobilizeAction::Filter(FilterName::Combat))
    );
    assert_eq!(
        steps[4].command,
        Command::Mobilize(MobilizeAction::Select(3))
    );
    assert_eq!(
        steps[5].command,
        Command::Mobilize(MobilizeAction::Loadout(0))
    );
    assert_eq!(
        steps[6].command,
        Command::Mobilize(MobilizeAction::Adjust {
            row: 0,
            increase: true
        })
    );
    assert_eq!(
        steps[7].command,
        Command::Mobilize(MobilizeAction::Adjust {
            row: 1,
            increase: false
        })
    );
    assert_eq!(
        steps[8].command,
        Command::Mobilize(MobilizeAction::Cargo(4))
    );
    assert_eq!(
        steps[9].command,
        Command::Mobilize(MobilizeAction::BuyIndex(14))
    );
    assert_eq!(steps[10].command, Command::Mobilize(MobilizeAction::Delete));
    assert_eq!(
        steps[11].command,
        Command::Assert(Assertion::CargoMode(false))
    );
    assert_eq!(
        steps[12].command,
        Command::Assert(Assertion::CargoTypes(Cmp::Eq, 15))
    );
    assert_eq!(
        steps[13].command,
        Command::Assert(Assertion::CargoCredits(Cmp::Ge, 100))
    );
    assert_eq!(
        steps[14].command,
        Command::Assert(Assertion::CargoDescription(Cmp::Ge, 100))
    );
    assert_eq!(
        steps[15].command,
        Command::Assert(Assertion::CargoStatIcons(Cmp::Ge, 10))
    );
    assert_eq!(
        steps[16].command,
        Command::Assert(Assertion::CargoPortrait(true))
    );

    assert!(parse_script("mobilize filter water\n").is_err());
    assert!(parse_script("mobilize mode neither\n").is_err());
    assert!(parse_script("assert cargo-mode=neither\n").is_err());
    assert!(parse_script("assert cargo-portrait=maybe\n").is_err());
}

#[test]
fn comments_and_empty_lines_are_skipped() {
    let script = "\
# заголовок
click 1 2  # клик по кнопке
\n
type \"a#b\"  # решётка в кавычках — часть текста
";

    let steps = parse_script(script).expect("разбирается");

    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].line, 2);
    assert_eq!(steps[0].command, Command::Click(1, 2));
    assert_eq!(steps[1].command, Command::Type("a#b".to_string()));
}

#[test]
fn step_numbers_point_at_the_source_line() {
    let script = "# c\n\nclick 1 2\n\nshot a\n";

    let steps = parse_script(script).expect("разбирается");

    assert_eq!(steps[0].line, 3);
    assert_eq!(steps[1].line, 5);
}

#[test]
fn errors_name_the_line() {
    let error = parse_script("click 1 2\nbogus 3\n").expect_err("опечатка ловится");

    assert!(error.contains("строка 2"), "{error}");
    assert!(error.contains("bogus"), "{error}");
}

#[test]
fn out_of_frame_and_bad_arguments_are_rejected() {
    for source in [
            "click 640 10",
            "click -1 10",
            "click 10 480",
            "click 10",
            "click 10 20 30",
            "key",
            "key F13",
            "type hello world",
            "type \"unterminated",
            "wait",
            "mem 1",
            "shot a/b",
            "shot ..",
            "save ../x.mxs",
            "load dir/SAVE.CAM",
            "assert",
            "assert turn",
            "assert turn=abc",
            "assert units>=-2",
            "assert team=9",
            "assert screen=Nope",
            "assert phase=Nope",
            "assert digest=zzz",
            "assert nope=1",
            "menu nope",
            "setup nope",
            "players 9 human",
            "players 0 alien",
            "players 0",
            "select 900 10",
            "select unit",
            "move grid 112 0",
            "move grid 1",
            "move near extra",
            "end_turn now",
            "exit now",
    ] {
        assert!(
            parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
        );
    }
}

#[test]
fn research_menu_and_help_commands_parse() {
    assert_eq!(
        one("research open"),
        Command::Research(ResearchCommand::Open)
    );
    assert_eq!(
        one("research row 0 down"),
        Command::Research(ResearchCommand::Row {
            row: 0,
            increase: false
        })
    );
    assert_eq!(
        one("research row 1 up"),
        Command::Research(ResearchCommand::Row {
            row: 1,
            increase: true
        })
    );
    assert_eq!(
        one("research select 7"),
        Command::Research(ResearchCommand::Select(7))
    );
    assert_eq!(
        one("research done"),
        Command::Research(ResearchCommand::Done)
    );
    assert_eq!(
        one("research cancel"),
        Command::Research(ResearchCommand::Cancel)
    );
    assert_eq!(
        one("assert research-menu=open"),
        Command::Assert(Assertion::ResearchMenuOpen(true))
    );
    assert_eq!(
        one("assert research-alloc attack=0"),
        Command::Assert(Assertion::ResearchAlloc(
            max_core::enums::ResearchTopic::Attack,
            Cmp::Eq,
            0
        ))
    );

    assert_eq!(
        one("help cargo"),
        Command::Help(HelpCommand::Open(crate::helpmenu::HelpSection::Cargo))
    );
    assert_eq!(
        one("help newgame"),
        Command::Help(HelpCommand::Open(crate::helpmenu::HelpSection::NewGame))
    );
    assert_eq!(one("help page down"), Command::Help(HelpCommand::PageDown));
    assert_eq!(one("help page up"), Command::Help(HelpCommand::PageUp));
    assert_eq!(one("help keys"), Command::Help(HelpCommand::Keys));
    assert_eq!(one("help done"), Command::Help(HelpCommand::Done));
    assert_eq!(one("help close"), Command::Help(HelpCommand::Close));
    assert_eq!(
        one("assert help=open"),
        Command::Assert(Assertion::HelpOpen(true))
    );
    assert_eq!(
        one("assert help-section=CARGO_SETUP"),
        Command::Assert(Assertion::HelpSection(crate::helpmenu::HelpSection::Cargo))
    );
    assert_eq!(
        one("assert help-row>=16"),
        Command::Assert(Assertion::HelpRow(Cmp::Ge, 16))
    );

    // Typos fail with a line number instead of "silently".
    for source in [
            "research nope",
            "research row 0 sideways",
            "help nope",
            "help page sideways",
            "assert help-section=nope",
            "assert research-alloc nope=1",
    ] {
        assert!(
            parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
        );
    }
}

#[test]
fn allocate_commands_parse() {
    use max_core::alloc::MiningKind;

    assert_eq!(
        one("allocate open"),
        Command::Allocate(AllocateCommand::Open)
    );
    assert_eq!(
        one("allocate select gold"),
        Command::Allocate(AllocateCommand::Select(MiningKind::Gold))
    );
    // `set N` — the selected resource; `set raw N` — explicit.
    assert_eq!(
        one("allocate set 4"),
        Command::Allocate(AllocateCommand::Set {
            kind: None,
            level: 4
        })
    );
    assert_eq!(
        one("allocate set fuel 2"),
        Command::Allocate(AllocateCommand::Set {
            kind: Some(MiningKind::Fuel),
            level: 2
        })
    );
    // The level is clipped by the mine capacity (16 units).
    assert_eq!(
        one("allocate set raw 99"),
        Command::Allocate(AllocateCommand::Set {
            kind: Some(MiningKind::Raw),
            level: 16
        })
    );
    assert_eq!(
        one("allocate row raw up"),
        Command::Allocate(AllocateCommand::Adjust {
            kind: Some(MiningKind::Raw),
            increase: true
        })
    );
    assert_eq!(
        one("allocate row down"),
        Command::Allocate(AllocateCommand::Adjust {
            kind: None,
            increase: false
        })
    );
    assert_eq!(
        one("allocate done"),
        Command::Allocate(AllocateCommand::Done)
    );
    assert_eq!(
        one("allocate cancel"),
        Command::Allocate(AllocateCommand::Cancel)
    );

    assert_eq!(
        one("assert alloc=open"),
        Command::Assert(Assertion::AllocOpen(true))
    );
    assert_eq!(
        one("assert alloc-raw=4"),
        Command::Assert(Assertion::AllocRaw(Cmp::Eq, 4))
    );
    assert_eq!(
        one("assert alloc-free>=0"),
        Command::Assert(Assertion::AllocFree(Cmp::Ge, 0))
    );
    assert_eq!(
        one("assert complex-raw>=4"),
        Command::Assert(Assertion::ComplexRaw(Cmp::Ge, 4))
    );
    assert_eq!(
        one("assert complex-gold=1"),
        Command::Assert(Assertion::ComplexGold(Cmp::Eq, 1))
    );
    assert_eq!(one("complex raw 60"), Command::ComplexRaw(60));
    assert_eq!(one("complex fuel 40"), Command::ComplexFuel(40));
    assert_eq!(one("complex gold -5"), Command::ComplexGold(-5));

    for source in [
            "allocate nope",
            "allocate select nope",
            "allocate set",
            "allocate set raw nope",
            "allocate row sideways",
            "assert alloc=nope",
            "assert alloc-raw=nope",
    ] {
        assert!(
            parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
        );
    }
}

#[test]
fn prefs_commands_are_parsed() {
    assert_eq!(one("prefs open"), Command::Prefs(PrefsCommand::Open));
    assert_eq!(
        one("prefs toggle effects"),
        Command::Prefs(PrefsCommand::Toggle("effects".to_string()))
    );
    assert_eq!(
        one("prefs toggle sound"),
        Command::Prefs(PrefsCommand::Toggle("sound".to_string()))
    );
    assert_eq!(
        one("prefs set quick_scroll 36"),
        Command::Prefs(PrefsCommand::Set {
            name: "quick_scroll".to_string(),
            value: "36".to_string()
        })
    );
    assert_eq!(
        one("prefs set timer 90"),
        Command::Prefs(PrefsCommand::Set {
            name: "timer".to_string(),
            value: "90".to_string()
        })
    );
    assert_eq!(
        one("prefs set player_name Commander"),
        Command::Prefs(PrefsCommand::Set {
            name: "player_name".to_string(),
            value: "Commander".to_string()
        })
    );
    assert_eq!(
        one("prefs set player_name Jane Doe"),
        Command::Prefs(PrefsCommand::Set {
            name: "player_name".to_string(),
            value: "Jane Doe".to_string()
        })
    );
    assert_eq!(
        one("assert prefs-text player_name=Commander"),
        Command::Assert(Assertion::PrefsText(
            crate::preferences::PrefKey::PlayerName,
                "Commander".to_string()
        ))
    );
    assert_eq!(one("prefs done"), Command::Prefs(PrefsCommand::Done));
    assert_eq!(one("prefs cancel"), Command::Prefs(PrefsCommand::Cancel));
    assert_eq!(one("prefs help"), Command::Prefs(PrefsCommand::Help));

    for source in [
            "prefs",
            "prefs nope",
            "prefs toggle",
            "prefs toggle bogus",
            "prefs toggle quick_scroll", // slider, not a checkbox
            "prefs set effects 1",       // checkbox, not a slider
            "prefs set quick_scroll nope",
            "prefs set quick_scroll",
    ] {
        assert!(
            parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
        );
    }
}

#[test]
fn cmp_tests_match_the_operator() {
    assert!(Cmp::Eq.test(3, 3));
    assert!(!Cmp::Eq.test(3, 4));
    assert!(Cmp::Ge.test(3, 3));
    assert!(!Cmp::Ge.test(2, 3));
    assert!(Cmp::Le.test(2, 3));
    assert!(Cmp::Gt.test(4, 3));
    assert!(Cmp::Lt.test(2, 3));
    assert_eq!(Cmp::Ge.as_str(), ">=");
}
