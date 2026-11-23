//! egui application: a read-only view over the background worker.
//!
//! Visual direction: 「燻しスレート — 実機の素材と、ひとつの光」. Surfaces are
//! stacked slate layers with hairlines and a top edge light; the LED color is
//! reserved for state and selection. The primary key is off-white, so the
//! brightest plane on screen is the next action.
//!
//! Layout: a 56 px top bar, a 264 px rail, a 40 px tab row with a full-width
//! hairline, and a content column capped at 700 px with a vertical hairline at
//! its right edge. Empty states are asymmetric: a ring on the left, the next
//! move on the right.

use std::collections::VecDeque;
use std::f32::consts::{FRAC_PI_3, TAU};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Frame, Id, Layout, Margin, Pos2, Rangef, Rect, RichText,
    Sense, Stroke, UiBuilder, Vec2,
};
use eightbitdo_core::ConfigSnapshot;
use eightbitdo_protocol::config_bt2::{Argb, MacroIndex, Slot, SpecialFeature};
use eightbitdo_protocol::keymap::{ButtonIndex, KeyMap};

use crate::backup::{default_backup_dir, BackupRef, BackupReport};
use crate::driver::{ConfigWriteReport, MacroWriteReport, WriteFailure, WritePhase};
use crate::i18n::{self, Key, Lang};
use crate::model::{
    format_rssi, format_u16_hex, format_u32_hex, ConfigView, DeviceInfoView, InputView,
    MacroSlotView, ScanEntry, VersionView, BUTTON_LABELS,
};
use crate::theme;
use crate::worker::{Command, Event, Notice, Worker};
use crate::write::{
    self, DraftError, MacroEditor, PendingPlan, PendingWrite, SettingsDraft, WriteTarget,
    GAMEPAD_MODES, KEY_TARGETS, MAX_CYCLES, MAX_INTERVAL_MS, SPECIAL_FEATURES,
};

const MAX_LOG_ENTRIES: usize = 500;

/// Fixed height of one log row; keeps the empty state from stealing height.
const LOG_ROW_HEIGHT: f32 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Disconnected,
    Scanning,
    Connecting,
    Connected,
}

impl ConnectionState {
    fn key(self) -> Key {
        match self {
            Self::Disconnected => Key::StatusDisconnected,
            Self::Scanning => Key::StatusScanning,
            Self::Connecting => Key::StatusConnecting,
            Self::Connected => Key::StatusConnected,
        }
    }

    fn text_color(self) -> Color32 {
        match self {
            Self::Disconnected => theme::TEXT_MUTED,
            Self::Scanning | Self::Connecting => theme::TEXT_BODY,
            Self::Connected => theme::TEXT,
        }
    }

    fn is_connected(self) -> bool {
        self == Self::Connected
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Device,
    Input,
    Config,
    Settings,
    Macro,
    Log,
}

impl Tab {
    const ALL: [Self; 6] = [
        Self::Device,
        Self::Input,
        Self::Config,
        Self::Settings,
        Self::Macro,
        Self::Log,
    ];

    fn key(self) -> Key {
        match self {
            Self::Device => Key::NavDevice,
            Self::Input => Key::NavInput,
            Self::Config => Key::NavConfig,
            Self::Settings => Key::NavSettings,
            Self::Macro => Key::NavMacro,
            Self::Log => Key::NavLog,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    Scanning,
    Connecting,
    Reconnecting,
    InitialRead,
    ReadVersion,
    ReadConfig,
    Backup,
    Disconnecting,
    ReadMacro,
    WriteConfig,
    WriteMacro,
}

impl Busy {
    fn key(self) -> Key {
        match self {
            Self::Scanning => Key::BusyScanning,
            Self::Connecting => Key::BusyConnecting,
            Self::Reconnecting => Key::BusyReconnecting,
            Self::InitialRead => Key::BusyInitialRead,
            Self::ReadVersion => Key::BusyReadVersion,
            Self::ReadConfig => Key::BusyReadConfig,
            Self::Backup => Key::BusyBackup,
            Self::Disconnecting => Key::BusyDisconnecting,
            Self::ReadMacro => Key::BusyReadMacro,
            Self::WriteConfig => Key::BusyWriteConfig,
            Self::WriteMacro => Key::BusyWriteMacro,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogLevel {
    Info,
    Error,
}

struct LogEntry {
    at: String,
    level: LogLevel,
    notice: Notice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Text,
    Data,
    Strong,
}

/// Completed write shown in the result banner.
#[derive(Debug, Clone)]
enum WriteOutcome {
    Config(Box<ConfigWriteReport>),
    Macro(Box<MacroWriteReport>),
}

/// Main application state.
pub struct App {
    worker: Worker,
    connection: ConnectionState,
    scan_entries: Vec<ScanEntry>,
    selected_address: Option<String>,
    device: Option<DeviceInfoView>,
    version: Option<VersionView>,
    config: Option<ConfigView>,
    config_snapshot: Option<ConfigSnapshot>,
    settings: Option<SettingsDraft>,
    settings_slot: Slot,
    macro_editor: MacroEditor,
    pending: Option<PendingWrite>,
    draft_error: Option<DraftError>,
    write_failure: Option<Box<WriteFailure>>,
    write_result: Option<WriteOutcome>,
    last_write_backup: Option<BackupRef>,
    input: Option<InputView>,
    live: bool,
    busy: Option<Busy>,
    backup: Option<BackupReport>,
    backup_dir: PathBuf,
    error: Option<Notice>,
    log: VecDeque<LogEntry>,
    tab: Tab,
    lang: Lang,
    report_window_start: Instant,
    report_window_count: u32,
    reports_per_second: f32,
    last_input_at: Option<Instant>,
    scan_started_at: Option<Instant>,
    left_trail: VecDeque<(f32, f32)>,
    right_trail: VecDeque<(f32, f32)>,
    tab_underline_x: f32,
    motion_scale: f32,
    /// Whether the licenses / open-source screen is open.
    licenses_open: bool,
}

impl App {
    /// Create the app, install the bundled theme/fonts, and start the worker.
    #[must_use]
    pub fn new(creation: &eframe::CreationContext<'_>) -> Self {
        theme::install(&creation.egui_ctx);
        Self::build(Worker::spawn(), default_backup_dir())
    }

    fn build(worker: Worker, backup_dir: PathBuf) -> Self {
        Self {
            worker,
            connection: ConnectionState::Disconnected,
            scan_entries: Vec::new(),
            selected_address: None,
            device: None,
            version: None,
            config: None,
            config_snapshot: None,
            settings: None,
            settings_slot: Slot::S1,
            macro_editor: MacroEditor::default(),
            pending: None,
            draft_error: None,
            write_failure: None,
            write_result: None,
            last_write_backup: None,
            input: None,
            live: false,
            busy: None,
            backup: None,
            backup_dir,
            error: None,
            log: VecDeque::new(),
            tab: Tab::Device,
            lang: Lang::default(),
            report_window_start: Instant::now(),
            report_window_count: 0,
            reports_per_second: 0.0,
            last_input_at: None,
            scan_started_at: None,
            left_trail: VecDeque::new(),
            right_trail: VecDeque::new(),
            tab_underline_x: 0.0,
            motion_scale: 1.0,
            licenses_open: false,
        }
    }

    fn drain_events(&mut self) {
        while let Some(event) = self.worker.try_recv() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::ScanStarted => {
                self.connection = ConnectionState::Scanning;
                self.busy = Some(Busy::Scanning);
                self.scan_started_at = Some(Instant::now());
                self.push_notice(LogLevel::Info, Notice::new(Key::LogScanStarted));
            }
            Event::ScanFinished(entries) => {
                if self.connection == ConnectionState::Scanning {
                    self.connection = ConnectionState::Disconnected;
                }
                self.scan_started_at = None;
                if self.selected_address.as_ref().is_none_or(|address| {
                    !entries
                        .iter()
                        .any(|entry| entry.is_app && &entry.address == address)
                }) {
                    self.selected_address = entries
                        .iter()
                        .find(|entry| entry.is_app)
                        .map(|entry| entry.address.clone());
                }
                let app_count = entries.iter().filter(|entry| entry.is_app).count();
                self.push_notice(
                    LogLevel::Info,
                    Notice::new(Key::LogScanFinished)
                        .with_arg(entries.len().to_string())
                        .with_arg(app_count.to_string()),
                );
                self.scan_entries = entries;
                self.busy = None;
            }
            Event::Connecting => {
                self.connection = ConnectionState::Connecting;
                self.busy = Some(if self.device.is_some() {
                    Busy::Reconnecting
                } else {
                    Busy::Connecting
                });
            }
            Event::Connected(info) => {
                self.connection = ConnectionState::Connected;
                self.device = Some(info);
                self.version = None;
                self.config = None;
                self.config_snapshot = None;
                self.settings = None;
                self.macro_editor = MacroEditor::default();
                self.pending = None;
                self.draft_error = None;
                self.write_failure = None;
                self.write_result = None;
                self.last_write_backup = None;
                self.input = None;
                self.live = false;
                self.backup = None;
                self.last_input_at = None;
                self.report_window_start = Instant::now();
                self.report_window_count = 0;
                self.reports_per_second = 0.0;
                self.left_trail.clear();
                self.right_trail.clear();
                self.busy = Some(Busy::InitialRead);
                self.error = None;
                self.push_notice(LogLevel::Info, Notice::new(Key::LogConnected));
            }
            Event::Version(version) => {
                let summary = version
                    .value
                    .map_or_else(|| version.raw_hex.clone(), |value| format!("0x{value:08X}"));
                self.push_notice(
                    LogLevel::Info,
                    Notice::new(Key::LogVersion).with_arg(summary),
                );
                self.version = Some(version);
            }
            Event::Config { snapshot, view } => {
                self.push_notice(
                    LogLevel::Info,
                    Notice::new(Key::LogConfig)
                        .with_arg(view.size.to_string())
                        .with_arg(view.sha256.clone()),
                );
                if self.macro_editor.is_loaded()
                    && !self.macro_editor.record_matches_config(&snapshot.config)
                {
                    let was_recording = self.macro_editor.recording;
                    self.macro_editor.unload();
                    if was_recording {
                        self.push_notice(LogLevel::Info, Notice::new(Key::LogRecordingStopped));
                    }
                    self.push_notice(LogLevel::Info, Notice::new(Key::LogMacroReload));
                }
                self.settings = Some(SettingsDraft::from_config(&snapshot.config));
                self.config_snapshot = Some(*snapshot);
                self.config = Some(*view);
            }
            Event::MacroLoaded { slot, index, entry } => {
                if self.macro_editor.load(*entry) {
                    self.push_notice(LogLevel::Info, Notice::new(Key::LogRecordingStopped));
                }
                self.push_notice(
                    LogLevel::Info,
                    Notice::new(Key::LogMacroRead)
                        .with_arg((index.index() + 1).to_string())
                        .with_arg(slot.number().to_string()),
                );
            }
            Event::Input(input) => {
                if self.macro_editor.recording {
                    let was_full = self.macro_editor.recorder.is_full();
                    self.macro_editor.record(&input);
                    if !was_full && self.macro_editor.recorder.is_full() {
                        self.push_notice(LogLevel::Info, Notice::new(Key::MacroRecordingFull));
                    }
                }
                self.input = Some(input);
                self.left_trail.push_back((
                    f32::from(input.left_stick.0) / 128.0,
                    f32::from(input.left_stick.1) / 128.0,
                ));
                self.right_trail.push_back((
                    f32::from(input.right_stick.0) / 128.0,
                    f32::from(input.right_stick.1) / 128.0,
                ));
                while self.left_trail.len() > 8 {
                    self.left_trail.pop_front();
                }
                while self.right_trail.len() > 8 {
                    self.right_trail.pop_front();
                }
                self.note_input();
            }
            Event::Live(enabled) => {
                self.live = enabled;
                if !enabled && self.macro_editor.recording {
                    self.macro_editor.stop_recording();
                }
                self.push_notice(
                    LogLevel::Info,
                    Notice::new(if enabled {
                        Key::LogLiveStarted
                    } else {
                        Key::LogLiveStopped
                    }),
                );
            }
            Event::Backup(report) => {
                self.push_notice(
                    LogLevel::Info,
                    Notice::new(Key::LogBackup)
                        .with_arg(report.raw_path.display().to_string())
                        .with_arg(report.sha256.clone()),
                );
                self.backup = Some(*report);
            }
            Event::WriteBackup(backup) => {
                self.last_write_backup = Some(backup);
            }
            Event::ConfigWritten(report) => {
                self.last_write_backup = Some(BackupRef::from(&report.backup));
                self.write_failure = None;
                self.draft_error = None;
                self.error = None;
                self.write_result = Some(WriteOutcome::Config(report));
                self.push_notice(LogLevel::Info, Notice::new(Key::LogConfigWritten));
            }
            Event::MacroWritten(report) => {
                self.last_write_backup = Some(BackupRef::from(&report.backup));
                self.write_failure = None;
                self.draft_error = None;
                self.error = None;
                if self.macro_editor.load(report.readback.clone()) {
                    self.push_notice(LogLevel::Info, Notice::new(Key::LogRecordingStopped));
                }
                self.write_result = Some(WriteOutcome::Macro(report.clone()));
                self.push_notice(
                    LogLevel::Info,
                    Notice::new(Key::LogMacroWritten)
                        .with_arg(self.macro_editor.slot.number().to_string())
                        .with_arg((self.macro_editor.index.index() + 1).to_string()),
                );
            }
            Event::WriteFailed(failure) => {
                self.write_failure = Some(failure);
                self.write_result = None;
            }
            Event::Disconnected { reason } => {
                self.connection = ConnectionState::Disconnected;
                self.live = false;
                self.pending = None;
                self.draft_error = None;
                if self.macro_editor.recording {
                    self.macro_editor.stop_recording();
                }
                self.input = None;
                self.scan_started_at = None;
                self.busy = None;
                if let Some(reason) = reason {
                    self.push_notice(LogLevel::Error, reason.clone());
                    self.error = Some(reason);
                } else {
                    self.push_notice(LogLevel::Info, Notice::new(Key::LogDisconnected));
                }
            }
            Event::Error(notice) => {
                self.push_notice(LogLevel::Error, notice.clone());
                self.error = Some(notice);
                self.busy = None;
                if self.macro_editor.recording {
                    self.macro_editor.stop_recording();
                }
            }
            Event::Log(notice) => {
                self.push_notice(LogLevel::Info, notice);
            }
            Event::Idle => {
                self.busy = None;
            }
        }
    }

    fn note_input(&mut self) {
        let now = Instant::now();
        self.last_input_at = Some(now);
        self.report_window_count += 1;
        let elapsed = now.duration_since(self.report_window_start);
        if elapsed >= Duration::from_secs(1) {
            self.reports_per_second = self.report_window_count as f32 / elapsed.as_secs_f32();
            self.report_window_start = now;
            self.report_window_count = 0;
        }
    }

    fn push_notice(&mut self, level: LogLevel, notice: Notice) {
        if self.log.len() >= MAX_LOG_ENTRIES {
            self.log.pop_front();
        }
        self.log.push_back(LogEntry {
            at: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            notice,
        });
    }

    fn send(&mut self, command: Command, busy: Option<Busy>) {
        if self.worker.send(command) {
            if let Some(busy) = busy {
                self.busy = Some(busy);
            }
        } else {
            let notice = Notice::new(Key::LogAppNotRunning);
            self.push_notice(LogLevel::Error, notice.clone());
            self.error = Some(notice);
        }
    }

    fn can_read(&self) -> bool {
        self.connection.is_connected() && self.busy.is_none()
    }

    /// True when the settings controls can be edited.
    fn can_edit_settings(&self) -> bool {
        self.connection.is_connected()
            && self.busy.is_none()
            && self.config_snapshot.is_some()
            && self.settings.is_some()
    }

    /// True when a write may be confirmed: connected, idle, configuration read,
    /// and no macro recording is running.
    ///
    /// Recording is excluded on purpose: the confirmation dialog is
    /// non-modal, so the user can start a recording behind it, and a write
    /// pauses the 20 ms keepalive (missing reports) or replaces the take in
    /// `MacroEditor::load`. The user must stop recording first.
    fn can_write(&self) -> bool {
        self.can_edit_settings() && !self.macro_editor.recording
    }

    /// Open the confirmation dialog for the current settings draft.
    fn review_settings(&mut self) {
        if self.macro_editor.recording {
            self.draft_error = Some(DraftError::Recording);
            return;
        }
        let (Some(snapshot), Some(settings)) =
            (self.config_snapshot.clone(), self.settings.clone())
        else {
            self.draft_error = Some(DraftError::NoChanges);
            return;
        };
        match settings.pending_write(&snapshot, self.settings_slot) {
            Ok(pending) => {
                self.draft_error = None;
                self.pending = Some(pending);
            }
            Err(error) => {
                self.draft_error = Some(error);
            }
        }
    }

    /// Open the confirmation dialog for the loaded macro.
    fn review_macro(&mut self) {
        if self.macro_editor.recording {
            self.draft_error = Some(DraftError::Recording);
            return;
        }
        let Some(snapshot) = self.config_snapshot.clone() else {
            self.draft_error = Some(DraftError::NoSnapshot);
            return;
        };
        match self.macro_editor.pending_write(&snapshot) {
            Ok(pending) => {
                self.draft_error = None;
                self.pending = Some(pending);
            }
            Err(error) => {
                self.draft_error = Some(error);
            }
        }
    }

    /// Send the confirmed write. The confirmation dialog is the only caller.
    fn confirm_write(&mut self, pending: PendingWrite) {
        if self.macro_editor.recording {
            // Keep the dialog open; the confirm key is disabled until the
            // recording is stopped.
            self.draft_error = Some(DraftError::Recording);
            self.pending = Some(pending);
            return;
        }
        if !self.can_write() {
            self.pending = None;
            return;
        }
        let dir = self.backup_dir.clone();
        if let Some((command, busy)) = pending_command(pending, dir) {
            self.send(command, Some(busy));
        }
        self.pending = None;
        self.write_result = None;
        self.write_failure = None;
    }

    /// Discard the settings draft and reload it from the last read block.
    fn reset_settings(&mut self) {
        if let Some(snapshot) = &self.config_snapshot {
            self.settings = Some(SettingsDraft::from_config(&snapshot.config));
            self.draft_error = None;
        }
    }

    /// Read the macro selected in the macro tab.
    fn request_read_macro(&mut self) {
        let (slot, index) = (self.macro_editor.slot, self.macro_editor.index);
        self.macro_editor.select(slot, index);
        self.send(Command::ReadMacro { slot, index }, Some(Busy::ReadMacro));
    }

    fn selected_app_address(&self) -> Option<String> {
        self.selected_address.clone().filter(|address| {
            self.scan_entries
                .iter()
                .any(|entry| entry.is_app && &entry.address == address)
        })
    }

    fn selected_address_or_first_app(&self) -> Option<&ScanEntry> {
        let address = self.selected_address.as_deref()?;
        self.scan_entries
            .iter()
            .find(|entry| entry.address == address)
    }

    /// True when device/version data from an earlier session is on screen.
    fn has_device_data(&self) -> bool {
        self.device.is_some() || self.version.is_some()
    }

    /// True when a configuration snapshot from an earlier session is on screen.
    fn has_config_data(&self) -> bool {
        self.config.is_some()
    }

    /// Only warn about stale data when there is data to be stale.
    fn stale_device(&self) -> bool {
        !self.connection.is_connected() && self.has_device_data()
    }

    /// Only warn about a stale configuration when one is cached.
    fn stale_config(&self) -> bool {
        !self.connection.is_connected() && self.has_config_data()
    }

    /// The onboarding/empty card replaces the device view until data exists.
    fn wants_onboarding(&self) -> bool {
        !self.connection.is_connected() && !self.has_device_data()
    }

    fn request_scan(&mut self) {
        self.send(Command::Scan, Some(Busy::Scanning));
    }

    fn request_connect(&mut self) {
        let address = self.selected_app_address();
        let busy = if self.device.is_some() {
            Busy::Reconnecting
        } else {
            Busy::Connecting
        };
        self.send(Command::Connect { address }, Some(busy));
    }

    fn request_disconnect(&mut self) {
        self.send(Command::Disconnect, Some(Busy::Disconnecting));
    }

    fn notice_text(&self, notice: &Notice) -> String {
        i18n::format(self.lang, notice.key, &notice.args)
    }

    fn text(&self, key: Key) -> &'static str {
        i18n::t(self.lang, key)
    }

    fn anim(&self, seconds: f32) -> f32 {
        seconds * self.motion_scale
    }

    fn scan_angle(&self) -> Option<f32> {
        if self.connection == ConnectionState::Scanning {
            self.scan_started_at
                .map(|started| started.elapsed().as_secs_f32() / 1.5 * TAU)
        } else {
            None
        }
    }

    fn connect_enabled(&self) -> bool {
        self.connection == ConnectionState::Disconnected && self.busy.is_none()
    }

    // -----------------------------------------------------------------------
    // Top bar
    // -----------------------------------------------------------------------

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top-bar")
            .exact_height(theme::TOP_BAR_HEIGHT)
            .frame(Frame::none().fill(theme::SURFACE_1))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                painter.hline(
                    Rangef::new(rect.left(), rect.right()),
                    rect.top() + 0.5,
                    Stroke::new(1.0_f32, theme::EDGE_LIGHT),
                );
                painter.hline(
                    Rangef::new(rect.left(), rect.right()),
                    rect.bottom() - 0.5,
                    Stroke::new(1.0_f32, theme::HAIRLINE),
                );

                let inner = rect.shrink2(Vec2::new(theme::SPACE_4, 0.0));
                let mut bar = ui.new_child(
                    UiBuilder::new()
                        .max_rect(inner)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                bar.vertical(|ui| {
                    ui.add(egui::Label::new(theme::job_title(self.text(Key::AppTitle))));
                    ui.add(egui::Label::new(theme::job_meta(
                        self.text(Key::WriteCapable),
                    )));
                });
                bar.add_space(theme::SPACE_5);
                let status = self.connection;
                status_dot(
                    &mut bar,
                    status,
                    self.scan_angle(),
                    ui.ctx().animate_value_with_time(
                        Id::new("status-glow"),
                        if status.is_connected() { 1.0 } else { 0.0 },
                        self.anim(0.3),
                    ),
                );
                bar.add_space(theme::SPACE_2);
                bar.add(egui::Label::new(theme::text_job(
                    self.text(status.key()),
                    theme::medium(theme::FONT_CONTROL),
                    status.text_color(),
                    0.0,
                    Some(theme::LINE_HEIGHT_CONTROL),
                )));

                bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.language_switch(ui);
                    if let Some(busy) = self.busy {
                        ui.add_space(theme::SPACE_3);
                        ui.add(egui::Label::new(theme::job_meta(self.text(busy.key()))));
                    }
                });
            });
    }

