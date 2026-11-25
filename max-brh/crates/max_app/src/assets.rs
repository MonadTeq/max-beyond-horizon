//! Application assets: `MAX.RES`, `PATCHES.RES`, `COLOR.PAL` and unit sprites.
//!
//! The module is the only place in `max_app` that knows about the game files. It
//! assembles [`Assets`] from them:
//!
//! * [`ResArchive`] for `MAX.RES` and `PATCHES.RES` (maps, sprites, data);
//! * the planet [`WrlMap`] from `MAX.RES` (the original game archive contains
//!   `SNOW_1`…`DESERT_6`, not separate `.WRL` files);
//! * [`Palette`] + [`BrightnessLut`] from `COLOR.PAL` (rendering [`max_render`]);
//! * [`SpriteSheet`] and [`ShadowSheet`] for unit types from [`UNIT_SPRITES`].
//!
//! Only the absence/corruption of the mandatory files and the planet map is
//! considered an error. An individual sprite missing from the archive is a warning in
//! [`Assets::warnings`]: the client must show the map and marker units even on
//! an incomplete data set.

use anyhow::{Context, Result};
use max_assets::flic::Flic;
use max_assets::sprite::{FrameInfo, ShadowFrame, SpriteFrame};
use max_assets::{
    decode_shadow, decode_sprite, BigImage, CursorSet, ResArchive, ResourceMarkers, ShadowSheet,
    SimpleImage, SpriteSheet, WrlMap,
};
use max_core::enums::{Planet, UnitType};
use max_render::palette::{BrightnessLut, Palette};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::AppConfig;

/// `MAX.RES` resource names for the 24 planets — `SNOW_1 + world_index`
/// (`resource_manager.cpp:1183`, `ResourceID` 0x76E…0x785).
pub const WORLD_ASSETS: [&str; 24] = [
    "SNOW_1", "SNOW_2", "SNOW_3", "SNOW_4", "SNOW_5", "SNOW_6", "CRATER_1", "CRATER_2", "CRATER_3",
    "CRATER_4", "CRATER_5", "CRATER_6", "GREEN_1", "GREEN_2", "GREEN_3", "GREEN_4", "GREEN_5",
    "GREEN_6", "DESERT_1", "DESERT_2", "DESERT_3", "DESERT_4", "DESERT_5", "DESERT_6",
];

/// Planet pictures by groups of 6 (`SNOW_PIC + world/6`,
/// `planetselectmenu.cpp:94`, `resourcetable.hpp:1925-1928`).
pub const PLANET_GROUP_PICTURES: [&str; 4] = ["SNOW_PIC", "CRTR_PIC", "GREN_PIC", "DSRT_PIC"];

/// Planet `.WRL` resource name (`settings["world"]`).
///
/// The [`Planet`] order matches the C++ resource order (`SNOW_1 + index`),
/// so this is simply table indexing.
pub fn world_asset_name(planet: Planet) -> &'static str {
    WORLD_ASSETS[planet as usize]
}

/// Where to look for `PATCHES.RES` if [`AppConfig::patches_res`] does not exist.
///
/// The paths are relative: to the repository root, the current directory and the
/// [`AppConfig::max_res`] directory. The first — the headless server template the live
/// smoke is launched from; the second — the repository's usual `assets/`; the third —
/// the C++ port's shared directory (`max-cpp-port/assets/`), where `PATCHES.RES`
/// lies in this checkout.
pub const PATCHES_RES_FALLBACKS: [&str; 3] = [
    "bin/linux-server-template/PATCHES.RES",
    "assets/PATCHES.RES",
    "max-cpp-port/assets/PATCHES.RES",
];

/// Game data directory name at the delivery root (`max-original/`).
pub const DATA_DIR_NAME: &str = "max-original";
/// Main data archive name.
pub const MAX_RES_NAME: &str = "MAX.RES";
/// Palette name.
pub const COLOR_PAL_NAME: &str = "COLOR.PAL";

/// Game data search roots in priority order (defect **TC-1109**).
///
/// Order:
/// 1. the [`AppConfig::max_res`] directory and its parent (data next to the config);
/// 2. the current directory and its parent (historical behavior, launch from `max-brh/`);
/// 3. the directory of the running **exe** ([`std::env::current_exe`]) and up the tree
///    to the nearest directory with `max-original` inclusive.
///
/// Point 3 is the TC-1109 fix: `bin\release\max_app.exe` (or `target\debug\…` in the
/// checkout) finds the data even if the CWD is a foreign directory.
pub fn data_search_roots(config: &AppConfig) -> Vec<PathBuf> {
    let cwd = std::env::current_dir().ok();
    let exe = std::env::current_exe().ok();

    data_search_roots_from(config, cwd.as_deref(), exe.as_deref())
}

/// [`data_search_roots`] with explicit CWD/exe — for tests with temporary directories.
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

/// The exe directory and up to the nearest directory with `max-original` (inclusive).
///
/// If there is no `max-original` nearby, only the exe directory remains: climbing
/// higher without a data marker is dangerous — this could accidentally find another
/// repository's `assets/` (see the test `absent_everywhere_keeps_the_configured_path`).
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

