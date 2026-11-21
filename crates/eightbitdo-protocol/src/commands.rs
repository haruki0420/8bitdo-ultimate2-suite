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

/// Magic `total_len` / `offset` used by `ReadVersion`.
pub const VERSION_QUERY: u32 = 0x0102_0030;

/// `ReadVersion` response payload length (version string + metadata).
pub const VERSION_RESPONSE_LEN: usize = 0x30;

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
}

/// `ReadConfig` for an arbitrary range.
#[must_use]
pub fn read_config(offset: u32, len: u32) -> Request {
    Request::new(cmd::READ_CONFIG, 0, len, offset)
}

/// Read the full 2768-byte configuration block.
#[must_use]
pub fn read_config_all() -> Request {
    read_config(0, CONFIG_SIZE as u32)
}

/// Read only the 116-byte header (`0x000..0x073`).
#[must_use]
pub fn read_config_header() -> Request {
    read_config(0, 0x0074)
}

/// Read the current slot value (`0x012`, 2 bytes).
#[must_use]
pub fn read_current_slot() -> Request {
    read_config(0x0012, 2)
}

/// `WriteConfig` for an arbitrary range.
///
/// `total_len` is always the full 2768 bytes, matching the official partial
/// write path. The payload may be shorter than the total.
#[must_use]
pub fn write_config(offset: u32, data: &[u8]) -> Request {
    Request::new(cmd::WRITE_CONFIG, 0, CONFIG_SIZE as u32, offset).with_payload(data)
}

/// Split a configuration write into transport-sized packets.
///
/// Offsets advance by `chunk_size`; `total_len` stays `0x0AD0`.
#[must_use]
pub fn write_config_packets(offset: u32, data: &[u8], chunk_size: usize) -> Vec<Request> {
    assert!(chunk_size > 0, "chunk_size must be positive");
    data.chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| write_config(offset + (i * chunk_size) as u32, chunk))
        .collect()
}

/// Commit pending configuration writes to flash.
#[must_use]
pub fn save(transport: SaveTransport) -> Request {
    Request::new(cmd::SAVE, transport.subcmd(), 0, 0)
}

