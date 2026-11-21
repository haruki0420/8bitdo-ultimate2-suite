//! Command builders for App Mode and Bootloader Mode.
//!
//! A [`Request`] pairs the 16-byte header with its payload. Encode it with
//! [`Request::encode_ble`] or [`Request::encode_usb_app`] depending on the
//! transport, then feed it to the transport layer.

use alloc::vec::Vec;

use crate::config_bt2::{MacroIndex, Slot, CONFIG_SIZE};
use crate::error::Error;
use crate::header::{self, CustomHeader, PacketKind, USB_REPORT_SIZE};
use crate::macro_data::{self, MacroStep};

/// App Mode command IDs.
pub mod cmd {
    /// Write configuration bytes.
    pub const WRITE_CONFIG: u16 = 0x0001;
    /// Read configuration bytes.
    pub const READ_CONFIG: u16 = 0x0002;
    /// Read / stream realtime input.
    pub const INPUT: u16 = 0x0003;
    /// Commit configuration to flash.
    pub const SAVE: u16 = 0x0006;
    /// Enable or disable realtime input notifications.
    pub const REPORT_ENABLE: u16 = 0x0007;
    /// Read firmware version.
    pub const READ_VERSION: u16 = 0x000B;

    /// Generic App Mode response envelope (`cmd 0x0004`).
    ///
    /// On the observed `0x600F` unit every command response arrives with this
    /// `cmd`; the request command is echoed in `subcmd` and the transfer size
    /// in `total_len`. A payload-less acknowledgement uses
    /// `chunk_len = total_len = 0`.
    pub const RESPONSE: u16 = 0x0004;
    /// Enter the BLE bootloader.
    pub const ENTER_BOOT: u16 = 0x000C;
    /// Disconnect the Bluetooth configuration link.
    pub const DISCONNECT_BT: u16 = 0x000F;
    /// Report the phone platform.
    pub const SET_PHONE_PLATFORM: u16 = 0x0010;
    /// Set the configuration state.
    pub const SET_CONFIG_STATE: u16 = 0x0011;
    /// Unsolicited "configuration allowed" notification.
    pub const ALLOW_CONFIG_DONE: u16 = 0x0012;
    /// Windows USB start-configuration handshake.
    pub const START_CONFIG: u16 = 0x0013;
    /// Switch the active profile slot.
    pub const WRITE_SLOT: u16 = 0x0015;
    /// Read macro steps.
    pub const READ_MACRO: u16 = 0x0102;
    /// Write macro steps.
    pub const WRITE_MACRO: u16 = 0x0103;
    /// Erase a macro flash sector.
    pub const ERASE_MACRO: u16 = 0x0104;
}

/// Sub-command values used by App Mode.
pub mod subcmd {
    /// `Save` sub-command for BLE / Android.
    pub const SAVE_BLE: u16 = 0x0067;
    /// `Save` sub-command for Windows USB HID.
    pub const SAVE_USB: u16 = 0x0123;
    /// Enter-bootloader sub-command.
    pub const ENTER_BOOT: u16 = 0x0067;
    /// Bootloader PID read sub-command.
    pub const BOOT_READ_PID: u16 = 0x0067;
    /// Bootloader version read sub-command.
    pub const BOOT_READ_VERSION: u16 = 0x0067;
    /// Disconnect sub-command.
    pub const DISCONNECT: u16 = 0x000F;
    /// Phone-platform sub-command.
    pub const PHONE_PLATFORM: u16 = 0x0001;
}

/// `total_len` of the `0x600F` (`gamepad_type == 1`) `ReadVersion` request.
///
/// The `0x01020030` value used by other controller generations is **not**
/// valid for the Ultimate 2 Bluetooth.
pub const VERSION_REQUEST_LEN: u32 = 4;

/// Dummy payload sent with the `0x600F` `ReadVersion` request.
///
/// The official `getVersionChannel` (0x2c7a0) points the channel payload at
/// the 4-byte `gamepadVersion` buffer and sets both `total_len` and
/// `chunk_len` to 4. `Generate_CustomDataCmd` (0x27620) copies those 4 bytes
/// into the request, so the wire request is 21 bytes
/// (`0x04` + 16-byte header + 4 bytes) rather than the 17-byte payload-less
/// frame. The buffer is zero-initialised on the first call; this
/// implementation always sends zeroes and does not cache a previous version.
pub const VERSION_REQUEST_PAYLOAD: [u8; VERSION_REQUEST_LEN as usize] = [0; 4];

/// `ReadVersion` response payload length on `0x600F` (`total_len = 4`).
///
/// The official `getVersion` (0x35700) reads the first four bytes as a
/// little-endian `i32`; no long version string is transferred.
pub const VERSION_RESPONSE_LEN: usize = VERSION_REQUEST_LEN as usize;

/// Maximum `WriteBT2Macro` (`cmd 0x0103`) payload per packet (`0x40`).
pub const MACRO_MAX_CHUNK_SIZE: usize = 0x40;

/// USB HID `WriteBT2Macro` payload per packet (`0x20`).
pub const MACRO_USB_CHUNK_SIZE: usize = 0x20;

/// Required 32-byte alignment of an encoded macro payload.
pub const MACRO_PAYLOAD_ALIGNMENT: usize = 0x20;

/// Maximum encoded macro payload: one 4 KB flash sector.
pub const MACRO_MAX_PAYLOAD: usize = macro_data::MACRO_SECTOR_SIZE as usize;

/// Which transport will issue a `Save` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveTransport {
    /// BLE (`subcmd 0x0067`).
    Ble,
    /// USB HID (`subcmd 0x0123`).
    Usb,
}

impl SaveTransport {
    /// The platform-specific `Save` sub-command.
    #[must_use]
    pub const fn subcmd(self) -> u16 {
        match self {
            Self::Ble => subcmd::SAVE_BLE,
            Self::Usb => subcmd::SAVE_USB,
        }
    }
}

/// One outgoing packet: header plus payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The 16-byte header, including `chunk_len` and `crc16`.
    pub header: CustomHeader,
    /// Payload bytes (`chunk_len` bytes).
    pub payload: Vec<u8>,
}

impl Request {
    /// Build an empty request.
    #[must_use]
    pub fn new(cmd: u16, subcmd: u16, total_len: u32, offset: u32) -> Self {
        Self {
            header: CustomHeader::new(cmd, subcmd, total_len, offset),
            payload: Vec::new(),
        }
    }

    /// Build a payload-less request that declares `chunk_len = total_len`.
    ///
    /// Read commands (`ReadConfig` / `ReadMacro`) on `0x600F` use this shape:
    /// the wire packet contains only the 16-byte header, but `chunk_len`
    /// carries the requested transfer length. See
    /// [`commands::read_config`] and [`commands::read_macro`].
    ///
    /// # Panics
    ///
    /// Panics when `total_len` does not fit in `u16`. All built-in read
    /// builders are bounded by the 2768-byte config block or one 4 KB macro
    /// sector, so this cannot happen through the public constructors.
    #[must_use]
    pub fn new_payloadless(cmd: u16, subcmd: u16, total_len: u32, offset: u32) -> Self {
        let chunk_len = u16::try_from(total_len)
            .expect("payload-less read length must fit in the wire chunk_len field");
        Self {
            header: CustomHeader::new(cmd, subcmd, total_len, offset).with_chunk_len(chunk_len),
            payload: Vec::new(),
        }
    }

    /// Attach a payload, updating `chunk_len` but not `crc16`.
    #[must_use]
    pub fn with_payload(mut self, payload: &[u8]) -> Self {
        assert!(payload.len() <= u16::MAX as usize, "payload exceeds u16");
        self.header.chunk_len = payload.len() as u16;
        self.payload = payload.to_vec();
        self
    }

