//! Background BLE worker.
//!
//! The UI thread only sends [`Command`] values and drains [`Event`] values.
//! All `await`s happen on a dedicated thread running a multi-threaded tokio
//! runtime, so a slow scan, a 30-second configuration read, or a wedged BLE
//! write can never block painting.
//!
//! One worker owns at most one [`Session`], preserving the core single-flight
//! contract. Live input starts automatically after the post-connect version
//! and configuration reads. While live input is running, one `0x0003` poll runs
//! per 20 ms tick and incoming reports are drained through
//! [`Session::try_next_input`]. This poll is also the keepalive: the core crate
//! has no session-owned keepalive yet (the CLI has the reference
//! implementation), so the link stays active while live input is on.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use eightbitdo_core::config_ops::ConfigWritePlan;
use eightbitdo_core::macro_ops::{MacroEntry, MacroWritePlan};
use eightbitdo_core::{
    scan_devices, BleDevice, BleOptions, BleTransport, ConfigSnapshot, Error as CoreError, Session,
    Transport, APP_BLE_NAME,
};
use eightbitdo_protocol::config_bt2::{MacroIndex, Slot};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{interval, Interval, MissedTickBehavior};

use crate::backup::{BackupRef, BackupReport};
use crate::driver::{
    ConfigWriteReport, DeviceSession, GuiError, MacroWriteReport, WriteFailure, WritePhase,
};
use crate::i18n::Key;
use crate::model::{ConfigView, DeviceInfoView, InputView, ScanEntry, VersionView};

/// Scan duration used by the GUI's Scan button.
const SCAN_TIMEOUT: Duration = Duration::from_secs(6);

/// Scan duration inside `BleTransport::connect`.
const CONNECT_SCAN_TIMEOUT: Duration = Duration::from_secs(8);

/// Connection and service-discovery timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Realtime input poll period (the official 20 ms keepalive cadence).
const INPUT_TICK: Duration = Duration::from_millis(20);

/// Per-tick wait for the answer to one `0x0003` poll. This matches the CLI's
/// top-level keepalive drain window; late reports remain queued for the next
/// tick.
const INPUT_WAIT: Duration = Duration::from_millis(20);

/// Bound for best-effort report disable and link teardown.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);

/// Disconnect after this many consecutive live-input failures.
const MAX_LIVE_FAILURES: u32 = 3;

/// A request from the UI thread.
#[derive(Debug, Clone)]
pub enum Command {
    /// Scan for BLE devices.
    Scan,
    /// Connect to `80ND`, optionally restricted to one address.
    Connect {
        /// Address selected from a previous scan.
        address: Option<String>,
    },
    /// Disable reports (best effort) and close the link.
    Disconnect,
    /// Read and decode the firmware version.
    ReadVersion,
    /// Read the 2768-byte configuration block.
    ReadConfig,
    /// Read a fresh configuration block and write raw + JSON backups.
    Backup {
        /// Destination directory shown in the UI.
        dir: PathBuf,
    },
    /// Read one macro with its steps.
    ReadMacro {
        /// Profile slot.
        slot: Slot,
        /// Macro index.
        index: MacroIndex,
    },
    /// Write a validated configuration plan after the user confirmed it.
    WriteConfig {
        /// Plan built against the snapshot shown in the confirmation dialog.
        plan: ConfigWritePlan,
        /// Directory for the mandatory pre-write backup.
        backup_dir: PathBuf,
    },
    /// Write one existing non-empty macro after the user confirmed it.
    WriteMacro {
        /// Profile slot.
        slot: Slot,
        /// Macro index.
        index: MacroIndex,
        /// Macro write plan.
        plan: MacroWritePlan,
        /// Directory for the mandatory pre-write macro backup.
        backup_dir: PathBuf,
    },
    /// Enable `0x0007` and start the `0x0003` poll loop.
    StartLive,
    /// Stop the poll loop and disable `0x0007`.
    StopLive,
    /// Disconnect and end the worker thread.
    Shutdown,
}

/// A structured user-facing message.
///
/// The worker only picks a translation [`Key`], its format arguments, and a
/// technical `detail` (English text from `eightbitdo-core`). The UI translates
/// the key with the current language, so switching languages also re-renders
/// messages that arrived earlier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Translation key for the message.
    pub key: Key,
    /// Arguments for `{}` placeholders in the key, in order.
    pub args: Vec<String>,
    /// Technical detail, shown separately and kept untranslated.
    pub detail: Option<String>,
}

