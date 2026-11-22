//! Session driver used by the background worker.
//!
//! All device operations go through one [`Session`], so the core
//! single-flight contract is preserved: the worker awaits one call at a time
//! and never runs the input poll concurrently with a read.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use eightbitdo_core::config_ops::{
    ConfigCrcStatus, ConfigRegion, ConfigWriteTimeouts, PreparedConfigWrite,
};
use eightbitdo_core::macro_ops::{MacroBackup, MacroEntry, MacroWritePlan};
use eightbitdo_core::{ConfigSnapshot, Error as CoreError, Session, Transport};
use eightbitdo_protocol::config_bt2::{Bt2Config, MacroIndex, Slot};

use crate::backup::{self, BackupError, BackupRef, BackupReport, MacroBackupReport};
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
    /// The block that was written.
    pub snapshot: ConfigSnapshot,
    /// Summary of the block that was written.
    pub view: ConfigView,
}

/// Phase in which a write preparation or commit failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePhase {
    /// Reading the configuration or macro backup from the controller.
    Read,
    /// Local validation of the plan against the fresh snapshot.
    Prepare,
    /// Writing the mandatory pre-write backup.
    Backup,
    /// Sending frames to the controller (write, Save, erase, verify).
    Device,
}

impl fmt::Display for WritePhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => formatter.write_str("Read"),
            Self::Prepare => formatter.write_str("Prepare"),
            Self::Backup => formatter.write_str("Backup"),
            Self::Device => formatter.write_str("Device"),
        }
    }
}

/// A failed write, carrying the pre-write backup when one exists.
///
/// `detail` is the untranslated core/technical text. `recovery` is the
/// core-provided English recovery guidance (it is shown verbatim as technical
/// detail; the UI adds its own localized recovery steps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFailure {
    /// Phase that failed.
    pub phase: WritePhase,
    /// Technical detail (English).
    pub detail: String,
    /// Backup written before the failure, if any.
    pub backup: Option<BackupRef>,
    /// Core recovery guidance, when the core supplied one.
    pub recovery: Option<String>,
    /// True when the link was disconnected.
    pub disconnected: bool,
}

impl WriteFailure {
    fn before(phase: WritePhase, detail: impl Into<String>) -> Box<Self> {
        Box::new(Self {
            phase,
            detail: detail.into(),
            backup: None,
            recovery: None,
            disconnected: false,
        })
    }

    fn from_read(error: CoreError) -> Box<Self> {
        let disconnected = matches!(error, CoreError::Disconnected);
        Box::new(Self {
            phase: WritePhase::Read,
            detail: error.to_string(),
            backup: None,
            recovery: None,
            disconnected,
        })
    }

    fn from_backup(error: BackupError) -> Box<Self> {
        Self::before(WritePhase::Backup, error.to_string())
    }

    fn from_device(error: CoreError, backup: BackupRef) -> Box<Self> {
        let disconnected = matches!(error, CoreError::Disconnected);
        let (detail, recovery) = match &error {
            CoreError::ConfigWrite {
                stage,
                state,
                reason,
                recovery,
            } => (
                format!("config write failed during {stage} ({state}): {reason}"),
                Some((*recovery).to_string()),
            ),
            CoreError::MacroWrite {
                stage,
                state,
                reason,
                recovery,
            } => (
                format!("macro write failed during {stage} ({state}): {reason}"),
                Some((*recovery).to_string()),
            ),
            other => (other.to_string(), None),
        };
        Box::new(Self {
            phase: WritePhase::Device,
            detail,
            backup: Some(backup),
            recovery,
            disconnected: disconnected || error.to_string().contains("transport disconnected"),
        })
    }
}

impl fmt::Display for WriteFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "write failed during {}: {}",
            self.phase, self.detail
        )
    }
}

impl std::error::Error for WriteFailure {}