    /// Attach a payload and compute the packet CRC-16/MODBUS over it.
    #[must_use]
    pub fn with_crc_payload(mut self, payload: &[u8]) -> Self {
        assert!(payload.len() <= u16::MAX as usize, "payload exceeds u16");
        self.header.chunk_len = payload.len() as u16;
        self.header.crc16 = crate::crc::packet_crc16(payload);
        self.payload = payload.to_vec();
        self
    }

    /// Encode for a BLE characteristic write.
    pub fn encode_ble(&self, kind: PacketKind) -> Result<Vec<u8>, Error> {
        header::encode_ble(kind, &self.header, &self.payload)
    }

    /// Encode for an App Mode USB HID write (65 bytes).
    pub fn encode_usb_app(&self) -> Result<[u8; USB_REPORT_SIZE], Error> {
        header::encode_usb_app(&self.header, &self.payload)
    }

    /// Validate the shape accepted by the payload-less encoders.
    ///
    /// Two families are allowed:
    ///
    /// - `ReadConfig` (`0x0002`) and `ReadMacro` (`0x0102`) with an empty
    ///   payload and `chunk_len == total_len`;
    /// - `EraseBT2Macro` (`0x0104`) with `chunk_len = total_len = 0x1000`,
    ///   `crc16 = 0xFFFF` and a 4 KB-aligned sector offset.
    fn validate_payloadless(&self) -> Result<(), Error> {
        if !self.payload.is_empty() {
            return Err(Error::UnexpectedPayload {
                actual: self.payload.len(),
            });
        }
        match self.header.cmd {
            cmd::READ_CONFIG | cmd::READ_MACRO => {
                if u32::from(self.header.chunk_len) != self.header.total_len {
                    return Err(Error::InvalidFrame(
                        "payload-less read chunk_len must equal total_len",
                    ));
                }
                Ok(())
            }
            cmd::ERASE_MACRO => {
                let expected_len = macro_data::MACRO_SECTOR_SIZE;
                if self.header.total_len != expected_len
                    || self.header.chunk_len != expected_len as u16
                {
                    return Err(Error::InvalidFrame(
                        "payloadless encoding is restricted to EraseBT2Macro \
                         (cmd 0x0104, total_len = chunk_len = 0x1000)",
                    ));
                }
                if self.header.crc16 != 0xFFFF {
                    return Err(Error::InvalidFrame(
                        "EraseBT2Macro header crc16 must be 0xFFFF",
                    ));
                }
                if !matches!(self.header.offset, 0 | 0x1000 | 0x2000 | 0x3000) {
                    return Err(Error::InvalidFrame(
                        "EraseBT2Macro offset must be macro_idx << 12",
                    ));
                }
                Ok(())
            }
            _ => Err(Error::InvalidFrame(
                "payloadless encoding is restricted to ReadConfig, ReadMacro \
                 and EraseBT2Macro",
            )),
        }
    }

    /// Encode a payload-less command whose header declares a non-zero
    /// `chunk_len`.
    ///
    /// `ReadConfig` (`0x0002`) and `ReadMacro` (`0x0102`) use this path with
    /// `chunk_len == total_len` and no payload bytes. `EraseBT2Macro`
    /// (`0x0104`) uses it with `chunk_len = total_len = 0x1000`. All other
    /// commands must use [`encode_ble`](Self::encode_ble), which enforces
    /// `chunk_len == payload.len()`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedPayload`] when a payload is attached, or
    /// [`Error::InvalidFrame`] for a non-App `kind` or any other command shape.
    pub fn encode_ble_payloadless(&self, kind: PacketKind) -> Result<Vec<u8>, Error> {
        if kind != PacketKind::App {
            return Err(Error::InvalidFrame(
                "payloadless BLE encoding is App Mode only",
            ));
        }
        self.validate_payloadless()?;
        Ok(header::encode_ble_payloadless(kind, &self.header))
    }

    /// Encode a payload-less request as a 65-byte App Mode USB HID report.
    ///
    /// See [`encode_ble_payloadless`](Self::encode_ble_payloadless) for the
    /// accepted command shapes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedPayload`] when a payload is attached, or
    /// [`Error::InvalidFrame`] for any other command shape.
    pub fn encode_usb_app_payloadless(&self) -> Result<[u8; USB_REPORT_SIZE], Error> {
        self.validate_payloadless()?;
        Ok(header::encode_usb_app_payloadless(&self.header))
    }
}

/// Validate a configuration range against the fixed 2768-byte buffer.
fn check_config_range(offset: u32, len: u64) -> Result<(), Error> {
    let limit = CONFIG_SIZE as u64;
    let end = u64::from(offset).saturating_add(len);
    if end > limit {
        return Err(Error::RangeOutOfBounds {
            offset: u64::from(offset),
            len,
            limit,
        });
    }
    Ok(())
}

/// `ReadConfig` for an arbitrary range.
///
/// The request is payload-less but declares `chunk_len == total_len == len`.
/// The official `getUT2_ReadCofigChannel` (0x29ea0) loads the 16-byte channel
/// fields from `0x1d5f0`: `offset = 0`, `total_len = chunk_len = 0x0AD0` for
/// the full block. `getBT2_head_CofigChannel` (0x2a000) uses `0x74` for the
/// header read.
///
/// # Errors
///
/// Returns [`Error::RangeOutOfBounds`] when `offset + len` exceeds the
/// 2768-byte configuration block.
pub fn read_config(offset: u32, len: u32) -> Result<Request, Error> {
    check_config_range(offset, u64::from(len))?;
    Ok(Request::new_payloadless(cmd::READ_CONFIG, 0, len, offset))
}

/// Read the full 2768-byte configuration block.
#[must_use]
pub fn read_config_all() -> Request {
    read_config(0, CONFIG_SIZE as u32).expect("the full configuration range is valid")
}

/// Read only the 116-byte header (`0x000..0x073`).
#[must_use]
pub fn read_config_header() -> Request {
    read_config(0, 0x0074).expect("the configuration header range is valid")
}

/// Read the current slot value (`0x012`, 2 bytes).
#[must_use]
pub fn read_current_slot() -> Request {
    read_config(0x0012, 2).expect("the current-slot range is valid")
}

/// `WriteConfig` for an arbitrary range.
///
/// `total_len` is always the full 2768 bytes, matching the official partial
/// write path. The payload may be shorter than the total.
///
/// The `0x600F` (`gamepad_type == 1`) path computes the packet CRC-16/MODBUS
/// over every chunk and stores it in the header `crc16` field.
///
/// # Errors
///
/// Returns [`Error::EmptyPayload`] when `data` is empty, or
/// [`Error::RangeOutOfBounds`] when `offset + data.len()` exceeds the
/// 2768-byte configuration block.
pub fn write_config(offset: u32, data: &[u8]) -> Result<Request, Error> {
    if data.is_empty() {
        return Err(Error::EmptyPayload);
    }
    check_config_range(offset, data.len() as u64)?;
    Ok(Request::new(cmd::WRITE_CONFIG, 0, CONFIG_SIZE as u32, offset).with_crc_payload(data))
}

