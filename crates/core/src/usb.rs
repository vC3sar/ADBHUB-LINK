//! Standard USB 2.0 / 3.0 descriptors, endpoints, interfaces, and topological identifiers.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AdbHubError, CoreResult};

/// Topological USB Bus ID compatible with `usbipd-win` and the Linux USB/IP protocol
/// (for example: `"1-4"` or `"2-3.1"`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BusId {
    /// Root hub / controller bus number.
    pub bus: u16,
    /// Port chain from the root hub (e.g. `[4]` for `1-4`, or `[3, 1]` for `2-3.1`).
    pub port_chain: Vec<u16>,
}

impl BusId {
    /// Creates a new [`BusId`] from a bus number and non-empty port chain.
    pub fn new(bus: u16, port_chain: Vec<u16>) -> CoreResult<Self> {
        if bus == 0 || port_chain.is_empty() || port_chain.contains(&0) {
            return Err(AdbHubError::InvalidBusId(format!("{bus}-{port_chain:?}")));
        }
        Ok(Self { bus, port_chain })
    }

    /// Returns the root port number on the controller.
    pub fn root_port(&self) -> u16 {
        self.port_chain.first().copied().unwrap_or(1)
    }
}

impl fmt::Display for BusId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ports: Vec<String> = self.port_chain.iter().map(u16::to_string).collect();
        write!(f, "{}-{}", self.bus, ports.join("."))
    }
}

impl FromStr for BusId {
    type Err = AdbHubError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let (bus_str, ports_str) = trimmed
            .split_once('-')
            .ok_or_else(|| AdbHubError::InvalidBusId(s.to_string()))?;

        let bus: u16 = bus_str
            .parse()
            .map_err(|_| AdbHubError::InvalidBusId(s.to_string()))?;

        let mut port_chain = Vec::new();
        for p in ports_str.split('.') {
            let port_num: u16 = p
                .parse()
                .map_err(|_| AdbHubError::InvalidBusId(s.to_string()))?;
            if port_num == 0 {
                return Err(AdbHubError::InvalidBusId(s.to_string()));
            }
            port_chain.push(port_num);
        }

        Self::new(bus, port_chain)
    }
}

/// Pair of USB Vendor ID (`idVendor`) and Product ID (`idProduct`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VidPid {
    pub vendor_id: u16,
    pub product_id: u16,
}

impl VidPid {
    pub const fn new(vendor_id: u16, product_id: u16) -> Self {
        Self {
            vendor_id,
            product_id,
        }
    }

    /// Attempts to extract `VID_xxxx&PID_yyyy` from a Windows PnP Hardware ID or Instance ID.
    pub fn from_windows_hardware_id(hw_id: &str) -> Option<Self> {
        let upper = hw_id.to_ascii_uppercase();
        let vid_idx = upper.find("VID_")?;
        let pid_idx = upper.find("PID_")?;

        let vid_hex = upper.get(vid_idx + 4..vid_idx + 8)?;
        let pid_hex = upper.get(pid_idx + 4..pid_idx + 8)?;

        let vendor_id = u16::from_str_radix(vid_hex, 16).ok()?;
        let product_id = u16::from_str_radix(pid_hex, 16).ok()?;
        Some(Self {
            vendor_id,
            product_id,
        })
    }
}

impl fmt::Display for VidPid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04X}:{:04X}", self.vendor_id, self.product_id)
    }
}

impl FromStr for VidPid {
    type Err = AdbHubError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let (v, p) = trimmed
            .split_once(':')
            .ok_or_else(|| AdbHubError::InvalidVidPid(s.to_string()))?;
        if v.len() != 4 || p.len() != 4 {
            return Err(AdbHubError::InvalidVidPid(s.to_string()));
        }
        let vendor_id =
            u16::from_str_radix(v, 16).map_err(|_| AdbHubError::InvalidVidPid(s.to_string()))?;
        let product_id =
            u16::from_str_radix(p, 16).map_err(|_| AdbHubError::InvalidVidPid(s.to_string()))?;
        Ok(Self {
            vendor_id,
            product_id,
        })
    }
}

/// Negotiated USB connection speed (matching USB/IP `usb_device_speed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsbSpeed {
    Unknown,
    /// USB 1.0 Low Speed (1.5 Mbps)
    Low,
    /// USB 1.1 Full Speed (12 Mbps)
    Full,
    /// USB 2.0 High Speed (480 Mbps)
    High,
    /// Wireless USB (480 Mbps)
    Wireless,
    /// USB 3.0 SuperSpeed (5 Gbps)
    Super,
    /// USB 3.1+ SuperSpeedPlus (10+ Gbps)
    SuperPlus,
}

impl UsbSpeed {
    /// Returns the numeric code expected by the USB/IP wire protocol (`enum usb_device_speed`).
    pub const fn to_usbip_code(self) -> u32 {
        match self {
            Self::Unknown => 0,
            Self::Low => 1,
            Self::Full => 2,
            Self::High => 3,
            Self::Wireless => 4,
            Self::Super => 5,
            Self::SuperPlus => 6,
        }
    }
}

