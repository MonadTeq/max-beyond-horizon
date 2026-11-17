//! HUD (M4.2): миникарта, панель выбранного юнита, журнал сообщений, полоса
//! ресурсов и кнопки **поверх** кадра карты.
//!
//! Всё здесь — чистые функции от состояния (`max_core::game::GameState`) и
//! небольшого [`HudState`], который принадлежит [`App`] и **не входит** в
//! снапшот/CRC: выделенный юнит, предпросмотр пути, журнал и исходящие
//! приказы — это UI, а не симуляция (`AppDigest` их не видит, детерминизм
//! тика не меняется).
//!
//! # Что рисуется
//!
//! | Элемент | Источник | Где |
//! |---|---|---|
//! | миникарта 112×112 → маленький буфер | [`SurfaceMap`], юниты своей команды | [`draw_minimap`] |
//! | панель юнита (имя/тип, HP, ammo, приказ) | [`UnitInfo`] + `base_values` | [`unit_panel_lines`] |
//! | журнал сообщений (кольцевой буфер) | [`MessageLog`] | [`draw_log`] |
//! | строка ввода чата (пакет 18) | [`ChatInput`] | [`draw_chat_input`] |
//! | полоса ресурсов команды | [`TeamUnits`] (gold + комплексы) | [`resources_line`] |
//! | кнопки End Turn / Menu | `max_render::ui::Button` | [`hud_ui`] |
//! | предпросмотр пути | [`PathPreview`] (линия по клеткам) | [`draw_path_preview`] |
//!
//! [`SurfaceMap`]: max_core::map::SurfaceMap
//! [`UnitInfo`]: max_core::unit::UnitInfo
//! [`TeamUnits`]: max_core::team::TeamUnits
//!
//! # Приказы
//!
//! [`build_move_order`] собирает пакет **38** (`Command::Move`, путь
//! `GroundPath`) через `max_core::pathfinding::find_ground_path` — тот же
//! поиск, что и `Paths_RequestPath` у получателя; [`build_attack_order`] —
//! пакет **08** с процессором `P3` (`ORDER_MOVE_TO_ATTACK` + цель). Оба
//! возвращают готовые [`Command`], которые:
//!
//! * в демо-режиме (без сети) применяются локально — [`App::flush_orders`]
//!   вызывает `max_core::events::apply_command`;
//! * в сетевой сессии уходят в `SessionHandle` (`net::flush_hud_orders`):
//!   сначала локальное применение (как в C++ перед multicast), затем
//!   `Packet::Command` в сессию — энкодеры `max_net::wire::command`.

