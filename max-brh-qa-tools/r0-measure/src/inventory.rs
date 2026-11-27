//! Asset inventory and HD volume: what the remaster must ship and how much
//! memory it takes at render scale S ∈ {2, 4}.
//!
//! * Images come from `MAX.RES` through the shared classifier
//!   (`max-resources-viewer::model::classify_archive`), so the classes here are
//!   the same ones the exporter writes to `max-brh-assets/images/`.
//! * Terrain tiles come from the 24 `*.WRL` maps: every map carries its own
//!   tile bank, and the remaster needs one HD texture per **unique** tile
//!   (tile banks repeat between the six maps of a planet and across planets),
//!   so the tiles are deduplicated by content.
//! * Fonts are the embedded GNW rasters (`max_render::text::gnw`), the same
//!   bytes the renderer draws with.
//!
//! Volume at scale S: every source pixel becomes S×S pixels and an HD texture
//! is RGBA8, so `bytes = pixels * S² * 4`. The `.ktx2` column is the same data
//! at the codec's measured bit rate (see the `transcode` subcommand).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use max_assets::res::ResArchive;
use max_assets::wrl::WrlMap;
use max_render::text::gnw::{GnwFontId, GNW_GLYPHS};
use max_resources_viewer::model::{classify_archive, Category, Kind, ResourceInfo};

/// One row of the inventory.
#[derive(Debug, Default, Clone)]
pub struct ClassStats {
    /// Records (or unique tiles) in the class.
    pub records: usize,
    /// Animation frames across the records.
    pub frames: usize,
    /// Source pixels (sum of `w * h` over frames).
    pub pixels: u64,
    /// Encoded bytes in the archive / on disk.
    pub encoded: u64,
}

impl ClassStats {
    /// RGBA8 bytes for `scale` (1 — the 640×480 source resolution).
    pub fn rgba(&self, scale: u64) -> u64 {
        self.pixels * scale * scale * 4
    }
}

/// The classes the remaster pipeline is planned around.
const CLASS_NAMES: [&str; 11] = [
    "tiles (WRL, unique)",
    "unit/building sprites",
    "shadows (S_*, mask)",
    "UI chrome (FRAMEPIC/PANELTOP/PANELBTM)",
    "UI panels/buttons/fields",
    "UI menus/dialog backgrounds",
    "misc pictures (planets, logos)",
    "icons (I_*)",
    "markers/cursors/portraits",
    "fonts (GNW rasters)",
    "non-image (not a texture)",
];

/// Marker tags of the map overlays (`RAWMSK*`/`FUELMK*`/`GOLDMK*` + small ones).
const MARKER_PREFIXES: [&str; 8] = [
    "RAWMSK", "FUELMK", "GOLDMK", "SMRWMK", "SMFLMK", "SMGDMK", "SPTMRK", "BLDMRK",
];

/// In-game window chrome: the fixed 640×480 frame and the two side panels.
const CHROME_TAGS: [&str; 3] = ["FRAMEPIC", "PANELTOP", "PANELBTM"];

fn is_marker(tag: &str) -> bool {
    MARKER_PREFIXES.iter().any(|prefix| tag.starts_with(prefix))
}

/// Pixels of one classified record.
fn record_pixels(info: &ResourceInfo) -> u64 {
    if info.frames.is_empty() {
        return u64::from(info.width.max(0) as u32) * u64::from(info.height.max(0) as u32);
    }

    info.frames
        .iter()
        .map(|frame| u64::from(frame.width) * u64::from(frame.height))
        .sum()
}

/// Places one record into its class.
fn class_of(info: &ResourceInfo) -> &'static str {
    if !info.kind.is_image() {
        return CLASS_NAMES[10];
    }

    if CHROME_TAGS.contains(&info.name.as_str()) {
        return CLASS_NAMES[3];
    }

    if is_marker(&info.name) || info.kind == Kind::Cursor {
        return CLASS_NAMES[8];
    }

    match (info.category, info.kind) {
        (Category::Units, Kind::Sprite) => CLASS_NAMES[1],
        (Category::Units, Kind::Shadow) => CLASS_NAMES[2],
        (Category::Ui, Kind::Big) => CLASS_NAMES[5],
        (Category::Ui, _) => CLASS_NAMES[4],
        (Category::Misc, Kind::Sprite) if info.name.starts_with("I_") => CLASS_NAMES[7],
        (Category::Misc, Kind::Big) => CLASS_NAMES[6],
        (Category::Misc, _) => CLASS_NAMES[8],
        _ => CLASS_NAMES[9],
    }
}

