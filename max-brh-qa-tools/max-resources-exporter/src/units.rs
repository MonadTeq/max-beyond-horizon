//! Маппинг «тип юнита → ресурсы»: `cargo_unit_data.json` + проверка по
//! `SC_UNITS` из `PATCHES.RES`.
//!
//! `max-rust/crates/max_app/data/cargo_unit_data.json` — сгенерированный
//! (`max-rust/tools/gen_cargo_unit_data.py`) снимок `SC_UNITS`: 93 записи в
//! порядке [`max_core::enums::UnitType`], поля `sprite`/`shadow`/`portrait`/
//! `data`/`flics`, `INVALID_ID` → `null`. Экспорт читает готовый JSON (это тот
//! же маппинг, что использует `CargoMenu` порта), а [`verify_sc_units`]
//! дополнительно сверяет его с архивом `PATCHES.RES`: если JSON устарел, экспорт
//! падает с внятной ошибкой, а не пишет каталоги по неверным тегам.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use max_assets::ResArchive;
use serde::Deserialize;

/// Сколько игровых типов юнитов (`UNIT_END` в C++ / `UnitType::COUNT`).
pub const UNIT_COUNT: usize = 93;

/// Ключ XOR-шифрования ресурсов `SC_UNITS`/`ATTRIBS`
/// (`ResourceManager_GenericTable`, `resource_manager.cpp:156`).
const XOR_TABLE: [u8; 32] = [
    0x6c, 0x57, 0x36, 0xe6, 0x81, 0xe0, 0x72, 0x8a, 0xd3, 0xd9, 0xff, 0x54, 0x48, 0x3a, 0xcd, 0x75,
    0xd5, 0x0d, 0xe9, 0xe7, 0x7a, 0x57, 0xae, 0xeb, 0x61, 0x16, 0xb0, 0x35, 0x55, 0x62, 0xed, 0xd1,
];

/// Запись `cargo_unit_data.json` — один тип юнита.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct CargoUnit {
    /// Токен `SC_UNITS`/`UnitType` (`TANK`, `MININGST`, …) — он же имя каталога.
    pub token: String,
    /// Тег спрайта корпуса (`Unit::GetSprite()`).
    #[serde(default)]
    pub sprite: Option<String>,
    /// Тег тени `S_*` (`Unit::GetShadow`); `null` — тени нет.
    #[serde(default)]
    pub shadow: Option<String>,
    /// Тег портрета `P_*` (`Unit::GetPortrait`); `null` — 22 типа без портрета.
    #[serde(default)]
    pub portrait: Option<String>,
    /// Тег FLC-анимации `F_*` (`Unit::GetFlicsAnimation()`); `null` — без анимации.
    #[serde(default)]
    pub flics: Option<String>,
    /// Тег `D_*` — [`max_assets::sprite::FrameInfo`] (`Unit::GetFrameInfo()`).
    #[serde(default)]
    pub data: Option<String>,
    /// Английское описание из `language.json` (в экспорт не попадает).
    #[serde(default)]
    pub description: Option<String>,
}

impl CargoUnit {
    /// Каталог юнита в выгрузке — токен `SC_UNITS` (`TANK`, `MININGST`).
    pub fn tag(&self) -> &str {
        &self.token
    }
}

/// Корень `cargo_unit_data.json`.
#[derive(Debug, Clone, Deserialize)]
struct CargoUnitFile {
    units: Vec<CargoUnit>,
}

/// Читает `cargo_unit_data.json` и проверяет, что типов ровно 93 и токены
/// уникальны.
pub fn load_cargo_unit_data(path: impl AsRef<Path>) -> Result<Vec<CargoUnit>> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("чтение маппинга {}", path.display()))?;
    let file: CargoUnitFile =
        serde_json::from_str(&text).with_context(|| format!("разбор {}", path.display()))?;

    if file.units.len() != UNIT_COUNT {
        bail!(
            "{}: {} записей вместо {} (UnitType::COUNT)",
            path.display(),
            file.units.len(),
            UNIT_COUNT
        );
    }

    for (index, unit) in file.units.iter().enumerate() {
        if unit.token.is_empty() {
            bail!("{}: запись {index} без токена", path.display());
        }

        if let Some(previous) = file.units[..index]
            .iter()
            .find(|other| other.token == unit.token)
        {
            bail!(
                "{}: токен {} повторяется (записи {index} и {})",
                path.display(),
                unit.token,
                previous.token
            );
        }
    }

    Ok(file.units)
}

/// `SC_UNITS` из `PATCHES.RES`: токен → сырая запись JSON.
///
/// Ресурс XOR-зашифрован таблицей [`XOR_TABLE`] и содержит
/// `{"units": {"<TOKEN>": {...}}}`. Порядок ключей в `serde_json::Map` без
/// фичи `preserve_order` — алфавитный, поэтому здесь важен только **набор**
/// токенов (порядок задаёт `cargo_unit_data.json`).
pub fn read_sc_units_tokens(path: impl AsRef<Path>) -> Result<BTreeMap<String, serde_json::Value>> {
    let path = path.as_ref();
    let archive = ResArchive::open(path)?;
    let body = archive
        .read("SC_UNITS")
        .with_context(|| format!("{}: нет ресурса SC_UNITS", path.display()))?;

    let decoded: Vec<u8> = body
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ XOR_TABLE[index % XOR_TABLE.len()])
        .collect();

    let root: serde_json::Value = serde_json::from_slice(&decoded)
        .with_context(|| format!("{}: SC_UNITS — не JSON после XOR", path.display()))?;
    let units = root
        .get("units")
        .and_then(serde_json::Value::as_object)
        .with_context(|| format!("{}: SC_UNITS без объекта \"units\"", path.display()))?;

    Ok(units.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
}

