//! egui application: read-only views over the background worker.
//!
//! The screen is composed of three panels:
//!
//! * a top bar with the brand, a read-only pill, the connection status pill,
//!   the language switch, and the error banner;
//! * a left rail with the connection state, the read-only actions, the backup
//!   destination, and the safety note;
//! * a central panel that shows the onboarding flow (status card, three steps,
//!   scan-result list) while disconnected, and the tabbed read-only views once
//!   a device is connected.
//!
//! Every user-visible string comes from [`crate::i18n`]; every color, spacing,
//! and font size comes from [`crate::theme`].

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Frame, Layout, Margin, Rect, RichText, Sense, Stroke,
    Vec2,
};

use crate::backup::{default_backup_dir, BackupReport};
use crate::i18n::{self, Key, Lang};
use crate::model::{
    format_rssi, format_u16_hex, format_u32_hex, ConfigView, DeviceInfoView, InputView,
    MacroSlotView, ScanEntry, VersionView, BUTTON_LABELS,
};
use crate::theme;
use crate::worker::{Command, Event, Notice, Worker};

const MAX_LOG_ENTRIES: usize = 500;

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

    fn color(self) -> Color32 {
        match self {
            Self::Disconnected => theme::TEXT_MUTED,
            Self::Scanning | Self::Connecting => theme::STATUS_AMBER,
            Self::Connected => theme::STATUS_GREEN,
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
    Log,
}

impl Tab {
    const ALL: [Self; 4] = [Self::Device, Self::Input, Self::Config, Self::Log];

    fn key(self) -> Key {
        match self {
            Self::Device => Key::NavDevice,
            Self::Input => Key::NavInput,
            Self::Config => Key::NavConfig,
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

/// Main application state.
pub struct App {
    worker: Worker,
    connection: ConnectionState,
    scan_entries: Vec<ScanEntry>,
    selected_address: Option<String>,
    device: Option<DeviceInfoView>,
    version: Option<VersionView>,
    config: Option<ConfigView>,
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
                self.push_notice(LogLevel::Info, Notice::new(Key::LogScanStarted));
            }
            Event::ScanFinished(entries) => {
                if self.connection == ConnectionState::Scanning {
                    self.connection = ConnectionState::Disconnected;
                }
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
                self.input = None;
                self.live = false;
                self.backup = None;
                self.last_input_at = None;
                self.report_window_start = Instant::now();
                self.report_window_count = 0;
                self.reports_per_second = 0.0;
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
            Event::Config(config) => {
                self.push_notice(
                    LogLevel::Info,
                    Notice::new(Key::LogConfig)
                        .with_arg(config.size.to_string())
                        .with_arg(config.sha256.clone()),
                );
                self.config = Some(*config);
            }
            Event::Input(input) => {
                self.input = Some(input);
                self.note_input();
            }
            Event::Live(enabled) => {
                self.live = enabled;
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
            Event::Disconnected { reason } => {
                self.connection = ConnectionState::Disconnected;
                self.live = false;
                self.input = None;
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

    fn selected_app_address(&self) -> Option<String> {
        self.selected_address.clone().filter(|address| {
            self.scan_entries
                .iter()
                .any(|entry| entry.is_app && &entry.address == address)
        })
    }

    fn selected_entry(&self) -> Option<&ScanEntry> {
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

    /// The onboarding flow replaces the device view until data exists.
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

    // -----------------------------------------------------------------------
    // Top bar
    // -----------------------------------------------------------------------

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top-bar")
            .frame(
                Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(Margin::symmetric(theme::SPACE_4, theme::SPACE_2))
                    .stroke(Stroke::new(1.0_f32, theme::BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.text(Key::AppTitle))
                            .font(theme::bold(theme::FONT_SIZE_TITLE))
                            .color(theme::TEXT),
                    );
                    ui.add_space(theme::SPACE_2);
                    self.read_only_pill(ui);
                    ui.add_space(theme::SPACE_2);
                    self.status_pill(ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.language_switch(ui);
                    });
                });

                if let Some(busy) = self.busy {
                    ui.add_space(theme::SPACE_1);
                    ui.add(
                        egui::Label::new(
                            RichText::new(self.text(busy.key()))
                                .size(theme::FONT_SIZE_SMALL)
                                .color(theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                }

                if let Some(error) = self.error.clone() {
                    self.error_banner(ui, error);
                }
            });
    }

    fn read_only_pill(&self, ui: &mut egui::Ui) {
        Frame::none()
            .fill(theme::PILL_BG)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .rounding(11.0_f32)
            .inner_margin(Margin::symmetric(10.0, 3.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(self.text(Key::ReadOnly))
                        .size(theme::FONT_SIZE_SMALL)
                        .color(theme::TEXT_MUTED),
                );
            })
            .response
            .on_hover_text(self.text(Key::ReadOnlyHint));
    }

    fn status_pill(&self, ui: &mut egui::Ui) {
        let color = self.connection.color();
        Frame::none()
            .fill(theme::PILL_BG)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .rounding(14.0_f32)
            .inner_margin(Margin::symmetric(10.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    status_dot(ui, color, theme::STATUS_DOT_SIZE);
                    ui.label(
                        RichText::new(self.text(self.connection.key()))
                            .font(theme::bold(theme::FONT_SIZE_BODY))
                            .color(color),
                    );
                    if self.busy.is_some() {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                });
            });
    }

    fn language_switch(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(self.text(Key::Language))
                    .size(theme::FONT_SIZE_SMALL)
                    .color(theme::TEXT_MUTED),
            );
            for (lang, key) in [(Lang::Ja, Key::LangJapanese), (Lang::En, Key::LangEnglish)] {
                let response = ui.add_sized(
                    Vec2::new(72.0, 28.0),
                    egui::SelectableLabel::new(
                        self.lang == lang,
                        RichText::new(i18n::t(lang, key)).size(theme::FONT_SIZE_SMALL),
                    ),
                );
                if response.clicked() {
                    self.lang = lang;
                }
            }
        });
    }

    fn error_banner(&mut self, ui: &mut egui::Ui, notice: Notice) {
        ui.add_space(theme::SPACE_2);
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 44.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, theme::RADIUS, theme::ERROR_BG);
        painter.rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height())),
            theme::RADIUS,
            theme::STATUS_RED,
        );

        let dismiss = self.text(Key::Dismiss).to_owned();
        let close_size = Vec2::new(96.0, 28.0);
        let close_rect = Rect::from_center_size(
            egui::pos2(
                rect.right() - theme::SPACE_3 - close_size.x / 2.0,
                rect.center().y,
            ),
            close_size,
        );
        if ui
            .put(close_rect, egui::Button::new(dismiss).small())
            .clicked()
        {
            self.error = None;
        }

        let message = format!("{}: {}", self.text(Key::Error), self.notice_text(&notice));
        let text_rect = Rect::from_min_max(
            egui::pos2(rect.left() + theme::SPACE_3, rect.top() + 2.0),
            egui::pos2(close_rect.left() - theme::SPACE_2, rect.bottom() - 2.0),
        );
        let galley = ui.painter().layout(
            message,
            FontId::proportional(theme::FONT_SIZE_BODY),
            theme::STATUS_RED,
            text_rect.width(),
        );
        ui.painter().galley(
            egui::pos2(
                text_rect.left(),
                text_rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            theme::STATUS_RED,
        );
        if let Some(detail) = &notice.detail {
            let hover = format!("{}: {detail}", self.text(Key::ErrorDetails));
            ui.interact(text_rect, ui.id().with("error-detail"), Sense::hover())
                .on_hover_text(hover);
        }
    }

    // -----------------------------------------------------------------------
    // Left rail
    // -----------------------------------------------------------------------

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("controls")
            .resizable(false)
            .exact_width(theme::SIDE_PANEL_WIDTH)
            .frame(
                Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(Margin::symmetric(theme::SPACE_3, theme::SPACE_3))
                    .stroke(Stroke::new(1.0_f32, theme::BORDER)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("rail-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.connection_section(ui);
                        section_separator(ui);
                        self.actions_section(ui);
                        section_separator(ui);
                        self.backup_section(ui);
                        section_separator(ui);
                        self.safety_section(ui);
                    });
            });
    }

    fn connection_section(&mut self, ui: &mut egui::Ui) {
        section_label(ui, self.lang, Key::SectionConnection);
        ui.add_space(theme::SPACE_1);
        ui.horizontal(|ui| {
            status_dot(ui, self.connection.color(), theme::STATUS_DOT_SIZE);
            ui.label(
                RichText::new(self.text(self.connection.key()))
                    .font(theme::bold(theme::FONT_SIZE_SUBHEAD))
                    .color(self.connection.color()),
            );
        });

        if let Some(device) = &self.device {
            ui.add_space(theme::SPACE_2);
            ui.label(
                RichText::new(device.name.as_deref().unwrap_or(self.text(Key::NoName)))
                    .size(theme::FONT_SIZE_BODY)
                    .color(theme::TEXT),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(&device.address)
                        .monospace()
                        .size(theme::FONT_SIZE_SMALL)
                        .color(theme::TEXT_MUTED),
                )
                .wrap(),
            );
            ui.label(
                RichText::new(i18n::format(
                    self.lang,
                    Key::MtuValue,
                    &[device.mtu.to_string(), device.characteristic.clone()],
                ))
                .monospace()
                .size(theme::FONT_SIZE_SMALL)
                .color(theme::TEXT_MUTED),
            );
        } else {
            ui.add_space(theme::SPACE_1);
            muted_wrapped(ui, self.lang, Key::NoDeviceHint);
        }

        if self.connection.is_connected() {
            ui.add_space(theme::SPACE_2);
            if action_button(
                ui,
                self.can_read(),
                self.text(Key::Disconnect),
                Some(self.text(Key::HintDisconnect)),
            )
            .clicked()
            {
                self.request_disconnect();
            }
        }
    }

    fn actions_section(&mut self, ui: &mut egui::Ui) {
        section_label(ui, self.lang, Key::SectionActions);
        ui.add_space(theme::SPACE_1);
        let can_read = self.can_read();
        if !can_read {
            let reason = if self.connection.is_connected() {
                Key::BusyHint
            } else {
                Key::ConnectFirstHint
            };
            ui.label(
                RichText::new(self.text(reason))
                    .size(theme::FONT_SIZE_SMALL)
                    .color(theme::TEXT_MUTED),
            );
            ui.add_space(theme::SPACE_1);
        }

        if action_button(
            ui,
            can_read,
            self.text(Key::ReadVersionAction),
            Some(self.text(Key::HintReadVersion)),
        )
        .clicked()
        {
            self.send(Command::ReadVersion, Some(Busy::ReadVersion));
        }
        if action_button(
            ui,
            can_read,
            self.text(Key::ReadConfigAction),
            Some(self.text(Key::HintReadConfig)),
        )
        .clicked()
        {
            self.send(Command::ReadConfig, Some(Busy::ReadConfig));
        }
        if action_button(
            ui,
            can_read,
            self.text(Key::BackupConfigAction),
            Some(self.text(Key::BackupConfigHint)),
        )
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
        if action_button(
            ui,
            can_read,
            self.text(live_key),
            Some(self.text(Key::HintLive)),
        )
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
            muted_wrapped(ui, self.lang, Key::LiveStoppedHint);
        }
    }

    fn backup_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(
            RichText::new(self.text(Key::SectionBackupFolder))
                .font(theme::bold(theme::FONT_SIZE_SUBHEAD))
                .color(theme::TEXT),
        )
        .id_salt("backup-folder")
        .default_open(false)
        .show(ui, |ui| {
            wrapped_mono(
                ui,
                &self.backup_dir.display().to_string(),
                theme::TEXT_MUTED,
            );
            if let Some(report) = &self.backup {
                ui.add_space(theme::SPACE_2);
                ui.label(
                    RichText::new(self.text(Key::LastBackup))
                        .font(theme::bold(theme::FONT_SIZE_BODY))
                        .color(theme::TEXT),
                );
                wrapped_mono(
                    ui,
                    &report.raw_path.display().to_string(),
                    theme::TEXT_MUTED,
                );
                wrapped_mono(
                    ui,
                    &report.json_path.display().to_string(),
                    theme::TEXT_MUTED,
                );
                ui.add_space(theme::SPACE_1);
                let copy = self.text(Key::CopySha256);
                if ui.small_button(copy).clicked() {
                    ui.output_mut(|output| output.copied_text = report.sha256.clone());
                }
                wrapped_mono(ui, &report.sha256, theme::TEXT_BODY);
                for warning in &report.warnings {
                    ui.label(
                        RichText::new(format!("{}: {warning}", self.text(Key::Warning)))
                            .size(theme::FONT_SIZE_SMALL)
                            .color(theme::STATUS_AMBER),
                    );
                }
            }
        });
    }

    fn safety_section(&self, ui: &mut egui::Ui) {
        section_label(ui, self.lang, Key::SectionSafety);
        ui.add_space(theme::SPACE_1);
        muted_wrapped(ui, self.lang, Key::SafetyText);
    }

    // -----------------------------------------------------------------------
    // Central panel
    // -----------------------------------------------------------------------

    fn central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(theme::BG)
                    .inner_margin(Margin::symmetric(theme::SPACE_4, theme::SPACE_3)),
            )
            .show(ctx, |ui| {
                self.tab_bar(ui);
                ui.add_space(theme::SPACE_3);
                egui::ScrollArea::vertical()
                    .id_salt("central-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.tab {
                        Tab::Device => self.device_tab(ui),
                        Tab::Input => self.input_tab(ui),
                        Tab::Config => self.config_tab(ui),
                        Tab::Log => self.log_tab(ui),
                    });
            });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tab in Tab::ALL {
                if tab_button(ui, self.lang, tab, self.tab == tab) {
                    self.tab = tab;
                }
                ui.add_space(theme::SPACE_1);
            }
        });
    }

    fn connect_card(&mut self, ui: &mut egui::Ui) {
        Frame::none()
            .fill(theme::CARD)
            .rounding(8.0_f32)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .inner_margin(Margin::same(theme::SPACE_4))
            .show(ui, |ui| {
                ui.set_min_height(theme::CARD_MIN_HEIGHT);
                ui.horizontal(|ui| {
                    status_dot(ui, self.connection.color(), 10.0);
                    ui.label(
                        RichText::new(self.text(self.connection.key()))
                            .font(theme::bold(theme::FONT_SIZE_HEADING))
                            .color(theme::TEXT),
                    );
                });
                ui.add_space(theme::SPACE_1);
                ui.add(
                    egui::Label::new(
                        RichText::new(self.text(Key::OnboardingBody))
                            .size(theme::FONT_SIZE_BODY)
                            .color(theme::TEXT_BODY),
                    )
                    .wrap(),
                );
                ui.add_space(theme::SPACE_3);
                self.connect_controls(ui);
            });
    }

    fn connect_controls(&mut self, ui: &mut egui::Ui) {
        let idle = self.connection == ConnectionState::Disconnected && self.busy.is_none();
        ui.horizontal(|ui| {
            let connect_key = if self.device.is_some() {
                Key::Reconnect
            } else {
                Key::Connect
            };
            if primary_button(
                ui,
                idle,
                self.text(connect_key),
                Vec2::new(theme::CTA_WIDTH, theme::CTA_HEIGHT),
            )
            .on_hover_text(self.text(if self.selected_app_address().is_some() {
                Key::ConnectUsesSelected
            } else {
                Key::AutoDetect80nd
            }))
            .clicked()
            {
                self.request_connect();
            }
            ui.add_space(theme::SPACE_2);
            if secondary_button(
                ui,
                idle,
                self.text(Key::Scan),
                Vec2::new(120.0, theme::CTA_HEIGHT),
            )
            .on_hover_text(self.text(Key::ScanHint))
            .clicked()
            {
                self.request_scan();
            }
        });

        if self.selected_entry().is_none() {
            ui.add_space(theme::SPACE_1);
            ui.label(
                RichText::new(self.text(Key::AutoDetect80nd))
                    .size(theme::FONT_SIZE_SMALL)
                    .color(theme::TEXT_MUTED),
            );
        }

        if let Some(entry) = self.selected_entry() {
            let name = entry
                .name
                .clone()
                .unwrap_or_else(|| self.text(Key::NoName).to_string());
            let label = i18n::format(
                self.lang,
                Key::SelectedDevice,
                &[format!("{name} ({})", entry.address)],
            );
            ui.add_space(theme::SPACE_2);
            ui.label(
                RichText::new(label)
                    .size(theme::FONT_SIZE_SMALL)
                    .color(theme::TEXT_MUTED),
            );
        }
        let has_non_app_selection = self.selected_address.as_ref().is_some_and(|address| {
            self.scan_entries
                .iter()
                .any(|entry| &entry.address == address && !entry.is_app)
        });
        if has_non_app_selection {
            muted_wrapped(ui, self.lang, Key::NotAppMode);
        }
    }

    fn device_tab(&mut self, ui: &mut egui::Ui) {
        if !self.connection.is_connected() {
            self.connect_card(ui);
            ui.add_space(theme::SPACE_4);
        }

        if self.has_device_data() {
            if self.stale_device() {
                warning_banner(
                    ui,
                    self.lang,
                    Key::LastSessionWarning,
                    Some(Key::LastSessionHint),
                );
                ui.add_space(theme::SPACE_3);
            }
            self.device_details(ui);
            if !self.connection.is_connected() && !self.scan_entries.is_empty() {
                ui.add_space(theme::SPACE_4);
                self.scan_results(ui);
            }
            return;
        }

        if self.connection.is_connected() {
            empty_card(ui, self.lang, Key::NoDeviceInfo, None);
            return;
        }

        if self.wants_onboarding() {
            self.steps(ui);
            ui.add_space(theme::SPACE_4);
        }
        self.scan_results(ui);
    }

    fn device_details(&self, ui: &mut egui::Ui) {
        section_label(ui, self.lang, Key::SectionDeviceInfo);
        ui.add_space(theme::SPACE_1);
        if let Some(device) = &self.device {
            let name = device
                .name
                .clone()
                .unwrap_or_else(|| self.text(Key::NoName).to_string());
            key_value_grid(
                ui,
                "device-info",
                &[
                    (self.text(Key::Name).to_owned(), name),
                    (self.text(Key::Address).to_owned(), device.address.clone()),
                    (self.text(Key::Mtu).to_owned(), device.mtu.to_string()),
                    (
                        self.text(Key::Characteristic).to_owned(),
                        device.characteristic.clone(),
                    ),
                ],
                None,
            );
        } else {
            muted_wrapped(ui, self.lang, Key::NoDeviceInfo);
        }

        ui.add_space(theme::SPACE_4);
        section_label(ui, self.lang, Key::SectionFirmware);
        ui.add_space(theme::SPACE_1);
        if let Some(version) = &self.version {
            key_value_grid(
                ui,
                "version-info",
                &[
                    (
                        self.text(Key::FirmwareRaw).to_owned(),
                        if version.raw_hex.is_empty() {
                            self.text(Key::EmptyValue).to_owned()
                        } else {
                            version.raw_hex.clone()
                        },
                    ),
                    (
                        self.text(Key::FirmwareText).to_owned(),
                        version
                            .text
                            .clone()
                            .unwrap_or_else(|| self.text(Key::None).to_owned()),
                    ),
                    (
                        self.text(Key::FirmwareValue).to_owned(),
                        version.value.map_or_else(
                            || self.text(Key::NotFourByteValue).to_owned(),
                            format_u32_hex,
                        ),
                    ),
                ],
                None,
            );
        } else {
            muted_wrapped(ui, self.lang, Key::NoVersionYet);
        }
    }

    fn steps(&self, ui: &mut egui::Ui) {
        section_label(ui, self.lang, Key::SectionGettingStarted);
        ui.add_space(theme::SPACE_2);
        let steps = [
            (Key::Step1Title, Key::Step1Body),
            (Key::Step2Title, Key::Step2Body),
            (Key::Step3Title, Key::Step3Body),
        ];
        for (index, (title, body)) in steps.into_iter().enumerate() {
            step_row(ui, self.lang, index + 1, title, body);
            if index < steps.len() - 1 {
                ui.add_space(theme::SPACE_2);
            }
        }
    }

    fn scan_results(&mut self, ui: &mut egui::Ui) {
        section_label(ui, self.lang, Key::SectionScanResults);
        ui.add_space(theme::SPACE_2);
        if self.scan_entries.is_empty() {
            muted_wrapped(ui, self.lang, Key::NoScanResults);
            return;
        }

        let entries = self.scan_entries.clone();
        let mut selected = None;
        let mut connect_now = None;
        for entry in &entries {
            let is_selected = self.selected_address.as_deref() == Some(entry.address.as_str());
            let response = device_row(ui, self.lang, entry, is_selected);
            let activated = response.clicked()
                || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
            if activated {
                selected = Some(entry.address.clone());
            }
            if response.double_clicked() {
                let address = entry.is_app.then(|| entry.address.clone());
                connect_now = Some(address);
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
        muted_wrapped(ui, self.lang, Key::DeviceRowHint);
    }

    fn input_tab(&mut self, ui: &mut egui::Ui) {
        let Some(input) = self.input else {
            if self.connection.is_connected() {
                empty_card(
                    ui,
                    self.lang,
                    Key::NoInputYet,
                    Some(Key::InputWaitingReports),
                );
            } else {
                empty_card(
                    ui,
                    self.lang,
                    Key::NoInputYet,
                    Some(Key::InputWaitingConnect),
                );
                ui.add_space(theme::SPACE_2);
                self.connect_controls(ui);
            }
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                stick_widget(ui, self.lang, Key::LeftStick, None);
                ui.add_space(theme::SPACE_4);
                stick_widget(ui, self.lang, Key::RightStick, None);
            });
            return;
        };

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(i18n::format(
                    self.lang,
                    Key::ReportsRate,
                    &[format!("{:.1}", self.reports_per_second)],
                ))
                .size(theme::FONT_SIZE_BODY)
                .color(theme::TEXT_BODY),
            );
            if let Some(last) = self.last_input_at {
                ui.add_space(theme::SPACE_3);
                ui.label(
                    RichText::new(i18n::format(
                        self.lang,
                        Key::LastReportAgo,
                        &[format!("{:.0}", last.elapsed().as_secs_f64() * 1000.0)],
                    ))
                    .size(theme::FONT_SIZE_BODY)
                    .color(theme::TEXT_MUTED),
                );
            }
            ui.add_space(theme::SPACE_3);
            ui.label(
                RichText::new(i18n::format(
                    self.lang,
                    Key::BatteryRaw,
                    &[input.battery.to_string()],
                ))
                .size(theme::FONT_SIZE_BODY)
                .color(theme::TEXT_MUTED),
            );
        });

        ui.add_space(theme::SPACE_3);
        ui.horizontal(|ui| {
            stick_widget(ui, self.lang, Key::LeftStick, Some(input.left_stick));
            ui.add_space(theme::SPACE_4);
            stick_widget(ui, self.lang, Key::RightStick, Some(input.right_stick));
        });

        ui.add_space(theme::SPACE_2);
        let bar_width = ui.available_width().min(420.0);
        ui.add(
            egui::ProgressBar::new(f32::from(input.trigger_l) / 255.0)
                .desired_width(bar_width)
                .text(i18n::format(
                    self.lang,
                    Key::TriggerL,
                    &[input.trigger_l.to_string()],
                )),
        );
        ui.add(
            egui::ProgressBar::new(f32::from(input.trigger_r) / 255.0)
                .desired_width(bar_width)
                .text(i18n::format(
                    self.lang,
                    Key::TriggerR,
                    &[input.trigger_r.to_string()],
                )),
        );

        section_separator(ui);
        section_label(ui, self.lang, Key::Buttons);
        ui.add_space(theme::SPACE_1);
        ui.horizontal_wrapped(|ui| {
            for (engraved, flag) in BUTTON_LABELS {
                let active = input.keys.contains(flag);
                let label = i18n::button_name(self.lang, engraved);
                let text = RichText::new(format!(" {label} "))
                    .size(theme::FONT_SIZE_SMALL)
                    .color(if active {
                        Color32::BLACK
                    } else {
                        theme::TEXT_MUTED
                    })
                    .background_color(if active {
                        theme::STATUS_GREEN
                    } else {
                        theme::CONTROL
                    });
                ui.label(text);
            }
        });

        section_separator(ui);
        key_value_grid(
            ui,
            "input-raw",
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
                ),
                (
                    self.text(Key::KeyMask).to_owned(),
                    format!(
                        "{} ({})",
                        format_u32_hex(input.keys.bits()),
                        i18n::keymap_text(self.lang, input.keys)
                    ),
                ),
            ],
            None,
        );
    }

    fn config_tab(&mut self, ui: &mut egui::Ui) {
        if self.config.is_none() {
            if self.connection.is_connected() {
                empty_card(ui, self.lang, Key::NoConfigYet, None);
            } else {
                empty_card(
                    ui,
                    self.lang,
                    Key::NoConfigYet,
                    Some(Key::ConfigWaitingConnect),
                );
                ui.add_space(theme::SPACE_2);
                self.connect_controls(ui);
            }
            return;
        }

        if self.stale_config() {
            warning_banner(
                ui,
                self.lang,
                Key::LastSessionWarning,
                Some(Key::LastSessionHint),
            );
            ui.add_space(theme::SPACE_3);
        }

        let lang = self.lang;
        let Some(config) = self.config.as_ref() else {
            return;
        };

        section_label(ui, lang, Key::SectionSummary);
        ui.add_space(theme::SPACE_1);
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
        key_value_grid(
            ui,
            "config-summary",
            &[
                (
                    i18n::t(lang, Key::SizeLabel).to_owned(),
                    i18n::format(lang, Key::BytesValue, &[config.size.to_string()]),
                ),
                (i18n::t(lang, Key::Sha256).to_owned(), config.sha256.clone()),
                (
                    i18n::t(lang, Key::CrcStored).to_owned(),
                    format!("0x{:04X}", config.stored_crc),
                ),
                (
                    i18n::t(lang, Key::CrcInPlace).to_owned(),
                    format!("0x{:04X} ({crc_notice})", config.computed_crc),
                ),
                (
                    i18n::t(lang, Key::ActiveProfile).to_owned(),
                    i18n::active_profile_text(lang, config.cur_slot_raw),
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
                ),
                (
                    i18n::t(lang, Key::ProfileFlags).to_owned(),
                    format!("[{flags}]"),
                ),
            ],
            Some(i18n::t(lang, Key::CrcNote)),
        );

        ui.add_space(theme::SPACE_4);
        section_label(ui, lang, Key::SectionProfiles);
        ui.add_space(theme::SPACE_1);
        ui.horizontal_wrapped(|ui| {
            for (index, name) in config.profile_names.iter().enumerate() {
                let shown = if name.is_empty() {
                    i18n::t(lang, Key::EmptyProfileName)
                } else {
                    name.as_str()
                };
                let row = i18n::format(
                    lang,
                    Key::ProfileRow,
                    &[
                        i18n::profile_text(lang, index as u8 + 1),
                        shown.to_owned(),
                        format_u32_hex(config.slot_flags[index]),
                    ],
                );
                ui.label(
                    RichText::new(row)
                        .size(theme::FONT_SIZE_BODY)
                        .color(theme::TEXT_BODY),
                );
            }
        });

        ui.add_space(theme::SPACE_4);
        section_label(ui, lang, Key::SectionMacros);
        ui.add_space(theme::SPACE_1);
        muted_wrapped(ui, lang, Key::MacroHeaderHint);
        for slot in &config.macro_slots {
            macro_slot(ui, lang, slot);
        }
    }

    fn log_tab(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(self.text(Key::Clear)).clicked() {
                self.log.clear();
            }
        });
        ui.separator();
        ui.add_space(theme::SPACE_1);
        if self.log.is_empty() {
            muted_wrapped(ui, self.lang, Key::LogEmpty);
            return;
        }
        for entry in &self.log {
            let color = match entry.level {
                LogLevel::Info => theme::TEXT_BODY,
                LogLevel::Error => theme::STATUS_RED,
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&entry.at)
                        .monospace()
                        .size(theme::FONT_SIZE_SMALL)
                        .color(theme::TEXT_MUTED),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(self.notice_text(&entry.notice))
                            .size(theme::FONT_SIZE_BODY)
                            .color(color),
                    )
                    .wrap(),
                );
                if let Some(detail) = &entry.notice.detail {
                    ui.label(
                        RichText::new(detail)
                            .monospace()
                            .size(theme::FONT_SIZE_SMALL)
                            .color(theme::TEXT_MUTED),
                    )
                    .on_hover_text(detail);
                }
            });
        }
    }
}

