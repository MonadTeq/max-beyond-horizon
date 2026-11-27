//! Build script: track the precompiled shaders.
//!
//! `src/gpu_present.rs` embeds the SPIR-V with `include_bytes!`, and Cargo does
//! **not** rebuild a crate when a file reached only through `include_bytes!`
//! changes — a recompiled `shaders/*.spv` would silently be ignored (the binary
//! keeps the old pass). These directives make the shaders a rebuild dependency.
//!
//! The shaders themselves are precompiled and committed: no GLSL compiler is
//! needed to build the client (`shaders/README.md` documents the regeneration).

fn main() {
    println!("cargo:rerun-if-changed=shaders/fxaa.vert");
    println!("cargo:rerun-if-changed=shaders/fxaa.frag");
    println!("cargo:rerun-if-changed=shaders/fxaa.vert.spv");
    println!("cargo:rerun-if-changed=shaders/fxaa.frag.spv");
}
