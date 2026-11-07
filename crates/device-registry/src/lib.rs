//! # `adbhub-device-registry`
//!
//! Implements the **Agents** and **Devices** domain entities and inventory registry (Section 8 & 9).

use std::collections::HashMap;

use adbhub_core::{
    classify_usb_device, AdbHubError, BusId, CoreResult, DeviceLifecycleState,
    InterfaceDescriptor, OdinCompatibilityStatus, UsbDeviceMode, VidPid,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Domain model for `Agents` (Section 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub hostname: String,
    pub platform: String,
    pub version: String,
    pub online: bool,
    pub last_seen: DateTime<Utc>,
    pub agent_public_key: String,
}

/// Domain model for `Devices` (Section 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub busid: BusId,
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: String,
    pub product_name: String,
    /// SHA-256 hashed serial string (privacy-preserving; never raw PII).
    pub serial_hash: Option<String>,
    pub interface_descriptors: Vec<InterfaceDescriptor>,
    pub device_state: DeviceLifecycleState,
    pub usb_mode: UsbDeviceMode,
    pub odin_compatibility: OdinCompatibilityStatus,
    pub online: bool,
    pub shared: bool,
    pub updated_at: DateTime<Utc>,
}

/// Payload sent by an Agent when registering or sending a heartbeat (`POST /api/v1/agents/heartbeat`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportedAgentDevice {
    pub busid: BusId,
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: String,
    pub product_name: String,
    pub serial_hash: Option<String>,
    #[serde(default)]
    pub interface_descriptors: Vec<InterfaceDescriptor>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RegistryFileSnapshot {
    agents: Vec<AgentRecord>,
    devices: Vec<DeviceRecord>,
}

/// Registry managing all connected Agents and their physical USB Devices with optional local file persistence.
#[derive(Default)]
pub struct DeviceRegistryService {
    storage_path: Option<std::path::PathBuf>,
    agents: RwLock<HashMap<Uuid, AgentRecord>>,
    devices: RwLock<HashMap<Uuid, DeviceRecord>>,
}

