//! Layers 2–4 of the QA strategy (`max-brh-qa/plans/qa-plan.md`): the scripted
//! UI driver, manual game recording and golden frames — without SDL, a window or the network.
//!
//! | Module | What it does |
//! |---|---|
//! | [`script`] | `.qa` script parser: line → [`script::Command`] |
//! | [`runner`] | executor: the same `InputEvent`s as the mouse/keyboard, frames to PNG, asserts |
//! | [`recorder`] | records a manual game into a `.qa` script (`--ui --record`) |
//! | [`png`] | PNG codec and per-pixel comparison (no external crates) |
//!
//! The entry points are the `max_app` examples:
//!
//! ```sh
//! # layer 2: run a scenario, get log.txt + PNG (SDL not needed)
//! cargo run -p max_app --no-default-features --example qa_run -- max-brh-qa/scripts/sp-full.qa qa-out/sp-full
//!
//! # layer 3: compare frames with the references (threshold 1% of pixels, Δ > 30)
//! cargo run -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens max-brh-qa/golden
//!
//! # layer 2.5: record a manual game into .qa (from max-brh/; GUI — default)
//! cargo run -p max_app -- --ui --record ../max-brh-qa/scripts/my.qa
//! ```
//!
//! Why this is possible without a window: the screens and the `max_app` game loop are pure
//! functions of [`crate::ui::InputEvent`] (`Screen::handle_input`,
//! `ui::handle_session_input`, `local::App::local_update`). The driver holds the same
//! `App`/`Screen`/`UiContext` as `main.rs` and runs the same game loop,
//! so the script exercises the real player path, not workaround calls.

pub mod png;
pub mod recorder;
pub mod runner;
pub mod script;

pub use png::{compare, FrameDiff, Image};
pub use recorder::{RecordContext, Recorder, DEFAULT_PAUSE_CAP};
pub use runner::{QaConfig, QaMenuClient, QaReport, QaRunner};
pub use script::{
    parse_script, Assertion, Cmp, Command, MenuTarget, PhaseName, ScreenName, SetupAction, Step,
    UpgradeCommand, FRAME_HEIGHT, FRAME_WIDTH,
};