impl Notice {
    /// Create a notice for `key` with no arguments or detail.
    #[must_use]
    pub fn new(key: Key) -> Self {
        Self {
            key,
            args: Vec::new(),
            detail: None,
        }
    }

    /// Append one `{}` argument.
    #[must_use]
    pub fn with_arg(mut self, argument: impl Into<String>) -> Self {
        self.args.push(argument.into());
        self
    }

    /// Attach the technical detail from the core crate.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// A notification for the UI thread.
#[derive(Debug)]
pub enum Event {
    /// A scan started.
    ScanStarted,
    /// A scan finished; empty when it failed.
    ScanFinished(Vec<ScanEntry>),
    /// The connection attempt started.
    Connecting,
    /// The link is up; version and configuration reads follow.
    Connected(DeviceInfoView),
    /// A version read completed.
    Version(VersionView),
    /// A configuration read completed, with the snapshot it was decoded from.
    Config {
        /// Raw 2768-byte block and CRC status.
        snapshot: Box<ConfigSnapshot>,
        /// Display summary built on the worker thread.
        view: Box<ConfigView>,
    },
    /// One macro read completed.
    MacroLoaded {
        /// Profile slot.
        slot: Slot,
        /// Macro index.
        index: MacroIndex,
        /// Macro steps and header.
        entry: Box<MacroEntry>,
    },
    /// One realtime input report.
    Input(InputView),
    /// Live input started or stopped.
    Live(bool),
    /// A backup completed.
    Backup(Box<BackupReport>),
    /// The mandatory pre-write backup was written; no flash frame has been
    /// sent yet.
    WriteBackup(BackupRef),
    /// A configuration write completed and its read-back matched.
    ConfigWritten(Box<ConfigWriteReport>),
    /// A macro write completed and its read-back matched.
    MacroWritten(Box<MacroWriteReport>),
    /// A write failed. `backup` inside the failure names the pre-write backup
    /// when one exists.
    WriteFailed(Box<WriteFailure>),
    /// The link is down. `reason` is set when the worker closed it itself.
    Disconnected {
        /// Why the worker disconnected.
        reason: Option<Notice>,
    },
    /// A non-fatal operation error.
    Error(Notice),
    /// Informational progress message.
    Log(Notice),
    /// The worker finished the current command.
    Idle,
}

/// Handle to the background worker.
pub struct Worker {
    commands: UnboundedSender<Command>,
    events: Receiver<Event>,
}

impl Worker {
    /// Start the worker thread.
    ///
    /// A failure to create the thread or its runtime is reported as
    /// [`Event::Error`] on the first [`Worker::try_recv`].
    #[must_use]
    pub fn spawn() -> Self {
        let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, events) = mpsc::channel();
        let thread_events = event_tx.clone();
        let spawn_result = thread::Builder::new()
            .name("eightbitdo-gui-ble".to_string())
            .spawn(move || {
                match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(run_loop(command_rx, thread_events)),
                    Err(error) => {
                        let _ = thread_events.send(Event::Error(
                            Notice::new(Key::ErrWorkerRuntime).with_detail(error.to_string()),
                        ));
                    }
                }
            });
        if let Err(error) = spawn_result {
            let _ = event_tx.send(Event::Error(
                Notice::new(Key::ErrWorkerThread).with_detail(error.to_string()),
            ));
        }
        Self { commands, events }
    }

    /// Send one command. Returns false when the worker has stopped.
    pub fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }

    /// True while the worker thread can still receive commands.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.commands.is_closed()
    }

    /// Take the next event, if any. Never blocks.
    #[must_use]
    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

/// Live-input / keepalive state.
///
/// A flash write pauses the 20 ms `0x0003` poll so the keepalive cannot race
/// the write's ACK stream; it resumes with a fresh ticker after the write.
/// This mirrors the CLI's operation lock without a second task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LiveState {
    live: bool,
    suspended: bool,
}

impl LiveState {
    /// True while the worker should send one `0x0003` poll per tick.
    fn can_poll(self) -> bool {
        self.live && !self.suspended
    }

    /// Reports are enabled and polling may run.
    fn start(&mut self) {
        self.live = true;
        self.suspended = false;
    }

    /// Reports are disabled or the user stopped live input.
    fn stop(&mut self) {
        self.live = false;
        self.suspended = false;
    }

    /// Pause polling for a flash operation. Returns whether it was polling.
    fn begin_write(&mut self) -> bool {
        let was_polling = self.can_poll();
        self.suspended = true;
        was_polling
    }

