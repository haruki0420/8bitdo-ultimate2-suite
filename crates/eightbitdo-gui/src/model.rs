//! Display models built from core/protocol values.
//!
//! The worker thread converts raw snapshots into these plain values once, so
//! the UI thread never parses protocol bytes while painting a frame.

use eightbitdo_core::{ConfigSnapshot, VersionInfo};
use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord, Slot, MACROS_PER_SLOT};
use eightbitdo_protocol::input_report::InputReport;
use eightbitdo_protocol::keymap::KeyMap;

use crate::util::sha256_hex;

/// One entry from a BLE scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    /// Advertised name, when the platform exposes one.
    pub name: Option<String>,
    /// Bluetooth address in platform display form.
    pub address: String,
    /// Signal strength in dBm.
    pub rssi: Option<i16>,
    /// True when the device advertises `80ND` or service `FF10`.
    pub is_app: bool,
}

/// Metadata for the connected link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfoView {
    /// Advertised name captured during the scan.
    pub name: Option<String>,
    /// Bluetooth address.
    pub address: String,
    /// Negotiated ATT MTU.
    pub mtu: u16,
    /// UUID of the App Mode characteristic (`FF11` or `FF13`).
    pub characteristic: String,
}

/// Decoded firmware version, mirroring the CLI `info` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionView {
    /// Raw response payload as lowercase hex.
    pub raw_hex: String,
    /// Printable ASCII run in the payload, when present.
    pub text: Option<String>,
    /// Little-endian `u32` value, when the payload is exactly four bytes.
    pub value: Option<u32>,
}

impl VersionView {
    /// Build the display form from a core [`VersionInfo`].
    #[must_use]
    pub fn from_info(info: &VersionInfo) -> Self {
        let value = <[u8; 4]>::try_from(info.raw.as_slice())
            .ok()
            .map(u32::from_le_bytes);
        Self {
            raw_hex: hex::encode(&info.raw),
            text: (!info.text.is_empty()).then(|| info.text.clone()),
            value,
        }
    }
}

/// One macro record header (no steps are read in Phase 3a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroRecordView {
    /// One-based macro number (`1..=4`).
    pub number: u8,
    /// Stored name, decoded as UTF-16BE by the protocol crate.
    pub name: String,
    /// Stored `gamepad_mode` byte.
    pub gamepad_mode: u8,
    /// Stored `special_flag` byte.
    pub special_flag: u8,
    /// Maximum number of 10-byte steps.
    pub max_steps: u16,
    /// Flash offset in bytes.
    pub offset: u16,
    /// Human-readable key mask.
    pub key_map: String,
    /// Raw key mask.
    pub key_map_bits: u32,
    /// Number of cycles.
    pub cycles: u32,
    /// Interval between steps in milliseconds.
    pub interval_ms: u32,
}

/// Macro header of one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroSlotView {
    /// One-based profile number (`1..=3`).
    pub profile: u8,
    /// Slot flag.
    pub flag: u32,
    /// Number of valid macros in this profile.
    pub total_cnt: u8,
    /// Four header records in wire order.
    pub records: [MacroRecordView; MACROS_PER_SLOT],
}

/// A read-only summary of the 2768-byte configuration block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigView {
    /// Block size in bytes.
    pub size: usize,
    /// SHA-256 of the raw block.
    pub sha256: String,
    /// Stored CRC at `+0x00C`.
    pub stored_crc: u16,
    /// CRC recomputed in place over the received buffer.
    pub computed_crc: u16,
    /// Informational comparison; a mismatch is not proof of corruption.
    pub crc_valid: bool,
    /// Raw `cur_slot` value (`4` means none).
    pub cur_slot_raw: u16,
    /// Human-readable active profile.
    pub cur_slot_label: String,
    /// Raw `gamepad_mode` value.
    pub gamepad_mode_raw: u16,
    /// Human-readable `gamepad_mode`.
    pub gamepad_mode_label: String,
    /// Three profile flags.
    pub slot_flags: [u32; 3],
    /// Three profile names (unescaped; use [`display_text`] in the UI).
    pub profile_names: [String; 3],
    /// Macro headers for every profile.
    pub macro_slots: [MacroSlotView; 3],
}

