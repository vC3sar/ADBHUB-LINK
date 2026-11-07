#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! # `adbhub-agent`
//!
//! ADBHUB-LINK Host Windows Agent (Phase 1 & Phase 2 Deliverable).
//! Performs real-time Windows USB enumeration, descriptor & mode classification
//! (ADB, MTP, Qualcomm EDL, MediaTek BROM, SPD/Unisoc, Samsung Download Mode / Odin),
//! hotplug/reenumeration lifecycle tracking, and local USB/IP export (`usbipd-win`).
//!
//! When launched without arguments (or with `--gui`), opens the **Native Windows Desktop GUI**
//! for the customer/client to see their connected mobile devices, status, and consent approvals.

pub mod gui;

use std::env;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use adbhub_core::{AdbHubError, BusId, CoreResult, UsbDeviceMode};
use adbhub_usb_discovery::{DiscoveryFilter, UsbDiscoveryEngine};
use adbhub_usb_exporter::UsbExportManager;
use adbhub_usb_monitor::{LifecycleManagerConfig, UsbHotplugMonitor};
use adbhub_usbip_adapter::{HostUsbipBindController, UsbipdWinController, DEFAULT_USBIP_PORT};
use adbhub_usbip_client_adapter::{TechnicianVhciController, UsbIpWin2ClientController};
use tokio::sync::watch;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentCliCommand {
    /// Launch the Native Desktop GUI application (Default).
    Gui,
    /// Perform a single USB bus scan and output structured table or JSON (`--scan` / `--json`).
    ScanOnce { json_output: bool, mobile_only: bool },
    /// Run continuous hotplug and reenumeration monitor (`--watch`).
    Watch {
        poll_ms: u64,
        grace_seconds: u64,
        mobile_only: bool,
    },
    /// Export (bind) a physical USB device by `BUSID` using `usbipd-win` (`export <busid> [--force]`).
    Export { bus_id: BusId, force: bool },
    /// Unexport (unbind) a physical USB device by `BUSID` (`unexport <busid>`).
    Unexport { bus_id: BusId },
    /// Attach a remote or local USB/IP device into the Technician VHCI stack (`attach <host:port> <busid>`).
    AttachVhci {
        server_addr: SocketAddr,
        bus_id: BusId,
    },
    /// Detach a virtual port from the Technician VHCI stack (`detach <vhci_port>`).
    DetachVhci { vhci_port: u16 },
    /// Check installation and readiness of `usbipd-win` (Host) and `usbip-win2` (Client).
    Doctor,
}

fn parse_cli_args() -> CoreResult<AgentCliCommand> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--gui" || a == "gui") {
        return Ok(AgentCliCommand::Gui);
    }

    let json_output = args.iter().any(|a| a == "--json");
    let mobile_only = args.iter().any(|a| a == "--mobile-only");
    let force = args.iter().any(|a| a == "--force" || a == "-f");

    if let Some(first) = args.first().map(String::as_str) {
        match first {
            "scan" | "--scan" => {
                return Ok(AgentCliCommand::ScanOnce {
                    json_output,
                    mobile_only,
                });
            }
            "watch" | "--watch" => {
                return Ok(AgentCliCommand::Watch {
                    poll_ms: 1000,
                    grace_seconds: 20,
                    mobile_only,
                });
            }
            "doctor" | "--doctor" => {
                return Ok(AgentCliCommand::Doctor);
            }
            "export" | "bind" => {
                let bus_str = args.get(1).ok_or_else(|| {
                    AdbHubError::InvalidBusId("Missing <busid> for export command".to_string())
                })?;
                let bus_id = BusId::from_str(bus_str)?;
                return Ok(AgentCliCommand::Export { bus_id, force });
            }
            "unexport" | "unbind" => {
                let bus_str = args.get(1).ok_or_else(|| {
                    AdbHubError::InvalidBusId("Missing <busid> for unexport command".to_string())
                })?;
                let bus_id = BusId::from_str(bus_str)?;
                return Ok(AgentCliCommand::Unexport { bus_id });
            }
            "attach" => {
                let addr_str = args.get(1).ok_or_else(|| {
                    AdbHubError::WindowsUsbError(
                        "Usage: adbhub-agent attach <host:port> <busid>".to_string(),
                    )
                })?;
                let bus_str = args.get(2).ok_or_else(|| {
                    AdbHubError::InvalidBusId(
                        "Usage: adbhub-agent attach <host:port> <busid>".to_string(),
                    )
                })?;
                let server_addr: SocketAddr = addr_str.parse().map_err(|e| {
                    AdbHubError::WindowsUsbError(format!("Invalid socket address '{addr_str}': {e}"))
                })?;
                let bus_id = BusId::from_str(bus_str)?;
                return Ok(AgentCliCommand::AttachVhci {
                    server_addr,
                    bus_id,
                });
            }
            "detach" => {
                let port_str = args.get(1).ok_or_else(|| {
                    AdbHubError::WindowsUsbError(
                        "Usage: adbhub-agent detach <vhci_port>".to_string(),
                    )
                })?;
                let vhci_port: u16 = port_str.parse().map_err(|e| {
                    AdbHubError::WindowsUsbError(format!("Invalid VHCI port '{port_str}': {e}"))
                })?;
                return Ok(AgentCliCommand::DetachVhci { vhci_port });
            }
            _ => {}
        }
    }

    Ok(AgentCliCommand::ScanOnce {
        json_output,
        mobile_only,
    })
}