    /// Resume the state the write paused, without emitting a Live event.
    fn end_write(&mut self) {
        self.suspended = false;
    }
}

struct Link {
    driver: DeviceSession<BleTransport>,
    ticker: Interval,
    live: LiveState,
    failures: u32,
    snapshot: Option<ConfigSnapshot>,
}

enum Wake {
    Command(Option<Command>),
    Tick,
}

async fn run_loop(mut commands: UnboundedReceiver<Command>, events: Sender<Event>) {
    let mut link: Option<Link> = None;
    loop {
        let live = link.as_ref().is_some_and(|link| link.live.can_poll());
        if live {
            let wake = {
                let Some(link) = link.as_mut() else {
                    continue;
                };
                tokio::select! {
                    command = commands.recv() => Wake::Command(command),
                    _ = link.ticker.tick() => Wake::Tick,
                }
            };
            match wake {
                Wake::Command(None) => break,
                Wake::Command(Some(command)) => {
                    if !handle_command(command, &mut link, &events).await {
                        break;
                    }
                    let _ = events.send(Event::Idle);
                }
                Wake::Tick => {
                    let Some(link_ref) = link.as_mut() else {
                        continue;
                    };
                    match link_ref.driver.poll_input(INPUT_WAIT).await {
                        Ok(Some(input)) => {
                            link_ref.failures = 0;
                            let _ = events.send(Event::Input(input));
                        }
                        Ok(None) => link_ref.failures = 0,
                        Err(error) => {
                            link_ref.failures += 1;
                            let detail = error.to_string();
                            let _ = events.send(Event::Error(
                                Notice::new(Key::ErrLiveInputFailed).with_detail(&detail),
                            ));
                            let fatal =
                                link_ref.failures >= MAX_LIVE_FAILURES || is_disconnected(&error);
                            if fatal {
                                let reason = Notice::new(Key::ErrLiveStoppedAfterFailures)
                                    .with_arg(link_ref.failures.to_string())
                                    .with_detail(detail);
                                teardown(link.take(), Some(reason), &events).await;
                            }
                        }
                    }
                }
            }
            continue;
        }

        let Some(command) = commands.recv().await else {
            break;
        };
        if !handle_command(command, &mut link, &events).await {
            break;
        }
        let _ = events.send(Event::Idle);
    }
    teardown(link.take(), None, &events).await;
}

