//! BLE GATT [`Transport`] backed by `btleplug`.
//!
//! The controller exposes the App Mode service `FF10` with a single
//! write/notify characteristic. The documented UUID is `FF11`, but some units
//! (including the Ultimate 2 Bluetooth unit used for hardware observation)
//! expose `FF13` instead, so both are resolved, `FF11` first.
//!
//! This module only performs read-only traffic. Write commands, flash and OTA
//! live in later phases.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use tokio::sync::{mpsc, Mutex};
use tokio::time::Instant;
use uuid::Uuid;

use crate::ble_lock::BleLock;
use crate::transport::Transport;
use crate::{
    Error, APP_BLE_NAME, APP_CHARACTERISTIC_UUID, APP_CHARACTERISTIC_UUID_ALT, APP_SERVICE_UUID,
    BOOT_BLE_NAME, BOOT_CHARACTERISTIC_UUID, BOOT_CHARACTERISTIC_UUID_ALT, BOOT_SERVICE_UUID,
};

/// How often the scan loop refreshes the peripheral list.
const SCAN_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Depth of the notification forwarding channel.
const NOTIFICATION_CHANNEL_DEPTH: usize = 64;

/// Default ATT MTU used when the BLE stack reports 0 or has not negotiated a
/// larger MTU.
const DEFAULT_ATT_MTU: u16 = 23;

/// Upper bound for a single GATT write operation.
const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Options for finding and connecting to an App Mode peripheral.
#[derive(Debug, Clone)]
pub struct BleOptions {
    /// Advertised local name to match. Defaults to [`APP_BLE_NAME`] (`"80ND"`).
    pub name: String,
    /// Optional Bluetooth address to match in addition to the name.
    ///
    /// Colons and dashes are ignored, so `AA:BB:CC:DD:EE:FF` and
    /// `aabbccddeeff` are equivalent.
    pub address: Option<String>,
    /// How long to scan before giving up.
    pub scan_timeout: Duration,
    /// Connection and service-discovery timeout.
    pub connect_timeout: Duration,
    /// Log every raw TX and RX frame to stderr.
    pub verbose: bool,
    /// Override the single-owner BLE lock path.
    ///
    /// `None` (the default) uses [`crate::default_ble_lock_path`]. Tests and
    /// wrappers set this to keep a temporary or per-user lock; see
    /// [`crate::BleLock`].
    pub lock_path: Option<PathBuf>,
}

impl Default for BleOptions {
    fn default() -> Self {
        Self {
            name: APP_BLE_NAME.to_string(),
            address: None,
            scan_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(20),
            verbose: false,
            lock_path: None,
        }
    }
}

/// Acquire the single-owner BLE lock for an operation.
///
/// `None` is the production default path and therefore uses the **enforcing**
/// parent-directory policy ([`BleLock::acquire`]); `Some` is an explicit test
/// or tool override ([`BleLock::acquire_at`]). Keeping the choice here makes
/// sure the default path is never opened with the override policy.
fn acquire_ble_lock(lock_path: Option<&Path>) -> Result<BleLock, Error> {
    match lock_path {
        Some(path) => BleLock::acquire_at(path),
        None => BleLock::acquire(),
    }
}

/// Test seam for [`acquire_ble_lock`] that substitutes the default path while
/// keeping the production policy (enforcing) for `None`.
#[cfg(test)]
fn acquire_ble_lock_with_default(
    lock_path: Option<&Path>,
    default_path: PathBuf,
) -> Result<BleLock, Error> {
    match lock_path {
        Some(path) => BleLock::acquire_at(path),
        None => BleLock::acquire_enforcing(default_path),
    }
}

