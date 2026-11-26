//! Frame timing of the shot sound: the gunshot must leave for the audio device in
//! the **same frame** that paints the shot flash.
//!
//! Oracle:
//! * `UnitInfo::PrepareFire` (`unitinfo.cpp:4709-4747`) calls
//!   `PlaySfx(this, Unit::SFX_TYPE_FIRE)` (`:4725`) and only then draws the firing
//!   frame (`DrawSpriteFrame(unit_angle + firing_image_base)`, `:4730-4745`) — the
//!   sound and the flash are produced by one call, in one frame;
//! * `PlaySfx` (`sound_manager.cpp:413`) queues a `SoundJob`; `ProcessJobs`
//!   (`:646`) starts it from the frame's message pump (`process_bk`,
//!   `input.cpp:326`/`gnw.c:909`), i.e. within the same frame, never on a game
//!   input event.
//!
//! The port accumulates `max_core::UnitSound` events in the tick and plays them in
//! the presentation layer, so the driver has to drain the queue once per frame —
//! after the fixed-step ticks, before the frame is drawn
//! ([`App::present_combat_audio`]). These tests pin both halves of the timing:
//! the event appears in the flash tick, and the frame drain submits it at once
//! without any input event (the UI once drained the queue from its input-event
//! loop, which delayed every shot to the next mouse/key event).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use max_app::audio::{AudioBackend, AudioPlayer, PlayMode};
use max_app::render;
use max_app::{App, AppConfig};
use max_assets::WavClip;
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::game::UnitSoundKind;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::UnitId;

/// `SC_UNITS` sounds the tank duel may produce (`data/unit_sfx.json`).
const TANK_TAGS: [&str; 5] = ["TANK14", "TANK15", "TANK16", "LNDEXPLD", "GEN_XPLD"];

/// Player mock: remembers which clip was submitted (`play_scaled`), without a device.
#[derive(Debug, Default)]
struct RecordingBackend {
    played: Arc<Mutex<Vec<(i16, f32)>>>,
}

impl AudioBackend for RecordingBackend {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn play(&mut self, _clip: Arc<WavClip>, _mode: PlayMode) -> Result<(), String> {
        Ok(())
    }

    fn play_scaled(
        &mut self,
        clip: Arc<WavClip>,
        _mode: PlayMode,
        gain: f32,
    ) -> Result<(), String> {
        // The first sample is the marker of the clip (see [`tagged_player`]).
        let marker = clip.samples.first().copied().unwrap_or(-1);

        self.played.lock().expect("lock").push((marker, gain));

        Ok(())
    }
}

/// A player with one preloaded clip per tag; the first sample encodes the tag.
fn tagged_player() -> (
    AudioPlayer,
    Arc<Mutex<Vec<(i16, f32)>>>,
    HashMap<i16, &'static str>,
) {
    let played = Arc::new(Mutex::new(Vec::new()));
    let mut player = AudioPlayer::with_backend(Box::new(RecordingBackend {
        played: played.clone(),
    }));
    let mut tags = HashMap::new();

    for (index, tag) in TANK_TAGS.iter().enumerate() {
        let marker = index as i16;
        player.preload(
            tag,
            WavClip {
                sample_rate: 22050,
                channels: 1,
                samples: vec![marker, 0],
            },
        );
        tags.insert(marker, *tag);
    }

    (player, played, tags)
}

/// Mini-match: a tank (RED, the active team) ordered to fire at a tank (GREEN)
/// one cell away — the same setup as the combat-sound tests.
fn duel_app() -> (App, UnitId, UnitId) {
    let mut app = App::new(AppConfig::default());

    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().teams_mut()[1].team_type = TeamKind::Computer;
    app.universe_mut().team_units_table_mut()[0] = TeamUnits::with_clan(max_core::TeamClan::Random);
    app.universe_mut().team_units_table_mut()[1] = TeamUnits::with_clan(max_core::TeamClan::Random);
    app.universe_mut().set_active_turn_team(TeamId::RED);
    app.universe_mut().set_player_team(TeamId::RED);

    let attacker = app
        .universe_mut()
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("attacker");
    let target = app
        .universe_mut()
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(11, 10),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("target");

    let mut data = app.state_ref().order_data(attacker);
    data.fire_on = Point::new(11, 10);
    data.move_to = Point::new(11, 10);
    app.universe_mut().set_order_data(attacker, data);
    app.universe_mut()
        .set_order(attacker, UnitOrder::Fire, UnitOrderState::Init);

    (app, attacker, target)
}