use max_core::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS;
use max_core::enums::{unit_flags, GameStateType, Surface, UnitOrder, UnitOrderState};
use max_core::events::command_finished_turn;
use max_core::game::GameState;
use max_core::geo::{PathStep, Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::input::{
    Command, MovePathData, OrderPayload, Packet08Order, Packet38Move, PayloadP1, PayloadP2,
    PayloadP3,
};
use max_core::pathfinding::find_ground_path;
use max_core::team::TeamId;
use max_core::unit::{UnitId, UnitInfo};
use max_core::unit_values::{ATTRIB_AMMO, ATTRIB_HITS};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::map_render::TileStyle;
use max_render::text::{draw_text_line, GLYPH_HEIGHT, GLYPH_WIDTH};
use max_render::ui::{render_ui, Button, Rect, UiState, UiTheme, Widget};
use max_render::viewport::Viewport;

use crate::App;

/// Ёмкость журнала сообщений по умолчанию (строк).
pub const MESSAGE_LOG_CAPACITY: usize = 8;

/// Внутренний отступ панелей HUD, пиксели.
pub const HUD_MARGIN: i32 = 4;
/// Высота полосы ресурсов сверху.
pub const HUD_TOP_BAR_HEIGHT: i32 = 14;
/// Высота строки статуса внизу.
pub const HUD_STATUS_HEIGHT: i32 = 12;
/// Максимальная сторона миникарты: 112 клеток по 1 пикселю.
pub const MINIMAP_MAX_SIDE: i32 = 112;
/// Ширина кнопки HUD.
pub const HUD_BUTTON_WIDTH: i32 = 96;
/// Высота кнопки HUD.
pub const HUD_BUTTON_HEIGHT: i32 = 20;
/// Зазор между кнопками.
pub const HUD_BUTTON_GAP: i32 = 4;

/// Предел шагов пути в пакете 38 (`REMOTE_MAX_VECTOR_LENGTH` в `max_net`).
///
/// Длиннее — `WireError::VectorTooLong` при кодировании, поэтому заказ
/// отклоняется ещё в HUD.
pub const MAX_PATH_STEPS: usize = 1024;

/// Цвета команд (порядок оригинала: `RED, GREEN, BLUE, GRAY, ALIEN`).
pub const TEAM_COLORS: [Rgba; 5] = [
    Rgba::rgb(224, 48, 48),
    Rgba::rgb(48, 192, 72),
    Rgba::rgb(64, 112, 224),
    Rgba::rgb(192, 192, 192),
    Rgba::rgb(212, 168, 64),
];

/// Цвет предпросмотра пути (не пересекается с цветами панелей и команд).
pub const PREVIEW_COLOR: Rgba = Rgba::rgb(120, 255, 152);
/// Фон панелей HUD.
pub const HUD_PANEL: Rgba = Rgba::rgb(14, 18, 26);
/// Рамка панелей HUD.
pub const HUD_BORDER: Rgba = Rgba::rgb(96, 116, 148);
/// Основной текст HUD.
pub const HUD_TEXT: Rgba = Rgba::rgb(232, 236, 244);
/// Приглушённый текст HUD.
pub const HUD_TEXT_DIM: Rgba = Rgba::rgb(148, 156, 168);

/// Цвет команды (индекс клампится, `ALIEN` — для всего не-игрового).
pub fn team_color(team: TeamId) -> Rgba {
    TEAM_COLORS
        .get(usize::from(team.0))
        .copied()
        .unwrap_or(TEAM_COLORS[TEAM_COLORS.len() - 1])
}

// ---------------------------------------------------------------------------
// Журнал сообщений
// ---------------------------------------------------------------------------

/// Кольцевой буфер строк журнала: старые записи вытесняются новыми.
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
    /// Журнал стандартной ёмкости ([`MESSAGE_LOG_CAPACITY`]).
    pub fn new() -> Self {
        Self::with_capacity(MESSAGE_LOG_CAPACITY)
    }

    /// Журнал ёмкости `capacity` (минимум 1).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            lines: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    /// Добавляет строку, вытесняя самую старую при переполнении.
    pub fn push(&mut self, line: impl Into<String>) {
        if self.lines.len() >= self.capacity {
            self.lines.remove(0);
        }
        self.lines.push(line.into());
    }

    /// Очищает журнал.
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Строки от старой к новой.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Число строк.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Журнал пуст.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Ёмкость журнала.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ---------------------------------------------------------------------------
// Чат (пакет 18)
// ---------------------------------------------------------------------------

/// Максимальная длина сообщения чата в символах.
///
/// В C++ буфер строки ввода — `char chat_input_buffer[150]`
/// (`networkmenu.hpp:171`), поэтому 149 символов + NUL всегда помещаются в
/// `SmartString` пакета 18 (`remote.cpp:2656-2685`).
pub const CHAT_MAX_CHARS: usize = 149;

/// Имена команд по умолчанию (порядок оригинала: `RED, GREEN, BLUE, GRAY`).
pub const TEAM_NAMES: [&str; 5] = ["RED", "GREEN", "BLUE", "GRAY", "ALIEN"];

/// Имя команды для журнала/чата (`Remote_ReceiveNetPacket_18` берёт имя из
/// настроек игры; в Rust-состоянии имён нет, поэтому используются имена слотов
/// по умолчанию).
pub fn team_name(team: TeamId) -> &'static str {
    TEAM_NAMES
        .get(usize::from(team.0))
        .copied()
        .unwrap_or(TEAM_NAMES[TEAM_NAMES.len() - 1])
}

/// Строка журнала для сообщения чата: `<имя команды>: <текст>`.
///
/// Ровно тот формат, что собирает `Remote_ReceiveNetPacket_18`:
/// `message = team_name; message += ": "; message += text`.
pub fn chat_line(sender: TeamId, message: &str) -> String {
    format!("{}: {message}", team_name(sender))
}

/// Состояние строки ввода чата (M4.4).
///
/// Живёт в [`HudState`] и, как весь HUD, **не входит** в снапшот/CRC: набор
/// текста не должен менять детерминизм симуляции. Пока [`Self::is_active`],
/// `ui::handle_session_input` отдаёт клавиатуру чату, а не приказам.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatInput {
    /// Строка ввода открыта (Enter/T в сессии).
    active: bool,
    /// Набранный ASCII-текст (до [`CHAT_MAX_CHARS`] символов).
    buffer: String,
}

impl ChatInput {
    /// Закрытая пустая строка ввода.
    pub const fn new() -> Self {
        Self {
            active: false,
            buffer: String::new(),
        }
    }

    /// Открывает ввод с пустым буфером (Enter/T).
    pub fn open(&mut self) {
        self.active = true;
        self.buffer.clear();
    }

