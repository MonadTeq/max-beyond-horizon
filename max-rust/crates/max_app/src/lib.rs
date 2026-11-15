//! `max_app` — приложение M.A.X. как библиотека: `max_core` + `max_net` +
//! `max_assets` + `max_render` в одном игровом цикле.
//!
//! Крейт отделяет **тестируемую логику** (этот файл и модули [`assets`],
//! [`render`], [`net`]) от **бинарника** (`src/main.rs`): SDL3 живёт только в
//! `main.rs` под feature `sdl`, поэтому `cargo test -p max_app` не требует
//! окна, дисплея и SDL.
//!
//! # Что умеет [`App`]
//!
//! | Метод | Назначение |
//! |---|---|
//! | [`App::init_assets`] | читает `MAX.RES`, `PATCHES.RES`, `COLOR.PAL`, карту планеты (`SNOW_1`…`DESERT_6`) и спрайты [`assets::UNIT_SPRITES`] |
//! | [`App::load_game`] / [`App::load_save`] | V70-сейв → [`max_core::game::GameState`] (нормализации `SaveLoad_LoadFormatV70`) |
//! | [`App::new_game`] / [`App::new_game_on_surface`] | `setup::generate_map` + настройки + alien-обломки |
//! | [`App::tick`] | один тик симуляции (`GameManager_ProcessTick`) |
//! | [`App::apply`] | команда `max_core::input::Command` (сетевой пакет или локальный ввод) |
//! | [`App::render_frame`] | кадр [`max_render::buffer::RgbaBuffer`] без SDL |
//! | [`hud::render_hud`] | HUD (M4.2): миникарта, панель юнита, журнал, ресурсы, кнопки |
//! | [`audio::AudioPlayer`] | звук (M6.2): WAV-клипы, `play_once`/`play_loop`; вывод — за feature `sound` |
//! | [`hud::build_move_order`] | пакет 38 из выделения/клика (путь `Paths_RequestPath`) |
//! | [`hud::build_attack_order`] | пакет 08 (`P3`, `ORDER_MOVE_TO_ATTACK`) по врагу |
//! | [`App::digest`] | CRC/снапшот состояния для десинк-детектора и тестов |
//! | [`App::run_demo`] | headless-сценарий «загрузить сейв → N тиков → digest» |
//!
//! # Детерминизм
//!
//! Всё, что влияет на симуляцию, приходит из `max_core`: тик — это
//! [`GameState::tick`], команда — [`max_core::events::apply_command`]. Само
//! приложение не хранит собственного игрового состояния, кроме счётчиков
//! диагностики (`ticks`, `applied_commands`), поэтому два прогона одного сейва
//! дают одинаковый [`AppDigest`].

#![forbid(unsafe_code)]

pub mod assets;
pub mod audio;
pub mod chrome;
pub mod hud;
pub mod panels;
pub mod net;
pub mod render;
pub mod ui;

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use max_core::crc;
use max_core::enums::{GameStateType, Planet, TeamClan};
use max_core::events::{apply_command, ApplyOutcome};
use max_core::game::{GameState, PLAYER_TEAM_COUNT};
use max_core::geo::Point;
use max_core::input::Command;
use max_core::rng::Randomizer;
use max_core::save::SaveFile;
use max_core::setup::{
    generate_map, AlienUnitCosts, GameSetup, GenerationSettings, ResourceLevel, TeamStart,
};
use max_core::team::{TeamId, TeamKind};
use max_core::unit_values::{base_unit_values, ATTRIB_TURNS};
use max_core::{SurfaceMap, UnitType};
use max_render::viewport::Viewport;

pub use assets::{
    resolve_color_pal, resolve_patches_res, Assets, AssetsReport, LoadedSprite, SpriteInfo,
    PATCHES_RES_FALLBACKS, UNIT_SPRITES, WORLD_ASSETS,
};
pub use audio::{AudioBackend, AudioPlayer, NoopBackend, PlayMode, MENU_CLICK};
pub use hud::{
    build_attack_order, build_move_order, hud_layout, render_hud, team_color, unit_at_screen,
    HudButton, HudLayout, HudState, HudView, MessageLog, MoveOrder, PathPreview,
};
pub use net::{
    connect_horizon, run_network, run_network_with_sink, FrameSink, NetworkOptions, NetworkReport,
};
pub use panels::{PanelButton, PanelState, Panels};
pub use render::{write_ppm, RenderStats};
pub use ui::{
    handle_session_input, CreateField, CreateForm, HorizonMenuClient, InputEvent, Key, MenuClient,
    Screen, ScreenView, SessionRequest, Transition, UiContext, DEFAULT_GAME_NAME, MAX_WORLD,
};

