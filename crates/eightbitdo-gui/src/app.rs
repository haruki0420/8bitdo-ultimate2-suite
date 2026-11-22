//! egui application: a read-only view over the background worker.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Stroke};

use crate::backup::{default_backup_dir, BackupReport};
use crate::model::{
    display_text, format_rssi, format_u16_hex, format_u32_hex, ConfigView, DeviceInfoView,
    InputView, MacroSlotView, ScanEntry, VersionView, BUTTON_LABELS,
};
use crate::worker::{Command, Event, Worker};

const MAX_LOG_ENTRIES: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Disconnected,
    Scanning,
    Connecting,
    Connected,
}

impl ConnectionState {
    fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Scanning => "Scanning",
            Self::Connecting => "Connecting",
            Self::Connected => "Connected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Device,
    Input,
    Config,
    Log,
}

struct LogEntry {
    at: String,
    error: bool,
    text: String,
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
    busy: Option<String>,
    backup: Option<BackupReport>,
    backup_dir: PathBuf,
    error: Option<String>,
    log: VecDeque<LogEntry>,
    tab: Tab,
    report_window_start: Instant,
    report_window_count: u32,
    reports_per_second: f32,
    last_input_at: Option<Instant>,
}

impl App {
    /// Create the app and start the background BLE worker.
    #[must_use]
    pub fn new(creation: &eframe::CreationContext<'_>) -> Self {
        creation.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut style = (*creation.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        creation.egui_ctx.set_style(style);

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
                self.busy = Some("Scanning for BLE devices...".to_string());
                self.push_log("Scan started.", false);
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
                self.push_log(
                    format!(
                        "Scan finished: {} device(s), {app_count} App Mode.",
                        entries.len()
                    ),
                    false,
                );
                self.scan_entries = entries;
                self.busy = None;
            }
            Event::Connecting => {
                self.connection = ConnectionState::Connecting;
                self.busy = Some("Connecting...".to_string());
                self.push_log("Connecting.", false);
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
                self.busy = Some("Reading version and configuration...".to_string());
                self.error = None;
                self.push_log("Connected.", false);
            }
            Event::Version(version) => {
                let summary = version
                    .value
                    .map_or_else(|| version.raw_hex.clone(), |value| format!("0x{value:08X}"));
                self.push_log(format!("Version: {summary}"), false);
                self.version = Some(version);
            }
            Event::Config(config) => {
                self.push_log(
                    format!(
                        "Configuration: {} bytes, SHA-256 {}",
                        config.size, config.sha256
                    ),
                    false,
                );
                self.config = Some(*config);
            }
            Event::Input(input) => {
                self.input = Some(input);
                self.note_input();
            }
            Event::Live(enabled) => {
                self.live = enabled;
                self.push_log(
                    if enabled {
                        "Live input started."
                    } else {
                        "Live input stopped."
                    },
                    false,
                );
            }
            Event::Backup(report) => {
                self.push_log(
                    format!(
                        "Backup written: {} (SHA-256 {})",
                        report.raw_path.display(),
                        report.sha256
                    ),
                    false,
                );
                self.backup = Some(*report);
            }
            Event::Disconnected { reason } => {
                self.connection = ConnectionState::Disconnected;
                self.live = false;
                self.input = None;
                self.busy = None;
                if let Some(reason) = reason {
                    self.push_log(format!("Disconnected: {reason}"), true);
                    self.error = Some(reason);
                } else {
                    self.push_log("Disconnected.", false);
                }
            }
            Event::Error(message) => {
                self.push_log(message.clone(), true);
                self.error = Some(message);
                self.busy = None;
            }
            Event::Log(message) => {
                self.push_log(message, false);
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

    fn push_log(&mut self, text: impl Into<String>, error: bool) {
        if self.log.len() >= MAX_LOG_ENTRIES {
            self.log.pop_front();
        }
        self.log.push_back(LogEntry {
            at: chrono::Local::now().format("%H:%M:%S").to_string(),
            error,
            text: text.into(),
        });
    }

    fn send(&mut self, command: Command, busy: Option<&str>) {
        if self.worker.send(command) {
            if let Some(busy) = busy {
                self.busy = Some(busy.to_string());
            }
        } else {
            let message = "The background worker is not running. Restart the application.";
            self.push_log(message, true);
            self.error = Some(message.to_string());
        }
    }

    fn can_read(&self) -> bool {
        self.connection == ConnectionState::Connected && self.busy.is_none()
    }

    fn selected_app_address(&self) -> Option<String> {
        self.selected_address.clone().filter(|address| {
            self.scan_entries
                .iter()
                .any(|entry| entry.is_app && &entry.address == address)
        })
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top-bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("8BitDo Ultimate 2");
                ui.label(RichText::new("read-only").small().weak());
                ui.separator();
                let (label, color) = match self.connection {
                    ConnectionState::Disconnected => {
                        (self.connection.label(), Color32::from_gray(160))
                    }
                    ConnectionState::Scanning => {
                        (self.connection.label(), Color32::from_rgb(230, 190, 90))
                    }
                    ConnectionState::Connecting => {
                        (self.connection.label(), Color32::from_rgb(230, 190, 90))
                    }
                    ConnectionState::Connected => {
                        (self.connection.label(), Color32::from_rgb(120, 220, 140))
                    }
                };
                ui.label(
                    RichText::new(label)
                        .strong()
                        .color(color)
                        .background_color(color.gamma_multiply(0.15)),
                );
                if let Some(busy) = &self.busy {
                    ui.spinner();
                    ui.label(busy);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });

            let error = self.error.clone();
            if let Some(error) = error {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::from_rgb(255, 120, 120), format!("Error: {error}"));
                    if ui.button("Dismiss").clicked() {
                        self.error = None;
                    }
                });
            }
        });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("controls")
            .resizable(false)
            .exact_width(300.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.connection_controls(ui);
                        ui.separator();
                        self.action_controls(ui);
                        ui.separator();
                        ui.label(RichText::new("Safety").strong());
                        ui.weak(
                            "Read-only build. No settings writes, macro erase/save, restore, or \
                             firmware update are implemented.",
                        );
                    });
            });
    }

    fn connection_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Connection");
        let idle = self.connection == ConnectionState::Disconnected && self.busy.is_none();
        ui.horizontal(|ui| {
            if ui.add_enabled(idle, egui::Button::new("Scan")).clicked() {
                self.send(Command::Scan, Some("Scanning..."));
            }
            let reconnecting = self.device.is_some();
            if ui
                .add_enabled(
                    idle,
                    egui::Button::new(if reconnecting { "Reconnect" } else { "Connect" }),
                )
                .clicked()
            {
                let address = self.selected_app_address();
                self.send(
                    Command::Connect { address },
                    Some(if reconnecting {
                        "Reconnecting..."
                    } else {
                        "Connecting..."
                    }),
                );
            }
            if ui
                .add_enabled(
                    self.connection == ConnectionState::Connected,
                    egui::Button::new("Disconnect"),
                )
                .clicked()
            {
                self.send(Command::Disconnect, Some("Disconnecting..."));
            }
        });

        ui.label("Target");
        let entries = &self.scan_entries;
        let selected = &mut self.selected_address;
        let selected_text = selected
            .as_ref()
            .and_then(|address| entries.iter().find(|entry| &entry.address == address))
            .map_or_else(|| "Any 80ND (scan by name)".to_string(), scan_entry_label);
        egui::ComboBox::from_id_salt("device-select")
            .width(ui.available_width())
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                ui.selectable_value(selected, None, "Any 80ND (scan by name)");
                for entry in entries {
                    ui.selectable_value(
                        selected,
                        Some(entry.address.clone()),
                        scan_entry_label(entry),
                    );
                }
            });
        if entries.is_empty() {
            ui.weak("No scan results yet. Scan, or connect by name.");
        } else if selected.as_ref().is_some_and(|address| {
            entries
                .iter()
                .any(|entry| entry.is_app && &entry.address == address)
        }) {
            ui.weak("Connect uses the selected address.");
        } else if selected.is_some() {
            ui.weak("Selected device is not an App Mode peripheral; Connect falls back to 80ND by name.");
        }

        if let Some(device) = &self.device {
            ui.separator();
            ui.label(RichText::new("Link").strong());
            ui.monospace(display_text(device.name.as_deref().unwrap_or("<no name>")));
            ui.monospace(&device.address);
            ui.monospace(format!("MTU {} / {}", device.mtu, device.characteristic));
        }
    }

    fn action_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Read-only actions");
        let can_read = self.can_read();
        if ui
            .add_enabled(can_read, egui::Button::new("Read version"))
            .clicked()
        {
            self.send(Command::ReadVersion, Some("Reading version..."));
        }
        if ui
            .add_enabled(can_read, egui::Button::new("Read configuration"))
            .clicked()
        {
            self.send(Command::ReadConfig, Some("Reading configuration..."));
        }
        if ui
            .add_enabled(can_read, egui::Button::new("Backup config (raw + JSON)"))
            .clicked()
        {
            self.send(
                Command::Backup {
                    dir: self.backup_dir.clone(),
                },
                Some("Reading and writing backup..."),
            );
        }

        let live_label = if self.live {
            "Stop live input"
        } else {
            "Start live input"
        };
        if ui
            .add_enabled(can_read, egui::Button::new(live_label))
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
        if self.connection == ConnectionState::Connected && !self.live {
            ui.weak("Live input is stopped; the 0x0003 keepalive is paused.");
        }

        ui.separator();
        ui.label("Backup folder");
        ui.monospace(self.backup_dir.display().to_string());
        if let Some(report) = &self.backup {
            ui.separator();
            ui.label(RichText::new("Last backup").strong());
            ui.monospace(report.raw_path.display().to_string());
            ui.monospace(report.json_path.display().to_string());
            ui.horizontal(|ui| {
                ui.label("SHA-256");
                if ui.small_button("Copy").clicked() {
                    ui.output_mut(|output| output.copied_text = report.sha256.clone());
                }
            });
            ui.monospace(&report.sha256);
            for warning in &report.warnings {
                ui.colored_label(Color32::from_rgb(230, 190, 90), warning);
            }
        }
    }

    fn central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Device, "Device");
                ui.selectable_value(&mut self.tab, Tab::Input, "Live input");
                ui.selectable_value(&mut self.tab, Tab::Config, "Configuration");
                ui.selectable_value(&mut self.tab, Tab::Log, "Log");
            });
            ui.separator();
            match self.tab {
                Tab::Device => self.device_tab(ui),
                Tab::Input => self.input_tab(ui),
                Tab::Config => self.config_tab(ui),
                Tab::Log => self.log_tab(ui),
            }
        });
    }

    fn device_tab(&self, ui: &mut egui::Ui) {
        ui.heading("Device");
        if self.connection != ConnectionState::Connected {
            ui.colored_label(
                Color32::from_rgb(230, 190, 90),
                "Showing the last session; reconnect to refresh.",
            );
        }
        if let Some(device) = &self.device {
            let name = device.name.clone().unwrap_or_else(|| "<none>".to_string());
            key_value_grid(
                ui,
                "device-info",
                &[
                    ("Name", display_text(&name)),
                    ("Address", device.address.clone()),
                    ("MTU", device.mtu.to_string()),
                    ("Characteristic", device.characteristic.clone()),
                ],
            );
        } else {
            ui.weak("No device connected. Use Scan and Connect in the left panel.");
        }

        ui.add_space(12.0);
        ui.heading("Firmware");
        if let Some(version) = &self.version {
            key_value_grid(
                ui,
                "version-info",
                &[
                    (
                        "Raw payload",
                        if version.raw_hex.is_empty() {
                            "<empty>".to_string()
                        } else {
                            version.raw_hex.clone()
                        },
                    ),
                    (
                        "Text",
                        version.text.clone().unwrap_or_else(|| "<none>".to_string()),
                    ),
                    (
                        "Value",
                        version
                            .value
                            .map_or_else(|| "<not a 4-byte value>".to_string(), format_u32_hex),
                    ),
                ],
            );
        } else {
            ui.weak("No version read yet.");
        }

        ui.add_space(12.0);
        ui.label(
            "Phase 3a reads version and configuration headers only. Macro steps and firmware \
             updates are not part of this build.",
        );
    }

    fn input_tab(&self, ui: &mut egui::Ui) {
        ui.heading("Live input");
        let Some(input) = self.input else {
            ui.weak(
                "No input report yet. Connect (live input starts automatically), or press Start \
                 live input again, and keep the controller in App Mode (Profile long-press).",
            );
            return;
        };

        ui.horizontal(|ui| {
            ui.label(format!("Reports: {:.1}/s", self.reports_per_second));
            if let Some(last) = self.last_input_at {
                ui.label(format!(
                    "Last report: {:.0} ms ago",
                    last.elapsed().as_secs_f64() * 1000.0
                ));
            }
            ui.label(format!("Battery raw: {}", input.battery));
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            stick_widget(ui, "Left stick", input.left_stick);
            ui.add_space(24.0);
            stick_widget(ui, "Right stick", input.right_stick);
        });

        ui.add_space(8.0);
        ui.add(
            egui::ProgressBar::new(f32::from(input.trigger_l) / 255.0)
                .desired_width(280.0)
                .text(format!("L2 / ZL: {} / 255", input.trigger_l)),
        );
        ui.add(
            egui::ProgressBar::new(f32::from(input.trigger_r) / 255.0)
                .desired_width(280.0)
                .text(format!("R2 / ZR: {} / 255", input.trigger_r)),
        );

        ui.separator();
        ui.heading("Buttons");
        ui.horizontal_wrapped(|ui| {
            for (label, flag) in BUTTON_LABELS {
                let active = input.keys.contains(flag);
                let text = RichText::new(format!(" {label} "))
                    .monospace()
                    .strong()
                    .color(if active {
                        Color32::BLACK
                    } else {
                        Color32::from_gray(185)
                    })
                    .background_color(if active {
                        Color32::from_rgb(120, 220, 140)
                    } else {
                        Color32::from_gray(45)
                    });
                ui.label(text);
            }
        });

        ui.separator();
        key_value_grid(
            ui,
            "input-raw",
            &[
                (
                    "Left stick (normalized)",
                    format!("x={} y={}", input.left_stick.0, input.left_stick.1),
                ),
                (
                    "Right stick (normalized)",
                    format!("x={} y={}", input.right_stick.0, input.right_stick.1),
                ),
                (
                    "Key mask",
                    format!(
                        "{} ({})",
                        format_u32_hex(input.keys.bits()),
                        crate::model::keymap_label(input.keys)
                    ),
                ),
            ],
        );
    }

    fn config_tab(&self, ui: &mut egui::Ui) {
        ui.heading("Configuration");
        if self.connection != ConnectionState::Connected {
            ui.colored_label(
                Color32::from_rgb(230, 190, 90),
                "Showing the last session; reconnect to refresh.",
            );
        }
        let Some(config) = &self.config else {
            ui.weak(
                "No configuration snapshot yet. Connect and press Read configuration, or run a \
                 backup.",
            );
            return;
        };

        let crc_note = if config.crc_valid {
            "match"
        } else {
            "mismatch (informational)"
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
                ("Size", format!("{} bytes", config.size)),
                ("SHA-256", config.sha256.clone()),
                ("CRC stored", format!("0x{:04X}", config.stored_crc)),
                (
                    "CRC in place",
                    format!("0x{:04X} ({crc_note})", config.computed_crc),
                ),
                ("Active profile", config.cur_slot_label.clone()),
                (
                    "Gamepad mode",
                    format!(
                        "{} (raw {})",
                        config.gamepad_mode_label, config.gamepad_mode_raw
                    ),
                ),
                ("Profile flags", format!("[{flags}]")),
            ],
        );
        ui.weak(
            "CRC is informational: the official in-place algorithm includes the stored CRC field.",
        );

        ui.add_space(12.0);
        ui.heading("Profiles");
        ui.horizontal_wrapped(|ui| {
            for (index, name) in config.profile_names.iter().enumerate() {
                let shown = if name.is_empty() {
                    "(empty)".to_string()
                } else {
                    display_text(name)
                };
                ui.label(format!(
                    "Profile {}: {shown} ({})",
                    index + 1,
                    format_u32_hex(config.slot_flags[index])
                ));
            }
        });

        ui.add_space(12.0);
        ui.heading("Macro headers");
        ui.weak("Only headers are read in Phase 3a. Macro steps are not downloaded.");
        for slot in &config.macro_slots {
            egui::CollapsingHeader::new(format!(
                "Profile {} (flag {}, macros {})",
                slot.profile,
                format_u32_hex(slot.flag),
                slot.total_cnt
            ))
            .id_salt(("macro-slot", slot.profile))
            .default_open(slot.profile == 1)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt(("macro-scroll", slot.profile))
                    .show(ui, |ui| macro_slot_table(ui, slot));
            });
        }
    }

    fn log_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Log");
            if ui.button("Clear").clicked() {
                self.log.clear();
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.log.is_empty() {
                    ui.weak("No log entries yet.");
                    return;
                }
                for entry in &self.log {
                    let color = if entry.error {
                        Color32::from_rgb(255, 140, 140)
                    } else {
                        ui.visuals().text_color()
                    };
                    ui.horizontal(|ui| {
                        ui.monospace(&entry.at);
                        ui.colored_label(color, &entry.text);
                    });
                }
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        if !self.worker.is_running() && self.connection != ConnectionState::Disconnected {
            self.connection = ConnectionState::Disconnected;
            self.live = false;
            self.input = None;
            self.busy = None;
            self.push_log("The background worker stopped.", true);
        }
        self.top_bar(ctx);
        self.side_panel(ctx);
        self.central_panel(ctx);

        // Checked after drawing so a button pressed in this frame schedules the
        // next frame. Events arrive over a std channel and cannot wake egui.
        let idling =
            self.connection == ConnectionState::Disconnected && self.busy.is_none() && !self.live;
        ctx.request_repaint_after(Duration::from_millis(if idling { 250 } else { 16 }));
    }
}