    /// Закрывает ввод, отбрасывая набранное (Esc/отправка).
    pub fn close(&mut self) {
        self.active = false;
        self.buffer.clear();
    }

    /// Строка ввода открыта.
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Набранный текст.
    pub fn message(&self) -> &str {
        &self.buffer
    }

    /// Добавляет печатный ASCII-символ; `false` — символ отклонён
    /// (не ASCII/буфер полон/ввод закрыт).
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

    /// Удаляет последний символ (Backspace); `false` — удалять нечего.
    pub fn backspace(&mut self) -> bool {
        self.active && self.buffer.pop().is_some()
    }

    /// Забирает сообщение и закрывает ввод.
    ///
    /// `None` — буфер пуст: C++ `Remote_SendNetPacket_18` такие сообщения не
    /// отправляет, поэтому Enter по пустой строке просто закрывает ввод.
    pub fn take_message(&mut self) -> Option<String> {
        self.active = false;
        let message = std::mem::take(&mut self.buffer);

        (!message.is_empty()).then_some(message)
    }
}

// ---------------------------------------------------------------------------
// Предпросмотр пути
// ---------------------------------------------------------------------------

/// Предпросмотр маршрута выбранного юнита: линия по клеткам.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPreview {
    /// Юнит, для которого построен маршрут.
    pub unit: UnitId,
    /// Стартовая клетка.
    pub from: Point,
    /// Клетка назначения (совпадает с `move_to` пакета 38).
    pub to: Point,
    /// Шаги пути в порядке следования (`PathStep`, как в `GroundPath`).
    pub steps: Vec<PathStep>,
}

impl PathPreview {
    /// Предпросмотр из готового списка шагов.
    pub fn new(unit: UnitId, from: Point, to: Point, steps: Vec<PathStep>) -> Self {
        Self {
            unit,
            from,
            to,
            steps,
        }
    }

    /// Клетки маршрута: старт и каждая следующая после шага (длина `steps + 1`).
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

    /// Длина маршрута в шагах.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Маршрут без шагов (юнит уже на месте).
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Конечная клетка фактического маршрута (сумма шагов), а не `to`.
    pub fn last_point(&self) -> Point {
        self.points().pop().unwrap_or(self.from)
    }
}

// ---------------------------------------------------------------------------
// Состояние HUD (не входит в снапшот/CRC)
// ---------------------------------------------------------------------------

/// Кнопка HUD под курсором/нажатием.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HudButton {
    /// End Turn: тот же путь, что и таймер (`06` + `52`/`finish_turn`).
    EndTurn,
    /// Menu: выход из сессии в главное меню.
    Menu,
}

impl HudButton {
    /// Все кнопки HUD в порядке индексов [`hud_ui`].
    pub const ALL: [Self; 2] = [Self::EndTurn, Self::Menu];

    /// Индекс кнопки в [`hud_ui`].
    pub const fn index(self) -> usize {
        match self {
            Self::EndTurn => 0,
            Self::Menu => 1,
        }
    }

    /// Подпись кнопки.
    pub const fn label(self) -> &'static str {
        match self {
            Self::EndTurn => "End Turn",
            Self::Menu => "Menu",
        }
    }
}

/// UI-состояние сессии: выделение, журнал, предпросмотр и исходящие приказы.
///
/// Хранится в [`App`], но **не входит** в `GameState::snapshot_bytes` и
/// `AppDigest`: выделенный юнит и журнал — это UI, они не влияют на тик и
/// CRC-барьер 45.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudState {
    /// Выбранный юнит (может устареть — проверяйте [`App::selected_unit`]).
    pub selected: Option<UnitId>,
    /// Предпросмотр маршрута (последний `RMB`-приказ).
    pub preview: Option<PathPreview>,
    /// Журнал сообщений (кольцевой буфер).
    pub log: MessageLog,
    /// Строка ввода чата (пакет 18): активна, пока игрок набирает сообщение.
    pub chat: ChatInput,
    /// Приказы, ещё не переданные сети/симуляции (38/08/18 из ввода).
    pub pending: Vec<Command>,
    /// Кнопка HUD под курсором.
    pub hover_button: Option<HudButton>,
    /// Кнопка панелей 1:1 под курсором (когда активен хром).
    pub hover_panel: Option<crate::panels::PanelButton>,
    /// Локальное состояние панелей окна 1:1: переключатели, таймер, курсор.
    pub panels: crate::panels::PanelState,
    /// Нажата End Turn в сетевой сессии (закрытие хода делает игровой цикл).
    pub end_turn_requested: bool,
    /// Позиция курсора в пикселях кадра (модальные экраны: `BuildMenu`).
    pub mouse_position: Option<(i32, i32)>,
}

impl Default for HudState {
    fn default() -> Self {
        Self::new()
    }
}