/// Handle one command. Returns false when the worker should exit.
async fn handle_command(command: Command, link: &mut Option<Link>, events: &Sender<Event>) -> bool {
    if matches!(command, Command::Shutdown) {
        teardown(link.take(), None, events).await;
        return false;
    }

    if link.is_none() {
        match command {
            Command::Scan => scan(events).await,
            Command::Connect { address } => connect(address, link, events).await,
            Command::Disconnect => {
                let _ = events.send(Event::Disconnected { reason: None });
            }
            _ => {
                let _ = events.send(Event::Error(Notice::new(Key::ErrConnectFirst)));
            }
        }
        return true;
    }

    match command {
        Command::Shutdown => {
            teardown(link.take(), None, events).await;
            return false;
        }
        Command::Scan => {
            let _ = events.send(Event::Error(Notice::new(Key::ErrDisconnectBeforeScan)));
        }
        Command::Connect { .. } => {
            let _ = events.send(Event::Error(Notice::new(Key::ErrAlreadyConnected)));
        }
        Command::Disconnect => {
            teardown(link.take(), None, events).await;
        }
        Command::ReadVersion => {
            let result = {
                let Some(link) = link.as_mut() else {
                    return true;
                };
                link.driver.read_version().await
            };
            match result {
                Ok(version) => {
                    let _ = events.send(Event::Version(version));
                }
                Err(error) => {
                    let disconnected = is_disconnected(&error);
                    let detail = error.to_string();
                    let _ = events.send(Event::Error(
                        Notice::new(Key::ErrVersionReadFailed).with_detail(&detail),
                    ));
                    if disconnected {
                        teardown(
                            link.take(),
                            Some(Notice::new(Key::ErrVersionLostLink).with_detail(&detail)),
                            events,
                        )
                        .await;
                    }
                }
            }
        }
        Command::ReadConfig => {
            let result = {
                let Some(link) = link.as_mut() else {
                    return true;
                };
                link.driver.read_config().await
            };
            match result {
                Ok(read) => {
                    if let Some(link) = link.as_mut() {
                        link.snapshot = Some(read.snapshot.clone());
                    }
                    let _ = events.send(Event::Config {
                        snapshot: Box::new(read.snapshot),
                        view: Box::new(read.view),
                    });
                }
                Err(error) => {
                    let disconnected = is_disconnected(&error);
                    let detail = error.to_string();
                    let _ = events.send(Event::Error(
                        Notice::new(Key::ErrConfigReadFailed).with_detail(&detail),
                    ));
                    if disconnected {
                        teardown(
                            link.take(),
                            Some(Notice::new(Key::ErrConfigLostLink).with_detail(&detail)),
                            events,
                        )
                        .await;
                    }
                }
            }
        }
        Command::Backup { dir } => {
            let result = {
                let Some(link) = link.as_mut() else {
                    return true;
                };
                link.driver.backup(&dir).await
            };
            match result {
                Ok(outcome) => {
                    if let Some(link) = link.as_mut() {
                        link.snapshot = Some(outcome.snapshot.clone());
                    }
                    let _ = events.send(Event::Config {
                        snapshot: Box::new(outcome.snapshot),
                        view: Box::new(outcome.view),
                    });
                    let _ = events.send(Event::Backup(Box::new(outcome.report)));
                }
                Err(error) => {
                    let disconnected = is_disconnected(&error);
                    let detail = error.to_string();
                    let _ = events.send(Event::Error(
                        Notice::new(Key::ErrBackupFailed).with_detail(&detail),
                    ));
                    if disconnected {
                        teardown(
                            link.take(),
                            Some(Notice::new(Key::ErrBackupLostLink).with_detail(&detail)),
                            events,
                        )
                        .await;
                    }
                }
            }
        }
        Command::ReadMacro { slot, index } => {
            let Some(snapshot) = ensure_snapshot(link, events).await else {
                return true;
            };
            let result = {
                let Some(link) = link.as_mut() else {
                    return true;
                };
                link.driver.read_macro(&snapshot, slot, index).await
            };
            match result {
                Ok(entry) => {
                    let _ = events.send(Event::MacroLoaded {
                        slot,
                        index,
                        entry: Box::new(entry),
                    });
                }
                Err(error) => {
                    let disconnected = is_disconnected(&error);
                    let detail = error.to_string();
                    let _ = events.send(Event::Error(
                        Notice::new(Key::ErrMacroReadFailed).with_detail(&detail),
                    ));
                    if disconnected {
                        teardown(
                            link.take(),
                            Some(Notice::new(Key::ErrMacroReadLostLink).with_detail(&detail)),
                            events,
                        )
                        .await;
                    }
                }
            }
        }
        Command::WriteConfig { plan, backup_dir } => {
            write_config(link, plan, &backup_dir, events).await;
        }
        Command::WriteMacro {
            slot,
            index,
            plan,
            backup_dir,
        } => {
            write_macro(link, slot, index, plan, &backup_dir, events).await;
        }
        Command::StartLive => {
            enable_live(link, events).await;
        }
        Command::StopLive => {
            let result = {
                let Some(link) = link.as_mut() else {
                    return true;
                };
                link.driver.set_reports_enabled(false).await
            };
            if let Some(link) = link.as_mut() {
                link.live.stop();
            }
            let _ = events.send(Event::Live(false));
            if let Err(error) = result {
                let disconnected = is_disconnected(&error);
                let detail = error.to_string();
                let _ = events.send(Event::Error(
                    Notice::new(Key::ErrStopLiveFailed).with_detail(&detail),
                ));
                if disconnected {
                    teardown(
                        link.take(),
                        Some(Notice::new(Key::ErrLiveStopLostLink).with_detail(detail)),
                        events,
                    )
                    .await;
                }
            }
        }
    }
    true
}

/// Return the cached snapshot, reading the configuration first when needed.
///
/// Emits the resulting [`Event::Config`] and tears the link down on a
/// disconnecting read error.
async fn ensure_snapshot(
    link: &mut Option<Link>,
    events: &Sender<Event>,
) -> Option<ConfigSnapshot> {
    if let Some(snapshot) = link.as_ref().and_then(|link| link.snapshot.clone()) {
        return Some(snapshot);
    }
    let link_ref = link.as_mut()?;
    let result = link_ref.driver.read_config().await;
    match result {
        Ok(read) => {
            if let Some(link) = link.as_mut() {
                link.snapshot = Some(read.snapshot.clone());
            }
            let snapshot = read.snapshot.clone();
            let _ = events.send(Event::Config {
                snapshot: Box::new(read.snapshot),
                view: Box::new(read.view),
            });
            Some(snapshot)
        }
        Err(error) => {
            let disconnected = is_disconnected(&error);
            let detail = error.to_string();
            let _ = events.send(Event::Error(
                Notice::new(Key::ErrConfigReadFailed).with_detail(&detail),
            ));
            if disconnected {
                teardown(
                    link.take(),
                    Some(Notice::new(Key::ErrConfigLostLink).with_detail(&detail)),
                    events,
                )
                .await;
            }
            None
        }
    }
}

