//! Campaign progression — `last_campaign` / `game_file_number` (plan M5,
//! `campaign-missions.md`).
//!
//! # Oracle
//!
//! * `menu.cpp:518-545` — after a **winning** campaign mission the mission manager
//!   loads the next mission of the list, `game_file_number` becomes its 1-based
//!   index and `last_campaign` is raised to it (never lowered), then the settings
//!   are saved. The won mission being the last of the list (`next_it == end`) leaves
//!   the settings untouched;
//! * `settings.cpp:101-105` — `game_file_number` and `last_campaign` are `SETUP`
//!   settings with default `1`, i.e. the progression is persistent user state, not
//!   a UI flag (`preferences.ini`, the port of the C++ settings file);
//! * `menu.cpp:1968-1983` — `GameSetupMenu_Menu` clamps `last_campaign` to the
//!   campaign mission count, writes the clamp back and opens the list on
//!   `game_file_number = last_campaign`;
//! * `gamesetupmenu.cpp:257-298,402-423` — `DrawMissionList`/`FindNextValidFile`:
//!   a campaign mission is locked while its 0-based index is `>= last_campaign`;
//!   locked rows stay in the list but are disabled and carry no title.
//!
//! # Layering
//!
//! The values live in [`Preferences`]; the menu layer only **reads** the mirror of
//! the application settings. Everything that writes them is here, in the
//! application/install layer: [`App::start_mission_from_registry`] records the
//! chosen mission, [`App::advance_campaign_after_victory`] runs the wrap-up, and
//! [`App::apply_campaign_clamp`] executes the clamp the menu reported. None of it
//! touches `GameState`: `snapshot_bytes` and the CRC never see the progression.
//!
//! [`App::start_mission_from_registry`]: crate::App::start_mission_from_registry

use max_core::universe::Dispatcher;
use max_core::victory::MissionCategory;

use crate::preferences::Preferences;
use crate::App;

/// Result of opening the campaign mission list — `GameSetupMenu_Menu` +
/// `GameSetupMenu::Init` (`menu.cpp:1968-1983`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignMenuOpen {
    /// Effective `last_campaign` (1-based unlocked count), clamped to the list.
    pub last_campaign: i32,
    /// 0-based selected mission (`game_file_number - 1`).
    pub selected: usize,
    /// The stored `last_campaign` was above the list: the oracle clamps it and
    /// saves the settings (`menu.cpp:1972-1977`).
    pub clamped: bool,
}

/// `GameSetupMenu_Menu` for `MISSION_CATEGORY_CAMPAIGN`: `last_campaign` is
/// clamped to `mission_count`, `game_file_number` becomes the clamped value and the
/// list opens on it (`menu.cpp:1968-1983`).
///
/// The oracle only clamps **down**; a value below zero unlocks nothing (the
/// `ini_last_campaign <= mission_index` test in `DrawMissionList` is then true for
/// every row) and the selection saturates to the first row like the C++
/// `game_file_number - 1` does for a non-positive value.
pub fn menu_open(last_campaign: i32, mission_count: usize) -> CampaignMenuOpen {
    let count = i32::try_from(mission_count).unwrap_or(i32::MAX);
    let clamped_last = last_campaign.clamp(0, count);
    let selected = usize::try_from(clamped_last.saturating_sub(1))
        .unwrap_or(0)
        .min(mission_count.saturating_sub(1));

    CampaignMenuOpen {
        last_campaign: clamped_last,
        selected,
        clamped: last_campaign > count,
    }
}

/// New persistent values after a winning campaign mission (`menu.cpp:518-545`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignAdvance {
    /// 1-based index of the next mission — the new `game_file_number`
    /// (`menu.cpp:532`).
    pub game_file_number: i32,
    /// `last_campaign` raised to the next mission if it is lower (`menu.cpp:534-536`).
    pub last_campaign: i32,
}

/// `menu.cpp:518-545`: the next mission of the category becomes
/// `game_file_number` and `last_campaign` never decreases. `None` — the won mission
/// is the last one of the list (`next_it == missions.end()`): the oracle leaves the
/// settings untouched.
pub fn victory_advance(
    current_index: usize,
    mission_count: usize,
    preferences: &Preferences,
) -> Option<CampaignAdvance> {
    let next_index = current_index.checked_add(1)?;

    if next_index >= mission_count {
        return None;
    }

    let game_file_number = i32::try_from(next_index + 1).ok()?;
    let last_campaign = preferences.last_campaign.max(game_file_number);

    Some(CampaignAdvance {
        game_file_number,
        last_campaign,
    })
}

impl<D: Dispatcher> App<D> {
    /// Campaign mission count of the mission registry
    /// (`MissionManager::GetMissions(MISSION_CATEGORY_CAMPAIGN).size()`), the bound
    /// of the victory progression.
    pub fn campaign_mission_count(&self) -> Option<usize> {
        self.campaign_mission_count
    }

