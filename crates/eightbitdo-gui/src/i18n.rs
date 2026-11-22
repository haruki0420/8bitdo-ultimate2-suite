//! Centralized UI strings and language selection.
//!
//! Every user-visible string lives in this module. [`Key`] is an exhaustive
//! enum: the two `match` arms in [`Key::ja`] and [`Key::en`] are generated from
//! the same list, so adding a key without both translations is a compile error.
//!
//! Values that vary at runtime use `{}` placeholders and are filled by
//! [`format`]. [`placeholder_count`] lets tests assert that both languages
//! expect the same number of arguments.
//!
//! Japanese is the default language; English is the fallback for users who
//! switch the language in the top bar.

use eightbitdo_protocol::config_bt2::{GamepadMode, Slot};
use eightbitdo_protocol::keymap::KeyMap;

use crate::model::BUTTON_LABELS;

/// UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    /// Japanese (default).
    #[default]
    Ja,
    /// English fallback.
    En,
}

macro_rules! define_keys {
    ($($variant:ident => $ja:literal | $en:literal),* $(,)?) => {
        /// Every user-visible string in the GUI.
        ///
        /// The list is defined once; the Japanese and English `match` arms are
        /// generated from it, so a missing translation cannot compile.
        #[allow(missing_docs)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Key {
            $($variant),*
        }

        impl Key {
            /// Every key, in declaration order. Used by completeness tests.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            fn ja(self) -> &'static str {
                match self {
                    $(Self::$variant => $ja),*
                }
            }

            fn en(self) -> &'static str {
                match self {
                    $(Self::$variant => $en),*
                }
            }
        }
    };
}

