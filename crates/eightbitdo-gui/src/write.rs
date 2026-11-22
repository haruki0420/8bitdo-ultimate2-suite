//! Write-side GUI state: settings drafts, button remapping, macro recording,
//! macro editing, and the pending-write confirmation model.
//!
//! Everything in this module is pure: no device I/O and no egui calls, so the
//! transitions can be unit-tested headlessly. The worker is the only place that
//! touches the controller; a [`PendingWrite`] produced here is only sent after
//! the user confirms it in the write dialog.
//!
//! # Safety rules encoded here
//!
//! * A draft edit only becomes a [`ConfigEdit`] when the value actually
//!   differs from the configuration read from the device.
//! * Button remapping targets are restricted to
//!   [`KeyMap::is_single_documented`]: `KeyMap::MY_NULL` (227), unknown bits and
//!   multi-button combinations are never selectable.
//! * The remap record `flag` is written as `0` or `1` only.
//! * A macro cannot be cleared: writing an empty step list is refused, and an
//!   empty target macro is refused before any frame is built.
//! * Macro durations are 1 ms based; a zero-duration step is refused.

use eightbitdo_core::config_ops::{button_mapping, map_key_flag, ConfigEdit, ConfigWritePlan};
use eightbitdo_core::macro_ops::{is_skipped_steps, MacroEntry, MacroWritePlan};
use eightbitdo_core::ConfigSnapshot;
use eightbitdo_protocol::config_bt2::{
    Bt2Config, DynamicTheme, GamepadMode, HotWheelTheme, JoyRecord, MacroIndex, RumbleRecord,
    SingleTheme, SixAxisRecord, Slot, SpecialFeature, SpecialRecord, TriggerRecord, BUTTON_COUNT,
    SINGLE_LED_COUNT,
};
use eightbitdo_protocol::keymap::{ButtonIndex, KeyMap};
use eightbitdo_protocol::macro_data::{MacroStep, AXIS_NEUTRAL, MAX_MACRO_STEPS};

use crate::model::InputView;

/// Length of one live-input recording step.
///
/// The worker polls `0x0003` every 20 ms, so one report per tick maps exactly
/// to a 20 ms macro step. Identical consecutive reports are coalesced by
/// extending the previous step's duration.
pub const RECORD_STEP_MS: u16 = 20;

/// Default duration inserted by "wait" in the macro editor.
pub const DEFAULT_WAIT_MS: u16 = 100;

/// Maximum profile / macro name length in UTF-16 code units.
pub const MAX_NAME_UNITS: usize = 16;

/// Maximum cycles and interval values offered by the macro editor.
///
/// The wire fields are `u32`; these bounds only keep the editor's numeric input
/// finite and readable.
pub const MAX_CYCLES: u32 = 9999;
/// Upper bound for the inter-loop delay in the macro editor.
pub const MAX_INTERVAL_MS: u32 = 60_000;

/// Gamepad modes offered by the settings editor, in wire order.
pub const GAMEPAD_MODES: [GamepadMode; 6] = [
    GamepadMode::Switch,
    GamepadMode::GamepadJM,
    GamepadMode::Mac,
    GamepadMode::XInput,
    GamepadMode::Extended,
    GamepadMode::Unselected,
];

/// Single documented key values that may be chosen as a remap target, a macro
/// trigger or a gyro activation button.
///
/// This is exactly the `KeyMap::is_single_documented` whitelist
/// (`KeyMap::MY_NULL` = 227, unknown bits and multi-button combinations are
/// excluded by construction).
pub const KEY_TARGETS: [KeyMap; 34] = [
    KeyMap::NONE,
    KeyMap::A,
    KeyMap::B,
    KeyMap::X,
    KeyMap::Y,
    KeyMap::L1,
    KeyMap::R1,
    KeyMap::L2,
    KeyMap::R2,
    KeyMap::L3,
    KeyMap::R3,
    KeyMap::SELECT,
    KeyMap::START,
    KeyMap::SHARE,
    KeyMap::LOGO,
    KeyMap::UP,
    KeyMap::DOWN,
    KeyMap::LEFT,
    KeyMap::RIGHT,
    KeyMap::P1,
    KeyMap::P2,
    KeyMap::P3,
    KeyMap::P4,
    KeyMap::HOME,
    KeyMap::TURBO,
    KeyMap::TURBO_AUTO,
    KeyMap::TURBO_3,
    KeyMap::SCREENSHOT,
    KeyMap::BT_CON,
    KeyMap::SWAP,
    KeyMap::M1,
    KeyMap::M2,
    KeyMap::M3,
    KeyMap::M4,
];

/// True when `target` can be selected as a remap target, macro trigger or gyro
/// button.
///
/// Rejects `KeyMap::MY_NULL` (227), unknown bits and composite masks.
#[must_use]
pub fn is_valid_key_target(target: KeyMap) -> bool {
    target.is_single_documented()
}

/// Special-feature bits the settings editor exposes, in display order.
pub const SPECIAL_FEATURES: [SpecialFeature; 13] = [
    SpecialFeature::LX_FLIP,
    SpecialFeature::LY_FLIP,
    SpecialFeature::RX_FLIP,
    SpecialFeature::RY_FLIP,
    SpecialFeature::STICK_SWAP,
    SpecialFeature::TRIGGER_SWAP,
    SpecialFeature::DPAD_SWAP,
    SpecialFeature::RS_TRIGGER_FLIP,
    SpecialFeature::MOTION_SENSITIVITY,
    SpecialFeature::ZERO_DEADZONE,
    SpecialFeature::FOUR_WAY,
    SpecialFeature::TRIGGER_VIBRATION,
    SpecialFeature::VIBRATION,
];