/// Режим работы приложения (влияет только на CLI и диагностику, не на симуляцию).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// `--demo`: загрузить сейв, прокрутить N тиков, напечатать digest.
    Demo,
    /// `--new-game`: локальная партия из `setup::generate_map`.
    NewGame,
    /// `--connect`: Horizon + ENet-сессия.
    Connect,
    /// `--ui`: интерактивное меню (создать/выбрать игру) и сессия.
    Ui,
}

impl fmt::Display for AppMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Demo => "demo",
            Self::NewGame => "new-game",
            Self::Connect => "connect",
            Self::Ui => "ui",
        })
    }
}

/// Конфигурация приложения: пути к данным игры, сеть, имя игрока, режим.
///
/// Пути по умолчанию — от корня репозитория (`max-original/MAX.RES` и т.д.);
/// CLI переопределяет их флагами, а тесты — [`AppConfig::with_save`] и
/// [`App::new_game_on_surface`] (без файлов игры).
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// `MAX.RES` (карты `SNOW_*`…`DESERT_*`, спрайты, данные).
    pub max_res: PathBuf,
    /// `PATCHES.RES` (правила миссий, `SC_*`).
    pub patches_res: PathBuf,
    /// `PATCHES.RES`, из которого считается **только** `data_hash` хендшейка.
    ///
    /// `None` — тот же файл, что и [`Self::patches_res`] (старое поведение).
    /// Отдельный путь нужен, когда серверный `game_data` не содержит
    /// `PATCHES.RES` (его часть хэша пуста), а клиент должен остаться с
    /// полноценными ассетами: тогда `--patches` указывает на реальный архив,
    /// а `--handshake-patches` — на несуществующий файл, повторяя серверную
    /// семантику (`docs/protocol.md` §7).
    pub handshake_patches: Option<PathBuf>,
    /// `COLOR.PAL` (256 × RGB).
    pub color_pal: PathBuf,
    /// Каталог `.WRL`-карт; `None` — рядом с [`Self::max_res`].
    pub world_dir: Option<PathBuf>,
    /// Сейв для demo-режима (обычно `SAVE1.CAM`).
    pub save: Option<PathBuf>,
    /// Адрес gRPC Horizon (`--connect`).
    pub horizon_host: String,
    /// Порт gRPC Horizon (`docs/protocol-grpc.md` §4.4: по умолчанию 8082).
    pub horizon_port: u16,
    /// Имя игрока в лобби (пакет 36).
    pub player_name: String,
    /// Предпочитаемый слот команды 0..=3 (пакет 35).
    pub team_slot: u8,
    /// Клан команды (пакет 26).
    pub clan: TeamClan,
    /// Планета для [`App::init_assets`] и [`App::new_game`].
    pub planet: Planet,
    /// Ширина кадра (окно рендера / SDL).
    pub screen_width: u32,
    /// Высота кадра.
    pub screen_height: u32,
    /// Режим приложения.
    pub mode: AppMode,
    /// Сколько тиков прокручивать в demo-режиме.
    pub demo_turns: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_res: PathBuf::from("max-original/MAX.RES"),
            patches_res: PathBuf::from("assets/PATCHES.RES"),
            handshake_patches: None,
            color_pal: PathBuf::from("max-original/COLOR.PAL"),
            world_dir: None,
            save: None,
            horizon_host: "127.0.0.1".to_string(),
            horizon_port: 8082,
            player_name: "max_app".to_string(),
            team_slot: 0,
            clan: TeamClan::Random,
            planet: Planet::Snowcrab,
            screen_width: max_render::layout::WINDOW_WIDTH as u32,
            screen_height: max_render::layout::WINDOW_HEIGHT as u32,
            mode: AppMode::Demo,
            demo_turns: 5,
        }
    }
}

impl AppConfig {
    /// Конфигурация с сейвом.
    pub fn with_save(mut self, save: impl Into<PathBuf>) -> Self {
        self.save = Some(save.into());
        self
    }

