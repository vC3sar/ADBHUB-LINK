//! Device Lifecycle State Machine (Section 10 — Gestión de Reenumeración USB).

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AdbHubError, CoreResult};
use crate::protocol::UsbDeviceMode;
use crate::usb::{BusId, VidPid};

/// All states required by Section 10 of the ADBHUB-LINK specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceLifecycleState {
    /// Physically detected on host USB bus.
    Discovered,
    /// Ready to be shared by the host operator.
    Available,
    /// Explicitly authorized/shared by the host operator for remote sessions.
    Shared,
    /// Assigned to an approved technician session, awaiting data-plane connection.
    Reserved,
    /// Technician is establishing the QUIC + USB/IP attachment.
    Connecting,
    /// Active USB/IP passthrough session in progress.
    Active,
    /// Device temporarily disconnected during a mode switch or reboot (e.g. Android -> Download Mode / EDL).
    Reenumerating,
    /// Physically unplugged outside of an expected reenumeration window, or grace period expired.
    Disconnected,
    /// Re-enumerated on the same physical topology; re-binding and re-attaching session.
    Recovering,
    /// Session gracefully terminated and resources freed.
    Released,
    /// Unrecoverable error state requiring reset or manual intervention.
    Error,
}

impl DeviceLifecycleState {
    /// Validates whether transitioning from `self` to `next` is permitted by the state machine.
    pub const fn can_transition_to(self, next: Self) -> bool {
        use DeviceLifecycleState::*;
        if matches!(next, Error) {
            return true;
        }
        match (self, next) {
            (Discovered, Available | Shared | Disconnected) => true,
            (Available, Shared | Disconnected) => true,
            (Shared, Available | Reserved | Connecting | Disconnected | Released) => true,
            (Reserved, Connecting | Active | Shared | Released | Disconnected) => true,
            (Connecting, Active | Reenumerating | Disconnected | Released) => true,
            (Active, Reenumerating | Disconnected | Released) => true,
            (Reenumerating, Recovering | Active | Disconnected | Released) => true,
            (Recovering, Active | Reenumerating | Disconnected | Released) => true,
            (Disconnected, Discovered | Available | Recovering | Released) => true,
            (Released, Available | Shared | Disconnected) => true,
            (Error, Available | Released | Disconnected) => true,
            _ => false,
        }
    }

    /// Attempts a state transition or returns [`AdbHubError::InvalidLifecycleTransition`].
    pub fn transition_to(self, next: Self, device_key: &str) -> CoreResult<Self> {
        if self == next {
            return Ok(self);
        }
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(AdbHubError::InvalidLifecycleTransition {
                device_key: device_key.to_string(),
                from: self.to_string(),
                to: next.to_string(),
            })
        }
    }
}

impl fmt::Display for DeviceLifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Discovered => "DISCOVERED",
            Self::Available => "AVAILABLE",
            Self::Shared => "SHARED",
            Self::Reserved => "RESERVED",
            Self::Connecting => "CONNECTING",
            Self::Active => "ACTIVE",
            Self::Reenumerating => "REENUMERATING",
            Self::Disconnected => "DISCONNECTED",
            Self::Recovering => "RECOVERING",
            Self::Released => "RELEASED",
            Self::Error => "ERROR",
        };
        f.write_str(s)
    }
}

/// Logical identity of a mobile device across mode transitions (Section 10:
/// "No identificar dispositivos exclusivamente por VID/PID").
///
/// When an Android device reboots into Samsung Download Mode (`04E8:6860` -> `04E8:685D`),
/// Qualcomm EDL (`05C6:9008`), or MediaTek BROM (`0E8D:0003`), its `VID:PID` and USB serial
/// number frequently change or disappear altogether (for example, MTK BROM and Qualcomm 9008
/// often have no serial string or a generic SoC serial).
///
/// Therefore, `LogicalDeviceIdentity` anchors the device by combining:
/// 1. Physical USB port topology (`BusId` + Windows PnP location path).
/// 2. Optional SHA-256 serial hash (when present).
/// 3. Historical chain of observed `(VidPid, UsbDeviceMode)` transitions during the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalDeviceIdentity {
    /// Deterministic or session-persistent UUID for this logical device slot.
    pub logical_id: Uuid,
    /// Physical USB bus/port topology (`<bus>-<port>`).
    pub bus_id: BusId,
    /// Optional Windows PnP location string (e.g. `PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(4)`).
    pub location_path: Option<String>,
    /// Last known SHA-256 serial hash (preserved when transitioning into serial-less boot modes).
    pub last_known_serial_hash: Option<String>,
    /// Initial `VID:PID` when first discovered.
    pub origin_vid_pid: VidPid,
    /// Current `VID:PID` on the bus.
    pub current_vid_pid: VidPid,
    /// Previous mode before the latest reenumeration, if any.
    pub previous_mode: Option<UsbDeviceMode>,
    /// Current classified USB mode.
    pub current_mode: UsbDeviceMode,
    /// Number of reenumerations tracked in the current logical lifecycle.
    pub reenumeration_count: u32,
    /// Timestamp of the last state or mode transition.
    pub updated_at: DateTime<Utc>,
}

