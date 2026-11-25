use super::*;
use max_core::map::{ResourceMap, ResourceMapEntry, SurfaceMap};

/// A map filled entirely with `surface`.
fn uniform_map(surface: Surface) -> Map {
    let mut surface_map = SurfaceMap::new();

    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            surface_map.set(x, y, surface);
        }
    }

    Map::from_parts(surface_map, ResourceMap::new())
}

fn style() -> TileStyle {
    TileStyle::default()
}

#[test]
fn surface_colors_are_distinct() {
    let style = style();
    let colors = [
        style.base_color(Surface::LAND),
        style.base_color(Surface::WATER),
        style.base_color(Surface::COAST),
        style.base_color(Surface::AIR),
        style.base_color(Surface::NONE),
    ];

    for (index, color) in colors.iter().enumerate() {
        for other in colors.iter().skip(index + 1) {
            assert_ne!(color, other, "цвета поверхностей должны различаться");
        }
    }

    // Priority for a mixed mask: WATER is more important than COAST, COAST than LAND.
    assert_eq!(
        style.base_color(Surface::LAND | Surface::COAST),
        style.coast
    );
    assert_eq!(
        style.base_color(Surface::LAND | Surface::WATER),
        style.water
    );
    assert_eq!(style.base_color(Surface::AIR | Surface::LAND), style.land);
}

#[test]
fn resource_colors_are_distinct() {
    let style = style();

    assert_ne!(
        style.resource_color(ResourceKind::Raw),
        style.resource_color(ResourceKind::Fuel)
    );
    assert_ne!(
        style.resource_color(ResourceKind::Raw),
        style.resource_color(ResourceKind::Gold)
    );
    assert_ne!(
        style.resource_color(ResourceKind::Fuel),
        style.resource_color(ResourceKind::Gold)
    );
}

#[test]
fn tile_base_and_pattern_alternate_cells() {
    let style = style();
    let map = uniform_map(Surface::LAND);
    let buffer = render_tile(&map, (10, 10), TILE_PIXELS, &style).unwrap();
    let land = style.land;

    assert_eq!(buffer.width(), 64);
    assert_eq!(buffer.height(), 64);
    // Pattern cell is 8 pixels: (0,0) and (8,0) have different parity.
    assert_eq!(buffer.get(0, 0), Some(land));
    assert_eq!(buffer.get(7, 0), Some(land));
    assert_eq!(buffer.get(8, 0), Some(land.darken(style.pattern_level)));
    assert_eq!(buffer.get(16, 0), Some(land));
    assert_eq!(buffer.get(8, 8), Some(land));
    // (63, 63) — cell (7, 7): even, so the base without darkening.
    assert_eq!(buffer.get(63, 63), Some(land));
    // (0, 8) — cell (0, 1): odd, darkened.
    assert_eq!(buffer.get(0, 8), Some(land.darken(style.pattern_level)));
}

#[test]
fn different_surfaces_give_different_tiles() {
    let style = style();
    let land = render_tile(&uniform_map(Surface::LAND), (1, 1), 16, &style).unwrap();
    let water = render_tile(&uniform_map(Surface::WATER), (1, 1), 16, &style).unwrap();
    let coast = render_tile(&uniform_map(Surface::COAST), (1, 1), 16, &style).unwrap();
    let air = render_tile(&uniform_map(Surface::AIR), (1, 1), 16, &style).unwrap();

    assert_eq!(land.get(0, 0), Some(style.land));
    assert_eq!(water.get(0, 0), Some(style.water));
    assert_eq!(coast.get(0, 0), Some(style.coast));
    assert_eq!(air.get(0, 0), Some(style.air));
}

#[test]
fn resource_markers_are_drawn_from_max_res() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAX.RES");

    if !path.is_file() {
        return;
    }

    let archive = max_assets::ResArchive::open(&path).expect("MAX.RES");
    let markers = max_assets::ResourceMarkers::load(&archive);
    assert!(markers.is_ready());

    let mut resources = ResourceMap::new();
    resources.set(
        52,
        53,
        // `HASH_TEAM_RED = 0x2000` (bit 13) — surveyed by the reds.
        ResourceMapEntry::new(4, Some(ResourceKind::Gold), 0b1000),
    );
    let map = Map::from_parts(uniform_map(Surface::LAND).surface.clone(), resources);
    let mut palette = [[0u8, 0, 0, 255]; 256];

    for (index, entry) in palette.iter_mut().enumerate() {
        *entry = [index as u8, 255 - index as u8, 128, 255];
    }

    let viewport = Viewport::new(448, 448);
    let mut buffer = RgbaBuffer::new(448, 448);
    let (painted, cells) =
        render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 0, false);

    assert!(painted > 0, "маркер золота должен нарисоваться");
    assert_eq!(cells, 1, "одна разведанная клетка — одна иконка");
    assert_eq!(
        render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 1, false),
        (0, 0),
            "клетка не разведана другой командой"
    );
    // `GameManager_MaxSurvey` — the cheat draws the marker on **every** land
    // cell of the view (`Survey_RenderMarkers`: `mask = 0xFFFF`).
    assert!(
        render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 1, true)
            .1
            > 1,
            "чит MAXSURVEY (max_survey) показывает маркеры всех клеток"
    );
}

