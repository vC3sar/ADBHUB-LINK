//! # `adbhub-relay-protocol`
//!
//! Defines the wire protocol, temporary cryptographic session tokens, anti-replay nonces,
//! heartbeats, and stream bindings for the **ADBHUB-LINK Centralized QUIC Relay** (Section 11 & 12).
//!
//! ### Key Security & Reliability Properties:
//! - **100% Outbound QUIC**: Both the Host Windows Agent and Technician Windows Desktop initiate
//!   outbound QUIC connections to the Relay (`ALPN = "adbhub-relay-v1"`). No inbound ports are
//!   required on customer or technician firewalls.
//! - **Cryptographic Ephemeral Session Tokens**: Every session stream is authenticated with an
//!   HMAC-SHA256 signed [`SignedRelayToken`] bound to `(session_id, peer_role, peer_id, bus_id, nonce, expires_at_unix)`.
//! - **Replay Protection**: Each token carries a unique 128-bit `nonce` (`Uuid::new_v4()`) and strict
//!   expiration timestamp verified by the Relay before pairing any data stream.
//! - **Zero Payload Inspection / Storage**: Once the `DataStreamBind` handshake completes on a
//!   bidirectional QUIC stream, raw USB/IP URB frames flow directly between the paired Agent and
//!   Technician streams with backpressure and zero disk/database storage.

use adbhub_core::{AdbHubError, BusId, CoreResult, UsbDeviceMode, VidPid};
use adbhub_usbip_adapter::UsbIpExportedDevice;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// ALPN protocol identifier negotiated by all ADBHUB-LINK QUIC connections.
pub const RELAY_ALPN_V1: &[u8] = b"adbhub-relay-v1";

/// Maximum size of a serialized control/handshake frame (`64 KiB`).
pub const MAX_CONTROL_FRAME_BYTES: usize = 64 * 1024;

/// Role of the endpoint connecting to the QUIC Relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelayPeerRole {
    /// Host Windows PC where the physical phone is plugged in (`adbhub-agent`).
    AgentHost,
    /// Remote Technician Windows PC running repair tools (`adbhub-desktop`).
    TechnicianDesktop,
}

/// Claims embedded inside an ephemeral Relay Session Token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayTokenClaims {
    /// Authorized session ID from the Control Plane.
    pub session_id: Uuid,
    /// Role allowed by this token (`AgentHost` or `TechnicianDesktop`).
    pub role: RelayPeerRole,
    /// Authenticated Agent ID or Technician User ID.
    pub peer_id: Uuid,
    /// Authorized physical USB `BusId` on the host.
    pub bus_id: BusId,
    /// Unique 128-bit anti-replay nonce.
    pub nonce: Uuid,
    /// Token issuance timestamp (Unix seconds UTC).
    pub issued_at_unix: i64,
    /// Token expiration timestamp (Unix seconds UTC).
    pub expires_at_unix: i64,
}

/// Cryptographically signed token authorizing a single QUIC control or data stream on the Relay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedRelayToken {
    pub claims: RelayTokenClaims,
    /// Hex-encoded HMAC-SHA256 signature over the canonical JSON serialization of `claims`.
    pub signature_hex: String,
}

impl SignedRelayToken {
    /// Issues and signs a new [`SignedRelayToken`] using the shared relay secret key.
    pub fn issue(claims: RelayTokenClaims, secret_key: &[u8]) -> CoreResult<Self> {
        let canonical = serde_json::to_vec(&claims)
            .map_err(|e| AdbHubError::WindowsUsbError(format!("Token serialize error: {e}")))?;

        let mut mac = HmacSha256::new_from_slice(secret_key)
            .map_err(|e| AdbHubError::WindowsUsbError(format!("HMAC key error: {e}")))?;
        mac.update(&canonical);
        let signature_hex = hex::encode(mac.finalize().into_bytes());

        Ok(Self {
            claims,
            signature_hex,
        })
    }

    /// Verifies the cryptographic signature and expiration of this token at `now_unix`.
    pub fn verify(&self, secret_key: &[u8], now_unix: i64) -> CoreResult<&RelayTokenClaims> {
        if now_unix > self.claims.expires_at_unix {
            return Err(AdbHubError::WindowsUsbError(format!(
                "Relay token expired for session {} (expired_at={}, now={})",
                self.claims.session_id, self.claims.expires_at_unix, now_unix
            )));
        }

        let canonical = serde_json::to_vec(&self.claims)
            .map_err(|e| AdbHubError::WindowsUsbError(format!("Token serialize error: {e}")))?;

        let sig_bytes = hex::decode(&self.signature_hex).map_err(|_| {
            AdbHubError::WindowsUsbError("Malformed hex signature on Relay token".to_string())
        })?;

        let mut mac = HmacSha256::new_from_slice(secret_key)
            .map_err(|e| AdbHubError::WindowsUsbError(format!("HMAC key error: {e}")))?;
        mac.update(&canonical);
        mac.verify_slice(&sig_bytes).map_err(|_| {
            AdbHubError::WindowsUsbError(
                "Invalid cryptographic HMAC signature on Relay token".to_string(),
            )
        })?;

        Ok(&self.claims)
    }

    /// Convenience helper verifying against `Utc::now().timestamp()`.
    pub fn verify_now(&self, secret_key: &[u8]) -> CoreResult<&RelayTokenClaims> {
        self.verify(secret_key, Utc::now().timestamp())
    }
}

