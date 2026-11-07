//! Typed errors for ADBHUB-LINK core operations.

use thiserror::Error;

/// Core error type across ADBHUB-LINK domain logic.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AdbHubError {
    /// Invalid USB Bus ID format (expected `<bus>-<port[.subport]>`).
    #[error("Invalid USB BusId format '{0}': expected '<bus>-<port>' (e.g. '1-4' or '2-1.3')")]
    InvalidBusId(String),

    /// Invalid VID:PID hexadecimal representation.
    #[error("Invalid VID:PID string '{0}': expected 'XXXX:YYYY' hex format")]
    InvalidVidPid(String),

    /// Binary USB descriptor parsing failed due to truncated or malformed data.
    #[error("Malformed USB binary descriptor: {reason} (offset {offset}, length {available})")]
    MalformedDescriptor {
        reason: String,
        offset: usize,
        available: usize,
    },

    /// Illegal state transition in the Device Lifecycle Manager.
    #[error("Invalid device lifecycle state transition from {from} to {to} for device {device_key}")]
    InvalidLifecycleTransition {
        device_key: String,
        from: String,
        to: String,
    },

    /// Device is already reserved or active in an exclusive technician session.
    #[error("Device {device_key} is already exclusively locked by session {session_id}")]
    DeviceBusy {
        device_key: String,
        session_id: String,
    },

    /// Attempted to disconnect or unshare a device while a critical flashing operation is active
    /// without explicit force confirmation.
    #[error("Operation blocked: device {device_key} has an active critical transfer (mode: {mode}). Disconnecting now may brick the target device.")]
    CriticalOperationGuardActive {
        device_key: String,
        mode: String,
    },

    /// Underlying Windows USB enumeration or SetupAPI error.
    #[error("Windows USB subsystem error: {0}")]
    WindowsUsbError(String),
}

/// Alias for results returning [`AdbHubError`].
pub type CoreResult<T> = Result<T, AdbHubError>;

