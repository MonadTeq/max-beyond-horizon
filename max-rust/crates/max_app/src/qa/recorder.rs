//! Запись `.qa`-сценария вручную (слой 2.5 QA): ввод игрока в окне → команды
//! драйвера [`crate::qa::runner`].
//!
//! Идея: в `main.rs` SDL-событие уже переведено в [`InputEvent`] **в пикселях
//! кадра 640×480** (`SdlWindow::poll_input` делает обратный пересчёт масштаба
//! окна), поэтому рекордеру не нужен ни SDL, ни знание о раскладке окна. Он
//! получает тот же поток событий, что уходит в `Screen::handle_input` /
//! `handle_session_input`, и превращает его в строки языка `.qa`:
//!
//! | Ввод | Команда |
//! |---|---|
//! | LMB по точке кадра | `click X Y` |
//! | RMB по точке кадра | `rclick X Y` |
//! | Enter/Esc/Tab/Backspace/стрелки | `key ENTER`, `key ESC`, … |
//! | печатный символ вне текстового ввода | `key A` (ровно нажатие) |
//! | символы внутри открытого текстового ввода (чат: T/Enter в сессии) | одна `type "…"` |
//! | пауза между событиями | `wait N`, `N = min(кадров, cap)` |
//!
//! # Почему запись воспроизводима
//!
//! Координаты — пиксели кадра, а не окна: при `--scale 3` клик в окне
//! 1920×1440 всё равно запишется как точка 640×480. Скрипт запускается тем же
//! `qa_run` и тем же стартовым состоянием (та же планета/seed/настройки),
//! поэтому проходит тот же путь игрока.
//!
//! # Ограничения (осознанные)
//!
//! * **`type` — только вставка символов**: Backspace не входит в грамматику
//!   `type`. Рекордер сбрасывает накопленный текст отдельной `type`, а сам
//!   Backspace пишет как `key BACKSPACE`; порядок ввода сохраняется, но
//!   удаление «в середине» воспроизводится как отдельная команда, а не как
//!   правка внутри `type`.
//! * **Движение мыши не пишется**: `MouseMove` не влияет на состояние игры
//!   (только подсветка), иначе скрипт распух бы на тысячу строк.
//! * **`Key::Other`** (модификаторы, F-клавиши) не имеет команды в языке —
//!   пропускается.
//! * **Пробел и управляющие символы вне текстового ввода** не выражаются
//!   однобуквенным `key` — пропускаются (внутри `type` пробел пишется).
//! * **Клик вне кадра** (чёрные поля при масштабировании) даёт `(-1, -1)`
//!   от `poll_input` и не пишется — парсер такие координаты отклонил бы.
//! * **Сетевую сессию** записать можно, но `qa_run` не поднимает сеть:
//!   воспроизводимы локальные партии (`--ui` → New Game). Для сети запись —
//!   справочный артефакт.
//! * Если выход случился при открытом текстовом вводе, файл заканчивается
//!   `type` без закрывающего `key` — на реплее строка останется открытой;
//!   QA-инженер дописывает `key ENTER`/`key ESC` вручную.
//!
//! Заголовок файла — комментарии (`#`), поэтому `qa::script` разбирает файл
//! целиком, без предварительной обработки.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::qa::script::{FRAME_HEIGHT, FRAME_WIDTH};
use crate::ui::{InputEvent, Key};

/// Сколько кадров паузы максимум пишется в `wait N`.
///
/// Игрок «думает» секундами, а прогон CI не должен растягиваться: 120 кадров —
/// примерно 2 секунды при 60 FPS. Всё, что дольше, режется до этого значения.
pub const DEFAULT_PAUSE_CAP: u32 = 120;

/// Что рекордер знает о состоянии UI на момент события.
///
/// `text_open` — авторитетный признак открытого текстового ввода
/// (`App::hud().chat.is_active()` в сессии). Пока он `true`, печатные символы
/// копятся в одну команду `type`; открытие/закрытие ввода (`T`/`Enter`/`Esc`)
/// остаётся отдельными `key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordContext {
    /// Идёт партия (`Screen::InSession` / сетевой `FrameSink`).
    pub in_session: bool,
    /// Текстовый ввод открыт **до** этого события.
    pub text_open: bool,
}

