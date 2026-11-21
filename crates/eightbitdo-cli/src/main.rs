//! Read-only command-line interface for the 8BitDo Ultimate 2 Bluetooth
//! controller.
//!
//! The binary exposes `scan`, `info`, `config dump`, `backup`, `monitor`,
//! `keepalive` and `session`. No configuration write, macro flash, save or OTA
//! command is reachable from this binary.

use std::future::Future;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use eightbitdo_core::{
    scan_devices, BleOptions, BleTransport, ConfigSnapshot, Error, Session, Transport, VersionInfo,
};
use eightbitdo_protocol::commands;
use eightbitdo_protocol::config_bt2::{Slot, CONFIG_SIZE};
use eightbitdo_protocol::header;
use eightbitdo_protocol::input_report::InputReport;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::AsyncBufReadExt;
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Parser)]
#[command(
    name = "eightbitdo",
    version,
    about = "Read-only tools for the 8BitDo Ultimate 2 Bluetooth controller"
)]
struct Cli {
    /// Advertised BLE name of the App Mode peripheral.
    #[arg(long, global = true, default_value = "80ND", value_name = "NAME")]
    name: String,

    /// Bluetooth address to match in addition to the name.
    #[arg(long, global = true, value_name = "ADDRESS")]
    address: Option<String>,

    /// BLE scan duration in seconds.
    #[arg(long, global = true, default_value_t = 10, value_name = "SECONDS")]
    scan_timeout: u64,

    /// Connection and service-discovery timeout in seconds.
    #[arg(long, global = true, default_value_t = 20, value_name = "SECONDS")]
    timeout: u64,

    /// Print every raw BLE TX/RX frame to stderr.
    #[arg(long, short = 'v', global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List nearby BLE devices and mark App Mode candidates.
    Scan,

    /// Connect and read the firmware version.
    Info,

    /// Configuration operations (read-only).
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Read version and configuration in one connection, optionally holding it.
    Backup {
        /// Write the raw 2768 bytes to this file.
        #[arg(long, value_name = "FILE")]
        raw: Option<PathBuf>,

        /// Write the JSON summary to this file.
        #[arg(long, value_name = "FILE")]
        json: Option<PathBuf>,

        /// Keep the connection alive with 20 ms input polling for this many seconds.
        #[arg(long, default_value_t = 0, value_name = "SECONDS")]
        hold: u64,
    },

    /// Stream realtime input reports.
    Monitor {
        /// Exit after this many reports instead of waiting for Ctrl-C.
        #[arg(long, value_name = "N")]
        count: Option<u64>,
    },

    /// Keep the BLE configuration mode alive with the official input polling.
    Keepalive {
        /// Input poll interval in milliseconds.
        #[arg(long, default_value_t = 20, value_name = "MILLISECONDS")]
        interval_ms: u64,

        /// Stop after this many seconds.
        #[arg(long, default_value_t = 120, value_name = "SECONDS")]
        duration: u64,
    },