/// Control & Signaling messages exchanged over QUIC control streams or during initial
/// data-stream binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayControlFrame {
    /// Sent as the first message on a dedicated QUIC **Data Stream** to bind it to an authorized
    /// USB/IP session (`session_id`, `generation`).
    BindDataStream {
        token: SignedRelayToken,
        /// Reenumeration generation counter (`0` for initial attach, `1+` for re-attachments after mode switches).
        generation: u32,
        /// Current USB mode of the device.
        device_mode: UsbDeviceMode,
        /// Provided by `AgentHost` so the Relay can immediately deliver the 312-byte device record
        /// to `TechnicianDesktop` upon pairing.
        exported_device: Option<UsbIpExportedDevice>,
    },

    /// Sent by the Relay to both peers once the `AgentHost` and `TechnicianDesktop` data streams
    /// for `(session_id, generation)` have been paired and raw URB forwarding begins.
    DataStreamPaired {
        session_id: Uuid,
        generation: u32,
        device_mode: UsbDeviceMode,
        exported_device: UsbIpExportedDevice,
    },

    /// Sent on the persistent QUIC **Session Control Stream** to measure RTT latency and keep
    /// NAT bindings alive during prolonged UFS erase / Odin flashing operations.
    Ping {
        session_id: Uuid,
        sequence: u64,
        client_timestamp_ms: i64,
    },

    /// Response to [`RelayControlFrame::Ping`].
    Pong {
        session_id: Uuid,
        sequence: u64,
        client_timestamp_ms: i64,
        relay_timestamp_ms: i64,
    },

    /// Sent by `AgentHost` on the control stream when the physical device enters `REENUMERATING`
    /// (e.g. `adb reboot download` or MediaTek BROM -> DA jump) so the Technician side prepares
    /// for generation `next_generation` without tearing down the session.
    SessionReenumerating {
        session_id: Uuid,
        next_generation: u32,
        previous_vid_pid: VidPid,
        previous_mode: UsbDeviceMode,
        grace_period_ms: u64,
    },

    /// Sent by `AgentHost` when the device finishes reenumerating on the same physical port.
    SessionReenumerationReady {
        session_id: Uuid,
        generation: u32,
        new_vid_pid: VidPid,
        new_mode: UsbDeviceMode,
        exported_device: UsbIpExportedDevice,
    },

    /// Immediate revocation or clean session termination.
    RevokeSession {
        session_id: Uuid,
        reason: String,
    },

    /// Error response from the Relay before closing a stream.
    Error {
        code: String,
        message: String,
    },
}

/// Writes a length-prefixed (`u32` Big-Endian + JSON payload) [`RelayControlFrame`] to an async stream.
pub async fn write_control_frame<W>(writer: &mut W, frame: &RelayControlFrame) -> CoreResult<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(frame).map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed to serialize RelayControlFrame: {e}"))
    })?;
    if payload.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(AdbHubError::MalformedDescriptor {
            reason: format!(
                "RelayControlFrame size {} exceeds MAX_CONTROL_FRAME_BYTES ({MAX_CONTROL_FRAME_BYTES})",
                payload.len()
            ),
            offset: 0,
            available: payload.len(),
        });
    }

    let len_be = (payload.len() as u32).to_be_bytes();
    writer.write_all(&len_be).await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed writing RelayControlFrame length: {e}"))
    })?;
    writer.write_all(&payload).await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed writing RelayControlFrame payload: {e}"))
    })?;
    writer.flush().await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed flushing RelayControlFrame: {e}"))
    })?;
    Ok(())
}

/// Reads a length-prefixed (`u32` Big-Endian + JSON payload) [`RelayControlFrame`] from an async stream.
pub async fn read_control_frame<R>(reader: &mut R) -> CoreResult<RelayControlFrame>
where
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed reading RelayControlFrame length: {e}"))
    })?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_CONTROL_FRAME_BYTES {
        return Err(AdbHubError::MalformedDescriptor {
            reason: format!("Invalid RelayControlFrame length {len}"),
            offset: 0,
            available: 4,
        });
    }

    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await.map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed reading RelayControlFrame payload: {e}"))
    })?;

    serde_json::from_slice(&payload).map_err(|e| {
        AdbHubError::MalformedDescriptor {
            reason: format!("Invalid RelayControlFrame JSON: {e}"),
            offset: 4,
            available: len,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_relay_token_issue_verify_and_expiration() -> CoreResult<()> {
        let secret = b"super-secret-relay-hmac-key-2026";
        let claims = RelayTokenClaims {
            session_id: Uuid::new_v4(),
            role: RelayPeerRole::AgentHost,
            peer_id: Uuid::new_v4(),
            bus_id: BusId::from_str("1-4")?,
            nonce: Uuid::new_v4(),
            issued_at_unix: 1_700_000_000,
            expires_at_unix: 1_700_000_600,
        };

        let token = SignedRelayToken::issue(claims.clone(), secret)?;
        let verified = token.verify(secret, 1_700_000_100)?;
        assert_eq!(verified, &claims);

        // Expired timestamp must be rejected
        assert!(token.verify(secret, 1_700_000_601).is_err());

        // Wrong secret key must be rejected
        assert!(token.verify(b"wrong-key", 1_700_000_100).is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_control_frame_wire_roundtrip() -> CoreResult<()> {
        let (mut tx, mut rx) = tokio::io::duplex(4096);
        let ping = RelayControlFrame::Ping {
            session_id: Uuid::new_v4(),
            sequence: 7,
            client_timestamp_ms: 123_456_789,
        };

        write_control_frame(&mut tx, &ping).await?;
        let decoded = read_control_frame(&mut rx).await?;
        assert_eq!(decoded, ping);
        Ok(())
    }
}

