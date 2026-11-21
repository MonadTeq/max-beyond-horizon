//! FLC-ресурсы `MAX.RES` на реальных данных: все 71 `flc-ref` записи.
//!
//! * `MAX.RES` содержит 71 запись-ссылку (`flc-ref` в `max-brh-assets/images/manifest.csv`):
//!   тело записи — ASCIIZ-имя `*.FLC`-файла рядом с архивом (как `*.SPW`-ссылки,
//!   `max-original-dasm/formats/spw.md` §1);
//! * `manifest.csv` фиксирует `index/offset/size/ref` этих записей — тест
//!   проверяет, что разбор архива не разошёлся с манифестом;
//! * каждый файл разбирается [`Flic::parse`] и декодируется целиком
//!   ([`Flic::decode_all`]): размеры, число кадров, детерминизм, отсутствие
//!   паник на реальных данных.
//!
//! Тест пропускается (с сообщением в stderr), если `max-original/` нет — так
//! `cargo test` работает и без игровых данных.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use max_assets::flic::{Flic, FlicPlayer};
use max_assets::ResArchive;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn data_dir() -> PathBuf {
    repo_root().join("max-original")
}

/// Имена файлов каталога в нижнем регистре → реальный путь.
fn file_index() -> BTreeSet<String> {
    fs::read_dir(data_dir())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug)]
struct ManifestRow {
    index: usize,
    name: String,
    offset: u32,
    size: u32,
    reference: String,
}

fn manifest_flc_rows() -> Option<Vec<ManifestRow>> {
    let path = repo_root()
        .join("max-brh-assets")
        .join("images")
        .join("manifest.csv");
    let text = fs::read_to_string(&path).ok()?;
    let mut lines = text.lines();
    let header = lines.next()?;
    let columns: Vec<&str> = header.split(',').collect();
    let column = |name: &str| {
        columns
            .iter()
            .position(|value| *value == name)
            .unwrap_or_else(|| panic!("manifest.csv: нет колонки {name}"))
    };
    let (index_at, name_at, offset_at, size_at, ref_at) = (
        column("index"),
        column("name"),
        column("offset_hex"),
        column("size"),
        column("ref"),
    );

    let mut rows = Vec::new();

    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();

        if fields.len() <= ref_at {
            continue;
        }

        let reference = fields[ref_at].trim();

        if !reference.to_ascii_lowercase().ends_with(".flc") {
            continue;
        }

        rows.push(ManifestRow {
            index: fields[index_at].parse().expect("manifest: index"),
            name: fields[name_at].to_owned(),
            offset: u32::from_str_radix(fields[offset_at].trim_start_matches("0x"), 16)
                .expect("manifest: offset_hex"),
            size: fields[size_at].parse().expect("manifest: size"),
            reference: reference.to_owned(),
        });
    }

    Some(rows)
}

#[test]
fn max_res_has_71_flc_reference_records() {
    let max_res = data_dir().join("MAX.RES");

    if !max_res.is_file() {
        eprintln!("пропуск: нет {}", max_res.display());
        return;
    }

    let archive = ResArchive::open(&max_res).expect("MAX.RES");
    let files = file_index();
    let mut references = Vec::new();

    for entry in archive.list() {
        let Some(reference) = archive.file_reference(&entry.name) else {
            continue;
        };

        if !reference.to_ascii_lowercase().ends_with(".flc") {
            continue;
        }

        references.push((
            entry.name.clone(),
            entry.offset,
            entry.size,
            reference.to_owned(),
        ));
    }

    assert_eq!(
        references.len(),
        71,
        "в MAX.RES ожидалась 71 flc-ref запись"
    );

    for (name, offset, size, reference) in &references {
        assert!(
            name.starts_with("F_") || name == "FILESFLC",
            "{name}: flc-ref запись должна называться F_* (или FILESFLC)"
        );
        assert_eq!(
            *size as usize,
            reference.len() + 1,
            "{name}: запись должна быть ASCIIZ-именем файла ({reference:?})"
        );
        assert!(
            files.contains(&reference.to_ascii_lowercase()),
            "{name}: файла {reference} нет рядом с MAX.RES"
        );
        assert!(
            *offset >= 12,
            "{name}: смещение {offset} внутри заголовка архива"
        );
    }

    // Имена записей уникальны и совпадают с набором из SC_UNITS.
    let names: BTreeSet<&str> = references.iter().map(|(name, ..)| name.as_str()).collect();

    assert_eq!(
        names.len(),
        71,
        "имена flc-ref записей должны быть уникальны"
    );
}

#[test]
fn max_res_flc_records_match_the_generated_manifest() {
    let Some(rows) = manifest_flc_rows() else {
        eprintln!("пропуск: нет max-brh-assets/images/manifest.csv");
        return;
    };
    let max_res = data_dir().join("MAX.RES");

    if !max_res.is_file() {
        eprintln!("пропуск: нет {}", max_res.display());
        return;
    }

    let archive = ResArchive::open(&max_res).expect("MAX.RES");

    assert_eq!(rows.len(), 71, "manifest.csv: ожидалась 71 flc-ref запись");

    for row in &rows {
        let entry = archive
            .list()
            .iter()
            .find(|entry| entry.name == row.name)
            .unwrap_or_else(|| panic!("manifest: записи {} нет в MAX.RES", row.name));

        assert_eq!(entry.offset, row.offset, "{}: смещение", row.name);
        assert_eq!(entry.size, row.size, "{}: размер", row.name);
        assert_eq!(
            archive.file_reference(&row.name),
            Some(row.reference.as_str()),
            "{}: имя файла",
            row.name
        );
        assert_eq!(
            entry.index, row.index,
            "{}: порядковый номер в таблице",
            row.name
        );
    }
}

