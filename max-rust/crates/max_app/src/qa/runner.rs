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
use crate::landing::{LANDING_AREA, LANDING_CELL};
use crate::local::{LocalPhase, LocalSetup};
use crate::menu;
use crate::mobilize::{
    AVAILABLE_AREA, AVAILABLE_ROW, BUY_BUTTON, CANCEL_BUTTON as MOBILIZE_CANCEL, DELETE_BUTTON,
    DONE_BUTTON as MOBILIZE_DONE, PURCHASE_UNITS,
};
use crate::qa::png::{self, Image};
use crate::qa::script::{
    self, Assertion, BuildCommand, Command, LandingCommand, MenuTarget, MobilizeAction, PhaseName,
    ScreenName, SetupAction, Step, UpgradeCommand,
};
use crate::setup::{self, SetupScreen};
use crate::ui::{
    handle_session_input, InputEvent, Key, MenuClient, Screen, SessionRequest, Transition, UiContext,
};
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
        match command {
            Command::Click(x, y) => self.send(InputEvent::MouseClick(*x, *y)),
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
        }
    }

    /// `build …`: экран `BuildMenu` (P2) — те же действия, что у игрока
    /// (клавиша `B`, клики по строкам списка, кнопки Build/Delete/Done).
    fn build(&mut self, action: BuildCommand) -> Result<Flow, String> {        let app = &mut self.app;

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
                if app.build_menu().is_none() {
                    return Err("build cancel: меню не открыто".to_string());
                }

                app.close_build_menu();
                "меню закрыто".to_string()
            }
        };

        self.detail = Some(detail);

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
    /// `mobilize …`: действия на экране `CargoMenu` через те же клики, что у
    /// игрока (строки списков, кнопки Buy/Delete/Done/Cancel).
    fn mobilize(&mut self, action: MobilizeAction) -> Result<Flow, String> {
        match action {
            MobilizeAction::Buy => {
                self.send(InputEvent::MouseClick(
                    BUY_BUTTON.ulx + BUY_BUTTON.width / 2,
                    BUY_BUTTON.uly + BUY_BUTTON.height / 2,
                ))
            }
            MobilizeAction::BuyIndex(index) => {
                let index = index as usize;

                if index >= PURCHASE_UNITS.len() {
                    return Err(format!(
                        "mobilize buy {index}: в списке {} юнитов",
                        PURCHASE_UNITS.len()
                    ));
                }

                self.send(InputEvent::MouseClick(
                    AVAILABLE_AREA.ulx + 4,
                    AVAILABLE_AREA.uly + AVAILABLE_ROW * index as i32 + 4,
                ))?;

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

    /// `landing …`: выбор точки высадки через клики по кадру и `Enter`/`Esc`.
    fn landing(&mut self, action: LandingCommand) -> Result<Flow, String> {
        match action {
            LandingCommand::Click(x, y) => self.send(InputEvent::MouseClick(x, y)),
            LandingCommand::Grid(grid_x, grid_y) => {
                let x = LANDING_AREA.ulx + i32::from(grid_x) * LANDING_CELL + LANDING_CELL / 2;
                let y = LANDING_AREA.uly + i32::from(grid_y) * LANDING_CELL + LANDING_CELL / 2;

                self.send(InputEvent::MouseClick(x, y))
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
            Transition::StartSession(request) => Err(format!(
                "сетевая сессия «{}» в QA-драйвере не поддерживается",
                request.game_id
            )),
        }
    }

    /// Один кадр игрового цикла — как блок `main.rs` после обработки ввода.
    fn frame(&mut self) {
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
        let palette = self
            .app
            .assets()
            .and_then(|assets| assets.menu.as_ref())
            .map(|menu| menu.palette);

        if let Screen::LocalSetup(form) = &mut self.screen {
            form.ensure_minimaps(palette.as_ref());
        }
    }

    /// Игра готова принять ввод (или уже окончена / показывает окно перехода).
    fn ready(&self) -> bool {
        match self.app.local() {
            None => true,
            Some(local) => {
                local.finished() || local.notice.is_some() || !local.input_blocked()
            }
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
        }
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

    fn require_selection(&mut self, what: &str) -> Result<Flow, String> {        let team = self.app.player_team();
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
            None => Err(format!(
                "{what}: в этой точке нет юнита команды {team:?} (выделение пустое)"
            )),
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

        if matches!(self.screen, Screen::InSession { .. })
            && self.app.hud().selected != Some(id)
        {
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

        self.detail = Some(format!("шагов {}, {}", self.path_steps(), after.map(|o| format!("{:?}", o.0)).unwrap_or_default()));
        Ok(Flow::Continue)
    }

    fn move_grid(&mut self, x: i16, y: i16) -> Result<Flow, String> {
        let target = Point::new(x, y);
        let (screen_x, screen_y) = hud::tile_center(&self.app.map_viewport(), target);
        let (frame_x, frame_y) = (screen_x + layout::MAIN_MAP.ulx, screen_y + layout::MAIN_MAP.uly);

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

        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
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

            rejections.push(format!("({}, {}): {}", target.x, target.y, self.last_log_line()));
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

    fn load(&mut self, name: &str) -> Result<Flow, String> {        let out_path = self.config.out_dir.join(name);
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

        let bytes = std::fs::read(&path)
            .map_err(|error| format!("load {}: {error}", path.display()))?;

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
                let actual = self
                    .app
                    .local_notice()
                    .map(|notice| notice.team.0);

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
                            if outcome.victory { "победа" } else { "поражение" },
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
            Assertion::UpgradeOpen(expected) => {
                let actual = self.app.upgrade_menu().is_some();

                self.expect(
                    actual == expected,
                    format!("upgrade={}", if expected { "open" } else { "closed" }),
                    format!("upgrade={}", if actual { "open" } else { "closed" }),
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
            Assertion::Selected => {                let team = self.app.player_team();
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
                            format!("выделен {:?}, подвижный: {}", unit.unit_type, hud::unit_can_move(unit))
                        }
                        Some(unit) => format!("выделен чужой {:?}", unit.team),
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
            _ => false,
        }
    }

    fn screen_label(&self) -> &'static str {
        match &self.screen {
            Screen::MainMenu => "MainMenu",
            Screen::JoinList { .. } => "JoinList",
            Screen::CreateForm(_) => "CreateForm",
            Screen::MultiplayerMenu => "MultiplayerMenu",
            Screen::GameOver => "GameOver",
            Screen::InSession { .. } => "InGame",
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