/// Finds `MAX.RES` for **asset loading** if the config path is missing.
///
/// TC-1109 defect: the relative `max-original/MAX.RES` did not resolve when
/// launched from another directory, although the data lies next to the exe. Returns
/// the chosen path and a warning (the config path itself is not changed).
pub fn resolve_max_res(config: &AppConfig) -> (PathBuf, Option<String>) {
    let configured = &config.max_res;

    if configured.is_file() {
        return (configured.clone(), None);
    }

    resolve_max_res_at(config, &data_search_roots(config))
}

/// [`resolve_max_res`] over the given search roots (for tests).
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

/// Finds `PATCHES.RES` for **asset loading** (SC_*/tables, sprites).
///
/// The handshake (`data_hash`) does not use this path: `net.rs` takes
/// [`AppConfig::handshake_patches_path`], so the "server's" empty
/// `PATCHES.RES` and the real assets may diverge. Returns the chosen
/// path and a warning if the fallback fired.
pub fn resolve_patches_res(config: &AppConfig) -> (PathBuf, Option<String>) {
    resolve_patches_res_at(config, &data_search_roots(config))
}

/// [`resolve_patches_res`] over the given search roots (for tests).
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

/// Finds `COLOR.PAL` for **asset loading** if the config path is missing.
///
/// The palette is part of the same data as `MAX.RES`; `--max-res` without
/// `--data-dir`/`--palette` (as in the live smoke) left the relative path
/// `max-original/COLOR.PAL` unresolved, and the world was not generated. The handshake
/// does not use `COLOR.PAL`, so the substitution is safe.
pub fn resolve_color_pal(config: &AppConfig) -> (PathBuf, Option<String>) {
    resolve_color_pal_at(config, &data_search_roots(config))
}

/// [`resolve_color_pal`] over the given search roots (for tests).
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

/// Index of `*.FLC` in the data directory: lowercase name → path.
///
/// Case does not matter: in the repository the files are uppercase (`AGT.FLC`), while the
/// references in `MAX.RES` are lowercase (`agt.flc`); on Linux the file system distinguishes
/// case, so we look up by index rather than by a direct path.
/// Index of `*.MVE` files next to `MAX.RES` (lowercase names).
fn mve_index(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut index = BTreeMap::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return index;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        if name.ends_with(".mve") {
            index.insert(name, path);
        }
    }

    index
}

fn flc_index(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut index = BTreeMap::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return index;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        if name.ends_with(".flc") {
            index.insert(name, path);
        }
    }

    index
}

