//! # `adbhub-relay-client`
//!
//! Outbound QUIC Relay Client used by both:
//! - **`ADBHUB-LINK Agent` (Host PC)**: Connects outbound to the Relay, binds a session data
//!   stream with [`RelayPeerRole::AgentHost`], advertises the 312-byte [`UsbIpExportedDevice`]
//!   descriptor, and bridges URBs to/from the local `usbipd-win` stream.
//! - **`ADBHUB-LINK Desktop` (Technician PC)**: Connects outbound to the Relay, binds a session
//!   data stream with [`RelayPeerRole::TechnicianDesktop`], receives the paired
//!   [`UsbIpExportedDevice`], and mounts it on the local Windows UDE/VHCI stack via
//!   [`LocalVhciBridgeServer`].

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use adbhub_core::{AdbHubError, CoreResult, UsbDeviceMode};
use adbhub_relay_protocol::{
    read_control_frame, write_control_frame, RelayControlFrame, RelayPeerRole, SignedRelayToken,
};
use adbhub_secure_transport::build_quic_client_config;
use adbhub_usb_exporter::{
    pump_usbip_urb_session, ExporterSessionConfig, UrbSessionMetrics, UrbTransferTracker,
};
use adbhub_usbip_adapter::UsbIpExportedDevice;
use chrono::Utc;
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tracing::info;
use uuid::Uuid;

/// Bidirectional QUIC stream implementing `tokio::io::AsyncRead` and `tokio::io::AsyncWrite`.
pub struct ClientQuicBiStream {
    pub send: SendStream,
    pub recv: RecvStream,
}

impl AsyncRead for ClientQuicBiStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for ClientQuicBiStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.send), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.send), cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        AsyncWrite::poll_shutdown(Pin::new(&mut self.send), cx)
    }
}

/// Outbound QUIC Relay Client connection.
#[derive(Clone)]
pub struct RelayQuicClient {
    endpoint: Endpoint,
    connection: Connection,
}

