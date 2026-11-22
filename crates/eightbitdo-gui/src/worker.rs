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
//! per 16 ms tick and incoming reports are drained through
//! [`Session::try_next_input`]. This poll is also the keepalive: the core crate
//! has no session-owned keepalive yet (the CLI has the reference
//! implementation), so the link stays active while live input is on.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use eightbitdo_core::{
    scan_devices, BleDevice, BleOptions, BleTransport, Error as CoreError, Session, Transport,
    APP_BLE_NAME,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{interval, Interval, MissedTickBehavior};

use crate::backup::BackupReport;
use crate::driver::{DeviceSession, GuiError};
use crate::model::{ConfigView, DeviceInfoView, InputView, ScanEntry, VersionView};

/// Scan duration used by the GUI's Scan button.
const SCAN_TIMEOUT: Duration = Duration::from_secs(6);

/// Scan duration inside `BleTransport::connect`.
const CONNECT_SCAN_TIMEOUT: Duration = Duration::from_secs(8);

/// Connection and service-discovery timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Realtime input poll period (about 60 Hz).
const INPUT_TICK: Duration = Duration::from_millis(16);

/// Per-tick wait for the answer to one `0x0003` poll.
const INPUT_WAIT: Duration = Duration::from_millis(4);

/// Bound for best-effort report disable and link teardown.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);

/// Disconnect after this many consecutive live-input failures.
const MAX_LIVE_FAILURES: u32 = 3;

