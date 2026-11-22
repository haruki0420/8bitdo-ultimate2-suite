//! BLE OTA (firmware update) state machine over the `"8bt"` bootloader.
//!
//! This module implements the documented BLE OTA path only
//! (`docs/PROTOCOL_SPEC.md` §8):
//!
//! ```text
//! App Mode "80ND"                    Bootloader "8bt" (service FF20 / char FF21)
//!   0x000C EnterBoot  ──┐
//!                       └─ BLE reconnect by the front end
//!                         0x0008 readPID            (5 s)
//!                         0x0005 readVersionInBoot  (5 s, optional)
//!                         0x0004 EraseFlash         (4 KB, 10 s)
//!                         0x0003 WriteFirmware      (64 B + CRC-16/MODBUS, 1 s)
//!                         0x00C4 WriteVersion       (24-byte header, 3 s)
//!                         0x0002 WriteFirmwareVersion (4 B, send-only)
//!                         0x0006 JumpApp            (send, best-effort ACK)
//! ```
//!
//! [`OtaUpdate`] enforces the stage order: no flash frame can be sent before
//! `readPID` confirmed a supported `gamepad_type`, and no write can start
//! before the whole image has been validated locally. Every failure is
//! fail-closed ([`Error::Ota`]) with the failing stage and how far the update
//! got ([`OtaWriteState`]).
//!
//! # USB is not implemented
//!
//! USB Boot TX framing (`EraseFlash` / `WriteFirmware` / `WriteVersion` /
//! `WriteAPPID` / `JumpApp`) is undocumented (`docs/PROTOCOL_SPEC.md` §8.4,
//! `AGENTS.md` §4.4). `[0x00, 0x05]` is an RX-only prefix and must not be
//! reused as TX framing, so this module implements **BLE only**. USB OTA is
//! `N/A` until the spec carries framing evidence, and USB recovery is
//! re-enumeration / reconnection, never the BLE `"8bt"` reconnect.
//!
//! # `gamepad_type` policy
//!
//! `readPID` is the gate before any offset is used. Only PID `0x600F` is
//! mapped to `gamepad_type == 1` ([`gamepad_type_from_pid`]); every other PID
//! returns [`Error::OtaUnsupportedPid`] and the update stops **before**
//! `readVersionInBoot` and before any flash frame. The `0x01017040` version
//! offset and `0x01018000` jump entry point are documented for `type 1` only;
//! the type `0` addresses (`0x08014034` / `0x08014000`) are not implemented
//! because no BLE OTA evidence covers them.
//!
//! # Assumptions that still need hardware evidence
//!
//! * Bootloader RX framing is assumed to be the same as App Mode: a 16-byte
//!   header with **no** leading prefix (`0x05` is TX-only here).
//! * The bootloader replies with the `cmd 0x0004` envelope echoing the request
//!   `cmd` in `subcmd`, or with a direct `cmd` echo. A payload-less envelope is
//!   treated as a ready ACK and skipped before read data.
//! * The EnterBoot ACK or the App link close both count as "reboot requested";
//!   neither leads to a flash write until `readPID` succeeds on `"8bt"`.
//! * EraseFlash addresses follow `PROTOCOL_SPEC` §8.3
//!   (`load_base_addr + sector_off`), where `load_base_addr` comes from
//!   `.dat +0x04`. `evidence.md` §6 reports that the Android implementation
//!   puts the image-relative `sector_off` on the wire instead; this conflict is
//!   recorded as unverified and the spec form is implemented.
//! * `WriteVersion` sends the documented 24-byte image header. `evidence.md`
//!   §2 reports the Android implementation sends a 16-byte frame with no
//!   payload; again unverified.
//! * Multi-section `.dat` images are rejected, not guessed at.

use std::time::Duration;

use eightbitdo_protocol::commands::{self, boot, cmd};
use eightbitdo_protocol::header::{decode_app_response, CustomHeader};
use tokio::time::Instant;

use crate::error::{OtaFailureStage, OtaWriteState};
use crate::session::{deadline_after, Session};
use crate::transport::Transport;
use crate::Error;

/// Wait for the App Mode `0x000C` EnterBoot acknowledgement.
///
/// Not documented; provisional, matching the bootloader read timeout.
pub const OTA_ENTER_BOOT_TIMEOUT: Duration = Duration::from_secs(5);

/// Wait for the Bootloader `0x0008` readPID response.
pub const OTA_PID_TIMEOUT: Duration = Duration::from_secs(5);

/// Wait for the Bootloader `0x0005` readVersionInBoot response.
pub const OTA_VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// Wait for one Bootloader `0x0004` EraseFlash acknowledgement.
pub const OTA_ERASE_TIMEOUT: Duration = Duration::from_secs(10);

/// Wait for one Bootloader `0x0003` WriteFirmware acknowledgement.
pub const OTA_WRITE_TIMEOUT: Duration = Duration::from_secs(1);

/// Wait for the Bootloader `0x00C4` WriteVersion acknowledgement.
pub const OTA_WRITE_VERSION_TIMEOUT: Duration = Duration::from_secs(3);

/// Best-effort wait for the Bootloader `0x0006` JumpApp acknowledgement.
pub const OTA_JUMP_TIMEOUT: Duration = Duration::from_secs(5);

/// Flash erase sector size (`0x1000` = 4096 bytes).
pub const OTA_SECTOR_SIZE: u32 = boot::SECTOR_SIZE;

/// Firmware write chunk size (`0x40` = 64 bytes).
pub const OTA_CHUNK_SIZE: usize = boot::CHUNK_SIZE;

/// Length of the `.dat` image header (`0x1C` = 28 bytes).
pub const OTA_IMAGE_HEADER_LEN: usize = 0x1C;

/// Length of the image-header metadata sent by `WriteVersion` (`0x18` = 24).
pub const OTA_VERSION_HEADER_LEN: usize = 0x18;

/// Bootloader PID of the Ultimate 2 Bluetooth (`gamepad_type == 1`).
pub const UT2_BOOT_PID: u16 = 0x600F;

/// The controller generation whose BLE OTA offsets are documented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadType {
    /// `gamepad_type == 1` (Ultimate 2 Bluetooth, PID `0x600F`): the
    /// `readVersionInBoot` / `WriteFirmwareVersion` offset `0x01017040` and the
    /// `JumpApp` entry `0x01018000` are documented.
    Type1,
}

/// Map a bootloader PID to a documented [`GamepadType`].
///
/// Returns `None` for every PID whose OTA offsets are not documented, so the
/// caller stops instead of guessing an address.
#[must_use]
pub fn gamepad_type_from_pid(pid: u16) -> Option<GamepadType> {
    match pid {
        UT2_BOOT_PID => Some(GamepadType::Type1),
        _ => None,
    }
}

/// Phase of the [`OtaUpdate`] state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtaPhase {
    /// No step has run yet.
    Idle,
    /// `0x000C` EnterBoot is in flight on the App Mode link.
    EnteringBoot,
    /// EnterBoot was acknowledged (or the App link closed). The front end must
    /// scan for `"8bt"` and connect to service FF20 / characteristic FF21.
    AwaitingBootReconnect,
    /// The bootloader link is up; readPID / readVersionInBoot are running.
    PreparingBoot,
    /// PID and `gamepad_type` are confirmed (and the boot version read if
    /// requested). The update may now erase and write.
    Ready,
    /// EraseFlash is being acknowledged.
    Erasing,
    /// WriteFirmware chunks are being acknowledged.
    Writing,
    /// All chunks are acknowledged; WriteVersion / WriteFirmwareVersion /
    /// JumpApp are running.
    Finalizing,
    /// JumpApp was sent. The controller should reboot into the application.
    Complete,
    /// The update stopped fail-closed. The controller stays in the bootloader;
    /// create a new [`OtaUpdate`] and attach to `"8bt"` to recover.
    Failed,
}

impl OtaPhase {
    /// True for [`OtaPhase::Complete`] and [`OtaPhase::Failed`].
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Failed)
    }
}

