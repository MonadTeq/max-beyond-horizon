//! Integration tests on **real** game data: the export of all 93 types,
//! the counters, PNG sizes, the `sprite.json` ↔ `D_*` (`FrameInfo`) correspondence,
//! GIF validity and the idempotency of a repeated run.
//!
//! If the game data (`max-original/MAX.RES`) is not nearby, the tests are skipped:
//! `cargo test` must pass in a checkout without the resources too.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use max_assets::sprite::{decode_sprite, FrameInfo};
use max_assets::ResArchive;
use serde_json::Value;
use max_resources_exporter::png;
use max_resources_exporter::{export_all, ExportOptions, ExportReport};

// ---------------------------------------------------------------------------
// Infrastructure
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("корень репозитория")
        .to_path_buf()
}

/// The game data is in place (otherwise the tests are skipped).
fn data_available() -> bool {
    let options = ExportOptions::defaults(repo_root());

    options.max_res.is_file() && options.color_pal.is_file() && options.units_json.is_file()
}

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("max-resources-exporter-{name}-{}", std::process::id()))
}

/// One shared export for the whole test binary: the 93 directories are written once,
/// the other tests read the ready tree (tests run in parallel).
struct Shared {
    root: PathBuf,
    report: ExportReport,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

fn shared() -> &'static Shared {
    SHARED.get_or_init(|| {
        let root = temp_dir("shared");
        let _ = fs::remove_dir_all(&root);

        let mut options = ExportOptions::defaults(repo_root());
        options.out_root = root.clone();

        let report = export_all(&options).expect("экспорт всех 93 типов");

        Shared { root, report }
    })
}

/// The export directory: `out_root` *is* the unit directory (`.../units`), and the type
/// directories sit right in it.
fn units_dir(root: &Path) -> PathBuf {
    root.to_path_buf()
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));

    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The PNG file size (via our own decoder: it also checks the CRC/structure).
fn png_size(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let image = png::decode(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));

    (image.width, image.height)
}

/// Walks the tree: relative path (with `/`) → contents.
fn collect_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read_dir").flatten() {
            let path = entry.path();

            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("внутри корня")
                    .to_string_lossy()
                    .replace('\\', "/");

                out.insert(relative, fs::read(&path).expect("чтение файла"));
            }
        }
    }

    out
}

/// `(width, height, frames)` from the GIF89a; also a structural check of the blocks.
fn parse_gif(bytes: &[u8]) -> (u16, u16, usize) {
    assert!(bytes.len() > 13 + 768, "GIF слишком короткий");
    assert_eq!(&bytes[..6], b"GIF89a", "сигнатура GIF89a");

    let width = u16::from_le_bytes([bytes[6], bytes[7]]);
    let height = u16::from_le_bytes([bytes[8], bytes[9]]);
    let packed = bytes[10];
    let mut pos = 13;

    if packed & 0x80 != 0 {
        let entries = 1usize << ((packed & 0x07) + 1);
        pos += 3 * entries;
    }

    let mut frames = 0;

    while pos < bytes.len() {
        match bytes[pos] {
            0x3B => break, // trailer
            0x21 => {
                pos += 2; // 0x21 + label

                loop {
                    let size = bytes[pos] as usize;
                    pos += 1;

                    if size == 0 {
                        break;
                    }

                    pos += size;
                }
            }
            0x2C => {
                frames += 1;
                let descriptor_packed = bytes[pos + 9];
                pos += 10;

                if descriptor_packed & 0x80 != 0 {
                    let entries = 1usize << ((descriptor_packed & 0x07) + 1);
                    pos += 3 * entries;
                }

                pos += 1; // LZW minimum code size

                loop {
                    let size = bytes[pos] as usize;
                    pos += 1;

                    if size == 0 {
                        break;
                    }

                    pos += size;
                }
            }
            other => panic!("GIF: неожиданный блок 0x{other:02X} на 0x{pos:X}"),
        }
    }

    (width, height, frames)
}

// ---------------------------------------------------------------------------
// Counters: the fixed numbers from the real data
// ---------------------------------------------------------------------------

