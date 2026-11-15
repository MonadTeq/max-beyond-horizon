//! UI-каркас клиента: экраны меню, переходы и отрисовка.
//!
//! Модуль — **чистое ядро** поверх [`max_render::ui`]:
//!
//! * [`Screen`] — состояние экрана: главное меню, список игр, форма создания
//!   игры и активная сессия;
//! * [`InputEvent`] — то, что SDL-слой превращает в клавишу/клик;
//! * [`Screen::handle_input`] — машина переходов: `MainMenu → CreateForm`,
//!   `MainMenu → JoinList` (через `list_games`), `CreateForm → create_game →
//!   InSession`, `JoinList → InSession`;
//! * [`UiContext`] — размер экрана, позиция мыши, строка статуса, клиент
//!   Horizon за трейтом [`MenuClient`] и [`AudioPlayer`] для кликов (M6.2);
//! * [`Screen::render`] — кадр без SDL: `render_ui` для меню и карта
//!   (`App::render_frame` → `max_render::map_render`) + строка статуса для
//!   [`Screen::InSession`].
//!
//! # Тестируемость без сети
//!
//! Сеть скрыта за трейтом [`MenuClient`]. Тесты передают мок-клиент с
//! заранее заданными [`GameInfo`] и проверяют переходы; настоящий клиент —
//! [`HorizonMenuClient`] — синхронная обёртка над асинхронным
//! [`max_net::HorizonClient`] (каждый вызов поднимает короткоживущий
//! `tokio` runtime). Вход в игру — [`Transition::StartSession`]: исполнение
//! сетевого цикла остаётся в `main.rs` и переиспользует
//! [`crate::run_network`].
//!
//! # Клавиши
//!
//! | Экран | Управление |
//! |---|---|
//! | MainMenu | Up/Down/Tab, Enter, мышь; `c`/`j`/`q`; Esc — выход |
//! | JoinList | Up/Down, Enter — войти, `r` — обновить, Esc — назад, мышь |
//! | CreateForm | Tab/Up/Down — поле, ввод, Left/Right — числа, Enter — создать, Esc — назад |
//! | InSession | LMB — выбрать юнита, RMB — приказ движения, LMB по врагу — атака, Enter/T — чат (пакет 18), End Turn/Menu, Esc — отмена (дважды — выход) |
//!
//! # Ввод в сессии (M4.3)
//!
//! Меню живёт в [`Screen::handle_input`], а ввод внутри партии — в
//! [`handle_session_input`]: он работает с [`App`] (выделение и приказы в
//! [`crate::hud::HudState`], а не в `GameState`), поэтому выделение юнита не
//! попадает в снапшот/CRC. Приказы (`38`/`08`) собирает
//! [`crate::hud::build_move_order`]/[`crate::hud::build_attack_order`] и кладёт
//! в `App::hud().pending`: в сетевой сессии их отправляет `net.rs`
//! (`flush_hud_orders`), в демо-режиме (`online = false`) они применяются
//! локально через `events::apply_command`.

use std::fmt;

use max_core::geo::Point as GridPoint;
use max_core::input::{Command, Packet18Chat};
use max_core::unit::UnitInfo;
use max_net::{
    CreateGameParams, GameInfo, HorizonClient, CLIENT_NAME, HORIZON_PROTOCOL_VERSION,
};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer, BYTES_PER_PIXEL};
use max_render::text::{draw_text_line, GLYPH_HEIGHT};
use max_render::ui::{
    hit_test, layout, render_ui, Button, Label, ListBox, Point, Rect, TextBox, UiState, UiTheme,
    Widget,
};

use crate::audio::{AudioPlayer, MENU_CLICK};
use crate::hud::{self, HudButton};
use crate::App;

/// Имя игры по умолчанию, если поле «Name» пустое.
pub const DEFAULT_GAME_NAME: &str = "M.A.X. Rust game";
/// Последний допустимый индекс планеты (`0..=23`, как `--world`).
pub const MAX_WORLD: i64 = 23;

/// Уровни/индексы виджетов, на которые опирается обработка мыши.
const MAIN_CREATE: usize = 2;
const MAIN_JOIN: usize = 3;
const MAIN_QUIT: usize = 4;

const JOIN_LIST: usize = 1;
const JOIN_BACK: usize = 2;
const JOIN_REFRESH: usize = 3;
const JOIN_JOIN: usize = 4;

/// Клавиша UI (SDL-слой переводит в неё `WindowKey`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Esc.
    Escape,
    /// Enter.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Стрелка вверх.
    Up,
    /// Стрелка вниз.
    Down,
    /// Стрелка влево.
    Left,
    /// Стрелка вправо.
    Right,
    /// Печатный символ.
    Char(char),
    /// Прочая клавиша (игнорируется).
    Other,
}

/// Входное событие UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// Клавиша.
    Key(Key),
    /// Клик левой кнопкой в пикселях окна.
    MouseClick(i32, i32),
    /// Клик правой кнопкой в пикселях окна (RMB — приказ движения/отмена).
    RightClick(i32, i32),
    /// Движение мыши в пикселях окна.
    MouseMove(i32, i32),
}

/// Параметры входа в сетевую сессию — результат выбора в меню.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequest {
    /// `game_id` из Horizon.
    pub game_id: String,
    /// Пароль игры, если он был задан/известен.
    pub password: Option<String>,
}

/// Переход, который должно исполнить приложение.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// Экран изменился/статус обновился — просто перерисовать.
    None,
    /// Войти в игру: `main.rs` запускает существующий `run_network`.
    StartSession(SessionRequest),
    /// Выйти из сессии (Esc в SDL-цикле).
    LeaveSession,
    /// Закрыть приложение.
    Quit,
}

/// Поле формы создания игры.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateField {
    /// Название игры.
    Name,
    /// Планета (`0..=23`).
    World,
    /// Секунды на ход.
    Timer,
    /// Секунды на завершение хода.
    EndTurn,
    /// Число противников (`1..=3`).
    Opponent,
    /// Пароль (пусто — открытая игра).
    Password,
}

impl CreateField {
    /// Все поля в порядке Tab.
    pub const ALL: [Self; 6] = [
        Self::Name,
        Self::World,
        Self::Timer,
        Self::EndTurn,
        Self::Opponent,
        Self::Password,
    ];

    /// Индекс поля в [`CreateField::ALL`].
    pub const fn index(self) -> usize {
        match self {
            Self::Name => 0,
            Self::World => 1,
            Self::Timer => 2,
            Self::EndTurn => 3,
            Self::Opponent => 4,
            Self::Password => 5,
        }
    }

    /// Следующее поле (по кругу).
    pub const fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// Предыдущее поле (по кругу).
    pub const fn prev(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Подпись поля.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::World => "World",
            Self::Timer => "Turn timer",
            Self::EndTurn => "End turn",
            Self::Opponent => "Opponents",
            Self::Password => "Password",
        }
    }

    /// Поле числовое (редактируется цифрами и стрелками).
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::World | Self::Timer | Self::EndTurn | Self::Opponent)
    }
}

