//! Build-time `.ktx2` encoder for the HD remaster (R2 of
//! `max-brh-docs/development/remaster-hd.md`).
//!
//! The pipeline is `lossless source -> ktx-rs (KTX-Software port) -> .ktx2 + remaster-manifest.json`,
//! both written into the remaster root (`target/remaster/`, outside git). Two codecs
//! are produced for every source, ETC1S and UASTC LDR 4x4 — the two the pipeline
//! compares in R0, with ETC1S the primary (the one `assets[].path` points at).
//!
//! Determinism is a hard requirement of the gate, so the encoder is built to be
//! reproducible on one machine:
//!
//! * the encoder crate version is pinned in `Cargo.toml` and `Cargo.lock` is committed;
//! * every encoder knob is a constant in [`encode`] (quality, UASTC flags),
//!   including a single job-pool thread, so the result cannot depend on scheduling;
//! * the manifest is serialized from sorted maps, and sources are processed in file
//!   name order, so the JSON byte order is fixed;
//! * the manifest records the SHA-256 of every generated file and of every source, so
//!   two runs can be compared without trusting the tool's own bookkeeping.
//!
//! Nothing here touches the game's production code: the crate is a separate Cargo
//! project that only reads `max-original/` through `max_assets`/`max_render` (the same
//! decoders the client uses) and writes into `target/`.

pub mod encode;
pub mod hash;
pub mod manifest;
pub mod sample;
pub mod sources;

pub use crate::encode::{Codec, RunSummary};
pub use crate::manifest::{
    AssetEntry, Manifest, SourceInfo, Variant, MANIFEST_FILE, MANIFEST_VERSION,
};
