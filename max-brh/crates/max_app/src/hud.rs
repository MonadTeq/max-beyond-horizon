//! HUD (M4.2): minimap, selected unit panel, message log, resource bar
//! and buttons **over** the map frame.
//!
//! Everything here — pure functions of the state (`max_core::game::GameState`) and
//! a small [`HudState`] that belongs to [`App`] and **is not part of**
//! the snapshot/CRC: the selected unit, the path preview, the log and outgoing
//! orders — this is UI, not simulation (`AppDigest` does not see them, tick
//! determinism does not change).
//!
//! # What is drawn
//!
//! | Element | Source | Where |
//! |---|---|---|
//! | minimap 112×112 → small buffer | [`SurfaceMap`], own team units | [`draw_minimap`] |
//! | unit panel (name/type, HP, ammo, order) | [`UnitInfo`] + `base_values` | [`unit_panel_lines`] |
//! | message log (ring buffer) | [`MessageLog`] | [`draw_log`] |
//! | chat input line (packet 18) | [`ChatInput`] | [`draw_chat_input`] |
//! | team resource bar | [`TeamUnits`] (gold + complexes) | [`resources_line`] |
//! | End Turn / Menu buttons | `max_render::ui::Button` | [`hud_ui`] |
//! | path preview | [`PathPreview`] (line by cells) | [`draw_path_preview`] |
//!
//! [`SurfaceMap`]: max_core::map::SurfaceMap
//! [`UnitInfo`]: max_core::unit::UnitInfo
//! [`TeamUnits`]: max_core::team::TeamUnits
//!
//! # Orders
//!
//! [`build_move_order`] assembles packet **38** (`Command::Move`, path
//! `GroundPath`) via `max_core::pathfinding::find_ground_path` — the same
//! search as `Paths_RequestPath` at the receiver; [`build_attack_order`] —
//! packet **08** with processor `P3` (`ORDER_MOVE_TO_ATTACK` + target). Both
//! return ready [`Command`]s that:
//!
//! * in demo mode (without the network) are applied locally — [`App::flush_orders`]
//!   calls `max_core::events::apply_command`;
//! * in a network session go to `SessionHandle` (`net::flush_hud_orders`):
//!   first the local application (as in C++ before multicast), then
//!   `Packet::Command` into the session — the `max_net::wire::command` encoders.

use max_core::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS;
use max_core::classification::Environment;
use max_core::enums::{unit_flags, GameStateType, Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::events::command_finished_turn;
use max_core::universe::{Dispatcher, StateRef};
use max_core::geo::{PathStep, Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::input::{
    Command, MovePathData, OrderPayload, Packet08Order, Packet38Move, PayloadP1, PayloadP2,
    PayloadP3,
};
use max_core::team::TeamId;
use max_core::unit::{UnitId, UnitInfo};
use max_core::unit_values::{ATTRIB_AMMO, ATTRIB_HITS};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::map_render::TileStyle;
use max_render::text::{draw_text_line, GLYPH_HEIGHT, GLYPH_WIDTH};
use max_render::ui::{render_ui, Button, Rect, UiState, UiTheme, Widget};
use max_render::viewport::Viewport;

use crate::App;

/// Default message log capacity (lines).
pub const MESSAGE_LOG_CAPACITY: usize = 8;

/// Inner HUD panel padding, pixels.
pub const HUD_MARGIN: i32 = 4;
/// Height of the resource bar at the top.
pub const HUD_TOP_BAR_HEIGHT: i32 = 14;
/// Height of the status line at the bottom.
pub const HUD_STATUS_HEIGHT: i32 = 12;
/// Maximum minimap side: 112 cells of 1 px.
pub const MINIMAP_MAX_SIDE: i32 = 112;
/// HUD button width.
pub const HUD_BUTTON_WIDTH: i32 = 96;
/// HUD button height.
pub const HUD_BUTTON_HEIGHT: i32 = 20;
/// Gap between buttons.
pub const HUD_BUTTON_GAP: i32 = 4;

/// Path step limit in packet 38 (`REMOTE_MAX_VECTOR_LENGTH` in `max_net`).
///
/// Longer — `WireError::VectorTooLong` when encoding, so the order
/// is rejected already in the HUD.
pub const MAX_PATH_STEPS: usize = 1024;

/// Team colors (original order: `RED, GREEN, BLUE, GRAY, ALIEN`).
pub const TEAM_COLORS: [Rgba; 5] = [
    Rgba::rgb(224, 48, 48),
    Rgba::rgb(48, 192, 72),
    Rgba::rgb(64, 112, 224),
    Rgba::rgb(192, 192, 192),
    Rgba::rgb(212, 168, 64),
];

/// Path preview color (does not overlap the panel and team colors).
pub const PREVIEW_COLOR: Rgba = Rgba::rgb(120, 255, 152);
/// HUD panel background.
pub const HUD_PANEL: Rgba = Rgba::rgb(14, 18, 26);
/// HUD panel frame.
pub const HUD_BORDER: Rgba = Rgba::rgb(96, 116, 148);
/// Main HUD text.
pub const HUD_TEXT: Rgba = Rgba::rgb(232, 236, 244);
/// Muted HUD text.
pub const HUD_TEXT_DIM: Rgba = Rgba::rgb(148, 156, 168);

/// Team color (the index is clamped, `ALIEN` — for everything non-game).
pub fn team_color(team: TeamId) -> Rgba {
    TEAM_COLORS
        .get(usize::from(team.0))
        .copied()
        .unwrap_or(TEAM_COLORS[TEAM_COLORS.len() - 1])
}

// ---------------------------------------------------------------------------
// Message log
// ---------------------------------------------------------------------------

/// Ring buffer of log lines: old records are evicted by new ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLog {
    lines: Vec<String>,
    capacity: usize,
}

impl Default for MessageLog {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageLog {
    /// Log of the standard capacity ([`MESSAGE_LOG_CAPACITY`]).
    pub fn new() -> Self {
        Self::with_capacity(MESSAGE_LOG_CAPACITY)
    }

    /// Log of capacity `capacity` (minimum 1).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            lines: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    /// Adds a line, evicting the oldest on overflow.
    pub fn push(&mut self, line: impl Into<String>) {
        if self.lines.len() >= self.capacity {
            self.lines.remove(0);
        }
        self.lines.push(line.into());
    }

    /// Clears the log.
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Lines from oldest to newest.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Number of lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// The log is empty.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Log capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ---------------------------------------------------------------------------
// Chat (packet 18)
// ---------------------------------------------------------------------------

/// Maximum chat message length in characters.
///
/// In C++ the input line buffer is `char chat_input_buffer[150]`
/// (`networkmenu.hpp:171`), so 149 characters + NUL always fit into
/// the `SmartString` of packet 18 (`remote.cpp:2656-2685`).
pub const CHAT_MAX_CHARS: usize = 149;

/// Default team names (original order: `RED, GREEN, BLUE, GRAY`).
pub const TEAM_NAMES: [&str; 5] = ["RED", "GREEN", "BLUE", "GRAY", "ALIEN"];

/// Team name for the log/chat (`Remote_ReceiveNetPacket_18` takes the name from
/// the game settings; the Rust state has no names, so the default slot names
/// are used).
pub fn team_name(team: TeamId) -> &'static str {
    TEAM_NAMES
        .get(usize::from(team.0))
        .copied()
        .unwrap_or(TEAM_NAMES[TEAM_NAMES.len() - 1])
}

/// Log line for a chat message: `<team name>: <text>`.
///
/// Exactly the format that `Remote_ReceiveNetPacket_18` assembles:
/// `message = team_name; message += ": "; message += text`.
pub fn chat_line(sender: TeamId, message: &str) -> String {
    format!("{}: {message}", team_name(sender))
}

/// State of the chat input line (M4.4).
///
/// Lives in [`HudState`] and, like the whole HUD, **is not part of** the snapshot/CRC: typing
/// text must not change the simulation determinism. While [`Self::is_active`],
/// `ui::handle_session_input` gives the keyboard to the chat, not to the orders.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatInput {
    /// The input line is open (Enter/T in a session).
    active: bool,
    /// The typed ASCII text (up to [`CHAT_MAX_CHARS`] characters).
    buffer: String,
}

impl ChatInput {
    /// Closed empty input line.
    pub const fn new() -> Self {
        Self {
            active: false,
            buffer: String::new(),
        }
    }

    /// Opens the input with an empty buffer (Enter/T).
    pub fn open(&mut self) {
        self.active = true;
        self.buffer.clear();
    }

    /// Closes the input, discarding what was typed (Esc/send).
    pub fn close(&mut self) {
        self.active = false;
        self.buffer.clear();
    }

    /// The input line is open.
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// The typed text.
    pub fn message(&self) -> &str {
        &self.buffer
    }

    /// Adds a printable ASCII character; `false` — the character was rejected
    /// (not ASCII/buffer full/input closed).
    pub fn push_char(&mut self, ch: char) -> bool {
        if !self.active
            || self.buffer.chars().count() >= CHAT_MAX_CHARS
            || !(ch.is_ascii_graphic() || ch == ' ')
        {
            return false;
        }

        self.buffer.push(ch);
        true
    }

    /// Deletes the last character (Backspace); `false` — nothing to delete.
    pub fn backspace(&mut self) -> bool {
        self.active && self.buffer.pop().is_some()
    }

    /// Takes the message and closes the input.
    ///
    /// `None` — the buffer is empty: C++ `Remote_SendNetPacket_18` does not send such
    /// messages, so Enter on an empty line simply closes the input.
    pub fn take_message(&mut self) -> Option<String> {
        self.active = false;
        let message = std::mem::take(&mut self.buffer);

        (!message.is_empty()).then_some(message)
    }
}

// ---------------------------------------------------------------------------
// Path preview
// ---------------------------------------------------------------------------

/// Route preview of the selected unit: a line by cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPreview {
    /// The unit the route was built for.
    pub unit: UnitId,
    /// Starting cell.
    pub from: Point,
    /// Destination cell (coincides with `move_to` of packet 38).
    pub to: Point,
    /// Path steps in order (`PathStep`, as in `GroundPath`).
    pub steps: Vec<PathStep>,
}

impl PathPreview {
    /// Preview from a ready list of steps.
    pub fn new(unit: UnitId, from: Point, to: Point, steps: Vec<PathStep>) -> Self {
        Self {
            unit,
            from,
            to,
            steps,
        }
    }

