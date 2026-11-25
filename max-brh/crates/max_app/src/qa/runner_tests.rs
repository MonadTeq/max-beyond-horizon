use super::*;

#[test]
fn qa_menu_client_lists_two_deterministic_games() {
    let mut client = QaMenuClient;
    let games = client.list_games().expect("список игр");

    assert_eq!(games.len(), 2);
    assert_eq!(games[0].name, "QA Skirmish");
    assert_eq!(games[1].world, 11);
    assert!(client.create_game(CreateGameParams::default()).is_err());
}

#[test]
fn default_config_points_at_the_repository_data() {
    let config = QaConfig::new("qa-out");

    assert!(config.data_dir.ends_with("max-original"));
    assert!(config.assets_dir.ends_with("assets"));
    assert_eq!(config.out_dir, PathBuf::from("qa-out"));
    assert_eq!(config.frame_limit, 40_000);
    assert!(!repo_root().as_os_str().is_empty());
}

#[test]
fn screen_names_and_phase_labels_are_stable() {
    assert_eq!(ScreenName::parse("InGame"), Some(ScreenName::InGame));
    assert_eq!(ScreenName::parse("hot seat"), None);
    assert_eq!(ScreenName::Players.as_str(), "Players");
    assert_eq!(PhaseName::parse("AI"), Some(PhaseName::AiTurn));
    assert_eq!(PhaseName::Finished.as_str(), "Finished");
}
