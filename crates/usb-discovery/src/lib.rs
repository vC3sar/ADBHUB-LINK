//! # `adbhub-usb-discovery`
//!
//! High-level USB discovery service that converts raw Windows SetupAPI / `usbipd-win`
//! records into fully classified [`DiscoveredUsbDevice`] objects with deterministic
//! logical identities across ADB, MTP, Qualcomm EDL, MediaTek BROM, SPD/Unisoc, and
//! Samsung Download Mode (Odin).

use std::sync::Arc;

use adbhub_core::{
    classify_usb_device, CoreResult, DeviceLifecycleState, DiscoveredUsbDevice,
    LogicalDeviceIdentity, UsbDeviceMode, UsbIpExportState,
};
use adbhub_usb_enumerator::{RawEnumeratedUsbDevice, UsbBusEnumerator, WindowsNativeUsbEnumerator};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

/// Namespace UUID v5 for deterministic logical device IDs per host topological slot.
const ADBHUB_PORT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30,
    0xc8,
]);

/// Filter configuration for [`UsbDiscoveryEngine`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryFilter {
    /// If true, only returns devices classified into one of the known mobile repair modes
    /// (ADB, MTP, Fastboot, Qualcomm EDL, MediaTek BROM/Preloader, SPD/Unisoc, Samsung Download).
    /// If false, returns all USB devices on the host (useful for diagnostics and unknown OEM PIDs).
    pub mobile_repair_devices_only: bool,
}

impl Default for DiscoveryFilter {
    fn default() -> Self {
        Self {
            mobile_repair_devices_only: false,
        }
    }
}

/// Orchestrates USB device discovery, descriptor classification, and logical identity generation.
pub struct UsbDiscoveryEngine {
    enumerator: Arc<dyn UsbBusEnumerator>,
    filter: DiscoveryFilter,
}

impl UsbDiscoveryEngine {
    /// Creates a discovery engine backed by the given [`UsbBusEnumerator`].
    pub fn new(enumerator: Arc<dyn UsbBusEnumerator>, filter: DiscoveryFilter) -> Self {
        Self { enumerator, filter }
    }

    /// Creates a production discovery engine using [`WindowsNativeUsbEnumerator`].
    pub fn with_windows_native(filter: DiscoveryFilter) -> Self {
        Self::new(Arc::new(WindowsNativeUsbEnumerator::new()), filter)
    }

    /// Scans the USB bus and produces classified [`DiscoveredUsbDevice`] snapshots.
    pub fn discover_all(&self) -> CoreResult<Vec<DiscoveredUsbDevice>> {
        let raw_list = self.enumerator.enumerate_devices()?;
        let mut discovered = Vec::with_capacity(raw_list.len());

        for raw in raw_list {
            let dev = build_discovered_device(raw);
            if self.filter.mobile_repair_devices_only
                && dev.primary_mode() == UsbDeviceMode::UnknownUsbDevice
            {
                continue;
            }
            debug!(
                bus_id = %dev.bus_id,
                vid_pid = %dev.vid_pid,
                mode = %dev.primary_mode(),
                product = %dev.product_name,
                "Discovered USB device"
            );
            discovered.push(dev);
        }

        Ok(discovered)
    }
}

