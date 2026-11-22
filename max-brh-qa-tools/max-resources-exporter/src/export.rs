//! Walks all 93 unit types and writes the `units/<TAG>/` directories.
//!
//! One type — one directory (`out_root` = `max-brh-assets/units`):
//!
//! ```text
//! max-brh-assets/units/TANK/
//! ├── sprites/          frame_XXX.png (body), sheet.png (strip), sprite.json
//! ├── turret/           turret frames (if any)
//! ├── firing/           firing frames (if any)
//! ├── connectors/       connector frames (if any)
//! ├── shadow/           shadow frames S_*
//! ├── portrait.png      P_* 300×240 (window palette) or a fallback from the sprite
//! ├── animation/        frame_000.png…, animation.gif, a copy of the source .FLC
//! └── unit.json         type/name/flags/sources/sizes/animation presence
//! ```
//!
//! Plus two root files in `out_root` (`max-brh-assets/units`):
//! `manifest.csv` (one row per type) and `README.md` (what was exported, how to
//! regenerate it, statistics).
//!
//! ## Idempotency
//!
//! The unit directory is synced against the **set of expected files**: the bytes are
//! built up front and written over the old ones, while files outside the expected set are deleted
//! together with the emptied subdirectories. Directories of types that are not among the 93
//! are removed from `out_root` entirely. Same input → same bytes (a PNG codec without
//! timestamps, a GIF without a frame counter, JSON with a fixed key order),
//! so a repeated run only rewrites the files.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use max_assets::flic::Flic;
use max_assets::image::BigImage;
use max_assets::res::ResArchive;
use max_assets::sprite::{decode_shadow, decode_sprite, FrameInfo, ShadowSheet, SpriteFrame, SpriteSheet};
use max_render::palette::Palette;
use serde_json::{json, Value};

use crate::gif::{self, GifFrame};
use crate::names::display_name;
use crate::png::{encode_auto, RgbaImage};
use crate::render::{
    big_image_image, fallback_scale, flic_frame_image, flic_palette, portrait_fallback,
    shadow_frame_image, sprite_frame_image, sprite_meta, sprite_strip, strip_dimensions,
    window_palette, PORTRAIT_HEIGHT, PORTRAIT_WIDTH,
};
use crate::units::{verify_sc_units, CargoUnit, ScUnitsCheck};

/// Where the game data lives and where to write the export.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// `MAX.RES` (sprites, shadows, portraits, `D_*`, `F_*` references).
    pub max_res: PathBuf,
    /// `COLOR.PAL` (sprite palette).
    pub color_pal: PathBuf,
    /// Directory with the `*.FLC` files (usually next to `MAX.RES`).
    pub flc_dir: PathBuf,
    /// `cargo_unit_data.json` — the type → resources mapping (a snapshot of `SC_UNITS`).
    pub units_json: PathBuf,
    /// Export root (`max-brh-assets/units`): the `<TAG>/` directories go here, along with
    /// `manifest.csv` and `README.md`. There is no `assets-base/` level.
    pub out_root: PathBuf,
    /// `PATCHES.RES` used to cross-check the mapping against `SC_UNITS`; `None` — do not check.
    pub patches: Option<PathBuf>,
    /// Restrict the token set (debugging/tests); `None` — all 93.
    pub only: Option<Vec<String>>,
}

impl ExportOptions {
    /// Default paths relative to the repository root.
    pub fn defaults(repo_root: impl AsRef<Path>) -> Self {
        let root = repo_root.as_ref();

        Self {
            max_res: root.join("max-original/MAX.RES"),
            color_pal: root.join("max-original/COLOR.PAL"),
            flc_dir: root.join("max-original"),
            units_json: root.join("max-brh/crates/max_app/data/cargo_unit_data.json"),
            out_root: root.join("max-brh-assets/units"),
            patches: default_patches(root),
            only: None,
        }
    }
}

/// Default `PATCHES.RES`: the historical `assets/` (if still present) or
/// the shared C++ port directory `max-cpp-port/assets/` (a fallback in
/// `max_app::assets`). `None` — there is nothing to check against `SC_UNITS`.
fn default_patches(root: &Path) -> Option<PathBuf> {
    [
        root.join("assets/PATCHES.RES"),
        root.join("max-cpp-port/assets/PATCHES.RES"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// Where the portrait came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortraitSource {
    /// `P_*` unchanged (window palette).
    Resource,
    /// Fallback: the first sprite frame, upscaled by an integer factor.
    Fallback,
    /// No portrait and no sprite (does not occur on real data).
    None,
}

impl PortraitSource {
    /// Row for the manifest and `unit.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resource => "resource",
            Self::Fallback => "fallback",
            Self::None => "none",
        }
    }
}

/// Report for a single unit type.
#[derive(Debug, Clone)]
pub struct UnitReport {
    /// The `SC_UNITS` token (`TANK`).
    pub token: String,
    /// `UnitType` index (0..92).
    pub unit_type: u16,
    /// English name (or the token for service types).
    pub name: String,
    /// The type is a building (`unit_flags::BUILDING`).
    pub building: bool,
    /// Body sprite tag.
    pub sprite_tag: Option<String>,
    /// Shadow tag `S_*`.
    pub shadow_tag: Option<String>,
    /// Portrait tag `P_*`.
    pub portrait_tag: Option<String>,
    /// FLC tag `F_*`.
    pub flc_tag: Option<String>,
    /// Body frames exported.
    pub body_frames: usize,
    /// Turret frames.
    pub turret_frames: usize,
    /// Firing frames.
    pub firing_frames: usize,
    /// Connector frames.
    pub connector_frames: usize,
    /// Shadow frames.
    pub shadow_frames: usize,
    /// Portrait source.
    pub portrait_source: PortraitSource,
    /// Portrait width (PNG).
    pub portrait_width: u32,
    /// Portrait height.
    pub portrait_height: u32,
    /// Name of the copied `.FLC`.
    pub flc_file: Option<String>,
    /// FLC frames.
    pub flc_frames: usize,
    /// Body frame width (the first frame).
    pub sprite_width: u32,
    /// Body frame height.
    pub sprite_height: u32,
    /// How many PNGs were written to the unit directory.
    pub pngs: usize,
    /// Total size of the directory's files, in bytes.
    pub bytes: u64,
    /// Notes (no shadow/portrait/FLC, truncated frames, etc.).
    pub notes: Vec<String>,
}

