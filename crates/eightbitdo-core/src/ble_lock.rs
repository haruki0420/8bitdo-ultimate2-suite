//! Single-owner advisory lock for the BLE adapter.
//!
//! btleplug talks to one system BLE stack per process. Two concurrent
//! operations (a scan in one shell while a resident `session` holds the link,
//! or a reconnect racing the previous link's teardown) make the controller and
//! the adapter return `In Progress`, and the losing side can leave a session
//! degraded without a usable link. Every operation that scans, connects or
//! holds a connection therefore takes this lock first.
//!
//! The lock is an ordinary [`flock`](https://man7.org/linux/man-pages/man2/flock.2.html)
//! on a file, so it is released by the kernel when the process exits (including
//! `SIGKILL`) and it also excludes a `flock(1)` invocation started by hand.
//! The file is never deleted: unlinking it would let a second process create a
//! new inode and take an "unlocked" lock while the first holder still owns the
//! old one.
//!
//! ## Path (per-user, private directory)
//!
//! 1. [`BLE_LOCK_PATH_ENV`] (`EIGHTBITDO_BLE_LOCK`) when set, for tests and
//!    wrappers;
//! 2. `$XDG_RUNTIME_DIR/8bitdo-ble.lock` when `XDG_RUNTIME_DIR` is absolute;
//! 3. `$XDG_CACHE_HOME/8bitdo/8bitdo-ble.lock` or
//!    `$HOME/.cache/8bitdo/8bitdo-ble.lock` when those are absolute/non-empty;
//! 4. `<platform temp dir>/8bitdo-ble-<euid>/8bitdo-ble.lock` as a last resort
//!    (`TMPDIR` is honoured through [`std::env::temp_dir`]).
//!
//! No path is placed directly in a shared `/tmp`: the lock lives in a per-user
//! directory that is created with mode `0700` when missing. Every component of
//! the fallback is per-user, so two users do **not** exclude each other (the
//! lock is per-user, not system-wide).
//!
//! ## Validation
//!
//! The file is opened with `O_NOFOLLOW` on Unix and validated as a regular
//! file owned by the effective user before it is truncated or locked. A
//! symlink, directory, FIFO or another user's file is refused with
//! [`crate::Error::BleLockInvalid`], so a pre-created path in a shared
//! directory cannot be used to truncate an arbitrary file. The open file
//! descriptor is validated as well (not just the path), so a race between the
//! pre-check and the open cannot redirect the truncate/set-permissions calls.
//!
//! ## Scope
//!
//! The lock is advisory: it stops cooperating processes (this CLI, the future
//! GUI, `scripts/session-loop.sh` users) from fighting over the adapter. It is
//! not a security boundary and it does not stop an unrelated program from
//! scanning.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::Error;

/// Environment variable that overrides the lock file path.
pub const BLE_LOCK_PATH_ENV: &str = "EIGHTBITDO_BLE_LOCK";

/// Lock file name used below the runtime/cache directory.
pub const BLE_LOCK_FILE_NAME: &str = "8bitdo-ble.lock";

/// Per-user subdirectory below `XDG_CACHE_HOME` / `$HOME/.cache`.
const CACHE_SUBDIR: &str = "8bitdo";

/// A held single-owner BLE lock.
///
/// The lock is released when this value is dropped (or when the process
/// exits). Keep it alive for as long as the BLE scan or connection lasts;
/// [`crate::BleTransport`] does that for a connection, and
/// [`crate::scan_devices`] for a scan.
#[derive(Debug)]
pub struct BleLock {
    file: File,
    path: PathBuf,
}

impl BleLock {
    /// Acquire the lock at the default path ([`default_ble_lock_path`]).
    ///
    /// # Errors
    ///
    /// Returns [`Error::BleBusy`] when another process holds the lock,
    /// [`Error::BleLockInvalid`] when the path is a symlink, is not a regular
    /// file, or is owned by another user, and [`Error::Io`] when the lock file
    /// cannot be created or locked.
    pub fn acquire() -> Result<Self, Error> {
        Self::acquire_at(default_ble_lock_path())
    }

