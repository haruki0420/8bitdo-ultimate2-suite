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
//! ## Path (per-user and environment-independent)
//!
//! 1. `EIGHTBITDO_BLE_LOCK` ([`BLE_LOCK_PATH_ENV`]) when set. This is a trusted
//!    override for tests and wrappers; the parent-directory policy below is
//!    skipped for it (the file itself is still validated).
//! 2. `$HOME/.cache/8bitdo/8bitdo-ble.lock` on Unix, where the home directory
//!    is taken from the passwd entry for the effective uid and only falls back
//!    to `$HOME` when there is no passwd entry. Deriving it from the uid makes
//!    the path identical for a desktop session, an SSH shell, cron and systemd
//!    (they often differ only in `XDG_RUNTIME_DIR`/`HOME`), so the same user
//!    always uses the same lock file.
//! 3. `<platform temp dir>/8bitdo-ble-<euid>/8bitdo-ble.lock` only when no home
//!    directory can be determined (`TMPDIR` is honoured through
//!    [`std::env::temp_dir`]).
//!
//! `XDG_RUNTIME_DIR` and `XDG_CACHE_HOME` are deliberately **not** consulted:
//! they differ between login contexts and would split the lock into separate
//! files for the same user.
//!
//! ## Validation
//!
//! The lock file is opened with `O_NOFOLLOW` on Unix and validated as a regular
//! file owned by the effective user before it is truncated or locked. A
//! symlink, directory, FIFO or another user's file is refused with
//! [`crate::Error::BleLockInvalid`], so a pre-created path in a shared
//! directory cannot be used to truncate an arbitrary file. The open file
//! descriptor is validated as well (not just the path), so a race between the
//! pre-check and the open cannot redirect the truncate/set-permissions calls.
//!
//! For the default path, an **existing** parent directory must also be owned by
//! the effective user and must not be group/other-writable
//! (`mode & 0o022 == 0`). A pre-created `0777` directory in a shared temp
//! directory is refused instead of being used for a second lock/DoS.
//! Directories created here are mode `0700`.
//!
//! ## Scope
//!
//! The lock is advisory and **per-user**: it stops cooperating processes of the
//! same user (this CLI, the future GUI, `scripts/session-loop.sh` users) from
//! fighting over the adapter. It is not a system-wide lock, not a security
//! boundary, and it does not stop an unrelated program from scanning.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::Error;

/// Environment variable that overrides the lock file path.
///
/// This is a trusted override for tests and wrappers: the parent-directory
/// policy is skipped for it. The lock file itself is still checked for
/// symlinks, regular-file type and (on Unix) ownership.
pub const BLE_LOCK_PATH_ENV: &str = "EIGHTBITDO_BLE_LOCK";

/// Lock file name used below the cache/temp directory.
pub const BLE_LOCK_FILE_NAME: &str = "8bitdo-ble.lock";

/// Per-user subdirectory below `$HOME/.cache`.
const CACHE_SUBDIR: &str = "8bitdo";

