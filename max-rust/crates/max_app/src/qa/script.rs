//! Парсер `.qa`-скриптов (слой 2 QA): строки → [`Command`].
//!
//! Формат описан в `max-rust-qa/scripts/README.md` и `max-rust-qa/plans/qa-plan.md`. Строка —
//! одна команда; `#` начинает комментарий (внутри `"…"` — не начинает);
//! пустые строки игнорируются. Координаты — в пикселях кадра **640×480**
//! (окно оригинала), поэтому одна и та же точка означает одно и то же на
//! любом хосте и в CI.
//!
//! Парсер ничего не исполняет: он только проверяет синтаксис и диапазоны,
//! чтобы опечатка в скрипте падала с номером строки, а не «молча кликом в
//! пустоту». Исполнение — [`crate::qa::runner`].

use max_core::team::TeamKind;

use crate::ui::Key;

/// Ширина кадра QA (окно оригинала).
pub const FRAME_WIDTH: i32 = 640;
/// Высота кадра QA.
pub const FRAME_HEIGHT: i32 = 480;

/// Оператор сравнения в `assert`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    /// `=`
    Eq,
    /// `>=`
    Ge,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `<`
    Lt,
}

impl Cmp {
    /// Проверка `left <op> right`.
    pub fn test(self, left: i64, right: i64) -> bool {
        match self {
            Self::Eq => left == right,
            Self::Ge => left >= right,
            Self::Le => left <= right,
            Self::Gt => left > right,
            Self::Lt => left < right,
        }
    }

    /// Символ оператора (для лога/сообщений об ошибке).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ge => ">=",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Lt => "<",
        }
    }
}

/// Имя экрана в `assert screen=…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenName {
    /// `Screen::MainMenu`.
    MainMenu,
    /// `Screen::JoinList`.
    JoinList,
    /// `Screen::CreateForm`.
    CreateForm,
    /// `Screen::MultiplayerMenu`.
    MultiplayerMenu,
    /// `Screen::LocalSetup` на экране Options.
    Options,
    /// `Screen::LocalSetup` на экране Planet.
    Planet,
    /// `Screen::LocalSetup` на экране Players.
    Players,
    /// `Screen::LocalSetup` на экране Clan.
    Clan,
    /// Любой экран `Screen::LocalSetup` (не важно, какой из трёх).
    LocalSetup,
    /// `Screen::PreGame` — мобилизация (`CargoMenu`).
    Mobilize,
    /// `Screen::PreGame` — выбор точки высадки (`GameManager_SelectSite`).
    Landing,
    /// `Screen::GameOver`.
    GameOver,
    /// `Screen::InSession` (идёт партия).
    InGame,
}

impl ScreenName {
    /// Разбор имени (регистр не важен).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "mainmenu" | "main_menu" | "main" => Some(Self::MainMenu),
            "joinlist" | "join_list" | "join" => Some(Self::JoinList),
            "createform" | "create_form" | "create" => Some(Self::CreateForm),
            "multiplayermenu" | "multiplayer_menu" | "multiplayer" => Some(Self::MultiplayerMenu),
            "options" => Some(Self::Options),
            "planet" | "planets" => Some(Self::Planet),
            "players" => Some(Self::Players),
            "clan" | "clans" => Some(Self::Clan),
            "localsetup" | "local_setup" | "setup" => Some(Self::LocalSetup),
            "mobilize" | "mobilization" | "cargo" => Some(Self::Mobilize),
            "landing" | "siteselect" | "site_select" => Some(Self::Landing),
            "gameover" | "game_over" | "winloss" => Some(Self::GameOver),
            "ingame" | "in_game" | "session" => Some(Self::InGame),
            _ => None,
        }
    }

    /// Каноническое имя (как в логе).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MainMenu => "MainMenu",
            Self::JoinList => "JoinList",
            Self::CreateForm => "CreateForm",
            Self::MultiplayerMenu => "MultiplayerMenu",
            Self::Options => "Options",
            Self::Planet => "Planet",
            Self::Players => "Players",
            Self::Clan => "Clan",
            Self::LocalSetup => "LocalSetup",
            Self::Mobilize => "Mobilize",
            Self::Landing => "Landing",
            Self::GameOver => "GameOver",
            Self::InGame => "InGame",
        }
    }
}

/// Фаза локальной партии в `assert phase=…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseName {
    /// Ход человека.
    PlayerTurn,
    /// Ход компьютера (не важно, какой команды).
    AiTurn,
    /// Переход к следующей команде.
    Advancing,
    /// Партия окончена.
    Finished,
}

impl PhaseName {
    /// Разбор имени (регистр не важен).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "playerturn" | "player_turn" | "player" => Some(Self::PlayerTurn),
            "aiturn" | "ai_turn" | "ai" => Some(Self::AiTurn),
            "advancing" | "next" => Some(Self::Advancing),
            "finished" | "over" => Some(Self::Finished),
            _ => None,
        }
    }

    /// Каноническое имя.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlayerTurn => "PlayerTurn",
            Self::AiTurn => "AiTurn",
            Self::Advancing => "Advancing",
            Self::Finished => "Finished",
        }
    }
}

