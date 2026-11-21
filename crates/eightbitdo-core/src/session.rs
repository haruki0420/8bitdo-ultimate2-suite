//! High-level, read-only App Mode operations on top of a [`Transport`].
//!
//! [`Session`] owns the framing rules: it encodes requests for the BLE App
//! channel (`0x04` prefix), waits for notifications, assembles split
//! transfers by header offset, and decodes the 12-byte input report.
//!
//! Every operation in this module is read-only except
//! [`Session::set_report_enable`], which toggles the realtime input stream.

use std::time::Duration;

use eightbitdo_protocol::commands;
use eightbitdo_protocol::config_bt2::{Bt2Config, CONFIG_SIZE};
use eightbitdo_protocol::header::{self, PacketKind};
use eightbitdo_protocol::input_report::InputReport;
use tokio::time::Instant;

use crate::transport::Transport;
use crate::Error;

/// Default per-frame wait used when no explicit timeout is configured.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Overall deadline for one 2768-byte configuration read.
pub const CONFIG_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound for an assembled `ReadVersion` response.
///
/// A real response is [`commands::VERSION_RESPONSE_LEN`] bytes. The bound
/// exists only to reject a corrupt or wrong-model `total_len` before
/// allocating.
const MAX_VERSION_LEN: usize = 0x1000;

/// Decoded firmware version response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// Raw response payload exactly as received (assembled when split).
    pub raw: Vec<u8>,
    /// Longest printable ASCII run in `raw`, trimmed.
    ///
    /// Empty when the payload contains no printable bytes.
    pub text: String,
}

/// A complete configuration read plus its CRC status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSnapshot {
    /// The 2768-byte configuration block, byte-for-byte as received.
    pub config: Bt2Config,
    /// CRC stored at `+0x0C` (`crc16_stored() as u16`).
    pub stored_crc: u16,
    /// CRC recomputed in place over the received buffer.
    ///
    /// This mirrors the official `ANSI_CRC_16_sh` call, which includes the
    /// existing CRC field in its input.
    pub computed_crc: u16,
    /// True when `stored_crc == computed_crc`.
    pub crc_valid: bool,
}

/// A read-only App Mode session over a [`Transport`].
///
/// The transport is consumed and can be recovered with
/// [`Session::into_inner`].
pub struct Session<T: Transport> {
    transport: T,
    timeout: Duration,
}

