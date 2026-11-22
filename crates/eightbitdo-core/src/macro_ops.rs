//! Macro flash read operations for all 3 slots × 4 macro indices.
//!
//! The macro step payload lives outside the 2768-byte configuration block.
//! Each macro owns one 4 KB flash sector (`macro_idx << 12`) and its length is
//! taken from the `max_steps` field of the matching `MacroFunPro3Record` in
//! the configuration read. One step is 10 bytes.
//!
//! This module is read-only for now; the write state machine is added
//! separately so a read-only build cannot send `0x0104` / `0x0103` / `0x0001`
//! / `0x0006` by accident.

use std::time::Duration;

use eightbitdo_protocol::commands::{self, cmd};
use eightbitdo_protocol::config_bt2::{
    Bt2Config, MacroIndex, MacroRecord, Slot, MACROS_PER_SLOT, SLOT_COUNT,
};
use eightbitdo_protocol::header::PacketKind;
use eightbitdo_protocol::macro_data::{self, MacroStep};
use tokio::time::Instant;

use crate::error::Error;
use crate::session::{deadline_after, out_of_range, ConfigSnapshot, Session, TransferBuffer};
use crate::transport::Transport;

/// Overall deadline for assembling one macro read.
///
/// A full 200-step macro is 2000 bytes and may arrive in several
/// notifications. The per-frame wait is the session timeout
/// ([`Session::with_timeout`]).
pub const MACRO_READ_TOTAL_TIMEOUT: Duration = crate::session::CONFIG_TOTAL_TIMEOUT;

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
        })
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

    /// `true` when the header declares zero steps or the unset marker, so no
    /// flash read was made.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        is_skipped_steps(self.record.max_steps)
    }
}

/// `max_steps` values that mean "no macro data to read".
///
/// `0` is documented as an empty macro. `0xFFFF` occurs in real dumps as an
/// unset marker (実機未確認); it is skipped rather than turned into a
/// 655,350-byte read. Both are recorded in the backup JSON.
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
    /// a flash read. Otherwise the request is payload-less
    /// `cmd 0x0102`, `subcmd = (gamepad_mode << 8) | slot`,
    /// `offset = macro_idx << 12`, and `total_len = chunk_len = steps * 10`.
    ///
    /// Responses may be a direct `cmd 0x0102` frame or the
    /// `cmd 0x0004, subcmd 0x0102` envelope. Split notifications are
    /// assembled by their reported offset. The response offset may be either
    /// sector-based (`macro_idx << 12`) or relative to the sector start; both
    /// in-range forms are accepted and overlap is rejected.
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
        let request = commands::read_macro(mode, slot, index, record.max_steps)?;
        let expected_len = usize::from(record.max_steps) * macro_data::MACRO_STEP_SIZE;
        let raw = self.read_macro_chunks(&request, expected_len).await?;
        MacroEntry::new(slot, index, record, raw)
    }

    async fn read_macro_chunks(
        &self,
        request: &commands::Request,
        expected_len: usize,
    ) -> Result<Vec<u8>, Error> {
        self.transport()
            .send(&request.encode_ble_payloadless(PacketKind::App)?)
            .await?;

        let sector = request.header.offset as usize;
        let mut buffer = TransferBuffer::new(expected_len);
        let deadline = deadline_after(MACRO_READ_TOTAL_TIMEOUT)?;

        while !buffer.is_complete() {
            let now = Instant::now();
            if now >= deadline {
                return Err(macro_read_timeout(buffer.received(), expected_len));
            }
            let wait = self
                .per_frame_timeout()
                .min(deadline.saturating_duration_since(now));
            let frame = match tokio::time::timeout(wait, self.transport().recv()).await {
                Ok(result) => result?,
                Err(_) => {
                    return Err(macro_read_timeout(buffer.received(), expected_len));
                }
            };

            let Some((header, payload)) =
                Self::classify_response(&frame, cmd::READ_MACRO, expected_len)?
            else {
                continue;
            };

            let reported = header.offset as usize;
            let relative = if reported >= sector && reported - sector < expected_len {
                reported - sector
            } else if reported < expected_len {
                reported
            } else {
                return Err(out_of_range(reported, expected_len));
            };

            // A final notification may carry padding past the declared read
            // length; only the step bytes are kept.
            let remaining = expected_len - relative;
            let copy_len = payload.len().min(remaining);
            buffer.insert(relative, &payload[..copy_len])?;
        }

        Ok(buffer.into_vec())
    }
}

fn macro_read_timeout(received: usize, expected: usize) -> Error {
    Error::Timeout {
        operation: format!("ReadBT2Macro cmd=0x0102 (received {received} of {expected} bytes)"),
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
    async fn read_macro_sends_exact_payloadless_request_and_decodes_split_envelope() {
        let config = config_with_macros();
        let snapshot = snapshot(config);
        let steps = sample_steps(4);
        let raw = sample_entry_payload(&steps);

        let mock = MockTransport::new();
        let first = &raw[..20];
        let second = &raw[20..];
        mock.push_response(&envelope(cmd::READ_MACRO, 40, 0, first), first);
        mock.push_response(&envelope(cmd::READ_MACRO, 40, 20, second), second);

        let session = Session::new(&mock);
        let entry = session
            .read_macro(&snapshot, Slot::S1, MacroIndex::M1)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0],
            vec![
                0x04, // BLE App prefix
                0x02, 0x01, // cmd = 0x0102
                0x00, 0x00, // subcmd = (Switch 0 << 8) | slot 0
                0x28, 0x00, // chunk_len = 40
                0x00, 0x00, // crc16
                0x28, 0x00, 0x00, 0x00, // total_len = 40
                0x00, 0x00, 0x00, 0x00, // offset = macro 0 << 12
            ]
        );

        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.steps(), steps);
        assert_eq!(entry.record().name(), "Combo");
        assert_eq!(entry.record().key_map, KeyMap::M1);
    }

    #[tokio::test]
    async fn read_macro_accepts_sector_based_offsets_and_direct_responses() {
        let mut config = config_with_macros();
        config.set_gamepad_mode(GamepadMode::XInput);
        let mut record = config.macro_record(Slot::S2, MacroIndex::M2);
        record.max_steps = 3;
        config.set_macro_record(Slot::S2, MacroIndex::M2, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(3);
        let raw = sample_entry_payload(&steps);
        let mock = MockTransport::new();
        for (index, chunk) in raw.chunks(10).enumerate() {
            let offset = 0x1000 + (index * 10) as u32;
            let header = CustomHeader::new(cmd::READ_MACRO, 0x0301, 30, offset)
                .with_chunk_len(chunk.len() as u16);
            mock.push_response(&header, chunk);
        }

        let session = Session::new(&mock);
        let entry = session
            .read_macro(&snapshot, Slot::S2, MacroIndex::M2)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 1);
        // subcmd = (XInput 3 << 8) | slot 1, offset = macro 1 << 12.
        assert_eq!(&sent[0][3..5], &[0x01, 0x03]);
        assert_eq!(&sent[0][13..17], &[0x00, 0x10, 0x00, 0x00]);
        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.steps(), steps);
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
}
