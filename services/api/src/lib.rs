//! # `adbhub-api-service`
//!
//! Cloud Control Plane REST API and WebSocket Signaling Server built on Axum 0.8.
//!
//! Implements every endpoint and WebSocket route specified in **Section 9 (API REST)**:
//! - `POST /api/v1/auth/register`
//! - `POST /api/v1/auth/login`
//! - `POST /api/v1/auth/refresh`
//! - `POST /api/v1/agents/register`
//! - `POST /api/v1/agents/heartbeat`
//! - `GET  /api/v1/agents`
//! - `GET  /api/v1/agents/{id}/devices`
//! - `GET  /api/v1/devices`
//! - `GET  /api/v1/devices/{id}`
//! - `POST /api/v1/devices/{id}/share`
//! - `POST /api/v1/devices/{id}/unshare`
//! - `POST /api/v1/sessions`
//! - `GET  /api/v1/sessions/{id}`
//! - `POST /api/v1/sessions/{id}/approve`
//! - `POST /api/v1/sessions/{id}/connect`
//! - `POST /api/v1/sessions/{id}/disconnect`
//! - `GET  /api/v1/audit`
//! - `GET  /api/v1/health`
//! - `GET  /api/v1/usage`
//! - WebSockets: `/ws/agent`, `/ws/desktop`, `/ws/session/{id}`

use std::sync::Arc;

use adbhub_audit::{AuditEvent, AuditEventType, AuditLogService};
use adbhub_auth::{AuthService, AuthTokenClaims, TokenPairResponse};
use adbhub_core::AdbHubError;
use adbhub_device_broker::{DeviceBroker, SessionPermissions};
use adbhub_device_registry::{
    AgentRecord, DeviceRecord, DeviceRegistryService, ReportedAgentDevice,
};
use adbhub_session_manager::{SessionConnectBundle, SessionManager, SessionRecord};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

/// Real-time signaling event broadcast over `/ws/agent`, `/ws/desktop`, and `/ws/session/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SignalingEvent {
    AgentHeartbeatUpdated {
        agent_id: Uuid,
        device_count: usize,
    },
    DeviceSharedStateChanged {
        device: DeviceRecord,
    },
    SessionRequested {
        session: SessionRecord,
    },
    SessionApproved {
        session: SessionRecord,
    },
    SessionConnected {
        session_id: Uuid,
        device_id: Uuid,
        agent_id: Uuid,
    },
    SessionDisconnected {
        session: SessionRecord,
    },
}

/// Shared Control Plane state injected into Axum handlers.
pub struct ApiAppState {
    pub auth: Arc<AuthService>,
    pub registry: Arc<DeviceRegistryService>,
    pub broker: Arc<DeviceBroker>,
    pub sessions: Arc<SessionManager>,
    pub audit: Arc<AuditLogService>,
    pub signaling_tx: broadcast::Sender<SignalingEvent>,
}

impl ApiAppState {
    pub fn new(jwt_secret: Vec<u8>, relay_secret: Vec<u8>, relay_endpoint: String) -> Arc<Self> {
        let auth = Arc::new(AuthService::new(jwt_secret));
        let registry = Arc::new(DeviceRegistryService::new());
        let broker = Arc::new(DeviceBroker::new());
        let sessions = Arc::new(SessionManager::new(
            Arc::clone(&registry),
            Arc::clone(&broker),
            relay_secret,
            relay_endpoint,
        ));
        let audit = Arc::new(AuditLogService::new());
        let (signaling_tx, _) = broadcast::channel(512);

        Arc::new(Self {
            auth,
            registry,
            broker,
            sessions,
            audit,
            signaling_tx,
        })
    }

    /// Initializes the Control Plane backed entirely by local files inside `data_dir`
    /// (`users.json`, `registry.json`, `sessions.json`, `audit_events.jsonl`), requiring
    /// zero Docker containers or external database servers.
    pub fn with_local_data_dir(
        data_dir: impl AsRef<std::path::Path>,
        jwt_secret: Vec<u8>,
        relay_secret: Vec<u8>,
        relay_endpoint: String,
    ) -> adbhub_core::CoreResult<Arc<Self>> {
        let base = data_dir.as_ref();
        std::fs::create_dir_all(base).map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed creating local data directory '{}': {e}",
                base.display()
            ))
        })?;

        let auth = Arc::new(AuthService::with_file_storage(
            jwt_secret,
            base.join("users.json"),
        )?);
        let registry = Arc::new(DeviceRegistryService::with_file_storage(
            base.join("registry.json"),
        )?);
        let broker = Arc::new(DeviceBroker::new());
        let sessions = Arc::new(SessionManager::with_file_storage(
            base.join("sessions.json"),
            Arc::clone(&registry),
            Arc::clone(&broker),
            relay_secret,
            relay_endpoint,
        )?);
        let audit = Arc::new(AuditLogService::with_file_storage(
            base.join("audit_events.jsonl"),
        )?);
        let (signaling_tx, _) = broadcast::channel(512);

        Ok(Arc::new(Self {
            auth,
            registry,
            broker,
            sessions,
            audit,
            signaling_tx,
        }))
    }
}

