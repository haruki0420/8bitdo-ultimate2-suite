//! Macro flash read and write operations for all 3 slots × 4 macro indices.
//!
//! The macro step payload lives outside the 2768-byte configuration block.
//! Each macro owns one 4 KB flash sector (`macro_idx << 12`) and its length is
//! taken from the `max_steps` field of the matching `MacroFunPro3Record` in
//! the configuration read. One step is 10 bytes.
//!
//! The write state machine requires a [`MacroBackup`] (all 12 entries) and
//! never sends `Save` (`0x0006`) if Erase, Write or the header update fails.

use std::time::Duration;

use eightbitdo_protocol::commands::{self, cmd, SaveTransport};
use eightbitdo_protocol::config_bt2::{
    macro_slot_offset, Bt2Config, MacroIndex, MacroRecord, MacroSlot, Slot, CONFIG_SIZE,
    MACROS_PER_SLOT, MACRO_RECORD_SIZE, MACRO_SLOT_SIZE, SLOT_COUNT,
};
use eightbitdo_protocol::header::{CustomHeader, PacketKind, HEADER_SIZE};
use eightbitdo_protocol::macro_data::{self, MacroStep};
use eightbitdo_protocol::KeyMap;
use tokio::time::Instant;

use crate::error::{Error, MacroWriteStage};
use crate::session::{
    deadline_after, ConfigSnapshot, Session, TransferBuffer, CONFIG_TOTAL_TIMEOUT,
};
use crate::transport::Transport;

/// Payload budget for one BLE frame: `mtu - 3` (ATT overhead) minus the
/// 1-byte packet prefix and 16-byte custom header.
///
/// This is the value the macro chunk sizes must respect; using `mtu - 17`
/// alone would exceed [`crate::ble::BleTransport`]'s `mtu - 3` send limit.
#[must_use]
pub fn max_protocol_payload(mtu: u16) -> usize {
    usize::from(mtu.saturating_sub(3)).saturating_sub(17)
}

/// Header (`0x0001`) chunk length for the negotiated MTU.
///
/// # Errors
///
/// Returns [`Error::MtuTooSmall`] when no payload byte fits.
pub fn macro_header_chunk_len(mtu: u16) -> Result<usize, Error> {
    let max_payload = max_protocol_payload(mtu);
    if max_payload == 0 {
        return Err(Error::MtuTooSmall {
            mtu,
            reason: "no room for 1-byte prefix + 16-byte header".to_string(),
        });
    }
    Ok(max_payload.min(MACRO_HEADER_CHUNK_SIZE))
}

/// `0x0103` chunk length for the negotiated MTU.
///
/// Macro writes support only 32- and 64-byte chunks (32-byte alignment).
///
/// # Errors
///
/// Returns [`Error::MtuTooSmall`] when even a 32-byte chunk does not fit.
pub fn macro_write_chunk_len(mtu: u16) -> Result<usize, Error> {
    let max_payload = max_protocol_payload(mtu);
    if max_payload >= commands::MACRO_MAX_CHUNK_SIZE {
        Ok(commands::MACRO_MAX_CHUNK_SIZE)
    } else if max_payload >= commands::MACRO_USB_CHUNK_SIZE {
        Ok(commands::MACRO_USB_CHUNK_SIZE)
    } else {
        Err(Error::MtuTooSmall {
            mtu,
            reason: format!(
                "macro writes need a 32-byte chunk but only {max_payload} bytes fit after \
                 the 1-byte prefix, 16-byte header and 3-byte ATT overhead"
            ),
        })
    }
}

/// Overall deadline for assembling one macro read.
///
/// A full 200-step macro is 2000 bytes and may arrive in several
/// notifications. The per-frame wait is the session timeout
/// ([`Session::with_timeout`]).
pub const MACRO_READ_TOTAL_TIMEOUT: Duration = crate::session::CONFIG_TOTAL_TIMEOUT;

/// ACK wait after `EraseBT2Macro` (`0x0104`, official 2 s).
pub const MACRO_ERASE_TIMEOUT: Duration = Duration::from_secs(2);

/// ACK wait after each `WriteBT2Macro` chunk (`0x0103`, official 2 s).
pub const MACRO_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// ACK wait after the `0x0001` header update (official 2 s).
pub const MACRO_HEADER_TIMEOUT: Duration = Duration::from_secs(2);

/// Header payload chunk size used for `0x0001`.
///
/// A 216-byte header does not fit one BLE frame at MTU 200: `1 + 16 + 216 =
/// 233` exceeds the usable `mtu - 3 = 197` bytes. The default is 142 bytes,
/// further reduced by the negotiated MTU. `total_len` stays `0x0AD0` and each
/// packet's `offset` advances by the actual chunk length.
pub const MACRO_HEADER_CHUNK_SIZE: usize = 142;

/// ACK wait after `Save` (`0x0006`).
///
/// 3 s is the documented Windows USB HID value. The BLE timeout is not
/// documented; this implementation uses the same 3 s and records the
/// uncertainty in `docs/PROTOCOL_SPEC.md`.
pub const MACRO_SAVE_TIMEOUT: Duration = Duration::from_secs(3);

/// Wait after the Erase ACK before starting Write.
///
/// The controller acknowledges Erase before the 4 KB sector is necessarily
/// ready for the next operation. 100 ms is a provisional settle delay based on
/// the observed macro-1 read-back failure; tune it from hardware measurements.
pub const MACRO_ERASE_SETTLE_DELAY: Duration = Duration::from_millis(100);

/// Wait after the final Write chunk ACK before the Header update.
///
/// 50 ms is provisional; the flash write completion time is not documented.
pub const MACRO_WRITE_SETTLE_DELAY: Duration = Duration::from_millis(50);

/// Wait after Save before reading the configuration back.
///
/// Extended from 100 ms to 300 ms after a hardware restore where all ACKs
/// arrived but the immediate `0x0102` read returned zeros. 300 ms is still
/// provisional and must be tuned from hardware measurements.
pub const MACRO_VERIFY_DELAY: Duration = Duration::from_millis(300);

/// Per-stage ACK timeouts for a macro write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroTimeouts {
    /// `0x0104` erase ACK wait.
    pub erase: Duration,
    /// Each `0x0103` chunk ACK wait.
    pub write: Duration,
    /// `0x0001` header ACK wait.
    pub header: Duration,
    /// `0x0006` save ACK wait.
    pub save: Duration,
}

impl Default for MacroTimeouts {
    fn default() -> Self {
        Self {
            erase: MACRO_ERASE_TIMEOUT,
            write: MACRO_WRITE_TIMEOUT,
            header: MACRO_HEADER_TIMEOUT,
            save: MACRO_SAVE_TIMEOUT,
        }
    }
}

/// How to update one macro and its slot header.
///
/// Every `Option` field preserves the value from the backup header when
/// `None`. The CLI decides whether an unused macro (`max_steps == 0`) should
/// enable the slot flag and raise `total_cnt`; the core does not guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroWritePlan {
    /// New 10-byte steps (`1..=200`).
    pub steps: Vec<MacroStep>,
    /// New name; `None` preserves the existing name.
    pub name: Option<String>,
    /// Trigger key mask; `None` preserves the existing `key_map`.
    pub key_map: Option<KeyMap>,
    /// Loop count; `None` preserves the existing value.
    pub cycles_num: Option<u32>,
    /// Inter-loop delay; `None` preserves the existing value.
    pub interval_ms: Option<u32>,
    /// Slot macro-enabled flag; `None` preserves the existing value.
    pub flag: Option<u32>,
    /// Registered macro count (`0..=4`); `None` preserves the existing value.
    pub total_cnt: Option<u8>,
    /// Save sub-command variant.
    ///
    /// Only [`SaveTransport::Ble`] is implemented; the USB path is rejected
    /// with [`Error::Unsupported`].
    pub save_transport: SaveTransport,
    /// ACK timeouts.
    pub timeouts: MacroTimeouts,
}

impl MacroWritePlan {
    /// A plan with 64-byte chunks, BLE Save and default timeouts.
    #[must_use]
    pub fn new(steps: Vec<MacroStep>) -> Self {
        Self {
            steps,
            name: None,
            key_map: None,
            cycles_num: None,
            interval_ms: None,
            flag: None,
            total_cnt: None,
            save_transport: SaveTransport::Ble,
            timeouts: MacroTimeouts::default(),
        }
    }

    /// Set the macro name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the trigger key mask.
    #[must_use]
    pub fn with_key_map(mut self, key_map: KeyMap) -> Self {
        self.key_map = Some(key_map);
        self
    }

    /// Set the loop count.
    #[must_use]
    pub fn with_cycles_num(mut self, cycles_num: u32) -> Self {
        self.cycles_num = Some(cycles_num);
        self
    }

    /// Set the inter-loop delay.
    #[must_use]
    pub fn with_interval_ms(mut self, interval_ms: u32) -> Self {
        self.interval_ms = Some(interval_ms);
        self
    }

    /// Set the slot flag and registered count explicitly.
    #[must_use]
    pub fn with_header_state(mut self, flag: u32, total_cnt: u8) -> Self {
        self.flag = Some(flag);
        self.total_cnt = Some(total_cnt);
        self
    }

    /// Set the Save sub-command variant.
    #[must_use]
    pub fn with_save_transport(mut self, save_transport: SaveTransport) -> Self {
        self.save_transport = save_transport;
        self
    }

    /// Override the ACK timeouts (used by tests and future tuning).
    #[must_use]
    pub fn with_timeouts(mut self, timeouts: MacroTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }
}

/// Result of a confirmed macro write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroWriteOutcome {
    /// Final header record written for this macro.
    pub record: MacroRecord,
    /// Slot macro-enabled flag after the update.
    pub flag: u32,
    /// Registered macro count after the update.
    pub total_cnt: u8,
    /// Total packets sent, including Erase, every Write chunk, Header and Save.
    pub packets_sent: usize,
    /// Read-back entry that matched the requested steps.
    pub readback: MacroEntry,
}

/// Result of a header-only write (`0x0001` + `0x0006`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroHeaderOutcome {
    /// Slot macro-enabled flag after the update.
    pub flag: u32,
    /// Registered macro count after the update.
    pub total_cnt: u8,
    /// Header chunk packets plus Save.
    pub packets_sent: usize,
}

/// One macro read from the controller.
///
/// `raw` is exactly `max_steps * 10` bytes (the flash read length, without
/// the 32-byte write alignment padding). `steps` is the decoded form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroEntry {
    slot: Slot,
    index: MacroIndex,
    record: MacroRecord,
    raw: Vec<u8>,
    steps: Vec<MacroStep>,
    read_offsets: Vec<u32>,
}

impl MacroEntry {
    /// Build an entry from a configuration record and raw step bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Protocol`] with [`eightbitdo_protocol::Error::InvalidLength`]
    /// when `raw.len() != record.max_steps * 10`, or
    /// [`eightbitdo_protocol::Error::TooManySteps`] when the record declares
    /// more than the 200-step maximum.
    pub fn new(
        slot: Slot,
        index: MacroIndex,
        record: MacroRecord,
        raw: Vec<u8>,
    ) -> Result<Self, Error> {
        let expected = if is_skipped_steps(record.max_steps) {
            0
        } else {
            usize::from(record.max_steps) * macro_data::MACRO_STEP_SIZE
        };
        if raw.len() != expected {
            return Err(Error::Protocol(eightbitdo_protocol::Error::InvalidLength {
                expected,
                actual: raw.len(),
            }));
        }
        let steps = if expected == 0 {
            Vec::new()
        } else {
            macro_data::decode_macro_steps(&raw, usize::from(record.max_steps))?
        };
        Ok(Self {
            slot,
            index,
            record,
            raw,
            steps,
            read_offsets: Vec::new(),
        })
    }

    /// Record the raw response offsets observed while reading this macro.
    ///
    /// The device may echo the requested offset, report `0` as its
    /// sequential-chunk convention, or report a sector-relative offset.
    /// Keeping the values makes a real-device log distinguish those cases.
    pub(crate) fn with_read_offsets(mut self, offsets: Vec<u32>) -> Self {
        self.read_offsets = offsets;
        self
    }

