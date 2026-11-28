//! Unit tests of the in-place rename field ([`RenameEditor`]).
//!
//! The field is initialized with `UnitInfo::GetName` (`game_manager.cpp:4001`),
//! accepts at most 29 bytes (`TextEdit(..., 30, ...)`), commits packet 43 and
//! treats an empty string as "clear the name" (`UnitInfo::SetName` with
//! `strlen == 0`).

use super::*;
use max_core::enums::UnitType;
use max_core::input::Command;

/// An unnamed Scout at mark level 2, instance 7 — the owner's `"Mk II Scout 7"`.
fn scout() -> UnitInfo {
    let mut unit = UnitInfo::new(UnitType::Scout, max_core::TeamId::RED, UnitId(3));
    unit.unit_id = 7;
    unit.base_values.version = 2;
    unit
}

#[test]
fn field_starts_with_get_name_not_with_an_empty_string() {
    let unit = scout();
    let editor = RenameEditor::new(&unit);

    // `GameManager_FlicButtonRFunction`: `GetName` → the edit field text.
    assert_eq!(editor.text, "Scout 7");
    assert_eq!(editor.unit, unit.id);
}

#[test]
fn named_unit_field_starts_with_the_user_name() {
    let mut unit = scout();
    unit.set_name("Oddball");
    let editor = RenameEditor::new(&unit);

    assert_eq!(editor.text, "Oddball");
}

/// `TextEdit::EnterTextEditField` selects the whole initial text, and
/// `InsertUTF8Input` clears a selected field before inserting: the first typed
/// character replaces `GetName`, it does not append to it.
#[test]
fn first_character_replaces_the_selected_get_name() {
    let unit = scout();
    let mut editor = RenameEditor::new(&unit);

    assert!(editor.selected);
    assert!(editor.push_char('O'));
    assert!(!editor.selected);
    assert_eq!(editor.text, "O");

    assert!(editor.push_char('d'));
    assert_eq!(editor.text, "Od");
}

/// `TextEdit::Backspace` on a selected field clears it as a whole.
#[test]
fn backspace_clears_a_selected_field() {
    let unit = scout();
    let mut editor = RenameEditor::new(&unit);

    assert!(editor.backspace());
    assert!(editor.text.is_empty());
    assert!(!editor.selected);
    assert!(!editor.backspace(), "nothing left to delete");
}

#[test]
fn typing_appends_and_backspace_removes() {
    let unit = scout();
    let mut editor = RenameEditor::new(&unit);

    editor.text.clear();
    assert!(editor.push_char('O'));
    assert!(editor.push_char('d'));
    assert!(editor.push_char('d'));
    assert_eq!(editor.text, "Odd");
    assert!(editor.backspace());
    assert_eq!(editor.text, "Od");
    assert!(editor.backspace());
    assert!(editor.backspace());
    assert!(!editor.backspace(), "empty field has nothing to delete");
}

#[test]
fn control_characters_are_ignored() {
    let unit = scout();
    let mut editor = RenameEditor::new(&unit);

    assert!(!editor.push_char('\n'));
    assert!(!editor.push_char('\u{7}'));
    assert_eq!(editor.text, "Scout 7");
}

#[test]
fn field_is_limited_to_29_bytes_like_the_cpp_text_edit() {
    let unit = scout();
    let mut editor = RenameEditor::new(&unit);

    editor.text.clear();
    for _ in 0..NAME_MAX_LEN {
        assert!(editor.push_char('a'));
    }
    assert_eq!(editor.text.len(), NAME_MAX_LEN);
    assert!(
        !editor.push_char('b'),
        "the 30th byte must not fit (buffer 30 with the terminating NUL)"
    );
    assert_eq!(editor.text.len(), NAME_MAX_LEN);
}

#[test]
fn commit_builds_packet_43_with_the_typed_name() {
    let unit = scout();
    let mut editor = RenameEditor::new(&unit);

    editor.text = "Oddball".to_string();

    match editor.command() {
        Command::Rename(packet) => {
            assert_eq!(packet.unit, unit.id);
            assert_eq!(packet.name, "Oddball");
        }
        other => panic!("expected Command::Rename (packet 43), got {other:?}"),
    }
}

#[test]
fn empty_field_commits_an_empty_name_which_clears_it() {
    let unit = scout();
    let mut editor = RenameEditor::new(&unit);

    editor.text.clear();

    match editor.command() {
        Command::Rename(packet) => assert!(packet.name.is_empty()),
        other => panic!("expected Command::Rename (packet 43), got {other:?}"),
    }

    // The C++ `SetName("")` deletes the name; the log reports the clear.
    assert_eq!(editor.log_line(), "Unit name cleared");
}