/// Состояние формы создания игры.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateForm {
    /// Название игры.
    pub name: String,
    /// Планета.
    pub world: i64,
    /// Секунды на ход.
    pub timer: i64,
    /// Секунды на завершение хода.
    pub endturn: i64,
    /// Число противников.
    pub opponent: i64,
    /// Пароль.
    pub password: String,
    /// Поле в фокусе.
    pub focus: CreateField,
}

impl Default for CreateForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            world: 0,
            timer: 60,
            endturn: 30,
            opponent: 1,
            password: String::new(),
            focus: CreateField::Name,
        }
    }
}

impl CreateForm {
    /// Текущее значение поля строкой (для отрисовки).
    pub fn value(&self, field: CreateField) -> String {
        match field {
            CreateField::Name => self.name.clone(),
            CreateField::World => self.world.to_string(),
            CreateField::Timer => self.timer.to_string(),
            CreateField::EndTurn => self.endturn.to_string(),
            CreateField::Opponent => self.opponent.to_string(),
            CreateField::Password => self.password.clone(),
        }
    }

    /// Вводит символ в поле в фокусе.
    pub fn push_char(&mut self, ch: char) {
        match self.focus {
            CreateField::Name => {
                if self.name.chars().count() < 24 && (ch.is_ascii_graphic() || ch == ' ') {
                    self.name.push(ch);
                }
            }
            CreateField::Password => {
                if self.password.chars().count() < 16 && (ch.is_ascii_graphic() || ch == ' ') {
                    self.password.push(ch);
                }
            }
            field => {
                if let Some(digit) = ch.to_digit(10) {
                    let value = self.numeric(field).saturating_mul(10).saturating_add(i64::from(digit));
                    self.set_numeric(field, value);
                }
            }
        }
    }

    /// Backspace в текущем поле.
    pub fn backspace(&mut self) {
        match self.focus {
            CreateField::Name => {
                self.name.pop();
            }
            CreateField::Password => {
                self.password.pop();
            }
            field => {
                let value = self.numeric(field) / 10;
                self.set_numeric(field, value);
            }
        }
    }

    /// Меняет числовое поле на `delta` (стрелки Left/Right).
    pub fn adjust(&mut self, delta: i64) {
        if self.focus.is_numeric() {
            let value = self.numeric(self.focus).saturating_add(delta);
            self.set_numeric(self.focus, value);
        }
    }

    /// Параметры `CreateGame`.
    pub fn create_params(&self) -> CreateGameParams {
        let name = self.name.trim();

        CreateGameParams {
            name: Some(if name.is_empty() {
                DEFAULT_GAME_NAME.to_string()
            } else {
                name.to_string()
            }),
            world: Some(self.world.clamp(0, MAX_WORLD)),
            timer: Some(self.timer.clamp(0, 3600)),
            endturn: Some(self.endturn.clamp(0, 600)),
            play_mode: None,
            start_gold: None,
            victory_type: None,
            victory_limit: None,
            min_players: Some((self.opponent.clamp(1, 3) + 1) as u32),
            password: if self.password.is_empty() {
                None
            } else {
                Some(self.password.clone())
            },
        }
    }

    /// Пароль для `JoinGame`, если игра защищена.
    pub fn password_value(&self) -> Option<String> {
        (!self.password.is_empty()).then(|| self.password.clone())
    }

    fn numeric(&self, field: CreateField) -> i64 {
        match field {
            CreateField::World => self.world,
            CreateField::Timer => self.timer,
            CreateField::EndTurn => self.endturn,
            CreateField::Opponent => self.opponent,
            _ => 0,
        }
    }

    fn set_numeric(&mut self, field: CreateField, value: i64) {
        match field {
            CreateField::World => self.world = value.clamp(0, MAX_WORLD),
            CreateField::Timer => self.timer = value.clamp(0, 3600),
            CreateField::EndTurn => self.endturn = value.clamp(0, 600),
            CreateField::Opponent => self.opponent = value.clamp(1, 3),
            _ => {}
        }
    }
}

/// Экран клиента.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// Главное меню.
    MainMenu,
    /// Список игр из `list_games`.
    JoinList {
        /// Игры лобби.
        games: Vec<GameInfo>,
        /// Выбранная игра.
        selected: usize,
    },
    /// Форма создания игры.
    CreateForm(CreateForm),
    /// Игра идёт: карта + статус; сетевой цикл запускает `main.rs`.
    InSession {
        /// Куда подключаться.
        request: SessionRequest,
    },
}

/// Минимальный клиент Horizon, нужный меню.
///
/// Синхронный, чтобы [`Screen::handle_input`] оставался чистой функцией без
/// `async`: сеть вызывается только по явному действию (Join/Refresh/Create).
pub trait MenuClient {
    /// `ListGames` лобби.
    fn list_games(&mut self) -> Result<Vec<GameInfo>, String>;
    /// `CreateGame`; возвращает созданную игру.
    fn create_game(&mut self, params: CreateGameParams) -> Result<GameInfo, String>;
}

/// Настоящий клиент Horizon: синхронная обёртка над `max_net::HorizonClient`.
///
/// Каждый вызов поднимает короткоживущий `tokio` runtime и закрывает gRPC
/// канал после ответа — для операций меню (список/создание) этого достаточно,
/// а долгая сессия идёт через [`crate::run_network`].
#[derive(Debug, Clone)]
pub struct HorizonMenuClient {
    host: String,
    port: u16,
}

impl HorizonMenuClient {
    /// Клиент для Horizon `host:port`.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    fn runtime() -> Result<tokio::runtime::Runtime, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("tokio runtime: {error}"))
    }

    async fn connect(host: &str, port: u16) -> Result<HorizonClient, String> {
        let mut client = HorizonClient::connect(host, port)
            .await
            .map_err(|error| format!("Horizon {host}:{port}: {error}"))?;

        client
            .hello(HORIZON_PROTOCOL_VERSION, CLIENT_NAME)
            .await
            .map_err(|error| format!("Horizon Hello: {error}"))?;

        Ok(client)
    }
}

impl MenuClient for HorizonMenuClient {
    fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
        let runtime = Self::runtime()?;

        runtime.block_on(async {
            let mut client = Self::connect(&self.host, self.port).await?;
            client
                .list_games()
                .await
                .map_err(|error| format!("Horizon ListGames: {error}"))
        })
    }

    fn create_game(&mut self, params: CreateGameParams) -> Result<GameInfo, String> {
        let runtime = Self::runtime()?;

        runtime.block_on(async {
            let mut client = Self::connect(&self.host, self.port).await?;
            let created = client
                .create_game(params)
                .await
                .map_err(|error| format!("Horizon CreateGame: {error}"))?;
            Ok(created.game)
        })
    }
}