/// Проверка состояния в `assert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assertion {
    /// `assert turn=N` / `turn>=N`: счётчик хода.
    Turn(Cmp, i32),
    /// `assert team=N|RED|GREEN|BLUE|GRAY`: чья сейчас ход (`active_turn_team`).
    Team(u8),
    /// `assert units>=N`: число юнитов в состоянии.
    Units(Cmp, u32),
    /// `assert screen=NAME`: текущий экран.
    Screen(ScreenName),
    /// `assert phase=NAME`: фаза локальной партии.
    Phase(PhaseName),
    /// `assert notice=TEAM|none`: окно «Begin turn» (M-HS) открыто для команды.
    Notice(Option<u8>),
    /// `assert victory`: партия окончена победой человека.
    Victory,
    /// `assert defeat`: партия окончена поражением.
    Defeat,
    /// `assert selected`: выделен подвижный юнит игрока.
    Selected,
    /// `assert build=open|closed`: открыт ли `BuildMenu` (P2).
    BuildOpen(bool),
    /// `assert upgrade=open|closed`: открыт ли экран апгрейдов (P6).
    UpgradeOpen(bool),
    /// `assert gold>=N`: золото локальной команды.
    Gold(Cmp, u32),
    /// `assert research attack>=1`: уровень темы исследований (P6).
    ResearchLevel(max_core::enums::ResearchTopic, Cmp, u32),
    /// `assert unit=NAME`: у игрока есть юнит с таким именем (P2: юнит/здание
    /// появились после ходов производства).
    UnitType(String),
    /// `assert digest=HEX`: CRC16 снапшота состояния (`App::digest`).
    Digest(u16),
    /// `assert cargo-mode=buy|upgrade`: режим списка `CargoMenu`.
    CargoMode(bool),
    /// `assert cargo-types=N`: длина списка типов (15 в покупке).
    CargoTypes(Cmp, u32),
    /// `assert cargo-credits>=N`: остаток кредитов в `CargoMenu`.
    CargoCredits(Cmp, u32),
    /// `assert cargo-description>=N`: длина описания выбранного типа
    /// (`Unit::GetDescription()`, символы).
    CargoDescription(Cmp, u32),
    /// `assert cargo-stat-icons>=N`: число pips-иконок в таблице статистики
    /// выбранного типа (`UnitStats_DrawStats`).
    CargoStatIcons(Cmp, u32),
    /// `assert cargo-portrait=yes|no`: есть ли у выбранного типа big image
    /// портрета `P_*` (`Unit::GetPortrait()`).
    CargoPortrait(bool),
}

/// Куда идти высокоуровневой командой `menu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    /// `New Game` главного меню → настройка M-SP.
    NewGame,
    /// `Multiplayer Game` → `Multiplayer Menu`.
    Multiplayer,
    /// `Hot Seat Game` → настройка M-HS.
    HotSeat,
    /// `Join Network Game` → список игр лобби.
    Join,
}

/// Действие на экране настройки (`setup`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupAction {
    /// Кнопка `Done` (Options → Planet → Players → старт партии).
    Done,
    /// Кнопка `Cancel`.
    Cancel,
    /// Кнопка `Random` (только на экране планет).
    Random,
}

/// Действие на экране мобилизации (`CargoMenu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobilizeAction {
    /// `buy` — купить выбранный юнит.
    Buy,
    /// `buy N` — выбрать строку N и купить её.
    BuyIndex(u32),
    /// `delete` — продать выбранный купленный юнит.
    Delete,
    /// `step N` — сдвинуть выбор по списку (может быть отрицательным).
    Step(i32),
    /// `mode buy|upgrade` — переключатель `CRGTOG`.
    Mode(bool),
    /// `filter ground|air|sea|building|combat` — кнопка `SEL*`.
    Filter(FilterName),
    /// `select N` — строка N списка типов.
    Select(u32),
    /// `loadout N` — строка N списка «Purchased».
    Loadout(u32),
    /// `row N up|down` — стрелка апгрейда строки N.
    Adjust { row: u32, increase: bool },
    /// `cargo N` — поставить слайдер cargo в N блоков (5·N единиц).
    Cargo(u32),
    /// `done` — `Done` (закрыть мобилизацию).
    Done,
    /// `cancel` — `Cancel`.
    Cancel,
}

/// Фильтр домена списка модернизации.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterName {
    /// Наземные (`SELGRD`).
    Ground,
    /// Воздушные (`SELAIR`).
    Air,
    /// Морские (`SELSEA`).
    Sea,
    /// Здания (`SELBLD`).
    Building,
    /// Только боевые (`SELCBT`).
    Combat,
}

impl FilterName {
    /// Индекс в `Mobilization::filters`.
    pub const fn index(self) -> usize {
        match self {
            Self::Ground => 0,
            Self::Air => 1,
            Self::Sea => 2,
            Self::Building => 3,
            Self::Combat => 4,
        }
    }

    /// Разбор имени (регистр не важен).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "ground" | "grd" | "land" => Some(Self::Ground),
            "air" => Some(Self::Air),
            "sea" | "naval" => Some(Self::Sea),
            "building" | "build" | "bld" => Some(Self::Building),
            "combat" | "cbt" => Some(Self::Combat),
            _ => None,
        }
    }
}

/// Действие экрана `BuildMenu` (P2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildCommand {
    /// `open` — открыть меню для выделенного строителя (клавиша `B`).
    Open,
    /// `select N` — выбрать строку списка.
    Select(u32),
    /// `add` — `Build` завода: добавить выбранный тип в очередь.
    Add,
    /// `delete` — `Delete`: убрать выбранный элемент очереди.
    Delete,
    /// `rate N` — скорость 1/2/4.
    Rate(u16),
    /// `repeat` — кнопка `Repeat` (только завод).
    Repeat,
    /// `step N` — стрелки списка (N > 0 — вниз).
    Step(i32),
    /// `queue step N` — стрелки очереди.
    QueueStep(i32),
    /// `done` — `Done`/Enter: заказ (`Build()`).
    Done,
    /// `cancel` — `Cancel`/Esc: закрыть без заказа.
    Cancel,
}

/// Действие экрана апгрейдов (P6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeCommand {
    /// `open` — открыть для выделенного юнита (клавиша `U`).
    Open,
    /// `select N` — выбрать строку списка типов.
    Select(u32),
    /// `row N up|down` — нажать стрелку апгрейда строки N.
    Adjust { row: u32, increase: bool },
    /// `done` — `CommitUpgradeChanges`.
    Done,
    /// `cancel` — закрыть без изменений.
    Cancel,
}

/// Действие на экране выбора точки высадки (`GameManager_SelectSite`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingCommand {
    /// `click X Y` — клик по кадру (двигает курсор).
    Click(i32, i32),
    /// `grid X Y` — клик по клетке карты 112×112.
    Grid(i16, i16),
    /// `done` — подтвердить точку (Enter/`Done`).
    Done,
    /// `cancel` — отменить выбор (Esc).
    Cancel,
}