impl App {
    /// Draw the whole window. Split out from [`eframe::App::update`] so tests
    /// can render it in a headless egui context.
    fn draw(&mut self, ctx: &egui::Context) {
        self.top_bar(ctx);
        self.side_panel(ctx);
        self.central_panel(ctx);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        if !self.worker.is_running() && !self.connection.is_connected() {
            // The worker only stops after `Shutdown` or a fatal thread error;
            // surface the stopped state without spamming the log every frame.
            if !self.log.iter().any(|entry| {
                entry.notice.key == Key::LogWorkerStopped && entry.level == LogLevel::Error
            }) {
                self.connection = ConnectionState::Disconnected;
                self.live = false;
                self.input = None;
                self.busy = None;
                self.push_notice(LogLevel::Error, Notice::new(Key::LogWorkerStopped));
            }
        }
        self.draw(ctx);

        // Checked after drawing so a button pressed in this frame schedules the
        // next frame. Events arrive over a std channel and cannot wake egui.
        let idling =
            self.connection == ConnectionState::Disconnected && self.busy.is_none() && !self.live;
        let active = self.busy.is_some() || self.live;
        let interval = if active {
            16
        } else if idling {
            250
        } else {
            100
        };
        ctx.request_repaint_after(Duration::from_millis(interval));
    }
}

