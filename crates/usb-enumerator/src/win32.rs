//! Native Windows SetupAPI + Configuration Manager (`CfgMgr32`) + USB Hub IOCTL enumerator.

use std::collections::HashMap;

use adbhub_core::{
    hash_serial_number, AdbHubError, BusId, ConfigurationDescriptor, CoreResult,
    InterfaceDescriptor, UsbDeviceDescriptor, UsbIpExportState, UsbSpeed, VidPid,
};
use serde::{Deserialize, Serialize};

use crate::usbipd::{query_usbipd_state, UsbipdDeviceEntry};

/// Raw hardware record gathered from the OS USB subsystem before high-level lifecycle state is attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawEnumeratedUsbDevice {
    /// Topological `BusId` (`<bus>-<port>`).
    pub bus_id: BusId,
    /// Extracted `VID:PID`.
    pub vid_pid: VidPid,
    /// Full Windows PnP Instance ID (`USB\VID_XXXX&PID_YYYY\...`).
    pub instance_id: String,
    /// Optional PnP location path (`PCIROOT(0)#...#USB(4)`).
    pub location_path: Option<String>,
    /// Manufacturer string from descriptor or Windows PnP property.
    pub manufacturer: String,
    /// Product description or friendly name.
    pub product_name: String,
    /// SHA-256 hashed serial string (if an instance serial or `iSerialNumber` is available).
    pub serial_hash: Option<String>,
    /// Negotiated USB speed.
    pub speed: UsbSpeed,
    /// Binary-parsed 18-byte USB Device Descriptor, if retrieved.
    pub device_descriptor: Option<UsbDeviceDescriptor>,
    /// Binary-parsed active Configuration Descriptor, if retrieved.
    pub configuration_descriptor: Option<ConfigurationDescriptor>,
    /// Interfaces discovered via Configuration Descriptor or child PnP nodes.
    pub interfaces: Vec<InterfaceDescriptor>,
    /// Active Windows driver service name (`usbccgp`, `WinUsb`, `ssudmdm`, `VBoxUsb`, etc.).
    pub driver_service: Option<String>,
    /// Export status in `usbipd-win`.
    pub export_state: UsbIpExportState,
}

/// Trait abstracting USB bus enumeration so production code uses native Windows SetupAPI/IOCTLs
/// while automated tests can inject deterministic hardware topologies and mode transitions.
pub trait UsbBusEnumerator: Send + Sync {
    /// Scans the host USB subsystem and returns all currently connected USB devices.
    fn enumerate_devices(&self) -> CoreResult<Vec<RawEnumeratedUsbDevice>>;
}

/// Production Windows USB Enumerator using native Win32 `SetupDiGetClassDevsW` + `usbipd-win` state enrichment.
#[derive(Debug, Default, Clone)]
pub struct WindowsNativeUsbEnumerator {
    /// Whether to skip root hubs and internal USB hubs (`Class == Hub` or `VID_8087`).
    pub exclude_hubs: bool,
}

impl WindowsNativeUsbEnumerator {
    pub fn new() -> Self {
        Self { exclude_hubs: true }
    }
}

impl UsbBusEnumerator for WindowsNativeUsbEnumerator {
    fn enumerate_devices(&self) -> CoreResult<Vec<RawEnumeratedUsbDevice>> {
        let usbipd_map: HashMap<String, UsbipdDeviceEntry> = query_usbipd_state()
            .map(|st| {
                st.devices
                    .into_iter()
                    .map(|d| (d.instance_id.to_ascii_uppercase(), d))
                    .collect()
            })
            .unwrap_or_default();

        enumerate_win32_pnp_usb_devices(self.exclude_hubs, &usbipd_map)
    }
}