    /// Сейв `dir/SAVE1.CAM` (удобно для `--data-dir`).
    pub fn with_save_in(mut self, dir: impl AsRef<Path>) -> Self {
        self.save = Some(dir.as_ref().join("SAVE1.CAM"));
        self
    }

    /// Каталог данных игры (`MAX.RES`, `COLOR.PAL`) — `max-original/`.
    pub fn with_data_dir(mut self, dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        self.max_res = dir.join("MAX.RES");
        self.color_pal = dir.join("COLOR.PAL");
        self
    }

    /// Каталог `.WRL`-карт (если он не совпадает с каталогом `MAX.RES`).
    pub fn with_world_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.world_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Каталог `assets/` с `PATCHES.RES`.
    pub fn with_assets_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.patches_res = dir.as_ref().join("PATCHES.RES");
        self
    }

    /// Отдельный `PATCHES.RES` **только** для `data_hash` хендшейка.
    ///
    /// Ассеты продолжают грузиться из [`Self::patches_res`] (или его
    /// fallback), поэтому путь может не существовать — как у сервера, чей
    /// `game_data` не содержит `PATCHES.RES` (`docs/protocol.md` §7).
    pub fn with_handshake_patches(mut self, path: impl Into<PathBuf>) -> Self {
        self.handshake_patches = Some(path.into());
        self
    }

    /// Путь `PATCHES.RES` для `data_hash` (`docs/protocol.md` §7).
    ///
    /// `handshake_patches == None` → assets-путь [`Self::patches_res`]
    /// (поведение до разделения путей).
    pub fn handshake_patches_path(&self) -> &Path {
        self.handshake_patches
            .as_deref()
            .unwrap_or(&self.patches_res)
    }

    /// Размер кадра.
    pub fn with_screen(mut self, width: u32, height: u32) -> Self {
        self.screen_width = width;
        self.screen_height = height;
        self
    }

    /// Адрес/порт gRPC Horizon.
    pub fn with_horizon(mut self, host: impl Into<String>, port: u16) -> Self {
        self.horizon_host = host.into();
        self.horizon_port = port;
        self
    }

    /// Имя игрока, слот и клан.
    pub fn with_player(mut self, name: impl Into<String>, team_slot: u8, clan: TeamClan) -> Self {
        self.player_name = name.into();
        self.team_slot = team_slot;
        self.clan = clan;
        self
    }

    /// Планета (`settings["world"]`).
    pub fn with_planet(mut self, planet: Planet) -> Self {
        self.planet = planet;
        self
    }

    /// Режим.
    pub fn with_mode(mut self, mode: AppMode) -> Self {
        self.mode = mode;
        self
    }

    /// Число тиков demo-режима.
    pub fn with_demo_turns(mut self, turns: u32) -> Self {
        self.demo_turns = turns;
        self
    }
}

/// Параметры новой локальной партии.
///
/// [`Self::p2_reference`] — ровно вход эталонного вектора P2
/// `p00_l111_d0_s0` (`max-rust/tools/map_vectors/vectors.txt`): планета 0,
/// уровни 1/1/1/0, seed 0, старты `(RED, 20, 20)` и `(BLUE, 70, 70)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGameConfig {
    /// Планета.
    pub planet: Planet,
    /// Seed ГПСЧ (`Remote_RngSeed`).
    pub seed: u32,
    /// Уровень материалов (`raw_resource` 0–2).
    pub raw: ResourceLevel,
    /// Уровень топлива.
    pub fuel: ResourceLevel,
    /// Уровень золота.
    pub gold: ResourceLevel,
    /// Уровень alien-обломков.
    pub derelicts: ResourceLevel,
    /// Стартовые позиции команд (мутируются `FindValidStartingPosition`).
    pub teams: Vec<TeamStart>,
    /// Какая команда — локальный игрок.
    pub player_team: TeamId,
    /// `start_gold` — золото команды на старте (`settings.cpp:118`, 150).
    ///
    /// Из него же считается `team_gold` после высадки: в custom-игре
    /// стартовая загрузка `SC_L0001` выдаётся бесплатно, апгрейдов нет.
    pub start_gold: u32,
}

impl Default for NewGameConfig {
    fn default() -> Self {
        Self::p2_reference()
    }
}

