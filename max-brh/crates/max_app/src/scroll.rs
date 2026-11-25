//! Map scrolling at the screen edges — a port of C++ `GameManager_ProcessInput`
//! (`game_manager.cpp:6351-6563`) and `GameManager_ScrollMainMapView`
//! (`:1652-1677`).
//!
//! # Oracle
//!
//! * `window_manager.cpp:90-101` — twelve `WINDOW_SCROLL_*` windows (8 px at
//!   the frame edges, 50×50 in the corners). `GameManager_ProcessInput` picks the **first**
//!   window under the mouse in the order of the `windows[]` table, so the rectangles
//!   are checked in the same order (corners before sides, `End Turn`/`Files`/
//!   `Preferences` — before scrolling, they are higher in the table);
//! * `GameManager_SetGridOffset` (`:6351`) — with `click_scroll=1` the offset is
//!   accepted only while the LMB is held (or multi-selection
//!   `RenderState == 2` is in progress); with `click_scroll=0` — on each hover;
//! * `GameManager_ScrollMainMapView` (`:1652`) — accumulation of
//!   `ScrollRate += GridStepLevel * ScrollRateLimit`, the step is the integer part;
//!   `GridStepLevel = (MapScalingFactor * quick_scroll) / GFX_SCALE_DENOMINATOR`
//!   (`:1711-1718`), which in **screen** pixels is exactly `quick_scroll`
//!   (the world↔screen conversion cancels out), so here the step is `quick_scroll` px;
//! * `GameManager_UpdateScrollRateLimit` (`:1636`) — a limit of `1.5` frames in
//!   case of a hang.
//!
//! # Input difference
//!
//! The window layer of the Rust client (`max_render::sdl_window::poll_input`) provides
//! only `MouseButtonDown`, without `MouseButtonUp`. Therefore "LMB held" is
//! modeled by a [`ScrollState::press`] pulse of length
//! [`CLICK_HOLD_SECONDS`]: a single click on an edge scrolls the map by several
//! frames, not until release. With `click_scroll=1` this is the only way to
//! scroll; with `click_scroll=0` pure hovering works, as in C++.

/// Frames per second the original's `GridStepLevel` is tied to
/// (`SVGA_DEFAULT_REFRESH_RATE = 30`, `svga.cpp:32`).
pub const REFRESH_RATE: f32 = 30.0;

/// Maximum `ScrollRateLimit` (`game_manager.cpp:1643`).
pub const MAX_RATE_LIMIT: f32 = 1.5;

/// How long the LMB "stays held" after a click (see the input difference in the module header).
pub const CLICK_HOLD_SECONDS: f32 = 0.12;

/// One of the twelve scroll windows (`WINDOW_SCROLL_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollZone {
    /// `WINDOW_SCROLL_UP_WINDOW` — (50,0)…(590,8).
    Up,
    /// `WINDOW_SCROLL_UP_RIGHT_WINDOW` — (590,0)…(639,8).
    UpRight,
    /// `WINDOW_SCROLL_RIGHT_UP_WINDOW` — (632,0)…(639,50).
    RightUp,
    /// `WINDOW_SCROLL_RIGHT_WINDOW` — (632,50)…(639,430).
    Right,
    /// `WINDOW_SCROLL_RIGHT_DOWN_WINDOW` — (632,430)…(639,479).
    RightDown,
    /// `WINDOW_SCROLL_DOWN_RIGHT_WINDOW` — (590,472)…(639,479).
    DownRight,
    /// `WINDOW_SCROLL_DOWN_WINDOW` — (50,472)…(590,479).
    Down,
    /// `WINDOW_SCROLL_DOWN_LEFT_WINDOW` — (0,472)…(50,479).
    DownLeft,
    /// `WINDOW_SCROLL_LEFT_DOWN_WINDOW` — (0,430)…(8,479).
    LeftDown,
    /// `WINDOW_SCROLL_LEFT_WINDOW` — (0,50)…(8,430).
    Left,
    /// `WINDOW_SCROLL_LEFT_UP_WINDOW` — (0,0)…(8,50).
    LeftUp,
    /// `WINDOW_SCROLL_UP_LEFT_WINDOW` — (0,0)…(50,8).
    UpLeft,
}

impl ScrollZone {
    /// All zones in the order of the `windows[]` table (important: intersections of corners and
    /// sides are resolved by the first match, as in `GameManager_ProcessInput`).
    pub const ALL: [Self; 12] = [
        Self::Up,
        Self::UpRight,
        Self::RightUp,
        Self::Right,
        Self::RightDown,
        Self::DownRight,
        Self::Down,
        Self::DownLeft,
        Self::LeftDown,
        Self::Left,
        Self::LeftUp,
        Self::UpLeft,
    ];

    /// Window rectangle in the 640×480 frame — **inclusive** C++ coordinates
    /// `WINDOW_RECT(ulx, uly, lrx, lry)`.
    pub const fn rect(self) -> (i32, i32, i32, i32) {
        match self {
            Self::Up => (50, 0, 590, 8),
            Self::UpRight => (590, 0, 639, 8),
            Self::RightUp => (632, 0, 639, 50),
            Self::Right => (632, 50, 639, 430),
            Self::RightDown => (632, 430, 639, 479),
            Self::DownRight => (590, 472, 639, 479),
            Self::Down => (50, 472, 590, 479),
            Self::DownLeft => (0, 472, 50, 479),
            Self::LeftDown => (0, 430, 8, 479),
            Self::Left => (0, 50, 8, 430),
            Self::LeftUp => (0, 0, 8, 50),
            Self::UpLeft => (0, 0, 50, 8),
        }
    }