/// Export result.
#[derive(Debug, Clone)]
pub struct ExportReport {
    /// Where the export went (`max-brh-assets/units`); the unit directories sit right here.
    pub out_root: PathBuf,
    /// Per-type reports in `UnitType` order.
    pub units: Vec<UnitReport>,
    /// Total PNGs in the export.
    pub pngs: usize,
    /// Total bytes in the unit directories.
    pub bytes: u64,
    /// Errors (the type was skipped, the rest were processed).
    pub errors: Vec<String>,
    /// Cross-check against `SC_UNITS` (if requested).
    pub sc_units: Option<ScUnitsCheck>,
}

impl ExportReport {
    /// How many types have a `P_*` portrait (not a fallback).
    pub fn portraits(&self) -> usize {
        self.units
            .iter()
            .filter(|unit| unit.portrait_source == PortraitSource::Resource)
            .count()
    }

    /// How many types have a fallback portrait.
    pub fn portrait_fallbacks(&self) -> usize {
        self.units
            .iter()
            .filter(|unit| unit.portrait_source == PortraitSource::Fallback)
            .count()
    }

    /// How many types have an FLC animation.
    pub fn animations(&self) -> usize {
        self.units
            .iter()
            .filter(|unit| unit.flc_frames > 0)
            .count()
    }

    /// How many distinct `.FLC` files were copied.
    pub fn flc_files(&self) -> usize {
        self.units
            .iter()
            .filter_map(|unit| unit.flc_file.as_deref())
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// Tokens without a `P_*` portrait.
    pub fn tokens_without_portrait(&self) -> Vec<&str> {
        self.units
            .iter()
            .filter(|unit| unit.portrait_source != PortraitSource::Resource)
            .map(|unit| unit.token.as_str())
            .collect()
    }

    /// Tokens without an FLC.
    pub fn tokens_without_animation(&self) -> Vec<&str> {
        self.units
            .iter()
            .filter(|unit| unit.flc_frames == 0)
            .map(|unit| unit.token.as_str())
            .collect()
    }

    /// Tokens with no `portrait.png` at all (neither `P_*` nor a sprite).
    pub fn tokens_without_portrait_png(&self) -> Vec<&str> {
        self.units
            .iter()
            .filter(|unit| unit.portrait_source == PortraitSource::None)
            .map(|unit| unit.token.as_str())
            .collect()
    }

    /// `(body, turret, firing, connectors, shadow)` — total frame counts.
    pub fn frame_totals(&self) -> (usize, usize, usize, usize, usize) {
        let mut totals = (0, 0, 0, 0, 0);

        for unit in &self.units {
            totals.0 += unit.body_frames;
            totals.1 += unit.turret_frames;
            totals.2 += unit.firing_frames;
            totals.3 += unit.connector_frames;
            totals.4 += unit.shadow_frames;
        }

        totals
    }
}

/// What to write into the unit directory and under which names.
struct UnitFiles {
    files: Vec<(String, Vec<u8>)>,
}

impl UnitFiles {
    fn new() -> Self {
        Self { files: Vec::new() }
    }

    fn png(&mut self, relative: impl Into<String>, image: &RgbaImage) {
        self.files.push((relative.into(), encode_auto(image)));
    }

    fn raw(&mut self, relative: impl Into<String>, bytes: Vec<u8>) {
        self.files.push((relative.into(), bytes));
    }

