//! Ассеты приложения: `MAX.RES`, `PATCHES.RES`, `COLOR.PAL` и спрайты юнитов.
//!
//! Модуль — единственное место `max_app`, которое знает о файлах игры. Он
//! собирает из них [`Assets`]:
//!
//! * [`ResArchive`] для `MAX.RES` и `PATCHES.RES` (карты, спрайты, данные);
//! * [`WrlMap`] планеты из `MAX.RES` (в архиве оригинальной игры лежат
//!   `SNOW_1`…`DESERT_6`, а не отдельные `.WRL`-файлы);
//! * [`Palette`] + [`BrightnessLut`] из `COLOR.PAL` (рендер [`max_render`]);
//! * [`SpriteSheet`] и [`ShadowSheet`] для типов юнитов из [`UNIT_SPRITES`].
//!
//! Ошибкой считается только отсутствие/битость обязательных файлов и карты
//! планеты. Отдельный спрайт, которого нет в архиве, — предупреждение в
//! [`Assets::warnings`]: клиент должен показывать карту и юнитов-маркеры даже на
//! неполном наборе данных.

use anyhow::{Context, Result};
use max_assets::{decode_shadow, decode_sprite, ResArchive, ShadowSheet, SpriteSheet, WrlMap};
use max_core::enums::{Planet, UnitType};
use max_render::palette::{BrightnessLut, Palette};

use crate::AppConfig;

/// Имена ресурсов `MAX.RES` для 24 планет — `SNOW_1 + world_index`
/// (`resource_manager.cpp:1183`, `ResourceID` 0x76E…0x785).
pub const WORLD_ASSETS: [&str; 24] = [
    "SNOW_1", "SNOW_2", "SNOW_3", "SNOW_4", "SNOW_5", "SNOW_6", "CRATER_1", "CRATER_2", "CRATER_3",
    "CRATER_4", "CRATER_5", "CRATER_6", "GREEN_1", "GREEN_2", "GREEN_3", "GREEN_4", "GREEN_5",
    "GREEN_6", "DESERT_1", "DESERT_2", "DESERT_3", "DESERT_4", "DESERT_5", "DESERT_6",
];

/// Имя `.WRL`-ресурса планеты (`settings["world"]`).
///
/// Порядок [`Planet`] совпадает с порядком ресурсов C++ (`SNOW_1 + index`),
/// поэтому это просто индексация таблицы.
pub fn world_asset_name(planet: Planet) -> &'static str {
    WORLD_ASSETS[planet as usize]
}

/// Какие типы юнитов грузятся из `MAX.RES` в [`Assets`].
///
/// Таблица покрывает юниты, которые чаще всего встречаются в сейвах и на
/// стартовой карте (техника, пехота, инженеры, воздух, здание-башня), плюс
/// пришельцы. Остальные типы рендерятся маркером команды (см.
/// [`crate::App::render_frame`]) — это осознанное упрощение R2.4.
///
/// Формат: `(тип, имя спрайта, имя тени)`. Тени в оригинале обрезаны до 8
/// символов (`S_` + первые 6 букв имени), поэтому имена заданы явно.
pub const UNIT_SPRITES: [(UnitType, &str, Option<&str>); 12] = [
    (UnitType::CommTower, "COMMTWR", Some("S_COMMTW")),
    (UnitType::Tank, "TANK", Some("S_TANK")),
    (UnitType::Infantry, "INFANTRY", Some("S_INFANT")),
    (UnitType::Scout, "SCOUT", Some("S_SCOUT")),
    (UnitType::Engineer, "ENGINEER", Some("S_ENGINE")),
    (UnitType::Constructor, "CONSTRCT", Some("S_CONSTR")),
    (UnitType::Bulldozer, "BULLDOZR", Some("S_BULLDO")),
    (UnitType::SupplyTruck, "SPLYTRCK", Some("S_SPLYTR")),
    (UnitType::GoldTruck, "GOLDTRCK", Some("S_GOLDTR")),
    (UnitType::Surveyor, "SURVEYOR", Some("S_SURVEY")),
    (UnitType::Fighter, "FIGHTER", Some("S_FIGHTE")),
    (UnitType::AlienTank, "ALNTANK", Some("S_ALNTAN")),
];

/// Загруженный спрайт одного типа юнита.
#[derive(Debug, Clone)]
pub struct LoadedSprite {
    /// Тип юнита (ключ поиска).
    pub unit_type: UnitType,
    /// Имя ресурса в `MAX.RES` (`TANK`, `INFANTRY`, …).
    pub sprite_name: String,
    /// Имя ресурса тени, если тень была в архиве и разобралась.
    pub shadow_name: Option<String>,
    /// Кадры обычного спрайта.
    pub sheet: SpriteSheet,
    /// Кадры тени (затемнение уже нарисованной карты).
    pub shadow: Option<ShadowSheet>,
}