    fn language_switch(&mut self, ui: &mut egui::Ui) {
        for (lang, key) in [(Lang::En, Key::LangEnglish), (Lang::Ja, Key::LangJapanese)] {
            let selected = self.lang == lang;
            let label = i18n::t(lang, key);
            let width = measure(ui, label, theme::medium(theme::FONT_META)) + theme::SPACE_3;
            ui.add_space(theme::SPACE_1);
            let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::click());
            let hover = ui.ctx().animate_bool_with_time(
                response.id.with("hover"),
                response.hovered(),
                self.anim(0.10),
            );
            if hover > 0.0 {
                ui.painter().rect_filled(
                    rect,
                    theme::RADIUS_TAB,
                    theme::lerp_color(theme::SURFACE_1, theme::SURFACE_2, hover),
                );
            }
            paint_job_centered(
                ui.painter(),
                rect.center(),
                theme::text_job(
                    label,
                    theme::medium(theme::FONT_META),
                    if selected {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    },
                    0.0,
                    Some(theme::LINE_HEIGHT_META),
                ),
            );
            if selected {
                ui.painter().rect_filled(
                    Rect::from_min_size(
                        Pos2::new(rect.left() + 8.0, rect.bottom() - 2.0),
                        Vec2::new(rect.width() - 16.0, 2.0),
                    ),
                    1.0,
                    theme::LED,
                );
            }
            focus_ring(ui, &response);
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
            });
            if activated(ui, &response) {
                self.lang = lang;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Left rail
    // -----------------------------------------------------------------------

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("rail")
            .resizable(false)
            .exact_width(theme::SIDE_PANEL_WIDTH)
            .frame(Frame::none().fill(theme::SURFACE_1))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                painter.hline(
                    Rangef::new(rect.left(), rect.right()),
                    rect.top() + 0.5,
                    Stroke::new(1.0_f32, theme::EDGE_LIGHT),
                );
                painter.vline(
                    rect.right() - 0.5,
                    Rangef::new(rect.top(), rect.bottom()),
                    Stroke::new(1.0_f32, theme::HAIRLINE),
                );

                let inner = rect.shrink2(Vec2::new(20.0, 20.0));
                let footer_height = 150.0;
                let body_height = (inner.height() - footer_height).max(80.0);
                let body_rect =
                    Rect::from_min_size(inner.min, Vec2::new(inner.width(), body_height));
                let mut body_ui = ui.new_child(UiBuilder::new().max_rect(body_rect));
                egui::ScrollArea::vertical()
                    .id_salt("rail-scroll")
                    .auto_shrink([false, false])
                    .show(&mut body_ui, |ui| self.rail_body(ui));

                let footer_rect = Rect::from_min_size(
                    Pos2::new(inner.left(), inner.max.y - footer_height),
                    Vec2::new(inner.width(), footer_height),
                );
                ui.painter().hline(
                    Rangef::new(footer_rect.left(), footer_rect.right()),
                    footer_rect.top(),
                    Stroke::new(1.0_f32, theme::HAIRLINE),
                );
                let mut footer_ui = ui.new_child(UiBuilder::new().max_rect(footer_rect));
                self.rail_footer(&mut footer_ui);
            });
    }

    fn rail_body(&mut self, ui: &mut egui::Ui) {
        ui.add_space(theme::SPACE_2);
        // The top bar is the single source of connection state. The rail shows
        // the device identity only, so the same "未接続" ring is not repeated.
        section_label(ui, self.lang, Key::SectionDevice);
        ui.add_space(theme::SPACE_2);
        if let Some(device) = &self.device {
            ui.add(egui::Label::new(theme::text_job(
                device.name.as_deref().unwrap_or(self.text(Key::NoName)),
                theme::medium(theme::FONT_BODY),
                theme::TEXT,
                0.0,
                Some(theme::LINE_HEIGHT_CONTROL),
            )));
            ui.add_space(theme::SPACE_1);
            ui.add(egui::Label::new(theme::job_data(
                &device.address,
                theme::TEXT_MUTED,
            )));
            ui.add(egui::Label::new(theme::job_data(
                &i18n::format(
                    self.lang,
                    Key::MtuValue,
                    &[device.mtu.to_string(), device.characteristic.clone()],
                ),
                theme::TEXT_MUTED,
            )));
        } else {
            ui.add(egui::Label::new(theme::job_meta(self.text(Key::NoDeviceHint))).wrap());
        }

        if self.connection.is_connected() {
            ui.add_space(theme::SPACE_3);
            if secondary_key(
                ui,
                "rail-disconnect",
                self.can_read(),
                self.text(Key::Disconnect),
                theme::SURFACE_1,
            )
            .on_hover_text(self.text(Key::HintDisconnect))
            .clicked()
            {
                self.request_disconnect();
            }
        }

        rail_divider(ui);
        section_label(ui, self.lang, Key::SectionActions);
        ui.add_space(theme::SPACE_2);
        let can_read = self.can_read();
        if !can_read {
            let reason = if self.connection.is_connected() {
                Key::BusyHint
            } else {
                Key::ConnectFirstHint
            };
            ui.add(egui::Label::new(theme::job_meta(self.text(reason))).wrap());
            ui.add_space(theme::SPACE_1);
        }
        if quiet_row(
            ui,
            "rail-version",
            self.text(Key::ReadVersionAction),
            can_read,
            false,
        )
        .on_hover_text(self.text(Key::HintReadVersion))
        .clicked()
        {
            self.send(Command::ReadVersion, Some(Busy::ReadVersion));
        }
        if quiet_row(
            ui,
            "rail-config",
            self.text(Key::ReadConfigAction),
            can_read,
            false,
        )
        .on_hover_text(self.text(Key::HintReadConfig))
        .clicked()
        {
            self.send(Command::ReadConfig, Some(Busy::ReadConfig));
        }
        if quiet_row(
            ui,
            "rail-backup",
            self.text(Key::BackupConfigAction),
            can_read,
            false,
        )
        .on_hover_text(self.text(Key::BackupConfigHint))
        .clicked()
        {
            let dir = self.backup_dir.clone();
            self.send(Command::Backup { dir }, Some(Busy::Backup));
        }
        let live_key = if self.live {
            Key::StopLiveAction
        } else {
            Key::StartLiveAction
        };
        if quiet_row(ui, "rail-live", self.text(live_key), can_read, false)
            .on_hover_text(self.text(Key::HintLive))
            .clicked()
        {
            self.send(
                if self.live {
                    Command::StopLive
                } else {
                    Command::StartLive
                },
                None,
            );
        }
        if self.connection.is_connected() && !self.live {
            ui.add_space(theme::SPACE_1);
            ui.add(egui::Label::new(theme::job_meta(self.text(Key::LiveStoppedHint))).wrap());
        }

        rail_divider(ui);
        egui::CollapsingHeader::new(theme::job_section(self.text(Key::SectionBackupFolder)))
            .id_salt("rail-backup-folder")
            .default_open(false)
            .show(ui, |ui| {
                ui.add(egui::Label::new(theme::job_data(
                    &self.backup_dir.display().to_string(),
                    theme::TEXT_MUTED,
                )));
                if let Some(report) = &self.backup {
                    ui.add_space(theme::SPACE_2);
                    ui.add(egui::Label::new(theme::job_meta(
                        self.text(Key::LastBackup),
                    )));
                    ui.add(egui::Label::new(theme::job_data(
                        &report.raw_path.display().to_string(),
                        theme::TEXT_MUTED,
                    )));
                    ui.add(egui::Label::new(theme::job_data(
                        &report.json_path.display().to_string(),
                        theme::TEXT_MUTED,
                    )));
                    ui.add_space(theme::SPACE_2);
                    if secondary_key(
                        ui,
                        "rail-copy-sha",
                        true,
                        self.text(Key::CopySha256),
                        theme::SURFACE_1,
                    )
                    .clicked()
                    {
                        ui.output_mut(|output| output.copied_text = report.sha256.clone());
                    }
                    ui.add(egui::Label::new(theme::job_data(
                        &report.sha256,
                        theme::TEXT_BODY,
                    )));
                    for warning in &report.warnings {
                        ui.add(egui::Label::new(theme::job_meta_color(
                            &format!("{}: {warning}", self.text(Key::Warning)),
                            theme::AMBER,
                        )));
                    }
                }
            });
    }

    fn rail_footer(&mut self, ui: &mut egui::Ui) {
        ui.add_space(theme::SPACE_3);
        section_label(ui, self.lang, Key::SectionSafety);
        ui.add_space(theme::SPACE_1);
        ui.add(egui::Label::new(theme::job_meta(self.text(Key::SafetyText))).wrap());
        ui.add_space(theme::SPACE_2);
        // The write confirmation is a centered modal that covers this button
        // and must be resolved first; disabling the entry keeps the two
        // dialogs from being open at once.
        if secondary_key(
            ui,
            "open-licenses",
            self.pending.is_none(),
            self.text(Key::OpenLicenses),
            theme::SURFACE_2,
        )
        .clicked()
        {
            self.licenses_open = true;
        }
    }

    // -----------------------------------------------------------------------
    // Central panel
    // -----------------------------------------------------------------------

    fn central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::none().fill(theme::BG))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let tab_bottom = rect.top() + theme::TAB_HEIGHT;

                let tabs_rect =
                    Rect::from_min_size(rect.min, Vec2::new(rect.width(), theme::TAB_HEIGHT));
                let mut tabs_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(tabs_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                tabs_ui.add_space(theme::CONTENT_INSET);
                self.tab_bar(&mut tabs_ui, rect);

                ui.painter().hline(
                    Rangef::new(rect.left(), rect.right()),
                    tab_bottom,
                    Stroke::new(1.0_f32, theme::HAIRLINE),
                );

                let content_left = rect.left() + theme::CONTENT_INSET;
                let content_right = (content_left + theme::CONTENT_MAX_WIDTH)
                    .min(rect.right() - theme::CONTENT_INSET);
                let content_top = tab_bottom + theme::SPACE_4;
                let content_bottom = rect.bottom() - theme::SPACE_4;
                let meta_height = theme::LINE_HEIGHT_META;
                let scroll_bottom = content_bottom - meta_height - theme::SPACE_2;
                let content_rect = Rect::from_min_max(
                    Pos2::new(content_left, content_top),
                    Pos2::new(content_right.max(content_left + 100.0), scroll_bottom),
                );
                ui.painter().vline(
                    content_right,
                    Rangef::new(content_top, content_bottom),
                    Stroke::new(1.0_f32, theme::HAIRLINE),
                );

                // A bottom meta line closes the column, so tall windows read as
                // a bounded sheet of paper rather than unfinished space.
                let footer = self.text(Key::FooterMeta).to_owned();
                let meta_rect = Rect::from_min_size(
                    Pos2::new(content_left, content_bottom - meta_height),
                    Vec2::new(content_right - content_left, meta_height),
                );
                paint_job_left_center(
                    ui.painter(),
                    Pos2::new(meta_rect.left(), meta_rect.center().y),
                    theme::job_meta(&footer),
                );
                ui.interact(meta_rect, ui.id().with("footer-meta"), Sense::hover())
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &footer)
                    });

                let mut content_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(Layout::top_down(Align::LEFT)),
                );
                egui::ScrollArea::vertical()
                    .id_salt("content-scroll")
                    .auto_shrink([false, false])
                    .show(&mut content_ui, |ui| {
                        if let Some(error) = self.error.clone() {
                            self.error_notice(ui, error);
                            ui.add_space(theme::SPACE_4);
                        }
                        if self.write_result.is_some() {
                            self.write_result_banner(ui);
                            ui.add_space(theme::SPACE_3);
                        }
                        if self.write_failure.is_some() {
                            self.write_failure_banner(ui);
                            ui.add_space(theme::SPACE_3);
                        }
                        match self.tab {
                            Tab::Device => self.device_tab(ui),
                            Tab::Input => self.input_tab(ui),
                            Tab::Config => self.config_tab(ui),
                            Tab::Settings => self.settings_tab(ui),
                            Tab::Macro => self.macro_tab(ui),
                            Tab::Log => self.log_tab(ui),
                        }
                    });
            });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui, panel_rect: Rect) {
        let mut selected_rect = None;
        let mut clicked = None;
        for tab in Tab::ALL {
            let label = self.text(tab.key());
            let width = measure(ui, label, theme::medium(theme::FONT_CONTROL)) + 28.0;
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(width, theme::TAB_HEIGHT), Sense::click());
            let hover = ui.ctx().animate_bool_with_time(
                response.id.with("hover"),
                response.hovered(),
                self.anim(0.10),
            );
            if hover > 0.0 {
                ui.painter().rect_filled(
                    rect.shrink2(Vec2::new(4.0, 6.0)),
                    theme::RADIUS_TAB,
                    theme::lerp_color(theme::BG, theme::SURFACE_2, hover),
                );
            }
            paint_job_centered(
                ui.painter(),
                rect.center(),
                theme::text_job(
                    label,
                    theme::medium(theme::FONT_CONTROL),
                    if self.tab == tab {
                        theme::TEXT
                    } else {
                        theme::TEXT_MUTED
                    },
                    0.0,
                    Some(theme::LINE_HEIGHT_CONTROL),
                ),
            );
            focus_ring(ui, &response);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    self.tab == tab,
                    label,
                )
            });
            if activated(ui, &response) {
                clicked = Some(tab);
            }
            if self.tab == tab {
                selected_rect = Some(rect);
            }
        }
        if let Some(tab) = clicked {
            self.tab = tab;
        }

        if self.tab == Tab::Log {
            let clear_rect = Rect::from_center_size(
                Pos2::new(
                    panel_rect.right() - theme::CONTENT_INSET - 56.0,
                    panel_rect.top() + theme::TAB_HEIGHT / 2.0,
                ),
                Vec2::new(112.0, 32.0),
            );
            if secondary_key_at(
                ui,
                clear_rect,
                "log-clear",
                !self.log.is_empty(),
                self.text(Key::Clear),
                self.anim(0.10),
                theme::BG,
            )
            .clicked()
            {
                self.log.clear();
            }
        }

        if let Some(rect) = selected_rect {
            let target = rect.left() + 12.0;
            let width = rect.width() - 24.0;
            let x =
                ui.ctx()
                    .animate_value_with_time(Id::new("tab-underline"), target, self.anim(0.18));
            self.tab_underline_x = x;
            ui.painter().rect_filled(
                Rect::from_min_size(
                    Pos2::new(
                        self.tab_underline_x,
                        panel_rect.top() + theme::TAB_HEIGHT - 2.0,
                    ),
                    Vec2::new(width, 2.0),
                ),
                1.0,
                theme::LED,
            );
        }
    }

    // -----------------------------------------------------------------------
    // Shared notices
    // -----------------------------------------------------------------------

    fn error_notice(&mut self, ui: &mut egui::Ui, notice: Notice) {
        Frame::none()
            .fill(theme::SURFACE_2)
            .rounding(theme::RADIUS_KEY)
            .inner_margin(Margin::symmetric(theme::SPACE_3, 12.0))
            .show(ui, |ui| {
                let text = self.notice_text(&notice);
                ui.horizontal(|ui| {
                    let (bar, _) = ui.allocate_exact_size(Vec2::new(2.0, 22.0), Sense::hover());
                    ui.painter().rect_filled(bar, 1.0, theme::RED);
                    ui.add_space(theme::SPACE_2);
                    ui.add(egui::Label::new(theme::job_body_color(&text, theme::RED)).wrap());
                });
                if let Some(detail) = &notice.detail {
                    egui::CollapsingHeader::new(theme::job_meta(self.text(Key::ErrorDetails)))
                        .id_salt("error-detail")
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(theme::job_data(detail, theme::TEXT_MUTED)));
                        });
                }
                ui.add_space(theme::SPACE_1);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), 32.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        if secondary_key(
                            ui,
                            "error-dismiss",
                            true,
                            self.text(Key::Dismiss),
                            theme::SURFACE_2,
                        )
                        .clicked()
                        {
                            self.error = None;
                        }
                    },
                );
            });
    }

    fn stale_warning(&mut self, ui: &mut egui::Ui, key: Key) {
        ui.horizontal(|ui| {
            status_dot(ui, ConnectionState::Disconnected, None, 0.0);
            ui.add_space(theme::SPACE_2);
            ui.add(egui::Label::new(theme::job_body_color(self.text(key), theme::AMBER)).wrap());
        });
        if !self.connection.is_connected() {
            ui.add_space(theme::SPACE_1);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), 36.0),
                Layout::right_to_left(Align::Center),
                |ui| {
                    if secondary_key(
                        ui,
                        "stale-reconnect",
                        true,
                        self.text(Key::Reconnect),
                        theme::BG,
                    )
                    .clicked()
                    {
                        self.request_connect();
                    }
                },
            );
        }
    }

    // -----------------------------------------------------------------------
    // Empty state (asymmetric: ring left, next move right)
    // -----------------------------------------------------------------------

    fn empty_state(&mut self, ui: &mut egui::Ui, tab: Tab) {
        let width = ui.available_width().min(theme::CONTENT_MAX_WIDTH);
        let card_height = match tab {
            Tab::Device => 224.0,
            Tab::Input | Tab::Config | Tab::Settings | Tab::Macro | Tab::Log => 200.0,
        };
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, card_height), Sense::hover());
        let painter = ui.painter();
        painter.rect(rect, theme::RADIUS_CARD, theme::SURFACE_3, Stroke::NONE);
        painter.hline(
            Rangef::new(rect.left() + 12.0, rect.right() - 12.0),
            rect.top() + 0.5,
            Stroke::new(1.0_f32, theme::EDGE_LIGHT),
        );

        let inner = rect.shrink(theme::CARD_PADDING);
        let ring_rect = Rect::from_center_size(
            Pos2::new(inner.left() + theme::RING_DIAMETER / 2.0, inner.center().y),
            Vec2::splat(theme::RING_DIAMETER),
        );
        let lit = self.connection.is_connected();
        let glow = ui.ctx().animate_value_with_time(
            Id::new("empty-ring-glow"),
            if lit { 1.0 } else { 0.0 },
            self.anim(0.3),
        );
        large_ring(
            ui,
            ring_rect,
            lit,
            glow,
            self.scan_angle(),
            self.text(self.connection.key()),
        );

        let center_rect = Rect::from_center_size(
            ring_rect.center(),
            Vec2::new(theme::KEY_MIN_WIDTH, theme::KEY_HEIGHT),
        );
        if self.connection.is_connected() {
            if let Some(device) = &self.device {
                let name = device.name.as_deref().unwrap_or(self.text(Key::NoName));
                paint_job_centered(
                    ui.painter(),
                    ring_rect.center() - Vec2::new(0.0, 8.0),
                    theme::text_job(
                        name,
                        theme::medium(theme::FONT_BODY),
                        theme::TEXT,
                        0.0,
                        Some(theme::LINE_HEIGHT_CONTROL),
                    ),
                );
                paint_job_centered(
                    ui.painter(),
                    ring_rect.center() + Vec2::new(0.0, 12.0),
                    theme::job_data(&device.address, theme::TEXT_MUTED),
                );
            }
        } else {
            let label = if self.device.is_some() {
                self.text(Key::Reconnect)
            } else {
                self.text(Key::Connect)
            };
            if key_at(
                ui,
                center_rect,
                "ring-primary",
                self.connect_enabled(),
                KeyKind::Primary,
                label,
                self.anim(0.10),
                theme::SURFACE_3,
            )
            .on_hover_text(self.text(if self.selected_app_address().is_some() {
                Key::ConnectUsesSelected
            } else {
                Key::ScanHint
            }))
            .clicked()
            {
                self.request_connect();
            }
        }

        let right_left = ring_rect.right() + theme::SPACE_5;
        let right_rect = Rect::from_min_max(
            Pos2::new(right_left, inner.top()),
            Pos2::new(inner.right(), inner.bottom()),
        );
        if right_rect.width() > 120.0 {
            let mut right = ui.new_child(
                UiBuilder::new()
                    .max_rect(right_rect)
                    .layout(Layout::top_down(Align::LEFT)),
            );
            let heading = match tab {
                Tab::Device => Key::SectionConnectSteps,
                Tab::Log => Key::NavLog,
                Tab::Settings => Key::SectionWriteSettings,
                Tab::Macro => Key::SectionMacroEditor,
                Tab::Input | Tab::Config => Key::SectionNextSteps,
            };
            section_label(&mut right, self.lang, heading);
            right.add_space(theme::SPACE_2);
            match tab {
                Tab::Device => {
                    let current_step = if self.connection == ConnectionState::Scanning
                        || !self.scan_entries.is_empty()
                    {
                        2
                    } else {
                        0
                    };
                    right.scope(|ui| {
                        // `ui.horizontal` otherwise enforces `interact_size.y`,
                        // which would push the third step and the Scan key out
                        // of the 224 pt card.
                        ui.spacing_mut().interact_size.y = 20.0;
                        for (index, (title, body)) in [
                            (Key::Step1Title, Key::Step1Body),
                            (Key::Step2Title, Key::Step2Body),
                            (Key::Step3Title, Key::Step3Body),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            ui.horizontal(|ui| {
                                step_indicator(ui, index == current_step);
                                ui.add_space(theme::SPACE_2);
                                ui.add(egui::Label::new(theme::job_data(
                                    &format!("{}", index + 1),
                                    theme::TEXT_MUTED,
                                )));
                                ui.add_space(theme::SPACE_2);
                                ui.add(egui::Label::new(theme::text_job(
                                    self.text(title),
                                    theme::medium(theme::FONT_BODY),
                                    theme::TEXT_BODY,
                                    0.0,
                                    Some(theme::LINE_HEIGHT_CONTROL),
                                )))
                                .on_hover_text(self.text(body));
                            });
                            ui.add_space(theme::SPACE_1);
                        }
                    });
                }
                Tab::Input => {
                    if self.connection.is_connected() {
                        right.add(
                            egui::Label::new(theme::job_body(self.text(Key::InputWaitingReports)))
                                .wrap(),
                        );
                    } else {
                        right.add(
                            egui::Label::new(theme::job_body(self.text(Key::NoInputYet))).wrap(),
                        );
                        right.add_space(theme::SPACE_1);
                        right.add(
                            egui::Label::new(theme::job_meta(self.text(Key::InputWaitingConnect)))
                                .wrap(),
                        );
                    }
                }
                Tab::Config => {
                    right
                        .add(egui::Label::new(theme::job_body(self.text(Key::NoConfigYet))).wrap());
                    right.add_space(theme::SPACE_1);
                    right.add(
                        egui::Label::new(theme::job_meta(self.text(Key::ConfigWaitingConnect)))
                            .wrap(),
                    );
                }
                Tab::Settings => {
                    right.add(
                        egui::Label::new(theme::job_body(self.text(Key::SettingsNeedsConfig)))
                            .wrap(),
                    );
                }
                Tab::Macro => {
                    right.add(
                        egui::Label::new(theme::job_body(self.text(Key::MacroNeedsConfig))).wrap(),
                    );
                    right.add_space(theme::SPACE_1);
                    right.add(
                        egui::Label::new(theme::job_meta(self.text(Key::MacroNotLoaded))).wrap(),
                    );
                }
                Tab::Log => {
                    right.add(egui::Label::new(theme::job_body(self.text(Key::LogEmpty))).wrap());
                    right.add_space(theme::SPACE_1);
                    right.add(
                        egui::Label::new(theme::job_meta(self.text(Key::LogEmptyBody))).wrap(),
                    );
                }
            }

            right.add_space(theme::SPACE_3);
            // Each arm draws its own key and acts on the response; moving the
            // widget calls into match guards would make the flow unreadable.
            #[allow(clippy::collapsible_match)]
            match (tab, self.connection.is_connected(), self.live) {
                (Tab::Device, false, _) => {
                    if secondary_key(
                        &mut right,
                        "empty-scan",
                        self.connect_enabled(),
                        self.text(Key::Scan),
                        theme::SURFACE_3,
                    )
                    .on_hover_text(self.text(Key::ScanHint))
                    .clicked()
                    {
                        self.request_scan();
                    }
                }
                (Tab::Config | Tab::Settings | Tab::Macro, true, _) => {
                    if secondary_key(
                        &mut right,
                        "empty-read-config",
                        self.can_read(),
                        self.text(Key::ReadConfigAction),
                        theme::SURFACE_3,
                    )
                    .clicked()
                    {
                        self.send(Command::ReadConfig, Some(Busy::ReadConfig));
                    }
                }
                (Tab::Input, true, false) => {
                    if secondary_key(
                        &mut right,
                        "empty-start-live",
                        self.can_read(),
                        self.text(Key::StartLiveAction),
                        theme::SURFACE_3,
                    )
                    .clicked()
                    {
                        self.send(Command::StartLive, None);
                    }
                }
                _ => {}
            }

            if !self.connection.is_connected() {
                if let Some(entry) = self.selected_address_or_first_app() {
                    let name = entry
                        .name
                        .clone()
                        .unwrap_or_else(|| self.text(Key::NoName).to_string());
                    let label = i18n::format(
                        self.lang,
                        Key::SelectedDevice,
                        &[format!("{name} ({})", entry.address)],
                    );
                    right.add_space(theme::SPACE_2);
                    right.add(egui::Label::new(theme::job_meta(&label)).wrap());
                }
            }

            let has_non_app_selection = self.selected_address.as_ref().is_some_and(|address| {
                self.scan_entries
                    .iter()
                    .any(|entry| &entry.address == address && !entry.is_app)
            });
            if has_non_app_selection {
                right.add_space(theme::SPACE_2);
                right.add(egui::Label::new(theme::job_meta(self.text(Key::NotAppMode))).wrap());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Tabs
    // -----------------------------------------------------------------------

    fn device_tab(&mut self, ui: &mut egui::Ui) {
        if self.has_device_data() {
            if self.stale_device() {
                self.stale_warning(ui, Key::LastSessionWarning);
                ui.add_space(theme::SPACE_4);
            }
            self.device_details(ui);
            if !self.connection.is_connected() && !self.scan_entries.is_empty() {
                ui.add_space(theme::SPACE_5);
                self.device_list(ui);
            }
            return;
        }

        if self.wants_onboarding() {
            self.empty_state(ui, Tab::Device);
            ui.add_space(theme::SPACE_5);
            self.device_list(ui);
            return;
        }

        // Connected but no data yet: the first version read is still running.
        self.empty_state(ui, Tab::Device);
    }

    fn device_details(&self, ui: &mut egui::Ui) {
        section_label(ui, self.lang, Key::SectionDeviceInfo);
        ui.add_space(theme::SPACE_2);
        if let Some(device) = &self.device {
            let name = device
                .name
                .clone()
                .unwrap_or_else(|| self.text(Key::NoName).to_string());
            data_grid(
                ui,
                &[
                    (self.text(Key::Name).to_owned(), name, ValueKind::Text),
                    (
                        self.text(Key::Address).to_owned(),
                        device.address.clone(),
                        ValueKind::Data,
                    ),
                    (
                        self.text(Key::Mtu).to_owned(),
                        device.mtu.to_string(),
                        ValueKind::Data,
                    ),
                    (
                        self.text(Key::Characteristic).to_owned(),
                        device.characteristic.clone(),
                        ValueKind::Data,
                    ),
                ],
            );
        } else {
            ui.add(egui::Label::new(theme::job_meta(self.text(Key::NoDeviceInfo))).wrap());
        }

        ui.add_space(theme::SPACE_5);
        section_label(ui, self.lang, Key::SectionFirmware);
        ui.add_space(theme::SPACE_2);
        if let Some(version) = &self.version {
            data_grid(
                ui,
                &[
                    (
                        self.text(Key::FirmwareRaw).to_owned(),
                        if version.raw_hex.is_empty() {
                            self.text(Key::EmptyValue).to_owned()
                        } else {
                            version.raw_hex.clone()
                        },
                        ValueKind::Data,
                    ),
                    (
                        self.text(Key::FirmwareText).to_owned(),
                        version
                            .text
                            .clone()
                            .unwrap_or_else(|| self.text(Key::None).to_owned()),
                        ValueKind::Text,
                    ),
                    (
                        self.text(Key::FirmwareValue).to_owned(),
                        version.value.map_or_else(
                            || self.text(Key::NotFourByteValue).to_owned(),
                            format_u32_hex,
                        ),
                        ValueKind::Strong,
                    ),
                ],
            );
        } else {
            ui.add(egui::Label::new(theme::job_meta(self.text(Key::NoVersionYet))).wrap());
        }
    }

    fn device_list(&mut self, ui: &mut egui::Ui) {
        section_label(ui, self.lang, Key::SectionScanResults);
        ui.add_space(theme::SPACE_2);
        if self.connection == ConnectionState::Scanning {
            let time = ui.input(|input| input.time) as f32;
            let pulse = 0.45 + 0.15 * (time * 4.0).sin();
            for _ in 0..3 {
                let width = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(width, theme::DEVICE_ROW_HEIGHT),
                    Sense::hover(),
                );
                let painter = ui.painter();
                painter.rect_filled(
                    rect,
                    theme::RADIUS_KEY,
                    theme::SURFACE_2.gamma_multiply(pulse),
                );
                painter.rect_filled(
                    Rect::from_min_size(rect.min + Vec2::new(12.0, 12.0), Vec2::new(150.0, 8.0)),
                    4.0,
                    theme::HAIRLINE,
                );
                painter.rect_filled(
                    Rect::from_min_size(rect.min + Vec2::new(12.0, 26.0), Vec2::new(96.0, 6.0)),
                    3.0,
                    theme::HAIRLINE,
                );
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(rect.right() - 90.0, rect.center().y - 10.0),
                        Vec2::new(78.0, 20.0),
                    ),
                    theme::RADIUS_BADGE,
                    theme::HAIRLINE,
                );
                ui.add_space(theme::SPACE_1);
            }
            ui.add(egui::Label::new(theme::job_meta(
                self.text(Key::BusyScanning),
            )));
            return;
        }
        if self.scan_entries.is_empty() {
            ui.add(egui::Label::new(theme::job_meta(self.text(Key::NoScanResults))).wrap());
            return;
        }
        let entries = self.scan_entries.clone();
        let mut selected = None;
        let mut connect_now = None;
        for entry in &entries {
            let is_selected = self.selected_address.as_deref() == Some(entry.address.as_str());
            let response = device_row(ui, self.lang, entry, is_selected, self.anim(0.10));
            if activated(ui, &response) {
                selected = Some(entry.address.clone());
            }
            if response.double_clicked() {
                connect_now = Some(entry.is_app.then(|| entry.address.clone()));
            }
            ui.add_space(theme::SPACE_1);
        }
        if let Some(address) = selected {
            self.selected_address = Some(address);
        }
        if let Some(address) = connect_now {
            let busy = if self.device.is_some() {
                Busy::Reconnecting
            } else {
                Busy::Connecting
            };
            self.send(Command::Connect { address }, Some(busy));
        }
        ui.add_space(theme::SPACE_1);
        ui.add(egui::Label::new(theme::job_meta(self.text(Key::DeviceRowHint))).wrap());
    }

    fn input_tab(&mut self, ui: &mut egui::Ui) {
        let Some(input) = self.input else {
            self.empty_state(ui, Tab::Input);
            return;
        };

        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = theme::LINE_HEIGHT_META;
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(theme::job_meta(&i18n::format(
                    self.lang,
                    Key::ReportsRate,
                    &[format!("{:.1}", self.reports_per_second)],
                ))));
                if let Some(last) = self.last_input_at {
                    ui.add_space(theme::SPACE_3);
                    ui.add(egui::Label::new(theme::job_meta(&i18n::format(
                        self.lang,
                        Key::LastReportAgo,
                        &[format!("{:.0}", last.elapsed().as_secs_f64() * 1000.0)],
                    ))));
                }
                ui.add_space(theme::SPACE_3);
                ui.add(egui::Label::new(theme::job_meta(&i18n::format(
                    self.lang,
                    Key::BatteryRaw,
                    &[input.battery.to_string()],
                ))));
            });
        });
        ui.add_space(theme::SPACE_4);

        ui.horizontal(|ui| {
            input_ring(
                ui,
                self.lang,
                &self.left_trail,
                Some(input.left_stick),
                i18n::t(self.lang, Key::LeftStick),
            );
            ui.add_space(theme::SPACE_5);
            input_ring(
                ui,
                self.lang,
                &self.right_trail,
                Some(input.right_stick),
                i18n::t(self.lang, Key::RightStick),
            );
        });
        ui.add_space(theme::SPACE_3);
        trigger_bar(
            ui,
            &i18n::format(self.lang, Key::TriggerL, &[input.trigger_l.to_string()]),
            input.trigger_l,
        );
        ui.add_space(theme::SPACE_2);
        trigger_bar(
            ui,
            &i18n::format(self.lang, Key::TriggerR, &[input.trigger_r.to_string()]),
            input.trigger_r,
        );

        ui.add_space(theme::SPACE_5);
        section_label(ui, self.lang, Key::Buttons);
        ui.add_space(theme::SPACE_2);
        ui.horizontal_wrapped(|ui| {
            for (engraved, flag) in BUTTON_LABELS {
                let active = input.keys.contains(flag);
                let label = i18n::button_name(self.lang, engraved);
                let text = RichText::new(format!(" {label} "))
                    .size(theme::FONT_META)
                    .color(if active {
                        theme::LED
                    } else {
                        theme::TEXT_MUTED
                    })
                    .background_color(if active {
                        theme::SELECT_TINT
                    } else {
                        theme::SURFACE_2
                    });
                ui.label(text);
            }
        });

        ui.add_space(theme::SPACE_5);
        data_grid(
            ui,
            &[
                (
                    self.text(Key::NormalizedLeft).to_owned(),
                    i18n::format(
                        self.lang,
                        Key::StickRaw,
                        &[
                            input.left_stick.0.to_string(),
                            input.left_stick.1.to_string(),
                        ],
                    ),
                    ValueKind::Data,
                ),
                (
                    self.text(Key::NormalizedRight).to_owned(),
                    i18n::format(
                        self.lang,
                        Key::StickRaw,
                        &[
                            input.right_stick.0.to_string(),
                            input.right_stick.1.to_string(),
                        ],
                    ),
                    ValueKind::Data,
                ),
                (
                    self.text(Key::KeyMask).to_owned(),
                    format!(
                        "{} ({})",
                        format_u32_hex(input.keys.bits()),
                        i18n::keymap_text(self.lang, input.keys)
                    ),
                    ValueKind::Data,
                ),
            ],
        );
    }

    fn config_tab(&mut self, ui: &mut egui::Ui) {
        if self.config.is_none() {
            self.empty_state(ui, Tab::Config);
            return;
        }

        if self.stale_config() {
            self.stale_warning(ui, Key::LastSessionWarning);
            ui.add_space(theme::SPACE_4);
        }

        let lang = self.lang;
        let Some(config) = self.config.as_ref() else {
            return;
        };

        section_label(ui, lang, Key::SectionSummary);
        ui.add_space(theme::SPACE_2);
        let crc_notice = if config.crc_valid {
            i18n::t(lang, Key::CrcMatch)
        } else {
            i18n::t(lang, Key::CrcMismatch)
        };
        let flags = config
            .slot_flags
            .iter()
            .map(|flag| format_u32_hex(*flag))
            .collect::<Vec<_>>()
            .join(", ");
        let summary = [
            (
                i18n::t(lang, Key::SizeLabel).to_owned(),
                i18n::format(lang, Key::BytesValue, &[config.size.to_string()]),
                ValueKind::Data,
            ),
            (
                i18n::t(lang, Key::Sha256).to_owned(),
                config.sha256.clone(),
                ValueKind::Data,
            ),
            (
                i18n::t(lang, Key::CrcStored).to_owned(),
                format!("0x{:04X}", config.stored_crc),
                ValueKind::Data,
            ),
            (
                i18n::t(lang, Key::CrcInPlace).to_owned(),
                format!("0x{:04X} ({crc_notice})", config.computed_crc),
                ValueKind::Data,
            ),
            (
                i18n::t(lang, Key::ActiveProfile).to_owned(),
                i18n::active_profile_text(lang, config.cur_slot_raw),
                ValueKind::Text,
            ),
            (
                i18n::t(lang, Key::GamepadMode).to_owned(),
                i18n::format(
                    lang,
                    Key::GamepadModeWithRaw,
                    &[
                        i18n::gamepad_mode_text(lang, config.gamepad_mode_raw),
                        config.gamepad_mode_raw.to_string(),
                    ],
                ),
                ValueKind::Text,
            ),
            (
                i18n::t(lang, Key::ProfileFlags).to_owned(),
                format!("[{flags}]"),
                ValueKind::Data,
            ),
        ];
        data_grid(ui, &summary);
        ui.add_space(theme::SPACE_2);
        ui.add(egui::Label::new(theme::job_meta(i18n::t(lang, Key::CrcNote))).wrap());

        ui.add_space(theme::SPACE_5);
        section_label(ui, lang, Key::SectionProfiles);
        ui.add_space(theme::SPACE_2);
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = theme::LINE_HEIGHT_CONTROL;
            for (index, name) in config.profile_names.iter().enumerate() {
                let shown = if name.is_empty() {
                    i18n::t(lang, Key::EmptyProfileName)
                } else {
                    name.as_str()
                };
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(theme::text_job(
                        &i18n::profile_text(lang, index as u8 + 1),
                        theme::medium(theme::FONT_BODY),
                        theme::TEXT,
                        0.0,
                        Some(theme::LINE_HEIGHT_CONTROL),
                    )));
                    ui.add_space(theme::SPACE_3);
                    ui.add(egui::Label::new(theme::job_body(shown)));
                    let flag = format_u32_hex(config.slot_flags[index]);
                    let flag_width = measure(ui, &flag, FontId::monospace(theme::FONT_DATA));
                    let remaining = ui.available_width() - flag_width;
                    if remaining > 0.0 {
                        ui.add_space(remaining);
                    }
                    ui.add(egui::Label::new(theme::job_data(&flag, theme::TEXT_MUTED)));
                });
            }
        });

        ui.add_space(theme::SPACE_5);
        section_label(ui, lang, Key::SectionMacros);
        ui.add_space(theme::SPACE_2);
        ui.add(egui::Label::new(theme::job_meta(i18n::t(lang, Key::MacroHeaderHint))).wrap());
        ui.add_space(theme::SPACE_1);
        for slot in &config.macro_slots {
            macro_slot(ui, lang, slot);
        }
    }

    // -----------------------------------------------------------------------
    // Settings tab (write path)
    // -----------------------------------------------------------------------

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        let Some(snapshot) = self.config_snapshot.clone() else {
            self.empty_state(ui, Tab::Settings);
            return;
        };
        let Some(mut settings) = self.settings.take() else {
            self.empty_state(ui, Tab::Settings);
            return;
        };
        if self.stale_config() {
            self.stale_warning(ui, Key::SettingsStale);
            ui.add_space(theme::SPACE_4);
        }

        let config = snapshot.config.clone();
        let lang = self.lang;
        let edit_enabled = self.can_edit_settings();
        let enabled = self.can_write();
        let mut slot = self.settings_slot;
        ui.add_enabled_ui(edit_enabled, |ui| {
            settings_editor(ui, lang, &config, &mut settings, &mut slot);
        });
        self.settings = Some(settings);
        self.settings_slot = slot;

        if !self.can_write() {
            let reason = if self.macro_editor.recording {
                Key::RecordingBlocksWrite
            } else if self.connection.is_connected() {
                Key::BusyHint
            } else {
                Key::ConnectFirstHint
            };
            ui.add_space(theme::SPACE_2);
            ui.add(egui::Label::new(theme::job_meta(self.text(reason))).wrap());
        }

        ui.add_space(theme::SPACE_4);
        let (changes, valid) = self
            .settings
            .as_ref()
            .map(|settings| {
                (
                    settings.changes(&config).len(),
                    settings.validate(&config).is_ok(),
                )
            })
            .unwrap_or((0, false));
        let mut review = false;
        let mut discard = false;
        let review_label = self.text(Key::ApplySettings).to_owned();
        let discard_label = self.text(Key::DiscardChanges).to_owned();
        ui.horizontal(|ui| {
            if secondary_key(
                ui,
                "settings-review",
                enabled && changes > 0 && valid,
                &review_label,
                theme::BG,
            )
            .on_hover_text(self.text(Key::SettingsDirtyHint))
            .clicked()
            {
                review = true;
            }
            if secondary_key(
                ui,
                "settings-discard",
                enabled && changes > 0,
                &discard_label,
                theme::BG,
            )
            .clicked()
            {
                discard = true;
            }
            if changes > 0 {
                ui.add(egui::Label::new(theme::job_meta(&i18n::format(
                    self.lang,
                    Key::ConfirmChangeCount,
                    &[changes.to_string()],
                ))));
            }
        });
        if review {
            self.review_settings();
        }
        if discard {
            self.reset_settings();
        }
        self.draft_error_line(ui);
        self.write_backup_line(ui);
    }

    /// Show the most recent pre-write backup path and SHA-256, so the backup is
    /// visible on the editing tab while a write is running.
    fn write_backup_line(&mut self, ui: &mut egui::Ui) {
        let Some(backup) = self.last_write_backup.clone() else {
            return;
        };
        ui.add_space(theme::SPACE_3);
        ui.add(egui::Label::new(theme::job_meta(&i18n::format(
            self.lang,
            Key::WriteBackup,
            &[
                backup.config_path.display().to_string(),
                backup.config_sha256.clone(),
            ],
        ))));
        if let Some(dir) = backup.macro_dir {
            ui.add(egui::Label::new(theme::job_meta(&i18n::format(
                self.lang,
                Key::WriteMacroBackup,
                &[dir.display().to_string(), backup.config_sha256.clone()],
            ))));
        }
    }

    /// One red line describing why the draft cannot be written.
    fn draft_error_line(&mut self, ui: &mut egui::Ui) {
        let Some(error) = self.draft_error.as_ref() else {
            return;
        };
        let text = i18n::draft_error_text(self.lang, error);
        ui.add_space(theme::SPACE_2);
        ui.add(egui::Label::new(theme::job_body_color(&text, theme::RED)).wrap());
    }

    // -----------------------------------------------------------------------
    // Macro tab (write path)
    // -----------------------------------------------------------------------

    fn macro_tab(&mut self, ui: &mut egui::Ui) {
        let Some(config) = self.config.clone() else {
            self.empty_state(ui, Tab::Macro);
            return;
        };
        if self.stale_config() {
            self.stale_warning(ui, Key::SettingsStale);
            ui.add_space(theme::SPACE_4);
        }
        let lang = self.lang;
        let mut editor = std::mem::take(&mut self.macro_editor);
        let signals = macro_editor_ui(
            ui,
            lang,
            &config,
            &mut editor,
            self.live,
            self.can_read(),
            self.can_write(),
        );
        self.macro_editor = editor;

        if signals.read {
            self.request_read_macro();
        }
        if signals.start_live {
            self.send(Command::StartLive, None);
            self.macro_editor.begin_recording();
        }
        if signals.record {
            self.macro_editor.begin_recording();
        }
        if signals.stop_record {
            self.macro_editor.stop_recording();
        }
        if signals.review {
            self.review_macro();
        }
        self.draft_error_line(ui);
        self.write_backup_line(ui);
    }

    // -----------------------------------------------------------------------
    // Write result and failure
    // -----------------------------------------------------------------------

    fn write_result_banner(&mut self, ui: &mut egui::Ui) {
        let Some(result) = self.write_result.clone() else {
            return;
        };
        let lang = self.lang;
        let mut close = false;
        Frame::none()
            .fill(theme::SURFACE_2)
            .rounding(theme::RADIUS_KEY)
            .inner_margin(Margin::symmetric(theme::SPACE_3, 12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (bar, _) = ui.allocate_exact_size(Vec2::new(2.0, 22.0), Sense::hover());
                    ui.painter().rect_filled(bar, 1.0, theme::LED);
                    ui.add_space(theme::SPACE_2);
                    ui.add(egui::Label::new(theme::job_body_color(
                        i18n::t(lang, Key::WriteSuccess),
                        theme::TEXT,
                    )));
                });
                ui.add_space(theme::SPACE_2);
                match &result {
                    WriteOutcome::Config(report) => {
                        write_banner_row(
                            ui,
                            lang,
                            Key::WriteBackup,
                            &[
                                report.backup.raw_path.display().to_string(),
                                report.backup.sha256.clone(),
                            ],
                        );
                        write_banner_row(
                            ui,
                            lang,
                            Key::WritePackets,
                            &[report.packets_sent.to_string()],
                        );
                        write_banner_row(
                            ui,
                            lang,
                            Key::WriteChangedRegions,
                            &[report.changed.len().to_string()],
                        );
                        let crc = if report.crc.is_valid() {
                            i18n::t(lang, Key::WriteCrcOk)
                        } else {
                            i18n::t(lang, Key::WriteCrcMismatch)
                        };
                        ui.add(egui::Label::new(theme::job_meta(crc)));
                    }
                    WriteOutcome::Macro(report) => {
                        write_banner_row(
                            ui,
                            lang,
                            Key::WriteMacroBackup,
                            &[
                                report.backup.dir.display().to_string(),
                                report.backup.config_sha256.clone(),
                            ],
                        );
                        ui.add(egui::Label::new(theme::job_meta(&i18n::format(
                            lang,
                            Key::WriteMacroVerified,
                            &[
                                report.slot.number().to_string(),
                                (report.index.index() + 1).to_string(),
                                report.steps.to_string(),
                            ],
                        ))));
                        write_banner_row(
                            ui,
                            lang,
                            Key::WritePackets,
                            &[report.packets_sent.to_string()],
                        );
                    }
                }
                ui.add_space(theme::SPACE_2);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), 32.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        let label = i18n::t(lang, Key::WriteCloseResult).to_owned();
                        if secondary_key(ui, "result-close", true, &label, theme::SURFACE_2)
                            .clicked()
                        {
                            close = true;
                        }
                        let copy = i18n::t(lang, Key::CopySha256).to_owned();
                        let sha = match &result {
                            WriteOutcome::Config(report) => report.backup.sha256.clone(),
                            WriteOutcome::Macro(report) => report.backup.config_sha256.clone(),
                        };
                        if secondary_key(ui, "result-copy", true, &copy, theme::SURFACE_2).clicked()
                        {
                            ui.output_mut(|output| output.copied_text = sha);
                        }
                    },
                );
            });
        if close {
            self.write_result = None;
        }
    }

    fn write_failure_banner(&mut self, ui: &mut egui::Ui) {
        let Some(failure) = self.write_failure.clone() else {
            return;
        };
        let lang = self.lang;
        let mut close = false;
        let headline = match failure.phase {
            WritePhase::Read => Key::WriteReadFailed,
            WritePhase::Prepare => Key::WritePrepareFailed,
            WritePhase::Backup => Key::WriteBackupFailed,
            WritePhase::ConfigChanged => Key::WriteChanged,
            WritePhase::Device => Key::WriteDeviceFailed,
        };
        Frame::none()
            .fill(theme::SURFACE_2)
            .rounding(theme::RADIUS_KEY)
            .inner_margin(Margin::symmetric(theme::SPACE_3, 12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (bar, _) = ui.allocate_exact_size(Vec2::new(2.0, 22.0), Sense::hover());
                    ui.painter().rect_filled(bar, 1.0, theme::RED);
                    ui.add_space(theme::SPACE_2);
                    ui.add(egui::Label::new(theme::job_body_color(
                        i18n::t(lang, Key::WriteFailureTitle),
                        theme::RED,
                    )));
                });
                ui.add_space(theme::SPACE_1);
                ui.add(egui::Label::new(theme::job_body(i18n::t(lang, headline))).wrap());
                if let Some(backup) = &failure.backup {
                    ui.add_space(theme::SPACE_2);
                    ui.add(egui::Label::new(theme::job_meta(i18n::t(
                        lang,
                        Key::WriteFailureBackup,
                    ))));
                    ui.add(egui::Label::new(theme::job_data(
                        &backup.config_path.display().to_string(),
                        theme::TEXT_BODY,
                    )));
                    ui.add(egui::Label::new(theme::job_data(
                        &backup.config_sha256,
                        theme::TEXT_MUTED,
                    )));
                    if let Some(dir) = &backup.macro_dir {
                        ui.add(egui::Label::new(theme::job_data(
                            &dir.display().to_string(),
                            theme::TEXT_BODY,
                        )));
                    }
                    ui.add(egui::Label::new(theme::job_meta(i18n::t(
                        lang,
                        Key::WriteFailureRecovery,
                    ))));
                    ui.add(
                        egui::Label::new(theme::job_body(i18n::t(
                            lang,
                            Key::WriteFailureRecoveryText,
                        )))
                        .wrap(),
                    );
                    if backup.macro_dir.is_some() {
                        ui.add(
                            egui::Label::new(theme::job_meta(i18n::t(
                                lang,
                                Key::WriteFailureMacroRestore,
                            )))
                            .wrap(),
                        );
                    } else {
                        ui.add(
                            egui::Label::new(theme::job_meta(i18n::t(
                                lang,
                                Key::WriteFailureConfigRestore,
                            )))
                            .wrap(),
                        );
                    }
                }
                if let Some(recovery) = &failure.recovery {
                    egui::CollapsingHeader::new(theme::job_meta(i18n::t(lang, Key::ErrorDetails)))
                        .id_salt("write-failure-detail")
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(theme::job_data(
                                recovery,
                                theme::TEXT_MUTED,
                            )));
                            ui.add(egui::Label::new(theme::job_data(
                                &failure.detail,
                                theme::TEXT_MUTED,
                            )));
                        });
                } else {
                    ui.add(egui::Label::new(theme::job_data(
                        &failure.detail,
                        theme::TEXT_MUTED,
                    )));
                }
                ui.add_space(theme::SPACE_1);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), 32.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        let label = i18n::t(lang, Key::Dismiss).to_owned();
                        if secondary_key(ui, "failure-close", true, &label, theme::SURFACE_2)
                            .clicked()
                        {
                            close = true;
                        }
                    },
                );
            });
        if close {
            self.write_failure = None;
        }
    }

    // -----------------------------------------------------------------------
    // Confirmation dialog
    // -----------------------------------------------------------------------

    /// Non-modal confirmation dialog.
    ///
    /// Background controls stay interactive, so the actual write is re-guarded
    /// at confirm time: `can_write` (connection, busy, recording), the reviewed
    /// target (not the current editor selection), and the expected configuration
    /// SHA-256 in the worker. The dialog itself is only the review surface.
    fn write_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        let lang = self.lang;
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new(i18n::t(lang, Key::ConfirmTitle))
            .id(Id::new("write-confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(520.0);
                write_dialog_target(ui, lang, &pending.target);
                ui.add_space(theme::SPACE_3);
                ui.add(
                    egui::Label::new(theme::job_meta(i18n::t(lang, Key::ConfirmBackupNote))).wrap(),
                );
                ui.add(egui::Label::new(theme::job_meta(&i18n::format(
                    lang,
                    Key::ConfirmBackupDir,
                    &[self.backup_dir.display().to_string()],
                ))));
                ui.add_space(theme::SPACE_2);
                ui.add(
                    egui::Label::new(theme::job_meta_color(
                        i18n::t(lang, Key::ConfirmWarning),
                        theme::AMBER,
                    ))
                    .wrap(),
                );
                if self.macro_editor.recording {
                    ui.add(
                        egui::Label::new(theme::job_meta_color(
                            i18n::t(lang, Key::RecordingBlocksWrite),
                            theme::AMBER,
                        ))
                        .wrap(),
                    );
                }
                ui.add_space(theme::SPACE_3);
                ui.horizontal(|ui| {
                    let confirm_label = i18n::t(lang, Key::WriteConfirm).to_owned();
                    if key(
                        ui,
                        "write-confirm-yes",
                        self.can_write(),
                        KeyKind::Primary,
                        &confirm_label,
                        0.10,
                        theme::SURFACE_2,
                    )
                    .clicked()
                    {
                        confirm = true;
                    }
                    let cancel_label = i18n::t(lang, Key::WriteCancel).to_owned();
                    if secondary_key(
                        ui,
                        "write-confirm-no",
                        true,
                        &cancel_label,
                        theme::SURFACE_2,
                    )
                    .clicked()
                    {
                        cancel = true;
                    }
                });
            });
        if confirm {
            self.confirm_write(pending);
        } else if cancel {
            self.pending = None;
        }
    }

    fn log_tab(&mut self, ui: &mut egui::Ui) {
        if self.log.is_empty() {
            self.empty_state(ui, Tab::Log);
            return;
        }
        for entry in &self.log {
            let color = match entry.level {
                LogLevel::Info => theme::TEXT_BODY,
                LogLevel::Error => theme::RED,
            };
            let text = self.notice_text(&entry.notice);
            let detail = entry.notice.detail.clone();
            let width = ui.available_width();
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(width, LOG_ROW_HEIGHT), Sense::hover());
            let mut row = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            row.add_sized(
                [62.0, 18.0],
                egui::Label::new(theme::job_data(&entry.at, theme::TEXT_MUTED)),
            );
            if entry.level == LogLevel::Error {
                let (dot, _) = row.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
                row.painter().circle_filled(dot.center(), 3.0, theme::RED);
                row.add_space(theme::SPACE_1);
            }
            let label = row.add(
                egui::Label::new(theme::text_job(
                    &text,
                    theme::regular(theme::FONT_BODY),
                    color,
                    0.0,
                    Some(theme::LINE_HEIGHT_CONTROL),
                ))
                .truncate(),
            );
            if let Some(detail) = detail {
                label.on_hover_text(detail);
            }
            let _ = response;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        if !self.worker.is_running()
            && !self.connection.is_connected()
            && !self.log.iter().any(|entry| {
                entry.notice.key == Key::LogWorkerStopped && entry.level == LogLevel::Error
            })
        {
            self.connection = ConnectionState::Disconnected;
            self.live = false;
            self.input = None;
            self.busy = None;
            self.push_notice(LogLevel::Error, Notice::new(Key::LogWorkerStopped));
        }
        self.draw(ctx);

        let interval = if self.connection == ConnectionState::Scanning {
            16
        } else if self.busy.is_some() || self.live {
            50
        } else {
            250
        };
        ctx.request_repaint_after(Duration::from_millis(interval));
    }
}

