//! Classification of MAX.RES records: kind, category, metadata, non-images.
//!
//! Order of the decoder probes (tag-aware, so that formats which are structurally
//! similar are not confused):
//!
//! 1. cursor — only the [`max_assets::CURSOR_RESOURCES`] tags and the exact size
//!    `8 + w*h*frames` (otherwise `HANDPTR` is indistinguishable from a simple image);
//! 2. shadow — only `S_*`, the [`max_assets::decode_shadow`] decoder;
//! 3. simple image — [`SimpleImage`] and the **exact** size `8 + w*h`
//!    (the decoder tolerates a tail, but in the real MAX.RES all 649 simple images
//!    fit exactly);
//! 4. sprite — [`max_assets::decode_sprite`] (it checks every frame/row itself);
//! 5. big image — [`BigImage`] (RLE; its own 776-byte palette header);
//! 6. shadow/cursor — a fallback for non-standard tags.
//!
//! If no decoder fits — `NonImage`: an ASCIIZ file reference
//! (`*.spw`/`*.wrl`/`*.flc`/`*.msc`/`*.mve`), a TTF (`FONT_*`), text/a script
//! (`ATTRIBS`, `CLANATRB`, `TIPS`, …) or binary data (`D_*`).

use max_assets::cursor::{CursorDescriptor, CURSOR_RESOURCES};
use max_assets::image::{BigImage, SimpleImage};
use max_assets::res::{parse_file_ref, ResArchive, ResEntry};
use max_assets::sprite::{decode_shadow, decode_sprite};

/// What kind of image (or non-image) the record holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// `ImageSimpleHeader`: the COLOR.PAL palette, no RLE.
    Simple,
    /// `ImageBigHeader`: its own palette + per-row RLE.
    Big,
    /// A multi-frame sprite (units/buildings/icons).
    Sprite,
    /// A multi-frame `S_*` shadow — a darkening mask.
    Shadow,
    /// `Cursor_Descriptor`: the frames are stacked vertically.
    Cursor,
    /// Not an image: a reference, TTF, text, data.
    NonImage,
}

impl Kind {
    /// A short name for the UI/manifest.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Big => "big",
            Self::Sprite => "sprite",
            Self::Shadow => "shadow",
            Self::Cursor => "cursor",
            Self::NonImage => "non-image",
        }
    }

    /// It is an image (there is something to export).
    pub const fn is_image(self) -> bool {
        !matches!(self, Self::NonImage)
    }
}

/// Export category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// Interface: window background, panels, buttons, fields, counters.
    Ui,
    /// Unit/building sprites and their shadows (COMMTWR…WALDO, S_*).
    Units,
    /// Icons, portraits, markers, logos, cursors, etc.
    Misc,
    /// Not an image (does not go into images/).
    NonImage,
}

impl Category {
    /// The `images/…` subdirectory name (`None` for non-images).
    pub const fn dir(self) -> Option<&'static str> {
        match self {
            Self::Ui => Some("ui"),
            Self::Units => Some("sprites/units"),
            Self::Misc => Some("misc"),
            Self::NonImage => None,
        }
    }

    /// A caption for the UI/README.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Units => "sprites/units",
            Self::Misc => "misc",
            Self::NonImage => "non-image",
        }
    }
}

/// The kind of non-image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NonImageKind {
    /// An ASCIIZ reference to `*.spw` (RIFF/WAVE next to MAX.RES).
    WavRef,
    /// An ASCIIZ reference to `*.wrl` (map).
    WrlRef,
    /// An ASCIIZ reference to `*.flc` (animation).
    FlcRef,
    /// An ASCIIZ reference to `*.msc` (music).
    MscRef,
    /// An ASCIIZ reference to `*.mve` (video).
    MveRef,
    /// A TrueType/OpenType font (`FONT_*` in PATCHES.RES).
    Ttf,
    /// A text script/INI (`;`/`[` at the start).
    Script,
    /// Text (help/hints).
    Text,
    /// Binary data (`D_*` — tables, SC_* in PATCHES.RES).
    Data,
}