/// Which unit types are loaded from `MAX.RES` into [`Assets`].
///
/// This is a line-by-line port of `SC_UNITS` (`assets/PATCHES.RES`,
/// `Units::LoadResource`): the `sprite` field (`Unit::GetSprite()`) and `shadow`
/// (`Unit::GetShadow()`; `None` — `INVALID_ID`). Order — [`UnitType`]
/// (0x00–0x5C, all 93 `UNIT_END` types), so **every** original unit
/// is drawn on the map with a sprite: body + shadow + a separate turret from `D_*`
/// (`Unit::GetFrameInfo()`), not with a team marker. The HUD portrait
/// (`panels::draw_portrait`), `BuildMenu` and `UpgradeMenu` take the same records
/// through [`Assets::sprite`].
///
/// Format: `(type, sprite name, shadow name)`. In the original the shadows are truncated to 8
/// characters (`S_` + the first 6 letters of the name), so the names are given explicitly.
///
/// The list covers not only mobile units but also **buildings**, debris,
/// mines, explosions and projectiles: in M.A.X. these are also `UnitType`s with a sprite from
/// `MAX.RES`.
///
/// Two discrepancies of the game data itself (not of the table) — see
/// [`UNIT_TYPES_WITHOUT_SPRITE_DATA`] and [`UNIT_TYPES_WITHOUT_SHADOW_DATA`]:
/// `MASTER` has no body resource either in `MAX.RES` or `PATCHES.RES`, and
/// `SUBMARNE` — the `S_SUBMAR` shadow. The original in these cases simply draws
/// nothing (`UnitInfo::RenderSprite`/`RenderShadow` check the data pointer);
/// the Rust client keeps the team marker for `MASTER`.
pub const UNIT_SPRITES: [(UnitType, &str, Option<&str>); 93] = [
    // --- buildings and stationary objects ---------------------------------
    (UnitType::GoldRefinery, "COMMTWR", Some("S_COMMTW")),
    (UnitType::PowerStation, "POWERSTN", Some("S_POWERS")),
    (UnitType::PowerGenerator, "POWGEN", Some("S_POWGEN")),
    (UnitType::Barracks, "BARRACKS", Some("S_BARRAC")),
    (UnitType::MonopoleMine, "SHIELDGN", Some("S_SHIELD")),
    (UnitType::Radar, "RADAR", Some("S_RADAR")),
    (UnitType::MaterialStorage, "ADUMP", Some("S_ADUMP")),
    (UnitType::FuelStorage, "FDUMP", Some("S_FDUMP")),
    (UnitType::GoldVault, "GOLDSM", Some("S_GOLDSM")),
    (UnitType::Depot, "DEPOT", Some("S_DEPOT")),
    (UnitType::Hangar, "HANGAR", Some("S_HANGAR")),
    (UnitType::Dock, "DOCK", Some("S_DOCK")),
    (UnitType::Connector, "CNCT_4W", Some("S_CNCT4W")),
    (UnitType::LargeRubble, "LRGRUBLE", Some("S_LRGRBL")),
    (UnitType::SmallRubble, "SMLRUBLE", Some("S_SMLRBL")),
    // Films — flat decals, no shadow in SC_UNITS (`INVALID_ID`).
    (UnitType::LargeTape, "LRGTAPE", None),
    (UnitType::SmallTape, "SMLTAPE", None),
    (UnitType::LargeSlab, "LRGSLAB", Some("S_LRGSLA")),
    (UnitType::SmallSlab, "SMLSLAB", Some("S_SMLSLA")),
    (UnitType::LargeCones, "LRGCONES", Some("S_LRGCON")),
    (UnitType::SmallCones, "SMLCONES", Some("S_SMLCON")),
    (UnitType::Road, "ROAD", Some("S_ROAD")),
    (UnitType::LandingPad, "LANDPAD", Some("S_LANDPA")),
    (UnitType::Shipyard, "SHIPYARD", Some("S_SHIPYA")),
    (UnitType::LightVehiclePlant, "LIGHTPLT", Some("S_LIGHTP")),
    (UnitType::HeavyVehiclePlant, "LANDPLT", Some("S_LANDPL")),
    (UnitType::SupportPlant, "SUPRTPLT", Some("S_SUPRTP")),
    (UnitType::AirUnitsPlant, "AIRPLT", Some("S_AIRPLT")),
    (UnitType::Habitat, "HABITAT", Some("S_HABITA")),
    (UnitType::ResearchCentre, "RESEARCH", Some("S_RESEAR")),
    (UnitType::EcoSphere, "GREENHSE", Some("S_GREENH")),
    (UnitType::RecreationCenter, "RECCENTR", Some("S_RECCEN")),
    (UnitType::TrainingHall, "TRAINHAL", Some("S_TRAINH")),
    (UnitType::WaterPlatform, "WTRPLTFM", Some("S_WTRPLT")),
    // --- defensive turrets, mines, explosions -----------------------------
    (UnitType::GunTurret, "GUNTURRT", Some("S_GUNTUR")),
    (UnitType::AntiAir, "ANTIAIR", Some("S_ANTIAI")),
    (UnitType::Artillery, "ARTYTRRT", Some("S_ARTYTR")),
    (UnitType::MissileLauncher, "ANTIMSSL", Some("S_ANTIMS")),
    (UnitType::ConcreteBlock, "BLOCK", Some("S_BLOCK")),
    (UnitType::Bridge, "BRIDGE", Some("S_BRIDGE")),
    (UnitType::MiningStation, "MININGST", Some("S_MINING")),
    (UnitType::LandMine, "LANDMINE", Some("S_LANDMI")),
    // Mine/explosions — no shadow in SC_UNITS (`INVALID_ID`).
    (UnitType::SeaMine, "SEAMINE", None),
    (UnitType::LandExplosion, "LNDEXPLD", None),
    (UnitType::AirExplosion, "AIREXPLD", None),
    (UnitType::SeaExplosion, "SEAEXPLD", None),
    (UnitType::BuildingExplosion, "BLDEXPLD", None),
    (UnitType::HitExplosion, "HITEXPLD", None),
    // --- ground vehicles and infantry -------------------------------------
    // `MASTER` — the only type without a body resource (see below).
    (UnitType::MasterBuilder, "MASTER", Some("S_MASTER")),
    (UnitType::Constructor, "CONSTRCT", Some("S_CONSTR")),
    (UnitType::Scout, "SCOUT", Some("S_SCOUT")),
    (UnitType::Tank, "TANK", Some("S_TANK")),
    (UnitType::AssaultGun, "ARTILLRY", Some("S_ARTILL")),
    (UnitType::RocketLauncher, "ROCKTLCH", Some("S_ROCKTL")),
    (UnitType::MissileCrawler, "MISSLLCH", Some("S_MISSLL")),
    (UnitType::MobileAntiAir, "SP_FLAK", Some("S_FLAK")),
    (UnitType::MineLayer, "MINELAYR", Some("S_MINELA")),
    (UnitType::Surveyor, "SURVEYOR", Some("S_SURVEY")),
    (UnitType::Scanner, "SCANNER", Some("S_SCANNE")),
    (UnitType::SupplyTruck, "SPLYTRCK", Some("S_SPLYTR")),
    (UnitType::GoldTruck, "GOLDTRCK", Some("S_GOLDTR")),
    (UnitType::Engineer, "ENGINEER", Some("S_ENGINE")),
    (UnitType::Bulldozer, "BULLDOZR", Some("S_BULLDO")),
    (UnitType::RepairUnit, "REPAIR", Some("S_REPAIR")),
    (UnitType::FuelTruck, "FUELTRCK", Some("S_FUELTR")),
    (
        UnitType::ArmouredPersonnelCarrier,
        "CLNTRANS",
        Some("S_CLNTRA"),
    ),
    (UnitType::Infiltrator, "COMMANDO", Some("S_COMMAN")),
    (UnitType::Infantry, "INFANTRY", Some("S_INFANT")),
    // --- navy -------------------------------------------------------------
    (UnitType::Escort, "FASTBOAT", Some("S_FASTBO")),
    (UnitType::Corvette, "CORVETTE", Some("S_CORVET")),
    (UnitType::Gunboat, "BATTLSHP", Some("S_BATTLS")),
    // `S_SUBMAR` is declared in SC_UNITS, but there is no resource in the game data.
    (UnitType::Submarine, "SUBMARNE", Some("S_SUBMAR")),
    (UnitType::SeaTransport, "SEATRANS", Some("S_SEATRA")),
    (UnitType::MissileCruiser, "MSSLBOAT", Some("S_MSSLBO")),
    (UnitType::SeaMineLayer, "SEAMNLYR", Some("S_SEAMNL")),
    (UnitType::CargoShip, "CARGOSHP", Some("S_CARGOS")),
    // --- aviation ---------------------------------------------------------
    (UnitType::Fighter, "FIGHTER", Some("S_FIGHTE")),
    (UnitType::Bomber, "BOMBER", Some("S_BOMBER")),
    (UnitType::AirTransport, "AIRTRANS", Some("S_AIRTRA")),
    (UnitType::Awac, "AWAC", Some("S_AWAC")),
    // --- aliens -----------------------------------------------------------
    (UnitType::Juggernaut, "JUGGRNT", Some("S_JUGGRN")),
    (UnitType::AlienTank, "ALNTANK", Some("S_ALNTAN")),
    (UnitType::AlienAssaultGun, "ALNASGUN", Some("S_ALNASG")),
    (UnitType::AlienPlane, "ALNPLANE", Some("S_ALNPLA")),
    // --- projectiles and trails (`MISSILE_UNIT` units, without shadows) ---
    (UnitType::Rocket, "ROCKET", None),
    (UnitType::Torpedo, "TORPEDO", None),
    (UnitType::AlienMissile, "ALNMISSL", None),
    (UnitType::AlienTankBall, "ALNTBALL", None),
    (UnitType::AlienArtilleryBall, "ALNABALL", None),
    (UnitType::RocketSmoke, "RKTSMOKE", None),
    (UnitType::TorpedoBubble, "TRPBUBLE", None),
    // --- extraction and decor ---------------------------------------------
    (UnitType::Harvester, "HARVSTER", Some("S_HARVST")),
    (UnitType::DeadWaldo, "WALDO", None),
];