    /// Route cells: the start and each next one after a step (length `steps + 1`).
    pub fn points(&self) -> Vec<Point> {
        let mut points = Vec::with_capacity(self.steps.len() + 1);
        let mut current = self.from;
        points.push(current);

        for step in &self.steps {
            current = Point::new(current.x + i16::from(step.x), current.y + i16::from(step.y));
            points.push(current);
        }

        points
    }

    /// Route length in steps.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Route without steps (the unit is already in place).
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// End cell of the actual route (the sum of steps), not `to`.
    pub fn last_point(&self) -> Point {
        self.points().pop().unwrap_or(self.from)
    }
}

// ---------------------------------------------------------------------------
// HUD state (not part of the snapshot/CRC)
// ---------------------------------------------------------------------------

/// HUD button under the cursor/press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HudButton {
    /// End Turn: the same path as the timer (`06` + `52`/`finish_turn`).
    EndTurn,
    /// Menu: exit the session to the main menu.
    Menu,
}

impl HudButton {
    /// All HUD buttons in [`hud_ui`] index order.
    pub const ALL: [Self; 2] = [Self::EndTurn, Self::Menu];

    /// Button index in [`hud_ui`].
    pub const fn index(self) -> usize {
        match self {
            Self::EndTurn => 0,
            Self::Menu => 1,
        }
    }

    /// Button label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::EndTurn => "End Turn",
            Self::Menu => "Menu",
        }
    }
}

/// Session UI state: selection, log, preview and outgoing orders.
///
/// Stored in [`App`] but **not part of** `GameState::snapshot_bytes` and
/// `AppDigest`: the selected unit and the log are UI, they do not affect the tick and
/// the CRC barrier 45.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudState {
    /// Selected unit (may become stale — check [`App::selected_unit`]).
    pub selected: Option<UnitId>,
    /// Route preview (the last `RMB` order).
    pub preview: Option<PathPreview>,
    /// Message log (ring buffer).
    pub log: MessageLog,
    /// Chat input line (packet 18): active while the player types a message.
    pub chat: ChatInput,
    /// Orders not yet passed to the network/simulation (38/08/18 from input).
    pub pending: Vec<Command>,
    /// HUD button under the cursor.
    pub hover_button: Option<HudButton>,
    /// 1:1 panel button under the cursor (when the chrome is active).
    pub hover_panel: Option<crate::panels::PanelButton>,
    /// Local state of the 1:1 window panels: toggles, timer, cursor.
    pub panels: crate::panels::PanelState,
    /// End Turn pressed in a network session (the turn is closed by the game loop).
    pub end_turn_requested: bool,
    /// Cursor position in frame pixels (modal screens: `BuildMenu`).
    pub mouse_position: Option<(i32, i32)>,
    /// Cursor mode of the selected unit — C++ `UnitInfo::cursor` (`3` x-fer,
    /// `4` repair, `6` reload, `7` load, `8` steal, `9` disable). `0` — none.
    ///
    /// UI state: not part of the snapshot/CRC (in C++ the field lives in `UnitInfo`,
    /// but here — so that switching the mode does not change the digest). Reset
    /// on selection change, like `GameManager_MenuUnitSelect` in C++.
    pub cursor_mode: u8,
    /// "Board the transport" mode — C++ `UnitInfo::enter_mode`; UI state.
    pub enter_mode: bool,
    /// Targeting mode — C++ `UnitInfo::targeting_mode`: a click on an enemy/empty
    /// cell within range gives `ORDER_MOVE_TO_ATTACK`/`ORDER_FIRE`
    /// (`GameManager_GetWindowCursor`/`SetUnitOrder`, `game_manager.cpp:4384`).
    pub targeting_mode: bool,
    /// Whether `Shift` is pressed — C++ `GameManager_IsShiftKeyPressed`. Enables
    /// `CURSOR_WAY` (route by points) in `GameManager_GetWindowCursor`
    /// (`game_manager.cpp:4459`/`:5989`). The port's input layer does not pass modifiers
    /// yet, so the flag is set via [`App::set_shift_pressed`]
    /// (SDL layer/QA); by default `false`.
    pub shift_pressed: bool,
    /// Hint above the map — C++ `MessageManager_DrawMessage`
    /// (`MESSAGE_BOX_INFO`, `MESSAGE_BOX_MODELESS`) from
    /// `GameManager_DrawUnitStatusMessage`. For a building unit:
    /// «Currently building: X. / Turns to completion: N.», for a ready one —
    /// «Finished building X.  To move the Y out of the construction site, …».
    ///
    /// UI state: not part of the snapshot/CRC. Lives until a click on the map/selection
    /// change, like `MessageManager_ClearMessageBox` in C++.
    pub hint: Option<String>,
}

impl Default for HudState {
    fn default() -> Self {
        Self::new()
    }
}

impl HudState {
    /// Empty HUD state.
    pub fn new() -> Self {
        Self {
            selected: None,
            preview: None,
            log: MessageLog::new(),
            chat: ChatInput::new(),
            pending: Vec::new(),
            hover_button: None,
            hover_panel: None,
            panels: crate::panels::PanelState::default(),
            end_turn_requested: false,
            mouse_position: None,
            cursor_mode: 0,
            enter_mode: false,
            targeting_mode: false,
            shift_pressed: false,
            hint: None,
        }
    }

    /// Resets everything (new match/save load).
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Selects a unit (`None` — clear the selection).
    ///
    /// Like `GameManager_MenuUnitSelect` in C++: the newly selected unit's
    /// `targeting_mode`/`enter_mode`/`cursor` are reset, so our UI modes are
    /// reset here too.
    pub fn select(&mut self, id: Option<UnitId>) {
        if self.selected != id {
            self.cursor_mode = 0;
            self.enter_mode = false;
            self.targeting_mode = false;
        }
        self.selected = id;
    }

    /// Writes a line to the log.
    pub fn push_log(&mut self, line: impl Into<String>) {
        self.log.push(line);
    }

    /// Clears the selection and the preview (Esc/right click on the unit).
    pub fn clear_interaction(&mut self) {
        self.selected = None;
        self.preview = None;
        self.cursor_mode = 0;
        self.enter_mode = false;
        self.targeting_mode = false;
        self.hint = None;
    }

    /// Shows a hint above the map (`MessageManager_DrawMessage`).
    pub fn set_hint(&mut self, text: impl Into<String>) {
        self.hint = Some(text.into());
    }

    /// `MessageManager_ClearMessageBox`: removes the hint.
    pub fn clear_hint(&mut self) {
        self.hint = None;
    }

    /// Requests to close the turn (the End Turn button in a network session).
    pub fn request_end_turn(&mut self) {
        self.end_turn_requested = true;
    }

    /// Takes the "End Turn pressed" flag.
    pub fn take_end_turn_request(&mut self) -> bool {
        std::mem::take(&mut self.end_turn_requested)
    }
}

// ---------------------------------------------------------------------------
// HUD layout
// ---------------------------------------------------------------------------

/// HUD rectangles in frame pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudLayout {
    /// Resource bar at the top.
    pub top_bar: PixelRect,
    /// Message log panel (under the resource bar).
    pub log_panel: PixelRect,
    /// The whole bottom panel.
    pub bar: PixelRect,
    /// Minimap.
    pub minimap: PixelRect,
    /// Selected unit panel.
    pub unit_panel: PixelRect,
    /// Status line at the bottom of the frame.
    pub status: PixelRect,
    /// End Turn button.
    pub end_turn: Rect,
    /// Menu button.
    pub menu: Rect,
}

impl HudLayout {
    /// Button rectangle.
    pub fn button(self, button: HudButton) -> Rect {
        match button {
            HudButton::EndTurn => self.end_turn,
            HudButton::Menu => self.menu,
        }
    }
}

/// HUD layout for a `width × height` frame.
///
/// The panels do not overlap and stay inside the frame; on small screens
/// (e.g. 64×48) the elements degenerate into empty rectangles, and
/// drawing skips them — the HUD never panics.
pub fn hud_layout(width: u32, height: u32) -> HudLayout {
    let w = width.max(1) as i32;
    let h = height.max(1) as i32;
    let margin = HUD_MARGIN;

    let top_bar = PixelRect::new(0, 0, w, HUD_TOP_BAR_HEIGHT.min(h));

    // Bottom panel: a quarter of the frame, but at least 72 px (and no more than the frame).
    let bar_height = (h / 4).clamp(72, 148).min(h);
    let bar_y = h - bar_height;
    let bar = PixelRect::new(0, bar_y, w, bar_height);

    let status_height = HUD_STATUS_HEIGHT.min(bar_height);
    let status = PixelRect::new(
        margin,
        h - status_height,
        (w - margin * 2).max(0),
        status_height,
    );

    // Minimap: a square fitted between the top of the panel and the status line.
    let inner_top = bar_y + margin;
    let inner_bottom = (h - status_height - margin).max(inner_top);
    let side = (inner_bottom - inner_top)
        .clamp(1, MINIMAP_MAX_SIDE)
        .min((w / 3).max(1));
    let minimap = PixelRect::new(margin, inner_top, side, side);

    // Buttons on the right: End Turn and Menu in a column (no wider than a third of the frame so as not to
    // overlap the minimap on small screens).
    let button_width = HUD_BUTTON_WIDTH.min((w / 3).max(0)).max(0);
    let button_height = HUD_BUTTON_HEIGHT
        .min((side - HUD_BUTTON_GAP).max(0) / 2)
        .max(0);
    let buttons_x = (w - margin - button_width).max(0);
    let end_turn = Rect::new(buttons_x, inner_top, button_width, button_height);
    let menu = Rect::new(
        buttons_x,
        inner_top + button_height + HUD_BUTTON_GAP,
        button_width,
        button_height,
    );

    // Unit panel — between the minimap and the buttons.
    let unit_x = minimap.lrx() + margin;
    let unit_width = (buttons_x - margin - unit_x).max(0);
    let unit_panel = PixelRect::new(unit_x, inner_top, unit_width, side);

    // Log: above the bottom panel on the left, as high as the buffer capacity.
    let log_wanted = (h - bar_height - HUD_TOP_BAR_HEIGHT - margin * 2).max(0);
    let log_height = (MESSAGE_LOG_CAPACITY as i32 * GLYPH_HEIGHT + 10)
        .min(log_wanted)
        .max(0);
    let log_width = (w - margin * 2).min(320).max(0);
    let log_panel = PixelRect::new(
        margin,
        HUD_TOP_BAR_HEIGHT.min(h),
        log_width,
        if log_height >= GLYPH_HEIGHT + 4 {
            log_height
        } else {
            0
        },
    );

    HudLayout {
        top_bar,
        log_panel,
        bar,
        minimap,
        unit_panel,
        status,
        end_turn,
        menu,
    }
}

