//! Исполнитель `.qa`-скриптов (слой 2 QA): in-process UI-драйвер без SDL.
//!
//! Драйвер держит те же объекты, что `main.rs`: [`App`] (состояние партии,
//! ассеты, камера) и [`Screen`] + [`UiContext`] (машина экранов). Каждая
//! команда скрипта превращается в **тот же** `InputEvent`, который пришёл бы
//! от мыши/клавиатуры:
//!
//! * меню/настройка → [`Screen::handle_input`];
//! * партия → [`crate::ui::handle_session_input`] (выделение, приказы,
//!   кнопка End Turn, окно перехода хода);
//! * игровой цикл → [`crate::App::local_update`] ровно так, как его крутит
//!   главный цикл `main.rs` (тик, ход ИИ, переход на экран победы).
//!
//! Поэтому скрипт проверяет не «внутренние функции», а полный путь игрока.
//! Шаги пишутся в `log.txt`, кадры `shot` — в PNG, падение любого `assert`
//! возвращает [`QaReport::passed`] = `false` (CLI-обёртка `qa_run` в этом
//! случае завершает процесс кодом 1).

use std::path::{Path, PathBuf};

use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_net::{CreateGameParams, GameInfo, GameStatus};
use max_render::buffer::RgbaBuffer;
use max_render::layout;

use crate::hud;
use crate::local::{LocalPhase, LocalSetup};
use crate::menu;
use crate::mobilize::{
    AVAILABLE_AREA, BUY_BUTTON, CANCEL_BUTTON as MOBILIZE_CANCEL, CARGO_BAR, DELETE_BUTTON,
    DONE_BUTTON as MOBILIZE_DONE, LIST_ROW, PURCHASED_AREA, PURCHASE_UNITS, TOGGLE_BUTTON,
};
use crate::qa::png::{self, Image};
use crate::qa::script::{
    self, Assertion, BuildCommand, Command, HelpCommand, LandingCommand, MenuTarget,
    MobilizeAction, PhaseName, PrefsCommand, ResearchCommand, ScreenName, SetupAction, Step,
    TransferCommand, UnitMenuCommand, UpgradeCommand,
};
use crate::setup::{self, SetupScreen};
use crate::ui::{
    handle_session_input, InputEvent, Key, MenuClient, Screen, SessionRequest, Transition,
    UiContext,
};
use crate::upgrademenu::UpgradeFilter;
use crate::{App, AppConfig, AppMode};

/// Корень репозитория (от `crates/max_app` три уровня вверх).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .to_path_buf()
}

/// Настройки прогона QA-скрипта.
#[derive(Debug, Clone)]
pub struct QaConfig {
    /// Каталог `max-original` (`MAX.RES`, `COLOR.PAL`, `.WRL`).
    pub data_dir: PathBuf,
    /// Каталог `assets` (`PATCHES.RES`).
    pub assets_dir: PathBuf,
    /// Куда писать `log.txt` и PNG кадров.
    pub out_dir: PathBuf,
    /// Предел кадров на одну команду `wait`/`end_turn` (защита от зависания).
    pub frame_limit: usize,
}

impl QaConfig {
    /// Настройки по умолчанию: данные из корня репозитория, вывод — `out_dir`.
    pub fn new(out_dir: impl Into<PathBuf>) -> Self {
        let root = repo_root();

        Self {
            data_dir: root.join("max-original"),
            assets_dir: root.join("assets"),
            out_dir: out_dir.into(),
            frame_limit: 40_000,
        }
    }

    /// Другой каталог данных игры.
    pub fn with_data_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.data_dir = dir.into();
        self
    }

    /// Другой каталог ассетов.
    pub fn with_assets_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.assets_dir = dir.into();
        self
    }

    /// Другой предел кадров.
    pub fn with_frame_limit(mut self, limit: usize) -> Self {
        self.frame_limit = limit;
        self
    }
}

/// Итог прогона скрипта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaReport {
    /// Все команды выполнились (и `assert` не падали).
    pub passed: bool,
    /// Тексты падений (по одному на первый сбой шага).
    pub failures: Vec<String>,
    /// Полный лог шагов.
    pub log: String,
    /// Записанные кадры.
    pub shots: Vec<PathBuf>,
    /// Сколько кадров игрового цикла прокручено.
    pub frames: usize,
}

impl QaReport {
    /// Пишет `log.txt` в каталог и возвращает путь.
    pub fn write_log(&self, dir: impl AsRef<Path>) -> Result<PathBuf, String> {
        let path = dir.as_ref().join("log.txt");

        std::fs::write(&path, &self.log)
            .map_err(|error| format!("лог {}: {error}", path.display()))?;

        Ok(path)
    }

    /// Одна строка для stdout/CI.
    pub fn summary(&self) -> String {
        if self.passed {
            format!(
                "PASS: {} кадров, {} кадров-файлов",
                self.frames,
                self.shots.len()
            )
        } else {
            format!(
                "FAIL: {} (кадров {}, файлов {})",
                self.failures.join("; "),
                self.frames,
                self.shots.len()
            )
        }
    }
}

/// Что делать после команды.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Exit,
}

/// Детерминированный клиент лобби: скриншот `JoinList` без Horizon.
///
/// Сеть в QA-драйвере не поднимается (слой 2 headless), поэтому `list_games`
/// возвращает фиксированный список — кадр `join-list` воспроизводим.
#[derive(Debug, Clone, Copy)]
pub struct QaMenuClient;

impl QaMenuClient {
    fn game(id: &str, name: &str, world: i64, port: u16, players: u32) -> GameInfo {
        GameInfo {
            id: id.to_string(),
            name: name.to_string(),
            world,
            status: GameStatus::Running,
            port,
            players,
            min_players: 2,
            password_protected: false,
        }
    }
}

impl MenuClient for QaMenuClient {
    fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
        Ok(vec![
            Self::game("qa-session-1", "QA Skirmish", 4, 31_554, 1),
            Self::game("qa-session-2", "QA Duel", 11, 31_555, 2),
        ])
    }

    fn create_game(&mut self, _params: CreateGameParams) -> Result<GameInfo, String> {
        Err("QA: создание сетевой игры в драйвере не поддерживается".to_string())
    }
}

/// In-process драйвер UI: приложение + машина экранов + кадровый буфер.
pub struct QaRunner {
    app: App,
    ctx: UiContext,
    screen: Screen,
    config: QaConfig,
    buffer: RgbaBuffer,
    log: String,
    shots: Vec<PathBuf>,
    frames: usize,
    detail: Option<String>,
}

impl QaRunner {
    /// Готовит приложение с настоящими ассетами (`MAX.RES`, `PATCHES.RES`).
    pub fn new(config: QaConfig) -> Result<Self, String> {
        std::fs::create_dir_all(&config.out_dir)
            .map_err(|error| format!("каталог {}: {error}", config.out_dir.display()))?;

        let app_config = AppConfig::default()
            .with_data_dir(&config.data_dir)
            .with_assets_dir(&config.assets_dir)
            .with_mode(AppMode::Ui)
            // P3: сейвы (в т.ч. автосейв) пишутся в каталог вывода QA, а не в
            // `max-original`: прогон не оставляет мусора в данных игры.
            .with_save_dir(&config.out_dir);

        let mut app = App::new(app_config);
        app.init_assets()
            .map_err(|error| format!("ассеты: {error:#}"))?;

        let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
        ctx.player_name = "Commander".to_string();
        ctx.world_dir = config.data_dir.clone();
        ctx.intro_mve_path = app.assets().and_then(|assets| assets.intro_mve.clone());
        ctx.status = String::new();

        Ok(Self {
            app,
            ctx,
            screen: Screen::MainMenu,
            config,
            buffer: RgbaBuffer::new(640, 480),
            log: String::new(),
            shots: Vec::new(),
            frames: 0,
            detail: None,
        })
    }

    /// Приложение (для тестов и диагностики).
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Текущий экран.
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// Записанные кадры.
    pub fn shots(&self) -> &[PathBuf] {
        &self.shots
    }