/// Types for which `SC_UNITS` defines a sprite but the game data has no resource.
///
/// `MASTER` (Master Builder): neither `MAX.RES` nor `PATCHES.RES` has `MASTER` or
/// `S_MASTER` (the game directory contains an orphaned `MASTR17.SPW`, but there is no
/// reference to it in the archives). C++ `UnitInfo::RenderSprite` checks
/// `sprite_data != null` and in this case draws nothing; the Rust client
/// keeps the team marker — this is the **only** type on the map without a sprite.
pub const UNIT_TYPES_WITHOUT_SPRITE_DATA: [UnitType; 1] = [UnitType::MasterBuilder];

/// Types whose `SC_UNITS` references an `S_*` shadow missing from the game
/// data (neither in `MAX.RES` nor in `PATCHES.RES`).
///
/// `SUBMARNE` → `S_SUBMAR`: the original draws the submarine without a shadow, the port too
/// (`LoadedSprite::shadow` remains `None`).
pub const UNIT_TYPES_WITHOUT_SHADOW_DATA: [UnitType; 1] = [UnitType::Submarine];

/// Loaded sprite of one unit type.
#[derive(Debug, Clone)]
pub struct LoadedSprite {
    /// Unit type (the lookup key).
    pub unit_type: UnitType,
    /// Resource name in `MAX.RES` (`TANK`, `INFANTRY`, …).
    pub sprite_name: String,
    /// Shadow resource name, if the shadow was in the archive and parsed.
    pub shadow_name: Option<String>,
    /// Normal sprite frames.
    pub sheet: SpriteSheet,
    /// Shadow frames (darkening of the already drawn map).
    pub shadow: Option<ShadowSheet>,
    /// `Unit::GetFrameInfo()` from the `D_*` resource: the turret frame base and
    /// `angle_offsets` for its overlay (`None` — no resource/not parsed).
    pub frame_info: Option<FrameInfo>,
}

impl LoadedSprite {
    /// Body frame by an explicit index (`GetSpriteFrame`).
    ///
    /// Needed when the frame index is chosen by the calling layer (e.g.
    /// a powered-on building draws `image_base + 1`, see
    /// `render::unit_frame_index`). If the index is outside the sprite,
    /// frame 0 is returned — like the [`Self::frame_for`] fallback.
    pub fn frame_at(&self, index: usize) -> Option<&max_assets::sprite::SpriteFrame> {
        self.sheet.frame(index).or_else(|| self.sheet.frame(0))
    }

    /// Shadow frame by the same index as [`Self::frame_at`].
    ///
    /// The shadow may have fewer frames than the body (or none at all) —
    /// then the last available/frame 0 is taken.
    pub fn shadow_at(&self, index: usize) -> Option<&max_assets::sprite::ShadowFrame> {
        let shadow = self.shadow.as_ref()?;
        let last = shadow.len().checked_sub(1)?;

        shadow.frame(index.min(last)).or_else(|| shadow.frame(0))
    }

    /// Frame for a unit: `image_base + angle` wrapped in `frame(0)`.
    ///
    /// The original layout — "phase × 8 + angle" (`max-brh-docs/formats/spw.md` §5.3), and
    /// in a V70 save `image_base`/`angle` are already stored per unit. The index
    /// is clamped by the frame count because buildings/vehicles have a different number of
    /// phases.
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