/// HUD button under the screen point (overlap considered — buttons are above the panels).
pub fn hud_button_at(width: u32, height: u32, x: i32, y: i32) -> Option<HudButton> {
    let layout = hud_layout(width, height);

    HudButton::ALL.into_iter().find(|button| {
        layout
            .button(*button)
            .contains(max_render::ui::Point::new(x, y))
    })
}

/// Minimap cell under the screen point (a click on the minimap moves the camera).
pub fn minimap_grid_at(layout: &HudLayout, x: i32, y: i32) -> Option<Point> {
    let rect = layout.minimap;

    if rect.is_empty() || !rect.contains(x, y) {
        return None;
    }

    let px = x - rect.ulx;
    let py = y - rect.uly;
    let grid_x = (px * i32::from(MAP_WIDTH) / rect.width).clamp(0, i32::from(MAP_WIDTH) - 1);
    let grid_y = (py * i32::from(MAP_HEIGHT) / rect.height).clamp(0, i32::from(MAP_HEIGHT) - 1);

    Some(Point::new(grid_x as i16, grid_y as i16))
}

// ---------------------------------------------------------------------------
// Orders from the HUD
// ---------------------------------------------------------------------------

/// Ready move order: packet 38 + route preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOrder {
    /// `Command::Move` (38) for the session/simulation.
    pub command: Command,
    /// Route line for the HUD.
    pub preview: PathPreview,
}

/// Builds a move order (packet **38**) for `id` into the cell `destination`.
///
/// The path is computed by [`find_ground_path`] with the same access modifier as
/// the receiver's `PathRequest` (`ACCESS_MODIFIER_SAME_CLASS_BLOCKS`), so the
/// route matches what `Paths_RequestPath` would build. The order state —
/// `InProgress`: the path is ready, the receiver does not need to request
/// it again (`apply_packet_38` does not set a request for `InProgress`).
///
/// `None` — no unit, it is destroyed, already in the target cell, the path is not
/// found or is longer than [`MAX_PATH_STEPS`].
pub fn build_move_order(state: StateRef<'_>, id: UnitId, destination: Point) -> Option<MoveOrder> {
    let unit = state.unit(id)?;

    if unit.hits == 0 || unit.grid == destination {
        return None;
    }

    if destination.x < 0
        || destination.y < 0
        || destination.x >= MAP_WIDTH
        || destination.y >= MAP_HEIGHT
    {
        return None;
    }

    let path = state.find_ground_path(
        unit,
        destination,
        ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
        0,
    )?;

    if path.steps.is_empty() || path.steps.len() > MAX_PATH_STEPS {
        return None;
    }

    let command = Command::Move(Packet38Move {
        unit: id,
        order: UnitOrder::Move,
        state: UnitOrderState::InProgress,
        grid: unit.grid,
        move_to: destination,
        group_speed: unit.group_speed,
        path: Some(MovePathData::Ground(path.steps.clone())),
    });
    let preview = PathPreview::new(id, unit.grid, destination, path.steps);

    Some(MoveOrder { command, preview })
}

/// Builds an attack order (packet **08**, processor `P3`) on the enemy `target`.
///
/// `P3 ⊃ P2 ⊃ P1`: the new order `ORDER_MOVE_TO_ATTACK`, state `Init`,
/// the target in `move_to`/`fire_on`/`enemy_unit`. The path to the attack position is built
/// by the receiver (`Paths_RequestPath` in `process_order_move`) — as in C++.
pub fn build_attack_order(state: StateRef<'_>, id: UnitId, target: UnitId) -> Option<Command> {
    let unit = state.unit(id)?;
    let enemy = state.unit(target)?;

    if unit.hits == 0 || enemy.hits == 0 || unit.team == enemy.team || id == target {
        return None;
    }

    let payload = PayloadP3 {
        p2: PayloadP2 {
            p1: PayloadP1 {
                order: UnitOrder::MoveToAttack,
                state: UnitOrderState::Init,
                prior_order: unit.orders,
                prior_state: unit.state,
                disabled_reaction_fire: unit.disabled_reaction_fire,
            },
            parent_unit: unit.parent_unit,
            experience: unit.experience,
        },
        move_to: enemy.grid,
        fire_on: enemy.grid,
        enemy_unit: Some(target),
        transfer_cargo: 0,
        stealth_dice_roll: 0,
    };

    Some(Command::Order(Packet08Order {
        unit: id,
        payload: OrderPayload::P3(payload),
    }))
}

/// Fire-at-cell order without a target — C++ `GameManager_SetUnitOrder`
/// (`game_manager.cpp:6360-6370`): in `targeting_mode` a click on an empty cell within
/// range gives `fire_on_grid = (x, y)` and `ORDER_FIRE`/`ORDER_STATE_INIT`.
///
/// `None` — the unit has no shots or cannot fire.
pub fn build_ground_fire_order(state: StateRef<'_>, id: UnitId, grid: Point) -> Option<Command> {
    let unit = state.unit(id)?;

    if unit.hits == 0 || unit.shots == 0 {
        return None;
    }

    Some(Command::Order(Packet08Order {
        unit: id,
        payload: OrderPayload::P3(PayloadP3 {
            p2: PayloadP2 {
                p1: PayloadP1 {
                    order: UnitOrder::Fire,
                    state: UnitOrderState::Init,
                    prior_order: unit.orders,
                    prior_state: unit.state,
                    disabled_reaction_fire: unit.disabled_reaction_fire,
                },
                parent_unit: unit.parent_unit,
                experience: unit.experience,
            },
            move_to: grid,
            fire_on: grid,
            enemy_unit: unit.enemy_unit,
            transfer_cargo: unit.transfer_cargo,
            stealth_dice_roll: unit.stealth_dice_roll,
        }),
    }))
}

/// Order for the builder to leave a completed site — packet **08**, processor
/// `P3` (`ORDER_ACTIVATE`/`ORDER_STATE_IN_TRANSITION`).
///
/// The literal C++ player path (`game_manager.cpp:6879-6890`): the selected unit in
/// ORDER_STATE_UNIT_READY receives move_to_grid = the cell under the cursor, then
/// `UnitsManager_SetNewOrder(selected, ORDER_ACTIVATE,
/// `ORDER_STATE_IN_TRANSITION)` (in the network — `Remote_SendNetPacket_08`). The path is
/// **not built** here: `PositionInTape` DEINIT takes the builder to the tape edge
/// toward `move_to`, `ActivateEngineer` removes the tape and cones and
/// issues `ORDER_MOVE` (the simulation will request the path), and the building opens
/// via `UnitsManager_BuildingReady` when the builder leaves the 2×2 bounds.
///
/// `None` — the unit is not a "completed builder" (no parent building in
/// `ORDER_IDLE`/`BUILDING_READY`): the click remains a regular move order.
pub fn build_builder_exit_order(
    state: StateRef<'_>,
    id: UnitId,
    destination: Point,
) -> Option<Command> {
    state.builder_awaiting_exit( id)?;

    let unit = state.unit(id)?;

    Some(Command::Order(Packet08Order {
        unit: id,
        payload: OrderPayload::P3(PayloadP3 {
            p2: PayloadP2 {
                p1: PayloadP1 {
                    order: UnitOrder::Activate,
                    state: UnitOrderState::InTransition,
                    prior_order: unit.orders,
                    prior_state: unit.state,
                    disabled_reaction_fire: unit.disabled_reaction_fire,
                },
                parent_unit: unit.parent_unit,
                experience: unit.experience,
            },
            move_to: destination,
            fire_on: Point::ZERO,
            enemy_unit: unit.enemy_unit,
            transfer_cargo: unit.transfer_cargo,
            stealth_dice_roll: unit.stealth_dice_roll,
        }),
    }))
}

/// `GameManager_IsUnitNextToPosition` (`game_manager.cpp:3208`) — the cell
/// falls into the rectangle `grid-1 .. grid+size` (bounds inclusive).
///
/// For a 2×2 building this is 4×4: the site cells themselves and the `BLDMRK*` ring around it.
/// C++ uses the check in `GameManager_GetBuilderUnitCursor`
/// (`:5832-5843`): while the builder is in `ORDER_STATE_UNIT_READY`, the cursor over such
/// a cell — `CURSOR_UNIT_GO`, and the click gives `ORDER_ACTIVATE`
/// (`:6879-6890`). The hint `_(276c)` asks the player to click "the cell with the
/// symbol" — this is exactly this ring.
pub fn builder_exit_zone_contains(building: &UnitInfo, cell: Point) -> bool {
    let size: i16 = if building.is_quad() { 2 } else { 1 };

    cell.x >= building.grid.x.wrapping_sub(1)
        && cell.x <= building.grid.x.wrapping_add(size)
        && cell.y >= building.grid.y.wrapping_sub(1)
        && cell.y <= building.grid.y.wrapping_add(size)
}

/// **Left** click on a cell next to a completed building —
/// `CURSOR_UNIT_GO` → `ORDER_ACTIVATE`/`IN_TRANSITION`
/// (`game_manager.cpp:5832-5843` + `:6879-6890`).
///
/// The difference from [`build_builder_exit_order`] is only the zone check: C++ gives
/// this order only when the cursor is over a cell "next to the building" (4×4), otherwise
/// the cursor — `CURSOR_UNIT_NO_GO` and `GameManager_ManagePlayerAction` clears
/// the selection.
pub fn build_builder_exit_click_order(
    state: StateRef<'_>,
    id: UnitId,
    destination: Point,
) -> Option<Command> {
    let building_id = state.builder_awaiting_exit( id)?;
    let building = state.unit(building_id)?;

    if !builder_exit_zone_contains(building, destination) {
        return None;
    }

    build_builder_exit_order(state, id, destination)
}