/// Standardized JSON error envelope returned by all Control Plane endpoints.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: String,
    pub code: String,
}

fn map_core_error(err: AdbHubError) -> (StatusCode, Json<ApiErrorResponse>) {
    match &err {
        AdbHubError::DeviceBusy { .. } | AdbHubError::CriticalOperationGuardActive { .. } => (
            StatusCode::CONFLICT,
            Json(ApiErrorResponse {
                error: err.to_string(),
                code: "DEVICE_BUSY_OR_GUARDED".to_string(),
            }),
        ),
        AdbHubError::InvalidBusId(_) | AdbHubError::InvalidVidPid(_) => (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorResponse {
                error: err.to_string(),
                code: "INVALID_IDENTIFIER".to_string(),
            }),
        ),
        _ => {
            let msg = err.to_string();
            let status = if msg.contains("not found") {
                StatusCode::NOT_FOUND
            } else if msg.contains("Invalid email or password")
                || msg.contains("expired")
                || msg.contains("signature")
            {
                StatusCode::UNAUTHORIZED
            } else if msg.contains("already registered") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(ApiErrorResponse {
                    error: msg,
                    code: "CONTROL_PLANE_ERROR".to_string(),
                }),
            )
        }
    }
}

fn require_bearer_auth(
    headers: &HeaderMap,
    auth: &AuthService,
) -> Result<AuthTokenClaims, (StatusCode, Json<ApiErrorResponse>)> {
    let header_val = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiErrorResponse {
                    error: "Missing Authorization Bearer header".to_string(),
                    code: "MISSING_AUTH_HEADER".to_string(),
                }),
            )
        })?;

    let token = header_val
        .strip_prefix("Bearer ")
        .or_else(|| header_val.strip_prefix("bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiErrorResponse {
                    error: "Authorization header must use Bearer scheme".to_string(),
                    code: "INVALID_AUTH_SCHEME".to_string(),
                }),
            )
        })?;

    auth.verify_access_token(token).map_err(map_core_error)
}

/// Builds the complete Axum [`Router`] for the ADBHUB-LINK Control Plane.
pub fn build_api_router(state: Arc<ApiAppState>) -> Router {
    Router::new()
        // Auth endpoints
        .route("/api/v1/auth/register", post(handle_auth_register))
        .route("/api/v1/auth/login", post(handle_auth_login))
        .route("/api/v1/auth/refresh", post(handle_auth_refresh))
        // Agent endpoints
        .route("/api/v1/agents/register", post(handle_agent_register))
        .route("/api/v1/agents/heartbeat", post(handle_agent_heartbeat))
        .route("/api/v1/agents", get(handle_list_agents))
        .route("/api/v1/agents/{id}/devices", get(handle_list_agent_devices))
        // Device endpoints
        .route("/api/v1/devices", get(handle_list_devices))
        .route("/api/v1/devices/{id}", get(handle_get_device))
        .route("/api/v1/devices/{id}/share", post(handle_share_device))
        .route("/api/v1/devices/{id}/unshare", post(handle_unshare_device))
        // Session endpoints
        .route("/api/v1/sessions", post(handle_create_session))
        .route("/api/v1/sessions/{id}", get(handle_get_session))
        .route("/api/v1/sessions/{id}/approve", post(handle_approve_session))
        .route("/api/v1/sessions/{id}/connect", post(handle_connect_session))
        .route(
            "/api/v1/sessions/{id}/disconnect",
            post(handle_disconnect_session),
        )
        // Audit, Health, and Usage endpoints
        .route("/api/v1/audit", get(handle_get_audit))
        .route("/api/v1/health", get(handle_get_health))
        .route("/api/v1/usage", get(handle_get_usage))
        // WebSocket Signaling endpoints
        .route("/ws/agent", get(handle_ws_agent))
        .route("/ws/desktop", get(handle_ws_desktop))
        .route("/ws/session/{id}", get(handle_ws_session))
        .with_state(state)
}

