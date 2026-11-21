//! 16-byte custom header (`cumstom_header_t`) and transport framing.
//!
//! The controller uses the same header over three transports, with different
//! wrappers:
//!
//! | Transport | Direction | Bytes |
//! | --- | --- | --- |
//! | BLE App (`"80ND"`) | TX | `0x04` + header + payload |
//! | BLE Bootloader (`"8bt"`) | TX | `0x05` + header + payload |
//! | BLE (`0x600F`) | RX | header + payload (no prefix) |
//! | USB HID App | TX | `0x81 0x04` + header + payload, padded to 65 bytes |
//! | USB HID App | RX | `0x02 0x04` + header + payload |

use alloc::vec::Vec;
use core::fmt;

use crate::crc::packet_crc16;
use crate::error::Error;

/// Size of `cumstom_header_t` in bytes.
pub const HEADER_SIZE: usize = 16;

/// Size of a USB HID report including the leading report ID byte.
pub const USB_REPORT_SIZE: usize = 65;

/// Maximum payload carried by one USB HID report (`0x20`).
pub const USB_MAX_PAYLOAD: usize = 0x20;

/// BLE App Mode packet prefix written before the header.
pub const BLE_APP_PREFIX: u8 = 0x04;

/// BLE Bootloader Mode packet prefix written before the header.
pub const BLE_BOOT_PREFIX: u8 = 0x05;

/// First two bytes of an App Mode USB HID report sent to the controller.
pub const USB_APP_TX_PREFIX: [u8; 2] = [0x81, 0x04];

/// First two bytes of an App Mode USB HID report received from the controller.
pub const USB_APP_RX_PREFIX: [u8; 2] = [0x02, 0x04];

/// First two bytes of a Bootloader USB HID report received from the controller.
pub const USB_BOOT_RX_PREFIX: [u8; 2] = [0x00, 0x05];

/// Which BLE channel a packet belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketKind {
    /// App Mode (`"80ND"`, service `FF10`, characteristic `FF11`).
    App,
    /// Bootloader Mode (`"8bt"`, service `FF20`, characteristic `FF21`).
    Boot,
}

impl PacketKind {
    /// BLE prefix byte for this channel (`0x04` or `0x05`).
    #[must_use]
    pub const fn ble_prefix(self) -> u8 {
        match self {
            Self::App => BLE_APP_PREFIX,
            Self::Boot => BLE_BOOT_PREFIX,
        }
    }
}

/// The 16-byte header prepended to every command and response.
///
/// All multi-byte fields are little-endian.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CustomHeader {
    /// Command ID (`0x0001` write config, `0x0002` read config, ...).
    pub cmd: u16,
    /// Sub-command, argument or slot selector.
    pub subcmd: u16,
    /// Number of payload bytes carried by this packet.
    pub chunk_len: u16,
    /// CRC-16/MODBUS of the payload, or `0` when unused.
    pub crc16: u16,
    /// Total transfer size in bytes.
    pub total_len: u32,
    /// Offset of this packet inside the total transfer.
    pub offset: u32,
}

impl CustomHeader {
    /// Build a header with `chunk_len = 0` and `crc16 = 0`.
    #[must_use]
    pub const fn new(cmd: u16, subcmd: u16, total_len: u32, offset: u32) -> Self {
        Self {
            cmd,
            subcmd,
            chunk_len: 0,
            crc16: 0,
            total_len,
            offset,
        }
    }

    /// Set `chunk_len` and return the updated header.
    #[must_use]
    pub const fn with_chunk_len(mut self, chunk_len: u16) -> Self {
        self.chunk_len = chunk_len;
        self
    }

    /// Set `chunk_len` from a payload and compute the packet CRC-16 over it.
    #[must_use]
    pub fn with_payload(mut self, payload: &[u8]) -> Self {
        self.chunk_len = payload.len() as u16;
        self.crc16 = packet_crc16(payload);
        self
    }

    /// Serialize the header to its 16-byte little-endian representation.
    #[must_use]
    pub fn to_bytes(self) -> [u8; HEADER_SIZE] {
        let mut out = [0u8; HEADER_SIZE];
        out[0..2].copy_from_slice(&self.cmd.to_le_bytes());
        out[2..4].copy_from_slice(&self.subcmd.to_le_bytes());
        out[4..6].copy_from_slice(&self.chunk_len.to_le_bytes());
        out[6..8].copy_from_slice(&self.crc16.to_le_bytes());
        out[8..12].copy_from_slice(&self.total_len.to_le_bytes());
        out[12..16].copy_from_slice(&self.offset.to_le_bytes());
        out
    }

