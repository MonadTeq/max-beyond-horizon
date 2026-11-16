//! Паспортные данные юнитов для экрана мобилизации (`CargoMenu`).
//!
//! Оракул: `cargomenu.cpp`/`abstractupgrademenu.cpp` —
//! `Unit::GetPortrait()` (`P_*` big image 300×240) и
//! `Unit::GetDescription()` (`language.json` по hex-ключу из `SC_UNITS`).
//! C++-порт не держит эти данные в коде: они читаются из ресурса
//! `SC_UNITS` (`assets/PATCHES.RES`, `Units::LoadResource`) и каталога
//! `assets/language.json` (`Language::GetEntry`; `0xffff` → пустая строка).
//!
//! Генератор `max-rust/tools/gen_cargo_unit_data.py` расшифровывает
//! `SC_UNITS` (XOR-таблица `ResourceManager_GenericTable`) и складывает
//! 93 записи в [`CARGO_UNIT_DATA_JSON`] (файл
//! `crates/max_app/data/cargo_unit_data.json`, коммитится). Таблица идёт в
//! порядке [`UnitType`] (ResourceID 0..92), поэтому запись типа — это
//! `units[unit as usize]`.
//!
//! Тексты — `en-US`, как и прочие подписи клиента (`assets/language.json`,
//! локаль по умолчанию). 24 типа без описания в оригинале — обломки,
//! взрывы, ракеты, `HARVSTER`, `WALDO` — в `CargoMenu` недостижимы
//! (`IsUnitFiltered`: нет `UPGRADABLE`/не строятся), т.е. у каждого
//! открываемого игроком типа описание непустое.

use std::sync::OnceLock;

use max_core::enums::{UnitType, UNIT_END};
use serde::Deserialize;

/// JSON-ресурс с 93 записями (сгенерирован из `SC_UNITS` + `language.json`).
pub const CARGO_UNIT_DATA_JSON: &str = include_str!("../data/cargo_unit_data.json");

/// Одна запись `SC_UNITS`: токен C++, портрет, кадры башни и описание (en-US).
#[derive(Debug, Clone, Deserialize)]
pub struct CargoUnitData {
    /// C++-токен ресурса (`TANK`, `LANDPLT`, …) — для сверки порядка.
    pub token: String,
    /// Ресурс портрета `P_*` (`Unit::GetPortrait`); `None` — `INVALID_ID`.
    pub portrait: Option<String>,
    /// Текст описания (`Unit::GetDescription`); пусто, если в оригинале нет.
    pub description: String,
    /// Ресурс `D_*` — `Unit::GetFrameInfo()`: базы кадров корпуса/башни,
    /// число кадров и `angle_offsets` для наложения башни.
    pub data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CargoUnitFile {
    units: Vec<CargoUnitData>,
}

/// Разобранные данные (индекс — [`UnitType`] как `usize`).
fn units() -> &'static [CargoUnitData] {
    static UNITS: OnceLock<Vec<CargoUnitData>> = OnceLock::new();

    UNITS
        .get_or_init(|| {
            serde_json::from_str::<CargoUnitFile>(CARGO_UNIT_DATA_JSON)
                .map(|file| file.units)
                .unwrap_or_default()
        })
        .as_slice()
}

/// Запись типа; `None`, если ресурс не разобрался или индекс вне таблицы.
pub fn unit_data(unit: UnitType) -> Option<&'static CargoUnitData> {
    units().get(unit as usize)
}

/// C++-токен типа (`TANK`, …); `""`, если данных нет.
pub fn unit_token(unit: UnitType) -> &'static str {
    unit_data(unit).map_or("", |data| data.token.as_str())
}

/// `Unit::GetDescription()` для `CargoMenu`; `""` — описания нет.
///
/// Соответствует `ResourceManager_GetLanguageEntry(0xffff)` → `std::string()`
/// и `Language::GetEntry` для hex-ключа из `SC_UNITS`.
pub fn unit_description(unit: UnitType) -> &'static str {
    unit_data(unit).map_or("", |data| data.description.as_str())
}

/// `Unit::GetPortrait()` — имя ресурса `P_*` (big image 300×240).
pub fn unit_portrait(unit: UnitType) -> Option<&'static str> {
    unit_data(unit).and_then(|data| data.portrait.as_deref())
}

/// `Unit::GetFrameInfo()` — имя ресурса `D_*` (кадры корпуса/башни,
/// `angle_offsets`); `None` — `INVALID_ID`.
pub fn unit_frame_data(unit: UnitType) -> Option<&'static str> {
    unit_data(unit).and_then(|data| data.data.as_deref())
}