#[test]
fn exports_all_93_unit_types_with_expected_counts() {
    if !data_available() {
        eprintln!("нет данных игры — тест пропущен");

        return;
    }

    let shared = shared();
    let report = &shared.report;

    assert!(report.errors.is_empty(), "ошибки: {:?}", report.errors);
    assert_eq!(report.units.len(), 93, "все UnitType");
    assert_eq!(report.units.len(), max_core::enums::UnitType::COUNT);

    // 93 directories, exactly one per type.
    let mut dirs: Vec<String> = fs::read_dir(units_dir(&shared.root))
        .expect("units")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    dirs.sort();

    let mut tokens: Vec<String> = report.units.iter().map(|unit| unit.token.clone()).collect();
    tokens.sort();

    assert_eq!(dirs, tokens);
    assert_eq!(dirs.len(), 93);

    // Portraits: 70 `P_*` + 22 fallbacks + MASTER without a resource (P_MASTER is not in MAX.RES,
    // a trimmed type).
    assert_eq!(report.portraits(), 70, "портреты P_*");
    assert_eq!(report.portrait_fallbacks(), 22, "fallback из спрайта");
    assert_eq!(report.tokens_without_portrait_png(), vec!["MASTER"]);
    assert_eq!(report.tokens_without_portrait().len(), 23);

    // FLC: 70 types (MASTER has neither F_MASTER nor the file), 23 without animation.
    assert_eq!(report.animations(), 70, "типы с FLC");
    assert_eq!(report.flc_files(), 70, "разные файлы .FLC");
    assert_eq!(report.tokens_without_animation().len(), 23);

    // Frames per kind (real MAX.RES + D_* data).
    assert_eq!(
        report.frame_totals(),
        (975, 118, 224, 188, 1357),
        "(корпус, башня, выстрел, коннекторы, тень)"
    );

    // Buildings/mobile units, and no FAIL for the ones without a sprite.
    let buildings = report.units.iter().filter(|unit| unit.building).count();
    assert_eq!(buildings, 22, "зданий по флагу BUILDING");

    // The manifest: header + 93 rows.
    let manifest = fs::read_to_string(shared.root.join("manifest.csv")).expect("manifest.csv");
    assert_eq!(manifest.lines().count(), 94);
    assert!(manifest.starts_with("token,unit_type,name,dir,"));
    assert!(
        manifest.contains("units/TANK"),
        "manifest: каталог юнита — units/<TAG>"
    );

    // The README with statistics.
    let readme = fs::read_to_string(shared.root.join("README.md")).expect("README.md");
    assert!(readme.contains("93"), "README: количество типов");
    assert!(readme.contains("70"), "README: портреты/анимации");
    assert!(readme.contains("units/"), "README: структура");

    // The mapping check against SC_UNITS was performed.
    let check = report.sc_units.as_ref().expect("сверка SC_UNITS");
    assert!(check.is_ok());
    assert_eq!(check.units, 93);
    assert_eq!(check.fields, 93 * 5);
}

// ---------------------------------------------------------------------------
// The layout of a specific type
// ---------------------------------------------------------------------------

