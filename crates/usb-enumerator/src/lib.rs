//! # `adbhub-usb-enumerator`
//!
//! Native Windows USB device enumeration using `SetupAPI` (`SetupDiGetClassDevsW`),
//! PnP topological location path extraction (`SPDRP_LOCATION_PATHS`), hardware serial
//! hashing, and `usbipd-win` state JSON synchronization.

pub mod usbipd;
pub mod win32;

pub use usbipd::{
    parse_usbipd_state_json, query_usbipd_state, UsbipdDeviceEntry, UsbipdStateOutput,
};
pub use win32::{
    extract_serial_hash_from_instance_id, parse_bus_id_from_windows_location,
    RawEnumeratedUsbDevice, UsbBusEnumerator, WindowsNativeUsbEnumerator,
};

#[cfg(test)]
mod tests {
    use super::*;
    use adbhub_core::{BusId, CoreResult, UsbIpExportState, VidPid};
    use std::str::FromStr;

    #[test]
    fn test_parse_bus_id_from_windows_pnp_location() -> CoreResult<()> {
        let loc_path = Some("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(4)#USB(2)");
        let bid = parse_bus_id_from_windows_location(None, loc_path, 99);
        assert_eq!(bid, BusId::from_str("1-4.2")?);

        let loc_info = Some("Port_#0005.Hub_#0002");
        let bid2 = parse_bus_id_from_windows_location(loc_info, None, 99);
        assert_eq!(bid2, BusId::from_str("2-5")?);
        Ok(())
    }

    #[test]
    fn test_extract_serial_hash_vs_windows_generated_id() {
        // Real serial number in instance ID (no '&' in third segment)
        let real_inst = r"USB\VID_04E8&PID_6860\R58M456789Z";
        let hash = extract_serial_hash_from_instance_id(real_inst);
        assert!(hash.is_some());

        // Windows-generated port instance ID when USB device has iSerialNumber == 0 (e.g. MTK BROM or EDL)
        let generated_inst = r"USB\VID_0E8D&PID_0003\6&2A3B4C5D&0&4";
        let no_hash = extract_serial_hash_from_instance_id(generated_inst);
        assert!(no_hash.is_none());
    }

    #[test]
    fn test_parse_usbipd_state_json_output() -> CoreResult<()> {
        let sample_json = r#"{
            "Devices": [
                {
                    "BusId": "1-4",
                    "ClientIPAddress": null,
                    "Description": "SAMSUNG Mobile USB Modem",
                    "InstanceId": "USB\\VID_04E8&PID_685D\\6&11223344&0&4",
                    "IsForced": false,
                    "PersistedGuid": "d4a1b2c3-e4f5-4678-9abc-def012345678",
                    "StubInstanceId": "USB\\VID_80EE&PID_CAFE\\6&11223344&0&4"
                },
                {
                    "BusId": "2-1",
                    "ClientIPAddress": "127.0.0.1",
                    "Description": "Qualcomm HS-USB QDLoader 9008",
                    "InstanceId": "USB\\VID_05C6&PID_9008\\7&55667788&0&1",
                    "IsForced": true,
                    "PersistedGuid": "a1b2c3d4-e5f6-4789-8abc-def012345678",
                    "StubInstanceId": null
                }
            ]
        }"#;

        let parsed = parse_usbipd_state_json(sample_json)
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
        assert_eq!(parsed.devices.len(), 2);

        let sam = &parsed.devices[0];
        assert_eq!(sam.parsed_bus_id(), Some(BusId::from_str("1-4")?));
        assert_eq!(sam.parsed_vid_pid(), Some(VidPid::new(0x04E8, 0x685D)));
        assert!(matches!(
            sam.export_state(),
            UsbIpExportState::BoundReady { .. }
        ));

        let qcom = &parsed.devices[1];
        assert_eq!(
            qcom.export_state(),
            UsbIpExportState::Attached {
                client_address: "127.0.0.1".to_string()
            }
        );

        Ok(())
    }

    #[test]
    fn test_live_windows_native_enumerator_runs_cleanly() -> CoreResult<()> {
        let enumerator = WindowsNativeUsbEnumerator::new();
        let devices = enumerator.enumerate_devices()?;
        // Verify that every enumerated device on the host has a valid non-zero BusId and VID:PID
        for dev in devices {
            assert!(dev.bus_id.bus >= 1);
            assert!(!dev.bus_id.port_chain.is_empty());
            assert!(!dev.instance_id.is_empty());
        }
        Ok(())
    }
}