// ---------------------------------------------------------------------------
// Small layout helpers
// ---------------------------------------------------------------------------

fn section_label(ui: &mut egui::Ui, lang: Lang, key: Key) {
    ui.label(
        RichText::new(i18n::t(lang, key))
            .font(theme::bold(theme::FONT_SIZE_SUBHEAD))
            .color(theme::TEXT_MUTED),
    );
}

fn section_separator(ui: &mut egui::Ui) {
    ui.add_space(theme::SPACE_3);
    ui.separator();
    ui.add_space(theme::SPACE_2);
}

fn muted_wrapped(ui: &mut egui::Ui, lang: Lang, key: Key) {
    ui.add(
        egui::Label::new(
            RichText::new(i18n::t(lang, key))
                .size(theme::FONT_SIZE_BODY)
                .color(theme::TEXT_MUTED),
        )
        .wrap(),
    );
}

fn wrapped_mono(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .monospace()
                .size(theme::FONT_SIZE_SMALL)
                .color(color),
        )
        .wrap(),
    );
}

fn status_dot(ui: &mut egui::Ui, color: Color32, diameter: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), diameter / 2.0, color);
}

fn focus_ring(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            theme::RADIUS,
            Stroke::new(theme::FOCUS_RING_WIDTH, theme::ACCENT),
        );
    }
}

