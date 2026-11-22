//! Full 2768-byte configuration writes (`cmd 0x0001` + Save `0x0006`).
//!
//! The macro path in [`crate::macro_ops`] only writes one slot's 216-byte macro
//! header and its flash sectors. This module owns the other half of the write
//! surface: taking the configuration currently read from the controller,
//! applying typed edits, recomputing the structure CRC and writing the whole
//! `0x0AD0`-byte block, then committing it with Save and verifying a read-back.
//!
//! # Safety contract
//!
//! * Edits are applied to the snapshot read from the device, so every byte the
//!   caller did not ask to change (including the undocumented legacy regions at
//!   `0x1F4..0x68C` and `0x914..0x92C`) is written back unchanged.
//! * The structure CRC is recomputed the same way the official tool does: over
//!   the whole buffer, with the previous CRC field still in place.
//! * No frame is sent until the plan has been validated and every packet has
//!   been built.
//! * The write sends `0x0001` chunks, then the BLE Save (`0x0006`,
//!   `subcmd 0x0067`), then reads the block back (`0x0002`) and compares it.
//! * Failures are fail-closed and carry the stage and whether a Save was
//!   acknowledged, so a caller can tell "nothing committed" from "committed but
//!   the read-back is unconfirmed".
//!
//! # Button remapping
//!
//! [`ConfigEdit::ButtonRemap`] changes exactly one 4-byte `MapKeyRecord.key[]`
//! entry at `+0x0E4 + slot * 0x5C + button * 4`, matching the official
//! `getBT2_WriteMapKey` / `_writeUltimateBT2MapKey` single-entry write
//! (re-derived from the official Android and Windows binaries; see
//! `.tmp/features/re/CONFIG_WRITE_RE.md` §3). The 22-entry order is the
//! non-3E layout of [`ButtonIndex`]: `A, B, X, Y, L1, R1, L2, R2, L3, R3,
//! Select, Start, Share, Logo, Up, Down, Left, Right, P1/PL, P2/PR, P3/L4,
//! P4/R4`. [`KeyMap::NONE`] (`0`) disables an entry; the official remap path
//! never writes `KeyMap::MY_NULL` (227).
//!
//! The record `flag` (`+0x0E0 + slot * 0x5C`, `1` = enabled) is **not**
//! changed by the single-entry write. [`ConfigEdit::MapKeyFlag`] edits it
//! explicitly when a caller needs to enable or disable the whole record.

use std::time::Duration;

use eightbitdo_protocol::commands::{self, cmd, SaveTransport};
use eightbitdo_protocol::config_bt2::{
    Bt2Config, DynamicTheme, GamepadMode, HotWheelTheme, JoyRecord, RumbleRecord, SingleTheme,
    SixAxisRecord, Slot, SpecialRecord, TriggerRecord, BUTTON_COUNT, CONFIG_SIZE, OFF_CRC16,
    OFF_LEGACY_A, OFF_LEGACY_B, OFF_MACROS, OFF_SIX_AXIS,
};
use eightbitdo_protocol::crc::ansi_crc16_sh;
use eightbitdo_protocol::keymap::{ButtonIndex, KeyMap};

use crate::error::{ConfigWriteStage, ConfigWriteState, Error};
use crate::macro_ops::max_protocol_payload;
use crate::session::{ConfigSnapshot, Session, CONFIG_TOTAL_TIMEOUT};
use crate::transport::Transport;

/// ACK wait for one `0x0001` configuration chunk.
///
/// Two seconds matches the existing macro write policy and the official macro
/// timeouts. The full configuration write is not documented separately; this
/// is the same bound.
pub const CONFIG_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// ACK wait for the `0x0006` Save.
///
/// Three seconds with no retry, matching [`crate::macro_ops::MACRO_SAVE_TIMEOUT`]
/// and the USB HID Save value. The official BLE value is not documented.
pub const CONFIG_SAVE_TIMEOUT: Duration = Duration::from_secs(3);

/// Settle delay between a Save ACK and the read-back.
///
/// Provisional, same order as the macro verify delay.
pub const CONFIG_VERIFY_DELAY: Duration = Duration::from_millis(300);

/// The `u32` CRC field at `+0x00C..+0x010`.
const CRC_RANGE: std::ops::Range<usize> = OFF_CRC16..OFF_CRC16 + 4;

/// Regions the official host never interprets. A read-modify-write must keep
/// them byte-identical.
const RESERVED_REGIONS: [(usize, usize); 2] =
    [(OFF_LEGACY_A, OFF_MACROS), (OFF_LEGACY_B, OFF_SIX_AXIS)];

/// Payload bytes one `0x0001` frame may carry at this MTU.
///
/// Uses the same budget as the macro path: `mtu - 3` (ATT overhead) minus the
/// 1-byte BLE prefix and the 16-byte custom header.
///
/// # Errors
///
/// Returns [`Error::MtuTooSmall`] when no payload byte fits.
pub fn config_write_chunk_len(mtu: u16) -> Result<usize, Error> {
    let max_payload = max_protocol_payload(mtu);
    if max_payload == 0 {
        return Err(Error::MtuTooSmall {
            mtu,
            reason: "no room for the 1-byte prefix and 16-byte header".to_string(),
        });
    }
    Ok(max_payload.min(CONFIG_SIZE))
}

/// One typed change to the configuration.
///
/// Every variant is built from a type in
/// [`eightbitdo_protocol::config_bt2`]; no field offset or bit layout is
/// invented here. A plan applies variants in order, so a later variant wins
/// over an earlier one for the same field.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigEdit {
    /// Set `gamepad_mode` (`+0x010`).
    GamepadMode(GamepadMode),
    /// Set `cur_slot` (`+0x012`); `None` writes `4` (`NONE`).
    CurSlot(Option<Slot>),
    /// Set one profile name (`+0x014 + slot * 0x20`, UTF-16BE, max 16 units).
    ProfileName {
        /// Profile slot whose name changes.
        slot: Slot,
        /// New name.
        name: String,
    },
    /// Replace one rumble record (`+0x074 + slot * 0x0C`).
    Rumble {
        /// Profile slot whose record changes.
        slot: Slot,
        /// New record.
        record: RumbleRecord,
    },
    /// Replace one stick record (`+0x098 + slot * 0x08`).
    Joy {
        /// Profile slot whose record changes.
        slot: Slot,
        /// New record.
        record: JoyRecord,
    },
    /// Replace one trigger record (`+0x0B0 + slot * 0x08`).
    Trigger {
        /// Profile slot whose record changes.
        slot: Slot,
        /// New record.
        record: TriggerRecord,
    },
    /// Replace one special-feature record (`+0x0C8 + slot * 0x08`).
    Special {
        /// Profile slot whose record changes.
        slot: Slot,
        /// New record.
        record: SpecialRecord,
    },
    /// Replace one six-axis / gyro record (`+0x92C + slot * 0x0C`).
    ///
    /// The Windows DLL offset is used by the setter, not the Android
    /// `offset = 0` bug.
    SixAxis {
        /// Profile slot whose record changes.
        slot: Slot,
        /// New record.
        record: SixAxisRecord,
    },
    /// Replace one dynamic / rainbow theme (`+0x950 + slot * 0x0C`).
    DynamicTheme {
        /// Profile slot whose theme changes.
        slot: Slot,
        /// New theme.
        theme: DynamicTheme,
    },
    /// Replace one hot-wheel theme (`+0x974 + slot * 0x10`).
    HotWheelTheme {
        /// Profile slot whose theme changes.
        slot: Slot,
        /// New theme.
        theme: HotWheelTheme,
    },
    /// Replace one single-color theme (`+0x9A4 + slot * 0x64`).
    SingleTheme {
        /// Profile slot whose theme changes.
        slot: Slot,
        /// New theme.
        theme: SingleTheme,
    },
    /// Set one button mapping entry (`+0x0E4 + slot * 0x5C + button * 4`).
    ///
    /// This changes only the 4-byte entry, matching the official
    /// `getBT2_WriteMapKey` / `_writeUltimateBT2MapKey` partial write. The
    /// record's `flag` is not touched, and [`KeyMap::NONE`] disables the
    /// mapping.
    ButtonRemap {
        /// Profile slot.
        slot: Slot,
        /// Physical button index.
        button: ButtonIndex,
        /// New mapping target.
        to: KeyMap,
    },
    /// Set the `MapKeyRecord` flag (`+0x0E0 + slot * 0x5C`, `1` = enabled).
    ///
    /// The official single-entry remap does not change this flag; this edit is
    /// for enabling or disabling the whole record explicitly.
    MapKeyFlag {
        /// Profile slot.
        slot: Slot,
        /// New flag value (`0` = disabled).
        flag: u32,
    },
}

