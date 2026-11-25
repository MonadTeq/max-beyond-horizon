//! UI sounds — a port of the `SoundManager::PlaySfx` calls of the C++ port.
//!
//! The module's job is to provide **one** place where a UI event turns into
//! the name of a `MAX.RES` sound resource. They are played by [`crate::audio::AudioPlayer`]
//! (in headless/CI — the silent `NoopBackend`), and `App` only accumulates the event queue:
//! this way tests check "which sound for which event" without an audio device.
//!
//! # Table (oracle — the C++ port)
//!
//! | Event | Resource | Where in C++ |
//! |---|---|---|
//! | click on the map/popup item (by key)/help item/menu rows | `KCARG0` | `game_manager.cpp:6711`, `:2689`; `helpmenu.cpp:331`; `gameconfigmenu.hpp` |
//! | `Done`/confirmation | `NDONE0` | `game_manager.cpp:6727`; `helpmenu.cpp:91`; `menu.cpp:2182+` (buttons `MBUTT0`, `Exit` — `NCANC0`) |
//! | `Cancel`/`Esc`/refusal | `NCANC0` | `units_manager.cpp:278`; `game_manager.cpp:4612`; `menu.cpp:2188` |
//! | panel/main menu buttons | `MBUTT0` | `game_manager.cpp:5446/5692/5713`; `menu.cpp:2182` |
//! | `Help` button of the setup screens | `NHELP0` | `gamesetupmenu.hpp:77`, `networkmenu.hpp` |
//! | player `Human`/`Computer`/`None`/`Clan` | `NHUMN0`/`NCOMP0`/`NNONE0`/`NCLAN0` | `chooseplayermenu.hpp:59-74` |
//! | self-destruct `Arm` | `MBUTT0` | `units_manager.cpp:312` |
//! | placing a build site | `V_M049` | `game_manager.cpp:4569` (`PlayVoice`) |
//! | repair/refuel/cargo transfer | `V_M210`/`V_M085`/`V_M224` | `game_manager.cpp:4609/4626/4675` |
//!
//! Voices (`V_*`) are also `.spw` resources, so they go through the same player; they
//! are disabled by the `disable_voice` setting, the effects — by `disable_fx`
//! (`Preferences`).
//!
//! # Combat sounds (`SC_UNITS`)
//!
//! The shot, hit and explosion take their resource **from the unit table**:
//! `SoundManager::PlaySfx(UnitInfo*, Unit::SfxType)` (`sound_manager.cpp:413-490`)
//! and looks up `Unit::SoundEffectInfo` in
//! `Unit::m_sound_effects` (`units.cpp` `ParseSoundEffects`, `units.cpp:228-290`):
//! first `units[TOKEN].sound_effects`, otherwise `default_sound_effects`. The table
//! is generated into `data/unit_sfx.json` (`tools/gen_unit_sfx.py`) and read by
//! [`unit_sound`]; the events are accumulated by the simulation (`max_core::UnitSound`), and
//! [`combat_sound`] turns them into `(tag, volume)`.
//!
//! C++ calls:
//! * shot — `UnitInfo::PrepareFire` (`unitinfo.cpp:4725`), `SFX_TYPE_FIRE`;
//! * hit — `UnitsManager_StartExplosion` when `hits > 0`
//!   (`units_manager.cpp:3922`), `SFX_TYPE_HIT`;
//! * explosion — in the same place when `hits == 0` (`units_manager.cpp:3929`),
//!   `SFX_TYPE_EXPLOAD` (`LNDEXPLD`/`AIREXPLD`/`SEAEXPLD`/`BLDEXPLD`/
//!   `HITEXPLD` have no sound of their own — `GEN_XPLD` plays, as in C++).

use std::sync::OnceLock;

use max_core::enums::{UnitType, UNIT_END};
use max_core::game::{UnitSound, UnitSoundKind};
use serde::Deserialize;