impl ConfigView {
    /// Build the display form from a validated core snapshot.
    #[must_use]
    pub fn from_snapshot(snapshot: &ConfigSnapshot) -> Self {
        let config = &snapshot.config;
        Self {
            size: config.as_bytes().len(),
            sha256: sha256_hex(config.as_bytes()),
            stored_crc: snapshot.stored_crc,
            computed_crc: snapshot.computed_crc,
            crc_valid: snapshot.crc_valid,
            cur_slot_raw: config.cur_slot_raw(),
            cur_slot_label: cur_slot_label(config.cur_slot_raw()),
            gamepad_mode_raw: config.gamepad_mode_raw(),
            gamepad_mode_label: config.gamepad_mode().map_or_else(
                || "unknown".to_string(),
                |mode| gamepad_mode_name(mode).to_string(),
            ),
            slot_flags: config.slot_flags(),
            profile_names: [
                config.profile_name(Slot::S1),
                config.profile_name(Slot::S2),
                config.profile_name(Slot::S3),
            ],
            macro_slots: [
                macro_slot_view(Slot::S1, config.macro_slot(Slot::S1)),
                macro_slot_view(Slot::S2, config.macro_slot(Slot::S2)),
                macro_slot_view(Slot::S3, config.macro_slot(Slot::S3)),
            ],
        }
    }
}

fn macro_slot_view(
    slot: Slot,
    header: eightbitdo_protocol::config_bt2::MacroSlot,
) -> MacroSlotView {
    MacroSlotView {
        profile: slot.number(),
        flag: header.flag,
        total_cnt: header.total_cnt,
        records: std::array::from_fn(|index| macro_record_view(index, &header.records[index])),
    }
}

fn macro_record_view(index: usize, record: &MacroRecord) -> MacroRecordView {
    MacroRecordView {
        number: index as u8 + 1,
        name: record.name(),
        gamepad_mode: record.gamepad_mode,
        special_flag: record.special_flag,
        max_steps: record.max_steps,
        offset: record.offset,
        key_map: keymap_label(record.key_map),
        key_map_bits: record.key_map.bits(),
        cycles: record.cycles_num,
        interval_ms: record.interval_ms,
    }
}

/// The latest realtime input report, normalized for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputView {
    /// Button mask.
    pub keys: KeyMap,
    /// Left stick in `-128..=128`.
    pub left_stick: (i16, i16),
    /// Right stick in `-128..=128`.
    pub right_stick: (i16, i16),
    /// Left trigger (`0..=255`).
    pub trigger_l: u8,
    /// Right trigger (`0..=255`).
    pub trigger_r: u8,
    /// Battery byte as reported; its scale is not confirmed.
    pub battery: u8,
}

impl From<InputReport> for InputView {
    fn from(report: InputReport) -> Self {
        Self {
            keys: report.key_map(),
            left_stick: report.left_stick(),
            right_stick: report.right_stick(),
            trigger_l: report.trigger_l,
            trigger_r: report.trigger_r,
            battery: report.battery,
        }
    }
}

/// Button labels in a stable display order.
pub const BUTTON_LABELS: [(&str, KeyMap); 26] = [
    ("A", KeyMap::A),
    ("B", KeyMap::B),
    ("X", KeyMap::X),
    ("Y", KeyMap::Y),
    ("L1", KeyMap::L1),
    ("R1", KeyMap::R1),
    ("L2", KeyMap::L2),
    ("R2", KeyMap::R2),
    ("L3", KeyMap::L3),
    ("R3", KeyMap::R3),
    ("Select", KeyMap::SELECT),
    ("Start", KeyMap::START),
    ("Share", KeyMap::SHARE),
    ("Logo", KeyMap::LOGO),
    ("Up", KeyMap::UP),
    ("Down", KeyMap::DOWN),
    ("Left", KeyMap::LEFT),
    ("Right", KeyMap::RIGHT),
    ("P1", KeyMap::P1),
    ("P2", KeyMap::P2),
    ("P3", KeyMap::P3),
    ("P4", KeyMap::P4),
    ("Home", KeyMap::HOME),
    ("Turbo", KeyMap::TURBO),
    ("Turbo Auto", KeyMap::TURBO_AUTO),
    ("Screenshot", KeyMap::SCREENSHOT),
];

