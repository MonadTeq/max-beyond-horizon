//! Интеграционные тесты чистой части `max_render` на эталонных данных.
//!
//! Фикстуры:
//! * `tests/data/COLOR.PAL` — копия `max-original/COLOR.PAL`;
//! * `tests/data/palette_ref.txt` — эталон из
//!   `max-rust/tools/gen_sprite_reference.py`: 8-битные значения палитры и LUT
//!   пяти команд (по `Color_SetSystemPaletteEntry` и
//!   `ResourceManager_BuildColorTables`);
//! * спрайты `TANK`/`S_TANK` — тела RES из `max_assets/tests/data`, декодер
//!   которых уже сверен с Python-эталоном (`max_assets/tests/sprite_res.rs`).
//!
//! Тест проверяет связку целиком: эталонный байт спрайта -> декодер ->
//! палитра/командная LUT -> RGBA-буфер, без SDL.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use max_assets::sprite::{decode_shadow, decode_sprite};
use max_assets::WrlMap;
use max_core::enums::Surface;
use max_core::map::{Map, ResourceKind, ResourceMap, ResourceMapEntry, SurfaceMap};
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::map_render::{
    map_from_wrl, render_map_into, render_tile, render_tile_into, TileStyle, TILE_PIXELS,
};
use max_render::palette::{
    BrightnessLut, ColorLut, Palette, TeamColors, FULL_BRIGHTNESS_LEVEL, PALETTE_SIZE,
    SHADOW_BRIGHTNESS_LEVEL,
};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::viewport::Viewport;

// ---------------------------------------------------------------------------
// Фикстуры
// ---------------------------------------------------------------------------

fn render_data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

/// Спрайты лежат в фикстурах `max_assets` — общий эталон для двух крейтов.
fn assets_data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("max_assets")
        .join("tests")
        .join("data")
        .join(name)
}

fn read(path: PathBuf) -> Vec<u8> {
    fs::read(&path).unwrap_or_else(|error| panic!("не читается {}: {error}", path.display()))
}

/// Эталон палитры и командных LUT из Python-генератора.
struct PaletteReference {
    rgb: [[u8; 3]; PALETTE_SIZE],
    teams: BTreeMap<String, [u8; PALETTE_SIZE]>,
}

fn read_palette_reference() -> PaletteReference {
    let text = fs::read_to_string(render_data("palette_ref.txt")).unwrap();
    let mut rgb = [[0u8; 3]; PALETTE_SIZE];
    let mut teams = BTreeMap::new();

    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let tokens: Vec<&str> = line.split_whitespace().collect();

        match tokens[0] {
            "palette" => {
                let index: usize = tokens[1].parse().unwrap();

                assert_eq!(tokens.len(), 8, "строка палитры {index}");

                rgb[index] = [
                    tokens[5].parse().unwrap(),
                    tokens[6].parse().unwrap(),
                    tokens[7].parse().unwrap(),
                ];
            }
            "team" => {
                let mut lut = [0u8; PALETTE_SIZE];

                assert_eq!(tokens.len(), 2 + PALETTE_SIZE, "LUT команды {}", tokens[1]);

                for (index, token) in tokens[2..].iter().enumerate() {
                    lut[index] = token.parse().unwrap();
                }

                teams.insert(tokens[1].to_owned(), lut);
            }
            other => panic!("неизвестная строка эталона: {other}"),
        }
    }

    assert_eq!(teams.len(), 5);

    PaletteReference { rgb, teams }
}

fn loaded_palette() -> Palette {
    Palette::load(render_data("COLOR.PAL")).expect("COLOR.PAL читается")
}

fn tank_bytes() -> Vec<u8> {
    read(assets_data("sprite_TANK.bin"))
}

fn s_tank_bytes() -> Vec<u8> {
    read(assets_data("sprite_S_TANK.bin"))
}

// ---------------------------------------------------------------------------
// Палитра
// ---------------------------------------------------------------------------