/// Запись действий игрока в `.qa`-файл.
///
/// Пишет построчно и сбрасывает буфер на диск после каждой команды: если
/// приложение упадёт, уже записанные шаги останутся в файле. [`Recorder::finish`]
/// дописывает хвост (незакрытый `type`, паузу после последнего события,
/// финальный комментарий).
pub struct Recorder {
    path: Option<PathBuf>,
    out: Option<BufWriter<File>>,
    header: Vec<String>,
    commands: Vec<String>,
    frame: u64,
    last_command_frame: Option<u64>,
    text: String,
    text_start_frame: Option<u64>,
    text_last_frame: Option<u64>,
    pause_cap: u32,
    events: usize,
    write_error: Option<String>,
    finished: bool,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Recorder")
            .field("path", &self.path)
            .field("frame", &self.frame)
            .field("events", &self.events)
            .field("commands", &self.commands.len())
            .finish()
    }
}

impl Recorder {
    /// Создаёт файл записи.
    ///
    /// Если `path` уже существует, берётся свободное имя с суффиксом:
    /// `run.qa` → `run-1.qa` → `run-2.qa` … (существующий файл никогда не
    /// перезаписывается без явного согласия пользователя).
    pub fn new(path: impl Into<PathBuf>) -> io::Result<Self> {
        let requested = path.into();
        let path = unique_path(&requested);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut recorder = Self::with_path(Some(path), Some(BufWriter::new(file)));

        recorder.write_header()?;

        Ok(recorder)
    }

    /// Рекордер в память (для тестов и API без файла).
    pub fn in_memory() -> Self {
        Self::with_path(None, None)
    }

    fn with_path(path: Option<PathBuf>, out: Option<BufWriter<File>>) -> Self {
        Self {
            path,
            out,
            header: header_lines(),
            commands: Vec::new(),
            frame: 0,
            last_command_frame: None,
            text: String::new(),
            text_start_frame: None,
            text_last_frame: None,
            pause_cap: DEFAULT_PAUSE_CAP,
            events: 0,
            write_error: None,
            finished: false,
        }
    }

    /// Другой cap паузы (кадры; `0` — паузы не писать).
    pub fn with_pause_cap(mut self, cap: u32) -> Self {
        self.pause_cap = cap;
        self
    }

    /// Файл, в который идёт запись (`None` — запись в память).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Заголовок файла (строки-комментарии без завершающего `\n`).
    pub fn header(&self) -> &[String] {
        &self.header
    }

    /// Записанные команды (без заголовка и финального комментария).
    pub fn commands(&self) -> &[String] {
        &self.commands
    }

    /// Сколько значимых событий увидел рекордер.
    pub fn events(&self) -> usize {
        self.events
    }

    /// Текущий номер кадра (для диагностики/тестов).
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Ошибка записи на диск, если она была (первая).
    pub fn write_error(&self) -> Option<&str> {
        self.write_error.as_deref()
    }

