//! # `adbhub-usb-monitor`
//!
//! Implements the **Device Lifecycle Manager** and asynchronous **USB Hotplug & Reenumeration Monitor**
//! specified in Section 10 of the ADBHUB-LINK architecture.
//!
//! ### Supported Mode Transitions & Reenumerations:
//! - Android (ADB/MTP) $\rightarrow$ Samsung Download Mode (`04E8:685D` / Odin)
//! - Android (ADB/MTP) $\rightarrow$ Qualcomm EDL (`05C6:9008`)
//! - Android (ADB/MTP) $\rightarrow$ MediaTek BROM (`0E8D:0003`) $\rightarrow$ Preloader (`0E8D:2001`)
//! - Android (ADB/MTP) $\rightarrow$ SPD / Unisoc (`1782:4D00`)
//! - Samsung Download Mode $\rightarrow$ Normal Android after Odin flash reboot

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use adbhub_core::{
    AdbHubError, BusId, CoreResult, DeviceLifecycleState, DiscoveredUsbDevice,
    ReenumerationMatchConfidence, UsbDeviceMode, VidPid,
};
use adbhub_usb_discovery::UsbDiscoveryEngine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

/// Events emitted by the [`DeviceLifecycleManager`] and [`UsbHotplugMonitor`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsbHotplugEvent {
    /// A new USB device was discovered on the host bus.
    DeviceArrived { device: DiscoveredUsbDevice },
    /// An existing device temporarily disconnected and entered `REENUMERATING` state
    /// while preserving its logical session slot for `grace_period_ms`.
    DeviceEnteringReenumeration {
        logical_id: Uuid,
        bus_id: BusId,
        previous_vid_pid: VidPid,
        previous_mode: UsbDeviceMode,
        session_id: Option<Uuid>,
        grace_period_ms: u64,
    },
    /// A reenumerating device re-appeared on the bus (potentially with a new `VID:PID` and
    /// [`UsbDeviceMode`], e.g., Android ADB -> Samsung Download Mode) and its session was recovered.
    DeviceReenumeratedAndRecovered {
        logical_id: Uuid,
        bus_id: BusId,
        previous_vid_pid: VidPid,
        new_vid_pid: VidPid,
        previous_mode: UsbDeviceMode,
        new_mode: UsbDeviceMode,
        confidence: ReenumerationMatchConfidence,
        session_id: Option<Uuid>,
        device: DiscoveredUsbDevice,
    },
    /// A device was physically unplugged and either had no active session or exceeded the
    /// reenumeration grace window; resources and session locks have been released.
    DeviceDisconnectedAndReleased {
        logical_id: Uuid,
        bus_id: BusId,
        last_vid_pid: VidPid,
        last_mode: UsbDeviceMode,
        released_session_id: Option<Uuid>,
    },
    /// Lifecycle state of a tracked device changed explicitly (e.g. `AVAILABLE -> SHARED -> ACTIVE`).
    LifecycleStateChanged {
        logical_id: Uuid,
        bus_id: BusId,
        from: DeviceLifecycleState,
        to: DeviceLifecycleState,
        session_id: Option<Uuid>,
    },
}

/// Internal tracking record managed by [`DeviceLifecycleManager`].
#[derive(Debug, Clone)]
pub struct TrackedDeviceSlot {
    pub device: DiscoveredUsbDevice,
    pub active_session_id: Option<Uuid>,
    pub active_technician_id: Option<Uuid>,
    pub reenumeration_started_at: Option<DateTime<Utc>>,
}

/// Configuration for the [`DeviceLifecycleManager`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LifecycleManagerConfig {
    /// How long to retain a logical device identity and session reservation when a device
    /// disconnects during a reboot / mode switch (default: 20,000 ms).
    pub reenumeration_grace_period: Duration,
}

impl Default for LifecycleManagerConfig {
    fn default() -> Self {
        Self {
            reenumeration_grace_period: Duration::from_secs(20),
        }
    }
}

/// Stateful manager implementing Section 10 (Gestión de Reenumeración USB) and Section 12
/// (exclusivity per device, immediate revocation, and critical operation guards).
pub struct DeviceLifecycleManager {
    config: LifecycleManagerConfig,
    /// Map keyed by topological [`BusId`].
    slots_by_bus: HashMap<BusId, TrackedDeviceSlot>,
}

