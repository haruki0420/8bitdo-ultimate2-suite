//! High-level, read-only App Mode operations on top of a [`Transport`].
//!
//! [`Session`] owns the framing rules: it encodes requests for the BLE App
//! channel (`0x04` prefix), waits for notifications, assembles split
//! transfers by header offset, and decodes the 12-byte input report.
//!
//! Every operation in this module is read-only except
//! [`Session::set_report_enable`], which toggles the realtime input stream.
//!
//! Reads fail closed: a stale-frame drain that cannot finish within its
//! budget refuses the read ([`Error::UnexpectedResponse`] with
//! `drain budget exhausted`) instead of risking a stale frame being mistaken
//! for the new response.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::Duration;

use eightbitdo_protocol::commands;
use eightbitdo_protocol::config_bt2::{Bt2Config, CONFIG_SIZE};
use eightbitdo_protocol::crc::packet_crc16;
use eightbitdo_protocol::header::{self, PacketKind};
use eightbitdo_protocol::input_report::InputReport;
use tokio::time::Instant;

use crate::transport::Transport;
use crate::Error;

/// Default per-frame wait used when no explicit timeout is configured.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Overall deadline for one 2768-byte configuration read.
pub const CONFIG_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum number of stale frames discarded in a single resync drain.
const MAX_DRAIN_FRAMES: usize = 64;

/// Maximum number of frames consumed by one [`Session::try_next_input`] call.
///
/// This is also the upper bound on how many non-input frames it skips before
/// returning `Ok(None)`, so a caller looping over input reports always
/// regains control.
pub const MAX_INPUT_DRAIN_FRAMES: usize = 16;

/// Malformed frames [`Session::try_next_input`] skips before it gives up.
///
/// One corrupt notification must not end a keepalive or monitor loop, so
/// decode failures are skipped; this many *in a row* means the link is
/// delivering garbage, and the call fails with a protocol error instead of
/// spinning until the caller's timeout.
pub const MAX_CONSECUTIVE_DECODE_FAILURES: usize = 8;

/// Overall time budget for discarding stale frames during resync.
const MAX_DRAIN_DURATION: Duration = Duration::from_millis(50);

/// Idle gap used to decide that no more stale frames are queued.
const DRAIN_FRAME_WAIT: Duration = Duration::from_millis(1);

/// Upper bound accepted for a single wait ([`Session::try_next_input`], the
/// per-frame and total timeouts). It mirrors the [`std::time::Instant`] range
/// so every computed deadline stays representable; anything larger is
/// rejected with [`Error::InvalidTimeout`] instead of panicking.
pub const MAX_WAIT: Duration = Duration::from_secs(3600 * 24 * 365 * 30);

/// Compute `Instant::now() + duration` with a range check.
///
/// [`tokio::time::Instant::add`] panics on overflow, and the CLI accepts
/// user-supplied durations (`--scan-timeout`, `--timeout`, `--hold`), so every
/// deadline goes through this helper.
fn deadline_after(duration: Duration) -> Result<Instant, Error> {
    if duration > MAX_WAIT {
        return Err(Error::InvalidTimeout(duration));
    }
    Instant::now()
        .checked_add(duration)
        .ok_or(Error::InvalidTimeout(duration))
}

/// Bounds applied to one stale-frame drain ([`Session::drain_pending`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DrainPolicy {
    /// Maximum number of frames discarded in one drain.
    max_frames: usize,
    /// Maximum wall-clock time spent in one drain.
    max_duration: Duration,
}

impl DrainPolicy {
    /// The bounds used by automatic resync: enough to clear the largest
    /// burst a full ReadConfig can leave behind, still bounded.
    const RESYNC: Self = Self {
        max_frames: MAX_DRAIN_FRAMES,
        max_duration: MAX_DRAIN_DURATION,
    };

    /// A small drain used by read paths that must not consume a response
    /// that is already queued for their own request.
    const OPPORTUNISTIC: Self = Self {
        max_frames: 1,
        max_duration: Duration::from_millis(1),
    };
}

/// Result of one bounded stale-frame drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainOutcome {
    /// The transport was idle within the current budget, so the queue is
    /// empty and the next read starts from a clean state.
    Quiesced,
    /// The frame or time budget ran out while frames could still arrive;
    /// stale frames may remain queued.
    Exhausted,
}

/// Marks a session for resync when a read returns an error or its future is
/// dropped before completion, and records what the session has in flight.
///
/// Dropping a **read** leaves its response in flight, so the next read must
/// drain stale frames first. Dropping an **input poll** leaves only a realtime
/// report (or nothing) behind, which the input path always skips, so it needs
/// no resync.
///
/// This is the whole judgement: a front end never has to tell the session
/// "that drop was mine", which is what allowed a stale frame to be accepted by
/// the next read.
struct ResyncGuard<'a> {
    needs_resync: &'a AtomicBool,
    in_flight: &'a AtomicU8,
    kind: InFlight,
    /// True once the operation finished successfully (`disarm`).
    disarmed: bool,
}

impl<'a> ResyncGuard<'a> {
    fn new(needs_resync: &'a AtomicBool, in_flight: &'a AtomicU8, kind: InFlight) -> Self {
        let previous = in_flight.swap(kind as u8, Ordering::SeqCst);
        debug_assert_eq!(
            previous,
            InFlight::None as u8,
            "operations on one session must not overlap"
        );
        Self {
            needs_resync,
            in_flight,
            kind,
            disarmed: false,
        }
    }

    /// The operation failed: its response may still arrive.
    fn fail(&mut self) {
        self.needs_resync.store(true, Ordering::SeqCst);
    }

    /// The operation completed successfully; nothing is left on the wire.
    fn disarm(mut self) {
        self.disarmed = true;
        self.clear();
    }

    fn clear(&self) {
        self.in_flight.store(InFlight::None as u8, Ordering::SeqCst);
    }
}

impl Drop for ResyncGuard<'_> {
    fn drop(&mut self) {
        self.clear();
        // A read whose future is dropped mid-flight leaves a response that the
        // next read must not mistake for its own. A successfully completed read
        // was disarmed, which took the early return below. An input poll never
        // needs the resync path.
        if self.kind != InFlight::Read || self.disarmed {
            return;
        }
        self.needs_resync.store(true, Ordering::SeqCst);
    }
}

/// The operation a [`Session`] currently has on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InFlight {
    /// No operation is in flight.
    None,
    /// A read (`ReadVersion`, `ReadConfig`, `read_transfer`) is waiting for a
    /// data response.
    Read,
    /// A realtime input poll (`0x0003` / `0x0007`) is waiting for a report.
    Input,
}

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
///
/// # Single-flight contract
///
/// Operations must not overlap: await each one to completion (or drop its
/// future) before starting the next. The session tracks the in-flight
/// operation and the resync flag in atomics, and a second concurrent
/// operation would corrupt that bookkeeping. The CLI serializes access with
/// a mutex; debug builds assert on overlap, but release builds have no guard,
/// so front ends (including a future UniFFI or GUI layer) must enforce the
/// contract themselves.
pub struct Session<T: Transport> {
    transport: T,
    timeout: Duration,
    needs_resync: AtomicBool,
    in_flight: AtomicU8,
    skipped_frames: AtomicU64,
    consecutive_decode_failures: AtomicU32,
    recovered_frame_runs: AtomicU64,
}

impl<T: Transport> Session<T> {
    /// Wrap a connected transport with the default per-frame timeout.
    #[must_use]
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            timeout: DEFAULT_TIMEOUT,
            needs_resync: AtomicBool::new(false),
            in_flight: AtomicU8::new(InFlight::None as u8),
            skipped_frames: AtomicU64::new(0),
            consecutive_decode_failures: AtomicU32::new(0),
            recovered_frame_runs: AtomicU64::new(0),
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