    /// Slot (`0..=2`).
    #[must_use]
    pub const fn slot(&self) -> Slot {
        self.slot
    }

    /// Macro index (`0..=3`).
    #[must_use]
    pub const fn index(&self) -> MacroIndex {
        self.index
    }

    /// The header record this entry was read against.
    #[must_use]
    pub const fn record(&self) -> &MacroRecord {
        &self.record
    }

    /// Raw step bytes, exactly `max_steps * 10` long.
    #[must_use]
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    /// Decoded steps.
    #[must_use]
    pub fn steps(&self) -> &[MacroStep] {
        &self.steps
    }

    /// Raw `offset` values in the response headers, one per received chunk.
    ///
    /// Empty for a skipped macro. A value of `0` after the first chunk means
    /// the device used its sequential-chunk convention; the requested offset
    /// means it echoed the request. This is diagnostic data for confirming
    /// real-device behaviour.
    #[must_use]
    pub fn read_offsets(&self) -> &[u32] {
        &self.read_offsets
    }

    /// `true` when the header declares zero steps or the unset marker, so no
    /// flash read was made.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        is_skipped_steps(self.record.max_steps)
    }
}

/// `max_steps` values that mean "no macro data to read".
///
/// `0` is documented as an empty macro. `0xFFFF` is an **unconfirmed
/// candidate for an unset marker** (未確認の未設定マーカー候補); it is skipped
/// rather than turned into a 655,350-byte read. Both are recorded in the
/// backup JSON.
#[must_use]
pub const fn is_skipped_steps(max_steps: u16) -> bool {
    max_steps == 0 || max_steps == u16::MAX
}

/// A complete 3-slot × 4-macro backup, plus the configuration it belongs to.
///
/// Constructing one requires all 12 entries and records that match the
/// supplied configuration snapshot. The write path will take this type as a
/// precondition so it cannot start without a successful full backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroBackup {
    snapshot: ConfigSnapshot,
    entries: Vec<MacroEntry>,
}

impl MacroBackup {
    /// Assemble a backup from a configuration snapshot and all 12 entries.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedResponse`] unless every slot / macro index
    /// appears exactly once and every entry record matches the snapshot.
    pub fn new(snapshot: ConfigSnapshot, entries: Vec<MacroEntry>) -> Result<Self, Error> {
        if entries.len() != SLOT_COUNT * MACROS_PER_SLOT {
            return Err(Error::UnexpectedResponse(format!(
                "macro backup needs {} entries (3 slots x 4 macros), got {}",
                SLOT_COUNT * MACROS_PER_SLOT,
                entries.len()
            )));
        }
        for slot in Slot::ALL {
            for index in MacroIndex::ALL {
                let mut matches = entries
                    .iter()
                    .filter(|entry| entry.slot == slot && entry.index == index);
                let Some(entry) = matches.next() else {
                    return Err(Error::UnexpectedResponse(format!(
                        "macro backup is missing slot {} macro {}",
                        slot.index(),
                        index.index()
                    )));
                };
                if matches.next().is_some() {
                    return Err(Error::UnexpectedResponse(format!(
                        "macro backup repeats slot {} macro {}",
                        slot.index(),
                        index.index()
                    )));
                }
                let expected = snapshot.config.macro_record(slot, index);
                if entry.record != expected {
                    return Err(Error::UnexpectedResponse(format!(
                        "macro backup header for slot {} macro {} does not match the config snapshot",
                        slot.index(),
                        index.index()
                    )));
                }
            }
        }
        Ok(Self { snapshot, entries })
    }

    /// The configuration snapshot this backup was read with.
    #[must_use]
    pub const fn snapshot(&self) -> &ConfigSnapshot {
        &self.snapshot
    }

    /// All 12 entries.
    #[must_use]
    pub fn entries(&self) -> &[MacroEntry] {
        &self.entries
    }

    /// One entry, if present.
    #[must_use]
    pub fn get(&self, slot: Slot, index: MacroIndex) -> Option<&MacroEntry> {
        self.entries
            .iter()
            .find(|entry| entry.slot == slot && entry.index == index)
    }
}

impl<T: Transport> Session<T> {
    /// Read one macro using the `max_steps` in `snapshot`.
    ///
    /// A header with `max_steps == 0` returns an empty entry without sending
    /// a flash read. Otherwise each chunk is requested with payload-less
    /// `cmd 0x0102`, `subcmd = (gamepad_mode << 8) | slot`,
    /// `offset = (macro_idx << 12) + chunk_offset`, and
    /// `total_len = chunk_len = steps * 10`.
    ///
    /// Responses may be a direct `cmd 0x0102` frame or the
    /// `cmd 0x0004, subcmd 0x0102` envelope. The response offset mode is
    /// fixed for the whole macro: absolute echo, sector-relative, or a first
    /// `0` that resolves on the next chunk. Mixing absolute and relative
    /// echoes is rejected so a stale response cannot be spliced into another
    /// macro.
    ///
    /// # Errors
    ///
    /// Returns a protocol error for an out-of-range `max_steps`, a timeout
    /// when the transfer does not complete, or [`Error::UnexpectedResponse`]
    /// for a mismatched envelope, `total_len`, offset, CRC or overlap.
    pub async fn read_macro(
        &self,
        snapshot: &ConfigSnapshot,
        slot: Slot,
        index: MacroIndex,
    ) -> Result<MacroEntry, Error> {
        self.prepare_read().await?;
        self.run_read(self.read_macro_with_config(&snapshot.config, slot, index))
            .await
    }

    /// Read all 3 slots × 4 macros.
    ///
    /// Stops at the first failed read and returns its error; no partial
    /// backup is returned.
    ///
    /// # Errors
    ///
    /// Returns the first macro read error, or a backup consistency error.
    pub async fn read_macro_backup(&self, snapshot: &ConfigSnapshot) -> Result<MacroBackup, Error> {
        let mut entries = Vec::with_capacity(SLOT_COUNT * MACROS_PER_SLOT);
        for slot in Slot::ALL {
            for index in MacroIndex::ALL {
                entries.push(self.read_macro(snapshot, slot, index).await?);
            }
        }
        MacroBackup::new(snapshot.clone(), entries)
    }

    /// Inner read used by both the public read and the future write verifier.
    ///
    /// The caller owns the session bookkeeping (`prepare_read` / `run_read`).
    pub(crate) async fn read_macro_with_config(
        &self,
        config: &Bt2Config,
        slot: Slot,
        index: MacroIndex,
    ) -> Result<MacroEntry, Error> {
        let record = config.macro_record(slot, index);
        if is_skipped_steps(record.max_steps) {
            return MacroEntry::new(slot, index, record, Vec::new());
        }

        // The official macro commands take the low 8 bits of the
        // configuration `gamepad_mode` field.
        let mode = config.gamepad_mode_raw() & 0xFF;
        let expected_len = usize::from(record.max_steps) * macro_data::MACRO_STEP_SIZE;
        let (raw, read_offsets) = self
            .read_macro_chunks(mode, slot, index, record.max_steps, expected_len)
            .await?;
        Ok(MacroEntry::new(slot, index, record, raw)?.with_read_offsets(read_offsets))
    }

    /// Read one macro chunk per request, advancing the wire `offset`.
    ///
    /// The device returns 142 bytes per notification (observed) even when
    /// `total_len` is 500 or more, so the loop requests
    /// `offset = (macro_idx << 12) + chunk_offset` again after each chunk.
    /// `total_len` / `chunk_len` stay at `steps * 10`; only the offset moves.
    ///
    /// Returns the assembled bytes plus every raw response `offset` in order.
    async fn read_macro_chunks(
        &self,
        mode: u16,
        slot: Slot,
        index: MacroIndex,
        steps: u16,
        expected_len: usize,
    ) -> Result<(Vec<u8>, Vec<u32>), Error> {
        let mut buffer = TransferBuffer::new(expected_len);
        let mut chunk_offset = 0usize;
        let mut response_offsets = Vec::new();
        let mut offset_mode: Option<OffsetMode> = None;
        let deadline = deadline_after(MACRO_READ_TOTAL_TIMEOUT)?;

        while chunk_offset < expected_len {
            let request = commands::read_macro_from(mode, slot, index, steps, chunk_offset as u32)?;
            self.transport()
                .send(&request.encode_ble_payloadless(PacketKind::App)?)
                .await?;
            let requested = request.header.offset;

            let payload = loop {
                let now = Instant::now();
                if now >= deadline {
                    return Err(macro_read_timeout(
                        chunk_offset,
                        expected_len,
                        &response_offsets,
                    ));
                }
                let wait = self
                    .per_frame_timeout()
                    .min(deadline.saturating_duration_since(now));
                let frame = match tokio::time::timeout(wait, self.transport().recv()).await {
                    Ok(result) => result?,
                    Err(_) => {
                        return Err(macro_read_timeout(
                            chunk_offset,
                            expected_len,
                            &response_offsets,
                        ));
                    }
                };

                let Some((header, payload)) =
                    Self::classify_response(&frame, cmd::READ_MACRO, expected_len)?
                else {
                    continue;
                };

                let reported = header.offset;
                // Fix one offset interpretation for the whole macro read.
                // A first `0` is ambiguous (sequential or relative); the next
                // non-zero chunk decides. Mixing absolute and relative echoes
                // within one macro is rejected so a stale response cannot be
                // spliced into a different macro.
                let mode = match offset_mode {
                    None if reported == 0 => OffsetMode::AmbiguousZero,
                    None if reported == requested => OffsetMode::Absolute,
                    None if u64::from(reported) == chunk_offset as u64 => OffsetMode::Relative,
                    None => {
                        return Err(offset_mode_error(reported, requested, chunk_offset, None));
                    }
                    Some(mode) => mode,
                };
                match mode {
                    OffsetMode::Absolute if reported != requested => {
                        return Err(offset_mode_error(
                            reported,
                            requested,
                            chunk_offset,
                            Some(mode),
                        ));
                    }
                    OffsetMode::Relative if u64::from(reported) != chunk_offset as u64 => {
                        return Err(offset_mode_error(
                            reported,
                            requested,
                            chunk_offset,
                            Some(mode),
                        ));
                    }
                    OffsetMode::AmbiguousZero if reported != 0 => {
                        if reported == requested {
                            offset_mode = Some(OffsetMode::Absolute);
                        } else if u64::from(reported) == chunk_offset as u64 {
                            offset_mode = Some(OffsetMode::Relative);
                        } else {
                            return Err(offset_mode_error(
                                reported,
                                requested,
                                chunk_offset,
                                Some(mode),
                            ));
                        }
                    }
                    _ => {}
                }
                if offset_mode.is_none() {
                    offset_mode = Some(mode);
                }
                response_offsets.push(reported);
                break payload.to_vec();
            };

            let remaining = expected_len - chunk_offset;
            let copy_len = payload.len().min(remaining);
            if copy_len == 0 {
                return Err(Error::UnexpectedResponse(
                    "macro chunk response carried no data".to_string(),
                ));
            }
            buffer.insert(chunk_offset, &payload[..copy_len])?;
            chunk_offset += copy_len;
        }

        Ok((buffer.into_vec(), response_offsets))
    }

