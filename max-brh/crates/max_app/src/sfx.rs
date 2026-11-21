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
//!
//! # Звуки боя (`SC_UNITS`)
//!
//! Выстрел, попадание и взрыв берут ресурс **из таблицы юнита**:
//! `SoundManager::PlaySfx(UnitInfo*, Unit::SfxType)` (`sound_manager.cpp:413-490`)
//! ищет `Unit::SoundEffectInfo` в
//! `Unit::m_sound_effects` (`units.cpp` `ParseSoundEffects`, `units.cpp:228-290`):
//! сначала `units[TOKEN].sound_effects`, иначе `default_sound_effects`. Таблица
//! сгенерирована в `data/unit_sfx.json` (`tools/gen_unit_sfx.py`) и читается
//! [`unit_sound`]; события копит симуляция (`max_core::UnitSound`), а
//! [`combat_sound`] превращает их в `(тег, громкость)`.
//!
//! Вызовы C++:
//! * выстрел — `UnitInfo::PrepareFire` (`unitinfo.cpp:4725`), `SFX_TYPE_FIRE`;
//! * попадание — `UnitsManager_StartExplosion` при `hits > 0`
//!   (`units_manager.cpp:3922`), `SFX_TYPE_HIT`;
//! * взрыв — там же при `hits == 0` (`units_manager.cpp:3929`),
//!   `SFX_TYPE_EXPLOAD` (у `LNDEXPLD`/`AIREXPLD`/`SEAEXPLD`/`BLDEXPLD`/
//!   `HITEXPLD` своего звука нет — играет `GEN_XPLD`, как в C++).

use std::sync::OnceLock;

use max_core::enums::{UnitType, UNIT_END};
use max_core::game::{UnitSound, UnitSoundKind};
use serde::Deserialize;

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

// ---------------------------------------------------------------------------
// Звуки юнитов (боевые) — SC_UNITS
// ---------------------------------------------------------------------------

/// JSON-таблица звуков (`tools/gen_unit_sfx.py` из `SC_UNITS`).
pub const UNIT_SFX_JSON: &str = include_str!("../data/unit_sfx.json");

/// `Unit::SoundEffectInfo` (`unit.hpp:64-70`) для одного `SfxType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSfxInfo {
    /// Тег ресурса `MAX.RES` (`TANK14` → `tank14.spw`).
    pub tag: &'static str,
    /// Громкость 0..100 из `SC_UNITS` (`SoundManager::PlaySfx`: `volume / 100`).
    pub volume: u8,
    /// `is_default` C++: юнит своего звука не задавал и сыграл
    /// `default_sound_effects` (`units.cpp:288`).
    pub is_default: bool,
}

/// Готовый к проигрыванию звук боя: тег ресурса и усиление 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombatSfx {
    /// Тег ресурса (`AudioPlayer::play_gain`).
    pub tag: &'static str,
    /// Громкость из таблицы юнита (`volume / 100`).
    pub gain: f32,
    /// `is_default` (диагностика/тесты).
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

/// Разобранная таблица (индекс — [`UnitType`] как `usize`).
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

/// `Unit::GetSoundEffect(SfxType)` (`unit.hpp`, `units.cpp`): ресурс и громкость
/// звука юнита, как их выбрал `ParseSoundEffects` (`units.cpp:228-290`).
///
/// `None` — тип вне таблицы или `INVALID_ID` (звука нет).
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

/// Событие симуляции → звук боя (`SoundManager::PlaySfx`).
///
/// Громкость — `volume / 100` из `SC_UNITS` (`sound_manager.cpp:479-482`:
/// `const float volume = sfx_info.volume / 100.0f`). Затухание по расстоянию и
/// панорама C++ (`GetDistanceAttenuation`/`GetPanning`) в порте не
/// моделируются: камера одна, а громкость эффекта уже задана настройкой `FX`.
pub fn combat_sound(event: UnitSound) -> Option<CombatSfx> {
    let info = unit_sound(event.unit_type, event.kind)?;

    Some(CombatSfx {
        tag: info.tag,
        gain: f32::from(info.volume) / 100.0,
        is_default: info.is_default,
    })
}

/// Размер таблицы звуков (диагностика/тесты): должно быть [`UNIT_END`].
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