impl<T: Transport> Session<T> {
    /// Wrap a connected transport with the default per-frame timeout.
    #[must_use]
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Set the per-frame wait used by version and input reads.
    ///
    /// Configuration reads always use [`CONFIG_TOTAL_TIMEOUT`] as their
    /// overall deadline and this timeout for each individual frame.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Borrow the underlying transport.
    #[must_use]
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Consume the session and return the transport.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.transport
    }

    /// Read the firmware version (`cmd 0x000B`).
    ///
    /// A response may arrive as one payload or split by `header.offset`. When
    /// the first chunk declares a larger `total_len` (or a non-zero offset),
    /// the remaining chunks are assembled before decoding.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when no matching notification arrives within
    /// the configured per-frame timeout, [`Error::UnexpectedResponse`] for a
    /// range outside the declared transfer, or a transport error.
    pub async fn read_version(&self) -> Result<VersionInfo, Error> {
        self.transport
            .send(&commands::read_version().encode_ble(PacketKind::App)?)
            .await?;

        let deadline = Instant::now() + self.timeout;
        let (header, payload) = loop {
            let frame = self
                .recv_before(deadline, "waiting for the version response")
                .await?;
            let (header, payload) = header::decode_app_response(&frame)?;
            if header.cmd == commands::cmd::READ_VERSION {
                break (header, payload.to_vec());
            }
        };

        let offset = header.offset as usize;
        let declared = header.total_len as usize;
        let split = offset > 0 || (declared > payload.len() && declared <= MAX_VERSION_LEN);

        let raw = if split {
            let expected = declared.max(offset.saturating_add(payload.len()));
            if expected > MAX_VERSION_LEN {
                return Err(Error::UnexpectedResponse(format!(
                    "version response range 0x{offset:X}..0x{expected:X} exceeds 0x{MAX_VERSION_LEN:X} bytes"
                )));
            }
            self.collect_transfer(
                commands::cmd::READ_VERSION,
                expected,
                Some((header.offset, payload)),
                self.timeout,
                self.timeout,
            )
            .await?
        } else {
            payload
        };

        let text = longest_printable_run(&raw);
        Ok(VersionInfo { raw, text })
    }

    /// Read and validate the full 2768-byte configuration block.
    ///
    /// Notifications with `cmd 0x0002` are assembled by `header.offset`.
    /// Unrelated commands (for example the asynchronous `0x0012`
    /// "configuration allowed" notice) are skipped. Duplicate and overlapping
    /// chunks are accepted; every byte is counted exactly once.
    ///
    /// The CRC is checked with the official in-place algorithm: the stored
    /// `+0x0C` value is compared with `ANSI_CRC_16_sh` over a copy of the
    /// received buffer, which still contains that stored value.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when the transfer does not complete within
    /// [`CONFIG_TOTAL_TIMEOUT`] or a single frame does not arrive within the
    /// configured per-frame timeout, [`Error::UnexpectedResponse`] when a
    /// chunk points outside the 2768-byte block, or a transport error.
    pub async fn read_config_snapshot(&self) -> Result<ConfigSnapshot, Error> {
        self.transport
            .send(&commands::read_config_all().encode_ble(PacketKind::App)?)
            .await?;

        let bytes = self
            .read_transfer(
                commands::cmd::READ_CONFIG,
                CONFIG_SIZE,
                self.timeout,
                CONFIG_TOTAL_TIMEOUT,
            )
            .await?;

        let config = Bt2Config::from_bytes(&bytes)?;
        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();

        Ok(ConfigSnapshot {
            config,
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        })
    }

    /// Enable or disable the realtime input stream (`cmd 0x0007`).
    ///
    /// # Errors
    ///
    /// Returns a transport or protocol encoding error.
    pub async fn set_report_enable(&self, enabled: bool) -> Result<(), Error> {
        self.transport
            .send(&commands::set_report_enable(enabled).encode_ble(PacketKind::App)?)
            .await
    }

    /// Wait up to `wait` for the next realtime input report.
    ///
    /// Frames whose command is not `0x0003` are skipped. A timeout returns
    /// `Ok(None)`; a closed transport returns [`Error::Disconnected`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Disconnected`] when the transport closes, a protocol
    /// error for a malformed input frame, or another transport error.
    pub async fn try_next_input(&self, wait: Duration) -> Result<Option<InputReport>, Error> {
        let deadline = Instant::now() + wait;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let remaining = deadline.saturating_duration_since(now);
            let frame = match tokio::time::timeout(
                remaining.min(self.timeout),
                self.transport.recv(),
            )
            .await
            {
                Ok(result) => result?,
                Err(_) => return Ok(None),
            };

            let (header, payload) = header::decode_app_response(&frame)?;
            if header.cmd != commands::cmd::INPUT {
                continue;
            }
            return Ok(Some(InputReport::from_bytes(payload)?));
        }
    }

    /// Read a transfer of exactly `expected_len` bytes for `cmd`.
    ///
    /// Frames with another command are ignored. `header.offset` and
    /// `header.chunk_len` must stay inside `expected_len`. Bytes are written
    /// only on their first arrival, so duplicates cannot shorten or corrupt
    /// the transfer.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when `total_timeout` expires or a frame does
    /// not arrive within `per_frame_timeout`, [`Error::UnexpectedResponse`] for
    /// an out-of-range chunk, or a transport/protocol error.
    pub async fn read_transfer(
        &self,
        cmd: u16,
        expected_len: usize,
        per_frame_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        self.collect_transfer(cmd, expected_len, None, per_frame_timeout, total_timeout)
            .await
    }

    async fn collect_transfer(
        &self,
        cmd: u16,
        expected_len: usize,
        first: Option<(u32, Vec<u8>)>,
        per_frame_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        let mut buffer = TransferBuffer::new(expected_len);
        if let Some((offset, payload)) = first {
            buffer.insert(offset as usize, &payload)?;
        }

        let deadline = Instant::now() + total_timeout;
        while !buffer.is_complete() {
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout {
                    operation: format!(
                        "read transfer cmd=0x{cmd:04X} (received {} of {expected_len} bytes)",
                        buffer.received()
                    ),
                });
            }

            let remaining = deadline.saturating_duration_since(now);
            let wait = per_frame_timeout.min(remaining);
            let frame = match tokio::time::timeout(wait, self.transport.recv()).await {
                Ok(result) => result?,
                Err(_) => {
                    return Err(Error::Timeout {
                        operation: format!(
                            "read transfer cmd=0x{cmd:04X} (received {} of {expected_len} bytes)",
                            buffer.received()
                        ),
                    })
                }
            };

            let (header, payload) = header::decode_app_response(&frame)?;
            if header.cmd != cmd {
                continue;
            }
            buffer.insert(header.offset as usize, payload)?;
        }

        Ok(buffer.into_vec())
    }

    async fn recv_before(&self, deadline: Instant, operation: &str) -> Result<Vec<u8>, Error> {
        let now = Instant::now();
        if now >= deadline {
            return Err(Error::Timeout {
                operation: operation.to_string(),
            });
        }
        let wait = deadline.saturating_duration_since(now);
        match tokio::time::timeout(wait, self.transport.recv()).await {
            Ok(result) => result,
            Err(_) => Err(Error::Timeout {
                operation: operation.to_string(),
            }),
        }
    }
}