#[test]
fn real_color_pal_matches_the_reference_dump() {
    let reference = read_palette_reference();
    let palette = loaded_palette();

    assert_eq!(
        fs::metadata(render_data("COLOR.PAL")).unwrap().len(),
        33_536
    );

    for index in 0..PALETTE_SIZE {
        assert_eq!(
            palette.rgb(index as u8),
            reference.rgb[index],
            "8-битный цвет индекса {index}"
        );
        assert_eq!(
            palette.rgba(index as u8),
            Rgba::from_rgb(reference.rgb[index])
        );
    }

    // 6-битный цвет — ровно четвёртая часть 8-битного.
    for index in 0..PALETTE_SIZE {
        let raw = palette.raw(index as u8);
        let rgb = palette.rgb(index as u8);

        assert_eq!(rgb, [raw[0] * 4, raw[1] * 4, raw[2] * 4], "индекс {index}");
    }
}

#[test]
fn team_luts_match_the_reference_dump() {
    let reference = read_palette_reference();

    for team in TeamColors::ALL {
        let expected = &reference.teams[team.name()];
        let lut = ColorLut::for_team(team);

        for index in 0..PALETTE_SIZE {
            assert_eq!(
                lut.map(index as u8),
                expected[index],
                "{}: слот {index}",
                team.name()
            );
        }
    }

    assert!(ColorLut::identity().is_identity());
}

#[test]
fn brightness_tables_are_built_from_the_real_palette() {
    let palette = loaded_palette();
    let brightness = BrightnessLut::build(&palette);

    // Полная яркость — identity на всех 256 индексах.
    for index in 0..PALETTE_SIZE {
        assert_eq!(
            brightness.map(FULL_BRIGHTNESS_LEVEL, index as u8),
            index as u8
        );
    }

    // Уровень тени меняет палитру: это не identity и не чёрный экран.
    let shadow = brightness.level(SHADOW_BRIGHTNESS_LEVEL);
    let changed = (0..PALETTE_SIZE)
        .filter(|&index| shadow[index] != index as u8)
        .count();

    assert!(changed > 100, "тень меняет {changed} из 256 цветов");
    assert_eq!(brightness.map(SHADOW_BRIGHTNESS_LEVEL, 31), 31);
}

// ---------------------------------------------------------------------------
// Спрайты
// ---------------------------------------------------------------------------

#[test]
fn real_tank_frame_renders_with_palette_and_command_colours() {
    let reference = read_palette_reference();
    let palette = loaded_palette();
    let brightness = BrightnessLut::build(&palette);
    let sheet = decode_sprite(&tank_bytes()).unwrap();
    let frame = sheet.frame(0).unwrap();

    let red = SpriteRenderer::new(
        &palette,
        &brightness,
        ColorLut::for_team(TeamColors::Red),
        FULL_BRIGHTNESS_LEVEL,
    );
    let green = SpriteRenderer::new(
        &palette,
        &brightness,
        ColorLut::for_team(TeamColors::Green),
        FULL_BRIGHTNESS_LEVEL,
    );

    let red_buffer = red.render_frame(frame);
    let green_buffer = green.render_frame(frame);

    // Размер буфера кадра = размер кадра (44×45), прозрачный фон.
    assert_eq!((red_buffer.width(), red_buffer.height()), (44, 45));
    assert_eq!(red_buffer.get(0, 0), Some(Rgba::TRANSPARENT));

    // Строка 0 §3.1: x = 4 -> индекс 0xCF (207), x = 8 -> 0xF0 (240).
    assert_eq!(
        red_buffer.get(4, 0),
        Some(Rgba::from_rgb(reference.rgb[0xCF]))
    );
    assert_eq!(
        red_buffer.get(8, 0),
        Some(Rgba::from_rgb(reference.rgb[0xF0]))
    );

    // Строка 8 дампа: x = 10 -> индекс 0x22 (34), это командный слот 32–39.
    // Красная команда перекрашивает его в 58, зелёная оставляет 34.
    assert_eq!(
        red_buffer.get(10, 8),
        Some(Rgba::from_rgb(reference.rgb[58]))
    );
    assert_eq!(
        green_buffer.get(10, 8),
        Some(Rgba::from_rgb(reference.rgb[34]))
    );
    assert_ne!(red_buffer.get(10, 8), green_buffer.get(10, 8));

    // Непрозрачных пикселей ровно столько, сколько в кадре.
    let opaque = frame.opaque_pixels();

    assert_eq!(
        red_buffer
            .pixels()
            .chunks_exact(4)
            .filter(|pixel| pixel[3] == 255)
            .count(),
        opaque
    );
}