    /// Shadow frame corresponding to [`Self::frame_for`] (where possible).
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

    /// Turret/gun frame — `turret_image_base + turret_angle`
    /// (`Unit::GetFrameInfo()`; `UnitInfo::RenderWithConnectors`).
    ///
    /// `None` if the type has no `D_*` or turret frames.
    pub fn turret_frame(&self, turret_angle: u8) -> Option<&SpriteFrame> {
        let info = self.frame_info?;

        if info.turret_image_count == 0 {
            return None;
        }

        let index = info.turret_frame_index(turret_angle);

        self.sheet.frame(index)
    }

    /// Turret shadow frame (`ShadowSheet`, the same index as the sprite).
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

    /// Where to draw the turret relative to the body anchor:
    /// `angle_offsets[body_angle]` (`UnitInfo::UpdateTurretAngle`).
    pub fn turret_offset(&self, body_angle: u8) -> (i32, i32) {
        self.frame_info
            .map_or((0, 0), |info| info.turret_offset(body_angle))
    }
}

/// Brief information about the loaded sprite (for the CLI report).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteInfo {
    /// Unit type.
    pub unit_type: UnitType,
    /// Resource name in `MAX.RES`.
    pub name: String,
    /// Number of frames.
    pub frames: usize,
    /// Number of shadow frames (0 — no shadow).
    pub shadow_frames: usize,
}

/// [`Assets::load`] report — what the CLI prints and the smoke tests check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetsReport {
    /// Number of records in `MAX.RES`.
    pub max_res_entries: usize,
    /// Number of records in `PATCHES.RES`.
    pub patches_res_entries: usize,
    /// Planet and its `.WRL` resource.
    pub planet: Planet,
    /// Map resource name (`GREEN_1`, …).
    pub world: String,
    /// Where the map was loaded from: the `.WRL` path or `MAX.RES:<name>`.
    pub world_source: String,
    /// Map size in cells.
    pub world_size: (i16, i16),
    /// Loaded sprites.
    pub sprites: Vec<SpriteInfo>,
    /// How many portrait FLC animations were loaded (the reference — 71).
    pub flics: usize,
    /// Non-critical problems (no sprite, broken shadow, …).
    pub warnings: Vec<String>,
}

impl AssetsReport {
    /// Number of loaded sprites.
    pub fn sprite_count(&self) -> usize {
        self.sprites.len()
    }
}

