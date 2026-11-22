//! Landing point selection phase — a port of `GameManager_SelectSite`
//! (`game_manager.cpp:1324`, state `GAME_STATE_7_SITE_SELECT`).
//!
//! # What the original does
//!
//! After mobilization (if the team had credits) C++ opens the map in
//! the `GAME_STATE_7_SITE_SELECT` state at zoom 4: the whole 112×112 cell map
//! occupies the `448×448` map area (4 px per cell). The player moves the cursor and
//! confirms the point; `GameManager_IsValidStartingPosition` requires that
//! the 4×4 cells around it (`x-1 … x+2`) be **exactly land**. Esc cancels the start
//! (`GAME_STATE_14_EXIT_SITE_SELECT`).
//!
//! # What is drawn on the map (C++)
//!
//! * `GameManager_RenderMap` (`game_manager.cpp:1096`) in
//!   `GAME_STATE_7_SITE_SELECT`: `DrawMap_RenderMapTiles` → real `.WRL` tiles
//!   via `Gfx_DecodeMapTile` (`gfx.cpp:125`) — the 64×64 pixels of a tile
//!   are reduced to 4×4 screen pixels (zoom 4). The grid is off by default
//!   (`GameManager_DisplayButtonGrid = false`), so there are no cell lines.
//! * `DrawMap_RenderUnits` draws only **visible** units
//!   (`UnitInfoGroup::IsRelevant`: `IsVisibleToTeam(PlayerTeam)`). At this
//!   phase there are no armies yet: they land after all points are chosen
//!   (`GameManager_ProcessTeamMissionSupplyUnits`), and the temporary `MASTER`
//!   is spawned only in the proximity check branch — not in the interactive loop.
//! * `GameManager_DrawUnitSelector`/`GameManager_RenderScanRangeIndicators`/
//!   `DrawMap_RenderBuildMarker` **are not drawn in normal selection**: `MASTER`
//!   is spawned only in the `GameManager_SelectSite` branch for the proximity
//!   check (`proximity_alert_ack` 1/2/3), and the interactive loop
//!   `GameManager_ProgressTurn` runs before it. Therefore `MASTER` has neither
//!   a 3×3 marker, nor a yellow scan circle, nor `BLDMRK*`; the point is selected
//!   by the mouse cursor only. (The oracle frame `build/tmp/oracle-MAX0002.png` of the C++ port
//!   confirms this: only the green `CURSOR_UNIT_GO` is on the map.)
//! * The message — `MessageManager_DrawMessage` (`_(dfe1)` =
//!   "Select starting location."): a window in the top-left corner of the map area
//!   (`+0,+10`), the background is a darkening of the map pixels by palette
//!   (`ResourceManager_DarkeningColorIndexTable`), one `Font5` line with an outline.
//! * The validity of the point is conveyed **by the mouse cursor**
//!   (`GameManager_GetWindowCursor`): `CURSOR_UNIT_GO` over a valid cell,
//!   `CURSOR_UNIT_NO_GO` over an invalid one, `CURSOR_HAND` outside the map. There is no separate
//!   green/red frame in the original.
//!
//! # What is ported
//!
//! * Real `.WRL` tiles ([`max_render::terrain::TerrainAtlas`]) with the exact
//!   `Gfx_DecodeMapTile` sampling ([`TerrainAtlas::render_tile_cpp_into`]).
//! * The C++ message window (one line, palette darkening).
//! * The cursor `CURSOR_UNIT_GO`/`NO_GO`/`HAND` — both in the SDL layer (`main.rs`) and in
//!   the frame (for headless QA shots, like `Cursor_RenderToBuffer` in
//!   `screendump.cpp`).
//! * The point check — [`max_core::setup::is_valid_starting_position`]
//!   (`GameManager_IsValidStartingPosition`): 4×4 around the selected cell.
//! * `Enter`/`Done` — confirm, `Esc`/`Cancel` — cancel.
//! * The arrows move the point by one cell.
//!
//! # Differences from C++ (deliberate)
//!
//! * A click on a valid cell immediately confirms the choice (like C++), but in the QA
//!   driver `landing grid` moves the point without a click, to take a shot.
//! * The `Cancel`/`Done` buttons of the bottom row are removed — they are not in C++.
//! * `GameManager_HandleProximityOverlaps`/`GameManager_CheckLandingZones` are not
//!   ported (together with them the `MASTER` marker, the scan circle and
//!   `BLDMRK*` are not needed).
//! * If there is no `.WRL` data (headless tests), the map is drawn in a simplified way
//!   with [`TileStyle`] colors — the only remaining fallback.