/// Контекст UI: экранные размеры, мышь, статус и клиент Horizon.
pub struct UiContext {
    /// Клиент лобби (в тестах — мок).
    pub client: Box<dyn MenuClient>,
    /// Ширина кадра.
    pub width: u32,
    /// Высота кадра.
    pub height: u32,
    /// Строка статуса/ошибки под списком кнопок.
    pub status: String,
    /// Позиция мыши (для hover).
    pub mouse: Option<Point>,
    /// Фокус главного меню (`0..=2`).
    pub focus: usize,
    /// Звук (M6.2): клики меню. Без feature `sound` — тихий no-op.
    pub audio: AudioPlayer,
}

impl UiContext {
    /// Контекст размера окна с пустым статусом.
    pub fn new(client: Box<dyn MenuClient>, width: u32, height: u32) -> Self {
        Self {
            client,
            width,
            height,
            status: String::new(),
            mouse: None,
            focus: 0,
            audio: AudioPlayer::new(),
        }
    }

    /// Контекст с готовым статусом.
    pub fn with_status(
        client: Box<dyn MenuClient>,
        width: u32,
        height: u32,
        status: impl Into<String>,
    ) -> Self {
        Self {
            status: status.into(),
            ..Self::new(client, width, height)
        }
    }

    /// Параметры отрисовки (без заимствования клиента).
    pub fn view<'a>(&'a self, app: Option<&'a App>) -> ScreenView<'a> {
        ScreenView {
            width: self.width,
            height: self.height,
            status: &self.status,
            mouse: self.mouse,
            focus: self.focus,
            app,
        }
    }

    /// Звук клика меню (`KBUY0` → `kbuy0.spw`).
    ///
    /// Вызывается на активации кнопок/строк меню. Ошибка/отсутствие файла не
    /// меняют состояние UI: звук — не блокер (M6.2).
    pub fn click(&mut self) {
        let _ = self.audio.play_once(MENU_CLICK);
    }
}

impl fmt::Debug for UiContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiContext")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("status", &self.status)
            .field("mouse", &self.mouse)
            .field("focus", &self.focus)
            .field("audio", &self.audio)
            .finish_non_exhaustive()
    }
}

/// Всё, что нужно [`Screen::render`] помимо самого экрана.
#[derive(Clone, Copy)]
pub struct ScreenView<'a> {
    /// Ширина кадра.
    pub width: u32,
    /// Высота кадра.
    pub height: u32,
    /// Строка статуса.
    pub status: &'a str,
    /// Позиция мыши.
    pub mouse: Option<Point>,
    /// Фокус главного меню.
    pub focus: usize,
    /// Состояние партии для [`Screen::InSession`].
    pub app: Option<&'a App>,
}

impl Screen {
    /// Обрабатывает ввод и двигает машину состояний.
    ///
    /// Возвращает [`Transition::StartSession`], когда игра создана/выбрана:
    /// сетевой цикл исполняет вызывающий (`main.rs`), а не UI-ядро. Ошибки
    /// сети остаются в [`UiContext::status`], экран при этом не меняется.
    pub fn handle_input(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        if let InputEvent::MouseMove(x, y) = event {
            ctx.mouse = Some(Point::new(x, y));
        }

        if matches!(self, Self::MainMenu) {
            return self.handle_main_menu(ctx, event);
        }
        if matches!(self, Self::JoinList { .. }) {
            return self.handle_join_list(ctx, event);
        }
        if matches!(self, Self::CreateForm(_)) {
            return self.handle_create_form(ctx, event);
        }

        match event {
            InputEvent::Key(Key::Escape) => Transition::LeaveSession,
            _ => Transition::None,
        }
    }

    /// Отрисовка экрана в кадр.
    ///
    /// Для [`Screen::InSession`] карта берётся из [`crate::App::render_frame`]
    /// (а он рисует `max_render::map_render`), сверху — строка статуса.
    pub fn render(&self, buffer: &mut RgbaBuffer, view: ScreenView<'_>) {
        match self {
            Self::MainMenu => {
                let ui = build_main_menu(view.width, view.height, view.status, view.mouse, view.focus);
                render_ui(buffer, &ui);
            }
            Self::JoinList { games, selected } => {
                let ui = build_join_list(
                    view.width,
                    view.height,
                    view.status,
                    view.mouse,
                    games,
                    *selected,
                );
                render_ui(buffer, &ui);
            }
            Self::CreateForm(form) => {
                let ui = build_create_form(view.width, view.height, view.status, view.mouse, form);
                render_ui(buffer, &ui);
            }
            Self::InSession { .. } => render_session(buffer, view),
        }
    }