    fn json(&mut self, relative: impl Into<String>, value: &Value) {
        let mut text = serde_json::to_string_pretty(value).expect("JSON сериализуется");

        text.push('\n');
        self.files.push((relative.into(), text.into_bytes()));
    }
}

/// Exports every type from `cargo_unit_data.json` into `out_root`.
pub fn export_all(options: &ExportOptions) -> Result<ExportReport> {
    let units = crate::units::load_cargo_unit_data(&options.units_json)?;

    let archive = ResArchive::open(&options.max_res)
        .with_context(|| format!("открытие {}", options.max_res.display()))?;
    let palette = Palette::load(&options.color_pal)
        .with_context(|| format!("палитра {}", options.color_pal.display()))?;

    // Window palette (`FRAMEPIC`): the C++ port draws portraits and FLC frames with it.
    let window = match archive.read("FRAMEPIC") {
        Ok(bytes) => {
            let image = BigImage::decode(bytes).map_err(|error| anyhow!("FRAMEPIC: {error}"))?;

            window_palette(&image)
        }
        Err(_) => {
            // Reserve: the sprite palette (matches P_* almost element by element).
            let mut colors = [[0u8; 3]; 256];

            for (index, color) in colors.iter_mut().enumerate() {
                *color = palette.rgb(index as u8);
            }

            colors
        }
    };

    let sc_units = match &options.patches {
        Some(path) if path.exists() => {
            let check = verify_sc_units(path, &units)?;

            if !check.is_ok() {
                bail!(
                    "{}: маппинг {} расходится с SC_UNITS ({} расхождений): {}",
                    path.display(),
                    options.units_json.display(),
                    check.mismatches.len(),
                    check.mismatches.join("; ")
                );
            }

            Some(check)
        }
        _ => None,
    };

    // `out_root` is already the unit directory (`max-brh-assets/units`): no `assets-base/`.
    fs::create_dir_all(&options.out_root)
        .with_context(|| format!("создание {}", options.out_root.display()))?;

    let only: Option<HashSet<&str>> = options
        .only
        .as_ref()
        .map(|tokens| tokens.iter().map(String::as_str).collect());

    let mut report = ExportReport {
        out_root: options.out_root.clone(),
        units: Vec::new(),
        pngs: 0,
        bytes: 0,
        errors: Vec::new(),
        sc_units,
    };

    for (index, unit) in units.iter().enumerate() {
        if let Some(only) = &only {
            if !only.contains(unit.token.as_str()) {
                continue;
            }
        }

        match export_unit(&archive, &palette, &window, options, unit, index) {
            Ok(unit_report) => {
                report.pngs += unit_report.pngs;
                report.bytes += unit_report.bytes;
                report.units.push(unit_report);
            }
            Err(error) => report.errors.push(format!("{}: {error:#}", unit.token)),
        }
    }

    // We drop directories of types that are not in the set (stale ones/typos).
    if only.is_none() {
        remove_stale_unit_dirs(&options.out_root, &report.units)?;
    }

    write_manifest(&options.out_root.join("manifest.csv"), &report)?;
    write_readme(&options.out_root.join("README.md"), &report)?;

    Ok(report)
}

/// Frames of one kind (body/turret/firing/connectors) from `D_*`.
struct FrameRanges {
    body: std::ops::Range<usize>,
    turret: std::ops::Range<usize>,
    firing: std::ops::Range<usize>,
    connectors: std::ops::Range<usize>,
    info: FrameInfo,
}

impl FrameRanges {
    /// Parses [`FrameInfo`] and clamps the ranges to the sprite frame count.
    ///
    /// `image_count == 0` for the body means "the whole sprite" (that is how `D_*` behaves without
    /// the fields), while for turret/firing/connectors it means "no frames".
    fn new(info: FrameInfo, total: usize) -> Self {
        let body = if info.image_count == 0 {
            0..total
        } else {
            clamp_range(info.image_base, info.image_count, total)
        };

        Self {
            body,
            turret: clamp_range(info.turret_image_base, info.turret_image_count, total),
            firing: clamp_range(info.firing_image_base, info.firing_image_count, total),
            connectors: clamp_range(info.connector_image_base, info.connector_image_count, total),
            info,
        }
    }
}

/// `base..base + count` clamped to `total`; `count == 0` — empty.
fn clamp_range(base: u8, count: u8, total: usize) -> std::ops::Range<usize> {
    let start = usize::from(base).min(total);
    let end = (usize::from(base) + usize::from(count)).min(total);

    start..end.max(start)
}

/// Export of a single type.
fn export_unit(
    archive: &ResArchive,
    palette: &Palette,
    window: &[[u8; 3]; 256],
    options: &ExportOptions,
    unit: &CargoUnit,
    index: usize,
) -> Result<UnitReport> {
    let unit_type = u16::try_from(index).expect("93 типа");
    let unit_kind = max_core::enums::UnitType::try_from(unit_type)
        .map_err(|_| anyhow!("тип {unit_type} вне UnitType"))?;
    let flags = max_core::access::unit_type_flags(unit_kind);
    let building = flags & max_core::enums::unit_flags::BUILDING != 0;
    let name = display_name(index, &unit.token).to_string();
    let mut files = UnitFiles::new();
    let mut notes = Vec::new();

    // --- sprite: body, turret, firing, connectors -----------------------
    let mut body_frames = 0;
    let mut turret_frames = 0;
    let mut firing_frames = 0;
    let mut connector_frames = 0;
    let mut shadow_frames = 0;
    let mut sprite_width = 0u32;
    let mut sprite_height = 0u32;

    let sprite_sheet: Option<SpriteSheet> = match &unit.sprite {
        Some(tag) => match archive.read(tag) {
            Ok(bytes) => match decode_sprite(bytes) {
                Ok(sheet) => Some(sheet),
                Err(error) => {
                    notes.push(format!("спрайт {tag}: {error}"));

                    None
                }
            },
            Err(_) => {
                // Types cut from the game (`MASTER`): the tag exists in `SC_UNITS`,
                // but the resource is no longer in `MAX.RES` — we create the directory anyway.
                notes.push(format!("спрайт {tag}: ресурса нет в MAX.RES"));

                None
            }
        },
        None => {
            notes.push("SC_UNITS: sprite = INVALID_ID".to_string());

            None
        }
    };

    let frame_info = match &unit.data {
        Some(tag) => match archive.read(tag) {
            Ok(bytes) => FrameInfo::decode(bytes),
            Err(error) => {
                notes.push(format!("D_* {tag}: {error}"));

                None
            }
        },
        None => {
            notes.push("SC_UNITS: data = INVALID_ID (кадры — весь спрайт)".to_string());

            None
        }
    };

    let mut sprite_json: Option<Value> = None;

    if let Some(sheet) = &sprite_sheet {
        let total = sheet.len();
        let ranges = FrameRanges::new(frame_info.unwrap_or_default(), total);

        if total < usize::from(ranges.info.image_base) + usize::from(ranges.info.image_count) {
            notes.push(format!(
                "D_*: image_base+image_count = {} > кадров в спрайте {total} — диапазон обрезан",
                usize::from(ranges.info.image_base) + usize::from(ranges.info.image_count)
            ));
        }

        for (dir, range, counter) in [
            ("sprites", &ranges.body, &mut body_frames),
            ("turret", &ranges.turret, &mut turret_frames),
            ("firing", &ranges.firing, &mut firing_frames),
            ("connectors", &ranges.connectors, &mut connector_frames),
        ] {
            for frame_index in range.clone() {
                files.png(
                    format!("{dir}/frame_{frame_index:03}.png"),
                    &sprite_frame_image(&sheet.frames[frame_index], palette),
                );
                *counter += 1;
            }
        }

        if body_frames == 0 {
            notes.push(format!(
                "корпус: пустой диапазон кадров (image_base={}, image_count={}, в спрайте {total})",
                ranges.info.image_base, ranges.info.image_count
            ));
        }

        let body: Vec<&SpriteFrame> = ranges.body.clone().map(|i| &sheet.frames[i]).collect();

        if let Some(first) = body.first() {
            sprite_width = u32::from(first.width);
            sprite_height = u32::from(first.height);

            let sheet_image = sprite_strip(&body, palette);
            let (sheet_width, sheet_height) = strip_dimensions(&sprite_meta(&body));

            files.png("sprites/sheet.png", &sheet_image);

            sprite_json = Some(json!({
                "unit": unit.token,
                "sprite_resource": unit.sprite,
                "data_resource": unit.data,
                "image_base": ranges.info.image_base,
                "image_count": ranges.info.image_count,
                "turret_image_base": ranges.info.turret_image_base,
                "turret_image_count": ranges.info.turret_image_count,
                "firing_image_base": ranges.info.firing_image_base,
                "firing_image_count": ranges.info.firing_image_count,
                "connector_image_base": ranges.info.connector_image_base,
                "connector_image_count": ranges.info.connector_image_count,
                "angle_offsets": ranges.info.angle_offsets
                    .iter()
                    .map(|(dx, dy)| [*dx, *dy])
                    .collect::<Vec<[i8; 2]>>(),
                "sheet_image_count": total,
                "sheet": {
                    "file": "sheet.png",
                    "width": sheet_width,
                    "height": sheet_height,
                    "frames": body.len(),
                    "layout": "horizontal_strip_by_hotspot",
                },
                "frame_size": [sprite_width, sprite_height],
                "hotspot": [
                    body[0].hotx,
                    body[0].hoty,
                ],
                "dirs": {
                    "sprites": ranges.body.clone().count(),
                    "turret": ranges.turret.clone().count(),
                    "firing": ranges.firing.clone().count(),
                    "connectors": ranges.connectors.clone().count(),
                },
                "palette": "COLOR.PAL (6-bit * 4)",
                "frames": collect_frames_json(sheet, &ranges),
            }));
        }
    }

    // --- shadow ---------------------------------------------------------
    let shadow_sheet: Option<ShadowSheet> = match &unit.shadow {
        Some(tag) => match archive.read(tag) {
            Ok(bytes) => match decode_shadow(bytes) {
                Ok(sheet) => Some(sheet),
                Err(error) => {
                    notes.push(format!("тень {tag}: {error}"));

                    None
                }
            },
            Err(error) => {
                notes.push(format!("тень {tag}: {error}"));

                None
            }
        },
        None => {
            notes.push("SC_UNITS: shadow = INVALID_ID (тени нет)".to_string());

            None
        }
    };

    let mut shadow_json = Value::Null;

    if let Some(sheet) = &shadow_sheet {
        for (frame_index, frame) in sheet.frames.iter().enumerate() {
            files.png(
                format!("shadow/frame_{frame_index:03}.png"),
                &shadow_frame_image(frame),
            );
            shadow_frames += 1;
        }

        if let Some(first) = sheet.frames.first() {
            shadow_json = json!({
                "resource": unit.shadow,
                "dir": "shadow",
                "frames": sheet.len(),
                "frame_size": [first.width, first.height],
                "hotspot": [first.hotx, first.hoty],
            });
        }
    }

    if let Some(json) = sprite_json.as_mut() {
        json["shadow"] = shadow_json.clone();
    }

    // --- portrait -------------------------------------------------------
    let mut portrait_source = PortraitSource::None;
    let mut portrait_width = 0;
    let mut portrait_height = 0;
    let mut portrait_scale = 0;

    if let Some(tag) = &unit.portrait {
        match archive.read(tag) {
            Ok(bytes) => match BigImage::decode(bytes) {
                Ok(image) => {
                    let portrait = big_image_image(&image, window);

                    portrait_width = portrait.width;
                    portrait_height = portrait.height;
                    portrait_source = PortraitSource::Resource;

                    if (portrait.width, portrait.height) != (PORTRAIT_WIDTH, PORTRAIT_HEIGHT) {
                        notes.push(format!(
                            "портрет {tag}: размер {}×{} вместо {PORTRAIT_WIDTH}×{PORTRAIT_HEIGHT}",
                            portrait.width, portrait.height
                        ));
                    }

                    files.png("portrait.png", &portrait);
                }
                Err(error) => notes.push(format!("портрет {tag}: {error}")),
            },
            Err(error) => notes.push(format!("портрет {tag}: {error}")),
        }
    }

    if portrait_source == PortraitSource::None {
        let mut made_fallback = false;

        if let Some(sheet) = &sprite_sheet {
            let ranges = FrameRanges::new(frame_info.unwrap_or_default(), sheet.len());

            if let Some(frame) = ranges.body.clone().next().map(|i| &sheet.frames[i]) {
                let portrait = portrait_fallback(frame, palette);

                portrait_width = portrait.width;
                portrait_height = portrait.height;
                portrait_scale = fallback_scale(frame);
                portrait_source = PortraitSource::Fallback;
                made_fallback = true;

                files.png("portrait.png", &portrait);
                notes.push(format!(
                    "портрет: P_* нет — fallback «первый кадр спрайта ×{portrait_scale}»"
                ));
            }
        }

        if !made_fallback {
            notes.push("портрет: нет ни P_*, ни спрайта — portrait.png не создан".to_string());
        }
    }

    // --- FLC animation --------------------------------------------------
    let mut flc_file = None;
    let mut flc_frames = 0;

    if let Some(tag) = &unit.flics {
        match export_animation(archive, options, tag, &mut files, &mut notes) {
            Ok((file, frames)) => {
                flc_file = Some(file);
                flc_frames = frames;
            }
            Err(error) => notes.push(format!("FLC {tag}: {error:#}")),
        }
    } else {
        notes.push("SC_UNITS: flics_animation = INVALID_ID (анимации нет)".to_string());
    }

    // --- sprite.json / unit.json ---------------------------------------
    if let Some(sprite_json) = &sprite_json {
        files.json("sprites/sprite.json", sprite_json);
    }

    let animation_json = if flc_frames > 0 {
        json!({
            "dir": "animation",
            "frames": flc_frames,
            "gif": "animation/animation.gif",
            "source_file": flc_file,
            "first_frame": "animation/frame_000.png",
        })
    } else {
        Value::Null
    };
    let portrait_file = if portrait_source == PortraitSource::None {
        Value::Null
    } else {
        json!("portrait.png")
    };
    // Sprite files exist only if the sprite was really exported (`MASTER` — no).
    let (sheet_file, sprite_json_file) = if sprite_json.is_some() {
        (json!("sprites/sheet.png"), json!("sprites/sprite.json"))
    } else {
        (Value::Null, Value::Null)
    };
    let kind = if building { "building" } else { "mobile" };

    let unit_json = json!({
        "token": unit.token,
        "unit_type": unit_type,
        "name": name,
        "kind": kind,
        "flags": {
            "building": building,
            "stationary": flags & max_core::enums::unit_flags::STATIONARY != 0,
            "mobile_land": flags & max_core::enums::unit_flags::MOBILE_LAND_UNIT != 0,
            "mobile_air": flags & max_core::enums::unit_flags::MOBILE_AIR_UNIT != 0,
            "mobile_sea": flags & max_core::enums::unit_flags::MOBILE_SEA_UNIT != 0,
            "turret_sprite": flags & max_core::enums::unit_flags::TURRET_SPRITE != 0,
            "spinning_turret": flags & max_core::enums::unit_flags::SPINNING_TURRET != 0,
        },
        "sources": {
            "sprite": unit.sprite,
            "shadow": unit.shadow,
            "data": unit.data,
            "portrait": unit.portrait,
            "flics": unit.flics,
        },
        "sprite": {
            "dir": "sprites",
            "sheet": sheet_file,
            "sprite_json": sprite_json_file,
            "frames_body": body_frames,
            "frames_turret": turret_frames,
            "frames_firing": firing_frames,
            "frames_connectors": connector_frames,
            "frames_shadow": shadow_frames,
            "frame_width": sprite_width,
            "frame_height": sprite_height,
        },
        "portrait": {
            "file": portrait_file,
            "source": portrait_source.as_str(),
            "resource": unit.portrait,
            "width": portrait_width,
            "height": portrait_height,
            "scale": portrait_scale,
            "palette": "window (FRAMEPIC)",
        },
        "animation": animation_json,
        "notes": notes.clone(),
    });

    files.json("unit.json", &unit_json);

    // --- writing to disk ------------------------------------------------
    let dir = options.out_root.join(&unit.token);
    let (pngs, bytes) = sync_unit_dir(&dir, &files)?;

    Ok(UnitReport {
        token: unit.token.clone(),
        unit_type,
        name,
        building,
        sprite_tag: unit.sprite.clone(),
        shadow_tag: unit.shadow.clone(),
        portrait_tag: unit.portrait.clone(),
        flc_tag: unit.flics.clone(),
        body_frames,
        turret_frames,
        firing_frames,
        connector_frames,
        shadow_frames,
        portrait_source,
        portrait_width,
        portrait_height,
        flc_file,
        flc_frames,
        sprite_width,
        sprite_height,
        pngs,
        bytes,
        notes,
    })
}

/// JSON records about the exported frames of all four kinds.
fn collect_frames_json(sheet: &SpriteSheet, ranges: &FrameRanges) -> Vec<Value> {
    let mut frames = Vec::new();

    for (dir, range) in [
        ("sprites", &ranges.body),
        ("turret", &ranges.turret),
        ("firing", &ranges.firing),
        ("connectors", &ranges.connectors),
    ] {
        for frame_index in range.clone() {
            let frame = &sheet.frames[frame_index];

            frames.push(json!({
                "dir": dir,
                "file": format!("{dir}/frame_{frame_index:03}.png"),
                "sheet_index": frame_index,
                "width": frame.width,
                "height": frame.height,
                "hotx": frame.hotx,
                "hoty": frame.hoty,
                "opaque_pixels": frame.opaque_pixels(),
            }));
        }
    }

    frames
}

/// FLC export: PNG frames, `animation.gif`, a copy of the `.FLC`.
fn export_animation(
    archive: &ResArchive,
    options: &ExportOptions,
    tag: &str,
    files: &mut UnitFiles,
    notes: &mut Vec<String>,
) -> Result<(String, usize)> {
    let bytes = archive
        .read(tag)
        .map_err(|error| anyhow!("ресурс {tag}: {error}"))?;
    let reference = max_assets::res::parse_file_ref(bytes)
        .ok_or_else(|| anyhow!("ресурс {tag}: не ссылка на файл"))?;
    let path = find_flc(&options.flc_dir, reference)
        .ok_or_else(|| anyhow!("файл {reference} не найден в {}", options.flc_dir.display()))?;

    let flic = Flic::open(&path).map_err(|error| anyhow!("{}: {error}", path.display()))?;
    let frames = flic
        .decode_all()
        .map_err(|error| anyhow!("{}: {error}", path.display()))?;

    if frames.is_empty() {
        bail!("{}: ни одного кадра", path.display());
    }

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| reference.to_uppercase());
    let speed_ms = flic.speed_ms();
    let delay = gif::delay_from_ms(speed_ms);
    let first_palette = flic_palette(&frames[0].palette);
    let mut local_palettes: Vec<Option<[[u8; 3]; 256]>> = Vec::with_capacity(frames.len());