define_keys! {
    // Window and top bar.
    WindowTitle => "8BitDo Ultimate 2 - 読み取り専用" | "8BitDo Ultimate 2 - Read-only",
    AppTitle => "8BitDo Ultimate 2" | "8BitDo Ultimate 2",
    ReadOnly => "読み取り専用" | "Read-only",
    ReadOnlyHint => "このビルドは読み取り専用です。書き込み・マクロ保存・復元・OTA は含まれません。" | "This build is read-only. No writes, macro save, restore, or OTA are included.",
    Language => "表示言語" | "Language",
    LangJapanese => "日本語" | "日本語",
    LangEnglish => "English" | "English",

    // Connection states.
    StatusDisconnected => "未接続" | "Disconnected",
    StatusScanning => "スキャン中" | "Scanning",
    StatusConnecting => "接続中" | "Connecting",
    StatusConnected => "接続済み" | "Connected",
    Error => "エラー" | "Error",
    ErrorDetails => "詳細" | "Details",
    Dismiss => "閉じる" | "Dismiss",
    Copy => "コピー" | "Copy",
    Clear => "クリア" | "Clear",

    // Busy indicators.
    BusyScanning => "BLE デバイスをスキャンしています…" | "Scanning for BLE devices…",
    BusyConnecting => "接続しています…" | "Connecting…",
    BusyReconnecting => "再接続しています…" | "Reconnecting…",
    BusyInitialRead => "バージョンと設定を読み取っています…" | "Reading version and configuration…",
    BusyReadVersion => "バージョンを読み取っています…" | "Reading version…",
    BusyReadConfig => "設定を読み取っています…" | "Reading configuration…",
    BusyBackup => "設定を読み取ってバックアップしています…" | "Reading and writing backup…",
    BusyDisconnecting => "切断しています…" | "Disconnecting…",

    // Section labels.
    NavDevice => "デバイス" | "Device",
    NavInput => "ライブ入力" | "Live input",
    NavConfig => "設定" | "Configuration",
    NavLog => "ログ" | "Log",
    SectionConnection => "接続" | "Connection",
    SectionActions => "読み取り専用の操作" | "Read-only actions",
    SectionBackupFolder => "保存先" | "Backup folder",
    SectionSafety => "安全上の注意" | "Safety",
    SectionDeviceInfo => "接続情報" | "Link",
    SectionFirmware => "ファームウェア" | "Firmware",
    SectionProfiles => "プロファイル" | "Profiles",
    SectionMacros => "マクロヘッダー" | "Macro headers",
    SectionGettingStarted => "はじめかた" | "Getting started",
    SectionScanResults => "スキャン結果" | "Scan results",
    SectionSummary => "設定の概要" | "Summary",

    // Connection controls and onboarding.
    Connect => "接続" | "Connect",
    Reconnect => "再接続" | "Reconnect",
    Scan => "スキャン" | "Scan",
    Disconnect => "切断" | "Disconnect",
    AutoDetect80nd => "80ND を自動検出" | "Any 80ND",
    SelectedDevice => "選択中: {}" | "Selected: {}",
    ConnectUsesSelected => "選択したデバイスに接続します。" | "Connects to the selected device.",
    NotAppMode => "選択したデバイスは App Mode ではありません。接続時は 80ND を名前で検索します。" | "The selected device is not in App Mode; Connect falls back to 80ND by name.",
    NoScanResults => "まだスキャン結果がありません。スキャンするか、80ND を自動検出して接続してください。" | "No scan results yet. Scan, or connect by name.",
    ScanHint => "周辺の BLE デバイスを検索します。" | "Looks for nearby BLE devices.",
    AppModeBadge => "App Mode" | "App Mode",
    OtherModeBadge => "通常モード" | "Other mode",
    DeviceRowHint => "行を選んで「接続」を押します。ダブルクリックでも接続できます。" | "Select a row and press Connect, or double-click the row.",
    ConnectFirstHint => "接続後に使えます。" | "Available after connecting.",
    BusyHint => "処理中は操作できません。" | "Unavailable while an operation runs.",
    NoDeviceHint => "接続するとデバイス情報が表示されます。" | "Device information appears after connecting.",
    Name => "名前" | "Name",
    Address => "アドレス" | "Address",
    Mtu => "MTU" | "MTU",
    Characteristic => "キャラクタリスティック" | "Characteristic",
    OnboardingBody => "コントローラを App Mode にして接続します。接続すると、デバイス情報・ライブ入力・設定を読み取り専用で表示できます。" | "Put the controller in App Mode and connect. Device information, live input, and configuration are then shown read-only.",
    Step1Title => "USB ケーブルを抜く" | "Unplug the USB cable",
    Step1Body => "有線接続を解除し、Bluetooth で使える状態にします。" | "Disconnect the cable so the controller can be used over Bluetooth.",
    Step2Title => "Profile ボタンを 3 秒長押し" | "Hold Profile for 3 seconds",
    Step2Body => "コントローラを App Mode（80ND）に切り替えます。" | "Switch the controller into App Mode (80ND).",
    Step3Title => "一覧から 80ND を選んで接続" | "Pick 80ND from the list and connect",
    Step3Body => "スキャン結果から 80ND を選び、「接続」を押します。" | "Select 80ND in the scan results and press Connect.",

    // Read-only actions.
    ReadVersionAction => "バージョンを読み取る" | "Read version",
    ReadConfigAction => "設定を読み取る" | "Read configuration",
    BackupConfigAction => "設定をバックアップ" | "Backup config",
    BackupConfigHint => "設定 2768 バイトの raw と JSON サマリを保存します。" | "Saves the 2768-byte raw block and a JSON summary.",
    StartLiveAction => "ライブ入力を開始" | "Start live input",
    StopLiveAction => "ライブ入力を停止" | "Stop live input",
    LiveStoppedHint => "ライブ入力は停止中です。keepalive も停止しています。" | "Live input is stopped; the keepalive is paused.",
    HintReadVersion => "現在のファームウェアバージョンを読み取ります。" | "Reads the current firmware version.",
    HintReadConfig => "設定 2768 バイトを読み取って表示します。" | "Reads and shows the 2768-byte configuration.",
    HintLive => "ライブ入力を停止すると keepalive も止まります。" | "Stopping live input also pauses the keepalive.",
    HintDisconnect => "接続を切断します。" | "Closes the current connection.",
    LastBackup => "直前のバックアップ" | "Last backup",
    CopySha256 => "SHA-256 をコピー" | "Copy SHA-256",
    SafetyText => "このビルドは読み取り専用です。設定の書き込み、マクロの消去・保存、バックアップからの復元、ファームウェア更新は実装していません。" | "Read-only build. No settings writes, macro erase/save, restore, or firmware update are implemented.",

    // Device and firmware view.
    NoDeviceInfo => "デバイス情報はまだありません。" | "No device information yet.",
    NoVersionYet => "まだバージョンを読み取っていません。" | "No version read yet.",
    FirmwareRaw => "受信データ" | "Raw payload",
    FirmwareText => "テキスト" | "Text",
    FirmwareValue => "値" | "Value",
    NotFourByteValue => "4 バイト値ではありません" | "Not a 4-byte value",
    EmptyValue => "未取得" | "empty",
    NoName => "名前なし" | "<no name>",
    None => "なし" | "None",
    EmptyProfileName => "未設定" | "(empty)",

    // Live input.
    NoInputYet => "入力レポートはまだありません。接続すると自動でライブ入力を開始します。" | "No input report yet. Live input starts automatically after connecting.",
    InputWaitingReports => "入力レポートを待っています。コントローラを操作すると表示が更新されます。" | "Waiting for input reports. Operate the controller to see updates.",
    InputWaitingConnect => "接続するとライブ入力が表示されます。" | "Connect to see live input.",
    ReportsRate => "受信レート: {} 回/秒" | "Reports: {}/s",
    LastReportAgo => "最終受信: {} ms 前" | "Last report: {} ms ago",
    BatteryRaw => "バッテリー（生値）: {}" | "Battery raw: {}",
    LeftStick => "左スティック" | "Left stick",
    RightStick => "右スティック" | "Right stick",
    TriggerL => "L2 / ZL: {} / 255" | "L2 / ZL: {} / 255",
    TriggerR => "R2 / ZR: {} / 255" | "R2 / ZR: {} / 255",
    Buttons => "ボタン" | "Buttons",
    NormalizedLeft => "左スティック（正規化）" | "Left stick (normalized)",
    NormalizedRight => "右スティック（正規化）" | "Right stick (normalized)",
    KeyMask => "キーマスク" | "Key mask",

    // Configuration view.
    NoConfigYet => "設定スナップショットはまだありません。接続すると自動で読み取ります。" | "No configuration snapshot yet. It is read automatically after connecting.",
    ConfigWaitingConnect => "接続すると設定が表示されます。" | "Connect to see the configuration.",
    LastSessionWarning => "前回のセッションの内容です（再接続で更新）。" | "Showing the last session; reconnect to refresh.",
    LastSessionHint => "再接続すると最新の内容に更新されます。" | "Reconnect to refresh this view.",
    SizeLabel => "サイズ" | "Size",
    BytesValue => "{} バイト" | "{} bytes",
    CrcStored => "保存 CRC" | "CRC stored",
    CrcInPlace => "計算 CRC" | "CRC in place",
    CrcMatch => "一致" | "match",
    CrcMismatch => "不一致（参考）" | "mismatch (informational)",
    CrcNote => "CRC は参考値です。公式の in-place 方式は保存 CRC 領域を含みます。" | "CRC is informational: the official in-place algorithm includes the stored CRC field.",
    ActiveProfile => "有効なプロファイル" | "Active profile",
    GamepadMode => "ゲームパッドモード" | "Gamepad mode",
    GamepadModeWithRaw => "{}（raw {}）" | "{} (raw {})",
    ProfileFlags => "プロファイルフラグ" | "Profile flags",
    ProfileWithRaw => "プロファイル {}（raw {}）" | "Profile {} (raw {})",
    ProfileNoneWithRaw => "なし（raw {}）" | "None (raw {})",
    ProfileUnknownWithRaw => "不明（raw {}）" | "Unknown (raw {})",
    ModeUnknownRaw => "不明（raw {}）" | "unknown (raw {})",
    ProfileLabel => "プロファイル {}" | "Profile {}",
    MacroHeaderHint => "このビルドはヘッダーのみ読み取ります。マクロのステップは取得しません。" | "Only headers are read. Macro steps are not downloaded.",
    MacroSlotLabel => "プロファイル {}（フラグ {}、マクロ {}）" | "Profile {} (flag {}, macros {})",
    ProfileRow => "{}: {} ({})" | "{}: {} ({})",
    IntervalMs => "{} ms" | "{} ms",
    StickRaw => "x={} y={}" | "x={} y={}",
    MtuValue => "MTU {} / {}" | "MTU {} / {}",
    TableNo => "No." | "#",
    TableName => "名前" | "Name",
    TableMode => "モード" | "Mode",
    TableSpecial => "特殊" | "Special",
    TableSteps => "ステップ数" | "Steps",
    TableOffset => "オフセット" | "Offset",
    TableKeyMap => "キーマップ" | "Key map",
    TableCycles => "回数" | "Cycles",
    TableInterval => "間隔" | "Interval",

    // Gamepad modes.
    ModeSwitch => "Switch" | "Switch",
    ModeGamepadJM => "ゲームパッド J/M" | "Gamepad J/M",
    ModeMac => "Mac" | "Mac",
    ModeXInput => "XInput" | "XInput",
    ModeExtended => "Extended" | "Extended",
    ModeUnselected => "未選択" | "Unselected",

    // Button names that are not engraved on the device.
    ButtonSelect => "セレクト" | "Select",
    ButtonStart => "スタート" | "Start",
    ButtonShare => "シェア" | "Share",
    ButtonLogo => "ロゴ" | "Logo",
    ButtonHome => "ホーム" | "Home",
    ButtonTurbo => "ターボ" | "Turbo",
    ButtonTurboAuto => "ターボ連射" | "Turbo Auto",
    ButtonScreenshot => "スクリーンショット" | "Screenshot",

    // Key masks.
    KeyNone => "なし" | "None",
    KeyUnknown => "不明({})" | "Unknown({})",

    // Log.
    LogEmpty => "ログはまだありません。" | "No log entries yet.",
    LogScanStarted => "スキャンを開始しました。" | "Scan started.",
    LogScanFinished => "スキャン完了: {} 件（App Mode {} 件）。" | "Scan finished: {} device(s), {} App Mode.",
    LogConnecting => "接続しています: {}" | "Connecting to {}…",
    LogConnected => "接続しました。" | "Connected.",
    LogVersion => "バージョン: {}" | "Version: {}",
    LogConfig => "設定: {} バイト、SHA-256 {}" | "Configuration: {} bytes, SHA-256 {}",
    LogLiveStarted => "ライブ入力を開始しました。" | "Live input started.",
    LogLiveStopped => "ライブ入力を停止しました。" | "Live input stopped.",
    LogBackup => "バックアップを保存しました: {}（SHA-256 {}）" | "Backup written: {} (SHA-256 {})",
    LogDisconnected => "切断しました。" | "Disconnected.",
    LogWorkerStopped => "バックグラウンドワーカーが停止しました。アプリを再起動してください。" | "The background worker stopped. Restart the application.",
    LogAppNotRunning => "バックグラウンドワーカーが動作していません。アプリを再起動してください。" | "The background worker is not running. Restart the application.",

    // Worker errors (message text; technical detail is kept separately).
    ErrWorkerRuntime => "BLE ワーカーのランタイムを開始できませんでした。" | "Cannot start the BLE worker runtime.",
    ErrWorkerThread => "BLE ワーカースレッドを開始できませんでした。" | "Cannot start the BLE worker thread.",
    ErrConnectFirst => "先にコントローラへ接続してください。" | "Connect to the controller first.",
    ErrDisconnectBeforeScan => "再スキャンするには切断してください。" | "Disconnect before scanning again.",
    ErrAlreadyConnected => "すでに接続しています。" | "Already connected.",
    ErrScanFailed => "スキャンに失敗しました。" | "Scan failed.",
    ErrVersionReadFailed => "バージョンの読み取りに失敗しました。" | "Version read failed.",
    ErrConfigReadFailed => "設定の読み取りに失敗しました。" | "Configuration read failed.",
    ErrBackupFailed => "バックアップに失敗しました。" | "Backup failed.",
    ErrStopLiveFailed => "ライブ入力を安全に停止できませんでした。" | "Cannot stop live input cleanly.",
    ErrStartLiveFailed => "ライブ入力を開始できませんでした。" | "Cannot start live input.",
    ErrConnectionFailed => "接続に失敗しました。" | "Connection failed.",
    ErrVersionLostLink => "バージョンの読み取り中に接続が切れました。" | "Version read lost the link.",
    ErrConfigLostLink => "設定の読み取り中に接続が切れました。" | "Configuration read lost the link.",
    ErrBackupLostLink => "バックアップ中に接続が切れました。" | "Backup lost the link.",
    ErrLiveInputFailed => "ライブ入力に失敗しました。" | "Live input failed.",
    ErrLiveStoppedAfterFailures => "ライブ入力が {} 回失敗したため停止しました。" | "Live input stopped after {} failure(s).",
    ErrLiveStartLostLink => "ライブ入力の開始時に接続が切れました。" | "Live input could not start.",
    ErrLiveStopLostLink => "ライブ入力の停止時に接続が切れました。" | "Live input stop lost the link.",

    // Misc labels.
    Sha256 => "SHA-256" | "SHA-256",
    Warning => "警告" | "Warning",
}