#[test]
fn tank_has_body_turret_firing_shadow_portrait_and_animation() {
    if !data_available() {
        return;
    }

    let shared = shared();
    let dir = units_dir(&shared.root).join("TANK");

    let count = |sub: &str| {
        fs::read_dir(dir.join(sub))
            .unwrap_or_else(|error| panic!("{sub}: {error}"))
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
            .count()
    };

    // D_TANK: body 0..8, turret 8..16, firing 16..24, no connectors.
    assert_eq!(count("sprites"), 9, "8 кадров корпуса + sheet.png");
    assert_eq!(count("turret"), 8);
    assert_eq!(count("firing"), 8);
    assert_eq!(count("shadow"), 24);
    assert!(!dir.join("connectors").exists(), "у TANK нет коннекторов");

    let animation = fs::read_dir(dir.join("animation")).expect("animation");
    let animation_files: Vec<String> = animation
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();

    assert_eq!(
        animation_files.iter().filter(|name| name.ends_with(".png")).count(),
        30,
        "кадры FLC"
    );
    assert!(animation_files.iter().any(|name| name == "animation.gif"));
    assert!(
        animation_files.iter().any(|name| name == "TANK03.FLC"),
        "копия исходного FLC: {animation_files:?}"
    );

    // The portrait is P_TANK 300×240.
    assert_eq!(png_size(&dir.join("portrait.png")), (300, 240));

    // unit.json.
    let unit = read_json(&dir.join("unit.json"));

    assert_eq!(unit["token"], "TANK");
    assert_eq!(unit["unit_type"], 51);
    assert_eq!(unit["name"], "Tank");
    assert_eq!(unit["kind"], "mobile");
    assert_eq!(unit["sources"]["sprite"], "TANK");
    assert_eq!(unit["sources"]["data"], "D_TANK");
    assert_eq!(unit["sources"]["portrait"], "P_TANK");
    assert_eq!(unit["sprite"]["frames_body"], 8);
    assert_eq!(unit["sprite"]["frames_turret"], 8);
    assert_eq!(unit["sprite"]["frames_firing"], 8);
    assert_eq!(unit["sprite"]["frames_connectors"], 0);
    assert_eq!(unit["sprite"]["frames_shadow"], 24);
    assert_eq!(unit["portrait"]["source"], "resource");
    assert_eq!(unit["portrait"]["width"], 300);
    assert_eq!(unit["portrait"]["height"], 240);
    assert_eq!(unit["animation"]["frames"], 30);
    assert_eq!(unit["animation"]["source_file"], "TANK03.FLC");

    // A body frame is 44×45 (TANK's first frame), a turret frame is 24×38 (frame 8).
    assert_eq!(png_size(&dir.join("sprites/frame_000.png")), (44, 45));
    assert_eq!(png_size(&dir.join("turret/frame_008.png")), (24, 38));
    assert_eq!(png_size(&dir.join("firing/frame_016.png")), (32, 57));
}

