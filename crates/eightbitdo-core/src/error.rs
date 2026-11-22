//! Error type shared by the transport and session layers.

use std::fmt;

/// Stage of a macro flash write that failed.
///
/// The stages run in order: Erase → Write → Header → Save → Verify. `Save`
/// and `Verify` failures happen after the flash commit command was sent;
/// `Erase` / `Write` / `Header` failures happen before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroWriteStage {
    /// `0x0104` erase of the macro's 4 KB sector.
    Erase,
    /// `0x0103` macro step chunk write.
    Write,
    /// `0x0001` macro header update (216 bytes of the config block).
    Header,
    /// `0x0006` flash commit.
    Save,
    /// `0x0102` read-back comparison.
    Verify,
}

impl fmt::Display for MacroWriteStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Erase => formatter.write_str("Erase"),
            Self::Write => formatter.write_str("Write"),
            Self::Header => formatter.write_str("Header"),
            Self::Save => formatter.write_str("Save"),
            Self::Verify => formatter.write_str("Verify"),
        }
    }
}

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

    /// A requested wait or timeout was too large to schedule.
    ///
    /// Returning this instead of panicking keeps user-supplied durations
    /// (`--timeout`, `--hold`, ...) from overflowing [`std::time::Instant`].
    #[error("timeout of {0:?} is too large (maximum is 30 years)")]
    InvalidTimeout(std::time::Duration),

    /// The negotiated MTU leaves no usable macro payload.
    #[error("MTU {mtu} is too small for this operation: {reason}")]
    MtuTooSmall {
        /// Negotiated ATT MTU.
        mtu: u16,
        /// Why the operation cannot proceed.
        reason: String,
    },

    /// Writing into an empty macro requires step-data clear support that is
    /// not implemented yet.
    #[error(
        "empty macro write requires step data clear support; not implemented. Choose an existing macro."
    )]
    EmptyMacroWrite,

    /// The requested operation or transport variant is not implemented.
    #[error("unsupported operation: {0}")]
    Unsupported(String),

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

    /// A macro flash write failed.
    ///
    /// `saved` is true once the `0x0006` Save command was sent **or its send
    /// was attempted**, even if its acknowledgement or the read-back did not
    /// complete. When `saved` is false the Save command was not sent, but
    /// Erase or a partial Write may already have changed the sector. Restore
    /// from the full 3-slot × 4-macro backup before retrying.
    #[error("macro write failed during {stage}: {reason}; {recovery}")]
    MacroWrite {
        /// Stage that failed.
        stage: MacroWriteStage,
        /// Underlying error text.
        reason: String,
        /// True once the Save command was sent.
        saved: bool,
        /// Operator-facing recovery guidance.
        recovery: &'static str,
    },

    /// Local file I/O failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