impl NonImageKind {
    /// A short name for the UI/report.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WavRef => "wav-ref",
            Self::WrlRef => "wrl-ref",
            Self::FlcRef => "flc-ref",
            Self::MscRef => "msc-ref",
            Self::MveRef => "mve-ref",
            Self::Ttf => "ttf",
            Self::Script => "script",
            Self::Text => "text",
            Self::Data => "data",
        }
    }
}

/// Metadata of a single frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameMeta {
    /// Frame width.
    pub width: u16,
    /// Frame height.
    pub height: u16,
    /// Hotspot/anchor along x.
    pub hot_x: i16,
    /// Hotspot/anchor along y.
    pub hot_y: i16,
}

/// Palette source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteSource {
    /// The system palette `COLOR.PAL`.
    ColorPal,
    /// The palette inside the big image (768 bytes).
    Own,
    /// A non-image has no palette.
    None,
}

impl PaletteSource {
    /// A caption for the UI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ColorPal => "COLOR.PAL",
            Self::Own => "own",
            Self::None => "-",
        }
    }
}

/// The full classification of one record.
#[derive(Debug, Clone)]
pub struct ResourceInfo {
    /// The ordinal number in the table (0-based).
    pub index: usize,
    /// The tag (a C string from the 8-byte field).
    pub name: String,
    /// Data offset from the start of the file.
    pub offset: u32,
    /// Data size in bytes.
    pub size: u32,
    /// The format.
    pub kind: Kind,
    /// Export category.
    pub category: Category,
    /// Width (first frame / image).
    pub width: i32,
    /// Height (first frame / image).
    pub height: i32,
    /// Frame count (1 for simple images).
    pub frame_count: usize,
    /// Frames (for the strip export); one for big/simple.
    pub frames: Vec<FrameMeta>,
    /// The palette.
    pub palette: PaletteSource,
    /// The transparent index (simple/big/cursor), if there is one.
    pub transparent: Option<u8>,
    /// The `ulx` of the simple/big header (hotspot x for a cursor).
    pub ulx: i16,
    /// The `uly` of the simple/big header (hotspot y for a cursor).
    pub uly: i16,
    /// The non-image subtype.
    pub non_image: Option<NonImageKind>,
    /// The file name from the link body (`tank1.spw`).
    pub file_ref: Option<String>,
    /// The first bytes of the body (hex metadata in the UI).
    pub first_bytes: Vec<u8>,
}

impl ResourceInfo {
    /// The export file name (`TANK_44x45_24f.png`).
    pub fn file_name(&self) -> String {
        let mut name = sanitize_tag(&self.name);
        name.push('_');
        name.push_str(&format!("{}x{}", self.width.max(0), self.height.max(0)));

        if self.frame_count > 1 {
            name.push_str(&format!("_{}f", self.frame_count));
        }
        if self.kind == Kind::Shadow {
            name.push_str("_mask");
        } else if self.kind == Kind::Big {
            name.push_str("_ownpal");
        }
        name.push_str(".png");
        name
    }

    /// Path relative to the export root (`images/ui/...`).
    pub fn relative_path(&self) -> Option<String> {
        let dir = self.category.dir()?;
        Some(format!("images/{dir}/{}", self.file_name()))
    }

    /// A caption for the UI list.
    pub fn list_label(&self) -> String {
        if let Some(non_image) = self.non_image {
            format!(
                "{:<8} {:<5} {:<12} {}",
                self.name,
                "-",
                non_image.as_str(),
                "non-image"
            )
        } else {
            format!(
                "{:<8} {:<5} {:>4}x{:<4} {:>3}f  {}",
                self.name,
                self.kind.as_str(),
                self.width,
                self.height,
                self.frame_count,
                self.category.as_str()
            )
        }
    }