/// Whether the lock's parent directory is validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParentPolicy {
    /// Default path: an existing parent must be ours and not group/other
    /// writable.
    Enforce,
    /// Explicit `EIGHTBITDO_BLE_LOCK` / [`BleLock::acquire_at`]: trusted
    /// override for tests and tools.
    Override,
}

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
    /// [`Error::BleLockInvalid`] when the path or its parent directory is
    /// unsafe (symlink, non-regular file, foreign owner, group/other-writable
    /// parent), and [`Error::Io`] when the lock file cannot be created or
    /// locked.
    pub fn acquire() -> Result<Self, Error> {
        match std::env::var_os(BLE_LOCK_PATH_ENV).filter(|value| !value.is_empty()) {
            Some(path) => Self::acquire_with_policy(PathBuf::from(path), ParentPolicy::Override),
            None => Self::acquire_with_policy(default_ble_lock_path(), ParentPolicy::Enforce),
        }
    }

    /// Acquire the lock at an explicit path.
    ///
    /// Tests use this with a temporary path so they never contend with the
    /// developer's real session; tools can use it to share a lock with the
    /// CLI by setting [`BLE_LOCK_PATH_ENV`] to the same path. This is the
    /// trusted-override policy: the parent directory is not validated, but the
    /// lock file itself is (symlink, regular file, owner).
    ///
    /// # Errors
    ///
    /// Same as [`BleLock::acquire`], except that the parent-directory policy
    /// is not applied.
    pub fn acquire_at(path: impl Into<PathBuf>) -> Result<Self, Error> {
        Self::acquire_with_policy(path.into(), ParentPolicy::Override)
    }

    /// Acquire at an explicit path with the default (enforcing) parent policy.
    ///
    /// Exposed to the crate so the production lock path can be tested with a
    /// temporary directory; production itself reaches this through
    /// [`BleLock::acquire`].
    #[cfg(test)]
    pub(crate) fn acquire_enforcing(path: impl Into<PathBuf>) -> Result<Self, Error> {
        Self::acquire_with_policy(path.into(), ParentPolicy::Enforce)
    }

    fn acquire_with_policy(path: PathBuf, policy: ParentPolicy) -> Result<Self, Error> {
        precheck_lock_path(&path)?;
        precheck_lock_parent(&path)?;
        prepare_lock_directory(&path, policy)?;
        let file = open_lock_file(&path, policy)?;
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

/// Default lock path: a per-user cache directory derived from the effective
/// uid, never a shared `/tmp` file and never an XDG directory.
///
/// [`BLE_LOCK_PATH_ENV`], when set to a non-empty value, wins (the caller
/// decides the parent-directory policy; [`BleLock::acquire`] treats it as a
/// trusted override).
#[must_use]
pub fn default_ble_lock_path() -> PathBuf {
    if let Some(path) = std::env::var_os(BLE_LOCK_PATH_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    resolve_default_lock_path(current_user_home(), std::env::temp_dir(), current_euid())
}

/// Pure path-resolution helper, separated so every fallback branch is testable
/// without mutating the process environment.
///
/// The signature deliberately carries no XDG values: the lock must not move
/// between login environments for the same user.
fn resolve_default_lock_path(home: Option<PathBuf>, temp_dir: PathBuf, euid: u32) -> PathBuf {
    match home.filter(|path| !path.as_os_str().is_empty()) {
        Some(home) => home
            .join(".cache")
            .join(CACHE_SUBDIR)
            .join(BLE_LOCK_FILE_NAME),
        None => temp_dir
            .join(format!("8bitdo-ble-{euid}"))
            .join(BLE_LOCK_FILE_NAME),
    }
}

#[cfg(unix)]
fn current_euid() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[cfg(not(unix))]
fn current_euid() -> u32 {
    0
}

#[cfg(unix)]
fn current_user_home() -> Option<PathBuf> {
    // NSS-aware lookup (`getpwuid_r`) so LDAP/SSSD users get their real home
    // too. `$HOME` is deliberately not used on Unix: it changes between a
    // desktop session, SSH, cron and systemd for the same user, which would
    // split the lock. When no passwd entry exists the caller falls back to the
    // per-uid temp directory.
    nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(current_euid()))
        .ok()
        .flatten()
        .map(|user| user.dir)
        .filter(|directory| !directory.as_os_str().is_empty())
}

#[cfg(not(unix))]
fn current_user_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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

/// Reject a symlinked parent directory before creating/opening anything.
///
/// `metadata` follows symlinks, so validating a parent through it would let an
/// attacker point `<temp>/8bitdo-ble-<euid>` at a victim directory and pass the
/// owner/mode check. The enforcing open closes the remaining TOCTOU window with
/// `openat(..., O_NOFOLLOW|O_DIRECTORY)`.
fn precheck_lock_parent(path: &Path) -> Result<(), Error> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    match std::fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::BleLockInvalid {
            path: parent.to_path_buf(),
            reason: "lock parent is a symbolic link; refusing to follow it".to_string(),
        }),
        Ok(metadata) if !metadata.is_dir() => Err(Error::BleLockInvalid {
            path: parent.to_path_buf(),
            reason: "lock parent is not a directory".to_string(),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Io(error)),
    }
}

/// Create the lock's parent directory with mode `0700` when missing.
///
/// Existing directories are validated for the default path (owner + no
/// group/other write) and accepted as-is for the trusted override. Only
/// directories this function creates are chmod-ed.
fn prepare_lock_directory(path: &Path, policy: ParentPolicy) -> Result<(), Error> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    if parent.is_dir() {
        if policy == ParentPolicy::Enforce {
            validate_lock_directory(parent)?;
        }
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
    if policy == ParentPolicy::Enforce {
        validate_lock_directory(parent)?;
    }
    Ok(())
}

fn validate_lock_directory(path: &Path) -> Result<(), Error> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::BleLockInvalid {
            path: path.to_path_buf(),
            reason: "lock parent is a symbolic link; refusing to follow it".to_string(),
        });
    }
    if !metadata.is_dir() {
        return Err(Error::BleLockInvalid {
            path: path.to_path_buf(),
            reason: "lock parent is not a directory".to_string(),
        });
    }
    validate_lock_directory_metadata(path, &metadata)
}

