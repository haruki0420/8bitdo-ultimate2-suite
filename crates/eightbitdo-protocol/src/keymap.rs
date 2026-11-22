//! 32-bit `KeyMap` button masks and the 22-entry remapping index.

use core::fmt;
use core::ops::{BitAnd, BitOr, BitXor};

/// A 32-bit button mask as used by mappings, macros and special features.
///
/// Single-button constants combine with the usual bit operators:
///
/// ```
/// use eightbitdo_protocol::keymap::KeyMap;
///
/// let combo = KeyMap::L1 | KeyMap::R1;
/// assert!(combo.contains(KeyMap::L1));
/// assert_eq!(combo.bits(), 0x0000_0C00);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct KeyMap(pub u32);

impl KeyMap {
    /// Unmapped / disabled (`0`).
    pub const NONE: Self = Self(0);
    /// Start / Menu / `+` (bit 0).
    pub const START: Self = Self(0x0000_0001);
    /// L3, left stick click (bit 1).
    pub const L3: Self = Self(0x0000_0002);
    /// R3, right stick click (bit 2).
    pub const R3: Self = Self(0x0000_0004);
    /// Select / View / `-` (bit 3).
    pub const SELECT: Self = Self(0x0000_0008);
    /// X (bit 4).
    pub const X: Self = Self(0x0000_0010);
    /// Y (bit 5).
    pub const Y: Self = Self(0x0000_0020);
    /// D-Pad right (bit 6).
    pub const RIGHT: Self = Self(0x0000_0040);
    /// D-Pad left (bit 7).
    pub const LEFT: Self = Self(0x0000_0080);
    /// D-Pad down (bit 8).
    pub const DOWN: Self = Self(0x0000_0100);
    /// D-Pad up (bit 9).
    pub const UP: Self = Self(0x0000_0200);
    /// L1 / LB (bit 10).
    pub const L1: Self = Self(0x0000_0400);
    /// R1 / RB (bit 11).
    pub const R1: Self = Self(0x0000_0800);
    /// B (bit 12).
    pub const B: Self = Self(0x0000_1000);
    /// A (bit 13).
    pub const A: Self = Self(0x0000_2000);
    /// L2 / LT (bit 14).
    pub const L2: Self = Self(0x0000_4000);
    /// R2 / RT (bit 15).
    pub const R2: Self = Self(0x0000_8000);
    /// Turbo / manual rapid fire (bit 16).
    pub const TURBO: Self = Self(0x0001_0000);
    /// Home (bit 17).
    pub const HOME: Self = Self(0x0002_0000);
    /// Bluetooth connect / pairing (bit 18).
    pub const BT_CON: Self = Self(0x0004_0000);
    /// P3, extra left bumper / L4 (bit 21).
    pub const P3: Self = Self(0x0020_0000);
    /// Capture / screenshot (bit 22).
    pub const SCREENSHOT: Self = Self(0x0040_0000);
    /// Special turbo, 3-stage (bit 23).
    pub const TURBO_3: Self = Self(0x0080_0000);
    /// Auto turbo (bit 24).
    pub const TURBO_AUTO: Self = Self(0x0100_0000);
    /// P1, left back paddle (bit 25).
    pub const P1: Self = Self(0x0200_0000);
    /// P2, right back paddle (bit 26).
    pub const P2: Self = Self(0x0400_0000);
    /// Profile / button swap (bit 27).
    pub const SWAP: Self = Self(0x0800_0000);
    /// P4, extra right bumper / R4 (bit 30).
    pub const P4: Self = Self(0x4000_0000);

    /// Macro 1 identifier (`119`), a non-bitmask value.
    pub const M1: Self = Self(119);
    /// Macro 2 identifier (`120`), a non-bitmask value.
    pub const M2: Self = Self(120);
    /// Macro 3 identifier (`121`), a non-bitmask value.
    pub const M3: Self = Self(121);
    /// Macro 4 identifier (`122`), a non-bitmask value.
    pub const M4: Self = Self(122);
    /// Share button identifier (`123`), a non-bitmask value.
    pub const SHARE: Self = Self(123);
    /// Logo / View button identifier (`124`), a non-bitmask value.
    pub const LOGO: Self = Self(124);
    /// UI placeholder for "null" (`227`).
    pub const MY_NULL: Self = Self(227);