/// Options for finding and connecting to the BLE bootloader (`"8bt"`).
///
/// The advertised name is fixed to [`BOOT_BLE_NAME`]; only the address filter
/// and timeouts are configurable.
#[derive(Debug, Clone)]
pub struct BleBootOptions {
    /// Optional Bluetooth address to match in addition to the `"8bt"` name.
    pub address: Option<String>,
    /// How long to scan before giving up.
    pub scan_timeout: Duration,
    /// Connection and service-discovery timeout.
    pub connect_timeout: Duration,
    /// Log every raw TX and RX frame to stderr.
    pub verbose: bool,
    /// Override the single-owner BLE lock path.
    ///
    /// `None` (the default) uses the production lock path. The boot link takes
    /// the same lock as the App Mode link, so an OTA cannot race a resident
    /// `session` / GUI for the adapter.
    pub lock_path: Option<PathBuf>,
}

impl Default for BleBootOptions {
    fn default() -> Self {
        Self {
            address: None,
            scan_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(20),
            verbose: false,
            lock_path: None,
        }
    }
}

/// GATT identity of one BLE firmware mode.
#[derive(Debug, Clone, Copy)]
struct BleMode {
    label: &'static str,
    service_uuid: Uuid,
    characteristics: [Uuid; 2],
    is_boot: bool,
}

impl BleMode {
    const APP: Self = Self {
        label: "App Mode",
        service_uuid: APP_SERVICE_UUID,
        characteristics: [APP_CHARACTERISTIC_UUID, APP_CHARACTERISTIC_UUID_ALT],
        is_boot: false,
    };

    const BOOT: Self = Self {
        label: "Bootloader",
        service_uuid: BOOT_SERVICE_UUID,
        characteristics: [BOOT_CHARACTERISTIC_UUID, BOOT_CHARACTERISTIC_UUID_ALT],
        is_boot: true,
    };
}

/// One device found during a scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleDevice {
    /// Advertised name, when the platform exposes one.
    pub name: Option<String>,
    /// Bluetooth address in platform display form.
    pub address: String,
    /// Latest received signal strength in dBm.
    pub rssi: Option<i16>,
    /// True when the device looks like an App Mode peripheral.
    ///
    /// This is set for the `"80ND"` name and for units advertising the
    /// `FF10` service.
    pub is_app: bool,
}

/// A connected BLE App Mode link.
pub struct BleTransport {
    peripheral: Peripheral,
    characteristic: Characteristic,
    write_type: WriteType,
    mtu: u16,
    write_timeout: Duration,
    name: Option<String>,
    verbose: bool,
    rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    /// Single-owner lock, held from before the scan until disconnect/drop.
    ///
    /// Wrapped in a mutex so [`Transport::disconnect`] can release it before
    /// the value is dropped; a process exit releases it through the kernel
    /// regardless. Declared last so it is the last field dropped: the
    /// peripheral/notification fields go first and another process cannot take
    /// the lock while this struct is still tearing down.
    ble_lock: std::sync::Mutex<Option<BleLock>>,
}

impl BleTransport {
    /// Scan for, connect to, and subscribe to an App Mode peripheral.
    ///
    /// Resolution order for the characteristic inside service `FF10` is
    /// `FF11`, then `FF13`. The chosen characteristic must support a write
    /// mode and notify/indicate.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Ble`] for adapter/backend failures,
    /// [`Error::DeviceNotFound`] when no matching peripheral appears within
    /// [`BleOptions::scan_timeout`], [`Error::CharacteristicNotFound`] when
    /// service `FF10` has no usable App Mode characteristic, or
    /// [`Error::BleBusy`] when another process holds the single-owner BLE lock.
    ///
    /// The lock is taken before the adapter is touched and is held for the
    /// lifetime of the returned transport: a resident `session`, a GUI and a
    /// one-shot `scan` cannot scan or reconnect over each other.
    pub async fn connect(options: &BleOptions) -> Result<Self, Error> {
        Self::connect_mode(options, BleMode::APP).await
    }

    /// Shared connect path for App Mode and the `"8bt"` bootloader.
    ///
    /// The single-owner BLE lock is taken before the adapter is touched and is
    /// held for the lifetime of the returned transport, including the boot
    /// phase (the boot transport passes its own `lock_path`).
    async fn connect_mode(options: &BleOptions, mode: BleMode) -> Result<Self, Error> {
        let lock = acquire_ble_lock(options.lock_path.as_deref())?;
        let manager = Manager::new().await.map_err(ble_error)?;
        let adapters = manager.adapters().await.map_err(ble_error)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| Error::Ble("no Bluetooth adapter found".to_string()))?;

        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(ble_error)?;
        let found = find_peripheral(&adapter, options).await;
        let _ = adapter.stop_scan().await;
        let found = found?;