/// Direction of a USB endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndpointDirection {
    /// Host-to-device (`OUT`, bit 7 = 0).
    Out,
    /// Device-to-host (`IN`, bit 7 = 1).
    In,
}

/// Transfer type of a USB endpoint (`bmAttributes & 0x03`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndpointTransferType {
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

/// Standard USB Endpoint Descriptor (USB 2.0 Spec Table 9-13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointDescriptor {
    /// Raw endpoint address (`bEndpointAddress`, e.g. `0x81` for EP1 IN, `0x02` for EP2 OUT).
    pub address: u8,
    /// Endpoint number (`address & 0x0F`).
    pub endpoint_number: u8,
    /// Transfer direction (`IN` or `OUT`).
    pub direction: EndpointDirection,
    /// Transfer type (`Control`, `Isochronous`, `Bulk`, `Interrupt`).
    pub transfer_type: EndpointTransferType,
    /// Raw `bmAttributes` byte.
    pub attributes: u8,
    /// Maximum packet size (`wMaxPacketSize`).
    pub max_packet_size: u16,
    /// Polling interval (`bInterval`).
    pub interval: u8,
}

impl EndpointDescriptor {
    /// Constructs an [`EndpointDescriptor`] from raw descriptor fields.
    pub const fn from_raw(address: u8, attributes: u8, max_packet_size: u16, interval: u8) -> Self {
        let direction = if (address & 0x80) != 0 {
            EndpointDirection::In
        } else {
            EndpointDirection::Out
        };
        let endpoint_number = address & 0x0F;
        let transfer_type = match attributes & 0x03 {
            0 => EndpointTransferType::Control,
            1 => EndpointTransferType::Isochronous,
            2 => EndpointTransferType::Bulk,
            _ => EndpointTransferType::Interrupt,
        };

        Self {
            address,
            endpoint_number,
            direction,
            transfer_type,
            attributes,
            max_packet_size,
            interval,
        }
    }
}

/// Standard USB Interface Descriptor (USB 2.0 Spec Table 9-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceDescriptor {
    /// Index of this interface (`bInterfaceNumber`).
    pub interface_number: u8,
    /// Alternate setting value (`bAlternateSetting`).
    pub alternate_setting: u8,
    /// Number of endpoints (`bNumEndpoints`).
    pub num_endpoints: u8,
    /// USB-IF Class code (`bInterfaceClass`).
    pub class_code: u8,
    /// USB-IF Subclass code (`bInterfaceSubClass`).
    pub subclass_code: u8,
    /// USB-IF Protocol code (`bInterfaceProtocol`).
    pub protocol_code: u8,
    /// Optional human-readable interface string descriptor (`iInterface`).
    pub interface_name: Option<String>,
    /// Endpoints belonging to this interface setting.
    pub endpoints: Vec<EndpointDescriptor>,
}

/// Standard 18-byte USB Device Descriptor (USB 2.0 Spec Table 9-8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbDeviceDescriptor {
    /// USB Specification Release Number in Binary-Coded Decimal (e.g. `0x0200` for USB 2.0).
    pub usb_version_bcd: u16,
    /// Device class code (`bDeviceClass`).
    pub device_class: u8,
    /// Device subclass code (`bDeviceSubClass`).
    pub device_subclass: u8,
    /// Device protocol code (`bDeviceProtocol`).
    pub device_protocol: u8,
    /// Maximum packet size for Endpoint 0 (`bMaxPacketSize0`).
    pub max_packet_size_ep0: u8,
    /// Vendor ID (`idVendor`).
    pub vendor_id: u16,
    /// Product ID (`idProduct`).
    pub product_id: u16,
    /// Device release number in BCD (`bcdDevice`).
    pub device_version_bcd: u16,
    /// Number of possible configurations (`bNumConfigurations`).
    pub num_configurations: u8,
}

impl UsbDeviceDescriptor {
    /// Parses a standard 18-byte USB Device Descriptor (`bDescriptorType == 0x01`).
    pub fn parse_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() < 18 {
            return Err(AdbHubError::MalformedDescriptor {
                reason: "Device descriptor requires at least 18 bytes".to_string(),
                offset: 0,
                available: bytes.len(),
            });
        }
        let b_length = bytes[0];
        let b_descriptor_type = bytes[1];
        if b_length < 18 || b_descriptor_type != 0x01 {
            return Err(AdbHubError::MalformedDescriptor {
                reason: format!(
                    "Invalid device descriptor header (bLength={b_length}, bDescriptorType=0x{b_descriptor_type:02X})"
                ),
                offset: 0,
                available: bytes.len(),
            });
        }

