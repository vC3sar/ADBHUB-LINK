//! # `adb.rs` — Driverless ADB Engine & AnyDesk-style Pairing Code
//!
//! Provides pure user-mode Android Debug Bridge discovery, remote command tunneling,
//! and human-readable pairing codes (`A55A-DDD5-CCH6-RRTY` or `954-218-037`) for zero-driver
//! remote phone diagnostics and repair.
//!
//! ### Cross-Platform:
//! - 100% compatible with Windows and Linux (Lubuntu, Ubuntu, Debian, Arch, etc.).
//! - Zero kernel drivers (`usbipd-win` or `usbip-win2` NOT required).
//! - Works with standard Android USB Debugging.

use std::collections::HashMap;
use std::fmt;
use std::process::Command;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AdbHubError, CoreResult};

/// Human-readable AnyDesk-style session pairing code (e.g. `A55A-DDD5-CCH6-RRTY` or `749-218-053`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnyDeskCode(String);

impl AnyDeskCode {
    /// Generates a new 16-character alphanumeric pairing code formatted in 4 blocks of 4
    /// (e.g. `A55A-DDD5-CCH6-RRTY`), matching the user's explicit request format.
    pub fn generate_alphanumeric() -> Self {
        const CHARSET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ"; // 32 unambiguous chars
        let rand_bytes = Uuid::new_v4();
        let bytes = rand_bytes.as_bytes();
        let mut raw = String::with_capacity(16);
        for b in bytes {
            let idx = (*b as usize) % CHARSET.len();
            raw.push(CHARSET[idx] as char);
        }

        let formatted = format!(
            "{}-{}-{}-{}",
            &raw[0..4],
            &raw[4..8],
            &raw[8..12],
            &raw[12..16]
        );
        Self(formatted)
    }

    /// Generates a simple 9-digit numeric code formatted in 3 blocks (e.g. `749-218-053`).
    pub fn generate_numeric() -> Self {
        let rand_bytes = Uuid::new_v4();
        let b = rand_bytes.as_bytes();
        let n1 = 100 + ((b[0] as u32 | ((b[1] as u32) << 8)) % 900);
        let n2 = 100 + ((b[2] as u32 | ((b[3] as u32) << 8)) % 900);
        let n3 = 100 + ((b[4] as u32 | ((b[5] as u32) << 8)) % 900);
        Self(format!("{n1:03}-{n2:03}-{n3:03}"))
    }

    /// Generates a 16-character pairing code that deterministically encodes the Client's
    /// local LAN IPv4 address and optional public Cloud Tunnel port (`bore.pub`), so any
    /// Technician (on LAN or over VPN/Internet) can resolve the exact endpoint from the code alone.
    pub fn encode_station(local_ip: &str, cloud_tunnel_port: u16, salt_byte: u8) -> Self {
        let octets: [u8; 4] = local_ip
            .split('.')
            .filter_map(|p| p.parse::<u8>().ok())
            .collect::<Vec<u8>>()
            .try_into()
            .unwrap_or([127, 0, 0, 1]);

        let port_be = cloud_tunnel_port.to_be_bytes();
        let raw_bytes: [u8; 8] = [
            0xAD ^ salt_byte,
            salt_byte,
            octets[0] ^ salt_byte ^ 0x5A,
            octets[1] ^ salt_byte ^ 0xA5,
            octets[2] ^ salt_byte ^ 0x3C,
            octets[3] ^ salt_byte ^ 0xC3,
            port_be[0] ^ salt_byte ^ 0x7E,
            port_be[1] ^ salt_byte ^ 0xE7,
        ];

        let hex_str = format!(
            "{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}",
            raw_bytes[0],
            raw_bytes[1],
            raw_bytes[2],
            raw_bytes[3],
            raw_bytes[4],
            raw_bytes[5],
            raw_bytes[6],
            raw_bytes[7]
        );
        Self(hex_str)
    }

