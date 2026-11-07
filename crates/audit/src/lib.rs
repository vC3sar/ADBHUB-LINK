//! # `adbhub-audit`
//!
//! Implements the **Audit Events** entity (Section 8) and privacy-preserving audit trail (Section 12):
//! *"No almacenar payloads USB ni información privada transferida por los teléfonos."*

use std::io::Write;
use std::path::PathBuf;

use adbhub_core::{AdbHubError, CoreResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

/// Strongly-typed audit event categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuditEventType {
    UserRegistered,
    UserLoggedIn,
    AgentRegistered,
    AgentHeartbeat,
    DeviceShared,
    DeviceUnshared,
    DeviceModeReenumerated,
    SessionRequested,
    SessionApproved,
    SessionConnected,
    SessionDisconnected,
    SessionRevoked,
}

/// Domain model for `Audit Events` (Section 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub event_type: AuditEventType,
    pub timestamp: DateTime<Utc>,
    /// Safe metadata JSON (never contains raw USB payloads, serial numbers, or credentials).
    pub metadata: serde_json::Value,
}

/// Thread-safe Audit Log Service with optional local JSON-Lines (`audit_events.jsonl`) file persistence.
#[derive(Default)]
pub struct AuditLogService {
    storage_path: Option<PathBuf>,
    events: RwLock<Vec<AuditEvent>>,
}

impl AuditLogService {
    pub fn new() -> Self {
        Self {
            storage_path: None,
            events: RwLock::new(Vec::new()),
        }
    }

    /// Creates an [`AuditLogService`] backed by an append-only local JSONL file (`audit_events.jsonl`).
    pub fn with_file_storage(path: impl Into<PathBuf>) -> CoreResult<Self> {
        let storage_path = path.into();
        let mut events = Vec::new();
        if storage_path.exists() {
            let content = std::fs::read_to_string(&storage_path).map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed reading audit file '{}': {e}",
                    storage_path.display()
                ))
            })?;
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Ok(ev) = serde_json::from_str::<AuditEvent>(trimmed) {
                        events.push(ev);
                    }
                }
            }
        }
        Ok(Self {
            storage_path: Some(storage_path),
            events: RwLock::new(events),
        })
    }

    /// Appends a new [`AuditEvent`] to memory and to the local `.jsonl` file if configured.
    pub async fn record(
        &self,
        user_id: Option<Uuid>,
        agent_id: Option<Uuid>,
        session_id: Option<Uuid>,
        event_type: AuditEventType,
        metadata: serde_json::Value,
    ) -> AuditEvent {
        let ev = AuditEvent {
            id: Uuid::new_v4(),
            user_id,
            agent_id,
            session_id,
            event_type,
            timestamp: Utc::now(),
            metadata,
        };

        info!(
            audit_id = %ev.id,
            ?event_type,
            ?user_id,
            ?agent_id,
            ?session_id,
            "Recorded audit event"
        );

        let mut guard = self.events.write().await;
        guard.push(ev.clone());

        if let Some(path) = &self.storage_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                if let Ok(line) = serde_json::to_string(&ev) {
                    let _ = writeln!(file, "{line}");
                }
            }
        }

        ev
    }

    /// Returns the most recent audit events (newest first).
    pub async fn list_recent(&self, limit: usize) -> Vec<AuditEvent> {
        let guard = self.events.read().await;
        guard.iter().rev().take(limit).cloned().collect()
    }

    /// Returns total recorded audit events.
    pub async fn total_count(&self) -> usize {
        self.events.read().await.len()
    }
}
