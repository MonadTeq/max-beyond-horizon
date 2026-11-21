//! The generated `horizon.v1` protobuf/gRPC code.
//!
//! `build.rs` compiles `max-brh-server/proto/horizon.proto` and
//! `types.proto` with `tonic-prost-build`; both the messages and the
//! `horizon_client` / `horizon_server` stubs land in this module. The modules
//! are generated, so no hand-written API lives here — the typed façade is
//! [`crate::service`] / [`crate::server`].

/// The `horizon.v1` package: messages from `types.proto` + `horizon.proto`,
/// plus `horizon_client` and `horizon_server` modules.
pub mod v1 {
    #![allow(clippy::all)]
    #![allow(rustdoc::broken_intra_doc_links)]

    tonic::include_proto!("horizon.v1");
}

pub use v1::*;
