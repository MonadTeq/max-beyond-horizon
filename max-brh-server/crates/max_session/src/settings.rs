//! Generates the `settings.json` a headless session runs with.
//!
//! The session directory starts as a copy of the manager's template directory; only
//! `settings.json` is rewritten, carrying the session port, the ticket and the game rules.

use crate::config::{SessionConfig, TeamConfig};
use std::path::Path;

/// Rewrites `settings.json` in `session_dir` for the given session.
///
/// The file must already exist (copied from the template); it is patched in place so any
/// template-specific keys survive unchanged.
pub fn write_session_settings(session_dir: &Path, config: &SessionConfig, port: u16, ticket: &str, game_data: &Path) -> anyhow::Result<()> {
    let path = session_dir.join("settings.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| anyhow::anyhow!("failed to read {}: {error}", path.display()))?;

    // Templates exported by Windows tooling often carry a UTF-8 BOM, which serde_json rejects.
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(&text);

    let mut value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| anyhow::anyhow!("{} is not valid JSON: {error}", path.display()))?;

    let settings = value
        .as_object_mut()
        .and_then(|root| root.get_mut("settings"))
        .and_then(|settings| settings.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("{} has no \"settings\" object", path.display()))?;

    // SETUP: the headless server never plays, sound/movies stay off, data path points at the
    // configured game data folder.
    {
        let setup = section(settings, "SETUP");
        setup.insert("player_name".to_string(), serde_json::json!("Server"));
        setup.insert("game_data".to_string(), serde_json::json!(game_data.to_string_lossy()));
        setup.insert("intro_movie".to_string(), serde_json::json!(0));
        setup.insert("disable_music".to_string(), serde_json::json!(1));
        setup.insert("disable_fx".to_string(), serde_json::json!(1));
        setup.insert("disable_voice".to_string(), serde_json::json!(1));
        setup.insert("disable_movie".to_string(), serde_json::json!(1));
    }

    // OPTIONS: the game rules of this session.
    {
        let options = section(settings, "OPTIONS");
        options.insert("world".to_string(), serde_json::json!(config.world));
        options.insert("timer".to_string(), serde_json::json!(config.timer));
        options.insert("endturn".to_string(), serde_json::json!(config.endturn));
        options.insert("play_mode".to_string(), serde_json::json!(config.play_mode));
        options.insert("start_gold".to_string(), serde_json::json!(config.start_gold));
        options.insert("victory_type".to_string(), serde_json::json!(config.victory_type));
        options.insert("victory_limit".to_string(), serde_json::json!(config.victory_limit));
        options.insert("raw_resource".to_string(), serde_json::json!(config.raw_resource));
        options.insert("fuel_resource".to_string(), serde_json::json!(config.fuel_resource));
        options.insert("gold_resource".to_string(), serde_json::json!(config.gold_resource));
        options.insert("alien_derelicts".to_string(), serde_json::json!(config.alien_derelicts));
    }

    // TEAMS: the server hosts without taking a team. The first `min_players` slots stay open
    // for remote players to claim (team_player 0 = NONE until a client joins and marks the
    // team REMOTE); every remaining slot is a COMPUTER team (team_player 2) so the headless
    // server and all peers simulate the same AI armies. Writing 0 for every team diverged
    // from clients that run AI teams and desynced on turn 1.
    {
        let teams = section(settings, "TEAMS");

        for (index, team) in config.teams.iter().enumerate() {
            write_team(teams, team_color(index), team, (index as u32) < config.min_players);
        }
    }

    // NETWORK: the dedicated port of this session and the ticket Horizon handed out.
    {
        let network = section(settings, "NETWORK_SETTINGS");
        network.insert("host_port".to_string(), serde_json::json!(port));
        network.insert("host_address".to_string(), serde_json::json!(config.host_address));
        network.insert("server_session_ticket".to_string(), serde_json::json!(ticket));
    }

    // DEBUG: dedicated-server mode.
    {
        let debug = section(settings, "DEBUG");
        debug.insert("server_mode".to_string(), serde_json::json!(1));
        debug.insert("server_min_players".to_string(), serde_json::json!(config.min_players));
        debug.insert("log_file_debug".to_string(), serde_json::json!(1));
    }

    std::fs::write(&path, serde_json::to_string_pretty(&value)?)
        .map_err(|error| anyhow::anyhow!("failed to write {}: {error}", path.display()))?;

    Ok(())
}

fn section<'a>(settings: &'a mut serde_json::Map<String, serde_json::Value>, name: &str) -> &'a mut serde_json::Map<String, serde_json::Value> {
    settings
        .entry(name.to_string())
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .expect("settings section is an object")
}

fn team_color(index: usize) -> &'static str {
    match index {
        0 => "red",
        1 => "green",
        2 => "blue",
        _ => "gray",
    }
}

