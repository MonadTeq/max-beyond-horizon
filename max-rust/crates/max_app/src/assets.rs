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
use max_assets::{
    decode_shadow, decode_sprite, BigImage, CursorSet, ResArchive, ResourceMarkers, ShadowSheet,
    SpriteSheet, WrlMap,
};
use max_assets::sprite::{FrameInfo, ShadowFrame, SpriteFrame};
use max_core::enums::{Planet, UnitType};
use max_render::palette::{BrightnessLut, Palette};

use std::path::PathBuf;

use crate::AppConfig;

/// Имена ресурсов `MAX.RES` для 24 планет — `SNOW_1 + world_index`
/// (`resource_manager.cpp:1183`, `ResourceID` 0x76E…0x785).
pub const WORLD_ASSETS: [&str; 24] = [
    "SNOW_1", "SNOW_2", "SNOW_3", "SNOW_4", "SNOW_5", "SNOW_6", "CRATER_1", "CRATER_2", "CRATER_3",
    "CRATER_4", "CRATER_5", "CRATER_6", "GREEN_1", "GREEN_2", "GREEN_3", "GREEN_4", "GREEN_5",
    "GREEN_6", "DESERT_1", "DESERT_2", "DESERT_3", "DESERT_4", "DESERT_5", "DESERT_6",
];

/// Картинки планет по группам из 6 (`SNOW_PIC + world/6`,
/// `planetselectmenu.cpp:94`, `resourcetable.hpp:1925-1928`).
pub const PLANET_GROUP_PICTURES: [&str; 4] = ["SNOW_PIC", "CRTR_PIC", "GREN_PIC", "DSRT_PIC"];

/// Имя `.WRL`-ресурса планеты (`settings["world"]`).
///
/// Порядок [`Planet`] совпадает с порядком ресурсов C++ (`SNOW_1 + index`),
/// поэтому это просто индексация таблицы.
pub fn world_asset_name(planet: Planet) -> &'static str {
    WORLD_ASSETS[planet as usize]
}

/// Где искать `PATCHES.RES`, если [`AppConfig::patches_res`] не существует.
///
/// Пути относительны: корня репозитория, текущего каталога и каталога
/// [`AppConfig::max_res`]. Первый — шаблон headless-сервера, из которого
/// запускается живой smoke; второй — обычный `assets/` репозитория.
pub const PATCHES_RES_FALLBACKS: [&str; 2] = [
    "build/linux-server-template/PATCHES.RES",
    "assets/PATCHES.RES",
];

/// Имя каталога данных игры в корне поставки (`max-original/`).
pub const DATA_DIR_NAME: &str = "max-original";
/// Имя основного архива данных.
pub const MAX_RES_NAME: &str = "MAX.RES";
/// Имя палитры.
pub const COLOR_PAL_NAME: &str = "COLOR.PAL";

/// Корни поиска данных игры в порядке приоритета (дефект **TC-1109**).
///
/// Порядок:
/// 1. каталог [`AppConfig::max_res`] и его родитель (данные рядом с конфигом);
/// 2. текущий каталог и его родитель (историческое поведение, запуск из `max-rust/`);
/// 3. каталог запущенного **exe** ([`std::env::current_exe`]) и вверх по дереву
///    до ближайшего каталога с `max-original` включительно.
///
/// Пункт 3 — фикс TC-1109: `release\max_app.exe` (или `target\debug\…` в
/// checkout'е) находит данные, даже если CWD — чужой каталог.
pub fn data_search_roots(config: &AppConfig) -> Vec<PathBuf> {
    let cwd = std::env::current_dir().ok();
    let exe = std::env::current_exe().ok();

    data_search_roots_from(config, cwd.as_deref(), exe.as_deref())
}

/// [`data_search_roots`] с явными CWD/exe — для тестов с временными каталогами.
fn data_search_roots_from(
    config: &AppConfig,
    cwd: Option<&std::path::Path>,
    exe: Option<&std::path::Path>,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    let push = |path: PathBuf, roots: &mut Vec<PathBuf>| {
        if !roots.iter().any(|existing| *existing == path) {
            roots.push(path);
        }
    };

    if let Some(dir) = config.max_res.parent() {
        push(dir.to_path_buf(), &mut roots);
        if let Some(parent) = dir.parent() {
            push(parent.to_path_buf(), &mut roots);
        }
    }

    if let Some(cwd) = cwd {
        push(cwd.to_path_buf(), &mut roots);
        if let Some(parent) = cwd.parent() {
            push(parent.to_path_buf(), &mut roots);
        }
    }

    if let Some(exe) = exe {
        for root in exe_search_roots(exe) {
            push(root, &mut roots);
        }
    }

    roots
}