/// Split a configuration write into transport-sized packets.
///
/// Offsets advance by `chunk_size`; `total_len` stays `0x0AD0`. Every packet
/// carries the CRC-16/MODBUS of its own payload, as required for `0x600F`
/// (`gamepad_type == 1`).
///
/// # Errors
///
/// Returns [`Error::EmptyPayload`] when `data` is empty,
/// [`Error::InvalidChunkSize`] when `chunk_size` is zero, or
/// [`Error::RangeOutOfBounds`] when the whole buffer exceeds the 2768-byte
/// configuration block.
pub fn write_config_packets(
    offset: u32,
    data: &[u8],
    chunk_size: usize,
) -> Result<Vec<Request>, Error> {
    if data.is_empty() {
        return Err(Error::EmptyPayload);
    }
    if chunk_size == 0 {
        return Err(Error::InvalidChunkSize {
            max: CONFIG_SIZE,
            actual: 0,
        });
    }
    check_config_range(offset, data.len() as u64)?;
    Ok(data
        .chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| {
            Request::new(
                cmd::WRITE_CONFIG,
                0,
                CONFIG_SIZE as u32,
                offset + (i * chunk_size) as u32,
            )
            .with_crc_payload(chunk)
        })
        .collect())
}

/// Commit pending configuration writes to flash.
#[must_use]
pub fn save(transport: SaveTransport) -> Request {
    Request::new(cmd::SAVE, transport.subcmd(), 0, 0)
}

/// Read the firmware version.
///
/// For the `0x600F` (`gamepad_type == 1`) the official request uses
/// `total_len = chunk_len = 4` and `offset = 0x01017040`. The
/// `0x01020030` query value is the `gamepad_type == 2` variant and must not
/// be used here.
///
/// The request carries a 4-byte zero payload. The official channel points at
/// the zero-initialised `gamepadVersion` buffer and `Generate_CustomDataCmd`
/// copies it into the packet (payload copy and CRC path around
/// `0x27a37..0x27b72` in `libadvancelib.so`). The packet CRC is computed over
/// those four bytes, so the wire header has `crc16 = 0x2400`
/// (CRC-16/MODBUS of `00 00 00 00`), not `0xFFFF`.
#[must_use]
pub fn read_version() -> Request {
    Request::new(
        cmd::READ_VERSION,
        0,
        VERSION_REQUEST_LEN,
        boot::VERSION_ADDR,
    )
    .with_crc_payload(&VERSION_REQUEST_PAYLOAD)
}

/// Report the host as a phone platform.
#[must_use]
pub fn set_phone_platform() -> Request {
    Request::new(cmd::SET_PHONE_PLATFORM, subcmd::PHONE_PLATFORM, 0, 0)
}

/// Enable or disable realtime input notifications.
#[must_use]
pub fn set_report_enable(enabled: bool) -> Request {
    Request::new(cmd::REPORT_ENABLE, if enabled { 1 } else { 0 }, 0, 0)
}

/// Request one realtime input report (`12` bytes).
#[must_use]
pub fn read_input() -> Request {
    Request::new(cmd::INPUT, 0, 0x0C, 0x0C)
}

/// Notify the controller of a configuration state change.
#[must_use]
pub fn set_config_state(state: u16) -> Request {
    Request::new(cmd::SET_CONFIG_STATE, state, 0, 0)
}

/// Windows USB `StartConfig` handshake.
#[must_use]
pub fn start_config() -> Request {
    Request::new(cmd::START_CONFIG, 0, 0, 0)
}

/// Switch the active profile slot.
#[must_use]
pub fn switch_slot(slot: Slot) -> Request {
    Request::new(cmd::WRITE_SLOT, slot.index() as u16, 0, 0)
}

/// Enter the BLE bootloader (`"8bt"`).
#[must_use]
pub fn enter_boot() -> Request {
    Request::new(cmd::ENTER_BOOT, subcmd::ENTER_BOOT, 4, 4)
}

/// Explicitly disconnect the BLE configuration link.
#[must_use]
pub fn disconnect_bt() -> Request {
    Request::new(cmd::DISCONNECT_BT, subcmd::DISCONNECT, 0, 4)
}

/// `subcmd` encoding shared by all macro commands: `(mode << 8) | slot`.
#[must_use]
pub const fn macro_subcmd(gamepad_mode: u16, slot: Slot) -> u16 {
    (gamepad_mode << 8) | slot.index() as u16
}

/// Read macro steps (`1 step = 10 bytes`, no 32-byte alignment).
///
/// The request is payload-less but declares
/// `chunk_len == total_len == steps * 10`. The official
/// `getBT2_ReadMacro` (0x2bd90) sets `+0x40 = +0x44 = +0x48` to that length,
/// keeps the 4 KB sector base in `+0x38`, and uses `cmd 0x0102`.
///
/// # Errors
///
/// Returns [`Error::TooManySteps`] when `steps` exceeds
/// [`macro_data::MAX_MACRO_STEPS`], or [`Error::RangeOutOfBounds`] when the
/// encoded steps would exceed one 4 KB flash sector.
pub fn read_macro(
    gamepad_mode: u16,
    slot: Slot,
    macro_idx: MacroIndex,
    steps: u16,
) -> Result<Request, Error> {
    let step_count = usize::from(steps);
    let total_len = u32::from(steps) * macro_data::MACRO_STEP_SIZE as u32;
    if total_len > macro_data::MACRO_SECTOR_SIZE {
        return Err(Error::RangeOutOfBounds {
            offset: u64::from(macro_idx.sector_offset()),
            len: u64::from(total_len),
            limit: u64::from(macro_data::MACRO_SECTOR_SIZE),
        });
    }
    if step_count > macro_data::MAX_MACRO_STEPS {
        return Err(Error::TooManySteps {
            max: macro_data::MAX_MACRO_STEPS,
            actual: step_count,
        });
    }
    Ok(Request::new_payloadless(
        cmd::READ_MACRO,
        macro_subcmd(gamepad_mode, slot),
        total_len,
        macro_idx.sector_offset(),
    ))
}

/// Write macro steps, splitting into packets of at most `chunk_size` bytes.
///
/// The payload is [`macro_data::encode_macro_steps`] (32-byte aligned) and
/// every packet's `offset` advances. BLE allows 64-byte chunks; USB HID
/// allows 32.
pub fn write_macro_steps(
    gamepad_mode: u16,
    slot: Slot,
    macro_idx: MacroIndex,
    steps: &[MacroStep],
    chunk_size: usize,
) -> Result<Vec<Request>, Error> {
    let payload = macro_data::encode_macro_steps(steps)?;
    write_macro_payload(gamepad_mode, slot, macro_idx, &payload, chunk_size)
}

/// Write a pre-encoded macro payload (already 32-byte aligned).
///
/// `payload` must not be empty, and `payload.len()` must be a multiple of
/// [`MACRO_PAYLOAD_ALIGNMENT`] (`0x20`). `chunk_size` must be
/// [`MACRO_USB_CHUNK_SIZE`] (`0x20`) or [`MACRO_MAX_CHUNK_SIZE`] (`0x40`);
/// larger values are first clamped to `0x40`, matching the official BLE limit.
/// Offsets advance by the **clamped** chunk size, and every packet carries the
/// CRC-16/MODBUS of its own payload.
///
/// # Errors
///
/// Returns [`Error::EmptyPayload`] when `payload` is empty,
/// [`Error::InvalidChunkSize`] when `chunk_size` is zero,
/// [`Error::PayloadTooLarge`] when `payload` exceeds one 4 KB flash sector,
/// [`Error::UnsupportedChunkSize`] when the clamped size is not 32 or 64, or
/// [`Error::UnalignedPayloadLength`] when the payload is not 32-byte aligned.
pub fn write_macro_payload(
    gamepad_mode: u16,
    slot: Slot,
    macro_idx: MacroIndex,
    payload: &[u8],
    chunk_size: usize,
) -> Result<Vec<Request>, Error> {
    if payload.is_empty() {
        return Err(Error::EmptyPayload);
    }
    if chunk_size == 0 {
        return Err(Error::InvalidChunkSize {
            max: MACRO_MAX_CHUNK_SIZE,
            actual: 0,
        });
    }
    if payload.len() > MACRO_MAX_PAYLOAD {
        return Err(Error::PayloadTooLarge {
            max: MACRO_MAX_PAYLOAD,
            actual: payload.len(),
        });
    }
    let chunk_size = chunk_size.min(MACRO_MAX_CHUNK_SIZE);
    if chunk_size != MACRO_USB_CHUNK_SIZE && chunk_size != MACRO_MAX_CHUNK_SIZE {
        return Err(Error::UnsupportedChunkSize { actual: chunk_size });
    }
    if payload.len() % MACRO_PAYLOAD_ALIGNMENT != 0 {
        return Err(Error::UnalignedPayloadLength {
            actual: payload.len(),
            alignment: MACRO_PAYLOAD_ALIGNMENT,
        });
    }
    let total_len = payload.len() as u32;
    let base = macro_idx.sector_offset();
    Ok(payload
        .chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| {
            Request::new(
                cmd::WRITE_MACRO,
                macro_subcmd(gamepad_mode, slot),
                total_len,
                base + (i * chunk_size) as u32,
            )
            .with_crc_payload(chunk)
        })
        .collect())
}

