use super::*;

#[test]
fn lobby_event_parses_game_list() {
    let json = r#"{"event":"game_list","games":[{"id":"abc","name":"smoke-one","world":1,
            "status":"running","port":31554,"players":1,"min_players":2,"password_protected":false}]}"#;
    let event: LobbyEvent = serde_json::from_str(json).unwrap();
    match event {
        LobbyEvent::GameList { games } => assert_eq!(games[0].port, 31554),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn lobby_event_parses_error() {
    let event: LobbyEvent = serde_json::from_str(r#"{"event":"error","error":"bad"}"#).unwrap();
    assert!(matches!(event, LobbyEvent::Error { error } if error == "bad"));
}