    for (index, frame) in frames.iter().enumerate() {
        let colors = flic_palette(&frame.palette);
        let image = flic_frame_image(frame, &colors);

        files.png(format!("animation/frame_{index:03}.png"), &image);

        // A local color table — only if the palette differs from frame 0.
        local_palettes.push(if frame.palette == frames[0].palette {
            None
        } else {
            Some(colors)
        });
    }

    let width = u16::try_from(frames[0].width).unwrap_or(u16::MAX);
    let height = u16::try_from(frames[0].height).unwrap_or(u16::MAX);
    let gif_frames: Vec<GifFrame<'_>> = frames
        .iter()
        .zip(local_palettes.iter())
        .map(|(frame, local)| GifFrame {
            indices: &frame.pixels,
            local_palette: local.as_ref(),
            delay_cs: delay,
        })
        .collect();

    files.raw(
        "animation/animation.gif",
        gif::encode(width, height, &first_palette, &gif_frames),
    );
    files.raw(
        format!("animation/{file_name}"),
        fs::read(&path).with_context(|| format!("копия {}", path.display()))?,
    );

    if speed_ms == 0 {
        notes.push(format!(
            "{file_name}: speed = 0 в заголовке FLC — GIF идёт с задержкой {delay} (1/100 с)"
        ));
    }

