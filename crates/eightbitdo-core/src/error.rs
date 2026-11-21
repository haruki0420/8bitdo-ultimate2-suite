//! Error type shared by the transport and session layers.

/// Errors returned by `eightbitdo-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The protocol layer rejected a builder or a frame.
    #[error("protocol error: {0}")]
    Protocol(#[from] eightbitdo_protocol::Error),

    /// The transport failed for a reason the higher layer cannot classify.
    #[error("transport error: {0}")]
    Transport(String),

    /// The BLE backend failed.
    #[cfg(feature = "ble")]
    #[error("BLE error: {0}")]
    Ble(String),

    /// No matching peripheral was found during the scan.
    #[error("device not found: name={name}, address={address:?}")]
    DeviceNotFound {
        /// Advertised name that was requested.
        name: String,
        /// Address that was requested, if any.
        address: Option<String>,
    },

    /// The App Mode characteristic was missing or did not support the needed
    /// operations.
    #[error("App Mode characteristic not found (FF11 or FF13) on service FF10")]
    CharacteristicNotFound,

    /// The controller did not answer within the deadline.
    #[error("timed out while waiting for {operation}")]
    Timeout {
        /// Human-readable description of the waited-for operation.
        operation: String,
    },

    /// A response carried an unexpected command or range.
    #[error("unexpected response: {0}")]
    UnexpectedResponse(String),

    /// The transfer ended before every byte arrived.
    #[error("incomplete transfer: got {got} of {expected} bytes")]
    IncompleteTransfer {
        /// Bytes that arrived.
        got: usize,
        /// Bytes that were expected.
        expected: usize,
    },

    /// The peer closed the notification channel.
    #[error("transport disconnected")]
    Disconnected,

    /// Local file I/O failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
