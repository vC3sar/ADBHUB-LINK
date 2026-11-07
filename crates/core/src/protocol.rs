//! Multi-protocol USB mode detection and classification for mobile repair workflows.
//!
//! Supports the 6 mandatory modes of ADBHUB-LINK:
//! 1. ADB (`AndroidAdb` / `CompositeAdbMtp`)
//! 2. MTP (`Mtp`)
//! 3. Qualcomm EDL (`QualcommEdl`)
//! 4. MediaTek BROM / Preloader (`MediatekBrom` / `MediatekPreloader`)
//! 5. SPD / Spreadtrum / Unisoc (`SpdUnisoc`)
//! 6. Samsung Download Mode / Odin (`SamsungDownload`)

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::usb::{InterfaceDescriptor, UsbDeviceDescriptor, VidPid};

/// Well-known USB Vendor IDs in mobile servicing.
pub mod vendors {
    pub const SAMSUNG: u16 = 0x04E8;
    pub const QUALCOMM: u16 = 0x05C6;
    pub const MEDIATEK: u16 = 0x0E8D;
    pub const SPREADTRUM_UNISOC: u16 = 0x1782;
    pub const GOOGLE: u16 = 0x18D1;
    pub const XIAOMI: u16 = 0x2717;
    pub const HUAWEI: u16 = 0x12D1;
    pub const MOTOROLA: u16 = 0x22B8;
    pub const OPPO_ONEPLUS_REALME: u16 = 0x22D9;
    pub const VIVO: u16 = 0x2D95;
    pub const SONY: u16 = 0x0FCE;
    pub const LG: u16 = 0x1004;
    pub const ASUS: u16 = 0x0B05;
    pub const ZTE: u16 = 0x19D2;
    pub const LENOVO: u16 = 0x17EF;
    pub const NOTHING: u16 = 0x2A70;
}

/// Primary operating mode of a connected mobile USB device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsbDeviceMode {
    /// Android Debug Bridge (`FF/42/01`) standalone.
    AndroidAdb,
    /// Media Transfer Protocol (`06/01/01` or `FF` with MTP descriptor).
    Mtp,
    /// Composite Android device exposing both ADB and MTP simultaneously.
    CompositeAdbMtp,
    /// Android Fastboot / Bootloader (`FF/42/03`).
    Fastboot,
    /// Qualcomm Emergency Download Mode (HS-USB QDLoader 9008 / 900E).
    QualcommEdl,
    /// MediaTek BootROM (`0E8D:0003` — MTK USB Port).
    MediatekBrom,
    /// MediaTek Preloader / Download Agent (`0E8D:2000`, `0E8D:2001`, `0E8D:0006`).
    MediatekPreloader,
    /// Spreadtrum / Unisoc BootROM or Diag Download (`1782:4D00`, `1782:4D03`, `1782:3D00`).
    SpdUnisoc,
    /// Samsung Download Mode (Odin / Loke protocol — `04E8:685D`, `04E8:6601`, `04E8:68C3`).
    SamsungDownload,
    /// Generic or unrecognized USB device.
    UnknownUsbDevice,
}

impl UsbDeviceMode {
    /// Human-readable badge label matching UI requirements (Section 13).
    pub const fn display_label(self) -> &'static str {
        match self {
            Self::AndroidAdb => "Android / ADB",
            Self::Mtp => "MTP",
            Self::CompositeAdbMtp => "Android / ADB + MTP",
            Self::Fastboot => "Android Fastboot",
            Self::QualcommEdl => "Qualcomm EDL",
            Self::MediatekBrom => "MediaTek BROM",
            Self::MediatekPreloader => "MediaTek Preloader",
            Self::SpdUnisoc => "SPD / Unisoc",
            Self::SamsungDownload => "Samsung Download Mode",
            Self::UnknownUsbDevice => "Unknown USB Device",
        }
    }

    /// Returns `true` if this mode is used for low-level flashing or boot-level recovery
    /// where unexpected disconnection can corrupt partitions.
    pub const fn is_flashing_or_boot_mode(self) -> bool {
        matches!(
            self,
            Self::QualcommEdl
                | Self::MediatekBrom
                | Self::MediatekPreloader
                | Self::SpdUnisoc
                | Self::SamsungDownload
                | Self::Fastboot
        )
    }

    /// Recommended default URB transfer timeout in milliseconds for this mode.
    ///
    /// As specified in Section 2.6, Samsung Download Mode and low-level flash modes require
    /// extended timeouts to accommodate UFS/eMMC partition erase and large block writes.
    pub const fn recommended_urb_timeout_ms(self) -> u64 {
        match self {
            Self::SamsungDownload => 60_000,
            Self::QualcommEdl | Self::MediatekBrom | Self::MediatekPreloader | Self::SpdUnisoc => {
                45_000
            }
            Self::Fastboot => 30_000,
            Self::AndroidAdb | Self::Mtp | Self::CompositeAdbMtp => 15_000,
            Self::UnknownUsbDevice => 10_000,
        }
    }

    /// Whether this mode requires sub-second "Pre-Bind" capture on the USB port due to a
    /// short BootROM handshake watchdog (e.g., MediaTek BROM ~2s window).
    pub const fn requires_fast_prebind(self) -> bool {
        matches!(
            self,
            Self::MediatekBrom | Self::MediatekPreloader | Self::SpdUnisoc
        )
    }
}

