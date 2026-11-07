//! # `adbhub-session-manager`
//!
//! Implements the **Sessions** entity (Section 8) and the full session workflow (Section 9 & 12):
//! - Request Session (`POST /api/v1/sessions`)
//! - Approve Session (`POST /api/v1/sessions/{id}/approve`)
//! - Connect Session & Issue Dual Cryptographic Relay Tokens (`POST /api/v1/sessions/{id}/connect`)
//! - Disconnect / Revoke Session (`POST /api/v1/sessions/{id}/disconnect`)

use std::collections::HashMap;
use std::sync::Arc;

use adbhub_core::{AdbHubError, BusId, CoreResult, DeviceLifecycleState};
use adbhub_device_broker::{DeviceBroker, SessionPermissions};
use adbhub_device_registry::DeviceRegistryService;
use adbhub_relay_protocol::{RelayPeerRole, RelayTokenClaims, SignedRelayToken};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Lifecycle state of a technician session (`Sessions.state` in Section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    PendingApproval,
    Approved,
    Connecting,
    Active,
    Disconnected,
    Revoked,
    Expired,
}

/// Domain model for `Sessions` (Section 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: Uuid,
    pub device_id: Uuid,
    pub technician_id: Uuid,
    pub agent_id: Uuid,
    pub busid: BusId,
    pub state: SessionState,
    pub permissions: SessionPermissions,
    pub expires_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// Connection credentials returned by `POST /api/v1/sessions/{id}/connect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionConnectBundle {
    pub session: SessionRecord,
    pub relay_endpoint: String,
    pub agent_relay_token: SignedRelayToken,
    pub technician_relay_token: SignedRelayToken,
}

/// Manages technician USB passthrough sessions, exclusivity locks, and Relay token generation,
/// with optional local JSON file persistence (`sessions.json`).
pub struct SessionManager {
    storage_path: Option<std::path::PathBuf>,
    sessions: RwLock<HashMap<Uuid, SessionRecord>>,
    registry: Arc<DeviceRegistryService>,
    broker: Arc<DeviceBroker>,
    relay_secret: Vec<u8>,
    relay_endpoint: String,
}

impl SessionManager {
    pub fn new(
        registry: Arc<DeviceRegistryService>,
        broker: Arc<DeviceBroker>,
        relay_secret: Vec<u8>,
        relay_endpoint: String,
    ) -> Self {
        Self {
            storage_path: None,
            sessions: RwLock::new(HashMap::new()),
            registry,
            broker,
            relay_secret,
            relay_endpoint,
        }
    }