/// `Survey_RenderMarker` (`survey.cpp:32-82`) — the surveyed-but-empty cell:
/// `min(CargoMap & CARGO_MASK, 16) == 0` still gets `RAWMSK0` (the "already
/// explored" circle) at `zoom > 16`, and nothing at `zoom <= 16`.
#[test]
fn a_surveyed_empty_cell_gets_the_circle_marker() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAX.RES");

    if !path.is_file() {
        return;
    }

    let archive = max_assets::ResArchive::open(&path).expect("MAX.RES");
    let markers = max_assets::ResourceMarkers::load(&archive);

    let mut resources = ResourceMap::new();
    // Surveyed by RED, but no deposit: `CargoMap` has only the team bit.
    resources.set(52, 53, ResourceMapEntry::new(0, None, 0b1000));
    let map = Map::from_parts(uniform_map(Surface::LAND).surface, resources);
    let mut palette = [[0u8, 0, 0, 255]; 256];

    for (index, entry) in palette.iter_mut().enumerate() {
        *entry = [index as u8, 255 - index as u8, 128, 255];
    }

    // Native zoom (64 > 16): the big `RAWMSK0` circle — 38×38 pixels.
    let viewport = Viewport::new(448, 448);
    let mut buffer = RgbaBuffer::new(448, 448);
    let (painted, cells) =
        render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 0, false);

    assert!(painted > 0, "кружок разведанной пустой клетки нарисован");
    assert_eq!(cells, 1, "ровно одна разведанная клетка в кадре");
    assert_eq!(
        render_resource_icons_into(&mut buffer, &map, &viewport, &markers, &palette, 1, false),
        (0, 0),
            "чужая разведка кружок не рисует"
    );

    // Zoom 16: C++ returns early for `resource_value == 0` (`survey.cpp:62-64`).
    let small = Viewport::with_center(448, 448, (52 * 64 + 32, 53 * 64 + 32), 16);
    assert_eq!(
        render_resource_icons_into(&mut buffer, &map, &small, &markers, &palette, 0, false),
        (0, 0),
            "на малом масштабе пустая клетка маркера не получает"
    );
}

#[test]
fn resource_dot_is_painted_at_the_centre() {
    let style = style();
    let map = uniform_map(Surface::LAND);
    let mut resources = ResourceMap::new();

    resources.set(
        10,
        10,
        ResourceMapEntry::new(5, Some(ResourceKind::Gold), 0),
    );

    let map = Map::from_parts(map.surface.clone(), resources);
    let buffer = render_tile(&map, (10, 10), TILE_PIXELS, &style).unwrap();

    // Center of the cell is gold, the corner is land (the checkerboard does not matter).
    assert_eq!(buffer.get(32, 32), Some(style.gold));
    assert_eq!(buffer.get(0, 0), Some(style.land));

    // A zero resource amount draws no dot.
    let mut empty = ResourceMap::new();
    empty.set(
        10,
        10,
        ResourceMapEntry::new(0, Some(ResourceKind::Gold), 0),
    );

    let map = Map::from_parts(uniform_map(Surface::LAND).surface, empty);
    let buffer = render_tile(&map, (10, 10), TILE_PIXELS, &style).unwrap();

    assert_eq!(buffer.get(32, 32), Some(style.land));
}

#[test]
fn resource_kinds_paint_different_dots() {
    let style = style();
    let mut surface_map = SurfaceMap::new();

    surface_map.set(1, 1, Surface::LAND);
    surface_map.set(2, 1, Surface::LAND);
    surface_map.set(3, 1, Surface::LAND);

    let mut resources = ResourceMap::new();

    resources.set(1, 1, ResourceMapEntry::new(4, Some(ResourceKind::Raw), 0));
    resources.set(2, 1, ResourceMapEntry::new(4, Some(ResourceKind::Fuel), 0));
    resources.set(3, 1, ResourceMapEntry::new(4, Some(ResourceKind::Gold), 0));

    let map = Map::from_parts(surface_map, resources);
    let raw = render_tile(&map, (1, 1), TILE_PIXELS, &style).unwrap();
    let fuel = render_tile(&map, (2, 1), TILE_PIXELS, &style).unwrap();
    let gold = render_tile(&map, (3, 1), TILE_PIXELS, &style).unwrap();

    assert_eq!(raw.get(32, 32), Some(style.materials));
    assert_eq!(fuel.get(32, 32), Some(style.fuel));
    assert_eq!(gold.get(32, 32), Some(style.gold));
}

#[test]
fn out_of_map_tiles_are_not_drawn() {
    let style = style();
    let map = uniform_map(Surface::LAND);
    let mut buffer = RgbaBuffer::new(32, 32);

    assert_eq!(render_tile(&map, (-1, 0), 32, &style), None);
    assert_eq!(
        render_tile_into(
            &mut buffer,
            &map,
            (-1, 0),
            PixelRect::new(0, 0, 32, 32),
            &style
        ),
        0
    );
    assert_eq!(
        render_tile_into(
            &mut buffer,
            &map,
            (max_core::geo::MAP_WIDTH, 0),
            PixelRect::new(0, 0, 32, 32),
            &style
        ),
        0
    );
}

#[test]
fn render_map_into_paints_exactly_the_visible_tiles() {
    let style = style();
    let map = uniform_map(Surface::WATER);
    let viewport = Viewport::new(512, 384);
    let mut buffer = RgbaBuffer::filled(512, 384, style.unknown);
    let range = viewport.visible_tiles().unwrap();

    let painted = render_map_into(&mut buffer, &map, &viewport, &style);

    assert_eq!(painted, range.count() as u32);
    // Cell (52,53) starts at (0,0) of the screen.
    assert_eq!(buffer.get(0, 0), Some(style.water));
    assert_eq!(
        buffer.get(63, 0),
        Some(style.water.darken(style.pattern_level))
    );
    // The bottom-right corner of the screen is also covered by the map.
    assert_ne!(buffer.get(511, 383), Some(style.unknown));
}