/// Apply one edit to `config`.
///
/// Only the edited field's bytes are touched; the caller checks the reserved
/// regions afterwards.
fn apply_edit(config: &mut Bt2Config, edit: &ConfigEdit) -> Result<(), Error> {
    match edit {
        ConfigEdit::GamepadMode(mode) => config.set_gamepad_mode(*mode),
        ConfigEdit::CurSlot(slot) => config.set_cur_slot(*slot),
        ConfigEdit::ProfileName { slot, name } => config.set_profile_name(*slot, name)?,
        ConfigEdit::Rumble { slot, record } => config.set_rumble(*slot, record),
        ConfigEdit::Joy { slot, record } => config.set_joy(*slot, record),
        ConfigEdit::Trigger { slot, record } => config.set_trigger(*slot, record),
        ConfigEdit::Special { slot, record } => config.set_special(*slot, record),
        ConfigEdit::SixAxis { slot, record } => config.set_six_axis(*slot, record),
        ConfigEdit::DynamicTheme { slot, theme } => config.set_dynamic_theme(*slot, theme),
        ConfigEdit::HotWheelTheme { slot, theme } => config.set_hotwheel_theme(*slot, theme),
        ConfigEdit::SingleTheme { slot, theme } => config.set_single_theme(*slot, theme),
        ConfigEdit::ButtonRemap { slot, button, to } => {
            config.set_map_key_entry(*slot, *button, *to);
        }
        ConfigEdit::MapKeyFlag { slot, flag } => {
            let mut record = config.map_key(*slot);
            record.flag = *flag;
            config.set_map_key(*slot, &record);
        }
    }
    Ok(())
}

/// Read the 22 button mappings of one slot, in wire order.
///
/// The order is `A, B, X, Y, L1, R1, L2, R2, L3, R3, Select, Start, Share,
/// Logo, Up, Down, Left, Right, P1/PL, P2/PR, P3/L4, P4/R4`. The record
/// `flag` is not part of this array; use [`map_key_flag`] for it.
#[must_use]
pub fn button_mappings(config: &Bt2Config, slot: Slot) -> [KeyMap; BUTTON_COUNT] {
    config.map_key(slot).keys
}

/// Read one physical button's mapping target (`0` = unassigned).
#[must_use]
pub fn button_mapping(config: &Bt2Config, slot: Slot, button: ButtonIndex) -> KeyMap {
    config.map_key_entry(slot, button)
}

/// Read a slot's `MapKeyRecord` flag (`1` = remapping enabled).
#[must_use]
pub fn map_key_flag(config: &Bt2Config, slot: Slot) -> u32 {
    config.map_key(slot).flag
}

/// A contiguous range of configuration bytes that differs from the device
/// snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigRegion {
    offset: usize,
    len: usize,
}

impl ConfigRegion {
    /// First differing byte.
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    /// Number of differing bytes.
    #[must_use]
    pub const fn len(self) -> usize {
        self.len
    }

    /// True when the region is empty (only possible for a hand-built value;
    /// the diff helper never emits one).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// One past the last differing byte.
    #[must_use]
    pub const fn end(self) -> usize {
        self.offset + self.len
    }

    /// The region as a standard range.
    #[must_use]
    pub fn range(self) -> std::ops::Range<usize> {
        self.offset..self.end()
    }
}

/// A list of typed edits to apply to the configuration read from the device.
///
/// Build it with [`ConfigWritePlan::new`] / [`ConfigWritePlan::with`], then
/// validate and materialize it with [`ConfigWritePlan::prepare`] **before**
/// connecting or writing. The prepared form contains the exact buffer and the
/// changed regions, so a front end can show a diff and write a pre-write
/// backup before any frame is sent.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConfigWritePlan {
    edits: Vec<ConfigEdit>,
}

impl ConfigWritePlan {
    /// An empty plan.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A plan from an existing list.
    #[must_use]
    pub fn from_edits(edits: Vec<ConfigEdit>) -> Self {
        Self { edits }
    }

    /// Borrow the edits.
    #[must_use]
    pub fn edits(&self) -> &[ConfigEdit] {
        &self.edits
    }

    /// True when the plan has no edits.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Append one edit.
    pub fn push(&mut self, edit: ConfigEdit) {
        self.edits.push(edit);
    }

    /// Append one edit and return `self` (builder form).
    #[must_use]
    pub fn with(mut self, edit: ConfigEdit) -> Self {
        self.push(edit);
        self
    }

    /// Apply the edits to `snapshot` and validate the result.
    ///
    /// The CRC is recomputed only when at least one byte actually changes. A
    /// plan that changes nothing yields a [`PreparedConfigWrite::is_noop`]
    /// result instead of a pointless flash commit.
    ///
    /// # Errors
    ///
    /// Returns the protocol error for an invalid profile name (too long or an
    /// embedded NUL), or [`Error::InvalidRequest`] when an edit would change a
    /// reserved region (which typed edits never do; this is a defensive check).
    pub fn prepare(&self, snapshot: &ConfigSnapshot) -> Result<PreparedConfigWrite, Error> {
        let mut expected = snapshot.config.clone();
        for edit in &self.edits {
            apply_edit(&mut expected, edit)?;
        }
        if expected == snapshot.config {
            return Ok(PreparedConfigWrite {
                expected: snapshot.config.clone(),
                changed: Vec::new(),
            });
        }
        ensure_reserved_unchanged(&snapshot.config, &expected)?;
        expected.recompute_crc();
        Ok(PreparedConfigWrite {
            changed: changed_regions(snapshot.config.as_bytes(), expected.as_bytes()),
            expected,
        })
    }
}

/// A validated, CRC-recomputed configuration buffer ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedConfigWrite {
    expected: Bt2Config,
    changed: Vec<ConfigRegion>,
}

impl PreparedConfigWrite {
    /// Build a prepared write from a complete 2768-byte buffer, e.g. a dump
    /// that was edited offline and loaded again.
    ///
    /// The reserved regions must still match the device snapshot; a buffer that
    /// changes them is refused rather than writing guessed values back. The
    /// CRC is recomputed with the official in-place rule when the buffer
    /// differs from the snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] when a reserved byte differs.
    pub fn from_buffer(snapshot: &ConfigSnapshot, edited: Bt2Config) -> Result<Self, Error> {
        if edited == snapshot.config {
            return Ok(Self {
                expected: snapshot.config.clone(),
                changed: Vec::new(),
            });
        }
        ensure_reserved_unchanged(&snapshot.config, &edited)?;
        let mut expected = edited;
        expected.recompute_crc();
        Ok(Self {
            changed: changed_regions(snapshot.config.as_bytes(), expected.as_bytes()),
            expected,
        })
    }