    /// Начался новый кадр приложения.
    ///
    /// Вызывается ровно один раз за итерацию игрового цикла: паузы в `wait`
    /// считаются как разница номеров кадров между событиями.
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.saturating_add(1);
    }

    /// Записывает одно событие ввода.
    ///
    /// `context` должен отражать состояние UI **до** применения события
    /// (в `main.rs` — `app.hud().chat.is_active()`). Незначимые события
    /// (движение мыши, клик вне кадра, `Key::Other`) не создают команд и не
    /// сдвигают отсчёт паузы.
    pub fn record(&mut self, event: InputEvent, context: RecordContext) {
        match event {
            InputEvent::MouseMove(..) => {}
            // Пока открыт чат, мышь игнорируется самим UI — не пишем её и мы.
            InputEvent::MouseClick(..) | InputEvent::RightClick(..) if context.text_open => {}
            InputEvent::MouseClick(x, y) => self.record_click("click", x, y),
            InputEvent::RightClick(x, y) => self.record_click("rclick", x, y),
            InputEvent::Key(key) if context.text_open => self.record_text_key(key),
            InputEvent::Key(key) => self.record_key(key),
        }
    }

    /// Завершает запись: сбрасывает открытый `type`, пишет паузу после
    /// последнего события и финальный комментарий.
    ///
    /// Возвращает путь записанного файла (`None` для записи в память).
    pub fn finish(&mut self) -> io::Result<Option<PathBuf>> {
        if !self.finished {
            self.flush_text();
            self.flush_trailing_wait();

            let summary = format!(
                "# конец записи: событий {}, команд {}, кадров {}",
                self.events,
                self.commands.len(),
                self.frame
            );
            self.push_comment(summary);

            self.finished = true;
        }

        if let Some(out) = self.out.as_mut() {
            out.flush()?;
        }

        if let Some(error) = self.write_error.take() {
            return Err(io::Error::other(error));
        }

        Ok(self.path.clone())
    }

    // -----------------------------------------------------------------------
    // Команды
    // -----------------------------------------------------------------------

    fn record_click(&mut self, name: &str, x: i32, y: i32) {
        if !(0..FRAME_WIDTH).contains(&x) || !(0..FRAME_HEIGHT).contains(&y) {
            return;
        }

        self.flush_text();
        self.push_timed(format!("{name} {x} {y}"));
        self.events += 1;
    }

    fn record_key(&mut self, key: Key) {
        let Some(line) = key_line(key) else {
            return;
        };

        self.flush_text();
        self.push_timed(line);
        self.events += 1;
    }

    fn record_text_key(&mut self, key: Key) {
        match key {
            Key::Char(character) if is_typeable(character) => {
                self.push_text_char(character);
            }
            Key::Char(_) => {}
            Key::Backspace => {
                self.flush_text();
                self.push_timed("key BACKSPACE".to_string());
                self.events += 1;
            }
            Key::Enter => {
                self.flush_text();
                self.push_timed("key ENTER".to_string());
                self.events += 1;
            }
            Key::Escape => {
                self.flush_text();
                self.push_timed("key ESC".to_string());
                self.events += 1;
            }
            Key::Tab | Key::Up | Key::Down | Key::Left | Key::Right | Key::PageUp | Key::PageDown => {
                if let Some(line) = key_line(key) {
                    self.flush_text();
                    self.push_timed(line);
                    self.events += 1;
                }
            }
            Key::Alt(_) => {}
            Key::Other => {}
        }
    }

    fn push_text_char(&mut self, character: char) {
        if self.text.is_empty() {
            self.text_start_frame = Some(self.frame);
        }

        self.text.push(character);
        self.text_last_frame = Some(self.frame);
        self.events += 1;
    }

    /// Сбрасывает накопленный текст в `type "…"`.
    ///
    /// Пауза перед началом набора пишется **перед** `type`, а пауза после
    /// последнего символа останется у следующей команды — так реплей
    /// сохраняет и «долго думал перед вводом», и «долго думал после».
    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }

        let text = std::mem::take(&mut self.text);
        let Some(start) = self.text_start_frame.take() else {
            return;
        };
        let end = self.text_last_frame.take().unwrap_or(start);

        self.push_wait_to(start);
        // Кавычки внутри текста parse_text сохраняет: он срезает только
        // внешние, поэтому `type "…"` кодирует любой ASCII из чата.
        self.push_line(format!("type \"{text}\""));
        self.last_command_frame = Some(end);
    }

    fn push_timed(&mut self, line: String) {
        self.push_wait_to(self.frame);
        self.push_line(line);
        self.last_command_frame = Some(self.frame);
    }

    fn push_wait_to(&mut self, target: u64) {
        let Some(last) = self.last_command_frame else {
            return;
        };

        let delta = target.saturating_sub(last);

        if delta == 0 || self.pause_cap == 0 {
            return;
        }

        self.push_line(format!("wait {}", delta.min(u64::from(self.pause_cap))));
    }

    fn flush_trailing_wait(&mut self) {
        self.push_wait_to(self.frame);
    }

    // -----------------------------------------------------------------------
    // Файл
    // -----------------------------------------------------------------------

    fn write_header(&mut self) -> io::Result<()> {
        let header = self.header.clone();

        for line in &header {
            self.write_raw(line)?;
        }

        Ok(())
    }

    fn push_line(&mut self, line: String) {
        self.commands.push(line.clone());
        self.push_comment(line);
    }

    /// Комментарий/заголовок: пишется в файл, но не в [`Self::commands`].
    fn push_comment(&mut self, line: String) {
        if self.write_error.is_none() {
            if let Err(error) = self.write_raw(&line) {
                self.write_error = Some(error.to_string());
                self.out = None;
            }
        }
    }

    fn write_raw(&mut self, line: &str) -> io::Result<()> {
        let Some(out) = self.out.as_mut() else {
            return Ok(());
        };

        writeln!(out, "{line}")?;
        out.flush()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}

