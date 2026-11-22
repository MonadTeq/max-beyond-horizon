//! Mission category (Infrastructure).
//!
//! [`MissionCategory`] — a pure enumeration of C++ `MISSION_CATEGORY_*`
//! (`mission.hpp:31`) independent of simulation data. Mission rules
//! (`mission_win_loss`, `WinLossStatus`) stay in `max_core::victory`.

use crate::enums::FileType;

/// `MissionCategory` (`mission.hpp:31`) — mission category (not to be confused with
/// `FileType` of the save header; the mapping is [`MissionCategory::from_file_type`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MissionCategory {
    /// `MISSION_CATEGORY_CUSTOM`.
    Custom = 0,
    /// `MISSION_CATEGORY_TRAINING`.
    Training = 1,
    /// `MISSION_CATEGORY_CAMPAIGN`.
    Campaign = 2,
    /// `MISSION_CATEGORY_HOT_SEAT`.
    HotSeat = 3,
    /// `MISSION_CATEGORY_MULTI`.
    Multiplayer = 4,
    /// `MISSION_CATEGORY_DEMO`.
    Demo = 5,
    /// `MISSION_CATEGORY_SCENARIO`.
    Scenario = 6,
    /// `MISSION_CATEGORY_MULTI_PLAYER_SCENARIO`.
    MultiPlayerScenario = 7,
}

impl MissionCategory {
    /// Mapping of the save header `FileType` to the mission category
    /// (`saveload.cpp` passes the category to `SaveLoad_LoadFormatV70`).
    pub fn from_file_type(file_type: FileType) -> Self {
        match file_type {
            FileType::Custom | FileType::Debug | FileType::Text => Self::Custom,
            FileType::Tutorial => Self::Training,
            FileType::Campaign => Self::Campaign,
            FileType::HotSeat => Self::HotSeat,
            FileType::Multiplayer => Self::Multiplayer,
            FileType::Demo => Self::Demo,
            FileType::Scenario => Self::Scenario,
            FileType::MultiScenario => Self::MultiPlayerScenario,
        }
    }
}
