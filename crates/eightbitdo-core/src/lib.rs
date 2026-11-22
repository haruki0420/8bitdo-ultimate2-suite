//! Device I/O for the **8BitDo Ultimate 2 Bluetooth Controller** (`0x600F`).
//!
//! This crate sits between the pure protocol codec in `eightbitdo-protocol`
//! and the platform UI. It defines the [`Transport`] seam, a session that
//! performs the App Mode handshake and transfers, and a BLE implementation
//! backed by `btleplug`.
//!
//! The first slice was read-only (version, configuration and realtime input).
//! The current core also has macro flash read/write APIs (`macro_ops`), the
//! full 2768-byte configuration write path (`config_ops`), and a BLE-only OTA
//! state machine (`ota`). Every write path is fail-closed and currently
//! mock-tested only; no front end is wired to OTA yet, and USB OTA is
//! intentionally absent (USB Boot TX framing is undocumented).
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
pub mod ota;
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
pub use error::{
    ConfigWriteStage, ConfigWriteState, MacroWriteStage, MacroWriteState, OtaFailureStage,
    OtaWriteState,
};
pub use macro_ops::{
    macro_header_chunk_len, macro_write_chunk_len, macro_write_chunk_len_requested,
    max_protocol_payload, validate_macro_chunk_size, MacroBackup, MacroEntry, MacroHeaderOutcome,
    MacroSlotWriteOutcome, MacroTimeouts, MacroWriteOutcome, MacroWritePlan,
    MACRO_HEADER_CHUNK_SIZE, MACRO_READ_TOTAL_TIMEOUT,
};
pub use mock::MockTransport;
#[cfg(feature = "mobile-boot")]
#[doc(hidden)]
pub use ota::MobileBootLink;
pub use ota::{
    gamepad_type_from_pid, BootReady, FirmwareImage, GamepadType, JumpOutcome, OtaConfig,
    OtaOutcome, OtaPhase, OtaProgress, OtaTimeouts, OtaTransport, OtaUpdate, OTA_CHUNK_SIZE,
    OTA_ENTER_BOOT_TIMEOUT, OTA_ERASE_TIMEOUT, OTA_IMAGE_HEADER_LEN, OTA_JUMP_TIMEOUT,
    OTA_MAX_FRAME_LEN, OTA_MIN_MTU, OTA_PID_TIMEOUT, OTA_SECTOR_SIZE, OTA_VERSION_HEADER_LEN,
    OTA_VERSION_TIMEOUT, OTA_WRITE_TIMEOUT, OTA_WRITE_VERSION_TIMEOUT, UT2_BOOT_PID,
    UT2_TYPE1_APP_BASE_MIN, UT2_TYPE1_APP_FLASH_END,
};
#[cfg(feature = "ota-bench")]
pub use ota::{OtaBenchOutcome, OtaCheckpoint};
pub use session::{
    ConfigSnapshot, Session, VersionInfo, CONFIG_TOTAL_TIMEOUT, DEFAULT_TIMEOUT,
    MAX_CONSECUTIVE_DECODE_FAILURES,
};
pub use transport::Transport;

#[cfg(feature = "ble")]
pub use ble::{
    scan_devices, scan_devices_with_lock_path, BleBootOptions, BleBootTransport, BleDevice,
    BleOptions, BleTransport,
};
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

/// Advertised name of the BLE bootloader peripheral (`"8bt"`).
pub const BOOT_BLE_NAME: &str = "8bt";

/// Bootloader GATT service (`0000FF20-...`).
pub const BOOT_SERVICE_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x0000_ff20_0000_1000_8000_0080_5f9b_34fb);

/// Bootloader write/notify characteristic (`0000FF21-...`).
pub const BOOT_CHARACTERISTIC_UUID: uuid::Uuid =
    uuid::Uuid::from_u128(0x0000_ff21_0000_1000_8000_0080_5f9b_34fb);

/// Alternate bootloader characteristic (`0000FF23-...`) seen on `_CY` units.
pub const BOOT_CHARACTERISTIC_UUID_ALT: uuid::Uuid =
    uuid::Uuid::from_u128(0x0000_ff23_0000_1000_8000_0080_5f9b_34fb);
