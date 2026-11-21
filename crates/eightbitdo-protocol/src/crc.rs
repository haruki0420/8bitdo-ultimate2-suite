//! The two CRC-16 variants used by the Ultimate 2 protocol.
//!
//! Both run LSB-first with initial value `0xFFFF` and no final XOR, but they
//! use different polynomials and must not be mixed up:
//!
//! | Function | Equivalent | Polynomial | Use |
//! | --- | --- | --- | --- |
//! | [`ansi_crc16_sh`] | CRC-16/MCRF4XX | `0x8408` | 2768-byte config struct (`+0x0C`) |
//! | [`packet_crc16`] | CRC-16/MODBUS | `0xA001` | packet header `+0x06`, bootloader writes |

const POLY_ANSI: u16 = 0x8408;
const POLY_PACKET: u16 = 0xA001;

const fn update(mut crc: u16, data: &[u8], poly: u16) -> u16 {
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i] as u16;
        let mut bit = 0;
        while bit < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ poly;
            } else {
                crc >>= 1;
            }
            bit += 1;
        }
        i += 1;
    }
    crc
}

/// CRC-16/MCRF4XX (`poly 0x8408`) used for the 2768-byte config structure.
///
/// The official implementation returns `0xFFFF` for empty input and stores
/// the result in `record_ultimatebt2[0x0C..0x10]` as a little-endian `u32`.
///
/// ```
/// assert_eq!(eightbitdo_protocol::ansi_crc16_sh(b"123456789"), 0x6F91);
/// assert_eq!(eightbitdo_protocol::ansi_crc16_sh(b""), 0xFFFF);
/// ```
pub fn ansi_crc16_sh(data: &[u8]) -> u16 {
    update(0xFFFF, data, POLY_ANSI)
}

/// CRC-16/MODBUS (`poly 0xA001`) used for packet headers and OTA chunks.
///
/// ```
/// assert_eq!(eightbitdo_protocol::packet_crc16(b"123456789"), 0x4B37);
/// assert_eq!(eightbitdo_protocol::packet_crc16(b""), 0xFFFF);
/// ```
pub fn packet_crc16(data: &[u8]) -> u16 {
    update(0xFFFF, data, POLY_PACKET)
}

/// Incremental CRC-16 state, useful when a payload arrives in several chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crc16 {
    state: u16,
    poly: u16,
}

impl Crc16 {
    /// Start a CRC-16/MCRF4XX accumulator (`0x8408`).
    #[must_use]
    pub const fn ansi() -> Self {
        Self {
            state: 0xFFFF,
            poly: POLY_ANSI,
        }
    }

    /// Start a CRC-16/MODBUS accumulator (`0xA001`).
    #[must_use]
    pub const fn packet() -> Self {
        Self {
            state: 0xFFFF,
            poly: POLY_PACKET,
        }
    }

    /// Feed more bytes into the accumulator.
    pub fn update(&mut self, data: &[u8]) {
        self.state = update(self.state, data, self.poly);
    }

    /// Return the current CRC value.
    #[must_use]
    pub const fn finish(self) -> u16 {
        self.state
    }
}

impl Default for Crc16 {
    fn default() -> Self {
        Self::ansi()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECK: &[u8] = b"123456789";

    #[test]
    fn known_vectors() {
        assert_eq!(ansi_crc16_sh(CHECK), 0x6F91);
        assert_eq!(packet_crc16(CHECK), 0x4B37);
    }

    #[test]
    fn empty_input_is_ffff() {
        assert_eq!(ansi_crc16_sh(&[]), 0xFFFF);
        assert_eq!(packet_crc16(&[]), 0xFFFF);
    }

    #[test]
    fn incremental_matches_one_shot() {
        let mut ansi = Crc16::ansi();
        for chunk in CHECK.chunks(2) {
            ansi.update(chunk);
        }
        assert_eq!(ansi.finish(), ansi_crc16_sh(CHECK));

        let mut packet = Crc16::packet();
        for chunk in CHECK.chunks(4) {
            packet.update(chunk);
        }
        assert_eq!(packet.finish(), packet_crc16(CHECK));
    }

    #[test]
    fn all_zero_and_all_ff_are_stable() {
        // Regression guards: catches accidental polynomial / direction changes.
        assert_eq!(ansi_crc16_sh(&[0u8; 2768]), 0x0BBC);
        assert_eq!(packet_crc16(&[0xFFu8; 64]), 0xBF01);
    }
}
