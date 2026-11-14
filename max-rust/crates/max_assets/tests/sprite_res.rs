//! Интеграционные тесты декодера спрайтов на эталонных телах из MAX.RES.
//!
//! Фикстуры (`tests/data/`, сгенерированы `max-rust/tools/gen_sprite_reference.py`;
//! тела ресурсов извлечены эталонным `max-original-dasm/tools/res_parser.py`):
//! * `sprite_TANK.bin` — тело ресурса `TANK` (35 211 байт, 24 кадра);
//! * `sprite_S_TANK.bin` — тело ресурса `S_TANK` (3 604 байта, 24 кадра тени);
//! * `sprite_COMMTWR.bin` — тело ресурса `COMMTWR` (14 153 байта, 10 кадров);
//! * `sprite_ref.txt` — эталонный вывод Python-декодера по
//!   `max-original-dasm/formats/spw.md`: заголовки всех кадров и полные строки
//!   пикселей выбранных кадров (`..` — прозрачность, `#` — затемнение тени).
//!
//! Числа в тестах взяты из спецификации формата (spw.md §3–§4) и реальных
//! файлов, поэтому они проверяют именно совместимость, а не сами себя.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use max_assets::sprite::{
    decode, decode_shadow, decode_sprite, DecodedSprite, SpriteError, SpriteFrame,
};

// ---------------------------------------------------------------------------
// Фикстуры
// ---------------------------------------------------------------------------

fn data_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(data_path(name)).unwrap_or_else(|error| panic!("не читается {name}: {error}"))
}

/// Один кадр эталона: заголовок + дамп строк (только те, что попали в дамп).
#[derive(Debug, Default)]
struct RefFrame {
    width: u16,
    height: u16,
    hotx: i16,
    hoty: i16,
    rows: BTreeMap<usize, Vec<String>>,
}

/// Эталон одного ресурса.
#[derive(Debug, Default)]
struct RefSprite {
    tag: String,
    bytes: usize,
    count: u16,
    shadow: bool,
    frames: Vec<RefFrame>,
}

/// Разбирает `sprite_ref.txt` в структуры (порядок секций — как в генераторе).
fn read_reference() -> Vec<RefSprite> {
    let text = fs::read_to_string(data_path("sprite_ref.txt")).unwrap();
    let mut sprites: Vec<RefSprite> = Vec::new();

    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let tokens: Vec<&str> = line.split_whitespace().collect();

        match tokens[0] {
            "sprite" => {
                let mut sprite = RefSprite {
                    tag: tokens[1].to_owned(),
                    ..RefSprite::default()
                };

                for token in &tokens[2..] {
                    let (key, value) = token.split_once('=').expect("ключ=значение");

                    match key {
                        "bytes" => sprite.bytes = value.parse().unwrap(),
                        "count" => sprite.count = value.parse().unwrap(),
                        "shadow" => sprite.shadow = value == "1",
                        other => panic!("неизвестное поле спрайта: {other}"),
                    }
                }

                sprites.push(sprite);
            }
            "frame" => {
                let sprite = sprites.last_mut().expect("frame без sprite");
                let frame = RefFrame {
                    width: tokens[2].parse().unwrap(),
                    height: tokens[3].parse().unwrap(),
                    hotx: tokens[4].parse().unwrap(),
                    hoty: tokens[5].parse().unwrap(),
                    rows: BTreeMap::new(),
                };

                assert!(
                    frame.width > 0 && frame.height > 0,
                    "кадр {} пустой",
                    tokens[1]
                );
                sprite.frames.push(frame);
            }
            "row" => {
                let sprite = sprites.last_mut().expect("row без sprite");
                let frame = sprite.frames.last_mut().expect("row без frame");
                let y: usize = tokens[1].parse().unwrap();
                let cells: Vec<String> =
                    tokens[2..].iter().map(|token| token.to_string()).collect();

                assert_eq!(cells.len(), frame.width as usize, "строка {y} неполная");

                if frame.rows.insert(y, cells).is_some() {
                    panic!("строка {y} описана дважды");
                }
            }
            other => panic!("неизвестная строка эталона: {other}"),
        }
    }

    assert!(!sprites.is_empty(), "эталон пуст");

    sprites
}

