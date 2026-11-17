//! Сверка декодера `max_assets::flic` с C++-эталоном (`tools/flic_vectors`).
//!
//! `vectors.txt` сгенерирован самостоятельной копией `max-cpp-port/src/flicsmgr.cpp`
//! (`tools/flic_vectors/main.cpp`) на всех 71 FLC-ресурсе MAX.RES:
//!
//! ```text
//! vector <file> <frame> <width> <height> <hex pixels>
//! digests <file> <frames> <fnv1a-32 каждого кадра>
//! ```
//!
//! Тест сверяет пиксели выбранных кадров бит-в-бит и FNV-1a-32 каждого кадра
//! каждого файла — то есть весь набор кадров, а не только первый.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use max_assets::flic::{Flic, FlicPlayer};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn vectors_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("flic_vectors")
        .join("vectors.txt")
}

/// Имена файлов `max-original` в нижнем регистре → реальный путь.
fn flc_index() -> BTreeMap<String, PathBuf> {
    let dir = repo_root().join("max-original");
    let mut index = BTreeMap::new();

    for entry in fs::read_dir(&dir).unwrap_or_else(|error| panic!("нет {}: {error}", dir.display()))
    {
        let path = entry.unwrap().path();

        if !path.is_file() {
            continue;
        }

        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();

        if name.ends_with(".flc") {
            index.insert(name, path);
        }
    }

    index
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 2_166_136_261u32;

    for &byte in bytes {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }

    hash
}

fn hex_to_bytes(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "нечётная hex-строка");

    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("hex"))
        .collect()
}

#[derive(Debug, Default)]
struct Vectors {
    pixels: Vec<(String, usize, usize, usize, Vec<u8>)>,
    digests: Vec<(String, Vec<u32>)>,
    files: Option<(usize, usize)>,
}

fn parse_vectors(text: &str) -> Vectors {
    let mut vectors = Vectors::default();

    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();

        match parts.next() {
            Some("vector") => {
                let file = parts.next().expect("vector: файл").to_owned();
                let frame = parts.next().expect("vector: кадр").parse().expect("кадр");
                let width = parts
                    .next()
                    .expect("vector: ширина")
                    .parse()
                    .expect("ширина");
                let height = parts
                    .next()
                    .expect("vector: высота")
                    .parse()
                    .expect("высота");
                let pixels = hex_to_bytes(parts.next().expect("vector: пиксели"));

                assert_eq!(
                    pixels.len(),
                    width * height,
                    "vector {file} {frame}: размер"
                );

                vectors.pixels.push((file, frame, width, height, pixels));
            }
            Some("digests") => {
                let file = parts.next().expect("digests: файл").to_owned();
                let count: usize = parts
                    .next()
                    .expect("digests: число")
                    .parse()
                    .expect("число");
                let hashes: Vec<u32> = parts
                    .map(|token| u32::from_str_radix(token, 16).expect("hash"))
                    .collect();

                assert_eq!(hashes.len(), count, "digests {file}: число хэшей");

                vectors.digests.push((file, hashes));
            }
            Some("files") => {
                let count = parts.next().expect("files: число").parse().expect("файлы");
                let frames = parts.next().expect("files: кадры").parse().expect("кадры");

                vectors.files = Some((count, frames));
            }
            other => panic!("неизвестная строка vectors.txt: {other:?}"),
        }
    }

    vectors
}

#[test]
fn vectors_match_cpp_flicsmgr_for_every_frame() {
    let text = fs::read_to_string(vectors_path())
        .unwrap_or_else(|error| panic!("нет {}: {error}", vectors_path().display()));
    let vectors = parse_vectors(&text);
    let index = flc_index();

    assert_eq!(vectors.pixels.len(), 6, "ожидались 6 пиксельных векторов");
    assert_eq!(vectors.digests.len(), 71, "ожидались 71 файл с хэшами");

    let (count, frames) = vectors.files.expect("итоговая строка files");

    assert_eq!(
        (count, frames),
        (71, 1115),
        "C++ эталон декодировал 71 файл / 1115 кадров"
    );

    let mut decoded_frames = 0usize;

    for (file, hashes) in &vectors.digests {
        let path = index
            .get(&file.to_ascii_lowercase())
            .unwrap_or_else(|| panic!("нет файла {file} в max-original"));
        let flic = Flic::open(path).unwrap_or_else(|error| panic!("{file}: {error}"));

        assert_eq!(
            flic.frame_count(),
            hashes.len(),
            "{file}: число кадров не совпало с C++"
        );

        // Инкрементальный проигрыватель: ровно тот же путь, что у C++
        // (`flicsmgr_advance_animation`), без повторного декодирования с нуля.
        let mut player = FlicPlayer::new(&flic).unwrap();

        for (frame, expected) in hashes.iter().enumerate() {
            if frame > 0 {
                assert_eq!(player.advance().unwrap(), frame, "{file}: кадр {frame}");
            }

            assert_eq!(
                fnv1a32(player.pixels()),
                *expected,
                "{file}: пиксели кадра {frame} отличаются от C++ (FNV-1a)"
            );
            decoded_frames += 1;
        }

        // Декодирование с нуля даёт тот же кадр, что и проигрыватель
        // (детерминизм: golden-кадры не зависят от истории проигрывания).
        let last = flic.frame_count() - 1;

        for frame in [0, last] {
            let direct = flic.decode_frame(frame).unwrap();

            assert_eq!(
                fnv1a32(&direct.pixels),
                hashes[frame],
                "{file}: decode_frame({frame}) расходится с C++"
            );
        }
    }

    assert_eq!(decoded_frames, frames, "декодированы все кадры эталона");

    for (file, frame, width, height, pixels) in &vectors.pixels {
        let path = index
            .get(&file.to_ascii_lowercase())
            .unwrap_or_else(|| panic!("нет файла {file} в max-original"));
        let flic = Flic::open(path).unwrap();
        let decoded = flic.decode_frame(*frame).unwrap();

        assert_eq!(
            (decoded.width, decoded.height),
            (*width, *height),
            "{file}: размер"
        );
        assert_eq!(
            &decoded.pixels, pixels,
            "{file} кадр {frame}: пиксели отличаются от C++ бит-в-бит"
        );
    }
}

#[test]
fn vectors_cover_every_flc_resource_referenced_by_max_res() {
    // 71 flc-ref запись MAX.RES: имя файла в каждой совпадает с набором
    // из `vectors.txt` (нижний регистр).
    let archive = max_assets::ResArchive::open(repo_root().join("max-original").join("MAX.RES"))
        .unwrap_or_else(|error| panic!("MAX.RES: {error}"));
    let mut referenced: Vec<String> = archive
        .list()
        .iter()
        .filter_map(|entry| archive.file_reference(&entry.name).map(str::to_owned))
        .filter(|name| name.to_ascii_lowercase().ends_with(".flc"))
        .collect();

    referenced.sort();
    referenced.dedup();

    assert_eq!(
        referenced.len(),
        71,
        "в MAX.RES ожидалась 71 flc-ref запись"
    );

    let text = fs::read_to_string(vectors_path()).unwrap();
    let vectors = parse_vectors(&text);
    let mut covered: Vec<String> = vectors
        .digests
        .iter()
        .map(|(file, _)| file.to_ascii_lowercase())
        .collect();

    covered.sort();

    assert_eq!(
        covered, referenced,
        "набор FLC в эталоне и в MAX.RES различается"
    );
}
