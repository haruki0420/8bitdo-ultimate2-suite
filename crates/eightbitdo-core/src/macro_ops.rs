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
use crate::session::{deadline_after, ConfigSnapshot, Session, TransferBuffer};
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
        let sector = index.sector_offset();
        let mut buffer = TransferBuffer::new(expected_len);
        let mut chunk_offset = 0usize;
        let mut response_offsets = Vec::new();
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
                // Accepted forms for every chunk:
                // - 0: the device's sequential-chunk convention;
                // - the requested absolute offset (`sector + chunk_offset`);
                // - a sector-relative offset equal to `chunk_offset`
                //   (未確定: macro_idx > 0 の実機応答で確認する).
                let relative_match =
                    reported >= sector && u64::from(reported - sector) == chunk_offset as u64;
                if reported != 0 && reported != requested && !relative_match {
                    return Err(Error::UnexpectedResponse(format!(
                        "macro chunk response offset 0x{reported:X} does not match requested \
                         0x{requested:X} (accepted: 0 for the sequential convention, or the \
                         requested offset; sector-relative 0x{chunk_offset:X} is also accepted)"
                    )));
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
    async fn read_macro_accepts_sector_based_and_zero_offsets_and_logs_them() {
        let mut config = config_with_macros();
        config.set_gamepad_mode(GamepadMode::XInput);
        let mut record = config.macro_record(Slot::S2, MacroIndex::M2);
        record.max_steps = 20;
        config.set_macro_record(Slot::S2, MacroIndex::M2, &record);
        config.recompute_crc();
        let snapshot = snapshot(config);

        let steps = sample_steps(20);
        let raw = sample_entry_payload(&steps);
        assert_eq!(raw.len(), 200);

        // First response reports the absolute sector base, the second uses the
        // device's sequential-chunk convention (offset 0).
        let mock = MockTransport::new();
        let first = &raw[..142];
        let second = &raw[142..];
        let mut first_header = envelope(cmd::READ_MACRO, 200, 0x1000, first);
        first_header.crc16 = packet_crc16(first);
        mock.push_response(&first_header, first);
        let mut second_header = envelope(cmd::READ_MACRO, 200, 0, second);
        second_header.crc16 = packet_crc16(second);
        mock.push_response(&second_header, second);

        let session = Session::new(&mock);
        let entry = session
            .read_macro(&snapshot, Slot::S2, MacroIndex::M2)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 2);
        let first_request = CustomHeader::parse(&sent[0][1..]).unwrap();
        assert_eq!(first_request.subcmd, 0x0301);
        assert_eq!(first_request.chunk_len, 200);
        assert_eq!(first_request.total_len, 200);
        assert_eq!(first_request.offset, 0x1000);
        let second_request = CustomHeader::parse(&sent[1][1..]).unwrap();
        assert_eq!(second_request.offset, 0x1000 + 142);

        assert_eq!(entry.raw(), raw);
        assert_eq!(entry.read_offsets(), &[0x1000, 0]);
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