    /// Generates a 9-digit numeric station code (`NNN-NNN-NNN`) encoding both the Cloud Tunnel
    /// port (`0..=65535`) and the last two octets of the local LAN IPv4 (`192.168.c.d`).
    pub fn encode_numeric_station(local_ip: &str, cloud_tunnel_port: u16) -> Self {
        let octets: [u8; 4] = local_ip
            .split('.')
            .filter_map(|p| p.parse::<u8>().ok())
            .collect::<Vec<u8>>()
            .try_into()
            .unwrap_or([192, 168, 1, 1]);

        let val: u64 = if cloud_tunnel_port > 0 {
            (cloud_tunnel_port as u64) * 10_000
                + ((octets[2] as u64 % 100) * 100)
                + (octets[3] as u64 % 100)
        } else {
            800_000_000 + ((octets[2] as u64 % 1000) * 1000) + (octets[3] as u64 % 1000)
        };

        let n1 = (val / 1_000_000) % 1000;
        let n2 = (val / 1_000) % 1000;
        let n3 = val % 1000;
        Self(format!("{n1:03}-{n2:03}-{n3:03}"))
    }

    /// Resolves a parsed `AnyDeskCode` into ordered candidate `host:port` endpoints
    /// (Cloud Tunnel `bore.pub:<port>` and/or Direct LAN `<ip>:5555`).
    pub fn resolve_endpoints(&self) -> Vec<String> {
        let cleaned: String = self
            .0
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_uppercase();

        let mut endpoints = Vec::new();

        // 1. Check 16-char hex station code
        if cleaned.len() == 16 && cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
            let mut b = [0u8; 8];
            let mut ok = true;
            for i in 0..8 {
                if let Ok(v) = u8::from_str_radix(&cleaned[i * 2..i * 2 + 2], 16) {
                    b[i] = v;
                } else {
                    ok = false;
                    break;
                }
            }
            if ok && (b[0] ^ b[1]) == 0xAD {
                let s = b[1];
                let ip = format!(
                    "{}.{}.{}.{}",
                    b[2] ^ s ^ 0x5A,
                    b[3] ^ s ^ 0xA5,
                    b[4] ^ s ^ 0x3C,
                    b[5] ^ s ^ 0xC3
                );
                let port = u16::from_be_bytes([b[6] ^ s ^ 0x7E, b[7] ^ s ^ 0xE7]);
                if port > 0 {
                    endpoints.push(format!("bore.pub:{port}"));
                }
                if ip != "0.0.0.0" && ip != "127.0.0.1" {
                    endpoints.push(format!("{ip}:5555"));
                }
                if !endpoints.is_empty() {
                    return endpoints;
                }
            }
        }

        // 2. Check 9-digit numeric station code
        if cleaned.len() == 9 && cleaned.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(val) = cleaned.parse::<u64>() {
                if (800_000_000..900_000_000).contains(&val) {
                    let rem = val - 800_000_000;
                    let c = rem / 1000;
                    let d = rem % 1000;
                    if c <= 255 && d <= 255 {
                        endpoints.push(format!("192.168.{c}.{d}:5555"));
                        return endpoints;
                    }
                } else if (10_240_000..=655_359_999).contains(&val) {
                    let port = val / 10_000;
                    let c = (val % 10_000) / 100;
                    let d = val % 100;
                    endpoints.push(format!("bore.pub:{port}"));
                    if d > 0 {
                        endpoints.push(format!("192.168.{c}.{d}:5555"));
                    }
                    return endpoints;
                }
            }
        }

        endpoints
    }
    /// Returns the canonical display string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Normalizes user input (handling spaces, dashes, lowercase) into canonical format.
    pub fn parse_flexible(input: &str) -> CoreResult<Self> {
        let cleaned: String = input
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_uppercase();

        if cleaned.len() == 16 {
            Ok(Self(format!(
                "{}-{}-{}-{}",
                &cleaned[0..4],
                &cleaned[4..8],
                &cleaned[8..12],
                &cleaned[12..16]
            )))
        } else if cleaned.len() == 9 {
            Ok(Self(format!(
                "{}-{}-{}",
                &cleaned[0..3],
                &cleaned[3..6],
                &cleaned[6..9]
            )))
        } else {
            Err(AdbHubError::InvalidBusId(format!(
                "Codigo de conexion invalido '{}'. Debe contener 9 digitos o 16 caracteres alfanumericos.",
                input
            )))
        }
    }
}