/// A request from the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Enable `0x0007` and start the `0x0003` poll loop.
    StartLive,
    /// Stop the poll loop and disable `0x0007`.
    StopLive,
    /// Disconnect and end the worker thread.
    Shutdown,
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
    /// A configuration read completed.
    Config(Box<ConfigView>),
    /// One realtime input report.
    Input(InputView),
    /// Live input started or stopped.
    Live(bool),
    /// A backup completed.
    Backup(Box<BackupReport>),
    /// The link is down. `reason` is set when the worker closed it itself.
    Disconnected {
        /// Why the worker disconnected.
        reason: Option<String>,
    },
    /// A non-fatal operation error.
    Error(String),
    /// Informational progress message.
    Log(String),
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
                        let _ = thread_events.send(Event::Error(format!(
                            "Cannot start the BLE worker runtime: {error}"
                        )));
                    }
                }
            });
        if let Err(error) = spawn_result {
            let _ = event_tx.send(Event::Error(format!(
                "Cannot start the BLE worker thread: {error}"
            )));
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

struct Link {
    driver: DeviceSession<BleTransport>,
    ticker: Interval,
    live: bool,
    failures: u32,
}

enum Wake {
    Command(Option<Command>),
    Tick,
}

async fn run_loop(mut commands: UnboundedReceiver<Command>, events: Sender<Event>) {
    let mut link: Option<Link> = None;
    loop {
        let live = link.as_ref().is_some_and(|link| link.live);
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
                            let _ =
                                events.send(Event::Error(format!("Live input failed: {detail}")));
                            let fatal =
                                link_ref.failures >= MAX_LIVE_FAILURES || is_disconnected(&error);
                            if fatal {
                                let reason = format!(
                                    "Live input stopped after {} failure(s): {detail}",
                                    link_ref.failures
                                );
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
                let _ = events.send(Event::Error("Connect to the controller first.".to_string()));
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
            let _ = events.send(Event::Error(
                "Disconnect before scanning again.".to_string(),
            ));
        }
        Command::Connect { .. } => {
            let _ = events.send(Event::Error("Already connected.".to_string()));
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
                    let _ = events.send(Event::Error(format!("Version read failed: {detail}")));
                    if disconnected {
                        teardown(
                            link.take(),
                            Some(format!("Version read lost the link: {detail}")),
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
                    let _ = events.send(Event::Config(Box::new(read.view)));
                }
                Err(error) => {
                    let disconnected = is_disconnected(&error);
                    let detail = error.to_string();
                    let _ =
                        events.send(Event::Error(format!("Configuration read failed: {detail}")));
                    if disconnected {
                        teardown(
                            link.take(),
                            Some(format!("Configuration read lost the link: {detail}")),
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
                    let _ = events.send(Event::Config(Box::new(outcome.view)));
                    let _ = events.send(Event::Backup(Box::new(outcome.report)));
                }
                Err(error) => {
                    let disconnected = is_disconnected(&error);
                    let detail = error.to_string();
                    let _ = events.send(Event::Error(format!("Backup failed: {detail}")));
                    if disconnected {
                        teardown(
                            link.take(),
                            Some(format!("Backup lost the link: {detail}")),
                            events,
                        )
                        .await;
                    }
                }
            }
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
                link.live = false;
            }
            let _ = events.send(Event::Live(false));
            if let Err(error) = result {
                let disconnected = is_disconnected(&error);
                let detail = error.to_string();
                let _ = events.send(Event::Error(format!(
                    "Cannot stop live input cleanly: {detail}"
                )));
                if disconnected {
                    teardown(
                        link.take(),
                        Some(format!("Live input stop lost the link: {detail}")),
                        events,
                    )
                    .await;
                }
            }
        }
    }
    true
}

async fn scan(events: &Sender<Event>) {
    let _ = events.send(Event::ScanStarted);
    match scan_devices(SCAN_TIMEOUT).await {
        Ok(devices) => {
            let entries = sorted_entries(devices);
            let _ = events.send(Event::ScanFinished(entries));
        }
        Err(error) => {
            let _ = events.send(Event::Error(format!("Scan failed: {error}")));
            let _ = events.send(Event::ScanFinished(Vec::new()));
        }
    }
}

async fn connect(address: Option<String>, link: &mut Option<Link>, events: &Sender<Event>) {
    let _ = events.send(Event::Connecting);
    let target = address
        .as_deref()
        .map_or_else(|| APP_BLE_NAME.to_string(), |address| address.to_string());
    let _ = events.send(Event::Log(format!("Connecting to {target}...")));

    let options = BleOptions {
        name: APP_BLE_NAME.to_string(),
        address,
        scan_timeout: CONNECT_SCAN_TIMEOUT,
        connect_timeout: CONNECT_TIMEOUT,
        verbose: false,
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
                live: false,
                failures: 0,
            });
            refresh_after_connect(link, events).await;
            if link.is_some() {
                enable_live(link, events).await;
            }
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = events.send(Event::Error(format!("Connection failed: {detail}")));
            let _ = events.send(Event::Disconnected {
                reason: Some(format!("Connection failed: {detail}")),
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
                link.live = true;
                link.failures = 0;
                link.ticker.reset();
            }
            let _ = events.send(Event::Live(true));
        }
        Err(error) => {
            let disconnected = is_disconnected(&error);
            let detail = error.to_string();
            let _ = events.send(Event::Error(format!("Cannot start live input: {detail}")));
            if disconnected {
                teardown(
                    link.take(),
                    Some(format!("Live input could not start: {detail}")),
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
            let _ = events.send(Event::Error(format!("Version read failed: {detail}")));
            if disconnected {
                teardown(
                    link.take(),
                    Some(format!("Version read lost the link: {detail}")),
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
            let _ = events.send(Event::Config(Box::new(read.view)));
        }
        Err(error) => {
            let disconnected = is_disconnected(&error);
            let detail = error.to_string();
            let _ = events.send(Event::Error(format!("Configuration read failed: {detail}")));
            if disconnected {
                teardown(
                    link.take(),
                    Some(format!("Configuration read lost the link: {detail}")),
                    events,
                )
                .await;
            }
        }
    }
}

async fn teardown(link: Option<Link>, reason: Option<String>, events: &Sender<Event>) {
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
    use super::*;

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