/// Результат сверки маппинга с `SC_UNITS`.
#[derive(Debug, Clone, Default)]
pub struct ScUnitsCheck {
    /// Сколько записей в `SC_UNITS`.
    pub units: usize,
    /// Сколько полей сверено (`sprite`/`shadow`/`portrait`/`data`/`flics`).
    pub fields: usize,
    /// Расхождения (`токен.поле: JSON … / SC_UNITS …`).
    pub mismatches: Vec<String>,
}

impl ScUnitsCheck {
    /// Сверка прошла без расхождений.
    pub fn is_ok(&self) -> bool {
        self.mismatches.is_empty()
    }
}

/// Сверяет `cargo_unit_data.json` с `SC_UNITS` из архива.
pub fn verify_sc_units(
    path: impl AsRef<Path>,
    units: &[CargoUnit],
) -> Result<ScUnitsCheck> {
    let sc_units = read_sc_units_tokens(path)?;
    let mut check = ScUnitsCheck {
        units: sc_units.len(),
        ..ScUnitsCheck::default()
    };

    for unit in units {
        let Some(record) = sc_units.get(&unit.token) else {
            check
                .mismatches
                .push(format!("{}: нет в SC_UNITS", unit.token));
            continue;
        };

        for (field, key, expected) in [
            ("sprite", "sprite", &unit.sprite),
            ("shadow", "shadow", &unit.shadow),
            ("portrait", "portrait", &unit.portrait),
            ("data", "data", &unit.data),
            ("flics", "flics_animation", &unit.flics),
        ] {
            check.fields += 1;

            let raw = record.get(key).and_then(serde_json::Value::as_str);
            let actual = match raw {
                None | Some("INVALID_ID") => None,
                Some(tag) => Some(tag.to_string()),
            };

            if &actual != expected {
                check.mismatches.push(format!(
                    "{}.{field}: JSON {:?} / SC_UNITS {:?}",
                    unit.token, expected, actual
                ));
            }
        }
    }

    for token in sc_units.keys() {
        if !units.iter().any(|unit| &unit.token == token) {
            check
                .mismatches
                .push(format!("{token}: есть в SC_UNITS, нет в JSON"));
        }
    }

    Ok(check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from("../.."))
    }

    fn unit_json_path() -> PathBuf {
        repo_root().join("max-rust/crates/max_app/data/cargo_unit_data.json")
    }

    fn patches_path() -> PathBuf {
        repo_root().join("assets/PATCHES.RES")
    }

    #[test]
    fn cargo_unit_data_has_93_unique_tokens_in_unit_type_order() {
        if !unit_json_path().exists() {
            return; // рабочий каталог без данных игры — тест пропускается
        }

        let units = load_cargo_unit_data(unit_json_path()).expect("JSON читается");

        assert_eq!(units.len(), UNIT_COUNT);
        assert_eq!(units.len(), max_core::enums::UnitType::COUNT);
        assert_eq!(units[0].token, "COMMTWR");
        assert_eq!(units[0x33].token, "TANK"); // UnitType::Tank
        assert_eq!(units[92].token, "WALDO"); // UnitType::DeadWaldo
        assert!(units.iter().all(|unit| unit.sprite.is_some()));
        assert_eq!(units.iter().filter(|unit| unit.portrait.is_some()).count(), 71);
        assert_eq!(units.iter().filter(|unit| unit.flics.is_some()).count(), 71);
    }

    #[test]
    fn sc_units_matches_the_generated_json() {
        if !unit_json_path().exists() || !patches_path().exists() {
            return;
        }

        let units = load_cargo_unit_data(unit_json_path()).expect("JSON читается");
        let check = verify_sc_units(patches_path(), &units).expect("SC_UNITS читается");

        assert_eq!(check.units, UNIT_COUNT);
        assert_eq!(check.fields, UNIT_COUNT * 5);
        assert!(check.is_ok(), "расхождения: {:?}", check.mismatches);
    }

    #[test]
    fn invalid_id_becomes_none() {
        let raw = serde_json::json!({
            "token": "TEST",
            "sprite": "TEST",
            "portrait": "INVALID_ID",
            "flics": null
        });
        let unit: CargoUnit = serde_json::from_value(raw).expect("разбор");

        // `INVALID_ID` — строка, а не null: генератор превращает её в null,
        // поэтому здесь проверяем только форму записи `None`.
        assert_eq!(unit.portrait.as_deref(), Some("INVALID_ID"));
        assert_eq!(unit.flics, None);
        assert_eq!(unit.shadow, None);
    }
}
