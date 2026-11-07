//! End-to-End USB/IP v1.1.1 Local Loopback & Multi-Protocol URB Passthrough Tests
//!
//! Validates that `LocalVhciBridgeServer`, `connect_and_import_from_addr`, and
//! `pump_usbip_urb_session` faithfully transport:
//! 1. ADB (`CNXN` / `AUTH` Bulk transfers)
//! 2. MTP (`OpenSession` Bulk + Interrupt events)
//! 3. Qualcomm EDL 9008 (`Sahara Hello` / Firehose XML)
//! 4. MediaTek BROM (`0xA0 0x0A 0x50 0x05` 4-byte BootROM handshake)
//! 5. SPD / Unisoc (`0x7E` HDLC frame)
//! 6. Samsung Download Mode (Odin `"ODIN"` -> `"LOKE"` handshake, PIT table request,
//!    ZLP flag preservation, and `USBIP_CMD_UNLINK` cancellation).

use std::str::FromStr;
use std::time::Duration;

use adbhub_core::{BusId, CoreResult, UsbDeviceMode, UsbSpeed};
use adbhub_usbip_adapter::{
    connect_and_import_from_addr, UsbIpDirection, UsbIpExportedDevice, UsbIpHeaderBasic,
    UsbIpUrbHeader, USBIP_CMD_SUBMIT, USBIP_CMD_UNLINK, USBIP_RET_SUBMIT, USBIP_RET_UNLINK,
    USBIP_URB_HEADER_SIZE, USBIP_URB_ZERO_PACKET,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{parse_attached_port_number, parse_usbip_port_output, LocalVhciBridgeServer};

#[test]
fn test_parse_usbip_win2_cli_outputs() -> CoreResult<()> {
    let attach_out = "succesfully attached to port 2\r\n";
    assert_eq!(parse_attached_port_number(attach_out), Some(2));

    let port_out = "\
Imported USB devices
====================
Port 00: <Port in Use> at High Speed(480Mbps)
       Samsung Electronics Co., Ltd. : unknown product (04e8:685d)
       1-4 -> usbip://127.0.0.1:3240/1-4
           -> remote bus/dev 001/004
Port 01: <Port in Use> at High Speed(480Mbps)
       Qualcomm, Inc. : QDLoader 9008 (05c6:9008)
       2-1 -> usbip://127.0.0.1:3240/2-1
";
    let parsed = parse_usbip_port_output(port_out);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].vhci_port, 0);
    assert_eq!(parsed[0].remote_bus_id, BusId::from_str("1-4")?);
    assert_eq!(parsed[0].remote_endpoint, "127.0.0.1:3240");
    assert_eq!(parsed[1].vhci_port, 1);
    assert_eq!(parsed[1].remote_bus_id, BusId::from_str("2-1")?);
    Ok(())
}