    /// Keep one connection open and run read-only commands interactively.
    Session {
        /// Input poll interval in milliseconds.
        #[arg(long, default_value_t = 20, value_name = "MILLISECONDS")]
        interval_ms: u64,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Read the 2768-byte configuration block and print a summary.
    Dump {
        /// Write the raw 2768 bytes to this file.
        #[arg(long, value_name = "FILE")]
        raw: Option<PathBuf>,

        /// Write a JSON summary to this file.
        #[arg(long, value_name = "FILE")]
        json: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode, Error> {
    let options = BleOptions {
        name: cli.name.clone(),
        address: cli.address.clone(),
        scan_timeout: Duration::from_secs(cli.scan_timeout),
        connect_timeout: Duration::from_secs(cli.timeout),
        verbose: cli.verbose,
    };

    match cli.command {
        Command::Scan => scan(Duration::from_secs(cli.scan_timeout)).await?,
        Command::Info => info(&options).await?,
        Command::Config { command } => match command {
            ConfigCommand::Dump { raw, json } => {
                dump_config(&options, raw.as_deref(), json.as_deref()).await?;
            }
        },
        Command::Backup { raw, json, hold } => {
            backup(
                &options,
                raw.as_deref(),
                json.as_deref(),
                Duration::from_secs(hold),
            )
            .await?;
        }
        Command::Monitor { count } => monitor(&options, count).await?,
        Command::Keepalive {
            interval_ms,
            duration,
        } => {
            return keepalive(
                &options,
                Duration::from_millis(interval_ms),
                Duration::from_secs(duration),
            )
            .await;
        }
        Command::Session { interval_ms } => {
            return session(&options, Duration::from_millis(interval_ms)).await;
        }
    }

    Ok(ExitCode::SUCCESS)
}

async fn scan(timeout: Duration) -> Result<(), Error> {
    let mut devices = scan_devices(timeout).await?;
    if devices.is_empty() {
        println!(
            "no BLE devices found within {} seconds",
            timeout.as_secs_f64()
        );
        return Ok(());
    }

    devices.sort_by_key(|device| std::cmp::Reverse(device.rssi));
    println!("{:<20} {:<20} {:>5}  MODE", "NAME", "ADDRESS", "RSSI");
    for device in &devices {
        let name = device.name.as_deref().unwrap_or("<unknown>");
        let rssi = device
            .rssi
            .map_or_else(|| "-".to_string(), |value| value.to_string());
        let mode = if device.is_app { "App Mode" } else { "" };
        println!(
            "{name:<20} {address:<20} {rssi:>5}  {mode}",
            address = device.address
        );
    }
    Ok(())
}

async fn info(options: &BleOptions) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let address = transport.address();
    let name = transport
        .name()
        .map_or_else(|| "<unknown>".to_string(), ToString::to_string);
    let mtu = transport.mtu();
    let characteristic = transport.characteristic_uuid();

    println!("address: {address}");
    println!("name: {name}");
    println!("mtu: {mtu}");
    println!("characteristic: {characteristic}");

    let session = Session::new(transport);
    let version = session.read_version().await;
    let _ = session.transport().disconnect().await;
    let version = version?;
    print_version(&version);
    Ok(())
}

fn print_version(version: &VersionInfo) {
    let raw = if version.raw.is_empty() {
        "<empty>".to_string()
    } else {
        hex::encode(&version.raw)
    };
    let text = if version.text.is_empty() {
        "<none>"
    } else {
        &version.text
    };

    println!("version raw: {raw}");
    println!("version text: {text}");
    if let Ok(bytes) = <[u8; 4]>::try_from(version.raw.as_slice()) {
        // The official getVersion (0x35700) reads the 4-byte payload as a
        // little-endian i32.
        println!("version value: 0x{:08X}", u32::from_le_bytes(bytes));
    }
}

async fn dump_config(
    options: &BleOptions,
    raw_path: Option<&Path>,
    json_path: Option<&Path>,
) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let snapshot = session.read_config_snapshot().await;
    let _ = session.transport().disconnect().await;
    let snapshot = snapshot?;

    let sha256 = write_config_outputs(&snapshot, raw_path, json_path)?;
    print_config_summary(&snapshot, &sha256);
    Ok(())
}

/// Write the optional raw/JSON files and return the SHA-256 hex digest.
fn write_config_outputs(
    snapshot: &ConfigSnapshot,
    raw_path: Option<&Path>,
    json_path: Option<&Path>,
) -> Result<String, Error> {
    let sha256 = hex::encode(Sha256::digest(snapshot.config.as_bytes()));

    if let Some(path) = raw_path {
        std::fs::write(path, snapshot.config.as_bytes())?;
        println!(
            "wrote raw configuration: {} ({CONFIG_SIZE} bytes)",
            path.display()
        );
    }

    if let Some(path) = json_path {
        let summary = json_summary(snapshot, &sha256);
        let encoded = serde_json::to_string_pretty(&summary).map_err(std::io::Error::other)?;
        std::fs::write(path, format!("{encoded}\n"))?;
        println!("wrote JSON summary: {}", path.display());
    }

    Ok(sha256)
}

fn json_summary(snapshot: &ConfigSnapshot, sha256: &str) -> serde_json::Value {
    let profile_names: Vec<String> = Slot::ALL
        .iter()
        .map(|slot| snapshot.config.profile_name(*slot))
        .collect();

    // The official ANSI_CRC_16_sh is recomputed in place over a buffer that
    // still contains the stored CRC field. A device-read buffer therefore
    // does not have to satisfy `crc_in_place == crc_stored`; both values are
    // informational.
    json!({
        "size": CONFIG_SIZE,
        "sha256": sha256,
        "cur_slot": snapshot.config.cur_slot_raw(),
        "gamepad_mode": snapshot.config.gamepad_mode_raw(),
        "slot_flags": snapshot.config.slot_flags(),
        "profile_names": profile_names,
        "crc_stored": snapshot.stored_crc,
        "crc_in_place": snapshot.computed_crc,
    })
}

fn cur_slot_label(raw: u16) -> String {
    match Slot::from_index(raw as usize) {
        Some(slot) => format!("{raw} (profile {})", slot.number()),
        None if raw == 4 => format!("{raw} (none)"),
        None => format!("unknown ({raw})"),
    }
}

fn format_slot_flags(flags: [u32; 3]) -> String {
    let values: Vec<String> = flags.iter().map(|flag| format!("0x{flag:08X}")).collect();
    format!("[{}]", values.join(", "))
}

fn print_config_summary(snapshot: &ConfigSnapshot, sha256: &str) {
    let mode = snapshot
        .config
        .gamepad_mode()
        .map_or_else(|| "unknown".to_string(), |mode| format!("{mode:?}"));
    let cur_slot = cur_slot_label(snapshot.config.cur_slot_raw());

    println!("config size: {CONFIG_SIZE} bytes");
    println!("sha256: {sha256}");
    println!("cur_slot: {cur_slot}");
    println!(
        "gamepad_mode: {} ({mode})",
        snapshot.config.gamepad_mode_raw()
    );
    println!(
        "slot_flags: {}",
        format_slot_flags(snapshot.config.slot_flags())
    );
    println!("  note: non-zero may mean enabled; 1/0 is a simplification (unconfirmed)");
    println!(
        "profile_names: {:?}",
        Slot::ALL
            .iter()
            .map(|slot| snapshot.config.profile_name(*slot))
            .collect::<Vec<_>>()
    );

    // Informational only: the official in-place recomputation includes the
    // stored CRC field, so a mismatch is not by itself a corruption signal.
    println!(
        "crc: stored=0x{:04X} in_place=0x{:04X} (informational)",
        snapshot.stored_crc, snapshot.computed_crc
    );
}

async fn backup(
    options: &BleOptions,
    raw_path: Option<&Path>,
    json_path: Option<&Path>,
    hold: Duration,
) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let result = backup_session(&session, raw_path, json_path, hold, shutdown).await;
    let _ = session.transport().disconnect().await;
    result
}