        let (peripheral, name) = found.ok_or_else(|| Error::DeviceNotFound {
            name: options.name.clone(),
            address: options.address.clone(),
        })?;

        peripheral
            .connect_with_timeout(options.connect_timeout)
            .await
            .map_err(ble_error)?;
        if let Err(error) = peripheral
            .discover_services_with_timeout(options.connect_timeout)
            .await
        {
            let _ = peripheral.disconnect().await;
            return Err(ble_error(error));
        }

        let service = match peripheral
            .services()
            .into_iter()
            .find(|service| service.uuid == mode.service_uuid)
        {
            Some(service) => service,
            None => {
                let _ = peripheral.disconnect().await;
                return Err(Error::Ble(format!(
                    "{} service {} was not found",
                    mode.label, mode.service_uuid
                )));
            }
        };
        let characteristic =
            match choose_characteristic(&service.characteristics, &mode.characteristics) {
                Some(characteristic) => characteristic,
                None => {
                    let _ = peripheral.disconnect().await;
                    if mode.is_boot {
                        return Err(Error::Ble(format!(
                            "{} characteristic FF21/FF23 was not found on service {}",
                            mode.label, mode.service_uuid
                        )));
                    }
                    return Err(Error::CharacteristicNotFound);
                }
            };
        let write_type = if characteristic.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else {
            WriteType::WithoutResponse
        };

        if let Err(error) = peripheral.subscribe(&characteristic).await {
            let _ = peripheral.disconnect().await;
            return Err(ble_error(error));
        }
        let stream = match peripheral.notifications().await {
            Ok(stream) => stream,
            Err(error) => {
                let _ = peripheral.disconnect().await;
                return Err(ble_error(error));
            }
        };

        let (tx, rx) = mpsc::channel(NOTIFICATION_CHANNEL_DEPTH);
        let characteristic_uuid = characteristic.uuid;
        let verbose = options.verbose;
        tokio::spawn(async move {
            let mut stream = stream;
            while let Some(notification) = stream.next().await {
                if notification.uuid != characteristic_uuid {
                    continue;
                }
                if verbose {
                    eprintln!(
                        "rx {} bytes: {}",
                        notification.value.len(),
                        hex_dump(&notification.value)
                    );
                }
                if tx.send(notification.value).await.is_err() {
                    break;
                }
            }
        });

        let mtu = match peripheral.mtu() {
            0 => DEFAULT_ATT_MTU,
            mtu => mtu,
        };

        Ok(Self {
            peripheral,
            characteristic,
            write_type,
            mtu,
            // Bound a single GATT write so Ctrl-C/shutdown can never wait
            // forever on a wedged BLE stack.
            write_timeout: compute_write_timeout(options.connect_timeout),
            name,
            verbose,
            rx: Mutex::new(rx),
            ble_lock: std::sync::Mutex::new(Some(lock)),
        })
    }

    /// Bluetooth address of the connected peripheral.
    #[must_use]
    pub fn address(&self) -> String {
        self.peripheral.address().to_string()
    }

    /// Advertised name captured during the scan, if any.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// UUID of the characteristic used for writes and notifications.
    ///
    /// This is `FF11` on most units and `FF13` on the observed unit.
    #[must_use]
    pub fn characteristic_uuid(&self) -> Uuid {
        self.characteristic.uuid
    }
}

/// A connected BLE bootloader link (`"8bt"`, service `FF20`, characteristic
/// `FF21` or `FF23`).
///
/// The App Mode [`BleTransport`] is fixed to `"80ND"` / service `FF10` and can
/// never reach the bootloader, so it is **not** an
/// [`OtaTransport`](crate::ota::OtaTransport). This type is the boot-only BLE
/// transport for the OTA phase.
pub struct BleBootTransport {
    inner: BleTransport,
}