fn primary_button(ui: &mut egui::Ui, enabled: bool, label: &str, size: Vec2) -> egui::Response {
    if enabled {
        let response = ui
            .scope(|ui| {
                let widgets = &mut ui.style_mut().visuals.widgets;
                widgets.inactive.weak_bg_fill = theme::ACCENT;
                widgets.inactive.bg_stroke = Stroke::NONE;
                widgets.inactive.fg_stroke.color = theme::ON_ACCENT;
                widgets.hovered.weak_bg_fill = theme::ACCENT_HOVER;
                widgets.hovered.bg_stroke = Stroke::NONE;
                widgets.hovered.fg_stroke.color = theme::ON_ACCENT;
                widgets.active.weak_bg_fill = theme::ACCENT_PRESSED;
                widgets.active.bg_stroke = Stroke::NONE;
                widgets.active.fg_stroke.color = theme::ON_ACCENT;
                ui.add_sized(
                    size,
                    egui::Button::new(RichText::new(label).size(theme::FONT_SIZE_SUBHEAD))
                        .rounding(theme::RADIUS),
                )
            })
            .inner;
        focus_ring(ui, &response);
        response
    } else {
        ui.add_enabled(
            false,
            egui::Button::new(
                RichText::new(label)
                    .size(theme::FONT_SIZE_SUBHEAD)
                    .color(theme::DISABLED_TEXT_PRE_FADE),
            )
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0_f32, theme::DISABLED_BORDER_PRE_FADE))
            .min_size(size)
            .rounding(theme::RADIUS),
        )
    }
}

