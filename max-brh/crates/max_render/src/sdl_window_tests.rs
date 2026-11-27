use super::*;

#[test]
fn focus_and_minimize_events_are_input_neutral() {
    for event in [
        WindowEvent::FocusLost,
        WindowEvent::FocusGained,
        WindowEvent::Minimized,
        WindowEvent::Restored,
        WindowEvent::Maximized,
        WindowEvent::Shown,
        WindowEvent::Hidden,
        WindowEvent::Exposed,
        WindowEvent::Occluded,
        WindowEvent::MouseEnter,
        WindowEvent::MouseLeave,
    ] {
        assert!(
            window_event_is_input_neutral(event),
                "{event:?} Р Р…Р Вµ Р Т‘Р С•Р В»Р В¶Р ВµР Р… Р С•РЎРѓРЎвЂљР В°Р Р…Р В°Р Р†Р В»Р С‘Р Р†Р В°РЎвЂљРЎРЉ РЎвЂ Р С‘Р С”Р В» Р С‘Р В»Р С‘ Р Т‘Р В°Р Р†Р В°РЎвЂљРЎРЉ Р Р†Р Р†Р С•Р Т‘"
        );
    }
}

#[test]
fn resize_move_and_close_are_not_neutral_input_events() {
    for event in [
        WindowEvent::Resized(640, 480),
        WindowEvent::Moved(10, 20),
        WindowEvent::CloseRequested,
        WindowEvent::HitTest(1, 2),
    ] {
        assert!(
            !window_event_is_input_neutral(event),
                "{event:?} Р Р…Р Вµ Р Т‘Р С•Р В»Р В¶Р ВµР Р… РЎРѓРЎвЂЎР С‘РЎвЂљР В°РЎвЂљРЎРЉРЎРѓРЎРЏ Р Р…Р ВµР в„–РЎвЂљРЎР‚Р В°Р В»РЎРЉР Р…РЎвЂ№Р С"
        );
    }
}

#[test]
fn alt_enter_is_the_fullscreen_shortcut_only_with_alt() {
    for code in [Keycode::Return, Keycode::Return2, Keycode::KpEnter] {
        assert!(is_toggle_fullscreen_shortcut(code, Mod::LALTMOD));
        assert!(is_toggle_fullscreen_shortcut(code, Mod::RALTMOD));
        assert!(!is_toggle_fullscreen_shortcut(code, Mod::NOMOD));
        assert!(!is_toggle_fullscreen_shortcut(
            code,
            Mod::LSHIFTMOD | Mod::RSHIFTMOD
        ));
    }

    // Alt + a letter is a game key (`Alt+P` РІР‚вЂќ pause), not fullscreen.
    assert!(!is_toggle_fullscreen_shortcut(Keycode::P, Mod::LALTMOD));
    // Enter without Alt is an ordinary UI key.
    assert!(!is_toggle_fullscreen_shortcut(Keycode::Return, Mod::NOMOD));
}

#[test]
fn focus_minimize_and_occlusion_change_drawing_only() {
    use crate::presentation::PresentationState;

    let mut state = PresentationState::new();

    assert!(state.should_draw());
    state.focus_lost();
    assert!(!state.is_focused());
    // Focus loss does not hide the window: the loop still draws it.
    assert!(state.should_draw());
    state.minimized();
    assert!(!state.should_draw());
    state.focus_gained();
    assert!(state.should_draw());
    state.occluded();
    assert!(!state.should_draw());
    state.exposed();
    assert!(state.should_draw());
    // No pause/state methods exist on the presentation state: it cannot
    // reset a match by construction.
}

/// The automatic window scale (`--scale 0`/no flag) fits the **render frame** of
/// the selected resolution into the desktop: this is the rule `reopen_with_render_scale`
/// re-applies when the user picks another resolution in the window menu.
#[test]
fn the_automatic_window_scale_fits_the_render_frame_into_the_desktop() {
    // No usable bounds: the window is at least as large as the render frame.
    assert_eq!(auto_window_scale(None, 640, 496), 1);
    // A 1920x1080 desktop fits 640x496 twice, but 1280x992 only once.
    assert_eq!(auto_window_scale(Some((1920, 1080)), 640, 496), 2);
    assert_eq!(auto_window_scale(Some((1920, 1080)), 1280, 992), 1);
    // The automatic scale is capped at 4 (the historical default, item F).
    assert_eq!(auto_window_scale(Some((7680, 4320)), 640, 496), 4);
    // A desktop smaller than the render frame still gets a 1x window.
    assert_eq!(auto_window_scale(Some((320, 240)), 640, 496), 1);
}
