//! # `adbhub-usbip-adapter`
//!
//! Implements the binary USB/IP v1.1.1 wire protocol (`OP_REQ_DEVLIST`, `OP_REP_DEVLIST`,
//! `OP_REQ_IMPORT`, `OP_REP_IMPORT`, `USBIP_CMD_SUBMIT`, `USBIP_RET_SUBMIT`,
//! `USBIP_CMD_UNLINK`, `USBIP_RET_UNLINK`) and the host-side `usbipd-win` controller.

pub mod controller;
pub mod protocol;

pub use controller::{
    connect_and_import_from_addr, HostUsbipBindController, UsbipdWinController,
};
pub use protocol::{
    encode_op_req_import, OpHeader, UsbIpDirection, UsbIpExportedDevice, UsbIpHeaderBasic,
    UsbIpUrbHeader, DEFAULT_USBIP_PORT, OP_REP_DEVLIST, OP_REP_IMPORT, OP_REQ_DEVLIST,
    OP_REQ_IMPORT, USBIP_CMD_SUBMIT, USBIP_CMD_UNLINK, USBIP_EXPORTED_DEVICE_SIZE,
    USBIP_RET_SUBMIT, USBIP_RET_UNLINK, USBIP_URB_HEADER_SIZE, USBIP_URB_ZERO_PACKET,
    USBIP_VERSION,
};

#[cfg(test)]
mod tests {
    use super::*;
    use adbhub_core::{BusId, CoreResult, UsbSpeed};
    use std::str::FromStr;

    #[test]
    fn test_exported_device_312_wire_roundtrip() -> CoreResult<()> {
        let dev = UsbIpExportedDevice {
            path: r"USB\VID_04E8&PID_685D\6&11223344&0&4".to_string(),
            bus_id: BusId::from_str("1-4")?,
            busnum: 1,
            devnum: 4,
            speed: UsbSpeed::High,
            id_vendor: 0x04E8,
            id_product: 0x685D,
            bcd_device: 0x0400,
            b_device_class: 0x02,
            b_device_subclass: 0x00,
            b_device_protocol: 0x00,
            b_configuration_value: 1,
            b_num_configurations: 1,
            b_num_interfaces: 2,
            interfaces: Vec::new(),
        };

        let wire = dev.to_wire_bytes_312();
        assert_eq!(wire.len(), 312);

        let decoded = UsbIpExportedDevice::from_wire_bytes_312(&wire)?;
        assert_eq!(decoded, dev);
        Ok(())
    }

    #[test]
    fn test_urb_cmd_submit_and_ret_submit_wire_roundtrip() -> CoreResult<()> {
        // Simulate Samsung Odin sending "ODIN" handshake on Bulk OUT EP 1 with ZLP flag
        let cmd = UsbIpUrbHeader::CmdSubmit {
            basic: UsbIpHeaderBasic {
                command: USBIP_CMD_SUBMIT,
                seqnum: 42,
                devid: 0x0001_0004,
                direction: UsbIpDirection::Out,
                ep: 1,
            },
            transfer_flags: USBIP_URB_ZERO_PACKET,
            transfer_buffer_length: 4,
            start_frame: 0,
            number_of_packets: 0,
            interval: 0,
            setup: [0; 8],
        };

        assert_eq!(cmd.following_payload_len(), 4);
        let wire = cmd.to_wire_bytes();
        let decoded = UsbIpUrbHeader::from_wire_bytes(&wire)?;
        assert_eq!(decoded, cmd);

        // Simulate Samsung device returning "LOKE" on Bulk IN EP 1
        let ret = UsbIpUrbHeader::RetSubmit {
            basic: UsbIpHeaderBasic {
                command: USBIP_RET_SUBMIT,
                seqnum: 43,
                devid: 0x0001_0004,
                direction: UsbIpDirection::In,
                ep: 1,
            },
            status: 0,
            actual_length: 4,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
        };

        assert_eq!(ret.following_payload_len(), 4);
        let ret_wire = ret.to_wire_bytes();
        let ret_decoded = UsbIpUrbHeader::from_wire_bytes(&ret_wire)?;
        assert_eq!(ret_decoded, ret);
        Ok(())
    }

    #[test]
    fn test_urb_unlink_roundtrip() -> CoreResult<()> {
        let unlink = UsbIpUrbHeader::CmdUnlink {
            basic: UsbIpHeaderBasic {
                command: USBIP_CMD_UNLINK,
                seqnum: 100,
                devid: 0x0001_0004,
                direction: UsbIpDirection::Out,
                ep: 1,
            },
            unlink_seqnum: 99,
        };

        assert_eq!(unlink.following_payload_len(), 0);
        let wire = unlink.to_wire_bytes();
        let decoded = UsbIpUrbHeader::from_wire_bytes(&wire)?;
        assert_eq!(decoded, unlink);
        Ok(())
    }
}