/// Erase one macro flash sector (`0x1000` bytes).
///
/// Official evidence (x86_64 `libadvancelib.so`):
/// `getBT2_EraseMacro` (0x2bb70) sets `total_len = chunk_len = 0x1000` and
/// sends no payload. `Generate_CustomDataCmd` (0x27620) still runs the
/// `gamepad_type == 1` CRC path over that empty payload, and `crc16` (0x28540)
/// returns `0xFFFF` for length <= 0. The header therefore carries
/// `crc16 = 0xFFFF`, not `0`.
///
/// Encode this request with
/// [`Request::encode_ble_payloadless`](Request::encode_ble_payloadless) or
/// [`Request::encode_usb_app_payloadless`](Request::encode_usb_app_payloadless);
/// the normal encoders correctly reject the declared-length mismatch.
#[must_use]
pub fn erase_macro(gamepad_mode: u16, slot: Slot, macro_idx: MacroIndex) -> Request {
    let mut request = Request::new(
        cmd::ERASE_MACRO,
        macro_subcmd(gamepad_mode, slot),
        macro_data::MACRO_SECTOR_SIZE,
        macro_idx.sector_offset(),
    );
    request.header.chunk_len = macro_data::MACRO_SECTOR_SIZE as u16;
    request.header.crc16 = crate::crc::packet_crc16(&request.payload);
    request
}

/// Bootloader Mode command builders (`prefix 0x05`, advertise name `"8bt"`).
pub mod boot {
    use alloc::vec::Vec;

    use super::{PacketKind, Request};

    /// Write the application firmware version (4 bytes + CRC).
    pub const CMD_WRITE_FIRMWARE_VERSION: u16 = 0x0002;
    /// Write a 64-byte firmware chunk.
    pub const CMD_WRITE_FIRMWARE: u16 = 0x0003;
    /// Erase a 4 KB flash sector.
    pub const CMD_ERASE_FLASH: u16 = 0x0004;
    /// Read the bootloader version.
    pub const CMD_READ_VERSION: u16 = 0x0005;
    /// Jump to the application.
    pub const CMD_JUMP_APP: u16 = 0x0006;
    /// Read the internal PID.
    pub const CMD_READ_PID: u16 = 0x0008;
    /// Write the 24-byte image header.
    pub const CMD_WRITE_VERSION: u16 = 0x00C4;

    /// Address used by `WriteFirmwareVersion` and `readVersionInBoot` for
    /// `gamepad_type == 1` (Ultimate 2 Bluetooth).
    pub const VERSION_ADDR: u32 = 0x0101_7040;
    /// Address used by `WriteAPPID` (skipped for `gamepad_type == 1`).
    pub const APPID_ADDR: u32 = 0x0801_401C;
    /// Application entry point used by `JumpApp`.
    pub const APP_ENTRY_ADDR: u32 = 0x0101_8000;
    /// Flash erase sector size.
    pub const SECTOR_SIZE: u32 = 0x1000;
    /// Firmware write chunk size.
    pub const CHUNK_SIZE: usize = 0x40;

    /// Read the device PID (`cmd 0x0008`, `subcmd 0x0067`).
    #[must_use]
    pub fn read_pid() -> Request {
        Request::new(CMD_READ_PID, 0x0067, 2, 0)
    }

    /// Read the bootloader version.
    #[must_use]
    pub fn read_version() -> Request {
        Request::new(CMD_READ_VERSION, 0x0067, 4, VERSION_ADDR)
    }

    /// Erase the 4 KB sector containing `addr`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::UnalignedAddress`] when `addr` is not a
    /// multiple of [`SECTOR_SIZE`] (`0x1000`).
    pub fn erase_flash(addr: u32) -> Result<Request, crate::error::Error> {
        if addr % SECTOR_SIZE != 0 {
            return Err(crate::error::Error::UnalignedAddress {
                address: addr,
                alignment: SECTOR_SIZE,
            });
        }
        Ok(Request::new(CMD_ERASE_FLASH, 0, SECTOR_SIZE, addr))
    }

    /// Write one full 64-byte firmware chunk with CRC-16/MODBUS.
    ///
    /// The primitive enforces the same guards as
    /// [`write_firmware_packets`], so callers cannot bypass them by sending
    /// individual chunks.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::InvalidLength`] unless `chunk` is
    /// exactly [`CHUNK_SIZE`] (`0x40`) bytes,
    /// [`crate::error::Error::EmptyFirmwareImage`] for `image_len == 0`,
    /// [`crate::error::Error::UnalignedImageLength`] when `image_len` is not a
    /// multiple of [`CHUNK_SIZE`], [`crate::error::Error::AddressOverflow`]
    /// when `addr + 64` exceeds `u32::MAX + 1`, or
    /// [`crate::error::Error::UnalignedAddress`] when `addr` is not 64-byte
    /// aligned.
    pub fn write_firmware(
        addr: u32,
        image_len: u32,
        chunk: &[u8],
    ) -> Result<Request, crate::error::Error> {
        if chunk.len() != CHUNK_SIZE {
            return Err(crate::error::Error::InvalidLength {
                expected: CHUNK_SIZE,
                actual: chunk.len(),
            });
        }
        if image_len == 0 {
            return Err(crate::error::Error::EmptyFirmwareImage);
        }
        if image_len % CHUNK_SIZE as u32 != 0 {
            return Err(crate::error::Error::UnalignedImageLength {
                actual: image_len as usize,
                alignment: CHUNK_SIZE,
            });
        }
        if u64::from(addr) + CHUNK_SIZE as u64 > u64::from(u32::MAX) + 1 {
            return Err(crate::error::Error::AddressOverflow {
                base: addr,
                offset: CHUNK_SIZE as u64,
            });
        }
        if addr % CHUNK_SIZE as u32 != 0 {
            return Err(crate::error::Error::UnalignedAddress {
                address: addr,
                alignment: CHUNK_SIZE as u32,
            });
        }
        Ok(Request::new(CMD_WRITE_FIRMWARE, 0, image_len, addr).with_crc_payload(chunk))
    }