/// Одна команда скрипта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `click X Y` — левая кнопка в пикселях кадра.
    Click(i32, i32),
    /// `rclick X Y` — правая кнопка (приказ движения/отмена).
    RightClick(i32, i32),
    /// `key NAME` — клавиша (`ENTER`, `ESC`, `T`, …).
    Key(Key),
    /// `type "text"` — печатные символы в текущий экран/строку.
    Type(String),
    /// `wait N` — до N кадров, пока игра не будет готова к вводу.
    Wait(u32),
    /// `tick N` — ровно N кадров игрового цикла.
    Tick(u32),
    /// `mem` — записать текущий RSS процесса в лог (P4 soak, TC-1108).
    Mem,
    /// `shot NAME` — PNG кадра в каталог вывода.
    Shot(String),
    /// `assert …`.
    Assert(Assertion),
    /// `save FILE` — контейнер `MXLS` (`App::write_local_save`).
    Save(String),
    /// `save_v70 FILE` — настоящий V70-сейв текущей партии
    /// (`GameState::to_save_bytes`, P3).
    SaveV70(String),
    /// `load FILE` — локальная партия из V70-сейва (`App::start_local_game_from_save`).
    Load(String),
    /// `exit` — остановить скрипт (успех).
    Exit,
    /// `menu new_game|multiplayer|hot_seat|join`.
    Menu(MenuTarget),
    /// `setup done|cancel|random`.
    Setup(SetupAction),
    /// `players SLOT human|computer|none`.
    Players {
        /// Слот команды `0..=3`.
        slot: u8,
        /// Кем управляется.
        kind: TeamKind,
    },
    /// `select X Y` — левый клик по точке кадра + проверка выделения.
    Select(i32, i32),
    /// `select unit N` — левый клик по N-му подвижному юниту игрока.
    SelectUnit(u32),
    /// `select last` — клик по самому новому юниту игрока (например, только что
    /// достроенному заводу — он не «подвижный», поэтому `select unit` не
    /// подходит).
    SelectLast,
    /// `select type NAME` — клик по юниту игрока с таким именем (`BuildMenu`
    /// показывает те же имена: «Training Hall», «Mining Station», …).
    SelectType(String),
    /// `move X Y` — правый клик по точке кадра + проверка приказа.
    Move(i32, i32),
    /// `move grid X Y` — правый клик по клетке карты.
    MoveGrid(i16, i16),
    /// `move near` — приказ в соседнюю свободную клетку выделенного юнита.
    MoveNear,
    /// `end_turn` — кнопка End Turn + ходы ИИ до готовности к вводу.
    EndTurn,
    /// `mobilize buy|buy N|delete|step N|done|cancel`.
    Mobilize(MobilizeAction),
    /// `landing click X Y|grid X Y|done|cancel`.
    Landing(LandingCommand),
    /// `build open|select N|add|delete|rate N|repeat|step N|queue step N|done|cancel`.
    Build(BuildCommand),
    /// `upgrade open|select N|row N up|down|done|cancel`.
    Upgrade(UpgradeCommand),
}

/// Строка скрипта с разобранной командой.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Номер строки в файле (1-based).
    pub line: usize,
    /// Исходный текст без комментария.
    pub source: String,
    /// Команда.
    pub command: Command,
}

/// Разбирает весь скрипт.
pub fn parse_script(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let source = strip_comment(raw);
        let source = source.trim();

        if source.is_empty() {
            continue;
        }

        let command = parse_command(source)
            .map_err(|error| format!("строка {line}: {error} — в «{source}»"))?;

        steps.push(Step {
            line,
            source: source.to_string(),
            command,
        });
    }

    Ok(steps)
}

/// Отрезает комментарий `#…`, не трогая содержимое кавычек.
fn strip_comment(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quoted = false;

    for character in line.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                out.push(character);
            }
            '#' if !quoted => break,
            _ => out.push(character),
        }
    }

    out
}

fn parse_command(source: &str) -> Result<Command, String> {
    let (name, rest) = split_word(source);
    let name = name.to_ascii_lowercase();
    let rest = rest.trim();

    match name.as_str() {
        "click" => {
            let (x, y) = parse_point(rest)?;
            Ok(Command::Click(x, y))
        }
        "rclick" | "right_click" => {
            let (x, y) = parse_point(rest)?;
            Ok(Command::RightClick(x, y))
        }
        "key" => Ok(Command::Key(parse_key(rest)?)),
        "type" => Ok(Command::Type(parse_text(rest)?)),
        "wait" => Ok(Command::Wait(parse_count(rest, "wait")?)),
        "tick" => Ok(Command::Tick(parse_count(rest, "tick")?)),
        "mem" => {
            if rest.is_empty() {
                Ok(Command::Mem)
            } else {
                Err(format!("mem не принимает аргументов: «{rest}»"))
            }
        }
        "shot" => Ok(Command::Shot(parse_name(rest, "shot")?)),
        "assert" => Ok(Command::Assert(parse_assertion(rest)?)),
        "save" => Ok(Command::Save(parse_name(rest, "save")?)),
        "save_v70" | "savev70" => Ok(Command::SaveV70(parse_name(rest, "save_v70")?)),
        "load" => Ok(Command::Load(parse_name(rest, "load")?)),
        "exit" => {
            if rest.is_empty() {
                Ok(Command::Exit)
            } else {
                Err(format!("exit не принимает аргументов: «{rest}»"))
            }
        }
        "menu" => Ok(Command::Menu(parse_menu(rest)?)),
        "setup" => Ok(Command::Setup(parse_setup(rest)?)),
        "players" => parse_players(rest),
        "select" => parse_select(rest),
        "move" => parse_move(rest),
        "mobilize" | "cargo" => parse_mobilize(rest),
        "landing" => parse_landing(rest),
        "build" => parse_build(rest),
        "upgrade" => parse_upgrade(rest),
        "end_turn" | "endturn" => {
            if rest.is_empty() {
                Ok(Command::EndTurn)
            } else {
                Err(format!("end_turn не принимает аргументов: «{rest}»"))
            }
        }
        other => Err(format!(
            "неизвестная команда «{other}» (см. max-rust-qa/scripts/README.md)"
        )),
    }
}

/// Первое слово и остаток строки.
fn split_word(source: &str) -> (&str, &str) {
    match source.find(char::is_whitespace) {
        Some(index) => (&source[..index], &source[index..]),
        None => (source, ""),
    }
}

fn parse_i32(text: &str, what: &str) -> Result<i32, String> {
    text.parse::<i32>()
        .map_err(|_| format!("{what}: ожидалось целое число, получено «{text}»"))
}

fn parse_u32(text: &str, what: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .map_err(|_| format!("{what}: ожидалось неотрицательное число, получено «{text}»"))
}