impl HudState {
    /// Пустое состояние HUD.
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
        }
    }

    /// Сбрасывает всё (новая партия/загрузка сейва).
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Выделяет юнита (`None` — снять выделение).
    pub fn select(&mut self, id: Option<UnitId>) {
        self.selected = id;
    }

    /// Пишет строку в журнал.
    pub fn push_log(&mut self, line: impl Into<String>) {
        self.log.push(line);
    }

    /// Снимает выделение и предпросмотр (Esc/правый клик по юниту).
    pub fn clear_interaction(&mut self) {
        self.selected = None;
        self.preview = None;
    }

    /// Просит закрыть ход (кнопка End Turn в сетевой сессии).
    pub fn request_end_turn(&mut self) {
        self.end_turn_requested = true;
    }

    /// Забирает флаг «End Turn нажат».
    pub fn take_end_turn_request(&mut self) -> bool {
        std::mem::take(&mut self.end_turn_requested)
    }
}

// ---------------------------------------------------------------------------
// Раскладка HUD
// ---------------------------------------------------------------------------

/// Прямоугольники HUD в пикселях кадра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudLayout {
    /// Полоса ресурсов сверху.
    pub top_bar: PixelRect,
    /// Панель журнала сообщений (под полосой ресурсов).
    pub log_panel: PixelRect,
    /// Нижняя панель целиком.
    pub bar: PixelRect,
    /// Миникарта.
    pub minimap: PixelRect,
    /// Панель выбранного юнита.
    pub unit_panel: PixelRect,
    /// Строка статуса внизу кадра.
    pub status: PixelRect,
    /// Кнопка End Turn.
    pub end_turn: Rect,
    /// Кнопка Menu.
    pub menu: Rect,
}

impl HudLayout {
    /// Прямоугольник кнопки.
    pub fn button(self, button: HudButton) -> Rect {
        match button {
            HudButton::EndTurn => self.end_turn,
            HudButton::Menu => self.menu,
        }
    }
}

/// Раскладка HUD для кадра `width × height`.
///
/// Панели не перекрываются и не выходят за кадр; на маленьких экранах
/// (например 64×48) элементы вырождаются в пустые прямоугольники, а
/// отрисовка их пропускает — HUD никогда не паникует.
pub fn hud_layout(width: u32, height: u32) -> HudLayout {
    let w = width.max(1) as i32;
    let h = height.max(1) as i32;
    let margin = HUD_MARGIN;

    let top_bar = PixelRect::new(0, 0, w, HUD_TOP_BAR_HEIGHT.min(h));

    // Нижняя панель: четверть кадра, но не меньше 72 px (и не больше кадра).
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

    // Миникарта: квадрат, вписанный между верхом панели и строкой статуса.
    let inner_top = bar_y + margin;
    let inner_bottom = (h - status_height - margin).max(inner_top);
    let side = (inner_bottom - inner_top)
        .clamp(1, MINIMAP_MAX_SIDE)
        .min((w / 3).max(1));
    let minimap = PixelRect::new(margin, inner_top, side, side);

    // Кнопки справа: End Turn и Menu столбиком (не шире трети кадра, чтобы не
    // налезать на миникарту на маленьких экранах).
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

    // Панель юнита — между миникартой и кнопками.
    let unit_x = minimap.lrx() + margin;
    let unit_width = (buttons_x - margin - unit_x).max(0);
    let unit_panel = PixelRect::new(unit_x, inner_top, unit_width, side);

    // Журнал: над нижней панелью слева, высотой под ёмкость буфера.
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

/// Кнопка HUD под точкой экрана (учёт наложения — кнопки выше панелей).
pub fn hud_button_at(width: u32, height: u32, x: i32, y: i32) -> Option<HudButton> {
    let layout = hud_layout(width, height);

    HudButton::ALL.into_iter().find(|button| {
        layout
            .button(*button)
            .contains(max_render::ui::Point::new(x, y))
    })
}

/// Клетка миникарты под точкой экрана (клик по миникарте двигает камеру).
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
// Приказы из HUD
// ---------------------------------------------------------------------------

/// Готовый приказ движения: пакет 38 + предпросмотр маршрута.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOrder {
    /// `Command::Move` (38) для сессии/симуляции.
    pub command: Command,
    /// Линия маршрута для HUD.
    pub preview: PathPreview,
}