impl LoadedSprite {
    /// Кадр для юнита: `image_base + angle` с обёрткой в `frame(0)`.
    ///
    /// Раскладка оригинала — «фаза × 8 + угол» (`docs/formats/spw.md` §5.3), а
    /// в V70-сейве `image_base`/`angle` уже хранятся по юниту. Индекс
    /// клампится по числу кадров, потому что у зданий/техники разное число
    /// фаз.
    pub fn frame_for(
        &self,
        image_base: i16,
        angle: u8,
    ) -> Option<&max_assets::sprite::SpriteFrame> {
        let base = usize::try_from(image_base.max(0)).unwrap_or(0);
        let index = base.saturating_add(usize::from(angle % 8));
        let last = self.sheet.len().checked_sub(1)?;

        self.sheet
            .frame(index.min(last))
            .or_else(|| self.sheet.frame(0))
    }

    /// Кадр тени, соответствующий [`Self::frame_for`] (по возможности).
    pub fn shadow_for(
        &self,
        image_base: i16,
        angle: u8,
    ) -> Option<&max_assets::sprite::ShadowFrame> {
        let shadow = self.shadow.as_ref()?;
        let last = shadow.len().checked_sub(1)?;
        let base = usize::try_from(image_base.max(0)).unwrap_or(0);
        let index = base.saturating_add(usize::from(angle % 8)).min(last);

        shadow.frame(index).or_else(|| shadow.frame(0))
    }
}

/// Краткая информация о загруженном спрайте (для отчёта CLI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteInfo {
    /// Тип юнита.
    pub unit_type: UnitType,
    /// Имя ресурса в `MAX.RES`.
    pub name: String,
    /// Число кадров.
    pub frames: usize,
    /// Число кадров тени (0 — тени нет).
    pub shadow_frames: usize,
}

/// Отчёт [`Assets::load`] — то, что печатает CLI и проверяют smoke-тесты.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetsReport {
    /// Число записей в `MAX.RES`.
    pub max_res_entries: usize,
    /// Число записей в `PATCHES.RES`.
    pub patches_res_entries: usize,
    /// Планета и её `.WRL`-ресурс.
    pub planet: Planet,
    /// Имя ресурса карты (`GREEN_1`, …).
    pub world: String,
    /// Откуда загружена карта: путь к `.WRL` или `MAX.RES:<имя>`.
    pub world_source: String,
    /// Размер карты в клетках.
    pub world_size: (i16, i16),
    /// Загруженные спрайты.
    pub sprites: Vec<SpriteInfo>,
    /// Некритичные проблемы (нет спрайта, битая тень, …).
    pub warnings: Vec<String>,
}

impl AssetsReport {
    /// Число загруженных спрайтов.
    pub fn sprite_count(&self) -> usize {
        self.sprites.len()
    }
}

/// Все данные игры, нужные приложению.
#[derive(Debug, Clone)]
pub struct Assets {
    /// Архив `MAX.RES`.
    pub max_res: ResArchive,
    /// Архив `PATCHES.RES` (правила миссий, `SC_*`).
    pub patches_res: ResArchive,
    /// Палитра `COLOR.PAL`.
    pub palette: Palette,
    /// Таблицы яркости, построенные из палитры.
    pub brightness: BrightnessLut,
    /// Планета, для которой загружена карта.
    pub planet: Planet,
    /// Имя `.WRL`-ресурса планеты.
    pub world_name: String,
    /// Откуда загружена карта (`путь` или `MAX.RES:<имя>`).
    pub world_source: String,
    /// Разобранная карта планеты.
    pub world: WrlMap,
    sprites: Vec<LoadedSprite>,
    warnings: Vec<String>,
}