    /// Write one macro with the mandatory Erase → Write → Header → Save flow.
    ///
    /// `backup` must contain all 12 entries (see [`MacroBackup::new`]); this is
    /// the "backup first" precondition. Writing into an empty macro
    /// (`max_steps == 0` / `0xFFFF`) is rejected with
    /// [`Error::EmptyMacroWrite`] because step-data clear support is not
    /// implemented.
    /// `SaveTransport::Usb` is rejected with [`Error::Unsupported`].
    /// If Erase, any Write chunk or the 216-byte header update fails,
    /// **Save is not sent** and [`Error::MacroWrite`] is returned with
    /// `saved = false`. After Save, the 216-byte config header and the
    /// `0x0102` raw steps are read back and compared.
    ///
    /// The header values `flag`, `total_cnt`, `key_map`, `cycles_num` and
    /// `interval_ms` are preserved from the backup unless the plan overrides
    /// them. `max_steps` is always set to `steps.len()`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyMacroWrite`] for an empty target,
    /// [`Error::Unsupported`] for `SaveTransport::Usb`, [`Error::MtuTooSmall`]
    /// when the MTU cannot carry a 32-byte chunk or any header payload, a
    /// protocol error for oversized steps or `total_cnt > 4`, or
    /// [`Error::MacroWrite`] for a failed stage, timeout, CRC error,
    /// disconnect or read-back mismatch.
    pub async fn write_macro(
        &self,
        backup: &MacroBackup,
        slot: Slot,
        index: MacroIndex,
        plan: &MacroWritePlan,
    ) -> Result<MacroWriteOutcome, Error> {
        if plan.steps.is_empty() {
            return Err(Error::Protocol(eightbitdo_protocol::Error::EmptyPayload));
        }
        if plan.steps.len() > macro_data::MAX_MACRO_STEPS {
            return Err(Error::Protocol(eightbitdo_protocol::Error::TooManySteps {
                max: macro_data::MAX_MACRO_STEPS,
                actual: plan.steps.len(),
            }));
        }
        if plan
            .total_cnt
            .is_some_and(|count| usize::from(count) > MACROS_PER_SLOT)
        {
            return Err(Error::Protocol(eightbitdo_protocol::Error::InvalidFrame(
                "macro total_cnt must be 0..=4",
            )));
        }
        if backup.get(slot, index).is_none() {
            return Err(Error::UnexpectedResponse(format!(
                "macro backup is missing slot {} macro {}",
                slot.index(),
                index.index()
            )));
        }
        if backup.get(slot, index).is_some_and(MacroEntry::is_empty) {
            return Err(Error::EmptyMacroWrite);
        }
        if plan.save_transport != SaveTransport::Ble {
            return Err(Error::Unsupported(
                "USB macro write is not implemented; only SaveTransport::Ble is supported"
                    .to_string(),
            ));
        }

        let mtu = self.transport().mtu();
        let header_chunk_size = macro_header_chunk_len(mtu)?;
        let write_chunk_size = macro_write_chunk_len(mtu)?;

        self.prepare_read().await?;
        self.run_write(self.write_macro_inner(
            backup,
            slot,
            index,
            plan,
            header_chunk_size,
            write_chunk_size,
        ))
        .await
    }

    async fn write_macro_inner(
        &self,
        backup: &MacroBackup,
        slot: Slot,
        index: MacroIndex,
        plan: &MacroWritePlan,
        header_chunk_size: usize,
        write_chunk_size: usize,
    ) -> Result<MacroWriteOutcome, Error> {
        let base_config = &backup.snapshot().config;
        let mode = base_config.gamepad_mode_raw() & 0xFF;

        // Build the complete header update before sending Erase. Any name or
        // range error therefore happens before the flash sector is touched.
        let (updated, record, header) = apply_macro_write_plan(backup, slot, index, plan)?;

        let header_offset = macro_slot_offset(slot);
        let header_bytes =
            updated.as_bytes()[header_offset..header_offset + MACRO_SLOT_SIZE].to_vec();

        // Build every request before erasing so a local encoding error cannot
        // leave the sector erased without new data.
        let erase = commands::erase_macro(mode, slot, index);
        let write_packets =
            commands::write_macro_steps(mode, slot, index, &plan.steps, write_chunk_size)?;
        let header_request =
            commands::write_config_packets(header_offset as u32, &header_bytes, header_chunk_size)?;
        let save = commands::save(plan.save_transport);

        let mut packets_sent = 0usize;
        self.send_and_await_ack(
            &erase,
            true,
            cmd::ERASE_MACRO,
            commands::macro_subcmd(mode, slot),
            plan.timeouts.erase,
            "EraseBT2Macro",
        )
        .await
        .map_err(|error| macro_write_failure(MacroWriteStage::Erase, error, false))?;
        packets_sent += 1;

        tokio::time::sleep(MACRO_ERASE_SETTLE_DELAY).await;

        for request in &write_packets {
            self.send_and_await_ack(
                request,
                false,
                cmd::WRITE_MACRO,
                commands::macro_subcmd(mode, slot),
                plan.timeouts.write,
                "WriteBT2Macro",
            )
            .await
            .map_err(|error| macro_write_failure(MacroWriteStage::Write, error, false))?;
            packets_sent += 1;
        }

        tokio::time::sleep(MACRO_WRITE_SETTLE_DELAY).await;

        for request in &header_request {
            self.send_and_await_ack(
                request,
                false,
                cmd::WRITE_CONFIG,
                0,
                plan.timeouts.header,
                "WriteMacroHeader",
            )
            .await
            .map_err(|error| macro_write_failure(MacroWriteStage::Header, error, false))?;
            packets_sent += 1;
        }

        self.send_and_await_ack(
            &save,
            false,
            cmd::SAVE,
            plan.save_transport.subcmd(),
            plan.timeouts.save,
            "Save",
        )
        .await
        .map_err(|error| macro_write_failure(MacroWriteStage::Save, error, true))?;
        packets_sent += 1;

        tokio::time::sleep(MACRO_VERIFY_DELAY).await;
        self.verify_config_header(header_offset, &header_bytes)
            .await
            .map_err(|error| macro_write_failure(MacroWriteStage::Verify, error, true))?;

        let readback = self
            .read_macro_with_config(&updated, slot, index)
            .await
            .map_err(|error| macro_write_failure(MacroWriteStage::Verify, error, true))?;
        let expected = macro_data::encode_macro_steps(&plan.steps)?;
        let expected_len = plan.steps.len() * macro_data::MACRO_STEP_SIZE;
        if readback.raw() != &expected[..expected_len] {
            return Err(macro_write_failure(
                MacroWriteStage::Verify,
                Error::UnexpectedResponse(format!(
                    "macro read-back mismatch for slot {} macro {} ({} bytes)",
                    slot.number(),
                    index.index(),
                    readback.raw().len()
                )),
                true,
            ));
        }

        Ok(MacroWriteOutcome {
            record,
            flag: header.flag,
            total_cnt: header.total_cnt,
            packets_sent,
            readback,
        })
    }

    /// Write only one slot's 216-byte macro header and Save.
    ///
    /// Used by `macro restore` for slots whose backup contains no non-empty
    /// macros: step data is not erased, but the header's `max_steps` values
    /// are restored (so previously active macros become unreferenced). No
    /// `0x0104` / `0x0103` is sent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::MtuTooSmall`] before sending, or
    /// [`Error::MacroWrite`] for a failed Header/Save/Verify stage.
    pub async fn write_macro_header(
        &self,
        backup: &MacroBackup,
        slot: Slot,
    ) -> Result<MacroHeaderOutcome, Error> {
        let header = backup.snapshot().config.macro_slot(slot);
        if header
            .records
            .iter()
            .any(|record| !is_skipped_steps(record.max_steps))
        {
            return Err(Error::Unsupported(
                "write_macro_header is only for an all-empty backup slot; use write_macro"
                    .to_string(),
            ));
        }
        if usize::from(header.total_cnt) > MACROS_PER_SLOT {
            return Err(Error::Protocol(eightbitdo_protocol::Error::InvalidFrame(
                "macro total_cnt must be 0..=4",
            )));
        }

        let mtu = self.transport().mtu();
        let header_chunk_size = macro_header_chunk_len(mtu)?;
        self.prepare_read().await?;
        self.run_write(self.write_macro_header_inner(backup, slot, header_chunk_size))
            .await
    }

    async fn write_macro_header_inner(
        &self,
        backup: &MacroBackup,
        slot: Slot,
        header_chunk_size: usize,
    ) -> Result<MacroHeaderOutcome, Error> {
        let mut updated = backup.snapshot().config.clone();
        let header = updated.macro_slot(slot);
        set_macro_slot_preserving_reserved(&mut updated, slot, &header);
        updated.recompute_crc();

        let header_offset = macro_slot_offset(slot);
        let header_bytes =
            updated.as_bytes()[header_offset..header_offset + MACRO_SLOT_SIZE].to_vec();
        let requests =
            commands::write_config_packets(header_offset as u32, &header_bytes, header_chunk_size)?;
        let save = commands::save(SaveTransport::Ble);

        let mut packets_sent = 0usize;
        for request in &requests {
            self.send_and_await_ack(
                request,
                false,
                cmd::WRITE_CONFIG,
                0,
                MACRO_HEADER_TIMEOUT,
                "WriteMacroHeader",
            )
            .await
            .map_err(|error| macro_write_failure(MacroWriteStage::Header, error, false))?;
            packets_sent += 1;
        }
        self.send_and_await_ack(
            &save,
            false,
            cmd::SAVE,
            SaveTransport::Ble.subcmd(),
            MACRO_SAVE_TIMEOUT,
            "Save",
        )
        .await
        .map_err(|error| macro_write_failure(MacroWriteStage::Save, error, true))?;
        packets_sent += 1;

        tokio::time::sleep(MACRO_VERIFY_DELAY).await;
        self.verify_config_header(header_offset, &header_bytes)
            .await
            .map_err(|error| macro_write_failure(MacroWriteStage::Verify, error, true))?;

        Ok(MacroHeaderOutcome {
            flag: header.flag,
            total_cnt: header.total_cnt,
            packets_sent,
        })
    }

    /// Read the full configuration and compare one 216-byte slot header.
    async fn verify_config_header(
        &self,
        header_offset: usize,
        expected: &[u8],
    ) -> Result<(), Error> {
        let config = self
            .read_config_chunks_inner(CONFIG_SIZE, self.per_frame_timeout(), CONFIG_TOTAL_TIMEOUT)
            .await?;
        let actual = &config[header_offset..header_offset + MACRO_SLOT_SIZE];
        if actual != expected {
            let first_diff = actual
                .iter()
                .zip(expected)
                .position(|(actual, expected)| actual != expected)
                .unwrap_or(0);
            let field = header_field_name(first_diff);
            return Err(Error::UnexpectedResponse(format!(
                "macro header read-back mismatch at config offset 0x{header_offset:X} (first \
                 differing byte +0x{first_diff:X} in {field}: device 0x{:02X}, sent 0x{:02X}). \
                 The device may normalize flag/total_cnt/reserved bytes; Save was sent \
                 (saved=true), so do not rewrite blindly.",
                actual.get(first_diff).copied().unwrap_or(0),
                expected.get(first_diff).copied().unwrap_or(0)
            )));
        }
        Ok(())
    }

    /// Send one App Mode request and wait for its ACK.
    ///
    /// Accepts a direct `cmd == ack_cmd` echo and the `cmd 0x0004` envelope
    /// whose `subcmd` is either the request command or `ack_subcmd`. A
    /// payload-less ACK (`chunk_len == 0`) is required; a data response while
    /// waiting for an ACK fails closed. Unrelated notifications (input
    /// reports, other ACKs) are skipped. `0x000E` is surfaced as an error.
    async fn send_and_await_ack(
        &self,
        request: &commands::Request,
        payloadless: bool,
        ack_cmd: u16,
        ack_subcmd: u16,
        timeout: Duration,
        description: &str,
    ) -> Result<(), Error> {
        let frame = if payloadless {
            request.encode_ble_payloadless(PacketKind::App)?
        } else {
            request.encode_ble(PacketKind::App)?
        };
        self.transport().send(&frame).await?;

        let deadline = deadline_after(timeout)?;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(ack_timeout(description, ack_cmd, timeout));
            }
            let wait = deadline.saturating_duration_since(now);
            let frame = match tokio::time::timeout(wait, self.transport().recv()).await {
                Ok(result) => result?,
                Err(_) => return Err(ack_timeout(description, ack_cmd, timeout)),
            };

            // ACK frames may be a bare 16-byte header even when the header
            // echoes a non-zero `chunk_len` (observed Erase ACK:
            // cmd=0x0004, subcmd=0x0104, chunk_len=0x1000, no payload).
            // Parse the header first and do not require a payload for ACKs.
            let primary = CustomHeader::parse(&frame).map_err(|error| {
                Error::UnexpectedResponse(format!(
                    "malformed frame while waiting for {description} ACK: {error}"
                ))
            })?;
            let mut response = primary;
            let mut payload = &frame[HEADER_SIZE..];
            if frame.len() > HEADER_SIZE && frame[0] == 0x04 {
                if let Ok(prefixed) = CustomHeader::parse(&frame[1..]) {
                    let prefixed_is_ack =
                        prefixed.cmd == 0x000E || ack_matches(&prefixed, ack_cmd, ack_subcmd);
                    if prefixed_is_ack {
                        response = prefixed;
                        payload = &frame[HEADER_SIZE + 1..];
                    }
                }
            }

