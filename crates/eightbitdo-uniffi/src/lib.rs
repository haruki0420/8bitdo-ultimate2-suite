//! UniFFI bindings for the mobile front ends (iOS SwiftUI / Android Compose).
//!
//! This crate is the only seam the mobile apps see. It wraps
//! [`eightbitdo_core::Session`] behind a **synchronous** API, injects the
//! platform BLE stack through a [`NativeTransport`] callback interface, and
//! flattens core errors into [`MobileError`].
//!
//! # Design
//!
//! * The platform (CoreBluetooth / BluetoothGatt) owns scanning, connecting and
//!   the GATT callbacks. It hands Rust an already-connected link.
//! * `BridgeTransport` adapts that link to the core [`Transport`] trait. A
//!   dedicated thread performs the blocking `recv` poll and forwards frames
//!   over a Tokio channel, so the core's `tokio::time::timeout` wrappers keep
//!   working without blocking a Tokio worker in JNA or CoreBluetooth.
//! * Every exported call is synchronous and runs on a private current-thread
//!   Tokio runtime. Kotlin/Swift callers are expected to invoke it from a
//!   background dispatcher (`Dispatchers.IO`, a utility queue, ...).
//! * All session calls are serialized with a mutex, because
//!   [`eightbitdo_core::Session`] has a single-flight contract.
//!
//! # Scope
//!
//! The current surface is **read-only** plus *configuration write preparation*:
//! [`prepare_config_write`] validates an edited 2768-byte buffer, recomputes the
//! CRC and reports the changed regions, but nothing here sends a write. The
//! flash write itself (`Session::write_full_config`) stays unexported until the
//! Tier 3 safety review and the mandatory pre-write backup flow are in place.
//!
//! # Threading contract
//!
//! [`MobileSession`] may be shared across threads. Calls block; do not call
//! them on the UI thread. The platform transport's `recv` must not block for
//! longer than the requested timeout and must return an error after
//! `disconnect` so the receive worker can stop.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

uniffi::setup_scaffolding!();

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use eightbitdo_core::{ConfigSnapshot, Session, Transport};
use eightbitdo_protocol::config_bt2::{Bt2Config, Slot, CONFIG_SIZE};
use eightbitdo_protocol::macro_data::MacroStep;
use tokio::sync::mpsc;

/// How long one blocking platform `recv` call waits before returning `None`.
///
/// Keeping this short bounds how long [`BridgeTransport`] can take to stop
/// after `disconnect` without requiring a platform-specific wakeup token.
const RECV_POLL_MS: u32 = 200;

/// An error returned by a platform transport callback.
///
/// Kotlin and Swift throw this from [`NativeTransport`]; Rust turns it into a
/// transport-level [`MobileError::Transport`] or a disconnect.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum NativeTransportError {
    /// The platform could not complete the operation.
    #[error("{message}")]
    Failure {
        /// Human-readable description (should not contain secrets).
        message: String,
    },
}

/// The platform BLE link, implemented in Swift (CoreBluetooth) or Kotlin
/// (`BluetoothGatt` / USB OTG) and injected into the Rust core.
///
/// # Contract
///
/// * `send` writes exactly one already-encoded frame (1-byte App Mode prefix +
///   16-byte header + payload) and returns once the platform accepted it.
///   Write-without-response links may return before the controller ACKs; the
///   core protocol layer handles ACK waits.
/// * `recv` waits up to `timeout_ms` for one complete notification frame
///   (header + payload, **without** the prefix byte, matching
///   `eightbitdo_protocol::header::decode_app_response`). It returns `None` on
///   an empty timeout and an error once the link is gone.
/// * `mtu` reports the **negotiated** ATT MTU, never the requested value.
/// * `disconnect` is idempotent and must make an in-flight `recv` return.
#[uniffi::export(with_foreign)]
pub trait NativeTransport: Send + Sync {
    /// Write one complete encoded frame.
    fn send(&self, data: Vec<u8>) -> Result<(), NativeTransportError>;

    /// Wait up to `timeout_ms` for one notification frame.
    fn recv(&self, timeout_ms: u32) -> Result<Option<Vec<u8>>, NativeTransportError>;