    /// Drain any stale frames if a previous read failed or was cancelled.
    ///
    /// Callers using [`Session::read_transfer`] directly after sending a
    /// custom request should invoke this **before** sending the request so
    /// stale frames from an earlier failed read are discarded without
    /// consuming the response to the new request.
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedResponse`] (message contains `drain budget
    /// exhausted`) when stale frames could not all be discarded within the
    /// drain budget, or the transport error that ended the drain.
    pub async fn resync(&self) -> Result<(), Error> {
        self.prepare_read().await
    }

    /// Run a read body, recording what is in flight and whether it failed.
    ///
    /// A drop (Ctrl-C, `monitor stop`) leaves `needs_resync` false, but the
    /// guard's own `Drop` bumps it back to true for reads: an abandoned read
    /// always leaves a response behind.
    async fn run_read<F, O>(&self, read: F) -> Result<O, Error>
    where
        F: Future<Output = Result<O, Error>>,
    {
        let mut guard = ResyncGuard::new(&self.needs_resync, &self.in_flight, InFlight::Read);
        let output = read.await;
        if output.is_err() {
            guard.fail();
        } else {
            guard.disarm();
        }
        output
    }

    /// Clear the resync flag, failing closed when the drain budget runs out.
    ///
    /// If stale frames may still be queued, the flag stays set so the next
    /// read retries the drain, and the current read is refused instead of
    /// risking a stale frame being mistaken for its response.
    async fn prepare_read(&self) -> Result<(), Error> {
        if self.needs_resync.load(Ordering::SeqCst) {
            match self.drain_pending(DrainPolicy::RESYNC).await {
                Ok(DrainOutcome::Quiesced) => {
                    self.needs_resync.store(false, Ordering::SeqCst);
                }
                Ok(DrainOutcome::Exhausted) => {
                    return Err(Error::UnexpectedResponse(
                        "drain budget exhausted while discarding stale frames; \
                         the read was refused so a stale frame cannot be used as its response"
                            .to_string(),
                    ))
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Discard frames left over from a timed-out or failed read.
    ///
    /// Bounded by [`DrainPolicy::max_frames`] and [`DrainPolicy::max_duration`]
    /// so a noisy transport cannot livelock the session. A transport error or
    /// disconnect also stops the drain.
    async fn drain_pending(&self, policy: DrainPolicy) -> Result<DrainOutcome, Error> {
        let deadline = deadline_after(policy.max_duration)?;
        for _ in 0..policy.max_frames {
            let now = Instant::now();
            if now >= deadline {
                return Ok(DrainOutcome::Exhausted);
            }
            let wait = DRAIN_FRAME_WAIT.min(deadline.saturating_duration_since(now));
            match tokio::time::timeout(wait, self.transport.recv()).await {
                // Idle gap: nothing more is queued right now.
                Err(_) => return Ok(DrainOutcome::Quiesced),
                Ok(Ok(_frame)) => {}
                // The stale transport is gone; reads will fail on their own.
                Ok(Err(Error::Disconnected)) => return Ok(DrainOutcome::Quiesced),
                Ok(Err(error)) => return Err(error),
            }
        }
        Ok(DrainOutcome::Exhausted)
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
    /// mismatched envelope, `total_len` or `crc16`, or a range outside the
    /// declared transfer, or a transport error.
    pub async fn read_version(&self) -> Result<VersionInfo, Error> {
        self.prepare_read().await?;
        let raw = self
            .run_read(async {
                self.transport
                    .send(&commands::read_version().encode_ble(PacketKind::App)?)
                    .await?;
                self.read_transfer_inner(
                    commands::cmd::READ_VERSION,
                    commands::VERSION_RESPONSE_LEN,
                    self.timeout,
                    self.timeout,
                )
                .await
            })
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
    /// are skipped. The first chunk may report `offset = 0` (the unit's
    /// sequential-chunk convention) and is placed at the requested offset;
    /// every later chunk must name the requested offset exactly. A chunk that
    /// overlaps bytes already received fails the read.
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
    /// chunk has a mismatched envelope, `total_len`, `crc16`, offset or range,
    /// or a transport error.
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
    /// Accepts both a direct `cmd=0x0003` frame and the `0x600F` envelope
    /// (`cmd=0x0004, subcmd=0x0003`). Other frames are skipped. A timeout
    /// returns `Ok(None)`; a closed transport returns [`Error::Disconnected`].
    ///
    /// A frame that does not decode as an App Mode response is skipped and
    /// counted in [`skipped_frames`](Self::skipped_frames); one corrupt
    /// notification must not stop a keepalive or monitor loop. Only
    /// [`MAX_CONSECUTIVE_DECODE_FAILURES`] malformed frames without a usable
    /// frame or a normal idle return in between end the call with a protocol
    /// error, so a link that keeps delivering garbage still fails instead of
    /// spinning, while sparse corruption does not accumulate across calls.
    ///
    /// The call is bounded in three ways so a caller that drains the input
    /// queue in a loop cannot hold the session lock forever: the `wait`
    /// budget, at most [`MAX_INPUT_DRAIN_FRAMES`] frames consumed per call
    /// (so at most that many non-input frames are skipped), and one final
    /// small stale-frame drain when the budget ends without a report. The
    /// whole call therefore settles within roughly `wait + 1 ms`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidTimeout`] when `wait` exceeds [`MAX_WAIT`],
    /// [`Error::Disconnected`] when the transport closes, a protocol error
    /// after [`MAX_CONSECUTIVE_DECODE_FAILURES`] malformed frames without a
    /// usable frame or idle return in between, or another transport error.
    /// Malformed frames below that limit are skipped, not returned.
    pub async fn try_next_input(&self, wait: Duration) -> Result<Option<InputReport>, Error> {
        // An abandoned input poll leaves at most one realtime report behind,
        // which every read path skips, so it never marks the session for
        // resync (see `ResyncGuard`).
        let _guard = ResyncGuard::new(&self.needs_resync, &self.in_flight, InFlight::Input);
        let deadline = deadline_after(wait)?;
        for _ in 0..MAX_INPUT_DRAIN_FRAMES {
            let now = Instant::now();
            if now >= deadline {
                self.drain_pending(DrainPolicy::OPPORTUNISTIC).await?;
                self.clear_decode_failures();
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
                Err(_) => {
                    self.drain_pending(DrainPolicy::OPPORTUNISTIC).await?;
                    self.clear_decode_failures();
                    return Ok(None);
                }
            };

            let (header, payload) = match header::decode_app_response(&frame) {
                Ok(decoded) => decoded,
                Err(error) => {
                    if self.record_decode_failure() {
                        return Err(Error::Protocol(error));
                    }
                    continue;
                }
            };
            let is_input = header.cmd == commands::cmd::INPUT
                || (header.cmd == commands::cmd::RESPONSE && header.subcmd == commands::cmd::INPUT);
            if !is_input || header.chunk_len == 0 {
                // A decodable frame that is not an input report (notice, ACK)
                // proves the link is delivering valid frames, so it ends the
                // malformed run like an idle return does.
                self.clear_decode_failures();
                continue;
            }
            let Ok(report) = InputReport::from_bytes(payload) else {
                if self.record_decode_failure() {
                    return Err(Error::Protocol(eightbitdo_protocol::Error::InvalidLength {
                        expected: eightbitdo_protocol::input_report::INPUT_REPORT_SIZE,
                        actual: payload.len(),
                    }));
                }
                continue;
            };
            if self.consecutive_decode_failures.swap(0, Ordering::Relaxed) > 0 {
                self.recovered_frame_runs.fetch_add(1, Ordering::Relaxed);
            }
            return Ok(Some(report));
        }

        // Frame budget spent: discard at most one more queued frame so the
        // next call starts with a fresh frame instead of the backlog.
        self.drain_pending(DrainPolicy::OPPORTUNISTIC).await?;
        self.clear_decode_failures();
        Ok(None)
    }

    /// Count one malformed input-path frame; true when the run is too long.
    ///
    /// A usable frame (input report, notice, ACK) and a normal idle return
    /// (`Ok(None)`) both clear the run. The load-bearing signal is "malformed
    /// frames since the last usable frame or idle gap", so sparse corruption
    /// spread over many calls never adds up to a degraded session, while one
    /// burst within a single call still fails after
    /// [`MAX_CONSECUTIVE_DECODE_FAILURES`].
    fn record_decode_failure(&self) -> bool {
        self.skipped_frames.fetch_add(1, Ordering::Relaxed);
        let failures = self
            .consecutive_decode_failures
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        failures >= MAX_CONSECUTIVE_DECODE_FAILURES as u32
    }

    /// End the current malformed-frame run after a usable frame or an idle
    /// return.
    fn clear_decode_failures(&self) {
        self.consecutive_decode_failures.store(0, Ordering::Relaxed);
    }

    /// Number of incoming frames this session skipped because they did not
    /// decode as an App Mode response or as a 12-byte input report.
    ///
    /// Counted by [`try_next_input`](Self::try_next_input). A small value is
    /// ordinary noise from a link that also carries non-App data; an
    /// increasing value means the link (or the peer) keeps sending garbage.
    #[must_use]
    pub fn skipped_frames(&self) -> u64 {
        self.skipped_frames.load(Ordering::Relaxed)
    }

    /// Number of malformed-frame runs that ended with a decoded input report.
    ///
    /// Complements [`skipped_frames`](Self::skipped_frames): a non-zero value
    /// proves the input stream recovered, which is what the skip rule is for.
    #[must_use]
    pub fn recovered_frame_runs(&self) -> u64 {
        self.recovered_frame_runs.load(Ordering::Relaxed)
    }

    /// Read a transfer of exactly `expected_len` bytes for `cmd`.
    ///
    /// Callers are expected to have already sent the request (calling
    /// [`Session::resync`] beforehand if needed). This method does not drain
    /// pending notifications before reading, so a response already queued for
    /// that request is never discarded.
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
        self.run_read(self.read_transfer_inner(cmd, expected_len, per_frame_timeout, total_timeout))
            .await
    }

    async fn read_transfer_inner(
        &self,
        cmd: u16,
        expected_len: usize,
        per_frame_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        let mut buffer = TransferBuffer::new(expected_len);
        let deadline = deadline_after(total_timeout)?;

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
    /// advances by `payload.len()`. `offset = 0` is accepted only for the
    /// first chunk, where it is the device's sequential-chunk convention;
    /// every later chunk must echo the requested offset, so
    /// a re-sent first chunk fails the read instead of shifting the block.
    /// Chunks that overlap bytes already received are rejected.
    ///
    /// `expected_len` must be [`CONFIG_SIZE`]. Use
    /// [`read_transfer`](Self::read_transfer) for single-request transfers
    /// such as `ReadVersion`.
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
        self.prepare_read().await?;
        self.run_read(self.read_config_chunks_inner(expected_len, per_frame_timeout, total_timeout))
            .await
    }

    async fn read_config_chunks_inner(
        &self,
        expected_len: usize,
        per_frame_timeout: Duration,
        total_timeout: Duration,
    ) -> Result<Vec<u8>, Error> {
        let mut buffer = TransferBuffer::new(expected_len);
        let mut offset = 0usize;
        let deadline = deadline_after(total_timeout)?;

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

                // The unit reports offset=0 only for the first chunk and
                // echoes the requested offset for every later chunk. A chunk
                // after the first that still names offset 0 can only be a
                // re-send of the first chunk: treating it as "the chunk I
                // asked for" would splice it at the requested offset, shift
                // every later chunk by one payload length and still fill the
                // buffer.
                let sequential_first = offset == 0 && response_offset == 0;
                if response_offset != offset && !sequential_first {
                    return Err(Error::UnexpectedResponse(format!(
                        "config chunk response offset 0x{response_offset:X} does not match requested offset 0x{offset:X}"
                    )));
                }

                let placement = offset;
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
        // answer with the request command directly. A payload-less direct ACK
        // must be skipped like the envelope form; otherwise the caller sees a
        // zero-length chunk and re-requests the same offset forever.
        if response.cmd == request_cmd {
            if response.chunk_len == 0 {
                return Ok(None);
            }
            Self::verify_response_crc(&response, payload)?;
            // A direct data frame declares the same transfer length as the
            // envelope form. Accept the observed `total_len = 0` convention of
            // direct echoes, but reject any other mismatch so a response for
            // a different transfer cannot be spliced into this one.
            if response.total_len != 0 && response.total_len as usize != expected_len {
                return Err(Error::UnexpectedResponse(format!(
                    "direct response total_len {} does not match the expected {expected_len} bytes",
                    response.total_len
                )));
            }
            return Ok(Some((response, payload)));
        }
        if response.cmd != commands::cmd::RESPONSE {
            return Ok(None);
        }

        // Input reports may arrive while a read is in progress because the
        // session enables notifications. They are unrelated to non-input
        // requests.
        if response.subcmd == commands::cmd::INPUT && request_cmd != commands::cmd::INPUT {
            return Ok(None);
        }

        // Payload-less acknowledgement / "ready" frame. Check this before
        // comparing subcmd so 0x0007 ACKs and other control ACKs are skipped.
        if response.chunk_len == 0 {
            return Ok(None);
        }

        if response.subcmd != request_cmd {
            return Err(Error::UnexpectedResponse(format!(
                "response envelope subcmd 0x{:04X} does not match request cmd 0x{request_cmd:04X}",
                response.subcmd
            )));
        }
        if response.total_len as usize != expected_len {
            return Err(Error::UnexpectedResponse(format!(
                "response envelope total_len {} does not match the expected {expected_len} bytes",
                response.total_len
            )));
        }
        Self::verify_response_crc(&response, payload)?;
        Ok(Some((response, payload)))
    }

    /// Check `crc16` on a data response when the device supplied one.
    ///
    /// `0x600F` data responses carry the packet CRC-16/MODBUS (poly `0xA001`,
    /// init `0xFFFF`) of their payload, e.g. `C8 00 00 00` → `0x443E`
    /// (`docs/PROTOCOL_SPEC.md` §4.3.1). `crc16 = 0` means "unused" and is
    /// accepted without checking, matching [`CustomHeader::new`] and the
    /// intentionally unchecked `0xFFFF` ACKs.
    fn verify_response_crc(response: &header::CustomHeader, payload: &[u8]) -> Result<(), Error> {
        if response.crc16 == 0 {
            return Ok(());
        }
        let computed = packet_crc16(payload);
        if computed != response.crc16 {
            return Err(Error::UnexpectedResponse(format!(
                "response crc16 0x{:04X} does not match the computed 0x{computed:04X} over {} payload bytes",
                response.crc16,
                payload.len()
            )));
        }
        Ok(())
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

        // A chunk that overlaps bytes already received is a re-send of an
        // older response (or device misbehaviour), not new data. Accepting it
        // silently would splice stale bytes into the transfer, so the whole
        // read fails instead.
        if let Some(position) = (offset..end).find(|&position| self.filled[position]) {
            return Err(Error::UnexpectedResponse(format!(
                "chunk at offset 0x{offset:X} overlaps bytes already received at 0x{position:X}"
            )));
        }

        self.data[offset..end].copy_from_slice(chunk);
        self.filled[offset..end].fill(true);
        self.received += chunk.len();
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
    use std::collections::VecDeque;
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

    /// `offset` values captured from the FF13 unit in the 20 responses of one
    /// full ReadConfig.
    ///
    /// The first chunk reports 0 (the unit's sequential-chunk convention) and
    /// every later chunk echoes the requested offset. Each chunk carries 142
    /// bytes, including the final one (70 valid bytes plus 72 bytes of
    /// padding past the end of the 2768-byte block).
    const OBSERVED_CONFIG_OFFSETS: [u32; 20] = [
        0, 142, 284, 426, 568, 710, 852, 994, 1136, 1278, 1420, 1562, 1704, 1846, 1988, 2130, 2272,
        2414, 2556, 2698,
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
    async fn read_version_skips_input_and_report_enable_ack() {
        let mock = MockTransport::new();
        let input = envelope_header(commands::cmd::INPUT, 0x0C, 0x0C, 12);
        mock.push_response(&input, &[0u8; 12]);
        let enable_ack =
            CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::REPORT_ENABLE, 0, 0);
        mock.push_response(&enable_ack, &[]);
        let data = envelope_header(
            commands::cmd::READ_VERSION,
            commands::VERSION_REQUEST_LEN,
            0,
            4,
        );
        mock.push_response(&data, &[0x11, 0x22, 0x33, 0x44]);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();
        assert_eq!(version.raw, [0x11, 0x22, 0x33, 0x44]);
    }

    #[tokio::test]
    async fn config_chunks_skip_interleaved_input_reports() {
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();
        for (index, chunk) in bytes.chunks(142).enumerate() {
            if index == 5 {
                let input = envelope_header(commands::cmd::INPUT, 0x0C, 0x0C, 12);
                mock.push_response(&input, &[0u8; 12]);
            }
            let offset = (index * 142) as u32;
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
        assert_eq!(snapshot.config.as_bytes(), bytes);
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
            // The unit reports offset=0 only for the first chunk; every later
            // chunk echoes the requested offset. A repeated offset=0 is a
            // stale re-send and is rejected (see
            // `resent_first_config_chunk_is_rejected_instead_of_shifting_the_block`).
            let offset = if index == 0 {
                0
            } else {
                (index * chunk_size) as u32
            };
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
    async fn read_config_snapshot_accepts_the_observed_hardware_offset_sequence() {
        // Golden test for the real FF13 sequence: the first response reports
        // offset 0 and every later response echoes the requested offset.
        // Payloads are 142 bytes even for the final chunk, whose last 72 bytes
        // are padding past the 2768-byte block.
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();

        for (index, &offset) in OBSERVED_CONFIG_OFFSETS.iter().enumerate() {
            if index > 0 {
                assert_eq!(
                    offset as usize,
                    index * 142,
                    "the observed offsets must be contiguous 142-byte chunks"
                );
            }
            let start = offset as usize;
            let valid = (CONFIG_SIZE - start).min(142);
            let mut payload = vec![0xAAu8; 142];
            payload[..valid].copy_from_slice(&bytes[start..start + valid]);

            let mut header =
                envelope_header(commands::cmd::READ_CONFIG, CONFIG_SIZE as u32, offset, 142);
            // The final chunk's CRC covers all 142 bytes, padding included:
            // the implementation verifies the CRC over the whole payload
            // before trimming the 72 bytes outside the block.
            header.crc16 = packet_crc16(&payload);
            mock.push_response(&header, &payload);
        }

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();

        assert_eq!(snapshot.config.as_bytes(), bytes);
        let sent = mock.sent_frames();
        assert_eq!(sent.len(), OBSERVED_CONFIG_OFFSETS.len());
        for (index, frame) in sent.iter().enumerate() {
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.cmd, commands::cmd::READ_CONFIG);
            assert_eq!(header.offset, OBSERVED_CONFIG_OFFSETS[index]);
            assert_eq!(header.chunk_len, CONFIG_SIZE as u16);
            assert_eq!(header.total_len, CONFIG_SIZE as u32);
        }
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
    async fn direct_config_ack_with_zero_chunk_len_is_skipped() {
        let config = sample_config();
        let mock = MockTransport::new();
        // Some units answer the first ReadConfig with a direct cmd=0x0002
        // frame instead of the 0x0004 envelope. Its chunk_len is 0, so it must
        // not be treated as a zero-byte chunk that stalls the offset.
        mock.push_incoming(&config_header(0, 0).to_bytes());
        push_config_chunks(&mock, config.as_bytes(), 142);

        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
        assert_eq!(mock.sent_frames().len(), 20);
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
    async fn resent_first_config_chunk_is_rejected_instead_of_shifting_the_block() {
        // B1 regression: after the first chunk the device must echo the
        // requested offset. A re-sent first chunk (offset=0, 142 bytes) must
        // not be spliced at offset 142; that would shift all later bytes by
        // 142 and still produce a complete 2768-byte buffer that the CLI would
        // save as a "valid" backup.
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();

        let first = envelope_header(commands::cmd::READ_CONFIG, CONFIG_SIZE as u32, 0, 142);
        mock.push_response(&first, &bytes[..142]);
        // The stale re-send of chunk 0 arrives for the offset=0x8E request.
        let resent = envelope_header(commands::cmd::READ_CONFIG, CONFIG_SIZE as u32, 0, 142);
        mock.push_response(&resent, &bytes[..142]);
        // Nothing later is queued: the read must fail on this frame instead of
        // advancing and shifting the rest of the block.

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        let Err(Error::UnexpectedResponse(message)) = result else {
            panic!("a re-sent first chunk must fail the read, got {result:?}");
        };
        assert!(
            message.contains("requested offset 0x8E") && message.contains("response offset 0x0"),
            "the error must name both the requested and the response offset: {message}"
        );
        assert_eq!(
            mock.sent_frames().len(),
            2,
            "the read must fail on the re-sent first chunk, not continue with later offsets"
        );
    }

    #[tokio::test]
    async fn duplicate_config_chunk_is_rejected_as_an_overlap() {
        // The ReadConfig loop rejects a stale re-send at the offset check
        // before it can reach the buffer; a single-request transfer with two
        // chunks at the same offset reaches `TransferBuffer::insert` instead,
        // which must reject the overlap rather than silently ignore it.
        let mock = MockTransport::new();
        let header = envelope_header(
            commands::cmd::READ_VERSION,
            commands::VERSION_REQUEST_LEN,
            0,
            2,
        );
        mock.push_response(&header, &[0x01, 0x02]);
        mock.push_response(&header, &[0x0A, 0x0B]);

        let session = Session::new(&mock);
        let result = session
            .read_transfer(
                commands::cmd::READ_VERSION,
                commands::VERSION_RESPONSE_LEN,
                Duration::from_millis(50),
                Duration::from_millis(50),
            )
            .await;
        assert!(
            matches!(result, Err(Error::UnexpectedResponse(ref message)) if message.contains("overlaps")),
            "a duplicate chunk must fail the read, got {result:?}"
        );
    }

    #[test]
    fn transfer_buffer_rejects_overlapping_inserts() {
        let mut buffer = TransferBuffer::new(16);
        buffer.insert(0, &[1u8; 8]).unwrap();
        buffer.insert(8, &[2u8; 8]).unwrap();
        assert!(buffer.is_complete());

        let mut buffer = TransferBuffer::new(16);
        buffer.insert(0, &[1u8; 8]).unwrap();
        let error = buffer.insert(4, &[2u8; 8]).unwrap_err();
        assert!(
            matches!(error, Error::UnexpectedResponse(ref message) if message.contains("overlaps")),
            "unexpected error: {error:?}"
        );
        assert_eq!(buffer.received(), 8, "a rejected chunk must not fill bytes");
        // A partial overlap at the end of the filled range is rejected too.
        let error = buffer.insert(6, &[3u8; 2]).unwrap_err();
        assert!(matches!(error, Error::UnexpectedResponse(_)));
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

    /// One step of a [`ScriptedTransport`] queue.
    enum ScriptedFrame {
        /// A complete notification frame.
        Data(Vec<u8>),
        /// Go idle longer than the session timeout, like an empty BLE queue.
        Stall,
    }

    /// Transport that replays a scripted sequence, then fails as disconnected.
    struct ScriptedTransport {
        script: Mutex<VecDeque<ScriptedFrame>>,
    }

    impl ScriptedTransport {
        fn new(script: Vec<ScriptedFrame>) -> Self {
            Self {
                script: Mutex::new(script.into()),
            }
        }
    }

    #[async_trait]
    impl Transport for ScriptedTransport {
        async fn send(&self, _data: &[u8]) -> Result<(), Error> {
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            let next = self
                .script
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .pop_front();
            match next {
                Some(ScriptedFrame::Data(frame)) => Ok(frame),
                Some(ScriptedFrame::Stall) => {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    Err(Error::Disconnected)
                }
                None => Err(Error::Disconnected),
            }
        }

        fn mtu(&self) -> u16 {
            64
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
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

    struct ResyncTransport {
        incoming: Mutex<VecDeque<Vec<u8>>>,
        fresh_config: Mutex<Vec<Vec<u8>>>,
        sent: Mutex<Vec<Vec<u8>>>,
        pending_before_first_config_send: Mutex<Option<usize>>,
    }

    impl ResyncTransport {
        fn new() -> Self {
            Self {
                incoming: Mutex::new(VecDeque::new()),
                fresh_config: Mutex::new(Vec::new()),
                sent: Mutex::new(Vec::new()),
                pending_before_first_config_send: Mutex::new(None),
            }
        }

        fn push_stale(&self, frame: Vec<u8>) {
            self.incoming.lock().unwrap().push_back(frame);
        }

        fn set_fresh_config(&self, frames: Vec<Vec<u8>>) {
            *self.fresh_config.lock().unwrap() = frames;
        }

        fn pending_before_first_config_send(&self) -> Option<usize> {
            *self.pending_before_first_config_send.lock().unwrap()
        }
    }

    #[async_trait]
    impl Transport for ResyncTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            self.sent.lock().unwrap().push(data.to_vec());
            if let Some(header_bytes) = data.get(1..) {
                if let Ok(header) = CustomHeader::parse(header_bytes) {
                    if header.cmd == commands::cmd::READ_CONFIG {
                        let fresh = std::mem::take(&mut *self.fresh_config.lock().unwrap());
                        if !fresh.is_empty() {
                            let mut incoming = self.incoming.lock().unwrap();
                            let mut recorded =
                                self.pending_before_first_config_send.lock().unwrap();
                            if recorded.is_none() {
                                *recorded = Some(incoming.len());
                            }
                            incoming.extend(fresh);
                        }
                    }
                }
            }
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            if let Some(frame) = self.incoming.lock().unwrap().pop_front() {
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

    fn build_fresh_config_frames(config: &Bt2Config) -> Vec<Vec<u8>> {
        let mut fresh = Vec::new();
        for (index, chunk) in config.as_bytes().chunks(142).enumerate() {
            let offset = (index * 142) as u32;
            let header = CustomHeader::new(
                commands::cmd::RESPONSE,
                commands::cmd::READ_CONFIG,
                CONFIG_SIZE as u32,
                offset,
            )
            .with_payload(chunk);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(chunk);
            fresh.push(frame);
        }
        fresh
    }

    fn build_stale_config_frame() -> Vec<u8> {
        let stale = CustomHeader::new(
            commands::cmd::RESPONSE,
            commands::cmd::READ_CONFIG,
            CONFIG_SIZE as u32,
            0,
        )
        .with_payload(&[0xAA; 142]);
        let mut stale_frame = stale.to_bytes().to_vec();
        stale_frame.extend_from_slice(&[0xAA; 142]);
        stale_frame
    }

    #[tokio::test]
    async fn resync_discards_stale_config_chunk_before_next_read() {
        let transport = ResyncTransport::new();
        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));

        // Force a timeout so the session records that a resync is needed.
        assert!(session.read_version().await.is_err());

        transport.push_stale(build_stale_config_frame());

        let config = sample_config();
        transport.set_fresh_config(build_fresh_config_frames(&config));

        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(
            transport.pending_before_first_config_send(),
            Some(0),
            "stale frame must be drained before the first ReadConfig send"
        );
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
        assert_ne!(&snapshot.config.as_bytes()[..8], &[0xAA; 8]);
    }

    #[tokio::test]
    async fn drain_pending_is_bounded_against_flooding_transport() {
        use std::sync::atomic::AtomicUsize;

        struct FloodingTransport {
            recv_calls: AtomicUsize,
            delay: Duration,
        }

        #[async_trait]
        impl Transport for FloodingTransport {
            async fn send(&self, _data: &[u8]) -> Result<(), Error> {
                Ok(())
            }

            async fn recv(&self) -> Result<Vec<u8>, Error> {
                if !self.delay.is_zero() {
                    tokio::time::sleep(self.delay).await;
                }
                self.recv_calls.fetch_add(1, Ordering::SeqCst);
                Ok(OBSERVED_VERSION_ACK.to_vec())
            }

            fn mtu(&self) -> u16 {
                64
            }

            async fn disconnect(&self) -> Result<(), Error> {
                Ok(())
            }
        }

        // Frame-count cap: immediate frames stop at MAX_DRAIN_FRAMES.
        let fast = FloodingTransport {
            recv_calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        };
        let session = Session::new(&fast);
        session.needs_resync.store(true, Ordering::SeqCst);
        let started = Instant::now();
        // The frame budget is spent before the transport goes idle, so the
        // drain cannot prove the queue is empty: it fails closed and keeps
        // the resync flag set for the next attempt.
        let result = session.resync().await;
        assert!(
            matches!(result, Err(Error::UnexpectedResponse(ref message)) if message.contains("drain budget exhausted")),
            "unexpected resync result: {result:?}"
        );
        assert_eq!(fast.recv_calls.load(Ordering::SeqCst), MAX_DRAIN_FRAMES);
        assert!(
            session.needs_resync.load(Ordering::SeqCst),
            "an exhausted drain must leave needs_resync set"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "drain_pending took {:?}",
            started.elapsed()
        );

        // Time-budget cap: sub-1ms frames stop once MAX_DRAIN_DURATION elapses.
        let paced = FloodingTransport {
            recv_calls: AtomicUsize::new(0),
            delay: Duration::from_micros(500),
        };
        let session = Session::new(&paced);
        session.needs_resync.store(true, Ordering::SeqCst);
        let started = Instant::now();
        // Each frame costs 500 µs of the 50 ms budget, so the time budget
        // expires well before the frame budget and the drain fails closed.
        let result = session.resync().await;
        assert!(
            matches!(result, Err(Error::UnexpectedResponse(ref message)) if message.contains("drain budget exhausted")),
            "unexpected paced resync result: {result:?}"
        );
        assert!(
            session.needs_resync.load(Ordering::SeqCst),
            "a time-exhausted drain must leave needs_resync set"
        );
        let calls = paced.recv_calls.load(Ordering::SeqCst);
        assert!(calls > 0 && calls <= MAX_DRAIN_FRAMES);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "paced drain_pending took {:?}",
            started.elapsed()
        );
    }

    /// Transport that never runs out of unrelated frames, so a drain loop can
    /// only stop because of its own frame or time budget.
    struct EndlessNoiseTransport {
        recv_calls: std::sync::atomic::AtomicUsize,
    }

    impl EndlessNoiseTransport {
        fn new() -> Self {
            Self {
                recv_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn recv_calls(&self) -> usize {
            self.recv_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Transport for EndlessNoiseTransport {
        async fn send(&self, _data: &[u8]) -> Result<(), Error> {
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            self.recv_calls.fetch_add(1, Ordering::SeqCst);
            // A complete but non-input frame (the 0x0012 notice).
            Ok(CustomHeader::new(commands::cmd::ALLOW_CONFIG_DONE, 0, 0, 0)
                .to_bytes()
                .to_vec())
        }

        fn mtu(&self) -> u16 {
            64
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn try_next_input_stops_after_frame_budget_with_endless_notifications() {
        let transport = EndlessNoiseTransport::new();
        let session = Session::new(&transport);

        // A caller loop (keepalive / monitor) must regain control even while
        // notifications never stop arriving.
        let started = Instant::now();
        for _ in 0..3 {
            let report = session
                .try_next_input(Duration::from_secs(5))
                .await
                .expect("non-input frames are skipped, not errored");
            assert!(report.is_none(), "no input report is ever delivered");
        }
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "three bounded drains took {:?}",
            started.elapsed()
        );
        // Per call: MAX_INPUT_DRAIN_FRAMES skipped frames plus at most one
        // final opportunistic drain.
        assert!(
            transport.recv_calls() <= 3 * (MAX_INPUT_DRAIN_FRAMES + 1),
            "try_next_input consumed {} frames",
            transport.recv_calls()
        );
    }

    #[tokio::test]
    async fn try_next_input_returns_trailing_report_after_skipped_frames() {
        let mock = MockTransport::new();
        for _ in 0..3 {
            let notice = CustomHeader::new(commands::cmd::ALLOW_CONFIG_DONE, 0, 0, 0);
            mock.push_response(&notice, &[]);
        }
        let report = InputReport {
            keys: 0x0000_0001,
            ..InputReport::default()
        };
        let input =
            CustomHeader::new(commands::cmd::INPUT, 0, 12, 12).with_payload(&report.to_bytes());
        mock.push_response(&input, &report.to_bytes());

        let session = Session::new(&mock);
        let decoded = session
            .try_next_input(Duration::from_millis(200))
            .await
            .unwrap()
            .expect("the queued report is found after the notices");
        assert_eq!(decoded, report);
    }

    #[tokio::test]
    async fn try_next_input_skips_one_malformed_frame_and_returns_the_next_report() {
        // B2 regression: a single 12-byte garbage notification used to return
        // `Err` from `try_next_input`, which the session keepalive counted as
        // a failure and degraded on after three ticks.
        let mock = MockTransport::new();
        mock.push_incoming(&[0xFF; 12]);
        let report = InputReport {
            keys: 0x0000_0010,
            ..InputReport::default()
        };
        let input =
            CustomHeader::new(commands::cmd::INPUT, 0, 12, 12).with_payload(&report.to_bytes());
        mock.push_response(&input, &report.to_bytes());

        let session = Session::new(&mock);
        let decoded = session
            .try_next_input(Duration::from_millis(200))
            .await
            .expect("one corrupt frame must be skipped, not returned as an error")
            .expect("the valid report is still delivered");
        assert_eq!(decoded, report);
        assert_eq!(session.skipped_frames(), 1);
    }

    #[tokio::test]
    async fn try_next_input_skips_frames_with_an_impossible_chunk_len() {
        let mock = MockTransport::new();
        // A 16-byte header that declares 512 payload bytes.
        let header = envelope_header(commands::cmd::INPUT, 12, 12, 512);
        mock.push_incoming(&header.to_bytes());
        let report = InputReport {
            keys: 0x0000_0020,
            ..InputReport::default()
        };
        let input =
            CustomHeader::new(commands::cmd::INPUT, 0, 12, 12).with_payload(&report.to_bytes());
        mock.push_response(&input, &report.to_bytes());

        let session = Session::new(&mock);
        let decoded = session
            .try_next_input(Duration::from_millis(200))
            .await
            .unwrap()
            .expect("a truncated frame must not hide the next report");
        assert_eq!(decoded, report);
        assert_eq!(session.skipped_frames(), 1);
    }

    #[tokio::test]
    async fn try_next_input_fails_after_repeated_malformed_frames() {
        let mock = MockTransport::new();
        for _ in 0..MAX_CONSECUTIVE_DECODE_FAILURES {
            mock.push_incoming(&[0xEE; 5]);
        }
        let session = Session::new(&mock);

        let result = session.try_next_input(Duration::from_millis(200)).await;
        assert!(
            matches!(result, Err(Error::Protocol(_))),
            "a link that only delivers garbage must fail, got {result:?}"
        );
        assert_eq!(
            session.skipped_frames(),
            MAX_CONSECUTIVE_DECODE_FAILURES as u64
        );
    }

    #[tokio::test]
    async fn seven_malformed_frames_then_a_report_stay_below_the_cap() {
        // B2 boundary: one malformed frame below the limit is still noise and
        // the report that follows is delivered.
        let mock = MockTransport::new();
        for _ in 0..MAX_CONSECUTIVE_DECODE_FAILURES - 1 {
            mock.push_incoming(&[0xEE; 5]);
        }
        let report = InputReport {
            keys: 0x0000_0041,
            ..InputReport::default()
        };
        let input =
            CustomHeader::new(commands::cmd::INPUT, 0, 12, 12).with_payload(&report.to_bytes());
        mock.push_response(&input, &report.to_bytes());

        let session = Session::new(&mock);
        let decoded = session
            .try_next_input(Duration::from_millis(200))
            .await
            .expect("seven malformed frames are below the cap")
            .expect("the report after the noise is delivered");
        assert_eq!(decoded, report);
        assert_eq!(
            session.skipped_frames(),
            (MAX_CONSECUTIVE_DECODE_FAILURES - 1) as u64
        );
        assert_eq!(session.recovered_frame_runs(), 1);
    }

    #[tokio::test]
    async fn idle_returns_clear_the_decode_failure_run_between_calls() {
        // M-2 regression: one malformed frame per call, with an idle gap that
        // ends the call normally, must not accumulate into a degraded link.
        let mut scripted = Vec::new();
        for _ in 0..MAX_CONSECUTIVE_DECODE_FAILURES {
            scripted.push(ScriptedFrame::Data(vec![0xEE; 5]));
            // Two idle gaps: the main wait and the opportunistic drain that
            // follows it each need to see an empty queue.
            scripted.push(ScriptedFrame::Stall);
            scripted.push(ScriptedFrame::Stall);
        }
        let transport = ScriptedTransport::new(scripted);
        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));

        for call in 0..MAX_CONSECUTIVE_DECODE_FAILURES {
            let result = session.try_next_input(Duration::from_millis(50)).await;
            assert!(
                matches!(result, Ok(None)),
                "call {call} must end idle, not fail: {result:?}"
            );
            assert_eq!(
                session.consecutive_decode_failures.load(Ordering::Relaxed),
                0,
                "call {call} must end the malformed run"
            );
        }
        assert_eq!(
            session.skipped_frames(),
            MAX_CONSECUTIVE_DECODE_FAILURES as u64
        );
    }

    #[tokio::test]
    async fn a_decodable_notice_clears_the_decode_failure_run() {
        // M-2: 7 malformed, a decodable notice, 7 more malformed and a report.
        // Without the reset on a decodable non-input frame the second run
        // reaches 8 and fails; the notice proves the link still works.
        let mock = MockTransport::new();
        for _ in 0..MAX_CONSECUTIVE_DECODE_FAILURES - 1 {
            mock.push_incoming(&[0xEE; 5]);
        }
        let notice = CustomHeader::new(commands::cmd::ALLOW_CONFIG_DONE, 0, 0, 0);
        mock.push_response(&notice, &[]);
        for _ in 0..MAX_CONSECUTIVE_DECODE_FAILURES - 1 {
            mock.push_incoming(&[0xEE; 5]);
        }
        let report = InputReport {
            keys: 0x0000_0042,
            ..InputReport::default()
        };
        let input =
            CustomHeader::new(commands::cmd::INPUT, 0, 12, 12).with_payload(&report.to_bytes());
        mock.push_response(&input, &report.to_bytes());

        let session = Session::new(&mock);
        let decoded = session
            .try_next_input(Duration::from_millis(200))
            .await
            .expect("a notice between the two malformed runs resets the count")
            .expect("the report is delivered");
        assert_eq!(decoded, report);
        assert_eq!(
            session.skipped_frames(),
            (2 * (MAX_CONSECUTIVE_DECODE_FAILURES - 1)) as u64
        );
    }

    #[tokio::test]
    async fn try_next_input_resets_the_failure_run_after_a_good_frame() {
        // Three malformed frames and then a report in one call: the decoded
        // report ends the run without reaching the cap, clears the counter,
        // and is counted as a recovery. (An idle return also clears the run;
        // see `idle_returns_clear_the_decode_failure_run_between_calls`.)
        let mock = MockTransport::new();
        for _ in 0..3 {
            mock.push_incoming(&[0xEE; 5]);
        }
        let report = InputReport {
            keys: 0x0000_0030,
            ..InputReport::default()
        };
        let input =
            CustomHeader::new(commands::cmd::INPUT, 0, 12, 12).with_payload(&report.to_bytes());
        mock.push_response(&input, &report.to_bytes());

        let session = Session::new(&mock);
        let decoded = session
            .try_next_input(Duration::from_millis(200))
            .await
            .expect("three malformed frames are below the cap")
            .expect("the report after the noise is delivered");
        assert_eq!(decoded, report);
        assert_eq!(session.skipped_frames(), 3);
        assert_eq!(
            session.recovered_frame_runs(),
            1,
            "the decoded report must end the malformed run and be counted as a recovery"
        );
        assert_eq!(
            session.consecutive_decode_failures.load(Ordering::Relaxed),
            0,
            "a decoded report must clear the pending run"
        );
    }

    #[tokio::test]
    async fn valid_input_frames_are_skipped_by_reads_and_short_frames_fail_closed() {
        // A valid realtime input report is unrelated to a non-input request:
        // the read path skips it without touching the input-path counters.
        let mock = MockTransport::new();
        let report = InputReport {
            keys: 0x0000_0040,
            ..InputReport::default()
        };
        let input =
            CustomHeader::new(commands::cmd::INPUT, 0, 12, 12).with_payload(&report.to_bytes());
        mock.push_response(&input, &report.to_bytes());
        let version = envelope_header(
            commands::cmd::READ_VERSION,
            commands::VERSION_REQUEST_LEN,
            0,
            4,
        );
        mock.push_response(&version, &[0x01, 0x02, 0x03, 0x04]);

        let session = Session::new(&mock);
        let read = session.read_version().await;
        assert!(
            read.is_ok(),
            "read_version must be unaffected by a complete input frame: {read:?}"
        );
        assert_eq!(
            session.skipped_frames(),
            0,
            "the input frame is skipped by classify_response, not counted as a decode failure"
        );
        assert!(
            !session.needs_resync.load(Ordering::SeqCst),
            "a successfully skipped frame must not leave a resync mark"
        );

        // A frame too short to contain an App Mode header is a framing error,
        // not input-path noise. Both read paths fail closed instead of
        // treating the following bytes as a response and resync is requested
        // so stale bytes cannot be reused by the next read.
        let mock = MockTransport::new();
        mock.push_incoming(&[0xEE; 5]);
        let session = Session::new(&mock);
        let version = session.read_version().await;
        assert!(
            matches!(version, Err(Error::Protocol(_))),
            "a 5-byte frame must fail read_version, got {version:?}"
        );
        assert!(
            session.needs_resync.load(Ordering::SeqCst),
            "a framing error must mark the session for resync"
        );
        assert_eq!(
            session.skipped_frames(),
            0,
            "read paths do not use the input-path skip counter"
        );

        let mock = MockTransport::new();
        mock.push_incoming(&[0xEE; 5]);
        let session = Session::new(&mock);
        let snapshot = session.read_config_snapshot().await;
        assert!(
            matches!(snapshot, Err(Error::Protocol(_))),
            "a 5-byte frame must fail read_config_snapshot, got {snapshot:?}"
        );
        assert!(
            session.needs_resync.load(Ordering::SeqCst),
            "a framing error during ReadConfig must mark the session for resync"
        );
    }

    #[tokio::test]
    async fn resync_with_idle_transport_clears_needs_resync() {
        let transport = ResyncTransport::new();
        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));
        assert!(session.read_version().await.is_err());
        assert!(session.needs_resync.load(Ordering::SeqCst));

        session.resync().await.expect("an idle queue quiesces");
        assert!(
            !session.needs_resync.load(Ordering::SeqCst),
            "a quiesced drain must clear needs_resync"
        );
    }

    fn observed_version_envelope_header() -> CustomHeader {
        let header = CustomHeader::parse(&OBSERVED_VERSION_DATA).unwrap();
        assert_eq!(header.crc16, 0x443E, "observed payload CRC");
        header
    }

    #[tokio::test]
    async fn envelope_response_with_matching_crc_is_accepted() {
        let mock = MockTransport::new();
        mock.push_incoming(&OBSERVED_VERSION_DATA);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();
        assert_eq!(version.raw, [0xC8, 0x00, 0x00, 0x00]);
    }

    #[tokio::test]
    async fn envelope_response_with_bad_crc_is_rejected() {
        let mock = MockTransport::new();
        let mut frame = OBSERVED_VERSION_DATA;
        // Flip one payload bit; crc16 still claims 0x443E.
        frame[16] ^= 0x01;
        mock.push_incoming(&frame);

        let session = Session::new(&mock);
        let result = session.read_version().await;
        assert!(
            matches!(result, Err(Error::UnexpectedResponse(ref message)) if message.contains("crc16")),
            "unexpected result: {result:?}"
        );
        assert!(
            session.needs_resync.load(Ordering::SeqCst),
            "a rejected frame must mark the session for resync"
        );
    }

    #[tokio::test]
    async fn envelope_response_with_zero_crc_is_accepted_without_check() {
        let mock = MockTransport::new();
        let mut header = observed_version_envelope_header();
        header.crc16 = 0;
        mock.push_response(&header, &[0xAA, 0xBB, 0xCC, 0xDD]);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();
        assert_eq!(version.raw, [0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[tokio::test]
    async fn direct_response_with_total_len_mismatch_is_rejected() {
        let mock = MockTransport::new();
        // A direct echo that declares a different transfer size (0x0AD0).
        let response = CustomHeader::new(commands::cmd::READ_VERSION, 0, CONFIG_SIZE as u32, 0)
            .with_payload(&[1, 2, 3, 4]);
        mock.push_response(&response, &[1, 2, 3, 4]);

        let session = Session::new(&mock);
        let result = session.read_version().await;
        assert!(
            matches!(result, Err(Error::UnexpectedResponse(ref message)) if message.contains("total_len")),
            "unexpected result: {result:?}"
        );
    }

    #[tokio::test]
    async fn direct_response_with_zero_total_len_is_accepted() {
        let mock = MockTransport::new();
        let response =
            CustomHeader::new(commands::cmd::READ_VERSION, 0, 0, 0).with_payload(&[7, 7, 7, 7]);
        mock.push_response(&response, &[7, 7, 7, 7]);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();
        assert_eq!(version.raw, [7, 7, 7, 7]);
    }

    #[tokio::test]
    async fn direct_response_with_bad_crc_is_rejected() {
        let mock = MockTransport::new();
        let payload = [1u8, 2, 3, 4];
        let mut header =
            CustomHeader::new(commands::cmd::READ_VERSION, 0, 4, 0).with_payload(&payload);
        header.crc16 ^= 0xFFFF;
        mock.push_response(&header, &payload);

        let session = Session::new(&mock);
        let result = session.read_version().await;
        assert!(
            matches!(result, Err(Error::UnexpectedResponse(ref message)) if message.contains("crc16")),
            "unexpected result: {result:?}"
        );
    }

    #[tokio::test]
    async fn direct_response_with_matching_crc_is_accepted() {
        let mock = MockTransport::new();
        let payload = [0x11u8, 0x22, 0x33, 0x44];
        let header = CustomHeader::new(commands::cmd::READ_VERSION, 0, 4, 0).with_payload(&payload);
        assert_ne!(header.crc16, 0, "the fixture must carry a non-zero CRC");
        mock.push_response(&header, &payload);

        let session = Session::new(&mock);
        let version = session.read_version().await.unwrap();
        assert_eq!(version.raw, payload);
    }

    #[tokio::test]
    async fn read_config_chunk_with_bad_crc_is_rejected() {
        let config = sample_config();
        let bytes = config.as_bytes();
        let mock = MockTransport::new();
        push_config_envelope_chunks(&mock, &bytes[..142], 142);

        // Second chunk flips one payload bit but keeps its crc16.
        let mut payload = bytes[142..284].to_vec();
        payload[3] ^= 0x80;
        let header = envelope_header(commands::cmd::READ_CONFIG, CONFIG_SIZE as u32, 142, 142)
            .with_payload(&bytes[142..284]);
        mock.push_response(&header, &payload);

        let session = Session::new(&mock);
        let result = session.read_config_snapshot().await;
        assert!(
            matches!(result, Err(Error::UnexpectedResponse(ref message)) if message.contains("crc16")),
            "unexpected result: {result:?}"
        );
    }

    #[tokio::test]
    async fn oversized_wait_is_rejected_without_panicking() {
        let mock = MockTransport::new();
        let session = Session::new(&mock);

        for wait in [Duration::MAX, MAX_WAIT + Duration::from_secs(1)] {
            let result = session.try_next_input(wait).await;
            assert!(
                matches!(result, Err(Error::InvalidTimeout(value)) if value == wait),
                "unexpected result for {wait:?}: {result:?}"
            );
        }

        let result = session
            .read_transfer(
                commands::cmd::READ_VERSION,
                4,
                Duration::from_millis(1),
                Duration::MAX,
            )
            .await;
        assert!(matches!(result, Err(Error::InvalidTimeout(_))));
        assert!(
            mock.sent_frames().is_empty(),
            "an invalid timeout is rejected before any I/O"
        );
    }

    #[test]
    fn deadline_after_accepts_max_wait_and_rejects_larger() {
        assert!(deadline_after(MAX_WAIT).is_ok());
        assert!(deadline_after(Duration::ZERO).is_ok());
        assert!(matches!(
            deadline_after(MAX_WAIT + Duration::from_nanos(1)),
            Err(Error::InvalidTimeout(_))
        ));
        assert!(matches!(
            deadline_after(Duration::MAX),
            Err(Error::InvalidTimeout(_))
        ));
    }

    #[tokio::test]
    async fn drop_mid_pipeline_marks_resync_and_next_read_drains() {
        // Cancelling while the 3rd config chunk is in flight leaves a response
        // in flight, so the next read drains the leftover chunks first and
        // then assembles the fresh transfer.
        let transport = ResyncTransport::new();
        let session = Session::new(&transport).with_timeout(Duration::from_secs(60));

        let cancelled =
            tokio::time::timeout(Duration::from_millis(10), session.read_config_snapshot()).await;
        assert!(cancelled.is_err(), "the read must be cancelled mid-flight");
        assert!(
            session.needs_resync.load(Ordering::SeqCst),
            "an abandoned read must mark the session for resync"
        );
        assert_eq!(
            session.in_flight.load(Ordering::SeqCst),
            InFlight::None as u8,
            "the guard must clear the in-flight marker when it drops"
        );

        let config = sample_config();
        transport.set_fresh_config(build_fresh_config_frames(&config));
        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(
            transport.pending_before_first_config_send(),
            Some(0),
            "the stale frame must be drained before the next ReadConfig send"
        );
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
        assert!(
            !session.needs_resync.load(Ordering::SeqCst),
            "a quiesced drain clears the mark"
        );
    }

    #[tokio::test]
    async fn an_abandoned_input_poll_does_not_require_a_resync() {
        // Ctrl-C during `monitor` drops `try_next_input`; the leftover is at
        // most a realtime report, which every read path skips. Marking the
        // session for resync here used to be hidden by the caller-side
        // acknowledgement, which also hid genuinely stale read responses.
        let transport = StallingTransport {
            first: Mutex::new(None),
        };
        let session = Session::new(&transport).with_timeout(Duration::from_secs(60));

        let cancelled = tokio::time::timeout(
            Duration::from_millis(10),
            session.try_next_input(Duration::from_secs(60)),
        )
        .await;
        assert!(cancelled.is_err(), "the poll must be cancelled mid-flight");
        assert!(
            !session.needs_resync.load(Ordering::SeqCst),
            "an abandoned input poll must not require a resync"
        );
        assert_eq!(
            session.in_flight.load(Ordering::SeqCst),
            InFlight::None as u8
        );
    }

    #[tokio::test]
    async fn a_timeout_on_a_read_keeps_the_resync_mark() {
        // A timeout is not a caller cancellation: the response may still
        // arrive, so the session must keep asking for a drain even though no
        // caller acknowledged anything.
        let transport = StallingTransport {
            first: Mutex::new(None),
        };
        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));

        let result = session.read_version().await;
        assert!(matches!(result, Err(Error::Timeout { .. })));
        assert!(
            session.needs_resync.load(Ordering::SeqCst),
            "a timed-out read must keep the resync mark"
        );
    }

    #[tokio::test]
    async fn read_transfer_preserves_response_when_needs_resync_is_set() {
        let transport = ResyncTransport::new();
        let session = Session::new(&transport).with_timeout(Duration::from_millis(20));

        // Force needs_resync = true.
        assert!(session.read_version().await.is_err());
        assert!(session.needs_resync.load(Ordering::SeqCst));

        // Simulate an external caller sending a request and then calling
        // `read_transfer`: the queued legitimate response must not be drained.
        session
            .transport()
            .send(
                &commands::read_version()
                    .encode_ble(PacketKind::App)
                    .unwrap(),
            )
            .await
            .unwrap();
        let response = envelope_header(
            commands::cmd::READ_VERSION,
            commands::VERSION_RESPONSE_LEN as u32,
            0,
            4,
        );
        let mut frame = response.to_bytes().to_vec();
        frame.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        transport.push_stale(frame);

        let raw = session
            .read_transfer(
                commands::cmd::READ_VERSION,
                commands::VERSION_RESPONSE_LEN,
                Duration::from_millis(50),
                Duration::from_millis(50),
            )
            .await
            .unwrap();
        assert_eq!(raw, [0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[tokio::test]
    async fn cancelled_read_marks_needs_resync_and_discards_stale_response() {
        let transport = ResyncTransport::new();
        let session = Session::new(&transport).with_timeout(Duration::from_secs(5));

        // Start a read whose future is dropped mid-flight (cancellation).
        let cancelled =
            tokio::time::timeout(Duration::from_millis(10), session.read_config_snapshot()).await;
        assert!(
            cancelled.is_err(),
            "read should be cancelled before timeout"
        );
        assert!(
            session.needs_resync.load(Ordering::SeqCst),
            "dropping an in-flight read future must mark needs_resync"
        );

        transport.push_stale(build_stale_config_frame());
        let config = sample_config();
        transport.set_fresh_config(build_fresh_config_frames(&config));

        let snapshot = session.read_config_snapshot().await.unwrap();
        assert_eq!(
            transport.pending_before_first_config_send(),
            Some(0),
            "stale frame must be drained before the post-cancel ReadConfig send"
        );
        assert_eq!(snapshot.config.as_bytes(), config.as_bytes());
        assert_ne!(&snapshot.config.as_bytes()[..8], &[0xAA; 8]);
    }

    #[test]
    fn classify_response_accepts_input_envelope_when_requesting_input() {
        let payload = [0x12u8; 12];
        let header = CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::INPUT, 12, 0)
            .with_payload(&payload);
        let mut frame = header.to_bytes().to_vec();
        frame.extend_from_slice(&payload);

        let classified =
            Session::<MockTransport>::classify_response(&frame, commands::cmd::INPUT, 12)
                .unwrap()
                .expect("INPUT envelope must not be skipped when request_cmd == INPUT");
        assert_eq!(classified.0.subcmd, commands::cmd::INPUT);
        assert_eq!(classified.1, &payload);

        // Non-input requests still skip interleaved input envelopes.
        let skipped =
            Session::<MockTransport>::classify_response(&frame, commands::cmd::READ_VERSION, 4)
                .unwrap();
        assert!(skipped.is_none());
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
    async fn input_reader_accepts_envelope_report() {
        let mock = MockTransport::new();
        let report = InputReport {
            keys: 0x0000_4002,
            left_stick_x: 0x40,
            left_stick_y: 0x80,
            right_stick_x: 0x10,
            right_stick_y: 0xF0,
            trigger_r: 0x20,
            trigger_l: 0x30,
            battery: 0x55,
            reserved: 0,
        };
        let header = CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::INPUT, 12, 12)
            .with_payload(&report.to_bytes());
        mock.push_response(&header, &report.to_bytes());

        let session = Session::new(&mock);
        let decoded = session
            .try_next_input(Duration::from_millis(200))
            .await
            .unwrap()
            .expect("an envelope report is queued");
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