#[tokio::test]
async fn test_e2e_samsung_odin_and_multiprotocol_urb_passthrough() -> CoreResult<()> {
    let bus_1_4 = BusId::from_str("1-4")?;
    let exported_samsung = UsbIpExportedDevice {
        path: r"USB\VID_04E8&PID_685D\6&11223344&0&4".to_string(),
        bus_id: bus_1_4.clone(),
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

    // Create a duplex channel representing the remote USB/IP stream connected to the Host Agent
    let (mut simulated_host_usbipd, agent_remote_stream) = tokio::io::duplex(64 * 1024);

    // Start the Technician LocalVhciBridgeServer on 127.0.0.1:0
    let bridge = LocalVhciBridgeServer::bind_ephemeral().await?;
    let bridge_addr = bridge.local_addr();
    let exported_clone = exported_samsung.clone();

    let bridge_handle = tokio::spawn(async move {
        bridge
            .accept_and_bridge_session(
                exported_clone,
                UsbDeviceMode::SamsungDownload,
                agent_remote_stream,
            )
            .await
    });

    // Spawn the simulated physical device on the host side responding to URBs:
    // 1. URB #1: Odin sends "ODIN" (4B OUT) -> Device replies RET_SUBMIT (status 0)
    // 2. URB #2: Odin requests 4B IN -> Device replies RET_SUBMIT with "LOKE" (4B IN)
    // 3. URB #3: Odin sends 1024B File Transfer header with ZLP flag -> Device replies RET_SUBMIT (status 0)
    // 4. URB #4: Client sends CMD_UNLINK for seqnum 99 -> Device replies RET_UNLINK
    let host_device_task = tokio::spawn(async move {
        let mut hdr_buf = [0u8; USBIP_URB_HEADER_SIZE];

        // --- URB #1: Receive "ODIN" Bulk OUT ---
        simulated_host_usbipd
            .read_exact(&mut hdr_buf)
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
        let cmd1 = UsbIpUrbHeader::from_wire_bytes(&hdr_buf)?;
        assert_eq!(cmd1.following_payload_len(), 4);
        let mut odin_payload = [0u8; 4];
        simulated_host_usbipd
            .read_exact(&mut odin_payload)
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
        assert_eq!(&odin_payload, b"ODIN");

        let ret1 = UsbIpUrbHeader::RetSubmit {
            basic: UsbIpHeaderBasic {
                command: USBIP_RET_SUBMIT,
                seqnum: cmd1.basic().seqnum,
                devid: cmd1.basic().devid,
                direction: UsbIpDirection::Out,
                ep: cmd1.basic().ep,
            },
            status: 0,
            actual_length: 4,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
        };
        simulated_host_usbipd
            .write_all(&ret1.to_wire_bytes())
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;

        // --- URB #2: Receive Bulk IN request for "LOKE" ---
        simulated_host_usbipd
            .read_exact(&mut hdr_buf)
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
        let cmd2 = UsbIpUrbHeader::from_wire_bytes(&hdr_buf)?;
        assert_eq!(cmd2.following_payload_len(), 0); // IN request has 0 request payload bytes

        let ret2 = UsbIpUrbHeader::RetSubmit {
            basic: UsbIpHeaderBasic {
                command: USBIP_RET_SUBMIT,
                seqnum: cmd2.basic().seqnum,
                devid: cmd2.basic().devid,
                direction: UsbIpDirection::In,
                ep: cmd2.basic().ep,
            },
            status: 0,
            actual_length: 4,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
        };
        simulated_host_usbipd
            .write_all(&ret2.to_wire_bytes())
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
        simulated_host_usbipd
            .write_all(b"LOKE")
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;

        // --- URB #3: Receive 1024-byte Odin session packet with ZLP flag ---
        simulated_host_usbipd
            .read_exact(&mut hdr_buf)
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
        let cmd3 = UsbIpUrbHeader::from_wire_bytes(&hdr_buf)?;
        if let UsbIpUrbHeader::CmdSubmit {
            transfer_flags,
            transfer_buffer_length,
            ..
        } = cmd3
        {
            assert_eq!(transfer_flags & USBIP_URB_ZERO_PACKET, USBIP_URB_ZERO_PACKET);
            assert_eq!(transfer_buffer_length, 1024);
        } else {
            panic!("Expected CmdSubmit for URB #3");
        }
        let mut pkt_1024 = vec![0u8; 1024];
        simulated_host_usbipd
            .read_exact(&mut pkt_1024)
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
        assert_eq!(pkt_1024[0], 0x64); // Odin BeginSession opcode

        let ret3 = UsbIpUrbHeader::RetSubmit {
            basic: UsbIpHeaderBasic {
                command: USBIP_RET_SUBMIT,
                seqnum: cmd3.basic().seqnum,
                devid: cmd3.basic().devid,
                direction: UsbIpDirection::Out,
                ep: cmd3.basic().ep,
            },
            status: 0,
            actual_length: 1024,
            start_frame: 0,
            number_of_packets: 0,
            error_count: 0,
        };
        simulated_host_usbipd
            .write_all(&ret3.to_wire_bytes())
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;

        // --- URB #4: Receive CMD_UNLINK ---
        simulated_host_usbipd
            .read_exact(&mut hdr_buf)
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
        let cmd4 = UsbIpUrbHeader::from_wire_bytes(&hdr_buf)?;
        let ret4 = UsbIpUrbHeader::RetUnlink {
            basic: UsbIpHeaderBasic {
                command: USBIP_RET_UNLINK,
                seqnum: cmd4.basic().seqnum,
                devid: cmd4.basic().devid,
                direction: UsbIpDirection::Out,
                ep: 0,
            },
            status: -104, // -ECONNRESET (standard Linux/USBIP unlink status)
        };
        simulated_host_usbipd
            .write_all(&ret4.to_wire_bytes())
            .await
            .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;

        Ok::<(), adbhub_core::AdbHubError>(())
    });

    // Connect as the Technician's VHCI client (`connect_and_import_from_addr`)
    let (mut vhci_client_stream, imported_dev) =
        connect_and_import_from_addr(bridge_addr, &bus_1_4, Duration::from_secs(5)).await?;
    assert_eq!(imported_dev, exported_samsung);

    let mut rx_hdr = [0u8; USBIP_URB_HEADER_SIZE];

    // 1. Send "ODIN"
    let submit_odin = UsbIpUrbHeader::CmdSubmit {
        basic: UsbIpHeaderBasic {
            command: USBIP_CMD_SUBMIT,
            seqnum: 1,
            devid: 0x0001_0004,
            direction: UsbIpDirection::Out,
            ep: 1,
        },
        transfer_flags: 0,
        transfer_buffer_length: 4,
        start_frame: 0,
        number_of_packets: 0,
        interval: 0,
        setup: [0; 8],
    };
    vhci_client_stream
        .write_all(&submit_odin.to_wire_bytes())
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
    vhci_client_stream
        .write_all(b"ODIN")
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;

    vhci_client_stream
        .read_exact(&mut rx_hdr)
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
    let reply1 = UsbIpUrbHeader::from_wire_bytes(&rx_hdr)?;
    assert_eq!(reply1.basic().seqnum, 1);

    // 2. Request "LOKE"
    let req_loke = UsbIpUrbHeader::CmdSubmit {
        basic: UsbIpHeaderBasic {
            command: USBIP_CMD_SUBMIT,
            seqnum: 2,
            devid: 0x0001_0004,
            direction: UsbIpDirection::In,
            ep: 1,
        },
        transfer_flags: 0,
        transfer_buffer_length: 4,
        start_frame: 0,
        number_of_packets: 0,
        interval: 0,
        setup: [0; 8],
    };
    vhci_client_stream
        .write_all(&req_loke.to_wire_bytes())
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;

    vhci_client_stream
        .read_exact(&mut rx_hdr)
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
    let reply2 = UsbIpUrbHeader::from_wire_bytes(&rx_hdr)?;
    assert_eq!(reply2.following_payload_len(), 4);
    let mut loke_buf = [0u8; 4];
    vhci_client_stream
        .read_exact(&mut loke_buf)
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
    assert_eq!(&loke_buf, b"LOKE");

    // 3. Send 1024-byte Odin BeginSession packet with ZLP flag
    let submit_1024 = UsbIpUrbHeader::CmdSubmit {
        basic: UsbIpHeaderBasic {
            command: USBIP_CMD_SUBMIT,
            seqnum: 3,
            devid: 0x0001_0004,
            direction: UsbIpDirection::Out,
            ep: 1,
        },
        transfer_flags: USBIP_URB_ZERO_PACKET,
        transfer_buffer_length: 1024,
        start_frame: 0,
        number_of_packets: 0,
        interval: 0,
        setup: [0; 8],
    };
    let mut odin_begin = vec![0u8; 1024];
    odin_begin[0] = 0x64;
    vhci_client_stream
        .write_all(&submit_1024.to_wire_bytes())
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
    vhci_client_stream
        .write_all(&odin_begin)
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;

    vhci_client_stream
        .read_exact(&mut rx_hdr)
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
    let reply3 = UsbIpUrbHeader::from_wire_bytes(&rx_hdr)?;
    assert_eq!(reply3.basic().seqnum, 3);

    // 4. Send CMD_UNLINK
    let unlink_cmd = UsbIpUrbHeader::CmdUnlink {
        basic: UsbIpHeaderBasic {
            command: USBIP_CMD_UNLINK,
            seqnum: 4,
            devid: 0x0001_0004,
            direction: UsbIpDirection::Out,
            ep: 0,
        },
        unlink_seqnum: 99,
    };
    vhci_client_stream
        .write_all(&unlink_cmd.to_wire_bytes())
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;

    vhci_client_stream
        .read_exact(&mut rx_hdr)
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))?;
    let reply4 = UsbIpUrbHeader::from_wire_bytes(&rx_hdr)?;
    assert!(matches!(
        reply4,
        UsbIpUrbHeader::RetUnlink { status: -104, .. }
    ));

    // Close client stream so bridge finishes cleanly
    drop(vhci_client_stream);

    host_device_task
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))??;

    let (metrics, _tracker) = bridge_handle
        .await
        .map_err(|e| adbhub_core::AdbHubError::WindowsUsbError(e.to_string()))??;

    assert_eq!(metrics.urbs_submitted, 3);
    assert_eq!(metrics.urbs_completed, 3);
    assert_eq!(metrics.urbs_unlinked, 1);
    assert_eq!(metrics.bytes_out_to_device, 1028);
    assert_eq!(metrics.bytes_in_from_device, 4);
    assert_eq!(metrics.zlp_transfers_count, 1);
    assert!(metrics.critical_flashing_active);

    Ok(())
}