impl fmt::Display for AnyDeskCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AnyDeskCode {
    type Err = AdbHubError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_flexible(s)
    }
}

/// Android device authorization / state reported by ADB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdbDeviceState {
    /// Fully authorized USB debugging.
    Device,
    /// Phone is asking customer to tap "Allow USB debugging".
    Unauthorized,
    /// Recovery Mode.
    Recovery,
    /// Sideload Mode.
    Sideload,
    /// Fastboot / Bootloader.
    Bootloader,
    /// Device is offline / unresponsive.
    Offline,
    /// Unknown.
    Unknown,
}

impl fmt::Display for AdbDeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Device => write!(f, "AUTORIZADO"),
            Self::Unauthorized => write!(f, "NO AUTORIZADO (Revisar pantalla del movil)"),
            Self::Recovery => write!(f, "MODO RECOVERY"),
            Self::Sideload => write!(f, "MODO SIDELOAD"),
            Self::Bootloader => write!(f, "MODO FASTBOOT/BOOTLOADER"),
            Self::Offline => write!(f, "DESCONECTADO/OFFLINE"),
            Self::Unknown => write!(f, "DESCONOCIDO"),
        }
    }
}

/// Details of a real connected Android device queried via ADB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdbDeviceInfo {
    pub serial: String,
    pub state: AdbDeviceState,
    pub product: Option<String>,
    pub model: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
    pub android_version: Option<String>,
    pub brand: Option<String>,
    pub battery_percent: Option<u8>,
    pub security_patch: Option<String>,
}

impl AdbDeviceInfo {
    /// Formats a user-friendly display name (e.g. `Samsung Galaxy S24 Ultra (SM-S928B)`).
    pub fn display_name(&self) -> String {
        let brand = self.brand.as_deref().unwrap_or("");
        let model = self.model.as_deref().unwrap_or("");
        let product = self.product.as_deref().unwrap_or("");

        if !brand.is_empty() && !model.is_empty() {
            format!("{brand} {model}")
        } else if !model.is_empty() {
            model.to_string()
        } else if !product.is_empty() {
            product.to_string()
        } else {
            format!("Dispositivo Android ({})", self.serial)
        }
    }

    /// Returns true if this device is connected over network (TCP endpoint such as `192.168.1.83:5555`).
    pub fn is_network_device(&self) -> bool {
        AdbEngine::is_network_serial(&self.serial)
    }

    /// Returns true if this device is physically connected via USB cable.
    pub fn is_usb_device(&self) -> bool {
        !self.is_network_device()
    }
}

/// Cross-platform local ADB command runner and discovery engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct AdbEngine;

impl AdbEngine {
    pub fn new() -> Self {
        Self
    }

    /// Returns true if a serial identifier represents a remote network endpoint or virtual emulator.
    pub fn is_network_serial(serial: &str) -> bool {
        serial.contains(':') || serial.starts_with("emulator-")
    }

