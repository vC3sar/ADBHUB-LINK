//! Binary wire protocol codec for USB/IP v1.1.1 (`0x0111`).
//!
//! Preserves all USB transfer semantics required by Section 3 ("Regla Fundamental: USB Passthrough Real"):
//! - Control, Bulk, Interrupt, and Isochronous transfers
//! - 8-byte SETUP packets
//! - Endpoint addresses & directions (`USBIP_DIR_OUT = 0`, `USBIP_DIR_IN = 1`)
//! - `transfer_flags` (including `URB_SHORT_NOT_OK`, `URB_ZERO_PACKET` for Samsung Odin ZLPs)
//! - Sequence numbers (`seqnum`), cancellation (`USBIP_CMD_UNLINK`), and completion status.

use adbhub_core::{AdbHubError, BusId, CoreResult, InterfaceDescriptor, UsbSpeed, VidPid};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Standard USB/IP protocol version supported by `usbipd-win` and `usbip-win2` (`v1.1.1`).
pub const USBIP_VERSION: u16 = 0x0111;

/// Default local TCP port used by `usbipd-win` on the host machine.
pub const DEFAULT_USBIP_PORT: u16 = 3240;

// Management Phase Operation Codes (Big-Endian on the wire)
pub const OP_REQ_DEVLIST: u16 = 0x8005;
pub const OP_REP_DEVLIST: u16 = 0x0005;
pub const OP_REQ_IMPORT: u16 = 0x8003;
pub const OP_REP_IMPORT: u16 = 0x0003;

// URB Data Phase Command Codes (Big-Endian on the wire)
pub const USBIP_CMD_SUBMIT: u32 = 0x0000_0001;
pub const USBIP_CMD_UNLINK: u32 = 0x0000_0002;
pub const USBIP_RET_SUBMIT: u32 = 0x0000_0003;
pub const USBIP_RET_UNLINK: u32 = 0x0000_0004;

/// Size of every URB phase packet header (`usbip_header_basic` [20B] + union payload [20B] + padding [8B]).
pub const USBIP_URB_HEADER_SIZE: usize = 48;

/// Fixed size of the exported device record (`struct usbip_usb_device`) in `OP_REP_DEVLIST` and `OP_REP_IMPORT`.
pub const USBIP_EXPORTED_DEVICE_SIZE: usize = 312;

/// Transfer flag indicating a Zero-Length Packet (ZLP) must terminate bulk transfers that are an
/// exact multiple of `wMaxPacketSize` (critical for Samsung Odin / Loke protocol).
pub const USBIP_URB_ZERO_PACKET: u32 = 0x0000_0040;

/// Direction in `usbip_header_basic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsbIpDirection {
    /// Host-to-device (`0`).
    Out = 0,
    /// Device-to-host (`1`).
    In = 1,
}

impl UsbIpDirection {
    pub const fn from_u32(val: u32) -> Self {
        if val == 0 {
            Self::Out
        } else {
            Self::In
        }
    }
}

/// 8-byte common header for `OP_REQ_*` and `OP_REP_*` handshake messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpHeader {
    pub version: u16,
    pub command: u16,
    pub status: u32,
}

impl OpHeader {
    pub const fn new_request(command: u16) -> Self {
        Self {
            version: USBIP_VERSION,
            command,
            status: 0,
        }
    }

    pub const fn to_bytes(self) -> [u8; 8] {
        let v = self.version.to_be_bytes();
        let c = self.command.to_be_bytes();
        let s = self.status.to_be_bytes();
        [v[0], v[1], c[0], c[1], s[0], s[1], s[2], s[3]]
    }

    pub fn from_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() < 8 {
            return Err(AdbHubError::MalformedDescriptor {
                reason: "USB/IP OpHeader requires 8 bytes".to_string(),
                offset: 0,
                available: bytes.len(),
            });
        }
        Ok(Self {
            version: u16::from_be_bytes([bytes[0], bytes[1]]),
            command: u16::from_be_bytes([bytes[2], bytes[3]]),
            status: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        })
    }
}