/// ACK timeouts for one OTA update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtaTimeouts {
    /// App Mode `0x000C` EnterBoot wait.
    pub enter_boot: Duration,
    /// Bootloader `0x0008` readPID wait.
    pub pid: Duration,
    /// Bootloader `0x0005` readVersionInBoot wait.
    pub version: Duration,
    /// Per-sector `0x0004` EraseFlash wait.
    pub erase: Duration,
    /// Per-chunk `0x0003` WriteFirmware wait.
    pub write: Duration,
    /// `0x00C4` WriteVersion wait.
    pub write_version: Duration,
    /// Best-effort `0x0006` JumpApp wait.
    pub jump: Duration,
}

impl Default for OtaTimeouts {
    fn default() -> Self {
        Self {
            enter_boot: OTA_ENTER_BOOT_TIMEOUT,
            pid: OTA_PID_TIMEOUT,
            version: OTA_VERSION_TIMEOUT,
            erase: OTA_ERASE_TIMEOUT,
            write: OTA_WRITE_TIMEOUT,
            write_version: OTA_WRITE_VERSION_TIMEOUT,
            jump: OTA_JUMP_TIMEOUT,
        }
    }
}

/// Configuration for one OTA update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtaConfig {
    /// ACK timeouts.
    pub timeouts: OtaTimeouts,
    /// Read `readVersionInBoot` (`0x0005`) after the PID check.
    ///
    /// The step is optional in the spec. It is read-only and its result is
    /// reported in [`OtaOutcome::boot_version`]; a failure aborts before any
    /// flash write.
    pub read_version_in_boot: bool,
}

impl Default for OtaConfig {
    fn default() -> Self {
        Self {
            timeouts: OtaTimeouts::default(),
            read_version_in_boot: true,
        }
    }
}

impl OtaConfig {
    /// Replace the ACK timeouts.
    #[must_use]
    pub fn with_timeouts(mut self, timeouts: OtaTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Choose whether `readVersionInBoot` is sent.
    #[must_use]
    pub fn with_read_version_in_boot(mut self, enabled: bool) -> Self {
        self.read_version_in_boot = enabled;
        self
    }
}

/// Progress snapshot passed to the [`OtaUpdate::write_firmware`] callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtaProgress {
    /// Current phase.
    pub phase: OtaPhase,
    /// Zero-based index of the sector being erased / written.
    pub sector_index: u32,
    /// Number of 4 KB sectors that cover the image.
    pub sector_count: u32,
    /// Firmware chunks acknowledged so far.
    pub chunks_written: u32,
    /// Total number of 64-byte chunks in the image.
    pub chunk_count: u32,
    /// Image bytes acknowledged so far.
    pub bytes_written: u32,
    /// Image size in bytes.
    pub total_bytes: u32,
}

/// Result of a successful OTA update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtaOutcome {
    /// PID reported by the bootloader.
    pub pid: u16,
    /// Documented gamepad type the PID maps to.
    pub gamepad_type: GamepadType,
    /// Bootloader version read with `0x0005`, or `None` when the optional read
    /// was disabled.
    pub boot_version: Option<u32>,
    /// 4 KB sectors erased.
    pub sectors_erased: u32,
    /// 64-byte chunks acknowledged.
    pub chunks_written: u32,
    /// Image bytes acknowledged (equals the `.dat` `image_len`).
    pub bytes_written: u32,
    /// Bootloader frames sent (readPID through JumpApp).
    pub packets_sent: u32,
    /// Whether JumpApp was acknowledged before the link closed.
    ///
    /// `false` means the frame was sent but the controller rebooted (or went
    /// silent) without a visible ACK; that is the expected behavior on real
    /// hardware and is not a failure.
    pub jump_acknowledged: bool,
}

/// One `.dat` firmware image, validated before the first flash frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareImage<'a> {
    dat: &'a [u8],
    version: u32,
    load_base: u32,
    image_len: u32,
}

impl<'a> FirmwareImage<'a> {
    /// Parse and validate a 28-byte `.dat` header plus its image data.
    ///
    /// The header is `version (u32)`, `load_base_addr (u32)`,
    /// `image_data_len (u32)` then 16 bytes of metadata. All multi-byte fields
    /// are little-endian.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OtaInvalidFirmware`] when the file is shorter than the
    /// 28-byte header, `image_data_len` is zero or not a multiple of 64, the
    /// base is not 64-byte aligned, `base + image_len` overflows `u32`, the
    /// file is shorter than `28 + image_len` (truncated), or the file carries
    /// trailing data (multi-section images are not implemented).
    pub fn parse(dat: &'a [u8]) -> Result<Self, Error> {
        if dat.len() < OTA_IMAGE_HEADER_LEN {
            return Err(invalid_firmware(format!(
                "file is {} bytes; the .dat header alone is {OTA_IMAGE_HEADER_LEN} bytes",
                dat.len()
            )));
        }
        let version = le_u32(&dat[0..4]);
        let load_base = le_u32(&dat[4..8]);
        let image_len = le_u32(&dat[8..12]);
        if image_len == 0 {
            return Err(invalid_firmware(
                "image_len at .dat +0x08 is 0; refusing an empty firmware image".to_string(),
            ));
        }
        if image_len % OTA_CHUNK_SIZE as u32 != 0 {
            return Err(invalid_firmware(format!(
                "image_len {image_len} is not a multiple of the {OTA_CHUNK_SIZE}-byte write chunk"
            )));
        }
        if load_base % OTA_CHUNK_SIZE as u32 != 0 {
            return Err(invalid_firmware(format!(
                "load_base_addr 0x{load_base:08X} is not {OTA_CHUNK_SIZE}-byte aligned"
            )));
        }
        if u64::from(load_base) + u64::from(image_len) > u64::from(u32::MAX) + 1 {
            return Err(invalid_firmware(format!(
                "load_base_addr 0x{load_base:08X} + image_len {image_len} overflows the 32-bit \
                 address space"
            )));
        }
        let image_len_usize = usize::try_from(image_len).map_err(|_| {
            invalid_firmware(format!("image_len {image_len} does not fit in usize"))
        })?;
        let expected_len = OTA_IMAGE_HEADER_LEN
            .checked_add(image_len_usize)
            .ok_or_else(|| invalid_firmware("image length overflows usize".to_string()))?;
        if dat.len() < expected_len {
            return Err(invalid_firmware(format!(
                "file is {} bytes but the header declares 28 + {image_len} = {expected_len}; \
                 the image is truncated",
                dat.len()
            )));
        }
        if dat.len() > expected_len {
            return Err(invalid_firmware(format!(
                "file is {} bytes but the header declares {expected_len}; trailing data looks \
                 like a multi-section .dat, which this implementation does not support",
                dat.len()
            )));
        }
        Ok(Self {
            dat,
            version,
            load_base,
            image_len,
        })
    }

    /// `firmwareFile_Version` (`.dat +0x00`).
    #[must_use]
    pub const fn version(self) -> u32 {
        self.version
    }

    /// `load_base_addr` (`.dat +0x04`).
    #[must_use]
    pub const fn load_base(self) -> u32 {
        self.load_base
    }

    /// `image_data_len` (`.dat +0x08`).
    #[must_use]
    pub const fn image_len(self) -> u32 {
        self.image_len
    }

    /// The image payload after the 28-byte header (`image_len` bytes).
    #[must_use]
    pub fn image(self) -> &'a [u8] {
        &self.dat[OTA_IMAGE_HEADER_LEN..OTA_IMAGE_HEADER_LEN + self.image_len as usize]
    }

    /// The 24-byte image header metadata sent by `WriteVersion` (`.dat +0x00`).
    #[must_use]
    pub fn version_header(self) -> &'a [u8] {
        &self.dat[..OTA_VERSION_HEADER_LEN]
    }

    /// Raw `.dat` bytes this image was parsed from.
    #[must_use]
    pub const fn as_dat(self) -> &'a [u8] {
        self.dat
    }
}