/// Worker-side configuration write: prepare, back up, commit.
///
/// The pre-write backup event is emitted before any flash frame is sent, so the
/// UI can show its path and SHA-256 while the write is still running.
async fn execute_config_write<T: Transport>(
    driver: &DeviceSession<T>,
    plan: ConfigWritePlan,
    backup_dir: &Path,
    events: &Sender<Event>,
) -> Result<ConfigWriteReport, Box<WriteFailure>> {
    let preparation = driver.prepare_config_write(&plan, backup_dir).await?;
    let _ = events.send(Event::WriteBackup(BackupRef::from(&preparation.backup)));
    let _ = events.send(Event::Log(
        Notice::new(Key::LogPreWriteBackup)
            .with_arg(preparation.backup.raw_path.display().to_string())
            .with_arg(preparation.backup.sha256.clone()),
    ));
    driver.commit_config_write(&preparation).await
}

/// Worker-side macro write: prepare, back up, commit.
async fn execute_macro_write<T: Transport>(
    driver: &DeviceSession<T>,
    slot: Slot,
    index: MacroIndex,
    plan: MacroWritePlan,
    backup_dir: &Path,
    events: &Sender<Event>,
) -> Result<MacroWriteReport, Box<WriteFailure>> {
    let preparation = driver.prepare_macro_write(slot, index, backup_dir).await?;
    let _ = events.send(Event::WriteBackup(BackupRef::from(&preparation.backup)));
    let _ = events.send(Event::Log(
        Notice::new(Key::LogPreWriteMacroBackup)
            .with_arg(preparation.backup.dir.display().to_string())
            .with_arg(preparation.backup.config_sha256.clone()),
    ));
    driver.commit_macro_write(&preparation, &plan).await
}

/// Pause the 20 ms keepalive, run a configuration write, and resume.
async fn write_config(
    link: &mut Option<Link>,
    plan: ConfigWritePlan,
    backup_dir: &Path,
    events: &Sender<Event>,
) {
    let result = {
        let Some(link) = link.as_mut() else {
            return;
        };
        let _was_polling = link.live.begin_write();
        let result = execute_config_write(&link.driver, plan, backup_dir, events).await;
        link.live.end_write();
        link.ticker.reset();
        result
    };
    match result {
        Ok(report) => {
            if let Some(link) = link.as_mut() {
                link.snapshot = Some(report.snapshot.clone());
            }
            let _ = events.send(Event::Config {
                snapshot: Box::new(report.snapshot.clone()),
                view: Box::new(report.view.clone()),
            });
            let _ = events.send(Event::ConfigWritten(Box::new(report)));
        }
        Err(failure) => fail_write(link, failure, events).await,
    }
}

/// Pause the 20 ms keepalive, run a macro write, and resume.
async fn write_macro(
    link: &mut Option<Link>,
    slot: Slot,
    index: MacroIndex,
    plan: MacroWritePlan,
    backup_dir: &Path,
    events: &Sender<Event>,
) {
    let result = {
        let Some(link) = link.as_mut() else {
            return;
        };
        let _was_polling = link.live.begin_write();
        let result = execute_macro_write(&link.driver, slot, index, plan, backup_dir, events).await;
        link.live.end_write();
        link.ticker.reset();
        result
    };
    match result {
        Ok(report) => {
            let _ = events.send(Event::MacroWritten(Box::new(report)));
            // The macro header lives in the 2768-byte configuration. Re-read it
            // so the settings tab shows the committed header values.
            let result = {
                let Some(link) = link.as_mut() else {
                    return;
                };
                link.driver.read_config().await
            };
            match result {
                Ok(read) => {
                    if let Some(link) = link.as_mut() {
                        link.snapshot = Some(read.snapshot.clone());
                    }
                    let _ = events.send(Event::Config {
                        snapshot: Box::new(read.snapshot),
                        view: Box::new(read.view),
                    });
                }
                Err(error) => {
                    let disconnected = is_disconnected(&error);
                    let detail = error.to_string();
                    let _ = events.send(Event::Error(
                        Notice::new(Key::ErrConfigReadFailed).with_detail(&detail),
                    ));
                    if disconnected {
                        teardown(
                            link.take(),
                            Some(Notice::new(Key::ErrConfigLostLink).with_detail(detail)),
                            events,
                        )
                        .await;
                    }
                }
            }
        }
        Err(failure) => fail_write(link, failure, events).await,
    }
}