/// Why a draft cannot be turned into a write request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    /// A profile name exceeds 16 UTF-16 code units.
    NameTooLong {
        /// Profile slot; `None` for a macro name.
        slot: Option<Slot>,
        /// Units the name needs.
        units: usize,
    },
    /// A profile or macro name contains a NUL code unit.
    NameHasNul {
        /// Profile slot; `None` for the macro name.
        slot: Option<Slot>,
    },
    /// The draft does not change any byte.
    NoChanges,
    /// The selected macro target has no steps, so creating it is not possible.
    EmptyTarget {
        /// Macro slot.
        slot: Slot,
        /// Macro index.
        index: MacroIndex,
    },
    /// A macro must keep at least one step.
    EmptySteps,
    /// A macro may hold at most 200 steps.
    TooManySteps {
        /// Requested step count.
        count: usize,
    },
    /// A macro step has a zero duration.
    ZeroDuration {
        /// Zero-based step index.
        index: usize,
    },
    /// A remap target is not a documented single key.
    InvalidKeyTarget {
        /// Profile slot.
        slot: Slot,
        /// Physical button.
        button: ButtonIndex,
    },
    /// A macro trigger is not a documented single key.
    InvalidTrigger {
        /// Unsupported trigger value.
        value: KeyMap,
    },
    /// The local plan failed core-side validation.
    InvalidPlan(String),
}

impl std::fmt::Display for DraftError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NameTooLong { slot, units } => match slot {
                Some(slot) => write!(
                    formatter,
                    "profile {} name needs {units} UTF-16 units; maximum is {MAX_NAME_UNITS}",
                    slot.number()
                ),
                None => write!(
                    formatter,
                    "macro name needs {units} UTF-16 units; maximum is {MAX_NAME_UNITS}"
                ),
            },
            Self::NameHasNul { slot } => match slot {
                Some(slot) => write!(formatter, "profile {} name contains NUL", slot.number()),
                None => formatter.write_str("macro name contains NUL"),
            },
            Self::NoChanges => formatter.write_str("the draft changes nothing"),
            Self::EmptyTarget { slot, index } => write!(
                formatter,
                "slot {} macro {} is empty; creating an empty macro is not supported",
                slot.number(),
                index.index() + 1
            ),
            Self::EmptySteps => formatter.write_str("a macro needs at least one step"),
            Self::TooManySteps { count } => write!(
                formatter,
                "macro has {count} steps; maximum is {MAX_MACRO_STEPS}"
            ),
            Self::ZeroDuration { index } => {
                write!(formatter, "macro step {} has a zero duration", index + 1)
            }
            Self::InvalidKeyTarget { slot, button } => write!(
                formatter,
                "profile {} button {} has an unsupported remap target",
                slot.number(),
                button.label()
            ),
            Self::InvalidTrigger { value } => {
                write!(
                    formatter,
                    "unsupported macro trigger 0x{:08X}",
                    value.bits()
                )
            }
            Self::InvalidPlan(detail) => write!(formatter, "{detail}"),
        }
    }
}

impl std::error::Error for DraftError {}

/// Validate a profile name against the 16-unit UTF-16BE field.
pub fn validate_name(name: &str, slot: Slot) -> Result<(), DraftError> {
    if name.encode_utf16().any(|unit| unit == 0) {
        return Err(DraftError::NameHasNul { slot: Some(slot) });
    }
    let units = name.encode_utf16().count();
    if units > MAX_NAME_UNITS {
        return Err(DraftError::NameTooLong {
            slot: Some(slot),
            units,
        });
    }
    Ok(())
}

fn validate_macro_name(name: &str) -> Result<(), DraftError> {
    if name.encode_utf16().any(|unit| unit == 0) {
        return Err(DraftError::NameHasNul { slot: None });
    }
    let units = name.encode_utf16().count();
    if units > MAX_NAME_UNITS {
        return Err(DraftError::NameTooLong { slot: None, units });
    }
    Ok(())
}

/// One editable profile's 22-entry button remapping record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemapDraft {
    /// Remapping enabled (`1`) or disabled (`0`).
    pub enabled: bool,
    /// True once the user toggled the flag in this draft.
    ///
    /// An untouched flag is never rewritten, so an undocumented stored value
    /// (anything other than `1`) cannot be silently normalized by an unrelated
    /// edit.
    pub flag_touched: bool,
    /// Mapping target per [`ButtonIndex`].
    pub keys: [KeyMap; BUTTON_COUNT],
}

impl RemapDraft {
    /// Build a draft from the configuration read from the device.
    #[must_use]
    pub fn from_config(config: &Bt2Config, slot: Slot) -> Self {
        Self {
            enabled: map_key_flag(config, slot) == 1,
            flag_touched: false,
            keys: std::array::from_fn(|index| {
                ButtonIndex::from_index(index)
                    .map_or(KeyMap::NONE, |button| button_mapping(config, slot, button))
            }),
        }
    }

    /// True when the stored value is something the combo box can select.
    ///
    /// An unsupported stored value is still displayed; it simply is not offered
    /// as a choice.
    #[must_use]
    pub fn is_selectable(&self, button: ButtonIndex) -> bool {
        is_valid_key_target(self.keys[button.index()])
    }

    /// Typed edits for the fields that differ from `config`.
    #[must_use]
    pub fn edits(&self, config: &Bt2Config, slot: Slot) -> Vec<ConfigEdit> {
        let mut edits = Vec::new();
        let target_flag = u32::from(self.enabled);
        if self.flag_touched && map_key_flag(config, slot) != target_flag {
            edits.push(ConfigEdit::MapKeyFlag {
                slot,
                flag: target_flag,
            });
        }
        for button in ButtonIndex::ALL {
            let current = button_mapping(config, slot, button);
            let target = self.keys[button.index()];
            if current != target && is_valid_key_target(target) {
                edits.push(ConfigEdit::ButtonRemap {
                    slot,
                    button,
                    to: target,
                });
            }
        }
        edits
    }

    /// Validate every mapping target.
    ///
    /// # Errors
    ///
    /// Returns [`DraftError::InvalidKeyTarget`] for an unsupported stored or
    /// edited target.
    pub fn validate(&self, slot: Slot) -> Result<(), DraftError> {
        for button in ButtonIndex::ALL {
            if !is_valid_key_target(self.keys[button.index()]) {
                return Err(DraftError::InvalidKeyTarget { slot, button });
            }
        }
        Ok(())
    }
}

/// One group of settings that differs from the device configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Profile name.
    ProfileName(Slot),
    /// `gamepad_mode`.
    GamepadMode,
    /// Active profile (`cur_slot`).
    ActiveProfile,
    /// Rumble record.
    Rumble(Slot),
    /// Stick dead-zone record.
    Sticks(Slot),
    /// Trigger dead-zone record.
    Triggers(Slot),
    /// Special-feature bit mask.
    Special(Slot),
    /// Six-axis / gyro record.
    Gyro(Slot),
    /// Any LED theme record.
    LedTheme(Slot),
    /// Remap record flag.
    RemapFlag(Slot),
    /// One remapped button.
    RemapButton(Slot, ButtonIndex),
}