impl BleBootTransport {
    /// Scan for, connect to, and subscribe to a `"8bt"` bootloader peripheral.
    ///
    /// The advertised name is fixed to [`BOOT_BLE_NAME`]; `options.address`,
    /// timeouts and verbose logging are honored. Service `FF20` is resolved
    /// with characteristic `FF21` first, then `FF23`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Ble`] for adapter/backend failures or a missing
    /// `FF20`/`FF21`/`FF23` characteristic, or [`Error::DeviceNotFound`] when
    /// no `"8bt"` peripheral appears within
    /// [`BleBootOptions::scan_timeout`].
    pub async fn connect(options: &BleBootOptions) -> Result<Self, Error> {
        let mode = BleMode::BOOT;
        let app_options = BleOptions {
            name: BOOT_BLE_NAME.to_string(),
            address: options.address.clone(),
            scan_timeout: options.scan_timeout,
            connect_timeout: options.connect_timeout,
            verbose: options.verbose,
            lock_path: options.lock_path.clone(),
        };
        let inner = BleTransport::connect_mode(&app_options, mode).await?;
        Ok(Self { inner })
    }

    /// Bluetooth address of the connected bootloader peripheral.
    #[must_use]
    pub fn address(&self) -> String {
        self.inner.address()
    }

    /// Advertised name captured during the scan, if any.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.inner.name()
    }

    /// UUID of the bootloader write/notify characteristic.
    ///
    /// This is `FF21` on most units and `FF23` on `_CY` units.
    #[must_use]
    pub fn characteristic_uuid(&self) -> Uuid {
        self.inner.characteristic_uuid()
    }
}

#[async_trait]
impl Transport for BleBootTransport {
    async fn send(&self, data: &[u8]) -> Result<(), Error> {
        self.inner.send(data).await
    }

    async fn recv(&self) -> Result<Vec<u8>, Error> {
        self.inner.recv().await
    }

    fn mtu(&self) -> u16 {
        self.inner.mtu()
    }

    async fn disconnect(&self) -> Result<(), Error> {
        self.inner.disconnect().await
    }
}

#[async_trait]
impl Transport for BleTransport {
    async fn send(&self, data: &[u8]) -> Result<(), Error> {
        if let MtuCheck::DelegateToStack { max_frame } = check_tx_frame_len(self.mtu, data.len())? {
            if self.verbose {
                eprintln!(
                    "warn: frame is {} bytes with unnegotiated/default MTU {} (single-write payload {}); delegating to BLE stack",
                    data.len(),
                    self.mtu,
                    max_frame
                );
            }
        }
        if self.verbose {
            eprintln!("tx {} bytes: {}", data.len(), hex_dump(data));
        }
        let write = self
            .peripheral
            .write(&self.characteristic, data, self.write_type);
        match tokio::time::timeout(self.write_timeout, write).await {
            Ok(result) => result.map_err(ble_error),
            Err(_) => Err(Error::Ble(format!(
                "BLE write timed out after {:.1}s",
                self.write_timeout.as_secs_f64()
            ))),
        }
    }

    async fn recv(&self) -> Result<Vec<u8>, Error> {
        self.rx.lock().await.recv().await.ok_or(Error::Disconnected)
    }

    fn mtu(&self) -> u16 {
        self.mtu
    }

    async fn disconnect(&self) -> Result<(), Error> {
        // Notify teardown and link teardown are both attempted so the error
        // reports every failed step. The single-owner lock is released only
        // when both succeed: after a partial teardown the old link may still
        // be alive, and letting another process scan over it is exactly the
        // contention this lock exists to prevent.
        let mut failures = Vec::new();
        if let Err(error) = self.peripheral.unsubscribe(&self.characteristic).await {
            failures.push(format!("unsubscribe: {error}"));
        }
        if let Err(error) = self.peripheral.disconnect().await {
            failures.push(format!("disconnect: {error}"));
        }
        let teardown = if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Ble(failures.join("; ")))
        };
        release_ble_lock_after_teardown(&self.ble_lock, &teardown)
    }
}