use max_assets::CursorSet;
use max_core::enums::Surface;
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::map::SurfaceMap;
use max_core::setup::{find_valid_starting_position, is_valid_starting_position};
use max_core::team::TeamId;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout::{MAIN_MAP, WINDOW_HEIGHT, WINDOW_WIDTH};
use max_render::map_render::TileStyle;
use max_render::terrain::TerrainAtlas;
use max_render::text::{draw_text_gnw, draw_text_gnw_shadowed, GnwFontId};

use crate::menu::MenuAssets;

/// Cell size on the point-selection screen — zoom 4 of the original
/// (`GameManager_UpdateMainMapView(MAP_VIEW_ZOOM, 4, …)`).
pub const LANDING_CELL: i32 = 4;

/// Map area on the screen: 112 × 4 = 448 — exactly `MAIN_MAP`.
pub const LANDING_AREA: PixelRect = MAIN_MAP;

/// Vertical offset of the message window from the top of the map area (`offset_y = 10`
/// in `MessageManager_DrawMessage`).
pub const MESSAGE_TOP_OFFSET: i32 = 10;
/// Horizontal offset of the text in the message window (10).
pub const MESSAGE_TEXT_LEFT: i32 = 10;
/// Vertical offset of the text in the message window (10).
pub const MESSAGE_TEXT_TOP: i32 = 10;
/// Height of the `Text_GetHeight()` line for `Font5` (10).
pub const MESSAGE_LINE_HEIGHT: i32 = 10;
/// Initial height of the message window (`MessageManager_MessageBox_Height = 20`).
pub const MESSAGE_BASE_HEIGHT: i32 = 20;
/// Text color of the message window (`GNW_TEXT_OUTLINE | 0xFF`).
pub const MESSAGE_TEXT_COLOR: u8 = 0xFF;

/// The C++ message text (`_(dfe1)` in `GameManager_SelectSite`).
pub const MESSAGE_TEXT: &str = "Select starting location.";

/// Result of input on the landing screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingAction {
    /// Nothing happened.
    None,
    /// The point was chosen and confirmed.
    Confirm,
    /// The player cancelled the choice.
    Cancel,
}

/// Landing point selection screen of one team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandingScreen {
    /// Whose landing.
    pub team: TeamId,
    /// Planet `0..=23` (for the caption).
    pub world: i64,
    /// Map surface.
    pub surface: SurfaceMap,
    /// Real `.WRL` tiles (if the planet data is loaded).
    pub terrain: Option<TerrainAtlas>,
    /// Current point under the cursor (`starting_position`).
    pub cursor: Point,
    /// Status line.
    pub status: String,
}

impl LandingScreen {
    /// Screen with the cursor at the team's starting point (`START_POSITIONS`).
    ///
    /// If the point is invalid, [`find_valid_starting_position`] moves it in
    /// a spiral — exactly as C++ does when initializing a mission.
    pub fn new(team: TeamId, world: i64, surface: SurfaceMap) -> Self {
        Self::new_with_terrain(team, world, surface, None)
    }

    /// The same, but with a ready `.WRL` tile atlas for real rendering.
    pub fn new_with_terrain(
        team: TeamId,
        world: i64,
        surface: SurfaceMap,
        terrain: Option<TerrainAtlas>,
    ) -> Self {
        let mut cursor = crate::local::START_POSITIONS[usize::from(team.0) % 4];
        find_valid_starting_position(&surface, &mut cursor);

        Self {
            team,
            world,
            surface,
            terrain,
            cursor,
            status: String::new(),
        }
    }

    /// Path to the planet `.WRL` (`None` — no name/file).
    fn world_path(world: i64, world_dir: &std::path::Path) -> Option<std::path::PathBuf> {
        let index = usize::try_from(world.clamp(0, 23)).ok()?;
        let name = crate::assets::WORLD_ASSETS.get(index)?;
        let path = world_dir.join(format!("{name}.WRL"));

        path.is_file().then_some(path)
    }

    /// Loads the planet surface from `world_dir` (`*.WRL`).
    ///
    /// `None` — no planet data; the UI then skips the point-selection phase and
    /// keeps the standard starting position.
    pub fn load_surface(world: i64, world_dir: &std::path::Path) -> Option<SurfaceMap> {
        let path = Self::world_path(world, world_dir)?;
        let map = max_assets::wrl::WrlMap::open(path).ok()?;

        Some(map.surface_map())
    }