/// The complete editable configuration, one draft per profile plus the shared
/// header fields.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsDraft {
    /// Three profile names.
    pub profile_names: [String; 3],
    /// Stored `gamepad_mode`.
    ///
    /// `None` means the device stores an undocumented value; it is left
    /// untouched unless the user picks a documented mode.
    pub gamepad_mode: Option<GamepadMode>,
    /// Active profile (`None` = no profile, raw 4).
    pub cur_slot: Option<Slot>,
    /// Rumble records.
    pub rumble: [RumbleRecord; 3],
    /// Stick dead-zone records.
    pub joy: [JoyRecord; 3],
    /// Trigger dead-zone records.
    pub trigger: [TriggerRecord; 3],
    /// Special-feature records.
    pub special: [SpecialRecord; 3],
    /// Six-axis / gyro records.
    pub six_axis: [SixAxisRecord; 3],
    /// Dynamic / rainbow themes.
    pub dynamic: [DynamicTheme; 3],
    /// Hot-wheel themes.
    pub hotwheel: [HotWheelTheme; 3],
    /// Single-color themes.
    pub single: [SingleTheme; 3],
    /// Button remapping drafts.
    pub remap: [RemapDraft; 3],
}

impl SettingsDraft {
    /// Build every draft from the configuration read from the device.
    #[must_use]
    pub fn from_config(config: &Bt2Config) -> Self {
        Self {
            profile_names: [
                config.profile_name(Slot::S1),
                config.profile_name(Slot::S2),
                config.profile_name(Slot::S3),
            ],
            gamepad_mode: config.gamepad_mode(),
            cur_slot: config.cur_slot(),
            rumble: [
                config.rumble(Slot::S1),
                config.rumble(Slot::S2),
                config.rumble(Slot::S3),
            ],
            joy: [
                config.joy(Slot::S1),
                config.joy(Slot::S2),
                config.joy(Slot::S3),
            ],
            trigger: [
                config.trigger(Slot::S1),
                config.trigger(Slot::S2),
                config.trigger(Slot::S3),
            ],
            special: [
                config.special(Slot::S1),
                config.special(Slot::S2),
                config.special(Slot::S3),
            ],
            six_axis: [
                config.six_axis(Slot::S1),
                config.six_axis(Slot::S2),
                config.six_axis(Slot::S3),
            ],
            dynamic: [
                config.dynamic_theme(Slot::S1),
                config.dynamic_theme(Slot::S2),
                config.dynamic_theme(Slot::S3),
            ],
            hotwheel: [
                config.hotwheel_theme(Slot::S1),
                config.hotwheel_theme(Slot::S2),
                config.hotwheel_theme(Slot::S3),
            ],
            single: [
                config.single_theme(Slot::S1),
                config.single_theme(Slot::S2),
                config.single_theme(Slot::S3),
            ],
            remap: [
                RemapDraft::from_config(config, Slot::S1),
                RemapDraft::from_config(config, Slot::S2),
                RemapDraft::from_config(config, Slot::S3),
            ],
        }
    }

    /// Replace one single-color theme with one color for all 24 LEDs.
    pub fn set_single_color(&mut self, slot: Slot, color: eightbitdo_protocol::config_bt2::Argb) {
        self.single[slot.index()].colors = [color; SINGLE_LED_COUNT];
    }

    /// Typed edits for every field that differs from `config`.
    ///
    /// The order is stable: header fields first, then profile-major records,
    /// then remapping. A later edit for the same field wins, and no field is
    /// emitted twice.
    #[must_use]
    pub fn edits(&self, config: &Bt2Config) -> Vec<ConfigEdit> {
        let mut edits = Vec::new();
        for slot in Slot::ALL {
            if self.profile_names[slot.index()] != config.profile_name(slot) {
                edits.push(ConfigEdit::ProfileName {
                    slot,
                    name: self.profile_names[slot.index()].clone(),
                });
            }
        }
        if let Some(mode) = self.gamepad_mode {
            if config.gamepad_mode() != Some(mode) {
                edits.push(ConfigEdit::GamepadMode(mode));
            }
        }
        if config.cur_slot() != self.cur_slot {
            edits.push(ConfigEdit::CurSlot(self.cur_slot));
        }
        for slot in Slot::ALL {
            if config.rumble(slot) != self.rumble[slot.index()] {
                edits.push(ConfigEdit::Rumble {
                    slot,
                    record: self.rumble[slot.index()],
                });
            }
            if config.joy(slot) != self.joy[slot.index()] {
                edits.push(ConfigEdit::Joy {
                    slot,
                    record: self.joy[slot.index()],
                });
            }
            if config.trigger(slot) != self.trigger[slot.index()] {
                edits.push(ConfigEdit::Trigger {
                    slot,
                    record: self.trigger[slot.index()],
                });
            }
            if config.special(slot) != self.special[slot.index()] {
                edits.push(ConfigEdit::Special {
                    slot,
                    record: self.special[slot.index()],
                });
            }
            if config.six_axis(slot) != self.six_axis[slot.index()] {
                edits.push(ConfigEdit::SixAxis {
                    slot,
                    record: self.six_axis[slot.index()],
                });
            }
            if config.dynamic_theme(slot) != self.dynamic[slot.index()] {
                edits.push(ConfigEdit::DynamicTheme {
                    slot,
                    theme: self.dynamic[slot.index()],
                });
            }
            if config.hotwheel_theme(slot) != self.hotwheel[slot.index()] {
                edits.push(ConfigEdit::HotWheelTheme {
                    slot,
                    theme: self.hotwheel[slot.index()],
                });
            }
            if config.single_theme(slot) != self.single[slot.index()] {
                edits.push(ConfigEdit::SingleTheme {
                    slot,
                    theme: self.single[slot.index()],
                });
            }
            edits.extend(self.remap[slot.index()].edits(config, slot));
        }
        edits
    }