/// Release the BLE lock only after a fully successful teardown.
///
/// A failed or timed-out teardown returns `Err` and keeps the lock in the
/// transport: the link may still be half-closed, so another process must not
/// start scanning. The kernel still releases the lock when the transport is
/// dropped or the process exits; the CLI prints a warning in that case.
fn release_ble_lock_after_teardown(
    lock: &std::sync::Mutex<Option<BleLock>>,
    teardown: &Result<(), Error>,
) -> Result<(), Error> {
    match teardown {
        Ok(()) => {
            if let Ok(mut lock) = lock.lock() {
                let _ = lock.take();
            }
            Ok(())
        }
        Err(error) => Err(Error::Ble(format!(
            "BLE teardown incomplete ({error}); keeping the single-owner BLE lock while this \
             transport is alive so another process cannot scan over a half-closed link"
        ))),
    }
}

/// Scan for BLE devices for `timeout` and return everything found.
///
/// The scan is stopped before returning, including on error.
///
/// # Errors
///
/// Returns [`Error::Ble`] when no adapter is available or the backend fails,
/// and [`Error::BleBusy`] when another process (for example a resident
/// `session`) holds the single-owner BLE lock.
pub async fn scan_devices(timeout: Duration) -> Result<Vec<BleDevice>, Error> {
    scan_devices_with_lock_path(timeout, None).await
}

/// [`scan_devices`] with an explicit lock path override.
///
/// `None` uses [`crate::default_ble_lock_path`]. Exposed so tests and wrappers
/// can share a private lock without touching the developer's real one.
///
/// # Errors
///
/// Same as [`scan_devices`].
pub async fn scan_devices_with_lock_path(
    timeout: Duration,
    lock_path: Option<PathBuf>,
) -> Result<Vec<BleDevice>, Error> {
    // Hold the lock for the whole scan: two scans at once are what makes the
    // controller report `In Progress` on the next connect.
    let _lock = acquire_ble_lock(lock_path.as_deref())?;
    let manager = Manager::new().await.map_err(ble_error)?;
    let adapters = manager.adapters().await.map_err(ble_error)?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| Error::Ble("no Bluetooth adapter found".to_string()))?;

    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(ble_error)?;
    let result = collect_devices(&adapter, timeout).await;
    let _ = adapter.stop_scan().await;
    result
}

async fn collect_devices(adapter: &Adapter, timeout: Duration) -> Result<Vec<BleDevice>, Error> {
    let deadline = checked_deadline(timeout)?;
    let mut devices: BTreeMap<String, BleDevice> = BTreeMap::new();

    loop {
        for peripheral in adapter.peripherals().await.map_err(ble_error)? {
            let Ok(Some(properties)) = peripheral.properties().await else {
                continue;
            };
            let address = peripheral.address().to_string();
            let name = properties
                .local_name
                .clone()
                .or(properties.advertisement_name.clone());
            let is_app = name.as_deref() == Some(APP_BLE_NAME)
                || properties.services.contains(&APP_SERVICE_UUID);
            devices.insert(
                address.clone(),
                BleDevice {
                    name,
                    address,
                    rssi: properties.rssi,
                    is_app,
                },
            );
        }

        let now = Instant::now();
        if now >= deadline {
            break;
        }
        tokio::time::sleep((deadline - now).min(SCAN_POLL_INTERVAL)).await;
    }

    Ok(devices.into_values().collect())
}

async fn find_peripheral(
    adapter: &Adapter,
    options: &BleOptions,
) -> Result<Option<(Peripheral, Option<String>)>, Error> {
    let deadline = checked_deadline(options.scan_timeout)?;

    loop {
        for peripheral in adapter.peripherals().await.map_err(ble_error)? {
            let Ok(Some(properties)) = peripheral.properties().await else {
                continue;
            };
            if !matches_name(&properties, &options.name) {
                continue;
            }
            if let Some(wanted) = options.address.as_deref() {
                if normalize_address(wanted) != normalize_address(&peripheral.address().to_string())
                {
                    continue;
                }
            }
            let name = properties.local_name.or(properties.advertisement_name);
            return Ok(Some((peripheral, name)));
        }

        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        tokio::time::sleep((deadline - now).min(SCAN_POLL_INTERVAL)).await;
    }
}

