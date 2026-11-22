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
//! ## Path
//!
//! `$XDG_RUNTIME_DIR/8bitdo-ble.lock` when `XDG_RUNTIME_DIR` is set to an
//! absolute path, otherwise `/tmp/8bitdo-ble.lock` on Unix (the platform
//! temporary directory elsewhere). [`BLE_LOCK_PATH_ENV`] overrides both; it is
//! intended for tests and for wrappers that need a per-user path.
//!
//! ## Scope
//!
//! The lock is advisory: it stops cooperating processes (this CLI, the future
//! GUI, `scripts/session-loop.sh` users) from fighting over the adapter. It is
//! not a security boundary and it does not stop an unrelated program from
//! scanning.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::Error;

/// Environment variable that overrides the lock file path.
pub const BLE_LOCK_PATH_ENV: &str = "EIGHTBITDO_BLE_LOCK";

/// Lock file name used below the runtime directory.
pub const BLE_LOCK_FILE_NAME: &str = "8bitdo-ble.lock";

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
    /// Returns [`Error::BleBusy`] when another process holds the lock, and
    /// [`Error::Io`] when the lock file cannot be created or locked.
    pub fn acquire() -> Result<Self, Error> {
        Self::acquire_at(default_ble_lock_path())
    }

    /// Acquire the lock at an explicit path.
    ///
    /// Tests use this with a temporary path so they never contend with the
    /// developer's real session; tools can use it to share a lock with the
    /// CLI by setting [`BLE_LOCK_PATH_ENV`] to the same path.
    ///
    /// # Errors
    ///
    /// Same as [`BleLock::acquire`].
    pub fn acquire_at(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(&path)?;

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

        // Record the holder for `fuser`-style diagnostics and for an operator
        // who inspects the file. Best effort: the lock is already held, and a
        // failed write must not fail the operation.
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

/// Default lock path: `$XDG_RUNTIME_DIR/8bitdo-ble.lock`, else `/tmp/...`.
///
/// [`BLE_LOCK_PATH_ENV`], when set to a non-empty value, wins.
#[must_use]
pub fn default_ble_lock_path() -> PathBuf {
    if let Some(path) = std::env::var_os(BLE_LOCK_PATH_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    lock_path_in(std::env::var_os("XDG_RUNTIME_DIR"))
}

/// Pure helper for [`default_ble_lock_path`] so the rule is testable without
/// mutating the process environment.
fn lock_path_in(runtime_dir: Option<std::ffi::OsString>) -> PathBuf {
    let directory = runtime_dir
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            if cfg!(unix) {
                PathBuf::from("/tmp")
            } else {
                std::env::temp_dir()
            }
        });
    directory.join(BLE_LOCK_FILE_NAME)
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
    fn default_path_prefers_an_absolute_xdg_runtime_dir() {
        assert_eq!(
            lock_path_in(Some(std::ffi::OsString::from("/run/user/1000"))),
            PathBuf::from("/run/user/1000").join(BLE_LOCK_FILE_NAME)
        );
    }

    #[test]
    fn default_path_ignores_a_relative_or_missing_xdg_runtime_dir() {
        let expected_dir = if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        assert_eq!(
            lock_path_in(Some(std::ffi::OsString::from("relative/dir"))),
            expected_dir.join(BLE_LOCK_FILE_NAME)
        );
        assert_eq!(lock_path_in(None), expected_dir.join(BLE_LOCK_FILE_NAME));
    }

    #[test]
    fn lock_file_records_the_holder_pid() {
        let path = temp_lock_path("pid");
        let guard = BleLock::acquire_at(&path).expect("acquire");
        let contents = std::fs::read_to_string(guard.path()).expect("read lock file");
        assert!(
            contents.contains(&format!("pid={}", std::process::id())),
            "lock file must name its holder: {contents:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
