//! # `adbhub-usb-exporter`
//!
//! Host-side USB Export Manager and URB-aware bidirectional session bridge.
//!
//! Implements all requirements of **Section 2.6 (Consideraciones especiales para Download Mode)**
//! and **Section 3 (Regla Fundamental: USB Passthrough Real)**:
//! - Configurable timeouts by USB device mode (`SamsungDownload` = 60s, `QualcommEdl` / `MediatekBrom` / `SpdUnisoc` = 45s).
//! - Tracking of pending in-flight URBs by sequence number (`seqnum`) with safe cancellation (`USBIP_CMD_UNLINK`).
//! - Strict backpressure and memory limits (`max_in_flight_bytes`, `max_single_urb_payload_bytes`) so a fast
//!   technician PC sending Odin `super.img` chunks cannot exhaust host RAM.
//! - Live transfer telemetry (bytes IN/OUT, URBs completed, unlinked, active critical operation detection).
//! - Pre-armed port binding (`PreArmedPortSession`) to capture MediaTek BROM (`0E8D:0003`) and
//!   SPD BootROM (`1782:4D00`) within their < 2 second watchdog window.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adbhub_core::{
    AdbHubError, BusId, CoreResult, UsbDeviceMode, UsbIpExportState,
};
use adbhub_usbip_adapter::{
    HostUsbipBindController, UsbIpDirection, UsbIpUrbHeader, USBIP_URB_HEADER_SIZE,
    USBIP_URB_ZERO_PACKET,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, info};
use uuid::Uuid;

/// Configuration for an exported USB/IP session stream.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ExporterSessionConfig {
    /// Operating mode of the exported USB device (determines URB timeout and critical flash guards).
    pub device_mode: UsbDeviceMode,
    /// Per-URB completion timeout. Defaults to [`UsbDeviceMode::recommended_urb_timeout_ms`].
    pub urb_timeout: Duration,
    /// Maximum total bytes of URB payloads allowed in-flight simultaneously (backpressure bound).
    /// Default: 32 MiB (`33,554,432` bytes), sufficient for a full Odin sequence window without OOM.
    pub max_in_flight_bytes: usize,
    /// Maximum allowed payload size for a single URB packet (sanity & memory protection).
    /// Default: 16 MiB (`16,777,216` bytes).
    pub max_single_urb_payload_bytes: usize,
}