/// Каталог exe и вверх до ближайшего каталога с `max-original` (включительно).
///
/// Если `max-original` рядом нет, остаётся только сам каталог exe: подниматься
/// выше без признака данных опасно — этим можно случайно найти `assets/`
/// чужого репозитория (см. тест `absent_everywhere_keeps_the_configured_path`).
fn exe_search_roots(exe: &std::path::Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Some(dir) = exe.parent() else {
        return roots;
    };

    roots.push(dir.to_path_buf());

    for ancestor in dir.ancestors().skip(1) {
        if ancestor.join(DATA_DIR_NAME).is_dir() {
            roots.push(ancestor.to_path_buf());
            break;
        }
    }

    roots
}

/// Находит `MAX.RES` для **загрузки ассетов**, если путь конфига отсутствует.
///
/// Дефект TC-1109: относительный `max-original/MAX.RES` не резолвился при
/// запуске из другого каталога, хотя данные лежат рядом с exe. Возвращает
/// выбранный путь и предупреждение (путь конфига при этом не меняется).
pub fn resolve_max_res(config: &AppConfig) -> (PathBuf, Option<String>) {
    let configured = &config.max_res;

    if configured.is_file() {
        return (configured.clone(), None);
    }

    resolve_max_res_at(config, &data_search_roots(config))
}

/// [`resolve_max_res`] по заданным корням поиска (для тестов).
fn resolve_max_res_at(config: &AppConfig, roots: &[PathBuf]) -> (PathBuf, Option<String>) {
    let configured = &config.max_res;

    if configured.is_file() {
        return (configured.clone(), None);
    }

    for root in roots {
        for candidate in [
            root.join(DATA_DIR_NAME).join(MAX_RES_NAME),
            root.join(MAX_RES_NAME),
        ] {
            if candidate.is_file() {
                let warning = format!(
                    "{} не найден; MAX.RES грузится из fallback {} (проверьте --data-dir/--max-res)",
                    configured.display(),
                    candidate.display()
                );

                return (candidate, Some(warning));
            }
        }
    }

    (configured.clone(), None)
}

/// Находит `PATCHES.RES` для **загрузки ассетов** (SC_*/таблицы, спрайты).
///
/// Хэндшейк (`data_hash`) этот путь не использует: `net.rs` берёт
/// [`AppConfig::handshake_patches_path`], поэтому «серверный» пустой
/// `PATCHES.RES` и реальные ассеты могут расходиться. Возвращает выбранный
/// путь и предупреждение, если сработал fallback.
pub fn resolve_patches_res(config: &AppConfig) -> (PathBuf, Option<String>) {
    resolve_patches_res_at(config, &data_search_roots(config))
}

/// [`resolve_patches_res`] по заданным корням поиска (для тестов).
fn resolve_patches_res_at(config: &AppConfig, roots: &[PathBuf]) -> (PathBuf, Option<String>) {
    let configured = &config.patches_res;

    if configured.is_file() {
        return (configured.clone(), None);
    }

    for base in roots {
        for relative in PATCHES_RES_FALLBACKS {
            let candidate = base.join(relative);

            if candidate.is_file() {
                let warning = format!(
                    "{} не найден; PATCHES.RES грузится из fallback {} (data_hash хендшейка не меняется)",
                    configured.display(),
                    candidate.display()
                );

                return (candidate, Some(warning));
            }
        }
    }

    (configured.clone(), None)
}

/// Находит `COLOR.PAL` для **загрузки ассетов**, если путь конфига отсутствует.
///
/// Палитра — часть тех же данных, что и `MAX.RES`; `--max-res` без
/// `--data-dir`/`--palette` (как в живом smoke) оставлял относительный путь
/// `max-original/COLOR.PAL` нерезолвимым, и мир не генерировался. Хэндшейк
/// `COLOR.PAL` не использует, поэтому подмена безопасна.
pub fn resolve_color_pal(config: &AppConfig) -> (PathBuf, Option<String>) {
    resolve_color_pal_at(config, &data_search_roots(config))
}