fn main() -> CoreResult<()> {
    let filter_layer =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter_layer)
        .with_target(false)
        .init();

    let cmd = parse_cli_args()?;

    match cmd {
        AgentCliCommand::Gui => {
            println!("Iniciando ADBHUB-LINK Agent en modo GUI nativo...");
            gui::run_agent_native_gui()
                .map_err(|e| AdbHubError::WindowsUsbError(format!("GUI runtime error: {e}")))?;
            Ok(())
        }
        cli_cmd => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| AdbHubError::WindowsUsbError(format!("Tokio runtime error: {e}")))?;
            rt.block_on(run_cli(cli_cmd))
        }
    }
}

async fn run_cli(cmd: AgentCliCommand) -> CoreResult<()> {
    match cmd {
        AgentCliCommand::Gui => {}
        AgentCliCommand::ScanOnce {
            json_output,
            mobile_only,
        } => {
            let engine = UsbDiscoveryEngine::with_windows_native(DiscoveryFilter {
                mobile_repair_devices_only: mobile_only,
            });
            let devices = engine.discover_all()?;

            if json_output {
                let serialized = serde_json::to_string_pretty(&devices)
                    .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;
                println!("{serialized}");
            } else {
                println!("================================================================================");
                println!(" ADBHUB-LINK Agent v0.2.0 — Windows USB Passthrough Discovery & Export Engine");
                println!("================================================================================");
                println!(" Total USB Devices Enumerated: {}", devices.len());
                println!("--------------------------------------------------------------------------------");
                for dev in &devices {
                    println!(
                        " [BUSID {:<7}] VID:PID {} | Mode: {:<22} | State: {}",
                        dev.bus_id,
                        dev.vid_pid,
                        dev.primary_mode(),
                        dev.lifecycle_state
                    );
                    println!(
                        "   Product      : {} ({})",
                        dev.product_name, dev.manufacturer
                    );
                    println!("   Instance ID  : {}", dev.instance_id);
                    println!("   Export State : {:?}", dev.export_state);
                    if let Some(drv) = &dev.host_driver_service {
                        println!("   Host Driver  : {drv}");
                    }
                    if let Some(sam) = &dev.classification.samsung_download_info {
                        println!(
                            "   Samsung Odin : Mode={} | Compatibility={} | BulkIN={:?} BulkOUT={:?}",
                            sam.mode_label,
                            sam.odin_compatibility.as_str(),
                            sam.bulk_in_endpoint,
                            sam.bulk_out_endpoint
                        );
                        println!("   Notice       : {}", sam.safety_warning);
                    }
                    println!("--------------------------------------------------------------------------------");
                }
            }
        }
        AgentCliCommand::Watch {
            poll_ms,
            grace_seconds,
            mobile_only,
        } => {
            info!(
                poll_ms,
                grace_seconds,
                mobile_only,
                "Starting ADBHUB-LINK Agent continuous USB hotplug & reenumeration monitor"
            );
            let engine = Arc::new(UsbDiscoveryEngine::with_windows_native(DiscoveryFilter {
                mobile_repair_devices_only: mobile_only,
            }));
            let mut monitor = UsbHotplugMonitor::new(
                engine,
                LifecycleManagerConfig {
                    reenumeration_grace_period: Duration::from_secs(grace_seconds),
                },
                Duration::from_millis(poll_ms),
            );

            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    let _ = shutdown_tx.send(true);
                }
            });

            monitor.run_until_cancelled(shutdown_rx).await?;
        }
        AgentCliCommand::Export { bus_id, force } => {
            let controller = Arc::new(UsbipdWinController::new());
            let exporter = UsbExportManager::new(controller.clone());
            let state = if force {
                controller.bind_bus_id(&bus_id, true)?
            } else {
                exporter.export_device(&bus_id, UsbDeviceMode::AndroidAdb)?
            };
            println!("Device {bus_id} exported successfully: {state:?}");
        }
        AgentCliCommand::Unexport { bus_id } => {
            let controller = Arc::new(UsbipdWinController::new());
            let exporter = UsbExportManager::new(controller);
            exporter.unexport_device(&bus_id)?;
            println!("Device {bus_id} unexported and returned to Windows Host PnP stack.");
        }
        AgentCliCommand::AttachVhci {
            server_addr,
            bus_id,
        } => {
            let client = UsbIpWin2ClientController::new();
            let attached = client.attach_device(server_addr, &bus_id)?;
            println!(
                "Attached remote device {} from {} on local Windows VHCI Port {}",
                attached.remote_bus_id, attached.remote_endpoint, attached.vhci_port
            );
        }
        AgentCliCommand::DetachVhci { vhci_port } => {
            let client = UsbIpWin2ClientController::new();
            client.detach_port(vhci_port)?;
            println!("Detached local Windows VHCI Port {vhci_port}");
        }
        AgentCliCommand::Doctor => {
            let host_ctrl = UsbipdWinController::new();
            let client_ctrl = UsbIpWin2ClientController::new();
            println!("================================================================================");
            println!(" ADBHUB-LINK System USB/IP Driver Readiness Check (Doctor)");
            println!("================================================================================");
            println!(
                " [Host Export]   usbipd-win (TCP 127.0.0.1:{DEFAULT_USBIP_PORT}) : {}",
                if host_ctrl.is_installed() {
                    "INSTALLED & SIGNED"
                } else {
                    "NOT DETECTED (Install via: winget install dorssel.usbipd-win)"
                }
            );
            println!(
                " [Client VHCI]   usbip-win2 (UDE Virtual Stack)    : {}",
                if client_ctrl.is_installed() {
                    "INSTALLED & SIGNED"
                } else {
                    "NOT DETECTED (Install via: winget install vadimgrn.usbip-win2)"
                }
            );
            println!("================================================================================");
        }
    }

    Ok(())
}
