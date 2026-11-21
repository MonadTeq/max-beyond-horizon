//! Build script for `max_net`.
//!
//! Two independent jobs:
//!
//! 1. **Horizon gRPC codegen** — compile `../../../max-horizon-server/proto`
//!    (relative to `CARGO_MANIFEST_DIR`, i.e. `max-brh/crates/max_net` →
//!    repository root `max-horizon-server/proto`) with `tonic-prost-build`.
//!    `protoc` comes from the `protoc-bin-vendored` crate, so Docker images
//!    do not need `apt-get install protobuf-compiler`.
//! 2. **Vendored ENet 1.3.18** — compile `vendor/enet/*.c` with `cc` into a
//!    static library linked into the crate. No system ENet, no apt package.
//!
//! The proto path is fixed and asserted: the schemas are the single source of
//! truth of the Horizon contract (`max-brh-docs/protocol-grpc.md` §2), and silently
//! skipping generation would produce a confusing `include_proto!` failure.

use std::path::{Path, PathBuf};

fn main() {
    compile_horizon_proto();
    compile_enet();
}

/// Generates `horizon.v1` client + server stubs into `OUT_DIR`.
fn compile_horizon_proto() {
    // `CARGO_MANIFEST_DIR` is `<repo>/max-brh/crates/max_net`.
    // `../../../max-horizon-server/proto` resolves to `<repo>/max-horizon-server/proto`.
    const PROTO_REL: &str = "../../../max-horizon-server/proto";
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let proto_dir = normalize(&manifest_dir.join(PROTO_REL));

    let horizon = proto_dir.join("horizon.proto");
    let types = proto_dir.join("types.proto");
    assert!(
        horizon.is_file() && types.is_file(),
        "max_net: Horizon proto files not found under {} (looked for horizon.proto and types.proto); \
         the repository layout must keep max-horizon-server/proto next to max-brh/",
        proto_dir.display()
    );

    // `protoc-bin-vendored` ships a prebuilt protoc for the host platform.
    // `prost-build` reads this environment variable; setting it here keeps
    // builds self-contained (no protobuf-compiler / choco install).
    let protoc = protoc_bin_vendored::protoc_bin_path()
        .expect("max_net: protoc-bin-vendored has no binary for this host");
    std::env::set_var("PROTOC", &protoc);

    println!("cargo:rerun-if-changed={}", horizon.display());
    println!("cargo:rerun-if-changed={}", types.display());
    println!("cargo:rerun-if-env-changed=PROTOC");

    let proto_dir_str = proto_dir.to_string_lossy().into_owned();
    let proto_files = [
        horizon.to_string_lossy().into_owned(),
        types.to_string_lossy().into_owned(),
    ];

    tonic_prost_build::configure()
        // Client is the product; server is needed by the mock-server tests.
        .build_client(true)
        .build_server(true)
        .compile_protos(&proto_files, &[proto_dir_str])
        .expect("max_net: failed to generate Horizon gRPC stubs from proto/");
}

/// Compiles the vendored ENet 1.3.18 sources (static library `enet`).
fn compile_enet() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let enet_dir = manifest_dir.join("vendor/enet");
    let include_dir = enet_dir.join("include");
    assert!(
        include_dir.join("enet/enet.h").is_file(),
        "max_net: vendored ENet not found at {} (expected a copy of enet-1.3.18)",
        enet_dir.display()
    );

    // ENet 1.3.18 is a plain C library. The exact same sources are pinned by
    // the C++ port (`CMakeLists.txt` -> `include(enet)` from
    // `dependencies/enet-1.3.18.tar.gz`), so both ends speak the same 1.3.18
    // wire protocol checked in the TP 00 handshake (`ENET_VERSION=0x010312`).
    let mut build = cc::Build::new();
    build
        .include(&include_dir)
        .warnings(false)
        .file(enet_dir.join("callbacks.c"))
        .file(enet_dir.join("compress.c"))
        .file(enet_dir.join("host.c"))
        .file(enet_dir.join("list.c"))
        .file(enet_dir.join("packet.c"))
        .file(enet_dir.join("peer.c"))
        .file(enet_dir.join("protocol.c"));

    // The platform backend MUST be chosen by the *target* OS, not by the host:
    // `cfg!(target_os = ...)` inside a build script expands for the host that
    // compiles the script, which silently picks `unix.c` when cross-compiling
    // to Windows (unix.c is wrapped in `#ifndef _WIN32` and win32.c in
    // `#ifdef _WIN32`, so the wrong choice yields an empty object file and
    // linker errors: undefined reference to `enet_socket_create` etc.).
    // `CARGO_CFG_TARGET_OS` is set by Cargo for the crate's target.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    if target_os == "windows" {
        build.file(enet_dir.join("win32.c"));
        // ENet needs WinSock 2 and the multimedia timer.
        println!("cargo:rustc-link-lib=winmm");
        println!("cargo:rustc-link-lib=ws2_32");
    } else {
        build.file(enet_dir.join("unix.c"));
    }

    build.compile("enet");

    println!(
        "cargo:rerun-if-changed={}",
        enet_dir.join("include/enet/enet.h").display()
    );
    for file in [
        "callbacks.c",
        "compress.c",
        "host.c",
        "list.c",
        "packet.c",
        "peer.c",
        "protocol.c",
        "unix.c",
        "win32.c",
    ] {
        println!("cargo:rerun-if-changed={}", enet_dir.join(file).display());
    }
}

/// Lexically normalizes `..` components so diagnostics show a clean path.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
