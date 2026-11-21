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

/// Decoded firmware version response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// Raw response payload exactly as received (`4` bytes on `0x600F`).
    pub raw: Vec<u8>,
    /// Longest printable ASCII run in `raw`, trimmed.
    ///
    /// Empty when the payload contains no printable bytes. The official
    /// `getVersion` treats the payload as a little-endian `i32`, so this is
    /// usually empty for a binary version value.
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
    /// The `0x600F` request carries a 4-byte zero payload and declares
    /// `total_len = chunk_len = 4`; the device answers with a
    /// `cmd = 0x0004, subcmd = 0x000B` envelope whose `total_len` is 4. A
    /// payload-less acknowledgement (`chunk_len = total_len = 0`) is skipped
    /// and the read continues until the data response arrives or the timeout
    /// expires.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when no matching notification arrives within
    /// the configured per-frame timeout, [`Error::UnexpectedResponse`] for a
    /// mismatched envelope or a range outside the declared transfer, or a
    /// transport error.
    pub async fn read_version(&self) -> Result<VersionInfo, Error> {
        self.transport
            .send(&commands::read_version().encode_ble(PacketKind::App)?)
            .await?;

        let raw = self
            .read_transfer(
                commands::cmd::READ_VERSION,
                commands::VERSION_RESPONSE_LEN,
                self.timeout,
                self.timeout,
            )
            .await?;

        let text = longest_printable_run(&raw);
        Ok(VersionInfo { raw, text })
    }

    /// Read and validate the full 2768-byte configuration block.
    ///
    /// The controller returns **one chunk per request**. The session starts at
    /// `offset = 0`, sends a payload-less `ReadConfig` with
    /// `chunk_len = total_len = 0x0AD0`, copies the returned payload to the
    /// requested offset, advances by `payload.len()`, and repeats until all
    /// 2768 bytes are assembled. The observed chunk is 142 bytes
    /// (`0x8E`), so a full read takes 20 requests (19 × 142 + 70).
    ///
    /// Responses are accepted both as the observed `cmd 0x0004,
    /// subcmd 0x0002` envelope and as a direct `cmd 0x0002` frame. A
    /// payload-less acknowledgement (`chunk_len = 0`) and unrelated commands
    /// (for example the asynchronous `0x0012` "configuration allowed" notice)
    /// are skipped. A response `offset` equal to the requested offset is used
    /// as-is; `offset = 0` is treated as a sequential chunk and placed at the
    /// requested offset.
    ///
    /// The CRC is checked with the official in-place algorithm: the stored
    /// `+0x0C` value is compared with `ANSI_CRC_16_sh` over a copy of the
    /// received buffer, which still contains that stored value.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when the transfer does not complete within
    /// [`CONFIG_TOTAL_TIMEOUT`] or a chunk does not arrive within the
    /// configured per-frame timeout, [`Error::UnexpectedResponse`] when a
    /// chunk has a mismatched envelope, offset or range, or a transport error.
    pub async fn read_config_snapshot(&self) -> Result<ConfigSnapshot, Error> {
        let bytes = self
            .read_config_chunks(CONFIG_SIZE, self.timeout, CONFIG_TOTAL_TIMEOUT)
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

    /// Send one realtime input request (`cmd 0x0003`) without waiting for a
    /// report.
    ///
    /// This mirrors the official Android app's fire-and-forget
    /// `ReadKeysAndJoy` poll (about every 20 ms while the app is in the
    /// foreground). Notifications are consumed separately, for example with
    /// [`try_next_input`](Self::try_next_input) or the transport's `recv`.
    ///
    /// # Errors
    ///
    /// Returns a transport or protocol encoding error.
    pub async fn request_input(&self) -> Result<(), Error> {
        self.transport
            .send(&commands::read_input().encode_ble(PacketKind::App)?)
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
    /// Both response shapes are accepted:
    ///
    /// - the generic envelope observed on `0x600F`
    ///   (`cmd = 0x0004`, `subcmd = request cmd`, `total_len = expected_len`);
    /// - a legacy direct echo whose `cmd` equals the request command.
    ///
    /// A payload-less acknowledgement (`cmd = 0x0004`, `chunk_len = 0`,
    /// `total_len = 0`) is skipped. Unrelated asynchronous frames (input
    /// reports, `0x0012` notices) are also skipped. Envelope frames with a
    /// mismatched `subcmd` or `total_len`, and chunks whose range falls
    /// outside `expected_len`, are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when `total_timeout` expires or a frame does
    /// not arrive within `per_frame_timeout`, [`Error::UnexpectedResponse`] for
    /// a mismatched envelope or an out-of-range chunk, or a transport/protocol
    /// error.
    pub async fn read_transfer(
        &self,
        cmd: u16,
        expected_len: usize,
        per_frame_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        let mut buffer = TransferBuffer::new(expected_len);
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

            let Some((header, payload)) = Self::classify_response(&frame, cmd, expected_len)?
            else {
                continue;
            };
            buffer.insert(header.offset as usize, payload)?;
        }

        Ok(buffer.into_vec())
    }

    /// Read the full configuration block with one request per chunk.
    ///
    /// This is the ReadConfig-specific transfer loop. Each iteration sends
    /// `read_config_from(offset)` and waits for exactly one data response.
    /// The response payload is copied at the requested offset, and the offset
    /// advances by `payload.len()`. A response `offset` equal to the requested
    /// offset is used as-is; `offset = 0` is treated as the device's
    /// sequential-chunk convention and placed at the requested offset.
    ///
    /// `expected_len` must be [`CONFIG_SIZE`]. Use [`read_transfer`] for
    /// single-request transfers such as `ReadVersion`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when the overall deadline or a per-chunk
    /// wait expires, [`Error::UnexpectedResponse`] for a mismatched envelope,
    /// offset, `total_len` or range, or a transport/protocol error.
    pub async fn read_config_chunks(
        &self,
        expected_len: usize,
        per_frame_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        if expected_len != CONFIG_SIZE {
            return Err(Error::UnexpectedResponse(format!(
                "chunked ReadConfig expects {CONFIG_SIZE} bytes, got {expected_len}"
            )));
        }

        let mut buffer = TransferBuffer::new(expected_len);
        let mut offset = 0usize;
        let deadline = Instant::now() + total_timeout;

        while offset < expected_len {
            let request = commands::read_config_from(offset as u32)?;
            self.transport
                .send(&request.encode_ble_payloadless(PacketKind::App)?)
                .await?;

            loop {
                let now = Instant::now();
                if now >= deadline {
                    return Err(config_chunk_timeout(
                        offset,
                        buffer.received(),
                        expected_len,
                    ));
                }
                let wait = per_frame_timeout.min(deadline.saturating_duration_since(now));
                let frame = match tokio::time::timeout(wait, self.transport.recv()).await {
                    Ok(result) => result?,
                    Err(_) => {
                        return Err(config_chunk_timeout(
                            offset,
                            buffer.received(),
                            expected_len,
                        ))
                    }
                };

                let Some((header, payload)) =
                    Self::classify_response(&frame, commands::cmd::READ_CONFIG, expected_len)?
                else {
                    continue;
                };

                let response_offset = header.offset as usize;
                if response_offset >= expected_len {
                    return Err(out_of_range(response_offset, expected_len));
                }
                if response_offset != offset && response_offset != 0 {
                    return Err(Error::UnexpectedResponse(format!(
                        "config chunk offset 0x{response_offset:X} does not match requested offset 0x{offset:X}"
                    )));
                }

                let placement = if response_offset == 0 {
                    offset
                } else {
                    response_offset
                };
                // The unit returns a fixed 142-byte final chunk even when the
                // remaining transfer is shorter. Keep only the bytes inside
                // the 2768-byte block and ignore the trailing padding.
                let remaining = expected_len - placement;
                let copy_len = payload.len().min(remaining);
                buffer.insert(placement, &payload[..copy_len])?;
                offset = placement + copy_len;
                break;
            }
        }

        Ok(buffer.into_vec())
    }

    /// Classify one notification for a request.
    ///
    /// Returns `Ok(None)` for a payload-less acknowledgement or an unrelated
    /// command, and an error for a response envelope that names a different
    /// request or transfer size.
    fn classify_response(
        frame: &[u8],
        request_cmd: u16,
        expected_len: usize,
    ) -> Result<Option<(header::CustomHeader, &[u8])>, Error> {
        let (response, payload) = header::decode_app_response(frame)?;

        // Backward compatibility: older mocks and possible firmware revisions
        // answer with the request command directly.
        if response.cmd == request_cmd {
            return Ok(Some((response, payload)));
        }
        if response.cmd != commands::cmd::RESPONSE {
            return Ok(None);
        }
        if response.subcmd != request_cmd {
            return Err(Error::UnexpectedResponse(format!(
                "response envelope subcmd 0x{:04X} does not match request cmd 0x{request_cmd:04X}",
                response.subcmd
            )));
        }

        // Payload-less acknowledgement / "ready" frame. The device does not
        // carry data when chunk_len is zero; total_len may be 0 (ReadVersion
        // ACK) or the full transfer size (ReadConfig ready ACK). Skip it and
        // keep waiting for the data frame(s).
        if response.chunk_len == 0 {
            return Ok(None);
        }
        if response.total_len as usize != expected_len {
            return Err(Error::UnexpectedResponse(format!(
                "response envelope total_len {} does not match the expected {expected_len} bytes",
                response.total_len
            )));
        }
        Ok(Some((response, payload)))
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

fn config_chunk_timeout(offset: usize, received: usize, expected: usize) -> Error {
    Error::Timeout {
        operation: format!(
            "read config chunk at offset 0x{offset:X} (received {received} of {expected} bytes)"
        ),
    }
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

    fn envelope_header(
        request_cmd: u16,
        total_len: u32,
        offset: u32,
        chunk_len: u16,
    ) -> CustomHeader {
        CustomHeader::new(commands::cmd::RESPONSE, request_cmd, total_len, offset)
            .with_chunk_len(chunk_len)
    }

    fn push_config_chunks(mock: &MockTransport, bytes: &[u8], chunk_size: usize) {
        for (index, chunk) in bytes.chunks(chunk_size).enumerate() {
            let header = config_header((index * chunk_size) as u32, chunk.len() as u16);
            mock.push_response(&header, chunk);
        }
    }

    fn push_config_envelope_chunks(mock: &MockTransport, bytes: &[u8], chunk_size: usize) {
        for (index, chunk) in bytes.chunks(chunk_size).enumerate() {
            let header = envelope_header(
                commands::cmd::READ_CONFIG,
                CONFIG_SIZE as u32,
                (index * chunk_size) as u32,
                chunk.len() as u16,
            );
            mock.push_response(&header, chunk);
        }
    }

    /// The 16-byte acknowledgement captured from the FF13 unit when the old
    /// payload-less ReadVersion request (`chunk_len = 0`) was sent.
    const OBSERVED_VERSION_ACK: [u8; 16] = [
        0x04, 0x00, 0x0B, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];

    /// The 20-byte data response captured from the FF13 unit for the corrected
    /// 21-byte ReadVersion request. Payload `C8 00 00 00` is version 200.
    const OBSERVED_VERSION_DATA: [u8; 20] = [
        0x04, 0x00, 0x0B, 0x00, 0x04, 0x00, 0x3E, 0x44, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xC8, 0x00, 0x00, 0x00,
    ];

    /// The 16-byte "ready" response captured from the FF13 unit for a config
    /// read: `chunk_len = 0` but `total_len = 0x0AD0`.
    const OBSERVED_CONFIG_ACK: [u8; 16] = [
        0x04, 0x00, 0x02, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xD0, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];

    /// Exact 17-byte ReadConfig request. `chunk_len` and `total_len` both
    /// declare the requested transfer length (0x0AD0).
    const READ_CONFIG_REQUEST: [u8; 17] = [
        0x04, // BLE App prefix
        0x02, 0x00, // cmd = 0x0002
        0x00, 0x00, // subcmd
        0xD0, 0x0A, // chunk_len = 0x0AD0
        0x00, 0x00, // crc16
        0xD0, 0x0A, 0x00, 0x00, // total_len = 0x0AD0
        0x00, 0x00, 0x00, 0x00, // offset = 0
    ];

    /// Exact 21-byte ReadVersion request derived from `getVersionChannel`
    /// (0x2c7a0) and `Generate_CustomDataCmd` (0x27620).
    const READ_VERSION_REQUEST: [u8; 21] = [
        0x04, // BLE App prefix
        0x0B, 0x00, // cmd = 0x000B
        0x00, 0x00, // subcmd
        0x04, 0x00, // chunk_len = 4
        0x00, 0x24, // crc16 = 0x2400
        0x04, 0x00, 0x00, 0x00, // total_len = 4
        0x40, 0x70, 0x01, 0x01, // offset = 0x01017040
        0x00, 0x00, 0x00, 0x00, // gamepadVersion dummy payload
    ];

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
    async fn read_version_sends_exact_app_frame_and_decodes_envelope() {
        let mock = MockTransport::new();
        let response = envelope_header(
            commands::cmd::READ_VERSION,
            commands::VERSION_REQUEST_LEN,
            0,
            4,
        );
        mock.push_response(&response, &[0x01, 0x02, 0x03, 0x04]);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0], READ_VERSION_REQUEST);
        assert_eq!(version.raw, [0x01, 0x02, 0x03, 0x04]);
    }

    #[tokio::test]
    async fn read_version_skips_observed_ack_before_data() {
        let mock = MockTransport::new();
        mock.push_incoming(&OBSERVED_VERSION_ACK);
        let data = envelope_header(
            commands::cmd::READ_VERSION,
            commands::VERSION_REQUEST_LEN,
            0,
            4,
        );
        mock.push_response(&data, &[0x00, 0x01, 0x02, 0x03]);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();

        assert_eq!(version.raw, [0x00, 0x01, 0x02, 0x03]);
        assert_eq!(mock.sent_frames(), vec![READ_VERSION_REQUEST.to_vec()]);
    }

    #[tokio::test]
    async fn read_version_skips_ack_and_times_out_when_no_data_arrives() {
        let transport = StallingTransport {
            first: Mutex::new(Some(OBSERVED_VERSION_ACK.to_vec())),
        };
        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));
        let result = session.read_version().await;
        assert!(matches!(result, Err(Error::Timeout { .. })));
    }

    #[tokio::test]
    async fn legacy_direct_cmd_version_response_is_accepted() {
        let mock = MockTransport::new();
        let response =
            CustomHeader::new(commands::cmd::READ_VERSION, 0, 4, 0).with_payload(&[9, 8, 7, 6]);
        mock.push_response(&response, &[9, 8, 7, 6]);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();
        assert_eq!(version.raw, [9, 8, 7, 6]);
    }

    #[tokio::test]
    async fn version_envelope_with_wrong_subcmd_is_rejected() {
        let mock = MockTransport::new();
        let response = envelope_header(commands::cmd::ENTER_BOOT, 4, 0, 4);
        mock.push_response(&response, &[0; 4]);

        let session = Session::new(&mock);
        let result = session.read_version().await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("subcmd")
        ));
    }

    #[tokio::test]
    async fn version_envelope_with_wrong_total_len_is_rejected() {
        let mock = MockTransport::new();
        let response = envelope_header(commands::cmd::READ_VERSION, 0x30, 0, 4);
        mock.push_response(&response, &[0; 4]);

        let session = Session::new(&mock);
        let result = session.read_version().await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("total_len")
        ));
    }

    #[tokio::test]
    async fn read_version_decodes_observed_data_response() {
        let mock = MockTransport::new();
        mock.push_incoming(&OBSERVED_VERSION_DATA);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();

        assert_eq!(version.raw, [0xC8, 0x00, 0x00, 0x00]);
        assert_eq!(
            u32::from_le_bytes(version.raw.as_slice().try_into().unwrap()),
            200
        );
        assert!(version.text.is_empty());
    }

    #[tokio::test]
    async fn version_ready_frame_without_data_is_skipped_then_times_out() {
        let response = envelope_header(commands::cmd::READ_VERSION, 4, 0, 0);
        let transport = StallingTransport {
            first: Mutex::new(Some(response.to_bytes().to_vec())),
        };

        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));
        let result = session.read_version().await;
        assert!(matches!(result, Err(Error::Timeout { .. })));
    }

    #[tokio::test]
    async fn config_chunks_are_re_requested_at_advancing_offsets() {
        let config = sample_config();
        let mock = MockTransport::new();
        let bytes = config.as_bytes();
        let chunk_size = 142;
        let chunks: Vec<&[u8]> = bytes.chunks(chunk_size).collect();
        assert_eq!(chunks.len(), 20);
        assert_eq!(chunks.last().unwrap().len(), 70);

        for (index, chunk) in chunks.iter().enumerate() {
            if index == 7 {
                mock.push_incoming(&OBSERVED_CONFIG_ACK);
            }
            // The unit returns offset=0 for sequential chunks.
            let header = envelope_header(
                commands::cmd::READ_CONFIG,
                CONFIG_SIZE as u32,
                0,
                chunk.len() as u16,
            );
            mock.push_response(&header, chunk);
        }

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), bytes);

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), chunks.len());
        assert_eq!(sent[0], READ_CONFIG_REQUEST);
        let mut expected_offset = 0usize;
        for (index, frame) in sent.iter().enumerate() {
            assert_eq!(frame.len(), 17);
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.cmd, commands::cmd::READ_CONFIG);
            assert_eq!(header.chunk_len, CONFIG_SIZE as u16);
            assert_eq!(header.total_len, CONFIG_SIZE as u32);
            assert_eq!(header.offset as usize, expected_offset);
            expected_offset += chunks[index].len();
        }
        assert_eq!(expected_offset, CONFIG_SIZE);
    }

    #[tokio::test]
    async fn config_chunks_accept_matching_response_offsets() {
        let config = sample_config();
        let mock = MockTransport::new();
        let chunk_size = 142;
        for (index, chunk) in config.as_bytes().chunks(chunk_size).enumerate() {
            let offset = (index * chunk_size) as u32;
            let header = envelope_header(
                commands::cmd::READ_CONFIG,
                CONFIG_SIZE as u32,
                offset,
                chunk.len() as u16,
            );
            mock.push_response(&header, chunk);
        }

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
    }

    #[tokio::test]
    async fn config_envelope_chunks_are_assembled() {
        let config = sample_config();
        let mock = MockTransport::new();
        push_config_envelope_chunks(&mock, config.as_bytes(), 692);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
        assert_eq!(snapshot.config.profile_name(Slot::S2), "Fight");
    }

    #[tokio::test]
    async fn config_envelope_skips_observed_ready_ack_and_assembles() {
        let config = sample_config();
        let mock = MockTransport::new();
        mock.push_incoming(&OBSERVED_CONFIG_ACK);
        push_config_envelope_chunks(&mock, config.as_bytes(), 692);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 4);
        for (index, frame) in sent.iter().enumerate() {
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.offset as usize, index * 692);
            assert_eq!(header.chunk_len, CONFIG_SIZE as u16);
            assert_eq!(header.total_len, CONFIG_SIZE as u32);
        }
    }

    #[tokio::test]
    async fn config_ack_is_skipped_regardless_of_total_len() {
        let config = sample_config();
        let mock = MockTransport::new();
        let odd_ack = CustomHeader::new(
            commands::cmd::RESPONSE,
            commands::cmd::READ_CONFIG,
            0x1234,
            0,
        );
        mock.push_response(&odd_ack, &[]);
        push_config_envelope_chunks(&mock, config.as_bytes(), 692);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
    }

    #[tokio::test]
    async fn config_envelope_with_wrong_subcmd_is_rejected() {
        let mock = MockTransport::new();
        let header = envelope_header(commands::cmd::READ_VERSION, CONFIG_SIZE as u32, 0, 8);
        mock.push_response(&header, &[0; 8]);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("subcmd")
        ));
    }

    #[tokio::test]
    async fn config_envelope_with_wrong_total_len_is_rejected() {
        let mock = MockTransport::new();
        let header = envelope_header(commands::cmd::READ_CONFIG, 0x100, 0, 8);
        mock.push_response(&header, &[0; 8]);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("total_len")
        ));
    }

    #[tokio::test]
    async fn config_envelope_out_of_range_offset_is_rejected() {
        let mock = MockTransport::new();
        let header = envelope_header(
            commands::cmd::READ_CONFIG,
            CONFIG_SIZE as u32,
            CONFIG_SIZE as u32 - 8,
            16,
        );
        mock.push_response(&header, &[0; 16]);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(matches!(result, Err(Error::UnexpectedResponse(_))));
    }

    #[tokio::test]
    async fn config_chunk_with_mismatched_nonzero_offset_is_rejected() {
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();
        push_config_envelope_chunks(&mock, &bytes[..426], 142);
        let bad = envelope_header(commands::cmd::READ_CONFIG, CONFIG_SIZE as u32, 100, 8);
        mock.push_response(&bad, &bytes[426..434]);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("does not match requested offset")
        ));
    }

    #[tokio::test]
    async fn config_final_chunk_padding_is_truncated() {
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();
        push_config_envelope_chunks(&mock, &bytes[..2698], 142);

        // The unit returns a fixed 142-byte final chunk: 70 valid bytes plus
        // 72 bytes of padding past the end of the 2768-byte block.
        let mut final_payload = bytes[2698..].to_vec();
        final_payload.extend_from_slice(&[0xAA; 72]);
        assert_eq!(final_payload.len(), 142);
        let header = envelope_header(commands::cmd::READ_CONFIG, CONFIG_SIZE as u32, 2698, 142);
        mock.push_response(&header, &final_payload);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), bytes);
        assert_ne!(&snapshot.config.as_bytes()[2768 - 8..], &[0xAA; 8]);
    }

    #[tokio::test]
    async fn config_final_chunk_with_exact_remaining_length_succeeds() {
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();
        push_config_envelope_chunks(&mock, &bytes[..2698], 142);
        let header = envelope_header(commands::cmd::READ_CONFIG, CONFIG_SIZE as u32, 2698, 70);
        mock.push_response(&header, &bytes[2698..]);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), bytes);
    }

    #[tokio::test]
    async fn config_response_offset_at_or_beyond_end_is_rejected() {
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();
        push_config_envelope_chunks(&mock, &bytes[..2698], 142);
        let bad = envelope_header(
            commands::cmd::READ_CONFIG,
            CONFIG_SIZE as u32,
            CONFIG_SIZE as u32,
            142,
        );
        mock.push_response(&bad, &[0u8; 142]);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("outside")
        ));
    }

    #[tokio::test]
    async fn config_chunk_with_wrong_subcmd_mid_transfer_is_rejected() {
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();
        push_config_envelope_chunks(&mock, &bytes[..426], 142);
        let bad = envelope_header(commands::cmd::READ_VERSION, CONFIG_SIZE as u32, 0, 8);
        mock.push_response(&bad, &[0u8; 8]);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("subcmd")
        ));
    }

    #[tokio::test]
    async fn config_chunk_with_wrong_total_len_mid_transfer_is_rejected() {
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();
        push_config_envelope_chunks(&mock, &bytes[..426], 142);
        let bad = envelope_header(commands::cmd::READ_CONFIG, 0x100, 0, 8);
        mock.push_response(&bad, &[0u8; 8]);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message)) if message.contains("total_len")
        ));
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