/// `render::firing_frame_active` for the shooter: the frame that paints the flash.
fn flash_active(app: &App, unit: UnitId) -> bool {
    app.state_ref()
        .unit(unit)
        .is_some_and(render::firing_frame_active)
}

/// The frame that paints the shot flash must also submit the shot sound.
///
/// This is the wiring regression: with the queue drained from the UI input-event
/// loop, a frame without mouse/key input delivered nothing (`submitted == 0`) and
/// the shot stayed queued until the next input event.
#[test]
fn shot_sound_is_submitted_in_the_flash_frame() {
    let (mut app, attacker, _target) = duel_app();
    let (mut player, played, tags) = tagged_player();

    let mut flash_frame = None;

    for frame in 0..40 {
        app.tick();

        let queued = app.unit_sounds().to_vec();
        let flash = flash_active(&app, attacker);

        // The frame's single audio step — exactly what the UI loop does after the
        // fixed-step ticks and before it draws the frame.
        let submitted = app.present_combat_audio(&mut player);

        assert_eq!(
            app.unit_sounds().len(),
            0,
            "frame {frame}: the frame drain must empty the combat queue"
        );

        if flash {
            flash_frame = Some(frame);
            assert!(
                queued.iter().any(|sound| sound.tag == "TANK14"),
                "frame {frame}: the flash frame must queue the tank shot (TANK14), queued {queued:?}"
            );
            assert!(
                submitted >= 1,
                "frame {frame}: the shot must reach the audio device in the flash frame"
            );
            break;
        }
    }

    assert!(
        flash_frame.is_some(),
        "the tank never reached the firing frame"
    );

    // The device actually got the fire sound — the tag, not just the count.
    let calls = played.lock().expect("lock");
    let fired = calls
        .iter()
        .any(|(marker, _)| tags.get(marker).copied() == Some("TANK14"));

    assert!(
        fired,
        "the tank shot (TANK14) never reached the device: {calls:?}"
    );
}

/// The `UnitSound` event is queued by the very tick that switches the unit into the
/// firing frame — the port's counterpart of `PlaySfx` before `DrawSpriteFrame`
/// (`unitinfo.cpp:4725` → `:4730`), not at the impact (`ProgressFire` → `Attack`).
#[test]
fn shot_sound_event_is_queued_by_the_flash_tick() {
    let (mut app, attacker, _target) = duel_app();
    // `GameUniverse::tick` moves the state's combat sounds into the facade stream;
    // the cursor is taken before the first tick, as the client does.
    let mut stream = app.universe_mut().subscribe();
    let mut flash_frame = None;

    for frame in 0..40 {
        app.universe_mut().tick();

        let events: Vec<_> = stream
            .drain(app.universe_mut())
            .into_iter()
            .filter_map(|event| match event {
                max_core::universe::GameEvent::Sound(sound) => Some(sound),
                _ => None,
            })
            .collect();
        let fire = events
            .iter()
            .any(|event| event.unit == attacker && event.kind == UnitSoundKind::Fire);
        let flash = flash_active(&app, attacker);

        if flash {
            assert!(
                fire,
                "frame {frame}: the tick that paints the flash must emit SFX_TYPE_FIRE, \
                 sound events {events:?}"
            );
            flash_frame = Some(frame);
            break;
        }

        assert!(
            !fire,
            "frame {frame}: a shot must not be heard before its flash frame"
        );
    }

    assert!(
        flash_frame.is_some(),
        "the tank never reached the firing frame"
    );
}

/// A run of frames without any input event must still play the shots: the queue is
/// drained every frame, so it neither accumulates nor overflows (`MAX_QUEUED`).
#[test]
fn shots_are_played_without_input_events() {
    let (mut app, attacker, _target) = duel_app();
    let (mut player, played, _tags) = tagged_player();

    let mut ticks_with_fire = 0;

    // No input events at all: only the simulation clock and the frame audio step.
    for _ in 0..40 {
        app.tick();

        if app.unit_sounds().iter().any(|sound| sound.tag == "TANK14") {
            ticks_with_fire += 1;
        }

        app.present_combat_audio(&mut player);

        assert!(
            app.unit_sounds().is_empty(),
            "the combat queue must not survive a frame without input"
        );
    }

    assert!(ticks_with_fire >= 1, "the tank never fired");
    assert!(
        !played.lock().expect("lock").is_empty(),
        "a shot fired without input events reached no audio device"
    );
    // The shooter is still alive (the duel just starts), so the flash is real.
    assert!(app.state_ref().unit(attacker).is_some());
}