/// All game data the application needs.
#[derive(Debug, Clone)]
pub struct Assets {
    /// `MAX.RES` archive.
    pub max_res: ResArchive,
    /// `PATCHES.RES` archive (mission rules, `SC_*`).
    pub patches_res: ResArchive,
    /// `COLOR.PAL` palette.
    pub palette: Palette,
    /// The game system palette used to draw sprites and rotated by
    /// the palette cycles (`World::InitColorCycles`): `FRAMEPIC` for slots
    /// 0..63/160..255 + the `.WRL` palette for 64..159.
    pub system_palette: Palette,
    /// Brightness tables built from the palette.
    pub brightness: BrightnessLut,
    /// The planet the map was loaded for.
    pub planet: Planet,
    /// Planet `.WRL` resource name.
    pub world_name: String,
    /// Where the map was loaded from (`path` or `MAX.RES:<name>`).
    pub world_source: String,
    /// Parsed planet map.
    pub world: WrlMap,
    /// Real surface tiles (palette + 64×64 from `.WRL`) for 1:1 rendering.
    pub terrain: max_render::terrain::TerrainAtlas,
    /// 1:1 window chrome (`FRAMEPIC` + panels), if present in `MAX.RES`.
    pub chrome: Option<crate::chrome::Chrome>,
    /// Panel contents over `FRAMEPIC` (buttons, counters, grid, zoom).
    pub panels: Option<crate::panels::Panels>,
    /// Menu on the original resources (`MAINPIC`/`PLANETSE`/`MULTGAME` +
    /// `BLANK_*`/`MNUBTN*`/`BTNHIDE`), item E.
    pub menu: Option<crate::menu::MenuAssets>,
    /// Unit/building order screen (`FACBUILD`/`CONBUILD` + `BLD*`), P2.
    pub build_menu: Option<crate::buildmenu::BuildMenuAssets>,
    /// V70 save/load window (`LOADPIC` + `FILE*` + `MNUBTN*`), P3.
    pub save_load: Option<crate::saveload::SaveLoadAssets>,
    /// Purchasable upgrades screen (`UPGRADE` + `UPG*`/`BLD*`/`SEL*`), P6.
    pub upgrade: Option<crate::upgrademenu::UpgradeMenuAssets>,
    /// Research screen (`RSRCHPIC` + `I_*` + `UPG*` + `PRFSLIDE`), P7.
    pub research: Option<crate::researchmenu::ResearchMenuAssets>,
    /// Help window (`HELPFRAM` + `HELPOK` + `BLDUP__`/`BLDDWN`).
    pub help: Option<crate::helpmenu::HelpMenuAssets>,
    /// `Preferences`/`Setup` dialog (`PREFSPIC` + `PRF*` + `UNCHKED`/`CHECKED`
    /// + `PREFEDIT`/`PREFNAME`), C++ `OptionsMenu`.
    pub preferences: Option<crate::preferences::PreferencesAssets>,
    /// Unit command menu (`UNTBTN_U`/`UNTBTN_D`, 42×21) — the
    /// `GameManager_PopupButtons`.
    pub unit_menu: Option<crate::unitmenu::UnitMenuAssets>,
    /// Building self-destruct dialog (`SELFDSTR`/`SLFDAR`/`SLFDCN`/
    /// `SLFDOPN1..6`/`SLFDOK`), C++ `UnitsManager_SelfDestructMenu`.
    pub self_destruct: Option<crate::selfdestruct::SelfDestructAssets>,
    /// Mine extraction allocation dialog (`ALLOCFRM` + `BARRAW`/`BARFUEL`/
    /// `BARGOLD` + `BARTAPE` + `LFARO_OF`/`RTARO_OF` + `MNUBTN*`), C++
    /// `AllocMenu`.
    pub alloc: Option<crate::allocmenu::AllocMenuAssets>,
    /// X-FER window (`XFERPIC` + `XFR*` + `SMBRRAW/FUEL/GOLD`), C++ `TransferMenu`.
    pub transfer: Option<crate::transfermenu::TransferMenuAssets>,
    /// Mobilization screen (`CARGOPIC` + `CRG*` + `BLD*`/`SEL*` + `VERT*` + `I_*`).
    pub cargo: Option<crate::mobilize::CargoMenuAssets>,
    /// Intro video: path to `maxint.mve` (the `INTROFLC` resource is an ASCIIZ reference).
    ///
    /// The 71 MB file itself is not read here: `max_app::intro` opens it when
    /// entering `Introduction`, so as not to pay for it at the start and in QA.
    pub intro_mve: Option<std::path::PathBuf>,
    /// Mouse cursors (`Cursor_Descriptor`, 30 resources).
    pub cursors: Option<CursorSet>,
    /// Resource markers on the map (`RAWMSK*`/`FUELMK*`/`GOLDMK*` + small ones).
    pub resource_markers: ResourceMarkers,
    /// Blinking construction markers `BLDMRK1..5` (`DrawMap_BuildMarkImages`,
    /// `drawmap.cpp:49`) — a hint to the player where to take the builder off the site
    /// (`_(276c)`: «click on a square with a symbol in it»).
    pub build_markers: Vec<SimpleImage>,
    /// Planet pictures by groups (4 × 128×128 big image, `SNOW_PIC + world/6`).
    pub planet_pictures: Vec<BigImage>,
    /// Portrait FLC animations: resource tag `F_*` → parsed file
    /// (`Unit::GetFlicsAnimation`, `GameManager_MenuCreateFlic`).
    ///
    /// There are 71 files (that many `flc-ref` records in `MAX.RES`); `Arc` so that
    /// [`Assets::clone`] does not copy ~13 MB of raw FLC bytes.
    flics: BTreeMap<String, Arc<Flic>>,
    sprites: Vec<LoadedSprite>,
    warnings: Vec<String>,
}

