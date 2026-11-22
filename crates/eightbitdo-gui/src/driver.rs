//! Session driver used by the background worker.
//!
//! All device operations go through one [`Session`], so the core
//! single-flight contract is preserved: the worker awaits one call at a time
//! and never runs the input poll concurrently with a read.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use eightbitdo_core::{ConfigSnapshot, Error as CoreError, Session, Transport};

use crate::backup::{self, BackupError, BackupReport};
use crate::model::{ConfigView, DeviceInfoView, InputView, VersionView};

/// An error from a device operation.
#[derive(Debug)]
pub enum GuiError {
    /// The core session or transport rejected the operation.
    Core(CoreError),
    /// Writing a backup failed.
    Backup(BackupError),
}

impl fmt::Display for GuiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "{error}"),
            Self::Backup(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for GuiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::Backup(error) => Some(error),
        }
    }
}

impl From<CoreError> for GuiError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

impl From<BackupError> for GuiError {
    fn from(error: BackupError) -> Self {
        Self::Backup(error)
    }
}

/// A full configuration read plus its display form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRead {
    /// Raw 2768-byte block and CRC status.
    pub snapshot: ConfigSnapshot,
    /// Display summary built once on the worker thread.
    pub view: ConfigView,
}

/// Result of a GUI backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupOutcome {
    /// Written files and SHA-256.
    pub report: BackupReport,
    /// Summary of the block that was written.
    pub view: ConfigView,
}

/// A connected controller with its session and link metadata.
pub struct DeviceSession<T: Transport> {
    session: Session<T>,
    info: DeviceInfoView,
    sent: u64,
    received: u64,
}

impl<T: Transport> DeviceSession<T> {
    /// Wrap a connected session and its link metadata.
    #[must_use]
    pub fn new(session: Session<T>, info: DeviceInfoView) -> Self {
        Self {
            session,
            info,
            sent: 0,
            received: 0,
        }
    }

    /// Link metadata captured at connect time.
    #[must_use]
    pub fn info(&self) -> &DeviceInfoView {
        &self.info
    }

    /// Borrow the underlying session.
    #[must_use]
    pub fn session(&self) -> &Session<T> {
        &self.session
    }

    /// `0x0003` requests sent and input reports received since connecting.
    #[must_use]
    pub fn counters(&self) -> (u64, u64) {
        (self.sent, self.received)
    }

    /// Read and decode the firmware version.
    ///
    /// # Errors
    ///
    /// Returns [`GuiError::Core`] for session, protocol, or transport errors.
    pub async fn read_version(&self) -> Result<VersionView, GuiError> {
        let info = self.session.read_version().await?;
        Ok(VersionView::from_info(&info))
    }

    /// Read and validate the full 2768-byte configuration block.
    ///
    /// # Errors
    ///
    /// Returns [`GuiError::Core`] for session, protocol, or transport errors.
    pub async fn read_config(&self) -> Result<ConfigRead, GuiError> {
        let snapshot = self.session.read_config_snapshot().await?;
        let view = ConfigView::from_snapshot(&snapshot);
        Ok(ConfigRead { snapshot, view })
    }

    /// Read a fresh configuration block and write raw + JSON backups.
    ///
    /// # Errors
    ///
    /// Returns [`GuiError::Core`] when the read fails, or
    /// [`GuiError::Backup`] when a file cannot be written.
    pub async fn backup(&self, dir: &Path) -> Result<BackupOutcome, GuiError> {
        let read = self.read_config().await?;
        let report = backup::write_backup(dir, &read.snapshot, &read.view)?;
        Ok(BackupOutcome {
            report,
            view: read.view,
        })
    }

    /// Enable or disable the realtime input stream (`0x0007`).
    ///
    /// # Errors
    ///
    /// Returns [`GuiError::Core`] when the request cannot be sent.
    pub async fn set_reports_enabled(&mut self, enabled: bool) -> Result<(), GuiError> {
        self.session.set_report_enable(enabled).await?;
        Ok(())
    }

