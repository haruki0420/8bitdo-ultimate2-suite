//! Command-line interface for the 8BitDo Ultimate 2 Bluetooth controller.
//!
//! The binary exposes `scan`, `info`, `config dump`, `backup`, `monitor`,
//! `keepalive`, `session` and the `macro` backup / write / restore commands.
//! Configuration-structure writes, config apply and OTA are not reachable
//! from this binary; macro flash write is.

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
    macro_header_chunk_len, macro_ops::is_skipped_steps, macro_write_chunk_len,
    macro_write_chunk_len_requested, scan_devices, BleOptions, BleTransport, ConfigSnapshot, Error,
    MacroBackup, MacroEntry, MacroWritePlan, Session, Transport, VersionInfo,
};
use eightbitdo_protocol::commands::SaveTransport;
use eightbitdo_protocol::config_bt2::{
    macro_record_offset, macro_slot_offset, Bt2Config, MacroIndex, MacroSlot, Slot, CONFIG_SIZE,
    MACROS_PER_SLOT, MACRO_SLOT_SIZE, SLOT_COUNT,
};
use eightbitdo_protocol::input_report::InputReport;
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
    Scan,

    /// Connect and read the firmware version.
    Info,

    /// Configuration operations (read-only).
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

#[derive(Debug, Subcommand)]
enum MacroCommand {
    /// Back up all 3 slots × 4 macros (12 entries) into a directory.
    Backup {
        /// Output directory for config.bin, config.json, macros.json,
        /// macros.toml and the per-macro raw .bin files.
        #[arg(long, value_name = "DIR")]
        dir: PathBuf,
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
        /// `macro-backup-<unix-seconds>-<pid>`.
        #[arg(long, value_name = "DIR")]
        backup_dir: Option<PathBuf>,

        /// Force 32- or 64-byte write chunks. Omit to select from the MTU.
        #[arg(long, value_name = "32|64")]
        chunk_size: Option<u16>,
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
        Command::Macro { command } => match command {
            MacroCommand::Backup { dir } => {
                macro_backup(&options, &dir).await?;
            }
            MacroCommand::Write {
                slot,
                index,
                file,
                trigger,
                backup_dir,
                chunk_size,
            } => {
                macro_write(
                    &options,
                    slot,
                    index,
                    &file,
                    trigger.as_deref(),
                    backup_dir.as_deref(),
                    chunk_size,
                )
                .await?;
            }
            MacroCommand::Restore {
                dir,
                force,
                chunk_size,
            } => {
                macro_restore(&options, &dir, force, chunk_size).await?;
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

/// Best-effort timeout for teardown operations (`set_report_enable(false)` and
/// `disconnect`) so exiting commands cannot hang on a wedged BLE stack.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

async fn best_effort_disable_reports<T: Transport>(session: &Session<T>, timeout: Duration) {
    let _ = tokio::time::timeout(timeout, session.set_report_enable(false)).await;
}

async fn best_effort_disconnect<T: Transport>(transport: &T, timeout: Duration) {
    let _ = tokio::time::timeout(timeout, transport.disconnect()).await;
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
) -> Result<(), Error> {
    validate_output_paths(raw_path, json_path)?;
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let result = backup_session(&session, raw_path, json_path, hold, shutdown).await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
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

/// Read JSON or TOML macro input.
fn read_macro_file(path: &Path) -> Result<MacroFile, Error> {
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

fn default_backup_dir(prefix: &str) -> PathBuf {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    PathBuf::from(format!("{prefix}-{seconds}-{}", std::process::id()))
}

fn pre_restore_dir(dir: &Path) -> PathBuf {
    let name = dir
        .file_name()
        .map_or_else(|| "macro-backup".into(), |name| name.to_string_lossy());
    let parent = dir.parent().unwrap_or_else(|| Path::new("."));
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    parent.join(format!(
        "{name}-pre-restore-{seconds}-{}",
        std::process::id()
    ))
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

async fn macro_write(
    options: &BleOptions,
    slot: u8,
    index: u8,
    file: &Path,
    trigger: Option<&str>,
    backup_dir: Option<&Path>,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    let slot = parse_slot(slot)?;
    let index = parse_macro_index(index)?;
    let file = read_macro_file(file)?;
    let trigger = trigger.map(parse_trigger).transpose()?;

    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let backup_dir = backup_dir.map_or_else(
        || default_backup_dir("macro-backup"),
        std::path::Path::to_path_buf,
    );
    let result = macro_write_session(
        &session,
        slot,
        index,
        &file,
        trigger,
        &backup_dir,
        chunk_size,
    )
    .await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    result
}

async fn macro_write_session<T: Transport>(
    session: &Session<T>,
    slot: Slot,
    index: MacroIndex,
    file: &MacroFile,
    trigger: Option<KeyMap>,
    backup_dir: &Path,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    let created = !backup_dir.exists();
    let result =
        macro_write_session_inner(session, slot, index, file, trigger, backup_dir, chunk_size)
            .await;
    if result.is_err() && created {
        // Do not leave an empty directory behind on an early failure (empty
        // macro, MTU too small, config read failure).
        let _ = std::fs::remove_dir(lexical_directory(backup_dir));
    }
    result
}

async fn macro_write_session_inner<T: Transport>(
    session: &Session<T>,
    slot: Slot,
    index: MacroIndex,
    file: &MacroFile,
    trigger: Option<KeyMap>,
    backup_dir: &Path,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    let mtu = session.transport().mtu();
    // Fail before creating a pre-write backup directory if this MTU cannot
    // support the macro write at all.
    macro_header_chunk_len(mtu)?;
    if let Some(requested) = chunk_size {
        macro_write_chunk_len_requested(mtu, requested)?;
    } else {
        macro_write_chunk_len(mtu)?;
    }
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
    plan.save_transport = SaveTransport::Ble;

    println!(
        "writing slot {} (profile {}) macro {} ({}): {} steps (chunk size from MTU {})",
        slot.index(),
        slot.number(),
        index.index(),
        macro_id(index),
        plan.steps.len(),
        mtu
    );
    eprintln!(
        "warning: do not press Ctrl-C during the macro write; a failure can leave a sector \
         partially updated. pre-write backup: {}; recover with: eightbitdo macro restore \
         --dir {}",
        backup_dir.display(),
        backup_dir.display()
    );
    let outcome = session.write_macro(&backup, slot, index, &plan).await?;

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
    println!("  read-back offsets: {:?}", outcome.readback.read_offsets());
    println!(
        "restore with: eightbitdo macro restore --dir {}",
        backup_dir.display()
    );
    Ok(())
}

/// Read and validate `config.bin` / `macros.json` (or `macros.toml`).
fn read_restore_backup(dir: &Path) -> Result<MacroBackup, Error> {
    if dir.as_os_str().is_empty() || lexical_directory(dir).as_os_str().is_empty() {
        return Err(invalid_input("backup directory must not be empty"));
    }
    let config_path = dir.join("config.bin");
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

async fn macro_restore(
    options: &BleOptions,
    dir: &Path,
    force: bool,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let result = macro_restore_session(&session, dir, force, chunk_size).await;
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
) -> MacroWritePlan {
    let mut plan = MacroWritePlan::new(entry.steps().to_vec())
        .with_key_map(entry.record().key_map)
        .with_cycles_num(entry.record().cycles_num)
        .with_interval_ms(entry.record().interval_ms)
        .with_header_state(header.flag, header.total_cnt)
        .with_save_transport(SaveTransport::Ble);
    plan.chunk_size = chunk_size;
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

async fn macro_restore_session<T: Transport>(
    session: &Session<T>,
    dir: &Path,
    force: bool,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    let pre_dir = pre_restore_dir(dir);
    macro_restore_session_with_pre_dir(session, dir, force, pre_dir, chunk_size).await
}

async fn macro_restore_session_with_pre_dir<T: Transport>(
    session: &Session<T>,
    dir: &Path,
    force: bool,
    pre_dir: PathBuf,
    chunk_size: Option<u16>,
) -> Result<(), Error> {
    let pre_created = !pre_dir.exists();
    let result = macro_restore_session_core(session, dir, force, &pre_dir, chunk_size).await;
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
) -> Result<(), Error> {
    let mtu = session.transport().mtu();
    macro_header_chunk_len(mtu)?;
    if let Some(requested) = chunk_size {
        macro_write_chunk_len_requested(mtu, requested)?;
    } else {
        macro_write_chunk_len(mtu)?;
    }
    let backup = read_restore_backup(dir)?;
    println!("loaded backup: {}", dir.display());

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

    if has_writes {
        eprintln!(
            "warning: do not press Ctrl-C during the macro write; a failure can leave a sector \
             partially updated. {}",
            restore_recovery_warning(pre_restore_complete, pre_dir, &skipped_reads)
        );
    }

    for slot in Slot::ALL {
        for index in MacroIndex::ALL {
            let entry = backup.get(slot, index).expect("full backup has 12 entries");
            let record = entry.record();
            if is_skipped_steps(record.max_steps) {
                continue;
            }
            let header = backup.snapshot().config.macro_slot(slot);
            let plan = restore_macro_plan(entry, &header, chunk_size);
            println!(
                "restoring slot {} macro {} ({} steps)...",
                slot.index(),
                index.index(),
                plan.steps.len()
            );
            if let Err(error) = session.write_macro(&backup, slot, index, &plan).await {
                eprintln!(
                    "restore stopped at slot {} macro {}: {error}",
                    slot.index(),
                    index.index()
                );
                report_restore_recovery(pre_restore_complete, pre_dir);
                return Err(error);
            }
            restored += 1;
        }
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
        if let Err(error) = session.write_macro_header(&backup, slot).await {
            eprintln!("restore header stopped at slot {}: {error}", slot.index());
            report_restore_recovery(pre_restore_complete, pre_dir);
            return Err(error);
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

/// Read the configuration and all 3 slots × 4 macros into `dir`.
async fn macro_backup(options: &BleOptions, dir: &Path) -> Result<(), Error> {
    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let result = macro_backup_session(&session, dir).await;
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

async fn monitor(options: &BleOptions, count: Option<u64>) -> Result<(), Error> {
    if count == Some(0) {
        return Ok(());
    }

    let transport = BleTransport::connect(options).await?;
    let session = Session::new(transport);
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let result = monitor_connection(&session, Duration::from_millis(20), count, shutdown).await;
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;
    result.map(|_| ())
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
    best_effort_disconnect(session.transport(), CLEANUP_TIMEOUT).await;

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
async fn start_session_keepalive<T: Transport + 'static>(
    session: Arc<AsyncMutex<Session<T>>>,
    interval: Duration,
) -> Result<SessionKeepalive, Error> {
    start_session_keepalive_with_config(session, SessionKeepaliveConfig::with_interval(interval))
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
        "keepalive degraded (consecutive failures {}): {}. The controller most likely \
         left BLE configuration mode; this session does not auto-reconnect. Press \
         Profile on the controller to re-enter `80ND` and start a new `eightbitdo session`",
        state.consecutive_failures.load(Ordering::SeqCst),
        state
            .last_error()
            .unwrap_or_else(|| "unknown error".to_string())
    )))
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

async fn execute_session_command<T: Transport + 'static>(
    session: &Arc<AsyncMutex<Session<T>>>,
    keepalive: &Arc<SessionKeepaliveState>,
    started: Instant,
    mtu: u16,
    interval: Duration,
    monitor: &mut Option<SessionMonitor>,
    command: SessionCommand,
) -> Result<SessionOutcome, Error> {
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
            print_session_help();
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
    R: tokio::io::AsyncBufRead + Unpin,
{
    read_session_line_inner(reader, state)
}

async fn read_session_line_inner<R>(
    reader: &mut R,
    state: &mut LineState,
) -> std::io::Result<LineRead>
where
    R: tokio::io::AsyncBufRead + Unpin,
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
    reader: &mut R,
    ctrl_c: F,
    interactive: bool,
) -> Result<(), Error>
where
    T: Transport + 'static,
    R: tokio::io::AsyncBufRead + Unpin,
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
                // instead. `Session` records abandoned reads through
                // `ResyncGuard` / `InFlight`; the next read calls
                // `prepare_read` and drains only when a read (not an input
                // poll) may have left a response behind. A protocol error from
                // an input poll marks the session explicitly.
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
                    LineRead::Eof => break,
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
                let outcome = tokio::select! {
                    biased;
                    _ = &mut ctrl_c => {
                        // The command future is dropped, not aborted. Dropping
                        // a `Session` read marks it for resync via
                        // `ResyncGuard` (an abandoned input poll does not; a
                        // protocol error from one is marked by `fail()`), so
                        // the next read drains only when a response could
                        // still be in flight. There is no caller-side mark to
                        // clear; `Session::InFlight` makes that decision.
                        break;
                    }
                    outcome = execute_session_command(
                        session,
                        keepalive,
                        started,
                        mtu,
                        interval,
                        &mut monitor,
                        command,
                    ) => outcome,
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
            if let Ok(guard) = tokio::time::timeout(CLEANUP_TIMEOUT, session.lock()).await {
                best_effort_disconnect(guard.transport(), CLEANUP_TIMEOUT).await;
            }
            eprintln!("session error: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };
    let state = Arc::clone(&keepalive.state);
    let started = Instant::now();
    let result = session_repl_loop(&session, &state, started, mtu, interval).await;
    shutdown_session(&session, keepalive).await;
    let result = finalize_session_result(result, &state);

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
    use eightbitdo_core::MockTransport;
    use eightbitdo_protocol::commands;
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
        fail_recv: Arc<AtomicBool>,
        disconnect_recv: Arc<AtomicBool>,
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
        mock.push_response(&CustomHeader::new(commands::cmd::ERASE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::WRITE_MACRO, 0, 0, 0), &[]);
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
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
        mock.push_response(&CustomHeader::new(commands::cmd::ERASE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::WRITE_MACRO, 0, 0, 0), &[]);
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
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
    fn restore_plan_does_not_reencode_name() {
        let dir = scratch_dir("macro-restore-name");
        let (_, backup) = backup_fixture(&dir);
        let entry = backup.get(Slot::S1, MacroIndex::M1).unwrap();
        let header = backup.snapshot().config.macro_slot(Slot::S1);
        let plan = restore_macro_plan(entry, &header, Some(32));
        assert_eq!(plan.name, None);
        assert_eq!(plan.chunk_size, Some(32));
        assert_eq!(plan.cycles_num, Some(entry.record().cycles_num));
        assert_eq!(plan.interval_ms, Some(entry.record().interval_ms));
        let default = restore_macro_plan(entry, &header, None);
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
    #[tokio::test]
    async fn macro_write_session_backs_up_all_macros_then_writes_and_verifies() {
        let root = scratch_dir("macro-write-success");
        let fixture_dir = root.join("fixture");
        let (snapshot, backup) = backup_fixture(&fixture_dir);
        let backup_dir = root.join("prewrite");

        let mock = MockTransport::with_mtu(200);
        push_config_responses(&mock, &snapshot.config);
        push_backup_macro_read(&mock, &backup, Slot::S1, MacroIndex::M1);
        mock.push_response(&CustomHeader::new(commands::cmd::ERASE_MACRO, 0, 0, 0), &[]);
        mock.push_response(&CustomHeader::new(commands::cmd::WRITE_MACRO, 0, 0, 0), &[]);
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
        mock.push_response(
            &CustomHeader::new(commands::cmd::WRITE_CONFIG, 0, 0, 0),
            &[],
        );
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
        assert_eq!(count(commands::cmd::SAVE), 1);

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