impl App {
    fn draw(&mut self, ctx: &egui::Context) {
        self.top_bar(ctx);
        self.side_panel(ctx);
        self.central_panel(ctx);
        self.write_dialog(ctx);
        self.licenses_dialog(ctx);
    }

    /// Licenses / open-source screen.
    ///
    /// Binary distribution must ship the OFL / UFL / MIT texts for the bundled
    /// fonts, so they are embedded with `include_str!` (see [`crate::licenses`])
    /// and shown here. `docs/DISTRIBUTION_POLICY.md` §4.4 keeps this list and
    /// `THIRD_PARTY_LICENSES.md` in sync.
    fn licenses_dialog(&mut self, ctx: &egui::Context) {
        if !self.licenses_open {
            return;
        }
        let lang = self.lang;
        let mut close = false;
        let mut open = true;
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.licenses_open = false;
            return;
        }
        egui::Window::new(i18n::t(lang, Key::LicensesTitle))
            .id(Id::new("licenses"))
            .collapsible(false)
            .resizable(false)
            .vscroll(false)
            .open(&mut open)
            // A centred window taller than the viewport clips both its title
            // bar and the close key, so cap it at the screen height.
            .max_height((ctx.screen_rect().height() - 32.0).max(260.0))
            .max_width(760.0)
            .default_width(720.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add(egui::Label::new(theme::job_meta(i18n::t(lang, Key::LicensesIntro))).wrap());
                ui.add_space(theme::SPACE_3);
                section_label(ui, lang, Key::LicensesAppHeading);
                ui.add(
                    egui::Label::new(theme::job_meta(i18n::t(lang, Key::LicensesAppBody))).wrap(),
                );
                ui.add_space(theme::SPACE_3);
                section_label(ui, lang, Key::LicensesNotoHeading);
                ui.add(
                    egui::Label::new(theme::job_meta(i18n::t(lang, Key::LicensesNotoNote))).wrap(),
                );
                ui.add_space(theme::SPACE_2);
                // The Noto Sans JP holder line belongs under the Noto heading;
                // the remaining holders belong to the egui default fonts.
                for (font, holder) in crate::licenses::ATTRIBUTIONS
                    .iter()
                    .filter(|(font, _)| font.starts_with("Noto Sans JP"))
                {
                    ui.add(egui::Label::new(theme::job_meta(&format!("{font}: {holder}"))).wrap());
                }
                ui.add_space(theme::SPACE_3);
                section_label(ui, lang, Key::LicensesEguiHeading);
                ui.add(
                    egui::Label::new(theme::job_meta(i18n::t(lang, Key::LicensesEguiNote))).wrap(),
                );
                ui.add_space(theme::SPACE_2);
                for (font, holder) in crate::licenses::ATTRIBUTIONS
                    .iter()
                    .filter(|(font, _)| !font.starts_with("Noto Sans JP"))
                {
                    ui.add(egui::Label::new(theme::job_meta(&format!("{font}: {holder}"))).wrap());
                }
                ui.add_space(theme::SPACE_3);
                // The header and the close key are fixed; only the text
                // viewport shrinks in short windows so the close key stays
                // reachable. The viewport height is the stable input here:
                // inside an auto-sized window the available height depends on
                // the size this frame has not settled on yet.
                let scroll_height = (ctx.screen_rect().height() - 560.0).clamp(96.0, 240.0);
                egui::ScrollArea::vertical()
                    .id_salt("licenses-scroll")
                    .max_height(scroll_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for license in crate::licenses::ALL {
                            ui.add_space(theme::SPACE_2);
                            ui.add(egui::Label::new(theme::text_job(
                                license.name,
                                theme::medium(theme::FONT_BODY),
                                theme::TEXT,
                                0.0,
                                None,
                            )));
                            ui.add_space(theme::SPACE_1);
                            ui.add(
                                egui::Label::new(theme::job_meta(license.body))
                                    .wrap()
                                    .selectable(true),
                            );
                        }
                    });
                ui.add_space(theme::SPACE_3);
                if secondary_key(
                    ui,
                    "licenses-close",
                    true,
                    i18n::t(lang, Key::LicensesClose),
                    theme::SURFACE_2,
                )
                .clicked()
                {
                    close = true;
                }
            });
        if close || !open {
            self.licenses_open = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Small drawing helpers
// ---------------------------------------------------------------------------

fn measure(ui: &egui::Ui, text: &str, font: FontId) -> f32 {
    ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
            .size()
            .x
    })
}

