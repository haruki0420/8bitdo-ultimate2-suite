//! 10-byte macro step codec (`MacroDatas`) and flash layout helpers.
//!
//! Each step is five little-endian `u16` words. For convenience this crate
//! exposes the packed word as separate 8-bit axis / trigger values, which is
//! how the GUI and JSON format present them.

use alloc::vec::Vec;

use crate::error::Error;
use crate::keymap::KeyMap;

/// Size of one macro step on the wire.
pub const MACRO_STEP_SIZE: usize = 10;

/// Maximum number of steps per macro (200 steps = 2000 bytes).
pub const MAX_MACRO_STEPS: usize = 200;

/// Flash erase sector size used for macro slots (`0x1000`).
pub const MACRO_SECTOR_SIZE: u32 = 0x1000;

/// Neutral 8-bit axis value (`0x7F`).
pub const AXIS_NEUTRAL: u8 = 0x7F;

/// One recorded macro step.
///
/// `keys` holds the low 16 bits of a [`KeyMap`] mask (Start..R2). Higher
/// buttons (Turbo, paddles, ...) cannot be recorded in a macro step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MacroStep {
    /// Duration of this step in milliseconds (1 ms resolution).
    pub ms_times: u16,
    /// Low 16 bits of the pressed-button mask.
    pub keys: u16,
    /// Right trigger R2/RT, `0x00..=0xFF`.
    pub rt_value: u8,
    /// Left trigger L2/LT, `0x00..=0xFF`.
    pub lt_value: u8,
    /// Left stick X, `0x00` = left, `0x7F` = neutral, `0xFF` = right.
    pub left_x: u8,
    /// Left stick Y, `0x00` = up, `0x7F` = neutral, `0xFF` = down.
    pub left_y: u8,
    /// Right stick X.
    pub right_x: u8,
    /// Right stick Y.
    pub right_y: u8,
}

impl MacroStep {
    /// A step with neutral sticks and no buttons or triggers.
    #[must_use]
    pub const fn neutral(ms_times: u16) -> Self {
        Self {
            ms_times,
            keys: 0,
            rt_value: 0,
            lt_value: 0,
            left_x: AXIS_NEUTRAL,
            left_y: AXIS_NEUTRAL,
            right_x: AXIS_NEUTRAL,
            right_y: AXIS_NEUTRAL,
        }
    }

    /// The pressed-button mask as a [`KeyMap`].
    #[must_use]
    pub const fn key_map(self) -> KeyMap {
        KeyMap::from_bits(self.keys as u32)
    }

    /// Decode a 10-byte step.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < MACRO_STEP_SIZE {
            return Err(Error::InvalidLength {
                expected: MACRO_STEP_SIZE,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            ms_times: u16::from_le_bytes([bytes[0], bytes[1]]),
            keys: u16::from_le_bytes([bytes[2], bytes[3]]),
            rt_value: bytes[4],
            lt_value: bytes[5],
            left_x: bytes[6],
            left_y: bytes[7],
            right_x: bytes[8],
            right_y: bytes[9],
        })
    }

    /// Encode this step into its 10-byte representation.
    #[must_use]
    pub fn to_bytes(self) -> [u8; MACRO_STEP_SIZE] {
        let trigger_value = u16::from_le_bytes([self.rt_value, self.lt_value]);
        let left_joy = u16::from_le_bytes([self.left_x, self.left_y]);
        let right_joy = u16::from_le_bytes([self.right_x, self.right_y]);
        let mut out = [0u8; MACRO_STEP_SIZE];
        out[0..2].copy_from_slice(&self.ms_times.to_le_bytes());
        out[2..4].copy_from_slice(&self.keys.to_le_bytes());
        out[4..6].copy_from_slice(&trigger_value.to_le_bytes());
        out[6..8].copy_from_slice(&left_joy.to_le_bytes());
        out[8..10].copy_from_slice(&right_joy.to_le_bytes());
        out
    }
}

/// Round `len` up to a multiple of 32 bytes.
///
/// This is the `total_len` used by `WriteBT2Macro` (`cmd 0x0103`).
#[must_use]
pub const fn align32(len: usize) -> usize {
    (len + 31) & !31
}

/// Flash offset for macro index `0..=3` (`macro_idx << 12`).
#[must_use]
pub const fn macro_sector_offset(macro_idx: u8) -> u32 {
    (macro_idx as u32) << 12
}

/// Encode steps into the 32-byte-aligned payload for `cmd 0x0103`.
///
/// The returned length is [`align32`]`(steps.len() * 10)` and the padding is
/// zero-filled, exactly as the official tool transmits it.
pub fn encode_macro_steps(steps: &[MacroStep]) -> Result<Vec<u8>, Error> {
    if steps.len() > MAX_MACRO_STEPS {
        return Err(Error::TooManySteps {
            max: MAX_MACRO_STEPS,
            actual: steps.len(),
        });
    }
    let mut out = alloc::vec![0u8; align32(steps.len() * MACRO_STEP_SIZE)];
    for (i, step) in steps.iter().enumerate() {
        out[i * MACRO_STEP_SIZE..(i + 1) * MACRO_STEP_SIZE].copy_from_slice(&step.to_bytes());
    }
    Ok(out)
}

