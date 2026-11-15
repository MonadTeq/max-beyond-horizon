//! Разбор UI-картинок из `MAX.RES` (FRAMEPIC/PANELTOP/PANELBTM) и дамп в BMP.
//!
//! ```sh
//! cargo run -p max_assets --example ui_images -- ../max-original/MAX.RES /tmp/ui
//! ```

use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

use max_assets::image::{BigImage, SimpleImage};
use max_assets::ResArchive;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let res_path = args.next().unwrap_or_else(|| "../../max-original/MAX.RES".to_string());
    let out_prefix = args.next().unwrap_or_else(|| "ui".to_string());

    let archive = ResArchive::open(&res_path)?;

    match archive.read("FRAMEPIC") {
        Ok(bytes) => match BigImage::decode(bytes) {
            Ok(image) => {
                println!(
                    "FRAMEPIC: {}x{} в ({},{}), палитра[1]={:?}, прозрачный={}",
                    image.width,
                    image.height,
                    image.ulx,
                    image.uly,
                    image.color(1),
                    image.transparent
                );
                let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
                for y in 0..image.height as u32 {
                    for x in 0..image.width as u32 {
                        rgb.extend_from_slice(&image.rgb(x, y).unwrap_or([0, 0, 0]));
                    }
                }
                let path = PathBuf::from(format!("{out_prefix}-framepic.bmp"));
                write_bmp(&path, image.width as u32, image.height as u32, &rgb)?;
                println!("  -> {}", path.display());
            }
            Err(error) => println!("FRAMEPIC: ошибка разбора: {error}"),
        },
        Err(error) => println!("FRAMEPIC: нет в архиве: {error}"),
    }

    for name in ["PANELTOP", "PANELBTM"] {
        match archive.read(name) {
            Ok(bytes) => match SimpleImage::decode(bytes) {
                Ok(image) => println!(
                    "{name}: {}x{} в ({},{}), прозрачный={}",
                    image.width, image.height, image.ulx, image.uly, image.transparent
                ),
                Err(error) => println!("{name}: ошибка разбора: {error}"),
            },
            Err(error) => println!("{name}: нет в архиве: {error}"),
        }
    }

    Ok(())
}

fn write_bmp(path: &PathBuf, width: u32, height: u32, rgb: &[u8]) -> io::Result<()> {
    let stride = (width * 3 + 3) & !3;
    let image_size = stride * height;
    let mut out = Vec::with_capacity((54 + image_size) as usize);

    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + image_size).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&image_size.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..height).rev() {
        let mut row = Vec::with_capacity(stride as usize);
        for x in 0..width {
            let base = ((y * width + x) * 3) as usize;
            row.extend_from_slice(&[rgb[base + 2], rgb[base + 1], rgb[base]]);
        }
        while row.len() < stride as usize {
            row.push(0);
        }
        out.extend_from_slice(&row);
    }

    File::create(path)?.write_all(&out)
}
