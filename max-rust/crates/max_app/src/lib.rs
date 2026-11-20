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

// `deny`, а не `forbid`: единственное исключение — чтение RSS процесса для
// soak-прогонов P4 (`mem.rs`, Windows: `GetProcessMemoryInfo`). Оно помечено
// `#[allow(unsafe_code)]` точечно; остальной крейт по-прежнему без unsafe.
#![deny(unsafe_code)]

pub mod allocmenu;
pub mod assets;
pub mod audio;
pub mod buildmenu;
pub mod buildsite;
pub mod cargo_unit_data;
pub mod chrome;
pub mod credits;
pub mod cursor;
pub mod dialog;
pub mod helpmenu;
pub mod hotseat;
pub mod hud;
pub mod intro;
pub mod landing;
pub mod local;
pub mod mem;
pub mod menu;
pub mod mobilize;
pub mod net;
pub mod panels;
pub mod portrait;
pub mod preferences;
pub mod pregame;
pub mod profile;
pub mod qa;
pub mod render;
pub mod researchmenu;
pub mod saveload;
pub mod scroll;
pub mod selfdestruct;
pub mod setup;
pub mod sfx;
pub mod soak;
pub mod transfermenu;
pub mod ui;
pub mod unitmenu;
pub mod upgrademenu;
pub mod winloss;

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use max_core::crc;
use max_core::deploy::TeamDeployment;
use max_core::enums::{GameStateType, Planet, TeamClan, UnitOrder, UnitOrderState};
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
use max_core::unit::UnitId;
use max_core::unit_values::{base_unit_values, ATTRIB_TURNS};
use max_core::{SurfaceMap, UnitType};
use max_render::viewport::Viewport;

pub use allocmenu::{AllocAction, AllocMenu, AllocMenuAssets};
pub use assets::{
    data_search_roots, resolve_color_pal, resolve_max_res, resolve_patches_res, Assets,
    AssetsReport, LoadedSprite, SpriteInfo, COLOR_PAL_NAME, DATA_DIR_NAME, MAX_RES_NAME,
    PATCHES_RES_FALLBACKS, PLANET_GROUP_PICTURES, UNIT_SPRITES, UNIT_TYPES_WITHOUT_SHADOW_DATA,
    UNIT_TYPES_WITHOUT_SPRITE_DATA, WORLD_ASSETS,
};
pub use audio::{AudioBackend, AudioPlayer, NoopBackend, PlayMode, MENU_CLICK};
pub use buildmenu::{BuildMenu, BuildMenuAction, BuildMenuAssets};
pub use cursor::{cursor_for_context, CursorContext, CursorHover, CursorId, CursorImage};
pub use hotseat::TurnNotice;
pub use hud::{
    build_attack_order, build_move_order, build_power_order, hud_layout, render_hud, team_color,
    unit_at_screen, HudButton, HudLayout, HudState, HudView, MessageLog, MoveOrder, PathPreview,
};
pub use landing::{LandingAction, LandingScreen};
pub use local::{LocalGame, LocalOutcome, LocalPhase, LocalSetup, LocalUpdate, LOCAL_TEAM_COUNT};
pub use mem::{memory_line, process_memory, ProcessMemory};
pub use mobilize::Mobilization;
pub use net::{
    connect_horizon, run_network, run_network_with_sink, FrameSink, NetworkOptions, NetworkReport,
};
pub use panels::{PanelButton, PanelState, Panels};
pub use preferences::{
    PrefKey, Preferences, PreferencesAction, PreferencesAssets, PreferencesDialog, PreferencesInfo,
    PreferencesVariant,
};
pub use pregame::{PreGameFlow, PreGameStage, PreGameStep};
pub use profile::{format_ns, Meter, MeterStats, ProfileSnapshot, FRAME_BUDGET_NS};
pub use render::{
    draw_unit_sprites, draw_unit_sprites_with_angle, draw_unit_sprites_with_base, unit_frame_index,
    unit_frame_index_for_angle, write_ppm, RenderStats, UnitSpriteStats,
};
pub use saveload::{
    save_meta_for_local_game, save_meta_for_local_game_with, SaveLoadAction, SaveLoadAssets,
    SaveLoadMenu, SaveLoadMode, AUTOSAVE_SLOT, LAST_SLOT, SLOTS_PER_PAGE,
};
pub use soak::{run_soak, SoakOptions, SoakReport, SoakTurn};
pub use transfermenu::{TransferAction, TransferMenu, TransferMenuAssets};
pub use ui::{
    handle_session_input, planet_cell_at, planet_cell_rect, planet_grid_rect, planet_name,
    CreateField, CreateForm, HorizonMenuClient, InputEvent, Key, MenuClient, Screen, ScreenView,
    SessionRequest, Transition, UiContext, DEFAULT_GAME_NAME, MAX_WORLD, PLANET_COLUMNS,
    PLANET_COUNT, PLANET_GRID_MIN_HEIGHT, PLANET_NAMES, PLANET_ROWS,
};
pub use unitmenu::{
    MenuEvent, UnitMenu, UnitMenuAssets, UnitMenuItem, CURSOR_LOAD, CURSOR_RELOAD, CURSOR_REPAIR,
    CURSOR_TRANSFER,
};
pub use upgrademenu::{UpgradeAction, UpgradeFilter, UpgradeMenu, UpgradeMenuAssets};

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
    /// Каталог V70-сейвов (`save1.cam`, автосейв `save10.*`). `None` — рядом с
    /// `MAX.RES` (как `FilePathGameData`/`FilePathGamePref` в C++).
    pub save_dir: Option<PathBuf>,
    /// Автосейв в конце хода (`SETUP.auto_save`, по умолчанию включён).
    pub autosave: bool,
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
            save_dir: None,
            autosave: true,
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

    /// Каталог V70-сейвов (сохранение/загрузка, автосейв).
    pub fn with_save_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.save_dir = Some(dir.into());
        self
    }

    /// Включает/выключает автосейв (`SETUP.auto_save`).
    pub fn with_autosave(mut self, enabled: bool) -> Self {
        self.autosave = enabled;
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
    /// Клан каждой из четырёх игровых команд (`menu_team_clan_setting`).
    /// `TeamClan::Random` — C++ выберет клан случайно.
    pub clans: [TeamClan; PLAYER_TEAM_COUNT],
    /// Какая команда — локальный игрок.
    pub player_team: TeamId,
    /// `start_gold` — золото команды на старте (`settings.cpp:118`, 150).
    ///
    /// Из него же считается `team_gold` после высадки: в custom-игре
    /// стартовая загрузка `SC_L0001` выдаётся бесплатно, апгрейдов нет.
    pub start_gold: u32,
    /// Готовые к высадке команды после фаз старта (`CargoMenu` + site select).
    ///
    /// Пусто — старый путь: всем командам [`Self::start_gold`] и
    /// [`max_core::deploy::DEFAULT_MISSION_LOADOUT`] по [`Self::teams`].
    pub deployments: Vec<TeamDeployment>,
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
            clans: [TeamClan::Random; PLAYER_TEAM_COUNT],
            player_team: TeamId::RED,
            start_gold: max_core::deploy::DEFAULT_START_GOLD,
            deployments: Vec::new(),
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
    /// Состояние локальной партии M-SP (`None` — сети/демо/меню).
    pub(crate) local: Option<local::LocalGame>,
    /// Открытый `BuildMenu` (P2). В снапшот/CRC не входит — это UI.
    build_menu: Option<buildmenu::BuildMenu>,
    /// Режим выбора места стройки (`ORDER_STATE_SELECT_SITE`, C++
    /// `GameManager_TempTape` + `GameManager_GetBuilderUnitCursor`). В
    /// снапшот/CRC не входит — до подтверждения приказ не отдан.
    pub(crate) build_site: Option<buildsite::BuildSiteSelect>,
    /// Открытое окно сохранения/загрузки V70 (P3). В снапшот/CRC не входит.
    pub(crate) save_load: Option<saveload::SaveLoadMenu>,
    /// Открытый экран покупных апгрейдов (P6). В снапшот/CRC не входит.
    upgrade_menu: Option<upgrademenu::UpgradeMenu>,
    /// Открытый экран исследований (P7, `ResearchMenu`). В снапшот/CRC не входит.
    research_menu: Option<researchmenu::ResearchMenu>,
    /// Открытое меню команд выделенного юнита (C++ `GameManager_PopupButtons`).
    /// В снапшот/CRC не входит — это UI.
    unit_menu: Option<unitmenu::UnitMenu>,
    /// Открытый диалог самоуничтожения здания (`UnitsManager_SelfDestructMenu`,
    /// окна `SELFDSTR`/`SLFDOPN`/`SLFDOK`). В снапшот/CRC не входит.
    self_destruct: Option<selfdestruct::SelfDestructMenu>,
    /// Очередь звуков UI за текущий кадр ([`sfx::Sfx`]): проигрывает `main.rs`
    /// (`AudioPlayer`), тесты проверяют по ней события. В снапшот/CRC не входит.
    sounds: Vec<sfx::Sfx>,
    /// Открытый диалог паузы (`PauseMenu_Menu` → `DialogMenu`, `Alt+P`).
    pause_dialog: Option<dialog::DialogMenu>,
    /// Какой была пауза до открытия диалога (для восстановления после `OK`).
    pause_was_paused: bool,
    /// Открытый диалог распределения добычи шахты (C++ `AllocMenu`, окно
    /// `ALLOCFRM`). В снапшот/CRC не входит — это UI; значения живут в полях
    /// юнитов (`raw_mining`/`fuel_mining`/`gold_mining`).
    alloc_menu: Option<allocmenu::AllocMenu>,
    /// Открытое окно X-FER (C++ `TransferMenu`, `XFERPIC`). В снапшот/CRC не
    /// входит: количество уходит приказом **08/P3** (`transfer_cargo`).
    transfer_menu: Option<transfermenu::TransferMenu>,
    /// Открытое окно помощи поверх игры (P7, `HelpMenu`). В снапшот/CRC не входит.
    help: Option<helpmenu::HelpScreen>,
    /// Режим помощи C++ (`HelpMenu_Menu`, `helpmenu.cpp:315`): курсор
    /// `CURSOR_HELP`, окно открывается кликом по элементу. В снапшот/CRC не входит.
    help_mode: Option<helpmenu::HelpMode>,
    /// Визуальные позиции юнитов в клетках (дробные) — только для отрисовки.
    ///
    /// Симуляция по-прежнему двигает юнита на целую клетку за тик; кадр идёт
    /// по фактическому маршруту клетка за клеткой с реальным временем
    /// ([`App::update_visual_units`]). В CRC/снапшот эти значения не входят.
    visual_units: std::collections::HashMap<UnitId, (f32, f32)>,
    /// Последняя известная симуляционная клетка юнита: по её изменениям
    /// строится визуальный маршрут. UI-состояние, в снапшот не входит.
    visual_goals: std::collections::HashMap<UnitId, (i16, i16)>,
    /// Очередь клеток маршрута, которые кадр ещё должен пройти. Симуляция
    /// исполняет приказ за тики (быстро), а кадр проходит те же клетки с
    /// реальной скоростью оригинала (`UnitInfo::Move`: 8 px/тик ~ 3.75 клетки/с).
    /// В каждой точке — направление движения к ней (`VisualStep.angle`),
    /// чтобы спрайт поворачивался на клетке поворота, а не раньше.
    visual_paths: std::collections::HashMap<UnitId, std::collections::VecDeque<VisualStep>>,
    /// Визуальные кадровые часы юнитов (`FrameAnimation`).
    ///
    /// Три механизма: цикл строителя (`ProcessOrderBuild`), анимация
    /// `ANIMATED`-зданий (`ProcessOrderSentry`, например RADAR) и
    /// **промежуточный** цикл кадров работающих заводов
    /// ([`WORKING_BUILDING_ANIM_FRAME_SECONDS`]). В снапшот/CRC не входят:
    /// симуляция не хранит «номер кадра» — она хранит только
    /// `moved`/`image_index`, которые C++ использует как счётчик. `dt = 0`
    /// (QA/headless) — статичный кадр C++, детерминированно.
    frame_anims: std::collections::HashMap<UnitId, FrameAnim>,
    /// Часы палитровых циклов (`World::InitColorCycles` +
    /// `World::UpdateColorAnimations`): 11 диапазонов слотов DAC, которые
    /// вращаются с частотой 2/6/8/9/10 fps. Именно они дают мигание маяков
    /// зданий и индикатора дрели — кадры спрайта при этом не листаются
    /// (`max-original-dasm/formats/unit-animation.md` §7.3). В снапшот/CRC не
    /// входят; `dt = 0` (QA/headless) оставляет палитру исходной.
    color_cycles: max_render::palette::ColorCycles,
    /// Снимок системной палитры с повёрнутыми диапазонами (кэш на кадр).
    ///
    /// `None` — циклы ещё не шагнули (или `dt = 0`): рисуется
    /// `Assets::system_palette` без изменений.
    cycle_palette: Option<max_render::palette::Palette>,
    /// «Вырастание» только что поставленного юнита (`UnitsManager_ScaleUnit`).
    ///
    /// `scaler_adjust` — **симуляционное** поле (`UnitInfo::RenderSprite`:
    /// масштаб `2^-scaler`); здесь — только визуальный переход к полному
    /// размеру по реальному `dt`. `dt = 0` (QA/headless) — полный размер.
    growth_anims: std::collections::HashMap<UnitId, GrowthAnim>,
    /// FLC-анимация портрета выделенного юнита (`WINDOW_CORNER_FLIC`).
    ///
    /// UI-состояние: в снапшот/CRC не входит. `None` — юнит не выбран, у типа
    /// нет `F_*`-ресурса или FLC не загрузился (тогда портрет — спрайт ×2,
    /// см. `panels::draw_portrait`).
    portrait: Option<portrait::PortraitAnim>,
    /// Пользовательские настройки (`SETUP`/`PREFERENCES`/`OPTIONS` из
    /// `settings.cpp`). В снапшот/CRC не входят: на симуляцию влияет только
    /// `effects` (кадры) и звук.
    pub(crate) preferences: Preferences,
    /// Открытый диалог `Preferences` (кнопка панели, C++ `OptionsMenu`).
    /// В снапшот/CRC не входит — это UI.
    pub(crate) preferences_dialog: Option<PreferencesDialog>,
    /// Номер кадра мигания маркеров стройки `BLDMRK1..5`
    /// (`DrawMap_BuildMarkImageIndex`) и секунды до следующего.
    ///
    /// C++ меняет кадр каждые три отрисовки карты
    /// (`DrawMap_BuildMMarkDelayCounter`), порт — по визуальным часам
    /// ([`BUILD_MARKER_FRAME_SECONDS`], 10 кадров/с, как цикл строителя); при
    /// `dt = 0` (QA/headless) кадр статичен и golden детерминированны.
    pub(crate) build_marker_step: u8,
    pub(crate) build_marker_elapsed: f32,
    /// Цвет индикатора выделения — `GameManager_MarkerColor`
    /// (`game_manager.cpp:222`): `0xFF` (белый) или `0x00` (чёрный).
    /// Переключается каждые [`render::UNIT_SELECTOR_BLINK_SECONDS`] (210 мс,
    /// `game_manager.cpp:1189-1192`); при `dt = 0` (QA/headless) — `0xFF`.
    pub(crate) marker_color_index: u8,
    pub(crate) marker_color_elapsed: f32,
    /// Мышь и «зажатая» ЛКМ для скролла камеры по краям
    /// (`GameManager_ProcessInput`/`GameManager_SetGridOffset`). В снапшот/CRC
    /// не входит — это чистый ввод.
    pub(crate) scroll: scroll::ScrollState,
}