/// 312-byte device metadata record (`struct usbip_usb_device`) returned during `OP_REP_IMPORT` and `OP_REP_DEVLIST`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbIpExportedDevice {
    /// Device sysfs/Windows path (up to 256 bytes null-padded).
    pub path: String,
    /// Topological Bus ID string (up to 32 bytes null-padded, e.g. `"1-4"`).
    pub bus_id: BusId,
    pub busnum: u32,
    pub devnum: u32,
    pub speed: UsbSpeed,
    pub id_vendor: u16,
    pub id_product: u16,
    pub bcd_device: u16,
    pub b_device_class: u8,
    pub b_device_subclass: u8,
    pub b_device_protocol: u8,
    pub b_configuration_value: u8,
    pub b_num_configurations: u8,
    pub b_num_interfaces: u8,
    pub interfaces: Vec<InterfaceDescriptor>,
}

impl UsbIpExportedDevice {
    pub const fn vid_pid(&self) -> VidPid {
        VidPid::new(self.id_vendor, self.id_product)
    }

    /// Serializes the 312-byte `usbip_usb_device` struct in Big-Endian wire format.
    pub fn to_wire_bytes_312(&self) -> [u8; USBIP_EXPORTED_DEVICE_SIZE] {
        let mut buf = [0u8; USBIP_EXPORTED_DEVICE_SIZE];

        let path_bytes = self.path.as_bytes();
        let path_len = path_bytes.len().min(255);
        buf[..path_len].copy_from_slice(&path_bytes[..path_len]);

        let busid_str = self.bus_id.to_string();
        let busid_bytes = busid_str.as_bytes();
        let busid_len = busid_bytes.len().min(31);
        buf[256..256 + busid_len].copy_from_slice(&busid_bytes[..busid_len]);

        buf[288..292].copy_from_slice(&self.busnum.to_be_bytes());
        buf[292..296].copy_from_slice(&self.devnum.to_be_bytes());
        buf[296..300].copy_from_slice(&self.speed.to_usbip_code().to_be_bytes());
        buf[300..302].copy_from_slice(&self.id_vendor.to_be_bytes());
        buf[302..304].copy_from_slice(&self.id_product.to_be_bytes());
        buf[304..306].copy_from_slice(&self.bcd_device.to_be_bytes());
        buf[306] = self.b_device_class;
        buf[307] = self.b_device_subclass;
        buf[308] = self.b_device_protocol;
        buf[309] = self.b_configuration_value;
        buf[310] = self.b_num_configurations;
        buf[311] = self.b_num_interfaces;

        buf
    }

    /// Parses the 312-byte `usbip_usb_device` struct from Big-Endian wire format.
    pub fn from_wire_bytes_312(buf: &[u8]) -> CoreResult<Self> {
        if buf.len() < USBIP_EXPORTED_DEVICE_SIZE {
            return Err(AdbHubError::MalformedDescriptor {
                reason: "usbip_usb_device requires 312 bytes".to_string(),
                offset: 0,
                available: buf.len(),
            });
        }

        let path_end = buf[..256].iter().position(|&b| b == 0).unwrap_or(256);
        let path = String::from_utf8_lossy(&buf[..path_end]).to_string();

        let busid_slice = &buf[256..288];
        let busid_end = busid_slice.iter().position(|&b| b == 0).unwrap_or(32);
        let busid_str = String::from_utf8_lossy(&busid_slice[..busid_end]);
        let bus_id = BusId::from_str(busid_str.trim())?;

        let busnum = u32::from_be_bytes([buf[288], buf[289], buf[290], buf[291]]);
        let devnum = u32::from_be_bytes([buf[292], buf[293], buf[294], buf[295]]);
        let speed_raw = u32::from_be_bytes([buf[296], buf[297], buf[298], buf[299]]);
        let speed = match speed_raw {
            1 => UsbSpeed::Low,
            2 => UsbSpeed::Full,
            3 => UsbSpeed::High,
            4 => UsbSpeed::Wireless,
            5 => UsbSpeed::Super,
            6 => UsbSpeed::SuperPlus,
            _ => UsbSpeed::Unknown,
        };

        let id_vendor = u16::from_be_bytes([buf[300], buf[301]]);
        let id_product = u16::from_be_bytes([buf[302], buf[303]]);
        let bcd_device = u16::from_be_bytes([buf[304], buf[305]]);

        Ok(Self {
            path,
            bus_id,
            busnum,
            devnum,
            speed,
            id_vendor,
            id_product,
            bcd_device,
            b_device_class: buf[306],
            b_device_subclass: buf[307],
            b_device_protocol: buf[308],
            b_configuration_value: buf[309],
            b_num_configurations: buf[310],
            b_num_interfaces: buf[311],
            interfaces: Vec::new(),
        })
    }
}