/// Строит приказ движения (пакет **38**) для `id` в клетку `destination`.
///
/// Путь считается [`find_ground_path`] с тем же модификатором доступа, что и
/// `PathRequest` получателя (`ACCESS_MODIFIER_SAME_CLASS_BLOCKS`), поэтому
/// маршрут совпадает с тем, что построил бы `Paths_RequestPath`. Состояние
/// приказа — `InProgress`: путь уже готов, получателю не нужно запрашивать
/// его повторно (`apply_packet_38` не ставит запрос для `InProgress`).
///
/// `None` — юнита нет, он уничтожен, уже стоит в целевой клетке, путь не
/// найден или длиннее [`MAX_PATH_STEPS`].
pub fn build_move_order(state: &GameState, id: UnitId, destination: Point) -> Option<MoveOrder> {
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

    let path = find_ground_path(
        state,
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

/// Строит приказ атаки (пакет **08**, процессор `P3`) по врагу `target`.
///
/// `P3 ⊃ P2 ⊃ P1`: новый приказ `ORDER_MOVE_TO_ATTACK`, состояние `Init`,
/// цель в `move_to`/`fire_on`/`enemy_unit`. Путь к позиции атаки строит
/// получатель (`Paths_RequestPath` в `process_order_move`) — как в C++.
pub fn build_attack_order(state: &GameState, id: UnitId, target: UnitId) -> Option<Command> {
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

/// Строит приказ включения/выключения питания (попап `Power On`/`Power Off`
/// в C++, пакет **08**, процессор `P1`).
///
/// Для `RESEARCH` включение питания запускает исследование темы по умолчанию
/// (C++ `ResearchMenu_UpdateResearchProgress(team, topic, +1)` в
/// `UnitsManager_ProcessOrderPowerOn`).
pub fn build_power_order(state: &GameState, id: UnitId) -> Option<Command> {
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

/// Юнит под точкой экрана: `viewport.screen_to_grid` + `GameState::units_at`.
///
/// Предпочитает юнита, видимого команде `team` (туман войны), иначе — любого
/// в клетке (сначала свои). Точка вне карты — `None`.
pub fn unit_at_screen(
    state: &GameState,
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

    let visible = ids.iter().copied().find(|id| {
        state
            .unit(*id)
            .is_some_and(|unit| unit.is_visible_to_team(team))
    });

    visible.or_else(|| ids.iter().copied().find(|id| state.unit(*id).is_some()))
}

/// Может ли юнит получить приказ движения от HUD.
///
/// Наземные и морские — да (маршрут `find_ground_path`). Воздушным нужен
/// `Paths_GetAirPath` с пиксельной геометрией (`MovePathData::Air`), поэтому
/// HUD их пока не двигает (см. README, «Ограничения»).
pub fn unit_can_move(unit: &UnitInfo) -> bool {
    unit.flags & (unit_flags::MOBILE_LAND_UNIT | unit_flags::MOBILE_SEA_UNIT) != 0
        && unit.flags & unit_flags::MOBILE_AIR_UNIT == 0
}

// ---------------------------------------------------------------------------
// Методы App: выделение, приказы, End Turn
// ---------------------------------------------------------------------------

impl App {
    /// Состояние HUD (выделение, журнал, предпросмотр).
    pub fn hud(&self) -> &HudState {
        &self.hud
    }

    /// Изменяемое состояние HUD.
    pub fn hud_mut(&mut self) -> &mut HudState {
        &mut self.hud
    }

    /// Команда локального игрока (`player_team` партии).
    pub fn player_team(&self) -> TeamId {
        self.state().player_team
    }

    /// Выбранный юнит, если он ещё существует.
    pub fn selected_unit(&self) -> Option<&UnitInfo> {
        self.hud.selected.and_then(|id| self.state().unit(id))
    }

    /// Выделяет юнита (устаревший id игнорируется при чтении).
    pub fn select_unit(&mut self, id: Option<UnitId>) {
        self.hud.select(id);
        // Портрет `WINDOW_CORNER_FLIC`: у нового юнита сразу кадр 0 его FLC
        // (`GameManager_MenuUnitSelect` → `MenuCreateFlic`).
        self.refresh_portrait();
    }

    /// Забирает накопленные HUD-приказы для отправки в сессию.
    pub fn take_outgoing_orders(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.hud.pending)
    }

    /// Применяет HUD-приказы локально (демо-режим без сети).
    ///
    /// Возвращает число применённых команд. Каждая проходит через
    /// `max_core::events::apply_command` — ровно как пакет из сети.
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

    /// End Turn: тот же путь, что и таймер в сетевом цикле (`06` + `52`).
    ///
    /// * пакет **06** (`Command::FinishedTurn`) — команда закончила ход;
    /// * фаза `EndTurn` (в `TurnBased` её выставляет сам 06) и
    ///   [`GameState::finish_turn`] — аналог закрытия 52-барьера.
    ///
    /// `true` — партия завершена (`check_end_game_conditions`).
    pub fn end_turn(&mut self) -> bool {
        let team = self.state().player_team;
        let _ = self.apply(command_finished_turn(team));

        if self.state().game_state != GameStateType::EndTurn {
            self.state_mut().game_state = GameStateType::EndTurn;
        }

        let over = self.state_mut().finish_turn();
        let line = if over {
            "End Turn: game over".to_string()
        } else {
            format!("End Turn: turn {} begins", self.state().turn_counter)
        };

        self.hud.push_log(line);
        over
    }
}

// ---------------------------------------------------------------------------
// Отрисовка
// ---------------------------------------------------------------------------

/// Всё, что нужно [`render_hud`]: состояние партии + UI-состояние.
#[derive(Clone, Copy)]
pub struct HudView<'a> {
    /// Состояние партии (юниты, карта, ресурсы).
    pub state: &'a GameState,
    /// Выделение, журнал, предпросмотр, кнопки.
    pub hud: &'a HudState,
    /// Камера: миникарта/линия пути в экранных координатах.
    pub viewport: &'a Viewport,
    /// Команда локального игрока.
    pub team: TeamId,
    /// Строка статуса (сессия/подключение/ошибка).
    pub status: &'a str,
}

/// Рисует HUD поверх готового кадра карты.
///
/// Порядок: предпросмотр пути → полоса ресурсов → журнал → нижняя панель
/// (миникарта, панель юнита, кнопки) → строка статуса. Ничего не паникует:
/// все прямоугольники клампятся по кадру, вне кадра `RgbaBuffer::set`
/// возвращает `false`.
pub fn render_hud(buffer: &mut RgbaBuffer, view: &HudView<'_>) {
    let layout = hud_layout(buffer.width(), buffer.height());

    if let Some(preview) = &view.hud.preview {
        draw_path_preview(buffer, view.viewport, preview);
    }

    // Полоса ресурсов.
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

    // Нижняя панель.
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

/// Прямоугольник строки ввода чата: полоса над нижней панелью HUD.
///
/// Отдельная функция, чтобы тесты и отрисовка видели один и тот же rect.
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

/// Строка ввода чата поверх HUD: `Say: <текст>_`.
///
/// Рисуется последней, поэтому не перекрывается журналом/кнопками; пока ввод
/// закрыт, не занимает ни одного пикселя.
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

/// UI-состояние кнопок HUD (End Turn / Menu) для [`render_ui`].
///
/// Тема — с прозрачным фоном: `render_ui` не заливает кадр и рисует только
/// сами кнопки поверх карты.
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

/// Строка ресурсов команды: gold + суммарные запасы комплексов.
pub fn resources_line(state: &GameState, team: TeamId) -> String {
    let index = usize::from(team.0);
    let units = state.team_units.get(index);
    let gold = units.map(|units| units.gold()).unwrap_or(0);
    let mut raw = 0i32;
    let mut fuel = 0i32;
    let mut complex_gold = 0i32;
    let mut power = 0i32;
    let mut workers = 0i32;
    let mut buildings = 0i32;

    if let Some(units) = units {
        for complex in units.complexes() {
            raw += i32::from(complex.material);
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

/// Строки панели выбранного юнита: имя/тип, HP, ammo, приказ, клетка.
pub fn unit_panel_lines(state: &GameState, id: UnitId) -> Vec<String> {
    let Some(unit) = state.unit(id) else {
        return vec!["(unit gone)".to_string()];
    };

    let name = if unit.name.is_empty() {
        format!("{:?}", unit.unit_type)
    } else {
        unit.name.clone()
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

/// Цвет клетки миникарты по её поверхности.
pub fn surface_color(surface: Surface) -> Rgba {
    TileStyle::default().base_color(surface).darken(5)
}

/// Рисует миникарту 112×112 → `rect`: поверхность, камера, юниты своей команды.
///
/// Клетки сэмплируются (`px * 112 / width`), поэтому миникарта работает при
/// любом размере панели, а не только 112×112.
pub fn draw_minimap(
    buffer: &mut RgbaBuffer,
    rect: PixelRect,
    state: &GameState,
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
            let color = surface_color(state.map.surface.get(tile_x, tile_y));

            buffer.set(rect.ulx + px, rect.uly + py, color);
        }
    }

    // Рамка видимой области камеры.
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

    // Юниты своей команды (и выделенный — крупнее и в цвет команды).
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

/// Рисует предпросмотр маршрута: линия по центрам клеток + маркер назначения.
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

/// Центр клетки на экране (в пикселях кадра).
pub fn tile_center(viewport: &Viewport, grid: Point) -> (i32, i32) {
    let rect = viewport.tile_rect(grid.x, grid.y);

    (rect.ulx + rect.width / 2, rect.uly + rect.height / 2)
}

/// Линия Брезенхема (включительно) цветом `color`; вне кадра — игнорируется.
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

/// Рамка 1 px по краям прямоугольника.
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

/// Строка текста, обрезанная по ширине `max_width` (шрифт моноширинный).
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

/// Панель журнала: последние строки, влезающие в прямоугольник.
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

/// Панель выбранного юнита (или подсказка по управлению без выделения).
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

    /// Приложение с танком RED (10,10), танком BLUE (14,10) и золотом команды.
    fn test_app() -> (App, UnitId, UnitId) {
        let mut app = App::new(AppConfig::default().with_screen(512, 384));

        let (tank, enemy) = {
            let state = app.state_mut();
            state.player_team = TeamId::RED;
            state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
            state.teams[TeamId::BLUE.0 as usize].team_type = TeamKind::Computer;
            // Атрибуты юнитов берутся из `current_values` команды (как в
            // `GameState::with_setup`), иначе `hits`/`ammo` были бы нулевыми.
            state.team_units[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
            state.team_units[TeamId::BLUE.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
            state.team_units[TeamId::RED.0 as usize].set_gold(1500);

            let tank = state
                .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
                .expect("tank spawned");
            let enemy = state
                .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(14, 10), 0)
                .expect("enemy spawned");

            (tank, enemy)
        };

        // Камера на середину между юнитами: обе клетки в кадре 512×384.
        app.viewport_mut().center_on_grid(12, 10);

        (app, tank, enemy)
    }

    /// Экранная точка центра клетки.
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
        // Ёмкость 0 трактуется как 1 — журнал не теряет последнюю строку.
        let mut one = MessageLog::with_capacity(0);
        one.push("a");
        one.push("b");
        assert_eq!(one.lines(), ["b"]);
    }

    #[test]
    fn hud_renders_on_empty_state_without_panics() {
        // Пустая партия: ни юнитов, ни команд, минимальный кадр и обычный.
        for (width, height) in [(320, 240), (512, 384), (64, 48)] {
            let app = App::new(AppConfig::default().with_screen(width, height));
            let mut buffer = RgbaBuffer::new(width, height);

            render_hud(
                &mut buffer,
                &HudView {
                    state: app.state(),
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
            app.state(),
            app.viewport(),
            app.player_team(),
            Some(tank),
        );

        // Клетка миникарты внутри карты — цвет поверхности (LAND), не прозрачная.
        let center = (
            layout.minimap.ulx + layout.minimap.width / 2,
            layout.minimap.uly + layout.minimap.height / 2,
        );
        let pixel = buffer.get(center.0, center.1).expect("pixel in minimap");

        assert_eq!(pixel.a, 255);
        assert_ne!(pixel, Rgba::TRANSPARENT);

        // Есть пиксели цвета команды (точка юнита, выделенный — 3×3).
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
        // Вне миникарты — None.
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

        // Кнопки не перекрываются и лежат в кадре.
        assert!(layout.end_turn.bottom() <= layout.menu.y);
        assert!(layout.menu.bottom() <= height as i32);
        // Центр карты — не кнопка.
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

        // Клик по врагу при выделенном своём юните — приказ атаки (08).
        let (ex, ey) = screen_of(&app, Point::new(14, 10));
        handle_session_input(&mut app, InputEvent::MouseClick(ex, ey), false);

        // Демо-режим применил пакет локально через `apply_command`.
        let unit = app.state().unit(tank).expect("tank");

        assert_eq!(unit.orders, UnitOrder::MoveToAttack);
        assert_eq!(unit.enemy_unit, Some(enemy));
        assert_eq!(app.hud().selected, Some(tank));
        assert!(app.hud().pending.is_empty(), "демо-режим не копит приказы");
        // Путь к позиции атаки запросит сам юнит на тике
        // (`process_order_move` → `Paths_RequestPath`), пакет 08 его не несёт.
        assert!(app.state().path_requests.is_empty());
    }

    #[test]
    fn right_click_on_a_unit_cancels_the_selection() {
        let (mut app, tank, _enemy) = test_app();
        let (x, y) = screen_of(&app, Point::new(10, 10));

        handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);
        assert_eq!(app.hud().selected, Some(tank));

        // Правый клик по любому юниту — отмена (как Esc).
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

        let order = build_move_order(app.state(), tank, destination).expect("route");

        assert_eq!(order.preview.unit, tank);
        assert_eq!(order.preview.from, Point::new(10, 10));
        assert_eq!(order.preview.to, destination);
        assert_eq!(order.preview.len(), order.preview.steps.len());
        assert!(order.preview.len() >= 3, "3 клетки — минимум 3 шага");
        // Конец фактического маршрута совпадает с целью.
        assert_eq!(order.preview.last_point(), destination);

        // Линия рисуется: до рендера кадр прозрачный, после — есть пиксели цвета
        // предпросмотра.
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
        // Соседняя клетка: путь из одного шага, байты полностью предсказуемы.
        let order = build_move_order(app.state(), tank, Point::new(11, 10)).expect("route");
        let bytes = Packet::Command(order.command.clone())
            .encode()
            .expect("38 encoded");

        let low = tank.0 as u8;
        let high = (tank.0 >> 8) as u8;
        let expected = [
            0x76, low, high, // type 0x26|APPL, unit_id
            0x02, 0x05, // order Move, state InProgress (путь готов)
            0x0A, 0x00, 0x0A, 0x00, // grid (10,10)
            0x0B, 0x00, 0x0A, 0x00, // move_to (11,10)
            0x00, 0x00, // group_speed
            0x01, 0x00, // steps_count
            0x01, 0x00, // PathStep (1,0)
        ];

        assert_eq!(bytes, expected);

        // Обратно декодируется в тот же `Command` (сверка с wire-энкодером).
        let decoded = Packet::decode(&bytes).expect("38 decoded");

        assert_eq!(decoded, Packet::Command(order.command));
    }

    #[test]
    fn attack_order_packet_08_matches_the_wire_layout() {
        let (app, tank, enemy) = test_app();
        let command = build_attack_order(app.state(), tank, enemy).expect("attack order");
        let bytes = Packet::Command(command.clone())
            .encode()
            .expect("08 encoded");

        let low = tank.0 as u8;
        let high = (tank.0 >> 8) as u8;
        let enemy_low = enemy.0 as u8;
        let enemy_high = (enemy.0 >> 8) as u8;
        let expected = [
            0x58, low, high, // type 0x08|APPL, unit_id
            0x1E, 0x00, 0x00, 0x00, 0x00, // MoveToAttack, Init, prior Await/Init, no disabled
            0x00, 0x00, 0x00, 0x00, // parent_unit, experience
            0x0E, 0x00, 0x0A, 0x00, // move_to (14,10) — клетка врага
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
        // Локально приказ ещё не применён — это сделает сетевой цикл.
        assert_eq!(
            app.state().unit(tank).expect("tank").orders,
            UnitOrder::Await
        );

        // Забираем: сессия отправила бы это в `SessionHandle`.
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

        let unit = app.state().unit(tank).expect("tank");

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
        let before = app.state().turn_counter;

        let over = app.end_turn();

        // 06 применён (как `Remote_ReceiveNetPacket_06`), ход закрыт.
        assert_eq!(app.applied_commands(), 1);
        assert!(app
            .hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("End Turn")));

        if over {
            // `check_end_game_conditions` завершил партию: `finish_turn` не
            // сбрасывает фазу и не двигает счётчик хода.
            assert_eq!(app.state().game_state, GameStateType::EndTurn);
            assert_eq!(app.state().turn_counter, before);
        } else {
            assert_eq!(app.state().game_state, GameStateType::InGame);
            assert_eq!(app.state().turn_counter, before + 1);
        }
    }

    #[test]
    fn resources_and_unit_panel_show_the_documented_fields() {
        let (app, tank, _enemy) = test_app();
        let line = resources_line(app.state(), TeamId::RED);

        assert!(line.contains("GOLD 1500"), "{line}");
        assert!(line.contains("TEAM 0"), "{line}");

        let lines = unit_panel_lines(app.state(), tank);

        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains("Tank"), "{lines:?}");
        assert!(lines[1].starts_with("HP "), "{lines:?}");
        assert!(lines[2].starts_with("AMMO "), "{lines:?}");
        assert!(lines[3].contains("ORDER Await"), "{lines:?}");
        assert!(lines[4].contains("GRID 10,10"), "{lines:?}");
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
        // Повторный Esc — выход из сессии.
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

        // Пустой Enter закрывает ввод и ничего не возвращает.
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
            state: app.state(),
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
            state: app.state(),
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

        // Enter открывает строку ввода.
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

        // Enter отправляет: в демо-режиме — сразу через apply_command.
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Enter), false),
            Transition::None
        );
        assert!(!app.hud().chat.is_active());
        assert_eq!(
            app.state().chat_log,
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

        // T тоже открывает строку ввода.
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
            app.state().chat_log.is_empty(),
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

        // Очередь — это ровно пакет 18 на проводе.
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
        assert!(app.state().chat_log.is_empty());
        assert!(app
            .hud()
            .log
            .lines()
            .iter()
            .any(|line| line == "Chat cancelled"));

        // Следующий Esc — уже обычное поведение сессии.
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape), false),
            Transition::LeaveSession
        );
    }
}