        Ok(Self {
            usb_version_bcd: u16::from_le_bytes([bytes[2], bytes[3]]),
            device_class: bytes[4],
            device_subclass: bytes[5],
            device_protocol: bytes[6],
            max_packet_size_ep0: bytes[7],
            vendor_id: u16::from_le_bytes([bytes[8], bytes[9]]),
            product_id: u16::from_le_bytes([bytes[10], bytes[11]]),
            device_version_bcd: u16::from_le_bytes([bytes[12], bytes[13]]),
            num_configurations: bytes[17],
        })
    }
}

/// Parsed USB Configuration Descriptor (USB 2.0 Spec Table 9-10) including its interfaces and endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigurationDescriptor {
    /// Configuration value (`bConfigurationValue`) used by `SET_CONFIGURATION`.
    pub configuration_value: u8,
    /// Number of interfaces in this configuration (`bNumInterfaces`).
    pub num_interfaces: u8,
    /// Configuration attributes (`bmAttributes`: self-powered, remote wakeup).
    pub attributes: u8,
    /// Maximum power consumption in 2mA units for USB 2.0 (`bMaxPower`).
    pub max_power_raw: u8,
    /// All interface descriptors (including alternate settings) in this configuration.
    pub interfaces: Vec<InterfaceDescriptor>,
}

impl ConfigurationDescriptor {
    /// Parses a full binary USB Configuration Descriptor blob (Configuration header + all nested
    /// Interface, Endpoint, and Class/Vendor descriptors).
    pub fn parse_bytes(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() < 9 {
            return Err(AdbHubError::MalformedDescriptor {
                reason: "Configuration descriptor requires at least 9 bytes".to_string(),
                offset: 0,
                available: bytes.len(),
            });
        }

        let header_len = bytes[0] as usize;
        let desc_type = bytes[1];
        if header_len < 9 || desc_type != 0x02 {
            return Err(AdbHubError::MalformedDescriptor {
                reason: format!(
                    "Invalid configuration descriptor header (bLength={header_len}, bDescriptorType=0x{desc_type:02X})"
                ),
                offset: 0,
                available: bytes.len(),
            });
        }

        let total_length = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
        let effective_len = total_length.min(bytes.len());
        let num_interfaces = bytes[4];
        let configuration_value = bytes[5];
        let attributes = bytes[7];
        let max_power_raw = bytes[8];

        let mut interfaces: Vec<InterfaceDescriptor> = Vec::new();
        let mut offset = header_len;

        while offset < effective_len {
            if offset + 2 > effective_len {
                break;
            }
            let b_len = bytes[offset] as usize;
            let b_type = bytes[offset + 1];

            if b_len < 2 || offset + b_len > effective_len {
                return Err(AdbHubError::MalformedDescriptor {
                    reason: format!("Invalid nested descriptor length {b_len} at offset {offset}"),
                    offset,
                    available: effective_len,
                });
            }

            match b_type {
                // Interface Descriptor (0x04)
                0x04 => {
                    if b_len < 9 {
                        return Err(AdbHubError::MalformedDescriptor {
                            reason: "Interface descriptor shorter than 9 bytes".to_string(),
                            offset,
                            available: b_len,
                        });
                    }
                    let slice = &bytes[offset..offset + b_len];
                    interfaces.push(InterfaceDescriptor {
                        interface_number: slice[2],
                        alternate_setting: slice[3],
                        num_endpoints: slice[4],
                        class_code: slice[5],
                        subclass_code: slice[6],
                        protocol_code: slice[7],
                        interface_name: None,
                        endpoints: Vec::new(),
                    });
                }
                // Endpoint Descriptor (0x05)
                0x05 => {
                    if b_len < 7 {
                        return Err(AdbHubError::MalformedDescriptor {
                            reason: "Endpoint descriptor shorter than 7 bytes".to_string(),
                            offset,
                            available: b_len,
                        });
                    }
                    let slice = &bytes[offset..offset + b_len];
                    let ep = EndpointDescriptor::from_raw(
                        slice[2],
                        slice[3],
                        u16::from_le_bytes([slice[4], slice[5]]),
                        slice[6],
                    );
                    if let Some(current_iface) = interfaces.last_mut() {
                        current_iface.endpoints.push(ep);
                    }
                }
                // Other descriptor types (IAD 0x0B, CDC Functional 0x24, HID 0x21, SS Companion 0x30)
                // are skipped cleanly without breaking traversal.
                _ => {}
            }

            offset += b_len;
        }

        Ok(Self {
            configuration_value,
            num_interfaces,
            attributes,
            max_power_raw,
            interfaces,
        })
    }
}

/// Computes a privacy-preserving SHA-256 hash of a raw USB serial number so that
/// `ADBHUB-LINK` can correlate device identity across reconnections without storing raw PII.
pub fn hash_serial_number(raw_serial: &str) -> String {
    let normalized = raw_serial.trim().to_ascii_uppercase();
    let mut hasher = Sha256::new();
    hasher.update(b"ADBHUB-LINK-SERIAL-V1:");
    hasher.update(normalized.as_bytes());
    hex::encode(hasher.finalize())
}

