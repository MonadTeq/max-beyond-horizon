//! In-place unit rename in the corner portrait — C++
//! `GameManager_FlicButtonRFunction` + `GameManager_TextEditUnitName`
//! (`game_manager.cpp:3993-4023`, `:5371-5381`).
//!
//! # Oracle flow
//!
//! * the corner portrait is a `GNW` button (`Gamemanager_FlicButton`,
//!   `game_manager.cpp:4000-4003`): it is enabled only for an own unit of a
//!   `TEAM_TYPE_PLAYER` team, and a click on it creates a `TextEdit` over the
//!   name line of the portrait:
//!   `GameManager_SelectedUnit->GetName(GameManager_UnitName, …)` — the field is
//!   initialized with [`UnitInfo::GetName`] (`"Scout 7"`), not empty;
//! * the field starts at `Text_GetWidth(display) - Text_GetWidth(name)`, i.e.
//!   exactly after the `Mk <roman> ` prefix of `GetDisplayName`
//!   ([`crate::unit_names::unit_display_name`]), and spans the rest of the
//!   window;
//! * the `TextEdit` buffer is 30 bytes (`TextEdit(window, GameManager_UnitName, 30, …)`),
//!   so at most [`NAME_MAX_LEN`] = 29 bytes are accepted;
//! * `Enter` (`GNW_KB_KEY_RETURN`) commits: `UnitInfo::SetName(text)` and, in a
//!   network game, `Remote_SendNetPacket_43`; `Esc` restores the previous name;
//!   an empty string clears the own name (`SetName` deletes the `char*`);
//! * selecting another unit (`GameManager_MenuUnitSelect`,
//!   `game_manager.cpp:7147-7150`) deletes the editor without committing.
//!
//! # Port
//!
//! The field is UI-only state ([`RenameEditor`]); the commit goes through the
//! command channel — [`Command::Rename`] is packet **43** in `max_net`
//! (`wire/command.rs`, `write_packet_43`) and `events::apply_packet_43` applies
//! it via [`UnitInfo::set_name`]. The UI therefore never writes the simulation
//! state directly: the local and the network path are the same command.

use max_core::input::{Command, Packet43Rename};
use max_core::unit::{UnitId, UnitInfo};

/// Longest name the C++ `TextEdit` accepts: buffer 30 bytes minus the
/// terminating NUL (`InsertCharacter`: `strlen(edited_text) < buffer_size - 1`).
pub const NAME_MAX_LEN: usize = 29;

/// Open rename field over the corner portrait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameEditor {
    /// The unit being renamed (it stays the selected unit while the field is open).
    pub unit: UnitId,
    /// Current text of the field — the C++ `GameManager_UnitName`.
    pub text: String,
    /// Text fully selected — the C++ `TextEdit::EnterTextEditField` sets
    /// `is_selected = true`, so the first typed character replaces the whole
    /// `GetName` value instead of appending to it.
    pub selected: bool,
}

impl RenameEditor {
    /// Opens the field for `unit` — `GameManager_FlicButtonRFunction`: the text
    /// starts as `UnitInfo::GetName` (the user name or `"Type <unit_id>"`) and is
    /// selected (`EnterTextEditField`).
    pub fn new(unit: &UnitInfo) -> Self {
        Self {
            unit: unit.id,
            text: crate::unit_names::unit_name(unit),
            selected: true,
        }
    }

    /// Appends a printable character; `false` when the field is full or the
    /// character is a control code (`TextEdit::ProcessUTF8Input`).
    ///
    /// A selected field is cleared first — `InsertCharacter`/`InsertUTF8Text`
    /// start with `if (is_selected) { Clear(); is_selected = false; }`.
    pub fn push_char(&mut self, character: char) -> bool {
        if character.is_control() {
            return false;
        }

        if self.selected {
            self.text.clear();
            self.selected = false;
        }

        // C++ measures the buffer in bytes (`strlen`); the buffer is 30 bytes
        // with the terminating NUL.
        if self.text.len() + character.len_utf8() > NAME_MAX_LEN {
            return false;
        }

        self.text.push(character);
        true
    }

    /// Removes the last character (`TextEdit::Backspace`); a selected field is
    /// cleared as a whole; `false` on an empty, unselected field.
    pub fn backspace(&mut self) -> bool {
        if self.selected {
            self.text.clear();
            self.selected = false;
            return true;
        }

        self.text.pop().is_some()
    }

    /// Packet **43** for the current text — `Remote_SendNetPacket_43` (an empty
    /// name is a valid "clear", it does not cancel the rename).
    pub fn command(&self) -> Command {
        Command::Rename(Packet43Rename {
            unit: self.unit,
            name: self.text.clone(),
        })
    }

    /// Log caption of the committed name (`None` — the name was cleared).
    pub fn log_line(&self) -> String {
        if self.text.is_empty() {
            "Unit name cleared".to_string()
        } else {
            format!("Unit renamed: {}", self.text)
        }
    }
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod tests;