/// Byte buffer that tracks which positions have been filled.
struct TransferBuffer {
    data: Vec<u8>,
    filled: Vec<bool>,
    received: usize,
}

impl TransferBuffer {
    fn new(len: usize) -> Self {
        Self {
            data: vec![0u8; len],
            filled: vec![false; len],
            received: 0,
        }
    }

    fn insert(&mut self, offset: usize, chunk: &[u8]) -> Result<(), Error> {
        let expected = self.data.len();
        let end = offset
            .checked_add(chunk.len())
            .ok_or_else(|| out_of_range(offset, expected))?;
        if end > expected {
            return Err(out_of_range(offset, expected));
        }

        for (index, &byte) in chunk.iter().enumerate() {
            let position = offset + index;
            if !self.filled[position] {
                self.data[position] = byte;
                self.filled[position] = true;
                self.received += 1;
            }
        }
        Ok(())
    }

    fn received(&self) -> usize {
        self.received
    }

    fn is_complete(&self) -> bool {
        self.received == self.data.len()
    }

    fn into_vec(self) -> Vec<u8> {
        self.data
    }
}

fn out_of_range(offset: usize, expected: usize) -> Error {
    Error::UnexpectedResponse(format!(
        "chunk offset 0x{offset:X} is outside the {expected}-byte transfer"
    ))
}