/// Пара координат кадра 640×480.
fn parse_point(rest: &str) -> Result<(i32, i32), String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.len() != 2 {
        return Err(format!(
            "ожидалось «X Y» (две координаты кадра), получено «{rest}»"
        ));
    }

    let x = parse_i32(parts[0], "X")?;
    let y = parse_i32(parts[1], "Y")?;

    if !(0..FRAME_WIDTH).contains(&x) || !(0..FRAME_HEIGHT).contains(&y) {
        return Err(format!(
            "точка ({x}, {y}) вне кадра {FRAME_WIDTH}×{FRAME_HEIGHT}"
        ));
    }

    Ok((x, y))
}

fn parse_count(rest: &str, what: &str) -> Result<u32, String> {
    if rest.is_empty() {
        return Err(format!("{what}: требуется число кадров"));
    }

    parse_u32(rest, what)
}

/// Имя файла кадра/сохранения: без путей и пробелов.
fn parse_name(rest: &str, what: &str) -> Result<String, String> {
    let rest = rest.trim();

    if rest.is_empty() {
        return Err(format!("{what}: требуется имя файла"));
    }
    if rest.contains('/') || rest.contains('\\') || rest.contains("..") {
        return Err(format!(
            "{what}: имя «{rest}» не должно содержать путь (файлы пишутся в каталог вывода)"
        ));
    }
    if !rest
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(format!(
            "{what}: в имени «{rest}» допустимы только A-Z a-z 0-9 . _ -"
        ));
    }

    Ok(rest.to_string())
}

fn parse_key(rest: &str) -> Result<Key, String> {
    let name = rest.trim();

    if name.is_empty() {
        return Err("key: требуется имя клавиши".to_string());
    }

    let key = match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" | "bs" => Key::Backspace,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "pageup" | "page_up" | "pgup" => Key::PageUp,
        "pagedown" | "page_down" | "pgdn" => Key::PageDown,
        _ => {
            let mut characters = name.chars();

            match (characters.next(), characters.next()) {
                (Some(character), None) if character.is_ascii_graphic() => Key::Char(character),
                _ => {
                    return Err(format!(
                        "key: неизвестная клавиша «{name}» (ENTER/ESC/TAB/BACKSPACE/UP/DOWN/LEFT/RIGHT/PAGEUP/PAGEDOWN или один символ)"
                    ))
                }
            }
        }
    };

    Ok(key)
}

/// Текст для `type`: в кавычках — как есть (пробелы разрешены), иначе слово.
fn parse_text(rest: &str) -> Result<String, String> {
    let rest = rest.trim();

    if rest.is_empty() {
        return Err("type: требуется текст".to_string());
    }

    if let Some(inner) = rest.strip_prefix('"') {
        let Some(inner) = inner.strip_suffix('"') else {
            return Err("type: незакрытая кавычка".to_string());
        };

        Ok(inner.to_string())
    } else if rest.contains(char::is_whitespace) {
        Err("type: текст с пробелами заключается в кавычки".to_string())
    } else {
        Ok(rest.to_string())
    }
}

fn parse_team(text: &str) -> Result<u8, String> {
    let lowered = text.trim().to_ascii_lowercase();

    let team = match lowered.as_str() {
        "red" => 0,
        "green" => 1,
        "blue" => 2,
        "gray" | "grey" => 3,
        _ => {
            let value = parse_u32(&lowered, "команда")?;
            if value > 3 {
                return Err(format!("команда {value}: слотов всего 4 (0..=3)"));
            }
            value as u8
        }
    };

    Ok(team)
}

fn parse_team_kind(text: &str) -> Result<TeamKind, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "human" | "player" => Ok(TeamKind::Player),
        "computer" | "ai" | "cpu" => Ok(TeamKind::Computer),
        "none" | "empty" => Ok(TeamKind::None),
        other => Err(format!(
            "players: тип «{other}» (ожидалось human|computer|none)"
        )),
    }
}

