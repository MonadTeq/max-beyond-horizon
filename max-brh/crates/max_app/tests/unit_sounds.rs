//! Combat sounds: unit shot/hit/explosion via the `SC_UNITS` `sfx` table and
//! the `AudioPlayer` mock (without an audio device).
//!
//! Oracle:
//! * `UnitInfo::PrepareFire` (`unitinfo.cpp:4725`) — `PlaySfx(this, SFX_TYPE_FIRE)`;
//! * `UnitsManager_StartExplosion` (`units_manager.cpp:3922/3929`) —
//!   `SFX_TYPE_HIT` (the unit is alive) / `SFX_TYPE_EXPLOAD` (destroyed);
//! * `SoundManager::PlaySfx(UnitInfo*, SfxType)` (`sound_manager.cpp:413-490`) —
//!   the resource and volume from the unit table: `units[TOKEN].sound_effects`, otherwise
//!   `default_sound_effects` (`ParseSoundEffects`, `units.cpp:228-290`).
//!
//! The simulation only accumulates `max_core::UnitSound` events (without references to
//! resources), the client turns them into tags (`max_app::sfx::combat_sound`) and
//! plays them via `AudioPlayer::play_gain` (`volume / 100`).

use std::sync::{Arc, Mutex};

use max_app::audio::{AudioBackend, AudioPlayer, PlayMode};
use max_app::sfx::{self, CombatSfx};
use max_app::{App, AppConfig};
use max_assets::WavClip;
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::game::UnitSoundKind;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};

/// Player mock: remembers tags with gain (`play_scaled`), without a device.
#[derive(Debug, Default)]
struct MockBackend {
    played: Arc<Mutex<Vec<(String, f32)>>>,
}

impl AudioBackend for MockBackend {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn play(&mut self, _clip: Arc<WavClip>, _mode: PlayMode) -> Result<(), String> {
        Ok(())
    }

    fn play_scaled(
        &mut self,
        _clip: Arc<WavClip>,
        _mode: PlayMode,
        gain: f32,
    ) -> Result<(), String> {
        self.played
            .lock()
            .expect("lock")
            .push(("(clip)".to_string(), gain));

        Ok(())
    }
}

fn clip() -> WavClip {
    WavClip {
        sample_rate: 22050,
        channels: 1,
        samples: vec![1, 2, 3],
    }
}

/// Mini-match: a shooter tank (RED) and a target tank (GREEN) next to each other.
fn duel_state(
    fire_on: Point,
) -> (
    max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    max_core::UnitId,
    max_core::UnitId,
) {
    let mut state = max_core::universe::GameUniverse::default();
    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = TeamKind::Player;
    state
        .team_mut(TeamId::GREEN)
        .expect("GREEN slot")
        .team_type = TeamKind::Computer;
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(max_core::TeamClan::Random);
    *state.team_units_mut(TeamId::GREEN).expect("GREEN economy") =
        TeamUnits::with_clan(max_core::TeamClan::Random);

    let attacker = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("attacker");
    let target = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            fire_on,
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("target");

    let mut data = state.snapshot().order_data(attacker);
    data.fire_on = fire_on;
    data.move_to = fire_on;
    state.set_order_data(attacker, data);
    state.set_order(attacker, UnitOrder::Fire, UnitOrderState::Init);

    (state, attacker, target)
}

/// Collects the match combat sounds from the facade event stream:
/// `GameEvent::Sound` → `sfx::combat_sound`.
fn collect(
    state: &mut max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    events: &mut max_core::universe::EventStream,
) -> Vec<CombatSfx> {
    events
        .drain(state)
        .into_iter()
        .filter_map(|event| match event {
            max_core::universe::GameEvent::Sound(sound) => sfx::combat_sound(sound),
            _ => None,
        })
        .collect()
}

#[test]
fn tank_fire_and_explosion_use_the_sc_units_sound_table() {
    let (mut state, _attacker, _target) = duel_state(Point::new(11, 10));
    let mut events = state.subscribe();

    let mut sounds = Vec::new();
    for _ in 0..40 {
        state.tick();
        sounds.extend(collect(&mut state, &mut events));
        if sounds.iter().any(|sound| sound.tag == "TANK14")
            && sounds
                .iter()
                .any(|sound| sound.tag == "TANK15" || sound.tag == "TANK16")
        {
            break;
        }
    }

    // `PrepareFire` → `SFX_TYPE_FIRE` of the tank = `TANK14` (volume 60 → gain 0.6).
    let fire = sounds
        .iter()
        .find(|sound| sound.tag == "TANK14")
        .unwrap_or_else(|| panic!("нет выстрела TANK14 в {sounds:?}"));
    assert!((fire.gain - 0.6).abs() < 1e-6, "volume 60 → 0.6");
    assert!(!fire.is_default);

    // Hit/explosion of the target: `TANK15` (hit) if the tank survived or `TANK16`
    // (expload, volume 80) if destroyed — the same SC_UNITS table.
    assert!(
        sounds
            .iter()
            .any(|sound| sound.tag == "TANK15" || sound.tag == "TANK16"),
        "нет попадания/взрыва в {sounds:?}"
    );
}