    /// Group-level changes for the confirmation dialog.
    #[must_use]
    pub fn changes(&self, config: &Bt2Config) -> Vec<ChangeKind> {
        let mut changes = Vec::new();
        for slot in Slot::ALL {
            if self.profile_names[slot.index()] != config.profile_name(slot) {
                changes.push(ChangeKind::ProfileName(slot));
            }
        }
        if self
            .gamepad_mode
            .is_some_and(|mode| config.gamepad_mode() != Some(mode))
        {
            changes.push(ChangeKind::GamepadMode);
        }
        if config.cur_slot() != self.cur_slot {
            changes.push(ChangeKind::ActiveProfile);
        }
        for slot in Slot::ALL {
            if config.rumble(slot) != self.rumble[slot.index()] {
                changes.push(ChangeKind::Rumble(slot));
            }
            if config.joy(slot) != self.joy[slot.index()] {
                changes.push(ChangeKind::Sticks(slot));
            }
            if config.trigger(slot) != self.trigger[slot.index()] {
                changes.push(ChangeKind::Triggers(slot));
            }
            if config.special(slot) != self.special[slot.index()] {
                changes.push(ChangeKind::Special(slot));
            }
            if config.six_axis(slot) != self.six_axis[slot.index()] {
                changes.push(ChangeKind::Gyro(slot));
            }
            if config.dynamic_theme(slot) != self.dynamic[slot.index()]
                || config.hotwheel_theme(slot) != self.hotwheel[slot.index()]
                || config.single_theme(slot) != self.single[slot.index()]
            {
                changes.push(ChangeKind::LedTheme(slot));
            }
            let remap = &self.remap[slot.index()];
            if remap.flag_touched && map_key_flag(config, slot) != u32::from(remap.enabled) {
                changes.push(ChangeKind::RemapFlag(slot));
            }
            for button in ButtonIndex::ALL {
                if button_mapping(config, slot, button) != remap.keys[button.index()] {
                    changes.push(ChangeKind::RemapButton(slot, button));
                }
            }
        }
        changes
    }

    /// Validate every name and remap target.
    ///
    /// # Errors
    ///
    /// Returns the first [`DraftError`].
    pub fn validate(&self) -> Result<(), DraftError> {
        for slot in Slot::ALL {
            validate_name(&self.profile_names[slot.index()], slot)?;
            self.remap[slot.index()].validate(slot)?;
        }
        Ok(())
    }

    /// Build the plan for a confirmed write.
    ///
    /// # Errors
    ///
    /// Returns [`DraftError`] when a name is invalid or nothing changes.
    pub fn plan(&self, snapshot: &ConfigSnapshot) -> Result<ConfigWritePlan, DraftError> {
        self.validate()?;
        let edits = self.edits(&snapshot.config);
        if edits.is_empty() {
            return Err(DraftError::NoChanges);
        }
        let plan = ConfigWritePlan::from_edits(edits);
        // Surface a core-side validation error (for example an invalid name)
        // before the confirmation dialog opens.
        plan.prepare(snapshot)
            .map_err(|error| DraftError::InvalidPlan(error.to_string()))?;
        Ok(plan)
    }

    /// Build a pending write for the confirmation dialog.
    ///
    /// # Errors
    ///
    /// Returns [`DraftError`] when the draft cannot be written.
    pub fn pending_write(
        &self,
        snapshot: &ConfigSnapshot,
        focus: Slot,
    ) -> Result<PendingWrite, DraftError> {
        let plan = self.plan(snapshot)?;
        Ok(PendingWrite {
            target: WriteTarget::Config {
                changes: self.changes(&snapshot.config),
                focus,
            },
            plan: PendingPlan::Config(plan),
        })
    }
}

/// One editable macro.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroDraft {
    /// Steps written to flash, in playback order.
    pub steps: Vec<MacroStep>,
    /// Stored name.
    pub name: String,
    /// Trigger button mask.
    pub key_map: KeyMap,
    /// Loop count (`0` = hold / infinite).
    pub cycles_num: u32,
    /// Delay between loops in milliseconds.
    pub interval_ms: u32,
}

impl MacroDraft {
    /// Build a draft from a macro read from the device.
    #[must_use]
    pub fn from_entry(entry: &MacroEntry) -> Self {
        let record = entry.record();
        Self {
            steps: entry.steps().to_vec(),
            name: record.name(),
            key_map: record.key_map,
            cycles_num: record.cycles_num,
            interval_ms: record.interval_ms,
        }
    }

    /// Total duration of the step list in milliseconds.
    #[must_use]
    pub fn total_ms(&self) -> u32 {
        self.steps.iter().map(|step| u32::from(step.ms_times)).sum()
    }

    /// Header fields that differ from the entry read from the device.
    #[must_use]
    pub fn changes(&self, entry: &MacroEntry) -> Vec<MacroChange> {
        let record = entry.record();
        let mut changes = Vec::new();
        if self.steps != entry.steps() {
            changes.push(MacroChange::Steps);
        }
        if self.name != record.name() {
            changes.push(MacroChange::Name);
        }
        if self.key_map != record.key_map {
            changes.push(MacroChange::Trigger);
        }
        if self.cycles_num != record.cycles_num {
            changes.push(MacroChange::Cycles);
        }
        if self.interval_ms != record.interval_ms {
            changes.push(MacroChange::Interval);
        }
        changes
    }

    /// Validate the draft against a macro read from the device.
    ///
    /// # Errors
    ///
    /// Returns [`DraftError::EmptyTarget`] when the device macro has no steps
    /// (creating a new macro is not supported), and the step / name errors for
    /// an invalid draft.
    pub fn validate(&self, entry: &MacroEntry) -> Result<(), DraftError> {
        let slot = entry.slot();
        let index = entry.index();
        if is_skipped_steps(entry.record().max_steps) {
            return Err(DraftError::EmptyTarget { slot, index });
        }
        if self.steps.is_empty() {
            return Err(DraftError::EmptySteps);
        }
        if self.steps.len() > MAX_MACRO_STEPS {
            return Err(DraftError::TooManySteps {
                count: self.steps.len(),
            });
        }
        if let Some(index) = self.steps.iter().position(|step| step.ms_times == 0) {
            return Err(DraftError::ZeroDuration { index });
        }
        validate_macro_name(&self.name)?;
        if !is_valid_key_target(self.key_map) {
            return Err(DraftError::InvalidTrigger {
                value: self.key_map,
            });
        }
        Ok(())
    }

