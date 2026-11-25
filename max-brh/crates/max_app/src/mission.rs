//! Headless mission start: mission registry → descriptor → V70 mission file →
//! local match.
//!
//! Stage 1 of the campaign plan (`max-brh-docs/development/campaign-missions.md`,
//! M4). Oracle: `menu.cpp:2052-2098` — `LoadMission(category, game_file_number - 1)`,
//! `SetNumericValue("game_file_number", …)` and `GameManager_GameLoop(GAME_STATE_10_LOAD_GAME)`;
//! `game_manager.cpp:1929-1961` — the load branch: `mission->GetCategory()`,
//! `mission->GetMission()`, `SaveLoad_Load(filepath, mission_category, …)`, then the
//! turn loop starts (`GAME_STATE_11_TURN_ACTIVE` → `GAME_STATE_8_IN_GAME`).
//!
//! The stage is UI-free. The caller passes the [`MissionRegistry`], the category and
//! the **0-based** mission index (the position inside the category — the same value
//! the C++ `GameSetupMenu` passes as `game_file_number - 1`). No `CargoMenu` /
//! `SelectSite` screen is opened: the mission comes up turn-active through the
//! facade install path (`App::load_save_bytes` → `App::adopt_universe`, stage C3),
//! and nothing outside the install API writes `App.universe`.
//!
//! The mission identity (`mission_category`, `mission_index`) is taken from the V70
//! header by the loader (`saveload.cpp:772`, `1609-1664`; `GameState::load_state`,
//! `game.rs:562-564`) — exactly like the C++ save reconstruction
//! (`SaveLoad_GetSaveFileInfo` → `SaveLoad_TranslateMissionIndexToHashKey`,
//! `saveload.cpp:196`). The registry only selects the file. A header that disagrees
//! with the descriptor is reported as a warning (content damage, like the optional
//! hash check of `mission.cpp:263`) and never zeroes `mission_index`.
//!
//! Out of scope here (later stages of the plan): the `GameSetupMenu` list
//! (stage 2), campaign progression and story screens (stage 3).

use std::path::PathBuf;

use max_assets::{MissionDescriptor, MissionRegistry};
use max_core::victory::MissionCategory;

use crate::local::LocalSetup;
use crate::App;

/// What [`App::start_mission_from_registry`] started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionStart {
    /// Resource tag of the descriptor (`SC_C0001`).
    pub tag: String,
    /// Localized title (`en-US`) — the briefing caption of the plan's stage 2.
    pub title: String,
    /// Mission category of the descriptor (the registry bucket). `GameState` keeps
    /// the save header's category; for shipped missions the two are the same, and a
    /// disagreement is only warned about.
    pub category: MissionCategory,
    /// 1-based `mission_index` of the save header — the value `mission_win_loss`
    /// evaluates (`game.rs:562`, `victory.rs:1444`).
    pub mission_index: u16,
    /// Mission file that was loaded.
    pub path: PathBuf,
}

impl App {
    /// Starts a mission by category and registry index — C++ `LoadMission` followed
    /// by `GAME_STATE_10_LOAD_GAME` (`menu.cpp:2052-2098`).
    ///
    /// `index` is 0-based and is the position inside [`MissionRegistry::missions`];
    /// the C++ menu passes the same value as `game_file_number - 1`. Headless: no
    /// menu is opened and the match starts turn-active.
    ///
    /// # Errors
    ///
    /// There is no mission at `index`; the descriptor has no mission file
    /// (`Custom`/`Hot-seat`/`Multiplayer`, `mission.cpp:272-291`); the file cannot be
    /// read; the file is not a V70 save.
    pub fn start_mission_from_registry(
        &mut self,
        registry: &MissionRegistry,
        category: MissionCategory,
        index: usize,
        setup: &LocalSetup,
    ) -> Result<MissionStart, String> {
        let missions = registry.missions(category);
        let descriptor = missions.get(index).ok_or_else(|| {
            format!(
                "no {category:?} mission at index {index}: the registry has {}",
                missions.len()
            )
        })?;
        let start = self.start_mission_from_descriptor(descriptor, setup)?;

        // `mission_index` is 1-based in the header (`saveload.cpp:772`), while the
        // registry index is the 0-based `game_file_number - 1`; for the shipped
        // missions they match. The header value wins (it is what C++ reconstructs
        // the mission from), so a disagreement is only a warning.
        let expected = u16::try_from(index + 1).unwrap_or(u16::MAX);

        if start.mission_index != expected {
            tracing::warn!(
                "{}: registry index {index} expects mission_index {expected}, the save \
                 header has {}; the win/loss rules use the header",
                descriptor.tag,
                start.mission_index
            );
        }

        Ok(start)
    }

    /// Starts the mission of an already resolved descriptor.
    ///
    /// Reads the mission file and hands it to [`App::start_local_game_from_save`],
    /// the same loader the save/load window uses — the registry path must not drift
    /// from a direct load (`snapshot_bytes` equality is the stage gate).
    ///
    /// # Errors
    ///
    /// The descriptor has no mission file; the file cannot be read; the file is not
    /// a V70 save.
    pub fn start_mission_from_descriptor(
        &mut self,
        descriptor: &MissionDescriptor,
        setup: &LocalSetup,
    ) -> Result<MissionStart, String> {
        let path = descriptor.mission_path().ok_or_else(|| {
            format!(
                "{}: the {:?} category has no mission file \
                 (`Mission_ProcessMission`, mission.cpp:272-291)",
                descriptor.tag, descriptor.category
            )
        })?;
        let bytes = std::fs::read(path).map_err(|error| {
            format!(
                "{}: cannot read the mission file {}: {error}",
                descriptor.tag,
                path.display()
            )
        })?;

        self.start_local_game_from_save(setup, &bytes)
            .map_err(|error| format!("{}: {error}", descriptor.tag))?;

        // The full load path is `menu.cpp:2052-2098` → `game_manager.cpp:1929-1961`:
        // the world is installed, the loaded team is active and the turn loop runs.
        let category = self.state_ref().mission_category();
        let mission_index = self.state_ref().mission_index();

        if category != descriptor.category {
            tracing::warn!(
                "{}: the descriptor category {:?} differs from the save header {:?}",
                descriptor.tag,
                descriptor.category,
                category
            );
        }

        tracing::info!(
            "mission start: {} ({:?}) index {mission_index} from {}",
            descriptor.tag,
            descriptor.category,
            path.display()
        );

        Ok(MissionStart {
            tag: descriptor.tag.clone(),
            title: descriptor.title_text().to_owned(),
            category: descriptor.category,
            mission_index,
            path: path.to_path_buf(),
        })
    }
}
