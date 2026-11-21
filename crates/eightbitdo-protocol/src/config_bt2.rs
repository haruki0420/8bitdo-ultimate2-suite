//! `record_ultimatebt2`: the 2768-byte (`0x0AD0`) configuration block.
//!
//! [`Bt2Config`] owns the raw buffer. Every accessor reads or writes only its
//! documented byte range, so the reserved regions at `0x1F4..0x68B` and
//! `0x914..0x92B` survive a read-modify-write cycle untouched.
//!
//! Multi-byte integers are little-endian. Profile and macro **names are the
//! only exception**: they are UTF-16**BE**, 32 bytes (16 code units) each.

use alloc::string::String;
use core::fmt;

use crate::crc::ansi_crc16_sh;
use crate::error::Error;
use crate::keymap::{ButtonIndex, KeyMap};

/// Total configuration size in bytes (`0x0AD0`).
pub const CONFIG_SIZE: usize = 0x0AD0;

/// Number of profile slots.
pub const SLOT_COUNT: usize = 3;

/// Offset of `slot_flags[3]`.
pub const OFF_SLOT_FLAGS: usize = 0x000;
/// Offset of the stored `ANSI_CRC_16_sh` value.
pub const OFF_CRC16: usize = 0x00C;
/// Offset of `gamepad_mode`.
pub const OFF_GAMEPAD_MODE: usize = 0x010;
/// Offset of `cur_slot`.
pub const OFF_CUR_SLOT: usize = 0x012;
/// Offset of `file_names[3]`.
pub const OFF_PROFILE_NAMES: usize = 0x014;
/// Offset of `rumble[3]`.
pub const OFF_RUMBLE: usize = 0x074;
/// Offset of `joy[3]`.
pub const OFF_JOY: usize = 0x098;
/// Offset of `trigger[3]`.
pub const OFF_TRIGGER: usize = 0x0B0;
/// Offset of `special[3]`.
pub const OFF_SPECIAL: usize = 0x0C8;
/// Offset of `map_key[3]`.
pub const OFF_MAP_KEY: usize = 0x0E0;
/// Offset of the legacy / reserved 3 x 392-byte region.
pub const OFF_LEGACY_A: usize = 0x1F4;
/// Offset of `macros[3]`.
pub const OFF_MACROS: usize = 0x68C;
/// Offset of the legacy / reserved 3 x 8-byte region.
pub const OFF_LEGACY_B: usize = 0x914;
/// Offset of `six_axis[3]`.
pub const OFF_SIX_AXIS: usize = 0x92C;
/// Offset of `dynamic_theme[3]`.
pub const OFF_DYNAMIC: usize = 0x950;
/// Offset of `hotwheel_theme[3]`.
pub const OFF_HOTWHEEL: usize = 0x974;
/// Offset of `single_theme[3]`.
pub const OFF_SINGLE: usize = 0x9A4;

/// Size of one profile name field.
pub const PROFILE_NAME_SIZE: usize = 0x20;
/// Size of one rumble record.
pub const RUMBLE_SIZE: usize = 0x0C;
/// Size of one stick record.
pub const JOY_SIZE: usize = 0x08;
/// Size of one trigger record.
pub const TRIGGER_SIZE: usize = 0x08;
/// Size of one special-feature record.
pub const SPECIAL_SIZE: usize = 0x08;
/// Size of one remapping record.
pub const MAP_KEY_SIZE: usize = 0x5C;
/// Size of the legacy region per slot.
pub const LEGACY_A_SLOT_SIZE: usize = 0x188;
/// Size of the legacy region B per slot.
pub const LEGACY_B_SLOT_SIZE: usize = 0x08;
/// Size of one macro slot header.
pub const MACRO_SLOT_SIZE: usize = 0xD8;
/// Size of one macro record inside a slot header.
pub const MACRO_RECORD_SIZE: usize = 0x34;
/// Size of one six-axis record.
pub const SIX_AXIS_SIZE: usize = 0x0C;
/// Size of one dynamic theme record.
pub const DYNAMIC_SIZE: usize = 0x0C;
/// Size of one hot-wheel theme record.
pub const HOTWHEEL_SIZE: usize = 0x10;
/// Size of one single-color theme record.
pub const SINGLE_SIZE: usize = 0x64;

/// Number of remappable buttons.
pub const BUTTON_COUNT: usize = 22;
/// Number of macros per slot.
pub const MACROS_PER_SLOT: usize = 4;
/// Number of LEDs in the single-color ring.
pub const SINGLE_LED_COUNT: usize = 24;

/// One of the three configuration slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Slot {
    /// Profile 1 (`0`).
    S1 = 0,
    /// Profile 2 (`1`).
    S2 = 1,
    /// Profile 3 (`2`).
    S3 = 2,
}

impl Slot {
    /// All slots in wire order.
    pub const ALL: [Self; SLOT_COUNT] = [Self::S1, Self::S2, Self::S3];

    /// Zero-based index (`0..=2`).
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// One-based profile number (`1..=3`).
    #[must_use]
    pub const fn number(self) -> u8 {
        self as u8 + 1
    }

    /// Look up a slot by zero-based index.
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::S1),
            1 => Some(Self::S2),
            2 => Some(Self::S3),
            _ => None,
        }
    }
}

impl TryFrom<u8> for Slot {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_index(value as usize).ok_or(Error::InvalidSlot(value))
    }
}

/// One of the four macro slots inside a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum MacroIndex {
    /// Macro 1 (`0`, flash sector `0x0000`).
    M1 = 0,
    /// Macro 2 (`1`, flash sector `0x1000`).
    M2 = 1,
    /// Macro 3 (`2`, flash sector `0x2000`).
    M3 = 2,
    /// Macro 4 (`3`, flash sector `0x3000`).
    M4 = 3,
}

impl MacroIndex {
    /// All macro indices.
    pub const ALL: [Self; MACROS_PER_SLOT] = [Self::M1, Self::M2, Self::M3, Self::M4];

    /// Zero-based index (`0..=3`).
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Look up a macro index.
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::M1),
            1 => Some(Self::M2),
            2 => Some(Self::M3),
            3 => Some(Self::M4),
            _ => None,
        }
    }

    /// Flash offset of this macro's 4 KB sector (`index << 12`).
    #[must_use]
    pub const fn sector_offset(self) -> u32 {
        (self as u32) << 12
    }
}

impl TryFrom<u8> for MacroIndex {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_index(value as usize).ok_or(Error::InvalidMacroIndex(value))
    }
}

/// `GAMEPAD_MODE_SWIFT` values stored at `+0x010`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum GamepadMode {
    /// Nintendo Switch mode.
    Switch = 0,
    /// Standard gamepad J/M mode.
    GamepadJM = 1,
    /// Apple / macOS mode.
    Mac = 2,
    /// XInput mode.
    XInput = 3,
    /// Extended mode.
    Extended = 4,
    /// No mode selected.
    Unselected = 5,
}

impl TryFrom<u16> for GamepadMode {
    type Error = Error;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Switch),
            1 => Ok(Self::GamepadJM),
            2 => Ok(Self::Mac),
            3 => Ok(Self::XInput),
            4 => Ok(Self::Extended),
            5 => Ok(Self::Unselected),
            other => Err(Error::UnknownGamepadMode(other)),
        }
    }
}

/// `0xAARRGGBB` color stored as a little-endian `u32` (wire bytes are B,G,R,A).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Argb(pub u32);