    /// Build the macro write plan for a confirmed write.
    ///
    /// Header fields are only overridden when they differ from the device entry,
    /// so an untouched lossily-decoded name keeps its original bytes. The plan
    /// always uses the absolute `total_len` layout and the BLE Save command.
    ///
    /// # Errors
    ///
    /// Returns the [`DraftError`] from [`MacroDraft::validate`].
    pub fn plan(&self, entry: &MacroEntry) -> Result<MacroWritePlan, DraftError> {
        self.validate(entry)?;
        let record = entry.record();
        let mut plan = MacroWritePlan::new(self.steps.clone());
        if self.name != record.name() {
            plan.name = Some(self.name.clone());
        }
        if self.key_map != record.key_map {
            plan.key_map = Some(self.key_map);
        }
        if self.cycles_num != record.cycles_num {
            plan.cycles_num = Some(self.cycles_num);
        }
        if self.interval_ms != record.interval_ms {
            plan.interval_ms = Some(self.interval_ms);
        }
        Ok(plan)
    }
}

/// One macro header field that differs from the device entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroChange {
    /// Step list changed.
    Steps,
    /// Name changed.
    Name,
    /// Trigger changed.
    Trigger,
    /// Loop count changed.
    Cycles,
    /// Inter-loop delay changed.
    Interval,
}

/// Live-input macro recorder.
///
/// One report per 20 ms tick becomes one step. Consecutive identical reports are
/// merged by extending the previous step, which keeps the 200-step budget for
/// actual changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MacroRecorder {
    steps: Vec<MacroStep>,
}

/// What one recorded report did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    /// A new step was appended.
    Added,
    /// The previous step's duration was extended.
    Extended,
    /// The recorder is full (200 steps).
    Full,
}

impl MacroRecorder {
    /// Empty recorder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Recorded steps.
    #[must_use]
    pub fn steps(&self) -> &[MacroStep] {
        &self.steps
    }

    /// True when no report was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Number of recorded steps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// True when the 200-step budget is used up.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.steps.len() >= MAX_MACRO_STEPS
    }

    /// Drop every recorded step.
    pub fn clear(&mut self) {
        self.steps.clear();
    }

    /// Record one live input report as a 20 ms step.
    pub fn record(&mut self, input: &InputView) -> RecordOutcome {
        let step = step_from_input(input);
        if let Some(last) = self.steps.last_mut() {
            let same = last.keys == step.keys
                && last.rt_value == step.rt_value
                && last.lt_value == step.lt_value
                && last.left_x == step.left_x
                && last.left_y == step.left_y
                && last.right_x == step.right_x
                && last.right_y == step.right_y;
            if same {
                if let Some(extended) = last.ms_times.checked_add(RECORD_STEP_MS) {
                    last.ms_times = extended;
                    return RecordOutcome::Extended;
                }
            }
        }
        if self.steps.len() >= MAX_MACRO_STEPS {
            return RecordOutcome::Full;
        }
        self.steps.push(step);
        RecordOutcome::Added
    }
}

/// Convert a live input report into one 20 ms macro step.
///
/// Only the low 16 bits of the button mask can be stored in a macro step; the
/// higher bits (paddles, Turbo, ...) are dropped by the wire format.
#[must_use]
pub fn step_from_input(input: &InputView) -> MacroStep {
    MacroStep {
        ms_times: RECORD_STEP_MS,
        keys: (input.keys.bits() & 0xFFFF) as u16,
        rt_value: input.trigger_r,
        lt_value: input.trigger_l,
        left_x: axis_from_normalized(input.left_stick.0),
        left_y: axis_from_normalized(input.left_stick.1),
        right_x: axis_from_normalized(input.right_stick.0),
        right_y: axis_from_normalized(input.right_stick.1),
    }
}

fn axis_from_normalized(value: i16) -> u8 {
    (value.saturating_add(i16::from(AXIS_NEUTRAL))).clamp(0, 255) as u8
}

/// Macro editor state: the selected target, loaded draft, recorder and the
/// selected step row.
#[derive(Debug, Clone)]
pub struct MacroEditor {
    /// Selected profile.
    pub slot: Slot,
    /// Selected macro index.
    pub index: MacroIndex,
    /// Loaded macro draft, when a macro has been read.
    pub draft: Option<MacroDraft>,
    /// The macro read from the device, used as the write baseline.
    pub original: Option<MacroEntry>,
    /// Live-input recorder.
    pub recorder: MacroRecorder,
    /// True while live reports are being recorded.
    pub recording: bool,
    /// Selected step row.
    pub selected_step: Option<usize>,
    /// Duration inserted by the "wait" action.
    pub wait_ms: u16,
}

impl Default for MacroEditor {
    fn default() -> Self {
        Self {
            slot: Slot::S1,
            index: MacroIndex::M1,
            draft: None,
            original: None,
            recorder: MacroRecorder::new(),
            recording: false,
            selected_step: None,
            wait_ms: DEFAULT_WAIT_MS,
        }
    }
}

impl MacroEditor {
    /// Select a profile / macro target, dropping a draft for another target.
    pub fn select(&mut self, slot: Slot, index: MacroIndex) {
        if self.slot != slot || self.index != index {
            self.slot = slot;
            self.index = index;
            self.draft = None;
            self.original = None;
            self.selected_step = None;
            self.recording = false;
            self.recorder.clear();
        }
    }

    /// Replace the loaded macro with a freshly read entry.
    pub fn load(&mut self, entry: MacroEntry) {
        self.slot = entry.slot();
        self.index = entry.index();
        self.draft = Some(MacroDraft::from_entry(&entry));
        self.original = Some(entry);
        self.selected_step = None;
        self.recording = false;
        self.recorder.clear();
    }

