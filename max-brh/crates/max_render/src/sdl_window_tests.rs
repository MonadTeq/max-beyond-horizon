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