            if response.cmd == 0x000E {
                return Err(Error::UnexpectedResponse(format!(
                    "controller returned error 0x000E while waiting for {description} ACK: {:02X?}",
                    payload
                )));
            }

            if ack_matches(&response, ack_cmd, ack_subcmd) {
                // Accept the ACK regardless of echoed chunk_len / total_len.
                return Ok(());
            }
        }
    }
}

fn apply_macro_write_plan(
    backup: &MacroBackup,
    slot: Slot,
    index: MacroIndex,
    plan: &MacroWritePlan,
) -> Result<(Bt2Config, MacroRecord, MacroSlot), Error> {
    let mut updated = backup.snapshot().config.clone();
    let mut header = updated.macro_slot(slot);
    let mut record = header.records[index.index()];
    record.max_steps = u16::try_from(plan.steps.len()).map_err(|_| {
        Error::Protocol(eightbitdo_protocol::Error::TooManySteps {
            max: macro_data::MAX_MACRO_STEPS,
            actual: plan.steps.len(),
        })
    })?;
    if let Some(name) = &plan.name {
        record.set_name(name)?;
    }
    if let Some(key_map) = plan.key_map {
        record.key_map = key_map;
    }
    if let Some(cycles_num) = plan.cycles_num {
        record.cycles_num = cycles_num;
    }
    if let Some(interval_ms) = plan.interval_ms {
        record.interval_ms = interval_ms;
    }
    header.records[index.index()] = record;
    if let Some(flag) = plan.flag {
        header.flag = flag;
    }
    if let Some(total_cnt) = plan.total_cnt {
        header.total_cnt = total_cnt;
    }
    if usize::from(header.total_cnt) > MACROS_PER_SLOT {
        return Err(Error::Protocol(eightbitdo_protocol::Error::InvalidFrame(
            "macro total_cnt must be 0..=4",
        )));
    }
    set_macro_slot_preserving_reserved(&mut updated, slot, &header);
    updated.recompute_crc();
    Ok((updated, record, header))
}

/// Write a slot header while keeping the reserved bytes from the original.
///
/// `Bt2Config::set_macro_slot` zeroes the 3-byte slot padding and the 2-byte
/// per-record padding. The controller may store non-zero values there, and the
/// write path must not rewrite unknown bytes.
fn set_macro_slot_preserving_reserved(config: &mut Bt2Config, slot: Slot, header: &MacroSlot) {
    let offset = macro_slot_offset(slot);
    let original = config.as_bytes()[offset..offset + MACRO_SLOT_SIZE].to_vec();
    config.set_macro_slot(slot, header);
    let bytes = &mut config.as_mut_bytes()[offset..offset + MACRO_SLOT_SIZE];
    bytes[5..8].copy_from_slice(&original[5..8]);
    for index in 0..MACROS_PER_SLOT {
        let record = 8 + index * MACRO_RECORD_SIZE;
        bytes[record + 0x26..record + 0x28]
            .copy_from_slice(&original[record + 0x26..record + 0x28]);
    }
}

/// Recovery guidance for a write that failed before Save.
const MACRO_NOT_SAVED_RECOVERY: &str = "Save was not sent, but the sector may already be erased or partially written; do not retry blindly, re-read the macro and restore from the 3-slot x 4-macro backup";
/// Recovery guidance for a write that failed after Save was sent.
const MACRO_SAVED_RECOVERY: &str = "Save was sent but the write could not be confirmed; re-read the macro and restore from the 3-slot x 4-macro backup if it differs";

fn macro_write_failure(stage: MacroWriteStage, error: Error, saved: bool) -> Error {
    Error::MacroWrite {
        stage,
        reason: error.to_string(),
        saved,
        recovery: if saved {
            MACRO_SAVED_RECOVERY
        } else {
            MACRO_NOT_SAVED_RECOVERY
        },
    }
}

fn ack_timeout(description: &str, ack_cmd: u16, timeout: Duration) -> Error {
    Error::Timeout {
        operation: format!("{description} ACK (cmd=0x{ack_cmd:04X}) within {timeout:?}"),
    }
}

fn ack_matches(response: &CustomHeader, ack_cmd: u16, ack_subcmd: u16) -> bool {
    response.cmd == ack_cmd
        || (response.cmd == cmd::RESPONSE
            && (response.subcmd == ack_cmd || response.subcmd == ack_subcmd))
}

