//! # `adbhub-usbip-client-adapter`
//!
//! Technician-side Windows Virtual USB Stack (`usbip-win2` UDE/VHCI) adapter and local
//! loopback USB/IP server bridge.
//!
//! ### How the Technician PC Mounts the Remote USB Device:
//! 1. In LAN/Direct mode (Phase 2) or QUIC Relay mode (Phase 3), `ADBHUB-LINK Desktop` spins up a
//!    local loopback TCP listener (`127.0.0.1:<ephemeral_port>`) or connects directly to the
//!    authorized tunnel.
//! 2. `UsbIpWin2ClientController` invokes the signed `usbip-win2` client (`usbip.exe attach -r 127.0.0.1 -P <port> -b <busid>`)
//!    so the kernel UDE driver (`usbip2_ude.sys`) connects to the local bridge socket via Winsock Kernel (WSK).
//! 3. Windows Plug and Play on the technician's machine enumerates the virtual USB device and loads
//!    the technician's local OEM drivers (`ssudbus.sys`/`ssudmdm.sys` for Samsung Odin, `qcser.sys`
//!    for Qualcomm 9008, `usb2ser.sys` for MediaTek BROM, `SciU2S.sys` for SPD/Unisoc, `winusb.sys` for ADB/MTP).

use std::net::SocketAddr;
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use adbhub_core::{AdbHubError, BusId, CoreResult, UsbDeviceMode};
use adbhub_usb_exporter::{pump_usbip_urb_session, ExporterSessionConfig, UrbSessionMetrics, UrbTransferTracker};
use adbhub_usbip_adapter::{
    OpHeader, UsbIpExportedDevice, OP_REP_IMPORT, OP_REQ_IMPORT, USBIP_EXPORTED_DEVICE_SIZE,
    USBIP_VERSION,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, info};

/// Represents an attached virtual USB port on the technician's Windows VHCI/UDE driver (`usbip-win2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachedVhciPort {
    /// Virtual port number assigned by `usbip-win2` (e.g. `Port 00` -> `0`, `Port 01` -> `1`).
    pub vhci_port: u16,
    /// Remote `BusId` imported on this virtual port.
    pub remote_bus_id: BusId,
    /// Remote endpoint description (e.g. `"127.0.0.1:3240"`).
    pub remote_endpoint: String,
}

/// Trait abstracting the technician's local VHCI driver controller (`usbip-win2`).
pub trait TechnicianVhciController: Send + Sync {
    /// Verifies that the signed `usbip-win2` UDE driver and CLI are installed on the technician PC.
    fn is_installed(&self) -> bool;

    /// Attaches a remote USB device (`bus_id`) exposed at `server_addr` into the local Windows
    /// Virtual USB Stack (UDE), returning the assigned virtual port.
    fn attach_device(&self, server_addr: SocketAddr, bus_id: &BusId) -> CoreResult<AttachedVhciPort>;

    /// Detaches a virtual USB port from the technician's Windows Virtual USB Stack.
    fn detach_port(&self, vhci_port: u16) -> CoreResult<()>;

    /// Lists all currently attached virtual USB ports on the technician PC.
    fn list_attached_ports(&self) -> CoreResult<Vec<AttachedVhciPort>>;
}

/// Production controller for `vadimgrn/usbip-win2` (`usbip.exe`).
#[derive(Debug, Clone)]
pub struct UsbIpWin2ClientController {
    pub executable_path: String,
}

