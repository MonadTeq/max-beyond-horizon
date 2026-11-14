//! Build script for `horizon_grpc`.
//!
//! Compiles `max-horizon-server/proto/horizon.proto` + `types.proto` (the
//! single source of truth of the Horizon gRPC contract) with
//! `tonic-prost-build` into `OUT_DIR`, which `src/proto.rs` includes as
//! `horizon.v1`. `protoc` comes from `protoc-bin-vendored`, so the build is
//! self-contained (no `apt-get install protobuf-compiler`).
//!
//! Both stubs are generated: the server is the product, the client is used by
//! the integration test (`tests/grpc_roundtrip.rs`) and by tooling.

use std::path::{Path, PathBuf};

fn main() {
    compile_horizon_proto();
}

fn compile_horizon_proto() {
    // `CARGO_MANIFEST_DIR` is `<repo>/max-horizon-server/crates/horizon_grpc`.
    // `../../proto` resolves to `<repo>/max-horizon-server/proto`.
    const PROTO_REL: &str = "../../proto";
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let proto_dir = normalize(&manifest_dir.join(PROTO_REL));

    let horizon = proto_dir.join("horizon.proto");
    let types = proto_dir.join("types.proto");
    assert!(
        horizon.is_file() && types.is_file(),
        "horizon_grpc: Horizon proto files not found under {} (looked for horizon.proto and \
         types.proto); the repository layout must keep max-horizon-server/proto next to crates/",
        proto_dir.display()
    );

    let protoc = protoc_bin_vendored::protoc_bin_path()
        .expect("horizon_grpc: protoc-bin-vendored has no binary for this host");
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
        // Server is the product; client is used by the integration test and by
        // grpcurl-style tooling written in Rust.
        .build_server(true)
        .build_client(true)
        .compile_protos(&proto_files, &[proto_dir_str])
        .expect("horizon_grpc: failed to generate horizon.v1 stubs from proto/");
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
