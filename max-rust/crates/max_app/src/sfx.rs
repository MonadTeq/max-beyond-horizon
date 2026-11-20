//! Звуки UI — порт `SoundManager::PlaySfx` вызовов C++-порта.
//!
//! Задача модуля — дать **одно** место, где событие интерфейса превращается в
//! имя звукового ресурса `MAX.RES`. Проигрывает их [`crate::audio::AudioPlayer`]
//! (в headless/CI — тихий `NoopBackend`), а `App` лишь копит очередь событий:
//! так тесты проверяют «какой звук по какому событию» без аудиоустройства.
//!
//! # Таблица (оракул — C++-порт)
//!
//! | Событие | Ресурс | Где в C++ |
//! |---|---|---|
//! | клик по карте/пункту попапа (клавишей)/элементу помощи/строкам меню | `KCARG0` | `game_manager.cpp:6711`, `:2689`; `helpmenu.cpp:331`; `gameconfigmenu.hpp` |
//! | `Done`/подтверждение | `NDONE0` | `game_manager.cpp:6727`; `helpmenu.cpp:91`; `menu.cpp:2182+` (кнопки `MBUTT0`, `Exit` — `NCANC0`) |
//! | `Cancel`/`Esc`/отказ | `NCANC0` | `units_manager.cpp:278`; `game_manager.cpp:4612`; `menu.cpp:2188` |
//! | кнопки панелей/главного меню | `MBUTT0` | `game_manager.cpp:5446/5692/5713`; `menu.cpp:2182` |
//! | кнопка `Help` экранов настройки | `NHELP0` | `gamesetupmenu.hpp:77`, `networkmenu.hpp` |
//! | `Human`/`Computer`/`None`/`Clan` игрока | `NHUMN0`/`NCOMP0`/`NNONE0`/`NCLAN0` | `chooseplayermenu.hpp:59-74` |
//! | `Arm` самоуничтожения | `MBUTT0` | `units_manager.cpp:312` |
//! | постановка площадки | `V_M049` | `game_manager.cpp:4569` (`PlayVoice`) |
//! | ремонт/дозаправка/передача груза | `V_M210`/`V_M085`/`V_M224` | `game_manager.cpp:4609/4626/4675` |
//!
//! Голоса (`V_*`) — тоже `.spw`-ресурсы, поэтому идут через тот же плеер; они
//! отключаются настройкой `disable_voice`, эффекты — `disable_fx`
//! (`Preferences`).

/// Звуковое событие UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sfx {
    /// `KCARG0` — клик по карте, строке меню, элементу помощи.
    Click,
    /// `NDONE0` — `Done`/`Enter`/подтверждение.
    Done,
    /// `NCANC0` — `Cancel`/`Esc`/недоступное действие.
    Cancel,
    /// `MBUTT0` — кнопка панели/главного меню.
    Button,
    /// `NHELP0` — кнопка `Help` экрана настройки.
    Help,
    /// `NHUMN0` — выбор «Human» в слоте игрока.
    PlayerHuman,
    /// `NCOMP0` — выбор «Computer».
    PlayerComputer,
    /// `NNONE0` — выбор «None».
    PlayerNone,
    /// `NCLAN0` — открытие выбора клана.
    PlayerClan,
    /// `V_M049` — постановка стройплощадки (`GameManager_PathBuild`).
    BuildSitePlaced,
    /// `V_M210` — ремонт (`GameManager_RepairUnit`).
    Repair,
    /// `V_M085` — дозаправка/дозарядка (`GameManager_ReloadUnit`).
    Reload,
    /// `V_M224` — передача груза (`GameManager_TransferCargo`).
    Transfer,
}

impl Sfx {
    /// Имя ресурса в `MAX.RES` (`AudioPlayer::play_once`).
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

    /// Это голос (`V_*`), а не SFX: управляется `disable_voice`.
    pub const fn is_voice(self) -> bool {
        matches!(
            self,
            Self::BuildSitePlaced | Self::Repair | Self::Reload | Self::Transfer
        )
    }
}

