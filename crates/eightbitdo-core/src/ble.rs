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

use crate::transport::Transport;
use crate::{
    Error, APP_BLE_NAME, APP_CHARACTERISTIC_UUID, APP_CHARACTERISTIC_UUID_ALT, APP_SERVICE_UUID,
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
}

impl Default for BleOptions {
    fn default() -> Self {
        Self {
            name: APP_BLE_NAME.to_string(),
            address: None,
            scan_timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(20),
            verbose: false,
        }
    }
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
    /// [`BleOptions::scan_timeout`], or [`Error::CharacteristicNotFound`] when
    /// service `FF10` has no usable App Mode characteristic.
    pub async fn connect(options: &BleOptions) -> Result<Self, Error> {
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
        let found = find_app_peripheral(&adapter, options).await;
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
            .find(|service| service.uuid == APP_SERVICE_UUID)
        {
            Some(service) => service,
            None => {
                let _ = peripheral.disconnect().await;
                return Err(Error::Ble(format!(
                    "App Mode service {APP_SERVICE_UUID} was not found"
                )));
            }
        };
        let characteristic = match choose_characteristic(&service.characteristics) {
            Some(characteristic) => characteristic,
            None => {
                let _ = peripheral.disconnect().await;
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
        // Best effort: notify teardown and link teardown are both idempotent
        // from the caller's point of view.
        let _ = self.peripheral.unsubscribe(&self.characteristic).await;
        let _ = self.peripheral.disconnect().await;
        Ok(())
    }
}

/// Scan for BLE devices for `timeout` and return everything found.
///
/// The scan is stopped before returning, including on error.
///
/// # Errors
///
/// Returns [`Error::Ble`] when no adapter is available or the backend fails.
pub async fn scan_devices(timeout: Duration) -> Result<Vec<BleDevice>, Error> {
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

async fn find_app_peripheral(
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
) -> Option<Characteristic> {
    [APP_CHARACTERISTIC_UUID, APP_CHARACTERISTIC_UUID_ALT]
        .iter()
        .find_map(|uuid| {
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
}
