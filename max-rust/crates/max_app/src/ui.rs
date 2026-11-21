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
//! | MainMenu | Up/Down/Tab, Enter, мышь; `c`/`j`/`i`/`r`/`q`; Esc — выход |
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

use max_core::enums::Planet;
use max_core::geo::{Point as GridPoint, MAP_HEIGHT, MAP_WIDTH};
use max_core::input::{Command, Packet18Chat};
use max_core::unit::UnitInfo;
use max_net::{CreateGameParams, GameInfo, HorizonClient, CLIENT_NAME, HORIZON_PROTOCOL_VERSION};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer, BYTES_PER_PIXEL};
use max_render::text::{draw_text_gnw, draw_text_line, GnwFontId, GLYPH_HEIGHT};
use max_render::ui::{
    hit_test, layout, render_ui, Button, Label, ListBox, Point, Rect, TextBox, UiState, UiTheme,
    Widget,
};
use max_render::viewport::Viewport;

use crate::audio::{AudioPlayer, MENU_CLICK};
use crate::credits;
use crate::helpmenu::{self, HelpAction, HelpPlacement, HelpSection};
use crate::hud::{self, HudButton};
use crate::intro;
use crate::landing::LandingAction;
use crate::local::LocalSetup;
use crate::menu;
use crate::mobilize::CargoAction;
use crate::panels::{self, PanelButton};
use crate::preferences::{
    self, Preferences, PreferencesDialog, PreferencesVariant,
};
use crate::pregame::{PreGameFlow, PreGameStage};
use crate::researchmenu::ResearchAction;
use crate::sfx::Sfx;
use crate::setup::{self, GameSetupForm, SetupTransition};
use crate::winloss;
use crate::App;

/// Имя игры по умолчанию, если поле «Name» пустое.
pub const DEFAULT_GAME_NAME: &str = "M.A.X. Rust game";
/// Последний допустимый индекс планеты (`0..=23`, как `--world`).
pub const MAX_WORLD: i64 = 23;

/// Уровни/индексы виджетов, на которые опирается обработка мыши.
///
/// Порядок как в `menu.cpp::main_menu`: `New Game`, `Load Game`,
/// `Multiplayer Game`, `Setup`, `Introduction`, `Credits`, `Exit`.
const MAIN_CREATE: usize = 0;
const MAIN_JOIN: usize = 2;
const MAIN_SETUP: usize = 3;
const MAIN_INTRO: usize = 4;
const MAIN_CREDITS: usize = 5;
const MAIN_QUIT: usize = 6;

/// Действия Rust-клиента в порядке фокуса (`ctx.focus`).
///
/// `Load Game` остаётся непортированным (`Unported`); `Setup` открывает диалог
/// `OptionsMenu(SETUPPIC)`, остальные действия — как в C++.
const MAIN_ACTIONS: [usize; 6] = [
    MAIN_CREATE,
    MAIN_JOIN,
    MAIN_SETUP,
    MAIN_INTRO,
    MAIN_CREDITS,
    MAIN_QUIT,
];

const JOIN_LIST: usize = 1;
const JOIN_BACK: usize = 2;
const JOIN_REFRESH: usize = 3;
const JOIN_JOIN: usize = 4;
const JOIN_HIDE: usize = 5;
const JOIN_HELP: usize = 6;

/// Кнопки формы создания игры (индексы виджетов).
const CREATE_BACK: usize = 7;
const CREATE_SUBMIT: usize = 8;
const CREATE_RANDOM: usize = 9;
const CREATE_HELP: usize = 10;

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
    /// Page Up (страницы списков, как `GNW_KB_KEY_PAGEUP`).
    PageUp,
    /// Page Down (`GNW_KB_KEY_PAGEDOWN`).
    PageDown,
    /// Печатный символ.
    Char(char),
    /// Печатный символ с зажатым Alt (`Alt+P` — пауза, `GNW_KB_KEY_LALT_P`).
    Alt(char),
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
    /// Начать локальную партию (M-SP) с этими настройками.
    StartLocal(Box<LocalSetup>),
    /// Выйти из сессии (Esc в SDL-цикле).
    LeaveSession,
    /// Применить настройки из диалога `Preferences`/`Setup` (главное меню).
    ApplyPreferences(Box<Preferences>),
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
        matches!(
            self,
            Self::World | Self::Timer | Self::EndTurn | Self::Opponent
        )
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
                    let value = self
                        .numeric(field)
                        .saturating_mul(10)
                        .saturating_add(i64::from(digit));
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
    /// `Multiplayer Menu` (`menu_multiplayer_menu_loop`): хост/join/hot-seat.
    MultiplayerMenu,
    /// `Introduction` (`Movie_PlayIntro`): MVE-видео `INTROFLC` (`maxint.mve`).
    Introduction(intro::IntroScreen),
    /// `Credits` (`menu_credits_menu_loop`): прокрутка титров по `CREDITS`.
    Credits(credits::CreditsScreen),
    /// Настройка локальной партии (M-SP/M-HS): Options → Planet → Players.
    LocalSetup(GameSetupForm),
    /// Диалог `Preferences`/`Setup` (`OptionsMenu`, `PREFSPIC`/`SETUPPIC`).
    ///
    /// Модален, как `OptionsMenu::Run` в C++: пока открыт, остальные экраны
    /// ввод не получают. Закрытие (`Done`/`Cancel`) возвращает в главное меню.
    Preferences(Box<PreferencesDialog>),
    /// Фазы старта после `Players`: мобилизация (`CargoMenu`) и выбор точки
    /// высадки (`GameManager_SelectSite`).
    PreGame(PreGameFlow),
    /// Экран победы/поражения локальной партии.
    GameOver,
    /// Модальное окно помощи поверх предыдущего экрана (`HelpMenu`).
    ///
    /// В C++ `HelpMenu` тоже модально: оно рисуется поверх меню/`CargoMenu`, а
    /// после закрытия управление возвращается прежнему экрану. Вложенный
    /// `screen` хранит именно его.
    Help {
        /// Экран под окном помощи.
        screen: Box<Screen>,
        /// Открытое окно помощи; `None` — активен режим выбора элемента
        /// (`HelpMenu_Menu`, `helpmenu.cpp:315`).
        help: Option<helpmenu::HelpScreen>,
        /// Секция режима помощи.
        mode: helpmenu::HelpMode,
    },
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
    /// Имя игрока по умолчанию (`--player`) для настройки M-SP.
    pub player_name: String,
    /// Каталог `.WRL`-карт для мини-карт экрана планет.
    pub world_dir: std::path::PathBuf,
    /// Звук (M6.2): клики меню. Без feature `sound` — тихий no-op.
    pub audio: AudioPlayer,
    /// Путь к видео интро (`INTROFLC` → `maxint.mve`); `None` — заглушка.
    pub intro_mve_path: Option<std::path::PathBuf>,
    /// Текущие пользовательские настройки (для диалога `Setup`).
    pub preferences: Preferences,
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
            player_name: "Player".to_string(),
            world_dir: std::path::PathBuf::from("max-original"),
            audio: AudioPlayer::new(),
            intro_mve_path: None,
            preferences: Preferences::default(),
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