/// UI sound event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    /// `KCARG0` — click on the map, menu row, help item.
    Click,
    /// `NDONE0` — `Done`/`Enter`/confirmation.
    Done,
    /// `NCANC0` — `Cancel`/`Esc`/unavailable action.
    Cancel,
    /// `MBUTT0` — panel/main menu button.
    Button,
    /// `NHELP0` — `Help` button of the setup screen.
    Help,
    /// `NHUMN0` — choosing «Human» in the player slot.
    PlayerHuman,
    /// `NCOMP0` — choosing «Computer».
    PlayerComputer,
    /// `NNONE0` — choosing «None».
    PlayerNone,
    /// `NCLAN0` — opening the clan selection.
    PlayerClan,
    /// `V_M049` — placing a build site (`GameManager_PathBuild`).
    BuildSitePlaced,
    /// `V_M210` — repair (`GameManager_RepairUnit`).
    Repair,
    /// `V_M085` — refuel/recharge (`GameManager_ReloadUnit`).
    Reload,
    /// `V_M224` — cargo transfer (`GameManager_TransferCargo`).
    Transfer,
}

impl Sfx {
    /// Resource name in `MAX.RES` (`AudioPlayer::play_once`).
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Click => "KCARG0",
            Self::Done => "NDONE0",
            Self::Cancel => "NCANC0",
            Self::Button => "MBUTT0",
            Self::Help => "NHELP0",
            Self::PlayerHuman => "NHUMN0",
            Self::PlayerComputer => "NCOMP0",
            Self::PlayerNone => "NNONE0",
            Self::PlayerClan => "NCLAN0",
            Self::BuildSitePlaced => "V_M049",
            Self::Repair => "V_M210",
            Self::Reload => "V_M085",
            Self::Transfer => "V_M224",
        }
    }

    /// This is a voice (`V_*`), not an SFX: controlled by `disable_voice`.
    pub const fn is_voice(self) -> bool {
        matches!(
            self,
            Self::BuildSitePlaced | Self::Repair | Self::Reload | Self::Transfer
        )
    }
}

// ---------------------------------------------------------------------------
// Unit sounds (combat) — SC_UNITS
// ---------------------------------------------------------------------------

/// Sound JSON table (`tools/gen_unit_sfx.py` from `SC_UNITS`).
pub const UNIT_SFX_JSON: &str = include_str!("../data/unit_sfx.json");

/// `Unit::SoundEffectInfo` (`unit.hpp:64-70`) for one `SfxType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSfxInfo {
    /// `MAX.RES` resource tag (`TANK14` → `tank14.spw`).
    pub tag: &'static str,
    /// Volume 0..100 from `SC_UNITS` (`SoundManager::PlaySfx`: `volume / 100`).
    pub volume: u8,
    /// `is_default` C++: the unit did not define its own sound and played
    /// `default_sound_effects` (`units.cpp:288`).
    pub is_default: bool,
}

/// A combat sound ready to play: the resource tag and gain 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombatSfx {
    /// Resource tag (`AudioPlayer::play_gain`).
    pub tag: &'static str,
    /// Volume from the unit table (`volume / 100`).
    pub gain: f32,
    /// `is_default` (diagnostics/tests).
    pub is_default: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct UnitSfxJson {
    resource: String,
    volume: u8,
    #[serde(default)]
    default: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct UnitSfxRecord {
    token: String,
    fire: Option<UnitSfxJson>,
    hit: Option<UnitSfxJson>,
    expload: Option<UnitSfxJson>,
}

#[derive(Debug, Deserialize)]
struct UnitSfxFile {
    units: Vec<UnitSfxRecord>,
}

/// Parsed table (index — [`UnitType`] as `usize`).
fn unit_sfx_table() -> &'static [UnitSfxRecord] {
    static TABLE: OnceLock<Vec<UnitSfxRecord>> = OnceLock::new();

    TABLE
        .get_or_init(|| {
            serde_json::from_str::<UnitSfxFile>(UNIT_SFX_JSON)
                .map(|file| file.units)
                .unwrap_or_default()
        })
        .as_slice()
}

/// `Unit::GetSoundEffect(SfxType)` (`unit.hpp`, `units.cpp`): the resource and volume
/// of the unit sound as chosen by `ParseSoundEffects` (`units.cpp:228-290`).
///
/// `None` — the type is outside the table or `INVALID_ID` (no sound).
pub fn unit_sound(unit_type: UnitType, kind: UnitSoundKind) -> Option<UnitSfxInfo> {
    let record = unit_sfx_table().get(unit_type as usize)?;

    let entry = match kind {
        UnitSoundKind::Fire => record.fire.as_ref(),
        UnitSoundKind::Hit => record.hit.as_ref(),
        UnitSoundKind::Explode => record.expload.as_ref(),
    }?;

    Some(UnitSfxInfo {
        tag: entry.resource.as_str(),
        volume: entry.volume,
        is_default: entry.default,
    })
}