/// Builds the 40-byte `OP_REQ_IMPORT` packet (`8B OpHeader + 32B null-padded busid`).
pub fn encode_op_req_import(bus_id: &BusId) -> [u8; 40] {
    let mut pkt = [0u8; 40];
    let hdr = OpHeader::new_request(OP_REQ_IMPORT).to_bytes();
    pkt[..8].copy_from_slice(&hdr);

    let bid_str = bus_id.to_string();
    let bid_bytes = bid_str.as_bytes();
    let copy_len = bid_bytes.len().min(31);
    pkt[8..8 + copy_len].copy_from_slice(&bid_bytes[..copy_len]);
    pkt
}

/// Common 20-byte `usbip_header_basic` present at the start of every 48-byte URB command/return packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbIpHeaderBasic {
    pub command: u32,
    pub seqnum: u32,
    pub devid: u32,
    pub direction: UsbIpDirection,
    pub ep: u32,
}

impl UsbIpHeaderBasic {
    pub fn to_bytes(self) -> [u8; 20] {
        let mut b = [0u8; 20];
        b[0..4].copy_from_slice(&self.command.to_be_bytes());
        b[4..8].copy_from_slice(&self.seqnum.to_be_bytes());
        b[8..12].copy_from_slice(&self.devid.to_be_bytes());
        b[12..16].copy_from_slice(&(self.direction as u32).to_be_bytes());
        b[16..20].copy_from_slice(&self.ep.to_be_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> CoreResult<Self> {
        if b.len() < 20 {
            return Err(AdbHubError::MalformedDescriptor {
                reason: "usbip_header_basic requires 20 bytes".to_string(),
                offset: 0,
                available: b.len(),
            });
        }
        Ok(Self {
            command: u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            seqnum: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
            devid: u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
            direction: UsbIpDirection::from_u32(u32::from_be_bytes([b[12], b[13], b[14], b[15]])),
            ep: u32::from_be_bytes([b[16], b[17], b[18], b[19]]),
        })
    }
}

/// Parsed 48-byte USB/IP URB Header (`CMD_SUBMIT`, `RET_SUBMIT`, `CMD_UNLINK`, or `RET_UNLINK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsbIpUrbHeader {
    /// `USBIP_CMD_SUBMIT` (`0x00000001`): Client submits an URB to the remote physical USB device.
    CmdSubmit {
        basic: UsbIpHeaderBasic,
        transfer_flags: u32,
        transfer_buffer_length: u32,
        start_frame: u32,
        number_of_packets: u32,
        interval: u32,
        setup: [u8; 8],
    },
    /// `USBIP_RET_SUBMIT` (`0x00000003`): Host returns completed URB status and IN data to the client.
    RetSubmit {
        basic: UsbIpHeaderBasic,
        status: i32,
        actual_length: u32,
        start_frame: u32,
        number_of_packets: u32,
        error_count: u32,
    },
    /// `USBIP_CMD_UNLINK` (`0x00000002`): Client cancels an in-flight URB identified by `unlink_seqnum`.
    CmdUnlink {
        basic: UsbIpHeaderBasic,
        unlink_seqnum: u32,
    },
    /// `USBIP_RET_UNLINK` (`0x00000004`): Host confirms cancellation of an URB.
    RetUnlink {
        basic: UsbIpHeaderBasic,
        status: i32,
    },
}

impl UsbIpUrbHeader {
    /// Returns the basic 20-byte header.
    pub const fn basic(&self) -> UsbIpHeaderBasic {
        match *self {
            Self::CmdSubmit { basic, .. }
            | Self::RetSubmit { basic, .. }
            | Self::CmdUnlink { basic, .. }
            | Self::RetUnlink { basic, .. } => basic,
        }
    }

    /// Returns the exact number of payload bytes that immediately follow this 48-byte header
    /// on the stream (for non-isochronous transfers).
    pub const fn following_payload_len(&self) -> usize {
        match *self {
            Self::CmdSubmit {
                basic,
                transfer_buffer_length,
                ..
            } => {
                // Only OUT transfers carry data from Client -> Host in CMD_SUBMIT
                match basic.direction {
                    UsbIpDirection::Out => transfer_buffer_length as usize,
                    UsbIpDirection::In => 0,
                }
            }
            Self::RetSubmit {
                basic,
                actual_length,
                ..
            } => {
                // Only IN transfers carry data from Host -> Client in RET_SUBMIT
                match basic.direction {
                    UsbIpDirection::In => actual_length as usize,
                    UsbIpDirection::Out => 0,
                }
            }
            Self::CmdUnlink { .. } | Self::RetUnlink { .. } => 0,
        }
    }