/// Report a failed write and tear the link down when it disconnected.
async fn fail_write(link: &mut Option<Link>, failure: Box<WriteFailure>, events: &Sender<Event>) {
    let disconnected = failure.disconnected;
    let detail = failure.detail.clone();
    let _ = events.send(Event::Error(write_failure_notice(&failure)));
    let _ = events.send(Event::WriteFailed(failure));
    if disconnected {
        teardown(
            link.take(),
            Some(Notice::new(Key::ErrWriteLostLink).with_detail(detail)),
            events,
        )
        .await;
    }
}

/// Translation key for a write failure phase, with the technical detail kept
/// separate.
fn write_failure_notice(failure: &WriteFailure) -> Notice {
    let key = match failure.phase {
        WritePhase::Read => Key::ErrWriteReadFailed,
        WritePhase::Prepare => Key::ErrWritePrepareFailed,
        WritePhase::Backup => Key::ErrWriteBackupFailed,
        WritePhase::Device => Key::ErrDeviceWriteFailed,
    };
    Notice::new(key).with_detail(failure.detail.clone())
}

async fn scan(events: &Sender<Event>) {
    let _ = events.send(Event::ScanStarted);
    match scan_devices(SCAN_TIMEOUT).await {
        Ok(devices) => {
            let entries = sorted_entries(devices);
            let _ = events.send(Event::ScanFinished(entries));
        }
        Err(error) => {
            let _ = events.send(Event::Error(
                Notice::new(Key::ErrScanFailed).with_detail(error.to_string()),
            ));
            let _ = events.send(Event::ScanFinished(Vec::new()));
        }
    }
}

async fn connect(address: Option<String>, link: &mut Option<Link>, events: &Sender<Event>) {
    let _ = events.send(Event::Connecting);
    let target = address
        .as_deref()
        .map_or_else(|| APP_BLE_NAME.to_string(), |address| address.to_string());
    let _ = events.send(Event::Log(
        Notice::new(Key::LogConnecting).with_arg(&target),
    ));

    let options = BleOptions {
        name: APP_BLE_NAME.to_string(),
        address,
        scan_timeout: CONNECT_SCAN_TIMEOUT,
        connect_timeout: CONNECT_TIMEOUT,
        verbose: false,
        lock_path: None,
    };

    match BleTransport::connect(&options).await {
        Ok(transport) => {
            let info = DeviceInfoView {
                name: transport.name().map(ToString::to_string),
                address: transport.address(),
                mtu: transport.mtu(),
                characteristic: transport.characteristic_uuid().to_string(),
            };
            let _ = events.send(Event::Connected(info.clone()));

            let mut ticker = interval(INPUT_TICK);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            *link = Some(Link {
                driver: DeviceSession::new(Session::new(transport), info),
                ticker,
                live: LiveState::default(),
                failures: 0,
                snapshot: None,
            });
            refresh_after_connect(link, events).await;
            if link.is_some() {
                enable_live(link, events).await;
            }
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = events.send(Event::Error(
                Notice::new(Key::ErrConnectionFailed).with_detail(&detail),
            ));
            let _ = events.send(Event::Disconnected {
                reason: Some(Notice::new(Key::ErrConnectionFailed).with_detail(detail)),
            });
        }
    }
}

/// Enable `0x0007`, start the poll loop, and reset the ticker.
async fn enable_live(link: &mut Option<Link>, events: &Sender<Event>) {
    let result = {
        let Some(link) = link.as_mut() else {
            return;
        };
        link.driver.set_reports_enabled(true).await
    };
    match result {
        Ok(()) => {
            if let Some(link) = link.as_mut() {
                link.live.start();
                link.failures = 0;
                link.ticker.reset();
            }
            let _ = events.send(Event::Live(true));
        }
        Err(error) => {
            let disconnected = is_disconnected(&error);
            let detail = error.to_string();
            let _ = events.send(Event::Error(
                Notice::new(Key::ErrStartLiveFailed).with_detail(&detail),
            ));
            if disconnected {
                teardown(
                    link.take(),
                    Some(Notice::new(Key::ErrLiveStartLostLink).with_detail(detail)),
                    events,
                )
                .await;
            }
        }
    }
}