fn secondary_button(ui: &mut egui::Ui, enabled: bool, label: &str, size: Vec2) -> egui::Response {
    if enabled {
        let response = ui
            .scope(|ui| {
                let widgets = &mut ui.style_mut().visuals.widgets;
                widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                widgets.inactive.bg_stroke = Stroke::new(1.0_f32, theme::BORDER_STRONG);
                widgets.inactive.fg_stroke.color = theme::TEXT;
                widgets.hovered.weak_bg_fill = theme::CONTROL_HOVER;
                widgets.hovered.bg_stroke = Stroke::new(1.0_f32, theme::BORDER_STRONG);
                widgets.hovered.fg_stroke.color = Color32::WHITE;
                ui.add_sized(
                    size,
                    egui::Button::new(RichText::new(label).size(theme::FONT_SIZE_SUBHEAD))
                        .rounding(theme::RADIUS),
                )
            })
            .inner;
        focus_ring(ui, &response);
        response
    } else {
        ui.add_enabled(
            false,
            egui::Button::new(
                RichText::new(label)
                    .size(theme::FONT_SIZE_SUBHEAD)
                    .color(theme::DISABLED_TEXT_PRE_FADE),
            )
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0_f32, theme::DISABLED_BORDER_PRE_FADE))
            .min_size(size)
            .rounding(theme::RADIUS),
        )
    }
}