#[test]
fn sounds_reach_the_player_through_the_mock_audio_backend() {
    let (mut state, _attacker, _target) = duel_state(Point::new(11, 10));
    let mut events = state.subscribe();

    let mut sounds = Vec::new();
    for _ in 0..40 {
        state.tick();
        sounds.extend(collect(&mut state, &mut events));
        if !sounds.is_empty() {
            break;
        }
    }
    assert!(!sounds.is_empty(), "выстрел обязан дать звук");

    let played = Arc::new(Mutex::new(Vec::new()));
    let mut player = AudioPlayer::with_backend(Box::new(MockBackend {
        played: played.clone(),
    }));

    // The clips "exist" — the tag is registered manually: the test needs no game directory.
    for sound in &sounds {
        player.preload(sound.tag, clip());
    }

    for sound in &sounds {
        assert!(
            player.play_gain(sound.tag, PlayMode::Once, sound.gain),
            "звук {} не проигрался (ошибка: {:?})",
            sound.tag,
            player.last_error()
        );
    }

    assert_eq!(player.plays(), sounds.len() as u64);
    let calls = played.lock().expect("lock");
    assert_eq!(calls.len(), sounds.len());

    // Gain — `volume / 100` from SC_UNITS (tank fire: 60 → 0.6).
    let expected = sounds[0].gain;
    assert!((calls[0].1 - expected).abs() < 1e-6);
}

#[test]
fn disabled_sound_does_not_reach_the_backend() {
    let (mut state, _attacker, _target) = duel_state(Point::new(11, 10));
    let mut events = state.subscribe();

    let mut sounds = Vec::new();
    for _ in 0..40 {
        state.tick();
        sounds.extend(collect(&mut state, &mut events));
        if !sounds.is_empty() {
            break;
        }
    }

    let played = Arc::new(Mutex::new(Vec::new()));
    let mut player = AudioPlayer::with_backend(Box::new(MockBackend {
        played: played.clone(),
    }));
    player.preload(sounds[0].tag, clip());
    // `Preferences::disable_fx` — `AudioPlayer::set_enabled(false)`.
    player.set_enabled(false);

    assert!(!player.play_gain(sounds[0].tag, PlayMode::Once, sounds[0].gain));
    assert_eq!(player.plays(), 0);
    assert!(played.lock().expect("lock").is_empty());
}

#[test]
fn app_collects_combat_sounds_into_the_frame_queue() {
    // `App::tick` → `collect_combat_sounds`: the frame queue is filled with SC_UNITS
    // tags, `take_unit_sounds` returns it (`main.rs` plays via
    // `AudioPlayer::play_gain`).
    let mut app = App::new(AppConfig::default());

    // Tank attributes are taken from the team `current_values`, and firing happens on
    // the active turn — like in a real match (`ResourceManager_InitTeamUnits`).
    app.universe.teams_mut()[0].team_type = TeamKind::Player;
    app.universe.teams_mut()[1].team_type = TeamKind::Computer;
    app.universe.team_units_table_mut()[0] = TeamUnits::with_clan(max_core::TeamClan::Random);
    app.universe.team_units_table_mut()[1] = TeamUnits::with_clan(max_core::TeamClan::Random);
    app.universe.set_active_turn_team(TeamId::RED);
    app.universe.set_player_team(TeamId::RED);

    let attacker = app
        .universe
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("attacker");
    let target = app
        .universe
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(11, 10),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("target");
    let _ = target;

    let mut data = app.state_ref().order_data(attacker);
    data.fire_on = Point::new(11, 10);
    data.move_to = Point::new(11, 10);
    app.universe.set_order_data(attacker, data);
    app.universe
        .set_order(attacker, UnitOrder::Fire, UnitOrderState::Init);

    let mut collected = Vec::new();
    for _ in 0..40 {
        app.tick();
        collected.extend(app.take_unit_sounds());
        if !collected.is_empty() {
            break;
        }
    }

    assert!(
        !collected.is_empty(),
        "выстрел обязан попасть в очередь звуков кадра"
    );
    assert!(
        collected.iter().any(|sound| sound.tag == "TANK14"),
        "ожидался TANK14 (fire танка), получено {collected:?}"
    );
    assert!(
        collected.iter().all(|sound| sound.gain > 0.0),
        "громкость из SC_UNITS всегда > 0"
    );
    // The queue is returned once (the frame does not play sounds twice).
    assert!(app.unit_sounds().is_empty());
}

#[test]
fn unit_sound_kind_covers_fire_hit_explode() {
    // The table covers all three combat `SfxType`s for each of the 93 types
    // (none has `INVALID_ID` in SC_UNITS: the defaults are always set).
    for index in 0..max_core::enums::UNIT_END {
        let unit_type = UnitType::try_from(index).expect("тип 0..92");
        for kind in [
            UnitSoundKind::Fire,
            UnitSoundKind::Hit,
            UnitSoundKind::Explode,
        ] {
            assert!(
                sfx::unit_sound(unit_type, kind).is_some(),
                "{unit_type:?}: нет звука {kind:?}"
            );
        }
    }
}
