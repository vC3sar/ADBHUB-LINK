//! # `adbhub-core`
//!
//! Core domain types, standard USB 2.0/3.0 binary descriptor parsers, multi-protocol
//! mobile repair mode classifier (ADB, MTP, Qualcomm EDL, MediaTek BROM, SPD/Unisoc,
//! Samsung Download Mode / Odin), and Device Lifecycle State Machine definitions.

pub mod adb;
pub mod device;
pub mod error;
pub mod lifecycle;
pub mod protocol;
pub mod usb;

pub use adb::{AdbDeviceInfo, AdbDeviceState, AdbEngine, AnyDeskCode, RemoteAdbMessage};
pub use device::{DiscoveredUsbDevice, UsbIpExportState};
pub use error::{AdbHubError, CoreResult};
pub use lifecycle::{
    is_valid_repair_mode_transition, DeviceLifecycleState, LogicalDeviceIdentity,
    ReenumerationMatchConfidence,
};
pub use protocol::{
    classify_usb_device, vendor_name_from_id, vendors, OdinCompatibilityStatus,
    ProtocolClassification, SamsungDownloadMetadata, UsbDeviceMode,
};
pub use usb::{
    hash_serial_number, BusId, ConfigurationDescriptor, EndpointDescriptor, EndpointDirection,
    EndpointTransferType, InterfaceDescriptor, UsbDeviceDescriptor, UsbSpeed, VidPid,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_bus_id_parsing_and_display() -> CoreResult<()> {
        let simple = BusId::from_str("1-4")?;
        assert_eq!(simple.bus, 1);
        assert_eq!(simple.port_chain, vec![4]);
        assert_eq!(simple.to_string(), "1-4");

        let nested = BusId::from_str("2-3.1.4")?;
        assert_eq!(nested.bus, 2);
        assert_eq!(nested.port_chain, vec![3, 1, 4]);
        assert_eq!(nested.root_port(), 3);
        assert_eq!(nested.to_string(), "2-3.1.4");

        assert!(BusId::from_str("0-1").is_err());
        assert!(BusId::from_str("1-0").is_err());
        assert!(BusId::from_str("invalid").is_err());
        Ok(())
    }

    #[test]
    fn test_vid_pid_parsing_and_hardware_id_extraction() -> CoreResult<()> {
        let vp = VidPid::from_str("04E8:685D")?;
        assert_eq!(vp.vendor_id, 0x04E8);
        assert_eq!(vp.product_id, 0x685D);
        assert_eq!(vp.to_string(), "04E8:685D");

        let from_hw =
            VidPid::from_windows_hardware_id(r"USB\VID_05C6&PID_9008&REV_0000").ok_or_else(
                || AdbHubError::InvalidVidPid("Failed to extract from hardware id".to_string()),
            )?;
        assert_eq!(from_hw, VidPid::new(0x05C6, 0x9008));
        Ok(())
    }

    #[test]
    fn test_parse_binary_device_and_configuration_descriptors() -> CoreResult<()> {
        // Real 18-byte USB Device Descriptor for a Samsung Galaxy device in Download Mode (04E8:685D)
        let dev_bytes: [u8; 18] = [
            0x12, // bLength = 18
            0x01, // bDescriptorType = DEVICE (0x01)
            0x00, 0x02, // bcdUSB = 2.00
            0x02, // bDeviceClass = 0x02 (Communications / CDC)
            0x00, // bDeviceSubClass
            0x00, // bDeviceProtocol
            0x40, // bMaxPacketSize0 = 64
            0xE8, 0x04, // idVendor = 0x04E8 (Samsung)
            0x5D, 0x68, // idProduct = 0x685D (Download Mode)
            0x00, 0x04, // bcdDevice = 4.00
            0x01, // iManufacturer
            0x02, // iProduct
            0x00, // iSerialNumber
            0x01, // bNumConfigurations = 1
        ];

        let dev_desc = UsbDeviceDescriptor::parse_bytes(&dev_bytes)?;
        assert_eq!(dev_desc.vendor_id, 0x04E8);
        assert_eq!(dev_desc.product_id, 0x685D);
        assert_eq!(dev_desc.device_class, 0x02);
        assert_eq!(dev_desc.max_packet_size_ep0, 64);

        // Configuration descriptor with Interface 0 (CDC Control + Interrupt IN) and Interface 1 (CDC Data + Bulk IN/OUT)
        let cfg_bytes: Vec<u8> = vec![
            // Configuration Descriptor (9 bytes)
            0x09, 0x02, 0x43, 0x00, // wTotalLength = 67 bytes (0x0043)
            0x02, // bNumInterfaces = 2
            0x01, // bConfigurationValue = 1
            0x00, // iConfiguration
            0xC0, // bmAttributes (Self-powered)
            0xFA, // bMaxPower (500mA)
            // Interface 0: CDC ACM Control (9 bytes)
            0x09, 0x04, 0x00, 0x00, 0x01, 0x02, 0x02, 0x01, 0x00,
            // CDC Functional Descriptors (skipped gracefully: 5 + 5 + 4 + 5 = 19 bytes)
            0x05, 0x24, 0x00, 0x10, 0x01, 0x05, 0x24, 0x01, 0x00, 0x01, 0x04, 0x24, 0x02, 0x02,
            0x05, 0x24, 0x06, 0x00, 0x01, // Endpoint 0x82 Interrupt IN (7 bytes)
            0x07, 0x05, 0x82, 0x03, 0x10, 0x00, 0x09,
            // Interface 1: CDC Data (9 bytes)
            0x09, 0x04, 0x01, 0x00, 0x02, 0x0A, 0x00, 0x00, 0x00,
            // Endpoint 0x81 Bulk IN, 512 bytes (7 bytes)
            0x07, 0x05, 0x81, 0x02, 0x00, 0x02, 0x00,
            // Endpoint 0x01 Bulk OUT, 512 bytes (7 bytes)
            0x07, 0x05, 0x01, 0x02, 0x00, 0x02, 0x00,
        ];

        let cfg = ConfigurationDescriptor::parse_bytes(&cfg_bytes)?;
        assert_eq!(cfg.num_interfaces, 2);
        assert_eq!(cfg.interfaces.len(), 2);
        assert_eq!(cfg.interfaces[0].class_code, 0x02);
        assert_eq!(cfg.interfaces[0].endpoints.len(), 1);
        assert_eq!(
            cfg.interfaces[0].endpoints[0].transfer_type,
            EndpointTransferType::Interrupt
        );
        assert_eq!(cfg.interfaces[1].class_code, 0x0A);
        assert_eq!(cfg.interfaces[1].endpoints.len(), 2);
        assert_eq!(cfg.interfaces[1].endpoints[0].address, 0x81);
        assert_eq!(cfg.interfaces[1].endpoints[0].max_packet_size, 512);
        assert_eq!(cfg.interfaces[1].endpoints[1].address, 0x01);
        assert_eq!(
            cfg.interfaces[1].endpoints[1].direction,
            EndpointDirection::Out
        );

        // Classify this parsed Samsung device
        let classification = classify_usb_device(
            VidPid::new(dev_desc.vendor_id, dev_desc.product_id),
            Some(&dev_desc),
            &cfg.interfaces,
            Some("SAMSUNG Mobile USB Modem"),
            Some("ssudmdm"),
        );

        assert_eq!(classification.primary_mode, UsbDeviceMode::SamsungDownload);
        let sam = classification.samsung_download_info.ok_or_else(|| {
            AdbHubError::WindowsUsbError("Missing Samsung metadata".to_string())
        })?;
        assert!(sam.has_odin_cdc_interfaces);
        assert_eq!(sam.bulk_in_endpoint, Some(0x81));
        assert_eq!(sam.bulk_out_endpoint, Some(0x01));
        assert_eq!(sam.odin_compatibility, OdinCompatibilityStatus::Unverified);

        Ok(())
    }

    #[test]
    fn test_classify_all_six_mandatory_protocols() {
        // 1. ADB + MTP Composite
        let adb_mtp_ifaces = vec![
            InterfaceDescriptor {
                interface_number: 0,
                alternate_setting: 0,
                num_endpoints: 3,
                class_code: 0x06,
                subclass_code: 0x01,
                protocol_code: 0x01,
                interface_name: Some("MTP".to_string()),
                endpoints: vec![],
            },
            InterfaceDescriptor {
                interface_number: 1,
                alternate_setting: 0,
                num_endpoints: 2,
                class_code: 0xFF,
                subclass_code: 0x42,
                protocol_code: 0x01,
                interface_name: Some("ADB Interface".to_string()),
                endpoints: vec![],
            },
        ];
        let c_adb = classify_usb_device(
            VidPid::new(vendors::SAMSUNG, 0x6860),
            None,
            &adb_mtp_ifaces,
            Some("Galaxy S23"),
            Some("usbccgp"),
        );
        assert_eq!(c_adb.primary_mode, UsbDeviceMode::CompositeAdbMtp);
        assert!(c_adb.capabilities.contains(&UsbDeviceMode::AndroidAdb));
        assert!(c_adb.capabilities.contains(&UsbDeviceMode::Mtp));

        // 2. Qualcomm EDL 9008
        let c_edl = classify_usb_device(
            VidPid::new(vendors::QUALCOMM, 0x9008),
            None,
            &[],
            Some("Qualcomm HS-USB QDLoader 9008 (COM10)"),
            Some("qcser"),
        );
        assert_eq!(c_edl.primary_mode, UsbDeviceMode::QualcommEdl);

        // 3. MediaTek BROM
        let c_mtk = classify_usb_device(
            VidPid::new(vendors::MEDIATEK, 0x0003),
            None,
            &[],
            Some("MTK USB Port"),
            Some("usb2ser"),
        );
        assert_eq!(c_mtk.primary_mode, UsbDeviceMode::MediatekBrom);
        assert!(c_mtk.primary_mode.requires_fast_prebind());

        // 4. SPD / Unisoc
        let c_spd = classify_usb_device(
            VidPid::new(vendors::SPREADTRUM_UNISOC, 0x4D00),
            None,
            &[],
            Some("SPRD U2S Diag"),
            Some("SciU2S"),
        );
        assert_eq!(c_spd.primary_mode, UsbDeviceMode::SpdUnisoc);
    }

    #[test]
    fn test_lifecycle_transitions_and_reenumeration_matching() -> CoreResult<()> {
        let mut state = DeviceLifecycleState::Discovered;
        state = state.transition_to(DeviceLifecycleState::Available, "1-4")?;
        state = state.transition_to(DeviceLifecycleState::Shared, "1-4")?;
        state = state.transition_to(DeviceLifecycleState::Reserved, "1-4")?;
        state = state.transition_to(DeviceLifecycleState::Connecting, "1-4")?;
        state = state.transition_to(DeviceLifecycleState::Active, "1-4")?;
        state = state.transition_to(DeviceLifecycleState::Reenumerating, "1-4")?;
        state = state.transition_to(DeviceLifecycleState::Recovering, "1-4")?;
        state = state.transition_to(DeviceLifecycleState::Active, "1-4")?;
        assert_eq!(state, DeviceLifecycleState::Active);

        // Illegal transition: Discovered -> Active directly must fail
        assert!(DeviceLifecycleState::Discovered
            .transition_to(DeviceLifecycleState::Active, "1-4")
            .is_err());

        // Logical identity matching when a Samsung device transitions from ADB (04E8:6860) to Download Mode (04E8:685D)
        // on the same physical port 1-4 (where Download Mode often drops the USB serial descriptor)
        let bus_1_4 = BusId::from_str("1-4")?;
        let identity = LogicalDeviceIdentity {
            logical_id: uuid::Uuid::new_v4(),
            bus_id: bus_1_4.clone(),
            location_path: Some("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(4)".to_string()),
            last_known_serial_hash: Some(hash_serial_number("R58M123456A")),
            origin_vid_pid: VidPid::new(0x04E8, 0x6860),
            current_vid_pid: VidPid::new(0x04E8, 0x6860),
            previous_mode: None,
            current_mode: UsbDeviceMode::CompositeAdbMtp,
            reenumeration_count: 0,
            updated_at: chrono::Utc::now(),
        };

        let confidence = identity.matches_reenumeration_candidate(
            &bus_1_4,
            Some("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(4)"),
            None, // Download mode frequently omits serial string
            VidPid::new(0x04E8, 0x685D),
            UsbDeviceMode::SamsungDownload,
        );
        assert_eq!(
            confidence,
            ReenumerationMatchConfidence::TopologicalPortModeTransition
        );
        assert!(confidence.is_recoverable());

        Ok(())
    }

    #[test]
    fn test_anydesk_code_generation_and_flexible_parsing() -> CoreResult<()> {
        let alpha = AnyDeskCode::generate_alphanumeric();
        assert_eq!(alpha.as_str().len(), 19); // 16 chars + 3 hyphens

        let num = AnyDeskCode::generate_numeric();
        assert_eq!(num.as_str().len(), 11); // 9 digits + 2 hyphens

        // Test flexible parsing with lowercase and spaces
        let parsed = AnyDeskCode::parse_flexible("a55a ddd5 cch6 rrty")?;
        assert_eq!(parsed.as_str(), "A55A-DDD5-CCH6-RRTY");

        let parsed_num = AnyDeskCode::parse_flexible("948 213 754")?;
        assert_eq!(parsed_num.as_str(), "948-213-754");

        assert!(AnyDeskCode::parse_flexible("invalid").is_err());
        Ok(())
    }
}