    /// The frame size range (`44x45..56x57`) or `—`.
    pub fn frame_size_range(&self) -> String {
        if self.frames.is_empty() {
            return "-".to_string();
        }

        let first = self.frames[0];
        let mut min_w = first.width;
        let mut max_w = first.width;
        let mut min_h = first.height;
        let mut max_h = first.height;

        for frame in &self.frames[1..] {
            min_w = min_w.min(frame.width);
            max_w = max_w.max(frame.width);
            min_h = min_h.min(frame.height);
            max_h = max_h.max(frame.height);
        }

        if min_w == max_w && min_h == max_h {
            format!("{min_w}x{min_h}")
        } else {
            format!("{min_w}x{min_h}..{max_w}x{max_h}")
        }
    }

    /// The first bytes as `74 61 6E 6B …`.
    pub fn first_bytes_hex(&self) -> String {
        self.first_bytes
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The list of M.A.X. unit/building/effect sprites (`COMMTWR`…`WALDO`).
///
/// These are the `SPRITE` records of `MAX.RES` without the `I_*` icons: 92 names, including
/// explosions/rockets/mines (they are `UnitType`s in the original too). `S_*` shadows
/// are classified into the same category separately.
pub const UNIT_SPRITES: [&str; 92] = [
    "RADAR", "GUNTURRT", "ARTYTRRT", "ANTIMSSL", "ANTIAIR", "POWERSTN", "LIGHTPLT", "LANDPLT",
    "MININGST", "DEPOT", "WTRPLTFM", "CNCT_4W", "ADUMP", "FDUMP", "POWGEN", "LRGSLAB", "SMLSLAB",
    "LRGTAPE", "LRGCONES", "SMLTAPE", "SMLCONES", "LRGRUBLE", "SMLRUBLE", "BLOCK", "BRIDGE",
    "ROAD", "HABITAT", "GREENHSE", "GOLDSM", "AIRPLT", "SUPRTPLT", "RESEARCH", "RECCENTR",
    "TRAINHAL", "SHIPYARD", "DOCK", "HANGAR", "SHIELDGN", "COMMTWR", "BARRACKS", "LANDPAD",
    "LNDEXPLD", "HITEXPLD", "BLDEXPLD", "RKTSMOKE", "AIREXPLD", "SEAEXPLD", "TRPBUBLE", "ROCKTLCH",
    "ROCKET", "FUELTRCK", "CONSTRCT", "SCANNER", "MISSLLCH", "BULLDOZR", "SPLYTRCK", "SURVEYOR",
    "MINELAYR", "REPAIR", "ENGINEER", "SCOUT", "ARTILLRY", "TANK", "LANDMINE", "GOLDTRCK",
    "CORVETTE", "FASTBOAT", "MSSLBOAT", "SUBMARNE", "BATTLSHP", "SEAMNLYR", "CARGOSHP", "SEATRANS",
    "TORPEDO", "ALNMISSL", "ALNTBALL", "ALNABALL", "CLNTRANS", "COMMANDO", "INFANTRY", "JUGGRNT",
    "ALNTANK", "ALNASGUN", "ALNPLANE", "AIRTRANS", "AWAC", "FIGHTER", "BOMBER", "SP_FLAK",
    "SEAMINE", "HARVSTER", "WALDO",
];

/// Resource/deposit marker tags (`misc`).
const MARKER_PREFIXES: [&str; 6] = ["RAWMSK", "FUELMK", "GOLDMK", "SMRWMK", "SMFLMK", "SMGDMK"];

/// Planet/star images (`misc`, 128×128 big image).
const PLANET_PICTURES: [&str; 5] = ["SNOW_PIC", "CRTR_PIC", "GREN_PIC", "DSRT_PIC", "STAR_PIC"];

/// Classifies all archive records (in file order, with duplicates).
pub fn classify_archive(archive: &ResArchive) -> Vec<ResourceInfo> {
    archive
        .list()
        .iter()
        .map(|entry| classify_entry(archive, entry))
        .collect()
}

/// Classifies a single record.
pub fn classify_entry(archive: &ResArchive, entry: &ResEntry) -> ResourceInfo {
    let bytes = entry_bytes(archive, entry);
    let name = entry.name.as_str();
    let mut info = ResourceInfo {
        index: entry.index,
        name: entry.name.clone(),
        offset: entry.offset,
        size: entry.size,
        kind: Kind::NonImage,
        category: Category::NonImage,
        width: 0,
        height: 0,
        frame_count: 0,
        frames: Vec::new(),
        palette: PaletteSource::None,
        transparent: None,
        ulx: 0,
        uly: 0,
        non_image: None,
        file_ref: None,
        first_bytes: bytes.iter().take(16).copied().collect(),
    };

    if let Some(cursor) = decode_cursor(name, bytes) {
        info.kind = Kind::Cursor;
        info.width = cursor.width;
        info.height = cursor.height;
        info.frame_count = usize::from(cursor.frame_count);
        info.frames = vec![
            FrameMeta {
                width: cursor.width as u16,
                height: cursor.height as u16,
                hot_x: cursor.hot_x as i16,
                hot_y: cursor.hot_y as i16,
            };
            usize::from(cursor.frame_count)
        ];
        info.palette = PaletteSource::ColorPal;
        info.transparent = Some(cursor.transparent);
        info.ulx = cursor.hot_x as i16;
        info.uly = cursor.hot_y as i16;
    } else if name.starts_with("S_") {
        if let Ok(sheet) = decode_shadow(bytes) {
            fill_shadow(&mut info, &sheet);
        }
    }

    if info.kind == Kind::NonImage {
        if let Some(image) = decode_simple(bytes) {
            info.kind = Kind::Simple;
            info.width = i32::from(image.width);
            info.height = i32::from(image.height);
            info.frame_count = 1;
            info.frames = vec![FrameMeta {
                width: image.width as u16,
                height: image.height as u16,
                hot_x: image.ulx,
                hot_y: image.uly,
            }];
            info.palette = PaletteSource::ColorPal;
            info.transparent = Some(image.transparent);
            info.ulx = image.ulx;
            info.uly = image.uly;
        }
    }

    if info.kind == Kind::NonImage {
        if let Ok(sheet) = decode_sprite(bytes) {
            info.kind = Kind::Sprite;
            info.frame_count = sheet.frames.len();
            info.frames = sheet
                .frames
                .iter()
                .map(|frame| FrameMeta {
                    width: frame.width,
                    height: frame.height,
                    hot_x: frame.hotx,
                    hot_y: frame.hoty,
                })
                .collect();
            if let Some(first) = sheet.frames.first() {
                info.width = i32::from(first.width);
                info.height = i32::from(first.height);
                info.ulx = first.hotx;
                info.uly = first.hoty;
            }
            info.palette = PaletteSource::ColorPal;
        }
    }

    if info.kind == Kind::NonImage {
        // `BigImage::decode` is very tolerant of short RLE and reads transparent
        // at offset 776; without a header check it "decodes" even text
        // (ATTRIBS/TIPS: the first ASCII bytes give width/height in the thousands).
        if big_header_plausible(bytes) {
            if let Ok(image) = BigImage::decode(bytes) {
                info.kind = Kind::Big;
                info.width = i32::from(image.width);
                info.height = i32::from(image.height);
                info.frame_count = 1;
                info.frames = vec![FrameMeta {
                    width: image.width as u16,
                    height: image.height as u16,
                    hot_x: image.ulx,
                    hot_y: image.uly,
                }];
                info.palette = PaletteSource::Own;
                info.transparent = Some(image.transparent);
                info.ulx = image.ulx;
                info.uly = image.uly;
            }
        }
    }

    if info.kind == Kind::NonImage {
        if let Ok(sheet) = decode_shadow(bytes) {
            fill_shadow(&mut info, &sheet);
        }
    }

    if info.kind == Kind::NonImage {
        if let Some(cursor) = decode_cursor_any(bytes) {
            info.kind = Kind::Cursor;
            info.width = cursor.width;
            info.height = cursor.height;
            info.frame_count = usize::from(cursor.frame_count);
            info.frames = vec![
                FrameMeta {
                    width: cursor.width as u16,
                    height: cursor.height as u16,
                    hot_x: cursor.hot_x as i16,
                    hot_y: cursor.hot_y as i16,
                };
                usize::from(cursor.frame_count)
            ];
            info.palette = PaletteSource::ColorPal;
            info.transparent = Some(cursor.transparent);
            info.ulx = cursor.hot_x as i16;
            info.uly = cursor.hot_y as i16;
        }
    }

    if info.kind.is_image() {
        info.category = image_category(name, info.kind);
    } else {
        let (non_image, file_ref) = classify_non_image(name, bytes);
        info.non_image = Some(non_image);
        info.file_ref = file_ref;
        info.category = Category::NonImage;
    }

    info
}

fn fill_shadow(info: &mut ResourceInfo, sheet: &max_assets::ShadowSheet) {
    info.kind = Kind::Shadow;
    info.frame_count = sheet.frames.len();
    info.frames = sheet
        .frames
        .iter()
        .map(|frame| FrameMeta {
            width: frame.width,
            height: frame.height,
            hot_x: frame.hotx,
            hot_y: frame.hoty,
        })
        .collect();
    if let Some(first) = sheet.frames.first() {
        info.width = i32::from(first.width);
        info.height = i32::from(first.height);
        info.ulx = first.hotx;
        info.uly = first.hoty;
    }
    info.palette = PaletteSource::ColorPal;
}

fn entry_bytes<'a>(archive: &'a ResArchive, entry: &ResEntry) -> &'a [u8] {
    let start = entry.offset as usize;
    let end = start
        .saturating_add(entry.size as usize)
        .min(archive.data().len());