fn action_button(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    hint: Option<&str>,
) -> egui::Response {
    let size = Vec2::new(ui.available_width(), theme::BUTTON_HEIGHT);
    let response = if enabled {
        let response = ui
            .scope(|ui| {
                ui.add_sized(
                    size,
                    egui::Button::new(RichText::new(label).size(theme::FONT_SIZE_BODY))
                        .rounding(theme::RADIUS),
                )
            })
            .inner;
        focus_ring(ui, &response);
        response
    } else {
        ui.add_enabled(
            false,
            egui::Button::new(
                RichText::new(label)
                    .size(theme::FONT_SIZE_BODY)
                    .color(theme::DISABLED_TEXT_PRE_FADE),
            )
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0_f32, theme::DISABLED_BORDER_PRE_FADE))
            .min_size(size)
            .rounding(theme::RADIUS),
        )
    };
    match hint {
        Some(hint) => response.on_hover_text(hint),
        None => response,
    }
}

fn key_value_grid(ui: &mut egui::Ui, id: &str, rows: &[(String, String)], tooltip: Option<&str>) {
    let response = egui::Grid::new(id)
        .num_columns(2)
        .spacing([theme::SPACE_3, theme::SPACE_2])
        .striped(true)
        .show(ui, |ui| {
            for (key, value) in rows {
                ui.label(
                    RichText::new(key)
                        .size(theme::FONT_SIZE_BODY)
                        .color(theme::TEXT_MUTED),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(value)
                            .monospace()
                            .size(theme::FONT_SIZE_BODY)
                            .color(theme::TEXT_BODY),
                    )
                    .wrap(),
                );
                ui.end_row();
            }
        })
        .response;
    if let Some(tooltip) = tooltip {
        response.on_hover_text(tooltip);
    }
}

fn tab_button(ui: &mut egui::Ui, lang: Lang, tab: Tab, selected: bool) -> bool {
    let label = i18n::t(lang, tab.key());
    let width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(
                label.to_owned(),
                theme::bold(theme::FONT_SIZE_SUBHEAD),
                theme::TEXT,
            )
            .size()
            .x
    }) + 32.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, theme::TAB_HEIGHT), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::CONTROL);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::bold(theme::FONT_SIZE_SUBHEAD),
        if selected {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        },
    );
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(
                egui::pos2(rect.left() + theme::SPACE_2, rect.bottom() - 2.0),
                Vec2::new(rect.width() - theme::SPACE_2 * 2.0, 2.0),
            ),
            1.0,
            theme::ACCENT,
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            theme::RADIUS,
            Stroke::new(theme::FOCUS_RING_WIDTH, theme::ACCENT),
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });

    let clicked = response.clicked()
        || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
    if clicked {
        ui.ctx().request_repaint();
    }
    clicked
}

fn step_row(ui: &mut egui::Ui, lang: Lang, number: usize, title: Key, body: Key) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 10.0, theme::CONTROL);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            number.to_string(),
            theme::bold(theme::FONT_SIZE_SMALL),
            theme::TEXT,
        );
        ui.add_space(theme::SPACE_2);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(i18n::t(lang, title))
                    .font(theme::bold(theme::FONT_SIZE_SUBHEAD))
                    .color(theme::TEXT),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(i18n::t(lang, body))
                        .size(theme::FONT_SIZE_BODY)
                        .color(theme::TEXT_MUTED),
                )
                .wrap(),
            );
        });
    });
}