    Ok((file_name, frames.len()))
}

/// Looks up the FLC file in the directory case-insensitively (`tank03.flc` → `TANK03.FLC`).
pub fn find_flc(dir: &Path, reference: &str) -> Option<PathBuf> {
    let wanted = reference.to_uppercase();
    let direct = dir.join(&wanted);

    if direct.is_file() {
        return Some(direct);
    }

    for entry in fs::read_dir(dir).ok()?.flatten() {
        if entry.file_name().to_string_lossy().to_uppercase() == wanted {
            return Some(entry.path());
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Writing and syncing the directory
// ---------------------------------------------------------------------------

/// Writes the unit files and deletes everything not in the set.
///
/// Returns `(how many PNGs, how many bytes)`.
fn sync_unit_dir(dir: &Path, files: &UnitFiles) -> Result<(usize, u64)> {
    let expected: HashSet<PathBuf> = files
        .files
        .iter()
        .map(|(relative, _)| dir.join(relative))
        .collect();

    if dir.exists() {
        remove_stale(dir, &expected)?;
    }

    fs::create_dir_all(dir).with_context(|| format!("создание {}", dir.display()))?;

    let mut pngs = 0;
    let mut bytes = 0;

    for (relative, data) in &files.files {
        let path = dir.join(relative);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("создание {}", parent.display()))?;
        }

        write_with_retry(&path, data)?;

        if relative.ends_with(".png") {
            pngs += 1;
        }

        bytes += data.len() as u64;
    }

    Ok((pngs, bytes))
}

/// Deletes files outside `expected` and the emptied directories.
fn remove_stale(dir: &Path, expected: &HashSet<PathBuf>) -> Result<()> {
    let mut stack = vec![dir.to_path_buf()];
    let mut directories = Vec::new();

    while let Some(current) = stack.pop() {
        let entries =
            fs::read_dir(&current).with_context(|| format!("чтение {}", current.display()))?;

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                directories.push(path.clone());
                stack.push(path);
            } else if !expected.contains(&path) {
                fs::remove_file(&path)
                    .with_context(|| format!("удаление устаревшего {}", path.display()))?;
            }
        }
    }

    directories.sort();
    directories.reverse();

    for path in directories {
        if fs::read_dir(&path)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
        {
            fs::remove_dir(&path)
                .with_context(|| format!("удаление пустого {}", path.display()))?;
        }
    }

    Ok(())
}