/// Order to bring a ready unit out of a factory/transport — C++ `CURSOR_ACTIVATE`
/// (`game_manager.cpp:6918-6926`), packet **08/P3**.
///
/// C++ in `GameManager_ManagePlayerAction` for a selected unit with
/// `GetOrder() == ORDER_IDLE` (the ready unit stands **inside** the container) computes
/// the cursor `GameManager_GetAirUnitCursor` (`game_manager.cpp:5937-5962`):
/// a click on a cell next to the container — `CURSOR_ACTIVATE`, and then:
///
/// ```text
/// UnitInfo* parent = GameManager_SelectedUnit->GetParent();
/// parent->move_to_grid_x/y = GameManager_MousePosition;   // the click cell
/// parent->SetParent(&*GameManager_SelectedUnit);
/// UnitsManager_SetNewOrder(parent, ORDER_ACTIVATE, ORDER_STATE_EXECUTING_ORDER);
/// ```
///
/// `parent` — the container (a factory whose `parent_unit` is the ready unit, or a transport
/// with a passenger); **it** receives the order, and `UnitsManager_ActivateUnit`
/// (`units_manager.cpp:3826`) moves/releases the unit. A cell inside the container's
/// 2×2 — `CURSOR_FRIEND`, no order.
///
/// `None` — the selected unit is not "in a container" (`ORDER_IDLE` without a parent),
/// the click is not next to the container or the cell is impassable for it.
pub fn build_activate_order(
    state: StateRef<'_>,
    id: UnitId,
    destination: Point,
) -> Option<Command> {
    let unit = state.unit(id)?;

    if unit.hits == 0 || unit.orders != UnitOrder::Idle {
        return None;
    }

    let parent_id = unit.parent_unit?;
    let parent = state.unit(parent_id)?;

    // `GameManager_IsUnitNextToPosition(parent, grid_x, grid_y)`.
    if !builder_exit_zone_contains(parent, destination) {
        return None;
    }

    // An air unit inside the container's 2×2 — `CURSOR_FRIEND` (not launched).
    if unit.is_air()
        && parent.grid.x <= destination.x
        && parent.grid.x + 1 >= destination.x
        && parent.grid.y <= destination.y
        && parent.grid.y + 1 >= destination.y
    {
        return None;
    }

    if state.is_accessible(
        unit.unit_type,
        unit.team,
        destination,
        max_core::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
    ) == 0
    {
        return None;
    }

    Some(Command::Order(Packet08Order {
        unit: parent_id,
        payload: OrderPayload::P3(PayloadP3 {
            p2: PayloadP2 {
                p1: PayloadP1 {
                    order: UnitOrder::Activate,
                    state: UnitOrderState::ExecutingOrder,
                    prior_order: parent.orders,
                    prior_state: parent.state,
                    disabled_reaction_fire: parent.disabled_reaction_fire,
                },
                parent_unit: Some(id),
                experience: parent.experience,
            },
            move_to: destination,
            fire_on: Point::ZERO,
            enemy_unit: parent.enemy_unit,
            transfer_cargo: parent.transfer_cargo,
            stealth_dice_roll: parent.stealth_dice_roll,
        }),
    }))
}

/// Air unit move order — packet **08/P3**
/// (`ORDER_MOVE`/`ORDER_STATE_INIT`).
///
/// In C++ the air unit receives the order through the same
/// `GameManager_SetUnitOrder(ORDER_MOVE, …)` (`game_manager.cpp:6898`), and the path
/// is built by the receiver: `UnitsManager_InitUnitPath` →
/// `Paths_GetAirPath` (`paths.cpp:97`) in a straight line in pixels. The port does not
/// run aviation through the ground `Packet38Move` (`build_move_order`), so here
/// only `move_to`: the simulation itself will compute `AirPath` and the takeoff
/// (`ExecuteMove` → `ORDER_TAKE_OFF`).
pub fn build_air_move_order(
    state: StateRef<'_>,
    id: UnitId,
    destination: Point,
) -> Option<Command> {
    let unit = state.unit(id)?;

    if unit.hits == 0 || unit.grid == destination {
        return None;
    }

    if destination.x < 0
        || destination.y < 0
        || destination.x >= MAP_WIDTH
        || destination.y >= MAP_HEIGHT
    {
        return None;
    }

    if !unit.is_air() {
        return None;
    }

    Some(Command::Order(Packet08Order {
        unit: id,
        payload: OrderPayload::P3(PayloadP3 {
            p2: PayloadP2 {
                p1: PayloadP1 {
                    order: UnitOrder::Move,
                    state: UnitOrderState::Init,
                    prior_order: unit.orders,
                    prior_state: unit.state,
                    disabled_reaction_fire: unit.disabled_reaction_fire,
                },
                parent_unit: unit.parent_unit,
                experience: unit.experience,
            },
            move_to: destination,
            fire_on: Point::ZERO,
            enemy_unit: unit.enemy_unit,
            transfer_cargo: unit.transfer_cargo,
            stealth_dice_roll: unit.stealth_dice_roll,
        }),
    }))
}

/// Builds a power on/off order (the `Power On`/`Power Off` popup
/// in C++, packet **08**, processor `P1`).
///
/// For `RESEARCH`, powering on starts research of the default topic
/// (C++ `ResearchMenu_UpdateResearchProgress(team, topic, +1)` in
/// `UnitsManager_ProcessOrderPowerOn`).
pub fn build_power_order(state: StateRef<'_>, id: UnitId) -> Option<Command> {
    let unit = state.unit(id)?;
    if unit.hits == 0 {
        return None;
    }

    let order = if unit.orders == UnitOrder::PowerOn {
        UnitOrder::PowerOff
    } else {
        UnitOrder::PowerOn
    };

    Some(Command::Order(Packet08Order {
        unit: id,
        payload: OrderPayload::P1(PayloadP1 {
            order,
            state: UnitOrderState::Init,
            prior_order: unit.orders,
            prior_state: unit.state,
            disabled_reaction_fire: unit.disabled_reaction_fire,
        }),
    }))
}

/// Unit under the screen point: `viewport.screen_to_grid` + `Access_GetTeamUnit`
/// (`access.cpp:1804`) / `Access_GetEnemyUnit` (`:1731`).
///
/// C++ order:
/// 1. `Access_GetTeamUnit(x, y, team, SELECTABLE)` — own unit: `team`,
///    `flags & SELECTABLE`, `order != ORDER_IDLE`; the first pass **without**
///    `GROUND_COVER` and tapes (the `LRGSLAB`/`SMLSLAB` slab lies under every
///    building and the click does not take it), the second — any `SELECTABLE` (the
///    construction site tape too: it is `GROUND_COVER`, but `SELECTABLE`);
/// 2. if there are no own ones — `Access_GetEnemyUnit`: `team != team`, `SELECTABLE`,
///    not `ORDER_IDLE`/`ORDER_EXPLODE`/`DESTROY`, again first without
///    `GROUND_COVER` (except mines), then any;
/// 3. if the found unit is a construction site tape, its **parent** is returned
///    (`unit = unit->GetParent()`), i.e. the builder: `DrawMap_RenderBuildMarker`
///    and `UnitsManager_Popup` work with it, not with the decal.
///
/// Points 1 and 3 are not cosmetic: on a completed site the builder stands in
/// the tape center (`PositionInTape`/`tape_center_offset` ±32 px), its sprite
/// is drawn at the corner of four cells, and the player clicks the **adjacent**
/// site cell. Without `SELECTABLE`/`tape → parent` the click went to the cones/slab
/// (they are not `SELECTABLE`, the port's second pass took them anyway), so there were
/// neither `BLDMRK*` markers nor the "Finished building …" hint, and a right
/// click canceled the order ("Unit cannot move (air/stationary)") — the builder could not
/// leave the site and the building did not open.
///
/// Visibility (`is_visible_to_team`, fog of war) — a port feature: first the units
/// visible to the team, then hidden ones (in C++ `Access_GetEnemyUnit` requires
/// `IsVisibleToTeam`, but in the port units invisible to the team can also be
/// selected — this is an existing simplification that combat and clicking an
/// enemy rely on). A point outside the map — `None`.
pub fn unit_at_screen(
    state: StateRef<'_>,
    viewport: &Viewport,
    team: TeamId,
    x: i32,
    y: i32,
) -> Option<UnitId> {
    let (grid_x, grid_y) = viewport.screen_to_grid(x, y);

    if grid_x < 0 || grid_y < 0 || grid_x >= MAP_WIDTH || grid_y >= MAP_HEIGHT {
        return None;
    }

    let grid = Point::new(grid_x, grid_y);
    let ids = state.units_at(grid);
    // `((*it).flags & flags)` + `GetOrder() != ORDER_IDLE` from C++.
    let selectable = |id: &UnitId, own: bool| {
        state.unit(*id).is_some_and(|unit| {
            (unit.team == team) == own
                && unit.flags & unit_flags::SELECTABLE != 0
                && unit.orders != UnitOrder::Idle
        })
    };
    let is_tape = |id: &UnitId| {
        state
            .unit(*id)
            .is_some_and(|unit| matches!(unit.unit_type, UnitType::LargeTape | UnitType::SmallTape))
    };
    let ground_cover = |id: &UnitId| {
        state
            .unit(*id)
            .is_some_and(|unit| unit.flags & unit_flags::GROUND_COVER != 0)
    };
    let is_mine = |id: &UnitId| {
        state
            .unit(*id)
            .is_some_and(|unit| matches!(unit.unit_type, UnitType::LandMine | UnitType::SeaMine))
    };
    let visible = |id: &UnitId| {
        state
            .unit(*id)
            .is_some_and(|unit| unit.is_visible_to_team(team))
    };
    let plain = |id: &UnitId, own: bool| selectable(id, own) && !is_tape(id) && !ground_cover(id);

    let pick = |predicate: &dyn Fn(&UnitId) -> bool| ids.iter().copied().find(predicate);

    // `Access_GetTeamUnit`: own ones, first visible and not `GROUND_COVER`.
    let own = pick(&|id| visible(id) && plain(id, true))
        .or_else(|| pick(&|id| visible(id) && selectable(id, true)))
        .or_else(|| pick(&|id| plain(id, true)))
        .or_else(|| pick(&|id| selectable(id, true)));

    // `Access_GetEnemyUnit`: foreign ones; mines — an exception to `GROUND_COVER`.
    let enemy = pick(&|id| selectable(id, false) && (plain(id, false) || is_mine(id)))
        .or_else(|| pick(&|id| selectable(id, false)));

    let found = own.or(enemy)?;

    // `if (unit->GetUnitType() == LRGTAPE || SMLTAPE) unit = unit->GetParent();`
    if is_tape(&found) {
        return state.unit(found).and_then(|unit| unit.parent_unit);
    }

    Some(found)
}

/// Whether the unit can receive a move order from the HUD.
///
/// Ground and sea — yes (`find_ground_path` route). Air ones need
/// `Paths_GetAirPath` with pixel geometry (`MovePathData::Air`), so the
/// HUD does not move them yet (see README, "Limitations").
pub fn unit_can_move(unit: &UnitInfo) -> bool {
    matches!(
        unit.environment(),
        Some(Environment::Ground | Environment::Sea | Environment::Amphibious)
    )
}