/// Transforms a [`RawEnumeratedUsbDevice`] into a fully classified [`DiscoveredUsbDevice`].
pub fn build_discovered_device(raw: RawEnumeratedUsbDevice) -> DiscoveredUsbDevice {
    let now = Utc::now();

    let interfaces = if !raw.interfaces.is_empty() {
        raw.interfaces
    } else if let Some(cfg) = &raw.configuration_descriptor {
        cfg.interfaces.clone()
    } else {
        Vec::new()
    };

    let classification = classify_usb_device(
        raw.vid_pid,
        raw.device_descriptor.as_ref(),
        &interfaces,
        Some(&raw.product_name),
        raw.driver_service.as_deref(),
    );

    // Derive initial lifecycle state from export binding state
    let lifecycle_state = match &raw.export_state {
        UsbIpExportState::NotBound => DeviceLifecycleState::Available,
        UsbIpExportState::BoundReady { .. } => DeviceLifecycleState::Shared,
        UsbIpExportState::Attached { .. } => DeviceLifecycleState::Active,
    };

    // Deterministic logical ID anchored on topological port + location path so mode transitions
    // on the same port preserve a consistent slot identity.
    let slot_seed = format!(
        "port:{}:loc:{}",
        raw.bus_id,
        raw.location_path.as_deref().unwrap_or("default")
    );
    let logical_id = Uuid::new_v5(&ADBHUB_PORT_NAMESPACE, slot_seed.as_bytes());

    let identity = LogicalDeviceIdentity {
        logical_id,
        bus_id: raw.bus_id.clone(),
        location_path: raw.location_path.clone(),
        last_known_serial_hash: raw.serial_hash.clone(),
        origin_vid_pid: raw.vid_pid,
        current_vid_pid: raw.vid_pid,
        previous_mode: None,
        current_mode: classification.primary_mode,
        reenumeration_count: 0,
        updated_at: now,
    };

    let critical_operation_guard = matches!(raw.export_state, UsbIpExportState::Attached { .. })
        && classification.primary_mode.is_flashing_or_boot_mode();

    DiscoveredUsbDevice {
        identity,
        lifecycle_state,
        bus_id: raw.bus_id,
        vid_pid: raw.vid_pid,
        instance_id: raw.instance_id,
        manufacturer: raw.manufacturer,
        product_name: raw.product_name,
        serial_hash: raw.serial_hash,
        speed: raw.speed,
        device_descriptor: raw.device_descriptor,
        configuration_descriptor: raw.configuration_descriptor,
        interfaces,
        classification,
        export_state: raw.export_state,
        host_driver_service: raw.driver_service,
        critical_operation_guard,
        captured_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adbhub_core::{BusId, UsbSpeed, VidPid};
    use std::str::FromStr;
    use std::sync::Mutex;

    struct MockEnumerator {
        devices: Mutex<Vec<RawEnumeratedUsbDevice>>,
    }

    impl UsbBusEnumerator for MockEnumerator {
        fn enumerate_devices(&self) -> CoreResult<Vec<RawEnumeratedUsbDevice>> {
            let guard = self
                .devices
                .lock()
                .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
            Ok(guard.clone())
        }
    }

    #[test]
    fn test_discovery_engine_and_mobile_filter() -> CoreResult<()> {
        let raw_devices = vec![
            RawEnumeratedUsbDevice {
                bus_id: BusId::from_str("1-2")?,
                vid_pid: VidPid::new(0x046D, 0xC52B),
                instance_id: r"USB\VID_046D&PID_C52B\6&1111&0&2".to_string(),
                location_path: Some("PCIROOT(0)#USBROOT(0)#USB(2)".to_string()),
                manufacturer: "Logitech".to_string(),
                product_name: "USB Receiver".to_string(),
                serial_hash: None,
                speed: UsbSpeed::Full,
                device_descriptor: None,
                configuration_descriptor: None,
                interfaces: vec![],
                driver_service: Some("HidUsb".to_string()),
                export_state: UsbIpExportState::NotBound,
            },
            RawEnumeratedUsbDevice {
                bus_id: BusId::from_str("1-4")?,
                vid_pid: VidPid::new(0x04E8, 0x685D),
                instance_id: r"USB\VID_04E8&PID_685D\6&2222&0&4".to_string(),
                location_path: Some("PCIROOT(0)#USBROOT(0)#USB(4)".to_string()),
                manufacturer: "Samsung".to_string(),
                product_name: "SAMSUNG Mobile USB Modem".to_string(),
                serial_hash: None,
                speed: UsbSpeed::High,
                device_descriptor: None,
                configuration_descriptor: None,
                interfaces: vec![],
                driver_service: Some("VBoxUsb".to_string()),
                export_state: UsbIpExportState::Attached {
                    client_address: "127.0.0.1".to_string(),
                },
            },
        ];

        let enumerator = Arc::new(MockEnumerator {
            devices: Mutex::new(raw_devices),
        });

        let engine_mobile_only = UsbDiscoveryEngine::new(
            enumerator,
            DiscoveryFilter {
                mobile_repair_devices_only: true,
            },
        );

        let list = engine_mobile_only.discover_all()?;
        assert_eq!(list.len(), 1);
        let sam = &list[0];
        assert_eq!(sam.primary_mode(), UsbDeviceMode::SamsungDownload);
        assert_eq!(sam.lifecycle_state, DeviceLifecycleState::Active);
        assert!(sam.critical_operation_guard);
        Ok(())
    }
}