fn paint_job_centered(painter: &egui::Painter, center: Pos2, job: egui::text::LayoutJob) {
    let galley = painter.layout_job(job);
    let rect = Align2::CENTER_CENTER.anchor_size(center, galley.size());
    painter.galley(rect.min, galley, Color32::WHITE);
}

fn paint_job_left_center(painter: &egui::Painter, left_center: Pos2, job: egui::text::LayoutJob) {
    let galley = painter.layout_job(job);
    let rect = Align2::LEFT_CENTER.anchor_size(left_center, galley.size());
    painter.galley(rect.min, galley, Color32::WHITE);
}

fn focus_ring(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        let painter = ui.painter();
        painter.rect_stroke(
            response.rect.expand(3.0),
            theme::RADIUS_KEY + 2.0,
            Stroke::new(1.0_f32, theme::BG),
        );
        painter.rect_stroke(
            response.rect.expand(1.0),
            theme::RADIUS_KEY + 1.0,
            Stroke::new(theme::FOCUS_RING_WIDTH, theme::LED),
        );
    }
}

fn activated(ui: &egui::Ui, response: &egui::Response) -> bool {
    response.clicked()
        || (response.has_focus()
            && ui.input(|input| {
                input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
            }))
}

fn status_dot(ui: &mut egui::Ui, state: ConnectionState, scan_angle: Option<f32>, glow: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(theme::STATUS_RING + 8.0), Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    match state {
        ConnectionState::Disconnected => {
            painter.circle_stroke(center, 4.0, Stroke::new(1.5_f32, theme::LED_DIM));
            painter.circle_filled(center, 1.2, theme::LED_DIM);
        }
        ConnectionState::Connecting => {
            painter.circle_filled(center, 3.0, theme::AMBER);
        }
        ConnectionState::Connected => {
            for (radius, alpha) in [(12.0, 0.03), (8.5, 0.06), (5.5, 0.10)] {
                painter.circle_filled(center, radius, theme::LED.linear_multiply(alpha * glow));
            }
            painter.circle_filled(center, 4.0, theme::LED.linear_multiply(glow.max(0.3)));
        }
        ConnectionState::Scanning => {
            painter.circle_stroke(center, 4.0, Stroke::new(1.0_f32, theme::LED_DIM));
            if let Some(angle) = scan_angle {
                let points: Vec<Pos2> = (0..=6)
                    .map(|index| {
                        let a = angle + (index as f32 / 6.0) * FRAC_PI_3;
                        center + Vec2::new(a.cos(), a.sin()) * 4.5
                    })
                    .collect();
                painter.add(egui::Shape::line(
                    points,
                    Stroke::new(2.0_f32, theme::AMBER),
                ));
            }
        }
    }
}