    fn handle_main_menu(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let ui = build_main_menu(ctx.width, ctx.height, &ctx.status, ctx.mouse, ctx.focus);
        let mut activate = None;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                let point = Point::new(x, y);

                if let Some(index) = hit_test(&ui, point) {
                    if (MAIN_CREATE..=MAIN_QUIT).contains(&index) {
                        ctx.focus = index - MAIN_CREATE;
                        activate = Some(index);
                    }
                }
            }
            InputEvent::Key(key) => match key {
                Key::Up => ctx.focus = (ctx.focus + 2) % 3,
                Key::Down | Key::Tab => ctx.focus = (ctx.focus + 1) % 3,
                Key::Enter => activate = Some(MAIN_CREATE + ctx.focus),
                Key::Escape => return Transition::Quit,
                Key::Char(character) => match character.to_ascii_lowercase() {
                    'c' => activate = Some(MAIN_CREATE),
                    'j' => activate = Some(MAIN_JOIN),
                    'q' => return Transition::Quit,
                    _ => {}
                },
                _ => {}
            },
        }

        if activate.is_some() {
            ctx.click();
        }

        match activate {
            Some(MAIN_CREATE) => {
                *self = Self::CreateForm(CreateForm::default());
                ctx.status = "Configure the game and press Enter".to_string();
                Transition::None
            }
            Some(MAIN_JOIN) => {
                self.refresh_games(ctx);
                Transition::None
            }
            Some(MAIN_QUIT) => Transition::Quit,
            _ => Transition::None,
        }
    }

    fn refresh_games(&mut self, ctx: &mut UiContext) {
        match ctx.client.list_games() {
            Ok(games) => {
                let count = games.len();
                *self = Self::JoinList {
                    games,
                    selected: 0,
                };
                ctx.status = if count == 0 {
                    "No games found — create one or refresh".to_string()
                } else {
                    format!("{count} game(s) — Enter joins, R refreshes")
                };
            }
            Err(error) => {
                ctx.status = format!("List games failed: {error}");
            }
        }
    }

    fn handle_join_list(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let (games, selected) = match self {
            Self::JoinList { games, selected } => (games.clone(), *selected),
            _ => return Transition::None,
        };
        let ui = build_join_list(
            ctx.width,
            ctx.height,
            &ctx.status,
            ctx.mouse,
            &games,
            selected,
        );

        enum Action {
            Select(usize),
            Back,
            Refresh,
            Join,
        }

        let mut action = None;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                let point = Point::new(x, y);

                match hit_test(&ui, point) {
                    Some(JOIN_LIST) => {
                        if let Some(list) = ui.list_box(JOIN_LIST) {
                            if let Some(index) = list.row_at(point) {
                                action = Some(Action::Select(index));
                            }
                        }
                    }
                    Some(JOIN_BACK) => action = Some(Action::Back),
                    Some(JOIN_REFRESH) => action = Some(Action::Refresh),
                    Some(JOIN_JOIN) => action = Some(Action::Join),
                    _ => {}
                }
            }
            InputEvent::Key(Key::Up) => {
                action = Some(Action::Select(selected.saturating_sub(1)));
            }
            InputEvent::Key(Key::Down) => {
                action = Some(Action::Select((selected + 1).min(games.len().saturating_sub(1))));
            }
            InputEvent::Key(Key::Enter) => action = Some(Action::Join),
            InputEvent::Key(Key::Escape) => action = Some(Action::Back),
            InputEvent::Key(Key::Char('r' | 'R')) => action = Some(Action::Refresh),
            _ => {}
        }

        if action.is_some() {
            ctx.click();
        }

        match action {
            Some(Action::Select(index)) => {
                if let Self::JoinList { selected, .. } = self {
                    *selected = index;
                }
                Transition::None
            }
            Some(Action::Back) => {
                *self = Self::MainMenu;
                ctx.status.clear();
                Transition::None
            }
            Some(Action::Refresh) => {
                self.refresh_games(ctx);
                Transition::None
            }
            Some(Action::Join) => {
                let Some(game) = games.get(selected) else {
                    ctx.status = "No game selected — refresh the list".to_string();
                    return Transition::None;
                };
                let request = SessionRequest {
                    game_id: game.id.clone(),
                    password: None,
                };

                ctx.status = format!("Joining {} ({})", game.name, game.id);
                *self = Self::InSession {
                    request: request.clone(),
                };
                Transition::StartSession(request)
            }
            None => Transition::None,
        }
    }

    fn handle_create_form(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let ui = match self {
            Self::CreateForm(form) => {
                build_create_form(ctx.width, ctx.height, &ctx.status, ctx.mouse, form)
            }
            _ => return Transition::None,
        };

        let mut back = false;
        let mut submit = false;
        let mut clicked = false;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                let point = Point::new(x, y);

                match hit_test(&ui, point) {
                    Some(7) => {
                        back = true;
                        clicked = true;
                    }
                    Some(8) => {
                        submit = true;
                        clicked = true;
                    }
                    Some(index @ 1..=6) => {
                        clicked = true;
                        if let Self::CreateForm(form) = self {
                            form.focus = CreateField::ALL[index - 1];
                        }
                    }
                    _ => {}
                }
            }
            InputEvent::Key(key) => {
                if let Self::CreateForm(form) = self {
                    match key {
                        Key::Escape => back = true,
                        Key::Enter => submit = true,
                        Key::Tab | Key::Down => form.focus = form.focus.next(),
                        Key::Up => form.focus = form.focus.prev(),
                        Key::Backspace => form.backspace(),
                        Key::Left => form.adjust(-1),
                        Key::Right => form.adjust(1),
                        Key::Char(character) => form.push_char(character),
                        Key::Other => {}
                    }
                }
            }
        }

        if clicked || back || submit {
            ctx.click();
        }

        if back {
            *self = Self::MainMenu;
            ctx.status.clear();
            return Transition::None;
        }

        if submit {
            return self.submit_create(ctx);
        }

        Transition::None
    }

    fn submit_create(&mut self, ctx: &mut UiContext) -> Transition {
        let (params, password) = match self {
            Self::CreateForm(form) => {
                if !(0..=MAX_WORLD).contains(&form.world) {
                    ctx.status = format!("World must be 0..={MAX_WORLD}");
                    return Transition::None;
                }

                (form.create_params(), form.password_value())
            }
            _ => return Transition::None,
        };

        match ctx.client.create_game(params) {
            Ok(game) => {
                let request = SessionRequest {
                    game_id: game.id.clone(),
                    password,
                };
                ctx.status = format!("Created \"{}\" ({})", game.name, game.id);
                *self = Self::InSession {
                    request: request.clone(),
                };
                Transition::StartSession(request)
            }
            Err(error) => {
                ctx.status = format!("Create failed: {error}");
                Transition::None
            }
        }
    }
}

/// Ввод внутри сессии: карта, HUD, приказы (M4.3).
///
/// Работает с [`App`] напрямую (а не через [`Screen`]): выделение и приказы
/// живут в [`crate::hud::HudState`], которому нужен доступ к `GameState` и
/// камере. Чистая функция: ни сети, ни SDL, ни времени — всё тестируется
/// headless.
///
/// * **LMB по своему юниту** — выделение (`App::hud().selected`);
/// * **LMB по врагу** — приказ атаки (пакет `08`, процессор `P3`);
/// * **RMB по свободной клетке** — маршрут `Paths_RequestPath` +
///   предпросмотр линии + пакет `38` (`Command::Move`);
/// * **RMB по юниту / Esc** — отмена (второй Esc — выход из сессии);
/// * **End Turn** — тот же путь, что и таймер (`06` + `52`/`finish_turn`);
/// * **Menu** — выход в главное меню.
///
/// # Чат (M4.4)
///
/// **Enter** или **T** открывают строку ввода (`ChatInput`): печатные ASCII-
/// символы идут в буфер, **Backspace** удаляет последний, **Enter** отправляет
/// `Command::Chat` (пакет **18**), **Esc** закрывает ввод без отправки. Пока
/// строка открыта, мышь и остальные клавиши игнорируются.
///
/// `online = true` — приказы и чат копятся в `App::hud().pending` и уходят в
/// сессию (`net::flush_hud_orders`); `false` — демо-режим, команды применяются
/// сразу через `max_core::events::apply_command` ([`App::flush_orders`]).
pub fn handle_session_input(app: &mut App, event: InputEvent, online: bool) -> Transition {
    let (width, height) = app.viewport().screen();

    // Пока открыта строка чата (M4.4), клавиатура принадлежит ей: печатные
    // ASCII-символы идут в буфер, Backspace/Enter/Esc управляют вводом. Мышь
    // игнорируется, чтобы клик во время набора не отдал приказ.
    if app.hud().chat.is_active() {
        return match event {
            InputEvent::Key(key) => session_chat_key(app, key, online),
            _ => Transition::None,
        };
    }

    match event {
        InputEvent::MouseMove(x, y) => {
            app.hud_mut().hover_button = hud::hud_button_at(width, height, x, y);
            Transition::None
        }
        InputEvent::MouseClick(x, y) => session_left_click(app, width, height, x, y, online),
        InputEvent::RightClick(x, y) => session_right_click(app, x, y, online),
        InputEvent::Key(Key::Enter) | InputEvent::Key(Key::Char('t' | 'T')) => {
            app.hud_mut().chat.open();
            app.hud_mut()
                .push_log("Chat: type a message, Enter sends, Esc cancels");
            Transition::None
        }
        InputEvent::Key(Key::Escape) => {
            if app.hud().selected.is_some() || app.hud().preview.is_some() {
                app.hud_mut().clear_interaction();
                app.hud_mut().push_log("Cancelled");
                Transition::None
            } else {
                Transition::LeaveSession
            }
        }
        InputEvent::Key(_) => Transition::None,
    }
}