impl NewGameConfig {
    /// Вход эталонного вектора P2 `p00_l111_d0_s0`.
    pub fn p2_reference() -> Self {
        Self {
            planet: Planet::Snowcrab,
            seed: 0,
            raw: ResourceLevel::NORMAL,
            fuel: ResourceLevel::NORMAL,
            gold: ResourceLevel::NORMAL,
            derelicts: ResourceLevel::LOW,
            teams: vec![
                TeamStart::new(TeamId::RED, Point::new(20, 20)),
                TeamStart::new(TeamId::BLUE, Point::new(70, 70)),
            ],
            player_team: TeamId::RED,
            start_gold: max_core::deploy::DEFAULT_START_GOLD,
        }
    }
}

/// Результат [`App::new_game`] / [`App::new_game_on_surface`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewGameReport {
    /// Планета.
    pub planet: Planet,
    /// Seed ГПСЧ.
    pub seed: u32,
    /// Сколько команд участвует.
    pub teams: usize,
    /// Сколько alien-обломков вернул `setup::generate_map` (порядок
    /// `UnitsManager_DeployUnit`; все они развёрнуты в `GameState` под
    /// `TeamId::ALIEN`).
    pub aliens: usize,
    /// Сколько стартовых юнитов высадил `deploy::deploy_starting_units`
    /// (`GameManager_ProcessTeamMissionSupplyUnits` для каждой команды).
    pub start_units: usize,
    /// Сколько юнитов в состоянии после генерации (`aliens` + `start_units`).
    pub units: usize,
    /// CRC16 поверхности карты.
    pub surface_crc: u16,
    /// CRC16 карты ресурсов.
    pub resources_crc: u16,
}

/// Результат одного [`App::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// Номер тика приложения (1-based).
    pub tick: u64,
    /// `GameManager_TurnCounter`.
    pub turn_counter: i32,
    /// `GameManager_GameState`.
    pub game_state: GameStateType,
    /// Число юнитов после тика.
    pub units: usize,
}

/// Канонический дайджест состояния — десинк-детектор приложения.
///
/// Три независимых значения `max_core::crc`:
///
/// * `state_digest` — CRC16 полного [`GameState::snapshot_bytes`] (все
///   симуляционные поля);
/// * `state_checksum` — CRC16 пакета 45 (`GameManager_CheckDesync`), его же
///   сравнивает сервер в lockstep-сессии;
/// * `replay_crc` — CRC16 дайджеста реплея (`crc::replay_digest_bytes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppDigest {
    /// Сколько тиков прокрутило приложение.
    pub ticks: u64,
    /// Номер хода.
    pub turn_counter: i32,
    /// Фаза партии.
    pub game_state: GameStateType,
    /// Число юнитов.
    pub units: usize,
    /// CRC16 снапшота состояния.
    pub state_digest: u16,
    /// CRC16 пакета 45.
    pub state_checksum: u16,
    /// CRC16 дайджеста реплея.
    pub replay_crc: u16,
    /// Длина снапшота в байтах.
    pub snapshot_len: usize,
}

impl AppDigest {
    /// Снимает дайджест с состояния.
    pub fn capture(state: &GameState, ticks: u64) -> Self {
        let snapshot = state.snapshot_bytes();

        Self {
            ticks,
            turn_counter: state.turn_counter,
            game_state: state.game_state,
            units: state.unit_count(),
            state_digest: crc::crc16(&snapshot),
            state_checksum: crc::state_checksum(state),
            replay_crc: crc::replay_digest_crc(state),
            snapshot_len: snapshot.len(),
        }
    }

    /// Однострочное представление для логов CLI.
    pub fn to_line(self) -> String {
        format!(
            "ticks={} turn={} state={:?} units={} state_digest=0x{:04X} state_checksum=0x{:04X} replay_crc=0x{:04X} snapshot={}B",
            self.ticks,
            self.turn_counter,
            self.game_state,
            self.units,
            self.state_digest,
            self.state_checksum,
            self.replay_crc,
            self.snapshot_len
        )
    }
}

impl fmt::Display for AppDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_line())
    }
}