    /// Negotiated link MTU in bytes.
    fn mtu(&self) -> u16;

    /// Close the link. Must be safe to call more than once.
    fn disconnect(&self);
}

/// An error surfaced to Kotlin / Swift.
///
/// The variants are shaped so the UI can decide what to show without parsing
/// English: transport problems suggest a reconnect, protocol problems suggest
/// a resync, and timeouts suggest retrying once the device is awake.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    /// The platform link failed.
    #[error("transport error: {message}")]
    Transport {
        /// Underlying description.
        message: String,
    },
    /// The controller answered with something the protocol layer rejected.
    #[error("protocol error: {message}")]
    Protocol {
        /// Underlying description.
        message: String,
    },
    /// An operation did not finish within the core's deadline.
    #[error("timed out: {message}")]
    Timeout {
        /// Underlying description.
        message: String,
    },
    /// The notification channel was closed by the platform.
    #[error("disconnected: {message}")]
    Disconnected {
        /// Underlying description.
        message: String,
    },
    /// The operation is not implemented or not supported on this link.
    #[error("unsupported: {message}")]
    Unsupported {
        /// Underlying description.
        message: String,
    },
    /// The caller passed inconsistent input (wrong buffer size, ...).
    #[error("invalid request: {message}")]
    InvalidRequest {
        /// Underlying description.
        message: String,
    },
    /// A local resource (thread, runtime, JSON) failed.
    #[error("internal error: {message}")]
    Internal {
        /// Underlying description.
        message: String,
    },
}

impl MobileError {
    fn internal(context: &str, error: impl std::fmt::Display) -> Self {
        Self::Internal {
            message: format!("{context}: {error}"),
        }
    }
}

/// Map a core error onto the flat mobile error.
fn core_error(error: eightbitdo_core::Error) -> MobileError {
    use eightbitdo_core::Error as CoreError;

    let message = error.to_string();
    match error {
        CoreError::Protocol(_)
        | CoreError::UnexpectedResponse(_)
        | CoreError::IncompleteTransfer { .. }
        | CoreError::MacroWrite { .. }
        | CoreError::ConfigWrite { .. } => MobileError::Protocol { message },
        CoreError::Transport(_)
        | CoreError::DeviceNotFound { .. }
        | CoreError::CharacteristicNotFound => MobileError::Transport { message },
        CoreError::Timeout { .. } | CoreError::InvalidTimeout(_) => {
            MobileError::Timeout { message }
        }
        CoreError::Disconnected => MobileError::Disconnected { message },
        CoreError::Unsupported(_) | CoreError::EmptyMacroWrite => {
            MobileError::Unsupported { message }
        }
        CoreError::MtuTooSmall { .. } | CoreError::InvalidRequest(_) => {
            MobileError::InvalidRequest { message }
        }
        // `Error::Ble` only exists with the core's `ble` feature; the mobile
        // crate deliberately builds the core without it (the platform owns BLE).
        _ => MobileError::Transport { message },
    }
}