fn header_field_name(offset: usize) -> &'static str {
    if offset < 4 {
        "slot flag"
    } else if offset == 4 {
        "total_cnt"
    } else if (5..8).contains(&offset) {
        "slot reserved"
    } else {
        for index in 0..MACROS_PER_SLOT {
            let record = 8 + index * MACRO_RECORD_SIZE;
            if offset == record + 0x26 || offset == record + 0x27 {
                return "record reserved";
            }
            if (record..record + MACRO_RECORD_SIZE).contains(&offset) {
                return "record field";
            }
        }
        "unknown"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OffsetMode {
    /// Response `offset` echoes the requested absolute offset.
    Absolute,
    /// Response `offset` is relative to the macro's sector base.
    Relative,
    /// Every response so far reported `0`; the next non-zero chunk decides.
    AmbiguousZero,
}

fn offset_mode_error(
    reported: u32,
    requested: u32,
    chunk_offset: usize,
    mode: Option<OffsetMode>,
) -> Error {
    let detail = match mode {
        None => {
            "the first response offset must be 0, the requested absolute offset, or the \
                 sector-relative chunk offset"
        }
        Some(OffsetMode::AmbiguousZero) => {
            "a leading 0 is neither absolute nor relative, so the next non-zero offset must be \
             the requested absolute offset or the sector-relative chunk offset"
        }
        Some(_) => "absolute and relative echoes must not be mixed within one macro",
    };
    Error::UnexpectedResponse(format!(
        "macro chunk response offset 0x{reported:X} does not match requested 0x{requested:X} \
         (chunk offset 0x{chunk_offset:X}, mode {mode:?}); {detail}"
    ))
}

fn macro_read_timeout(chunk_offset: usize, expected: usize, response_offsets: &[u32]) -> Error {
    Error::Timeout {
        operation: format!(
            "ReadBT2Macro cmd=0x0102 at chunk offset 0x{chunk_offset:X} \
             (received {chunk_offset} of {expected} bytes; response offsets: {response_offsets:?})"
        ),
    }
}

#[cfg(test)]
mod tests {
    use eightbitdo_protocol::config_bt2::GamepadMode;
    use eightbitdo_protocol::crc::packet_crc16;
    use eightbitdo_protocol::header::CustomHeader;
    use eightbitdo_protocol::KeyMap;

    use super::*;
    use crate::mock::MockTransport;
    use crate::transport::Transport;

    /// Replays one response, then never yields another frame.
    struct StallAfterFirst {
        first: std::sync::Mutex<Option<Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl Transport for StallAfterFirst {
        async fn send(&self, _data: &[u8]) -> Result<(), Error> {
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            if let Some(frame) = self.first.lock().unwrap().take() {
                return Ok(frame);
            }
            std::future::pending().await
        }

        fn mtu(&self) -> u16 {
            64
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    fn sample_entry_payload(steps: &[MacroStep]) -> Vec<u8> {
        let encoded = macro_data::encode_macro_steps(steps).unwrap();
        let len = steps.len() * macro_data::MACRO_STEP_SIZE;
        encoded[..len].to_vec()
    }

    fn sample_steps(count: usize) -> Vec<MacroStep> {
        (0..count)
            .map(|i| MacroStep {
                ms_times: (i as u16 + 1) * 10,
                keys: if i % 2 == 0 {
                    KeyMap::A.bits() as u16
                } else {
                    0
                },
                rt_value: i as u8,
                lt_value: 0,
                left_x: 0x7F,
                left_y: 0x7F,
                right_x: 0x7F,
                right_y: 0x7F,
            })
            .collect()
    }

    fn config_with_macros() -> Bt2Config {
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);

        let mut record = MacroRecord {
            max_steps: 4,
            key_map: KeyMap::M1,
            cycles_num: 2,
            interval_ms: 10,
            ..MacroRecord::default()
        };
        record.set_name("Combo").unwrap();
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);

        let mut unset = MacroRecord {
            max_steps: u16::MAX,
            ..MacroRecord::default()
        };
        unset.set_name("Unset").unwrap();
        config.set_macro_record(Slot::S1, MacroIndex::M2, &unset);

        config.recompute_crc();
        config
    }

    fn snapshot(config: Bt2Config) -> ConfigSnapshot {
        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        ConfigSnapshot {
            config,
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        }
    }

    fn envelope(request_cmd: u16, total_len: u32, offset: u32, payload: &[u8]) -> CustomHeader {
        CustomHeader::new(cmd::RESPONSE, request_cmd, total_len, offset)
            .with_chunk_len(payload.len() as u16)
            .with_payload(payload)
    }

    #[tokio::test]
    async fn zero_and_unset_steps_are_skipped_without_a_request() {
        let config = config_with_macros();
        let snapshot = snapshot(config);
        let mock = MockTransport::new();
        let session = Session::new(&mock);

        let zero = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M3)
            .await
            .unwrap();
        assert!(zero.is_empty());
        assert!(zero.steps().is_empty());

        let unset = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M2)
            .await
            .unwrap();
        assert!(unset.is_empty());
        assert_eq!(unset.record().max_steps, u16::MAX);
        assert!(mock.sent_frames().is_empty());
    }

    #[tokio::test]
    async fn read_macro_reads_500_bytes_as_four_142_byte_chunks() {
        let mut config = config_with_macros();
        let mut record = config.macro_record(Slot::S1, MacroIndex::M1);
        record.max_steps = 50;
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(50);
        let raw = sample_entry_payload(&steps);
        assert_eq!(raw.len(), 500);

        let mock = MockTransport::new();
        let mut chunk_offset = 0usize;
        for chunk in raw.chunks(142) {
            let header = envelope(cmd::READ_MACRO, 500, chunk_offset as u32, chunk);
            let mut header = header;
            header.crc16 = packet_crc16(chunk);
            mock.push_response(&header, chunk);
            chunk_offset += chunk.len();
        }
        assert_eq!(chunk_offset, 500);

        let session = Session::new(&mock);
        let entry = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M1)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 4);
        assert_eq!(
            sent[0],
            vec![
                0x04, // BLE App prefix
                0x02, 0x01, // cmd = 0x0102
                0x00, 0x00, // subcmd = (Switch 0 << 8) | slot 0
                0xF4, 0x01, // chunk_len = 500
                0x00, 0x00, // crc16
                0xF4, 0x01, 0x00, 0x00, // total_len = 500
                0x00, 0x00, 0x00, 0x00, // offset = macro 0 sector + 0
            ]
        );
        let expected_offsets = [0u32, 142, 284, 426];
        for (frame, expected_offset) in sent.iter().zip(expected_offsets) {
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.cmd, 0x0102);
            assert_eq!(header.subcmd, 0);
            assert_eq!(header.chunk_len, 500);
            assert_eq!(header.total_len, 500);
            assert_eq!(header.offset, expected_offset);
        }

        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.steps(), steps);
        assert_eq!(entry.read_offsets(), &expected_offsets[..]);
        assert_eq!(entry.record().name(), "Combo");
        assert_eq!(entry.record().key_map, KeyMap::M1);
    }

    #[tokio::test]
    async fn read_macro_reads_460_bytes_from_sector_4096_with_echoed_offsets() {
        let mut config = config_with_macros();
        config.set_gamepad_mode(GamepadMode::XInput);
        let mut record = config.macro_record(Slot::S2, MacroIndex::M2);
        record.max_steps = 46;
        config.set_macro_record(Slot::S2, MacroIndex::M2, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(46);
        let raw = sample_entry_payload(&steps);
        assert_eq!(raw.len(), 460);

        let mock = MockTransport::new();
        let expected_offsets = [4096u32, 4238, 4380, 4522];
        let mut chunk_offset = 0usize;
        for (chunk, expected_offset) in raw.chunks(142).zip(expected_offsets) {
            let mut header = envelope(cmd::READ_MACRO, 460, expected_offset, chunk);
            header.crc16 = packet_crc16(chunk);
            mock.push_response(&header, chunk);
            chunk_offset += chunk.len();
        }
        assert_eq!(chunk_offset, 460);

        let session = Session::new(&mock);
        let entry = session
            .read_macro(&snapshot, Slot::S2, MacroIndex::M2)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 4);
        let request_offsets = [4096u32, 4238, 4380, 4522];
        for (frame, expected_offset) in sent.iter().zip(request_offsets) {
            let request = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(request.cmd, 0x0102);
            assert_eq!(request.subcmd, 0x0301);
            assert_eq!(request.chunk_len, 460);
            assert_eq!(request.total_len, 460);
            assert_eq!(request.offset, expected_offset);
        }

        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.steps(), steps);
        assert_eq!(entry.read_offsets(), &request_offsets);
    }

    #[tokio::test]
    async fn read_macro_accepts_zero_offsets_as_a_sequential_fallback() {
        let mut config = config_with_macros();
        let mut record = config.macro_record(Slot::S1, MacroIndex::M1);
        record.max_steps = 20;
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(20);
        let raw = sample_entry_payload(&steps);
        assert_eq!(raw.len(), 200);

        let mock = MockTransport::new();
        for chunk in raw.chunks(142) {
            let mut header = envelope(cmd::READ_MACRO, 200, 0, chunk);
            header.crc16 = packet_crc16(chunk);
            mock.push_response(&header, chunk);
        }

        let session = Session::new(&mock);
        let entry = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M1)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 2);
        assert_eq!(CustomHeader::parse(&sent[1][1..]).unwrap().offset, 142);
        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.read_offsets(), &[0, 0]);
    }

    #[tokio::test]
    async fn read_macro_accepts_sector_relative_offsets_for_macro_2() {
        let mut config = config_with_macros();
        let mut record = config.macro_record(Slot::S1, MacroIndex::M3);
        record.max_steps = 30;
        config.set_macro_record(Slot::S1, MacroIndex::M3, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(30);
        let raw = sample_entry_payload(&steps);
        assert_eq!(raw.len(), 300);

        let mock = MockTransport::new();
        let expected_offsets = [0u32, 142, 284];
        for (chunk, offset) in raw.chunks(142).zip(expected_offsets) {
            let mut header = envelope(cmd::READ_MACRO, 300, offset, chunk);
            header.crc16 = packet_crc16(chunk);
            mock.push_response(&header, chunk);
        }

        let session = Session::new(&mock);
        let entry = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M3)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        let request_offsets = [0x2000u32, 0x208E, 0x211C];
        for (frame, offset) in sent.iter().zip(request_offsets) {
            let request = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(request.offset, offset);
            assert_eq!(request.total_len, 300);
        }
        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.read_offsets(), &expected_offsets);
    }

    #[tokio::test]
    async fn read_macro_resolves_ambiguous_zero_to_absolute_offsets() {
        let mut config = config_with_macros();
        let mut record = config.macro_record(Slot::S1, MacroIndex::M2);
        record.max_steps = 30;
        config.set_macro_record(Slot::S1, MacroIndex::M2, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(30);
        let raw = sample_entry_payload(&steps);
        let mock = MockTransport::new();
        let response_offsets = [0u32, 0x108E, 0x111C];
        for (chunk, offset) in raw.chunks(142).zip(response_offsets) {
            let mut header = envelope(cmd::READ_MACRO, 300, offset, chunk);
            header.crc16 = packet_crc16(chunk);
            mock.push_response(&header, chunk);
        }

        let session = Session::new(&mock);
        let entry = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M2)
            .await
            .unwrap();

        let request_offsets = [0x1000u32, 0x108E, 0x111C];
        let sent = mock.sent_frames();
        for (frame, offset) in sent.iter().zip(request_offsets) {
            assert_eq!(CustomHeader::parse(&frame[1..]).unwrap().offset, offset);
        }
        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.read_offsets(), &response_offsets);
    }

    #[tokio::test]
    async fn read_macro_rejects_unresolved_leading_zero() {
        let mut config = config_with_macros();
        let mut record = config.macro_record(Slot::S1, MacroIndex::M3);
        record.max_steps = 30;
        config.set_macro_record(Slot::S1, MacroIndex::M3, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(30);
        let raw = sample_entry_payload(&steps);
        let mock = MockTransport::new();
        let mut first = envelope(cmd::READ_MACRO, 300, 0, &raw[..142]);
        first.crc16 = packet_crc16(&raw[..142]);
        mock.push_response(&first, &raw[..142]);
        let mut second = envelope(cmd::READ_MACRO, 300, 999, &raw[142..284]);
        second.crc16 = packet_crc16(&raw[142..284]);
        mock.push_response(&second, &raw[142..284]);

        let session = Session::new(&mock);
        let result = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M3)
            .await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("leading 0")
        ));
    }

    #[tokio::test]
    async fn read_macro_rejects_mixed_absolute_and_relative_offsets() {
        let mut config = config_with_macros();
        let mut record = config.macro_record(Slot::S1, MacroIndex::M3);
        record.max_steps = 30;
        config.set_macro_record(Slot::S1, MacroIndex::M3, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(30);
        let raw = sample_entry_payload(&steps);
        let mock = MockTransport::new();
        // First chunk is an absolute echo (0x2000), then a stale relative
        // response (142) that would belong to macro 0.
        let mut first = envelope(cmd::READ_MACRO, 300, 0x2000, &raw[..142]);
        first.crc16 = packet_crc16(&raw[..142]);
        mock.push_response(&first, &raw[..142]);
        let mut second = envelope(cmd::READ_MACRO, 300, 142, &raw[142..284]);
        second.crc16 = packet_crc16(&raw[142..284]);
        mock.push_response(&second, &raw[142..284]);

        let session = Session::new(&mock);
        let result = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M3)
            .await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("must not be mixed")
        ));
    }

    #[tokio::test]
    async fn read_macro_response_crc_is_validated() {
        let config = config_with_macros();
        let snapshot = snapshot(config);
        let steps = sample_steps(4);
        let raw = sample_entry_payload(&steps);

        let mock = MockTransport::new();
        let mut header = envelope(cmd::READ_MACRO, 40, 0, &raw);
        header.crc16 = packet_crc16(&raw).wrapping_add(1);
        mock.push_response(&header, &raw);

        let session = Session::new(&mock);
        let result = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M1)
            .await;
        assert!(matches!(result, Err(Error::UnexpectedResponse(_))));
    }

    #[tokio::test]
    async fn read_macro_times_out_mid_transfer_without_returning_partial_data() {
        let mut config = config_with_macros();
        let mut record = config.macro_record(Slot::S1, MacroIndex::M1);
        record.max_steps = 50;
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(50);
        let raw = sample_entry_payload(&steps);
        let first = &raw[..142];
        let mut header = envelope(cmd::READ_MACRO, 500, 0, first);
        header.crc16 = packet_crc16(first);
        let mut frame = header.to_bytes().to_vec();
        frame.extend_from_slice(first);

        let transport = StallAfterFirst {
            first: std::sync::Mutex::new(Some(frame)),
        };
        let session = Session::new(&transport).with_timeout(Duration::from_millis(10));
        let result = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M1)
            .await;
        assert!(matches!(
            result,
            Err(Error::Timeout { operation })
                if operation.contains("chunk offset 0x8E")
                    && operation.contains("response offsets: [0]")
        ));
    }
}