#[test]
fn sprite_json_matches_frame_info_for_every_unit() {
    if !data_available() {
        return;
    }

    let shared = shared();
    let options = ExportOptions::defaults(repo_root());
    let archive = ResArchive::open(&options.max_res).expect("MAX.RES");

    let mut checked = 0;

    for unit in &shared.report.units {
        let dir = units_dir(&shared.root).join(&unit.token);
        let sprite_json_path = dir.join("sprites/sprite.json");

        // MASTER has no sprite in MAX.RES — sprite.json is not written.
        if !sprite_json_path.exists() {
            assert_eq!(unit.token, "MASTER");
            assert!(unit.notes.iter().any(|note| note.contains("нет в MAX.RES")));

            continue;
        }

        let sprite_json = read_json(&sprite_json_path);
        let data_resource = sprite_json["data_resource"]
            .as_str()
            .expect("data_resource")
            .to_string();
        let frame_info = FrameInfo::decode(archive.read(&data_resource).expect("D_*")).expect("FrameInfo");

        assert_eq!(
            sprite_json["image_base"].as_u64(),
            Some(u64::from(frame_info.image_base)),
            "{}: image_base",
            unit.token
        );
        assert_eq!(
            sprite_json["image_count"].as_u64(),
            Some(u64::from(frame_info.image_count)),
            "{}: image_count",
            unit.token
        );
        assert_eq!(
            sprite_json["turret_image_base"].as_u64(),
            Some(u64::from(frame_info.turret_image_base)),
            "{}: turret_image_base",
            unit.token
        );
        assert_eq!(
            sprite_json["turret_image_count"].as_u64(),
            Some(u64::from(frame_info.turret_image_count)),
            "{}: turret_image_count",
            unit.token
        );
        assert_eq!(
            sprite_json["firing_image_base"].as_u64(),
            Some(u64::from(frame_info.firing_image_base)),
            "{}: firing_image_base",
            unit.token
        );
        assert_eq!(
            sprite_json["connector_image_base"].as_u64(),
            Some(u64::from(frame_info.connector_image_base)),
            "{}: connector_image_base",
            unit.token
        );

        // `angle_offsets` — 8 pairs, 1:1 with FrameInfo.
        let angles = sprite_json["angle_offsets"].as_array().expect("angle_offsets");

        assert_eq!(angles.len(), 8, "{}: 8 углов", unit.token);

        for (index, (dx, dy)) in frame_info.angle_offsets.iter().enumerate() {
            assert_eq!(
                (
                    angles[index][0].as_i64().unwrap(),
                    angles[index][1].as_i64().unwrap()
                ),
                (i64::from(*dx), i64::from(*dy)),
                "{}: angle_offsets[{index}]",
                unit.token
            );
        }

        // The frames in JSON are exactly what is on disk, with sizes from the sprite.
        let sheet_tag = sprite_json["sprite_resource"].as_str().expect("sprite_resource");
        let sheet = decode_sprite(archive.read(sheet_tag).expect("спрайт")).expect("декод спрайта");
        let frames = sprite_json["frames"].as_array().expect("frames");

        assert_eq!(frames.len(), unit.body_frames + unit.turret_frames + unit.firing_frames + unit.connector_frames);

        for frame in frames {
            let relative = frame["file"].as_str().expect("file");
            let path = dir.join(relative);

            assert!(
                path.is_file(),
                "{}: нет файла {relative} из sprite.json",
                unit.token
            );

            let index = frame["sheet_index"].as_u64().expect("sheet_index") as usize;
            let source = &sheet.frames[index];

            assert_eq!(
                (frame["width"].as_u64().unwrap() as u32, frame["height"].as_u64().unwrap() as u32),
                (u32::from(source.width), u32::from(source.height)),
                "{}: {relative} размер кадра",
                unit.token
            );
            assert_eq!(png_size(&path), (u32::from(source.width), u32::from(source.height)));
            assert_eq!(
                frame["hotx"].as_i64(),
                Some(i64::from(source.hotx)),
                "{}: {relative} hotx",
                unit.token
            );
            assert_eq!(
                frame["hoty"].as_i64(),
                Some(i64::from(source.hoty)),
                "{}: {relative} hoty",
                unit.token
            );
        }

        // `sheet.png` matches the sprite.json entry in dimensions.
        let sheet_png = dir.join("sprites/sheet.png");

        assert_eq!(
            png_size(&sheet_png),
            (
                sprite_json["sheet"]["width"].as_u64().unwrap() as u32,
                sprite_json["sheet"]["height"].as_u64().unwrap() as u32
            ),
            "{}: sheet.png",
            unit.token
        );

        checked += 1;
    }

    assert_eq!(checked, 92, "все типы, у которых есть спрайт");
}

#[test]
fn animation_gif_is_a_valid_gif89a_with_flc_frames() {
    if !data_available() {
        return;
    }

    let shared = shared();
    let dir = units_dir(&shared.root).join("TANK");
    let bytes = fs::read(dir.join("animation/animation.gif")).expect("animation.gif");
    let (width, height, frames) = parse_gif(&bytes);

    assert_eq!((width, height), (128, 128), "FLC TANK03 — 128×128");
    assert_eq!(frames, 30, "кадров в FLC");
    assert_eq!(*bytes.last().unwrap(), 0x3B, "trailer GIF");

    // The PNG frames are the same size.
    assert_eq!(png_size(&dir.join("animation/frame_000.png")), (128, 128));
    assert_eq!(png_size(&dir.join("animation/frame_029.png")), (128, 128));

    // And for a building: a GIF exists wherever an FLC does.
    let commtower = units_dir(&shared.root).join("COMMTWR");
    let bytes = fs::read(commtower.join("animation/animation.gif")).expect("COMMTWR animation.gif");

    assert_eq!(parse_gif(&bytes).2, 1, "у COMMTWR FLC из одного кадра");
}

