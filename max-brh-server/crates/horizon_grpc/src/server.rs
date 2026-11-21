//! The gRPC listener: configuration type and server bootstrap.
//!
//! The listener is configured by a `[server.grpc]` section of the Horizon
//! `config.toml`:
//!
//! ```toml
//! [server.grpc]
//! enabled = true
//! bind_address = "0.0.0.0:8082"
//! announce_host = "max.example.com"   # what JoinGame returns to clients
//! ```
//!
//! `enabled` defaults to `false`, `bind_address` to `0.0.0.0:8082` — the
//! WebSocket listener (8081) keeps running unchanged next to it
//! (`max-brh-docs/protocol-grpc.md` §4.4). `announce_host` defaults to `127.0.0.1` (the
//! legacy WS value) and is independent of `bind_address`, so a public server
//! can listen on `0.0.0.0` but advertise its real host. TLS is not enabled in
//! phase 1; put the listener behind a TLS terminator if clients need
//! `https://`.
//!
//! [`serve`] builds the production [`MaxBackend`] from the same `config.toml`
//! file (the `[max]` section plus `MAX_*` environment variables) and serves
//! `horizon.v1.Horizon` until `shutdown` resolves. [`serve_with_listener`] is
//! the same thing over an already bound `TcpListener`, which is what the
//! integration test uses to serve on `127.0.0.1:0`.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

use crate::backend::{HorizonBackend, MaxBackend};
use crate::proto::v1::horizon_server::HorizonServer;
use crate::service::HorizonService;

/// Default bind address of the gRPC listener (`max-brh-docs/protocol-grpc.md` §4.4).
pub const DEFAULT_GRPC_BIND_ADDRESS: &str = "0.0.0.0:8082";

/// Default advertised host of `JoinGameResponse.host`: the legacy WS value.
pub const DEFAULT_ANNOUNCE_HOST: &str = "127.0.0.1";

fn default_grpc_bind_address() -> String {
    DEFAULT_GRPC_BIND_ADDRESS.to_string()
}

fn default_announce_host() -> String {
    DEFAULT_ANNOUNCE_HOST.to_string()
}

/// `[server.grpc]`: settings of the Horizon lobby/admin gRPC listener.
///
/// The section is optional; without it the listener is disabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrpcSettings {
    /// Whether to start the listener at all.
    #[serde(default)]
    pub enabled: bool,
    /// Address to bind, e.g. `"0.0.0.0:8082"`.
    #[serde(default = "default_grpc_bind_address")]
    pub bind_address: String,
    /// Host advertised to clients in `JoinGameResponse.host` — the address an
    /// ENet client should connect to. It is independent of `bind_address`
    /// (typically `0.0.0.0`): set it to the public address of this server when
    /// clients do not run on the same machine. Defaults to `127.0.0.1`, the
    /// legacy WS value.
    #[serde(default = "default_announce_host")]
    pub announce_host: String,
}

impl Default for GrpcSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_address: default_grpc_bind_address(),
            announce_host: default_announce_host(),
        }
    }
}

impl GrpcSettings {
    /// Parses `bind_address`.
    pub fn socket_addr(&self) -> Result<SocketAddr, String> {
        self.bind_address.parse::<SocketAddr>().map_err(|error| {
            format!(
                "invalid [server.grpc] bind_address {:?}: {error}",
                self.bind_address
            )
        })
    }

    /// Validates the section (called from `AppConfig::validate`).
    pub fn validate(&self) -> Result<(), String> {
        self.socket_addr()?;

        let host = self.announce_host.trim();
        if host.is_empty() {
            return Err("invalid [server.grpc] announce_host: must not be empty".to_string());
        }
        if host.chars().any(char::is_whitespace) {
            return Err(format!(
                "invalid [server.grpc] announce_host {host:?}: must not contain whitespace"
            ));
        }

        Ok(())
    }
}

/// Serves `horizon.v1.Horizon` on `settings.bind_address` until `shutdown`
/// resolves, using the production backend built from `config_path`.
///
/// A missing or incomplete `[max]` section does not prevent the listener from
/// starting: the RPCs answer the same "not configured" error the WS plugins
/// send.
pub async fn serve<F>(
    settings: &GrpcSettings,
    config_path: &Path,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send,
{
    let address = settings.socket_addr().map_err(anyhow::Error::msg)?;
    let backend = Arc::new(
        MaxBackend::from_config_file(config_path)
            .with_host(settings.announce_host.trim().to_string()),
    );
    serve_with_backend(backend, address, shutdown).await
}

/// Serves the service on `address` (binds the socket) until `shutdown` resolves.
pub async fn serve_with_backend<F>(
    backend: Arc<dyn HorizonBackend>,
    address: SocketAddr,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send,
{
    let listener = TcpListener::bind(address).await?;
    serve_with_listener(backend, listener, shutdown).await
}

/// Serves the service over an already bound listener until `shutdown` resolves.
///
/// Used by the integration test to bind `127.0.0.1:0` and learn the port before
/// the server starts. On shutdown every session the backend owns is stopped.
pub async fn serve_with_listener<F>(
    backend: Arc<dyn HorizonBackend>,
    listener: TcpListener,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send,
{
    let address = listener.local_addr()?;
    tracing::info!(
        %address,
        "horizon_grpc: lobby/admin gRPC listener started (horizon.v1.Horizon)"
    );

    let service = HorizonService::new(Arc::clone(&backend));
    Server::builder()
        .add_service(HorizonServer::new(service))
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
        .await?;

    backend.shutdown().await;
    tracing::info!(%address, "horizon_grpc: lobby/admin gRPC listener stopped");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_contract() {
        let settings = GrpcSettings::default();

        assert!(!settings.enabled);
        assert_eq!(settings.bind_address, "0.0.0.0:8082");
        assert_eq!(settings.announce_host, "127.0.0.1");
        assert_eq!(settings.socket_addr().unwrap().port(), 8082);
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn section_parses_from_toml_with_defaults() {
        // Only `enabled` set: the address and the announced host keep the defaults.
        let settings: GrpcSettings = toml::from_str("enabled = true").unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.bind_address, DEFAULT_GRPC_BIND_ADDRESS);
        assert_eq!(settings.announce_host, DEFAULT_ANNOUNCE_HOST);

        // Explicit address and announced host.
        let settings: GrpcSettings = toml::from_str(
            "enabled = true\nbind_address = \"127.0.0.1:9000\"\nannounce_host = \"max.example.com\"",
        )
        .unwrap();
        assert_eq!(settings.socket_addr().unwrap().to_string(), "127.0.0.1:9000");
        assert_eq!(settings.announce_host, "max.example.com");
    }

    #[test]
    fn invalid_bind_address_is_rejected() {
        let settings = GrpcSettings {
            enabled: true,
            bind_address: "not-an-address".to_string(),
            ..GrpcSettings::default()
        };

        let error = settings.socket_addr().unwrap_err();
        assert!(error.contains("invalid [server.grpc] bind_address"));
    }

    #[test]
    fn empty_or_whitespace_announce_host_is_rejected() {
        for host in ["", "   ", "max example com"] {
            let settings = GrpcSettings {
                enabled: true,
                announce_host: host.to_string(),
                ..GrpcSettings::default()
            };

            let error = settings.validate().unwrap_err();
            assert!(
                error.contains("invalid [server.grpc] announce_host"),
                "unexpected error for {host:?}: {error}"
            );
        }
    }
}