impl fmt::Display for UsbDeviceMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_label())
    }
}

/// Verification state for Samsung Odin compatibility (Section 13:
/// "No mostrar compatibilidad como verificada hasta contar con pruebas reales").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OdinCompatibilityStatus {
    /// Lab-tested with real firmware flashing over ADBHUB-LINK.
    Verified,
    /// Detected as Samsung Download Mode, but this specific model/revision has not yet been
    /// signed off in lab flash testing.
    Unverified,
    /// Not a Samsung Download Mode device or unknown state.
    Unknown,
}

impl OdinCompatibilityStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verificada",
            Self::Unverified => "no verificada",
            Self::Unknown => "desconocida",
        }
    }
}

/// Specific telemetry and diagnostic metadata when a Samsung device is in Download Mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamsungDownloadMetadata {
    /// Always `"Samsung"` for `04E8`.
    pub manufacturer: String,
    /// Mode label: `"Download Mode"`.
    pub mode_label: String,
    /// Detected local or host driver (e.g., `"ssudmdm / ssudbus"`, `"VBoxUsb (Exported)"`, or `"CDC ACM"`).
    pub detected_driver: Option<String>,
    /// Whether the device has both CDC Control (`0x02`) and CDC Data (`0x0A`) interfaces expected by Odin.
    pub has_odin_cdc_interfaces: bool,
    /// Bulk IN endpoint address used for `"LOKE"` / PIT responses.
    pub bulk_in_endpoint: Option<u8>,
    /// Bulk OUT endpoint address used for `"ODIN"` / firmware blocks.
    pub bulk_out_endpoint: Option<u8>,
    /// Verification status (defaults strictly to `Unverified` until proven in lab matrix).
    pub odin_compatibility: OdinCompatibilityStatus,
    /// Warning message displayed in UI when connected or flashing.
    pub safety_warning: String,
}

/// Detailed multi-protocol classification result for a USB device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolClassification {
    /// Primary operating mode.
    pub primary_mode: UsbDeviceMode,
    /// All individual capabilities detected across interfaces (e.g., `[Mtp, AndroidAdb]`).
    pub capabilities: Vec<UsbDeviceMode>,
    /// Inferred manufacturer name based on USB descriptor or known VID table.
    pub manufacturer_name: String,
    /// Samsung-specific Download Mode diagnostic metadata, if applicable.
    pub samsung_download_info: Option<SamsungDownloadMetadata>,
}

