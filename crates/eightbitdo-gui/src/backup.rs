//! Configuration backup writing.
//!
//! The GUI writes the same two artifacts as `eightbitdo backup`: the raw
//! 2768-byte block and a JSON summary with the SHA-256 of the raw bytes. The
//! CLI requires explicit output paths; the GUI defaults to a per-user data
//! directory (`<data dir>/8bitdo-backups`).
//!
//! Every file is created with `create_new` and mode `0600` on Unix. A backup
//! never replaces an existing file: a name collision moves to `-1`, `-2`, ...
//! instead. The raw file is written first; if the JSON file cannot be created,
//! the new raw file is removed so a half backup is not left behind.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use eightbitdo_core::macro_ops::{self, MacroBackup};
use eightbitdo_core::ConfigSnapshot;
use eightbitdo_protocol::config_bt2::{MACROS_PER_SLOT, SLOT_COUNT};
use eightbitdo_protocol::macro_data::MacroStep;
use serde::Serialize;
use serde_json::json;

use crate::model::{format_u16_hex, format_u32_hex, ConfigView};
use crate::util::sha256_hex;

/// JSON schema marker written into every summary.
pub const BACKUP_FORMAT: &str = "eightbitdo-config-backup-v1";

/// Manifest format version of a GUI macro backup.
///
/// This matches the CLI's `macro backup` manifest (`format: 1`), so a
/// pre-write backup directory written by the GUI can be restored with
/// `eightbitdo macro restore --dir <dir>`.
pub const MACRO_BACKUP_FORMAT: u32 = 1;

/// File-name prefix of the mandatory configuration backup written just before
/// a configuration flash commit.
pub const PREWRITE_CONFIG_PREFIX: &str = "prewrite-config";

/// File-name prefix of the directory holding the mandatory macro backup
/// written just before a macro flash commit.
pub const PREWRITE_MACRO_PREFIX: &str = "macro-prewrite";

/// Maximum number of `-N` suffixes tried before giving up.
const MAX_NAME_ATTEMPTS: u32 = 100;

/// A failed backup write.
#[derive(Debug)]
pub enum BackupError {
    /// Creating or writing a file failed.
    Io(io::Error),
    /// Serializing the JSON summary failed.
    Json(serde_json::Error),
}

impl fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "backup I/O error: {error}"),
            Self::Json(error) => write!(formatter, "backup JSON error: {error}"),
        }
    }
}

impl std::error::Error for BackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
        }
    }
}

impl From<io::Error> for BackupError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for BackupError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Where a backup landed and what it contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupReport {
    /// Absolute or platform-native path of the 2768-byte file.
    pub raw_path: PathBuf,
    /// Path of the JSON summary.
    pub json_path: PathBuf,
    /// SHA-256 of the raw block.
    pub sha256: String,
    /// Raw block size in bytes.
    pub bytes: usize,
    /// Non-fatal warnings, for example a failed directory permission change.
    pub warnings: Vec<String>,
}

/// Where a macro backup directory landed and what it contains.
///
/// The layout is the CLI `macro backup` layout (`config.bin`, `config.json`,
/// `macros.json`, per-macro `slotS-macroM.bin`), so the directory can be used
/// directly with `eightbitdo macro restore --dir <dir>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroBackupReport {
    /// Backup directory.
    pub dir: PathBuf,
    /// Path of the 2768-byte `config.bin`.
    pub config_path: PathBuf,
    /// SHA-256 of `config.bin`.
    pub config_sha256: String,
    /// Path of the `macros.json` manifest.
    pub manifest_path: PathBuf,
    /// SHA-256 of `macros.json`.
    pub manifest_sha256: String,
    /// Stored macro entries with step data.
    pub macro_count: usize,
    /// Empty / skipped macro headers (no step data read).
    pub skipped_count: usize,
    /// Total bytes of the raw files.
    pub bytes: usize,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
}

impl MacroBackupReport {
    /// A stable reference for UI display and failure messages.
    #[must_use]
    pub fn reference(&self) -> BackupRef {
        BackupRef {
            config_path: self.config_path.clone(),
            config_sha256: self.config_sha256.clone(),
            macro_dir: Some(self.dir.clone()),
            macro_manifest_sha256: Some(self.manifest_sha256.clone()),
        }
    }
}