    /// True when a draft and its device baseline are both present.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.draft.is_some() && self.original.is_some()
    }

    /// True when the loaded target macro is non-empty.
    #[must_use]
    pub fn target_has_steps(&self) -> bool {
        self.original
            .as_ref()
            .is_some_and(|entry| !is_skipped_steps(entry.record().max_steps))
    }

    /// Start recording and clear the previous take.
    ///
    /// The current step list is replaced only when the user stops recording and
    /// at least one report was recorded.
    pub fn begin_recording(&mut self) {
        if !self.is_loaded() {
            return;
        }
        self.recording = true;
        self.recorder.clear();
    }

    /// Feed one live input report to the recorder.
    pub fn record(&mut self, input: &InputView) {
        if !self.recording {
            return;
        }
        let outcome = self.recorder.record(input);
        if let Some(draft) = self.draft.as_mut() {
            if !self.recorder.is_empty() {
                draft.steps = self.recorder.steps().to_vec();
            }
        }
        if outcome == RecordOutcome::Full {
            self.stop_recording();
        }
    }

    /// Stop recording and keep the take when it is non-empty.
    pub fn stop_recording(&mut self) {
        self.recording = false;
        if self.recorder.is_empty() {
            return;
        }
        if let Some(draft) = self.draft.as_mut() {
            draft.steps = self.recorder.steps().to_vec();
            self.selected_step = Some(draft.steps.len() - 1);
        }
    }

    /// Delete the selected step. The last remaining step cannot be deleted.
    pub fn delete_selected(&mut self) -> bool {
        let Some(step) = self.selected_step else {
            return false;
        };
        let Some(draft) = self.draft.as_mut() else {
            return false;
        };
        if draft.steps.len() <= 1 || step >= draft.steps.len() {
            return false;
        }
        draft.steps.remove(step);
        self.selected_step = if draft.steps.is_empty() {
            None
        } else {
            Some(step.min(draft.steps.len() - 1))
        };
        true
    }

    /// Insert a neutral wait step after the selected step (or at the end).
    pub fn insert_wait(&mut self) -> bool {
        let Some(draft) = self.draft.as_mut() else {
            return false;
        };
        if draft.steps.len() >= MAX_MACRO_STEPS {
            return false;
        }
        let wait = MacroStep::neutral(self.wait_ms.max(1));
        let position = self
            .selected_step
            .map_or(draft.steps.len(), |step| (step + 1).min(draft.steps.len()));
        draft.steps.insert(position, wait);
        self.selected_step = Some(position);
        true
    }

    /// Borrow the selected step for editing.
    pub fn selected_step_mut(&mut self) -> Option<&mut MacroStep> {
        let index = self.selected_step?;
        self.draft
            .as_mut()
            .and_then(|draft| draft.steps.get_mut(index))
    }

    /// Build a pending write for the confirmation dialog.
    ///
    /// # Errors
    ///
    /// Returns [`DraftError`] when nothing is loaded or the draft is invalid.
    pub fn pending_write(&self) -> Result<PendingWrite, DraftError> {
        let (Some(draft), Some(original)) = (self.draft.as_ref(), self.original.as_ref()) else {
            return Err(DraftError::EmptySteps);
        };
        let plan = draft.plan(original)?;
        let changes = draft.changes(original);
        if changes.is_empty() {
            return Err(DraftError::NoChanges);
        }
        Ok(PendingWrite {
            target: WriteTarget::Macro {
                slot: self.slot,
                index: self.index,
                steps: draft.steps.len(),
                total_ms: draft.total_ms(),
                name: draft.name.clone(),
                trigger: draft.key_map,
                cycles: draft.cycles_num,
                interval_ms: draft.interval_ms,
                changes,
            },
            plan: PendingPlan::Macro(plan),
        })
    }
}

/// The payload of a confirmed write.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingPlan {
    /// Full 2768-byte configuration write.
    Config(ConfigWritePlan),
    /// One macro write into an existing non-empty macro.
    Macro(MacroWritePlan),
}

/// What the confirmation dialog explains before sending the write.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteTarget {
    /// Configuration write.
    Config {
        /// Group-level changes.
        changes: Vec<ChangeKind>,
        /// Profile the editor was focused on.
        focus: Slot,
    },
    /// Macro write.
    Macro {
        /// Profile.
        slot: Slot,
        /// Macro index.
        index: MacroIndex,
        /// New step count.
        steps: usize,
        /// Total playback duration.
        total_ms: u32,
        /// New name.
        name: String,
        /// New trigger.
        trigger: KeyMap,
        /// Loop count.
        cycles: u32,
        /// Inter-loop delay.
        interval_ms: u32,
        /// Changed header fields.
        changes: Vec<MacroChange>,
    },
}

/// A validated write waiting for explicit confirmation.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingWrite {
    /// What the user is about to write.
    pub target: WriteTarget,
    /// Prepared core plan.
    pub plan: PendingPlan,
}

impl PendingWrite {
    /// True for a configuration write.
    #[must_use]
    pub fn is_config(&self) -> bool {
        matches!(self.plan, PendingPlan::Config(_))
    }
}

#[cfg(test)]
mod tests {
    use eightbitdo_protocol::config_bt2::{MacroRecord, MacroSlot};
    use eightbitdo_protocol::keymap::KeyMap;
    use eightbitdo_protocol::macro_data::MacroStep;

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

    fn entry(
        config: &Bt2Config,
        slot: Slot,
        index: MacroIndex,
        steps: Vec<MacroStep>,
    ) -> MacroEntry {
        let mut record = config.macro_record(slot, index);
        record.max_steps = steps.len() as u16;
        let raw = eightbitdo_protocol::macro_data::encode_macro_steps(&steps)
            .unwrap()
            .into_iter()
            .take(steps.len() * 10)
            .collect();
        MacroEntry::new(slot, index, record, raw).unwrap()
    }

    #[test]
    fn remap_targets_reject_null_unknown_and_composites() {
        assert!(is_valid_key_target(KeyMap::A));
        assert!(is_valid_key_target(KeyMap::M1));
        assert!(is_valid_key_target(KeyMap::NONE));
        assert!(!is_valid_key_target(KeyMap::MY_NULL));
        assert!(!is_valid_key_target(KeyMap::from_bits(1 << 31)));
        assert!(!is_valid_key_target(KeyMap::A | KeyMap::B));
        assert!(!is_valid_key_target(KeyMap::from_bits(0x00FF_FFFF)));
    }