/// Removes from `out_root` (`units`) everything that is not a directory of one of the 93
/// types. The root `manifest.csv`/`README.md` live here too — we do not touch them.
fn remove_stale_unit_dirs(units_root: &Path, units: &[UnitReport]) -> Result<()> {
    let expected: HashSet<&str> = units.iter().map(|unit| unit.token.as_str()).collect();
    let entries =
        fs::read_dir(units_root).with_context(|| format!("чтение {}", units_root.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();

        if path.is_dir() {
            if !expected.contains(name.as_str()) {
                fs::remove_dir_all(&path)
                    .with_context(|| format!("удаление устаревшего {}", path.display()))?;
            }
        } else if !matches!(name.as_str(), "manifest.csv" | "README.md") {
            fs::remove_file(&path)
                .with_context(|| format!("удаление лишнего файла {}", path.display()))?;
        }
    }

    Ok(())
}

/// File writing with retries: Windows antivirus/indexer sometimes keeps a file
/// open right after it is created (`os error 1224`).
fn write_with_retry(path: &Path, data: &[u8]) -> Result<()> {
    let mut last = None;

    for attempt in 0..6u64 {
        match fs::write(path, data) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(120 * (attempt + 1)));
            }
        }
    }

    Err(anyhow!(
        "запись {}: {}",
        path.display(),
        last.map(|error| error.to_string())
            .unwrap_or_else(|| "неизвестная ошибка".to_string())
    ))
}

// ---------------------------------------------------------------------------
// manifest.csv and README.md
// ---------------------------------------------------------------------------

/// Escapes a CSV field (quotes only if it contains a separator).
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// `manifest.csv` — one row per unit type.
fn write_manifest(path: &Path, report: &ExportReport) -> Result<()> {
    let mut out = String::from(
        "token,unit_type,name,dir,building,sprite,shadow,portrait,flics,body_frames,turret_frames,firing_frames,connector_frames,shadow_frames,portrait_present,portrait_source,portrait_width,portrait_height,flc_present,flc_frames,flc_file,sprite_width,sprite_height,pngs,bytes,notes\n",
    );

    for unit in &report.units {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv_field(&unit.token),
            unit.unit_type,
            csv_field(&unit.name),
            csv_field(&format!("units/{}", unit.token)),
            u8::from(unit.building),
            csv_field(unit.sprite_tag.as_deref().unwrap_or("")),
            csv_field(unit.shadow_tag.as_deref().unwrap_or("")),
            csv_field(unit.portrait_tag.as_deref().unwrap_or("")),
            csv_field(unit.flc_tag.as_deref().unwrap_or("")),
            unit.body_frames,
            unit.turret_frames,
            unit.firing_frames,
            unit.connector_frames,
            unit.shadow_frames,
            u8::from(unit.portrait_source != PortraitSource::None),
            unit.portrait_source.as_str(),
            unit.portrait_width,
            unit.portrait_height,
            u8::from(unit.flc_frames > 0),
            unit.flc_frames,
            csv_field(unit.flc_file.as_deref().unwrap_or("")),
            unit.sprite_width,
            unit.sprite_height,
            unit.pngs,
            unit.bytes,
            csv_field(&unit.notes.join("; ")),
        ));
    }

    fs::write(path, out).with_context(|| format!("запись {}", path.display()))?;

    Ok(())
}