    /// Wrap raw mask bits.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Return the raw mask bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// True when no button is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when every bit of `other` is present in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Name of a single-button or well-known identifier mask.
    ///
    /// Returns `None` for combined masks such as `L1 | R1`.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            0 => "None",
            0x0000_0001 => "Start",
            0x0000_0002 => "L3",
            0x0000_0004 => "R3",
            0x0000_0008 => "Select",
            0x0000_0010 => "X",
            0x0000_0020 => "Y",
            0x0000_0040 => "Right",
            0x0000_0080 => "Left",
            0x0000_0100 => "Down",
            0x0000_0200 => "Up",
            0x0000_0400 => "L1",
            0x0000_0800 => "R1",
            0x0000_1000 => "B",
            0x0000_2000 => "A",
            0x0000_4000 => "L2",
            0x0000_8000 => "R2",
            0x0001_0000 => "Turbo",
            0x0002_0000 => "Home",
            0x0004_0000 => "BT_CON",
            0x0020_0000 => "P3",
            0x0040_0000 => "ScreenShot",
            0x0080_0000 => "Turbo_3",
            0x0100_0000 => "Turbo_Auto",
            0x0200_0000 => "P1",
            0x0400_0000 => "P2",
            0x0800_0000 => "Swap",
            0x4000_0000 => "P4",
            119 => "M1",
            120 => "M2",
            121 => "M3",
            122 => "M4",
            123 => "Share",
            124 => "Logo",
            227 => "My_Null",
            _ => return None,
        })
    }
}

impl KeyMap {
    /// True when this is one documented single `KeyMap` constant.
    ///
    /// This is the whitelist for button remapping and `gyro.key`: the value
    /// must have a name in [`KeyMap::name`] and must not be the unconfirmed
    /// `KeyMap::MY_NULL` (227). Unknown bits, combinations and 227 return
    /// false. Deriving the answer from `name()` keeps a single source of
    /// truth for the documented constants.
    #[must_use]
    pub const fn is_single_documented(self) -> bool {
        self.0 != 227 && self.name().is_some()
    }
}

impl From<u32> for KeyMap {
    fn from(bits: u32) -> Self {
        Self(bits)
    }
}

impl From<KeyMap> for u32 {
    fn from(map: KeyMap) -> Self {
        map.0
    }
}

impl BitOr for KeyMap {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitAnd for KeyMap {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitXor for KeyMap {
    type Output = Self;

    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

impl fmt::Display for KeyMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "KeyMap(0x{:08X})", self.0),
        }
    }
}

/// Physical button index used by the 22-entry `MapKeyRecord.key` array.
///
/// The order is the non-3E layout of `BasicAdvanceUI.getMappings()`:
/// `[A, B, X, Y, L1, R1, L2, R2, L3, R3, Select, Start, Share, Logo,
/// Up, Down, Left, Right, P1, P2, P3, P4]`.
///
/// Switch UI swaps A/B and X/Y labels, but the stored indices do not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum ButtonIndex {
    /// A (physical bottom face button).
    A = 0,
    /// B (physical right face button).
    B = 1,
    /// X (physical left face button).
    X = 2,
    /// Y (physical top face button).
    Y = 3,
    /// L1 / LB.
    L1 = 4,
    /// R1 / RB.
    R1 = 5,
    /// L2 / LT.
    L2 = 6,
    /// R2 / RT.
    R2 = 7,
    /// L3 (left stick click).
    L3 = 8,
    /// R3 (right stick click).
    R3 = 9,
    /// Select / View / `-`.
    Select = 10,
    /// Start / Menu / `+`.
    Start = 11,
    /// Share / Star / screenshot.
    Share = 12,
    /// Logo / Home.
    Logo = 13,
    /// D-Pad up.
    Up = 14,
    /// D-Pad down.
    Down = 15,
    /// D-Pad left.
    Left = 16,
    /// D-Pad right.
    Right = 17,
    /// P1, left back paddle.
    P1 = 18,
    /// P2, right back paddle.
    P2 = 19,
    /// P3, extra left bumper.
    P3 = 20,
    /// P4, extra right bumper.
    P4 = 21,
}