impl DeviceLifecycleManager {
    /// Creates a new [`DeviceLifecycleManager`].
    pub fn new(config: LifecycleManagerConfig) -> Self {
        Self {
            config,
            slots_by_bus: HashMap::new(),
        }
    }

    /// Returns a snapshot of all currently tracked device slots.
    pub fn tracked_devices(&self) -> Vec<DiscoveredUsbDevice> {
        let mut list: Vec<DiscoveredUsbDevice> = self
            .slots_by_bus
            .values()
            .map(|s| s.device.clone())
            .collect();
        list.sort_by(|a, b| a.bus_id.cmp(&b.bus_id));
        list
    }

    /// Looks up a tracked slot by its [`BusId`].
    pub fn get_by_bus_id(&self, bus_id: &BusId) -> Option<&TrackedDeviceSlot> {
        self.slots_by_bus.get(bus_id)
    }

    /// Authorizes sharing a device (`AVAILABLE -> SHARED`).
    pub fn share_device(&mut self, bus_id: &BusId) -> CoreResult<UsbHotplugEvent> {
        let slot = self
            .slots_by_bus
            .get_mut(bus_id)
            .ok_or_else(|| AdbHubError::InvalidBusId(bus_id.to_string()))?;

        let prev = slot.device.lifecycle_state;
        let next = prev.transition_to(DeviceLifecycleState::Shared, &bus_id.to_string())?;
        slot.device.lifecycle_state = next;

        Ok(UsbHotplugEvent::LifecycleStateChanged {
            logical_id: slot.device.identity.logical_id,
            bus_id: bus_id.clone(),
            from: prev,
            to: next,
            session_id: slot.active_session_id,
        })
    }

    /// Binds an exclusive technician session to a shared device (`SHARED -> RESERVED -> CONNECTING -> ACTIVE`).
    /// Enforces the Acceptance Matrix requirement: *"Dos técnicos mismo dispositivo -> Segundo acceso rechazado"*.
    pub fn acquire_exclusive_session(
        &mut self,
        bus_id: &BusId,
        session_id: Uuid,
        technician_id: Uuid,
    ) -> CoreResult<UsbHotplugEvent> {
        let slot = self
            .slots_by_bus
            .get_mut(bus_id)
            .ok_or_else(|| AdbHubError::InvalidBusId(bus_id.to_string()))?;

        if let Some(existing_session) = slot.active_session_id {
            if existing_session != session_id {
                return Err(AdbHubError::DeviceBusy {
                    device_key: bus_id.to_string(),
                    session_id: existing_session.to_string(),
                });
            }
        }

        let prev = slot.device.lifecycle_state;
        // Validate transition through Reserved -> Active
        let reserved = prev.transition_to(DeviceLifecycleState::Reserved, &bus_id.to_string())?;
        let active = reserved.transition_to(DeviceLifecycleState::Active, &bus_id.to_string())?;

        slot.device.lifecycle_state = active;
        slot.active_session_id = Some(session_id);
        slot.active_technician_id = Some(technician_id);

        if slot.device.primary_mode().is_flashing_or_boot_mode() {
            slot.device.critical_operation_guard = true;
        }

        info!(
            bus_id = %bus_id,
            %session_id,
            %technician_id,
            mode = %slot.device.primary_mode(),
            "Exclusive USB session activated"
        );

        Ok(UsbHotplugEvent::LifecycleStateChanged {
            logical_id: slot.device.identity.logical_id,
            bus_id: bus_id.clone(),
            from: prev,
            to: active,
            session_id: Some(session_id),
        })
    }

    /// Sets or clears the critical flashing operation guard (e.g., when Odin starts or finishes flashing).
    pub fn set_critical_operation_guard(&mut self, bus_id: &BusId, active: bool) -> CoreResult<()> {
        let slot = self
            .slots_by_bus
            .get_mut(bus_id)
            .ok_or_else(|| AdbHubError::InvalidBusId(bus_id.to_string()))?;
        slot.device.critical_operation_guard = active;
        Ok(())
    }

