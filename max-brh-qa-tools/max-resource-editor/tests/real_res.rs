//! Интеграционные тесты на реальном `MAX.RES`/`PATCHES.RES`.
//!
//! Файлы игры лежат в репозитории (`max-original/`, `assets/`); если их нет,
//! тесты пропускаются с сообщением, а не падают — как в `max_assets`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use max_assets::ResArchive;
use max_render::palette::Palette;
use max_resource_editor::export::{export_archive, ExportOptions};
use max_resource_editor::model::{classify_archive, Category, Kind, NonImageKind, ResourceInfo};
use max_resource_editor::render::strip_dimensions;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn max_res() -> PathBuf {
    root().join("max-original/MAX.RES")
}

fn patches_res() -> PathBuf {
    root().join("assets/PATCHES.RES")
}

fn color_pal() -> PathBuf {
    root().join("max-original/COLOR.PAL")
}

fn classify(path: &Path) -> Option<Vec<ResourceInfo>> {
    if !path.is_file() {
        eprintln!("skip: {} отсутствует", path.display());
        return None;
    }

    let archive = ResArchive::open(path).expect("RES открывается");
    Some(classify_archive(&archive))
}

fn find<'a>(infos: &'a [ResourceInfo], name: &str) -> &'a ResourceInfo {
    infos
        .iter()
        .find(|info| info.name == name)
        .unwrap_or_else(|| panic!("нет ресурса {name}"))
}

#[test]
fn all_1828_max_res_entries_are_classified() {
    let Some(infos) = classify(&max_res()) else {
        return;
    };

    assert_eq!(infos.len(), 1828, "в MAX.RES ровно 1828 записей");

    for info in &infos {
        if info.kind.is_image() {
            assert_ne!(
                info.category,
                Category::NonImage,
                "{}: изображение без категории",
                info.name
            );
            assert!(
                !info.frames.is_empty(),
                "{}: изображение без кадров",
                info.name
            );
            assert!(
                info.width > 0 && info.height > 0,
                "{}: неположительный размер",
                info.name
            );
        } else {
            assert!(
                info.non_image.is_some(),
                "{}: не-изображение без подтипа",
                info.name
            );
            assert_eq!(info.category, Category::NonImage);
        }
    }

    let stats = max_resource_editor::model::ClassificationStats::from_infos(&infos);
    assert_eq!(stats.entries, 1828);
    assert_eq!(stats.images, 1070, "изображений в MAX.RES");
    assert_eq!(stats.non_images, 758, "не-изображений в MAX.RES");
    assert_eq!(stats.ui, 537);
    assert_eq!(stats.units, 170);
    assert_eq!(stats.misc, 363);
    assert_eq!(stats.simple, 649);
    assert_eq!(stats.big, 152);
    assert_eq!(stats.sprite, 162);
    assert_eq!(stats.shadow, 78);
    assert_eq!(stats.cursor, 29);
    assert_eq!(stats.non_image_count(NonImageKind::WavRef), 607);
    assert_eq!(stats.non_image_count(NonImageKind::FlcRef), 71);
    assert_eq!(stats.non_image_count(NonImageKind::WrlRef), 24);
    assert_eq!(stats.non_image_count(NonImageKind::MscRef), 16);
    assert_eq!(stats.non_image_count(NonImageKind::MveRef), 3);
    assert_eq!(stats.non_image_count(NonImageKind::Data), 32);
    assert_eq!(stats.non_image_count(NonImageKind::Script), 3);
    assert_eq!(stats.non_image_count(NonImageKind::Text), 2);
}

#[test]
fn known_tags_have_expected_kind_and_category() {
    let Some(infos) = classify(&max_res()) else {
        return;
    };

    let framepic = find(&infos, "FRAMEPIC");
    assert_eq!(framepic.kind, Kind::Big);
    assert_eq!(framepic.category, Category::Ui);
    assert_eq!((framepic.width, framepic.height), (640, 480));
    assert_eq!(framepic.palette.as_str(), "own");

    let tank = find(&infos, "TANK");
    assert_eq!(tank.kind, Kind::Sprite);
    assert_eq!(tank.category, Category::Units);
    assert_eq!(tank.frame_count, 24);

    let shadow = find(&infos, "S_TANK");
    assert_eq!(shadow.kind, Kind::Shadow);
    assert_eq!(shadow.category, Category::Units);
    assert_eq!(shadow.frame_count, 24);

    let hand = find(&infos, "HANDPTR");
    assert_eq!(hand.kind, Kind::Cursor);
    assert_eq!(hand.category, Category::Misc);
    assert_eq!((hand.width, hand.height), (18, 18));
    assert_eq!(hand.frame_count, 1);

    let snow = find(&infos, "SNOW_PIC");
    assert_eq!(snow.kind, Kind::Big);
    assert_eq!(snow.category, Category::Misc);
    assert_eq!((snow.width, snow.height), (128, 128));

    let panel = find(&infos, "PANELTOP");
    assert_eq!(panel.kind, Kind::Simple);
    assert_eq!(panel.category, Category::Ui);
    assert_eq!((panel.width, panel.height), (171, 239));

    let portrait = find(&infos, "P_TANK");
    assert_eq!(portrait.kind, Kind::Big);
    assert_eq!(portrait.category, Category::Misc);
    assert_eq!((portrait.width, portrait.height), (300, 240));

    let icon = find(&infos, "I_TANK");
    assert_eq!(icon.kind, Kind::Sprite);
    assert_eq!(icon.category, Category::Misc);

    let tank1 = find(&infos, "TANK1");
    assert_eq!(tank1.kind, Kind::NonImage);
    assert_eq!(tank1.non_image, Some(NonImageKind::WavRef));
    assert_eq!(tank1.file_ref.as_deref(), Some("tank1.spw"));

    let snow1 = find(&infos, "SNOW_1");
    assert_eq!(snow1.non_image, Some(NonImageKind::WrlRef));
    assert_eq!(snow1.file_ref.as_deref(), Some("snow_1.wrl"));
}