impl Argb {
    /// Build a color from channels.
    #[must_use]
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    /// Alpha channel.
    #[must_use]
    pub const fn a(self) -> u8 {
        (self.0 >> 24) as u8
    }

    /// Red channel.
    #[must_use]
    pub const fn r(self) -> u8 {
        (self.0 >> 16) as u8
    }

    /// Green channel.
    #[must_use]
    pub const fn g(self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// Blue channel.
    #[must_use]
    pub const fn b(self) -> u8 {
        self.0 as u8
    }

    /// Wire bytes in memory order (B, G, R, A).
    #[must_use]
    pub const fn to_le_bytes(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    /// Decode wire bytes in memory order (B, G, R, A).
    #[must_use]
    pub const fn from_le_bytes(bytes: [u8; 4]) -> Self {
        Self(u32::from_le_bytes(bytes))
    }
}

/// Rumble record (`+0x074 + slot * 0x0C`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RumbleRecord {
    /// `1` when custom rumble is enabled.
    pub flag: u32,
    /// Left motor multiplier (`f32`).
    pub left: f32,
    /// Right motor multiplier (`f32`).
    pub right: f32,
}

impl RumbleRecord {
    /// The six UI levels and their exact `f32` values.
    pub const LEVELS: [f32; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];

    /// Build a record from a UI level (`0..=5`).
    #[must_use]
    pub fn from_level(level: u8) -> Option<Self> {
        let value = *Self::LEVELS.get(level as usize)?;
        Some(Self {
            flag: 1,
            left: value,
            right: value,
        })
    }

    /// Return the matching UI level when both motors share a known value.
    #[must_use]
    pub fn level(&self) -> Option<u8> {
        if self.left != self.right {
            return None;
        }
        Self::LEVELS
            .iter()
            .position(|&value| value == self.left)
            .map(|index| index as u8)
    }
}

impl Default for RumbleRecord {
    fn default() -> Self {
        Self {
            flag: 0,
            left: 1.0,
            right: 1.0,
        }
    }
}

/// Stick dead-zone record (`+0x098 + slot * 0x08`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JoyRecord {
    /// `1` when custom stick settings are enabled.
    pub flag: u32,
    /// Left stick inner dead zone (`0..=127`).
    pub left_start_value: u8,
    /// Left stick outer boundary (`0..=128`).
    pub left_end_value: u8,
    /// Right stick inner dead zone.
    pub right_start_value: u8,
    /// Right stick outer boundary.
    pub right_end_value: u8,
}

impl JoyRecord {
    /// Defaults used when `flag != 1`: no dead zone, full range.
    #[must_use]
    pub const fn defaults() -> Self {
        Self {
            flag: 0,
            left_start_value: 0,
            left_end_value: 128,
            right_start_value: 0,
            right_end_value: 128,
        }
    }
}

/// Trigger dead-zone record (`+0x0B0 + slot * 0x08`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TriggerRecord {
    /// `1` when custom trigger settings are enabled.
    pub flag: u32,
    /// Left trigger start value (`0..=255`; `77` for hair-trigger mode).
    pub left_start_value: u8,
    /// Left trigger end value.
    pub left_end_value: u8,
    /// Right trigger start value.
    pub right_start_value: u8,
    /// Right trigger end value.
    pub right_end_value: u8,
}

impl TriggerRecord {
    /// Defaults used when `flag != 1`: full range.
    #[must_use]
    pub const fn defaults() -> Self {
        Self {
            flag: 0,
            left_start_value: 0,
            left_end_value: 255,
            right_start_value: 0,
            right_end_value: 255,
        }
    }
}

/// Special-feature bitmask (`+0x0C8 + slot * 0x08`, `+0x04`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpecialFeature(pub u32);

impl SpecialFeature {
    /// Left stick X inversion.
    pub const LX_FLIP: Self = Self(0x0000_0001);
    /// Left stick Y inversion.
    pub const LY_FLIP: Self = Self(0x0000_0002);
    /// Right stick X inversion.
    pub const RX_FLIP: Self = Self(0x0000_0004);
    /// Right stick Y inversion.
    pub const RY_FLIP: Self = Self(0x0000_0008);
    /// Swap left and right sticks.
    pub const STICK_SWAP: Self = Self(0x0000_0010);
    /// Swap L2 and R2.
    pub const TRIGGER_SWAP: Self = Self(0x0000_0080);
    /// Swap left stick and D-Pad.
    pub const DPAD_SWAP: Self = Self(0x0000_0100);
    /// Right stick / trigger remap flag.
    pub const RS_TRIGGER_FLIP: Self = Self(0x0000_0400);
    /// Motion sensitivity flag.
    pub const MOTION_SENSITIVITY: Self = Self(0x0000_0800);
    /// Zero dead-zone mode.
    pub const ZERO_DEADZONE: Self = Self(0x0000_1000);
    /// 4-way D-Pad mode.
    pub const FOUR_WAY: Self = Self(0x0000_2000);
    /// Impulse trigger vibration.
    pub const TRIGGER_VIBRATION: Self = Self(0x0000_4000);
    /// Body vibration motors.
    pub const VIBRATION: Self = Self(0x0000_8000);
    /// `joyShare == 1`.
    pub const JOY_SHARE_1: Self = Self(0x0010_0000);
    /// `joyShare == 2`.
    pub const JOY_SHARE_2: Self = Self(0x0020_0000);

    /// Raw bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// True when every bit of `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl core::ops::BitOr for SpecialFeature {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Special-feature record (`+0x0C8 + slot * 0x08`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpecialRecord {
    /// `1` when the feature mask is active.
    pub flag: u32,
    /// Feature bitmask.
    pub features: SpecialFeature,
}

/// Remapping record (`+0x0E0 + slot * 0x5C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapKeyRecord {
    /// `1` when remapping is enabled.
    pub flag: u32,
    /// Mapping target for each of the 22 physical buttons.
    pub keys: [KeyMap; BUTTON_COUNT],
}

impl Default for MapKeyRecord {
    fn default() -> Self {
        Self {
            flag: 0,
            keys: [KeyMap::NONE; BUTTON_COUNT],
        }
    }
}

impl MapKeyRecord {
    /// A record that maps every physical button to itself.
    #[must_use]
    pub fn identity() -> Self {
        let mut keys = [KeyMap::NONE; BUTTON_COUNT];
        for (i, button) in ButtonIndex::ALL.iter().enumerate() {
            keys[i] = button.default_key();
        }
        Self { flag: 1, keys }
    }

    /// Mapping target for one physical button.
    #[must_use]
    pub fn get(&self, button: ButtonIndex) -> KeyMap {
        self.keys[button.index()]
    }

    /// Set the mapping target for one physical button.
    pub fn set(&mut self, button: ButtonIndex, key: KeyMap) {
        self.keys[button.index()] = key;
    }
}

/// One macro definition (`MacroFunPro3Record`, 52 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroRecord {
    /// UTF-16BE name bytes, zero padded.
    pub name: [u8; PROFILE_NAME_SIZE],
    /// Target controller mode.
    pub gamepad_mode: u8,
    /// Special flag, normally `0`.
    pub special_flag: u8,
    /// Actual number of steps (`0..=200`).
    pub max_steps: u16,
    /// Flash offset / app-managed index.
    pub offset: u16,
    /// Trigger button mask.
    pub key_map: KeyMap,
    /// Loop count (`0` = hold / infinite).
    pub cycles_num: u32,
    /// Delay between loops in milliseconds.
    pub interval_ms: u32,
}