/// Строка `key` для клавиши или `None`, если язык её не выражает.
fn key_line(key: Key) -> Option<String> {
    let name = match key {
        Key::Enter => "ENTER",
        Key::Escape => "ESC",
        Key::Tab => "TAB",
        Key::Backspace => "BACKSPACE",
        Key::Up => "UP",
        Key::Down => "DOWN",
        Key::Left => "LEFT",
        Key::Right => "RIGHT",
        Key::PageUp => "PAGEUP",
        Key::PageDown => "PAGEDOWN",
        Key::Char(character) if character.is_ascii_graphic() => {
            return Some(format!("key {character}"));
        }
        Key::Char(_) | Key::Alt(_) | Key::Other => return None,
    };

    Some(format!("key {name}"))
}

/// Символ можно записать внутрь `type "…"`: печатный ASCII (пробел включён).
fn is_typeable(character: char) -> bool {
    character.is_ascii() && !character.is_ascii_control()
}

/// Свободное имя: существующий файл не перезаписывается, добавляется `-N`.
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("record");
    let extension = path.extension().and_then(|extension| extension.to_str());

    for index in 1..10_000 {
        let name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = parent.join(name);

        if !candidate.exists() {
            return candidate;
        }
    }

    path.to_path_buf()
}

fn header_lines() -> Vec<String> {
    vec![
        "# QA-сценарий записан вручную (max_app --ui --record)".to_string(),
        format!("# дата: {}", utc_timestamp()),
        format!("# билд: {}", build_label()),
        format!(
            "# кадр: {}x{}; координаты кликов — пиксели кадра",
            FRAME_WIDTH, FRAME_HEIGHT
        ),
        "# формат: max-brh-qa/drivers/README.md, раздел «Запись сценариев вручную»".to_string(),
        "# реплей: cargo run -p max_app --example qa_run -- <этот файл> <out_dir>".to_string(),
        "# ассерты и shot QA-инженер добавляет вручную после записи".to_string(),
    ]
}

fn build_label() -> String {
    format!(
        "max_app {} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// UTC-время `YYYY-MM-DD HH:MM:SS UTC` без внешних крейтов.
fn utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let days = (seconds / 86_400) as i64;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);

    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        time / 3600,
        (time / 60) % 60,
        time % 60
    )
}

