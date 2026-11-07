//! Standalone executable for the ADBHUB-LINK Cloud Control Plane (`services/api`).

use std::env;
use std::net::SocketAddr;

use adbhub_api_service::{build_api_router, ApiAppState};
use adbhub_core::{AdbHubError, CoreResult};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> CoreResult<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let bind_str = env::var("ADBHUB_API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let bind_addr: SocketAddr = bind_str.parse().map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Invalid ADBHUB_API_BIND '{bind_str}': {e}"))
    })?;

    let jwt_secret = env::var("ADBHUB_JWT_SECRET")
        .unwrap_or_else(|_| "adbhub-link-dev-jwt-secret-2026".to_string())
        .into_bytes();
    let relay_secret = env::var("ADBHUB_RELAY_SECRET")
        .unwrap_or_else(|_| "adbhub-link-dev-relay-hmac-secret-2026".to_string())
        .into_bytes();
    let relay_endpoint =
        env::var("ADBHUB_RELAY_ENDPOINT").unwrap_or_else(|_| "127.0.0.1:4433".to_string());
    let data_dir = env::var("ADBHUB_DATA_DIR").unwrap_or_else(|_| "./data".to_string());

    let state = ApiAppState::with_local_data_dir(&data_dir, jwt_secret, relay_secret, relay_endpoint)?;
    let app = build_api_router(state);

    let listener = TcpListener::bind(bind_addr).await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed to bind API server on {bind_addr}: {e}"))
    })?;

    info!(%bind_addr, "ADBHUB-LINK Cloud Control Plane API & WebSocket Server listening");

    axum::serve(listener, app)
        .await
        .map_err(|e| AdbHubError::WindowsUsbError(format!("Axum server error: {e}")))
}