impl Default for MacroRecord {
    fn default() -> Self {
        Self {
            name: [0u8; PROFILE_NAME_SIZE],
            gamepad_mode: 0,
            special_flag: 0,
            max_steps: 0,
            offset: 0,
            key_map: KeyMap::NONE,
            cycles_num: 0,
            interval_ms: 0,
        }
    }
}

impl MacroRecord {
    /// Decode the stored name.
    #[must_use]
    pub fn name(&self) -> String {
        decode_utf16be(&self.name)
    }

    /// Encode and store a name (max 16 UTF-16 code units).
    pub fn set_name(&mut self, name: &str) -> Result<(), Error> {
        encode_utf16be(name, &mut self.name)
    }
}

/// Macro header for one slot (`Macro_Pro3_Struct`, 216 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroSlot {
    /// `1` when macros are enabled.
    pub flag: u32,
    /// Number of registered macros (`0..=4`).
    pub total_cnt: u8,
    /// The four macro definitions.
    pub records: [MacroRecord; MACROS_PER_SLOT],
}

impl Default for MacroSlot {
    fn default() -> Self {
        Self {
            flag: 0,
            total_cnt: 0,
            records: [MacroRecord::default(); MACROS_PER_SLOT],
        }
    }
}

/// Six-axis / gyro record (`+0x92C + slot * 0x0C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SixAxisRecord {
    /// `1` when gyro is enabled.
    pub flag: u32,
    /// Button that activates gyro (`0` = always / unspecified).
    pub key_map: KeyMap,
    /// `0` = hold, `1` = toggle.
    pub trigger_mode: u8,
    /// Sensitivity level (`1..=15`, default `5`).
    pub sensitivity: u8,
    /// Dead-zone compensation (`0..=255`, default `40`).
    pub dead_compensate: u8,
    /// `0` = left stick, `1` = right stick, `2` = mouse.
    pub mapping_type: u8,
}

impl Default for SixAxisRecord {
    fn default() -> Self {
        Self {
            flag: 0,
            key_map: KeyMap::NONE,
            trigger_mode: 0,
            sensitivity: 5,
            dead_compensate: 40,
            mapping_type: 0,
        }
    }
}

/// Gyro trigger modes.
pub mod gyro_trigger {
    /// Active while the button is held.
    pub const HOLD: u8 = 0;
    /// Toggle on each press.
    pub const TOGGLE: u8 = 1;
}

/// Gyro output targets.
pub mod gyro_target {
    /// Left stick.
    pub const LEFT_STICK: u8 = 0;
    /// Right stick.
    pub const RIGHT_STICK: u8 = 1;
    /// Mouse emulation.
    pub const MOUSE: u8 = 2;
}

/// Dynamic / rainbow theme (`+0x950 + slot * 0x0C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicTheme {
    /// `1` when the theme is active.
    pub flag: u32,
    /// Foreground color.
    pub color: Argb,
    /// Background color.
    pub bg_color: Argb,
}

impl Default for DynamicTheme {
    fn default() -> Self {
        Self {
            flag: 0,
            color: Argb(0xFF26_0032),
            bg_color: Argb(0xFF08_4E50),
        }
    }
}

/// Hot-wheel theme (`+0x974 + slot * 0x10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotWheelTheme {
    /// `1` when the theme is active.
    pub flag: u32,
    /// Foreground color.
    pub color: Argb,
    /// Background color.
    pub bg_color: Argb,
    /// Effect speed (`0..=15`, default `7`).
    pub speed: u8,
}

impl Default for HotWheelTheme {
    fn default() -> Self {
        Self {
            flag: 0,
            color: Argb(0xFFCC_6200),
            bg_color: Argb(0xFFCC_0000),
            speed: 7,
        }
    }
}

/// Single-color theme with 24 individual LEDs (`+0x9A4 + slot * 0x64`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleTheme {
    /// `1` when the theme is active.
    pub flag: u32,
    /// Color of each LED around the ring.
    pub colors: [Argb; SINGLE_LED_COUNT],
}

impl Default for SingleTheme {
    fn default() -> Self {
        Self {
            flag: 0,
            colors: [Argb(0xFF7E_3200); SINGLE_LED_COUNT],
        }
    }
}

/// Raw offset of a slot's profile name.
#[must_use]
pub const fn profile_name_offset(slot: Slot) -> usize {
    OFF_PROFILE_NAMES + slot.index() * PROFILE_NAME_SIZE
}

/// Raw offset of a slot's rumble record.
#[must_use]
pub const fn rumble_offset(slot: Slot) -> usize {
    OFF_RUMBLE + slot.index() * RUMBLE_SIZE
}

/// Raw offset of a slot's stick record.
#[must_use]
pub const fn joy_offset(slot: Slot) -> usize {
    OFF_JOY + slot.index() * JOY_SIZE
}

/// Raw offset of a slot's trigger record.
#[must_use]
pub const fn trigger_offset(slot: Slot) -> usize {
    OFF_TRIGGER + slot.index() * TRIGGER_SIZE
}

/// Raw offset of a slot's special-feature record.
#[must_use]
pub const fn special_offset(slot: Slot) -> usize {
    OFF_SPECIAL + slot.index() * SPECIAL_SIZE
}

/// Raw offset of a slot's remapping record.
#[must_use]
pub const fn map_key_offset(slot: Slot) -> usize {
    OFF_MAP_KEY + slot.index() * MAP_KEY_SIZE
}

/// Raw offset of one remapping entry.
#[must_use]
pub const fn map_key_entry_offset(slot: Slot, button: ButtonIndex) -> usize {
    map_key_offset(slot) + 4 + button.index() * 4
}

/// Raw offset of a slot's macro header.
#[must_use]
pub const fn macro_slot_offset(slot: Slot) -> usize {
    OFF_MACROS + slot.index() * MACRO_SLOT_SIZE
}

/// Raw offset of one macro record inside a slot header.
#[must_use]
pub const fn macro_record_offset(slot: Slot, macro_idx: MacroIndex) -> usize {
    macro_slot_offset(slot) + 8 + macro_idx.index() * MACRO_RECORD_SIZE
}

/// Raw offset of a slot's six-axis record.
#[must_use]
pub const fn six_axis_offset(slot: Slot) -> usize {
    OFF_SIX_AXIS + slot.index() * SIX_AXIS_SIZE
}

/// Raw offset of a slot's dynamic theme.
#[must_use]
pub const fn dynamic_offset(slot: Slot) -> usize {
    OFF_DYNAMIC + slot.index() * DYNAMIC_SIZE
}

/// Raw offset of a slot's hot-wheel theme.
#[must_use]
pub const fn hotwheel_offset(slot: Slot) -> usize {
    OFF_HOTWHEEL + slot.index() * HOTWHEEL_SIZE
}

/// Raw offset of a slot's single-color theme.
#[must_use]
pub const fn single_offset(slot: Slot) -> usize {
    OFF_SINGLE + slot.index() * SINGLE_SIZE
}

/// The complete 2768-byte configuration block.
///
/// Setters never touch bytes outside their own field. Call
/// [`recompute_crc`](Self::recompute_crc) after edits, before sending the
/// buffer with `cmd 0x0001`.
#[derive(Clone, PartialEq, Eq)]
pub struct Bt2Config {
    raw: [u8; CONFIG_SIZE],
}