    /// Loads the real planet tiles (`WRL` → [`TerrainAtlas`]).
    pub fn load_terrain(world: i64, world_dir: &std::path::Path) -> Option<TerrainAtlas> {
        let path = Self::world_path(world, world_dir)?;
        let map = max_assets::wrl::WrlMap::open(path).ok()?;

        Some(TerrainAtlas::from_wrl(&map))
    }

    /// Loads the surface and real tiles from one `*.WRL` parse.
    pub fn load_parts(
        world: i64,
        world_dir: &std::path::Path,
    ) -> Option<(SurfaceMap, TerrainAtlas)> {
        let path = Self::world_path(world, world_dir)?;
        let map = max_assets::wrl::WrlMap::open(path).ok()?;

        Some((map.surface_map(), TerrainAtlas::from_wrl(&map)))
    }

    /// Loads the surface and tiles from one `*.WRL` parse and creates the screen.
    ///
    /// `None` — no planet data; the UI then shows an honest error.
    pub fn load(team: TeamId, world: i64, world_dir: &std::path::Path) -> Option<Self> {
        let (surface, terrain) = Self::load_parts(world, world_dir)?;

        Some(Self::new_with_terrain(team, world, surface, Some(terrain)))
    }

    /// Whether the current point is valid (`GameManager_IsValidStartingPosition`).
    pub fn cursor_valid(&self) -> bool {
        is_valid_starting_position(
            &self.surface,
            i32::from(self.cursor.x),
            i32::from(self.cursor.y),
        )
    }

    /// Whether an arbitrary cell is valid.
    pub fn is_valid_at(&self, grid: Point) -> bool {
        is_valid_starting_position(&self.surface, i32::from(grid.x), i32::from(grid.y))
    }

    /// Moves the cursor (clamped to the map).
    pub fn step(&mut self, dx: i32, dy: i32) {
        self.cursor.x = (i32::from(self.cursor.x) + dx).clamp(1, i32::from(MAP_WIDTH) - 4) as i16;
        self.cursor.y = (i32::from(self.cursor.y) + dy).clamp(1, i32::from(MAP_HEIGHT) - 4) as i16;
        self.status.clear();
    }

    /// Converts a frame point into a map cell ([`LANDING_AREA`] area).
    pub fn screen_to_grid(x: i32, y: i32) -> Option<Point> {
        if !LANDING_AREA.contains(x, y) {
            return None;
        }

        Some(Point::new(
            ((x - LANDING_AREA.ulx) / LANDING_CELL) as i16,
            ((y - LANDING_AREA.uly) / LANDING_CELL) as i16,
        ))
    }

    /// Handling of a click on the map.
    ///
    /// Like C++ (`game_manager.cpp:6718-6727`): a click on a valid cell immediately
    /// confirms the choice (`CURSOR_UNIT_GO`), a click on an invalid one only
    /// moves the point and writes the status (`CURSOR_UNIT_NO_GO`). Clicks outside the map
    /// area are ignored.
    pub fn click(&mut self, x: i32, y: i32) -> LandingAction {
        let Some(point) = Self::screen_to_grid(x, y) else {
            return LandingAction::None;
        };

        self.cursor = point;

        if self.cursor_valid() {
            self.status = format!("Landing site: {},{}", self.cursor.x + 1, self.cursor.y + 1);
            LandingAction::Confirm
        } else {
            self.status = "Invalid landing site: need 4x4 land".to_string();
            LandingAction::None
        }
    }

    /// Confirmation of the current point (`Enter`/`Done`).
    pub fn confirm(&mut self) -> LandingAction {
        if self.cursor_valid() {
            self.status = format!("Landing site: {},{}", self.cursor.x + 1, self.cursor.y + 1);
            LandingAction::Confirm
        } else {
            self.status = "Invalid landing site: need 4x4 land".to_string();
            LandingAction::None
        }
    }

