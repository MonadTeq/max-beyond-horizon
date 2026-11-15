//! Слои 2–4 QA-стратегии (`max-rust-qa/plans/qa-plan.md`): скриптовый
//! UI-драйвер, запись ручной игры и golden-кадры — без SDL, окна и сети.
//!
//! | Модуль | Что делает |
//! |---|---|
//! | [`script`] | парсер `.qa`-скриптов: строка → [`script::Command`] |
//! | [`runner`] | исполнитель: те же `InputEvent`, что у мыши/клавиатуры, кадры в PNG, ассерты |
//! | [`recorder`] | запись ручной игры в `.qa`-скрипт (`--ui --record`) |
//! | [`png`] | PNG-кодек и попиксельная сверка (без внешних крейтов) |
//!
//! Точки входа — примеры `max_app`:
//!
//! ```sh
//! # слой 2: прогнать сценарий, получить log.txt + PNG
//! cargo run -p max_app --example qa_run -- max-rust-qa/scripts/sp-full.qa qa-out/sp-full
//!
//! # слой 3: сверить кадры с эталонами (порог 1 % пикселей, Δ > 30)
//! cargo run -p max_app --example qa_golden_check -- qa-out/visual-screens max-rust-qa/golden
//!
//! # слой 2.5: записать ручную игру в .qa (из max-rust/)
//! cargo run -p max_app --features sdl -- --ui --record ../max-rust-qa/scripts/my.qa
//! ```
//!
//! Почему это возможно без окна: экраны и игровой цикл `max_app` — чистые
//! функции от [`crate::ui::InputEvent`] (`Screen::handle_input`,
//! `ui::handle_session_input`, `local::App::local_update`). Драйвер держит те
//! же `App`/`Screen`/`UiContext`, что `main.rs`, и крутит тот же игровой цикл,
//! поэтому скрипт проверяет настоящий путь игрока, а не обходные вызовы.

pub mod png;
pub mod recorder;
pub mod runner;
pub mod script;

pub use png::{compare, FrameDiff, Image};
pub use recorder::{RecordContext, Recorder, DEFAULT_PAUSE_CAP};
pub use runner::{QaConfig, QaMenuClient, QaReport, QaRunner};
pub use script::{
    parse_script, Assertion, Cmp, Command, MenuTarget, PhaseName, ScreenName, SetupAction, Step,
    FRAME_HEIGHT, FRAME_WIDTH,
};