    if start <= end {
        &archive.data()[start..end]
    } else {
        &[]
    }
}

fn big_header_plausible(bytes: &[u8]) -> bool {
    if bytes.len() <= 776 {
        return false;
    }

    let width = i16::from_le_bytes([bytes[4], bytes[5]]);
    let height = i16::from_le_bytes([bytes[6], bytes[7]]);

    width > 0
        && height > 0
        && width <= 1024
        && height <= 1024
        && usize::from(width as u16) * usize::from(height as u16) <= 1_048_576
}

fn decode_simple(bytes: &[u8]) -> Option<SimpleImage> {
    // Exactly 8 bytes — a header without pixels; `SimpleImage::decode` reads
    // `bytes[8]` as transparent_color and panics on such a slice.
    if bytes.len() < 9 {
        return None;
    }
    let image = SimpleImage::decode(bytes).ok()?;
    let expected =
        8usize.checked_add(usize::from(image.width as u16) * usize::from(image.height as u16))?;

    (bytes.len() == expected).then_some(image)
}

fn decode_cursor(name: &str, bytes: &[u8]) -> Option<CursorDescriptor> {
    if !CURSOR_RESOURCES.contains(&name) {
        return None;
    }

    decode_cursor_any(bytes)
}

fn decode_cursor_any(bytes: &[u8]) -> Option<CursorDescriptor> {
    // `CursorDescriptor::decode` also reads the 9th byte (transparent).
    if bytes.len() < 9 {
        return None;
    }
    let cursor = CursorDescriptor::decode(bytes).ok()?;
    let expected =
        8usize + cursor.width as usize * cursor.height as usize * usize::from(cursor.frame_count);

    (bytes.len() == expected).then_some(cursor)
}