/// Read the firmware version.
#[must_use]
pub fn read_version() -> Request {
    Request::new(cmd::READ_VERSION, 0, VERSION_QUERY, VERSION_QUERY)
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
#[must_use]
pub fn read_macro(gamepad_mode: u16, slot: Slot, macro_idx: MacroIndex, steps: u16) -> Request {
    let total_len = u32::from(steps) * 10;
    Request::new(
        cmd::READ_MACRO,
        macro_subcmd(gamepad_mode, slot),
        total_len,
        macro_idx.sector_offset(),
    )
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
    Ok(write_macro_payload(
        gamepad_mode,
        slot,
        macro_idx,
        &payload,
        chunk_size,
    ))
}

/// Write a pre-encoded macro payload (already 32-byte aligned).
#[must_use]
pub fn write_macro_payload(
    gamepad_mode: u16,
    slot: Slot,
    macro_idx: MacroIndex,
    payload: &[u8],
    chunk_size: usize,
) -> Vec<Request> {
    assert!(chunk_size > 0, "chunk_size must be positive");
    let total_len = payload.len() as u32;
    let base = macro_idx.sector_offset();
    payload
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
        .collect()
}

/// Erase one macro flash sector (`0x1000` bytes).
#[must_use]
pub fn erase_macro(gamepad_mode: u16, slot: Slot, macro_idx: MacroIndex) -> Request {
    Request::new(
        cmd::ERASE_MACRO,
        macro_subcmd(gamepad_mode, slot),
        0x1000,
        macro_idx.sector_offset(),
    )
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
    #[must_use]
    pub fn erase_flash(addr: u32) -> Request {
        Request::new(CMD_ERASE_FLASH, 0, SECTOR_SIZE, addr)
    }

    /// Write one firmware chunk (at most 64 bytes) with CRC-16/MODBUS.
    #[must_use]
    pub fn write_firmware(addr: u32, image_len: u32, chunk: &[u8]) -> Request {
        let mut request = Request::new(CMD_WRITE_FIRMWARE, 0, image_len, addr);
        request.header.crc16 = crate::crc::packet_crc16(chunk);
        request.header.chunk_len = chunk.len() as u16;
        request.payload = chunk.to_vec();
        request
    }

    /// Split a firmware image into 64-byte write packets.
    #[must_use]
    pub fn write_firmware_packets(addr: u32, image_len: u32, image: &[u8]) -> Vec<Request> {
        image
            .chunks(CHUNK_SIZE)
            .enumerate()
            .map(|(i, chunk)| write_firmware(addr + (i * CHUNK_SIZE) as u32, image_len, chunk))
            .collect()
    }

    /// Write the 24-byte image header (`cmd 0x00C4`).
    #[must_use]
    pub fn write_version_header(image_len: u32, header24: &[u8]) -> Request {
        let mut request = Request::new(CMD_WRITE_VERSION, 0, image_len, 0);
        request.header.chunk_len = header24.len() as u16;
        request.payload = header24.to_vec();
        request
    }

    /// Write the 4-byte firmware version value.
    #[must_use]
    pub fn write_firmware_version(version: u32) -> Request {
        Request::new(CMD_WRITE_FIRMWARE_VERSION, 0, 4, VERSION_ADDR)
            .with_crc_payload(&version.to_le_bytes())
    }

    /// Write the 12-byte encrypted APPID (not used for `gamepad_type == 1`).
    #[must_use]
    pub fn write_appid(serial: &[u8]) -> Request {
        Request::new(CMD_WRITE_FIRMWARE_VERSION, 0, 0x0C, APPID_ADDR).with_crc_payload(serial)
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
        assert_eq!(request.header.offset, 0);
        assert!(request.payload.is_empty());

        let header = read_config_header();
        assert_eq!(header.header.total_len, 0x74);
        assert_eq!(read_current_slot().header.offset, 0x12);
    }

    #[test]
    fn write_config_is_cmd_1_with_full_total_len() {
        let request = write_config(0x74, &[1, 2, 3, 4]);
        assert_eq!(request.header.cmd, 0x0001);
        assert_eq!(request.header.subcmd, 0);
        assert_eq!(request.header.total_len, 0x0AD0);
        assert_eq!(request.header.offset, 0x74);
        assert_eq!(request.header.chunk_len, 4);
        assert_eq!(request.payload, [1, 2, 3, 4]);
    }

    #[test]
    fn config_packets_advance_offsets() {
        let data = [0xAAu8; 100];
        let packets = write_config_packets(0x100, &data, 32);
        assert_eq!(packets.len(), 4);
        assert_eq!(packets[0].header.offset, 0x100);
        assert_eq!(packets[1].header.offset, 0x120);
        assert_eq!(packets[2].header.offset, 0x140);
        assert_eq!(packets[3].header.offset, 0x160);
        assert_eq!(packets[3].payload.len(), 4);
        for packet in &packets {
            assert_eq!(packet.header.total_len, 0x0AD0);
        }
    }

    #[test]
    fn save_subcommands_differ_by_transport() {
        assert_eq!(save(SaveTransport::Ble).header.subcmd, 0x0067);
        assert_eq!(save(SaveTransport::Usb).header.subcmd, 0x0123);
        assert_eq!(save(SaveTransport::Ble).header.cmd, 0x0006);
    }

    #[test]
    fn version_query_magic() {
        let request = read_version();
        assert_eq!(request.header.cmd, 0x000B);
        assert_eq!(request.header.total_len, 0x0102_0030);
        assert_eq!(request.header.offset, 0x0102_0030);
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
        let request = read_macro(3, Slot::S2, MacroIndex::M3, 42);
        assert_eq!(request.header.cmd, 0x0102);
        assert_eq!(request.header.subcmd, 0x0301);
        assert_eq!(request.header.offset, 0x2000);
        assert_eq!(request.header.total_len, 420);
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
    fn erase_macro_is_one_sector() {
        let request = erase_macro(3, Slot::S1, MacroIndex::M2);
        assert_eq!(request.header.cmd, 0x0104);
        assert_eq!(request.header.total_len, 0x1000);
        assert_eq!(request.header.offset, 0x1000);
        assert!(request.payload.is_empty());
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
        let packets = boot::write_firmware_packets(0x0800_0000, image.len() as u32, &image);
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[1].header.offset, 0x0800_0040);
        assert_eq!(packets[0].payload, [0x5A; 64]);
        for packet in &packets {
            assert_eq!(
                packet.header.crc16,
                crate::crc::packet_crc16(&packet.payload)
            );
        }
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

        let header = [0x11u8; 24];
        let request = boot::write_version_header(0x1000, &header);
        assert_eq!(request.header.cmd, 0x00C4);
        assert_eq!(request.header.chunk_len, 24);
        assert_eq!(request.header.total_len, 0x1000);
        assert_eq!(request.payload, header);
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