// ---------------------------------------------------------------------------
// App methods: selection, orders, End Turn
// ---------------------------------------------------------------------------

// Selection, orders and End Turn read/mutate only the facade and the UI state,
// never the transport policy — so they live in the dispatcher-agnostic `App<D>`
// surface (`game-universe-stage-c.md` §2). The generic `App::select_next_unit`
// calls `select_unit`, and the network driver sends the same queued orders.
impl<D: Dispatcher> App<D> {
    /// HUD state (selection, log, preview).
    pub fn hud(&self) -> &HudState {
        &self.hud
    }

    /// Editable HUD state.
    pub fn hud_mut(&mut self) -> &mut HudState {
        &mut self.hud
    }

    /// Whether `Shift` is pressed — C++ `GameManager_IsShiftKeyPressed` (cursor
    /// `CURSOR_WAY`, route by points).
    pub fn shift_pressed(&self) -> bool {
        self.hud.shift_pressed
    }

    /// Sets the `Shift` state (SDL modifier layer/QA).
    pub fn set_shift_pressed(&mut self, pressed: bool) {
        self.hud.shift_pressed = pressed;
    }

    /// Local player team (`player_team` of the match).
    pub fn player_team(&self) -> TeamId {
        self.state_ref().player_team()
    }

    /// The selected unit, if it still exists.
    pub fn selected_unit(&self) -> Option<&UnitInfo> {
        self.hud.selected.and_then(|id| self.state_ref().unit(id))
    }

    /// Selects a unit (a stale id is ignored on read).
    pub fn select_unit(&mut self, id: Option<UnitId>) {
        self.hud.select(id);
        // `WINDOW_CORNER_FLIC` portrait: the new unit immediately gets frame 0 of its FLC
        // (`GameManager_MenuUnitSelect` → `MenuCreateFlic`).
        self.refresh_portrait();
    }

    /// Takes the accumulated HUD orders for sending to the session.
    pub fn take_outgoing_orders(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.hud.pending)
    }

    /// Applies HUD orders locally (demo mode without the network).
    ///
    /// Returns the number of applied commands. Each goes through
    /// `max_core::events::apply_command` — exactly like a packet from the network.
    pub fn flush_orders(&mut self) -> usize {
        let pending = std::mem::take(&mut self.hud.pending);
        let mut applied = 0;

        for command in pending {
            if self.apply(command) == max_core::events::ApplyOutcome::Applied {
                applied += 1;
            }
        }

        applied
    }

    /// End Turn: the same path as the timer in the network loop (`06` + `52`).
    ///
    /// * packet **06** (`Command::FinishedTurn`) — the team finished the turn;
    /// * the `EndTurn` phase (in `TurnBased` 06 sets it itself) and
    ///   [`GameState::finish_turn`] — the analog of closing the 52 barrier.
    ///
    /// `true` — the match is over (`check_end_game_conditions`).
    pub fn end_turn(&mut self) -> bool {
        let team = self.state_ref().player_team();
        let _ = self.apply(command_finished_turn(team));

        if self.state_ref().game_state() != GameStateType::EndTurn {
            self.universe_mut().set_game_state(GameStateType::EndTurn);
        }

        let over = self.universe_mut().finish_turn();
        let line = if over {
            "End Turn: game over".to_string()
        } else {
            format!("End Turn: turn {} begins", self.state_ref().turn_counter())
        };

        self.hud.push_log(line);
        over
    }
}