    /// Split a firmware image into 64-byte write packets.
    ///
    /// The base address and every chunk address must be 64-byte aligned. A
    /// short final chunk is rejected rather than padded: the official tool
    /// only writes full 64-byte chunks, and the padding policy for a real
    /// image whose length is not a multiple of 64 is still pending hardware
    /// confirmation. No padding bytes are invented here.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::EmptyFirmwareImage`] for an empty image
    /// or zero `image_len`, [`crate::error::Error::UnalignedImageLength`] when
    /// `image.len()` is not a multiple of [`CHUNK_SIZE`],
    /// [`crate::error::Error::ImageLengthMismatch`] when it disagrees with
    /// `image_len`, [`crate::error::Error::UnalignedAddress`] for an unaligned
    /// base address, or [`crate::error::Error::AddressOverflow`] when a chunk
    /// address would exceed `u32`.
    pub fn write_firmware_packets(
        addr: u32,
        image_len: u32,
        image: &[u8],
    ) -> Result<Vec<Request>, crate::error::Error> {
        if image.is_empty() || image_len == 0 {
            return Err(crate::error::Error::EmptyFirmwareImage);
        }
        if image.len() % CHUNK_SIZE != 0 {
            return Err(crate::error::Error::UnalignedImageLength {
                actual: image.len(),
                alignment: CHUNK_SIZE,
            });
        }
        let actual_len =
            u32::try_from(image.len()).map_err(|_| crate::error::Error::PayloadTooLarge {
                max: u32::MAX as usize,
                actual: image.len(),
            })?;
        if actual_len != image_len {
            return Err(crate::error::Error::ImageLengthMismatch {
                declared: image_len,
                actual: actual_len,
            });
        }
        if addr % CHUNK_SIZE as u32 != 0 {
            return Err(crate::error::Error::UnalignedAddress {
                address: addr,
                alignment: CHUNK_SIZE as u32,
            });
        }
        image
            .chunks(CHUNK_SIZE)
            .enumerate()
            .map(|(i, chunk)| {
                let offset = i as u64 * CHUNK_SIZE as u64;
                let offset_u32 = u32::try_from(offset)
                    .map_err(|_| crate::error::Error::AddressOverflow { base: addr, offset })?;
                let chunk_addr = addr
                    .checked_add(offset_u32)
                    .ok_or(crate::error::Error::AddressOverflow { base: addr, offset })?;
                write_firmware(chunk_addr, image_len, chunk)
            })
            .collect()
    }

