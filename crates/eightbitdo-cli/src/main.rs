//! Command-line interface for the 8BitDo Ultimate 2 Bluetooth controller.
//!
//! The binary exposes `scan`, `info`, `config dump` / `write` / `set` /
//! `remap` / `map show`, `backup`, `monitor`, `keepalive`, `session` and the
//! `macro` backup / write / restore commands. Full 2768-byte configuration
//! writes, macro flash writes and single-button remapping are reachable; OTA
//! is not.

use std::future::Future;
use std::io::{IsTerminal, Write};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use eightbitdo_core::{
    button_mapping, button_mappings, config_write_chunk_len, macro_header_chunk_len,
    macro_ops::is_skipped_steps, macro_write_chunk_len, macro_write_chunk_len_requested,
    map_key_flag, max_protocol_payload, scan_devices, validate_macro_chunk_size, BleOptions,
    BleTransport, ConfigCrcStatus, ConfigEdit, ConfigRegion, ConfigSnapshot, ConfigWritePlan,
    ConfigWriteTimeouts, Error, MacroBackup, MacroEntry, MacroWritePlan, PreparedConfigWrite,
    Session, Transport, VersionInfo, APP_BLE_NAME,
};
use eightbitdo_protocol::commands::{MacroWriteLayout, SaveTransport};
use eightbitdo_protocol::config_bt2::{
    macro_record_offset, macro_slot_offset, Argb, Bt2Config, GamepadMode, JoyRecord, MacroIndex,
    MacroSlot, RumbleRecord, SixAxisRecord, Slot, TriggerRecord, CONFIG_SIZE, MACROS_PER_SLOT,
    MACRO_SLOT_SIZE, OFF_LEGACY_A, OFF_LEGACY_B, OFF_MACROS, OFF_SIX_AXIS, SLOT_COUNT,
};
use eightbitdo_protocol::input_report::InputReport;
use eightbitdo_protocol::keymap::ButtonIndex;
use eightbitdo_protocol::macro_data::{MacroFile, MacroStep, MAX_MACRO_STEPS};
use eightbitdo_protocol::KeyMap;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Parser)]
#[command(
    name = "eightbitdo",
    version,
    about = "Configuration and macro tools for the 8BitDo Ultimate 2 Bluetooth controller"
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
    ///
    /// With `--check`, print one line and exit 0 when an App Mode device is
    /// present or 3 when it is not, without parsing the human-readable table.
    Scan {
        /// Exit 0 when an App Mode (`80ND`) device is found, 3 when none is.
        #[arg(long)]
        check: bool,
    },

    /// Connect and read the firmware version.
    Info,

    /// Configuration operations: dump, write, set, remap and map show.
    ///
    /// Configuration writes use BLE only; USB HID config write is not
    /// implemented (the USB Save sub-command 0x0123 exists but no USB
    /// transport is wired for config writes).
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Macro operations: full 3-slot × 4-macro backup, single write, restore.
    Macro {
        #[command(subcommand)]
        command: MacroCommand,
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

    /// Keep one connection open and run commands interactively.
    ///
    /// The REPL is read-only by default. `macro write` / `macro restore`
    /// require `--allow-macro-write`; `config set` / `config remap` /
    /// `config write` require `--allow-config-write` and an explicit `--yes`
    /// or the interactive `yes` prompt.
    Session {
        /// Input poll interval in milliseconds.
        #[arg(long, default_value_t = 20, value_name = "MILLISECONDS")]
        interval_ms: u64,

        /// Enable `macro write` / `macro restore` inside the REPL.
        ///
        /// `macro backup` is read-only on the controller and is always
        /// available. This flag does not turn the session into a general
        /// write mode: only the two macro flash commands are enabled.
        #[arg(long)]
        allow_macro_write: bool,

        /// Enable `config set` / `config remap` / `config write` inside the
        /// REPL. Each write still needs `--yes` or the interactive `yes`
        /// confirmation, and a pre-write backup is always taken.
        #[arg(long)]
        allow_config_write: bool,

        /// Do not send the `0x0007` ReportEnable(true) when the session
        /// starts.
        ///
        /// The official app enables the realtime input stream before polling,
        /// so the default is ON. With this flag the REPL keeps polling
        /// `0x0003` but never sends `0x0007`, matching the top-level
        /// `eightbitdo macro write` default (which sends no ReportEnable).
        #[arg(long)]
        no_report_enable: bool,
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

    /// Write a complete 2768-byte configuration buffer and Save it.
    ///
    /// Read the current block with `config dump --raw FILE`, edit the bytes
    /// offline, then write the result. A pre-write backup is mandatory and the
    /// result is verified by reading the block back.
    ///
    /// Known fields are validated before connecting (gamepad_mode, cur_slot,
    /// joy ranges, enabled gyro ranges, hotwheel speed). Changes to the macro
    /// header region (0x68C..0x914) and to gamepad_mode/cur_slot are warned
    /// about because step data is not part of this write and the BLE link may
    /// restart.
    Write {
        /// 2768-byte configuration file (`config dump --raw` output, edited).
        #[arg(long, value_name = "FILE")]
        file: PathBuf,

        /// Directory for the mandatory pre-write backup (config.bin +
        /// config.json). Defaults to `config-backup-<unix-nanos>-<pid>-<seq>`.
        #[arg(long, value_name = "DIR")]
        backup_dir: Option<PathBuf>,

        /// Copy the device's current reserved bytes (0x1F4..0x68C and
        /// 0x914..0x92C) into the file before writing.
        ///
        /// Use this to restore a backup after the device normalized reserved
        /// bytes; without it a reserved-byte difference is refused.
        #[arg(long)]
        accept_reserved_drift: bool,

        /// Downgrade undocumented / out-of-range field checks to warnings.
        ///
        /// Use only to restore a backup that contains values the device
        /// actually holds but this tool does not recognize (for example an
        /// undocumented key_map). The reserved-region guard still applies.
        #[arg(long)]
        force_raw: bool,

        /// Override the 0x0001 write payload size in bytes.
        ///
        /// Defaults to min(MTU payload, 74): 74 reproduces the official
        /// writer (MAX_ATT_MTU 92 - 0x12, static analysis). Accepted range
        /// before connecting is 1..=180 (largest observed BLE payload); the
        /// negotiated MTU is enforced after connecting. Larger values are
        /// experimental A/B.
        #[arg(long, value_name = "BYTES")]
        write_chunk: Option<u16>,

        /// Skip the interactive confirmation prompt.
        #[arg(long)]
        yes: bool,
    },

    /// Apply one typed field change, then write the whole block and Save it.
    ///
    /// Supported fields: cur_slot, gamepad_mode, profile_name, rumble.level,
    /// joy.left_start / left_end / right_start / right_end,
    /// trigger.left_start / left_end / right_start / right_end,
    /// gyro.enabled / trigger_mode / sensitivity / dead_compensate / mapping /
    /// key, led.dynamic.color / bg_color / flag, led.hotwheel.color / bg_color
    /// / speed / flag, map_key.flag (typed values 0 = disabled, 1 = enabled;
    /// a raw dump may store opaque observed values such as 0x20200911 or
    /// 0xFFFFFFFF, which this typed field does not accept).
    ///
    /// Side effects: joy.* and trigger.* set that record's `flag` to 1
    /// (custom settings enabled), and led.*.color sets that theme's `flag` to
    /// 1. Flag fields only accept 0 or 1.
    Set {
        /// Field path (see the command description).
        field: String,

        /// New value. Colors accept 0xAARRGGBB or #RRGGBB.
        value: String,

        /// Profile slot (0..=2) for slot-scoped fields.
        #[arg(long, default_value_t = 0, value_name = "N")]
        slot: u8,

        /// Directory for the mandatory pre-write backup.
        #[arg(long, value_name = "DIR")]
        backup_dir: Option<PathBuf>,

        /// Override the 0x0001 write payload size in bytes (default
        /// min(MTU payload, 74), the official 74-byte split; accepted
        /// 1..=180 before connecting, then the negotiated MTU is enforced).
        #[arg(long, value_name = "BYTES")]
        write_chunk: Option<u16>,

        /// Skip the interactive confirmation prompt.
        #[arg(long)]
        yes: bool,
    },

    /// Remap one physical button and write the whole configuration + Save.
    ///
    /// Only the 4-byte `MapKeyRecord.key[]` entry changes, matching the
    /// official single-entry write. Invalid targets (`KeyMap_My_NUll = 227`,
    /// unknown combinations) are rejected before any I/O. A pre-write backup
    /// is mandatory and the result is verified by a read-back.
    Remap {
        /// Physical button: A, B, X, Y, L1..L3, R1..R3, Select, Start, Share,
        /// Logo, Up, Down, Left, Right, P1/PL, P2/PR, P3/L4, P4/R4, or 0..=21.
        from: String,

        /// Target: a KeyMap name, M1..M4, `null`/`disable`, 0xHEX or decimal.
        to: String,

        /// Profile slot (0..=2).
        #[arg(long, default_value_t = 0, value_name = "N")]
        slot: u8,

        /// Directory for the mandatory pre-write backup.
        #[arg(long, value_name = "DIR")]
        backup_dir: Option<PathBuf>,

        /// Override the 0x0001 write payload size in bytes (default
        /// min(MTU payload, 74), the official 74-byte split; accepted
        /// 1..=180 before connecting, then the negotiated MTU is enforced).
        #[arg(long, value_name = "BYTES")]
        write_chunk: Option<u16>,

        /// Skip the interactive confirmation prompt.
        #[arg(long)]
        yes: bool,
    },

    /// Show or edit the 22 button mappings.
    Map {
        #[command(subcommand)]
        command: MapCommand,
    },

    /// Switch the controller's active profile (`cmd 0x0015`).
    ///
    /// Sends one payload-less `WriteSLOT` request (`subcmd = slot`,
    /// `total_len = 0`, `offset = 0`) and waits for its acknowledgement. No
    /// `Save (0x0006)` is sent: whether `0x0015` needs one is **unconfirmed on
    /// hardware** (`docs/PROTOCOL_SPEC.md` §6.2, docs/FEATURE_MATRIX.md).
    ///
    /// This changes the controller's active profile immediately. It does not
    /// rewrite the 2768-byte configuration, so no pre-write backup is taken;
    /// switch back with another `profile` call if needed.
    Profile {
        /// Profile slot (0..=2, matching the protocol and `config map`).
        slot: u8,

        /// Skip the interactive confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum MapCommand {
    /// Print the 22 mappings of one profile slot (read-only).
    Show {
        /// Profile slot (0..=2).
        #[arg(long, default_value_t = 0, value_name = "N")]
        slot: u8,
    },
}

#[derive(Debug, Subcommand)]
enum MacroCommand {
    /// Back up all 3 slots × 4 macros (12 entries) into a directory.
    Backup {
        /// Output directory for config.bin, config.json, macros.json,
        /// macros.toml and the per-macro raw .bin files.
        #[arg(long, value_name = "DIR")]
        dir: PathBuf,

        /// Input poll interval in milliseconds while the operation runs.
        /// Experimental: the official app does not poll during a macro write,
        /// so the default is 0 (disabled).
        #[arg(long, default_value_t = 0, value_name = "MS")]
        poll_interval_ms: u64,

        /// Experimental: send `0x0007` ReportEnable before the operation
        /// (independent of --poll-interval-ms). Not a full reproduction of the
        /// official sequence: no connect-time ReportEnable(0) is sent.
        #[arg(long)]
        report_enable: bool,
    },

    /// Write one macro from a JSON/TOML macro file.
    Write {
        /// Slot index (0..=2, matching the protocol and docs).
        #[arg(long, value_name = "N")]
        slot: u8,

        /// Macro index (0..=3, matching the protocol and docs).
        #[arg(long, value_name = "N")]
        index: u8,

        /// JSON or TOML macro file (MacroFile format).
        #[arg(long, value_name = "FILE")]
        file: PathBuf,

        /// Trigger key: M1..M4, a KeyMap name, 0xHEX or decimal.
        /// Omit to preserve the existing header value.
        #[arg(long, value_name = "KEY")]
        trigger: Option<String>,

        /// Directory for the mandatory pre-write backup. Defaults to
        /// `macro-backup-<unix-nanos>-<pid>-<sequence>`.
        #[arg(long, value_name = "DIR")]
        backup_dir: Option<PathBuf>,

        /// Force 32- or 64-byte write chunks. Omit to select from the MTU.
        #[arg(long, value_name = "32|64")]
        chunk_size: Option<u16>,

        /// EXPERIMENT: revert WriteBT2Macro `total_len` to the pre-2026-11-22
        /// framing `align32(steps * 10)` instead of the default absolute end
        /// address `(macro_index << 12) + align32(steps * 10)`.
        ///
        /// Device evidence is limited to **BLE, slot S1, `macro_idx=0 (M1)` and
        /// `macro_idx=1 (M2)`**: `macro_idx=0` is unaffected (sector base 0),
        /// while `macro_idx=1` ACKs but does not persist without the sector
        /// base. `macro_idx=2/3 (M3/M4)` and slots S2/S3 are extrapolated and
        /// unverified. Use with `--backup-dir` and the mandatory pre-write
        /// backup.
        #[arg(long)]
        write_total_relative: bool,

        /// Input poll interval in milliseconds while the operation runs.
        /// Experimental: the official app does not poll during a macro write,
        /// so the default is 0 (disabled).
        #[arg(long, default_value_t = 0, value_name = "MS")]
        poll_interval_ms: u64,

        /// Experimental: send `0x0007` ReportEnable before the operation
        /// (independent of --poll-interval-ms). Not a full reproduction of the
        /// official sequence: no connect-time ReportEnable(0) is sent.
        #[arg(long)]
        report_enable: bool,
    },

    /// Restore all non-empty macros from a backup directory.
    Restore {
        /// Backup directory created by `macro backup`.
        #[arg(long, value_name = "DIR")]
        dir: PathBuf,

        /// Allow skipped unreadable current macros and allow a backup where a
        /// macro is empty but the current controller macro is non-empty (the
        /// 216B slot header is overwritten; stale step data remains in flash).
        /// A failed pre-restore backup always aborts, even with --force.
        #[arg(long)]
        force: bool,

        /// Force 32- or 64-byte write chunks. Omit to select from the MTU.
        #[arg(long, value_name = "32|64")]
        chunk_size: Option<u16>,

        /// EXPERIMENT: relative `total_len` for every restored macro (see
        /// `macro write --write-total-relative`). Applies to all per-macro
        /// plans built by this restore.
        #[arg(long)]
        write_total_relative: bool,

        /// Input poll interval in milliseconds while the operation runs.
        /// Experimental: the official app does not poll during a macro write,
        /// so the default is 0 (disabled).
        #[arg(long, default_value_t = 0, value_name = "MS")]
        poll_interval_ms: u64,

        /// Experimental: send `0x0007` ReportEnable before the operation
        /// (independent of --poll-interval-ms). Not a full reproduction of the
        /// official sequence: no connect-time ReportEnable(0) is sent.
        #[arg(long)]
        report_enable: bool,
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
        lock_path: None,
    };

    match cli.command {
        // `scan` only touches the adapter, not the controller link: SIGINT keeps
        // its default termination and there is no App Mode to warn about.
        Command::Scan { check } => {
            return scan(Duration::from_secs(cli.scan_timeout), check).await;
        }
        // `backup` and `monitor` own their full duration; both race the whole
        // body (connect, reads, optional hold) against SIGINT so a Ctrl-C
        // cannot be swallowed while the link is held.
        Command::Backup { raw, json, hold } => {
            return backup(
                &options,
                raw.as_deref(),
                json.as_deref(),
                Duration::from_secs(hold),
            )
            .await;
        }
        Command::Monitor { count } => return monitor(&options, count).await,
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
        Command::Session {
            interval_ms,
            allow_macro_write,
            allow_config_write,
            no_report_enable,
        } => {
            return session(
                &options,
                Duration::from_millis(interval_ms),
                allow_macro_write,
                allow_config_write,
                session_report_enable(no_report_enable),
            )
            .await;
        }
        // Everything else holds the link through `BleTransport::connect` and
        // would otherwise die on SIGINT before the operator learns that 80ND
        // is gone. Wrap connect + command as one cancellable unit; flash
        // commands use the two-phase `with_sigint_flash` instead.
        command @ (Command::Info | Command::Config { .. } | Command::Macro { .. }) => {
            if is_flash_command(&command) {
                // Two phases: the prepare/prompt/backup part is cancellable,
                // the write part is not (see `with_sigint_flash`).
                reset_flash_write_active();
                let outcome = with_sigint_flash(
                    dispatch_connected_command(&options, command),
                    wait_for_sigint(),
                )
                .await;
                reset_flash_write_active();
                match outcome {
                    FlashSigintOutcome::Finished(result) => result?,
                    FlashSigintOutcome::FinishedAfterInterrupt(result) => {
                        if let Err(error) = &result {
                            eprintln!("error: {error}");
                        }
                        warn_flash_interrupted_recovery();
                        return Ok(ExitCode::from(if result.is_ok() {
                            EXIT_INTERRUPTED
                        } else {
                            EXIT_SESSION_ERROR
                        }));
                    }
                    FlashSigintOutcome::Cancelled => {
                        // Dropped during prepare; no flash frame was sent.
                        warn_sigint_disconnects();
                        return Ok(ExitCode::from(EXIT_INTERRUPTED));
                    }
                }
            } else {
                match with_sigint(
                    dispatch_connected_command(&options, command),
                    wait_for_sigint(),
                )
                .await
                {
                    SigintOutcome::Finished(result) => result?,
                    SigintOutcome::Interrupted => {
                        warn_sigint_disconnects();
                        return Ok(ExitCode::from(EXIT_INTERRUPTED));
                    }
                }
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Dispatch the commands whose whole duration owns the BLE link.
///
/// Only the variants wrapped by [`with_sigint`] in [`run`] reach here; the
/// lifecycle-owning commands (`scan`, `backup`, `monitor`, `keepalive`,
/// `session`) are handled by `run` directly.
async fn dispatch_connected_command(options: &BleOptions, command: Command) -> Result<(), Error> {
    match command {
        Command::Info => info(options).await,
        Command::Config { command } => dispatch_config_command(options, command).await,
        Command::Macro { command } => dispatch_macro_command(options, command).await,
        Command::Scan { .. }
        | Command::Backup { .. }
        | Command::Monitor { .. }
        | Command::Keepalive { .. }
        | Command::Session { .. } => {
            unreachable!("only SIGINT-wrapped connected commands are dispatched here")
        }
    }
}

/// Whether a connected subcommand writes flash and therefore must not be
/// cancelled in the middle of the write.
fn is_flash_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Config {
            command: ConfigCommand::Write { .. }
                | ConfigCommand::Set { .. }
                | ConfigCommand::Remap { .. }
        } | Command::Macro {
            command: MacroCommand::Write { .. } | MacroCommand::Restore { .. }
        }
    )
}

async fn dispatch_config_command(
    options: &BleOptions,
    command: ConfigCommand,
) -> Result<(), Error> {
    match command {
        ConfigCommand::Dump { raw, json } => {
            dump_config(options, raw.as_deref(), json.as_deref()).await?;
        }
        ConfigCommand::Write {
            file,
            backup_dir,
            accept_reserved_drift,
            force_raw,
            write_chunk,
            yes,
        } => {
            config_write(
                options,
                &file,
                backup_dir.as_deref(),
                accept_reserved_drift,
                force_raw,
                write_chunk,
                yes,
            )
            .await?;
        }
        ConfigCommand::Set {
            field,
            value,
            slot,
            backup_dir,
            write_chunk,
            yes,
        } => {
            let slot = parse_slot(slot)?;
            let set = parse_config_set(&field, &value, slot)?;
            config_set(options, &set, backup_dir.as_deref(), write_chunk, yes).await?;
        }
        ConfigCommand::Remap {
            from,
            to,
            slot,
            backup_dir,
            write_chunk,
            yes,
        } => {
            let slot = parse_slot(slot)?;
            let button = parse_physical_button(&from)?;
            let target = parse_remap_target(&to)?;
            config_remap(
                options,
                slot,
                button,
                target,
                backup_dir.as_deref(),
                write_chunk,
                yes,
            )
            .await?;
        }
        ConfigCommand::Map { command } => match command {
            MapCommand::Show { slot } => {
                let slot = parse_slot(slot)?;
                config_map_show(options, slot).await?;
            }
        },
        ConfigCommand::Profile { slot, yes } => {
            let slot = parse_slot(slot)?;
            config_profile_switch(options, slot, yes).await?;
        }
    }
    Ok(())
}

async fn dispatch_macro_command(options: &BleOptions, command: MacroCommand) -> Result<(), Error> {
    match command {
        MacroCommand::Backup {
            dir,
            poll_interval_ms,
            report_enable,
        } => {
            macro_backup(options, &dir, poll_interval_ms, report_enable).await?;
        }
        MacroCommand::Write {
            slot,
            index,
            file,
            trigger,
            backup_dir,
            chunk_size,
            write_total_relative,
            poll_interval_ms,
            report_enable,
        } => {
            macro_write(
                options,
                slot,
                index,
                &file,
                trigger.as_deref(),
                backup_dir.as_deref(),
                chunk_size,
                macro_write_layout(write_total_relative),
                poll_interval_ms,
                report_enable,
            )
            .await?;
        }
        MacroCommand::Restore {
            dir,
            force,
            chunk_size,
            write_total_relative,
            poll_interval_ms,
            report_enable,
        } => {
            macro_restore(
                options,
                &dir,
                force,
                chunk_size,
                macro_write_layout(write_total_relative),
                poll_interval_ms,
                report_enable,
            )
            .await?;
        }
    }
    Ok(())
}

async fn scan(timeout: Duration, check: bool) -> Result<ExitCode, Error> {
    let mut devices = scan_devices(timeout).await?;
    if check {
        let code = scan_check_exit_code(&devices);
        if code == 0 {
            if let Some(device) = devices.iter().find(|device| device.is_app) {
                println!(
                    "App Mode device found: {}",
                    device.name.as_deref().unwrap_or("<unnamed>")
                );
            }
        } else {
            eprintln!(
                "no App Mode (`{APP_BLE_NAME}`) device found within {} seconds",
                timeout.as_secs_f64()
            );
        }
        return Ok(ExitCode::from(code));
    }
    if devices.is_empty() {
        println!(
            "no BLE devices found within {} seconds",
            timeout.as_secs_f64()
        );
        return Ok(ExitCode::SUCCESS);
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
    Ok(ExitCode::SUCCESS)
}

/// Machine-readable result of `scan --check`.
///
/// Exit 0 when an App Mode (`80ND`) device is visible, 3 when none is. This is
/// the contract `scripts/session-loop.sh` relies on instead of grepping the
/// human-readable table.
fn scan_check_exit_code(devices: &[eightbitdo_core::BleDevice]) -> u8 {
    if devices.iter().any(|device| device.is_app) {
        0
    } else {
        EXIT_SCAN_NO_APP_MODE
    }
}

/// Best-effort timeout for teardown operations (`set_report_enable(false)` and
/// `disconnect`) so exiting commands cannot hang on a wedged BLE stack.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

async fn best_effort_disable_reports<T: Transport>(session: &Session<T>, timeout: Duration) {
    let _ = tokio::time::timeout(timeout, session.set_report_enable(false)).await;
}

async fn best_effort_disconnect<T: Transport>(transport: &T, timeout: Duration) {
    match tokio::time::timeout(timeout, transport.disconnect()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            // `BleTransport::disconnect` keeps the single-owner lock after a
            // partial teardown. The link may still be half-closed, so warn
            // instead of silently starting the next command.
            eprintln!(
                "warning: BLE teardown failed: {error}. The single-owner BLE lock is held \
                 until this process exits; wait for it to exit before scanning or \
                 connecting again."
            );
        }
        Err(_) => eprintln!(
            "warning: BLE disconnect did not finish within {timeout:?}. The single-owner BLE \
             lock is held until this process exits; wait for it to exit before scanning or \
             connecting again."
        ),
    }
}

/// Wait for SIGINT and resolve; the value itself is ignored.
async fn wait_for_sigint() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Result of racing a cancellable BLE command against SIGINT.
#[derive(Debug)]
enum SigintOutcome<T> {
    /// The command finished before a signal arrived.
    Finished(T),
    /// SIGINT arrived first; the command future was dropped.
    Interrupted,
}

/// Race a cancellable BLE-owning future against SIGINT.
///
/// Completion is preferred when both are ready: a command that finished at the
/// same moment as the signal keeps its result.
async fn with_sigint<T, F, S>(future: F, signal: S) -> SigintOutcome<T>
where
    F: Future<Output = T>,
    S: Future<Output = ()>,
{
    tokio::pin!(future);
    tokio::select! {
        biased;
        value = &mut future => SigintOutcome::Finished(value),
        _ = signal => SigintOutcome::Interrupted,
    }
}

/// Set when the first flash frame is about to be sent.
///
/// The top-level SIGINT wrapper uses this to split a flash command into a
/// cancellable prepare phase (connect, reads, pre-write backup, confirmation
/// prompt, plan) and a non-cancellable write phase. A single CLI command runs
/// per process; tests exercise the wrapper with an explicit reset.
static FLASH_WRITE_ACTIVE: AtomicBool = AtomicBool::new(false);

fn mark_flash_write_active() {
    FLASH_WRITE_ACTIVE.store(true, Ordering::SeqCst);
}

fn reset_flash_write_active() {
    FLASH_WRITE_ACTIVE.store(false, Ordering::SeqCst);
}

/// Result of racing a two-phase flash command against SIGINT.
#[derive(Debug)]
enum FlashSigintOutcome<T> {
    /// The command finished before any signal arrived.
    Finished(T),
    /// SIGINT arrived after the write started; the write was allowed to
    /// finish and its result is reported here.
    FinishedAfterInterrupt(T),
    /// SIGINT arrived during the cancellable prepare phase; the future was
    /// dropped before any flash frame was sent.
    Cancelled,
}

/// Race a two-phase flash command against SIGINT.
///
/// Before [`mark_flash_write_active`] the future is dropped on SIGINT (prompt,
/// pre-write backup and file reads are all cancellable). Once the first flash
/// frame is about to be sent the signal is handled once (the App Mode warning
/// is printed immediately) and the future is allowed to complete, so a
/// partially written sector/configuration is not abandoned.
async fn with_sigint_flash<T, F, S>(future: F, signal: S) -> FlashSigintOutcome<T>
where
    F: Future<Output = T>,
    S: Future<Output = ()>,
{
    with_sigint_flash_gate(future, signal, &FLASH_WRITE_ACTIVE).await
}

/// Inner form of [`with_sigint_flash`] with the write-active gate injected so
/// tests do not race the process-wide flag.
async fn with_sigint_flash_gate<T, F, S>(
    future: F,
    signal: S,
    write_active: &AtomicBool,
) -> FlashSigintOutcome<T>
where
    F: Future<Output = T>,
    S: Future<Output = ()>,
{
    tokio::pin!(future);
    tokio::pin!(signal);
    let mut signalled = false;
    loop {
        tokio::select! {
            biased;
            value = &mut future => {
                return if signalled {
                    FlashSigintOutcome::FinishedAfterInterrupt(value)
                } else {
                    FlashSigintOutcome::Finished(value)
                };
            }
            _ = &mut signal, if !signalled => {
                if write_active.load(Ordering::SeqCst) {
                    signalled = true;
                    warn_app_mode_requires_press(
                        "SIGINT received; the flash write has started and is not cancellable, \
                         so it will finish before the process exits",
                    );
                } else {
                    return FlashSigintOutcome::Cancelled;
                }
            }
        }
    }
}

/// Convert `--poll-interval-ms` into the session keepalive interval.
///
/// `0` disables the keepalive, which is the default for the macro commands:
/// the adjudicated official behaviour does not poll `0x0003` during a macro
/// write, so polling is an opt-in experiment.
fn poll_interval(ms: u64) -> Option<Duration> {
    (ms > 0).then(|| Duration::from_millis(ms))
}

/// Print keepalive counters under `--verbose`, and always warn when the
/// explicitly requested `0x0007` enable failed.
fn print_poll_stats<T: Transport>(options: &BleOptions, session: &Session<T>) {
    let enable_errors = session.report_enable_errors();
    if enable_errors > 0 {
        eprintln!(
            "warning: 0x0007 ReportEnable failed {enable_errors} time(s); the operation continued"
        );
    }
    // Show the poll line when polling ran, or when the enable was explicitly
    // requested (then it reports `0 sent, M errors`).
    if !options.verbose || (session.poll_interval().is_none() && !session.report_enable()) {
        return;
    }
    let errors = session.input_poll_errors();
    eprintln!(
        "input polls: {} sent, {} errors",
        session.input_polls_sent(),
        errors
    );
    if errors > 0 {
        eprintln!("warning: keepalive polling reported {errors} errors; the operation continued");
    }
}

/// Build the macro-command session from the two experimental keepalive flags.
fn macro_session<T: Transport>(
    transport: T,
    poll_interval_ms: u64,
    report_enable: bool,
) -> Session<T> {
    Session::new(transport)
        .with_poll_interval(poll_interval(poll_interval_ms))
        .with_report_enable(report_enable)
}

fn validate_output_paths(raw_path: Option<&Path>, json_path: Option<&Path>) -> Result<(), Error> {
    let (Some(raw), Some(json)) = (raw_path, json_path) else {
        return Ok(());
    };

    // Lexical comparison first: it also catches a real path against a
    // symlink that does not exist yet.
    if raw == json {
        return Err(conflicting_output_paths(raw, json));
    }

    let raw_key = OutputPathKey::new(raw)?;
    let json_key = OutputPathKey::new(json)?;
    // `real` keeps the final component unresolved, `spelled` is the lexical
    // absolute path, and `link_target` is where a dangling symlink points.
    // Any match means one path names the file the other one targets.
    let same_entry = raw_key.real == json_key.real
        || raw_key.spelled == json_key.spelled
        || raw_key.link_target.as_ref() == Some(&json_key.real)
        || raw_key.link_target.as_ref() == Some(&json_key.spelled)
        || json_key.link_target.as_ref() == Some(&raw_key.real)
        || json_key.link_target.as_ref() == Some(&raw_key.spelled);
    if same_entry {
        return Err(conflicting_output_paths(raw, json));
    }
    if let (Some(raw_meta), Some(json_meta)) = (&raw_key.existing, &json_key.existing) {
        if same_file(raw_meta, json_meta) {
            return Err(conflicting_output_paths(raw, json));
        }
    }
    Ok(())
}

fn conflicting_output_paths(raw: &Path, json: &Path) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "--raw ({}) and --json ({}) must not point to the same file",
            raw.display(),
            json.display()
        ),
    ))
}

/// Filesystem identity of an output path.
struct OutputPathKey {
    /// Path with the existing parent directory canonicalized.
    ///
    /// `.` / `..` components are resolved and symlinked directories are
    /// followed. The final component itself stays unresolved; symlinks there
    /// are handled by [`Self::spelled`], [`Self::link_target`] and
    /// [`Self::existing`].
    real: PathBuf,
    /// Lexically normalized absolute spelling of the final component.
    ///
    /// Keeping the unresolved spelling catches `link -> other` versus
    /// `other`, which canonicalization would hide.
    spelled: PathBuf,
    /// Target of a symlink at the final component that does not exist yet.
    link_target: Option<PathBuf>,
    /// Identity metadata when the path already exists.
    existing: Option<std::fs::Metadata>,
}

impl OutputPathKey {
    fn new(path: &Path) -> Result<Self, Error> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
        let canonical_parent = std::fs::canonicalize(parent).map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                format!(
                    "cannot resolve the directory of {}: {error}",
                    path.display()
                ),
            ))
        })?;
        let real = match absolute.file_name() {
            Some(name) => canonical_parent.join(name),
            None => canonical_parent,
        };
        // An existing symlink resolves to its target (which then also has
        // inode identity); a dangling one keeps its own name and exposes its
        // target separately.
        let link_target = dangling_link_target(real.clone(), 0);
        let real = if link_target.is_none() {
            std::fs::canonicalize(&real).unwrap_or(real)
        } else {
            real
        };
        Ok(Self {
            link_target,
            real,
            spelled: normalize_lexically(&absolute),
            existing: std::fs::symlink_metadata(&absolute).ok(),
        })
    }
}

/// Follow a symlink at `path` whose target does not exist.
///
/// Returns `None` when `path` is not a symlink. A relative target is resolved
/// against the link's canonical parent, so `dir/link -> ../missing` yields the
/// absolute `missing` path even when it does not exist yet.
fn dangling_link_target(path: PathBuf, depth: usize) -> Option<PathBuf> {
    if depth > 8 {
        return None;
    }
    let link_target = std::fs::read_link(&path).ok()?;
    let resolved = if link_target.is_absolute() {
        link_target
    } else {
        path.parent()?.join(link_target)
    };
    if std::fs::canonicalize(&resolved).is_ok() {
        // The link resolves to an existing file; inode identity covers it.
        return None;
    }
    let parent = resolved.parent().unwrap_or_else(|| Path::new("."));
    let canonical_parent = std::fs::canonicalize(parent).ok()?;
    let next = match resolved.file_name() {
        Some(name) => canonical_parent.join(name),
        None => canonical_parent,
    };
    Some(dangling_link_target(next.clone(), depth + 1).unwrap_or(next))
}

/// Resolve `.` / `..` without touching the filesystem or following symlinks.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True when both metadata entries describe the same file.
///
/// Unix compares `(device, inode)`, which also catches hard links whose paths
/// differ. Other platforms cannot tell hard links apart from paths alone:
/// Windows file identity (volume serial + file index) is not implemented here.
///
/// This is not a gap for the backup output check. A case-insensitive alias such
/// as `Backup.bin` versus `backup.bin` is caught by the post-write
/// revalidation: once the raw file exists, `OutputPathKey::real` canonicalizes
/// both spellings to the same path, so `validate_output_paths` rejects the
/// pair even though `same_file` returns false on Windows. Hard-link detection
/// stays Unix-only; on Windows the raw and JSON names would have to be
/// different links to the same file, which the operator would have to create
/// explicitly.
#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(not(unix))]
fn same_file(_a: &std::fs::Metadata, _b: &std::fs::Metadata) -> bool {
    false
}

/// Mode bits of the file that `rename` is about to replace, if it exists.
#[cfg(unix)]
fn replaced_file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions().mode() & 0o777)
}

/// Persist the rename itself. Best effort: a directory might not be
/// openable on every filesystem, and the data file is already fsynced.
#[cfg(unix)]
fn sync_parent_directory(parent: &Path) {
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) {}

/// Refuse to replace anything but a regular file.
///
/// `rename` replaces a symlink itself (not its target), and it will also
/// replace a FIFO, socket or device node that an operator aimed the output at
/// by mistake (`--raw link`, `--raw /dev/null`). Only a regular file or a path
/// that does not exist yet may be an output target.
fn ensure_regular_target(path: &Path) -> Result<(), Error> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(Error::Io(std::io::Error::new(
                error.kind(),
                format!("cannot inspect {}: {error}", path.display()),
            )))
        }
    };
    let file_type = metadata.file_type();
    // On Unix `is_file()` alone accepts FIFOs, sockets and device nodes, so
    // they are excluded explicitly. Windows has no `FileTypeExt` equivalent;
    // NUL and named pipes are not regular files and are rejected by
    // `is_file()` where metadata is available, and any remaining special
    // target makes the later `rename` fail with its own error.
    #[cfg(unix)]
    let regular = {
        use std::os::unix::fs::FileTypeExt as _;
        file_type.is_file()
            && !file_type.is_fifo()
            && !file_type.is_socket()
            && !file_type.is_block_device()
            && !file_type.is_char_device()
    };
    #[cfg(not(unix))]
    let regular = file_type.is_file();
    if regular {
        Ok(())
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{} is not a regular file (symlink, directory, or special file); refusing to replace it",
                path.display()
            ),
        )))
    }
}

/// Write `contents` through a temporary file in the same directory, then
/// rename it over `path`.
///
/// A crash or a failed write therefore leaves either the previous file or
/// the complete new one, never a truncated 2768-byte backup. The temporary
/// file starts at `0600` and adopts the replaced file's mode before the
/// rename, so rewriting an existing `0600` backup never widens it to `0644`.
fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), Error> {
    use std::io::Write as _;

    ensure_regular_target(path)?;
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let file_name = path
        .file_name()
        .ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} does not name a file", path.display()),
            ))
        })?
        .to_string_lossy()
        .into_owned();
    #[cfg(unix)]
    let replaced_mode = replaced_file_mode(path);

    let mut attempt = 0u32;
    let (temp_path, mut file) = loop {
        let candidate = parent.join(format!(
            ".{file_name}.eightbitdo-{}-{attempt}.tmp",
            std::process::id()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            // Backups contain personal settings. Create the temporary file
            // private even under a permissive umask; the mode is adjusted
            // below when an existing file is being replaced.
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < 16 => {
                attempt += 1;
            }
            Err(error) => {
                return Err(Error::Io(std::io::Error::new(
                    error.kind(),
                    format!(
                        "cannot create a temporary file next to {}: {error}",
                        path.display()
                    ),
                )))
            }
        }
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = replaced_mode.unwrap_or(0o600);
        // Best effort: FAT/exFAT, NFS mounts and some emulated platforms
        // accept the write but reject chmod. The file was created with
        // `mode(0o600)` above, so a failed adjustment leaves a new backup
        // private instead of failing the whole export. When an existing mode
        // cannot be preserved, the file stays at 0600: narrower than the
        // default but possibly wider than a replaced file whose mode was more
        // restrictive (0400). Failing the export would lose the backup, so
        // that trade-off is reported as a warning.
        if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(mode)) {
            eprintln!(
                "warning: cannot set the mode of {} to {mode:o}: {error}; continuing",
                path.display()
            );
        }
    }

    let result = file
        .write_all(contents)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&temp_path, path)
        });
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(Error::Io(std::io::Error::new(
            error.kind(),
            format!("cannot write {}: {error}", path.display()),
        )));
    }
    sync_parent_directory(parent);
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
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
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
    validate_output_paths(raw_path, json_path)?;
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let snapshot = session.read_config_snapshot().await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    let snapshot = snapshot?;

    let sha256 = write_config_outputs(&snapshot, raw_path, json_path)?;
    print_config_summary(&snapshot, &sha256);
    Ok(())
}

// --- configuration write / set ---------------------------------------------

/// `config set` field paths, for error messages and the command help.
const CONFIG_SET_FIELDS: &str = "cur_slot, gamepad_mode, profile_name, rumble.level, \
     joy.left_start, joy.left_end, joy.right_start, joy.right_end, \
     trigger.left_start, trigger.left_end, trigger.right_start, trigger.right_end, \
     gyro.enabled, gyro.trigger_mode, gyro.sensitivity, gyro.dead_compensate, gyro.mapping, \
     gyro.key, led.dynamic.color, led.dynamic.bg_color, led.dynamic.flag, \
     led.hotwheel.color, led.hotwheel.bg_color, led.hotwheel.speed, led.hotwheel.flag, \
     led.single.color, led.single.flag, map_key.flag";

/// One stick-record field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JoyField {
    LeftStart,
    LeftEnd,
    RightStart,
    RightEnd,
}

impl JoyField {
    fn apply(self, record: &mut JoyRecord, value: u8) {
        match self {
            Self::LeftStart => record.left_start_value = value,
            Self::LeftEnd => record.left_end_value = value,
            Self::RightStart => record.right_start_value = value,
            Self::RightEnd => record.right_end_value = value,
        }
    }
}

/// One trigger-record field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriggerField {
    LeftStart,
    LeftEnd,
    RightStart,
    RightEnd,
}

impl TriggerField {
    fn apply(self, record: &mut TriggerRecord, value: u8) {
        match self {
            Self::LeftStart => record.left_start_value = value,
            Self::LeftEnd => record.left_end_value = value,
            Self::RightStart => record.right_start_value = value,
            Self::RightEnd => record.right_end_value = value,
        }
    }
}

/// One six-axis / gyro field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GyroField {
    Enabled,
    TriggerMode,
    Sensitivity,
    DeadCompensate,
    Mapping,
}

impl GyroField {
    fn apply(self, record: &mut SixAxisRecord, value: u8) {
        match self {
            Self::Enabled => record.flag = u32::from(value),
            Self::TriggerMode => record.trigger_mode = value,
            Self::Sensitivity => record.sensitivity = value,
            Self::DeadCompensate => record.dead_compensate = value,
            Self::Mapping => record.mapping_type = value,
        }
    }
}

/// One RGB change. Color/flag changes enable the theme (`flag = 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedChange {
    DynamicColor(Argb),
    DynamicBg(Argb),
    DynamicFlag(u32),
    HotWheelColor(Argb),
    HotWheelBg(Argb),
    HotWheelSpeed(u8),
    HotWheelFlag(u32),
    SingleColor(Argb),
    SingleFlag(u32),
}

impl LedChange {
    fn edit(self, slot: Slot, config: &Bt2Config) -> ConfigEdit {
        match self {
            Self::DynamicColor(color) => {
                let mut theme = config.dynamic_theme(slot);
                theme.color = color;
                theme.flag = 1;
                ConfigEdit::DynamicTheme { slot, theme }
            }
            Self::DynamicBg(color) => {
                let mut theme = config.dynamic_theme(slot);
                theme.bg_color = color;
                theme.flag = 1;
                ConfigEdit::DynamicTheme { slot, theme }
            }
            Self::DynamicFlag(flag) => {
                let mut theme = config.dynamic_theme(slot);
                theme.flag = flag;
                ConfigEdit::DynamicTheme { slot, theme }
            }
            Self::HotWheelColor(color) => {
                let mut theme = config.hotwheel_theme(slot);
                theme.color = color;
                theme.flag = 1;
                ConfigEdit::HotWheelTheme { slot, theme }
            }
            Self::HotWheelBg(color) => {
                let mut theme = config.hotwheel_theme(slot);
                theme.bg_color = color;
                theme.flag = 1;
                ConfigEdit::HotWheelTheme { slot, theme }
            }
            Self::HotWheelSpeed(speed) => {
                let mut theme = config.hotwheel_theme(slot);
                theme.speed = speed;
                ConfigEdit::HotWheelTheme { slot, theme }
            }
            Self::HotWheelFlag(flag) => {
                let mut theme = config.hotwheel_theme(slot);
                theme.flag = flag;
                ConfigEdit::HotWheelTheme { slot, theme }
            }
            Self::SingleColor(color) => {
                let mut theme = config.single_theme(slot);
                theme.colors = [color; eightbitdo_protocol::config_bt2::SINGLE_LED_COUNT];
                theme.flag = 1;
                ConfigEdit::SingleTheme { slot, theme }
            }
            Self::SingleFlag(flag) => {
                let mut theme = config.single_theme(slot);
                theme.flag = flag;
                ConfigEdit::SingleTheme { slot, theme }
            }
        }
    }
}

/// A validated `config set <field> <value>` request.
///
/// Parsing happens before any device I/O so a bad field or value can never
/// touch the controller or create an empty pre-write backup directory.
#[derive(Debug, Clone, PartialEq)]
enum ConfigSet {
    CurSlot(Option<Slot>),
    GamepadMode(GamepadMode),
    ProfileName {
        slot: Slot,
        name: String,
    },
    RumbleLevel {
        slot: Slot,
        level: u8,
    },
    Joy {
        slot: Slot,
        field: JoyField,
        value: u8,
    },
    Trigger {
        slot: Slot,
        field: TriggerField,
        value: u8,
    },
    Gyro {
        slot: Slot,
        field: GyroField,
        value: u8,
    },
    GyroKeyMap {
        slot: Slot,
        key: KeyMap,
    },
    Led {
        slot: Slot,
        change: LedChange,
    },
    MapKeyFlag {
        slot: Slot,
        flag: u32,
    },
}

impl ConfigSet {
    /// Build the core write plan from the current device snapshot.
    ///
    /// Slot-scoped record edits preserve every other field of that record.
    /// Stick and trigger edits set the record's `flag` to `1` (custom
    /// settings enabled); gyro field edits preserve the existing flag.
    fn plan(&self, snapshot: &ConfigSnapshot) -> Result<ConfigWritePlan, Error> {
        let config = &snapshot.config;
        let edit = match self {
            Self::CurSlot(slot) => ConfigEdit::CurSlot(*slot),
            Self::GamepadMode(mode) => ConfigEdit::GamepadMode(*mode),
            Self::ProfileName { slot, name } => ConfigEdit::ProfileName {
                slot: *slot,
                name: name.clone(),
            },
            Self::RumbleLevel { slot, level } => {
                let record = RumbleRecord::from_level(*level).ok_or_else(|| {
                    invalid_input(format!("rumble.level must be 0..=5, got {level}"))
                })?;
                ConfigEdit::Rumble {
                    slot: *slot,
                    record,
                }
            }
            Self::Joy { slot, field, value } => {
                let mut record = config.joy(*slot);
                record.flag = 1;
                field.apply(&mut record, *value);
                ConfigEdit::Joy {
                    slot: *slot,
                    record,
                }
            }
            Self::Trigger { slot, field, value } => {
                let mut record = config.trigger(*slot);
                record.flag = 1;
                field.apply(&mut record, *value);
                ConfigEdit::Trigger {
                    slot: *slot,
                    record,
                }
            }
            Self::Gyro { slot, field, value } => {
                let mut record = config.six_axis(*slot);
                field.apply(&mut record, *value);
                ConfigEdit::SixAxis {
                    slot: *slot,
                    record,
                }
            }
            Self::GyroKeyMap { slot, key } => {
                let mut record = config.six_axis(*slot);
                record.key_map = *key;
                ConfigEdit::SixAxis {
                    slot: *slot,
                    record,
                }
            }
            Self::Led { slot, change } => change.edit(*slot, config),
            Self::MapKeyFlag { slot, flag } => ConfigEdit::MapKeyFlag {
                slot: *slot,
                flag: *flag,
            },
        };
        Ok(ConfigWritePlan::new().with(edit))
    }
}

/// Parse `config set <field> <value>`.
fn parse_config_set(field: &str, value: &str, slot: Slot) -> Result<ConfigSet, Error> {
    let name = field.trim().to_ascii_lowercase();
    let set = match name.as_str() {
        "cur_slot" => ConfigSet::CurSlot(parse_config_cur_slot(value)?),
        "gamepad_mode" => ConfigSet::GamepadMode(parse_gamepad_mode(value)?),
        "profile_name" => ConfigSet::ProfileName {
            slot,
            name: value.to_string(),
        },
        "rumble.level" => ConfigSet::RumbleLevel {
            slot,
            level: parse_range_u8(value, &name, 0, 5)?,
        },
        "joy.left_start" => ConfigSet::Joy {
            slot,
            field: JoyField::LeftStart,
            value: parse_range_u8(value, &name, 0, 127)?,
        },
        "joy.left_end" => ConfigSet::Joy {
            slot,
            field: JoyField::LeftEnd,
            value: parse_range_u8(value, &name, 0, 128)?,
        },
        "joy.right_start" => ConfigSet::Joy {
            slot,
            field: JoyField::RightStart,
            value: parse_range_u8(value, &name, 0, 127)?,
        },
        "joy.right_end" => ConfigSet::Joy {
            slot,
            field: JoyField::RightEnd,
            value: parse_range_u8(value, &name, 0, 128)?,
        },
        "trigger.left_start" => ConfigSet::Trigger {
            slot,
            field: TriggerField::LeftStart,
            value: parse_u8(value, &name)?,
        },
        "trigger.left_end" => ConfigSet::Trigger {
            slot,
            field: TriggerField::LeftEnd,
            value: parse_u8(value, &name)?,
        },
        "trigger.right_start" => ConfigSet::Trigger {
            slot,
            field: TriggerField::RightStart,
            value: parse_u8(value, &name)?,
        },
        "trigger.right_end" => ConfigSet::Trigger {
            slot,
            field: TriggerField::RightEnd,
            value: parse_u8(value, &name)?,
        },
        "gyro.enabled" => ConfigSet::Gyro {
            slot,
            field: GyroField::Enabled,
            value: u8::from(parse_on_off(value)?),
        },
        "gyro.trigger_mode" => ConfigSet::Gyro {
            slot,
            field: GyroField::TriggerMode,
            value: parse_gyro_trigger_mode(value)?,
        },
        "gyro.sensitivity" => ConfigSet::Gyro {
            slot,
            field: GyroField::Sensitivity,
            value: parse_range_u8(value, &name, 1, 15)?,
        },
        "gyro.dead_compensate" => ConfigSet::Gyro {
            slot,
            field: GyroField::DeadCompensate,
            value: parse_u8(value, &name)?,
        },
        "gyro.mapping" => ConfigSet::Gyro {
            slot,
            field: GyroField::Mapping,
            value: parse_gyro_mapping(value)?,
        },
        // gyro.key uses the same documented single-KeyMap whitelist as remap:
        // KeyMap_My_NUll (227), unknown bits and combinations are rejected.
        "gyro.key" => ConfigSet::GyroKeyMap {
            slot,
            key: parse_key_map_value(value)?,
        },
        "led.dynamic.color" => ConfigSet::Led {
            slot,
            change: LedChange::DynamicColor(parse_argb(value, &name)?),
        },
        "led.dynamic.bg_color" => ConfigSet::Led {
            slot,
            change: LedChange::DynamicBg(parse_argb(value, &name)?),
        },
        "led.dynamic.flag" => ConfigSet::Led {
            slot,
            change: LedChange::DynamicFlag(parse_flag(value, &name)?),
        },
        "led.hotwheel.color" => ConfigSet::Led {
            slot,
            change: LedChange::HotWheelColor(parse_argb(value, &name)?),
        },
        "led.hotwheel.bg_color" => ConfigSet::Led {
            slot,
            change: LedChange::HotWheelBg(parse_argb(value, &name)?),
        },
        "led.hotwheel.speed" => ConfigSet::Led {
            slot,
            change: LedChange::HotWheelSpeed(parse_range_u8(value, &name, 0, 15)?),
        },
        "led.hotwheel.flag" => ConfigSet::Led {
            slot,
            change: LedChange::HotWheelFlag(parse_flag(value, &name)?),
        },
        "led.single.color" => ConfigSet::Led {
            slot,
            change: LedChange::SingleColor(parse_argb(value, &name)?),
        },
        "led.single.flag" => ConfigSet::Led {
            slot,
            change: LedChange::SingleFlag(parse_flag(value, &name)?),
        },
        "map_key.flag" | "mapkey.flag" => ConfigSet::MapKeyFlag {
            slot,
            flag: parse_flag(value, &name)?,
        },
        other => {
            return Err(invalid_input(format!(
                "unknown config field {other:?}; supported fields: {CONFIG_SET_FIELDS}"
            )))
        }
    };
    Ok(set)
}

fn parse_config_cur_slot(value: &str) -> Result<Option<Slot>, Error> {
    let text = value.trim().to_ascii_lowercase();
    if text == "none" || text == "4" {
        return Ok(None);
    }
    let index: usize = text
        .parse()
        .map_err(|_| invalid_input(format!("cur_slot must be 0..=2 or none/4, got {value:?}")))?;
    let slot = Slot::from_index(index)
        .ok_or_else(|| invalid_input(format!("cur_slot must be 0..=2 or none/4, got {value:?}")))?;
    Ok(Some(slot))
}

fn parse_gamepad_mode(value: &str) -> Result<GamepadMode, Error> {
    let text = value.trim();
    if let Ok(raw) = text.parse::<u16>() {
        return GamepadMode::try_from(raw).map_err(|_| {
            invalid_input(format!(
                "gamepad_mode must be 0..=5 or a mode name, got {value:?}"
            ))
        });
    }
    match text.to_ascii_lowercase().as_str() {
        "switch" => Ok(GamepadMode::Switch),
        "jm" | "gamepad_jm" | "gamepad" => Ok(GamepadMode::GamepadJM),
        "mac" | "apple" => Ok(GamepadMode::Mac),
        "xinput" | "xinput_m" => Ok(GamepadMode::XInput),
        "extended" | "extend" => Ok(GamepadMode::Extended),
        "unselected" | "none" => Ok(GamepadMode::Unselected),
        other => Err(invalid_input(format!(
            "unknown gamepad_mode {other:?}; use switch, jm, mac, xinput, extended, \
             unselected, or 0..=5"
        ))),
    }
}

fn parse_gyro_trigger_mode(value: &str) -> Result<u8, Error> {
    match value.trim().to_ascii_lowercase().as_str() {
        "hold" => Ok(0),
        "toggle" => Ok(1),
        _ => {
            let raw = parse_u8(value, "gyro.trigger_mode")?;
            if raw > 1 {
                return Err(invalid_input(format!(
                    "gyro.trigger_mode must be hold/toggle or 0/1, got {value:?}"
                )));
            }
            Ok(raw)
        }
    }
}

fn parse_gyro_mapping(value: &str) -> Result<u8, Error> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" | "left_stick" => Ok(0),
        "right" | "right_stick" => Ok(1),
        "mouse" => Ok(2),
        _ => {
            let raw = parse_u8(value, "gyro.mapping")?;
            if raw > 2 {
                return Err(invalid_input(format!(
                    "gyro.mapping must be left/right/mouse or 0..=2, got {value:?}"
                )));
            }
            Ok(raw)
        }
    }
}

fn parse_on_off(value: &str) -> Result<bool, Error> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" | "enabled" => Ok(true),
        "0" | "off" | "false" | "no" | "disabled" => Ok(false),
        other => Err(invalid_input(format!(
            "expected 0/1 or on/off, got {other:?}"
        ))),
    }
}

fn parse_u8(value: &str, field: &str) -> Result<u8, Error> {
    let raw = parse_u32(value, field)?;
    u8::try_from(raw)
        .map_err(|_| invalid_input(format!("{field} must fit in 0..=255, got {value:?}")))
}

/// Parse a decimal/0x integer and enforce an inclusive range.
fn parse_range_u8(value: &str, field: &str, min: u8, max: u8) -> Result<u8, Error> {
    let parsed = parse_u8(value, field)?;
    if !(min..=max).contains(&parsed) {
        return Err(invalid_input(format!(
            "{field} must be {min}..={max}, got {parsed}"
        )));
    }
    Ok(parsed)
}

/// Parse a 0/1 flag value (led.*.flag, map_key.flag).
fn parse_flag(value: &str, field: &str) -> Result<u32, Error> {
    let parsed = parse_u32(value, field)?;
    if parsed > 1 {
        return Err(invalid_input(format!(
            "{field} must be 0 or 1, got {parsed}"
        )));
    }
    Ok(parsed)
}

fn parse_u32(value: &str, field: &str) -> Result<u32, Error> {
    let text = value.trim();
    let parsed = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else {
        text.parse::<u32>()
    };
    parsed.map_err(|_| {
        invalid_input(format!(
            "{field} must be a decimal or 0x-prefixed integer, got {value:?}"
        ))
    })
}

fn parse_argb(value: &str, field: &str) -> Result<Argb, Error> {
    let text = value.trim();
    let hex = text
        .strip_prefix('#')
        .or_else(|| text.strip_prefix("0x"))
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    let valid = matches!(hex.len(), 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    if !valid {
        return Err(invalid_input(format!(
            "{field} must be 0xAARRGGBB or #RRGGBB, got {value:?}"
        )));
    }
    let raw = u32::from_str_radix(hex, 16).map_err(|_| {
        invalid_input(format!(
            "{field} must be 0xAARRGGBB or #RRGGBB, got {value:?}"
        ))
    })?;
    Ok(Argb(if hex.len() == 6 {
        0xFF00_0000 | raw
    } else {
        raw
    }))
}

/// Parse a physical button name or wire index (`0..=21`).
///
/// The index order is the RE-confirmed non-3E `MapKeyRecord` order:
/// `A, B, X, Y, L1, R1, L2, R2, L3, R3, Select, Start, Share, Logo, Up, Down,
/// Left, Right, P1/PL, P2/PR, P3/L4, P4/R4`.
fn parse_physical_button(value: &str) -> Result<ButtonIndex, Error> {
    let text = value.trim();
    if let Ok(index) = text.parse::<usize>() {
        return ButtonIndex::from_index(index)
            .ok_or_else(|| invalid_input(format!("button index must be 0..=21, got {value:?}")));
    }
    let upper = text.to_ascii_uppercase().replace(['-', ' '], "_");
    let button = match upper.as_str() {
        "A" => ButtonIndex::A,
        "B" => ButtonIndex::B,
        "X" => ButtonIndex::X,
        "Y" => ButtonIndex::Y,
        "L1" | "LB" => ButtonIndex::L1,
        "R1" | "RB" => ButtonIndex::R1,
        "L2" | "LT" => ButtonIndex::L2,
        "R2" | "RT" => ButtonIndex::R2,
        "L3" | "LS" => ButtonIndex::L3,
        "R3" | "RS" => ButtonIndex::R3,
        "SELECT" | "VIEW" | "MINUS" => ButtonIndex::Select,
        "START" | "MENU" | "PLUS" => ButtonIndex::Start,
        "SHARE" | "STAR" | "SCREENSHOT" => ButtonIndex::Share,
        "LOGO" | "HOME" => ButtonIndex::Logo,
        "UP" | "DPAD_UP" => ButtonIndex::Up,
        "DOWN" | "DPAD_DOWN" => ButtonIndex::Down,
        "LEFT" | "DPAD_LEFT" => ButtonIndex::Left,
        "RIGHT" | "DPAD_RIGHT" => ButtonIndex::Right,
        "P1" | "PL" => ButtonIndex::P1,
        "P2" | "PR" => ButtonIndex::P2,
        "P3" | "L4" => ButtonIndex::P3,
        "P4" | "R4" => ButtonIndex::P4,
        _ => {
            return Err(invalid_input(format!(
                "unknown physical button {value:?}; use A, B, X, Y, L1..R3, Select, Start, \
                 Share, Logo, Up, Down, Left, Right, P1/PL, P2/PR, P3/L4, P4/R4, or 0..=21"
            )))
        }
    };
    Ok(button)
}

/// Parse and validate a documented single `KeyMap` value.
///
/// Accepts `null`/`disable`, a KeyMap name, M1..M4, `0xHEX` or decimal. The
/// whitelist lives in the protocol crate ([`KeyMap::is_single_documented`],
/// derived from the same table as [`KeyMap::name`]); `KeyMap_My_NUll` (227),
/// unknown bits and combinations are rejected. Used by both `config remap`
/// and `config set gyro.key`.
fn parse_key_map_value(value: &str) -> Result<KeyMap, Error> {
    let text = value.trim();
    let lower = text.to_ascii_lowercase();
    let key = if matches!(
        lower.as_str(),
        "null" | "disable" | "disabled" | "unassigned"
    ) {
        KeyMap::NONE
    } else {
        parse_trigger(text).map_err(|_| {
            invalid_input(format!(
                "unknown KeyMap value {value:?}; use a KeyMap name, M1..M4, null/disable, \
                 0xHEX or decimal"
            ))
        })?
    };
    if !key.is_single_documented() {
        return Err(invalid_input(format!(
            "KeyMap value {value:?} (0x{:08X}) is not a documented single value; use one KeyMap \
             constant (0 = unassigned). KeyMap_My_NUll (227), unknown bits and combinations are \
             rejected.",
            key.bits()
        )));
    }
    Ok(key)
}

/// Parse a `config remap` target.
fn parse_remap_target(value: &str) -> Result<KeyMap, Error> {
    parse_key_map_value(value)
}

fn key_label(key: KeyMap) -> String {
    key.name()
        .map_or_else(|| format!("0x{:08X}", key.bits()), str::to_string)
}

fn button_label(button: ButtonIndex) -> &'static str {
    match button {
        ButtonIndex::P1 => "P1/PL",
        ButtonIndex::P2 => "P2/PR",
        ButtonIndex::P3 => "P3/L4",
        ButtonIndex::P4 => "P4/R4",
        other => other.label(),
    }
}

/// One line per index plus a header, used by `config map show` and its tests.
fn mapping_lines(config: &Bt2Config, slot: Slot) -> Vec<String> {
    let record = config.map_key(slot);
    let mut lines = Vec::with_capacity(24);
    let flag_display = match record.flag {
        0 => "0 (typed 0 = disabled; a raw dump may store other values)".to_string(),
        1 => "1 (typed 1 = enabled; a raw dump may store other values)".to_string(),
        observed => format!("0x{observed:08X} (observed on hardware; meaning unconfirmed/opaque)"),
    };
    lines.push(format!(
        "slot {} (profile {}): map_key flag {flag_display}",
        slot.index(),
        slot.number()
    ));
    lines.push("index  physical   key".to_string());
    let mappings = button_mappings(config, slot);
    for (index, button) in ButtonIndex::ALL.into_iter().enumerate() {
        let key = mappings[index];
        lines.push(format!(
            "{index:>5}  {:<10} {} (0x{:08X})",
            button_label(button),
            key_label(key),
            key.bits()
        ));
    }
    lines
}

async fn config_remap_session<T: Transport>(
    session: &Session<T>,
    slot: Slot,
    button: ButtonIndex,
    target: KeyMap,
    backup_dir: Option<&Path>,
    confirmation: ConfigConfirmation<'_>,
    write_chunk: Option<u16>,
) -> Result<(), Error> {
    let snapshot = session.read_config_snapshot().await?;
    let before = button_mapping(&snapshot.config, slot, button);
    let prepared = ConfigWritePlan::new()
        .with(ConfigEdit::ButtonRemap {
            slot,
            button,
            to: target,
        })
        .prepare(&snapshot)?;
    println!(
        "slot {} (profile {}) button {} ({}): {} (0x{:08X}) -> {} (0x{:08X})",
        slot.index(),
        slot.number(),
        button_label(button),
        button.label(),
        key_label(before),
        before.bits(),
        key_label(target),
        target.bits()
    );
    println!(
        "map_key record flag: {} (the official single-entry remap does not change it; \
         use `config set map_key.flag 1` to enable the record)",
        map_key_flag(&snapshot.config, slot)
    );
    apply_config_prepared(
        session,
        &snapshot,
        &prepared,
        backup_dir,
        confirmation,
        write_chunk,
    )
    .await
}

async fn config_remap(
    options: &BleOptions,
    slot: Slot,
    button: ButtonIndex,
    target: KeyMap,
    backup_dir: Option<&Path>,
    write_chunk: Option<u16>,
    assume_yes: bool,
) -> Result<(), Error> {
    validate_write_chunk_preconnect(write_chunk)?;
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let mut confirmation_io = TopLevelConfirmation::new(assume_yes);
    let confirmation = confirmation_io.confirmation();
    let result = config_remap_session(
        &session,
        slot,
        button,
        target,
        backup_dir,
        confirmation,
        write_chunk,
    )
    .await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    result
}

async fn config_map_show(options: &BleOptions, slot: Slot) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let snapshot = session.read_config_snapshot().await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    let snapshot = snapshot?;
    for line in mapping_lines(&snapshot.config, slot) {
        println!("{line}");
    }
    Ok(())
}

/// Switch the active profile (`cmd 0x0015`).
///
/// The wire is RE-confirmed (`docs/PROTOCOL_SPEC.md` §6.2) but the ACK shape,
/// the need for a Save and the persistence of the selection are **unconfirmed
/// on hardware**. This sends exactly the official request and reports what the
/// controller acknowledged; it deliberately does not claim the switch persisted.
async fn config_profile_switch(
    options: &BleOptions,
    slot: Slot,
    assume_yes: bool,
) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let mut confirmation_io = TopLevelConfirmation::new(assume_yes);
    let mut confirmation = confirmation_io.confirmation();
    if !confirmation.confirm_with(PROFILE_CONFIRM_PROMPT).await? {
        best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
        // Answering "no" is not a failure: match the other write commands,
        // which report the abort and exit successfully.
        println!("aborted; no profile switch was sent");
        return Ok(());
    }
    let result = session.switch_profile(slot).await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    if let Err(error) = result {
        if matches!(error, Error::Timeout { .. }) {
            eprintln!(
                "warning: no WriteSLOT acknowledgement arrived. The 0x0015 ACK shape is \
                 unconfirmed, so the switch may still have happened; check the active profile \
                 on the controller."
            );
        }
        return Err(error);
    }
    println!(
        "sent WriteSLOT slot {} (profile {}) (cmd 0x0015, no Save); acknowledged but unverified",
        slot.index(),
        slot.index() + 1
    );
    println!(
        "note: ACK shape, Save requirement and persistence are unconfirmed on hardware; \
         no read-back is claimed"
    );
    Ok(())
}

/// Read a 2768-byte configuration file.
fn read_config_file(path: &Path) -> Result<Bt2Config, Error> {
    ensure_regular_file(path, "configuration file")?;
    let bytes = std::fs::read(path)?;
    if bytes.len() != CONFIG_SIZE {
        return Err(invalid_data(format!(
            "configuration file {} is {} bytes; expected exactly {CONFIG_SIZE}",
            path.display(),
            bytes.len()
        )));
    }
    Bt2Config::from_bytes(&bytes)
        .map_err(|error| invalid_data(format!("{}: {error}", path.display())))
}

fn print_changed_regions(regions: &[ConfigRegion]) {
    println!("changed regions: {}", regions.len());
    for region in regions.iter().take(32) {
        println!(
            "  0x{:04X}..0x{:04X} ({} bytes)",
            region.offset(),
            region.end(),
            region.len()
        );
    }
    if regions.len() > 32 {
        println!("  ... and {} more", regions.len() - 32);
    }
}

fn config_readback_crc_label(crc: ConfigCrcStatus) -> &'static str {
    if crc.matches_sent() {
        "device kept the sent CRC"
    } else if crc.from_sent() {
        "device recomputed the CRC over the sent buffer"
    } else {
        "device stored the in-place recomputation"
    }
}

/// How a configuration write gets its confirmation.
enum ConfigConfirmation<'a> {
    /// `--yes` was passed: write without prompting.
    AssumeYes,
    /// Prompt on stderr and read one line through this synchronous callback.
    /// Kept for the existing unit tests; production top-level commands use
    /// [`ConfigConfirmation::AsyncReader`] so SIGINT can cancel the prompt.
    #[cfg_attr(not(test), allow(dead_code))]
    Reader(&'a mut (dyn FnMut() -> Result<bool, Error> + Send)),
    /// Prompt and read one line from the session REPL's async reader so the
    /// operator answers on the same input stream. `LineState` keeps the REPL's
    /// 4096-byte limit and NUL/CR handling identical to command lines.
    AsyncReader {
        /// The session REPL input.
        reader: &'a mut (dyn tokio::io::AsyncBufRead + Unpin + Send),
        /// Cancellation-safe line state shared with the REPL loop.
        line_state: &'a mut LineState,
    },
}

impl ConfigConfirmation<'_> {
    async fn confirm(&mut self) -> Result<bool, Error> {
        self.confirm_with(CONFIG_CONFIRM_PROMPT).await
    }

    /// Confirm using an operation-specific prompt.
    async fn confirm_with(&mut self, prompt: &str) -> Result<bool, Error> {
        match self {
            Self::AssumeYes => Ok(true),
            Self::Reader(read) => read(),
            Self::AsyncReader { reader, line_state } => {
                confirm_config_write_async_with(*reader, line_state, prompt).await
            }
        }
    }
}

/// The confirmation prompt shared by the sync and async readers.
const CONFIG_CONFIRM_PROMPT: &str = "Type 'yes' to write the 2768-byte configuration: ";

/// Confirmation prompt for the profile switch (`0x0015`), which does **not**
/// write the 2768-byte configuration and therefore must not reuse the config
/// write wording.
const PROFILE_CONFIRM_PROMPT: &str =
    "Type 'yes' to switch the controller's active profile (cmd 0x0015, no Save): ";

/// Top-level `yes/no` confirmation that is assembled lazily.
///
/// The async stdin reader must outlive the `ConfigConfirmation` borrow, so it
/// lives in this struct and [`TopLevelConfirmation::confirmation`] borrows it.
/// Confirming through async I/O (instead of a blocking `std::io::stdin` read)
/// lets SIGINT cancel the top-level prompt before any flash frame is sent.
struct TopLevelConfirmation {
    assume_yes: bool,
    stdin: tokio::io::BufReader<tokio::io::Stdin>,
    line_state: LineState,
}

impl TopLevelConfirmation {
    fn new(assume_yes: bool) -> Self {
        Self {
            assume_yes,
            stdin: tokio::io::BufReader::new(tokio::io::stdin()),
            line_state: LineState::default(),
        }
    }

    fn confirmation(&mut self) -> ConfigConfirmation<'_> {
        if self.assume_yes {
            ConfigConfirmation::AssumeYes
        } else {
            ConfigConfirmation::AsyncReader {
                reader: &mut self.stdin,
                line_state: &mut self.line_state,
            }
        }
    }
}

/// Ask for confirmation unless `--yes` was passed (synchronous test helper).
#[cfg_attr(not(test), allow(dead_code))]
fn confirm_config_write(reader: &mut impl std::io::BufRead) -> Result<bool, Error> {
    eprint!("{CONFIG_CONFIRM_PROMPT}");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    reader.read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}

/// Async form used by the session REPL.
///
/// It reuses the REPL's `read_session_line` so the confirmation has the same
/// 4096-byte limit, NUL cut and CRLF handling as a command line.
async fn confirm_config_write_async(
    reader: &mut (dyn tokio::io::AsyncBufRead + Unpin + Send),
    line_state: &mut LineState,
) -> Result<bool, Error> {
    confirm_config_write_async_with(reader, line_state, CONFIG_CONFIRM_PROMPT).await
}

/// Async confirmation with an explicit prompt, used by the profile switch.
async fn confirm_config_write_async_with(
    reader: &mut (dyn tokio::io::AsyncBufRead + Unpin + Send),
    line_state: &mut LineState,
    prompt: &str,
) -> Result<bool, Error> {
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    match read_session_line(reader, line_state).await? {
        LineRead::Line { text, cut } => {
            if cut {
                eprintln!(
                    "note: confirmation line cut at an embedded NUL byte; treating the prefix as \
                     the answer"
                );
            }
            Ok(text.trim().eq_ignore_ascii_case("yes"))
        }
        LineRead::TooLong { discarded } => {
            eprintln!(
                "confirmation line too long: discarded {discarded} bytes \
                 (limit {MAX_SESSION_LINE_BYTES}); aborted"
            );
            Ok(false)
        }
        LineRead::Eof => Ok(false),
    }
}

/// Reserved regions the official host does not interpret.
const RESERVED_RANGES: [(usize, usize); 2] =
    [(OFF_LEGACY_A, OFF_MACROS), (OFF_LEGACY_B, OFF_SIX_AXIS)];

/// The 216-byte-per-slot macro header region inside the 2768-byte block.
const MACRO_HEADER_RANGE: std::ops::Range<usize> = OFF_MACROS..OFF_LEGACY_B;

/// Copy the device's current reserved bytes over an edited buffer.
///
/// Used by `config write --accept-reserved-drift` after the device normalized
/// reserved bytes between the backup and the restore.
fn copy_device_reserved(current: &Bt2Config, edited: &mut Bt2Config) {
    for (start, end) in RESERVED_RANGES {
        edited.as_mut_bytes()[start..end].copy_from_slice(&current.as_bytes()[start..end]);
    }
}

/// Largest `--write-chunk` value accepted before connecting.
///
/// BLE ATT payloads above this are only possible with a negotiated MTU above
/// the observed 200; the exact negotiated budget is enforced after connecting.
const MAX_PRECONNECT_WRITE_CHUNK: u16 = 180;

/// Validate `--write-chunk` before the BLE connection is opened.
fn validate_write_chunk_preconnect(chunk: Option<u16>) -> Result<(), Error> {
    let Some(chunk) = chunk else {
        return Ok(());
    };
    if !(1..=MAX_PRECONNECT_WRITE_CHUNK).contains(&chunk) {
        return Err(Error::InvalidRequest(format!(
            "--write-chunk must be 1..={MAX_PRECONNECT_WRITE_CHUNK}, got {chunk} \
             (the negotiated MTU payload is enforced after connecting)"
        )));
    }
    Ok(())
}

/// Non-blocking warnings for a raw configuration file.
///
/// `cur_slot` has firmware-observed values outside the documented enum
/// (`6`), so unknown values are preserved with a warning instead of being
/// rejected. Everything the official writers interpret is validated in
/// `validate_raw_config_for_write`.
fn raw_config_warnings(config: &Bt2Config) -> Vec<String> {
    let mut warnings = Vec::new();
    let cur_slot = config.cur_slot_raw();
    if !matches!(cur_slot, 0 | 1 | 2 | 4) {
        warnings.push(format!(
            "cur_slot {cur_slot} is not a documented value (0..=2 or 4); preserving it as-is"
        ));
    }
    warnings
}

/// True for key-map values acceptable in a raw dump.
///
/// The documented single values plus the erased `0xFFFFFFFF` sentinel observed
/// on unused hardware slots (all 22 keys of slot 2 in the real dump). The
/// typed `config remap` / `config set gyro.key` paths stay stricter and reject
/// `0xFFFFFFFF`.
/// Flag values observed in the real hardware dump (plus the documented 0/1).
///
/// Unknown values are not accepted silently: they become a raw-validation
/// problem and therefore require `--force-raw` (which prints them as warnings).
fn raw_flag_is_observed(flag: u32) -> bool {
    matches!(flag, 0 | 1 | 0x2020_0911 | 0xFFFF_FFFF)
}

/// `gyro` additionally stores `0x20190000` on the real slot 0.
fn raw_gyro_flag_is_observed(flag: u32) -> bool {
    raw_flag_is_observed(flag) || flag == 0x2019_0000
}

fn push_unobserved_flag(problems: &mut Vec<String>, field: &str, flag: u32, gyro: bool) {
    let observed = if gyro {
        "0, 1, 0x20200911, 0xFFFFFFFF, 0x20190000 (gyro)"
    } else {
        "0, 1, 0x20200911, 0xFFFFFFFF"
    };
    problems.push(format!(
        "{field}.flag 0x{flag:08X} is not an observed hardware value ({observed}); \
         pass --force-raw only to restore an unknown device state"
    ));
}

/// The real fully-erased slot state.
///
/// The hardware slot 2 combines all of these at once: joy flag `0xFFFFFFFF`
/// with all four values 255, rumble flag `0xFFFFFFFF` with both multipliers
/// NaN, and map_key flag `0xFFFFFFFF` with all 22 entries `0xFFFFFFFF`. The
/// numeric-range / sentinel exceptions apply only to this complete state, not
/// to one record independently.
fn raw_erased_slot_state(config: &Bt2Config, slot: Slot) -> bool {
    let joy = config.joy(slot);
    let rumble = config.rumble(slot);
    let map_key = config.map_key(slot);
    config.slot_flag(slot) == u32::MAX
        && joy.flag == u32::MAX
        && [
            joy.left_start_value,
            joy.left_end_value,
            joy.right_start_value,
            joy.right_end_value,
        ]
        .iter()
        .all(|value| *value == u8::MAX)
        && rumble.flag == u32::MAX
        && rumble.left.is_nan()
        && rumble.right.is_nan()
        && map_key.flag == u32::MAX
        && map_key.keys.iter().all(|key| key.bits() == u32::MAX)
}

/// Validate the known fields of a complete raw configuration file.
///
/// Every field the official writers interpret must pass the same checks the
/// typed `config set` / `config remap` paths use, so a hand-edited file cannot
/// bypass them: 22 `MapKeyRecord` entries, `gyro.key_map`, record flags, macro
/// header counts / step counts / trigger key maps. Record flags accept only
/// the values observed on hardware (plus the documented 0/1); anything else
/// needs `--force-raw`. Numeric ranges are always checked for non-erased
/// records regardless of the opaque flag value; the observed fully-erased slot
/// (flag `0xFFFFFFFF`, joy all-255, rumble NaN, 22 erased key-map entries) is
/// accepted as that hardware state.
fn raw_config_problems(config: &Bt2Config) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    if config.gamepad_mode().is_none() {
        problems.push(format!(
            "gamepad_mode {} is not a known 0..=5 value",
            config.gamepad_mode_raw()
        ));
    }
    for slot in Slot::ALL {
        let slot_flag = config.slot_flag(slot);
        if !matches!(slot_flag, 0 | 0x2020_0911 | 0xFFFF_FFFF) {
            problems.push(format!(
                "slot_flags[{}] 0x{slot_flag:08X} is not an observed hardware value \
                 (0, 0x20200911, 0xFFFFFFFF); pass --force-raw only to restore an unknown device \
                 state",
                slot.index()
            ));
        }
        let erased_slot_state = raw_erased_slot_state(config, slot);
        let joy = config.joy(slot);
        if !raw_flag_is_observed(joy.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("joy[{}]", slot.index()),
                joy.flag,
                false,
            );
        }
        if !erased_slot_state {
            for (name, value, max) in [
                ("left_start_value", joy.left_start_value, 127u8),
                ("left_end_value", joy.left_end_value, 128),
                ("right_start_value", joy.right_start_value, 127),
                ("right_end_value", joy.right_end_value, 128),
            ] {
                if value > max {
                    problems.push(format!(
                        "joy[{}].{name} {value} exceeds {max}",
                        slot.index()
                    ));
                }
            }
        }

        let trigger = config.trigger(slot);
        if !raw_flag_is_observed(trigger.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("trigger[{}]", slot.index()),
                trigger.flag,
                false,
            );
        }

        let gyro = config.six_axis(slot);
        // Hardware stores opaque values such as 0x20200911 / 0x20190000 /
        // 0xFFFFFFFF; accept only the observed set and leave 0/1 semantics to
        // the typed `config set` paths.
        if !raw_gyro_flag_is_observed(gyro.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("gyro[{}]", slot.index()),
                gyro.flag,
                true,
            );
        }
        {
            if !(1..=15).contains(&gyro.sensitivity) {
                problems.push(format!(
                    "gyro[{}].sensitivity {} is outside 1..=15",
                    slot.index(),
                    gyro.sensitivity
                ));
            }
            if gyro.trigger_mode > 1 {
                problems.push(format!(
                    "gyro[{}].trigger_mode {} is outside 0..=1",
                    slot.index(),
                    gyro.trigger_mode
                ));
            }
            if gyro.mapping_type > 2 {
                problems.push(format!(
                    "gyro[{}].mapping_type {} is outside 0..=2",
                    slot.index(),
                    gyro.mapping_type
                ));
            }
        }
        if !gyro.key_map.is_single_documented() {
            problems.push(format!(
                "gyro[{}].key_map 0x{:08X} is not a documented single KeyMap value",
                slot.index(),
                gyro.key_map.bits()
            ));
        }

        let map_key = config.map_key(slot);
        if !raw_flag_is_observed(map_key.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("map_key[{}]", slot.index()),
                map_key.flag,
                false,
            );
        }
        for (button, key) in ButtonIndex::ALL.into_iter().zip(map_key.keys) {
            let acceptable =
                key.is_single_documented() || (key.bits() == u32::MAX && erased_slot_state);
            if !acceptable {
                problems.push(format!(
                    "button mapping slot {} index {} ({}): 0x{:08X} is not a documented single \
                     KeyMap value (0xFFFFFFFF is only accepted when all 22 entries of the slot \
                     are erased)",
                    slot.index(),
                    button.index(),
                    button.label(),
                    key.bits()
                ));
            }
        }

        let hotwheel = config.hotwheel_theme(slot);
        if !raw_flag_is_observed(hotwheel.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("hotwheel[{}]", slot.index()),
                hotwheel.flag,
                false,
            );
        }
        if hotwheel.speed > 15 {
            problems.push(format!(
                "hotwheel[{}].speed {} is outside 0..=15",
                slot.index(),
                hotwheel.speed
            ));
        }
        let rumble = config.rumble(slot);
        if !raw_flag_is_observed(rumble.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("rumble[{}]", slot.index()),
                rumble.flag,
                false,
            );
        }
        if !erased_slot_state {
            for (name, value) in [("left", rumble.left), ("right", rumble.right)] {
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    problems.push(format!(
                        "rumble[{}].{name} {value} is not a finite 0.0..=1.0 multiplier",
                        slot.index()
                    ));
                }
            }
        }

        let dynamic = config.dynamic_theme(slot);
        if !raw_flag_is_observed(dynamic.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("dynamic[{}]", slot.index()),
                dynamic.flag,
                false,
            );
        }
        let single = config.single_theme(slot);
        if !raw_flag_is_observed(single.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("single[{}]", slot.index()),
                single.flag,
                false,
            );
        }
        let special = config.special(slot);
        if !raw_flag_is_observed(special.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("special[{}]", slot.index()),
                special.flag,
                false,
            );
        }

        let macro_slot = config.macro_slot(slot);
        if !raw_flag_is_observed(macro_slot.flag) {
            push_unobserved_flag(
                &mut problems,
                &format!("macro[{}]", slot.index()),
                macro_slot.flag,
                false,
            );
        }
        if usize::from(macro_slot.total_cnt) > MACROS_PER_SLOT {
            problems.push(format!(
                "macro[{}].total_cnt {} exceeds {MACROS_PER_SLOT}",
                slot.index(),
                macro_slot.total_cnt
            ));
        }
        for (index, record) in macro_slot.records.iter().enumerate() {
            if !is_skipped_steps(record.max_steps) && !(1..=200).contains(&record.max_steps) {
                problems.push(format!(
                    "macro[{}].record[{index}].max_steps {} is not 0/0xFFFF/1..=200",
                    slot.index(),
                    record.max_steps
                ));
            }
            // M2: a macro trigger may be a combined KeyMap (the
            // `macro write --trigger` parser accepts any KeyMap value), so the
            // raw path must not require a single documented value here.
        }
    }
    problems
}

/// Validate a raw configuration file, refusing on any problem.
///
/// Test helper; the write path uses [`validate_raw_config_for_write`] so
/// `--force-raw` can downgrade the same problems to warnings.
#[cfg(test)]
fn validate_raw_config(config: &Bt2Config) -> Result<(), Error> {
    let problems = raw_config_problems(config);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(invalid_data(format!(
            "configuration file has invalid known fields; refusing to write:\n  - {}",
            problems.join("\n  - ")
        )))
    }
}

/// Validate a raw configuration file for an actual write.
///
/// With `force_raw`, undocumented / out-of-range values are warnings instead
/// of errors so a backup the device produced can be restored. The reserved
/// region guard is separate and is never bypassed by this flag.
fn validate_raw_config_for_write(config: &Bt2Config, force_raw: bool) -> Result<(), Error> {
    let problems = raw_config_problems(config);
    if problems.is_empty() {
        return Ok(());
    }
    if force_raw {
        eprintln!(
            "warning: --force-raw is writing {} undocumented / out-of-range field(s):",
            problems.len()
        );
        for problem in &problems {
            eprintln!("  - {problem}");
        }
        eprintln!(
            "warning: --force-raw keeps the reserved-region guard; review the values before saving"
        );
        Ok(())
    } else {
        Err(invalid_data(format!(
            "configuration file has invalid known fields; refusing to write (use --force-raw only \
             to restore an undocumented device backup):\n  - {}",
            problems.join("\n  - ")
        )))
    }
}

/// Warn when a write changes the macro header region.
fn warn_if_macro_header_changes(changed: &[ConfigRegion]) {
    let touches_macro_header = changed.iter().any(|region| {
        region.offset() < MACRO_HEADER_RANGE.end && region.end() > MACRO_HEADER_RANGE.start
    });
    if touches_macro_header {
        eprintln!(
            "warning: this write changes macro header bytes 0x{:03X}..0x{:03X}. Macro step data \
             (the 4 KB flash sectors) is not part of the 2768-byte configuration and is not backed \
             up or rewritten here; non-empty macros may change behavior.",
            MACRO_HEADER_RANGE.start, MACRO_HEADER_RANGE.end
        );
    }
}

/// Warn when a write changes the connection mode or the active slot.
fn warn_if_mode_or_slot_changes(snapshot: &ConfigSnapshot, expected: &Bt2Config) {
    if snapshot.config.gamepad_mode_raw() != expected.gamepad_mode_raw() {
        eprintln!(
            "warning: gamepad_mode changes from {} to {}; the BLE configuration link may restart. \
             If the write fails, reconnect and re-run.",
            snapshot.config.gamepad_mode_raw(),
            expected.gamepad_mode_raw()
        );
    }
    if snapshot.config.cur_slot_raw() != expected.cur_slot_raw() {
        eprintln!(
            "warning: cur_slot changes from {} to {}; the controller may switch profiles or \
             restart the BLE configuration link.",
            snapshot.config.cur_slot_raw(),
            expected.cur_slot_raw()
        );
    }
}

/// A configuration write whose mandatory pre-write backup already exists.
#[derive(Debug)]
struct PreparedSessionConfigWrite {
    snapshot: ConfigSnapshot,
    prepared: PreparedConfigWrite,
    backup: PathBuf,
    recover: String,
    write_chunk: Option<u16>,
}

/// Print the diff, validate the chunk override, and write the mandatory
/// pre-write backup.
///
/// Returns `None` for a no-op plan. The caller may release the operation lock
/// after this returns and prompt the operator without stopping the keepalive.
async fn prepare_config_prepared<T: Transport>(
    session: &Session<T>,
    snapshot: &ConfigSnapshot,
    prepared: &PreparedConfigWrite,
    backup_dir: Option<&Path>,
    write_chunk: Option<u16>,
) -> Result<Option<PreparedSessionConfigWrite>, Error> {
    let before_sha = sha256_hex(snapshot.config.as_bytes());
    println!("config size: {CONFIG_SIZE} bytes");
    println!("before sha256: {before_sha}");
    // Validate an explicit chunk before the no-op check, the backup and the
    // prompt so a bad value is always reported (even for a no-op plan).
    if let Some(chunk) = write_chunk {
        let limit = max_protocol_payload(session.transport().mtu());
        if chunk == 0 || usize::from(chunk) > limit {
            return Err(Error::InvalidRequest(format!(
                "--write-chunk {chunk} must be 1..={limit} for MTU {}",
                session.transport().mtu()
            )));
        }
    }
    if prepared.is_noop() {
        println!("no changes; nothing was written");
        return Ok(None);
    }
    println!(
        "after  sha256 (planned): {}",
        sha256_hex(prepared.expected_bytes())
    );
    print_changed_regions(prepared.changed());
    warn_if_macro_header_changes(prepared.changed());
    warn_if_mode_or_slot_changes(snapshot, prepared.expected());

    let backup = backup_dir.map_or_else(|| default_backup_dir("config-backup"), Path::to_path_buf);
    prepare_output_directory(&backup)?;
    let backup_sha = write_config_outputs(
        snapshot,
        Some(&backup.join("config.bin")),
        Some(&backup.join("config.json")),
    )?;
    let backup_file = backup.join("config.bin");
    let recover = format!(
        "eightbitdo config write --file {file} --yes (inside the session REPL: config write --file {file} --yes)",
        file = backup_file.display()
    );
    eprintln!(
        "pre-write backup: {} (config.bin sha256 {backup_sha})",
        backup.display()
    );
    eprintln!(
        "note: config writes do not touch macro step sectors (0x0103/0x0104); the backup covers \
         the 2768-byte configuration only"
    );
    eprintln!(
        "recover with: {recover} (add --accept-reserved-drift if the device normalized reserved \
         bytes; add --force-raw only when the backup contains undocumented fields)"
    );

    Ok(Some(PreparedSessionConfigWrite {
        snapshot: snapshot.clone(),
        prepared: prepared.clone(),
        backup,
        recover,
        write_chunk,
    }))
}

/// Re-read the configuration (TOCTOU), then write + Save + verify it.
///
/// This covers the non-cancellable flash section; the caller must not release
/// the operation lock while it runs.
async fn finish_config_prepared<T: Transport>(
    session: &Session<T>,
    state: &PreparedSessionConfigWrite,
) -> Result<(), Error> {
    // TOCTOU: the operator may have changed the profile (or another host may
    // have written) while the prompt was open. Re-read and refuse rather than
    // overwriting a state the prepared buffer was not based on.
    let current = session.read_config_snapshot().await?;
    if current.config != state.snapshot.config {
        return Err(Error::InvalidRequest(format!(
            "configuration changed while waiting for confirmation (before sha256 {}, now {}); \
             aborted without sending a write. The pre-write backup at {} is the plan-time snapshot \
             taken before the prompt (not the current device state). Re-run the command.",
            sha256_hex(state.snapshot.config.as_bytes()),
            sha256_hex(current.config.as_bytes()),
            state.backup.display()
        )));
    }

    let default_chunk = config_write_chunk_len(session.transport().mtu())?;
    let effective_chunk = state.write_chunk.map_or(default_chunk, usize::from);
    eprintln!("write chunk: {effective_chunk} bytes (official 74; override with --write-chunk)");
    eprintln!(
        "writing 2768 bytes + Save; do not interrupt (Ctrl-C or power loss can leave the commit \
         unconfirmed)"
    );
    mark_flash_write_active();
    let outcome = match session
        .write_full_config_with_chunk(
            &state.snapshot,
            &state.prepared,
            state.write_chunk.map(usize::from),
            ConfigWriteTimeouts::default(),
        )
        .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!(
                "config write failed; pre-write backup: {}",
                state.backup.display()
            );
            eprintln!(
                "recover with: {} (add --accept-reserved-drift if the device normalized reserved \
                 bytes; add --force-raw only when the backup contains undocumented fields)",
                state.recover
            );
            return Err(error);
        }
    };

    println!(
        "wrote configuration: {} packets, {} changed regions",
        outcome.packets_sent,
        outcome.changed.len()
    );
    println!(
        "read-back sha256: {}",
        sha256_hex(outcome.read_back.as_bytes())
    );
    println!(
        "crc: sent=0x{:04X} stored=0x{:04X} in_place=0x{:04X} ({})",
        outcome.crc.expected,
        outcome.crc.stored,
        outcome.crc.recomputed,
        config_readback_crc_label(outcome.crc)
    );
    println!("pre-write backup: {}", state.backup.display());
    println!("configuration written and verified");
    Ok(())
}

/// Prepare, confirm, then write one configuration (top-level flow).
///
/// The pre-write backup is written before the prompt and before any frame is
/// sent. After the prompt the configuration is re-read; if it changed while
/// the operator was deciding, the write is aborted without sending
/// `0x0001`/`0x0006`.
async fn apply_config_prepared<T: Transport>(
    session: &Session<T>,
    snapshot: &ConfigSnapshot,
    prepared: &PreparedConfigWrite,
    backup_dir: Option<&Path>,
    mut confirmation: ConfigConfirmation<'_>,
    write_chunk: Option<u16>,
) -> Result<(), Error> {
    let Some(state) =
        prepare_config_prepared(session, snapshot, prepared, backup_dir, write_chunk).await?
    else {
        return Ok(());
    };
    if !confirmation.confirm().await? {
        println!("aborted; no configuration was written");
        return Ok(());
    }
    finish_config_prepared(session, &state).await
}

async fn config_write_session<T: Transport>(
    session: &Session<T>,
    edited: &Bt2Config,
    backup_dir: Option<&Path>,
    accept_reserved_drift: bool,
    confirmation: ConfigConfirmation<'_>,
    write_chunk: Option<u16>,
) -> Result<(), Error> {
    let snapshot = session.read_config_snapshot().await?;
    let mut edited = edited.clone();
    if accept_reserved_drift {
        copy_device_reserved(&snapshot.config, &mut edited);
        eprintln!("--accept-reserved-drift: using the device's current reserved bytes");
    }
    let prepared = PreparedConfigWrite::from_buffer(&snapshot, edited)?;
    apply_config_prepared(
        session,
        &snapshot,
        &prepared,
        backup_dir,
        confirmation,
        write_chunk,
    )
    .await
}

async fn config_set_session<T: Transport>(
    session: &Session<T>,
    set: &ConfigSet,
    backup_dir: Option<&Path>,
    confirmation: ConfigConfirmation<'_>,
    write_chunk: Option<u16>,
) -> Result<(), Error> {
    let snapshot = session.read_config_snapshot().await?;
    let plan = set.plan(&snapshot)?;
    let prepared = plan.prepare(&snapshot)?;
    apply_config_prepared(
        session,
        &snapshot,
        &prepared,
        backup_dir,
        confirmation,
        write_chunk,
    )
    .await
}

/// Read and validate a raw configuration file for a write.
///
/// Shared by the top-level `config write` and the session REPL so both apply
/// the same `--force-raw` downgrade and warnings.
fn read_and_validate_config_file(path: &Path, force_raw: bool) -> Result<Bt2Config, Error> {
    let edited = read_config_file(path)?;
    validate_raw_config_for_write(&edited, force_raw)?;
    for warning in raw_config_warnings(&edited) {
        eprintln!("warning: {warning}");
    }
    Ok(edited)
}

/// Confirmation source for a REPL config write.
///
/// `--yes` skips the prompt; otherwise the REPL's async reader is used. A
/// non-REPL caller without a reader must pass `--yes` (the top-level command
/// has its own synchronous stdin reader).
fn config_confirmation_for_session<'a>(
    yes: bool,
    reader: &'a mut Option<&mut (dyn tokio::io::AsyncBufRead + Unpin + Send)>,
    line_state: &'a mut Option<&mut LineState>,
) -> Result<ConfigConfirmation<'a>, Error> {
    match (yes, reader.as_deref_mut(), line_state.as_deref_mut()) {
        (true, _, _) => Ok(ConfigConfirmation::AssumeYes),
        (false, Some(reader), Some(line_state)) => {
            Ok(ConfigConfirmation::AsyncReader { reader, line_state })
        }
        (false, _, _) => Err(Error::InvalidRequest(
            "interactive confirmation is only available in the session REPL; pass --yes"
                .to_string(),
        )),
    }
}

async fn run_session_config_write<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    edited: &Bt2Config,
    backup_dir: Option<&Path>,
    accept_reserved_drift: bool,
    mut confirmation: ConfigConfirmation<'_>,
    write_chunk: Option<u16>,
) -> Result<(), Error> {
    // Lock only for the snapshot read + pre-write backup; the operator is
    // prompted after the lock is released so the 20 ms keepalive can continue
    // while they decide.
    let state = {
        let guard = session.lock().await;
        let snapshot = guard.read_config_snapshot().await?;
        let mut edited = edited.clone();
        if accept_reserved_drift {
            copy_device_reserved(&snapshot.config, &mut edited);
            eprintln!("--accept-reserved-drift: using the device's current reserved bytes");
        }
        let prepared = PreparedConfigWrite::from_buffer(&snapshot, edited)?;
        prepare_config_prepared(&guard, &snapshot, &prepared, backup_dir, write_chunk).await?
    };
    let Some(state) = state else {
        return Ok(());
    };
    if !confirmation.confirm().await? {
        println!("aborted; no configuration was written");
        return Ok(());
    }
    // Re-lock for the non-cancellable flash section (TOCTOU + write + Save +
    // read-back). The keepalive cannot interleave a 0x0003 here.
    let guard = session.lock().await;
    finish_config_prepared(&guard, &state).await
}

async fn run_session_config_set<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    set: &ConfigSet,
    backup_dir: Option<&Path>,
    mut confirmation: ConfigConfirmation<'_>,
    write_chunk: Option<u16>,
) -> Result<(), Error> {
    let state = {
        let guard = session.lock().await;
        let snapshot = guard.read_config_snapshot().await?;
        let plan = set.plan(&snapshot)?;
        let prepared = plan.prepare(&snapshot)?;
        prepare_config_prepared(&guard, &snapshot, &prepared, backup_dir, write_chunk).await?
    };
    let Some(state) = state else {
        return Ok(());
    };
    if !confirmation.confirm().await? {
        println!("aborted; no configuration was written");
        return Ok(());
    }
    let guard = session.lock().await;
    finish_config_prepared(&guard, &state).await
}

async fn run_session_config_remap<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    slot: Slot,
    button: ButtonIndex,
    target: KeyMap,
    backup_dir: Option<&Path>,
    mut confirmation: ConfigConfirmation<'_>,
    write_chunk: Option<u16>,
) -> Result<(), Error> {
    let state = {
        let guard = session.lock().await;
        let snapshot = guard.read_config_snapshot().await?;
        let prepared = ConfigWritePlan::new()
            .with(ConfigEdit::ButtonRemap {
                slot,
                button,
                to: target,
            })
            .prepare(&snapshot)?;
        prepare_config_prepared(&guard, &snapshot, &prepared, backup_dir, write_chunk).await?
    };
    let Some(state) = state else {
        return Ok(());
    };
    if !confirmation.confirm().await? {
        println!("aborted; no configuration was written");
        return Ok(());
    }
    let guard = session.lock().await;
    finish_config_prepared(&guard, &state).await
}

/// A REPL config write whose preparation (snapshot, plan, backup) is done.
struct PendingReplConfigWrite {
    state: PreparedSessionConfigWrite,
    needs_confirmation: bool,
}

/// Phase 1 of a REPL config write: parse, validate and prepare (including the
/// pre-write backup) while holding the operation lock only for that work.
///
/// The caller runs this in a Ctrl-C-cancellable select; the prompt and the
/// flash write are separate phases.
async fn prepare_repl_config_command<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    command: &SessionCommand,
) -> Result<Option<PendingReplConfigWrite>, Error> {
    match command {
        SessionCommand::ConfigSet {
            field,
            value,
            slot,
            backup_dir,
            yes,
            write_chunk,
        } => {
            let slot = parse_slot(*slot)?;
            let set = parse_config_set(field, value, slot)?;
            validate_write_chunk_preconnect(*write_chunk)?;
            let state = {
                let guard = session.lock().await;
                let snapshot = guard.read_config_snapshot().await?;
                let plan = set.plan(&snapshot)?;
                let prepared = plan.prepare(&snapshot)?;
                prepare_config_prepared(
                    &guard,
                    &snapshot,
                    &prepared,
                    backup_dir.as_deref(),
                    *write_chunk,
                )
                .await?
            };
            Ok(state.map(|state| PendingReplConfigWrite {
                state,
                needs_confirmation: !*yes,
            }))
        }
        SessionCommand::ConfigRemap {
            from,
            to,
            slot,
            backup_dir,
            yes,
            write_chunk,
        } => {
            let slot = parse_slot(*slot)?;
            let button = parse_physical_button(from)?;
            let target = parse_remap_target(to)?;
            validate_write_chunk_preconnect(*write_chunk)?;
            let state = {
                let guard = session.lock().await;
                let snapshot = guard.read_config_snapshot().await?;
                let prepared = ConfigWritePlan::new()
                    .with(ConfigEdit::ButtonRemap {
                        slot,
                        button,
                        to: target,
                    })
                    .prepare(&snapshot)?;
                prepare_config_prepared(
                    &guard,
                    &snapshot,
                    &prepared,
                    backup_dir.as_deref(),
                    *write_chunk,
                )
                .await?
            };
            Ok(state.map(|state| PendingReplConfigWrite {
                state,
                needs_confirmation: !*yes,
            }))
        }
        SessionCommand::ConfigWrite {
            file,
            backup_dir,
            accept_reserved_drift,
            force_raw,
            yes,
            write_chunk,
        } => {
            validate_write_chunk_preconnect(*write_chunk)?;
            let edited = read_and_validate_config_file(file, *force_raw)?;
            let state = {
                let guard = session.lock().await;
                let snapshot = guard.read_config_snapshot().await?;
                let mut edited = edited.clone();
                if *accept_reserved_drift {
                    copy_device_reserved(&snapshot.config, &mut edited);
                    eprintln!("--accept-reserved-drift: using the device's current reserved bytes");
                }
                let prepared = PreparedConfigWrite::from_buffer(&snapshot, edited)?;
                prepare_config_prepared(
                    &guard,
                    &snapshot,
                    &prepared,
                    backup_dir.as_deref(),
                    *write_chunk,
                )
                .await?
            };
            Ok(state.map(|state| PendingReplConfigWrite {
                state,
                needs_confirmation: !*yes,
            }))
        }
        _ => Err(Error::InvalidRequest(
            "not a config write command".to_string(),
        )),
    }
}

/// Phase 3 of a REPL config write: the non-cancellable TOCTOU + write + Save +
/// read-back section (caller keeps the command out of the Ctrl-C branch).
async fn finish_repl_config_command<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    state: &PreparedSessionConfigWrite,
) -> Result<(), Error> {
    let guard = session.lock().await;
    finish_config_prepared(&guard, state).await
}

async fn config_write(
    options: &BleOptions,
    file: &Path,
    backup_dir: Option<&Path>,
    accept_reserved_drift: bool,
    force_raw: bool,
    write_chunk: Option<u16>,
    assume_yes: bool,
) -> Result<(), Error> {
    // Read and validate the file (and the chunk override) before opening the
    // BLE connection: an invalid path, length, field or chunk must not enter
    // configuration mode at all.
    validate_write_chunk_preconnect(write_chunk)?;
    let edited = read_and_validate_config_file(file, force_raw)?;
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let mut confirmation_io = TopLevelConfirmation::new(assume_yes);
    let confirmation = confirmation_io.confirmation();
    let result = config_write_session(
        &session,
        &edited,
        backup_dir,
        accept_reserved_drift,
        confirmation,
        write_chunk,
    )
    .await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    result
}

async fn config_set(
    options: &BleOptions,
    set: &ConfigSet,
    backup_dir: Option<&Path>,
    write_chunk: Option<u16>,
    assume_yes: bool,
) -> Result<(), Error> {
    validate_write_chunk_preconnect(write_chunk)?;
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let mut confirmation_io = TopLevelConfirmation::new(assume_yes);
    let confirmation = confirmation_io.confirmation();
    let result = config_set_session(&session, set, backup_dir, confirmation, write_chunk).await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    result
}

/// Write the raw configuration, then re-check that the JSON path is still a
/// different file.
///
/// [`validate_output_paths`] runs once before the backup, when neither file
/// exists. On a case-insensitive filesystem (APFS, NTFS) `Backup.bin` and
/// `backup.bin` therefore look distinct until the raw file exists. Once it
/// does, canonicalization / metadata resolve both names to the same entry and
/// this second check rejects the pair before the JSON rename can replace the
/// 2768-byte raw backup.
///
/// This recheck is also what catches a case-insensitive alias on Windows,
/// where [`same_file`] is always false (file identity is Unix-only): the
/// canonical path and `symlink_metadata` of the existing raw file still see the
/// alias.
///
/// It is a consistency check for accidental aliases, not a security boundary:
/// an attacker who can write to the output directory can still race the check
/// (TOCTOU / concurrent rename) before the JSON file is written.
fn write_raw_and_revalidate(raw: &Path, json: Option<&Path>, bytes: &[u8]) -> Result<(), Error> {
    write_raw_and_revalidate_with(raw, json, bytes, || {})
}

/// Inner form with a test seam between the write and the recheck.
///
/// Linux cannot create a case-insensitive alias, so the test uses
/// `before_recheck` to link the JSON path to the just-written raw file: the
/// alias exists only after the raw write, which is the same window the second
/// check exists for.
fn write_raw_and_revalidate_with(
    raw: &Path,
    json: Option<&Path>,
    bytes: &[u8],
    before_recheck: impl FnOnce(),
) -> Result<(), Error> {
    write_atomic(raw, bytes)?;
    before_recheck();
    validate_output_paths(Some(raw), json)?;
    // Only report success once the recheck above has accepted the pair; on a
    // rejected alias the raw file exists, but the requested backup as a whole
    // did not complete.
    println!(
        "wrote raw configuration: {} ({CONFIG_SIZE} bytes)",
        raw.display()
    );
    Ok(())
}

/// Write the optional raw/JSON files and return the SHA-256 hex digest.
fn write_config_outputs(
    snapshot: &ConfigSnapshot,
    raw_path: Option<&Path>,
    json_path: Option<&Path>,
) -> Result<String, Error> {
    validate_output_paths(raw_path, json_path)?;
    let sha256 = sha256_hex(snapshot.config.as_bytes());

    if let Some(path) = raw_path {
        write_raw_and_revalidate(path, json_path, snapshot.config.as_bytes())?;
    }

    if let Some(path) = json_path {
        let summary = json_summary(snapshot, &sha256);
        let encoded = serde_json::to_string_pretty(&summary).map_err(std::io::Error::other)?;
        write_atomic(path, format!("{encoded}\n").as_bytes())?;
        println!("wrote JSON summary: {}", path.display());
    }

    Ok(sha256)
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
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
) -> Result<ExitCode, Error> {
    validate_output_paths(raw_path, json_path)?;
    let transport = match with_sigint(BleTransport::connect(options), wait_for_sigint()).await {
        SigintOutcome::Finished(Ok(transport)) => transport,
        SigintOutcome::Finished(Err(error)) => return Err(error),
        SigintOutcome::Interrupted => {
            warn_sigint_disconnects();
            return Ok(ExitCode::from(EXIT_INTERRUPTED));
        }
    };
    let session = Session::new(transport);
    // Race every read (and the optional hold) against SIGINT. `backup_session`
    // keeps its shutdown parameter for tests, but production passes a pending
    // future: this outer race owns the signal so a Ctrl-C during the reads
    // cannot be swallowed.
    let outcome = with_sigint(
        backup_session(
            &session,
            raw_path,
            json_path,
            hold,
            std::future::pending::<()>(),
        ),
        wait_for_sigint(),
    )
    .await;
    best_effort_disable_reports(&session, CLEANUP_TIMEOUT).await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    match outcome {
        SigintOutcome::Finished(result) => {
            result?;
            Ok(ExitCode::SUCCESS)
        }
        SigintOutcome::Interrupted => {
            warn_sigint_disconnects();
            Ok(ExitCode::from(EXIT_INTERRUPTED))
        }
    }
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
    validate_output_paths(raw_path, json_path)?;
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

/// One JSON/TOML entry for one macro in a backup directory.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MacroBackupDocumentEntry {
    /// Slot index (`0..=2`), matching `Macro_Pro3_Struct` order.
    slot: u8,
    /// Human-facing profile number (`1..=3`).
    profile: u8,
    /// Macro index (`0..=3`), matching `MacroFunPro3Record` order.
    index: u8,
    /// Display id (`M1`..`M4`).
    id: String,
    /// Stored UTF-16BE name decoded to text.
    name: String,
    /// `true` when `max_steps` is `0` or the `0xFFFF` unset marker.
    skipped: bool,
    /// Why the flash read was skipped (`None` when read).
    skip_reason: Option<String>,
    /// Raw `max_steps` value from the configuration header.
    max_steps: u16,
    /// Trigger key mask from the configuration header.
    key_map: u32,
    /// Name of the trigger key when it is a known single value.
    key_map_name: Option<String>,
    /// Loop count (`0` = hold / infinite).
    cycles_num: u32,
    /// Delay between loops in milliseconds.
    interval_ms: u32,
    /// Relative file name of the raw step bytes (`None` when skipped).
    raw_file: Option<String>,
    /// SHA-256 of the raw step bytes (`None` when skipped).
    raw_sha256: Option<String>,
    /// Raw `offset` values in the `0x0102` response headers, one per chunk.
    ///
    /// `0` after the first chunk means the sequential-chunk convention; the
    /// requested offset means the device echoed it. This is diagnostic data
    /// for confirming real-device behaviour.
    response_offsets: Vec<u32>,
    /// Decoded 10-byte steps for human editing (raw `.bin` is canonical).
    steps: Vec<MacroStep>,
}

/// Machine-readable manifest written as both `macros.json` and `macros.toml`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MacroBackupDocument {
    /// Manifest format version.
    format: u32,
    /// Relative file name of the 2768-byte configuration snapshot.
    config_file: String,
    /// SHA-256 of the 2768-byte configuration snapshot.
    config_sha256: String,
    /// Raw `gamepad_mode` at `+0x010`; the low 8 bits form the macro `subcmd`.
    gamepad_mode: u16,
    /// Always 3.
    slot_count: usize,
    /// Always 4.
    macros_per_slot: usize,
    /// All 12 entries in slot-major order.
    macros: Vec<MacroBackupDocumentEntry>,
}

/// Reject a path that is not a regular file before any blocking read.
///
/// A FIFO, socket or directory passed to `read_to_string` / `read` can block
/// indefinitely (or fail late). During a `session` macro write the preparation
/// phase is not cancellable by Ctrl-C, so a FIFO would make the REPL hang
/// with no way to interrupt it. `metadata` follows symlinks: a symlink to a
/// regular file is accepted, a symlink to a FIFO/directory is not.
fn ensure_regular_file(path: &Path, what: &str) -> Result<(), Error> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(invalid_input(format!(
            "{what} {} is not a regular file; FIFOs, sockets and directories are refused",
            path.display()
        )));
    }
    Ok(())
}

/// Read JSON or TOML macro input.
fn read_macro_file(path: &Path) -> Result<MacroFile, Error> {
    ensure_regular_file(path, "macro file")?;
    let text = std::fs::read_to_string(path)?;
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    let parsed: MacroFile = match extension.as_deref() {
        Some("toml") => toml::from_str(&text)
            .map_err(|error| invalid_data(format!("{}: invalid TOML: {error}", path.display())))?,
        Some("json") => serde_json::from_str(&text)
            .map_err(|error| invalid_data(format!("{}: invalid JSON: {error}", path.display())))?,
        _ => serde_json::from_str(&text).or_else(|json_error| {
            toml::from_str(&text).map_err(|toml_error| {
                invalid_data(format!(
                    "{}: not valid JSON ({json_error}) or TOML ({toml_error})",
                    path.display()
                ))
            })
        })?,
    };
    if parsed.steps.is_empty() {
        return Err(invalid_input(
            "macro file has no steps; deleting a macro is not implemented",
        ));
    }
    if parsed.steps.len() > MAX_MACRO_STEPS {
        return Err(invalid_input(format!(
            "macro file has {} steps; maximum is {MAX_MACRO_STEPS}",
            parsed.steps.len()
        )));
    }
    Ok(parsed)
}

fn invalid_input(message: impl Into<String>) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

fn invalid_data(message: impl Into<String>) -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    ))
}

/// Parse `--trigger`: M1..M4, a common KeyMap name, `0xHEX` or decimal.
fn parse_trigger(text: &str) -> Result<KeyMap, Error> {
    let trimmed = text.trim();
    let upper = trimmed.to_ascii_uppercase();
    let named = match upper.as_str() {
        "NONE" => Some(KeyMap::NONE),
        "M1" => Some(KeyMap::M1),
        "M2" => Some(KeyMap::M2),
        "M3" => Some(KeyMap::M3),
        "M4" => Some(KeyMap::M4),
        "A" => Some(KeyMap::A),
        "B" => Some(KeyMap::B),
        "X" => Some(KeyMap::X),
        "Y" => Some(KeyMap::Y),
        "START" => Some(KeyMap::START),
        "SELECT" => Some(KeyMap::SELECT),
        "HOME" => Some(KeyMap::HOME),
        "L1" => Some(KeyMap::L1),
        "L2" => Some(KeyMap::L2),
        "R1" => Some(KeyMap::R1),
        "R2" => Some(KeyMap::R2),
        "P1" => Some(KeyMap::P1),
        "P2" => Some(KeyMap::P2),
        "P3" => Some(KeyMap::P3),
        "P4" => Some(KeyMap::P4),
        "UP" => Some(KeyMap::UP),
        "DOWN" => Some(KeyMap::DOWN),
        "LEFT" => Some(KeyMap::LEFT),
        "RIGHT" => Some(KeyMap::RIGHT),
        _ => None,
    };
    if let Some(key) = named {
        return Ok(key);
    }
    let parsed = if let Some(hex) = upper.strip_prefix("0X") {
        u32::from_str_radix(hex, 16)
    } else {
        upper.parse::<u32>()
    };
    parsed.map(KeyMap::from_bits).map_err(|_| {
        invalid_input(format!(
            "unknown trigger {text:?}; use M1..M4, a KeyMap name, 0xHEX or decimal"
        ))
    })
}

fn parse_slot(value: u8) -> Result<Slot, Error> {
    Slot::from_index(usize::from(value))
        .ok_or_else(|| invalid_input(format!("slot must be 0..=2, got {value}")))
}

fn parse_macro_index(value: u8) -> Result<MacroIndex, Error> {
    MacroIndex::from_index(usize::from(value))
        .ok_or_else(|| invalid_input(format!("macro index must be 0..=3, got {value}")))
}

/// Process-wide counter that makes two backup directories created in the same
/// nanosecond (or from a test overriding the clock) still distinct.
static BACKUP_DIR_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Unique `-<nanos>-<pid>-<sequence>` suffix for generated backup directories.
///
/// Seconds alone collided when two macro writes were started in the same
/// second from one `session` (and with one process id). Nanoseconds make the
/// intent obvious and the counter makes uniqueness unconditional.
fn unique_backup_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = BACKUP_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{nanos}-{}-{sequence}", std::process::id())
}

fn default_backup_dir(prefix: &str) -> PathBuf {
    PathBuf::from(format!("{prefix}-{}", unique_backup_suffix()))
}

fn pre_restore_dir(dir: &Path) -> PathBuf {
    let name = dir
        .file_name()
        .map_or_else(|| "macro-backup".into(), |name| name.to_string_lossy());
    let parent = dir.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{name}-pre-restore-{}", unique_backup_suffix()))
}

/// Build the core write plan.
///
/// Writing into an empty macro (`max_steps == 0` / `0xFFFF`) is rejected:
/// creating a new macro requires step-data clear support, which is not
/// implemented. `macro restore` can still write a non-empty backup entry into
/// a currently empty slot. `key_map` / `name` / `cycles_num` / `interval_ms`
/// are overridden only when the file or `--trigger` specifies them.
fn build_macro_write_plan(
    file: &MacroFile,
    trigger: Option<KeyMap>,
    backup: &MacroBackup,
    slot: Slot,
    index: MacroIndex,
) -> Result<MacroWritePlan, Error> {
    let record = backup.snapshot().config.macro_record(slot, index);
    if is_skipped_steps(record.max_steps) {
        return Err(Error::EmptyMacroWrite);
    }
    let mut plan = MacroWritePlan::new(file.steps.clone());
    if let Some(name) = &file.name {
        plan.name = Some(name.clone());
    }
    if let Some(trigger) = trigger {
        plan.key_map = Some(trigger);
    }
    plan.cycles_num = file.cycles_num;
    plan.interval_ms = file.interval_ms;
    Ok(plan)
}

/// Build the Write layout from the two CLI/REPL flags.
///
/// The default is the absolute `total_len` (BLE, slot S1, `macro_idx=0/1`
/// device-confirmed; other indices/slots extrapolated); the experiment flag
/// opts into the pre-2026-11-22 framing.
///
/// Write chunk offsets are always sector-based (`macro_index << 12`); a
/// relative-offset mode is not exposed because `Erase` always clears
/// `macro_index << 12` and a 0-based write would destroy another macro sector.
fn macro_write_layout(total_relative: bool) -> MacroWriteLayout {
    MacroWriteLayout {
        absolute_total_len: !total_relative,
    }
}

/// One-line description of the `WriteBT2Macro` `total_len` framing.
///
/// Printed at the start of `macro write` / `macro restore` so a device log
/// records which framing was used (A/B evidence).
fn macro_write_layout_label(layout: MacroWriteLayout) -> &'static str {
    if layout.absolute_total_len {
        "absolute (sector base + aligned)"
    } else {
        "relative (align32 only)"
    }
}

/// A validated `macro write` request, parsed before any device I/O.
///
/// Parsing and file reading happen before the connection (CLI) or before the
/// operation lock is taken (session REPL), so a bad slot, index, trigger or
/// macro file never touches the controller and never leaves an empty pre-write
/// backup directory behind.
struct PreparedMacroWrite {
    slot: Slot,
    index: MacroIndex,
    file: MacroFile,
    trigger: Option<KeyMap>,
    layout: MacroWriteLayout,
}

fn prepare_macro_write(
    slot: u8,
    index: u8,
    file: &Path,
    trigger: Option<&str>,
    layout: MacroWriteLayout,
) -> Result<PreparedMacroWrite, Error> {
    Ok(PreparedMacroWrite {
        slot: parse_slot(slot)?,
        index: parse_macro_index(index)?,
        file: read_macro_file(file)?,
        trigger: trigger.map(parse_trigger).transpose()?,
        layout,
    })
}

/// Resolve the mandatory pre-write backup directory.
///
/// The session REPL has no `--backup-dir` option, so it always gets the
/// timestamped default.
fn resolve_write_backup_dir(backup_dir: Option<&Path>) -> PathBuf {
    backup_dir.map_or_else(
        || default_backup_dir("macro-backup"),
        std::path::Path::to_path_buf,
    )
}

/// Run one macro write on an existing session.
///
/// Shared by the top-level `macro write` command and the `session` REPL. The
/// caller owns the connection; the REPL wrapper [`run_session_macro_write`]
/// owns the operation lock for the whole call.
///
/// Only the Ctrl-C warning is printed here, before any device I/O. The
/// pre-write backup path is printed later, after the backup directory has
/// actually been written, so an early failure never points the operator at a
/// path that does not exist.
async fn run_macro_write<T: Transport>(
    session: &Session<T>,
    request: &PreparedMacroWrite,
    backup_dir: &Path,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    eprintln!(
        "warning: do not press Ctrl-C during the macro write; a failure can leave a sector \
         partially updated."
    );
    macro_write_session_with_layout(
        session,
        request.slot,
        request.index,
        &request.file,
        request.trigger,
        backup_dir,
        chunk_size,
        request.layout,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn macro_write(
    options: &BleOptions,
    slot: u8,
    index: u8,
    file: &Path,
    trigger: Option<&str>,
    backup_dir: Option<&Path>,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
    poll_interval_ms: u64,
    report_enable: bool,
) -> Result<(), Error> {
    let request = prepare_macro_write(slot, index, file, trigger, layout)?;
    let backup_dir = resolve_write_backup_dir(backup_dir);

    let transport = BleTransport::connect(options).await?;
    let session = macro_session(transport, poll_interval_ms, report_enable);
    let result = run_macro_write(&session, &request, &backup_dir, chunk_size).await;
    print_poll_stats(options, &session);
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    result
}

/// Default-layout wrapper for tests and older callers.
///
/// Production goes through [`macro_write_session_with_layout`] so the
/// experimental flags are explicit.
#[cfg(test)]
async fn macro_write_session<T: Transport>(
    session: &Session<T>,
    slot: Slot,
    index: MacroIndex,
    file: &MacroFile,
    trigger: Option<KeyMap>,
    backup_dir: &Path,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    macro_write_session_with_layout(
        session,
        slot,
        index,
        file,
        trigger,
        backup_dir,
        chunk_size,
        MacroWriteLayout::default_ble(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn macro_write_session_with_layout<T: Transport>(
    session: &Session<T>,
    slot: Slot,
    index: MacroIndex,
    file: &MacroFile,
    trigger: Option<KeyMap>,
    backup_dir: &Path,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
) -> Result<(), Error> {
    let created = !backup_dir.exists();
    let result = macro_write_session_inner(
        session, slot, index, file, trigger, backup_dir, chunk_size, layout,
    )
    .await;
    if result.is_err() && created {
        // Do not leave an empty directory behind on an early failure (empty
        // macro, MTU too small, config read failure).
        let _ = std::fs::remove_dir(lexical_directory(backup_dir));
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn macro_write_session_inner<T: Transport>(
    session: &Session<T>,
    slot: Slot,
    index: MacroIndex,
    file: &MacroFile,
    trigger: Option<KeyMap>,
    backup_dir: &Path,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
) -> Result<(), Error> {
    let mtu = session.transport().mtu();
    // Fail before creating a pre-write backup directory if this MTU cannot
    // support the macro write at all.
    macro_header_chunk_len(mtu)?;
    let effective_chunk_size = match chunk_size {
        Some(requested) => macro_write_chunk_len_requested(mtu, requested)?,
        None => macro_write_chunk_len(mtu)?,
    };
    prepare_output_directory(backup_dir)?;
    println!("reading configuration...");
    let snapshot = session.read_config_snapshot().await?;
    let target = snapshot.config.macro_record(slot, index);
    if is_skipped_steps(target.max_steps) {
        return Err(Error::EmptyMacroWrite);
    }

    println!("reading full 3-slot × 4-macro backup...");
    let mut entries = Vec::with_capacity(SLOT_COUNT * MACROS_PER_SLOT);
    for backup_slot in Slot::ALL {
        for backup_index in MacroIndex::ALL {
            match session
                .read_macro(&snapshot, backup_slot, backup_index)
                .await
            {
                Ok(entry) => entries.push(entry),
                Err(error) if backup_slot == slot => {
                    return Err(Error::Unsupported(format!(
                        "unreadable macro in target slot; use macro restore or official app. \
                         slot {} macro {}: {error}",
                        backup_slot.index(),
                        backup_index.index()
                    )));
                }
                Err(error) => return Err(error),
            }
        }
    }
    let backup = MacroBackup::new(snapshot.clone(), entries)?;
    let config_sha256 = sha256_hex(snapshot.config.as_bytes());
    write_macro_backup(backup_dir, &snapshot, &backup, &config_sha256)?;
    // Only now is there a real recovery backup to point at. Printing it before
    // this point would advertise a path when an early failure (empty macro,
    // MTU too small, config read error) leaves no backup at all.
    eprintln!(
        "pre-write backup: {}; recover with: eightbitdo macro restore --dir {}",
        backup_dir.display(),
        backup_dir.display()
    );

    if trigger.is_none() && !file.key_map.is_empty() {
        println!(
            "note: the file key_map 0x{:08X} is ignored; use --trigger to change the trigger",
            file.key_map.bits()
        );
    }
    if file.gamepad_mode != 0 {
        println!(
            "note: MacroFile.gamepad_mode = {} is ignored; the current device mode is used",
            file.gamepad_mode
        );
    }

    let mut plan = build_macro_write_plan(file, trigger, &backup, slot, index)?;
    plan.chunk_size = chunk_size;
    plan.absolute_total_len = layout.absolute_total_len;
    plan.save_transport = SaveTransport::Ble;

    println!("write total_len: {}", macro_write_layout_label(layout));
    println!(
        "writing slot {} (profile {}) macro {} ({}): {} steps (chunk size {effective_chunk_size}B, MTU {mtu})",
        slot.index(),
        slot.number(),
        index.index(),
        macro_id(index),
        plan.steps.len(),
    );
    mark_flash_write_active();
    let outcome = session.write_macro(&backup, slot, index, &plan).await?;
    warn_if_config_crc_invalid(outcome.config_crc_valid);

    println!("wrote macro: slot {} macro {}", slot.index(), index.index());
    println!("  name: {:?}", outcome.record.name());
    println!("  max_steps: {}", outcome.record.max_steps);
    println!(
        "  key_map: 0x{:08X} ({})",
        outcome.record.key_map.bits(),
        outcome
            .record
            .key_map
            .name()
            .map_or_else(|| "unknown/combination".to_string(), str::to_string)
    );
    println!(
        "  flag: 0x{:08X}, total_cnt: {}",
        outcome.flag, outcome.total_cnt
    );
    println!(
        "  cycles_num: {}, interval_ms: {}",
        outcome.record.cycles_num, outcome.record.interval_ms
    );
    println!("  packets sent: {}", outcome.packets_sent);
    println!(
        "  config crc: {}",
        config_crc_label(outcome.config_crc_valid)
    );
    println!("  read-back sha256: {}", sha256_hex(outcome.readback.raw()));
    println!("  read-back offsets: {:?}", outcome.readback.read_offsets());
    println!(
        "restore with: eightbitdo macro restore --dir {}",
        backup_dir.display()
    );
    Ok(())
}

/// Human-readable macro write header/ACK CRC state.
fn config_crc_label(config_crc_valid: bool) -> &'static str {
    if config_crc_valid {
        "valid"
    } else {
        "invalid (216B partial header write; a full 2768B config write may be required)"
    }
}

/// Read and validate `config.bin` / `macros.json` (or `macros.toml`).
fn read_restore_backup(dir: &Path) -> Result<MacroBackup, Error> {
    if dir.as_os_str().is_empty() || lexical_directory(dir).as_os_str().is_empty() {
        return Err(invalid_input("backup directory must not be empty"));
    }
    let config_path = dir.join("config.bin");
    ensure_regular_file(&config_path, "backup config")?;
    let config_bytes = std::fs::read(&config_path)?;
    let config = Bt2Config::from_bytes(&config_bytes)?;
    let stored_crc = config.crc16_stored() as u16;
    let mut check = config.clone();
    let computed_crc = check.recompute_crc();
    let snapshot = ConfigSnapshot {
        config,
        stored_crc,
        computed_crc,
        crc_valid: stored_crc == computed_crc,
    };

    let manifest_path = if dir.join("macros.json").is_file() {
        dir.join("macros.json")
    } else if dir.join("macros.toml").is_file() {
        dir.join("macros.toml")
    } else {
        return Err(invalid_input(format!(
            "{} has neither macros.json nor macros.toml",
            dir.display()
        )));
    };
    ensure_regular_file(&manifest_path, "backup manifest")?;
    let manifest_text = std::fs::read_to_string(&manifest_path)?;
    let manifest: MacroBackupDocument =
        if manifest_path.extension().is_some_and(|ext| ext == "json") {
            serde_json::from_str(&manifest_text)
                .map_err(|error| invalid_data(format!("{}: {error}", manifest_path.display())))?
        } else {
            toml::from_str(&manifest_text)
                .map_err(|error| invalid_data(format!("{}: {error}", manifest_path.display())))?
        };

    if manifest.format != 1 {
        return Err(invalid_data(format!(
            "unsupported backup manifest format {}",
            manifest.format
        )));
    }
    if manifest.slot_count != SLOT_COUNT || manifest.macros_per_slot != MACROS_PER_SLOT {
        return Err(invalid_data(format!(
            "backup manifest declares {} slots x {} macros, expected 3 x 4",
            manifest.slot_count, manifest.macros_per_slot
        )));
    }
    let actual_sha = sha256_hex(&config_bytes);
    if manifest.config_sha256 != actual_sha {
        return Err(invalid_data(format!(
            "config.bin SHA-256 {} does not match the manifest {}",
            actual_sha, manifest.config_sha256
        )));
    }

    let mut entries = Vec::with_capacity(SLOT_COUNT * MACROS_PER_SLOT);
    for slot in Slot::ALL {
        for index in MacroIndex::ALL {
            let record = snapshot.config.macro_record(slot, index);
            let document = manifest
                .macros
                .iter()
                .find(|entry| {
                    entry.slot == slot.index() as u8 && entry.index == index.index() as u8
                })
                .ok_or_else(|| {
                    invalid_data(format!(
                        "backup manifest is missing slot {} macro {}",
                        slot.index(),
                        index.index()
                    ))
                })?;
            let skipped = is_skipped_steps(record.max_steps);
            if document.skipped != skipped {
                return Err(invalid_data(format!(
                    "slot {} macro {}: manifest skipped={} but config max_steps={}",
                    slot.index(),
                    index.index(),
                    document.skipped,
                    record.max_steps
                )));
            }
            if document.max_steps != record.max_steps {
                return Err(invalid_data(format!(
                    "slot {} macro {}: manifest max_steps={} but config max_steps={}",
                    slot.index(),
                    index.index(),
                    document.max_steps,
                    record.max_steps
                )));
            }
            let raw = if skipped {
                Vec::new()
            } else {
                let raw_file = document.raw_file.as_deref().ok_or_else(|| {
                    invalid_data(format!(
                        "slot {} macro {}: manifest has no raw_file",
                        slot.index(),
                        index.index()
                    ))
                })?;
                let raw_path = safe_backup_path(dir, raw_file)?;
                ensure_regular_file(&raw_path, "backup macro raw file")?;
                let bytes = std::fs::read(&raw_path)?;
                let expected_len = usize::from(record.max_steps) * 10;
                if bytes.len() != expected_len {
                    return Err(invalid_data(format!(
                        "{}: {} bytes, expected {expected_len}",
                        raw_path.display(),
                        bytes.len()
                    )));
                }
                let expected = document.raw_sha256.as_deref().ok_or_else(|| {
                    invalid_data(format!(
                        "slot {} macro {}: non-empty manifest entry has no raw_sha256",
                        slot.index(),
                        index.index()
                    ))
                })?;
                let actual = sha256_hex(&bytes);
                if actual != expected {
                    return Err(invalid_data(format!(
                        "{}: SHA-256 {actual} does not match the manifest {expected}",
                        raw_path.display()
                    )));
                }
                bytes
            };
            entries.push(MacroEntry::new(slot, index, record, raw)?);
        }
    }
    MacroBackup::new(snapshot, entries)
}

fn safe_backup_path(dir: &Path, name: &str) -> Result<PathBuf, Error> {
    let path = Path::new(name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(dir.join(path)),
        _ => Err(invalid_input(format!(
            "backup raw_file {name:?} must be a plain file name inside the backup directory"
        ))),
    }
}

fn ensure_backup_gamepad_mode(
    backup: &ConfigSnapshot,
    current: &ConfigSnapshot,
) -> Result<(), Error> {
    let expected = backup.config.gamepad_mode_raw() & 0xFF;
    let actual = current.config.gamepad_mode_raw() & 0xFF;
    if expected != actual {
        return Err(invalid_input(format!(
            "backup gamepad_mode 0x{expected:02X} does not match the current 0x{actual:02X}; \
             switch the controller to the same mode before restoring"
        )));
    }
    Ok(())
}

/// Run one macro restore on an existing session.
///
/// Shared by the top-level `macro restore` command and the `session` REPL.
/// The pre-restore backup, the gamepad-mode check, the `--force` conflict
/// rules, the symlink/complete-backup validation and the read-back
/// verification all stay in `macro_restore_session_core`.
async fn run_macro_restore<T: Transport>(
    session: &Session<T>,
    dir: &Path,
    force: bool,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
) -> Result<(), Error> {
    macro_restore_session_with_layout(session, dir, force, chunk_size, layout).await
}

async fn macro_restore(
    options: &BleOptions,
    dir: &Path,
    force: bool,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
    poll_interval_ms: u64,
    report_enable: bool,
) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let session = macro_session(transport, poll_interval_ms, report_enable);
    let result = run_macro_restore(&session, dir, force, chunk_size, layout).await;
    print_poll_stats(options, &session);
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    result
}

fn macro_header_bytes(config: &Bt2Config, slot: Slot) -> &[u8] {
    let offset = macro_slot_offset(slot);
    &config.as_bytes()[offset..offset + MACRO_SLOT_SIZE]
}

fn restore_macro_plan(
    entry: &MacroEntry,
    header: &MacroSlot,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
) -> MacroWritePlan {
    let mut plan = MacroWritePlan::new(entry.steps().to_vec())
        .with_key_map(entry.record().key_map)
        .with_cycles_num(entry.record().cycles_num)
        .with_interval_ms(entry.record().interval_ms)
        .with_header_state(header.flag, header.total_cnt)
        .with_save_transport(SaveTransport::Ble);
    plan.chunk_size = chunk_size;
    plan.absolute_total_len = layout.absolute_total_len;
    plan
}

fn config_snapshot(config: Bt2Config) -> ConfigSnapshot {
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

fn restore_conflicts_from_config(backup: &MacroBackup, current: &ConfigSnapshot) -> Vec<String> {
    let mut conflicts = Vec::new();
    for slot in Slot::ALL {
        for index in MacroIndex::ALL {
            let target = backup.get(slot, index).expect("full backup has 12 entries");
            let record = current.config.macro_record(slot, index);
            if target.is_empty() && !is_skipped_steps(record.max_steps) {
                conflicts.push(format!(
                    "slot {} macro {} ({} steps -> 0)",
                    slot.index(),
                    index.index(),
                    record.max_steps
                ));
            }
        }
    }
    conflicts
}

fn ensure_restore_allowed_from_config(
    backup: &MacroBackup,
    current: &ConfigSnapshot,
    force: bool,
) -> Result<Vec<String>, Error> {
    let conflicts = restore_conflicts_from_config(backup, current);
    if !conflicts.is_empty() && !force {
        return Err(invalid_input(format!(
            "restore would make non-empty macros empty; rerun with --force. targets: {}",
            conflicts.join(", ")
        )));
    }
    Ok(conflicts)
}

/// Capture the current macros without aborting on one unreadable entry.
///
/// The returned backup marks unreadable macros as skipped (`max_steps = 0`)
/// and lists them in the second tuple element. It is only a pre-restore
/// recovery artifact; `--force` is required to continue when entries were
/// skipped.
async fn read_current_macros_best_effort<T: Transport>(
    session: &Session<T>,
    snapshot: &ConfigSnapshot,
) -> Result<(MacroBackup, Vec<String>), Error> {
    let mut config = snapshot.config.clone();
    let mut entries = Vec::with_capacity(SLOT_COUNT * MACROS_PER_SLOT);
    let mut skipped = Vec::new();
    for slot in Slot::ALL {
        for index in MacroIndex::ALL {
            let record = snapshot.config.macro_record(slot, index);
            if is_skipped_steps(record.max_steps) {
                entries.push(MacroEntry::new(slot, index, record, Vec::new())?);
                continue;
            }
            match session.read_macro(snapshot, slot, index).await {
                Ok(entry) => entries.push(entry),
                Err(error) => {
                    skipped.push(format!(
                        "slot {} macro {}: {error}",
                        slot.index(),
                        index.index()
                    ));
                    let mut disabled = record;
                    disabled.max_steps = 0;
                    let offset = macro_record_offset(slot, index) + 0x22;
                    config.as_mut_bytes()[offset..offset + 2].copy_from_slice(&0u16.to_le_bytes());
                    entries.push(MacroEntry::new(slot, index, disabled, Vec::new())?);
                }
            }
        }
    }
    if skipped.is_empty() {
        return Ok((MacroBackup::new(snapshot.clone(), entries)?, skipped));
    }
    config.recompute_crc();
    Ok((MacroBackup::new(config_snapshot(config), entries)?, skipped))
}

/// Default-layout restore wrapper for tests.
#[cfg(test)]
async fn macro_restore_session<T: Transport>(
    session: &Session<T>,
    dir: &Path,
    force: bool,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    macro_restore_session_with_layout(
        session,
        dir,
        force,
        chunk_size,
        MacroWriteLayout::default_ble(),
    )
    .await
}

async fn macro_restore_session_with_layout<T: Transport>(
    session: &Session<T>,
    dir: &Path,
    force: bool,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
) -> Result<(), Error> {
    let pre_dir = pre_restore_dir(dir);
    macro_restore_session_with_pre_dir_and_layout(session, dir, force, pre_dir, chunk_size, layout)
        .await
}

/// Default-layout restore wrapper with an injected pre-restore directory.
#[cfg(test)]
async fn macro_restore_session_with_pre_dir<T: Transport>(
    session: &Session<T>,
    dir: &Path,
    force: bool,
    pre_dir: PathBuf,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    macro_restore_session_with_pre_dir_and_layout(
        session,
        dir,
        force,
        pre_dir,
        chunk_size,
        MacroWriteLayout::default_ble(),
    )
    .await
}

async fn macro_restore_session_with_pre_dir_and_layout<T: Transport>(
    session: &Session<T>,
    dir: &Path,
    force: bool,
    pre_dir: PathBuf,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
) -> Result<(), Error> {
    let pre_created = !pre_dir.exists();
    let result =
        macro_restore_session_core(session, dir, force, &pre_dir, chunk_size, layout).await;
    if result.is_err() && pre_created {
        // Keep a non-empty incomplete backup for recovery; remove only an
        // empty directory created before the failure.
        let _ = std::fs::remove_dir(lexical_directory(&pre_dir));
    }
    result
}

fn report_restore_recovery(pre_restore_complete: bool, pre_dir: &Path) {
    if pre_restore_complete {
        eprintln!("pre-restore backup: {}", pre_dir.display());
    } else {
        eprintln!("no complete pre-restore backup is available");
    }
}

fn warn_if_config_crc_invalid(config_crc_valid: bool) {
    if !config_crc_valid {
        eprintln!("warning: config CRC is invalid; a full 2768B config write may be required");
    }
}

fn restore_recovery_warning(
    pre_restore_complete: bool,
    pre_dir: &Path,
    skipped: &[String],
) -> String {
    if pre_restore_complete {
        format!(
            "pre-restore backup: {}; recover with: eightbitdo macro restore --dir {}",
            pre_dir.display(),
            pre_dir.display()
        )
    } else {
        format!(
            "no complete recovery backup is available; skipped: {}",
            skipped.join(", ")
        )
    }
}

async fn macro_restore_session_core<T: Transport>(
    session: &Session<T>,
    dir: &Path,
    force: bool,
    pre_dir: &Path,
    chunk_size: Option<u16>,
    layout: MacroWriteLayout,
) -> Result<(), Error> {
    let mtu = session.transport().mtu();
    if let Some(requested) = chunk_size {
        // Reject unsupported values before any backup directory is created.
        // MTU fit is checked later, only when a non-empty macro is restored.
        validate_macro_chunk_size(requested)?;
    }
    let backup = read_restore_backup(dir)?;
    println!("loaded backup: {}", dir.display());
    println!(
        "write total_len: {} (all restored macros)",
        macro_write_layout_label(layout)
    );

    println!("taking a fresh pre-restore backup of the current controller...");
    let current_snapshot = session.read_config_snapshot().await?;
    ensure_backup_gamepad_mode(backup.snapshot(), &current_snapshot)?;
    let (current, skipped_reads) =
        read_current_macros_best_effort(session, &current_snapshot).await?;
    if let Err(error) = write_macro_backup(
        pre_dir,
        current.snapshot(),
        &current,
        &sha256_hex(current.snapshot().config.as_bytes()),
    ) {
        return Err(invalid_input(format!(
            "pre-restore backup failed; complete recovery backup could not be created: {error}. \
             --force does not allow a failed pre-restore backup"
        )));
    }
    println!("pre-restore backup: {}", pre_dir.display());
    let pre_restore_complete = skipped_reads.is_empty();

    if !skipped_reads.is_empty() {
        for warning in &skipped_reads {
            eprintln!("warning: pre-restore backup skipped {warning}");
        }
        if !force {
            return Err(invalid_input(format!(
                "pre-restore backup is incomplete; rerun with --force. skipped: {}; path: {}",
                skipped_reads.join(", "),
                pre_dir.display()
            )));
        }
        eprintln!(
            "warning: --force continues without a complete pre-restore backup. skipped: {}",
            skipped_reads.join(", ")
        );
    }

    let conflicts = ensure_restore_allowed_from_config(&backup, &current_snapshot, force)?;
    if !conflicts.is_empty() {
        eprintln!(
            "warning: --force will overwrite the 216B slot header, making these macros empty \
             (step data remains in flash): {}",
            conflicts.join(", ")
        );
        if pre_restore_complete {
            eprintln!("recovery backup: {}", pre_dir.display());
        } else {
            eprintln!("no complete pre-restore backup is available");
        }
    }

    let mut slots_with_non_empty = [false; SLOT_COUNT];
    let mut restored = 0usize;
    let mut headers_restored = 0usize;
    let mut skipped = 0usize;
    for slot in Slot::ALL {
        for index in MacroIndex::ALL {
            let entry = backup.get(slot, index).expect("full backup has 12 entries");
            let record = entry.record();
            if is_skipped_steps(record.max_steps) {
                skipped += 1;
                continue;
            }
            slots_with_non_empty[slot.index()] = true;
        }
    }

    let mut has_writes = slots_with_non_empty.iter().any(|written| *written);
    for slot in Slot::ALL {
        if !slots_with_non_empty[slot.index()]
            && macro_header_bytes(&backup.snapshot().config, slot)
                != macro_header_bytes(&current_snapshot.config, slot)
        {
            has_writes = true;
        }
    }

    let effective_chunk_size = if slots_with_non_empty.iter().any(|written| *written) {
        let size = match chunk_size {
            Some(requested) => macro_write_chunk_len_requested(mtu, requested)?,
            None => macro_write_chunk_len(mtu)?,
        };
        println!("macro write chunk size: {size} bytes");
        Some(size as u16)
    } else {
        None
    };

    if has_writes {
        eprintln!(
            "warning: do not press Ctrl-C during the macro write; a failure can leave a sector \
             partially updated. {}",
            restore_recovery_warning(pre_restore_complete, pre_dir, &skipped_reads)
        );
    }

    for slot in Slot::ALL {
        if !slots_with_non_empty[slot.index()] {
            continue;
        }
        let header = backup.snapshot().config.macro_slot(slot);
        let plans: Vec<(MacroIndex, MacroWritePlan)> = MacroIndex::ALL
            .iter()
            .filter_map(|index| {
                let entry = backup
                    .get(slot, *index)
                    .expect("full backup has 12 entries");
                if is_skipped_steps(entry.record().max_steps) {
                    return None;
                }
                Some((
                    *index,
                    restore_macro_plan(entry, &header, effective_chunk_size, layout),
                ))
            })
            .collect();
        println!(
            "restoring slot {} ({} macros: Header -> Save -> Erase/Write -> Save)...",
            slot.index(),
            plans.len()
        );
        mark_flash_write_active();
        match session.write_macro_slot(&backup, slot, &plans).await {
            Ok(outcome) => {
                println!(
                    "  slot {}: packets sent {}, config crc {}",
                    slot.index(),
                    outcome.packets_sent,
                    config_crc_label(outcome.config_crc_valid)
                );
                for readback in &outcome.readbacks {
                    println!(
                        "    macro {} ({}): read-back sha256 {}",
                        readback.index().index(),
                        macro_id(readback.index()),
                        sha256_hex(readback.raw())
                    );
                }
                warn_if_config_crc_invalid(outcome.config_crc_valid);
            }
            Err(error) => {
                eprintln!("restore stopped at slot {}: {error}", slot.index());
                report_restore_recovery(pre_restore_complete, pre_dir);
                return Err(error);
            }
        }
        restored += plans.len();
    }

    for slot in Slot::ALL {
        if slots_with_non_empty[slot.index()] {
            continue;
        }
        if macro_header_bytes(&backup.snapshot().config, slot)
            == macro_header_bytes(&current_snapshot.config, slot)
        {
            continue;
        }
        println!(
            "restoring slot {} 216B header only (no step data written)...",
            slot.index()
        );
        match session.write_macro_header(&backup, slot).await {
            Ok(outcome) => {
                println!(
                    "  slot {}: header packets sent {}, config crc {}",
                    slot.index(),
                    outcome.packets_sent,
                    config_crc_label(outcome.config_crc_valid)
                );
                warn_if_config_crc_invalid(outcome.config_crc_valid);
            }
            Err(error) => {
                eprintln!("restore header stopped at slot {}: {error}", slot.index());
                report_restore_recovery(pre_restore_complete, pre_dir);
                return Err(error);
            }
        }
        headers_restored += 1;
    }

    verify_restored_state(
        session,
        &backup,
        if pre_restore_complete {
            Some(pre_dir)
        } else {
            None
        },
    )
    .await?;
    println!(
        "restore finished: {restored} macros restored, {headers_restored} header-only slots, \
         {skipped} empty entries skipped (step data not erased)"
    );
    Ok(())
}

async fn verify_restored_state<T: Transport>(
    session: &Session<T>,
    backup: &MacroBackup,
    pre_restore_backup: Option<&Path>,
) -> Result<(), Error> {
    let recovery = pre_restore_backup.map_or_else(
        || "no complete pre-restore backup".to_string(),
        |path| format!("pre-restore backup: {}", path.display()),
    );
    let final_snapshot = session.read_config_snapshot().await?;
    for slot in Slot::ALL {
        if macro_header_bytes(&backup.snapshot().config, slot)
            != macro_header_bytes(&final_snapshot.config, slot)
        {
            return Err(Error::UnexpectedResponse(format!(
                "restore verification failed: slot {} 216B header differs. The device may have \
                 normalized flag/total_cnt/reserved bytes; {recovery}",
                slot.index()
            )));
        }
    }
    for slot in Slot::ALL {
        for index in MacroIndex::ALL {
            let entry = backup.get(slot, index).expect("full backup has 12 entries");
            if entry.is_empty() {
                continue;
            }
            let readback = session.read_macro(&final_snapshot, slot, index).await?;
            if readback.raw() != entry.raw() {
                return Err(Error::UnexpectedResponse(format!(
                    "restore verification failed: slot {} macro {} raw steps differ; {recovery}",
                    slot.index(),
                    index.index()
                )));
            }
        }
    }
    let final_sha = sha256_hex(final_snapshot.config.as_bytes());
    let backup_sha = sha256_hex(backup.snapshot().config.as_bytes());
    println!("final config sha256 (informational): {final_sha} (backup {backup_sha})");
    Ok(())
}

/// Run one macro backup on an existing session.
///
/// Shared by the top-level `macro backup` command and the `session` REPL.
/// Read-only on the controller; the output-directory rules (empty/non-empty
/// directory, symlink) live in [`macro_backup_session`].
async fn run_macro_backup<T: Transport>(session: &Session<T>, dir: &Path) -> Result<(), Error> {
    macro_backup_session(session, dir).await
}

/// Read the configuration and all 3 slots × 4 macros into `dir`.
async fn macro_backup(
    options: &BleOptions,
    dir: &Path,
    poll_interval_ms: u64,
    report_enable: bool,
) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let session = macro_session(transport, poll_interval_ms, report_enable);
    let result = run_macro_backup(&session, dir).await;
    print_poll_stats(options, &session);
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    result
}

/// One-connection macro backup: config snapshot, then every macro, then files.
///
/// Files are written only after every macro read succeeds, so a directory
/// never looks like a complete backup while one read is missing.
async fn macro_backup_session<T: Transport>(session: &Session<T>, dir: &Path) -> Result<(), Error> {
    let created = !dir.exists();
    let result = macro_backup_session_inner(session, dir).await;
    if result.is_err() && created {
        // Best effort: a failed backup should not leave an empty directory
        // that looks like a usable backup target.
        let _ = std::fs::remove_dir(lexical_directory(dir));
    }
    result
}

async fn macro_backup_session_inner<T: Transport>(
    session: &Session<T>,
    dir: &Path,
) -> Result<(), Error> {
    prepare_output_directory(dir)?;
    println!("reading configuration...");
    let snapshot = session.read_config_snapshot().await?;
    let config_sha256 = sha256_hex(snapshot.config.as_bytes());
    println!("read config: {CONFIG_SIZE} bytes, sha256 {config_sha256}");
    println!(
        "gamepad_mode: {} ({})",
        snapshot.config.gamepad_mode_raw(),
        snapshot
            .config
            .gamepad_mode()
            .map_or_else(|| "unknown".to_string(), |mode| format!("{mode:?}"))
    );

    let mut entries = Vec::with_capacity(SLOT_COUNT * MACROS_PER_SLOT);
    for slot in Slot::ALL {
        for index in MacroIndex::ALL {
            let entry = session.read_macro(&snapshot, slot, index).await?;
            print_macro_progress(&entry);
            entries.push(entry);
        }
    }
    let backup = MacroBackup::new(snapshot.clone(), entries)?;

    write_macro_backup(dir, &snapshot, &backup, &config_sha256)?;
    println!("wrote macro backup: {}", dir.display());
    println!("  config.bin / config.json");
    println!("  macros.json / macros.toml");
    println!("  per-macro raw .bin files (skipped entries have none)");
    Ok(())
}

fn print_macro_progress(entry: &MacroEntry) {
    let slot = entry.slot();
    let index = entry.index();
    let record = entry.record();
    let label = macro_id(index);
    if is_skipped_steps(record.max_steps) {
        let reason = if record.max_steps == 0 {
            "max_steps = 0"
        } else {
            "max_steps = 0xFFFF (unset marker)"
        };
        println!(
            "slot {} (profile {}) macro {} ({}): {}, skipped",
            slot.index(),
            slot.number(),
            index.index(),
            label,
            reason
        );
    } else {
        println!(
            "slot {} (profile {}) macro {} ({}): max_steps = {}, read {} bytes",
            slot.index(),
            slot.number(),
            index.index(),
            label,
            record.max_steps,
            entry.raw().len()
        );
        println!("  response offsets: {:?}", entry.read_offsets());
    }
}

fn macro_id(index: MacroIndex) -> &'static str {
    match index {
        MacroIndex::M1 => "M1",
        MacroIndex::M2 => "M2",
        MacroIndex::M3 => "M3",
        MacroIndex::M4 => "M4",
    }
}

fn macro_skip_reason(max_steps: u16) -> Option<&'static str> {
    match max_steps {
        0 => Some("max_steps = 0"),
        u16::MAX => Some("max_steps = 0xFFFF (unset marker; 実機未確認)"),
        _ => None,
    }
}

/// Remove trailing `/` and `.` components for a lexical symlink check.
///
/// `symlink_metadata("link/")` follows the symlink and reports the target
/// directory, so the raw path must be normalized first.
fn lexical_directory(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        if component != Component::CurDir {
            normalized.push(component.as_os_str());
        }
    }
    normalized
}

/// Prepare a backup output directory without overwriting an existing backup.
///
/// A non-existent or empty directory is accepted. An empty path, a non-empty
/// directory, or a non-directory is rejected before any file is written.
/// Symlinks are rejected only at the **final path component** (including
/// `link/` / `link/.`); symlinks in intermediate components are not detected.
fn prepare_output_directory(dir: &Path) -> Result<(), Error> {
    if dir.as_os_str().is_empty() {
        return Err(invalid_input("backup directory must not be empty"));
    }
    let lexical = lexical_directory(dir);
    if lexical.as_os_str().is_empty() {
        return Err(invalid_input(
            "backup directory must not resolve to an empty path",
        ));
    }
    match std::fs::symlink_metadata(&lexical) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(invalid_input(format!(
                    "{} is a symlink; choose a new backup directory",
                    dir.display()
                )));
            }
            if !metadata.is_dir() {
                return Err(invalid_input(format!(
                    "{} is not a directory",
                    dir.display()
                )));
            }
            if std::fs::read_dir(&lexical)?.next().is_some() {
                return Err(invalid_input(format!(
                    "{} is not empty; choose a new backup directory",
                    dir.display()
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(&lexical)?;
            Ok(())
        }
        Err(error) => Err(Error::Io(error)),
    }
}

/// Write a complete backup directory.
///
/// `config.bin` and per-macro `.bin` files are raw bytes. `macros.json` and
/// `macros.toml` carry the same manifest including SHA-256 digests and decoded
/// steps. The manifest is written last.
fn write_macro_backup(
    dir: &Path,
    snapshot: &ConfigSnapshot,
    backup: &MacroBackup,
    config_sha256: &str,
) -> Result<(), Error> {
    prepare_output_directory(dir)?;

    write_atomic(&dir.join("config.bin"), snapshot.config.as_bytes())?;
    let summary = json_summary(snapshot, config_sha256);
    let encoded = serde_json::to_string_pretty(&summary).map_err(std::io::Error::other)?;
    write_atomic(&dir.join("config.json"), format!("{encoded}\n").as_bytes())?;

    let mut entries = Vec::with_capacity(SLOT_COUNT * MACROS_PER_SLOT);
    for entry in backup.entries() {
        let slot = entry.slot();
        let index = entry.index();
        let record = entry.record();
        let skipped = is_skipped_steps(record.max_steps);

        let (raw_file, raw_sha256) = if skipped {
            (None, None)
        } else {
            let file_name = format!("slot{}-macro{}.bin", slot.index(), index.index());
            write_atomic(&dir.join(&file_name), entry.raw())?;
            (Some(file_name), Some(sha256_hex(entry.raw())))
        };

        entries.push(MacroBackupDocumentEntry {
            slot: slot.index() as u8,
            profile: slot.number(),
            index: index.index() as u8,
            id: macro_id(index).to_string(),
            name: record.name(),
            skipped,
            skip_reason: macro_skip_reason(record.max_steps).map(str::to_string),
            max_steps: record.max_steps,
            key_map: record.key_map.bits(),
            key_map_name: record.key_map.name().map(str::to_string),
            cycles_num: record.cycles_num,
            interval_ms: record.interval_ms,
            raw_file,
            raw_sha256,
            response_offsets: entry.read_offsets().to_vec(),
            steps: entry.steps().to_vec(),
        });
    }

    let document = MacroBackupDocument {
        format: 1,
        config_file: "config.bin".to_string(),
        config_sha256: config_sha256.to_string(),
        gamepad_mode: snapshot.config.gamepad_mode_raw(),
        slot_count: SLOT_COUNT,
        macros_per_slot: MACROS_PER_SLOT,
        macros: entries,
    };

    let json = serde_json::to_string_pretty(&document).map_err(std::io::Error::other)?;
    write_atomic(&dir.join("macros.json"), format!("{json}\n").as_bytes())?;
    let toml = toml::to_string_pretty(&document).map_err(std::io::Error::other)?;
    write_atomic(&dir.join("macros.toml"), toml.as_bytes())?;

    println!("config sha256: {config_sha256}");
    Ok(())
}

async fn monitor(options: &BleOptions, count: Option<u64>) -> Result<ExitCode, Error> {
    if count == Some(0) {
        return Ok(ExitCode::SUCCESS);
    }

    let transport = match with_sigint(BleTransport::connect(options), wait_for_sigint()).await {
        SigintOutcome::Finished(Ok(transport)) => transport,
        SigintOutcome::Finished(Err(error)) => return Err(error),
        SigintOutcome::Interrupted => {
            warn_sigint_disconnects();
            return Ok(ExitCode::from(EXIT_INTERRUPTED));
        }
    };
    let session = Session::new(transport);
    // Race the whole monitor (including its initial 0x0007 enable and the poll
    // loop) against SIGINT. The inner shutdown stays pending so this outer
    // race is the only signal consumer.
    let outcome = with_sigint(
        monitor_connection(
            &session,
            Duration::from_millis(20),
            count,
            std::future::pending::<()>(),
        ),
        wait_for_sigint(),
    )
    .await;
    match outcome {
        SigintOutcome::Finished(result) => {
            best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
            result.map(|_| ExitCode::SUCCESS)
        }
        SigintOutcome::Interrupted => {
            // The cancelled monitor did not reach its own report disable.
            best_effort_disable_reports(&session, CLEANUP_TIMEOUT).await;
            best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
            warn_sigint_disconnects();
            Ok(ExitCode::from(EXIT_INTERRUPTED))
        }
    }
}

/// Run the top-level monitor against an already connected session.
///
/// Unlike the background session monitor, this entry point owns the report
/// enable: it enables reports, polls `0x0003` every `interval`, prints each
/// report, then disables reports on every exit path. The connection itself
/// stays owned by the caller.
async fn monitor_connection<T: Transport>(
    session: &Session<T>,
    interval: Duration,
    count: Option<u64>,
    shutdown: impl Future<Output = ()>,
) -> Result<MonitorPollOutcome, Error> {
    session.set_report_enable(true).await?;
    let stop = Notify::new();
    let stats = MonitorStats::default();
    tokio::pin!(shutdown);
    let result = monitor_poll(session, interval, count, &stop, &mut shutdown, &stats).await;
    // Best effort: stop notifications even if the poll failed or was
    // interrupted. The caller still disconnects the link.
    best_effort_disable_reports(session, CLEANUP_TIMEOUT).await;
    result
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
    if duration.is_zero() {
        println!("keepalive duration is 0; no poll was sent");
        return Ok(ExitCode::SUCCESS);
    }

    let started = Instant::now();
    let transport = match with_sigint(BleTransport::connect(options), wait_for_sigint()).await {
        SigintOutcome::Finished(Ok(transport)) => transport,
        SigintOutcome::Finished(Err(error)) => {
            eprintln!("keepalive error after 0.0s: {error}");
            return Ok(ExitCode::FAILURE);
        }
        SigintOutcome::Interrupted => {
            warn_sigint_disconnects();
            return Ok(ExitCode::from(EXIT_INTERRUPTED));
        }
    };
    let session = Session::new(transport);

    // Race the whole keepalive (including its initial 0x0007 enable and the
    // poll loop) against SIGINT. The inner shutdown stays pending so this
    // outer race is the only signal consumer.
    let outcome = with_sigint(
        run_keepalive(&session, interval, duration, std::future::pending::<()>()),
        wait_for_sigint(),
    )
    .await;
    let elapsed = started.elapsed().as_secs_f64();
    match outcome {
        SigintOutcome::Finished(result) => {
            best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
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
        SigintOutcome::Interrupted => {
            // The cancelled loop did not reach its own report disable.
            best_effort_disable_reports(&session, CLEANUP_TIMEOUT).await;
            best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
            warn_sigint_disconnects();
            Ok(ExitCode::from(EXIT_INTERRUPTED))
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
    best_effort_disable_reports(session, CLEANUP_TIMEOUT).await;
    result
}

/// Per-frame wait for the top-level `keepalive` notification drain.
///
/// It matches the official app's 20 ms poll cadence and bounds how long the
/// loop can block before it services its tickers or the shutdown future.
const KEEPALIVE_FRAME_WAIT: Duration = Duration::from_millis(20);

/// Poll `0x0003` at `interval` and drain every incoming notification.
///
/// The official Android app sends `ReadKeysAndJoy` every 20 ms in the
/// foreground, with no response timeout or retry. Notifications are consumed
/// here so the 64-slot BLE queue cannot fill up.
///
/// The drain goes through [`Session::try_next_input`] so this command uses the
/// same policy as the `session` keepalive: one corrupt frame is skipped, and
/// only a run of malformed frames without a usable frame or an idle return
/// ends the command.
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
            report = session.try_next_input(KEEPALIVE_FRAME_WAIT) => {
                if report?.is_some() {
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

/// Shared counters, failure state, and shutdown signalling for the session
/// keepalive task.
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

/// Per-tick input wait inside the session keepalive drain.
const KEEPALIVE_INPUT_WAIT: Duration = Duration::from_millis(1);

/// Maximum input reports drained in one session keepalive tick.
///
/// The official poll answers at most one report per `0x0003`; the cap only
/// bounds a backlog so one tick cannot hold the operation lock indefinitely.
const SESSION_KEEPALIVE_MAX_DRAIN: u64 = 4;

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
        // Never leave an orphaned keepalive task behind: cancel it and abort
        // the handle (every call inside a tick is cancellation-safe).
        self.token.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SessionCommand {
    Info,
    ConfigDump {
        raw: Option<PathBuf>,
        json: Option<PathBuf>,
    },
    ConfigSet {
        field: String,
        value: String,
        slot: u8,
        backup_dir: Option<PathBuf>,
        yes: bool,
        write_chunk: Option<u16>,
    },
    ConfigRemap {
        from: String,
        to: String,
        slot: u8,
        backup_dir: Option<PathBuf>,
        yes: bool,
        write_chunk: Option<u16>,
    },
    ConfigMapShow {
        slot: u8,
    },
    ProfileSwitch {
        slot: u8,
        yes: bool,
    },
    ConfigWrite {
        file: PathBuf,
        backup_dir: Option<PathBuf>,
        accept_reserved_drift: bool,
        force_raw: bool,
        yes: bool,
        write_chunk: Option<u16>,
    },
    Backup {
        raw: Option<PathBuf>,
        json: Option<PathBuf>,
    },
    MacroBackup {
        dir: PathBuf,
    },
    MacroWrite {
        slot: u8,
        index: u8,
        file: PathBuf,
        trigger: Option<String>,
        chunk_size: Option<u16>,
        layout: MacroWriteLayout,
    },
    MacroRestore {
        dir: PathBuf,
        force: bool,
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

/// Parse `--raw FILE` / `--json FILE` pairs.
///
/// Options are strict: a repeated option, a missing value, or a value that
/// looks like another option (`--json`) rejects the whole command instead of
/// silently letting the later value win.
fn parse_file_options<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<(Option<PathBuf>, Option<PathBuf>)> {
    fn value<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Option<PathBuf> {
        let value = parts.next()?;
        if value.starts_with("--") {
            return None;
        }
        Some(PathBuf::from(value))
    }

    let mut raw = None;
    let mut json = None;
    while let Some(part) = parts.next() {
        match part {
            "--raw" => {
                if raw.is_some() {
                    return None;
                }
                raw = Some(value(&mut parts)?);
            }
            "--json" => {
                if json.is_some() {
                    return None;
                }
                json = Some(value(&mut parts)?);
            }
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
        "config" => parse_config_session_command(parts)
            .unwrap_or_else(|| SessionCommand::Unknown(line.to_string())),
        "profile" => parse_profile_session_command(parts)
            .unwrap_or_else(|| SessionCommand::Unknown(line.to_string())),
        "backup" => match parse_file_options(parts) {
            Some((raw, json)) => SessionCommand::Backup { raw, json },
            None => SessionCommand::Unknown(line.to_string()),
        },
        "macro" => parse_macro_session_command(parts)
            .unwrap_or_else(|| SessionCommand::Unknown(line.to_string())),
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

/// One positional REPL value; option-looking tokens are rejected so a missing
/// argument (`macro backup --force`) cannot silently become a path.
fn session_plain_value<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let value = parts.next()?;
    (!value.starts_with("--")).then_some(value)
}

/// Parse the `profile ...` REPL subcommand (`profile switch <N> [--yes]`).
fn parse_profile_session_command<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<SessionCommand> {
    if parts.next()? != "switch" {
        return None;
    }
    let slot: u8 = session_plain_value(&mut parts)?.parse().ok()?;
    let mut yes = false;
    for part in parts {
        match part {
            "--yes" if !yes => yes = true,
            _ => return None,
        }
    }
    Some(SessionCommand::ProfileSwitch { slot, yes })
}

/// Parse the `config ...` REPL subcommands.
///
/// `config dump` keeps its read-only file options. The write subcommands mirror
/// the top-level flags: `--slot`, `--write-chunk`, `--accept-reserved-drift`,
/// `--force-raw`, `--backup-dir` and `--yes`. Values are whitespace tokens, so
/// a value containing spaces (a profile name) needs the top-level command.
fn parse_config_session_command<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<SessionCommand> {
    match parts.next()? {
        "dump" => {
            parse_file_options(parts).map(|(raw, json)| SessionCommand::ConfigDump { raw, json })
        }
        "map" => {
            if parts.next() != Some("show") {
                return None;
            }
            let mut slot = 0u8;
            let mut seen_slot = false;
            while let Some(part) = parts.next() {
                match part {
                    "--slot" if !seen_slot => {
                        slot = session_plain_value(&mut parts)?.parse().ok()?;
                        seen_slot = true;
                    }
                    _ => return None,
                }
            }
            Some(SessionCommand::ConfigMapShow { slot })
        }
        "set" => {
            let field = session_plain_value(&mut parts)?.to_string();
            let value = session_plain_value(&mut parts)?.to_string();
            let (slot, backup_dir, yes, write_chunk) = parse_config_write_options(parts)?;
            Some(SessionCommand::ConfigSet {
                field,
                value,
                slot,
                backup_dir,
                yes,
                write_chunk,
            })
        }
        "remap" => {
            let from = session_plain_value(&mut parts)?.to_string();
            let to = session_plain_value(&mut parts)?.to_string();
            let (slot, backup_dir, yes, write_chunk) = parse_config_write_options(parts)?;
            Some(SessionCommand::ConfigRemap {
                from,
                to,
                slot,
                backup_dir,
                yes,
                write_chunk,
            })
        }
        "write" => {
            let mut file = None;
            let mut backup_dir = None;
            let mut accept_reserved_drift = false;
            let mut force_raw = false;
            let mut yes = false;
            let mut write_chunk = None;
            while let Some(part) = parts.next() {
                match part {
                    "--file" if file.is_none() => {
                        file = Some(PathBuf::from(session_plain_value(&mut parts)?));
                    }
                    "--backup-dir" if backup_dir.is_none() => {
                        backup_dir = Some(PathBuf::from(session_plain_value(&mut parts)?));
                    }
                    "--write-chunk" if write_chunk.is_none() => {
                        write_chunk = Some(session_plain_value(&mut parts)?.parse().ok()?);
                    }
                    "--accept-reserved-drift" if !accept_reserved_drift => {
                        accept_reserved_drift = true;
                    }
                    "--force-raw" if !force_raw => force_raw = true,
                    "--yes" if !yes => yes = true,
                    _ => return None,
                }
            }
            Some(SessionCommand::ConfigWrite {
                file: file?,
                backup_dir,
                accept_reserved_drift,
                force_raw,
                yes,
                write_chunk,
            })
        }
        _ => None,
    }
}

/// Parse the shared `config set` / `config remap` options.
fn parse_config_write_options<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<(u8, Option<PathBuf>, bool, Option<u16>)> {
    let mut slot = 0u8;
    let mut seen_slot = false;
    let mut backup_dir = None;
    let mut yes = false;
    let mut write_chunk = None;
    while let Some(part) = parts.next() {
        match part {
            "--slot" if !seen_slot => {
                slot = session_plain_value(&mut parts)?.parse().ok()?;
                seen_slot = true;
            }
            "--backup-dir" if backup_dir.is_none() => {
                backup_dir = Some(PathBuf::from(session_plain_value(&mut parts)?));
            }
            "--write-chunk" if write_chunk.is_none() => {
                write_chunk = Some(session_plain_value(&mut parts)?.parse().ok()?);
            }
            "--yes" if !yes => yes = true,
            _ => return None,
        }
    }
    Some((slot, backup_dir, yes, write_chunk))
}

/// Parse the `macro ...` REPL subcommands.
///
/// These mirror the top-level `macro backup` / `macro write` / `macro restore`
/// arguments, but the macro write backup directory is not exposed (the
/// timestamped default is used). Options are strict: an unknown option, a
/// repeated option, or a missing value rejects the whole line.
fn parse_macro_session_command<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<SessionCommand> {
    match parts.next()? {
        "backup" => {
            let dir = PathBuf::from(session_plain_value(&mut parts)?);
            parts
                .next()
                .is_none()
                .then_some(SessionCommand::MacroBackup { dir })
        }
        "restore" => {
            let dir = PathBuf::from(session_plain_value(&mut parts)?);
            let mut force = false;
            for part in parts {
                match part {
                    "--force" if !force => force = true,
                    _ => return None,
                }
            }
            Some(SessionCommand::MacroRestore { dir, force })
        }
        "write" => {
            let slot: u8 = parts.next()?.parse().ok()?;
            let index: u8 = parts.next()?.parse().ok()?;
            let file = PathBuf::from(session_plain_value(&mut parts)?);
            let mut trigger = None;
            let mut chunk_size = None;
            let mut write_total_relative = false;
            while let Some(part) = parts.next() {
                match part {
                    "--trigger" if trigger.is_none() => {
                        trigger = Some(session_plain_value(&mut parts)?.to_string());
                    }
                    "--chunk-size" if chunk_size.is_none() => {
                        let requested: u16 = session_plain_value(&mut parts)?.parse().ok()?;
                        if requested != 32 && requested != 64 {
                            return None;
                        }
                        chunk_size = Some(requested);
                    }
                    "--write-total-relative" if !write_total_relative => {
                        write_total_relative = true;
                    }
                    _ => return None,
                }
            }
            Some(SessionCommand::MacroWrite {
                slot,
                index,
                file,
                trigger,
                chunk_size,
                layout: macro_write_layout(write_total_relative),
            })
        }
        _ => None,
    }
}

/// True for the REPL commands that perform a configuration flash write.
fn is_config_write_command(command: &SessionCommand) -> bool {
    matches!(
        command,
        SessionCommand::ConfigSet { .. }
            | SessionCommand::ConfigRemap { .. }
            | SessionCommand::ConfigWrite { .. }
            | SessionCommand::ProfileSwitch { .. }
    )
}

/// True for REPL config commands that run the two-phase
/// prepare (backup / plan / prompt) then TOCTOU-write path.
///
/// `profile switch` is deliberately **not** in this set: it sends one
/// payload-less `0x0015`, takes no pre-write backup and has no plan, so it is
/// dispatched through the normal single-phase path. It still needs
/// `--allow-config-write` (see [`is_config_write_command`]) because it changes
/// controller state.
fn is_prepared_config_write(command: &SessionCommand) -> bool {
    matches!(
        command,
        SessionCommand::ConfigSet { .. }
            | SessionCommand::ConfigRemap { .. }
            | SessionCommand::ConfigWrite { .. }
    )
}

/// True for the two REPL commands that write macro flash.
///
/// These are the commands that must not be cancelled by Ctrl-C, and the ones
/// gated by `--allow-macro-write`.
fn is_macro_flash_command(command: &SessionCommand) -> bool {
    matches!(
        command,
        SessionCommand::MacroWrite { .. } | SessionCommand::MacroRestore { .. }
    )
}

/// The refusal printed when `macro write` / `macro restore` is entered without
/// `--allow-macro-write`.
fn macro_write_disabled_message(command: &SessionCommand) -> Option<&'static str> {
    match command {
        SessionCommand::MacroWrite { .. } => {
            Some("macro write is disabled; restart with --allow-macro-write")
        }
        SessionCommand::MacroRestore { .. } => {
            Some("macro restore is disabled; restart with --allow-macro-write")
        }
        _ => None,
    }
}

fn print_session_help(allow_macro_write: bool, allow_config_write: bool) {
    println!("commands:");
    println!("  info");
    println!("  config dump [--raw FILE] [--json FILE]");
    println!("  config map show [--slot N]");
    if allow_config_write {
        println!("  config set <FIELD> <VALUE> [--slot N] [--write-chunk BYTES] [--yes]");
        println!("  config remap <PHYSICAL> <TARGET> [--slot N] [--write-chunk BYTES] [--yes]");
        println!("  profile switch <0|1|2> [--yes]");
        println!(
            "  config write --file <PATH> [--backup-dir DIR] [--write-chunk BYTES] \
             [--accept-reserved-drift] [--force-raw] [--yes]"
        );
    } else {
        println!("  config set ... (disabled; restart with --allow-config-write)");
        println!("  config remap ... (disabled; restart with --allow-config-write)");
        println!("  profile switch ... (disabled; restart with --allow-config-write)");
        println!("  config write ... (disabled; restart with --allow-config-write)");
    }
    println!("  backup [--raw FILE] [--json FILE]");
    println!(
        "  note: REPL values are whitespace-separated; use the top-level config set for a \
         profile name containing spaces"
    );
    println!("  macro backup <DIR>");
    if allow_macro_write {
        println!(
            "  macro write <SLOT> <INDEX> <FILE> [--trigger <KEY>] [--chunk-size 32|64] \
             [--write-total-relative]"
        );
        println!("  macro restore <DIR> [--force]");
    } else {
        println!("  macro write ... (disabled; restart with --allow-macro-write)");
        println!("  macro restore ... (disabled; restart with --allow-macro-write)");
    }
    println!("  monitor [--count N]");
    println!("  monitor stop");
    println!("  status");
    println!("  help");
    println!("  quit | exit");
    println!(
        "monitor runs in the background until 'monitor stop' or Ctrl-C; Ctrl-C exits the session."
    );
    println!(
        "monitor --count N returns after N reports; without responses it can wait indefinitely."
    );
    println!(
        "macro write/restore hold the operation lock: the keepalive pauses during the operation \
         and resumes after it; do not press Ctrl-C during a write."
    );
    println!(
        "note: REPL 'macro write' has no --backup-dir; it always uses the timestamped default \
         pre-write backup directory in the current directory. Use top-level \
         'eightbitdo macro write --backup-dir DIR' to choose the path."
    );
    println!(
        "note: REPL 'macro restore' has no --chunk-size; the write chunk size is selected from \
         the MTU. Use top-level 'eightbitdo macro restore --chunk-size 32|64' to force it."
    );
    println!(
        "experiment: Write total_len defaults to the absolute end address \
         (macro_index << 12) + align32(steps*10) (BLE, macro_idx=0/1 on slot S1 \
         device-confirmed; other indices/slots extrapolated); '--write-total-relative' reverts \
         to align32(steps*10). Write offsets are always sector-based; a relative-offset mode is \
         not offered because Erase clears (macro_index << 12)."
    );
}

/// Format an unknown REPL line for the terminal.
///
/// The line is debug-escaped so ANSI escapes and control bytes from a paste
/// cannot repaint the terminal or forge extra output lines.
fn unknown_command_message(line: &str) -> String {
    format!("unknown command: {line:?}; type 'help'")
}

/// Start the official-style keepalive on a shared session.
///
/// `session` is the single operation lock: keepalive ticks and REPL commands
/// all take this mutex, so a command never races a keepalive drain for a
/// response.
///
/// Production goes through [`start_session_keepalive_with_config`] so the
/// `--no-report-enable` decision is explicit; this default-ON wrapper is kept
/// for the tests that only need a running keepalive.
#[cfg(test)]
async fn start_session_keepalive<T: Transport + 'static>(
    session: Arc<AsyncMutex<Session<T>>>,
    interval: Duration,
) -> Result<SessionKeepalive, Error> {
    start_session_keepalive_with_config(
        session,
        SessionKeepaliveConfig::with_interval(interval),
        true,
    )
    .await
}

fn spawn_session_keepalive<T: Transport + 'static>(
    session: Arc<AsyncMutex<Session<T>>>,
    config: SessionKeepaliveConfig,
) -> SessionKeepalive {
    let state = Arc::new(SessionKeepaliveState::new());
    let task_state = Arc::clone(&state);
    let token = CancellationToken::new();
    let task_token = token.clone();
    let task = tokio::spawn(session_keepalive_task(
        session, config, task_state, task_token,
    ));
    SessionKeepalive {
        state,
        token,
        task: Some(task),
    }
}

async fn start_session_keepalive_with_config<T: Transport + 'static>(
    session: Arc<AsyncMutex<Session<T>>>,
    config: SessionKeepaliveConfig,
    enable_reports: bool,
) -> Result<SessionKeepalive, Error> {
    if config.interval.is_zero() {
        return Err(Error::Transport(
            "session interval must be greater than zero".to_string(),
        ));
    }
    if enable_reports {
        let guard = session.lock().await;
        guard.set_report_enable(true).await?;
    }
    Ok(spawn_session_keepalive(session, config))
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
            biased;
            _ = token.cancelled() => break,
            _ = ticker.tick() => {}
        }
        if token.is_cancelled() {
            break;
        }

        let guard = tokio::select! {
            biased;
            _ = token.cancelled() => break,
            guard = session.lock() => guard,
        };
        if token.is_cancelled() {
            break;
        }

        let result = async {
            guard.request_input().await?;
            state.sent.fetch_add(1, Ordering::SeqCst);
            let mut drained = 0u64;
            // Bounded per tick: `try_next_input` caps its own frame count and
            // budget, and this cap keeps a notification flood from stretching
            // one tick (and the session-lock hold) beyond a few milliseconds.
            while drained < SESSION_KEEPALIVE_MAX_DRAIN {
                match guard.try_next_input(KEEPALIVE_INPUT_WAIT).await {
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
                *state
                    .last_error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
    async fn stop_with_timeout(mut self, timeout: Duration) {
        self.token.cancel();
        if let Some(mut task) = self.task.take() {
            if tokio::time::timeout(timeout, &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
            }
        }
    }
}

fn keepalive_degraded_error(state: &SessionKeepaliveState) -> Option<Error> {
    if !state.degraded.load(Ordering::SeqCst) {
        return None;
    }
    Some(Error::Transport(format!(
        "keepalive degraded (consecutive failures {}): {}. The last few keepalive ticks failed \
         with I/O errors (a missing input report is normal idle and is not counted as a \
         failure). The controller left BLE App Mode, so `{APP_BLE_NAME}` no longer advertises \
         and this session does not auto-reconnect. Press Profile on the controller for about \
         3 seconds to re-enter App Mode, then start a new `eightbitdo session`",
        state.consecutive_failures.load(Ordering::SeqCst),
        state
            .last_error()
            .unwrap_or_else(|| "unknown error".to_string())
    )))
}

/// Process exit code for a degraded keepalive.
///
/// This is deliberately distinct from `1` (ordinary error): the resident loop
/// must stop and wait for a physical Profile press instead of reconnecting,
/// because a disconnected controller does not advertise `80ND` at all. No
/// automatic retry can succeed until the user performs the press.
const EXIT_SESSION_DEGRADED: u8 = 3;

/// Exit code for a session that ended for any reason other than a clean
/// `quit`.
const EXIT_SESSION_ERROR: u8 = 1;

/// Exit code for a top-level BLE command interrupted by SIGINT.
///
/// `130` is the conventional shell status for a process terminated by
/// `SIGINT`; the command itself still prints the App Mode press warning first.
const EXIT_INTERRUPTED: u8 = 130;

/// `scan --check` result when no App Mode (`80ND`) device is visible.
const EXIT_SCAN_NO_APP_MODE: u8 = 3;

/// Map a finished REPL result and the keepalive state to a process exit code.
///
/// Kept as a pure function so the degraded/clean/error mapping is testable
/// without a controller.
fn session_exit_code(result: &Result<(), Error>, degraded: bool) -> u8 {
    if degraded {
        EXIT_SESSION_DEGRADED
    } else if result.is_ok() {
        0
    } else {
        EXIT_SESSION_ERROR
    }
}

/// Tell the operator that SIGINT is about to drop the BLE link.
///
/// The session is the only owner of the link while it runs; exiting hands the
/// controller back to its normal mode and `80ND` disappears, so the next
/// session needs a physical Profile press. SIGINT is still honoured (it is an
/// explicit user action); the warning exists so the press is expected.
fn warn_sigint_disconnects() {
    warn_app_mode_requires_press("SIGINT received; exiting closes the BLE link and exits App Mode");
}

/// EOF on the control pipe means the same lost link as `quit`/SIGINT.
fn warn_eof_disconnects() {
    warn_app_mode_requires_press(
        "stdin ended (EOF); exiting closes the BLE link and exits App Mode",
    );
}

fn warn_app_mode_requires_press(reason: &str) {
    eprintln!("{}", app_mode_press_message(reason));
}

/// Recovery guidance after a flash write was interrupted by SIGINT.
///
/// The write itself is allowed to finish (it is not cancellable), but the
/// operator still needs to confirm the device state and know how to restore
/// the pre-write backup if the write was partially applied.
fn warn_flash_interrupted_recovery() {
    eprintln!("{}", flash_interrupted_recovery_message());
}

/// Recovery text for [`warn_flash_interrupted_recovery`].
///
/// Separate from printing so the required recovery commands and the
/// read-back instruction are testable.
fn flash_interrupted_recovery_message() -> &'static str {
    "warning: SIGINT interrupted a flash write. The write was allowed to finish, but the \
     device may be partially updated: verify it with a read-back, and if the write failed \
     or the data looks wrong, restore the pre-write backup with \
     `eightbitdo macro restore <DIR>` (macro) or \
     `eightbitdo config write --file <BACKUP>/config.bin` (config), then read it back."
}

/// The operator-facing warning shared by SIGINT, EOF and a lost link.
///
/// Kept separate from the printing so the required content (`80ND` stops
/// advertising, Profile ~3 seconds) is testable without capturing stderr.
fn app_mode_press_message(reason: &str) -> String {
    format!(
        "note: {reason}. The controller stops advertising `{APP_BLE_NAME}`; to re-enter \
         App Mode, press Profile on the controller for about 3 seconds before starting a \
         new session."
    )
}

async fn shutdown_session<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: SessionKeepalive,
) {
    shutdown_session_with_timeout(session, keepalive, CLEANUP_TIMEOUT).await;
}

async fn shutdown_session_with_timeout<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: SessionKeepalive,
    timeout: Duration,
) {
    keepalive.stop_with_timeout(timeout).await;
    let guard = match tokio::time::timeout(timeout, session.lock()).await {
        Ok(guard) => guard,
        Err(_) => {
            // A monitor or command is holding the operation lock. Do not hang
            // shutdown waiting for it; the link will be dropped with the
            // process.
            eprintln!(
                "session shutdown: operation lock still held after {:.1}s; skipping report disable and disconnect",
                timeout.as_secs_f64()
            );
            return;
        }
    };
    // Every exit path that reaches the lock ends with both steps: a failed
    // report disable must not skip the disconnect.
    best_effort_disable_reports(&guard, timeout).await;
    best_effort_disconnect(guard.transport(), timeout).await;
}

#[derive(Debug, PartialEq, Eq)]
enum MonitorPollOutcome {
    CountReached(usize),
    Stopped,
    Quit,
}

/// Send/receive counters for one monitor run.
///
/// The background session monitor shares these with the REPL so `status` can
/// report its traffic without racing the operation lock.
#[derive(Debug, Default)]
struct MonitorStats {
    sent: AtomicU64,
    received: AtomicU64,
}

impl MonitorStats {
    fn sent(&self) -> u64 {
        self.sent.load(Ordering::SeqCst)
    }

    fn received(&self) -> u64 {
        self.received.load(Ordering::SeqCst)
    }
}

async fn monitor_poll<T, F>(
    session: &Session<T>,
    interval: Duration,
    count: Option<u64>,
    stop: &Notify,
    ctrl_c: &mut Pin<&mut F>,
    stats: &MonitorStats,
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
            biased;
            _ = stop.notified() => return Ok(MonitorPollOutcome::Stopped),
            _ = ctrl_c.as_mut() => return Ok(MonitorPollOutcome::Quit),
            _ = tokio::time::sleep_until(next_send) => {}
        }

        tokio::select! {
            biased;
            _ = stop.notified() => return Ok(MonitorPollOutcome::Stopped),
            _ = ctrl_c.as_mut() => return Ok(MonitorPollOutcome::Quit),
            result = session.request_input() => {
                result?;
                stats.sent.fetch_add(1, Ordering::SeqCst);
            }
        }
        next_send = tokio::time::Instant::now() + interval;

        let wait = next_send
            .saturating_duration_since(tokio::time::Instant::now())
            .max(Duration::from_millis(1));
        let report = tokio::select! {
            biased;
            _ = stop.notified() => return Ok(MonitorPollOutcome::Stopped),
            _ = ctrl_c.as_mut() => return Ok(MonitorPollOutcome::Quit),
            report = session.try_next_input(wait) => report?,
        };
        if let Some(report) = report {
            stats.received.fetch_add(1, Ordering::SeqCst);
            print_report(&report);
            printed += 1;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MonitorError {
    disconnected: bool,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MonitorExitStatus {
    Clean,
    Disconnected(String),
    Error(String),
    Panicked(String),
    Aborted,
}

struct NotifyOnDrop {
    done: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }
}

struct SessionMonitor {
    stop: Arc<Notify>,
    finished: Arc<Notify>,
    done: Arc<AtomicBool>,
    error: Arc<StdMutex<Option<MonitorError>>>,
    stats: Arc<MonitorStats>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for SessionMonitor {
    fn drop(&mut self) {
        // Losing the handle must not leave the task polling in the
        // background: signal a clean stop and abort the handle, which also
        // drops the operation-lock guard it may hold.
        self.stop.notify_one();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl SessionMonitor {
    fn is_finished(&self) -> bool {
        self.done.load(Ordering::SeqCst)
            || self
                .task
                .as_ref()
                .is_some_and(tokio::task::JoinHandle::is_finished)
    }

    fn error(&self) -> Option<MonitorError> {
        self.error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    async fn stop(self) -> MonitorExitStatus {
        self.stop_with_timeout(CLEANUP_TIMEOUT).await
    }

    async fn stop_with_timeout(mut self, timeout: Duration) -> MonitorExitStatus {
        self.stop.notify_one();
        let join_result = if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(timeout, &mut task).await {
                Ok(result) => Some(result),
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    return MonitorExitStatus::Aborted;
                }
            }
        } else {
            None
        };

        if let Some(Err(join_error)) = join_result {
            if join_error.is_panic() {
                return MonitorExitStatus::Panicked(join_error.to_string());
            }
            return MonitorExitStatus::Aborted;
        }

        match self.error() {
            Some(err) if err.disconnected => MonitorExitStatus::Disconnected(err.message),
            Some(err) => MonitorExitStatus::Error(err.message),
            None => MonitorExitStatus::Clean,
        }
    }
}

async fn reap_finished_monitor(monitor: &mut Option<SessionMonitor>) -> Option<MonitorExitStatus> {
    if !monitor.as_ref().is_some_and(SessionMonitor::is_finished) {
        return None;
    }
    let running = monitor.take()?;
    Some(running.stop().await)
}

fn report_monitor_exit(status: &MonitorExitStatus) -> Result<(), Error> {
    match status {
        MonitorExitStatus::Clean => Ok(()),
        MonitorExitStatus::Disconnected(_) => Err(Error::Disconnected),
        MonitorExitStatus::Error(message) => {
            eprintln!("monitor stopped with error: {message}");
            Ok(())
        }
        MonitorExitStatus::Panicked(message) => {
            eprintln!("monitor task panicked: {message}");
            Ok(())
        }
        MonitorExitStatus::Aborted => {
            eprintln!("monitor did not stop in time and was aborted");
            Ok(())
        }
    }
}

fn monitor_stop_stdout_message(
    stopped: Option<&MonitorExitStatus>,
    reaped: Option<&MonitorExitStatus>,
) -> Option<&'static str> {
    match stopped.or(reaped) {
        Some(MonitorExitStatus::Clean) => Some("monitor stopped"),
        Some(_) => None,
        None => Some("no monitor is running"),
    }
}

async fn wait_for_monitor_finish(monitor: Option<&SessionMonitor>) {
    match monitor {
        Some(running) => {
            if running.is_finished() {
                return;
            }
            running.finished.notified().await;
        }
        None => std::future::pending().await,
    }
}

async fn wait_for_keepalive_stop(keepalive: &SessionKeepaliveState) {
    if keepalive.degraded.load(Ordering::SeqCst) {
        return;
    }
    keepalive.stopped_notify.notified().await;
}

fn finalize_session_result(
    result: Result<(), Error>,
    keepalive: &SessionKeepaliveState,
) -> Result<(), Error> {
    if result.is_ok() {
        if let Some(error) = keepalive_degraded_error(keepalive) {
            return Err(error);
        }
    }
    result
}

fn start_background_monitor<T: Transport + 'static>(
    session: Arc<AsyncMutex<Session<T>>>,
    interval: Duration,
) -> SessionMonitor {
    // The monitor does not own the `0x0007` enable. `start_session_keepalive`
    // sends it once before the REPL accepts commands, and the keepalive and
    // this monitor share it. Disabling reports here would silently stop the
    // keepalive's notifications; `shutdown_session` owns the final disable.
    let stop = Arc::new(Notify::new());
    let task_stop = Arc::clone(&stop);
    let finished = Arc::new(Notify::new());
    let task_finished = Arc::clone(&finished);
    let done = Arc::new(AtomicBool::new(false));
    let task_done = Arc::clone(&done);
    let error = Arc::new(StdMutex::new(None));
    let task_error = Arc::clone(&error);
    let stats = Arc::new(MonitorStats::default());
    let task_stats = Arc::clone(&stats);
    let task = tokio::spawn(async move {
        let _finish_guard = NotifyOnDrop {
            done: task_done,
            notify: task_finished,
        };
        let guard = tokio::select! {
            biased;
            _ = task_stop.notified() => return,
            guard = session.lock() => guard,
        };
        let never = std::future::pending::<()>();
        tokio::pin!(never);
        if let Err(err) =
            monitor_poll(&guard, interval, None, &task_stop, &mut never, &task_stats).await
        {
            *task_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(MonitorError {
                disconnected: matches!(err, Error::Disconnected),
                message: err.to_string(),
            });
        }
    });
    SessionMonitor {
        stop,
        finished,
        done,
        error,
        stats,
        task: Some(task),
    }
}

fn monitor_counters(monitor: Option<&SessionMonitor>) -> (u64, u64) {
    monitor.map_or((0, 0), |running| {
        (running.stats.sent(), running.stats.received())
    })
}

/// How `status` describes the keepalive.
///
/// The keepalive takes the operation lock on every tick, so while a
/// background monitor holds that lock the keepalive makes no progress even
/// though its task is alive. Reporting that as a plain "running" hid the
/// difference between a healthy session and one whose keepalive is starved.
fn keepalive_state_label(monitor_running: bool, degraded: bool) -> &'static str {
    if degraded {
        "degraded"
    } else if monitor_running {
        "paused (monitor running)"
    } else {
        "running"
    }
}

/// Build the full `status` report.
///
/// Kept as a pure function so the output is testable without capturing stdout.
#[allow(clippy::too_many_arguments)]
fn status_report(
    degraded: bool,
    mtu: u16,
    keepalive_sent: u64,
    keepalive_received: u64,
    last_error: Option<&str>,
    keepalive_elapsed: Duration,
    session_elapsed: Duration,
    monitor_running: bool,
    monitor_sent: u64,
    monitor_received: u64,
) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "connected: {}",
        if degraded { "false (degraded)" } else { "true" }
    );
    let _ = writeln!(out, "mtu: {mtu}");
    let _ = writeln!(out, "keepalive sent: {keepalive_sent}");
    let _ = writeln!(out, "keepalive received: {keepalive_received}");
    let _ = writeln!(out, "keepalive degraded: {degraded}");
    let _ = writeln!(
        out,
        "keepalive: {}",
        keepalive_state_label(monitor_running, degraded)
    );
    if let Some(error) = last_error {
        let _ = writeln!(out, "keepalive last_error: {error}");
    }
    let _ = writeln!(
        out,
        "keepalive elapsed: {:.1}s",
        keepalive_elapsed.as_secs_f64()
    );
    let _ = writeln!(
        out,
        "session elapsed: {:.1}s",
        session_elapsed.as_secs_f64()
    );
    let _ = writeln!(
        out,
        "monitor: {}",
        if monitor_running { "running" } else { "idle" }
    );
    let _ = writeln!(out, "monitor sent: {monitor_sent}");
    let _ = writeln!(out, "monitor received: {monitor_received}");
    out
}

/// Run `macro backup` while holding the operation lock.
///
/// Named wrappers keep the lock inside the production call rather than in the
/// caller: the REPL arms and the tests execute the same lock-and-run unit.
async fn run_session_macro_backup<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    dir: &Path,
) -> Result<(), Error> {
    let guard = session.lock().await;
    run_macro_backup(&guard, dir).await
}

/// Run `macro write` while holding the operation lock for the whole operation.
///
/// The keepalive task takes the same lock on every 20 ms tick, so no `0x0003`
/// can be sent between the first configuration read and the final Save.
///
/// Ctrl-C is deferred for this entire call, including the pre-write backup
/// reads. That region is bounded by the Session per-frame timeout (5 s) and
/// the config/macro total timeouts (30 s each), so it can take minutes in a
/// pathological timeout case even though it is normally seconds. A follow-up
/// can split the read-only preparation from the flash write to shorten the
/// deferral window.
async fn run_session_macro_write<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    request: &PreparedMacroWrite,
    backup_dir: &Path,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    let guard = session.lock().await;
    run_macro_write(&guard, request, backup_dir, chunk_size).await
}

/// Run `macro restore` while holding the operation lock for the whole
/// operation, including the pre-restore backup and the final verification.
async fn run_session_macro_restore<T: Transport>(
    session: &Arc<AsyncMutex<Session<T>>>,
    dir: &Path,
    force: bool,
) -> Result<(), Error> {
    let guard = session.lock().await;
    run_macro_restore(&guard, dir, force, None, MacroWriteLayout::default_ble()).await
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
async fn execute_session_command<T: Transport + 'static>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: &Arc<SessionKeepaliveState>,
    started: Instant,
    mtu: u16,
    interval: Duration,
    allow_macro_write: bool,
    monitor: &mut Option<SessionMonitor>,
    command: SessionCommand,
) -> Result<SessionOutcome, Error> {
    // Tests and non-REPL callers have no async prompt; config writes without
    // `--yes` are refused by `config_confirmation_for_session`.
    execute_session_command_with_io(
        session,
        keepalive,
        started,
        mtu,
        interval,
        allow_macro_write,
        false,
        monitor,
        command,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_session_command_with_io<T: Transport + 'static>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: &Arc<SessionKeepaliveState>,
    started: Instant,
    mtu: u16,
    interval: Duration,
    allow_macro_write: bool,
    allow_config_write: bool,
    monitor: &mut Option<SessionMonitor>,
    command: SessionCommand,
    mut repl_reader: Option<&mut (dyn tokio::io::AsyncBufRead + Unpin + Send)>,
    mut repl_line_state: Option<&mut LineState>,
) -> Result<SessionOutcome, Error> {
    // The read-only default is enforced before touching the session: a
    // disabled write/restore must not send even one frame.
    if !allow_macro_write {
        if let Some(message) = macro_write_disabled_message(&command) {
            eprintln!("{message}");
            return Ok(SessionOutcome::Continue);
        }
    }
    if !allow_config_write && is_config_write_command(&command) {
        eprintln!(
            "config writes are disabled; restart with --allow-config-write \
             (config dump / config map show need no flag)"
        );
        return Ok(SessionOutcome::Continue);
    }
    let reaped = reap_finished_monitor(monitor).await;
    if let Some(status) = &reaped {
        report_monitor_exit(status)?;
    }
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
            validate_output_paths(raw.as_deref(), json.as_deref())?;
            let guard = session.lock().await;
            let snapshot = guard.read_config_snapshot().await?;
            let sha256 = write_config_outputs(&snapshot, raw.as_deref(), json.as_deref())?;
            print_config_summary(&snapshot, &sha256);
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::ConfigSet {
            field,
            value,
            slot,
            backup_dir,
            yes,
            write_chunk,
        } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            // Parse and validate before the lock: a bad field/value must not
            // stall or touch the controller.
            let slot = parse_slot(slot)?;
            let set = parse_config_set(&field, &value, slot)?;
            validate_write_chunk_preconnect(write_chunk)?;
            let confirmation =
                config_confirmation_for_session(yes, &mut repl_reader, &mut repl_line_state)?;
            run_session_config_set(
                session,
                &set,
                backup_dir.as_deref(),
                confirmation,
                write_chunk,
            )
            .await?;
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::ConfigRemap {
            from,
            to,
            slot,
            backup_dir,
            yes,
            write_chunk,
        } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            let slot = parse_slot(slot)?;
            let button = parse_physical_button(&from)?;
            let target = parse_remap_target(&to)?;
            validate_write_chunk_preconnect(write_chunk)?;
            let confirmation =
                config_confirmation_for_session(yes, &mut repl_reader, &mut repl_line_state)?;
            run_session_config_remap(
                session,
                slot,
                button,
                target,
                backup_dir.as_deref(),
                confirmation,
                write_chunk,
            )
            .await?;
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::ConfigMapShow { slot } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            let slot = parse_slot(slot)?;
            let guard = session.lock().await;
            let snapshot = guard.read_config_snapshot().await?;
            for line in mapping_lines(&snapshot.config, slot) {
                println!("{line}");
            }
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::ProfileSwitch { slot, yes } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            let slot = parse_slot(slot)?;
            let mut confirmation =
                config_confirmation_for_session(yes, &mut repl_reader, &mut repl_line_state)?;
            if !confirmation.confirm_with(PROFILE_CONFIRM_PROMPT).await? {
                eprintln!("aborted; no profile switch was sent");
                return Ok(SessionOutcome::Continue);
            }
            let guard = session.lock().await;
            guard.switch_profile(slot).await?;
            println!(
                "sent WriteSLOT slot {} (profile {}) (cmd 0x0015, no Save); acknowledged but \
                 unverified",
                slot.index(),
                slot.index() + 1
            );
            println!(
                "note: ACK shape, Save requirement and persistence are unconfirmed on hardware"
            );
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::ConfigWrite {
            file,
            backup_dir,
            accept_reserved_drift,
            force_raw,
            yes,
            write_chunk,
        } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            validate_write_chunk_preconnect(write_chunk)?;
            let edited = read_and_validate_config_file(&file, force_raw)?;
            let confirmation =
                config_confirmation_for_session(yes, &mut repl_reader, &mut repl_line_state)?;
            run_session_config_write(
                session,
                &edited,
                backup_dir.as_deref(),
                accept_reserved_drift,
                confirmation,
                write_chunk,
            )
            .await?;
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Backup { raw, json } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            validate_output_paths(raw.as_deref(), json.as_deref())?;
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
        SessionCommand::MacroBackup { dir } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            // Read-only on the controller, so no --allow-macro-write needed.
            run_session_macro_backup(session, &dir).await?;
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::MacroWrite {
            slot,
            index,
            file,
            trigger,
            chunk_size,
            layout,
        } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            // Parse the file and validate slot/index/trigger before the lock:
            // a bad request must not stall (or touch) the controller.
            let request = prepare_macro_write(slot, index, &file, trigger.as_deref(), layout)?;
            let backup_dir = resolve_write_backup_dir(None);
            // `run_session_macro_write` holds the operation lock from the first
            // configuration read to the final Save, so the 20 ms keepalive
            // cannot interleave a 0x0003.
            run_session_macro_write(session, &request, &backup_dir, chunk_size).await?;
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::MacroRestore { dir, force } => {
            if monitor.is_some() {
                eprintln!("monitor is running; use 'monitor stop' first");
                return Ok(SessionOutcome::Continue);
            }
            eprintln!(
                "warning: do not press Ctrl-C during the macro restore; a failure can leave a \
                 sector partially updated. A pre-restore backup of the current controller is \
                 taken before any write."
            );
            run_session_macro_restore(session, &dir, force).await?;
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
            let stats = MonitorStats::default();
            let outcome =
                monitor_poll(&guard, interval, Some(count), &stop, &mut ctrl_c, &stats).await?;
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
            let stopped = if let Some(running) = monitor.take() {
                let status = running.stop().await;
                report_monitor_exit(&status)?;
                Some(status)
            } else {
                None
            };
            if let Some(message) = monitor_stop_stdout_message(stopped.as_ref(), reaped.as_ref()) {
                println!("{message}");
            }
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Status => {
            let degraded = keepalive.degraded.load(Ordering::SeqCst);
            let (monitor_sent, monitor_received) = monitor_counters(monitor.as_ref());
            let report = status_report(
                degraded,
                mtu,
                keepalive.sent.load(Ordering::SeqCst),
                keepalive.received.load(Ordering::SeqCst),
                keepalive.last_error().as_deref(),
                keepalive.started_at.elapsed(),
                started.elapsed(),
                monitor.is_some(),
                monitor_sent,
                monitor_received,
            );
            print!("{report}");
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Help => {
            print_session_help(allow_macro_write, allow_config_write);
            Ok(SessionOutcome::Continue)
        }
        SessionCommand::Quit => Ok(SessionOutcome::Quit),
        SessionCommand::Empty => Ok(SessionOutcome::Continue),
        SessionCommand::Unknown(line) => {
            eprintln!("{}", unknown_command_message(&line));
            Ok(SessionOutcome::Continue)
        }
    }
}

/// Maximum byte length of one REPL command line, excluding the line ending.
///
/// Longer lines are reported once and discarded up to the next newline so the
/// next prompt reads a clean line.
const MAX_SESSION_LINE_BYTES: usize = 4096;

/// Why [`read_session_line`] returned.
#[derive(Debug, PartialEq, Eq)]
enum LineRead {
    /// A line (without its `\n` / `\r\n`) plus whether it was cut at a NUL.
    Line { text: String, cut: bool },
    /// The input ended before a line was complete (EOF).
    Eof,
    /// The line exceeded [`MAX_SESSION_LINE_BYTES`] and the rest was dropped.
    TooLong { discarded: usize },
}

/// Progress of one REPL line across `read_session_line` calls.
///
/// This is caller-owned so that dropping the read future (Ctrl-C, `select!`)
/// loses nothing. In particular `overflow` must survive cancellation with
/// `pending`: otherwise a 4096-byte prefix of a longer line would look like a
/// complete line and could be executed as a command.
#[derive(Debug, Default)]
struct LineState {
    /// Bytes of the current line, up to [`MAX_SESSION_LINE_BYTES`].
    pending: Vec<u8>,
    /// Bytes of the current line beyond the limit: counted, not stored.
    overflow: usize,
    /// True while the only overflow byte is a single `\r` that may be the CR of
    /// a CRLF whose `\n` arrives in the next chunk. Such a byte belongs to the
    /// line ending and must not count against `MAX_SESSION_LINE_BYTES`.
    overflow_is_only_cr: bool,
}

/// Read one line without losing input when the future is cancelled.
///
/// The previous `BufReader::read_line` inside `tokio::select!` lost the
/// partial line on a Ctrl-C and could split a multi-byte character. This
/// version keeps every consumed byte and the over-limit count in the
/// caller-owned [`LineState`] and reads one `fill_buf` at a time, so dropping
/// the future at any `await` point resumes with exactly the progress that was
/// already made (including a line that must still be reported as too long).
///
/// NUL bytes end the line: on Unix a very long paste without a newline is
/// still delivered through the terminal's canonical buffer and
/// NUL-separated, so each chunk becomes its own line instead of one unbounded
/// line.
fn read_session_line<'a, R>(
    reader: &'a mut R,
    state: &'a mut LineState,
) -> impl Future<Output = std::io::Result<LineRead>> + 'a
where
    R: tokio::io::AsyncBufRead + Unpin + ?Sized,
{
    read_session_line_inner(reader, state)
}

async fn read_session_line_inner<R>(
    reader: &mut R,
    state: &mut LineState,
) -> std::io::Result<LineRead>
where
    R: tokio::io::AsyncBufRead + Unpin + ?Sized,
{
    use tokio::io::AsyncBufReadExt as _;

    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            // Only an empty `fill_buf` is EOF. A consumed newline/NUL with no
            // bytes before it is an empty line, not the end of the stream.
            return finish_session_line(state, false);
        }

        let newline = buffer.iter().position(|&byte| byte == b'\n');
        let nul = buffer.iter().position(|&byte| byte == b'\0');
        let (end, cut) = match (newline, nul) {
            (Some(newline), Some(nul)) if nul < newline => (nul, true),
            (Some(newline), _) => (newline, false),
            (None, Some(nul)) => (nul, true),
            (None, None) => (buffer.len(), false),
        };
        let consumed = end + usize::from(end < buffer.len());
        // A `\r` immediately before a `\n` in this same buffer is part of the
        // line ending, so it must not count against MAX_SESSION_LINE_BYTES.
        // Without a terminator in this buffer a trailing `\r` is ordinary
        // content: the next chunk may be `--raw x`, and dropping the CR would
        // turn `backup\r--raw x` into the unknown command `backup--raw x`.
        // A CRLF split across chunks is handled by the `end == 0` correction
        // below.
        let content_end = if !cut && end < buffer.len() && end > 0 && buffer[end - 1] == b'\r' {
            end - 1
        } else {
            end
        };

        let room = MAX_SESSION_LINE_BYTES.saturating_sub(state.pending.len());
        let accepted = room.min(content_end);
        state.pending.extend_from_slice(&buffer[..accepted]);
        let dropped = &buffer[accepted..content_end];
        if !dropped.is_empty() {
            state.overflow += dropped.len();
            state.overflow_is_only_cr = state.overflow == 1 && dropped == b"\r";
        }
        reader.consume(consumed);

        if !cut && end == 0 {
            // The CR of a CRLF was consumed by the previous chunk: it is
            // already in `pending`, or it was the only overflow byte.
            if state.overflow == 0 && state.pending.last() == Some(&b'\r') {
                state.pending.pop();
            } else if state.overflow_is_only_cr {
                state.overflow -= 1;
            }
        }

        // A NUL (possibly a long paste) or a newline ends the line.
        if cut || consumed > end {
            let mut read = finish_session_line(state, true)?;
            if let LineRead::Line { cut: line_cut, .. } = &mut read {
                *line_cut = cut;
            }
            return Ok(read);
        }
    }
}

fn finish_session_line(state: &mut LineState, terminated: bool) -> std::io::Result<LineRead> {
    // `terminated` distinguishes a newline/NUL that ended an empty line from
    // an empty `fill_buf` at EOF. Without it, pressing Enter ended the REPL.
    let overflow = std::mem::take(&mut state.overflow);
    state.overflow_is_only_cr = false;
    if !terminated && state.pending.is_empty() && overflow == 0 {
        return Ok(LineRead::Eof);
    }
    let bytes = std::mem::take(&mut state.pending);
    if overflow > 0 {
        return Ok(LineRead::TooLong {
            discarded: overflow,
        });
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    while text.ends_with(['\r', '\n']) {
        text.pop();
    }
    Ok(LineRead::Line { text, cut: false })
}

async fn session_repl_loop<T: Transport + 'static>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: &Arc<SessionKeepaliveState>,
    started: Instant,
    mtu: u16,
    interval: Duration,
    allow_macro_write: bool,
    allow_config_write: bool,
) -> Result<(), Error> {
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    session_repl_loop_with_io(
        session,
        keepalive,
        started,
        mtu,
        interval,
        allow_macro_write,
        allow_config_write,
        &mut reader,
        ctrl_c,
        interactive,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn session_repl_loop_with_io<T, R, F>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: &Arc<SessionKeepaliveState>,
    started: Instant,
    mtu: u16,
    interval: Duration,
    allow_macro_write: bool,
    allow_config_write: bool,
    reader: &mut R,
    ctrl_c: F,
    interactive: bool,
) -> Result<(), Error>
where
    T: Transport + 'static,
    R: tokio::io::AsyncBufRead + Unpin + Send,
    F: Future<Output = ()>,
{
    enum LoopEvent {
        CtrlC,
        KeepaliveStopped,
        MonitorFinished,
        Line(std::io::Result<LineRead>),
    }

    // Cancellation-safe line state: bytes already consumed from `reader`, and
    // the over-limit count, survive a dropped `read_session_line`.
    let mut line_state = LineState::default();
    tokio::pin!(ctrl_c);
    let mut monitor: Option<SessionMonitor> = None;
    let mut result = Ok(());
    // `ctrl_c` is polled to completion inside the non-cancellable flash branch.
    // A future must not be polled again after completion, so the loop exits
    // right after that command instead of re-entering the outer select.
    let mut ctrl_c_deferred = false;

    loop {
        if interactive {
            print!("8bitdo> ");
            let _ = std::io::stdout().flush();
        }
        let event = tokio::select! {
            biased;
            _ = &mut ctrl_c => LoopEvent::CtrlC,
            _ = wait_for_keepalive_stop(keepalive) => LoopEvent::KeepaliveStopped,
            _ = wait_for_monitor_finish(monitor.as_ref()) => LoopEvent::MonitorFinished,
            read_result = read_session_line(reader, &mut line_state) => LoopEvent::Line(read_result),
        };
        match event {
            LoopEvent::CtrlC => {
                // Ctrl-C while idle: no command future is in flight. If a
                // command was running, its branch below drops the future
                // instead; `Session` marks that through `ResyncGuard` /
                // `InFlight`. Honouring SIGINT is standard, but it costs the
                // App Mode link, so say so before leaving.
                warn_sigint_disconnects();
                break;
            }
            LoopEvent::KeepaliveStopped => {
                if let Some(error) = keepalive_degraded_error(keepalive) {
                    result = Err(error);
                } else {
                    eprintln!("keepalive stopped");
                }
                break;
            }
            LoopEvent::MonitorFinished => {
                if let Some(running) = monitor.take() {
                    let status = running.stop().await;
                    if let Err(error) = report_monitor_exit(&status) {
                        result = Err(error);
                        break;
                    }
                }
            }
            LoopEvent::Line(read_result) => {
                let read = match read_result {
                    Ok(read) => read,
                    Err(error) => {
                        result = Err(Error::Io(error));
                        break;
                    }
                };
                let text = match read {
                    LineRead::Eof => {
                        warn_eof_disconnects();
                        break;
                    }
                    LineRead::TooLong { discarded } => {
                        eprintln!(
                            "line too long: discarded {discarded} bytes (limit {MAX_SESSION_LINE_BYTES} per line); command ignored"
                        );
                        continue;
                    }
                    LineRead::Line { text, cut } => {
                        if cut {
                            eprintln!(
                                "note: line cut at an embedded NUL byte (long paste?); the remainder runs as its own line"
                            );
                        }
                        text
                    }
                };
                let command = parse_session_command(text.trim());
                // Config writes are two-phase: preparation + prompt are
                // Ctrl-C-cancellable (the keepalive continues while the prompt
                // waits), and only the actual 0x0001/Save/read-back section is
                // non-cancellable.
                if allow_config_write && is_prepared_config_write(&command) {
                    if monitor.is_some() {
                        eprintln!("monitor is running; use 'monitor stop' first");
                        continue;
                    }
                    let prepared = tokio::select! {
                        biased;
                        _ = &mut ctrl_c => {
                            warn_sigint_disconnects();
                            eprintln!(
                                "note: Ctrl-C received; configuration preparation was cancelled \
                                 before any 0x0001/Save. If a pre-write backup was created it remains."
                            );
                            break;
                        }
                        result = prepare_repl_config_command(session, &command) => result,
                    };
                    let pending = match prepared {
                        Ok(Some(pending)) => pending,
                        Ok(None) => continue,
                        Err(error) => {
                            if matches!(error, Error::Disconnected) {
                                result = Err(error);
                                break;
                            }
                            eprintln!("error: {error}");
                            continue;
                        }
                    };
                    if pending.needs_confirmation {
                        let answer = tokio::select! {
                            biased;
                            _ = &mut ctrl_c => {
                                warn_sigint_disconnects();
                                eprintln!(
                                    "note: Ctrl-C received; configuration write cancelled before \
                                     any 0x0001/Save. Pre-write backup: {} (plan-time snapshot); \
                                     recover with: {}",
                                    pending.state.backup.display(),
                                    pending.state.recover
                                );
                                break;
                            }
                            answer = confirm_config_write_async(&mut *reader, &mut line_state) => answer,
                        };
                        match answer {
                            Ok(true) => {}
                            Ok(false) => {
                                println!("aborted; no configuration was written");
                                continue;
                            }
                            Err(error) => {
                                if matches!(error, Error::Disconnected) {
                                    result = Err(error);
                                    break;
                                }
                                eprintln!("error: {error}");
                                continue;
                            }
                        }
                    }

                    // Non-cancellable flash section: TOCTOU re-read, 0x0001
                    // chunks, Save and read-back.
                    let mut finish =
                        std::pin::pin!(finish_repl_config_command(session, &pending.state));
                    let outcome = tokio::select! {
                        biased;
                        result = &mut finish => result,
                        _ = &mut ctrl_c => {
                            warn_sigint_disconnects();
                            eprintln!(
                                "note: Ctrl-C received; the configuration flash write \
                                 (0x0001/Save) is not cancellable and will finish before the \
                                 session exits"
                            );
                            ctrl_c_deferred = true;
                            finish.await
                        }
                    };
                    match outcome {
                        Ok(()) => {
                            if ctrl_c_deferred {
                                break;
                            }
                            continue;
                        }
                        Err(error) => {
                            if matches!(error, Error::Disconnected) {
                                result = Err(error);
                                break;
                            }
                            eprintln!("error: {error}");
                            if ctrl_c_deferred {
                                break;
                            }
                            continue;
                        }
                    }
                }
                // A flash write must not be dropped mid-operation: a Ctrl-C
                // that cancels the future between chunks / Erase and Save can
                // leave a sector or configuration partially updated, which is
                // exactly what the pre-write warning tells the operator to
                // avoid. The signal is observed once (with a notice) and then
                // serviced after the command returns by the loop's own Ctrl-C
                // branch.
                let flash_command = (allow_macro_write && is_macro_flash_command(&command))
                    || (allow_config_write && is_prepared_config_write(&command));
                let outcome = if flash_command {
                    let mut command_future = std::pin::pin!(execute_session_command_with_io(
                        session,
                        keepalive,
                        started,
                        mtu,
                        interval,
                        allow_macro_write,
                        allow_config_write,
                        &mut monitor,
                        command,
                        Some(&mut *reader),
                        Some(&mut line_state),
                    ));
                    tokio::select! {
                        biased;
                        outcome = &mut command_future => outcome,
                        _ = &mut ctrl_c => {
                            warn_sigint_disconnects();
                            eprintln!(
                                "note: Ctrl-C received; the flash operation is not cancellable \
                                 and will finish before the session exits"
                            );
                            ctrl_c_deferred = true;
                            command_future.await
                        }
                    }
                } else {
                    tokio::select! {
                        biased;
                        _ = &mut ctrl_c => {
                            // The command future is dropped, not aborted. Dropping
                            // a `Session` read marks it for resync via
                            // `ResyncGuard` (an abandoned input poll does not; a
                            // protocol error from one is marked by `fail()`), so
                            // the next read drains only when a response could
                            // still be in flight. There is no caller-side mark to
                            // clear; `Session::InFlight` makes that decision.
                            warn_sigint_disconnects();
                            break;
                        }
                        outcome = execute_session_command_with_io(
                            session,
                            keepalive,
                            started,
                            mtu,
                            interval,
                            allow_macro_write,
                            allow_config_write,
                            &mut monitor,
                            command,
                            Some(&mut *reader),
                            Some(&mut line_state),
                        ) => outcome,
                    }
                };
                match outcome {
                    Ok(SessionOutcome::Quit) => break,
                    Ok(SessionOutcome::Continue | SessionOutcome::MonitorReports(_)) => {}
                    Err(error) => {
                        if matches!(error, Error::Disconnected) {
                            result = Err(error);
                            break;
                        }
                        eprintln!("error: {error}");
                    }
                }
                if ctrl_c_deferred {
                    // Exit only after the flash command finished; the ctrl_c
                    // future was already driven to completion above.
                    break;
                }
            }
        }
    }

    if let Some(running) = monitor.take() {
        let status = running.stop().await;
        if let Err(error) = report_monitor_exit(&status) {
            if result.is_ok() {
                result = Err(error);
            } else {
                eprintln!("monitor stopped with disconnect: {error}");
            }
        }
    }

    finalize_session_result(result, keepalive)
}

async fn session(
    options: &BleOptions,
    interval: Duration,
    allow_macro_write: bool,
    allow_config_write: bool,
    report_enable: bool,
) -> Result<ExitCode, Error> {
    if interval.is_zero() {
        eprintln!("session error: --interval-ms must be greater than zero");
        return Ok(ExitCode::FAILURE);
    }
    // The connect scan can take seconds and already leaves App Mode when it is
    // interrupted: warn instead of dying silently under the default SIGINT.
    let transport = match with_sigint(BleTransport::connect(options), wait_for_sigint()).await {
        SigintOutcome::Finished(Ok(transport)) => transport,
        SigintOutcome::Finished(Err(error)) => {
            eprintln!("session error: {error}");
            return Ok(ExitCode::FAILURE);
        }
        SigintOutcome::Interrupted => {
            warn_sigint_disconnects();
            return Ok(ExitCode::from(EXIT_INTERRUPTED));
        }
    };
    let session = Arc::new(AsyncMutex::new(Session::new(transport)));
    let mtu = {
        let guard = session.lock().await;
        guard.transport().mtu()
    };
    // The startup `0x0007` enable is also part of the link-owning phase.
    let keepalive_start = start_session_keepalive_with_config(
        Arc::clone(&session),
        SessionKeepaliveConfig::with_interval(interval),
        report_enable,
    );
    let keepalive = match with_sigint(keepalive_start, wait_for_sigint()).await {
        SigintOutcome::Finished(Ok(keepalive)) => keepalive,
        SigintOutcome::Finished(Err(error)) => {
            if let Ok(guard) = tokio::time::timeout(CLEANUP_TIMEOUT, session.lock()).await {
                best_effort_disconnect(guard.transport(), CLEANUP_TIMEOUT).await;
            }
            eprintln!("session error: {error}");
            return Ok(ExitCode::FAILURE);
        }
        SigintOutcome::Interrupted => {
            warn_sigint_disconnects();
            if let Ok(guard) = tokio::time::timeout(CLEANUP_TIMEOUT, session.lock()).await {
                best_effort_disconnect(guard.transport(), CLEANUP_TIMEOUT).await;
            }
            return Ok(ExitCode::from(EXIT_INTERRUPTED));
        }
    };
    // Make the 0x0007 state explicit: a REPL macro write happens while this
    // state is active, which differs from the top-level `macro write` default.
    println!(
        "report enable (0x0007): {}. REPL `macro write` runs in this state; the top-level \
         `eightbitdo macro write` default sends no 0x0007 (use --no-report-enable to match it).",
        if report_enable {
            "on (official default)"
        } else {
            "off (--no-report-enable)"
        }
    );
    let state = Arc::clone(&keepalive.state);
    let started = Instant::now();
    let result = session_repl_loop(
        &session,
        &state,
        started,
        mtu,
        interval,
        allow_macro_write,
        allow_config_write,
    )
    .await;
    shutdown_session(&session, keepalive).await;
    let degraded = state.degraded.load(Ordering::SeqCst);
    let result = finalize_session_result(result, &state);

    if session_exit_code(&result, degraded) == EXIT_SESSION_DEGRADED {
        // Reuse the keepalive message (it names the failure and the Profile
        // press), then make the terminal state and exit code explicit: the
        // resident loop keys on this code to stop instead of reconnecting.
        if let Some(error) = keepalive_degraded_error(&state) {
            eprintln!("session error: {error}");
        }
        eprintln!(
            "session exit code {EXIT_SESSION_DEGRADED}: keepalive degraded; the BLE link is \
             gone and `{APP_BLE_NAME}` is not advertising. Physical controller action \
             (Profile, about 3 seconds) is required before another session can connect. \
             This is not retried automatically."
        );
        return Ok(ExitCode::from(EXIT_SESSION_DEGRADED));
    }

    match result {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) => {
            let disconnected = matches!(error, Error::Disconnected);
            eprintln!("session error: {error}");
            if disconnected {
                // The link was lost outside the keepalive path (a command
                // observed the disconnect). The controller is already out of
                // App Mode, so the same press guidance applies.
                warn_app_mode_requires_press("the BLE link was lost");
            }
            Ok(ExitCode::from(EXIT_SESSION_ERROR))
        }
    }
}

/// Map `--no-report-enable` to the startup `0x0007` decision.
///
/// Kept as a pure function so the flag mapping is testable without a device.
fn session_report_enable(no_report_enable: bool) -> bool {
    !no_report_enable
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
    use eightbitdo_core::MockTransport;
    use eightbitdo_protocol::commands;
    use eightbitdo_protocol::config_bt2::Bt2Config;
    use eightbitdo_protocol::header::CustomHeader;

    use super::*;

    #[test]
    fn poll_interval_maps_milliseconds_to_session_interval() {
        // `--poll-interval-ms 0` disables the keepalive; a positive value is
        // passed through as-is.
        assert_eq!(poll_interval(0), None);
        assert_eq!(poll_interval(20), Some(Duration::from_millis(20)));
    }

    #[test]
    fn macro_session_maps_poll_and_report_enable_flags() {
        let defaults = macro_session(MockTransport::new(), 0, false);
        assert_eq!(defaults.poll_interval(), None);
        assert!(!defaults.report_enable());

        let both = macro_session(MockTransport::new(), 20, true);
        assert_eq!(both.poll_interval(), Some(Duration::from_millis(20)));
        assert!(both.report_enable());

        let enable_only = macro_session(MockTransport::new(), 0, true);
        assert_eq!(enable_only.poll_interval(), None);
        assert!(enable_only.report_enable());
    }

    /// Transport that records writes and replays a finite queue of input
    /// notifications, then stays pending instead of reporting a disconnect.
    #[derive(Clone)]
    struct KeepaliveTestTransport {
        sent: Arc<Mutex<Vec<Vec<u8>>>>,
        incoming: Arc<Mutex<VecDeque<Vec<u8>>>>,
        disconnected: Arc<AtomicBool>,
        fail_input: Arc<AtomicBool>,
        fail_recv: Arc<AtomicBool>,
        disconnect_recv: Arc<AtomicBool>,
        mtu: u16,
    }

    impl KeepaliveTestTransport {
        fn new() -> Self {
            Self {
                sent: Arc::new(Mutex::new(Vec::new())),
                incoming: Arc::new(Mutex::new(VecDeque::new())),
                disconnected: Arc::new(AtomicBool::new(false)),
                fail_input: Arc::new(AtomicBool::new(false)),
                fail_recv: Arc::new(AtomicBool::new(false)),
                disconnect_recv: Arc::new(AtomicBool::new(false)),
                mtu: 64,
            }
        }

        /// Same transport with another negotiated MTU (macro writes need 200).
        fn with_mtu(mtu: u16) -> Self {
            Self { mtu, ..Self::new() }
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

        /// Queue a notification exactly as given, without a header.
        fn queue_raw(&self, frame: &[u8]) {
            self.incoming.lock().unwrap().push_back(frame.to_vec());
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

        fn fail_receives(&self) {
            self.fail_recv.store(true, Ordering::SeqCst);
        }

        fn allow_receives(&self) {
            self.fail_recv.store(false, Ordering::SeqCst);
            self.disconnect_recv.store(false, Ordering::SeqCst);
        }

        fn disconnect_receives(&self) {
            self.disconnect_recv.store(true, Ordering::SeqCst);
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
            if self.disconnect_recv.load(Ordering::SeqCst) {
                return Err(Error::Disconnected);
            }
            if self.fail_recv.load(Ordering::SeqCst) {
                return Err(Error::Transport("monitor receive failure".to_string()));
            }
            if let Some(frame) = self.incoming.lock().unwrap().pop_front() {
                return Ok(frame);
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
            Err(Error::Disconnected)
        }

        fn mtu(&self) -> u16 {
            self.mtu
        }

        async fn disconnect(&self) -> Result<(), Error> {
            self.disconnected.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Transport that answers each scripted macro command with the next frame.
    ///
    /// Responses are queued at `send` time, not before the operation starts.
    /// That matters for the concurrency test: a keepalive tick that wins the
    /// race before the command locks the session drains the notification
    /// queue, so pre-queued replies would be stolen. Here the queue is empty
    /// until the command itself sends a request.
    #[derive(Clone)]
    struct AutoRespondTransport {
        inner: KeepaliveTestTransport,
        script: Arc<Mutex<VecDeque<Vec<u8>>>>,
        served: Arc<Mutex<Vec<u16>>>,
    }

    impl AutoRespondTransport {
        fn new(mtu: u16, script: Vec<Vec<u8>>) -> Self {
            Self {
                inner: KeepaliveTestTransport::with_mtu(mtu),
                script: Arc::new(Mutex::new(script.into())),
                served: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn sent_frames(&self) -> Vec<Vec<u8>> {
            self.inner.sent_frames()
        }

        fn remaining_script_entries(&self) -> usize {
            self.script.lock().unwrap().len()
        }

        fn served_commands(&self) -> Vec<u16> {
            self.served.lock().unwrap().clone()
        }
    }

    /// Commands that consume one scripted response in `AutoRespondTransport`.
    fn consumes_script_entry(command: u16) -> bool {
        matches!(
            command,
            commands::cmd::READ_CONFIG
                | commands::cmd::READ_MACRO
                | commands::cmd::WRITE_CONFIG
                | commands::cmd::SAVE
                | commands::cmd::ERASE_MACRO
                | commands::cmd::WRITE_MACRO
        )
    }

    #[async_trait]
    impl Transport for AutoRespondTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            self.inner.send(data).await?;
            let header = CustomHeader::parse(&data[1..])
                .map_err(|error| Error::Transport(format!("malformed request frame: {error}")))?;
            if consumes_script_entry(header.cmd) {
                let next = self.script.lock().unwrap().pop_front();
                let Some(frame) = next else {
                    return Err(Error::Transport(format!(
                        "response script exhausted at cmd 0x{:04X}",
                        header.cmd
                    )));
                };
                self.served.lock().unwrap().push(header.cmd);
                self.inner.queue_raw(&frame);
            }
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            self.inner.recv().await
        }

        fn mtu(&self) -> u16 {
            self.inner.mtu()
        }

        async fn disconnect(&self) -> Result<(), Error> {
            self.inner.disconnect().await
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

    #[tokio::test]
    async fn keepalive_skips_one_malformed_frame_and_counts_the_next_report() {
        // M-1 regression: the top-level `keepalive` used to propagate a decode
        // error from a single corrupt notification and exit. It now follows
        // the session keepalive's skip policy.
        let transport = KeepaliveTestTransport::new();
        transport.queue_raw(&[0xEE; 5]);
        transport.queue_input(&sample_input_report());

        let session = Session::new(&transport);
        let stats = run_keepalive(
            &session,
            Duration::from_millis(1),
            Duration::from_millis(100),
            std::future::pending::<()>(),
        )
        .await
        .expect("one malformed frame must not end the keepalive");
        assert_eq!(
            stats.received, 1,
            "the report after the malformed frame must be counted"
        );
        assert_eq!(session.skipped_frames(), 1);
    }

    #[tokio::test]
    async fn keepalive_fails_after_a_run_of_malformed_frames() {
        // The same run limit as the session keepalive: a link that only
        // delivers garbage still ends the command instead of spinning.
        let transport = KeepaliveTestTransport::new();
        for _ in 0..eightbitdo_core::MAX_CONSECUTIVE_DECODE_FAILURES {
            transport.queue_raw(&[0xEE; 5]);
        }

        let session = Session::new(&transport);
        let result = run_keepalive(
            &session,
            Duration::from_millis(1),
            Duration::from_millis(100),
            std::future::pending::<()>(),
        )
        .await;
        assert!(
            matches!(result, Err(Error::Protocol(_))),
            "a run of malformed frames must end the keepalive, got {result:?}"
        );
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

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn write_config_outputs_hashes_the_2768_byte_buffer() {
        let snapshot = ConfigSnapshot {
            config: Bt2Config::zeroed(),
            stored_crc: 0,
            computed_crc: 0,
            crc_valid: true,
        };
        assert!(snapshot.config.as_bytes().iter().all(|&byte| byte == 0));
        let digest = write_config_outputs(&snapshot, None, None).unwrap();
        assert_eq!(
            digest,
            "ec5e098511f8ba7b974422f9bfa76e3f1fe350ccb70401174671c5eb5e50b74e"
        );
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
        for frame in config_response_frames(config) {
            transport.queue_raw(&frame);
        }
    }

    /// Build the 20 READ_CONFIG response frames for `config`, in offset order.
    fn config_response_frames(config: &Bt2Config) -> Vec<Vec<u8>> {
        let bytes = config.as_bytes();
        let mut frames = Vec::with_capacity(bytes.len() / 142 + 1);
        for (index, chunk) in bytes.chunks(142).enumerate() {
            let offset = (index * 142) as u32;
            let header = CustomHeader::new(
                commands::cmd::RESPONSE,
                commands::cmd::READ_CONFIG,
                CONFIG_SIZE as u32,
                offset,
            )
            .with_payload(chunk);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(chunk);
            frames.push(frame);
        }
        frames
    }

    /// Build one payload-less ACK frame for `command`.
    fn ack_frame(command: u16) -> Vec<u8> {
        CustomHeader::new(command, 0, 0, 0).to_bytes().to_vec()
    }

    /// Build the READ_MACRO response frames for one raw step payload.
    fn macro_read_frames(raw: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let mut offset = 0u32;
        for chunk in raw.chunks(142) {
            let mut header = CustomHeader::new(
                commands::cmd::RESPONSE,
                commands::cmd::READ_MACRO,
                raw.len() as u32,
                offset,
            )
            .with_chunk_len(chunk.len() as u16);
            header.crc16 = eightbitdo_protocol::crc::packet_crc16(chunk);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(chunk);
            frames.push(frame);
            offset += chunk.len() as u32;
        }
        frames
    }

    fn commands_of(frames: &[Vec<u8>]) -> Vec<u16> {
        frames
            .iter()
            .map(|frame| CustomHeader::parse(&frame[1..]).unwrap().cmd)
            .collect()
    }

    fn sent_commands(transport: &KeepaliveTestTransport) -> Vec<u16> {
        commands_of(&transport.sent_frames())
    }

    fn sent_command_count(transport: &KeepaliveTestTransport, command: u16) -> usize {
        sent_commands(transport)
            .into_iter()
            .filter(|sent| *sent == command)
            .count()
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
            false,
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
            false,
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
            false,
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

    /// Default pre-write backup directories currently visible in the CWD,
    /// used to find (and remove) the directory a REPL write created.
    fn default_backup_dirs_in_cwd() -> std::collections::BTreeSet<PathBuf> {
        std::fs::read_dir(".")
            .expect("read the CWD")
            .filter_map(std::result::Result::ok)
            .map(|entry| PathBuf::from(entry.file_name()))
            .filter(|path| {
                path.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.starts_with("macro-backup-"))
            })
            .collect()
    }

    /// Remove a directory even when an assertion later in the test panics.
    struct RemoveDirOnDrop(PathBuf);

    impl Drop for RemoveDirOnDrop {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// End-to-end session macro write through `execute_session_command`.
    ///
    /// The keepalive ticks every 20 ms while the write runs (~450 ms of
    /// official settle delays), so the handler's operation lock is what keeps
    /// `0x0003` off the wire between the first configuration read and the
    /// final read-back. If the lock is missing (or the keepalive ignores it),
    /// INPUT frames appear inside that frame window and this test fails.
    #[tokio::test]
    async fn session_macro_write_pauses_and_resumes_the_keepalive() {
        let root = scratch_dir("session-macro-keepalive");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture(&fixture_dir);
        let macro_file = root.join("edit.json");
        std::fs::write(&macro_file, serde_json::to_vec(&test_macro_file()).unwrap()).unwrap();
        let existing = backup.get(Slot::S1, MacroIndex::M1).unwrap().raw();

        // Response script in exact request order: config snapshot, the one
        // non-empty macro, Header x2, Save, Erase, Write, Save, read-back
        // config, read-back macro. Responses are served on `send` so a
        // keepalive tick that races the command cannot steal them.
        let mut script = config_response_frames(&snapshot.config);
        script.extend(macro_read_frames(existing));
        script.extend([
            ack_frame(commands::cmd::WRITE_CONFIG),
            ack_frame(commands::cmd::WRITE_CONFIG),
            ack_frame(commands::cmd::SAVE),
            ack_frame(commands::cmd::ERASE_MACRO),
            ack_frame(commands::cmd::WRITE_MACRO),
            ack_frame(commands::cmd::SAVE),
        ]);
        script.extend(config_response_frames(&snapshot.config));
        script.extend(macro_read_frames(existing));

        let transport = AutoRespondTransport::new(200, script);
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let interval = Duration::from_millis(20);
        let keepalive = start_session_keepalive_with_config(
            Arc::clone(&session),
            SessionKeepaliveConfig::with_interval(interval),
            true,
        )
        .await
        .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        let dirs_before = default_backup_dirs_in_cwd();
        let keepalive_before = keepalive.state.sent.load(Ordering::SeqCst);
        let mut monitor: Option<SessionMonitor> = None;
        let started = Instant::now();
        let outcome = execute_session_command(
            &session,
            &keepalive.state,
            started,
            200,
            interval,
            true,
            &mut monitor,
            SessionCommand::MacroWrite {
                slot: 0,
                index: 0,
                file: macro_file,
                trigger: None,
                chunk_size: None,
                layout: MacroWriteLayout::default_ble(),
            },
        )
        .await
        .expect("session macro write must succeed");
        let elapsed = started.elapsed();
        assert_eq!(outcome, SessionOutcome::Continue);

        // Non-vacuous timing: the write must span many 20 ms keepalive
        // periods. With the previous 1 s interval this was false and the test
        // would have passed even with no operation lock.
        assert!(
            elapsed > interval * 10,
            "write took only {elapsed:?}; the keepalive could not have ticked during it"
        );

        let frames = transport.sent_frames();
        let commands: Vec<u16> = frames
            .iter()
            .map(|frame| CustomHeader::parse(&frame[1..]).unwrap().cmd)
            .collect();
        let start = commands
            .iter()
            .position(|command| *command == commands::cmd::READ_CONFIG)
            .expect("the write must send a configuration read");
        let end = commands
            .iter()
            .rposition(|command| *command == commands::cmd::READ_MACRO)
            .expect("the write must end with a macro read-back");
        assert!(start < end, "unexpected write frame window {start}..={end}");
        let input_during: Vec<usize> = (start..=end)
            .filter(|index| commands[*index] == commands::cmd::INPUT)
            .collect();
        assert!(
            input_during.is_empty(),
            "0x0003 was sent during the macro write at frame indices {input_during:?}"
        );

        let count = |command: u16| commands.iter().filter(|sent| **sent == command).count();
        assert_eq!(count(commands::cmd::WRITE_CONFIG), 2);
        assert_eq!(count(commands::cmd::SAVE), 2);
        assert_eq!(count(commands::cmd::ERASE_MACRO), 1);
        assert_eq!(count(commands::cmd::WRITE_MACRO), 1);
        assert_eq!(count(commands::cmd::READ_CONFIG), 40);
        assert_eq!(count(commands::cmd::READ_MACRO), 2);
        assert_eq!(transport.remaining_script_entries(), 0);
        assert_eq!(transport.served_commands().len(), 48);

        // The handler resolved the default backup path and wrote a real backup.
        let dirs_after = default_backup_dirs_in_cwd();
        let created: Vec<PathBuf> = dirs_after.difference(&dirs_before).cloned().collect();
        assert_eq!(
            created.len(),
            1,
            "expected exactly one new pre-write backup directory, got {created:?}"
        );
        let backup_dir = RemoveDirOnDrop(created[0].clone());
        assert!(backup_dir.0.join("config.bin").is_file());
        assert!(backup_dir.0.join("macros.json").is_file());

        // The lock is released: the keepalive resumes with a new 0x0003 after
        // the write frame window.
        let mut resumed = false;
        for _ in 0..100 {
            let commands = commands_of(&transport.sent_frames());
            if keepalive.state.sent.load(Ordering::SeqCst) > keepalive_before
                && commands
                    .iter()
                    .skip(end + 1)
                    .any(|command| *command == commands::cmd::INPUT)
            {
                resumed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(
            resumed,
            "the keepalive did not resume after the macro write: sent={}",
            keepalive.state.sent.load(Ordering::SeqCst)
        );

        shutdown_session(&session, keepalive).await;
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The REPL loop must pass its `allow_macro_write` state to the handler.
    ///
    /// A valid macro file is used on purpose: if the loop hardcoded `true`, the
    /// handler would start reading the controller and the transport would
    /// record frames. The disabled flag must reject before any device I/O.
    #[tokio::test]
    async fn session_repl_rejects_macro_write_from_input_without_the_opt_in_flag() {
        let root = scratch_dir("session-gate-repl");
        let macro_file = root.join("edit.json");
        std::fs::write(&macro_file, serde_json::to_vec(&test_macro_file()).unwrap()).unwrap();

        // An empty response script: if the loop wrongly allowed the command,
        // the first configuration read would be recorded and then fail.
        let transport = AutoRespondTransport::new(200, Vec::new());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let input = format!("macro write 0 0 {}\nquit\n", macro_file.display());
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            false,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;

        assert!(
            result.is_ok(),
            "a disabled macro write must not end the session: {result:?}"
        );
        assert!(
            transport.sent_frames().is_empty(),
            "the loop must forward allow_macro_write=false; a device read was sent"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ctrl-C during `macro restore` is deferred until the command finishes.
    ///
    /// The ctrl_c future resolves as soon as the restore has sent its first
    /// Erase, i.e. deep inside the write. If the loop cancelled the command
    /// future (the behaviour for read-only commands), the final verification
    /// reads would be missing and the pre-restore backup would be incomplete.
    #[tokio::test]
    async fn session_ctrl_c_during_macro_restore_is_deferred_until_completion() {
        let root = scratch_dir("session-ctrl-c-restore");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture(&fixture_dir);

        // Same response script as `restore_verifies_non_empty_macro_raw`.
        let transport = KeepaliveTestTransport::with_mtu(200);
        for frame in config_response_frames(&snapshot.config) {
            transport.queue_raw(&frame);
        }
        for frame in macro_read_frames(backup.get(Slot::S1, MacroIndex::M1).unwrap().raw()) {
            transport.queue_raw(&frame);
        }
        for frame in [
            ack_frame(commands::cmd::WRITE_CONFIG),
            ack_frame(commands::cmd::WRITE_CONFIG),
            ack_frame(commands::cmd::SAVE),
            ack_frame(commands::cmd::ERASE_MACRO),
            ack_frame(commands::cmd::WRITE_MACRO),
            ack_frame(commands::cmd::SAVE),
        ] {
            transport.queue_raw(&frame);
        }
        for frame in config_response_frames(&snapshot.config) {
            transport.queue_raw(&frame);
        }
        for frame in macro_read_frames(backup.get(Slot::S1, MacroIndex::M1).unwrap().raw()) {
            transport.queue_raw(&frame);
        }
        for frame in config_response_frames(&snapshot.config) {
            transport.queue_raw(&frame);
        }
        for frame in macro_read_frames(backup.get(Slot::S1, MacroIndex::M1).unwrap().raw()) {
            transport.queue_raw(&frame);
        }

        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());

        // The ctrl_c future completes only once the restore is writing.
        let watched = transport.clone();
        let notify = Arc::new(Notify::new());
        let watcher_notify = Arc::clone(&notify);
        let watcher = tokio::spawn(async move {
            loop {
                let writing = watched.sent_frames().iter().any(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::ERASE_MACRO
                });
                if writing {
                    watcher_notify.notify_one();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
        let ctrl_c = async move { notify.notified().await };

        let input = format!("macro restore {}\n", fixture_dir.display());
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let started = Instant::now();
        let result = session_repl_loop_with_io(
            &session,
            &state,
            started,
            200,
            Duration::from_millis(20),
            true,
            false,
            &mut reader,
            ctrl_c,
            false,
        )
        .await;
        let elapsed = started.elapsed();
        watcher.await.unwrap();

        assert!(
            result.is_ok(),
            "the session must exit cleanly after the deferred Ctrl-C: {result:?}"
        );
        // The command completed: initial config/pre-restore read, batch
        // verification and the final whole-backup verification.
        let commands = sent_commands(&transport);
        let count = |command: u16| commands.iter().filter(|sent| **sent == command).count();
        assert_eq!(count(commands::cmd::READ_CONFIG), 60);
        assert_eq!(count(commands::cmd::READ_MACRO), 3);
        assert_eq!(count(commands::cmd::SAVE), 2);
        assert!(
            elapsed >= Duration::from_millis(100),
            "restore finished too fast ({elapsed:?}) for the Ctrl-C injection to be meaningful"
        );
        // The pre-restore backup was fully written before the deferred exit.
        let pre_restore_written = std::fs::read_dir(&root).unwrap().any(|entry| {
            entry.is_ok_and(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("-pre-restore-")
            })
        });
        assert!(
            pre_restore_written,
            "the deferred restore must have written its pre-restore backup"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn session_report_enable_flag_maps_to_the_startup_0007_decision() {
        assert!(session_report_enable(false));
        assert!(!session_report_enable(true));
    }

    #[tokio::test]
    async fn session_keepalive_can_skip_the_0007_enable() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive_with_config(
            Arc::clone(&session),
            SessionKeepaliveConfig::with_interval(Duration::from_millis(1)),
            false,
        )
        .await
        .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        let enable_true_count = || {
            transport
                .sent_frames()
                .iter()
                .filter(|frame| {
                    let header = CustomHeader::parse(&frame[1..]).unwrap();
                    header.cmd == commands::cmd::REPORT_ENABLE && header.subcmd == 1
                })
                .count()
        };
        assert_eq!(enable_true_count(), 0, "0x0007 enable must be skipped");
        assert!(
            sent_command_count(&transport, commands::cmd::INPUT) >= 1,
            "the keepalive must still poll 0x0003"
        );

        shutdown_session(&session, keepalive).await;
        // Shutdown still sends a best-effort disable (subcmd 0), never enable 1.
        assert_eq!(enable_true_count(), 0);
    }

    #[test]
    fn macro_file_rejects_directories_and_sockets() {
        let root = scratch_dir("macro-file-nonregular");
        let directory = root.join("a-directory");
        std::fs::create_dir(&directory).unwrap();
        let error = read_macro_file(&directory).unwrap_err();
        assert!(is_invalid_input(&error), "unexpected error: {error:?}");
        assert!(error.to_string().contains("not a regular file"));

        #[cfg(unix)]
        {
            let socket = root.join("a.sock");
            let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            let error = read_macro_file(&socket).unwrap_err();
            assert!(is_invalid_input(&error), "unexpected error: {error:?}");
            assert!(error.to_string().contains("not a regular file"));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_backup_rejects_non_regular_files() {
        let root = scratch_dir("restore-nonregular");

        // config.bin as a directory: rejected before the blocking read.
        let config_dir = root.join("config-dir");
        std::fs::create_dir_all(config_dir.join("config.bin")).unwrap();
        let error = read_restore_backup(&config_dir).unwrap_err();
        assert!(is_invalid_input(&error), "unexpected error: {error:?}");
        assert!(error.to_string().contains("not a regular file"));

        // A raw macro file replaced by a directory.
        let fixture = root.join("fixture");
        backup_fixture(&fixture);
        std::fs::remove_file(fixture.join("slot0-macro0.bin")).unwrap();
        std::fs::create_dir(fixture.join("slot0-macro0.bin")).unwrap();
        let error = read_restore_backup(&fixture).unwrap_err();
        assert!(is_invalid_input(&error), "unexpected error: {error:?}");
        assert!(error.to_string().contains("not a regular file"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn generated_backup_directories_are_unique() {
        // Two writes started in the same second/process must not share a path.
        assert_ne!(
            default_backup_dir("macro-backup"),
            default_backup_dir("macro-backup")
        );
        let base = PathBuf::from("/tmp/backup");
        assert_ne!(pre_restore_dir(&base), pre_restore_dir(&base));
    }

    #[test]
    fn parse_session_commands_covers_empty_unknown_and_options() {
        assert_eq!(parse_session_command(""), SessionCommand::Empty);
        assert_eq!(
            parse_session_command("monitor --count 0"),
            SessionCommand::Monitor { count: Some(0) }
        );
        assert_eq!(
            parse_session_command("monitor stop"),
            SessionCommand::MonitorStop
        );
        assert!(matches!(
            parse_session_command("config dump --raw"),
            SessionCommand::Unknown(_)
        ));
        assert!(matches!(
            parse_session_command("bogus"),
            SessionCommand::Unknown(_)
        ));
    }

    #[test]
    fn parse_session_commands_rejects_ambiguous_file_options() {
        for line in [
            "backup --raw a --raw b",
            "backup --json a --json b",
            "config dump --raw a --raw b",
            "config dump --raw",
            "config dump --json",
            "backup --raw --json x",
            "backup --json --raw x",
            "backup --raw a --bogus b",
        ] {
            assert!(
                matches!(parse_session_command(line), SessionCommand::Unknown(_)),
                "ambiguous options must be rejected: {line}"
            );
        }

        assert_eq!(
            parse_session_command("backup --raw a --json b"),
            SessionCommand::Backup {
                raw: Some(PathBuf::from("a")),
                json: Some(PathBuf::from("b")),
            }
        );
    }

    #[test]
    fn parse_session_macro_commands_matches_the_documented_syntax() {
        assert_eq!(
            parse_session_command("macro backup out"),
            SessionCommand::MacroBackup {
                dir: PathBuf::from("out")
            }
        );
        assert_eq!(
            parse_session_command("macro write 0 1 file.toml"),
            SessionCommand::MacroWrite {
                slot: 0,
                index: 1,
                file: PathBuf::from("file.toml"),
                trigger: None,
                chunk_size: None,
                layout: MacroWriteLayout::default_ble(),
            }
        );
        assert_eq!(
            parse_session_command("macro write 2 3 file.json --trigger 0x00000008 --chunk-size 32"),
            SessionCommand::MacroWrite {
                slot: 2,
                index: 3,
                file: PathBuf::from("file.json"),
                trigger: Some("0x00000008".to_string()),
                chunk_size: Some(32),
                layout: MacroWriteLayout::default_ble(),
            }
        );
        assert_eq!(
            parse_session_command("macro write 0 1 file.toml --chunk-size 64"),
            SessionCommand::MacroWrite {
                slot: 0,
                index: 1,
                file: PathBuf::from("file.toml"),
                trigger: None,
                chunk_size: Some(64),
                layout: MacroWriteLayout::default_ble(),
            }
        );
        assert_eq!(
            parse_session_command("macro write 1 0 file.toml --write-total-relative"),
            SessionCommand::MacroWrite {
                slot: 1,
                index: 0,
                file: PathBuf::from("file.toml"),
                trigger: None,
                chunk_size: None,
                layout: MacroWriteLayout::relative_total_len(),
            }
        );
        assert_eq!(
            parse_session_command("macro restore backup"),
            SessionCommand::MacroRestore {
                dir: PathBuf::from("backup"),
                force: false,
            }
        );
        assert_eq!(
            parse_session_command("macro restore backup --force"),
            SessionCommand::MacroRestore {
                dir: PathBuf::from("backup"),
                force: true,
            }
        );
    }

    #[test]
    fn parse_session_macro_commands_rejects_ambiguous_or_incomplete_lines() {
        for line in [
            "macro",
            "macro backup",
            "macro backup a b",
            "macro backup --force",
            "macro write",
            "macro write 0",
            "macro write 0 1",
            "macro write 0 1 file.toml --trigger",
            "macro write 0 1 file.toml --trigger a --trigger b",
            "macro write 0 1 file.toml --chunk-size",
            "macro write 0 1 file.toml --chunk-size 48",
            "macro write 0 1 file.toml --bogus x",
            "macro write 0 1 file.toml --write-total-relative --write-total-relative",
            "macro write 0 1 file.toml --write-total-relative true",
            // Removed flags: absolute is the default and a relative Write
            // offset would erase one sector and write another.
            "macro write 0 1 file.toml --write-total-absolute",
            "macro write 0 1 file.toml --write-offset-relative",
            "macro write 256 1 file.toml",
            "macro restore",
            "macro restore --force",
            "macro restore a --force --force",
            "macro restore a --bogus",
        ] {
            assert!(
                matches!(parse_session_command(line), SessionCommand::Unknown(_)),
                "must be rejected: {line}"
            );
        }
    }

    #[test]
    fn macro_write_layout_flags_map_to_protocol_layout() {
        // No flags: the absolute total_len (device-confirmed for macro_idx=0/1
        // on slot S1, BLE).
        assert_eq!(macro_write_layout(false), MacroWriteLayout::default_ble());
        assert!(macro_write_layout(false).absolute_total_len);
        // --write-total-relative reverts only the total_len theory.
        assert_eq!(
            macro_write_layout(true),
            MacroWriteLayout::relative_total_len()
        );
        assert!(!macro_write_layout(true).absolute_total_len);
    }

    #[test]
    fn macro_write_disabled_message_names_the_opt_in_flag() {
        assert_eq!(
            macro_write_disabled_message(&SessionCommand::MacroWrite {
                slot: 0,
                index: 1,
                file: PathBuf::from("f.toml"),
                trigger: None,
                chunk_size: None,
                layout: MacroWriteLayout::default_ble(),
            }),
            Some("macro write is disabled; restart with --allow-macro-write")
        );
        assert_eq!(
            macro_write_disabled_message(&SessionCommand::MacroRestore {
                dir: PathBuf::from("backup"),
                force: false,
            }),
            Some("macro restore is disabled; restart with --allow-macro-write")
        );
        // Read-only commands, including `macro backup`, are never gated.
        assert_eq!(
            macro_write_disabled_message(&SessionCommand::MacroBackup {
                dir: PathBuf::from("out")
            }),
            None
        );
        assert_eq!(macro_write_disabled_message(&SessionCommand::Info), None);
        assert!(is_macro_flash_command(&SessionCommand::MacroWrite {
            slot: 0,
            index: 0,
            file: PathBuf::from("f"),
            trigger: None,
            chunk_size: None,
            layout: MacroWriteLayout::default_ble(),
        }));
        assert!(is_macro_flash_command(&SessionCommand::MacroRestore {
            dir: PathBuf::from("b"),
            force: false,
        }));
        assert!(!is_macro_flash_command(&SessionCommand::MacroBackup {
            dir: PathBuf::from("b")
        }));
    }

    #[tokio::test]
    async fn session_rejects_macro_write_and_restore_without_the_opt_in_flag() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let mut monitor: Option<SessionMonitor> = None;

        // The paths do not exist on purpose: the refusal must happen before
        // the file is read, the operation lock is taken, or any frame is sent.
        for command in [
            SessionCommand::MacroWrite {
                slot: 0,
                index: 0,
                file: PathBuf::from("/nonexistent/macro.toml"),
                trigger: None,
                chunk_size: None,
                layout: MacroWriteLayout::default_ble(),
            },
            SessionCommand::MacroRestore {
                dir: PathBuf::from("/nonexistent/backup"),
                force: false,
            },
        ] {
            let outcome = execute_session_command(
                &session,
                &state,
                Instant::now(),
                200,
                Duration::from_millis(20),
                false,
                &mut monitor,
                command,
            )
            .await
            .unwrap();
            assert_eq!(outcome, SessionOutcome::Continue);
        }
        assert!(
            transport.sent_frames().is_empty(),
            "a disabled macro write/restore must not send a single frame"
        );
    }

    #[tokio::test]
    async fn session_macro_backup_runs_without_the_opt_in_flag() {
        let root = scratch_dir("session-macro-backup");
        let dir = root.join("backup");
        let config = Bt2Config::zeroed();
        let transport = KeepaliveTestTransport::with_mtu(200);
        queue_config_responses(&transport, &config);

        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let mut monitor: Option<SessionMonitor> = None;
        let outcome = execute_session_command(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            &mut monitor,
            SessionCommand::MacroBackup { dir: dir.clone() },
        )
        .await
        .unwrap();

        // `macro backup` is read-only on the controller, so it is not gated.
        assert_eq!(outcome, SessionOutcome::Continue);
        assert!(dir.join("config.bin").is_file());
        assert!(dir.join("macros.json").is_file());
        assert_eq!(
            sent_command_count(&transport, commands::cmd::READ_CONFIG),
            20
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_command_message_escapes_terminal_control_bytes() {
        let message = unknown_command_message("bogus\u{1b}[31m\nforged\u{7}");
        assert!(
            !message.contains(['\u{1b}', '\u{7}', '\n']),
            "control bytes must not reach the terminal: {message:?}"
        );
        assert!(message.contains("\\u{1b}[31m"));
        assert!(message.contains("\\n"));
        assert!(message.contains("\\u{7}"));
        assert!(message.contains("type 'help'"));
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
            false,
            &mut monitor_state,
            SessionCommand::Monitor { count: None },
        )
        .await
        .unwrap();
        assert_eq!(outcome, SessionOutcome::Continue);
        assert!(monitor_state.is_some());
        for _ in 0..100 {
            if monitor_state
                .as_ref()
                .is_some_and(|monitor| monitor.stats.sent() > 0)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let monitor_sent = monitor_state
            .as_ref()
            .map_or(0, |monitor| monitor.stats.sent());
        assert!(
            monitor_sent > 0,
            "background monitor did not send its own 0x0003"
        );
        assert_eq!(
            monitor_counters(monitor_state.as_ref()).0,
            monitor_sent,
            "status counters must expose the background monitor traffic"
        );
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
            false,
            &mut monitor_state,
            SessionCommand::MonitorStop,
        )
        .await
        .unwrap();
        assert_eq!(outcome, SessionOutcome::Continue);
        assert!(monitor_state.is_none());
        assert_eq!(monitor_counters(monitor_state.as_ref()), (0, 0));
        let at_stop = input_count();

        tokio::time::sleep(Duration::from_millis(20)).await;
        let after_stop = input_count();
        assert!(
            after_stop > at_stop,
            "keepalive did not resume after monitor stop"
        );

        let report_enable = |subcmd: u16| {
            transport
                .sent_frames()
                .iter()
                .filter(|frame| {
                    let header = CustomHeader::parse(&frame[1..]).unwrap();
                    header.cmd == commands::cmd::REPORT_ENABLE && header.subcmd == subcmd
                })
                .count()
        };
        assert_eq!(report_enable(1), 1, "the session sends one shared enable");
        assert_eq!(
            report_enable(0),
            0,
            "stopping the monitor must not disable the keepalive's reports"
        );

        shutdown_session(&session, keepalive).await;
        assert_eq!(
            report_enable(0),
            1,
            "session shutdown owns the final report disable"
        );
        assert!(transport.is_disconnected());
    }

    #[tokio::test]
    async fn monitor_connection_polls_and_disables_reports() {
        let transport = KeepaliveTestTransport::new();
        for _ in 0..2 {
            transport.queue_envelope_input(&sample_input_report());
        }
        let session = Session::new(&transport);

        let outcome = monitor_connection(
            &session,
            Duration::from_millis(1),
            Some(2),
            std::future::pending::<()>(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, MonitorPollOutcome::CountReached(2));

        let frames = transport.sent_frames();
        assert_eq!(frames.len(), 4, "expected enable + 2 polls + disable");
        let enable = CustomHeader::parse(&frames[0][1..]).unwrap();
        assert_eq!(enable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(enable.subcmd, 1);
        for frame in &frames[1..3] {
            assert_eq!(
                CustomHeader::parse(&frame[1..]).unwrap().cmd,
                commands::cmd::INPUT
            );
        }
        let disable = CustomHeader::parse(&frames[3][1..]).unwrap();
        assert_eq!(disable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(disable.subcmd, 0);
    }

    #[tokio::test]
    async fn monitor_connection_disables_reports_on_shutdown() {
        let transport = KeepaliveTestTransport::new();
        let session = Session::new(&transport);

        let outcome = monitor_connection(
            &session,
            Duration::from_millis(20),
            None,
            std::future::ready(()),
        )
        .await
        .unwrap();
        assert_eq!(outcome, MonitorPollOutcome::Quit);

        let frames = transport.sent_frames();
        assert_eq!(
            frames.len(),
            2,
            "expected enable + disable on immediate shutdown"
        );
        let enable = CustomHeader::parse(&frames[0][1..]).unwrap();
        assert_eq!(enable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(enable.subcmd, 1);
        let disable = CustomHeader::parse(&frames[1][1..]).unwrap();
        assert_eq!(disable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(disable.subcmd, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn background_monitor_polls_every_20ms_without_keepalive() {
        let transport = KeepaliveTestTransport::new();
        // Queue more reports than 80 ms of 20 ms ticks so `try_next_input`
        // returns immediately on every poll; without the 20 ms floor, the loop
        // would burst far more than 4..=5 requests.
        for _ in 0..10 {
            transport.queue_envelope_input(&sample_input_report());
        }
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let monitor = start_background_monitor(Arc::clone(&session), Duration::from_millis(20));

        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        assert_eq!(monitor.stats.sent(), 1, "monitor did not start polling");

        for _ in 0..4 {
            tokio::time::advance(Duration::from_millis(20)).await;
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }
        let sent = monitor.stats.sent();
        assert!(
            (4..=5).contains(&sent),
            "unexpected 20 ms monitor poll count: {sent}"
        );
        assert_eq!(
            transport
                .sent_frames()
                .iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::INPUT
                })
                .count() as u64,
            sent,
            "background monitor shares the input poll counter"
        );
        assert_eq!(monitor.stats.received(), sent);
        let (status_sent, status_received) = monitor_counters(Some(&monitor));
        assert_eq!(status_sent, sent);
        assert_eq!(status_received, sent);
        assert_eq!(monitor_counters(None), (0, 0));

        monitor.stop().await;
        assert_eq!(
            transport
                .sent_frames()
                .iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::REPORT_ENABLE
                })
                .count(),
            0,
            "the background monitor must not send its own 0x0007 enable/disable"
        );
    }

    #[tokio::test]
    async fn monitor_error_is_reaped_and_info_succeeds() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_secs(3600))
            .await
            .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        transport.fail_receives();
        let state = Arc::clone(&keepalive.state);
        let started = Instant::now();
        let mut monitor_state: Option<SessionMonitor> = None;
        execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            false,
            &mut monitor_state,
            SessionCommand::Monitor { count: None },
        )
        .await
        .unwrap();
        assert!(monitor_state.is_some());

        for _ in 0..100 {
            if monitor_state
                .as_ref()
                .is_some_and(SessionMonitor::is_finished)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(monitor_state
            .as_ref()
            .is_some_and(SessionMonitor::is_finished));
        let recorded = monitor_state
            .as_ref()
            .and_then(SessionMonitor::error)
            .expect("monitor error must be recorded before reaping");
        assert!(!recorded.disconnected);
        assert!(
            recorded.message.contains("monitor receive failure"),
            "unexpected monitor error message: {}",
            recorded.message
        );
        transport.allow_receives();
        queue_version_response(&transport);

        let outcome = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            false,
            &mut monitor_state,
            SessionCommand::Info,
        )
        .await
        .unwrap();
        assert_eq!(outcome, SessionOutcome::Continue);
        assert!(monitor_state.is_none());

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

        // The tick's bounded input drain waits for real elapsed time
        // (`try_next_input` uses a wall-clock deadline, not the paused timer),
        // so advance in small steps until each tick has been recorded instead
        // of racing one 21 ms jump.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        for _ in 0..4 {
            let before = keepalive.state.sent.load(Ordering::SeqCst);
            while keepalive.state.sent.load(Ordering::SeqCst) == before {
                assert!(
                    std::time::Instant::now() < deadline,
                    "keepalive tick did not complete"
                );
                tokio::time::advance(Duration::from_millis(1)).await;
                tokio::task::yield_now().await;
            }
        }
        let sent = keepalive.state.sent.load(Ordering::SeqCst);
        assert!(
            (3..=6).contains(&sent),
            "unexpected keepalive poll count: {sent}"
        );

        shutdown_session(&session, keepalive).await;
        assert!(transport.is_disconnected());
    }

    #[derive(Clone)]
    struct HangingTransport {
        send_calls: Arc<std::sync::atomic::AtomicUsize>,
        send_entered: Arc<Notify>,
        disconnect_entered: Arc<AtomicBool>,
    }

    impl HangingTransport {
        fn new() -> Self {
            Self {
                send_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                send_entered: Arc::new(Notify::new()),
                disconnect_entered: Arc::new(AtomicBool::new(false)),
            }
        }

        async fn wait_until_send_entered(&self) {
            if self.send_calls.load(Ordering::SeqCst) > 0 {
                return;
            }
            self.send_entered.notified().await;
        }
    }

    #[async_trait]
    impl Transport for HangingTransport {
        async fn send(&self, _data: &[u8]) -> Result<(), Error> {
            self.send_calls.fetch_add(1, Ordering::SeqCst);
            self.send_entered.notify_waiters();
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Err(Error::Disconnected)
        }

        fn mtu(&self) -> u16 {
            200
        }

        async fn disconnect(&self) -> Result<(), Error> {
            self.disconnect_entered.store(true, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn shutdown_is_bounded_with_hanging_transport() {
        let transport = HangingTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = spawn_session_keepalive(
            Arc::clone(&session),
            SessionKeepaliveConfig::with_interval(Duration::from_millis(1)),
        );
        // Wait until the keepalive task holds the session lock and hangs
        // inside HangingTransport::send().
        transport.wait_until_send_entered().await;
        assert!(
            session.try_lock().is_err(),
            "keepalive must hold the session lock while hung in send()"
        );

        let started = Instant::now();
        shutdown_session_with_timeout(&session, keepalive, Duration::from_millis(20)).await;
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "shutdown took {elapsed:?}"
        );
        assert!(
            transport.send_calls.load(Ordering::SeqCst) >= 2,
            "expected hung keepalive send plus best-effort report disable send"
        );
        assert!(
            transport.disconnect_entered.load(Ordering::SeqCst),
            "shutdown must reach and bound the hanging disconnect() path"
        );
        assert!(
            session.try_lock().is_ok(),
            "aborted keepalive must have released the session lock"
        );
    }

    #[tokio::test]
    async fn session_monitor_stop_with_timeout_aborts_hanging_task() {
        let stop = Arc::new(Notify::new());
        let finished = Arc::new(Notify::new());
        let done = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn(std::future::pending::<()>());
        let monitor = SessionMonitor {
            stop,
            finished,
            done,
            error: Arc::new(StdMutex::new(None)),
            stats: Arc::new(MonitorStats::default()),
            task: Some(task),
        };

        let started = Instant::now();
        let status = monitor.stop_with_timeout(Duration::from_millis(20)).await;
        assert_eq!(status, MonitorExitStatus::Aborted);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "stop_with_timeout took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn background_monitor_stop_cancels_lock_wait() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport)));
        // Hold the operation lock so start_background_monitor blocks at lock().
        let guard = session.lock().await;
        let monitor = start_background_monitor(Arc::clone(&session), Duration::from_millis(10));
        tokio::task::yield_now().await;

        let started = Instant::now();
        let status = monitor.stop_with_timeout(Duration::from_secs(2)).await;
        assert_eq!(status, MonitorExitStatus::Clean);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "stop should cancel lock wait immediately, took {:?}",
            started.elapsed()
        );
        drop(guard);
    }

    #[tokio::test]
    async fn shutdown_is_bounded_when_operation_lock_is_held() {
        let session = Arc::new(AsyncMutex::new(Session::new(KeepaliveTestTransport::new())));
        let guard = session.lock().await;
        let keepalive = SessionKeepalive {
            state: Arc::new(SessionKeepaliveState::new()),
            token: CancellationToken::new(),
            task: None,
        };

        let started = Instant::now();
        shutdown_session_with_timeout(&session, keepalive, Duration::from_millis(20)).await;
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "shutdown took {:?}",
            started.elapsed()
        );
        drop(guard);
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
        let keepalive = start_session_keepalive_with_config(Arc::clone(&session), config, true)
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
        assert!(keepalive_degraded_error(&keepalive.state).is_some());

        shutdown_session(&session, keepalive).await;
        assert!(transport.is_disconnected());
    }

    #[tokio::test]
    async fn monitor_stop_after_error_reports_error_not_idle() {
        assert_eq!(
            monitor_stop_stdout_message(Some(&MonitorExitStatus::Clean), None),
            Some("monitor stopped")
        );
        assert_eq!(
            monitor_stop_stdout_message(None, Some(&MonitorExitStatus::Error("boom".to_string()))),
            None
        );
        assert_eq!(
            monitor_stop_stdout_message(
                None,
                Some(&MonitorExitStatus::Panicked("panic".to_string()))
            ),
            None
        );
        assert_eq!(
            monitor_stop_stdout_message(None, None),
            Some("no monitor is running")
        );

        // Also exercise execute_session_command with MonitorStop right after a
        // non-disconnect monitor failure and a disconnect failure.
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let started = Instant::now();
        let mut monitor_state: Option<SessionMonitor> = None;

        transport.fail_receives();
        execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            false,
            &mut monitor_state,
            SessionCommand::Monitor { count: None },
        )
        .await
        .unwrap();
        for _ in 0..100 {
            if monitor_state
                .as_ref()
                .is_some_and(SessionMonitor::is_finished)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let outcome = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            false,
            &mut monitor_state,
            SessionCommand::MonitorStop,
        )
        .await
        .unwrap();
        assert_eq!(outcome, SessionOutcome::Continue);
        assert!(monitor_state.is_none());

        transport.disconnect_receives();
        execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            false,
            &mut monitor_state,
            SessionCommand::Monitor { count: None },
        )
        .await
        .unwrap();
        for _ in 0..100 {
            if monitor_state
                .as_ref()
                .is_some_and(SessionMonitor::is_finished)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let result = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(1),
            false,
            &mut monitor_state,
            SessionCommand::MonitorStop,
        )
        .await;
        assert!(matches!(result, Err(Error::Disconnected)));
        assert!(monitor_state.is_none());
    }

    #[tokio::test]
    async fn shutdown_after_monitor_disconnect_disables_and_disconnects() {
        // M4 regression: after the background monitor has ended with
        // `Disconnected`, the shutdown that `run_session` performs when the
        // REPL returns must still send the `0x0007` disable and close the
        // link. The REPL half (waking from the monitor's disconnected state
        // and returning `Err(Disconnected)`) is covered by
        // `repl_detects_background_monitor_error_and_disconnect_without_input`;
        // this test keeps the transport disconnected and checks the shutdown
        // half that follows it.
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_millis(1))
            .await
            .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        transport.disconnect_receives();
        let state = Arc::clone(&keepalive.state);
        let started = Instant::now();
        let mut monitor_state: Option<SessionMonitor> = None;
        execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(20),
            false,
            &mut monitor_state,
            SessionCommand::Monitor { count: None },
        )
        .await
        .unwrap();
        assert!(monitor_state.is_some());
        for _ in 0..200 {
            if monitor_state
                .as_ref()
                .is_some_and(SessionMonitor::is_finished)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(
            monitor_state
                .as_ref()
                .is_some_and(SessionMonitor::is_finished),
            "the monitor must fail with Disconnected"
        );

        // Receives stay disconnected: shutdown must rely on `send` and
        // `disconnect` only, as it does when the link is already gone.
        let before = transport.sent_frames().len();
        shutdown_session(&session, keepalive).await;
        let frames = transport.sent_frames();
        let shutdown_frames = &frames[before.min(frames.len())..];
        assert!(
            shutdown_frames.iter().any(|frame| {
                let header = CustomHeader::parse(&frame[1..]).unwrap();
                header.cmd == commands::cmd::REPORT_ENABLE && header.subcmd == 0
            }),
            "shutdown after a monitor disconnect must send 0x0007 disable"
        );
        assert!(
            transport.is_disconnected(),
            "shutdown after a monitor disconnect must disconnect the link"
        );
    }

    /// `profile switch` must reach the normal dispatch, not the two-phase
    /// config-write path (`prepare_repl_config_command`, which rejects it).
    ///
    /// This is the regression test for the REPL dead-code bug: the command
    /// classifies as a config write for the `--allow-config-write` gate but it
    /// is not a prepared flash write, so it must be executed directly.
    #[tokio::test]
    async fn repl_profile_switch_sends_one_0x0015_frame_and_no_save() {
        let transport = MockTransport::new();
        // A direct `cmd = 0x0015` ACK for slot 1.
        let ack = CustomHeader::new(commands::cmd::WRITE_SLOT, 1, 0, 0)
            .to_bytes()
            .to_vec();
        transport.push_incoming(&ack);

        let session = Arc::new(AsyncMutex::new(Session::new(transport)));
        let state = Arc::new(SessionKeepaliveState::new());
        let input = b"profile switch 1 --yes\nquit\n";
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.to_vec()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            64,
            Duration::from_millis(1),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;

        assert!(result.is_ok(), "the REPL must not fail: {result:?}");
        let sent = session.lock().await.transport().sent_frames();
        let switches: Vec<_> = sent
            .iter()
            .filter(|frame| {
                frame.len() > 1
                    && CustomHeader::parse(&frame[1..])
                        .map(|header| header.cmd == commands::cmd::WRITE_SLOT)
                        .unwrap_or(false)
            })
            .collect();
        assert_eq!(
            switches.len(),
            1,
            "exactly one 0x0015 frame must be sent (a REPL dead-code regression sends none); \
             frames: {sent:?}"
        );
        assert!(
            !sent.iter().any(|frame| {
                frame.len() > 1
                    && CustomHeader::parse(&frame[1..])
                        .map(|header| header.cmd == commands::cmd::SAVE)
                        .unwrap_or(false)
            }),
            "no Save may be sent with 0x0015"
        );
    }

    #[tokio::test]
    async fn repl_detects_background_monitor_error_and_disconnect_without_input() {
        use tokio::io::AsyncWriteExt as _;

        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_secs(3600))
            .await
            .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        let (mut writer, reader) = tokio::io::duplex(256);
        let mut buf_reader = tokio::io::BufReader::new(reader);
        let repl_session = Arc::clone(&session);
        let repl_state = Arc::clone(&keepalive.state);
        let repl_task = tokio::spawn(async move {
            session_repl_loop_with_io(
                &repl_session,
                &repl_state,
                Instant::now(),
                64,
                Duration::from_millis(1),
                false,
                false,
                &mut buf_reader,
                std::future::pending::<()>(),
                false,
            )
            .await
        });

        // 1. Non-disconnect monitor failure is reaped immediately without any
        //    further stdin input, and the session remains alive.
        transport.fail_receives();
        writer.write_all(b"monitor\n").await.unwrap();
        // Wait for the background monitor to run, fail, and release the lock.
        let mut reaped_cleanly = false;
        for _ in 0..200 {
            if transport.sent_frames().len() >= 3 && session.try_lock().is_ok() {
                reaped_cleanly = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(reaped_cleanly, "monitor did not exit after receive error");
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            !repl_task.is_finished(),
            "non-disconnect monitor error must not terminate the REPL"
        );

        // 2. Disconnect error in background monitor terminates the REPL
        //    immediately with Err(Error::Disconnected) even while stdin stays
        //    open with no further input.
        transport.allow_receives();
        transport.disconnect_receives();
        writer.write_all(b"monitor\n").await.unwrap();

        let repl_result = tokio::time::timeout(Duration::from_secs(1), repl_task)
            .await
            .expect("REPL must wake and exit on monitor disconnect without stdin input")
            .expect("REPL task must not panic");
        assert!(matches!(repl_result, Err(Error::Disconnected)));

        drop(writer);
        shutdown_session(&session, keepalive).await;
    }

    #[tokio::test]
    async fn session_monitor_stop_reports_task_panic() {
        let stop = Arc::new(Notify::new());
        let finished = Arc::new(Notify::new());
        let done = Arc::new(AtomicBool::new(false));
        let task_finished = Arc::clone(&finished);
        let task_done = Arc::clone(&done);
        let task = tokio::spawn(async move {
            let _guard = NotifyOnDrop {
                done: task_done,
                notify: task_finished,
            };
            panic!("simulated monitor panic");
        });
        let monitor = SessionMonitor {
            stop,
            finished,
            done,
            error: Arc::new(StdMutex::new(None)),
            stats: Arc::new(MonitorStats::default()),
            task: Some(task),
        };
        wait_for_monitor_finish(Some(&monitor)).await;
        let status = monitor.stop().await;
        assert!(matches!(status, MonitorExitStatus::Panicked(_)));
    }

    #[tokio::test]
    async fn repl_returns_error_when_keepalive_degraded_on_eof_or_quit() {
        // Case 1: EOF arrives when keepalive is already degraded.
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        state.record_error(&Error::Disconnected);
        state.consecutive_failures.store(3, Ordering::SeqCst);
        state.degraded.store(true, Ordering::SeqCst);
        state.stopped_notify.notify_one();

        let mut eof_reader = tokio::io::BufReader::new(tokio::io::empty());
        let eof_result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            64,
            Duration::from_millis(10),
            false,
            false,
            &mut eof_reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(
            matches!(eof_result, Err(Error::Transport(ref msg)) if msg.contains("keepalive degraded")),
            "expected keepalive degraded error on EOF, got {eof_result:?}"
        );

        // Case 2: "quit\n" is immediately available while keepalive is degraded.
        let state_quit = Arc::new(SessionKeepaliveState::new());
        state_quit.record_error(&Error::Disconnected);
        state_quit.consecutive_failures.store(3, Ordering::SeqCst);
        state_quit.degraded.store(true, Ordering::SeqCst);

        let mut quit_reader = tokio::io::BufReader::new(std::io::Cursor::new(b"quit\n".to_vec()));
        let quit_result = session_repl_loop_with_io(
            &session,
            &state_quit,
            Instant::now(),
            64,
            Duration::from_millis(10),
            false,
            false,
            &mut quit_reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(
            matches!(quit_result, Err(Error::Transport(ref msg)) if msg.contains("keepalive degraded")),
            "expected keepalive degraded error on quit, got {quit_result:?}"
        );

        // Case 3: Even if REPL returned Ok(()) (e.g. race with Ctrl-C),
        // finalize_session_result still surfaces degraded keepalive as Err.
        let finalized = finalize_session_result(Ok(()), &state_quit);
        assert!(
            matches!(finalized, Err(Error::Transport(ref msg)) if msg.contains("keepalive degraded"))
        );
    }

    #[test]
    fn app_mode_press_message_names_the_link_the_press_and_the_wait() {
        for reason in [
            "SIGINT received; exiting closes the BLE link and exits App Mode",
            "stdin ended (EOF); exiting closes the BLE link and exits App Mode",
            "the BLE link was lost",
        ] {
            let message = app_mode_press_message(reason);
            assert!(message.contains(reason), "reason missing: {message}");
            assert!(message.contains("80ND"), "80ND missing: {message}");
            assert!(
                message.contains("Profile"),
                "press instruction missing: {message}"
            );
            assert!(
                message.contains("3 seconds"),
                "press duration missing: {message}"
            );
        }
    }

    #[test]
    fn interrupted_commands_use_the_conventional_sigint_exit_code() {
        // 128 + SIGINT(2), distinct from the degraded exit code 3 so the
        // resident loop cannot confuse a user interrupt with a lost link.
        assert_eq!(EXIT_INTERRUPTED, 130);
        assert_ne!(EXIT_INTERRUPTED, EXIT_SESSION_DEGRADED);
    }

    #[tokio::test]
    async fn with_sigint_prefers_completed_results_and_reports_interruption() {
        // Completion wins when both are ready, so a command that finished at
        // the same moment as the signal keeps its result.
        let finished = with_sigint(async { Ok::<u8, Error>(7) }, std::future::ready(())).await;
        assert!(
            matches!(finished, SigintOutcome::Finished(Ok(7))),
            "completion must win the race"
        );

        let interrupted = with_sigint(
            std::future::pending::<Result<u8, Error>>(),
            std::future::ready(()),
        )
        .await;
        assert!(
            matches!(interrupted, SigintOutcome::Interrupted),
            "a pending command must yield to the signal"
        );

        let failed = with_sigint(
            async { Err::<u8, Error>(Error::Disconnected) },
            std::future::pending::<()>(),
        )
        .await;
        assert!(
            matches!(failed, SigintOutcome::Finished(Err(Error::Disconnected))),
            "a command error must not be mistaken for an interruption"
        );
    }

    #[tokio::test]
    async fn with_sigint_flash_cancels_only_before_the_write_starts() {
        let write_active = AtomicBool::new(false);

        // Prepare phase: the pending future is dropped at once, i.e. no flash
        // frame is sent.
        let outcome = with_sigint_flash_gate(
            std::future::pending::<Result<u8, Error>>(),
            std::future::ready(()),
            &write_active,
        )
        .await;
        assert!(
            matches!(outcome, FlashSigintOutcome::Cancelled),
            "a signal during prepare must cancel before any flash frame"
        );

        // Write phase: the future is allowed to finish.
        write_active.store(true, Ordering::SeqCst);
        let future = async {
            tokio::task::yield_now().await;
            Ok::<u8, Error>(9)
        };
        let outcome = with_sigint_flash_gate(future, std::future::ready(()), &write_active).await;
        assert!(
            matches!(outcome, FlashSigintOutcome::FinishedAfterInterrupt(Ok(9))),
            "a signal after the write started must not abandon the write"
        );

        let outcome = with_sigint_flash_gate(
            async { Ok::<u8, Error>(9) },
            std::future::pending::<()>(),
            &write_active,
        )
        .await;
        assert!(
            matches!(outcome, FlashSigintOutcome::Finished(Ok(9))),
            "without a signal the result is a plain finished outcome"
        );
    }

    #[tokio::test]
    async fn config_prompt_sigint_cancels_before_any_flash_frame() {
        // Real top-level flow: read + pre-write backup + async prompt. SIGINT
        // while the prompt is pending must drop the future before the first
        // 0x0001/Save frame is sent.
        let dir = scratch_dir("flash-prompt-cancel");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.set_profile_name(Slot::S2, "Edited").unwrap();
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let (_writer, reader) = tokio::io::duplex(64);
        let mut reader = tokio::io::BufReader::new(reader);
        let mut line_state = LineState::default();
        let confirmation = ConfigConfirmation::AsyncReader {
            reader: &mut reader,
            line_state: &mut line_state,
        };
        let backup = dir.join("backup");
        let write_active = AtomicBool::new(false);

        let outcome = with_sigint_flash_gate(
            async {
                config_write_session(&session, &edited, Some(&backup), false, confirmation, None)
                    .await
            },
            std::future::ready(()),
            &write_active,
        )
        .await;
        assert!(
            matches!(outcome, FlashSigintOutcome::Cancelled),
            "a prompt SIGINT must cancel before the flash phase"
        );

        let sent = transport.sent();
        let flash_frame = sent.iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(
            !flash_frame,
            "the cancelled prompt must not send any 0x0001/Save frame"
        );
        assert!(
            backup.join("config.bin").is_file(),
            "the pre-write backup (prepare phase) still exists"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flash_interrupted_recovery_names_the_restore_commands() {
        let message = flash_interrupted_recovery_message();
        assert!(message.contains("partially updated"), "{message}");
        assert!(message.contains("macro restore"), "{message}");
        assert!(message.contains("config write"), "{message}");
        assert!(message.contains("read it back"), "{message}");
    }

    #[test]
    fn scan_check_uses_app_mode_presence() {
        let app = eightbitdo_core::BleDevice {
            name: Some("80ND".to_string()),
            address: "aa:bb:cc:dd:ee:ff".to_string(),
            rssi: Some(-40),
            is_app: true,
        };
        let other = eightbitdo_core::BleDevice {
            name: Some("Something".to_string()),
            address: "11:22:33:44:55:66".to_string(),
            rssi: Some(-60),
            is_app: false,
        };
        assert_eq!(scan_check_exit_code(&[app, other.clone()]), 0);
        assert_eq!(scan_check_exit_code(&[other]), EXIT_SCAN_NO_APP_MODE);
        assert_eq!(scan_check_exit_code(&[]), EXIT_SCAN_NO_APP_MODE);
        assert_eq!(EXIT_SCAN_NO_APP_MODE, 3);
    }

    #[test]
    fn flash_command_classification_covers_every_write() {
        fn parse(args: &[&str]) -> Command {
            Cli::try_parse_from(args).expect("parse command").command
        }
        for args in [
            vec![
                "eightbitdo",
                "macro",
                "write",
                "--slot",
                "1",
                "--index",
                "1",
                "--file",
                "steps.toml",
            ],
            vec!["eightbitdo", "macro", "restore", "--dir", "backup-dir"],
            vec!["eightbitdo", "config", "write", "--file", "config.bin"],
            vec!["eightbitdo", "config", "set", "profile_name", "p1"],
            vec!["eightbitdo", "config", "remap", "a", "b"],
        ] {
            assert!(
                is_flash_command(&parse(&args)),
                "expected a flash command: {args:?}"
            );
        }
        for args in [
            vec!["eightbitdo", "info"],
            vec!["eightbitdo", "scan"],
            vec!["eightbitdo", "scan", "--check"],
            vec!["eightbitdo", "macro", "backup", "--dir", "dir"],
            vec!["eightbitdo", "config", "dump"],
            vec!["eightbitdo", "config", "map", "show"],
        ] {
            assert!(
                !is_flash_command(&parse(&args)),
                "expected a read-only command: {args:?}"
            );
        }
    }

    #[test]
    fn session_exit_code_marks_degraded_as_three() {
        // The resident loop keys on 3 to stop and wait for a physical Profile
        // press. A clean quit is 0 and every other error is 1.
        assert_eq!(session_exit_code(&Ok(()), true), EXIT_SESSION_DEGRADED);
        assert_eq!(
            session_exit_code(&Err(Error::Disconnected), true),
            EXIT_SESSION_DEGRADED
        );
        assert_eq!(session_exit_code(&Ok(()), false), 0);
        assert_eq!(
            session_exit_code(&Err(Error::Disconnected), false),
            EXIT_SESSION_ERROR
        );
        assert_ne!(EXIT_SESSION_DEGRADED, EXIT_SESSION_ERROR);
    }

    #[tokio::test]
    async fn repl_exits_when_keepalive_degrades_with_stdin_still_open() {
        // The failure mode this guards: a resident session sits in the REPL
        // line read while the keepalive degrades, so the process keeps waiting
        // for a command instead of exiting for the next physical press. The
        // write half is kept alive, so nothing but the keepalive stop can end
        // the loop.
        let (_writer, reader) = tokio::io::duplex(64);
        let mut reader = tokio::io::BufReader::new(reader);
        let session = Arc::new(AsyncMutex::new(Session::new(KeepaliveTestTransport::new())));
        let state = Arc::new(SessionKeepaliveState::new());
        state.record_error(&Error::Disconnected);
        state.consecutive_failures.store(3, Ordering::SeqCst);
        state.degraded.store(true, Ordering::SeqCst);
        state.stopped_notify.notify_one();

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            session_repl_loop_with_io(
                &session,
                &state,
                Instant::now(),
                64,
                Duration::from_millis(10),
                false,
                false,
                &mut reader,
                std::future::pending::<()>(),
                false,
            ),
        )
        .await
        .expect("the REPL must exit on a degraded keepalive instead of waiting for input");
        assert!(
            matches!(result, Err(Error::Transport(ref msg)) if msg.contains("keepalive degraded")),
            "expected the degraded exit, got {result:?}"
        );
    }

    #[tokio::test]
    async fn repl_exits_after_three_real_keepalive_failures_with_stdin_open() {
        // End-to-end version of the incident: the keepalive task itself fails
        // three consecutive polls while the REPL is blocked in the line read.
        // The session must surface the degraded error and stop waiting for
        // commands, not sit in the REPL until a physical press.
        let transport = KeepaliveTestTransport::new();
        // Three consecutive non-`Disconnected` failures: a disconnected recv
        // degrades on the first failure by design, so use the generic receive
        // error to exercise the 3-failure threshold.
        transport.fail_receives();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive_with_config(
            Arc::clone(&session),
            SessionKeepaliveConfig {
                interval: Duration::from_millis(1),
                max_consecutive_failures: 3,
            },
            false,
        )
        .await
        .expect("start keepalive");
        let state = Arc::clone(&keepalive.state);
        let (_writer, reader) = tokio::io::duplex(64);
        let mut reader = tokio::io::BufReader::new(reader);

        let result = tokio::time::timeout(
            Duration::from_secs(2),
            session_repl_loop_with_io(
                &session,
                &state,
                Instant::now(),
                64,
                Duration::from_millis(1),
                false,
                false,
                &mut reader,
                std::future::pending::<()>(),
                false,
            ),
        )
        .await
        .expect("the REPL must exit after three keepalive failures");
        assert!(
            matches!(result, Err(Error::Transport(ref msg)) if msg.contains("keepalive degraded")),
            "expected the degraded exit, got {result:?}"
        );
        assert_eq!(state.consecutive_failures.load(Ordering::SeqCst), 3);
        assert!(state.degraded.load(Ordering::SeqCst));
        assert_eq!(session_exit_code(&result, true), EXIT_SESSION_DEGRADED);

        shutdown_session(&session, keepalive).await;
        assert!(transport.is_disconnected());
    }

    #[tokio::test]
    async fn session_refuses_to_start_while_another_process_holds_the_ble_lock() {
        // The session must fail before touching the adapter, with the ordinary
        // error exit code (not the degraded 3), so a resident loop cannot
        // mistake lock contention for a lost link.
        let path = std::env::temp_dir().join(format!(
            "eightbitdo-cli-session-lock-{}-{}.lock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        let held = eightbitdo_core::BleLock::acquire_at(&path).expect("hold the lock");
        let options = BleOptions {
            lock_path: Some(path.clone()),
            ..BleOptions::default()
        };
        let code = session(&options, Duration::from_millis(20), false, false, true)
            .await
            .expect("session returns an exit code instead of an Err");
        assert_eq!(
            code,
            ExitCode::FAILURE,
            "a lock conflict is an ordinary failure, not a degraded exit"
        );
        assert_eq!(session_exit_code(&Ok(()), false), 0);
        drop(held);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn validate_output_paths_rejects_identical_paths_before_io() {
        let dir = scratch_dir("outputs-identical");
        let same = dir.join("same-output.bin");
        let other = dir.join("other-output.bin");
        let err = validate_output_paths(Some(&same), Some(&same)).unwrap_err();
        assert!(matches!(
            err,
            Error::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::InvalidInput
        ));
        assert!(validate_output_paths(Some(&same), Some(&other)).is_ok());
        assert!(validate_output_paths(Some(&same), None).is_ok());

        // backup_session rejects identical paths before sending any BLE frame.
        let transport = KeepaliveTestTransport::new();
        let session = Session::new(&transport);
        let err = backup_session(
            &session,
            Some(&same),
            Some(&same),
            Duration::ZERO,
            std::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            Error::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::InvalidInput
        ));
        assert!(
            transport.sent_frames().is_empty(),
            "backup_session must reject identical --raw/--json before any device I/O"
        );

        // Session REPL config dump / backup also reject identical paths before I/O.
        let shared = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let mut monitor: Option<SessionMonitor> = None;
        for cmd in [
            SessionCommand::ConfigDump {
                raw: Some(same.to_path_buf()),
                json: Some(same.to_path_buf()),
            },
            SessionCommand::Backup {
                raw: Some(same.to_path_buf()),
                json: Some(same.to_path_buf()),
            },
        ] {
            let err = execute_session_command(
                &shared,
                &state,
                Instant::now(),
                64,
                Duration::from_millis(10),
                false,
                &mut monitor,
                cmd,
            )
            .await
            .unwrap_err();
            assert!(matches!(
                err,
                Error::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::InvalidInput
            ));
        }
        assert!(
            transport.sent_frames().is_empty(),
            "session commands must reject identical --raw/--json before any device I/O"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn monitor_connection_disables_reports_on_poll_error() {
        let transport = KeepaliveTestTransport::new();
        transport.fail_receives();
        let session = Session::new(&transport);

        let err = monitor_connection(
            &session,
            Duration::from_millis(1),
            None,
            std::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, Error::Transport(ref msg) if msg.contains("monitor receive failure")),
            "expected original poll error, got {err:?}"
        );

        let frames = transport.sent_frames();
        assert_eq!(frames.len(), 3, "expected enable + input poll + disable");
        let enable = CustomHeader::parse(&frames[0][1..]).unwrap();
        assert_eq!(enable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(enable.subcmd, 1);
        let input = CustomHeader::parse(&frames[1][1..]).unwrap();
        assert_eq!(input.cmd, commands::cmd::INPUT);
        let disable = CustomHeader::parse(&frames[2][1..]).unwrap();
        assert_eq!(disable.cmd, commands::cmd::REPORT_ENABLE);
        assert_eq!(disable.subcmd, 0);
    }

    /// Transport that models the configuration block in memory.
    ///
    /// It acknowledges `0x0001` chunks, `0x0006` Save and answers `0x0002`
    /// read-back requests from the bytes written so far, so the whole
    /// `config write` / `config set` path (including read-back verification)
    /// can be tested without a device.
    #[derive(Clone)]
    struct ConfigDeviceTransport {
        config: Arc<Mutex<Bt2Config>>,
        incoming: Arc<Mutex<VecDeque<Vec<u8>>>>,
        sent: Arc<Mutex<Vec<Vec<u8>>>>,
        mtu: u16,
        stall_when_empty: bool,
        write_delay: Duration,
    }

    impl ConfigDeviceTransport {
        fn new(config: Bt2Config) -> Self {
            Self::with_mtu(config, 200)
        }

        fn with_mtu(config: Bt2Config, mtu: u16) -> Self {
            Self {
                config: Arc::new(Mutex::new(config)),
                incoming: Arc::new(Mutex::new(VecDeque::new())),
                sent: Arc::new(Mutex::new(Vec::new())),
                mtu,
                stall_when_empty: false,
                write_delay: Duration::ZERO,
            }
        }

        /// Make `recv` stay pending when no frame is queued, so a keepalive
        /// poll times out instead of seeing a disconnect.
        fn stalling(mut self) -> Self {
            self.stall_when_empty = true;
            self
        }

        /// Delay each `0x0001` ACK so a Ctrl-C can be injected mid-write.
        fn with_write_delay(mut self, delay: Duration) -> Self {
            self.write_delay = delay;
            self
        }

        fn set_config(&self, config: Bt2Config) {
            *self.config.lock().unwrap() = config;
        }

        fn config(&self) -> Bt2Config {
            self.config.lock().unwrap().clone()
        }

        fn sent(&self) -> Vec<Vec<u8>> {
            self.sent.lock().unwrap().clone()
        }

        fn queue(&self, header: &CustomHeader, payload: &[u8]) {
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(payload);
            self.incoming.lock().unwrap().push_back(frame);
        }

        /// Queue the single 142-byte data response for one read request,
        /// matching the device's 1 request = 1 chunk behavior.
        fn queue_readback_at(&self, config: &Bt2Config, requested_offset: u32) {
            let offset = requested_offset as usize;
            if offset >= CONFIG_SIZE {
                return;
            }
            let valid = (CONFIG_SIZE - offset).min(142);
            let mut payload = config.as_bytes()[offset..offset + valid].to_vec();
            payload.resize(142, 0);
            let header = CustomHeader::new(
                commands::cmd::RESPONSE,
                commands::cmd::READ_CONFIG,
                CONFIG_SIZE as u32,
                requested_offset,
            )
            .with_payload(&payload);
            self.queue(&header, &payload);
        }
    }

    #[async_trait]
    impl Transport for ConfigDeviceTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            self.sent.lock().unwrap().push(data.to_vec());
            let header = CustomHeader::parse(&data[1..])?;
            match header.cmd {
                commands::cmd::WRITE_CONFIG => {
                    if !self.write_delay.is_zero() {
                        tokio::time::sleep(self.write_delay).await;
                    }
                    let payload = &data[1 + eightbitdo_protocol::HEADER_SIZE..];
                    let start = header.offset as usize;
                    self.config.lock().unwrap().as_mut_bytes()[start..start + payload.len()]
                        .copy_from_slice(payload);
                    let ack = CustomHeader::new(
                        commands::cmd::RESPONSE,
                        commands::cmd::WRITE_CONFIG,
                        0,
                        0,
                    );
                    self.queue(&ack, &[]);
                }
                commands::cmd::SAVE => {
                    let ack =
                        CustomHeader::new(commands::cmd::SAVE, SaveTransport::Ble.subcmd(), 0, 0);
                    self.queue(&ack, &[]);
                }
                commands::cmd::READ_CONFIG => {
                    let config = self.config();
                    self.queue_readback_at(&config, header.offset);
                }
                _ => {}
            }
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            if let Some(frame) = self.incoming.lock().unwrap().pop_front() {
                return Ok(frame);
            }
            if self.stall_when_empty {
                std::future::pending::<()>().await;
            }
            Err(Error::Disconnected)
        }

        fn mtu(&self) -> u16 {
            self.mtu
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    fn config_write_fixture() -> Bt2Config {
        use eightbitdo_protocol::config_bt2::{
            OFF_LEGACY_A, OFF_LEGACY_B, OFF_MACROS, OFF_SIX_AXIS,
        };

        let mut config = Bt2Config::zeroed();
        for slot in Slot::ALL {
            config.set_slot_flag(slot, 0x2020_0911);
            config.set_profile_name(slot, "Slot").unwrap();
            config.set_rumble(
                slot,
                &RumbleRecord {
                    flag: 1,
                    left: 0.6,
                    right: 0.6,
                },
            );
            config.set_joy(
                slot,
                &JoyRecord {
                    flag: 1,
                    left_start_value: 1,
                    left_end_value: 127,
                    right_start_value: 2,
                    right_end_value: 126,
                },
            );
            config.set_six_axis(
                slot,
                &SixAxisRecord {
                    flag: 1,
                    key_map: KeyMap::NONE,
                    trigger_mode: 0,
                    sensitivity: 5,
                    dead_compensate: 40,
                    mapping_type: 0,
                },
            );
        }
        config.set_gamepad_mode(GamepadMode::Switch);
        config.set_cur_slot(Some(Slot::S1));
        config.as_mut_bytes()[OFF_LEGACY_A..OFF_MACROS].fill(0xA5);
        config.as_mut_bytes()[OFF_LEGACY_B..OFF_SIX_AXIS].fill(0x5A);
        config.recompute_crc();
        config
    }

    #[test]
    fn parse_config_set_accepts_documented_fields() {
        let slot = Slot::S2;
        assert_eq!(
            parse_config_set("cur_slot", "none", slot).unwrap(),
            ConfigSet::CurSlot(None)
        );
        assert_eq!(
            parse_config_set("cur_slot", "2", slot).unwrap(),
            ConfigSet::CurSlot(Some(Slot::S3))
        );
        assert_eq!(
            parse_config_set("gamepad_mode", "xinput", slot).unwrap(),
            ConfigSet::GamepadMode(GamepadMode::XInput)
        );
        assert_eq!(
            parse_config_set("rumble.level", "5", slot).unwrap(),
            ConfigSet::RumbleLevel { slot, level: 5 }
        );
        assert_eq!(
            parse_config_set("joy.left_start", "12", slot).unwrap(),
            ConfigSet::Joy {
                slot,
                field: JoyField::LeftStart,
                value: 12
            }
        );
        assert_eq!(
            parse_config_set("trigger.right_end", "200", slot).unwrap(),
            ConfigSet::Trigger {
                slot,
                field: TriggerField::RightEnd,
                value: 200
            }
        );
        assert_eq!(
            parse_config_set("gyro.trigger_mode", "toggle", slot).unwrap(),
            ConfigSet::Gyro {
                slot,
                field: GyroField::TriggerMode,
                value: 1
            }
        );
        assert_eq!(
            parse_config_set("gyro.mapping", "mouse", slot).unwrap(),
            ConfigSet::Gyro {
                slot,
                field: GyroField::Mapping,
                value: 2
            }
        );
        assert_eq!(
            parse_config_set("gyro.key", "L2", slot).unwrap(),
            ConfigSet::GyroKeyMap {
                slot,
                key: KeyMap::L2
            }
        );
        assert_eq!(
            parse_config_set("led.dynamic.color", "0xFF260032", slot).unwrap(),
            ConfigSet::Led {
                slot,
                change: LedChange::DynamicColor(Argb(0xFF26_0032))
            }
        );
        assert_eq!(
            parse_config_set("led.single.color", "#112233", slot).unwrap(),
            ConfigSet::Led {
                slot,
                change: LedChange::SingleColor(Argb(0xFF11_2233))
            }
        );
        assert_eq!(
            parse_config_set("led.hotwheel.speed", "15", slot).unwrap(),
            ConfigSet::Led {
                slot,
                change: LedChange::HotWheelSpeed(15)
            }
        );
        assert_eq!(
            parse_config_set("profile_name", "Combo", slot).unwrap(),
            ConfigSet::ProfileName {
                slot,
                name: "Combo".to_string()
            }
        );
        assert_eq!(
            parse_config_set("map_key.flag", "1", slot).unwrap(),
            ConfigSet::MapKeyFlag { slot, flag: 1 }
        );
    }

    #[test]
    fn parse_config_set_rejects_unknown_fields_and_bad_values() {
        let slot = Slot::S1;
        for field in ["nope", "button_map", "led.dynamic"] {
            let error = parse_config_set(field, "1", slot).unwrap_err();
            assert!(is_invalid_input(&error), "{field}: {error:?}");
        }
        for (field, value) in [
            ("rumble.level", "6"),
            ("cur_slot", "3"),
            ("gyro.trigger_mode", "2"),
            ("gyro.mapping", "4"),
            ("led.dynamic.color", "12345"),
            ("led.dynamic.color", "#1234567"),
            ("joy.left_start", "256"),
            ("gyro.enabled", "maybe"),
        ] {
            let error = parse_config_set(field, value, slot).unwrap_err();
            assert!(is_invalid_input(&error), "{field}={value}: {error:?}");
        }
    }

    #[test]
    fn config_set_plan_preserves_other_record_fields() {
        let config = config_write_fixture();
        let snapshot = config_snapshot(config.clone());
        let set = parse_config_set("trigger.left_end", "200", Slot::S2).unwrap();
        let prepared = set.plan(&snapshot).unwrap().prepare(&snapshot).unwrap();
        let expected = prepared.expected();
        assert_eq!(expected.trigger(Slot::S2).left_end_value, 200);
        assert_eq!(
            expected.trigger(Slot::S2).left_start_value,
            config.trigger(Slot::S2).left_start_value
        );
        assert_eq!(expected.trigger(Slot::S2).flag, 1);
        assert_eq!(expected.joy(Slot::S2), config.joy(Slot::S2));
        assert_eq!(expected.profile_name(Slot::S2), "Slot");
        let trigger_start = eightbitdo_protocol::config_bt2::trigger_offset(Slot::S2);
        assert!(prepared.changed().iter().all(|region| {
            (region.offset() < trigger_start + 8 && region.end() > trigger_start)
                || (region.offset() < eightbitdo_protocol::config_bt2::OFF_CRC16 + 4
                    && region.end() > eightbitdo_protocol::config_bt2::OFF_CRC16)
        }));
    }

    #[test]
    fn config_set_plan_edits_gyro_and_led_while_keeping_other_fields() {
        let config = config_write_fixture();
        let snapshot = config_snapshot(config.clone());

        let set = parse_config_set("gyro.sensitivity", "9", Slot::S1).unwrap();
        let prepared = set.plan(&snapshot).unwrap().prepare(&snapshot).unwrap();
        let gyro = prepared.expected().six_axis(Slot::S1);
        assert_eq!(gyro.sensitivity, 9);
        assert_eq!(gyro.flag, config.six_axis(Slot::S1).flag);
        assert_eq!(
            gyro.dead_compensate,
            config.six_axis(Slot::S1).dead_compensate
        );

        let set = parse_config_set("led.single.color", "#112233", Slot::S2).unwrap();
        let prepared = set.plan(&snapshot).unwrap().prepare(&snapshot).unwrap();
        let theme = prepared.expected().single_theme(Slot::S2);
        assert_eq!(theme.flag, 1);
        assert!(theme.colors.iter().all(|color| *color == Argb(0xFF11_2233)));
        assert_eq!(
            prepared.expected().single_theme(Slot::S1),
            config.single_theme(Slot::S1)
        );
    }

    #[test]
    fn read_config_file_requires_exactly_2768_bytes() {
        let dir = scratch_dir("config-file");
        let short = dir.join("short.bin");
        std::fs::write(&short, vec![0u8; CONFIG_SIZE - 1]).unwrap();
        let error = read_config_file(&short).unwrap_err();
        assert!(
            matches!(error, Error::Io(ref io) if io.kind() == std::io::ErrorKind::InvalidData),
            "unexpected error: {error:?}"
        );

        let good = dir.join("good.bin");
        std::fs::write(&good, config_write_fixture().as_bytes()).unwrap();
        assert!(read_config_file(&good).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confirm_config_write_requires_an_explicit_yes() {
        assert!(confirm_config_write(&mut std::io::Cursor::new(b"yes\n".to_vec())).unwrap());
        assert!(confirm_config_write(&mut std::io::Cursor::new(b"YES\n".to_vec())).unwrap());
        assert!(!confirm_config_write(&mut std::io::Cursor::new(b"y\n".to_vec())).unwrap());
        assert!(!confirm_config_write(&mut std::io::Cursor::new(b"\n".to_vec())).unwrap());
    }

    #[test]
    fn parse_physical_button_accepts_names_aliases_and_indices() {
        assert_eq!(parse_physical_button("A").unwrap(), ButtonIndex::A);
        assert_eq!(parse_physical_button("a").unwrap(), ButtonIndex::A);
        assert_eq!(parse_physical_button("LB").unwrap(), ButtonIndex::L1);
        assert_eq!(parse_physical_button("LT").unwrap(), ButtonIndex::L2);
        assert_eq!(
            parse_physical_button("DPAD-RIGHT").unwrap(),
            ButtonIndex::Right
        );
        assert_eq!(parse_physical_button("VIEW").unwrap(), ButtonIndex::Select);
        assert_eq!(parse_physical_button("MENU").unwrap(), ButtonIndex::Start);
        assert_eq!(parse_physical_button("PL").unwrap(), ButtonIndex::P1);
        assert_eq!(parse_physical_button("PR").unwrap(), ButtonIndex::P2);
        assert_eq!(parse_physical_button("L4").unwrap(), ButtonIndex::P3);
        assert_eq!(parse_physical_button("R4").unwrap(), ButtonIndex::P4);
        assert_eq!(parse_physical_button("P1").unwrap(), ButtonIndex::P1);
        assert_eq!(parse_physical_button("0").unwrap(), ButtonIndex::A);
        assert_eq!(parse_physical_button("21").unwrap(), ButtonIndex::P4);
        assert!(parse_physical_button("22").is_err());
        assert!(parse_physical_button("nope").is_err());
    }

    #[test]
    fn parse_remap_target_accepts_documented_values_and_rejects_227() {
        assert_eq!(parse_remap_target("B").unwrap(), KeyMap::B);
        assert_eq!(parse_remap_target("L2").unwrap(), KeyMap::L2);
        assert_eq!(parse_remap_target("M1").unwrap(), KeyMap::M1);
        assert_eq!(parse_remap_target("null").unwrap(), KeyMap::NONE);
        assert_eq!(parse_remap_target("disable").unwrap(), KeyMap::NONE);
        assert_eq!(parse_remap_target("0").unwrap(), KeyMap::NONE);
        assert_eq!(parse_remap_target("0x4000").unwrap(), KeyMap::L2);
        assert_eq!(parse_remap_target("119").unwrap(), KeyMap::M1);
        // KeyMap_My_NUll (227) is unconfirmed and never written by the
        // official remap path.
        assert!(parse_remap_target("227").is_err());
        assert!(parse_remap_target("0xE3").is_err());
        // Combinations are not a documented remap target.
        assert!(parse_remap_target("0x3000").is_err());
        assert!(parse_remap_target("nope").is_err());
    }

    #[test]
    fn mapping_lines_list_all_22_indices_with_back_button_labels() {
        let mut config = config_write_fixture();
        config.set_map_key_entry(Slot::S1, ButtonIndex::P1, KeyMap::L2);
        let mut record = config.map_key(Slot::S1);
        record.flag = 0x2020_0911;
        config.set_map_key(Slot::S1, &record);
        let lines = mapping_lines(&config, Slot::S1);
        assert_eq!(lines.len(), 24, "header + column header + 22 rows");
        assert!(lines[0].contains("slot 0 (profile 1)"), "{}", lines[0]);
        assert!(lines[0].contains("observed"), "{}", lines[0]);
        assert!(lines[0].contains("0x20200911"), "{}", lines[0]);
        assert!(lines[1].contains("index"));
        let a = lines.iter().find(|line| line.contains("  0  ")).unwrap();
        assert!(a.contains('A'), "{a}");
        let p1 = lines.iter().find(|line| line.contains("P1/PL")).unwrap();
        assert!(p1.contains("L2"), "{p1}");
        assert!(lines.iter().any(|line| line.contains("P2/PR")));
        assert!(lines.iter().any(|line| line.contains("P3/L4")));
        assert!(lines.iter().any(|line| line.contains("P4/R4")));
    }

    #[tokio::test]
    async fn config_remap_session_writes_one_entry_and_preserves_the_rest() {
        let dir = scratch_dir("config-remap");
        let initial = config_write_fixture();
        let before = config_snapshot(initial.clone());
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        config_remap_session(
            &session,
            Slot::S2,
            ButtonIndex::P4,
            KeyMap::L2,
            Some(&dir.join("backup")),
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap();

        let device = transport.config();
        assert_eq!(
            button_mapping(&device, Slot::S2, ButtonIndex::P4),
            KeyMap::L2
        );
        for button in ButtonIndex::ALL {
            if button == ButtonIndex::P4 {
                continue;
            }
            assert_eq!(
                button_mapping(&device, Slot::S2, button),
                button_mapping(&before.config, Slot::S2, button),
                "unrelated mapping {button:?} changed"
            );
        }
        // The official single-entry remap does not change the record flag.
        assert_eq!(
            map_key_flag(&device, Slot::S2),
            map_key_flag(&before.config, Slot::S2)
        );
        // Reserved regions are preserved.
        for (start, end) in [
            (
                eightbitdo_protocol::config_bt2::OFF_LEGACY_A,
                eightbitdo_protocol::config_bt2::OFF_MACROS,
            ),
            (
                eightbitdo_protocol::config_bt2::OFF_LEGACY_B,
                eightbitdo_protocol::config_bt2::OFF_SIX_AXIS,
            ),
        ] {
            assert_eq!(
                &device.as_bytes()[start..end],
                &before.config.as_bytes()[start..end]
            );
        }
        assert!(dir.join("backup").join("config.bin").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_write_session_backs_up_and_verifies_the_full_block() {
        let dir = scratch_dir("config-write");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.set_profile_name(Slot::S2, "Edited").unwrap();
        let file = dir.join("edited.bin");
        std::fs::write(&file, edited.as_bytes()).unwrap();

        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let backup = dir.join("backup");
        config_write_session(
            &session,
            &edited,
            Some(&backup),
            false,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap();

        let device = transport.config();
        assert_eq!(device.profile_name(Slot::S2), "Edited");
        assert_eq!(device.profile_name(Slot::S1), "Slot");
        let mut expected_crc = edited.clone();
        expected_crc.recompute_crc();
        assert_eq!(device.crc16_stored(), expected_crc.crc16_stored());

        // The mandatory pre-write backup holds the unmodified bytes.
        assert_eq!(
            std::fs::read(backup.join("config.bin")).unwrap(),
            initial.as_bytes()
        );
        assert!(backup.join("config.json").is_file());

        let sent = transport.sent();
        let cmds: Vec<u16> = sent
            .iter()
            .map(|frame| CustomHeader::parse(&frame[1..]).unwrap().cmd)
            .collect();
        assert!(
            cmds.iter()
                .filter(|cmd| **cmd == commands::cmd::WRITE_CONFIG)
                .count()
                > 1
        );
        assert_eq!(
            cmds.iter()
                .filter(|cmd| **cmd == commands::cmd::SAVE)
                .count(),
            1
        );
        assert!(
            cmds.iter()
                .filter(|cmd| **cmd == commands::cmd::READ_CONFIG)
                .count()
                > 0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_set_session_writes_only_the_requested_field() {
        let dir = scratch_dir("config-set");
        let initial = config_write_fixture();
        let before = config_snapshot(initial.clone());
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let set = parse_config_set("joy.left_start", "42", Slot::S2).unwrap();
        config_set_session(
            &session,
            &set,
            Some(&dir.join("backup")),
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap();

        let device = transport.config();
        let joy = device.joy(Slot::S2);
        assert_eq!(joy.left_start_value, 42);
        assert_eq!(
            joy.left_end_value,
            before.config.joy(Slot::S2).left_end_value
        );
        assert_eq!(joy.flag, 1);

        let joy_offset = eightbitdo_protocol::config_bt2::joy_offset(Slot::S2);
        let crc = eightbitdo_protocol::config_bt2::OFF_CRC16;
        for offset in 0..CONFIG_SIZE {
            if (joy_offset..joy_offset + 8).contains(&offset) || (crc..crc + 4).contains(&offset) {
                continue;
            }
            assert_eq!(
                before.config.as_bytes()[offset],
                device.as_bytes()[offset],
                "byte 0x{offset:X} changed outside the requested field"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_write_session_rejects_a_reserved_region_change_before_backup() {
        let dir = scratch_dir("config-reserved");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.as_mut_bytes()[eightbitdo_protocol::config_bt2::OFF_LEGACY_A] ^= 0xFF;
        let file = dir.join("edited.bin");
        std::fs::write(&file, edited.as_bytes()).unwrap();

        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let backup = dir.join("backup");
        let error = config_write_session(
            &session,
            &edited,
            Some(&backup),
            false,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)), "{error:?}");
        assert!(!backup.exists(), "no backup should be created");
        assert_eq!(transport.config().as_bytes(), initial.as_bytes());
        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(!wrote, "no write or Save frame should be sent");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_set_ranges_and_flags_are_validated_before_io() {
        let slot = Slot::S1;
        for (field, value) in [
            ("rumble.level", "6"),
            ("joy.left_start", "128"),
            ("joy.left_end", "129"),
            ("joy.right_start", "128"),
            ("joy.right_end", "129"),
            ("gyro.sensitivity", "0"),
            ("gyro.sensitivity", "16"),
            ("led.hotwheel.speed", "16"),
            ("led.dynamic.flag", "2"),
            ("led.hotwheel.flag", "0x10"),
            ("led.single.flag", "2"),
            ("map_key.flag", "2"),
            ("gyro.key", "227"),
            ("gyro.key", "0x3000"),
        ] {
            let error = parse_config_set(field, value, slot).unwrap_err();
            assert!(is_invalid_input(&error), "{field}={value}: {error:?}");
        }
        // Inclusive boundaries and documented single KeyMaps are accepted.
        for (field, value) in [
            ("rumble.level", "0"),
            ("rumble.level", "5"),
            ("joy.left_start", "127"),
            ("joy.left_end", "128"),
            ("gyro.sensitivity", "1"),
            ("gyro.sensitivity", "15"),
            ("led.hotwheel.speed", "15"),
            ("led.dynamic.flag", "1"),
            ("map_key.flag", "0"),
            ("gyro.key", "L2"),
            ("gyro.key", "null"),
        ] {
            assert!(
                parse_config_set(field, value, slot).is_ok(),
                "{field}={value} should be accepted"
            );
        }
    }

    #[test]
    fn raw_config_validation_checks_known_fields() {
        let mut config = config_write_fixture();
        assert!(
            validate_raw_config(&config).is_ok(),
            "{:?}",
            validate_raw_config(&config)
        );

        config.set_gamepad_mode_raw(9);
        assert!(validate_raw_config(&config).is_err());
        config.set_gamepad_mode(GamepadMode::Switch);

        // Unknown cur_slot values are preserved with a warning (6 has been
        // observed on hardware) instead of blocking a raw write.
        config.set_cur_slot_raw(3);
        assert!(validate_raw_config(&config).is_ok());
        assert!(raw_config_warnings(&config)
            .iter()
            .any(|warning| warning.contains("cur_slot 3")));
        config.set_cur_slot_raw(6);
        assert!(validate_raw_config(&config).is_ok());
        assert!(raw_config_warnings(&config)
            .iter()
            .any(|warning| warning.contains("cur_slot 6")));
        config.set_cur_slot_raw(4);
        assert!(validate_raw_config(&config).is_ok());
        assert!(raw_config_warnings(&config).is_empty());

        let mut joy = config.joy(Slot::S2);
        joy.left_start_value = 128;
        config.set_joy(Slot::S2, &joy);
        assert!(validate_raw_config(&config).is_err());
        joy.left_start_value = 127;
        joy.right_end_value = 129;
        config.set_joy(Slot::S2, &joy);
        assert!(validate_raw_config(&config).is_err());
        joy.right_end_value = 128;
        config.set_joy(Slot::S2, &joy);
        assert!(validate_raw_config(&config).is_ok());
        // Raw ranges are checked regardless of the flag value: an opaque
        // disabled flag does not bypass them.
        joy.flag = 0x2020_0911;
        joy.left_start_value = 255;
        config.set_joy(Slot::S2, &joy);
        assert!(validate_raw_config(&config).is_err());
        joy.flag = 1;
        joy.left_start_value = 0;
        config.set_joy(Slot::S2, &joy);

        let mut gyro = config.six_axis(Slot::S1);
        gyro.flag = 1;
        gyro.sensitivity = 0;
        config.set_six_axis(Slot::S1, &gyro);
        assert!(validate_raw_config(&config).is_err());
        gyro.sensitivity = 5;
        gyro.mapping_type = 3;
        config.set_six_axis(Slot::S1, &gyro);
        assert!(validate_raw_config(&config).is_err());
        gyro.mapping_type = 2;
        gyro.trigger_mode = 1;
        config.set_six_axis(Slot::S1, &gyro);
        assert!(validate_raw_config(&config).is_ok());
        // An opaque flag does not bypass the gyro ranges either.
        gyro.flag = 0x2019_0000;
        gyro.sensitivity = 0;
        gyro.mapping_type = 3;
        config.set_six_axis(Slot::S1, &gyro);
        assert!(validate_raw_config(&config).is_err());
        gyro.flag = 1;
        gyro.sensitivity = 5;
        gyro.mapping_type = 0;
        config.set_six_axis(Slot::S1, &gyro);

        let mut hotwheel = config.hotwheel_theme(Slot::S3);
        // Hotwheel speed is also checked with an opaque flag.
        hotwheel.flag = 0x2020_0911;
        hotwheel.speed = 16;
        config.set_hotwheel_theme(Slot::S3, &hotwheel);
        assert!(validate_raw_config(&config).is_err());
        hotwheel.speed = 15;
        config.set_hotwheel_theme(Slot::S3, &hotwheel);
        assert!(validate_raw_config(&config).is_ok());
    }

    #[test]
    fn raw_config_validation_checks_ranges_with_opaque_flags() {
        // An opaque (hardware) flag must not bypass the numeric ranges.

        // joy
        let mut config = config_write_fixture();
        let mut joy = config.joy(Slot::S1);
        joy.flag = 0x2020_0911;
        joy.left_start_value = 128;
        config.set_joy(Slot::S1, &joy);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "joy: opaque flag bypassed the raw range check"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());

        // gyro
        let mut config = config_write_fixture();
        let mut gyro = config.six_axis(Slot::S1);
        gyro.flag = 0x2019_0000;
        gyro.sensitivity = 0;
        config.set_six_axis(Slot::S1, &gyro);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "gyro: opaque flag bypassed the raw range check"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());

        // hotwheel
        let mut config = config_write_fixture();
        let mut hotwheel = config.hotwheel_theme(Slot::S1);
        hotwheel.flag = 0x2020_0911;
        hotwheel.speed = 16;
        config.set_hotwheel_theme(Slot::S1, &hotwheel);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "hotwheel: opaque flag bypassed the raw range check"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());

        // rumble
        let mut config = config_write_fixture();
        let mut rumble = config.rumble(Slot::S1);
        rumble.flag = 0x2020_0911;
        rumble.left = 2.0;
        config.set_rumble(Slot::S1, &rumble);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "rumble: opaque flag bypassed the raw range check"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());
    }

    #[test]
    fn raw_config_validation_rejects_unobserved_flags_without_force_raw() {
        let mut config = config_write_fixture();
        let mut joy = config.joy(Slot::S1);
        joy.flag = 0xDEAD_BEEF;
        config.set_joy(Slot::S1, &joy);
        let error = validate_raw_config_for_write(&config, false).unwrap_err();
        assert!(
            error.to_string().contains("0xDEADBEEF"),
            "the error must name the unobserved flag: {error}"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());
    }

    #[test]
    fn raw_config_validation_accepts_the_real_erased_slot_state() {
        // Mirrors the real slot 2 payload: flag 0xFFFFFFFF, joy all 255,
        // rumble NaN, all 22 map-key entries 0xFFFFFFFF.
        let mut config = config_write_fixture();
        config.set_slot_flag(Slot::S3, 0xFFFF_FFFF);
        let mut joy = config.joy(Slot::S3);
        joy.flag = 0xFFFF_FFFF;
        joy.left_start_value = 255;
        joy.left_end_value = 255;
        joy.right_start_value = 255;
        joy.right_end_value = 255;
        config.set_joy(Slot::S3, &joy);
        let mut rumble = config.rumble(Slot::S3);
        rumble.flag = 0xFFFF_FFFF;
        rumble.left = f32::NAN;
        rumble.right = f32::NAN;
        config.set_rumble(Slot::S3, &rumble);
        let mut trigger = config.trigger(Slot::S3);
        trigger.flag = 0xFFFF_FFFF;
        trigger.left_start_value = 255;
        trigger.left_end_value = 255;
        trigger.right_start_value = 255;
        trigger.right_end_value = 255;
        config.set_trigger(Slot::S3, &trigger);
        let mut map_key = config.map_key(Slot::S3);
        map_key.flag = 0xFFFF_FFFF;
        map_key.keys = [KeyMap::from_bits(0xFFFF_FFFF); 22];
        config.set_map_key(Slot::S3, &map_key);
        assert!(
            validate_raw_config_for_write(&config, false).is_ok(),
            "the real erased slot state must restore without --force-raw: {:?}",
            validate_raw_config_for_write(&config, false)
        );
    }

    #[test]
    fn raw_config_validation_rejects_partial_erased_slot_states() {
        // The erased-slot exception must apply to the complete slot state, not
        // to joy / rumble / map_key independently.

        // joy only
        let mut config = config_write_fixture();
        config.set_slot_flag(Slot::S3, 0xFFFF_FFFF);
        let mut joy = config.joy(Slot::S3);
        joy.flag = 0xFFFF_FFFF;
        joy.left_start_value = 255;
        joy.left_end_value = 255;
        joy.right_start_value = 255;
        joy.right_end_value = 255;
        config.set_joy(Slot::S3, &joy);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "a joy-only erased record must not be accepted"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());

        // rumble only
        let mut config = config_write_fixture();
        config.set_slot_flag(Slot::S3, 0xFFFF_FFFF);
        let mut rumble = config.rumble(Slot::S3);
        rumble.flag = 0xFFFF_FFFF;
        rumble.left = f32::NAN;
        rumble.right = f32::NAN;
        config.set_rumble(Slot::S3, &rumble);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "a rumble-only erased record must not be accepted"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());

        // map_key only
        let mut config = config_write_fixture();
        config.set_slot_flag(Slot::S3, 0xFFFF_FFFF);
        let mut map_key = config.map_key(Slot::S3);
        map_key.flag = 0xFFFF_FFFF;
        map_key.keys = [KeyMap::from_bits(0xFFFF_FFFF); 22];
        config.set_map_key(Slot::S3, &map_key);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "a map_key-only erased record must not be accepted"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());

        // joy + rumble but normal map_key
        let mut config = config_write_fixture();
        config.set_slot_flag(Slot::S3, 0xFFFF_FFFF);
        let mut joy = config.joy(Slot::S3);
        joy.flag = 0xFFFF_FFFF;
        joy.left_start_value = 255;
        joy.left_end_value = 255;
        joy.right_start_value = 255;
        joy.right_end_value = 255;
        config.set_joy(Slot::S3, &joy);
        let mut rumble = config.rumble(Slot::S3);
        rumble.flag = 0xFFFF_FFFF;
        rumble.left = f32::NAN;
        rumble.right = f32::NAN;
        config.set_rumble(Slot::S3, &rumble);
        assert!(validate_raw_config_for_write(&config, false).is_err());
        assert!(validate_raw_config_for_write(&config, true).is_ok());
    }

    /// Load the local real-device dump used by the opt-in E2E tests.
    ///
    /// `CARGO_MANIFEST_DIR` points at `crates/eightbitdo-cli`, so the repository
    /// root dump is two directories up. `EIGHTBITDO_REAL_DUMP` overrides it.
    /// The loader verifies the evidence SHA-256 and reports every skip reason;
    /// the tests are `#[ignore]`d so a missing dump is never a silent pass.
    fn real_device_dump_bytes() -> Option<Vec<u8>> {
        const EVIDENCE_SHA256: &str =
            "1dca47facfc1c0394a8893ac0a5b7ef80678c51081001886a4bf17ce2f59c600";
        let path = if let Some(path) = std::env::var_os("EIGHTBITDO_REAL_DUMP") {
            PathBuf::from(path)
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/restore-baseline/config.bin")
        };
        if !path.is_file() {
            eprintln!(
                "skip: real device dump not found at {} (set EIGHTBITDO_REAL_DUMP to override)",
                path.display()
            );
            return None;
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!(
                    "skip: cannot read real device dump {}: {error}",
                    path.display()
                );
                return None;
            }
        };
        let sha256 = sha256_hex(&bytes);
        if !sha256.eq_ignore_ascii_case(EVIDENCE_SHA256) {
            eprintln!(
                "skip: real device dump SHA-256 {sha256} does not match evidence {EVIDENCE_SHA256}"
            );
            return None;
        }
        eprintln!(
            "real dump: using {} (sha256 {sha256}, {} bytes)",
            path.display(),
            bytes.len()
        );
        Some(bytes)
    }

    #[test]
    #[ignore = "requires the local real-device dump (EIGHTBITDO_REAL_DUMP or .tmp/restore-baseline/config.bin)"]
    fn real_device_dump_is_accepted_when_local_evidence_exists() {
        let Some(bytes) = real_device_dump_bytes() else {
            return;
        };
        assert_eq!(bytes.len(), CONFIG_SIZE);
        let config = Bt2Config::from_bytes(&bytes).expect("real dump length");
        assert!(
            validate_raw_config_for_write(&config, false).is_ok(),
            "the real device dump must pass raw validation without --force-raw: {:?}",
            validate_raw_config_for_write(&config, false)
        );
    }

    #[tokio::test]
    #[ignore = "requires the local real-device dump (EIGHTBITDO_REAL_DUMP or .tmp/restore-baseline/config.bin)"]
    async fn config_write_accepts_the_real_device_dump_when_present() {
        let Some(bytes) = real_device_dump_bytes() else {
            return;
        };
        let initial = Bt2Config::from_bytes(&bytes).expect("real dump length");
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let dir = scratch_dir("config-real-dump");
        let mut edited = initial.clone();
        edited.set_profile_name(Slot::S2, "RealDump").unwrap();
        config_write_session(
            &session,
            &edited,
            Some(&dir.join("backup")),
            false,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .expect("the real device dump must be writable without --force-raw");
        let device = transport.config();
        assert_eq!(device.profile_name(Slot::S2), "RealDump");
        assert_eq!(map_key_flag(&device, Slot::S3), 0xFFFF_FFFF);
        assert_eq!(device.joy(Slot::S3).left_start_value, 255);
        assert_eq!(
            transport
                .sent()
                .iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE
                })
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn raw_config_validation_rejects_unobserved_slot_flags() {
        let mut config = config_write_fixture();
        config.set_slot_flag(Slot::S1, 0xDEAD_BEEF);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "an unobserved slot_flags value must require --force-raw"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());
    }

    #[test]
    fn raw_config_validation_rejects_a_gyro_key_map_sentinel() {
        // The real gyro key_map is 0; 0xFFFFFFFF is not accepted even for an
        // otherwise fully-erased slot.
        let mut config = synthetic_observed_dump();
        let mut gyro = config.six_axis(Slot::S3);
        gyro.key_map = KeyMap::from_bits(0xFFFF_FFFF);
        config.set_six_axis(Slot::S3, &gyro);
        assert!(
            validate_raw_config_for_write(&config, false).is_err(),
            "gyro.key_map 0xFFFFFFFF must not be accepted"
        );
        assert!(validate_raw_config_for_write(&config, true).is_ok());
    }

    #[test]
    fn raw_config_validation_rejects_partial_map_key_sentinel_and_gyro_context() {
        // One 0xFFFFFFFF entry in an otherwise populated slot is corruption,
        // not the erased-slot sentinel.
        let mut config = config_write_fixture();
        let mut map_key = config.map_key(Slot::S1);
        map_key.flag = 0xFFFF_FFFF;
        map_key.keys[0] = KeyMap::from_bits(0xFFFF_FFFF);
        config.set_map_key(Slot::S1, &map_key);
        assert!(validate_raw_config_for_write(&config, false).is_err());
        assert!(validate_raw_config_for_write(&config, true).is_ok());

        // gyro.key_map sentinel requires an erased gyro flag.
        let mut config = config_write_fixture();
        let mut gyro = config.six_axis(Slot::S2);
        gyro.flag = 0x2020_0911;
        gyro.key_map = KeyMap::from_bits(0xFFFF_FFFF);
        config.set_six_axis(Slot::S2, &gyro);
        assert!(validate_raw_config_for_write(&config, false).is_err());
        assert!(validate_raw_config_for_write(&config, true).is_ok());
    }

    #[test]
    fn raw_config_validation_rejects_undocumented_keymaps_flags_and_macro_fields() {
        let config = config_write_fixture();
        assert!(validate_raw_config(&config).is_ok());

        // A 227 or combined KeyMap in one of the 22 mappings must be refused
        // on the raw path too (the typed remap path rejects them).
        for bad in [KeyMap::MY_NULL, KeyMap::A | KeyMap::B] {
            let mut edited = config.clone();
            edited.set_map_key_entry(Slot::S2, ButtonIndex::A, bad);
            let error = validate_raw_config(&edited).unwrap_err();
            assert!(
                error.to_string().contains("button mapping"),
                "0x{:08X}: {error}",
                bad.bits()
            );
        }

        let mut edited = config.clone();
        let mut gyro = edited.six_axis(Slot::S1);
        gyro.key_map = KeyMap::MY_NULL;
        edited.set_six_axis(Slot::S1, &gyro);
        assert!(validate_raw_config(&edited)
            .unwrap_err()
            .to_string()
            .contains("gyro[0].key_map"));

        let mut edited = config.clone();
        let mut record = edited.macro_record(Slot::S1, MacroIndex::M1);
        record.max_steps = 300;
        edited.set_macro_record(Slot::S1, MacroIndex::M1, &record);
        assert!(validate_raw_config(&edited)
            .unwrap_err()
            .to_string()
            .contains("max_steps 300"));

        let mut edited = config.clone();
        let mut header = edited.macro_slot(Slot::S1);
        header.total_cnt = 5;
        edited.set_macro_slot(Slot::S1, &header);
        assert!(validate_raw_config(&edited)
            .unwrap_err()
            .to_string()
            .contains("total_cnt 5"));

        // M2: a macro trigger may be any KeyMap (matching `macro write
        // --trigger`), so an undocumented / combined value is accepted.
        let mut edited = config.clone();
        let mut header = edited.macro_slot(Slot::S1);
        header.records[0].max_steps = 10;
        header.records[0].key_map = KeyMap::MY_NULL;
        edited.set_macro_slot(Slot::S1, &header);
        assert!(validate_raw_config(&edited).is_ok());

        // A valid dump (0/0xFFFF max_steps, documented key maps) still passes.
        let mut valid = config.clone();
        valid.set_map_key_entry(Slot::S1, ButtonIndex::L2, KeyMap::P2);
        valid.set_map_key_entry(Slot::S1, ButtonIndex::P1, KeyMap::NONE);
        assert!(validate_raw_config(&valid).is_ok());
    }

    #[test]
    fn raw_config_validation_accepts_observed_record_flags() {
        // Real hardware stores opaque flag values in every record
        // (DEVICE_CONFIG_EVIDENCE): 0x20200911 on populated slots,
        // 0xFFFFFFFF on the erased slot, and 0x20190000 for gyro slot 0.
        let mut config = config_write_fixture();
        for slot in Slot::ALL {
            let (common, gyro_flag) = match slot {
                Slot::S1 => (0x2020_0911, 0x2019_0000),
                Slot::S2 => (0x2020_0911, 0x2020_0911),
                Slot::S3 => (0xFFFF_FFFF, 0x2020_0911),
            };
            let mut joy = config.joy(slot);
            joy.flag = common;
            config.set_joy(slot, &joy);
            let mut trigger = config.trigger(slot);
            trigger.flag = common;
            config.set_trigger(slot, &trigger);
            let mut rumble = config.rumble(slot);
            rumble.flag = common;
            config.set_rumble(slot, &rumble);
            let mut special = config.special(slot);
            special.flag = common;
            config.set_special(slot, &special);
            let mut gyro = config.six_axis(slot);
            gyro.flag = gyro_flag;
            config.set_six_axis(slot, &gyro);
            let mut map_key = config.map_key(slot);
            map_key.flag = common;
            config.set_map_key(slot, &map_key);
            let mut dynamic = config.dynamic_theme(slot);
            dynamic.flag = common;
            config.set_dynamic_theme(slot, &dynamic);
            let mut hotwheel = config.hotwheel_theme(slot);
            hotwheel.flag = common;
            config.set_hotwheel_theme(slot, &hotwheel);
            let mut single = config.single_theme(slot);
            single.flag = common;
            config.set_single_theme(slot, &single);
            let mut macro_slot = config.macro_slot(slot);
            macro_slot.flag = common;
            config.set_macro_slot(slot, &macro_slot);
        }
        assert!(
            validate_raw_config(&config).is_ok(),
            "hardware-observed record flags must pass raw validation: {:?}",
            validate_raw_config(&config)
        );
        // No --force-raw needed on the write-path wrapper either.
        assert!(validate_raw_config_for_write(&config, false).is_ok());

        // The typed flag field keeps its 0/1 restriction.
        assert!(parse_config_set("map_key.flag", "0x20200911", Slot::S1).is_err());
        assert!(parse_config_set("map_key.flag", "0xFFFFFFFF", Slot::S1).is_err());
    }

    #[test]
    fn raw_config_validation_accepts_the_erased_map_key_sentinel() {
        // The real erased slot (slot 2) stores 0xFFFFFFFF in all 22 entries;
        // raw restore must accept that sentinel, while the typed remap path
        // stays strict.
        let mut config = config_write_fixture();
        let mut record = config.map_key(Slot::S3);
        record.flag = 0xFFFF_FFFF;
        record.keys = [KeyMap::from_bits(0xFFFF_FFFF); 22];
        config.set_map_key(Slot::S3, &record);
        config.set_slot_flag(Slot::S3, 0xFFFF_FFFF);
        // The map_key sentinel is slot-wide: joy and rumble must be erased too.
        let mut joy = config.joy(Slot::S3);
        joy.flag = 0xFFFF_FFFF;
        joy.left_start_value = 255;
        joy.left_end_value = 255;
        joy.right_start_value = 255;
        joy.right_end_value = 255;
        config.set_joy(Slot::S3, &joy);
        let mut rumble = config.rumble(Slot::S3);
        rumble.flag = 0xFFFF_FFFF;
        rumble.left = f32::NAN;
        rumble.right = f32::NAN;
        config.set_rumble(Slot::S3, &rumble);

        assert!(
            validate_raw_config(&config).is_ok(),
            "the complete erased slot must pass raw validation: {:?}",
            validate_raw_config(&config)
        );
        assert!(validate_raw_config_for_write(&config, false).is_ok());
        assert!(parse_remap_target("0xFFFFFFFF").is_err());
    }

    /// Deterministic synthetic dump carrying the real-device values from
    /// `DEVICE_CONFIG_EVIDENCE.md` (PII-free): observed flags on every record,
    /// gyro slot0 `0x20190000`, and the fully-erased slot 3 with slot flag
    /// `0xFFFFFFFF`, all-255 joy, NaN rumble and 22 erased key-map entries.
    fn synthetic_observed_dump() -> Bt2Config {
        let mut dump = config_write_fixture();
        for slot in Slot::ALL {
            let (common, gyro_flag) = match slot {
                Slot::S1 => (0x2020_0911, 0x2019_0000),
                Slot::S2 => (0x2020_0911, 0x2020_0911),
                Slot::S3 => (0xFFFF_FFFF, 0x2020_0911),
            };
            let mut joy = dump.joy(slot);
            joy.flag = common;
            dump.set_joy(slot, &joy);
            let mut trigger = dump.trigger(slot);
            trigger.flag = common;
            dump.set_trigger(slot, &trigger);
            let mut rumble = dump.rumble(slot);
            rumble.flag = common;
            dump.set_rumble(slot, &rumble);
            let mut special = dump.special(slot);
            special.flag = common;
            dump.set_special(slot, &special);
            let mut gyro = dump.six_axis(slot);
            gyro.flag = gyro_flag;
            dump.set_six_axis(slot, &gyro);
            let mut map_key = dump.map_key(slot);
            map_key.flag = common;
            if slot != Slot::S3 {
                for (index, button) in ButtonIndex::ALL.into_iter().enumerate() {
                    map_key.keys[index] = button.default_key();
                }
            }
            dump.set_map_key(slot, &map_key);
            let mut dynamic = dump.dynamic_theme(slot);
            dynamic.flag = common;
            dump.set_dynamic_theme(slot, &dynamic);
            let mut hotwheel = dump.hotwheel_theme(slot);
            hotwheel.flag = common;
            dump.set_hotwheel_theme(slot, &hotwheel);
            let mut single = dump.single_theme(slot);
            single.flag = common;
            dump.set_single_theme(slot, &single);
            let mut macro_slot = dump.macro_slot(slot);
            macro_slot.flag = common;
            dump.set_macro_slot(slot, &macro_slot);
        }
        // The real erased slot also has an erased slot_flags entry and a
        // fully erased map_key / joy / rumble state.
        dump.set_slot_flag(Slot::S3, 0xFFFF_FFFF);
        let mut erased = dump.map_key(Slot::S3);
        erased.keys = [KeyMap::from_bits(0xFFFF_FFFF); 22];
        dump.set_map_key(Slot::S3, &erased);
        let mut joy = dump.joy(Slot::S3);
        joy.left_start_value = 255;
        joy.left_end_value = 255;
        joy.right_start_value = 255;
        joy.right_end_value = 255;
        dump.set_joy(Slot::S3, &joy);
        let mut trigger = dump.trigger(Slot::S3);
        trigger.left_start_value = 255;
        trigger.left_end_value = 255;
        trigger.right_start_value = 255;
        trigger.right_end_value = 255;
        dump.set_trigger(Slot::S3, &trigger);
        let mut rumble = dump.rumble(Slot::S3);
        rumble.left = f32::NAN;
        rumble.right = f32::NAN;
        dump.set_rumble(Slot::S3, &rumble);
        dump
    }

    #[test]
    fn raw_config_validation_accepts_a_synthetic_real_dump() {
        let dump = synthetic_observed_dump();
        assert!(
            validate_raw_config_for_write(&dump, false).is_ok(),
            "the synthetic real dump must pass raw validation without --force-raw: {:?}",
            validate_raw_config_for_write(&dump, false)
        );
    }

    #[tokio::test]
    async fn config_write_restores_an_observed_flag_dump_without_force_raw() {
        let dir = scratch_dir("config-observed-flags");
        let initial = config_write_fixture();
        let dump = synthetic_observed_dump();

        assert!(
            validate_raw_config_for_write(&dump, false).is_ok(),
            "the observed dump must pass raw validation without --force-raw: {:?}",
            validate_raw_config_for_write(&dump, false)
        );

        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        config_write_session(
            &session,
            &dump,
            Some(&dir.join("backup")),
            false,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .expect("the observed dump must be restorable without --force-raw");

        let device = transport.config();
        assert_eq!(map_key_flag(&device, Slot::S1), 0x2020_0911);
        assert_eq!(map_key_flag(&device, Slot::S3), 0xFFFF_FFFF);
        assert_eq!(device.six_axis(Slot::S1).flag, 0x2019_0000);
        assert_eq!(
            device.map_key(Slot::S3).keys[0],
            KeyMap::from_bits(0xFFFF_FFFF)
        );
        assert_eq!(
            transport
                .sent()
                .iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE
                })
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn force_raw_downgrades_unknown_fields_but_keeps_the_reserved_guard() {
        let config = config_write_fixture();
        let mut undocumented = config.clone();
        // A KeyMap value this tool refuses on the typed path.
        undocumented.set_map_key_entry(Slot::S1, ButtonIndex::A, KeyMap::MY_NULL);
        assert!(validate_raw_config_for_write(&undocumented, false).is_err());
        assert!(validate_raw_config_for_write(&undocumented, true).is_ok());

        // A range/enum problem is downgraded as well.
        let mut out_of_range = config.clone();
        out_of_range.set_gamepad_mode_raw(9);
        assert!(validate_raw_config_for_write(&out_of_range, false).is_err());
        assert!(validate_raw_config_for_write(&out_of_range, true).is_ok());

        // --force-raw does not bypass the reserved-region guard: that guard
        // lives in PreparedConfigWrite::from_buffer.
        let snapshot = config_snapshot(config.clone());
        let mut drifted = config.clone();
        drifted.as_mut_bytes()[eightbitdo_protocol::config_bt2::OFF_LEGACY_A] ^= 0xFF;
        drifted.set_map_key_entry(Slot::S1, ButtonIndex::A, KeyMap::MY_NULL);
        assert!(PreparedConfigWrite::from_buffer(&snapshot, drifted).is_err());
    }

    #[test]
    fn raw_config_validation_accepts_combined_macro_key_maps() {
        let config = config_write_fixture();
        let mut edited = config.clone();
        let mut header = edited.macro_slot(Slot::S2);
        header.records[0].max_steps = 10;
        header.records[0].key_map = KeyMap::A | KeyMap::B;
        edited.set_macro_slot(Slot::S2, &header);
        assert!(
            validate_raw_config(&edited).is_ok(),
            "{:?}",
            validate_raw_config(&edited)
        );
        // The same dump also passes the write-path wrapper without --force-raw.
        assert!(validate_raw_config_for_write(&edited, false).is_ok());
    }

    #[tokio::test]
    async fn config_write_end_to_end_restores_a_combined_macro_trigger() {
        let dir = scratch_dir("config-combined-trigger");
        let initial = config_write_fixture();
        let mut dump = initial.clone();
        let mut header = dump.macro_slot(Slot::S1);
        header.records[0].max_steps = 10;
        header.records[0].key_map = KeyMap::A | KeyMap::B;
        dump.set_macro_slot(Slot::S1, &header);

        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        config_write_session(
            &session,
            &dump,
            Some(&dir.join("backup")),
            false,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap();
        let restored = transport.config();
        assert_eq!(
            restored.macro_record(Slot::S1, MacroIndex::M1).key_map,
            KeyMap::A | KeyMap::B
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_chunk_is_validated_before_the_connection() {
        for chunk in [None, Some(1), Some(3), Some(74), Some(180)] {
            assert!(validate_write_chunk_preconnect(chunk).is_ok(), "{chunk:?}");
        }
        for chunk in [Some(0), Some(181), Some(u16::MAX)] {
            let error = validate_write_chunk_preconnect(chunk).unwrap_err();
            assert!(matches!(error, Error::InvalidRequest(_)), "{error:?}");
        }
    }

    #[tokio::test]
    async fn config_write_accept_reserved_drift_uses_device_reserved_bytes() {
        let dir = scratch_dir("config-drift");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        // The file carries different reserved bytes (e.g. after the device
        // normalized them) plus one intended change.
        edited.as_mut_bytes()[eightbitdo_protocol::config_bt2::OFF_LEGACY_A] ^= 0xFF;
        edited.set_profile_name(Slot::S2, "Drift").unwrap();

        // Without the flag the reserved difference is refused.
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let error = config_write_session(
            &session,
            &edited,
            Some(&dir.join("rejected")),
            false,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)), "{error:?}");

        // With the flag the device's reserved bytes win and the intended
        // change is written.
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        config_write_session(
            &session,
            &edited,
            Some(&dir.join("accepted")),
            true,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap();
        let device = transport.config();
        assert_eq!(device.profile_name(Slot::S2), "Drift");
        for (start, end) in RESERVED_RANGES {
            assert_eq!(
                &device.as_bytes()[start..end],
                &initial.as_bytes()[start..end]
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_confirm_abort_keeps_backup_and_sends_no_write_frames() {
        let dir = scratch_dir("config-abort");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.set_profile_name(Slot::S2, "Aborted").unwrap();
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let backup = dir.join("backup");
        let mut reader = std::io::Cursor::new(b"no\n".to_vec());
        let mut prompt = || confirm_config_write(&mut reader);
        config_write_session(
            &session,
            &edited,
            Some(&backup),
            false,
            ConfigConfirmation::Reader(&mut prompt),
            None,
        )
        .await
        .unwrap();

        assert_eq!(transport.config().as_bytes(), initial.as_bytes());
        assert!(
            backup.join("config.bin").is_file(),
            "the pre-write backup must remain after an aborted confirmation"
        );
        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(!wrote, "confirmation abort must not send write/Save frames");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_write_aborts_when_the_device_changed_after_the_snapshot() {
        let dir = scratch_dir("config-toctou");
        let initial = config_write_fixture();
        let stale = config_snapshot(initial.clone());
        let mut edited = initial.clone();
        edited.set_profile_name(Slot::S2, "Planned").unwrap();
        let prepared = PreparedConfigWrite::from_buffer(&stale, edited).unwrap();

        // Another host changed the profile after the snapshot was taken.
        let mut changed = initial.clone();
        changed.set_profile_name(Slot::S1, "Changed").unwrap();
        changed.recompute_crc();
        let transport = ConfigDeviceTransport::new(changed.clone());
        let session = Session::new(&transport);
        let backup = dir.join("backup");
        let error = apply_config_prepared(
            &session,
            &stale,
            &prepared,
            Some(&backup),
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)), "{error:?}");
        assert!(error.to_string().contains("changed"), "{error}");
        assert!(
            error.to_string().contains(&backup.display().to_string()),
            "the abort message must name the pre-write backup: {error}"
        );
        assert!(
            error.to_string().contains("plan-time snapshot"),
            "the abort message must say the backup is not the current state: {error}"
        );
        assert_eq!(transport.config().as_bytes(), changed.as_bytes());
        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(!wrote, "TOCTOU abort must not send write/Save frames");
        assert!(backup.join("config.bin").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_write_accept_reserved_drift_restores_a_backup_after_normalization() {
        let dir = scratch_dir("config-drift-recovery");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.as_mut_bytes()[eightbitdo_protocol::config_bt2::OFF_LEGACY_A] ^= 0xFF;
        edited.set_profile_name(Slot::S2, "First").unwrap();

        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let backup = dir.join("backup");
        config_write_session(
            &session,
            &edited,
            Some(&backup),
            true,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap();
        assert_eq!(transport.config().profile_name(Slot::S2), "First");

        // The device normalized the reserved region after the Save.
        let mut normalized = transport.config();
        normalized.as_mut_bytes()[eightbitdo_protocol::config_bt2::OFF_LEGACY_A
            ..eightbitdo_protocol::config_bt2::OFF_MACROS]
            .fill(0x33);
        normalized.recompute_crc();
        transport.set_config(normalized.clone());

        // Recovering the pre-write backup by the documented command fails
        // without the explicit drift option.
        let backup_config =
            Bt2Config::from_bytes(&std::fs::read(backup.join("config.bin")).unwrap()).unwrap();
        let error = config_write_session(
            &session,
            &backup_config,
            Some(&dir.join("rejected")),
            false,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .expect_err("reserved drift must be refused without --accept-reserved-drift");
        assert!(matches!(error, Error::InvalidRequest(_)), "{error:?}");

        // The documented recovery command (`--accept-reserved-drift`) restores
        // the names while keeping the device's current reserved bytes.
        let recovery_transport = ConfigDeviceTransport::new(normalized.clone());
        let recovery_session = Session::new(&recovery_transport);
        config_write_session(
            &recovery_session,
            &backup_config,
            Some(&dir.join("recovered")),
            true,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap();
        let restored = recovery_transport.config();
        assert_eq!(restored.profile_name(Slot::S2), "Slot");
        assert_eq!(
            &restored.as_bytes()[eightbitdo_protocol::config_bt2::OFF_LEGACY_A
                ..eightbitdo_protocol::config_bt2::OFF_MACROS],
            &normalized.as_bytes()[eightbitdo_protocol::config_bt2::OFF_LEGACY_A
                ..eightbitdo_protocol::config_bt2::OFF_MACROS]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_write_end_to_end_at_mtu_23_uses_three_byte_chunks() {
        let dir = scratch_dir("config-mtu23");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.set_profile_name(Slot::S2, "Tiny").unwrap();

        let transport = ConfigDeviceTransport::with_mtu(initial.clone(), 23);
        let session = Session::new(&transport);
        config_write_session(
            &session,
            &edited,
            Some(&dir.join("backup")),
            false,
            ConfigConfirmation::AssumeYes,
            None,
        )
        .await
        .unwrap();

        let chunk_lens: Vec<u16> = transport
            .sent()
            .iter()
            .filter_map(|frame| {
                let header = CustomHeader::parse(&frame[1..]).unwrap();
                (header.cmd == commands::cmd::WRITE_CONFIG).then_some(header.chunk_len)
            })
            .collect();
        assert_eq!(chunk_lens.len(), CONFIG_SIZE.div_ceil(3));
        let (last, head) = chunk_lens.split_last().expect("at least one chunk");
        assert!(head.iter().all(|len| *len == 3), "{chunk_lens:?}");
        assert_eq!(*last, (CONFIG_SIZE % 3) as u16, "{chunk_lens:?}");
        assert_eq!(transport.config().profile_name(Slot::S2), "Tiny");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn config_write_honours_the_write_chunk_override() {
        let dir = scratch_dir("config-chunk");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.set_profile_name(Slot::S2, "Chunk").unwrap();
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        config_write_session(
            &session,
            &edited,
            Some(&dir.join("backup")),
            false,
            ConfigConfirmation::AssumeYes,
            Some(180),
        )
        .await
        .unwrap();
        let sent = transport.sent();
        let chunk_lens: Vec<u16> = sent
            .iter()
            .filter_map(|frame| {
                let header = CustomHeader::parse(&frame[1..]).unwrap();
                (header.cmd == commands::cmd::WRITE_CONFIG).then_some(header.chunk_len)
            })
            .collect();
        assert!(!chunk_lens.is_empty());
        assert!(chunk_lens.iter().all(|len| *len <= 180), "{chunk_lens:?}");
        assert_eq!(chunk_lens[0], 180);

        // An override above the negotiated payload is rejected before the
        // backup is created and before any frame is sent.
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Session::new(&transport);
        let rejected = dir.join("rejected-chunk");
        let error = config_write_session(
            &session,
            &edited,
            Some(&rejected),
            false,
            ConfigConfirmation::AssumeYes,
            Some(181),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)), "{error:?}");
        assert!(
            !rejected.exists(),
            "chunk validation must run before backup"
        );
        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(!wrote);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_session_config_commands_accept_the_top_level_options() {
        assert_eq!(
            parse_session_command("config set joy.left_start 42 --slot 2 --yes"),
            SessionCommand::ConfigSet {
                field: "joy.left_start".to_string(),
                value: "42".to_string(),
                slot: 2,
                backup_dir: None,
                yes: true,
                write_chunk: None,
            }
        );
        assert_eq!(
            parse_session_command(
                "config remap A B --slot 1 --backup-dir /tmp/b --write-chunk 74 --yes"
            ),
            SessionCommand::ConfigRemap {
                from: "A".to_string(),
                to: "B".to_string(),
                slot: 1,
                backup_dir: Some(PathBuf::from("/tmp/b")),
                yes: true,
                write_chunk: Some(74),
            }
        );
        assert_eq!(
            parse_session_command("config map show"),
            SessionCommand::ConfigMapShow { slot: 0 }
        );
        assert_eq!(
            parse_session_command("config map show --slot 2"),
            SessionCommand::ConfigMapShow { slot: 2 }
        );
        assert_eq!(
            parse_session_command("profile switch 1"),
            SessionCommand::ProfileSwitch {
                slot: 1,
                yes: false
            }
        );
        assert_eq!(
            parse_session_command("profile switch 2 --yes"),
            SessionCommand::ProfileSwitch { slot: 2, yes: true }
        );
        assert_eq!(
            parse_session_command(
                "config write --file /tmp/c.bin --backup-dir /tmp/b --accept-reserved-drift \
                 --force-raw --write-chunk 180 --yes"
            ),
            SessionCommand::ConfigWrite {
                file: PathBuf::from("/tmp/c.bin"),
                backup_dir: Some(PathBuf::from("/tmp/b")),
                accept_reserved_drift: true,
                force_raw: true,
                yes: true,
                write_chunk: Some(180),
            }
        );
        // Unknown subcommands, missing values and duplicate options are
        // rejected as a whole.
        for line in [
            "config set",
            "config set field",
            "config remap A",
            "config write --file",
            "config write",
            "config map",
            "config set a b --slot 1 --slot 2",
            "config write --file /tmp/c.bin --yes --yes",
            "config unknown",
            "profile",
            "profile switch",
            "profile switch 1 --yes --yes",
            "profile switch 1 extra",
            "profile other 1",
        ] {
            assert!(
                matches!(parse_session_command(line), SessionCommand::Unknown(_)),
                "{line}"
            );
        }
    }

    #[tokio::test]
    async fn session_repl_config_set_writes_backup_and_reads_back() {
        let dir = scratch_dir("session-config-set");
        let initial = config_write_fixture();
        let before = config_snapshot(initial.clone());
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let backup = dir.join("backup");
        let input = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {} --yes\n\
             config map show --slot 1\n\
             quit\n",
            backup.display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");

        let device = transport.config();
        assert_eq!(device.joy(Slot::S2).left_start_value, 42);
        assert_eq!(
            device.joy(Slot::S2).left_end_value,
            before.config.joy(Slot::S2).left_end_value
        );
        assert!(backup.join("config.bin").is_file());
        assert!(backup.join("config.json").is_file());
        let frames = transport.sent();
        assert!(frames.iter().any(|frame| {
            CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_CONFIG
        }));
        assert_eq!(
            frames
                .iter()
                .filter(|frame| CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE)
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_set_abort_confirmation_keeps_backup_and_sends_no_write() {
        let dir = scratch_dir("session-config-abort");
        let initial = config_write_fixture();
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let backup = dir.join("backup");
        let input = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {}\nno\nquit\n",
            backup.display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");

        assert_eq!(transport.config().as_bytes(), initial.as_bytes());
        assert!(backup.join("config.bin").is_file(), "backup must remain");
        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(!wrote, "aborted confirmation must not write");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_write_force_raw_keeps_the_reserved_guard() {
        let dir = scratch_dir("session-config-force");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.as_mut_bytes()[eightbitdo_protocol::config_bt2::OFF_LEGACY_A] ^= 0xFF;
        edited.set_map_key_entry(Slot::S1, ButtonIndex::A, KeyMap::MY_NULL);
        let file = dir.join("edited.bin");
        std::fs::write(&file, edited.as_bytes()).unwrap();

        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let input = format!(
            "config write --file {} --backup-dir {} --force-raw --yes\nquit\n",
            file.display(),
            dir.join("backup").display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");

        // --force-raw downgrades the key_map check, but the reserved-region
        // guard still refuses the file before any write/Save frame.
        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(
            !wrote,
            "reserved drift must be refused even with --force-raw"
        );
        assert_eq!(transport.config().as_bytes(), initial.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_writes_are_gated_without_the_opt_in_flag() {
        let dir = scratch_dir("session-config-gate");
        let initial = config_write_fixture();
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let input = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {} --yes\nquit\n",
            dir.join("backup").display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            false,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");
        assert!(
            transport.sent().is_empty(),
            "gated config write sent frames"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_write_holds_the_operation_lock_against_keepalive() {
        let dir = scratch_dir("session-config-lock");
        let initial = config_write_fixture();
        let transport = ConfigDeviceTransport::new(initial.clone()).stalling();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_millis(5))
            .await
            .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;

        let state = Arc::clone(&keepalive.state);
        let backup = dir.join("backup");
        let input = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {} --yes\nquit\n",
            backup.display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        shutdown_session(&session, keepalive).await;
        assert!(result.is_ok(), "{result:?}");

        let frames = transport.sent();
        let first_write = frames.iter().position(|frame| {
            CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_CONFIG
        });
        let save = frames
            .iter()
            .position(|frame| CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE);
        let (first_write, save) = (first_write.unwrap(), save.unwrap());
        assert!(first_write < save);
        assert!(
            frames[first_write..=save].iter().all(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd != commands::cmd::INPUT
            }),
            "keepalive 0x0003 interleaved with the config write"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_set_typed_yes_writes_the_change() {
        let dir = scratch_dir("session-config-typed-yes");
        let initial = config_write_fixture();
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let backup = dir.join("backup");
        let input = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {}\nyes\nquit\n",
            backup.display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(transport.config().joy(Slot::S2).left_start_value, 42);
        assert!(backup.join("config.bin").is_file());
        assert_eq!(
            transport
                .sent()
                .iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE
                })
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_remap_executes_the_wired_flow() {
        let dir = scratch_dir("session-config-remap");
        let initial = config_write_fixture();
        let before = config_snapshot(initial.clone());
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let backup = dir.join("backup");
        let input = format!(
            "config remap A B --slot 1 --backup-dir {} --yes\nquit\n",
            backup.display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");

        let device = transport.config();
        assert_eq!(button_mapping(&device, Slot::S2, ButtonIndex::A), KeyMap::B);
        for button in ButtonIndex::ALL {
            if button == ButtonIndex::A {
                continue;
            }
            assert_eq!(
                button_mapping(&device, Slot::S2, button),
                button_mapping(&before.config, Slot::S2, button)
            );
        }
        assert_eq!(
            map_key_flag(&device, Slot::S2),
            map_key_flag(&before.config, Slot::S2)
        );
        assert!(backup.join("config.bin").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_write_survives_ctrl_c_until_completion() {
        let dir = scratch_dir("session-config-ctrl-c");
        let initial = config_write_fixture();
        let transport =
            ConfigDeviceTransport::new(initial.clone()).with_write_delay(Duration::from_millis(2));
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());

        let watched = transport.clone();
        let notify = Arc::new(Notify::new());
        let watcher_notify = Arc::clone(&notify);
        let watcher = tokio::spawn(async move {
            loop {
                let writing = watched.sent().iter().any(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_CONFIG
                });
                if writing {
                    watcher_notify.notify_one();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
        let ctrl_c = async move { notify.notified().await };

        let backup = dir.join("backup");
        let input = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {} --yes\nquit\n",
            backup.display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let started = Instant::now();
        let result = session_repl_loop_with_io(
            &session,
            &state,
            started,
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            ctrl_c,
            false,
        )
        .await;
        let elapsed = started.elapsed();
        watcher.await.unwrap();

        assert!(
            result.is_ok(),
            "the session must exit cleanly after the deferred Ctrl-C: {result:?}"
        );
        let device = transport.config();
        assert_eq!(device.joy(Slot::S2).left_start_value, 42);
        assert_eq!(
            transport
                .sent()
                .iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE
                })
                .count(),
            1,
            "the config write did not reach Save after Ctrl-C"
        );
        assert!(
            elapsed >= Duration::from_millis(20),
            "write finished too fast for the Ctrl-C injection to be meaningful: {elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_prompt_keeps_keepalive_and_toctou_detects_a_change() {
        use tokio::io::AsyncWriteExt as _;

        let dir = scratch_dir("session-config-prompt");
        let initial = config_write_fixture();
        let transport = ConfigDeviceTransport::new(initial.clone()).stalling();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_millis(5))
            .await
            .unwrap();
        wait_for_keepalive_send(&keepalive.state).await;
        let sent_before_command = keepalive.state.sent.load(Ordering::SeqCst);

        let (mut writer, reader) = tokio::io::duplex(1024);
        let mut buf_reader = tokio::io::BufReader::new(reader);
        let repl_session = Arc::clone(&session);
        let repl_state = Arc::clone(&keepalive.state);
        let repl_task = tokio::spawn(async move {
            session_repl_loop_with_io(
                &repl_session,
                &repl_state,
                Instant::now(),
                200,
                Duration::from_millis(20),
                false,
                true,
                &mut buf_reader,
                std::future::pending::<()>(),
                false,
            )
            .await
        });

        let backup = dir.join("backup");
        let command = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {}\n",
            backup.display()
        );
        writer.write_all(command.as_bytes()).await.unwrap();

        // Wait until the pre-write backup exists and the operation lock has
        // been released for the prompt.
        let mut prepared = false;
        for _ in 0..500 {
            if backup.join("config.bin").is_file() {
                prepared = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(prepared, "pre-write backup was not created");
        tokio::time::sleep(Duration::from_millis(10)).await;

        // The keepalive keeps running while the prompt waits.
        let mut progressed = false;
        for _ in 0..200 {
            if keepalive.state.sent.load(Ordering::SeqCst) > sent_before_command {
                progressed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(
            progressed,
            "the keepalive stopped while the confirmation prompt was waiting"
        );

        // Another writer changes the configuration during the prompt.
        let mut changed = initial.clone();
        changed.set_profile_name(Slot::S1, "Changed").unwrap();
        changed.recompute_crc();
        transport.set_config(changed.clone());

        writer.write_all(b"yes\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        writer.write_all(b"quit\n").await.unwrap();
        let result = repl_task.await.unwrap();
        assert!(result.is_ok(), "{result:?}");

        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(!wrote, "TOCTOU change must abort the write");
        assert_eq!(transport.config().as_bytes(), changed.as_bytes());
        shutdown_session(&session, keepalive).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_confirmation_rejects_an_overlong_line() {
        let dir = scratch_dir("session-config-long-line");
        let initial = config_write_fixture();
        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let backup = dir.join("backup");
        let long = "x".repeat(MAX_SESSION_LINE_BYTES + 100);
        let input = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {}\n{long}\nquit\n",
            backup.display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");
        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(!wrote, "an overlong confirmation line must abort");
        assert!(backup.join("config.bin").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_repl_config_prompt_ctrl_c_aborts_without_writing() {
        use tokio::io::AsyncWriteExt as _;

        let dir = scratch_dir("session-config-prompt-cancel");
        let initial = config_write_fixture();
        let transport = ConfigDeviceTransport::new(initial.clone()).stalling();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));

        let notify = Arc::new(Notify::new());
        let ctrl_c = {
            let notify = Arc::clone(&notify);
            async move { notify.notified().await }
        };

        let (mut writer, reader) = tokio::io::duplex(1024);
        let mut buf_reader = tokio::io::BufReader::new(reader);
        let repl_session = Arc::clone(&session);
        let repl_state = Arc::new(SessionKeepaliveState::new());
        let repl_task = tokio::spawn(async move {
            session_repl_loop_with_io(
                &repl_session,
                &repl_state,
                Instant::now(),
                200,
                Duration::from_millis(20),
                false,
                true,
                &mut buf_reader,
                ctrl_c,
                false,
            )
            .await
        });

        let backup = dir.join("backup");
        let command = format!(
            "config set joy.left_start 42 --slot 1 --backup-dir {}\n",
            backup.display()
        );
        writer.write_all(command.as_bytes()).await.unwrap();
        let mut prepared = false;
        for _ in 0..500 {
            if backup.join("config.bin").is_file() {
                prepared = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(prepared, "pre-write backup was not created");
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Ctrl-C while the confirmation prompt is open.
        notify.notify_one();
        let result = repl_task.await.unwrap();
        assert!(result.is_ok(), "{result:?}");

        let wrote = transport.sent().iter().any(|frame| {
            let cmd = CustomHeader::parse(&frame[1..]).unwrap().cmd;
            cmd == commands::cmd::WRITE_CONFIG || cmd == commands::cmd::SAVE
        });
        assert!(!wrote, "prompt Ctrl-C must not send a write or Save");
        assert!(backup.join("config.bin").is_file(), "backup must remain");
        assert_eq!(transport.config().as_bytes(), initial.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn noop_config_write_still_validates_the_chunk_override() {
        let initial = config_write_fixture();
        let snapshot = config_snapshot(initial.clone());
        let prepared = ConfigWritePlan::new().prepare(&snapshot).unwrap();
        assert!(prepared.is_noop());
        let transport = ConfigDeviceTransport::new(initial);
        let session = Session::new(&transport);

        let error = prepare_config_prepared(&session, &snapshot, &prepared, None, Some(181))
            .await
            .unwrap_err();
        assert!(matches!(error, Error::InvalidRequest(_)), "{error:?}");
        // A valid chunk is accepted and reports the no-op.
        let state = prepare_config_prepared(&session, &snapshot, &prepared, None, Some(74))
            .await
            .unwrap();
        assert!(state.is_none());
    }

    #[tokio::test]
    async fn session_repl_config_write_file_executes_the_wired_flow() {
        let dir = scratch_dir("session-config-write-file");
        let initial = config_write_fixture();
        let mut edited = initial.clone();
        edited.set_profile_name(Slot::S2, "ReplFile").unwrap();
        let file = dir.join("edited.bin");
        std::fs::write(&file, edited.as_bytes()).unwrap();

        let transport = ConfigDeviceTransport::new(initial.clone());
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let backup = dir.join("backup");
        let input = format!(
            "config write --file {} --backup-dir {} --yes\nquit\n",
            file.display(),
            backup.display()
        );
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        let result = session_repl_loop_with_io(
            &session,
            &state,
            Instant::now(),
            200,
            Duration::from_millis(20),
            false,
            true,
            &mut reader,
            std::future::pending::<()>(),
            false,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");

        assert_eq!(transport.config().profile_name(Slot::S2), "ReplFile");
        assert!(backup.join("config.bin").is_file());
        assert_eq!(
            transport
                .sent()
                .iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE
                })
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Unique scratch directory for tests that touch the filesystem.
    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eightbitdo-{tag}-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn is_invalid_input(error: &Error) -> bool {
        matches!(error, Error::Io(io) if io.kind() == std::io::ErrorKind::InvalidInput)
    }

    #[test]
    fn validate_output_paths_rejects_alias_spellings_of_one_file() {
        let dir = scratch_dir("alias");
        let plain = dir.join("x.bin");
        let dotted = dir.join(".").join("x.bin");
        let parented = dir.join("sub").join("..").join("x.bin");
        std::fs::create_dir_all(dir.join("sub")).unwrap();

        // Lexically different spellings of a path that does not exist yet.
        let err = validate_output_paths(Some(&plain), Some(&dotted)).unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        let err = validate_output_paths(Some(&plain), Some(&parented)).unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");

        // A relative spelling of the same file must also be rejected.
        let cwd = std::env::current_dir().unwrap();
        if let Ok(relative) = dir.strip_prefix(&cwd) {
            let relative = relative.join("x.bin");
            let err = validate_output_paths(Some(&plain), Some(&relative)).unwrap_err();
            assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        }

        // Distinct names in the same directory are accepted.
        assert!(validate_output_paths(Some(&plain), Some(&dir.join("y.bin"))).is_ok());
        assert!(validate_output_paths(None, Some(&plain)).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn validate_output_paths_rejects_symlink_to_the_other_file() {
        use std::os::unix::fs::symlink;

        let dir = scratch_dir("symlink");
        let target = dir.join("raw.bin");
        std::fs::write(&target, b"old backup").unwrap();
        let link = dir.join("link.json");
        symlink(&target, &link).unwrap();

        let err = validate_output_paths(Some(&target), Some(&link)).unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        let err = validate_output_paths(Some(&link), Some(&target)).unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");

        // Dangling symlink pointing at a file that does not exist yet.
        let missing = dir.join("created.bin");
        let dangling = dir.join("dangling.json");
        symlink(&missing, &dangling).unwrap();
        let err = validate_output_paths(Some(&missing), Some(&dangling));
        assert!(
            matches!(&err, Err(error) if is_invalid_input(error)),
            "a dangling symlink to the other path must be rejected: {err:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn validate_output_paths_rejects_hard_link_to_the_other_file() {
        let dir = scratch_dir("hardlink");
        let a = dir.join("a.bin");
        std::fs::write(&a, b"old backup").unwrap();
        let b = dir.join("b.json");
        std::fs::hard_link(&a, &b).unwrap();

        let err = validate_output_paths(Some(&a), Some(&b)).unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_replaces_the_file_and_leaves_no_temp_files() {
        let dir = scratch_dir("atomic");
        let path = dir.join("config.bin");

        write_atomic(&path, b"first").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_preserves_modes_and_creates_private_files() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch_dir("atomic-mode");
        let path = dir.join("backup.bin");
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;

        write_atomic(&path, b"first").unwrap();
        assert_eq!(
            mode(&path),
            0o600,
            "a new backup must not be group/world readable"
        );

        write_atomic(&path, b"second").unwrap();
        assert_eq!(mode(&path), 0o600, "rewriting a 0600 backup must keep 0600");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&path, b"third").unwrap();
        assert_eq!(mode(&path), 0o640, "an existing mode must be preserved");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_refuses_symlinks_and_directories() {
        let dir = scratch_dir("atomic-refuse");
        let target = dir.join("target.bin");
        std::fs::write(&target, b"keep").unwrap();
        let link = dir.join("link.bin");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = write_atomic(&link, b"replaced").unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"keep",
            "the symlink target must not be overwritten"
        );
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink itself must not be replaced by a regular file"
        );

        let subdir = dir.join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let err = write_atomic(&subdir, b"x").unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        assert!(subdir.is_dir(), "the directory must be left in place");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_refuses_sockets() {
        use std::os::unix::fs::FileTypeExt as _;
        use std::os::unix::net::UnixListener;

        let dir = scratch_dir("atomic-socket");
        let socket = dir.join("out.sock");
        let _listener = UnixListener::bind(&socket).unwrap();

        let err = write_atomic(&socket, b"x").unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        assert!(
            std::fs::symlink_metadata(&socket)
                .unwrap()
                .file_type()
                .is_socket(),
            "the socket must not be replaced by a regular file"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn write_atomic_refuses_character_devices() {
        use std::os::unix::fs::FileTypeExt as _;

        let null = Path::new("/dev/null");
        if !null.exists() {
            return;
        }
        let err = write_atomic(null, b"x").unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        assert!(
            std::fs::metadata(null)
                .unwrap()
                .file_type()
                .is_char_device(),
            "/dev/null must stay a character device"
        );
    }

    #[test]
    fn write_config_outputs_writes_both_files_atomically() {
        let dir = scratch_dir("outputs");
        let raw = dir.join("config.raw");
        let json = dir.join("config.json");
        let snapshot = ConfigSnapshot {
            config: Bt2Config::zeroed(),
            stored_crc: 0,
            computed_crc: 0,
            crc_valid: true,
        };

        let digest = write_config_outputs(&snapshot, Some(&raw), Some(&json)).unwrap();
        assert_eq!(digest, sha256_hex(snapshot.config.as_bytes()));
        let raw_bytes = std::fs::read(&raw).unwrap();
        assert_eq!(raw_bytes.len(), CONFIG_SIZE);
        assert_eq!(raw_bytes, snapshot.config.as_bytes());
        let text = std::fs::read_to_string(&json).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["sha256"], digest);
        assert_eq!(value["size"], CONFIG_SIZE);
        assert!(text.ends_with('\n'));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn raw_write_is_revalidated_before_the_json_file() {
        // B-2 regression: on a case-insensitive filesystem `Backup.bin` and
        // `backup.bin` look distinct before either exists, then resolve to the
        // same entry after the raw write. The second check runs while the raw
        // file exists and must reject the pair. Linux cannot make a
        // case-insensitive alias, so the seam creates a hard link at the JSON
        // path between the write and the recheck; hard links need Unix inode
        // identity to be detected (`same_file`).
        let dir = scratch_dir("outputs-recheck");
        let raw = dir.join("backup.bin");
        let json = dir.join("backup.json");

        let err = write_raw_and_revalidate_with(&raw, Some(&json), b"raw backup", || {
            std::fs::hard_link(&raw, &json).unwrap();
        })
        .unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        assert_eq!(std::fs::read(&raw).unwrap(), b"raw backup");
        // The JSON path was never given a JSON summary; it is still the raw
        // bytes via the alias, so the backup was not replaced.
        assert_eq!(std::fs::read(&json).unwrap(), b"raw backup");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn raw_write_revalidation_rejects_a_case_insensitive_alias() {
        // B-2 regression for the platform that motivated the recheck. Before
        // the raw file exists the pair looks distinct; after the write,
        // `OutputPathKey::real` canonicalizes both spellings to the same entry.
        // On Windows `same_file` is always false, so this test exercises the
        // canonical-path/metadata half of the check.
        let dir = scratch_dir("outputs-case");
        let raw = dir.join("Backup.bin");
        let json = dir.join("backup.BIN");

        // APFS can be formatted case-sensitive and Windows can mark a
        // directory case-sensitive, so only run the behavioural half where the
        // filesystem actually folds case.
        let probe = dir.join("CaseProbe");
        std::fs::write(&probe, b"probe").unwrap();
        if !dir.join("caseprobe").exists() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        assert!(
            validate_output_paths(Some(&raw), Some(&json)).is_ok(),
            "the pre-write check cannot see the alias before the raw file exists"
        );

        let err = write_raw_and_revalidate(&raw, Some(&json), b"raw backup").unwrap_err();
        assert!(is_invalid_input(&err), "unexpected error: {err:?}");
        assert_eq!(std::fs::read(&raw).unwrap(), b"raw backup");
        assert_eq!(
            std::fs::read(&json).unwrap(),
            b"raw backup",
            "the JSON path still reaches the raw bytes; the backup was not replaced"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn raw_write_revalidation_allows_unrelated_paths() {
        let dir = scratch_dir("outputs-recheck-ok");
        let raw = dir.join("backup.bin");
        let json = dir.join("backup.json");
        write_raw_and_revalidate(&raw, Some(&json), b"raw backup").unwrap();
        assert_eq!(std::fs::read(&raw).unwrap(), b"raw backup");
        assert!(!json.exists(), "revalidation must not create the JSON file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn repl_reader_keeps_partial_line_across_cancellation() {
        use tokio::io::AsyncWriteExt as _;

        let (mut writer, reader) = tokio::io::duplex(64);
        let mut reader = tokio::io::BufReader::new(reader);
        let mut state = LineState::default();

        writer.write_all(b"inf").await.unwrap();
        let cancelled = tokio::time::timeout(
            Duration::from_millis(20),
            read_session_line(&mut reader, &mut state),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "no newline yet, so the read must still be pending"
        );
        assert_eq!(
            state.pending, b"inf",
            "consumed bytes must survive the cancellation"
        );

        writer.write_all(b"o\n").await.unwrap();
        let read = tokio::time::timeout(
            Duration::from_millis(500),
            read_session_line(&mut reader, &mut state),
        )
        .await
        .expect("the rest of the line arrives")
        .unwrap();
        assert_eq!(
            read,
            LineRead::Line {
                text: "info".to_string(),
                cut: false
            },
            "the line must be reassembled, not lost or split"
        );
        assert!(state.pending.is_empty());
    }

    #[tokio::test]
    async fn repl_reader_keeps_a_trailing_cr_before_more_content() {
        use tokio::io::AsyncWriteExt as _;

        let (mut writer, reader) = tokio::io::duplex(64);
        let mut reader = tokio::io::BufReader::new(reader);
        let mut state = LineState::default();

        writer.write_all(b"ab\r").await.unwrap();
        let cancelled = tokio::time::timeout(
            Duration::from_millis(20),
            read_session_line(&mut reader, &mut state),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "no newline yet, so the read must still be pending"
        );
        assert_eq!(
            state.pending, b"ab\r",
            "a trailing CR is content when the next chunk continues the line"
        );

        writer.write_all(b"cd\n").await.unwrap();
        let read = tokio::time::timeout(
            Duration::from_millis(500),
            read_session_line(&mut reader, &mut state),
        )
        .await
        .expect("the rest of the line arrives")
        .unwrap();
        assert_eq!(
            read,
            LineRead::Line {
                text: "ab\rcd".to_string(),
                cut: false
            },
            "the CR must survive when the newline is not in the same buffer"
        );
    }

    #[tokio::test]
    async fn repl_reader_keeps_a_trailing_cr_before_command_options() {
        // Blocker regression: `backup\r` followed by `--raw x\n` must stay
        // `backup\r--raw x` (CR is whitespace for the parser), not collapse
        // into the unknown command `backup--raw x`.
        use tokio::io::AsyncWriteExt as _;

        let (mut writer, reader) = tokio::io::duplex(64);
        let mut reader = tokio::io::BufReader::new(reader);
        let mut state = LineState::default();

        writer.write_all(b"backup\r").await.unwrap();
        let cancelled = tokio::time::timeout(
            Duration::from_millis(20),
            read_session_line(&mut reader, &mut state),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "no newline yet, so the read must still be pending"
        );

        writer.write_all(b"--raw x\n").await.unwrap();
        let read = tokio::time::timeout(
            Duration::from_millis(500),
            read_session_line(&mut reader, &mut state),
        )
        .await
        .expect("the rest of the line arrives")
        .unwrap();
        let text = match read {
            LineRead::Line { text, cut: false } => text,
            other => panic!("expected a complete line, got {other:?}"),
        };
        assert_eq!(text, "backup\r--raw x");
        assert_eq!(
            parse_session_command(&text),
            SessionCommand::Backup {
                raw: Some(PathBuf::from("x")),
                json: None,
            },
            "the preserved CR must keep the options as separate words"
        );
    }

    #[tokio::test]
    async fn repl_reader_reports_a_cancelled_overlong_line_as_too_long() {
        // Major regression: `overflow` used to live inside the read future.
        // Dropping it (Ctrl-C / select!) forgot that the 4096-byte prefix was
        // already too long, so the prefix could run as a command.
        use tokio::io::AsyncWriteExt as _;

        let extra = 904;
        let (mut writer, reader) = tokio::io::duplex(8192);
        let mut reader = tokio::io::BufReader::new(reader);
        let mut state = LineState::default();

        writer
            .write_all(&vec![b'a'; MAX_SESSION_LINE_BYTES + extra])
            .await
            .unwrap();
        let cancelled = tokio::time::timeout(
            Duration::from_millis(20),
            read_session_line(&mut reader, &mut state),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "the line is not terminated, so the read must still be pending"
        );
        assert_eq!(state.pending.len(), MAX_SESSION_LINE_BYTES);
        assert_eq!(
            state.overflow, extra,
            "the over-limit count must survive the dropped future"
        );

        writer.write_all(b"\n").await.unwrap();
        let read = tokio::time::timeout(
            Duration::from_millis(500),
            read_session_line(&mut reader, &mut state),
        )
        .await
        .expect("the newline arrives")
        .unwrap();
        assert_eq!(
            read,
            LineRead::TooLong { discarded: extra },
            "the truncated prefix must not be returned as a runnable line"
        );
        assert!(state.pending.is_empty());
        assert_eq!(state.overflow, 0);
    }

    #[tokio::test]
    async fn repl_reader_strips_a_crlf_split_across_buffers() {
        // A one-byte buffer forces the CR and LF into separate `fill_buf`
        // calls, so the CR is already in `pending` when the `\n` arrives. The
        // `end == 0` correction must still treat the pair as one ending.
        let mut reader =
            tokio::io::BufReader::with_capacity(1, std::io::Cursor::new(b"ab\r\nnext\n".to_vec()));
        let mut state = LineState::default();

        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::Line {
                text: "ab".to_string(),
                cut: false
            }
        );
        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::Line {
                text: "next".to_string(),
                cut: false
            }
        );
    }

    #[tokio::test]
    async fn repl_reader_handles_crlf_and_eof() {
        let mut reader =
            tokio::io::BufReader::new(std::io::Cursor::new(b"info\r\nbogus\n".to_vec()));
        let mut state = LineState::default();

        for expected in ["info", "bogus"] {
            let read = read_session_line(&mut reader, &mut state).await.unwrap();
            assert_eq!(
                read,
                LineRead::Line {
                    text: expected.to_string(),
                    cut: false
                }
            );
        }
        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::Eof
        );
    }

    #[tokio::test]
    async fn repl_reader_treats_empty_lines_as_empty_commands() {
        // B-3 regression: a consumed newline with no bytes before it used to
        // come back as `Eof`, so pressing Enter at the prompt ended the
        // session.
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(b"\n\r\ninfo\n".to_vec()));
        let mut state = LineState::default();

        for expected in ["", "", "info"] {
            assert_eq!(
                read_session_line(&mut reader, &mut state).await.unwrap(),
                LineRead::Line {
                    text: expected.to_string(),
                    cut: false
                },
                "an empty line must stay a line, not become EOF"
            );
        }
        assert!(state.pending.is_empty());
        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::Eof
        );
    }

    #[tokio::test]
    async fn repl_reader_treats_a_lone_nul_as_an_empty_cut_line() {
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(b"\0info\n".to_vec()));
        let mut state = LineState::default();

        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::Line {
                text: String::new(),
                cut: true
            }
        );
        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::Line {
                text: "info".to_string(),
                cut: false
            }
        );
    }

    #[tokio::test]
    async fn repl_reader_keeps_reading_after_an_empty_line_from_a_pipe() {
        use tokio::io::AsyncWriteExt as _;

        let (mut writer, reader) = tokio::io::duplex(64);
        let mut reader = tokio::io::BufReader::new(reader);
        let mut state = LineState::default();

        writer.write_all(b"\n").await.unwrap();
        let empty = tokio::time::timeout(
            Duration::from_millis(500),
            read_session_line(&mut reader, &mut state),
        )
        .await
        .expect("the empty line arrives")
        .unwrap();
        assert_eq!(
            empty,
            LineRead::Line {
                text: String::new(),
                cut: false
            }
        );

        writer.write_all(b"info\n").await.unwrap();
        let info = tokio::time::timeout(
            Duration::from_millis(500),
            read_session_line(&mut reader, &mut state),
        )
        .await
        .expect("the next line arrives after the empty one")
        .unwrap();
        assert_eq!(
            info,
            LineRead::Line {
                text: "info".to_string(),
                cut: false
            }
        );
    }

    #[tokio::test]
    async fn repl_reader_limits_line_length_and_resyncs() {
        let mut input = vec![b'a'; MAX_SESSION_LINE_BYTES + 10];
        input.extend_from_slice(b"\ninfo\n");
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input));
        let mut state = LineState::default();

        let long = read_session_line(&mut reader, &mut state).await.unwrap();
        assert_eq!(long, LineRead::TooLong { discarded: 10 });
        assert!(state.pending.is_empty());
        let next = read_session_line(&mut reader, &mut state).await.unwrap();
        assert_eq!(
            next,
            LineRead::Line {
                text: "info".to_string(),
                cut: false
            },
            "the next prompt must see a clean line after an over-long one"
        );
    }

    #[tokio::test]
    async fn repl_reader_does_not_count_crlf_toward_the_line_limit() {
        // M-1 regression: a line of exactly MAX_SESSION_LINE_BYTES characters
        // followed by CRLF is not too long; the `\r` belongs to the ending.
        // With the default and 4096-byte capacities the `\r` and `\n` arrive
        // together, so `content_end` drops the CR before it is counted. With
        // 4097 bytes the first chunk ends on the CR (no terminator yet), and
        // the next chunk's `\n` takes the `end == 0` correction for the only
        // overflow byte.
        for capacity in [8 * 1024, MAX_SESSION_LINE_BYTES, MAX_SESSION_LINE_BYTES + 1] {
            let mut input = vec![b'a'; MAX_SESSION_LINE_BYTES];
            input.extend_from_slice(b"\r\n");
            let mut reader =
                tokio::io::BufReader::with_capacity(capacity, std::io::Cursor::new(input));
            let mut state = LineState::default();

            let read = read_session_line(&mut reader, &mut state).await.unwrap();
            assert_eq!(
                read,
                LineRead::Line {
                    text: "a".repeat(MAX_SESSION_LINE_BYTES),
                    cut: false
                },
                "CRLF must not count against the limit (buffer capacity {capacity})"
            );
            assert!(state.pending.is_empty());
        }
    }

    #[tokio::test]
    async fn repl_reader_still_rejects_one_byte_over_the_limit_with_crlf() {
        let mut input = vec![b'a'; MAX_SESSION_LINE_BYTES + 1];
        input.extend_from_slice(b"\r\n");
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input));
        let mut state = LineState::default();

        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::TooLong { discarded: 1 },
            "one content byte over the limit must still be reported, without the CR"
        );
        assert!(state.pending.is_empty());
    }

    #[tokio::test]
    async fn repl_reader_cuts_nul_separated_paste_without_losing_lines() {
        let mut input = vec![b'x'; MAX_SESSION_LINE_BYTES + 5];
        input.push(0);
        input.extend_from_slice(b"info\n");
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(input));
        let mut state = LineState::default();

        let cut = read_session_line(&mut reader, &mut state).await.unwrap();
        assert_eq!(cut, LineRead::TooLong { discarded: 5 });
        let next = read_session_line(&mut reader, &mut state).await.unwrap();
        assert_eq!(
            next,
            LineRead::Line {
                text: "info".to_string(),
                cut: false
            }
        );
    }

    #[tokio::test]
    async fn repl_reader_cuts_at_nul_below_the_limit() {
        let mut reader = tokio::io::BufReader::new(std::io::Cursor::new(b"fi\0ve\n".to_vec()));
        let mut state = LineState::default();
        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::Line {
                text: "fi".to_string(),
                cut: true
            }
        );
        assert_eq!(
            read_session_line(&mut reader, &mut state).await.unwrap(),
            LineRead::Line {
                text: "ve".to_string(),
                cut: false
            }
        );
    }

    /// Transport whose notifications never stop.
    #[derive(Clone)]
    struct FloodTransport {
        recv_calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl Transport for FloodTransport {
        async fn send(&self, _data: &[u8]) -> Result<(), Error> {
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            self.recv_calls.fetch_add(1, Ordering::SeqCst);
            let payload = sample_input_report().to_bytes();
            let header = CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::INPUT, 0x0C, 0)
                .with_payload(&payload);
            let mut frame = header.to_bytes().to_vec();
            frame.extend_from_slice(&payload);
            Ok(frame)
        }

        fn mtu(&self) -> u16 {
            64
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn session_keepalive_tick_is_finite_under_endless_notifications() {
        let transport = FloodTransport {
            recv_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let keepalive = start_session_keepalive(Arc::clone(&session), Duration::from_millis(1))
            .await
            .unwrap();

        // With an endless stream the keepalive must keep making progress:
        // each tick drains at most SESSION_KEEPALIVE_MAX_DRAIN reports and
        // releases the operation lock in between.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while keepalive.state.sent.load(Ordering::SeqCst) < 3 {
            assert!(
                std::time::Instant::now() < deadline,
                "keepalive could not complete three ticks while flooded"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let sent = keepalive.state.sent.load(Ordering::SeqCst);
        let received = keepalive.state.received.load(Ordering::SeqCst);
        assert!(
            received <= sent * SESSION_KEEPALIVE_MAX_DRAIN,
            "a tick drained more than the per-tick cap: sent={sent} received={received}"
        );

        // The lock is released between ticks, so a REPL command can run.
        let guard = tokio::time::timeout(Duration::from_secs(1), session.lock())
            .await
            .expect("operation lock must be releasable while the input stream floods");
        drop(guard);

        shutdown_session(&session, keepalive).await;
    }

    #[tokio::test]
    async fn status_reports_paused_keepalive_while_the_monitor_runs() {
        // M5: with a background monitor holding the operation lock the
        // keepalive only waits for it, so `status` must say so instead of
        // implying it is making progress.
        let report = status_report(
            false,
            64,
            10,
            5,
            None,
            Duration::from_secs(1),
            Duration::from_secs(2),
            true,
            20,
            20,
        );
        assert!(
            report.contains("keepalive: paused (monitor running)"),
            "status must mark the keepalive as paused:\n{report}"
        );
        assert!(report.contains("monitor: running"), "{report}");
        assert!(!report.contains("keepalive: running"), "{report}");

        // Without a monitor the keepalive is reported as running.
        let idle = status_report(
            false,
            64,
            10,
            5,
            None,
            Duration::from_secs(1),
            Duration::from_secs(2),
            false,
            0,
            0,
        );
        assert!(idle.contains("keepalive: running"), "{idle}");
        assert!(idle.contains("monitor: idle"), "{idle}");

        // A degraded keepalive is reported as degraded, not as paused.
        let degraded = status_report(
            true,
            64,
            10,
            5,
            Some("boom"),
            Duration::from_secs(1),
            Duration::from_secs(2),
            false,
            0,
            0,
        );
        assert!(degraded.contains("keepalive: degraded"), "{degraded}");
        assert!(
            degraded.contains("keepalive last_error: boom"),
            "{degraded}"
        );
    }

    #[tokio::test]
    async fn status_command_accepts_a_running_background_monitor() {
        // The `status` command itself must return a report (and an observable
        // paused state) while the background monitor holds the lock.
        let transport = KeepaliveTestTransport::new();
        for _ in 0..10 {
            transport.queue_envelope_input(&sample_input_report());
        }
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let started = Instant::now();
        let mut monitor: Option<SessionMonitor> = None;

        let outcome = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(20),
            false,
            &mut monitor,
            SessionCommand::Monitor { count: None },
        )
        .await
        .unwrap();
        assert_eq!(outcome, SessionOutcome::Continue);
        assert!(monitor.is_some());

        let outcome = execute_session_command(
            &session,
            &state,
            started,
            64,
            Duration::from_millis(20),
            false,
            &mut monitor,
            SessionCommand::Status,
        )
        .await
        .unwrap();
        assert_eq!(outcome, SessionOutcome::Continue);

        // The `Status` branch above builds exactly this report with
        // `status_report` and prints it. Verifying the pure function covers
        // the printed text without having to capture stdout.
        let (monitor_sent, monitor_received) = monitor_counters(monitor.as_ref());
        let report = status_report(
            state.degraded.load(Ordering::SeqCst),
            64,
            state.sent.load(Ordering::SeqCst),
            state.received.load(Ordering::SeqCst),
            state.last_error().as_deref(),
            state.started_at.elapsed(),
            started.elapsed(),
            monitor.is_some(),
            monitor_sent,
            monitor_received,
        );
        assert!(
            report.contains("keepalive: paused (monitor running)"),
            "status must report the starved keepalive while the monitor runs:\n{report}"
        );
        assert!(report.contains("monitor: running"), "{report}");
        assert!(
            monitor.is_some(),
            "the status command must not stop the background monitor"
        );

        let stopped = monitor.take().unwrap();
        stopped.stop().await;
    }

    #[tokio::test]
    async fn dropping_session_monitor_releases_the_operation_lock() {
        let transport = KeepaliveTestTransport::new();
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let monitor = start_background_monitor(Arc::clone(&session), Duration::from_millis(1));
        for _ in 0..100 {
            if monitor.stats.sent() > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(session.try_lock().is_err(), "monitor must hold the lock");
        drop(monitor);

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while session.try_lock().is_err() {
            assert!(
                std::time::Instant::now() < deadline,
                "dropping the monitor must release the operation lock"
            );
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn ctrl_c_during_monitor_count_leaves_the_next_read_usable() {
        // B3 regression: a `monitor --count N` interrupted by Ctrl-C drops the
        // input poll. The leftover realtime report must not make the session
        // look dirty, and the next `read_version` must succeed without
        // consuming a stale frame as its own response.
        let transport = KeepaliveTestTransport::new();
        // Plenty of reports so the poll is guaranteed to be in flight.
        for _ in 0..8 {
            transport.queue_envelope_input(&sample_input_report());
        }
        let session = Arc::new(AsyncMutex::new(Session::new(transport.clone())));
        let state = Arc::new(SessionKeepaliveState::new());
        let mut monitor: Option<SessionMonitor> = None;

        let cancelled = tokio::time::timeout(
            Duration::from_millis(5),
            execute_session_command(
                &session,
                &state,
                Instant::now(),
                64,
                Duration::from_millis(20),
                false,
                &mut monitor,
                SessionCommand::Monitor { count: Some(1000) },
            ),
        )
        .await;
        assert!(cancelled.is_err(), "the monitor must be cancelled mid-run");

        // A read that arrives after the cancellation is answered normally.
        queue_version_response(&transport);
        let version = execute_session_command(
            &session,
            &state,
            Instant::now(),
            64,
            Duration::from_millis(20),
            false,
            &mut monitor,
            SessionCommand::Info,
        )
        .await;
        assert!(
            version.is_ok(),
            "read_version after an abandoned input poll must succeed: {version:?}"
        );
    }

    #[test]
    fn macro_backup_writes_manifest_and_skips_unset_steps() {
        use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord};
        use eightbitdo_protocol::macro_data::{self, MacroStep};
        use eightbitdo_protocol::KeyMap;

        let dir = scratch_dir("macro-backup");
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);

        let mut record = MacroRecord {
            max_steps: 2,
            key_map: KeyMap::M1,
            cycles_num: 3,
            interval_ms: 20,
            ..MacroRecord::default()
        };
        record.set_name("Combo").unwrap();
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);

        let mut unset = MacroRecord {
            max_steps: u16::MAX,
            ..MacroRecord::default()
        };
        unset.set_name("Unset").unwrap();
        config.set_macro_record(Slot::S2, MacroIndex::M3, &unset);
        config.recompute_crc();

        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        let snapshot = ConfigSnapshot {
            config: config.clone(),
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        };

        let steps = vec![MacroStep::neutral(10), MacroStep::neutral(20)];
        let raw = macro_data::encode_macro_steps(&steps).unwrap()[..20].to_vec();
        let mut entries = Vec::new();
        for slot in Slot::ALL {
            for index in MacroIndex::ALL {
                let entry_record = config.macro_record(slot, index);
                let entry_raw = if slot == Slot::S1 && index == MacroIndex::M1 {
                    raw.clone()
                } else {
                    Vec::new()
                };
                entries.push(MacroEntry::new(slot, index, entry_record, entry_raw).unwrap());
            }
        }
        let backup = MacroBackup::new(snapshot.clone(), entries).unwrap();

        write_macro_backup(&dir, &snapshot, &backup, "config-sha").unwrap();

        assert_eq!(
            std::fs::read(dir.join("config.bin")).unwrap().len(),
            CONFIG_SIZE
        );
        assert_eq!(std::fs::read(dir.join("slot0-macro0.bin")).unwrap(), raw);
        assert!(!dir.join("slot1-macro2.bin").exists());

        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("macros.json")).unwrap())
                .unwrap();
        assert_eq!(json["macros"].as_array().unwrap().len(), 12);
        let read = &json["macros"][0];
        assert_eq!(read["slot"], 0);
        assert_eq!(read["index"], 0);
        assert_eq!(read["id"], "M1");
        assert_eq!(read["max_steps"], 2);
        assert_eq!(read["raw_file"], "slot0-macro0.bin");
        assert_eq!(read["steps"].as_array().unwrap().len(), 2);
        assert_eq!(
            json["macros"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["skipped"] == true)
                .count(),
            11
        );

        let toml: toml::Value =
            toml::from_str(&std::fs::read_to_string(dir.join("macros.toml")).unwrap()).unwrap();
        assert_eq!(toml["macros"].as_array().unwrap().len(), 12);
        assert_eq!(toml["config_sha256"].as_str(), Some("config-sha"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn macro_backup_writes_nothing_when_a_read_fails_mid_transfer() {
        use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord};

        let root = scratch_dir("macro-backup-abort");
        let dir = root.join("out");

        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        let record = MacroRecord {
            max_steps: 50,
            ..MacroRecord::default()
        };
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);
        config.recompute_crc();

        let mock = MockTransport::new();
        // A full 2768-byte config read, one 142-byte chunk per request.
        for (index, chunk) in config.as_bytes().chunks(142).enumerate() {
            let header = CustomHeader::new(
                commands::cmd::READ_CONFIG,
                0,
                CONFIG_SIZE as u32,
                (index * 142) as u32,
            )
            .with_chunk_len(chunk.len() as u16);
            mock.push_response(&header, chunk);
        }
        // Only the first 142 bytes of the 500-byte macro. The second request
        // finds an empty queue, so the read fails before any file is written.
        let first = [0u8; 142];
        let header = CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::READ_MACRO, 500, 0)
            .with_chunk_len(first.len() as u16);
        mock.push_response(&header, &first);

        let session = Session::new(&mock);
        let result = macro_backup_session(&session, &dir).await;

        assert!(matches!(result, Err(Error::Disconnected)));
        assert!(
            !dir.exists(),
            "a failed macro read must remove the newly created empty backup directory"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn parse_trigger_accepts_macro_ids_names_and_numbers() {
        assert_eq!(parse_trigger("M1").unwrap(), KeyMap::M1);
        assert_eq!(parse_trigger("m4").unwrap(), KeyMap::M4);
        assert_eq!(parse_trigger("P1").unwrap(), KeyMap::P1);
        assert_eq!(parse_trigger(" 0x77 ").unwrap(), KeyMap::M1);
        assert_eq!(parse_trigger("119").unwrap(), KeyMap::M1);
        assert!(parse_trigger("not-a-key").is_err());
    }

    #[test]
    fn read_macro_file_accepts_json_and_toml() {
        let dir = scratch_dir("macro-file");
        let file = MacroFile {
            name: Some("Combo".to_string()),
            gamepad_mode: 0,
            key_map: KeyMap::M1,
            cycles_num: Some(2),
            interval_ms: Some(15),
            steps: vec![MacroStep::neutral(10), MacroStep::neutral(20)],
        };
        let json = dir.join("macro.json");
        let toml = dir.join("macro.toml");
        std::fs::write(&json, serde_json::to_string_pretty(&file).unwrap()).unwrap();
        std::fs::write(&toml, toml::to_string(&file).unwrap()).unwrap();

        assert_eq!(read_macro_file(&json).unwrap(), file);
        assert_eq!(read_macro_file(&toml).unwrap(), file);

        std::fs::write(dir.join("empty.json"), r#"{"steps":[]}"#).unwrap();
        assert!(read_macro_file(&dir.join("empty.json")).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Returns Disconnected on the next READ_MACRO and suppresses recv until
    /// a non-READ_MACRO request is sent, so stale-response drains quiesce
    /// without consuming preloaded post-failure frames.
    struct FailMacroReadTransport {
        inner: MockTransport,
        fail_macro: std::sync::atomic::AtomicBool,
        suppress: std::sync::atomic::AtomicBool,
    }

    impl FailMacroReadTransport {
        fn new() -> Self {
            Self {
                inner: MockTransport::with_mtu(200),
                fail_macro: std::sync::atomic::AtomicBool::new(false),
                suppress: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl Transport for FailMacroReadTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            let command = CustomHeader::parse(&data[1..]).unwrap().cmd;
            if command == commands::cmd::READ_MACRO {
                self.fail_macro.store(true, Ordering::SeqCst);
            } else {
                self.suppress.store(false, Ordering::SeqCst);
            }
            self.inner.send(data).await
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            if self.fail_macro.swap(false, Ordering::SeqCst) {
                self.suppress.store(true, Ordering::SeqCst);
                return Err(Error::Disconnected);
            }
            if self.suppress.load(Ordering::SeqCst) {
                return Err(Error::Disconnected);
            }
            self.inner.recv().await
        }

        fn mtu(&self) -> u16 {
            200
        }

        async fn disconnect(&self) -> Result<(), Error> {
            self.inner.disconnect().await
        }
    }

    fn push_config_responses(mock: &MockTransport, config: &Bt2Config) {
        for (index, chunk) in config.as_bytes().chunks(142).enumerate() {
            let header = CustomHeader::new(
                commands::cmd::READ_CONFIG,
                0,
                CONFIG_SIZE as u32,
                (index * 142) as u32,
            )
            .with_chunk_len(chunk.len() as u16);
            mock.push_response(&header, chunk);
        }
    }

    fn push_macro_read(mock: &MockTransport, config: &Bt2Config, slot: Slot, index: MacroIndex) {
        use eightbitdo_protocol::macro_data;

        let record = config.macro_record(slot, index);
        if is_skipped_steps(record.max_steps) {
            return;
        }
        let steps = vec![MacroStep::neutral(10); usize::from(record.max_steps)];
        let raw = macro_data::encode_macro_steps(&steps).unwrap()
            [..usize::from(record.max_steps) * 10]
            .to_vec();
        let mut header = CustomHeader::new(
            commands::cmd::RESPONSE,
            commands::cmd::READ_MACRO,
            raw.len() as u32,
            0,
        )
        .with_chunk_len(raw.len() as u16);
        header.crc16 = eightbitdo_protocol::crc::packet_crc16(&raw);
        mock.push_response(&header, &raw);
    }

    fn push_macro_raw(mock: &MockTransport, raw: &[u8]) {
        let mut offset = 0u32;
        for chunk in raw.chunks(142) {
            let mut header = CustomHeader::new(
                commands::cmd::RESPONSE,
                commands::cmd::READ_MACRO,
                raw.len() as u32,
                offset,
            )
            .with_chunk_len(chunk.len() as u16);
            header.crc16 = eightbitdo_protocol::crc::packet_crc16(chunk);
            mock.push_response(&header, chunk);
            offset += chunk.len() as u32;
        }
    }

    fn push_backup_macro_read(
        mock: &MockTransport,
        backup: &MacroBackup,
        slot: Slot,
        index: MacroIndex,
    ) {
        let entry = backup.get(slot, index).unwrap();
        if entry.is_empty() {
            return;
        }
        let raw = entry.raw();
        let mut offset = 0u32;
        for chunk in raw.chunks(142) {
            let mut header = CustomHeader::new(
                commands::cmd::RESPONSE,
                commands::cmd::READ_MACRO,
                raw.len() as u32,
                offset,
            )
            .with_chunk_len(chunk.len() as u16);
            header.crc16 = eightbitdo_protocol::crc::packet_crc16(chunk);
            mock.push_response(&header, chunk);
            offset += chunk.len() as u32;
        }
    }

    fn is_write_command(frame: &[u8]) -> bool {
        let command = CustomHeader::parse(&frame[1..]).unwrap().cmd;
        matches!(
            command,
            commands::cmd::ERASE_MACRO
                | commands::cmd::WRITE_MACRO
                | commands::cmd::WRITE_CONFIG
                | commands::cmd::SAVE
        )
    }

    fn empty_backup_fixture(dir: &Path) -> (ConfigSnapshot, MacroBackup) {
        use eightbitdo_protocol::config_bt2::GamepadMode;

        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        config.recompute_crc();
        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        let snapshot = ConfigSnapshot {
            config: config.clone(),
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        };
        let mut entries = Vec::new();
        for slot in Slot::ALL {
            for index in MacroIndex::ALL {
                entries.push(
                    MacroEntry::new(slot, index, config.macro_record(slot, index), Vec::new())
                        .unwrap(),
                );
            }
        }
        let backup = MacroBackup::new(snapshot.clone(), entries).unwrap();
        write_macro_backup(
            dir,
            &snapshot,
            &backup,
            &sha256_hex(snapshot.config.as_bytes()),
        )
        .unwrap();
        (snapshot, backup)
    }

    #[test]
    fn macro_file_omits_and_defaults_unset_loop_fields() {
        let file = MacroFile {
            name: None,
            gamepad_mode: 0,
            key_map: KeyMap::NONE,
            cycles_num: None,
            interval_ms: None,
            steps: vec![MacroStep::neutral(10)],
        };
        let json = serde_json::to_value(&file).unwrap();
        assert!(json.get("cycles_num").is_none());
        assert!(json.get("interval_ms").is_none());
        let toml = toml::to_string(&file).unwrap();
        assert!(!toml.contains("cycles_num"));
        assert!(!toml.contains("interval_ms"));

        let parsed: MacroFile = serde_json::from_value(serde_json::json!({
            "steps": [{
                "ms_times": 10,
                "keys": 0,
                "rt_value": 0,
                "lt_value": 0,
                "left_x": 127,
                "left_y": 127,
                "right_x": 127,
                "right_y": 127
            }]
        }))
        .unwrap();
        assert_eq!(parsed.cycles_num, None);
        assert_eq!(parsed.interval_ms, None);
    }

    fn push_ack(mock: &MockTransport, command: u16) {
        mock.push_response(&CustomHeader::new(command, 0, 0, 0), &[]);
    }

    fn backup_fixture_with_macros(
        dir: &Path,
        macros: &[(MacroIndex, &str, Vec<MacroStep>)],
    ) -> (ConfigSnapshot, MacroBackup) {
        use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord};
        use eightbitdo_protocol::macro_data;

        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        for (index, name, steps) in macros {
            let mut record = MacroRecord {
                max_steps: steps.len() as u16,
                ..MacroRecord::default()
            };
            record.set_name(name).unwrap();
            config.set_macro_record(Slot::S1, *index, &record);
        }
        config.recompute_crc();

        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        let snapshot = ConfigSnapshot {
            config: config.clone(),
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        };

        let mut entries = Vec::new();
        for slot in Slot::ALL {
            for index in MacroIndex::ALL {
                let record = config.macro_record(slot, index);
                let raw = macros
                    .iter()
                    .find(|(macro_index, _, _)| slot == Slot::S1 && *macro_index == index)
                    .map(|(_, _, steps)| {
                        macro_data::encode_macro_steps(steps).unwrap()
                            [..steps.len() * macro_data::MACRO_STEP_SIZE]
                            .to_vec()
                    })
                    .unwrap_or_default();
                entries.push(MacroEntry::new(slot, index, record, raw).unwrap());
            }
        }
        let backup = MacroBackup::new(snapshot.clone(), entries).unwrap();
        write_macro_backup(
            dir,
            &snapshot,
            &backup,
            &sha256_hex(snapshot.config.as_bytes()),
        )
        .unwrap();
        (snapshot, backup)
    }

    fn backup_fixture(dir: &Path) -> (ConfigSnapshot, MacroBackup) {
        use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord};
        use eightbitdo_protocol::macro_data;

        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        let mut record = MacroRecord {
            max_steps: 2,
            key_map: KeyMap::M1,
            ..MacroRecord::default()
        };
        record.set_name("Combo").unwrap();
        config.set_macro_record(Slot::S1, MacroIndex::M1, &record);
        config.recompute_crc();

        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        let snapshot = ConfigSnapshot {
            config: config.clone(),
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        };

        let raw = macro_data::encode_macro_steps(&[MacroStep::neutral(10), MacroStep::neutral(20)])
            .unwrap()[..20]
            .to_vec();
        let mut entries = Vec::new();
        for slot in Slot::ALL {
            for index in MacroIndex::ALL {
                let record = config.macro_record(slot, index);
                let entry_raw = if slot == Slot::S1 && index == MacroIndex::M1 {
                    raw.clone()
                } else {
                    Vec::new()
                };
                entries.push(MacroEntry::new(slot, index, record, entry_raw).unwrap());
            }
        }
        let backup = MacroBackup::new(snapshot.clone(), entries).unwrap();
        write_macro_backup(
            dir,
            &snapshot,
            &backup,
            &sha256_hex(snapshot.config.as_bytes()),
        )
        .unwrap();
        (snapshot, backup)
    }

    #[test]
    fn build_write_plan_rejects_empty_target_and_preserves_unspecified_values() {
        let dir = scratch_dir("macro-plan");
        let (_, backup) = backup_fixture(&dir);
        let file = MacroFile {
            name: None,
            gamepad_mode: 0,
            key_map: KeyMap::NONE,
            cycles_num: Some(1),
            interval_ms: Some(5),
            steps: vec![MacroStep::neutral(10), MacroStep::neutral(20)],
        };

        assert!(matches!(
            build_macro_write_plan(&file, Some(KeyMap::M2), &backup, Slot::S1, MacroIndex::M3),
            Err(Error::EmptyMacroWrite)
        ));

        let used = build_macro_write_plan(&file, None, &backup, Slot::S1, MacroIndex::M1).unwrap();
        assert_eq!(used.flag, None);
        assert_eq!(used.total_cnt, None);
        assert_eq!(used.key_map, None);
        assert_eq!(used.name, None);
        assert_eq!(used.cycles_num, Some(1));
        assert_eq!(used.interval_ms, Some(5));

        let steps_only = MacroFile {
            name: None,
            gamepad_mode: 0,
            key_map: KeyMap::NONE,
            cycles_num: None,
            interval_ms: None,
            steps: vec![MacroStep::neutral(10)],
        };
        let preserved =
            build_macro_write_plan(&steps_only, None, &backup, Slot::S1, MacroIndex::M1).unwrap();
        assert_eq!(preserved.cycles_num, None);
        assert_eq!(preserved.interval_ms, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_rejects_gamepad_mode_mismatch() {
        use eightbitdo_protocol::config_bt2::GamepadMode;

        let dir = scratch_dir("macro-mode");
        let (snapshot, _) = backup_fixture(&dir);
        let mut current = snapshot.clone();
        current.config.set_gamepad_mode(GamepadMode::XInput);
        assert!(ensure_backup_gamepad_mode(&snapshot, &current).is_err());
        assert!(ensure_backup_gamepad_mode(&snapshot, &snapshot).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_backup_loads_and_rejects_tampered_files() {
        let dir = scratch_dir("macro-restore");
        let (_, backup) = backup_fixture(&dir);

        let loaded = read_restore_backup(&dir).unwrap();
        assert_eq!(
            loaded.get(Slot::S1, MacroIndex::M1).unwrap().steps().len(),
            2
        );
        assert_eq!(loaded.get(Slot::S2, MacroIndex::M2).unwrap().raw().len(), 0);
        assert_eq!(
            backup.get(Slot::S1, MacroIndex::M1).unwrap().raw().len(),
            20
        );

        // Tamper with the raw bytes: the manifest hash must reject them.
        std::fs::write(dir.join("slot0-macro0.bin"), [0u8; 20]).unwrap();
        assert!(read_restore_backup(&dir).is_err());

        // Restore the bytes and point the manifest at a path outside the
        // directory: the plain-file-name rule must reject it before reading.
        let _ = std::fs::remove_dir_all(&dir);
        backup_fixture(&dir);
        let manifest_path = dir.join("macros.json");
        let mut manifest: MacroBackupDocument =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest.macros[0].raw_file = Some("../evil.bin".to_string());
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();
        assert!(read_restore_backup(&dir).is_err());

        assert!(safe_backup_path(&dir, "../evil.bin").is_err());
        assert!(safe_backup_path(&dir, "/etc/passwd").is_err());
        assert_eq!(
            safe_backup_path(&dir, "slot0-macro0.bin").unwrap(),
            dir.join("slot0-macro0.bin")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
    fn test_macro_file() -> MacroFile {
        MacroFile {
            name: None,
            gamepad_mode: 0,
            key_map: KeyMap::NONE,
            cycles_num: None,
            interval_ms: None,
            steps: vec![MacroStep::neutral(10), MacroStep::neutral(20)],
        }
    }

    #[tokio::test]
    async fn macro_write_empty_target_sends_no_write_frames() {
        let root = scratch_dir("macro-empty-target");
        let fixture_dir = root.join("fixture");
        let (snapshot, _) = backup_fixture(&fixture_dir);
        let backup_dir = root.join("backup");

        let mock = MockTransport::new();
        push_config_responses(&mock, &snapshot.config);
        let session = Session::new(&mock);
        let result = macro_write_session(
            &session,
            Slot::S1,
            MacroIndex::M3,
            &test_macro_file(),
            None,
            &backup_dir,
            None,
        )
        .await;

        assert!(matches!(result, Err(Error::EmptyMacroWrite)));
        assert!(
            !mock
                .sent_frames()
                .iter()
                .any(|frame| is_write_command(frame)),
            "empty macro rejection must not send Erase/Write/Header/Save"
        );
        assert!(!backup_dir.exists(), "empty backup dir must be removed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn macro_write_config_read_failure_sends_no_write_frames() {
        let root = scratch_dir("macro-prebackup-fail");
        let backup_dir = root.join("backup");
        let mock = MockTransport::new();
        let session = Session::new(&mock);
        let result = macro_write_session(
            &session,
            Slot::S1,
            MacroIndex::M1,
            &test_macro_file(),
            None,
            &backup_dir,
            None,
        )
        .await;

        assert!(result.is_err());
        assert!(!mock
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        assert!(!backup_dir.exists(), "empty backup dir must be removed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn macro_write_rejects_non_empty_backup_dir_before_reading() {
        let root = scratch_dir("macro-backup-nonempty");
        let backup_dir = root.join("backup");
        std::fs::create_dir_all(&backup_dir).unwrap();
        std::fs::write(backup_dir.join("config.bin"), [0u8; 1]).unwrap();

        let mock = MockTransport::new();
        let session = Session::new(&mock);
        let result = macro_write_session(
            &session,
            Slot::S1,
            MacroIndex::M1,
            &test_macro_file(),
            None,
            &backup_dir,
            None,
        )
        .await;

        assert!(result.is_err());
        assert!(mock.sent_frames().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_rejects_gamepad_mode_mismatch_before_writes() {
        use eightbitdo_protocol::config_bt2::GamepadMode;

        let root = scratch_dir("macro-restore-mode");
        let fixture_dir = root.join("fixture");
        let (snapshot, _) = backup_fixture(&fixture_dir);
        let mut current = snapshot.config.clone();
        current.set_gamepad_mode(GamepadMode::XInput);
        current.recompute_crc();

        let mock = MockTransport::new();
        push_config_responses(&mock, &current);
        let session = Session::new(&mock);
        let result = macro_restore_session(&session, &fixture_dir, false, None).await;

        assert!(result.is_err());
        assert!(!mock
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_two_macros_uses_one_per_slot_header_save_and_final_save() {
        let root = scratch_dir("macro-restore-batch");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture_with_macros(
            &fixture_dir,
            &[
                (MacroIndex::M1, "One", vec![MacroStep::neutral(10); 2]),
                (MacroIndex::M2, "Two", vec![MacroStep::neutral(20); 2]),
            ],
        );

        let mock = MockTransport::with_mtu(200);
        // Initial config and the pre-restore backup reads.
        push_config_responses(&mock, &snapshot.config);
        push_macro_read(&mock, &snapshot.config, Slot::S1, MacroIndex::M1);
        push_macro_read(&mock, &snapshot.config, Slot::S1, MacroIndex::M2);
        // One header Save for the slot, then Erase/Write for both macros,
        // then one final Save.
        push_ack(&mock, commands::cmd::WRITE_CONFIG);
        push_ack(&mock, commands::cmd::WRITE_CONFIG);
        push_ack(&mock, commands::cmd::SAVE);
        push_ack(&mock, commands::cmd::ERASE_MACRO);
        push_ack(&mock, commands::cmd::WRITE_MACRO);
        push_ack(&mock, commands::cmd::ERASE_MACRO);
        push_ack(&mock, commands::cmd::WRITE_MACRO);
        push_ack(&mock, commands::cmd::SAVE);
        // Batch verification, then the final whole-backup verification.
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M2);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M2);

        let session = Session::new(&mock);
        macro_restore_session(&session, &fixture_dir, false, None)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        let commands: Vec<u16> = sent
            .iter()
            .map(|frame| CustomHeader::parse(&frame[1..]).unwrap().cmd)
            .filter(|command| {
                matches!(
                    *command,
                    commands::cmd::WRITE_CONFIG
                        | commands::cmd::SAVE
                        | commands::cmd::ERASE_MACRO
                        | commands::cmd::WRITE_MACRO
                )
            })
            .collect();
        assert_eq!(
            commands,
            vec![
                commands::cmd::WRITE_CONFIG,
                commands::cmd::WRITE_CONFIG,
                commands::cmd::SAVE,
                commands::cmd::ERASE_MACRO,
                commands::cmd::WRITE_MACRO,
                commands::cmd::ERASE_MACRO,
                commands::cmd::WRITE_MACRO,
                commands::cmd::SAVE,
            ]
        );
        assert_eq!(
            sent.iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE
                })
                .count(),
            2
        );

        // M1's write payload is unchanged by the later M2 write.
        let m1 = backup.get(Slot::S1, MacroIndex::M1).unwrap();
        let first_write = sent
            .iter()
            .find(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_MACRO
            })
            .unwrap();
        assert_eq!(&first_write[17..17 + m1.raw().len()], m1.raw());
        // Per-macro absolute `total_len` on the wire: macro_idx=0 (M1) is
        // 0x20 (base 0), macro_idx=1 (M2) is 0x1020 (0x1000 + 0x20).
        let write_total_lens: Vec<u32> = sent
            .iter()
            .filter(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_MACRO
            })
            .map(|frame| CustomHeader::parse(&frame[1..]).unwrap().total_len)
            .collect();
        assert_eq!(write_total_lens, vec![0x20, 0x1020]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_mtu_23_header_only_succeeds() {
        let root = scratch_dir("macro-restore-mtu23");
        let empty_dir = root.join("empty");
        let current_dir = root.join("current");
        let (empty_snapshot, _) = empty_backup_fixture(&empty_dir);
        let (current_snapshot, _) = backup_fixture(&current_dir);

        let mock = MockTransport::with_mtu(23);
        push_config_responses(&mock, &current_snapshot.config);
        push_macro_read(&mock, &current_snapshot.config, Slot::S1, MacroIndex::M1);
        // MTU 23 leaves 3 payload bytes per header packet: 216 / 3 = 72.
        for _ in 0..72 {
            push_ack(&mock, commands::cmd::WRITE_CONFIG);
        }
        push_ack(&mock, commands::cmd::SAVE);
        push_config_responses(&mock, &empty_snapshot.config);
        push_config_responses(&mock, &empty_snapshot.config);

        let session = Session::new(&mock);
        macro_restore_session(&session, &empty_dir, true, None)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert_eq!(
            sent.iter()
                .filter(|frame| {
                    CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_CONFIG
                })
                .count(),
            72
        );
        assert!(!sent.iter().any(|frame| matches!(
            CustomHeader::parse(&frame[1..]).unwrap().cmd,
            commands::cmd::ERASE_MACRO | commands::cmd::WRITE_MACRO
        )));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_rejects_chunk_size_48_before_creating_a_backup() {
        let root = scratch_dir("macro-restore-chunk48");
        let fixture_dir = root.join("fixture");
        empty_backup_fixture(&fixture_dir);
        let before: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();

        let mock = MockTransport::with_mtu(200);
        let session = Session::new(&mock);
        let result = macro_restore_session(&session, &fixture_dir, false, Some(48)).await;
        assert!(matches!(
            result,
            Err(Error::Protocol(
                eightbitdo_protocol::Error::UnsupportedChunkSize { actual: 48 }
            ))
        ));
        assert!(mock.sent_frames().is_empty());
        let after: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(before, after, "no pre-restore directory may be created");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_chunk_size_32_writes_32_byte_macro_chunks() {
        let root = scratch_dir("macro-restore-chunk32");
        let fixture_dir = root.join("fixture");
        let steps = vec![MacroStep::neutral(10); 46];
        let (snapshot, backup) =
            backup_fixture_with_macros(&fixture_dir, &[(MacroIndex::M2, "Long", steps)]);

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_macro_read(&mock, &snapshot.config, Slot::S1, MacroIndex::M2);
        push_ack(&mock, commands::cmd::WRITE_CONFIG);
        push_ack(&mock, commands::cmd::WRITE_CONFIG);
        push_ack(&mock, commands::cmd::SAVE);
        push_ack(&mock, commands::cmd::ERASE_MACRO);
        for _ in 0..15 {
            push_ack(&mock, commands::cmd::WRITE_MACRO);
        }
        push_ack(&mock, commands::cmd::SAVE);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M2);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M2);

        let session = Session::new(&mock);
        macro_restore_session(&session, &fixture_dir, false, Some(32))
            .await
            .unwrap();

        let writes: Vec<_> = mock
            .sent_frames()
            .iter()
            .filter(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_MACRO
            })
            .cloned()
            .collect();
        assert_eq!(writes.len(), 15);
        for (index, frame) in writes.iter().enumerate() {
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            // Macro 2 is sector 0x1000, so the default total_len is the
            // absolute end address 0x1000 + 480 = 0x11E0.
            assert_eq!(header.total_len, 0x11E0);
            assert_eq!(header.chunk_len, 32);
            assert_eq!(header.offset, 0x1000 + (index * 32) as u32);
            assert_eq!(frame[17..].len(), 32);
            assert_eq!(
                header.crc16,
                eightbitdo_protocol::crc::packet_crc16(&frame[17..])
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_verifies_final_config_headers() {
        let root = scratch_dir("macro-restore-verify");
        let fixture_dir = root.join("fixture");
        let (snapshot, _) = empty_backup_fixture(&fixture_dir);

        let mock = MockTransport::new();
        push_config_responses(&mock, &snapshot.config);
        push_config_responses(&mock, &snapshot.config);
        let session = Session::new(&mock);
        macro_restore_session(&session, &fixture_dir, false, None)
            .await
            .unwrap();

        assert!(!mock
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_final_config_mismatch_is_an_error() {
        let root = scratch_dir("macro-restore-verify-fail");
        let fixture_dir = root.join("fixture");
        let (snapshot, _) = empty_backup_fixture(&fixture_dir);

        let mut changed = snapshot.config.clone();
        let mut header = changed.macro_slot(Slot::S1);
        header.flag = 1;
        changed.set_macro_slot(Slot::S1, &header);
        changed.recompute_crc();

        let mock = MockTransport::new();
        push_config_responses(&mock, &snapshot.config);
        push_config_responses(&mock, &changed);
        let session = Session::new(&mock);
        let result = macro_restore_session(&session, &fixture_dir, false, None).await;

        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message))
                if message.contains("216B header") && message.contains("pre-restore backup")
        ));
        assert!(!mock
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_force_gate_lists_targets_and_allows_with_force() {
        let root = scratch_dir("macro-force-gate");
        let empty_dir = root.join("empty");
        let current_dir = root.join("current");
        let (_, empty_backup) = empty_backup_fixture(&empty_dir);
        let (current_snapshot, _) = backup_fixture(&current_dir);

        let conflicts = restore_conflicts_from_config(&empty_backup, &current_snapshot);
        assert!(conflicts
            .iter()
            .any(|target| target.contains("slot 0 macro 0")));
        assert!(
            ensure_restore_allowed_from_config(&empty_backup, &current_snapshot, false).is_err()
        );
        assert_eq!(
            ensure_restore_allowed_from_config(&empty_backup, &current_snapshot, true).unwrap(),
            conflicts
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_directory_must_be_empty() {
        let root = scratch_dir("macro-dir-guard");
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(prepare_output_directory(&empty).is_ok());

        std::fs::write(empty.join("config.bin"), [0u8; 1]).unwrap();
        assert!(prepare_output_directory(&empty).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
    #[tokio::test]
    async fn restore_force_allows_header_only_overwrite() {
        let root = scratch_dir("macro-force-restore");
        let empty_dir = root.join("empty");
        let current_dir = root.join("current");
        let (empty_snapshot, _) = empty_backup_fixture(&empty_dir);
        let (current_snapshot, _) = backup_fixture(&current_dir);

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &current_snapshot.config);
        push_macro_read(&mock, &current_snapshot.config, Slot::S1, MacroIndex::M1);
        // Header-only write for slot 0: two header chunks + Save, then the
        // device returns the empty backup header on the verification read.
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(&CustomHeader::new(commands::cmd::SAVE, 0, 0, 0), &[]);
        push_config_responses(&mock, &empty_snapshot.config);
        push_config_responses(&mock, &empty_snapshot.config);

        let session = Session::new(&mock);
        macro_restore_session(&session, &empty_dir, true, None)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert!(sent
            .iter()
            .any(|frame| { CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE }));
        assert!(!sent.iter().any(|frame| {
            matches!(
                CustomHeader::parse(&frame[1..]).unwrap().cmd,
                commands::cmd::ERASE_MACRO | commands::cmd::WRITE_MACRO
            )
        }));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_without_force_refuses_before_header_write() {
        let root = scratch_dir("macro-no-force-restore");
        let empty_dir = root.join("empty");
        let current_dir = root.join("current");
        empty_backup_fixture(&empty_dir);
        let (current_snapshot, _) = backup_fixture(&current_dir);

        let mock = MockTransport::new();
        push_config_responses(&mock, &current_snapshot.config);
        push_macro_read(&mock, &current_snapshot.config, Slot::S1, MacroIndex::M1);
        let session = Session::new(&mock);
        let result = macro_restore_session(&session, &empty_dir, false, None).await;

        assert!(result.is_err());
        assert!(!mock
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        let _ = std::fs::remove_dir_all(&root);
    }
    #[tokio::test]
    async fn macro_backup_rejects_non_empty_dir_before_reading() {
        let root = scratch_dir("macro-backup-session-guard");
        let dir = root.join("backup");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.bin"), [0u8; 1]).unwrap();

        let mock = MockTransport::new();
        let session = Session::new(&mock);
        let result = macro_backup_session(&session, &dir).await;
        assert!(result.is_err());
        assert!(mock.sent_frames().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_macro_backup_refuses_existing_files() {
        let root = scratch_dir("macro-write-backup-guard");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture(&fixture_dir);
        let dir = root.join("existing");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.bin"), b"keep").unwrap();

        let result = write_macro_backup(
            &dir,
            &snapshot,
            &backup,
            &sha256_hex(snapshot.config.as_bytes()),
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(dir.join("config.bin")).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&root);
    }
    #[tokio::test]
    async fn restore_verifies_non_empty_macro_raw() {
        let root = scratch_dir("macro-restore-raw");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture(&fixture_dir);

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(&CustomHeader::new(commands::cmd::SAVE, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::ERASE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::WRITE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::SAVE, 0, 0, 0), &[]);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);

        let session = Session::new(&mock);
        macro_restore_session(&session, &fixture_dir, false, None)
            .await
            .unwrap();

        let sent = mock.sent_frames();
        assert!(sent
            .iter()
            .any(|frame| { CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE }));
        assert!(sent.iter().any(|frame| {
            CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::READ_MACRO
        }));
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn empty_backup_path_is_rejected_without_touching_cwd() {
        let cwd_config = PathBuf::from("config.bin");
        let before = std::fs::read(&cwd_config).ok();
        assert!(prepare_output_directory(Path::new("")).is_err());
        assert!(read_restore_backup(Path::new("")).is_err());

        let root = scratch_dir("macro-empty-path");
        let (snapshot, backup) = backup_fixture(&root.join("fixture"));
        assert!(write_macro_backup(
            Path::new(""),
            &snapshot,
            &backup,
            &sha256_hex(snapshot.config.as_bytes())
        )
        .is_err());
        assert_eq!(std::fs::read(&cwd_config).ok(), before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn backup_dir_rejects_trailing_slash_symlink() {
        use std::os::unix::fs::symlink;

        let root = scratch_dir("macro-symlink-slash");
        let real = root.join("real");
        let link = root.join("link");
        std::fs::create_dir_all(&real).unwrap();
        symlink(&real, &link).unwrap();

        let with_slash = PathBuf::from(format!("{}/", link.display()));
        let with_dot = link.join(".");
        assert!(prepare_output_directory(&with_slash).is_err());
        assert!(prepare_output_directory(&with_dot).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn mtu_23_fails_before_creating_pre_write_backup() {
        let root = scratch_dir("macro-mtu23");
        let dir = root.join("backup");
        let mock = MockTransport::with_mtu(23);
        let session = Session::new(&mock);
        let result = macro_write_session(
            &session,
            Slot::S1,
            MacroIndex::M1,
            &test_macro_file(),
            None,
            &dir,
            None,
        )
        .await;
        assert!(matches!(result, Err(Error::MtuTooSmall { mtu: 23, .. })));
        assert!(mock.sent_frames().is_empty());
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn current_macro_backup_skips_unreadable_entry() {
        use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord};

        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        for (slot, index) in [(Slot::S1, MacroIndex::M1), (Slot::S1, MacroIndex::M2)] {
            let record = MacroRecord {
                max_steps: 2,
                ..MacroRecord::default()
            };
            config.set_macro_record(slot, index, &record);
        }
        config.recompute_crc();
        let snapshot = config_snapshot(config);

        let mock = MockTransport::new();
        push_macro_read(&mock, &snapshot.config, Slot::S1, MacroIndex::M1);
        let session = Session::new(&mock);
        let (backup, skipped) = read_current_macros_best_effort(&session, &snapshot)
            .await
            .unwrap();

        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("slot 0 macro 1"));
        assert!(backup.get(Slot::S1, MacroIndex::M2).unwrap().is_empty());

        let dir = scratch_dir("macro-best-effort-manifest");
        write_macro_backup(
            &dir,
            backup.snapshot(),
            &backup,
            &sha256_hex(backup.snapshot().config.as_bytes()),
        )
        .unwrap();
        let manifest: MacroBackupDocument =
            serde_json::from_str(&std::fs::read_to_string(dir.join("macros.json")).unwrap())
                .unwrap();
        let skipped_entry = manifest
            .macros
            .iter()
            .find(|entry| entry.slot == 0 && entry.index == 1)
            .unwrap();
        assert!(skipped_entry.skipped);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn current_macro_backup_preserves_config_when_complete() {
        let root = scratch_dir("macro-best-effort-complete");
        let fixture_dir = root.join("fixture");
        let (snapshot, _) = backup_fixture(&fixture_dir);
        let mock = MockTransport::new();
        push_macro_read(&mock, &snapshot.config, Slot::S1, MacroIndex::M1);
        let session = Session::new(&mock);
        let (backup, skipped) = read_current_macros_best_effort(&session, &snapshot)
            .await
            .unwrap();
        assert!(skipped.is_empty());
        assert_eq!(
            backup.snapshot().config.as_bytes(),
            snapshot.config.as_bytes()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_final_raw_mismatch_is_an_error() {
        let root = scratch_dir("macro-restore-raw-fail");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture(&fixture_dir);

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(&CustomHeader::new(commands::cmd::SAVE, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::ERASE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::WRITE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::SAVE, 0, 0, 0), &[]);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        push_config_responses(&mock, &snapshot.config);
        let mut wrong = backup.get(Slot::S1, MacroIndex::M1).unwrap().raw().to_vec();
        wrong[0] ^= 0xFF;
        push_macro_raw(&mock, &wrong);

        let session = Session::new(&mock);
        let result = macro_restore_session(&session, &fixture_dir, false, None).await;
        assert!(matches!(
            result,
            Err(Error::UnexpectedResponse(message))
                if message.contains("raw steps differ") && message.contains("pre-restore backup")
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_requires_raw_sha256_for_non_empty_entry() {
        let root = scratch_dir("macro-restore-no-sha");
        let dir = root.join("fixture");
        backup_fixture(&dir);
        let manifest_path = dir.join("macros.json");
        let mut manifest: MacroBackupDocument =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest.macros[0].raw_sha256 = None;
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();
        assert!(read_restore_backup(&dir).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_plan_propagates_the_experimental_write_layout() {
        let dir = scratch_dir("macro-restore-layout");
        let (_, backup) = backup_fixture(&dir);
        let entry = backup.get(Slot::S1, MacroIndex::M1).unwrap();
        let header = backup.snapshot().config.macro_slot(Slot::S1);
        let layout = MacroWriteLayout::relative_total_len();
        let plan = restore_macro_plan(entry, &header, None, layout);
        assert_eq!(plan.layout(), layout);
        // The restore default is the absolute layout.
        let default = restore_macro_plan(entry, &header, None, MacroWriteLayout::default_ble());
        assert!(default.absolute_total_len);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_plan_does_not_reencode_name() {
        let dir = scratch_dir("macro-restore-name");
        let (_, backup) = backup_fixture(&dir);
        let entry = backup.get(Slot::S1, MacroIndex::M1).unwrap();
        let header = backup.snapshot().config.macro_slot(Slot::S1);
        let plan = restore_macro_plan(entry, &header, Some(32), MacroWriteLayout::default_ble());
        assert_eq!(plan.name, None);
        assert_eq!(plan.chunk_size, Some(32));
        assert_eq!(plan.cycles_num, Some(entry.record().cycles_num));
        assert_eq!(plan.interval_ms, Some(entry.record().interval_ms));
        let default = restore_macro_plan(entry, &header, None, MacroWriteLayout::default_ble());
        assert_eq!(default.chunk_size, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_write_plan_carries_trigger_and_name() {
        let dir = scratch_dir("macro-trigger-name");
        let (_, backup) = backup_fixture(&dir);
        let file = MacroFile {
            name: Some("PlanName".to_string()),
            gamepad_mode: 0,
            key_map: KeyMap::NONE,
            cycles_num: None,
            interval_ms: None,
            steps: vec![MacroStep::neutral(10)],
        };
        let plan =
            build_macro_write_plan(&file, Some(KeyMap::M2), &backup, Slot::S1, MacroIndex::M1)
                .unwrap();
        assert_eq!(plan.name.as_deref(), Some("PlanName"));
        assert_eq!(plan.key_map, Some(KeyMap::M2));
        let _ = std::fs::remove_dir_all(&dir);
    }
    fn non_empty_current_config() -> ConfigSnapshot {
        use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord};

        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        config.set_macro_record(
            Slot::S1,
            MacroIndex::M1,
            &MacroRecord {
                max_steps: 2,
                ..MacroRecord::default()
            },
        );
        config.recompute_crc();
        config_snapshot(config)
    }

    #[tokio::test]
    async fn restore_force_aborts_on_pre_restore_backup_failure() {
        let root = scratch_dir("macro-force-backup-fail");
        let target_dir = root.join("target");
        let (target_snapshot, _) = empty_backup_fixture(&target_dir);
        let pre_dir = root.join("pre");
        std::fs::create_dir_all(&pre_dir).unwrap();
        std::fs::write(pre_dir.join("config.bin"), b"occupied").unwrap();

        let mock = MockTransport::new();
        push_config_responses(&mock, &target_snapshot.config);
        let session = Session::new(&mock);
        let result =
            macro_restore_session_with_pre_dir(&session, &target_dir, true, pre_dir.clone(), None)
                .await;

        assert!(matches!(
            result,
            Err(Error::Io(error))
                if error.to_string().contains("complete recovery backup could not be created")
        ));
        assert!(!mock
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_without_force_aborts_on_skipped_current_macro() {
        let root = scratch_dir("macro-skip-noforce");
        let target_dir = root.join("target");
        empty_backup_fixture(&target_dir);
        let current = non_empty_current_config();
        let pre_dir = root.join("pre");

        let transport = FailMacroReadTransport::new();
        push_config_responses(&transport.inner, &current.config);
        let session = Session::new(&transport);
        let result =
            macro_restore_session_with_pre_dir(&session, &target_dir, false, pre_dir, None).await;

        assert!(result.is_err());
        assert!(!transport
            .inner
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn restore_force_continues_when_current_macro_is_unreadable() {
        let root = scratch_dir("macro-skip-force");
        let target_dir = root.join("target");
        let (target_snapshot, _) = empty_backup_fixture(&target_dir);
        let current = non_empty_current_config();
        let pre_dir = root.join("pre");

        let transport = FailMacroReadTransport::new();
        push_config_responses(&transport.inner, &current.config);
        transport.inner.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        transport.inner.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        transport
            .inner
            .push_response(&CustomHeader::new(commands::cmd::SAVE, 0, 0, 0), &[]);
        push_config_responses(&transport.inner, &target_snapshot.config);
        push_config_responses(&transport.inner, &target_snapshot.config);

        let session = Session::new(&transport);
        let result =
            macro_restore_session_with_pre_dir(&session, &target_dir, true, pre_dir, None).await;
        assert!(
            result.is_ok(),
            "force must continue after skipped: {result:?}"
        );
        assert!(transport
            .inner
            .sent_frames()
            .iter()
            .any(|frame| { CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::SAVE }));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn macro_write_rejects_unreadable_macro_in_target_slot() {
        use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord};

        let root = scratch_dir("macro-target-unreadable");
        let backup_dir = root.join("backup");
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        for index in [MacroIndex::M1, MacroIndex::M2] {
            config.set_macro_record(
                Slot::S1,
                index,
                &MacroRecord {
                    max_steps: 2,
                    ..MacroRecord::default()
                },
            );
        }
        config.recompute_crc();
        let snapshot = config_snapshot(config);

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_macro_read(&mock, &snapshot.config, Slot::S1, MacroIndex::M1);
        let session = Session::new(&mock);
        let result = macro_write_session(
            &session,
            Slot::S1,
            MacroIndex::M1,
            &test_macro_file(),
            None,
            &backup_dir,
            None,
        )
        .await;

        assert!(matches!(
            result,
            Err(Error::Unsupported(message))
                if message.contains("unreadable macro in target slot")
                    && message.contains("macro restore or official app")
        ));
        assert!(!mock
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        assert!(!backup_dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn incomplete_recovery_warning_does_not_advertise_recover_with() {
        let skipped = vec!["slot 0 macro 1: disconnected".to_string()];
        let incomplete = restore_recovery_warning(false, Path::new("/tmp/backup"), &skipped);
        assert!(!incomplete.contains("recover with"));
        assert!(incomplete.contains("no complete recovery backup"));
        assert!(incomplete.contains("slot 0 macro 1"));
        let complete = restore_recovery_warning(true, Path::new("/tmp/backup"), &[]);
        assert!(complete.contains("recover with"));
    }

    #[test]
    fn dot_backup_dir_is_rejected() {
        assert!(prepare_output_directory(Path::new(".")).is_err());
        assert!(prepare_output_directory(Path::new("./")).is_err());
        assert!(read_restore_backup(Path::new(".")).is_err());
        assert!(read_restore_backup(Path::new("./")).is_err());
    }
    /// The default absolute `total_len` reaches the wire: `macro_idx=1 (M2)`
    /// (sector 0x1000, slot S1, BLE) with 2 steps sends `total_len = 0x1020`,
    /// `offset = 0x1000`. `macro_idx=0 (M1)` is unaffected and is covered by
    /// the protocol unit test.
    #[tokio::test]
    async fn macro_write_default_absolute_total_len_reaches_the_wire() {
        let root = scratch_dir("macro-absolute-total");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture_with_macros(
            &fixture_dir,
            &[
                (MacroIndex::M1, "One", vec![MacroStep::neutral(10); 2]),
                (
                    MacroIndex::M2,
                    "Two",
                    vec![MacroStep::neutral(10), MacroStep::neutral(20)],
                ),
            ],
        );
        let backup_dir = root.join("prewrite");

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M2);
        push_ack(&mock, commands::cmd::WRITE_CONFIG);
        push_ack(&mock, commands::cmd::WRITE_CONFIG);
        push_ack(&mock, commands::cmd::SAVE);
        push_ack(&mock, commands::cmd::ERASE_MACRO);
        push_ack(&mock, commands::cmd::WRITE_MACRO);
        push_ack(&mock, commands::cmd::SAVE);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M2);

        let session = Session::new(&mock);
        // The default wrapper uses MacroWriteLayout::default_ble(), i.e. no
        // experimental flag: this is the production frame.
        macro_write_session(
            &session,
            Slot::S1,
            MacroIndex::M2,
            &test_macro_file(),
            None,
            &backup_dir,
            None,
        )
        .await
        .expect("default absolute-total write must succeed against the mock");

        let sent = mock.sent_frames();
        let write = sent
            .iter()
            .find(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_MACRO
            })
            .map(|frame| CustomHeader::parse(&frame[1..]).unwrap())
            .expect("one WriteBT2Macro frame");
        // 2 steps = 20 bytes -> 32-byte alignment; macro_idx=1 (M2) sector
        // base 0x1000.
        assert_eq!(write.total_len, 0x1020);
        assert_eq!(write.offset, 0x1000);
        assert_eq!(write.chunk_len, 32);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The relative framing is still reachable for A/B work and reaches the
    /// wire unchanged: 115 steps (1150B -> 0x480) send
    /// `total_len = 0x480` (no sector base) while the Write offsets keep the
    /// macro sector base (`0x1000` for `macro_idx=1 (M2)`).
    #[tokio::test]
    async fn macro_write_relative_total_len_keeps_the_sector_offsets() {
        let root = scratch_dir("macro-relative-total");
        let fixture_dir = root.join("fixture");
        let steps = vec![MacroStep::neutral(10); 115];
        let (snapshot, backup) =
            backup_fixture_with_macros(&fixture_dir, &[(MacroIndex::M2, "Long", steps.clone())]);
        let backup_dir = root.join("prewrite");
        let file = MacroFile {
            name: None,
            gamepad_mode: 0,
            key_map: KeyMap::NONE,
            cycles_num: None,
            interval_ms: None,
            steps,
        };

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M2);
        push_ack(&mock, commands::cmd::WRITE_CONFIG);
        push_ack(&mock, commands::cmd::WRITE_CONFIG);
        push_ack(&mock, commands::cmd::SAVE);
        push_ack(&mock, commands::cmd::ERASE_MACRO);
        // 1150B -> 0x480 aligned -> 18 x 64B chunks.
        for _ in 0..18 {
            push_ack(&mock, commands::cmd::WRITE_MACRO);
        }
        push_ack(&mock, commands::cmd::SAVE);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M2);

        let session = Session::new(&mock);
        macro_write_session_with_layout(
            &session,
            Slot::S1,
            MacroIndex::M2,
            &file,
            None,
            &backup_dir,
            None,
            MacroWriteLayout::relative_total_len(),
        )
        .await
        .expect("relative-total write must succeed against the mock");

        let writes: Vec<_> = mock
            .sent_frames()
            .iter()
            .filter(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_MACRO
            })
            .cloned()
            .collect();
        assert_eq!(writes.len(), 18);
        for (index, frame) in writes.iter().enumerate() {
            let header = CustomHeader::parse(&frame[1..]).unwrap();
            assert_eq!(header.total_len, 0x480);
            assert_eq!(header.chunk_len, 64);
            assert_eq!(header.offset, 0x1000 + (index * 64) as u32);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn macro_write_session_backs_up_all_macros_then_writes_and_verifies() {
        let root = scratch_dir("macro-write-success");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture(&fixture_dir);
        let backup_dir = root.join("prewrite");

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(&CustomHeader::new(commands::cmd::SAVE, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::ERASE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::WRITE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::SAVE, 0, 0, 0), &[]);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);

        let session = Session::new(&mock);
        macro_write_session(
            &session,
            Slot::S1,
            MacroIndex::M1,
            &test_macro_file(),
            None,
            &backup_dir,
            None,
        )
        .await
        .expect("full backup -> write -> verify must succeed");

        assert!(backup_dir.join("config.bin").is_file());
        assert_eq!(
            std::fs::read(backup_dir.join("config.bin")).unwrap().len(),
            CONFIG_SIZE
        );
        let manifest: MacroBackupDocument =
            serde_json::from_str(&std::fs::read_to_string(backup_dir.join("macros.json")).unwrap())
                .unwrap();
        assert_eq!(manifest.macros.len(), 12);
        let order: Vec<(u8, u8)> = manifest
            .macros
            .iter()
            .map(|entry| (entry.slot, entry.index))
            .collect();
        assert_eq!(
            order,
            vec![
                (0, 0),
                (0, 1),
                (0, 2),
                (0, 3),
                (1, 0),
                (1, 1),
                (1, 2),
                (1, 3),
                (2, 0),
                (2, 1),
                (2, 2),
                (2, 3),
            ]
        );

        let loaded = read_restore_backup(&backup_dir).unwrap();
        assert_eq!(
            loaded.get(Slot::S1, MacroIndex::M1).unwrap().steps().len(),
            2
        );
        assert!(loaded.get(Slot::S1, MacroIndex::M2).unwrap().is_empty());

        let sent = mock.sent_frames();
        let count = |command: u16| {
            sent.iter()
                .filter(|frame| CustomHeader::parse(&frame[1..]).unwrap().cmd == command)
                .count()
        };
        assert_eq!(count(commands::cmd::READ_CONFIG), 40);
        assert_eq!(count(commands::cmd::READ_MACRO), 2);
        assert_eq!(count(commands::cmd::ERASE_MACRO), 1);
        assert_eq!(count(commands::cmd::WRITE_MACRO), 1);
        assert_eq!(count(commands::cmd::WRITE_CONFIG), 2);
        assert_eq!(count(commands::cmd::SAVE), 2);

        let write = sent
            .iter()
            .find(|frame| {
                CustomHeader::parse(&frame[1..]).unwrap().cmd == commands::cmd::WRITE_MACRO
            })
            .map(|frame| CustomHeader::parse(&frame[1..]).unwrap())
            .unwrap();
        assert_eq!(write.subcmd, 0);
        assert_eq!(write.offset, 0);
        assert_eq!(write.total_len, 32);
        assert_eq!(write.chunk_len, 32);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn macro_write_non_target_unreadable_macro_sends_no_write_frames() {
        use eightbitdo_protocol::config_bt2::{GamepadMode, MacroRecord};

        let root = scratch_dir("macro-nontarget-unreadable");
        let backup_dir = root.join("prewrite");
        let mut config = Bt2Config::zeroed();
        config.set_gamepad_mode(GamepadMode::Switch);
        for (slot, index) in [(Slot::S1, MacroIndex::M1), (Slot::S2, MacroIndex::M1)] {
            config.set_macro_record(
                slot,
                index,
                &MacroRecord {
                    max_steps: 2,
                    ..MacroRecord::default()
                },
            );
        }
        config.recompute_crc();
        let snapshot = config_snapshot(config);

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_macro_read(&mock, &snapshot.config, Slot::S1, MacroIndex::M1);
        let session = Session::new(&mock);
        let result = macro_write_session(
            &session,
            Slot::S1,
            MacroIndex::M1,
            &test_macro_file(),
            None,
            &backup_dir,
            None,
        )
        .await;

        assert!(result.is_err());
        assert!(!mock
            .sent_frames()
            .iter()
            .any(|frame| is_write_command(frame)));
        assert!(!backup_dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