/// Действие ввода на экранах фаз старта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreGameInput {
    /// Ничего.
    None,
    /// Отмена всей цепочки (назад на `Players`).
    Cancel,
    /// Клик на экране мобилизации (`CargoMenu`).
    MobilizeClick(i32, i32),
    /// Клавиша на экране мобилизации.
    MobilizeKey(Key),
    /// Подтвердить точку высадки.
    LandingConfirm,
    /// Сдвинуть курсор высадки.
    LandingStep(i32, i32),
    /// Клик по экрану высадки.
    LandingClick(i32, i32),
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

        // Режим/окно помощи (`HelpMenu_Menu` + `HelpMenu`): ввод получает
        // только он. Клик по элементу с записью `help.json` открывает окно;
        // `Esc`/RMB/`?` закрывают окно (возврат в режим) или сам режим.
        if matches!(self, Self::Help { .. }) {
            let mut exit = false;

            if let Self::Help { help, mode, .. } = self {
                match event {
                    InputEvent::MouseMove(..) => {}
                    InputEvent::MouseClick(x, y) => {
                        if let Some(window) = help.as_mut() {
                            if window.input(InputEvent::MouseClick(x, y)) == HelpAction::Close {
                                *help = None;
                            }
                        } else if let Some(window) = mode.click(x, y) {
                            *help = Some(window);
                            ctx.click();
                        }
                    }
                    InputEvent::RightClick(..) => {
                        if help.is_some() {
                            *help = None;
                        } else {
                            exit = true;
                        }
                    }
                    InputEvent::Key(Key::Escape) | InputEvent::Key(Key::Char('?')) => {
                        if help.is_some() {
                            *help = None;
                        } else {
                            exit = true;
                        }
                    }
                    InputEvent::Key(key) => {
                        if let Some(window) = help.as_mut() {
                            if window.input(InputEvent::Key(key)) == HelpAction::Close {
                                *help = None;
                            }
                        }
                    }
                }
            }

            if exit {
                if let Self::Help { screen, .. } = std::mem::replace(self, Self::MainMenu) {
                    *self = *screen;
                }
            }

            return Transition::None;
        }

        if matches!(self, Self::MainMenu) {
            return self.handle_main_menu(ctx, event);
        }
        if matches!(self, Self::Preferences(_)) {
            return self.handle_preferences(ctx, event);
        }
        if matches!(self, Self::JoinList { .. }) {
            return self.handle_join_list(ctx, event);
        }
        if matches!(self, Self::CreateForm(_)) {
            return self.handle_create_form(ctx, event);
        }
        if matches!(self, Self::MultiplayerMenu) {
            return self.handle_multiplayer_menu(ctx, event);
        }
        if matches!(self, Self::LocalSetup(_)) {
            return self.handle_local_setup(ctx, event);
        }
        if matches!(self, Self::PreGame(_)) {
            return self.handle_pregame(ctx, event);
        }
        if matches!(self, Self::GameOver) {
            return match event {
                InputEvent::Key(Key::Enter) | InputEvent::Key(Key::Escape) => {
                    Transition::LeaveSession
                }
                InputEvent::MouseClick(x, y) if winloss::ok_at(x, y) => Transition::LeaveSession,
                _ => Transition::None,
            };
        }
        if matches!(self, Self::Introduction(_)) {
            // MVE: как `mve_cb_ctl` в оригинале — любая клавиша/клик
            // прерывает и возвращает в главное меню.
            return match event {
                InputEvent::MouseMove(..) => Transition::None,
                _ => {
                    *self = Self::MainMenu;
                    ctx.status = "Introduction closed".to_string();
                    Transition::None
                }
            };
        }
        if matches!(self, Self::Credits(_)) {
            return self.handle_credits(ctx, event);
        }

        match event {
            InputEvent::Key(Key::Escape) => Transition::LeaveSession,
            _ => Transition::None,
        }
    }

    /// Один шаг анимации экрана (`dt` секунд; `dt <= 0` — экран стоит).
    ///
    /// Анимируется только [`Screen::Credits`] (прокрутка титров). Возвращает
    /// `true`, если показ закончился и экран должен вернуться в главное меню
    /// (в C++ `menu_credits_menu_loop` выходит по `height_offset <= bounds.uly`).
    pub fn advance(&mut self, dt: f32) -> bool {
        if let Self::Credits(screen) = self {
            screen.advance(dt);

            return screen.finished();
        }

        if let Self::Introduction(screen) = self {
            return screen.advance(dt);
        }

        false
    }

    /// Ввод на экране титров: `KP-`/`KP+` меняют скорость (в клиенте — `-`/`+`),
    /// любая другая клавиша или клик закрывает экран (`menu_credits_menu_loop`).
    fn handle_credits(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let Self::Credits(screen) = self else {
            return Transition::None;
        };
        let mut close = false;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::Key(Key::Char('+')) | InputEvent::Key(Key::Char('=')) => screen.faster(),
            InputEvent::Key(Key::Char('-')) | InputEvent::Key(Key::Char('_')) => screen.slower(),
            _ => close = true,
        }

        if close {
            // Возврат в главное меню (в C++ цикл титров просто завершается).
            ctx.status = "Credits closed".to_string();
            *self = Self::MainMenu;
        }

        Transition::None
    }

    /// Ввод на экране настройки M-SP (`Options` → `Planet` → `Players`).
    fn handle_local_setup(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let Self::LocalSetup(form) = self else {
            return Transition::None;
        };

        let transition = form.handle_input(event, ctx.width, ctx.height);
        let hot_seat = form.hot_seat;

        match transition {
            SetupTransition::None => Transition::None,
            SetupTransition::Back => {
                // Esc/Cancel возвращает туда, откуда пришли: в `Multiplayer
                // Menu` для hot-seat (как `menu_custom_game_menu`), иначе в
                // главное меню.
                let back = if hot_seat {
                    Self::MultiplayerMenu
                } else {
                    Self::MainMenu
                };
                *self = back;
                ctx.status = "New game cancelled".to_string();
                Transition::None
            }
            SetupTransition::Start(setup) => {
                // Фазы старта: мобилизация и выбор точки высадки — до партии,
                // как `GameManager_PlayerMissionSetup` в C++.
                let flow = PreGameFlow::start(form.clone(), *setup, &ctx.world_dir);
                let finished = flow.finished();
                let ready = flow.setup.clone();

                *self = Self::PreGame(flow);
                ctx.status.clear();

                if finished {
                    Transition::StartLocal(Box::new(ready))
                } else {
                    Transition::None
                }
            }
            SetupTransition::Help(section) => {
                // Кнопка `?` меню настройки: окно `HELPFRAM` поверх экрана
                // (`HelpMenu_Menu`); позиция — центр окна 640×480.
                let inner = std::mem::replace(self, Self::MainMenu);
                *self = Self::Help {
                    screen: Box::new(inner),
                    help: None,
                    mode: helpmenu::HelpMode::new(section, HelpPlacement::Menu),
                };
                ctx.click();
                Transition::None
            }
        }
    }

    /// Ввод на экранах фаз старта (`CargoMenu` и выбор точки высадки).
    fn handle_pregame(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let Self::PreGame(flow) = self else {
            return Transition::None;
        };

        // Действие считается по текущему экрану, применяется уже без borrow.
        let action = match &mut flow.stage {
            PreGameStage::Mobilize(_) => match event {
                InputEvent::MouseMove(..) | InputEvent::RightClick(..) => PreGameInput::None,
                InputEvent::MouseClick(x, y) => PreGameInput::MobilizeClick(x, y),
                InputEvent::Key(key) => PreGameInput::MobilizeKey(key),
            },
            PreGameStage::Landing(_) => match event {
                InputEvent::MouseMove(..) | InputEvent::RightClick(..) => PreGameInput::None,
                InputEvent::Key(Key::Escape) => PreGameInput::Cancel,
                InputEvent::Key(Key::Enter) => PreGameInput::LandingConfirm,
                InputEvent::Key(Key::Left) => PreGameInput::LandingStep(-1, 0),
                InputEvent::Key(Key::Right) => PreGameInput::LandingStep(1, 0),
                InputEvent::Key(Key::Up) => PreGameInput::LandingStep(0, -1),
                InputEvent::Key(Key::Down) => PreGameInput::LandingStep(0, 1),
                InputEvent::Key(_) => PreGameInput::None,
                InputEvent::MouseClick(x, y) => PreGameInput::LandingClick(x, y),
            },
            PreGameStage::Finished => PreGameInput::None,
        };

        let mut back_to_form = false;
        let mut confirmed = false;
        let mut open_help = false;

        match action {
            PreGameInput::None => {}
            PreGameInput::Cancel => back_to_form = true,
            PreGameInput::MobilizeClick(x, y) => {
                if let PreGameStage::Mobilize(mobilization) = &mut flow.stage {
                    match mobilization.handle_click(x, y) {
                        CargoAction::None => {}
                        CargoAction::Cancel => back_to_form = true,
                        CargoAction::Done => {
                            flow.commit_mobilization();
                            confirmed = true;
                        }
                        CargoAction::Help => open_help = true,
                    }
                }
            }
            PreGameInput::MobilizeKey(key) => {
                if let PreGameStage::Mobilize(mobilization) = &mut flow.stage {
                    match mobilization.handle_key(key) {
                        CargoAction::None => {}
                        CargoAction::Cancel => back_to_form = true,
                        CargoAction::Done => {
                            flow.commit_mobilization();
                            confirmed = true;
                        }
                        CargoAction::Help => open_help = true,
                    }
                }
            }
            PreGameInput::LandingConfirm => {
                let ok = if let PreGameStage::Landing(landing) = &mut flow.stage {
                    landing.confirm() == LandingAction::Confirm
                } else {
                    false
                };

                if ok {
                    flow.commit_landing();
                    confirmed = true;
                }
            }
            PreGameInput::LandingStep(dx, dy) => {
                if let PreGameStage::Landing(landing) = &mut flow.stage {
                    landing.step(dx, dy);
                }
            }
            PreGameInput::LandingClick(x, y) => {
                let result = if let PreGameStage::Landing(landing) = &mut flow.stage {
                    landing.click(x, y)
                } else {
                    LandingAction::None
                };

                match result {
                    LandingAction::Confirm => {
                        flow.commit_landing();
                        confirmed = true;
                    }
                    LandingAction::Cancel => back_to_form = true,
                    LandingAction::None => {}
                }
            }
        }

        if back_to_form {
            *self = Self::LocalSetup(flow.form.clone());
            ctx.status = "Start cancelled".to_string();

            return Transition::None;
        }

        if confirmed && flow.finished() {
            return Transition::StartLocal(Box::new(flow.setup.clone()));
        }

        if open_help {
            // `CargoMenu` фазы старта — состояние `GAME_STATE_6_GAME_SETUP`:
            // `GameManager_GetDialogWindowCenterMode()` даёт `WINDOW_MAIN_MAP`,
            // поэтому окно стоит в (253,126), как `HelpMenu` в игре.
            let inner = std::mem::replace(self, Self::MainMenu);
            *self = Self::Help {
                screen: Box::new(inner),
                help: None,
                mode: helpmenu::HelpMode::new(HelpSection::Cargo, HelpPlacement::Game),
            };
            ctx.click();
        }

        Transition::None
    }

    /// Ввод `Multiplayer Menu` (`menu_multiplayer_menu_loop`).
    ///
    /// Работают: `Join Network Game` (список игр Horizon), `Hot Seat Game`
    /// (M-HS — те же три экрана настройки, но `is_single_player = false`) и
    /// `Cancel`. `Host Network Game`, `Hot Seat Scenarios` и
    /// `Load Hot Seat Game` пока не портированы — пишут строку статуса.
    fn handle_multiplayer_menu(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let mut action = None;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                action = menu::multiplayer_button_at(x, y);
            }
            InputEvent::Key(Key::Escape) => action = Some(menu::MultiplayerAction::Cancel),
            // Горячие клавиши оригинала (`menu_multiplayer_menu_loop`):
            // Shift+H/J/O/T/L/C — хост, join, hot seat, сценарии, загрузка, отмена.
            InputEvent::Key(Key::Char('h' | 'H')) => action = Some(menu::MultiplayerAction::Host),
            InputEvent::Key(Key::Char('j' | 'J')) => action = Some(menu::MultiplayerAction::Join),
            InputEvent::Key(Key::Char('o' | 'O')) => {
                action = Some(menu::MultiplayerAction::HotSeat)
            }
            InputEvent::Key(Key::Char('t' | 'T')) => {
                action = Some(menu::MultiplayerAction::HotSeatScenarios)
            }
            InputEvent::Key(Key::Char('l' | 'L')) => {
                action = Some(menu::MultiplayerAction::LoadHotSeat)
            }
            InputEvent::Key(Key::Char('c' | 'C')) => action = Some(menu::MultiplayerAction::Cancel),
            _ => {}
        }

        if action.is_some() {
            ctx.click();
        }

        match action {
            Some(menu::MultiplayerAction::Host) => {
                // Создание игры в Horizon (в оригинале — `Host Network Game`
                // через `Remote_Lobby`).
                *self = Self::CreateForm(CreateForm::default());
                ctx.status = "Create a network game (Enter creates, Esc back)".to_string();
                Transition::None
            }
            Some(menu::MultiplayerAction::Join) => {
                self.refresh_games(ctx);
                Transition::None
            }
            Some(menu::MultiplayerAction::HotSeat) => {
                let mut form = GameSetupForm::hot_seat();
                // C++ `menu_choose_player_menu_loop`: имя человека берётся из
                // `settings["player_name"]` (`menu.cpp:1944`), а не из CLI.
                form.setup.player_name = ctx.preferences.player_name.clone();
                form.world_dir = ctx.world_dir.clone();
                *self = Self::LocalSetup(form);
                ctx.status = "Hot seat: 2-4 humans, Options -> Planet -> Players".to_string();
                Transition::None
            }
            Some(menu::MultiplayerAction::HotSeatScenarios) => {
                ctx.status = "Hot Seat Scenarios: screen is not ported yet".to_string();
                Transition::None
            }
            Some(menu::MultiplayerAction::LoadHotSeat) => {
                ctx.status = "Load Hot Seat Game: screen is not ported yet".to_string();
                Transition::None
            }
            Some(menu::MultiplayerAction::Cancel) => {
                *self = Self::MainMenu;
                ctx.status.clear();
                Transition::None
            }
            _ => Transition::None,
        }
    }

    /// Отрисовка экрана в кадр.
    ///
    /// Для [`Screen::InSession`] карта берётся из [`crate::App::render_frame`]
    /// (а он рисует `max_render::map_render`), сверху — строка статуса.
    pub fn render(&self, buffer: &mut RgbaBuffer, view: ScreenView<'_>) {
        // Меню на оригинальных ресурсах (пункт E) — только в кадре 640×480:
        // все координаты и картинки взяты из C++-порта для окна оригинала.
        let menu_assets = view
            .app
            .and_then(|app| app.assets())
            .and_then(|assets| assets.menu.as_ref());
        let native_frame =
            view.width as i32 == menu::MENU_WIDTH && view.height as i32 == menu::MENU_HEIGHT;
        let mouse = view.mouse.map(|point| (point.x, point.y));

        match self {
            Self::Help { screen, help, .. } => {
                // Окно помощи модально: сначала экран под ним, затем `HELPFRAM`.
                screen.render(buffer, view);

                let assets = view.app.and_then(|app| app.assets());
                let fallback = assets.and_then(|assets| {
                    assets
                        .menu
                        .as_ref()
                        .map(|menu| menu.palette)
                        .or_else(|| panels::UiPalette::from_assets(assets))
                });

                if let Some(help) = help {
                    help.draw(
                        buffer,
                        assets.and_then(|assets| assets.help.as_ref()),
                        fallback,
                    );
                }
            }
            Self::MainMenu => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    menu::draw_main_menu(buffer, menu_assets, view.status, mouse);
                    return;
                }

                let ui =
                    build_main_menu(view.width, view.height, view.status, view.mouse, view.focus);
                render_ui(buffer, &ui);
            }
            Self::JoinList { games, selected } => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    menu::draw_join_list(buffer, menu_assets, games, *selected, view.status, mouse);
                    return;
                }

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
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    menu::draw_create_form(
                        buffer,
                        menu_assets,
                        view.app.and_then(|app| app.assets()),
                        form,
                        view.status,
                        mouse,
                    );
                    return;
                }

                let ui = build_create_form(view.width, view.height, view.status, view.mouse, form);
                render_ui(buffer, &ui);
                draw_planet_picker(
                    buffer,
                    view.app.and_then(|app| app.assets()),
                    form,
                    view.width,
                    view.height,
                );
            }
            Self::MultiplayerMenu => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    menu::draw_multiplayer_menu(buffer, menu_assets, view.status, mouse);
                    return;
                }
                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Multiplayer Menu uses the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::Introduction(screen) => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    screen.draw(buffer, menu_assets);
                    return;
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Introduction uses the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::Credits(screen) => {
                let background = menu_assets.and_then(|menu| menu.credits.as_ref());

                if let (Some(background), true) = (background, native_frame) {
                    screen.draw(buffer, background);
                    return;
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Credits need the original 640x480 frame and the CREDITS image",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::PreGame(flow) => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    match &flow.stage {
                        PreGameStage::Mobilize(mobilization) => {
                            // `CargoMenu` рисует иконки `I_*` и полосы — нужен
                            // полный набор ассетов, а не только меню.
                            if let Some(assets) = view.app.and_then(|app| app.assets()) {
                                mobilization.draw(buffer, assets, view.status, mouse);
                                return;
                            }
                        }
                        PreGameStage::Landing(landing) => {
                            let assets = view.app.and_then(|app| app.assets());
                            let chrome = assets.and_then(|assets| assets.chrome.as_ref());
                            let cursors = assets.and_then(|assets| assets.cursors.as_ref());
                            landing.draw(buffer, menu_assets, cursors, chrome, view.status, mouse);
                            return;
                        }
                        PreGameStage::Finished => {
                            buffer.fill(Rgba::rgb(12, 14, 18));
                        }
                    }
                    return;
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Start phases use the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::LocalSetup(form) => {
                if let (Some(menu_assets), true) = (menu_assets, native_frame) {
                    setup::draw_setup(buffer, menu_assets, form, view.status, mouse);
                    return;
                }

                // Кадр не 640×480: экраны настройки нарисованы в координатах
                // оригинала, поэтому показываем честную подсказку.
                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Setup screens use the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::Preferences(dialog) => {
                // `OptionsMenu` — отдельное окно поверх главного меню: в C++
                // фон меню остаётся видимым вокруг `PREFSPIC`.
                if let (Some(menu_assets), Some(app), true) = (menu_assets, view.app, native_frame) {
                    menu::draw_main_menu(buffer, menu_assets, view.status, mouse);

                    if let Some(assets) = app.assets() {
                        dialog.draw(buffer, assets);
                        return;
                    }
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(
                    buffer,
                    12,
                    12,
                    "Preferences use the original 640x480 frame",
                    Rgba::rgb(232, 236, 244),
                );
            }
            Self::GameOver => {
                if let (Some(menu_assets), Some(app)) = (menu_assets, view.app) {
                    if let Some(local) = app.local() {
                        winloss::draw_game_over(buffer, menu_assets, app.state(), local, mouse);
                        return;
                    }
                }

                buffer.fill(Rgba::rgb(12, 14, 18));
                draw_text_line(buffer, 12, 12, view.status, Rgba::rgb(232, 236, 244));
            }
            Self::InSession { .. } => render_session(buffer, view),
        }
    }

    fn handle_main_menu(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let ui = build_main_menu(ctx.width, ctx.height, &ctx.status, ctx.mouse, ctx.focus);
        let mut activate: Option<usize> = None;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                let point = Point::new(x, y);

                if let Some(index) = hit_test(&ui, point) {
                    if let Some(action_index) = MAIN_ACTIONS.iter().position(|&item| item == index)
                    {
                        ctx.focus = action_index;
                        activate = Some(index);
                    } else if let Some(button) = menu::MAIN_MENU_BUTTONS.get(index) {
                        // Кнопки оригинала, которых в Rust-клиенте ещё нет
                        // (`Load Game`, `Setup`, …): не молчим, а пишем статус.
                        ctx.status = format!("{}: screen is not ported yet", button.label);
                        ctx.click();
                    }
                }
            }
            InputEvent::Key(key) => match key {
                Key::Up => {
                    let count = MAIN_ACTIONS.len();
                    ctx.focus = (ctx.focus + count - 1) % count;
                }
                Key::Down | Key::Tab => ctx.focus = (ctx.focus + 1) % MAIN_ACTIONS.len(),
                Key::Enter => {
                    activate = Some(MAIN_ACTIONS[ctx.focus.min(MAIN_ACTIONS.len() - 1)]);
                }
                Key::Escape => return Transition::Quit,
                Key::Char(character) => match character.to_ascii_lowercase() {
                    'c' => activate = Some(MAIN_CREATE),
                    'j' => activate = Some(MAIN_JOIN),
                    's' => activate = Some(MAIN_SETUP),
                    'i' => activate = Some(MAIN_INTRO),
                    'r' => activate = Some(MAIN_CREDITS),
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
                // M-SP: «New Game» открывает настройку локальной партии
                // (`menu_custom_game_menu`: Options → Planet → Players).
                let mut form = GameSetupForm::default();
                form.setup.player_name = ctx.preferences.player_name.clone();
                form.world_dir = ctx.world_dir.clone();
                *self = Self::LocalSetup(form);
                ctx.status = "New game: options, Enter — next screen, Esc — back".to_string();
                Transition::None
            }
            Some(MAIN_JOIN) => {
                // `Multiplayer Game` → `Multiplayer Menu` (`menu.cpp:2361`):
                // Host/Join/Hot Seat Game/Hot Seat Scenarios/Load Hot Seat/Cancel.
                *self = Self::MultiplayerMenu;
                ctx.status = "Multiplayer: Host, Join or Hot Seat Game".to_string();
                Transition::None
            }
            Some(MAIN_SETUP) => {
                // `menu_setup_window()` (`menu.cpp:2331`) → `OptionsMenu(SETUPPIC)`:
                // только секция `setup` (звук/видео/автосейв/сеть/имя), поверх
                // главного меню, как отдельное окно `PREFSPIC`.
                let dialog = PreferencesDialog::new(
                    PreferencesVariant::Setup,
                    ctx.preferences.clone(),
                    preferences::PreferencesInfo::default(),
                );
                *self = Self::Preferences(Box::new(dialog));
                ctx.status = "Setup: Done saves, Cancel reverts".to_string();
                Transition::None
            }
            Some(MAIN_INTRO) => {
                // `Movie_PlayIntro` (`menu.cpp:2666`): MVE-видео `INTROFLC`
                // (файл `maxint.mve`). Кадр 0 декодируется сразу, чтобы
                // `shot`/`dt = 0` были детерминированы; Esc/клик — в меню.
                let screen = intro::IntroScreen::new(ctx.intro_mve_path.clone());
                ctx.status = if screen.is_loaded() {
                    format!("Introduction: maxint.mve — кадр {} (Esc)", screen.frame_index())
                } else if let Some(error) = screen.error() {
                    format!("Introduction: {error} — Esc returns")
                } else {
                    "Introduction: maxint.mve не найден — Esc returns".to_string()
                };
                *self = Self::Introduction(screen);
                Transition::None
            }
            Some(MAIN_CREDITS) => {
                // `menu_credits_menu_loop` (`menu.cpp:1332`) — прокрутка титров
                // по `CREDITS` 640×480; Esc/любая клавиша закрывает экран.
                *self = Self::Credits(credits::CreditsScreen::new());
                ctx.status = "Credits: Esc returns to the menu".to_string();
                Transition::None
            }
            Some(MAIN_QUIT) => Transition::Quit,
            _ => Transition::None,
        }
    }

    /// Ввод в диалоге `Preferences`/`Setup` (`OptionsMenu`).
    ///
    /// `Done` сохраняет значения и возвращает в главное меню; `Cancel` их
    /// отбрасывает; `?` открывает окно помощи `SETUP_MENU_SETUP` поверх диалога
    /// (`HelpMenu_Menu` в `OptionsMenu::ProcessKeyPress`).
    fn handle_preferences(&mut self, ctx: &mut UiContext, event: InputEvent) -> Transition {
        let Self::Preferences(dialog) = self else {
            return Transition::None;
        };

        match dialog.input(event) {
            preferences::PreferencesAction::None => Transition::None,
            preferences::PreferencesAction::Changed => {
                ctx.preferences = dialog.draft.clone();
                Transition::None
            }
            preferences::PreferencesAction::Help => {
                let section = dialog.variant().help_section();
                let inner = std::mem::replace(self, Self::MainMenu);
                *self = Self::Help {
                    screen: Box::new(inner),
                    help: None,
                    mode: helpmenu::HelpMode::new(section, HelpPlacement::Menu),
                };
                ctx.click();
                Transition::None
            }
            preferences::PreferencesAction::Done => {
                let prefs = dialog.draft.clone();
                ctx.preferences = prefs.clone();
                // Следующая партия начнётся с этим именем (`menu.cpp:1944`).
                ctx.player_name = prefs.player_name.clone();
                ctx.status = "Preferences saved".to_string();
                *self = Self::MainMenu;
                ctx.click();
                Transition::ApplyPreferences(Box::new(prefs))
            }
            preferences::PreferencesAction::Cancel => {
                ctx.preferences = dialog.initial.clone();
                ctx.status = "Preferences cancelled".to_string();
                *self = Self::MainMenu;
                Transition::None
            }
        }
    }

    fn refresh_games(&mut self, ctx: &mut UiContext) {
        match ctx.client.list_games() {
            Ok(games) => {
                let count = games.len();
                *self = Self::JoinList { games, selected: 0 };
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
        let native_frame =
            ctx.width == menu::MENU_WIDTH as u32 && ctx.height == menu::MENU_HEIGHT as u32;

        enum Action {
            Select(usize),
            Back,
            Refresh,
            Join,
            Help,
            Create,
        }

        let mut action = None;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                if native_frame {
                    if let Some(index) = menu::join_row_at(x, y, games.len(), selected) {
                        action = Some(Action::Select(index));
                    } else if let Some(button) = menu::join_button_at(x, y) {
                        action = Some(match button {
                            menu::JoinButton::Refresh => Action::Refresh,
                            menu::JoinButton::Back | menu::JoinButton::Hide => Action::Back,
                            menu::JoinButton::Help => Action::Help,
                            menu::JoinButton::Join => Action::Join,
                        });
                    }
                } else {
                    let point = Point::new(x, y);

                    match hit_test(&ui, point) {
                        Some(JOIN_LIST) => {
                            if let Some(list) = ui.list_box(JOIN_LIST) {
                                if let Some(index) = list.row_at(point) {
                                    action = Some(Action::Select(index));
                                }
                            }
                        }
                        Some(JOIN_BACK | JOIN_HIDE) => action = Some(Action::Back),
                        Some(JOIN_REFRESH) => action = Some(Action::Refresh),
                        Some(JOIN_JOIN) => action = Some(Action::Join),
                        Some(JOIN_HELP) => action = Some(Action::Help),
                        _ => {}
                    }
                }
            }
            InputEvent::Key(Key::Up) => {
                action = Some(Action::Select(selected.saturating_sub(1)));
            }
            InputEvent::Key(Key::Down) => {
                action = Some(Action::Select(
                    (selected + 1).min(games.len().saturating_sub(1)),
                ));
            }
            InputEvent::Key(Key::Enter) => action = Some(Action::Join),
            InputEvent::Key(Key::Escape) => action = Some(Action::Back),
            InputEvent::Key(Key::Char('r' | 'R')) => action = Some(Action::Refresh),
            // Создание сетевой игры (форма `CreateForm`) — с экрана списка игр:
            // в главном меню эта кнопка теперь открывает настройку M-SP.
            InputEvent::Key(Key::Char('c' | 'C')) => action = Some(Action::Create),
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
            Some(Action::Help) => {
                ctx.status = "Help: screen is not ported yet".to_string();
                Transition::None
            }
            Some(Action::Create) => {
                *self = Self::CreateForm(CreateForm::default());
                ctx.status = "Create a network game (Enter creates, Esc back)".to_string();
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
        let native_frame =
            ctx.width == menu::MENU_WIDTH as u32 && ctx.height == menu::MENU_HEIGHT as u32;

        let mut back = false;
        let mut submit = false;
        let mut clicked = false;

        match event {
            InputEvent::MouseMove(..) => {}
            InputEvent::RightClick(..) => {}
            InputEvent::MouseClick(x, y) => {
                // Сетка планет: клик по картинке/имени выбирает планету. Это
                // делается до `hit_test`, потому что подписи планет — обычные
                // Label'ы, а числовое поле остаётся fallback'ом по Tab.
                let planet = (ctx.height >= PLANET_GRID_MIN_HEIGHT)
                    .then(|| planet_cell_at(ctx.width, ctx.height, x, y))
                    .flatten();

                if let Some(index) = planet {
                    if let Self::CreateForm(form) = self {
                        form.world = index as i64;
                        form.focus = CreateField::World;
                    }
                    clicked = true;
                } else if native_frame {
                    // Оригинальные прямоугольники PLANETSE / кнопок MNUBTN.
                    if let Some(button) = menu::create_button_at(x, y) {
                        clicked = true;

                        match button.action {
                            menu::MenuButtonAction::Cancel => back = true,
                            menu::MenuButtonAction::Done => submit = true,
                            menu::MenuButtonAction::Random => {
                                if let Self::CreateForm(form) = self {
                                    form.world = random_planet(form.world);
                                    form.focus = CreateField::World;
                                }
                            }
                            menu::MenuButtonAction::Help => {
                                ctx.status = "Help: screen is not ported yet".to_string();
                            }
                        }
                    } else if let Some(field) = menu::create_field_at(x, y) {
                        clicked = true;

                        if let Self::CreateForm(form) = self {
                            form.focus = field;
                        }
                    }
                } else {
                    let point = Point::new(x, y);

                    match hit_test(&ui, point) {
                        Some(CREATE_BACK) => {
                            back = true;
                            clicked = true;
                        }
                        Some(CREATE_SUBMIT) => {
                            submit = true;
                            clicked = true;
                        }
                        Some(CREATE_RANDOM) => {
                            clicked = true;
                            if let Self::CreateForm(form) = self {
                                form.world = random_planet(form.world);
                                form.focus = CreateField::World;
                            }
                        }
                        Some(CREATE_HELP) => {
                            clicked = true;
                            ctx.status = "Help: screen is not ported yet".to_string();
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
                        Key::PageUp | Key::PageDown | Key::Alt(_) | Key::Other => {}
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
    let chrome = app.chrome_active();

    // Мышь для краевого скролла камеры (`Preferences`: `click_scroll`):
    // `GameManager_ProcessInput` помнит позицию и кнопки между кадрами.
    match event {
        InputEvent::MouseMove(x, y) => app.note_mouse_move(x, y),
        InputEvent::MouseClick(x, y) => app.note_mouse_click(x, y),
        _ => {}
    }

    // `Alt+P` — `PauseMenu_Menu` (`game_manager.cpp:5542`): диалог «Game Paused»
    // поверх всего, тики локальной партии останавливаются.
    if matches!(event, InputEvent::Key(Key::Alt('p' | 'P'))) {
        if app.pause_dialog().is_some() {
            return Transition::None;
        }
        if app.local_active() && app.open_pause_dialog() {
            app.hud_mut().push_log("Paused");
        }
        return Transition::None;
    }

    if app.pause_dialog().is_some() {
        return session_pause_input(app, event);
    }

    // M-HS: окно перехода хода модально — как `DialogMenu::Run()` в C++, оно
    // принимает только подтверждение (клик по `OK`, Enter, Esc). Мышь двигается
    // (подсветка кнопки), всё остальное игнорируется, включая чат и приказы.
    if app.local_notice().is_some() {
        return match event {
            InputEvent::Key(Key::Enter) | InputEvent::Key(Key::Escape) => {
                app.local_acknowledge_turn();
                Transition::None
            }
            InputEvent::MouseClick(x, y) if crate::hotseat::ok_at(x, y) => {
                app.local_acknowledge_turn();
                Transition::None
            }
            _ => Transition::None,
        };
    }

    // Модальные окна: помощь (`HelpMenu`) — самое верхнее, затем экран
    // исследований, затем `BuildMenu` (как `GameManager_DisableMainMenu` в C++):
    // карта/панели/чат приказов не получают ввод, пока они открыты.
    if app.help_screen().is_some() {
        return session_help_input(app, event);
    }

    // Режим помощи C++ (`HelpMenu_Menu`): модален, клик по элементу с записью
    // `help.json` открывает окно; `Esc`/RMB/`?` выключают режим.
    if app.help_mode().is_some() {
        return session_help_mode_input(app, event);
    }

    // Самоуничтожение здания (`UnitsManager_SelfDestructMenu`) — модальное окно
    // поверх всего, включая меню команд (`GameManager_DeinitPopupButtons`
    // вызывается до открытия).
    if app.self_destruct().is_some() {
        return session_self_destruct_input(app, event, online);
    }

    // `Preferences` (`OptionsMenu::Run`, `PREFSPIC`) — модальный диалог: карта и
    // панели ввода не получают (`GameManager_DisableMainMenu`).
    if app.preferences_dialog().is_some() {
        return session_preferences_input(app, event);
    }

    // Экран исследований (P7) — модальный, как `ResearchMenu::Run`.
    if app.research_menu().is_some() {
        return session_research_menu_input(app, event);
    }

    // Распределение добычи шахты (C++ `AllocMenu`, окно `ALLOCFRM`) —
    // модальный диалог: карта/панели ввода не получают.
    if app.alloc_menu().is_some() {
        return session_alloc_menu_input(app, event, online);
    }

    // X-FER (C++ `TransferMenu`, окно `XFERPIC`) — модальное окно количества:
    // `GameManager_DisableMainMenu` вокруг `TransferMenu::Run`.
    if app.transfer_menu().is_some() {
        return session_transfer_menu_input(app, event, online);
    }

    if app.build_menu().is_some() {
        return session_build_menu_input(app, event, online);
    }

    // Экран покупных апгрейдов (P6) — модальный, как `UpgradeMenu::Run`.
    if app.upgrade_menu().is_some() {
        return session_upgrade_menu_input(app, event, online);
    }

    // Окно сохранения/загрузки V70 (P3) тоже модально: пока оно открыто,
    // карта/панели/чат приказов не получают (`GameManager_DisableMainMenu`).
    if app.save_load().is_some() {
        return session_saveload_input(app, event);
    }

    // Меню команд выделенного юнита (C++ `GameManager_PopupButtons`) — модально:
    // пока оно открыто, карта/панели/чат приказ не получают, а клик вне меню
    // или Esc его закрывают (как `GameManager_DeinitPopupButtons`).
    if app.unit_menu().is_some() {
        return session_unit_menu_input(app, event, online);
    }

    // Пока ходит компьютер/идёт переход, приказы и выделение невозможны:
    // в C++ карта в это время не перерисовывается (`game_manager.cpp:1179`),
    // а приказы отбрасываются по `GameManager_ActiveTurnTeam`.
    let blocked = app.local_active() && app.local_input_blocked();

    // Пока открыта строка чата (M4.4), клавиатура принадлежит ей: печатные
    // ASCII-символы идут в буфер, Backspace/Enter/Esc управляют вводом. Мышь
    // игнорируется, чтобы клик во время набора не отдал приказ.
    if app.hud().chat.is_active() {
        return match event {
            InputEvent::Key(key) => session_chat_key(app, key, online),
            _ => Transition::None,
        };
    }

    // Режим выбора места стройки (`ORDER_STATE_SELECT_SITE`) — порт
    // `GameManager_GetBuilderUnitCursor`/`PathBuild`/`ManagePlayerAction`:
    // LMB ставит площадку в клетку под курсором, Esc/RMB отменяют. Панели C++
    // в это время остаются включёнными, поэтому клики мимо карты уходят
    // обычным путём (`None`). Модальные экраны и чат выше важнее.
    if app.build_site().is_some() && !blocked {
        if let Some(transition) = session_build_site_input(app, event, online, chrome) {
            return transition;
        }
    }

    match event {
        InputEvent::MouseMove(x, y) => {
            if chrome {
                // Панели: подпись/подсветка и клетка под курсором для нижней
                // строки координат.
                app.hud_mut().hover_button = None;
                app.hud_mut().hover_panel = panels::panel_button_at(x, y);
                let grid = panels::screen_to_map(x, y).and_then(|(map_x, map_y)| {
                    let (grid_x, grid_y) = app.map_viewport().screen_to_grid(map_x, map_y);

                    (grid_x >= 0 && grid_y >= 0 && grid_x < MAP_WIDTH && grid_y < MAP_HEIGHT)
                        .then_some((grid_x, grid_y))
                });

                app.hud_mut().panels.cursor_grid = grid;
            } else {
                app.hud_mut().hover_button = hud::hud_button_at(width, height, x, y);
            }

            Transition::None
        }
        InputEvent::MouseClick(x, y) => {
            if chrome {
                // Панели/миникарта — только просмотр, когда ход не наш; карта
                // (выделение/атака) заблокирована.
                if blocked && panels::panel_button_at(x, y).is_none() {
                    return Transition::None;
                }
                session_chrome_left_click(app, x, y, online)
            } else if blocked {
                Transition::None
            } else {
                session_left_click(app, width, height, x, y, online)
            }
        }
        InputEvent::RightClick(x, y) => {
            if blocked {
                return Transition::None;
            }
            if chrome {
                session_chrome_right_click(app, x, y, online)
            } else {
                session_right_click(app, x, y, online)
            }
        }
        InputEvent::Key(Key::Char('p' | 'P')) if app.local_active() => {
            // Пауза локальной партии (`PauseMenu`): тики и таймер стоят.
            app.local_toggle_pause();
            Transition::None
        }
        // `B` — кнопка `Build` панели юнита (`GameManager_MenuClickBuildButton`
        // → `BuildMenu_Menu`): открывает заказ для выделенного строителя.
        InputEvent::Key(Key::Char('b' | 'B')) if !blocked => {
            if app.open_build_menu() {
                app.hud_mut().mouse_position = None;
                app.hud_mut().push_log("Build menu");
            } else {
                app.hud_mut()
                    .push_log("Build: select a constructor, engineer or factory first");
            }
            Transition::None
        }
        // `O` — питание выделенного юнита (попап `Power On`/`Power Off` в C++;
        // для `RESEARCH` включается исследование темы по умолчанию).
        InputEvent::Key(Key::Char('o' | 'O')) if !blocked => {
            let Some(id) = app.hud().selected else {
                app.hud_mut().push_log("Power: select a unit first");
                return Transition::None;
            };
            let Some(command) = hud::build_power_order(app.state(), id) else {
                app.hud_mut().push_log("Power: unit cannot be powered");
                return Transition::None;
            };
            let label = "Power toggled";
            app.hud_mut().pending.push(command);
            app.hud_mut().push_log(label);
            if !online {
                apply_pending_orders(app);
            }
            Transition::None
        }
        // `U` — экран апгрейдов (`UpgradeMenu`): пункт `buy upg` меню команд
        // (C++ `UnitsManager_Popup_OnClick_BuyUpgrade`); клавиша — быстрый путь.
        InputEvent::Key(Key::Char('u' | 'U')) if !blocked => {
            if app.open_upgrade_menu() {
                app.hud_mut().mouse_position = None;
                app.hud_mut().push_log("Upgrade menu");
            } else {
                app.hud_mut()
                    .push_log("Upgrade: select your unit (or gold refinery) first");
            }
            Transition::None
        }
        // `R` — экран исследований (`ResearchMenu`): пункт `resrch` меню команд
        // (C++ `UnitsManager_Popup_OnClick_Research`); клавиша — быстрый путь.
        InputEvent::Key(Key::Char('r' | 'R')) if !blocked => {
            if app.open_research_menu() {
                app.hud_mut().mouse_position = None;
                app.hud_mut().push_log("Research menu");
            } else {
                app.hud_mut().push_log(
                    "Research: select a powered Research Center of your team (O to power)",
                );
            }
            Transition::None
        }
        // `?` — режим помощи C++ (`GNW_KB_KEY_SHIFT_DIVIDE`): окно
        // `GAME_SCREEN_SETUP` поверх игры (панель `HELP_OF` делает то же).
        InputEvent::Key(Key::Char('?')) if !blocked => {
            app.open_help(HelpSection::GameScreen);
            app.hud_mut().push_log("Help: game screen");
            Transition::None
        }
        InputEvent::Key(Key::Enter) | InputEvent::Key(Key::Char('t' | 'T')) if !blocked => {
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
        // `S` — сохранение партии, `L` — загрузка (C++ `Alt+F`/`Alt+L`,
        // `GameManager_MenuClickFileButton`). В сети сохранение/загрузка
        // недоступны (пакет 16 не портирован) — `App::open_*_menu` пишет
        // объяснение в журнал.
        InputEvent::Key(Key::Char('s' | 'S')) if !blocked => {
            app.open_save_menu(online);
            Transition::None
        }
        InputEvent::Key(Key::Char('l' | 'L')) if !blocked => {
            app.open_load_menu(online);
            Transition::None
        }
        InputEvent::Key(_) => Transition::None,
    }
}

/// Ввод в диалоге самоуничтожения (`UnitsManager_SelfDestructMenu`).
///
/// Окно модально: `Arm`/`Enter` запускает анимацию `SLFDOPN1..6`, после неё
/// `OK`/`Enter` отдаёт `ORDER_EXPLODE`, `Esc`/`Cancel` закрывает без приказа
/// (C++ `UnitsManager_SelfDestructMenu` возвращает `false`).
fn session_self_destruct_input(
    app: &mut App,
    event: InputEvent,
    online: bool,
) -> Transition {
    let unit = app.self_destruct().map(|menu| menu.unit);
    let stage_before = app.self_destruct().map(|menu| menu.stage);

    match app.self_destruct_input(event) {
        Some(crate::selfdestruct::SelfDestructAction::None) => {
            // `Arm` (`units_manager.cpp:312`): sfx `MBUTT0`.
            if stage_before == Some(crate::selfdestruct::Stage::Confirm)
                && app
                    .self_destruct()
                    .is_some_and(|menu| menu.stage == crate::selfdestruct::Stage::Armed)
            {
                app.play_sfx(Sfx::Button);
            }
        }
        Some(crate::selfdestruct::SelfDestructAction::Cancel) => {
            // `NCANC0` (`units_manager.cpp:278/319`).
            app.play_sfx(Sfx::Cancel);
            app.hud_mut().push_log("Self destruct: cancelled");
        }
        Some(crate::selfdestruct::SelfDestructAction::Explode) => {
            // `SLFDOK` sfx `NDONE0` (`units_manager.cpp:254`).
            app.play_sfx(Sfx::Done);

            if !online {
                apply_pending_orders(app);
            }
            let name = unit
                .and_then(|unit| app.state().unit(unit))
                .map(unit_label)
                .unwrap_or_else(|| "unit".to_string());
            app.hud_mut()
                .push_log(format!("Self destruct: {name} — ORDER_EXPLODE"));
        }
        None => {}
    }

    Transition::None
}

/// Ввод в диалоге паузы (`PauseMenu_Menu` → `DialogMenu`, `Alt+P`).
fn session_pause_input(app: &mut App, event: InputEvent) -> Transition {
    match app.pause_dialog_input(event) {
        Some(crate::dialog::DialogAction::Close) => {
            app.hud_mut().push_log("Resumed");
        }
        _ => {}
    }

    Transition::None
}

/// Ввод в открытом меню команд выделенного юнита (C++ `GameManager_PopupButtons`).
///
/// Меню модально: `Esc`/клик вне/правый клик закрывают его
/// (`GameManager_DeinitPopupButtons`), клик по пункту и его горячая клавиша
/// (`GameManager_ProcessPopupMenuInput`) вызывают обработчик пункта
/// (`App::activate_unit_menu_item`). Стрелки и Enter в меню C++ не работают —
/// здесь тоже (они уходят в `MenuEvent::None`).
fn session_unit_menu_input(app: &mut App, event: InputEvent, online: bool) -> Transition {
    let Some(outcome) = app.unit_menu_input(event) else {
        return Transition::None;
    };

    match outcome {
        crate::unitmenu::MenuEvent::None | crate::unitmenu::MenuEvent::Hover(_) => {}
        crate::unitmenu::MenuEvent::Close => {
            app.close_unit_menu();
            app.hud_mut().push_log("Unit menu closed");
        }
        crate::unitmenu::MenuEvent::Item(index) => {
            // `GameManager_ProcessPopupMenuInput` (`game_manager.cpp:2689`):
            // пункт попапа — `PlaySfx(KCARG0)`.
            app.play_sfx(Sfx::Click);

            if let Some(line) = app.activate_unit_menu_item(index) {
                app.hud_mut().push_log(line);

                if !online {
                    apply_pending_orders(app);
                }
            }
        }
    }

    Transition::None
}

/// Ввод в открытом диалоге распределения добычи шахты (C++ `AllocMenu`).
///
/// Диалог модальный (C++ `GameManager_DisableMainMenu` вокруг `AllocMenu::Run`):
/// клики по карте/панелям приказов не дают. `Done`/`Esc`/`KP_Enter` закрывают
/// окно — значения уже применены кликами (`UpdateRawAllocation`), а на выходе
/// C++ рассылает `Remote_SendNetPacket_11` → `ORDER_NEW_ALLOCATE` (08/P5) на
/// каждую шахту комплекса ([`App::queue_alloc_orders`]).
fn session_alloc_menu_input(app: &mut App, event: InputEvent, online: bool) -> Transition {
    let Some(action) = app.alloc_menu_input(event) else {
        return Transition::None;
    };

    match action {
        crate::allocmenu::AllocAction::None => {}
        crate::allocmenu::AllocAction::Help => {
            app.open_help(HelpSection::Allocate);
            app.hud_mut().push_log("Allocate help");
        }
        crate::allocmenu::AllocAction::Close => {
            let queued = app.queue_alloc_orders();

            if let Some(line) = app.close_alloc_menu() {
                app.hud_mut().push_log(line);
            }
            if queued > 0 && !online {
                apply_pending_orders(app);
            }
        }
    }

    Transition::None
}

/// Ввод в открытом окне X-FER (C++ `TransferMenu::Run`).
///
/// Окно модально (`GameManager_DisableMainMenu`): карта приказ не получает.
/// `Done`/`Enter` отдаёт `ORDER_TRANSFER` (08/**P3**, `transfer_cargo`),
/// `Esc`/`Cancel` закрывает без приказа, `?` открывает `TRANSFER_SETUP`
/// поверх окна (в C++ `HelpMenu_Menu` возвращается в `TransferMenu`).
fn session_transfer_menu_input(app: &mut App, event: InputEvent, online: bool) -> Transition {
    let Some(action) = app.transfer_menu_input(event) else {
        return Transition::None;
    };

    match action {
        crate::transfermenu::TransferAction::None => {}
        crate::transfermenu::TransferAction::Help => {
            app.open_help(HelpSection::Transfer);
            app.hud_mut().push_log("Transfer help");
        }
        crate::transfermenu::TransferAction::Cancel => {
            if let Some(line) = app.cancel_transfer() {
                app.hud_mut().push_log(line);
            }
        }
        crate::transfermenu::TransferAction::Confirm => {
            if let Some((line, queued)) = app.confirm_transfer() {
                app.hud_mut().push_log(line);

                // Офлайн приказ исполняется сразу (как остальные приказы из
                // меню); в сети он уходит пакетом 08/P3.
                if queued && !online {
                    apply_pending_orders(app);
                }
            }
        }
    }

    Transition::None
}
///
/// Действия окна — [`crate::SaveLoadAction`]: записать слот, загрузить слот или
/// закрыть. Ошибки файлового I/O остаются в строке статуса окна, чтобы игрок
/// видел причину, а не молчаливое закрытие.
fn session_saveload_input(app: &mut App, event: InputEvent) -> Transition {
    let Some(menu) = app.save_load_mut() else {
        return Transition::None;
    };
    let action = menu.input(event);

    match action {
        crate::SaveLoadAction::None => Transition::None,
        crate::SaveLoadAction::Cancel => {
            app.close_save_load();
            app.hud_mut().push_log("Save/Load cancelled");
            Transition::None
        }
        crate::SaveLoadAction::Save { slot, name } => {
            match app.save_game_to_slot(slot, &name) {
                Ok(path) => {
                    app.close_save_load();
                    app.hud_mut().push_log(format!("Saved {}", path.display()));
                }
                Err(error) => {
                    if let Some(menu) = app.save_load_mut() {
                        menu.status = format!("Save failed: {error}");
                    }
                }
            }
            Transition::None
        }
        crate::SaveLoadAction::Load { slot } => {
            match app.load_game_slot(slot) {
                Ok(path) => {
                    app.close_save_load();
                    app.hud_mut().push_log(format!("Loaded {}", path.display()));
                }
                Err(error) => {
                    if let Some(menu) = app.save_load_mut() {
                        menu.status = format!("Load failed: {error}");
                    }
                }
            }
            Transition::None
        }
    }
}

/// Ввод в открытый `BuildMenu` (модальный экран P2).
///
/// `Done`/Enter — `Build()`: заказ уходит `Command::Order` с `PayloadP4`
/// (пакет 08) — локально применяется сразу, в сети уходит в очередь HUD.
/// `Cancel`/Esc — закрыть без заказа.
fn session_build_menu_input(app: &mut App, event: InputEvent, online: bool) -> Transition {
    let action = app.build_menu_input(event);

    match action {
        Some(crate::buildmenu::BuildMenuAction::Commit) => {
            if let Some(line) = app.commit_build_menu(online) {
                app.hud_mut().push_log(line);
            }
            // `UnitsManager_Popup_OnClick_Build` → `GameManager_AutoSelectNext`
            // (`units_manager.cpp:825-826`): после заказа постройки C++ снимает
            // выделение со строителя, если он больше не интерактивен.
            app.auto_select_after_order();
        }
        Some(crate::buildmenu::BuildMenuAction::PathBuild) => {
            if let Some(line) = app.commit_build_menu_path(online) {
                app.hud_mut().push_log(line);
            }
            app.auto_select_after_order();
        }
        Some(crate::buildmenu::BuildMenuAction::Close) => {
            app.close_build_menu();
            app.hud_mut().push_log("Build cancelled");
        }
        _ => {}
    }

    Transition::None
}

/// Ввод в режиме выбора места стройки (`ORDER_STATE_SELECT_SITE`).
///
/// Повторяет C++:
/// * **движение мыши** — `GameManager_GetBuilderUnitCursor`: призрак-лента
///   переставляется под курсор (якорь 2×2 привязан к строителю), курсор окна
///   становится `CURSOR_MAP2`/`CURSOR_UNIT_NO_GO`;
/// * **LMB по карте** — `GameManager_PathBuild`: приказ `ORDER_BUILD` с
///   клеткой площадки; по невалидной клетке C++ (`CURSOR_UNIT_NO_GO` →
///   `ManagePlayerAction`) отменяет выбор;
/// * **RMB по карте / Esc** — отмена (`GameManager_SelectBuildSite`): приказ не
///   уходит, очередь строителя не меняется.
///
/// `None` — событие не наше (клик по панели/клавиша не Esc): вызывающий
/// обработает его обычным путём, как C++ с включённым главным меню.
fn session_build_site_input(
    app: &mut App,
    event: InputEvent,
    online: bool,
    chrome: bool,
) -> Option<Transition> {
    // Клетка карты под точкой кадра — та же проекция, что у обычных кликов
    // (`session_map_left_click`: `screen_to_map` при хроме).
    let grid_at = |app: &App, x: i32, y: i32| -> Option<max_core::geo::Point> {
        let (gx, gy) = if chrome {
            let (map_x, map_y) = panels::screen_to_map(x, y)?;
            app.map_viewport().screen_to_grid(map_x, map_y)
        } else {
            app.viewport().screen_to_grid(x, y)
        };

        Some(max_core::geo::Point::new(gx as i16, gy as i16))
    };

    match event {
        InputEvent::MouseMove(x, y) => {
            app.hud_mut().mouse_position = Some((x, y));

            if let Some(grid) = grid_at(app, x, y) {
                if let Some(mut site) = app.build_site.take() {
                    site.update_cursor(app.state(), grid);
                    app.build_site = Some(site);
                }
            }

            // Дальше обычная обработка движения (подпись панели, координаты).
            None
        }
        InputEvent::MouseClick(x, y) => {
            let Some(grid) = grid_at(app, x, y) else {
                return None;
            };
            let valid = match app.build_site.take() {
                Some(mut site) => {
                    site.update_cursor(app.state(), grid);
                    let valid = site.valid(app.state());
                    app.build_site = Some(site);
                    valid
                }
                None => false,
            };

            let line = if valid {
                // `GameManager_PathBuild` (`game_manager.cpp:4569`): голос
                // `V_M049`, затем `UnitsManager_StartBuild`.
                app.play_sfx(Sfx::Done);
                app.play_sfx(Sfx::BuildSitePlaced);
                app.confirm_build_site(online)
            } else {
                // C++: `CURSOR_UNIT_NO_GO` → `GameManager_ManagePlayerAction` —
                // выбор отменяется целиком.
                app.play_sfx(Sfx::Cancel);
                app.cancel_build_site()
                    .map(|line| format!("{line} (invalid cell {},{} )", grid.x, grid.y))
            };

            if let Some(line) = line {
                app.hud_mut().push_log(line);
            }
            if !online {
                apply_pending_orders(app);
            }

            Some(Transition::None)
        }
        InputEvent::RightClick(x, y) => {
            // RMB по карте отменяет выбор; по панели — обычная обработка.
            grid_at(app, x, y)?;

            if let Some(line) = app.cancel_build_site() {
                app.hud_mut().push_log(line);
            }

            Some(Transition::None)
        }
        InputEvent::Key(Key::Escape) => {
            if let Some(line) = app.cancel_build_site() {
                app.hud_mut().push_log(line);
            }

            Some(Transition::None)
        }
        _ => None,
    }
}

/// Ввод в открытом экране апгрейдов (модальный экран P6).
///
/// `Done`/Enter — `CommitUpgradeChanges`: изменённые типы пишутся в
/// `TeamUnits` (пакеты 10 в сети), золото списывается. `Cancel`/Esc — закрыть
/// без изменений (C++ `UpgradeMenu::Run` возвращает `false`). `Help` —
/// `UPGRADES_SETUP` поверх экрана (`HelpMenu_Menu`).
fn session_upgrade_menu_input(app: &mut App, event: InputEvent, online: bool) -> Transition {
    let action = app.upgrade_menu_input(event);

    match action {
        Some(crate::upgrademenu::UpgradeAction::Commit) => {
            if let Some(line) = app.commit_upgrade_menu(online) {
                app.hud_mut().push_log(line);
            }
        }
        Some(crate::upgrademenu::UpgradeAction::Cancel) => {
            app.close_upgrade_menu();
            app.hud_mut().push_log("Upgrades cancelled");
        }
        Some(crate::upgrademenu::UpgradeAction::Help) => {
            app.open_help(HelpSection::Upgrades);
            app.hud_mut().push_log("Upgrades help");
        }
        _ => {}
    }

    Transition::None
}

/// Ввод в окне помощи поверх игры (`HelpMenu`).
///
/// Окно модально; `Done`/`Enter`/`Esc` закрывают его (`event_click_cancel`), а
/// экран под ним (игра/`ResearchMenu`/`UpgradeMenu`) продолжает работать с тем
/// же состоянием.
fn session_help_input(app: &mut App, event: InputEvent) -> Transition {
    // Кнопки окна: `Done` — `NDONE0`, прокрутка/`Keys` — `KCARG0`
    // (`helpmenu.cpp:91/131/135`).
    match event {
        InputEvent::MouseClick(x, y) => {
            if app.help_screen().is_some_and(|help| help.done_rect().contains(x, y)) {
                app.play_sfx(Sfx::Done);
            } else {
                app.play_sfx(Sfx::Click);
            }
        }
        InputEvent::Key(Key::Enter) => app.play_sfx(Sfx::Done),
        InputEvent::Key(Key::PageUp)
        | InputEvent::Key(Key::PageDown)
        | InputEvent::Key(Key::Up)
        | InputEvent::Key(Key::Down) => app.play_sfx(Sfx::Click),
        InputEvent::Key(Key::Char('k' | 'K')) => app.play_sfx(Sfx::Done),
        _ => {}
    }

    if app.help_input(event) == Some(HelpAction::Close) {
        app.close_help();
        app.hud_mut().push_log("Help closed");
    }

    Transition::None
}

/// Ввод в режиме помощи (`HelpMenu_Menu`, `helpmenu.cpp:315`).
///
/// Клик по элементу с записью `help.json` открывает окно (`HelpMenu`); клик по
/// пустому месту окно не открывает и режим не выключает (C++: пустой текст —
/// окно не создаётся). `Esc`, правая кнопка или повторный `?` выключают режим
/// (`MOUSE_RELEASE_RIGHT`/`GNW_KB_KEY_CTRL_ESCAPE`/`GNW_KB_KEY_SHIFT_DIVIDE`).
fn session_help_mode_input(app: &mut App, event: InputEvent) -> Transition {
    match event {
        InputEvent::MouseClick(x, y) => {
            if app.help_mode_click(x, y) {
                // `HelpMenu_Menu` (`helpmenu.cpp:331`): выбор элемента — `KCARG0`.
                app.play_sfx(Sfx::Click);
                let entry = app
                    .help_screen()
                    .and_then(|help| help.entry_name())
                    .unwrap_or("?");
                app.hud_mut().push_log(format!("Help: {entry}"));
            } else {
                app.hud_mut()
                    .push_log("Help: здесь нет подсказки — выберите элемент");
            }
        }
        InputEvent::RightClick(..)
        | InputEvent::Key(Key::Escape)
        | InputEvent::Key(Key::Char('?')) => {
            app.exit_help_mode();
            app.hud_mut().push_log("Help closed");
        }
        _ => {}
    }

    Transition::None
}

/// Ввод в диалоге `Preferences` поверх игры (C++ `OptionsMenu::Run`, `PREFSPIC`).
///
/// `Done`/`Enter` применяют и сохраняют настройки, `Esc` возвращает снимок
/// открытия, `?` открывает окно помощи `PREFS_MENU_SETUP`. Пока диалог открыт,
/// `App::local_update` не тикает (C++ цикл стоит в `Run`).
fn session_preferences_input(app: &mut App, event: InputEvent) -> Transition {
    match app.preferences_input(event) {
        Some(preferences::PreferencesAction::Done) => {
            // Значения уже в `App::preferences`; для следующего кадра кадровые
            // часы читают новый `effects`.
        }
        Some(preferences::PreferencesAction::Cancel) => {
            app.hud_mut().push_log("Preferences cancelled");
        }
        Some(preferences::PreferencesAction::Help) => {
            let section = app
                .preferences_dialog()
                .map(|dialog| dialog.variant().help_section())
                .unwrap_or(HelpSection::Preferences);
            app.open_help(section);
            app.hud_mut().push_log("Preferences help");
        }
        _ => {}
    }

    Transition::None
}

/// Ввод в открытом экране исследований (модальный экран P7).
///
/// `Done`/`Enter` — `ResearchMenu_OnClick_Done`: распределение центров пишется
/// в `GameState` ([`max_core::game::GameState::set_research_allocation`]) и
/// подхватывается `finish_turn` (`ResearchMenu_NewTurn`). `Cancel`/`Esc` —
/// закрыть без изменений. `Help` — `RESEARCH_SETUP`.
fn session_research_menu_input(app: &mut App, event: InputEvent) -> Transition {
    let action = app.research_menu_input(event);

    match action {
        Some(ResearchAction::Done) => {
            if let Some(line) = app.commit_research_menu() {
                app.hud_mut().push_log(line);
            }
        }
        Some(ResearchAction::Cancel) => {
            app.close_research_menu();
            app.hud_mut().push_log("Research cancelled");
        }
        Some(ResearchAction::Help) => {
            app.open_help(HelpSection::Research);
            app.hud_mut().push_log("Research help");
        }
        _ => {}
    }

    Transition::None
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
        Key::Tab
        | Key::Up
        | Key::Down
        | Key::Left
        | Key::Right
        | Key::PageUp
        | Key::PageDown
        | Key::Alt(_) | Key::Other => {}
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

/// Левая кнопка без хрома: кнопки процедурного HUD → миникарта → карта.
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

    let viewport = *app.viewport();

    session_map_left_click(app, &viewport, x, y, online)
}

/// Левая кнопка с хромом 1:1: панели → миникарта → карта.
///
/// Экранные координаты окна переводятся в координаты области карты
/// ([`panels::screen_to_map`]) до `screen_to_grid` — иначе клик уезжает на
/// (180,18) (риск 4 в `docs/visual-1to1-status.md`).
fn session_chrome_left_click(app: &mut App, x: i32, y: i32, online: bool) -> Transition {
    if let Some(button) = panels::panel_button_at(x, y) {
        return session_panel_button(app, button, x, online);
    }

    // Миникарта: 112×112 клеток мира в окне (15,356). В 2x-режиме клик
    // переводится через `GameManager_GetMinimapPosition` (область 56×56 вокруг
    // центра камеры), иначе клик уезжает.
    let minimap = max_render::layout::MINIMAP;

    if minimap.contains(x, y) {
        let state = app.state();
        let map_width = i32::from(state.map.size.width);
        let map_height = i32::from(state.map.size.height);
        let two_x = app.hud().panels.minimap_2x;

        if let Some((grid_x, grid_y)) =
            panels::minimap_grid_at(x, y, map_width, map_height, app.viewport(), two_x)
        {
            app.viewport_mut().center_on_grid(grid_x, grid_y);
        }

        return Transition::None;
    }

    // Клик вне карты (рамка/прокрутка) — не приказ.
    let Some((map_x, map_y)) = panels::screen_to_map(x, y) else {
        return Transition::None;
    };
    let viewport = app.map_viewport();

    session_map_left_click(app, &viewport, map_x, map_y, online)
}

/// Кнопка панели 1:1: End Turn, переключатели, зум, миникарта, меню.
fn session_panel_button(app: &mut App, button: PanelButton, x: i32, online: bool) -> Transition {
    // Кнопки панелей — `PlaySfx(MBUTT0)` (`game_manager.cpp:5446/5692/5713`).
    app.play_sfx(Sfx::Button);

    match button {
        PanelButton::EndTurn => {
            session_end_turn(app, online);
            return Transition::None;
        }
        PanelButton::Center => {
            if let Some(grid) = app.selected_unit().map(|unit| unit.grid) {
                app.viewport_mut().center_on_grid(grid.x, grid.y);
                app.hud_mut().push_log("Center on selected unit");
            }
            return Transition::None;
        }
        PanelButton::ZoomPlus => {
            let zoom = (app.viewport().zoom() + 8).clamp(8, 64);
            app.viewport_mut().set_zoom(zoom);
            return Transition::None;
        }
        PanelButton::ZoomMinus => {
            let zoom = app.viewport().zoom().saturating_sub(8).max(8);
            app.viewport_mut().set_zoom(zoom);
            return Transition::None;
        }
        PanelButton::ZoomSlider => {
            let zoom = panels::zoom_from_slider_x(x);
            app.viewport_mut().set_zoom(zoom);
            return Transition::None;
        }
        PanelButton::Minimap2x => {
            let current = app.hud().panels.minimap_2x;
            app.hud_mut().panels.minimap_2x = !current;
            return Transition::None;
        }
        PanelButton::MinimapTnt => {
            let current = app.hud().panels.minimap_tnt;
            app.hud_mut().panels.minimap_tnt = !current;
            return Transition::None;
        }
        PanelButton::Pause if app.local_active() => {
            // Пауза M-SP: `PauseMenu` — тики и таймер хода останавливаются.
            app.local_toggle_pause();
            return Transition::None;
        }
        PanelButton::Help => {
            // `GameManager_MenuClickHelpButton` → `HelpMenu_Menu("GAME_SCREEN_SETUP")`
            // (`WINDOW_MAIN_MAP`); `?`/Shift+/ в игре делает то же самое.
            app.open_help(HelpSection::GameScreen);
            app.hud_mut().push_log("Help: game screen");
            return Transition::None;
        }
        PanelButton::Preferences => {
            // `GameManager_MenuClickPreferencesButton` (`game_manager.cpp:3918`)
            // → `menu_preferences_window` (`OptionsMenu`, `PREFSPIC`).
            if app.open_preferences() {
                app.hud_mut().push_log("Preferences");
            } else {
                app.hud_mut()
                    .push_log("Preferences: close the open dialog first");
            }
            return Transition::None;
        }
        PanelButton::Files => {
            // `GameManager_MenuClickFileButton` (`Alt+F`) — окно V70.
            app.open_save_menu(online);
            return Transition::None;
        }
        PanelButton::Play
        | PanelButton::Pause
        | PanelButton::Lock
        | PanelButton::Prev
        | PanelButton::Done
        | PanelButton::Next
        | PanelButton::Reports
        | PanelButton::Chat => {
            // Экраны этих кнопок ещё не портированы (пункты C/E плана).
            app.hud_mut()
                .push_log(format!("{button:?}: screen is not ported yet"));
            return Transition::None;
        }
        _ if button.is_toggle() => {
            let enabled = app.hud_mut().panels.toggle(button);
            let caption = button.caption().unwrap_or("toggle");
            app.hud_mut()
                .push_log(format!("{caption}: {}", if enabled { "on" } else { "off" }));
            return Transition::None;
        }
        _ => {}
    }

    Transition::None
}

/// `MAX_DEBUG_INPUT=1` — трассировать ввод сессии в stderr (диагностика).
///
/// Без переменной окружения ни одна ветка ничего не печатает: вывод не
/// попадает ни в обычный запуск, ни в QA/golden.
fn debug_input() -> bool {
    std::env::var_os("MAX_DEBUG_INPUT").is_some()
}

/// Выделение/атака по точке **камеры карты** (448×448 при хроме).
fn session_map_left_click(
    app: &mut App,
    viewport: &Viewport,
    x: i32,
    y: i32,
    online: bool,
) -> Transition {
    // `GameManager_ProcessPlayerInput` (`game_manager.cpp:6711`): отпускание LMB
    // по карте — `PlaySfx(KCARG0)`.
    app.play_sfx(Sfx::Click);

    let team = app.player_team();
    let grid = viewport.screen_to_grid(x, y);
    if debug_input() {
        let at = app
            .state()
            .units_at(max_core::geo::Point::new(grid.0, grid.1))
            .len();
        let origin = viewport.origin();
        eprintln!(
            "[click] map=({x},{y}) grid=({},{}) units_at={at} viewport_origin=({},{})",
            grid.0, grid.1, origin.0, origin.1
        );
    }
    let Some(id) = hud::unit_at_screen(app.state(), viewport, team, x, y) else {
        // `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5832-5843`):
        // выделенный строитель в `ORDER_STATE_UNIT_READY` + клетка рядом с
        // достроенным зданием (кольцо `BLDMRK*`) — курсор `CURSOR_UNIT_GO`, и
        // LMB отдаёт `ORDER_ACTIVATE`/`IN_TRANSITION` (`:6879-6890`), а не
        // снимает выделение. Подсказка `_(276c)` зовёт именно кликнуть «клетку
        // с символом».
        let exit_cell = max_core::geo::Point::new(grid.0, grid.1);
        if let Some(selected) = app.hud().selected {
            if let Some(command) =
                hud::build_builder_exit_click_order(app.state(), selected, exit_cell)
            {
                app.hud_mut().preview = None;
                app.hud_mut().pending.push(command);
                app.hud_mut().push_log(format!(
                    "Leave construction site: click ({}, {})",
                    exit_cell.x, exit_cell.y
                ));

                if !online {
                    apply_pending_orders(app);
                }

                return Transition::None;
            }
        }

        // `GameManager_ProcessPlayerInput`: клик по карте закрывает окно
        // сообщения (`MessageManager_ClearMessageBox`).
        app.hud_mut().clear_hint();

        // `targeting_mode` и пустая клетка: `GameManager_SetUnitOrder`
        // (`game_manager.cpp:6360`) — если цель не найдена, а выстрелы есть,
        // `fire_on = (x,y)` и `ORDER_FIRE`/`INIT` (стрельба по площади).
        if app.hud().targeting_mode {
            if let Some(selected) = app.hud().selected {
                let cell = max_core::geo::Point::new(grid.0, grid.1);
                let in_range = app.state().unit(selected).is_some_and(|unit| {
                    max_core::access::is_within_attack_range(
                        app.state(),
                        unit,
                        cell,
                        i32::from(unit.base_values.get_attribute(
                            max_core::unit_values::ATTRIB_RANGE,
                        )),
                    )
                });

                if in_range {
                    match hud::build_ground_fire_order(app.state(), selected, cell) {
                        Some(command) => {
                            app.hud_mut().pending.push(command);
                            app.hud_mut()
                                .push_log(format!("Attack ground: ({}, {})", grid.0, grid.1));
                            if !online {
                                apply_pending_orders(app);
                            }
                            return Transition::None;
                        }
                        None => {
                            app.hud_mut().push_log("Attack: нет выстрелов");
                            return Transition::None;
                        }
                    }
                }
            }
        }

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
        let selected = app.hud().selected;

        // Курсорный режим выделенного юнита (C++ `UnitInfo::cursor` 4/6, меню
        // команд): клик по своему юниту отдаёт `ORDER_REPAIR`/`ORDER_RELOAD`
        // (`GameManager_RepairUnit`/`GameManager_ReloadUnit`).
        let cursor = app.hud().cursor_mode;
        if cursor != 0 {
            if let Some(selected) = selected.filter(|selected| *selected != id) {
                // X-FER: `GameManager_ProcessMouse` → CURSOR_TRANSFER →
                // `GameManager_TransferCargo` → `TransferMenu_Menu`.
                if cursor == crate::unitmenu::CURSOR_TRANSFER {
                    match crate::transfermenu::resolve_target(app.state(), selected, id) {
                        Ok(Some(target)) => {
                            if app.open_transfer_menu(selected, target) {
                                let label = app
                                    .state()
                                    .unit(target)
                                    .map(unit_label)
                                    .unwrap_or_else(|| format!("unit {target:?}"));
                                app.hud_mut()
                                    .push_log(format!("x-fer: цель {label} — выберите количество"));
                            } else {
                                app.hud_mut().push_log("x-fer: окно не открылось");
                            }
                        }
                        Ok(None) => {
                            // C++ молча ничего не делает: у цели нет
                            // совместимого склада/типа груза.
                        }
                        Err(message) => app.hud_mut().push_log(message),
                    }
                    return Transition::None;
                }

                match crate::unitmenu::cursor_mode_click(app.state(), selected, id, cursor) {
                    Ok(Some(command)) => {
                        app.hud_mut().pending.push(command);
                        let label = match cursor {
                            crate::unitmenu::CURSOR_REPAIR => "Repair",
                            _ => "Reload",
                        };
                        // `GameManager_RepairUnit`/`ReloadUnit`
                        // (`game_manager.cpp:4609/4626`): `V_M210`/`V_M085`.
                        app.play_sfx(if cursor == crate::unitmenu::CURSOR_REPAIR {
                            Sfx::Repair
                        } else {
                            Sfx::Reload
                        });
                        app.hud_mut().push_log(format!("{label}: приказ отдан"));
                        if !online {
                            apply_pending_orders(app);
                        }
                        return Transition::None;
                    }
                    Ok(None) => {}
                    Err(message) => {
                        // Недоступное действие — `NCANC0` (`game_manager.cpp:4612`).
                        app.play_sfx(Sfx::Cancel);
                        app.hud_mut().push_log(message);
                        return Transition::None;
                    }
                }

                // `load` (`cursor = 7`) — `GameManager_ProcessMouse`,
                // ветка `CURSOR_LOAD` (`game_manager.cpp:6807`).
                if cursor == crate::unitmenu::CURSOR_LOAD {
                    // C++ отдаёт приказ только когда курсор — `CURSOR_LOAD`
                    // (`load_mode_applies`); иначе цель не принимается.
                    let applies = match (app.state().unit(selected), app.state().unit(id)) {
                        (Some(transport), Some(passenger)) => {
                            crate::unitmenu::load_mode_applies(transport, passenger)
                        }
                        _ => false,
                    };

                    if !applies {
                        app.play_sfx(Sfx::Cancel);
                        app.hud_mut()
                            .push_log("load: цель не принимается транспортом");
                        return Transition::None;
                    }

                    match crate::unitmenu::load_mode_click(app.state(), selected, id) {
                        Ok(Some(command)) => {
                            app.hud_mut().pending.push(command);
                            app.hud_mut()
                                .push_log(format!("load: погрузка ({label}) — приказ отдан"));
                            if !online {
                                apply_pending_orders(app);
                            }
                            return Transition::None;
                        }
                        Ok(None) => {}
                        Err(message) => {
                            app.hud_mut().push_log(message);
                            return Transition::None;
                        }
                    }
                }
            }
        }

        // `enter_mode` (C++ `UnitsManager_Popup_OnClick_Enter`): клик по своему
        // юниту — `ORDER_MOVE_TO_UNIT` в его клетку (посадка в транспорт).
        if app.hud().enter_mode {
            if let Some(selected) = selected.filter(|selected| *selected != id) {
                if let Some(command) = crate::unitmenu::enter_mode_click(app.state(), selected, id)
                {
                    app.hud_mut().pending.push(command);
                    app.hud_mut().enter_mode = false;
                    app.hud_mut().push_log(format!("Enter: move to unit ({label})"));
                    if !online {
                        apply_pending_orders(app);
                    }
                    return Transition::None;
                }
            }
            app.hud_mut().enter_mode = false;
        }

        // Повторный клик по уже выделенному юниту открывает меню команд —
        // C++ `GameManager_ClickUnit` (`unit1 == unit2` → `InitPopupButtons`).
        if selected == Some(id) {
            app.hud_mut().preview = None;
            session_show_unit_status(app, id);

            let line = if app.open_unit_menu() {
                let items = app.unit_menu().map_or(0, |menu| menu.items.len());
                format!("Unit menu: {label} ({items})")
            } else {
                format!("Unit menu: no commands for {label}")
            };
            app.hud_mut().push_log(line);

            return Transition::None;
        }

        app.hud_mut().select(Some(id));
        app.hud_mut().preview = None;
        app.hud_mut().push_log(format!("Selected {label}"));
        // `GameManager_UnitSelect` → `GameManager_DrawUnitStatusMessage`:
        // подсказка сверху карты о текущей стройке выделенного строителя.
        session_show_unit_status(app, id);
        return Transition::None;
    }

    // Враг: диверсант (курсорные режимы 8/9) или атака (пакет 08, P3).
    let Some(selected) = app.hud().selected else {
        app.hud_mut()
            .push_log("Enemy unit — select your unit first");
        return Transition::None;
    };

    let cursor = app.hud().cursor_mode;
    if matches!(
        cursor,
        crate::unitmenu::CURSOR_STEAL | crate::unitmenu::CURSOR_DISABLE
    ) {
        // `GameManager_StealUnit`/`GameManager_DisableUnit`
        // (`game_manager.cpp:4680/4696`): `Randomizer_Generate(101)` уходит в
        // пакет 08/P3 (`stealth_dice_roll`), поэтому ролл генерируется здесь.
        let roll = app.state_mut().rng.generate(101i32) as u8;

        match crate::unitmenu::stealth_mode_click(app.state(), selected, id, cursor, roll) {
            Ok(Some(command)) => {
                app.hud_mut().pending.push(command);
                let label = if cursor == crate::unitmenu::CURSOR_STEAL {
                    "steal"
                } else {
                    "disable"
                };
                // `GameManager_StealUnit`/`DisableUnit` — выбор цели как KCARG0.
                app.play_sfx(Sfx::Click);
                app.hud_mut().push_log(format!("{label}: действие назначено"));
                if !online {
                    apply_pending_orders(app);
                }
                return Transition::None;
            }
            Ok(None) => {
                app.play_sfx(Sfx::Cancel);
                app.hud_mut()
                    .push_log("Infiltrator: цель не подходит");
                return Transition::None;
            }
            Err(message) => {
                app.play_sfx(Sfx::Cancel);
                app.hud_mut().push_log(message);
                return Transition::None;
            }
        }
    }

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

/// `GameManager_DrawUnitStatusMessage` (`game_manager.cpp:8038`): подсказка
/// сверху карты для выделенного строителя.
///
/// Для строящего (`ORDER_BUILD`) — «Currently building: X. / Turns to
/// completion: N.» (`ee24`), для достроившего (`ORDER_STATE_UNIT_READY`) —
/// «Finished building X. …» (`276c`). Для остальных юнитов окно сообщения
/// закрывается, как `MessageManager_ClearMessageBox` в `GameManager_UnitSelect`.
fn session_show_unit_status(app: &mut App, id: max_core::unit::UnitId) {
    let message = app
        .state()
        .unit(id)
        .and_then(|unit| crate::panels::builder_status_message(app.state(), unit));

    match message {
        Some(text) => app.hud_mut().set_hint(text),
        None => app.hud_mut().clear_hint(),
    }
}

/// Правая кнопка без хрома.
fn session_right_click(app: &mut App, x: i32, y: i32, online: bool) -> Transition {
    let viewport = *app.viewport();

    session_map_right_click(app, &viewport, x, y, online)
}

/// Правая кнопка с хромом: сначала панели (они не дают приказ), затем карта.
fn session_chrome_right_click(app: &mut App, x: i32, y: i32, online: bool) -> Transition {
    if panels::panel_button_at(x, y).is_some() || max_render::layout::MINIMAP.contains(x, y) {
        return Transition::None;
    }
    let Some((map_x, map_y)) = panels::screen_to_map(x, y) else {
        return Transition::None;
    };
    let viewport = app.map_viewport();

    session_map_right_click(app, &viewport, map_x, map_y, online)
}

/// Правая кнопка по карте: отмена по юниту, иначе приказ движения в клетку.
fn session_map_right_click(
    app: &mut App,
    viewport: &Viewport,
    x: i32,
    y: i32,
    online: bool,
) -> Transition {
    let team = app.player_team();
    let (grid_x, grid_y) = viewport.screen_to_grid(x, y);
    let debug = debug_input();

    if debug {
        let origin = viewport.origin();
        eprintln!(
            "[rclick] map=({x},{y}) grid=({grid_x},{grid_y}) viewport_origin=({},{}) selected={:?}",
            origin.0,
            origin.1,
            app.hud().selected
        );
    }

    // «Отмена — Esc/правый клик по юниту».
    if let Some(id) = hud::unit_at_screen(app.state(), viewport, team, x, y) {
        let label = app
            .state()
            .unit(id)
            .map(unit_label)
            .unwrap_or_else(|| "unit".to_string());

        if debug {
            eprintln!("[rclick] result=cancel unit={} ({label})", id.0);
        }

        app.hud_mut().clear_interaction();
        app.hud_mut().push_log(format!("Cancelled ({label})"));
        return Transition::None;
    }

    let Some(selected) = app.hud().selected else {
        if debug {
            eprintln!("[rclick] result=no-selection");
        }
        app.hud_mut().push_log("No unit selected — LMB selects");
        return Transition::None;
    };

    let destination = GridPoint::new(grid_x, grid_y);

    if grid_x < 0
        || grid_y < 0
        || grid_x >= max_core::geo::MAP_WIDTH
        || grid_y >= max_core::geo::MAP_HEIGHT
    {
        if debug {
            eprintln!("[rclick] result=outside-map grid=({grid_x},{grid_y})");
        }
        app.hud_mut().push_log("Outside the map");
        return Transition::None;
    }

    let can_move = app.state().unit(selected).is_some_and(hud::unit_can_move);

    if !can_move {
        if debug {
            eprintln!("[rclick] result=cannot-move selected={}", selected.0);
        }
        app.hud_mut().push_log("Unit cannot move (air/stationary)");
        return Transition::None;
    }

    // Достроивший строитель стоит на площадке (`ORDER_BUILD`/`UNIT_READY`,
    // здание `ORDER_IDLE`/`BUILDING_READY`): клик отдаёт `ORDER_ACTIVATE`/
    // `IN_TRANSITION` — ровно `game_manager.cpp:6879-6890`. Путь не строится
    // (C++ ставит только `move_to`), поэтому `build_move_order` здесь не
    // вызывается.
    if let Some(command) = hud::build_builder_exit_order(app.state(), selected, destination) {
        app.hud_mut().preview = None;
        app.hud_mut().pending.push(command);
        app.hud_mut().push_log(format!(
            "Leave construction site: click ({}, {})",
            destination.x, destination.y
        ));

        if debug {
            eprintln!(
                "[rclick] result=leave-site selected={} to=({},{})",
                selected.0, destination.x, destination.y
            );
        }

        if !online {
            apply_pending_orders(app);
        }

        return Transition::None;
    }

    // Строитель на недостроенной площадке: C++ не даёт приказ движения
    // (`GameManager_GetBuilderUnitCursor`, `game_manager.cpp:5927-5931` —
    // `CURSOR_UNIT_NO_GO` для всех состояний, кроме `UNIT_READY`/`SELECT_SITE`).
    // Увести его можно только явной отменой `stop` в попапе
    // (`UnitsManager_Popup_OnClick_BuildStop` → `ORDER_BUILD`/`BUILD_CANCEL`).
    if let Some(site) = app
        .state()
        .unit(selected)
        .and_then(max_core::unit_order::active_build_site)
    {
        app.hud_mut().preview = None;
        app.play_sfx(Sfx::Cancel);
        app.hud_mut().push_log(format!(
            "Cannot move while building at ({}, {}) — use stop",
            site.x, site.y
        ));

        if debug {
            eprintln!(
                "[rclick] result=no-go-building selected={} site=({},{})",
                selected.0, site.x, site.y
            );
        }

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

            if debug {
                eprintln!(
                    "[rclick] result=move selected={} steps={steps} to=({},{})",
                    selected.0, destination.x, destination.y
                );
            }

            if !online {
                apply_pending_orders(app);
            }
        }
        None => {
            if debug {
                eprintln!(
                    "[rclick] result=no-path selected={} to=({},{})",
                    selected.0, destination.x, destination.y
                );
            }
            app.hud_mut().preview = None;
            app.hud_mut()
                .push_log(format!("No path to ({}, {})", destination.x, destination.y));
        }
    }

    Transition::None
}

/// End Turn: в локальной партии — ход ИИ, в сети — флаг для игрового цикла
/// (`06`+`52` там же), в демо — локальный проход [`App::end_turn`].
fn session_end_turn(app: &mut App, online: bool) {
    if app.local_active() {
        app.local_end_turn();
    } else if online {
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
/// Если загружен хром окна 1:1 (`FRAMEPIC`) и кадр 640×480, HUD оригинала уже
/// нарисован в самой картинке — процедурный HUD не рисуется, поверх области
/// карты выводится только строка статуса. Без хрома — прежний вид.
///
/// Без `App` (headless-рендер до сети) остаётся прежний фон со строкой статуса.
fn render_session(buffer: &mut RgbaBuffer, view: ScreenView<'_>) {
    if let Some(app) = view.app {
        let frame = app.render_frame();
        blit(buffer, &frame);

        let chrome_active = app.chrome_active();

        if chrome_active {
            // Область карты уже содержит тайлы, юнитов и панели; пишем только
            // строку сообщений в верхнем левом углу карты — шрифтом GNW, как
            // `MessageManager_DrawMessage`.
            let area = max_render::layout::MAIN_MAP;
            let status: String = view.status.chars().take(70).collect();

            // Открытый `BuildMenu` — модальный экран поверх кадра: строка
            // статуса партии его не перекрывает (как `GameManager_DisableMainMenu`).
            // Окно сохранения/загрузки (P3) — такое же модальное.
            // Диалог распределения добычи (`AllocMenu`) занимает весь кадр
            // 640×480 (`ALLOCFRM`), поэтому строка статуса не рисуется поверх
            // него — как модальное окно C++.
            if app.build_menu().is_none()
                && app.save_load().is_none()
                && app.alloc_menu().is_none()
            {
                if let Some(assets) = app.assets() {
                    max_render::text::draw_text_gnw_shadowed(
                        buffer,
                        area.ulx + 4,
                        area.uly + 4,
                        max_render::text::GnwFontId::Font5.font(),
                        &status,
                        assets.palette.rgba(0xA0),
                        assets.palette.rgba(0xE6),
                        assets.palette.rgba(0x3F),
                    );
                }
            }

            // M-HS: окно перехода хода поверх кадра — модальное, как `DialogMenu`.
            draw_local_notice(buffer, app, view.mouse);
            return;
        }

        let hud_view = hud::HudView {
            state: app.state(),
            hud: app.hud(),
            viewport: app.viewport(),
            team: app.player_team(),
            status: view.status,
        };

        if app.build_menu().is_none() && app.save_load().is_none() {
            hud::render_hud(buffer, &hud_view);
        }
        draw_local_notice(buffer, app, view.mouse);
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
        draw_text_line(buffer, 4, y + 4, view.status, Rgba::rgb(232, 236, 244));
    }
}

/// Окно перехода хода M-HS поверх кадра сессии (`DialogMenu`).
///
/// Рисуется только когда открыт `LocalGame::notice` — то есть в hot-seat на
/// смене человеческой команды (`GameManager_UpdateGuiControl`).
fn draw_local_notice(buffer: &mut RgbaBuffer, app: &App, mouse: Option<Point>) {
    let Some(notice) = app.local_notice() else {
        return;
    };
    let Some(assets) = app.assets() else {
        return;
    };
    let Some(menu) = assets.menu.as_ref() else {
        return;
    };
    let Some(palette) = crate::panels::UiPalette::from_assets(assets) else {
        return;
    };

    crate::hotseat::draw_turn_notice(
        buffer,
        menu,
        &palette,
        notice,
        mouse.map(|point| (point.x, point.y)),
    );
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
    let theme = UiTheme::default();
    let native_frame = width == menu::MENU_WIDTH && height == menu::MENU_HEIGHT;
    let mut widgets = Vec::with_capacity(12);

    if native_frame {
        // Семь кнопок оригинала: индексы 0..=6, из них действия Rust-клиента —
        // 0 (New Game), 2 (Multiplayer Game), 6 (Exit).
        for button in &menu::MAIN_MENU_BUTTONS {
            widgets.push(Widget::Button(Button::new(
                button.label,
                Rect::new(
                    button.rect.ulx,
                    button.rect.uly,
                    button.rect.width,
                    button.rect.height,
                ),
            )));
        }

        widgets.push(Widget::Label(Label::colored(
            "Main Menu",
            Rect::new(236, 145, 172, 15),
            theme.accent,
        )));
        widgets.push(Widget::Label(Label::colored(
            status,
            Rect::new(10, 452, 370, 12),
            theme.accent,
        )));
        widgets.push(Widget::Label(Label::colored(
            "Up/Down + Enter or mouse; C/J/I/R/Q; Esc — quit",
            Rect::new(10, 436, 370, 12),
            theme.text_disabled,
        )));
    } else {
        let margin = horizontal_margin(width);
        let content = (width - margin * 2).max(60);
        let x = (width - content) / 2;
        let heights = [24, 12, 28, 28, 28, 12];
        let gap = 8;
        let total = heights.iter().sum::<i32>() + gap * (heights.len() as i32 - 1);
        let top = ((height - total) / 2).max(8);
        let rects = layout(Rect::new(x, top, content, total), &heights, gap);
        let empty = Rect::new(0, 0, 0, 0);

        // Индексы обязаны совпадать с оригинальным порядком (`MAIN_*`):
        // 0 — Create, 1 — Load, 2 — Join, 3..=5 — не портированы, 6 — Exit.
        let buttons = [
            ("New Game", rects[2]),
            ("Load Game", empty),
            ("Multiplayer Game", rects[3]),
            ("Setup", empty),
            ("Introduction", empty),
            ("Credits", empty),
            ("Exit", rects[4]),
        ];

        for (label, rect) in buttons {
            let enabled = !rect.is_empty();
            widgets.push(Widget::Button(
                Button::new(label, rect).with_enabled(enabled),
            ));
        }

        widgets.push(Widget::Label(Label::colored(
            "M.A.X.",
            rects[0],
            theme.accent,
        )));
        widgets.push(Widget::Label(Label::colored(
            "Rust client — M4.1 UI",
            rects[1],
            theme.text_disabled,
        )));
        widgets.push(Widget::Label(Label::colored(
            "Up/Down + Enter or mouse; C/J/I/R/Q; Esc — quit",
            rects[5],
            theme.text_disabled,
        )));
        widgets.push(Widget::Label(Label::colored(
            status,
            Rect::new(margin, (height - 18).max(0), content, 12),
            theme.accent,
        )));
    }

    let mut ui = UiState::new(widgets);
    ui.focus = Some(MAIN_ACTIONS[focus.min(MAIN_ACTIONS.len() - 1)]);
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
    let theme = UiTheme::default();
    let native_frame = width == menu::MENU_WIDTH && height == menu::MENU_HEIGHT;
    let items: Vec<String> = games
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
    let empty = Rect::new(0, 0, 0, 0);

    let (list_rect, row_height, back, refresh, join, hide, help) = if native_frame {
        (
            Rect::new(
                menu::JOIN_LIST.ulx,
                menu::JOIN_LIST.uly,
                menu::JOIN_LIST.width,
                menu::JOIN_LIST.height,
            ),
            menu::JOIN_ROW_HEIGHT,
            Rect::new(354, 438, 109, 40),
            Rect::new(243, 438, 109, 40),
            Rect::new(514, 438, 109, 40),
            Rect::new(
                menu::JOIN_HIDE.ulx,
                menu::JOIN_HIDE.uly,
                menu::JOIN_HIDE.width,
                menu::JOIN_HIDE.height,
            ),
            Rect::new(465, 438, 48, 40),
        )
    } else {
        let margin = horizontal_margin(width);
        let content = (width - margin * 2).max(60);
        let x = (width - content) / 2;
        let buttons_y = (height - 72).max(44);
        let list_height = (buttons_y - 6 - 30).max(20);

        (
            Rect::new(x, 30, content, list_height),
            14,
            Rect::new(x, buttons_y, 100, 26),
            Rect::new(x + 108, buttons_y, 100, 26),
            Rect::new(x + content - 100, buttons_y, 100, 26),
            empty,
            empty,
        )
    };

    let mut list = ListBox::with_items(list_rect, items);
    list.row_height = row_height;
    list.selected = (!games.is_empty()).then_some(selected.min(games.len().saturating_sub(1)));
    list.scroll = menu::join_scroll(selected);
    list.ensure_visible();

    let title = if native_frame {
        Rect::new(
            menu::JOIN_TITLE.ulx,
            menu::JOIN_TITLE.uly,
            menu::JOIN_TITLE.width,
            menu::JOIN_TITLE.height,
        )
    } else {
        Rect::new(back.x, 12, back.width.max(100) * 4, 12)
    };
    let hint = if native_frame {
        Rect::new(150, 418, 462, 12)
    } else {
        Rect::new(back.x, (height - 38).max(0), back.width.max(100) * 4, 12)
    };
    let status_rect = if native_frame {
        Rect::new(
            menu::JOIN_STATUS.ulx,
            menu::JOIN_STATUS.uly,
            menu::JOIN_STATUS.width,
            menu::JOIN_STATUS.height,
        )
    } else {
        Rect::new(back.x, (height - 18).max(0), back.width.max(100) * 4, 12)
    };

    let widgets = vec![
        Widget::Label(Label::colored("JOIN GAME", title, theme.accent)),
        Widget::ListBox(list),
        Widget::Button(Button::new("Cancel", back)),
        Widget::Button(Button::new("Options", refresh)),
        Widget::Button(Button::new("Ready", join).with_enabled(!games.is_empty())),
        Widget::Button(Button::new("Hide", hide).with_enabled(!hide.is_empty())),
        Widget::Button(Button::new("?", help).with_enabled(!help.is_empty())),
        Widget::Label(Label::colored(
            "Up/Down, Enter — join; R — refresh; Esc — back",
            hint,
            theme.text_disabled,
        )),
        Widget::Label(Label::colored(status, status_rect, theme.accent)),
    ];

    let mut ui = UiState::new(widgets);
    ui.focus = Some(JOIN_LIST);
    ui.set_hover(mouse);
    ui
}

/// Число планет в оригинале (`PlanetSelectMenu` — 24, `PlanetType`).
pub const PLANET_COUNT: usize = 24;
/// Колонок в сетке планет.
pub const PLANET_COLUMNS: i32 = 8;
/// Строк в сетке планет.
pub const PLANET_ROWS: i32 = 3;
/// Минимальная высота окна, при которой показывается сетка планет.
pub const PLANET_GRID_MIN_HEIGHT: u32 = 320;

/// Имена 24 планет — `_(e43b)`…`_(ba99)` из `planetselectmenu.cpp:120-122`
/// (`language.json`, en-US).
pub const PLANET_NAMES: [&str; PLANET_COUNT] = [
    "Snowcrab",
    "Frigia",
    "Ice Berg",
    "The Cooler",
    "Ultima Thule",
    "Long Floes",
    "Iron Cross",
    "Splatterscape",
    "Peak-a-boo",
    "Valentine's Planet",
    "Three Rings",
    "Great divide",
    "New Luzon",
    "Middle Sea",
    "High Impact",
    "Sanctuary",
    "Islandia",
    "Hammerhead",
    "Freckles",
    "Sandspit",
    "Great Circle",
    "Long Passage",
    "Flash Point",
    "Bottleneck",
];

/// Имя планеты (`planetselectmenu.cpp:138`).
pub fn planet_name(planet: Planet) -> &'static str {
    PLANET_NAMES
        .get(planet as usize)
        .copied()
        .unwrap_or("Unknown")
}

/// Планета по индексу сетки `0..24` (`Planet::try_from`).
pub fn planet_from_index(index: usize) -> Option<Planet> {
    (index < PLANET_COUNT)
        .then(|| Planet::try_from(index as u8).ok())
        .flatten()
}

/// Следующая «случайная» планета для кнопки `Random`.
///
/// Оригинал берёт `Randomizer_Generate(24)`; в Rust-клиенте выбор детерминирован
/// (без общего ГПСЧ UI), чтобы кадры и тесты были воспроизводимы.
pub fn random_planet(current: i64) -> i64 {
    ((current.clamp(0, MAX_WORLD) * 7 + 5) % (MAX_WORLD + 1)).clamp(0, MAX_WORLD)
}

/// Горизонтальные поля формы.
fn form_content(width: i32) -> (i32, i32) {
    let margin = horizontal_margin(width);
    let content = (width - margin * 2).max(60);
    let x = (width - content) / 2;

    (x, content)
}

/// Прямоугольник сетки планет внутри формы (24 ячейки 8×3).
///
/// В кадре 640×480 — точная область 3×2 слотов `PLANETSE` (`192,51 388×249`),
/// иначе — прежняя адаптивная раскладка для тестов/малых кадров.
pub fn planet_grid_rect(width: u32, height: u32) -> Rect {
    if width as i32 == menu::MENU_WIDTH && height as i32 == menu::MENU_HEIGHT {
        return menu::planet_grid_rect();
    }

    let width = width as i32;
    let height = height as i32;
    let (x, content) = form_content(width);
    let cell_height = ((height - 220) / PLANET_ROWS).clamp(30, 56);

    Rect::new(x, 24, content, cell_height * PLANET_ROWS)
}

/// Прямоугольник поля формы (оригинальная раскладка 3×2 в описании PLANETSE).
pub fn create_field_rect(field: CreateField) -> Rect {
    menu::CREATE_FIELD_RECTS
        .iter()
        .find(|(item, _)| *item == field)
        .map(|(_, rect)| Rect::new(rect.ulx, rect.uly, rect.width, rect.height))
        .unwrap_or(Rect::new(0, 0, 0, 0))
}

/// Прямоугольник одной планеты в сетке.
pub fn planet_cell_rect(width: u32, height: u32, index: usize) -> Rect {
    let grid = planet_grid_rect(width, height);
    let cell_width = (grid.width / PLANET_COLUMNS).max(1);
    let cell_height = (grid.height / PLANET_ROWS).max(1);
    let column = index as i32 % PLANET_COLUMNS;
    let row = index as i32 / PLANET_COLUMNS;

    Rect::new(
        grid.x + column * cell_width,
        grid.y + row * cell_height,
        cell_width,
        cell_height,
    )
}

/// Планета под точкой (`None` — точка вне сетки).
pub fn planet_cell_at(width: u32, height: u32, x: i32, y: i32) -> Option<usize> {
    for index in 0..PLANET_COUNT {
        let rect = planet_cell_rect(width, height, index);

        if x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height {
            return Some(index);
        }
    }

    None
}

fn build_create_form(
    width: u32,
    height: u32,
    status: &str,
    mouse: Option<Point>,
    form: &CreateForm,
) -> UiState {
    let width_i = width as i32;
    let height_i = height as i32;
    let (x, content) = form_content(width_i);
    let native_frame = width_i == menu::MENU_WIDTH && height_i == menu::MENU_HEIGHT;
    let theme = UiTheme::default();
    let empty = Rect::new(0, 0, 0, 0);

    let values = [
        (CreateField::Name, 24usize),
        (CreateField::World, 2),
        (CreateField::Timer, 4),
        (CreateField::EndTurn, 4),
        (CreateField::Opponent, 1),
        (CreateField::Password, 16),
    ];

    let (title_text, field_rects, back, create, random, help, hint_rect, status_rect) =
        if native_frame {
            (
                "Planet Select Menu",
                CreateField::ALL
                    .iter()
                    .map(|field| create_field_rect(*field))
                    .collect::<Vec<_>>(),
                Rect::new(354, 438, 109, 40),
                Rect::new(514, 438, 109, 40),
                Rect::new(243, 438, 109, 40),
                Rect::new(465, 438, 48, 40),
                Rect::new(41, 424, 559, 12),
                Rect::new(
                    menu::PLANET_STATUS.ulx,
                    menu::PLANET_STATUS.uly,
                    menu::PLANET_STATUS.width,
                    menu::PLANET_STATUS.height,
                ),
            )
        } else {
            let show_planets = height_i >= PLANET_GRID_MIN_HEIGHT as i32;
            let grid = planet_grid_rect(width, height);
            let fields_top = if show_planets {
                grid.y + grid.height + 6
            } else {
                28
            };
            let field_gap = 4;
            let field_height = ((height_i - fields_top - 88) / 6).clamp(12, 22);
            let rects = layout(
                Rect::new(x, fields_top, content, 0),
                &[field_height; 6],
                field_gap,
            );
            let buttons_y = fields_top + 6 * (field_height + field_gap) + 2;

            (
                if show_planets {
                    "NEW GAME - choose a planet"
                } else {
                    "NEW GAME - settings"
                },
                rects,
                Rect::new(x, buttons_y, 100, 24),
                Rect::new(x + content - 100, buttons_y, 100, 24),
                empty,
                empty,
                Rect::new(x, buttons_y + 32, content, 12),
                Rect::new(x, (height_i - 18).max(0), content, 12),
            )
        };

    let mut widgets = vec![Widget::Label(Label::colored(
        title_text,
        if native_frame {
            Rect::new(230, 6, 180, 20)
        } else {
            Rect::new(x, 10, content, 12)
        },
        theme.accent,
    ))];

    for ((field, max_chars), rect) in values.into_iter().zip(field_rects.iter().copied()) {
        let placeholder = match field {
            CreateField::Name => DEFAULT_GAME_NAME,
            CreateField::World => "0..23 (click a planet)",
            CreateField::Timer => "seconds",
            CreateField::EndTurn => "seconds",
            CreateField::Opponent => "1..3",
            CreateField::Password => "(open game)",
        };
        let text_box = TextBox::with_value(field.label(), form.value(field), placeholder, rect)
            .with_max_chars(max_chars);

        widgets.push(Widget::TextBox(text_box));
    }

    widgets.push(Widget::Button(Button::new("Cancel", back)));
    widgets.push(Widget::Button(Button::new("Done", create)));
    widgets.push(Widget::Button(
        Button::new("Random", random).with_enabled(!random.is_empty()),
    ));
    widgets.push(Widget::Button(
        Button::new("?", help).with_enabled(!help.is_empty()),
    ));

    // Подписи планет рисуются в `draw_planet_picker` шрифтом GNW Font5
    // (в C++ имена планет — `GNW_TEXT_FONT_5`), а не 8×8 Label'ами: иначе
    // «Valentine's Planet» обрезается до 8 символов.

    widgets.push(Widget::Label(Label::colored(
        if native_frame {
            "Click a planet; Tab/World - number; Enter - create; Esc - back"
        } else if height_i >= PLANET_GRID_MIN_HEIGHT as i32 {
            "Click a planet; Tab/World - number; Enter - create; Esc - back"
        } else {
            "Tab/Up/Down - field; Left/Right - numbers; Enter - create; Esc - back"
        },
        hint_rect,
        theme.text_disabled,
    )));
    widgets.push(Widget::Label(Label::colored(
        status,
        status_rect,
        theme.accent,
    )));

    let mut ui = UiState::new(widgets);
    ui.focus = Some(1 + form.focus.index());
    ui.set_hover(mouse);
    ui
}

/// Рисует картинки планет в сетку формы (`SNOW_PIC + world/6`).
///
/// `render_ui` заливает фон и подписи, поэтому картинки кладутся поверх — в
/// область над подписью, которую виджеты не занимают. Без ассетов рисуются
/// рамки-заглушки, чтобы сетка была видна и в headless-тестах.
fn draw_planet_picker(
    buffer: &mut RgbaBuffer,
    assets: Option<&crate::Assets>,
    form: &CreateForm,
    width: u32,
    height: u32,
) {
    if height < PLANET_GRID_MIN_HEIGHT {
        return;
    }

    let theme = UiTheme::default();

    for index in 0..PLANET_COUNT {
        let cell = planet_cell_rect(width, height, index);
        // Квадратная миниатюра: `SNOW_PIC` — 128×128, растягивать её по ширине
        // ячейки нельзя (планета превращается в блин).
        let size = (cell.width - 4).min(cell.height - 12).max(1);
        let image_rect = Rect::new(cell.x + (cell.width - size) / 2, cell.y + 1, size, size);
        let planet = Planet::try_from(index as u8).ok();
        let picture =
            planet.and_then(|planet| assets.and_then(|assets| assets.planet_picture(planet)));

        if let Some(picture) = picture {
            blit_big_image_scaled(buffer, picture, image_rect);
        } else {
            buffer.fill_rect(image_rect.into(), theme.panel);
            draw_text_line(
                buffer,
                image_rect.x + 3,
                image_rect.y + 3,
                &format!("{index}"),
                theme.text_disabled,
            );
        }

        // Имя планеты — GNW Font5 (как `planetselectmenu.cpp:124-138`),
        // по центру под миниатюрой; длинное имя обрезается по ширине ячейки.
        if let Some(planet) = planet {
            let font = GnwFontId::Font5.font();
            let max_width = (cell.width - 2).max(1);
            let mut name = planet_name(planet).to_string();

            while font.text_width(&name) > max_width {
                if name.pop().is_none() {
                    break;
                }
            }

            let x = cell.x + (cell.width - font.text_width(&name)).max(0) / 2;
            let y = cell.y + cell.height - font.height();

            draw_text_gnw(buffer, x, y, font, &name, theme.text);
        }

        let selected = form.world == index as i64;
        let border = if selected {
            Rgba::rgb(252, 0, 0)
        } else {
            theme.border
        };
        let thickness = if selected { 2 } else { 1 };

        draw_rect_border(buffer, cell, border, thickness);
    }
}

/// Рамка толщиной `thickness` внутри `rect`.
fn draw_rect_border(buffer: &mut RgbaBuffer, rect: Rect, color: Rgba, thickness: i32) {
    let thickness = thickness.max(1);

    for inset in 0..thickness {
        let x = rect.x + inset;
        let y = rect.y + inset;
        let width = rect.width - inset * 2;
        let height = rect.height - inset * 2;

        if width <= 0 || height <= 0 {
            break;
        }

        for dx in 0..width {
            buffer.set(x + dx, y, color);
            buffer.set(x + dx, y + height - 1, color);
        }
        for dy in 0..height {
            buffer.set(x, y + dy, color);
            buffer.set(x + width - 1, y + dy, color);
        }
    }
}

/// Масштабирует big image в `rect` (nearest neighbor), пропуская прозрачный
/// индекс — картинки планет 128×128 в ячейку 56×40.
fn blit_big_image_scaled(buffer: &mut RgbaBuffer, image: &max_assets::BigImage, rect: Rect) {
    if rect.width <= 0 || rect.height <= 0 || image.width <= 0 || image.height <= 0 {
        return;
    }

    for y in 0..rect.height {
        let source_y = (y * i32::from(image.height) / rect.height) as u32;

        for x in 0..rect.width {
            let source_x = (x * i32::from(image.width) / rect.width) as u32;
            let Some(index) = image.pixel(source_x, source_y) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            let [r, g, b] = image.color(index);

            buffer.set(rect.x + x, rect.y + y, Rgba::rgb(r, g, b));
        }
    }
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
        fn new(games: Vec<GameInfo>, created: Option<GameInfo>) -> (Self, Rc<RefCell<MockLog>>) {
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
    fn main_menu_opens_the_local_setup_and_escape_goes_back() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::MainMenu;

        // M-SP: «New Game» открывает настройку локальной партии (Options).
        assert_eq!(key(&mut screen, &mut ctx, Key::Enter), Transition::None);
        assert!(matches!(screen, Screen::LocalSetup(_)));
        if let Screen::LocalSetup(form) = &screen {
            assert_eq!(form.screen, crate::setup::SetupScreen::Options);
        }

        // Enter ведёт к следующему экрану (Planet).
        key(&mut screen, &mut ctx, Key::Enter);
        if let Screen::LocalSetup(form) = &screen {
            assert_eq!(form.screen, crate::setup::SetupScreen::Planet);
        }

        key(&mut screen, &mut ctx, Key::Escape);
        assert_eq!(screen, Screen::MainMenu);
    }

    #[test]
    fn multiplayer_menu_opens_the_hot_seat_setup() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::MainMenu;

        // Главное меню → «Multiplayer Game» (`menu.cpp:2361`).
        char(&mut screen, &mut ctx, 'j');
        assert_eq!(screen, Screen::MultiplayerMenu);

        // Кнопка `Hot Seat Game` (385,280) 200×29.
        let button = menu::MULTIPLAYER_BUTTONS[2];
        let transition = screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(
                button.rect.ulx + button.rect.width / 2,
                button.rect.uly + button.rect.height / 2,
            ),
        );

        assert_eq!(transition, Transition::None);
        match &screen {
            Screen::LocalSetup(form) => {
                assert!(form.hot_seat);
                assert_eq!(form.players_title(), "Hot Seat Menu");
                assert_eq!(form.setup.human_count(), 2);
                assert_eq!(form.screen, crate::setup::SetupScreen::Options);
            }
            other => panic!("ожидалась настройка hot-seat, получено {other:?}"),
        }

        // Esc с первого экрана возвращает в `Multiplayer Menu`, а не в главное.
        key(&mut screen, &mut ctx, Key::Escape);
        assert_eq!(screen, Screen::MultiplayerMenu);

        // Горячая клавиша оригинала `Shift+O` (`GNW_KB_KEY_SHIFT_O`).
        char(&mut screen, &mut ctx, 'o');
        assert!(matches!(screen, Screen::LocalSetup(_)));

        // `Cancel` (`Shift+C`) возвращает в главное меню.
        key(&mut screen, &mut ctx, Key::Escape);
        key(&mut screen, &mut ctx, Key::Escape);
        assert_eq!(screen, Screen::MainMenu);
    }

    #[test]
    fn planet_click_changes_world_and_keeps_numeric_fallback() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::CreateForm(CreateForm::default());

        // Клик по 13-й планете (индекс 12) выбирает её.
        let cell = planet_cell_rect(ctx.width, ctx.height, 12);
        let transition = screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(cell.x + cell.width / 2, cell.y + cell.height / 2),
        );

        assert_eq!(transition, Transition::None);
        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.world, 12);
            assert_eq!(form.focus, CreateField::World);
        } else {
            panic!("ожидалась форма создания");
        }

        // Числовой fallback: Tab до поля World и цифры по-прежнему работают.
        if let Screen::CreateForm(form) = &mut screen {
            form.focus = CreateField::Name;
            form.world = 0;
        }
        key(&mut screen, &mut ctx, Key::Tab);
        char(&mut screen, &mut ctx, '7');

        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.world, 7, "цифровой ввод — fallback");
        }

        // Клик по имени планеты (низ ячейки) тоже выбирает.
        let cell = planet_cell_rect(ctx.width, ctx.height, 23);
        screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(cell.x + 2, cell.y + cell.height - 2),
        );
        if let Screen::CreateForm(form) = &screen {
            assert_eq!(form.world, 23);
        }
    }

    #[test]
    fn planet_grid_is_eight_by_three_and_inside_the_form() {
        assert_eq!(PLANET_COLUMNS * PLANET_ROWS, PLANET_COUNT as i32);

        for (width, height) in [(512, 384), (640, 480), (320, 240)] {
            let grid = planet_grid_rect(width, height);
            assert!(grid.width > 0 && grid.height > 0);

            for index in 0..PLANET_COUNT {
                let cell = planet_cell_rect(width, height, index);
                assert!(cell.x >= grid.x && cell.x + cell.width <= grid.x + grid.width);
                assert!(cell.y >= grid.y && cell.y + cell.height <= grid.y + grid.height);
                assert_eq!(planet_cell_at(width, height, cell.x, cell.y), Some(index));
            }

            assert_eq!(planet_cell_at(width, height, 0, 0), None);
        }
    }

    #[test]
    fn create_form_paints_planet_pictures_from_max_res() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let max_res = root.join("max-original/MAX.RES");
        let color_pal = root.join("max-original/COLOR.PAL");
        let patches = root.join("assets/PATCHES.RES");

        if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
            return;
        }

        let config = AppConfig {
            max_res,
            color_pal,
            patches_res: patches,
            ..Default::default()
        };
        let mut app = crate::App::new(config);
        app.init_assets().expect("ассеты игры загружаются");

        let form = CreateForm {
            world: 12,
            ..Default::default()
        };
        let mut buffer = RgbaBuffer::filled(640, 480, Rgba::rgb(1, 2, 3));
        let view = ScreenView {
            width: 640,
            height: 480,
            status: "planets",
            mouse: None,
            focus: 0,
            app: Some(&app),
        };
        Screen::CreateForm(form).render(&mut buffer, view);

        let cell = planet_cell_rect(640, 480, 12);
        let image = Rect::new(cell.x + 3, cell.y + 3, cell.width - 6, cell.height - 16);
        let mut colors = std::collections::BTreeSet::new();

        for y in image.y..image.y + image.height {
            for x in image.x..image.x + image.width {
                if let Some(pixel) = buffer.get(x, y) {
                    colors.insert([pixel.r, pixel.g, pixel.b]);
                }
            }
        }

        assert!(
            colors.len() > 8,
            "картинка планеты должна давать больше 8 цветов, а не заливку: {colors:?}"
        );
    }

    #[test]
    fn create_form_creates_a_game_and_enters_the_session_with_mock_client() {
        let created = game("created-1", "Duel");
        let (mut ctx, log) = context(Vec::new(), Some(created.clone()));
        let mut screen = Screen::MainMenu;

        // Сетевая форма доступна из `Multiplayer Menu`: «Multiplayer Game» →
        // `Join Network Game` (список игр) → `c` (создать игру).
        char(&mut screen, &mut ctx, 'j');
        char(&mut screen, &mut ctx, 'j');
        char(&mut screen, &mut ctx, 'c');
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
        assert_eq!(
            log.list_calls, 1,
            "Create не возвращается в список: единственный ListGames — вход в список"
        );
        assert!(
            ctx.status.contains("Created \"Duel\""),
            "статус подтверждает создание: {}",
            ctx.status
        );
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
        let (mut ctx, log) = context(vec![game("g1", "Alpha"), game("g2", "Beta")], None);
        let mut screen = Screen::MainMenu;

        // MainMenu: «Multiplayer Game» открывает `Multiplayer Menu` (`menu.cpp:2361`).
        key(&mut screen, &mut ctx, Key::Down);
        key(&mut screen, &mut ctx, Key::Enter);
        assert_eq!(screen, Screen::MultiplayerMenu);

        // `Join Network Game` — список игр Horizon.
        char(&mut screen, &mut ctx, 'j');

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
        let mut screen = Screen::JoinList { games, selected: 0 };

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
        char(&mut screen, &mut ctx, 'j');

        // Ошибка сети не уводит из меню: остаёмся в `Multiplayer Menu` со
        // строкой статуса (список игр тоже доступен по повторному `j`).
        assert_eq!(screen, Screen::MultiplayerMenu);
        assert!(ctx.status.contains("no horizon"));
    }

    /// Пункт E: в кадре 640×480 меню рисуется из ресурсов `MAX.RES`, а не
    /// процедурно.
    #[test]
    fn native_frame_menus_are_drawn_from_max_res() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let max_res = root.join("max-original/MAX.RES");
        let color_pal = root.join("max-original/COLOR.PAL");
        let patches = root.join("assets/PATCHES.RES");

        if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
            return;
        }

        let mut app = crate::App::new(AppConfig {
            max_res,
            color_pal,
            patches_res: patches,
            ..Default::default()
        });
        app.init_assets().expect("ассеты игры");
        let menu = app
            .assets()
            .and_then(|assets| assets.menu.as_ref())
            .expect("MenuAssets из MAX.RES");

        let view = ScreenView {
            width: 640,
            height: 480,
            status: "status",
            mouse: Some(Point::new(390, 180)),
            focus: 0,
            app: Some(&app),
        };

        // Главное меню: фон — MAINPIC.
        let mut buffer = RgbaBuffer::new(640, 480);
        Screen::MainMenu.render(&mut buffer, view);
        assert_eq!(
            buffer.get(0, 0),
            menu.mainpic.rgb(0, 0).map(Rgba::from_rgb),
            "фон главного меню — MAINPIC"
        );

        // Форма создания: фон — PLANETSE, кнопки MNUBTN внизу.
        let mut buffer = RgbaBuffer::new(640, 480);
        Screen::CreateForm(CreateForm::default()).render(&mut buffer, view);
        assert_eq!(
            buffer.get(600, 300),
            menu.planet_background().rgb(600, 300).map(Rgba::from_rgb),
            "фон формы — PLANETSE"
        );

        // Список игр: фон — MULTGAME.
        let mut buffer = RgbaBuffer::new(640, 480);
        Screen::JoinList {
            games: vec![game("g1", "Alpha")],
            selected: 0,
        }
        .render(&mut buffer, view);
        assert_eq!(
            buffer.get(10, 10),
            menu.join_background().rgb(10, 10).map(Rgba::from_rgb),
            "фон списка игр — MULTGAME"
        );
    }

    /// Пункт E: клики по кнопкам главного меню попадают в оригинальные
    /// прямоугольники `menu.cpp` (а не в процедурную раскладку).
    #[test]
    fn main_menu_clicks_use_the_cpp_button_rectangles() {
        let (mut ctx, _log) = context(Vec::new(), None);
        ctx.width = 640;
        ctx.height = 480;
        let mut screen = Screen::MainMenu;

        let create = menu::main_menu_rect(menu::MainMenuAction::Create).expect("New Game");
        let transition = screen.handle_input(
            &mut ctx,
            InputEvent::MouseClick(
                create.ulx + create.width / 2,
                create.uly + create.height / 2,
            ),
        );

        assert_eq!(transition, Transition::None);
        assert!(
            matches!(screen, Screen::LocalSetup(_)),
            "New Game открывает настройку локальной партии"
        );

        let mut screen = Screen::MainMenu;
        let exit = menu::main_menu_rect(menu::MainMenuAction::Quit).expect("Exit");
        assert_eq!(
            screen.handle_input(&mut ctx, InputEvent::MouseClick(exit.ulx + 4, exit.uly + 4),),
            Transition::Quit
        );
    }

    /// Пункт F: клик в окне 1280×960 (масштаб 2) переводится в кадр 640×480 и
    /// попадает в ту же клетку карты, что и клик в кадре.
    #[test]
    fn window_click_at_2x_lands_in_the_same_map_cell() {
        use max_render::frame_scale::FrameScale;
        use max_render::layout;

        let scale = FrameScale::native(1280, 960);
        assert_eq!(scale.scale(), 2);

        let viewport = panels::map_viewport(&Viewport::new(640, 480));
        // Точка в области карты: (180,18) + 3.5 клетки по x и 2.5 по y.
        let frame_click = (
            layout::MAIN_MAP.ulx + 3 * 64 + 32,
            layout::MAIN_MAP.uly + 2 * 64 + 32,
        );
        let local = (
            frame_click.0 - layout::MAIN_MAP.ulx,
            frame_click.1 - layout::MAIN_MAP.uly,
        );
        let expected_grid = viewport.screen_to_grid(local.0, local.1);

        let window_click = scale.frame_to_window(frame_click.0, frame_click.1);
        let mapped = scale
            .window_to_frame(window_click.0 + 1, window_click.1 + 1)
            .expect("клик внутри кадра");
        let (map_x, map_y) = panels::screen_to_map(mapped.0, mapped.1).expect("точка на карте");
        let grid = viewport.screen_to_grid(map_x, map_y);

        assert_eq!(grid, expected_grid);
        assert_eq!(grid, (56, 55), "клетка под точкой (180+224, 18+160)");

        // Клик по чёрному полю окна 1000×700 (letterbox) не приходит в кадр.
        let letterboxed = FrameScale::native(1000, 700);
        assert_eq!(letterboxed.window_to_frame(10, 10), None);
    }

    #[test]
    fn setup_help_button_opens_and_closes_the_help_window() {
        let (mut ctx, _log) = context(Vec::new(), None);
        let mut screen = Screen::MainMenu;

        key(&mut screen, &mut ctx, Key::Enter);
        assert!(matches!(screen, Screen::LocalSetup(_)));

        // Кнопка `?` (465,438 48×40) на экране Options.
        let transition = screen.handle_input(&mut ctx, InputEvent::MouseClick(489, 458));
        assert_eq!(transition, Transition::None);

        match &screen {
            Screen::Help {
                screen, help, mode, ..
            } => {
                // C++ `HelpMenu_Menu`: кнопка `?` включает режим помощи, окна
                // ещё нет; запись выбирается кликом по элементу.
                assert!(help.is_none(), "режим помощи: окна ещё нет");
                assert_eq!(mode.section, HelpSection::Options);
                assert_eq!(mode.placement, HelpPlacement::Menu);
                assert!(matches!(
                    screen.as_ref(),
                    Screen::LocalSetup(form) if form.screen == crate::setup::SetupScreen::Options
                ));
            }
            other => panic!("ожидалось окно помощи, получено {other:?}"),
        }

        // Клик по элементу `PLAY_MODE` (426..632, 39..227) открывает запись.
        screen.handle_input(&mut ctx, InputEvent::MouseClick(529, 133));
        if let Screen::Help { help: Some(help), .. } = &screen {
            assert_eq!(help.section(), HelpSection::Options);
            assert_eq!(help.entry_name(), Some("PLAY_MODE"));
        } else {
            panic!("клик по элементу не открыл запись");
        }

        // `PageDown` прокручивает запись; `Esc` закрывает окно и возвращает в
        // режим (C++ `HelpMenu::Run` возвращает в `HelpMenu_Menu`).
        let rows_before = match &screen {
            Screen::Help { help: Some(help), .. } => help.line_count(),
            _ => 0,
        };
        screen.handle_input(&mut ctx, InputEvent::Key(Key::PageDown));
        if let Screen::Help { help: Some(help), .. } = &screen {
            if rows_before > crate::helpmenu::ROWS_PER_PAGE {
                assert_eq!(help.row_index(), crate::helpmenu::ROWS_PER_PAGE);
            } else {
                assert_eq!(help.row_index(), 0);
            }
        }

        screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape));
        assert!(
            matches!(&screen, Screen::Help { help: None, .. }),
            "Esc закрывает окно, режим остаётся"
        );

        // Второй `Esc` выключает режим и возвращает экран Options.
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape));
        assert!(matches!(
            screen,
            Screen::LocalSetup(form) if form.screen == crate::setup::SetupScreen::Options
        ));
    }

    #[test]
    fn research_menu_opens_for_a_powered_center_and_commits() {
        use max_core::enums::{ResearchTopic, UnitOrder, UnitType};
        use max_core::geo::Point;
        use max_core::team::TeamId;

        let (_ctx, _log) = context(Vec::new(), None);
        let mut app = App::new(AppConfig::default());
        let id = app
            .state_mut()
            .spawn_unit(UnitType::ResearchCenter, TeamId::RED, Point::new(20, 20), 0)
            .expect("center");
        let mut unit = app.state().unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.research_topic = ResearchTopic::Attack;
        app.state_mut().write_unit(id, unit);
        app.state_mut()
            .update_research_progress(TeamId::RED, ResearchTopic::Attack, 1);
        app.hud_mut().select(Some(id));

        assert!(app.open_research_menu(), "меню открывается для центра");
        assert_eq!(app.research_menu().map(|menu| menu.active_centers), Some(1));

        // Клавишей `R` меню не открывается повторно (уже открыто).
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(Key::Char('r')), false),
            Transition::None
        );

        // `←`/`→` и `Done` через тот же путь ввода, что у игрока.
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(Key::Down), false),
            Transition::None
        );
        handle_session_input(&mut app, InputEvent::Key(Key::Left), false);
        handle_session_input(&mut app, InputEvent::Key(Key::Right), false);
        assert_eq!(
            handle_session_input(&mut app, InputEvent::Key(Key::Enter), false),
            Transition::None
        );
        assert!(app.research_menu().is_none());
        assert_eq!(
            app.state().research_allocation(TeamId::RED)[ResearchTopic::Attack as usize],
            1
        );
    }

    #[test]
    fn panel_help_opens_game_screen_help_and_escape_closes_it() {
        use max_core::enums::{UnitOrder, UnitType};
        use max_core::geo::Point;
        use max_core::team::TeamId;

        let mut app = App::new(AppConfig::default());
        let id = app
            .state_mut()
            .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
            .expect("unit");
        let mut unit = app.state().unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::Await;
        app.state_mut().write_unit(id, unit);

        // `?`/кнопка HELP_OF включает режим помощи (`HelpMenu_Menu`): окна нет,
        // запись выбирается кликом по элементу.
        assert!(app.open_help(HelpSection::GameScreen));
        assert_eq!(
            app.help_mode().map(|mode| mode.section),
            Some(HelpSection::GameScreen)
        );
        assert!(app.help_screen().is_none());

        // Клик по панели `FILES` — запись `FILES_BUTTON` (17..83, 4..20).
        assert_eq!(
            handle_session_input(&mut app, InputEvent::MouseClick(50, 12), false),
            Transition::None
        );
        assert_eq!(
            app.help_screen().and_then(|help| help.entry_name()),
            Some("FILES_BUTTON")
        );

        // `Esc` закрывает окно и возвращает в режим (C++ `HelpMenu::Run`).
        handle_session_input(&mut app, InputEvent::Key(Key::Escape), false);
        assert!(app.help_screen().is_none());
        assert!(app.help_mode().is_some());

        // Второй `Esc` выключает режим.
        handle_session_input(&mut app, InputEvent::Key(Key::Escape), false);
        assert!(app.help_mode().is_none());
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
            Screen::MultiplayerMenu,
            Screen::Help {
                screen: Box::new(Screen::LocalSetup(GameSetupForm::default())),
                help: Some(helpmenu::HelpScreen::new(
                    HelpSection::Options,
                    HelpPlacement::Menu,
                )),
                mode: helpmenu::HelpMode::new(HelpSection::Options, HelpPlacement::Menu),
            },
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
        assert!(matches!(screen, Screen::LocalSetup(_)));
        assert_eq!(*calls.lock().expect("lock"), 1, "клик должен озвучиваться");
        assert_eq!(ctx.audio.plays(), 1);
    }
}