/// Голос статуса выделенного юнита — `GameManager_PlayUnitStatusVoice`
/// (`game_manager.cpp:7943-8036`).
///
/// Возвращает `(V_START, статус)`: C++ сначала играет `V_START`/`V_START2`,
/// затем — голос по состоянию юнита. Условия повторены в порядке C++.
pub fn unit_status_voice(unit: &max_core::unit::UnitInfo) -> (&'static str, Option<&'static str>) {
    use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
    use max_core::unit_values::{ATTRIB_AMMO, ATTRIB_HITS};

    let ammo_base = unit.base_values.get_attribute(ATTRIB_AMMO);
    let hits_base = unit.base_values.get_attribute(ATTRIB_HITS);

    let ammo_depleted = ammo_base != 0 && unit.ammo == 0;
    let ammo_low = ammo_base != 0 && i32::from(unit.ammo) <= ammo_base / 4;
    let movement_exhausted = unit.flags
        & (unit_flags::MOBILE_AIR_UNIT
            | unit_flags::MOBILE_SEA_UNIT
            | unit_flags::MOBILE_LAND_UNIT)
        != 0
        && unit.speed == 0;
    let critical = unit.flags
        & (unit_flags::MOBILE_AIR_UNIT
            | unit_flags::MOBILE_SEA_UNIT
            | unit_flags::MOBILE_LAND_UNIT
            | unit_flags::STATIONARY)
        != 0
        && i32::from(unit.hits) <= hits_base / 4;
    let damaged = unit.flags
        & (unit_flags::MOBILE_AIR_UNIT
            | unit_flags::MOBILE_SEA_UNIT
            | unit_flags::MOBILE_LAND_UNIT
            | unit_flags::STATIONARY)
        != 0
        && i32::from(unit.hits) <= hits_base / 2;

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
    } else if unit.orders == UnitOrder::Sentry
        && unit.flags
            & (unit_flags::MOBILE_AIR_UNIT
                | unit_flags::MOBILE_SEA_UNIT
                | unit_flags::MOBILE_LAND_UNIT)
            != 0
    {
        Some("V_M158")
    } else if unit.orders == UnitOrder::Build {
        match unit.state {
            UnitOrderState::BuildingReady => Some("V_M162"),
            UnitOrderState::UnitReady => Some("V_M166"),
            _ => Some("V_M284"),
        }
    } else if unit.unit_type == UnitType::ResearchCenter && unit.orders == UnitOrder::PowerOn {
        // C++ дополнительно проверяет `turns_to_complete == 0`; в порте счётчик
        // живёт в `GameState`, поэтому V_M093 играет при выборе включённого
        // Research Center (см. `App::select_unit`).
        Some("V_M093")
    } else if unit.flags
        & (unit_flags::MOBILE_AIR_UNIT | unit_flags::MOBILE_SEA_UNIT | unit_flags::MOBILE_LAND_UNIT)
        != 0
    {
        Some("V_M001")
    } else {
        None
    };

    ("V_START", status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_match_the_cpp_resource_names() {
        assert_eq!(Sfx::Click.tag(), "KCARG0");
        assert_eq!(Sfx::Done.tag(), "NDONE0");
        assert_eq!(Sfx::Cancel.tag(), "NCANC0");
        assert_eq!(Sfx::Button.tag(), "MBUTT0");
        assert_eq!(Sfx::Help.tag(), "NHELP0");
        assert_eq!(Sfx::BuildSitePlaced.tag(), "V_M049");
        assert!(Sfx::BuildSitePlaced.is_voice());
        assert!(!Sfx::Click.is_voice());
    }

    #[test]
    fn unit_status_voice_follows_cpp_order() {
        use max_core::enums::{UnitOrder, UnitType};
        use max_core::geo::Point;
        use max_core::team::TeamId;

        let mut unit =
            max_core::unit::UnitInfo::new(UnitType::Tank, TeamId::RED, max_core::unit::UnitId(1));
        unit.grid = Point::new(5, 5);
        unit.flags = max_core::access::unit_type_flags(UnitType::Tank);
        unit.base_values = max_core::unit_values::base_unit_values(UnitType::Tank);
        unit.hits = unit
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_HITS) as u16;
        unit.ammo = unit
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_AMMO) as u8;
        unit.speed = unit
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_SPEED) as u16;

        // Танк без повреждений в AWAIT — `V_M001`.
        unit.orders = UnitOrder::Await;
        assert_eq!(unit_status_voice(&unit), ("V_START", Some("V_M001")));

        // Без боезапаса — `V_M142` (первым в цепочке).
        unit.ammo = 0;
        assert_eq!(unit_status_voice(&unit).1, Some("V_M142"));

        // Разведчик — `V_M191`, если всё в порядке.
        let mut surveyor = max_core::unit::UnitInfo::new(
            UnitType::Surveyor,
            TeamId::RED,
            max_core::unit::UnitId(2),
        );
        surveyor.grid = Point::new(6, 6);
        surveyor.flags = max_core::access::unit_type_flags(UnitType::Surveyor);
        surveyor.base_values = max_core::unit_values::base_unit_values(UnitType::Surveyor);
        surveyor.hits = surveyor
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_HITS) as u16;
        surveyor.ammo = surveyor
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_AMMO) as u8;
        surveyor.speed = surveyor
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_SPEED) as u16;
        assert_eq!(unit_status_voice(&surveyor).1, Some("V_M191"));
    }
}