#[test]
fn real_tank_frame_is_positioned_by_its_hot_spot() {
    let palette = loaded_palette();
    let brightness = BrightnessLut::build(&palette);
    let renderer = SpriteRenderer::neutral(&palette, &brightness);
    let sheet = decode_sprite(&tank_bytes()).unwrap();
    let frame = sheet.frame(0).unwrap();

    let mut canvas = RgbaBuffer::new(200, 200);
    let anchor = (100, 100);
    let stats = renderer.draw_frame(&mut canvas, frame, anchor);

    assert_eq!(stats.drawn, frame.opaque_pixels() as u32);
    assert_eq!(stats.clipped, 0);

    // Горячая точка (20,23): кадр начинается в (80,77).
    let (ulx, uly) = (100 - 20, 100 - 23);

    assert_eq!(
        canvas.get(ulx + 4, uly),
        Some(Rgba::from_rgb(palette.rgb(0xCF)))
    );
    assert_eq!(canvas.get(ulx + 3, uly), Some(Rgba::TRANSPARENT));
    assert_eq!(renderer.frame_rect(frame, anchor).ulx, ulx);
    assert_eq!(renderer.frame_rect(frame, anchor).uly, uly);
}

#[test]
fn real_shadow_darkens_the_map_under_the_unit() {
    let palette = loaded_palette();
    let brightness = BrightnessLut::build(&palette);
    let renderer = SpriteRenderer::neutral(&palette, &brightness);
    let shadow = decode_shadow(&s_tank_bytes()).unwrap();
    let frame = shadow.frame(0).unwrap();

    assert_eq!(frame.darkened_pixels(), 2365);

    let base = Rgba::rgb(86, 124, 58);
    let mut canvas = RgbaBuffer::filled(64, 64, base);
    let stats = renderer.draw_shadow(&mut canvas, frame, (32, 32));

    assert_eq!(stats.drawn, frame.darkened_pixels() as u32);
    assert_eq!(stats.clipped, 0);

    // Горячая точка (20,23): кадр тени начинается в (12,9).
    // Строка 0: {4,5} -> x = 16..=20 затемнены, x = 21 — нет.
    let ulx = 32 - 20;
    let uly = 32 - 23;

    for local_x in 4..=8 {
        assert_eq!(
            canvas.get(ulx + local_x, uly),
            Some(base.darken(SHADOW_BRIGHTNESS_LEVEL))
        );
    }

    assert_eq!(canvas.get(15, uly), Some(base));
    assert_eq!(canvas.get(21, uly), Some(base));
    assert_eq!(base.darken(5), Rgba::rgb(61, 88, 41));
}

// ---------------------------------------------------------------------------
// Карта
// ---------------------------------------------------------------------------

/// Карта с разными поверхностями и ресурсами в известных клетках.
fn sample_map() -> Map {
    let mut surface = SurfaceMap::new();
    let mut resources = ResourceMap::new();

    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            surface.set(x, y, Surface::LAND);
        }
    }

    surface.set(1, 1, Surface::WATER);
    surface.set(2, 1, Surface::COAST);
    surface.set(3, 1, Surface::AIR);
    resources.set(1, 1, ResourceMapEntry::new(9, Some(ResourceKind::Raw), 0));
    resources.set(2, 1, ResourceMapEntry::new(3, Some(ResourceKind::Fuel), 0));
    resources.set(3, 1, ResourceMapEntry::new(1, Some(ResourceKind::Gold), 0));

    Map::from_parts(surface, resources)
}