impl Assets {
    /// Грузит обязательные данные и спрайты из [`UNIT_SPRITES`].
    ///
    /// # Ошибки
    ///
    /// `MAX.RES`/`PATCHES.RES`/`COLOR.PAL` не читаются или нет карты планеты
    /// (ни `.WRL`-файла, ни ресурса в архиве) — [`Err`] с контекстом
    /// (путь/имя ресурса). Отсутствующий спрайт — только запись в
    /// [`Self::warnings`].
    pub fn load(config: &AppConfig) -> Result<Self> {
        let max_res = ResArchive::open(&config.max_res)
            .with_context(|| format!("не удалось открыть {}", config.max_res.display()))?;
        let patches_res = ResArchive::open(&config.patches_res)
            .with_context(|| format!("не удалось открыть {}", config.patches_res.display()))?;
        let palette = Palette::load(&config.color_pal)
            .with_context(|| format!("не удалось прочитать {}", config.color_pal.display()))?;
        let brightness = BrightnessLut::build(&palette);

        let world_name = world_asset_name(config.planet);
        // Оригинальная игра держит `.WRL` отдельными файлами рядом с `MAX.RES`
        // (в архиве лежат только строки `snow_1.w`/`green_1.` — имена файлов).
        // Но некоторые сборки пакуют `.WRL` внутрь `MAX.RES`, поэтому порядок
        // такой: файл в `world_dir` → ресурс архива.
        let world_dir = config
            .world_dir
            .clone()
            .or_else(|| config.max_res.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_default();
        let world_file = world_dir.join(format!("{world_name}.WRL"));
        let (world, world_source) = if world_file.is_file() {
            let source = world_file.display().to_string();
            let world = WrlMap::open(&world_file)
                .with_context(|| format!("не удалось разобрать {source}"))?;
            (world, source)
        } else {
            let bytes = max_res.read(world_name).with_context(|| {
                format!(
                    "нет {world_name}.WRL в {} и нет ресурса {world_name} в {}",
                    world_dir.display(),
                    config.max_res.display()
                )
            })?;
            let world = WrlMap::parse(bytes)
                .with_context(|| format!("не удалось разобрать ресурс {world_name}"))?;
            (world, format!("{}:{world_name}", config.max_res.display()))
        };

        let mut warnings = Vec::new();
        let mut sprites = Vec::with_capacity(UNIT_SPRITES.len());

        for (unit_type, sprite_name, shadow_name) in UNIT_SPRITES {
            let Ok(bytes) = max_res.read(sprite_name) else {
                warnings.push(format!("нет спрайта {sprite_name} ({unit_type:?})"));
                continue;
            };
            let sheet = match decode_sprite(bytes) {
                Ok(sheet) => sheet,
                Err(error) => {
                    warnings.push(format!("спрайт {sprite_name} не разобрался: {error}"));
                    continue;
                }
            };

            let mut shadow = None;
            let mut loaded_shadow_name = None;

            if let Some(name) = shadow_name {
                match max_res.read(name).map_err(|error| error.to_string()) {
                    Ok(bytes) => match decode_shadow(bytes) {
                        Ok(decoded) => {
                            shadow = Some(decoded);
                            loaded_shadow_name = Some(name.to_string());
                        }
                        Err(error) => warnings.push(format!("тень {name} не разобралась: {error}")),
                    },
                    Err(error) => warnings.push(format!("нет тени {name}: {error}")),
                }
            }

            sprites.push(LoadedSprite {
                unit_type,
                sprite_name: sprite_name.to_string(),
                shadow_name: loaded_shadow_name,
                sheet,
                shadow,
            });
        }

        Ok(Self {
            max_res,
            patches_res,
            palette,
            brightness,
            planet: config.planet,
            world_name: world_name.to_string(),
            world_source,
            world,
            sprites,
            warnings,
        })
    }

    /// Спрайт для типа юнита (если он был в архиве и разобрался).
    pub fn sprite(&self, unit_type: UnitType) -> Option<&LoadedSprite> {
        self.sprites
            .iter()
            .find(|sprite| sprite.unit_type == unit_type)
    }

    /// Все загруженные спрайты в порядке [`UNIT_SPRITES`].
    pub fn sprites(&self) -> &[LoadedSprite] {
        &self.sprites
    }

    /// Некритичные проблемы загрузки.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Отчёт о загруженных данных (без повторного чтения файлов).
    pub fn report(&self) -> AssetsReport {
        AssetsReport {
            max_res_entries: self.max_res.len(),
            patches_res_entries: self.patches_res.len(),
            planet: self.planet,
            world: self.world_name.clone(),
            world_source: self.world_source.clone(),
            world_size: (self.world.width(), self.world.height()),
            sprites: self
                .sprites
                .iter()
                .map(|sprite| SpriteInfo {
                    unit_type: sprite.unit_type,
                    name: sprite.sprite_name.clone(),
                    frames: sprite.sheet.len(),
                    shadow_frames: sprite.shadow.as_ref().map_or(0, ShadowSheet::len),
                })
                .collect(),
            warnings: self.warnings.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::Planet;

    #[test]
    fn every_planet_maps_to_a_distinct_world_asset() {
        let mut names = Vec::new();

        for index in 0..WORLD_ASSETS.len() {
            let planet = Planet::try_from(index as u8).expect("planet id in range");
            let name = world_asset_name(planet);
            assert!(!name.is_empty());
            names.push(name);
        }

        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), WORLD_ASSETS.len());
        assert_eq!(world_asset_name(Planet::Snowcrab), "SNOW_1");
        assert_eq!(world_asset_name(Planet::Bottleneck), "DESERT_6");
    }

    #[test]
    fn sprite_table_covers_distinct_unit_types() {
        let mut types: Vec<u16> = UNIT_SPRITES
            .iter()
            .map(|(unit_type, _, _)| *unit_type as u16)
            .collect();
        types.sort_unstable();
        types.dedup();
        assert_eq!(types.len(), UNIT_SPRITES.len());
    }
}