/// C++-токен типа (`TANK`, …) в таблице звуков; `""` — записи нет.
pub fn unit_sfx_token(unit_type: UnitType) -> &'static str {
    unit_sfx_table()
        .get(unit_type as usize)
        .map_or("", |record| record.token.as_str())
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
    } else if unit.unit_type == UnitType::ResearchCentre && unit.orders == UnitOrder::PowerOn {
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

    // -----------------------------------------------------------------------
    // Звуки боя: таблица `SC_UNITS` (`ParseSoundEffects`, units.cpp:228-290)
    // -----------------------------------------------------------------------

    #[test]
    fn unit_sfx_table_covers_all_types_in_cpp_order() {
        assert_eq!(
            unit_sfx_count(),
            UNIT_END as usize,
            "таблица должна покрывать все {} типов SC_UNITS",
            UNIT_END
        );
        assert_eq!(unit_sfx_token(UnitType::GoldRefinery), "COMMTWR");
        assert_eq!(unit_sfx_token(UnitType::Tank), "TANK");
        assert_eq!(unit_sfx_token(UnitType::AirUnitsPlant), "AIRPLT");
        assert_eq!(unit_sfx_token(UnitType::RocketSmoke), "RKTSMOKE");
    }

    #[test]
    fn unit_fire_hit_explosion_come_from_the_unit_tables() {
        // `TANK` задаёт свои звуки: fire = TANK14, hit = TANK15, expload = TANK16.
        let fire = unit_sound(UnitType::Tank, UnitSoundKind::Fire).expect("fire");
        assert_eq!(fire.tag, "TANK14");
        assert_eq!(fire.volume, 60);
        assert!(!fire.is_default);

        let hit = unit_sound(UnitType::Tank, UnitSoundKind::Hit).expect("hit");
        assert_eq!(hit.tag, "TANK15");
        assert!(!hit.is_default);

        let explosion = unit_sound(UnitType::Tank, UnitSoundKind::Explode).expect("expload");
        assert_eq!(explosion.tag, "TANK16");
        assert_eq!(explosion.volume, 80);
        assert!(!explosion.is_default);

        // Пехота (`INFANTRY`) — `INFAN14/15/16`.
        assert_eq!(
            unit_sound(UnitType::Infantry, UnitSoundKind::Fire)
                .expect("infantry fire")
                .tag,
            "INFAN14"
        );

        // `FIGHTER` — `FIGHT14/15/16` (авиация).
        assert_eq!(
            unit_sound(UnitType::Fighter, UnitSoundKind::Explode)
                .expect("fighter expload")
                .tag,
            "FIGHT16"
        );
    }

    #[test]
    fn units_without_own_sound_fall_back_to_default_table() {
        // У `LNDEXPLD`/`AIREXPLD`/`SEAEXPLD`/`BLDEXPLD` своего `sound_effects`
        // нет — `ParseSoundEffects` берёт `default_sound_effects` и помечает
        // `is_default` (`units.cpp:288`). `GEN_XPLD` — громкость 80.
        for unit_type in [
            UnitType::LandExplosion,
            UnitType::AirExplosion,
            UnitType::SeaExplosion,
            UnitType::BuildingExplosion,
        ] {
            let explosion = unit_sound(unit_type, UnitSoundKind::Explode)
                .unwrap_or_else(|| panic!("{unit_type:?}: default expload"));
            assert_eq!(explosion.tag, "GEN_XPLD", "{unit_type:?}");
            assert_eq!(explosion.volume, 80);
            assert!(explosion.is_default, "{unit_type:?}: звук по умолчанию");
        }

        // Ракета/след тоже без своих звуков — дефолтные.
        assert_eq!(
            unit_sound(UnitType::Rocket, UnitSoundKind::Fire)
                .expect("rocket fire")
                .tag,
            "GEN_FIRE"
        );
    }

    #[test]
    fn combat_sound_maps_events_to_tags_and_volume() {
        let event = UnitSound {
            unit: max_core::unit::UnitId(7),
            unit_type: UnitType::Tank,
            kind: UnitSoundKind::Fire,
        };
        let sound = combat_sound(event).expect("tank fire");
        assert_eq!(sound.tag, "TANK14");
        assert!((sound.gain - 0.6).abs() < 1e-6, "volume 60 → gain 0.6");

        let explosion = combat_sound(UnitSound {
            unit: max_core::unit::UnitId(7),
            unit_type: UnitType::LandExplosion,
            kind: UnitSoundKind::Explode,
        })
        .expect("explosion");
        assert_eq!(explosion.tag, "GEN_XPLD");
        assert!((explosion.gain - 0.8).abs() < 1e-6);
        assert!(explosion.is_default);
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