impl Default for UsbIpWin2ClientController {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbIpWin2ClientController {
    pub fn new() -> Self {
        let candidates = [
            r"C:\Program Files\USBip\usbip.exe",
            r"C:\Program Files\usbip-win2\usbip.exe",
        ];
        let executable_path = candidates
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .unwrap_or("usbip")
            .to_string();

        Self { executable_path }
    }
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

impl TechnicianVhciController for UsbIpWin2ClientController {
    fn is_installed(&self) -> bool {
        silent_command(&self.executable_path)
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn attach_device(
        &self,
        server_addr: SocketAddr,
        bus_id: &BusId,
    ) -> CoreResult<AttachedVhciPort> {
        let host_ip = server_addr.ip().to_string();
        let tcp_port = server_addr.port().to_string();

        info!(
            %server_addr,
            bus_id = %bus_id,
            "Attaching remote USB device to technician Windows UDE/VHCI stack via usbip-win2"
        );

        let out = silent_command(&self.executable_path)
            .args([
                "attach",
                "-r",
                &host_ip,
                "-P",
                &tcp_port,
                "-b",
                &bus_id.to_string(),
            ])
            .output()
            .map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed to execute '{} attach': {e}",
                    self.executable_path
                ))
            })?;

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);

        if !out.status.success() {
            return Err(AdbHubError::WindowsUsbError(format!(
                "usbip-win2 attach failed for {bus_id} at {server_addr}: {}",
                stderr.trim()
            )));
        }

        // Parse assigned port number from stdout (e.g. "succesfully attached to port 1" or via `usbip port`)
        let vhci_port = parse_attached_port_number(&stdout).unwrap_or(1);

        Ok(AttachedVhciPort {
            vhci_port,
            remote_bus_id: bus_id.clone(),
            remote_endpoint: server_addr.to_string(),
        })
    }

    fn detach_port(&self, vhci_port: u16) -> CoreResult<()> {
        let out = silent_command(&self.executable_path)
            .args(["detach", "-p", &vhci_port.to_string()])
            .output()
            .map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed to execute '{} detach -p {vhci_port}': {e}",
                    self.executable_path
                ))
            })?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(AdbHubError::WindowsUsbError(format!(
                "usbip-win2 detach failed for port {vhci_port}: {}",
                stderr.trim()
            )));
        }

        info!(vhci_port, "Detached virtual USB port from Windows UDE stack");
        Ok(())
    }

    fn list_attached_ports(&self) -> CoreResult<Vec<AttachedVhciPort>> {
        let out = silent_command(&self.executable_path)
            .arg("port")
            .output()
            .map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed to execute '{} port': {e}",
                    self.executable_path
                ))
            })?;

        let stdout = String::from_utf8_lossy(&out.stdout);
        Ok(parse_usbip_port_output(&stdout))
    }
}

/// Extracts the virtual port number from `usbip attach` output.
pub fn parse_attached_port_number(output: &str) -> Option<u16> {
    let lower = output.to_ascii_lowercase();
    if let Some(idx) = lower.find("port ") {
        let after = &lower[idx + 5..];
        let digits: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(p) = digits.parse::<u16>() {
            return Some(p);
        }
    }
    None
}

/// Parses the output of `usbip port` into structured [`AttachedVhciPort`] records.
pub fn parse_usbip_port_output(output: &str) -> Vec<AttachedVhciPort> {
    let mut ports = Vec::new();
    let mut current_port: Option<u16> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Port ") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            current_port = digits.parse::<u16>().ok();
        } else if let Some(p) = current_port {
            if let Some(uri_idx) = trimmed.find("usbip://") {
                let uri = &trimmed[uri_idx + 8..];
                // Format: `usbip://127.0.0.1:3240/1-4`
                if let Some((endpoint, busid_part)) = uri.split_once('/') {
                    let clean_bus = busid_part.split_whitespace().next().unwrap_or("");
                    if let Ok(bus_id) = BusId::from_str(clean_bus) {
                        ports.push(AttachedVhciPort {
                            vhci_port: p,
                            remote_bus_id: bus_id,
                            remote_endpoint: endpoint.to_string(),
                        });
                        current_port = None;
                    }
                }
            }
        }
    }

    ports
}

/// Local USB/IP Loopback Bridge on the Technician PC (`127.0.0.1:<ephemeral_port>`).
///
/// The `usbip-win2` kernel UDE driver connects via standard TCP (`OP_REQ_IMPORT`) to this local
/// listener; `LocalVhciBridgeServer` validates the requested `BusId`, responds with `OP_REP_IMPORT` +
/// the 312-byte [`UsbIpExportedDevice`] descriptor, and then hands the socket over to
/// [`pump_usbip_urb_session`] to forward URBs over the remote transport stream.
pub struct LocalVhciBridgeServer {
    listener: TcpListener,
    local_addr: SocketAddr,
}

