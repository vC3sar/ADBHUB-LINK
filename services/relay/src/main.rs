//! Standalone executable for the ADBHUB-LINK QUIC Relay Server (`services/relay`).

use std::env;
use std::net::SocketAddr;

use adbhub_core::{AdbHubError, CoreResult};
use adbhub_relay_service::QuicRelayServer;
use adbhub_secure_transport::generate_self_signed_cert_bundle;
use tokio::sync::watch;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> CoreResult<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let bind_str = env::var("ADBHUB_RELAY_BIND").unwrap_or_else(|_| "0.0.0.0:4433".to_string());
    let bind_addr: SocketAddr = bind_str.parse().map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Invalid ADBHUB_RELAY_BIND '{bind_str}': {e}"))
    })?;

    let secret = env::var("ADBHUB_RELAY_SECRET")
        .unwrap_or_else(|_| "adbhub-link-dev-relay-hmac-secret-2026".to_string());

    let cert_bundle = generate_self_signed_cert_bundle(&[
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "relay.adbhub.link".to_string(),
    ])?;

    let server = QuicRelayServer::bind(bind_addr, &cert_bundle, secret.into_bytes())?;
    info!(
        listen_addr = %server.local_addr(),
        "ADBHUB-LINK Centralized QUIC Relay started"
    );

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = shutdown_tx.send(true);
        }
    });

    server.run_until_cancelled(shutdown_rx).await
}

