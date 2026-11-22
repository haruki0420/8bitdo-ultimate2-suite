//! BLE OTA (firmware update) state machine over the `"8bt"` bootloader.
//!
//! This module implements the documented BLE OTA path only
//! (`docs/PROTOCOL_SPEC.md` §8). The Android v2.01 evidence behind the
//! unverified items is kept in [docs/OTA_BLE_EVIDENCE.md](../../../docs/OTA_BLE_EVIDENCE.md).
//!
//! ```text
//! App Mode "80ND"                    Bootloader "8bt" (service FF20 / char FF21)
//!   0x000C EnterBoot  ──┐
//!                       └─ BLE reconnect by the front end
//!                         0x0008 readPID            (5 s)
//!                         0x0005 readVersionInBoot  (5 s, default on)
//!                         0x0004 EraseFlash         (4 KB, 10 s, no retry)
//!                         0x0003 WriteFirmware      (64 B + CRC-16/MODBUS, 1 s, no retry)
//!                         0x00C4 WriteVersion       (24-byte header, 3 s, no retry)
//!                         0x0002 WriteFirmwareVersion (4 B, send-only)
//!                         0x0006 JumpApp            (ACK or link close; timeout fails)
//! ```
//!
//! # API contract
//!
//! 1. [`OtaUpdate::enter_boot`] sends `0x000C` on the App Mode [`Session`].
//! 2. The front end reconnects to `"8bt"` and keeps the boot [`Transport`].
//! 3. [`OtaUpdate::prepare_boot`] reads the PID, validates `gamepad_type`, and
//!    returns a [`BootReady`] guard that **borrows the boot transport**.
//! 4. [`OtaUpdate::write_firmware`] accepts only that guard, so the PID check
//!    and the flash writes cannot be performed on different links. The guard
//!    is also bound to its originating [`OtaUpdate`] at runtime.
//!
//! No flash frame is sent before `readPID` confirmed `gamepad_type == 1`, the
//! application base matches the caller-supplied expected base, the whole image
//! has been validated locally, and the negotiated MTU can carry the largest
//! write frame. Every failure is fail-closed ([`Error::Ota`]) with the failing
//! stage and how far the update got ([`OtaWriteState`]). There are **no
//! retries**: a timeout or refused frame stops the update immediately and
//! recovery is a fresh connection plus a whole-image erase/rewrite.
//!
//! # Unverified wire gate
//!
//! The bootloader RX shape, ready-vs-completion ACK distinction, erase/write
//! address convention and `WriteVersion` payload are not hardware-verified
//! (`docs/PROTOCOL_SPEC.md` §8.5). [`OtaConfig::allow_unverified_wire`]
//! therefore defaults to `false` and [`OtaUpdate::write_firmware`] refuses to
//! send any flash frame until a caller explicitly opts in with
//! [`OtaConfig::with_unverified_wire_allowed`]. **Do not use this path on real
//! hardware yet**; it exists for MockTransport tests and bench experiments on a
//! recoverable controller.
//!
//! # Trust boundary
//!
//! `FirmwareImage::parse` validates structure, not provenance. The caller must
//! supply `.dat` bytes from a trusted source and must also pass the expected
//! application base. The address allowlist is built in: the base must lie in
//! the provisional type-1 window
//! [`UT2_TYPE1_APP_BASE_MIN`]..[`UT2_TYPE1_APP_FLASH_END`] and
//! `base + image_len` must not exceed the window end. Both checks happen before
//! the first EraseFlash. Only single-section `.dat` files are accepted;
//! official firmware images may be multi-section, in which case the update
//! refuses instead of writing a partial image (**blocker** for official
//! firmware support, see §8.5).
//!
//! # Cancellation contract
//!
//! A dropped [`OtaUpdate::write_firmware`] future (Cargo timeout, UI cancel,
//! process shutdown) stops at the current await point without returning a
//! recovery error. The flash window is not transactional: the controller may
//! be erased or partially written. A front end must therefore make the erase /
//! write / finalize window non-cancellable, and after any forced cancellation
//! it must reconnect to `"8bt"`, read the PID, and rewrite the whole image.
//!
//! # JumpApp
//!
//! Success requires an ACK or an observed link close. A timeout is a failure
//! (`OtaFailureStage::Jump`, [`OtaWriteState::Written`]) with recovery guidance
//! to check whether the application came back and verify its version; a silent
//! link is not treated as a successful jump.
//!
//! # USB is not implemented
//!
//! USB Boot TX framing (`EraseFlash` / `WriteFirmware` / `WriteVersion` /
//! `WriteAPPID` / `JumpApp`) is undocumented (`docs/PROTOCOL_SPEC.md` §8.4,
//! `AGENTS.md` §4.4). `[0x00, 0x05]` is an RX-only prefix and must not be
//! reused as TX framing, so this module implements **BLE only**. Only types
//! implementing the sealed [`OtaTransport`] marker can be passed to the boot
//! phase; the marker is implemented only for
//! [`MockTransport`](crate::mock::MockTransport) and the boot-specific
//! [`BleBootTransport`](crate::ble::BleBootTransport). The App Mode
//! [`BleTransport`](crate::ble::BleTransport) (`"80ND"` / `FF10`) cannot reach
//! `"8bt"` and is deliberately excluded. USB OTA is `N/A` until the spec
//! carries framing evidence, and USB recovery is re-enumeration /
//! reconnection, never the BLE `"8bt"` reconnect.
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
//!   skipped before read data.
//! * The EnterBoot ACK or the App link close both count as "reboot requested";
//!   neither leads to a flash write until `readPID` succeeds on `"8bt"`.
//! * EraseFlash addresses follow `PROTOCOL_SPEC` §8.3
//!   (`load_base_addr + sector_off`), where `load_base_addr` comes from
//!   `.dat +0x04`. `docs/OTA_BLE_EVIDENCE.md` §3 reports that the Android
//!   implementation puts the image-relative `sector_off` on the wire instead;
//!   this conflict is recorded as unverified and the spec form is implemented.
//! * `WriteVersion` sends the documented 24-byte image header.
//!   `docs/OTA_BLE_EVIDENCE.md` §2 reports the Android implementation sends a
//!   16-byte frame with no payload; again unverified.
//! * The boot link takes the same single-owner BLE lock as the App Mode link.
//!   Drop (or disconnect) the App Mode [`Session`] **before**
//!   `BleBootTransport::connect`; otherwise the boot connect returns
//!   [`Error::BleBusy`]. [`OtaUpdate::enter_boot_and_release`] performs the
//!   EnterBoot step and then closes the App link for this reason.
//! * Ready-vs-completion ACK shapes are unverified. With
//!   [`OtaConfig::require_completion_ack`] enabled, the first payload-less
//!   `cmd 0x0004` matching frame is treated as a ready ACK and skipped for
//!   EraseFlash / WriteFirmware / WriteVersion; the option is off by default
//!   until btsnoop evidence exists. Because EraseFlash's request `cmd` is also
//!   `0x0004`, a payload-less direct echo (`cmd=0x0004, subcmd=0`) is
//!   indistinguishable from the envelope and is also skipped as a ready ACK in
//!   that mode. For other commands a direct `cmd` echo is not `0x0004` and
//!   completes immediately.
//! * Bootloader ACKs carry no sequence number. A duplicated or delayed ACK for
//!   a previous WriteFirmware chunk can therefore be matched as the ACK of the
//!   next chunk, which would advance the write before the previous chunk is
//!   known to be committed. `require_completion_ack` consumes two matching
//!   frames per command and reduces, but does not eliminate, this risk. Do not
//!   run the flash path on hardware until btsnoop confirms the ACK ordering.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use eightbitdo_protocol::commands::{self, boot, cmd};
use eightbitdo_protocol::header::{decode_app_response, CustomHeader, HEADER_SIZE};
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

/// Maximum wait for evidence that `0x0006` JumpApp took effect (ACK or link
/// close). A timeout is a failure, not a fire-and-forget success.
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

/// Largest bootloader frame this module sends: the 64-byte WriteFirmware frame
/// (`1` prefix + `16` header + `64` payload = `81` bytes).
pub const OTA_MAX_FRAME_LEN: usize = 1 + HEADER_SIZE + OTA_CHUNK_SIZE;