fn warning_banner(ui: &mut egui::Ui, lang: Lang, key: Key, hint: Option<Key>) {
    Frame::none()
        .fill(theme::WARNING_BG)
        .rounding(theme::RADIUS)
        .stroke(Stroke::new(1.0_f32, theme::STATUS_AMBER))
        .inner_margin(Margin::symmetric(theme::SPACE_3, theme::SPACE_2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                status_dot(ui, theme::STATUS_AMBER, theme::STATUS_DOT_SIZE);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(i18n::t(lang, key))
                            .size(theme::FONT_SIZE_BODY)
                            .color(theme::STATUS_AMBER),
                    );
                    if let Some(hint) = hint {
                        ui.label(
                            RichText::new(i18n::t(lang, hint))
                                .size(theme::FONT_SIZE_SMALL)
                                .color(theme::TEXT_MUTED),
                        );
                    }
                });
            });
        });
}

fn empty_card(ui: &mut egui::Ui, lang: Lang, title: Key, body: Option<Key>) {
    Frame::none()
        .fill(theme::CARD)
        .rounding(8.0_f32)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .inner_margin(Margin::same(theme::SPACE_3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(i18n::t(lang, title))
                    .font(theme::bold(theme::FONT_SIZE_SUBHEAD))
                    .color(theme::TEXT),
            );
            if let Some(body) = body {
                ui.add_space(theme::SPACE_1);
                muted_wrapped(ui, lang, body);
            }
        });
}

/// Quantize an RSSI reading into 0..=4 bars.
fn rssi_bars(rssi: Option<i16>) -> u8 {
    match rssi {
        None => 0,
        Some(value) if value >= -50 => 4,
        Some(value) if value >= -62 => 3,
        Some(value) if value >= -74 => 2,
        Some(_) => 1,
    }
}

fn device_row(ui: &mut egui::Ui, lang: Lang, entry: &ScanEntry, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, theme::DEVICE_ROW_HEIGHT), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));

    let fill = if selected || response.hovered() {
        theme::CARD_HOVER
    } else {
        theme::CARD
    };
    painter.rect(
        rect,
        theme::RADIUS,
        fill,
        Stroke::new(
            1.0_f32,
            if selected {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ),
    );
    if selected {
        painter.rect_filled(
            Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + 10.0),
                Vec2::new(3.0, rect.height() - 20.0),
            ),
            1.5,
            theme::ACCENT,
        );
    }

    let badge_text = if entry.is_app {
        i18n::t(lang, Key::AppModeBadge)
    } else {
        i18n::t(lang, Key::OtherModeBadge)
    };
    let badge_galley = ui.fonts(|fonts| {
        fonts.layout_no_wrap(
            badge_text.to_owned(),
            theme::bold(theme::FONT_SIZE_SMALL),
            theme::TEXT,
        )
    });
    let badge_size = Vec2::new(badge_galley.size().x + 16.0, 22.0);
    let badge_rect = Rect::from_center_size(
        egui::pos2(rect.right() - 14.0 - badge_size.x / 2.0, rect.center().y),
        badge_size,
    );

    let name = entry
        .name
        .clone()
        .unwrap_or_else(|| i18n::t(lang, Key::NoName).to_string());
    let text_clip = Rect::from_min_max(
        egui::pos2(rect.left() + 12.0, rect.top()),
        egui::pos2(badge_rect.left() - 12.0, rect.bottom()),
    );
    let text_painter = painter.with_clip_rect(text_clip);
    text_painter.text(
        egui::pos2(rect.left() + 14.0, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        &name,
        theme::bold(theme::FONT_SIZE_BODY),
        theme::TEXT,
    );
    text_painter.text(
        egui::pos2(rect.left() + 14.0, rect.center().y + 9.0),
        Align2::LEFT_CENTER,
        &entry.address,
        FontId::monospace(theme::FONT_SIZE_SMALL),
        theme::TEXT_MUTED,
    );
    painter.rect(
        badge_rect,
        4.0_f32,
        if entry.is_app {
            theme::BADGE_BG
        } else {
            theme::PILL_BG
        },
        Stroke::new(
            1.0_f32,
            if entry.is_app {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ),
    );
    painter.text(
        badge_rect.center(),
        Align2::CENTER_CENTER,
        badge_text,
        theme::bold(theme::FONT_SIZE_SMALL),
        if entry.is_app {
            theme::LINK
        } else {
            theme::TEXT_MUTED
        },
    );

    let bars = rssi_bars(entry.rssi);
    let bar_width = 3.0;
    let bar_gap = 2.0;
    let bars_left = badge_rect.left() - 12.0 - 4.0 * bar_gap - 4.0 * bar_width;
    let baseline = rect.center().y + 9.0;
    for index in 0..4 {
        let height = 4.0 + 3.0 * index as f32;
        let x = bars_left + index as f32 * (bar_width + bar_gap);
        let bar_rect = Rect::from_min_max(
            egui::pos2(x, baseline - height),
            egui::pos2(x + bar_width, baseline),
        );
        painter.rect_filled(
            bar_rect,
            1.0_f32,
            if index < usize::from(bars) {
                match bars {
                    4 | 3 => theme::STATUS_GREEN,
                    2 => theme::STATUS_AMBER,
                    _ => theme::TEXT_MUTED,
                }
            } else {
                theme::BORDER
            },
        );
    }

    let accessible_name = name.clone();
    let accessible_address = entry.address.clone();
    response.widget_info(move || {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!("{accessible_name} ({accessible_address})"),
        )
    });
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            theme::RADIUS,
            Stroke::new(theme::FOCUS_RING_WIDTH, theme::ACCENT),
        );
    }
    response.on_hover_text(format_rssi(entry.rssi))
}