impl DeviceRegistryService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a [`DeviceRegistryService`] backed by a local JSON file (`registry.json`).
    pub fn with_file_storage(path: impl Into<std::path::PathBuf>) -> CoreResult<Self> {
        let storage_path = path.into();
        let mut agents = HashMap::new();
        let mut devices = HashMap::new();

        if storage_path.exists() {
            let bytes = std::fs::read(&storage_path).map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed reading local registry file '{}': {e}",
                    storage_path.display()
                ))
            })?;
            if !bytes.is_empty() {
                let snap: RegistryFileSnapshot = serde_json::from_slice(&bytes).map_err(|e| {
                    AdbHubError::WindowsUsbError(format!(
                        "Corrupted local registry file '{}': {e}",
                        storage_path.display()
                    ))
                })?;
                for a in snap.agents {
                    agents.insert(a.id, a);
                }
                for d in snap.devices {
                    devices.insert(d.id, d);
                }
            }
        }

        Ok(Self {
            storage_path: Some(storage_path),
            agents: RwLock::new(agents),
            devices: RwLock::new(devices),
        })
    }

    async fn flush_to_disk(&self) -> CoreResult<()> {
        if let Some(path) = &self.storage_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AdbHubError::WindowsUsbError(format!(
                        "Failed creating registry directory '{}': {e}",
                        parent.display()
                    ))
                })?;
            }
            let snap = RegistryFileSnapshot {
                agents: self.agents.read().await.values().cloned().collect(),
                devices: self.devices.read().await.values().cloned().collect(),
            };
            let bytes = serde_json::to_vec_pretty(&snap).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed serializing registry: {e}"))
            })?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, bytes).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed writing temp registry file: {e}"))
            })?;
            std::fs::rename(&tmp, path).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed committing registry file: {e}"))
            })?;
        }
        Ok(())
    }

    /// Registers a new host Windows Agent (`POST /api/v1/agents/register`).
    pub async fn register_agent(
        &self,
        owner_id: Uuid,
        hostname: String,
        platform: String,
        version: String,
        agent_public_key: String,
    ) -> AgentRecord {
        let record = AgentRecord {
            id: Uuid::new_v4(),
            owner_id,
            hostname,
            platform,
            version,
            online: true,
            last_seen: Utc::now(),
            agent_public_key,
        };
        self.agents.write().await.insert(record.id, record.clone());
        let _ = self.flush_to_disk().await;
        record
    }

    /// Processes an Agent heartbeat (`POST /api/v1/agents/heartbeat`) and synchronizes its reported
    /// USB devices, preserving logical `DeviceRecord.id` across mode reenumerations on the same `(agent_id, busid)`.
    pub async fn process_heartbeat(
        &self,
        agent_id: Uuid,
        reported_devices: Vec<ReportedAgentDevice>,
    ) -> CoreResult<Vec<DeviceRecord>> {
        let now = Utc::now();
        {
            let mut agents = self.agents.write().await;
            let agent = agents.get_mut(&agent_id).ok_or_else(|| {
                AdbHubError::WindowsUsbError(format!("Agent {agent_id} not found"))
            })?;
            agent.online = true;
            agent.last_seen = now;
        }

        let mut devices_guard = self.devices.write().await;

        // Index existing devices for this agent by BusId so reenumerations on the same port keep the same Device UUID
        let mut existing_by_bus: HashMap<BusId, Uuid> = devices_guard
            .values()
            .filter(|d| d.agent_id == agent_id)
            .map(|d| (d.busid.clone(), d.id))
            .collect();

        let mut updated_records = Vec::with_capacity(reported_devices.len());

        for rep in reported_devices {
            let vid_pid = VidPid::new(rep.vendor_id, rep.product_id);
            let classification = classify_usb_device(
                vid_pid,
                None,
                &rep.interface_descriptors,
                Some(&rep.product_name),
                None,
            );
            let odin_compat = classification
                .samsung_download_info
                .map(|s| s.odin_compatibility)
                .unwrap_or(OdinCompatibilityStatus::Unknown);

            if let Some(existing_id) = existing_by_bus.remove(&rep.busid) {
                if let Some(dev) = devices_guard.get_mut(&existing_id) {
                    dev.vendor_id = rep.vendor_id;
                    dev.product_id = rep.product_id;
                    dev.manufacturer = rep.manufacturer;
                    dev.product_name = rep.product_name;
                    if rep.serial_hash.is_some() {
                        dev.serial_hash = rep.serial_hash;
                    }
                    dev.interface_descriptors = rep.interface_descriptors;
                    dev.usb_mode = classification.primary_mode;
                    dev.odin_compatibility = odin_compat;
                    dev.online = true;
                    if dev.device_state == DeviceLifecycleState::Disconnected {
                        dev.device_state = if dev.shared {
                            DeviceLifecycleState::Shared
                        } else {
                            DeviceLifecycleState::Available
                        };
                    }
                    dev.updated_at = now;
                    updated_records.push(dev.clone());
                }
            } else {
                let new_dev = DeviceRecord {
                    id: Uuid::new_v4(),
                    agent_id,
                    busid: rep.busid,
                    vendor_id: rep.vendor_id,
                    product_id: rep.product_id,
                    manufacturer: rep.manufacturer,
                    product_name: rep.product_name,
                    serial_hash: rep.serial_hash,
                    interface_descriptors: rep.interface_descriptors,
                    device_state: DeviceLifecycleState::Available,
                    usb_mode: classification.primary_mode,
                    odin_compatibility: odin_compat,
                    online: true,
                    shared: false,
                    updated_at: now,
                };
                devices_guard.insert(new_dev.id, new_dev.clone());
                updated_records.push(new_dev);
            }
        }

        // Any previously known devices on this agent that were not in the heartbeat are marked offline/disconnected
        for (_bus, missing_id) in existing_by_bus {
            if let Some(dev) = devices_guard.get_mut(&missing_id) {
                dev.online = false;
                dev.device_state = DeviceLifecycleState::Disconnected;
                dev.updated_at = now;
            }
        }

        drop(devices_guard);
        self.flush_to_disk().await?;

        Ok(updated_records)
    }

    /// Lists all registered Agents (`GET /api/v1/agents`).
    pub async fn list_agents(&self) -> Vec<AgentRecord> {
        self.agents.read().await.values().cloned().collect()
    }

    /// Lists all devices belonging to a specific Agent (`GET /api/v1/agents/{id}/devices`).
    pub async fn list_agent_devices(&self, agent_id: Uuid) -> Vec<DeviceRecord> {
        self.devices
            .read()
            .await
            .values()
            .filter(|d| d.agent_id == agent_id)
            .cloned()
            .collect()
    }

    /// Lists all devices across all agents (`GET /api/v1/devices`).
    pub async fn list_all_devices(&self) -> Vec<DeviceRecord> {
        self.devices.read().await.values().cloned().collect()
    }

    /// Gets a single device by ID (`GET /api/v1/devices/{id}`).
    pub async fn get_device(&self, device_id: Uuid) -> CoreResult<DeviceRecord> {
        self.devices
            .read()
            .await
            .get(&device_id)
            .cloned()
            .ok_or_else(|| AdbHubError::WindowsUsbError(format!("Device {device_id} not found")))
    }

    /// Sets the `shared` flag on a device (`POST /api/v1/devices/{id}/share` or `/unshare`).
    pub async fn set_device_shared(
        &self,
        device_id: Uuid,
        shared: bool,
    ) -> CoreResult<DeviceRecord> {
        let mut guard = self.devices.write().await;
        let dev = guard
            .get_mut(&device_id)
            .ok_or_else(|| AdbHubError::WindowsUsbError(format!("Device {device_id} not found")))?;

        if !shared && dev.device_state == DeviceLifecycleState::Active {
            return Err(AdbHubError::DeviceBusy {
                device_key: dev.busid.to_string(),
                session_id: "ACTIVE_SESSION".to_string(),
            });
        }

        dev.shared = shared;
        dev.device_state = if shared {
            DeviceLifecycleState::Shared
        } else {
            DeviceLifecycleState::Available
        };
        dev.updated_at = Utc::now();
        let out = dev.clone();
        drop(guard);
        self.flush_to_disk().await?;
        Ok(out)
    }

    /// Updates a device's [`DeviceLifecycleState`].
    pub async fn update_device_lifecycle_state(
        &self,
        device_id: Uuid,
        new_state: DeviceLifecycleState,
    ) -> CoreResult<DeviceRecord> {
        let mut guard = self.devices.write().await;
        let dev = guard
            .get_mut(&device_id)
            .ok_or_else(|| AdbHubError::WindowsUsbError(format!("Device {device_id} not found")))?;
        dev.device_state = new_state;
        dev.updated_at = Utc::now();
        let out = dev.clone();
        drop(guard);
        self.flush_to_disk().await?;
        Ok(out)
    }
}