/// Визуальные кадровые часы одного юнита.
///
/// Каждый счётчик живёт независимо: строитель листает кадры на 10 Гц
/// ([`BUILDER_ANIM_FRAME_SECONDS`]), `ANIMATED`-здание — на каждом тике C++
/// ([`SENTRY_ANIM_FRAME_SECONDS`]), **промежуточный** полный цикл работающего
/// здания — со скоростью `RADAR` ([`render::WORKING_BUILDING_ANIM_FRAME_SECONDS`],
/// 1/30 с). Все они — только для кадра и в снапшот/CRC не входят.
#[derive(Debug, Clone, Copy, Default)]
struct FrameAnim {
    /// Секунды с прошлого сдвига кадра строителя.
    builder_elapsed: f32,
    /// `0` — `+16` (начало стройки), `1` — `+24`, `2` — `+32` (C++ `+8`).
    builder_step: u8,
    /// Секунды с прошлого тика `ProcessOrderSentry`.
    sentry_elapsed: f32,
    /// Номер тика `ProcessOrderSentry` (ANIMATED-здание).
    sentry_step: u32,
    /// Секунды с прошлого кадра полного цикла работающего здания.
    work_elapsed: f32,
    /// Номер кадра цикла (`0` — статичный кадр C++, дальше по кругу).
    work_step: u32,
}

/// Визуальный переход «вырастания» от `scaler_adjust` к полному размеру.
#[derive(Debug, Clone, Copy)]
struct GrowthAnim {
    /// Масштаб в момент первого кадра (`render::scaler_scale`), 1/16..1/2.
    start: f32,
    /// Секунды анимации; после [`GROWTH_ANIM_SECONDS`] — полный размер.
    elapsed: f32,
}

/// Секунд на один кадр рабочего цикла строителя.
///
/// C++ `UnitsManager_ProcessOrderBuild` листает кадр на каждом втором тике
/// (`moved & 1`), а тик оригинала — ~30 Гц; здесь кадры идут по реальному
/// времени интерфейса, 10 кадров/с — цикл `+16 → +24 → +32` за 0.3 с.
pub const BUILDER_ANIM_FRAME_SECONDS: f32 = 0.1;

/// Кадров в цикле маркеров стройки — `BLDMRK1..5`
/// (`DrawMap_BuildMarkImages`, `drawmap.cpp:49`).
pub const BUILD_MARKER_FRAMES: usize = 5;

/// Секунд на кадр мигания маркеров стройки.
///
/// C++ меняет кадр каждые три отрисовки карты (`DrawMap_BuildMMarkDelayCounter`
/// = 3), а карта рисуется ~30 раз в секунду → ~10 кадров/с. Порт берёт ту же
/// частоту через визуальные часы, потому что его частота кадров не равна 30 Гц.
pub const BUILD_MARKER_FRAME_SECONDS: f32 = 0.1;

/// Секунд на один кадр `ProcessOrderSentry` (`ANIMATED`-здание, RADAR).
///
/// C++ листает кадр на каждом тике обработки приказов (`UnitsManager_ProcessTick`
/// → `UnitsManager_ProcessOrders` → `UnitsManager_ProcessOrderSentry`), а тик —
/// ~30 Гц (`TickTimer`, `timer_get`). В интерактиве эти часы идут по реальному
/// `dt`: 1/30 с на кадр, 16 кадров RADAR — оборот за ~0.53 с.
pub const SENTRY_ANIM_FRAME_SECONDS: f32 = 1.0 / 30.0;

/// Скорость и включение промежуточного цикла кадров работающих заводов.
///
/// **ПРОМЕЖУТОЧНОЕ правило** до съёмки работающего завода: завод листает блок
/// `image_base + angle .. image_index_max` по кругу с той же скоростью, что
/// `RADAR` (1 кадр за 1/30 с). Обе ручки — «одна строка на откат»: скорость в
/// [`render::WORKING_BUILDING_ANIM_FRAME_SECONDS`], включение — в
/// [`render::WORKING_BUILDING_ANIMATION_ENABLED`]. У шахты цикла нет:
/// DOS-съёмка показала статичный корпус в idle — кадры дрели включаются
/// отдельной константой [`render::MINING_DRILL_FRAMES_ENABLED`].
pub use crate::render::{
    MINING_DRILL_FRAMES_ENABLED, WORKING_BUILDING_ANIMATION_ENABLED,
    WORKING_BUILDING_ANIM_FRAME_SECONDS,
};

/// Длительность визуального «вырастания» юнита, секунды.
///
/// C++ `UnitsManager_ProcessOrderAwaitScaling` уменьшает `scaler_adjust` на 1
/// за тик обработки приказов (5 шагов: 1/16 → 1). В интерактиве шаги
/// сглаживаются по реальному времени; `dt = 0` (QA/headless) сразу даёт
/// полный размер, чтобы кадры оставались детерминированными.
pub const GROWTH_ANIM_SECONDS: f32 = 1.0;

/// Скорость видимого движения юнитов по карте — клеток в секунду.
///
/// Оригинал: `UnitInfo::Move` (`unitinfo.cpp:1730`) двигает юнита на `velocity`
/// пикселей за тик, а `Paths_CalculateStep` (`paths.cpp:292`) ограничивает
/// `max_velocity` восемью пикселями; тик — примерно 30 Гц (`TickTimer`,
/// `timer_get`). Одна клетка — `GFX_MAP_TILE_SIZE = 64` px: 8 × 30 / 64 ≈
/// **3.75 клетки/с**. Берём 4 — так кадр проходит маршрут за то же время, что
/// и пиксельное движение в C++.
pub const VISUAL_CELLS_PER_SECOND: f32 = 4.0;

/// Скачок больше этого числа клеток — не маршрут, а телепорт/синхронизация
/// состояния: визуальная позиция ставится в цель без анимации.
pub const VISUAL_TELEPORT_CELLS: i32 = 6;

/// Один шаг визуального маршрута юнита: точка в клетках и направление движения
/// к ней (`Paths_GetAngle`, 0..7).
///
/// Угол запоминается **на отрезке**, а не читается из `UnitInfo::angle` в
/// момент кадра. Симуляция исполняет маршрут быстро (клетка за тик) и уже
/// повернула к следующей клетке — или прошла несколько клеток вперёд, — пока
/// спрайт по-прежнему идёт по этому отрезку с реальной скоростью. Без снимка
/// угла кадр показывал бы поворот на 45–90° раньше, чем спрайт дошёл до клетки
/// поворота: юнит «смотрел бы» вбок, двигаясь прямо.
#[derive(Debug, Clone, Copy, PartialEq)]
struct VisualStep {
    /// Координата клетки (дробная — на промежуточных точках скачка).
    x: f32,
    /// Координата клетки (дробная).
    y: f32,
    /// Направление движения к этой точке: `Paths_GetAngle(dx, dy)`.
    angle: u8,
}