/// Recovery text for a failure before any flash frame.
const RECOVERY_READ_ONLY: &str =
    "nothing was erased or written. Reconnect to the bootloader (\"8bt\"), read the PID again, \
     and restart the update; if \"8bt\" is no longer advertised, power-cycle the controller and \
     retry from the App Mode link.";

/// Recovery text for an EnterBoot failure.
const RECOVERY_ENTER_BOOT: &str =
    "nothing was erased or written. Reconnect to the App Mode peripheral (\"80ND\"), confirm the \
     controller is idle, and retry EnterBoot; if the controller already advertises \"8bt\", attach \
     to the bootloader and start from the boot phase.";

/// Recovery text for an EraseFlash failure.
const RECOVERY_ERASE: &str =
    "flash erase was sent but not acknowledged, so the application area may be erased. Reconnect \
     to \"8bt\", read the PID, and erase and rewrite every sector from offset 0; do not send \
     JumpApp.";

/// Recovery text for a WriteFirmware failure.
const RECOVERY_WRITE: &str =
    "the image is partially written. Reconnect to \"8bt\", read the PID, erase every sector, and \
     rewrite the whole image from offset 0; do not send JumpApp until the rewrite completes.";

/// Recovery text for a finalization failure.
const RECOVERY_FINALIZE: &str =
    "every firmware chunk was acknowledged, but finalization failed. Reconnect to \"8bt\", read \
     the PID, erase and rewrite the whole image again, and only then finalize and send JumpApp.";

/// BLE OTA state machine.
///
/// Construct one per update attempt. The intended order is:
///
/// 1. [`OtaUpdate::enter_boot`] on the App Mode [`Session`] (`0x000C`),
/// 2. let the front end reconnect to `"8bt"` (service FF20 / characteristic
///    FF21) and keep the boot [`Transport`],
/// 3. [`OtaUpdate::prepare_boot`] (`readPID`, `gamepad_type` check, optional
///    `readVersionInBoot`),
/// 4. [`OtaUpdate::write_firmware`] (erase / write / finalize / jump).
///
/// If the controller is already in the bootloader (recovery after a failed
/// update), [`OtaUpdate::prepare_boot`] may be called from [`OtaPhase::Idle`]
/// without [`OtaUpdate::enter_boot`].
///
/// A failed update marks the phase [`OtaPhase::Failed`] and cannot continue;
/// create a new `OtaUpdate` for the recovery attempt.
pub struct OtaUpdate {
    config: OtaConfig,
    phase: OtaPhase,
    pid: Option<u16>,
    gamepad_type: Option<GamepadType>,
    boot_version: Option<u32>,
    sectors_erased: u32,
    chunks_written: u32,
    bytes_written: u32,
    packets_sent: u32,
    image_len: u32,
}

impl Default for OtaUpdate {
    fn default() -> Self {
        Self::new()
    }
}