    #[test]
    fn remap_draft_emits_only_changed_entries_and_binary_flag() {
        let config = Bt2Config::default();
        let mut draft = RemapDraft::from_config(&config, Slot::S1);
        assert!(draft.edits(&config, Slot::S1).is_empty());

        draft.keys[ButtonIndex::A.index()] = KeyMap::B;
        draft.enabled = true;
        draft.flag_touched = true;
        let edits = draft.edits(&config, Slot::S1);
        assert_eq!(
            edits,
            vec![
                ConfigEdit::MapKeyFlag {
                    slot: Slot::S1,
                    flag: 1
                },
                ConfigEdit::ButtonRemap {
                    slot: Slot::S1,
                    button: ButtonIndex::A,
                    to: KeyMap::B
                },
            ],
            "only the changed button and the flag are emitted"
        );

        draft.keys[ButtonIndex::B.index()] = KeyMap::MY_NULL;
        assert_eq!(
            draft.validate(Slot::S1),
            Err(DraftError::InvalidKeyTarget {
                slot: Slot::S1,
                button: ButtonIndex::B
            })
        );
    }

    #[test]
    fn an_untouched_remap_flag_is_never_rewritten() {
        let mut config = Bt2Config::default();
        let mut record = config.map_key(Slot::S1);
        record.flag = 2;
        config.set_map_key(Slot::S1, &record);

        let mut draft = RemapDraft::from_config(&config, Slot::S1);
        assert!(!draft.enabled);
        draft.keys[ButtonIndex::A.index()] = KeyMap::B;
        assert!(
            draft
                .edits(&config, Slot::S1)
                .iter()
                .all(|edit| !matches!(edit, ConfigEdit::MapKeyFlag { .. })),
            "an unrelated edit must not normalize an undocumented flag"
        );

        draft.enabled = true;
        draft.flag_touched = true;
        assert!(draft
            .edits(&config, Slot::S1)
            .iter()
            .any(|edit| matches!(edit, ConfigEdit::MapKeyFlag { flag: 1, .. })));
    }

    #[test]
    fn settings_draft_round_trips_without_edits() {
        let mut config = Bt2Config::default();
        config.set_profile_name(Slot::S1, "Alpha").unwrap();
        config.set_cur_slot(Some(Slot::S2));
        config.set_gamepad_mode(GamepadMode::XInput);
        let snapshot = snapshot(config);
        let draft = SettingsDraft::from_config(&snapshot.config);

        assert!(draft.edits(&snapshot.config).is_empty());
        assert!(draft.changes(&snapshot.config).is_empty());
        assert_eq!(
            draft.plan(&snapshot),
            Err(DraftError::NoChanges),
            "a no-op plan is refused before the dialog"
        );
    }

    #[test]
    fn settings_draft_reports_grouped_changes_and_plan() {
        let mut config = Bt2Config::default();
        config.set_profile_name(Slot::S1, "Alpha").unwrap();
        let snapshot = snapshot(config);
        let mut draft = SettingsDraft::from_config(&snapshot.config);
        draft.profile_names[0] = "Bravo".to_string();
        draft.rumble[0].flag = 1;
        draft.remap[0].keys[ButtonIndex::X.index()] = KeyMap::Y;
        draft.gamepad_mode = Some(GamepadMode::Mac);

        let changes = draft.changes(&snapshot.config);
        assert!(changes.contains(&ChangeKind::ProfileName(Slot::S1)));
        assert!(changes.contains(&ChangeKind::Rumble(Slot::S1)));
        assert!(changes.contains(&ChangeKind::RemapButton(Slot::S1, ButtonIndex::X)));
        assert!(changes.contains(&ChangeKind::GamepadMode));

        let plan = draft.plan(&snapshot).unwrap();
        let pending = draft.pending_write(&snapshot, Slot::S1).unwrap();
        assert!(pending.is_config());
        let regions = plan.prepare(&snapshot).unwrap().changed().to_vec();
        for offset in [12usize, 16, 116, 236] {
            assert!(
                regions
                    .iter()
                    .any(|region| region.range().contains(&offset)),
                "changed regions must cover config offset 0x{offset:X}: {regions:?}"
            );
        }
    }

    #[test]
    fn profile_names_are_limited_to_sixteen_utf16_units() {
        let slot = Slot::S1;
        assert!(validate_name("1234567890123456", slot).is_ok());
        assert!(matches!(
            validate_name("12345678901234567", slot),
            Err(DraftError::NameTooLong { .. })
        ));
        assert!(matches!(
            validate_name("123456789012345\u{1F600}", slot),
            Err(DraftError::NameTooLong { units: 17, .. })
        ));
        assert!(matches!(
            validate_name("a\u{0}b", slot),
            Err(DraftError::NameHasNul { .. })
        ));
    }

    #[test]
    fn recorder_coalesces_identical_reports_and_caps_at_200_steps() {
        let mut recorder = MacroRecorder::new();
        let neutral = InputView::default();
        assert_eq!(recorder.record(&neutral), RecordOutcome::Added);
        assert_eq!(recorder.record(&neutral), RecordOutcome::Extended);
        assert_eq!(recorder.len(), 1);
        assert_eq!(recorder.steps()[0].ms_times, 2 * RECORD_STEP_MS);

        let pressed = InputView {
            keys: KeyMap::A | KeyMap::START,
            left_stick: (128, 0),
            right_stick: (0, -128),
            trigger_l: 10,
            trigger_r: 20,
            ..InputView::default()
        };
        assert_eq!(recorder.record(&pressed), RecordOutcome::Added);
        assert_eq!(recorder.len(), 2);
        let step = recorder.steps()[1];
        assert_eq!(step.keys, (KeyMap::A | KeyMap::START).bits() as u16);
        assert_eq!(step.left_x, 255);
        assert_eq!(step.left_y, AXIS_NEUTRAL);
        assert_eq!(step.right_y, 0);
        assert_eq!(step.lt_value, 10);
        assert_eq!(step.rt_value, 20);
    }

    #[test]
    fn a_full_recorder_stops_instead_of_overwriting() {
        let mut recorder = MacroRecorder::new();
        for step in 0..MAX_MACRO_STEPS {
            let input = InputView {
                keys: KeyMap::from_bits(step as u32 + 1),
                ..InputView::default()
            };
            assert_eq!(recorder.record(&input), RecordOutcome::Added);
        }
        assert!(recorder.is_full());
        let input = InputView {
            keys: KeyMap::from_bits(0xFFFF),
            ..InputView::default()
        };
        assert_eq!(recorder.record(&input), RecordOutcome::Full);
        assert_eq!(recorder.len(), MAX_MACRO_STEPS);
    }