/// Minimum negotiated ATT MTU needed before the first EraseFlash.
///
/// The protocol's usable payload per write is `mtu - 3`, so the 81-byte
/// WriteFirmware frame needs `mtu >= 84`. Checked before the first flash frame
/// so an avoidable erase cannot happen on a link that cannot carry the writes.
pub const OTA_MIN_MTU: u16 = (OTA_MAX_FRAME_LEN + 3) as u16;

/// Provisional type-1 application window start.
///
/// Derived from the only documented type-1 addresses: `readVersionInBoot` /
/// `WriteFirmwareVersion` at `0x01017040` and the `JumpApp` entry at
/// `0x01018000` (`docs/PROTOCOL_SPEC.md` §8.3). The official `.dat`
/// `load_base_addr` has **not been measured**, so this 64 KiB-aligned window
/// (`0x01010000..0x01020000`) is a conservative allowlist that contains both
/// documented addresses and excludes address 0 / the bootloader area / the
/// type-0 `0x0801_xxxx` region. It is provisional and must be confirmed from an
/// official `.dat` and a real device before CLI wiring.
pub const UT2_TYPE1_APP_BASE_MIN: u32 = 0x0101_0000;

/// Provisional exclusive end of the type-1 application flash window.
///
/// See [`UT2_TYPE1_APP_BASE_MIN`]. `expected_load_base + image_len` must not
/// exceed this address, so the last EraseFlash/WriteFirmware sector cannot
/// leave the application window.
pub const UT2_TYPE1_APP_FLASH_END: u32 = 0x0102_0000;

/// Marker for transports that may carry the BLE `"8bt"` bootloader protocol.
///
/// The boot phase is BLE-only: USB Boot TX framing is undocumented, so a USB
/// transport must not be passed to [`OtaUpdate::prepare_boot`] /
/// [`OtaUpdate::write_firmware`]. The marker is sealed (downstream crates
/// cannot implement it) and is implemented only inside this crate for
/// [`MockTransport`](crate::mock::MockTransport) and the boot-specific
/// [`BleBootTransport`](crate::ble::BleBootTransport) (`"8bt"`, service `FF20`,
/// characteristic `FF21`/`FF23`).
///
/// The App Mode [`BleTransport`](crate::ble::BleTransport) is fixed to
/// `"80ND"` / service `FF10` and cannot reach the bootloader, so it is
/// deliberately **not** an `OtaTransport`. Desktop front ends must use
/// [`BleBootTransport`](crate::ble::BleBootTransport).
///
/// For native mobile transports the sealed marker cannot be implemented by
/// `eightbitdo-uniffi`; use the hidden [`MobileBootLink::from_native`] adapter
/// instead. That adapter accepts any [`Transport`], so the boot framing /
/// ownership / lock contract becomes the caller's responsibility; **USB and
/// App Mode links are not excluded by the type system** on that path.
pub trait OtaTransport: Transport + sealed::Sealed {}

mod sealed {
    /// Prevents downstream crates from implementing [`OtaTransport`].
    pub trait Sealed {}
}

impl sealed::Sealed for crate::mock::MockTransport {}
impl OtaTransport for crate::mock::MockTransport {}

#[cfg(feature = "ble")]
impl sealed::Sealed for crate::ble::BleBootTransport {}
#[cfg(feature = "ble")]
impl OtaTransport for crate::ble::BleBootTransport {}

/// Mobile-only adapter for a boot transport supplied by another crate
/// (UniFFI / native CoreBluetooth / Android BluetoothGatt).
///
/// [`OtaTransport`] is sealed, so downstream crates cannot implement it.
/// `MobileBootLink` is the **documented escape hatch for native mobile
/// transports**: the crate implements the marker for this wrapper, and the
/// caller injects its own [`Transport`]. It is `#[doc(hidden)]` because desktop
/// front ends must use [`BleBootTransport`](crate::ble::BleBootTransport),
/// which resolves `"8bt"` / `FF20` / `FF21` and takes the single-owner BLE lock.
///
/// # Trust contract (caller-owned)
///
/// `MobileBootLink` cannot verify what it wraps: a [`Transport`] is just
/// bytes. Wrapping the App Mode `BleTransport`, a USB link, or any other
/// transport is **not prevented by the type system** and is a caller error.
/// The wrapped transport must:
///
/// * be connected to the advertised name `"8bt"`, service `FF20`, and
///   characteristic `FF21` or `FF23`;
/// * write the bytes it is given unchanged (the core already prepends the
///   `0x05` bootloader prefix; the native layer must not add or strip framing);
/// * return one complete notification frame per [`Transport::recv`], starting
///   with the 16-byte header (no prefix byte);
/// * report the negotiated ATT MTU from [`Transport::mtu`] (`mtu - 3` usable);
/// * be owned exclusively by this update, and hold its own single-owner lock
///   (or equivalent) so no App Mode session / other process races it. The
///   adapter does **not** acquire the BLE lock and does not close the App link.
#[doc(hidden)]
pub struct MobileBootLink<T: Transport>(T);

impl<T: Transport> MobileBootLink<T> {
    /// Wrap a native boot transport for UniFFI / mobile integration.
    ///
    /// The caller owns the trust contract in the [`MobileBootLink`] docs.
    #[must_use]
    pub const fn from_native(transport: T) -> Self {
        Self(transport)
    }

    /// Consume the adapter and return the wrapped transport.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: Transport> sealed::Sealed for MobileBootLink<T> {}

impl<T: Transport> OtaTransport for MobileBootLink<T> {}

#[async_trait]
impl<T: Transport> Transport for MobileBootLink<T> {
    async fn send(&self, data: &[u8]) -> Result<(), Error> {
        self.0.send(data).await
    }

    async fn recv(&self) -> Result<Vec<u8>, Error> {
        self.0.recv().await
    }

    fn mtu(&self) -> u16 {
        self.0.mtu()
    }

    async fn disconnect(&self) -> Result<(), Error> {
        self.0.disconnect().await
    }
}

/// The controller generation whose BLE OTA offsets are documented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadType {
    /// `gamepad_type == 1` (Ultimate 2 Bluetooth, PID `0x600F`): the
    /// `readVersionInBoot` / `WriteFirmwareVersion` offset `0x01017040` and the
    /// `JumpApp` entry `0x01018000` are documented.
    Type1,
}

/// How the `0x0006` JumpApp transition was confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpOutcome {
    /// The bootloader acknowledged JumpApp before the link closed.
    Acknowledged,
    /// The link closed after JumpApp was sent, which is the expected behavior
    /// when the controller reboots into the application.
    LinkClosed,
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
    /// requested). A [`BootReady`] guard exists; no flash frame has been sent.
    Ready,
    /// EraseFlash is being acknowledged (or its ACK is awaited).
    Erasing,
    /// WriteFirmware chunks are being acknowledged.
    Writing,
    /// All chunks are acknowledged; WriteVersion / WriteFirmwareVersion /
    /// JumpApp are running.
    Finalizing,
    /// JumpApp was acknowledged or the link closed after it was sent. The
    /// controller should reboot into the application; verify the version.
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
    /// Maximum wait for JumpApp evidence (ACK or link close). A timeout fails.
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
    /// Expected application `load_base_addr` from the trusted `.dat`.
    ///
    /// Mandatory trust boundary: an image whose header names any other base is
    /// refused before the first EraseFlash, so a malformed `load_base = 0`
    /// cannot erase the bootloader area. Must be inside the provisional type-1
    /// window [`UT2_TYPE1_APP_BASE_MIN`]..[`UT2_TYPE1_APP_FLASH_END`] and 4 KB
    /// aligned; `expected_load_base + image_len` must also stay inside the
    /// window.
    expected_load_base: u32,
    /// ACK timeouts.
    timeouts: OtaTimeouts,
    /// Read `readVersionInBoot` (`0x0005`) after the PID check.
    ///
    /// Defaults to `true` (the documented sequence). It is read-only and its
    /// result is reported in [`OtaOutcome::boot_version`]; a failure aborts
    /// before any flash write.
    read_version_in_boot: bool,
    /// Treat the first payload-less `cmd 0x0004` matching frame for
    /// EraseFlash / WriteFirmware / WriteVersion as a ready ACK and wait for a
    /// completion ACK.
    ///
    /// Off by default because the bootloader ACK shapes are unverified. When
    /// enabled, a device that emits only one ACK will time out (fail-closed);
    /// when disabled, the first matching ACK completes the command (current
    /// documented behavior). For EraseFlash the direct echo (`cmd=0x0004,
    /// subcmd=0`) shares the `cmd` value with the envelope, so it is also
    /// treated as a ready ACK in this mode; for the other commands a direct
    /// echo completes immediately.
    require_completion_ack: bool,
    /// Allow flash writes while the bootloader wire shape is unverified.
    ///
    /// `false` (default) makes [`OtaUpdate::write_firmware`] refuse before any
    /// flash frame. Only MockTransport tests and bench experiments on a
    /// recoverable controller should opt in; see the module docs.
    allow_unverified_wire: bool,
}