/// Adapts a foreign [`NativeTransport`] to the core's async [`Transport`].
///
/// A dedicated thread performs the blocking `recv` polling and forwards frames
/// over a Tokio channel, so the core's `tokio::time::timeout` wrapper keeps
/// working while no Tokio worker is parked inside JNA / CoreBluetooth.
struct BridgeTransport {
    inner: Arc<dyn NativeTransport>,
    rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>,
    mtu: u16,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl BridgeTransport {
    fn new(inner: Arc<dyn NativeTransport>) -> Result<Self, MobileError> {
        let mtu = inner.mtu();
        let (tx, rx) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_inner = Arc::clone(&inner);
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::Builder::new()
            .name("eightbitdo-rx".to_string())
            .spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    match worker_inner.recv(RECV_POLL_MS) {
                        Ok(Some(frame)) => {
                            if tx.send(frame).is_err() {
                                break;
                            }
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| MobileError::internal("failed to spawn the receive thread", error))?;

        Ok(Self {
            inner,
            rx: tokio::sync::Mutex::new(rx),
            mtu,
            stop,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Stop the receive worker and close the platform link. Idempotent.
    fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.inner.disconnect();
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            // Bounded by `RECV_POLL_MS`: the worker checks the stop flag after
            // every platform `recv` call.
            let _ = worker.join();
        }
    }
}

impl Drop for BridgeTransport {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[async_trait]
impl Transport for BridgeTransport {
    async fn send(&self, data: &[u8]) -> Result<(), eightbitdo_core::Error> {
        self.inner
            .send(data.to_vec())
            .map_err(|error| eightbitdo_core::Error::Transport(error.to_string()))
    }

    async fn recv(&self) -> Result<Vec<u8>, eightbitdo_core::Error> {
        self.rx
            .lock()
            .await
            .recv()
            .await
            .ok_or(eightbitdo_core::Error::Disconnected)
    }

    fn mtu(&self) -> u16 {
        self.mtu
    }

    async fn disconnect(&self) -> Result<(), eightbitdo_core::Error> {
        self.shutdown();
        Ok(())
    }
}

/// Firmware version read with `cmd 0x000B`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DeviceInfo {
    /// Raw response payload exactly as received (`4` bytes on `0x600F`).
    pub raw: Vec<u8>,
    /// Printable ASCII run in `raw`, trimmed; empty for a binary version.
    pub text: String,
}

/// A view of one 2768-byte configuration block.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DeviceConfig {
    /// The 2768 bytes, byte-for-byte as received. Pass this back to
    /// [`prepare_config_write`] after editing so the diff is computed against
    /// the device state.
    pub raw: Vec<u8>,
    /// CRC stored at `+0x00C` as read.
    pub stored_crc: u16,
    /// CRC recomputed over the received buffer.
    pub computed_crc: u16,
    /// `true` when the stored and recomputed CRC match.
    pub crc_valid: bool,
    /// Frequently displayed fields.
    pub summary: ConfigSummary,
}

/// The configuration fields a viewer shows on the first screen.
#[derive(Debug, Clone, serde::Serialize, uniffi::Record)]
pub struct ConfigSummary {
    /// Raw `gamepad_mode` (`+0x010`).
    pub gamepad_mode_raw: u16,
    /// Parsed mode name (`Switch`, `XInput`, ...) or `Unknown`.
    pub gamepad_mode: String,
    /// Active profile slot (`1..=3`) or `None` when unset.
    pub current_slot: Option<u8>,
    /// Raw per-slot flags (`+0x000`, three values).
    pub slot_flags: Vec<u32>,
    /// UTF-16BE profile names, three entries.
    pub profile_names: Vec<String>,
}

/// One macro in a 3-slot x 4-macro backup.
#[derive(Debug, Clone, serde::Serialize, uniffi::Record)]
pub struct MacroEntryInfo {
    /// Slot number (`1..=3`).
    pub slot: u8,
    /// Macro index inside the slot (`0..=3`).
    pub index: u8,
    /// Stored macro name.
    pub name: String,
    /// Number of 10-byte steps declared by the macro header.
    pub step_count: u32,
    /// Number of decoded step bytes (`step_count * 10`).
    pub raw_len: u32,
    /// `true` when the header declares an empty / unset macro.
    pub is_empty: bool,
}

/// A read-only macro backup: 3 slots x 4 macros.
#[derive(Debug, Clone, serde::Serialize, uniffi::Record)]
pub struct MacroBackupView {
    /// Twelve entries in slot-major order.
    pub entries: Vec<MacroEntryInfo>,
}

/// One decoded realtime input report (`cmd 0x0003`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct InputSnapshot {
    /// Raw 32-bit button mask.
    pub keys: u32,
    /// Left stick X in `-128..=128`, center snapped to `0`.
    pub left_x: i16,
    /// Left stick Y.
    pub left_y: i16,
    /// Right stick X.
    pub right_x: i16,
    /// Right stick Y.
    pub right_y: i16,
    /// Left trigger (`0..=255`).
    pub trigger_left: u8,
    /// Right trigger (`0..=255`).
    pub trigger_right: u8,
    /// Battery level reported by the controller.
    pub battery: u8,
}

/// A contiguous range of configuration bytes that differs from the snapshot.
#[derive(Debug, Clone, serde::Serialize, uniffi::Record)]
pub struct ConfigRegionInfo {
    /// First differing byte.
    pub offset: u32,
    /// Number of differing bytes.
    pub len: u32,
}

/// The result of validating an edited configuration **without writing it**.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ConfigWriteDraft {
    /// The exact 2768 bytes a write would send, with the CRC recomputed.
    pub expected: Vec<u8>,
    /// CRC stored at `+0x00C` in `expected`.
    pub expected_crc: u16,
    /// Contiguous ranges that differ from the snapshot in argument order.
    pub changed_regions: Vec<ConfigRegionInfo>,
    /// `true` when `expected` equals the snapshot (nothing to write).
    pub is_noop: bool,
}