#[test]
fn units_without_resources_are_exported_with_notes() {
    if !data_available() {
        return;
    }

    let shared = shared();

    // MASTER is cut from the game: the tags exist in SC_UNITS, the resources are not in MAX.RES.
    let master = units_dir(&shared.root).join("MASTER");

    assert!(master.is_dir(), "каталог MASTER всё равно создан");
    assert!(master.join("unit.json").is_file());
    assert!(!master.join("portrait.png").exists());
    assert!(!master.join("sprites").exists());
    assert!(!master.join("animation").exists());

    let unit = read_json(&master.join("unit.json"));

    assert_eq!(unit["token"], "MASTER");
    assert_eq!(unit["portrait"]["source"], "none");
    assert_eq!(unit["sprite"]["frames_body"], 0);
    assert!(unit["animation"].is_null());
    assert!(
        unit["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("S_MASTER")),
        "отметка об отсутствующей тени"
    );

    // SUBMARNE: `S_SUBMAR` is missing from MAX.RES — the shadow is skipped with a note.
    let submarine = units_dir(&shared.root).join("SUBMARNE");

    assert!(submarine.is_dir());
    assert!(!submarine.join("shadow").exists(), "тени нет");

    let unit = read_json(&submarine.join("unit.json"));

    assert_eq!(unit["sprite"]["frames_shadow"], 0);
    assert!(unit["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str().unwrap().contains("S_SUBMAR")));
    assert!(unit["portrait"]["source"] == "resource");
}

#[test]
fn fallback_portrait_is_a_scaled_sprite_frame() {
    if !data_available() {
        return;
    }

    let shared = shared();

    // LNDEXPLD — no P_*: an 11×14 frame, a 300×240 fallback.
    let dir = units_dir(&shared.root).join("LNDEXPLD");
    let unit = read_json(&dir.join("unit.json"));

    assert_eq!(unit["portrait"]["source"], "fallback");
    assert_eq!(png_size(&dir.join("portrait.png")), (300, 240));

    let scale = unit["portrait"]["scale"].as_u64().unwrap() as u32;

    assert!(scale >= 1);
    assert_eq!(
        png_size(&dir.join("sprites/frame_000.png")),
        (
            unit["sprite"]["frame_width"].as_u64().unwrap() as u32,
            unit["sprite"]["frame_height"].as_u64().unwrap() as u32
        )
    );
    assert!(
        unit["sprite"]["frame_width"].as_u64().unwrap() as u32 * scale <= 300,
        "увеличенный кадр влезает в портрет по ширине"
    );
    assert!(
        unit["sprite"]["frame_height"].as_u64().unwrap() as u32 * scale <= 240,
        "увеличенный кадр влезает в портрет по высоте"
    );
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn repeated_export_is_byte_identical_and_leaves_no_junk() {
    if !data_available() {
        return;
    }

    let root = temp_dir("idempotent");
    let _ = fs::remove_dir_all(&root);

    let mut options = ExportOptions::defaults(repo_root());
    options.out_root = root.clone();

    let first = export_all(&options).expect("первый экспорт");
    assert!(first.errors.is_empty());

    let tree_before = collect_tree(&root);

    // Garbage that must disappear: an extra frame in a unit directory, a whole
    // "foreign" type directory and a file in the units root.
    fs::write(
        units_dir(&root).join("TANK/sprites/frame_999.png"),
        b"junk",
    )
    .expect("мусорный кадр");
    fs::create_dir_all(units_dir(&root).join("ZZZZ")).expect("мусорный каталог");
    fs::write(units_dir(&root).join("ZZZZ/unit.json"), b"{}").expect("мусорный json");
    fs::write(units_dir(&root).join("stray.txt"), b"stray").expect("мусорный файл");

    let second = export_all(&options).expect("повторный экспорт");

    assert!(second.errors.is_empty());
    assert_eq!(second.units.len(), first.units.len());
    assert_eq!(second.pngs, first.pngs);
    assert_eq!(second.bytes, first.bytes);

    assert!(!units_dir(&root).join("ZZZZ").exists(), "чужой каталог удалён");
    assert!(
        !units_dir(&root).join("stray.txt").exists(),
        "лишний файл удалён"
    );
    assert!(
        !units_dir(&root).join("TANK/sprites/frame_999.png").exists(),
        "лишний кадр удалён"
    );

    let tree_after = collect_tree(&root);

    assert_eq!(
        tree_before, tree_after,
        "повторный экспорт должен дать те же байты"
    );

    let _ = fs::remove_dir_all(&root);
}