    /// Acquire the lock at an explicit path.
    ///
    /// Tests use this with a temporary path so they never contend with the
    /// developer's real session; tools can use it to share a lock with the
    /// CLI by setting [`BLE_LOCK_PATH_ENV`] to the same path. The same symlink,
    /// regular-file and owner checks apply to an explicit path.
    ///
    /// # Errors
    ///
    /// Same as [`BleLock::acquire`].
    pub fn acquire_at(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        precheck_lock_path(&path)?;
        ensure_lock_directory(&path)?;
        let file = open_lock_file(&path)?;
        validate_lock_file(&path, &file)?;

        // `try_lock_exclusive` rather than blocking: the caller must tell the
        // operator that another process owns the adapter instead of hanging
        // behind a resident session.
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(Error::BleBusy { path });
            }
            Err(error) => return Err(Error::Io(error)),
        }

        // The descriptor is a regular file owned by us, so tightening the mode
        // and truncating/writing the holder line cannot touch another file
        // through this path.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        let mut holder = &file;
        let _ = holder.set_len(0);
        let _ = writeln!(holder, "pid={} cmd=eightbitdo", std::process::id());

        Ok(Self { file, path })
    }

    /// Path of the lock file held by this guard.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The open file descriptor behind the lock.
    ///
    /// Kept for diagnostics; dropping the guard is what releases the lock.
    #[must_use]
    pub fn file(&self) -> &File {
        &self.file
    }
}

/// Default lock path: per-user runtime/cache directory, never a shared `/tmp`
/// file. See the module documentation for the full resolution order.
///
/// [`BLE_LOCK_PATH_ENV`], when set to a non-empty value, wins.
#[must_use]
pub fn default_ble_lock_path() -> PathBuf {
    resolve_lock_path(
        std::env::var_os(BLE_LOCK_PATH_ENV),
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("XDG_CACHE_HOME"),
        std::env::var_os("HOME"),
        std::env::temp_dir(),
        current_euid(),
    )
}

/// Pure path-resolution helper, separated so every fallback branch is testable
/// without mutating the process environment.
fn resolve_lock_path(
    override_path: Option<std::ffi::OsString>,
    runtime_dir: Option<std::ffi::OsString>,
    cache_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    temp_dir: PathBuf,
    euid: u32,
) -> PathBuf {
    if let Some(path) = override_path.filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(directory) = runtime_dir
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return directory.join(BLE_LOCK_FILE_NAME);
    }
    if let Some(directory) = cache_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return directory.join(CACHE_SUBDIR).join(BLE_LOCK_FILE_NAME);
    }
    if let Some(directory) = home
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return directory
            .join(".cache")
            .join(CACHE_SUBDIR)
            .join(BLE_LOCK_FILE_NAME);
    }
    // Last resort when HOME/XDG are unavailable (containers, CI sandboxes).
    // Keep it per-user: a shared directory plus a fixed file name would let
    // another user pre-create/symlink the path.
    temp_dir
        .join(format!("8bitdo-ble-{euid}"))
        .join(BLE_LOCK_FILE_NAME)
}

#[cfg(unix)]
fn current_euid() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[cfg(not(unix))]
fn current_euid() -> u32 {
    0
}

/// Reject an existing symlink or non-regular file before opening it.
///
/// This is the cross-platform check; Unix also opens with `O_NOFOLLOW` so a
/// swap between this check and the open cannot redirect truncation.
fn precheck_lock_path(path: &Path) -> Result<(), Error> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::BleLockInvalid {
                path: path.to_path_buf(),
                reason: "path is a symbolic link; refusing to follow it".to_string(),
            });
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(Error::BleLockInvalid {
                path: path.to_path_buf(),
                reason: "path is not a regular file".to_string(),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::Io(error)),
    }
    Ok(())
}

/// Create the lock's parent directory with mode `0700` when it is missing.
///
/// Existing directories (including `/tmp` for explicit test paths) are never
/// chmod-ed: only directories this function creates are made private.
fn ensure_lock_directory(path: &Path) -> Result<(), Error> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    if parent.is_dir() {
        return Ok(());
    }
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(parent)?;
    Ok(())
}