/// A durable pointer to the backup written before a device write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRef {
    /// Path of the pre-write `config.bin`.
    pub config_path: PathBuf,
    /// SHA-256 of that file.
    pub config_sha256: String,
    /// Macro backup directory, when one was written.
    pub macro_dir: Option<PathBuf>,
    /// SHA-256 of the macro backup manifest, when one was written.
    pub macro_manifest_sha256: Option<String>,
}

impl From<&BackupReport> for BackupRef {
    fn from(report: &BackupReport) -> Self {
        Self {
            config_path: report.raw_path.clone(),
            config_sha256: report.sha256.clone(),
            macro_dir: None,
            macro_manifest_sha256: None,
        }
    }
}

impl From<&MacroBackupReport> for BackupRef {
    fn from(report: &MacroBackupReport) -> Self {
        report.reference()
    }
}

/// Default backup directory: the platform data directory plus
/// `8bitdo-backups`.
///
/// On Linux this is `$XDG_DATA_HOME/8bitdo-backups`, falling back to
/// `~/.local/share/8bitdo-backups`. Other platforms use the native data
/// directory reported by `dirs`.
#[must_use]
pub fn default_backup_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("8bitdo-backups")
}

/// Write the raw block and JSON summary into `dir`.
///
/// # Errors
///
/// Returns [`BackupError::Io`] when the directory or a file cannot be created,
/// and [`BackupError::Json`] when the summary cannot be serialized.
pub fn write_backup(
    dir: &Path,
    snapshot: &ConfigSnapshot,
    view: &ConfigView,
) -> Result<BackupReport, BackupError> {
    write_config_backup(dir, "config", snapshot, view)
}

/// Write the mandatory configuration backup taken immediately before a flash
/// commit.
///
/// The file names use the [`PREWRITE_CONFIG_PREFIX`] prefix so a pre-write
/// backup is distinguishable from a user-requested `config-*` backup. The raw
/// file is the exact 2768-byte block the plan was prepared against.
///
/// # Errors
///
/// Same as [`write_backup`].
pub fn write_prewrite_backup(
    dir: &Path,
    snapshot: &ConfigSnapshot,
    view: &ConfigView,
) -> Result<BackupReport, BackupError> {
    write_config_backup(dir, PREWRITE_CONFIG_PREFIX, snapshot, view)
}

fn write_config_backup(
    dir: &Path,
    prefix: &str,
    snapshot: &ConfigSnapshot,
    view: &ConfigView,
) -> Result<BackupReport, BackupError> {
    let warnings = prepare_backup_directory(dir)?;
    let base = format!("{prefix}-{}", Utc::now().format("%Y%m%d-%H%M%S"));
    write_backup_with_base(dir, &base, snapshot, view, warnings)
}

/// Create the backup directory and return non-fatal permission warnings.
fn prepare_backup_directory(dir: &Path) -> Result<Vec<String>, BackupError> {
    // Reject a symlinked final component before `create_dir_all` follows it.
    // The CLI's Phase 2a backup path validation rejects symlinks for the same
    // reason: a local link must not redirect personal configuration data or
    // cause the process to chmod an unrelated directory.
    if let Ok(metadata) = fs::symlink_metadata(dir) {
        if metadata.file_type().is_symlink() {
            return Err(BackupError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("backup directory must not be a symlink: {}", dir.display()),
            )));
        }
    }
    fs::create_dir_all(dir)?;
    let mut warnings = Vec::new();
    ensure_private_directory(dir, &mut warnings);
    Ok(warnings)
}