/// Read version and configuration once after connecting.
async fn refresh_after_connect(link: &mut Option<Link>, events: &Sender<Event>) {
    let version = {
        let Some(link) = link.as_mut() else {
            return;
        };
        link.driver.read_version().await
    };
    match version {
        Ok(version) => {
            let _ = events.send(Event::Version(version));
        }
        Err(error) => {
            let disconnected = is_disconnected(&error);
            let detail = error.to_string();
            let _ = events.send(Event::Error(
                Notice::new(Key::ErrVersionReadFailed).with_detail(&detail),
            ));
            if disconnected {
                teardown(
                    link.take(),
                    Some(Notice::new(Key::ErrVersionLostLink).with_detail(&detail)),
                    events,
                )
                .await;
                return;
            }
        }
    }

    let config = {
        let Some(link) = link.as_mut() else {
            return;
        };
        link.driver.read_config().await
    };
    match config {
        Ok(read) => {
            if let Some(link) = link.as_mut() {
                link.snapshot = Some(read.snapshot.clone());
            }
            let _ = events.send(Event::Config {
                snapshot: Box::new(read.snapshot),
                view: Box::new(read.view),
            });
        }
        Err(error) => {
            let disconnected = is_disconnected(&error);
            let detail = error.to_string();
            let _ = events.send(Event::Error(
                Notice::new(Key::ErrConfigReadFailed).with_detail(&detail),
            ));
            if disconnected {
                teardown(
                    link.take(),
                    Some(Notice::new(Key::ErrConfigLostLink).with_detail(detail)),
                    events,
                )
                .await;
            }
        }
    }
}

async fn teardown(link: Option<Link>, reason: Option<Notice>, events: &Sender<Event>) {
    if let Some(mut link) = link {
        // Best effort: stop notifications even if live input was already
        // stopped or never started. The caller still disconnects the link.
        let _ = tokio::time::timeout(CLEANUP_TIMEOUT, link.driver.set_reports_enabled(false)).await;
        let _ = tokio::time::timeout(
            CLEANUP_TIMEOUT,
            link.driver.session().transport().disconnect(),
        )
        .await;
    }
    let _ = events.send(Event::Disconnected { reason });
}

fn is_disconnected(error: &GuiError) -> bool {
    matches!(error, GuiError::Core(CoreError::Disconnected))
}

fn sorted_entries(devices: Vec<BleDevice>) -> Vec<ScanEntry> {
    let mut entries: Vec<ScanEntry> = devices
        .into_iter()
        .map(|device| ScanEntry {
            name: device.name,
            address: device.address,
            rssi: device.rssi,
            is_app: device.is_app,
        })
        .collect();
    entries.sort_by(|left, right| {
        right
            .is_app
            .cmp(&left.is_app)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.address.cmp(&right.address))
    });
    entries
}

#[cfg(test)]
mod tests {
    use eightbitdo_core::config_ops::{config_write_chunk_len, ConfigEdit, ConfigWritePlan};
    use eightbitdo_core::MockTransport;
    use eightbitdo_protocol::commands;
    use eightbitdo_protocol::config_bt2::{Bt2Config, CONFIG_SIZE};
    use eightbitdo_protocol::CustomHeader;

    use super::*;

    fn push_config_chunks(mock: &MockTransport, bytes: &[u8], chunk_size: usize) {
        for (index, chunk) in bytes.chunks(chunk_size).enumerate() {
            let header = CustomHeader::new(
                commands::cmd::READ_CONFIG,
                0,
                CONFIG_SIZE as u32,
                (index * chunk_size) as u32,
            )
            .with_chunk_len(chunk.len() as u16);
            mock.push_response(&header, chunk);
        }
    }

    fn push_ack(mock: &MockTransport, cmd: u16, subcmd: u16) {
        let header = CustomHeader::new(cmd, subcmd, 0, 0);
        mock.push_response(&header, &[]);
    }

    fn info() -> DeviceInfoView {
        DeviceInfoView {
            name: Some("80ND".to_string()),
            address: "AA:BB:CC:DD:EE:FF".to_string(),
            mtu: 64,
            characteristic: "ff13".to_string(),
        }
    }

    fn device(name: Option<&str>, address: &str, is_app: bool) -> BleDevice {
        BleDevice {
            name: name.map(ToString::to_string),
            address: address.to_string(),
            rssi: None,
            is_app,
        }
    }