    /// The buffer to write.
    #[must_use]
    pub fn expected(&self) -> &Bt2Config {
        &self.expected
    }

    /// The buffer to write, as raw bytes.
    #[must_use]
    pub const fn expected_bytes(&self) -> &[u8; CONFIG_SIZE] {
        self.expected.as_bytes()
    }

    /// The contiguous byte ranges that differ from the snapshot.
    #[must_use]
    pub fn changed(&self) -> &[ConfigRegion] {
        &self.changed
    }

    /// True when the plan changes no byte.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.changed.is_empty()
    }
}

/// ACK timeouts for a full configuration write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigWriteTimeouts {
    /// Each `0x0001` chunk ACK wait.
    pub write: Duration,
    /// The `0x0006` Save ACK wait.
    pub save: Duration,
}

impl Default for ConfigWriteTimeouts {
    fn default() -> Self {
        Self {
            write: CONFIG_WRITE_TIMEOUT,
            save: CONFIG_SAVE_TIMEOUT,
        }
    }
}

/// CRC state of a verified read-back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigCrcStatus {
    /// CRC field of the buffer this host sent.
    pub expected: u16,
    /// CRC field the device returned.
    pub stored: u16,
    /// `ANSI_CRC_16_sh` recomputed over the returned buffer, including its own
    /// CRC field (the official in-place rule).
    pub recomputed: u16,
    /// `ANSI_CRC_16_sh` over the returned buffer with the CRC field replaced by
    /// the value this host sent.
    ///
    /// This is what the device would store if it re-ran the official
    /// in-place recomputation over the received buffer on Save.
    pub recomputed_from_sent: u16,
}

impl ConfigCrcStatus {
    /// True when the device kept the host's CRC value.
    #[must_use]
    pub const fn matches_sent(self) -> bool {
        self.stored == self.expected
    }

    /// True when the stored value is the official in-place recomputation over
    /// the returned buffer.
    #[must_use]
    pub const fn in_place(self) -> bool {
        self.stored == self.recomputed
    }

    /// True when the stored value is the recomputation over the buffer as it
    /// was sent (the device re-ran the official recomputation on Save).
    #[must_use]
    pub const fn from_sent(self) -> bool {
        self.stored == self.recomputed_from_sent
    }

    /// True when any accepted interpretation holds.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.matches_sent() || self.in_place() || self.from_sent()
    }
}

/// Result of a successful full configuration write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWriteOutcome {
    /// Ranges that differed from the pre-write snapshot.
    pub changed: Vec<ConfigRegion>,
    /// Frames sent: `0x0001` chunks + one Save.
    pub packets_sent: usize,
    /// The 2768 bytes read back after Save.
    pub read_back: Bt2Config,
    /// CRC state of the read-back.
    pub crc: ConfigCrcStatus,
}

/// No Save was sent; the flash configuration is unchanged.
const RECOVERY_CONFIG_WRITE_FAILED: &str =
    "Save was not sent, so the flash configuration is unchanged. The controller may hold partially \
     updated bytes in RAM. Re-read the configuration before retrying, and power-cycle the \
     controller to discard uncommitted RAM state.";
/// Save was sent but its ACK never arrived.
const RECOVERY_CONFIG_SAVE_UNCONFIRMED: &str =
    "Save was sent but its ACK did not arrive, so the commit is unconfirmed. Do not repeat the \
     write blindly: re-read the configuration, compare it with the pre-write backup, and retry or \
     restore only if it differs.";
/// Save was acknowledged, but the read-back is unconfirmed or wrong.
const RECOVERY_CONFIG_SAVED: &str =
    "Save was acknowledged, so the new configuration is committed; the device may have normalized \
     values. Re-read the configuration, and if the result is wrong write the full 2768-byte \
     pre-write backup back with another configuration write.";

impl<T: Transport> Session<T> {
    /// Write the whole 2768-byte configuration and Save it.
    ///
    /// `prepared` must come from [`ConfigWritePlan::prepare`] or
    /// [`PreparedConfigWrite::from_buffer`], which validates the buffer and
    /// recomputes the CRC. The call:
    ///
    /// 1. splits the 2768 bytes into MTU-sized `0x0001` packets (every packet
    ///    carries the CRC-16/MODBUS of its own payload),
    /// 2. waits for each ACK (`2 s`, [`CONFIG_WRITE_TIMEOUT`]),
    /// 3. sends the BLE Save (`0x0006`, `subcmd 0x0067`, `3 s`,
    ///    [`CONFIG_SAVE_TIMEOUT`]),
    /// 4. waits [`CONFIG_VERIFY_DELAY`], reads the 2768 bytes back with
    ///    `0x0002` and compares them (CRC field excluded, CRC reconciled),
    /// 5. fails closed with [`Error::ConfigWrite`] and the stage / Save state.
    ///
    /// The caller is responsible for the mandatory pre-write backup; nothing
    /// here touches the local filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] for a no-op plan,
    /// [`Error::MtuTooSmall`] when no configuration payload fits, or
    /// [`Error::ConfigWrite`] for a refused / unanswered frame or a read-back
    /// mismatch.
    pub async fn write_full_config(
        &self,
        snapshot: &ConfigSnapshot,
        prepared: &PreparedConfigWrite,
        timeouts: ConfigWriteTimeouts,
    ) -> Result<ConfigWriteOutcome, Error> {
        if prepared.is_noop() {
            return Err(Error::InvalidRequest(
                "configuration write plan changes nothing; refusing a no-op write".to_string(),
            ));
        }

        let mtu = self.transport().mtu();
        let chunk_size = config_write_chunk_len(mtu)?;
        // Build every frame before the first one is sent: a local framing error
        // must not leave a partially written block.
        let requests =
            commands::write_config_packets(0, prepared.expected().as_bytes(), chunk_size)?;
        let save = commands::save(SaveTransport::Ble);

        self.start_input_keepalive().await;
        self.prepare_read().await?;
        self.run_write(self.write_full_config_inner(snapshot, prepared, &requests, &save, timeouts))
            .await
    }

    async fn write_full_config_inner(
        &self,
        snapshot: &ConfigSnapshot,
        prepared: &PreparedConfigWrite,
        requests: &[commands::Request],
        save: &commands::Request,
        timeouts: ConfigWriteTimeouts,
    ) -> Result<ConfigWriteOutcome, Error> {
        let mut packets_sent = 0usize;
        for request in requests {
            self.send_and_await_ack(
                request,
                false,
                cmd::WRITE_CONFIG,
                0,
                timeouts.write,
                "WriteConfig",
            )
            .await
            .map_err(|error| {
                config_write_failure(
                    ConfigWriteStage::Write,
                    ConfigWriteState::NotSaved,
                    error,
                    RECOVERY_CONFIG_WRITE_FAILED,
                )
            })?;
            packets_sent += 1;
        }

        self.send_and_await_ack(
            save,
            false,
            cmd::SAVE,
            SaveTransport::Ble.subcmd(),
            timeouts.save,
            "Save",
        )
        .await
        .map_err(|error| {
            config_write_failure(
                ConfigWriteStage::Save,
                ConfigWriteState::NotSaved,
                error,
                RECOVERY_CONFIG_SAVE_UNCONFIRMED,
            )
        })?;
        packets_sent += 1;

        self.settle_with_polling(CONFIG_VERIFY_DELAY).await;
        let read_back_bytes = self
            .read_config_chunks_inner(CONFIG_SIZE, self.per_frame_timeout(), CONFIG_TOTAL_TIMEOUT)
            .await
            .map_err(|error| {
                config_write_failure(
                    ConfigWriteStage::Verify,
                    ConfigWriteState::Saved,
                    error,
                    RECOVERY_CONFIG_SAVED,
                )
            })?;
        let read_back = Bt2Config::from_bytes(&read_back_bytes).map_err(|error| {
            config_write_failure(
                ConfigWriteStage::Verify,
                ConfigWriteState::Saved,
                Error::from(error),
                RECOVERY_CONFIG_SAVED,
            )
        })?;

        let crc =
            verify_config_readback(snapshot, prepared.expected(), &read_back).map_err(|error| {
                config_write_failure(
                    ConfigWriteStage::Verify,
                    ConfigWriteState::Saved,
                    error,
                    RECOVERY_CONFIG_SAVED,
                )
            })?;

        Ok(ConfigWriteOutcome {
            changed: prepared.changed().to_vec(),
            packets_sent,
            read_back,
            crc,
        })
    }
}