fn ensure_private_directory(dir: &Path, warnings: &mut Vec<String>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if let Err(error) = fs::set_permissions(dir, fs::Permissions::from_mode(0o700)) {
            warnings.push(format!(
                "cannot set backup directory permissions to 0700: {error}"
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = (dir, warnings);
}

fn write_backup_with_base(
    dir: &Path,
    base: &str,
    snapshot: &ConfigSnapshot,
    view: &ConfigView,
    warnings: Vec<String>,
) -> Result<BackupReport, BackupError> {
    let sha256 = sha256_hex(snapshot.config.as_bytes());
    let summary = summary_json(view, &sha256);
    let encoded = format!("{}\n", serde_json::to_string_pretty(&summary)?);

    for attempt in 0..MAX_NAME_ATTEMPTS {
        let stem = if attempt == 0 {
            base.to_string()
        } else {
            format!("{base}-{attempt}")
        };
        let raw_path = dir.join(format!("{stem}.bin"));
        let json_path = dir.join(format!("{stem}.json"));
        if raw_path.exists() || json_path.exists() {
            continue;
        }

        match write_new_private_file(&raw_path, snapshot.config.as_bytes()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(BackupError::Io(error)),
        }

        if let Err(error) = write_new_private_file(&json_path, encoded.as_bytes()) {
            let _ = fs::remove_file(&raw_path);
            if error.kind() == io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(BackupError::Io(error));
        }

        return Ok(BackupReport {
            raw_path,
            json_path,
            sha256,
            bytes: snapshot.config.as_bytes().len(),
            warnings,
        });
    }

    Err(BackupError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "could not find an unused backup file name after {MAX_NAME_ATTEMPTS} attempts in {}",
            dir.display()
        ),
    )))
}

fn write_new_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // `mode(0o600)` is filtered by the umask; set the exact mode after
        // creation. Best effort: the file can only be narrower than 0600 if
        // this fails, never wider.
        let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
    }
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}

/// One manifest entry, matching the CLI `macro backup` JSON schema.
///
/// The CLI reads `macros.json` (or `macros.toml`), validates the SHA-256 of
/// `config.bin` and every per-macro raw file, and rebuilds a `MacroBackup`.
#[derive(Debug, Serialize)]
struct MacroManifestEntry {
    slot: u8,
    profile: u8,
    index: u8,
    id: String,
    name: String,
    skipped: bool,
    skip_reason: Option<String>,
    max_steps: u16,
    key_map: u32,
    key_map_name: Option<String>,
    cycles_num: u32,
    interval_ms: u32,
    raw_file: Option<String>,
    raw_sha256: Option<String>,
    response_offsets: Vec<u32>,
    steps: Vec<MacroStep>,
}

/// The CLI-compatible macro backup manifest (`format: 1`).
#[derive(Debug, Serialize)]
struct MacroManifest {
    format: u32,
    config_file: String,
    config_sha256: String,
    gamepad_mode: u16,
    slot_count: usize,
    macros_per_slot: usize,
    macros: Vec<MacroManifestEntry>,
}

/// Write a complete, CLI-compatible macro backup directory.
///
/// This is the mandatory pre-write backup for a macro flash commit. It reads
/// nothing from the device: the caller passes the `MacroBackup` it already
/// read (which the core write path also requires). The directory contains:
///
/// ```text
/// <backup dir>/macro-prewrite-YYYYMMDD-HHMMSS/
///   config.bin                 raw 2768-byte configuration
///   config.json                the same summary as a config backup
///   macros.json                manifest with SHA-256 for every raw file
///   slotS-macroM.bin           raw 10-byte steps for every non-empty macro
/// ```
///
/// A partly written directory is removed again, so a failed backup never
/// looks complete.
///
/// # Errors
///
/// Returns [`BackupError::Io`] when the directory or a file cannot be created,
/// and [`BackupError::Json`] when the manifest cannot be serialized.
pub fn write_macro_backup(
    dir: &Path,
    snapshot: &ConfigSnapshot,
    backup: &MacroBackup,
) -> Result<MacroBackupReport, BackupError> {
    let warnings = prepare_backup_directory(dir)?;
    let base = format!(
        "{PREWRITE_MACRO_PREFIX}-{}",
        Utc::now().format("%Y%m%d-%H%M%S")
    );

    for attempt in 0..MAX_NAME_ATTEMPTS {
        let name = if attempt == 0 {
            base.clone()
        } else {
            format!("{base}-{attempt}")
        };
        let target = dir.join(name);
        match fs::create_dir(&target) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(BackupError::Io(error)),
        }
        let mut target_warnings = warnings.clone();
        ensure_private_directory(&target, &mut target_warnings);
        return match write_macro_backup_dir(&target, snapshot, backup, target_warnings) {
            Ok(report) => Ok(report),
            Err(error) => {
                let _ = fs::remove_dir_all(&target);
                Err(error)
            }
        };
    }

    Err(BackupError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "could not find an unused macro backup directory name after {MAX_NAME_ATTEMPTS} \
             attempts in {}",
            dir.display()
        ),
    )))
}

