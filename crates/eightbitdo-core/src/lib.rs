//! Device I/O for the **8BitDo Ultimate 2 Bluetooth Controller** (`0x600F`).
//!
//! This crate sits between the pure protocol codec in `eightbitdo-protocol`
//! and the platform UI. It defines the [`Transport`] seam, a session that
//! performs the App Mode handshake and transfers, and a BLE implementation
//! backed by `btleplug`.
//!
//! The first slice was read-only (version, configuration and realtime input).
//! The current core also has macro flash read/write APIs (`macro_ops`) and the
//! full 2768-byte configuration write path (`config_ops`); both write paths are
//! fail-closed and currently mock-tested only. OTA is still a separate phase.
//!
//! Button remapping (`MapKeyRecord`) is implemented in [`config_ops`] from the
//! official binary layout: [`config_ops::ConfigEdit::ButtonRemap`] changes one
//! 4-byte `key[]` entry and [`config_ops::map_key_flag`] reads the record flag.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config_ops;
pub mod error;
pub mod macro_ops;
pub mod mock;
pub mod session;
pub mod transport;

#[cfg(feature = "ble")]
pub mod ble;
#[cfg(feature = "ble")]
pub mod ble_lock;

pub use config_ops::{
    button_mapping, button_mappings, config_write_chunk_len, map_key_flag, ConfigCrcStatus,
    ConfigEdit, ConfigRegion, ConfigWriteOutcome, ConfigWritePlan, ConfigWriteTimeouts,
    PreparedConfigWrite, CONFIG_SAVE_TIMEOUT, CONFIG_VERIFY_DELAY, CONFIG_WRITE_TIMEOUT,
    OFFICIAL_WRITE_CHUNK,
};
pub use error::Error;
pub use error::{ConfigWriteStage, ConfigWriteState, MacroWriteStage, MacroWriteState};
pub use macro_ops::{
    macro_header_chunk_len, macro_write_chunk_len, macro_write_chunk_len_requested,
    max_protocol_payload, validate_macro_chunk_size, MacroBackup, MacroEntry, MacroHeaderOutcome,
    MacroSlotWriteOutcome, MacroTimeouts, MacroWriteOutcome, MacroWritePlan,
    MACRO_HEADER_CHUNK_SIZE, MACRO_READ_TOTAL_TIMEOUT,
};
pub use mock::MockTransport;
pub use session::{
    ConfigSnapshot, Session, VersionInfo, CONFIG_TOTAL_TIMEOUT, DEFAULT_TIMEOUT,
    MAX_CONSECUTIVE_DECODE_FAILURES,
};
pub use transport::Transport;

#[cfg(feature = "ble")]
pub use ble::{scan_devices, scan_devices_with_lock_path, BleDevice, BleOptions, BleTransport};
#[cfg(feature = "ble")]
pub use ble_lock::{default_ble_lock_path, BleLock, BLE_LOCK_FILE_NAME, BLE_LOCK_PATH_ENV};

/// 8BitDo vendor ID.
pub const VENDOR_ID: u16 = 0x2DC8;

/// Advertised name of the App Mode BLE peripheral.
pub const APP_BLE_NAME: &str = "80ND";

/// App Mode GATT service (`0000FF10-...`).
pub const APP_SERVICE_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x0000_ff10_0000_1000_8000_0080_5f9b_34fb);

/// App Mode write/notify characteristic (`0000FF11-...`).
pub const APP_CHARACTERISTIC_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x0000_ff11_0000_1000_8000_0080_5f9b_34fb);

/// Alternate App Mode characteristic (`0000FF13-...`) seen on some units.
pub const APP_CHARACTERISTIC_UUID_ALT: uuid::Uuid =
    uuid::Uuid::from_u128(0x0000_ff13_0000_1000_8000_0080_5f9b_34fb);