fn config_write_failure(
    stage: ConfigWriteStage,
    state: ConfigWriteState,
    error: Error,
    recovery: &'static str,
) -> Error {
    Error::ConfigWrite {
        stage,
        state,
        reason: error.to_string(),
        recovery,
    }
}

/// Compare the read-back with the buffer that was sent.
///
/// Every byte outside the CRC field must match. The CRC field may be the value
/// this host sent, the official in-place recomputation over the returned
/// buffer, or that same recomputation over the buffer as it was sent (the
/// device re-ran it on Save); anything else is a mismatch. The reserved
/// regions must still equal the pre-write snapshot.
fn verify_config_readback(
    snapshot: &ConfigSnapshot,
    expected: &Bt2Config,
    actual: &Bt2Config,
) -> Result<ConfigCrcStatus, Error> {
    for (start, end) in RESERVED_REGIONS {
        let before = &snapshot.config.as_bytes()[start..end];
        let after = &actual.as_bytes()[start..end];
        if before != after {
            let first = before
                .iter()
                .zip(after)
                .position(|(before, after)| before != after)
                .unwrap_or(0);
            return Err(Error::UnexpectedResponse(format!(
                "config read-back changed a reserved byte at 0x{:X} (device 0x{:02X}, before \
                 0x{:02X}); the reserved regions must be preserved byte-for-byte",
                start + first,
                after.first().copied().unwrap_or(0),
                before.first().copied().unwrap_or(0)
            )));
        }
    }

    for offset in 0..CONFIG_SIZE {
        if CRC_RANGE.contains(&offset) {
            continue;
        }
        let sent = expected.as_bytes()[offset];
        let got = actual.as_bytes()[offset];
        if sent != got {
            return Err(Error::UnexpectedResponse(format!(
                "config read-back mismatch at offset 0x{offset:X} ({}): device 0x{got:02X}, \
                 sent 0x{sent:02X}",
                config_field_name(offset)
            )));
        }
    }

    let expected_crc = expected.crc16_stored() as u16;
    let mut without_stored_crc = *actual.as_bytes();
    without_stored_crc[CRC_RANGE].copy_from_slice(&u32::from(expected_crc).to_le_bytes());
    let crc = ConfigCrcStatus {
        expected: expected_crc,
        stored: actual.crc16_stored() as u16,
        recomputed: ansi_crc16_sh(actual.as_bytes()),
        recomputed_from_sent: ansi_crc16_sh(&without_stored_crc),
    };
    if !crc.is_valid() {
        return Err(Error::UnexpectedResponse(format!(
            "config read-back CRC 0x{:04X} matches neither the sent 0x{:04X}, the in-place \
             recomputation 0x{:04X}, nor the recomputation over the sent buffer 0x{:04X}",
            crc.stored, crc.expected, crc.recomputed, crc.recomputed_from_sent
        )));
    }
    Ok(crc)
}

/// Human-readable section name for a read-back mismatch.
fn config_field_name(offset: usize) -> &'static str {
    if offset < OFF_CRC16 {
        "slot_flags"
    } else if CRC_RANGE.contains(&offset) {
        "crc16"
    } else if offset < 0x014 {
        "header / cur_slot"
    } else if offset < 0x074 {
        "profile name"
    } else if offset < 0x098 {
        "rumble"
    } else if offset < 0x0B0 {
        "joystick"
    } else if offset < 0x0C8 {
        "trigger"
    } else if offset < 0x0E0 {
        "special feature"
    } else if offset < OFF_LEGACY_A {
        "button remap"
    } else if offset < OFF_MACROS {
        "reserved legacy A"
    } else if offset < OFF_LEGACY_B {
        "macro header"
    } else if offset < OFF_SIX_AXIS {
        "reserved legacy B"
    } else if offset < 0x950 {
        "six-axis"
    } else if offset < 0x974 {
        "dynamic theme"
    } else if offset < 0x9A4 {
        "hot-wheel theme"
    } else {
        "single theme"
    }
}

/// Refuse a buffer that differs from `before` in a reserved region.
fn ensure_reserved_unchanged(before: &Bt2Config, after: &Bt2Config) -> Result<(), Error> {
    for (start, end) in RESERVED_REGIONS {
        let before_bytes = &before.as_bytes()[start..end];
        let after_bytes = &after.as_bytes()[start..end];
        if before_bytes != after_bytes {
            let first = before_bytes
                .iter()
                .zip(after_bytes)
                .position(|(before, after)| before != after)
                .unwrap_or(0);
            return Err(Error::InvalidRequest(format!(
                "edit would change reserved configuration byte 0x{:X}; reserved regions must be \
                 preserved",
                start + first
            )));
        }
    }
    Ok(())
}