/// Decode `max_steps` steps from a response payload.
///
/// `data` may be longer than needed (chunk padding); extra bytes are ignored.
pub fn decode_macro_steps(data: &[u8], max_steps: usize) -> Result<Vec<MacroStep>, Error> {
    if max_steps > MAX_MACRO_STEPS {
        return Err(Error::TooManySteps {
            max: MAX_MACRO_STEPS,
            actual: max_steps,
        });
    }
    let needed = max_steps * MACRO_STEP_SIZE;
    if data.len() < needed {
        return Err(Error::InvalidLength {
            expected: needed,
            actual: data.len(),
        });
    }
    let mut steps = Vec::with_capacity(max_steps);
    for chunk in data[..needed].chunks_exact(MACRO_STEP_SIZE) {
        steps.push(MacroStep::from_bytes(chunk)?);
    }
    Ok(steps)
}

/// Human-readable JSON/TOML representation of one macro.
#[cfg(feature = "serde")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MacroFile {
    /// Optional macro name (UTF-16BE on the controller, plain string here).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub name: Option<alloc::string::String>,
    /// Target `gamepad_mode`.
    ///
    /// **Ignored by the current CLI and core write path**: the macro command
    /// `subcmd` is derived from the device's current configuration mode, not
    /// from this file field. The CLI warns when this is non-zero.
    #[cfg_attr(feature = "serde", serde(default))]
    pub gamepad_mode: u16,
    /// Button that triggers the macro (`0` = unassigned).
    #[cfg_attr(feature = "serde", serde(default))]
    pub key_map: KeyMap,
    /// Number of loop cycles (`0` = hold / infinite).
    ///
    /// `None` means "not specified"; a plan built from this file preserves
    /// the existing header value.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub cycles_num: Option<u32>,
    /// Delay between loops in milliseconds.
    ///
    /// `None` means "not specified"; a plan built from this file preserves
    /// the existing header value.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub interval_ms: Option<u32>,
    /// Step list.
    #[cfg_attr(feature = "serde", serde(default))]
    pub steps: Vec<MacroStep>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_step() -> MacroStep {
        MacroStep {
            ms_times: 50,
            keys: 0x0200,
            rt_value: 0xFF,
            lt_value: 0x80,
            left_x: 0x7F,
            left_y: 0x00,
            right_x: 0xFF,
            right_y: 0x7F,
        }
    }

    #[test]
    fn step_layout_is_exact() {
        assert_eq!(
            sample_step().to_bytes(),
            [
                0x32, 0x00, // ms_times = 50
                0x00, 0x02, // keys = 0x0200 (D-Pad Up)
                0xFF, 0x80, // trigger_value = 0x80FF
                0x7F, 0x00, // left_joy = 0x007F (X=0x7F, Y=0x00)
                0xFF, 0x7F, // right_joy = 0x7FFF (X=0xFF, Y=0x7F)
            ]
        );
    }

    #[test]
    fn step_roundtrip() {
        let step = sample_step();
        assert_eq!(MacroStep::from_bytes(&step.to_bytes()), Ok(step));
    }

    #[test]
    fn neutral_step_uses_0x7f_axes() {
        let step = MacroStep::neutral(16);
        let bytes = step.to_bytes();
        assert_eq!(bytes[6..], [0x7F, 0x7F, 0x7F, 0x7F]);
        assert_eq!(step.key_map(), KeyMap::NONE);
        assert_eq!(step.ms_times, 16);
    }

    #[test]
    fn step_key_map_uses_low_16_bits() {
        let step = MacroStep {
            keys: 0x8000,
            ..MacroStep::neutral(1)
        };
        assert_eq!(step.key_map(), KeyMap::R2);
    }

    #[test]
    fn alignment_matches_official_formula() {
        assert_eq!(align32(0), 0);
        assert_eq!(align32(1), 32);
        assert_eq!(align32(10), 32);
        assert_eq!(align32(32), 32);
        assert_eq!(align32(40), 64);
        assert_eq!(align32(160), 160);
        assert_eq!(align32(2000), 2016);
    }

    #[test]
    fn encode_pads_to_32_bytes() {
        let steps = [MacroStep::neutral(10); 4];
        let encoded = encode_macro_steps(&steps).unwrap();
        assert_eq!(encoded.len(), 64);
        assert_eq!(&encoded[40..], &[0u8; 24]);
    }

    #[test]
    fn decode_ignores_trailing_padding() {
        let steps = [MacroStep::neutral(10); 4];
        let encoded = encode_macro_steps(&steps).unwrap();
        assert_eq!(decode_macro_steps(&encoded, 4).unwrap(), steps);
    }

    #[test]
    fn maximum_step_count() {
        let steps = alloc::vec![MacroStep::neutral(1); MAX_MACRO_STEPS];
        let encoded = encode_macro_steps(&steps).unwrap();
        assert_eq!(encoded.len(), align32(MAX_MACRO_STEPS * MACRO_STEP_SIZE));
        assert_eq!(encoded.len(), 2016);

        let too_many = alloc::vec![MacroStep::neutral(1); MAX_MACRO_STEPS + 1];
        assert!(matches!(
            encode_macro_steps(&too_many),
            Err(Error::TooManySteps { .. })
        ));
    }

    #[test]
    fn sector_offsets_are_4k_steps() {
        assert_eq!(macro_sector_offset(0), 0x0000);
        assert_eq!(macro_sector_offset(1), 0x1000);
        assert_eq!(macro_sector_offset(2), 0x2000);
        assert_eq!(macro_sector_offset(3), 0x3000);
    }
}