#[cfg(test)]
mod write_tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use eightbitdo_protocol::config_bt2::GamepadMode;
    use eightbitdo_protocol::config_bt2::MACRO_RECORD_SIZE;
    use eightbitdo_protocol::crc::packet_crc16;
    use eightbitdo_protocol::header::CustomHeader;

    use super::*;
    use crate::mock::MockTransport;

    fn sample_steps(count: usize) -> Vec<MacroStep> {
        (0..count)
            .map(|i| MacroStep {
                ms_times: (i as u16 + 1) * 10,
                keys: if i % 2 == 0 {
                    KeyMap::A.bits() as u16
                } else {
                    0
                },
                rt_value: i as u8,
                lt_value: 0,
                left_x: 0x7F,
                left_y: 0x7F,
                right_x: 0x7F,
                right_y: 0x7F,
            })
            .collect()
    }

    fn raw_steps(steps: &[MacroStep]) -> Vec<u8> {
        macro_data::encode_macro_steps(steps).unwrap()[..steps.len() * 10].to_vec()
    }

    fn config_with_existing_macro() -> Bt2Config {
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        let mut record = MacroRecord {
            max_steps: 4,
            key_map: KeyMap::M1,
            cycles_num: 2,
            interval_ms: 10,
            ..MacroRecord::default()
        };
        record.set_name("Old").unwrap();
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);
        config.recompute_crc();
        config
    }

    fn snapshot(config: Bt2Config) -> ConfigSnapshot {
        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        ConfigSnapshot {
            config,
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        }
    }

    fn backup_from(config: Bt2Config) -> MacroBackup {
        let mut entries = Vec::new();
        for slot in Slot::ALL {
            for index in MacroIndex::ALL {
                let record = config.macro_record(slot, index);
                let raw = if is_skipped_steps(record.max_steps) {
                    Vec::new()
                } else {
                    raw_steps(&sample_steps(usize::from(record.max_steps)))
                };
                entries.push(MacroEntry::new(slot, index, record, raw).unwrap());
            }
        }
        MacroBackup::new(snapshot(config), entries).unwrap()
    }

    fn push_ack(mock: &MockTransport, request_cmd: u16, direct: bool) {
        let header = if direct {
            CustomHeader::new(request_cmd, 0, 0, 0)
        } else {
            CustomHeader::new(cmd::RESPONSE, request_cmd, 0, 0)
        };
        mock.push_response(&header, &[]);
    }

    fn push_readback(mock: &MockTransport, steps: &[MacroStep]) {
        let raw = raw_steps(steps);
        let mut offset = 0u32;
        for chunk in raw.chunks(142) {
            let mut header =
                CustomHeader::new(cmd::RESPONSE, cmd::READ_MACRO, raw.len() as u32, offset)
                    .with_chunk_len(chunk.len() as u16);
            header.crc16 = packet_crc16(chunk);
            mock.push_response(&header, chunk);
            offset += chunk.len() as u32;
        }
    }

    fn config_frames(config: &Bt2Config) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        for (index, chunk) in config.as_bytes().chunks(142).enumerate() {
            let header = CustomHeader::new(
                cmd::READ_CONFIG,
                0,
                CONFIG_SIZE as u32,
                (index * 142) as u32,
            )
            .with_chunk_len(chunk.len() as u16);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(chunk);
            frames.push(frame);
        }
        frames
    }

    fn push_config(mock: &MockTransport, config: &Bt2Config) {
        for frame in config_frames(config) {
            mock.push_incoming(&frame);
        }
    }

    fn expected_config(
        backup: &MacroBackup,
        slot: Slot,
        index: MacroIndex,
        plan: &MacroWritePlan,
    ) -> Bt2Config {
        apply_macro_write_plan(backup, slot, index, plan).unwrap().0
    }

    fn frame_cmd(frame: &[u8]) -> u16 {
        CustomHeader::parse(&frame[1..]).unwrap().cmd
    }

    #[tokio::test]
    async fn write_macro_accepts_payloadless_acks_with_declared_chunk_len() {
        let backup = backup_from(config_with_existing_macro());
        let steps = sample_steps(4);
        let plan = MacroWritePlan::new(steps.clone());

        let mock = MockTransport::with_mtu(200);
        // Observed Erase ACK: cmd=0x0004, subcmd=0x0104, chunk_len=0x1000,
        // total_len=0x1000, no payload.
        let mut erase_ack = CustomHeader::new(cmd::RESPONSE, cmd::ERASE_MACRO, 0x1000, 0);
        erase_ack.chunk_len = 0x1000;
        erase_ack.crc16 = 0x0101;
        mock.push_response(&erase_ack, &[]);
        // Write ACK declares 64 bytes but has no payload; send it with a
        // BLE 0x04 prefix to cover the 17-byte form too.
        let write_ack =
            CustomHeader::new(cmd::RESPONSE, cmd::WRITE_MACRO, 64, 0).with_chunk_len(64);
        let mut write_frame = write_ack.to_bytes().to_vec();
        write_frame.insert(0, 0x04);
        mock.push_incoming(&write_frame);
        // Header ACKs declare 142 bytes but have no payload.
        let header_ack =
            CustomHeader::new(cmd::RESPONSE, cmd::WRITE_CONFIG, 0x0AD0, 0x68C).with_chunk_len(142);
        mock.push_response(&header_ack, &[]);
        mock.push_response(&header_ack, &[]);
        mock.push_response(&CustomHeader::new(cmd::RESPONSE, cmd::SAVE, 0, 0), &[]);
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        push_config(&mock, &expected);
        push_readback(&mock, &steps);

        let session = Session::new(&mock);
        let outcome = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await
            .expect("payloadless ACKs with echoed chunk_len must be accepted");
        assert_eq!(outcome.readback.steps(), steps);
    }

    #[tokio::test]
    async fn write_macro_sends_erase_write_chunks_header_save_and_verifies() {
        let backup = backup_from(config_with_existing_macro());
        let steps = sample_steps(20);
        let mut plan = MacroWritePlan::new(steps.clone()).with_name("Edited");
        plan.key_map = Some(KeyMap::M3);
        plan.cycles_num = Some(5);
        plan.interval_ms = Some(30);

        let mock = MockTransport::with_mtu(200);
        push_ack(&mock, cmd::ERASE_MACRO, true);
        for _ in 0..4 {
            push_ack(&mock, cmd::WRITE_MACRO, false);
        }
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        // Real BLE saves may echo the platform sub-command (0x0067) instead
        // of 0x0006 in an envelope.
        let save_ack = CustomHeader::new(cmd::RESPONSE, SaveTransport::Ble.subcmd(), 0, 0);
        mock.push_response(&save_ack, &[]);
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        push_config(&mock, &expected);
        push_readback(&mock, &steps);

        let session = Session::new(&mock);
        let outcome = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        let commands: Vec<u16> = sent
            .iter()
            .map(|frame| frame_cmd(frame))
            .filter(|command| *command != cmd::READ_CONFIG)
            .collect();
        assert_eq!(
            commands,
            vec![
                cmd::ERASE_MACRO,
                cmd::WRITE_MACRO,
                cmd::WRITE_MACRO,
                cmd::WRITE_MACRO,
                cmd::WRITE_MACRO,
                cmd::WRITE_CONFIG,
                cmd::WRITE_CONFIG,
                cmd::SAVE,
                cmd::READ_MACRO,
                cmd::READ_MACRO,
            ]
        );

        // Erase: payload-less, 4 KB declared length, crc16 = 0xFFFF.
        assert_eq!(
            sent[0],
            vec![
                0x04, 0x04, 0x01, 0x00, 0x00, 0x00, 0x10, 0xFF, 0xFF, 0x00, 0x10, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00,
            ]
        );

        // 200 steps = 200 bytes, 32-byte aligned to 224: 64/64/64/32.
        let aligned = macro_data::encode_macro_steps(&steps).unwrap();
        assert_eq!(aligned.len(), 224);
        for (i, chunk) in aligned.chunks(64).enumerate() {
            let request = &sent[1 + i];
            let header = CustomHeader::parse(&request[1..]).unwrap();
            assert_eq!(header.cmd, cmd::WRITE_MACRO);
            assert_eq!(header.subcmd, 0);
            assert_eq!(header.total_len, 224);
            assert_eq!(header.offset, (i * 64) as u32);
            assert_eq!(header.chunk_len as usize, chunk.len());
            assert_eq!(header.crc16, packet_crc16(chunk));
            assert_eq!(&request[17..], chunk);
        }

        // Header: 216 bytes split into 142 + 74 at 0x68C / 0x71A.
        // `total_len` stays 0x0AD0 on both packets.
        let first_header = CustomHeader::parse(&sent[5][1..]).unwrap();
        assert_eq!(first_header.cmd, cmd::WRITE_CONFIG);
        assert_eq!(first_header.subcmd, 0);
        assert_eq!(first_header.total_len, 0x0AD0);
        assert_eq!(first_header.offset, 0x68C);
        assert_eq!(first_header.chunk_len, 142);
        assert_eq!(first_header.crc16, packet_crc16(&sent[5][17..]));
        assert_eq!(sent[5].len(), 17 + 142);
        let second_header = CustomHeader::parse(&sent[6][1..]).unwrap();
        assert_eq!(second_header.cmd, cmd::WRITE_CONFIG);
        assert_eq!(second_header.subcmd, 0);
        assert_eq!(second_header.total_len, 0x0AD0);
        assert_eq!(second_header.offset, 0x68C + 142);
        assert_eq!(second_header.chunk_len, 74);
        assert_eq!(second_header.crc16, packet_crc16(&sent[6][17..]));
        assert_eq!(sent[6].len(), 17 + 74);
        let header_payload: Vec<u8> = sent[5][17..]
            .iter()
            .chain(&sent[6][17..])
            .copied()
            .collect();
        assert_eq!(header_payload.len(), MACRO_SLOT_SIZE);
        let record = &header_payload[8..8 + MACRO_RECORD_SIZE];
        assert_eq!(&record[0..4], &[0x00, 0x45, 0x00, 0x64]); // "Ed" UTF-16BE
        assert_eq!(u16::from_le_bytes([record[0x22], record[0x23]]), 20);
        assert_eq!(
            u32::from_le_bytes([record[0x28], record[0x29], record[0x2A], record[0x2B]]),
            KeyMap::M3.bits()
        );
        assert_eq!(
            u32::from_le_bytes([record[0x2C], record[0x2D], record[0x2E], record[0x2F]]),
            5
        );
        assert_eq!(
            u32::from_le_bytes([record[0x30], record[0x31], record[0x32], record[0x33]]),
            30
        );
        assert_eq!(record[0x20], 0);

        // Save: BLE subcmd 0x0067, no payload.
        assert_eq!(
            sent[7],
            vec![
                0x04, 0x06, 0x00, 0x67, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00,
            ]
        );

        assert_eq!(outcome.packets_sent, 8);
        assert_eq!(outcome.record.max_steps, 20);
        assert_eq!(outcome.record.key_map, KeyMap::M3);
        assert_eq!(outcome.record.name(), "Edited");
        assert_eq!(outcome.record.cycles_num, 5);
        assert_eq!(outcome.record.interval_ms, 30);
        assert_eq!(outcome.readback.raw(), raw_steps(&steps));
        assert_eq!(outcome.readback.read_offsets(), &[0, 142]);
        assert_eq!(outcome.flag, 0);
        assert_eq!(outcome.total_cnt, 0);
    }

    #[tokio::test]
    async fn mtu_64_uses_44_byte_header_chunks_and_32_byte_writes() {
        let backup = backup_from(config_with_existing_macro());
        let steps = sample_steps(10);
        let plan = MacroWritePlan::new(steps.clone());

        let mock = MockTransport::with_mtu(64);
        push_ack(&mock, cmd::ERASE_MACRO, false);
        for _ in 0..4 {
            push_ack(&mock, cmd::WRITE_MACRO, false);
        }
        // 216 bytes at 44 bytes per header chunk: 44*4 + 40.
        for _ in 0..5 {
            push_ack(&mock, cmd::WRITE_CONFIG, false);
        }
        push_ack(&mock, cmd::SAVE, false);
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        push_config(&mock, &expected);
        push_readback(&mock, &steps);

        let session = Session::new(&mock);
        session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert!(
            sent.iter().all(|frame| frame.len() <= 61),
            "every frame must fit mtu - 3 = 61 bytes"
        );
        // 100 bytes -> 128 aligned -> 4 x 32.
        for (i, request) in sent[1..5].iter().enumerate() {
            let header = CustomHeader::parse(&request[1..]).unwrap();
            assert_eq!(header.total_len, 128);
            assert_eq!(header.chunk_len, 32);
            assert_eq!(header.offset, (i * 32) as u32);
        }
        let header_frames: Vec<_> = sent
            .iter()
            .filter(|frame| frame_cmd(frame) == cmd::WRITE_CONFIG)
            .collect();
        assert_eq!(header_frames.len(), 5);
        let expected_lengths = [44u16, 44, 44, 44, 40];
        let expected_offsets = [0x68Cu32, 0x6B8, 0x6E4, 0x710, 0x73C];
        for ((frame, length), offset) in header_frames
            .iter()
            .zip(expected_lengths)
            .zip(expected_offsets)
        {
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.total_len, 0x0AD0);
            assert_eq!(header.chunk_len, length);
            assert_eq!(header.offset, offset);
            assert_eq!(frame.len(), 17 + usize::from(length));
        }
        let combined: Vec<u8> = header_frames
            .iter()
            .flat_map(|frame| frame[17..].iter().copied())
            .collect();
        assert_eq!(combined.len(), MACRO_SLOT_SIZE);
        let record = &combined[8..8 + MACRO_RECORD_SIZE];
        assert_eq!(&record[0..6], &[0x00, 0x4F, 0x00, 0x6C, 0x00, 0x64]); // "Old"
        assert_eq!(u16::from_le_bytes([record[0x22], record[0x23]]), 10);
        assert_eq!(
            u32::from_le_bytes([record[0x28], record[0x29], record[0x2A], record[0x2B]]),
            KeyMap::M1.bits()
        );
        assert_eq!(
            u32::from_le_bytes([record[0x2C], record[0x2D], record[0x2E], record[0x2F]]),
            2
        );
        assert_eq!(
            u32::from_le_bytes([record[0x30], record[0x31], record[0x32], record[0x33]]),
            10
        );
        assert_eq!(u32::from_le_bytes(combined[0..4].try_into().unwrap()), 0);
        assert_eq!(combined[4], 0);
    }

    #[tokio::test]
    async fn write_macro_applies_flag_and_total_cnt_overrides() {
        let backup = backup_from(config_with_existing_macro());
        let steps = sample_steps(20);
        let plan = MacroWritePlan::new(steps.clone())
            .with_key_map(KeyMap::P1)
            .with_header_state(1, 2);

        let mock = MockTransport::with_mtu(200);
        push_ack(&mock, cmd::ERASE_MACRO, true);
        for _ in 0..4 {
            push_ack(&mock, cmd::WRITE_MACRO, true);
        }
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::SAVE, true);
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        push_config(&mock, &expected);
        push_readback(&mock, &steps);

        let session = Session::new(&mock);
        let outcome = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await
            .unwrap();

        let header = CustomHeader::parse(&mock.sent_frames()[5][1..]).unwrap();
        let payload = &mock.sent_frames()[5][17..];
        assert_eq!(header.offset, 0x68C);
        assert_eq!(u32::from_le_bytes(payload[0..4].try_into().unwrap()), 1);
        assert_eq!(payload[4], 2);
        assert_eq!(outcome.flag, 1);
        assert_eq!(outcome.total_cnt, 2);
        assert_eq!(outcome.record.key_map, KeyMap::P1);
    }

    /// Transport with a preloaded response queue that stalls (instead of
    /// disconnecting) when the queue is empty, so ACK timeouts are testable.
    struct ScriptedTransport {
        incoming: Mutex<VecDeque<Vec<u8>>>,
        sent: Mutex<Vec<Vec<u8>>>,
        stall_when_empty: bool,
        mtu: u16,
    }

    impl ScriptedTransport {
        fn new(frames: Vec<Vec<u8>>, stall_when_empty: bool, mtu: u16) -> Self {
            Self {
                incoming: Mutex::new(frames.into()),
                sent: Mutex::new(Vec::new()),
                stall_when_empty,
                mtu,
            }
        }

        fn response(header: CustomHeader, payload: &[u8]) -> Vec<u8> {
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(payload);
            frame
        }

        fn sent(&self) -> Vec<Vec<u8>> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Transport for ScriptedTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            self.sent.lock().unwrap().push(data.to_vec());
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            let next = self.incoming.lock().unwrap().pop_front();
            match next {
                Some(frame) => Ok(frame),
                None if self.stall_when_empty => std::future::pending().await,
                None => Err(Error::Disconnected),
            }
        }

        fn mtu(&self) -> u16 {
            self.mtu
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    fn short_timeouts() -> MacroTimeouts {
        MacroTimeouts {
            erase: Duration::from_millis(10),
            write: Duration::from_millis(10),
            header: Duration::from_millis(10),
            save: Duration::from_millis(10),
        }
    }

    fn ack_frame(request_cmd: u16, direct: bool) -> Vec<u8> {
        let header = if direct {
            CustomHeader::new(request_cmd, 0, 0, 0)
        } else {
            CustomHeader::new(cmd::RESPONSE, request_cmd, 0, 0)
        };
        ScriptedTransport::response(header, &[])
    }

    async fn write_with_script(
        script: Vec<Vec<u8>>,
        stall_when_empty: bool,
        steps: &[MacroStep],
    ) -> (Result<MacroWriteOutcome, Error>, Vec<Vec<u8>>) {
        let backup = backup_from(config_with_existing_macro());
        let plan = MacroWritePlan::new(steps.to_vec()).with_timeouts(short_timeouts());
        let transport = ScriptedTransport::new(script, stall_when_empty, 200);
        let session = Session::new(&transport);
        let result = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await;
        let sent = transport.sent();
        (result, sent)
    }

    fn assert_no_save(sent: &[Vec<u8>]) {
        assert!(
            sent.iter().all(|frame| frame_cmd(frame) != cmd::SAVE),
            "Save must not be sent after a pre-Save failure"
        );
    }

    #[tokio::test]
    async fn erase_timeout_does_not_send_save() {
        let steps = sample_steps(10);
        let (result, sent) = write_with_script(Vec::new(), true, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Erase,
                saved: false,
                ..
            })
        ));
        assert_eq!(sent.len(), 1);
        assert_no_save(&sent);
    }

    #[tokio::test]
    async fn write_timeout_does_not_send_header_or_save() {
        let steps = sample_steps(10);
        let script = vec![ack_frame(cmd::ERASE_MACRO, true)];
        let (result, sent) = write_with_script(script, true, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Write,
                saved: false,
                ..
            })
        ));
        assert_eq!(sent.len(), 2);
        assert_no_save(&sent);
        assert!(sent
            .iter()
            .all(|frame| frame_cmd(frame) != cmd::WRITE_CONFIG));
    }

    #[tokio::test]
    async fn header_timeout_does_not_send_save() {
        let steps = sample_steps(20);
        let mut script = vec![ack_frame(cmd::ERASE_MACRO, false)];
        for _ in 0..4 {
            script.push(ack_frame(cmd::WRITE_MACRO, false));
        }
        let (result, sent) = write_with_script(script, true, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Header,
                saved: false,
                ..
            })
        ));
        assert_eq!(sent.len(), 6);
        assert_no_save(&sent);
    }

    #[tokio::test]
    async fn second_header_chunk_timeout_does_not_send_save() {
        let steps = sample_steps(20);
        let mut script = vec![ack_frame(cmd::ERASE_MACRO, true)];
        for _ in 0..4 {
            script.push(ack_frame(cmd::WRITE_MACRO, true));
        }
        // ACK only the first 142-byte header chunk; the 74-byte second chunk
        // is sent but never acknowledged.
        script.push(ack_frame(cmd::WRITE_CONFIG, true));
        let (result, sent) = write_with_script(script, true, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Header,
                saved: false,
                ..
            })
        ));
        assert_eq!(sent.len(), 7);
        assert_no_save(&sent);
        let header_frames: Vec<_> = sent
            .iter()
            .filter(|frame| frame_cmd(frame) == cmd::WRITE_CONFIG)
            .collect();
        assert_eq!(header_frames.len(), 2);
        assert_eq!(header_frames[0].len(), 17 + 142);
        assert_eq!(header_frames[1].len(), 17 + 74);
    }

    #[tokio::test]
    async fn save_timeout_is_reported_as_saved() {
        let steps = sample_steps(20);
        let mut script = vec![ack_frame(cmd::ERASE_MACRO, true)];
        for _ in 0..4 {
            script.push(ack_frame(cmd::WRITE_MACRO, true));
        }
        script.push(ack_frame(cmd::WRITE_CONFIG, true));
        script.push(ack_frame(cmd::WRITE_CONFIG, true));
        let (result, sent) = write_with_script(script, true, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Save,
                saved: true,
                ..
            })
        ));
        assert_eq!(frame_cmd(sent.last().unwrap()), cmd::SAVE);
    }

    #[tokio::test]
    async fn disconnect_mid_write_does_not_send_save() {
        let steps = sample_steps(10);
        let script = vec![ack_frame(cmd::ERASE_MACRO, true)];
        let (result, sent) = write_with_script(script, false, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Write,
                saved: false,
                ..
            })
        ));
        assert_no_save(&sent);
    }

    fn full_success_script(steps: &[MacroStep], config: &Bt2Config) -> Vec<Vec<u8>> {
        let mut script = vec![ack_frame(cmd::ERASE_MACRO, true)];
        for _ in 0..4 {
            script.push(ack_frame(cmd::WRITE_MACRO, true));
        }
        script.push(ack_frame(cmd::WRITE_CONFIG, true));
        script.push(ack_frame(cmd::WRITE_CONFIG, true));
        script.push(ack_frame(cmd::SAVE, true));
        script.extend(config_frames(config));
        let raw = raw_steps(steps);
        let mut offset = 0u32;
        for chunk in raw.chunks(142) {
            let mut header =
                CustomHeader::new(cmd::RESPONSE, cmd::READ_MACRO, raw.len() as u32, offset)
                    .with_chunk_len(chunk.len() as u16);
            header.crc16 = packet_crc16(chunk);
            script.push(ScriptedTransport::response(header, chunk));
            offset += chunk.len() as u32;
        }
        script
    }

    #[tokio::test]
    async fn readback_crc_mismatch_fails_after_save() {
        let steps = sample_steps(20);
        let backup = backup_from(config_with_existing_macro());
        let plan = MacroWritePlan::new(steps.clone());
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        let mut script = full_success_script(&steps, &expected);
        // Corrupt the first read-back CRC.
        let first_readback = script.len() - 1;
        let mut header = CustomHeader::parse(&script[first_readback][..16]).unwrap();
        header.crc16 = header.crc16.wrapping_add(1);
        script[first_readback][..16].copy_from_slice(&header.to_bytes());

        let (result, sent) = write_with_script(script, true, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Verify,
                saved: true,
                ..
            })
        ));
        assert!(sent.iter().any(|frame| frame_cmd(frame) == cmd::SAVE));
    }

    #[tokio::test]
    async fn readback_mismatch_fails_after_save() {
        let steps = sample_steps(20);
        let backup = backup_from(config_with_existing_macro());
        let plan = MacroWritePlan::new(steps.clone());
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        let mut script = full_success_script(&steps, &expected);
        // Replace the last read-back payload with different bytes of the same
        // length and a valid CRC.
        let last = script.len() - 1;
        let payload = vec![0xAAu8; script[last].len() - 16];
        let mut header = CustomHeader::parse(&script[last][..16]).unwrap();
        header.crc16 = packet_crc16(&payload);
        script[last] = ScriptedTransport::response(header, &payload);

        let (result, sent) = write_with_script(script, true, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Verify,
                saved: true,
                ..
            })
        ));
        assert!(sent.iter().any(|frame| frame_cmd(frame) == cmd::SAVE));
    }

    #[tokio::test]
    async fn backup_timeout_starts_no_write() {
        let backup = backup_from(config_with_existing_macro());
        let transport = ScriptedTransport::new(Vec::new(), true, 200);
        let session = Session::new(&transport).with_timeout(Duration::from_millis(10));
        let result = session.read_macro_backup(backup.snapshot()).await;
        assert!(result.is_err());
        let sent = transport.sent();
        assert!(!sent.is_empty());
        assert!(sent.iter().all(|frame| frame_cmd(frame) == cmd::READ_MACRO));
        assert_no_save(&sent);
    }
    /// Wraps a scripted transport with a BLE-like frame-size limit.
    struct LimitedTransport {
        inner: ScriptedTransport,
        limit: usize,
    }

    #[async_trait]
    impl Transport for LimitedTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            if data.len() > self.limit {
                return Err(Error::Transport(format!(
                    "frame of {} bytes exceeds the BLE limit {}",
                    data.len(),
                    self.limit
                )));
            }
            self.inner.send(data).await
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            self.inner.recv().await
        }

        fn mtu(&self) -> u16 {
            200
        }

        async fn disconnect(&self) -> Result<(), Error> {
            self.inner.disconnect().await
        }
    }

    #[tokio::test]
    async fn header_split_fits_mtu_200_frame_limit() {
        let steps = sample_steps(20);
        let backup = backup_from(config_with_existing_macro());
        let plan = MacroWritePlan::new(steps.clone());
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        let transport = LimitedTransport {
            inner: ScriptedTransport::new(full_success_script(&steps, &expected), true, 200),
            limit: 200 - 3,
        };
        let session = Session::new(&transport);
        session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await
            .unwrap();

        let sent = transport.inner.sent();
        assert!(
            sent.iter().all(|frame| frame.len() <= 197),
            "every BLE frame must fit mtu - 3 = 197 bytes"
        );
        let headers: Vec<_> = sent
            .iter()
            .filter(|frame| frame_cmd(frame) == cmd::WRITE_CONFIG)
            .collect();
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].len(), 17 + 142);
        assert_eq!(headers[1].len(), 17 + 74);
        assert_eq!(CustomHeader::parse(&headers[0][1..]).unwrap().offset, 0x68C);
        assert_eq!(
            CustomHeader::parse(&headers[1][1..]).unwrap().offset,
            0x68C + 142
        );
        assert_eq!(
            CustomHeader::parse(&headers[0][1..]).unwrap().total_len,
            0x0AD0
        );
        assert_eq!(
            CustomHeader::parse(&headers[1][1..]).unwrap().total_len,
            0x0AD0
        );
    }
    #[test]
    fn mtu_chunk_helpers_follow_att_limit() {
        assert_eq!(max_protocol_payload(200), 180);
        assert_eq!(macro_header_chunk_len(200).unwrap(), 142);
        assert_eq!(macro_write_chunk_len(200).unwrap(), 64);

        assert_eq!(max_protocol_payload(64), 44);
        assert_eq!(macro_header_chunk_len(64).unwrap(), 44);
        assert_eq!(macro_write_chunk_len(64).unwrap(), 32);

        assert_eq!(max_protocol_payload(23), 3);
        assert_eq!(macro_header_chunk_len(23).unwrap(), 3);
        assert!(matches!(
            macro_write_chunk_len(23),
            Err(Error::MtuTooSmall { mtu: 23, .. })
        ));

        assert_eq!(max_protocol_payload(20), 0);
        assert!(matches!(
            macro_header_chunk_len(20),
            Err(Error::MtuTooSmall { mtu: 20, .. })
        ));
    }

    #[tokio::test]
    async fn mtu_23_rejects_macro_write_before_sending() {
        let backup = backup_from(config_with_existing_macro());
        let steps = sample_steps(10);
        let plan = MacroWritePlan::new(steps).with_timeouts(short_timeouts());
        let transport = ScriptedTransport::new(Vec::new(), true, 23);
        let session = Session::new(&transport);
        let result = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await;
        assert!(matches!(result, Err(Error::MtuTooSmall { mtu: 23, .. })));
        assert!(
            transport.sent().is_empty(),
            "MTU 23 must fail before Erase/Write/Header/Save"
        );
    }
    #[tokio::test]
    async fn empty_macro_write_is_rejected_before_any_frame() {
        let backup = backup_from(config_with_existing_macro());
        let steps = sample_steps(10);
        let plan = MacroWritePlan::new(steps);
        let transport = ScriptedTransport::new(Vec::new(), true, 200);
        let session = Session::new(&transport);
        let result = session
            .write_macro(&backup, Slot::S1, MacroIndex::M3, &plan)
            .await;
        assert!(matches!(result, Err(Error::EmptyMacroWrite)));
        assert!(
            transport.sent().is_empty(),
            "empty macro rejection must not send Erase/Write/Header/Save"
        );
    }

    #[tokio::test]
    async fn usb_save_transport_is_rejected_before_any_frame() {
        let backup = backup_from(config_with_existing_macro());
        let steps = sample_steps(10);
        let plan = MacroWritePlan::new(steps).with_save_transport(SaveTransport::Usb);
        let transport = ScriptedTransport::new(Vec::new(), true, 200);
        let session = Session::new(&transport);
        let result = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await;
        assert!(matches!(
            result,
            Err(Error::Unsupported(message)) if message.contains("USB")
        ));
        assert!(transport.sent().is_empty());
    }

    #[tokio::test]
    async fn header_readback_mismatch_fails_after_save() {
        let steps = sample_steps(20);
        let backup = backup_from(config_with_existing_macro());
        // The device returns the original header instead of the updated one.
        let script = full_success_script(&steps, &backup.snapshot().config);
        let (result, sent) = write_with_script(script, true, &steps).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Verify,
                saved: true,
                reason,
                ..
            }) if reason.contains("normalize")
        ));
        assert!(sent.iter().any(|frame| frame_cmd(frame) == cmd::SAVE));
    }

    #[tokio::test]
    async fn write_macro_header_writes_only_the_header_and_saves() {
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        config.recompute_crc();
        let backup = backup_from(config);
        let mock = MockTransport::with_mtu(200);
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::SAVE, true);
        push_config(&mock, &backup.snapshot().config);

        let session = Session::new(&mock);
        let outcome = session.write_macro_header(&backup, Slot::S1).await.unwrap();

        let sent = mock.sent_frames();
        assert!(!sent
            .iter()
            .any(|frame| matches!(frame_cmd(frame), cmd::ERASE_MACRO | cmd::WRITE_MACRO)));
        assert_eq!(outcome.packets_sent, 3);
        let commands: Vec<u16> = sent
            .iter()
            .map(|frame| frame_cmd(frame))
            .filter(|command| *command != cmd::READ_CONFIG)
            .collect();
        assert_eq!(
            commands,
            vec![cmd::WRITE_CONFIG, cmd::WRITE_CONFIG, cmd::SAVE]
        );
    }

    #[tokio::test]
    async fn write_macro_header_rejects_non_empty_slot() {
        let backup = backup_from(config_with_existing_macro());
        let transport = ScriptedTransport::new(Vec::new(), true, 200);
        let session = Session::new(&transport);
        let result = session.write_macro_header(&backup, Slot::S1).await;
        assert!(matches!(
            result,
            Err(Error::Unsupported(message)) if message.contains("all-empty")
        ));
        assert!(transport.sent().is_empty());
    }

    #[tokio::test]
    async fn write_macro_header_readback_mismatch_fails_after_save() {
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        config.recompute_crc();
        let backup = backup_from(config.clone());

        let mut changed = config.clone();
        let mut header = changed.macro_slot(Slot::S1);
        header.flag = 1;
        changed.set_macro_slot(Slot::S1, &header);
        changed.recompute_crc();

        let mock = MockTransport::with_mtu(200);
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::SAVE, true);
        push_config(&mock, &changed);

        let session = Session::new(&mock);
        let result = session.write_macro_header(&backup, Slot::S1).await;
        assert!(matches!(
            result,
            Err(Error::MacroWrite {
                stage: MacroWriteStage::Verify,
                saved: true,
                reason,
                ..
            }) if reason.contains("normalize")
        ));
    }

    #[tokio::test]
    async fn write_macro_header_rejects_total_cnt_above_four() {
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        let mut header = config.macro_slot(Slot::S1);
        header.total_cnt = 5;
        config.set_macro_slot(Slot::S1, &header);
        config.recompute_crc();
        let backup = backup_from(config);

        let transport = ScriptedTransport::new(Vec::new(), true, 200);
        let session = Session::new(&transport);
        let result = session.write_macro_header(&backup, Slot::S1).await;
        assert!(matches!(result, Err(Error::Protocol(_))));
        assert!(transport.sent().is_empty());
    }

    #[tokio::test]
    async fn write_macro_preserves_unspecified_header_fields() {
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::XInput);
        let mut header = config.macro_slot(Slot::S1);
        header.flag = 1;
        header.total_cnt = 2;
        let mut record = MacroRecord {
            max_steps: 4,
            key_map: KeyMap::M2,
            cycles_num: 7,
            interval_ms: 25,
            offset: 0x1234,
            special_flag: 2,
            gamepad_mode: 1,
            ..MacroRecord::default()
        };
        record.set_name("Keep").unwrap();
        header.records[0] = record;
        config.set_macro_slot(Slot::S1, &header);
        let header_offset = macro_slot_offset(Slot::S1);
        config.as_mut_bytes()[header_offset + 5..header_offset + 8]
            .copy_from_slice(&[0xA1, 0xA2, 0xA3]);
        config.as_mut_bytes()[header_offset + 8 + 0x26..header_offset + 8 + 0x28]
            .copy_from_slice(&[0xB1, 0xB2]);
        config.recompute_crc();
        let backup = backup_from(config);

        let steps = sample_steps(10);
        let plan = MacroWritePlan::new(steps.clone());
        let mock = MockTransport::with_mtu(200);
        push_ack(&mock, cmd::ERASE_MACRO, true);
        for _ in 0..2 {
            push_ack(&mock, cmd::WRITE_MACRO, true);
        }
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::SAVE, true);
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        push_config(&mock, &expected);
        push_readback(&mock, &steps);

        let session = Session::new(&mock);
        let outcome = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await
            .unwrap();

        assert_eq!(outcome.record.max_steps, 10);
        assert_eq!(outcome.record.name(), "Keep");
        assert_eq!(outcome.record.key_map, KeyMap::M2);
        assert_eq!(outcome.record.cycles_num, 7);
        assert_eq!(outcome.record.interval_ms, 25);
        assert_eq!(outcome.record.offset, 0x1234);
        assert_eq!(outcome.record.special_flag, 2);
        assert_eq!(outcome.record.gamepad_mode, 1);
        assert_eq!(outcome.flag, 1);
        assert_eq!(outcome.total_cnt, 2);

        let header_payload: Vec<u8> = mock
            .sent_frames()
            .iter()
            .filter(|frame| frame_cmd(frame) == cmd::WRITE_CONFIG)
            .flat_map(|frame| frame[17..].iter().copied())
            .collect();
        let record = &header_payload[8..8 + MACRO_RECORD_SIZE];
        assert_eq!(
            &record[0..8],
            &[0x00, 0x4B, 0x00, 0x65, 0x00, 0x65, 0x00, 0x70]
        );
        assert_eq!(record[0x20], 1);
        assert_eq!(record[0x21], 2);
        assert_eq!(u16::from_le_bytes([record[0x22], record[0x23]]), 10);
        assert_eq!(u16::from_le_bytes([record[0x24], record[0x25]]), 0x1234);
        assert_eq!(
            u32::from_le_bytes([record[0x28], record[0x29], record[0x2A], record[0x2B]]),
            KeyMap::M2.bits()
        );
        assert_eq!(
            u32::from_le_bytes([record[0x2C], record[0x2D], record[0x2E], record[0x2F]]),
            7
        );
        assert_eq!(
            u32::from_le_bytes([record[0x30], record[0x31], record[0x32], record[0x33]]),
            25
        );
        assert_eq!(
            u32::from_le_bytes(header_payload[0..4].try_into().unwrap()),
            1
        );
        assert_eq!(header_payload[4], 2);
        assert_eq!(&header_payload[5..8], &[0xA1, 0xA2, 0xA3]);
        assert_eq!(&record[0x26..0x28], &[0xB1, 0xB2]);
    }
    #[test]
    fn macro_backup_rejects_missing_duplicate_and_mismatched_entries() {
        let config = config_with_existing_macro();
        let mut entries = Vec::new();
        for slot in Slot::ALL {
            for index in MacroIndex::ALL {
                let record = config.macro_record(slot, index);
                let raw = if is_skipped_steps(record.max_steps) {
                    Vec::new()
                } else {
                    raw_steps(&sample_steps(usize::from(record.max_steps)))
                };
                entries.push(MacroEntry::new(slot, index, record, raw).unwrap());
            }
        }
        let snapshot = snapshot(config.clone());

        let mut missing = entries.clone();
        missing.pop();
        assert!(matches!(
            MacroBackup::new(snapshot.clone(), missing),
            Err(Error::UnexpectedResponse(message))
                if message.contains("needs") || message.contains("missing")
        ));

        let mut duplicate = entries.clone();
        duplicate[1] = duplicate[0].clone();
        assert!(matches!(
            MacroBackup::new(snapshot.clone(), duplicate),
            Err(Error::UnexpectedResponse(message)) if message.contains("repeats")
        ));

        let record = config.macro_record(Slot::S1, MacroIndex::M1);
        let mut mismatched = entries;
        let mut changed = record;
        changed.max_steps += 1;
        mismatched[0] = MacroEntry::new(
            Slot::S1,
            MacroIndex::M1,
            changed,
            raw_steps(&sample_steps(usize::from(changed.max_steps))),
        )
        .unwrap();
        assert!(matches!(
            MacroBackup::new(snapshot, mismatched),
            Err(Error::UnexpectedResponse(message)) if message.contains("does not match")
        ));
    }

    #[tokio::test]
    async fn write_macro_rejects_base_header_total_cnt_above_four() {
        let mut config = config_with_existing_macro();
        let mut header = config.macro_slot(Slot::S1);
        header.total_cnt = 5;
        config.set_macro_slot(Slot::S1, &header);
        config.recompute_crc();
        let backup = backup_from(config);
        let plan = MacroWritePlan::new(sample_steps(10));

        let transport = ScriptedTransport::new(Vec::new(), true, 200);
        let session = Session::new(&transport);
        let result = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await;
        assert!(matches!(result, Err(Error::Protocol(_))));
        assert!(transport.sent().is_empty());
    }

    #[tokio::test]
    async fn write_macro_preserves_non_canonical_raw_name_bytes() {
        let mut config = config_with_existing_macro();
        let mut record = config.macro_record(Slot::S1, MacroIndex::M1);
        record.name = [0u8; 32];
        record.name[0] = 0x00;
        record.name[1] = 0xD8; // unpaired high surrogate
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);
        config.recompute_crc();
        let backup = backup_from(config);

        let steps = sample_steps(10);
        let plan = MacroWritePlan::new(steps.clone());
        let mock = MockTransport::with_mtu(200);
        push_ack(&mock, cmd::ERASE_MACRO, true);
        for _ in 0..2 {
            push_ack(&mock, cmd::WRITE_MACRO, true);
        }
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::WRITE_CONFIG, true);
        push_ack(&mock, cmd::SAVE, true);
        let expected = expected_config(&backup, Slot::S1, MacroIndex::M1, &plan);
        push_config(&mock, &expected);
        push_readback(&mock, &steps);

        let session = Session::new(&mock);
        session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await
            .unwrap();

        let header: Vec<u8> = mock
            .sent_frames()
            .iter()
            .filter(|frame| frame_cmd(frame) == cmd::WRITE_CONFIG)
            .flat_map(|frame| frame[17..].iter().copied())
            .collect();
        assert_eq!(&header[8..12], &[0x00, 0xD8, 0x00, 0x00]);
    }

    #[tokio::test]
    async fn write_macro_rejects_total_cnt_above_four() {
        let backup = backup_from(config_with_existing_macro());
        let steps = sample_steps(10);
        let plan = MacroWritePlan::new(steps).with_header_state(1, 5);
        let transport = ScriptedTransport::new(Vec::new(), true, 200);
        let session = Session::new(&transport);
        let result = session
            .write_macro(&backup, Slot::S1, MacroIndex::M1, &plan)
            .await;
        assert!(matches!(result, Err(Error::Protocol(_))));
        assert!(transport.sent().is_empty());
    }
}