    /// Write the 24-byte image header (`cmd 0x00C4`).
    ///
    /// `image_len` must equal the little-endian `u32` at `header24[8..12]`
    /// (`image_data_len`, which the official tool uses as `total_len`).
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::InvalidLength`] unless `header24` is
    /// exactly 24 bytes, [`crate::error::Error::EmptyFirmwareImage`] for
    /// `image_len == 0`, [`crate::error::Error::UnalignedImageLength`] when
    /// `image_len` is not a multiple of [`CHUNK_SIZE`], or
    /// [`crate::error::Error::ImageLengthMismatch`] when `image_len` disagrees
    /// with the header field.
    pub fn write_version_header(
        image_len: u32,
        header24: &[u8],
    ) -> Result<Request, crate::error::Error> {
        if header24.len() != 0x18 {
            return Err(crate::error::Error::InvalidLength {
                expected: 0x18,
                actual: header24.len(),
            });
        }
        if image_len == 0 {
            return Err(crate::error::Error::EmptyFirmwareImage);
        }
        if image_len % CHUNK_SIZE as u32 != 0 {
            return Err(crate::error::Error::UnalignedImageLength {
                actual: image_len as usize,
                alignment: CHUNK_SIZE,
            });
        }
        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&header24[8..12]);
        let header_image_len = u32::from_le_bytes(length_bytes);
        if header_image_len != image_len {
            return Err(crate::error::Error::ImageLengthMismatch {
                declared: image_len,
                actual: header_image_len,
            });
        }
        let mut request = Request::new(CMD_WRITE_VERSION, 0, image_len, 0);
        request.header.chunk_len = header24.len() as u16;
        request.payload = header24.to_vec();
        Ok(request)
    }

    /// Write the 4-byte firmware version value.
    #[must_use]
    pub fn write_firmware_version(version: u32) -> Request {
        Request::new(CMD_WRITE_FIRMWARE_VERSION, 0, 4, VERSION_ADDR)
            .with_crc_payload(&version.to_le_bytes())
    }

    /// Write the 12-byte encrypted APPID (not used for `gamepad_type == 1`).
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::InvalidLength`] unless `serial` is
    /// exactly 12 bytes.
    pub fn write_appid(serial: &[u8]) -> Result<Request, crate::error::Error> {
        if serial.len() != 0x0C {
            return Err(crate::error::Error::InvalidLength {
                expected: 0x0C,
                actual: serial.len(),
            });
        }
        Ok(Request::new(CMD_WRITE_FIRMWARE_VERSION, 0, 0x0C, APPID_ADDR).with_crc_payload(serial))
    }

    /// Jump to the application and leave bootloader mode.
    #[must_use]
    pub fn jump_app() -> Request {
        Request::new(CMD_JUMP_APP, 0, 0, APP_ENTRY_ADDR)
    }

    /// Convenience: encode a bootloader request with the `0x05` prefix.
    pub fn encode(request: &Request) -> Result<alloc::vec::Vec<u8>, crate::error::Error> {
        request.encode_ble(PacketKind::Boot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_bt2::GamepadMode;

    #[test]
    fn read_config_is_cmd_2() {
        let request = read_config_all();
        assert_eq!(request.header.cmd, 0x0002);
        assert_eq!(request.header.subcmd, 0);
        assert_eq!(request.header.total_len, 0x0AD0);
        assert_eq!(request.header.chunk_len, 0x0AD0);
        assert_eq!(request.header.offset, 0);
        assert!(request.payload.is_empty());

        let header = read_config_header();
        assert_eq!(header.header.total_len, 0x74);
        assert_eq!(header.header.chunk_len, 0x74);
        let slot = read_current_slot();
        assert_eq!(slot.header.offset, 0x12);
        assert_eq!(slot.header.chunk_len, 2);
        assert_eq!(slot.header.total_len, 2);
    }

    #[test]
    fn read_config_payloadless_encoding_is_17_bytes_with_declared_chunk_len() {
        let bytes = read_config_all()
            .encode_ble_payloadless(PacketKind::App)
            .unwrap();
        assert_eq!(
            bytes,
            [
                0x04, // BLE App prefix
                0x02, 0x00, // cmd = 0x0002
                0x00, 0x00, // subcmd
                0xD0, 0x0A, // chunk_len = 0x0AD0
                0x00, 0x00, // crc16
                0xD0, 0x0A, 0x00, 0x00, // total_len = 0x0AD0
                0x00, 0x00, 0x00, 0x00, // offset = 0
            ]
        );
        assert_eq!(bytes.len(), 17);
    }

    #[test]
    fn payloadless_read_requires_chunk_len_equal_total_len() {
        let mut config = read_config_all();
        config.header.chunk_len = 0;
        assert!(matches!(
            config.encode_ble_payloadless(PacketKind::App),
            Err(Error::InvalidFrame(_))
        ));

        let mut macro_request = read_macro(3, Slot::S2, MacroIndex::M3, 42).unwrap();
        macro_request.header.chunk_len = 0;
        assert!(matches!(
            macro_request.encode_ble_payloadless(PacketKind::App),
            Err(Error::InvalidFrame(_))
        ));

        // The normal encoder still rejects a declared length without payload.
        assert_eq!(
            read_config_all().encode_ble(PacketKind::App),
            Err(Error::ChunkLenMismatch {
                declared: 0x0AD0,
                actual: 0,
            })
        );
    }

    #[test]
    fn write_config_is_cmd_1_with_full_total_len_and_crc() {
        let request = write_config(0x74, &[1, 2, 3, 4]).unwrap();
        assert_eq!(request.header.cmd, 0x0001);
        assert_eq!(request.header.subcmd, 0);
        assert_eq!(request.header.total_len, 0x0AD0);
        assert_eq!(request.header.offset, 0x74);
        assert_eq!(request.header.chunk_len, 4);
        assert_eq!(
            request.header.crc16,
            crate::crc::packet_crc16(&[1, 2, 3, 4])
        );
        assert_eq!(request.payload, [1, 2, 3, 4]);
    }

    #[test]
    fn config_packets_advance_offsets_and_carry_crc() {
        let data = [0xAAu8; 100];
        let packets = write_config_packets(0x100, &data, 32).unwrap();
        assert_eq!(packets.len(), 4);
        assert_eq!(packets[0].header.offset, 0x100);
        assert_eq!(packets[1].header.offset, 0x120);
        assert_eq!(packets[2].header.offset, 0x140);
        assert_eq!(packets[3].header.offset, 0x160);
        assert_eq!(packets[3].payload.len(), 4);
        for packet in &packets {
            assert_eq!(packet.header.total_len, 0x0AD0);
            assert_eq!(packet.header.chunk_len as usize, packet.payload.len());
            assert_eq!(
                packet.header.crc16,
                crate::crc::packet_crc16(&packet.payload)
            );
            assert_ne!(packet.header.crc16, 0);
        }
    }

    #[test]
    fn config_ranges_are_bounded() {
        assert!(matches!(
            read_config(0x0AD0, 1),
            Err(Error::RangeOutOfBounds {
                offset: 0x0AD0,
                len: 1,
                limit: 0x0AD0,
            })
        ));
        assert!(matches!(
            read_config(0x0AC0, 0x11),
            Err(Error::RangeOutOfBounds { .. })
        ));
        assert!(matches!(
            write_config(0x0ACF, &[0, 0]),
            Err(Error::RangeOutOfBounds { .. })
        ));
        assert!(matches!(
            write_config_packets(0x0AD0, &[1], 1),
            Err(Error::RangeOutOfBounds { .. })
        ));
        assert!(matches!(
            write_config_packets(0, &[1], 0),
            Err(Error::InvalidChunkSize { actual: 0, .. })
        ));
        assert!(matches!(write_config(0, &[]), Err(Error::EmptyPayload)));
        assert!(matches!(
            write_config_packets(0, &[], 32),
            Err(Error::EmptyPayload)
        ));
    }

    #[test]
    fn save_subcommands_differ_by_transport() {
        assert_eq!(save(SaveTransport::Ble).header.subcmd, 0x0067);
        assert_eq!(save(SaveTransport::Usb).header.subcmd, 0x0123);
        assert_eq!(save(SaveTransport::Ble).header.cmd, 0x0006);
    }

    #[test]
    fn read_version_uses_type_1_parameters() {
        let request = read_version();
        assert_eq!(request.header.cmd, 0x000B);
        assert_eq!(request.header.subcmd, 0);
        assert_eq!(request.header.chunk_len, 4);
        assert_eq!(request.header.crc16, 0x2400);
        assert_eq!(request.header.total_len, 4);
        assert_eq!(request.header.offset, boot::VERSION_ADDR);
        assert_eq!(request.header.offset, 0x0101_7040);
        assert_eq!(request.payload, [0, 0, 0, 0]);
    }

    #[test]
    fn read_version_golden_request_matches_hardware_shape() {
        // Evidence: getVersionChannel (0x2c7a0) sets chunk_len = total_len = 4
        // and points the payload at gamepadVersion; Generate_CustomDataCmd
        // copies 4 zero bytes and computes CRC-16/MODBUS over them.
        let bytes = read_version().encode_ble(PacketKind::App).unwrap();
        assert_eq!(
            bytes,
            [
                0x04, // BLE App prefix
                0x0B, 0x00, // cmd = 0x000B
                0x00, 0x00, // subcmd
                0x04, 0x00, // chunk_len = 4
                0x00, 0x24, // crc16 = 0x2400
                0x04, 0x00, 0x00, 0x00, // total_len = 4
                0x40, 0x70, 0x01, 0x01, // offset = 0x01017040
                0x00, 0x00, 0x00, 0x00, // payload
            ]
        );
    }

    #[test]
    fn version_response_envelope_constant_is_not_a_request_command() {
        assert_eq!(cmd::RESPONSE, 0x0004);
        assert_ne!(cmd::RESPONSE, cmd::READ_VERSION);
        assert_ne!(cmd::RESPONSE, cmd::READ_CONFIG);
    }

    #[test]
    fn report_enable_and_input() {
        assert_eq!(set_report_enable(true).header.subcmd, 1);
        assert_eq!(set_report_enable(false).header.subcmd, 0);
        let input = read_input();
        assert_eq!(input.header.cmd, 0x0003);
        assert_eq!(input.header.total_len, 0x0C);
        assert_eq!(input.header.offset, 0x0C);
    }

    #[test]
    fn macro_subcmd_packs_mode_and_slot() {
        assert_eq!(macro_subcmd(GamepadMode::Switch as u16, Slot::S1), 0x0000);
        assert_eq!(macro_subcmd(GamepadMode::XInput as u16, Slot::S2), 0x0301);
        assert_eq!(macro_subcmd(0x00AB, Slot::S3), 0xAB02);
    }

    #[test]
    fn read_macro_uses_raw_length_and_sector_offset() {
        let request = read_macro(3, Slot::S2, MacroIndex::M3, 42).unwrap();
        assert_eq!(request.header.cmd, 0x0102);
        assert_eq!(request.header.subcmd, 0x0301);
        assert_eq!(request.header.offset, 0x2000);
        assert_eq!(request.header.total_len, 420);
        assert_eq!(request.header.chunk_len, 420);
        assert!(request.payload.is_empty());

        let bytes = request.encode_ble_payloadless(PacketKind::App).unwrap();
        assert_eq!(bytes.len(), 17);
        assert_eq!(bytes[0], 0x04);
        let parsed = CustomHeader::parse(&bytes[1..]).unwrap();
        assert_eq!(parsed.cmd, 0x0102);
        assert_eq!(parsed.subcmd, 0x0301);
        assert_eq!(parsed.chunk_len, 420);
        assert_eq!(parsed.total_len, 420);
        assert_eq!(parsed.offset, 0x2000);
    }

    #[test]
    fn read_macro_rejects_oversized_requests() {
        assert!(matches!(
            read_macro(
                0,
                Slot::S1,
                MacroIndex::M1,
                macro_data::MAX_MACRO_STEPS as u16 + 1
            ),
            Err(Error::TooManySteps { .. })
        ));
        assert!(matches!(
            read_macro(0, Slot::S1, MacroIndex::M1, 0x1000 / 10 + 1),
            Err(Error::RangeOutOfBounds { .. })
        ));
    }

    #[test]
    fn write_macro_aligns_and_chunks() {
        let steps = [MacroStep::neutral(10); 20];
        let packets = write_macro_steps(0, Slot::S1, MacroIndex::M1, &steps, 64).unwrap();
        // 20 * 10 = 200 -> 224 bytes -> 4 packets of 64/64/64/32.
        assert_eq!(packets.len(), 4);
        assert_eq!(packets[0].header.total_len, 224);
        assert_eq!(packets[0].header.offset, 0x0000);
        assert_eq!(packets[1].header.offset, 0x0040);
        assert_eq!(packets[2].header.offset, 0x0080);
        assert_eq!(packets[3].header.offset, 0x00C0);
        assert_eq!(packets[3].payload.len(), 32);
        for packet in &packets {
            assert_eq!(packet.header.chunk_len as usize, packet.payload.len());
            assert_eq!(
                packet.header.crc16,
                crate::crc::packet_crc16(&packet.payload)
            );
        }
    }

    #[test]
    fn write_macro_rejects_too_many_steps() {
        let steps = alloc::vec![MacroStep::neutral(1); macro_data::MAX_MACRO_STEPS + 1];
        assert!(matches!(
            write_macro_steps(0, Slot::S1, MacroIndex::M1, &steps, 64),
            Err(Error::TooManySteps { .. })
        ));
    }

    #[test]
    fn write_macro_payload_clamps_chunk_size_to_64() {
        let payload = [0x33u8; 160];
        let packets = write_macro_payload(0, Slot::S2, MacroIndex::M2, &payload, 128).unwrap();
        // 160 bytes clamped to 64-byte chunks: 64 + 64 + 32.
        assert_eq!(packets.len(), 3);
        assert_eq!(packets[0].header.offset, 0x1000);
        assert_eq!(packets[1].header.offset, 0x1040);
        assert_eq!(packets[2].header.offset, 0x1080);
        assert_eq!(packets[2].payload.len(), 32);
        for packet in &packets {
            assert!(packet.payload.len() <= 64);
            assert!(packet.header.chunk_len <= 0x40);
            assert_eq!(packet.header.chunk_len as usize, packet.payload.len());
            assert_eq!(
                packet.header.crc16,
                crate::crc::packet_crc16(&packet.payload)
            );
        }
    }

    #[test]
    fn write_macro_payload_rejects_oversized_payload() {
        let payload = alloc::vec![0u8; MACRO_MAX_PAYLOAD + 1];
        assert!(matches!(
            write_macro_payload(0, Slot::S1, MacroIndex::M1, &payload, 64),
            Err(Error::PayloadTooLarge {
                max: MACRO_MAX_PAYLOAD,
                ..
            })
        ));
        assert!(matches!(
            write_macro_payload(0, Slot::S1, MacroIndex::M1, &payload[..32], 0),
            Err(Error::InvalidChunkSize { actual: 0, .. })
        ));
    }

    #[test]
    fn write_macro_payload_requires_32_byte_alignment_and_known_chunk_sizes() {
        let unaligned = [0u8; 33];
        assert!(matches!(
            write_macro_payload(0, Slot::S1, MacroIndex::M1, &unaligned, 64),
            Err(Error::UnalignedPayloadLength {
                actual: 33,
                alignment: 32,
            })
        ));

        let aligned = [0u8; 32];
        for chunk_size in [1usize, 48] {
            assert!(matches!(
                write_macro_payload(0, Slot::S1, MacroIndex::M1, &aligned, chunk_size),
                Err(Error::UnsupportedChunkSize { actual }) if actual == chunk_size
            ));
        }
        assert!(
            write_macro_payload(0, Slot::S1, MacroIndex::M1, &aligned, MACRO_USB_CHUNK_SIZE)
                .is_ok()
        );
    }

    #[test]
    fn write_macro_rejects_empty_payloads() {
        assert!(matches!(
            write_macro_payload(0, Slot::S1, MacroIndex::M1, &[], 64),
            Err(Error::EmptyPayload)
        ));
        let no_steps: [MacroStep; 0] = [];
        assert!(matches!(
            write_macro_steps(0, Slot::S1, MacroIndex::M1, &no_steps, 64),
            Err(Error::EmptyPayload)
        ));
    }

    #[test]
    fn erase_macro_declares_one_sector_without_payload() {
        let request = erase_macro(3, Slot::S1, MacroIndex::M2);
        assert_eq!(request.header.cmd, 0x0104);
        assert_eq!(request.header.total_len, 0x1000);
        assert_eq!(request.header.chunk_len, 0x1000);
        assert_eq!(request.header.offset, 0x1000);
        assert_eq!(request.header.subcmd, 0x0300);
        assert_eq!(request.header.crc16, 0xFFFF);
        assert!(request.payload.is_empty());
    }

    #[test]
    fn erase_macro_encodes_payloadless_with_declared_chunk_len() {
        let request = erase_macro(3, Slot::S1, MacroIndex::M2);

        let ble = request.encode_ble_payloadless(PacketKind::App).unwrap();
        assert_eq!(ble.len(), 17);
        assert_eq!(ble[0], 0x04);
        let header = CustomHeader::parse(&ble[1..17]).unwrap();
        assert_eq!(header.chunk_len, 0x1000);
        assert_eq!(header.total_len, 0x1000);
        assert_eq!(header.offset, 0x1000);
        assert_eq!(header.cmd, 0x0104);
        assert_eq!(header.crc16, 0xFFFF);

        let usb = request.encode_usb_app_payloadless().unwrap();
        assert_eq!(usb.len(), 65);
        assert_eq!(&usb[0..2], &[0x81, 0x04]);
        let header = CustomHeader::parse(&usb[2..18]).unwrap();
        assert_eq!(header.chunk_len, 0x1000);
        assert_eq!(header.total_len, 0x1000);
        assert_eq!(header.crc16, 0xFFFF);
        assert!(usb[18..].iter().all(|&byte| byte == 0));

        // The normal encoders must keep rejecting the declared-length mismatch.
        assert_eq!(
            request.encode_ble(PacketKind::App),
            Err(Error::ChunkLenMismatch {
                declared: 0x1000,
                actual: 0,
            })
        );
        assert_eq!(
            request.encode_usb_app(),
            Err(Error::ChunkLenMismatch {
                declared: 0x1000,
                actual: 0,
            })
        );
    }

    #[test]
    fn payloadless_encoder_rejects_payload_and_other_commands() {
        let mut with_payload = erase_macro(3, Slot::S1, MacroIndex::M2);
        with_payload.payload.push(0);
        assert!(matches!(
            with_payload.encode_ble_payloadless(PacketKind::App),
            Err(Error::UnexpectedPayload { actual: 1 })
        ));
        assert!(matches!(
            with_payload.encode_usb_app_payloadless(),
            Err(Error::UnexpectedPayload { actual: 1 })
        ));

        let save_request = save(SaveTransport::Ble);
        assert!(matches!(
            save_request.encode_ble_payloadless(PacketKind::App),
            Err(Error::InvalidFrame(_))
        ));

        let boot_request = erase_macro(3, Slot::S1, MacroIndex::M2);
        assert!(matches!(
            boot_request.encode_ble_payloadless(PacketKind::Boot),
            Err(Error::InvalidFrame(_))
        ));

        let mut bad_offset = erase_macro(3, Slot::S1, MacroIndex::M2);
        bad_offset.header.offset = 0x0800;
        assert!(matches!(
            bad_offset.encode_ble_payloadless(PacketKind::App),
            Err(Error::InvalidFrame(_))
        ));
        assert!(matches!(
            bad_offset.encode_usb_app_payloadless(),
            Err(Error::InvalidFrame(_))
        ));

        let mut bad_crc = erase_macro(3, Slot::S1, MacroIndex::M2);
        bad_crc.header.crc16 = 0;
        assert!(matches!(
            bad_crc.encode_ble_payloadless(PacketKind::App),
            Err(Error::InvalidFrame(_))
        ));
        assert!(matches!(
            bad_crc.encode_usb_app_payloadless(),
            Err(Error::InvalidFrame(_))
        ));
    }

    #[test]
    fn enter_boot_matches_spec() {
        let request = enter_boot();
        assert_eq!(request.header.cmd, 0x000C);
        assert_eq!(request.header.subcmd, 0x0067);
        assert_eq!(request.header.total_len, 4);
        assert_eq!(request.header.offset, 4);
    }

    #[test]
    fn ble_encoding_uses_app_prefix() {
        let packet = save(SaveTransport::Ble)
            .encode_ble(PacketKind::App)
            .unwrap();
        assert_eq!(packet[0], 0x04);
        assert_eq!(packet.len(), 17);
    }

    #[test]
    fn boot_read_pid_and_version() {
        assert_eq!(boot::read_pid().header.cmd, 0x0008);
        assert_eq!(boot::read_pid().header.subcmd, 0x0067);
        assert_eq!(boot::read_pid().header.total_len, 2);
        assert_eq!(boot::read_version().header.cmd, 0x0005);
        assert_eq!(boot::read_version().header.offset, 0x0101_7040);
    }

    #[test]
    fn boot_firmware_chunks_have_crc() {
        let image = [0x5Au8; 128];
        let packets =
            boot::write_firmware_packets(0x0800_0000, image.len() as u32, &image).unwrap();
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[1].header.offset, 0x0800_0040);
        assert_eq!(packets[0].payload, [0x5A; 64]);
        for packet in &packets {
            assert_eq!(packet.header.chunk_len, 0x40);
            assert_eq!(
                packet.header.crc16,
                crate::crc::packet_crc16(&packet.payload)
            );
        }
    }

    #[test]
    fn boot_firmware_rejects_empty_short_and_mismatched_images() {
        assert!(matches!(
            boot::write_firmware_packets(0x0800_0000, 0, &[]),
            Err(Error::EmptyFirmwareImage)
        ));
        assert!(matches!(
            boot::write_firmware_packets(0x0800_0000, 0, &[0u8; 64]),
            Err(Error::EmptyFirmwareImage)
        ));

        for len in [63usize, 65] {
            let image = alloc::vec![0x5Au8; len];
            assert!(matches!(
                boot::write_firmware_packets(0x0800_0000, len as u32, &image),
                Err(Error::UnalignedImageLength {
                    actual,
                    alignment: 0x40,
                }) if actual == len
            ));
        }

        let image = [0x5Au8; 128];
        assert!(matches!(
            boot::write_firmware_packets(0x0800_0000, 64, &image),
            Err(Error::ImageLengthMismatch {
                declared: 64,
                actual: 128,
            })
        ));
        for len in [0usize, 1, 63] {
            assert!(matches!(
                boot::write_firmware(0x0800_0000, 128, &alloc::vec![0u8; len]),
                Err(Error::InvalidLength {
                    expected: 0x40,
                    actual,
                }) if actual == len
            ));
        }
        assert!(boot::write_firmware(0x0800_0000, 128, &[0u8; 64]).is_ok());
        assert!(matches!(
            boot::write_firmware(0x0800_0000, 128, &[0u8; 65]),
            Err(Error::InvalidLength {
                expected: 0x40,
                actual: 65,
            })
        ));
    }

    #[test]
    fn boot_firmware_requires_aligned_base_and_rejects_address_overflow() {
        let image = [0x5Au8; 128];
        assert!(matches!(
            boot::write_firmware_packets(0x0800_0020, 128, &image),
            Err(Error::UnalignedAddress {
                address: 0x0800_0020,
                alignment: 0x40,
            })
        ));
        assert!(matches!(
            boot::write_firmware_packets(0xFFFF_FFC0, 128, &image),
            Err(Error::AddressOverflow {
                base: 0xFFFF_FFC0,
                offset: 64,
            })
        ));
        // A single chunk ending exactly at 0xFFFF_FFFF is still valid.
        assert!(boot::write_firmware_packets(0xFFFF_FFC0, 64, &[0u8; 64]).is_ok());
    }

    #[test]
    fn boot_write_firmware_enforces_ota_primitive_guards() {
        let chunk = [0x5Au8; 64];
        assert!(matches!(
            boot::write_firmware(0x0800_0000, 0, &chunk),
            Err(Error::EmptyFirmwareImage)
        ));
        assert!(matches!(
            boot::write_firmware(0x0800_0000, 65, &chunk),
            Err(Error::UnalignedImageLength {
                actual: 65,
                alignment: 0x40,
            })
        ));
        assert!(matches!(
            boot::write_firmware(0x0800_0020, 128, &chunk),
            Err(Error::UnalignedAddress {
                address: 0x0800_0020,
                alignment: 0x40,
            })
        ));
        assert!(matches!(
            boot::write_firmware(0xFFFF_FFFF, 128, &chunk),
            Err(Error::AddressOverflow {
                base: 0xFFFF_FFFF,
                offset: 64,
            })
        ));
        assert!(boot::write_firmware(0xFFFF_FFC0, 64, &chunk).is_ok());
    }

    #[test]
    fn boot_erase_requires_sector_alignment() {
        assert!(boot::erase_flash(0x0800_0000).is_ok());
        assert!(boot::erase_flash(0x0800_1000).is_ok());
        assert!(matches!(
            boot::erase_flash(0x0800_0040),
            Err(Error::UnalignedAddress {
                address: 0x0800_0040,
                alignment: 0x1000,
            })
        ));
    }

    #[test]
    fn boot_version_writes() {
        let version = boot::write_firmware_version(0x0102_0304);
        assert_eq!(version.header.cmd, 0x0002);
        assert_eq!(version.header.offset, 0x0101_7040);
        assert_eq!(version.payload, 0x0102_0304u32.to_le_bytes());
        assert_eq!(
            version.header.crc16,
            crate::crc::packet_crc16(&version.payload)
        );

        let mut header = [0x11u8; 24];
        header[8..12].copy_from_slice(&0x1000u32.to_le_bytes());
        let request = boot::write_version_header(0x1000, &header).unwrap();
        assert_eq!(request.header.cmd, 0x00C4);
        assert_eq!(request.header.chunk_len, 24);
        assert_eq!(request.header.total_len, 0x1000);
        assert_eq!(request.payload, header);

        for len in [23usize, 25] {
            assert!(matches!(
                boot::write_version_header(0x1000, &alloc::vec![0u8; len]),
                Err(Error::InvalidLength {
                    expected: 24,
                    actual,
                }) if actual == len
            ));
        }
        assert!(matches!(
            boot::write_version_header(0x2000, &header),
            Err(Error::ImageLengthMismatch {
                declared: 0x2000,
                actual: 0x1000,
            })
        ));

        let mut zero_header = header;
        zero_header[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            boot::write_version_header(0, &zero_header),
            Err(Error::EmptyFirmwareImage)
        ));
        let mut short_header = header;
        short_header[8..12].copy_from_slice(&65u32.to_le_bytes());
        assert!(matches!(
            boot::write_version_header(65, &short_header),
            Err(Error::UnalignedImageLength {
                actual: 65,
                alignment: 0x40,
            })
        ));

        let appid = boot::write_appid(&[0x42u8; 12]).unwrap();
        assert_eq!(appid.header.total_len, 0x0C);
        assert_eq!(appid.header.offset, 0x0801_401C);
        assert_eq!(appid.header.crc16, crate::crc::packet_crc16(&[0x42; 12]));
        for len in [11usize, 13] {
            assert!(matches!(
                boot::write_appid(&alloc::vec![0u8; len]),
                Err(Error::InvalidLength {
                    expected: 12,
                    actual,
                }) if actual == len
            ));
        }
    }

    #[test]
    fn boot_jump_and_encode() {
        let request = boot::jump_app();
        assert_eq!(request.header.cmd, 0x0006);
        assert_eq!(request.header.offset, 0x0101_8000);
        let bytes = boot::encode(&request).unwrap();
        assert_eq!(bytes[0], 0x05);
    }
}
