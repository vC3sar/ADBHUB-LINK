//! Host-side process and socket controller for `usbipd-win` (binding, unbinding,
//! fast pre-bind for MediaTek BROM / SPD, and opening local `127.0.0.1:3240` import streams).

use std::net::SocketAddr;
use std::process::Command;
use std::time::Duration;

use adbhub_core::{AdbHubError, BusId, CoreResult, UsbIpExportState};
use adbhub_usb_enumerator::query_usbipd_state;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

use crate::protocol::{
    encode_op_req_import, OpHeader, UsbIpExportedDevice, DEFAULT_USBIP_PORT, OP_REP_IMPORT,
    USBIP_EXPORTED_DEVICE_SIZE,
};

/// Trait abstracting host-side USB/IP driver binding (`usbipd bind` / `unbind`) so production
/// code interacts with `usbipd-win` while integration tests can run end-to-end without requiring
/// Administrator elevation on CI/test runners.
pub trait HostUsbipBindController: Send + Sync {
    /// Checks whether `usbipd-win` is installed and reachable on the host PC.
    fn is_installed(&self) -> bool;

    /// Binds (`shares`) a physical USB port identified by `bus_id` so `usbipd-win` attaches its
    /// signed `VBoxUsb` stub driver.
    ///
    /// When `force` is `true` (required on Windows when WPD/MTP or composite drivers hold the device),
    /// passes `--force`.
    fn bind_bus_id(&self, bus_id: &BusId, force: bool) -> CoreResult<UsbIpExportState>;

    /// Unbinds a previously shared `bus_id`, returning control to the local Windows host drivers.
    fn unbind_bus_id(&self, bus_id: &BusId) -> CoreResult<()>;

    /// Queries the current [`UsbIpExportState`] for `bus_id`.
    fn query_bus_export_state(&self, bus_id: &BusId) -> CoreResult<UsbIpExportState>;
}

/// Production controller for `dorssel/usbipd-win`.
#[derive(Debug, Clone)]
pub struct UsbipdWinController {
    pub executable_path: String,
    pub daemon_addr: SocketAddr,
}

impl Default for UsbipdWinController {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbipdWinController {
    /// Creates a controller resolving `usbipd.exe` from standard installation paths.
    pub fn new() -> Self {
        let default_prog = r"C:\Program Files\usbipd-win\usbipd.exe";
        let executable_path = if std::path::Path::new(default_prog).exists() {
            default_prog.to_string()
        } else {
            "usbipd".to_string()
        };
        Self {
            executable_path,
            daemon_addr: SocketAddr::from(([127, 0, 0, 1], DEFAULT_USBIP_PORT)),
        }
    }

