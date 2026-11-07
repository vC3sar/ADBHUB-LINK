//! # `adbhub-device-broker`
//!
//! Enforces strict per-device exclusivity across technician sessions (Section 12 & Section 15:
//! *"Exclusividad por dispositivo"* / *"Dos técnicos mismo dispositivo -> Segundo acceso rechazado"*).

use std::collections::HashMap;

use adbhub_core::{AdbHubError, CoreResult};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Granular permissions granted for a USB passthrough session (`Sessions.permissions` in Section 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPermissions {
    /// Allowed to perform standard ADB and MTP transfers.
    pub allow_adb_mtp: bool,
    /// Allowed to trigger or maintain reenumeration across boot/download modes (`adb reboot download`, EDL, BROM).
    pub allow_mode_transitions: bool,
    /// Explicitly authorized to perform firmware flashing operations (Samsung Odin, Qualcomm Firehose, MTK DA, SPD).
    pub allow_firmware_flashing: bool,
}

impl Default for SessionPermissions {
    fn default() -> Self {
        Self {
            allow_adb_mtp: true,
            allow_mode_transitions: true,
            allow_firmware_flashing: true,
        }
    }
}

/// Active exclusive lock record on a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceLockEntry {
    pub device_id: Uuid,
    pub session_id: Uuid,
    pub technician_id: Uuid,
}

/// Broker coordinating exclusive device access locks.
#[derive(Default)]
pub struct DeviceBroker {
    locks_by_device: RwLock<HashMap<Uuid, DeviceLockEntry>>,
}

impl DeviceBroker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquires an exclusive lock on `device_id` for `(session_id, technician_id)`.
    /// If another session already holds the lock on `device_id`, returns [`AdbHubError::DeviceBusy`].
    pub async fn acquire_lock(
        &self,
        device_id: Uuid,
        session_id: Uuid,
        technician_id: Uuid,
    ) -> CoreResult<DeviceLockEntry> {
        let mut guard = self.locks_by_device.write().await;
        if let Some(existing) = guard.get(&device_id) {
            if existing.session_id != session_id {
                return Err(AdbHubError::DeviceBusy {
                    device_key: device_id.to_string(),
                    session_id: existing.session_id.to_string(),
                });
            }
            return Ok(*existing);
        }

        let entry = DeviceLockEntry {
            device_id,
            session_id,
            technician_id,
        };
        guard.insert(device_id, entry);
        Ok(entry)
    }

    /// Releases the lock on `device_id` if held by `session_id`.
    pub async fn release_lock(&self, device_id: Uuid, session_id: Uuid) -> bool {
        let mut guard = self.locks_by_device.write().await;
        if let Some(existing) = guard.get(&device_id) {
            if existing.session_id == session_id {
                guard.remove(&device_id);
                return true;
            }
        }
        false
    }

    /// Checks whether `device_id` is currently locked by an active session.
    pub async fn current_lock(&self, device_id: Uuid) -> Option<DeviceLockEntry> {
        self.locks_by_device.read().await.get(&device_id).copied()
    }
}