fn large_ring(
    ui: &mut egui::Ui,
    rect: Rect,
    lit: bool,
    glow: f32,
    scan_angle: Option<f32>,
    label: &str,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) / 2.0 - 4.0;
    let painter = ui.painter();
    painter.circle_stroke(center, radius, Stroke::new(1.5_f32, theme::LED_DIM));
    if lit {
        for (r, alpha) in [(30.0, 0.03), (22.0, 0.06), (14.0, 0.10)] {
            painter.circle_filled(center, r, theme::LED.linear_multiply(alpha * glow));
        }
        painter.circle_filled(center, 5.0, theme::LED.linear_multiply(glow.max(0.3)));
    }
    if let Some(angle) = scan_angle {
        let points: Vec<Pos2> = (0..=12)
            .map(|index| {
                let a = angle + (index as f32 / 12.0) * FRAC_PI_3;
                center + Vec2::new(a.cos(), a.sin()) * radius
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(3.0_f32, theme::AMBER),
        ));
    }
    let response = ui.interact(rect, ui.id().with("empty-ring"), Sense::hover());
    let label_owned = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label_owned)
    });
}

/// Small unlit ring used as a step marker; only the current step lights up.
fn step_indicator(ui: &mut egui::Ui, current: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    let painter = ui.painter();
    painter.circle_stroke(rect.center(), 3.5, Stroke::new(1.2_f32, theme::LED_DIM));
    if current {
        painter.circle_filled(rect.center(), 1.8, theme::LED);
    }
}

fn section_label(ui: &mut egui::Ui, lang: Lang, key: Key) {
    ui.add(egui::Label::new(theme::job_section(i18n::t(lang, key))));
}

fn rail_divider(ui: &mut egui::Ui) {
    ui.add_space(theme::SPACE_4);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::HAIRLINE);
    ui.add_space(theme::SPACE_3);
}

// Painting a key needs its geometry, identity, enabled/selected state, label,
// animation time, and the surface it sits on; grouping them into a struct only
// moves the arguments around.
#[allow(clippy::too_many_arguments)]
fn key_at(
    ui: &mut egui::Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash,
    enabled: bool,
    kind: KeyKind,
    label: &str,
    animation: f32,
    surface: Color32,
) -> egui::Response {
    let id = ui.id().with(id_salt);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    let hover =
        ui.ctx()
            .animate_bool_with_time(id.with("hover"), enabled && response.hovered(), animation);
    let pressed = enabled && response.is_pointer_button_down_on();
    let painter = ui.painter();
    if enabled {
        match kind {
            KeyKind::Primary => {
                let fill = if pressed {
                    theme::KEY_LIGHT_PRESSED
                } else {
                    theme::lerp_color(theme::KEY_LIGHT, theme::KEY_LIGHT_HOVER, hover)
                };
                painter.rect(rect, theme::RADIUS_KEY, fill, Stroke::NONE);
            }
            KeyKind::Secondary => {
                // Secondary keys are transparent at rest; the hover fill fades
                // in from the surface they sit on, not from a fixed dark tone.
                let stroke = Stroke::new(1.0_f32, theme::CONTROL_LINE);
                if hover > 0.001 {
                    let fill = theme::lerp_color(surface, theme::SURFACE_2, hover);
                    let alpha = (hover * 255.0).round().clamp(0.0, 255.0) as u8;
                    painter.rect(
                        rect,
                        theme::RADIUS_KEY,
                        Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), alpha),
                        stroke,
                    );
                } else {
                    painter.rect_stroke(rect, theme::RADIUS_KEY, stroke);
                }
            }
        }
        let color = match kind {
            KeyKind::Primary => theme::ON_KEY_LIGHT,
            KeyKind::Secondary => theme::TEXT_BODY,
        };
        paint_job_centered(painter, rect.center(), theme::job_control(label, color));
        focus_ring(ui, &response);
    } else {
        paint_job_centered(
            painter,
            rect.center(),
            theme::job_control(label, theme::TEXT_DISABLED),
        );
    }
    let enabled_for_info = enabled;
    let label_owned = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled_for_info, &label_owned)
    });
    response
}

fn key(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    enabled: bool,
    kind: KeyKind,
    label: &str,
    animation: f32,
    surface: Color32,
) -> egui::Response {
    let min_width = match kind {
        KeyKind::Primary => theme::KEY_MIN_WIDTH,
        KeyKind::Secondary => 0.0,
    };
    let width =
        (measure(ui, label, theme::medium(theme::FONT_CONTROL)) + theme::SPACE_5).max(min_width);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, theme::KEY_HEIGHT), Sense::hover());
    key_at(ui, rect, id_salt, enabled, kind, label, animation, surface)
}

fn secondary_key(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    enabled: bool,
    label: &str,
    surface: Color32,
) -> egui::Response {
    key(
        ui,
        id_salt,
        enabled,
        KeyKind::Secondary,
        label,
        0.10,
        surface,
    )
}

fn secondary_key_at(
    ui: &mut egui::Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash,
    enabled: bool,
    label: &str,
    animation: f32,
    surface: Color32,
) -> egui::Response {
    key_at(
        ui,
        rect,
        id_salt,
        enabled,
        KeyKind::Secondary,
        label,
        animation,
        surface,
    )
}

fn quiet_row(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    label: &str,
    enabled: bool,
    selected: bool,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, theme::ROW_HEIGHT), Sense::hover());
    let id = ui.id().with(id_salt);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    let hover =
        ui.ctx()
            .animate_bool_with_time(id.with("hover"), enabled && response.hovered(), 0.10);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::RADIUS_TAB, theme::SELECT_TINT);
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
            1.0,
            theme::LED,
        );
    } else if hover > 0.0 {
        painter.rect_filled(
            rect,
            theme::RADIUS_TAB,
            theme::lerp_color(theme::SURFACE_1, theme::SURFACE_2, hover),
        );
    }
    let color = if enabled {
        if selected || hover > 0.5 {
            theme::TEXT
        } else {
            theme::TEXT_BODY
        }
    } else {
        theme::TEXT_DISABLED
    };
    paint_job_left_center(
        painter,
        Pos2::new(rect.left() + 10.0, rect.center().y),
        theme::job_control(label, color),
    );
    focus_ring(ui, &response);
    let enabled_for_info = enabled;
    let label_owned = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            enabled_for_info,
            selected,
            &label_owned,
        )
    });
    response
}

fn device_row(
    ui: &mut egui::Ui,
    lang: Lang,
    entry: &ScanEntry,
    selected: bool,
    animation: f32,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, theme::DEVICE_ROW_HEIGHT), Sense::hover());
    let response = ui.interact(
        rect,
        ui.id().with(("device", &entry.address)),
        Sense::click(),
    );
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), animation);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let fill = if selected {
        theme::SELECT_TINT
    } else {
        theme::lerp_color(theme::BG, theme::SURFACE_2, hover)
    };
    if selected || hover > 0.0 {
        painter.rect_filled(rect, theme::RADIUS_KEY, fill);
    }
    if selected {
        painter.rect_filled(
            Rect::from_min_size(
                rect.min + Vec2::new(0.0, 8.0),
                Vec2::new(2.0, rect.height() - 16.0),
            ),
            1.0,
            theme::LED,
        );
    }

    let badge_text = if entry.is_app {
        i18n::t(lang, Key::AppModeBadge)
    } else {
        i18n::t(lang, Key::OtherModeBadge)
    };
    let badge_width = measure(ui, badge_text, theme::medium(theme::FONT_META)) + 14.0;
    let badge_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 12.0 - badge_width / 2.0, rect.center().y),
        Vec2::new(badge_width, 20.0),
    );

    let name = entry
        .name
        .clone()
        .unwrap_or_else(|| i18n::t(lang, Key::NoName).to_string());
    let text_clip = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, rect.top()),
        Pos2::new(badge_rect.left() - 24.0, rect.bottom()),
    );
    let text_painter = painter.with_clip_rect(text_clip);
    paint_job_left_center(
        &text_painter,
        Pos2::new(rect.left() + 12.0, rect.center().y - 8.0),
        theme::text_job(
            &name,
            theme::medium(theme::FONT_BODY),
            theme::TEXT,
            0.0,
            Some(theme::LINE_HEIGHT_CONTROL),
        ),
    );
    paint_job_left_center(
        &text_painter,
        Pos2::new(rect.left() + 12.0, rect.center().y + 9.0),
        theme::job_data(&entry.address, theme::TEXT_MUTED),
    );

    painter.rect(
        badge_rect,
        theme::RADIUS_BADGE,
        if entry.is_app {
            theme::SELECT_TINT
        } else {
            theme::SURFACE_2
        },
        if entry.is_app {
            Stroke::new(1.0_f32, theme::LED)
        } else {
            Stroke::NONE
        },
    );
    paint_job_centered(
        &painter,
        badge_rect.center(),
        theme::text_job(
            badge_text,
            theme::medium(theme::FONT_META),
            if entry.is_app {
                theme::LED
            } else {
                theme::TEXT_MUTED
            },
            0.0,
            Some(theme::LINE_HEIGHT_META),
        ),
    );

    let bars = rssi_bars(entry.rssi);
    let bar_width = 2.0;
    let bar_gap = 2.0;
    let bars_left = badge_rect.left() - 10.0 - 4.0 * bar_gap - 4.0 * bar_width;
    let baseline = rect.center().y + 6.0;
    for index in 0..4 {
        let height = 3.0 + 2.0 * index as f32;
        let x = bars_left + index as f32 * (bar_width + bar_gap);
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x, baseline - height),
                Pos2::new(x + bar_width, baseline),
            ),
            1.0,
            if index < usize::from(bars) {
                match bars {
                    3 | 4 => theme::LED,
                    2 => theme::AMBER,
                    _ => theme::TEXT_MUTED,
                }
            } else {
                theme::HAIRLINE
            },
        );
    }

    focus_ring(ui, &response);
    let accessible = format!("{name} ({})", entry.address);
    response.widget_info(move || {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            &accessible,
        )
    });
    response.on_hover_text(format_rssi(entry.rssi))
}

fn data_grid(ui: &mut egui::Ui, rows: &[(String, String, ValueKind)]) {
    let label_width = 150.0;
    // A horizontal row otherwise reserves `interact_size.y`, which spreads
    // measurement rows to 40 pt. Keep the grid at the meta line height.
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = theme::LINE_HEIGHT_META;
        for (label, value, kind) in rows {
            let job = match kind {
                ValueKind::Text => theme::text_job(
                    value,
                    theme::regular(theme::FONT_BODY),
                    theme::TEXT_BODY,
                    0.0,
                    Some(theme::LINE_HEIGHT_BODY),
                ),
                ValueKind::Data => theme::job_data(value, theme::TEXT_BODY),
                ValueKind::Strong => theme::job_strong(value),
            };
            if value.chars().count() > 48 {
                // SHA-256 digests and long hex values get the full column width
                // so the wrap point does not depend on the horizontal layout.
                ui.add(egui::Label::new(theme::job_meta(label)));
                ui.add_space(theme::SPACE_1);
                ui.add(egui::Label::new(job).wrap());
            } else {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [label_width, theme::LINE_HEIGHT_META],
                        egui::Label::new(theme::job_meta(label)),
                    );
                    ui.add_space(theme::SPACE_2);
                    ui.add(egui::Label::new(job).wrap());
                });
            }
            ui.add_space(theme::SPACE_1);
        }
    });
}

fn macro_slot(ui: &mut egui::Ui, lang: Lang, slot: &MacroSlotView) {
    let title = i18n::format(
        lang,
        Key::MacroSlotLabel,
        &[
            slot.profile.to_string(),
            format_u32_hex(slot.flag),
            slot.total_cnt.to_string(),
        ],
    );
    egui::CollapsingHeader::new(theme::text_job(
        &title,
        theme::medium(theme::FONT_BODY),
        theme::TEXT_BODY,
        0.0,
        Some(theme::LINE_HEIGHT_CONTROL),
    ))
    .id_salt(("macro-slot", slot.profile))
    .default_open(slot.profile == 1)
    .show(ui, |ui| {
        egui::ScrollArea::horizontal()
            .id_salt(("macro-scroll", slot.profile))
            .show(ui, |ui| macro_slot_table(ui, lang, slot));
    });
}

fn macro_slot_table(ui: &mut egui::Ui, lang: Lang, slot: &MacroSlotView) {
    egui::Grid::new(("macro-table", slot.profile))
        .num_columns(9)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for heading in [
                Key::TableNo,
                Key::TableName,
                Key::TableMode,
                Key::TableSpecial,
                Key::TableSteps,
                Key::TableOffset,
                Key::TableKeyMap,
                Key::TableCycles,
                Key::TableInterval,
            ] {
                ui.add(egui::Label::new(theme::text_job(
                    i18n::t(lang, heading),
                    theme::medium(theme::FONT_META),
                    theme::TEXT_MUTED,
                    theme::TRACKING_SECTION,
                    Some(theme::LINE_HEIGHT_META),
                )));
            }
            ui.end_row();

            for record in &slot.records {
                let name = if record.name.is_empty() {
                    i18n::t(lang, Key::EmptyProfileName).to_owned()
                } else {
                    record.name.clone()
                };
                for value in [
                    record.number.to_string(),
                    name,
                    record.gamepad_mode.to_string(),
                    record.special_flag.to_string(),
                    record.max_steps.to_string(),
                    format_u16_hex(record.offset),
                    record.key_map.clone(),
                    record.cycles.to_string(),
                    i18n::format(lang, Key::IntervalMs, &[record.interval_ms.to_string()]),
                ] {
                    ui.add(egui::Label::new(theme::job_data(&value, theme::TEXT_BODY)));
                }
                ui.end_row();
            }
        });
}

fn input_ring(
    ui: &mut egui::Ui,
    lang: Lang,
    trail: &VecDeque<(f32, f32)>,
    value: Option<(i16, i16)>,
    label: &str,
) {
    ui.vertical(|ui| {
        ui.add(egui::Label::new(theme::job_meta(label)));
        ui.add_space(theme::SPACE_1);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::splat(theme::STICK_DIAMETER), Sense::hover());
        let center = rect.center();
        let radius = theme::STICK_DIAMETER / 2.0 - 4.0;
        let painter = ui.painter();
        painter.circle_stroke(center, radius, Stroke::new(1.5_f32, theme::LED_DIM));
        painter.circle_stroke(center, radius - 8.0, Stroke::new(1.0_f32, theme::HAIRLINE));
        painter.circle_stroke(center, 12.0, Stroke::new(1.0_f32, theme::HAIRLINE));

        if let Some((x, y)) = value {
            let travel = radius - 14.0;
            let to_point = |(nx, ny): (f32, f32)| {
                center + Vec2::new(nx.clamp(-1.0, 1.0) * travel, -ny.clamp(-1.0, 1.0) * travel)
            };
            let points: Vec<Pos2> = trail.iter().copied().map(to_point).collect();
            if points.len() >= 2 {
                for (index, window) in points.windows(2).enumerate() {
                    let alpha = (index + 1) as f32 / points.len() as f32;
                    painter.line_segment(
                        [window[0], window[1]],
                        Stroke::new(
                            1.5_f32,
                            Color32::from_rgba_unmultiplied(
                                0x6F,
                                0xE7,
                                0xD8,
                                (alpha * 140.0) as u8,
                            ),
                        ),
                    );
                }
            }
            let point = center
                + Vec2::new(
                    f32::from(x).clamp(-128.0, 128.0) / 128.0 * travel,
                    -f32::from(y).clamp(-128.0, 128.0) / 128.0 * travel,
                );
            painter.circle_filled(point, 14.0, theme::LED.linear_multiply(0.06));
            painter.circle_filled(point, 6.0, theme::LED);
            let value_text = i18n::format(lang, Key::StickRaw, &[x.to_string(), y.to_string()]);
            ui.monospace(value_text);
        } else {
            ui.add_space(theme::SPACE_1);
            ui.add(egui::Label::new(theme::job_data("-", theme::TEXT_MUTED)));
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, label));
    });
}

fn trigger_bar(ui: &mut egui::Ui, label: &str, value: u8) {
    ui.add(egui::Label::new(theme::job_meta(label)));
    ui.add_space(theme::SPACE_1);
    let width = ui.available_width().min(420.0);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, theme::TRIGGER_BAR_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::HAIRLINE);
    let filled = Rect::from_min_size(
        rect.min,
        Vec2::new(width * f32::from(value) / 255.0, rect.height()),
    );
    painter.rect_filled(filled, 3.0, theme::LED);
}

fn rssi_bars(rssi: Option<i16>) -> u8 {
    match rssi {
        None => 0,
        Some(value) if value >= -50 => 4,
        Some(value) if value >= -62 => 3,
        Some(value) if value >= -74 => 2,
        Some(_) => 1,
    }
}

/// Turn a reviewed [`PendingWrite`] into the worker command.
///
/// The macro target comes from the pending write, not from the current editor
/// selection: the user can still click the profile/macro combo boxes behind the
/// dialog, and the reviewed target must be the written one.
fn pending_command(pending: PendingWrite, backup_dir: PathBuf) -> Option<(Command, Busy)> {
    let PendingWrite {
        target,
        plan,
        expected_config_sha256,
        expected_macro_raw_sha256,
    } = pending;
    match plan {
        PendingPlan::Config(plan) => Some((
            Command::WriteConfig {
                plan,
                backup_dir,
                expected_config_sha256,
            },
            Busy::WriteConfig,
        )),
        PendingPlan::Macro(plan) => match target {
            WriteTarget::Macro { slot, index, .. } => {
                let expected_macro_raw_sha256 = expected_macro_raw_sha256?;
                Some((
                    Command::WriteMacro {
                        slot,
                        index,
                        plan,
                        backup_dir,
                        expected_config_sha256,
                        expected_macro_raw_sha256,
                    },
                    Busy::WriteMacro,
                ))
            }
            WriteTarget::Config { .. } => None,
        },
    }
}