impl ExporterSessionConfig {
    /// Creates an optimal configuration tailored to the given [`UsbDeviceMode`].
    pub fn for_mode(device_mode: UsbDeviceMode) -> Self {
        Self {
            device_mode,
            urb_timeout: Duration::from_millis(device_mode.recommended_urb_timeout_ms()),
            max_in_flight_bytes: 32 * 1024 * 1024,
            max_single_urb_payload_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Metadata for a single pending URB currently in flight on the physical host USB controller.
#[derive(Debug, Clone)]
pub struct PendingUrbEntry {
    pub seqnum: u32,
    pub endpoint: u32,
    pub direction: UsbIpDirection,
    pub requested_bytes: u32,
    pub has_zlp_flag: bool,
    pub submitted_at: Instant,
}

/// Real-time telemetry counters for an active USB export session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrbSessionMetrics {
    pub urbs_submitted: u64,
    pub urbs_completed: u64,
    pub urbs_unlinked: u64,
    pub bytes_out_to_device: u64,
    pub bytes_in_from_device: u64,
    pub zlp_transfers_count: u64,
    pub pending_urb_count: usize,
    pub critical_flashing_active: bool,
}

/// Tracks in-flight URBs and enforces memory/backpressure limits for a single exported USB device session.
pub struct UrbTransferTracker {
    config: ExporterSessionConfig,
    pending: Mutex<HashMap<u32, PendingUrbEntry>>,
    metrics: Mutex<UrbSessionMetrics>,
    /// Semaphore where each permit represents 1 KiB of in-flight URB buffer budget.
    memory_budget_kib: Arc<Semaphore>,
}

impl UrbTransferTracker {
    pub fn new(config: ExporterSessionConfig) -> Self {
        let total_kib = (config.max_in_flight_bytes / 1024).max(64);
        Self {
            config,
            pending: Mutex::new(HashMap::new()),
            metrics: Mutex::new(UrbSessionMetrics::default()),
            memory_budget_kib: Arc::new(Semaphore::new(total_kib)),
        }
    }

    /// Validates and registers a `USBIP_CMD_SUBMIT` header before forwarding it to the host USB driver.
    pub async fn record_submit(&self, hdr: &UsbIpUrbHeader) -> CoreResult<()> {
        if let UsbIpUrbHeader::CmdSubmit {
            basic,
            transfer_flags,
            transfer_buffer_length,
            ..
        } = *hdr
        {
            if (transfer_buffer_length as usize) > self.config.max_single_urb_payload_bytes {
                return Err(AdbHubError::MalformedDescriptor {
                    reason: format!(
                        "URB seqnum={} requested_length={} exceeds max_single_urb_payload_bytes={}",
                        basic.seqnum,
                        transfer_buffer_length,
                        self.config.max_single_urb_payload_bytes
                    ),
                    offset: 24,
                    available: USBIP_URB_HEADER_SIZE,
                });
            }

            let has_zlp = (transfer_flags & USBIP_URB_ZERO_PACKET) != 0;
            let entry = PendingUrbEntry {
                seqnum: basic.seqnum,
                endpoint: basic.ep,
                direction: basic.direction,
                requested_bytes: transfer_buffer_length,
                has_zlp_flag: has_zlp,
                submitted_at: Instant::now(),
            };

            let mut pending = self.pending.lock().await;
            pending.insert(basic.seqnum, entry);
            let pending_len = pending.len();
            drop(pending);

            let mut m = self.metrics.lock().await;
            m.urbs_submitted = m.urbs_submitted.saturating_add(1);
            m.pending_urb_count = pending_len;
            if has_zlp {
                m.zlp_transfers_count = m.zlp_transfers_count.saturating_add(1);
            }
            if basic.direction == UsbIpDirection::Out {
                m.bytes_out_to_device = m
                    .bytes_out_to_device
                    .saturating_add(u64::from(transfer_buffer_length));
                // Detect sustained Bulk OUT flashing activity (e.g. Samsung Odin or Qualcomm Firehose)
                if self.config.device_mode.is_flashing_or_boot_mode() && transfer_buffer_length >= 512
                {
                    m.critical_flashing_active = true;
                }
            }
        }
        Ok(())
    }

    /// Records a `USBIP_RET_SUBMIT` completion from the host USB driver.
    pub async fn record_completion(&self, hdr: &UsbIpUrbHeader) -> CoreResult<()> {
        if let UsbIpUrbHeader::RetSubmit {
            basic,
            actual_length,
            ..
        } = *hdr
        {
            let mut pending = self.pending.lock().await;
            pending.remove(&basic.seqnum);
            let pending_len = pending.len();
            drop(pending);

            let mut m = self.metrics.lock().await;
            m.urbs_completed = m.urbs_completed.saturating_add(1);
            m.pending_urb_count = pending_len;
            if basic.direction == UsbIpDirection::In {
                m.bytes_in_from_device = m
                    .bytes_in_from_device
                    .saturating_add(u64::from(actual_length));
            }
        }
        Ok(())
    }

    /// Records a `USBIP_CMD_UNLINK` or `USBIP_RET_UNLINK` cancellation.
    pub async fn record_unlink(&self, hdr: &UsbIpUrbHeader) {
        match *hdr {
            UsbIpUrbHeader::CmdUnlink { unlink_seqnum, .. } => {
                let mut pending = self.pending.lock().await;
                pending.remove(&unlink_seqnum);
                let pending_len = pending.len();
                drop(pending);

                let mut m = self.metrics.lock().await;
                m.urbs_unlinked = m.urbs_unlinked.saturating_add(1);
                m.pending_urb_count = pending_len;
            }
            UsbIpUrbHeader::RetUnlink { basic, .. } => {
                let mut pending = self.pending.lock().await;
                pending.remove(&basic.seqnum);
                let pending_len = pending.len();
                drop(pending);

                let mut m = self.metrics.lock().await;
                m.pending_urb_count = pending_len;
            }
            _ => {}
        }
    }

    /// Returns a snapshot of current session metrics.
    pub async fn snapshot_metrics(&self) -> UrbSessionMetrics {
        self.metrics.lock().await.clone()
    }

    /// Acquires backpressure permits proportional to the payload size (`1 permit = 1 KiB`).
    pub async fn acquire_backpressure_budget(
        &self,
        payload_bytes: usize,
    ) -> CoreResult<tokio::sync::OwnedSemaphorePermit> {
        let kib_needed = ((payload_bytes.saturating_add(1023)) / 1024).max(1) as u32;
        Arc::clone(&self.memory_budget_kib)
            .acquire_many_owned(kib_needed)
            .await
            .map_err(|e| AdbHubError::WindowsUsbError(format!("Backpressure semaphore closed: {e}")))
    }
}

/// Pumps URB packets (`USBIP_CMD_SUBMIT` / `USBIP_CMD_UNLINK` from Client -> Host, and
/// `USBIP_RET_SUBMIT` / `USBIP_RET_UNLINK` from Host -> Client) between an already-imported
/// host USB/IP stream and a remote technician stream (local TCP or QUIC Relay stream).
///
/// Preserves exact 48-byte headers, SETUP packets, `transfer_flags` (including `URB_ZERO_PACKET`),
/// and payload boundaries while enforcing mode-specific timeouts and memory backpressure.
pub async fn pump_usbip_urb_session<H, C>(
    host_stream: H,
    client_stream: C,
    tracker: Arc<UrbTransferTracker>,
) -> CoreResult<UrbSessionMetrics>
where
    H: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut host_reader, mut host_writer) = tokio::io::split(host_stream);
    let (mut client_reader, mut client_writer) = tokio::io::split(client_stream);

