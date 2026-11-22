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
//! switch the language in the top bar. Copy follows the design brief's
//! glossary: labels are noun phrases without a trailing `。`, body copy uses
//! です・ます, and errors end with the next action.

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
    ReadOnly => "読み取り専用ビルド" | "Read-only build",
    LangJapanese => "日本語" | "日本語",
    LangEnglish => "English" | "English",

    // Connection states.
    StatusDisconnected => "未接続" | "Disconnected",
    StatusScanning => "スキャン中" | "Scanning",
    StatusConnecting => "接続中" | "Connecting",
    StatusConnected => "接続済み" | "Connected",
    ErrorDetails => "詳細" | "Details",
    Dismiss => "閉じる" | "Dismiss",
    CopySha256 => "SHA-256 をコピー" | "Copy SHA-256",
    Clear => "ログを消す" | "Clear log",

    // Busy indicators.
    BusyScanning => "スキャンしています…" | "Scanning…",
    BusyConnecting => "接続しています…" | "Connecting…",
    BusyReconnecting => "再接続しています…" | "Reconnecting…",
    BusyInitialRead => "バージョンと設定値を読み取っています…" | "Reading version and configuration…",
    BusyReadVersion => "バージョンを読み取っています…" | "Reading version…",
    BusyReadConfig => "設定値を読み取っています…" | "Reading configuration…",
    BusyBackup => "設定値を読み取って書き出しています…" | "Reading and writing the backup…",
    BusyDisconnecting => "切断しています…" | "Disconnecting…",

    // Section labels.
    NavDevice => "デバイス" | "Device",
    NavInput => "ライブ入力" | "Live input",
    NavConfig => "設定値" | "Configuration",
    NavLog => "ログ" | "Log",
    SectionDevice => "デバイス" | "Device",
    SectionActions => "読み取り操作" | "Read-only actions",
    SectionBackupFolder => "バックアップの保存先" | "Backup folder",
    SectionSafety => "安全上の注意" | "Safety",
    SectionDeviceInfo => "接続情報" | "Connection info",
    SectionFirmware => "ファームウェア" | "Firmware",
    SectionProfiles => "プロファイル" | "Profiles",
    SectionMacros => "マクロヘッダー" | "Macro headers",
    SectionScanResults => "デバイス一覧" | "Device list",
    SectionSummary => "設定値の概要" | "Summary",
    SectionNextSteps => "次の一手" | "Next step",
    SectionConnectSteps => "接続の手順" | "Connection steps",
    FooterMeta => "読み取り専用 / 設定 2768 バイト / マクロ 4×3" | "Read-only / 2768-byte config / 4×3 macros",

    // Connection controls and onboarding.
    Connect => "80ND に接続" | "Connect to 80ND",
    Reconnect => "再接続" | "Reconnect",
    Scan => "スキャン" | "Scan",
    Disconnect => "切断" | "Disconnect",
    SelectedDevice => "接続先 {}" | "Target {}",
    ConnectUsesSelected => "一覧で選んだデバイスに接続します。" | "Connects to the device selected in the list.",
    NotAppMode => "選んだデバイスは App Mode ではありません。80ND を名前で探します。" | "The selected device is not in App Mode; Connect looks for 80ND by name.",
    NoScanResults => "まだ見つかっていません。「スキャン」を押すと、近くの 80ND が一覧に出ます。" | "Nothing found yet. Press Scan to list nearby 80ND controllers.",
    ScanHint => "周辺の BLE デバイスをスキャンします。" | "Scans for nearby BLE devices.",
    AppModeBadge => "App Mode" | "App Mode",
    OtherModeBadge => "通常モード" | "Other mode",
    DeviceRowHint => "行を選ぶと接続先になります。ダブルクリックでも接続できます。" | "Select a row to set the target; double-click also connects.",
    ConnectFirstHint => "接続すると使えるようになります。" | "Available after connecting.",
    BusyHint => "処理が終わると使えます。" | "Available when the current operation finishes.",
    NoDeviceHint => "接続すると、名前・アドレス・MTU が表示されます。" | "Connecting shows the name, address, and MTU.",
    Name => "名前" | "Name",
    Address => "アドレス" | "Address",
    Mtu => "MTU" | "MTU",
    Characteristic => "キャラクタリスティック" | "Characteristic",
    Step1Title => "USB ケーブルを抜く" | "Unplug the USB cable",
    Step1Body => "有線接続を解除し、Bluetooth で使える状態にします。" | "Disconnect the cable so the controller can be used over Bluetooth.",
    Step2Title => "Profile ボタンを 3 秒間押し続ける" | "Hold Profile for 3 seconds",
    Step2Body => "コントローラを App Mode（80ND）に切り替えます。" | "Switch the controller into App Mode (80ND).",
    Step3Title => "80ND を選んで接続する" | "Select 80ND and connect",
    Step3Body => "デバイス一覧から 80ND を選び、「80ND に接続」を押します。" | "Select 80ND in the device list and press Connect to 80ND.",

    // Read-only actions.
    ReadVersionAction => "バージョンを読み取る" | "Read version",
    ReadConfigAction => "設定値を読み取る" | "Read configuration",
    BackupConfigAction => "設定を書き出す" | "Write a backup",
    BackupConfigHint => "設定 2768 バイトの raw と JSON を書き出します。" | "Writes the 2768-byte raw block and a JSON summary.",
    StartLiveAction => "ライブ入力を開始" | "Start live input",
    StopLiveAction => "ライブ入力を停止" | "Stop live input",
    LiveStoppedHint => "ライブ入力は停止しています。" | "Live input is stopped.",
    HintReadVersion => "ファームウェアのバージョンを読み取ります。" | "Reads the firmware version.",
    HintReadConfig => "設定 2768 バイトを読み取ります。" | "Reads the 2768-byte configuration.",
    HintLive => "ライブ入力を止めると、接続維持の通信も止まります。" | "Stopping live input also pauses the keepalive.",
    HintDisconnect => "接続を切断します。" | "Closes the current connection.",
    LastBackup => "直前のバックアップ" | "Last backup",
    SafetyText => "設定の書き込み、マクロの保存、バックアップの復元、ファームウェア更新は行いません。" | "It does not write settings, save macros, restore backups, or update firmware.",

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
    NoInputYet => "接続すると、入力がここに流れます。" | "Input appears here after connecting.",
    InputWaitingReports => "コントローラを操作すると、スティックとボタンの状態が表示されます。" | "Operate the controller to see the sticks and buttons.",
    InputWaitingConnect => "スティックとボタンの状態が表示されます。" | "Stick and button state appears here.",
    ReportsRate => "受信レート {} 回/秒" | "Reports {} per second",
    LastReportAgo => "最終受信 {} ms 前" | "Last report {} ms ago",
    BatteryRaw => "バッテリー生値 {}" | "Battery raw {}",
    LeftStick => "左スティック" | "Left stick",
    RightStick => "右スティック" | "Right stick",
    TriggerL => "L2 / ZL {} / 255" | "L2 / ZL {} / 255",
    TriggerR => "R2 / ZR {} / 255" | "R2 / ZR {} / 255",
    Buttons => "ボタン" | "Buttons",
    NormalizedLeft => "左スティック（正規化）" | "Left stick (normalized)",
    NormalizedRight => "右スティック（正規化）" | "Right stick (normalized)",
    KeyMask => "キーマスク" | "Key mask",

    // Configuration view.
    NoConfigYet => "接続すると、設定 2768 バイトを読み取ります。" | "Connecting reads the 2768-byte configuration.",
    ConfigWaitingConnect => "プロファイルとマクロヘッダーを表示します。" | "Shows profiles and macro headers.",
    LastSessionWarning => "前回の内容を表示しています。再接続で更新されます。" | "Showing content from the last session. Reconnect to refresh.",
    SizeLabel => "サイズ" | "Size",
    BytesValue => "{} バイト" | "{} bytes",
    CrcStored => "保存 CRC" | "CRC stored",
    CrcInPlace => "計算 CRC" | "CRC in place",
    CrcMatch => "一致" | "match",
    CrcMismatch => "不一致" | "mismatch",
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
    MacroHeaderHint => "マクロのヘッダーだけを読み取ります。ステップは取得しません。" | "Only macro headers are read; steps are not downloaded.",
    MacroSlotLabel => "プロファイル {}（フラグ {}、マクロ {}）" | "Profile {} (flag {}, macros {})",
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
    LogEmpty => "まだ記録がありません。" | "No entries yet.",
    LogEmptyBody => "接続や読み取りの結果がここに残ります。" | "Connection and read results appear here.",
    LogScanStarted => "スキャンを開始しました。" | "Scan started.",
    LogScanFinished => "デバイス {} 件、App Mode {} 件。" | "{} device(s); {} in App Mode.",
    LogConnecting => "{} に接続しています…" | "Connecting to {}…",
    LogConnected => "接続しました。" | "Connected.",
    LogVersion => "バージョン {}" | "Version {}",
    LogConfig => "設定値 {} バイト、SHA-256 {}" | "Configuration {} bytes, SHA-256 {}",
    LogLiveStarted => "ライブ入力を開始しました。" | "Live input started.",
    LogLiveStopped => "ライブ入力を停止しました。" | "Live input stopped.",
    LogBackup => "設定を書き出しました。{} / SHA-256 {}" | "Backup written: {} / SHA-256 {}",
    LogDisconnected => "切断しました。" | "Disconnected.",
    LogWorkerStopped => "バックグラウンド処理が停止しました。アプリを再起動してください。" | "The background worker stopped. Restart the application.",
    LogAppNotRunning => "バックグラウンド処理が動作していません。アプリを再起動してください。" | "The background worker is not running. Restart the application.",

    // Worker errors: headline only; technical detail is separate.
    ErrWorkerRuntime => "BLE の処理を開始できませんでした。" | "Could not start the BLE worker runtime.",
    ErrWorkerThread => "BLE の処理を開始できませんでした。" | "Could not start the BLE worker thread.",
    ErrConnectFirst => "先にコントローラに接続してください。" | "Connect to the controller first.",
    ErrDisconnectBeforeScan => "スキャンするには、いったん切断してください。" | "Disconnect before scanning again.",
    ErrAlreadyConnected => "すでに接続しています。" | "Already connected.",
    ErrScanFailed => "スキャンできませんでした。" | "Could not scan.",
    ErrVersionReadFailed => "バージョンを読み取れませんでした。" | "Could not read the version.",
    ErrConfigReadFailed => "設定値を読み取れませんでした。" | "Could not read the configuration.",
    ErrBackupFailed => "設定を書き出せませんでした。" | "Could not write the backup.",
    ErrStopLiveFailed => "ライブ入力を停止できませんでした。" | "Could not stop live input.",
    ErrStartLiveFailed => "ライブ入力を開始できませんでした。" | "Could not start live input.",
    ErrConnectionFailed => "接続できませんでした。" | "Could not connect.",
    ErrVersionLostLink => "バージョンの読み取り中に接続が切れました。" | "The link dropped while reading the version.",
    ErrConfigLostLink => "設定値の読み取り中に接続が切れました。" | "The link dropped while reading the configuration.",
    ErrBackupLostLink => "設定の書き出し中に接続が切れました。" | "The link dropped while writing the backup.",
    ErrLiveInputFailed => "ライブ入力が途切れました。" | "Live input failed.",
    ErrLiveStoppedAfterFailures => "ライブ入力が {} 回失敗したため停止しました。" | "Live input stopped after {} failures.",
    ErrLiveStartLostLink => "ライブ入力の開始時に接続が切れました。" | "The link dropped when live input started.",
    ErrLiveStopLostLink => "ライブ入力の停止時に接続が切れました。" | "The link dropped when live input stopped.",

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
        assert_eq!(t(Lang::Ja, Key::Connect), "80ND に接続");
        assert_eq!(t(Lang::En, Key::Connect), "Connect to 80ND");
        assert_eq!(t(Lang::Ja, Key::NavConfig), "設定値");
        assert_eq!(t(Lang::Ja, Key::SectionActions), "読み取り操作");
        assert_eq!(t(Lang::Ja, Key::SectionDevice), "デバイス");
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
        assert_eq!(filled, "デバイス 3 件、App Mode 2 件。");
        let english = format(
            Lang::En,
            Key::LogScanFinished,
            &["3".to_string(), "2".to_string()],
        );
        assert_eq!(english, "3 device(s); 2 in App Mode.");
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

    #[test]
    fn labels_stay_short_and_unpunctuated() {
        for key in [
            Key::Connect,
            Key::Reconnect,
            Key::Scan,
            Key::Disconnect,
            Key::NavDevice,
            Key::NavInput,
            Key::NavConfig,
            Key::NavLog,
            Key::ReadVersionAction,
            Key::ReadConfigAction,
            Key::BackupConfigAction,
            Key::StartLiveAction,
            Key::StopLiveAction,
            Key::Clear,
            Key::ErrorDetails,
            Key::Dismiss,
            Key::CopySha256,
        ] {
            let ja = t(Lang::Ja, key);
            assert!(!ja.ends_with('。'), "{key:?} ends with a period: {ja}");
            assert!(!ja.contains('：'), "{key:?} uses a full-width colon: {ja}");
            assert!(
                ja.chars().count() <= 12,
                "{key:?} is longer than 12 characters: {ja}"
            );
        }
    }

    #[test]
    fn copy_does_not_use_full_width_colons() {
        for &key in Key::ALL {
            assert!(
                !t(Lang::Ja, key).contains('：'),
                "{key:?} uses a full-width colon: {:?}",
                t(Lang::Ja, key)
            );
        }
    }
}