impl Assets {
    /// Loads the mandatory data and sprites from [`UNIT_SPRITES`].
    ///
    /// # Errors
    ///
    /// `MAX.RES`/`PATCHES.RES`/`COLOR.PAL` are unreadable or there is no planet map
    /// (neither a `.WRL` file nor an archive resource) — [`Err`] with context
    /// (path/resource name). A missing sprite — only a record in
    /// [`Self::warnings`].
    pub fn load(config: &AppConfig) -> Result<Self> {
        // TC-1109: `MAX.RES` is also searched from the exe directory/upwards, not only by
        // the config (launching `bin\release\max_app.exe` from a foreign CWD).
        let (max_res_path, max_res_warning) = resolve_max_res(config);
        let max_res = ResArchive::open(&max_res_path).with_context(|| {
            format!(
                "не удалось открыть {} (искали также рядом с exe; проверьте --data-dir/--max-res)",
                max_res_path.display()
            )
        })?;
        // Assets and data_hash are separate: `config.patches_res` may deliberately be
        // missing (a server without PATCHES.RES), then the archive is taken from the
        // fallback, while the hash is still computed by the handshake path.
        let (patches_path, patches_warning) = resolve_patches_res(config);
        let patches_res = ResArchive::open(&patches_path)
            .with_context(|| format!("не удалось открыть {}", patches_path.display()))?;
        let (palette_path, palette_warning) = resolve_color_pal(config);
        let palette = Palette::load(&palette_path)
            .with_context(|| format!("не удалось прочитать {}", palette_path.display()))?;
        let brightness = BrightnessLut::build(&palette);

        let world_name = world_asset_name(config.planet);
        // The original game keeps `.WRL` as separate files next to `MAX.RES`
        // (the archive contains only `snow_1.w`/`green_1.` strings — file names).
        // But some builds pack `.WRL` inside `MAX.RES`, so the order is
        // this: a file in `world_dir` → an archive resource.
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

        // 1:1 window chrome (`FRAMEPIC` + panels) — optional: without it the renderer
        // works in the former full-screen mode.
        let chrome = crate::chrome::Chrome::load(&max_res);

        // Game system palette: slots 0..63 and 160..255 — the window palette
        // (`FRAMEPIC`; C++ `WindowManager_LoadBigImage(..., palette_from_image=true)`),
        // slots 64..159 — the world palette (`World::ApplyPaletteAndTints`,
        // `world.cpp:329`: the `.WRL` components are divided by 4, i.e. remain
        // 6-bit). It is these slots that the palette cycles rotate
        // (`World::InitColorCycles`), so sprites are drawn with the system
        // palette, while `COLOR.PAL` remains the source of the brightness tables.
        // Without `FRAMEPIC` (e.g. synthetic tests) `COLOR.PAL` is taken.
        let system_palette = {
            let ui_palette = chrome
                .as_ref()
                .map(|chrome| chrome.frame.palette)
                .or_else(|| {
                    max_res
                        .read("FRAMEPIC")
                        .ok()
                        .and_then(|bytes| BigImage::decode(bytes).ok().map(|image| image.palette))
                });
            let fallback = palette.to_rgb_bytes();
            let mut bytes = ui_palette
                .as_ref()
                .map(|entries| entries.to_vec())
                .filter(|entries| entries.len() >= max_render::palette::PALETTE_BYTES)
                .unwrap_or(fallback);

            let world_palette = world.palette();

            for index in 64..160 {
                let source = (index - 64) * 3;

                bytes[index * 3..index * 3 + 3]
                    .copy_from_slice(&world_palette[source..source + 3]);
            }

            Palette::from_rgb8(&bytes).unwrap_or_else(|_| palette.clone())
        };

        let panels = crate::panels::Panels::load(&max_res);
        let menu = crate::menu::MenuAssets::load(&max_res);
        let build_menu = crate::buildmenu::BuildMenuAssets::load(&max_res);
        // V70 save/load window (`LOADPIC` + `FILE*_UP/DN` + buttons), P3.
        let save_load = crate::saveload::SaveLoadAssets::load(&max_res);
        // Upgrades screen (`UPGRADE` + `UPG*` + `BLD*` + `SEL*`), P6.
        let upgrade = crate::upgrademenu::UpgradeMenuAssets::load(&max_res);
        // Research screen (`RSRCHPIC` + `I_*` icons + arrows + slider).
        let research = crate::researchmenu::ResearchMenuAssets::load(&max_res);
        // Help window (`HELPFRAM` + `HELPOK` + scrolling) — all Help screens.
        let help = crate::helpmenu::HelpMenuAssets::load(&max_res);
        // `Preferences`/`Setup` dialog (`PREFSPIC` + `PRF*` + checkboxes + fields).
        let preferences = crate::preferences::PreferencesAssets::load(&max_res);
        // Unit command menu (`UNTBTN_U`/`UNTBTN_D`, 42×21).
        let unit_menu = crate::unitmenu::UnitMenuAssets::load(&max_res);
        // Self-destruct dialog (`SELFDSTR` + `SLFDAR`/`SLFDCN`/`SLFDOPN`/`SLFDOK`).
        let self_destruct = crate::selfdestruct::SelfDestructAssets::load(&max_res);
        // Mine extraction allocation dialog (`ALLOCFRM` + `BAR*` + `LFARO/RTARO`).
        let alloc = crate::allocmenu::AllocMenuAssets::load(&max_res);
        // X-FER window (`XFERPIC` + `XFR*` + `SMBR*`), C++ `TransferMenu`.
        let transfer = crate::transfermenu::TransferMenuAssets::load(&max_res);
        // Mobilization screen (`CARGOPIC` + `CRG*` + bars + `I_*` icons).
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

        if !research.is_ready() {
            warnings.push(
                "ResearchMenu: RSRCHPIC/DPTBAY/PRFSLIDE не загрузились — экран на рамках"
                    .to_string(),
            );
        }

        if !help.is_ready() {
            warnings.push(
                "HelpMenu: HELPFRAM/HELPOK не загрузились — окно помощи на рамках".to_string(),
            );
        }

        if !preferences.is_ready() {
            warnings.push(
                "Preferences: PREFSPIC/PRFSLIT/PRFSLIDE/UNCHKED/CHECKED/PRF* не загрузились — диалог на рамках"
                    .to_string(),
            );
        }

        if !unit_menu.is_ready() {
            warnings.push(
                "UnitMenu: UNTBTN_U/UNTBTN_D не загрузились — меню команд не рисуется"
                    .to_string(),
            );
        }

        if !alloc.is_ready() {
            warnings.push(
                "AllocMenu: ALLOCFRM/BARRAW/BARTAPE не загрузились — распределение шахты не рисуется"
                    .to_string(),
            );
        }

        if !transfer.is_ready() {
            warnings.push(
                "TransferMenu: XFERPIC/XFR*/SMBRRAW не загрузились — окно x-fer не рисуется"
                    .to_string(),
            );
        }

        if !save_load.is_ready() {
            warnings.push(
                "SaveLoad: LOADPIC/FILE*_UP не загрузились — окно сохранения на рамках".to_string(),
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

        // Cursors: 30 `Cursor_ResourceLut` resources (`cursor.cpp:55-58`).
        let cursors = CursorSet::load(&max_res);
        if cursors.loaded() < max_assets::CURSOR_COUNT {
            warnings.push(format!(
                "курсоры: загрузилось {}/{} (нет: {})",
                cursors.loaded(),
                max_assets::CURSOR_COUNT,
                cursors.missing().join(", ")
            ));
        }

        // Resource markers: real map sprites instead of colored dots.
        let resource_markers = ResourceMarkers::load(&max_res);
        if !resource_markers.is_ready() {
            warnings.push("маркеры ресурсов (RAWMSK*/FUELMK*/GOLDMK*) не загрузились".to_string());
        }

        // Construction markers `BLDMRK1..5` (`DrawMap_BuildMarkImages`): 62×62,
        // drawn at the cell center (hotspot `ulx/uly` from the resource header).
        let mut build_markers = Vec::with_capacity(5);
        for index in 1..=5 {
            let name = format!("BLDMRK{index}");

            match max_res
                .read(&name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
            {
                Some(image) => build_markers.push(image),
                None => warnings.push(format!("маркер стройки {name} не загрузился")),
            }
        }

        // Planet pictures `SNOW_PIC + world/6` — for the planet selection screen.
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

        // Portrait FLC animations (`WINDOW_CORNER_FLIC`, 128×128). In `MAX.RES`
        // 71 `F_*` records, whose body is an ASCIIZ name of a `*.FLC` next to the archive
        // (like the `*.SPW` references): we load the file from disk, the frames are decoded by
        // `max_assets::flic`.
        let flic_dir = max_res_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let flc_files = flc_index(&flic_dir);
        let mut flics: BTreeMap<String, Arc<Flic>> = BTreeMap::new();
        let mut missing_flics: Vec<String> = Vec::new();

        for (unit_type, ..) in UNIT_SPRITES {
            let Some(tag) = crate::cargo_unit_data::unit_flic(unit_type) else {
                continue;
            };

            if flics.contains_key(tag) {
                continue;
            }

            let Some(file_name) = max_res.file_reference(tag) else {
                missing_flics.push(format!("{tag}: нет записи-ссылки в MAX.RES"));
                continue;
            };

            let Some(path) = flc_files.get(&file_name.to_ascii_lowercase()) else {
                missing_flics.push(format!("{tag}: нет файла {file_name} рядом с MAX.RES"));
                continue;
            };

            match Flic::open(path) {
                Ok(flic) => {
                    flics.insert(tag.to_string(), Arc::new(flic));
                }
                Err(error) => missing_flics.push(format!("{tag} ({file_name}): {error}")),
            }
        }

        if !missing_flics.is_empty() {
            warnings.push(format!(
                "FLC-анимации портретов: загрузилось {}/{} ({})",
                flics.len(),
                flics.len() + missing_flics.len(),
                missing_flics.join("; ")
            ));
        }

        // Intro video: `INTROFLC` — a reference to `maxint.mve` next to MAX.RES.
        // The file is not read here (71 MB); the `Introduction` screen will open it itself.
        let mve_files = mve_index(&flic_dir);
        let intro_mve = max_res
            .file_reference("INTROFLC")
            .and_then(|file_name| mve_files.get(&file_name.to_ascii_lowercase()).cloned());

        if intro_mve.is_none() && max_res.file_reference("INTROFLC").is_some() {
            warnings.push("Intro: INTROFLC есть в MAX.RES, но файла *.mve рядом нет".to_string());
        }

        Ok(Self {
            max_res,
            chrome,
            panels,
            menu,
            build_menu: Some(build_menu),
            save_load: Some(save_load),
            upgrade: Some(upgrade),
            research: Some(research),
            help: Some(help),
            preferences: Some(preferences),
            unit_menu: Some(unit_menu),
            self_destruct: Some(self_destruct),
            alloc: Some(alloc),
            transfer: Some(transfer),
            cargo: Some(cargo),
            intro_mve,
            cursors: Some(cursors),
            resource_markers,
            build_markers,
            planet_pictures,
            flics,
            patches_res,
            palette,
            system_palette,
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

    /// Sprite for a unit type (if it was in the archive and parsed).
    pub fn sprite(&self, unit_type: UnitType) -> Option<&LoadedSprite> {
        self.sprites
            .iter()
            .find(|sprite| sprite.unit_type == unit_type)
    }

    /// FLC portrait animation of a unit type (`Unit::GetFlicsAnimation`).
    ///
    /// `None` — the type has no `F_*` resource (22 types: debris, explosions, missiles,
    /// `HARVSTER`, `WALDO`) or the file did not load; then `panels::draw_portrait`
    /// draws a static sprite (fallback).
    pub fn flic(&self, unit_type: UnitType) -> Option<&Arc<Flic>> {
        let tag = crate::cargo_unit_data::unit_flic(unit_type)?;

        self.flics.get(tag)
    }

    /// How many FLC animations were loaded (the reference — 71).
    pub fn flic_count(&self) -> usize {
        self.flics.len()
    }

    /// Planet group picture `SNOW_PIC + world/6` (`planetselectmenu.cpp:94`).
    ///
    /// Planets of one group (6 in a row) share a 128×128 picture; its own
    /// `ulx/uly` (26,98 for `SNOW_PIC`) — the position on the selection screen.
    pub fn planet_picture(&self, planet: Planet) -> Option<&BigImage> {
        self.planet_pictures.get(planet as usize / 6)
    }

    /// All loaded sprites in [`UNIT_SPRITES`] order.
    pub fn sprites(&self) -> &[LoadedSprite] {
        &self.sprites
    }

    /// Non-critical load problems.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Report on the loaded data (without re-reading the files).
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
            flics: self.flics.len(),
            warnings: self.warnings.clone(),
        }
    }
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;