/// Приложение: состояние партии + ассеты + камера + счётчики.
///
/// Создаётся без файлов игры ([`App::new`]); ассеты подгружаются явно
/// ([`App::init_assets`]), а состояние — сейвом ([`App::load_game`]) или
/// генерацией ([`App::new_game`]). Всё остальное (тик, команды, кадр,
/// дайджест) работает и без ассетов: карта рендерится упрощённо, юниты —
/// маркерами команд.
#[derive(Debug, Clone)]
pub struct App {
    config: AppConfig,
    state: GameState,
    assets: Option<Assets>,
    viewport: Viewport,
    ticks: u64,
    applied_commands: u64,
    /// UI-состояние сессии (M4.2): выделенный юнит, журнал, предпросмотр
    /// маршрута и приказы из ввода. В снапшот/CRC **не входит** — выделение
    /// не должно менять детерминизм симуляции ([`AppDigest`] его не видит).
    hud: hud::HudState,
}

impl App {
    /// Пустое приложение (без ассетов; состояние — пустая партия `GameState::new`).
    pub fn new(config: AppConfig) -> Self {
        let viewport = Viewport::new(config.screen_width, config.screen_height);

        Self {
            config,
            state: GameState::new(),
            assets: None,
            viewport,
            ticks: 0,
            applied_commands: 0,
            hud: hud::HudState::new(),
        }
    }

    /// Конфигурация.
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Изменяемая конфигурация (например, перед `init_assets`).
    pub fn config_mut(&mut self) -> &mut AppConfig {
        &mut self.config
    }

    /// Состояние партии.
    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// Изменяемое состояние партии.
    pub fn state_mut(&mut self) -> &mut GameState {
        &mut self.state
    }

    /// Ассеты игры (`None` до [`App::init_assets`]).
    pub fn assets(&self) -> Option<&Assets> {
        self.assets.as_ref()
    }

    /// Камера/масштаб кадра.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Активен ли хром окна 1:1 (`FRAMEPIC` и кадр ровно 640×480).
    pub fn chrome_active(&self) -> bool {
        self.viewport.screen()
            == (
                max_render::layout::WINDOW_WIDTH as u32,
                max_render::layout::WINDOW_HEIGHT as u32,
            )
            && self
                .assets
                .as_ref()
                .and_then(|assets| assets.chrome.as_ref())
                .is_some_and(|chrome| chrome.is_ready())
    }

    /// Камера области карты окна: 448×448, тот же центр и зум.
    ///
    /// При активном хроме карта рисуется в собственный буфер 448×448
    /// (`WINDOW_MAIN_MAP`), поэтому и мышь переводится этой камерой, а не
    /// камерой всего окна.
    pub fn map_viewport(&self) -> Viewport {
        if self.chrome_active() {
            crate::panels::map_viewport(&self.viewport)
        } else {
            self.viewport
        }
    }