    /// Прокрученные кадры игрового цикла.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Разбирает и прогоняет файл скрипта.
    pub fn run_file(&mut self, path: impl AsRef<Path>) -> QaReport {
        let path = path.as_ref();

        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                return QaReport {
                    passed: false,
                    failures: vec![format!("{}: {error}", path.display())],
                    log: format!("# qa_run: не прочитан {}: {error}\n", path.display()),
                    shots: Vec::new(),
                    frames: 0,
                }
            }
        };

        match script::parse_script(&text) {
            Ok(steps) => self.run(&steps, &path.display().to_string()),
            Err(error) => {
                let message = format!("{}: {error}", path.display());

                QaReport {
                    passed: false,
                    failures: vec![message.clone()],
                    log: format!("# qa_run: {message}\n"),
                    shots: Vec::new(),
                    frames: 0,
                }
            }
        }
    }

    /// Прогоняет разобранные шаги.
    pub fn run(&mut self, steps: &[Step], title: &str) -> QaReport {
        self.log.clear();
        self.shots.clear();
        self.frames = 0;
        self.detail = None;

        self.log
            .push_str(&format!("# qa_run: {title} ({} шагов)\n", steps.len()));
        // P4 (TC-1108): память на старте прогона — чтобы soak-лог показывал
        // рост RSS между «до» и «после», а не только абсолютное значение.
        let mem_start = crate::mem::process_memory();
        self.log
            .push_str(&format!("# mem start: {}\n", crate::mem::memory_line()));
        crate::profile::reset();

        let mut failures = Vec::new();

        for (index, step) in steps.iter().enumerate() {
            self.detail = None;
            let head = format!("[{:03}] {}", index + 1, step.source);

            match self.execute(&step.command) {
                Ok(Flow::Continue) => {
                    let detail = self
                        .detail
                        .as_ref()
                        .map(|detail| format!(" ({detail})"))
                        .unwrap_or_default();
                    self.log.push_str(&format!("{head} -> ok{detail}\n"));
                }
                Ok(Flow::Exit) => {
                    self.log.push_str(&format!("{head} -> exit\n"));
                    break;
                }
                Err(error) => {
                    self.log.push_str(&format!("{head} -> FAIL: {error}\n"));
                    failures.push(format!("строка {}: {error}", step.line));
                    break;
                }
            }
        }

        let passed = failures.is_empty();

        let mem_end = crate::mem::process_memory();
        let delta = match (mem_start, mem_end) {
            (Some(start), Some(end)) => format!(" (delta={:+} KiB)", end.growth_kb(start)),
            _ => String::new(),
        };
        self.log.push_str(&format!(
            "# mem end: {}{delta}\n",
            crate::mem::memory_line()
        ));
        // P4: профиль тика — только когда включён `MAX_TICK_PROFILE=1`
        // (иначе счётчики пусты и строка была бы шумом).
        if crate::profile::enabled() {
            self.log.push_str(&crate::profile::summary());
            self.log.push('\n');
        }

        self.log.push_str(&format!(
            "RESULT: {} · кадров {}, PNG {}\n",
            if passed { "PASS" } else { "FAIL" },
            self.frames,
            self.shots.len()
        ));

        QaReport {
            passed,
            failures,
            log: self.log.clone(),
            shots: self.shots.clone(),
            frames: self.frames,
        }
    }

    fn execute(&mut self, command: &Command) -> Result<Flow, String> {
        // `main.rs` каждый кадр проводит `settings["disable_fx"]`/громкость в
        // аудио-слой; QA-драйвер делает то же перед каждой командой, чтобы
        // `assert prefs-audio` видел эффект `prefs toggle disable_fx`.
        self.sync_audio();

        match command {
            Command::Click(x, y) => self.send(InputEvent::MouseClick(*x, *y)),
            Command::ClickUnit(name) => self.click_unit(name),
            Command::RightClick(x, y) => self.send(InputEvent::RightClick(*x, *y)),
            Command::Key(key) => self.send(InputEvent::Key(*key)),
            Command::Type(text) => {
                for character in text.chars() {
                    self.send(InputEvent::Key(Key::Char(character)))?;
                }
                self.detail = Some(format!("{text:?}"));
                Ok(Flow::Continue)
            }
            Command::Wait(frames) => self.wait(*frames as usize),
            Command::Tick(frames) => {
                for _ in 0..*frames {
                    self.frame();
                }
                self.detail = Some(format!("{frames} кадров"));
                Ok(Flow::Continue)
            }
            Command::Mem => self.mem(),
            Command::Anim(millis) => self.anim(*millis),
            Command::Shot(name) => self.shot(name),
            Command::Assert(assertion) => self.check(assertion.clone()),
            Command::Save(name) => self.save(name),
            Command::SaveV70(name) => self.save_v70(name),
            Command::Load(name) => self.load(name),
            Command::Exit => Ok(Flow::Exit),
            Command::Menu(target) => self.menu(*target),
            Command::Setup(action) => self.setup(*action),
            Command::Players { slot, kind } => self.players(*slot, *kind),
            Command::Select(x, y) => self.select_point(*x, *y),
            Command::SelectUnit(index) => self.select_unit(*index),
            Command::SelectLast => self.select_last(),
            Command::SelectType(name) => self.select_type(name),
            Command::Move(x, y) => self.move_point(*x, *y, &format!("move {x} {y}")),
            Command::MoveGrid(x, y) => self.move_grid(*x, *y),
            Command::MoveNear => self.move_near(),
            Command::EndTurn => self.end_turn(),
            Command::Mobilize(action) => self.mobilize(*action),
            Command::Landing(action) => self.landing(*action),
            Command::Build(action) => self.build(*action),
            Command::Upgrade(action) => self.upgrade(*action),
            Command::Research(action) => self.research(*action),
            Command::Help(action) => self.help(*action),
            Command::UnitMenu(action) => self.unit_menu(action.clone()),
            Command::Allocate(action) => self.allocate(action.clone()),
            Command::Transfer(action) => self.transfer(*action),
            Command::Prefs(action) => self.prefs(action.clone()),
        }
    }

    /// `allocate …` — диалог распределения добычи шахты (C++ `AllocMenu`,
    /// окно `ALLOCFRM`).
    fn allocate(&mut self, action: crate::qa::script::AllocateCommand) -> Result<Flow, String> {
        use crate::qa::script::AllocateCommand;

        let app = &mut self.app;

        let detail = match action {
            AllocateCommand::Open => {
                // Диалог открывается тем же путём, что у игрока
                // (`App::open_alloc_menu` — обработчик пункта `allocate`).
                if app.open_alloc_menu() {
                    let (complex, free) = app
                        .alloc_menu()
                        .map(|menu| (menu.complex.0, menu.free_capacity()))
                        .unwrap_or((0, 0));

                    format!("меню открыто (комплекс {complex}, свободно {free})")
                } else {
                    return Err(
                        "allocate open: нужна выделенная включённая шахта (select type MiningStation)"
                            .to_string(),
                    );
                }
            }
            AllocateCommand::Select(kind) => {
                let Some(menu) = app.alloc_menu_mut() else {
                    return Err("allocate select: меню не открыто".to_string());
                };
                menu.selected = kind;
                format!("выбран ресурс {}", kind.name())
            }
            AllocateCommand::Set { kind, level } => {
                let Some(menu) = app.alloc_menu() else {
                    return Err("allocate set: меню не открыто".to_string());
                };
                let kind = kind.unwrap_or(menu.selected);
                app.alloc_menu_set(kind, level);
                let Some(menu) = app.alloc_menu() else {
                    return Err("allocate set: меню закрылось".to_string());
                };
                format!(
                    "{} = {} (свободно {})",
                    kind.name(),
                    menu.production_of(kind),
                    menu.free_capacity()
                )
            }
            AllocateCommand::Adjust { kind, increase } => {
                let Some(menu) = app.alloc_menu() else {
                    return Err("allocate row: меню не открыто".to_string());
                };
                let kind = kind.unwrap_or(menu.selected);
                app.alloc_menu_adjust(kind, increase);
                let Some(menu) = app.alloc_menu() else {
                    return Err("allocate row: меню закрылось".to_string());
                };
                format!(
                    "{} {}= {} (свободно {})",
                    kind.name(),
                    if increase { "+1" } else { "-1" },
                    menu.production_of(kind),
                    menu.free_capacity()
                )
            }
            AllocateCommand::Done | AllocateCommand::Cancel => {
                if app.alloc_menu().is_none() {
                    return Err("allocate done: меню не открыто".to_string());
                }
                // `Remote_SendNetPacket_11`: `ORDER_NEW_ALLOCATE` (08/P5) на
                // каждую шахту комплекса, затем закрытие.
                let queued = app.queue_alloc_orders();
                app.flush_orders();
                let line = app
                    .close_alloc_menu()
                    .unwrap_or_else(|| "меню закрыто".to_string());
                format!("{line}; приказов 08/P5: {queued}")
            }
        };

        self.detail = Some(detail);

        Ok(Flow::Continue)
    }

    /// `settings["disable_fx"]`/`fx_sound_level` → `AudioPlayer`.
    fn sync_audio(&mut self) {
        self.ctx.audio.set_enabled(self.app.sound_enabled());
        self.ctx
            .audio
            .set_volume(self.app.preferences().sound_volume());
    }

    /// Текущий диалог `Preferences` (в меню — `Screen::Preferences`, в игре —
    /// модальное окно `App::preferences_dialog`).
    fn prefs_dialog(&self) -> Option<&crate::preferences::PreferencesDialog> {
        match &self.screen {
            Screen::Preferences(dialog) => Some(dialog),
            Screen::InSession { .. } => self.app.preferences_dialog(),
            _ => None,
        }
    }

    fn require_prefs(&self, what: &str) -> Result<(), String> {
        if self.prefs_dialog().is_some() {
            Ok(())
        } else {
            Err(format!(
                "prefs {what}: диалог не открыт (сейчас {})",
                self.screen_label()
            ))
        }
    }

    /// `prefs …` — диалог `Preferences`/`Setup` (C++ `OptionsMenu`).
    fn prefs(&mut self, action: PrefsCommand) -> Result<Flow, String> {
        use crate::preferences::{PrefKey, DONE_BUTTON, CANCEL_BUTTON, HELP_BUTTON, WINDOW_ULX, WINDOW_ULY};

        let detail = match action {
            PrefsCommand::Open => {
                if self.prefs_dialog().is_some() {
                    return Err("prefs open: диалог уже открыт".to_string());
                }

                match &self.screen {
                    Screen::InSession { .. } => {
                        let rect = crate::panels::PanelButton::Preferences.rect();
                        self.send(InputEvent::MouseClick(
                            rect.ulx + rect.width / 2,
                            rect.uly + rect.height / 2,
                        ))?;

                        if self.app.preferences_dialog().is_none() {
                            return Err(
                                "prefs open: кнопка Preferences панели не открыла диалог".to_string(),
                            );
                        }
                        "Preferences (PREFSPIC)".to_string()
                    }
                    Screen::MainMenu => {
                        self.click_main_menu_button(3, "prefs open")?;
                        if !matches!(self.screen, Screen::Preferences(_)) {
                            return Err(format!(
                                "prefs open: Setup открыл {} вместо диалога",
                                self.screen_label()
                            ));
                        }
                        "Setup (SETUPPIC)".to_string()
                    }
                    _ => {
                        return Err(format!(
                            "prefs open: нужен MainMenu или InGame, сейчас {}",
                            self.screen_label()
                        ))
                    }
                }
            }
            PrefsCommand::Toggle(name) => {
                self.require_prefs("toggle")?;
                let key = PrefKey::parse(&name)
                    .ok_or_else(|| format!("prefs toggle: неизвестная опция «{name}»"))?;
                let dialog = self.prefs_dialog().expect("prefs открыт");
                let rect = dialog
                    .screen_rect(key)
                    .ok_or_else(|| format!("prefs toggle {name}: контрол не виден в этом варианте"))?;
                let before = dialog.value(key);
                let (x, y) = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

                self.send(InputEvent::MouseClick(x, y))?;

                let dialog = self.prefs_dialog().expect("prefs остаётся открытым");
                let after = dialog.value(key);
                if before == after {
                    return Err(format!(
                        "prefs toggle {name}: значение не изменилось ({before:?})"
                    ));
                }
                format!("{name} = {after:?}")
            }
            PrefsCommand::Set { name, value } => {
                self.require_prefs("set")?;
                let key = PrefKey::parse(&name)
                    .ok_or_else(|| format!("prefs set: неизвестная опция «{name}»"))?;
                let dialog = self.prefs_dialog().expect("prefs открыт");
                let x = dialog.slider_screen_x(key, value).ok_or_else(|| {
                    format!("prefs set {name}: контрол не виден в этом варианте")
                })?;
                let expected = dialog
                    .slider_value_at(key, x)
                    .ok_or_else(|| format!("prefs set {name}: нет жёлоба слайдера"))?;
                let rect = dialog
                    .screen_rect(key)
                    .ok_or_else(|| format!("prefs set {name}: нет области клика"))?;
                let y = rect.uly + rect.height / 2;
                let (min, max) = key.range();
                let target = value.clamp(min, max);

                self.send(InputEvent::MouseClick(x, y))?;

                let dialog = self.prefs_dialog().expect("prefs остаётся открытым");
                let actual = dialog.value(key);
                if actual != Some(expected) {
                    return Err(format!(
                        "prefs set {name}: клик в {x} дал {actual:?}, ожидалось {expected}"
                    ));
                }
                if (actual.unwrap_or(min) - target).abs() > 1 {
                    return Err(format!(
                        "prefs set {name}: значение {target} недостижимо кликом (дали {actual:?})"
                    ));
                }
                format!("{name} = {}", actual.unwrap_or(target))
            }
            action @ (PrefsCommand::Done | PrefsCommand::Cancel) => {
                self.require_prefs("done")?;
                let dialog = self.prefs_dialog().expect("prefs открыт");
                let variant = dialog.variant();
                let rect = if matches!(action, PrefsCommand::Done) {
                    DONE_BUTTON
                } else {
                    CANCEL_BUTTON
                };
                let (x, y) = (
                    WINDOW_ULX + rect.ulx + rect.width / 2,
                    WINDOW_ULY + rect.uly + rect.height / 2,
                );

                self.send(InputEvent::MouseClick(x, y))?;

                if self.prefs_dialog().is_some() {
                    return Err("prefs done/cancel: диалог не закрылся".to_string());
                }
                format!("{:?} ({variant:?})", action)
            }
            PrefsCommand::Help => {
                self.require_prefs("help")?;
                let (x, y) = (
                    WINDOW_ULX + HELP_BUTTON.ulx + HELP_BUTTON.width / 2,
                    WINDOW_ULY + HELP_BUTTON.uly + HELP_BUTTON.height / 2,
                );

                self.send(InputEvent::MouseClick(x, y))?;

                if !matches!(self.screen, Screen::Help { .. }) && self.app.help_screen().is_none() {
                    return Err("prefs help: окно помощи не открылось".to_string());
                }
                "Help".to_string()
            }
        };

        self.detail = Some(detail);
        Ok(Flow::Continue)
    }

    /// `unit-menu …` — меню команд выделенного юнита (C++
    /// `GameManager_PopupButtons`): открытие тем же повторным кликом, выбор
    /// пункта кликом по его кнопке или горячей клавишей, закрытие `Esc`.
    fn unit_menu(&mut self, action: UnitMenuCommand) -> Result<Flow, String> {
        match action {
            UnitMenuCommand::Open => {
                let Some(id) = self.app.hud().selected else {
                    return Err("unit-menu open: сначала выделите своего юнита".to_string());
                };
                let grid = self
                    .app
                    .state()
                    .unit(id)
                    .map(|unit| unit.grid)
                    .ok_or_else(|| "unit-menu open: юнит исчез".to_string())?;

                // Триггер — повторный клик по уже выделенному юниту
                // (`GameManager_ClickUnit`).
                self.app.viewport_mut().center_on_grid(grid.x, grid.y);
                let (x, y) = hud::tile_center(&self.app.map_viewport(), grid);
                self.send(InputEvent::MouseClick(
                    x + layout::MAIN_MAP.ulx,
                    y + layout::MAIN_MAP.uly,
                ))?;

                let Some(menu) = self.app.unit_menu() else {
                    let unit = self.app.state().unit(id);
                    return Err(format!(
                        "unit-menu open: меню не открылось (юнит {:?} в ({}, {}), orders={:?}/{:?})",
                        unit.map(|unit| unit.unit_type),
                        grid.x,
                        grid.y,
                        unit.map(|unit| unit.orders),
                        unit.map(|unit| unit.state)
                    ));
                };

                self.detail = Some(format!(
                    "пунктов: {} — {}",
                    menu.items.len(),
                    menu.items
                        .iter()
                        .map(|item| item.caption.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                Ok(Flow::Continue)
            }
            UnitMenuCommand::Item(name) => {
                let Some(menu) = self.app.unit_menu() else {
                    return Err(format!("unit-menu item {name}: меню не открыто"));
                };

                let index = menu.index_by_text(&name).ok_or_else(|| {
                    format!(
                        "unit-menu item {name}: нет такого пункта (есть: {})",
                        menu.items
                            .iter()
                            .map(|item| item.caption.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;
                let item = menu.items[index].clone();

                if !item.enabled {
                    return Err(format!(
                        "unit-menu item {name}: пункт отключён — {}",
                        item.note
                    ));
                }

                let rect = menu.button_rect(index);
                let point = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);
                self.detail = Some(format!("{} в ({}, {})", item.caption, point.0, point.1));
                self.send(InputEvent::MouseClick(point.0, point.1))
            }
            UnitMenuCommand::Key(character) => {
                self.send(InputEvent::Key(Key::Char(character)))
            }
            UnitMenuCommand::Close => self.send(InputEvent::Key(Key::Escape)),
        }
    }

    /// `click unit NAME` — левый клик по юниту команды игрока без проверки
    /// выделения (курсорные режимы: x-fer/repair/reload). Камера
    /// центрируется, поэтому клик всегда попадает в клетку юнита.
    fn click_unit(&mut self, name: &str) -> Result<Flow, String> {
        let team = self.app.player_team();
        let wanted = name.trim().to_ascii_lowercase();
        let id = self
            .app
            .state()
            .unit_ids_in_order()
            .into_iter()
            .filter(|id| {
                self.app.state().unit(*id).is_some_and(|unit| {
                    unit.team == team
                        && (crate::buildmenu::unit_name(unit.unit_type).to_ascii_lowercase()
                            == wanted
                            || format!("{:?}", unit.unit_type).to_ascii_lowercase() == wanted)
                })
            })
            .next_back()
            .ok_or_else(|| format!("click unit {name}: у команды {team:?} нет такого юнита"))?;

        let grid = self
            .app
            .state()
            .unit(id)
            .map(|unit| unit.grid)
            .ok_or_else(|| "click unit: юнит исчез".to_string())?;

        self.app.viewport_mut().center_on_grid(grid.x, grid.y);

        let (x, y) = hud::tile_center(&self.app.map_viewport(), grid);
        self.detail = Some(format!("клик по {name} в ({x}, {y})"));
        self.send(InputEvent::MouseClick(
            x + layout::MAIN_MAP.ulx,
            y + layout::MAIN_MAP.uly,
        ))
    }

    /// `transfer …` — окно X-FER (C++ `TransferMenu`): значение полосы,
    /// кнопки `±1`, `Done` (Enter → 08/P3 `ORDER_TRANSFER`) и `Esc`.
    fn transfer(&mut self, action: TransferCommand) -> Result<Flow, String> {
        if self.app.transfer_menu().is_none() {
            return Err("transfer: окно X-FER не открыто".to_string());
        }

        match action {
            TransferCommand::Amount(value) => {
                self.app.transfer_set_amount(value);
                let actual = self
                    .app
                    .transfer_menu()
                    .map(|menu| menu.amount)
                    .unwrap_or(0);
                self.detail = Some(format!("amount={actual}"));
                Ok(Flow::Continue)
            }
            TransferCommand::Right => {
                let changed = self.app.transfer_adjust(true);
                let actual = self
                    .app
                    .transfer_menu()
                    .map(|menu| menu.amount)
                    .unwrap_or(0);
                self.detail = Some(format!("+1 -> {actual} (изменение: {changed})"));
                Ok(Flow::Continue)
            }
            TransferCommand::Left => {
                let changed = self.app.transfer_adjust(false);
                let actual = self
                    .app
                    .transfer_menu()
                    .map(|menu| menu.amount)
                    .unwrap_or(0);
                self.detail = Some(format!("-1 -> {actual} (изменение: {changed})"));
                Ok(Flow::Continue)
            }
            TransferCommand::Done => {
                let amount = self
                    .app
                    .transfer_menu()
                    .map(|menu| menu.amount)
                    .unwrap_or(0);
                self.detail = Some(format!("Done: {amount}"));
                // Через ввод — тот же путь, что Enter у игрока (и офлайн
                // приказ сразу применяется).
                self.send(InputEvent::Key(Key::Enter))
            }
            TransferCommand::Cancel => {
                self.detail = Some("Cancel".to_string());
                self.send(InputEvent::Key(Key::Escape))
            }
            TransferCommand::Help => {
                self.detail = Some("Help".to_string());
                self.send(InputEvent::Key(Key::Char('?')))
            }
        }
    }

    /// `build …`: экран `BuildMenu` (P2) — те же действия, что у игрока
    /// (клавиша `B`, клики по строкам списка, кнопки Build/Delete/Done).
    fn build(&mut self, action: BuildCommand) -> Result<Flow, String> {
        let app = &mut self.app;

        let detail = match action {
            BuildCommand::Open => {
                if !app.open_build_menu() {
                    return Err(
                        "build open: выделенный юнит не строитель (нужен CONSTRCT/ENGINEER/завод)"
                            .to_string(),
                    );
                }
                "меню открыто".to_string()
            }
            BuildCommand::Select(index) => {
                let Some(menu) = app.build_menu() else {
                    return Err("build select: меню не открыто".to_string());
                };

                if index as usize >= menu.unit_types.len() {
                    return Err(format!(
                        "build select {index}: в списке {} типов",
                        menu.unit_types.len()
                    ));
                }

                let name = crate::buildmenu::unit_name(menu.unit_types[index as usize]).to_string();
                app.build_menu_select(index as usize);
                format!("выбран {name}")
            }
            BuildCommand::Add => {
                if app.build_menu().is_none() {
                    return Err("build add: меню не открыто".to_string());
                }

                if !app.build_menu_add() {
                    return Err("build add: тип не добавлен в очередь".to_string());
                }

                format!("очередь: {}", app.build_menu_queue().len())
            }
            BuildCommand::Delete => {
                if !app.build_menu_delete() {
                    return Err("build delete: очередь пуста".to_string());
                }

                format!("очередь: {}", app.build_menu_queue().len())
            }
            BuildCommand::Rate(rate) => {
                if !app.build_menu_set_rate(rate) {
                    let status = app
                        .build_menu()
                        .map(|menu| menu.status.clone())
                        .unwrap_or_default();
                    return Err(format!("build rate {rate}: {status}"));
                }

                format!("{rate}x")
            }
            BuildCommand::Repeat => {
                if !app.build_menu_toggle_repeat() {
                    return Err("build repeat: повтор только у завода".to_string());
                }

                "repeat переключён".to_string()
            }
            BuildCommand::Step(delta) => {
                for _ in 0..delta.unsigned_abs() {
                    app.build_menu_step(delta > 0);
                }

                format!("шаг {delta}")
            }
            BuildCommand::QueueStep(delta) => {
                for _ in 0..delta.unsigned_abs() {
                    app.build_menu_queue_step(delta > 0);
                }

                format!("очередь, шаг {delta}")
            }
            BuildCommand::Done => {
                if app.build_menu().is_none() {
                    return Err("build done: меню не открыто".to_string());
                }

                let line = app.commit_build_menu(false);

                if app.build_menu().is_some() {
                    return Err(format!(
                        "build done: заказ не принят ({})",
                        line.unwrap_or_default()
                    ));
                }

                line.unwrap_or_else(|| "заказ принят".to_string())
            }
            BuildCommand::Cancel => {
                if app.build_menu().is_none() && app.build_site().is_none() {
                    return Err("build cancel: меню не открыто".to_string());
                }

                if app.build_menu().is_some() {
                    app.close_build_menu();
                    "меню закрыто".to_string()
                } else {
                    app.cancel_build_site()
                        .unwrap_or_else(|| "выбор места отменён".to_string())
                }
            }
            BuildCommand::Place(x, y) => {
                return self.build_place(Some(Point::new(x, y)));
            }
            BuildCommand::PlaceNear => {
                return self.build_place(None);
            }
            BuildCommand::Cursor(x, y) => {
                return self.build_cursor(Point::new(x, y));
            }
        };

        self.detail = Some(detail);

        Ok(Flow::Continue)
    }

    /// `build cursor X Y` — навести мышь на клетку без клика: призрак-лента
    /// переезжает (`GameManager_GetBuilderUnitCursor`), заказ не отдаётся.
    fn build_cursor(&mut self, grid: Point) -> Result<Flow, String> {
        if self.app.build_site().is_none() {
            return Err("build cursor: режим выбора места не открыт".to_string());
        }

        self.app.viewport_mut().center_on_grid(grid.x, grid.y);

        let (screen_x, screen_y) = hud::tile_center(&self.app.map_viewport(), grid);
        let (frame_x, frame_y) = (
            screen_x + layout::MAIN_MAP.ulx,
            screen_y + layout::MAIN_MAP.uly,
        );

        if !layout::MAIN_MAP.contains(frame_x, frame_y) {
            return Err(format!(
                "build cursor {},{}: клетка вне видимой области карты",
                grid.x, grid.y
            ));
        }

        self.send(InputEvent::MouseMove(frame_x, frame_y))?;

        let (anchor, valid) = self
            .app
            .build_site()
            .map(|site| (site.anchor, site.valid(self.app.state())))
            .ok_or_else(|| "build cursor: режим закрылся".to_string())?;

        self.detail = Some(format!(
            "призрак ({}, {}), {}",
            anchor.x,
            anchor.y,
            if valid {
                "валидно"
            } else {
                "невалидно"
            }
        ));

        Ok(Flow::Continue)
    }

    /// `build place X Y|near` — подтверждение места стройки левым кликом по
    /// карте (C++ `GameManager_PathBuild`).
    ///
    /// Клик идёт тем же путём, что у игрока: камера наводится на клетку, затем
    /// `MouseClick` в её центр, поэтому проверяется и попадание клика в клетку,
    /// и весь путь ввода.
    fn build_place(&mut self, target: Option<Point>) -> Result<Flow, String> {
        let app = &mut self.app;

        let Some(site) = app.build_site() else {
            return Err(
                "build place: режим выбора места не открыт (здание у мобильного строителя: build done)"
                    .to_string(),
            );
        };
        let builder = site.builder;

        let target = match target {
            Some(target) => target,
            None => app
                .state()
                .unit(builder)
                .map(|unit| unit.grid)
                .ok_or_else(|| "build place near: строитель исчез".to_string())?,
        };

        app.viewport_mut().center_on_grid(target.x, target.y);

        let (screen_x, screen_y) = hud::tile_center(&app.map_viewport(), target);
        let (frame_x, frame_y) = (
            screen_x + layout::MAIN_MAP.ulx,
            screen_y + layout::MAIN_MAP.uly,
        );

        if !layout::MAIN_MAP.contains(frame_x, frame_y) {
            return Err(format!(
                "build place {},{}: клетка вне видимой области карты",
                target.x, target.y
            ));
        }

        self.send(InputEvent::MouseClick(frame_x, frame_y))?;

        if self.app.build_site().is_some() {
            return Err(format!(
                "build place {},{}: клик не подтвердил площадку",
                target.x, target.y
            ));
        }

        let tape = self
            .app
            .state()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| self.app.state().unit(id))
            .filter(|unit| {
                unit.team == self.app.state().player_team
                    && matches!(
                        unit.unit_type,
                        max_core::enums::UnitType::LargeTape | max_core::enums::UnitType::SmallTape
                    )
                    && unit.parent_unit == Some(builder)
            })
            .map(|unit| unit.grid)
            .next();

        self.detail = Some(match tape {
            Some(site) => format!("площадка ({}, {})", site.x, site.y),
            None => "площадка ещё не поставлена (тик симуляции?)".to_string(),
        });

        Ok(Flow::Continue)
    }
    /// `upgrade …`: экран покупных апгрейдов (P6).
    fn upgrade(&mut self, action: UpgradeCommand) -> Result<Flow, String> {
        let app = &mut self.app;

        let detail = match action {
            UpgradeCommand::Open => {
                if !app.open_upgrade_menu() {
                    return Err(
                        "upgrade open: нужен выделенный юнит своей команды (select type …)"
                            .to_string(),
                    );
                }
                let count = app
                    .upgrade_menu()
                    .map(|menu| menu.unit_types.len())
                    .unwrap_or(0);
                format!("меню открыто, типов {count}")
            }
            UpgradeCommand::Select(index) => {
                let Some(menu) = app.upgrade_menu() else {
                    return Err("upgrade select: меню не открыто".to_string());
                };
                if index as usize >= menu.unit_types.len() {
                    return Err(format!(
                        "upgrade select {index}: в списке {} типов",
                        menu.unit_types.len()
                    ));
                }
                let name = menu
                    .unit_types
                    .get(index as usize)
                    .copied()
                    .map(crate::buildmenu::unit_name)
                    .unwrap_or("unit");
                app.upgrade_menu_select(index as usize);
                format!("выбран {name}")
            }
            UpgradeCommand::Adjust { row, increase } => {
                if app.upgrade_menu().is_none() {
                    return Err("upgrade row: меню не открыто".to_string());
                }
                if !app.upgrade_menu_adjust(row as usize, increase) {
                    return Err(format!("upgrade row {row}: изменение невозможно"));
                }
                format!("строка {row} {}", if increase { "up" } else { "down" })
            }
            UpgradeCommand::Done => {
                if app.upgrade_menu().is_none() {
                    return Err("upgrade done: меню не открыто".to_string());
                }
                app.commit_upgrade_menu(false)
                    .unwrap_or_else(|| "изменения зафиксированы".to_string())
            }
            UpgradeCommand::Cancel => {
                if app.upgrade_menu().is_none() {
                    return Err("upgrade cancel: меню не открыто".to_string());
                }
                app.close_upgrade_menu();
                "меню закрыто".to_string()
            }
        };

        self.detail = Some(detail);

        Ok(Flow::Continue)
    }

    /// `research …`: экран исследований (P7, `ResearchMenu`).
    fn research(&mut self, action: ResearchCommand) -> Result<Flow, String> {
        let app = &mut self.app;

        let detail = match action {
            ResearchCommand::Open => {
                if !app.open_research_menu() {
                    return Err(
                        "research open: нужен выделенный включённый Research Center своей команды (select type …, key O)"
                            .to_string(),
                    );
                }
                let (centers, free) = app
                    .research_menu()
                    .map(|menu| (menu.active_centers, menu.free_capacity))
                    .unwrap_or((0, 0));
                format!("меню открыто, центров {centers}, свободно {free}")
            }
            ResearchCommand::Select(index) => {
                let Some(_menu) = app.research_menu() else {
                    return Err("research select: меню не открыто".to_string());
                };
                if index as usize >= max_core::enums::RESEARCH_TOPIC_COUNT {
                    return Err(format!(
                        "research select {index}: тем всего {}",
                        max_core::enums::RESEARCH_TOPIC_COUNT
                    ));
                }
                app.research_menu_select(index as usize);
                format!(
                    "выбрана тема {}",
                    crate::researchmenu::TOPIC_LABELS[index as usize]
                )
            }
            ResearchCommand::Row { row, increase } => {
                if app.research_menu().is_none() {
                    return Err("research row: меню не открыто".to_string());
                }
                if row as usize >= max_core::enums::RESEARCH_TOPIC_COUNT {
                    return Err(format!(
                        "research row {row}: тем всего {}",
                        max_core::enums::RESEARCH_TOPIC_COUNT
                    ));
                }
                if !app.research_menu_adjust(row as usize, increase) {
                    return Err(format!(
                        "research row {row}: {} центров (свободно {}) — изменение невозможно",
                        app.research_menu()
                            .map(|menu| menu.value(row as usize))
                            .unwrap_or(0),
                        app.research_menu()
                            .map(|menu| menu.free_capacity)
                            .unwrap_or(0)
                    ));
                }
                let menu = app.research_menu().expect("menu");
                format!(
                    "{}: {} центров, свободно {}",
                    crate::researchmenu::TOPIC_LABELS[row as usize],
                    menu.value(row as usize),
                    menu.free_capacity
                )
            }
            ResearchCommand::Done => {
                if app.research_menu().is_none() {
                    return Err("research done: меню не открыто".to_string());
                }
                app.commit_research_menu()
                    .unwrap_or_else(|| "распределение применено".to_string())
            }
            ResearchCommand::Cancel => {
                if app.research_menu().is_none() {
                    return Err("research cancel: меню не открыто".to_string());
                }
                app.close_research_menu();
                "меню закрыто без изменений".to_string()
            }
        };

        self.detail = Some(detail);

        Ok(Flow::Continue)
    }

    /// `help …`: окно помощи (`HelpMenu`) — четыре экрана меню/мобилизации,
    /// `GAME_SCREEN_SETUP`, `RESEARCH_SETUP`, `UPGRADES_SETUP`.
    fn help(&mut self, action: HelpCommand) -> Result<Flow, String> {
        let detail = match action {
            HelpCommand::Open(section) => {
                if self.current_help_section().is_some() {
                    return Err(format!(
                        "help {}: окно уже открыто ({:?})",
                        section.as_str(),
                        self.current_help_section()
                    ));
                }

                if matches!(self.screen, Screen::InSession { .. }) {
                    match section {
                        crate::helpmenu::HelpSection::Research => {
                            if self.app.research_menu().is_none() {
                                return Err("help research: сначала research open".to_string());
                            }
                            let rect = crate::researchmenu::HELP_RECT;
                            self.send(InputEvent::MouseClick(
                                crate::researchmenu::WINDOW_RECT.ulx + rect.ulx + rect.width / 2,
                                crate::researchmenu::WINDOW_RECT.uly + rect.uly + rect.height / 2,
                            ))?;
                        }
                        crate::helpmenu::HelpSection::Upgrades => {
                            if self.app.upgrade_menu().is_none() {
                                return Err("help upgrades: сначала upgrade open".to_string());
                            }
                            let rect = crate::upgrademenu::HELP;
                            self.send(InputEvent::MouseClick(
                                rect.ulx + rect.width / 2,
                                rect.uly + rect.height / 2,
                            ))?;
                        }
                        crate::helpmenu::HelpSection::GameScreen => {
                            // Панель `HELP_OF` (`GameManager_MenuClickHelpButton`).
                            // Без хрома панели нет — открываем как `?`.
                            if self.app.chrome_active() {
                                let rect = max_render::layout::HELP_BUTTON;
                                self.send(InputEvent::MouseClick(
                                    rect.ulx + rect.width / 2,
                                    rect.uly + rect.height / 2,
                                ))?;
                            } else {
                                self.app.open_help(section);
                            }
                        }
                        _ => {
                            return Err(format!(
                                "help {}: секция доступна только в меню/мобилизации",
                                section.as_str()
                            ))
                        }
                    }
                } else {
                    match &self.screen {
                        Screen::LocalSetup(form) => {
                            let expected = match form.screen {
                                SetupScreen::Options => crate::helpmenu::HelpSection::Options,
                                SetupScreen::Planet => crate::helpmenu::HelpSection::Planet,
                                SetupScreen::Players if form.hot_seat => {
                                    crate::helpmenu::HelpSection::HotSeat
                                }
                                SetupScreen::Players => crate::helpmenu::HelpSection::NewGame,
                                SetupScreen::Clan => crate::helpmenu::HelpSection::Clan,
                            };
                            if expected != section {
                                return Err(format!(
                                    "help {}: текущий экран — {:?} ({expected:?})",
                                    section.as_str(),
                                    form.screen
                                ));
                            }
                            // `?` в нижней строке всех экранов настройки.
                            self.send(InputEvent::MouseClick(489, 458))?;
                        }
                        Screen::PreGame(flow) => {
                            if !matches!(flow.stage, crate::pregame::PreGameStage::Mobilize(_))
                                || section != crate::helpmenu::HelpSection::Cargo
                            {
                                return Err(format!(
                                    "help {}: доступно только на мобилизации (CargoMenu)",
                                    section.as_str()
                                ));
                            }
                            let rect = crate::mobilize::HELP_BUTTON;
                            self.send(InputEvent::MouseClick(
                                rect.ulx + rect.width / 2,
                                rect.uly + rect.height / 2,
                            ))?;
                        }
                        _ => {
                            return Err(format!(
                                "help {}: текущий экран {} не открывает помощь",
                                section.as_str(),
                                self.screen_label()
                            ))
                        }
                    }
                }

                if self.current_help_section() != Some(section) {
                    return Err(format!(
                        "help {}: окно не открылось (сейчас {:?})",
                        section.as_str(),
                        self.current_help_section()
                    ));
                }

                format!(
                    "{}: строк {}, страница {}",
                    section.as_str(),
                    self.current_help_lines(),
                    crate::helpmenu::ROWS_PER_PAGE
                )
            }
            HelpCommand::PageUp => {
                self.require_help()?;
                self.send(InputEvent::Key(Key::PageUp))?;
                format!("help row {}", self.current_help_row())
            }
            HelpCommand::PageDown => {
                self.require_help()?;
                self.send(InputEvent::Key(Key::PageDown))?;
                format!("help row {}", self.current_help_row())
            }
            HelpCommand::Up => {
                self.require_help()?;
                self.send(InputEvent::Key(Key::Up))?;
                format!("help row {}", self.current_help_row())
            }
            HelpCommand::Down => {
                self.require_help()?;
                self.send(InputEvent::Key(Key::Down))?;
                format!("help row {}", self.current_help_row())
            }
            HelpCommand::Keys => {
                self.require_help()?;
                self.send(InputEvent::Key(Key::Char('k')))?;
                let mode = self
                    .app
                    .help_screen()
                    .map(|help| help.keys_mode())
                    .unwrap_or(false);
                format!("keys mode {}", if mode { "on" } else { "off" })
            }
            HelpCommand::Done => {
                self.require_help()?;
                self.send(InputEvent::Key(Key::Enter))?;
                if self.current_help_section().is_some() {
                    return Err("help done: окно не закрылось".to_string());
                }
                "окно закрыто".to_string()
            }
            HelpCommand::Close => {
                self.require_help()?;
                self.send(InputEvent::Key(Key::Escape))?;
                if self.current_help_section().is_some() {
                    return Err("help close: окно не закрылось".to_string());
                }
                "окно закрыто".to_string()
            }
        };

        self.detail = Some(detail);

        Ok(Flow::Continue)
    }

    /// Секция открытого окна помощи (в игре — `App::help_screen`, в меню —
    /// `Screen::Help`).
    fn current_help_section(&self) -> Option<crate::helpmenu::HelpSection> {
        if let Some(help) = self.app.help_screen() {
            return Some(help.section());
        }
        match &self.screen {
            Screen::Help { help, .. } => Some(help.section()),
            _ => None,
        }
    }

    fn current_help_row(&self) -> usize {
        self.app
            .help_screen()
            .map(|help| help.row_index())
            .or_else(|| match &self.screen {
                Screen::Help { help, .. } => Some(help.row_index()),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn current_help_lines(&self) -> usize {
        self.app
            .help_screen()
            .map(|help| help.line_count())
            .or_else(|| match &self.screen {
                Screen::Help { help, .. } => Some(help.line_count()),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn require_help(&self) -> Result<(), String> {
        if self.current_help_section().is_some() {
            Ok(())
        } else {
            Err("help: окно не открыто".to_string())
        }
    }
    fn mobilize(&mut self, action: MobilizeAction) -> Result<Flow, String> {
        match action {
            MobilizeAction::Buy => self.send(InputEvent::MouseClick(
                BUY_BUTTON.ulx + BUY_BUTTON.width / 2,
                BUY_BUTTON.uly + BUY_BUTTON.height / 2,
            )),
            MobilizeAction::BuyIndex(index) => {
                let index = index as usize;

                if index >= PURCHASE_UNITS.len() {
                    return Err(format!(
                        "mobilize buy {index}: в списке {} юнитов",
                        PURCHASE_UNITS.len()
                    ));
                }

                // Повторный клик по выбранной строке в C++ — `BuyUnit`, поэтому
                // если строка уже выбрана, сразу жмём `Buy` (иначе купили бы
                // дважды).
                if !self.type_row_selected(index) {
                    self.click_type_row(index)?;
                }

                self.send(InputEvent::MouseClick(
                    BUY_BUTTON.ulx + BUY_BUTTON.width / 2,
                    BUY_BUTTON.uly + BUY_BUTTON.height / 2,
                ))
            }
            MobilizeAction::Delete => self.send(InputEvent::MouseClick(
                DELETE_BUTTON.ulx + DELETE_BUTTON.width / 2,
                DELETE_BUTTON.uly + DELETE_BUTTON.height / 2,
            )),
            MobilizeAction::Step(delta) => {
                let key = if delta < 0 { Key::Up } else { Key::Down };

                for _ in 0..delta.unsigned_abs() {
                    self.send(InputEvent::Key(key))?;
                }

                Ok(Flow::Continue)
            }
            MobilizeAction::Mode(buy) => {
                let current = self
                    .mobilization()
                    .map(|m| m.mode == crate::mobilize::CargoMode::Buy);

                if current == Some(buy) {
                    return Ok(Flow::Continue);
                }

                self.send(InputEvent::MouseClick(
                    TOGGLE_BUTTON.ulx + TOGGLE_BUTTON.width / 2,
                    TOGGLE_BUTTON.uly + TOGGLE_BUTTON.height / 2,
                ))
            }
            MobilizeAction::Filter(name) => {
                let filter = match name {
                    script::FilterName::Ground => UpgradeFilter::Ground,
                    script::FilterName::Air => UpgradeFilter::Air,
                    script::FilterName::Sea => UpgradeFilter::Sea,
                    script::FilterName::Building => UpgradeFilter::Building,
                    script::FilterName::Combat => UpgradeFilter::Combat,
                };
                let rect = filter.rect();

                self.send(InputEvent::MouseClick(
                    rect.ulx + rect.width / 2,
                    rect.uly + rect.height / 2,
                ))
            }
            MobilizeAction::Select(index) => {
                if self.type_row_selected(index as usize) {
                    Ok(Flow::Continue)
                } else {
                    self.click_type_row(index as usize)
                }
            }
            MobilizeAction::Loadout(index) => {
                if self.loadout_row_selected(index as usize) {
                    Ok(Flow::Continue)
                } else {
                    self.click_loadout_row(index as usize)
                }
            }
            MobilizeAction::Adjust { row, increase } => {
                let point = self
                    .mobilization()
                    .and_then(|mobilization| mobilization.control_arrows(row as usize))
                    .ok_or_else(|| format!("mobilize row {row}: нет такой строки апгрейда"))?;
                let rect = if increase { point.1 } else { point.0 };

                self.send(InputEvent::MouseClick(
                    rect.ulx + rect.width / 2,
                    rect.uly + rect.height / 2,
                ))
            }
            MobilizeAction::Cargo(value) => {
                let mobilization = self
                    .mobilization()
                    .ok_or_else(|| "mobilize cargo: экрана мобилизации нет".to_string())?;
                let capacity = mobilization.cargo_capacity();
                if capacity <= 0 {
                    return Err("mobilize cargo: у выбранного юнита нет ёмкости".to_string());
                }
                let value = value as i32;
                let mut point = None;
                for y in CARGO_BAR.uly..CARGO_BAR.lry() {
                    let from_bottom = CARGO_BAR.lry() - y;
                    let slider = (CARGO_BAR.height / 2 + capacity * from_bottom) / CARGO_BAR.height;
                    if slider == value {
                        point = Some(y);
                        break;
                    }
                }
                let y = point.ok_or_else(|| {
                    format!("mobilize cargo {value}: значение вне диапазона слайдера")
                })?;

                self.send(InputEvent::MouseClick(CARGO_BAR.ulx + 2, y))
            }
            MobilizeAction::Done => self.send(InputEvent::MouseClick(
                MOBILIZE_DONE.ulx + MOBILIZE_DONE.width / 2,
                MOBILIZE_DONE.uly + MOBILIZE_DONE.height / 2,
            )),
            MobilizeAction::Cancel => self.send(InputEvent::MouseClick(
                MOBILIZE_CANCEL.ulx + MOBILIZE_CANCEL.width / 2,
                MOBILIZE_CANCEL.uly + MOBILIZE_CANCEL.height / 2,
            )),
        }
    }

    /// Клик по строке `index` списка типов (с учётом прокрутки: если строка не
    /// видна, скрипт нажимает стрелки списка, как игрок).
    fn click_type_row(&mut self, index: usize) -> Result<Flow, String> {
        let mut guard = 0;
        loop {
            let page = self
                .mobilization()
                .ok_or_else(|| "mobilize: экрана мобилизации нет".to_string())?
                .type_page;
            let rows = crate::mobilize::AVAILABLE_ROWS;

            if index < page {
                self.send(InputEvent::MouseClick(
                    crate::mobilize::LIST_UP.ulx + 4,
                    crate::mobilize::LIST_UP.uly + 4,
                ))?;
            } else if index >= page + rows {
                self.send(InputEvent::MouseClick(
                    crate::mobilize::LIST_DOWN.ulx + 4,
                    crate::mobilize::LIST_DOWN.uly + 4,
                ))?;
            } else {
                let row = index - page;
                return self.send(InputEvent::MouseClick(
                    AVAILABLE_AREA.ulx + 4,
                    AVAILABLE_AREA.uly + LIST_ROW * row as i32 + 4,
                ));
            }

            guard += 1;
            if guard > 64 {
                return Err(format!("mobilize select {index}: список не прокрутился"));
            }
        }
    }

    /// Клик по строке `index` списка «Purchased».
    fn click_loadout_row(&mut self, index: usize) -> Result<Flow, String> {
        let mut guard = 0;
        loop {
            let page = self
                .mobilization()
                .ok_or_else(|| "mobilize: экрана мобилизации нет".to_string())?
                .loadout_page;
            let rows = crate::mobilize::PURCHASED_ROWS;

            if index < page {
                self.send(InputEvent::MouseClick(
                    crate::mobilize::PURCHASE_UP.ulx + 4,
                    crate::mobilize::PURCHASE_UP.uly + 4,
                ))?;
            } else if index >= page + rows {
                self.send(InputEvent::MouseClick(
                    crate::mobilize::PURCHASE_DOWN.ulx + 4,
                    crate::mobilize::PURCHASE_DOWN.uly + 4,
                ))?;
            } else {
                let row = index - page;
                return self.send(InputEvent::MouseClick(
                    PURCHASED_AREA.ulx + 4,
                    PURCHASED_AREA.uly + LIST_ROW * row as i32 + 4,
                ));
            }

            guard += 1;
            if guard > 64 {
                return Err(format!("mobilize loadout {index}: список не прокрутился"));
            }
        }
    }

    /// Выбрана ли строка `index` списка типов и совпадает ли с панелью информации.
    fn type_row_selected(&self, index: usize) -> bool {
        self.mobilization().is_some_and(|mobilization| {
            mobilization.type_selected == index
                && mobilization.info == mobilization.types.get(index).copied()
        })
    }

    /// Выбрана ли строка `index` списка «Purchased».
    fn loadout_row_selected(&self, index: usize) -> bool {
        self.mobilization().is_some_and(|mobilization| {
            mobilization.loadout_selected == index
                && mobilization.info
                    == mobilization
                        .loadout_row(index)
                        .map(|supply| supply.unit_type)
        })
    }

    /// `landing …`: выбор точки высадки через клики по кадру и `Enter`/`Esc`.
    fn landing(&mut self, action: LandingCommand) -> Result<Flow, String> {
        match action {
            LandingCommand::Click(x, y) => self.send(InputEvent::MouseClick(x, y)),
            LandingCommand::Grid(grid_x, grid_y) => {
                // Детерминированно ставим точку без клика: в C++ клик по
                // валидной клетке сразу подтверждает выбор, а сценарию нужно
                // снять кадр с курсором до подтверждения.
                if let Screen::PreGame(flow) = &mut self.screen {
                    if let crate::pregame::PreGameStage::Landing(landing) = &mut flow.stage {
                        landing.cursor = max_core::geo::Point::new(grid_x, grid_y);
                        landing.status.clear();
                    }
                }

                // Курсор C++ (`Cursor_RenderToBuffer`) попадает в кадр: ставим
                // его в центр выбранной клетки, как реальная мышь.
                self.ctx.mouse = Some(max_render::ui::Point::new(
                    crate::landing::LANDING_AREA.ulx
                        + i32::from(grid_x) * crate::landing::LANDING_CELL
                        + crate::landing::LANDING_CELL / 2,
                    crate::landing::LANDING_AREA.uly
                        + i32::from(grid_y) * crate::landing::LANDING_CELL
                        + crate::landing::LANDING_CELL / 2,
                ));

                Ok(Flow::Continue)
            }
            LandingCommand::Done => self.send(InputEvent::Key(Key::Enter)),
            LandingCommand::Cancel => self.send(InputEvent::Key(Key::Escape)),
        }
    }

    // -----------------------------------------------------------------------
    // Ввод
    // -----------------------------------------------------------------------

    /// Отправляет событие в текущий экран и исполняет переход.
    fn send(&mut self, event: InputEvent) -> Result<Flow, String> {
        if matches!(self.screen, Screen::InSession { .. }) {
            return match handle_session_input(&mut self.app, event, false) {
                Transition::None => Ok(Flow::Continue),
                Transition::LeaveSession => {
                    self.screen = Screen::MainMenu;
                    self.app.leave_local_game();
                    self.ctx.status = "Left the session".to_string();
                    Ok(Flow::Continue)
                }
                Transition::Quit => Ok(Flow::Exit),
                Transition::ApplyPreferences(preferences) => {
                    self.app.set_preferences(*preferences);
                    self.app.persist_preferences();
                    self.ctx.preferences = self.app.preferences().clone();
                    Ok(Flow::Continue)
                }
                Transition::StartLocal(_) => {
                    Err("ввод в сессии не может начать локальную партию".to_string())
                }
                Transition::StartSession(request) => Err(format!(
                    "сетевая сессия «{}» в QA-драйвере не поддерживается",
                    request.game_id
                )),
            };
        }

        match self.screen.handle_input(&mut self.ctx, event) {
            Transition::None => Ok(Flow::Continue),
            Transition::StartLocal(setup) => {
                self.app
                    .start_local_game(&setup)
                    .map_err(|error| format!("start_local_game: {error}"))?;
                self.screen = Screen::InSession {
                    request: SessionRequest {
                        game_id: "local".to_string(),
                        password: None,
                    },
                };
                self.ctx.status = String::new();
                Ok(Flow::Continue)
            }
            Transition::LeaveSession => {
                self.screen = Screen::MainMenu;
                self.app.leave_local_game();
                Ok(Flow::Continue)
            }
            Transition::Quit => Ok(Flow::Exit),
            Transition::ApplyPreferences(preferences) => {
                self.app.set_preferences(*preferences);
                self.app.persist_preferences();
                self.ctx.preferences = self.app.preferences().clone();
                Ok(Flow::Continue)
            }
            Transition::StartSession(request) => Err(format!(
                "сетевая сессия «{}» в QA-драйвере не поддерживается",
                request.game_id
            )),
        }
    }

    /// Один кадр игрового цикла — как блок `main.rs` после обработки ввода.
    fn frame(&mut self) {
        // Анимация экранов меню (`Credits`): фиксированный `dt = 1/60`, поэтому
        // кадр — чистая функция от числа шагов сценария. Когда прокрутка
        // доходит до верха области, оригинал выходит из цикла — возвращаемся
        // в главное меню.
        if self.screen.advance(1.0 / 60.0) {
            let was_intro = matches!(self.screen, Screen::Introduction(_));

            self.screen = Screen::MainMenu;
            self.ctx.status = if was_intro {
                "Introduction ended".to_string()
            } else {
                "Credits ended".to_string()
            };
        }

        if matches!(self.screen, Screen::InSession { .. }) && self.app.local_active() {
            let update = self.app.local_update(0.0);

            if let Some(local) = self.app.local() {
                self.ctx.status = local.status_line(self.app.state().turn_counter);
            }

            if update.finished || self.app.local().is_some_and(|local| local.finished()) {
                self.screen = Screen::GameOver;
                self.ctx.status = "Game over — Enter/Esc returns to the menu".to_string();
            }
        }

        self.prepare();
        self.frames += 1;
    }

    /// Подготовка кадра к отрисовке (мини-карты планет грузятся один раз).
    fn prepare(&mut self) {
        // Портрет `CORNER_FLIC`: синхронизация с выделением без продвижения
        // анимации (`dt = 0`) — кадр `shot` не зависит от числа кадров до него.
        self.app.update_portrait(0.0);

        let palette = self
            .app
            .assets()
            .and_then(|assets| assets.menu.as_ref())
            .map(|menu| menu.palette);

        // Окно помощи может быть открыто поверх экрана настройки: мини-карты
        // планет всё равно должны быть готовы к кадру.
        let screen = match &mut self.screen {
            Screen::Help { screen, .. } => screen.as_mut(),
            screen => screen,
        };

        if let Screen::LocalSetup(form) = screen {
            form.ensure_minimaps(palette.as_ref());
        }
    }

    /// Игра готова принять ввод (или уже окончена / показывает окно перехода).
    fn ready(&self) -> bool {
        match self.app.local() {
            None => true,
            Some(local) => local.finished() || local.notice.is_some() || !local.input_blocked(),
        }
    }

    /// `wait N`: до N кадров, пока игра не будет готова к вводу.
    fn wait(&mut self, limit: usize) -> Result<Flow, String> {
        let mut frames = 0usize;

        while !self.ready() {
            if frames >= limit {
                return Err(format!(
                    "wait: игра не пришла в готовность за {limit} кадров (phase={}, notice={})",
                    self.phase_label(),
                    self.notice_label()
                ));
            }

            self.frame();
            frames += 1;
        }

        self.detail = Some(format!("{frames} кадров"));
        Ok(Flow::Continue)
    }

    /// `mem`: текущий RSS процесса в лог шага (P4 soak, TC-1108).
    fn mem(&mut self) -> Result<Flow, String> {
        self.detail = Some(crate::mem::memory_line());

        Ok(Flow::Continue)
    }

    /// `anim N`: прокрутить визуальные часы на `N` мс реального `dt`.
    ///
    /// Симуляция не тикает (`GameState::tick` не вызывается), поэтому кадры
    /// анимации детерминированы: `anim` идёт фиксированными шагами 1/60 с
    /// ровно как `main.rs` в интерактиве. `dt = 0` у `tick`/`wait` наоборот
    /// даёт статичный кадр (рабочая поза `+16`, полный размер постройки).
    fn anim(&mut self, millis: u32) -> Result<Flow, String> {
        let total = f64::from(millis) / 1000.0;
        let step = 1.0f64 / 60.0;
        let mut elapsed = 0.0f64;

        while elapsed + f64::EPSILON < total {
            let dt = step.min(total - elapsed).max(f64::EPSILON) as f32;
            self.app.update_visual_units(dt);
            elapsed += f64::from(dt);
        }

        self.detail = Some(format!("{millis} мс визуального времени"));
        Ok(Flow::Continue)
    }

    // -----------------------------------------------------------------------
    // Кадры
    // -----------------------------------------------------------------------

    fn shot(&mut self, name: &str) -> Result<Flow, String> {
        self.prepare();
        self.screen
            .render(&mut self.buffer, self.ctx.view(Some(&self.app)));

        let (width, height) = (self.buffer.width(), self.buffer.height());

        if (width, height) != (layout::WINDOW_WIDTH as u32, layout::WINDOW_HEIGHT as u32) {
            return Err(format!(
                "shot: кадр {width}×{height} вместо {}×{}",
                layout::WINDOW_WIDTH,
                layout::WINDOW_HEIGHT
            ));
        }

        let image = Image::from_rgba(width, height, self.buffer.pixels());
        let path = self.config.out_dir.join(format!("{name}.png"));
        png::write(&path, &image)?;

        self.shots.push(path.clone());
        self.detail = Some(format!("{}", path.display()));

        Ok(Flow::Continue)
    }

    // -----------------------------------------------------------------------
    // Высокоуровневые команды
    // -----------------------------------------------------------------------

    fn menu(&mut self, target: MenuTarget) -> Result<Flow, String> {
        match target {
            MenuTarget::NewGame => {
                if !matches!(self.screen, Screen::MainMenu) {
                    return Err(format!(
                        "menu new_game: нужен MainMenu, сейчас {}",
                        self.screen_label()
                    ));
                }

                let rect = menu::MAIN_MENU_BUTTONS[0].rect;
                self.send(InputEvent::MouseClick(
                    rect.ulx + rect.width / 2,
                    rect.uly + rect.height / 2,
                ))?;
                self.require_screen(ScreenName::LocalSetup, "menu new_game")
            }
            MenuTarget::Multiplayer => {
                self.enter_multiplayer_menu()?;
                self.require_screen(ScreenName::MultiplayerMenu, "menu multiplayer")
            }
            MenuTarget::HotSeat => {
                self.enter_multiplayer_menu()?;

                let rect = menu::MULTIPLAYER_BUTTONS[2].rect;
                self.send(InputEvent::MouseClick(
                    rect.ulx + rect.width / 2,
                    rect.uly + rect.height / 2,
                ))?;

                let hot_seat = matches!(&self.screen, Screen::LocalSetup(form) if form.hot_seat);

                if hot_seat {
                    self.detail = Some("Hot Seat Menu".to_string());
                    Ok(Flow::Continue)
                } else {
                    Err(format!(
                        "menu hot_seat: открылся {} вместо настройки hot-seat",
                        self.screen_label()
                    ))
                }
            }
            MenuTarget::Join => {
                self.enter_multiplayer_menu()?;

                let rect = menu::MULTIPLAYER_BUTTONS[1].rect;
                self.send(InputEvent::MouseClick(
                    rect.ulx + rect.width / 2,
                    rect.uly + rect.height / 2,
                ))?;
                self.require_screen(ScreenName::JoinList, "menu join")
            }
            MenuTarget::Introduction => {
                self.click_main_menu_button(4, "menu intro")?;
                self.require_screen(ScreenName::Introduction, "menu intro")
            }
            MenuTarget::Credits => {
                self.click_main_menu_button(5, "menu credits")?;
                self.require_screen(ScreenName::Credits, "menu credits")
            }
            MenuTarget::Setup => {
                self.click_main_menu_button(3, "menu setup")?;
                self.require_screen(ScreenName::Preferences, "menu setup")
            }
        }
    }

    /// Клик по кнопке главного меню `index` (0..=6) с проверкой, что экран
    /// действительно сменился (для `menu intro`/`menu credits`).
    fn click_main_menu_button(&mut self, index: usize, what: &str) -> Result<(), String> {
        if !matches!(self.screen, Screen::MainMenu) {
            return Err(format!(
                "{what}: нужен MainMenu, сейчас {}",
                self.screen_label()
            ));
        }

        let Some(button) = menu::MAIN_MENU_BUTTONS.get(index) else {
            return Err(format!("{what}: нет кнопки {index}"));
        };
        let rect = button.rect;

        self.send(InputEvent::MouseClick(
            rect.ulx + rect.width / 2,
            rect.uly + rect.height / 2,
        ))?;

        Ok(())
    }

    fn enter_multiplayer_menu(&mut self) -> Result<(), String> {
        match &self.screen {
            Screen::MultiplayerMenu => Ok(()),
            Screen::MainMenu => {
                let rect = menu::MAIN_MENU_BUTTONS[2].rect;
                self.send(InputEvent::MouseClick(
                    rect.ulx + rect.width / 2,
                    rect.uly + rect.height / 2,
                ))?;

                if matches!(self.screen, Screen::MultiplayerMenu) {
                    Ok(())
                } else {
                    Err(format!(
                        "menu: ожидался MultiplayerMenu, сейчас {}",
                        self.screen_label()
                    ))
                }
            }
            _ => Err(format!(
                "menu: перейти в MultiplayerMenu можно только из MainMenu (сейчас {})",
                self.screen_label()
            )),
        }
    }

    fn setup(&mut self, action: SetupAction) -> Result<Flow, String> {
        let Some(current) = self.setup_screen() else {
            return Err(format!(
                "setup: нужен экран настройки, сейчас {}",
                self.screen_label()
            ));
        };

        let (x, y, what) = match action {
            SetupAction::Done => (514 + 2, 438 + 2, "Done"),
            SetupAction::Cancel => (354 + 2, 438 + 2, "Cancel"),
            SetupAction::Random => {
                if current != SetupScreen::Planet {
                    return Err(format!(
                        "setup random: кнопка Random есть только на экране планет (сейчас {:?})",
                        current
                    ));
                }
                (243 + 2, 438 + 2, "Random")
            }
        };

        self.send(InputEvent::MouseClick(x, y))?;

        let after = self.setup_screen();

        match (action, after) {
            (_, None) => {
                self.detail = Some(format!("{what} → {}", self.screen_label()));
                Ok(Flow::Continue)
            }
            (_, Some(screen)) if screen != current => {
                self.detail = Some(format!("{what} → {:?}", screen));
                Ok(Flow::Continue)
            }
            _ => {
                let status = match &self.screen {
                    Screen::LocalSetup(form) => form.status.clone(),
                    _ => String::new(),
                };

                Err(format!(
                    "setup {what}: экран не сменился (статус: «{status}»)"
                ))
            }
        }
    }

    fn players(&mut self, slot: u8, kind: TeamKind) -> Result<Flow, String> {
        if self.setup_screen() != Some(SetupScreen::Players) {
            return Err(format!(
                "players: нужен экран Players, сейчас {}",
                self.screen_label()
            ));
        }

        let column = match kind {
            TeamKind::Player => 0,
            TeamKind::Computer => 1,
            _ => 2,
        };
        let rect = setup::player_type_button_rect(slot as usize, column);

        self.send(InputEvent::MouseClick(
            rect.ulx + rect.width / 2,
            rect.uly + rect.height / 2,
        ))?;

        let actual = match &self.screen {
            Screen::LocalSetup(form) => form.setup.team_types.get(slot as usize).copied(),
            _ => None,
        };

        match actual {
            Some(actual) if actual == kind => {
                self.detail = Some(format!("слот {slot}: {kind:?}"));
                Ok(Flow::Continue)
            }
            Some(actual) => Err(format!(
                "players {slot} {kind:?}: в слоте {actual:?} (кнопку перекрыло правило режима?)"
            )),
            None => Err("players: форма настройки закрылась".to_string()),
        }
    }

    fn select_point(&mut self, x: i32, y: i32) -> Result<Flow, String> {
        self.send(InputEvent::MouseClick(x, y))?;
        self.require_selection(&format!("select {x} {y}"))
    }

    fn select_unit(&mut self, index: u32) -> Result<Flow, String> {
        let team = self.app.player_team();
        let ids: Vec<_> = self
            .app
            .state()
            .unit_ids_in_order()
            .into_iter()
            .filter(|id| {
                self.app
                    .state()
                    .unit(*id)
                    .is_some_and(|unit| unit.team == team && hud::unit_can_move(unit))
            })
            .collect();

        let Some(id) = ids.get(index as usize).copied() else {
            return Err(format!(
                "select unit {index}: у команды {team:?} всего {} подвижных юнитов",
                ids.len()
            ));
        };

        let grid = self
            .app
            .state()
            .unit(id)
            .map(|unit| unit.grid)
            .ok_or_else(|| "select unit: юнит исчез".to_string())?;

        // Камера на юните: точка клика всегда внутри области карты, независимо
        // от того, куда игрок увёл камеру раньше.
        self.app.viewport_mut().center_on_grid(grid.x, grid.y);

        let (x, y) = hud::tile_center(&self.app.map_viewport(), grid);

        self.select_point(x + layout::MAIN_MAP.ulx, y + layout::MAIN_MAP.uly)
    }

    /// `select last`: самый новый юнит игрока (в т.ч. стационарный — только что
    /// достроенный завод). Камера центрируется, клик идёт по клетке.
    fn select_last(&mut self) -> Result<Flow, String> {
        let team = self.app.player_team();
        let id = self
            .app
            .state()
            .unit_ids_in_order()
            .into_iter()
            .filter(|id| {
                self.app
                    .state()
                    .unit(*id)
                    .is_some_and(|unit| unit.team == team)
            })
            .next_back()
            .ok_or_else(|| format!("select last: у команды {team:?} нет юнитов"))?;

        let grid = self
            .app
            .state()
            .unit(id)
            .map(|unit| unit.grid)
            .ok_or_else(|| "select last: юнит исчез".to_string())?;

        self.app.viewport_mut().center_on_grid(grid.x, grid.y);

        let (x, y) = hud::tile_center(&self.app.map_viewport(), grid);

        self.select_point(x + layout::MAIN_MAP.ulx, y + layout::MAIN_MAP.uly)
    }

    /// `select type NAME`: юнит игрока с таким именем (`unit_name` из
    /// `BuildMenu`, без учёта регистра). Берётся самый новый подходящий.
    fn select_type(&mut self, name: &str) -> Result<Flow, String> {
        let team = self.app.player_team();
        let wanted = name.trim().to_ascii_lowercase();
        let id = self
            .app
            .state()
            .unit_ids_in_order()
            .into_iter()
            .filter(|id| {
                self.app.state().unit(*id).is_some_and(|unit| {
                    unit.team == team
                        && (crate::buildmenu::unit_name(unit.unit_type).to_ascii_lowercase()
                            == wanted
                            || format!("{:?}", unit.unit_type).to_ascii_lowercase() == wanted)
                })
            })
            .next_back()
            .ok_or_else(|| format!("select type {name}: у команды {team:?} нет такого юнита"))?;

        let grid = self
            .app
            .state()
            .unit(id)
            .map(|unit| unit.grid)
            .ok_or_else(|| "select type: юнит исчез".to_string())?;

        self.app.viewport_mut().center_on_grid(grid.x, grid.y);

        let (x, y) = hud::tile_center(&self.app.map_viewport(), grid);

        self.select_point(x + layout::MAIN_MAP.ulx, y + layout::MAIN_MAP.uly)
    }

    fn require_selection(&mut self, what: &str) -> Result<Flow, String> {
        let team = self.app.player_team();
        let selected = self.app.hud().selected;

        match selected.and_then(|id| self.app.state().unit(id)) {
            Some(unit) if unit.team == team => {
                self.detail = Some(format!(
                    "{:?} в ({}, {}), подвижный: {}",
                    unit.unit_type,
                    unit.grid.x,
                    unit.grid.y,
                    hud::unit_can_move(unit)
                ));
                Ok(Flow::Continue)
            }
            Some(unit) => Err(format!(
                "{what}: выделен чужой юнит команды {:?}",
                unit.team
            )),
            None => {
                let phase = self.app.local().map(|local| local.phase);
                let blocked = self.app.local_input_blocked();
                let frame = self
                    .app
                    .hud()
                    .mouse_position
                    .map(|(x, y)| format!("{x},{y}"))
                    .unwrap_or_default();
                Err(format!(
                    "{what}: в этой точке нет юнита команды {team:?} (выделение пустое; phase={phase:?} blocked={blocked} mouse={frame})"
                ))
            }
        }
    }

    fn path_steps(&self) -> usize {
        self.app
            .hud()
            .preview
            .as_ref()
            .map(|preview| preview.len())
            .unwrap_or(0)
    }

    fn selected_orders(
        &self,
    ) -> Option<(max_core::enums::UnitOrder, max_core::enums::UnitOrderState)> {
        let id = self.app.hud().selected?;

        self.app
            .state()
            .unit(id)
            .map(|unit| (unit.orders, unit.state))
    }

    fn move_point(&mut self, x: i32, y: i32, what: &str) -> Result<Flow, String> {
        let Some(id) = self.app.hud().selected else {
            return Err(format!("{what}: нет выделенного юнита (сначала select)"));
        };

        let before = self.selected_orders();

        self.send(InputEvent::RightClick(x, y))?;

        if matches!(self.screen, Screen::InSession { .. }) && self.app.hud().selected != Some(id) {
            return Err(format!(
                "{what}: правый клик снял выделение вместо приказа ({})",
                self.last_log_line()
            ));
        }

        let after = self.selected_orders();
        let preview = self.app.hud().preview.is_some();

        if before == after && !preview {
            return Err(format!(
                "{what}: приказ не принят ({})",
                self.last_log_line()
            ));
        }

        self.detail = Some(format!(
            "шагов {}, {}",
            self.path_steps(),
            after.map(|o| format!("{:?}", o.0)).unwrap_or_default()
        ));
        Ok(Flow::Continue)
    }

    fn move_grid(&mut self, x: i16, y: i16) -> Result<Flow, String> {
        let target = Point::new(x, y);
        let (screen_x, screen_y) = hud::tile_center(&self.app.map_viewport(), target);
        let (frame_x, frame_y) = (
            screen_x + layout::MAIN_MAP.ulx,
            screen_y + layout::MAIN_MAP.uly,
        );

        if !layout::MAIN_MAP.contains(frame_x, frame_y) {
            return Err(format!(
                "move grid {x} {y}: клетка вне видимой области карты (клик ({frame_x}, {frame_y}))"
            ));
        }

        self.move_point(frame_x, frame_y, &format!("move grid {x} {y}"))
    }

    /// Приказ в соседнюю свободную клетку (как игрок мышью).
    ///
    /// Перебирает соседей выделенного юнита в детерминированном порядке и
    /// оставляет первый, который принял приказ: так скрипт не зависит от
    /// точных координат стартовой позиции на карте.
    fn move_near(&mut self) -> Result<Flow, String> {
        let Some(id) = self.app.hud().selected else {
            return Err("move near: нет выделенного юнита (сначала select)".to_string());
        };

        let grid = self
            .app
            .state()
            .unit(id)
            .map(|unit| unit.grid)
            .ok_or_else(|| "move near: выделенный юнит исчез".to_string())?;

        self.app.viewport_mut().center_on_grid(grid.x, grid.y);

        let mut rejections = Vec::new();

        for (dx, dy) in [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
        ] {
            let target = Point::new(grid.x + dx, grid.y + dy);

            if target.x < 0
                || target.y < 0
                || target.x >= max_core::geo::MAP_WIDTH
                || target.y >= max_core::geo::MAP_HEIGHT
            {
                continue;
            }
            if !self.app.state().units_at(target).is_empty() {
                rejections.push(format!("({}, {}): занято", target.x, target.y));
                continue;
            }

            let before = self.selected_orders();
            let (x, y) = hud::tile_center(&self.app.map_viewport(), target);

            self.send(InputEvent::RightClick(
                x + layout::MAIN_MAP.ulx,
                y + layout::MAIN_MAP.uly,
            ))?;

            let after = self.selected_orders();

            if before != after || self.app.hud().preview.is_some() {
                self.detail = Some(format!(
                    "приказ в ({}, {}), шагов {}",
                    target.x,
                    target.y,
                    self.path_steps()
                ));
                return Ok(Flow::Continue);
            }

            rejections.push(format!(
                "({}, {}): {}",
                target.x,
                target.y,
                self.last_log_line()
            ));
        }

        Err(format!(
            "move near: свободной соседней клетки, принявшей приказ, нет ({})",
            rejections.join("; ")
        ))
    }

    fn end_turn(&mut self) -> Result<Flow, String> {
        if !self.app.local_active() {
            return Err("end_turn: локальная партия не идёт".to_string());
        }

        let state = self.local_state_tuple();

        if !matches!(state.1, Some(LocalPhase::PlayerTurn)) || state.2.is_some() {
            return Err(format!(
                "end_turn: сейчас не ход человека (phase={}, notice={})",
                self.phase_label(),
                self.notice_label()
            ));
        }

        let rect = layout::END_TURN_BUTTON;
        self.send(InputEvent::MouseClick(
            rect.ulx + rect.width / 2,
            rect.uly + rect.height / 2,
        ))?;

        let closed = self.local_state_tuple();

        if closed == state {
            return Err("end_turn: кнопка End Turn не закрыла ход".to_string());
        }

        let before = self.frames;
        self.wait(self.config.frame_limit)?;

        self.detail = Some(format!(
            "ход {} · {} кадров",
            self.app.state().turn_counter,
            self.frames - before
        ));
        Ok(Flow::Continue)
    }

    fn local_state_tuple(&self) -> (i32, Option<LocalPhase>, Option<u8>) {
        (
            self.app.state().turn_counter,
            self.app.local().map(|local| local.phase),
            self.app
                .local()
                .and_then(|local| local.notice.as_ref())
                .map(|notice| notice.team.0),
        )
    }

    // -----------------------------------------------------------------------
    // Сохранения
    // -----------------------------------------------------------------------

    fn save(&mut self, name: &str) -> Result<Flow, String> {
        let path = self.config.out_dir.join(name);
        let bytes = self
            .app
            .write_local_save(&path)
            .map_err(|error| format!("save {name}: {error}"))?;

        self.detail = Some(format!("{} ({bytes} байт)", path.display()));
        Ok(Flow::Continue)
    }

    /// `save_v70 FILE` — настоящий V70-сейв (P3, `GameState::to_save_bytes`).
    ///
    /// В отличие от `save` (контейнер `MXLS` с настройками и снапшотом) это
    /// файл формата оригинала: его можно загрузить обратно командой `load`
    /// (и открыть в C++-порте).
    fn save_v70(&mut self, name: &str) -> Result<Flow, String> {
        let path = self.config.out_dir.join(name);
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "QA save".to_string());
        let bytes = self
            .app
            .save_v70_to(&path, &name)
            .map_err(|error| format!("save_v70 {name}: {error}"))?;

        self.detail = Some(format!("{} ({bytes} байт V70)", path.display()));
        Ok(Flow::Continue)
    }

    fn load(&mut self, name: &str) -> Result<Flow, String> {
        let out_path = self.config.out_dir.join(name);
        let data_path = self.config.data_dir.join(name);
        let path = if out_path.is_file() {
            out_path
        } else if data_path.is_file() {
            data_path
        } else {
            return Err(format!(
                "load {name}: нет файла ни в каталоге вывода, ни в {}",
                self.config.data_dir.display()
            ));
        };

        let bytes =
            std::fs::read(&path).map_err(|error| format!("load {}: {error}", path.display()))?;

        let setup = match &self.screen {
            Screen::LocalSetup(form) => form.setup.clone(),
            _ => LocalSetup::default(),
        };

        self.app
            .start_local_game_from_save(&setup, &bytes)
            .map_err(|error| format!("load {}: {error}", path.display()))?;

        self.screen = Screen::InSession {
            request: SessionRequest {
                game_id: "local".to_string(),
                password: None,
            },
        };
        self.ctx.status = String::new();

        self.detail = Some(format!(
            "{}: ход {}, юнитов {}",
            path.display(),
            self.app.state().turn_counter,
            self.app.state().unit_count()
        ));
        Ok(Flow::Continue)
    }

    // -----------------------------------------------------------------------
    // Ассерты
    // -----------------------------------------------------------------------

    fn check(&mut self, assertion: Assertion) -> Result<Flow, String> {
        match assertion {
            Assertion::Turn(cmp, expected) => {
                let actual = i64::from(self.app.state().turn_counter);

                self.expect(
                    cmp.test(actual, i64::from(expected)),
                    format!("turn{}{expected}", cmp.as_str()),
                    format!("turn={actual}"),
                )
            }
            Assertion::Units(cmp, expected) => {
                let actual = i64::try_from(self.app.state().unit_count()).unwrap_or(i64::MAX);

                self.expect(
                    cmp.test(actual, i64::from(expected)),
                    format!("units{}{expected}", cmp.as_str()),
                    format!("units={actual}"),
                )
            }
            Assertion::Team(expected) => {
                let actual = self.app.state().active_turn_team.0;

                self.expect(
                    actual == expected,
                    format!("team={expected}"),
                    format!("team={actual} ({})", self.team_name(TeamId(actual))),
                )
            }
            Assertion::Screen(expected) => self.expect(
                self.screen_matches(expected),
                format!("screen={}", expected.as_str()),
                format!("screen={}", self.screen_label()),
            ),
            Assertion::Phase(expected) => {
                let actual = self.phase_name();

                self.expect(
                    actual == Some(expected),
                    format!("phase={}", expected.as_str()),
                    format!("phase={}", self.phase_label()),
                )
            }
            Assertion::Notice(expected) => {
                let actual = self.app.local_notice().map(|notice| notice.team.0);

                self.expect(
                    actual == expected,
                    match expected {
                        Some(team) => format!("notice={team}"),
                        None => "notice=none".to_string(),
                    },
                    format!("notice={}", self.notice_label()),
                )
            }
            Assertion::Victory | Assertion::Defeat => {
                let want_victory = assertion == Assertion::Victory;
                let outcome = self.app.local().and_then(|local| local.outcome.as_ref());

                match outcome {
                    Some(outcome) => self.expect(
                        outcome.victory == want_victory,
                        if want_victory { "victory" } else { "defeat" }.to_string(),
                        format!(
                            "исход: {} (ход {}, команда {})",
                            if outcome.victory {
                                "победа"
                            } else {
                                "поражение"
                            },
                            outcome.turn_counter,
                            outcome.team.0
                        ),
                    ),
                    None => Err(format!(
                        "{}: партия ещё не окончена (phase={}, ход {})",
                        if want_victory { "victory" } else { "defeat" },
                        self.phase_label(),
                        self.app.state().turn_counter
                    )),
                }
            }
            Assertion::BuildOpen(expected) => {
                let actual = self.app.build_menu().is_some();

                self.expect(
                    actual == expected,
                    format!("build={}", if expected { "open" } else { "closed" }),
                    format!("build={}", if actual { "open" } else { "closed" }),
                )
            }
            Assertion::BuildSiteOpen(expected) => {
                let actual = self.app.build_site().is_some();

                self.expect(
                    actual == expected,
                    format!("build-site={}", if expected { "open" } else { "closed" }),
                    match self.app.build_site() {
                        Some(site) => format!(
                            "build-site=open, призрак {} в ({}, {})",
                            crate::buildmenu::unit_name(site.unit_type),
                            site.anchor.x,
                            site.anchor.y
                        ),
                        None => "build-site=closed".to_string(),
                    },
                )
            }
            Assertion::BuildSiteValid(expected) => {
                let actual = self
                    .app
                    .build_site()
                    .map(|site| site.valid(self.app.state()));

                self.expect(
                    actual == Some(expected),
                    format!("build-site={}", if expected { "valid" } else { "invalid" }),
                    match self.app.build_site() {
                        Some(site) => format!(
                            "build-site={} (якорь {},{}; курсор {},{}), статус «{}»",
                            if site.valid(self.app.state()) {
                                "valid"
                            } else {
                                "invalid"
                            },
                            site.anchor.x,
                            site.anchor.y,
                            site.cursor.x,
                            site.cursor.y,
                            site.status_line(self.app.state())
                        ),
                        None => "build-site=closed (режим не открыт)".to_string(),
                    },
                )
            }
            Assertion::BuildCenter(expected) => {
                let builder = self
                    .app
                    .build_site()
                    .map(|site| site.builder)
                    .or_else(|| self.app.hud().selected);
                let Some(unit) = builder.and_then(|id| self.app.state().unit(id)) else {
                    return self.expect(
                        false,
                        "build-center=yes".to_string(),
                        "нет строителя".to_string(),
                    );
                };
                let offset = crate::buildsite::tape_center_offset(self.app.state(), unit);
                let tape = self
                    .app
                    .state()
                    .unit_ids_in_order()
                    .into_iter()
                    .filter_map(|id| self.app.state().unit(id))
                    .find(|candidate| {
                        candidate.parent_unit == Some(unit.id)
                            && matches!(
                                candidate.unit_type,
                                max_core::enums::UnitType::LargeTape
                                    | max_core::enums::UnitType::SmallTape
                            )
                    });
                let centered = match tape {
                    Some(tape) => {
                        // Центр ленты: половина её габарита (2×2 — 64 px,
                        // 1×1 — 32 px) от якоря.
                        let half = if tape.unit_type == max_core::enums::UnitType::LargeTape {
                            64
                        } else {
                            32
                        };
                        let (center_x, center_y) = (
                            i32::from(tape.grid.x) * 64 + half,
                            i32::from(tape.grid.y) * 64 + half,
                        );
                        let (anchor_x, anchor_y) = (
                            i32::from(unit.grid.x) * 64 + 32 + offset.0,
                            i32::from(unit.grid.y) * 64 + 32 + offset.1,
                        );

                        (anchor_x - center_x).abs() <= 1 && (anchor_y - center_y).abs() <= 1
                    }
                    None => false,
                };
                let actual = if centered { "centered" } else { "not centered" };

                self.expect(
                    centered == expected,
                    format!("build-center={}", if expected { "yes" } else { "no" }),
                    format!(
                        "build-center={actual} (сдвиг {},{}; клетка строителя {},{}",
                        offset.0, offset.1, unit.grid.x, unit.grid.y
                    ),
                )
            }
            Assertion::SitePos(x, y) => {
                let team = self.app.player_team();
                let tape = self
                    .app
                    .state()
                    .unit_ids_in_order()
                    .into_iter()
                    .filter_map(|id| self.app.state().unit(id))
                    .filter(|unit| {
                        unit.team == team
                            && matches!(
                                unit.unit_type,
                                max_core::enums::UnitType::LargeTape
                                    | max_core::enums::UnitType::SmallTape
                            )
                    })
                    .map(|unit| unit.grid)
                    .next();

                self.expect(
                    tape == Some(Point::new(x, y)),
                    format!("site-pos={x},{y}"),
                    match tape {
                        Some(site) => format!("site-pos={},{}", site.x, site.y),
                        None => "площадки нет".to_string(),
                    },
                )
            }
            Assertion::BuilderInside(inside) => {
                let team = self.app.player_team();
                let builder = self
                    .app
                    .state()
                    .unit_ids_in_order()
                    .into_iter()
                    .filter_map(|id| self.app.state().unit(id))
                    .filter(|unit| {
                        unit.team == team && unit.unit_type == max_core::enums::UnitType::Constructor
                    })
                    .next_back();
                let Some(builder) = builder else {
                    return self.expect(
                        false,
                        format!("builder={}", if inside { "inside" } else { "outside" }),
                        "строителя нет".to_string(),
                    );
                };
                // Здание — ближайшее своё `BUILDING` (достроенное или ещё
                // ждущее ухода строителя: `SpawnNewUnit` уже поставил его на
                // площадку). Родитель строителя после `ActivateEngineer`
                // сбрасывается, поэтому опора — габариты здания.
                let building = self
                    .app
                    .state()
                    .unit_ids_in_order()
                    .into_iter()
                    .filter_map(|id| self.app.state().unit(id))
                    .filter(|unit| {
                        unit.team == team
                            && unit.flags & max_core::enums::unit_flags::BUILDING != 0
                    })
                    .min_by_key(|unit| {
                        let size: i16 = 2;
                        let dx = (i16::from(unit.grid.x) - i16::from(builder.grid.x)).abs()
                            - (size - 1).max(0);
                        let dy = (i16::from(unit.grid.y) - i16::from(builder.grid.y)).abs()
                            - (size - 1).max(0);
                        (dx.max(0) + dy.max(0), u16::MAX - unit.id.0)
                    });
                let Some(building) = building else {
                    return self.expect(
                        false,
                        format!("builder={}", if inside { "inside" } else { "outside" }),
                        "у команды нет зданий".to_string(),
                    );
                };
                let actual_inside =
                    max_core::production::unit_bounds_contains(building, builder.grid);

                self.expect(
                    actual_inside == inside,
                    format!("builder={}", if inside { "inside" } else { "outside" }),
                    format!(
                        "строитель в ({},{}), здание {:?} в ({},{}) 2×2 — {}",
                        builder.grid.x,
                        builder.grid.y,
                        building.id,
                        building.grid.x,
                        building.grid.y,
                        if actual_inside { "внутри" } else { "снаружи" }
                    ),
                )
            }
            Assertion::BuildReady(waiting) => {
                let team = self.app.player_team();
                let found = self
                    .app
                    .state()
                    .unit_ids_in_order()
                    .into_iter()
                    .filter_map(|id| self.app.state().unit(id))
                    .find(|unit| {
                        unit.team == team
                            && unit.orders == max_core::enums::UnitOrder::Idle
                            && unit.state == max_core::enums::UnitOrderState::BuildingReady
                    });

                self.expect(
                    found.is_some() == waiting,
                    format!("build-ready={}", if waiting { "yes" } else { "no" }),
                    match found {
                        Some(unit) => format!(
                            "ждущее здание {:?} в ({},{})",
                            unit.id, unit.grid.x, unit.grid.y
                        ),
                        None => "ждущих зданий нет".to_string(),
                    },
                )
            }
            Assertion::EndTurnPressed(pressed) => {
                // Та же функция, что рисует кнопку (`panels::end_turn_pressed`):
                // ход компьютера — нажата, ход человека — отжата.
                let active = self.app.state().active_turn_team;
                let kind = self
                    .app
                    .state()
                    .teams
                    .get(usize::from(active.0))
                    .map(|team| team.team_type)
                    .unwrap_or(max_core::team::TeamKind::None);
                let actual = crate::panels::end_turn_pressed(kind);

                self.expect(
                    actual == pressed,
                    format!(
                        "endturn={}",
                        if pressed { "pressed" } else { "normal" }
                    ),
                    format!(
                        "endturn={} (команда {:?}, тип {:?})",
                        if actual { "pressed" } else { "normal" },
                        active,
                        kind
                    ),
                )
            }
            Assertion::Hint(expected) => {
                let actual = self.app.hud().hint.clone();
                let ok = match (&expected, &actual) {
                    (None, None) => true,
                    (Some(expected), Some(actual)) => actual
                        .to_ascii_lowercase()
                        .contains(&expected.to_ascii_lowercase()),
                    _ => false,
                };

                self.expect(
                    ok,
                    match &expected {
                        Some(expected) => format!("hint={expected}"),
                        None => "hint=none".to_string(),
                    },
                    match actual {
                        Some(text) => format!("hint={}", text.replace('\n', " / ")),
                        None => "hint=none".to_string(),
                    },
                )
            }
            Assertion::UpgradeOpen(expected) => {
                let actual = self.app.upgrade_menu().is_some();

                self.expect(
                    actual == expected,
                    format!("upgrade={}", if expected { "open" } else { "closed" }),
                    format!("upgrade={}", if actual { "open" } else { "closed" }),
                )
            }
            Assertion::ResearchMenuOpen(expected) => {
                let actual = self.app.research_menu().is_some();

                self.expect(
                    actual == expected,
                    format!("research-menu={}", if expected { "open" } else { "closed" }),
                    format!("research-menu={}", if actual { "open" } else { "closed" }),
                )
            }
            Assertion::UnitMenuOpen(expected) => {
                let actual = self.app.unit_menu().is_some();

                self.expect(
                    actual == expected,
                    format!("unit-menu={}", if expected { "open" } else { "closed" }),
                    format!("unit-menu={}", if actual { "open" } else { "closed" }),
                )
            }
            Assertion::UnitMenuItem(name) => {
                let actual = self
                    .app
                    .unit_menu()
                    .and_then(|menu| crate::unitmenu::item_enabled(menu, &name));

                self.expect(
                    actual == Some(true),
                    format!("unit-menu-item={name} (включён)"),
                    match actual {
                        Some(true) => format!("{name}: включён"),
                        Some(false) => format!("{name}: отключён"),
                        None => match self.app.unit_menu() {
                            Some(menu) => format!(
                                "нет пункта {name} (есть: {})",
                                menu.items
                                    .iter()
                                    .map(|item| item.caption.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            None => "меню не открыто".to_string(),
                        },
                    },
                )
            }
            Assertion::UnitMenuDisabled(name) => {
                let actual = self
                    .app
                    .unit_menu()
                    .and_then(|menu| crate::unitmenu::item_enabled(menu, &name));

                self.expect(
                    actual == Some(false),
                    format!("unit-menu-disabled={name}"),
                    match actual {
                        Some(false) => format!("{name}: отключён"),
                        Some(true) => format!("{name}: включён"),
                        None => "нет такого пункта".to_string(),
                    },
                )
            }
            Assertion::UnitOrder(expected) => {
                let actual = self
                    .app
                    .hud()
                    .selected
                    .and_then(|id| self.app.state().unit(id))
                    .map(|unit| unit.orders);

                self.expect(
                    actual == Some(expected),
                    format!("unit-order={expected:?}"),
                    match actual {
                        Some(actual) => format!("unit-order={actual:?}"),
                        None => "выделения нет".to_string(),
                    },
                )
            }
            Assertion::ResearchAlloc(topic, cmp, expected) => {
                let actual =
                    self.app.state().research_allocation(self.app.player_team())[topic as usize];

                self.expect(
                    cmp.test(i64::from(actual), i64::from(expected)),
                    format!("research-alloc {topic:?} {}{expected}", cmp.as_str()),
                    format!("research-alloc {topic:?}={actual}"),
                )
            }
            Assertion::AllocOpen(expected) => {
                let actual = self.app.alloc_menu().is_some();

                self.expect(
                    actual == expected,
                    format!("alloc={}", if expected { "open" } else { "closed" }),
                    format!("alloc={}", if actual { "open" } else { "closed" }),
                )
            }
            Assertion::AllocRaw(cmp, expected) => {
                self.check_alloc_value("alloc-raw", cmp, expected, |menu| {
                    menu.production_of(max_core::alloc::MiningKind::Raw)
                })
            }
            Assertion::AllocFuel(cmp, expected) => {
                self.check_alloc_value("alloc-fuel", cmp, expected, |menu| {
                    menu.production_of(max_core::alloc::MiningKind::Fuel)
                })
            }
            Assertion::AllocGold(cmp, expected) => {
                self.check_alloc_value("alloc-gold", cmp, expected, |menu| {
                    menu.production_of(max_core::alloc::MiningKind::Gold)
                })
            }
            Assertion::AllocFree(cmp, expected) => {
                self.check_alloc_value("alloc-free", cmp, expected, |menu| menu.free_capacity())
            }
            Assertion::ComplexRaw(cmp, expected) => {
                self.check_complex_value("complex-raw", cmp, expected, |complex| {
                    i32::from(complex.material)
                })
            }
            Assertion::ComplexFuel(cmp, expected) => {
                self.check_complex_value("complex-fuel", cmp, expected, |complex| {
                    i32::from(complex.fuel)
                })
            }
            Assertion::ComplexGold(cmp, expected) => {
                self.check_complex_value("complex-gold", cmp, expected, |complex| {
                    i32::from(complex.gold)
                })
            }
            Assertion::TransferOpen(expected) => {
                let actual = self.app.transfer_menu().is_some();

                self.expect(
                    actual == expected,
                    format!("transfer={}", if expected { "open" } else { "closed" }),
                    format!("transfer={}", if actual { "open" } else { "closed" }),
                )
            }
            Assertion::TransferAmount(cmp, expected) => {
                let actual = self.app.transfer_menu().map(|menu| menu.amount);

                self.expect(
                    actual.is_some_and(|actual| cmp.test(i64::from(actual), i64::from(expected))),
                    format!("transfer-amount{}{expected}", cmp.as_str()),
                    match actual {
                        Some(actual) => format!("transfer-amount={actual}"),
                        None => "окно X-FER не открыто".to_string(),
                    },
                )
            }
            Assertion::UnitStorage(cmp, expected) => {
                let actual = self
                    .app
                    .hud()
                    .selected
                    .and_then(|id| self.app.state().unit(id))
                    .map(|unit| i32::from(unit.storage));

                self.expect(
                    actual.is_some_and(|actual| cmp.test(i64::from(actual), i64::from(expected))),
                    format!("unit-storage{}{expected}", cmp.as_str()),
                    match actual {
                        Some(actual) => format!("unit-storage={actual}"),
                        None => "юнит не выделен".to_string(),
                    },
                )
            }
            Assertion::HelpOpen(expected) => {
                let actual = self.current_help_section().is_some();

                self.expect(
                    actual == expected,
                    format!("help={}", if expected { "open" } else { "closed" }),
                    format!("help={}", if actual { "open" } else { "closed" }),
                )
            }
            Assertion::HelpSection(section) => {
                let actual = self.current_help_section();

                self.expect(
                    actual == Some(section),
                    format!("help-section={}", section.as_str()),
                    match actual {
                        Some(actual) => format!("help-section={}", actual.as_str()),
                        None => "окно помощи не открыто".to_string(),
                    },
                )
            }
            Assertion::HelpRow(cmp, expected) => {
                let actual = self.current_help_row();

                self.expect(
                    cmp.test(actual as i64, i64::from(expected)),
                    format!("help-row{}{expected}", cmp.as_str()),
                    format!("help-row={actual}"),
                )
            }
            Assertion::Gold(cmp, expected) => {
                let actual = self
                    .app
                    .state()
                    .team_units
                    .get(self.app.player_team().0 as usize)
                    .map(|units| units.gold)
                    .unwrap_or(0);
                let ok = match cmp {
                    crate::qa::script::Cmp::Eq => actual == expected,
                    crate::qa::script::Cmp::Ge => actual >= expected,
                    crate::qa::script::Cmp::Le => actual <= expected,
                    crate::qa::script::Cmp::Gt => actual > expected,
                    crate::qa::script::Cmp::Lt => actual < expected,
                };

                self.expect(
                    ok,
                    format!("gold {:?} {expected}", cmp),
                    format!("gold {actual}"),
                )
            }
            Assertion::ResearchLevel(topic, cmp, expected) => {
                let actual = self.app.state().teams[self.app.player_team().0 as usize]
                    .research_topics[topic as usize]
                    .research_level;
                let ok = match cmp {
                    crate::qa::script::Cmp::Eq => actual == expected,
                    crate::qa::script::Cmp::Ge => actual >= expected,
                    crate::qa::script::Cmp::Le => actual <= expected,
                    crate::qa::script::Cmp::Gt => actual > expected,
                    crate::qa::script::Cmp::Lt => actual < expected,
                };

                self.expect(
                    ok,
                    format!("research {topic:?} {:?} {expected}", cmp),
                    format!("research {topic:?} {actual}"),
                )
            }
            Assertion::UnitType(name) => {
                let team = self.app.player_team();
                let wanted = name.trim().to_ascii_lowercase();
                let found = self.app.state().unit_ids_in_order().into_iter().any(|id| {
                    self.app.state().unit(id).is_some_and(|unit| {
                        unit.team == team
                            && (crate::buildmenu::unit_name(unit.unit_type).to_ascii_lowercase()
                                == wanted
                                || format!("{:?}", unit.unit_type).to_ascii_lowercase() == wanted)
                    })
                });

                self.expect(
                    found,
                    format!("unit={name}"),
                    format!(
                        "юнита «{name}» у команды {team:?} {}",
                        if found { "есть" } else { "нет" }
                    ),
                )
            }
            Assertion::ConstructionSite(large) => {
                let team = self.app.player_team();
                let mut matching = Vec::new();
                let mut any_site = Vec::new();

                for id in self.app.state().unit_ids_in_order() {
                    let Some(tape) = self.app.state().unit(id) else {
                        continue;
                    };
                    let (is_large, cones_type, slab_type) = match tape.unit_type {
                        max_core::enums::UnitType::LargeTape => (
                            true,
                            max_core::enums::UnitType::LargeCones,
                            max_core::enums::UnitType::LargeSlab,
                        ),
                        max_core::enums::UnitType::SmallTape => (
                            false,
                            max_core::enums::UnitType::SmallCones,
                            max_core::enums::UnitType::SmallSlab,
                        ),
                        _ => continue,
                    };
                    if tape.team != team {
                        continue;
                    }

                    let site = tape.grid;
                    let units = self.app.state().units_at(site);
                    let has_cones = units.iter().any(|other| {
                        self.app
                            .state()
                            .unit(*other)
                            .is_some_and(|unit| unit.unit_type == cones_type)
                    });
                    let has_slab = units.iter().any(|other| {
                        self.app
                            .state()
                            .unit(*other)
                            .is_some_and(|unit| unit.unit_type == slab_type)
                    });

                    any_site.push((site, is_large, has_cones, has_slab));

                    if large == Some(is_large) && has_cones && has_slab {
                        matching.push(site);
                    }
                }

                let ok = match large {
                    Some(_) => !matching.is_empty(),
                    None => any_site.is_empty(),
                };
                let label = match large {
                    Some(true) => "large",
                    Some(false) => "small",
                    None => "none",
                };
                let actual = if any_site.is_empty() {
                    "площадок нет".to_string()
                } else {
                    any_site
                        .iter()
                        .map(|(site, is_large, has_cones, has_slab)| {
                            format!(
                                "{} ({},{}): cones={}, slab={}",
                                if *is_large { "large" } else { "small" },
                                site.x,
                                site.y,
                                has_cones,
                                has_slab
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                };

                self.expect(ok, format!("site={label}"), actual)
            }
            Assertion::ConstructionSiteDrawn => {
                let team = self.app.player_team();
                let site_types = [
                    max_core::enums::UnitType::LargeTape,
                    max_core::enums::UnitType::LargeCones,
                    max_core::enums::UnitType::LargeSlab,
                    max_core::enums::UnitType::SmallTape,
                    max_core::enums::UnitType::SmallCones,
                    max_core::enums::UnitType::SmallSlab,
                ];
                // Только декали **стройплощадки**: клетка ленты (`LRG/SMLTAPE`)
                // и всё, что в ней лежит. Постоянные плиты-фундаменты базы под
                // это не попадают.
                let mut decals: Vec<(max_core::unit::UnitId, max_core::enums::UnitType)> =
                    Vec::new();

                for id in self.app.state().unit_ids_in_order() {
                    let Some(tape) = self.app.state().unit(id) else {
                        continue;
                    };
                    if tape.team != team
                        || !matches!(
                            tape.unit_type,
                            max_core::enums::UnitType::LargeTape
                                | max_core::enums::UnitType::SmallTape
                        )
                    {
                        continue;
                    }

                    for other in self.app.state().units_at(tape.grid) {
                        let Some(unit) = self.app.state().unit(*other) else {
                            continue;
                        };
                        if unit.team == team && site_types.contains(&unit.unit_type) {
                            decals.push((*other, unit.unit_type));
                        }
                    }
                }

                if decals.is_empty() {
                    return self.expect(
                        false,
                        "site=drawn".to_string(),
                        "площадок нет".to_string(),
                    );
                }

                // Кадр со всеми декалями против кадра без одной: если декаль
                // реально нарисована, её пиксели исчезнут (`IsRelevant`:
                // `ORDER_IDLE` не рисуется).
                let full = self.app.render_frame();
                let mut ok = true;
                let mut parts = Vec::new();

                for (id, unit_type) in &decals {
                    if let Some(unit) = self.app.state_mut().unit_mut(*id) {
                        unit.set_visible_to_team(team, false);
                    }
                    let hidden = self.app.render_frame();
                    if let Some(unit) = self.app.state_mut().unit_mut(*id) {
                        unit.set_visible_to_team(team, true);
                    }

                    let own = full
                        .pixels()
                        .chunks_exact(4)
                        .zip(hidden.pixels().chunks_exact(4))
                        .filter(|(left, right)| left != right)
                        .count();

                    if own < 40 {
                        ok = false;
                    }

                    parts.push(format!("{unit_type:?}: {own} px"));
                }

                self.expect(ok, "site=drawn".to_string(), parts.join(", "))
            }
            Assertion::PrefsOpen(open) => {
                let actual = self.prefs_dialog().is_some();
                self.expect(
                    actual == open,
                    format!("prefs={}", if open { "open" } else { "closed" }),
                    format!("prefs={}", if actual { "open" } else { "closed" }),
                )
            }
            Assertion::PrefsValue(key, cmp, expected) => {
                let dialog = self.prefs_dialog();
                let actual = dialog.and_then(|dialog| dialog.value(key));
                let ok = actual.is_some_and(|actual| cmp.test(i64::from(actual), i64::from(expected)));

                self.expect(
                    ok,
                    format!("prefs-value {}={expected}", key.name()),
                    match actual {
                        Some(actual) => format!("{}={actual}", key.name()),
                        None => "диалог не открыт/контрол скрыт".to_string(),
                    },
                )
            }
            Assertion::PrefsEffects(on) => {
                // Пока диалог открыт, действует его черновик (как
                // `settings->SetNumericValue` в C++ применяет клик сразу).
                let actual = self
                    .prefs_dialog()
                    .map(|dialog| dialog.draft.effects)
                    .unwrap_or_else(|| self.app.effects_enabled());
                self.expect(
                    actual == on,
                    format!("prefs-effects={}", if on { "on" } else { "off" }),
                    format!("prefs-effects={}", if actual { "on" } else { "off" }),
                )
            }
            Assertion::PrefsSound(on) => {
                let actual = self
                    .prefs_dialog()
                    .map(|dialog| dialog.draft.sound_enabled())
                    .unwrap_or_else(|| self.app.sound_enabled());
                self.expect(
                    actual == on,
                    format!("prefs-sound={}", if on { "on" } else { "off" }),
                    format!("prefs-sound={}", if actual { "on" } else { "off" }),
                )
            }
            Assertion::PrefsAudio(on) => {
                // Флаг `disable_fx` доходит до аудио-слоя (`AudioPlayer::enabled`).
                let actual = self.ctx.audio.enabled();
                self.expect(
                    actual == on,
                    format!("prefs-audio={}", if on { "on" } else { "off" }),
                    format!("prefs-audio={}", if actual { "on" } else { "off" }),
                )
            }
            Assertion::Selected => {
                let team = self.app.player_team();
                let selected = self
                    .app
                    .hud()
                    .selected
                    .and_then(|id| self.app.state().unit(id));
                let ok = selected.is_some_and(|unit| unit.team == team && hud::unit_can_move(unit));

                self.expect(
                    ok,
                    "selected".to_string(),
                    match selected {
                        Some(unit) if unit.team == team => {
                            format!(
                                "выделен {:?}, подвижный: {}",
                                unit.unit_type,
                                hud::unit_can_move(unit)
                            )
                        }
                        Some(unit) => format!("выделен чужой {:?}", unit.team),
                        None => "выделения нет".to_string(),
                    },
                )
            }
            Assertion::Preview => {
                let preview = self.app.hud().preview.as_ref();
                let steps = preview.map(|preview| preview.len()).unwrap_or(0);

                self.expect(
                    steps > 0,
                    "preview".to_string(),
                    match preview {
                        Some(preview) => format!(
                            "предпросмотр: {} шагов {:?} → {:?}",
                            preview.len(),
                            preview.from,
                            preview.to
                        ),
                        None => "предпросмотра нет".to_string(),
                    },
                )
            }
            Assertion::Log(text) => {
                let found = self
                    .app
                    .hud()
                    .log
                    .lines()
                    .iter()
                    .find(|line| line.contains(&text))
                    .cloned();
                let last = self
                    .app
                    .hud()
                    .log
                    .lines()
                    .last()
                    .cloned()
                    .unwrap_or_default();

                self.expect(
                    found.is_some(),
                    format!("log={text}"),
                    match found {
                        Some(line) => format!("найдено в журнале: «{line}»"),
                        None => format!("в журнале нет «{text}»; последняя строка: «{last}»"),
                    },
                )
            }
            Assertion::UnitPos(x, y) => {
                let selected = self
                    .app
                    .hud()
                    .selected
                    .and_then(|id| self.app.state().unit(id));

                self.expect(
                    selected.is_some_and(|unit| unit.grid == max_core::geo::Point::new(x, y)),
                    format!("unit-pos={x},{y}"),
                    match selected {
                        Some(unit) => format!(
                            "выделенный {:?} в ({}, {})",
                            unit.unit_type, unit.grid.x, unit.grid.y
                        ),
                        None => "выделения нет".to_string(),
                    },
                )
            }
            Assertion::Angle(expected) => {
                let selected = self
                    .app
                    .hud()
                    .selected
                    .and_then(|id| self.app.state().unit(id));
                let names = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

                self.expect(
                    selected.is_some_and(|unit| unit.angle == expected),
                    format!(
                        "angle={expected} ({})",
                        names.get(usize::from(expected)).copied().unwrap_or("?")
                    ),
                    match selected {
                        Some(unit) => format!(
                            "angle={} ({})",
                            unit.angle,
                            names.get(usize::from(unit.angle)).copied().unwrap_or("?")
                        ),
                        None => "выделения нет".to_string(),
                    },
                )
            }
            Assertion::Digest(expected) => {
                let actual = self.app.digest().state_digest;

                self.expect(
                    actual == expected,
                    format!("digest=0x{expected:04X}"),
                    format!("digest=0x{actual:04X}"),
                )
            }
            Assertion::CargoMode(expected) => {
                let actual = self
                    .mobilization()
                    .map(|m| m.mode == crate::mobilize::CargoMode::Buy);

                self.expect(
                    actual == Some(expected),
                    format!("cargo-mode={}", if expected { "buy" } else { "upgrade" }),
                    match actual {
                        Some(true) => "cargo-mode=buy".to_string(),
                        Some(false) => "cargo-mode=upgrade".to_string(),
                        None => "экрана мобилизации нет".to_string(),
                    },
                )
            }
            Assertion::CargoTypes(cmp, expected) => {
                let actual = self.mobilization().map(|m| m.types.len() as i64);

                self.expect(
                    actual.is_some_and(|actual| cmp.test(actual, i64::from(expected))),
                    format!("cargo-types{}{expected}", cmp.as_str()),
                    match actual {
                        Some(actual) => format!("cargo-types={actual}"),
                        None => "экрана мобилизации нет".to_string(),
                    },
                )
            }
            Assertion::CargoCredits(cmp, expected) => {
                let actual = self.mobilization().map(|m| i64::from(m.credits));

                self.expect(
                    actual.is_some_and(|actual| cmp.test(actual, i64::from(expected))),
                    format!("cargo-credits{}{expected}", cmp.as_str()),
                    match actual {
                        Some(actual) => format!("cargo-credits={actual}"),
                        None => "экрана мобилизации нет".to_string(),
                    },
                )
            }
            Assertion::CargoDescription(cmp, expected) => {
                let actual = self
                    .mobilization()
                    .map(|m| m.description().chars().count() as i64);

                self.expect(
                    actual.is_some_and(|actual| cmp.test(actual, i64::from(expected))),
                    format!("cargo-description{}{expected}", cmp.as_str()),
                    match actual {
                        Some(actual) => format!("cargo-description={actual}"),
                        None => "экрана мобилизации нет".to_string(),
                    },
                )
            }
            Assertion::CargoStatIcons(cmp, expected) => {
                let actual = self.mobilization().map(|m| m.stats_icon_count() as i64);

                self.expect(
                    actual.is_some_and(|actual| cmp.test(actual, i64::from(expected))),
                    format!("cargo-stat-icons{}{expected}", cmp.as_str()),
                    match actual {
                        Some(actual) => format!("cargo-stat-icons={actual}"),
                        None => "экрана мобилизации нет".to_string(),
                    },
                )
            }
            Assertion::CargoPortrait(expected) => {
                let actual = self.mobilization().map(|m| m.has_portrait());

                self.expect(
                    actual == Some(expected),
                    format!("cargo-portrait={}", if expected { "yes" } else { "no" }),
                    match actual {
                        Some(true) => "cargo-portrait=yes".to_string(),
                        Some(false) => "cargo-portrait=no".to_string(),
                        None => "экрана мобилизации нет".to_string(),
                    },
                )
            }
        }
    }

    /// Состояние экрана мобилизации, если он сейчас открыт.
    fn mobilization(&self) -> Option<&crate::mobilize::Mobilization> {
        match &self.screen {
            Screen::PreGame(flow) => match &flow.stage {
                crate::pregame::PreGameStage::Mobilize(mobilization) => Some(mobilization),
                _ => None,
            },
            _ => None,
        }
    }

    fn expect(&mut self, ok: bool, description: String, actual: String) -> Result<Flow, String> {
        if ok {
            self.detail = Some(actual);
            Ok(Flow::Continue)
        } else {
            Err(format!("assert {description}: фактически {actual}"))
        }
    }

    /// Проверка числа из открытого диалога распределения добычи.
    fn check_alloc_value(
        &mut self,
        label: &str,
        cmp: crate::qa::script::Cmp,
        expected: i32,
        value: impl Fn(&crate::allocmenu::AllocMenu) -> i32,
    ) -> Result<Flow, String> {
        let actual = self.app.alloc_menu().map(|menu| value(menu));

        match actual {
            Some(actual) => self.expect(
                cmp.test(i64::from(actual), i64::from(expected)),
                format!("{label}{}{expected}", cmp.as_str()),
                format!("{label}={actual}"),
            ),
            None => Err(format!("assert {label}: диалог распределения не открыт")),
        }
    }

    /// Проверка запаса комплекса (`Complex::material/...`).
    ///
    /// Комплекс — выделенного юнита, иначе первый комплекс команды игрока
    /// ([`crate::App::economy_complex`]): после смены хода выделение снимается,
    /// а запас комплекса шахты всё равно нужно проверить.
    fn check_complex_value(
        &mut self,
        label: &str,
        cmp: crate::qa::script::Cmp,
        expected: i32,
        value: impl Fn(&max_core::team::Complex) -> i32,
    ) -> Result<Flow, String> {
        let team = self.app.player_team();
        let complex = self
            .app
            .economy_complex()
            .and_then(|id| self.app.state().complex(team, id));

        match complex {
            Some(complex) => {
                let actual = value(complex);
                self.expect(
                    cmp.test(i64::from(actual), i64::from(expected)),
                    format!("{label}{}{expected}", cmp.as_str()),
                    format!("{label}={actual}"),
                )
            }
            None => Err(format!(
                "assert {label}: у команды {team:?} нет комплексов"
            )),
        }
    }

    fn require_screen(&mut self, expected: ScreenName, what: &str) -> Result<Flow, String> {
        if self.screen_matches(expected) {
            self.detail = Some(format!("screen={}", self.screen_label()));
            Ok(Flow::Continue)
        } else {
            Err(format!(
                "{what}: ожидался экран {}, сейчас {}",
                expected.as_str(),
                self.screen_label()
            ))
        }
    }

    // -----------------------------------------------------------------------
    // Диагностика состояния
    // -----------------------------------------------------------------------

    fn setup_screen(&self) -> Option<SetupScreen> {
        match &self.screen {
            Screen::LocalSetup(form) => Some(form.screen),
            _ => None,
        }
    }

    fn screen_matches(&self, expected: ScreenName) -> bool {
        match (&self.screen, expected) {
            (Screen::MainMenu, ScreenName::MainMenu) => true,
            (Screen::JoinList { .. }, ScreenName::JoinList) => true,
            (Screen::CreateForm(_), ScreenName::CreateForm) => true,
            (Screen::MultiplayerMenu, ScreenName::MultiplayerMenu) => true,
            (Screen::Introduction(_), ScreenName::Introduction) => true,
            (Screen::Credits(_), ScreenName::Credits) => true,
            (Screen::GameOver, ScreenName::GameOver) => true,
            (Screen::InSession { .. }, ScreenName::InGame) => true,
            (Screen::LocalSetup(_), ScreenName::LocalSetup) => true,
            (Screen::LocalSetup(form), ScreenName::Options) => form.screen == SetupScreen::Options,
            (Screen::LocalSetup(form), ScreenName::Planet) => form.screen == SetupScreen::Planet,
            (Screen::LocalSetup(form), ScreenName::Players) => form.screen == SetupScreen::Players,
            (Screen::LocalSetup(form), ScreenName::Clan) => form.screen == SetupScreen::Clan,
            (Screen::PreGame(flow), ScreenName::Mobilize) => {
                matches!(flow.stage, crate::pregame::PreGameStage::Mobilize(_))
            }
            (Screen::PreGame(flow), ScreenName::Landing) => {
                matches!(flow.stage, crate::pregame::PreGameStage::Landing(_))
            }
            (Screen::Help { .. }, ScreenName::Help) => true,
            (Screen::Preferences(_), ScreenName::Preferences) => true,
            (Screen::Help { screen, .. }, _) => match screen.as_ref() {
                Screen::LocalSetup(form) => match expected {
                    ScreenName::LocalSetup => true,
                    ScreenName::Options => form.screen == SetupScreen::Options,
                    ScreenName::Planet => form.screen == SetupScreen::Planet,
                    ScreenName::Players => form.screen == SetupScreen::Players,
                    ScreenName::Clan => form.screen == SetupScreen::Clan,
                    _ => false,
                },
                Screen::Preferences(_) => expected == ScreenName::Preferences,
                Screen::PreGame(flow) => match expected {
                    ScreenName::Mobilize => {
                        matches!(flow.stage, crate::pregame::PreGameStage::Mobilize(_))
                    }
                    ScreenName::Landing => {
                        matches!(flow.stage, crate::pregame::PreGameStage::Landing(_))
                    }
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        }
    }

    fn screen_label(&self) -> &'static str {
        match &self.screen {
            Screen::MainMenu => "MainMenu",
            Screen::JoinList { .. } => "JoinList",
            Screen::CreateForm(_) => "CreateForm",
            Screen::MultiplayerMenu => "MultiplayerMenu",
            Screen::Introduction(_) => "Introduction",
            Screen::Credits(_) => "Credits",
            Screen::GameOver => "GameOver",
            Screen::InSession { .. } => "InGame",
            Screen::Help { .. } => "Help",
            Screen::Preferences(_) => "Preferences",
            Screen::LocalSetup(form) => match form.screen {
                SetupScreen::Options => "Options",
                SetupScreen::Planet => "Planet",
                SetupScreen::Players => "Players",
                SetupScreen::Clan => "Clan",
            },
            Screen::PreGame(flow) => match &flow.stage {
                crate::pregame::PreGameStage::Mobilize(_) => "Mobilize",
                crate::pregame::PreGameStage::Landing(_) => "Landing",
                crate::pregame::PreGameStage::Finished => "PreGame",
            },
        }
    }

    fn phase_name(&self) -> Option<PhaseName> {
        self.app.local().map(|local| match local.phase {
            LocalPhase::PlayerTurn => PhaseName::PlayerTurn,
            LocalPhase::AiTurn(_) => PhaseName::AiTurn,
            LocalPhase::Advancing => PhaseName::Advancing,
            LocalPhase::Finished => PhaseName::Finished,
        })
    }

    fn phase_label(&self) -> String {
        match self.app.local() {
            Some(local) => match local.phase {
                LocalPhase::PlayerTurn => "PlayerTurn".to_string(),
                LocalPhase::AiTurn(team) => format!("AiTurn({})", self.team_name(team)),
                LocalPhase::Advancing => "Advancing".to_string(),
                LocalPhase::Finished => "Finished".to_string(),
            },
            None => "нет партии".to_string(),
        }
    }

    fn notice_label(&self) -> String {
        match self.app.local_notice() {
            Some(notice) => format!("{}", self.team_name(notice.team)),
            None => "none".to_string(),
        }
    }

    fn team_name(&self, team: TeamId) -> String {
        crate::local::team_name(team).to_string()
    }

    fn last_log_line(&self) -> String {
        self.app
            .hud()
            .log
            .lines()
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qa_menu_client_lists_two_deterministic_games() {
        let mut client = QaMenuClient;
        let games = client.list_games().expect("список игр");

        assert_eq!(games.len(), 2);
        assert_eq!(games[0].name, "QA Skirmish");
        assert_eq!(games[1].world, 11);
        assert!(client.create_game(CreateGameParams::default()).is_err());
    }

    #[test]
    fn default_config_points_at_the_repository_data() {
        let config = QaConfig::new("qa-out");

        assert!(config.data_dir.ends_with("max-original"));
        assert!(config.assets_dir.ends_with("assets"));
        assert_eq!(config.out_dir, PathBuf::from("qa-out"));
        assert_eq!(config.frame_limit, 40_000);
        assert!(!repo_root().as_os_str().is_empty());
    }

    #[test]
    fn screen_names_and_phase_labels_are_stable() {
        assert_eq!(ScreenName::parse("InGame"), Some(ScreenName::InGame));
        assert_eq!(ScreenName::parse("hot seat"), None);
        assert_eq!(ScreenName::Players.as_str(), "Players");
        assert_eq!(PhaseName::parse("AI"), Some(PhaseName::AiTurn));
        assert_eq!(PhaseName::Finished.as_str(), "Finished");
    }
}