    /// Frame drawing: `.WRL` tiles, C++ markers, message window.
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        menu: &MenuAssets,
        cursors: Option<&CursorSet>,
        chrome: Option<&crate::chrome::Chrome>,
        status: &str,
        mouse: Option<(i32, i32)>,
    ) -> u32 {
        // System window palette: 64…159 come from `.WRL`
        // (`World::Load`), the rest — from the menu palette (`MAINPIC`).
        let palette = match &self.terrain {
            Some(terrain) if !terrain.is_empty() => {
                menu.palette.with_wrl_palette(terrain.palette())
            }
            _ => menu.palette,
        };
        let palette = &palette;
        let mut painted = 0;

        frame.fill_rect(
            PixelRect::new(0, 0, WINDOW_WIDTH, WINDOW_HEIGHT),
            Rgba::rgb(12, 12, 14),
        );

        if let Some(chrome) = chrome.filter(|chrome| chrome.is_ready()) {
            painted += chrome.draw(frame);
        }

        painted += self.draw_map(frame);

        // `MessageManager_DrawMessage` is the only element over the map in
        // normal point selection (MASTER/markers appear only in the proximity
        // check branch, which we did not port).
        painted += self.draw_message(frame, palette);

        // C++ cursor (`Cursor_RenderToBuffer` in `screendump.cpp`): in the live UI it
        // is set by the SDL layer, and it gets into the frame for headless QA shots.
        if let (Some(cursors), Some((x, y))) = (cursors, mouse) {
            painted += self.draw_cursor(frame, cursors, palette, x, y);
        }

        // Status line in the bottom panel (our chrome; in C++ — coordinates/log).
        let team = crate::hotseat::team_display_name(self.team);
        let line = if status.is_empty() {
            self.status.as_str()
        } else {
            status
        };
        let text = if line.is_empty() {
            format!("{team}: select landing site")
        } else {
            line.chars().take(26).collect()
        };

        draw_text_gnw(
            frame,
            11,
            424,
            GnwFontId::Font5.font(),
            &text,
            palette.rgba(crate::setup::GREEN),
        );

        painted
    }

    /// Map: real `.WRL` tiles or (without data) `TileStyle`.
    fn draw_map(&self, frame: &mut RgbaBuffer) -> u32 {
        match &self.terrain {
            Some(terrain) if !terrain.is_empty() => draw_terrain(frame, terrain),
            _ => self.draw_flat_map(frame),
        }
    }

    /// Simplified map with surface colors — only without `.WRL` data.
    fn draw_flat_map(&self, frame: &mut RgbaBuffer) -> u32 {
        let style = TileStyle::default();
        let mut painted = 0;

        for grid_y in 0..MAP_HEIGHT {
            for grid_x in 0..MAP_WIDTH {
                let surface = self.surface.get(grid_x, grid_y);
                let color = if surface == Surface::NONE {
                    style.unknown
                } else {
                    style.base_color(surface)
                };

                painted += frame.fill_rect(
                    PixelRect::new(
                        LANDING_AREA.ulx + i32::from(grid_x) * LANDING_CELL,
                        LANDING_AREA.uly + i32::from(grid_y) * LANDING_CELL,
                        LANDING_CELL,
                        LANDING_CELL,
                    ),
                    color,
                );
            }
        }

        painted
    }

    /// `MessageManager_DrawMessage` message window: map darkening by
    /// palette and one line `Select starting location.`
    fn draw_message(&self, frame: &mut RgbaBuffer, palette: &crate::panels::UiPalette) -> u32 {
        let font = GnwFontId::Font5.font();
        let width = font.text_width(MESSAGE_TEXT) + MESSAGE_TEXT_LEFT * 2;
        let height = MESSAGE_BASE_HEIGHT + MESSAGE_LINE_HEIGHT;
        let rect = PixelRect::new(
            LANDING_AREA.ulx,
            LANDING_AREA.uly + MESSAGE_TOP_OFFSET,
            width,
            height,
        );
        let mut painted = 0;

        // `ResourceManager_DarkeningColorIndexTable`: `max(7, c/4)*31/63`
        // and the nearest palette color (`Color_MapColor`) — as in C++, where the map
        // is stored as indices rather than RGB. Cache by source color: in the window area
        // there are only a few dozen palette colors, while `map_color` is a 256-entry scan.
        let mut cache: std::collections::HashMap<[u8; 3], Rgba> = std::collections::HashMap::new();

        for y in rect.uly..rect.lry() {
            for x in rect.ulx..rect.lrx() {
                let Some(color) = frame.get(x, y) else {
                    continue;
                };
                let key = [color.r, color.g, color.b];
                let mapped = match cache.get(&key) {
                    Some(mapped) => *mapped,
                    None => {
                        let dark = color.darken(1);
                        let mapped = palette.map_color([dark.r, dark.g, dark.b]);
                        cache.insert(key, mapped);
                        mapped
                    }
                };

                if frame.set(x, y, mapped) {
                    painted += 1;
                }
            }
        }

        let drawn = draw_text_gnw_shadowed(
            frame,
            rect.ulx + MESSAGE_TEXT_LEFT,
            rect.uly + MESSAGE_TEXT_TOP,
            font,
            MESSAGE_TEXT,
            palette.rgba(MESSAGE_TEXT_COLOR),
            Rgba::BLACK,
            Rgba::BLACK,
        );

        painted + drawn.max(0) as u32
    }

    /// `GameManager_GetWindowCursor` cursor in the frame (`Cursor_RenderToBuffer`).
    fn draw_cursor(
        &self,
        frame: &mut RgbaBuffer,
        cursors: &CursorSet,
        palette: &crate::panels::UiPalette,
        x: i32,
        y: i32,
    ) -> u32 {
        let (id, frame_index) = match Self::screen_to_grid(x, y) {
            Some(grid) if self.is_valid_at(grid) => (crate::cursor::CursorId::UnitGo, 0),
            Some(_) => (crate::cursor::CursorId::UnitNoGo, 0),
            None => (crate::cursor::CursorId::Hand, 0),
        };
        let Some(image) =
            crate::cursor::cursor_image(cursors, &palette.palette_rgba(), id, frame_index)
        else {
            return 0;
        };
        let left = x - image.hot_x;
        let top = y - image.hot_y;
        let mut painted = 0;

        for py in 0..image.height {
            for px in 0..image.width {
                let Some([r, g, b, a]) = image.pixel(px, py) else {
                    continue;
                };

                if a == 0 {
                    continue;
                }

                if frame.set(
                    left + px as i32,
                    top + py as i32,
                    Rgba::from_rgba([r, g, b, a]),
                ) {
                    painted += 1;
                }
            }
        }

        painted
    }
}