    /// Releases or revokes an active session on a device.
    /// If `force_revoke` is `false` and `critical_operation_guard` is active (e.g. Samsung Odin flashing),
    /// returns [`AdbHubError::CriticalOperationGuardActive`] to prevent accidental bricking.
    pub fn release_session(
        &mut self,
        bus_id: &BusId,
        force_revoke: bool,
    ) -> CoreResult<UsbHotplugEvent> {
        let slot = self
            .slots_by_bus
            .get_mut(bus_id)
            .ok_or_else(|| AdbHubError::InvalidBusId(bus_id.to_string()))?;

        if slot.device.critical_operation_guard && !force_revoke {
            return Err(AdbHubError::CriticalOperationGuardActive {
                device_key: bus_id.to_string(),
                mode: slot.device.primary_mode().to_string(),
            });
        }

        let prev = slot.device.lifecycle_state;
        let released = prev.transition_to(DeviceLifecycleState::Released, &bus_id.to_string())?;
        let final_state =
            released.transition_to(DeviceLifecycleState::Shared, &bus_id.to_string())?;

        let old_session = slot.active_session_id.take();
        slot.active_technician_id = None;
        slot.device.critical_operation_guard = false;
        slot.device.lifecycle_state = final_state;

        Ok(UsbHotplugEvent::LifecycleStateChanged {
            logical_id: slot.device.identity.logical_id,
            bus_id: bus_id.clone(),
            from: prev,
            to: final_state,
            session_id: old_session,
        })
    }