/// [`resolve_color_pal`] по заданным корням поиска (для тестов).
fn resolve_color_pal_at(config: &AppConfig, roots: &[PathBuf]) -> (PathBuf, Option<String>) {
    let configured = &config.color_pal;

    if configured.is_file() {
        return (configured.clone(), None);
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    for root in roots {
        candidates.push(root.join(COLOR_PAL_NAME));
        candidates.push(root.join(DATA_DIR_NAME).join(COLOR_PAL_NAME));
    }

    for candidate in candidates {
        if candidate.is_file() {
            let warning = format!(
                "{} не найден; COLOR.PAL грузится из fallback {}",
                configured.display(),
                candidate.display()
            );

            return (candidate, Some(warning));
        }
    }

    (configured.clone(), None)
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
///
/// Список покрывает не только подвижные юниты, но и **здания**: в M.A.X.
/// постройки — это тоже `UnitType` со спрайтом из `MAX.RES` (`HANGAR`,
/// `DEPOT`, `POWERSTN`, …). Без них карта выглядела пустой: здания рисовались
/// квадратным маркером команды вместо изометрии 1:1.
pub const UNIT_SPRITES: [(UnitType, &str, Option<&str>); 43] = [
    // --- здания (стационарные юниты, `UNIT_START..`) ----------------------
    (UnitType::CommTower, "COMMTWR", Some("S_COMMTW")),
    (UnitType::PowerStation, "POWERSTN", Some("S_POWERS")),
    (UnitType::PowerGenerator, "POWGEN", Some("S_POWGEN")),
    (UnitType::Barracks, "BARRACKS", Some("S_BARRAC")),
    (UnitType::ShieldGenerator, "SHIELDGN", Some("S_SHIELD")),
    (UnitType::Radar, "RADAR", Some("S_RADAR")),
    (UnitType::ADump, "ADUMP", Some("S_ADUMP")),
    (UnitType::FDump, "FDUMP", Some("S_FDUMP")),
    (UnitType::GoldSmelter, "GOLDSM", Some("S_GOLDSM")),
    (UnitType::Depot, "DEPOT", Some("S_DEPOT")),
    (UnitType::Hangar, "HANGAR", Some("S_HANGAR")),
    (UnitType::Dock, "DOCK", Some("S_DOCK")),
    (UnitType::Connector, "CNCT_4W", Some("S_CNCT4W")),
    (UnitType::Road, "ROAD", Some("S_ROAD")),
    (UnitType::LandingPad, "LANDPAD", Some("S_LANDPA")),
    (UnitType::Shipyard, "SHIPYARD", Some("S_SHIPYA")),
    (UnitType::LightVehiclePlant, "LIGHTPLT", Some("S_LIGHTP")),
    (UnitType::HeavyVehiclePlant, "LANDPLT", Some("S_LANDPL")),
    (UnitType::SupportPlant, "SUPRTPLT", Some("S_SUPRTP")),
    (UnitType::AirUnitsPlant, "AIRPLT", Some("S_AIRPLT")),
    (UnitType::Habitat, "HABITAT", Some("S_HABITA")),
    (UnitType::ResearchCenter, "RESEARCH", Some("S_RESEAR")),
    (UnitType::Greenhouse, "GREENHSE", Some("S_GREENH")),
    (UnitType::RecreationCenter, "RECCENTR", Some("S_RECCEN")),
    (UnitType::TrainingHall, "TRAINHAL", Some("S_TRAINH")),
    (UnitType::WaterPlatform, "WTRPLTFM", Some("S_WTRPLT")),
    (UnitType::GunTurret, "GUNTURRT", Some("S_GUNTUR")),
    (UnitType::AntiAir, "ANTIAIR", Some("S_ANTIAI")),
    // --- обломки alien-базы (щит/склад/развлечения, плита и мост) ---------
    (UnitType::LargeSlab, "LRGSLAB", Some("S_LRGSLA")),
    (UnitType::Bridge, "BRIDGE", Some("S_BRIDGE")),
    // --- подвижные юниты --------------------------------------------------
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
    (UnitType::AlienAssaultGun, "ALNASGUN", Some("S_ALNASG")),
    (UnitType::AlienPlane, "ALNPLANE", Some("S_ALNPLA")),
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
    /// `Unit::GetFrameInfo()` из ресурса `D_*`: база кадров башни и
    /// `angle_offsets` для её наложения (`None` — ресурса нет/не разобрался).
    pub frame_info: Option<FrameInfo>,
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

    /// Кадр башни/пушки — `turret_image_base + turret_angle`
    /// (`Unit::GetFrameInfo()`; `UnitInfo::RenderWithConnectors`).
    ///
    /// `None`, если у типа нет `D_*` или кадров башни.
    pub fn turret_frame(&self, turret_angle: u8) -> Option<&SpriteFrame> {
        let info = self.frame_info?;

        if info.turret_image_count == 0 {
            return None;
        }

        let index = info.turret_frame_index(turret_angle);

        self.sheet.frame(index)
    }

    /// Кадр тени башни (`ShadowSheet`, тот же индекс, что у спрайта).
    pub fn turret_shadow(&self, turret_angle: u8) -> Option<&ShadowFrame> {
        let info = self.frame_info?;
        let shadow = self.shadow.as_ref()?;

        if info.turret_image_count == 0 {
            return None;
        }

        let index = info.turret_frame_index(turret_angle);
        let last = shadow.len().checked_sub(1)?;

        shadow.frame(index.min(last))
    }

    /// Куда рисовать башню относительно якоря корпуса:
    /// `angle_offsets[body_angle]` (`UnitInfo::UpdateTurretAngle`).
    pub fn turret_offset(&self, body_angle: u8) -> (i32, i32) {
        self.frame_info
            .map_or((0, 0), |info| info.turret_offset(body_angle))
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
    /// Настоящие тайлы поверхности (палитра + 64×64 из `.WRL`) для рендера 1:1.
    pub terrain: max_render::terrain::TerrainAtlas,
    /// Хром окна 1:1 (`FRAMEPIC` + панели), если он есть в `MAX.RES`.
    pub chrome: Option<crate::chrome::Chrome>,
    /// Наполнение панелей поверх `FRAMEPIC` (кнопки, счётчики, сетка, зум).
    pub panels: Option<crate::panels::Panels>,
    /// Меню на оригинальных ресурсах (`MAINPIC`/`PLANETSE`/`MULTGAME` +
    /// `BLANK_*`/`MNUBTN*`/`BTNHIDE`), пункт E.
    pub menu: Option<crate::menu::MenuAssets>,
    /// Экран заказа юнитов/зданий (`FACBUILD`/`CONBUILD` + `BLD*`), P2.
    pub build_menu: Option<crate::buildmenu::BuildMenuAssets>,
    /// Окно сохранения/загрузки V70 (`LOADPIC` + `FILE*` + `MNUBTN*`), P3.
    pub save_load: Option<crate::saveload::SaveLoadAssets>,
    /// Экран покупных апгрейдов (`UPGRADE` + `UPG*`/`BLD*`/`SEL*`), P6.
    pub upgrade: Option<crate::upgrademenu::UpgradeMenuAssets>,
    /// Экран мобилизации (`CARGOPIC` + `CRG*` + `BLD*`/`SEL*` + `VERT*` + `I_*`).
    pub cargo: Option<crate::mobilize::CargoMenuAssets>,
    /// Курсоры мыши (`Cursor_Descriptor`, 30 ресурсов).
    pub cursors: Option<CursorSet>,
    /// Маркеры ресурсов на карте (`RAWMSK*`/`FUELMK*`/`GOLDMK*` + малые).
    pub resource_markers: ResourceMarkers,
    /// Картинки планет по группам (4 × 128×128 big image, `SNOW_PIC + world/6`).
    pub planet_pictures: Vec<BigImage>,
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
        // TC-1109: `MAX.RES` тоже ищется от каталога exe/вверх, а не только по
        // конфигу (запуск `release\max_app.exe` из чужого CWD).
        let (max_res_path, max_res_warning) = resolve_max_res(config);
        let max_res = ResArchive::open(&max_res_path).with_context(|| {
            format!(
                "не удалось открыть {} (искали также рядом с exe; проверьте --data-dir/--max-res)",
                max_res_path.display()
            )
        })?;
        // Ассеты и data_hash разделены: `config.patches_res` может намеренно
        // отсутствовать (сервер без PATCHES.RES), тогда архив берётся из
        // fallback, а хэш по-прежнему считается по пути хендшейка.
        let (patches_path, patches_warning) = resolve_patches_res(config);
        let patches_res = ResArchive::open(&patches_path)
            .with_context(|| format!("не удалось открыть {}", patches_path.display()))?;
        let (palette_path, palette_warning) = resolve_color_pal(config);
        let palette = Palette::load(&palette_path)
            .with_context(|| format!("не удалось прочитать {}", palette_path.display()))?;
        let brightness = BrightnessLut::build(&palette);

        let world_name = world_asset_name(config.planet);
        // Оригинальная игра держит `.WRL` отдельными файлами рядом с `MAX.RES`
        // (в архиве лежат только строки `snow_1.w`/`green_1.` — имена файлов).
        // Но некоторые сборки пакуют `.WRL` внутрь `MAX.RES`, поэтому порядок
        // такой: файл в `world_dir` → ресурс архива.
        let world_dir = config
            .world_dir
            .clone()
            .or_else(|| max_res_path.parent().map(std::path::Path::to_path_buf))
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
                    max_res_path.display()
                )
            })?;
            let world = WrlMap::parse(bytes)
                .with_context(|| format!("не удалось разобрать ресурс {world_name}"))?;
            (world, format!("{}:{world_name}", max_res_path.display()))
        };

        let mut warnings = Vec::new();

        if let Some(warning) = max_res_warning {
            warnings.push(warning);
        }
        if let Some(warning) = patches_warning {
            warnings.push(warning);
        }
        if let Some(warning) = palette_warning {
            warnings.push(warning);
        }
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

            let frame_info = crate::cargo_unit_data::unit_frame_data(unit_type)
                .and_then(|name| max_res.read(name).ok())
                .and_then(|bytes| FrameInfo::decode(&bytes));

            sprites.push(LoadedSprite {
                unit_type,
                sprite_name: sprite_name.to_string(),
                shadow_name: loaded_shadow_name,
                sheet,
                shadow,
                frame_info,
            });
        }

        // Хром окна 1:1 (`FRAMEPIC` + панели) — необязателен: без него рендер
        // работает в прежнем полноэкранном режиме.
        let chrome = crate::chrome::Chrome::load(&max_res);
        let panels = crate::panels::Panels::load(&max_res);
        let menu = crate::menu::MenuAssets::load(&max_res);
        let build_menu = crate::buildmenu::BuildMenuAssets::load(&max_res);
        // Окно сохранения/загрузки V70 (`LOADPIC` + `FILE*_UP/DN` + кнопки), P3.
        let save_load = crate::saveload::SaveLoadAssets::load(&max_res);
        // Экран апгрейдов (`UPGRADE` + `UPG*` + `BLD*` + `SEL*`), P6.
        let upgrade = crate::upgrademenu::UpgradeMenuAssets::load(&max_res);
        // Экран мобилизации (`CARGOPIC` + `CRG*` + полосы + иконки `I_*`).
        let cargo = crate::mobilize::CargoMenuAssets::load(&max_res);

        if !cargo.is_ready() {
            warnings.push(
                "CargoMenu: CARGOPIC/BLDONE/BLDCAN не загрузились — мобилизация на рамках"
                    .to_string(),
            );
        }

        if !upgrade.is_ready() {
            warnings.push(
                "UpgradeMenu: UPGRADE/BLDONE/BLDCAN не загрузились — экран на рамках".to_string(),
            );
        }

        if !save_load.is_ready() {
            warnings.push(
                "SaveLoad: LOADPIC/FILE*_UP не загрузились — окно сохранения на рамках"
                    .to_string(),
            );
        }

        if !build_menu.is_ready() {
            warnings.push(
                "BuildMenu: FACBUILD/CONBUILD или кнопки BLD* не загрузились — экран на рамках"
                    .to_string(),
            );
        }

        if menu.is_none() {
            warnings.push(
                "меню на оригинальных ресурсах (MAINPIC/BLANK_*/MNUBTN*/…) не загрузилось"
                    .to_string(),
            );
        }

        if chrome.is_some() && panels.is_none() {
            warnings.push(
                "FRAMEPIC есть, а картинки панелей (FILES_OF/ENDTRN_U/…) не загрузились"
                    .to_string(),
            );
        }

        // Курсоры: 30 ресурсов `Cursor_ResourceLut` (`cursor.cpp:55-58`).
        let cursors = CursorSet::load(&max_res);
        if cursors.loaded() < max_assets::CURSOR_COUNT {
            warnings.push(format!(
                "курсоры: загрузилось {}/{} (нет: {})",
                cursors.loaded(),
                max_assets::CURSOR_COUNT,
                cursors.missing().join(", ")
            ));
        }

        // Маркеры ресурсов: настоящие спрайты карты вместо цветных точек.
        let resource_markers = ResourceMarkers::load(&max_res);
        if !resource_markers.is_ready() {
            warnings.push("маркеры ресурсов (RAWMSK*/FUELMK*/GOLDMK*) не загрузились".to_string());
        }

        // Картинки планет `SNOW_PIC + world/6` — для экрана выбора планеты.
        let mut planet_pictures = Vec::with_capacity(PLANET_GROUP_PICTURES.len());
        for name in PLANET_GROUP_PICTURES {
            match max_res.read(name) {
                Ok(bytes) => match BigImage::decode(bytes) {
                    Ok(image) => planet_pictures.push(image),
                    Err(error) => {
                        warnings.push(format!("картинка планеты {name} не разобралась: {error}"))
                    }
                },
                Err(error) => warnings.push(format!("нет картинки планеты {name}: {error}")),
            }
        }

        if planet_pictures.len() != PLANET_GROUP_PICTURES.len() {
            warnings.push(format!(
                "картинки планет: загрузилось {}/{}",
                planet_pictures.len(),
                PLANET_GROUP_PICTURES.len()
            ));
        }

        Ok(Self {
            max_res,
            chrome,
            panels,
            menu,
            build_menu: Some(build_menu),
            save_load: Some(save_load),
            upgrade: Some(upgrade),
            cargo: Some(cargo),
            cursors: Some(cursors),
            resource_markers,
            planet_pictures,
            patches_res,
            palette,
            brightness,
            planet: config.planet,
            world_name: world_name.to_string(),
            world_source,
            terrain: max_render::terrain::TerrainAtlas::from_wrl(&world),
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

    /// Картинка группы планет `SNOW_PIC + world/6` (`planetselectmenu.cpp:94`).
    ///
    /// У планет одной группы (6 подряд) общая картинка 128×128; её собственные
    /// `ulx/uly` (26,98 у `SNOW_PIC`) — позиция на экране выбора.
    pub fn planet_picture(&self, planet: Planet) -> Option<&BigImage> {
        self.planet_pictures.get(planet as usize / 6)
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
    fn alien_derelict_sprites_are_loaded() {
        use max_core::enums::UnitType;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let max_res = root.join("max-original/MAX.RES");
        let color_pal = root.join("max-original/COLOR.PAL");
        let patches = root.join("assets/PATCHES.RES");

        if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
            return;
        }

        let config = AppConfig {
            max_res,
            color_pal,
            patches_res: patches,
            ..Default::default()
        };
        let assets = Assets::load(&config).expect("ассеты игры");

        // Обломки alien-базы: три юнита (`ALN*`) + плита/мост и здания.
        for unit_type in [
            UnitType::AlienTank,
            UnitType::AlienAssaultGun,
            UnitType::AlienPlane,
            UnitType::LargeSlab,
            UnitType::Bridge,
            UnitType::ShieldGenerator,
            UnitType::SupportPlant,
            UnitType::RecreationCenter,
        ] {
            let sprite = assets
                .sprite(unit_type)
                .unwrap_or_else(|| panic!("нет спрайта {unit_type:?}"));
            assert!(!sprite.sheet.is_empty(), "{unit_type:?}: нет кадров");
        }

        // `RADAR` — это спрайт здания-радара (20 кадров 64×64, 60 449 Б), а не
        // рамка миникарты: рамка нарисована в `FRAMEPIC`
        // (`ResourceManager_MinimapBgImage` — снимок окна миникарты).
        let radar = assets.sprite(UnitType::Radar).expect("RADAR");
        assert_eq!(radar.sheet.len(), 20);
        assert_eq!(
            (radar.sheet.frames[0].width, radar.sheet.frames[0].height),
            (64, 64)
        );
        assert_eq!(
            (radar.sheet.frames[0].hotx, radar.sheet.frames[0].hoty),
            (32, 32)
        );
    }

    #[test]
    fn planet_pictures_cover_all_four_groups() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let max_res = root.join("max-original/MAX.RES");
        let color_pal = root.join("max-original/COLOR.PAL");
        let patches = root.join("assets/PATCHES.RES");

        if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
            return;
        }

        let assets = Assets::load(&AppConfig {
            max_res,
            color_pal,
            patches_res: patches,
            ..Default::default()
        })
        .expect("ассеты игры");

        assert_eq!(assets.planet_pictures.len(), 4);

        for planet in [
            Planet::Snowcrab,
            Planet::IronCross,
            Planet::NewLuzon,
            Planet::Bottleneck,
        ] {
            let picture = assets.planet_picture(planet).expect("картинка планеты");
            assert_eq!((picture.width, picture.height), (128, 128));
        }
    }

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

    /// Уникальный временный каталог теста.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "max_app_assets_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Путь внутри репозитория (`max-rust/crates/max_app/../../..`).
    fn repo_path(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(relative)
    }

    #[test]
    fn configured_patches_res_wins_over_any_fallback() {
        let dir = temp_dir("direct");
        let patches = dir.join("PATCHES.RES");
        std::fs::write(&patches, b"RES0").expect("write PATCHES.RES");

        let config = AppConfig {
            patches_res: patches.clone(),
            ..Default::default()
        };

        let (path, warning) = resolve_patches_res(&config);
        assert_eq!(path, patches);
        assert!(warning.is_none(), "без fallback предупреждения нет");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_patches_res_falls_back_to_the_server_template() {
        let dir = temp_dir("template");
        let data = dir.join("max-original");
        std::fs::create_dir_all(&data).expect("data dir");
        let max_res = data.join("MAX.RES");
        std::fs::write(&max_res, b"RES0").expect("write MAX.RES");

        let fallback = dir.join("build/linux-server-template/PATCHES.RES");
        std::fs::create_dir_all(fallback.parent().expect("parent")).expect("template dir");
        std::fs::write(&fallback, b"RES0fallback").expect("write fallback");

        let missing = dir.join("nowhere/PATCHES.RES");
        let config = AppConfig {
            max_res,
            patches_res: missing.clone(),
            ..Default::default()
        };

        let (path, warning) = resolve_patches_res(&config);
        assert_eq!(path, fallback, "шаблон headless-сервера — первый fallback");
        assert!(
            warning.as_deref().unwrap_or("").contains("fallback"),
            "о подмене нужно предупредить: {warning:?}"
        );
        assert_eq!(
            config.patches_res, missing,
            "config не меняется (хэш отдельно)"
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_patches_res_falls_back_to_the_repository_assets_dir() {
        let dir = temp_dir("assets");
        let data = dir.join("max-original");
        std::fs::create_dir_all(&data).expect("data dir");
        let max_res = data.join("MAX.RES");
        std::fs::write(&max_res, b"RES0").expect("write MAX.RES");

        let fallback = dir.join("assets/PATCHES.RES");
        std::fs::create_dir_all(fallback.parent().expect("parent")).expect("assets dir");
        std::fs::write(&fallback, b"RES0assets").expect("write fallback");

        let config = AppConfig {
            max_res,
            patches_res: dir.join("nowhere/PATCHES.RES"),
            ..Default::default()
        };

        let (path, warning) = resolve_patches_res(&config);
        assert_eq!(path, fallback, "без шаблона берётся assets/PATCHES.RES");
        assert!(warning.is_some());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn absent_everywhere_keeps_the_configured_path() {
        // Тест герметичен: с TC-1109 ambient-резолвер ищет ещё и рядом с exe,
        // а тестовый бинарь лежит внутри checkout'а, где `assets/PATCHES.RES`
        // реально существует. «Нигде нет» проверяется на пустых корнях.
        let dir = temp_dir("nowhere");
        let config = AppConfig {
            max_res: dir.join("missing/MAX.RES"),
            patches_res: dir.join("missing/PATCHES.RES"),
            color_pal: dir.join("missing/COLOR.PAL"),
            ..Default::default()
        };
        let roots = vec![dir.clone()];

        let (path, warning) = resolve_patches_res_at(&config, &roots);
        assert_eq!(path, config.patches_res);
        assert!(warning.is_none());

        let (path, warning) = resolve_max_res_at(&config, &roots);
        assert_eq!(path, config.max_res);
        assert!(warning.is_none());

        let (path, warning) = resolve_color_pal_at(&config, &roots);
        assert_eq!(path, config.color_pal);
        assert!(warning.is_none());

        std::fs::remove_dir_all(dir).ok();
    }

    /// TC-1109: `MAX.RES`/`PATCHES.RES`/`COLOR.PAL` находятся от каталога exe,
    /// когда CWD — чужой пустой каталог (запуск `release\max_app.exe`).
    #[test]
    fn exe_dir_resolves_game_data_when_cwd_is_elsewhere() {
        let root = temp_dir("exe");
        let cwd = root.join("cwd");
        let exe_dir = root.join("release");
        let data = exe_dir.join(DATA_DIR_NAME);
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::create_dir_all(&data).expect("data dir");
        std::fs::create_dir_all(exe_dir.join("assets")).expect("assets dir");

        let max_res = data.join(MAX_RES_NAME);
        let palette = data.join(COLOR_PAL_NAME);
        let patches = exe_dir.join("assets/PATCHES.RES");
        std::fs::write(&max_res, b"RES0").expect("write MAX.RES");
        std::fs::write(&palette, b"palette").expect("write COLOR.PAL");
        std::fs::write(&patches, b"RES0").expect("write PATCHES.RES");

        let config = AppConfig {
            max_res: cwd.join("max-original/MAX.RES"),
            color_pal: cwd.join("max-original/COLOR.PAL"),
            patches_res: cwd.join("assets/PATCHES.RES"),
            ..Default::default()
        };
        let exe = exe_dir.join("max_app.exe");
        let roots = data_search_roots_from(&config, Some(&cwd), Some(&exe));

        let (path, warning) = resolve_max_res_at(&config, &roots);
        assert_eq!(path, max_res, "MAX.RES от каталога exe");
        assert!(
            warning.as_deref().unwrap_or("").contains("MAX.RES"),
            "предупреждение о подмене: {warning:?}"
        );

        let (path, warning) = resolve_color_pal_at(&config, &roots);
        assert_eq!(path, palette, "COLOR.PAL от каталога exe");
        assert!(warning.is_some());

        let (path, warning) = resolve_patches_res_at(&config, &roots);
        assert_eq!(path, patches, "PATCHES.RES от каталога exe");
        assert!(warning.is_some());

        // Конфиг не мутируется: хэндшейк считает путь из `config`.
        assert_eq!(config.max_res, cwd.join("max-original/MAX.RES"));

        std::fs::remove_dir_all(root).ok();
    }

    /// TC-1109: если exe лежит глубже (`bin/`), данные ищутся вверх до корня
    /// с `max-original/` — но не выше него (без находки выше не поднимаемся).
    #[test]
    fn exe_ancestors_are_searched_up_to_the_data_root() {
        let root = temp_dir("exe-up");
        let bin = root.join("release/bin");
        let data = root.join(DATA_DIR_NAME);
        std::fs::create_dir_all(&bin).expect("bin");
        std::fs::create_dir_all(&data).expect("data dir");
        std::fs::write(data.join(MAX_RES_NAME), b"RES0").expect("write MAX.RES");

        let exe = bin.join("max_app.exe");
        let roots = exe_search_roots(&exe);

        assert_eq!(
            roots.first(),
            Some(&bin),
            "первый корень — сам каталог exe"
        );
        assert!(
            roots.iter().any(|candidate| *candidate == root),
            "корень с max-original должен войти в поиск: {roots:?}"
        );

        let config = AppConfig {
            max_res: bin.join("max-original/MAX.RES"),
            ..Default::default()
        };
        let (path, _) = resolve_max_res_at(&config, &roots);
        assert_eq!(path, data.join(MAX_RES_NAME));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn missing_color_pal_falls_back_next_to_max_res() {
        let dir = temp_dir("palette");
        let data = dir.join("max-original");
        std::fs::create_dir_all(&data).expect("data dir");
        let max_res = data.join("MAX.RES");
        std::fs::write(&max_res, b"RES0").expect("write MAX.RES");
        let palette = data.join("COLOR.PAL");
        std::fs::write(&palette, b"palette").expect("write COLOR.PAL");

        let config = AppConfig {
            max_res,
            color_pal: dir.join("nowhere/COLOR.PAL"),
            ..Default::default()
        };

        let (path, warning) = resolve_color_pal(&config);
        assert_eq!(path, palette);
        assert!(warning.as_deref().unwrap_or("").contains("COLOR.PAL"));
        assert_eq!(config.color_pal, dir.join("nowhere/COLOR.PAL"));

        std::fs::remove_dir_all(dir).ok();
    }

    /// Ассеты грузятся из fallback, а `patches_res` остаётся путём хэндшейка.
    ///
    /// Пропускается, если данных игры нет рядом с репозиторием (как
    /// `assets_integration_smoke` в `tests/app.rs`).
    #[test]
    fn assets_load_from_fallback_when_configured_patches_res_is_missing() {
        let max_res = repo_path("max-original/MAX.RES");
        let color_pal = repo_path("max-original/COLOR.PAL");

        if !max_res.is_file() || !color_pal.is_file() {
            return;
        }

        let missing = repo_path("build/does-not-exist/PATCHES.RES");
        let config = AppConfig {
            max_res,
            color_pal: repo_path("max-original/does-not-exist.COLOR.PAL"),
            patches_res: missing.clone(),
            ..Default::default()
        };

        let assets = Assets::load(&config).expect("ассеты из fallback");
        let report = assets.report();

        assert!(
            report.patches_res_entries > 0,
            "PATCHES.RES должен загрузиться из fallback"
        );
        assert_eq!(
            config.patches_res, missing,
            "путь хэндшейка не подменяется fallback'ом"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("fallback")),
            "предупреждение о fallback должно попасть в отчёт: {:?}",
            report.warnings
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("COLOR.PAL")),
            "палитра тоже должна резолвиться рядом с MAX.RES: {:?}",
            report.warnings
        );
    }
}
