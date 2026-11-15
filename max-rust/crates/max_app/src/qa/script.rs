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
    /// Любой экран `Screen::LocalSetup` (не важно, какой из трёх).
    LocalSetup,
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
            "localsetup" | "local_setup" | "setup" => Some(Self::LocalSetup),
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
            Self::LocalSetup => "LocalSetup",
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// `assert digest=HEX`: CRC16 снапшота состояния (`App::digest`).
    Digest(u16),
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
    /// `shot NAME` — PNG кадра в каталог вывода.
    Shot(String),
    /// `assert …`.
    Assert(Assertion),
    /// `save FILE` — контейнер `MXLS` (`App::write_local_save`).
    Save(String),
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
    /// `move X Y` — правый клик по точке кадра + проверка приказа.
    Move(i32, i32),
    /// `move grid X Y` — правый клик по клетке карты.
    MoveGrid(i16, i16),
    /// `move near` — приказ в соседнюю свободную клетку выделенного юнита.
    MoveNear,
    /// `end_turn` — кнопка End Turn + ходы ИИ до готовности к вводу.
    EndTurn,
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
        "shot" => Ok(Command::Shot(parse_name(rest, "shot")?)),
        "assert" => Ok(Command::Assert(parse_assertion(rest)?)),
        "save" => Ok(Command::Save(parse_name(rest, "save")?)),
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
        _ => {
            let mut characters = name.chars();

            match (characters.next(), characters.next()) {
                (Some(character), None) if character.is_ascii_graphic() => Key::Char(character),
                _ => {
                    return Err(format!(
                        "key: неизвестная клавиша «{name}» (ENTER/ESC/TAB/BACKSPACE/UP/DOWN/LEFT/RIGHT или один символ)"
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
        "digest" => {
            let digits = value.trim_start_matches("0x").trim_start_matches("0X");
            let digest = u16::from_str_radix(digits, 16)
                .map_err(|_| format!("assert digest: ожидалось hex16, получено «{value}»"))?;
            Ok(Assertion::Digest(digest))
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

fn parse_select(rest: &str) -> Result<Command, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.first().is_some_and(|part| part.eq_ignore_ascii_case("unit")) {
        if parts.len() != 2 {
            return Err(format!(
                "select unit: ожидалось «select unit N», получено «{rest}»"
            ));
        }

        return Ok(Command::SelectUnit(parse_u32(parts[1], "select unit")?));
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