/// Приказ выделенного юнита на начало тика — для авто-выделения
/// (`Preferences::auto_select`) после завершения приказа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderSnapshot {
    /// Id юнита на момент снимка.
    pub id: UnitId,
    /// Приказ (`UnitInfo::orders`).
    pub orders: UnitOrder,
    /// Состояние исполнения (`UnitInfo::state`).
    pub state: UnitOrderState,
    /// Был ли маршрут (`UnitInfo::path`).
    pub has_path: bool,
    /// Очки движения на начало тика (`UnitInfo::speed`).
    pub speed: u16,
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
            local: None,
            build_menu: None,
            build_site: None,
            save_load: None,
            upgrade_menu: None,
            research_menu: None,
            unit_menu: None,
            self_destruct: None,
            sounds: Vec::new(),
            pause_dialog: None,
            pause_was_paused: false,
            alloc_menu: None,
            transfer_menu: None,
            help: None,
            help_mode: None,
            visual_units: std::collections::HashMap::new(),
            visual_goals: std::collections::HashMap::new(),
            visual_paths: std::collections::HashMap::new(),
            frame_anims: std::collections::HashMap::new(),
            color_cycles: max_render::palette::ColorCycles::new(),
            cycle_palette: None,
            growth_anims: std::collections::HashMap::new(),
            portrait: None,
            preferences: Preferences::default(),
            preferences_dialog: None,
            build_marker_step: 0,
            build_marker_elapsed: 0.0,
            marker_color_index: 0xFF,
            marker_color_elapsed: 0.0,
            scroll: scroll::ScrollState::new(),
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

    /// Открытый `BuildMenu` (P2); `None` — экран закрыт.
    pub fn build_menu(&self) -> Option<&buildmenu::BuildMenu> {
        self.build_menu.as_ref()
    }

    /// Открытый `BuildMenu` для ввода (изменяемый).
    pub fn build_menu_mut(&mut self) -> Option<&mut buildmenu::BuildMenu> {
        self.build_menu.as_mut()
    }

    /// Открывает `BuildMenu` для выделенного юнита — кнопка `Build` в C++
    /// (`GameManager_MenuClickBuildButton` → `BuildMenu_Menu`).
    ///
    /// `false`, если юнит не выделен или не умеет строить.
    pub fn open_build_menu(&mut self) -> bool {
        let Some(id) = self.hud.selected else {
            return false;
        };

        let Some(menu) = buildmenu::BuildMenu::new(&self.state, id) else {
            return false;
        };

        if menu.team != self.state.player_team {
            return false;
        }

        self.build_menu = Some(menu);
        true
    }

    /// Закрывает `BuildMenu` (Esc/`Cancel`/после заказа).
    pub fn close_build_menu(&mut self) {
        self.build_menu = None;
    }

    /// Открытый экран апгрейдов (P6); `None` — закрыт.
    pub fn upgrade_menu(&self) -> Option<&upgrademenu::UpgradeMenu> {
        self.upgrade_menu.as_ref()
    }

    /// Открытый экран апгрейдов для ввода.
    pub fn upgrade_menu_mut(&mut self) -> Option<&mut upgrademenu::UpgradeMenu> {
        self.upgrade_menu.as_mut()
    }

    /// Открывает экран апгрейдов — C++ `UpgradeMenu(team, complex).Run()`
    /// (пункт `buy upg` меню команд, `units_manager.cpp:956`).
    ///
    /// Вызывается пунктом `buy upg` попапа ([`unitmenu::ItemId::BuyUpgrade`]) и
    /// клавишей `U` (быстрый путь) — для выделенного юнита своей команды.
    pub fn open_upgrade_menu(&mut self) -> bool {
        let Some(team) = self.hud.selected.and_then(|id| self.state.unit(id)) else {
            return false;
        };
        let team = team.team;
        if team != self.state.player_team {
            return false;
        }
        let Some(menu) = upgrademenu::UpgradeMenu::new(&self.state, team) else {
            return false;
        };
        self.upgrade_menu = Some(menu);
        true
    }

    /// Закрывает экран апгрейдов без фиксации.
    pub fn close_upgrade_menu(&mut self) {
        self.upgrade_menu = None;
    }

    /// Фиксирует изменения (`CommitUpgradeChanges`) и закрывает экран.
    ///
    /// В локальной игре изменения уже применены к `TeamUnits`; в сети каждый
    /// изменённый тип уходит пакетом **10** (`Command::UpgradeUnitType`).
    /// Пакет 09 (золото команды) в Rust-протоколе отсутствует — ограничение
    /// задокументировано в `upgrademenu` (шапка модуля).
    pub fn commit_upgrade_menu(&mut self, online: bool) -> Option<String> {
        let mut menu = self.upgrade_menu.take()?;
        let changed = menu.commit(&mut self.state);

        if online {
            for (unit_type, values) in &changed {
                self.hud.pending.push(Command::UpgradeUnitType(
                    max_core::Packet10UpgradeUnitType {
                        team: menu.team,
                        unit_type: *unit_type,
                        values: *values,
                    },
                ));
            }
        }

        let gold = self
            .state
            .team_units
            .get(menu.team.0 as usize)
            .map(|units| units.gold)
            .unwrap_or(0);

        Some(if changed.is_empty() {
            format!("Upgrades: no changes (gold {gold})")
        } else {
            format!(
                "Upgrades: {} unit type(s) changed (gold {gold})",
                changed.len()
            )
        })
    }

    /// Ввод в открытом экране апгрейдов — как `AbstractUpgradeMenu::ProcessKey`.
    pub fn upgrade_menu_input(&mut self, event: InputEvent) -> Option<upgrademenu::UpgradeAction> {
        let Some(mut menu) = self.upgrade_menu.take() else {
            return None;
        };
        let action = match event {
            InputEvent::MouseClick(x, y) => menu.click(&self.state, x, y),
            InputEvent::Key(key) => menu.key(&self.state, key),
            _ => upgrademenu::UpgradeAction::None,
        };
        self.upgrade_menu = Some(menu);
        Some(action)
    }

    /// Выбор строки списка типов в открытом экране апгрейдов (QA/клавиатура).
    pub fn upgrade_menu_select(&mut self, index: usize) -> bool {
        let Some(mut menu) = self.upgrade_menu.take() else {
            return false;
        };
        if index >= menu.unit_types.len() {
            self.upgrade_menu = Some(menu);
            return false;
        }
        menu.selected = index;
        menu.page_min = menu.page_min.min(
            menu.unit_types
                .len()
                .saturating_sub(crate::upgrademenu::LIST_ROWS as usize),
        );
        menu.rebuild_controls(&self.state);
        self.upgrade_menu = Some(menu);
        true
    }

    /// Стрелка апгрейда строки `row` в открытом экране (QA).
    pub fn upgrade_menu_adjust(&mut self, row: usize, increase: bool) -> bool {
        let Some(mut menu) = self.upgrade_menu.take() else {
            return false;
        };
        let changed = menu.adjust(&self.state, row, increase);
        self.upgrade_menu = Some(menu);
        changed
    }

    // -----------------------------------------------------------------------
    // Экран исследований (`ResearchMenu`, P7)
    // -----------------------------------------------------------------------

    /// Открытый экран исследований; `None` — закрыт.
    pub fn research_menu(&self) -> Option<&researchmenu::ResearchMenu> {
        self.research_menu.as_ref()
    }

    /// Открытый экран исследований для ввода.
    pub fn research_menu_mut(&mut self) -> Option<&mut researchmenu::ResearchMenu> {
        self.research_menu.as_mut()
    }

    /// Открывает экран исследований для выделенного включённого
    /// `RESEARCH`-центра своей команды — C++ popup `Research`
    /// (`UnitsManager_Popup_OnClick_Research`, пункт `resrch` меню команд);
    /// вызывается пунктом попапа и клавишей `R` (быстрый путь).
    pub fn open_research_menu(&mut self) -> bool {
        let Some(id) = self.hud.selected else {
            return false;
        };
        let Some(unit) = self.state.unit(id) else {
            return false;
        };
        if unit.team != self.state.player_team
            || unit.unit_type != UnitType::ResearchCenter
            || unit.orders != max_core::enums::UnitOrder::PowerOn
        {
            return false;
        }
        let Some(menu) = researchmenu::ResearchMenu::new(&self.state, unit.team) else {
            return false;
        };

        self.research_menu = Some(menu);
        true
    }

    /// Закрывает экран исследований без применения распределения.
    pub fn close_research_menu(&mut self) {
        self.research_menu = None;
    }

    /// `ResearchMenu_OnClick_Done`: применить распределение центров к
    /// `GameState` (`set_research_allocation`) и закрыть экран.
    pub fn commit_research_menu(&mut self) -> Option<String> {
        let mut menu = self.research_menu.take()?;
        let actual = menu.commit(&mut self.state);
        let total: u32 = actual.iter().sum();

        Some(format!("Research: {} center(s) assigned (Done)", total))
    }

    /// Ввод в открытом экране исследований — как `ResearchMenu::Run`.
    pub fn research_menu_input(
        &mut self,
        event: InputEvent,
    ) -> Option<researchmenu::ResearchAction> {
        let Some(mut menu) = self.research_menu.take() else {
            return None;
        };
        let action = menu.input(event);
        self.research_menu = Some(menu);
        Some(action)
    }

    /// Выбор темы (клавиатура/QA).
    pub fn research_menu_select(&mut self, index: usize) -> bool {
        let Some(menu) = self.research_menu.as_mut() else {
            return false;
        };
        if index >= max_core::enums::RESEARCH_TOPIC_COUNT {
            return false;
        }
        menu.selected = index;
        true
    }

    /// Стрелка темы `row` (QA): `+1`/`−1` центр, как клик по `UPGRGT`/`UPGLFT`.
    pub fn research_menu_adjust(&mut self, row: usize, increase: bool) -> bool {
        let Some(menu) = self.research_menu.as_mut() else {
            return false;
        };
        menu.adjust(row, increase)
    }

    // -----------------------------------------------------------------------
    // Меню команд выделенного юнита (C++ `GameManager_PopupButtons`)
    // -----------------------------------------------------------------------

    /// Открытое меню команд юнита; `None` — закрыто.
    pub fn unit_menu(&self) -> Option<&unitmenu::UnitMenu> {
        self.unit_menu.as_ref()
    }

    /// Открытое меню команд для ввода.
    pub fn unit_menu_mut(&mut self) -> Option<&mut unitmenu::UnitMenu> {
        self.unit_menu.as_mut()
    }

    /// Открывает меню команд выделенного юнита — C++ `GameManager_InitPopupButtons`.
    ///
    /// `false`, если юнита нет, он чужой или пунктов не набралось (C++
    /// `popup_count == 0`, `unit->GetOrder() == ORDER_DISABLE`, `UNIT_READY`
    /// у наземного юнита — см. [`unitmenu::can_open`]).
    pub fn open_unit_menu(&mut self) -> bool {
        let Some(id) = self.hud.selected else {
            return false;
        };
        let Some(unit) = self.state.unit(id).cloned() else {
            return false;
        };

        if unit.team != self.state.player_team {
            return false;
        }

        if unit.team != self.state.active_turn_team {
            return false;
        }

        let area = if self.chrome_active() {
            max_render::layout::MAIN_MAP
        } else {
            let (width, height) = self.viewport.screen();
            max_render::buffer::PixelRect::new(0, 0, width as i32, height as i32)
        };

        // Якорь попапа — ровно как C++ `GameManager_InitPopupButtons`:
        // `ulx = (unit->x + 32)`, `uly = unit->y - height / 2`, где `unit->x/y`
        // — центр клетки (`UnitInfo::SetPosition`: `grid*64 + 32`), то есть
        // меню встаёт справа от клетки юнита и по центру её высоты.
        let viewport = self.map_viewport();
        let (center_x, center_y) = crate::hud::tile_center(&viewport, unit.grid);
        let zoom = viewport.zoom();
        let anchor = (center_x + zoom as i32 / 2 + area.ulx, center_y + area.uly);

        let Some(mut menu) = unitmenu::UnitMenu::new(&self.state, &unit, anchor, area, zoom) else {
            return false;
        };

        // C++ `state[i]`: пункт нажат, если включён его режим
        // (`unit->cursor == 3/4/6/7`, `unit->enter_mode`).
        for item in &mut menu.items {
            item.pressed = match item.id {
                unitmenu::ItemId::Transfer => self.hud.cursor_mode == unitmenu::CURSOR_TRANSFER,
                unitmenu::ItemId::Repair => self.hud.cursor_mode == unitmenu::CURSOR_REPAIR,
                unitmenu::ItemId::Reload => self.hud.cursor_mode == unitmenu::CURSOR_RELOAD,
                unitmenu::ItemId::Load => self.hud.cursor_mode == unitmenu::CURSOR_LOAD,
                unitmenu::ItemId::Disable => self.hud.cursor_mode == unitmenu::CURSOR_DISABLE,
                unitmenu::ItemId::Steal => self.hud.cursor_mode == unitmenu::CURSOR_STEAL,
                unitmenu::ItemId::Attack => self.hud.targeting_mode,
                unitmenu::ItemId::Enter => self.hud.enter_mode,
                _ => item.pressed,
            };
        }

        self.unit_menu = Some(menu);
        true
    }

    /// Закрывает меню команд (`GameManager_DeinitPopupButtons`).
    pub fn close_unit_menu(&mut self) {
        self.unit_menu = None;
    }

    /// Ввод в открытом меню команд — [`unitmenu::menu_event`].
    pub fn unit_menu_input(&mut self, event: InputEvent) -> Option<unitmenu::MenuEvent> {
        let Some(mut menu) = self.unit_menu.take() else {
            return None;
        };
        let outcome = unitmenu::menu_event(&menu, event);
        menu.hover = match outcome {
            unitmenu::MenuEvent::Hover(index) => index,
            _ => menu.hover,
        };
        self.unit_menu = Some(menu);
        Some(outcome)
    }

    // -----------------------------------------------------------------------
    // Распределение добычи шахты (C++ `AllocMenu`, окно `ALLOCFRM`)
    // -----------------------------------------------------------------------

    /// Открытый диалог распределения добычи; `None` — закрыт.
    pub fn alloc_menu(&self) -> Option<&allocmenu::AllocMenu> {
        self.alloc_menu.as_ref()
    }

    /// Комплекс выделенного юнита (`unit->GetComplex()`), для QA/журнала.
    pub fn selected_complex(&self) -> Option<u16> {
        let id = self.hud.selected?;

        Some(self.state.unit(id)?.complex?.0 as u16)
    }

    /// Комплекс для экономических проверок QA: выделенного юнита, а если у него
    /// комплекса нет (или ничего не выделено) — первый комплекс команды игрока
    /// (наименьший id, как `TeamUnits::complexes[0]` в C++-списке).
    pub fn economy_complex(&self) -> Option<u16> {
        if let Some(complex) = self.selected_complex() {
            return Some(complex);
        }

        self.state
            .team_units
            .get(self.state.player_team.0 as usize)
            .and_then(|units| units.complexes().first())
            .map(|complex| complex.id as u16)
    }

    /// Открытый диалог распределения для ввода/QA.
    pub fn alloc_menu_mut(&mut self) -> Option<&mut allocmenu::AllocMenu> {
        self.alloc_menu.as_mut()
    }

    /// Открывает диалог для выделенной шахты — C++ `AllocMenu_Menu(unit)`.
    ///
    /// Пункт `allocate` меню команд (`UnitsManager_Popup_OnClick_Allocate`)
    /// вызывает это. `false` — юнит не шахта, не своя команда, выключена или
    /// без комплекса.
    pub fn open_alloc_menu(&mut self) -> bool {
        let Some(id) = self.hud.selected else {
            return false;
        };
        let Some(unit) = self.state.unit(id) else {
            return false;
        };
        if unit.team != self.state.player_team {
            return false;
        }

        let Some(menu) = allocmenu::AllocMenu::new(&mut self.state, id) else {
            return false;
        };

        self.unit_menu = None;
        self.alloc_menu = Some(menu);
        true
    }

    /// Закрывает диалог (`AllocMenu::Run` → `GameManager_EnableMainMenu`).
    ///
    /// Значения уже применены кликами (как в C++): `Done`/`Esc` только
    /// закрывают окно. Возвращает строку журнала.
    pub fn close_alloc_menu(&mut self) -> Option<String> {
        let menu = self.alloc_menu.take()?;
        let [raw, fuel, gold] = allocmenu::totals(&menu);

        Some(format!("allocate: raw {raw}, fuel {fuel}, gold {gold}"))
    }

    /// Ввод в открытом диалоге — [`allocmenu::AllocMenu::input`].
    pub fn alloc_menu_input(&mut self, event: InputEvent) -> Option<allocmenu::AllocAction> {
        let Some(mut menu) = self.alloc_menu.take() else {
            return None;
        };
        let action = menu.input(&mut self.state, event);
        self.alloc_menu = Some(menu);
        Some(action)
    }

    /// `allocate set N`/QA: уровень добычи ресурса в комплексе.
    pub fn alloc_menu_set(&mut self, kind: max_core::alloc::MiningKind, target: i32) -> bool {
        let Some(mut menu) = self.alloc_menu.take() else {
            return false;
        };
        menu.set_level(&mut self.state, kind, target);
        self.alloc_menu = Some(menu);
        true
    }

    /// `allocate row …`/QA: `+1`/`−1` единица ресурса.
    pub fn alloc_menu_adjust(&mut self, kind: max_core::alloc::MiningKind, increase: bool) -> bool {
        let Some(mut menu) = self.alloc_menu.take() else {
            return false;
        };
        let changed = if increase {
            menu.increase(&mut self.state, kind)
        } else {
            menu.decrease(&mut self.state, kind)
        };
        self.alloc_menu = Some(menu);
        changed
    }

    /// Команды `ORDER_NEW_ALLOCATE` (пакет 08, **P5**) для всех шахт комплекса —
    /// `Remote_SendNetPacket_11` (`remote.cpp:2195`) в конце `AllocMenu::Run`.
    ///
    /// C++ отсылает их только в сети; здесь приказы всегда кладутся в
    /// [`hud::HudState::pending`] — офлайн их применяет
    /// [`App::flush_orders`] (поля уже выставлены диалогом, приказ лишь
    /// транзиентный `ORDER_NEW_ALLOCATE`, как в C++), в сети они уходят
    /// пакетом 08/P5 и доносят добычу до остальных клиентов. Возвращает число
    /// приказов.
    ///
    /// Вызывать **до** [`Self::close_alloc_menu`] — комплекс берётся из
    /// открытого диалога.
    pub fn queue_alloc_orders(&mut self) -> usize {
        let Some(menu) = self.alloc_menu.as_ref() else {
            return 0;
        };
        let complex_id = menu.complex.0;
        let team = menu.team;
        let ids = self
            .state
            .unit_list(max_core::unit::UnitListKind::Stationary)
            .to_vec();
        let mut packets = Vec::new();

        for id in ids {
            let Some(unit) = self.state.unit(id) else {
                continue;
            };
            if unit.team != team
                || unit.complex.map(|complex| complex.0) != Some(complex_id)
                || !max_core::alloc::is_active_mine(unit)
            {
                continue;
            }
            // `UnitsManager_SetNewOrderInt` не трогает `prior_order`, если
            // текущий приказ уже `ORDER_NEW_ALLOCATE`.
            let (prior_order, prior_state) =
                if unit.orders == max_core::enums::UnitOrder::NewAllocate {
                    (unit.prior_orders, unit.prior_state)
                } else {
                    (unit.orders, unit.state)
                };
            let fire_on = self.state.order_data(id).fire_on;

            packets.push(max_core::input::Command::Order(
                max_core::input::Packet08Order {
                    unit: id,
                    payload: max_core::input::OrderPayload::P5(max_core::input::PayloadP5 {
                        p3: max_core::input::PayloadP3 {
                            p2: max_core::input::PayloadP2 {
                                p1: max_core::input::PayloadP1 {
                                    order: max_core::enums::UnitOrder::NewAllocate,
                                    state: max_core::enums::UnitOrderState::Init,
                                    prior_order,
                                    prior_state,
                                    disabled_reaction_fire: unit.disabled_reaction_fire,
                                },
                                parent_unit: unit.parent_unit,
                                experience: unit.experience,
                            },
                            move_to: unit.target_grid,
                            fire_on,
                            enemy_unit: None,
                            transfer_cargo: unit.transfer_cargo,
                            stealth_dice_roll: unit.stealth_dice_roll,
                        },
                        total_mining: unit.total_mining,
                        raw_mining: unit.raw_mining,
                        fuel_mining: unit.fuel_mining,
                        gold_mining: unit.gold_mining,
                    }),
                },
            ));
        }

        let count = packets.len();
        self.hud.pending.extend(packets);
        count
    }

    // -----------------------------------------------------------------------
    // X-FER (C++ `TransferMenu`, окно `XFERPIC`)
    // -----------------------------------------------------------------------

    /// Открытое окно X-FER; `None` — закрыто.
    pub fn transfer_menu(&self) -> Option<&transfermenu::TransferMenu> {
        self.transfer_menu.as_ref()
    }

    /// Окно X-FER для ввода/QA.
    pub fn transfer_menu_mut(&mut self) -> Option<&mut transfermenu::TransferMenu> {
        self.transfer_menu.as_mut()
    }

    /// Открывает окно X-FER между `source` и `target` —
    /// `GameManager_TransferCargo` (`game_manager.cpp:4634`).
    ///
    /// C++ сначала делает `unit1->SetParent(unit2)`; это поле едет в пакете
    /// 08/P3, поэтому выставляется сразу (как и в оригинале — отмена диалога
    /// родителя не восстанавливает). `false` — окно не открылось (нет
    /// комплекса/типа груза).
    pub fn open_transfer_menu(&mut self, source: UnitId, target: UnitId) -> bool {
        let Some(menu) = transfermenu::TransferMenu::new(&self.state, source, target) else {
            return false;
        };

        if let Some(unit) = self.state.unit_mut(source) {
            unit.parent_unit = Some(target);
        }

        self.unit_menu = None;
        self.transfer_menu = Some(menu);
        true
    }

    /// Закрывает окно X-FER без приказа (C++ `Esc`/`Cancel`).
    pub fn cancel_transfer(&mut self) -> Option<String> {
        let menu = self.transfer_menu.take()?;
        // `TransferMenu::~TransferMenu` → `GameManager_EnableMainMenu(&*SelectedUnit)`
        // → `GameManager_MenuUnitSelect` сбрасывает `cursor`/`enter_mode`.
        self.hud.cursor_mode = 0;
        self.hud.enter_mode = false;

        Some(format!(
            "x-fer: отменено (было {})",
            transfermenu::amount_to_transfer(&menu)
        ))
    }

    /// `Done`/`Enter` — `unit1->transfer_cargo = cargo_transferred;
    /// ORDER_TRANSFER` (пакет 08/**P3**).
    ///
    /// Нулевое количество, как в C++, приказа не даёт (меню просто
    /// закрывается). Возвращает строку журнала и признак «приказ поставлен в
    /// [`hud::HudState::pending`]».
    pub fn confirm_transfer(&mut self) -> Option<(String, bool)> {
        let menu = self.transfer_menu.take()?;
        // Как и в `~TransferMenu`: `EnableMainMenu` → `MenuUnitSelect` сбрасывает
        // курсорный режим выделенного юнита.
        self.hud.cursor_mode = 0;
        self.hud.enter_mode = false;
        let amount = transfermenu::amount_to_transfer(&menu);

        if amount == 0 {
            return Some(("x-fer: 0 — приказ не отдан".to_string(), false));
        }

        // `unit1->transfer_cargo = cargo_transferred` до `SendNetPacket_08`.
        if let Some(unit) = self.state.unit_mut(menu.unit) {
            unit.transfer_cargo = amount;
            unit.parent_unit = Some(menu.target);
        }

        let Some(command) =
            transfermenu::transfer_order(self.state(), menu.unit, menu.target, amount)
        else {
            return Some(("x-fer: юнит исчез".to_string(), false));
        };

        self.hud.pending.push(command);
        // `GameManager_TransferCargo` (`game_manager.cpp:4675`): голос `V_M224`
        // после успешной передачи.
        self.play_sfx(sfx::Sfx::Transfer);

        Some((
            format!("x-fer: {amount} ({:?} → {:?})", menu.cargo, menu.target),
            true,
        ))
    }

    /// `transfer amount N`/QA: значение полосы с обрезкой (как клик по полосе).
    pub fn transfer_set_amount(&mut self, amount: i32) -> bool {
        let Some(menu) = self.transfer_menu.as_mut() else {
            return false;
        };
        menu.set_amount(amount);
        true
    }

    /// `transfer right|left`/QA: один шаг полосы.
    pub fn transfer_adjust(&mut self, increase: bool) -> bool {
        let Some(menu) = self.transfer_menu.as_mut() else {
            return false;
        };
        menu.adjust(increase)
    }

    /// Ввод в открытом окне X-FER — [`transfermenu::TransferMenu::input`].
    pub fn transfer_menu_input(
        &mut self,
        event: InputEvent,
    ) -> Option<transfermenu::TransferAction> {
        let Some(mut menu) = self.transfer_menu.take() else {
            return None;
        };
        let action = menu.input(event);
        self.transfer_menu = Some(menu);
        Some(action)
    }

    /// Активирует пункт меню — C++ `GameManager_PopupButtons.r_func[i]`.
    ///
    /// Меню закрывается (как `GameManager_DeinitPopupButtons(true)` в каждом
    /// обработчике C++). Приказ, если пункт его выдаёт, кладётся в
    /// [`hud::HudState::pending`] — дальше [`App::flush_orders`] (офлайн) или
    /// сетевой слой. Возвращает строку журнала.
    ///
    /// `None` — пункта нет/он отключён (механика не портирована, см.
    /// [`unitmenu::UnitMenuItem::note`]).
    pub fn activate_unit_menu_item(&mut self, index: usize) -> Option<String> {
        let menu = self.unit_menu.take()?;
        let item = menu.items.get(index)?.clone();
        let unit_id = menu.unit;

        if !item.enabled {
            // Отключённый пункт остаётся в меню (как в C++ кнопка без обработчика).
            self.unit_menu = Some(menu);
            return Some(unitmenu::disabled_line(&item));
        }

        let line = match item.id {
            unitmenu::ItemId::Build => {
                if self.open_build_menu() {
                    format!("{} — BuildMenu", item.caption)
                } else {
                    format!("{}: юнит не умеет строить", item.caption)
                }
            }
            unitmenu::ItemId::BuyUpgrade => {
                if self.open_upgrade_menu() {
                    "buy upg — UpgradeMenu".to_string()
                } else {
                    "buy upg: экран апгрейдов недоступен".to_string()
                }
            }
            unitmenu::ItemId::Research => {
                if self.open_research_menu() {
                    "resrch — ResearchMenu".to_string()
                } else {
                    "resrch: экран исследований недоступен".to_string()
                }
            }
            unitmenu::ItemId::PowerOn | unitmenu::ItemId::PowerOff => {
                match crate::hud::build_power_order(self.state(), unit_id) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("{} — {}", item.caption, "Power toggled")
                    }
                    None => format!("{}: питание недоступно", item.caption),
                }
            }
            unitmenu::ItemId::Sentry => {
                let sentry = self
                    .state
                    .unit(unit_id)
                    .is_some_and(|unit| unit.orders == max_core::enums::UnitOrder::Sentry);

                let (order, order_state) = if sentry {
                    (
                        max_core::enums::UnitOrder::Await,
                        max_core::enums::UnitOrderState::ExecutingOrder,
                    )
                } else {
                    (
                        max_core::enums::UnitOrder::Sentry,
                        max_core::enums::UnitOrderState::ExecutingOrder,
                    )
                };

                match unitmenu::p1_order(self.state(), unit_id, order, order_state) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        if sentry {
                            "sentry off — Await".to_string()
                        } else {
                            "sentry — ORDER_SENTRY".to_string()
                        }
                    }
                    None => "sentry: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::Stop => {
                let move_to = self
                    .state
                    .unit(unit_id)
                    .map(|unit| unit.target_grid)
                    .unwrap_or(Point::ZERO);

                match unitmenu::p3_order(
                    self.state(),
                    unit_id,
                    max_core::enums::UnitOrder::Await,
                    max_core::enums::UnitOrderState::ClearPath,
                    move_to,
                    None,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        "stop — ORDER_AWAIT/CLEAR_PATH".to_string()
                    }
                    None => "stop: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::Upgrade => {
                match unitmenu::p2_order(
                    self.state(),
                    unit_id,
                    max_core::enums::UnitOrder::Upgrade,
                    max_core::enums::UnitOrderState::Init,
                    Some(unit_id),
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        "upgrade — ORDER_UPGRADE".to_string()
                    }
                    None => "upgrade: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::Done => {
                // `UnitsManager_PerformAction` в объёме, который есть в нашем
                // стеке: `Await` + `STATIONARY` → `READY_TO_EXECUTE_ORDER`
                // (приказом, чтобы состояние ушло и в сеть), затем выделение
                // снимается (`auto_select` в C++).
                let ready = self.state.unit(unit_id).is_some_and(|unit| {
                    unit.orders == max_core::enums::UnitOrder::Await
                        && unit.flags & max_core::enums::unit_flags::STATIONARY != 0
                        && unit.state != max_core::enums::UnitOrderState::ReadyToExecuteOrder
                });

                if ready {
                    if let Some(command) = unitmenu::p3_order(
                        self.state(),
                        unit_id,
                        max_core::enums::UnitOrder::Await,
                        max_core::enums::UnitOrderState::ReadyToExecuteOrder,
                        self.state
                            .unit(unit_id)
                            .map(|unit| unit.target_grid)
                            .unwrap_or(Point::ZERO),
                        None,
                    ) {
                        self.hud.pending.push(command);
                    }
                }

                self.hud.clear_interaction();
                "Done".to_string()
            }
            unitmenu::ItemId::Manual => {
                let unit = self.state.unit(unit_id).cloned()?;
                let order = unit.orders;
                let order_state = if order == max_core::enums::UnitOrder::Sentry {
                    max_core::enums::UnitOrderState::ExecutingOrder
                } else {
                    unit.state
                };

                // P1 несёт `disabled_reaction_fire` — тем же путём, что сеть.
                let command = unitmenu::p1_order(self.state(), unit_id, order, order_state)?;
                if let Command::Order(packet) = command {
                    let mut packet = packet;
                    if let max_core::input::OrderPayload::P1(p1) = &mut packet.payload {
                        p1.disabled_reaction_fire = !unit.disabled_reaction_fire;
                    }
                    self.hud.pending.push(Command::Order(packet));
                }

                if unit.disabled_reaction_fire {
                    "Manual: ответный огонь включён".to_string()
                } else {
                    "Manual: ответный огонь выключен".to_string()
                }
            }
            unitmenu::ItemId::Auto => {
                let Some(unit) = self.state.unit_mut(unit_id) else {
                    return Some("Auto: юнит исчез".to_string());
                };
                unit.auto_survey = !unit.auto_survey;
                let enabled = unit.auto_survey;

                if enabled {
                    "Auto: авто-разведка включена".to_string()
                } else {
                    "Auto: авто-разведка выключена".to_string()
                }
            }
            unitmenu::ItemId::Enter => {
                // C++ `UnitsManager_Popup_OnClick_Enter`: тумблер `enter_mode`.
                self.hud.enter_mode = !self.hud.enter_mode;
                if self.hud.enter_mode {
                    "enter: клик по своему транспорту — посадка".to_string()
                } else {
                    "enter: выключено".to_string()
                }
            }
            unitmenu::ItemId::Allocate => {
                // C++ `UnitsManager_Popup_OnClick_Allocate` → `AllocMenu_Menu(unit)`.
                if self.open_alloc_menu() {
                    "allocate — Allocation Menu".to_string()
                } else {
                    "allocate: нужна включённая шахта с комплексом".to_string()
                }
            }
            unitmenu::ItemId::Transfer
            | unitmenu::ItemId::Reload
            | unitmenu::ItemId::Repair
            | unitmenu::ItemId::Load
            | unitmenu::ItemId::Disable
            | unitmenu::ItemId::Steal => {
                // Курсорные режимы C++ (`unit->cursor = 3/6/4/7/9/8`).
                let (cursor, label) = match item.id {
                    unitmenu::ItemId::Transfer => (unitmenu::CURSOR_TRANSFER, "x-fer"),
                    unitmenu::ItemId::Reload => (unitmenu::CURSOR_RELOAD, "reload"),
                    unitmenu::ItemId::Repair => (unitmenu::CURSOR_REPAIR, "repair"),
                    unitmenu::ItemId::Disable => (unitmenu::CURSOR_DISABLE, "disable"),
                    unitmenu::ItemId::Steal => (unitmenu::CURSOR_STEAL, "steal"),
                    _ => (unitmenu::CURSOR_LOAD, "load"),
                };

                self.hud.cursor_mode = if self.hud.cursor_mode == cursor {
                    0
                } else {
                    cursor
                };

                if self.hud.cursor_mode == cursor {
                    format!("{label}: выберите цель кликом")
                } else {
                    format!("{label}: режим выключен")
                }
            }
            unitmenu::ItemId::Attack => {
                // `UnitsManager_Popup_OnClick_TargetingMode`
                // (`units_manager.cpp:617`): тумблер `targeting_mode`, курсор и
                // `enter_mode` сбрасываются.
                self.hud.targeting_mode = !self.hud.targeting_mode;
                self.hud.cursor_mode = 0;
                self.hud.enter_mode = false;

                if self.hud.targeting_mode {
                    "attack: наведение включено — клик по врагу или клетке в радиусе".to_string()
                } else {
                    "attack: наведение выключено".to_string()
                }
            }
            unitmenu::ItemId::RemoveStationary => {
                // `UnitsManager_Popup_OnClick_Remove` → `UnitsManager_SelfDestructMenu`.
                if self.open_self_destruct(unit_id) {
                    format!("{}: подтвердите уничтожение здания", item.caption)
                } else {
                    format!("{}: здание сейчас нельзя уничтожить", item.caption)
                }
            }
            unitmenu::ItemId::RemoveRubble => {
                // `UnitsManager_Popup_OnClick_StopRemove` (`units_manager.cpp:1082`):
                // `Access_GetRemovableRubble` → `parent = обломки`,
                // `build_time` 4/1 и `ORDER_CLEAR`/`INIT`.
                let Some(unit) = self.state.unit(unit_id).cloned() else {
                    return Some("remove: юнит исчез".to_string());
                };
                let team = unit.team;
                let grid = unit.grid;

                let Some(rubble) = unitmenu::removable_rubble(self.state(), team, grid) else {
                    return Some("Unable to clear at current location.".to_string());
                };

                let rubble_is_building = self
                    .state
                    .unit(rubble)
                    .is_some_and(|unit| unit.flags & max_core::enums::unit_flags::BUILDING != 0);
                let clearing_time = if rubble_is_building { 4 } else { 1 };

                if let Some(unit) = self.state.unit_mut(unit_id) {
                    unit.parent_unit = Some(rubble);
                    unit.build_time = clearing_time;
                }

                match unitmenu::clear_order(self.state(), unit_id, rubble) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("Number of turns to clear site: {clearing_time}.")
                    }
                    None => "remove: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::StopRemove => {
                // `GameManager_SelectBuildSite` (`game_manager.cpp:3549-3552`):
                // `move_to = своя клетка` + `ORDER_BUILD`/`BUILD_CANCEL`;
                // `CancelBuilding` не-стационарного юнита возвращает `AWAIT`.
                match unitmenu::p1_order(
                    self.state(),
                    unit_id,
                    max_core::enums::UnitOrder::Build,
                    max_core::enums::UnitOrderState::BuildCancel,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        "stop: расчистка отменена".to_string()
                    }
                    None => "stop: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::UpgradeAll => {
                // `UnitsManager_Popup_OnClick_UpgradeAll` (`units_manager.cpp:414`).
                match unitmenu::upgrade_all_orders(self.state(), unit_id) {
                    Some(report) => {
                        self.hud.pending.extend(report.commands);
                        report.message
                    }
                    None => "Upg. All: юнит исчез".to_string(),
                }
            }
            unitmenu::ItemId::StartBuild => {
                // C++ `UnitsManager_Popup_OnClick_BuildOrder` →
                // `UnitInfo::BuildOrder`: очередь и `build_time` остались от
                // `CancelBuilding` (завод), поэтому достаточно `ORDER_BUILD` +
                // `ORDER_STATE_INIT` — `start_building` возобновит постройку.
                match unitmenu::p1_order(
                    self.state(),
                    unit_id,
                    max_core::enums::UnitOrder::Build,
                    max_core::enums::UnitOrderState::Init,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("{} — ORDER_BUILD/INIT (продолжение)", item.caption)
                    }
                    None => format!("{}: юнит исчез", item.caption),
                }
            }
            unitmenu::ItemId::StopBuild => {
                // C++ `GameManager_SelectBuildSite` (`game_manager.cpp:3529`):
                // `ORDER_BUILD` + `ORDER_STATE_BUILD_CANCEL` — `CancelBuilding`
                // у мобильного строителя очищает очередь и возвращает
                // `ORDER_AWAIT`, у завода — `HaltBuilding` (стройку можно
                // продолжить пунктом `start`).
                match unitmenu::p1_order(
                    self.state(),
                    unit_id,
                    max_core::enums::UnitOrder::Build,
                    max_core::enums::UnitOrderState::BuildCancel,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("{} — ORDER_BUILD/BUILD_CANCEL", item.caption)
                    }
                    None => format!("{}: юнит исчез", item.caption),
                }
            }
            unitmenu::ItemId::PlaceMine | unitmenu::ItemId::RemoveMine => {
                let state_order = if item.id == unitmenu::ItemId::PlaceMine {
                    max_core::enums::UnitOrderState::PlacingMines
                } else {
                    max_core::enums::UnitOrderState::RemovingMines
                };

                match unitmenu::p1_order(
                    self.state(),
                    unit_id,
                    max_core::enums::UnitOrder::LayMine,
                    state_order,
                ) {
                    Some(command) => {
                        self.hud.pending.push(command);
                        format!("{} — ORDER_LAY_MINE", item.caption)
                    }
                    None => format!("{}: юнит исчез", item.caption),
                }
            }
            // Остальные пункты отключены при построении списка.
            _ => unitmenu::disabled_line(&item),
        };

        Some(line)
    }

    // -----------------------------------------------------------------------
    // Окно помощи (`HelpMenu`, P7)
    // -----------------------------------------------------------------------

    /// Открытое окно помощи поверх игры.
    pub fn help_screen(&self) -> Option<&helpmenu::HelpScreen> {
        self.help.as_ref()
    }

    /// Открытое окно помощи для ввода.
    pub fn help_screen_mut(&mut self) -> Option<&mut helpmenu::HelpScreen> {
        self.help.as_mut()
    }

    /// Включает режим помощи — C++ `HelpMenu_Menu(section, WINDOW_MAIN_MAP)`
    /// (`helpmenu.cpp:315`): курсор `CURSOR_HELP`, окно откроется кликом по
    /// элементу с записью `help.json` под точкой.
    pub fn open_help(&mut self, section: helpmenu::HelpSection) -> bool {
        self.open_help_with(section, helpmenu::HelpPlacement::Game)
    }

    /// Режим помощи с явной позицией окна.
    pub fn open_help_with(
        &mut self,
        section: helpmenu::HelpSection,
        placement: helpmenu::HelpPlacement,
    ) -> bool {
        self.help = None;
        self.help_mode = Some(helpmenu::HelpMode::new(section, placement));
        true
    }

    /// Открывает окно помощи сразу на записи `name` (QA/тесты; в C++ окно
    /// открывается только кликом по элементу).
    pub fn open_help_entry(
        &mut self,
        section: helpmenu::HelpSection,
        placement: helpmenu::HelpPlacement,
        name: &str,
    ) -> bool {
        let Some(index) = helpmenu::entry_by_name(section, name) else {
            return false;
        };

        self.help = Some(helpmenu::HelpScreen::for_entry(section, placement, index));
        true
    }

    /// Окно для первой записи секции (без режима; страховка для тестов).
    pub fn open_help_first(
        &mut self,
        section: helpmenu::HelpSection,
        placement: helpmenu::HelpPlacement,
    ) -> bool {
        self.help = Some(helpmenu::HelpScreen::new(section, placement));
        true
    }

    /// Активный режим помощи.
    pub fn help_mode(&self) -> Option<helpmenu::HelpMode> {
        self.help_mode
    }

    /// Клик в режиме помощи: `true` — запись найдена и окно открыто.
    ///
    /// C++ `HelpMenu_Menu`: клик по пустому месту окно не открывает и режим не
    /// выключает; `Esc`/правая кнопка/`Shift-/` выключают режим.
    pub fn help_mode_click(&mut self, x: i32, y: i32) -> bool {
        let Some(mode) = self.help_mode else {
            return false;
        };
        let Some(screen) = mode.click(x, y) else {
            return false;
        };

        self.help = Some(screen);
        true
    }

    /// Выход из режима помощи (`Esc`/RMB/`Shift-/`).
    pub fn exit_help_mode(&mut self) {
        self.help = None;
        self.help_mode = None;
    }

    /// Закрывает окно помощи (как `Done`/`Enter`/`Esc`); режим остаётся
    /// активным — C++ `HelpMenu::Run` возвращает управление в `HelpMenu_Menu`.
    pub fn close_help(&mut self) {
        self.help = None;
    }

    /// Ввод в окне помощи; возвращает действие (закрыть/нет).
    pub fn help_input(&mut self, event: InputEvent) -> Option<helpmenu::HelpAction> {
        let Some(mut help) = self.help.take() else {
            return None;
        };
        let action = help.input(event);
        self.help = Some(help);
        Some(action)
    }

    // -----------------------------------------------------------------------
    // Самоуничтожение здания (C++ `UnitsManager_SelfDestructMenu`)
    // -----------------------------------------------------------------------

    /// Открытый диалог самоуничтожения; `None` — закрыт.
    pub fn self_destruct(&self) -> Option<&selfdestruct::SelfDestructMenu> {
        self.self_destruct.as_ref()
    }

    /// Открывает диалог для здания — `UnitsManager_Popup_OnClick_Remove`
    /// (`units_manager.cpp:506`): только для стационарного своего юнита.
    pub fn open_self_destruct(&mut self, unit_id: UnitId) -> bool {
        let Some(unit) = self.state.unit(unit_id) else {
            return false;
        };

        if unit.team != self.state.player_team
            || unit.flags & max_core::enums::unit_flags::STATIONARY == 0
        {
            return false;
        }

        self.unit_menu = None;
        self.self_destruct = Some(selfdestruct::SelfDestructMenu::new(unit_id));
        true
    }

    /// Ввод в диалоге (`SelfDestructMenu`).
    ///
    /// `Explode` — `ORDER_EXPLODE`/`ORDER_STATE_EXPLODE` в очередь приказов,
    /// как `UnitsManager_SetNewOrder(unit, ORDER_EXPLODE, ORDER_STATE_EXPLODE)`.
    pub fn self_destruct_input(
        &mut self,
        event: InputEvent,
    ) -> Option<selfdestruct::SelfDestructAction> {
        let mut menu = self.self_destruct.take()?;
        let action = menu.input(event);

        match action {
            selfdestruct::SelfDestructAction::None => {
                self.self_destruct = Some(menu);
            }
            selfdestruct::SelfDestructAction::Cancel => {}
            selfdestruct::SelfDestructAction::Explode => {
                if let Some(command) = unitmenu::explode_order(&self.state, menu.unit) {
                    self.hud.pending.push(command);
                }
            }
        }

        Some(action)
    }

    /// Продвигает кадры `SLFDOPN1..6` по визуальным часам.
    pub fn advance_self_destruct(&mut self, dt: f32) {
        if let Some(menu) = self.self_destruct.as_mut() {
            menu.advance(dt);
        }
    }

    // -----------------------------------------------------------------------
    // Звуки UI (C++ `SoundManager::PlaySfx`)
    // -----------------------------------------------------------------------

    /// Ставит звук в очередь кадра (проигрывает `main.rs` через `AudioPlayer`).
    pub fn play_sfx(&mut self, sfx: sfx::Sfx) {
        self.sounds.push(sfx);
    }

    /// Звуки, накопленные с прошлого вызова.
    pub fn take_sounds(&mut self) -> Vec<sfx::Sfx> {
        std::mem::take(&mut self.sounds)
    }

    /// Текущая очередь звуков (для тестов/QA).
    pub fn sounds(&self) -> &[sfx::Sfx] {
        &self.sounds
    }

    // -----------------------------------------------------------------------
    // Пауза (`PauseMenu_Menu` → `DialogMenu`, `Alt+P`)
    // -----------------------------------------------------------------------

    /// Открыт ли диалог паузы.
    pub fn pause_dialog(&self) -> Option<&dialog::DialogMenu> {
        self.pause_dialog.as_ref()
    }

    /// Открывает паузу — `PauseMenu_Menu` (`menu.cpp:1747`): только в партии,
    /// тики и таймер хода останавливаются (C++ корректирует
    /// `menu_turn_timer_value` на время диалога).
    pub fn open_pause_dialog(&mut self) -> bool {
        if self.local.is_none() || self.pause_dialog.is_some() {
            return false;
        }

        self.pause_was_paused = self.local.as_ref().is_some_and(|local| local.paused);

        if let Some(local) = self.local.as_mut() {
            local.paused = true;
        }

        self.pause_dialog = Some(dialog::DialogMenu::pause());
        self.play_sfx(sfx::Sfx::Button);
        true
    }

    /// Ввод в диалоге паузы; `OK`/`Enter`/`Esc` закрывают его.
    pub fn pause_dialog_input(&mut self, event: InputEvent) -> Option<dialog::DialogAction> {
        let mut dialog = self.pause_dialog.take()?;
        let action = dialog.input(event);

        match action {
            dialog::DialogAction::None => {
                self.pause_dialog = Some(dialog);
            }
            dialog::DialogAction::Close => {
                if let Some(local) = self.local.as_mut() {
                    local.paused = self.pause_was_paused;
                }
                self.play_sfx(if matches!(event, InputEvent::Key(ui::Key::Escape)) {
                    sfx::Sfx::Cancel
                } else {
                    sfx::Sfx::Done
                });
            }
        }

        Some(action)
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

    // -----------------------------------------------------------------------
    // Скролл камеры (Preferences: click_scroll / quick_scroll)
    // -----------------------------------------------------------------------

    /// Запоминает позицию мыши для краевого скролла (`MouseMove`).
    pub fn note_mouse_move(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 {
            self.scroll.clear_pointer();
        } else {
            self.scroll.set_pointer(x, y);
        }
    }

    /// Клик ЛКМ: при `click_scroll` включает «удержание» для скролла.
    pub fn note_mouse_click(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 {
            self.scroll.press(x, y);
        }
    }

    /// Состояние скролла (указатель/удержание) — для тестов и живого UI.
    pub fn scroll_state(&self) -> &scroll::ScrollState {
        &self.scroll
    }

    /// Один кадр краевого скролла — порт `GameManager_ProcessInput` →
    /// `GameManager_SetGridOffset` → `GameManager_ScrollMainMapView`.
    ///
    /// Возвращает сдвиг камеры в экранных пикселях, если он был. Смещение
    /// применяется к [`App::viewport`] той же функцией
    /// [`Viewport::scroll_by_screen`], что и остальной ввод мыши, поэтому клики
    /// после скролла попадают в те же клетки.
    ///
    /// `click_scroll`: при `true` карта едет только пока «зажата» ЛКМ
    /// (`GameManager_SetGridOffset`, `game_manager.cpp:6352`); при `false` — по
    /// наведению. `quick_scroll` — «Scroll Speed» (`GridStepLevel`,
    /// `game_manager.cpp:1711`).
    pub fn update_scroll(&mut self, dt: f32) -> Option<(i32, i32)> {
        self.scroll.tick(dt);

        let (dx, dy) = self.scroll.step(
            dt,
            self.preferences.click_scroll,
            self.preferences.quick_scroll,
        )?;

        if dx == 0 && dy == 0 {
            return None;
        }

        let before = self.viewport.center();
        self.viewport.scroll_by_screen(dx, dy);

        // `GameManager_ScrollMainMapView` возвращает `false`, если границы
        // карты не изменились; у края карты шаг не считается прокруткой.
        (self.viewport.center() != before).then_some((dx, dy))
    }

    // -----------------------------------------------------------------------
    // Камера за выделенным юнитом (Preferences: follow_unit)
    // -----------------------------------------------------------------------

    /// Порт `UnitInfo::FollowUnit` (`unitinfo.cpp:3597`).
    ///
    /// При `follow_unit` камера центрируется на выделенном юните, как только он
    /// выходит за пределы окна карты. Условия видимости — дословно C++:
    /// turn-based, либо ход команды юнита, либо она уже закончила ход; юнит
    /// виден локальной команде.
    ///
    /// Возвращает `true`, если камера сдвинулась.
    pub fn follow_selected_unit(&mut self) -> bool {
        if !self.preferences.follow_unit {
            return false;
        }

        let Some(id) = self.hud.selected else {
            return false;
        };
        let player = self.state.player_team;
        let Some(unit) = self.state.unit(id) else {
            return false;
        };

        if !unit.is_visible_to_team(player) {
            return false;
        }

        let finished_turn = self.state.teams[player.0 as usize].finished_turn;
        let allowed = self.state.play_mode == max_core::enums::PlayMode::TurnBased
            || unit.team == player
            || finished_turn;

        if !allowed {
            return false;
        }

        let grid = unit.grid;

        if self.map_viewport().is_tile_visible(grid.x, grid.y) {
            return false;
        }

        self.viewport.center_on_grid(grid.x, grid.y);

        true
    }

    // -----------------------------------------------------------------------
    // Авто-выделение следующего юнита (Preferences: auto_select)
    // -----------------------------------------------------------------------

    /// Снимок приказа выделенного юнита перед тиком — чтобы отличить
    /// «приказ только что завершился» от «юнит стоит и ждёт».
    fn selected_order_snapshot(&self) -> Option<OrderSnapshot> {
        let id = self.hud.selected?;
        let unit = self.state.unit(id)?;

        Some(OrderSnapshot {
            id,
            orders: unit.orders,
            state: unit.state,
            has_path: unit.path.is_some(),
            speed: unit.speed,
        })
    }

    /// Реагирует на завершение приказа выделенного юнита, как
    /// `UnitsManager_PerformAction`/`UnitInfo::MoveFinished`
    /// (`unitinfo.cpp:3654` → `GameManager_AutoSelectNext`, `game_manager.cpp:1779`).
    ///
    /// * приказ движения (`ORDER_MOVE`/`MOVE_TO_UNIT`/`MOVE_TO_ATTACK`) был в
    ///   работе и завершился (`AWAIT` без пути) или юнит уничтожен;
    /// * если `auto_select` выключен или юнит ещё «интерактивен»
    ///   (`GameManager_IsInteractable`) — выделение остаётся;
    /// * иначе выделяется следующий юнит команды (`GameManager_SelectNextUnit`).
    pub fn auto_select_after_tick(&mut self, before: Option<OrderSnapshot>) {
        if !self.preferences.auto_select {
            return;
        }

        let Some(before) = before else {
            return;
        };

        // Игрок успел выделить другого юнита — не перебиваем.
        if self.hud.selected != Some(before.id) {
            return;
        }

        let moved_order = matches!(
            before.orders,
            UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
        );

        if !moved_order {
            return;
        }

        let was_moving = before.has_path
            || matches!(
                before.state,
                UnitOrderState::InProgress
                    | UnitOrderState::InTransition
                    | UnitOrderState::Init
                    | UnitOrderState::MoveInit
                    | UnitOrderState::IssuingPath
                    | UnitOrderState::ExecutingOrder
            );

        if !was_moving {
            return;
        }

        let finished = match self.state.unit(before.id) {
            // Юнит уничтожен на этом тике (`CheckIfDestroyed` в C++ зовёт
            // `GameManager_AutoSelectNext` для выделенного).
            None => true,
            Some(unit) => {
                unit.hits == 0
                    || (matches!(unit.orders, UnitOrder::Await | UnitOrder::Idle)
                        && unit.path.is_none())
            }
        };

        if !finished {
            return;
        }

        if let Some(unit) = self.state.unit(before.id) {
            if unit.hits > 0 && self.is_interactable(unit) {
                return;
            }
        }

        self.select_next_unit();
    }

    /// После `BuildMenu` (`UnitsManager_Popup_OnClick_Build` →
    /// `GameManager_AutoSelectNext`) — авто-выделение следующего юнита, если
    /// текущий больше не интерактивен.
    pub fn auto_select_after_order(&mut self) {
        if !self.preferences.auto_select {
            return;
        }

        let Some(id) = self.hud.selected else {
            return;
        };

        if let Some(unit) = self.state.unit(id) {
            if unit.hits > 0 && self.is_interactable(unit) {
                return;
            }
        }

        self.select_next_unit();
    }

    /// `GameManager_IsInteractable` (`game_manager.cpp:3230`): юнит ещё может
    /// действовать, поэтому авто-выделение его не пропускает.
    ///
    /// Точный порт `Access_FindReachableSpot` в max_core отсутствует; «есть
    /// куда стрелять» оценивается как `shots > 0` и враг в радиусе `RANGE`
    /// (квадрат расстояния, как в [`max_core::access::is_within_attack_range`]).
    fn is_interactable(&self, unit: &max_core::unit::UnitInfo) -> bool {
        if matches!(
            unit.orders,
            UnitOrder::Sentry
                | UnitOrder::Idle
                | UnitOrder::Disable
                | UnitOrder::Explode
                | UnitOrder::AwaitScaling
        ) || matches!(
            unit.state,
            UnitOrderState::Destroy | UnitOrderState::ReadyToExecuteOrder
        ) {
            return false;
        }

        if matches!(unit.orders, UnitOrder::Build | UnitOrder::Clear) {
            return unit.state == UnitOrderState::SelectSite;
        }

        if unit.speed > 0 {
            return true;
        }

        if unit.shots > 0 {
            let range =
                i32::from(unit.base_values.get_attribute(max_core::unit_values::ATTRIB_RANGE));
            let squared = range * range;

            for id in self.state.unit_ids_in_order() {
                let Some(enemy) = self.state.unit(id) else {
                    continue;
                };

                if enemy.team == unit.team || enemy.hits == 0 || !enemy.is_visible_to_team(unit.team)
                {
                    continue;
                }

                if max_core::access::squared_distance(unit.grid, enemy.grid) <= squared {
                    return true;
                }
            }
        }

        false
    }

    /// `GameManager_SelectNextUnit(1)` (`game_manager.cpp:1544`): следующий
    /// валидный юнит команды в порядке `Access_UnitsLists`, с переносом в
    /// начало; камера центрируется, если он за пределами окна.
    pub fn select_next_unit(&mut self) -> Option<UnitId> {
        let player = self.state.player_team;
        let current = self.hud.selected;
        let next = self.next_valid_unit(player, current)?;

        self.close_unit_menu();
        self.select_unit(Some(next));

        let grid = self.state.unit(next).map(|unit| unit.grid);

        if let Some(grid) = grid {
            if !self.map_viewport().is_tile_visible(grid.x, grid.y) {
                self.viewport.center_on_grid(grid.x, grid.y);
            }
        }

        Some(next)
    }

    /// `Access_SeekNextUnit(team, unit, +1)` по пяти спискам в порядке
    /// `Access_UnitsLists` (mobile land/sea → air → stationary → ground cover →
    /// particles), затем `GameManager_GetFirstRelevantUnit`.
    fn next_valid_unit(&self, team: TeamId, current: Option<UnitId>) -> Option<UnitId> {
        const ORDER_: [max_core::unit::UnitListKind; 5] = [
            max_core::unit::UnitListKind::MobileLandSea,
            max_core::unit::UnitListKind::MobileAir,
            max_core::unit::UnitListKind::Stationary,
            max_core::unit::UnitListKind::GroundCover,
            max_core::unit::UnitListKind::Particle,
        ];

        // Плоский порядок обхода + позиция текущего юнита в нём.
        let mut flat: Vec<UnitId> = Vec::new();

        for kind in ORDER_ {
            flat.extend_from_slice(self.state.unit_list(kind));
        }

        let start = current
            .and_then(|id| flat.iter().position(|candidate| *candidate == id))
            .map(|index| index + 1)
            .unwrap_or(0);

        for step in 0..flat.len() {
            let id = flat[(start + step) % flat.len()];
            let Some(unit) = self.state.unit(id) else {
                continue;
            };

            // C++ `Access_SeekNextUnit` не возвращает сам текущий юнит.
            if Some(id) == current {
                continue;
            }

            if unit.team == team && Self::is_valid_next_unit(unit) {
                return Some(id);
            }
        }

        // `GameManager_GetFirstRelevantUnit`: шахта, иначе первый активный юнит.
        for kind in ORDER_ {
            for id in self.state.unit_list(kind) {
                let Some(unit) = self.state.unit(*id) else {
                    continue;
                };

                if unit.team == team && unit.hits > 0 && unit.unit_type == UnitType::MiningStation {
                    return Some(*id);
                }
            }
        }

        for kind in ORDER_ {
            for id in self.state.unit_list(kind) {
                let Some(unit) = self.state.unit(*id) else {
                    continue;
                };

                if unit.team == team && unit.hits > 0 && Self::is_valid_next_unit(unit) {
                    return Some(*id);
                }
            }
        }

        None
    }

    /// `Access_IsValidNextUnit` (`access.cpp:1377`).
    fn is_valid_next_unit(unit: &max_core::unit::UnitInfo) -> bool {
        if matches!(
            unit.orders,
            UnitOrder::Sentry | UnitOrder::Idle | UnitOrder::Disable | UnitOrder::Explode
        ) || matches!(
            unit.state,
            UnitOrderState::Destroy | UnitOrderState::ReadyToExecuteOrder
        ) || (unit.orders == UnitOrder::Build && unit.state != UnitOrderState::UnitReady)
        {
            return false;
        }

        if unit.speed > 0 || unit.shots > 0 || unit.storage != 0 {
            return true;
        }

        unit.orders != UnitOrder::Await
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
        // `SaveLoad_Load` оставляет `settings` игрока как есть: `enemy_halt`
        // берётся из `Preferences`, а не из файла (`saveload.cpp:1175` пишет в
        // настройки только таймеры).
        self.state.enemy_halt = self.preferences.enemy_halt;
        self.ticks = 0;
        self.applied_commands = 0;
        self.hud.clear();
        self.local = None;
        self.build_menu = None;
        self.save_load = None;
        self.upgrade_menu = None;
        self.research_menu = None;
        self.unit_menu = None;
        self.self_destruct = None;
        self.alloc_menu = None;
        self.transfer_menu = None;
        self.help = None;
        self.help_mode = None;
        self.reset_visual_state();
        self.focus_player();
        // Стартовый снимок: первый тик с `dt > 0` увидит, из какой клетки
        // юнит начал движение, иначе первый шаг маршрута «телепортировался».
        self.update_visual_units(0.0);

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

        // Фазы старта (`CargoMenu` + site select) уже отработали: люди выбрали
        // точки сами, компьютеры выбирают их как `Ai_SelectStartingPosition`
        // (`GameManager_InitGame`: цикл по командам-компьютерам после людей, но
        // до `GameManager_PopulateMapWithResources`).
        //
        // C++ пересеивает ГПСЧ в `GameManager_PopulateMapWithResources`, поэтому
        // выбор ИИ идёт отдельным потоком от того же seed, а генерация карты —
        // уже с чистого seed (как и раньше).
        let mut deployments = config.deployments.clone();

        if !deployments.is_empty() {
            let mut choice_rng = Randomizer::new(config.seed);
            let mut taken: Vec<Point> = deployments.iter().map(|entry| entry.position).collect();

            for start in setup.teams.iter_mut() {
                if deployments.iter().any(|entry| entry.team == start.team) {
                    continue;
                }

                let position =
                    max_core::setup::ai_choose_starting_position(surface, &mut choice_rng, &taken);

                start.position = position;
                taken.push(position);
                deployments.push(TeamDeployment::default_loadout(
                    start.team,
                    position,
                    config.start_gold,
                ));
            }
        }

        let output = generate_map(&mut setup, surface, &mut rng);
        // Снимок карты ресурсов **сразу после генерации**
        // (`GameManager_PopulateMapWithResources`, `game_manager.cpp:4730`):
        // `NewGameReport::resources_crc` сверяется с P2-вектором генерации и не
        // должен содержать командную разведку, которую накапливают высаженные
        // разведчики (`Survey_SurveyArea` → `CargoMap |= hash_team_id`).
        let generated_resources_crc = crc::crc16(&output.map.resources.to_le_bytes());

        // `GameManager_PopulateMapWithResources` (`game_manager.cpp:4730`) перед
        // высадкой двигает `TeamMissionSupplies[team].starting_position`
        // **каждой** играющей команды: `FindValidStartingPosition`, затем
        // `Optimize` материалов/топлива. Поэтому `ProcessTeamMissionSupplyUnits`
        // (и `GetStartPositionMiningStation`) видит уже итоговую точку — и для
        // ИИ, и для человека, выбравшего точку в фазе site select.
        //
        // Раньше синхронизировались только точки, добавленные здесь для ИИ;
        // интерактивный поток (`start_positions` из фазы высадки) уходил в
        // `deploy_starting_units_with` со своим исходным значением. Пока точка
        // валидна (её проверяет `LandingScreen`), результат совпадает, но при
        // любой будущей правке `Optimize` раскладка разъехалась бы с C++.
        for deployment in deployments.iter_mut() {
            if let Some(start) = setup
                .teams
                .iter()
                .find(|start| start.team == deployment.team)
            {
                deployment.position = start.position;
            }
        }

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
        // Живые настройки, которые читает симуляция (`settings["enemy_halt"]`).
        state.enemy_halt = self.preferences.enemy_halt;
        // Кланы из настроек — до высадки юнитов: `deploy_starting_units`
        // берёт атрибуты из `team_units` (`ResourceManager_InitClanUnitValues`).
        for index in 0..PLAYER_TEAM_COUNT {
            let clan = config.clans.get(index).copied().unwrap_or(TeamClan::Random);
            state.teams[index].team_clan = clan;
            let values = max_core::unit_values::team_base_values(clan);
            state.team_units[index].base_values = values;
            state.team_units[index].current_values = values;
        }

        // `CargoMenu::CommitUpgradeChanges`: купленные до высадки апгрейды
        // (`unitvalues_actual`) заменяют текущие значения команды до
        // `GameManager_ProcessTeamMissionSupplyUnits`, поэтому высаженные юниты
        // получают уже улучшенные атрибуты.
        for deployment in &deployments {
            let index = deployment.team.0 as usize;

            if index >= PLAYER_TEAM_COUNT {
                continue;
            }

            for (unit_type, values) in &deployment.upgrades {
                let slot = *unit_type as usize;
                if slot < state.team_units[index].current_values.len() {
                    state.team_units[index].current_values[slot] = *values;
                }
            }
            state.teams[index].stats_gold_spent_on_upgrades = deployment.gold_spent_on_upgrades;
        }
        // Поток ГПСЧ продолжается после генерации — как в оригинале.
        state.rng = rng;

        // `GameManager_SpawnAlienDerelicts`: все размещения — `DeployUnit` с
        // `team = PLAYER_TEAM_ALIEN`, в порядке генерации. C++ передаёт
        // `skip_map_status_update = true`; в max_core `update_map_status`
        // сам игнорирует `ALIEN` (у команды нет heat-map), поэтому
        // результат тот же — командные слои не трогаются.
        for placement in &output.aliens {
            let _ = state.spawn_unit(placement.unit_type, TeamId::ALIEN, placement.position, 0);
        }

        // `GameManager_ProcessTeamMissionSupplyUnits` для каждой команды
        // (`team_type != NONE`), в порядке возрастания индекса команды и уже
        // после alien-обломков — ровно как в `GameManager_InitGame`. Без этого
        // `menu_check_end_game_conditions` завершает партию сразу после первого
        // хода: у команд нет ни инфраструктуры, ни войск.
        //
        // Если фазы старта уже отработали (`Mobilization` + site select), у
        // каждой команды свои точка высадки, кредиты и загрузка
        // (`TeamMissionSupplies` в C++).
        let start_units = if deployments.is_empty() {
            max_core::deploy::deploy_starting_units(
                &mut state,
                config.start_gold,
                setup.settings.max_resources,
                &setup.teams,
            )
        } else {
            max_core::deploy::deploy_starting_units_with(
                &mut state,
                setup.settings.max_resources,
                &deployments,
            )
        };

        // `Access_UpdateVisibilityStatus` после высадки (`game_manager.cpp:1965`):
        // сбрасывает видимость юнитов по `InitStealthStatus` и заново строит
        // покрытие сканирования/тепловые карты. Без этого порядок высадки
        // оставлял «одностороннюю» видимость (кто высажен позже — видит
        // ранее высаженных, но не наоборот), а `visible_to_team` от спавна
        // делал чужие базы видимыми всем без разведки.
        state.update_visibility_status(true);

        let report = NewGameReport {
            planet: config.planet,
            seed: config.seed,
            teams: setup.teams.len(),
            aliens: output.aliens.len(),
            start_units,
            units: state.unit_count(),
            surface_crc: crc::crc16(&state.map.surface.to_bytes()),
            resources_crc: generated_resources_crc,
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
        self.local = None;
        self.build_menu = None;
        self.save_load = None;
        self.upgrade_menu = None;
        self.research_menu = None;
        self.unit_menu = None;
        self.self_destruct = None;
        self.alloc_menu = None;
        self.transfer_menu = None;
        self.help = None;
        self.help_mode = None;
        self.reset_visual_state();
        self.focus_player();
        // Стартовый снимок визуальных позиций (см. `load_game`).
        self.update_visual_units(0.0);

        report
    }

    /// Один тик симуляции (`GameManager_ProcessTick`).
    ///
    /// Заодно повторяет UI-хук `UnitInfo::MoveFinished` → `AutoSelectNext`
    /// (`auto_select`) для сетевого цикла: выделение — локальное UI-состояние,
    /// поэтому настройка действует и в lockstep-партии.
    pub fn tick(&mut self) -> TickReport {
        let before = self.selected_order_snapshot();
        self.state.tick();
        self.auto_select_after_tick(before);
        self.ticks += 1;

        TickReport {
            tick: self.ticks,
            turn_counter: self.state.turn_counter,
            game_state: self.state.game_state,
            units: self.state.unit_count(),
        }
    }

    /// Сбрасывает визуальное состояние (позиции, цели и маршруты юнитов).
    ///
    /// Вызывается при загрузке сейва и старте новой партии: иначе кадр мог бы
    /// анимировать «переезд» юнитов из старой партии в новую.
    fn reset_visual_state(&mut self) {
        self.visual_units.clear();
        self.visual_goals.clear();
        self.visual_paths.clear();
        self.frame_anims.clear();
        self.growth_anims.clear();
        // Режим выбора места относится к прежней партии: призрак не должен
        // переехать в новую.
        self.build_site = None;
    }

    /// Визуальная позиция юнита в клетках (дробная) для кадра.
    ///
    /// Если [`App::update_visual_units`] ещё не вызывался, возвращается
    /// симуляционная клетка — headless-кадры и тесты видят ровно `grid`.
    pub fn visual_position(&self, id: UnitId) -> Option<(f32, f32)> {
        let unit = self.state.unit(id)?;
        Some(
            self.visual_units
                .get(&id)
                .copied()
                .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y))),
        )
    }

    /// Угол корпуса для кадра: направление текущего визуального отрезка.
    ///
    /// Пока спрайт идёт по маршруту, это направление отрезка, на котором он
    /// находится (поле `angle` шага очереди); когда очередь пуста (юнит стоит
    /// или кадр догнал симуляцию) — текущий `UnitInfo::angle`. Так кадр
    /// поворачивает юнита ровно на клетке поворота и не забегает вперёд
    /// симуляции.
    ///
    /// Значение — только для отрисовки: в снапшот/CRC не входит и симуляцию не
    /// меняет.
    pub fn visual_angle(&self, id: UnitId) -> Option<u8> {
        let unit = self.state.unit(id)?;

        Some(
            match self.visual_paths.get(&id).and_then(|queue| queue.front()) {
                Some(step) => step.angle,
                None => unit.angle,
            },
        )
    }

    /// Номер кадра рабочего цикла строителя для отрисовки (`0..3`).
    ///
    /// `0` — кадр `+16` (`DrawSpriteFrame(image_index + 16)` в C++
    /// `StartBuilding`); дальше `+24`, `+32` — цикл `UnitsManager_ProcessOrderBuild`
    /// (`+8`, а при `image_index + 8 >= 40` — `-16`). Значение двигает
    /// [`App::update_unit_animations`] по реальному `dt`; `dt = 0` оставляет
    /// `0`, поэтому QA-кадры детерминированы.
    pub fn build_animation_step(&self, id: UnitId) -> u8 {
        self.frame_anims
            .get(&id)
            .map(|anim| anim.builder_step)
            .unwrap_or(0)
    }

    /// Кадровые часы юнита для отрисовки ([`render::FrameAnimation`]).
    ///
    /// Содержит цикл строителя, тик `ProcessOrderSentry` (ANIMATED-здания) и
    /// **промежуточный** полный цикл кадров работающих шахт/заводов. Значение
    /// двигает [`App::update_unit_animations`] по реальному `dt`; `dt = 0` —
    /// все счётчики нулевые, то есть статичный кадр C++ (golden не дрейфуют).
    pub fn unit_animation(&self, id: UnitId) -> crate::render::FrameAnimation {
        self.frame_anims
            .get(&id)
            .map(|anim| crate::render::FrameAnimation {
                builder_step: anim.builder_step,
                sentry_step: anim.sentry_step,
                work_step: anim.work_step,
            })
            .unwrap_or_default()
    }

    /// Снимок системной палитры с палитровыми циклами (`None` — без изменений).
    ///
    /// `Some` только после того, как часы [`App::color_cycles`] шагнули хотя бы
    /// раз (то есть при `dt > 0`). Отрисовка юнитов берёт этот снимок, иначе —
    /// `Assets::system_palette`. Палитра — только кадр: в снапшот/CRC не входит.
    pub fn animated_palette(&self) -> Option<&max_render::palette::Palette> {
        self.cycle_palette.as_ref()
    }

    /// Часы палитровых циклов (для тестов/диагностики).
    pub fn color_cycles(&self) -> &max_render::palette::ColorCycles {
        &self.color_cycles
    }

    /// Двигает палитровые циклы по реальному `dt` и обновляет снимок палитры.
    ///
    /// Порт `World::UpdateColorAnimations` (`world.cpp:190`): каждый цикл
    /// вращает свой диапазон слотов, когда накопилось `time_limit` мс. `dt <= 0`
    /// (QA/headless) сбрасывает часы — палитра снова исходная, golden-кадры
    /// детерминированы.
    fn update_color_cycles(&mut self, dt: f32) {
        // Клампим до 0.5 с: реальный кадр ~16 мс, а большой скачок (пауза окна)
        // не должен прокручивать палитру на сотни шагов.
        let dt = dt.clamp(0.0, 0.5);

        if !(dt > 0.0) {
            self.color_cycles.reset();
            self.cycle_palette = None;
            return;
        }

        if !self.color_cycles.advance(dt) {
            return;
        }

        let palette = self.assets.as_ref().map(|assets| {
            max_render::palette::rotate_palette(&assets.system_palette, &self.color_cycles)
        });

        self.cycle_palette = palette;
    }

    /// Визуальный масштаб юнита (`1.0` — полный размер).
    /// `2^-scaler_adjust`: `1/16 … 1` по мере `UnitsManager_ProcessOrderAwaitScaling`.
    /// В интерактиве переход сглаживается по реальному `dt`
    /// ([`GROWTH_ANIM_SECONDS`]); `dt = 0` и юниты без `scaler_adjust` — `1.0`.
    pub fn unit_growth_scale(&self, id: UnitId) -> f32 {
        self.growth_anims.get(&id).map_or(1.0, |anim| {
            let progress = (anim.elapsed / GROWTH_ANIM_SECONDS).clamp(0.0, 1.0);
            anim.start + (1.0 - anim.start) * progress
        })
    }

    /// Масштаб камеры для отрисовки юнита с учётом «вырастания».
    ///
    /// `UnitInfo::RenderSprite` не меняет `Gfx_ZoomLevel`: он делит размер
    /// кадра на `2^scaler_adjust`. В порте то же самое выражается уменьшенным
    /// `zoom` спрайта (1:1 — `64`), тень/башня масштабируются тем же кодом.
    pub fn unit_render_zoom(&self, id: UnitId, zoom: u32) -> u32 {
        let scale = self.unit_growth_scale(id);
        if scale >= 1.0 {
            return zoom.max(1);
        }
        ((zoom as f32) * scale).round().max(1.0) as u32
    }

    /// Визуальные часы анимаций кадра: строитель, `ANIMATED`-здания,
    /// промежуточный цикл работающих заводов и «вырастание».
    ///
    /// Как и [`App::update_visual_units`], это **только кадр**: `frame_anims`/
    /// `growth_anims` не входят в снапшот/CRC и не влияют на симуляцию.
    /// `dt <= 0` (QA/headless) сбрасывает их: строитель — поза `+16`, здание —
    /// статичный кадр C++, любая «вырастающая» постройка — полный размер,
    /// поэтому golden-кадры детерминированы.
    /// Часы мигания маркеров стройки (`DrawMap_BuildMMarkDelayCounter`,
    /// `drawmap.cpp:47`): кадр `BLDMRK1..5` меняется каждые три отрисовки
    /// карты. Порт считает по визуальному времени: `dt = 0` (QA/headless) —
    /// кадр 0, детерминированно.
    fn update_build_marker(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);

        if !(dt > 0.0) {
            self.build_marker_step = 0;
            self.build_marker_elapsed = 0.0;
            return;
        }

        self.build_marker_elapsed += dt;

        while self.build_marker_elapsed >= BUILD_MARKER_FRAME_SECONDS {
            self.build_marker_elapsed -= BUILD_MARKER_FRAME_SECONDS;
            self.build_marker_step = (self.build_marker_step + 1) % BUILD_MARKER_FRAMES as u8;
        }
    }

    /// Цвет индикатора выделения (`GameManager_MarkerColor`,
    /// `game_manager.cpp:1189-1192`): `0xFF ^= 0xFF` каждые
    /// [`render::UNIT_SELECTOR_BLINK_SECONDS`] = 210 мс (`TIMER_FPS_TO_MS(24) * 5`).
    /// `dt = 0` (QA/headless) возвращает `0xFF` — golden детерминированы.
    fn update_marker_color(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);

        if !(dt > 0.0) {
            self.marker_color_index = 0xFF;
            self.marker_color_elapsed = 0.0;
            return;
        }

        self.marker_color_elapsed += dt;

        while self.marker_color_elapsed >= crate::render::UNIT_SELECTOR_BLINK_SECONDS {
            self.marker_color_elapsed -= crate::render::UNIT_SELECTOR_BLINK_SECONDS;
            self.marker_color_index ^= 0xFF;
        }
    }

    fn update_unit_animations(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);

        if !(dt > 0.0) {
            self.frame_anims.clear();
            self.growth_anims.clear();
            return;
        }

        let ids = self.state.unit_ids_in_order();
        // C++ `settings["effects"]` (`settings.cpp:128`): при выключенных
        // эффектах `UnitsManager_ProcessOrderBuild`/`ProcessOrderSentry`
        // (`units_manager.cpp:2857/3030`) не листают кадры, поэтому и наши
        // кадровые часы не идут — кадр остаётся статичным.
        let effects = self.preferences.effects;
        let state = &self.state;

        self.frame_anims.retain(|id, _| state.unit(*id).is_some());

        for id in &ids {
            let Some(unit) = state.unit(*id) else {
                continue;
            };

            let builder = crate::render::builder_animation_active(unit);
            let sentry = effects && crate::render::sentry_animation_active(unit);
            let working = effects
                && WORKING_BUILDING_ANIMATION_ENABLED
                && crate::render::working_building_animation_active(unit);

            if !(builder || sentry || working) {
                self.frame_anims.remove(id);
                continue;
            }

            let anim = self.frame_anims.entry(*id).or_default();

            if builder {
                anim.builder_elapsed += dt;

                while anim.builder_elapsed >= BUILDER_ANIM_FRAME_SECONDS {
                    anim.builder_elapsed -= BUILDER_ANIM_FRAME_SECONDS;
                    anim.builder_step =
                        (anim.builder_step + 1) % crate::render::BUILDER_WORK_FRAME_STEPS;
                }
            } else {
                anim.builder_elapsed = 0.0;
                anim.builder_step = 0;
            }

            if sentry {
                anim.sentry_elapsed += dt;

                while anim.sentry_elapsed >= SENTRY_ANIM_FRAME_SECONDS {
                    anim.sentry_elapsed -= SENTRY_ANIM_FRAME_SECONDS;
                    anim.sentry_step = anim.sentry_step.wrapping_add(1);
                }
            } else {
                anim.sentry_elapsed = 0.0;
                anim.sentry_step = 0;
            }

            // ПРОМЕЖУТОЧНОЕ правило: полный цикл кадров работающего здания со
            // скоростью RADAR (1 кадр за 1/30 с). Модуль длины блока считает
            // рендер — здесь только счётчик кадров.
            if working {
                anim.work_elapsed += dt;

                while anim.work_elapsed >= WORKING_BUILDING_ANIM_FRAME_SECONDS {
                    anim.work_elapsed -= WORKING_BUILDING_ANIM_FRAME_SECONDS;
                    anim.work_step = anim.work_step.wrapping_add(1);
                }
            } else {
                anim.work_elapsed = 0.0;
                anim.work_step = 0;
            }
        }

        // «Вырастание»: старт от симуляционного `scaler_adjust` (C++ 2^-n).
        for id in &ids {
            let Some(unit) = self.state.unit(*id) else {
                continue;
            };

            if unit.scaler_adjust == 0 {
                continue;
            }

            self.growth_anims.entry(*id).or_insert(GrowthAnim {
                start: crate::render::scaler_scale(unit.scaler_adjust),
                elapsed: 0.0,
            });
        }

        for anim in self.growth_anims.values_mut() {
            anim.elapsed = (anim.elapsed + dt).min(GROWTH_ANIM_SECONDS);
        }

        // Готовые анимации живут до удаления юнита: иначе юнит с ещё не
        // сброшенным `scaler_adjust` (в QA симуляция между кадрами не тикает)
        // начинал бы «вырастать» заново на каждом кадре.
        self.growth_anims
            .retain(|id, _| self.state.unit(*id).is_some());
    }

    /// Подтягивает визуальные позиции к симуляционным.
    ///
    /// Симуляция двигает юнита на целую клетку за тик; кадр проходит **ту же
    /// последовательность клеток** с реальной скоростью оригинала
    /// ([`VISUAL_CELLS_PER_SECOND`]). Это и есть видимое движение «клетка за
    /// клеткой»: при `dt > 0` позиция спрайта между кадрами меняется, а после
    /// прохода маршрута совпадает с целевой клеткой.
    ///
    /// `dt <= 0` — снимок без интерполяции: headless/QA-кадры остаются
    /// детерминированными и совпадают с эталонами. То же правило для
    /// визуальных часов анимаций: строитель показывает рабочую позу `+16`, а
    /// «вырастающий» юнит — полный размер.
    ///
    /// P4: карта обновляется на месте (`retain` + `insert`), а не собирается
    /// заново каждый кадр. Это убирает аллокацию `HashMap` с `ids.len()`
    /// ячеек на каждом кадре 60 Гц; значения/порядок обхода не используются
    /// ни симуляцией, ни CRC (визуальные позиции — только для кадра).
    pub fn update_visual_units(&mut self, dt: f32) {
        // Портрет идёт теми же кадрами, что и карта: `dt = 0` (QA) его не двигает.
        self.update_portrait(dt);
        // Покадровая анимация строителя и «вырастание» — тоже по реальному dt.
        self.update_unit_animations(dt);
        // Мигание маркеров стройки (`DrawMap_RenderBuildMarker`).
        self.update_build_marker(dt);
        // Мигание индикатора выделения (`GameManager_MarkerColor`).
        self.update_marker_color(dt);
        // Палитровые циклы (маяки зданий, индикатор дрели) — тоже только кадр.
        self.update_color_cycles(dt);
        // Кадры `SLFDOPN1..6` диалога самоуничтожения (`TIMER_FPS_TO_MS(48)`).
        self.advance_self_destruct(dt);

        let ids = self.state.unit_ids_in_order();
        let dt = dt.clamp(0.0, 0.1);
        // `fast_movement` («Double Unit Steps»): C++ удваивает скорость
        // движения (`UnitInfo::Move`: `velocity += FastMovement ? 2 : 1`,
        // `unitinfo.cpp:1742`; `Paths_CalculateStep`: `max_velocity` 16 вместо
        // 8, `paths.cpp:297`), поэтому спрайт проходит клетку вдвое быстрее.
        // Симуляция порта двигает юнита на клетку за тик; скорость кадра —
        // единственное место, где настройка видна, ровно как визуальный эффект
        // оригинала.
        let speed_scale = if self.preferences.fast_movement {
            2.0
        } else {
            1.0
        };

        // Раздельные заимствования полей: `retain` читает состояние.
        let state = &self.state;
        let visual = &mut self.visual_units;
        let goals = &mut self.visual_goals;
        let paths = &mut self.visual_paths;

        visual.retain(|id, _| state.unit(*id).is_some());
        goals.retain(|id, _| state.unit(*id).is_some());
        paths.retain(|id, _| state.unit(*id).is_some());

        // `dt = 0` (headless/QA): снимок — кадр читает ровно симуляционную
        // клетку. Очередь маршрута сбрасывается, поэтому golden не дрейфуют.
        if dt <= 0.0 {
            for id in ids {
                let Some(unit) = state.unit(id) else {
                    continue;
                };
                let target = (f32::from(unit.grid.x), f32::from(unit.grid.y));
                visual.insert(id, target);
                goals.insert(id, (unit.grid.x, unit.grid.y));
                paths.remove(&id);
            }
            return;
        }

        let budget = VISUAL_CELLS_PER_SECOND * speed_scale * dt;

        for id in ids {
            let Some(unit) = state.unit(id) else {
                continue;
            };
            let grid = (unit.grid.x, unit.grid.y);
            let target = (f32::from(grid.0), f32::from(grid.1));
            let current = visual.entry(id).or_insert(target);
            let goal = goals.entry(id).or_insert(grid);
            let queue = paths.entry(id).or_default();

            // Симуляция сдвинула юнита (обычно на одну клетку за тик).
            // Дописываем клетки фактического маршрута, чтобы кадр шёл по ним,
            // а не срезал путь по прямой; большой скачок — телепорт/синк.
            if *goal != grid {
                let dx = i32::from(grid.0) - i32::from(goal.0);
                let dy = i32::from(grid.1) - i32::from(goal.1);
                let distance = dx.abs().max(dy.abs());

                if distance > VISUAL_TELEPORT_CELLS {
                    queue.clear();
                    *current = target;
                } else {
                    let divisor = distance.max(1);
                    let mut previous = *goal;

                    for step in 1..=divisor {
                        let x = goal.0 + (dx * step / divisor) as i16;
                        let y = goal.1 + (dy * step / divisor) as i16;
                        let step_dx = i32::from(x) - i32::from(previous.0);
                        let step_dy = i32::from(y) - i32::from(previous.1);
                        // Направление отрезка — из фактического смещения клеток,
                        // а не из текущего `unit.angle`: к моменту кадра
                        // симуляция уже могла повернуться к следующему шагу
                        // (`GroundPath::UpdateUnitAngle`) или уйти дальше.
                        let angle = if step_dx == 0 && step_dy == 0 {
                            unit.angle
                        } else {
                            max_core::unit_order::paths_get_angle(step_dx, step_dy) as u8
                        };

                        queue.push_back(VisualStep {
                            x: f32::from(x),
                            y: f32::from(y),
                            angle,
                        });
                        previous = (x, y);
                    }
                }

                *goal = grid;
            }

            // Проходим не больше `budget` клеток за кадр; остаток бюджета
            // переносится на следующие точки маршрута — скорость постоянна.
            let mut left = budget;

            while left > 0.0 {
                let Some(&waypoint) = queue.front() else {
                    break;
                };
                let dx = waypoint.x - current.0;
                let dy = waypoint.y - current.1;
                let distance = dx.abs().max(dy.abs());

                if distance <= left {
                    *current = (waypoint.x, waypoint.y);
                    queue.pop_front();
                    left -= distance;
                } else {
                    let factor = left / distance;
                    current.0 += dx * factor;
                    current.1 += dy * factor;
                    left = 0.0;
                }
            }
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

    /// Центрирует камеру на первом значимом юните локального игрока.
    ///
    /// Это `GameManager_GetFirstRelevantUnit` (`game_manager.cpp:3140`), а не
    /// «первый юнит в списке»: C++ ищет **шахту** (`Access_GetFirstMiningStation`),
    /// затем первый активный наземный/морской, воздушный и, наконец,
    /// стационарный юнит. Раньше камера вставала на первый юнит канонического
    /// обхода — а это `GROUND_COVER` (`SMLSLAB`/`LRGSLAB`), клетка на одну
    /// вниз-влево от базы. Из-за сдвига клики, рассчитанные от базы, попадали
    /// на соседнюю клетку со стационарной шахтой: выделялась она, RMB отвечал
    /// «Unit cannot move (air/stationary)», и юнит не двигался.
    pub fn focus_player(&mut self) {
        let team = self.state.player_team;
        let mut grid = None;

        // 1. `Access_GetFirstMiningStation`: шахта команды в `StationaryUnits`.
        for id in self
            .state
            .unit_list(max_core::unit::UnitListKind::Stationary)
        {
            if let Some(unit) = self.state.unit(*id) {
                if unit.team == team && unit.unit_type == UnitType::MiningStation {
                    grid = Some(unit.grid);
                    break;
                }
            }
        }

        // 2. `Access_GetFirstActiveUnit` по трём спискам в порядке C++.
        if grid.is_none() {
            for kind in [
                max_core::unit::UnitListKind::MobileLandSea,
                max_core::unit::UnitListKind::MobileAir,
                max_core::unit::UnitListKind::Stationary,
            ] {
                for id in self.state.unit_list(kind) {
                    if let Some(unit) = self.state.unit(*id) {
                        if unit.team == team && unit.orders != max_core::enums::UnitOrder::Idle {
                            grid = Some(unit.grid);
                            break;
                        }
                    }
                }

                if grid.is_some() {
                    break;
                }
            }
        }

        if let Some(grid) = grid {
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
        let hash = max_net::DataHash::from_files(&split.max_res, split.handshake_patches_path())
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