fn key_value_grid(ui: &mut egui::Ui, id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([16.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            for (key, value) in rows {
                ui.label(*key);
                ui.monospace(value);
                ui.end_row();
            }
        });
}

fn scan_entry_label(entry: &ScanEntry) -> String {
    let name = entry
        .name
        .as_deref()
        .map_or_else(|| "<no name>".to_string(), display_text);
    let kind = if entry.is_app { "App Mode" } else { "other" };
    format!(
        "{name} [{}] ({kind}, {})",
        entry.address,
        format_rssi(entry.rssi)
    )
}

fn macro_slot_table(ui: &mut egui::Ui, slot: &MacroSlotView) {
    egui::Grid::new(("macro-table", slot.profile))
        .num_columns(9)
        .spacing([12.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for heading in [
                "#", "Name", "Mode", "Special", "Steps", "Offset", "Key map", "Cycles", "Interval",
            ] {
                ui.strong(heading);
            }
            ui.end_row();

            for record in &slot.records {
                let name = if record.name.is_empty() {
                    "(empty)".to_string()
                } else {
                    display_text(&record.name)
                };
                ui.monospace(record.number.to_string());
                ui.monospace(name);
                ui.monospace(record.gamepad_mode.to_string());
                ui.monospace(record.special_flag.to_string());
                ui.monospace(record.max_steps.to_string());
                ui.monospace(format_u16_hex(record.offset));
                ui.monospace(&record.key_map);
                ui.monospace(record.cycles.to_string());
                ui.monospace(format!("{} ms", record.interval_ms));
                ui.end_row();
            }
        });
}

