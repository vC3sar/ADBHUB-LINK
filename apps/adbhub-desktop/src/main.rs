#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Standalone executable for `ADBHUB-LINK Desktop` (`apps/adbhub-desktop`).
//!
//! Launches the **Native Windows Desktop GUI** application (DirectX/GPU accelerated via `eframe`),
//! while running the local Control Plane and background USB/IP VHCI Bridge controller.

pub mod gui;

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use adbhub_core::{AdbHubError, CoreResult};
use adbhub_desktop::{build_desktop_router, DesktopController};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> CoreResult<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    // Create background multi-thread Tokio runtime for networking & async USB/IP pumps
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| AdbHubError::WindowsUsbError(format!("Failed to create async runtime: {e}")))?,
    );

    let bind_str = env::var("ADBHUB_DESKTOP_BIND").unwrap_or_else(|_| "127.0.0.1:3420".to_string());
    let bind_addr: SocketAddr = bind_str.parse().map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Invalid ADBHUB_DESKTOP_BIND '{bind_str}': {e}"))
    })?;

    let data_dir = env::var("ADBHUB_DATA_DIR").unwrap_or_else(|_| "./data".to_string());

    let controller = rt.block_on(DesktopController::new_with_local_storage(&data_dir))?;
    let app = build_desktop_router(controller.clone());

    // Spawn the local API router in the background
    let bg_rt = rt.clone();
    rt.spawn(async move {
        if let Ok(listener) = TcpListener::bind(bind_addr).await {
            info!(
                bind = %bind_addr,
                "Background Desktop Control Plane listener active"
            );
            let _ = axum::serve(listener, app).await;
        }
    });

    info!("Starting ADBHUB-LINK Desktop Native GUI application...");

    // Run the native Windows desktop application on the main thread
    gui::run_desktop_native_gui(bg_rt, controller)
        .map_err(|e| AdbHubError::WindowsUsbError(format!("Native GUI window error: {e}")))?;

    Ok(())
}