/// Longest run of printable ASCII (`0x20..=0x7E`), trimmed.
fn longest_printable_run(bytes: &[u8]) -> String {
    let mut best: &[u8] = &[];
    let mut start: Option<usize> = None;

    for (index, &byte) in bytes.iter().enumerate() {
        if (0x20..=0x7E).contains(&byte) {
            if start.is_none() {
                start = Some(index);
            }
        } else if let Some(run_start) = start.take() {
            if index - run_start > best.len() {
                best = &bytes[run_start..index];
            }
        }
    }
    if let Some(run_start) = start {
        if bytes.len() - run_start > best.len() {
            best = &bytes[run_start..];
        }
    }

    String::from_utf8_lossy(best).trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use eightbitdo_protocol::config_bt2::{GamepadMode, Slot};
    use eightbitdo_protocol::header::CustomHeader;

    use super::*;
    use crate::mock::MockTransport;
    use crate::transport::Transport;

    fn config_header(offset: u32, chunk_len: u16) -> CustomHeader {
        CustomHeader::new(commands::cmd::READ_CONFIG, 0, CONFIG_SIZE as u32, offset)
            .with_chunk_len(chunk_len)
    }

    fn push_config_chunks(mock: &MockTransport, bytes: &[u8], chunk_size: usize) {
        for (index, chunk) in bytes.chunks(chunk_size).enumerate() {
            let header = config_header((index * chunk_size) as u32, chunk.len() as u16);
            mock.push_response(&header, chunk);
        }
    }

    fn sample_config() -> Bt2Config {
        let mut config = Bt2Config::zeroed();
        for slot in Slot::ALL {
            config.set_slot_flag(slot, 1);
        }
        config.set_profile_name(Slot::S1, "Main").unwrap();
        config.set_profile_name(Slot::S2, "Fight").unwrap();
        config.set_gamepad_mode(GamepadMode::Switch);
        config.set_cur_slot(Some(Slot::S2));
        config.recompute_crc();
        config
    }

    #[tokio::test]
    async fn read_version_sends_exact_app_frame_and_decodes_text() {
        let mock = MockTransport::new();
        let response =
            CustomHeader::new(commands::cmd::READ_VERSION, 0, 4, 0).with_payload(b"FW 1.23");
        mock.push_response(&response, b"FW 1.23");

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 1);
        let mut expected = vec![0x04];
        expected.extend_from_slice(&commands::read_version().header.to_bytes());
        assert_eq!(sent[0], expected);
        assert_eq!(sent[0][0], 0x04);
        assert_eq!(version.raw, b"FW 1.23");
        assert_eq!(version.text, "FW 1.23");
    }

    #[tokio::test]
    async fn version_chunks_are_assembled_by_offset() {
        let mock = MockTransport::new();
        let first = CustomHeader::new(commands::cmd::READ_VERSION, 0, 8, 0).with_payload(b"FW v");
        let second = CustomHeader::new(commands::cmd::READ_VERSION, 0, 8, 4).with_payload(b"1.23");
        mock.push_response(&first, b"FW v");
        mock.push_response(&second, b"1.23");

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();

        assert_eq!(version.raw, b"FW v1.23");
        assert_eq!(version.text, "FW v1.23");
    }

    #[tokio::test]
    async fn config_chunks_may_arrive_out_of_order() {
        let config = sample_config();
        let mock = MockTransport::new();
        let mut chunks: Vec<(u32, Vec<u8>)> = config
            .as_bytes()
            .chunks(692)
            .enumerate()
            .map(|(index, chunk)| ((index * 692) as u32, chunk.to_vec()))
            .collect();
        chunks.reverse();
        for (offset, chunk) in &chunks {
            mock.push_response(&config_header(*offset, chunk.len() as u16), chunk);
        }

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();

        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
        assert_eq!(snapshot.config.profile_name(Slot::S2), "Fight");
        assert_eq!(snapshot.config.cur_slot(), Some(Slot::S2));
    }

    #[tokio::test]
    async fn duplicate_chunks_do_not_count_twice_or_shorten_transfer() {
        let config = sample_config();
        let mock = MockTransport::new();
        let bytes = config.as_bytes();
        let first = &bytes[..1024];
        let second = &bytes[1024..2048];
        let third = &bytes[2048..];

        // The first chunk is repeated. If the assembler counted frames instead
        // of bytes it would finish after the third distinct payload.
        for (offset, chunk) in [(0u32, first), (0, first), (1024, second), (2048, third)] {
            mock.push_response(&config_header(offset, chunk.len() as u16), chunk);
        }

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), bytes);
    }

    #[tokio::test]
    async fn unrelated_commands_are_skipped() {
        let config = sample_config();
        let mock = MockTransport::new();

        let notice = CustomHeader::new(commands::cmd::ALLOW_CONFIG_DONE, 0, 0, 0);
        mock.push_response(&notice, &[]);
        let version =
            CustomHeader::new(commands::cmd::READ_VERSION, 0, 4, 0).with_payload(b"stale");
        mock.push_response(&version, b"stale");
        push_config_chunks(&mock, config.as_bytes(), 1000);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
    }

    #[tokio::test]
    async fn out_of_range_config_chunk_is_rejected() {
        let mock = MockTransport::new();
        let payload = [0u8; 16];
        let header = config_header(CONFIG_SIZE as u32 - 8, 16);
        mock.push_response(&header, &payload);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(matches!(result, Err(Error::UnexpectedResponse(_))));
    }

    /// Transport that delivers one frame, then never answers again.
    struct StallingTransport {
        first: Mutex<Option<Vec<u8>>>,
    }

    #[async_trait]
    impl Transport for StallingTransport {
        async fn send(&self, _data: &[u8]) -> Result<(), Error> {
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            let queued = self
                .first
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(frame) = queued {
                return Ok(frame);
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
            Err(Error::Disconnected)
        }

        fn mtu(&self) -> u16 {
            64
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn incomplete_transfer_times_out() {
        let mut frame = config_header(0, 1024).to_bytes().to_vec();
        frame.extend_from_slice(&[0u8; 1024]);
        let transport = StallingTransport {
            first: Mutex::new(Some(frame)),
        };

        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));
        let result = session.read_config_snapshot().await;
        assert!(matches!(result, Err(Error::Timeout { .. })));
    }

    #[tokio::test]
    async fn input_reader_skips_other_frames_and_decodes_report() {
        let mock = MockTransport::new();
        let notice = CustomHeader::new(commands::cmd::READ_VERSION, 0, 4, 0).with_payload(b"noise");
        mock.push_response(&notice, b"noise");

        let report = InputReport {
            keys: 0x0000_2001,
            left_stick_x: 0x7F,
            left_stick_y: 0x80,
            right_stick_x: 0xFF,
            right_stick_y: 0x00,
            trigger_r: 0xCC,
            trigger_l: 0x33,
            battery: 0x4B,
            reserved: 0,
        };
        let header =
            CustomHeader::new(commands::cmd::INPUT, 0, 12, 12).with_payload(&report.to_bytes());
        mock.push_response(&header, &report.to_bytes());

        let session = Session::new(&mock);
        let decoded = session
            .try_next_input(Duration::from_millis(200))
            .await
            .unwrap()
            .expect("a report is queued");
        assert_eq!(decoded, report);
    }

    #[tokio::test]
    async fn input_reader_returns_none_on_timeout() {
        let transport = StallingTransport {
            first: Mutex::new(None),
        };
        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));
        let result = session
            .try_next_input(Duration::from_millis(20))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn crc_valid_snapshot_is_reported_valid() {
        // 0x580C is the fixed point of ANSI_CRC_16_sh over an otherwise
        // zeroed 2768-byte buffer: storing it at +0x0C makes an in-place
        // recomputation return the same value. This is the only way the
        // official CRC rule can report "valid" without a previous CRC value.
        let mut config = Bt2Config::zeroed();
        config.as_mut_bytes()[0x0C..0x10].copy_from_slice(&0x580Cu32.to_le_bytes());

        let mock = MockTransport::new();
        push_config_chunks(&mock, config.as_bytes(), 1024);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.stored_crc, 0x580C);
        assert_eq!(snapshot.computed_crc, 0x580C);
        assert!(snapshot.crc_valid);
    }

    #[tokio::test]
    async fn crc_flip_is_reported_invalid() {
        let mut config = Bt2Config::zeroed();
        config.as_mut_bytes()[0x0C..0x10].copy_from_slice(&0x580Cu32.to_le_bytes());
        config.as_mut_bytes()[0] = 0x01;

        let mock = MockTransport::new();
        push_config_chunks(&mock, config.as_bytes(), 1024);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.stored_crc, 0x580C);
        assert_ne!(snapshot.computed_crc, snapshot.stored_crc);
        assert!(!snapshot.crc_valid);
    }
}