fn stick_widget(ui: &mut egui::Ui, lang: Lang, label: Key, value: Option<(i16, i16)>) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(i18n::t(lang, label))
                .size(theme::FONT_SIZE_BODY)
                .color(theme::TEXT_MUTED),
        );
        ui.add_space(theme::SPACE_1);
        let (response, painter) =
            ui.allocate_painter(Vec2::splat(theme::STICK_SIZE), Sense::hover());
        let rect = response.rect;
        let center = rect.center();
        let radius = theme::STICK_SIZE / 2.0;

        painter.circle_filled(center, radius, theme::PANEL);
        painter.circle_stroke(center, radius - 1.0, Stroke::new(1.0_f32, theme::BORDER));
        painter.line_segment(
            [
                egui::pos2(rect.left() + 10.0, center.y),
                egui::pos2(rect.right() - 10.0, center.y),
            ],
            Stroke::new(1.0_f32, theme::BORDER),
        );
        painter.line_segment(
            [
                egui::pos2(center.x, rect.top() + 10.0),
                egui::pos2(center.x, rect.bottom() - 10.0),
            ],
            Stroke::new(1.0_f32, theme::BORDER),
        );
        painter.circle_stroke(center, 14.0, Stroke::new(1.0_f32, theme::BORDER_STRONG));

        match value {
            Some((x, y)) => {
                let travel = radius - 16.0;
                let point = egui::pos2(
                    center.x + f32::from(x) / 128.0 * travel,
                    center.y - f32::from(y) / 128.0 * travel,
                );
                painter.circle_filled(point, 10.0, theme::ACCENT);
                painter.circle_stroke(point, 10.0, Stroke::new(1.5_f32, Color32::WHITE));
                ui.monospace(i18n::format(
                    lang,
                    Key::StickRaw,
                    &[x.to_string(), y.to_string()],
                ));
            }
            None => {
                ui.label(
                    RichText::new("-")
                        .monospace()
                        .size(theme::FONT_SIZE_SMALL)
                        .color(theme::TEXT_MUTED),
                );
            }
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
    egui::CollapsingHeader::new(
        RichText::new(title)
            .size(theme::FONT_SIZE_BODY)
            .color(theme::TEXT_BODY),
    )
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
        .striped(true)
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
                ui.label(
                    RichText::new(i18n::t(lang, heading))
                        .font(theme::bold(theme::FONT_SIZE_SMALL))
                        .color(theme::TEXT_MUTED),
                );
            }
            ui.end_row();

            for record in &slot.records {
                let name = if record.name.is_empty() {
                    i18n::t(lang, Key::EmptyProfileName).to_owned()
                } else {
                    record.name.clone()
                };
                ui.monospace(record.number.to_string());
                ui.monospace(name);
                ui.monospace(record.gamepad_mode.to_string());
                ui.monospace(record.special_flag.to_string());
                ui.monospace(record.max_steps.to_string());
                ui.monospace(format_u16_hex(record.offset));
                ui.monospace(&record.key_map);
                ui.monospace(record.cycles.to_string());
                ui.monospace(i18n::format(
                    lang,
                    Key::IntervalMs,
                    &[record.interval_ms.to_string()],
                ));
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use eightbitdo_core::ConfigSnapshot;
    use eightbitdo_protocol::config_bt2::Bt2Config;

    use super::*;

    fn config_view() -> ConfigView {
        let config = Bt2Config::default();
        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        ConfigView::from_snapshot(&ConfigSnapshot {
            config,
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        })
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
    fn headless_draw_covers_onboarding_stale_data_and_every_tab() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                Vec2::new(1200.0, 800.0),
            )),
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

        // Stale session data plus an error banner and every tab connected.
        app.device = Some(device());
        app.version = Some(VersionView {
            raw_hex: "c8000000".to_string(),
            text: None,
            value: Some(200),
        });
        app.config = Some(config_view());
        app.error = Some(Notice::new(Key::ErrScanFailed).with_detail("scan failed"));
        let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));

        app.connection = ConnectionState::Connected;
        app.input = Some(InputView::default());
        app.live = true;
        for tab in Tab::ALL {
            app.tab = tab;
            let _ = ctx.run(input.clone(), |ctx| app.draw(ctx));
        }
    }

    #[test]
    fn connection_and_read_events_drive_the_busy_state() {
        let mut app = app();
        assert_eq!(app.connection, ConnectionState::Disconnected);
        assert_eq!(app.lang, Lang::Ja);

        app.handle_event(Event::ScanStarted);
        assert_eq!(app.connection, ConnectionState::Scanning);
        assert!(app.busy.is_some());

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

        app.handle_event(Event::Config(Box::new(config_view())));
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

        // Disconnected with cached data: warning, but no onboarding.
        assert_eq!(app.connection, ConnectionState::Disconnected);
        assert!(app.stale_device());
        assert!(app.stale_config());
        assert!(!app.wants_onboarding());

        // Connected: never stale.
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

        app.selected_address = Some("33".to_string());
        assert_eq!(app.selected_app_address(), None);
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
    fn busy_states_have_localized_labels() {
        for busy in [
            Busy::Scanning,
            Busy::Connecting,
            Busy::Reconnecting,
            Busy::InitialRead,
            Busy::ReadVersion,
            Busy::ReadConfig,
            Busy::Backup,
            Busy::Disconnecting,
        ] {
            let ja = i18n::t(Lang::Ja, busy.key());
            let en = i18n::t(Lang::En, busy.key());
            assert!(!ja.is_empty() && !en.is_empty());
            assert_ne!(ja, en);
        }
    }

    #[test]
    // The layout tokens are compile-time constants; this test guards them.
    #[allow(clippy::assertions_on_constants)]
    fn layout_fits_the_minimum_window_size() {
        // The window's minimum size is 900x620 (main.rs). 820 is used here as a
        // conservative lower bound: even that width leaves enough central room.
        let central = 900.0 - theme::SIDE_PANEL_WIDTH;
        assert!(central >= 520.0);
        let conservative = 820.0 - theme::SIDE_PANEL_WIDTH;
        let cta_row = theme::CTA_WIDTH + 120.0 + theme::SPACE_2;
        assert!(cta_row < conservative);
        assert!(theme::DEVICE_ROW_HEIGHT >= 40.0);
        assert!(theme::CARD_MIN_HEIGHT >= 120.0);
        // Two 14 px Japanese labels must fit the rail width (about 19 chars).
        assert!(theme::SIDE_PANEL_WIDTH - 2.0 * theme::SPACE_3 >= 19.0 * theme::FONT_SIZE_BODY);
    }

    #[test]
    fn status_colors_are_used_consistently() {
        assert_eq!(ConnectionState::Disconnected.color(), theme::TEXT_MUTED);
        assert_eq!(ConnectionState::Scanning.color(), theme::STATUS_AMBER);
        assert_eq!(ConnectionState::Connecting.color(), theme::STATUS_AMBER);
        assert_eq!(ConnectionState::Connected.color(), theme::STATUS_GREEN);
        assert_ne!(
            ConnectionState::Disconnected.key(),
            ConnectionState::Connected.key()
        );
    }
}