fn validate_lock_directory_metadata(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), Error> {
    if !metadata.is_dir() {
        return Err(Error::BleLockInvalid {
            path: path.to_path_buf(),
            reason: "lock parent is not a directory".to_string(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        validate_lock_directory_permissions(path, metadata.uid(), current_euid(), metadata.mode())?;
    }
    Ok(())
}

/// Owner/mode check split out so a pre-created `0777` directory is testable
/// without changing umask or chown.
#[cfg(unix)]
fn validate_lock_directory_permissions(
    path: &Path,
    owner: u32,
    euid: u32,
    mode: u32,
) -> Result<(), Error> {
    if owner != euid {
        return Err(Error::BleLockInvalid {
            path: path.to_path_buf(),
            reason: format!(
                "lock directory is owned by uid {owner}, not the current user (uid {euid})"
            ),
        });
    }
    if mode & 0o022 != 0 {
        return Err(Error::BleLockInvalid {
            path: path.to_path_buf(),
            reason: format!(
                "lock directory is group/other-writable (mode {:o}); refusing a shared, \
                 predictable directory",
                mode & 0o777
            ),
        });
    }
    Ok(())
}

fn open_lock_file(path: &Path, policy: ParentPolicy) -> Result<File, Error> {
    // On Unix the default path is opened relative to a verified directory
    // descriptor, closing the parent-symlink/rename TOCTOU window. The
    // trusted override keeps the path-based open (its parent is the caller's
    // responsibility) but still uses O_NOFOLLOW for the file itself.
    #[cfg(unix)]
    if policy == ParentPolicy::Enforce {
        return open_lock_file_via_dirfd(path);
    }
    #[cfg(not(unix))]
    let _ = policy;
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

/// Open the lock file relative to a verified parent directory descriptor.
#[cfg(unix)]
fn open_lock_file_via_dirfd(path: &Path) -> Result<File, Error> {
    use rustix::fs::{openat, Mode, OFlags, CWD};

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| Error::BleLockInvalid {
            path: path.to_path_buf(),
            reason: "lock path has no parent directory".to_string(),
        })?;
    let name = path.file_name().ok_or_else(|| Error::BleLockInvalid {
        path: path.to_path_buf(),
        reason: "lock path has no file name".to_string(),
    })?;

    let directory = openat(
        CWD,
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        lock_open_error(
            parent,
            &std::io::Error::from_raw_os_error(error.raw_os_error()),
        )
    })?;
    let directory = File::from(directory);
    validate_lock_directory_metadata(parent, &directory.metadata()?)?;

    let file = openat(
        &directory,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(|error| {
        lock_open_error(
            path,
            &std::io::Error::from_raw_os_error(error.raw_os_error()),
        )
    })?;
    Ok(File::from(file))
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

    fn temp_parent_dir(label: &str, mode: u32) -> PathBuf {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "eightbitdo-lock-dir-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).expect("create temp directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(mode))
                .expect("set directory mode");
        }
        directory
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
        // Repeated acquire/drop catches a delayed release. The file is not
        // unlinked between iterations: unlinking a locked file could let the
        // kernel reuse its inode for an unrelated lock file.
        let path = temp_lock_path("release");
        for _ in 0..50 {
            {
                let _guard = BleLock::acquire_at(&path).expect("first acquire");
            }
            let guard = BleLock::acquire_at(&path).expect("lock must be released on drop");
            drop(guard);
        }
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
        let guard = BleLock::acquire_at(&path).expect("lock must be free after unlock");
        drop(guard);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_path_is_environment_independent() {
        // The resolver has no XDG inputs: the same home always maps to the
        // same path, so changing XDG_RUNTIME_DIR/XDG_CACHE_HOME between a
        // desktop session and cron cannot split the lock.
        let home = Some(PathBuf::from("/home/alice"));
        let temp = PathBuf::from("/tmp");
        let first = resolve_default_lock_path(home.clone(), temp.clone(), 1000);
        let second = resolve_default_lock_path(home, temp, 1000);
        assert_eq!(first, second);
        assert_eq!(
            first,
            PathBuf::from("/home/alice/.cache/8bitdo/8bitdo-ble.lock")
        );
        assert!(
            !first.to_string_lossy().contains("XDG"),
            "the default path must not depend on XDG directories"
        );
    }

    #[test]
    fn default_path_falls_back_to_a_per_user_temp_directory() {
        assert_eq!(
            resolve_default_lock_path(None, PathBuf::from("/var/tmp"), 1000),
            PathBuf::from("/var/tmp")
                .join("8bitdo-ble-1000")
                .join(BLE_LOCK_FILE_NAME)
        );
        assert_eq!(
            resolve_default_lock_path(Some(PathBuf::new()), PathBuf::from("/tmp"), 1000),
            PathBuf::from("/tmp")
                .join("8bitdo-ble-1000")
                .join(BLE_LOCK_FILE_NAME)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parent_directory_is_rejected() {
        let real = temp_parent_dir("parent-real", 0o700);
        let link = temp_parent_dir("parent-link-holder", 0o700).join("link");
        std::os::unix::fs::symlink(&real, &link).expect("create parent symlink");
        let path = link.join(BLE_LOCK_FILE_NAME);

        // The enforcing (production) policy must not follow the directory
        // symlink; otherwise an attacker could point the per-uid temp
        // directory at a victim directory and bypass the owner/mode checks.
        let error = BleLock::acquire_with_policy(path.clone(), ParentPolicy::Enforce)
            .expect_err("symlinked parent must be refused");
        assert!(
            matches!(error, Error::BleLockInvalid { ref reason, .. }
                if reason.contains("parent is a symbolic link")),
            "expected a parent-symlink rejection, got {error:?}"
        );
        // Even the trusted override rejects a symlinked parent.
        let error = BleLock::acquire_at(&path).expect_err("override must reject it too");
        assert!(
            matches!(error, Error::BleLockInvalid { ref reason, .. }
                if reason.contains("parent is a symbolic link")),
            "expected a parent-symlink rejection, got {error:?}"
        );

        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&real);
    }

    #[cfg(unix)]
    #[test]
    fn default_policy_rejects_a_world_writable_parent() {
        let parent = temp_parent_dir("parent-0777", 0o777);
        let path = parent.join(BLE_LOCK_FILE_NAME);
        let error = BleLock::acquire_with_policy(path, ParentPolicy::Enforce)
            .expect_err("a 0777 parent must be refused");
        assert!(
            matches!(error, Error::BleLockInvalid { ref reason, .. }
                if reason.contains("group/other-writable")),
            "expected a parent-mode rejection, got {error:?}"
        );
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[cfg(unix)]
    #[test]
    fn default_policy_accepts_a_private_parent() {
        let parent = temp_parent_dir("parent-0700", 0o700);
        let path = parent.join(BLE_LOCK_FILE_NAME);
        let guard = BleLock::acquire_with_policy(path.clone(), ParentPolicy::Enforce)
            .expect("a 0700 parent is accepted");
        assert_eq!(guard.path(), path);
        drop(guard);
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[cfg(unix)]
    #[test]
    fn trusted_override_skips_the_parent_policy() {
        // `EIGHTBITDO_BLE_LOCK` / `acquire_at` are documented test/tool
        // exceptions, so a lock directly in a shared temp directory works.
        let parent = temp_parent_dir("parent-override", 0o777);
        let path = parent.join(BLE_LOCK_FILE_NAME);
        let guard = BleLock::acquire_at(path.clone()).expect("override policy accepts it");
        assert_eq!(guard.path(), path);
        drop(guard);
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[test]
    fn directory_permission_check_rejects_foreign_owner_and_group_write() {
        #[cfg(unix)]
        {
            let path = Path::new("/tmp/example");
            let error = validate_lock_directory_permissions(path, 1001, 1000, 0o700)
                .expect_err("a foreign owner must be refused");
            assert!(
                matches!(error, Error::BleLockInvalid { ref reason, .. }
                    if reason.contains("owned by uid")),
                "got {error:?}"
            );
            let error = validate_lock_directory_permissions(path, 1000, 1000, 0o770)
                .expect_err("group-writable must be refused");
            assert!(
                matches!(error, Error::BleLockInvalid { ref reason, .. }
                    if reason.contains("group/other-writable")),
                "got {error:?}"
            );
            validate_lock_directory_permissions(path, 1000, 1000, 0o700)
                .expect("0700 and owned is accepted");
            validate_lock_directory_permissions(path, 1000, 1000, 0o755)
                .expect("0755 has no group/other write and is accepted");
        }
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
        use rustix::fs::{Mode, CWD};
        let path = temp_lock_path("fifo");
        // `mkfifoat` instead of `Command::new("mkfifo")`: a fork+exec child can
        // briefly inherit another test's flock descriptor and make its next
        // acquisition fail spuriously.
        rustix::fs::mkfifoat(CWD, &path, Mode::from_bits_truncate(0o600)).expect("create FIFO");
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
        drop(guard);
        let _ = std::fs::remove_file(&path);
    }
}