fn parse_assertion(rest: &str) -> Result<Assertion, String> {
    let rest = rest.trim();

    if rest.is_empty() {
        return Err("assert: требуется проверка".to_string());
    }

    let lowered = rest.to_ascii_lowercase();

    match lowered.as_str() {
        "victory" | "win" => return Ok(Assertion::Victory),
        "defeat" | "lose" => return Ok(Assertion::Defeat),
        "selected" => return Ok(Assertion::Selected),
        _ => {}
    }

    // Оператор ищем от самого длинного, иначе «>=» распадётся на «>» и «=».
    let (key, operator, value) = if let Some(index) = rest.find(">=") {
        (&rest[..index], Cmp::Ge, &rest[index + 2..])
    } else if let Some(index) = rest.find("<=") {
        (&rest[..index], Cmp::Le, &rest[index + 2..])
    } else if let Some(index) = rest.find('=') {
        (&rest[..index], Cmp::Eq, &rest[index + 1..])
    } else if let Some(index) = rest.find('>') {
        (&rest[..index], Cmp::Gt, &rest[index + 1..])
    } else if let Some(index) = rest.find('<') {
        (&rest[..index], Cmp::Lt, &rest[index + 1..])
    } else {
        return Err(format!(
            "assert: нет оператора в «{rest}» (например turn>=3, screen=InGame, victory)"
        ));
    };

    let key = key.trim().to_ascii_lowercase();
    let value = value.trim();

    if value.is_empty() {
        return Err(format!("assert {key}: пустое значение"));
    }

    match key.as_str() {
        "turn" => Ok(Assertion::Turn(operator, parse_i32(value, "assert turn")?)),
        "units" => Ok(Assertion::Units(
            operator,
            parse_u32(value, "assert units")?,
        )),
        "team" => Ok(Assertion::Team(parse_team(value)?)),
        "screen" => {
            let screen = ScreenName::parse(value)
                .ok_or_else(|| format!("assert screen: неизвестный экран «{value}»"))?;
            Ok(Assertion::Screen(screen))
        }
        "phase" => {
            let phase = PhaseName::parse(value)
                .ok_or_else(|| format!("assert phase: неизвестная фаза «{value}»"))?;
            Ok(Assertion::Phase(phase))
        }
        "notice" => {
            if value.eq_ignore_ascii_case("none") {
                Ok(Assertion::Notice(None))
            } else {
                Ok(Assertion::Notice(Some(parse_team(value)?)))
            }
        }
        "build" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::BuildOpen(true)),
            "closed" | "none" => Ok(Assertion::BuildOpen(false)),
            other => Err(format!(
                "assert build: ожидалось open|closed, получено «{other}»"
            )),
        },
        "upgrade" => match value.to_ascii_lowercase().as_str() {
            "open" | "opened" => Ok(Assertion::UpgradeOpen(true)),
            "closed" | "none" => Ok(Assertion::UpgradeOpen(false)),
            other => Err(format!(
                "assert upgrade: ожидалось open|closed, получено «{other}»"
            )),
        },
        "gold" => Ok(Assertion::Gold(
            operator,
            parse_u32(value, "assert gold")?,
        )),
        "unit" => Ok(Assertion::UnitType(value.to_string())),
        "cargo-mode" => match value.to_ascii_lowercase().as_str() {
            "buy" | "purchase" => Ok(Assertion::CargoMode(true)),
            "upgrade" | "upgrades" => Ok(Assertion::CargoMode(false)),
            other => Err(format!(
                "assert cargo-mode: ожидалось buy|upgrade, получено «{other}»"
            )),
        },
        "cargo-types" => Ok(Assertion::CargoTypes(
            operator,
            parse_u32(value, "assert cargo-types")?,
        )),
        "cargo-credits" => Ok(Assertion::CargoCredits(
            operator,
            parse_u32(value, "assert cargo-credits")?,
        )),
        "cargo-description" => Ok(Assertion::CargoDescription(
            operator,
            parse_u32(value, "assert cargo-description")?,
        )),
        "cargo-stat-icons" => Ok(Assertion::CargoStatIcons(
            operator,
            parse_u32(value, "assert cargo-stat-icons")?,
        )),
        "cargo-portrait" => match value.to_ascii_lowercase().as_str() {
            "yes" | "true" | "1" => Ok(Assertion::CargoPortrait(true)),
            "no" | "false" | "0" => Ok(Assertion::CargoPortrait(false)),
            other => Err(format!(
                "assert cargo-portrait: ожидалось yes|no, получено «{other}»"
            )),
        },
        "digest" => {
            let digits = value.trim_start_matches("0x").trim_start_matches("0X");
            let digest = u16::from_str_radix(digits, 16)
                .map_err(|_| format!("assert digest: ожидалось hex16, получено «{value}»"))?;
            Ok(Assertion::Digest(digest))
        }
        other if other.starts_with("research ") => {
            let topic = match other["research ".len()..].trim() {
                "attack" => max_core::enums::ResearchTopic::Attack,
                "shots" => max_core::enums::ResearchTopic::Shots,
                "range" => max_core::enums::ResearchTopic::Range,
                "armor" => max_core::enums::ResearchTopic::Armor,
                "hits" => max_core::enums::ResearchTopic::Hits,
                "speed" => max_core::enums::ResearchTopic::Speed,
                "scan" => max_core::enums::ResearchTopic::Scan,
                "cost" => max_core::enums::ResearchTopic::Cost,
                other => {
                    return Err(format!(
                        "assert research: неизвестная тема «{other}» (attack|shots|range|armor|hits|speed|scan|cost)"
                    ))
                }
            };
            Ok(Assertion::ResearchLevel(
                topic,
                operator,
                parse_u32(value, "assert research")?,
            ))
        }
        other => Err(format!(
            "assert: неизвестная проверка «{other}» (turn|team|units|screen|phase|notice|digest|victory|defeat|selected)"
        )),
    }
}

fn parse_menu(rest: &str) -> Result<MenuTarget, String> {
    match rest.trim().to_ascii_lowercase().as_str() {
        "new_game" | "new" | "newgame" => Ok(MenuTarget::NewGame),
        "multiplayer" | "multi" => Ok(MenuTarget::Multiplayer),
        "hot_seat" | "hotseat" | "hot" => Ok(MenuTarget::HotSeat),
        "join" => Ok(MenuTarget::Join),
        other => Err(format!(
            "menu: неизвестный пункт «{other}» (new_game|multiplayer|hot_seat|join)"
        )),
    }
}

fn parse_setup(rest: &str) -> Result<SetupAction, String> {
    match rest.trim().to_ascii_lowercase().as_str() {
        "done" | "ok" => Ok(SetupAction::Done),
        "cancel" | "back" => Ok(SetupAction::Cancel),
        "random" => Ok(SetupAction::Random),
        other => Err(format!(
            "setup: неизвестное действие «{other}» (done|cancel|random)"
        )),
    }
}

fn parse_players(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.len() != 2 {
        return Err(format!(
            "players: ожидалось «SLOT human|computer|none», получено «{rest}»"
        ));
    }

    let slot = parse_u32(parts[0], "players: слот")?;
    if slot > 3 {
        return Err(format!("players: слот {slot} (всего 4: 0..=3)"));
    }

    Ok(Command::Players {
        slot: slot as u8,
        kind: parse_team_kind(parts[1])?,
    })
}

fn parse_mobilize(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.is_empty() {
        return Err("mobilize: требуется действие".to_string());
    }

    let action = match parts[0].to_ascii_lowercase().as_str() {
        "buy" => match parts.get(1) {
            Some(index) => {
                if parts.len() > 2 {
                    return Err(format!("mobilize buy: лишние аргументы «{rest}»"));
                }
                MobilizeAction::BuyIndex(parse_u32(index, "mobilize buy")?)
            }
            None => MobilizeAction::Buy,
        },
        "delete" => MobilizeAction::Delete,
        "done" => MobilizeAction::Done,
        "cancel" => MobilizeAction::Cancel,
        "step" => {
            let value = parts
                .get(1)
                .ok_or_else(|| "mobilize step: требуется число".to_string())?;

            MobilizeAction::Step(parse_i32(value, "mobilize step")?)
        }
        "mode" => match parts.get(1).map(|part| part.to_ascii_lowercase()) {
            Some(word) if word == "buy" || word == "purchase" => MobilizeAction::Mode(true),
            Some(word) if word == "upgrade" || word == "upgrades" => MobilizeAction::Mode(false),
            _ => {
                return Err(format!(
                    "mobilize mode: ожидалось buy|upgrade, получено «{rest}»"
                ))
            }
        },
        "filter" => {
            let name = parts
                .get(1)
                .and_then(|part| FilterName::parse(part))
                .ok_or_else(|| {
                    format!(
                        "mobilize filter: ожидалось ground|air|sea|building|combat, получено «{rest}»"
                    )
                })?;
            MobilizeAction::Filter(name)
        }
        "select" => MobilizeAction::Select(parse_u32(
            parts
                .get(1)
                .ok_or_else(|| "mobilize select: требуется индекс".to_string())?,
            "mobilize select",
        )?),
        "loadout" => MobilizeAction::Loadout(parse_u32(
            parts
                .get(1)
                .ok_or_else(|| "mobilize loadout: требуется индекс".to_string())?,
            "mobilize loadout",
        )?),
        "row" => {
            if parts.len() != 3 {
                return Err(format!(
                    "mobilize row: ожидалось «mobilize row N up|down», получено «{rest}»"
                ));
            }
            let row = parse_u32(parts[1], "mobilize row")?;
            let increase = match parts[2].to_ascii_lowercase().as_str() {
                "up" | "increase" | "+" => true,
                "down" | "decrease" | "-" => false,
                other => {
                    return Err(format!(
                        "mobilize row: ожидалось up|down, получено «{other}»"
                    ))
                }
            };
            MobilizeAction::Adjust { row, increase }
        }
        "cargo" => MobilizeAction::Cargo(parse_u32(
            parts
                .get(1)
                .ok_or_else(|| "mobilize cargo: требуется число блоков".to_string())?,
            "mobilize cargo",
        )?),
        other => {
            return Err(format!(
                "mobilize: ожидалось buy [N]|delete|step N|mode buy|upgrade|filter NAME|select N|loadout N|row N up|down|cargo N|done|cancel, получено «{other}»"
            ))
        }
    };

    Ok(Command::Mobilize(action))
}