// Local (single-player) surface: the turn timer lives in the local match
// (`local::LocalGame`), so it stays on `App<LocalDispatcher>`.
impl App {
    /// Remaining turn timer in seconds — `GameManager_TurnTimerValue`.
    ///
    /// C++ stores it as an integer number of seconds and decreases it by 1 per second
    /// (`GameManager_SyncTurnTimer`, `game_manager.cpp:5033`); the panel shows
    /// `"%2.2i:%2.2i"` of this value (`GameManager_DrawTurnTimer`). In the port
    /// the local match counts `f32` seconds ([`crate::local::LocalGame`]),
    /// so `ceil` is taken here — exactly the number C++ would show in
    /// the current second (`180.0 → «03:00»`, `179.4 → «03:00»`, `178.9 → «02:59»`).
    ///
    /// Network matches do not model the turn timer (in C++ it lives in
    /// `Remote_UpdatePauseTimer`): there the panel value remains, by default 0 —
    /// `GameManager_DrawTurnTimer(0)` → «00:00» in `COLOR_RED`.
    pub fn turn_timer_seconds(&self) -> u32 {
        if let Some(local) = self.local() {
            let seconds = local.timer_left.ceil();

            return seconds.clamp(0.0, 3599.0) as u32;
        }

        self.hud().panels.timer_seconds
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Everything [`render_hud`] needs: match state + UI state.
#[derive(Clone, Copy)]
pub struct HudView<'a> {
    /// Match state (units, map, resources).
    pub state: StateRef<'a>,
    /// Selection, log, preview, buttons.
    pub hud: &'a HudState,
    /// Camera: minimap/route line in screen coordinates.
    pub viewport: &'a Viewport,
    /// Local player team.
    pub team: TeamId,
    /// Status line (session/connection/error).
    pub status: &'a str,
}

/// Draws the HUD over the ready map frame.
///
/// Order: path preview → resource bar → log → bottom panel
/// (minimap, unit panel, buttons) → status line. Nothing panics:
/// all rectangles are clamped to the frame, outside the frame `RgbaBuffer::set`
/// returns `false`.
pub fn render_hud(buffer: &mut RgbaBuffer, view: &HudView<'_>) {
    let layout = hud_layout(buffer.width(), buffer.height());

    if let Some(preview) = &view.hud.preview {
        draw_path_preview(buffer, view.viewport, preview);
    }

    // Resource bar.
    if !layout.top_bar.is_empty() {
        buffer.fill_rect(layout.top_bar, HUD_PANEL);
        buffer.fill_rect(
            PixelRect::new(
                layout.top_bar.ulx,
                layout.top_bar.lry() - 1,
                layout.top_bar.width,
                1,
            ),
            HUD_BORDER,
        );
        draw_line_limited(
            buffer,
            layout.top_bar.ulx + 4,
            layout.top_bar.uly + 3,
            layout.top_bar.width - 8,
            &resources_line(view.state, view.team),
            HUD_TEXT,
        );
    }

    draw_log(buffer, &layout, view.hud);

    // Bottom panel.
    if !layout.bar.is_empty() {
        buffer.fill_rect(layout.bar, HUD_PANEL);
        buffer.fill_rect(
            PixelRect::new(layout.bar.ulx, layout.bar.uly, layout.bar.width, 1),
            HUD_BORDER,
        );
    }

    draw_minimap(
        buffer,
        layout.minimap,
        view.state,
        view.viewport,
        view.team,
        view.hud.selected,
    );
    draw_unit_panel(buffer, &layout, view);

    if !layout.status.is_empty() {
        draw_line_limited(
            buffer,
            layout.status.ulx,
            layout.status.uly + 2,
            layout.status.width,
            view.status,
            HUD_TEXT_DIM,
        );
    }

    render_ui(buffer, &hud_ui(&layout, view.hud));
    draw_chat_input(buffer, &layout, view.hud);
}

/// Chat input line rectangle: the bar above the HUD bottom panel.
///
/// A separate function so that tests and drawing see the same rect.
pub fn chat_input_rect(width: u32, height: u32, layout: &HudLayout) -> PixelRect {
    let bar_width = width.max(1) as i32;
    let bar_height = height.max(1) as i32;
    let rect_height = (GLYPH_HEIGHT + 8).max(0);
    let rect_width = (bar_width - HUD_MARGIN * 2).clamp(0, 420);
    let above_bar = layout.bar.uly - rect_height - HUD_MARGIN;
    let max_y = (bar_height - rect_height).max(0);
    let y = above_bar.clamp(0, max_y);

    PixelRect::new(HUD_MARGIN, y, rect_width, rect_height)
}

/// Chat input line over the HUD: `Say: <text>_`.
///
/// Drawn last, so it is not overlapped by the log/buttons; while the input
/// is closed, it takes not a single pixel.
fn draw_chat_input(buffer: &mut RgbaBuffer, layout: &HudLayout, hud: &HudState) {
    if !hud.chat.is_active() {
        return;
    }

    let rect = chat_input_rect(buffer.width(), buffer.height(), layout);

    if rect.is_empty() {
        return;
    }

    buffer.fill_rect(rect, HUD_PANEL);
    stroke(buffer, rect, HUD_BORDER);

    let line = format!("Say: {}_", hud.chat.message());

    draw_line_limited(
        buffer,
        rect.ulx + 4,
        rect.uly + 4,
        rect.width - 8,
        &line,
        HUD_TEXT,
    );
}

/// UI state of the HUD buttons (End Turn / Menu) for [`render_ui`].
///
/// The theme has a transparent background: `render_ui` does not fill the frame and draws only
/// the buttons themselves over the map.
pub fn hud_ui(layout: &HudLayout, hud: &HudState) -> UiState {
    let theme = UiTheme {
        background: Rgba::TRANSPARENT,
        panel: Rgba::TRANSPARENT,
        ..UiTheme::default()
    };
    let widgets = HudButton::ALL
        .into_iter()
        .map(|button| Widget::Button(Button::new(button.label(), layout.button(button))))
        .collect();
    let mut ui = UiState::with_theme(widgets, theme);
    ui.hover = hud.hover_button.map(HudButton::index);

    ui
}

/// Team resource line: gold + total complex stocks.
pub fn resources_line(state: StateRef<'_>, team: TeamId) -> String {
    let index = usize::from(team.0);
    let units = state.team_units().get(index);
    let gold = units.map(|units| units.gold()).unwrap_or(0);
    let mut raw = 0i32;
    let mut fuel = 0i32;
    let mut complex_gold = 0i32;
    let mut power = 0i32;
    let mut workers = 0i32;
    let mut buildings = 0i32;

    if let Some(units) = units {
        for complex in units.complexes() {
            raw += i32::from(complex.raw);
            fuel += i32::from(complex.fuel);
            complex_gold += i32::from(complex.gold);
            power += i32::from(complex.power);
            workers += i32::from(complex.workers);
            buildings += i32::from(complex.buildings);
        }
    }

    format!(
        "TEAM {index}  GOLD {gold}  RAW {raw}  FUEL {fuel}  GOLD+ {complex_gold}  POWER {power}  WORKERS {workers}  BLD {buildings}"
    )
}

/// Panel lines of the selected unit: name/type, HP, ammo, order, cell.
pub fn unit_panel_lines(state: StateRef<'_>, id: UnitId) -> Vec<String> {
    let Some(unit) = state.unit(id) else {
        return vec!["(unit gone)".to_string()];
    };

    let name = if unit.name.is_empty() {
        crate::unit_names::unit_user_friendly_name(unit.unit_type)
    } else {
        unit.name.as_str()
    };
    let max_hits = unit.base_values.get_attribute(ATTRIB_HITS);
    let max_ammo = unit.base_values.get_attribute(ATTRIB_AMMO);

    vec![
        format!("{name} [{:?}]", unit.unit_type),
        format!("HP {}/{}", unit.hits, max_hits.max(0)),
        format!("AMMO {}/{}", unit.ammo, max_ammo.max(0)),
        format!("ORDER {:?} {:?}", unit.orders, unit.state),
        format!("GRID {},{}  TEAM {}", unit.grid.x, unit.grid.y, unit.team.0),
    ]
}

/// Minimap cell color by its surface.
pub fn surface_color(surface: Surface) -> Rgba {
    TileStyle::default().base_color(surface).darken(5)
}

/// Draws the minimap 112×112 → `rect`: surface, camera, own team units.
///
/// Cells are sampled (`px * 112 / width`), so the minimap works at
/// any panel size, not only 112×112.
pub fn draw_minimap(
    buffer: &mut RgbaBuffer,
    rect: PixelRect,
    state: StateRef<'_>,
    viewport: &Viewport,
    team: TeamId,
    selected: Option<UnitId>,
) {
    if rect.is_empty() {
        return;
    }

    for py in 0..rect.height {
        for px in 0..rect.width {
            let tile_x = (px * i32::from(MAP_WIDTH) / rect.width) as i16;
            let tile_y = (py * i32::from(MAP_HEIGHT) / rect.height) as i16;
            let color = surface_color(state.map().surface.get(tile_x, tile_y));

            buffer.set(rect.ulx + px, rect.uly + py, color);
        }
    }

    // Camera view rectangle.
    if let Some(range) = viewport.visible_tiles() {
        let x0 = rect.ulx + range.x0 as i32 * rect.width / i32::from(MAP_WIDTH);
        let y0 = rect.uly + range.y0 as i32 * rect.height / i32::from(MAP_HEIGHT);
        let x1 = rect.ulx + (range.x1 as i32 + 1) * rect.width / i32::from(MAP_WIDTH);
        let y1 = rect.uly + (range.y1 as i32 + 1) * rect.height / i32::from(MAP_HEIGHT);
        let view = PixelRect::from_corners(
            x0.min(rect.lrx() - 1),
            y0.min(rect.lry() - 1),
            x1.max(x0 + 1).min(rect.lrx()),
            y1.max(y0 + 1).min(rect.lry()),
        );

        stroke(buffer, view, HUD_TEXT_DIM);
    }

    // Own team units (and the selected one — larger and in the team color).
    for id in state.unit_ids_in_order() {
        let Some(unit) = state.unit(id) else {
            continue;
        };
        if unit.team != team {
            continue;
        }

        let px = rect.ulx + i32::from(unit.grid.x) * rect.width / i32::from(MAP_WIDTH);
        let py = rect.uly + i32::from(unit.grid.y) * rect.height / i32::from(MAP_HEIGHT);
        let color = team_color(unit.team);
        let radius = if selected == Some(id) { 1 } else { 0 };

        for dy in -radius..=radius {
            for dx in -radius..=radius {
                buffer.set(px + dx, py + dy, color);
            }
        }
    }

    stroke(buffer, rect, HUD_BORDER);
}

/// Draws the route preview: a line through cell centers + the destination marker.
pub fn draw_path_preview(buffer: &mut RgbaBuffer, viewport: &Viewport, preview: &PathPreview) {
    let points = preview.points();

    for pair in points.windows(2) {
        let from = tile_center(viewport, pair[0]);
        let to = tile_center(viewport, pair[1]);

        draw_line(buffer, from, to, PREVIEW_COLOR);
    }

    if let Some(last) = points.last() {
        let (x, y) = tile_center(viewport, *last);
        let marker = PixelRect::new(x - 3, y - 3, 7, 7);

        stroke(buffer, marker, PREVIEW_COLOR);
    }
}

/// Cell center on the screen (in frame pixels).
pub fn tile_center(viewport: &Viewport, grid: Point) -> (i32, i32) {
    let rect = viewport.tile_rect(grid.x, grid.y);

    (rect.ulx + rect.width / 2, rect.uly + rect.height / 2)
}

/// Bresenham line (inclusive) in `color`; outside the frame — ignored.
pub fn draw_line(buffer: &mut RgbaBuffer, from: (i32, i32), to: (i32, i32), color: Rgba) {
    let (mut x, mut y) = from;
    let dx = (to.0 - from.0).abs();
    let dy = -(to.1 - from.1).abs();
    let step_x = if from.0 < to.0 { 1 } else { -1 };
    let step_y = if from.1 < to.1 { 1 } else { -1 };
    let mut error = dx + dy;

    loop {
        buffer.set(x, y, color);

        if x == to.0 && y == to.1 {
            break;
        }

        let doubled = 2 * error;

        if doubled >= dy {
            error += dy;
            x += step_x;
        }
        if doubled <= dx {
            error += dx;
            y += step_y;
        }
    }
}

/// 1 px frame along the rectangle edges.
fn stroke(buffer: &mut RgbaBuffer, rect: PixelRect, color: Rgba) {
    if rect.is_empty() {
        return;
    }

    buffer.fill_rect(PixelRect::new(rect.ulx, rect.uly, rect.width, 1), color);
    buffer.fill_rect(
        PixelRect::new(rect.ulx, rect.lry() - 1, rect.width, 1),
        color,
    );
    buffer.fill_rect(PixelRect::new(rect.ulx, rect.uly, 1, rect.height), color);
    buffer.fill_rect(
        PixelRect::new(rect.lrx() - 1, rect.uly, 1, rect.height),
        color,
    );
}

/// A text line clipped to `max_width` (monospace font).
fn draw_line_limited(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    max_width: i32,
    text: &str,
    color: Rgba,
) {
    let max_chars = (max_width.max(0) / GLYPH_WIDTH) as usize;

    if max_chars == 0 {
        return;
    }

    let line: String = text.chars().take(max_chars).collect();

    draw_text_line(buffer, x, y, &line, color);
}

/// Log panel: the last lines that fit into the rectangle.
fn draw_log(buffer: &mut RgbaBuffer, layout: &HudLayout, hud: &HudState) {
    let panel = layout.log_panel;

    if panel.is_empty() || hud.log.is_empty() {
        return;
    }

    buffer.fill_rect(panel, HUD_PANEL);
    stroke(buffer, panel, HUD_BORDER);

    let max_lines = ((panel.height - 8).max(0) / GLYPH_HEIGHT).max(1) as usize;
    let lines = hud.log.lines();
    let start = lines.len().saturating_sub(max_lines);

    for (row, line) in lines[start..].iter().enumerate() {
        draw_line_limited(
            buffer,
            panel.ulx + 4,
            panel.uly + 4 + row as i32 * GLYPH_HEIGHT,
            panel.width - 8,
            line,
            HUD_TEXT,
        );
    }
}

/// Selected unit panel (or a control hint without a selection).
fn draw_unit_panel(buffer: &mut RgbaBuffer, layout: &HudLayout, view: &HudView<'_>) {
    let panel = layout.unit_panel;

    if panel.is_empty() {
        return;
    }

    buffer.fill_rect(panel, HUD_PANEL);
    stroke(buffer, panel, HUD_BORDER);

    let lines = match view.hud.selected {
        Some(id) if view.state.unit(id).is_some() => {
            let mut lines = unit_panel_lines(view.state, id);

            if let Some(preview) = view
                .hud
                .preview
                .as_ref()
                .filter(|preview| preview.unit == id)
            {
                lines.push(format!("PATH {} steps", preview.len()));
            }

            lines
        }
        _ => vec![
            "No unit selected".to_string(),
            "LMB  - select unit".to_string(),
            "RMB  - move order".to_string(),
            "LMB enemy - attack".to_string(),
            "Esc  - cancel / leave".to_string(),
        ],
    };

    let max_lines = ((panel.height - 8).max(0) / GLYPH_HEIGHT).max(1) as usize;

    for (row, line) in lines.iter().take(max_lines).enumerate() {
        draw_line_limited(
            buffer,
            panel.ulx + 4,
            panel.uly + 4 + row as i32 * GLYPH_HEIGHT,
            panel.width - 8,
            line,
            HUD_TEXT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use max_core::enums::{TeamClan, UnitType};
    use max_core::input::Packet18Chat;
    use max_core::team::{TeamKind, TeamUnits};
    use max_net::Packet;

    use crate::ui::{handle_session_input, InputEvent};
    use crate::{App, AppConfig, Transition};

    /// Application with a RED tank (10,10), a BLUE tank (14,10) and team gold.
    fn test_app() -> (App, UnitId, UnitId) {
        let mut app = App::new(AppConfig::default().with_screen(512, 384));

        let (tank, enemy) = {
            let universe = app.universe_mut();
            universe.set_player_team(TeamId::RED);
            if let Some(info) = universe.team_mut(TeamId::RED) {
                info.team_type = TeamKind::Player;
            }
            if let Some(info) = universe.team_mut(TeamId::BLUE) {
                info.team_type = TeamKind::Computer;
            }
            // Unit attributes are taken from the team `current_values` (as in
            // `GameState::with_setup`), otherwise `hits`/`ammo` would be zero.
            if let Some(units) = universe.team_units_mut(TeamId::RED) {
                *units = TeamUnits::with_clan(TeamClan::Random);
                units.set_gold(1500);
            }
            if let Some(units) = universe.team_units_mut(TeamId::BLUE) {
                *units = TeamUnits::with_clan(TeamClan::Random);
            }

            let tank = universe
                .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
                .expect("tank spawned");
            let enemy = universe
                .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(14, 10), 0)
                .expect("enemy spawned");

            (tank, enemy)
        };

        // Camera on the middle between the units: both cells in the 512×384 frame.
        app.viewport_mut().center_on_grid(12, 10);

        (app, tank, enemy)
    }

    /// Screen point of the cell center.
    fn screen_of(app: &App, grid: Point) -> (i32, i32) {
        tile_center(app.viewport(), grid)
    }

    #[test]
    fn message_log_is_a_ring_buffer() {
        let mut log = MessageLog::with_capacity(3);

        for index in 0..5 {
            log.push(format!("line {index}"));
        }

        assert_eq!(log.capacity(), 3);
        assert_eq!(log.len(), 3);
        assert_eq!(log.lines(), ["line 2", "line 3", "line 4"]);

        log.clear();
        assert!(log.is_empty());
        // Capacity 0 is treated as 1 — the log does not lose the last line.
        let mut one = MessageLog::with_capacity(0);
        one.push("a");
        one.push("b");
        assert_eq!(one.lines(), ["b"]);
    }

    #[test]
    fn ground_cover_slab_under_a_building_does_not_steal_the_click() {
        // C++ `Access_GetTeamUnit` (`access.cpp:1804`) first looks for
        // `SELECTABLE` and not `GROUND_COVER`; the `LRGSLAB` slab lies under every
        // building, but the click must go to the building. The port earlier took the first
        // unit in `Hash_MapHash`, and after construction the slab was inserted before
        // the building — `LargeSlab` was selected.
        let (mut app, _tank, _enemy) = test_app();
        let grid = Point::new(11, 10);

        let slab = app.universe_mut()
            .spawn_unit(UnitType::LargeSlab, TeamId::RED, grid, 0)
            .expect("slab");
        let building = app.universe_mut()
            .spawn_unit(UnitType::TrainingHall, TeamId::RED, grid, 0)
            .expect("building");

        let (x, y) = screen_of(&app, grid);
        let picked = unit_at_screen(app.state_ref(), app.viewport(), TeamId::RED, x, y);

        assert_eq!(
            picked,
            Some(building),
            "клик должен выбрать здание, не плиту"
        );
        assert_ne!(picked, Some(slab));
    }

    #[test]
    fn hud_renders_on_empty_state_without_panics() {
        // Empty match: no units, no teams, a minimal frame and a normal one.
        for (width, height) in [(320, 240), (512, 384), (64, 48)] {
            let app = App::new(AppConfig::default().with_screen(width, height));
            let mut buffer = RgbaBuffer::new(width, height);

            render_hud(
                &mut buffer,
                &HudView {
                    state: app.state_ref(),
                    hud: app.hud(),
                    viewport: app.viewport(),
                    team: app.player_team(),
                    status: "empty",
                },
            );

            assert_eq!(buffer.width(), width);
            assert!(
                buffer.pixels().chunks_exact(4).any(|pixel| pixel[3] == 255),
                "HUD не нарисовал ни одного пикселя на {width}x{height}"
            );
        }
    }

    #[test]
    fn minimap_draws_surface_colors_and_own_units() {
        let (app, tank, _enemy) = test_app();
        let layout = hud_layout(512, 384);
        let mut buffer = RgbaBuffer::new(512, 384);

        draw_minimap(
            &mut buffer,
            layout.minimap,
            app.state_ref(),
            app.viewport(),
            app.player_team(),
            Some(tank),
        );

        // A minimap cell inside the map — the surface color (LAND), not transparent.
        let center = (
            layout.minimap.ulx + layout.minimap.width / 2,
            layout.minimap.uly + layout.minimap.height / 2,
        );
        let pixel = buffer.get(center.0, center.1).expect("pixel in minimap");

        assert_eq!(pixel.a, 255);
        assert_ne!(pixel, Rgba::TRANSPARENT);

        // There are pixels of the team color (the unit dot, the selected one — 3×3).
        let team_pixels = buffer
            .pixels()
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] == team_color(TeamId::RED).to_rgba()[..3])
            .count();

        assert!(
            team_pixels >= 9,
            "точка выделенного юнита должна быть видна на миникарте (найдено {team_pixels})"
        );
    }