/// One `*.WRL` map: (file name, width, height, tiles in the bank).
type MapInfo = (String, u16, u16, u16);

/// Unique terrain tiles across every `*.WRL` map, deduplicated by content.
///
/// Returns (unique tiles, encoded bytes of the maps, per-map tile counts).
fn terrain_tiles(world_dir: &Path) -> Result<(ClassStats, Vec<MapInfo>)> {
    let mut maps: Vec<PathBuf> = std::fs::read_dir(world_dir)
        .with_context(|| format!("cannot list {}", world_dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wrl"))
        })
        .collect();
    maps.sort();

    let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut stats = ClassStats::default();
    let mut per_map = Vec::new();

    for path in maps {
        let bytes = std::fs::read(&path)?;
        stats.encoded += bytes.len() as u64;

        let map =
            WrlMap::parse(&bytes).with_context(|| format!("cannot parse {}", path.display()))?;
        let tile_size = u64::from(map.tile_count());
        // A tile is only the same *texture* if the palette matches too: two
        // maps with the same indices but different palettes draw different
        // colours, so the pair (palette, tile) is the unit of deduplication.
        let palette_hash = fnv1a(map.palette());

        for id in 0..map.tile_count() {
            let Some(tile) = map.tile_pixels(id) else {
                continue;
            };

            let hash = fnv1a(tile) ^ palette_hash.rotate_left(17);
            if seen.insert(hash) {
                stats.records += 1;
                stats.frames += 1;
                stats.pixels += tile.len() as u64;
            }
        }

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        per_map.push((
            file_name,
            map.width() as u16,
            map.height() as u16,
            tile_size as u16,
        ));
    }

    Ok((stats, per_map))
}

/// FNV-1a 64-bit: a stable content hash for tile deduplication.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

/// Pixels of the embedded GNW fonts (one glyph bitmap per character).
fn font_pixels() -> ClassStats {
    let mut stats = ClassStats::default();

    for id in GnwFontId::ALL {
        let font = id.font();
        stats.records += 1;

        for code in 0..=GNW_GLYPHS as u32 {
            let Some(ch) = char::from_u32(code) else {
                continue;
            };
            let glyph = font.glyph(ch);
            // The atlas stores the glyph height for every advance width; the
            // bitmap is `stride * height` bytes of 1-bit rows.
            stats.pixels += u64::from(glyph.width) * u64::from(glyph.height);
            stats.frames += 1;
        }
    }

    stats
}