    /// Finds the adb executable path on Windows or Linux.
    pub fn adb_binary_name() -> &'static str {
        if cfg!(windows) {
            "adb.exe"
        } else {
            "adb"
        }
    }

    /// Creates a Command configured with the optimal path to adb (local bundled binary preferred over system PATH),
    /// ensuring correct working directory and local DLL resolution without crashing or flashing consoles.
    pub fn adb_command() -> Command {
        // 1. Locate best adb binary
        let (bin_path, working_dir) = if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let candidate = parent.join(Self::adb_binary_name());
                if candidate.exists() {
                    (candidate, Some(parent.to_path_buf()))
                } else {
                    let local = std::path::Path::new(Self::adb_binary_name());
                    if local.exists() {
                        let cur = std::env::current_dir().ok();
                        (local.to_path_buf(), cur)
                    } else {
                        (std::path::PathBuf::from(Self::adb_binary_name()), None)
                    }
                }
            } else {
                (std::path::PathBuf::from(Self::adb_binary_name()), None)
            }
        } else {
            (std::path::PathBuf::from(Self::adb_binary_name()), None)
        };

        let mut cmd = Command::new(bin_path);
        if let Some(dir) = working_dir {
            cmd.current_dir(&dir);
            // Prepend directory to PATH so Windows strictly loads local matching AdbWinApi.dll
            if let Ok(current_path) = std::env::var("PATH") {
                let sep = if cfg!(windows) { ";" } else { ":" };
                let new_path = format!("{}{}{}", dir.display(), sep, current_path);
                cmd.env("PATH", new_path);
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        cmd
    }

    /// Queries real connected ADB devices using `adb devices -l`.
    pub fn list_devices(&self) -> CoreResult<Vec<AdbDeviceInfo>> {
        let output = Self::adb_command()
            .args(["devices", "-l"])
            .output();

        let output = match output {
            Ok(o) => o,
            Err(_) => {
                // If system PATH adb is not found, try common local bundled paths
                return Ok(Vec::new());
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut devices = Vec::new();

        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("List of devices") || trimmed.starts_with("*") {
                continue;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let serial = parts[0].to_string();
            let state = match parts[1] {
                "device" => AdbDeviceState::Device,
                "unauthorized" => AdbDeviceState::Unauthorized,
                "recovery" => AdbDeviceState::Recovery,
                "sideload" => AdbDeviceState::Sideload,
                "bootloader" => AdbDeviceState::Bootloader,
                "offline" => AdbDeviceState::Offline,
                _ => AdbDeviceState::Unknown,
            };

            let mut product = None;
            let mut model = None;
            let mut device = None;
            let mut transport_id = None;

            for part in &parts[2..] {
                if let Some(val) = part.strip_prefix("product:") {
                    product = Some(val.to_string());
                } else if let Some(val) = part.strip_prefix("model:") {
                    model = Some(val.replace('_', " "));
                } else if let Some(val) = part.strip_prefix("device:") {
                    device = Some(val.to_string());
                } else if let Some(val) = part.strip_prefix("transport_id:") {
                    transport_id = Some(val.to_string());
                }
            }

            // If device is authorized, query deep properties
            let mut brand = None;
            let mut android_version = None;
            let mut security_patch = None;
            let mut battery_percent = None;

            if state == AdbDeviceState::Device {
                if let Ok(props) = self.get_device_props(&serial) {
                    if let Some(m) = props.get("ro.product.model") {
                        model = Some(m.clone());
                    }
                    brand = props.get("ro.product.brand")
                        .or_else(|| props.get("ro.product.manufacturer"))
                        .cloned();
                    android_version = props.get("ro.build.version.release")
                        .or_else(|| props.get("ro.system.build.version.release"))
                        .or_else(|| props.get("ro.product.build.version.release"))
                        .cloned();
                    security_patch = props.get("ro.build.version.security_patch").cloned();
                }
                battery_percent = self.get_battery_level(&serial).ok();
            }

            devices.push(AdbDeviceInfo {
                serial,
                state,
                product,
                model,
                device,
                transport_id,
                android_version,
                brand,
                battery_percent,
                security_patch,
            });
        }

        Ok(devices)
    }

    /// Queries system properties via `adb -s <serial> shell getprop`.
    pub fn get_device_props(&self, serial: &str) -> CoreResult<HashMap<String, String>> {
        let output = Self::adb_command()
            .args(["-s", serial, "shell", "getprop"])
            .output()
            .map_err(|e| AdbHubError::WindowsUsbError(format!("Failed to execute adb getprop: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut props = HashMap::new();

        for line in stdout.lines() {
            // Lines format: [key]: [value]
            if let Some((k, v)) = line.split_once("]: [") {
                let key = k.trim_start_matches('[').trim().to_string();
                let val = v.trim_end_matches(']').trim().to_string();
                props.insert(key, val);
            }
        }

        Ok(props)
    }

    /// Queries battery level percentage.
    pub fn get_battery_level(&self, serial: &str) -> CoreResult<u8> {
        let output = Self::adb_command()
            .args(["-s", serial, "shell", "dumpsys", "battery"])
            .output()
            .map_err(|e| AdbHubError::WindowsUsbError(format!("Failed to query battery: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(rest) = line.trim().strip_prefix("level:") {
                if let Ok(pct) = rest.trim().parse::<u8>() {
                    return Ok(pct);
                }
            }
        }
        Err(AdbHubError::WindowsUsbError("Battery level not found".to_string()))
    }

    /// Executes an interactive shell command on the connected device.
    pub fn execute_shell(&self, serial: Option<&str>, command: &str) -> CoreResult<String> {
        let mut cmd = Self::adb_command();
        if let Some(s) = serial {
            cmd.args(["-s", s]);
        }
        cmd.args(["shell", command]);

        let output = cmd
            .output()
            .map_err(|e| AdbHubError::WindowsUsbError(format!("ADB shell execution failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !stderr.is_empty() && stdout.is_empty() {
            Ok(stderr)
        } else {
            Ok(stdout)
        }
    }

    /// Reboots the device (normal, recovery, bootloader, edl).
    pub fn reboot(&self, serial: Option<&str>, target: &str) -> CoreResult<()> {
        let mut cmd = Self::adb_command();
        if let Some(s) = serial {
            cmd.args(["-s", s]);
        }
        if target.is_empty() || target == "system" {
            cmd.arg("reboot");
        } else {
            cmd.args(["reboot", target]);
        }

        cmd.output()
            .map_err(|e| AdbHubError::WindowsUsbError(format!("ADB reboot failed: {e}")))?;

        Ok(())
    }

    /// Configures the local Android device for TCP/IP ADB forwarding over port 5555.
    pub fn enable_tcpip_forward(&self, serial: Option<&str>, port: u16) -> CoreResult<()> {
        let port_str = port.to_string();
        let mut cmd = Self::adb_command();
        if let Some(s) = serial {
            cmd.args(["-s", s]);
        }
        cmd.args(["tcpip", &port_str]);
        let _ = cmd.output();

        let mut fwd_cmd = Self::adb_command();
        if let Some(s) = serial {
            fwd_cmd.args(["-s", s]);
        }
        let forward_spec = format!("tcp:{port}");
        fwd_cmd.args(["forward", &forward_spec, &forward_spec]);
        let _ = fwd_cmd.output();

        Ok(())
    }

    /// Mounts the remote bridged device onto the local technician ADB server (`adb connect <target>`).
    pub fn connect_tcp_device(&self, target_host_port: &str) -> CoreResult<String> {
        let output = Self::adb_command()
            .args(["connect", target_host_port])
            .output()
            .map_err(|e| AdbHubError::WindowsUsbError(format!("Failed to run adb connect: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(stdout)
    }

    /// Unmounts the remote bridged device from the technician ADB server (`adb disconnect <target>`).
    pub fn disconnect_tcp_device(&self, target_host_port: &str) -> CoreResult<String> {
        let output = Self::adb_command()
            .args(["disconnect", target_host_port])
            .output()
            .map_err(|e| AdbHubError::WindowsUsbError(format!("Failed to run adb disconnect: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(stdout)
    }

    /// Queries only physically connected local USB devices (excludes remote IP:port network endpoints).
    pub fn list_local_usb_devices(&self) -> CoreResult<Vec<AdbDeviceInfo>> {
        let all = self.list_devices()?;
        Ok(all.into_iter().filter(|d| !Self::is_network_serial(&d.serial)).collect())
    }

    /// Disconnects all remote network devices from the local ADB server, leaving only physical USB devices.
    pub fn disconnect_all_network_devices(&self) -> CoreResult<()> {
        let _ = Self::adb_command().arg("disconnect").output();
        Ok(())
    }

    /// Stops local ADB server process (`adb kill-server`).
    pub fn kill_server(&self) -> CoreResult<()> {
        let _ = Self::adb_command().arg("kill-server").output();
        Ok(())
    }

    /// Starts local ADB server process (`adb start-server`).
    pub fn start_server(&self) -> CoreResult<()> {
        let _ = Self::adb_command().arg("start-server").output();
        Ok(())
    }

    /// Spawns a background Cloud TCP Tunnel (`bore.pub:7835`) forwarding to `127.0.0.1:local_port`,
    /// enabling AnyDesk-style Code connections across VPNs, NAT routers, and the Internet.
    pub fn spawn_cloud_tunnel<F>(local_port: u16, on_assigned: F)
    where
        F: Fn(u16) + Send + 'static,
    {
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            use std::net::TcpStream;
            use std::time::Duration;

            loop {
                if let Ok(mut ctrl) = TcpStream::connect("bore.pub:7835") {
                    let _ = ctrl.set_read_timeout(Some(Duration::from_secs(45)));
                    if ctrl.write_all(b"{\"Hello\":0}\0").is_ok() {
                        let mut buf = Vec::new();
                        let mut byte = [0u8; 1];
                        while ctrl.read_exact(&mut byte).is_ok() {
                            if byte[0] == 0 {
                                if let Ok(line) = std::str::from_utf8(&buf) {
                                    if let Some(idx) = line.find("\"Hello\":") {
                                        let rest = &line[idx + 8..];
                                        let digits: String = rest
                                            .chars()
                                            .take_while(|c| c.is_ascii_digit())
                                            .collect();
                                        if let Ok(port) = digits.parse::<u16>() {
                                            on_assigned(port);
                                        }
                                    } else if let Some(idx) = line.find("\"Connection\":\"") {
                                        let rest = &line[idx + 14..];
                                        if let Some(end) = rest.find('"') {
                                            let uuid = rest[..end].to_string();
                                            std::thread::spawn(move || {
                                                if let (Ok(mut remote), Ok(mut local)) = (
                                                    TcpStream::connect("bore.pub:7835"),
                                                    TcpStream::connect((
                                                        "127.0.0.1",
                                                        local_port,
                                                    )),
                                                ) {
                                                    let accept_msg =
                                                        format!("{{\"Accept\":\"{uuid}\"}}\0");
                                                    if remote
                                                        .write_all(accept_msg.as_bytes())
                                                        .is_ok()
                                                    {
                                                        let mut r_clone = match remote.try_clone() {
                                                            Ok(c) => c,
                                                            Err(_) => return,
                                                        };
                                                        let mut l_clone = match local.try_clone() {
                                                            Ok(c) => c,
                                                            Err(_) => return,
                                                        };
                                                        std::thread::spawn(move || {
                                                            let _ = std::io::copy(
                                                                &mut r_clone,
                                                                &mut l_clone,
                                                            );
                                                        });
                                                        let _ = std::io::copy(
                                                            &mut local,
                                                            &mut remote,
                                                        );
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                                buf.clear();
                            } else {
                                buf.push(byte[0]);
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });
    }
}

/// High-level messages exchanged across the remote driverless ADB tunnel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RemoteAdbMessage {
    /// Technician attempts to pair using the AnyDesk code.
    ConnectRequest {
        code: AnyDeskCode,
        technician_name: String,
        app_version: String,
    },
    /// Client accepts the request with the connected device details.
    ConnectAccepted {
        device: AdbDeviceInfo,
    },
    /// Client rejects the connection.
    ConnectRejected {
        reason: String,
    },
    /// Technician requests execution of a shell command.
    ShellCommand {
        command: String,
        exec_id: u64,
    },
    /// Client returns shell output.
    ShellResult {
        exec_id: u64,
        output: String,
    },
    /// Technician requests device reboot.
    RebootRequest {
        target: String,
    },
    /// Keepalive ping.
    Ping,
    /// Keepalive pong.
    Pong,
    /// Disconnect session.
    Disconnect,
}