    #[test]
    fn minimap_click_maps_to_a_tile_and_moves_the_camera() {
        let layout = hud_layout(512, 384);
        let point = (
            layout.minimap.ulx + layout.minimap.width / 2,
            layout.minimap.uly + layout.minimap.height / 2,
        );
        let grid = minimap_grid_at(&layout, point.0, point.1).expect("grid under minimap");

        assert!((50..=60).contains(&grid.x), "центр карты по x: {grid:?}");
        assert!((50..=60).contains(&grid.y), "центр карты по y: {grid:?}");
        // Outside the minimap — None.
        assert_eq!(minimap_grid_at(&layout, -5, -5), None);
    }

    #[test]
    fn hud_buttons_are_hit_testable() {
        let (width, height) = (512, 384);
        let layout = hud_layout(width, height);

        for button in HudButton::ALL {
            let rect = layout.button(button);
            let point = (rect.x + rect.width / 2, rect.y + rect.height / 2);

            assert_eq!(hud_button_at(width, height, point.0, point.1), Some(button));
        }

        // The buttons do not overlap and lie inside the frame.
        assert!(layout.end_turn.bottom() <= layout.menu.y);
        assert!(layout.menu.bottom() <= height as i32);
        // The map center is not a button.
        assert_eq!(hud_button_at(width, height, 100, 100), None);
    }

    #[test]
    fn click_selects_own_unit_and_enemy_click_orders_attack() {
        let (mut app, tank, enemy) = test_app();

        let (x, y) = screen_of(&app, Point::new(10, 10));
        let transition = handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);

