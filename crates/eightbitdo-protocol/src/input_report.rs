//! 12-byte realtime input report (`cmd 0x0003` / `SetReportEnable`).
//!
//! Layout (little-endian):
//!
//! | Offset | Size | Field |
//! | --- | --- | --- |
//! | `0x00` | 4B | `Key` button mask |
//! | `0x04` | 1B | left stick X |
//! | `0x05` | 1B | left stick Y |
//! | `0x06` | 1B | right stick X |
//! | `0x07` | 1B | right stick Y |
//! | `0x08` | 1B | right trigger |
//! | `0x09` | 1B | left trigger |
//! | `0x0A` | 1B | battery |
//! | `0x0B` | 1B | reserved |

use crate::error::Error;
use crate::keymap::KeyMap;

/// Size of one realtime input report.
pub const INPUT_REPORT_SIZE: usize = 12;

/// A decoded realtime input report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InputReport {
    /// Raw 32-bit button mask.
    pub keys: u32,
    /// Left stick X (`0..=255`, neutral around `127`/`128`).
    pub left_stick_x: u8,
    /// Left stick Y.
    pub left_stick_y: u8,
    /// Right stick X.
    pub right_stick_x: u8,
    /// Right stick Y.
    pub right_stick_y: u8,
    /// Right trigger (`0..=255`).
    pub trigger_r: u8,
    /// Left trigger (`0..=255`).
    pub trigger_l: u8,
    /// Battery level reported by the controller.
    pub battery: u8,
    /// Reserved byte.
    pub reserved: u8,
}

impl InputReport {
    /// Decode a 12-byte notification payload.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < INPUT_REPORT_SIZE {
            return Err(Error::InvalidLength {
                expected: INPUT_REPORT_SIZE,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            keys: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            left_stick_x: bytes[4],
            left_stick_y: bytes[5],
            right_stick_x: bytes[6],
            right_stick_y: bytes[7],
            trigger_r: bytes[8],
            trigger_l: bytes[9],
            battery: bytes[10],
            reserved: bytes[11],
        })
    }

    /// Encode back into the 12-byte wire representation.
    #[must_use]
    pub fn to_bytes(self) -> [u8; INPUT_REPORT_SIZE] {
        let mut out = [0u8; INPUT_REPORT_SIZE];
        out[0..4].copy_from_slice(&self.keys.to_le_bytes());
        out[4] = self.left_stick_x;
        out[5] = self.left_stick_y;
        out[6] = self.right_stick_x;
        out[7] = self.right_stick_y;
        out[8] = self.trigger_r;
        out[9] = self.trigger_l;
        out[10] = self.battery;
        out[11] = self.reserved;
        out
    }

    /// Button mask as a [`KeyMap`].
    #[must_use]
    pub const fn key_map(self) -> KeyMap {
        KeyMap::from_bits(self.keys)
    }

    /// Left stick as `(x, y)` in `-128..=128`, with the `127`/`128` center
    /// snapped to `(0, 0)`.
    #[must_use]
    pub const fn left_stick(self) -> (i16, i16) {
        normalize_pair(self.left_stick_x, self.left_stick_y)
    }

    /// Right stick as `(x, y)` in `-128..=128`, with the center snapped to
    /// `(0, 0)`.
    #[must_use]
    pub const fn right_stick(self) -> (i16, i16) {
        normalize_pair(self.right_stick_x, self.right_stick_y)
    }
}

const fn is_center(value: u8) -> bool {
    value == 127 || value == 128
}

const fn normalize_pair(x: u8, y: u8) -> (i16, i16) {
    if is_center(x) && is_center(y) {
        (0, 0)
    } else {
        (x as i16 - 127, y as i16 - 127)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> [u8; INPUT_REPORT_SIZE] {
        [
            0x01, 0x20, 0x00, 0x00, // keys = 0x0000_2001 (A | Start)
            0x7F, 0x80, // left centered
            0xFF, 0x00, // right = right+up
            0xCC, 0x33, // R=204, L=51
            0x4B, 0x00, // battery 75
        ]
    }

    #[test]
    fn decodes_all_fields() {
        let report = InputReport::from_bytes(&sample()).unwrap();
        assert_eq!(report.keys, 0x0000_2001);
        assert_eq!(report.key_map(), KeyMap::START | KeyMap::A);
        assert_eq!(report.left_stick(), (0, 0));
        assert_eq!(report.right_stick(), (128, -127));
        assert_eq!(report.trigger_r, 0xCC);
        assert_eq!(report.trigger_l, 0x33);
        assert_eq!(report.battery, 0x4B);
        assert_eq!(report.reserved, 0);
    }

    #[test]
    fn roundtrip() {
        let report = InputReport::from_bytes(&sample()).unwrap();
        assert_eq!(report.to_bytes(), sample());
    }

    #[test]
    fn sticks_use_signed_range() {
        let report = InputReport {
            left_stick_x: 0,
            left_stick_y: 0,
            ..InputReport::default()
        };
        assert_eq!(report.left_stick(), (-127, -127));

        let report = InputReport {
            right_stick_x: 255,
            right_stick_y: 127,
            ..InputReport::default()
        };
        assert_eq!(report.right_stick(), (128, 0));
    }

    #[test]
    fn center_tolerates_127_and_128_mix() {
        let report = InputReport {
            left_stick_x: 128,
            left_stick_y: 127,
            ..InputReport::default()
        };
        assert_eq!(report.left_stick(), (0, 0));

        let report = InputReport {
            left_stick_x: 128,
            left_stick_y: 0,
            ..InputReport::default()
        };
        assert_eq!(report.left_stick(), (1, -127));
    }

    #[test]
    fn rejects_short_buffers() {
        assert_eq!(
            InputReport::from_bytes(&[0u8; 11]),
            Err(Error::InvalidLength {
                expected: 12,
                actual: 11
            })
        );
    }
}