// ============================================================================
// Request / Response DTOs
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthCredentialsRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshTokenRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterAgentRequest {
    pub hostname: String,
    pub platform: String,
    pub version: String,
    pub agent_public_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentHeartbeatRequest {
    pub agent_id: Uuid,
    pub devices: Vec<ReportedAgentDevice>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub device_id: Uuid,
    #[serde(default)]
    pub permissions: SessionPermissions,
    #[serde(default = "default_session_duration")]
    pub duration_minutes: i64,
}

const fn default_session_duration() -> i64 {
    60
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DisconnectSessionRequest {
    #[serde(default)]
    pub revoked: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
    pub timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageStatsResponse {
    pub registered_users: usize,
    pub registered_agents: usize,
    pub total_devices: usize,
    pub active_sessions: usize,
    pub total_sessions: usize,
    pub audit_events_count: usize,
}

// ============================================================================
// Route Handlers
// ============================================================================

async fn handle_auth_register(
    State(state): State<Arc<ApiAppState>>,
    Json(req): Json<AuthCredentialsRequest>,
) -> Result<(StatusCode, Json<TokenPairResponse>), (StatusCode, Json<ApiErrorResponse>)> {
    let tokens = state
        .auth
        .register_user(&req.email, &req.password)
        .await
        .map_err(map_core_error)?;

    state
        .audit
        .record(
            Some(tokens.user.id),
            None,
            None,
            AuditEventType::UserRegistered,
            serde_json::json!({ "email": tokens.user.email }),
        )
        .await;

    Ok((StatusCode::CREATED, Json(tokens)))
}

async fn handle_auth_login(
    State(state): State<Arc<ApiAppState>>,
    Json(req): Json<AuthCredentialsRequest>,
) -> Result<Json<TokenPairResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    let tokens = state
        .auth
        .login_user(&req.email, &req.password)
        .await
        .map_err(map_core_error)?;

    state
        .audit
        .record(
            Some(tokens.user.id),
            None,
            None,
            AuditEventType::UserLoggedIn,
            serde_json::json!({ "email": tokens.user.email }),
        )
        .await;

    Ok(Json(tokens))
}

async fn handle_auth_refresh(
    State(state): State<Arc<ApiAppState>>,
    Json(req): Json<RefreshTokenRequest>,
) -> Result<Json<TokenPairResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    let tokens = state
        .auth
        .refresh_tokens(&req.refresh_token)
        .await
        .map_err(map_core_error)?;
    Ok(Json(tokens))
}

async fn handle_agent_register(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Json(req): Json<RegisterAgentRequest>,
) -> Result<(StatusCode, Json<AgentRecord>), (StatusCode, Json<ApiErrorResponse>)> {
    let claims = require_bearer_auth(&headers, &state.auth)?;
    let agent = state
        .registry
        .register_agent(
            claims.sub,
            req.hostname,
            req.platform,
            req.version,
            req.agent_public_key,
        )
        .await;

    state
        .audit
        .record(
            Some(claims.sub),
            Some(agent.id),
            None,
            AuditEventType::AgentRegistered,
            serde_json::json!({ "hostname": agent.hostname, "version": agent.version }),
        )
        .await;

    Ok((StatusCode::CREATED, Json(agent)))
}

async fn handle_agent_heartbeat(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Json(req): Json<AgentHeartbeatRequest>,
) -> Result<Json<Vec<DeviceRecord>>, (StatusCode, Json<ApiErrorResponse>)> {
    let claims = require_bearer_auth(&headers, &state.auth)?;
    let updated_devices = state
        .registry
        .process_heartbeat(req.agent_id, req.devices)
        .await
        .map_err(map_core_error)?;

    let _ = state
        .signaling_tx
        .send(SignalingEvent::AgentHeartbeatUpdated {
            agent_id: req.agent_id,
            device_count: updated_devices.len(),
        });

    state
        .audit
        .record(
            Some(claims.sub),
            Some(req.agent_id),
            None,
            AuditEventType::AgentHeartbeat,
            serde_json::json!({ "device_count": updated_devices.len() }),
        )
        .await;

    Ok(Json(updated_devices))
}

async fn handle_list_agents(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<AgentRecord>>, (StatusCode, Json<ApiErrorResponse>)> {
    let _claims = require_bearer_auth(&headers, &state.auth)?;
    Ok(Json(state.registry.list_agents().await))
}

async fn handle_list_agent_devices(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<Vec<DeviceRecord>>, (StatusCode, Json<ApiErrorResponse>)> {
    let _claims = require_bearer_auth(&headers, &state.auth)?;
    Ok(Json(state.registry.list_agent_devices(agent_id).await))
}

async fn handle_list_devices(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<DeviceRecord>>, (StatusCode, Json<ApiErrorResponse>)> {
    let _claims = require_bearer_auth(&headers, &state.auth)?;
    Ok(Json(state.registry.list_all_devices().await))
}

async fn handle_get_device(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Path(device_id): Path<Uuid>,
) -> Result<Json<DeviceRecord>, (StatusCode, Json<ApiErrorResponse>)> {
    let _claims = require_bearer_auth(&headers, &state.auth)?;
    let dev = state
        .registry
        .get_device(device_id)
        .await
        .map_err(map_core_error)?;
    Ok(Json(dev))
}

async fn handle_share_device(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Path(device_id): Path<Uuid>,
) -> Result<Json<DeviceRecord>, (StatusCode, Json<ApiErrorResponse>)> {
    let claims = require_bearer_auth(&headers, &state.auth)?;
    let dev = state
        .registry
        .set_device_shared(device_id, true)
        .await
        .map_err(map_core_error)?;

    let _ = state
        .signaling_tx
        .send(SignalingEvent::DeviceSharedStateChanged {
            device: dev.clone(),
        });

    state
        .audit
        .record(
            Some(claims.sub),
            Some(dev.agent_id),
            None,
            AuditEventType::DeviceShared,
            serde_json::json!({ "device_id": dev.id, "busid": dev.busid.to_string(), "mode": dev.usb_mode.to_string() }),
        )
        .await;

    Ok(Json(dev))
}

async fn handle_unshare_device(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Path(device_id): Path<Uuid>,
) -> Result<Json<DeviceRecord>, (StatusCode, Json<ApiErrorResponse>)> {
    let claims = require_bearer_auth(&headers, &state.auth)?;
    let dev = state
        .registry
        .set_device_shared(device_id, false)
        .await
        .map_err(map_core_error)?;

    let _ = state
        .signaling_tx
        .send(SignalingEvent::DeviceSharedStateChanged {
            device: dev.clone(),
        });

    state
        .audit
        .record(
            Some(claims.sub),
            Some(dev.agent_id),
            None,
            AuditEventType::DeviceUnshared,
            serde_json::json!({ "device_id": dev.id, "busid": dev.busid.to_string() }),
        )
        .await;

    Ok(Json(dev))
}

async fn handle_create_session(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionRecord>), (StatusCode, Json<ApiErrorResponse>)> {
    let claims = require_bearer_auth(&headers, &state.auth)?;
    let session = state
        .sessions
        .create_session(
            req.device_id,
            claims.sub,
            req.permissions,
            req.duration_minutes,
        )
        .await
        .map_err(map_core_error)?;

    let _ = state.signaling_tx.send(SignalingEvent::SessionRequested {
        session: session.clone(),
    });

    state
        .audit
        .record(
            Some(claims.sub),
            Some(session.agent_id),
            Some(session.id),
            AuditEventType::SessionRequested,
            serde_json::json!({ "device_id": session.device_id, "busid": session.busid.to_string() }),
        )
        .await;

    Ok((StatusCode::CREATED, Json(session)))
}

async fn handle_get_session(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
) -> Result<Json<SessionRecord>, (StatusCode, Json<ApiErrorResponse>)> {
    let _claims = require_bearer_auth(&headers, &state.auth)?;
    let sess = state
        .sessions
        .get_session(session_id)
        .await
        .map_err(map_core_error)?;
    Ok(Json(sess))
}

async fn handle_approve_session(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
) -> Result<Json<SessionRecord>, (StatusCode, Json<ApiErrorResponse>)> {
    let claims = require_bearer_auth(&headers, &state.auth)?;
    let sess = state
        .sessions
        .approve_session(session_id)
        .await
        .map_err(map_core_error)?;

    let _ = state.signaling_tx.send(SignalingEvent::SessionApproved {
        session: sess.clone(),
    });

    state
        .audit
        .record(
            Some(claims.sub),
            Some(sess.agent_id),
            Some(sess.id),
            AuditEventType::SessionApproved,
            serde_json::json!({ "approved_by": claims.sub }),
        )
        .await;

    Ok(Json(sess))
}

async fn handle_connect_session(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
) -> Result<Json<SessionConnectBundle>, (StatusCode, Json<ApiErrorResponse>)> {
    let claims = require_bearer_auth(&headers, &state.auth)?;
    let bundle = state
        .sessions
        .connect_session(session_id)
        .await
        .map_err(map_core_error)?;

    let _ = state.signaling_tx.send(SignalingEvent::SessionConnected {
        session_id: bundle.session.id,
        device_id: bundle.session.device_id,
        agent_id: bundle.session.agent_id,
    });

    state
        .audit
        .record(
            Some(claims.sub),
            Some(bundle.session.agent_id),
            Some(bundle.session.id),
            AuditEventType::SessionConnected,
            serde_json::json!({ "relay_endpoint": bundle.relay_endpoint }),
        )
        .await;

    Ok(Json(bundle))
}

async fn handle_disconnect_session(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
     maybe_body: Option<Json<DisconnectSessionRequest>>,
) -> Result<Json<SessionRecord>, (StatusCode, Json<ApiErrorResponse>)> {
    let claims = require_bearer_auth(&headers, &state.auth)?;
    let revoked = maybe_body.map(|b| b.revoked).unwrap_or(false);

    let sess = state
        .sessions
        .disconnect_session(session_id, revoked)
        .await
        .map_err(map_core_error)?;

    let _ = state
        .signaling_tx
        .send(SignalingEvent::SessionDisconnected {
            session: sess.clone(),
        });

    let event_type = if revoked {
        AuditEventType::SessionRevoked
    } else {
        AuditEventType::SessionDisconnected
    };

    state
        .audit
        .record(
            Some(claims.sub),
            Some(sess.agent_id),
            Some(sess.id),
            event_type,
            serde_json::json!({ "revoked": revoked }),
        )
        .await;

    Ok(Json(sess))
}

async fn handle_get_audit(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<AuditEvent>>, (StatusCode, Json<ApiErrorResponse>)> {
    let _claims = require_bearer_auth(&headers, &state.auth)?;
    Ok(Json(state.audit.list_recent(200).await))
}

async fn handle_get_health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        service: "adbhub-link-cloud-api".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: Utc::now(),
    })
}

async fn handle_get_usage(
    State(state): State<Arc<ApiAppState>>,
    headers: HeaderMap,
) -> Result<Json<UsageStatsResponse>, (StatusCode, Json<ApiErrorResponse>)> {
    let _claims = require_bearer_auth(&headers, &state.auth)?;
    let registered_users = state.auth.user_count().await;
    let registered_agents = state.registry.list_agents().await.len();
    let total_devices = state.registry.list_all_devices().await.len();
    let active_sessions = state.sessions.active_session_count().await;
    let total_sessions = state.sessions.total_session_count().await;
    let audit_events_count = state.audit.total_count().await;

    Ok(Json(UsageStatsResponse {
        registered_users,
        registered_agents,
        total_devices,
        active_sessions,
        total_sessions,
        audit_events_count,
    }))
}

// ============================================================================
// WebSocket Signaling Handlers (/ws/agent, /ws/desktop, /ws/session/{id})
// ============================================================================

async fn handle_ws_agent(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ApiAppState>>,
) -> Response {
    let rx = state.signaling_tx.subscribe();
    ws.on_upgrade(move |socket| forward_signaling_events(socket, rx, None))
}

async fn handle_ws_desktop(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ApiAppState>>,
) -> Response {
    let rx = state.signaling_tx.subscribe();
    ws.on_upgrade(move |socket| forward_signaling_events(socket, rx, None))
}

async fn handle_ws_session(
    ws: WebSocketUpgrade,
    Path(session_id): Path<Uuid>,
    State(state): State<Arc<ApiAppState>>,
) -> Response {
    let rx = state.signaling_tx.subscribe();
    ws.on_upgrade(move |socket| forward_signaling_events(socket, rx, Some(session_id)))
}

async fn forward_signaling_events(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<SignalingEvent>,
    filter_session_id: Option<Uuid>,
) {
    while let Ok(ev) = rx.recv().await {
        if let Some(target_sid) = filter_session_id {
            let matches = match &ev {
                SignalingEvent::SessionRequested { session }
                | SignalingEvent::SessionApproved { session }
                | SignalingEvent::SessionDisconnected { session } => session.id == target_sid,
                SignalingEvent::SessionConnected { session_id, .. } => *session_id == target_sid,
                _ => false,
            };
            if !matches {
                continue;
            }
        }

        if let Ok(json_text) = serde_json::to_string(&ev) {
            if socket
                .send(Message::Text(json_text.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests;