/// Inspects a device's [`VidPid`], optional [`UsbDeviceDescriptor`], [`InterfaceDescriptor`] list,
/// product description, and active Windows service/driver name to classify its mobile repair mode.
pub fn classify_usb_device(
    vid_pid: VidPid,
    device_desc: Option<&UsbDeviceDescriptor>,
    interfaces: &[InterfaceDescriptor],
    product_name: Option<&str>,
    driver_service: Option<&str>,
) -> ProtocolClassification {
    let mut capabilities = Vec::new();
    let prod_upper = product_name.unwrap_or_default().to_ascii_uppercase();

    // 1. Check Vendor/Product specific BootROM, EDL, and Download Mode signatures first.
    match (vid_pid.vendor_id, vid_pid.product_id) {
        // Qualcomm EDL 9008 & 900E
        (vendors::QUALCOMM, 0x9008 | 0x900E | 0x9091) => {
            capabilities.push(UsbDeviceMode::QualcommEdl);
        }
        // MediaTek BootROM
        (vendors::MEDIATEK, 0x0003) => {
            capabilities.push(UsbDeviceMode::MediatekBrom);
        }
        // MediaTek Preloader / DA VCOM
        (vendors::MEDIATEK, 0x2000 | 0x2001 | 0x0006) => {
            capabilities.push(UsbDeviceMode::MediatekPreloader);
        }
        // Spreadtrum / Unisoc Download & Diag
        (vendors::SPREADTRUM_UNISOC, 0x4D00 | 0x4D03 | 0x3D00) => {
            capabilities.push(UsbDeviceMode::SpdUnisoc);
        }
        // Samsung Download Mode (Odin)
        // Note: 0x685D is the standard Download Mode PID; 0x6601 and 0x68C3 are also used on
        // specific Galaxy families in Download Mode.
        (vendors::SAMSUNG, 0x685D | 0x6601 | 0x68C3) => {
            capabilities.push(UsbDeviceMode::SamsungDownload);
        }
        _ => {}
    }

    // 2. Inspect interface-level class/subclass/protocol descriptors (USB-IF standard)
    let mut has_adb = false;
    let mut has_mtp = false;
    let mut has_fastboot = false;
    let mut has_cdc_control = false;
    let mut has_cdc_data = false;
    let mut bulk_in_ep = None;
    let mut bulk_out_ep = None;

    for iface in interfaces {
        // Record first Bulk IN / Bulk OUT endpoints for diagnostic metadata
        for ep in &iface.endpoints {
            if ep.transfer_type == crate::usb::EndpointTransferType::Bulk {
                match ep.direction {
                    crate::usb::EndpointDirection::In if bulk_in_ep.is_none() => {
                        bulk_in_ep = Some(ep.address);
                    }
                    crate::usb::EndpointDirection::Out if bulk_out_ep.is_none() => {
                        bulk_out_ep = Some(ep.address);
                    }
                    _ => {}
                }
            }
        }

        match (iface.class_code, iface.subclass_code, iface.protocol_code) {
            // Official Android ADB interface signature: Class 0xFF, SubClass 0x42, Protocol 0x01
            (0xFF, 0x42, 0x01) => {
                has_adb = true;
            }
            // Official Android Fastboot interface signature: Class 0xFF, SubClass 0x42, Protocol 0x03
            (0xFF, 0x42, 0x03) => {
                has_fastboot = true;
            }
            // Standard Still Image / MTP class (0x06, 0x01, 0x01)
            (0x06, 0x01, 0x01) => {
                has_mtp = true;
            }
            // Vendor-specific MTP interface (0xFF, 0xFF, 0x00) or named "MTP"
            (0xFF, _, _) => {
                if let Some(name) = &iface.interface_name {
                    let name_up = name.to_ascii_uppercase();
                    if name_up.contains("MTP") {
                        has_mtp = true;
                    } else if name_up.contains("ADB") {
                        has_adb = true;
                    }
                }
            }
            // CDC ACM Control (0x02, 0x02, _)
            (0x02, 0x02, _) => {
                has_cdc_control = true;
            }
            // CDC Data (0x0A, _, _)
            (0x0A, _, _) => {
                has_cdc_data = true;
            }
            _ => {}
        }
    }

    // 3. Fallback heuristics when Windows PnP description is available while device is bound or composite
    if !has_adb && (prod_upper.contains("ADB INTERFACE") || prod_upper.contains("ANDROID ADB")) {
        has_adb = true;
    }
    if !has_mtp
        && (prod_upper.contains("MTP")
            || (vid_pid.vendor_id == vendors::SAMSUNG && vid_pid.product_id == 0x6860)
            || (vid_pid.vendor_id == vendors::GOOGLE
                && (vid_pid.product_id == 0x4EE1 || vid_pid.product_id == 0x4EE2)))
    {
        has_mtp = true;
    }
    if !has_adb && vid_pid.vendor_id == vendors::GOOGLE && vid_pid.product_id == 0x4EE2 {
        has_adb = true;
    }
    if !capabilities.contains(&UsbDeviceMode::QualcommEdl)
        && (prod_upper.contains("QDLOADER 9008") || prod_upper.contains("9008"))
    {
        capabilities.push(UsbDeviceMode::QualcommEdl);
    }
    if !capabilities.contains(&UsbDeviceMode::MediatekBrom)
        && (prod_upper.contains("MTK USB PORT") || prod_upper.contains("MEDIATEK BOOT"))
    {
        capabilities.push(UsbDeviceMode::MediatekBrom);
    }
    if !capabilities.contains(&UsbDeviceMode::MediatekPreloader)
        && prod_upper.contains("PRELOADER USB VCOM")
    {
        capabilities.push(UsbDeviceMode::MediatekPreloader);
    }
    if !capabilities.contains(&UsbDeviceMode::SpdUnisoc)
        && (prod_upper.contains("SPRD U2S DIAG") || prod_upper.contains("SCI USB2SERIAL"))
    {
        capabilities.push(UsbDeviceMode::SpdUnisoc);
    }

    // Add interface-derived modes
    if has_mtp && !capabilities.contains(&UsbDeviceMode::Mtp) {
        capabilities.push(UsbDeviceMode::Mtp);
    }
    if has_adb && !capabilities.contains(&UsbDeviceMode::AndroidAdb) {
        capabilities.push(UsbDeviceMode::AndroidAdb);
    }
    if has_fastboot && !capabilities.contains(&UsbDeviceMode::Fastboot) {
        capabilities.push(UsbDeviceMode::Fastboot);
    }

    // 4. Determine primary mode with clear precedence
    let primary_mode = if capabilities.contains(&UsbDeviceMode::SamsungDownload) {
        // Distinguish Samsung 04E8:685D in Download Mode vs normal ACM modem when ADB/MTP are absent
        if has_adb || has_mtp {
            if has_adb && has_mtp {
                UsbDeviceMode::CompositeAdbMtp
            } else if has_adb {
                UsbDeviceMode::AndroidAdb
            } else {
                UsbDeviceMode::Mtp
            }
        } else {
            UsbDeviceMode::SamsungDownload
        }
    } else if capabilities.contains(&UsbDeviceMode::QualcommEdl) {
        UsbDeviceMode::QualcommEdl
    } else if capabilities.contains(&UsbDeviceMode::MediatekBrom) {
        UsbDeviceMode::MediatekBrom
    } else if capabilities.contains(&UsbDeviceMode::MediatekPreloader) {
        UsbDeviceMode::MediatekPreloader
    } else if capabilities.contains(&UsbDeviceMode::SpdUnisoc) {
        UsbDeviceMode::SpdUnisoc
    } else if has_adb && has_mtp {
        UsbDeviceMode::CompositeAdbMtp
    } else if has_adb {
        UsbDeviceMode::AndroidAdb
    } else if has_mtp {
        UsbDeviceMode::Mtp
    } else if has_fastboot {
        UsbDeviceMode::Fastboot
    } else {
        UsbDeviceMode::UnknownUsbDevice
    };

    let manufacturer_name = vendor_name_from_id(vid_pid.vendor_id).to_string();

    let samsung_download_info = if primary_mode == UsbDeviceMode::SamsungDownload {
        let has_cdc_class_on_dev = device_desc.map(|d| d.device_class == 0x02).unwrap_or(false);
        let has_odin_cdc_interfaces =
            (has_cdc_control && has_cdc_data) || has_cdc_class_on_dev || interfaces.is_empty();

        Some(SamsungDownloadMetadata {
            manufacturer: "Samsung".to_string(),
            mode_label: "Download Mode".to_string(),
            detected_driver: driver_service.map(str::to_string),
            has_odin_cdc_interfaces,
            bulk_in_endpoint: bulk_in_ep,
            bulk_out_endpoint: bulk_out_ep,
            // Strictly Unverified until validated in physical device test matrix (Rule 15 & Section 13)
            odin_compatibility: OdinCompatibilityStatus::Unverified,
            safety_warning: "ADVERTENCIA CRÍTICA: Dispositivo Samsung en Download Mode. No desconecte el cable físico ni cierre la sesión mientras Odin esté transmitiendo particiones (BL/AP/CP/CSC).".to_string(),
        })
    } else {
        None
    };

    ProtocolClassification {
        primary_mode,
        capabilities,
        manufacturer_name,
        samsung_download_info,
    }
}

/// Maps known mobile USB Vendor IDs to canonical manufacturer names.
pub const fn vendor_name_from_id(vendor_id: u16) -> &'static str {
    match vendor_id {
        vendors::SAMSUNG => "Samsung Electronics",
        vendors::QUALCOMM => "Qualcomm Technologies",
        vendors::MEDIATEK => "MediaTek Inc.",
        vendors::SPREADTRUM_UNISOC => "Spreadtrum / Unisoc",
        vendors::GOOGLE => "Google Inc.",
        vendors::XIAOMI => "Xiaomi Inc.",
        vendors::HUAWEI => "Huawei Technologies",
        vendors::MOTOROLA => "Motorola Mobility",
        vendors::OPPO_ONEPLUS_REALME => "OPPO / OnePlus / Realme",
        vendors::VIVO => "Vivo Mobile",
        vendors::SONY => "Sony Mobile",
        vendors::LG => "LG Electronics",
        vendors::ASUS => "ASUSTeK Computer",
        vendors::ZTE => "ZTE Corporation",
        vendors::LENOVO => "Lenovo",
        vendors::NOTHING => "Nothing Technology",
        _ => "Generic USB Vendor",
    }
}