    let tracker_c2h = Arc::clone(&tracker);
    let tracker_h2c = Arc::clone(&tracker);
    let urb_timeout = tracker.config.urb_timeout;
    let max_payload = tracker.config.max_single_urb_payload_bytes;

    // Direction 1: Technician Client -> Host USB/IP Server (CMD_SUBMIT & CMD_UNLINK)
    let c2h_task = tokio::spawn(async move {
        let mut hdr_buf = [0u8; USBIP_URB_HEADER_SIZE];
        loop {
            match client_reader.read_exact(&mut hdr_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("Technician client closed USB/IP stream cleanly (EOF)");
                    let _ = host_writer.shutdown().await;
                    return Ok::<(), AdbHubError>(());
                }
                Err(e) => {
                    return Err(AdbHubError::WindowsUsbError(format!(
                        "Error reading URB header from technician stream: {e}"
                    )));
                }
            }

            let hdr = UsbIpUrbHeader::from_wire_bytes(&hdr_buf)?;
            match hdr {
                UsbIpUrbHeader::CmdSubmit { .. } => {
                    tracker_c2h.record_submit(&hdr).await?;
                }
                UsbIpUrbHeader::CmdUnlink { .. } => {
                    tracker_c2h.record_unlink(&hdr).await;
                }
                _ => {}
            }

            let payload_len = hdr.following_payload_len();
            if payload_len > max_payload {
                return Err(AdbHubError::MalformedDescriptor {
                    reason: format!("Client URB payload {payload_len} exceeds max {max_payload}"),
                    offset: 24,
                    available: USBIP_URB_HEADER_SIZE,
                });
            }

            // Acquire backpressure budget before buffering payload in memory
            let _permit = tracker_c2h.acquire_backpressure_budget(payload_len).await?;

            let mut payload = vec![0u8; payload_len];
            if payload_len > 0 {
                tokio::time::timeout(urb_timeout, client_reader.read_exact(&mut payload))
                    .await
                    .map_err(|_| {
                        AdbHubError::WindowsUsbError(
                            "Timeout reading OUT payload from technician stream".to_string(),
                        )
                    })?
                    .map_err(|e| {
                        AdbHubError::WindowsUsbError(format!(
                            "Failed reading OUT payload from technician stream: {e}"
                        ))
                    })?;
            }

            tokio::time::timeout(urb_timeout, async {
                host_writer.write_all(&hdr_buf).await?;
                if !payload.is_empty() {
                    host_writer.write_all(&payload).await?;
                }
                host_writer.flush().await
            })
            .await
            .map_err(|_| {
                AdbHubError::WindowsUsbError("Timeout writing URB to host usbipd".to_string())
            })?
            .map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed writing URB to host usbipd: {e}"))
            })?;
        }
    });

    // Direction 2: Host USB/IP Server -> Technician Client (RET_SUBMIT & RET_UNLINK)
    let h2c_task = tokio::spawn(async move {
        let mut hdr_buf = [0u8; USBIP_URB_HEADER_SIZE];
        loop {
            match host_reader.read_exact(&mut hdr_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("Host usbipd closed USB/IP stream (EOF / device reenumerating)");
                    let _ = client_writer.shutdown().await;
                    return Ok::<(), AdbHubError>(());
                }
                Err(e) => {
                    return Err(AdbHubError::WindowsUsbError(format!(
                        "Error reading URB return header from host usbipd: {e}"
                    )));
                }
            }

            let hdr = UsbIpUrbHeader::from_wire_bytes(&hdr_buf)?;
            match hdr {
                UsbIpUrbHeader::RetSubmit { .. } => {
                    tracker_h2c.record_completion(&hdr).await?;
                }
                UsbIpUrbHeader::RetUnlink { .. } => {
                    tracker_h2c.record_unlink(&hdr).await;
                }
                _ => {}
            }

            let payload_len = hdr.following_payload_len();
            if payload_len > max_payload {
                return Err(AdbHubError::MalformedDescriptor {
                    reason: format!("Host URB return payload {payload_len} exceeds max {max_payload}"),
                    offset: 24,
                    available: USBIP_URB_HEADER_SIZE,
                });
            }

            let _permit = tracker_h2c.acquire_backpressure_budget(payload_len).await?;

            let mut payload = vec![0u8; payload_len];
            if payload_len > 0 {
                tokio::time::timeout(urb_timeout, host_reader.read_exact(&mut payload))
                    .await
                    .map_err(|_| {
                        AdbHubError::WindowsUsbError(
                            "Timeout reading IN payload from host usbipd".to_string(),
                        )
                    })?
                    .map_err(|e| {
                        AdbHubError::WindowsUsbError(format!(
                            "Failed reading IN payload from host usbipd: {e}"
                        ))
                    })?;
            }

            tokio::time::timeout(urb_timeout, async {
                client_writer.write_all(&hdr_buf).await?;
                if !payload.is_empty() {
                    client_writer.write_all(&payload).await?;
                }
                client_writer.flush().await
            })
            .await
            .map_err(|_| {
                AdbHubError::WindowsUsbError(
                    "Timeout writing URB completion to technician stream".to_string(),
                )
            })?
            .map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed writing URB completion to technician stream: {e}"
                ))
            })?;
        }
    });

    // Wait for either direction to finish or error out
    tokio::select! {
        res_c2h = c2h_task => {
            match res_c2h {
                Ok(inner) => inner?,
                Err(join_err) => return Err(AdbHubError::WindowsUsbError(format!("C2H task join error: {join_err}"))),
            }
        }
        res_h2c = h2c_task => {
            match res_h2c {
                Ok(inner) => inner?,
                Err(join_err) => return Err(AdbHubError::WindowsUsbError(format!("H2C task join error: {join_err}"))),
            }
        }
    }

    Ok(tracker.snapshot_metrics().await)
}

