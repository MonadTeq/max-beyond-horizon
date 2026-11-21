//! M.A.X. MAX.RES: классификатор ресурсов, растеризация и PNG-экспорт.
//!
//! Библиотека отделена от бинарника (`main.rs`), чтобы интеграционные тесты
//! могли проверять классификатор и экспорт на реальном `MAX.RES`.
//!
//! * [`model`] — kind/category/метаданные, не-изображения;
//! * [`render`] — RGBA-растеризация (палитра, прозрачность, ленты кадров);
//! * [`export`] — пакетный экспорт PNG + `manifest.csv`;
//! * [`png`] — PNG-кодек без внешних зависимостей (копия QA-кодека с RGBA);
//! * [`ui`] — чистое состояние/отрисовка редактора (без SDL);
//! * [`startup`] — поиск `MAX.RES`/`COLOR.PAL`, заглушка-палитра;
//! * `app` — SDL3-окно под feature `sdl`.
#![forbid(unsafe_code)]

pub mod export;
pub mod model;
pub mod png;
pub mod render;
pub mod startup;
pub mod ui;

#[cfg(feature = "sdl")]
pub mod app;