#[test]
fn font_ttf_in_patches_is_non_image_ttf() {
    let Some(infos) = classify(&patches_res()) else {
        return;
    };

    let font = find(&infos, "FONT_1");
    assert_eq!(font.kind, Kind::NonImage);
    assert_eq!(font.category, Category::NonImage);
    assert_eq!(font.non_image, Some(NonImageKind::Ttf));
    assert_eq!(font.size, 31_896);
    assert!(
        font.first_bytes.starts_with(&[0x00, 0x01, 0x00, 0x00]),
        "FONT_1 — TrueType: {:02X?}",
        font.first_bytes
    );
}

#[test]
fn export_of_selected_images_has_expected_dimensions_and_is_idempotent() {
    let (res, pal) = (max_res(), color_pal());
    if !res.is_file() || !pal.is_file() {
        eprintln!("skip: нет MAX.RES/COLOR.PAL");
        return;
    }

    let archive = ResArchive::open(&res).expect("MAX.RES");
    let palette = Palette::load(&pal).expect("COLOR.PAL");
    let infos = classify_archive(&archive);
    let dir = std::env::temp_dir().join(format!(
        "max_res_editor_export_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp");

    let options = ExportOptions::only(&["TANK", "FRAMEPIC", "SNOW_PIC"]);
    let first = export_archive(&archive, &palette, &dir, &infos, &options).expect("export");
    assert!(first.errors.is_empty(), "ошибки: {:?}", first.errors);
    assert_eq!(first.exported, 3);

    let files: BTreeMap<String, Vec<u8>> = first
        .files
        .iter()
        .map(|relative| {
            let bytes = std::fs::read(dir.join(relative)).expect("чтение PNG");
            (relative.clone(), bytes)
        })
        .collect();
    assert_eq!(files.len(), 3);

    // TANK: лента 24 кадров, 44x45 первый кадр.
    let tank_name = first
        .files
        .iter()
        .find(|path| path.contains("/TANK_"))
        .expect("TANK PNG");
    let tank = max_resource_editor::png::read(dir.join(tank_name)).expect("TANK PNG читается");
    let tank_info = find(&infos, "TANK");
    let tank_frames: Vec<_> = tank_info
        .frames
        .iter()
        .map(|frame| (frame.width, frame.height, frame.hot_x, frame.hot_y))
        .collect();
    let (expected_w, expected_h) = strip_dimensions(&tank_frames);
    assert_eq!(
        (tank.width, tank.height),
        (1620, 92),
        "TANK — известная лента 24 кадров"
    );
    assert_eq!((tank.width, tank.height), (expected_w, expected_h));
    assert!(tank.width > tank.height, "TANK — горизонтальная лента");

    let framepic_name = first
        .files
        .iter()
        .find(|path| path.contains("/FRAMEPIC_"))
        .expect("FRAMEPIC PNG");
    let framepic =
        max_resource_editor::png::read(dir.join(framepic_name)).expect("FRAMEPIC PNG читается");
    assert_eq!((framepic.width, framepic.height), (640, 480));

    let snow_name = first
        .files
        .iter()
        .find(|path| path.contains("/SNOW_PIC_"))
        .expect("SNOW_PIC PNG");
    let snow = max_resource_editor::png::read(dir.join(snow_name)).expect("SNOW_PIC PNG читается");
    assert_eq!((snow.width, snow.height), (128, 128));

    // Повторный экспорт в тот же каталог — те же байты.
    let second = export_archive(&archive, &palette, &dir, &infos, &options).expect("export #2");
    assert_eq!(second.exported, 3);
    for (relative, expected) in &files {
        let actual = std::fs::read(dir.join(relative)).expect("чтение PNG #2");
        assert_eq!(&actual, expected, "файл {relative} изменился при повторе");
    }

    std::fs::remove_dir_all(&dir).ok();
}