/// Клавиша в открытой строке ввода чата.
///
/// * печатный ASCII (включая пробел) — в буфер (до [`hud::CHAT_MAX_CHARS`]);
/// * Backspace — удалить последний символ;
/// * Enter — отправить [`Packet18Chat`] (`Command::Chat`): в сети — в очередь
///   `App::hud().pending` (её отправляет `net::flush_hud_orders`), в демо —
///   сразу в `apply_command`; строка журнала пишется в обоих режимах;
/// * Esc — закрыть ввод без отправки.
///
/// Остальные клавиши (стрелки/Tab) в чате игнорируются, чтобы случайно не
/// открыть меню/приказ.
fn session_chat_key(app: &mut App, key: Key, online: bool) -> Transition {
    match key {
        Key::Char(character) => {
            app.hud_mut().chat.push_char(character);
        }
        Key::Backspace => {
            app.hud_mut().chat.backspace();
        }
        Key::Enter => return submit_chat(app, online),
        Key::Escape => {
            app.hud_mut().chat.close();
            app.hud_mut().push_log("Chat cancelled");
        }
        Key::Tab | Key::Up | Key::Down | Key::Left | Key::Right | Key::Other => {}
    }

    Transition::None
}

/// Отправка строки чата: журнал + [`Command::Chat`] (пакет **18**).
///
/// Пустая строка не отправляется (как `Remote_SendNetPacket_18`, который
/// возвращается на `message[0] == '\0'`), ввод просто закрывается.
fn submit_chat(app: &mut App, online: bool) -> Transition {
    let Some(message) = app.hud_mut().chat.take_message() else {
        return Transition::None;
    };

    let sender = app.player_team();
    let line = hud::chat_line(sender, &message);
    let command = Command::Chat(Packet18Chat { sender, message });

    app.hud_mut().push_log(line);

    if online {
        app.hud_mut().pending.push(command);
    } else {
        app.apply(command);
    }

    Transition::None
}

/// Левая кнопка: кнопки HUD → миникарта → выделение/атака на карте.
fn session_left_click(
    app: &mut App,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    online: bool,
) -> Transition {
    if let Some(button) = hud::hud_button_at(width, height, x, y) {
        return match button {
            HudButton::EndTurn => {
                session_end_turn(app, online);
                Transition::None
            }
            HudButton::Menu => Transition::LeaveSession,
        };
    }

    // Клик по миникарте — центрирование камеры (как в оригинале).
    let layout = hud::hud_layout(width, height);

    if let Some(grid) = hud::minimap_grid_at(&layout, x, y) {
        app.viewport_mut().center_on_grid(grid.x, grid.y);
        return Transition::None;
    }

    let team = app.player_team();
    let Some(id) = hud::unit_at_screen(app.state(), app.viewport(), team, x, y) else {
        if app.hud().selected.is_some() {
            app.hud_mut().clear_interaction();
            app.hud_mut().push_log("Deselected");
        }
        return Transition::None;
    };

    let (unit_team, label) = match app.state().unit(id) {
        Some(unit) => (unit.team, unit_label(unit)),
        None => return Transition::None,
    };

    if unit_team == team {
        app.hud_mut().select(Some(id));
        app.hud_mut().preview = None;
        app.hud_mut().push_log(format!("Selected {label}"));
        return Transition::None;
    }

    // Враг: атака по выделенному юниту (пакет 08, P3).
    let Some(selected) = app.hud().selected else {
        app.hud_mut().push_log("Enemy unit — select your unit first");
        return Transition::None;
    };

    match hud::build_attack_order(app.state(), selected, id) {
        Some(command) => {
            app.hud_mut().preview = None;
            app.hud_mut().pending.push(command);
            app.hud_mut().push_log(format!("Attack: {label}"));
            if !online {
                apply_pending_orders(app);
            }
        }
        None => app.hud_mut().push_log("Cannot attack this unit"),
    }

    Transition::None
}

/// Правая кнопка: отмена по юниту, иначе приказ движения в клетку.
fn session_right_click(app: &mut App, x: i32, y: i32, online: bool) -> Transition {
    let team = app.player_team();

    // «Отмена — Esc/правый клик по юниту».
    if let Some(id) = hud::unit_at_screen(app.state(), app.viewport(), team, x, y) {
        let label = app
            .state()
            .unit(id)
            .map(unit_label)
            .unwrap_or_else(|| "unit".to_string());

        app.hud_mut().clear_interaction();
        app.hud_mut().push_log(format!("Cancelled ({label})"));
        return Transition::None;
    }

    let Some(selected) = app.hud().selected else {
        app.hud_mut().push_log("No unit selected — LMB selects");
        return Transition::None;
    };

    let (grid_x, grid_y) = app.viewport().screen_to_grid(x, y);
    let destination = GridPoint::new(grid_x, grid_y);

    if grid_x < 0
        || grid_y < 0
        || grid_x >= max_core::geo::MAP_WIDTH
        || grid_y >= max_core::geo::MAP_HEIGHT
    {
        app.hud_mut().push_log("Outside the map");
        return Transition::None;
    }

    let can_move = app
        .state()
        .unit(selected)
        .is_some_and(hud::unit_can_move);

    if !can_move {
        app.hud_mut()
            .push_log("Unit cannot move (air/stationary)");
        return Transition::None;
    }

    match hud::build_move_order(app.state(), selected, destination) {
        Some(order) => {
            let steps = order.preview.len();
            app.hud_mut().preview = Some(order.preview);
            app.hud_mut().pending.push(order.command);
            app.hud_mut().push_log(format!(
                "Move: {steps} steps to ({}, {})",
                destination.x, destination.y
            ));
            if !online {
                apply_pending_orders(app);
            }
        }
        None => {
            app.hud_mut().preview = None;
            app.hud_mut().push_log(format!(
                "No path to ({}, {})",
                destination.x, destination.y
            ));
        }
    }

    Transition::None
}

/// End Turn: в сети — флаг для игрового цикла (`06`+`52` там же), в демо —
/// локальный проход [`App::end_turn`].
fn session_end_turn(app: &mut App, online: bool) {
    if online {
        app.hud_mut().request_end_turn();
        app.hud_mut().push_log("End Turn requested");
    } else {
        app.end_turn();
    }
}

/// Применяет накопленные приказы локально (демо-режим) и пишет об этом в журнал.
fn apply_pending_orders(app: &mut App) {
    let applied = app.flush_orders();

    if applied > 0 {
        let line = format!("{applied} order(s) applied locally");
        app.hud_mut().push_log(line);
    }
}