impl LogicalDeviceIdentity {
    /// Evaluates whether a newly enumerated USB device on `candidate_bus_id` with `candidate_vid_pid`
    /// and `candidate_mode` belongs to this logical device undergoing reenumeration.
    pub fn matches_reenumeration_candidate(
        &self,
        candidate_bus_id: &BusId,
        candidate_location_path: Option<&str>,
        candidate_serial_hash: Option<&str>,
        candidate_vid_pid: VidPid,
        candidate_mode: UsbDeviceMode,
    ) -> ReenumerationMatchConfidence {
        // 1. Exact serial hash match on the same topological bus/port
        if let (Some(prev_serial), Some(cand_serial)) =
            (&self.last_known_serial_hash, candidate_serial_hash)
        {
            if prev_serial == cand_serial && &self.bus_id == candidate_bus_id {
                return ReenumerationMatchConfidence::ExactSerialAndPort;
            }
            if prev_serial == cand_serial {
                return ReenumerationMatchConfidence::ExactSerialDifferentPort;
            }
        }

        // 2. Same physical USB port (BusId) and compatible location path
        if &self.bus_id == candidate_bus_id {
            let loc_matches = match (&self.location_path, candidate_location_path) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                _ => true,
            };

            if loc_matches {
                // Known mobile repair mode transitions on the exact same physical port:
                // - AndroidAdb / Mtp / Composite -> SamsungDownload / QualcommEdl / MediatekBrom / SpdUnisoc / Fastboot
                // - MediatekBrom -> MediatekPreloader
                // - SamsungDownload -> AndroidAdb / Mtp / Composite (reboot after Odin flash)
                // - Same mode re-enumeration (USB reset)
                if is_valid_repair_mode_transition(
                    self.current_mode,
                    candidate_mode,
                    self.current_vid_pid,
                    candidate_vid_pid,
                ) {
                    return ReenumerationMatchConfidence::TopologicalPortModeTransition;
                }
            }
        }

        ReenumerationMatchConfidence::NoMatch
    }
}

/// Confidence level when correlating a newly enumerated USB device with a reenumerating session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReenumerationMatchConfidence {
    /// Same SHA-256 serial hash and same physical `BusId`.
    ExactSerialAndPort,
    /// Same SHA-256 serial hash (user moved cable or hub port re-indexed).
    ExactSerialDifferentPort,
    /// Same physical `BusId` + PnP location path + valid repair mode transition (e.g. Android -> Download Mode / EDL / BROM).
    TopologicalPortModeTransition,
    /// Does not match the reenumerating device.
    NoMatch,
}

impl ReenumerationMatchConfidence {
    /// Returns `true` if the confidence is sufficient to automatically recover the session.
    pub const fn is_recoverable(self) -> bool {
        matches!(
            self,
            Self::ExactSerialAndPort
                | Self::ExactSerialDifferentPort
                | Self::TopologicalPortModeTransition
        )
    }
}

/// Verifies if transitioning from `from_mode` (`from_vid_pid`) to `to_mode` (`to_vid_pid`) on the same
/// physical USB port is a legitimate mobile servicing reenumeration.
pub fn is_valid_repair_mode_transition(
    from_mode: UsbDeviceMode,
    to_mode: UsbDeviceMode,
    from_vid_pid: VidPid,
    to_vid_pid: VidPid,
) -> bool {
    if from_mode == to_mode && from_vid_pid == to_vid_pid {
        return true;
    }

    use UsbDeviceMode::*;
    match (from_mode, to_mode) {
        // Normal Android (ADB/MTP) rebooting into any servicing/bootloader mode
        (
            AndroidAdb | Mtp | CompositeAdbMtp,
            SamsungDownload
            | QualcommEdl
            | MediatekBrom
            | MediatekPreloader
            | SpdUnisoc
            | Fastboot
            | AndroidAdb
            | Mtp
            | CompositeAdbMtp,
        ) => true,

        // Servicing/Bootloader mode rebooting back into normal Android after flashing or exit
        (
            SamsungDownload | QualcommEdl | MediatekBrom | MediatekPreloader | SpdUnisoc | Fastboot,
            AndroidAdb | Mtp | CompositeAdbMtp,
        ) => true,

        // MediaTek BROM <-> Preloader / DA transitions during SP Flash Tool / DA jump
        (MediatekBrom, MediatekPreloader) | (MediatekPreloader, MediatekBrom) => true,

        // Spreadtrum BootROM -> FDL1/FDL2 reenumeration
        (SpdUnisoc, SpdUnisoc) => true,

        // Samsung Download Mode re-enumeration
        (SamsungDownload, SamsungDownload) => true,

        // Fastboot <-> Fastbootd / EDL
        (Fastboot, Fastboot | QualcommEdl) => true,

        // Same vendor changing PID on the exact same port (e.g. toggling USB debugging)
        _ if from_vid_pid.vendor_id == to_vid_pid.vendor_id => true,

        _ => false,
    }
}