/// Signals from the macro editor to the application shell.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct MacroTabSignals {
    read: bool,
    start_live: bool,
    record: bool,
    stop_record: bool,
    review: bool,
}

/// The 16 buttons a macro step can hold, in bit order.
const MACRO_STEP_KEYS: [(&str, KeyMap); 16] = [
    ("Start", KeyMap::START),
    ("L3", KeyMap::L3),
    ("R3", KeyMap::R3),
    ("Select", KeyMap::SELECT),
    ("X", KeyMap::X),
    ("Y", KeyMap::Y),
    ("Right", KeyMap::RIGHT),
    ("Left", KeyMap::LEFT),
    ("Down", KeyMap::DOWN),
    ("Up", KeyMap::UP),
    ("L1", KeyMap::L1),
    ("R1", KeyMap::R1),
    ("B", KeyMap::B),
    ("A", KeyMap::A),
    ("L2", KeyMap::L2),
    ("R2", KeyMap::R2),
];

fn write_banner_row(ui: &mut egui::Ui, lang: Lang, key: Key, args: &[String]) {
    let text = i18n::format(lang, key, args);
    ui.add(egui::Label::new(theme::job_meta(&text)).wrap());
}

fn write_dialog_target(ui: &mut egui::Ui, lang: Lang, target: &WriteTarget) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [120.0, theme::LINE_HEIGHT_META],
            egui::Label::new(theme::job_meta(i18n::t(lang, Key::ConfirmTarget))),
        );
        match target {
            WriteTarget::Config { changes, focus } => {
                ui.add(egui::Label::new(theme::job_body(i18n::t(
                    lang,
                    Key::ConfirmConfigTarget,
                ))));
                ui.add_space(theme::SPACE_2);
                ui.add(egui::Label::new(theme::job_meta(&i18n::format(
                    lang,
                    Key::ConfirmChangeCount,
                    &[changes.len().to_string()],
                ))));
                let _ = focus;
            }
            WriteTarget::Macro {
                slot,
                index,
                steps,
                total_ms,
                ..
            } => {
                ui.add(egui::Label::new(theme::job_body(&i18n::format(
                    lang,
                    Key::ConfirmMacroTarget,
                    &[slot.number().to_string(), (index.index() + 1).to_string()],
                ))));
                let _ = (steps, total_ms);
            }
        }
    });
    ui.add_space(theme::SPACE_1);
    match target {
        WriteTarget::Config { changes, .. } => {
            if !changes.is_empty() {
                ui.scope(|ui| {
                    ui.spacing_mut().interact_size.y = theme::LINE_HEIGHT_META;
                    for change in changes {
                        ui.add(egui::Label::new(theme::job_data(
                            &i18n::change_text(lang, *change),
                            theme::TEXT_BODY,
                        )));
                    }
                });
            }
        }
        WriteTarget::Macro {
            slot,
            index,
            steps,
            total_ms,
            name,
            trigger,
            cycles,
            interval_ms,
            changes,
        } => {
            ui.add(
                egui::Label::new(theme::job_body(&i18n::format(
                    lang,
                    Key::ConfirmMacroSummary,
                    &[
                        slot.number().to_string(),
                        (index.index() + 1).to_string(),
                        steps.to_string(),
                        total_ms.to_string(),
                    ],
                )))
                .wrap(),
            );
            ui.add_space(theme::SPACE_1);
            ui.add(
                egui::Label::new(theme::job_data(
                    &i18n::format(
                        lang,
                        Key::ConfirmMacroHeader,
                        &[
                            name.clone(),
                            i18n::key_target_text(lang, *trigger),
                            cycles.to_string(),
                            interval_ms.to_string(),
                        ],
                    ),
                    theme::TEXT_BODY,
                ))
                .wrap(),
            );
            if !changes.is_empty() {
                ui.add(egui::Label::new(theme::job_meta(
                    &changes
                        .iter()
                        .map(|change| i18n::macro_change_text(lang, *change))
                        .collect::<Vec<_>>()
                        .join(" / "),
                )));
            }
        }
    }
}

fn labeled_control(ui: &mut egui::Ui, label: impl Into<String>, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [180.0, theme::LINE_HEIGHT_CONTROL],
            egui::Label::new(theme::job_meta(&label.into())),
        );
        add(ui);
    });
}

fn checkbox_flag(ui: &mut egui::Ui, label: &str, flag: &mut u32) -> bool {
    let mut on = *flag == 1;
    let changed = ui.checkbox(&mut on, label).changed();
    if changed {
        *flag = u32::from(on);
    }
    changed
}

fn argb_picker(ui: &mut egui::Ui, argb: &mut Argb) -> bool {
    let mut rgb = [argb.r(), argb.g(), argb.b()];
    let changed = ui.color_edit_button_srgb(&mut rgb).changed();
    if changed {
        *argb = Argb::from_rgba(rgb[0], rgb[1], rgb[2], 0xFF);
    }
    changed
}

fn key_target_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    lang: Lang,
    value: &mut KeyMap,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(i18n::key_target_text(lang, *value))
        .width(180.0)
        .show_ui(ui, |ui| {
            for target in KEY_TARGETS {
                if ui
                    .selectable_value(value, target, i18n::key_target_text(lang, target))
                    .changed()
                {
                    changed = true;
                }
            }
        });
    changed
}

/// Settings editor. Pure UI over a [`SettingsDraft`]; the caller owns the
/// enabled/disabled wrapper and the write actions.
fn settings_editor(
    ui: &mut egui::Ui,
    lang: Lang,
    config: &eightbitdo_protocol::config_bt2::Bt2Config,
    settings: &mut SettingsDraft,
    slot: &mut Slot,
) {
    section_label(ui, lang, Key::SectionProfileSettings);
    ui.add_space(theme::SPACE_2);
    ui.horizontal(|ui| {
        ui.add_sized(
            [180.0, theme::LINE_HEIGHT_CONTROL],
            egui::Label::new(theme::job_meta(i18n::t(lang, Key::MacroProfile))),
        );
        for candidate in Slot::ALL {
            let label = i18n::profile_text(lang, candidate.number());
            if ui.selectable_label(*slot == candidate, label).clicked() {
                *slot = candidate;
            }
        }
    });
    let index = slot.index();

    let mut mode_selected = settings.gamepad_mode;
    labeled_control(ui, i18n::t(lang, Key::GamepadMode), |ui| {
        let selected = mode_selected.map_or_else(
            || i18n::gamepad_mode_text(lang, config.gamepad_mode_raw()),
            |mode| i18n::gamepad_mode_text(lang, mode as u16),
        );
        egui::ComboBox::from_id_salt("settings-gamepad-mode")
            .selected_text(selected)
            .width(180.0)
            .show_ui(ui, |ui| {
                for mode in GAMEPAD_MODES {
                    ui.selectable_value(
                        &mut settings.gamepad_mode,
                        Some(mode),
                        i18n::gamepad_mode_text(lang, mode as u16),
                    );
                }
            });
        mode_selected = settings.gamepad_mode;
    });

    labeled_control(ui, i18n::t(lang, Key::ActiveProfile), |ui| {
        let selected = settings.cur_slot.map_or_else(
            || i18n::t(lang, Key::ProfileNoSelection).to_string(),
            |slot| i18n::profile_text(lang, slot.number()),
        );
        egui::ComboBox::from_id_salt("settings-active-profile")
            .selected_text(selected)
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut settings.cur_slot,
                    None,
                    i18n::t(lang, Key::ProfileNoSelection),
                );
                for candidate in Slot::ALL {
                    ui.selectable_value(
                        &mut settings.cur_slot,
                        Some(candidate),
                        i18n::profile_text(lang, candidate.number()),
                    );
                }
            });
    });

    labeled_control(ui, i18n::t(lang, Key::ProfileNameLabel), |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut settings.profile_names[index])
                .desired_width(260.0)
                .hint_text(i18n::t(lang, Key::ProfileNameHint)),
        );
    });
    if let Err(error) = write::validate_name(&settings.profile_names[index], *slot) {
        ui.add(egui::Label::new(theme::job_meta_color(
            &i18n::draft_error_text(lang, &error),
            theme::RED,
        )));
    }
    ui.add(egui::Label::new(theme::job_meta(i18n::t(
        lang,
        Key::ProfileNameHint,
    ))));

    ui.add_space(theme::SPACE_4);
    section_label(ui, lang, Key::SectionRumble);
    ui.add_space(theme::SPACE_2);
    checkbox_flag(
        ui,
        i18n::t(lang, Key::RumbleEnabled),
        &mut settings.rumble[index].flag,
    );
    labeled_control(ui, i18n::t(lang, Key::RumbleLeft), |ui| {
        ui.add(egui::Slider::new(&mut settings.rumble[index].left, 0.0..=1.0).fixed_decimals(2));
    });
    labeled_control(ui, i18n::t(lang, Key::RumbleRight), |ui| {
        ui.add(egui::Slider::new(&mut settings.rumble[index].right, 0.0..=1.0).fixed_decimals(2));
    });

    ui.add_space(theme::SPACE_4);
    section_label(ui, lang, Key::SectionSticks);
    ui.add_space(theme::SPACE_2);
    checkbox_flag(
        ui,
        i18n::t(lang, Key::StickEnabled),
        &mut settings.joy[index].flag,
    );
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = 160.0;
        labeled_control(ui, i18n::t(lang, Key::StickLeftStart), |ui| {
            ui.add(egui::Slider::new(
                &mut settings.joy[index].left_start_value,
                0..=127,
            ));
        });
        labeled_control(ui, i18n::t(lang, Key::StickLeftEnd), |ui| {
            ui.add(egui::Slider::new(
                &mut settings.joy[index].left_end_value,
                0..=128,
            ));
        });
        labeled_control(ui, i18n::t(lang, Key::StickRightStart), |ui| {
            ui.add(egui::Slider::new(
                &mut settings.joy[index].right_start_value,
                0..=127,
            ));
        });
        labeled_control(ui, i18n::t(lang, Key::StickRightEnd), |ui| {
            ui.add(egui::Slider::new(
                &mut settings.joy[index].right_end_value,
                0..=128,
            ));
        });
    });

    ui.add_space(theme::SPACE_4);
    section_label(ui, lang, Key::SectionTriggers);
    ui.add_space(theme::SPACE_2);
    checkbox_flag(
        ui,
        i18n::t(lang, Key::TriggerEnabled),
        &mut settings.trigger[index].flag,
    );
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = 160.0;
        labeled_control(ui, i18n::t(lang, Key::TriggerLeftStart), |ui| {
            ui.add(egui::Slider::new(
                &mut settings.trigger[index].left_start_value,
                0..=255,
            ));
        });
        labeled_control(ui, i18n::t(lang, Key::TriggerLeftEnd), |ui| {
            ui.add(egui::Slider::new(
                &mut settings.trigger[index].left_end_value,
                0..=255,
            ));
        });
        labeled_control(ui, i18n::t(lang, Key::TriggerRightStart), |ui| {
            ui.add(egui::Slider::new(
                &mut settings.trigger[index].right_start_value,
                0..=255,
            ));
        });
        labeled_control(ui, i18n::t(lang, Key::TriggerRightEnd), |ui| {
            ui.add(egui::Slider::new(
                &mut settings.trigger[index].right_end_value,
                0..=255,
            ));
        });
    });

    ui.add_space(theme::SPACE_4);
    section_label(ui, lang, Key::SectionSpecial);
    ui.add_space(theme::SPACE_2);
    checkbox_flag(
        ui,
        i18n::t(lang, Key::SpecialEnabled),
        &mut settings.special[index].flag,
    );
    ui.horizontal_wrapped(|ui| {
        let mut bits = settings.special[index].features.bits();
        for feature in SPECIAL_FEATURES {
            let mut on = bits & feature.bits() != 0;
            if ui
                .checkbox(&mut on, i18n::special_feature_text(lang, feature))
                .changed()
            {
                if on {
                    bits |= feature.bits();
                } else {
                    bits &= !feature.bits();
                }
            }
        }
        settings.special[index].features = SpecialFeature(bits);
    });

    ui.add_space(theme::SPACE_4);
    section_label(ui, lang, Key::SectionGyro);
    ui.add_space(theme::SPACE_2);
    let gyro = &mut settings.six_axis[index];
    checkbox_flag(ui, i18n::t(lang, Key::GyroEnabled), &mut gyro.flag);
    labeled_control(ui, i18n::t(lang, Key::GyroTriggerMode), |ui| {
        ui.selectable_value(
            &mut gyro.trigger_mode,
            eightbitdo_protocol::config_bt2::gyro_trigger::HOLD,
            i18n::t(lang, Key::GyroHold),
        );
        ui.selectable_value(
            &mut gyro.trigger_mode,
            eightbitdo_protocol::config_bt2::gyro_trigger::TOGGLE,
            i18n::t(lang, Key::GyroToggle),
        );
    });
    labeled_control(ui, i18n::t(lang, Key::GyroSensitivity), |ui| {
        ui.add(egui::Slider::new(&mut gyro.sensitivity, 1..=15));
    });
    labeled_control(ui, i18n::t(lang, Key::GyroDeadCompensate), |ui| {
        ui.add(egui::Slider::new(&mut gyro.dead_compensate, 0..=255));
    });
    labeled_control(ui, i18n::t(lang, Key::GyroMapping), |ui| {
        ui.selectable_value(
            &mut gyro.mapping_type,
            eightbitdo_protocol::config_bt2::gyro_target::LEFT_STICK,
            i18n::t(lang, Key::LeftStick),
        );
        ui.selectable_value(
            &mut gyro.mapping_type,
            eightbitdo_protocol::config_bt2::gyro_target::RIGHT_STICK,
            i18n::t(lang, Key::RightStick),
        );
        ui.selectable_value(
            &mut gyro.mapping_type,
            eightbitdo_protocol::config_bt2::gyro_target::MOUSE,
            i18n::t(lang, Key::GyroMouse),
        );
    });
    labeled_control(ui, i18n::t(lang, Key::GyroButton), |ui| {
        key_target_combo(ui, ("gyro-key", index), lang, &mut gyro.key_map);
    });

    ui.add_space(theme::SPACE_4);
    section_label(ui, lang, Key::SectionLed);
    ui.add_space(theme::SPACE_2);
    ui.collapsing(i18n::t(lang, Key::LedDynamic), |ui| {
        let theme = &mut settings.dynamic[index];
        checkbox_flag(ui, i18n::t(lang, Key::LedEnabled), &mut theme.flag);
        labeled_control(ui, i18n::t(lang, Key::LedForeground), |ui| {
            argb_picker(ui, &mut theme.color);
        });
        labeled_control(ui, i18n::t(lang, Key::LedBackground), |ui| {
            argb_picker(ui, &mut theme.bg_color);
        });
    });
    ui.collapsing(i18n::t(lang, Key::LedHotWheel), |ui| {
        let theme = &mut settings.hotwheel[index];
        checkbox_flag(ui, i18n::t(lang, Key::LedEnabled), &mut theme.flag);
        labeled_control(ui, i18n::t(lang, Key::LedForeground), |ui| {
            argb_picker(ui, &mut theme.color);
        });
        labeled_control(ui, i18n::t(lang, Key::LedBackground), |ui| {
            argb_picker(ui, &mut theme.bg_color);
        });
        labeled_control(ui, i18n::t(lang, Key::LedSpeed), |ui| {
            ui.add(egui::Slider::new(&mut theme.speed, 0..=15));
        });
    });
    ui.collapsing(i18n::t(lang, Key::LedSingle), |ui| {
        let theme = &mut settings.single[index];
        checkbox_flag(ui, i18n::t(lang, Key::LedEnabled), &mut theme.flag);
        let mut color = theme.colors[0];
        labeled_control(ui, i18n::t(lang, Key::LedAllColor), |ui| {
            if argb_picker(ui, &mut color) {
                settings.set_single_color(*slot, color);
            }
        });
    });

    ui.add_space(theme::SPACE_4);
    section_label(ui, lang, Key::SectionRemap);
    ui.add_space(theme::SPACE_2);
    // The draft stores the flag as a bool so the written value stays 0/1.
    let mut remap_on = settings.remap[index].enabled;
    if ui
        .checkbox(&mut remap_on, i18n::t(lang, Key::RemapEnabled))
        .changed()
    {
        settings.remap[index].enabled = remap_on;
        settings.remap[index].flag_touched = true;
    }
    ui.add(egui::Label::new(theme::job_meta(i18n::t(
        lang,
        Key::RemapHint,
    ))));
    ui.add_space(theme::SPACE_1);
    let mut preserved_unsupported = 0usize;
    for button in ButtonIndex::ALL {
        ui.horizontal(|ui| {
            ui.add_sized(
                [150.0, theme::LINE_HEIGHT_CONTROL],
                egui::Label::new(theme::text_job(
                    i18n::button_name(lang, button.label()),
                    theme::medium(theme::FONT_BODY),
                    theme::TEXT_BODY,
                    0.0,
                    Some(theme::LINE_HEIGHT_CONTROL),
                )),
            );
            let current = settings.remap[index].keys[button.index()];
            key_target_combo(
                ui,
                ("remap", index, button.index()),
                lang,
                &mut settings.remap[index].keys[button.index()],
            );
            let target = settings.remap[index].keys[button.index()];
            if !write::is_valid_key_target(target) {
                if target == current {
                    preserved_unsupported += 1;
                } else {
                    // The combo only offers valid keys, so this is a defensive
                    // path for a hand-built draft.
                    ui.add(egui::Label::new(theme::job_meta_color(
                        &i18n::format(
                            lang,
                            Key::RemapUnsupported,
                            &[format!("{:08X}", target.bits())],
                        ),
                        theme::AMBER,
                    )));
                }
            }
        });
    }
    if preserved_unsupported > 0 {
        ui.add(egui::Label::new(theme::job_meta_color(
            &i18n::format(
                lang,
                Key::RemapPreservedCount,
                &[preserved_unsupported.to_string()],
            ),
            theme::AMBER,
        )));
    }
    let _ = *slot;
}

