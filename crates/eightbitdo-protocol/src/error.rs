//! Error type shared by the protocol codec.

use core::fmt;

/// Errors produced while encoding or decoding protocol data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A buffer did not have the expected length.
    InvalidLength {
        /// Minimum or exact number of bytes required.
        expected: usize,
        /// Number of bytes actually available.
        actual: usize,
    },
    /// A slot index outside `0..=2` was supplied.
    InvalidSlot(u8),
    /// A macro index outside `0..=3` was supplied.
    InvalidMacroIndex(u8),
    /// A macro exceeded [`crate::macro_data::MAX_MACRO_STEPS`].
    TooManySteps {
        /// Maximum supported number of steps.
        max: usize,
        /// Number of steps supplied.
        actual: usize,
    },
    /// A profile or macro name did not fit in its 32-byte, UTF-16BE field.
    StringTooLong {
        /// Maximum number of UTF-16 code units.
        max_utf16_units: usize,
        /// Number of UTF-16 code units supplied.
        actual: usize,
    },
    /// A name contained a NUL code unit, which would truncate it on the wire.
    EmbeddedNul,
    /// A payload exceeded the transport-specific chunk limit.
    PayloadTooLarge {
        /// Maximum payload size in bytes.
        max: usize,
        /// Payload size supplied.
        actual: usize,
    },
    /// A chunk size was zero or exceeded the protocol limit.
    InvalidChunkSize {
        /// Maximum supported chunk size in bytes.
        max: usize,
        /// Chunk size supplied.
        actual: usize,
    },
    /// A read or write range exceeded its fixed-size backing buffer.
    RangeOutOfBounds {
        /// Start offset of the requested range.
        offset: u64,
        /// Length of the requested range in bytes.
        len: u64,
        /// Exclusive upper bound of the backing buffer.
        limit: u64,
    },
    /// A firmware image length was not a multiple of the 64-byte chunk size.
    UnalignedImageLength {
        /// Supplied image length in bytes.
        actual: usize,
        /// Required alignment in bytes.
        alignment: usize,
    },
    /// The declared firmware image length disagreed with the supplied slice.
    ImageLengthMismatch {
        /// Length declared in the request header.
        declared: u32,
        /// Actual length of the supplied image slice.
        actual: usize,
    },
    /// A flash address was not aligned to the erase sector size.
    UnalignedAddress {
        /// Address supplied by the caller.
        address: u32,
        /// Required alignment in bytes.
        alignment: u32,
    },
    /// The header `chunk_len` field disagreed with the actual payload length.
    ChunkLenMismatch {
        /// Value stored in the header.
        declared: u16,
        /// Actual payload length.
        actual: usize,
    },
    /// A packet did not start with the expected tag / prefix bytes.
    InvalidFrame(&'static str),
    /// An unknown `gamepad_mode` discriminant was read from a config block.
    UnknownGamepadMode(u16),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { expected, actual } => {
                write!(f, "invalid length: expected {expected} bytes, got {actual}")
            }
            Self::InvalidSlot(slot) => write!(f, "invalid slot {slot}, expected 0..=2"),
            Self::InvalidMacroIndex(idx) => write!(f, "invalid macro index {idx}, expected 0..=3"),
            Self::TooManySteps { max, actual } => {
                write!(f, "too many macro steps: max {max}, got {actual}")
            }
            Self::StringTooLong {
                max_utf16_units,
                actual,
            } => write!(
                f,
                "string too long: max {max_utf16_units} UTF-16 units, got {actual}"
            ),
            Self::EmbeddedNul => write!(f, "string contains a NUL code unit"),
            Self::PayloadTooLarge { max, actual } => {
                write!(f, "payload too large: max {max} bytes, got {actual}")
            }
            Self::InvalidChunkSize { max, actual } => {
                write!(
                    f,
                    "invalid chunk size: expected 1..={max} bytes, got {actual}"
                )
            }
            Self::RangeOutOfBounds { offset, len, limit } => write!(
                f,
                "range 0x{offset:X}..0x{:X} exceeds limit 0x{limit:X}",
                offset.saturating_add(*len)
            ),
            Self::UnalignedImageLength { actual, alignment } => write!(
                f,
                "firmware image length {actual} is not a multiple of {alignment}"
            ),
            Self::ImageLengthMismatch { declared, actual } => write!(
                f,
                "declared firmware image length {declared} does not match supplied {actual} bytes"
            ),
            Self::UnalignedAddress { address, alignment } => {
                write!(f, "address 0x{address:X} is not aligned to 0x{alignment:X}")
            }
            Self::ChunkLenMismatch { declared, actual } => write!(
                f,
                "header chunk_len is {declared} but payload is {actual} bytes"
            ),
            Self::InvalidFrame(what) => write!(f, "invalid frame: {what}"),
            Self::UnknownGamepadMode(mode) => write!(f, "unknown gamepad mode {mode}"),
        }
    }
}

impl core::error::Error for Error {}