    /// Send one `0x0003` poll and wait briefly for its input report.
    ///
    /// # Errors
    ///
    /// Returns [`GuiError::Core`] when the send or drain fails. A missing
    /// report is `Ok(None)`.
    pub async fn poll_input(&mut self, wait: Duration) -> Result<Option<InputView>, GuiError> {
        self.session.request_input().await?;
        self.sent += 1;
        let report = self.session.try_next_input(wait).await?;
        if report.is_some() {
            self.received += 1;
        }
        Ok(report.map(InputView::from))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use eightbitdo_core::MockTransport;
    use eightbitdo_protocol::commands;
    use eightbitdo_protocol::config_bt2::{Bt2Config, CONFIG_SIZE};
    use eightbitdo_protocol::CustomHeader;

    use super::*;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn info() -> DeviceInfoView {
        DeviceInfoView {
            name: Some("80ND".to_string()),
            address: "AA:BB:CC:DD:EE:FF".to_string(),
            mtu: 64,
            characteristic: "0000ff13-0000-1000-8000-00805f9b34fb".to_string(),
        }
    }

    fn driver(mock: MockTransport) -> DeviceSession<MockTransport> {
        DeviceSession::new(Session::new(mock), info())
    }

    fn push_config_chunks(mock: &MockTransport, bytes: &[u8], chunk_size: usize) {
        for (index, chunk) in bytes.chunks(chunk_size).enumerate() {
            let header = CustomHeader::new(
                commands::cmd::READ_CONFIG,
                0,
                CONFIG_SIZE as u32,
                (index * chunk_size) as u32,
            )
            .with_chunk_len(chunk.len() as u16);
            mock.push_response(&header, chunk);
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "eightbitdo-gui-driver-{}-{unique}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[tokio::test]
    async fn read_version_decodes_the_observed_response() {
        let mock = MockTransport::new();
        let header = CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::READ_VERSION, 4, 0)
            .with_chunk_len(4);
        mock.push_response(&header, &[0xC8, 0x00, 0x00, 0x00]);
        let driver = driver(mock);

        let version = driver.read_version().await.unwrap();
        assert_eq!(version.value, Some(200));
        assert_eq!(version.raw_hex, "c8000000");

        let sent = driver.session().transport().sent_frames();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0][0], 0x04);
        assert_eq!(
            u16::from_le_bytes([sent[0][1], sent[0][2]]),
            commands::cmd::READ_VERSION
        );
    }

    #[tokio::test]
    async fn read_config_assembles_the_full_block() {
        let mock = MockTransport::new();
        let config = Bt2Config::default();
        push_config_chunks(&mock, config.as_bytes(), 142);
        let driver = driver(mock);

        let read = driver.read_config().await.unwrap();
        assert_eq!(read.snapshot.config.as_bytes(), config.as_bytes());
        assert_eq!(read.view.size, CONFIG_SIZE);
        assert_eq!(read.view.sha256, crate::util::sha256_hex(config.as_bytes()));
        assert_eq!(read.view.profile_names, ["", "", ""]);

        let sent = driver.session().transport().sent_frames();
        assert_eq!(sent.len(), 20);
        for frame in &sent {
            assert_eq!(
                u16::from_le_bytes([frame[1], frame[2]]),
                commands::cmd::READ_CONFIG
            );
        }
    }

    #[tokio::test]
    async fn poll_input_uses_0003_and_decodes_the_report() {
        let mock = MockTransport::new();
        let header = CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::INPUT, 12, 0)
            .with_chunk_len(12);
        mock.push_response(
            &header,
            &[
                0x00, 0x20, 0x00, 0x00, 0x7F, 0x80, 0xFF, 0x00, 0xCC, 0x33, 0x4B, 0x00,
            ],
        );
        let mut driver = driver(mock);

        let input = driver
            .poll_input(Duration::from_millis(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(input.trigger_l, 0x33);
        assert_eq!(input.trigger_r, 0xCC);
        assert_eq!(input.right_stick, (128, -127));
        assert_eq!(driver.counters(), (1, 1));

        let sent = driver.session().transport().sent_frames();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            u16::from_le_bytes([sent[0][1], sent[0][2]]),
            commands::cmd::INPUT
        );
    }

    #[tokio::test]
    async fn a_closed_link_is_reported_as_an_error() {
        let mock = MockTransport::new();
        let driver = driver(mock);
        let error = driver.read_version().await.unwrap_err();
        assert!(matches!(error, GuiError::Core(CoreError::Disconnected)));
    }

    #[tokio::test]
    async fn backup_reads_then_writes_raw_and_json() {
        let mock = MockTransport::new();
        let mut config = Bt2Config::default();
        config
            .set_profile_name(eightbitdo_protocol::config_bt2::Slot::S3, "Gamma")
            .unwrap();
        push_config_chunks(&mock, config.as_bytes(), 142);
        let driver = driver(mock);
        let dir = temp_dir("backup");

        let outcome = driver.backup(&dir).await.unwrap();
        assert_eq!(outcome.report.bytes, CONFIG_SIZE);
        assert_eq!(outcome.view.profile_names[2], "Gamma");
        assert_eq!(
            fs::read(&outcome.report.raw_path).unwrap(),
            config.as_bytes()
        );
        assert!(outcome.report.json_path.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn input_frame_counts_are_visible_to_the_ui() {
        let mock = MockTransport::new();
        let header = CustomHeader::new(commands::cmd::RESPONSE, commands::cmd::INPUT, 12, 0)
            .with_chunk_len(12);
        mock.push_response(&header, &[0u8; 12]);
        let mut driver = driver(mock);

        assert_eq!(driver.counters(), (0, 0));
        let _ = driver.poll_input(Duration::from_millis(1)).await.unwrap();
        assert_eq!(driver.counters(), (1, 1));
    }
}