/// Macro editor UI. The application shell handles the returned signals.
fn macro_editor_ui(
    ui: &mut egui::Ui,
    lang: Lang,
    config: &ConfigView,
    editor: &mut MacroEditor,
    live: bool,
    can_read: bool,
    can_write: bool,
) -> MacroTabSignals {
    let mut signals = MacroTabSignals::default();

    section_label(ui, lang, Key::SectionMacroEditor);
    ui.add_space(theme::SPACE_2);
    let mut slot = editor.slot;
    let mut index = editor.index;
    ui.horizontal(|ui| {
        ui.add_sized(
            [120.0, theme::LINE_HEIGHT_CONTROL],
            egui::Label::new(theme::job_meta(i18n::t(lang, Key::MacroProfile))),
        );
        egui::ComboBox::from_id_salt("macro-slot")
            .selected_text(i18n::profile_text(lang, slot.number()))
            .width(150.0)
            .show_ui(ui, |ui| {
                for candidate in Slot::ALL {
                    ui.selectable_value(
                        &mut slot,
                        candidate,
                        i18n::profile_text(lang, candidate.number()),
                    );
                }
            });
        ui.add_space(theme::SPACE_3);
        ui.add_sized(
            [80.0, theme::LINE_HEIGHT_CONTROL],
            egui::Label::new(theme::job_meta(i18n::t(lang, Key::MacroIndexLabel))),
        );
        egui::ComboBox::from_id_salt("macro-index")
            .selected_text(format!("M{}", index.index() + 1))
            .width(110.0)
            .show_ui(ui, |ui| {
                for candidate in MacroIndex::ALL {
                    ui.selectable_value(
                        &mut index,
                        candidate,
                        format!("M{}", candidate.index() + 1),
                    );
                }
            });
    });
    if slot != editor.slot || index != editor.index {
        editor.select(slot, index);
    }
    ui.add_space(theme::SPACE_2);
    ui.horizontal(|ui| {
        let read_label = i18n::t(lang, Key::MacroLoad).to_owned();
        if secondary_key(ui, "macro-read", can_read, &read_label, theme::BG)
            .on_hover_text(i18n::t(lang, Key::MacroLoadHint))
            .clicked()
        {
            signals.read = true;
        }
    });

    let target = &config.macro_slots[editor.slot.index()].records[editor.index.index()];
    ui.add_space(theme::SPACE_2);
    data_grid(
        ui,
        &[
            (
                i18n::t(lang, Key::TableName).to_owned(),
                if target.name.is_empty() {
                    i18n::t(lang, Key::EmptyProfileName).to_owned()
                } else {
                    target.name.clone()
                },
                ValueKind::Text,
            ),
            (
                i18n::t(lang, Key::TableSteps).to_owned(),
                target.max_steps.to_string(),
                ValueKind::Data,
            ),
            (
                i18n::t(lang, Key::TableKeyMap).to_owned(),
                target.key_map.clone(),
                ValueKind::Data,
            ),
        ],
    );
    let target_empty = target.max_steps == 0 || target.max_steps == u16::MAX;
    if target_empty {
        ui.add_space(theme::SPACE_2);
        ui.add(
            egui::Label::new(theme::job_body_color(
                i18n::t(lang, Key::MacroEmptyTarget),
                theme::AMBER,
            ))
            .wrap(),
        );
        return signals;
    }

    if !editor.is_loaded() || editor.draft.is_none() || editor.original.is_none() {
        ui.add_space(theme::SPACE_3);
        ui.add(egui::Label::new(theme::job_meta(i18n::t(lang, Key::MacroNotLoaded))).wrap());
        return signals;
    }

    ui.add_space(theme::SPACE_3);
    let original = editor
        .original
        .as_ref()
        .expect("the editor was checked to have an original")
        .clone();

    ui.horizontal(|ui| {
        if editor.recording {
            let stop_label = i18n::t(lang, Key::MacroStopRecording).to_owned();
            if secondary_key(ui, "macro-record-stop", true, &stop_label, theme::BG).clicked() {
                signals.stop_record = true;
            }
        } else {
            let start_label = i18n::t(lang, Key::MacroStartRecording).to_owned();
            if secondary_key(
                ui,
                "macro-record-start",
                can_read && !target_empty,
                &start_label,
                theme::BG,
            )
            .clicked()
            {
                if live {
                    signals.record = true;
                } else {
                    signals.start_live = true;
                }
            }
            if !live {
                ui.add(egui::Label::new(theme::job_meta(i18n::t(
                    lang,
                    Key::MacroRecordingNeedsLive,
                ))));
            }
        }
    });
    ui.add(egui::Label::new(theme::job_meta(i18n::t(
        lang,
        Key::MacroRecordingHint,
    ))));
    ui.add_space(theme::SPACE_3);

    {
        let draft = editor
            .draft
            .as_mut()
            .expect("the editor was checked to have a draft");
        section_label(ui, lang, Key::SectionMacroHeader);
        ui.add_space(theme::SPACE_2);
        labeled_control(ui, i18n::t(lang, Key::Name), |ui| {
            ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(220.0));
        });
        labeled_control(ui, i18n::t(lang, Key::MacroTrigger), |ui| {
            key_target_combo(ui, "macro-trigger", lang, &mut draft.key_map);
        });
        labeled_control(ui, i18n::t(lang, Key::MacroCycles), |ui| {
            ui.add(egui::DragValue::new(&mut draft.cycles_num).range(0..=MAX_CYCLES));
        });
        labeled_control(ui, i18n::t(lang, Key::MacroInterval), |ui| {
            ui.add(egui::DragValue::new(&mut draft.interval_ms).range(0..=MAX_INTERVAL_MS));
        });
    }

    ui.add_space(theme::SPACE_4);
    section_label(ui, lang, Key::SectionMacroSteps);
    ui.add_space(theme::SPACE_2);
    {
        let draft = editor
            .draft
            .as_ref()
            .expect("the editor was checked to have a draft");
        ui.add(egui::Label::new(theme::job_meta(&i18n::format(
            lang,
            Key::MacroTotalDuration,
            &[draft.total_ms().to_string(), draft.steps.len().to_string()],
        ))));
        ui.add(egui::Label::new(theme::job_meta(i18n::t(
            lang,
            Key::MacroSelectStepHint,
        ))));
    }
    ui.add_space(theme::SPACE_1);
    let mut selected = editor.selected_step;
    {
        let draft = editor
            .draft
            .as_ref()
            .expect("the editor was checked to have a draft");
        egui::ScrollArea::vertical()
            .id_salt("macro-steps")
            .max_height(280.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("macro-steps-horizontal")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (index, step) in draft.steps.iter().enumerate() {
                            let is_selected = selected == Some(index);
                            let keys =
                                i18n::keymap_text(lang, KeyMap::from_bits(u32::from(step.keys)));
                            let sticks = format!(
                                "x={} y={} / x={} y={}",
                                step.left_x, step.left_y, step.right_x, step.right_y
                            );
                            let triggers = format!(
                                "{} / {}",
                                i18n::format(lang, Key::TriggerL, &[step.lt_value.to_string()]),
                                i18n::format(lang, Key::TriggerR, &[step.rt_value.to_string()])
                            );
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(is_selected, format!("#{}", index + 1))
                                    .clicked()
                                {
                                    selected = Some(index);
                                }
                                ui.add_sized(
                                    [72.0, theme::LINE_HEIGHT_META],
                                    egui::Label::new(theme::job_data(
                                        &format!("{} ms", step.ms_times),
                                        theme::TEXT_BODY,
                                    )),
                                );
                                ui.add_sized(
                                    [170.0, theme::LINE_HEIGHT_META],
                                    egui::Label::new(theme::job_data(&keys, theme::TEXT_BODY))
                                        .truncate(),
                                );
                                ui.add_sized(
                                    [190.0, theme::LINE_HEIGHT_META],
                                    egui::Label::new(theme::job_data(&sticks, theme::TEXT_MUTED))
                                        .truncate(),
                                );
                                ui.add_sized(
                                    [160.0, theme::LINE_HEIGHT_META],
                                    egui::Label::new(theme::job_data(&triggers, theme::TEXT_MUTED))
                                        .truncate(),
                                );
                            });
                        }
                    });
            });
    }
    editor.selected_step = selected;

    if let Some(index) = editor.selected_step {
        if let Some(draft) = editor.draft.as_mut() {
            if let Some(step) = draft.steps.get_mut(index) {
                ui.add_space(theme::SPACE_2);
                labeled_control(ui, i18n::t(lang, Key::MacroStepMs), |ui| {
                    ui.add(egui::DragValue::new(&mut step.ms_times).range(1..=u16::MAX));
                });
                labeled_control(ui, i18n::t(lang, Key::MacroStepKeys), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (label, flag) in MACRO_STEP_KEYS {
                            let mut on = step.keys & flag.bits() as u16 != 0;
                            if ui.checkbox(&mut on, label).changed() {
                                if on {
                                    step.keys |= flag.bits() as u16;
                                } else {
                                    step.keys &= !(flag.bits() as u16);
                                }
                            }
                        }
                    });
                });
                ui.add(egui::Label::new(theme::job_meta(i18n::t(
                    lang,
                    Key::MacroKeysHint,
                ))));
                ui.scope(|ui| {
                    ui.spacing_mut().slider_width = 80.0;
                    labeled_control(ui, i18n::t(lang, Key::MacroStepSticks), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(egui::Slider::new(&mut step.left_x, 0..=255).text("LX"));
                            ui.add(egui::Slider::new(&mut step.left_y, 0..=255).text("LY"));
                            ui.add(egui::Slider::new(&mut step.right_x, 0..=255).text("RX"));
                            ui.add(egui::Slider::new(&mut step.right_y, 0..=255).text("RY"));
                        });
                    });
                    labeled_control(ui, i18n::t(lang, Key::MacroStepTriggers), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(egui::Slider::new(&mut step.lt_value, 0..=255).text("L2"));
                            ui.add(egui::Slider::new(&mut step.rt_value, 0..=255).text("R2"));
                        });
                    });
                });
            }
        }
    }

    ui.add_space(theme::SPACE_2);
    let can_delete = editor
        .draft
        .as_ref()
        .is_some_and(|draft| draft.steps.len() > 1);
    ui.horizontal(|ui| {
        let delete_label = i18n::t(lang, Key::MacroDeleteStep).to_owned();
        if secondary_key(
            ui,
            "macro-delete",
            editor.selected_step.is_some() && can_delete,
            &delete_label,
            theme::BG,
        )
        .clicked()
        {
            editor.delete_selected();
        }
        let insert_label = i18n::t(lang, Key::MacroInsertWait).to_owned();
        if secondary_key(ui, "macro-insert-wait", true, &insert_label, theme::BG).clicked() {
            editor.insert_wait();
        }
        ui.add(egui::Label::new(theme::job_meta(i18n::t(
            lang,
            Key::MacroWaitMs,
        ))));
        ui.add(egui::DragValue::new(&mut editor.wait_ms).range(1..=u16::MAX));
    });

    let (validation, changes) = {
        let draft = editor
            .draft
            .as_ref()
            .expect("the editor was checked to have a draft");
        (draft.validate(&original), draft.changes(&original))
    };
    if !changes.is_empty() {
        ui.add_space(theme::SPACE_2);
        ui.add(egui::Label::new(theme::job_meta(
            &changes
                .iter()
                .map(|change| i18n::macro_change_text(lang, *change))
                .collect::<Vec<_>>()
                .join(" / "),
        )));
    }
    if let Err(error) = &validation {
        ui.add(egui::Label::new(theme::job_body_color(
            &i18n::draft_error_text(lang, error),
            theme::RED,
        )));
    }
    ui.add_space(theme::SPACE_2);
    let review_label = i18n::t(lang, Key::MacroWriteAction).to_owned();
    if secondary_key(
        ui,
        "macro-review",
        can_write && !editor.recording && validation.is_ok() && !changes.is_empty(),
        &review_label,
        theme::BG,
    )
    .clicked()
    {
        signals.review = true;
    }

    signals
}

#[cfg(test)]
mod tests {
    use eightbitdo_core::ConfigSnapshot;
    use eightbitdo_protocol::config_bt2::Bt2Config;

    use super::*;