    #[test]
    fn app_devices_come_first_then_names_and_addresses() {
        let entries = sorted_entries(vec![
            device(Some("zzz"), "03", false),
            device(Some("Beta"), "02", true),
            device(Some("Alpha"), "01", true),
            device(None, "00", false),
        ]);
        let addresses: Vec<&str> = entries.iter().map(|entry| entry.address.as_str()).collect();
        assert_eq!(addresses, ["01", "02", "00", "03"]);
        assert!(entries[0].is_app);
        assert!(entries[1].is_app);
        assert!(!entries[2].is_app);
    }

    #[test]
    fn gui_error_disconnected_is_detected() {
        assert!(is_disconnected(&GuiError::Core(CoreError::Disconnected)));
        assert!(!is_disconnected(&GuiError::Core(CoreError::Timeout {
            operation: "test".to_string()
        })));
    }

    #[test]
    fn worker_and_events_are_send_when_moved_to_the_worker_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<Worker>();
        assert_send::<Event>();
    }

    #[test]
    fn live_state_pauses_polling_for_a_write_and_resumes_it() {
        let mut live = LiveState::default();
        assert!(!live.can_poll());
        live.start();
        assert!(live.can_poll());
        assert!(
            live.begin_write(),
            "the write must report that polling was active"
        );
        assert!(!live.can_poll(), "no 20 ms poll may run during a write");
        live.end_write();
        assert!(live.can_poll(), "polling resumes after the write");
        live.stop();
        assert!(!live.can_poll());

        let mut idle = LiveState::default();
        assert!(!idle.begin_write(), "a paused link stays paused");
        idle.end_write();
        assert!(!idle.can_poll());
    }

    #[test]
    fn write_failure_phases_map_to_specific_headlines() {
        for (phase, key) in [
            (WritePhase::Read, Key::ErrWriteReadFailed),
            (WritePhase::Prepare, Key::ErrWritePrepareFailed),
            (WritePhase::Backup, Key::ErrWriteBackupFailed),
            (WritePhase::Device, Key::ErrDeviceWriteFailed),
        ] {
            let failure = WriteFailure {
                phase,
                detail: "technical".to_string(),
                backup: None,
                recovery: None,
                disconnected: false,
            };
            let notice = write_failure_notice(&failure);
            assert_eq!(notice.key, key);
            assert_eq!(notice.detail.as_deref(), Some("technical"));
        }
    }

    #[tokio::test]
    async fn the_worker_write_path_emits_the_backup_before_the_result() {
        let mock = MockTransport::new();
        let mut config = Bt2Config::default();
        config.recompute_crc();
        push_config_chunks(&mock, config.as_bytes(), 142);
        let write_chunk = config_write_chunk_len(mock.mtu()).unwrap();
        for _ in 0..CONFIG_SIZE.div_ceil(write_chunk) {
            push_ack(&mock, commands::cmd::RESPONSE, commands::cmd::WRITE_CONFIG);
        }
        push_ack(&mock, commands::cmd::SAVE, commands::subcmd::SAVE_BLE);
        let mut after = config.clone();
        after.set_gamepad_mode(eightbitdo_protocol::config_bt2::GamepadMode::Mac);
        after.recompute_crc();
        push_config_chunks(&mock, after.as_bytes(), 142);

        let driver = DeviceSession::new(Session::new(mock), info());
        let dir = std::env::temp_dir().join(format!(
            "eightbitdo-gui-worker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let plan = ConfigWritePlan::new().with(ConfigEdit::GamepadMode(
            eightbitdo_protocol::config_bt2::GamepadMode::Mac,
        ));
        let (events, receiver) = std::sync::mpsc::channel();
        let report = execute_config_write(&driver, plan, &dir, &events)
            .await
            .expect("mock write succeeds");

        let events: Vec<Event> = receiver.try_iter().collect();
        assert!(
            matches!(events.first(), Some(Event::WriteBackup(_))),
            "the pre-write backup must be visible before the result"
        );
        assert!(
            events.iter().any(
                |event| matches!(event, Event::Log(notice) if notice.key == Key::LogPreWriteBackup)
            ),
            "the backup path and SHA-256 are logged before the flash commit"
        );
        assert_eq!(report.view.gamepad_mode_raw, 2);
        assert!(report.backup.raw_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worker_runtime_starts_and_handles_shutdown() {
        let worker = Worker::spawn();
        assert!(worker.send(Command::Shutdown));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while worker.is_running() {
            assert!(
                std::time::Instant::now() < deadline,
                "the worker did not stop within 5 seconds"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