/// Render a key mask as labels, with unknown bits kept visible.
#[must_use]
pub fn keymap_label(map: KeyMap) -> String {
    if map.is_empty() {
        return "None".to_string();
    }
    let mut parts = Vec::new();
    let mut known = 0u32;
    for (label, flag) in BUTTON_LABELS {
        if map.contains(flag) {
            parts.push(label.to_string());
            known |= flag.bits();
        }
    }
    let unknown = map.bits() & !known;
    if unknown != 0 {
        parts.push(format!("Unknown(0x{unknown:08X})"));
    }
    parts.join(" + ")
}

/// Escape non-ASCII and control characters for the default (Latin) font.
///
/// The GUI does not bundle a CJK font, so a device name such as `プロファイル`
/// is shown as `\u{30D7}...` instead of missing glyphs. The raw value is kept
/// unchanged in [`ConfigView`] and in the JSON backup.
#[must_use]
pub fn display_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_graphic() || character == ' ' {
            output.push(character);
        } else if character.is_ascii_control() {
            output.push_str(&format!("\\x{:02X}", character as u32));
        } else {
            output.push_str(&format!("\\u{{{:04X}}}", character as u32));
        }
    }
    output
}

fn gamepad_mode_name(mode: GamepadMode) -> &'static str {
    match mode {
        GamepadMode::Switch => "Switch",
        GamepadMode::GamepadJM => "Gamepad J/M",
        GamepadMode::Mac => "Mac",
        GamepadMode::XInput => "XInput",
        GamepadMode::Extended => "Extended",
        GamepadMode::Unselected => "Unselected",
    }
}

fn cur_slot_label(raw: u16) -> String {
    match Slot::from_index(raw as usize) {
        Some(slot) => format!("Profile {} (raw {raw})", slot.number()),
        None if raw == 4 => "None (raw 4)".to_string(),
        None => format!("Unknown (raw {raw})"),
    }
}

/// Format a 32-bit value as `0xXXXXXXXX`.
#[must_use]
pub fn format_u32_hex(value: u32) -> String {
    format!("0x{value:08X}")
}

/// Format a 16-bit value as `0xXXXX`.
#[must_use]
pub fn format_u16_hex(value: u16) -> String {
    format!("0x{value:04X}")
}

/// Format an RSSI reading, or `-` when the platform did not report one.
#[must_use]
pub fn format_rssi(rssi: Option<i16>) -> String {
    rssi.map_or_else(|| "-".to_string(), |value| format!("{value} dBm"))
}

#[cfg(test)]
mod tests {
    use eightbitdo_protocol::config_bt2::{Bt2Config, MacroSlot, CONFIG_SIZE};

    use super::*;

    fn snapshot(config: Bt2Config) -> ConfigSnapshot {
        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        ConfigSnapshot {
            config,
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        }
    }

    #[test]
    fn default_config_is_viewable_without_panicking() {
        let view = ConfigView::from_snapshot(&snapshot(Bt2Config::default()));
        assert_eq!(view.size, CONFIG_SIZE);
        assert_eq!(view.sha256.len(), 64);
        assert_eq!(view.cur_slot_label, "Profile 1 (raw 0)");
        assert_eq!(view.gamepad_mode_label, "Switch");
        assert_eq!(view.profile_names, ["", "", ""]);
        assert_eq!(view.macro_slots.len(), 3);
        assert_eq!(view.macro_slots[0].records.len(), 4);
        assert_eq!(view.macro_slots[2].records[3].number, 4);
    }