fn parse_landing(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.is_empty() {
        return Err("landing: требуется действие".to_string());
    }

    let command = match parts[0].to_ascii_lowercase().as_str() {
        "click" => {
            if parts.len() != 3 {
                return Err(format!(
                    "landing click: ожидалось «landing click X Y», получено «{rest}»"
                ));
            }

            LandingCommand::Click(
                parse_i32(parts[1], "landing click X")?,
                parse_i32(parts[2], "landing click Y")?,
            )
        }
        "grid" => {
            if parts.len() != 3 {
                return Err(format!(
                    "landing grid: ожидалось «landing grid X Y», получено «{rest}»"
                ));
            }

            let x = parse_i32(parts[1], "landing grid X")?;
            let y = parse_i32(parts[2], "landing grid Y")?;

            if !(0..112).contains(&x) || !(0..112).contains(&y) {
                return Err(format!("landing grid: клетка ({x}, {y}) вне карты 112×112"));
            }

            LandingCommand::Grid(x as i16, y as i16)
        }
        "done" => LandingCommand::Done,
        "cancel" => LandingCommand::Cancel,
        other => {
            return Err(format!(
                "landing: неизвестное действие «{other}» (click|grid|done|cancel)"
            ))
        }
    };

    Ok(Command::Landing(command))
}

fn parse_build(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = parts.first().map(|part| part.to_ascii_lowercase()) else {
        return Err("build: ожидалось действие (open|select N|add|delete|rate N|repeat|step N|queue step N|done|cancel)".to_string());
    };

    let command = match (first.as_str(), parts.as_slice()) {
        ("open", [_]) => BuildCommand::Open,
        ("select", [_, index]) => BuildCommand::Select(parse_u32(index, "build select")?),
        ("add", [_]) => BuildCommand::Add,
        ("delete", [_]) => BuildCommand::Delete,
        ("rate", [_, rate]) => {
            let rate = parse_u32(rate, "build rate")?;

            if ![1, 2, 4].contains(&rate) {
                return Err(format!("build rate: скорость {rate} не из 1/2/4"));
            }

            BuildCommand::Rate(rate as u16)
        }
        ("repeat", [_]) => BuildCommand::Repeat,
        ("step", [_, delta]) => BuildCommand::Step(parse_i32(delta, "build step")?),
        ("queue", [_, step, delta]) if step.eq_ignore_ascii_case("step") => {
            BuildCommand::QueueStep(parse_i32(delta, "build queue step")?)
        }
        ("done", [_]) => BuildCommand::Done,
        ("cancel", [_]) => BuildCommand::Cancel,
        _ => {
            return Err(format!(
                "build: неизвестное действие «{rest}» (open|select N|add|delete|rate N|repeat|step N|queue step N|done|cancel)"
            ))
        }
    };

    Ok(Command::Build(command))
}

fn parse_upgrade(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let Some(first) = parts.first().map(|part| part.to_ascii_lowercase()) else {
        return Err(
            "upgrade: ожидалось действие (open|select N|row N up|down|done|cancel)".to_string(),
        );
    };

    let command = match (first.as_str(), parts.as_slice()) {
        ("open", [_]) => UpgradeCommand::Open,
        ("select", [_, index]) => UpgradeCommand::Select(parse_u32(index, "upgrade select")?),
        ("row", [_, row, direction]) => {
            let row = parse_u32(row, "upgrade row")?;
            let increase = match direction.to_ascii_lowercase().as_str() {
                "up" | "inc" | "increase" => true,
                "down" | "dec" | "decrease" => false,
                other => {
                    return Err(format!(
                        "upgrade row: ожидалось up|down, получено «{other}»"
                    ))
                }
            };
            UpgradeCommand::Adjust { row, increase }
        }
        ("done", [_]) => UpgradeCommand::Done,
        ("cancel", [_]) => UpgradeCommand::Cancel,
        _ => {
            return Err(format!(
                "upgrade: неизвестное действие «{rest}» (open|select N|row N up|down|done|cancel)"
            ))
        }
    };

    Ok(Command::Upgrade(command))
}

fn parse_select(rest: &str) -> Result<Command, String> {    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.first().is_some_and(|part| part.eq_ignore_ascii_case("unit")) {
        if parts.len() != 2 {
            return Err(format!(
                "select unit: ожидалось «select unit N», получено «{rest}»"
            ));
        }

        return Ok(Command::SelectUnit(parse_u32(parts[1], "select unit")?));
    }

    if parts.first().is_some_and(|part| part.eq_ignore_ascii_case("last")) {
        if parts.len() != 1 {
            return Err(format!("select last: лишние аргументы «{rest}»"));
        }

        return Ok(Command::SelectLast);
    }

    if parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("type"))
    {
        let name = rest
            .trim_start_matches(|character: char| character.is_ascii_alphabetic())
            .trim();

        if name.is_empty() {
            return Err(format!(
                "select type: ожидалось «select type NAME», получено «{rest}»"
            ));
        }

        return Ok(Command::SelectType(name.to_string()));
    }

    let (x, y) = parse_point(rest)?;

    Ok(Command::Select(x, y))
}