/// Text for `key` in `lang`.
#[must_use]
pub fn t(lang: Lang, key: Key) -> &'static str {
    match lang {
        Lang::Ja => key.ja(),
        Lang::En => key.en(),
    }
}

/// Number of `{}` placeholders in `text`.
#[must_use]
pub fn placeholder_count(text: &str) -> usize {
    text.matches("{}").count()
}

/// Fill `{}` placeholders in `key`'s text with `args`, left to right.
///
/// Missing arguments leave the placeholder in place; extra arguments are
/// ignored. Tests assert that every key is formatted without leaving `{}`.
#[must_use]
pub fn format(lang: Lang, key: Key, args: &[String]) -> String {
    let text = t(lang, key);
    let mut output = String::with_capacity(text.len() + 8);
    let mut rest = text;
    let mut args = args.iter();
    while let Some(position) = rest.find("{}") {
        output.push_str(&rest[..position]);
        match args.next() {
            Some(argument) => output.push_str(argument),
            None => output.push_str("{}"),
        }
        rest = &rest[position + 2..];
    }
    output.push_str(rest);
    output
}

/// Display name of an engraving label from [`BUTTON_LABELS`].
///
/// Labels that are printed on the controller (A/B/X/Y, L1/R1, ...) stay in
/// English so the on-screen chips match the device. The rest are localized.
#[must_use]
pub fn button_name(lang: Lang, engraved: &'static str) -> &'static str {
    match engraved {
        "Select" => t(lang, Key::ButtonSelect),
        "Start" => t(lang, Key::ButtonStart),
        "Share" => t(lang, Key::ButtonShare),
        "Logo" => t(lang, Key::ButtonLogo),
        "Home" => t(lang, Key::ButtonHome),
        "Turbo" => t(lang, Key::ButtonTurbo),
        "Turbo Auto" => t(lang, Key::ButtonTurboAuto),
        "Screenshot" => t(lang, Key::ButtonScreenshot),
        other => BUTTON_LABELS
            .iter()
            .find(|(label, _)| *label == other)
            .map_or(other, |(label, _)| *label),
    }
}

/// Render a key mask as localized labels, with unknown bits kept visible.
#[must_use]
pub fn keymap_text(lang: Lang, map: KeyMap) -> String {
    if map.is_empty() {
        return t(lang, Key::KeyNone).to_string();
    }
    let mut parts = Vec::new();
    let mut known = 0u32;
    for (label, flag) in BUTTON_LABELS {
        if map.contains(flag) {
            parts.push(button_name(lang, label).to_string());
            known |= flag.bits();
        }
    }
    let unknown = map.bits() & !known;
    if unknown != 0 {
        parts.push(format(lang, Key::KeyUnknown, &[format!("0x{unknown:08X}")]));
    }
    parts.join(" + ")
}

/// Localized `gamepad_mode` label for a raw value.
#[must_use]
pub fn gamepad_mode_text(lang: Lang, raw: u16) -> String {
    match GamepadMode::try_from(raw) {
        Ok(GamepadMode::Switch) => t(lang, Key::ModeSwitch),
        Ok(GamepadMode::GamepadJM) => t(lang, Key::ModeGamepadJM),
        Ok(GamepadMode::Mac) => t(lang, Key::ModeMac),
        Ok(GamepadMode::XInput) => t(lang, Key::ModeXInput),
        Ok(GamepadMode::Extended) => t(lang, Key::ModeExtended),
        Ok(GamepadMode::Unselected) => t(lang, Key::ModeUnselected),
        Err(_) => {
            return format(lang, Key::ModeUnknownRaw, &[raw.to_string()]);
        }
    }
    .to_string()
}

/// Localized active-profile label for a raw `cur_slot` value.
#[must_use]
pub fn active_profile_text(lang: Lang, raw: u16) -> String {
    let raw_text = raw.to_string();
    match Slot::from_index(raw as usize) {
        Some(slot) => format(
            lang,
            Key::ProfileWithRaw,
            &[slot.number().to_string(), raw_text],
        ),
        None if raw == 4 => format(lang, Key::ProfileNoneWithRaw, &[raw_text]),
        None => format(lang, Key::ProfileUnknownWithRaw, &[raw_text]),
    }
}

/// Localized label for a one-based profile number.
#[must_use]
pub fn profile_text(lang: Lang, number: u8) -> String {
    format(lang, Key::ProfileLabel, &[number.to_string()])
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn japanese_is_the_default_language() {
        assert_eq!(Lang::default(), Lang::Ja);
        assert_eq!(t(Lang::Ja, Key::Connect), "接続");
        assert_eq!(t(Lang::En, Key::Connect), "Connect");
    }

    #[test]
    fn every_key_has_both_languages_and_matching_placeholders() {
        let mut seen = HashSet::new();
        for &key in Key::ALL {
            assert!(seen.insert(key), "{key:?} is listed twice");
            let ja = t(Lang::Ja, key);
            let en = t(Lang::En, key);
            assert!(!ja.trim().is_empty(), "{key:?} has empty Japanese text");
            assert!(!en.trim().is_empty(), "{key:?} has empty English text");
            assert_eq!(
                placeholder_count(ja),
                placeholder_count(en),
                "{key:?} has different placeholder counts: ja={ja:?} en={en:?}"
            );

            let args: Vec<String> = (0..placeholder_count(ja))
                .map(|index| format!("<arg{index}>"))
                .collect();
            let filled = format(Lang::Ja, key, &args);
            assert!(
                !filled.contains("{}"),
                "{key:?} left a placeholder after formatting: {filled:?}"
            );
        }
        assert_eq!(seen.len(), Key::ALL.len());
    }

    #[test]
    fn format_fills_placeholders_left_to_right() {
        let filled = format(
            Lang::Ja,
            Key::LogScanFinished,
            &["3".to_string(), "2".to_string()],
        );
        assert_eq!(filled, "スキャン完了: 3 件（App Mode 2 件）。");
        let english = format(
            Lang::En,
            Key::LogScanFinished,
            &["3".to_string(), "2".to_string()],
        );
        assert_eq!(english, "Scan finished: 3 device(s), 2 App Mode.");
        assert_eq!(
            format(Lang::Ja, Key::Connect, &[]),
            "接続",
            "keys without placeholders ignore arguments"
        );
    }

    #[test]
    fn button_names_localize_only_labels_not_engraved_on_the_device() {
        assert_eq!(button_name(Lang::Ja, "A"), "A");
        assert_eq!(button_name(Lang::Ja, "L1"), "L1");
        assert_eq!(button_name(Lang::Ja, "Select"), "セレクト");
        assert_eq!(button_name(Lang::Ja, "Turbo Auto"), "ターボ連射");
        assert_eq!(button_name(Lang::En, "Select"), "Select");
    }

    #[test]
    fn key_masks_use_localized_names_and_keep_unknown_bits() {
        assert_eq!(keymap_text(Lang::Ja, KeyMap::NONE), "なし");
        assert_eq!(
            keymap_text(Lang::Ja, KeyMap::A | KeyMap::SELECT),
            "A + セレクト"
        );
        assert_eq!(
            keymap_text(Lang::Ja, KeyMap::from_bits(1 << 31)),
            "不明(0x80000000)"
        );
        assert_eq!(keymap_text(Lang::En, KeyMap::NONE), "None");
    }

    #[test]
    fn gamepad_modes_and_profiles_are_localized() {
        assert_eq!(gamepad_mode_text(Lang::Ja, 0), "Switch");
        assert_eq!(gamepad_mode_text(Lang::Ja, 1), "ゲームパッド J/M");
        assert_eq!(gamepad_mode_text(Lang::Ja, 5), "未選択");
        assert_eq!(gamepad_mode_text(Lang::Ja, 9), "不明（raw 9）");
        assert_eq!(gamepad_mode_text(Lang::En, 1), "Gamepad J/M");

        assert_eq!(active_profile_text(Lang::Ja, 0), "プロファイル 1（raw 0）");
        assert_eq!(active_profile_text(Lang::Ja, 4), "なし（raw 4）");
        assert_eq!(active_profile_text(Lang::Ja, 9), "不明（raw 9）");
        assert_eq!(profile_text(Lang::Ja, 2), "プロファイル 2");
    }

    #[test]
    fn english_fallback_has_no_japanese_characters() {
        for &key in Key::ALL {
            if key == Key::LangJapanese {
                // Language names stay in their own script in both languages.
                continue;
            }
            let en = t(Lang::En, key);
            assert!(
                !en.chars().any(|character| {
                    matches!(character as u32,
                        0x3000..=0x30FF | 0x4E00..=0x9FFF | 0xFF00..=0xFFEF)
                }),
                "{key:?} English text contains Japanese: {en:?}"
            );
        }
    }
}