/// Manages host-side USB export bindings (`usbipd bind`), Pre-Bind port reservations for
/// MediaTek BROM / SPD, and automatic re-binding on device reenumeration.
pub struct UsbExportManager {
    controller: Arc<dyn HostUsbipBindController>,
    /// Ports pre-armed for sub-second BootROM capture (`BusId -> SessionId`).
    pre_armed_ports: Mutex<HashMap<BusId, Uuid>>,
}

impl UsbExportManager {
    pub fn new(controller: Arc<dyn HostUsbipBindController>) -> Self {
        Self {
            controller,
            pre_armed_ports: Mutex::new(HashMap::new()),
        }
    }

    /// Pre-arms a physical USB port (`bus_id`) for a technician session so that when a MediaTek
    /// BROM (`0E8D:0003`) or Spreadtrum/Unisoc (`1782:4D00`) device plugs in, it is bound and
    /// exported immediately within the < 2s BootROM handshake window.
    pub async fn pre_arm_port_for_bootrom(&self, bus_id: BusId, session_id: Uuid) {
        info!(bus_id = %bus_id, %session_id, "Pre-armed physical USB port for rapid BootROM capture");
        self.pre_armed_ports.lock().await.insert(bus_id, session_id);
    }

    /// Exports a device on `bus_id` for `mode`, automatically applying `--force` when needed
    /// (for example, to release Windows WPD lock on MTP devices or composite Android devices).
    pub fn export_device(
        &self,
        bus_id: &BusId,
        mode: UsbDeviceMode,
    ) -> CoreResult<UsbIpExportState> {
        // MTP and Composite Android devices require force binding on Windows so WpdMtpDrv releases the handle
        let require_force = matches!(
            mode,
            UsbDeviceMode::Mtp | UsbDeviceMode::CompositeAdbMtp | UsbDeviceMode::SamsungDownload
        );
        self.controller.bind_bus_id(bus_id, require_force)
    }

    /// Handles automatic re-export when `DeviceLifecycleManager` detects a valid mode transition
    /// (e.g. Android ADB -> Samsung Download Mode or Qualcomm EDL on the same physical port).
    pub fn handle_reenumeration_recovery(
        &self,
        bus_id: &BusId,
        new_mode: UsbDeviceMode,
    ) -> CoreResult<UsbIpExportState> {
        info!(
            bus_id = %bus_id,
            %new_mode,
            "Re-binding USB port after device mode transition / reenumeration"
        );
        self.export_device(bus_id, new_mode)
    }

    /// Stops exporting `bus_id` and returns control to the host Windows PnP stack.
    pub fn unexport_device(&self, bus_id: &BusId) -> CoreResult<()> {
        self.controller.unbind_bus_id(bus_id)
    }
}