/// The export's `README.md` — what was exported, how to regenerate it, statistics.
fn write_readme(path: &Path, report: &ExportReport) -> Result<()> {
    let (body, turret, firing, connectors, shadow) = report.frame_totals();
    let without_portrait = report.tokens_without_portrait();
    let without_portrait_png = report.tokens_without_portrait_png();
    let without_animation = report.tokens_without_animation();
    let buildings = report.units.iter().filter(|unit| unit.building).count();
    let mobile = report.units.len() - buildings;
    let payload = report.pngs;

    let mut out = String::new();

    out.push_str("# max-brh-assets/units — ресурсы M.A.X. по юнитам\n\n");
    out.push_str(
        "Выгрузка «один каталог на тип юнита»: спрайты корпуса, башня, выстрел,\n\
коннекторы, тень, портрет `P_*` и FLC-анимация — всё, что игра берёт для\n\
конкретного типа через `SC_UNITS` (`sprite` / `shadow` / `portrait` / `data` /\n\
`flics_animation`).\n\n",
    );
    out.push_str("```text\n");
    out.push_str("max-brh-assets/units/\n");
    out.push_str("├── README.md            этот файл (в git)\n");
    out.push_str("├── manifest.csv         строка на тип: теги, кадры, портрет, FLC (в git)\n");
    out.push_str("├── TANK/\n");
    out.push_str("│   ├── sprites/       frame_XXX.png (корпус), sheet.png (лента), sprite.json\n");
    out.push_str("│   ├── turret/        кадры башни (если есть)\n");
    out.push_str("│   ├── firing/        кадры выстрела (если есть)\n");
    out.push_str("│   ├── connectors/    кадры коннекторов (если есть)\n");
    out.push_str("│   ├── shadow/        кадры тени S_*\n");
    out.push_str("│   ├── portrait.png   P_* 300×240 (палитра окна) или fallback\n");
    out.push_str("│   ├── animation/     frame_000.png…, animation.gif, копия .FLC\n");
    out.push_str("│   └── unit.json      тип/имя/флаги/источники/размеры/анимация\n");
    out.push_str("├── MININGST/\n");
    out.push_str("└── …                  всего 93 каталога (все UnitType)\n");
    out.push_str("```\n\n");
    out.push_str(
        "Каталоги `<TAG>/` — сама выгрузка: в git не коммитятся\n\
(`.gitignore` → `max-brh-assets/units/*/`), потому что регенерируются из данных\n\
игры.\n\n",
    );

    out.push_str("## Как перегенерировать\n\n");
    out.push_str("Из корня репозитория:\n\n");
    out.push_str("```sh\n");
    out.push_str("cd max-brh-qa-tools/max-resources-exporter\n");
    out.push_str("cargo run --release\n");
    out.push_str("```\n\n");
    out.push_str(
        "Пути по умолчанию — относительно корня репозитория\n\
(`../../max-original/MAX.RES`, `../../max-original/COLOR.PAL`,\n\
`../../max-original/*.FLC`,\n\
`../../max-brh/crates/max_app/data/cargo_unit_data.json`,\n\
`../../max-cpp-port/assets/PATCHES.RES` для сверки `SC_UNITS`, вывод —\n\
`../../max-brh-assets/units`). Любой путь переопределяется флагом:\n\n",
    );
    out.push_str("```sh\n");
    out.push_str("cargo run --release -- \\\n");
    out.push_str("  --res ../../max-original/MAX.RES \\\n");
    out.push_str("  --pal ../../max-original/COLOR.PAL \\\n");
    out.push_str("  --flc-dir ../../max-original \\\n");
    out.push_str("  --units ../../max-brh/crates/max_app/data/cargo_unit_data.json \\\n");
    out.push_str("  --patches ../../max-cpp-port/assets/PATCHES.RES \\\n");
    out.push_str("  --out ../../max-brh-assets/units \\\n");
    out.push_str("  --only TANK,MININGST\n");
    out.push_str("```\n\n");
    out.push_str(
        "Без `--patches` сверка с `SC_UNITS` пропускается (маппинг всё равно\n\
читается из `cargo_unit_data.json`). Повторный прогон **идемпотентен**: файлы\n\
перезаписываются теми же байтами, а всё лишнее в каталогах юнитов (и каталоги\n\
типов, которых нет среди 93) удаляется — мусора не остаётся.\n\n",
    );

    out.push_str("## Статистика последнего экспорта\n\n");
    out.push_str("| Показатель | Значение |\n|---|---:|\n");
    out.push_str(&format!("| Типов юнитов | **{}** |\n", report.units.len()));
    out.push_str(&format!("| — зданий / мобильных | {buildings} / {mobile} |\n"));
    out.push_str(&format!(
        "| Каталогов в `units/` | {} |\n",
        report.units.len()
    ));
    out.push_str(&format!("| PNG всего | **{payload}** |\n"));
    out.push_str(&format!(
        "| Объём каталогов | {:.1} МБ |\n",
        report.bytes as f64 / (1024.0 * 1024.0)
    ));
    out.push_str(&format!("| Кадров корпуса | {body} |\n"));
    out.push_str(&format!("| Кадров башни | {turret} |\n"));
    out.push_str(&format!("| Кадров выстрела | {firing} |\n"));
    out.push_str(&format!("| Кадров коннекторов | {connectors} |\n"));
    out.push_str(&format!("| Кадров тени | {shadow} |\n"));
    out.push_str(&format!(
        "| Портретов `P_*` (300×240) | **{}** |\n",
        report.portraits()
    ));
    out.push_str(&format!(
        "| Портретов-fallback (кадр спрайта) | {} |\n",
        report.portrait_fallbacks()
    ));
    out.push_str(&format!(
        "| Типов без `portrait.png` вовсе | {} |\n",
        report.tokens_without_portrait_png().len()
    ));
    out.push_str(&format!(
        "| Типов с FLC-анимацией | **{}** ({} разных `.FLC`) |\n",
        report.animations(),
        report.flc_files()
    ));
    out.push_str(&format!(
        "| Типов без анимации | {} |\n",
        without_animation.len()
    ));
    out.push_str(&format!(
        "| Сверка с `SC_UNITS` | {} |\n\n",
        match &report.sc_units {
            Some(check) if check.is_ok() => {
                format!("✔ {} записей, {} полей", check.units, check.fields)
            }
            Some(check) => format!("✘ {} расхождений", check.mismatches.len()),
            None => "не выполнялась (`--patches` не найден)".to_string(),
        }
    ));

    out.push_str("### У кого нет портрета `P_*`\n\n");
    out.push_str(
        "Служебные и «общие» типы (обломки, покрытия, взрывы, ракеты, `WALDO`): в\n\
`SC_UNITS` у них `portrait = INVALID_ID`. Для них `portrait.png` — fallback из\n\
первого кадра спрайта, увеличенного целым множителем на чёрном фоне\n\
(`unit.json` → `portrait.source = \"fallback\"`, `portrait.scale`).\n\n",
    );
    out.push_str(&format!(
        "{} тип(ов): `{}`\n\n",
        without_portrait.len(),
        without_portrait.join("`, `")
    ));

    if !without_portrait_png.is_empty() {
        out.push_str(&format!(
            "Совсем без `portrait.png` (нет ни `P_*`, ни спрайта для fallback —\n\
вырезанный из игры тип): `{}` — в каталоге только `unit.json`.\n\n",
            without_portrait_png.join("`, `")
        ));
    }

    out.push_str("### У кого нет FLC-анимации\n\n");
    out.push_str(
        "У большинства из них `flics_animation = INVALID_ID` (эффекты, ракеты,\n\
обломки, `HARVSTER`, `WALDO`); у `MASTER` тег `F_MASTER` есть, но самого\n\
ресурса (и файла) в данных игры нет. Каталога `animation/` у таких типов нет\n\
(в `unit.json` → `animation: null`).\n\n",
    );
    out.push_str(&format!(
        "{} тип(ов): `{}`\n\n",
        without_animation.len(),
        without_animation.join("`, `")
    ));

    out.push_str("## Как это устроено\n\n");
    out.push_str(concat!(
        "* **Спрайты** — `max_assets::sprite`, мультикадровый RLE из `MAX.RES`.\n",
        "  Кадры корпуса берутся по `image_base..+image_count` из `D_*`\n",
        "  (`max_assets::sprite::FrameInfo`), башня/выстрел/коннекторы — по\n",
        "  `turret_image_base`/`firing_image_base`/`connector_image_base` из того же\n",
        "  `D_*`. Пиксели — индексы `COLOR.PAL` (6-бит → ×4), пропущенные RLE-пиксели\n",
        "  прозрачны.\n",
        "* **Лента** `sprites/sheet.png` — горизонтальная укладка кадров корпуса по\n",
        "  горячей точке, как в `max-brh-assets/README.md` §«Лента кадров» (у\n",
        "  `INFANTRY`/`COMMANDO` она до ~25 000 px в ширину: 200 кадров, ячейка\n",
        "  расширяется по горячей точке).\n",
        "* **Тень** `S_*` — маска: чёрный, альфа 160.\n",
        "* **Портрет** `P_*` — big image 300×240; индексы переводятся **палитрой окна**\n",
        "  (`FRAMEPIC`), как `CargoMenu`/`BuildMenu` порта\n",
        "  (`WindowManager_LoadBigImage(..., palette_from_image = false)`). Своя палитра\n",
        "  `P_*` отличается от палитры окна на единицы в младших битах — картинка та же.\n",
        "* **FLC** — `F_*` из `MAX.RES` это ссылка на файл (`tank03.flc`); кадры\n",
        "  декодируются `max_assets::flic` и пишутся PNG по собственной палитре\n",
        "  `COLOR_256` кадра, `animation.gif` собирается своим GIF89a-кодировщиком\n",
        "  (`src/gif.rs`: LZW + локальные палитры кадров), исходный `.FLC` копируется\n",
        "  рядом.\n",
        "* **`sprite.json`** — 1:1 с `FrameInfo`: `image_base`, `image_count`,\n",
        "  `turret_image_*`, `firing_image_*`, `connector_image_*`, `angle_offsets`,\n",
        "  плюс размеры/горячие точки каждого выгруженного кадра и габариты ленты.\n\n",
    ));

    out.push_str("## Проверки\n\n");
    out.push_str("`cargo test` в `max-brh-qa-tools/max-resources-exporter` (числа — см. таблицу\n«Статистика последнего экспорта» выше):\n\n");
    out.push_str(concat!(
        "* счётчики: 93 каталога (все `UnitType`), число портретов `P_*`,\n",
        "  fallback-портретов и типов с FLC сверяется с реальными данными;\n",
        "* размеры PNG совпадают с кадрами спрайта, `portrait.png` — 300×240;\n",
        "* `sprite.json` соответствует `D_*` (`image_base`/`image_count`/\n",
        "  `turret_*`/`firing_*`/`connector_*`/`angle_offsets`) и числу выгруженных\n",
        "  кадров у каждого из 92 типов со спрайтом;\n",
        "* `animation.gif` — настоящий GIF89a (разбор блоков + число кадров FLC);\n",
        "* повторный экспорт даёт те же байты и не оставляет мусора.\n",
    ));

    fs::write(path, out).with_context(|| format!("запись {}", path.display()))?;

    Ok(())
}