/// The image category by tag/format.
pub fn image_category(name: &str, kind: Kind) -> Category {
    if kind == Kind::NonImage {
        return Category::NonImage;
    }
    if kind == Kind::Shadow {
        return Category::Units;
    }
    if kind == Kind::Sprite && UNIT_SPRITES.contains(&name) {
        return Category::Units;
    }
    if is_misc(name) {
        return Category::Misc;
    }

    Category::Ui
}

/// The tag belongs to `misc` (icons, portraits, markers, logos, cursors…).
pub fn is_misc(name: &str) -> bool {
    if CURSOR_RESOURCES.contains(&name) {
        return true;
    }
    if MARKER_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return true;
    }
    if PLANET_PICTURES.contains(&name) || name == "ILOGO" {
        return true;
    }
    if name.starts_with("I_")
        || name.starts_with("P_")
        || name.starts_with("A_")
        || name.starts_with("E_")
    {
        return true;
    }
    if name.starts_with("CLN") && name.ends_with("LOGO") {
        return true;
    }
    if name.starts_with("CH_CN") || name.starts_with("CH_TM") {
        return true;
    }

    false
}

/// The non-image subtype and the reference file name.
pub fn classify_non_image(name: &str, bytes: &[u8]) -> (NonImageKind, Option<String>) {
    if name.starts_with("FONT_") || looks_like_ttf(bytes) {
        return (NonImageKind::Ttf, None);
    }

    if let Some(reference) = parse_file_ref(bytes) {
        let kind = match reference
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .as_deref()
        {
            Some("spw") => NonImageKind::WavRef,
            Some("wrl") => NonImageKind::WrlRef,
            Some("flc") => NonImageKind::FlcRef,
            Some("msc") => NonImageKind::MscRef,
            Some("mve") => NonImageKind::MveRef,
            Some("ttf") | Some("fon") => NonImageKind::Ttf,
            _ => NonImageKind::Data,
        };

        return (kind, Some(reference.to_owned()));
    }

    if looks_like_text(bytes) {
        let trimmed = bytes
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .map(|start| &bytes[start..])
            .unwrap_or(&[]);
        let kind = match trimmed.first() {
            Some(b';') | Some(b'[') => NonImageKind::Script,
            _ => NonImageKind::Text,
        };

        return (kind, None);
    }

    (NonImageKind::Data, None)
}