fn stick_widget(ui: &mut egui::Ui, label: &str, value: (i16, i16)) {
    ui.vertical(|ui| {
        ui.label(label);
        let side = 150.0;
        let (response, painter) = ui.allocate_painter(egui::vec2(side, side), egui::Sense::hover());
        let rect = response.rect;
        let background = ui.visuals().extreme_bg_color;
        let border = ui.visuals().widgets.noninteractive.bg_stroke;
        let grid = Stroke::new(1.0_f32, border.color.gamma_multiply(0.4));
        let marker_fill = ui.visuals().selection.bg_fill;
        let marker_stroke = ui.visuals().selection.stroke;

        painter.rect_filled(rect, 6.0, background);
        painter.rect_stroke(rect, 6.0, border);
        let center = rect.center();
        painter.line_segment(
            [
                egui::pos2(rect.left() + 8.0, center.y),
                egui::pos2(rect.right() - 8.0, center.y),
            ],
            grid,
        );
        painter.line_segment(
            [
                egui::pos2(center.x, rect.top() + 8.0),
                egui::pos2(center.x, rect.bottom() - 8.0),
            ],
            grid,
        );

        let radius = side / 2.0 - 12.0;
        let point = egui::pos2(
            center.x + f32::from(value.0) / 128.0 * radius,
            center.y - f32::from(value.1) / 128.0 * radius,
        );
        painter.circle_filled(point, 7.0, marker_fill);
        painter.circle_stroke(point, 7.0, marker_stroke);

        ui.monospace(format!("x={} y={}", value.0, value.1));
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

    #[test]
    fn connection_and_read_events_drive_the_busy_state() {
        let mut app = App::build(Worker::spawn(), PathBuf::from("target/test-backups"));
        assert_eq!(app.connection, ConnectionState::Disconnected);

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
        let mut app = App::build(Worker::spawn(), PathBuf::from("target/test-backups"));
        app.handle_event(Event::ScanStarted);
        app.handle_event(Event::Error("scan failed".to_string()));

        assert_eq!(app.error.as_deref(), Some("scan failed"));
        assert!(app.busy.is_none());
        assert!(app.log.iter().any(|entry| entry.error));

        app.handle_event(Event::ScanFinished(Vec::new()));
        assert_eq!(app.connection, ConnectionState::Disconnected);
        assert!(app.scan_entries.is_empty());
    }
}