/// Runs the inventory and prints it as markdown (and optionally CSV).
pub fn run(root: &Path, csv: Option<&Path>) -> Result<()> {
    let max_res_path = root.join("max-original").join("MAX.RES");
    let archive = ResArchive::open(&max_res_path)
        .with_context(|| format!("cannot open {}", max_res_path.display()))?;
    let infos = classify_archive(&archive);

    let mut classes: BTreeMap<&'static str, ClassStats> = CLASS_NAMES
        .iter()
        .map(|name| (*name, ClassStats::default()))
        .collect();

    for info in &infos {
        let stats = classes.get_mut(class_of(info)).expect("class is known");
        stats.records += 1;
        stats.frames += info.frame_count.max(1);
        stats.pixels += record_pixels(info);
        stats.encoded += u64::from(info.size);
    }

    let (tiles_stats, per_map) = terrain_tiles(&root.join("max-original"))?;
    classes.insert(CLASS_NAMES[0], tiles_stats.clone());
    classes.insert(CLASS_NAMES[9], font_pixels());

    // Summary numbers.
    let images: Vec<&ResourceInfo> = infos.iter().filter(|info| info.kind.is_image()).collect();
    let full_screen: Vec<&&ResourceInfo> = images
        .iter()
        .filter(|info| info.width == 640 && info.height == 480)
        .collect();
    let total_pixels: u64 = classes.values().map(|stats| stats.pixels).sum();
    let total_encoded: u64 = classes.values().map(|stats| stats.encoded).sum();

    println!("## R0 asset inventory");
    println!();
    println!(
        "Source: `{}` ({} records), 24 `*.WRL` maps.",
        max_res_path.display(),
        infos.len()
    );
    println!(
        "Images: {} of {} records; full-screen 640×480 images: {}.",
        images.len(),
        infos.len(),
        full_screen.len()
    );
    println!();
    println!(
        "| class | records | frames | source pixels | source MiB | RGBA ×1 | RGBA ×2 | RGBA ×4 |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|");

    for name in CLASS_NAMES {
        let stats = &classes[name];

        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            name,
            stats.records,
            stats.frames,
            stats.pixels,
            mib(stats.encoded, 2),
            mib(stats.rgba(1), 2),
            mib(stats.rgba(2), 2),
            mib(stats.rgba(4), 2),
        );
    }

    println!(
        "| **total** | | | **{}** | **{}** | **{}** | **{}** | **{}** |",
        total_pixels,
        mib(total_encoded, 2),
        mib(total_pixels * 4, 2),
        mib(total_pixels * 16, 2),
        mib(total_pixels * 64, 2)
    );

    println!();
    println!("### Largest images (RGBA ×1 / ×2 / ×4)");
    println!();
    println!("| tag | kind | size | frames | px | ×1 | ×2 | ×4 |");
    println!("|---|---|---:|---:|---:|---:|---:|---:|");

    let mut sorted = images.clone();
    sorted.sort_by_key(|info| std::cmp::Reverse(record_pixels(info)));

    for info in sorted.iter().take(12) {
        let pixels = record_pixels(info);

        println!(
            "| {} | {} | {}×{} | {} | {} | {} | {} | {} |",
            info.name,
            info.kind.as_str(),
            info.width,
            info.height,
            info.frame_count,
            pixels,
            mib(pixels * 4, 2),
            mib(pixels * 16, 2),
            mib(pixels * 64, 2),
        );
    }

    println!();
    println!("### WRL maps (tile bank size)");
    println!();
    println!("| map | cells | unique tiles in bank |");
    println!("|---|---:|---:|");
    for (name, width, height, tiles) in &per_map {
        println!("| {} | {}×{} | {} |", name, width, height, tiles);
    }

    println!();
    println!(
        "Unique terrain tiles over all 24 maps: **{}** ({} RGBA ×1). \
         A single map's bank is 154…411 tiles (SNOW_1: 337); the deduplicated \
         set is the pair (map palette, tile pixels), because the same indices \
         draw different colours under a different palette.",
        tiles_stats.records,
        mib(tiles_stats.rgba(1), 2)
    );

    // Two classes need a note, because their "pixels" are not HD art:
    let shadows = &classes[CLASS_NAMES[2]];
    let data = &classes[CLASS_NAMES[10]];
    println!();
    println!(
        "`S_*` shadows are 1-bit darkening masks ({}), not colour textures: an HD \
         remaster can keep them at source resolution and stretch them, or regenerate \
         them; the RGBA column above is their bounding-box upper bound only.",
        human(shadows.pixels / 8)
    );
    println!(
        "`D_*` records ({} of them) are 8/24-byte turret/animation tables \
         (`GetFrameInfo`), not images: they have no pixel volume at all.",
        data.records
    );

    if let Some(path) = csv {
        write_csv(path, &classes, &per_map)?;
        println!();
        println!("CSV written to `{}`.", path.display());
    }

    Ok(())
}

/// Human-readable byte size (`742 KiB`, `3.5 MiB`).
fn human(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;

    if value >= MIB {
        format!("{:.2} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn write_csv(
    path: &Path,
    classes: &BTreeMap<&'static str, ClassStats>,
    per_map: &[(String, u16, u16, u16)],
) -> Result<()> {
    let mut text = String::from("class,records,frames,pixels,encoded_bytes,rgba1,rgba2,rgba4\n");

    for (name, stats) in classes {
        text.push_str(&format!(
            "\"{}\",{},{},{},{},{},{},{}\n",
            name,
            stats.records,
            stats.frames,
            stats.pixels,
            stats.encoded,
            stats.rgba(1),
            stats.rgba(2),
            stats.rgba(4)
        ));
    }

    text.push_str("\nmap,width,height,tiles\n");
    for (name, width, height, tiles) in per_map {
        text.push_str(&format!("{name},{width},{height},{tiles}\n"));
    }

    std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
}

/// Bytes in MiB with `digits` decimals (`0.00 MiB` for zero).
fn mib(bytes: u64, digits: usize) -> String {
    format!(
        "{:.digits$}",
        bytes as f64 / (1024.0 * 1024.0),
        digits = digits
    )
}