/// Разобрались ли все [`UNIT_END`] записей (диагностика/тесты).
pub fn is_loaded() -> bool {
    units().len() == UNIT_END as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Все 93 типа присутствуют в ресурсе, порядок совпадает с `UnitType`.
    #[test]
    fn resource_carries_all_unit_types_in_enum_order() {
        assert!(is_loaded(), "JSON не разобрался: {} записей", units().len());

        for index in 0..UNIT_END {
            let unit = UnitType::try_from(index).expect("UnitType 0..92");
            let data = unit_data(unit).expect("запись типа");
            assert!(!data.token.is_empty(), "{unit:?}");
        }

        // C++-токены особых типов (сверка порядка `SC_UNITS`/`UnitType`).
        assert_eq!(unit_token(UnitType::CommTower), "COMMTWR");
        assert_eq!(unit_token(UnitType::HeavyVehiclePlant), "LANDPLT");
        assert_eq!(unit_token(UnitType::MineLayer), "MINELAYR");
        assert_eq!(unit_token(UnitType::DeadWaldo), "WALDO");
    }

    /// Портрет есть у всех, кроме 22 не-юнитов оригинала (`INVALID_ID`).
    #[test]
    fn portraits_match_sc_units() {
        let with_portrait = (0..UNIT_END)
            .map(|index| UnitType::try_from(index).unwrap())
            .filter(|unit| unit_portrait(*unit).is_some())
            .count();

        assert_eq!(with_portrait, 71, "SC_UNITS: 71 ресурс P_*");
        assert_eq!(unit_portrait(UnitType::Tank), Some("P_TANK"));
        assert_eq!(unit_portrait(UnitType::CommTower), Some("P_TRANSP"));
        assert_eq!(unit_portrait(UnitType::HeavyVehiclePlant), Some("P_HVYPLT"));
        assert_eq!(unit_portrait(UnitType::Constructor), Some("P_CONTRC"));
        // Не-юниты: `portrait` = `INVALID_ID` в `SC_UNITS`.
        for unit in [
            UnitType::Rocket,
            UnitType::Torpedo,
            UnitType::Harvester,
            UnitType::DeadWaldo,
            UnitType::LargeRubble,
        ] {
            assert_eq!(unit_portrait(unit), None, "{unit:?}");
        }
    }

    /// Описания: 69 непустых; каждый тип, достижимый в `CargoMenu`, описан.
    #[test]
    fn descriptions_cover_every_cargo_menu_unit() {
        let described = (0..UNIT_END)
            .map(|index| UnitType::try_from(index).unwrap())
            .filter(|unit| !unit_description(*unit).is_empty())
            .count();
        assert_eq!(described, 69, "69 описаний в language.json");

        // Тексты — дословно `language.json` (en-US).
        assert!(unit_description(UnitType::Tank).starts_with("Heavily armored fighting vehicle."));
        assert!(unit_description(UnitType::Scout)
            .starts_with("High speed scouting vehicle for exploration and spotting enemy units."));
        assert_eq!(
            unit_description(UnitType::Road),
            "Units move twice as fast over road squares."
        );

        // 24 типа без описания — обломки/взрывы/снаряды/`HARVSTER`/`WALDO`;
        // ни один не проходит `IsUnitFiltered` (проверяется в `mobilize`).
        for unit in [
            UnitType::Rocket,
            UnitType::Torpedo,
            UnitType::LandExplosion,
            UnitType::LargeRubble,
            UnitType::Harvester,
            UnitType::DeadWaldo,
            UnitType::SupportPlant,
            UnitType::RecreationCenter,
        ] {
            assert_eq!(unit_description(unit), "", "{unit:?}");
        }
    }

    /// `Unit::GetFrameInfo()`: у всех 93 типов есть ресурс `D_*`; у танка,
    /// мобильной зенитки и эскорта — отдельная башня.
    #[test]
    fn frame_data_covers_all_units() {
        let with_data = (0..UNIT_END)
            .map(|index| UnitType::try_from(index).unwrap())
            .filter(|unit| unit_frame_data(*unit).is_some())
            .count();
        assert_eq!(with_data, 93, "SC_UNITS: 93 ресурса D_*");

        assert_eq!(unit_frame_data(UnitType::Tank), Some("D_TANK"));
        assert_eq!(unit_frame_data(UnitType::MobileAntiAir), Some("D_SP_FLK"));
        assert_eq!(unit_frame_data(UnitType::FastBoat), Some("D_ESCORT"));
        assert_eq!(unit_frame_data(UnitType::GunTurret), Some("D_FIXED"));
        assert_eq!(unit_frame_data(UnitType::Scanner), Some("D_SCANNR"));
    }

    /// Таблица непустая даже при сбое JSON — но тогда `is_loaded()` ложен.
    #[test]
    fn json_resource_is_the_generated_file() {
        assert!(CARGO_UNIT_DATA_JSON.contains("\"COMMTWR\""));
        assert!(CARGO_UNIT_DATA_JSON.contains("\"P_CONTRC\""));
        assert!(CARGO_UNIT_DATA_JSON.contains("\"D_TANK\""));
        assert!(CARGO_UNIT_DATA_JSON.len() > 15_000);
    }
}