    /// Point inside the window (inclusive bounds, like `WINDOW_RECT`).
    pub fn contains(self, x: i32, y: i32) -> bool {
        let (ulx, uly, lrx, lry) = self.rect();

        x >= ulx && x <= lrx && y >= uly && y <= lry
    }

    /// Scroll direction in cells: `GameManager_SetGridOffset(±1, ±1)`.
    pub const fn direction(self) -> (i32, i32) {
        match self {
            Self::Up | Self::UpLeft | Self::RightUp => (0, -1),
            Self::UpRight => (1, -1),
            Self::Right => (1, 0),
            Self::RightDown | Self::DownRight => (1, 1),
            Self::Down => (0, 1),
            Self::DownLeft | Self::LeftDown => (-1, 1),
            Self::Left => (-1, 0),
            Self::LeftUp => (-1, -1),
        }
    }
}

/// Scroll zone under the window point (`None` — the point is not in a scroll strip).
///
/// The order is like the `windows[]` traversal in `GameManager_ProcessInput`: the first
/// match wins, so the corner windows overlap the sides exactly
/// as in C++ (`UpRight` before `RightUp`, etc.).
pub fn zone_at(x: i32, y: i32) -> Option<ScrollZone> {
    ScrollZone::ALL
        .into_iter()
        .find(|zone| zone.contains(x, y))
}

/// Scroll state: pointer, "held" LMB and the step accumulator.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrollState {
    pointer: Option<(i32, i32)>,
    hold: bool,
    hold_left: f32,
    rate: f32,
}

impl ScrollState {
    /// Empty state (mouse outside the frame, button not pressed).
    pub fn new() -> Self {
        Self::default()
    }

    /// Remembers the pointer position (`MouseMove`).
    pub fn set_pointer(&mut self, x: i32, y: i32) {
        self.pointer = Some((x, y));
    }

    /// The pointer left the frame (`MouseMove(-1,-1)` outside the field).
    pub fn clear_pointer(&mut self) {
        self.pointer = None;
    }

    /// LMB click: starts the "hold" pulse (`click_scroll`).
    pub fn press(&mut self, x: i32, y: i32) {
        self.pointer = Some((x, y));
        self.hold = true;
        self.hold_left = CLICK_HOLD_SECONDS;
    }

    /// LMB release (if it appears in the window layer).
    pub fn release(&mut self) {
        self.hold = false;
        self.hold_left = 0.0;
    }

    /// Advances the hold pulse (`dt` seconds).
    pub fn tick(&mut self, dt: f32) {
        if self.hold {
            self.hold_left -= dt.max(0.0);

            if self.hold_left <= 0.0 {
                self.hold = false;
                self.hold_left = 0.0;
            }
        }
    }

    /// The LMB is considered held (the click is still "held").
    pub fn is_held(&self) -> bool {
        self.hold
    }

    /// Current pointer.
    pub fn pointer(&self) -> Option<(i32, i32)> {
        self.pointer
    }

    /// Zone under the pointer.
    pub fn zone(&self) -> Option<ScrollZone> {
        self.pointer.and_then(|(x, y)| zone_at(x, y))
    }

    /// One scrolling step: `Some((dx, dy))` in **screen** pixels.
    ///
    /// `click_scroll` — the "Click to Scroll" setting: with `true` scrolling happens
    /// only while the LMB is held, with `false` — on hover. `quick_scroll` —
    /// "Scroll Speed" (4…128). The step accumulates like
    /// `GameManager_ScrollMainMapView`: the fractional part is carried over to the next
    /// frame, so the speed does not depend on the frame rate.
    pub fn step(
        &mut self,
        dt: f32,
        click_scroll: bool,
        quick_scroll: i32,
    ) -> Option<(i32, i32)> {
        let (x, y) = self.pointer?;
        let Some(zone) = active_zone_at(x, y) else {
            self.rate = 0.0;
            return None;
        };

        if click_scroll && !self.hold {
            self.rate = 0.0;
            return None;
        }

        let rate_limit = if dt <= 0.0 {
            0.0
        } else {
            (dt * REFRESH_RATE).min(MAX_RATE_LIMIT)
        };

        self.rate += quick_scroll.clamp(1, 128) as f32 * rate_limit;

        let step = self.rate.floor() as i32;

        if step <= 0 {
            return None;
        }

        self.rate -= step as f32;

        let (dx, dy) = zone.direction();

        Some((dx * step, dy * step))
    }

    /// Resets the accumulator (zone change/stop).
    pub fn reset_rate(&mut self) {
        self.rate = 0.0;
    }
}

/// Scroll strip rectangle in `PixelRect` coordinates (for drawing/
/// tests): `(ulx, uly, width, height)`.
pub fn zone_rect(zone: ScrollZone) -> (i32, i32, i32, i32) {
    let (ulx, uly, lrx, lry) = zone.rect();

    (ulx, uly, lrx - ulx + 1, lry - uly + 1)
}

/// The scroll strip must not overlap the clickable panel buttons: in C++
/// `WINDOW_ENDTURN_BUTTON`/`FILES`/`PREFS` are earlier in the table than scrolling,
/// so `GameManager_ProcessInput` gives the click to them. The check is extracted here
/// so that both the client (`App`) and the tests use it.
pub fn panel_button_blocks(x: i32, y: i32) -> bool {
    crate::panels::panel_button_at(x, y).is_some()
}

/// Point in the scroll strip taking the panel button priority into account.
pub fn active_zone_at(x: i32, y: i32) -> Option<ScrollZone> {
    if panel_button_blocks(x, y) {
        return None;
    }

    zone_at(x, y)
}

#[cfg(test)]
#[path = "scroll_tests.rs"]
mod tests;