fn parse_move(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    match parts.first().map(|part| part.to_ascii_lowercase()) {
        Some(word) if word == "near" => {
            if parts.len() != 1 {
                return Err(format!("move near: лишние аргументы «{rest}»"));
            }
            Ok(Command::MoveNear)
        }
        Some(word) if word == "grid" => {
            if parts.len() != 3 {
                return Err(format!(
                    "move grid: ожидалось «move grid X Y», получено «{rest}»"
                ));
            }

            let x = parse_i32(parts[1], "move grid X")?;
            let y = parse_i32(parts[2], "move grid Y")?;

            if !(0..112).contains(&x) || !(0..112).contains(&y) {
                return Err(format!("move grid: клетка ({x}, {y}) вне карты 112×112"));
            }

            Ok(Command::MoveGrid(x as i16, y as i16))
        }
        _ => {
            let (x, y) = parse_point(rest)?;
            Ok(Command::Move(x, y))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(source: &str) -> Command {
        let steps = parse_script(source).expect("скрипт разбирается");
        assert_eq!(steps.len(), 1, "ожидалась одна команда: {source}");
        steps.into_iter().next().expect("шаг").command
    }

    #[test]
    fn every_low_level_command_is_parsed() {
        assert_eq!(one("click 10 20"), Command::Click(10, 20));
        assert_eq!(one("rclick 639 479"), Command::RightClick(639, 479));
        assert_eq!(one("key ENTER"), Command::Key(Key::Enter));
        assert_eq!(one("key esc"), Command::Key(Key::Escape));
        assert_eq!(one("key t"), Command::Key(Key::Char('t')));
        assert_eq!(
            one("type \"Hello, world\""),
            Command::Type("Hello, world".to_string())
        );
        assert_eq!(one("wait 40"), Command::Wait(40));
        assert_eq!(one("tick 3"), Command::Tick(3));
        assert_eq!(one("mem"), Command::Mem);
        assert_eq!(one("shot main-menu"), Command::Shot("main-menu".to_string()));
        assert_eq!(one("save sp.mxs"), Command::Save("sp.mxs".to_string()));
        assert_eq!(
            one("load SAVE1.CAM"),
            Command::Load("SAVE1.CAM".to_string())
        );
        assert_eq!(one("exit"), Command::Exit);
    }

    #[test]
    fn every_assertion_is_parsed() {
        assert_eq!(
            one("assert turn=3"),
            Command::Assert(Assertion::Turn(Cmp::Eq, 3))
        );
        assert_eq!(
            one("assert turn>=3"),
            Command::Assert(Assertion::Turn(Cmp::Ge, 3))
        );
        assert_eq!(
            one("assert turn<=3"),
            Command::Assert(Assertion::Turn(Cmp::Le, 3))
        );
        assert_eq!(
            one("assert units>4"),
            Command::Assert(Assertion::Units(Cmp::Gt, 4))
        );
        assert_eq!(
            one("assert team=1"),
            Command::Assert(Assertion::Team(1))
        );
        assert_eq!(
            one("assert team=RED"),
            Command::Assert(Assertion::Team(0))
        );
        assert_eq!(
            one("assert screen=InGame"),
            Command::Assert(Assertion::Screen(ScreenName::InGame))
        );
        assert_eq!(
            one("assert phase=PlayerTurn"),
            Command::Assert(Assertion::Phase(PhaseName::PlayerTurn))
        );
        assert_eq!(
            one("assert notice=GREEN"),
            Command::Assert(Assertion::Notice(Some(1)))
        );
        assert_eq!(
            one("assert notice=none"),
            Command::Assert(Assertion::Notice(None))
        );
        assert_eq!(one("assert victory"), Command::Assert(Assertion::Victory));
        assert_eq!(one("assert defeat"), Command::Assert(Assertion::Defeat));
        assert_eq!(one("assert selected"), Command::Assert(Assertion::Selected));
        assert_eq!(
            one("assert digest=0xBEEF"),
            Command::Assert(Assertion::Digest(0xBEEF))
        );
    }

    #[test]
    fn high_level_commands_are_parsed() {
        assert_eq!(one("menu new_game"), Command::Menu(MenuTarget::NewGame));
        assert_eq!(one("menu multiplayer"), Command::Menu(MenuTarget::Multiplayer));
        assert_eq!(one("menu hot_seat"), Command::Menu(MenuTarget::HotSeat));
        assert_eq!(one("menu join"), Command::Menu(MenuTarget::Join));
        assert_eq!(one("setup done"), Command::Setup(SetupAction::Done));
        assert_eq!(one("setup cancel"), Command::Setup(SetupAction::Cancel));
        assert_eq!(one("setup random"), Command::Setup(SetupAction::Random));
        assert_eq!(
            one("players 2 none"),
            Command::Players {
                slot: 2,
                kind: TeamKind::None
            }
        );
        assert_eq!(
            one("players 1 computer"),
            Command::Players {
                slot: 1,
                kind: TeamKind::Computer
            }
        );
        assert_eq!(
            one("players 0 human"),
            Command::Players {
                slot: 0,
                kind: TeamKind::Player
            }
        );
        assert_eq!(one("select 404 242"), Command::Select(404, 242));
        assert_eq!(one("select unit 2"), Command::SelectUnit(2));
        assert_eq!(one("move 468 306"), Command::Move(468, 306));
        assert_eq!(one("move grid 24 25"), Command::MoveGrid(24, 25));
        assert_eq!(one("move near"), Command::MoveNear);
        assert_eq!(one("end_turn"), Command::EndTurn);
    }

    #[test]
    fn upgrade_and_research_commands_parse() {
        let steps = parse_script(
            "upgrade open\n\
             upgrade select 3\n\
             upgrade row 0 up\n\
             upgrade row 1 down\n\
             upgrade done\n\
             upgrade cancel\n\
             assert upgrade=open\n\
             assert gold<150\n\
             assert research attack>=1\n",
        )
        .expect("P6-команды разбираются");

        assert_eq!(steps[0].command, Command::Upgrade(UpgradeCommand::Open));
        assert_eq!(
            steps[1].command,
            Command::Upgrade(UpgradeCommand::Select(3))
        );
        assert_eq!(
            steps[2].command,
            Command::Upgrade(UpgradeCommand::Adjust {
                row: 0,
                increase: true
            })
        );
        assert_eq!(
            steps[3].command,
            Command::Upgrade(UpgradeCommand::Adjust {
                row: 1,
                increase: false
            })
        );
        assert_eq!(steps[6].command, Command::Assert(Assertion::UpgradeOpen(true)));
        assert_eq!(
            steps[7].command,
            Command::Assert(Assertion::Gold(Cmp::Lt, 150))
        );
        assert_eq!(
            steps[8].command,
            Command::Assert(Assertion::ResearchLevel(
                max_core::enums::ResearchTopic::Attack,
                Cmp::Ge,
                1
            ))
        );
    }

    #[test]
    fn cargo_menu_commands_and_assertions_parse() {
        let steps = parse_script(
            "mobilize mode upgrade\n\
             mobilize mode buy\n\
             mobilize filter ground\n\
             mobilize filter combat\n\
             mobilize select 3\n\
             mobilize loadout 0\n\
             mobilize row 0 up\n\
             mobilize row 1 down\n\
             mobilize cargo 4\n\
             mobilize buy 14\n\
             mobilize delete\n\
             assert cargo-mode=upgrade\n\
             assert cargo-types=15\n\
             assert cargo-credits>=100\n\
             assert cargo-description>=100\n\
             assert cargo-stat-icons>=10\n\
             assert cargo-portrait=yes\n",
        )
        .expect("P7-команды разбираются");

        assert_eq!(
            steps[0].command,
            Command::Mobilize(MobilizeAction::Mode(false))
        );
        assert_eq!(
            steps[1].command,
            Command::Mobilize(MobilizeAction::Mode(true))
        );
        assert_eq!(
            steps[2].command,
            Command::Mobilize(MobilizeAction::Filter(FilterName::Ground))
        );
        assert_eq!(
            steps[3].command,
            Command::Mobilize(MobilizeAction::Filter(FilterName::Combat))
        );
        assert_eq!(
            steps[4].command,
            Command::Mobilize(MobilizeAction::Select(3))
        );
        assert_eq!(
            steps[5].command,
            Command::Mobilize(MobilizeAction::Loadout(0))
        );
        assert_eq!(
            steps[6].command,
            Command::Mobilize(MobilizeAction::Adjust {
                row: 0,
                increase: true
            })
        );
        assert_eq!(
            steps[7].command,
            Command::Mobilize(MobilizeAction::Adjust {
                row: 1,
                increase: false
            })
        );
        assert_eq!(
            steps[8].command,
            Command::Mobilize(MobilizeAction::Cargo(4))
        );
        assert_eq!(
            steps[9].command,
            Command::Mobilize(MobilizeAction::BuyIndex(14))
        );
        assert_eq!(
            steps[10].command,
            Command::Mobilize(MobilizeAction::Delete)
        );
        assert_eq!(
            steps[11].command,
            Command::Assert(Assertion::CargoMode(false))
        );
        assert_eq!(
            steps[12].command,
            Command::Assert(Assertion::CargoTypes(Cmp::Eq, 15))
        );
        assert_eq!(
            steps[13].command,
            Command::Assert(Assertion::CargoCredits(Cmp::Ge, 100))
        );
        assert_eq!(
            steps[14].command,
            Command::Assert(Assertion::CargoDescription(Cmp::Ge, 100))
        );
        assert_eq!(
            steps[15].command,
            Command::Assert(Assertion::CargoStatIcons(Cmp::Ge, 10))
        );
        assert_eq!(
            steps[16].command,
            Command::Assert(Assertion::CargoPortrait(true))
        );

        assert!(parse_script("mobilize filter water\n").is_err());
        assert!(parse_script("mobilize mode neither\n").is_err());
        assert!(parse_script("assert cargo-mode=neither\n").is_err());
        assert!(parse_script("assert cargo-portrait=maybe\n").is_err());
    }

    #[test]
    fn comments_and_empty_lines_are_skipped() {
        let script = "\
# заголовок
click 1 2  # клик по кнопке
\n
type \"a#b\"  # решётка в кавычках — часть текста
";

        let steps = parse_script(script).expect("разбирается");

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].line, 2);
        assert_eq!(steps[0].command, Command::Click(1, 2));
        assert_eq!(steps[1].command, Command::Type("a#b".to_string()));
    }

    #[test]
    fn step_numbers_point_at_the_source_line() {
        let script = "# c\n\nclick 1 2\n\nshot a\n";

        let steps = parse_script(script).expect("разбирается");

        assert_eq!(steps[0].line, 3);
        assert_eq!(steps[1].line, 5);
    }

    #[test]
    fn errors_name_the_line() {
        let error = parse_script("click 1 2\nbogus 3\n").expect_err("опечатка ловится");

        assert!(error.contains("строка 2"), "{error}");
        assert!(error.contains("bogus"), "{error}");
    }

    #[test]
    fn out_of_frame_and_bad_arguments_are_rejected() {
        for source in [
            "click 640 10",
            "click -1 10",
            "click 10 480",
            "click 10",
            "click 10 20 30",
            "key",
            "key F13",
            "type hello world",
            "type \"unterminated",
            "wait",
            "mem 1",
            "shot a/b",
            "shot ..",
            "save ../x.mxs",
            "load dir/SAVE.CAM",
            "assert",
            "assert turn",
            "assert turn=abc",
            "assert units>=-2",
            "assert team=9",
            "assert screen=Nope",
            "assert phase=Nope",
            "assert digest=zzz",
            "assert nope=1",
            "menu nope",
            "setup nope",
            "players 9 human",
            "players 0 alien",
            "players 0",
            "select 900 10",
            "select unit",
            "move grid 112 0",
            "move grid 1",
            "move near extra",
            "end_turn now",
            "exit now",
        ] {
            assert!(
                parse_script(&format!("{source}\n")).is_err(),
                "должно быть отклонено: {source}"
            );
        }
    }

    #[test]
    fn cmp_tests_match_the_operator() {
        assert!(Cmp::Eq.test(3, 3));
        assert!(!Cmp::Eq.test(3, 4));
        assert!(Cmp::Ge.test(3, 3));
        assert!(!Cmp::Ge.test(2, 3));
        assert!(Cmp::Le.test(2, 3));
        assert!(Cmp::Gt.test(4, 3));
        assert!(Cmp::Lt.test(2, 3));
        assert_eq!(Cmp::Ge.as_str(), ">=");
    }
}