/// Simulation event → combat sound (`SoundManager::PlaySfx`).
///
/// Volume — `volume / 100` from `SC_UNITS` (`sound_manager.cpp:479-482`:
/// `const float volume = sfx_info.volume / 100.0f`). Distance attenuation and
/// panning in C++ (`GetDistanceAttenuation`/`GetPanning`) are not
/// modeled in the port: there is one camera, and the effect volume is already set by the `FX` setting.
pub fn combat_sound(event: UnitSound) -> Option<CombatSfx> {
    let info = unit_sound(event.unit_type, event.kind)?;

    Some(CombatSfx {
        tag: info.tag,
        gain: f32::from(info.volume) / 100.0,
        is_default: info.is_default,
    })
}

/// Size of the sound table (diagnostics/tests): must be [`UNIT_END`].
pub fn unit_sfx_count() -> usize {
    let count = unit_sfx_table().len();

    debug_assert!(
        count == 0 || count == UNIT_END as usize,
        "unit_sfx.json: {} записей вместо {}",
        count,
        UNIT_END
    );

    count
}

/// C++ type token (`TANK`, …) in the sound table; `""` — no record.
pub fn unit_sfx_token(unit_type: UnitType) -> &'static str {
    unit_sfx_table()
        .get(unit_type as usize)
        .map_or("", |record| record.token.as_str())
}

/// Status voice of the selected unit — `GameManager_PlayUnitStatusVoice`
/// (`game_manager.cpp:7943-8036`).
///
/// Returns `(V_START, status)`: C++ first plays `V_START`/`V_START2`,
/// then — the voice by the unit state. The conditions are repeated in C++ order.
pub fn unit_status_voice(unit: &max_core::unit::UnitInfo) -> (&'static str, Option<&'static str>) {
    use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
    use max_core::unit_values::{ATTRIB_AMMO, ATTRIB_HITS};

    let ammo_base = unit.base_values.get_attribute(ATTRIB_AMMO);
    let hits_base = unit.base_values.get_attribute(ATTRIB_HITS);

    let ammo_depleted = ammo_base != 0 && unit.ammo == 0;
    let ammo_low = ammo_base != 0 && i32::from(unit.ammo) <= ammo_base / 4;
    // Instance class: C++ checks the bits `MOBILE_AIR|MOBILE_SEA|MOBILE_LAND`
    // (= the domain is defined) and `STATIONARY` (= `!is_mobile()`).
    let mobile = unit.environment().is_some();
    let movement_exhausted = mobile && unit.speed == 0;
    let critical = (mobile || !unit.is_mobile()) && i32::from(unit.hits) <= hits_base / 4;
    let damaged = (mobile || !unit.is_mobile()) && i32::from(unit.hits) <= hits_base / 2;

    let status = if ammo_depleted {
        Some("V_M142")
    } else if ammo_low {
        Some("V_M138")
    } else if movement_exhausted {
        Some("V_M145")
    } else if critical {
        Some("V_M154")
    } else if damaged {
        Some("V_M150")
    } else if unit.unit_type == UnitType::Surveyor {
        Some("V_M191")
    } else if unit.orders == UnitOrder::MoveToAttack {
        Some("V_M196")
    } else if matches!(unit.orders, UnitOrder::Await | UnitOrder::Move) && unit.laying_state == 2 {
        Some("V_M181")
    } else if matches!(unit.orders, UnitOrder::Await | UnitOrder::Move) && unit.laying_state == 1 {
        Some("V_M186")
    } else if unit.orders == UnitOrder::Clear {
        Some("V_M171")
    } else if unit.orders == UnitOrder::Sentry && mobile {
        Some("V_M158")
    } else if unit.orders == UnitOrder::Build {
        match unit.state {
            UnitOrderState::BuildingReady => Some("V_M162"),
            UnitOrderState::UnitReady => Some("V_M166"),
            _ => Some("V_M284"),
        }
    } else if unit.unit_type == UnitType::ResearchCentre && unit.orders == UnitOrder::PowerOn {
        // C++ additionally checks `turns_to_complete == 0`; in the port the counter
        // lives in `GameState`, so V_M093 plays when a powered-on
        // Research Center is selected (see `App::select_unit`).
        Some("V_M093")
    } else if mobile {
        Some("V_M001")
    } else {
        None
    };

    ("V_START", status)
}

#[cfg(test)]
#[path = "sfx_tests.rs"]
mod tests;
