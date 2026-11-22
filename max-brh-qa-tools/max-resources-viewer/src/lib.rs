//! M.A.X. MAX.RES: resource classifier, rasterization and PNG export.
//!
//! The library is separated from the binary (`main.rs`) so that integration tests
//! can check the classifier and the export on a real `MAX.RES`.
//!
//! * [`model`] — kind/category/metadata, non-images;
//! * [`render`] — RGBA rasterization (palette, transparency, frame strips);
//! * [`export`] — batch PNG export + `manifest.csv`;
//! * [`png`] — a PNG codec without external dependencies (a copy of the QA codec with RGBA);
//! * [`ui`] — the editor's pure state/drawing (no SDL);
//! * [`startup`] — finding `MAX.RES`/`COLOR.PAL`, the stub palette;
//! * `app` — the SDL3 window under the `sdl` feature.
#![forbid(unsafe_code)]

pub mod export;
pub mod model;
pub mod png;
pub mod render;
pub mod startup;
pub mod ui;

#[cfg(feature = "sdl")]
pub mod app;