impl ButtonIndex {
    /// All 22 indices in wire order.
    pub const ALL: [Self; 22] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::L1,
        Self::R1,
        Self::L2,
        Self::R2,
        Self::L3,
        Self::R3,
        Self::Select,
        Self::Start,
        Self::Share,
        Self::Logo,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::P1,
        Self::P2,
        Self::P3,
        Self::P4,
    ];

    /// Array index (`0..=21`).
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Look up an index from the wire array.
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Self> {
        if index < 22 {
            Some(Self::ALL[index])
        } else {
            None
        }
    }

    /// The default (identity) mapping target for this physical button.
    #[must_use]
    pub const fn default_key(self) -> KeyMap {
        match self {
            Self::A => KeyMap::A,
            Self::B => KeyMap::B,
            Self::X => KeyMap::X,
            Self::Y => KeyMap::Y,
            Self::L1 => KeyMap::L1,
            Self::R1 => KeyMap::R1,
            Self::L2 => KeyMap::L2,
            Self::R2 => KeyMap::R2,
            Self::L3 => KeyMap::L3,
            Self::R3 => KeyMap::R3,
            Self::Select => KeyMap::SELECT,
            Self::Start => KeyMap::START,
            Self::Share => KeyMap::SHARE,
            Self::Logo => KeyMap::LOGO,
            Self::Up => KeyMap::UP,
            Self::Down => KeyMap::DOWN,
            Self::Left => KeyMap::LEFT,
            Self::Right => KeyMap::RIGHT,
            Self::P1 => KeyMap::P1,
            Self::P2 => KeyMap::P2,
            Self::P3 => KeyMap::P3,
            Self::P4 => KeyMap::P4,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::L1 => "L1",
            Self::R1 => "R1",
            Self::L2 => "L2",
            Self::R2 => "R2",
            Self::L3 => "L3",
            Self::R3 => "R3",
            Self::Select => "Select",
            Self::Start => "Start",
            Self::Share => "Share",
            Self::Logo => "Logo",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::P1 => "P1",
            Self::P2 => "P2",
            Self::P3 => "P3",
            Self::P4 => "P4",
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn bit_positions_match_the_spec() {
        let expected = [
            (KeyMap::START, 0),
            (KeyMap::L3, 1),
            (KeyMap::R3, 2),
            (KeyMap::SELECT, 3),
            (KeyMap::X, 4),
            (KeyMap::Y, 5),
            (KeyMap::RIGHT, 6),
            (KeyMap::LEFT, 7),
            (KeyMap::DOWN, 8),
            (KeyMap::UP, 9),
            (KeyMap::L1, 10),
            (KeyMap::R1, 11),
            (KeyMap::B, 12),
            (KeyMap::A, 13),
            (KeyMap::L2, 14),
            (KeyMap::R2, 15),
            (KeyMap::TURBO, 16),
            (KeyMap::HOME, 17),
            (KeyMap::BT_CON, 18),
            (KeyMap::P3, 21),
            (KeyMap::SCREENSHOT, 22),
            (KeyMap::TURBO_3, 23),
            (KeyMap::TURBO_AUTO, 24),
            (KeyMap::P1, 25),
            (KeyMap::P2, 26),
            (KeyMap::SWAP, 27),
            (KeyMap::P4, 30),
        ];
        for (key, bit) in expected {
            assert_eq!(key.bits(), 1 << bit, "{key}");
        }
    }

    #[test]
    fn non_bitmask_constants() {
        assert_eq!(KeyMap::M1.bits(), 119);
        assert_eq!(KeyMap::M2.bits(), 120);
        assert_eq!(KeyMap::M3.bits(), 121);
        assert_eq!(KeyMap::M4.bits(), 122);
        assert_eq!(KeyMap::SHARE.bits(), 123);
        assert_eq!(KeyMap::LOGO.bits(), 124);
        assert_eq!(KeyMap::MY_NULL.bits(), 227);
    }

    #[test]
    fn combining_and_testing_bits() {
        let combo = KeyMap::L1 | KeyMap::R2;
        assert!(combo.contains(KeyMap::L1));
        assert!(combo.contains(KeyMap::R2));
        assert!(!combo.contains(KeyMap::R1));
        assert_eq!(combo.bits(), 0x0000_8400);
    }

    #[test]
    fn names_and_display() {
        assert_eq!(KeyMap::A.name(), Some("A"));
        assert_eq!(KeyMap::NONE.name(), Some("None"));
        assert_eq!((KeyMap::A | KeyMap::B).name(), None);
        assert_eq!(KeyMap::A.to_string(), "A");
        assert_eq!((KeyMap::A | KeyMap::B).to_string(), "KeyMap(0x00003000)");
    }

    #[test]
    fn single_documented_whitelist_matches_the_documented_constants() {
        for key in [
            KeyMap::NONE,
            KeyMap::START,
            KeyMap::L3,
            KeyMap::R3,
            KeyMap::SELECT,
            KeyMap::X,
            KeyMap::Y,
            KeyMap::RIGHT,
            KeyMap::LEFT,
            KeyMap::DOWN,
            KeyMap::UP,
            KeyMap::L1,
            KeyMap::R1,
            KeyMap::B,
            KeyMap::A,
            KeyMap::L2,
            KeyMap::R2,
            KeyMap::TURBO,
            KeyMap::HOME,
            KeyMap::BT_CON,
            KeyMap::P3,
            KeyMap::SCREENSHOT,
            KeyMap::TURBO_3,
            KeyMap::TURBO_AUTO,
            KeyMap::P1,
            KeyMap::P2,
            KeyMap::SWAP,
            KeyMap::P4,
            KeyMap::M1,
            KeyMap::M2,
            KeyMap::M3,
            KeyMap::M4,
            KeyMap::SHARE,
            KeyMap::LOGO,
        ] {
            assert!(
                key.is_single_documented(),
                "0x{:08X} should be documented",
                key.bits()
            );
        }
        assert!(!KeyMap::MY_NULL.is_single_documented());
        assert!(!KeyMap::from_bits(227).is_single_documented());
        assert!(!(KeyMap::A | KeyMap::B).is_single_documented());
        assert!(!KeyMap::from_bits(0x0000_0018).is_single_documented());
        assert!(!KeyMap::from_bits(0x8000_0000).is_single_documented());
        // The whitelist and the name table agree for every accepted value.
        for bits in 0u32..=0xFFFF {
            let key = KeyMap::from_bits(bits);
            if key.is_single_documented() {
                assert!(key.name().is_some());
            }
        }
    }

    #[test]
    fn button_index_order_matches_mapping_array() {
        assert_eq!(ButtonIndex::ALL.len(), 22);
        for (i, button) in ButtonIndex::ALL.iter().enumerate() {
            assert_eq!(button.index(), i);
            assert_eq!(ButtonIndex::from_index(i), Some(*button));
        }
        assert_eq!(ButtonIndex::from_index(22), None);

        assert_eq!(ButtonIndex::A.index(), 0);
        assert_eq!(ButtonIndex::Y.index(), 3);
        assert_eq!(ButtonIndex::Start.index(), 11);
        assert_eq!(ButtonIndex::P1.index(), 18);
        assert_eq!(ButtonIndex::P4.index(), 21);
    }

    #[test]
    fn default_mapping_is_identity() {
        for button in ButtonIndex::ALL {
            assert_eq!(button.default_key(), button.default_key());
        }
        assert_eq!(ButtonIndex::A.default_key(), KeyMap::A);
        assert_eq!(ButtonIndex::Share.default_key(), KeyMap::SHARE);
        assert_eq!(ButtonIndex::Logo.default_key(), KeyMap::LOGO);
    }

    #[test]
    fn conversions_are_lossless() {
        let key = KeyMap::P4;
        assert_eq!(KeyMap::from(u32::from(key)), key);
        assert_eq!(u32::from(KeyMap::from(0x1234_5678)), 0x1234_5678);
    }
}