/// Дни от 1970-01-01 → (год, месяц, день) по алгоритму Говарда Хиннанта.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = (days - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;

    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(recorder: &mut Recorder, event: InputEvent) {
        recorder.record(
            event,
            RecordContext {
                in_session: false,
                text_open: false,
            },
        );
    }

    fn click(recorder: &mut Recorder, x: i32, y: i32) {
        record(recorder, InputEvent::MouseClick(x, y));
    }

    #[test]
    fn clicks_are_written_in_frame_pixels_and_waits_count_frames() {
        let mut recorder = Recorder::in_memory();

        click(&mut recorder, 10, 20);
        recorder.begin_frame();
        recorder.begin_frame();
        click(&mut recorder, 30, 40);
        record(&mut recorder, InputEvent::RightClick(0, 0));

        assert_eq!(
            recorder.commands(),
            [
                "click 10 20",
                "wait 2",
                "click 30 40",
                "rclick 0 0"
            ]
        );
        // Первое событие — без wait: паузы пишутся только между событиями.
        assert_eq!(recorder.events(), 3);
    }

    #[test]
    fn out_of_frame_clicks_and_mouse_motion_are_dropped() {
        let mut recorder = Recorder::in_memory();

        record(&mut recorder, InputEvent::MouseMove(100, 100));
        click(&mut recorder, -1, -1);
        click(&mut recorder, FRAME_WIDTH, 10);
        click(&mut recorder, 10, FRAME_HEIGHT);
        click(&mut recorder, -5, 20);

        assert!(recorder.commands().is_empty());
        assert_eq!(recorder.events(), 0);
    }

    #[test]
    fn named_keys_and_printable_chars_become_key_commands() {
        let mut recorder = Recorder::in_memory();

        for key in [
            Key::Enter,
            Key::Escape,
            Key::Tab,
            Key::Backspace,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Char('a'),
            Key::Char('T'),
        ] {
            record(&mut recorder, InputEvent::Key(key));
        }
        // Вне текстового ввода пробел и прочие клавиши не выражаются.
        record(&mut recorder, InputEvent::Key(Key::Char(' ')));
        record(&mut recorder, InputEvent::Key(Key::Other));

        assert_eq!(
            recorder.commands(),
            [
                "key ENTER",
                "key ESC",
                "key TAB",
                "key BACKSPACE",
                "key UP",
                "key DOWN",
                "key LEFT",
                "key RIGHT",
                "key a",
                "key T"
            ]
        );
    }

    #[test]
    fn printable_text_between_open_and_enter_is_one_type_command() {
        let mut recorder = Recorder::in_memory();
        let session = RecordContext {
            in_session: true,
            text_open: false,
        };

        // Enter в сессии открывает чат — это отдельный key.
        recorder.record(InputEvent::Key(Key::Enter), session);
        let open = RecordContext {
            in_session: true,
            text_open: true,
        };

        for character in "Hello, world 42".chars() {
            recorder.record(InputEvent::Key(Key::Char(character)), open);
        }

        // Enter отправляет сообщение: type закрывается, затем key ENTER.
        recorder.record(InputEvent::Key(Key::Enter), open);

        assert_eq!(
            recorder.commands(),
            ["key ENTER", "type \"Hello, world 42\"", "key ENTER"]
        );
    }

    #[test]
    fn text_is_flushed_before_escape_and_backspace_splits_type() {
        let mut recorder = Recorder::in_memory();
        let open = RecordContext {
            in_session: true,
            text_open: true,
        };

        for character in "abc".chars() {
            recorder.record(InputEvent::Key(Key::Char(character)), open);
        }
        recorder.record(InputEvent::Key(Key::Backspace), open);
        for character in "d".chars() {
            recorder.record(InputEvent::Key(Key::Char(character)), open);
        }
        recorder.record(InputEvent::Key(Key::Escape), open);

        assert_eq!(
            recorder.commands(),
            [
                "type \"abc\"",
                "key BACKSPACE",
                "type \"d\"",
                "key ESC"
            ]
        );
    }

    #[test]
    fn wait_before_type_and_before_closing_key_preserve_pauses() {
        let mut recorder = Recorder::in_memory();
        let session = RecordContext {
            in_session: true,
            text_open: false,
        };
        let open = RecordContext {
            in_session: true,
            text_open: true,
        };

        recorder.record(InputEvent::Key(Key::Char('t')), session);
        for _ in 0..5 {
            recorder.begin_frame();
        }
        for character in "hi".chars() {
            recorder.record(InputEvent::Key(Key::Char(character)), open);
        }
        for _ in 0..3 {
            recorder.begin_frame();
        }
        recorder.record(InputEvent::Key(Key::Enter), open);

        assert_eq!(
            recorder.commands(),
            ["key t", "wait 5", "type \"hi\"", "wait 3", "key ENTER"]
        );
    }

    #[test]
    fn long_pauses_are_capped() {
        let mut recorder = Recorder::in_memory().with_pause_cap(120);

        click(&mut recorder, 1, 2);
        for _ in 0..900 {
            recorder.begin_frame();
        }
        click(&mut recorder, 5, 6);

        assert_eq!(
            recorder.commands(),
            ["click 1 2", "wait 120", "click 5 6"]
        );
    }

    #[test]
    fn zero_cap_disables_waits() {
        let mut recorder = Recorder::in_memory().with_pause_cap(0);

        click(&mut recorder, 1, 2);
        for _ in 0..10 {
            recorder.begin_frame();
        }
        click(&mut recorder, 3, 4);

        assert_eq!(recorder.commands(), ["click 1 2", "click 3 4"]);
    }

    #[test]
    fn finish_flushes_open_text_and_trailing_wait() {
        let mut recorder = Recorder::in_memory();
        let open = RecordContext {
            in_session: true,
            text_open: true,
        };

        recorder.record(InputEvent::Key(Key::Char('h')), open);
        recorder.record(InputEvent::Key(Key::Char('i')), open);
        for _ in 0..4 {
            recorder.begin_frame();
        }

        let path = recorder.finish().expect("finish");

        assert!(path.is_none());
        assert_eq!(recorder.commands(), ["type \"hi\"", "wait 4"]);
    }

    #[test]
    fn typical_session_reads_like_a_qa_script() {
        let mut recorder = Recorder::in_memory();

        // Главное меню → New Game.
        click(&mut recorder, 485, 189);
        for _ in 0..2 {
            recorder.begin_frame();
        }
        // Options: No Limit + Turn Based → Done.
        click(&mut recorder, 240, 110);
        click(&mut recorder, 500, 100);
        click(&mut recorder, 516, 440);
        for _ in 0..2 {
            recorder.begin_frame();
        }
        // Planet → Done.
        click(&mut recorder, 516, 440);
        for _ in 0..2 {
            recorder.begin_frame();
        }
        // Players: RED — человек, GREEN — компьютер, Done.
        click(&mut recorder, 202, 102);
        click(&mut recorder, 312, 194);
        click(&mut recorder, 516, 440);
        for _ in 0..2 {
            recorder.begin_frame();
        }
        // В партии: выделение, приказ движения, End Turn.
        click(&mut recorder, 404, 242);
        for _ in 0..3 {
            recorder.begin_frame();
        }
        record(&mut recorder, InputEvent::RightClick(468, 306));
        for _ in 0..2 {
            recorder.begin_frame();
        }
        click(&mut recorder, 426, 12);

        assert_eq!(
            recorder.commands(),
            [
                "click 485 189",
                "wait 2",
                "click 240 110",
                "click 500 100",
                "click 516 440",
                "wait 2",
                "click 516 440",
                "wait 2",
                "click 202 102",
                "click 312 194",
                "click 516 440",
                "wait 2",
                "click 404 242",
                "wait 3",
                "rclick 468 306",
                "wait 2",
                "click 426 12"
            ]
        );
    }

    #[test]
    fn existing_file_is_not_overwritten() {
        let dir = std::env::temp_dir().join(format!(
            "max_qa_record_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("run.qa");

        let mut first = Recorder::new(&path).expect("first");
        click(&mut first, 1, 2);
        let first_path = first.finish().expect("finish").expect("path");
        assert_eq!(first_path, path);
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), {
            first.header().len() + first.commands().len() + 1
        });

        let mut second = Recorder::new(&path).expect("second");
        click(&mut second, 3, 4);
        let second_path = second.finish().expect("finish").expect("path");
        assert_eq!(second_path, dir.join("run-1.qa"));
        assert!(second_path.is_file());

        // Оба файла — валидные скрипты для qa::script.
        for file in [&first_path, &second_path] {
            let text = std::fs::read_to_string(file).expect("read");
            let steps = crate::qa::script::parse_script(&text).expect("parse");
            assert!(!steps.is_empty(), "{}", file.display());
        }

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn header_mentions_manual_recording_and_build() {
        let recorder = Recorder::in_memory();
        let header = recorder.header().join("\n");

        assert!(header.contains("записан вручную"), "{header}");
        assert!(header.contains("дата:"), "{header}");
        assert!(header.contains("билд:"), "{header}");
        assert!(
            header.lines().all(|line| line.starts_with('#')),
            "{header}"
        );
    }

    #[test]
    fn date_conversion_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_686), (2026, 8, 21));
        assert!(utc_timestamp().ends_with(" UTC"));
    }
}
