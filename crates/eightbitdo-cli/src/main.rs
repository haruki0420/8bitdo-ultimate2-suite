//! Read-only command-line interface for the 8BitDo Ultimate 2 Bluetooth
//! controller.
//!
//! The binary exposes `scan`, `info`, `config dump`, `monitor` and
//! `keepalive`. No configuration write, macro flash, save or OTA command is
//! reachable from this binary.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use eightbitdo_core::{
    scan_devices, BleOptions, BleTransport, ConfigSnapshot, Error, Session, Transport,
};
use eightbitdo_protocol::commands;
use eightbitdo_protocol::config_bt2::{Slot, CONFIG_SIZE};
use eightbitdo_protocol::header;
use eightbitdo_protocol::input_report::InputReport;
use serde_json::json;
use sha2::{Digest, Sha256};

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
    Ok(())
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

    let sha256 = hex::encode(Sha256::digest(snapshot.config.as_bytes()));

    if let Some(path) = raw_path {
        std::fs::write(path, snapshot.config.as_bytes())?;
        println!(
            "wrote raw configuration: {} ({CONFIG_SIZE} bytes)",
            path.display()
        );
    }

    if let Some(path) = json_path {
        let summary = json_summary(&snapshot, &sha256);
        let encoded = serde_json::to_string_pretty(&summary).map_err(std::io::Error::other)?;
        std::fs::write(path, format!("{encoded}\n"))?;
        println!("wrote JSON summary: {}", path.display());
    }

    print_config_summary(&snapshot, &sha256);
    Ok(())
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
    Ok(header.cmd == commands::cmd::INPUT)
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
    struct KeepaliveTestTransport {
        sent: Mutex<Vec<Vec<u8>>>,
        incoming: Mutex<VecDeque<Vec<u8>>>,
    }

    impl KeepaliveTestTransport {
        fn new() -> Self {
            Self {
                sent: Mutex::new(Vec::new()),
                incoming: Mutex::new(VecDeque::new()),
            }
        }

        fn queue_input(&self, report: &InputReport) {
            let payload = report.to_bytes();
            let header =
                CustomHeader::new(commands::cmd::INPUT, 0, 0x0C, 0x0C).with_payload(&payload);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(&payload);
            self.incoming.lock().unwrap().push_back(frame);
        }

        fn sent_frames(&self) -> Vec<Vec<u8>> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Transport for KeepaliveTestTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
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
        for _ in 0..3 {
            transport.queue_input(&report);
        }

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
}
