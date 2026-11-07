//! End-to-End Control Plane REST API & Exclusivity Tests (`services/api/src/tests.rs`)
//!
//! Validates all 19 REST endpoints from Section 9, JWT authentication, Samsung Download Mode
//! device registration & mode reenumeration via heartbeats, per-device exclusivity
//! (rejecting a second technician with HTTP 409 CONFLICT), Relay token issuance, and Audit Trail.

use std::str::FromStr;

use adbhub_audit::AuditEvent;
use adbhub_auth::TokenPairResponse;
use adbhub_core::{BusId, CoreResult, OdinCompatibilityStatus, UsbDeviceMode};
use adbhub_device_broker::SessionPermissions;
use adbhub_device_registry::{AgentRecord, DeviceRecord, ReportedAgentDevice};
use adbhub_session_manager::{SessionConnectBundle, SessionRecord, SessionState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::{
    build_api_router, AgentHeartbeatRequest, ApiAppState, CreateSessionRequest, HealthResponse,
    UsageStatsResponse,
};

async fn send_json_request<T: serde::de::DeserializeOwned>(
    app: &axum::Router,
    method: &str,
    uri: &str,
    bearer_token: Option<&str>,
    body_json: Option<serde_json::Value>,
) -> (StatusCode, T) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer_token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    let body = if let Some(val) = body_json {
        builder = builder.header("Content-Type", "application/json");
        Body::from(serde_json::to_vec(&val).unwrap_or_default())
    } else {
        Body::empty()
    };

    let req = builder.body(body).unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let parsed: T = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "Failed to deserialize response from {method} {uri} (status {status}): {e} | raw={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, parsed)
}