fn open_lock_file(path: &Path) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use rustix::fs::OFlags;
        use std::os::unix::fs::OpenOptionsExt as _;
        // O_NOFOLLOW closes the precheck/open race; O_NONBLOCK keeps a
        // pre-created FIFO from blocking the open before the metadata check.
        options
            .mode(0o600)
            .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32);
    }
    options
        .open(path)
        .map_err(|error| lock_open_error(path, &error))
}

/// Turn the two security-relevant open failures into actionable errors.
fn lock_open_error(path: &Path, error: &std::io::Error) -> Error {
    #[cfg(unix)]
    {
        use rustix::io::Errno;
        if matches!(error.raw_os_error(), Some(raw) if raw == Errno::LOOP.raw_os_error()) {
            return Error::BleLockInvalid {
                path: path.to_path_buf(),
                reason: "path is a symbolic link; refusing to follow it".to_string(),
            };
        }
        if matches!(error.raw_os_error(), Some(raw) if raw == Errno::ISDIR.raw_os_error()) {
            return Error::BleLockInvalid {
                path: path.to_path_buf(),
                reason: "path is a directory, not a lock file".to_string(),
            };
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Error::Io(std::io::Error::new(error.kind(), error.to_string()))
}

/// Validate the *opened descriptor*: a regular file owned by the effective
/// user.
fn validate_lock_file(path: &Path, file: &File) -> Result<(), Error> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::BleLockInvalid {
            path: path.to_path_buf(),
            reason: "path is not a regular file".to_string(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        validate_lock_owner(path, metadata.uid(), current_euid())?;
    }
    Ok(())
}

/// Owner check split out so a mismatch is testable without root/chown.
///
/// Unix-only: Windows has no portable uid check, so it relies on the
/// `symlink_metadata` precheck (regular file, not a symlink) instead.
#[cfg(unix)]
fn validate_lock_owner(path: &Path, owner: u32, euid: u32) -> Result<(), Error> {
    if owner == euid {
        return Ok(());
    }
    Err(Error::BleLockInvalid {
        path: path.to_path_buf(),
        reason: format!("lock file is owned by uid {owner}, not the current user (uid {euid})"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temp_lock_path(label: &str) -> PathBuf {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "eightbitdo-lock-test-{label}-{}-{sequence}.lock",
            std::process::id()
        ))
    }

    #[test]
    fn second_acquire_is_rejected_with_ble_busy() {
        let path = temp_lock_path("busy");
        let first = BleLock::acquire_at(&path).expect("first acquire");
        let error = BleLock::acquire_at(&path).expect_err("second acquire must fail");
        assert!(
            matches!(error, Error::BleBusy { path: ref held } if held == &path),
            "expected BleBusy, got {error:?}"
        );
        drop(first);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn dropping_the_guard_releases_the_lock() {
        let path = temp_lock_path("release");
        {
            let _guard = BleLock::acquire_at(&path).expect("first acquire");
        }
        let _guard = BleLock::acquire_at(&path).expect("lock must be released on drop");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_external_flock_holder_is_reported_as_busy() {
        // Take the flock with `fs2` directly, the way `flock(1)` would, and
        // verify the guard refuses instead of blocking.
        let path = temp_lock_path("external");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .expect("open lock file");
        FileExt::try_lock_exclusive(&file).expect("external flock");
        let error = BleLock::acquire_at(&path).expect_err("external holder must win");
        assert!(matches!(error, Error::BleBusy { .. }), "got {error:?}");
        FileExt::unlock(&file).expect("unlock");
        let _guard = BleLock::acquire_at(&path).expect("lock must be free after unlock");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_path_resolution_prefers_the_override_then_runtime_then_cache() {
        let name = BLE_LOCK_FILE_NAME;
        assert_eq!(
            resolve_lock_path(
                Some("/custom/lock".into()),
                Some("/run/user/1000".into()),
                Some("/cache".into()),
                Some("/home/user".into()),
                PathBuf::from("/tmp"),
                1000,
            ),
            PathBuf::from("/custom/lock")
        );
        assert_eq!(
            resolve_lock_path(
                None,
                Some("/run/user/1000".into()),
                Some("/cache".into()),
                Some("/home/user".into()),
                PathBuf::from("/tmp"),
                1000,
            ),
            PathBuf::from("/run/user/1000").join(name)
        );
        assert_eq!(
            resolve_lock_path(
                None,
                Some("relative/dir".into()),
                Some("/cache".into()),
                Some("/home/user".into()),
                PathBuf::from("/tmp"),
                1000,
            ),
            PathBuf::from("/cache").join(CACHE_SUBDIR).join(name)
        );
        assert_eq!(
            resolve_lock_path(
                None,
                None,
                Some("relative".into()),
                Some("/home/user".into()),
                PathBuf::from("/tmp"),
                1000,
            ),
            PathBuf::from("/home/user")
                .join(".cache")
                .join(CACHE_SUBDIR)
                .join(name)
        );
    }

    #[test]
    fn default_path_falls_back_to_a_per_user_temp_directory() {
        assert_eq!(
            resolve_lock_path(None, None, None, None, PathBuf::from("/var/tmp"), 1000,),
            PathBuf::from("/var/tmp")
                .join("8bitdo-ble-1000")
                .join(BLE_LOCK_FILE_NAME)
        );
    }

    #[test]
    fn explicit_override_wins_even_when_xdg_is_set() {
        assert_eq!(
            resolve_lock_path(
                Some("/explicit.lock".into()),
                Some("/run/user/1000".into()),
                None,
                None,
                PathBuf::from("/tmp"),
                1000,
            ),
            PathBuf::from("/explicit.lock")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_lock_path_is_rejected_and_the_target_is_untouched() {
        let target = temp_lock_path("symlink-target");
        std::fs::write(&target, b"do not truncate me").expect("write target");
        let link = temp_lock_path("symlink-link");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let error = BleLock::acquire_at(&link).expect_err("symlink must be refused");
        assert!(
            matches!(error, Error::BleLockInvalid { path: ref held, ref reason }
                if held == &link && reason.contains("symbolic link")),
            "expected BleLockInvalid for the symlink, got {error:?}"
        );
        assert_eq!(
            std::fs::read(&target).expect("read target"),
            b"do not truncate me",
            "the symlink target must not be truncated"
        );
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_file(&target);
    }

    #[cfg(unix)]
    #[test]
    fn directory_lock_path_is_rejected() {
        let path = temp_lock_path("directory");
        std::fs::create_dir(&path).expect("create directory");
        let error = BleLock::acquire_at(&path).expect_err("directory must be refused");
        assert!(
            matches!(error, Error::BleLockInvalid { path: ref held, .. } if held == &path),
            "expected BleLockInvalid for a directory, got {error:?}"
        );
        let _ = std::fs::remove_dir(&path);
    }

    #[cfg(unix)]
    #[test]
    fn fifo_lock_path_is_rejected_without_blocking() {
        let path = temp_lock_path("fifo");
        let created = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .is_ok_and(|status| status.success());
        if !created {
            // `mkfifo` is absent on this test host; the directory/non-regular
            // coverage above still exercises the reject path.
            return;
        }
        let error = BleLock::acquire_at(&path).expect_err("FIFO must be refused");
        assert!(
            matches!(error, Error::BleLockInvalid { path: ref held, .. } if held == &path),
            "expected BleLockInvalid for a FIFO, got {error:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn another_users_file_is_rejected() {
        let path = temp_lock_path("owner");
        let euid = current_euid();
        let error = validate_lock_owner(&path, euid.wrapping_add(1), euid)
            .expect_err("foreign owner must be refused");
        assert!(
            matches!(error, Error::BleLockInvalid { path: ref held, ref reason }
                if held == &path && reason.contains("owned by uid")),
            "expected an owner mismatch, got {error:?}"
        );
        validate_lock_owner(&path, euid, euid).expect("the current user's file is accepted");
    }

    #[cfg(unix)]
    #[test]
    fn lock_file_is_private_and_records_the_holder_pid() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = temp_lock_path("pid");
        let guard = BleLock::acquire_at(&path).expect("acquire");
        let metadata = std::fs::metadata(guard.path()).expect("lock metadata");
        assert_eq!(
            metadata.permissions().mode() & 0o777,
            0o600,
            "lock file must be private"
        );
        let contents = std::fs::read_to_string(guard.path()).expect("read lock file");
        assert!(
            contents.contains(&format!("pid={}", std::process::id())),
            "lock file must name its holder: {contents:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