/// A configuration write prepared against a fresh snapshot, with its mandatory
/// pre-write backup already written.
#[derive(Debug)]
pub struct ConfigWritePreparation {
    /// The exact snapshot the plan was prepared against and the backup holds.
    pub snapshot: ConfigSnapshot,
    /// Display summary of that snapshot.
    pub view: ConfigView,
    /// Validated, CRC-recomputed buffer to send.
    pub prepared: PreparedConfigWrite,
    /// Written pre-write backup.
    pub backup: BackupReport,
}

/// Result of a confirmed configuration write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigWriteReport {
    /// Written pre-write backup.
    pub backup: BackupReport,
    /// Configuration read back after Save.
    pub snapshot: ConfigSnapshot,
    /// Display summary of the read-back.
    pub view: ConfigView,
    /// Byte ranges that differed from the pre-write snapshot.
    pub changed: Vec<ConfigRegion>,
    /// Frames sent: `0x0001` chunks + one Save.
    pub packets_sent: usize,
    /// CRC state of the read-back.
    pub crc: ConfigCrcStatus,
}

/// A macro write prepared against a fresh configuration and full macro backup.
#[derive(Debug)]
pub struct MacroWritePreparation {
    /// Configuration snapshot the macro backup was read against.
    pub snapshot: ConfigSnapshot,
    /// Display summary of that snapshot.
    pub view: ConfigView,
    /// Full 12-entry macro backup (also the core write precondition).
    pub macro_backup: MacroBackup,
    /// The target macro as read from the device.
    pub target: MacroEntry,
    /// Written pre-write backup directory.
    pub backup: MacroBackupReport,
}