    /// Reconciles a fresh scan of discovered USB devices with the current state machine,
    /// handling hotplug arrivals, mode-switch reenumerations, and grace-period expirations.
    pub fn reconcile_scan(
        &mut self,
        current_scan: Vec<DiscoveredUsbDevice>,
        now: DateTime<Utc>,
    ) -> Vec<UsbHotplugEvent> {
        let mut events = Vec::new();
        let mut seen_buses: HashMap<BusId, DiscoveredUsbDevice> = current_scan
            .into_iter()
            .map(|d| (d.bus_id.clone(), d))
            .collect();

        // 1. Inspect existing tracked slots for disappearances, mode changes, or recoveries
        let existing_keys: Vec<BusId> = self.slots_by_bus.keys().cloned().collect();

        for bus_id in existing_keys {
            if let Some(mut fresh_dev) = seen_buses.remove(&bus_id) {
                // A device is present on this BusId in the new scan
                if let Some(slot) = self.slots_by_bus.get_mut(&bus_id) {
                    let was_reenumerating =
                        slot.device.lifecycle_state == DeviceLifecycleState::Reenumerating;
                    let vid_pid_changed = slot.device.vid_pid != fresh_dev.vid_pid;
                    let mode_changed = slot.device.primary_mode() != fresh_dev.primary_mode();

                    if was_reenumerating || vid_pid_changed || mode_changed {
                        let confidence = slot.device.identity.matches_reenumeration_candidate(
                            &fresh_dev.bus_id,
                            fresh_dev.identity.location_path.as_deref(),
                            fresh_dev.serial_hash.as_deref(),
                            fresh_dev.vid_pid,
                            fresh_dev.primary_mode(),
                        );

                        if confidence.is_recoverable() {
                            let prev_vid_pid = slot.device.vid_pid;
                            let prev_mode = slot.device.primary_mode();
                            let new_vid_pid = fresh_dev.vid_pid;
                            let new_mode = fresh_dev.primary_mode();

                            // Preserve logical identity across the mode switch
                            fresh_dev.identity.logical_id = slot.device.identity.logical_id;
                            fresh_dev.identity.origin_vid_pid = slot.device.identity.origin_vid_pid;
                            fresh_dev.identity.previous_mode = Some(prev_mode);
                            fresh_dev.identity.current_mode = new_mode;
                            fresh_dev.identity.current_vid_pid = new_vid_pid;
                            fresh_dev.identity.reenumeration_count = slot
                                .device
                                .identity
                                .reenumeration_count
                                .saturating_add(1);
                            if fresh_dev.identity.last_known_serial_hash.is_none() {
                                fresh_dev.identity.last_known_serial_hash =
                                    slot.device.identity.last_known_serial_hash.clone();
                            }
                            fresh_dev.identity.updated_at = now;

                            // Restore session state (`RECOVERING -> ACTIVE` or `SHARED`)
                            let target_state = if slot.active_session_id.is_some() {
                                DeviceLifecycleState::Active
                            } else if slot.device.lifecycle_state == DeviceLifecycleState::Shared {
                                DeviceLifecycleState::Shared
                            } else {
                                DeviceLifecycleState::Available
                            };

                            fresh_dev.lifecycle_state = target_state;
                            fresh_dev.critical_operation_guard = slot.active_session_id.is_some()
                                && new_mode.is_flashing_or_boot_mode();

                            slot.reenumeration_started_at = None;
                            slot.device = fresh_dev.clone();

                            info!(
                                bus_id = %bus_id,
                                logical_id = %fresh_dev.identity.logical_id,
                                %prev_vid_pid,
                                %new_vid_pid,
                                %prev_mode,
                                %new_mode,
                                ?confidence,
                                "USB device reenumeration recovered"
                            );

                            events.push(UsbHotplugEvent::DeviceReenumeratedAndRecovered {
                                logical_id: fresh_dev.identity.logical_id,
                                bus_id,
                                previous_vid_pid: prev_vid_pid,
                                new_vid_pid,
                                previous_mode: prev_mode,
                                new_mode,
                                confidence,
                                session_id: slot.active_session_id,
                                device: fresh_dev,
                            });
                        }
                    }
                }
            } else {
                // Device is NO LONGER present on this BusId in the current scan
                let mut should_remove = false;
                if let Some(slot) = self.slots_by_bus.get_mut(&bus_id) {
                    if slot.device.lifecycle_state != DeviceLifecycleState::Reenumerating {
                        // First scan where the device disappeared: enter REENUMERATING grace period
                        slot.device.lifecycle_state = DeviceLifecycleState::Reenumerating;
                        slot.reenumeration_started_at = Some(now);
                        let grace_ms = self.config.reenumeration_grace_period.as_millis() as u64;

                        warn!(
                            bus_id = %bus_id,
                            logical_id = %slot.device.identity.logical_id,
                            vid_pid = %slot.device.vid_pid,
                            mode = %slot.device.primary_mode(),
                            grace_ms,
                            "USB device disconnected; entering REENUMERATING grace window"
                        );

                        events.push(UsbHotplugEvent::DeviceEnteringReenumeration {
                            logical_id: slot.device.identity.logical_id,
                            bus_id: bus_id.clone(),
                            previous_vid_pid: slot.device.vid_pid,
                            previous_mode: slot.device.primary_mode(),
                            session_id: slot.active_session_id,
                            grace_period_ms: grace_ms,
                        });
                    } else if let Some(started) = slot.reenumeration_started_at {
                        let elapsed = (now - started).to_std().unwrap_or(Duration::ZERO);
                        if elapsed >= self.config.reenumeration_grace_period {
                            should_remove = true;
                        }
                    }
                }

                if should_remove {
                    if let Some(removed_slot) = self.slots_by_bus.remove(&bus_id) {
                        warn!(
                            bus_id = %bus_id,
                            logical_id = %removed_slot.device.identity.logical_id,
                            "Reenumeration grace period expired; releasing device and session"
                        );
                        events.push(UsbHotplugEvent::DeviceDisconnectedAndReleased {
                            logical_id: removed_slot.device.identity.logical_id,
                            bus_id,
                            last_vid_pid: removed_slot.device.vid_pid,
                            last_mode: removed_slot.device.primary_mode(),
                            released_session_id: removed_slot.active_session_id,
                        });
                    }
                }
            }
        }

        // 2. Any remaining devices in `seen_buses` are newly arrived USB devices
        for (bus_id, mut new_dev) in seen_buses {
            // Transition Discovered -> Available
            if new_dev.lifecycle_state == DeviceLifecycleState::Discovered {
                new_dev.lifecycle_state = DeviceLifecycleState::Available;
            }
            info!(
                bus_id = %bus_id,
                vid_pid = %new_dev.vid_pid,
                mode = %new_dev.primary_mode(),
                product = %new_dev.product_name,
                "New USB device arrived"
            );
            events.push(UsbHotplugEvent::DeviceArrived {
                device: new_dev.clone(),
            });
            self.slots_by_bus.insert(
                bus_id,
                TrackedDeviceSlot {
                    device: new_dev,
                    active_session_id: None,
                    active_technician_id: None,
                    reenumeration_started_at: None,
                },
            );
        }

        events
    }
}

/// Asynchronous hotplug monitor task that periodically polls the [`UsbDiscoveryEngine`]
/// and broadcasts [`UsbHotplugEvent`] notifications to subscribers (Agent control plane, UI, etc.).
pub struct UsbHotplugMonitor {
    discovery: Arc<UsbDiscoveryEngine>,
    lifecycle: DeviceLifecycleManager,
    poll_interval: Duration,
    event_tx: broadcast::Sender<UsbHotplugEvent>,
}