    /// Connects to the local `usbipd-win` TCP service (`127.0.0.1:3240`), sends `OP_REQ_IMPORT`
    /// for `bus_id`, validates `OP_REP_IMPORT`, and returns the upgraded [`TcpStream`] ready for
    /// bidirectional URB forwarding (`USBIP_CMD_SUBMIT` / `USBIP_RET_SUBMIT`) along with the
    /// 312-byte [`UsbIpExportedDevice`] descriptor.
    pub async fn connect_and_import_device(
        &self,
        bus_id: &BusId,
        connect_timeout: Duration,
    ) -> CoreResult<(TcpStream, UsbIpExportedDevice)> {
        connect_and_import_from_addr(self.daemon_addr, bus_id, connect_timeout).await
    }
}

/// Connects to any USB/IP server socket (`addr`), performs the `OP_REQ_IMPORT` / `OP_REP_IMPORT`
/// handshake with `TCP_NODELAY` enabled, and returns the live URB stream + exported device record.
pub async fn connect_and_import_from_addr(
    addr: SocketAddr,
    bus_id: &BusId,
    connect_timeout: Duration,
) -> CoreResult<(TcpStream, UsbIpExportedDevice)> {
    let stream = tokio::time::timeout(connect_timeout, TcpStream::connect(addr))
        .await
        .map_err(|_| {
            AdbHubError::WindowsUsbError(format!(
                "Timeout connecting to USB/IP endpoint {addr} for busid {bus_id}"
            ))
        })?
        .map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed to connect to USB/IP endpoint {addr}: {e}"
            ))
        })?;

    // Disable Nagle's algorithm so 48-byte URB headers and ZLP packets are flushed immediately
    stream.set_nodelay(true).map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed to set TCP_NODELAY on USB/IP socket: {e}"))
    })?;

    let mut stream = stream;
    let req_bytes = encode_op_req_import(bus_id);
    stream.write_all(&req_bytes).await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed to send OP_REQ_IMPORT for {bus_id}: {e}"))
    })?;

    let mut rep_hdr_buf = [0u8; 8];
    stream.read_exact(&mut rep_hdr_buf).await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!(
            "Failed to read OP_REP_IMPORT header for {bus_id}: {e}"
        ))
    })?;

    let rep_hdr = OpHeader::from_bytes(&rep_hdr_buf)?;
    if rep_hdr.command != OP_REP_IMPORT {
        return Err(AdbHubError::MalformedDescriptor {
            reason: format!(
                "Expected OP_REP_IMPORT (0x0003), got 0x{:04X}",
                rep_hdr.command
            ),
            offset: 2,
            available: 8,
        });
    }
    if rep_hdr.status != 0 {
        return Err(AdbHubError::WindowsUsbError(format!(
            "USB/IP server rejected OP_REQ_IMPORT for busid {bus_id} with status {}",
            rep_hdr.status
        )));
    }

    let mut dev_buf = [0u8; USBIP_EXPORTED_DEVICE_SIZE];
    stream.read_exact(&mut dev_buf).await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!(
            "Failed to read 312-byte usbip_usb_device record for {bus_id}: {e}"
        ))
    })?;

    let exported_device = UsbIpExportedDevice::from_wire_bytes_312(&dev_buf)?;
    debug!(
        bus_id = %exported_device.bus_id,
        vid_pid = %exported_device.vid_pid(),
        speed = ?exported_device.speed,
        "USB/IP OP_REQ_IMPORT handshake succeeded"
    );

    Ok((stream, exported_device))
}

fn silent_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

impl HostUsbipBindController for UsbipdWinController {
    fn is_installed(&self) -> bool {
        silent_command(&self.executable_path)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn bind_bus_id(&self, bus_id: &BusId, force: bool) -> CoreResult<UsbIpExportState> {
        let mut cmd = silent_command(&self.executable_path);
        cmd.arg("bind").arg("--busid").arg(bus_id.to_string());
        if force {
            cmd.arg("--force");
        }

        info!(
            bus_id = %bus_id,
            force,
            "Executing usbipd bind on host"
        );

        let out = cmd.output().map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed to execute '{} bind --busid {bus_id}': {e}",
                self.executable_path
            ))
        })?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            warn!(bus_id = %bus_id, %stderr, "usbipd bind returned non-zero exit code");
            return Err(AdbHubError::WindowsUsbError(format!(
                "usbipd bind failed for {bus_id}: {}",
                stderr.trim()
            )));
        }

        self.query_bus_export_state(bus_id)
    }

    fn unbind_bus_id(&self, bus_id: &BusId) -> CoreResult<()> {
        let out = silent_command(&self.executable_path)
            .arg("unbind")
            .arg("--busid")
            .arg(bus_id.to_string())
            .output()
            .map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed to execute '{} unbind --busid {bus_id}': {e}",
                    self.executable_path
                ))
            })?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(AdbHubError::WindowsUsbError(format!(
                "usbipd unbind failed for {bus_id}: {}",
                stderr.trim()
            )));
        }
        info!(bus_id = %bus_id, "Unbound USB device from usbipd-win");
        Ok(())
    }

    fn query_bus_export_state(&self, bus_id: &BusId) -> CoreResult<UsbIpExportState> {
        if let Some(state) = query_usbipd_state() {
            for dev in state.devices {
                if dev.parsed_bus_id().as_ref() == Some(bus_id) {
                    return Ok(dev.export_state());
                }
            }
        }
        Ok(UsbIpExportState::NotBound)
    }
}

