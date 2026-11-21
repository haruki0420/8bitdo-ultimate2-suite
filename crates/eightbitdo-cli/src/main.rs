//! Read-only command-line interface for the 8BitDo Ultimate 2 Bluetooth
//! controller.
//!
//! Phase 2a deliberately exposes only `scan`, `info`, `config dump` and
//! `monitor`. No configuration write, macro flash, save or OTA command is
//! reachable from this binary.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use eightbitdo_core::{
    scan_devices, BleOptions, BleTransport, ConfigSnapshot, Error, Session, Transport,
};
use eightbitdo_protocol::config_bt2::{Slot, CONFIG_SIZE};
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
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Error> {
    let options = BleOptions {
        name: cli.name.clone(),
        address: cli.address.clone(),
        scan_timeout: Duration::from_secs(cli.scan_timeout),
        connect_timeout: Duration::from_secs(cli.timeout),
        verbose: cli.verbose,
    };

    match cli.command {
        Command::Scan => scan(Duration::from_secs(cli.scan_timeout)).await,
        Command::Info => info(&options).await,
        Command::Config { command } => match command {
            ConfigCommand::Dump { raw, json } => {
                dump_config(&options, raw.as_deref(), json.as_deref()).await
            }
        },
        Command::Monitor { count } => monitor(&options, count).await,
    }
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

    json!({
        "size": CONFIG_SIZE,
        "sha256": sha256,
        "cur_slot": snapshot.config.cur_slot_raw(),
        "gamepad_mode": snapshot.config.gamepad_mode_raw(),
        "slot_flags": snapshot.config.slot_flags(),
        "profile_names": profile_names,
        "stored_crc": snapshot.stored_crc,
        "computed_crc": snapshot.computed_crc,
        "crc_valid": snapshot.crc_valid,
    })
}

fn print_config_summary(snapshot: &ConfigSnapshot, sha256: &str) {
    let mode = snapshot
        .config
        .gamepad_mode()
        .map_or_else(|| "unknown".to_string(), |mode| format!("{mode:?}"));
    let cur_slot = snapshot.config.cur_slot().map_or_else(
        || format!("{} (none)", snapshot.config.cur_slot_raw()),
        |slot| {
            format!(
                "{} (profile {})",
                snapshot.config.cur_slot_raw(),
                slot.number()
            )
        },
    );

    println!("config size: {CONFIG_SIZE} bytes");
    println!("sha256: {sha256}");
    println!("cur_slot: {cur_slot}");
    println!(
        "gamepad_mode: {} ({mode})",
        snapshot.config.gamepad_mode_raw()
    );
    println!("slot_flags: {:?}", snapshot.config.slot_flags());
    println!(
        "profile_names: {:?}",
        Slot::ALL
            .iter()
            .map(|slot| snapshot.config.profile_name(*slot))
            .collect::<Vec<_>>()
    );

    if snapshot.crc_valid {
        println!(
            "crc: stored=0x{:04X} computed=0x{:04X} OK",
            snapshot.stored_crc, snapshot.computed_crc
        );
    } else {
        println!(
            "crc: stored=0x{:04X} computed=0x{:04X} MISMATCH",
            snapshot.stored_crc, snapshot.computed_crc
        );
        eprintln!(
            "warning: configuration CRC mismatch (stored 0x{:04X}, computed 0x{:04X}); \
             the dump may be incomplete or the stored value may not be self-consistent",
            snapshot.stored_crc, snapshot.computed_crc
        );
    }
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