/// The first 4 bytes of a TTF/OTF (`00 01 00 00`) or an `FFTM` table in the header.
fn looks_like_ttf(bytes: &[u8]) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    if bytes.starts_with(&[0x00, 0x01, 0x00, 0x00]) {
        return true;
    }

    bytes[..bytes.len().min(64)]
        .windows(4)
        .any(|window| window == b"FFTM")
}

/// The slice looks like text: 95% printable ASCII/CR/LF/TAB, with no NUL.
fn looks_like_text(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(512)];

    if sample.is_empty() {
        return false;
    }

    let printable = sample
        .iter()
        .filter(|&&byte| {
            byte == b'\r' || byte == b'\n' || byte == b'\t' || (0x20..0x7F).contains(&byte)
        })
        .count();

    printable * 100 >= sample.len() * 95
}

/// A safe file name from the 8-byte tag.
pub fn sanitize_tag(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());

    for ch in tag.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    if out.is_empty() {
        "resource".to_string()
    } else {
        out
    }
}

/// Summary statistics for the archive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassificationStats {
    /// Total records.
    pub entries: usize,
    /// Images.
    pub images: usize,
    /// UI.
    pub ui: usize,
    /// Units/buildings/shadows.
    pub units: usize,
    /// Other.
    pub misc: usize,
    /// Non-images.
    pub non_images: usize,
    /// simple.
    pub simple: usize,
    /// big.
    pub big: usize,
    /// sprite.
    pub sprite: usize,
    /// shadow.
    pub shadow: usize,
    /// cursor.
    pub cursor: usize,
    /// Non-images by subtype.
    pub non_image_kinds: Vec<(NonImageKind, usize)>,
}