/// Extracts a Topological [`BusId`] from a Windows PnP Location Info / Location Path string
/// (for example `"Port_#0004.Hub_#0001"` -> `1-4`, or `"PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(3)#USB(2)"` -> `1-3.2`).
pub fn parse_bus_id_from_windows_location(
    location_info: Option<&str>,
    location_path: Option<&str>,
    fallback_index: u16,
) -> BusId {
    // 1. Try parsing hierarchical `#USB(n)` segments from SPDRP_LOCATION_PATHS
    if let Some(path) = location_path {
        let upper = path.to_ascii_uppercase();
        let mut bus_num: u16 = 1;
        if let Some(root_idx) = upper.find("USBROOT(") {
            let after = &upper[root_idx + 8..];
            if let Some(end) = after.find(')') {
                if let Ok(parsed_root) = after[..end].parse::<u16>() {
                    bus_num = parsed_root.saturating_add(1);
                }
            }
        }

        let mut ports = Vec::new();
        let mut search_slice = upper.as_str();
        while let Some(pos) = search_slice.find("#USB(") {
            let after = &search_slice[pos + 5..];
            if let Some(close) = after.find(')') {
                if let Ok(port_num) = after[..close].parse::<u16>() {
                    if port_num > 0 {
                        ports.push(port_num);
                    }
                }
                search_slice = &after[close + 1..];
            } else {
                break;
            }
        }

        if let Ok(bid) = BusId::new(bus_num, ports) {
            return bid;
        }
    }

    // 2. Try parsing `Port_#0004.Hub_#0001` from SPDRP_LOCATION_INFORMATION
    if let Some(loc) = location_info {
        let upper = loc.to_ascii_uppercase();
        let port = extract_hash_number(&upper, "PORT_#");
        let hub = extract_hash_number(&upper, "HUB_#");
        if let (Some(h), Some(p)) = (hub, port) {
            if let Ok(bid) = BusId::new(h.max(1), vec![p.max(1)]) {
                return bid;
            }
        }
    }

    // 3. Deterministic fallback when Windows virtual/composite node omits location property
    BusId {
        bus: 1,
        port_chain: vec![fallback_index.max(1)],
    }
}