impl OtaUpdate {
    /// Create an update with the default [`OtaConfig`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(OtaConfig::default())
    }

    /// Create an update with an explicit configuration.
    #[must_use]
    pub fn with_config(config: OtaConfig) -> Self {
        Self {
            config,
            phase: OtaPhase::Idle,
            pid: None,
            gamepad_type: None,
            boot_version: None,
            sectors_erased: 0,
            chunks_written: 0,
            bytes_written: 0,
            packets_sent: 0,
            image_len: 0,
        }
    }

    /// Configured timeouts and options.
    #[must_use]
    pub const fn config(&self) -> &OtaConfig {
        &self.config
    }

    /// Current state-machine phase.
    #[must_use]
    pub const fn phase(&self) -> OtaPhase {
        self.phase
    }

    /// PID read from the bootloader, if [`OtaUpdate::prepare_boot`] got that far.
    #[must_use]
    pub const fn pid(&self) -> Option<u16> {
        self.pid
    }

    /// Documented gamepad type of [`OtaUpdate::pid`], if supported.
    #[must_use]
    pub const fn gamepad_type(&self) -> Option<GamepadType> {
        self.gamepad_type
    }

    /// Bootloader version read by `readVersionInBoot`, if it was enabled.
    #[must_use]
    pub const fn boot_version(&self) -> Option<u32> {
        self.boot_version
    }

    /// 4 KB sectors erased so far.
    #[must_use]
    pub const fn sectors_erased(&self) -> u32 {
        self.sectors_erased
    }

    /// 64-byte chunks acknowledged so far.
    #[must_use]
    pub const fn chunks_written(&self) -> u32 {
        self.chunks_written
    }

    /// Image bytes acknowledged so far.
    #[must_use]
    pub const fn bytes_written(&self) -> u32 {
        self.bytes_written
    }

    /// Bootloader frames sent so far.
    #[must_use]
    pub const fn packets_sent(&self) -> u32 {
        self.packets_sent
    }

    /// Send App Mode `0x000C` EnterBoot and wait for its ACK or the link close.
    ///
    /// Acknowledging the command and closing the App Mode link are both
    /// accepted: the controller reboots into the bootloader, so a disconnect is
    /// the expected end state. Either way no flash frame is sent here; the next
    /// gate is [`OtaUpdate::prepare_boot`] on the `"8bt"` link.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] unless the phase is [`OtaPhase::Idle`],
    /// or [`Error::Ota`] with stage [`OtaFailureStage::EnterBoot`] when the
    /// frame could not be sent or the ACK timed out.
    pub async fn enter_boot<T: Transport>(&mut self, app: &Session<T>) -> Result<(), Error> {
        if self.phase != OtaPhase::Idle {
            return Err(Error::InvalidRequest(format!(
                "EnterBoot requires the Idle phase (current phase is {:?})",
                self.phase
            )));
        }
        self.phase = OtaPhase::EnteringBoot;
        let result = app
            .run_write(app.send_and_await_ack(
                &commands::enter_boot(),
                false,
                cmd::ENTER_BOOT,
                commands::subcmd::ENTER_BOOT,
                self.config.timeouts.enter_boot,
                "EnterBoot",
            ))
            .await;
        match result {
            Ok(()) => {
                self.phase = OtaPhase::AwaitingBootReconnect;
                Ok(())
            }
            // The controller reboots into "8bt" and may drop the link before
            // (or instead of) an ACK. No flash frame follows without a PID
            // check, so this is safe to accept.
            Err(Error::Disconnected) => {
                self.phase = OtaPhase::AwaitingBootReconnect;
                Ok(())
            }
            Err(error) => Err(self.fail(
                OtaFailureStage::EnterBoot,
                OtaWriteState::Untouched,
                error,
                RECOVERY_ENTER_BOOT,
            )),
        }
    }

    /// Read the bootloader PID, verify `gamepad_type`, and optionally read the
    /// bootloader version.
    ///
    /// This is the mandatory gate before ([`OtaUpdate::write_firmware`]): a
    /// non-type-1 PID stops with [`Error::OtaUnsupportedPid`] without guessing
    /// the `readVersionInBoot` offset and without sending any flash frame.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] unless the phase is
    /// [`OtaPhase::Idle`] (already-booted recovery) or
    /// [`OtaPhase::AwaitingBootReconnect`]; [`Error::OtaUnsupportedPid`] for a
    /// PID whose OTA offsets are undocumented; or [`Error::Ota`] with stage
    /// [`OtaFailureStage::ReadPid`] / [`OtaFailureStage::ReadVersion`] when a
    /// read fails.
    pub async fn prepare_boot<T: Transport>(&mut self, boot_transport: &T) -> Result<(), Error> {
        if !matches!(self.phase, OtaPhase::Idle | OtaPhase::AwaitingBootReconnect) {
            return Err(Error::InvalidRequest(format!(
                "prepare_boot requires the Idle or AwaitingBootReconnect phase (current phase \
                 is {:?})",
                self.phase
            )));
        }
        self.phase = OtaPhase::PreparingBoot;

        self.packets_sent += 1;
        let pid = match boot_read_pid(boot_transport, self.config.timeouts.pid).await {
            Ok(pid) => pid,
            Err(error) => {
                return Err(self.fail(
                    OtaFailureStage::ReadPid,
                    OtaWriteState::Untouched,
                    error,
                    RECOVERY_READ_ONLY,
                ))
            }
        };
        self.pid = Some(pid);
        let Some(gamepad_type) = gamepad_type_from_pid(pid) else {
            self.phase = OtaPhase::Failed;
            return Err(Error::OtaUnsupportedPid { pid });
        };
        self.gamepad_type = Some(gamepad_type);

        if self.config.read_version_in_boot {
            self.packets_sent += 1;
            match boot_read_version(boot_transport, self.config.timeouts.version).await {
                Ok(version) => self.boot_version = Some(version),
                Err(error) => {
                    return Err(self.fail(
                        OtaFailureStage::ReadVersion,
                        OtaWriteState::Untouched,
                        error,
                        RECOVERY_READ_ONLY,
                    ))
                }
            }
        }

        self.phase = OtaPhase::Ready;
        Ok(())
    }

    /// Erase, write, finalize, and jump to the application.
    ///
    /// Every frame is built and validated before the first EraseFlash is sent.
    /// Per sector the order is EraseFlash → its ACK → 64 WriteFirmware chunks
    /// (one ACK each) → next sector. After the last chunk the order is
    /// WriteVersion → WriteFirmwareVersion → JumpApp
    /// (`docs/PROTOCOL_SPEC.md` §8.4).
    ///
    /// `progress` is called before each sector erase, after every acknowledged
    /// chunk, when finalization starts, and on completion. The initial
    /// [`OtaPhase::Erasing`] callback carries `bytes_written = 0`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] unless the phase is [`OtaPhase::Ready`]
    /// (so a PID check cannot be skipped), [`Error::OtaInvalidFirmware`] for an
    /// image that fails local validation (including a `load_base_addr` that is
    /// not 4 KB aligned), or [`Error::Ota`] with the failing stage and
    /// [`OtaWriteState`].
    pub async fn write_firmware<T: Transport>(
        &mut self,
        boot_transport: &T,
        image: &FirmwareImage<'_>,
        progress: &mut dyn FnMut(OtaProgress),
    ) -> Result<OtaOutcome, Error> {
        if self.phase != OtaPhase::Ready {
            return Err(Error::InvalidRequest(format!(
                "write_firmware requires the Ready phase after prepare_boot (current phase is \
                 {:?})",
                self.phase
            )));
        }
        if image.load_base() % OTA_SECTOR_SIZE != 0 {
            return Err(self.invalid_firmware(format!(
                "load_base_addr 0x{:08X} is not {OTA_SECTOR_SIZE}-byte aligned; EraseFlash \
                 addresses are load_base_addr + sector_off, so every erase sector must be aligned",
                image.load_base()
            )));
        }

        // Build every frame first: a local framing error must not leave a
        // partially erased device.
        let write_requests =
            match boot::write_firmware_packets(image.load_base(), image.image_len(), image.image())
            {
                Ok(requests) => requests,
                Err(error) => return Err(self.invalid_firmware(error.to_string())),
            };
        let version_header =
            match boot::write_version_header(image.image_len(), image.version_header()) {
                Ok(request) => request,
                Err(error) => return Err(self.invalid_firmware(error.to_string())),
            };
        let firmware_version = boot::write_firmware_version(image.version());
        let jump = boot::jump_app();

        self.image_len = image.image_len();
        let chunk_count = image.image_len() / OTA_CHUNK_SIZE as u32;
        let sector_count = image.image_len().div_ceil(OTA_SECTOR_SIZE);
        let chunks_per_sector = OTA_SECTOR_SIZE / OTA_CHUNK_SIZE as u32;

        let mut chunk_index: u32 = 0;
        for sector_index in 0..sector_count {
            self.phase = OtaPhase::Erasing;
            self.emit(
                progress,
                OtaPhase::Erasing,
                sector_index,
                sector_count,
                chunk_count,
            );

            let sector_addr = image.load_base() + sector_index * OTA_SECTOR_SIZE;
            let erase = match boot::erase_flash(sector_addr) {
                Ok(request) => request,
                Err(error) => return Err(self.invalid_firmware(error.to_string())),
            };
            self.packets_sent += 1;
            if let Err(error) = boot_send_and_await_ack(
                boot_transport,
                &erase,
                self.config.timeouts.erase,
                "EraseFlash",
            )
            .await
            {
                return Err(self.fail(
                    OtaFailureStage::Erase,
                    OtaWriteState::EraseStarted,
                    error,
                    RECOVERY_ERASE,
                ));
            }
            self.sectors_erased += 1;

            self.phase = OtaPhase::Writing;
            let sector_chunks = (chunk_count - chunk_index).min(chunks_per_sector);
            for _ in 0..sector_chunks {
                let request = &write_requests[chunk_index as usize];
                self.packets_sent += 1;
                if let Err(error) = boot_send_and_await_ack(
                    boot_transport,
                    request,
                    self.config.timeouts.write,
                    "WriteFirmware",
                )
                .await
                {
                    return Err(self.fail(
                        OtaFailureStage::Write,
                        OtaWriteState::PartiallyWritten,
                        error,
                        RECOVERY_WRITE,
                    ));
                }
                chunk_index += 1;
                self.chunks_written += 1;
                self.bytes_written += OTA_CHUNK_SIZE as u32;
                self.emit(
                    progress,
                    OtaPhase::Writing,
                    sector_index,
                    sector_count,
                    chunk_count,
                );
            }
        }

        self.phase = OtaPhase::Finalizing;
        self.emit(
            progress,
            OtaPhase::Finalizing,
            sector_count.saturating_sub(1),
            sector_count,
            chunk_count,
        );

        self.packets_sent += 1;
        if let Err(error) = boot_send_and_await_ack(
            boot_transport,
            &version_header,
            self.config.timeouts.write_version,
            "WriteVersion",
        )
        .await
        {
            return Err(self.fail(
                OtaFailureStage::WriteVersion,
                OtaWriteState::Written,
                error,
                RECOVERY_FINALIZE,
            ));
        }

        // The spec gives WriteFirmwareVersion a zero timeout (send-only), so no
        // ACK is consumed here.
        self.packets_sent += 1;
        if let Err(error) = boot_send_only(boot_transport, &firmware_version).await {
            return Err(self.fail(
                OtaFailureStage::WriteFirmwareVersion,
                OtaWriteState::Written,
                error,
                RECOVERY_FINALIZE,
            ));
        }

        self.packets_sent += 1;
        let jump_acknowledged =
            match boot_jump_app(boot_transport, &jump, self.config.timeouts.jump).await {
                Ok(acknowledged) => acknowledged,
                Err(error) => {
                    return Err(self.fail(
                        OtaFailureStage::Jump,
                        OtaWriteState::Written,
                        error,
                        RECOVERY_FINALIZE,
                    ))
                }
            };

        self.phase = OtaPhase::Complete;
        self.emit(
            progress,
            OtaPhase::Complete,
            sector_count.saturating_sub(1),
            sector_count,
            chunk_count,
        );

        let (Some(pid), Some(gamepad_type)) = (self.pid, self.gamepad_type) else {
            return Err(Error::InvalidRequest(
                "write_firmware completed without a PID gate result; the update state is \
                 inconsistent"
                    .to_string(),
            ));
        };

        Ok(OtaOutcome {
            pid,
            gamepad_type,
            boot_version: self.boot_version,
            sectors_erased: self.sectors_erased,
            chunks_written: self.chunks_written,
            bytes_written: self.bytes_written,
            packets_sent: self.packets_sent,
            jump_acknowledged,
        })
    }

    fn emit(
        &self,
        progress: &mut dyn FnMut(OtaProgress),
        phase: OtaPhase,
        sector_index: u32,
        sector_count: u32,
        chunk_count: u32,
    ) {
        progress(OtaProgress {
            phase,
            sector_index,
            sector_count,
            chunks_written: self.chunks_written,
            chunk_count,
            bytes_written: self.bytes_written,
            total_bytes: self.image_len,
        });
    }

    fn fail(
        &mut self,
        stage: OtaFailureStage,
        state: OtaWriteState,
        error: Error,
        recovery: &'static str,
    ) -> Error {
        self.phase = OtaPhase::Failed;
        Error::Ota {
            stage,
            state,
            reason: error.to_string(),
            recovery,
        }
    }

    fn invalid_firmware(&mut self, reason: String) -> Error {
        self.phase = OtaPhase::Failed;
        Error::OtaInvalidFirmware(reason)
    }
}