/// One-connection backup: ReadVersion, the 2768-byte config, optional hold.
///
/// The connection stays open for the whole sequence. If `hold` is non-zero,
/// the official-style keepalive (`0x0007` enable, 20 ms `0x0003`, receive
/// drain, `0x0007` disable) keeps it alive before disconnecting.
async fn backup_session<T: Transport>(
    session: &Session<T>,
    raw_path: Option<&Path>,
    json_path: Option<&Path>,
    hold: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<(), Error> {
    let version = session.read_version().await?;
    print_version(&version);

    let snapshot = session.read_config_snapshot().await?;
    let sha256 = write_config_outputs(&snapshot, raw_path, json_path)?;
    print_config_summary(&snapshot, &sha256);

    if !hold.is_zero() {
        let stats = run_keepalive(session, Duration::from_millis(20), hold, shutdown).await?;
        println!(
            "hold finished: sent={} received={}",
            stats.sent, stats.received
        );
    }
    Ok(())
}

async fn monitor(options: &BleOptions, count: Option<u64>) -> Result<(), Error> {
    if count == Some(0) {
        return Ok(());
    }

    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    if let Err(error) = session.set_report_enable(true).await {
        let _ = session.transport().disconnect().await;
        return Err(error);
    }

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    let mut printed: u64 = 0;
    let mut outcome = Ok(());
    loop {
        if count.is_some_and(|limit| printed >= limit) {
            break;
        }
        tokio::select! {
            signal = &mut ctrl_c => {
                if let Err(error) = signal {
                    outcome = Err(Error::Io(error));
                }
                break;
            }
            report = session.try_next_input(Duration::from_secs(5)) => {
                match report {
                    Ok(Some(report)) => {
                        print_report(&report);
                        printed += 1;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        outcome = Err(error);
                        break;
                    }
                }
            }
        }
    }

    let _ = session.set_report_enable(false).await;
    let _ = session.transport().disconnect().await;
    outcome
}

async fn keepalive(
    options: &BleOptions,
    interval: Duration,
    duration: Duration,
) -> Result<ExitCode, Error> {
    if interval.is_zero() {
        eprintln!("keepalive error after 0.0s: --interval-ms must be greater than zero");
        return Ok(ExitCode::FAILURE);
    }

    let started = Instant::now();
    let transport = match BleTransport::connect(options).await {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("keepalive error after 0.0s: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };
    let session = Session::new(transport);

    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let result = run_keepalive(&session, interval, duration, shutdown).await;
    let _ = session.transport().disconnect().await;

    let elapsed = started.elapsed().as_secs_f64();
    match result {
        Ok(stats) => {
            println!(
                "keepalive finished: elapsed={elapsed:.1}s sent={} received={}",
                stats.sent, stats.received
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("keepalive error after {elapsed:.1}s: {error}");
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Counters for one keepalive run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct KeepaliveStats {
    /// `ReadKeysAndJoy` (`0x0003`) requests written.
    sent: u64,
    /// `0x0003` notifications drained from the transport.
    received: u64,
}

/// Enable notifications and run the official foreground polling loop.
///
/// This wrapper is deliberately testable without a CLI or Ctrl-C: the
/// `shutdown` future ends the loop early. A future GUI or long-lived session
/// should move this idle keepalive into `eightbitdo-core::Session` instead of
/// duplicating it in each front end.
async fn run_keepalive<T: Transport>(
    session: &Session<T>,
    interval: Duration,
    duration: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<KeepaliveStats, Error> {
    session.set_report_enable(true).await?;
    let result = keepalive_loop(session, interval, duration, shutdown).await;
    // Best effort: stop notifications even if the loop failed or Ctrl-C
    // interrupted it. The caller still disconnects the link.
    let _ = session.set_report_enable(false).await;
    result
}

/// Poll `0x0003` at `interval` and drain every incoming notification.
///
/// The official Android app sends `ReadKeysAndJoy` every 20 ms in the
/// foreground, with no response timeout or retry. Notifications are consumed
/// here so the 64-slot BLE queue cannot fill up.
async fn keepalive_loop<T: Transport>(
    session: &Session<T>,
    interval: Duration,
    duration: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<KeepaliveStats, Error> {
    let mut input_ticker = tokio::time::interval(interval);
    input_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut report_ticker = tokio::time::interval(Duration::from_secs(1));
    report_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The first interval tick is immediate; skip it so the first summary is
    // printed after one second.
    report_ticker.tick().await;

    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);
    tokio::pin!(shutdown);

    let started = Instant::now();
    let mut stats = KeepaliveStats::default();
    loop {
        tokio::select! {
            _ = input_ticker.tick() => {
                session.request_input().await?;
                stats.sent += 1;
            }
            frame = session.transport().recv() => {
                let frame = frame?;
                if is_input_notification(&frame)? {
                    stats.received += 1;
                }
            }
            _ = report_ticker.tick() => {
                println!(
                    "keepalive: sent={} received={} elapsed={:.1}s",
                    stats.sent,
                    stats.received,
                    started.elapsed().as_secs_f64()
                );
            }
            _ = &mut deadline => break,
            _ = &mut shutdown => break,
        }
    }
    Ok(stats)
}

fn is_input_notification(frame: &[u8]) -> Result<bool, Error> {
    let (header, _) = header::decode_app_response(frame)?;
    let is_input = header.cmd == commands::cmd::INPUT
        || (header.cmd == commands::cmd::RESPONSE && header.subcmd == commands::cmd::INPUT);
    Ok(is_input && header.chunk_len > 0)
}

/// Shared counters and shutdown signalling for the session keepalive task.
/// Shared counters and failure state for the session keepalive task.
struct SessionKeepaliveState {
    sent: AtomicU64,
    received: AtomicU64,
    consecutive_failures: AtomicU32,
    degraded: AtomicBool,
    last_error: StdMutex<Option<String>>,
    started_at: Instant,
    stopped_notify: Notify,
}

impl SessionKeepaliveState {
    fn new() -> Self {
        Self {
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            consecutive_failures: AtomicU32::new(0),
            degraded: AtomicBool::new(false),
            last_error: StdMutex::new(None),
            started_at: Instant::now(),
            stopped_notify: Notify::new(),
        }
    }

    fn record_error(&self, error: &Error) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
    }

    fn last_error(&self) -> Option<String> {
        self.last_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[derive(Debug, Clone, Copy)]
struct SessionKeepaliveConfig {
    interval: Duration,
    max_consecutive_failures: u32,
}

impl SessionKeepaliveConfig {
    fn with_interval(interval: Duration) -> Self {
        Self {
            interval,
            max_consecutive_failures: 3,
        }
    }
}

struct SessionKeepalive {
    state: Arc<SessionKeepaliveState>,
    token: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for SessionKeepalive {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SessionCommand {
    Info,
    ConfigDump {
        raw: Option<PathBuf>,
        json: Option<PathBuf>,
    },
    Backup {
        raw: Option<PathBuf>,
        json: Option<PathBuf>,
    },
    Monitor {
        count: Option<u64>,
    },
    MonitorStop,
    Status,
    Help,
    Quit,
    Empty,
    Unknown(String),
}

#[derive(Debug, PartialEq, Eq)]
enum SessionOutcome {
    Continue,
    Quit,
    MonitorReports(usize),
}

fn parse_file_options<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<(Option<PathBuf>, Option<PathBuf>)> {
    let mut raw = None;
    let mut json = None;
    while let Some(part) = parts.next() {
        match part {
            "--raw" => raw = Some(PathBuf::from(parts.next()?)),
            "--json" => json = Some(PathBuf::from(parts.next()?)),
            _ => return None,
        }
    }
    Some((raw, json))
}

fn parse_session_command(line: &str) -> SessionCommand {
    let mut parts = line.split_whitespace();
    let Some(head) = parts.next() else {
        return SessionCommand::Empty;
    };
    match head {
        "info" if parts.next().is_none() => SessionCommand::Info,
        "config" => {
            if parts.next() != Some("dump") {
                return SessionCommand::Unknown(line.to_string());
            }
            match parse_file_options(parts) {
                Some((raw, json)) => SessionCommand::ConfigDump { raw, json },
                None => SessionCommand::Unknown(line.to_string()),
            }
        }
        "backup" => match parse_file_options(parts) {
            Some((raw, json)) => SessionCommand::Backup { raw, json },
            None => SessionCommand::Unknown(line.to_string()),
        },
        "monitor" => match parts.next() {
            None => SessionCommand::Monitor { count: None },
            Some("stop") if parts.next().is_none() => SessionCommand::MonitorStop,
            Some("--count") => match parts.next().and_then(|value| value.parse().ok()) {
                Some(count) if parts.next().is_none() => {
                    SessionCommand::Monitor { count: Some(count) }
                }
                _ => SessionCommand::Unknown(line.to_string()),
            },
            _ => SessionCommand::Unknown(line.to_string()),
        },
        "status" if parts.next().is_none() => SessionCommand::Status,
        "help" if parts.next().is_none() => SessionCommand::Help,
        "quit" | "exit" if parts.next().is_none() => SessionCommand::Quit,
        _ => SessionCommand::Unknown(line.to_string()),
    }
}

fn print_session_help() {
    println!("commands:");
    println!("  info");
    println!("  config dump [--raw FILE] [--json FILE]");
    println!("  backup [--raw FILE] [--json FILE]");
    println!("  monitor [--count N]");
    println!("  status");
    println!("  help");
    println!("  quit | exit");
    println!("monitor without --count runs until Ctrl-C; Ctrl-C exits the session.");
}

/// Start the official-style keepalive on a shared session.
///
/// `session` is the single operation lock: keepalive ticks and REPL commands
/// all take this mutex, so a command never races a keepalive drain for a
/// response.
async fn start_session_keepalive<T: Transport + 'static>(
    session: Arc<AsyncMutex<Session<T>>>,
    interval: Duration,
) -> Result<SessionKeepalive, Error> {
    start_session_keepalive_with_config(session, SessionKeepaliveConfig::with_interval(interval))
        .await
}

async fn start_session_keepalive_with_config<T: Transport + 'static>(
    session: Arc<AsyncMutex<Session<T>>>,
    config: SessionKeepaliveConfig,
) -> Result<SessionKeepalive, Error> {
    if config.interval.is_zero() {
        return Err(Error::Transport(
            "session interval must be greater than zero".to_string(),
        ));
    }
    {
        let guard = session.lock().await;
        guard.set_report_enable(true).await?;
    }
    let state = Arc::new(SessionKeepaliveState::new());
    let task_state = Arc::clone(&state);
    let token = CancellationToken::new();
    let task_token = token.clone();
    let task = tokio::spawn(session_keepalive_task(
        session, config, task_state, task_token,
    ));
    Ok(SessionKeepalive {
        state,
        token,
        task: Some(task),
    })
}

async fn session_keepalive_task<T: Transport>(
    session: Arc<AsyncMutex<Session<T>>>,
    config: SessionKeepaliveConfig,
    state: Arc<SessionKeepaliveState>,
    token: CancellationToken,
) {
    let mut ticker = tokio::time::interval(config.interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = ticker.tick() => {}
        }
        if token.is_cancelled() {
            break;
        }

        let guard = session.lock().await;
        if token.is_cancelled() {
            break;
        }

        let result = async {
            guard.request_input().await?;
            state.sent.fetch_add(1, Ordering::SeqCst);
            let mut drained = 0u64;
            loop {
                match guard.try_next_input(Duration::from_millis(1)).await {
                    Ok(Some(_)) => drained += 1,
                    Ok(None) => break,
                    Err(error) => return Err(error),
                }
            }
            Ok::<u64, Error>(drained)
        }
        .await;

        match result {
            Ok(drained) => {
                state.received.fetch_add(drained, Ordering::SeqCst);
                state.consecutive_failures.store(0, Ordering::SeqCst);
            }
            Err(error) => {
                state.record_error(&error);
                let failures = state.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
                if failures >= config.max_consecutive_failures
                    || matches!(error, Error::Disconnected)
                {
                    state.degraded.store(true, Ordering::SeqCst);
                    token.cancel();
                    state.stopped_notify.notify_one();
                    break;
                }
            }
        }
    }
}

impl SessionKeepalive {
    async fn stop(mut self) {
        self.token.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

async fn shutdown_session<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: SessionKeepalive,
) {
    keepalive.stop().await;
    let guard = session.lock().await;
    let _ = guard.set_report_enable(false).await;
    let _ = guard.transport().disconnect().await;
}

#[derive(Debug, PartialEq, Eq)]
enum MonitorPollOutcome {
    CountReached(usize),
    Stopped,
    Quit,
}

async fn monitor_poll<T, F>(
    session: &Session<T>,
    interval: Duration,
    count: Option<u64>,
    stop: &Notify,
    ctrl_c: &mut Pin<&mut F>,
) -> Result<MonitorPollOutcome, Error>
where
    T: Transport,
    F: Future<Output = ()>,
{
    let mut printed = 0usize;
    let mut next_send = tokio::time::Instant::now();

    loop {
        if count.is_some_and(|limit| (printed as u64) >= limit) {
            return Ok(MonitorPollOutcome::CountReached(printed));
        }

        tokio::select! {
            _ = stop.notified() => return Ok(MonitorPollOutcome::Stopped),
            _ = ctrl_c.as_mut() => return Ok(MonitorPollOutcome::Quit),
            _ = tokio::time::sleep_until(next_send) => {}
        }

        tokio::select! {
            _ = stop.notified() => return Ok(MonitorPollOutcome::Stopped),
            _ = ctrl_c.as_mut() => return Ok(MonitorPollOutcome::Quit),
            result = session.request_input() => result?,
        }
        next_send = tokio::time::Instant::now() + interval;

        let wait = next_send
            .saturating_duration_since(tokio::time::Instant::now())
            .max(Duration::from_millis(1));
        let report = tokio::select! {
            _ = stop.notified() => return Ok(MonitorPollOutcome::Stopped),
            _ = ctrl_c.as_mut() => return Ok(MonitorPollOutcome::Quit),
            report = session.try_next_input(wait) => report?,
        };
        if let Some(report) = report {
            print_report(&report);
            printed += 1;
        }
    }
}

struct SessionMonitor {
    stop: Arc<Notify>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for SessionMonitor {
    fn drop(&mut self) {
        self.stop.notify_one();
    }
}

impl SessionMonitor {
    async fn stop(mut self) {
        self.stop.notify_one();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

fn start_background_monitor<T: Transport + 'static>(
    session: Arc<AsyncMutex<Session<T>>>,
    interval: Duration,
) -> SessionMonitor {
    let stop = Arc::new(Notify::new());
    let task_stop = Arc::clone(&stop);
    let task = tokio::spawn(async move {
        let guard = session.lock().await;
        let never = std::future::pending::<()>();
        tokio::pin!(never);
        if let Err(error) = monitor_poll(&guard, interval, None, &task_stop, &mut never).await {
            eprintln!("monitor error: {error}");
        }
    });
    SessionMonitor {
        stop,
        task: Some(task),
    }
}

async fn execute_session_command<T: Transport + 'static>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: &Arc<SessionKeepaliveState>,
    started: Instant,
    mtu: u16,
    interval: Duration,
    monitor: &mut Option<SessionMonitor>,
    command: SessionCommand,
) -> Result<SessionOutcome, Error> {
    match command {
        SessionCommand::Info => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            let guard = session.lock().await;
            let version = guard.read_version().await?;
            print_version(&version);
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::ConfigDump { raw, json } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            let guard = session.lock().await;
            let snapshot = guard.read_config_snapshot().await?;
            let sha256 = write_config_outputs(&snapshot, raw.as_deref(), json.as_deref())?;
            print_config_summary(&snapshot, &sha256);
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Backup { raw, json } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            let guard = session.lock().await;
            backup_session(
                &guard,
                raw.as_deref(),
                json.as_deref(),
                Duration::ZERO,
                std::future::pending::<()>(),
            )
            .await?;
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Monitor { count: Some(count) } => {
            if monitor.is_some() {
                eprintln!("monitor is already running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            let guard = session.lock().await;
            let stop = Notify::new();
            let ctrl_c = async {
                let _ = tokio::signal::ctrl_c().await;
            };
            tokio::pin!(ctrl_c);
            let outcome = monitor_poll(&guard, interval, Some(count), &stop, &mut ctrl_c).await?;
            Ok(match outcome {
                MonitorPollOutcome::CountReached(printed) => {
                    SessionOutcome::MonitorReports(printed)
                }
                MonitorPollOutcome::Quit => SessionOutcome::Quit,
                MonitorPollOutcome::Stopped => SessionOutcome::Continue,
            })
        }
        SessionCommand::Monitor { count: None } => {
            if monitor.is_some() {
                eprintln!("monitor is already running");
            } else {
                *monitor = Some(start_background_monitor(Arc::clone(session), interval));
                println!("monitor started; use 'monitor stop' or Ctrl-C");
            }
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::MonitorStop => {
            if let Some(running) = monitor.take() {
                running.stop().await;
                println!("monitor stopped");
            } else {
                println!("no monitor is running");
            }
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Status => {
            println!("connected: true");
            println!("mtu: {mtu}");
            println!("keepalive sent: {}", keepalive.sent.load(Ordering::SeqCst));
            println!(
                "keepalive received: {}",
                keepalive.received.load(Ordering::SeqCst)
            );
            println!(
                "keepalive degraded: {}",
                keepalive.degraded.load(Ordering::SeqCst)
            );
            if let Some(error) = keepalive.last_error() {
                println!("keepalive last_error: {error}");
            }
            println!(
                "keepalive elapsed: {:.1}s",
                keepalive.started_at.elapsed().as_secs_f64()
            );
            println!("session elapsed: {:.1}s", started.elapsed().as_secs_f64());
            println!(
                "monitor: {}",
                if monitor.is_some() { "running" } else { "idle" }
            );
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Help => {
            print_session_help();
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Quit => Ok(SessionOutcome::Quit),
        SessionCommand::Empty => Ok(SessionOutcome::Continue),
        SessionCommand::Unknown(line) => {
            eprintln!("unknown command: {line}; type 'help'");
            Ok(SessionOutcome::Continue)
        }
    }
}

async fn session_repl_loop<T: Transport + 'static>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: &Arc<SessionKeepaliveState>,
    started: Instant,
    mtu: u16,
    interval: Duration,
) -> Result<(), Error> {
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut line = String::new();
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    let interactive = std::io::stdout().is_terminal();
    let mut monitor: Option<SessionMonitor> = None;
    let mut result = Ok(());

    loop {
        if interactive {
            print!("8bitdo> ");
            let _ = std::io::stdout().flush();
        }
        let read = tokio::select! {
            _ = &mut ctrl_c => break,
            _ = keepalive.stopped_notify.notified() => {
                eprintln!(
                    "keepalive stopped (degraded={}): {}",
                    keepalive.degraded.load(Ordering::SeqCst),
                    keepalive.last_error().unwrap_or_else(|| "unknown error".to_string())
                );
                break;
            }
            result = reader.read_line(&mut line) => result?,
        };
        if read == 0 {
            break;
        }
        let command = parse_session_command(line.trim());
        line.clear();
        match execute_session_command(
            session,
            keepalive,
            started,
            mtu,
            interval,
            &mut monitor,
            command,
        )
        .await
        {
            Ok(SessionOutcome::Quit) => break,
            Ok(SessionOutcome::Continue | SessionOutcome::MonitorReports(_)) => {}
            Err(error) => {
                eprintln!("error: {error}");
                if matches!(error, Error::Disconnected) {
                    result = Err(error);
                    break;
                }
            }
        }
    }

    if let Some(running) = monitor.take() {
        running.stop().await;
    }
    result
}

async fn session(options: &BleOptions, interval: Duration) -> Result<ExitCode, Error> {
    if interval.is_zero() {
        eprintln!("session error: --interval-ms must be greater than zero");
        return Ok(ExitCode::FAILURE);
    }
    let transport = match BleTransport::connect(options).await {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("session error: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };
    let session = Arc::new(AsyncMutex::new(Session::new(transport)));
    let mtu = {
        let guard = session.lock().await;
        guard.transport().mtu()
    };
    let keepalive = match start_session_keepalive(Arc::clone(&session), interval).await {
        Ok(keepalive) => keepalive,
        Err(error) => {
            let guard = session.lock().await;
            let _ = guard.transport().disconnect().await;
            eprintln!("session error: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };
    let state = Arc::clone(&keepalive.state);
    let started = Instant::now();
    let result = session_repl_loop(&session, &state, started, mtu, interval).await;
    shutdown_session(&session, keepalive).await;

    match result {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            eprintln!("session error: {error}");
            Ok(ExitCode::FAILURE)
        }
    }
}

fn print_report(report: &InputReport) {
    println!(
        "keys=0x{:08X} LX={} LY={} RX={} RY={} L2={} R2={} battery={}",
        report.keys,
        report.left_stick_x,
        report.left_stick_y,
        report.right_stick_x,
        report.right_stick_y,
        report.trigger_l,
        report.trigger_r,
        report.battery
    );
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use eightbitdo_protocol::config_bt2::Bt2Config;
    use eightbitdo_protocol::header::CustomHeader;

    use super::*;

    /// Transport that records writes and replays a finite queue of input
    /// notifications, then stays pending instead of reporting a disconnect.
    #[derive(Clone)]
    struct KeepaliveTestTransport {
        sent: Arc<Mutex<Vec<Vec<u8>>>>,
        incoming: Arc<Mutex<VecDeque<Vec<u8>>>>,
        disconnected: Arc<AtomicBool>,
        fail_input: Arc<AtomicBool>,
    }

    impl KeepaliveTestTransport {
        fn new() -> Self {
            Self {
                sent: Arc::new(Mutex::new(Vec::new())),
                incoming: Arc::new(Mutex::new(VecDeque::new())),
                disconnected: Arc::new(AtomicBool::new(false)),
                fail_input: Arc::new(AtomicBool::new(false)),
            }
        }

        fn queue_input(&self, report: &InputReport) {
            let payload = report.to_bytes();
            let header =
                CustomHeader::new(commands::cmd::INPUT, 0, 0x0C, 0x0C).with_payload(&payload);
            self.queue_frame(&header, &payload);
        }

        fn queue_envelope_input(&self, report: &InputReport) {
            let payload = report.to_bytes();
            let header =
                CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::INPUT, 0x0C, 0x0C)
                    .with_payload(&payload);
            self.queue_frame(&header, &payload);
        }

        fn queue_response(&self, header: &CustomHeader, payload: &[u8]) {
            self.queue_frame(header, payload);
        }

        fn queue_frame(&self, header: &CustomHeader, payload: &[u8]) {
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(payload);
            self.incoming.lock().unwrap().push_back(frame);
        }

        fn sent_frames(&self) -> Vec<Vec<u8>> {
            self.sent.lock().unwrap().clone()
        }

        fn is_disconnected(&self) -> bool {
            self.disconnected.load(Ordering::SeqCst)
        }

        fn fail_inputs(&self) {
            self.fail_input.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl Transport for KeepaliveTestTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            if self.fail_input.load(Ordering::SeqCst) {
                if let Some(header_bytes) = data.get(1..) {
                    if let Ok(header) = CustomHeader::parse(header_bytes) {
                        if header.cmd == commands::cmd::INPUT {
                            return Err(Error::Disconnected);
                        }
                    }
                }
            }
            self.sent.lock().unwrap().push(data.to_vec());
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            if let Some(frame) = self.incoming.lock().unwrap().pop_front() {
                return Ok(frame);
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
            Err(Error::Disconnected)
        }

        fn mtu(&self) -> u16 {
            64
        }

        async fn disconnect(&self) -> Result<(), Error> {
            self.disconnected.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn keepalive_enables_reports_polls_inputs_and_drains_notifications() {
        let transport = KeepaliveTestTransport::new();
        let report = InputReport {
            keys: 0x0000_2001,
            left_stick_x: 0x7F,
            left_stick_y: 0x80,
            right_stick_x: 0xFF,
            right_stick_y: 0x00,
            trigger_r: 0xCC,
            trigger_l: 0x33,
            battery: 0x4B,
            reserved: 0,
        };
        transport.queue_input(&report);
        transport.queue_envelope_input(&report);
        transport.queue_envelope_input(&report);

        let session = Session::new(&transport);
        let stats = run_keepalive(
            &session,
            Duration::from_millis(1),
            Duration::from_millis(50),
            std::future::pending::<()>(),
        )
        .await
        .unwrap();

        assert_eq!(stats.received, 3);
        assert!(
            stats.sent >= 2,
            "only {} input requests were sent",
            stats.sent
        );

        let frames = transport.sent_frames();
        assert_eq!(frames.len() as u64, stats.sent + 2);
        assert_eq!(frames[0][0], 0x04);
        let enable = CustomHeader::parse(&frames[0][1..]).unwrap();
        assert_eq!(enable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(enable.subcmd, 1);

        let disable = CustomHeader::parse(&frames[frames.len() - 1][1..]).unwrap();
        assert_eq!(disable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(disable.subcmd, 0);

        for frame in &frames[1..frames.len() - 1] {
            assert_eq!(frame.len(), 17);
            assert_eq!(frame[0], 0x04);
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.cmd, commands::cmd::INPUT);
            assert_eq!(header.chunk_len, 0);
            assert_eq!(header.total_len, 0x0C);
            assert_eq!(header.offset, 0x0C);
        }
    }

    #[test]
    fn cur_slot_labels_cover_profiles_none_and_unknown() {
        assert_eq!(cur_slot_label(0), "0 (profile 1)");
        assert_eq!(cur_slot_label(1), "1 (profile 2)");
        assert_eq!(cur_slot_label(2), "2 (profile 3)");
        assert_eq!(cur_slot_label(4), "4 (none)");
        assert_eq!(cur_slot_label(6), "unknown (6)");
    }

    #[test]
    fn slot_flags_render_as_hex() {
        assert_eq!(
            format_slot_flags([0x2020_0911, 0x2020_0911, 0xFFFF_FFFF]),
            "[0x20200911, 0x20200911, 0xFFFFFFFF]"
        );
    }

    #[test]
    fn summary_json_uses_informational_crc_fields() {
        let snapshot = ConfigSnapshot {
            config: Bt2Config::zeroed(),
            stored_crc: 0x2C55,
            computed_crc: 0x6A05,
            crc_valid: false,
        };
        let summary = json_summary(&snapshot, "abc");
        assert_eq!(summary["crc_stored"], 0x2C55);
        assert_eq!(summary["crc_in_place"], 0x6A05);
        assert!(summary.get("crc_valid").is_none());
        assert!(summary.get("stored_crc").is_none());
        assert!(summary.get("computed_crc").is_none());
    }

    fn sample_input_report() -> InputReport {
        InputReport {
            keys: 0x0000_2001,
            left_stick_x: 0x7F,
            left_stick_y: 0x80,
            right_stick_x: 0xFF,
            right_stick_y: 0x00,
            trigger_r: 0xCC,
            trigger_l: 0x33,
            battery: 0x4B,
            reserved: 0,
        }
    }

    fn queue_version_response(transport: &KeepaliveTestTransport) {
        let payload = [0u8; 4];
        let header = CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::READ_VERSION, 4, 0)
            .with_payload(&payload);
        transport.queue_response(&header, &payload);
    }

    fn queue_config_responses(transport: &KeepaliveTestTransport, config: &Bt2Config) {
        let bytes = config.as_bytes();
        for (index, chunk) in bytes.chunks(142).enumerate() {
            let offset = (index * 142) as u32;
            let header = CustomHeader::new(
                commands::cmd::RESPONSE,
                commands::cmd::READ_CONFIG,
                CONFIG_SIZE as u32,
                offset,
            )
            .with_payload(chunk);
            transport.queue_response(&header, chunk);
        }
    }

    #[tokio::test]
    async fn backup_reads_version_and_config_in_one_session() {
        let transport = KeepaliveTestTransport::new();
        queue_version_response(&transport);
        let config = Bt2Config::zeroed();
        queue_config_responses(&transport, &config);

        let session = Session::new(&transport);
        backup_session(
            &session,
            None,
            None,
            Duration::ZERO,
            std::future::pending::<()>(),
        )
        .await
        .unwrap();

        let frames = transport.sent_frames();
        assert_eq!(frames.len(), 1 + 20);
        let version = CustomHeader::parse(&frames[0][1..]).unwrap();
        assert_eq!(version.cmd, commands::cmd::READ_VERSION);
        assert_eq!(version.chunk_len, 4);

        for (index, frame) in frames[1..].iter().enumerate() {
            assert_eq!(frame.len(), 17);
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.cmd, commands::cmd::READ_CONFIG);
            assert_eq!(header.offset as usize, index * 142);
            assert_eq!(header.chunk_len, CONFIG_SIZE as u16);
            assert_eq!(header.total_len, CONFIG_SIZE as u32);
        }
    }

    #[tokio::test]
    async fn backup_hold_runs_official_input_keepalive() {
        let transport = KeepaliveTestTransport::new();
        queue_version_response(&transport);
        let config = Bt2Config::zeroed();
        queue_config_responses(&transport, &config);
        transport.queue_envelope_input(&sample_input_report());
        transport.queue_envelope_input(&sample_input_report());

        let session = Session::new(&transport);
        backup_session(
            &session,
            None,
            None,
            Duration::from_millis(50),
            std::future::pending::<()>(),
        )
        .await
        .unwrap();

        let frames = transport.sent_frames();
        assert!(
            frames.len() >= 1 + 20 + 2 + 2,
            "unexpected frame count: {}",
            frames.len()
        );
        let hold = &frames[21..];
        let enable = CustomHeader::parse(&hold[0][1..]).unwrap();
        assert_eq!(enable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(enable.subcmd, 1);
        let disable = CustomHeader::parse(&hold[hold.len() - 1][1..]).unwrap();
        assert_eq!(disable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(disable.subcmd, 0);
        for frame in &hold[1..hold.len() - 1] {
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.cmd, commands::cmd::INPUT);
        }
    }

    async fn wait_for_keepalive_send(state: &SessionKeepaliveState) {
        for _ in 0..100 {
            if state.sent.load(Ordering::SeqCst) > 0 {
                // Let the 1 ms receive drain finish before queueing command
                // responses for the test.
                tokio::time::sleep(Duration::from_millis(10)).await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("keepalive did not send its first poll");
    }

    #[tokio::test]
    async fn session_commands_share_one_connection_and_quit_disables() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_secs(3600))
            .await
            .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        queue_version_response(&transport);
        let config = Bt2Config::zeroed();
        queue_config_responses(&transport, &config);
        transport.queue_envelope_input(&sample_input_report());
        transport.queue_envelope_input(&sample_input_report());

        let state = Arc::clone(&keepalive.state);
        let started = Instant::now();
        let mut monitor_state: Option<SessionMonitor> = None;

        let info = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            &mut monitor_state,
            SessionCommand::Info,
        )
        .await
        .unwrap();
        assert_eq!(info, SessionOutcome::Continue);

        let dump = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            &mut monitor_state,
            SessionCommand::ConfigDump {
                raw: None,
                json: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(dump, SessionOutcome::Continue);

        let monitor = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            &mut monitor_state,
            SessionCommand::Monitor { count: Some(2) },
        )
        .await
        .unwrap();
        assert_eq!(monitor, SessionOutcome::MonitorReports(2));

        shutdown_session(&session, keepalive).await;

        let frames = transport.sent_frames();
        let first = CustomHeader::parse(&frames[0][1..]).unwrap();
        assert_eq!(first.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(first.subcmd, 1);
        let last = CustomHeader::parse(&frames[frames.len() - 1][1..]).unwrap();
        assert_eq!(last.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(last.subcmd, 0);

        let version_requests = frames
            .iter()
            .filter(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::READ_VERSION
            })
            .count();
        assert_eq!(version_requests, 1);
        let config_requests = frames
            .iter()
            .filter(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::READ_CONFIG
            })
            .count();
        assert_eq!(config_requests, 20);
        let input_requests = frames
            .iter()
            .filter(|frame| CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::INPUT)
            .count();
        assert!(
            input_requests >= 2,
            "monitor sent only {input_requests} input requests"
        );
        assert!(transport.is_disconnected());
    }

    #[tokio::test]
    async fn session_keepalive_stops_while_session_lock_is_held() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_millis(1))
            .await
            .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        let guard = session.lock().await;
        let sent_before = keepalive.state.sent.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        let sent_after = keepalive.state.sent.load(Ordering::SeqCst);
        assert_eq!(sent_before, sent_after);
        drop(guard);

        shutdown_session(&session, keepalive).await;
        assert!(transport.is_disconnected());
    }

    #[tokio::test]
    async fn session_monitor_stop_stops_background_monitor_and_resumes_keepalive() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_millis(1))
            .await
            .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        for _ in 0..2 {
            transport.queue_envelope_input(&sample_input_report());
        }
        let input_count = || {
            transport
                .sent_frames()
                .iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::INPUT
                })
                .count()
        };
        let before_monitor = input_count();

        let state = Arc::clone(&keepalive.state);
        let started = Instant::now();
        let mut monitor_state: Option<SessionMonitor> = None;

        let outcome = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            &mut monitor_state,
            SessionCommand::Monitor { count: None },
        )
        .await
        .unwrap();
        assert_eq!(outcome, SessionOutcome::Continue);
        assert!(monitor_state.is_some());
        tokio::time::sleep(Duration::from_millis(10)).await;
        let during_monitor = input_count();
        assert!(
            during_monitor > before_monitor,
            "monitor did not send 0x0003"
        );

        let outcome = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            &mut monitor_state,
            SessionCommand::MonitorStop,
        )
        .await
        .unwrap();
        assert_eq!(outcome, SessionOutcome::Continue);
        assert!(monitor_state.is_none());
        let at_stop = input_count();

        tokio::time::sleep(Duration::from_millis(20)).await;
        let after_stop = input_count();
        assert!(
            after_stop > at_stop,
            "keepalive did not resume after monitor stop"
        );

        shutdown_session(&session, keepalive).await;
        assert!(transport.is_disconnected());
    }

    #[tokio::test(start_paused = true)]
    async fn session_keepalive_ticks_follow_20ms_interval() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_millis(20))
            .await
            .unwrap();

        for _ in 0..4 {
            tokio::time::advance(Duration::from_millis(21)).await;
            for _ in 0..10 {
                tokio::task::yield_now().await;
            }
        }
        let sent = keepalive.state.sent.load(Ordering::SeqCst);
        assert!(sent >= 3, "only {sent} keepalive polls were sent");

        shutdown_session(&session, keepalive).await;
        assert!(transport.is_disconnected());
    }

    #[tokio::test]
    async fn session_keepalive_degrades_after_failure_limit() {
        let transport = KeepaliveTestTransport::new();
        transport.fail_inputs();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let config = SessionKeepaliveConfig {
            interval: Duration::from_millis(1),
            max_consecutive_failures: 1,
        };
        let keepalive = start_session_keepalive_with_config(Arc::clone(&session), config)
            .await
            .unwrap();

        for _ in 0..100 {
            if keepalive.state.degraded.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(keepalive.state.degraded.load(Ordering::SeqCst));
        assert!(keepalive.state.last_error().is_some());

        shutdown_session(&session, keepalive).await;
        assert!(transport.is_disconnected());
    }
}