/// Coalesce every differing byte into maximal contiguous regions.
fn changed_regions(before: &[u8], after: &[u8]) -> Vec<ConfigRegion> {
    let mut regions = Vec::new();
    let mut start: Option<usize> = None;
    for (offset, (before, after)) in before.iter().zip(after).enumerate() {
        if before == after {
            if let Some(start) = start.take() {
                regions.push(ConfigRegion {
                    offset: start,
                    len: offset - start,
                });
            }
        } else if start.is_none() {
            start = Some(offset);
        }
    }
    if let Some(start) = start {
        regions.push(ConfigRegion {
            offset: start,
            len: before.len() - start,
        });
    }
    regions
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Mutex, PoisonError};

    use async_trait::async_trait;
    use eightbitdo_protocol::commands::cmd;
    use eightbitdo_protocol::config_bt2::{
        map_key_entry_offset, map_key_offset, profile_name_offset, rumble_offset, single_offset,
        six_axis_offset, Argb, Slot, CONFIG_SIZE,
    };
    use eightbitdo_protocol::crc::packet_crc16;
    use eightbitdo_protocol::header::{CustomHeader, HEADER_SIZE};
    use eightbitdo_protocol::keymap::{ButtonIndex, KeyMap};

    use super::*;
    use crate::session::Session;

    fn sample_snapshot() -> ConfigSnapshot {
        let mut config = Bt2Config::zeroed();
        for slot in Slot::ALL {
            config.set_slot_flag(slot, 0x2020_0911);
            config.set_profile_name(slot, "Slot").unwrap();
            config.set_rumble(
                slot,
                &RumbleRecord {
                    flag: 1,
                    left: 0.6,
                    right: 0.6,
                },
            );
            config.set_joy(
                slot,
                &JoyRecord {
                    flag: 1,
                    left_start_value: 10,
                    left_end_value: 120,
                    right_start_value: 11,
                    right_end_value: 121,
                },
            );
        }
        config.set_gamepad_mode(GamepadMode::Switch);
        config.set_cur_slot(Some(Slot::S1));
        config.as_mut_bytes()[OFF_LEGACY_A..OFF_MACROS].fill(0xA5);
        config.as_mut_bytes()[OFF_LEGACY_B..OFF_SIX_AXIS].fill(0x5A);
        config.recompute_crc();
        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        ConfigSnapshot {
            crc_valid: stored_crc == computed_crc,
            config,
            stored_crc,
            computed_crc,
        }
    }

    fn name_plan(name: &str) -> ConfigWritePlan {
        ConfigWritePlan::new().with(ConfigEdit::ProfileName {
            slot: Slot::S2,
            name: name.to_string(),
        })
    }

    // --- transport -------------------------------------------------------

    struct ScriptedTransport {
        mtu: u16,
        script: Mutex<VecDeque<Vec<u8>>>,
        incoming: Mutex<VecDeque<Vec<u8>>>,
        sent: Mutex<Vec<Vec<u8>>>,
        stall_when_empty: bool,
    }

    impl ScriptedTransport {
        fn new(script: Vec<Vec<u8>>, mtu: u16) -> Self {
            Self {
                mtu,
                script: Mutex::new(script.into()),
                incoming: Mutex::new(VecDeque::new()),
                sent: Mutex::new(Vec::new()),
                stall_when_empty: false,
            }
        }

        fn stalling(script: Vec<Vec<u8>>, mtu: u16) -> Self {
            Self {
                stall_when_empty: true,
                ..Self::new(script, mtu)
            }
        }

        fn sent(&self) -> Vec<Vec<u8>> {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    #[async_trait]
    impl Transport for ScriptedTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(data.to_vec());
            if let Some(frame) = self
                .script
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
            {
                self.incoming
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push_back(frame);
            }
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            if let Some(frame) = self
                .incoming
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
            {
                return Ok(frame);
            }
            if self.stall_when_empty {
                std::future::pending::<()>().await;
            }
            Err(Error::Disconnected)
        }

        fn mtu(&self) -> u16 {
            self.mtu
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    fn ack(request_cmd: u16) -> Vec<u8> {
        CustomHeader::new(cmd::RESPONSE, request_cmd, 0, 0)
            .to_bytes()
            .to_vec()
    }

    fn save_ack() -> Vec<u8> {
        CustomHeader::new(cmd::SAVE, SaveTransport::Ble.subcmd(), 0, 0)
            .to_bytes()
            .to_vec()
    }

    /// One `0x0002` data response per read request, emulating the unit's
    /// fixed 142-byte chunks (the final chunk is padded).
    fn read_responses(config: &Bt2Config, chunk_len: usize) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let mut offset = 0usize;
        while offset < CONFIG_SIZE {
            let remaining = CONFIG_SIZE - offset;
            let valid = remaining.min(chunk_len);
            let mut payload = config.as_bytes()[offset..offset + valid].to_vec();
            payload.resize(chunk_len, 0);
            let header = CustomHeader::new(
                cmd::RESPONSE,
                cmd::READ_CONFIG,
                CONFIG_SIZE as u32,
                offset as u32,
            )
            .with_payload(&payload);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(&payload);
            frames.push(frame);
            offset += valid;
        }
        frames
    }

    fn short_timeouts() -> ConfigWriteTimeouts {
        ConfigWriteTimeouts {
            write: Duration::from_millis(10),
            save: Duration::from_millis(10),
        }
    }

    fn request_cmd(frame: &[u8]) -> u16 {
        CustomHeader::parse(&frame[1..]).expect("frame header").cmd
    }

    fn count_cmd(sent: &[Vec<u8>], command: u16) -> usize {
        sent.iter()
            .filter(|frame| request_cmd(frame) == command)
            .count()
    }

    async fn run_write(
        prepared: &PreparedConfigWrite,
        script: Vec<Vec<u8>>,
        mtu: u16,
        timeouts: ConfigWriteTimeouts,
    ) -> (Result<ConfigWriteOutcome, Error>, Vec<Vec<u8>>) {
        let snapshot = sample_snapshot();
        let transport = ScriptedTransport::new(script, mtu);
        let session = Session::new(&transport);
        let result = session
            .write_full_config(&snapshot, prepared, timeouts)
            .await;
        (result, transport.sent())
    }

    fn script_with_readback(
        prepared: &PreparedConfigWrite,
        mtu: u16,
        readback: &Bt2Config,
    ) -> Vec<Vec<u8>> {
        let chunk_len = config_write_chunk_len(mtu).unwrap();
        let chunk_count = prepared.expected_bytes().len().div_ceil(chunk_len);
        let mut script = vec![ack(cmd::WRITE_CONFIG); chunk_count];
        script.push(save_ack());
        script.extend(read_responses(readback, 142));
        script
    }

    fn successful_script(prepared: &PreparedConfigWrite, mtu: u16) -> Vec<Vec<u8>> {
        script_with_readback(prepared, mtu, prepared.expected())
    }

    // --- prepare ---------------------------------------------------------

    #[test]
    fn prepare_changes_only_the_edited_record_and_crc() {
        let snapshot = sample_snapshot();
        let plan = name_plan("Slot2");
        let prepared = plan.prepare(&snapshot).unwrap();

        assert!(!prepared.is_noop());
        // The name field (32 B) and the CRC field (4 B) are the only changes.
        let name_start = profile_name_offset(Slot::S2);
        assert!(prepared
            .changed()
            .iter()
            .any(|region| { region.offset() < name_start + 0x20 && region.end() > name_start }));
        assert!(prepared
            .changed()
            .iter()
            .any(|region| { region.offset() < OFF_CRC16 + 4 && region.end() > OFF_CRC16 }));

        let before = snapshot.config.as_bytes();
        let after = prepared.expected_bytes();
        for offset in 0..CONFIG_SIZE {
            let changed = prepared
                .changed()
                .iter()
                .any(|r| r.range().contains(&offset));
            if !changed {
                assert_eq!(before[offset], after[offset], "byte 0x{offset:X} changed");
            }
        }
        for (start, end) in RESERVED_REGIONS {
            assert_eq!(&before[start..end], &after[start..end]);
        }
        assert_eq!(prepared.expected().profile_name(Slot::S2), "Slot2");
        assert_eq!(prepared.expected().profile_name(Slot::S1), "Slot");
    }

    #[test]
    fn config_write_chunk_len_tracks_the_mtu_and_rejects_a_tiny_one() {
        assert_eq!(config_write_chunk_len(200).unwrap(), 180);
        assert_eq!(config_write_chunk_len(64).unwrap(), 44);
        assert_eq!(config_write_chunk_len(23).unwrap(), 3);
        let error = config_write_chunk_len(20).unwrap_err();
        assert!(
            matches!(error, Error::MtuTooSmall { mtu: 20, .. }),
            "{error}"
        );
    }

    #[test]
    fn changed_regions_are_maximal_and_ignore_equal_bytes() {
        let before = [0u8; 8];
        let mut after = before;
        after[1] = 1;
        after[2] = 1;
        after[5] = 1;
        let regions = changed_regions(&before, &after);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].offset(), 1);
        assert_eq!(regions[0].len(), 2);
        assert_eq!(regions[1].offset(), 5);
        assert_eq!(regions[1].len(), 1);
        assert!(changed_regions(&before, &before).is_empty());
    }

    #[test]
    fn prepare_with_no_effective_edit_is_a_noop() {
        let snapshot = sample_snapshot();
        let same_name = snapshot.config.profile_name(Slot::S2);
        let prepared = name_plan(&same_name).prepare(&snapshot).unwrap();
        assert!(prepared.is_noop());
        assert_eq!(prepared.expected(), &snapshot.config);
    }

    #[test]
    fn prepare_edits_known_typed_records() {
        let snapshot = sample_snapshot();
        let mut single = SingleTheme {
            flag: 1,
            ..SingleTheme::default()
        };
        single.colors[7] = Argb(0xFF12_3456);
        let plan = ConfigWritePlan::new()
            .with(ConfigEdit::CurSlot(Some(Slot::S3)))
            .with(ConfigEdit::GamepadMode(GamepadMode::XInput))
            .with(ConfigEdit::Rumble {
                slot: Slot::S2,
                record: RumbleRecord::from_level(2).unwrap(),
            })
            .with(ConfigEdit::Joy {
                slot: Slot::S3,
                record: JoyRecord {
                    flag: 1,
                    left_start_value: 5,
                    left_end_value: 127,
                    right_start_value: 6,
                    right_end_value: 126,
                },
            })
            .with(ConfigEdit::Trigger {
                slot: Slot::S1,
                record: TriggerRecord {
                    flag: 1,
                    left_start_value: 77,
                    left_end_value: 255,
                    right_start_value: 77,
                    right_end_value: 255,
                },
            })
            .with(ConfigEdit::Special {
                slot: Slot::S1,
                record: SpecialRecord {
                    flag: 1,
                    features: eightbitdo_protocol::config_bt2::SpecialFeature::VIBRATION,
                },
            })
            .with(ConfigEdit::SixAxis {
                slot: Slot::S2,
                record: SixAxisRecord {
                    flag: 1,
                    key_map: KeyMap::L2,
                    trigger_mode: 1,
                    sensitivity: 9,
                    dead_compensate: 12,
                    mapping_type: 2,
                },
            })
            .with(ConfigEdit::DynamicTheme {
                slot: Slot::S1,
                theme: DynamicTheme {
                    flag: 1,
                    color: Argb(0xFF11_2233),
                    bg_color: Argb(0xFF44_5566),
                },
            })
            .with(ConfigEdit::HotWheelTheme {
                slot: Slot::S2,
                theme: HotWheelTheme {
                    flag: 1,
                    color: Argb(0xFF77_8899),
                    bg_color: Argb(0xFFAA_BBCC),
                    speed: 15,
                },
            })
            .with(ConfigEdit::SingleTheme {
                slot: Slot::S3,
                theme: single,
            });

        let prepared = plan.prepare(&snapshot).unwrap();
        let config = prepared.expected();
        assert_eq!(config.cur_slot(), Some(Slot::S3));
        assert_eq!(config.gamepad_mode(), Some(GamepadMode::XInput));
        assert_eq!(
            config.rumble(Slot::S2),
            RumbleRecord::from_level(2).unwrap()
        );
        assert_eq!(config.joy(Slot::S3).left_start_value, 5);
        assert_eq!(config.trigger(Slot::S1).left_start_value, 77);
        assert!(config
            .special(Slot::S1)
            .features
            .contains(eightbitdo_protocol::config_bt2::SpecialFeature::VIBRATION));
        assert_eq!(config.six_axis(Slot::S2).sensitivity, 9);
        assert_eq!(config.dynamic_theme(Slot::S1).color, Argb(0xFF11_2233));
        assert_eq!(config.hotwheel_theme(Slot::S2).speed, 15);
        assert_eq!(config.single_theme(Slot::S3), single);
        for (start, end) in RESERVED_REGIONS {
            assert_eq!(
                &snapshot.config.as_bytes()[start..end],
                &prepared.expected_bytes()[start..end]
            );
        }
        let overlaps = |start: usize, len: usize| {
            prepared
                .changed()
                .iter()
                .any(|region| region.offset() < start + len && region.end() > start)
        };
        assert!(overlaps(
            six_axis_offset(Slot::S2),
            eightbitdo_protocol::config_bt2::SIX_AXIS_SIZE
        ));
        assert!(overlaps(
            single_offset(Slot::S3),
            eightbitdo_protocol::config_bt2::SINGLE_SIZE
        ));
        assert!(overlaps(
            rumble_offset(Slot::S2),
            eightbitdo_protocol::config_bt2::RUMBLE_SIZE
        ));
    }

    #[test]
    fn from_buffer_rejects_reserved_region_changes() {
        let snapshot = sample_snapshot();
        let mut edited = snapshot.config.clone();
        edited.as_mut_bytes()[OFF_LEGACY_A] ^= 0xFF;
        let error = PreparedConfigWrite::from_buffer(&snapshot, edited).unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)), "{error}");
        assert!(error.to_string().contains("reserved"));
    }

    #[test]
    fn from_buffer_recomputes_crc_and_reports_only_real_changes() {
        let snapshot = sample_snapshot();
        let mut edited = snapshot.config.clone();
        edited.set_profile_name(Slot::S1, "Edited").unwrap();
        // A stale CRC field must be replaced by the recomputed value.
        edited.as_mut_bytes()[OFF_CRC16..OFF_CRC16 + 4].fill(0xEE);
        let prepared = PreparedConfigWrite::from_buffer(&snapshot, edited).unwrap();
        assert!(!prepared.is_noop());
        let name_start = profile_name_offset(Slot::S1);
        assert!(prepared
            .changed()
            .iter()
            .any(|region| { region.offset() < name_start + 0x20 && region.end() > name_start }));
        assert!(prepared
            .changed()
            .iter()
            .any(|region| region.range().contains(&OFF_CRC16)));
        assert_ne!(
            prepared.expected().crc16_stored(),
            0xEEEE_EEEE,
            "the stale CRC field must be replaced by the recomputed value"
        );
        // The reserved regions are untouched.
        for (start, end) in RESERVED_REGIONS {
            assert_eq!(
                &snapshot.config.as_bytes()[start..end],
                &prepared.expected_bytes()[start..end]
            );
        }
    }

    // --- write path ------------------------------------------------------

    #[tokio::test]
    async fn write_full_config_sends_chunks_save_and_verifies_readback() {
        let snapshot = sample_snapshot();
        let prepared = name_plan("Slot2").prepare(&snapshot).unwrap();
        let expected = prepared.expected().clone();
        let changed = prepared.changed().to_vec();
        let script = successful_script(&prepared, 200);

        let (result, sent) = run_write(&prepared, script, 200, short_timeouts()).await;
        let outcome = result.expect("write should succeed");

        let chunk_len = config_write_chunk_len(200).unwrap();
        assert_eq!(chunk_len, 180);
        let write_frames: Vec<&Vec<u8>> = sent
            .iter()
            .filter(|frame| request_cmd(frame) == cmd::WRITE_CONFIG)
            .collect();
        assert_eq!(write_frames.len(), CONFIG_SIZE.div_ceil(chunk_len));
        assert_eq!(outcome.packets_sent, write_frames.len() + 1);
        assert_eq!(count_cmd(&sent, cmd::SAVE), 1);

        // Every chunk: correct offset, total_len, payload length and CRC.
        let mut expected_offset = 0usize;
        for frame in &write_frames {
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.subcmd, 0);
            assert_eq!(header.total_len, CONFIG_SIZE as u32);
            assert_eq!(header.offset as usize, expected_offset);
            let payload = &frame[1 + HEADER_SIZE..];
            let sent_slice =
                &expected.as_bytes()[expected_offset..expected_offset + header.chunk_len as usize];
            assert_eq!(payload, sent_slice);
            assert_eq!(header.crc16, packet_crc16(&frame[1 + HEADER_SIZE..]));
            expected_offset += header.chunk_len as usize;
        }
        assert_eq!(expected_offset, CONFIG_SIZE);
        assert_eq!(outcome.changed, changed);
        assert_eq!(outcome.read_back, expected);
        assert!(outcome.crc.matches_sent());
        assert_eq!(outcome.crc.expected, expected.crc16_stored() as u16);
    }

    #[tokio::test]
    async fn write_full_config_uses_mtu_sized_chunks_for_smaller_mtus() {
        let snapshot = sample_snapshot();
        let prepared = name_plan("Tiny").prepare(&snapshot).unwrap();
        let mtu = 64;
        let script = successful_script(&prepared, mtu);

        let (result, sent) = run_write(&prepared, script, mtu, short_timeouts()).await;
        let outcome = result.expect("write should succeed");

        let chunk_len = config_write_chunk_len(mtu).unwrap();
        assert_eq!(chunk_len, 44);
        let mut payload_total = 0usize;
        for frame in &sent {
            if request_cmd(frame) != cmd::WRITE_CONFIG {
                continue;
            }
            assert!(frame.len() <= usize::from(mtu) - 3, "frame over MTU");
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert!(header.chunk_len as usize <= chunk_len);
            payload_total += header.chunk_len as usize;
        }
        assert_eq!(payload_total, CONFIG_SIZE);
        assert_eq!(outcome.read_back, *prepared.expected());
    }

    #[tokio::test]
    async fn noop_write_is_refused_before_sending_anything() {
        let snapshot = sample_snapshot();
        let prepared = ConfigWritePlan::new().prepare(&snapshot).unwrap();
        assert!(prepared.is_noop());
        let transport = ScriptedTransport::new(Vec::new(), 200);
        let session = Session::new(&transport);
        let error = session
            .write_full_config(&snapshot, &prepared, short_timeouts())
            .await
            .unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)), "{error}");
        assert!(transport.sent().is_empty());
    }

    #[tokio::test]
    async fn write_ack_failure_is_fail_closed_and_never_saves() {
        let snapshot = sample_snapshot();
        let prepared = name_plan("Slot2").prepare(&snapshot).unwrap();
        // Acknowledge nothing: the first chunk ACK times out.
        let snapshot_for_run = sample_snapshot();
        let transport = ScriptedTransport::stalling(Vec::new(), 200);
        let session = Session::new(&transport);
        let result = session
            .write_full_config(&snapshot_for_run, &prepared, short_timeouts())
            .await;
        let sent = transport.sent();
        let error = result.unwrap_err();
        match error {
            Error::ConfigWrite {
                stage,
                state,
                reason,
                recovery,
            } => {
                assert_eq!(stage, ConfigWriteStage::Write);
                assert_eq!(state, ConfigWriteState::NotSaved);
                assert!(reason.contains("WriteConfig"), "{reason}");
                assert!(recovery.contains("flash configuration is unchanged"));
            }
            other => panic!("expected ConfigWrite, got {other}"),
        }
        assert_eq!(count_cmd(&sent, cmd::SAVE), 0);
        assert_eq!(count_cmd(&sent, cmd::WRITE_CONFIG), 1);
    }

    #[tokio::test]
    async fn save_ack_timeout_reports_not_saved_but_save_was_sent() {
        let snapshot = sample_snapshot();
        let prepared = name_plan("Slot2").prepare(&snapshot).unwrap();
        let chunk_len = config_write_chunk_len(200).unwrap();
        let chunk_count = CONFIG_SIZE.div_ceil(chunk_len);
        let script = vec![ack(cmd::WRITE_CONFIG); chunk_count];
        let transport = ScriptedTransport::stalling(script, 200);
        let session = Session::new(&transport);
        let error = session
            .write_full_config(&snapshot, &prepared, short_timeouts())
            .await
            .unwrap_err();

        match error {
            Error::ConfigWrite {
                stage,
                state,
                recovery,
                ..
            } => {
                assert_eq!(stage, ConfigWriteStage::Save);
                assert_eq!(state, ConfigWriteState::NotSaved);
                assert!(recovery.contains("commit is unconfirmed"));
            }
            other => panic!("expected ConfigWrite, got {other}"),
        }
        let sent = transport.sent();
        assert_eq!(count_cmd(&sent, cmd::SAVE), 1);
    }

    #[tokio::test]
    async fn readback_accepts_a_device_recomputed_crc() {
        let snapshot = sample_snapshot();
        let prepared = name_plan("Slot2").prepare(&snapshot).unwrap();
        // If the device re-runs the official in-place recomputation over the
        // received buffer on Save, it stores ansi(buffer as sent).
        let recomputed_on_save = ansi_crc16_sh(prepared.expected_bytes());
        let mut device_view = prepared.expected().clone();
        device_view.as_mut_bytes()[CRC_RANGE]
            .copy_from_slice(&u32::from(recomputed_on_save).to_le_bytes());
        let script = script_with_readback(&prepared, 200, &device_view);

        let (result, _sent) = run_write(&prepared, script, 200, short_timeouts()).await;
        let outcome = result.expect("a device recomputed CRC must be accepted");
        assert_eq!(outcome.crc.stored, recomputed_on_save);
        assert!(!outcome.crc.matches_sent());
        assert!(outcome.crc.from_sent());
        assert!(outcome.crc.is_valid());
        assert_eq!(outcome.read_back, device_view);
    }

    #[tokio::test]
    async fn readback_mismatch_is_fail_closed_after_save() {
        let snapshot = sample_snapshot();
        let prepared = name_plan("Slot2").prepare(&snapshot).unwrap();
        let mut tampered = prepared.expected().clone();
        tampered.set_profile_name(Slot::S1, "Wrong").unwrap();
        let chunk_len = config_write_chunk_len(200).unwrap();
        let chunk_count = CONFIG_SIZE.div_ceil(chunk_len);
        let mut script = vec![ack(cmd::WRITE_CONFIG); chunk_count];
        script.push(save_ack());
        script.extend(read_responses(&tampered, 142));

        let (result, sent) = run_write(&prepared, script, 200, short_timeouts()).await;
        let error = result.unwrap_err();
        match error {
            Error::ConfigWrite {
                stage,
                state,
                reason,
                recovery,
            } => {
                assert_eq!(stage, ConfigWriteStage::Verify);
                assert_eq!(state, ConfigWriteState::Saved);
                assert!(reason.contains("read-back mismatch"), "{reason}");
                assert!(recovery.contains("committed"));
            }
            other => panic!("expected ConfigWrite, got {other}"),
        }
        assert_eq!(count_cmd(&sent, cmd::SAVE), 1);
    }

    #[tokio::test]
    async fn readback_crc_mismatch_is_fail_closed() {
        let snapshot = sample_snapshot();
        let prepared = name_plan("Slot2").prepare(&snapshot).unwrap();
        let expected_crc = prepared.expected().crc16_stored() as u16;
        let mut tampered = prepared.expected().clone();
        let mut attempts = 0u32;
        loop {
            let candidate = expected_crc ^ (0xFFFFu16.wrapping_mul(1 + attempts as u16));
            tampered.as_mut_bytes()[CRC_RANGE].copy_from_slice(&u32::from(candidate).to_le_bytes());
            let recomputed = ansi_crc16_sh(tampered.as_bytes());
            let mut sent_view = *tampered.as_bytes();
            sent_view[CRC_RANGE].copy_from_slice(&u32::from(expected_crc).to_le_bytes());
            let from_sent = ansi_crc16_sh(&sent_view);
            if candidate != expected_crc && candidate != recomputed && candidate != from_sent {
                break;
            }
            attempts += 1;
            assert!(attempts < 10, "could not build an invalid CRC");
        }

        let chunk_len = config_write_chunk_len(200).unwrap();
        let chunk_count = CONFIG_SIZE.div_ceil(chunk_len);
        let mut script = vec![ack(cmd::WRITE_CONFIG); chunk_count];
        script.push(save_ack());
        script.extend(read_responses(&tampered, 142));

        let (result, _sent) = run_write(&prepared, script, 200, short_timeouts()).await;
        let error = result.unwrap_err();
        match error {
            Error::ConfigWrite {
                stage,
                state,
                reason,
                ..
            } => {
                assert_eq!(stage, ConfigWriteStage::Verify);
                assert_eq!(state, ConfigWriteState::Saved);
                assert!(reason.contains("CRC"), "{reason}");
            }
            other => panic!("expected ConfigWrite, got {other}"),
        }
    }

    // --- button remapping (RE-confirmed MapKeyRecord layout) ------------

    #[test]
    fn button_remap_offsets_match_the_re_formula() {
        for (slot_index, slot) in Slot::ALL.into_iter().enumerate() {
            assert_eq!(map_key_offset(slot), 0x0E0 + slot_index * 0x5C);
            for (index, button) in ButtonIndex::ALL.into_iter().enumerate() {
                assert_eq!(
                    map_key_entry_offset(slot, button),
                    0x0E4 + slot_index * 0x5C + index * 4,
                    "slot {slot_index} index {index}"
                );
            }
        }
        // Spot values from the RE document: slot 0 index 0, slot 2 index 21.
        assert_eq!(map_key_entry_offset(Slot::S1, ButtonIndex::A), 0x0E4);
        assert_eq!(
            map_key_entry_offset(Slot::S3, ButtonIndex::P4),
            0x0E4 + 2 * 0x5C + 21 * 4
        );
    }

    #[test]
    fn button_remap_roundtrips_every_index() {
        let snapshot = sample_snapshot();
        let original = snapshot.config.map_key(Slot::S2);
        for (index, button) in ButtonIndex::ALL.into_iter().enumerate() {
            let target = match index % 4 {
                0 => KeyMap::A,
                1 => KeyMap::B,
                2 => KeyMap::P1,
                _ => KeyMap::NONE,
            };
            let prepared = ConfigWritePlan::new()
                .with(ConfigEdit::ButtonRemap {
                    slot: Slot::S2,
                    button,
                    to: target,
                })
                .prepare(&snapshot)
                .unwrap();
            assert_eq!(
                button_mapping(prepared.expected(), Slot::S2, button),
                target
            );
            let record = prepared.expected().map_key(Slot::S2);
            for (other_index, other) in ButtonIndex::ALL.into_iter().enumerate() {
                if other_index == index {
                    continue;
                }
                assert_eq!(
                    record.keys[other_index], original.keys[other_index],
                    "index {index} changed unrelated entry {other_index} ({other:?})"
                );
            }
            assert_eq!(
                prepared.expected().map_key(Slot::S1),
                snapshot.config.map_key(Slot::S1)
            );
            assert_eq!(
                prepared.expected().map_key(Slot::S3),
                snapshot.config.map_key(Slot::S3)
            );
        }
    }

    #[test]
    fn button_remap_changes_only_the_entry_and_the_crc() {
        let snapshot = sample_snapshot();
        let button = ButtonIndex::P2;
        let prepared = ConfigWritePlan::new()
            .with(ConfigEdit::ButtonRemap {
                slot: Slot::S3,
                button,
                to: KeyMap::L2,
            })
            .prepare(&snapshot)
            .unwrap();
        let entry = map_key_entry_offset(Slot::S3, button);
        assert_eq!(
            prepared.changed().len(),
            2,
            "entry + CRC only: {:?}",
            prepared.changed()
        );
        // `changed()` reports the bytes that actually differ, so a single
        // 4-byte entry update can be a shorter run. Require every region to be
        // inside either the edited entry or the CRC field.
        for region in prepared.changed() {
            let in_entry = region.offset() >= entry && region.end() <= entry + 4;
            let in_crc = region.offset() >= OFF_CRC16 && region.end() <= OFF_CRC16 + 4;
            assert!(
                in_entry || in_crc,
                "unexpected changed region {region:?} (entry 0x{entry:X}, crc {CRC_RANGE:?})"
            );
        }
        assert!(prepared
            .changed()
            .iter()
            .any(|region| region.offset() >= entry && region.offset() < entry + 4));
        for (start, end) in RESERVED_REGIONS {
            assert_eq!(
                &snapshot.config.as_bytes()[start..end],
                &prepared.expected_bytes()[start..end]
            );
        }
        // The record flag is not changed by a single-entry remap.
        assert_eq!(
            map_key_flag(prepared.expected(), Slot::S3),
            map_key_flag(&snapshot.config, Slot::S3)
        );
        assert_eq!(
            button_mapping(prepared.expected(), Slot::S3, button),
            KeyMap::L2
        );
    }

    #[test]
    fn map_key_flag_edit_only_changes_the_flag() {
        let snapshot = sample_snapshot();
        let keys_before = snapshot.config.map_key(Slot::S1).keys;
        let prepared = ConfigWritePlan::new()
            .with(ConfigEdit::MapKeyFlag {
                slot: Slot::S1,
                flag: 1,
            })
            .prepare(&snapshot)
            .unwrap();
        assert_eq!(map_key_flag(prepared.expected(), Slot::S1), 1);
        assert_eq!(prepared.expected().map_key(Slot::S1).keys, keys_before);
        let flag_offset = map_key_offset(Slot::S1);
        assert!(prepared
            .changed()
            .iter()
            .any(|region| region.offset() >= flag_offset && region.offset() < flag_offset + 4));
    }

    #[tokio::test]
    async fn button_remap_is_written_and_verified_by_a_full_config_write() {
        let snapshot = sample_snapshot();
        let prepared = ConfigWritePlan::new()
            .with(ConfigEdit::ButtonRemap {
                slot: Slot::S2,
                button: ButtonIndex::P4,
                to: KeyMap::P2,
            })
            .with(ConfigEdit::MapKeyFlag {
                slot: Slot::S2,
                flag: 1,
            })
            .prepare(&snapshot)
            .unwrap();
        let script = successful_script(&prepared, 200);
        let (result, sent) = run_write(&prepared, script, 200, short_timeouts()).await;
        let outcome = result.expect("remap full write should succeed");
        assert_eq!(
            button_mapping(&outcome.read_back, Slot::S2, ButtonIndex::P4),
            KeyMap::P2
        );
        assert_eq!(map_key_flag(&outcome.read_back, Slot::S2), 1);
        assert_eq!(count_cmd(&sent, cmd::SAVE), 1);
        let entry = map_key_entry_offset(Slot::S2, ButtonIndex::P4);
        let flag_offset = map_key_offset(Slot::S2);
        assert!(outcome
            .changed
            .iter()
            .any(|region| region.offset() >= entry && region.offset() < entry + 4));
        assert!(outcome
            .changed
            .iter()
            .any(|region| { region.offset() >= flag_offset && region.offset() < flag_offset + 4 }));
    }

    #[tokio::test]
    async fn readback_of_changed_reserved_bytes_is_fail_closed() {
        let snapshot = sample_snapshot();
        let prepared = name_plan("Slot2").prepare(&snapshot).unwrap();
        let mut tampered = prepared.expected().clone();
        tampered.as_mut_bytes()[OFF_LEGACY_A] ^= 0xFF;
        tampered.recompute_crc();
        let chunk_len = config_write_chunk_len(200).unwrap();
        let chunk_count = CONFIG_SIZE.div_ceil(chunk_len);
        let mut script = vec![ack(cmd::WRITE_CONFIG); chunk_count];
        script.push(save_ack());
        script.extend(read_responses(&tampered, 142));

        let (result, _sent) = run_write(&prepared, script, 200, short_timeouts()).await;
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            Error::ConfigWrite {
                stage: ConfigWriteStage::Verify,
                state: ConfigWriteState::Saved,
                ..
            }
        ));
        assert!(error.to_string().contains("reserved"));
    }
}