impl LocalVhciBridgeServer {
    /// Binds a loopback listener on `127.0.0.1:0` (OS-assigned ephemeral port).
    pub async fn bind_ephemeral() -> CoreResult<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed to bind local VHCI loopback listener on 127.0.0.1:0: {e}"
            ))
        })?;
        let local_addr = listener.local_addr().map_err(|e| {
            AdbHubError::WindowsUsbError(format!("Failed to get local_addr of VHCI bridge: {e}"))
        })?;
        Ok(Self {
            listener,
            local_addr,
        })
    }

    /// Returns the local `127.0.0.1:<port>` address to pass to `usbip.exe attach`.
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Accepts one connection from the technician's local `usbip-win2` driver, completes the
    /// `OP_REQ_IMPORT` / `OP_REP_IMPORT` handshake for `exported_device`, and pumps all URBs
    /// between the local VHCI driver and `remote_urb_stream` with mode-specific timeouts and backpressure.
    pub async fn accept_and_bridge_session<R>(
        self,
        exported_device: UsbIpExportedDevice,
        mode: UsbDeviceMode,
        remote_urb_stream: R,
    ) -> CoreResult<(UrbSessionMetrics, Arc<UrbTransferTracker>)>
    where
        R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (mut vhci_stream, peer_addr) = self.listener.accept().await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed to accept VHCI connection on {}: {e}",
                self.local_addr
            ))
        })?;

        vhci_stream.set_nodelay(true).map_err(|e| {
            AdbHubError::WindowsUsbError(format!("Failed to set TCP_NODELAY on VHCI socket: {e}"))
        })?;

        debug!(%peer_addr, "Accepted local connection from technician VHCI driver");

        // 1. Read 40-byte OP_REQ_IMPORT (8B OpHeader + 32B busid)
        let mut req_buf = [0u8; 40];
        vhci_stream.read_exact(&mut req_buf).await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed reading OP_REQ_IMPORT from local VHCI driver: {e}"
            ))
        })?;

        let req_hdr = OpHeader::from_bytes(&req_buf[..8])?;
        if req_hdr.command != OP_REQ_IMPORT {
            return Err(AdbHubError::MalformedDescriptor {
                reason: format!(
                    "Expected OP_REQ_IMPORT (0x8003) from VHCI driver, got 0x{:04X}",
                    req_hdr.command
                ),
                offset: 2,
                available: 40,
            });
        }

        let busid_end = req_buf[8..40].iter().position(|&b| b == 0).unwrap_or(32);
        let requested_busid = String::from_utf8_lossy(&req_buf[8..8 + busid_end]);
        let parsed_req_busid = BusId::from_str(requested_busid.trim())?;

        if parsed_req_busid != exported_device.bus_id {
            // Send error status 1 in OP_REP_IMPORT
            let err_rep = OpHeader {
                version: USBIP_VERSION,
                command: OP_REP_IMPORT,
                status: 1,
            };
            let _ = vhci_stream.write_all(&err_rep.to_bytes()).await;
            return Err(AdbHubError::InvalidBusId(format!(
                "VHCI requested busid {parsed_req_busid}, expected {}",
                exported_device.bus_id
            )));
        }

        // 2. Send OP_REP_IMPORT (8B header with status=0 + 312B UsbIpExportedDevice)
        let ok_rep = OpHeader {
            version: USBIP_VERSION,
            command: OP_REP_IMPORT,
            status: 0,
        };
        let mut rep_pkt = [0u8; 8 + USBIP_EXPORTED_DEVICE_SIZE];
        rep_pkt[..8].copy_from_slice(&ok_rep.to_bytes());
        rep_pkt[8..].copy_from_slice(&exported_device.to_wire_bytes_312());

        vhci_stream.write_all(&rep_pkt).await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed sending OP_REP_IMPORT to local VHCI driver: {e}"
            ))
        })?;
        vhci_stream.flush().await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed flushing OP_REP_IMPORT to local VHCI driver: {e}"
            ))
        })?;

        info!(
            bus_id = %exported_device.bus_id,
            vid_pid = %exported_device.vid_pid(),
            %mode,
            "VHCI OP_REP_IMPORT completed; starting bidirectional URB passthrough"
        );

        // 3. Pump URBs between remote host stream and technician VHCI stream
        let config = ExporterSessionConfig::for_mode(mode);
        let tracker = Arc::new(UrbTransferTracker::new(config));
        let metrics =
            pump_usbip_urb_session(remote_urb_stream, vhci_stream, Arc::clone(&tracker)).await?;

        Ok((metrics, tracker))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod multiprotocol_validation;