fn write_macro_backup_dir(
    dir: &Path,
    snapshot: &ConfigSnapshot,
    backup: &MacroBackup,
    warnings: Vec<String>,
) -> Result<MacroBackupReport, BackupError> {
    let view = ConfigView::from_snapshot(snapshot);
    let config_sha256 = sha256_hex(snapshot.config.as_bytes());
    let config_path = dir.join("config.bin");
    write_new_private_file(&config_path, snapshot.config.as_bytes())?;

    let summary = summary_json(&view, &config_sha256);
    let summary_json = format!("{}\n", serde_json::to_string_pretty(&summary)?);
    write_new_private_file(&dir.join("config.json"), summary_json.as_bytes())?;

    let mut bytes = snapshot.config.as_bytes().len();
    let mut macro_count = 0usize;
    let mut skipped_count = 0usize;
    let mut entries = Vec::with_capacity(SLOT_COUNT * MACROS_PER_SLOT);
    for entry in backup.entries() {
        let slot = entry.slot();
        let index = entry.index();
        let record = entry.record();
        let skipped = macro_ops::is_skipped_steps(record.max_steps);
        let (raw_file, raw_sha256) = if skipped {
            skipped_count += 1;
            (None, None)
        } else {
            let file_name = format!("slot{}-macro{}.bin", slot.index(), index.index());
            write_new_private_file(&dir.join(&file_name), entry.raw())?;
            bytes += entry.raw().len();
            macro_count += 1;
            (Some(file_name), Some(sha256_hex(entry.raw())))
        };
        entries.push(MacroManifestEntry {
            slot: slot.index() as u8,
            profile: slot.number(),
            index: index.index() as u8,
            id: format!("M{}", index.index() + 1),
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

    let document = MacroManifest {
        format: MACRO_BACKUP_FORMAT,
        config_file: "config.bin".to_string(),
        config_sha256: config_sha256.clone(),
        gamepad_mode: snapshot.config.gamepad_mode_raw(),
        slot_count: SLOT_COUNT,
        macros_per_slot: MACROS_PER_SLOT,
        macros: entries,
    };
    let manifest = format!("{}\n", serde_json::to_string_pretty(&document)?);
    let manifest_path = dir.join("macros.json");
    write_new_private_file(&manifest_path, manifest.as_bytes())?;

    Ok(MacroBackupReport {
        dir: dir.to_path_buf(),
        config_path,
        config_sha256,
        manifest_path,
        manifest_sha256: sha256_hex(manifest.as_bytes()),
        macro_count,
        skipped_count,
        bytes,
        warnings,
    })
}

fn macro_skip_reason(max_steps: u16) -> Option<&'static str> {
    match max_steps {
        0 => Some("max_steps == 0 (empty macro)"),
        u16::MAX => Some("max_steps == 0xFFFF (unset marker candidate)"),
        _ => None,
    }
}

fn summary_json(view: &ConfigView, sha256: &str) -> serde_json::Value {
    json!({
        "format": BACKUP_FORMAT,
        "created_utc": Utc::now().to_rfc3339(),
        "size": view.size,
        "sha256": sha256,
        "cur_slot": view.cur_slot_raw,
        "cur_slot_label": view.cur_slot_label,
        "gamepad_mode": view.gamepad_mode_raw,
        "gamepad_mode_label": view.gamepad_mode_label,
        "slot_flags": view.slot_flags,
        "profile_names": view.profile_names,
        "crc_stored": view.stored_crc,
        "crc_in_place": view.computed_crc,
        "crc_in_place_note": "informational: the official in-place CRC includes the stored CRC field",
        "macros": view.macro_slots.iter().map(|slot| json!({
            "profile": slot.profile,
            "flag": slot.flag,
            "total_cnt": slot.total_cnt,
            "records": slot.records.iter().map(|record| json!({
                "number": record.number,
                "name": record.name,
                "gamepad_mode": record.gamepad_mode,
                "special_flag": record.special_flag,
                "max_steps": record.max_steps,
                "offset": record.offset,
                "offset_hex": format_u16_hex(record.offset),
                "key_map": record.key_map,
                "key_map_bits": record.key_map_bits,
                "key_map_bits_hex": format_u32_hex(record.key_map_bits),
                "cycles": record.cycles,
                "interval_ms": record.interval_ms,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use eightbitdo_protocol::config_bt2::Bt2Config;

    use super::*;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "eightbitdo-gui-test-{}-{unique}-{name}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample() -> (ConfigSnapshot, ConfigView) {
        let mut config = Bt2Config::default();
        config
            .set_profile_name(eightbitdo_protocol::config_bt2::Slot::S1, "Alpha")
            .unwrap();
        let stored_crc = config.crc16_stored() as u16;
        let mut check = config.clone();
        let computed_crc = check.recompute_crc();
        let snapshot = ConfigSnapshot {
            config,
            stored_crc,
            computed_crc,
            crc_valid: stored_crc == computed_crc,
        };
        let view = ConfigView::from_snapshot(&snapshot);
        (snapshot, view)
    }

    #[test]
    fn writes_raw_and_json_with_sha256() {
        let dir = TempDir::new("backup");
        let (snapshot, view) = sample();
        let report = write_backup_with_base(&dir.0, "config-test", &snapshot, &view, Vec::new())
            .expect("backup should be written");

        assert_eq!(report.bytes, 2768);
        assert_eq!(report.sha256, sha256_hex(snapshot.config.as_bytes()));
        assert_eq!(
            fs::read(&report.raw_path).unwrap(),
            snapshot.config.as_bytes()
        );
        assert_eq!(
            report.raw_path.file_name().unwrap().to_str().unwrap(),
            "config-test.bin"
        );
        assert_eq!(
            report.json_path.file_name().unwrap().to_str().unwrap(),
            "config-test.json"
        );

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&report.json_path).unwrap()).unwrap();
        assert_eq!(value["format"], BACKUP_FORMAT);
        assert_eq!(value["size"], 2768);
        assert_eq!(value["sha256"], report.sha256.as_str());
        assert_eq!(value["profile_names"][0], "Alpha");
        assert_eq!(value["cur_slot"], 0);
        assert_eq!(value["gamepad_mode"], 0);
        assert_eq!(value["macros"].as_array().unwrap().len(), 3);
        assert_eq!(value["macros"][0]["records"].as_array().unwrap().len(), 4);
        assert_eq!(value["macros"][0]["records"][0]["number"], 1);
    }

    #[test]
    fn existing_backup_names_are_never_replaced() {
        let dir = TempDir::new("collision");
        let (snapshot, view) = sample();

        let first =
            write_backup_with_base(&dir.0, "config-test", &snapshot, &view, Vec::new()).unwrap();
        let second =
            write_backup_with_base(&dir.0, "config-test", &snapshot, &view, Vec::new()).unwrap();

        assert_eq!(
            first.raw_path.file_name().unwrap().to_str().unwrap(),
            "config-test.bin"
        );
        assert_eq!(
            second.raw_path.file_name().unwrap().to_str().unwrap(),
            "config-test-1.bin"
        );
        assert_eq!(
            second.json_path.file_name().unwrap().to_str().unwrap(),
            "config-test-1.json"
        );
    }

    #[cfg(unix)]
    #[test]
    fn backup_files_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = TempDir::new("mode");
        let (snapshot, view) = sample();
        let report = write_backup(&dir.0, &snapshot, &view).unwrap();

        let raw_mode = fs::metadata(&report.raw_path).unwrap().permissions().mode() & 0o777;
        let json_mode = fs::metadata(&report.json_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let dir_mode = fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777;
        assert_eq!(raw_mode, 0o600);
        assert_eq!(json_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn default_directory_is_named_8bitdo_backups() {
        assert!(default_backup_dir().ends_with("8bitdo-backups"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_backup_directory_is_rejected_without_writing() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new("symlink");
        let target = dir.0.join("target");
        fs::create_dir(&target).unwrap();
        let link = dir.0.join("link");
        symlink(&target, &link).unwrap();
        let (snapshot, view) = sample();

        let error = write_backup(&link, &snapshot, &view).unwrap_err();
        assert!(matches!(error, BackupError::Io(_)));
        assert!(fs::read_dir(&target).unwrap().next().is_none());
    }

    #[test]
    fn a_file_blocking_the_directory_is_reported() {
        let dir = TempDir::new("not-a-dir");
        let target = dir.0.join("blocked");
        fs::write(&target, b"not a directory").unwrap();
        let (snapshot, view) = sample();

        let error = write_backup(&target, &snapshot, &view).unwrap_err();
        assert!(matches!(error, BackupError::Io(_)));
    }
}