    /// Encodes this URB header into its canonical 48-byte Big-Endian wire representation.
    pub fn to_wire_bytes(&self) -> [u8; USBIP_URB_HEADER_SIZE] {
        let mut buf = [0u8; USBIP_URB_HEADER_SIZE];
        let basic_bytes = self.basic().to_bytes();
        buf[0..20].copy_from_slice(&basic_bytes);

        match *self {
            Self::CmdSubmit {
                transfer_flags,
                transfer_buffer_length,
                start_frame,
                number_of_packets,
                interval,
                setup,
                ..
            } => {
                buf[20..24].copy_from_slice(&transfer_flags.to_be_bytes());
                buf[24..28].copy_from_slice(&transfer_buffer_length.to_be_bytes());
                buf[28..32].copy_from_slice(&start_frame.to_be_bytes());
                buf[32..36].copy_from_slice(&number_of_packets.to_be_bytes());
                buf[36..40].copy_from_slice(&interval.to_be_bytes());
                buf[40..48].copy_from_slice(&setup);
            }
            Self::RetSubmit {
                status,
                actual_length,
                start_frame,
                number_of_packets,
                error_count,
                ..
            } => {
                buf[20..24].copy_from_slice(&status.to_be_bytes());
                buf[24..28].copy_from_slice(&actual_length.to_be_bytes());
                buf[28..32].copy_from_slice(&start_frame.to_be_bytes());
                buf[32..36].copy_from_slice(&number_of_packets.to_be_bytes());
                buf[36..40].copy_from_slice(&error_count.to_be_bytes());
            }
            Self::CmdUnlink { unlink_seqnum, .. } => {
                buf[20..24].copy_from_slice(&unlink_seqnum.to_be_bytes());
            }
            Self::RetUnlink { status, .. } => {
                buf[20..24].copy_from_slice(&status.to_be_bytes());
            }
        }

        buf
    }

    /// Decodes a 48-byte Big-Endian wire buffer into a typed [`UsbIpUrbHeader`].
    pub fn from_wire_bytes(buf: &[u8]) -> CoreResult<Self> {
        if buf.len() < USBIP_URB_HEADER_SIZE {
            return Err(AdbHubError::MalformedDescriptor {
                reason: "USB/IP URB header requires 48 bytes".to_string(),
                offset: 0,
                available: buf.len(),
            });
        }

        let basic = UsbIpHeaderBasic::from_bytes(&buf[0..20])?;

        match basic.command {
            USBIP_CMD_SUBMIT => {
                let mut setup = [0u8; 8];
                setup.copy_from_slice(&buf[40..48]);
                Ok(Self::CmdSubmit {
                    basic,
                    transfer_flags: u32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]),
                    transfer_buffer_length: u32::from_be_bytes([
                        buf[24], buf[25], buf[26], buf[27],
                    ]),
                    start_frame: u32::from_be_bytes([buf[28], buf[29], buf[30], buf[31]]),
                    number_of_packets: u32::from_be_bytes([buf[32], buf[33], buf[34], buf[35]]),
                    interval: u32::from_be_bytes([buf[36], buf[37], buf[38], buf[39]]),
                    setup,
                })
            }
            USBIP_RET_SUBMIT => Ok(Self::RetSubmit {
                basic,
                status: i32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]),
                actual_length: u32::from_be_bytes([buf[24], buf[25], buf[26], buf[27]]),
                start_frame: u32::from_be_bytes([buf[28], buf[29], buf[30], buf[31]]),
                number_of_packets: u32::from_be_bytes([buf[32], buf[33], buf[34], buf[35]]),
                error_count: u32::from_be_bytes([buf[36], buf[37], buf[38], buf[39]]),
            }),
            USBIP_CMD_UNLINK => Ok(Self::CmdUnlink {
                basic,
                unlink_seqnum: u32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]),
            }),
            USBIP_RET_UNLINK => Ok(Self::RetUnlink {
                basic,
                status: i32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]),
            }),
            other => Err(AdbHubError::MalformedDescriptor {
                reason: format!("Unknown USB/IP URB command code: 0x{other:08X}"),
                offset: 0,
                available: buf.len(),
            }),
        }
    }
}