fn write_team(teams: &mut serde_json::Map<String, serde_json::Value>, color: &str, team: &TeamConfig, open: bool) {
    // The game's team_player setting carries the TEAM_TYPE enum: 0 = NONE (open slot, no
    // team simulated), 1 = PLAYER, 2 = COMPUTER, 3 = REMOTE. A remote player claims an open
    // slot at join time and the headless server marks it REMOTE itself.
    teams.insert(format!("{color}_team_player"), serde_json::json!(if open { 0 } else { 2 }));
    teams.insert(format!("{color}_team_clan"), serde_json::json!(team.clan));

    if !team.name.is_empty() {
        teams.insert(format!("{color}_team_name"), serde_json::json!(team.name));
    }
    if !team.strategy.is_empty() {
        teams.insert(format!("{color}_team_strategy"), serde_json::json!(team.strategy));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SessionConfig;

    fn template_settings() -> serde_json::Value {
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "settings": {
                "SETUP": {
                    "player_name": "Player 1",
                    "game_data": "."
                },
                "OPTIONS": {
                    "world": 5,
                    "timer": 60,
                    "endturn": 30
                },
                "TEAMS": {
                    "red_team_name": "Red Team",
                    "green_team_name": "Green Team"
                },
                "NETWORK_SETTINGS": {
                    "transport": "udp_default",
                    "host_port": 31554
                }
            }
        })
    }

    fn write_temp(config: &SessionConfig) -> (tempfile::TempDir, serde_json::Value) {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = temp.path().join("settings.json");
        std::fs::write(&settings_path, template_settings().to_string()).unwrap();

        write_session_settings(temp.path(), config, 25001, "ticket-1", std::path::Path::new("/data/MAX")).unwrap();

        let rewritten: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(settings_path).unwrap()).unwrap();
        (temp, rewritten)
    }

    #[test]
    fn patches_port_ticket_and_server_mode() {
        let (_temp, settings) = write_temp(&SessionConfig::default());

        assert_eq!(settings["settings"]["NETWORK_SETTINGS"]["host_port"], 25001);
        assert_eq!(settings["settings"]["NETWORK_SETTINGS"]["server_session_ticket"], "ticket-1");
        assert_eq!(settings["settings"]["DEBUG"]["server_mode"], 1);
        assert_eq!(settings["settings"]["DEBUG"]["server_min_players"], 2);
        assert_eq!(settings["settings"]["SETUP"]["game_data"], "/data/MAX");
    }

    #[test]
    fn patches_game_rules() {
        let mut config = SessionConfig::default();
        config.world = 3;
        config.timer = 45;
        config.play_mode = 0;
        config.start_gold = 400;

        let (_temp, settings) = write_temp(&config);

        assert_eq!(settings["settings"]["OPTIONS"]["world"], 3);
        assert_eq!(settings["settings"]["OPTIONS"]["timer"], 45);
        assert_eq!(settings["settings"]["OPTIONS"]["play_mode"], 0);
        assert_eq!(settings["settings"]["OPTIONS"]["start_gold"], 400);
    }

    #[test]
    fn open_slots_and_computer_teams_follow_min_players() {
        // Default min_players = 2: red and green are open (0), blue and gray are COMPUTER (2).
        let (_temp, settings) = write_temp(&SessionConfig::default());

        for (index, color) in ["red", "green", "blue", "gray"].iter().enumerate() {
            let expected = if index < 2 { 0 } else { 2 };
            assert_eq!(settings["settings"]["TEAMS"][format!("{color}_team_player")], expected, "{color}");
        }
    }

    #[test]
    fn single_player_session_runs_three_computer_teams() {
        let mut config = SessionConfig::default();
        config.min_players = 1;

        let (_temp, settings) = write_temp(&config);

        assert_eq!(settings["settings"]["TEAMS"]["red_team_player"], 0);
        assert_eq!(settings["settings"]["TEAMS"]["green_team_player"], 2);
        assert_eq!(settings["settings"]["TEAMS"]["blue_team_player"], 2);
        assert_eq!(settings["settings"]["TEAMS"]["gray_team_player"], 2);
    }

    #[test]
    fn team_overrides_are_written() {
        let mut config = SessionConfig::default();
        config.teams[1].name = "Green Machine".to_string();
        config.teams[1].clan = 5;

        let (_temp, settings) = write_temp(&config);

        assert_eq!(settings["settings"]["TEAMS"]["green_team_name"], "Green Machine");
        assert_eq!(settings["settings"]["TEAMS"]["green_team_clan"], 5);
        // Untouched template keys survive.
        assert_eq!(settings["settings"]["TEAMS"]["red_team_name"], "Red Team");
    }

    #[test]
    fn missing_file_reports_an_error() {
        let temp = tempfile::tempdir().unwrap();

        assert!(write_session_settings(temp.path(), &SessionConfig::default(), 25001, "", std::path::Path::new(".")).is_err());
    }

    #[test]
    fn malformed_json_reports_an_error() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("settings.json"), b"{ not json").unwrap();

        assert!(write_session_settings(temp.path(), &SessionConfig::default(), 25001, "", std::path::Path::new(".")).is_err());
    }
}