impl OtaConfig {
    /// Expected application base from the trusted `.dat`.
    #[must_use]
    pub const fn expected_load_base(&self) -> u32 {
        self.expected_load_base
    }

    /// ACK timeouts.
    #[must_use]
    pub const fn timeouts(&self) -> OtaTimeouts {
        self.timeouts
    }

    /// Whether `readVersionInBoot` is sent (default `true`).
    #[must_use]
    pub const fn read_version_in_boot(&self) -> bool {
        self.read_version_in_boot
    }

    /// Whether the first payload-less ready ACK is skipped for the flash
    /// commands (Erase / Write / WriteVersion).
    #[must_use]
    pub const fn require_completion_ack(&self) -> bool {
        self.require_completion_ack
    }

    /// Whether flash writes are allowed while the wire shape is unverified.
    #[must_use]
    pub const fn allow_unverified_wire(&self) -> bool {
        self.allow_unverified_wire
    }

    /// Create a configuration with a mandatory expected application base.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OtaInvalidFirmware`] when `expected_load_base` is `0`
    /// or not 4 KB aligned. Both would make EraseFlash target an unvalidated
    /// region.
    pub fn new(expected_load_base: u32) -> Result<Self, Error> {
        validate_expected_load_base(expected_load_base)?;
        Ok(Self {
            expected_load_base,
            timeouts: OtaTimeouts::default(),
            read_version_in_boot: true,
            require_completion_ack: false,
            allow_unverified_wire: false,
        })
    }

    /// Replace the ACK timeouts.
    #[must_use]
    pub fn with_timeouts(mut self, timeouts: OtaTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Choose whether `readVersionInBoot` is sent (default `true`).
    #[must_use]
    pub fn with_read_version_in_boot(mut self, enabled: bool) -> Self {
        self.read_version_in_boot = enabled;
        self
    }

    /// Require a completion ACK after a payload-less ready ACK.
    #[must_use]
    pub fn with_completion_ack_required(mut self, required: bool) -> Self {
        self.require_completion_ack = required;
        self
    }

    /// Acknowledge that the bootloader wire shape is unverified.
    ///
    /// Only enable this for MockTransport tests or a bench experiment with a
    /// recoverable controller.
    #[must_use]
    pub fn with_unverified_wire_allowed(mut self, allowed: bool) -> Self {
        self.allow_unverified_wire = allowed;
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
    /// Number of 4 KB sectors that cover the image (`ceil(image_len / 4096)`).
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
    /// 4 KB sectors whose erase was acknowledged.
    pub sectors_erased: u32,
    /// 64-byte chunks whose write was acknowledged.
    pub chunks_written: u32,
    /// Image bytes acknowledged (equals the `.dat` `image_len`).
    pub bytes_written: u32,
    /// Bootloader frames sent (readPID through JumpApp).
    pub packets_sent: u32,
    /// How the JumpApp reboot was confirmed.
    pub jump: JumpOutcome,
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
    /// This validates the file structure only. The 4 KB alignment needed by
    /// EraseFlash and the application-region allowlist
    /// ([`OtaConfig::expected_load_base`]) are checked by
    /// [`OtaUpdate::write_firmware`] before the first flash frame.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OtaInvalidFirmware`] when the file is shorter than the
    /// 28-byte header, `image_data_len` is zero or not a multiple of 64, the
    /// base is not 64-byte aligned, `base + image_len` overflows `u32`, the
    /// file is shorter than `28 + image_len` (truncated), or the file carries
    /// trailing data. Trailing data is rejected because this implementation
    /// supports **single-section `.dat` files only**; official multi-section
    /// firmware is not supported yet (see `docs/PROTOCOL_SPEC.md` §8.5).
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
                 like a multi-section .dat, which this implementation does not support (only a \
                 single-section .dat is accepted; official firmware may be multi-section).",
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

/// Proof that `readPID` succeeded on one bootloader transport.
///
/// Returned by [`OtaUpdate::prepare_boot`] and required by
/// [`OtaUpdate::write_firmware`]. The guard borrows the transport that was
/// used for the PID check, so the write phase cannot be moved to a different
/// link. Its constructor is private and it is bound to the originating
/// [`OtaUpdate`], so a guard from another update is rejected at runtime.
///
/// Dropping the guard without calling [`OtaUpdate::write_firmware`] abandons
/// the attempt (the phase stays [`OtaPhase::Ready`] because no flash frame was
/// sent); create a new [`OtaUpdate`] and call `prepare_boot` again to retry.
pub struct BootReady<'a, T: OtaTransport> {
    transport: &'a T,
    owner: u64,
    pid: u16,
    gamepad_type: GamepadType,
    boot_version: Option<u32>,
}

impl<T: OtaTransport> BootReady<'_, T> {
    /// PID reported by the bootloader.
    #[must_use]
    pub const fn pid(&self) -> u16 {
        self.pid
    }

    /// Documented gamepad type of [`BootReady::pid`].
    #[must_use]
    pub const fn gamepad_type(&self) -> GamepadType {
        self.gamepad_type
    }

    /// Bootloader version read by `readVersionInBoot`, if enabled.
    #[must_use]
    pub const fn boot_version(&self) -> Option<u32> {
        self.boot_version
    }
}

impl<T: OtaTransport> std::fmt::Debug for BootReady<'_, T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BootReady")
            .field("pid", &self.pid)
            .field("gamepad_type", &self.gamepad_type)
            .field("boot_version", &self.boot_version)
            .finish_non_exhaustive()
    }
}

/// Recovery text for a failure before any flash frame.
const RECOVERY_READ_ONLY: &str =
    "nothing was erased or written. Start recovery with a new OtaUpdate (phase Idle) and call \
     prepare_boot on the \"8bt\" transport, or power-cycle the controller and retry from the App \
     Mode link if \"8bt\" is no longer advertised.";

/// Recovery text for an EnterBoot failure.
const RECOVERY_ENTER_BOOT: &str =
    "nothing was erased or written. Reconnect to the App Mode peripheral (\"80ND\"), confirm the \
     controller is idle, and retry EnterBoot; if the controller already advertises \"8bt\", create \
     a new OtaUpdate (phase Idle) and start from prepare_boot.";

/// Recovery text for a failed App-link teardown after EnterBoot.
const RECOVERY_ENTER_BOOT_RELEASE: &str =
    "EnterBoot was accepted but the App Mode link could not be closed. The boot link takes the \
     same single-owner BLE lock as App Mode, so disconnect/drop the App Mode session (and any GUI \
     or session process holding the lock) before connecting to \"8bt\"; then create a new \
     OtaUpdate (phase Idle) and call prepare_boot.";

/// Recovery text for an EraseFlash failure.
const RECOVERY_ERASE: &str =
    "flash erase was sent but not acknowledged, so the application area may be erased. Start \
     recovery with a new OtaUpdate (phase Idle) and call prepare_boot on \"8bt\", then erase and \
     rewrite every sector from offset 0; do not send JumpApp.";

/// Recovery text for a WriteFirmware failure.
const RECOVERY_WRITE: &str =
    "the image is partially written. Start recovery with a new OtaUpdate (phase Idle) and call \
     prepare_boot on \"8bt\", then erase every sector and rewrite the whole image from offset 0; \
     do not send JumpApp until the rewrite completes.";

/// Recovery text for a finalization failure.
const RECOVERY_FINALIZE: &str =
    "every firmware chunk was acknowledged, but finalization failed. Start recovery with a new \
     OtaUpdate (phase Idle) and call prepare_boot on \"8bt\", then erase and rewrite the whole \
     image again before finalizing and sending JumpApp.";

/// Recovery text for a JumpApp failure.
const RECOVERY_JUMP: &str =
    "JumpApp was sent but neither an ACK nor a link close was observed, so the jump is \
     unconfirmed and the controller may still be in the bootloader. Start recovery with a new \
     OtaUpdate (phase Idle) and call prepare_boot on \"8bt\"; if the controller instead \
     advertises \"80ND\", reconnect to it and verify the firmware version with the App Mode \
     version read. If the application is not running correctly, rewrite the whole image from \
     offset 0. Do not repeat JumpApp blindly.";