/// `.WRL` tiles on the landing map — `Gfx_DecodeMapTile` sampling
/// (zoom 4: 64×64 → 4×4).
pub fn draw_terrain(frame: &mut RgbaBuffer, terrain: &TerrainAtlas) -> u32 {
    let (width, height) = terrain.size();
    let mut painted = 0;

    for grid_y in 0..height {
        for grid_x in 0..width {
            let rect = PixelRect::new(
                LANDING_AREA.ulx + i32::from(grid_x) * LANDING_CELL,
                LANDING_AREA.uly + i32::from(grid_y) * LANDING_CELL,
                LANDING_CELL,
                LANDING_CELL,
            );

            painted += terrain.render_tile_cpp_into(frame, (grid_x, grid_y), rect);
        }
    }

    painted
}

/// Window cursor for a frame point on the landing screen —
/// `GameManager_GetWindowCursor` (`game_manager.cpp:4263`).
///
/// * outside the map area — `HANDPTR`;
/// * a valid cell — `CURSOR_UNIT_GO`;
/// * an invalid one — `CURSOR_UNIT_NO_GO`.
pub fn cursor_for_screen_point(
    landing: &LandingScreen,
    x: i32,
    y: i32,
) -> Option<crate::cursor::CursorId> {
    let grid = LandingScreen::screen_to_grid(x, y)?;

    Some(if landing.is_valid_at(grid) {
        crate::cursor::CursorId::UnitGo
    } else {
        crate::cursor::CursorId::UnitNoGo
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Map: all land except a vertical water strip in columns 50..52.
    fn surface() -> SurfaceMap {
        let mut surface = SurfaceMap::new();

        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                let tile = if (50..53).contains(&x) {
                    Surface::WATER
                } else {
                    Surface::LAND
                };
                *surface
                    .tiles_mut()
                    .get_mut(y as usize * MAP_WIDTH as usize + x as usize)
                    .unwrap() = tile;
            }
        }

        surface
    }

    #[test]
    fn landing_accepts_only_4x4_land() {
        let mut landing = LandingScreen::new(TeamId::RED, 0, surface());

        landing.cursor = Point::new(20, 20);
        assert!(landing.cursor_valid());
        assert_eq!(landing.confirm(), LandingAction::Confirm);

        landing.cursor = Point::new(51, 20);
        assert!(!landing.cursor_valid());
        assert_eq!(landing.confirm(), LandingAction::None);
        assert!(landing.status.contains("Invalid"));
    }

    #[test]
    fn clicks_map_frame_coordinates_to_grid() {
        assert_eq!(
            LandingScreen::screen_to_grid(LANDING_AREA.ulx, LANDING_AREA.uly),
            Some(Point::new(0, 0))
        );
        assert_eq!(
            LandingScreen::screen_to_grid(LANDING_AREA.ulx + 4 * 10, LANDING_AREA.uly + 4 * 20),
            Some(Point::new(10, 20))
        );
        assert_eq!(LandingScreen::screen_to_grid(0, 0), None, "вне карты");
    }

    #[test]
    fn valid_click_confirms_and_invalid_click_only_moves() {
        let mut landing = LandingScreen::new(TeamId::RED, 0, surface());

        let x = LANDING_AREA.ulx + 30 * LANDING_CELL + 2;
        let y = LANDING_AREA.uly + 22 * LANDING_CELL + 2;

        // A valid cell — C++ confirms the choice immediately.
        assert_eq!(landing.click(x, y), LandingAction::Confirm);
        assert_eq!(landing.cursor, Point::new(30, 22));

        // An invalid one — only moves the point.
        let water_x = LANDING_AREA.ulx + 51 * LANDING_CELL + 2;
        assert_eq!(landing.click(water_x, y), LandingAction::None);
        assert_eq!(landing.cursor, Point::new(51, 22));
        assert!(landing.status.contains("Invalid"));

        // Outside the map area — ignore.
        assert_eq!(landing.click(0, 0), LandingAction::None);
        assert_eq!(landing.cursor, Point::new(51, 22));
    }

    #[test]
    fn arrows_move_the_cursor_within_the_map() {
        let mut landing = LandingScreen::new(TeamId::RED, 0, surface());

        landing.cursor = Point::new(5, 5);
        landing.step(1, 0);
        assert_eq!(landing.cursor, Point::new(6, 5));

        landing.step(-100, -100);
        assert_eq!(landing.cursor, Point::new(1, 1), "кламп по краю карты");
    }

    #[test]
    fn default_cursor_is_a_valid_site() {
        let landing = LandingScreen::new(TeamId::BLUE, 0, surface());

        assert!(
            landing.cursor_valid(),
            "find_valid_starting_position обязан дать валидную точку"
        );
    }

    #[test]
    fn cursor_id_matches_the_cpp_validity_check() {
        let landing = LandingScreen::new(TeamId::RED, 0, surface());
        let center = |grid: Point| {
            (
                LANDING_AREA.ulx + i32::from(grid.x) * LANDING_CELL + LANDING_CELL / 2,
                LANDING_AREA.uly + i32::from(grid.y) * LANDING_CELL + LANDING_CELL / 2,
            )
        };

        let (x, y) = center(Point::new(30, 22));
        assert_eq!(
            cursor_for_screen_point(&landing, x, y),
            Some(crate::cursor::CursorId::UnitGo),
            "CURSOR_UNIT_GO над валидной клеткой"
        );

        let (x, y) = center(Point::new(51, 22));
        assert_eq!(
            cursor_for_screen_point(&landing, x, y),
            Some(crate::cursor::CursorId::UnitNoGo),
            "CURSOR_UNIT_NO_GO над невалидной клеткой"
        );

        assert_eq!(
            cursor_for_screen_point(&landing, 0, 0),
            None,
            "вне карты — HANDPTR (SDL-слой)"
        );
    }

    #[test]
    fn message_box_is_one_line_and_darkens_the_map() {
        let landing = LandingScreen::new(TeamId::RED, 0, surface());
        let mut colors = [[0u8, 0, 0]; 256];
        colors[usize::from(MESSAGE_TEXT_COLOR)] = [255, 255, 255];
        colors[1] = [52, 60, 72];
        let palette = crate::panels::UiPalette::from_colors(colors);
        let water = Rgba::rgb(80, 128, 164);
        let mut frame = RgbaBuffer::filled(640, 480, water);

        let painted = landing.draw_message(&mut frame, &palette);

        assert!(painted > 0);
        // Exactly one line high: `20 + 10` (`MessageManager_MessageBox_Height`).
        let rect = PixelRect::new(
            LANDING_AREA.ulx,
            LANDING_AREA.uly + MESSAGE_TOP_OFFSET,
            GnwFontId::Font5.font().text_width(MESSAGE_TEXT) + MESSAGE_TEXT_LEFT * 2,
            MESSAGE_BASE_HEIGHT + MESSAGE_LINE_HEIGHT,
        );
        assert_ne!(
            frame.get(rect.ulx, rect.uly),
            Some(water),
            "фон окна затемнён по палитре"
        );
        assert_eq!(
            frame.get(rect.ulx - 1, rect.uly),
            Some(water),
            "за окном карта не тронута"
        );
    }
}
