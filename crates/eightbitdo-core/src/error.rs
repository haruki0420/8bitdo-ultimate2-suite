//! Error type shared by the transport and session layers.

use std::fmt;

/// Stage of a macro flash write that failed.
///
/// The official order is Header → HeaderSave → Erase → Write → FinalSave →
/// Verify. `NotSaved` means neither Save nor Erase was sent; `HeaderSaved`
/// means the header Save was sent (the sector may also be erased/partially
/// written); `DataSaved` means header + data were sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroWriteStage {
    /// `0x0001` macro header write (216 bytes, `total_len = 0xD8`).
    Header,
    /// `0x0006` Save immediately after the header write.
    HeaderSave,
    /// `0x0104` erase of a macro's 4 KB sector.
    Erase,
    /// `0x0103` macro step chunk write.
    Write,
    /// `0x0006` Save after all data chunks.
    FinalSave,
    /// `0x0002` config header / `0x0102` raw read-back comparison.
    Verify,
}

impl fmt::Display for MacroWriteStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Header => formatter.write_str("Header"),
            Self::HeaderSave => formatter.write_str("HeaderSave"),
            Self::Erase => formatter.write_str("Erase"),
            Self::Write => formatter.write_str("Write"),
            Self::FinalSave => formatter.write_str("FinalSave"),
            Self::Verify => formatter.write_str("Verify"),
        }
    }
}

/// How far a failed macro write got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroWriteState {
    /// Header write failed; neither Save nor Erase was sent. The macro sector
    /// is unchanged.
    NotSaved,
    /// Header Save was sent, or Erase/Write failed after the header Save.
    /// The sector may be unchanged (header-save failure) or erased/partially
    /// written (erase/write failure). A header-only write (`0x0001` +
    /// `0x0006` with no Erase/Write) that fails its Verify also reports
    /// `HeaderSaved`: no step data and no Erase were sent, so the macro sector
    /// is unchanged.
    HeaderSaved,
    /// Final Save or Verify failed after header + data were sent. Only paths
    /// that sent the step data (and its Erase) report `DataSaved`.
    DataSaved,
}

impl fmt::Display for MacroWriteState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSaved => formatter.write_str("NotSaved"),
            Self::HeaderSaved => formatter.write_str("HeaderSaved"),
            Self::DataSaved => formatter.write_str("DataSaved"),
        }
    }
}

/// Stage of a full 2768-byte configuration write that failed.
///
/// The order is Write (`0x0001` chunks) → Save (`0x0006`) → Verify
/// (`0x0002` read-back). Prepare failures are local validation errors and do
/// not use this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigWriteStage {
    /// One `0x0001` configuration chunk was refused or timed out.
    Write,
    /// The `0x0006` Save was refused or timed out.
    Save,
    /// The post-Save `0x0002` read-back failed, did not match, or the CRC
    /// could not be reconciled.
    Verify,
}

impl fmt::Display for ConfigWriteStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Write => formatter.write_str("Write"),
            Self::Save => formatter.write_str("Save"),
            Self::Verify => formatter.write_str("Verify"),
        }
    }
}

/// How far a failed full configuration write got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigWriteState {
    /// No Save was acknowledged. For a `Write` stage failure no Save was sent
    /// at all; for a `Save` stage failure the Save was sent but its ACK never
    /// arrived, so the flash commit is unconfirmed. The controller must be
    /// re-read (and may keep partially updated bytes in RAM).
    NotSaved,
    /// The Save was acknowledged, so the new configuration is committed (or
    /// the device normalized it). Only the read-back / CRC verification is
    /// unconfirmed.
    Saved,
}

impl fmt::Display for ConfigWriteState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSaved => formatter.write_str("NotSaved"),
            Self::Saved => formatter.write_str("Saved"),
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

    /// The request is internally inconsistent.
    ///
    /// Examples: an empty per-slot batch, duplicate macro indices in one
    /// batch, or macros with different chunk sizes in one batch.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

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
    #[error("macro write failed during {stage} ({state}): {reason}; {recovery}")]
    MacroWrite {
        /// Stage that failed.
        stage: MacroWriteStage,
        /// How far the write got.
        state: MacroWriteState,
        /// Underlying error text.
        reason: String,
        /// Operator-facing recovery guidance.
        recovery: &'static str,
    },

    /// A full 2768-byte configuration write failed.
    #[error("config write failed during {stage} ({state}): {reason}; {recovery}")]
    ConfigWrite {
        /// Stage that failed.
        stage: ConfigWriteStage,
        /// How far the write got (before / after a confirmed Save).
        state: ConfigWriteState,
        /// Underlying error text.
        reason: String,
        /// Operator-facing recovery guidance.
        recovery: &'static str,
    },

    /// Local file I/O failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