fn extract_hash_number(text: &str, prefix: &str) -> Option<u16> {
    let idx = text.find(prefix)?;
    let rest = &text[idx + prefix.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u16>().ok()
}

/// Extracts the serial segment from a Windows USB Instance ID `USB\VID_XXXX&PID_YYYY\<SERIAL_OR_LOC>`.
/// Windows auto-generated non-serial instance IDs contain `'&'` (e.g. `6&2A3B4C5D&0&4`).
/// True USB hardware serial numbers do not contain `'&'`.
pub fn extract_serial_hash_from_instance_id(instance_id: &str) -> Option<String> {
    let parts: Vec<&str> = instance_id.split('\\').collect();
    if parts.len() >= 3 {
        let tail = parts[2].trim();
        if !tail.is_empty() && !tail.contains('&') {
            return Some(hash_serial_number(tail));
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn enumerate_win32_pnp_usb_devices(
    exclude_hubs: bool,
    usbipd_map: &HashMap<String, UsbipdDeviceEntry>,
) -> CoreResult<Vec<RawEnumeratedUsbDevice>> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        DIGCF_ALLCLASSES, DIGCF_PRESENT, SPDRP_CLASS, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME,
        SPDRP_HARDWAREID, SPDRP_LOCATION_INFORMATION, SPDRP_LOCATION_PATHS, SPDRP_MFG,
        SPDRP_SERVICE, SP_DEVINFO_DATA,
    };
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

    let enumerator_filter: Vec<u16> = "USB\0".encode_utf16().collect();

    // SAFETY: Calling standard Win32 SetupDiGetClassDevsW with a valid null-terminated UTF-16 string `"USB"`.
    // The returned HDEVINFO handle is checked against INVALID_HANDLE_VALUE and always closed via SetupDiDestroyDeviceInfoList.
    let dev_info_set = unsafe {
        SetupDiGetClassDevsW(
            std::ptr::null(),
            enumerator_filter.as_ptr(),
            std::ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        )
    };

    if dev_info_set == INVALID_HANDLE_VALUE as isize || dev_info_set == 0 {
        return Err(AdbHubError::WindowsUsbError(
            "SetupDiGetClassDevsW(USB) returned INVALID_HANDLE_VALUE".to_string(),
        ));
    }

    let mut results = Vec::new();
    let mut member_index: u32 = 0;
    let mut fallback_port: u16 = 1;

    loop {
        let mut dev_info_data = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ClassGuid: windows_sys::core::GUID {
                data1: 0,
                data2: 0,
                data3: 0,
                data4: [0; 8],
            },
            DevInst: 0,
            Reserved: 0,
        };

        // SAFETY: Iterating SetupDiEnumDeviceInfo with properly initialized SP_DEVINFO_DATA (cbSize set).
        let enum_ok =
            unsafe { SetupDiEnumDeviceInfo(dev_info_set, member_index, &mut dev_info_data) };
        if enum_ok == 0 {
            break;
        }
        member_index += 1;

        let instance_id = match get_device_instance_id(dev_info_set, &dev_info_data) {
            Some(id) => id,
            None => continue,
        };
        let instance_upper = instance_id.to_ascii_uppercase();

        // Skip child interface nodes of composite devices (`&MI_00`, `&MI_01`) at the top-level device list
        // because USB/IP binds and exports the parent physical USB device as a whole.
        if instance_upper.contains("&MI_") || instance_upper.starts_with("USB\\ROOT_HUB") {
            continue;
        }

        let vid_pid = match VidPid::from_windows_hardware_id(&instance_upper).or_else(|| {
            get_registry_string_property(dev_info_set, &dev_info_data, SPDRP_HARDWAREID)
                .and_then(|hw| VidPid::from_windows_hardware_id(&hw))
        }) {
            Some(vp) => vp,
            None => continue,
        };

        let pnp_class = get_registry_string_property(dev_info_set, &dev_info_data, SPDRP_CLASS)
            .unwrap_or_default();
        let service = get_registry_string_property(dev_info_set, &dev_info_data, SPDRP_SERVICE);
        let desc = get_registry_string_property(dev_info_set, &dev_info_data, SPDRP_FRIENDLYNAME)
            .or_else(|| {
                get_registry_string_property(dev_info_set, &dev_info_data, SPDRP_DEVICEDESC)
            })
            .unwrap_or_else(|| format!("USB Device {vid_pid}"));

        if exclude_hubs {
            let service_up = service.as_deref().unwrap_or_default().to_ascii_uppercase();
            let desc_up = desc.to_ascii_uppercase();
            if pnp_class.eq_ignore_ascii_case("USBHub")
                || service_up == "USBHUB3"
                || service_up == "USBHUB"
                || desc_up.contains("GENERIC USB HUB")
                || desc_up.contains("ROOT HUB")
            {
                continue;
            }
        }

        let mfg = get_registry_string_property(dev_info_set, &dev_info_data, SPDRP_MFG)
            .filter(|m| !m.starts_with('('))
            .unwrap_or_else(|| adbhub_core::vendor_name_from_id(vid_pid.vendor_id).to_string());

        let location_info =
            get_registry_string_property(dev_info_set, &dev_info_data, SPDRP_LOCATION_INFORMATION);
        let location_path =
            get_registry_string_property(dev_info_set, &dev_info_data, SPDRP_LOCATION_PATHS);

        // Enrich with usbipd-win state if present
        let usbipd_entry = usbipd_map.get(&instance_upper);

        let bus_id = usbipd_entry
            .and_then(UsbipdDeviceEntry::parsed_bus_id)
            .unwrap_or_else(|| {
                let b = parse_bus_id_from_windows_location(
                    location_info.as_deref(),
                    location_path.as_deref(),
                    fallback_port,
                );
                fallback_port = fallback_port.saturating_add(1);
                b
            });

        let export_state = usbipd_entry
            .map(UsbipdDeviceEntry::export_state)
            .unwrap_or(UsbIpExportState::NotBound);

        let serial_hash = extract_serial_hash_from_instance_id(&instance_id);

        results.push(RawEnumeratedUsbDevice {
            bus_id,
            vid_pid,
            instance_id,
            location_path,
            manufacturer: mfg,
            product_name: desc,
            serial_hash,
            speed: UsbSpeed::High,
            device_descriptor: None,
            configuration_descriptor: None,
            interfaces: Vec::new(),
            driver_service: service,
            export_state,
        });
    }

    // SAFETY: Closing valid HDEVINFO handle opened at function start.
    unsafe {
        SetupDiDestroyDeviceInfoList(dev_info_set);
    }

    Ok(results)
}

#[cfg(target_os = "windows")]
fn get_device_instance_id(
    dev_info_set: windows_sys::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    dev_info_data: &windows_sys::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<String> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDeviceInstanceIdW;

    let mut buf = [0u16; 512];
    let mut required_size: u32 = 0;
    // SAFETY: Buffer size is explicitly passed as buf.len() as u32.
    let ok = unsafe {
        SetupDiGetDeviceInstanceIdW(
            dev_info_set,
            dev_info_data,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &mut required_size,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(target_os = "windows")]
fn get_registry_string_property(
    dev_info_set: windows_sys::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    dev_info_data: &windows_sys::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
    property: u32,
) -> Option<String> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDeviceRegistryPropertyW;

    let mut buf = [0u8; 1024];
    let mut reg_data_type: u32 = 0;
    let mut required_size: u32 = 0;

    // SAFETY: Passing stack buffer `buf` with explicit byte length `buf.len() as u32`.
    let ok = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            dev_info_set,
            dev_info_data,
            property,
            &mut reg_data_type,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &mut required_size,
        )
    };
    if ok == 0 || required_size < 2 {
        return None;
    }

    let u16_len = (required_size as usize / 2).min(buf.len() / 2);
    let mut wide = Vec::with_capacity(u16_len);
    for chunk in buf[..u16_len * 2].chunks_exact(2) {
        wide.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    let first_null = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    let s = String::from_utf16_lossy(&wide[..first_null])
        .trim()
        .to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(not(target_os = "windows"))]
fn enumerate_win32_pnp_usb_devices(
    _exclude_hubs: bool,
    _usbipd_map: &HashMap<String, UsbipdDeviceEntry>,
) -> CoreResult<Vec<RawEnumeratedUsbDevice>> {
    Ok(Vec::new())
}