fn matches_name(properties: &btleplug::api::PeripheralProperties, wanted: &str) -> bool {
    properties.local_name.as_deref() == Some(wanted)
        || properties.advertisement_name.as_deref() == Some(wanted)
}

fn normalize_address(address: &str) -> String {
    address
        .chars()
        .filter(char::is_ascii_hexdigit)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn choose_characteristic(
    characteristics: &std::collections::BTreeSet<Characteristic>,
    uuids: &[Uuid],
) -> Option<Characteristic> {
    uuids.iter().find_map(|uuid| {
        characteristics
            .iter()
            .find(|characteristic| {
                characteristic.uuid == *uuid && supports_app_traffic(characteristic.properties)
            })
            .cloned()
    })
}

fn supports_app_traffic(properties: CharPropFlags) -> bool {
    let can_write = properties.contains(CharPropFlags::WRITE)
        || properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE);
    let can_notify =
        properties.contains(CharPropFlags::NOTIFY) || properties.contains(CharPropFlags::INDICATE);
    can_write && can_notify
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MtuCheck {
    /// Frame fits within a single ATT payload (`mtu - 3`).
    WithinMtu,
    /// MTU is unreported or at the default ATT minimum (`<= 23`), so frames
    /// larger than `mtu - 3` (such as the 21-byte `ReadVersion` request) are
    /// left to the OS/BlueZ Long Write path instead of being rejected locally.
    DelegateToStack { max_frame: usize },
}

fn check_tx_frame_len(mtu: u16, frame_len: usize) -> Result<MtuCheck, Error> {
    let max_frame = usize::from(mtu.saturating_sub(3));
    if frame_len <= max_frame {
        return Ok(MtuCheck::WithinMtu);
    }
    if mtu <= DEFAULT_ATT_MTU {
        return Ok(MtuCheck::DelegateToStack { max_frame });
    }
    Err(Error::Ble(format!(
        "frame is {frame_len} bytes but the negotiated MTU {mtu} allows at most {max_frame}"
    )))
}

fn compute_write_timeout(connect_timeout: Duration) -> Duration {
    DEFAULT_WRITE_TIMEOUT.min(connect_timeout)
}

/// `Instant::now() + timeout`, rejecting durations that cannot be scheduled.
///
/// `Duration::add` on `tokio::time::Instant` panics on overflow; user-supplied
/// `--scan-timeout` / `--timeout` values reach here, so overflow must become an
/// error instead of a panic.
fn checked_deadline(timeout: Duration) -> Result<Instant, Error> {
    if timeout > crate::session::MAX_WAIT {
        return Err(Error::InvalidTimeout(timeout));
    }
    Instant::now()
        .checked_add(timeout)
        .ok_or(Error::InvalidTimeout(timeout))
}

fn ble_error(error: btleplug::Error) -> Error {
    Error::Ble(error.to_string())
}

fn hex_dump(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        let _ = write!(out, "{byte:02X}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_match_the_app_mode_name() {
        let options = BleOptions::default();
        assert_eq!(options.name, "80ND");
        assert_eq!(options.address, None);
        assert_eq!(options.scan_timeout, Duration::from_secs(10));
        assert_eq!(options.connect_timeout, Duration::from_secs(20));
    }

    #[test]
    fn boot_mode_uses_the_8bt_service_and_characteristics() {
        assert_eq!(BOOT_BLE_NAME, "8bt");
        assert_eq!(BleMode::BOOT.label, "Bootloader");
        assert_eq!(BleMode::BOOT.service_uuid, BOOT_SERVICE_UUID);
        assert_eq!(
            BleMode::BOOT.characteristics,
            [BOOT_CHARACTERISTIC_UUID, BOOT_CHARACTERISTIC_UUID_ALT]
        );
        assert_eq!(BleMode::APP.service_uuid, APP_SERVICE_UUID);
        assert_ne!(BleMode::BOOT.service_uuid, BleMode::APP.service_uuid);
    }

    #[test]
    fn default_boot_options_use_the_bootloader_defaults() {
        let options = BleBootOptions::default();
        assert_eq!(options.address, None);
        assert_eq!(options.scan_timeout, Duration::from_secs(10));
        assert_eq!(options.connect_timeout, Duration::from_secs(20));
        assert!(!options.verbose);
    }

    #[test]
    fn checked_deadline_rejects_unschedulable_timeouts() {
        assert!(checked_deadline(Duration::ZERO).is_ok());
        assert!(checked_deadline(Duration::from_secs(10)).is_ok());
        // `--scan-timeout` / `--timeout` accept any u64; these must become
        // errors, not `Instant::add` panics.
        assert!(matches!(
            checked_deadline(Duration::MAX),
            Err(Error::InvalidTimeout(_))
        ));
        assert!(matches!(
            checked_deadline(crate::session::MAX_WAIT + Duration::from_nanos(1)),
            Err(Error::InvalidTimeout(_))
        ));
    }

    #[test]
    fn address_normalization_ignores_separators_and_case() {
        assert_eq!(
            normalize_address("AA:BB:CC:DD:EE:FF"),
            normalize_address("aabbccddeeff")
        );
        assert_eq!(
            normalize_address("AA-BB-CC-DD-EE-FF"),
            normalize_address("AaBbCcDdEeFf")
        );
    }

    #[test]
    fn writeable_and_notifying_characteristic_is_accepted() {
        let flags = CharPropFlags::WRITE | CharPropFlags::NOTIFY;
        assert!(supports_app_traffic(flags));
        let flags = CharPropFlags::WRITE_WITHOUT_RESPONSE | CharPropFlags::INDICATE;
        assert!(supports_app_traffic(flags));
        assert!(!supports_app_traffic(CharPropFlags::WRITE));
        assert!(!supports_app_traffic(CharPropFlags::NOTIFY));
    }

    #[test]
    fn fallback_mtu_23_allows_21_byte_version_request() {
        assert_eq!(check_tx_frame_len(23, 20).unwrap(), MtuCheck::WithinMtu);
        assert_eq!(
            check_tx_frame_len(23, 21).unwrap(),
            MtuCheck::DelegateToStack { max_frame: 20 }
        );
        assert_eq!(
            check_tx_frame_len(0, 21).unwrap(),
            MtuCheck::DelegateToStack { max_frame: 0 }
        );
    }

    #[test]
    fn negotiated_mtu_rejects_oversized_frame() {
        assert_eq!(check_tx_frame_len(24, 21).unwrap(), MtuCheck::WithinMtu);
        let err = check_tx_frame_len(24, 22).unwrap_err();
        assert!(
            matches!(err, Error::Ble(message) if message.contains("22 bytes") && message.contains("MTU 24") && message.contains("at most 21"))
        );
    }

    #[test]
    fn write_timeout_is_capped_at_five_seconds() {
        assert_eq!(
            compute_write_timeout(Duration::from_secs(20)),
            Duration::from_secs(5)
        );
        assert_eq!(
            compute_write_timeout(Duration::from_secs(2)),
            Duration::from_secs(2)
        );
    }

    fn temp_lock_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "eightbitdo-ble-{label}-{}-{}.lock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ))
    }

    #[cfg(unix)]
    fn temp_lock_dir(label: &str, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = std::env::temp_dir().join(format!(
            "eightbitdo-ble-dir-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        std::fs::create_dir(&directory).expect("create lock parent");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(mode))
            .expect("set lock parent mode");
        directory
    }

    #[cfg(unix)]
    #[test]
    fn production_lock_path_enforces_the_parent_policy() {
        // `lock_path: None` (production) must use the enforcing policy: a
        // 0777 parent is refused, even though an explicit override accepts it.
        let public_parent = temp_lock_dir("production-public", 0o777);
        let public_path = public_parent.join(crate::BLE_LOCK_FILE_NAME);
        let error = acquire_ble_lock_with_default(None, public_path.clone())
            .expect_err("the production default must reject a 0777 parent");
        assert!(
            matches!(error, Error::BleLockInvalid { ref reason, .. }
                if reason.contains("group/other-writable")),
            "expected a parent-mode rejection, got {error:?}"
        );

        let guard = acquire_ble_lock_with_default(Some(&public_path), public_path.clone())
            .expect("an explicit override skips the parent policy");
        drop(guard);

        let private_parent = temp_lock_dir("production-private", 0o700);
        let private_path = private_parent.join(crate::BLE_LOCK_FILE_NAME);
        let guard = acquire_ble_lock_with_default(None, private_path)
            .expect("the production default accepts a private parent");
        drop(guard);

        let _ = std::fs::remove_file(&public_path);
        let _ = std::fs::remove_dir_all(&public_parent);
        let _ = std::fs::remove_dir_all(&private_parent);
    }

    #[test]
    fn connect_options_default_to_the_shared_lock() {
        assert_eq!(BleOptions::default().lock_path, None);
    }

    #[tokio::test]
    async fn connect_is_refused_while_another_process_holds_the_lock() {
        let path = temp_lock_path("connect-busy");
        let _held = BleLock::acquire_at(&path).expect("hold the lock");
        let options = BleOptions {
            lock_path: Some(path.clone()),
            ..BleOptions::default()
        };
        let error = match BleTransport::connect(&options).await {
            Ok(_) => panic!("connect must refuse while the lock is held"),
            Err(error) => error,
        };
        assert!(
            matches!(error, Error::BleBusy { path: ref held } if held == &path),
            "expected BleBusy before the adapter is touched, got {error:?}"
        );
        drop(_held);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn scan_is_refused_while_an_external_flock_is_held() {
        use fs2::FileExt;

        let path = temp_lock_path("scan-busy");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .expect("open lock file");
        // `flock(1)`-style holder: the CLI must report busy, not block, and it
        // must not touch the adapter.
        FileExt::try_lock_exclusive(&file).expect("external flock");
        let error = scan_devices_with_lock_path(Duration::from_millis(1), Some(path.clone()))
            .await
            .expect_err("scan must refuse while the lock is held");
        assert!(
            matches!(error, Error::BleBusy { path: ref held } if held == &path),
            "expected BleBusy, got {error:?}"
        );
        FileExt::unlock(&file).expect("unlock");
        drop(file);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn failed_teardown_keeps_the_ble_lock() {
        let path = temp_lock_path("teardown-failure");
        let lock = std::sync::Mutex::new(Some(BleLock::acquire_at(&path).expect("hold the lock")));
        let failed: Result<(), Error> = Err(Error::Ble("disconnect: timed out".to_string()));

        let error = release_ble_lock_after_teardown(&lock, &failed)
            .expect_err("a failed teardown must not release the lock");
        assert!(
            error
                .to_string()
                .contains("keeping the single-owner BLE lock"),
            "the failure must explain that the lock is kept: {error}"
        );
        assert!(lock.lock().expect("lock mutex").is_some());
        let busy = BleLock::acquire_at(&path).expect_err("another process must still be refused");
        assert!(matches!(busy, Error::BleBusy { .. }), "got {busy:?}");

        // A successful retry releases it.
        release_ble_lock_after_teardown(&lock, &Ok(())).expect("successful teardown releases");
        let reacquired = BleLock::acquire_at(&path).expect("lock must be free after teardown");
        drop(reacquired);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn successful_teardown_releases_the_ble_lock() {
        let path = temp_lock_path("teardown-ok");
        let lock = std::sync::Mutex::new(Some(BleLock::acquire_at(&path).expect("hold the lock")));
        release_ble_lock_after_teardown(&lock, &Ok(())).expect("release");
        assert!(lock.lock().expect("lock mutex").is_none());
        let reacquired = BleLock::acquire_at(&path).expect("lock must be free");
        drop(reacquired);
        let _ = std::fs::remove_file(&path);
    }
}
