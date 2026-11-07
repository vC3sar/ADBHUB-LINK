//! Complete discovered USB device snapshot combining descriptors, topology, classification, and state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::lifecycle::{DeviceLifecycleState, LogicalDeviceIdentity};
use crate::protocol::{ProtocolClassification, UsbDeviceMode};
use crate::usb::{
    BusId, ConfigurationDescriptor, InterfaceDescriptor, UsbDeviceDescriptor, UsbSpeed, VidPid,
};

/// Export status of the device in the local `usbipd-win` subsystem on the host PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsbIpExportState {
    /// Device is not bound to `usbipd-win` (controlled by standard Windows host drivers).
    NotBound,
    /// Device is bound (`Persisted` / `Shared` in `usbipd-win`) and ready for TCP/QUIC attachment.
    BoundReady { persisted_guid: Option<Uuid> },
    /// Device is actively attached to a USB/IP client socket.
    Attached { client_address: String },
}

/// Complete representation of a physical USB device discovered on the Windows Agent host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredUsbDevice {
    /// Logical identity tracking this device across mode changes and reenumerations.
    pub identity: LogicalDeviceIdentity,
    /// Current lifecycle state (`DISCOVERED`, `AVAILABLE`, `SHARED`, `ACTIVE`, `REENUMERATING`, etc.).
    pub lifecycle_state: DeviceLifecycleState,
    /// Topological USB/IP Bus ID (`<bus>-<port>`).
    pub bus_id: BusId,
    /// Vendor ID and Product ID (`VID:PID`).
    pub vid_pid: VidPid,
    /// Windows PnP Instance ID (e.g. `USB\VID_04E8&PID_685D\...`).
    pub instance_id: String,
    /// Manufacturer string from USB descriptor or PnP registry.
    pub manufacturer: String,
    /// Product name from USB descriptor or PnP friendly name.
    pub product_name: String,
    /// Privacy-preserving SHA-256 hash of the device serial number, if available.
    pub serial_hash: Option<String>,
    /// Negotiated USB speed.
    pub speed: UsbSpeed,
    /// Parsed 18-byte USB Device Descriptor, when directly queried from hub.
    pub device_descriptor: Option<UsbDeviceDescriptor>,
    /// Active USB Configuration Descriptor, when queried from hub.
    pub configuration_descriptor: Option<ConfigurationDescriptor>,
    /// Flattened list of interfaces on the active configuration.
    pub interfaces: Vec<InterfaceDescriptor>,
    /// Multi-protocol classification result (ADB, MTP, EDL, BROM, SPD, Samsung Download Mode).
    pub classification: ProtocolClassification,
    /// Current export binding status in `usbipd-win`.
    pub export_state: UsbIpExportState,
    /// Active Windows service/driver name on the host (`winusb`, `VBoxUsb`, `ssudmdm`, `usbccgp`, etc.).
    pub host_driver_service: Option<String>,
    /// Whether an active critical flashing transfer guard is protecting this device from accidental disconnect.
    pub critical_operation_guard: bool,
    /// Timestamp when this snapshot was captured.
    pub captured_at: DateTime<Utc>,
}

impl DiscoveredUsbDevice {
    /// Returns the primary [`UsbDeviceMode`] of this device.
    pub fn primary_mode(&self) -> UsbDeviceMode {
        self.classification.primary_mode
    }
}