/// Summary for stdout.
pub fn summary(report: &ExportReport) -> String {
    let (body, turret, firing, connectors, shadow) = report.frame_totals();
    let mut out = String::new();

    out.push_str(&format!(
        "юнитов: {} (зданий {}, мобильных {}), PNG: {}, {:.1} МБ\n",
        report.units.len(),
        report.units.iter().filter(|unit| unit.building).count(),
        report.units.iter().filter(|unit| !unit.building).count(),
        report.pngs,
        report.bytes as f64 / (1024.0 * 1024.0)
    ));
    out.push_str(&format!(
        "кадры: корпус {body}, башня {turret}, выстрел {firing}, коннекторы {connectors}, тень {shadow}\n"
    ));
    out.push_str(&format!(
        "портреты: {} P_* + {} fallback; анимации: {} типов / {} файлов FLC, без анимации: {}\n",
        report.portraits(),
        report.portrait_fallbacks(),
        report.animations(),
        report.flc_files(),
        report.tokens_without_animation().len()
    ));

    if let Some(check) = &report.sc_units {
        out.push_str(&format!(
            "SC_UNITS: {} записей, {} полей сверено\n",
            check.units, check.fields
        ));
    }

    for error in &report.errors {
        out.push_str(&format!("  ошибка: {error}\n"));
    }

    out
}