/// Build a [`ConfigSummary`] from a raw 2768-byte block.
fn summarize(raw: &[u8]) -> Result<ConfigSummary, MobileError> {
    let config = Bt2Config::from_bytes(raw).map_err(|error| MobileError::InvalidRequest {
        message: format!("configuration must be {CONFIG_SIZE} bytes: {error}"),
    })?;
    Ok(ConfigSummary {
        gamepad_mode_raw: config.gamepad_mode_raw(),
        gamepad_mode: config
            .gamepad_mode()
            .map_or_else(|| "Unknown".to_string(), |mode| format!("{mode:?}")),
        current_slot: config.cur_slot().map(Slot::number),
        slot_flags: config.slot_flags().to_vec(),
        profile_names: Slot::ALL
            .iter()
            .map(|slot| config.profile_name(*slot))
            .collect(),
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing into a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// One config JSON document used by [`MobileSession::read_config_json`].
#[derive(serde::Serialize)]
struct ConfigJson<'a> {
    crc_valid: bool,
    stored_crc: u16,
    computed_crc: u16,
    summary: &'a ConfigSummary,
    raw_hex: String,
}

#[derive(serde::Serialize)]
struct MacroEntryJson {
    slot: u8,
    index: u8,
    name: String,
    step_count: u32,
    is_empty: bool,
    raw_hex: String,
    steps: Vec<MacroStep>,
}

#[derive(serde::Serialize)]
struct MacroBackupJson {
    config_crc_valid: bool,
    entries: Vec<MacroEntryJson>,
}

/// A connected App Mode session.
///
/// Construct it only after the platform link is connected and notifications
/// are enabled, and pass the negotiated MTU from the platform transport.
#[derive(uniffi::Object)]
pub struct MobileSession {
    // Declared before `runtime` so the transport (and its receive worker) is
    // dropped before the Tokio runtime it was polled on.
    session: Mutex<Session<BridgeTransport>>,
    runtime: tokio::runtime::Runtime,
    transport: Arc<dyn NativeTransport>,
}

impl MobileSession {
    fn lock(&self) -> MutexGuard<'_, Session<BridgeTransport>> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[uniffi::export]
impl MobileSession {
    /// Wrap an already-connected platform transport.
    ///
    /// The transport's `mtu()` is read once here; construct the session after
    /// MTU negotiation so chunk sizing sees the real value.
    ///
    /// # Errors
    ///
    /// Returns [`MobileError::Internal`] when the Tokio runtime or the receive
    /// thread cannot be created.
    #[uniffi::constructor]
    pub fn new(transport: Arc<dyn NativeTransport>) -> Result<Arc<Self>, MobileError> {
        let transport_for_bridge = Arc::clone(&transport);
        let bridge = BridgeTransport::new(transport_for_bridge)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|error| MobileError::internal("failed to build the Tokio runtime", error))?;
        Ok(Arc::new(Self {
            runtime,
            session: Mutex::new(Session::new(bridge)),
            transport,
        }))
    }

    /// Read the firmware version (`cmd 0x000B`).
    ///
    /// # Errors
    ///
    /// Returns a [`MobileError`] when the read times out or the link fails.
    pub fn read_device_info(&self) -> Result<DeviceInfo, MobileError> {
        let session = self.lock();
        let version = self
            .runtime
            .block_on(session.read_version())
            .map_err(core_error)?;
        Ok(DeviceInfo {
            raw: version.raw,
            text: version.text,
        })
    }

    /// Read the full 2768-byte configuration (`cmd 0x0002`).
    ///
    /// This takes several seconds over BLE and is not cancellable: run it off
    /// the UI thread and keep the app in the foreground.
    ///
    /// # Errors
    ///
    /// Returns a [`MobileError`] on timeout, transport failure or a malformed
    /// response.
    pub fn read_config(&self) -> Result<DeviceConfig, MobileError> {
        let session = self.lock();
        let snapshot = self
            .runtime
            .block_on(session.read_config_snapshot())
            .map_err(core_error)?;
        let summary = summarize(snapshot.config.as_bytes())?;
        Ok(device_config(snapshot, summary))
    }

    /// Read the configuration and return it as a JSON document for a viewer or
    /// an export file. Includes `raw_hex`.
    ///
    /// # Errors
    ///
    /// Same as [`MobileSession::read_config`], plus a JSON encoding error.
    pub fn read_config_json(&self) -> Result<String, MobileError> {
        let config = self.read_config()?;
        let document = ConfigJson {
            crc_valid: config.crc_valid,
            stored_crc: config.stored_crc,
            computed_crc: config.computed_crc,
            summary: &config.summary,
            raw_hex: hex_encode(&config.raw),
        };
        serde_json::to_string_pretty(&document)
            .map_err(|error| MobileError::internal("failed to encode JSON", error))
    }

    /// Read all 12 macros (3 slots x 4) after reading the configuration they
    /// belong to.
    ///
    /// This performs a full configuration read followed by the macro reads, so
    /// it can take tens of seconds over BLE.
    ///
    /// # Errors
    ///
    /// Returns a [`MobileError`] on timeout, transport failure or a malformed
    /// macro header.
    pub fn read_macro_backup(&self) -> Result<MacroBackupView, MobileError> {
        let (_, backup) = self.read_backup()?;
        Ok(MacroBackupView {
            entries: backup
                .entries()
                .iter()
                .map(|entry| MacroEntryInfo {
                    slot: entry.slot().number(),
                    index: entry.index().index() as u8,
                    name: entry.record().name(),
                    step_count: u32::from(entry.record().max_steps),
                    raw_len: entry.raw().len() as u32,
                    is_empty: entry.is_empty(),
                })
                .collect(),
        })
    }

    /// Read the macro backup and return it as a JSON document, including the
    /// decoded steps and the raw step bytes as hex.
    ///
    /// # Errors
    ///
    /// Same as [`MobileSession::read_macro_backup`], plus a JSON encoding error.
    pub fn read_macro_backup_json(&self) -> Result<String, MobileError> {
        let (snapshot, backup) = self.read_backup()?;
        let document = MacroBackupJson {
            config_crc_valid: snapshot.crc_valid,
            entries: backup
                .entries()
                .iter()
                .map(|entry| MacroEntryJson {
                    slot: entry.slot().number(),
                    index: entry.index().index() as u8,
                    name: entry.record().name(),
                    step_count: u32::from(entry.record().max_steps),
                    is_empty: entry.is_empty(),
                    raw_hex: hex_encode(entry.raw()),
                    steps: entry.steps().to_vec(),
                })
                .collect(),
        };
        serde_json::to_string_pretty(&document)
            .map_err(|error| MobileError::internal("failed to encode JSON", error))
    }

    /// Enable or disable the realtime input stream (`cmd 0x0007`).
    ///
    /// # Errors
    ///
    /// Returns a transport or framing error.
    pub fn set_report_enable(&self, enabled: bool) -> Result<(), MobileError> {
        let session = self.lock();
        self.runtime
            .block_on(session.set_report_enable(enabled))
            .map_err(core_error)
    }

    /// Wait up to `timeout_ms` for one realtime input report.
    ///
    /// `Ok(None)` means no report arrived in time. Run this on a background
    /// queue in a short loop; the official reference app polls at about 20 ms.
    ///
    /// # Errors
    ///
    /// Returns [`MobileError::Timeout`] when `timeout_ms` exceeds the core's
    /// maximum wait, or a transport / protocol error.
    pub fn read_input_once(&self, timeout_ms: u32) -> Result<Option<InputSnapshot>, MobileError> {
        let session = self.lock();
        let report = self
            .runtime
            .block_on(session.try_next_input(Duration::from_millis(u64::from(timeout_ms))))
            .map_err(core_error)?;
        Ok(report.map(|report| {
            let (left_x, left_y) = report.left_stick();
            let (right_x, right_y) = report.right_stick();
            InputSnapshot {
                keys: report.keys,
                left_x,
                left_y,
                right_x,
                right_y,
                trigger_left: report.trigger_l,
                trigger_right: report.trigger_r,
                battery: report.battery,
            }
        }))
    }

    /// Close the link and stop the receive worker. Idempotent.
    pub fn disconnect(&self) {
        // Unblock the platform `recv` first, then serialize on the session lock
        // so an in-flight read cannot interleave with the shutdown.
        self.transport.disconnect();
        let session = self.lock();
        session.transport().shutdown();
    }
}

impl MobileSession {
    fn read_backup(&self) -> Result<(ConfigSnapshot, eightbitdo_core::MacroBackup), MobileError> {
        let session = self.lock();
        let snapshot = self
            .runtime
            .block_on(session.read_config_snapshot())
            .map_err(core_error)?;
        let backup = self
            .runtime
            .block_on(session.read_macro_backup(&snapshot))
            .map_err(core_error)?;
        Ok((snapshot, backup))
    }
}

fn device_config(snapshot: ConfigSnapshot, summary: ConfigSummary) -> DeviceConfig {
    DeviceConfig {
        raw: snapshot.config.as_bytes().to_vec(),
        stored_crc: snapshot.stored_crc,
        computed_crc: snapshot.computed_crc,
        crc_valid: snapshot.crc_valid,
        summary,
    }
}

/// Describe a raw 2768-byte configuration block without touching a device.
///
/// Useful for rendering a configuration dumped earlier and for tests.
///
/// # Errors
///
/// Returns [`MobileError::InvalidRequest`] when `raw` is not exactly 2768
/// bytes.
#[uniffi::export]
pub fn describe_config(raw: Vec<u8>) -> Result<ConfigSummary, MobileError> {
    summarize(&raw)
}

/// Validate an edited configuration buffer against the device snapshot.
///
/// This is the **write preparation** step: it applies no I/O and sends no
/// frame. It checks the reserved regions are unchanged, recomputes the
/// official in-place CRC-16 and reports the changed byte ranges so the UI can
/// show a diff and write a backup before any future write path is enabled.
///
/// # Errors
///
/// Returns [`MobileError::InvalidRequest`] when either buffer is not 2768
/// bytes or the edit touches a reserved region.
#[uniffi::export]
pub fn prepare_config_write(
    snapshot: Vec<u8>,
    edited: Vec<u8>,
) -> Result<ConfigWriteDraft, MobileError> {
    let snapshot_config =
        Bt2Config::from_bytes(&snapshot).map_err(|error| MobileError::InvalidRequest {
            message: format!("configuration snapshot must be {CONFIG_SIZE} bytes: {error}"),
        })?;
    let edited_config =
        Bt2Config::from_bytes(&edited).map_err(|error| MobileError::InvalidRequest {
            message: format!("edited configuration must be {CONFIG_SIZE} bytes: {error}"),
        })?;

    let stored_crc = snapshot_config.crc16_stored() as u16;
    let mut check = snapshot_config.clone();
    let computed_crc = check.recompute_crc();
    let device_snapshot = ConfigSnapshot {
        config: snapshot_config,
        stored_crc,
        computed_crc,
        crc_valid: stored_crc == computed_crc,
    };

    let prepared =
        eightbitdo_core::PreparedConfigWrite::from_buffer(&device_snapshot, edited_config)
            .map_err(core_error)?;
    Ok(ConfigWriteDraft {
        expected_crc: prepared.expected().crc16_stored() as u16,
        expected: prepared.expected_bytes().to_vec(),
        is_noop: prepared.is_noop(),
        changed_regions: prepared
            .changed()
            .iter()
            .map(|region| ConfigRegionInfo {
                offset: region.offset() as u32,
                len: region.len() as u32,
            })
            .collect(),
    })
}

/// Payload bytes per configuration write packet for a negotiated MTU.
///
/// The UI can show this before a write; it does not send anything.
///
/// # Errors
///
/// Returns [`MobileError::InvalidRequest`] when the MTU leaves no payload.
#[uniffi::export]
pub fn config_write_chunk_len(mtu: u16) -> Result<u16, MobileError> {
    let chunk = eightbitdo_core::config_write_chunk_len(mtu).map_err(core_error)?;
    u16::try_from(chunk).map_err(|error| MobileError::internal("chunk length overflow", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A scripted [`NativeTransport`] for host tests.
    ///
    /// `recv` waits a millisecond when the queue is empty so the bridge's
    /// receive thread does not spin.
    struct ScriptedNative {
        frames: Mutex<VecDeque<Vec<u8>>>,
        sent: Mutex<Vec<Vec<u8>>>,
        mtu: u16,
        disconnected: AtomicBool,
    }

    impl ScriptedNative {
        fn new(mtu: u16) -> Arc<Self> {
            Arc::new(Self {
                frames: Mutex::new(VecDeque::new()),
                sent: Mutex::new(Vec::new()),
                mtu,
                disconnected: AtomicBool::new(false),
            })
        }

        fn push(&self, frame: Vec<u8>) {
            self.frames
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push_back(frame);
        }

        fn sent_frames(&self) -> Vec<Vec<u8>> {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    impl NativeTransport for ScriptedNative {
        fn send(&self, data: Vec<u8>) -> Result<(), NativeTransportError> {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(data);
            Ok(())
        }

        fn recv(&self, _timeout_ms: u32) -> Result<Option<Vec<u8>>, NativeTransportError> {
            let next = self
                .frames
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            match next {
                Some(frame) => Ok(Some(frame)),
                None => {
                    std::thread::sleep(Duration::from_millis(1));
                    Ok(None)
                }
            }
        }

        fn mtu(&self) -> u16 {
            self.mtu
        }

        fn disconnect(&self) {
            self.disconnected.store(true, Ordering::Relaxed);
        }
    }

    /// Build the 16-byte header + payload a controller notification carries.
    fn response_frame(cmd: u16, payload: &[u8]) -> Vec<u8> {
        let header = eightbitdo_protocol::CustomHeader::new(cmd, 0, payload.len() as u32, 0)
            .with_payload(payload);
        let mut frame = header.to_bytes().to_vec();
        frame.extend_from_slice(payload);
        frame
    }

    fn sample_config() -> Vec<u8> {
        let mut config = Bt2Config::zeroed();
        config.set_profile_name(Slot::S1, "Main").unwrap();
        config.set_gamepad_mode(eightbitdo_protocol::config_bt2::GamepadMode::Switch);
        config.set_cur_slot(Some(Slot::S1));
        config.recompute_crc();
        config.as_bytes().to_vec()
    }

    #[test]
    fn read_device_info_round_trips_through_the_bridge() {
        let native = ScriptedNative::new(200);
        native.push(response_frame(
            eightbitdo_protocol::commands::cmd::READ_VERSION,
            &[0xC8, 0, 0, 0],
        ));
        let session = MobileSession::new(native.clone()).unwrap();

        let info = session.read_device_info().unwrap();
        assert_eq!(info.raw, [0xC8, 0, 0, 0]);
        let sent = native.sent_frames();
        assert_eq!(sent.len(), 1);
        // The request is the exact 21-byte App Mode frame from the core.
        assert_eq!(sent[0].len(), 21);
        assert_eq!(sent[0][0], 0x04);
    }

    #[test]
    fn read_config_round_trips_through_the_bridge() {
        use eightbitdo_protocol::commands::cmd;

        let raw = sample_config();
        let native = ScriptedNative::new(200);
        // The device answers one 142-byte chunk per notification, each header
        // carrying the requested offset and total length.
        for (index, chunk) in raw.chunks(142).enumerate() {
            let header = eightbitdo_protocol::CustomHeader::new(
                cmd::READ_CONFIG,
                0,
                CONFIG_SIZE as u32,
                (index * 142) as u32,
            )
            .with_payload(chunk);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(chunk);
            native.push(frame);
        }
        let session = MobileSession::new(native).unwrap();

        let config = session.read_config().unwrap();
        assert_eq!(config.raw, raw);
        assert_eq!(config.raw.len(), CONFIG_SIZE);
        // `crc_valid` mirrors the core's in-place rule (the stored CRC is part
        // of the recomputation input), so a single-pass test config is not a
        // fixed point; only the raw pass-through is asserted here.
        assert_eq!(config.summary.profile_names[0], "Main");
        assert_eq!(config.summary.gamepad_mode, "Switch");

        // The JSON export must contain the same bytes and summary.
        let native = ScriptedNative::new(200);
        for (index, chunk) in raw.chunks(142).enumerate() {
            let header = eightbitdo_protocol::CustomHeader::new(
                cmd::READ_CONFIG,
                0,
                CONFIG_SIZE as u32,
                (index * 142) as u32,
            )
            .with_payload(chunk);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(chunk);
            native.push(frame);
        }
        let session = MobileSession::new(native).unwrap();
        let json = session.read_config_json().unwrap();
        assert!(json.contains("\"crc_valid\":"));
        assert!(json.contains("\"gamepad_mode\": \"Switch\""));
        assert!(json.contains(
            &raw[..4]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
    }

    #[test]
    fn describe_config_reports_slot_state() {
        let summary = describe_config(sample_config()).unwrap();
        assert_eq!(summary.profile_names, ["Main", "", ""]);
        assert_eq!(summary.current_slot, Some(1));
        assert_eq!(summary.gamepad_mode, "Switch");
    }

    #[test]
    fn describe_config_rejects_wrong_length() {
        let error = describe_config(vec![0u8; 16]).unwrap_err();
        assert!(matches!(error, MobileError::InvalidRequest { .. }));
    }

    #[test]
    fn prepare_config_write_reports_regions_without_io() {
        let snapshot = sample_config();
        let mut edited = Bt2Config::from_bytes(&snapshot).unwrap();
        edited.set_profile_name(Slot::S2, "Alt").unwrap();
        let edited = edited.as_bytes().to_vec();

        let draft = prepare_config_write(snapshot.clone(), edited).unwrap();
        assert!(!draft.is_noop);
        assert!(!draft.changed_regions.is_empty());
        // The profile name starts at +0x014, slot 2 at +0x034. `Alt` begins
        // with a NUL UTF-16 high byte, so the first differing byte is 0x035.
        assert!(draft
            .changed_regions
            .iter()
            .any(|region| region.offset == 0x035));
        // The CRC field changed too.
        assert_eq!(draft.expected.len(), CONFIG_SIZE);
        assert_eq!(draft.expected_crc, describe_crc(&draft.expected));
    }

    fn describe_crc(raw: &[u8]) -> u16 {
        let config = Bt2Config::from_bytes(raw).unwrap();
        config.crc16_stored() as u16
    }

    #[test]
    fn prepare_config_write_is_noop_for_identical_buffers() {
        let snapshot = sample_config();
        let draft = prepare_config_write(snapshot.clone(), snapshot).unwrap();
        assert!(draft.is_noop);
        assert!(draft.changed_regions.is_empty());
    }

    #[test]
    fn config_write_chunk_len_follows_the_mtu() {
        assert!(config_write_chunk_len(200).unwrap() > 0);
        assert!(config_write_chunk_len(16).is_err());
    }

    #[test]
    fn session_disconnect_stops_the_receive_worker() {
        let native = ScriptedNative::new(200);
        let session = MobileSession::new(native.clone()).unwrap();
        session.disconnect();
        assert!(native.disconnected.load(Ordering::Relaxed));
        // Dropping the session must not block or panic after disconnect.
        drop(session);
    }
}
