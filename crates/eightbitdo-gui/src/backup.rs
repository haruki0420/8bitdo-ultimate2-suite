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
use eightbitdo_core::ConfigSnapshot;
use serde_json::json;

use crate::model::{format_u16_hex, format_u32_hex, ConfigView};
use crate::util::sha256_hex;

/// JSON schema marker written into every summary.
pub const BACKUP_FORMAT: &str = "eightbitdo-config-backup-v1";

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

    let base = format!("config-{}", Utc::now().format("%Y%m%d-%H%M%S"));
    write_backup_with_base(dir, &base, snapshot, view, warnings)
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