impl Default for Bt2Config {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl fmt::Debug for Bt2Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bt2Config")
            .field("slot_flags", &self.slot_flags())
            .field("crc16", &self.crc16_stored())
            .field("gamepad_mode", &self.gamepad_mode_raw())
            .field("cur_slot", &self.cur_slot_raw())
            .finish_non_exhaustive()
    }
}

impl Bt2Config {
    /// A fully zeroed buffer.
    #[must_use]
    pub const fn zeroed() -> Self {
        Self {
            raw: [0u8; CONFIG_SIZE],
        }
    }

    /// Copy an exact 2768-byte buffer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != CONFIG_SIZE {
            return Err(Error::InvalidLength {
                expected: CONFIG_SIZE,
                actual: bytes.len(),
            });
        }
        let mut raw = [0u8; CONFIG_SIZE];
        raw.copy_from_slice(bytes);
        Ok(Self { raw })
    }

    /// Borrow the raw buffer.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CONFIG_SIZE] {
        &self.raw
    }

    /// Mutably borrow the raw buffer.
    ///
    /// Prefer the typed setters; this exists for advanced partial writes.
    #[must_use]
    pub fn as_mut_bytes(&mut self) -> &mut [u8; CONFIG_SIZE] {
        &mut self.raw
    }

    /// Consume the value and return the raw buffer.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; CONFIG_SIZE] {
        self.raw
    }

    // --- header ---------------------------------------------------------

    /// All three slot flags.
    #[must_use]
    pub fn slot_flags(&self) -> [u32; SLOT_COUNT] {
        [
            self.u32_at(OFF_SLOT_FLAGS),
            self.u32_at(OFF_SLOT_FLAGS + 4),
            self.u32_at(OFF_SLOT_FLAGS + 8),
        ]
    }

    /// Flag of one slot (`1` = valid, `0` = empty).
    #[must_use]
    pub fn slot_flag(&self, slot: Slot) -> u32 {
        self.u32_at(OFF_SLOT_FLAGS + slot.index() * 4)
    }

    /// Set a slot flag.
    pub fn set_slot_flag(&mut self, slot: Slot, value: u32) {
        self.set_u32(OFF_SLOT_FLAGS + slot.index() * 4, value);
    }

    /// The stored CRC field (`+0x00C`), as read.
    #[must_use]
    pub fn crc16_stored(&self) -> u32 {
        self.u32_at(OFF_CRC16)
    }

    /// Recompute `ANSI_CRC_16_sh` over all 2768 bytes and store it at `+0x00C`.
    ///
    /// The official implementation does **not** zero the CRC field first, so
    /// the previous value is part of the input. This method reproduces that.
    pub fn recompute_crc(&mut self) -> u16 {
        let crc = ansi_crc16_sh(&self.raw);
        self.set_u32(OFF_CRC16, u32::from(crc));
        crc
    }

    /// Raw `gamepad_mode` value.
    #[must_use]
    pub fn gamepad_mode_raw(&self) -> u16 {
        self.u16_at(OFF_GAMEPAD_MODE)
    }

    /// Parsed `gamepad_mode`, or `None` for an unknown value.
    #[must_use]
    pub fn gamepad_mode(&self) -> Option<GamepadMode> {
        GamepadMode::try_from(self.gamepad_mode_raw()).ok()
    }

    /// Set a known `gamepad_mode`.
    pub fn set_gamepad_mode(&mut self, mode: GamepadMode) {
        self.set_u16(OFF_GAMEPAD_MODE, mode as u16);
    }

    /// Set a raw `gamepad_mode` (for preserving unknown values).
    pub fn set_gamepad_mode_raw(&mut self, mode: u16) {
        self.set_u16(OFF_GAMEPAD_MODE, mode);
    }

    /// Raw `cur_slot` value (`4` = none).
    #[must_use]
    pub fn cur_slot_raw(&self) -> u16 {
        self.u16_at(OFF_CUR_SLOT)
    }

    /// Active slot, or `None` when unset.
    #[must_use]
    pub fn cur_slot(&self) -> Option<Slot> {
        Slot::from_index(self.cur_slot_raw() as usize)
    }

    /// Set the active slot.
    pub fn set_cur_slot(&mut self, slot: Option<Slot>) {
        self.set_cur_slot_raw(slot.map_or(4, |slot| slot.index() as u16));
    }

    /// Set a raw `cur_slot` value.
    pub fn set_cur_slot_raw(&mut self, value: u16) {
        self.set_u16(OFF_CUR_SLOT, value);
    }

    // --- profile names --------------------------------------------------

    /// Raw 32-byte name field of a slot.
    #[must_use]
    pub fn profile_name_raw(&self, slot: Slot) -> &[u8] {
        let offset = profile_name_offset(slot);
        &self.raw[offset..offset + PROFILE_NAME_SIZE]
    }

    /// Decoded profile name (UTF-16BE, stop at the first NUL unit).
    #[must_use]
    pub fn profile_name(&self, slot: Slot) -> String {
        decode_utf16be(self.profile_name_raw(slot))
    }

    /// Encode a profile name (max 16 UTF-16 code units).
    pub fn set_profile_name(&mut self, slot: Slot, name: &str) -> Result<(), Error> {
        let offset = profile_name_offset(slot);
        encode_utf16be(name, &mut self.raw[offset..offset + PROFILE_NAME_SIZE])
    }

    // --- rumble / sticks / triggers / features --------------------------

    /// Rumble record of a slot.
    #[must_use]
    pub fn rumble(&self, slot: Slot) -> RumbleRecord {
        let offset = rumble_offset(slot);
        RumbleRecord {
            flag: self.u32_at(offset),
            left: f32::from_le_bytes(self.array_at(offset + 4)),
            right: f32::from_le_bytes(self.array_at(offset + 8)),
        }
    }

    /// Write a rumble record.
    pub fn set_rumble(&mut self, slot: Slot, record: &RumbleRecord) {
        let offset = rumble_offset(slot);
        self.set_u32(offset, record.flag);
        self.set_bytes(offset + 4, &record.left.to_le_bytes());
        self.set_bytes(offset + 8, &record.right.to_le_bytes());
    }

    /// Stick record of a slot.
    #[must_use]
    pub fn joy(&self, slot: Slot) -> JoyRecord {
        let offset = joy_offset(slot);
        JoyRecord {
            flag: self.u32_at(offset),
            left_start_value: self.raw[offset + 4],
            left_end_value: self.raw[offset + 5],
            right_start_value: self.raw[offset + 6],
            right_end_value: self.raw[offset + 7],
        }
    }

    /// Write a stick record.
    pub fn set_joy(&mut self, slot: Slot, record: &JoyRecord) {
        let offset = joy_offset(slot);
        self.set_u32(offset, record.flag);
        self.raw[offset + 4] = record.left_start_value;
        self.raw[offset + 5] = record.left_end_value;
        self.raw[offset + 6] = record.right_start_value;
        self.raw[offset + 7] = record.right_end_value;
    }

    /// Trigger record of a slot.
    #[must_use]
    pub fn trigger(&self, slot: Slot) -> TriggerRecord {
        let offset = trigger_offset(slot);
        TriggerRecord {
            flag: self.u32_at(offset),
            left_start_value: self.raw[offset + 4],
            left_end_value: self.raw[offset + 5],
            right_start_value: self.raw[offset + 6],
            right_end_value: self.raw[offset + 7],
        }
    }

    /// Write a trigger record.
    pub fn set_trigger(&mut self, slot: Slot, record: &TriggerRecord) {
        let offset = trigger_offset(slot);
        self.set_u32(offset, record.flag);
        self.raw[offset + 4] = record.left_start_value;
        self.raw[offset + 5] = record.left_end_value;
        self.raw[offset + 6] = record.right_start_value;
        self.raw[offset + 7] = record.right_end_value;
    }

    /// Special-feature record of a slot.
    #[must_use]
    pub fn special(&self, slot: Slot) -> SpecialRecord {
        let offset = special_offset(slot);
        SpecialRecord {
            flag: self.u32_at(offset),
            features: SpecialFeature(self.u32_at(offset + 4)),
        }
    }

    /// Write a special-feature record.
    pub fn set_special(&mut self, slot: Slot, record: &SpecialRecord) {
        let offset = special_offset(slot);
        self.set_u32(offset, record.flag);
        self.set_u32(offset + 4, record.features.bits());
    }

    // --- remapping ------------------------------------------------------

    /// Remapping record of a slot.
    #[must_use]
    pub fn map_key(&self, slot: Slot) -> MapKeyRecord {
        let offset = map_key_offset(slot);
        let mut keys = [KeyMap::NONE; BUTTON_COUNT];
        for (i, key) in keys.iter_mut().enumerate() {
            *key = KeyMap::from_bits(self.u32_at(offset + 4 + i * 4));
        }
        MapKeyRecord {
            flag: self.u32_at(offset),
            keys,
        }
    }

    /// Write a remapping record.
    pub fn set_map_key(&mut self, slot: Slot, record: &MapKeyRecord) {
        let offset = map_key_offset(slot);
        self.set_u32(offset, record.flag);
        for (i, key) in record.keys.iter().enumerate() {
            self.set_u32(offset + 4 + i * 4, key.bits());
        }
    }

    /// Read the mapping target of one physical button.
    #[must_use]
    pub fn map_key_entry(&self, slot: Slot, button: ButtonIndex) -> KeyMap {
        KeyMap::from_bits(self.u32_at(map_key_entry_offset(slot, button)))
    }

    /// Write only one 4-byte remapping entry (official partial-write unit).
    pub fn set_map_key_entry(&mut self, slot: Slot, button: ButtonIndex, key: KeyMap) {
        self.set_u32(map_key_entry_offset(slot, button), key.bits());
    }

    // --- macros ---------------------------------------------------------

    /// Macro header of a slot.
    #[must_use]
    pub fn macro_slot(&self, slot: Slot) -> MacroSlot {
        let offset = macro_slot_offset(slot);
        let mut records = [MacroRecord::default(); MACROS_PER_SLOT];
        for (i, record) in records.iter_mut().enumerate() {
            *record = self.macro_record_at(offset + 8 + i * MACRO_RECORD_SIZE);
        }
        MacroSlot {
            flag: self.u32_at(offset),
            total_cnt: self.raw[offset + 4],
            records,
        }
    }

    /// One macro record.
    #[must_use]
    pub fn macro_record(&self, slot: Slot, macro_idx: MacroIndex) -> MacroRecord {
        self.macro_record_at(macro_record_offset(slot, macro_idx))
    }

    /// Write one macro record.
    pub fn set_macro_record(&mut self, slot: Slot, macro_idx: MacroIndex, record: &MacroRecord) {
        let offset = macro_record_offset(slot, macro_idx);
        self.set_bytes(offset, &record.name);
        self.raw[offset + 0x20] = record.gamepad_mode;
        self.raw[offset + 0x21] = record.special_flag;
        self.set_u16(offset + 0x22, record.max_steps);
        self.set_u16(offset + 0x24, record.offset);
        self.set_u16(offset + 0x26, 0);
        self.set_u32(offset + 0x28, record.key_map.bits());
        self.set_u32(offset + 0x2C, record.cycles_num);
        self.set_u32(offset + 0x30, record.interval_ms);
    }

    /// Write a whole macro header (flag, count and all four records).
    pub fn set_macro_slot(&mut self, slot: Slot, header: &MacroSlot) {
        let offset = macro_slot_offset(slot);
        self.set_u32(offset, header.flag);
        self.raw[offset + 4] = header.total_cnt;
        self.raw[offset + 5..offset + 8].fill(0);
        for (i, record) in header.records.iter().enumerate() {
            let record_offset = offset + 8 + i * MACRO_RECORD_SIZE;
            self.set_bytes(record_offset, &record.name);
            self.raw[record_offset + 0x20] = record.gamepad_mode;
            self.raw[record_offset + 0x21] = record.special_flag;
            self.set_u16(record_offset + 0x22, record.max_steps);
            self.set_u16(record_offset + 0x24, record.offset);
            self.set_u16(record_offset + 0x26, 0);
            self.set_u32(record_offset + 0x28, record.key_map.bits());
            self.set_u32(record_offset + 0x2C, record.cycles_num);
            self.set_u32(record_offset + 0x30, record.interval_ms);
        }
    }

    // --- gyro / RGB -----------------------------------------------------

    /// Six-axis record of a slot.
    #[must_use]
    pub fn six_axis(&self, slot: Slot) -> SixAxisRecord {
        let offset = six_axis_offset(slot);
        SixAxisRecord {
            flag: self.u32_at(offset),
            key_map: KeyMap::from_bits(self.u32_at(offset + 4)),
            trigger_mode: self.raw[offset + 8],
            sensitivity: self.raw[offset + 9],
            dead_compensate: self.raw[offset + 10],
            mapping_type: self.raw[offset + 11],
        }
    }

    /// Write a six-axis record.
    ///
    /// This uses the Windows DLL offset (`0x92C + slot * 0x0C`), not the
    /// Android `libadvancelib.so` `offset = 0` bug.
    pub fn set_six_axis(&mut self, slot: Slot, record: &SixAxisRecord) {
        let offset = six_axis_offset(slot);
        self.set_u32(offset, record.flag);
        self.set_u32(offset + 4, record.key_map.bits());
        self.raw[offset + 8] = record.trigger_mode;
        self.raw[offset + 9] = record.sensitivity;
        self.raw[offset + 10] = record.dead_compensate;
        self.raw[offset + 11] = record.mapping_type;
    }

    /// Dynamic theme of a slot.
    #[must_use]
    pub fn dynamic_theme(&self, slot: Slot) -> DynamicTheme {
        let offset = dynamic_offset(slot);
        DynamicTheme {
            flag: self.u32_at(offset),
            color: Argb(self.u32_at(offset + 4)),
            bg_color: Argb(self.u32_at(offset + 8)),
        }
    }

    /// Write a dynamic theme.
    pub fn set_dynamic_theme(&mut self, slot: Slot, theme: &DynamicTheme) {
        let offset = dynamic_offset(slot);
        self.set_u32(offset, theme.flag);
        self.set_u32(offset + 4, theme.color.0);
        self.set_u32(offset + 8, theme.bg_color.0);
    }

    /// Hot-wheel theme of a slot.
    #[must_use]
    pub fn hotwheel_theme(&self, slot: Slot) -> HotWheelTheme {
        let offset = hotwheel_offset(slot);
        HotWheelTheme {
            flag: self.u32_at(offset),
            color: Argb(self.u32_at(offset + 4)),
            bg_color: Argb(self.u32_at(offset + 8)),
            speed: self.raw[offset + 12],
        }
    }

    /// Write a hot-wheel theme.
    pub fn set_hotwheel_theme(&mut self, slot: Slot, theme: &HotWheelTheme) {
        let offset = hotwheel_offset(slot);
        self.set_u32(offset, theme.flag);
        self.set_u32(offset + 4, theme.color.0);
        self.set_u32(offset + 8, theme.bg_color.0);
        self.raw[offset + 12] = theme.speed;
        self.raw[offset + 13..offset + 16].fill(0);
    }

    /// Single-color theme of a slot.
    #[must_use]
    pub fn single_theme(&self, slot: Slot) -> SingleTheme {
        let offset = single_offset(slot);
        let mut colors = [Argb(0); SINGLE_LED_COUNT];
        for (i, color) in colors.iter_mut().enumerate() {
            *color = Argb(self.u32_at(offset + 4 + i * 4));
        }
        SingleTheme {
            flag: self.u32_at(offset),
            colors,
        }
    }

    /// Write a single-color theme.
    pub fn set_single_theme(&mut self, slot: Slot, theme: &SingleTheme) {
        let offset = single_offset(slot);
        self.set_u32(offset, theme.flag);
        for (i, color) in theme.colors.iter().enumerate() {
            self.set_u32(offset + 4 + i * 4, color.0);
        }
    }

    // --- slot operations ------------------------------------------------

    /// Copy every per-slot section from `src` to `dst`.
    ///
    /// This mirrors the official `copyConfig`: it also copies the legacy
    /// reserved regions and rewrites the fifth character of the copied name to
    /// the destination profile number, as the original tool does.
    pub fn copy_slot(&mut self, dst: Slot, src: Slot) {
        if dst == src {
            return;
        }
        let mut copy = |offset: usize, len: usize| {
            let from = offset + src.index() * len;
            let to = offset + dst.index() * len;
            self.raw.copy_within(from..from + len, to);
        };
        copy(OFF_PROFILE_NAMES, PROFILE_NAME_SIZE);
        copy(OFF_RUMBLE, RUMBLE_SIZE);
        copy(OFF_JOY, JOY_SIZE);
        copy(OFF_TRIGGER, TRIGGER_SIZE);
        copy(OFF_SPECIAL, SPECIAL_SIZE);
        copy(OFF_MAP_KEY, MAP_KEY_SIZE);
        copy(OFF_LEGACY_A, LEGACY_A_SLOT_SIZE);
        copy(OFF_LEGACY_B, LEGACY_B_SLOT_SIZE);
        copy(OFF_MACROS, MACRO_SLOT_SIZE);
        copy(OFF_SIX_AXIS, SIX_AXIS_SIZE);
        copy(OFF_DYNAMIC, DYNAMIC_SIZE);
        copy(OFF_HOTWHEEL, HOTWHEEL_SIZE);
        copy(OFF_SINGLE, SINGLE_SIZE);

        self.set_slot_flag(dst, 1);
        // Official copyConfig writes `buf[dst * 0x20 + 0x1d] = dst + '1'`.
        self.raw[profile_name_offset(dst) + 9] = b'1' + dst.index() as u8;
    }

    /// Clear one slot, matching the official `deleteCurslot` behaviour.
    pub fn delete_slot(&mut self, slot: Slot) {
        self.set_slot_flag(slot, 0);

        let name = profile_name_offset(slot);
        self.raw[name..name + PROFILE_NAME_SIZE].fill(0);

        for offset in [
            rumble_offset(slot),
            joy_offset(slot),
            trigger_offset(slot),
            special_offset(slot),
            map_key_offset(slot),
            macro_slot_offset(slot),
            six_axis_offset(slot),
            dynamic_offset(slot),
            hotwheel_offset(slot),
            single_offset(slot),
        ] {
            self.set_u32(offset, 0);
        }

        // Legacy regions are only ever cleared in their first u32.
        self.set_u32(OFF_LEGACY_A + slot.index() * LEGACY_A_SLOT_SIZE, 0);
        self.set_u32(OFF_LEGACY_B + slot.index() * LEGACY_B_SLOT_SIZE, 0);

        if self.cur_slot() == Some(slot) {
            self.set_cur_slot(None);
        }
    }

    // --- raw helpers ----------------------------------------------------

    fn macro_record_at(&self, offset: usize) -> MacroRecord {
        let mut name = [0u8; PROFILE_NAME_SIZE];
        name.copy_from_slice(&self.raw[offset..offset + PROFILE_NAME_SIZE]);
        MacroRecord {
            name,
            gamepad_mode: self.raw[offset + 0x20],
            special_flag: self.raw[offset + 0x21],
            max_steps: self.u16_at(offset + 0x22),
            offset: self.u16_at(offset + 0x24),
            key_map: KeyMap::from_bits(self.u32_at(offset + 0x28)),
            cycles_num: self.u32_at(offset + 0x2C),
            interval_ms: self.u32_at(offset + 0x30),
        }
    }

    fn u16_at(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.raw[offset], self.raw[offset + 1]])
    }

    fn u32_at(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.array_at(offset))
    }

    fn array_at<const N: usize>(&self, offset: usize) -> [u8; N] {
        let mut out = [0u8; N];
        out.copy_from_slice(&self.raw[offset..offset + N]);
        out
    }

    fn set_u16(&mut self, offset: usize, value: u16) {
        self.raw[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn set_u32(&mut self, offset: usize, value: u32) {
        self.raw[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn set_bytes(&mut self, offset: usize, bytes: &[u8]) {
        self.raw[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
}

/// Encode `name` as UTF-16BE into a fixed-size field, zero padded.
fn encode_utf16be(name: &str, dst: &mut [u8]) -> Result<(), Error> {
    let max_units = dst.len() / 2;
    let mut units = 0;
    for unit in name.encode_utf16() {
        if unit == 0 {
            return Err(Error::EmbeddedNul);
        }
        if units == max_units {
            return Err(Error::StringTooLong {
                max_utf16_units: max_units,
                actual: name.encode_utf16().count(),
            });
        }
        dst[units * 2..units * 2 + 2].copy_from_slice(&unit.to_be_bytes());
        units += 1;
    }
    dst[units * 2..].fill(0);
    Ok(())
}

/// Decode a zero-terminated UTF-16BE field.
fn decode_utf16be(src: &[u8]) -> String {
    let mut units = alloc::vec::Vec::with_capacity(src.len() / 2);
    for chunk in src.chunks_exact(2) {
        let unit = u16::from_be_bytes([chunk[0], chunk[1]]);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Bt2Config {
        let mut config = Bt2Config::zeroed();
        for slot in Slot::ALL {
            config.set_slot_flag(slot, 1);
            config.set_profile_name(slot, "Slot1").unwrap();
        }
        config.set_gamepad_mode(GamepadMode::Switch);
        config.set_cur_slot(Some(Slot::S1));
        config
    }

    #[test]
    fn size_and_offsets_are_consistent() {
        assert_eq!(CONFIG_SIZE, 0x0AD0);
        assert_eq!(OFF_PROFILE_NAMES + 3 * PROFILE_NAME_SIZE, OFF_RUMBLE);
        assert_eq!(OFF_RUMBLE + 3 * RUMBLE_SIZE, OFF_JOY);
        assert_eq!(OFF_JOY + 3 * JOY_SIZE, OFF_TRIGGER);
        assert_eq!(OFF_TRIGGER + 3 * TRIGGER_SIZE, OFF_SPECIAL);
        assert_eq!(OFF_SPECIAL + 3 * SPECIAL_SIZE, OFF_MAP_KEY);
        assert_eq!(OFF_MAP_KEY + 3 * MAP_KEY_SIZE, OFF_LEGACY_A);
        assert_eq!(OFF_LEGACY_A + 3 * LEGACY_A_SLOT_SIZE, OFF_MACROS);
        assert_eq!(OFF_MACROS + 3 * MACRO_SLOT_SIZE, OFF_LEGACY_B);
        assert_eq!(OFF_LEGACY_B + 3 * LEGACY_B_SLOT_SIZE, OFF_SIX_AXIS);
        assert_eq!(OFF_SIX_AXIS + 3 * SIX_AXIS_SIZE, OFF_DYNAMIC);
        assert_eq!(OFF_DYNAMIC + 3 * DYNAMIC_SIZE, OFF_HOTWHEEL);
        assert_eq!(OFF_HOTWHEEL + 3 * HOTWHEEL_SIZE, OFF_SINGLE);
        assert_eq!(OFF_SINGLE + 3 * SINGLE_SIZE, CONFIG_SIZE);
    }

    #[test]
    fn header_roundtrip() {
        let config = sample();
        assert_eq!(config.slot_flags(), [1, 1, 1]);
        assert_eq!(config.gamepad_mode(), Some(GamepadMode::Switch));
        assert_eq!(config.cur_slot(), Some(Slot::S1));
        assert_eq!(config.slot_flag(Slot::S2), 1);
    }

    #[test]
    fn profile_names_are_utf16be() {
        let mut config = Bt2Config::zeroed();
        config.set_profile_name(Slot::S1, "Slot1").unwrap();
        assert_eq!(
            &config.profile_name_raw(Slot::S1)[..10],
            &[0x00, 0x53, 0x00, 0x6C, 0x00, 0x6F, 0x00, 0x74, 0x00, 0x31]
        );
        assert!(config.profile_name_raw(Slot::S1)[10..]
            .iter()
            .all(|&b| b == 0));
        assert_eq!(config.profile_name(Slot::S1), "Slot1");
    }

    #[test]
    fn japanese_names_roundtrip() {
        let mut config = Bt2Config::zeroed();
        config.set_profile_name(Slot::S2, "テスト").unwrap();
        assert_eq!(config.profile_name(Slot::S2), "テスト");
        assert_eq!(
            &config.profile_name_raw(Slot::S2)[..6],
            &[0x30, 0xC6, 0x30, 0xB9, 0x30, 0xC8]
        );
    }

    #[test]
    fn name_length_limits() {
        let mut config = Bt2Config::zeroed();
        let sixteen = "1234567890123456";
        config.set_profile_name(Slot::S1, sixteen).unwrap();
        assert_eq!(config.profile_name(Slot::S1), sixteen);

        let seventeen = "12345678901234567";
        assert!(matches!(
            config.set_profile_name(Slot::S1, seventeen),
            Err(Error::StringTooLong { .. })
        ));

        // A name with a NUL would be truncated on the wire.
        assert_eq!(
            config.set_profile_name(Slot::S1, "ab\0cd"),
            Err(Error::EmbeddedNul)
        );
    }

    #[test]
    fn rumble_uses_ieee754_little_endian() {
        let mut config = Bt2Config::zeroed();
        let record = RumbleRecord::from_level(1).unwrap();
        config.set_rumble(Slot::S1, &record);
        assert_eq!(config.rumble(Slot::S1), record);
        let bytes = &config.as_bytes()[rumble_offset(Slot::S1) + 4..rumble_offset(Slot::S1) + 8];
        assert_eq!(bytes, &0.2f32.to_le_bytes()[..]);
        assert_eq!(bytes, &[0xCD, 0xCC, 0x4C, 0x3E][..]);
        assert_eq!(config.rumble(Slot::S1).level(), Some(1));
    }

    #[test]
    fn default_records_match_reset_file() {
        assert_eq!(JoyRecord::defaults().left_end_value, 128);
        assert_eq!(TriggerRecord::defaults().right_end_value, 255);
        assert_eq!(SixAxisRecord::default().sensitivity, 5);
        assert_eq!(SixAxisRecord::default().dead_compensate, 40);
        assert_eq!(HotWheelTheme::default().speed, 7);
        assert_eq!(SingleTheme::default().colors.len(), 24);
    }

    #[test]
    fn special_features_roundtrip() {
        let mut config = Bt2Config::zeroed();
        let features = SpecialFeature::VIBRATION | SpecialFeature::LX_FLIP;
        config.set_special(Slot::S3, &SpecialRecord { flag: 1, features });
        let read = config.special(Slot::S3);
        assert_eq!(read.flag, 1);
        assert!(read.features.contains(SpecialFeature::VIBRATION));
        assert!(read.features.contains(SpecialFeature::LX_FLIP));
        assert_eq!(read.features.bits(), 0x8001);
    }

    #[test]
    fn remapping_index_order_is_preserved() {
        let mut config = Bt2Config::zeroed();
        let mut record = MapKeyRecord::identity();
        record.set(ButtonIndex::A, KeyMap::B);
        record.set(ButtonIndex::P1, KeyMap::M1);
        config.set_map_key(Slot::S2, &record);

        assert_eq!(config.map_key_entry(Slot::S2, ButtonIndex::A), KeyMap::B);
        assert_eq!(config.map_key_entry(Slot::S2, ButtonIndex::B), KeyMap::B);
        assert_eq!(config.map_key_entry(Slot::S2, ButtonIndex::P1), KeyMap::M1);
        assert_eq!(config.map_key(Slot::S2), record);

        config.set_map_key_entry(Slot::S2, ButtonIndex::Up, KeyMap::NONE);
        assert_eq!(
            config.map_key_entry(Slot::S2, ButtonIndex::Up),
            KeyMap::NONE
        );
    }

    #[test]
    fn macro_record_offsets_match_spec() {
        let mut config = Bt2Config::zeroed();
        let mut record = MacroRecord {
            max_steps: 42,
            key_map: KeyMap::P1,
            cycles_num: 3,
            interval_ms: 500,
            ..MacroRecord::default()
        };
        record.set_name("Combo").unwrap();
        config.set_macro_record(Slot::S2, MacroIndex::M3, &record);

        let base = 0x694 + 0xD8 + 2 * 0x34;
        assert_eq!(&config.as_bytes()[base..base + 10], &record.name[..10]);
        assert_eq!(
            config.as_bytes()[base + 0x22..base + 0x24],
            42u16.to_le_bytes()
        );
        assert_eq!(
            config.as_bytes()[base + 0x28..base + 0x2C],
            KeyMap::P1.bits().to_le_bytes()
        );
        assert_eq!(config.macro_record(Slot::S2, MacroIndex::M3), record);
    }

    #[test]
    fn macro_slot_roundtrip() {
        let mut config = Bt2Config::zeroed();
        let mut header = MacroSlot {
            flag: 1,
            total_cnt: 2,
            ..MacroSlot::default()
        };
        header.records[0].max_steps = 10;
        header.records[1].max_steps = 20;
        header.records[1].set_name("Second").unwrap();
        config.set_macro_slot(Slot::S3, &header);

        let read = config.macro_slot(Slot::S3);
        assert_eq!(read.flag, 1);
        assert_eq!(read.total_cnt, 2);
        assert_eq!(read.records[0].max_steps, 10);
        assert_eq!(read.records[1].name(), "Second");
        assert_eq!(read, header);
    }

    #[test]
    fn six_axis_uses_windows_offsets() {
        let mut config = Bt2Config::zeroed();
        let record = SixAxisRecord {
            flag: 1,
            key_map: KeyMap::L2,
            trigger_mode: gyro_trigger::TOGGLE,
            sensitivity: 9,
            dead_compensate: 12,
            mapping_type: gyro_target::MOUSE,
        };
        config.set_six_axis(Slot::S3, &record);
        assert_eq!(config.six_axis(Slot::S3), record);
        let offset = 0x92C + 2 * 0x0C;
        assert_eq!(config.as_bytes()[offset], 1);
        assert_eq!(config.as_bytes()[offset + 9], 9);
    }

    #[test]
    fn argb_memory_order_is_bgra() {
        let mut config = Bt2Config::zeroed();
        let theme = DynamicTheme {
            flag: 1,
            color: Argb::from_rgba(0x26, 0x00, 0x32, 0xFF),
            bg_color: Argb::from_rgba(0x08, 0x4E, 0x50, 0xFF),
        };
        config.set_dynamic_theme(Slot::S1, &theme);
        let offset = dynamic_offset(Slot::S1);
        assert_eq!(
            &config.as_bytes()[offset + 4..offset + 8],
            &[0x32, 0x00, 0x26, 0xFF]
        );
        assert_eq!(config.dynamic_theme(Slot::S1), theme);
        assert_eq!(theme.color.a(), 0xFF);
        assert_eq!(theme.color.r(), 0x26);
        assert_eq!(theme.color.b(), 0x32);
    }

    #[test]
    fn hotwheel_and_single_themes_roundtrip() {
        let mut config = Bt2Config::zeroed();
        let hotwheel = HotWheelTheme {
            flag: 1,
            color: Argb(0xFFCC_6200),
            bg_color: Argb(0xFFCC_0000),
            speed: 15,
        };
        config.set_hotwheel_theme(Slot::S2, &hotwheel);
        assert_eq!(config.hotwheel_theme(Slot::S2), hotwheel);

        let mut single = SingleTheme {
            flag: 1,
            ..SingleTheme::default()
        };
        single.colors[23] = Argb(0xFF00_FF00);
        config.set_single_theme(Slot::S2, &single);
        assert_eq!(config.single_theme(Slot::S2), single);
        let offset = single_offset(Slot::S2) + 4 + 23 * 4;
        assert_eq!(
            &config.as_bytes()[offset..offset + 4],
            &[0x00, 0xFF, 0x00, 0xFF]
        );
    }

    #[test]
    fn crc_is_computed_over_the_whole_buffer_without_zeroing() {
        let mut config = sample();
        // Put a known stale value into the CRC field first.
        config.as_mut_bytes()[OFF_CRC16..OFF_CRC16 + 4]
            .copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let expected = ansi_crc16_sh(config.as_bytes());
        let returned = config.recompute_crc();
        assert_eq!(returned, expected);
        assert_eq!(config.crc16_stored(), u32::from(expected));
        assert_eq!(
            &config.as_bytes()[OFF_CRC16..OFF_CRC16 + 4],
            &u32::from(expected).to_le_bytes()
        );
    }

    #[test]
    fn changing_one_field_leaves_everything_else_alone() {
        // Deterministic pseudo-random buffer, so the reserved regions are non-zero.
        let mut raw = [0u8; CONFIG_SIZE];
        let mut state = 0x1234_5678u32;
        for byte in &mut raw {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state as u8;
        }
        let original = Bt2Config::from_bytes(&raw).unwrap();
        let mut config = original.clone();

        config.set_profile_name(Slot::S2, "Changed").unwrap();
        let start = profile_name_offset(Slot::S2);
        let end = start + PROFILE_NAME_SIZE;
        for (i, (&before, &after)) in original
            .as_bytes()
            .iter()
            .zip(config.as_bytes())
            .enumerate()
        {
            if (start..end).contains(&i) {
                continue;
            }
            assert_eq!(before, after, "byte {i:#X} changed unexpectedly");
        }
        assert_eq!(config.profile_name(Slot::S2), "Changed");
    }

    #[test]
    fn from_bytes_requires_exact_length() {
        assert_eq!(
            Bt2Config::from_bytes(&[0u8; CONFIG_SIZE - 1]),
            Err(Error::InvalidLength {
                expected: CONFIG_SIZE,
                actual: CONFIG_SIZE - 1
            })
        );
    }

    #[test]
    fn copy_slot_copies_all_sections_and_legacy_regions() {
        let mut config = sample();
        config.set_rumble(Slot::S1, &RumbleRecord::from_level(3).unwrap());
        config.set_six_axis(
            Slot::S1,
            &SixAxisRecord {
                flag: 1,
                ..SixAxisRecord::default()
            },
        );
        // Non-zero legacy bytes.
        let legacy_a = OFF_LEGACY_A + 7;
        config.as_mut_bytes()[legacy_a] = 0xAB;
        let legacy_b = OFF_LEGACY_B + 1;
        config.as_mut_bytes()[legacy_b] = 0xCD;

        config.copy_slot(Slot::S2, Slot::S1);

        assert_eq!(config.slot_flag(Slot::S2), 1);
        assert_eq!(config.rumble(Slot::S2), config.rumble(Slot::S1));
        assert_eq!(config.six_axis(Slot::S2), config.six_axis(Slot::S1));
        assert_eq!(
            config.as_bytes()[OFF_LEGACY_A + LEGACY_A_SLOT_SIZE + 7],
            0xAB
        );
        assert_eq!(
            config.as_bytes()[OFF_LEGACY_B + LEGACY_B_SLOT_SIZE + 1],
            0xCD
        );
        // Official copy rewrites the fifth character's low byte to the profile number.
        assert_eq!(config.profile_name(Slot::S2), "Slot2");
    }

    #[test]
    fn delete_slot_clears_only_that_slot() {
        let mut config = sample();
        config.set_rumble(Slot::S2, &RumbleRecord::from_level(2).unwrap());
        config.set_cur_slot(Some(Slot::S2));
        config.delete_slot(Slot::S2);

        assert_eq!(config.slot_flag(Slot::S2), 0);
        assert_eq!(config.profile_name(Slot::S2), "");
        assert_eq!(config.rumble(Slot::S2).flag, 0);
        assert_eq!(config.cur_slot(), None);
        // Other slots untouched.
        assert_eq!(config.slot_flag(Slot::S1), 1);
        assert_eq!(config.profile_name(Slot::S1), "Slot1");
    }

    #[test]
    fn raw_roundtrip_preserves_unknown_values() {
        let mut raw = [0u8; CONFIG_SIZE];
        for (i, byte) in raw.iter_mut().enumerate() {
            *byte = (i * 37 % 251) as u8;
        }
        let config = Bt2Config::from_bytes(&raw).unwrap();
        assert_eq!(config.as_bytes(), &raw);
        assert_eq!(config.into_bytes(), raw);
    }

    #[test]
    fn mode_conversion() {
        assert_eq!(GamepadMode::try_from(0), Ok(GamepadMode::Switch));
        assert_eq!(GamepadMode::try_from(3), Ok(GamepadMode::XInput));
        assert_eq!(GamepadMode::try_from(9), Err(Error::UnknownGamepadMode(9)));
        assert_eq!(Slot::try_from(2), Ok(Slot::S3));
        assert_eq!(Slot::try_from(3), Err(Error::InvalidSlot(3)));
        assert_eq!(MacroIndex::try_from(3), Ok(MacroIndex::M4));
        assert_eq!(MacroIndex::try_from(4), Err(Error::InvalidMacroIndex(4)));
    }
}
