//! User-Friendly Name (UFN) типов юнитов — слой представления.
//!
//! Решение владельца Ф0 п.4 (`max-brh-docs/development/PLAN-refactor.md` §3):
//! UFN-строки **не** входят в `max_core` — там только ресурсный токен
//! ([`UnitType::tag`]). Таблица UFN — данные приложения:
//! [`UNIT_NAMES_JSON`] (`crates/max_app/data/unit_names.json`), ключ — токен.
//!
//! Канон значений — `max-brh-docs/unit-name-review.md`, колонка
//! «User-Friendly Name» (английская строка, как в оригинальном мануале/игре).
//! Покрыты 70 юнитов/зданий/alien-типов; 23 не-юнита (эффекты, снаряды, следы,
//! обломки, декорации, `MASTER_BUILDER`, `HARVSTER`, `WALDO`) в интерфейсе игры
//! не отображаются — для них [`unit_user_friendly_name`] возвращает `tag()`.
//!
//! C++: `ResourceManager_GetUnit(...).GetSingularName()`/`GetPluralName()`
//! (`resourcetable.hpp`, ресурс `SC_UNITS`); строки — `assets/language.json`.

use std::collections::HashMap;
use std::sync::OnceLock;

use max_core::enums::UnitType;
use serde::Deserialize;

/// JSON-ресурс UFN (70 записей; генерируется/сверяется по канону вручную).
pub const UNIT_NAMES_JSON: &str = include_str!("../data/unit_names.json");

/// Содержимое [`UNIT_NAMES_JSON`]: метаданные (`$comment`/`$nonUnits`) и таблица
/// «токен → UFN».
#[derive(Debug, Default, Deserialize)]
struct UnitNamesFile {
    /// Токен [`UnitType::tag`] → User-Friendly Name.
    #[serde(default)]
    units: HashMap<String, String>,
}

/// Разобранная таблица (единожды; ошибка разбора — пустая таблица).
fn unit_names() -> &'static UnitNamesFile {
    static FILE: OnceLock<UnitNamesFile> = OnceLock::new();

    FILE.get_or_init(|| serde_json::from_str::<UnitNamesFile>(UNIT_NAMES_JSON).unwrap_or_default())
}

/// User-Friendly Name типа — строка для интерфейса игры.
///
/// `GoldRefinery` → `"Gold Refinery"`, `MiningStation` → `"Mining Station"`.
/// Для 23 не-юнитов (эффекты/декорации) записи нет — возвращается
/// [`UnitType::tag`] (`"LNDEXPLD"`, `"WALDO"`, …), чтобы функция была тотальной
/// и вызов из отладочного UI никогда не показывал пустую строку.
pub fn unit_user_friendly_name(unit: UnitType) -> &'static str {
    unit_names()
        .units
        .get(unit.tag())
        .map_or_else(|| unit.tag(), |name| name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 23 не-юнита: эффекты, снаряды/следы, обломки/декорации, служебные типы.
    /// Мануал их не описывает, в интерфейсе они не отображаются (UFN нет).
    const NON_UNITS: [UnitType; 23] = [
        UnitType::LargeRubble,
        UnitType::SmallRubble,
        UnitType::LargeTape,
        UnitType::SmallTape,
        UnitType::LargeSlab,
        UnitType::SmallSlab,
        UnitType::LargeCones,
        UnitType::SmallCones,
        UnitType::LandExplosion,
        UnitType::AirExplosion,
        UnitType::SeaExplosion,
        UnitType::BuildingExplosion,
        UnitType::HitExplosion,
        UnitType::MasterBuilder,
        UnitType::Rocket,
        UnitType::Torpedo,
        UnitType::AlienMissile,
        UnitType::AlienTankBall,
        UnitType::AlienArtilleryBall,
        UnitType::RocketSmoke,
        UnitType::TorpedoBubble,
        UnitType::Harvester,
        UnitType::DeadWaldo,
    ];

    #[test]
    fn table_matches_canon() {
        assert_eq!(UnitType::GoldRefinery.tag(), "COMMTWR");
        assert_eq!(
            unit_user_friendly_name(UnitType::GoldRefinery),
            "Gold Refinery"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::MaterialStorage),
            "Material Storage"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::FuelStorage),
            "Fuel Storage"
        );
        assert_eq!(unit_user_friendly_name(UnitType::GoldVault), "Gold Vault");
        assert_eq!(
            unit_user_friendly_name(UnitType::ResearchCentre),
            "Research Centre"
        );
        assert_eq!(unit_user_friendly_name(UnitType::EcoSphere), "Eco-Sphere");
        assert_eq!(
            unit_user_friendly_name(UnitType::MissileLauncher),
            "Missile Launcher"
        );
        assert_eq!(unit_user_friendly_name(UnitType::Artillery), "Artillery");
        assert_eq!(
            unit_user_friendly_name(UnitType::ConcreteBlock),
            "Concrete Block"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::ArmouredPersonnelCarrier),
            "Armoured Personnel Carrier"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::Infiltrator),
            "Infiltrator"
        );
        assert_eq!(unit_user_friendly_name(UnitType::Escort), "Escort");
        assert_eq!(unit_user_friendly_name(UnitType::Gunboat), "Gunboat");
        assert_eq!(
            unit_user_friendly_name(UnitType::MissileCruiser),
            "Missile Cruiser"
        );
        assert_eq!(
            unit_user_friendly_name(UnitType::Bomber),
            "Ground Attack Plane"
        );
        assert_eq!(unit_user_friendly_name(UnitType::Awac), "AWAC");
        assert_eq!(
            unit_user_friendly_name(UnitType::SeaTransport),
            "Sea Transport"
        );
        assert_eq!(unit_user_friendly_name(UnitType::CargoShip), "Cargo Ship");
    }

    #[test]
    fn table_covers_70_units_and_23_non_units() {
        let table = unit_names();
        assert_eq!(table.units.len(), 70);
        assert_eq!(NON_UNITS.len() + table.units.len(), UnitType::COUNT);

        // Каждый ключ — токен существующего типа (без «висячих» записей).
        for token in table.units.keys() {
            assert!(
                UnitType::ALL.iter().any(|unit_type| unit_type.tag() == token),
                "ключ «{token}» не соответствует ни одному UnitType::tag()"
            );
        }

        // Юниты: запись есть, UFN непустая (`Awac` — исключение по канону:
        // его User-Friendly Name «AWAC» совпадает с токеном `AWAC`).
        for unit_type in UnitType::ALL {
            if NON_UNITS.contains(unit_type) {
                continue;
            }
            assert!(
                table.units.contains_key(unit_type.tag()),
                "{unit_type:?}: нет записи UFN (токен {})",
                unit_type.tag()
            );
            let name = unit_user_friendly_name(*unit_type);
            assert!(!name.is_empty(), "{unit_type:?}: пустая UFN");
        }

        // Не-юниты: записи нет, функция отдаёт tag().
        for unit_type in NON_UNITS {
            assert!(
                !table.units.contains_key(unit_type.tag()),
                "{unit_type:?}: у эффекта/декорации не должно быть UFN"
            );
            assert_eq!(unit_user_friendly_name(unit_type), unit_type.tag());
        }
    }
}