/// Read the bootloader PID (`0x0008`).
async fn boot_read_pid<T: Transport>(transport: &T, timeout: Duration) -> Result<u16, Error> {
    let payload = boot_request_read(transport, &boot::read_pid(), 2, timeout, "readPID").await?;
    Ok(u16::from_le_bytes([payload[0], payload[1]]))
}

/// Read `readVersionInBoot` (`0x0005`, `offset = 0x01017040`).
async fn boot_read_version<T: Transport>(transport: &T, timeout: Duration) -> Result<u32, Error> {
    let payload = boot_request_read(
        transport,
        &boot::read_version(),
        4,
        timeout,
        "readVersionInBoot",
    )
    .await?;
    Ok(le_u32(&payload))
}

/// Send a bootloader read request and return its data payload.
///
/// A matching payload-less envelope (ready ACK) is skipped; unrelated frames
/// are skipped as well. A matching frame with a non-empty but unexpected
/// payload length fails closed instead of being truncated or padded.
async fn boot_request_read<T: Transport>(
    transport: &T,
    request: &commands::Request,
    expected_len: usize,
    timeout: Duration,
    description: &str,
) -> Result<Vec<u8>, Error> {
    transport.send(&boot::encode(request)?).await?;
    let deadline = deadline_after(timeout)?;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(response_timeout(description, timeout));
        }
        let wait = deadline.saturating_duration_since(now);
        let frame = match tokio::time::timeout(wait, transport.recv()).await {
            Ok(result) => result?,
            Err(_) => return Err(response_timeout(description, timeout)),
        };
        let (header, payload) = decode_boot_frame(&frame)?;
        if header.cmd == 0x000E {
            return Err(controller_error(description, payload));
        }
        if !boot_response_matches(&header, request) || payload.is_empty() {
            continue;
        }
        if payload.len() != expected_len {
            return Err(Error::UnexpectedResponse(format!(
                "{description} response carried {} payload bytes; expected {expected_len}",
                payload.len()
            )));
        }
        return Ok(payload.to_vec());
    }
}

/// Send a bootloader command and wait for its ACK.
async fn boot_send_and_await_ack<T: Transport>(
    transport: &T,
    request: &commands::Request,
    timeout: Duration,
    description: &str,
) -> Result<(), Error> {
    transport.send(&boot::encode(request)?).await?;
    let deadline = deadline_after(timeout)?;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(response_timeout(description, timeout));
        }
        let wait = deadline.saturating_duration_since(now);
        let frame = match tokio::time::timeout(wait, transport.recv()).await {
            Ok(result) => result?,
            Err(_) => return Err(response_timeout(description, timeout)),
        };
        let (header, payload) = decode_boot_frame(&frame)?;
        if header.cmd == 0x000E {
            return Err(controller_error(description, payload));
        }
        if boot_ack_matches(&header, request) {
            return Ok(());
        }
    }
}

/// Send a bootloader command without waiting for its ACK.
async fn boot_send_only<T: Transport>(
    transport: &T,
    request: &commands::Request,
) -> Result<(), Error> {
    transport.send(&boot::encode(request)?).await
}

/// Send JumpApp and report whether an ACK was seen.
///
/// The controller reboots into the application, so a link close is the normal
/// outcome and is not an error. Silence until the timeout is also accepted and
/// reported as unacknowledged; a transport error other than a disconnect is
/// returned.
async fn boot_jump_app<T: Transport>(
    transport: &T,
    request: &commands::Request,
    timeout: Duration,
) -> Result<bool, Error> {
    transport.send(&boot::encode(request)?).await?;
    let deadline = deadline_after(timeout)?;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        let wait = deadline.saturating_duration_since(now);
        match tokio::time::timeout(wait, transport.recv()).await {
            Ok(Ok(frame)) => {
                let Ok((header, payload)) = decode_boot_frame(&frame) else {
                    continue;
                };
                if header.cmd == 0x000E {
                    return Err(controller_error("JumpApp", payload));
                }
                if boot_ack_matches(&header, request) {
                    return Ok(true);
                }
            }
            Ok(Err(Error::Disconnected)) => return Ok(false),
            Ok(Err(error)) => return Err(error),
            Err(_) => return Ok(false),
        }
    }
}

fn decode_boot_frame(frame: &[u8]) -> Result<(CustomHeader, &[u8]), Error> {
    decode_app_response(frame)
        .map_err(|error| Error::UnexpectedResponse(format!("malformed bootloader frame: {error}")))
}

/// Read-response matcher: direct `cmd` echo or the `0x0004` envelope.
fn boot_response_matches(response: &CustomHeader, request: &commands::Request) -> bool {
    response.cmd == request.header.cmd
        || (response.cmd == cmd::RESPONSE
            && (response.subcmd == request.header.cmd || response.subcmd == request.header.subcmd))
}

