//! # `adbhub-relay-service`
//!
//! Centralized QUIC Relay Server (Section 11 — Relay Centralizado).
//!
//! ### Responsibilities:
//! - Accepts 100% outbound QUIC connections from both `AgentHost` (Windows Agent) and
//!   `TechnicianDesktop` (Windows Desktop Client) so neither side requires inbound firewall rules.
//! - Verifies HMAC-SHA256 [`SignedRelayToken`] credentials and enforces anti-replay protection
//!   via a bounded nonce cache.
//! - Pairs authorized `(session_id, generation)` streams in a rendezvous table (`PendingRendezvous`).
//! - Sends [`RelayControlFrame::DataStreamPaired`] with the 312-byte [`UsbIpExportedDevice`]
//!   descriptor to both peers and transitions the stream pair into raw, zero-storage URB forwarding
//!   using [`pump_usbip_urb_session`] with mode-specific timeouts and memory backpressure.
//! - Responds to [`RelayControlFrame::Ping`] with [`RelayControlFrame::Pong`] for live RTT telemetry.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use adbhub_core::{AdbHubError, CoreResult, UsbDeviceMode};
use adbhub_relay_protocol::{
    read_control_frame, write_control_frame, RelayControlFrame, RelayPeerRole,
};
use adbhub_secure_transport::{build_quic_server_config, RelayCertificateBundle};
use adbhub_usb_exporter::{
    pump_usbip_urb_session, ExporterSessionConfig, UrbSessionMetrics, UrbTransferTracker,
};
use adbhub_usbip_adapter::UsbIpExportedDevice;
use chrono::Utc;
use quinn::{Endpoint, RecvStream, SendStream};
use tokio::io::AsyncRead;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Composite key identifying a specific USB/IP data stream pairing (`session_id` + reenumeration `generation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionGenerationKey {
    pub session_id: Uuid,
    pub generation: u32,
}

/// Combined async read/write wrapper over a bidirectional QUIC stream (`SendStream` + `RecvStream`).
pub struct QuicBiStream {
    pub send: SendStream,
    pub recv: RecvStream,
}

impl AsyncRead for QuicBiStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for QuicBiStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        tokio::io::AsyncWrite::poll_write(std::pin::Pin::new(&mut self.send), cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(std::pin::Pin::new(&mut self.send), cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(std::pin::Pin::new(&mut self.send), cx)
    }
}

struct WaitingAgentStream {
    stream: QuicBiStream,
    device_mode: UsbDeviceMode,
    exported_device: UsbIpExportedDevice,
}

enum RendezvousSlot {
    WaitingForTechnician(WaitingAgentStream),
    WaitingForAgent(oneshot::Sender<WaitingAgentStream>),
}

/// Shared state of the QUIC Relay Server.
pub struct RelayServerState {
    secret_key: Vec<u8>,
    used_nonces: Mutex<HashSet<Uuid>>,
    revoked_sessions: Mutex<HashSet<Uuid>>,
    rendezvous: Mutex<HashMap<SessionGenerationKey, RendezvousSlot>>,
    completed_metrics: Mutex<HashMap<SessionGenerationKey, UrbSessionMetrics>>,
}

impl RelayServerState {
    pub fn new(secret_key: Vec<u8>) -> Self {
        Self {
            secret_key,
            used_nonces: Mutex::new(HashSet::new()),
            revoked_sessions: Mutex::new(HashSet::new()),
            rendezvous: Mutex::new(HashMap::new()),
            completed_metrics: Mutex::new(HashMap::new()),
        }
    }

    /// Immediately marks a `session_id` as revoked so any pending or future streams are rejected.
    pub async fn revoke_session(&self, session_id: Uuid) {
        self.revoked_sessions.lock().await.insert(session_id);
        let mut r = self.rendezvous.lock().await;
        r.retain(|k, _| k.session_id != session_id);
        info!(%session_id, "Session marked as revoked on QUIC Relay");
    }

    /// Returns completed URB session metrics for `(session_id, generation)`, if available.
    pub async fn get_completed_metrics(
        &self,
        session_id: Uuid,
        generation: u32,
    ) -> Option<UrbSessionMetrics> {
        self.completed_metrics
            .lock()
            .await
            .get(&SessionGenerationKey {
                session_id,
                generation,
            })
            .cloned()
    }
}

/// Running QUIC Relay Server instance.
pub struct QuicRelayServer {
    endpoint: Endpoint,
    local_addr: SocketAddr,
    state: Arc<RelayServerState>,
}

impl QuicRelayServer {
    /// Binds a new QUIC Relay Server on `bind_addr` using the provided [`RelayCertificateBundle`]
    /// and HMAC `secret_key`.
    pub fn bind(
        bind_addr: SocketAddr,
        cert_bundle: &RelayCertificateBundle,
        secret_key: Vec<u8>,
    ) -> CoreResult<Self> {
        let server_config = build_quic_server_config(cert_bundle)?;
        let endpoint = Endpoint::server(server_config, bind_addr).map_err(|e| {
            AdbHubError::WindowsUsbError(format!("Failed to bind QUIC Relay on {bind_addr}: {e}"))
        })?;
        let local_addr = endpoint.local_addr().map_err(|e| {
            AdbHubError::WindowsUsbError(format!("Failed to query QUIC Relay local_addr: {e}"))
        })?;

        Ok(Self {
            endpoint,
            local_addr,
            state: Arc::new(RelayServerState::new(secret_key)),
        })
    }

    /// Returns the bound UDP socket address of the QUIC Relay.
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Returns a handle to the shared [`RelayServerState`].
    pub fn state(&self) -> Arc<RelayServerState> {
        Arc::clone(&self.state)
    }

    /// Accepts incoming outbound QUIC connections from Agents and Technicians until `shutdown_rx` signals.
    pub async fn run_until_cancelled(
        &self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> CoreResult<()> {
        info!(addr = %self.local_addr, "ADBHUB-LINK QUIC Relay Server listening");
        loop {
            tokio::select! {
                maybe_incoming = self.endpoint.accept() => {
                    match maybe_incoming {
                        Some(incoming) => {
                            let state = Arc::clone(&self.state);
                            tokio::spawn(async move {
                                match incoming.await {
                                    Ok(connection) => {
                                        debug!(peer = %connection.remote_address(), "Accepted QUIC connection on Relay");
                                        handle_quic_connection(connection, state).await;
                                    }
                                    Err(e) => {
                                        warn!("QUIC handshake failed: {e}");
                                    }
                                }
                            });
                        }
                        None => break,
                    }
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        info!("Shutting down QUIC Relay Server cleanly");
                        self.endpoint.close(0u32.into(), b"server shutdown");
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

async fn handle_quic_connection(connection: quinn::Connection, state: Arc<RelayServerState>) {
    loop {
        match connection.accept_bi().await {
            Ok((send, recv)) => {
                let st = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = handle_bi_stream(QuicBiStream { send, recv }, st).await {
                        warn!("QUIC Relay stream closed with error: {e}");
                    }
                });
            }
            Err(quinn::ConnectionError::ApplicationClosed { .. })
            | Err(quinn::ConnectionError::LocallyClosed) => {
                debug!("QUIC peer closed connection cleanly");
                break;
            }
            Err(e) => {
                debug!("QUIC connection terminated: {e}");
                break;
            }
        }
    }
}

async fn handle_bi_stream(
    mut stream: QuicBiStream,
    state: Arc<RelayServerState>,
) -> CoreResult<()> {
    let first_frame = tokio::time::timeout(Duration::from_secs(15), read_control_frame(&mut stream))
        .await
        .map_err(|_| {
            AdbHubError::WindowsUsbError("Timeout reading initial frame on QUIC stream".to_string())
        })??;

    match first_frame {
        RelayControlFrame::Ping {
            session_id,
            sequence,
            client_timestamp_ms,
        } => {
            let pong = RelayControlFrame::Pong {
                session_id,
                sequence,
                client_timestamp_ms,
                relay_timestamp_ms: Utc::now().timestamp_millis(),
            };
            write_control_frame(&mut stream, &pong).await?;
            Ok(())
        }

        RelayControlFrame::RevokeSession { session_id, reason } => {
            info!(%session_id, %reason, "Processing immediate session revocation on Relay");
            state.revoke_session(session_id).await;
            Ok(())
        }

        RelayControlFrame::BindDataStream {
            token,
            generation,
            device_mode,
            exported_device,
        } => {
            // 1. Verify HMAC-SHA256 signature and expiration
            let claims = token.verify_now(&state.secret_key)?.clone();

            // 2. Check session revocation list
            if state.revoked_sessions.lock().await.contains(&claims.session_id) {
                let err_frame = RelayControlFrame::Error {
                    code: "SESSION_REVOKED".to_string(),
                    message: format!("Session {} has been revoked", claims.session_id),
                };
                let _ = write_control_frame(&mut stream, &err_frame).await;
                return Err(AdbHubError::WindowsUsbError(format!(
                    "Rejected stream for revoked session {}",
                    claims.session_id
                )));
            }

            // 3. Enforce anti-replay nonce uniqueness
            {
                let mut nonces = state.used_nonces.lock().await;
                if !nonces.insert(claims.nonce) {
                    let err_frame = RelayControlFrame::Error {
                        code: "TOKEN_REPLAY_DETECTED".to_string(),
                        message: "Token nonce has already been used".to_string(),
                    };
                    let _ = write_control_frame(&mut stream, &err_frame).await;
                    return Err(AdbHubError::WindowsUsbError(format!(
                        "Replay attack prevented for nonce {}",
                        claims.nonce
                    )));
                }
            }

            let key = SessionGenerationKey {
                session_id: claims.session_id,
                generation,
            };

            match claims.role {
                RelayPeerRole::AgentHost => {
                    let exported = exported_device.ok_or_else(|| {
                        AdbHubError::MalformedDescriptor {
                            reason: "AgentHost BindDataStream must include exported_device"
                                .to_string(),
                            offset: 0,
                            available: 0,
                        }
                    })?;

                    let waiting_agent = WaitingAgentStream {
                        stream,
                        device_mode,
                        exported_device: exported,
                    };

                    let mut r = state.rendezvous.lock().await;
                    if let Some(RendezvousSlot::WaitingForAgent(tx)) = r.remove(&key) {
                        drop(r);
                        let _ = tx.send(waiting_agent);
                    } else {
                        r.insert(key, RendezvousSlot::WaitingForTechnician(waiting_agent));
                    }
                    Ok(())
                }

                RelayPeerRole::TechnicianDesktop => {
                    let waiting_agent = {
                        let mut r = state.rendezvous.lock().await;
                        if let Some(RendezvousSlot::WaitingForTechnician(agent)) = r.remove(&key) {
                            Some(agent)
                        } else {
                            let (tx, rx) = oneshot::channel();
                            r.insert(key, RendezvousSlot::WaitingForAgent(tx));
                            drop(r);
                            // Wait up to 30 seconds for the AgentHost stream to arrive
                            Some(
                                tokio::time::timeout(Duration::from_secs(30), rx)
                                    .await
                                    .map_err(|_| {
                                        AdbHubError::WindowsUsbError(format!(
                                            "Timeout waiting for AgentHost stream on session {}",
                                            claims.session_id
                                        ))
                                    })?
                                    .map_err(|_| {
                                        AdbHubError::WindowsUsbError(
                                            "Rendezvous channel closed before pairing".to_string(),
                                        )
                                    })?,
                            )
                        }
                    };

                    if let Some(mut agent) = waiting_agent {
                        let paired_frame = RelayControlFrame::DataStreamPaired {
                            session_id: claims.session_id,
                            generation,
                            device_mode: agent.device_mode,
                            exported_device: agent.exported_device.clone(),
                        };

                        // Notify both AgentHost and TechnicianDesktop that pairing is complete
                        write_control_frame(&mut agent.stream, &paired_frame).await?;
                        write_control_frame(&mut stream, &paired_frame).await?;

                        info!(
                            session_id = %claims.session_id,
                            generation,
                            bus_id = %agent.exported_device.bus_id,
                            vid_pid = %agent.exported_device.vid_pid(),
                            mode = %agent.device_mode,
                            "Paired AgentHost and TechnicianDesktop QUIC streams; starting URB relay"
                        );

                        // Pump raw USB/IP URB packets between AgentHost and TechnicianDesktop with
                        // mode-specific timeouts and memory backpressure.
                        let cfg = ExporterSessionConfig::for_mode(agent.device_mode);
                        let tracker = Arc::new(UrbTransferTracker::new(cfg));
                        let metrics =
                            pump_usbip_urb_session(agent.stream, stream, tracker).await?;

                        info!(
                            session_id = %claims.session_id,
                            generation,
                            urbs_completed = metrics.urbs_completed,
                            bytes_out = metrics.bytes_out_to_device,
                            bytes_in = metrics.bytes_in_from_device,
                            "QUIC Relay URB session completed cleanly"
                        );

                        state.completed_metrics.lock().await.insert(key, metrics);
                    }
                    Ok(())
                }
            }
        }

        other => Err(AdbHubError::MalformedDescriptor {
            reason: format!("Unexpected initial frame on QUIC Relay stream: {other:?}"),
            offset: 0,
            available: 0,
        }),
    }
}