    /// Records the registry's campaign mission count.
    ///
    /// The drivers call it once the registry is built; the headless mission start
    /// does too, so the victory wrap-up knows where the mission list ends.
    pub fn set_campaign_mission_count(&mut self, count: usize) {
        self.campaign_mission_count = Some(count);
    }

    /// `GameSetupMenu_Menu` (`menu.cpp:1972-1978`): a `last_campaign` above the
    /// mission list is clamped to the list, `game_file_number` follows it and the
    /// settings file is rewritten.
    ///
    /// The UI only reports the clamp ([`crate::ui::UiContext::campaign_clamp`]);
    /// the menu layer never writes the application (rule C3), so the driver executes
    /// it here.
    pub fn apply_campaign_clamp(&mut self, last_campaign: i32) {
        self.preferences.last_campaign = last_campaign;
        self.preferences.game_file_number = last_campaign;

        tracing::info!("campaign: last_campaign clamped to {last_campaign}");
        self.persist_preferences();
    }
}

impl App {
    /// `menu.cpp:518-545`: a **won** campaign mission advances the persistent
    /// progression to the next mission of the list.
    ///
    /// Called by [`App::leave_local_game`] — the point at which the C++ wrap-up
    /// (after the victory and story screens) returns to the main menu. A defeat, a
    /// non-campaign category and the last mission of the list change nothing.
    ///
    /// Returns `true` when the progression was advanced and written.
    pub fn advance_campaign_after_victory(&mut self) -> bool {
        let won = self
            .local
            .as_ref()
            .and_then(|local| local.outcome.as_ref())
            .is_some_and(|outcome| outcome.victory);

        if !won || self.state_ref().mission_category() != MissionCategory::Campaign {
            return false;
        }

        let (Some(count), Some(index)) = (self.campaign_mission_count, self.campaign_mission_index)
        else {
            tracing::warn!(
                "campaign: the mission list is unknown, the won mission does not \
                 advance the progression"
            );
            return false;
        };

        let Some(advance) = victory_advance(index, count, &self.preferences) else {
            tracing::info!("campaign: the last mission of the list was won");
            return false;
        };

        self.preferences.game_file_number = advance.game_file_number;
        self.preferences.last_campaign = advance.last_campaign;
        self.persist_preferences();
        self.hud.push_log(format!(
            "Campaign: mission {} unlocked",
            advance.game_file_number
        ));
        tracing::info!(
            "campaign: mission {index} won, game_file_number={}, last_campaign={}",
            advance.game_file_number,
            advance.last_campaign
        );

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DrawMissionList`/`EventSelectItem` bound: a mission is unlocked while its
    /// 0-based index is below `last_campaign`.
    #[test]
    fn campaign_list_opens_on_last_campaign_and_limits_the_unlocked_rows() {
        let open = menu_open(3, 9);
        assert_eq!(open.last_campaign, 3);
        assert_eq!(open.selected, 2, "`game_file_number - 1`");
        assert!(!open.clamped);
    }

    /// `menu.cpp:1970-1982`: a value above the list is clamped down and reported for
    /// the write-back; a value below zero unlocks nothing.
    #[test]
    fn campaign_open_clamps_only_above_the_list() {
        let above = menu_open(99, 9);
        assert_eq!(above.last_campaign, 9);
        assert_eq!(above.selected, 8);
        assert!(above.clamped, "the oracle writes the clamp back");

        let exact = menu_open(9, 9);
        assert_eq!(exact.last_campaign, 9);
        assert!(!exact.clamped, "no write-back when the value fits");

        let below = menu_open(-4, 9);
        assert_eq!(below.last_campaign, 0, "negative: nothing unlocked");
        assert_eq!(below.selected, 0);
        assert!(!below.clamped);

        let empty = menu_open(4, 0);
        assert_eq!(empty.last_campaign, 0);
        assert_eq!(empty.selected, 0);
        assert!(empty.clamped, "a non-empty progression over an empty list");
    }

    /// `menu.cpp:518-545`: only a mission that has a successor advances the values;
    /// `last_campaign` is a high-water mark and never decreases.
    #[test]
    fn victory_advance_moves_to_the_next_mission_and_keeps_the_high_water_mark() {
        let mut prefs = Preferences::default();
        assert_eq!(prefs.last_campaign, 1);

        let advance = victory_advance(0, 9, &prefs).expect("mission 2 exists");
        assert_eq!(advance.game_file_number, 2, "1-based next mission");
        assert_eq!(advance.last_campaign, 2);

        prefs.last_campaign = 5;
        let replay = victory_advance(0, 9, &prefs).expect("mission 2 exists");
        assert_eq!(replay.game_file_number, 2);
        assert_eq!(
            replay.last_campaign, 5,
            "the high-water mark does not shrink"
        );

        assert!(
            victory_advance(8, 9, &prefs).is_none(),
            "the last mission has no successor: the settings stay untouched"
        );
        assert!(victory_advance(0, 1, &prefs).is_none());
    }
}