    #[test]
    fn macro_editor_loads_deletes_and_inserts_waits() {
        let mut config = Bt2Config::default();
        let steps = vec![MacroStep::neutral(20), MacroStep::neutral(20)];
        let mut header = MacroSlot::default();
        header.records[0].max_steps = 2;
        config.set_macro_slot(Slot::S1, &header);
        let entry = entry(&config, Slot::S1, MacroIndex::M1, steps);

        let mut editor = MacroEditor::default();
        editor.load(entry);
        assert!(editor.is_loaded());
        assert!(editor.target_has_steps());
        assert_eq!(editor.draft.as_ref().unwrap().steps.len(), 2);

        editor.selected_step = Some(0);
        editor.wait_ms = 100;
        assert!(editor.insert_wait());
        assert_eq!(editor.draft.as_ref().unwrap().steps.len(), 3);
        assert_eq!(editor.draft.as_ref().unwrap().steps[1].ms_times, 100);

        assert!(editor.delete_selected());
        assert_eq!(editor.draft.as_ref().unwrap().steps.len(), 2);

        assert!(editor.delete_selected(), "one of two steps can be deleted");
        assert_eq!(editor.draft.as_ref().unwrap().steps.len(), 1);
        assert!(!editor.delete_selected(), "the last step is kept");
    }

    #[test]
    fn macro_draft_refuses_empty_targets_and_empty_steps() {
        let config = Bt2Config::default();
        let empty = entry(&config, Slot::S1, MacroIndex::M1, Vec::new());
        let steps = vec![MacroStep::neutral(20)];
        let mut draft = MacroDraft::from_entry(&empty);
        draft.steps = steps.clone();
        assert_eq!(
            draft.validate(&empty),
            Err(DraftError::EmptyTarget {
                slot: Slot::S1,
                index: MacroIndex::M1
            })
        );

        let mut header = MacroSlot::default();
        header.records[0].max_steps = 1;
        let mut config = Bt2Config::default();
        config.set_macro_slot(Slot::S1, &header);
        let non_empty = entry(&config, Slot::S1, MacroIndex::M1, steps);
        let mut draft = MacroDraft::from_entry(&non_empty);
        draft.steps.clear();
        assert_eq!(draft.validate(&non_empty), Err(DraftError::EmptySteps));
    }

    #[test]
    fn macro_plan_only_overrides_changed_header_fields_and_uses_absolute_layout() {
        let mut header = MacroSlot::default();
        header.records[0] = MacroRecord {
            max_steps: 1,
            key_map: KeyMap::A,
            cycles_num: 2,
            interval_ms: 50,
            ..MacroRecord::default()
        };
        let mut config = Bt2Config::default();
        config.set_macro_slot(Slot::S1, &header);
        let entry = entry(
            &config,
            Slot::S1,
            MacroIndex::M1,
            vec![MacroStep::neutral(20)],
        );

        let draft = MacroDraft::from_entry(&entry);
        let plan = draft.plan(&entry).unwrap();
        assert!(plan.absolute_total_len, "absolute total_len is mandatory");
        assert_eq!(plan.name, None);
        assert_eq!(plan.key_map, None);
        assert_eq!(plan.cycles_num, None);
        assert_eq!(plan.interval_ms, None);

        let mut changed = MacroDraft::from_entry(&entry);
        changed.name = "Combo".to_string();
        changed.cycles_num = 3;
        let plan = changed.plan(&entry).unwrap();
        assert_eq!(plan.name.as_deref(), Some("Combo"));
        assert_eq!(plan.cycles_num, Some(3));
        assert_eq!(plan.key_map, None);
    }

    #[test]
    fn macro_draft_rejects_zero_duration_steps_and_too_many_steps() {
        let mut header = MacroSlot::default();
        header.records[0].max_steps = 1;
        let mut config = Bt2Config::default();
        config.set_macro_slot(Slot::S1, &header);
        let entry = entry(
            &config,
            Slot::S1,
            MacroIndex::M1,
            vec![MacroStep::neutral(20)],
        );
        let mut draft = MacroDraft::from_entry(&entry);
        draft.steps[0].ms_times = 0;
        assert_eq!(
            draft.validate(&entry),
            Err(DraftError::ZeroDuration { index: 0 })
        );

        let mut draft = MacroDraft::from_entry(&entry);
        draft.steps = vec![MacroStep::neutral(1); MAX_MACRO_STEPS + 1];
        assert!(matches!(
            draft.validate(&entry),
            Err(DraftError::TooManySteps { .. })
        ));
    }

    #[test]
    fn macro_pending_write_describes_the_change() {
        let mut header = MacroSlot::default();
        header.records[0].max_steps = 1;
        let mut config = Bt2Config::default();
        config.set_macro_slot(Slot::S1, &header);
        let entry = entry(
            &config,
            Slot::S1,
            MacroIndex::M1,
            vec![MacroStep::neutral(20)],
        );

        let mut editor = MacroEditor::default();
        editor.load(entry);
        assert_eq!(editor.pending_write(), Err(DraftError::NoChanges));

        editor.draft.as_mut().unwrap().steps[0].ms_times = 40;
        let pending = editor.pending_write().unwrap();
        match pending.target {
            WriteTarget::Macro {
                steps,
                total_ms,
                changes,
                ..
            } => {
                assert_eq!(steps, 1);
                assert_eq!(total_ms, 40);
                assert_eq!(changes, vec![MacroChange::Steps]);
            }
            WriteTarget::Config { .. } => panic!("expected a macro target"),
        }
        assert!(matches!(pending.plan, PendingPlan::Macro(_)));
    }

    #[test]
    fn unsupported_single_theme_edit_reaches_the_plan() {
        let snapshot = snapshot(Bt2Config::default());
        let mut draft = SettingsDraft::from_config(&snapshot.config);
        draft.set_single_color(
            Slot::S2,
            eightbitdo_protocol::config_bt2::Argb::from_rgba(0x10, 0x20, 0x30, 0xFF),
        );
        let edits = draft.edits(&snapshot.config);
        assert!(edits
            .iter()
            .any(|edit| matches!(edit, ConfigEdit::SingleTheme { slot: Slot::S2, .. })));
    }
}