/// Подпись юнита для журнала: имя (если задано) и тип.
fn unit_label(unit: &UnitInfo) -> String {
    if unit.name.is_empty() {
        format!("{:?}", unit.unit_type)
    } else {
        format!("{} [{:?}]", unit.name, unit.unit_type)
    }
}

/// Кадр сессии: карта из [`App::render_frame`] + HUD (M4.2) поверх неё.
///
/// Без `App` (headless-рендер до сети) остаётся прежний фон со строкой статуса.
fn render_session(buffer: &mut RgbaBuffer, view: ScreenView<'_>) {
    if let Some(app) = view.app {
        let frame = app.render_frame();
        blit(buffer, &frame);

        let hud_view = hud::HudView {
            state: app.state(),
            hud: app.hud(),
            viewport: app.viewport(),
            team: app.player_team(),
            status: view.status,
        };

        hud::render_hud(buffer, &hud_view);
        return;
    }

    buffer.fill(Rgba::rgb(8, 12, 20));

    let bar_height = (GLYPH_HEIGHT + 8).max(16);
    let y = view.height as i32 - bar_height;

    if y >= 0 {
        buffer.fill_rect(
            PixelRect::new(0, y, view.width as i32, bar_height),
            Rgba::rgb(8, 12, 20),
        );
        draw_text_line(
            buffer,
            4,
            y + 4,
            view.status,
            Rgba::rgb(232, 236, 244),
        );
    }
}

/// Копирует кадр `src` в `dst` с обрезкой по меньшему размеру.
fn blit(dst: &mut RgbaBuffer, src: &RgbaBuffer) {
    let width = dst.width().min(src.width()) as usize;
    let height = dst.height().min(src.height()) as usize;

    if width == 0 || height == 0 {
        return;
    }

    let src_stride = src.stride();
    let dst_stride = dst.stride();
    let row_bytes = width * BYTES_PER_PIXEL;
    let src_pixels = src.pixels();
    let dst_pixels = dst.pixels_mut();

    for row in 0..height {
        let src_start = row * src_stride;
        let dst_start = row * dst_stride;
        dst_pixels[dst_start..dst_start + row_bytes]
            .copy_from_slice(&src_pixels[src_start..src_start + row_bytes]);
    }
}

// ---------------------------------------------------------------------------
// Раскладка экранов
// ---------------------------------------------------------------------------

fn horizontal_margin(width: i32) -> i32 {
    (width / 12).clamp(12, 48)
}

fn build_main_menu(
    width: u32,
    height: u32,
    status: &str,
    mouse: Option<Point>,
    focus: usize,
) -> UiState {
    let width = width as i32;
    let height = height as i32;
    let margin = horizontal_margin(width);
    let content = (width - margin * 2).max(60);
    let x = (width - content) / 2;
    let heights = [24, 12, 28, 28, 28, 12];
    let gap = 8;
    let total = heights.iter().sum::<i32>() + gap * (heights.len() as i32 - 1);
    let top = ((height - total) / 2).max(8);
    let rects = layout(Rect::new(x, top, content, total), &heights, gap);
    let theme = UiTheme::default();
    let mut widgets = vec![
        Widget::Label(Label::colored("M.A.X.", rects[0], theme.accent)),
        Widget::Label(Label::colored(
            "Rust client — M4.1 UI",
            rects[1],
            theme.text_disabled,
        )),
        Widget::Button(Button::new("Create game", rects[2])),
        Widget::Button(Button::new("Join game", rects[3])),
        Widget::Button(Button::new("Quit", rects[4])),
        Widget::Label(Label::colored(
            "Up/Down + Enter or mouse; C/J/Q; Esc — quit",
            rects[5],
            theme.text_disabled,
        )),
    ];

    widgets.push(Widget::Label(Label::colored(
        status,
        Rect::new(margin, (height - 18).max(0), content, 12),
        theme.accent,
    )));

    let mut ui = UiState::new(widgets);
    ui.focus = Some(MAIN_CREATE + focus.min(2));
    ui.set_hover(mouse);
    ui
}

fn build_join_list(
    width: u32,
    height: u32,
    status: &str,
    mouse: Option<Point>,
    games: &[GameInfo],
    selected: usize,
) -> UiState {
    let width = width as i32;
    let height = height as i32;
    let margin = horizontal_margin(width);
    let content = (width - margin * 2).max(60);
    let x = (width - content) / 2;
    let buttons_y = (height - 72).max(44);
    let list_top = 30;
    let list_height = (buttons_y - 6 - list_top).max(20);
    let theme = UiTheme::default();

    let items = games
        .iter()
        .map(|game| {
            let lock = if game.password_protected { " *" } else { "" };
            format!(
                "{}  [{}]  {}/{}{}",
                game.name,
                game.status.as_ws_str(),
                game.players,
                game.min_players,
                lock
            )
        })
        .collect();

    let mut list = ListBox::with_items(
        Rect::new(x, list_top, content, list_height),
        items,
    );
    list.selected = (!games.is_empty()).then_some(selected.min(games.len().saturating_sub(1)));
    list.ensure_visible();

    let back = Rect::new(x, buttons_y, 100, 26);
    let refresh = Rect::new(x + 108, buttons_y, 100, 26);
    let join = Rect::new(x + content - 100, buttons_y, 100, 26);
    let widgets = vec![
        Widget::Label(Label::colored(
            "JOIN GAME — select a game and press Enter",
            Rect::new(x, 12, content, 12),
            theme.accent,
        )),
        Widget::ListBox(list),
        Widget::Button(Button::new("Back", back)),
        Widget::Button(Button::new("Refresh", refresh)),
        Widget::Button(Button::new("Join", join).with_enabled(!games.is_empty())),
        Widget::Label(Label::colored(
            "Up/Down, Enter — join; R — refresh; Esc — back",
            Rect::new(x, (height - 38).max(0), content, 12),
            theme.text_disabled,
        )),
        Widget::Label(Label::colored(
            status,
            Rect::new(x, (height - 18).max(0), content, 12),
            theme.accent,
        )),
    ];

    let mut ui = UiState::new(widgets);
    ui.focus = Some(JOIN_LIST);
    ui.set_hover(mouse);
    ui
}

