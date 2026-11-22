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

/// Stage of a BLE OTA update that failed.
///
/// The order is EnterBoot → ReadPid → ReadVersion → Erase → Write →
/// WriteVersion → WriteFirmwareVersion → Jump. `EnterBoot` is an App Mode
/// command/ACK stage (not a read); it, `ReadPid`, and `ReadVersion` all happen
/// before the first flash frame. The flash stages are only reachable after a
/// successful PID check, a matching expected application base, a sufficient
/// MTU, and an explicit unverified-wire opt-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtaFailureStage {
    /// App Mode `0x000C` EnterBoot.
    EnterBoot,
    /// Bootloader `0x0008` readPID.
    ReadPid,
    /// Bootloader `0x0005` readVersionInBoot.
    ReadVersion,
    /// Bootloader `0x0004` EraseFlash.
    Erase,
    /// Bootloader `0x0003` WriteFirmware.
    Write,
    /// Bootloader `0x00C4` WriteVersion (24-byte image header).
    WriteVersion,
    /// Bootloader `0x0002` WriteFirmwareVersion.
    WriteFirmwareVersion,
    /// Bootloader `0x0006` JumpApp.
    Jump,
}

impl fmt::Display for OtaFailureStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnterBoot => formatter.write_str("EnterBoot"),
            Self::ReadPid => formatter.write_str("ReadPid"),
            Self::ReadVersion => formatter.write_str("ReadVersion"),
            Self::Erase => formatter.write_str("Erase"),
            Self::Write => formatter.write_str("Write"),
            Self::WriteVersion => formatter.write_str("WriteVersion"),
            Self::WriteFirmwareVersion => formatter.write_str("WriteFirmwareVersion"),
            Self::Jump => formatter.write_str("Jump"),
        }
    }
}

/// How far a failed BLE OTA update got, measured by **frames the host tried to
/// send** (not by ACKs received).
///
/// The bootloader keeps running after every failed stage (the application area
/// is only valid again after a full erase + rewrite + JumpApp), so recovery is
/// always a fresh `OtaUpdate` from `Idle` → `prepare_boot` on `"8bt"` → erase
/// and rewrite every sector from offset 0. The state tells the caller how much
/// of that recovery is required. The associated counters
/// (`OtaUpdate::sectors_erased`, `OtaUpdate::chunks_written`) count only
/// acknowledged operations and can therefore be lower than the frames sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtaWriteState {
    /// No EraseFlash or WriteFirmware frame was sent. This covers EnterBoot,
    /// PID / version read failures, PID mismatch, local image validation,
    /// expected-base / MTU / unverified-wire refusals, and a send that failed
    /// before any flash frame. The flash content is assumed unchanged
    /// (EnterBoot may still have rebooted the controller).
    Untouched,
    /// At least one EraseFlash frame was sent and no WriteFirmware frame was
    /// sent yet. Its ACK may not have arrived, so the sector may be erased,
    /// partially erased, or still pending; treat the flash content as unknown.
    EraseStarted,
    /// At least one WriteFirmware frame was sent (chunk ACKs may be zero or
    /// more). Part of the image may be written; erase and rewrite the whole
    /// image from offset 0.
    PartiallyWritten,
    /// Every WriteFirmware chunk was acknowledged, but finalization
    /// (WriteVersion / WriteFirmwareVersion / JumpApp) did not complete. The
    /// image data may be complete but the application is not confirmed; erase
    /// and rewrite the whole image before retrying JumpApp.
    Written,
}

impl fmt::Display for OtaWriteState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Untouched => formatter.write_str("Untouched"),
            Self::EraseStarted => formatter.write_str("EraseStarted"),
            Self::PartiallyWritten => formatter.write_str("PartiallyWritten"),
            Self::Written => formatter.write_str("Written"),
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

    /// Another process owns the single-owner BLE lock.
    ///
    /// Scanning, connecting and holding a resident session all take
    /// [`crate::BleLock`] first; contention is reported instead of waiting,
    /// because waiting behind a resident session would look like a hang. The
    /// controller intentionally is not touched.
    #[cfg(feature = "ble")]
    #[error(
        "another process is using the BLE adapter (lock file {}); close the other \
         scan/session/connect and retry (if an App Mode session is open, disconnect/drop it \
         first: the \"8bt\" boot link takes the same single-owner lock)",
        path.display()
    )]
    BleBusy {
        /// Lock file whose holder refused this operation.
        path: std::path::PathBuf,
    },

    /// The BLE lock path is unsafe to use.
    ///
    /// The lock file must be a regular file owned by the effective user and
    /// must not be a symbolic link: the guard truncates the file and tightens
    /// its mode, so following an attacker-controlled path in a shared
    /// directory could damage an arbitrary file.
    #[cfg(feature = "ble")]
    #[error("refusing to use BLE lock file {}: {reason}", path.display())]
    BleLockInvalid {
        /// Rejected lock path.
        path: std::path::PathBuf,
        /// Why the path was rejected.
        reason: String,
    },

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

    /// A BLE OTA (firmware update) failed.
    #[error("OTA update failed during {stage} ({state}): {reason}; {recovery}")]
    Ota {
        /// Stage that failed.
        stage: OtaFailureStage,
        /// How far the update got (whether flash may have changed).
        state: OtaWriteState,
        /// Underlying error text.
        reason: String,
        /// Operator-facing recovery guidance.
        recovery: &'static str,
    },

    /// The bootloader PID is not one whose `readVersionInBoot` / `JumpApp`
    /// offsets are documented.
    ///
    /// Only `gamepad_type == 1` (PID `0x600F`) is documented. The version and
    /// jump addresses of any other type are unknown, so the update stops after
    /// `readPID` without sending a read at a guessed offset or any flash write.
    /// This is a pre-write failure and always carries
    /// [`OtaWriteState::Untouched`].
    #[error(
        "unsupported bootloader PID 0x{pid:04X} ({state}): only gamepad_type 1 (PID 0x600F) has \
         documented BLE OTA readVersionInBoot/JumpApp offsets; stopping before any flash write"
    )]
    OtaUnsupportedPid {
        /// PID the bootloader reported.
        pid: u16,
        /// Always [`OtaWriteState::Untouched`]; no flash frame was sent.
        state: OtaWriteState,
    },

    /// A `.dat` firmware image failed local validation before any flash write.
    ///
    /// Also used for a `load_base_addr` that does not match the mandatory
    /// `OtaConfig::expected_load_base` allowlist. This is a pre-write failure
    /// equivalent to [`OtaWriteState::Untouched`]. Only single-section `.dat`
    /// files are supported; official multi-section firmware is rejected.
    #[error("invalid firmware image: {0}")]
    OtaInvalidFirmware(String),

    /// Local file I/O failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