impl ClassificationStats {
    /// Computes statistics over the already classified records.
    pub fn from_infos(infos: &[ResourceInfo]) -> Self {
        let mut stats = Self {
            entries: infos.len(),
            ..Self::default()
        };
        let mut non_image_counts: Vec<(NonImageKind, usize)> = Vec::new();

        for info in infos {
            match info.kind {
                Kind::Simple => stats.simple += 1,
                Kind::Big => stats.big += 1,
                Kind::Sprite => stats.sprite += 1,
                Kind::Shadow => stats.shadow += 1,
                Kind::Cursor => stats.cursor += 1,
                Kind::NonImage => stats.non_images += 1,
            }

            if info.kind.is_image() {
                stats.images += 1;
                match info.category {
                    Category::Ui => stats.ui += 1,
                    Category::Units => stats.units += 1,
                    Category::Misc => stats.misc += 1,
                    Category::NonImage => {}
                }
            } else if let Some(kind) = info.non_image {
                if let Some(entry) = non_image_counts
                    .iter_mut()
                    .find(|(existing, _)| *existing == kind)
                {
                    entry.1 += 1;
                } else {
                    non_image_counts.push((kind, 1));
                }
            }
        }

        non_image_counts.sort_by_key(|(kind, _)| *kind as u8);
        stats.non_image_kinds = non_image_counts;
        stats
    }

    /// The number of non-images of the given subtype.
    pub fn non_image_count(&self, kind: NonImageKind) -> usize {
        self.non_image_kinds
            .iter()
            .find(|(existing, _)| *existing == kind)
            .map(|(_, count)| *count)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_tag_keeps_safe_ascii() {
        assert_eq!(sanitize_tag("TANK"), "TANK");
        assert_eq!(sanitize_tag("S_TANK"), "S_TANK");
        assert_eq!(sanitize_tag("A/B C"), "A_B_C");
        assert_eq!(sanitize_tag(""), "resource");
    }

    #[test]
    fn misc_rules_match_the_documented_sets() {
        assert!(is_misc("HANDPTR"));
        assert!(is_misc("RAWMSK0"));
        assert!(is_misc("SNOW_PIC"));
        assert!(is_misc("I_TANK"));
        assert!(is_misc("P_TANK"));
        assert!(is_misc("CLN3LOGO"));
        assert!(is_misc("CH_CN1_U"));
        assert!(!is_misc("FRAMEPIC"));
        assert!(!is_misc("TANK"));
        assert!(!is_misc("S_TANK"));
        assert!(!is_misc("PANELTOP"));
    }

    #[test]
    fn unit_category_covers_sprites_and_shadows() {
        assert_eq!(image_category("TANK", Kind::Sprite), Category::Units);
        assert_eq!(image_category("COMMTWR", Kind::Sprite), Category::Units);
        assert_eq!(image_category("S_TANK", Kind::Shadow), Category::Units);
        assert_eq!(image_category("FRAMEPIC", Kind::Big), Category::Ui);
        assert_eq!(image_category("PANELTOP", Kind::Simple), Category::Ui);
        assert_eq!(image_category("HANDPTR", Kind::Cursor), Category::Misc);
    }

    #[test]
    fn text_and_data_are_not_mistaken_for_images() {
        assert_eq!(
            classify_non_image("ATTRIBS", b";Attribute Table\r\n; NOTE"),
            (NonImageKind::Script, None)
        );
        assert_eq!(
            classify_non_image("HELP_ENG", b"\r\n\r\n Main Headers [in brackets]"),
            (NonImageKind::Text, None)
        );
        assert_eq!(
            classify_non_image("D_TANK", &[0, 8, 8, 8, 16, 8, 0, 0]),
            (NonImageKind::Data, None)
        );
        assert_eq!(
            classify_non_image("TANK1", b"tank1.spw\0"),
            (NonImageKind::WavRef, Some("tank1.spw".to_string()))
        );
        assert_eq!(
            classify_non_image("SNOW_1", b"snow_1.wrl\0"),
            (NonImageKind::WrlRef, Some("snow_1.wrl".to_string()))
        );
    }
}
