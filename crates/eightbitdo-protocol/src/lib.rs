//! Pure, I/O-free protocol implementation for the
//! **8BitDo Ultimate 2 Bluetooth Controller** (`PID 0x600F`).
//!
//! This crate knows nothing about BLE, USB HID or async runtimes. It only
//! turns Rust values into the exact byte sequences described in
//! `docs/PROTOCOL_SPEC.md` and `docs/STRUCT_BT2_2768B.md`, and back.
//!
//! ```no_run
//! use eightbitdo_protocol::commands;
//! use eightbitdo_protocol::config_bt2::{Bt2Config, Slot};
//!
//! # fn main() -> Result<(), eightbitdo_protocol::Error> {
//! // Read the whole 2768-byte configuration block (cmd 0x0002).
//! let request = commands::read_config_all();
//! assert_eq!(request.header.cmd, 0x0002);
//!
//! // Modify only the profile name, then recompute the struct CRC.
//! let mut config = Bt2Config::zeroed();
//! config.set_profile_name(Slot::S1, "Slot1")?;
//! config.recompute_crc();
//! # Ok(())
//! # }
//! ```
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod commands;
pub mod config_bt2;
pub mod crc;
pub mod error;
pub mod header;
pub mod input_report;
pub mod keymap;
pub mod macro_data;

pub use config_bt2::{Bt2Config, Slot, CONFIG_SIZE, SLOT_COUNT};
pub use crc::{ansi_crc16_sh, packet_crc16};
pub use error::Error;
pub use header::{CustomHeader, PacketKind, HEADER_SIZE};
pub use input_report::InputReport;
pub use keymap::{ButtonIndex, KeyMap};
pub use macro_data::{align32, MacroStep, MACRO_STEP_SIZE, MAX_MACRO_STEPS};

/// Version of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Total size of `record_ultimatebt2` in bytes (`0x0AD0`).
pub const RECORD_ULTIMATEBT2_SIZE: usize = CONFIG_SIZE;