        assert_eq!(transition, Transition::None);
        assert_eq!(app.hud().selected, Some(tank));
        assert!(app
            .hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("Selected")));

        // A click on an enemy with an own unit selected — an attack order (08).
        let (ex, ey) = screen_of(&app, Point::new(14, 10));
        handle_session_input(&mut app, InputEvent::MouseClick(ex, ey), false);

        // Demo mode applied the packet locally via `apply_command`.
        let unit = app.state_ref().unit(tank).expect("tank");

        assert_eq!(unit.orders, UnitOrder::MoveToAttack);
        assert_eq!(unit.enemy_unit, Some(enemy));
        assert_eq!(app.hud().selected, Some(tank));
        assert!(app.hud().pending.is_empty(), "демо-режим не копит приказы");
        // The path to the attack position will be requested by the unit itself on the tick
        // (`process_order_move` → `Paths_RequestPath`), packet 08 does not carry it.
        assert!(app.state_ref().path_requests().is_empty());
    }

    #[test]
    fn right_click_on_a_unit_cancels_the_selection() {
        let (mut app, tank, _enemy) = test_app();
        let (x, y) = screen_of(&app, Point::new(10, 10));

        handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);
        assert_eq!(app.hud().selected, Some(tank));

        // A right click on any unit — cancel (like Esc).
        handle_session_input(&mut app, InputEvent::RightClick(x, y), false);

        assert_eq!(app.hud().selected, None);
        assert_eq!(app.hud().preview, None);
        assert!(app
            .hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("Cancelled")));
    }

    #[test]
    fn path_preview_builds_a_route_and_draws_a_line() {
        let (app, tank, _enemy) = test_app();
        let destination = Point::new(13, 10);

        let order = build_move_order(app.state_ref(), tank, destination).expect("route");

        assert_eq!(order.preview.unit, tank);
        assert_eq!(order.preview.from, Point::new(10, 10));
        assert_eq!(order.preview.to, destination);
        assert_eq!(order.preview.len(), order.preview.steps.len());
        assert!(order.preview.len() >= 3, "3 клетки — минимум 3 шага");
        // The end of the actual route coincides with the target.
        assert_eq!(order.preview.last_point(), destination);

        // The line is drawn: before rendering the frame is transparent, after — there are pixels of the
        // preview color.
        let mut buffer = RgbaBuffer::new(512, 384);
        draw_path_preview(&mut buffer, app.viewport(), &order.preview);

        let painted = buffer
            .pixels()
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] == PREVIEW_COLOR.to_rgba()[..3])
            .count();

        assert!(
            painted > 50,
            "линия маршрута должна быть видна ({painted} px)"
        );
    }

    #[test]
    fn move_order_packet_38_matches_the_wire_layout() {
        let (app, tank, _enemy) = test_app();
        // Adjacent cell: a one-step path, the bytes are fully predictable.
        let order = build_move_order(app.state_ref(), tank, Point::new(11, 10)).expect("route");
        let bytes = Packet::Command(order.command.clone())
            .encode()
            .expect("38 encoded");

        let low = tank.0 as u8;
        let high = (tank.0 >> 8) as u8;
        let expected = [
            0x76, low, high, // type 0x26|APPL, unit_id
            0x02, 0x05, // order Move, state InProgress (the path is ready)
            0x0A, 0x00, 0x0A, 0x00, // grid (10,10)
            0x0B, 0x00, 0x0A, 0x00, // move_to (11,10)
            0x00, 0x00, // group_speed
            0x01, 0x00, // steps_count
            0x01, 0x00, // PathStep (1,0)
        ];

        assert_eq!(bytes, expected);

        // Decodes back into the same `Command` (checked against the wire encoder).
        let decoded = Packet::decode(&bytes).expect("38 decoded");

        assert_eq!(decoded, Packet::Command(order.command));
    }

    #[test]
    fn attack_order_packet_08_matches_the_wire_layout() {
        let (app, tank, enemy) = test_app();
        let command = build_attack_order(app.state_ref(), tank, enemy).expect("attack order");
        let bytes = Packet::Command(command.clone())
            .encode()
            .expect("08 encoded");

        let low = tank.0 as u8;
        let high = (tank.0 >> 8) as u8;
        let enemy_low = enemy.0 as u8;
        let enemy_high = (enemy.0 >> 8) as u8;
        let expected = [
            0x58, low, high, // type 0x08|APPL, unit_id
            // MoveToAttack/Init; prior — `Await`/`ExecutingOrder`: the new unit
            // is born in `ORDER_AWAIT`/`ORDER_STATE_EXECUTING_ORDER`
            // (`unitinfo.cpp:201-204`), and `SetNewOrderInt` saves it in prior
            // (`units_manager.cpp:1970-1971`).
            0x1E, 0x00, 0x00, 0x01, 0x00, // no disabled
            0x00, 0x00, 0x00, 0x00, // parent_unit, experience
            0x0E, 0x00, 0x0A, 0x00, // move_to (14,10) — the enemy cell
            0x0E, 0x00, 0x0A, 0x00, // fire_on (14,10)
            enemy_low, enemy_high, // enemy_unit
            0x00, 0x00, // transfer_cargo
            0x00, // stealth_dice_roll
        ];

        assert_eq!(bytes, expected);
        assert_eq!(
            Packet::decode(&bytes).expect("08 decoded"),
            Packet::Command(command)
        );
    }

    #[test]
    fn right_click_on_a_free_cell_queues_a_move_in_online_mode() {
        let (mut app, tank, _enemy) = test_app();
        let (x, y) = screen_of(&app, Point::new(10, 10));

        handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);

        let (mx, my) = screen_of(&app, Point::new(11, 10));
        handle_session_input(&mut app, InputEvent::RightClick(mx, my), true);

        assert_eq!(
            app.hud().pending.len(),
            1,
            "онлайн-режим копит приказ для сессии"
        );
        assert!(matches!(&app.hud().pending[0], Command::Move(_)));
        assert_eq!(
            app.hud().preview.as_ref().map(|preview| preview.to),
            Some(Point::new(11, 10))
        );
        // Locally the order is not applied yet — the network loop will do it.
        assert_eq!(
            app.state_ref().unit(tank).expect("tank").orders,
            UnitOrder::Await
        );

        // We take them: the session would send this to `SessionHandle`.
        let outgoing = app.take_outgoing_orders();
        assert_eq!(outgoing.len(), 1);
        assert!(app.hud().pending.is_empty());
    }

    #[test]
    fn demo_orders_apply_through_apply_command() {
        let (mut app, tank, _enemy) = test_app();
        let (x, y) = screen_of(&app, Point::new(10, 10));

        handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);

        let (mx, my) = screen_of(&app, Point::new(13, 10));
        handle_session_input(&mut app, InputEvent::RightClick(mx, my), false);

        let unit = app.state_ref().unit(tank).expect("tank");

        assert_eq!(unit.orders, UnitOrder::Move);
        assert_eq!(unit.state, UnitOrderState::InProgress);
        assert!(matches!(
            &unit.path,
            Some(max_core::unit::UnitPath::Ground(_))
        ));
        assert_eq!(unit.target_grid, Point::new(13, 10));
        assert_eq!(app.applied_commands(), 1);
        assert!(app
            .hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("Move")));
    }

    #[test]
    fn end_turn_closes_the_turn_like_the_network_timer() {
        let (mut app, _tank, _enemy) = test_app();
        let before = app.state_ref().turn_counter();

        let over = app.end_turn();

        // 06 applied (as `Remote_ReceiveNetPacket_06`), the turn is closed.
        assert_eq!(app.applied_commands(), 1);
        assert!(app
            .hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("End Turn")));

        if over {
            // `check_end_game_conditions` ended the match: `finish_turn` does not
            // reset the phase and does not advance the turn counter.
            assert_eq!(app.state_ref().game_state(), GameStateType::EndTurn);
            assert_eq!(app.state_ref().turn_counter(), before);
        } else {
            assert_eq!(app.state_ref().game_state(), GameStateType::InGame);
            assert_eq!(app.state_ref().turn_counter(), before + 1);
        }
    }

    #[test]
    fn resources_and_unit_panel_show_the_documented_fields() {
        let (app, tank, _enemy) = test_app();
        let line = resources_line(app.state_ref(), TeamId::RED);

        assert!(line.contains("GOLD 1500"), "{line}");
        assert!(line.contains("TEAM 0"), "{line}");

        let lines = unit_panel_lines(app.state_ref(), tank);

        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains("Tank"), "{lines:?}");
        assert!(lines[1].starts_with("HP "), "{lines:?}");
        assert!(lines[2].starts_with("AMMO "), "{lines:?}");
        assert!(lines[3].contains("ORDER Await"), "{lines:?}");
        assert!(lines[4].contains("GRID 10,10"), "{lines:?}");
    }

    /// The unit panel (bottom lines, not `STAT_WINDOW`) does not show Speed —
    /// neither for a tank nor for a building (in C++ `Speed` is not in this panel at all).
    #[test]
    fn unit_panel_lines_never_show_speed() {
        let (mut app, tank, _enemy) = test_app();
        let mine = app.universe_mut()
            .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(16, 10), 0)
            .expect("шахта создана");

        for (id, what) in [(tank, "танк"), (mine, "шахта")] {
            let lines = unit_panel_lines(app.state_ref(), id);

            assert!(
                lines
                    .iter()
                    .all(|line| !line.contains("SPEED") && !line.contains("Speed")),
                "{what}: {lines:?}"
            );
        }
    }

    #[test]
    fn escape_cancels_then_leaves_the_session() {
        let (mut app, tank, _enemy) = test_app();
        let (x, y) = screen_of(&app, Point::new(10, 10));

        handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);
        assert_eq!(app.hud().selected, Some(tank));

        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape), false),
            Transition::None
        );
        assert_eq!(app.hud().selected, None);
        // A second Esc — exit the session.
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape), false),
            Transition::LeaveSession
        );
    }

    #[test]
    fn selection_stays_out_of_the_snapshot() {
        let (mut app, tank, _enemy) = test_app();
        let before = app.snapshot();

        app.select_unit(Some(tank));
        app.hud_mut().push_log("hello");
        app.hud_mut().hover_button = Some(HudButton::EndTurn);

        assert_eq!(app.snapshot(), before, "HUD не входит в снапшот/CRC");
    }

    #[test]
    fn chat_input_is_ascii_only_with_backspace_and_a_limit() {
        let mut chat = ChatInput::new();

        assert!(!chat.push_char('A'), "закрытый ввод не принимает символы");
        chat.open();
        assert!(chat.is_active());
        assert!(chat.push_char('H'));
        assert!(chat.push_char('i'));
        assert!(chat.push_char(' '));
        assert!(!chat.push_char('Ж'), "только ASCII");
        assert!(!chat.push_char('\n'), "служебные символы не печатаются");
        assert_eq!(chat.message(), "Hi ");

        assert!(chat.backspace());
        assert_eq!(chat.message(), "Hi");

        for _ in 0..(CHAT_MAX_CHARS + 10) {
            chat.push_char('x');
        }
        assert_eq!(chat.message().chars().count(), CHAT_MAX_CHARS);

        chat.close();
        assert!(!chat.is_active());
        assert_eq!(chat.message(), "");

        // An empty Enter closes the input and returns nothing.
        chat.open();
        assert_eq!(chat.take_message(), None);
        assert!(!chat.is_active());
    }

    #[test]
    fn chat_overlay_draws_the_prompt_above_the_hud_bar() {
        let (mut app, _tank, _enemy) = test_app();
        let layout = hud_layout(512, 384);
        let rect = chat_input_rect(512, 384, &layout);

        assert!(!rect.is_empty());
        assert!(rect.uly >= 0 && rect.lry() <= 384, "{rect:?}");
        assert!(
            rect.uly <= layout.bar.uly,
            "строка ввода выше нижней панели"
        );

        let view = HudView {
            state: app.state_ref(),
            hud: app.hud(),
            viewport: app.viewport(),
            team: app.player_team(),
            status: "",
        };
        let mut closed = RgbaBuffer::new(512, 384);
        render_hud(&mut closed, &view);
        let before = closed.get(rect.ulx + 2, rect.uly + 2).expect("pixel");

        app.hud_mut().chat.open();
        app.hud_mut().chat.push_char('h');
        let view = HudView {
            state: app.state_ref(),
            hud: app.hud(),
            viewport: app.viewport(),
            team: app.player_team(),
            status: "",
        };
        let mut open = RgbaBuffer::new(512, 384);
        render_hud(&mut open, &view);
        let after = open.get(rect.ulx + 2, rect.uly + 2).expect("pixel");

        assert_eq!(after, HUD_PANEL, "панель ввода залита цветом HUD");
        assert_ne!(before, after, "активный ввод виден поверх HUD");
    }

    #[test]
    fn typing_a_chat_message_logs_and_applies_it_in_demo_mode() {
        let (mut app, _tank, _enemy) = test_app();

        // Enter opens the input line.
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Enter), false),
            Transition::None
        );
        assert!(app.hud().chat.is_active());

        for character in "Helo".chars() {
            handle_session_input(
                &mut app,
                InputEvent::Key(crate::ui::Key::Char(character)),
                false,
            );
        }
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Backspace), false);
        for character in "lo".chars() {
            handle_session_input(
                &mut app,
                InputEvent::Key(crate::ui::Key::Char(character)),
                false,
            );
        }
        assert_eq!(app.hud().chat.message(), "Hello");

        // Enter sends: in demo mode — immediately via apply_command.
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Enter), false),
            Transition::None
        );
        assert!(!app.hud().chat.is_active());
        assert_eq!(
            app.state_ref().chat_log(),
            vec![(TeamId::RED, "Hello".to_string())]
        );
        assert!(
            app.hud()
                .log
                .lines()
                .iter()
                .any(|line| line == "RED: Hello"),
            "журнал содержит отправленное: {:?}",
            app.hud().log.lines()
        );
        assert_eq!(app.applied_commands(), 1);
        assert!(app.hud().pending.is_empty(), "демо-режим не копит очередь");
    }

    #[test]
    fn online_chat_is_queued_as_packet_18_for_the_session() {
        let (mut app, _tank, _enemy) = test_app();

        // T also opens the input line.
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Char('T')), true);
        assert!(app.hud().chat.is_active());

        for character in "gg".chars() {
            handle_session_input(
                &mut app,
                InputEvent::Key(crate::ui::Key::Char(character)),
                true,
            );
        }
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Enter), true);

        assert!(
            app.state_ref().chat_log().is_empty(),
            "локальное применение делает net::flush_hud_orders"
        );

        let outgoing = app.take_outgoing_orders();

        assert_eq!(
            outgoing,
            vec![Command::Chat(Packet18Chat {
                sender: TeamId::RED,
                message: "gg".to_string(),
            })]
        );

        // The queue is exactly packet 18 on the wire.
        let bytes = Packet::Command(outgoing[0].clone())
            .encode()
            .expect("18 encoded");

        assert_eq!(bytes, [0x62, 0x00, 0x00, 0x03, 0x00, b'g', b'g', 0x00]);
    }

    #[test]
    fn escape_cancels_chat_without_leaving_the_session() {
        let (mut app, _tank, _enemy) = test_app();

        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Char('t')), false);
        for character in "no".chars() {
            handle_session_input(
                &mut app,
                InputEvent::Key(crate::ui::Key::Char(character)),
                false,
            );
        }

        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape), false),
            Transition::None,
            "Esc в чате не выходит из сессии"
        );
        assert!(!app.hud().chat.is_active());
        assert!(app.state_ref().chat_log().is_empty());
        assert!(app
            .hud()
            .log
            .lines()
            .iter()
            .any(|line| line == "Chat cancelled"));

        // The next Esc — the usual session behavior already.
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape), false),
            Transition::LeaveSession
        );
    }
}