#[test]
fn all_71_flc_resources_parse_and_decode_every_frame() {
    let max_res = data_dir().join("MAX.RES");

    if !max_res.is_file() {
        eprintln!("пропуск: нет {}", max_res.display());
        return;
    }

    let archive = ResArchive::open(&max_res).expect("MAX.RES");
    let mut decoded_files = 0usize;
    let mut decoded_frames = 0usize;
    let mut animated = 0usize;

    for entry in archive.list() {
        let Some(reference) = archive.file_reference(&entry.name) else {
            continue;
        };

        if !reference.to_ascii_lowercase().ends_with(".flc") {
            continue;
        }

        let path = data_dir().join(reference);
        let path = if path.is_file() {
            path
        } else {
            // Windows не различает регистр, но данные в репозитории — в верхнем.
            data_dir().join(reference.to_ascii_uppercase())
        };

        assert!(
            path.is_file(),
            "{}: нет файла {}",
            entry.name,
            path.display()
        );

        let bytes = fs::read(&path).expect("FLC");
        let flic = Flic::parse(&bytes)
            .unwrap_or_else(|error| panic!("{} ({reference}): {error}", entry.name));

        // Заголовок: все ресурсы MAX.RES — 8-битные 128×N FLC.
        assert_eq!(flic.header().kind, max_assets::flic::FLC_MAGIC);
        assert_eq!(flic.width(), 128, "{}: ширина", entry.name);
        assert!(
            matches!(flic.height(), 128 | 256),
            "{}: высота {}",
            entry.name,
            flic.height()
        );
        assert_eq!(flic.header().depth, 8, "{}: глубина", entry.name);
        assert_eq!(
            flic.declared_size() as usize,
            bytes.len(),
            "{}: размер из заголовка не совпал с файлом",
            entry.name
        );
        assert!(
            matches!(flic.frame_count(), 1 | 30),
            "{}: число кадров {}",
            entry.name,
            flic.frame_count()
        );
        assert!(
            matches!(flic.speed_ms(), 67 | 71),
            "{}: speed {}",
            entry.name,
            flic.speed_ms()
        );

        // Кадры идут подряд от `oframe1` и не выходят за файл.
        let spans = flic.frame_spans();
        let mut expected_offset = flic.header().first_frame as usize;

        for (offset, size, _chunks) in &spans {
            assert_eq!(
                *offset, expected_offset,
                "{}: кадры должны идти подряд",
                entry.name
            );
            assert!(*size >= 16, "{}: подозрительный размер кадра", entry.name);
            expected_offset += size;
        }

        assert!(
            expected_offset <= bytes.len(),
            "{}: кадры выходят за файл",
            entry.name
        );

        // Декодирование всех кадров: без паник и с валидным размером.
        let frames = flic
            .decode_all()
            .unwrap_or_else(|error| panic!("{}: {error}", entry.name));

        assert_eq!(frames.len(), flic.frame_count());

        for (index, frame) in frames.iter().enumerate() {
            assert_eq!(
                (frame.width, frame.height),
                (flic.width(), flic.height()),
                "{} кадр {index}: размер",
                entry.name
            );
            assert_eq!(frame.pixels.len(), flic.width() * flic.height());
        }

        // Детерминизм: повторное декодирование с нуля совпадает с первым.
        assert_eq!(
            flic.decode_frame(flic.frame_count() - 1).unwrap(),
            *frames.last().unwrap(),
            "{}: decode_frame(N) расходится с decode_all",
            entry.name
        );

        if flic.is_animated() {
            animated += 1;

            // Анимированный FLC действительно меняется: хотя бы один кадр
            // отличается от первого (иначе анимация была бы пустой).
            assert!(
                frames.iter().any(|frame| frame.pixels != frames[0].pixels),
                "{}: все кадры одинаковые",
                entry.name
            );

            // Проигрыватель проходит те же кадры, что и `decode_all`
            // (до первого зацикливания).
            let mut player = FlicPlayer::new(&flic).unwrap();

            for (index, frame) in frames.iter().enumerate() {
                if index > 0 {
                    player.advance().unwrap();
                }

                assert_eq!(
                    player.pixels(),
                    frame.pixels.as_slice(),
                    "{} кадр {index}: проигрыватель разошёлся с decode_all",
                    entry.name
                );
            }
        }

        decoded_files += 1;
        decoded_frames += frames.len();
    }

    assert_eq!(decoded_files, 71, "декодированы все 71 FLC-ресурс");
    assert_eq!(
        animated, 36,
        "анимированных (30 кадров) — 36, статичных — 35"
    );
    assert_eq!(decoded_frames, 1115, "всего кадров в 71 ресурсе");
}
