//! Integration with `usbipd-win` state JSON output (`usbipd state`) to enrich
//! native Windows USB enumeration with exact `BUSID` (`<bus>-<port>`) and binding states.

use std::process::Command;
use std::str::FromStr;

use adbhub_core::{BusId, UsbIpExportState, VidPid};
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

/// Top-level JSON output returned by `usbipd state` in `usbipd-win` v4+.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UsbipdStateOutput {
    #[serde(default)]
    pub devices: Vec<UsbipdDeviceEntry>,
}

/// Individual USB device entry reported by `usbipd state`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UsbipdDeviceEntry {
    pub bus_id: Option<String>,
    pub client_i_p_address: Option<String>,
    pub description: Option<String>,
    pub instance_id: String,
    pub is_forced: bool,
    pub persisted_guid: Option<Uuid>,
    pub stub_instance_id: Option<String>,
}

impl UsbipdDeviceEntry {
    /// Extracts the parsed [`BusId`] if the device is currently plugged in (`BusId` is non-null).
    pub fn parsed_bus_id(&self) -> Option<BusId> {
        self.bus_id.as_deref().and_then(|s| BusId::from_str(s).ok())
    }

    /// Extracts [`VidPid`] from the `InstanceId` field.
    pub fn parsed_vid_pid(&self) -> Option<VidPid> {
        VidPid::from_windows_hardware_id(&self.instance_id)
    }

    /// Computes the [`UsbIpExportState`] for this entry.
    pub fn export_state(&self) -> UsbIpExportState {
        if let Some(ip) = &self.client_i_p_address {
            if !ip.trim().is_empty() {
                return UsbIpExportState::Attached {
                    client_address: ip.clone(),
                };
            }
        }
        if self.persisted_guid.is_some() {
            return UsbIpExportState::BoundReady {
                persisted_guid: self.persisted_guid,
            };
        }
        UsbIpExportState::NotBound
    }
}

/// Queries `usbipd state` if `usbipd-win` is installed on the host Windows machine.
///
/// If `usbipd.exe` is not installed yet on the developer or client machine, returns `Ok(None)`
/// gracefully so native Windows SetupAPI enumeration still works autonomously.
pub fn query_usbipd_state() -> Option<UsbipdStateOutput> {
    let candidate_bins = [
        "usbipd",
        r"C:\Program Files\usbipd-win\usbipd.exe",
    ];

    for bin in candidate_bins {
        let mut cmd = Command::new(bin);
        cmd.arg("state");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        if let Ok(output) = cmd.output() {
            if output.status.success() {
                if let Ok(parsed) = serde_json::from_slice::<UsbipdStateOutput>(&output.stdout) {
                    debug!(
                        device_count = parsed.devices.len(),
                        binary = bin,
                        "Loaded live state from usbipd-win"
                    );
                    return Some(parsed);
                }
            }
        }
    }

    None
}

/// Parses raw JSON text from `usbipd state` (used by unit and integration tests as well as runtime).
pub fn parse_usbipd_state_json(json_str: &str) -> Result<UsbipdStateOutput, serde_json::Error> {
    serde_json::from_str(json_str)
}