/// Пиксельный токен эталона -> `Option<u8>` (для спрайта).
fn ref_pixel(token: &str) -> Option<u8> {
    if token == ".." {
        None
    } else {
        Some(u8::from_str_radix(token, 16).unwrap_or_else(|_| panic!("не hex-пиксель: {token:?}")))
    }
}

/// Формат пикселя кадра для сообщений об ошибках.
fn pixel_text(frame: &SpriteFrame, x: usize, y: usize) -> String {
    match frame.pixel(x as u32, y as u32) {
        Some(index) => format!("{index:02x}"),
        None => "..".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Эталон: каждый кадр и каждая дампированная строка
// ---------------------------------------------------------------------------

#[test]
fn reference_dumps_decode_bit_exactly() {
    for reference in read_reference() {
        let bytes = read_fixture(&format!("sprite_{}.bin", reference.tag));

        assert_eq!(
            bytes.len(),
            reference.bytes,
            "{}: размер тела",
            reference.tag
        );

        let sheet = if reference.shadow {
            None
        } else {
            Some(decode_sprite(&bytes).unwrap_or_else(|error| panic!("{}: {error}", reference.tag)))
        };
        let shadow = if reference.shadow {
            Some(decode_shadow(&bytes).unwrap_or_else(|error| panic!("{}: {error}", reference.tag)))
        } else {
            None
        };
        let count = sheet
            .as_ref()
            .map(|sheet| sheet.image_count)
            .or_else(|| shadow.as_ref().map(|sheet| sheet.image_count))
            .unwrap();

        assert_eq!(count, reference.count, "{}: image_count", reference.tag);
        assert_eq!(reference.frames.len(), reference.count as usize);

        for (index, expected) in reference.frames.iter().enumerate() {
            if let Some(sheet) = &sheet {
                let frame = sheet.frame(index).expect("кадр есть");

                assert_eq!(
                    frame.width, expected.width,
                    "{}[{index}]: width",
                    reference.tag
                );
                assert_eq!(
                    frame.height, expected.height,
                    "{}[{index}]: height",
                    reference.tag
                );
                assert_eq!(
                    frame.hotx, expected.hotx,
                    "{}[{index}]: hotx",
                    reference.tag
                );
                assert_eq!(
                    frame.hoty, expected.hoty,
                    "{}[{index}]: hoty",
                    reference.tag
                );
                assert_eq!(
                    frame.pixels.len(),
                    frame.width as usize * frame.height as usize
                );

                for (&y, cells) in &expected.rows {
                    for (x, token) in cells.iter().enumerate() {
                        assert_eq!(
                            pixel_text(frame, x, y),
                            *token,
                            "{}[{index}] пиксель ({x},{y})",
                            reference.tag
                        );
                        assert_eq!(
                            frame.pixel(x as u32, y as u32),
                            ref_pixel(token),
                            "{}[{index}] пиксель ({x},{y}) значение",
                            reference.tag
                        );
                    }
                }
            } else if let Some(shadow) = &shadow {
                let frame = shadow.frame(index).expect("кадр есть");

                assert_eq!(
                    frame.width, expected.width,
                    "{}[{index}]: width",
                    reference.tag
                );
                assert_eq!(
                    frame.height, expected.height,
                    "{}[{index}]: height",
                    reference.tag
                );
                assert_eq!(
                    frame.hotx, expected.hotx,
                    "{}[{index}]: hotx",
                    reference.tag
                );
                assert_eq!(
                    frame.hoty, expected.hoty,
                    "{}[{index}]: hoty",
                    reference.tag
                );

                for (&y, cells) in &expected.rows {
                    for (x, token) in cells.iter().enumerate() {
                        let darkened = token == "#";

                        assert_eq!(
                            frame.darkened(x as u32, y as u32),
                            darkened,
                            "{}[{index}] тень ({x},{y})",
                            reference.tag
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TANK — спецификация §3.1
// ---------------------------------------------------------------------------

#[test]
fn tank_matches_the_documented_hex() {
    let bytes = read_fixture("sprite_TANK.bin");

    assert_eq!(bytes.len(), 35_211);
    assert_eq!(&bytes[0..2], &[0x18, 0x00], "image_count = 24");
    assert_eq!(
        &bytes[2..6],
        &[0x62, 0x00, 0x00, 0x00],
        "frame[0] @ 0x62 = 2 + 4*24"
    );

    let sheet = decode_sprite(&bytes).unwrap();

    assert_eq!(sheet.image_count, 24);
    assert_eq!(sheet.len(), 24);

    let frame = sheet.frame(0).unwrap();

    assert_eq!((frame.width, frame.height), (44, 45));
    assert_eq!((frame.hotx, frame.hoty), (20, 23));

    // §3.1: строка 0 — прогоны {4,5} и {25,5}, итого 39 из 44 пикселей.
    assert_eq!(frame.rows[0].painted, 39);
    assert_eq!(
        frame.rows[0].runs,
        vec![
            max_assets::SpriteRun {
                skip: 4,
                count: 5,
                pixels: vec![0xCF, 0xCE, 0xCB, 0xCA, 0xF0],
            },
            max_assets::SpriteRun {
                skip: 25,
                count: 5,
                pixels: vec![0xCF, 0xCA, 0xC7, 0xD1, 0xE8],
            },
        ]
    );

    // Хвост строки (x = 39..44) прозрачен и в файле отсутствует.
    for x in 0..4 {
        assert_eq!(frame.pixel(x, 0), None, "x={x} прозрачен");
    }
    assert_eq!(frame.pixel(4, 0), Some(0xCF));
    assert_eq!(frame.pixel(8, 0), Some(0xF0));
    assert_eq!(frame.pixel(9, 0), None);
    assert_eq!(frame.pixel(34, 0), Some(0xCF));
    assert_eq!(frame.pixel(38, 0), Some(0xE8));
    for x in 39..44 {
        assert_eq!(frame.pixel(x, 0), None, "хвост x={x} прозрачен");
    }

    // Размеры кадров разные (кадр 1 — 56×57, §5.3): у каждого свои заголовки.
    assert_eq!((sheet.frames[1].width, sheet.frames[1].height), (56, 57));
    assert!(sheet
        .frames
        .iter()
        .all(|frame| frame.rows.len() == frame.height as usize));
}

#[test]
fn tank_frame_pixels_are_never_outside_the_frame() {
    let bytes = read_fixture("sprite_TANK.bin");
    let sheet = decode_sprite(&bytes).unwrap();

    for (index, frame) in sheet.frames.iter().enumerate() {
        let pixels = frame.width as usize * frame.height as usize;

        assert_eq!(frame.pixels.len(), pixels, "кадр {index}");
        assert!(frame.opaque_pixels() > 0, "кадр {index} не пустой");
        assert!(
            frame
                .rows
                .iter()
                .all(|row| row.painted as u16 <= frame.width),
            "кадр {index}: сумма прогонов не выходит за width"
        );
    }
}

// ---------------------------------------------------------------------------
// S_TANK — тени §3.2
// ---------------------------------------------------------------------------

#[test]
fn s_tank_shadow_has_no_pixel_bytes() {
    let bytes = read_fixture("sprite_S_TANK.bin");

    assert_eq!(bytes.len(), 3_604);

    let sheet = decode_shadow(&bytes).unwrap();

    assert_eq!(sheet.image_count, 24);

    let frame = sheet.frame(0).unwrap();

    assert_eq!((frame.width, frame.height), (49, 53));
    assert_eq!((frame.hotx, frame.hoty), (20, 23));

    // §3.2: строка 0 — {4,5}, {25,5}, 0xFF -> покрыто 39 пикселей из 49.
    assert_eq!(frame.rows[0].painted, 39);
    assert_eq!(
        frame.rows[0].runs,
        vec![
            max_assets::SpriteRun {
                skip: 4,
                count: 5,
                pixels: Vec::new(),
            },
            max_assets::SpriteRun {
                skip: 25,
                count: 5,
                pixels: Vec::new(),
            },
        ]
    );
    assert!(frame.darkened_pixels() > 0);
    assert!(!frame.darkened(0, 0));
    assert!(frame.darkened(4, 0));
    assert!(frame.darkened(8, 0));
    assert!(!frame.darkened(9, 0));
    // Второй прогон {25, 5}: 4 + 5 + 25 = 34 ..= 38.
    assert!(!frame.darkened(33, 0));
    assert!(frame.darkened(34, 0));
    assert!(frame.darkened(38, 0));
    assert!(!frame.darkened(39, 0));

    // В тени нет пиксельных байтов — это видно по разобранным строкам.
    assert!(frame
        .rows
        .iter()
        .flat_map(|row| &row.runs)
        .all(max_assets::SpriteRun::is_shadow));
}

#[test]
fn shadow_mode_differs_from_sprite_mode_for_the_same_body() {
    let bytes = read_fixture("sprite_S_TANK.bin");

    assert!(matches!(
        decode(&bytes, true).unwrap(),
        DecodedSprite::Shadow(_)
    ));
    // В обычном режиме те же байты не дают спрайта: пары счётчиков тени
    // читаются как счётчики+пиксели, и строки выходят за width (§3.4).
    assert!(matches!(
        decode(&bytes, false),
        Err(SpriteError::RowOverrun { .. })
            | Err(SpriteError::PixelPayloadTruncated { .. })
            | Err(SpriteError::RowUnterminated { .. })
    ));
}

// ---------------------------------------------------------------------------
// COMMTWR — здание §3.3
// ---------------------------------------------------------------------------

#[test]
fn commtower_has_ten_frames_of_arbitrary_size() {
    let bytes = read_fixture("sprite_COMMTWR.bin");

    assert_eq!(bytes.len(), 14_153);

    let sheet = decode_sprite(&bytes).unwrap();

    assert_eq!(sheet.image_count, 10);
    assert_eq!(sheet.len(), 10);

    let frame = sheet.frame(0).unwrap();

    assert_eq!((frame.width, frame.height), (112, 63));
    assert_eq!((frame.hotx, frame.hoty), (53, 29));

    // Число кадров здания не кратно 8 — раскладка по направлениям ограничена
    // `image_count`, а не предполагаемыми фазами.
    assert!(sheet.directional_frame(0, 1, 0).is_some()); // кадр 8
    assert!(sheet.directional_frame(0, 2, 0).is_none()); // кадр 16 >= 10
    assert!(sheet.frame(10).is_none());
}

// ---------------------------------------------------------------------------
// Ошибки на реальных байтах
// ---------------------------------------------------------------------------

#[test]
fn truncated_real_bodies_are_errors_not_panics() {
    let bytes = read_fixture("sprite_TANK.bin");

    // Обрезка по всем «интересным» границам формата.
    for cut in [0, 1, 2, 4, 97, 98, 106, 200, 286, 1_000, 10_000, 35_210] {
        let _ = decode_sprite(&bytes[..cut]);
        let _ = decode_shadow(&bytes[..cut]);
    }

    assert!(matches!(
        decode_sprite(&bytes[..1]),
        Err(SpriteError::TooShort { .. })
    ));
    // Таблица смещений кадров: 2 + 4*24 = 98 байт, поэтому 97 — ещё обрезка.
    assert!(matches!(
        decode_sprite(&bytes[..97]),
        Err(SpriteError::OffsetTableTruncated { .. })
    ));
    assert!(matches!(
        decode_sprite(&bytes[..100]),
        Err(SpriteError::FrameOutOfBounds { .. } | SpriteError::FrameTruncated { .. })
    ));

    // Полное тело по-прежнему декодируется целиком: все 24 кадра непустые.
    let sheet = decode_sprite(&bytes).unwrap();

    assert_eq!(sheet.len(), 24);
    assert!(sheet.frames.iter().all(|frame| frame.opaque_pixels() > 0));
}