fn build_create_form(
    width: u32,
    height: u32,
    status: &str,
    mouse: Option<Point>,
    form: &CreateForm,
) -> UiState {
    let width = width as i32;
    let height = height as i32;
    let margin = horizontal_margin(width);
    let content = (width - margin * 2).max(60);
    let x = (width - content) / 2;
    let field_height = ((height - 96) / 8).clamp(14, 22);
    let field_gap = 4;
    let fields_top = 28;
    let rects = layout(
        Rect::new(x, fields_top, content, 0),
        &[field_height; 6],
        field_gap,
    );
    let buttons_y = fields_top + 6 * (field_height + field_gap) + 2;
    let back = Rect::new(x, buttons_y, 100, 24);
    let create = Rect::new(x + content - 100, buttons_y, 100, 24);
    let theme = UiTheme::default();

    let values = [
        (CreateField::Name, 24usize),
        (CreateField::World, 2),
        (CreateField::Timer, 4),
        (CreateField::EndTurn, 4),
        (CreateField::Opponent, 1),
        (CreateField::Password, 16),
    ];

    let mut widgets = vec![Widget::Label(Label::colored(
        "NEW GAME — settings",
        Rect::new(x, 10, content, 12),
        theme.accent,
    ))];

    for (index, (field, max_chars)) in values.into_iter().enumerate() {
        let placeholder = match field {
            CreateField::Name => DEFAULT_GAME_NAME,
            CreateField::World => "0..23",
            CreateField::Timer => "seconds",
            CreateField::EndTurn => "seconds",
            CreateField::Opponent => "1..3",
            CreateField::Password => "(open game)",
        };
        let text_box = TextBox::with_value(
            field.label(),
            form.value(field),
            placeholder,
            rects[index],
        )
        .with_max_chars(max_chars);

        widgets.push(Widget::TextBox(text_box));
    }

    widgets.push(Widget::Button(Button::new("Back", back)));
    widgets.push(Widget::Button(Button::new("Create", create)));
    widgets.push(Widget::Label(Label::colored(
        "Tab/Up/Down — field; Left/Right — numbers; Enter — create; Esc — back",
        Rect::new(x, buttons_y + 32, content, 12),
        theme.text_disabled,
    )));
    widgets.push(Widget::Label(Label::colored(
        status,
        Rect::new(x, (height - 18).max(0), content, 12),
        theme.accent,
    )));

    let mut ui = UiState::new(widgets);
    ui.focus = Some(1 + form.focus.index());
    ui.set_hover(mouse);
    ui
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::rc::Rc;

    use max_net::GameStatus;

    use crate::AppConfig;

    #[derive(Default)]
    struct MockLog {
        list_calls: usize,
        create_calls: usize,
        last_params: Option<CreateGameParams>,
    }

    struct MockClient {
        log: Rc<RefCell<MockLog>>,
        games: Vec<GameInfo>,
        created: Option<GameInfo>,
        list_error: Option<String>,
        create_error: Option<String>,
    }

    impl MockClient {
        fn new(
            games: Vec<GameInfo>,
            created: Option<GameInfo>,
        ) -> (Self, Rc<RefCell<MockLog>>) {
            let log = Rc::new(RefCell::new(MockLog::default()));
            let client = Self {
                log: Rc::clone(&log),
                games,
                created,
                list_error: None,
                create_error: None,
            };

            (client, log)
        }
    }

    impl MenuClient for MockClient {
        fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
            self.log.borrow_mut().list_calls += 1;
            self.list_error
                .clone()
                .map_or_else(|| Ok(self.games.clone()), Err)
        }

        fn create_game(&mut self, params: CreateGameParams) -> Result<GameInfo, String> {
            self.log.borrow_mut().create_calls += 1;
            self.log.borrow_mut().last_params = Some(params);
            self.create_error.clone().map_or_else(
                || {
                    self.created
                        .clone()
                        .ok_or_else(|| "mock: no created game".to_string())
                },
                Err,
            )
        }
    }

    fn game(id: &str, name: &str) -> GameInfo {
        GameInfo {
            id: id.to_string(),
            name: name.to_string(),
            world: 3,
            status: GameStatus::Running,
            port: 31554,
            players: 1,
            min_players: 2,
            password_protected: false,
        }
    }

    /// Backend, считающий вызовы `play` (звук в тестах без устройства).
    #[derive(Debug)]
    struct CountingBackend {
        calls: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl crate::audio::AudioBackend for CountingBackend {
        fn name(&self) -> &'static str {
            "counting"
        }

        fn play(
            &mut self,
            _clip: std::sync::Arc<max_assets::WavClip>,
            _mode: crate::audio::PlayMode,
        ) -> Result<(), String> {
            *self.calls.lock().expect("lock") += 1;

            Ok(())
        }
    }

    fn context(
        games: Vec<GameInfo>,
        created: Option<GameInfo>,
    ) -> (UiContext, Rc<RefCell<MockLog>>) {
        let (client, log) = MockClient::new(games, created);
        (UiContext::new(Box::new(client), 512, 384), log)
    }

    fn key(screen: &mut Screen, ctx: &mut UiContext, key: Key) -> Transition {
        screen.handle_input(ctx, InputEvent::Key(key))
    }

    fn char(screen: &mut Screen, ctx: &mut UiContext, c: char) -> Transition {
        key(screen, ctx, Key::Char(c))
    }

    #[test]
    fn main_menu_opens_the_create_form_and_escape_goes_back() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::MainMenu;

        assert_eq!(key(&mut screen, &mut ctx, Key::Enter), Transition::None);
        assert!(matches!(screen, Screen::CreateForm(_)));

        // Tab ведёт к следующему полю.
        key(&mut screen, &mut ctx, Key::Tab);
        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.focus, CreateField::World);
        } else {
            panic!("ожидалась форма создания");
        }

        key(&mut screen, &mut ctx, Key::Escape);
        assert_eq!(screen, Screen::MainMenu);
    }

    #[test]
    fn create_form_creates_a_game_and_enters_the_session_with_mock_client() {
        let created = game("created-1", "Duel");
        let (mut ctx, log) = context(Vec::new(), Some(created.clone()));
        let mut screen = Screen::MainMenu;

        // Create game (фокус по умолчанию — первая кнопка).
        key(&mut screen, &mut ctx, Key::Enter);
        for c in "Duel".chars() {
            char(&mut screen, &mut ctx, c);
        }

        // World 0 по умолчанию; проверим переключение поля и числовой ввод.
        key(&mut screen, &mut ctx, Key::Tab);
        char(&mut screen, &mut ctx, '1');
        char(&mut screen, &mut ctx, '2');
        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.world, 12);
        }

        let transition = key(&mut screen, &mut ctx, Key::Enter);

        assert_eq!(
            transition,
            Transition::StartSession(SessionRequest {
                game_id: "created-1".to_string(),
                password: None,
            })
        );
        assert_eq!(
            screen,
            Screen::InSession {
                request: SessionRequest {
                    game_id: "created-1".to_string(),
                    password: None,
                }
            }
        );

        let log = log.borrow();
        assert_eq!(log.create_calls, 1);
        let params = log.last_params.as_ref().expect("параметры CreateGame");
        assert_eq!(params.name.as_deref(), Some("Duel"));
        assert_eq!(params.world, Some(12));
        assert_eq!(params.min_players, Some(2));
        assert_eq!(params.password, None);
    }

    #[test]
    fn create_failure_keeps_the_form_and_reports_status() {
        let (client, _log) = MockClient::new(Vec::new(), None);
        let client = MockClient {
            create_error: Some("horizon down".to_string()),
            ..client
        };
        let mut ctx = UiContext::new(Box::new(client), 512, 384);
        let mut screen = Screen::CreateForm(CreateForm::default());

        let transition = key(&mut screen, &mut ctx, Key::Enter);

        assert_eq!(transition, Transition::None);
        assert!(matches!(screen, Screen::CreateForm(_)));
        assert!(ctx.status.contains("horizon down"));
    }

    #[test]
    fn join_list_lists_games_and_selects_with_keyboard() {
        let (mut ctx, log) = context(
            vec![game("g1", "Alpha"), game("g2", "Beta")],
            None,
        );
        let mut screen = Screen::MainMenu;

        // MainMenu: Join game.
        key(&mut screen, &mut ctx, Key::Down);
        key(&mut screen, &mut ctx, Key::Enter);

        assert_eq!(
            screen,
            Screen::JoinList {
                games: vec![game("g1", "Alpha"), game("g2", "Beta")],
                selected: 0,
            }
        );
        assert_eq!(log.borrow().list_calls, 1);

        key(&mut screen, &mut ctx, Key::Down);
        if let Screen::JoinList { selected, .. } = &screen {
            assert_eq!(*selected, 1);
        } else {
            panic!("ожидался список игр");
        }

        let transition = key(&mut screen, &mut ctx, Key::Enter);
        assert_eq!(
            transition,
            Transition::StartSession(SessionRequest {
                game_id: "g2".to_string(),
                password: None,
            })
        );
        assert!(matches!(screen, Screen::InSession { .. }));
    }

    #[test]
    fn join_list_mouse_click_selects_a_row() {
        let games = vec![game("g1", "Alpha"), game("g2", "Beta")];
        let (mut ctx, _log) = context(games.clone(), None);
        let mut screen = Screen::JoinList {
            games,
            selected: 0,
        };

        let ui = build_join_list(
            ctx.width,
            ctx.height,
            &ctx.status,
            None,
            match &screen {
                Screen::JoinList { games, .. } => games,
                _ => unreachable!(),
            },
            0,
        );
        let list = ui.list_box(JOIN_LIST).expect("ListBox");
        let point = Point::new(list.rect.x + 4, list.rect.y + list.row_height + 2);

        screen.handle_input(&mut ctx, InputEvent::MouseClick(point.x, point.y));

        if let Screen::JoinList { selected, .. } = &screen {
            assert_eq!(*selected, 1);
        } else {
            panic!("ожидался список игр");
        }
    }

    #[test]
    fn join_list_refresh_uses_list_games_again() {
        let (mut ctx, log) = context(vec![game("g1", "Alpha")], None);
        let mut screen = Screen::JoinList {
            games: vec![game("g1", "Alpha")],
            selected: 0,
        };

        key(&mut screen, &mut ctx, Key::Char('r'));

        assert_eq!(log.borrow().list_calls, 1);
        assert!(matches!(screen, Screen::JoinList { .. }));
    }

    #[test]
    fn list_games_failure_stays_in_the_menu() {
        let (client, _log) = MockClient::new(Vec::new(), None);
        let client = MockClient {
            list_error: Some("no horizon".to_string()),
            ..client
        };
        let mut ctx = UiContext::new(Box::new(client), 512, 384);
        let mut screen = Screen::MainMenu;

        key(&mut screen, &mut ctx, Key::Down);
        key(&mut screen, &mut ctx, Key::Enter);

        assert_eq!(screen, Screen::MainMenu);
        assert!(ctx.status.contains("no horizon"));
    }

    #[test]
    fn all_screens_render_without_panics() {
        let app = crate::App::new(AppConfig::default().with_screen(320, 240));
        let mut buffer = RgbaBuffer::new(320, 240);

        let screens = [
            Screen::MainMenu,
            Screen::JoinList {
                games: vec![game("g1", "Alpha")],
                selected: 0,
            },
            Screen::CreateForm(CreateForm::default()),
            Screen::InSession {
                request: SessionRequest {
                    game_id: "g1".to_string(),
                    password: None,
                },
            },
        ];

        for screen in &screens {
            let view = ScreenView {
                width: 320,
                height: 240,
                status: "status",
                mouse: Some(Point::new(10, 10)),
                focus: 0,
                app: Some(&app),
            };
            screen.render(&mut buffer, view);

            assert_eq!(buffer.get(0, 0).map(|pixel| pixel.a), Some(255));
            assert!(
                buffer.pixels().chunks_exact(4).any(|pixel| pixel[3] != 0),
                "экран {screen:?} не нарисовал ни одного пикселя"
            );
        }

        // Без App (headless-рендер до сети) сессия всё равно рисует статус.
        let mut ctx2 = UiContext::new(Box::new(MockClient::new(Vec::new(), None).0), 64, 32);
        ctx2.status = "offline".to_string();
        let screen = Screen::InSession {
            request: SessionRequest {
                game_id: "g1".to_string(),
                password: None,
            },
        };
        let mut small = RgbaBuffer::new(64, 32);
        screen.render(&mut small, ctx2.view(None));
        assert!(small.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn create_params_carry_password_and_opponents() {
        let form = CreateForm {
            name: "  Password game  ".to_string(),
            world: 7,
            timer: 90,
            endturn: 15,
            opponent: 3,
            password: "sesame".to_string(),
            focus: CreateField::Name,
        };

        let params = form.create_params();
        assert_eq!(params.name.as_deref(), Some("Password game"));
        assert_eq!(params.world, Some(7));
        assert_eq!(params.min_players, Some(4));
        assert_eq!(params.password.as_deref(), Some("sesame"));
        assert_eq!(form.password_value().as_deref(), Some("sesame"));
    }

    /// M6.2: клик по кнопке меню вызывает `AudioPlayer::play_once(KBUY0)`.
    ///
    /// Клип кладётся в кэш (`preload`), поэтому тесту не нужны ни игровые
    /// данные, ни аудиоустройство — проверяется именно хук UI.
    #[test]
    fn menu_button_click_triggers_menu_sound() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let (mut ctx, _log) = context(Vec::new(), None);
        ctx.audio = AudioPlayer::with_backend(Box::new(CountingBackend {
            calls: std::sync::Arc::clone(&calls),
        }));
        ctx.audio.preload(
            MENU_CLICK,
            max_assets::WavClip {
                sample_rate: 22050,
                channels: 1,
                samples: vec![1, 2, 3],
            },
        );

        let ui = build_main_menu(ctx.width, ctx.height, &ctx.status, ctx.mouse, ctx.focus);
        let create = ui.widgets[MAIN_CREATE].rect().center();
        let mut screen = Screen::MainMenu;

        let transition = screen.handle_input(&mut ctx, InputEvent::MouseClick(create.x, create.y));

        assert_eq!(transition, Transition::None);
        assert!(matches!(screen, Screen::CreateForm(_)));
        assert_eq!(*calls.lock().expect("lock"), 1, "клик должен озвучиваться");
        assert_eq!(ctx.audio.plays(), 1);
    }
}