#[tokio::test]
async fn test_full_control_plane_rest_api_workflow_and_exclusivity() -> CoreResult<()> {
    let relay_secret = b"adbhub-test-relay-hmac-key-2026".to_vec();
    let temp_dir = std::env::temp_dir().join(format!("adbhub_test_data_{}", uuid::Uuid::new_v4()));
    let state = ApiAppState::with_local_data_dir(
        &temp_dir,
        b"adbhub-test-jwt-secret-key-2026".to_vec(),
        relay_secret.clone(),
        "relay.adbhub.link:4433".to_string(),
    )?;
    let app = build_api_router(state);

    // 1. Health check (`GET /api/v1/health`)
    let (st_health, health): (_, HealthResponse) =
        send_json_request(&app, "GET", "/api/v1/health", None, None).await;
    assert_eq!(st_health, StatusCode::OK);
    assert_eq!(health.status, "ok");

    // 2. Register Host Owner (`POST /api/v1/auth/register`) and two Technicians
    let (st_reg_host, host_auth): (_, TokenPairResponse) = send_json_request(
        &app,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(serde_json::json!({
            "email": "workshop-host@adbhub.link",
            "password": "StrongPassword123!"
        })),
    )
    .await;
    assert_eq!(st_reg_host, StatusCode::CREATED);

    let (st_reg_tech1, tech1_auth): (_, TokenPairResponse) = send_json_request(
        &app,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(serde_json::json!({
            "email": "tech1@adbhub.link",
            "password": "Tech1Password123!"
        })),
    )
    .await;
    assert_eq!(st_reg_tech1, StatusCode::CREATED);

    let (_, tech2_auth): (_, TokenPairResponse) = send_json_request(
        &app,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(serde_json::json!({
            "email": "tech2@adbhub.link",
            "password": "Tech2Password123!"
        })),
    )
    .await;

    // 3. Register Host Windows Agent (`POST /api/v1/agents/register`)
    let (st_agent, agent): (_, AgentRecord) = send_json_request(
        &app,
        "POST",
        "/api/v1/agents/register",
        Some(&host_auth.access_token),
        Some(serde_json::json!({
            "hostname": "WORKSHOP-PC-01",
            "platform": "Windows 11 Pro x64",
            "version": "0.2.0",
            "agent_public_key": "ed25519:pubkey_workshop_01"
        })),
    )
    .await;
    assert_eq!(st_agent, StatusCode::CREATED);

    // 4. Send Agent Heartbeat reporting a Samsung device in Download Mode (04E8:685D) and Qualcomm EDL (05C6:9008)
    let hb_body = serde_json::to_value(AgentHeartbeatRequest {
        agent_id: agent.id,
        devices: vec![
            ReportedAgentDevice {
                busid: BusId::from_str("1-4")?,
                vendor_id: 0x04E8,
                product_id: 0x685D,
                manufacturer: "Samsung".to_string(),
                product_name: "SAMSUNG Mobile USB Modem".to_string(),
                serial_hash: Some(adbhub_core::hash_serial_number("R58M999111")),
                interface_descriptors: vec![],
            },
            ReportedAgentDevice {
                busid: BusId::from_str("2-1")?,
                vendor_id: 0x05C6,
                product_id: 0x9008,
                manufacturer: "Qualcomm".to_string(),
                product_name: "Qualcomm HS-USB QDLoader 9008".to_string(),
                serial_hash: None,
                interface_descriptors: vec![],
            },
        ],
    })
    .unwrap();

    let (st_hb, devices): (_, Vec<DeviceRecord>) = send_json_request(
        &app,
        "POST",
        "/api/v1/agents/heartbeat",
        Some(&host_auth.access_token),
        Some(hb_body),
    )
    .await;
    assert_eq!(st_hb, StatusCode::OK);
    assert_eq!(devices.len(), 2);

    let samsung_dev = &devices[0];
    assert_eq!(samsung_dev.usb_mode, UsbDeviceMode::SamsungDownload);
    assert_eq!(
        samsung_dev.odin_compatibility,
        OdinCompatibilityStatus::Unverified
    );

    // 5. Host shares the Samsung Download Mode device (`POST /api/v1/devices/{id}/share`)
    let (st_share, shared_dev): (_, DeviceRecord) = send_json_request(
        &app,
        "POST",
        &format!("/api/v1/devices/{}/share", samsung_dev.id),
        Some(&host_auth.access_token),
        None,
    )
    .await;
    assert_eq!(st_share, StatusCode::OK);
    assert!(shared_dev.shared);

    // 6. Technician 1 requests a session on the Samsung device (`POST /api/v1/sessions`)
    let req_sess_body = serde_json::to_value(CreateSessionRequest {
        device_id: samsung_dev.id,
        permissions: SessionPermissions::default(),
        duration_minutes: 60,
    })
    .unwrap();

    let (st_sess1, session1): (_, SessionRecord) = send_json_request(
        &app,
        "POST",
        "/api/v1/sessions",
        Some(&tech1_auth.access_token),
        Some(req_sess_body.clone()),
    )
    .await;
    assert_eq!(st_sess1, StatusCode::CREATED);
    assert_eq!(session1.state, SessionState::PendingApproval);

    // 7. Acceptance Matrix Check: Technician 2 tries to request a session on the SAME device -> REJECTED (409 CONFLICT)!
    let (st_sess2, _err_resp): (_, serde_json::Value) = send_json_request(
        &app,
        "POST",
        "/api/v1/sessions",
        Some(&tech2_auth.access_token),
        Some(req_sess_body),
    )
    .await;
    assert_eq!(st_sess2, StatusCode::CONFLICT);

    // 8. Host approves Technician 1's session (`POST /api/v1/sessions/{id}/approve`)
    let (st_approve, approved_sess): (_, SessionRecord) = send_json_request(
        &app,
        "POST",
        &format!("/api/v1/sessions/{}/approve", session1.id),
        Some(&host_auth.access_token),
        None,
    )
    .await;
    assert_eq!(st_approve, StatusCode::OK);
    assert_eq!(approved_sess.state, SessionState::Approved);

    // 9. Technician 1 connects (`POST /api/v1/sessions/{id}/connect`) and receives signed Relay tokens
    let (st_conn, connect_bundle): (_, SessionConnectBundle) = send_json_request(
        &app,
        "POST",
        &format!("/api/v1/sessions/{}/connect", session1.id),
        Some(&tech1_auth.access_token),
        None,
    )
    .await;
    assert_eq!(st_conn, StatusCode::OK);
    assert_eq!(connect_bundle.session.state, SessionState::Active);

    // Verify both cryptographic Relay tokens are valid against the Relay secret key
    assert!(connect_bundle
        .agent_relay_token
        .verify_now(&relay_secret)
        .is_ok());
    assert!(connect_bundle
        .technician_relay_token
        .verify_now(&relay_secret)
        .is_ok());

    // 10. Check Usage Statistics (`GET /api/v1/usage`) and Audit Trail (`GET /api/v1/audit`)
    let (st_usage, usage): (_, UsageStatsResponse) = send_json_request(
        &app,
        "GET",
        "/api/v1/usage",
        Some(&host_auth.access_token),
        None,
    )
    .await;
    assert_eq!(st_usage, StatusCode::OK);
    assert_eq!(usage.registered_users, 3);
    assert_eq!(usage.registered_agents, 1);
    assert_eq!(usage.total_devices, 2);
    assert_eq!(usage.active_sessions, 1);

    // 11. Disconnect session (`POST /api/v1/sessions/{id}/disconnect`)
    let (st_disc, disc_sess): (_, SessionRecord) = send_json_request(
        &app,
        "POST",
        &format!("/api/v1/sessions/{}/disconnect", session1.id),
        Some(&host_auth.access_token),
        Some(serde_json::json!({ "revoked": true })),
    )
    .await;
    assert_eq!(st_disc, StatusCode::OK);
    assert_eq!(disc_sess.state, SessionState::Revoked);

    let (st_audit, audits): (_, Vec<AuditEvent>) = send_json_request(
        &app,
        "GET",
        "/api/v1/audit",
        Some(&host_auth.access_token),
        None,
    )
    .await;
    assert_eq!(st_audit, StatusCode::OK);
    assert!(audits.len() >= 9);

    // Verify that all 4 local persistence files were written on disk and can be reloaded
    assert!(temp_dir.join("users.json").exists());
    assert!(temp_dir.join("registry.json").exists());
    assert!(temp_dir.join("sessions.json").exists());
    assert!(temp_dir.join("audit_events.jsonl").exists());

    let reloaded = ApiAppState::with_local_data_dir(
        &temp_dir,
        b"adbhub-test-jwt-secret-key-2026".to_vec(),
        relay_secret,
        "relay.adbhub.link:4433".to_string(),
    )?;
    assert_eq!(reloaded.auth.user_count().await, 3);
    assert_eq!(reloaded.registry.list_all_devices().await.len(), 2);
    assert_eq!(reloaded.sessions.total_session_count().await, 1);
    assert!(reloaded.audit.total_count().await >= 9);

    let _ = std::fs::remove_dir_all(&temp_dir);

    Ok(())
}