/// BLE OTA state machine.
///
/// Construct one per update attempt:
///
/// 1. [`OtaUpdate::enter_boot`] on the App Mode [`Session`] (`0x000C`),
/// 2. close/drop the App Mode [`Session`] and reconnect to `"8bt"` (service
///    FF20 / characteristic FF21); the boot link shares the single-owner BLE
///    lock with App Mode, so a live App session yields [`Error::BleBusy`].
///    [`OtaUpdate::enter_boot_and_release`] combines steps 1 and the teardown,
/// 3. [`OtaUpdate::prepare_boot`] (`readPID`, `gamepad_type` check, optional
///    `readVersionInBoot`) returns a [`BootReady`] guard,
/// 4. [`OtaUpdate::write_firmware`] with that guard (erase / write / finalize /
///    jump).
///
/// If the controller is already in the bootloader (recovery after a failed
/// update), [`OtaUpdate::prepare_boot`] may be called from [`OtaPhase::Idle`]
/// without [`OtaUpdate::enter_boot`]. A failed update marks the phase
/// [`OtaPhase::Failed`] and cannot continue; create a new `OtaUpdate` for the
/// recovery attempt.
///
/// Flash writes are refused unless [`OtaConfig::allow_unverified_wire`] is
/// explicitly enabled (see the module docs).
pub struct OtaUpdate {
    id: u64,
    config: OtaConfig,
    phase: OtaPhase,
    pid: Option<u16>,
    gamepad_type: Option<GamepadType>,
    boot_version: Option<u32>,
    sectors_erased: u32,
    erase_frames_sent: u32,
    write_frames_sent: u32,
    chunks_written: u32,
    bytes_written: u32,
    packets_sent: u32,
    image_len: u32,
}

static NEXT_UPDATE_ID: AtomicU64 = AtomicU64::new(1);

impl OtaUpdate {
    /// Create an update with the documented defaults and a mandatory expected
    /// application base.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OtaInvalidFirmware`] when `expected_load_base` is `0`
    /// or not 4 KB aligned.
    pub fn new(expected_load_base: u32) -> Result<Self, Error> {
        Self::with_config(OtaConfig::new(expected_load_base)?)
    }