    /// Parse a header from at least 16 bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::InvalidLength {
                expected: HEADER_SIZE,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            cmd: u16::from_le_bytes([bytes[0], bytes[1]]),
            subcmd: u16::from_le_bytes([bytes[2], bytes[3]]),
            chunk_len: u16::from_le_bytes([bytes[4], bytes[5]]),
            crc16: u16::from_le_bytes([bytes[6], bytes[7]]),
            total_len: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            offset: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        })
    }
}

impl fmt::Display for CustomHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cmd=0x{:04X} subcmd=0x{:04X} chunk_len={} total_len=0x{:X} offset=0x{:X}",
            self.cmd, self.subcmd, self.chunk_len, self.total_len, self.offset
        )
    }
}

/// Encode a BLE write: 1-byte prefix + 16-byte header + payload.
///
/// The header's `chunk_len` must equal `payload.len()`. The controller does
/// not send the prefix back, so responses are decoded with
/// [`decode_app_response`].
pub fn encode_ble(
    kind: PacketKind,
    header: &CustomHeader,
    payload: &[u8],
) -> Result<Vec<u8>, Error> {
    if header.chunk_len as usize != payload.len() {
        return Err(Error::ChunkLenMismatch {
            declared: header.chunk_len,
            actual: payload.len(),
        });
    }
    let mut out = Vec::with_capacity(1 + HEADER_SIZE + payload.len());
    out.push(kind.ble_prefix());
    out.extend_from_slice(&header.to_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Decode an App Mode BLE notification (`0x600F`: no prefix byte).
///
/// Returns the header and a slice containing exactly `chunk_len` payload
/// bytes. Trailing padding, if a stack delivers any, is ignored.
pub fn decode_app_response(data: &[u8]) -> Result<(CustomHeader, &[u8]), Error> {
    let header = CustomHeader::parse(data)?;
    let end = HEADER_SIZE + header.chunk_len as usize;
    if data.len() < end {
        return Err(Error::InvalidLength {
            expected: end,
            actual: data.len(),
        });
    }
    Ok((header, &data[HEADER_SIZE..end]))
}

/// Encode an App Mode USB HID report (65 bytes: report ID + 64-byte report).
///
/// `payload` is limited to 32 bytes (`0x20`); the remaining report bytes are
/// zero padding.
pub fn encode_usb_app(
    header: &CustomHeader,
    payload: &[u8],
) -> Result<[u8; USB_REPORT_SIZE], Error> {
    if payload.len() > USB_MAX_PAYLOAD {
        return Err(Error::PayloadTooLarge {
            max: USB_MAX_PAYLOAD,
            actual: payload.len(),
        });
    }
    if header.chunk_len as usize != payload.len() {
        return Err(Error::ChunkLenMismatch {
            declared: header.chunk_len,
            actual: payload.len(),
        });
    }
    let mut out = [0u8; USB_REPORT_SIZE];
    out[0] = USB_APP_TX_PREFIX[0];
    out[1] = USB_APP_TX_PREFIX[1];
    out[2..2 + HEADER_SIZE].copy_from_slice(&header.to_bytes());
    out[2 + HEADER_SIZE..2 + HEADER_SIZE + payload.len()].copy_from_slice(payload);
    Ok(out)
}

/// Decode an App Mode USB HID report.
///
/// Accepts either the raw 64-byte report or the 65-byte read buffer with a
/// leading report ID, which is stripped. The remaining data must start with
/// [`USB_APP_RX_PREFIX`].
pub fn decode_usb_app(report: &[u8]) -> Result<(CustomHeader, &[u8]), Error> {
    let data = if report.len() == USB_REPORT_SIZE {
        &report[1..]
    } else {
        report
    };
    if data.len() < 2 + HEADER_SIZE {
        return Err(Error::InvalidLength {
            expected: 2 + HEADER_SIZE,
            actual: data.len(),
        });
    }
    if data[0..2] != USB_APP_RX_PREFIX {
        return Err(Error::InvalidFrame(
            "USB App report must start with 0x02 0x04",
        ));
    }
    let header = CustomHeader::parse(&data[2..])?;
    let end = 2 + HEADER_SIZE + header.chunk_len as usize;
    if data.len() < end {
        return Err(Error::InvalidLength {
            expected: end,
            actual: data.len(),
        });
    }
    Ok((header, &data[2 + HEADER_SIZE..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> CustomHeader {
        CustomHeader {
            cmd: 0x0103,
            subcmd: 0x0301,
            chunk_len: 4,
            crc16: 0x1234,
            total_len: 0x0AD0,
            offset: 0x2000,
        }
    }

    #[test]
    fn header_is_little_endian_on_the_wire() {
        let bytes = sample_header().to_bytes();
        assert_eq!(
            bytes,
            [
                0x03, 0x01, 0x01, 0x03, 0x04, 0x00, 0x34, 0x12, 0xD0, 0x0A, 0x00, 0x00, 0x00, 0x20,
                0x00, 0x00,
            ]
        );
    }

    #[test]
    fn header_roundtrip() {
        let header = sample_header();
        assert_eq!(CustomHeader::parse(&header.to_bytes()), Ok(header));
    }

    #[test]
    fn header_rejects_short_buffers() {
        assert_eq!(
            CustomHeader::parse(&[0u8; 15]),
            Err(Error::InvalidLength {
                expected: 16,
                actual: 15
            })
        );
    }

    #[test]
    fn ble_app_packet_has_prefix_and_payload() {
        let header = CustomHeader::new(0x0001, 0, 0x0AD0, 0x74).with_payload(&[0xAA, 0xBB]);
        let packet = encode_ble(PacketKind::App, &header, &[0xAA, 0xBB]).unwrap();
        assert_eq!(packet.len(), 1 + 16 + 2);
        assert_eq!(packet[0], 0x04);
        assert_eq!(&packet[1..17], &header.to_bytes());
        assert_eq!(&packet[17..], &[0xAA, 0xBB]);
    }

    #[test]
    fn ble_boot_packet_uses_0x05() {
        let header = CustomHeader::new(0x0008, 0x0067, 2, 0);
        let packet = encode_ble(PacketKind::Boot, &header, &[]).unwrap();
        assert_eq!(packet[0], 0x05);
    }

    #[test]
    fn ble_encode_rejects_chunk_len_mismatch() {
        let header = CustomHeader::new(1, 0, 0, 0).with_chunk_len(3);
        assert_eq!(
            encode_ble(PacketKind::App, &header, &[0, 0]),
            Err(Error::ChunkLenMismatch {
                declared: 3,
                actual: 2
            })
        );
    }

    #[test]
    fn ble_response_has_no_prefix() {
        let header = CustomHeader::new(0x000B, 0, 4, 0).with_payload(&[1, 2, 3, 4]);
        let mut rx = header.to_bytes().to_vec();
        rx.extend_from_slice(&[1, 2, 3, 4]);
        // Padding some stacks may append must be ignored.
        rx.extend_from_slice(&[0xFF; 8]);
        let (parsed, payload) = decode_app_response(&rx).unwrap();
        assert_eq!(parsed, header);
        assert_eq!(payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn usb_report_layout() {
        let header = CustomHeader::new(0x0001, 0, 0x0AD0, 0x74).with_payload(&[0x11, 0x22]);
        let report = encode_usb_app(&header, &[0x11, 0x22]).unwrap();
        assert_eq!(report.len(), 65);
        assert_eq!(&report[0..2], &[0x81, 0x04]);
        assert_eq!(&report[2..18], &header.to_bytes());
        assert_eq!(&report[18..20], &[0x11, 0x22]);
        assert!(report[20..].iter().all(|&b| b == 0));
    }

    #[test]
    fn usb_decode_strips_report_id() {
        let header = CustomHeader::new(0x0002, 0, 2, 0).with_payload(&[0x02, 0x04]);
        let mut report = [0u8; 65];
        report[0] = 0x00;
        report[1..3].copy_from_slice(&USB_APP_RX_PREFIX);
        report[3..19].copy_from_slice(&header.to_bytes());
        report[19..21].copy_from_slice(&[0x02, 0x04]);

        let (parsed, payload) = decode_usb_app(&report).unwrap();
        assert_eq!(parsed, header);
        assert_eq!(payload, &[0x02, 0x04]);
    }

    #[test]
    fn usb_decode_rejects_wrong_prefix() {
        let report = [0u8; 64];
        assert!(matches!(
            decode_usb_app(&report),
            Err(Error::InvalidFrame(_))
        ));
    }

    #[test]
    fn usb_rejects_large_payload() {
        let header = CustomHeader::new(1, 0, 0, 0).with_chunk_len(33);
        assert_eq!(
            encode_usb_app(&header, &[0u8; 33]),
            Err(Error::PayloadTooLarge {
                max: 32,
                actual: 33
            })
        );
    }
}
