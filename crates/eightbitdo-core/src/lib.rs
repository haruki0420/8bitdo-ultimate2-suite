//! Device I/O for the **8BitDo Ultimate 2 Bluetooth Controller** (`0x600F`).
//!
//! This crate sits between the pure protocol codec in `eightbitdo-protocol`
//! and the platform UI. It defines the [`Transport`] seam, a session that
//! performs the App Mode handshake and transfers, and a BLE implementation
//! backed by `btleplug`.
//!
//! The first slice was read-only (version, configuration and realtime input).
//! The current core also has macro flash read/write APIs (`macro_ops`); the
//! write path is fail-closed and currently mock-tested only. Configuration
//! apply and OTA are still separate phases.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod macro_ops;
pub mod mock;
pub mod session;
pub mod transport;

#[cfg(feature = "ble")]
pub mod ble;

pub use error::Error;
pub use error::MacroWriteStage;
pub use macro_ops::{
    macro_header_chunk_len, macro_write_chunk_len, max_protocol_payload, MacroBackup, MacroEntry,
    MacroHeaderOutcome, MacroTimeouts, MacroWriteOutcome, MacroWritePlan, MACRO_HEADER_CHUNK_SIZE,
    MACRO_READ_TOTAL_TIMEOUT,
};
pub use mock::MockTransport;
pub use session::{
    ConfigSnapshot, Session, VersionInfo, CONFIG_TOTAL_TIMEOUT, DEFAULT_TIMEOUT,
    MAX_CONSECUTIVE_DECODE_FAILURES,
};
pub use transport::Transport;

#[cfg(feature = "ble")]
pub use ble::{scan_devices, BleDevice, BleOptions, BleTransport};

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