    /// Create an update with an explicit configuration.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OtaInvalidFirmware`] when the configuration's expected
    /// application base is outside the provisional type-1 window or not 4 KB
    /// aligned. The same invariant is re-checked before the first EraseFlash.
    pub fn with_config(config: OtaConfig) -> Result<Self, Error> {
        validate_expected_load_base(config.expected_load_base)?;
        Ok(Self {
            id: NEXT_UPDATE_ID.fetch_add(1, Ordering::Relaxed),
            config,
            phase: OtaPhase::Idle,
            pid: None,
            gamepad_type: None,
            boot_version: None,
            sectors_erased: 0,
            erase_frames_sent: 0,
            write_frames_sent: 0,
            chunks_written: 0,
            bytes_written: 0,
            packets_sent: 0,
            image_len: 0,
        })
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

    /// 4 KB sectors whose erase was acknowledged so far.
    #[must_use]
    pub const fn sectors_erased(&self) -> u32 {
        self.sectors_erased
    }

    /// 64-byte chunks whose write was acknowledged so far.
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
    /// The App Mode session **must be dropped/disconnected before**
    /// [`BleBootTransport::connect`](crate::ble::BleBootTransport::connect):
    /// both links take the same single-owner BLE lock, so a live App session
    /// makes the boot connect fail with [`Error::BleBusy`]. Use
    /// [`OtaUpdate::enter_boot_and_release`] to do both steps in one call.
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

    /// Send EnterBoot, then consume the App Mode [`Session`] and close its link.
    ///
    /// This is the recommended pairing for a real update: EnterBoot makes the
    /// controller reboot, and the App Mode link must be closed before
    /// [`BleBootTransport::connect`](crate::ble::BleBootTransport::connect) can
    /// take the single-owner BLE lock for `"8bt"` (otherwise it returns
    /// [`Error::BleBusy`]). The helper calls [`OtaUpdate::enter_boot`], then
    /// [`Session::into_inner`] and [`Transport::disconnect`].
    ///
    /// # Errors
    ///
    /// Same as [`OtaUpdate::enter_boot`], or [`Error::Ota`] with stage
    /// [`OtaFailureStage::EnterBoot`] when the App link teardown fails. In that
    /// case the boot link cannot take the lock; close the App session and
    /// restart from a new `OtaUpdate`.
    pub async fn enter_boot_and_release<T: Transport>(
        &mut self,
        app: Session<T>,
    ) -> Result<(), Error> {
        self.enter_boot(&app).await?;
        let transport = app.into_inner();
        if let Err(error) = transport.disconnect().await {
            return Err(self.fail(
                OtaFailureStage::EnterBoot,
                OtaWriteState::Untouched,
                error,
                RECOVERY_ENTER_BOOT_RELEASE,
            ));
        }
        Ok(())
    }

    /// Read the bootloader PID, verify `gamepad_type`, and optionally read the
    /// bootloader version.
    ///
    /// This is the mandatory gate before [`OtaUpdate::write_firmware`]: a
    /// non-type-1 PID stops with [`Error::OtaUnsupportedPid`] without guessing
    /// the `readVersionInBoot` offset and without sending any flash frame.
    ///
    /// The returned [`BootReady`] guard borrows `boot_transport`; pass it to
    /// [`OtaUpdate::write_firmware`] so the PID check and the writes use the
    /// same link.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] unless the phase is
    /// [`OtaPhase::Idle`] (already-booted recovery) or
    /// [`OtaPhase::AwaitingBootReconnect`]; [`Error::OtaUnsupportedPid`] for a
    /// PID whose OTA offsets are undocumented; or [`Error::Ota`] with stage
    /// [`OtaFailureStage::ReadPid`] / [`OtaFailureStage::ReadVersion`] when a
    /// read fails.
    pub async fn prepare_boot<'a, T: OtaTransport>(
        &mut self,
        boot_transport: &'a T,
    ) -> Result<BootReady<'a, T>, Error> {
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
            return Err(Error::OtaUnsupportedPid {
                pid,
                state: OtaWriteState::Untouched,
            });
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
        Ok(BootReady {
            transport: boot_transport,
            owner: self.id,
            pid,
            gamepad_type,
            boot_version: self.boot_version,
        })
    }

    /// Erase, write, finalize, and jump to the application.
    ///
    /// Every frame is built and validated before the first EraseFlash is sent.
    /// Per sector the order is EraseFlash → its ACK → 64 WriteFirmware chunks
    /// (one ACK each) → next sector. After the last chunk the order is
    /// WriteVersion → WriteFirmwareVersion → JumpApp
    /// (`docs/PROTOCOL_SPEC.md` §8.4). No command is retried.
    ///
    /// `progress` is called before each sector erase, after every acknowledged
    /// chunk, when finalization starts, and on completion. The initial
    /// [`OtaPhase::Erasing`] callback carries `bytes_written = 0`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] when `ready` belongs to another
    /// update, the phase is not [`OtaPhase::Ready`], or the unverified-wire
    /// gate is closed; [`Error::OtaInvalidFirmware`] when the image base does
    /// not match [`OtaConfig::expected_load_base`]; [`Error::MtuTooSmall`] when
    /// the link cannot carry an 81-byte WriteFirmware frame (checked before the
    /// first EraseFlash); or [`Error::Ota`] with the failing stage and
    /// [`OtaWriteState`].
    pub async fn write_firmware<T: OtaTransport>(
        &mut self,
        ready: BootReady<'_, T>,
        image: &FirmwareImage<'_>,
        progress: &mut dyn FnMut(OtaProgress),
    ) -> Result<OtaOutcome, Error> {
        if ready.owner != self.id {
            return Err(Error::InvalidRequest(
                "the BootReady guard belongs to a different OtaUpdate; call prepare_boot on this \
                 update against the same boot transport"
                    .to_string(),
            ));
        }
        if self.phase != OtaPhase::Ready {
            return Err(Error::InvalidRequest(format!(
                "write_firmware requires the Ready phase after prepare_boot (current phase is \
                 {:?})",
                self.phase
            )));
        }
        if !self.config.allow_unverified_wire {
            self.phase = OtaPhase::Failed;
            return Err(Error::InvalidRequest(
                "refusing BLE OTA flash writes: the bootloader RX shape, ready/completion ACK \
                 distinction, erase/write address convention and WriteVersion payload are \
                 unverified (docs/PROTOCOL_SPEC.md §8.5). Set \
                 OtaConfig::with_unverified_wire_allowed(true) only for MockTransport tests or a \
                 bench experiment on a recoverable controller."
                    .to_string(),
            ));
        }
        // Re-validate the config value and the image range before any frame:
        // `expected_load_base + image_len` must stay inside the provisional
        // type-1 application window, so the last erase/write cannot leave it.
        if let Err(error) = validate_image_range(self.config.expected_load_base, image.image_len())
        {
            self.phase = OtaPhase::Failed;
            return Err(error);
        }
        if image.load_base() != self.config.expected_load_base {
            return Err(self.invalid_firmware(format!(
                "load_base_addr 0x{:08X} does not match the expected application base 0x{:08X}; \
                 refusing to erase an unvalidated flash region (a zero or bootloader base would \
                 be destructive)",
                image.load_base(),
                self.config.expected_load_base
            )));
        }

        let boot_transport = ready.transport;
        let mtu = boot_transport.mtu();
        let usable_payload = usize::from(mtu.saturating_sub(3));
        if usable_payload < OTA_MAX_FRAME_LEN {
            self.phase = OtaPhase::Failed;
            return Err(Error::MtuTooSmall {
                mtu,
                reason: format!(
                    "a WriteFirmware frame is {OTA_MAX_FRAME_LEN} bytes (1-byte prefix + 16-byte \
                     header + {OTA_CHUNK_SIZE}-byte chunk) but MTU {mtu} allows only \
                     {usable_payload} bytes; refusing to erase before a write that cannot fit"
                ),
            });
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
        let require_completion_ack = self.config.require_completion_ack;

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
            self.erase_frames_sent += 1;
            if let Err(error) = boot_send_and_await_ack(
                boot_transport,
                &erase,
                self.config.timeouts.erase,
                "EraseFlash",
                require_completion_ack,
            )
            .await
            {
                let state = if self.write_frames_sent > 0 {
                    OtaWriteState::PartiallyWritten
                } else {
                    OtaWriteState::EraseStarted
                };
                return Err(self.fail(OtaFailureStage::Erase, state, error, RECOVERY_ERASE));
            }
            self.sectors_erased += 1;

            self.phase = OtaPhase::Writing;
            let sector_chunks = (chunk_count - chunk_index).min(chunks_per_sector);
            for _ in 0..sector_chunks {
                let request = &write_requests[chunk_index as usize];
                self.packets_sent += 1;
                self.write_frames_sent += 1;
                if let Err(error) = boot_send_and_await_ack(
                    boot_transport,
                    request,
                    self.config.timeouts.write,
                    "WriteFirmware",
                    require_completion_ack,
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
            require_completion_ack,
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
        let jump_outcome =
            match boot_jump_app(boot_transport, &jump, self.config.timeouts.jump).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    return Err(self.fail(
                        OtaFailureStage::Jump,
                        OtaWriteState::Written,
                        error,
                        RECOVERY_JUMP,
                    ))
                }
            };

        let (Some(pid), Some(gamepad_type)) = (self.pid, self.gamepad_type) else {
            return Err(Error::InvalidRequest(
                "write_firmware completed without a PID gate result; the update state is \
                 inconsistent"
                    .to_string(),
            ));
        };
        debug_assert_eq!(pid, ready.pid);
        debug_assert_eq!(gamepad_type, ready.gamepad_type);

        self.phase = OtaPhase::Complete;
        self.emit(
            progress,
            OtaPhase::Complete,
            sector_count.saturating_sub(1),
            sector_count,
            chunk_count,
        );

        Ok(OtaOutcome {
            pid,
            gamepad_type,
            boot_version: ready.boot_version,
            sectors_erased: self.sectors_erased,
            chunks_written: self.chunks_written,
            bytes_written: self.bytes_written,
            packets_sent: self.packets_sent,
            jump: jump_outcome,
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

fn validate_expected_load_base(expected_load_base: u32) -> Result<(), Error> {
    if !(UT2_TYPE1_APP_BASE_MIN..UT2_TYPE1_APP_FLASH_END).contains(&expected_load_base) {
        return Err(invalid_firmware(format!(
            "expected_load_base 0x{expected_load_base:08X} is outside the provisional type-1 \
             application window 0x{UT2_TYPE1_APP_BASE_MIN:08X}..0x{UT2_TYPE1_APP_FLASH_END:08X}; \
             refusing to erase an unvalidated flash region (address 0 / the bootloader area / the \
             type-0 0x0801_xxxx region are not valid type-1 application bases)"
        )));
    }
    if expected_load_base % OTA_SECTOR_SIZE != 0 {
        return Err(invalid_firmware(format!(
            "expected_load_base 0x{expected_load_base:08X} is not {OTA_SECTOR_SIZE}-byte aligned; \
             EraseFlash addresses are expected_load_base + sector_off"
        )));
    }
    Ok(())
}

/// Validate the whole image range against the provisional type-1 window.
fn validate_image_range(expected_load_base: u32, image_len: u32) -> Result<(), Error> {
    validate_expected_load_base(expected_load_base)?;
    let end = u64::from(expected_load_base) + u64::from(image_len);
    if end > u64::from(UT2_TYPE1_APP_FLASH_END) {
        return Err(invalid_firmware(format!(
            "expected_load_base 0x{expected_load_base:08X} + image_len {image_len} = \
             0x{end:08X} exceeds the type-1 application flash end \
             0x{UT2_TYPE1_APP_FLASH_END:08X}; refusing to erase past the application window"
        )));
    }
    Ok(())
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
///
/// `require_completion_ack` makes the first payload-less `cmd 0x0004` matching
/// frame count as a ready ACK only; the next matching frame completes the
/// command. This is an opt-in guard for the unverified ready/completion ACK
/// distinction. Because `EraseFlash`'s request `cmd` is `0x0004`, its
/// payload-less direct echo (`cmd=0x0004, subcmd=0`) is indistinguishable from
/// the envelope and is also skipped in this mode; for every other command a
/// direct echo has a different `cmd` and completes immediately. No command is
/// ever retried.
async fn boot_send_and_await_ack<T: Transport>(
    transport: &T,
    request: &commands::Request,
    timeout: Duration,
    description: &str,
    require_completion_ack: bool,
) -> Result<(), Error> {
    transport.send(&boot::encode(request)?).await?;
    let deadline = deadline_after(timeout)?;
    let mut saw_ready_ack = false;
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
        if !boot_ack_matches(&header, request) {
            continue;
        }
        if require_completion_ack
            && !saw_ready_ack
            && header.cmd == cmd::RESPONSE
            && payload.is_empty()
        {
            saw_ready_ack = true;
            continue;
        }
        return Ok(());
    }
}

/// Send a bootloader command without waiting for its ACK.
async fn boot_send_only<T: Transport>(
    transport: &T,
    request: &commands::Request,
) -> Result<(), Error> {
    transport.send(&boot::encode(request)?).await
}

/// Send JumpApp and require evidence that the transition happened.
///
/// An ACK or a link close counts as success. A timeout is an error: a silent
/// link is not proof that the controller jumped to the application.
async fn boot_jump_app<T: Transport>(
    transport: &T,
    request: &commands::Request,
    timeout: Duration,
) -> Result<JumpOutcome, Error> {
    transport.send(&boot::encode(request)?).await?;
    let deadline = deadline_after(timeout)?;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(response_timeout("JumpApp", timeout));
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
                    return Ok(JumpOutcome::Acknowledged);
                }
            }
            Ok(Err(Error::Disconnected)) => return Ok(JumpOutcome::LinkClosed),
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(response_timeout("JumpApp", timeout)),
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

    fn base_config() -> OtaConfig {
        OtaConfig::new(TEST_BASE)
            .unwrap()
            .with_unverified_wire_allowed(true)
    }

    fn new_update() -> OtaUpdate {
        OtaUpdate::with_config(base_config()).unwrap()
    }

    fn update_with_timeouts(timeouts: OtaTimeouts) -> OtaUpdate {
        OtaUpdate::with_config(base_config().with_timeouts(timeouts)).unwrap()
    }

    /// MockTransport with the App Mode-observed MTU (92), enough for the
    /// 81-byte WriteFirmware frame.
    fn high_mtu_mock() -> MockTransport {
        MockTransport::with_mtu(92)
    }

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

    /// Test-only access to the frames a transport recorded.
    trait SentFrames {
        fn sent_frames(&self) -> Vec<Vec<u8>>;
    }

    impl SentFrames for MockTransport {
        fn sent_frames(&self) -> Vec<Vec<u8>> {
            MockTransport::sent_frames(self)
        }
    }

    /// Decode every sent bootloader frame into (header, payload).
    fn sent_boot<T: Transport + SentFrames>(mock: &T) -> Vec<(CustomHeader, Vec<u8>)> {
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
        let mock = high_mtu_mock();
        mock.push_incoming(&frame(cmd::RESPONSE, cmd::ENTER_BOOT, &[]));
        let session = Session::new(mock);
        let mut update = new_update();

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
        let mock = high_mtu_mock();
        let session = Session::new(mock);
        let mut update = new_update();

        update.enter_boot(&session).await.unwrap();

        assert_eq!(update.phase(), OtaPhase::AwaitingBootReconnect);
        assert_eq!(session.transport().sent_frames().len(), 1);
    }

    #[tokio::test]
    async fn prepare_boot_reads_pid_and_optional_version() {
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let mut update = new_update();

        let _ready = update.prepare_boot(&mock).await.unwrap();

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
        let mock = high_mtu_mock();
        mock.push_incoming(&pid_response(UT2_BOOT_PID));
        let config = base_config().with_read_version_in_boot(false);
        let mut update = OtaUpdate::with_config(config).unwrap();

        let _ready = update.prepare_boot(&mock).await.unwrap();

        assert_eq!(update.phase(), OtaPhase::Ready);
        assert_eq!(update.boot_version(), None);
        assert_eq!(sent_boot(&mock).len(), 1);
    }

    #[tokio::test]
    async fn prepare_boot_stops_on_an_unsupported_pid_before_any_write() {
        let mock = high_mtu_mock();
        mock.push_incoming(&pid_response(0x6011));
        let mut update = new_update();

        let error = update.prepare_boot(&mock).await.unwrap_err();

        assert!(matches!(
            error,
            Error::OtaUnsupportedPid {
                pid: 0x6011,
                state: OtaWriteState::Untouched,
                ..
            }
        ));
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
        let mock = high_mtu_mock();
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
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let mut progress = Vec::new();
        let outcome = update
            .write_firmware(_ready, &image, &mut |snapshot| progress.push(snapshot))
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
        assert_eq!(outcome.jump, JumpOutcome::Acknowledged);

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
        let mock = high_mtu_mock();
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
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let mut progress = Vec::new();
        let outcome = update
            .write_firmware(_ready, &image, &mut |snapshot| progress.push(snapshot))
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
        let erasing_sectors: Vec<u32> = progress
            .iter()
            .filter(|snapshot| snapshot.phase == OtaPhase::Erasing)
            .map(|snapshot| snapshot.sector_index)
            .collect();
        assert_eq!(erasing_sectors, vec![0, 1]);
        let writing_bytes: Vec<u32> = progress
            .iter()
            .filter(|snapshot| snapshot.phase == OtaPhase::Writing)
            .map(|snapshot| snapshot.bytes_written)
            .collect();
        assert_eq!(writing_bytes.len(), 65);
        assert_eq!(writing_bytes[63], 0x1000);
        assert_eq!(writing_bytes[64], 0x1040);
    }

    #[tokio::test]
    async fn second_sector_erase_failure_reports_partially_written() {
        let image_bytes = dat(0x1040, TEST_BASE);
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes[..64] {
            queue_ack(&mock, request);
        }
        // The second sector's EraseFlash is deliberately left unanswered.
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::Erase,
                    state: OtaWriteState::PartiallyWritten,
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
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes[..3] {
            queue_ack(&mock, request);
        }
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes {
            queue_ack(&mock, request);
        }
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
        let mock = high_mtu_mock();
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
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let outcome = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap();

        assert_eq!(update.phase(), OtaPhase::Complete);
        assert_eq!(outcome.jump, JumpOutcome::LinkClosed);
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
        let mut update = update_with_timeouts(OtaTimeouts {
            write: Duration::from_millis(1),
            ..OtaTimeouts::default()
        });
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
        let mut update = update_with_timeouts(OtaTimeouts {
            erase: Duration::from_millis(1),
            ..OtaTimeouts::default()
        });
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
    async fn write_firmware_rejects_a_guard_from_another_update() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let mut provider = new_update();
        let ready = provider.prepare_boot(&mock).await.unwrap();

        let mut consumer = new_update();
        let error = consumer
            .write_firmware(ready, &image, &mut |_| {})
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::InvalidRequest(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(consumer.phase(), OtaPhase::Idle);
        assert_eq!(sent_boot(&mock).len(), 2, "only the PID/version reads");

        // The same update cannot re-run prepare_boot once it is Ready, and
        // EnterBoot cannot be used after the boot phase started.
        let error = provider.prepare_boot(&mock).await.unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)));
        let app_mock = high_mtu_mock();
        let session = Session::new(app_mock);
        let error = provider.enter_boot(&session).await.unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)));
        assert!(session.transport().sent_frames().is_empty());
    }

    #[tokio::test]
    async fn load_base_outside_the_expected_base_is_rejected_before_any_erase() {
        // The image claims the next sector; the mandatory expected base is
        // TEST_BASE, so the write must stop before the first EraseFlash.
        let image_bytes = dat(0x1000, TEST_BASE + 0x1000);
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(matches!(error, Error::OtaInvalidFirmware(_)));
        assert_eq!(update.phase(), OtaPhase::Failed);
        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 2, "only readPID and readVersion may be sent");
        assert!(sent
            .iter()
            .all(|(header, _)| header.cmd != boot::CMD_ERASE_FLASH));

        // A zero base would target the bootloader region and is rejected by
        // the config constructor as well as by the write preflight.
        assert!(matches!(
            OtaConfig::new(0),
            Err(Error::OtaInvalidFirmware(_))
        ));
        assert!(matches!(
            OtaConfig::new(TEST_BASE + 0x40),
            Err(Error::OtaInvalidFirmware(_))
        ));
    }

    #[tokio::test]
    async fn read_skips_a_ready_ack_and_accepts_a_direct_echo() {
        let mock = high_mtu_mock();
        mock.push_incoming(&frame(cmd::RESPONSE, boot::CMD_READ_PID, &[]));
        mock.push_incoming(&frame(
            boot::CMD_READ_PID,
            commands::subcmd::BOOT_READ_PID,
            &UT2_BOOT_PID.to_le_bytes(),
        ));
        let config = base_config().with_read_version_in_boot(false);
        let mut update = OtaUpdate::with_config(config).unwrap();

        let _ready = update.prepare_boot(&mock).await.unwrap();

        assert_eq!(update.pid(), Some(UT2_BOOT_PID));
        assert_eq!(sent_boot(&mock).len(), 1);
    }

    #[tokio::test]
    async fn read_pid_rejects_an_unexpected_payload_length() {
        let mock = high_mtu_mock();
        mock.push_incoming(&frame(
            cmd::RESPONSE,
            boot::CMD_READ_PID,
            &[0x0F, 0x60, 0x00, 0x00],
        ));
        let mut update = new_update();

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
        let mut update = update_with_timeouts(OtaTimeouts {
            enter_boot: Duration::from_millis(1),
            ..OtaTimeouts::default()
        });

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

    #[tokio::test]
    async fn read_version_failure_stops_before_any_flash_frame() {
        let transport = PendingAfter::new(vec![pid_response(UT2_BOOT_PID)]);
        let mut update = update_with_timeouts(OtaTimeouts {
            version: Duration::from_millis(1),
            ..OtaTimeouts::default()
        });

        let error = update.prepare_boot(&transport).await.unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::ReadVersion,
                    state: OtaWriteState::Untouched,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
        assert_eq!(update.packets_sent(), 2, "readPID + readVersionInBoot");
        assert_eq!(transport.sent.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn first_erase_timeout_stops_after_one_attempt_without_retry() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let transport = PendingAfter::new(vec![
            pid_response(UT2_BOOT_PID),
            version_response(0x0000_0102),
        ]);
        let mut update = update_with_timeouts(OtaTimeouts {
            erase: Duration::from_millis(1),
            ..OtaTimeouts::default()
        });
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
        let sent = sent_boot(&transport);
        assert_eq!(sent.len(), 3, "readPID, readVersion, one erase attempt");
        assert_eq!(sent.last().unwrap().0.cmd, boot::CMD_ERASE_FLASH);
        assert_eq!(update.sectors_erased(), 0);
    }

    #[tokio::test]
    async fn read_pid_controller_error_frame_fails_closed() {
        let mock = high_mtu_mock();
        mock.push_incoming(&frame(0x000E, 0, &[0x01]));
        let mut update = new_update();

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
    }

    #[tokio::test]
    async fn erase_controller_error_frame_fails_closed() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        mock.push_incoming(&frame(0x000E, 0, &[0x01]));
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
        assert_eq!(update.chunks_written(), 0);
    }

    #[tokio::test]
    async fn unverified_wire_gate_refuses_before_any_flash_frame() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let mut update = OtaUpdate::with_config(OtaConfig::new(TEST_BASE).unwrap()).unwrap();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(error, Error::InvalidRequest(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
        assert_eq!(sent_boot(&mock).len(), 2, "only the PID/version reads");
    }

    #[tokio::test]
    async fn mtu_too_small_stops_before_the_first_erase() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = MockTransport::with_mtu(23);
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(error, Error::MtuTooSmall { mtu: 23, .. }),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 2, "only readPID and readVersion may be sent");
        assert!(sent
            .iter()
            .all(|(header, _)| header.cmd != boot::CMD_ERASE_FLASH));
    }

    #[tokio::test]
    async fn completion_ack_guard_skips_the_first_ready_ack() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let erase = boot::erase_flash(TEST_BASE).unwrap();
        queue_ack(&mock, &erase);
        queue_ack(&mock, &erase);
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes {
            queue_ack(&mock, request);
            queue_ack(&mock, request);
        }
        let version_header =
            boot::write_version_header(image.image_len(), image.version_header()).unwrap();
        queue_ack(&mock, &version_header);
        queue_ack(&mock, &version_header);
        queue_ack(&mock, &boot::jump_app());
        let mut update =
            OtaUpdate::with_config(base_config().with_completion_ack_required(true)).unwrap();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let outcome = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap();

        assert_eq!(outcome.chunks_written, 64);
        assert_eq!(outcome.jump, JumpOutcome::Acknowledged);
        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 70);
        assert_eq!(sent[2].0.cmd, boot::CMD_ERASE_FLASH);
    }

    #[tokio::test]
    async fn completion_ack_guard_times_out_on_a_single_ready_ack() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let transport = PendingAfter::new(vec![
            pid_response(UT2_BOOT_PID),
            version_response(0x0000_0102),
            ack(&boot::erase_flash(TEST_BASE).unwrap()),
        ]);
        let mut update = OtaUpdate::with_config(
            base_config()
                .with_completion_ack_required(true)
                .with_timeouts(OtaTimeouts {
                    erase: Duration::from_millis(1),
                    ..OtaTimeouts::default()
                }),
        )
        .unwrap();
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
        assert_eq!(
            sent_boot(&transport).len(),
            3,
            "no WriteFirmware may follow a ready ACK"
        );
    }

    #[tokio::test]
    async fn write_send_failure_reports_partially_written() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let transport = FailingSend::new(
            vec![
                pid_response(UT2_BOOT_PID),
                version_response(0x0000_0102),
                ack(&boot::erase_flash(TEST_BASE).unwrap()),
            ],
            boot::CMD_WRITE_FIRMWARE,
        );
        let mut update = new_update();
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
        assert_eq!(update.chunks_written(), 0);
    }

    #[tokio::test]
    async fn erase_send_failure_reports_erase_started() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let transport = FailingSend::new(
            vec![pid_response(UT2_BOOT_PID), version_response(0x0000_0102)],
            boot::CMD_ERASE_FLASH,
        );
        let mut update = new_update();
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
    async fn write_firmware_version_send_failure_reports_written() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mut frames = vec![
            pid_response(UT2_BOOT_PID),
            version_response(0x0000_0102),
            ack(&boot::erase_flash(TEST_BASE).unwrap()),
        ];
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        frames.extend(writes.iter().map(ack));
        frames.push(ack(&boot::write_version_header(
            image.image_len(),
            image.version_header(),
        )
        .unwrap()));
        let transport = FailingSend::new(frames, boot::CMD_WRITE_FIRMWARE_VERSION);
        let mut update = new_update();
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::WriteFirmwareVersion,
                    state: OtaWriteState::Written,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.chunks_written(), 64);
    }

    #[tokio::test]
    async fn jump_send_failure_reports_written() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mut frames = vec![
            pid_response(UT2_BOOT_PID),
            version_response(0x0000_0102),
            ack(&boot::erase_flash(TEST_BASE).unwrap()),
        ];
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        frames.extend(writes.iter().map(ack));
        frames.push(ack(&boot::write_version_header(
            image.image_len(),
            image.version_header(),
        )
        .unwrap()));
        let transport = FailingSend::new(frames, boot::CMD_JUMP_APP);
        let mut update = new_update();
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::Jump,
                    state: OtaWriteState::Written,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
    }

    #[tokio::test]
    async fn jump_timeout_is_a_failure() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mut frames = vec![
            pid_response(UT2_BOOT_PID),
            version_response(0x0000_0102),
            ack(&boot::erase_flash(TEST_BASE).unwrap()),
        ];
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        frames.extend(writes.iter().map(ack));
        frames.push(ack(&boot::write_version_header(
            image.image_len(),
            image.version_header(),
        )
        .unwrap()));
        let transport = PendingAfter::new(frames);
        let mut update = update_with_timeouts(OtaTimeouts {
            jump: Duration::from_millis(1),
            ..OtaTimeouts::default()
        });
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::Jump,
                    state: OtaWriteState::Written,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.phase(), OtaPhase::Failed);
    }

    #[tokio::test]
    async fn partial_last_sector_writes_one_chunk() {
        let image_bytes = dat(OTA_CHUNK_SIZE as u32, TEST_BASE);
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(TEST_BASE).unwrap());
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        assert_eq!(writes.len(), 1);
        queue_ack(&mock, &writes[0]);
        queue_ack(
            &mock,
            &boot::write_version_header(image.image_len(), image.version_header()).unwrap(),
        );
        queue_ack(&mock, &boot::jump_app());
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let mut progress = Vec::new();
        let outcome = update
            .write_firmware(_ready, &image, &mut |snapshot| progress.push(snapshot))
            .await
            .unwrap();

        assert_eq!(outcome.sectors_erased, 1);
        assert_eq!(outcome.chunks_written, 1);
        assert_eq!(outcome.bytes_written, OTA_CHUNK_SIZE as u32);
        assert_eq!(progress.last().unwrap().sector_count, 1);
        assert_eq!(progress.last().unwrap().chunk_count, 1);
    }

    #[tokio::test]
    async fn cancelled_write_leaves_no_recovery_error() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let transport = PendingAfter::new(vec![
            pid_response(UT2_BOOT_PID),
            version_response(0x0000_0102),
        ]);
        let mut update = new_update();
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let cancelled = tokio::time::timeout(
            Duration::from_millis(1),
            update.write_firmware(_ready, &image, &mut |_| {}),
        )
        .await;
        assert!(cancelled.is_err(), "the outer timeout must fire");
        assert_eq!(update.phase(), OtaPhase::Erasing);
        let error = update.prepare_boot(&transport).await.unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)));
    }

    #[test]
    fn multi_section_dat_is_rejected_with_a_clear_message() {
        let mut bytes = dat(0x80, TEST_BASE);
        bytes.push(0x01);
        let error = FirmwareImage::parse(&bytes).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("multi-section") && message.contains("does not support"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn load_base_plus_image_len_accepts_the_2_32_boundary() {
        let exact = dat(0x1000, 0xFFFF_F000);
        assert!(FirmwareImage::parse(&exact).is_ok());
        let over = dat(0x1040, 0xFFFF_F000);
        let error = FirmwareImage::parse(&over).unwrap_err();
        assert!(matches!(error, Error::OtaInvalidFirmware(_)));
    }

    #[tokio::test]
    async fn failed_update_cannot_be_reused() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let mut update = new_update();
        let _ready = update.prepare_boot(&mock).await.unwrap();
        update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();
        assert_eq!(update.phase(), OtaPhase::Failed);

        let error = update.prepare_boot(&high_mtu_mock()).await.unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)));

        // Recovery uses a fresh update from Idle on the "8bt" link.
        let recovery_mock = high_mtu_mock();
        queue_prepare(&recovery_mock, UT2_BOOT_PID, 0x0000_0102);
        let mut recovery = new_update();
        assert!(recovery.prepare_boot(&recovery_mock).await.is_ok());
        assert_eq!(recovery.phase(), OtaPhase::Ready);
    }

    #[test]
    fn zero_expected_base_is_rejected_by_with_config() {
        // `OtaConfig` fields are private, so `OtaConfig::new` is the only public
        // construction path. Even the in-crate struct-literal bypass is caught
        // by `OtaUpdate::with_config`, and `write_firmware` re-validates again.
        let config = OtaConfig {
            expected_load_base: 0,
            ..base_config()
        };
        assert!(matches!(
            OtaUpdate::with_config(config),
            Err(Error::OtaInvalidFirmware(_))
        ));
    }

    #[test]
    fn expected_base_outside_the_type1_window_is_rejected() {
        assert!(OtaConfig::new(UT2_TYPE1_APP_BASE_MIN).is_ok());
        for base in [
            0x0000_0000,
            0x0100_F000,
            0x0102_0000,
            0x0801_0000,
            0xFFFF_F000,
        ] {
            assert!(
                matches!(OtaConfig::new(base), Err(Error::OtaInvalidFirmware(_))),
                "base 0x{base:08X} must be outside the type-1 window"
            );
        }
    }

    #[tokio::test]
    async fn image_end_beyond_the_type1_flash_window_is_rejected_before_erase() {
        // Base is valid, but base + image_len crosses the provisional end.
        let base = UT2_TYPE1_APP_FLASH_END - OTA_SECTOR_SIZE;
        let image_bytes = dat(0x2000, base);
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        let config = OtaConfig::new(base)
            .unwrap()
            .with_unverified_wire_allowed(true);
        let mut update = OtaUpdate::with_config(config).unwrap();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap_err();

        assert!(matches!(error, Error::OtaInvalidFirmware(_)));
        assert_eq!(update.phase(), OtaPhase::Failed);
        let sent = sent_boot(&mock);
        assert_eq!(sent.len(), 2, "only the PID/version reads");
        assert!(sent
            .iter()
            .all(|(header, _)| header.cmd != boot::CMD_ERASE_FLASH));
    }

    #[tokio::test]
    async fn image_end_at_the_type1_flash_boundary_is_accepted() {
        let base = UT2_TYPE1_APP_FLASH_END - OTA_SECTOR_SIZE;
        let image_bytes = dat(OTA_SECTOR_SIZE, base);
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
        queue_prepare(&mock, UT2_BOOT_PID, 0x0000_0102);
        queue_ack(&mock, &boot::erase_flash(base).unwrap());
        let writes = boot::write_firmware_packets(base, image.image_len(), image.image()).unwrap();
        for request in &writes {
            queue_ack(&mock, request);
        }
        queue_ack(
            &mock,
            &boot::write_version_header(image.image_len(), image.version_header()).unwrap(),
        );
        queue_ack(&mock, &boot::jump_app());
        let config = OtaConfig::new(base)
            .unwrap()
            .with_unverified_wire_allowed(true);
        let mut update = OtaUpdate::with_config(config).unwrap();
        let _ready = update.prepare_boot(&mock).await.unwrap();

        let outcome = update
            .write_firmware(_ready, &image, &mut |_| {})
            .await
            .unwrap();

        assert_eq!(outcome.chunks_written, 64);
        assert_eq!(outcome.bytes_written, OTA_SECTOR_SIZE);
    }

    #[tokio::test]
    async fn write_version_ready_ack_guard_times_out_on_a_single_ack() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mut frames = vec![pid_response(UT2_BOOT_PID), version_response(0x0000_0102)];
        let erase = boot::erase_flash(TEST_BASE).unwrap();
        frames.push(ack(&erase));
        frames.push(ack(&erase));
        let writes =
            boot::write_firmware_packets(TEST_BASE, image.image_len(), image.image()).unwrap();
        for request in &writes {
            frames.push(ack(request));
            frames.push(ack(request));
        }
        // Only one WriteVersion ACK: the guard treats it as a ready ACK and
        // must wait for a completion ACK until the timeout fires.
        frames.push(ack(&boot::write_version_header(
            image.image_len(),
            image.version_header(),
        )
        .unwrap()));
        let transport = PendingAfter::new(frames);
        let config = base_config()
            .with_completion_ack_required(true)
            .with_timeouts(OtaTimeouts {
                write_version: Duration::from_millis(1),
                ..OtaTimeouts::default()
            });
        let mut update = OtaUpdate::with_config(config).unwrap();
        let _ready = update.prepare_boot(&transport).await.unwrap();

        let error = update
            .write_firmware(_ready, &image, &mut |_| {})
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
    }

    #[tokio::test]
    async fn mobile_boot_link_allows_an_external_boot_transport() {
        let image_bytes = one_sector_image();
        let image = FirmwareImage::parse(&image_bytes).unwrap();
        let mock = high_mtu_mock();
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

        // UniFFI injects a native transport through this adapter; the sealed
        // OtaTransport marker is implemented by the crate, not downstream.
        let link = MobileBootLink::from_native(&mock);
        assert_eq!(link.mtu(), 92);
        let mut update = new_update();
        let ready = update.prepare_boot(&link).await.unwrap();
        let outcome = update
            .write_firmware(ready, &image, &mut |_| {})
            .await
            .unwrap();

        assert_eq!(outcome.chunks_written, 64);
        assert_eq!(outcome.jump, JumpOutcome::Acknowledged);
        assert_eq!(sent_boot(&mock).len(), 70);
    }

    #[test]
    fn mobile_boot_link_returns_the_wrapped_transport() {
        let mock = high_mtu_mock();
        let link = MobileBootLink::from_native(mock);
        let mock = link.into_inner();
        assert_eq!(mock.mtu(), 92);
    }

    #[tokio::test]
    async fn mobile_boot_link_delegates_receive_errors() {
        let transport = PendingAfter::new(vec![pid_response(UT2_BOOT_PID)]);
        let link = MobileBootLink::from_native(&transport);
        let mut update = update_with_timeouts(OtaTimeouts {
            version: Duration::from_millis(1),
            ..OtaTimeouts::default()
        });

        let error = update.prepare_boot(&link).await.unwrap_err();

        assert!(
            matches!(
                error,
                Error::Ota {
                    stage: OtaFailureStage::ReadVersion,
                    state: OtaWriteState::Untouched,
                    ..
                }
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(update.packets_sent(), 2);
    }

    #[tokio::test]
    async fn enter_boot_and_release_closes_the_app_link() {
        let mock = high_mtu_mock();
        mock.push_incoming(&frame(cmd::RESPONSE, cmd::ENTER_BOOT, &[]));
        let session = Session::new(&mock);
        let mut update = new_update();

        update.enter_boot_and_release(session).await.unwrap();

        assert_eq!(update.phase(), OtaPhase::AwaitingBootReconnect);
        assert!(mock.is_disconnected());
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

    impl super::sealed::Sealed for PendingAfter {}

    impl OtaTransport for PendingAfter {}

    impl SentFrames for PendingAfter {
        fn sent_frames(&self) -> Vec<Vec<u8>> {
            self.sent.lock().unwrap().clone()
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
            92
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    /// A transport that returns an injected send error for one command.
    struct FailingSend {
        incoming: Mutex<VecDeque<Vec<u8>>>,
        sent: Mutex<Vec<Vec<u8>>>,
        fail_cmd: u16,
    }

    impl FailingSend {
        fn new(incoming: Vec<Vec<u8>>, fail_cmd: u16) -> Self {
            Self {
                incoming: Mutex::new(incoming.into()),
                sent: Mutex::new(Vec::new()),
                fail_cmd,
            }
        }
    }

    impl super::sealed::Sealed for FailingSend {}

    impl OtaTransport for FailingSend {}

    impl SentFrames for FailingSend {
        fn sent_frames(&self) -> Vec<Vec<u8>> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Transport for FailingSend {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            let header = CustomHeader::parse(&data[1..]).expect("bootloader frame header");
            self.sent.lock().unwrap().push(data.to_vec());
            if header.cmd == self.fail_cmd {
                return Err(Error::Transport("injected send failure".to_string()));
            }
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            self.incoming
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
                .ok_or(Error::Disconnected)
        }

        fn mtu(&self) -> u16 {
            92
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }
}