#[test]
fn map_tiles_show_surface_and_resource() {
    let style = TileStyle::default();
    let map = sample_map();

    let water = render_tile(&map, (1, 1), TILE_PIXELS, &style).unwrap();

    assert_eq!(water.get(0, 0), Some(style.water));
    assert_eq!(water.get(32, 32), Some(style.materials));

    let coast = render_tile(&map, (2, 1), TILE_PIXELS, &style).unwrap();

    assert_eq!(coast.get(0, 0), Some(style.coast));
    assert_eq!(coast.get(32, 32), Some(style.fuel));

    let air = render_tile(&map, (3, 1), TILE_PIXELS, &style).unwrap();

    assert_eq!(air.get(0, 0), Some(style.air));
    assert_eq!(air.get(32, 32), Some(style.gold));

    // Клетка без ресурса: только поверхность и паттерн.
    let plain = render_tile(&map, (4, 1), TILE_PIXELS, &style).unwrap();

    assert_eq!(plain.get(0, 0), Some(style.land));
    // (32, 32) — ячейка (4, 4): чётная, значит база без затемнения.
    assert_eq!(plain.get(32, 32), Some(style.land));
}

#[test]
fn map_view_fills_the_whole_screen_with_visible_tiles() {
    let style = TileStyle::default();
    let map = sample_map();
    let viewport = Viewport::new(512, 384);
    let mut screen = RgbaBuffer::filled(512, 384, style.unknown);
    let range = viewport.visible_tiles().unwrap();

    let painted = render_map_into(&mut screen, &map, &viewport, &style);

    assert_eq!(painted, range.count() as u32);
    // Клетка (52,53) рисуется в (0,0): там суша (ресурсы только в (1..=3,1)).
    assert_eq!(screen.get(0, 0), Some(style.land));
    // Углы экрана тоже покрыты картой — фон «неизвестно» не виден.
    for corner in [(0, 0), (511, 0), (0, 383), (511, 383)] {
        assert_ne!(
            screen.get(corner.0, corner.1),
            Some(style.unknown),
            "угол {:?} не закрашен",
            corner
        );
    }
}

#[test]
fn tile_rendering_clips_and_reports_area() {
    let style = TileStyle::default();
    let map = sample_map();
    let mut buffer = RgbaBuffer::new(32, 32);

    // Клетка 64×64 в буфер 32×32: половина пикселей вне буфера.
    let rect = max_render::buffer::PixelRect::new(0, 0, TILE_PIXELS, TILE_PIXELS);
    let painted = render_tile_into(&mut buffer, &map, (10, 10), rect, &style);

    assert_eq!(painted, 32 * 32);
    assert_eq!(buffer.get(0, 0), Some(style.land));
}

#[test]
fn wrl_map_renders_through_the_same_pipeline() {
    // CRATER_3.WRL — эталонная карта из фикстур max_assets (tile_count = 154).
    let bytes = read(assets_data("CRATER_3.WRL"));
    let wrl = WrlMap::parse(&bytes).unwrap();
    let map = map_from_wrl(&wrl);
    let style = TileStyle::default();

    assert_eq!(
        (map.size.width, map.size.height),
        (wrl.width(), wrl.height())
    );
    assert_eq!(map.surface.tiles(), wrl.surface_map().tiles());

    // На карте есть суша: находим клетку и рисуем её тайл.
    let (grid_x, grid_y) = (0..map.size.height)
        .flat_map(|y| (0..map.size.width).map(move |x| (x, y)))
        .find(|&(x, y)| map.surface_at(x, y).contains(Surface::LAND))
        .expect("CRATER_3 содержит сушу");

    let tile = render_tile(&map, (grid_x, grid_y), TILE_PIXELS, &style).unwrap();

    // Ресурсов в WRL-карте нет, поэтому (0,0) клетки — чистый цвет суши
    // (ячейка (0,0) паттерна всегда без затемнения).
    assert_eq!(tile.get(0, 0), Some(style.land));
    assert_eq!(tile.get(32, 32), Some(style.land));
}