/// ACK matcher: direct `cmd` echo or the `0x0004` envelope echoing the request
/// `cmd` in `subcmd`.
fn boot_ack_matches(response: &CustomHeader, request: &commands::Request) -> bool {
    if response.cmd == cmd::RESPONSE && response.subcmd == request.header.cmd {
        return true;
    }
    if response.cmd != request.header.cmd {
        return false;
    }
    // `EraseFlash` has `cmd == 0x0004`, the same wire value as the response
    // envelope. A direct echo is therefore only accepted with the request
    // sub-command (or the direct-ACK `0`); otherwise an unrelated `0x0004`
    // envelope would be mistaken for the erase ACK and the next frame could be
    // attributed to the wrong command.
    request.header.cmd != cmd::RESPONSE
        || response.subcmd == request.header.subcmd
        || response.subcmd == 0
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn invalid_firmware(reason: String) -> Error {
    Error::OtaInvalidFirmware(reason)
}

fn response_timeout(description: &str, timeout: Duration) -> Error {
    Error::Timeout {
        operation: format!("{description} response within {timeout:?}"),
    }
}

fn controller_error(description: &str, payload: &[u8]) -> Error {
    Error::UnexpectedResponse(format!(
        "controller returned error 0x000E while waiting for {description}: {payload:02X?}"
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Mutex, PoisonError};

    use async_trait::async_trait;
    use eightbitdo_protocol::crc::packet_crc16;
    use eightbitdo_protocol::header::{BLE_BOOT_PREFIX, HEADER_SIZE};

    use super::*;
    use crate::mock::MockTransport;

    const TEST_BASE: u32 = 0x0101_0000;
    const TEST_VERSION: u32 = 0x0102_0304;

    /// Build a valid `.dat` with a deterministic payload pattern.
    fn dat(image_len: u32, base: u32) -> Vec<u8> {
        let mut dat = vec![0u8; OTA_IMAGE_HEADER_LEN + image_len as usize];
        dat[0..4].copy_from_slice(&TEST_VERSION.to_le_bytes());
        dat[4..8].copy_from_slice(&base.to_le_bytes());
        dat[8..12].copy_from_slice(&image_len.to_le_bytes());
        for (offset, byte) in dat[OTA_IMAGE_HEADER_LEN..].iter_mut().enumerate() {
            *byte = (offset % 251) as u8;
        }
        dat
    }

    fn frame(cmd_id: u16, subcmd: u16, payload: &[u8]) -> Vec<u8> {
        let header =
            CustomHeader::new(cmd_id, subcmd, payload.len() as u32, 0).with_payload(payload);
        let mut frame = header.to_bytes().to_vec();
        frame.extend_from_slice(payload);
        frame
    }

    fn pid_response(pid: u16) -> Vec<u8> {
        frame(cmd::RESPONSE, boot::CMD_READ_PID, &pid.to_le_bytes())
    }

    fn version_response(version: u32) -> Vec<u8> {
        frame(
            cmd::RESPONSE,
            boot::CMD_READ_VERSION,
            &version.to_le_bytes(),
        )
    }

    fn ack(request: &commands::Request) -> Vec<u8> {
        frame(cmd::RESPONSE, request.header.cmd, &[])
    }

    fn queue_ack(mock: &MockTransport, request: &commands::Request) {
        mock.push_incoming(&ack(request));
    }

    /// Decode every sent bootloader frame into (header, payload).
    fn sent_boot(mock: &MockTransport) -> Vec<(CustomHeader, Vec<u8>)> {
        mock.sent_frames()
            .into_iter()
            .map(|frame| {
                assert_eq!(
                    frame.first().copied(),
                    Some(BLE_BOOT_PREFIX),
                    "every bootloader frame must start with the 0x05 prefix"
                );
                let header = CustomHeader::parse(&frame[1..]).unwrap();
                let payload = frame[1 + HEADER_SIZE..].to_vec();
                assert_eq!(payload.len(), header.chunk_len as usize);
                (header, payload)
            })
            .collect()
    }

    fn one_sector_image() -> Vec<u8> {
        dat(0x1000, TEST_BASE)
    }

    fn queue_prepare(mock: &MockTransport, pid: u16, version: u32) {
        mock.push_incoming(&pid_response(pid));
        mock.push_incoming(&version_response(version));
    }

    #[test]
    fn gamepad_type_maps_only_the_documented_pid() {
        assert_eq!(
            gamepad_type_from_pid(UT2_BOOT_PID),
            Some(GamepadType::Type1)
        );
        assert_eq!(gamepad_type_from_pid(0x6011), None);
        assert_eq!(gamepad_type_from_pid(0x0002), None);
        assert_eq!(gamepad_type_from_pid(0x0000), None);
    }

    #[test]
    fn firmware_image_parses_header_and_payload() {
        let bytes = dat(0x140, TEST_BASE);
        let image = FirmwareImage::parse(&bytes).unwrap();

        assert_eq!(image.version(), TEST_VERSION);
        assert_eq!(image.load_base(), TEST_BASE);
        assert_eq!(image.image_len(), 0x140);
        assert_eq!(image.image(), &bytes[OTA_IMAGE_HEADER_LEN..]);
        assert_eq!(image.version_header(), &bytes[..OTA_VERSION_HEADER_LEN]);
        assert_eq!(image.as_dat(), bytes.as_slice());
    }

    #[test]
    fn firmware_image_rejects_malformed_headers() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("short header", vec![0u8; OTA_IMAGE_HEADER_LEN - 1]),
            ("zero image_len", dat(0, TEST_BASE)),
            ("unaligned image_len", dat(0x41, TEST_BASE)),
            ("unaligned base", dat(0x80, TEST_BASE + 0x08)),
            ("truncated image", {
                let mut bytes = dat(0x80, TEST_BASE);
                bytes.truncate(OTA_IMAGE_HEADER_LEN + 0x40);
                bytes
            }),
            ("trailing second section", {
                let mut bytes = dat(0x80, TEST_BASE);
                bytes.push(0xAA);
                bytes
            }),
            ("address overflow", dat(0x100, 0xFFFF_FFC0)),
        ];
        for (name, bytes) in cases {
            let error = FirmwareImage::parse(&bytes).unwrap_err();
            assert!(
                matches!(error, Error::OtaInvalidFirmware(_)),
                "{name}: expected OtaInvalidFirmware, got {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn enter_boot_sends_the_documented_app_frame() {
        let mock = MockTransport::new();
        mock.push_incoming(&frame(cmd::RESPONSE, cmd::ENTER_BOOT, &[]));
        let session = Session::new(mock);
        let mut update = OtaUpdate::new();

        update.enter_boot(&session).await.unwrap();

        assert_eq!(update.phase(), OtaPhase::AwaitingBootReconnect);
        let sent = session.transport().sent_frames();
        assert_eq!(
            sent,
            vec![vec![
                0x04, // App Mode prefix
                0x0C, 0x00, // cmd = 0x000C EnterBoot
                0x67, 0x00, // subcmd = 0x0067
                0x00, 0x00, // chunk_len = 0
                0x00, 0x00, // crc16 = 0
                0x04, 0x00, 0x00, 0x00, // total_len = 4
                0x04, 0x00, 0x00, 0x00, // offset = 4
            ]]
        );
    }

    #[tokio::test]
    async fn enter_boot_accepts_the_app_link_closing() {
        let mock = MockTransport::new();
        let session = Session::new(mock);
        let mut update = OtaUpdate::new();

        update.enter_boot(&session).await.unwrap();

        assert_eq!(update.phase(), OtaPhase::AwaitingBootReconnect);
        assert_eq!(session.transport().sent_frames().len(), 1);
    }

    #[tokio::test]
    async fn prepare_boot_reads_pid_and_optional_version() {
        let mock = MockTransport::new();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let mut update = OtaUpdate::new();

        update.prepare_boot(&mock).await.unwrap();

        assert_eq!(update.phase(), OtaPhase::Ready);
        assert_eq!(update.pid(), Some(UT2_BOOT_PID));
        assert_eq!(update.gamepad_type(), Some(GamepadType::Type1));
        assert_eq!(update.boot_version(), Some(0x0000_0102));
        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0.cmd, boot::CMD_READ_PID);
        assert_eq!(sent[0].0.subcmd, 0x0067);
        assert_eq!(sent[0].0.total_len, 2);
        assert_eq!(sent[0].0.offset, 0);
        assert!(sent[0].1.is_empty());
        assert_eq!(sent[1].0.cmd, boot::CMD_READ_VERSION);
        assert_eq!(sent[1].0.subcmd, 0x0067);
        assert_eq!(sent[1].0.total_len, 4);
        assert_eq!(sent[1].0.offset, boot::VERSION_ADDR);
    }

    #[tokio::test]
    async fn prepare_boot_can_skip_the_version_read() {
        let mock = MockTransport::new();
        mock.push_incoming(&pid_response(UT2_BOOT_PID));
        let config = OtaConfig::default().with_read_version_in_boot(false);
        let mut update = OtaUpdate::with_config(config);

        update.prepare_boot(&mock).await.unwrap();

        assert_eq!(update.phase(), OtaPhase::Ready);
        assert_eq!(update.boot_version(), None);
        assert_eq!(sent_boot(&mock).len(), 1);
    }

    #[tokio::test]
    async fn prepare_boot_stops_on_an_unsupported_pid_before_any_write() {
        let mock = MockTransport::new();
        mock.push_incoming(&pid_response(0x6011));
        let mut update = OtaUpdate::new();

        let error = update.prepare_boot(&mock).await.unwrap_err();

        assert!(matches!(error, Error::OtaUnsupportedPid { pid: 0x6011 }));
        assert_eq!(update.phase(), OtaPhase::Failed);
        assert_eq!(update.pid(), Some(0x6011));
        assert_eq!(update.gamepad_type(), None);
        assert_eq!(update.boot_version(), None);
        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 1, "only readPID may be sent");
        assert_eq!(sent[0].0.cmd, boot::CMD_READ_PID);
    }

    #[tokio::test]
    async fn write_firmware_sends_the_full_one_sector_sequence() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::new();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes {
            queue_ack(&mock, request);
        }
        queue_ack(
            &mock,
            &boot::write_version_header(image.image_len(), image.version_header()).unwrap(),
        );
        queue_ack(&mock, &boot::jump_app());
        let mut update = OtaUpdate::new();
        update.prepare_boot(&mock).await.unwrap();

        let mut progress = Vec::new();
        let outcome = update
            .write_firmware(&mock, &image, &mut |snapshot| progress.push(snapshot))
            .await
            .unwrap();

        assert_eq!(update.phase(), OtaPhase::Complete);
        assert_eq!(outcome.pid, UT2_BOOT_PID);
        assert_eq!(outcome.gamepad_type, GamepadType::Type1);
        assert_eq!(outcome.boot_version, Some(0x0000_0102));
        assert_eq!(outcome.sectors_erased, 1);
        assert_eq!(outcome.chunks_written, 64);
        assert_eq!(outcome.bytes_written, 0x1000);
        assert_eq!(outcome.packets_sent, 70);
        assert!(outcome.jump_acknowledged);

        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 70);
        // readPID, readVersionInBoot, EraseFlash
        assert_eq!(sent[0].0.cmd, boot::CMD_READ_PID);
        assert_eq!(sent[1].0.cmd, boot::CMD_READ_VERSION);
        assert_eq!(sent[2].0.cmd, boot::CMD_ERASE_FLASH);
        assert_eq!(sent[2].0.subcmd, 0);
        assert_eq!(sent[2].0.total_len, OTA_SECTOR_SIZE);
        assert_eq!(sent[2].0.offset, TEST_BASE);
        assert!(sent[2].1.is_empty());

        // 64 write chunks with per-chunk CRC, increasing offsets and the full
        // image length in total_len.
        for (index, (header, payload)) in sent[3..3 + 64].iter().enumerate() {
            assert_eq!(header.cmd, boot::CMD_WRITE_FIRMWARE);
            assert_eq!(header.subcmd, 0);
            assert_eq!(header.chunk_len as usize, OTA_CHUNK_SIZE);
            assert_eq!(header.total_len, image.image_len());
            assert_eq!(
                header.offset,
                TEST_BASE + index as u32 * OTA_CHUNK_SIZE as u32
            );
            assert_eq!(header.crc16, packet_crc16(payload));
            assert_eq!(
                payload.as_slice(),
                &image.image()[index * OTA_CHUNK_SIZE..(index + 1) * OTA_CHUNK_SIZE]
            );
        }

        let (write_version, write_version_payload) = &sent[67];
        assert_eq!(write_version.cmd, boot::CMD_WRITE_VERSION);
        assert_eq!(write_version.subcmd, 0);
        assert_eq!(write_version.total_len, image.image_len());
        assert_eq!(write_version.offset, 0);
        assert_eq!(write_version.chunk_len as usize, OTA_VERSION_HEADER_LEN);
        assert_eq!(write_version_payload.as_slice(), image.version_header());

        let (firmware_version, firmware_version_payload) = &sent[68];
        assert_eq!(firmware_version.cmd, boot::CMD_WRITE_FIRMWARE_VERSION);
        assert_eq!(firmware_version.subcmd, 0);
        assert_eq!(firmware_version.total_len, 4);
        assert_eq!(firmware_version.offset, boot::VERSION_ADDR);
        assert_eq!(firmware_version.chunk_len, 4);
        assert_eq!(
            firmware_version.crc16,
            packet_crc16(firmware_version_payload)
        );
        assert_eq!(
            firmware_version_payload.as_slice(),
            &TEST_VERSION.to_le_bytes()
        );

        let (jump, jump_payload) = &sent[69];
        assert_eq!(jump.cmd, boot::CMD_JUMP_APP);
        assert_eq!(jump.subcmd, 0);
        assert_eq!(jump.total_len, 0);
        assert_eq!(jump.offset, boot::APP_ENTRY_ADDR);
        assert!(jump_payload.is_empty());

        assert_eq!(progress.first().unwrap().phase, OtaPhase::Erasing);
        assert_eq!(progress.first().unwrap().bytes_written, 0);
        assert!(progress.iter().any(|p| p.phase == OtaPhase::Writing));
        assert!(progress.iter().any(|p| p.phase == OtaPhase::Finalizing));
        let last = progress.last().unwrap();
        assert_eq!(last.phase, OtaPhase::Complete);
        assert_eq!(last.sector_count, 1);
        assert_eq!(last.chunk_count, 64);
        assert_eq!(last.bytes_written, 0x1000);
        assert_eq!(last.total_bytes, 0x1000);
    }

    #[tokio::test]
    async fn write_firmware_handles_sector_boundaries() {
        let image_bytes = dat(0x1040, TEST_BASE);
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::new();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes[..64] {
            queue_ack(&mock, request);
        }
        queue_ack(
            &mock,
            &boot::erase_flash(TEST_BASE + OTA_SECTOR_SIZE).unwrap(),
        );
        for request in &writes[64..] {
            queue_ack(&mock, request);
        }
        queue_ack(
            &mock,
            &boot::write_version_header(image.image_len(), image.version_header()).unwrap(),
        );
        queue_ack(&mock, &boot::jump_app());
        let mut update = OtaUpdate::new();
        update.prepare_boot(&mock).await.unwrap();

        let mut progress = Vec::new();
        let outcome = update
            .write_firmware(&mock, &image, &mut |snapshot| progress.push(snapshot))
            .await
            .unwrap();

        assert_eq!(outcome.sectors_erased, 2);
        assert_eq!(outcome.chunks_written, 65);
        assert_eq!(outcome.bytes_written, 0x1040);
        assert_eq!(outcome.packets_sent, 72);

        let sent = sent_boot(&mock);
        let erases: Vec<_> = sent
            .iter()
            .filter(|(header, _)| header.cmd == boot::CMD_ERASE_FLASH)
            .map(|(header, _)| header.offset)
            .collect();
        assert_eq!(erases, vec![TEST_BASE, TEST_BASE + OTA_SECTOR_SIZE]);
        let writes: Vec<_> = sent
            .iter()
            .filter(|(header, _)| header.cmd == boot::CMD_WRITE_FIRMWARE)
            .map(|(header, _)| header.offset)
            .collect();
        assert_eq!(writes.len(), 65);
        assert_eq!(writes[0], TEST_BASE);
        assert_eq!(writes[63], TEST_BASE + 63 * OTA_CHUNK_SIZE as u32);
        assert_eq!(writes[64], TEST_BASE + OTA_SECTOR_SIZE);
        assert_eq!(progress.last().unwrap().sector_count, 2);
        assert_eq!(progress.last().unwrap().bytes_written, 0x1040);
    }

    #[tokio::test]
    async fn erase_failure_stops_before_any_write() {
        let image_bytes = dat(0x1040, TEST_BASE);
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::new();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes[..64] {
            queue_ack(&mock, request);
        }
        // The second sector's EraseFlash is deliberately left unanswered.
        let mut update = OtaUpdate::new();
        update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(&mock, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::Erase,
                    state: OtaWriteState::EraseStarted,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
        assert_eq!(update.sectors_erased(), 1);
        assert_eq!(update.chunks_written(), 64);
        let sent = sent_boot(&mock);
        assert_eq!(
            sent.len(),
            68,
            "readPID, readVersion, erase, 64 writes, failing erase"
        );
        assert_eq!(sent.last().unwrap().0.cmd, boot::CMD_ERASE_FLASH);
        assert_eq!(sent.last().unwrap().0.offset, TEST_BASE + OTA_SECTOR_SIZE);
    }

    #[tokio::test]
    async fn write_failure_reports_a_partially_written_image() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::new();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes[..3] {
            queue_ack(&mock, request);
        }
        let mut update = OtaUpdate::new();
        update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(&mock, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::Write,
                    state: OtaWriteState::PartiallyWritten,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
        assert_eq!(update.chunks_written(), 3);
        assert_eq!(update.bytes_written(), 3 * OTA_CHUNK_SIZE as u32);
        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 7, "readPID, readVersion, erase, four writes");
        assert!(sent.iter().all(|(header, _)| {
            header.cmd != boot::CMD_WRITE_VERSION
                && header.cmd != boot::CMD_WRITE_FIRMWARE_VERSION
                && header.cmd != boot::CMD_JUMP_APP
        }));
    }

    #[tokio::test]
    async fn write_version_failure_reports_a_written_image() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::new();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes {
            queue_ack(&mock, request);
        }
        let mut update = OtaUpdate::new();
        update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(&mock, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::WriteVersion,
                    state: OtaWriteState::Written,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.chunks_written(), 64);
        let sent = sent_boot(&mock);
        assert_eq!(sent.last().unwrap().0.cmd, boot::CMD_WRITE_VERSION);
    }

    #[tokio::test]
    async fn jump_without_an_ack_is_not_a_failure() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::new();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes {
            queue_ack(&mock, request);
        }
        queue_ack(
            &mock,
            &boot::write_version_header(image.image_len(), image.version_header()).unwrap(),
        );
        let mut update = OtaUpdate::new();
        update.prepare_boot(&mock).await.unwrap();

        let outcome = update
            .write_firmware(&mock, &image, &mut |_| {})
            .await
            .unwrap();

        assert_eq!(update.phase(), OtaPhase::Complete);
        assert!(!outcome.jump_acknowledged);
        assert_eq!(outcome.bytes_written, 0x1000);
        assert_eq!(sent_boot(&mock).last().unwrap().0.cmd, boot::CMD_JUMP_APP);
    }

    #[tokio::test]
    async fn write_ack_timeout_fails_closed() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let transport = PendingAfter::new(vec![
            pid_response(UT2_BOOT_PID),
            version_response(0x0000_0102),
            ack(&boot::erase_flash(TEST_BASE).unwrap()),
        ]);
        let mut update = OtaUpdate::with_config(OtaConfig::default().with_timeouts(OtaTimeouts {
            write: Duration::from_millis(1),
            ..OtaTimeouts::default()
        }));
        update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(&transport, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::Write,
                    state: OtaWriteState::PartiallyWritten,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
        assert_eq!(update.chunks_written(), 0);
    }

    #[tokio::test]
    async fn erase_ack_timeout_fails_closed() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let transport = PendingAfter::new(vec![
            pid_response(UT2_BOOT_PID),
            version_response(0x0000_0102),
        ]);
        let mut update = OtaUpdate::with_config(OtaConfig::default().with_timeouts(OtaTimeouts {
            erase: Duration::from_millis(1),
            ..OtaTimeouts::default()
        }));
        update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(&transport, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::Erase,
                    state: OtaWriteState::EraseStarted,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.sectors_erased(), 0);
    }

    #[tokio::test]
    async fn write_requires_the_pid_gate() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::new();
        let mut update = OtaUpdate::new();

        let error = update
            .write_firmware(&mock, &image, &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)));
        assert_eq!(update.phase(), OtaPhase::Idle);
        assert!(mock.sent_frames().is_empty());

        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        update.prepare_boot(&mock).await.unwrap();
        let error = update.prepare_boot(&mock).await.unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)));

        let app_mock = MockTransport::new();
        let session = Session::new(app_mock);
        let error = update.enter_boot(&session).await.unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)));
        assert!(session.transport().sent_frames().is_empty());
    }

    #[tokio::test]
    async fn local_validation_happens_before_the_first_flash_frame() {
        let image_bytes = dat(0x1000, TEST_BASE + 0x40);
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::new();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let mut update = OtaUpdate::new();
        update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(&mock, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(matches!(error, Error::OtaInvalidFirmware(_)));
        assert_eq!(update.phase(), OtaPhase::Failed);
        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 2, "only readPID and readVersion may be sent");
        assert!(sent
            .iter()
            .all(|(header, _)| header.cmd != boot::CMD_ERASE_FLASH));
    }

    #[tokio::test]
    async fn read_skips_a_ready_ack_and_accepts_a_direct_echo() {
        let mock = MockTransport::new();
        mock.push_incoming(&frame(cmd::RESPONSE, boot::CMD_READ_PID, &[]));
        mock.push_incoming(&frame(
            boot::CMD_READ_PID,
            commands::subcmd::BOOT_READ_PID,
            &UT2_BOOT_PID.to_le_bytes(),
        ));
        let config = OtaConfig::default().with_read_version_in_boot(false);
        let mut update = OtaUpdate::with_config(config);

        update.prepare_boot(&mock).await.unwrap();

        assert_eq!(update.pid(), Some(UT2_BOOT_PID));
        assert_eq!(sent_boot(&mock).len(), 1);
    }

    #[tokio::test]
    async fn read_pid_rejects_an_unexpected_payload_length() {
        let mock = MockTransport::new();
        mock.push_incoming(&frame(
            cmd::RESPONSE,
            boot::CMD_READ_PID,
            &[0x0F, 0x60, 0x00, 0x00],
        ));
        let mut update = OtaUpdate::new();

        let error = update.prepare_boot(&mock).await.unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::ReadPid,
                    state: OtaWriteState::Untouched,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
        assert_eq!(update.pid(), None);
    }

    #[tokio::test]
    async fn enter_boot_timeout_fails_closed() {
        let transport = PendingAfter::new(Vec::new());
        let session = Session::new(transport);
        let mut update = OtaUpdate::with_config(OtaConfig::default().with_timeouts(OtaTimeouts {
            enter_boot: Duration::from_millis(1),
            ..OtaTimeouts::default()
        }));

        let error = update.enter_boot(&session).await.unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::EnterBoot,
                    state: OtaWriteState::Untouched,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
    }

    #[test]
    fn ack_matching_does_not_confuse_erase_with_the_response_envelope() {
        let erase = boot::erase_flash(TEST_BASE).unwrap();
        let erase_envelope = CustomHeader::new(cmd::RESPONSE, boot::CMD_ERASE_FLASH, 0, 0);
        let erase_direct = CustomHeader::new(boot::CMD_ERASE_FLASH, 0, 0, 0);
        let unrelated = CustomHeader::new(cmd::RESPONSE, boot::CMD_READ_PID, 0, 0);
        assert!(boot_ack_matches(&erase_envelope, &erase));
        assert!(boot_ack_matches(&erase_direct, &erase));
        assert!(!boot_ack_matches(&unrelated, &erase));

        let write =
            boot::write_firmware(TEST_BASE, OTA_CHUNK_SIZE as u32, &[0u8; OTA_CHUNK_SIZE]).unwrap();
        assert!(boot_ack_matches(
            &CustomHeader::new(cmd::RESPONSE, boot::CMD_WRITE_FIRMWARE, 0, 0),
            &write
        ));
        assert!(!boot_ack_matches(&erase_envelope, &write));
    }

    #[test]
    fn phase_terminal_flag() {
        assert!(!OtaPhase::Idle.is_terminal());
        assert!(!OtaPhase::Ready.is_terminal());
        assert!(OtaPhase::Complete.is_terminal());
        assert!(OtaPhase::Failed.is_terminal());
    }

    /// A transport that replays queued frames and then never answers again.
    struct PendingAfter {
        incoming: Mutex<VecDeque<Vec<u8>>>,
        sent: Mutex<Vec<Vec<u8>>>,
    }

    impl PendingAfter {
        fn new(incoming: Vec<Vec<u8>>) -> Self {
            Self {
                incoming: Mutex::new(incoming.into()),
                sent: Mutex::new(Vec::new()),
            }
        }

        fn lock_incoming(&self) -> std::sync::MutexGuard<'_, VecDeque<Vec<u8>>> {
            self.incoming.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    #[async_trait]
    impl Transport for PendingAfter {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            self.sent.lock().unwrap().push(data.to_vec());
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            let frame = self.lock_incoming().pop_front();
            match frame {
                Some(frame) => Ok(frame),
                None => std::future::pending().await,
            }
        }

        fn mtu(&self) -> u16 {
            64
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }
}