/// Result of a confirmed macro write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroWriteReport {
    /// Written pre-write backup.
    pub backup: MacroBackupReport,
    /// Profile.
    pub slot: Slot,
    /// Macro index.
    pub index: MacroIndex,
    /// Number of written steps.
    pub steps: usize,
    /// Macro read back after the final Save and verification.
    pub readback: MacroEntry,
    /// Slot macro-enabled flag after the update.
    pub flag: u32,
    /// Registered macro count after the update.
    pub total_cnt: u8,
    /// Frames sent for the whole sequence.
    pub packets_sent: usize,
    /// Whether the read-back 2768-byte config satisfies stored == computed CRC.
    pub config_crc_valid: bool,
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
            snapshot: read.snapshot,
            view: read.view,
        })
    }

    /// Read one macro with the configuration snapshot it belongs to.
    ///
    /// # Errors
    ///
    /// Returns [`GuiError::Core`] for read, protocol, or transport errors.
    pub async fn read_macro(
        &self,
        snapshot: &ConfigSnapshot,
        slot: Slot,
        index: MacroIndex,
    ) -> Result<MacroEntry, GuiError> {
        Ok(self.session.read_macro(snapshot, slot, index).await?)
    }

    /// Read a fresh configuration, validate a configuration plan against it,
    /// and write the mandatory pre-write backup.
    ///
    /// Nothing is sent to flash before the backup is on disk. The returned
    /// preparation owns the exact snapshot the backup contains, so a later
    /// read-back comparison cannot mix snapshots.
    ///
    /// # Errors
    ///
    /// Returns [`WriteFailure`] with the failing phase. A `Prepare` failure
    /// (including a no-op plan) happens before any backup or frame; a `Backup`
    /// failure happens before any frame.
    pub async fn prepare_config_write(
        &self,
        plan: &eightbitdo_core::config_ops::ConfigWritePlan,
        dir: &Path,
    ) -> Result<ConfigWritePreparation, Box<WriteFailure>> {
        let snapshot = self
            .session
            .read_config_snapshot()
            .await
            .map_err(WriteFailure::from_read)?;
        let view = ConfigView::from_snapshot(&snapshot);
        let prepared = plan
            .prepare(&snapshot)
            .map_err(|error| WriteFailure::before(WritePhase::Prepare, error.to_string()))?;
        if prepared.is_noop() {
            return Err(WriteFailure::before(
                WritePhase::Prepare,
                "configuration write plan changes nothing",
            ));
        }
        let backup = backup::write_prewrite_backup(dir, &snapshot, &view)
            .map_err(WriteFailure::from_backup)?;
        Ok(ConfigWritePreparation {
            snapshot,
            view,
            prepared,
            backup,
        })
    }

    /// Send a prepared configuration write, Save, and verify the read-back.
    ///
    /// # Errors
    ///
    /// Returns [`WriteFailure`] with the backup reference and the core recovery
    /// guidance when the device write, Save, or verification fails.
    pub async fn commit_config_write(
        &self,
        preparation: &ConfigWritePreparation,
    ) -> Result<ConfigWriteReport, Box<WriteFailure>> {
        let outcome = self
            .session
            .write_full_config(
                &preparation.snapshot,
                &preparation.prepared,
                ConfigWriteTimeouts::default(),
            )
            .await
            .map_err(|error| {
                WriteFailure::from_device(error, BackupRef::from(&preparation.backup))
            })?;
        let snapshot = snapshot_from_config(outcome.read_back);
        let view = ConfigView::from_snapshot(&snapshot);
        Ok(ConfigWriteReport {
            backup: preparation.backup.clone(),
            snapshot,
            view,
            changed: outcome.changed,
            packets_sent: outcome.packets_sent,
            crc: outcome.crc,
        })
    }

    /// Read the full configuration and all 12 macros, then write the mandatory
    /// pre-write macro backup directory.
    ///
    /// The target macro must already hold steps: creating an empty macro needs
    /// flash step-clear support that is not implemented, so it is refused
    /// before any frame.
    ///
    /// # Errors
    ///
    /// Returns [`WriteFailure`] with the failing phase; a `Prepare` failure
    /// (empty target) happens before any backup or frame.
    pub async fn prepare_macro_write(
        &self,
        slot: Slot,
        index: MacroIndex,
        dir: &Path,
    ) -> Result<MacroWritePreparation, Box<WriteFailure>> {
        let snapshot = self
            .session
            .read_config_snapshot()
            .await
            .map_err(WriteFailure::from_read)?;
        let view = ConfigView::from_snapshot(&snapshot);
        let record = snapshot.config.macro_record(slot, index);
        if eightbitdo_core::macro_ops::is_skipped_steps(record.max_steps) {
            return Err(WriteFailure::before(
                WritePhase::Prepare,
                "target macro is empty; creating an empty macro is not supported",
            ));
        }
        let macro_backup = self
            .session
            .read_macro_backup(&snapshot)
            .await
            .map_err(WriteFailure::from_read)?;
        let backup = backup::write_macro_backup(dir, &snapshot, &macro_backup)
            .map_err(WriteFailure::from_backup)?;
        let target = macro_backup.get(slot, index).cloned().ok_or_else(|| {
            WriteFailure::before(
                WritePhase::Prepare,
                "macro backup does not contain the target macro",
            )
        })?;
        Ok(MacroWritePreparation {
            snapshot,
            view,
            macro_backup,
            target,
            backup,
        })
    }

    /// Send a prepared macro write (Header → Save → Erase → Write → Save) and
    /// verify the read-back.
    ///
    /// # Errors
    ///
    /// Returns [`WriteFailure`] with the backup reference and the core recovery
    /// guidance when a flash stage or the verification fails.
    pub async fn commit_macro_write(
        &self,
        preparation: &MacroWritePreparation,
        plan: &MacroWritePlan,
    ) -> Result<MacroWriteReport, Box<WriteFailure>> {
        let slot = preparation.target.slot();
        let index = preparation.target.index();
        let outcome = self
            .session
            .write_macro(&preparation.macro_backup, slot, index, plan)
            .await
            .map_err(|error| {
                WriteFailure::from_device(error, BackupRef::from(&preparation.backup))
            })?;
        Ok(MacroWriteReport {
            backup: preparation.backup.clone(),
            slot,
            index,
            steps: plan.steps.len(),
            readback: outcome.readback,
            flag: outcome.flag,
            total_cnt: outcome.total_cnt,
            packets_sent: outcome.packets_sent,
            config_crc_valid: outcome.config_crc_valid,
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

/// Rebuild a validated snapshot from a buffer the device returned.
fn snapshot_from_config(config: Bt2Config) -> ConfigSnapshot {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use eightbitdo_core::config_ops::{config_write_chunk_len, ConfigEdit, ConfigWritePlan};
    use eightbitdo_core::macro_ops::{is_skipped_steps, macro_header_chunk_len, MacroWritePlan};
    use eightbitdo_core::MockTransport;
    use eightbitdo_core::Transport;
    use eightbitdo_protocol::commands;
    use eightbitdo_protocol::config_bt2::{Bt2Config, MacroIndex, MacroSlot, Slot, CONFIG_SIZE};
    use eightbitdo_protocol::keymap::KeyMap;
    use eightbitdo_protocol::macro_data::{encode_macro_steps, MacroStep};
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

    fn push_ack(mock: &MockTransport, cmd: u16, subcmd: u16) {
        let header = CustomHeader::new(cmd, subcmd, 0, 0);
        mock.push_response(&header, &[]);
    }

    fn frame_cmd(frame: &[u8]) -> u16 {
        u16::from_le_bytes([frame[1], frame[2]])
    }

    fn count_cmd(frames: &[Vec<u8>], cmd: u16) -> usize {
        frames
            .iter()
            .filter(|frame| frame_cmd(frame) == cmd)
            .count()
    }

    fn valid_config(mut config: Bt2Config) -> Bt2Config {
        config.recompute_crc();
        config
    }

    fn push_macro_response(mock: &MockTransport, mode: u16, slot: Slot, raw: &[u8]) {
        let header = CustomHeader::new(
            commands::cmd::READ_MACRO,
            commands::macro_subcmd(mode, slot),
            raw.len() as u32,
            0,
        )
        .with_payload(raw);
        mock.push_response(&header, raw);
    }

    #[tokio::test]
    async fn config_write_takes_a_prewrite_backup_and_verifies_the_read_back() {
        let mock = MockTransport::new();
        let mut before = Bt2Config::default();
        before.set_profile_name(Slot::S1, "Before").unwrap();
        before = valid_config(before);
        push_config_chunks(&mock, before.as_bytes(), 142);

        let write_chunk = config_write_chunk_len(mock.mtu()).unwrap();
        for _ in 0..CONFIG_SIZE.div_ceil(write_chunk) {
            push_ack(&mock, commands::cmd::RESPONSE, commands::cmd::WRITE_CONFIG);
        }
        push_ack(&mock, commands::cmd::SAVE, commands::subcmd::SAVE_BLE);

        let mut after = before.clone();
        after.set_profile_name(Slot::S1, "After").unwrap();
        after = valid_config(after);
        push_config_chunks(&mock, after.as_bytes(), 142);

        let driver = driver(mock);
        let dir = temp_dir("prewrite-config");
        let plan = ConfigWritePlan::new().with(ConfigEdit::ProfileName {
            slot: Slot::S1,
            name: "After".to_string(),
        });

        let preparation = driver.prepare_config_write(&plan, &dir).await.unwrap();
        assert_eq!(
            preparation.backup.sha256,
            crate::util::sha256_hex(before.as_bytes())
        );
        assert_eq!(
            fs::read(&preparation.backup.raw_path).unwrap(),
            before.as_bytes()
        );
        assert!(!fs::read(&preparation.backup.json_path).unwrap().is_empty());
        let filename = preparation
            .backup
            .raw_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            filename.starts_with("prewrite-config-"),
            "pre-write backups are distinguishable from manual ones: {filename}"
        );

        let report = driver.commit_config_write(&preparation).await.unwrap();
        assert_eq!(report.view.profile_names[0], "After");
        assert!(!report.changed.is_empty());
        assert!(report.crc.matches_sent());
        assert_eq!(report.packets_sent, CONFIG_SIZE.div_ceil(write_chunk) + 1);

        let sent = driver.session().transport().sent_frames();
        assert_eq!(count_cmd(&sent, commands::cmd::READ_CONFIG), 40);
        assert_eq!(
            count_cmd(&sent, commands::cmd::WRITE_CONFIG),
            CONFIG_SIZE.div_ceil(write_chunk)
        );
        assert_eq!(count_cmd(&sent, commands::cmd::SAVE), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_noop_config_plan_is_refused_before_a_backup_is_written() {
        let mock = MockTransport::new();
        let config = valid_config(Bt2Config::default());
        push_config_chunks(&mock, config.as_bytes(), 142);
        let driver = driver(mock);
        let dir = temp_dir("noop");

        let failure = driver
            .prepare_config_write(&ConfigWritePlan::new(), &dir)
            .await
            .unwrap_err();
        assert_eq!(failure.phase, WritePhase::Prepare);
        assert!(failure.backup.is_none());
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            0,
            "no backup may be left behind for a refused plan"
        );
        let sent = driver.session().transport().sent_frames();
        assert_eq!(count_cmd(&sent, commands::cmd::READ_CONFIG), 20);
        assert_eq!(count_cmd(&sent, commands::cmd::WRITE_CONFIG), 0);
        assert_eq!(count_cmd(&sent, commands::cmd::SAVE), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_backup_failure_stops_before_any_write_frame() {
        let mock = MockTransport::new();
        let config = valid_config(Bt2Config::default());
        push_config_chunks(&mock, config.as_bytes(), 142);
        let driver = driver(mock);
        let parent = temp_dir("backup-blocked");
        let blocked = parent.join("not-a-directory");
        fs::write(&blocked, b"not a directory").unwrap();

        let plan = ConfigWritePlan::new().with(ConfigEdit::GamepadMode(
            eightbitdo_protocol::config_bt2::GamepadMode::Mac,
        ));
        let failure = driver
            .prepare_config_write(&plan, &blocked)
            .await
            .unwrap_err();
        assert_eq!(failure.phase, WritePhase::Backup);
        assert!(failure.backup.is_none());

        let sent = driver.session().transport().sent_frames();
        assert_eq!(count_cmd(&sent, commands::cmd::WRITE_CONFIG), 0);
        assert_eq!(count_cmd(&sent, commands::cmd::SAVE), 0);

        let _ = fs::remove_dir_all(&parent);
    }

    #[tokio::test]
    async fn a_disconnect_during_the_commit_reports_the_backup() {
        let mock = MockTransport::new();
        let config = valid_config(Bt2Config::default());
        push_config_chunks(&mock, config.as_bytes(), 142);
        let driver = driver(mock);
        let dir = temp_dir("disconnect");

        let plan = ConfigWritePlan::new().with(ConfigEdit::GamepadMode(
            eightbitdo_protocol::config_bt2::GamepadMode::Mac,
        ));
        let preparation = driver.prepare_config_write(&plan, &dir).await.unwrap();
        let failure = driver.commit_config_write(&preparation).await.unwrap_err();
        assert_eq!(failure.phase, WritePhase::Device);
        assert!(failure.disconnected);
        let backup = failure
            .backup
            .clone()
            .expect("the pre-write backup must be reported");
        assert_eq!(backup.config_path, preparation.backup.raw_path);
        assert!(backup.config_path.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn an_empty_macro_target_is_refused_before_the_macro_backup() {
        let mock = MockTransport::new();
        let config = valid_config(Bt2Config::default());
        push_config_chunks(&mock, config.as_bytes(), 142);
        let driver = driver(mock);
        let dir = temp_dir("empty-macro");

        let failure = driver
            .prepare_macro_write(Slot::S1, MacroIndex::M1, &dir)
            .await
            .unwrap_err();
        assert_eq!(failure.phase, WritePhase::Prepare);
        assert!(failure.backup.is_none());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        let sent = driver.session().transport().sent_frames();
        assert_eq!(count_cmd(&sent, commands::cmd::READ_CONFIG), 20);
        assert_eq!(count_cmd(&sent, commands::cmd::READ_MACRO), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn macro_write_takes_a_full_backup_and_reads_the_steps_back() {
        let mock = MockTransport::new();
        let original_steps = vec![
            MacroStep::neutral(20),
            MacroStep {
                ms_times: 40,
                keys: KeyMap::A.bits() as u16,
                ..MacroStep::neutral(40)
            },
        ];
        let mut header = MacroSlot::default();
        header.records[0].max_steps = original_steps.len() as u16;
        let mut config = Bt2Config::default();
        config.set_macro_slot(Slot::S1, &header);
        config = valid_config(config);

        push_config_chunks(&mock, config.as_bytes(), 142);
        let original_raw =
            encode_macro_steps(&original_steps).unwrap()[..original_steps.len() * 10].to_vec();
        push_macro_response(
            &mock,
            config.gamepad_mode_raw() & 0xFF,
            Slot::S1,
            &original_raw,
        );

        let header_chunk = macro_header_chunk_len(mock.mtu()).unwrap();
        for _ in 0..216usize.div_ceil(header_chunk) {
            push_ack(&mock, commands::cmd::RESPONSE, commands::cmd::WRITE_CONFIG);
        }
        push_ack(&mock, commands::cmd::SAVE, commands::subcmd::SAVE_BLE);
        push_ack(&mock, commands::cmd::RESPONSE, commands::cmd::ERASE_MACRO);
        push_ack(&mock, commands::cmd::RESPONSE, commands::cmd::WRITE_MACRO);
        push_ack(&mock, commands::cmd::SAVE, commands::subcmd::SAVE_BLE);

        let new_steps = vec![
            MacroStep::neutral(40),
            MacroStep {
                ms_times: 60,
                keys: (KeyMap::B | KeyMap::R1).bits() as u16,
                ..MacroStep::neutral(60)
            },
        ];
        assert_eq!(new_steps.len(), original_steps.len());
        push_config_chunks(&mock, config.as_bytes(), 142);
        let new_raw = encode_macro_steps(&new_steps).unwrap()[..new_steps.len() * 10].to_vec();
        push_macro_response(&mock, config.gamepad_mode_raw() & 0xFF, Slot::S1, &new_raw);

        let driver = driver(mock);
        let dir = temp_dir("macro-write");
        let preparation = driver
            .prepare_macro_write(Slot::S1, MacroIndex::M1, &dir)
            .await
            .unwrap();
        assert_eq!(preparation.backup.macro_count, 1);
        assert_eq!(preparation.backup.skipped_count, 11);
        assert_eq!(
            fs::read(&preparation.backup.config_path).unwrap(),
            config.as_bytes()
        );
        assert!(preparation.backup.manifest_path.exists());
        assert!(preparation.backup.dir.join("slot0-macro0.bin").exists());
        assert!(!is_skipped_steps(preparation.target.record().max_steps));

        let plan = MacroWritePlan::new(new_steps.clone());
        let report = driver
            .commit_macro_write(&preparation, &plan)
            .await
            .unwrap();
        assert_eq!(report.slot, Slot::S1);
        assert_eq!(report.index, MacroIndex::M1);
        assert_eq!(report.steps, new_steps.len());
        assert_eq!(report.readback.steps(), new_steps.as_slice());
        assert!(report.backup.manifest_path.exists());

        let sent = driver.session().transport().sent_frames();
        assert_eq!(count_cmd(&sent, commands::cmd::READ_CONFIG), 40);
        assert_eq!(count_cmd(&sent, commands::cmd::READ_MACRO), 2);
        assert_eq!(
            count_cmd(&sent, commands::cmd::WRITE_CONFIG),
            216usize.div_ceil(header_chunk)
        );
        assert_eq!(count_cmd(&sent, commands::cmd::ERASE_MACRO), 1);
        assert_eq!(count_cmd(&sent, commands::cmd::WRITE_MACRO), 1);
        assert_eq!(count_cmd(&sent, commands::cmd::SAVE), 2);

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