impl RelayQuicClient {
    /// Establishes an outbound QUIC + TLS 1.3 connection to `relay_addr` (`server_name`).
    /// Binds to ephemeral UDP port `0.0.0.0:0` so no inbound firewall rules are needed.
    pub async fn connect_outbound(
        relay_addr: SocketAddr,
        server_name: &str,
        trusted_cert_der: &[u8],
    ) -> CoreResult<Self> {
        let client_cfg = build_quic_client_config(trusted_cert_der)?;
        let bind_addr: SocketAddr = if relay_addr.is_ipv4() {
            SocketAddr::from(([0, 0, 0, 0], 0))
        } else {
            SocketAddr::from(([0u16; 8], 0))
        };

        let mut endpoint = Endpoint::client(bind_addr).map_err(|e| {
            AdbHubError::WindowsUsbError(format!("Failed to bind outbound QUIC UDP socket: {e}"))
        })?;
        endpoint.set_default_client_config(client_cfg);

        let connecting = endpoint.connect(relay_addr, server_name).map_err(|e| {
            AdbHubError::WindowsUsbError(format!("QUIC connect initiation error: {e}"))
        })?;

        let connection = tokio::time::timeout(Duration::from_secs(10), connecting)
            .await
            .map_err(|_| {
                AdbHubError::WindowsUsbError(format!(
                    "Timeout establishing QUIC connection to Relay {relay_addr}"
                ))
            })?
            .map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "QUIC handshake failed with Relay {relay_addr}: {e}"
                ))
            })?;

        info!(%relay_addr, %server_name, "Outbound QUIC connection established with ADBHUB-LINK Relay");

        Ok(Self {
            endpoint,
            connection,
        })
    }

    /// Measures application-level RTT latency to the QUIC Relay using a `Ping` / `Pong` control exchange.
    pub async fn measure_rtt_latency(&self, session_id: Uuid, sequence: u64) -> CoreResult<Duration> {
        let (send, recv) = self.connection.open_bi().await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!("Failed opening QUIC stream for Ping: {e}"))
        })?;
        let mut stream = ClientQuicBiStream { send, recv };

        let t0 = Instant::now();
        let ping = RelayControlFrame::Ping {
            session_id,
            sequence,
            client_timestamp_ms: Utc::now().timestamp_millis(),
        };
        write_control_frame(&mut stream, &ping).await?;
        let reply = read_control_frame(&mut stream).await?;

        match reply {
            RelayControlFrame::Pong {
                sequence: seq_out, ..
            } if seq_out == sequence => Ok(t0.elapsed()),
            other => Err(AdbHubError::MalformedDescriptor {
                reason: format!("Expected Pong for sequence {sequence}, got {other:?}"),
                offset: 0,
                available: 0,
            }),
        }
    }

    /// Sends an immediate session revocation command (`RevokeSession`) to the Relay.
    pub async fn send_session_revocation(&self, session_id: Uuid, reason: &str) -> CoreResult<()> {
        let (send, recv) = self.connection.open_bi().await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!("Failed opening QUIC stream for Revoke: {e}"))
        })?;
        let mut stream = ClientQuicBiStream { send, recv };
        let frame = RelayControlFrame::RevokeSession {
            session_id,
            reason: reason.to_string(),
        };
        write_control_frame(&mut stream, &frame).await
    }

    /// Opens an **AgentHost** data stream on the Relay for `(token, generation)`, waits until the
    /// TechnicianDesktop stream pairs, and then bridges all USB/IP URB traffic to/from `local_host_usbip_stream`.
    pub async fn bridge_agent_host_session<H>(
        &self,
        token: SignedRelayToken,
        generation: u32,
        device_mode: UsbDeviceMode,
        exported_device: UsbIpExportedDevice,
        local_host_usbip_stream: H,
    ) -> CoreResult<UrbSessionMetrics>
    where
        H: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        if token.claims.role != RelayPeerRole::AgentHost {
            return Err(AdbHubError::WindowsUsbError(
                "bridge_agent_host_session requires an AgentHost token".to_string(),
            ));
        }

        let (send, recv) = self.connection.open_bi().await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!("Failed opening AgentHost QUIC data stream: {e}"))
        })?;
        let mut quic_stream = ClientQuicBiStream { send, recv };

        let bind_frame = RelayControlFrame::BindDataStream {
            token,
            generation,
            device_mode,
            exported_device: Some(exported_device),
        };
        write_control_frame(&mut quic_stream, &bind_frame).await?;

        // Wait for DataStreamPaired from the Relay
        let response = read_control_frame(&mut quic_stream).await?;
        match response {
            RelayControlFrame::DataStreamPaired {
                session_id,
                generation: paired_gen,
                ..
            } => {
                info!(
                    %session_id,
                    paired_gen,
                    %device_mode,
                    "AgentHost QUIC stream paired with Technician; starting URB forwarding"
                );
                let cfg = ExporterSessionConfig::for_mode(device_mode);
                let tracker = Arc::new(UrbTransferTracker::new(cfg));
                pump_usbip_urb_session(local_host_usbip_stream, quic_stream, tracker).await
            }
            RelayControlFrame::Error { code, message } => Err(AdbHubError::WindowsUsbError(
                format!("Relay rejected AgentHost stream [{code}]: {message}"),
            )),
            other => Err(AdbHubError::MalformedDescriptor {
                reason: format!("Unexpected Relay response to AgentHost bind: {other:?}"),
                offset: 0,
                available: 0,
            }),
        }
    }

    /// Opens a **TechnicianDesktop** data stream on the Relay for `(token, generation)`, waits
    /// for `DataStreamPaired`, and returns the upgraded [`ClientQuicBiStream`] together with the
    /// remote [`UsbIpExportedDevice`] descriptor and [`UsbDeviceMode`] so it can be passed directly
    /// to [`adbhub_usbip_client_adapter::LocalVhciBridgeServer::accept_and_bridge_session`].
    pub async fn open_technician_data_stream(
        &self,
        token: SignedRelayToken,
        generation: u32,
        expected_mode: UsbDeviceMode,
    ) -> CoreResult<(ClientQuicBiStream, UsbIpExportedDevice, UsbDeviceMode)> {
        if token.claims.role != RelayPeerRole::TechnicianDesktop {
            return Err(AdbHubError::WindowsUsbError(
                "open_technician_data_stream requires a TechnicianDesktop token".to_string(),
            ));
        }

        let (send, recv) = self.connection.open_bi().await.map_err(|e| {
            AdbHubError::WindowsUsbError(format!(
                "Failed opening TechnicianDesktop QUIC data stream: {e}"
            ))
        })?;
        let mut quic_stream = ClientQuicBiStream { send, recv };

        let bind_frame = RelayControlFrame::BindDataStream {
            token,
            generation,
            device_mode: expected_mode,
            exported_device: None,
        };
        write_control_frame(&mut quic_stream, &bind_frame).await?;

        let response = read_control_frame(&mut quic_stream).await?;
        match response {
            RelayControlFrame::DataStreamPaired {
                session_id,
                generation: paired_gen,
                device_mode,
                exported_device,
            } => {
                info!(
                    %session_id,
                    paired_gen,
                    bus_id = %exported_device.bus_id,
                    vid_pid = %exported_device.vid_pid(),
                    %device_mode,
                    "TechnicianDesktop QUIC stream paired with remote AgentHost"
                );
                Ok((quic_stream, exported_device, device_mode))
            }
            RelayControlFrame::Error { code, message } => Err(AdbHubError::WindowsUsbError(
                format!("Relay rejected TechnicianDesktop stream [{code}]: {message}"),
            )),
            other => Err(AdbHubError::MalformedDescriptor {
                reason: format!("Unexpected Relay response to TechnicianDesktop bind: {other:?}"),
                offset: 0,
                available: 0,
            }),
        }
    }

    /// Cleanly closes the QUIC connection and waits for background sockets to drain.
    pub fn close(&self) {
        self.connection.close(0u32.into(), b"client closed");
        self.endpoint.close(0u32.into(), b"endpoint closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adbhub_core::{BusId, UsbSpeed};
    use adbhub_relay_protocol::RelayTokenClaims;
    use adbhub_relay_service::QuicRelayServer;
    use adbhub_secure_transport::generate_self_signed_cert_bundle;
    use adbhub_usbip_adapter::{
        connect_and_import_from_addr, UsbIpDirection, UsbIpHeaderBasic, UsbIpUrbHeader,
        USBIP_CMD_SUBMIT, USBIP_RET_SUBMIT, USBIP_URB_HEADER_SIZE, USBIP_URB_ZERO_PACKET,
    };
    use adbhub_usbip_client_adapter::LocalVhciBridgeServer;
    use std::str::FromStr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::watch;

    #[tokio::test]
    async fn test_e2e_internet_quic_relay_usbip_passthrough_and_security() -> CoreResult<()> {
        let relay_secret = b"adbhub-test-hmac-secret-key-2026".to_vec();
        let cert_bundle = generate_self_signed_cert_bundle(&[
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])?;

        // 1. Start the Centralized QUIC Relay Server on 127.0.0.1:0
        let relay_server = QuicRelayServer::bind(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            &cert_bundle,
            relay_secret.clone(),
        )?;
        let relay_addr = relay_server.local_addr();
        let relay_state = relay_server.state();

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let relay_task = tokio::spawn(async move {
            relay_server.run_until_cancelled(shutdown_rx).await
        });

        // 2. Connect AgentHost and TechnicianDesktop outbound via QUIC + TLS 1.3
        let agent_client =
            RelayQuicClient::connect_outbound(relay_addr, "localhost", &cert_bundle.cert_der)
                .await?;
        let tech_client =
            RelayQuicClient::connect_outbound(relay_addr, "localhost", &cert_bundle.cert_der)
                .await?;

        // 3. Measure RTT Latency over QUIC Ping/Pong
        let session_id = Uuid::new_v4();
        let rtt = tech_client.measure_rtt_latency(session_id, 1).await?;
        assert!(rtt < Duration::from_secs(2));

        // 4. Issue cryptographic HMAC tokens for AgentHost and TechnicianDesktop
        let bus_1_4 = BusId::from_str("1-4")?;
        let now_unix = Utc::now().timestamp();

        let agent_token = SignedRelayToken::issue(
            RelayTokenClaims {
                session_id,
                role: RelayPeerRole::AgentHost,
                peer_id: Uuid::new_v4(),
                bus_id: bus_1_4.clone(),
                nonce: Uuid::new_v4(),
                issued_at_unix: now_unix,
                expires_at_unix: now_unix + 600,
            },
            &relay_secret,
        )?;

        let tech_token = SignedRelayToken::issue(
            RelayTokenClaims {
                session_id,
                role: RelayPeerRole::TechnicianDesktop,
                peer_id: Uuid::new_v4(),
                bus_id: bus_1_4.clone(),
                nonce: Uuid::new_v4(),
                issued_at_unix: now_unix,
                expires_at_unix: now_unix + 600,
            },
            &relay_secret,
        )?;

        let exported_samsung = UsbIpExportedDevice {
            path: r"USB\VID_04E8&PID_685D\6&11223344&0&4".to_string(),
            bus_id: bus_1_4.clone(),
            busnum: 1,
            devnum: 4,
            speed: UsbSpeed::High,
            id_vendor: 0x04E8,
            id_product: 0x685D,
            bcd_device: 0x0400,
            b_device_class: 0x02,
            b_device_subclass: 0x00,
            b_device_protocol: 0x00,
            b_configuration_value: 1,
            b_num_configurations: 1,
            b_num_interfaces: 2,
            interfaces: Vec::new(),
        };

        // 5. Simulate the physical Samsung device on the AgentHost side (`usbipd-win` stream)
        let (mut host_physical_usb, agent_local_stream) = tokio::io::duplex(64 * 1024);

        let agent_client_clone = agent_client.clone();
        let agent_token_clone = agent_token.clone();
        let exported_clone = exported_samsung.clone();

        let agent_bridge_task = tokio::spawn(async move {
            agent_client_clone
                .bridge_agent_host_session(
                    agent_token_clone,
                    0,
                    UsbDeviceMode::SamsungDownload,
                    exported_clone,
                    agent_local_stream,
                )
                .await
        });

        // 6. Technician opens the QUIC data stream and feeds it into LocalVhciBridgeServer
        let (tech_quic_stream, paired_exported_dev, paired_mode) = tech_client
            .open_technician_data_stream(tech_token.clone(), 0, UsbDeviceMode::SamsungDownload)
            .await?;

        assert_eq!(paired_exported_dev, exported_samsung);
        assert_eq!(paired_mode, UsbDeviceMode::SamsungDownload);

        let vhci_bridge = LocalVhciBridgeServer::bind_ephemeral().await?;
        let vhci_addr = vhci_bridge.local_addr();

        let vhci_bridge_task = tokio::spawn(async move {
            vhci_bridge
                .accept_and_bridge_session(paired_exported_dev, paired_mode, tech_quic_stream)
                .await
        });

        // 7. Spawn simulated physical device responder on Host PC
        let physical_device_task = tokio::spawn(async move {
            let mut hdr_buf = [0u8; USBIP_URB_HEADER_SIZE];
            // Read "ODIN" Bulk OUT
            host_physical_usb
                .read_exact(&mut hdr_buf)
                .await
                .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;
            let cmd = UsbIpUrbHeader::from_wire_bytes(&hdr_buf)?;
            let mut body = [0u8; 4];
            host_physical_usb
                .read_exact(&mut body)
                .await
                .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;
            assert_eq!(&body, b"ODIN");

            // Reply with "LOKE"
            let ret = UsbIpUrbHeader::RetSubmit {
                basic: UsbIpHeaderBasic {
                    command: USBIP_RET_SUBMIT,
                    seqnum: cmd.basic().seqnum,
                    devid: cmd.basic().devid,
                    direction: UsbIpDirection::In,
                    ep: 1,
                },
                status: 0,
                actual_length: 4,
                start_frame: 0,
                number_of_packets: 0,
                error_count: 0,
            };
            host_physical_usb
                .write_all(&ret.to_wire_bytes())
                .await
                .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;
            host_physical_usb
                .write_all(b"LOKE")
                .await
                .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;
            Ok::<(), AdbHubError>(())
        });

        // 8. Technician's local VHCI driver (`usbip-win2`) attaches to `vhci_addr` and exchanges URBs over QUIC!
        let (mut vhci_driver_socket, attached_dev) =
            connect_and_import_from_addr(vhci_addr, &bus_1_4, Duration::from_secs(5)).await?;
        assert_eq!(attached_dev, exported_samsung);

        let submit_odin = UsbIpUrbHeader::CmdSubmit {
            basic: UsbIpHeaderBasic {
                command: USBIP_CMD_SUBMIT,
                seqnum: 77,
                devid: 0x0001_0004,
                direction: UsbIpDirection::Out,
                ep: 1,
            },
            transfer_flags: USBIP_URB_ZERO_PACKET,
            transfer_buffer_length: 4,
            start_frame: 0,
            number_of_packets: 0,
            interval: 0,
            setup: [0; 8],
        };
        vhci_driver_socket
            .write_all(&submit_odin.to_wire_bytes())
            .await
            .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;
        vhci_driver_socket
            .write_all(b"ODIN")
            .await
            .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;

        let mut ret_hdr_buf = [0u8; USBIP_URB_HEADER_SIZE];
        vhci_driver_socket
            .read_exact(&mut ret_hdr_buf)
            .await
            .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;
        let ret_hdr = UsbIpUrbHeader::from_wire_bytes(&ret_hdr_buf)?;
        assert_eq!(ret_hdr.basic().seqnum, 77);
        assert_eq!(ret_hdr.following_payload_len(), 4);

        let mut loke_reply = [0u8; 4];
        vhci_driver_socket
            .read_exact(&mut loke_reply)
            .await
            .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))?;
        assert_eq!(&loke_reply, b"LOKE");

        // Close VHCI socket and verify clean end-to-end metrics across Agent, Relay, and Technician
        drop(vhci_driver_socket);
        physical_device_task
            .await
            .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))??;
        let (tech_metrics, _) = vhci_bridge_task
            .await
            .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))??;
        let agent_metrics = agent_bridge_task
            .await
            .map_err(|e| AdbHubError::WindowsUsbError(e.to_string()))??;

        assert_eq!(tech_metrics.urbs_submitted, 1);
        assert_eq!(tech_metrics.urbs_completed, 1);
        assert_eq!(agent_metrics.urbs_submitted, 1);
        assert_eq!(agent_metrics.urbs_completed, 1);

        // 9. Verify Replay Protection: Re-using `tech_token` (same nonce) MUST be rejected by the Relay!
        let replay_attempt = tech_client
            .open_technician_data_stream(tech_token, 0, UsbDeviceMode::SamsungDownload)
            .await;
        assert!(replay_attempt.is_err());

        // 10. Verify Immediate Session Revocation
        agent_client
            .send_session_revocation(session_id, "Host operator revoked session")
            .await?;

        // Wait briefly for Relay to process revocation
        tokio::time::sleep(Duration::from_millis(50)).await;
        let new_token_for_revoked_session = SignedRelayToken::issue(
            RelayTokenClaims {
                session_id,
                role: RelayPeerRole::TechnicianDesktop,
                peer_id: Uuid::new_v4(),
                bus_id: bus_1_4,
                nonce: Uuid::new_v4(), // Fresh nonce, but session_id is revoked!
                issued_at_unix: now_unix,
                expires_at_unix: now_unix + 600,
            },
            &relay_secret,
        )?;
        let revoked_attempt = tech_client
            .open_technician_data_stream(
                new_token_for_revoked_session,
                1,
                UsbDeviceMode::SamsungDownload,
            )
            .await;
        assert!(revoked_attempt.is_err());

        // Verify Relay recorded session metrics
        let relay_recorded = relay_state.get_completed_metrics(session_id, 0).await;
        assert!(relay_recorded.is_some());

        agent_client.close();
        tech_client.close();
        let _ = shutdown_tx.send(true);
        let _ = relay_task.await;

        Ok(())
    }
}