    fn config_snapshot() -> ConfigSnapshot {
        let config = Bt2Config::default();
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

    fn config_view() -> ConfigView {
        ConfigView::from_snapshot(&config_snapshot())
    }

    fn config_event() -> Event {
        config_event_with(config_snapshot())
    }

    fn config_event_with(snapshot: ConfigSnapshot) -> Event {
        Event::Config {
            snapshot: Box::new(snapshot.clone()),
            view: Box::new(ConfigView::from_snapshot(&snapshot)),
        }
    }

    fn config_snapshot_with_macro() -> ConfigSnapshot {
        use eightbitdo_protocol::config_bt2::MacroSlot;

        let mut config = Bt2Config::default();
        let mut header = MacroSlot::default();
        header.records[0].max_steps = 2;
        config.set_macro_slot(Slot::S1, &header);
        config.recompute_crc();
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

    fn loaded_macro() -> eightbitdo_core::macro_ops::MacroEntry {
        use eightbitdo_protocol::config_bt2::MacroRecord;
        use eightbitdo_protocol::macro_data::{encode_macro_steps, MacroStep};

        let steps = vec![MacroStep::neutral(20), MacroStep::neutral(20)];
        let raw = encode_macro_steps(&steps).unwrap()[..steps.len() * 10].to_vec();
        let record = MacroRecord {
            max_steps: steps.len() as u16,
            ..MacroRecord::default()
        };
        eightbitdo_core::macro_ops::MacroEntry::new(Slot::S1, MacroIndex::M1, record, raw).unwrap()
    }

    fn backup_report() -> BackupReport {
        BackupReport {
            raw_path: PathBuf::from("target/test-backups/prewrite-config.bin"),
            json_path: PathBuf::from("target/test-backups/prewrite-config.json"),
            sha256: "0".repeat(64),
            bytes: 2768,
            warnings: Vec::new(),
        }
    }

    fn device() -> DeviceInfoView {
        DeviceInfoView {
            name: Some("80ND".to_string()),
            address: "AA:BB:CC:DD:EE:FF".to_string(),
            mtu: 64,
            characteristic: "ff13".to_string(),
        }
    }

    fn app() -> App {
        App::build(Worker::spawn(), PathBuf::from("target/test-backups"))
    }

    #[test]
    fn connection_and_read_events_drive_the_busy_state() {
        let mut app = app();
        assert_eq!(app.connection, ConnectionState::Disconnected);
        assert_eq!(app.lang, Lang::Ja);

        app.handle_event(Event::ScanStarted);
        assert_eq!(app.connection, ConnectionState::Scanning);
        assert!(app.busy.is_some());
        assert!(app.scan_angle().is_some());

        app.handle_event(Event::ScanFinished(vec![
            ScanEntry {
                name: Some("other".to_string()),
                address: "11".to_string(),
                rssi: None,
                is_app: false,
            },
            ScanEntry {
                name: Some("80ND".to_string()),
                address: "22".to_string(),
                rssi: Some(-40),
                is_app: true,
            },
        ]));
        assert_eq!(app.connection, ConnectionState::Disconnected);
        assert_eq!(app.selected_address.as_deref(), Some("22"));
        assert!(app.busy.is_none());
        assert!(app.scan_angle().is_none());

        app.handle_event(Event::Connecting);
        assert_eq!(app.connection, ConnectionState::Connecting);

        app.handle_event(Event::Connected(device()));
        assert_eq!(app.connection, ConnectionState::Connected);
        assert!(app.busy.is_some());

        app.handle_event(Event::Version(VersionView {
            raw_hex: "c8000000".to_string(),
            text: None,
            value: Some(200),
        }));
        assert!(app.busy.is_some(), "auto configuration read still runs");

        app.handle_event(config_event());
        app.handle_event(Event::Live(true));
        app.handle_event(Event::Input(InputView::default()));
        assert!(app.input.is_some());
        assert!(app.live);

        app.handle_event(Event::Idle);
        assert!(app.busy.is_none());

        app.handle_event(Event::Disconnected { reason: None });
        assert_eq!(app.connection, ConnectionState::Disconnected);
        assert!(!app.live);
        assert!(app.input.is_none());
    }

    #[test]
    fn errors_are_visible_and_a_failed_scan_clears_busy() {
        let mut app = app();
        app.handle_event(Event::ScanStarted);
        app.handle_event(Event::Error(
            Notice::new(Key::ErrScanFailed).with_detail("scan failed"),
        ));

        assert!(app.error.is_some());
        assert_eq!(
            app.error.as_ref().map(|notice| notice.key),
            Some(Key::ErrScanFailed)
        );
        assert!(app.busy.is_none());
        assert!(app.log.iter().any(|entry| entry.level == LogLevel::Error));

        app.handle_event(Event::ScanFinished(Vec::new()));
        assert_eq!(app.connection, ConnectionState::Disconnected);
        assert!(app.scan_entries.is_empty());
    }

    #[test]
    fn stale_warnings_require_cached_data() {
        let mut app = app();
        assert!(!app.stale_device());
        assert!(!app.stale_config());
        assert!(app.wants_onboarding());

        app.device = Some(device());
        app.version = Some(VersionView {
            raw_hex: "c8000000".to_string(),
            text: None,
            value: Some(200),
        });
        app.config = Some(config_view());

        assert_eq!(app.connection, ConnectionState::Disconnected);
        assert!(app.stale_device());
        assert!(app.stale_config());
        assert!(!app.wants_onboarding());

        app.connection = ConnectionState::Connected;
        assert!(!app.stale_device());
        assert!(!app.stale_config());
    }

    #[test]
    fn onboarding_is_limited_to_the_disconnected_state_without_data() {
        let mut app = app();
        assert!(app.wants_onboarding());

        app.handle_event(Event::ScanStarted);
        assert!(app.wants_onboarding(), "scanning is still disconnected");

        app.handle_event(Event::Connected(device()));
        assert!(!app.wants_onboarding());
        assert!(!app.stale_device());
    }

    #[test]
    fn log_entries_keep_translation_keys_so_language_switches_apply() {
        let mut app = app();
        app.handle_event(Event::ScanStarted);
        assert_eq!(
            app.log.back().map(|entry| entry.notice.key),
            Some(Key::LogScanStarted)
        );
        assert_eq!(
            app.notice_text(&app.log.back().expect("entry").notice),
            "スキャンを開始しました。"
        );
        app.lang = Lang::En;
        assert_eq!(
            app.notice_text(&app.log.back().expect("entry").notice),
            "Scan started."
        );
    }

    #[test]
    fn connect_target_uses_only_app_mode_scan_entries() {
        let mut app = app();
        app.scan_entries = vec![
            ScanEntry {
                name: Some("keyboard".to_string()),
                address: "11".to_string(),
                rssi: None,
                is_app: false,
            },
            ScanEntry {
                name: Some("80ND".to_string()),
                address: "22".to_string(),
                rssi: Some(-40),
                is_app: true,
            },
        ];
        app.selected_address = Some("22".to_string());
        assert_eq!(app.selected_app_address().as_deref(), Some("22"));

        app.selected_address = Some("11".to_string());
        assert_eq!(
            app.selected_app_address(),
            None,
            "non-App selection falls back to 80ND by name"
        );
    }

    #[test]
    fn rssi_is_quantized_into_four_bars() {
        assert_eq!(rssi_bars(None), 0);
        assert_eq!(rssi_bars(Some(-40)), 4);
        assert_eq!(rssi_bars(Some(-55)), 3);
        assert_eq!(rssi_bars(Some(-70)), 2);
        assert_eq!(rssi_bars(Some(-90)), 1);
    }

    #[test]
    fn stick_trails_keep_only_the_latest_eight_points() {
        let mut app = app();
        for index in 0..12 {
            app.handle_event(Event::Input(InputView {
                left_stick: (index, -index),
                right_stick: (index, index),
                ..InputView::default()
            }));
        }
        assert_eq!(app.left_trail.len(), 8);
        assert_eq!(app.right_trail.len(), 8);
        let newest = app.left_trail.back().copied().expect("trail");
        assert!((newest.0 - 11.0 / 128.0).abs() < f32::EPSILON);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn layout_constants_keep_the_content_column_narrow() {
        assert_eq!(theme::SIDE_PANEL_WIDTH, 264.0);
        assert_eq!(theme::CONTENT_MAX_WIDTH, 700.0);
        assert!(900.0 - theme::SIDE_PANEL_WIDTH - 2.0 * theme::CONTENT_INSET >= 500.0);
        assert!(theme::KEY_HEIGHT >= 40.0);
        assert!(theme::TAB_HEIGHT >= 40.0);
        assert_eq!(LOG_ROW_HEIGHT, 28.0);
    }

    #[test]
    fn write_events_populate_results_failures_and_the_macro_editor() {
        let mut app = app();
        app.handle_event(config_event());
        assert!(app.settings.is_some());
        assert!(app.config_snapshot.is_some());

        app.handle_event(Event::WriteBackup(BackupRef::from(&backup_report())));
        assert!(app.last_write_backup.is_some());

        app.handle_event(Event::ConfigWritten(Box::new(ConfigWriteReport {
            backup: backup_report(),
            snapshot: config_snapshot(),
            view: config_view(),
            changed: Vec::new(),
            packets_sent: 1,
            crc: eightbitdo_core::config_ops::ConfigCrcStatus {
                expected: 0,
                stored: 0,
                recomputed: 0,
                recomputed_from_sent: 0,
            },
        })));
        assert!(matches!(app.write_result, Some(WriteOutcome::Config(_))));
        assert!(app.write_failure.is_none());

        app.handle_event(Event::WriteFailed(Box::new(WriteFailure {
            phase: WritePhase::Device,
            detail: "timeout".to_string(),
            backup: Some(BackupRef::from(&backup_report())),
            recovery: Some("recovery".to_string()),
            disconnected: false,
        })));
        assert!(app.write_result.is_none());
        assert!(app.write_failure.is_some());

        app.handle_event(Event::MacroLoaded {
            slot: Slot::S1,
            index: MacroIndex::M1,
            entry: Box::new(loaded_macro()),
        });
        assert!(app.macro_editor.is_loaded());

        app.macro_editor.begin_recording();
        assert!(app.macro_editor.recording);
        app.handle_event(Event::Input(InputView::default()));
        assert!(!app.macro_editor.recorder.is_empty());
        app.handle_event(Event::Live(false));
        assert!(!app.macro_editor.recording);
    }

    #[test]
    fn review_opens_a_pending_write_only_for_a_change_and_confirmation_needs_the_link() {
        let mut app = app();
        app.handle_event(config_event());
        assert!(!app.can_write(), "a disconnected link cannot write");

        app.settings.as_mut().unwrap().profile_names[0] = "Edited".to_string();
        app.review_settings();
        assert!(
            matches!(
                app.pending,
                Some(PendingWrite {
                    target: WriteTarget::Config { .. },
                    ..
                })
            ),
            "review opens the confirmation dialog for a real change"
        );
        let pending = app.pending.take().unwrap();
        app.confirm_write(pending);
        assert!(app.pending.is_none());
        assert!(
            app.busy.is_none(),
            "confirming while disconnected must not start a write"
        );

        app.connection = ConnectionState::Connected;
        app.settings = Some(SettingsDraft::from_config(&config_snapshot().config));
        app.review_settings();
        assert_eq!(app.draft_error, Some(DraftError::NoChanges));
        assert!(app.pending.is_none());
    }

    #[test]
    fn macro_review_requires_a_loaded_existing_macro() {
        let mut app = app();
        app.handle_event(config_event());
        app.macro_editor.load(loaded_macro());
        app.review_macro();
        assert_eq!(app.draft_error, Some(DraftError::NoChanges));

        app.macro_editor.draft.as_mut().unwrap().steps[0].ms_times = 40;
        app.connection = ConnectionState::Connected;
        app.review_macro();
        assert!(
            matches!(
                app.pending,
                Some(PendingWrite {
                    target: WriteTarget::Macro { .. },
                    ..
                })
            ),
            "a changed macro opens the macro confirmation"
        );
    }

    #[test]
    fn a_reviewed_macro_command_keeps_the_reviewed_target() {
        let mut app = app();
        app.handle_event(config_event());
        app.macro_editor.load(loaded_macro());
        app.macro_editor.draft.as_mut().unwrap().steps[0].ms_times = 40;
        app.connection = ConnectionState::Connected;
        app.review_macro();
        let pending = app.pending.take().unwrap();

        // The user can change the selection behind the dialog; the reviewed
        // target must still be the one that is written.
        app.macro_editor.select(Slot::S3, MacroIndex::M4);
        let (command, busy) =
            pending_command(pending, PathBuf::from("target/test-backups")).unwrap();
        assert_eq!(busy, Busy::WriteMacro);
        match command {
            Command::WriteMacro { slot, index, .. } => {
                assert_eq!(slot, Slot::S1);
                assert_eq!(index, MacroIndex::M1);
            }
            other => panic!("expected a macro write command, got {other:?}"),
        }
    }

    #[test]
    fn recording_blocks_write_confirmation_and_loading_reports_the_stop() {
        let mut recording_app = app();
        recording_app.handle_event(config_event());
        recording_app.connection = ConnectionState::Connected;
        recording_app.macro_editor.load(loaded_macro());
        recording_app.macro_editor.begin_recording();
        assert!(recording_app.macro_editor.recording);
        assert!(
            !recording_app.can_write(),
            "recording must block the write path"
        );

        recording_app.settings.as_mut().unwrap().profile_names[0] = "Edited".to_string();
        recording_app.review_settings();
        assert_eq!(recording_app.draft_error, Some(DraftError::Recording));
        assert!(recording_app.pending.is_none());

        // A dialog opened before recording must stay open (not silently send).
        let mut app = app();
        app.handle_event(config_event());
        app.connection = ConnectionState::Connected;
        app.settings.as_mut().unwrap().profile_names[0] = "Edited".to_string();
        app.review_settings();
        let pending = app.pending.take().expect("review opens the dialog");
        app.macro_editor.load(loaded_macro());
        app.macro_editor.begin_recording();
        app.confirm_write(pending);
        assert!(
            app.pending.is_some(),
            "the dialog stays open while recording"
        );
        assert!(app.busy.is_none(), "no write may be sent while recording");

        app.handle_event(Event::MacroLoaded {
            slot: Slot::S1,
            index: MacroIndex::M1,
            entry: Box::new(loaded_macro()),
        });
        assert!(!app.macro_editor.recording);
        assert!(app
            .log
            .iter()
            .any(|entry| entry.notice.key == Key::LogRecordingStopped));
    }

    #[test]
    fn a_config_refresh_drops_a_stale_macro_baseline_and_logs_it() {
        let mut app = app();
        app.handle_event(config_event());
        app.macro_editor.load(loaded_macro());
        assert!(app.macro_editor.is_loaded());

        // The refreshed configuration has no steps for M1, so the loaded
        // baseline no longer matches the displayed header.
        app.handle_event(config_event());
        assert!(
            !app.macro_editor.is_loaded(),
            "a stale macro baseline must not stay loaded"
        );
        assert!(app
            .log
            .iter()
            .any(|entry| entry.notice.key == Key::LogMacroReload));

        // A matching refresh keeps the loaded macro intact.
        let matching = config_snapshot_with_macro();
        app.handle_event(config_event_with(matching.clone()));
        app.macro_editor.load(loaded_macro());
        app.handle_event(config_event_with(matching));
        assert!(app.macro_editor.is_loaded());
    }

    #[test]
    fn headless_draw_covers_onboarding_stale_data_and_every_tab() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))),
            ..Default::default()
        };

        let mut app = app();
        app.scan_entries = vec![
            ScanEntry {
                name: Some("80ND".to_string()),
                address: "AA:BB:CC:DD:EE:FF".to_string(),
                rssi: Some(-48),
                is_app: true,
            },
            ScanEntry {
                name: None,
                address: "11:22:33:44:55:66".to_string(),
                rssi: Some(-82),
                is_app: false,
            },
        ];
        app.selected_address = Some("AA:BB:CC:DD:EE:FF".to_string());
        let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));

        // Scanning state exercises the rotating amber arc.
        app.handle_event(Event::ScanStarted);
        let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));
        app.handle_event(Event::ScanFinished(app.scan_entries.clone()));

        // Stale data + error banner + every tab.
        app.device = Some(device());
        app.version = Some(VersionView {
            raw_hex: "c8000000".to_string(),
            text: None,
            value: Some(200),
        });
        let snapshot = config_snapshot_with_macro();
        app.settings = Some(SettingsDraft::from_config(&snapshot.config));
        app.config = Some(ConfigView::from_snapshot(&snapshot));
        app.config_snapshot = Some(snapshot);
        app.macro_editor.load(loaded_macro());
        app.error = Some(Notice::new(Key::ErrScanFailed).with_detail("scan failed"));
        let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));

        app.connection = ConnectionState::Connected;
        app.input = Some(InputView::default());
        app.live = true;
        for tab in Tab::ALL {
            app.tab = tab;
            let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));
        }

        // A confirmation dialog and the result / failure banners paint on top.
        app.settings.as_mut().unwrap().profile_names[0] = "Combo".to_string();
        app.review_settings();
        let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));
        app.pending = None;
        app.write_result = Some(WriteOutcome::Config(Box::new(ConfigWriteReport {
            backup: backup_report(),
            snapshot: config_snapshot(),
            view: config_view(),
            changed: Vec::new(),
            packets_sent: 1,
            crc: eightbitdo_core::config_ops::ConfigCrcStatus {
                expected: 0,
                stored: 0,
                recomputed: 0,
                recomputed_from_sent: 0,
            },
        })));
        let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));
        app.write_result = None;
        app.write_failure = Some(Box::new(WriteFailure {
            phase: WritePhase::Device,
            detail: "config write failed during Save (NotSaved): timeout".to_string(),
            backup: Some(BackupRef::from(&backup_report())),
            recovery: Some("recovery text".to_string()),
            disconnected: false,
        }));
        let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));

        // Log rows use a fixed height and the clear key lives in the tab row.
        app.handle_event(Event::Error(
            Notice::new(Key::ErrScanFailed).with_detail("scan failed"),
        ));
        app.tab = Tab::Log;
        let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));
    }

    fn text_shapes(output: &egui::FullOutput) -> Vec<(String, Rect, Color32)> {
        fn visit(shape: &egui::Shape, out: &mut Vec<(String, Rect, Color32)>) {
            match shape {
                egui::Shape::Text(text) => {
                    let color = text
                        .galley
                        .job
                        .sections
                        .first()
                        .map_or(text.fallback_color, |section| section.format.color);
                    out.push((
                        text.galley.job.text.clone(),
                        Rect::from_min_size(text.pos, text.galley.size()),
                        color,
                    ));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        visit(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            visit(&clipped.shape, &mut out);
        }
        out
    }

    #[test]
    fn device_empty_state_keeps_the_scan_key_inside_the_card() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))),
            ..Default::default()
        };

        let mut app = app();
        let output = ctx.run(input, |ctx| app.draw(ctx));
        let texts = text_shapes(&output);
        let find = |needle: &str| {
            texts
                .iter()
                .find(|(text, _, _)| text == needle)
                .map(|(_, rect, _)| *rect)
        };

        // The empty card starts below the tab row and is 224 pt tall.
        let card_top = theme::TOP_BAR_HEIGHT + theme::TAB_HEIGHT + theme::SPACE_4;
        let card_bottom = card_top + 224.0;

        let scan = find("スキャン").expect("scan key is painted");
        let scan_bottom = scan.center().y + theme::KEY_HEIGHT / 2.0;
        assert!(
            scan_bottom <= card_bottom + 0.5,
            "scan key bottom {scan_bottom} escaped the card ({card_top}..={card_bottom})"
        );

        let last_step = find("80ND を選んで接続する").expect("third step is painted");
        assert!(
            last_step.bottom() <= card_bottom + 0.5,
            "third step bottom {} escaped the card",
            last_step.bottom()
        );

        // Step numbers are information, not light: they must use TEXT_MUTED.
        for number in ["1", "2", "3"] {
            let (_, _, color) = texts
                .iter()
                .find(|(text, _, _)| text == number)
                .unwrap_or_else(|| panic!("missing step number {number}"));
            assert_eq!(
                *color,
                theme::TEXT_MUTED,
                "step number {number} must not use the unlit LED color"
            );
        }
        // No text may be painted in LED_DIM (that token is for ring outlines).
        for (text, _, color) in &texts {
            assert_ne!(
                *color,
                theme::LED_DIM,
                "text {text:?} uses the unlit LED color"
            );
        }

        // The top bar is the single source of the connection state.
        let status_labels = texts.iter().filter(|(text, _, _)| text == "未接続").count();
        assert_eq!(
            status_labels, 1,
            "connection state must appear exactly once (top bar)"
        );
    }

    #[test]
    fn headless_layout_keeps_tabs_horizontal_and_the_log_empty_state_visible() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))),
            ..Default::default()
        };

        let mut app = app();
        let output = ctx.run(input.clone(), |ctx| app.draw(ctx));
        let texts = text_shapes(&output);

        let find_tab = |needle: &str| {
            texts
                .iter()
                .find(|(text, rect, _)| {
                    text == needle
                        && rect.left() > theme::SIDE_PANEL_WIDTH
                        && rect.center().y > theme::TOP_BAR_HEIGHT
                        && rect.center().y < theme::TOP_BAR_HEIGHT + theme::TAB_HEIGHT
                })
                .map(|(_, rect, _)| *rect)
        };
        let tabs: Vec<Rect> = ["デバイス", "ライブ入力", "設定値", "設定", "マクロ", "ログ"]
            .iter()
            .map(|label| find_tab(label).unwrap_or_else(|| panic!("missing tab {label}")))
            .collect();
        for window in tabs.windows(2) {
            assert!(
                (window[0].center().y - window[1].center().y).abs() < 1.0,
                "tabs must share one horizontal row"
            );
            assert!(
                window[0].left() < window[1].left(),
                "tabs must be laid out left to right"
            );
        }

        // The log tab with no entries must paint its empty state and keep the
        // clear key in the tab row, at the right edge.
        app.tab = Tab::Log;
        let output = ctx.run(input, |ctx| app.draw(ctx));
        let texts = text_shapes(&output);
        let find = |needle: &str| {
            texts
                .iter()
                .find(|(text, _, _)| text == needle)
                .map(|(_, rect, _)| *rect)
        };
        let empty = find("まだ記録がありません。").expect("log empty copy is painted");
        assert!(
            empty.top() > tabs[0].bottom(),
            "empty copy belongs below the tab row"
        );
        assert!(
            !texts.iter().any(|(text, _, _)| text == "次の一手"),
            "the log tab must not use the device step heading"
        );
        assert!(
            texts
                .iter()
                .any(|(text, _, _)| text == "書き込み対応 / 設定 2768 バイト / マクロ 4×3"),
            "the bottom meta line closes the content column"
        );
        let clear = find("ログを消す").expect("clear key is painted");
        let log_tab = find("ログ").expect("log tab label");
        assert!(
            clear.left() > log_tab.right() + 200.0,
            "clear key must sit at the right end of the tab row"
        );
        assert!(
            (clear.center().y - log_tab.center().y).abs() < 1.0,
            "clear key shares the tab row"
        );

        // Body copy is wrapped inside the 700 px content column.
        let widest = texts
            .iter()
            .map(|(_, rect, _)| rect.width())
            .fold(0.0_f32, f32::max);
        assert!(
            widest <= theme::CONTENT_MAX_WIDTH + 1.0,
            "body text must stay inside the content column, widest was {widest}"
        );
    }

    fn painted_text_rects(texts: &[(String, Rect, Color32)], exact: &str) -> Vec<Rect> {
        texts
            .iter()
            .filter(|(text, _, _)| text == exact)
            .map(|(_, rect, _)| *rect)
            .collect()
    }

    /// Locate the window title-bar `×` close key. egui paints it as two
    /// crossing line segments rather than text, so find the pair of
    /// equal-sized diagonal segments that share a centre.
    fn window_close_button_center(output: &egui::FullOutput) -> Pos2 {
        fn visit(shape: &egui::Shape, segments: &mut Vec<(Pos2, Pos2)>) {
            match shape {
                egui::Shape::LineSegment { points, .. } => segments.push((points[0], points[1])),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        visit(shape, segments);
                    }
                }
                _ => {}
            }
        }
        let mut segments = Vec::new();
        for clipped in &output.shapes {
            visit(&clipped.shape, &mut segments);
        }
        let centres: Vec<Pos2> = segments
            .iter()
            .map(|(a, b)| Rect::from_two_pos(*a, *b))
            .filter(|rect| {
                (rect.width() - rect.height()).abs() < 2.0 && (4.0..=20.0).contains(&rect.width())
            })
            .map(|rect| rect.center())
            .collect();
        let mut candidates = Vec::new();
        for (index, centre) in centres.iter().enumerate() {
            for other in &centres[index + 1..] {
                if (*centre - *other).length() < 1.0 {
                    candidates.push(*centre);
                }
            }
        }
        candidates
            .into_iter()
            .min_by(|a, b| a.y.total_cmp(&b.y))
            .expect("the licenses window × key is not painted")
    }

    #[test]
    fn headless_licenses_screen_opens_shows_attribution_and_closes() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let base = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))),
            ..Default::default()
        };

        let mut app = app();
        app.licenses_open = true;
        // An egui window auto-sizes over several frames (text wraps once the
        // width is known), so let the layout settle before reading rects.
        for _ in 0..8 {
            let _ = ctx.run(base.clone(), |ctx| app.draw(ctx));
        }
        let output = ctx.run(base.clone(), |ctx| app.draw(ctx));
        let texts = text_shapes(&output);

        // The app license (MIT + Apache-2.0) and every attribution line are on
        // screen, and the subset marker required by DISTRIBUTION_POLICY §4.4 is
        // part of the heading.
        assert!(texts
            .iter()
            .any(|(text, _, _)| text == "ライセンス / オープンソース"));
        assert!(texts
            .iter()
            .any(|(text, _, _)| text == "Noto Sans JP (subset) — 同梱フォント"));
        assert!(texts
            .iter()
            .any(|(text, _, _)| text.contains("eightbitdo-suite — MIT License")));
        // Headings and holder lines are painted exactly once, and each holder
        // sits under the heading it belongs to. A first-match search would
        // miss a duplicated Noto holder line or a holder placed in the wrong
        // group, so count every painted occurrence instead (PR #24 review
        // follow-up).
        let noto_heading = painted_text_rects(&texts, "Noto Sans JP (subset) — 同梱フォント");
        let egui_heading = painted_text_rects(&texts, "egui 既定フォント");
        assert_eq!(noto_heading.len(), 1, "one Noto Sans JP heading");
        assert_eq!(egui_heading.len(), 1, "one egui default-font heading");
        let noto_heading = noto_heading[0];
        let egui_heading = egui_heading[0];
        for (font, holder) in crate::licenses::ATTRIBUTIONS {
            let line = format!("{font}: {holder}");
            let rects = painted_text_rects(&texts, &line);
            assert_eq!(
                rects.len(),
                1,
                "the attribution line for {font} must be painted exactly once: {line:?}"
            );
            let rect = rects[0];
            if *font == "Noto Sans JP (subset)" {
                assert!(
                    rect.top() > noto_heading.bottom() && rect.bottom() < egui_heading.top(),
                    "the Noto Sans JP holder must sit between the Noto and egui headings"
                );
            } else {
                assert!(
                    rect.top() > egui_heading.bottom(),
                    "{font} holder must sit below the egui heading"
                );
            }
        }
        // The full texts live in a scroll area, so a body's heading is painted
        // once it is scrolled into view. Wheel over the first body heading
        // until the Apache-2.0 terms and the Ubuntu Font Licence title appear.
        let body_at = texts
            .iter()
            .find(|(text, _, _)| text.contains("eightbitdo-suite — MIT License"))
            .map(|(_, rect, _)| rect.center())
            .expect("the first license heading is painted");
        let mut seen_apache_terms = false;
        let mut seen_ubuntu_licence = false;
        for _ in 0..400 {
            let mut scroll_input = base.clone();
            scroll_input.events = vec![
                egui::Event::PointerMoved(body_at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    // A negative Y delta scrolls the content up (reveals below).
                    delta: Vec2::new(0.0, -160.0),
                    modifiers: egui::Modifiers::default(),
                },
            ];
            let scrolled = ctx.run(scroll_input, |ctx| app.draw(ctx));
            let scrolled_texts = text_shapes(&scrolled);
            seen_apache_terms |= scrolled_texts
                .iter()
                .any(|(text, _, _)| text.contains("TERMS AND CONDITIONS"));
            seen_ubuntu_licence |= scrolled_texts
                .iter()
                .any(|(text, _, _)| text.contains("UBUNTU FONT LICENCE"));
            if seen_apache_terms && seen_ubuntu_licence {
                break;
            }
        }
        assert!(
            seen_apache_terms,
            "the Apache-2.0 body heading must paint when scrolled into view"
        );
        assert!(
            seen_ubuntu_licence,
            "the Ubuntu Font Licence heading must paint when scrolled into view"
        );
        let close = texts
            .iter()
            .find(|(text, _, _)| text == "閉じる")
            .map(|(_, rect, _)| *rect)
            .expect("the close key is painted");

        // A press and a release on 閉じる dismisses the screen. egui resolves a
        // same-frame press/release as a click (it is not treated as a drag).
        let at = close.center();
        let pointer_button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let mut click = base.clone();
        click.events = vec![
            egui::Event::PointerMoved(at),
            pointer_button(true),
            pointer_button(false),
        ];
        let _ = ctx.run(click, |ctx| app.draw(ctx));
        assert!(
            !app.licenses_open,
            "the close key must close the licenses screen"
        );

        // The title-bar × key closes it as well (AGENTS.md §6.1).
        app.licenses_open = true;
        let _ = ctx.run(base.clone(), |ctx| app.draw(ctx));
        let settled = ctx.run(base.clone(), |ctx| app.draw(ctx));
        let at = window_close_button_center(&settled);
        let pointer_button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let mut click = base.clone();
        click.events = vec![
            egui::Event::PointerMoved(at),
            pointer_button(true),
            pointer_button(false),
        ];
        let _ = ctx.run(click, |ctx| app.draw(ctx));
        assert!(
            !app.licenses_open,
            "the title-bar × key must close the licenses screen"
        );

        // Escape closes it too.
        app.licenses_open = true;
        // Let the window lay out again before sending the key.
        let _ = ctx.run(base.clone(), |ctx| app.draw(ctx));
        let mut escape = base;
        escape.events = vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }];
        let _ = ctx.run(escape, |ctx| app.draw(ctx));
        assert!(!app.licenses_open, "Escape must close the licenses screen");
    }
}