    #[test]
    fn config_view_reads_profile_and_macro_headers() {
        let mut config = Bt2Config::default();
        config.set_profile_name(Slot::S1, "Alpha").unwrap();
        config.set_gamepad_mode(GamepadMode::XInput);
        config.set_cur_slot(Some(Slot::S2));
        config.set_slot_flag(Slot::S1, 0x2020_0911);

        let mut header = MacroSlot {
            flag: 0x0000_0001,
            total_cnt: 2,
            ..MacroSlot::default()
        };
        let record = &mut header.records[0];
        record.set_name("Combo").unwrap();
        record.max_steps = 12;
        record.offset = 0x1000;
        record.key_map = KeyMap::A | KeyMap::B;
        record.cycles_num = 3;
        record.interval_ms = 20;
        config.set_macro_slot(Slot::S2, &header);

        let view = ConfigView::from_snapshot(&snapshot(config));
        assert_eq!(view.profile_names[0], "Alpha");
        assert_eq!(view.gamepad_mode_raw, GamepadMode::XInput as u16);
        assert_eq!(view.gamepad_mode_label, "XInput");
        assert_eq!(view.cur_slot_label, "Profile 2 (raw 1)");
        assert_eq!(view.slot_flags[0], 0x2020_0911);

        let macro_view = &view.macro_slots[1];
        assert_eq!(macro_view.profile, 2);
        assert_eq!(macro_view.flag, 1);
        assert_eq!(macro_view.total_cnt, 2);
        assert_eq!(macro_view.records[0].name, "Combo");
        assert_eq!(macro_view.records[0].max_steps, 12);
        assert_eq!(macro_view.records[0].offset, 0x1000);
        assert_eq!(macro_view.records[0].key_map, "A + B");
        assert_eq!(macro_view.records[0].key_map_bits, 0x3000);
        assert_eq!(macro_view.records[0].cycles, 3);
        assert_eq!(macro_view.records[0].interval_ms, 20);
    }

    #[test]
    fn keymap_label_keeps_unknown_bits_visible() {
        assert_eq!(keymap_label(KeyMap::NONE), "None");
        assert_eq!(keymap_label(KeyMap::R1 | KeyMap::L1), "L1 + R1");
        assert_eq!(
            keymap_label(KeyMap::from_bits(1 << 31)),
            "Unknown(0x80000000)"
        );
        assert_eq!(
            keymap_label(KeyMap::A | KeyMap::from_bits(1 << 31)),
            "A + Unknown(0x80000000)"
        );
    }

    #[test]
    fn input_view_normalizes_sticks_and_keeps_raw_triggers() {
        let report = InputReport::from_bytes(&[
            0x01, 0x20, 0x00, 0x00, 0x7F, 0x80, 0xFF, 0x00, 0xCC, 0x33, 0x4B, 0x00,
        ])
        .unwrap();
        let view = InputView::from(report);
        assert_eq!(view.keys, KeyMap::START | KeyMap::A);
        assert_eq!(view.left_stick, (0, 0));
        assert_eq!(view.right_stick, (128, -127));
        assert_eq!(view.trigger_l, 0x33);
        assert_eq!(view.trigger_r, 0xCC);
        assert_eq!(view.battery, 0x4B);
    }

    #[test]
    fn version_view_uses_little_endian_value() {
        let info = VersionInfo {
            raw: vec![0xC8, 0x00, 0x00, 0x00],
            text: String::new(),
        };
        let view = VersionView::from_info(&info);
        assert_eq!(view.raw_hex, "c8000000");
        assert_eq!(view.value, Some(200));
        assert_eq!(view.text, None);

        let text_info = VersionInfo {
            raw: vec![0x56, 0x32, 0x2E, 0x30],
            text: "V2.0".to_string(),
        };
        let text_view = VersionView::from_info(&text_info);
        assert_eq!(text_view.text.as_deref(), Some("V2.0"));
        assert_eq!(
            text_view.value,
            Some(u32::from_le_bytes([0x56, 0x32, 0x2E, 0x30]))
        );
    }

    #[test]
    fn display_text_escapes_non_latin_and_control_characters() {
        assert_eq!(display_text("Profile 1"), "Profile 1");
        assert_eq!(display_text("Macro\t1"), "Macro\\x091");
        assert_eq!(display_text("プロ"), "\\u{30D7}\\u{30ED}");
        assert_eq!(display_text("a\u{7}b"), "a\\x07b");
    }

    #[test]
    fn hex_and_rssi_helpers_are_stable() {
        assert_eq!(format_u32_hex(0x2020_0911), "0x20200911");
        assert_eq!(format_u16_hex(0x0AD0), "0x0AD0");
        assert_eq!(format_rssi(None), "-");
        assert_eq!(format_rssi(Some(-42)), "-42 dBm");
    }
}