impl UsbHotplugMonitor {
    /// Creates a new [`UsbHotplugMonitor`].
    pub fn new(
        discovery: Arc<UsbDiscoveryEngine>,
        config: LifecycleManagerConfig,
        poll_interval: Duration,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            discovery,
            lifecycle: DeviceLifecycleManager::new(config),
            poll_interval,
            event_tx,
        }
    }

    /// Subscribes to live [`UsbHotplugEvent`] notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<UsbHotplugEvent> {
        self.event_tx.subscribe()
    }

    /// Executes a single discovery and lifecycle reconciliation tick.
    pub fn poll_once(&mut self) -> CoreResult<Vec<UsbHotplugEvent>> {
        let discovered = self.discovery.discover_all()?;
        let events = self.lifecycle.reconcile_scan(discovered, Utc::now());
        for ev in &events {
            let _ = self.event_tx.send(ev.clone());
        }
        Ok(events)
    }

    /// Accesses the underlying [`DeviceLifecycleManager`].
    pub fn lifecycle_manager(&self) -> &DeviceLifecycleManager {
        &self.lifecycle
    }

    /// Mutable access to the underlying [`DeviceLifecycleManager`].
    pub fn lifecycle_manager_mut(&mut self) -> &mut DeviceLifecycleManager {
        &mut self.lifecycle
    }

    /// Runs the monitor loop until the provided cancellation receiver completes.
    pub async fn run_until_cancelled(
        &mut self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> CoreResult<()> {
        let mut ticker = tokio::time::interval(self.poll_interval);
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let _ = self.poll_once()?;
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        info!("USB hotplug monitor shutting down cleanly");
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adbhub_core::{UsbIpExportState, UsbSpeed};
    use adbhub_usb_discovery::build_discovered_device;
    use adbhub_usb_enumerator::RawEnumeratedUsbDevice;
    use std::str::FromStr;

    fn sample_raw_device(
        bus_id: &str,
        vid: u16,
        pid: u16,
        product: &str,
        serial: Option<&str>,
    ) -> CoreResult<DiscoveredUsbDevice> {
        let inst = match serial {
            Some(s) => format!("USB\\VID_{vid:04X}&PID_{pid:04X}\\{s}"),
            None => format!("USB\\VID_{vid:04X}&PID_{pid:04X}\\6&1A2B3C&0&4"),
        };
        let serial_hash = serial.map(adbhub_core::hash_serial_number);
        Ok(build_discovered_device(RawEnumeratedUsbDevice {
            bus_id: BusId::from_str(bus_id)?,
            vid_pid: VidPid::new(vid, pid),
            instance_id: inst,
            location_path: Some("PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(4)".to_string()),
            manufacturer: adbhub_core::vendor_name_from_id(vid).to_string(),
            product_name: product.to_string(),
            serial_hash,
            speed: UsbSpeed::High,
            device_descriptor: None,
            configuration_descriptor: None,
            interfaces: vec![],
            driver_service: Some("usbccgp".to_string()),
            export_state: UsbIpExportState::NotBound,
        }))
    }

    #[test]
    fn test_samsung_adb_to_download_mode_reenumeration_and_odin_guard() -> CoreResult<()> {
        let mut mgr = DeviceLifecycleManager::new(LifecycleManagerConfig {
            reenumeration_grace_period: Duration::from_secs(15),
        });
        let t0 = Utc::now();

        // Step 1: Samsung Galaxy S24 connected in Normal Android (ADB + MTP) on port 1-4
        let s24_adb = sample_raw_device(
            "1-4",
            0x04E8,
            0x6860,
            "Galaxy S24 ADB Interface",
            Some("R58X998877K"),
        )?;
        let ev1 = mgr.reconcile_scan(vec![s24_adb], t0);
        assert_eq!(ev1.len(), 1);
        assert!(matches!(ev1[0], UsbHotplugEvent::DeviceArrived { .. }));

        let bus_1_4 = BusId::from_str("1-4")?;
        let orig_logical_id = mgr
            .get_by_bus_id(&bus_1_4)
            .ok_or_else(|| AdbHubError::InvalidBusId("1-4".to_string()))?
            .device
            .identity
            .logical_id;

        // Host shares the device and Technician 1 opens an exclusive session
        mgr.share_device(&bus_1_4)?;
        let session_id = Uuid::new_v4();
        let tech_1 = Uuid::new_v4();
        mgr.acquire_exclusive_session(&bus_1_4, session_id, tech_1)?;

        // Acceptance Matrix check: Second technician attempting to connect to the same device is rejected!
        let tech_2 = Uuid::new_v4();
        let second_attempt = mgr.acquire_exclusive_session(&bus_1_4, Uuid::new_v4(), tech_2);
        assert!(matches!(second_attempt, Err(AdbHubError::DeviceBusy { .. })));

        // Step 2: Technician runs `adb reboot download`. Phone disconnects from USB bus at t0 + 2s.
        let t1 = t0 + chrono::Duration::seconds(2);
        let ev2 = mgr.reconcile_scan(vec![], t1);
        assert_eq!(ev2.len(), 1);
        assert!(matches!(
            ev2[0],
            UsbHotplugEvent::DeviceEnteringReenumeration {
                previous_mode: UsbDeviceMode::CompositeAdbMtp,
                ..
            }
        ));

        // Step 3: 3 seconds later (t0 + 5s), phone re-enumerates on port 1-4 in Samsung Download Mode (04E8:685D)
        let t2 = t0 + chrono::Duration::seconds(5);
        let s24_download =
            sample_raw_device("1-4", 0x04E8, 0x685D, "SAMSUNG Mobile USB Modem", None)?;
        let ev3 = mgr.reconcile_scan(vec![s24_download], t2);
        assert_eq!(ev3.len(), 1);

        match &ev3[0] {
            UsbHotplugEvent::DeviceReenumeratedAndRecovered {
                logical_id,
                previous_mode,
                new_mode,
                session_id: rec_session,
                device,
                ..
            } => {
                assert_eq!(*logical_id, orig_logical_id);
                assert_eq!(*previous_mode, UsbDeviceMode::CompositeAdbMtp);
                assert_eq!(*new_mode, UsbDeviceMode::SamsungDownload);
                assert_eq!(*rec_session, Some(session_id));
                assert_eq!(device.lifecycle_state, DeviceLifecycleState::Active);
                // Critical operation guard is automatically enabled for active Samsung Download Mode
                assert!(device.critical_operation_guard);
            }
            other => panic!("Expected DeviceReenumeratedAndRecovered, got {other:?}"),
        }

        // Step 4: Verify accidental disconnect is blocked while critical_operation_guard is active
        let blocked = mgr.release_session(&bus_1_4, false);
        assert!(matches!(
            blocked,
            Err(AdbHubError::CriticalOperationGuardActive { .. })
        ));

        // Explicit force revocation by host operator succeeds immediately
        let forced = mgr.release_session(&bus_1_4, true)?;
        assert!(matches!(
            forced,
            UsbHotplugEvent::LifecycleStateChanged {
                to: DeviceLifecycleState::Shared,
                ..
            }
        ));

        Ok(())
    }

    #[test]
    fn test_physical_unplug_expiration_releases_session() -> CoreResult<()> {
        let mut mgr = DeviceLifecycleManager::new(LifecycleManagerConfig {
            reenumeration_grace_period: Duration::from_secs(10),
        });
        let t0 = Utc::now();

        let edl_dev = sample_raw_device(
            "2-1",
            0x05C6,
            0x9008,
            "Qualcomm HS-USB QDLoader 9008",
            None,
        )?;
        mgr.reconcile_scan(vec![edl_dev], t0);

        let bus_2_1 = BusId::from_str("2-1")?;
        mgr.share_device(&bus_2_1)?;
        let sess = Uuid::new_v4();
        mgr.acquire_exclusive_session(&bus_2_1, sess, Uuid::new_v4())?;

        // Unplug at t0 + 1s -> enters REENUMERATING
        let ev_unplug = mgr.reconcile_scan(vec![], t0 + chrono::Duration::seconds(1));
        assert!(matches!(
            ev_unplug[0],
            UsbHotplugEvent::DeviceEnteringReenumeration { .. }
        ));

        // Still unplugged at t0 + 15s (exceeds 10s grace period) -> session released!
        let ev_expired = mgr.reconcile_scan(vec![], t0 + chrono::Duration::seconds(15));
        assert_eq!(ev_expired.len(), 1);
        assert!(matches!(
            ev_expired[0],
            UsbHotplugEvent::DeviceDisconnectedAndReleased {
                released_session_id: Some(s),
                last_mode: UsbDeviceMode::QualcommEdl,
                ..
            } if s == sess
        ));
        assert!(mgr.get_by_bus_id(&bus_2_1).is_none());

        Ok(())
    }
}

