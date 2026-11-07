//! # `adbhub-secure-transport`
//!
//! Provides TLS 1.3 (`rustls` with `ring` crypto provider) and QUIC (`quinn`) transport
//! builders tuned specifically for USB/IP URB streaming and prolonged flashing operations
//! (Samsung Odin, Qualcomm Firehose, MediaTek DA, SPD FDL).
//!
//! ### QUIC Transport Tuning for Mobile Flashing:
//! - **Keep-Alive Interval (`5s`)**: Prevents NAT/firewall UDP state expiration while a phone's
//!   UFS/eMMC controller is erasing a large partition (which can pause USB traffic for 15–25s).
//! - **Max Idle Timeout (`90s`)**: Exceeds the maximum 60s Samsung Download Mode URB timeout.
//! - **Stream Receive Window (`16 MiB`) & Connection Window (`32 MiB`)**: High-throughput bulk
//!   pipelining over WAN links without unbounded memory growth.

use std::sync::Arc;
use std::time::Duration;

use adbhub_core::{AdbHubError, CoreResult};
use adbhub_relay_protocol::RELAY_ALPN_V1;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, IdleTimeout, ServerConfig, TransportConfig, VarInt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// Cryptographic certificate bundle (DER-encoded certificate + private key) for a QUIC Relay node.
#[derive(Debug, Clone)]
pub struct RelayCertificateBundle {
    pub cert_der: Vec<u8>,
    pub private_key_pkcs8_der: Vec<u8>,
}

/// Ensures the process-level `rustls::crypto::ring` provider is installed.
pub fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Generates a fresh X.509 certificate and private key valid for `server_names`
/// (e.g., `["localhost", "127.0.0.1", "relay.adbhub.link"]`).
pub fn generate_self_signed_cert_bundle(
    server_names: &[String],
) -> CoreResult<RelayCertificateBundle> {
    ensure_crypto_provider();
    let certified = rcgen::generate_simple_self_signed(server_names).map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Failed to generate X.509 certificate: {e}"))
    })?;

    Ok(RelayCertificateBundle {
        cert_der: certified.cert.der().to_vec(),
        private_key_pkcs8_der: certified.key_pair.serialize_der(),
    })
}

/// Builds a shared `quinn::TransportConfig` tuned for low-latency URB headers and large flashing streams.
pub fn build_flashing_transport_config() -> CoreResult<Arc<TransportConfig>> {
    let mut transport = TransportConfig::default();

    // Keep UDP NAT bindings alive every 5 seconds
    transport.keep_alive_interval(Some(Duration::from_secs(5)));

    // 90-second idle timeout to survive prolonged flash erase operations
    let idle = IdleTimeout::try_from(Duration::from_secs(90)).map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Invalid QUIC idle timeout: {e}"))
    })?;
    transport.max_idle_timeout(Some(idle));

    // Bounded flow control windows (16 MiB per stream, 32 MiB per connection)
    transport.stream_receive_window(VarInt::from_u32(16 * 1024 * 1024));
    transport.receive_window(VarInt::from_u32(32 * 1024 * 1024));
    transport.send_window(32 * 1024 * 1024);

    Ok(Arc::new(transport))
}

/// Builds a `quinn::ServerConfig` from a [`RelayCertificateBundle`].
pub fn build_quic_server_config(bundle: &RelayCertificateBundle) -> CoreResult<ServerConfig> {
    ensure_crypto_provider();

    let cert = CertificateDer::from(bundle.cert_der.clone());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        bundle.private_key_pkcs8_der.clone(),
    ));

    let mut tls_server = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| AdbHubError::WindowsUsbError(format!("Rustls ServerConfig error: {e}")))?;

    tls_server.alpn_protocols = vec![RELAY_ALPN_V1.to_vec()];

    let quic_tls = QuicServerConfig::try_from(tls_server).map_err(|e| {
        AdbHubError::WindowsUsbError(format!("QuicServerConfig conversion error: {e}"))
    })?;

    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_tls));
    server_config.transport_config(build_flashing_transport_config()?);

    Ok(server_config)
}

/// Builds a `quinn::ClientConfig` trusting the provided root/relay CA certificate (`trusted_cert_der`).
pub fn build_quic_client_config(trusted_cert_der: &[u8]) -> CoreResult<ClientConfig> {
    ensure_crypto_provider();

    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from(trusted_cert_der.to_vec()))
        .map_err(|e| AdbHubError::WindowsUsbError(format!("Failed adding root CA cert: {e}")))?;

    let mut tls_client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    tls_client.alpn_protocols = vec![RELAY_ALPN_V1.to_vec()];

    let quic_tls = QuicClientConfig::try_from(tls_client).map_err(|e| {
        AdbHubError::WindowsUsbError(format!("QuicClientConfig conversion error: {e}"))
    })?;

    let mut client_config = ClientConfig::new(Arc::new(quic_tls));
    client_config.transport_config(build_flashing_transport_config()?);

    Ok(client_config)
}