    /// Изменяемая камера.
    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }

    /// Сколько тиков прокручено с момента загрузки/генерации.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Сколько команд успешно применено.
    pub fn applied_commands(&self) -> u64 {
        self.applied_commands
    }

    /// Грузит данные игры из путей [`AppConfig`].
    ///
    /// Возвращает отчёт; повторный вызов перечитывает файлы.
    pub fn init_assets(&mut self) -> Result<AssetsReport> {
        let assets = Assets::load(&self.config)?;
        let report = assets.report();

        for warning in &report.warnings {
            tracing::warn!("assets: {warning}");
        }
        tracing::info!(
            "assets: MAX.RES={} PATCHES.RES={} world={} ({}) {}x{} sprites={}",
            report.max_res_entries,
            report.patches_res_entries,
            report.world,
            report.world_source,
            report.world_size.0,
            report.world_size.1,
            report.sprite_count()
        );

        self.assets = Some(assets);

        Ok(report)
    }

    /// Загружает сейв из [`AppConfig::save`].
    ///
    /// # Ошибки
    ///
    /// Сейв не задан или не читается/не разбирается как V70.
    pub fn load_game(&mut self) -> Result<()> {
        let save = self
            .config
            .save
            .clone()
            .context("в AppConfig не задан путь к сейву (--demo <save>)")?;

        self.load_save(save)
    }

    /// Загружает V70-сейв по пути.
    ///
    /// Дальше состояние нормализовано ([`GameState::from_save`]) и готово к
    /// [`App::tick`]; счётчики тиков/команд сбрасываются.
    pub fn load_save(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .with_context(|| format!("не удалось прочитать сейв {}", path.display()))?;

        self.load_save_bytes(&bytes)
            .with_context(|| format!("не удалось разобрать сейв {}", path.display()))
    }

    /// Загружает V70-сейв из байтов (тесты и встроенные фикстуры).
    pub fn load_save_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let save = SaveFile::parse(bytes).map_err(|error| anyhow::anyhow!("{error}"))?;

        self.state = GameState::from_save(&save);
        self.ticks = 0;
        self.applied_commands = 0;
        self.hud.clear();
        self.focus_player();

        Ok(())
    }

    /// Создаёт локальную партию по [`NewGameConfig`] на карте планеты из ассетов.
    ///
    /// Если ассеты ещё не загружены или загружены для другой планеты, они
    /// перечитываются из [`AppConfig`] с `planet = config.planet`: карта
    /// (`.WRL`) — часть данных игры, а не состояния партии.
    pub fn new_game(&mut self, config: &NewGameConfig) -> Result<NewGameReport> {
        self.config.planet = config.planet;

        if self.assets.as_ref().map(|assets| assets.planet) != Some(config.planet) {
            self.init_assets()?;
        }

        let surface = self
            .assets
            .as_ref()
            .expect("assets loaded above")
            .world
            .surface_map();

        Ok(self.new_game_on_surface(config, &surface))
    }

    /// Создаёт локальную партию на готовой карте поверхности.
    ///
    /// Порт `GameManager_InitGame` в объёме P2 + M3.2: `setup::generate_map`
    /// (ресурсы + alien-обломки), затем `GameState::with_setup`, развёртывание
    /// alien-обломков (`UnitsManager_DeployUnit` с `team = PLAYER_TEAM_ALIEN`),
    /// высадка стартовых армий
    /// ([`max_core::deploy::deploy_starting_units`] —
    /// `GameManager_ProcessTeamMissionSupplyUnits`) и продолжение потока ГПСЧ
    /// (как в C++, где `Randomizer_SetSeed` стоит в начале генерации партии).
    ///
    /// # Стартовые армии
    ///
    /// Каждая играющая команда получает шахту, генератор, две плиты и загрузку
    /// `SC_L0001` (`CONSTRCT` 40, `ENGINEER` 20, `SURVEYOR` 0), золото —
    /// [`NewGameConfig::start_gold`]. Без этого условия победы/поражения
    /// (`SC_L0002`) считают команду выбитой уже на первом ходу.
    ///
    /// # Alien-обломки
    ///
    /// Порядок [`NewGameReport::aliens`] — это порядок `setup::generate_map`
    /// (плита → здание → мосты → юниты), в котором C++
    /// `GameManager_SpawnAlienDerelicts` вызывает `UnitsManager_DeployUnit`.
    /// Юниты создаются в `GameState` от имени `TeamId::ALIEN`; тепловые карты
    /// и командная статистика/условия победы их не видят (как в C++:
    /// у `PLAYER_TEAM_ALIEN` `team_type == TEAM_TYPE_NONE` и `heat_map == null`).
    pub fn new_game_on_surface(
        &mut self,
        config: &NewGameConfig,
        surface: &SurfaceMap,
    ) -> NewGameReport {
        let settings = GenerationSettings::from_menu_levels(
            config.raw,
            config.fuel,
            config.gold,
            config.derelicts,
        );
        let alien_costs = AlienUnitCosts {
            alien_tank: base_unit_values(UnitType::AlienTank).get_attribute(ATTRIB_TURNS),
            juggernaut: base_unit_values(UnitType::Juggernaut).get_attribute(ATTRIB_TURNS),
            alien_assault_gun: base_unit_values(UnitType::AlienAssaultGun)
                .get_attribute(ATTRIB_TURNS),
            alien_plane: base_unit_values(UnitType::AlienPlane).get_attribute(ATTRIB_TURNS),
        };
        let mut setup = GameSetup {
            planet: config.planet,
            settings,
            teams: config.teams.clone(),
            alien_costs,
        };
        let mut rng = Randomizer::new(config.seed);
        let output = generate_map(&mut setup, surface, &mut rng);

        // Команды из setup (позиции могли сдвинуться `FindValidStartingPosition`).
        let mut team_kinds = [TeamKind::None; PLAYER_TEAM_COUNT];
        for start in &setup.teams {
            let index = start.team.0 as usize;
            if index < PLAYER_TEAM_COUNT {
                team_kinds[index] = if start.team == config.player_team {
                    TeamKind::Player
                } else {
                    TeamKind::Computer
                };
            }
        }

        let mut state = GameState::with_setup(output.map, team_kinds);
        state.player_team = config.player_team;
        // Поток ГПСЧ продолжается после генерации — как в оригинале.
        state.rng = rng;

        // `GameManager_SpawnAlienDerelicts`: все размещения — `DeployUnit` с
        // `team = PLAYER_TEAM_ALIEN`, в порядке генерации. C++ передаёт
        // `skip_map_status_update = true`; в max_core `update_map_status`
        // сам игнорирует `ALIEN` (у команды нет heat-map), поэтому
        // результат тот же — командные слои не трогаются.
        for placement in &output.aliens {
            let _ = state.spawn_unit(
                placement.unit_type,
                TeamId::ALIEN,
                placement.position,
                0,
            );
        }

        // `GameManager_ProcessTeamMissionSupplyUnits` для каждой команды
        // (`team_type != NONE`), в порядке возрастания индекса команды и уже
        // после alien-обломков — ровно как в `GameManager_InitGame`. Без этого
        // `menu_check_end_game_conditions` завершает партию сразу после первого
        // хода: у команд нет ни инфраструктуры, ни войск.
        let start_units = max_core::deploy::deploy_starting_units(
            &mut state,
            config.start_gold,
            setup.settings.max_resources,
            &setup.teams,
        );

        let report = NewGameReport {
            planet: config.planet,
            seed: config.seed,
            teams: setup.teams.len(),
            aliens: output.aliens.len(),
            start_units,
            units: state.unit_count(),
            surface_crc: crc::crc16(&state.map.surface.to_bytes()),
            resources_crc: crc::crc16(&state.map.resources.to_le_bytes()),
        };

        tracing::info!(
            "new game: planet={:?} seed={} teams={} aliens={} start_units={} units={} surface_crc=0x{:04X}",
            report.planet,
            report.seed,
            report.teams,
            report.aliens,
            report.start_units,
            report.units,
            report.surface_crc
        );

        self.state = state;
        self.ticks = 0;
        self.applied_commands = 0;
        self.hud.clear();
        self.focus_player();

        report
    }

    /// Один тик симуляции (`GameManager_ProcessTick`).
    pub fn tick(&mut self) -> TickReport {
        self.state.tick();
        self.ticks += 1;

        TickReport {
            tick: self.ticks,
            turn_counter: self.state.turn_counter,
            game_state: self.state.game_state,
            units: self.state.unit_count(),
        }
    }

    /// Применяет команду сразу (порядок команд задаёт вызывающий).
    ///
    /// Для сетевого/lockstep-цикла команды приходят в [`max_net::SessionEvent`]
    /// и применяются в порядке получения — ровно как `ProcessNetPackets`.
    pub fn apply(&mut self, command: Command) -> ApplyOutcome {
        let outcome = apply_command(&mut self.state, &command);

        if outcome == ApplyOutcome::Applied {
            self.applied_commands += 1;
        }

        outcome
    }

    /// Кладёт команду в очередь `GameState` (применится на следующем тике).
    pub fn enqueue(&mut self, command: Command) {
        self.state.enqueue(command);
    }

    /// Канонический дайджест состояния.
    pub fn digest(&self) -> AppDigest {
        AppDigest::capture(&self.state, self.ticks)
    }

    /// Канонические байты состояния ([`GameState::snapshot_bytes`]).
    ///
    /// Тот же снапшот, CRC которого входит в [`AppDigest::state_digest`];
    /// удобно сохранить в файл и сравнить два прогона побайтово.
    pub fn snapshot(&self) -> Vec<u8> {
        self.state.snapshot_bytes()
    }

    /// Headless demo: загрузить сейв, прокрутить `turns` тиков, вернуть digest.
    ///
    /// Это ровно сценарий `--demo`, поэтому тест и CLI проверяют один код.
    pub fn run_demo(&mut self, save: impl AsRef<Path>, turns: u32) -> Result<AppDigest> {
        self.load_save(save)?;

        for _ in 0..turns {
            self.tick();
        }

        Ok(self.digest())
    }

    /// Центрирует камеру на первом юните локального игрока (или на карте).
    pub fn focus_player(&mut self) {
        let unit = self
            .state
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| self.state.unit(id))
            .find(|unit| unit.team == self.state.player_team)
            .map(|unit| unit.grid);

        if let Some(grid) = unit {
            self.viewport.center_on_grid(grid.x, grid.y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_points_at_the_repository_layout() {
        let config = AppConfig::default();

        assert_eq!(config.mode, AppMode::Demo);
        assert_eq!(config.demo_turns, 5);
        assert_eq!(config.horizon_port, 8082);
        assert_eq!(config.max_res, PathBuf::from("max-original/MAX.RES"));
        assert_eq!(config.save, None);
    }

    #[test]
    fn p2_reference_matches_the_vector_case() {
        let config = NewGameConfig::p2_reference();

        assert_eq!(config.planet, Planet::Snowcrab);
        assert_eq!(config.seed, 0);
        assert_eq!(config.raw.value(), 1);
        assert_eq!(config.fuel.value(), 1);
        assert_eq!(config.gold.value(), 1);
        assert_eq!(config.derelicts.value(), 0);
        assert_eq!(
            config.teams,
            vec![
                TeamStart::new(TeamId::RED, Point::new(20, 20)),
                TeamStart::new(TeamId::BLUE, Point::new(70, 70)),
            ]
        );
    }

    #[test]
    fn digest_line_has_the_documented_fields() {
        let digest = AppDigest::capture(&GameState::new(), 0);
        let line = digest.to_line();

        assert!(line.contains("ticks=0"));
        assert!(line.contains("state_digest=0x"));
        assert!(line.contains("replay_crc=0x"));
        assert_eq!(digest, AppDigest::capture(&GameState::new(), 0));
    }

    /// Уникальный временный каталог теста (без внешних крейтов).
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "max_app_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn handshake_patches_default_to_the_assets_path() {
        let config = AppConfig::default();

        assert_eq!(config.handshake_patches, None);
        assert_eq!(
            config.handshake_patches_path(),
            config.patches_res.as_path(),
            "None → хэш из assets-пути patches_res, как раньше"
        );
    }

    #[test]
    fn handshake_patches_override_only_the_hash_path() {
        let config = AppConfig::default().with_handshake_patches("/tmp/nonexistent.res");

        assert_eq!(
            config.handshake_patches_path(),
            Path::new("/tmp/nonexistent.res")
        );
        assert_eq!(
            config.patches_res,
            PathBuf::from("assets/PATCHES.RES"),
            "ассетный путь не меняется"
        );
    }

    #[test]
    fn data_hash_uses_handshake_patches_and_leaves_the_patches_part_empty_when_missing() {
        let dir = temp_dir("handshake_hash");
        let max_res = dir.join("MAX.RES");
        let patches = dir.join("PATCHES.RES");
        std::fs::write(&max_res, b"max-res-bytes").expect("write MAX.RES");
        std::fs::write(&patches, b"patches-bytes").expect("write PATCHES.RES");

        // None → PATCHES-часть хэша считается по assets-пути.
        let assets_only = AppConfig {
            max_res: max_res.clone(),
            patches_res: patches.clone(),
            ..Default::default()
        };
        let hash = max_net::DataHash::from_files(
            &assets_only.max_res,
            assets_only.handshake_patches_path(),
        )
        .expect("hash files");
        assert!(hash.max_res.is_some());
        assert!(
            hash.patches_res.is_some(),
            "None → хэш из assets-пути patches_res"
        );

        // Some(несуществующий путь) → пустая PATCHES-часть, но ассеты остаются.
        let split = AppConfig {
            max_res,
            patches_res: patches,
            handshake_patches: Some(dir.join("nonexistent.res")),
            ..Default::default()
        };
        let hash =
            max_net::DataHash::from_files(&split.max_res, split.handshake_patches_path())
                .expect("hash files");
        assert!(hash.max_res.is_some());
        assert!(
            hash.patches_res.is_none(),
            "путь хендшейка не существует → хэш без PATCHES-части (семантика сервера)"
        );
        assert_eq!(
            split.handshake_patches_path(),
            dir.join("nonexistent.res").as_path()
        );

        std::fs::remove_dir_all(dir).ok();
    }
}