    /// Creates a [`SessionManager`] backed by a local file (`sessions.json`).
    pub fn with_file_storage(
        path: impl Into<std::path::PathBuf>,
        registry: Arc<DeviceRegistryService>,
        broker: Arc<DeviceBroker>,
        relay_secret: Vec<u8>,
        relay_endpoint: String,
    ) -> CoreResult<Self> {
        let storage_path = path.into();
        let mut sessions = HashMap::new();
        if storage_path.exists() {
            let bytes = std::fs::read(&storage_path).map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed reading local sessions file '{}': {e}",
                    storage_path.display()
                ))
            })?;
            if !bytes.is_empty() {
                let loaded: Vec<SessionRecord> = serde_json::from_slice(&bytes).map_err(|e| {
                    AdbHubError::WindowsUsbError(format!(
                        "Corrupted local sessions file '{}': {e}",
                        storage_path.display()
                    ))
                })?;
                for s in loaded {
                    sessions.insert(s.id, s);
                }
            }
        }
        Ok(Self {
            storage_path: Some(storage_path),
            sessions: RwLock::new(sessions),
            registry,
            broker,
            relay_secret,
            relay_endpoint,
        })
    }

    fn persist_sessions(&self, map: &HashMap<Uuid, SessionRecord>) -> CoreResult<()> {
        if let Some(path) = &self.storage_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AdbHubError::WindowsUsbError(format!(
                        "Failed creating sessions directory '{}': {e}",
                        parent.display()
                    ))
                })?;
            }
            let list: Vec<&SessionRecord> = map.values().collect();
            let bytes = serde_json::to_vec_pretty(&list).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed serializing sessions: {e}"))
            })?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, bytes).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed writing temp sessions file: {e}"))
            })?;
            std::fs::rename(&tmp, path).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed committing sessions file: {e}"))
            })?;
        }
        Ok(())
    }

    /// Creates a new session request (`POST /api/v1/sessions`) and immediately acquires the
    /// exclusive device lock via [`DeviceBroker`] so no second technician can hijack the device.
    pub async fn create_session(
        &self,
        device_id: Uuid,
        technician_id: Uuid,
        permissions: SessionPermissions,
        duration_minutes: i64,
    ) -> CoreResult<SessionRecord> {
        let dev = self.registry.get_device(device_id).await?;
        if !dev.online {
            return Err(AdbHubError::WindowsUsbError(format!(
                "Device {device_id} ({}) is currently offline",
                dev.busid
            )));
        }
        if !dev.shared {
            return Err(AdbHubError::WindowsUsbError(format!(
                "Device {device_id} ({}) has not been shared by the host operator",
                dev.busid
            )));
        }

        let session_id = Uuid::new_v4();

        // Enforce exclusivity per device (Acceptance Matrix: "Dos técnicos mismo dispositivo -> Segundo acceso rechazado")
        self.broker
            .acquire_lock(device_id, session_id, technician_id)
            .await?;

        let now = Utc::now();
        let record = SessionRecord {
            id: session_id,
            device_id,
            technician_id,
            agent_id: dev.agent_id,
            busid: dev.busid,
            state: SessionState::PendingApproval,
            permissions,
            expires_at: now + Duration::minutes(duration_minutes.clamp(5, 480)),
            started_at: None,
            ended_at: None,
        };

        self.registry
            .update_device_lifecycle_state(device_id, DeviceLifecycleState::Reserved)
            .await?;

        let mut guard = self.sessions.write().await;
        guard.insert(record.id, record.clone());
        self.persist_sessions(&guard)?;
        Ok(record)
    }

    /// Retrieves a session by ID (`GET /api/v1/sessions/{id}`).
    pub async fn get_session(&self, session_id: Uuid) -> CoreResult<SessionRecord> {
        self.sessions
            .read()
            .await
            .get(&session_id)
            .cloned()
            .ok_or_else(|| AdbHubError::WindowsUsbError(format!("Session {session_id} not found")))
    }

    /// Approves a pending session (`POST /api/v1/sessions/{id}/approve`).
    pub async fn approve_session(&self, session_id: Uuid) -> CoreResult<SessionRecord> {
        let mut guard = self.sessions.write().await;
        let sess = guard
            .get_mut(&session_id)
            .ok_or_else(|| AdbHubError::WindowsUsbError(format!("Session {session_id} not found")))?;

        if sess.state != SessionState::PendingApproval {
            return Err(AdbHubError::WindowsUsbError(format!(
                "Session {session_id} cannot be approved from state {:?}",
                sess.state
            )));
        }

        sess.state = SessionState::Approved;
        let out = sess.clone();
        self.persist_sessions(&guard)?;
        Ok(out)
    }

    /// Transitions an approved session to `Active` and issues ephemeral HMAC-signed Relay tokens
    /// for both `AgentHost` and `TechnicianDesktop` (`POST /api/v1/sessions/{id}/connect`).
    pub async fn connect_session(&self, session_id: Uuid) -> CoreResult<SessionConnectBundle> {
        let mut guard = self.sessions.write().await;
        let sess = guard
            .get_mut(&session_id)
            .ok_or_else(|| AdbHubError::WindowsUsbError(format!("Session {session_id} not found")))?;

        if sess.state != SessionState::Approved && sess.state != SessionState::Active {
            return Err(AdbHubError::WindowsUsbError(format!(
                "Session {session_id} must be APPROVED before connecting (current: {:?})",
                sess.state
            )));
        }

        let now = Utc::now();
        if now > sess.expires_at {
            sess.state = SessionState::Expired;
            return Err(AdbHubError::WindowsUsbError(format!(
                "Session {session_id} has expired"
            )));
        }

        sess.state = SessionState::Active;
        if sess.started_at.is_none() {
            sess.started_at = Some(now);
        }

        let updated_sess = sess.clone();
        self.persist_sessions(&guard)?;
        drop(guard);

        self.registry
            .update_device_lifecycle_state(updated_sess.device_id, DeviceLifecycleState::Active)
            .await?;

        let now_unix = now.timestamp();
        let exp_unix = updated_sess.expires_at.timestamp();

        let agent_relay_token = SignedRelayToken::issue(
            RelayTokenClaims {
                session_id: updated_sess.id,
                role: RelayPeerRole::AgentHost,
                peer_id: updated_sess.agent_id,
                bus_id: updated_sess.busid.clone(),
                nonce: Uuid::new_v4(),
                issued_at_unix: now_unix,
                expires_at_unix: exp_unix,
            },
            &self.relay_secret,
        )?;

        let technician_relay_token = SignedRelayToken::issue(
            RelayTokenClaims {
                session_id: updated_sess.id,
                role: RelayPeerRole::TechnicianDesktop,
                peer_id: updated_sess.technician_id,
                bus_id: updated_sess.busid.clone(),
                nonce: Uuid::new_v4(),
                issued_at_unix: now_unix,
                expires_at_unix: exp_unix,
            },
            &self.relay_secret,
        )?;

        Ok(SessionConnectBundle {
            session: updated_sess,
            relay_endpoint: self.relay_endpoint.clone(),
            agent_relay_token,
            technician_relay_token,
        })
    }

    /// Disconnects or revokes a session and releases the exclusive device lock (`POST /api/v1/sessions/{id}/disconnect`).
    pub async fn disconnect_session(
        &self,
        session_id: Uuid,
        revoked: bool,
    ) -> CoreResult<SessionRecord> {
        let mut guard = self.sessions.write().await;
        let sess = guard
            .get_mut(&session_id)
            .ok_or_else(|| AdbHubError::WindowsUsbError(format!("Session {session_id} not found")))?;

        sess.state = if revoked {
            SessionState::Revoked
        } else {
            SessionState::Disconnected
        };
        sess.ended_at = Some(Utc::now());
        let snapshot = sess.clone();
        self.persist_sessions(&guard)?;
        drop(guard);

        self.broker
            .release_lock(snapshot.device_id, snapshot.id)
            .await;

        let _ = self
            .registry
            .update_device_lifecycle_state(snapshot.device_id, DeviceLifecycleState::Shared)
            .await;

        Ok(snapshot)
    }

    /// Returns total active sessions count.
    pub async fn active_session_count(&self) -> usize {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.state == SessionState::Active)
            .count()
    }

    /// Returns total sessions created.
    pub async fn total_session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}
