//! Centralized UI strings and language selection.
//!
//! Every user-visible string lives in this module. [`Key`] is an exhaustive
//! enum: the two `match` arms in `Key::ja` and `Key::en` are generated from
//! the same list, so adding a key without both translations is a compile error.
//!
//! Values that vary at runtime use `{}` placeholders and are filled by
//! `format!`. [`placeholder_count`] lets tests assert that both languages
//! expect the same number of arguments.
//!
//! Japanese is the default language; English is the fallback for users who
//! switch the language in the top bar. Copy follows the design brief's
//! glossary: labels are noun phrases without a trailing `。`, body copy uses
//! です・ます, and errors end with the next action.

use eightbitdo_protocol::config_bt2::{GamepadMode, Slot, SpecialFeature};
use eightbitdo_protocol::keymap::KeyMap;

use crate::model::BUTTON_LABELS;
use crate::write::{ChangeKind, DraftError, MacroChange, MAX_NAME_UNITS};

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
    WindowTitle => "8BitDo Ultimate 2" | "8BitDo Ultimate 2",
    AppTitle => "8BitDo Ultimate 2" | "8BitDo Ultimate 2",
    WriteCapable => "書き込み対応ビルド" | "Write-capable build",
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
    BusyReadMacro => "マクロを読み取っています…" | "Reading the macro…",
    BusyWriteConfig => "設定値を書き込んで検証しています…" | "Writing and verifying the configuration…",
    BusyWriteMacro => "マクロを書き込んで検証しています…" | "Writing and verifying the macro…",

    // Section labels.
    NavDevice => "デバイス" | "Device",
    NavInput => "ライブ入力" | "Live input",
    NavConfig => "設定値" | "Configuration",
    NavSettings => "設定" | "Settings",
    NavMacro => "マクロ" | "Macro",
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
    SectionWriteSettings => "設定の書き込み" | "Write settings",
    SectionProfileSettings => "プロファイル設定" | "Profile settings",
    SectionRumble => "振動" | "Rumble",
    SectionSticks => "スティック" | "Sticks",
    SectionTriggers => "トリガー" | "Triggers",
    SectionSpecial => "特殊機能" | "Special features",
    SectionGyro => "ジャイロ" | "Gyro",
    SectionLed => "LED" | "LED",
    SectionRemap => "ボタン割り当て" | "Button remapping",
    SectionMacroEditor => "マクロ編集" | "Macro editor",
    SectionMacroHeader => "マクロヘッダー" | "Macro header",
    SectionMacroSteps => "ステップ" | "Steps",
    SectionWriteResult => "書き込み結果" | "Write result",
    SectionNextSteps => "次の一手" | "Next step",
    SectionConnectSteps => "接続の手順" | "Connection steps",
    FooterMeta => "書き込み対応 / 設定 2768 バイト / マクロ 4×3" | "Write-capable / 2768-byte config / 4×3 macros",
    OpenLicenses => "ライセンス" | "Licenses",
    LicensesTitle => "ライセンス / オープンソース" | "Licenses / open source",
    LicensesIntro => "このアプリと同梱フォントのライセンス全文です。依存クレートの完全な第三者通知はリポジトリの THIRD_PARTY_LICENSES.md と cargo-about を参照してください。" | "Full license texts for this application and its bundled fonts. For complete third-party notices of the dependency crates, see THIRD_PARTY_LICENSES.md and cargo-about in the repository.",
    LicensesAppHeading => "このアプリ" | "This application",
    LicensesAppBody => "eightbitdo-suite は MIT License または Apache License 2.0 のいずれかを選択して利用できます（MIT OR Apache-2.0）。全文はリポジトリの LICENSE-MIT / LICENSE-APACHE にあります。" | "eightbitdo-suite is available under either the MIT License or the Apache License 2.0 (MIT OR Apache-2.0). The full texts are in LICENSE-MIT / LICENSE-APACHE in the repository.",
    LicensesNotoHeading => "Noto Sans JP (subset) — 同梱フォント" | "Noto Sans JP (subset) — bundled font",
    LicensesNotoNote => "SIL Open Font License 1.1。サブセットを同梱しています。" | "SIL Open Font License 1.1. A subset is bundled.",
    LicensesEguiHeading => "egui 既定フォント" | "egui default fonts",
    LicensesEguiNote => "NotoEmoji（OFL 1.1）、Ubuntu-Light（Ubuntu Font Licence 1.0）、Hack（MIT + Bitstream Vera + DejaVu）、emoji-icon-font（MIT）。各全文を以下に収録しています。" | "NotoEmoji (OFL 1.1), Ubuntu-Light (Ubuntu Font Licence 1.0), Hack (MIT + Bitstream Vera + DejaVu), emoji-icon-font (MIT). The full texts follow.",
    LicensesClose => "閉じる" | "Close",

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
    SafetyText => "書き込みは確認後だけ。直前にバックアップし、read-back で検証します。" | "Writes only after confirmation; backup first, then read-back verification.",

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
    ButtonP1 => "P1 / PL" | "P1 / PL",
    ButtonP2 => "P2 / PR" | "P2 / PR",
    ButtonP3 => "P3 / L4" | "P3 / L4",
    ButtonP4 => "P4 / R4" | "P4 / R4",

    // Key masks.
    KeyNone => "なし" | "None",
    KeyUnknown => "不明({})" | "Unknown({})",

    // Write settings.
    ProfileNameLabel => "プロファイル名" | "Profile name",
    ProfileNameHint => "16 文字まで（UTF-16）" | "Up to 16 UTF-16 units",
    ProfileNoSelection => "プロファイルなし" | "No profile",
    RumbleEnabled => "カスタム振動" | "Custom rumble",
    RumbleLeft => "左モーター" | "Left motor",
    RumbleRight => "右モーター" | "Right motor",
    StickEnabled => "カスタムスティック" | "Custom sticks",
    StickLeftStart => "左スティック 開始" | "Left stick start",
    StickLeftEnd => "左スティック 終了" | "Left stick end",
    StickRightStart => "右スティック 開始" | "Right stick start",
    StickRightEnd => "右スティック 終了" | "Right stick end",
    TriggerEnabled => "カスタムトリガー" | "Custom triggers",
    TriggerLeftStart => "L2 開始" | "L2 start",
    TriggerLeftEnd => "L2 終了" | "L2 end",
    TriggerRightStart => "R2 開始" | "R2 start",
    TriggerRightEnd => "R2 終了" | "R2 end",
    SpecialEnabled => "特殊機能を有効化" | "Enable special features",
    SpecialLxFlip => "左スティック X 反転" | "Left stick X invert",
    SpecialLyFlip => "左スティック Y 反転" | "Left stick Y invert",
    SpecialRxFlip => "右スティック X 反転" | "Right stick X invert",
    SpecialRyFlip => "右スティック Y 反転" | "Right stick Y invert",
    SpecialStickSwap => "スティック入れ替え" | "Swap sticks",
    SpecialTriggerSwap => "トリガー入れ替え" | "Swap triggers",
    SpecialDpadSwap => "十字キー入れ替え" | "Swap D-pad",
    SpecialRsTriggerFlip => "右スティック/トリガー反転" | "Right stick / trigger invert",
    SpecialMotionSensitivity => "モーション感度" | "Motion sensitivity",
    SpecialZeroDeadzone => "デッドゾーンなし" | "No dead zone",
    SpecialFourWay => "十字キー 4 方向" | "4-way D-pad",
    SpecialTriggerVibration => "トリガー振動" | "Trigger vibration",
    SpecialVibration => "本体振動" | "Body vibration",
    GyroEnabled => "ジャイロを有効化" | "Enable gyro",
    GyroTriggerMode => "作動方法" | "Activation",
    GyroHold => "押している間" | "Hold",
    GyroToggle => "切り替え" | "Toggle",
    GyroSensitivity => "感度" | "Sensitivity",
    GyroDeadCompensate => "デッドゾーン補正" | "Dead-zone compensation",
    GyroMapping => "出力先" | "Output",
    GyroMouse => "マウス" | "Mouse",
    GyroButton => "作動ボタン" | "Activation button",
    LedEnabled => "有効" | "Enabled",
    LedDynamic => "ダイナミック" | "Dynamic",
    LedHotWheel => "ホットホイール" | "Hot wheel",
    LedSingle => "単色" | "Single color",
    LedForeground => "前景色" | "Foreground",
    LedBackground => "背景色" | "Background",
    LedSpeed => "速度" | "Speed",
    LedAllColor => "全 LED の色" | "All LED color",
    RemapEnabled => "リマップを有効化" | "Enable remapping",
    RemapPhysical => "物理ボタン" | "Physical button",
    RemapTarget => "割り当て先" | "Assigned to",
    RemapUnsupported => "0x{}（選択不可）" | "0x{} (not selectable)",
    RemapPreserved => "0x{} は未変更のため保持します" | "Keeping 0x{} unchanged",
    RemapPreservedCount => "未変更の選択不可値 {} 件を保持します" | "Keeping {} unchanged unsupported value(s)",
    RemapHint => "227・未知ビット・複合割り当ては選択肢に出ません。" | "227, unknown bits, and combined assignments are not offered.",
    SettingsNeedsConfig => "接続して設定値を読み取ると編集できます。" | "Connect and read the configuration to edit it.",
    SettingsStale => "前回読み取った設定を編集しています。書き込み直前に再読み取りして検証します。" | "Editing the last read configuration; it is re-read and re-validated immediately before writing.",
    SettingsDirtyHint => "変更は「変更を確認」から確認ダイアログを開いた後だけ書き込まれます。" | "Changes are written only after opening the confirmation dialog with Review changes.",
    RecordingBlocksWrite => "録音中は書き込めません。録音を停止してから確認してください。" | "Stop recording before writing.",
    ApplySettings => "変更を確認" | "Review changes",
    DiscardChanges => "変更を破棄" | "Discard changes",
    WriteConfirm => "書き込む" | "Write",
    WriteCancel => "キャンセル" | "Cancel",
    ConfirmTitle => "書き込みの確認" | "Confirm write",
    ConfirmBackupNote => "書き込みの直前にバックアップを作成し、書き込み後に read-back で検証します。" | "A backup is written immediately before the write, and the result is verified by read-back.",
    ConfirmConfigSummary => "設定値 2768 バイトを書き換えます。" | "Writes the 2768-byte configuration.",
    ConfirmMacroSummary => "プロファイル {} のマクロ {}（{} ステップ / {} ms）を書き換えます。" | "Writes profile {} macro {} ({} steps / {} ms).",
    ConfirmMacroHeader => "名前 {} / トリガー {} / 回数 {} / 間隔 {} ms" | "Name {} / trigger {} / cycles {} / interval {} ms",
    ConfirmBackupDir => "バックアップ先 {}" | "Backup folder {}",
    ConfirmChangeCount => "変更 {} 件" | "{} change(s)",
    ConfirmTarget => "対象" | "Target",
    ConfirmConfigTarget => "設定値（2768 バイト）" | "Configuration (2768 bytes)",
    ConfirmMacroTarget => "プロファイル {} / マクロ {}" | "Profile {} / macro {}",
    ConfirmDuration => "再生時間" | "Playback duration",
    ConfirmNoteReadback => "書き込み後に read-back で検証します。" | "The result is verified by read-back after writing.",
    ConfirmWarning => "失敗した場合、設定の一部だけが更新される可能性があります。表示されるバックアップを保管してください。" | "If it fails, part of the configuration may remain updated. Keep the backup shown after the write.",
    ChangeProfileName => "プロファイル {} の名前" | "Profile {} name",
    ChangeGamepadMode => "ゲームパッドモード" | "Gamepad mode",
    ChangeActiveProfile => "有効なプロファイル" | "Active profile",
    ChangeRumble => "プロファイル {} の振動" | "Profile {} rumble",
    ChangeSticks => "プロファイル {} のスティック" | "Profile {} sticks",
    ChangeTriggers => "プロファイル {} のトリガー" | "Profile {} triggers",
    ChangeSpecial => "プロファイル {} の特殊機能" | "Profile {} special features",
    ChangeGyro => "プロファイル {} のジャイロ" | "Profile {} gyro",
    ChangeLedTheme => "プロファイル {} の LED" | "Profile {} LED",
    ChangeRemapFlag => "プロファイル {} のリマップ有効化" | "Profile {} remapping flag",
    ChangeRemapButton => "プロファイル {} の {} 割り当て" | "Profile {} {} assignment",
    MacroChangeSteps => "ステップ" | "Steps",
    MacroChangeName => "名前" | "Name",
    MacroChangeTrigger => "トリガー" | "Trigger",
    MacroChangeCycles => "回数" | "Cycles",
    MacroChangeInterval => "間隔" | "Interval",
    MacroProfile => "プロファイル" | "Profile",
    MacroIndexLabel => "マクロ" | "Macro",
    MacroLoad => "マクロを読み取る" | "Read macro",
    MacroLoadHint => "選んだマクロのステップを読み取ります。" | "Reads the selected macro's steps.",
    MacroEmptyTarget => "空のマクロには書き込めません。既存のマクロを選んでください。" | "An empty macro cannot be written; choose an existing macro.",
    MacroNotLoaded => "マクロを読み取るとステップを編集できます。" | "Read a macro to edit its steps.",
    MacroNeedsConfig => "接続して設定値を読み取ってください。" | "Connect and read the configuration first.",
    MacroStartRecording => "録音を開始" | "Start recording",
    MacroStopRecording => "録音を停止" | "Stop recording",
    MacroRecordingHint => "ライブ入力の 20 ms レポートを録音します。録音を止めると現在のステップを置き換えます。" | "Records the 20 ms live reports. Stopping replaces the current steps.",
    MacroRecordingNeedsLive => "録音を開始するとライブ入力も有効にします。" | "Starting a recording also enables live input.",
    MacroRecordedSteps => "録音 {} ステップ / {} ms" | "Recorded {} steps / {} ms",
    MacroNoSteps => "ステップがありません。" | "No steps.",
    MacroStepMs => "時間 (ms)" | "Duration (ms)",
    MacroStepKeys => "キー" | "Keys",
    MacroStepSticks => "スティック" | "Sticks",
    MacroStepTriggers => "トリガー" | "Triggers",
    MacroDeleteStep => "このステップを削除" | "Delete step",
    MacroInsertWait => "待ち時間を挿入" | "Insert wait",
    MacroWaitMs => "待ち時間 (ms)" | "Wait (ms)",
    MacroSelectStepHint => "一覧の行を選ぶと、そのステップの値を編集できます。" | "Select a row to edit that step's values.",
    MacroTotalDuration => "合計 {} ms / {} ステップ" | "{} ms total / {} steps",
    MacroWriteAction => "マクロの書き込みを確認" | "Review macro write",
    MacroTrigger => "トリガー" | "Trigger",
    MacroCycles => "回数" | "Cycles",
    MacroInterval => "間隔 (ms)" | "Interval (ms)",
    MacroKeysHint => "マクロに記録できるのは Start〜R2 の 16 ボタンです。" | "A macro step stores the 16 buttons from Start to R2.",
    MacroRecordingFull => "200 ステップに達したため録音を停止しました。" | "Recording stopped at the 200-step limit.",
    WriteSuccess => "書き込みました。" | "Write complete.",
    WriteBackup => "バックアップ {} / SHA-256 {}" | "Backup {} / SHA-256 {}",
    WriteMacroBackup => "マクロバックアップ {} / config SHA-256 {}" | "Macro backup {} / config SHA-256 {}",
    WritePackets => "送信 {} フレーム" | "{} frames sent",
    WriteChangedRegions => "変更領域 {} 件" | "{} changed region(s)",
    WriteCrcOk => "read-back CRC 一致" | "read-back CRC matches",
    WriteCrcMismatch => "read-back CRC 不一致" | "read-back CRC mismatch",
    WriteMacroVerified => "プロファイル {} マクロ {} / {} ステップを検証しました。" | "Verified profile {} macro {} with {} steps.",
    WriteCloseResult => "結果を閉じる" | "Close result",
    CopyPath => "パスをコピー" | "Copy path",
    WriteFailureTitle => "書き込みに失敗しました。" | "The write failed.",
    WriteReadFailed => "書き込み前の読み取りに失敗しました。書き込みは行っていません。" | "The pre-write read failed. Nothing was written.",
    WritePrepareFailed => "書き込み内容の検証に失敗しました。書き込みは行っていません。" | "The write request was rejected. Nothing was written.",
    WriteBackupFailed => "バックアップを作成できなかったため、書き込みを中止しました。" | "The backup could not be written, so the write was stopped.",
    WriteDeviceFailed => "書き込み中に失敗しました。" | "The write failed on the controller.",
    WriteFailureBackup => "復旧用バックアップ" | "Recovery backup",
    WriteFailureRecovery => "復旧手順" | "Recovery",
    WriteFailureRecoveryText => "1. 再接続して設定値を読み取り、現在値を確認します。 2. 失敗が書き込み前なら、そのまま再試行できます。 3. Save 後なら、表示されたバックアップから復元します。" | "1. Reconnect and read the configuration to inspect the current values. 2. If the failure was before Save, retry. 3. If Save was sent, restore from the backup shown.",
    WriteFailureMacroRestore => "マクロバックアップは `eightbitdo macro restore --dir <パス>` で復元できます。" | "The macro backup can be restored with `eightbitdo macro restore --dir <path>`.",
    WriteFailureConfigRestore => "設定の raw バックアップは CLI の設定書き込みで復元できます。" | "The raw configuration backup can be restored with the CLI configuration write.",
    WriteLostLink => "書き込み中に接続が切れました。" | "The link dropped during the write.",
    WriteChanged => "確認後にコントローラの設定が変わったため、書き込みを中止しました。読み取り直してください。" | "The controller changed after review, so the write was stopped. Read it again.",
    NameTooLong => "名前は {} 文字までです（現在 {}）。" | "Names are limited to {} units (currently {}).",
    NameHasNul => "名前に NUL 文字は使えません。" | "Names cannot contain NUL.",
    InvalidTargetError => "割り当て先には単一のキーを選んでください。" | "Choose a single documented assignment target.",
    InvalidTriggerError => "トリガーには単一のキーを選んでください。" | "Choose a single documented trigger.",
    EmptyStepsError => "マクロには 1 ステップ以上必要です。" | "A macro needs at least one step.",
    ZeroStepError => "ステップ {} の時間が 0 ms です。" | "Step {} has a 0 ms duration.",
    MacroStepLimitError => "マクロは 200 ステップまでです（現在 {}）。" | "A macro is limited to 200 steps (currently {}).",
    NoChangesError => "書き込む変更がありません。" | "There are no changes to write.",
    InvalidPlanError => "変更内容を検証できませんでした。" | "The changes could not be validated.",
    NoSnapshotError => "設定値が未読み取りです。読み取ってから操作してください。" | "Read the configuration before writing.",
    KeyBtCon => "Bluetooth 接続" | "Bluetooth",
    KeySwap => "スワップ" | "Swap",
    KeyTurbo3 => "ターボ 3" | "Turbo 3",

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
    LogPreWriteBackup => "書き込み前バックアップ: {} / SHA-256 {}" | "Pre-write backup: {} / SHA-256 {}",
    LogPreWriteMacroBackup => "書き込み前マクロバックアップ: {} / config SHA-256 {}" | "Pre-write macro backup: {} / config SHA-256 {}",
    LogConfigWritten => "設定値を書き込みました。" | "Configuration written.",
    LogMacroWritten => "プロファイル {} のマクロ {} を書き込みました。" | "Wrote macro {} in profile {}.",
    LogMacroRead => "プロファイル {} のマクロ {} を読み取りました。" | "Read macro {} in profile {}.",
    LogRecordingStopped => "録音を停止しました。" | "Recording stopped.",
    LogMacroReload => "設定値が更新されたため、マクロを読み直してください。" | "The configuration changed; read the macro again.",
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
    ErrMacroReadFailed => "マクロを読み取れませんでした。" | "Could not read the macro.",
    ErrMacroReadLostLink => "マクロの読み取り中に接続が切れました。" | "The link dropped while reading the macro.",
    ErrWriteReadFailed => "書き込み前の読み取りに失敗しました。書き込みは行っていません。" | "The pre-write read failed. Nothing was written.",
    ErrWritePrepareFailed => "書き込み内容の検証に失敗しました。書き込みは行っていません。" | "The write request was rejected. Nothing was written.",
    ErrWriteBackupFailed => "バックアップを作成できなかったため、書き込みを中止しました。" | "The backup could not be written, so the write was stopped.",
    ErrDeviceWriteFailed => "書き込み中に失敗しました。" | "The write failed on the controller.",
    ErrWriteLostLink => "書き込み中に接続が切れました。" | "The link dropped during the write.",
    ErrWriteChanged => "確認後にコントローラの設定が変わったため、書き込みを中止しました。" | "The controller changed after review; the write was stopped.",
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
        "P1" => t(lang, Key::ButtonP1),
        "P2" => t(lang, Key::ButtonP2),
        "P3" => t(lang, Key::ButtonP3),
        "P4" => t(lang, Key::ButtonP4),
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

/// Display name of one documented key target (remap, trigger or gyro button).
///
/// Unsupported stored values (`KeyMap::MY_NULL` = 227, unknown bits, composite
/// masks) are shown as a raw mask so the user can see why the combo box does
/// not offer them.
#[must_use]
pub fn key_target_text(lang: Lang, map: KeyMap) -> String {
    match map.name() {
        Some("None") => t(lang, Key::KeyNone).to_string(),
        Some("ScreenShot") => t(lang, Key::ButtonScreenshot).to_string(),
        Some("Turbo_Auto") => t(lang, Key::ButtonTurboAuto).to_string(),
        Some("Turbo_3") => t(lang, Key::KeyTurbo3).to_string(),
        Some("BT_CON") => t(lang, Key::KeyBtCon).to_string(),
        Some("Swap") => t(lang, Key::KeySwap).to_string(),
        Some("M1" | "M2" | "M3" | "M4") => map.name().unwrap_or_default().to_string(),
        Some("My_Null") => format!("0x{:08X}", map.bits()),
        Some(name) => button_name(lang, name).to_string(),
        None => format(lang, Key::KeyUnknown, &[format!("0x{:08X}", map.bits())]),
    }
}

/// Display name of one special-feature bit.
#[must_use]
pub fn special_feature_text(lang: Lang, feature: SpecialFeature) -> &'static str {
    if feature == SpecialFeature::LX_FLIP {
        t(lang, Key::SpecialLxFlip)
    } else if feature == SpecialFeature::LY_FLIP {
        t(lang, Key::SpecialLyFlip)
    } else if feature == SpecialFeature::RX_FLIP {
        t(lang, Key::SpecialRxFlip)
    } else if feature == SpecialFeature::RY_FLIP {
        t(lang, Key::SpecialRyFlip)
    } else if feature == SpecialFeature::STICK_SWAP {
        t(lang, Key::SpecialStickSwap)
    } else if feature == SpecialFeature::TRIGGER_SWAP {
        t(lang, Key::SpecialTriggerSwap)
    } else if feature == SpecialFeature::DPAD_SWAP {
        t(lang, Key::SpecialDpadSwap)
    } else if feature == SpecialFeature::RS_TRIGGER_FLIP {
        t(lang, Key::SpecialRsTriggerFlip)
    } else if feature == SpecialFeature::MOTION_SENSITIVITY {
        t(lang, Key::SpecialMotionSensitivity)
    } else if feature == SpecialFeature::ZERO_DEADZONE {
        t(lang, Key::SpecialZeroDeadzone)
    } else if feature == SpecialFeature::FOUR_WAY {
        t(lang, Key::SpecialFourWay)
    } else if feature == SpecialFeature::TRIGGER_VIBRATION {
        t(lang, Key::SpecialTriggerVibration)
    } else {
        t(lang, Key::SpecialVibration)
    }
}

/// Display name of one grouped configuration change.
#[must_use]
pub fn change_text(lang: Lang, change: ChangeKind) -> String {
    match change {
        ChangeKind::ProfileName(slot) => {
            format(lang, Key::ChangeProfileName, &[slot.number().to_string()])
        }
        ChangeKind::GamepadMode => t(lang, Key::ChangeGamepadMode).to_string(),
        ChangeKind::ActiveProfile => t(lang, Key::ChangeActiveProfile).to_string(),
        ChangeKind::Rumble(slot) => format(lang, Key::ChangeRumble, &[slot.number().to_string()]),
        ChangeKind::Sticks(slot) => format(lang, Key::ChangeSticks, &[slot.number().to_string()]),
        ChangeKind::Triggers(slot) => {
            format(lang, Key::ChangeTriggers, &[slot.number().to_string()])
        }
        ChangeKind::Special(slot) => format(lang, Key::ChangeSpecial, &[slot.number().to_string()]),
        ChangeKind::Gyro(slot) => format(lang, Key::ChangeGyro, &[slot.number().to_string()]),
        ChangeKind::LedTheme(slot) => {
            format(lang, Key::ChangeLedTheme, &[slot.number().to_string()])
        }
        ChangeKind::RemapFlag(slot) => {
            format(lang, Key::ChangeRemapFlag, &[slot.number().to_string()])
        }
        ChangeKind::RemapButton(slot, button) => format(
            lang,
            Key::ChangeRemapButton,
            &[
                slot.number().to_string(),
                button_name(lang, button.label()).to_string(),
            ],
        ),
    }
}

/// Display name of one changed macro header field.
#[must_use]
pub fn macro_change_text(lang: Lang, change: MacroChange) -> &'static str {
    match change {
        MacroChange::Steps => t(lang, Key::MacroChangeSteps),
        MacroChange::Name => t(lang, Key::MacroChangeName),
        MacroChange::Trigger => t(lang, Key::MacroChangeTrigger),
        MacroChange::Cycles => t(lang, Key::MacroChangeCycles),
        MacroChange::Interval => t(lang, Key::MacroChangeInterval),
    }
}

/// Localized explanation of a refused draft.
#[must_use]
pub fn draft_error_text(lang: Lang, error: &DraftError) -> String {
    match error {
        DraftError::NameTooLong { units, .. } => format(
            lang,
            Key::NameTooLong,
            &[MAX_NAME_UNITS.to_string(), units.to_string()],
        ),
        DraftError::NameHasNul { .. } => t(lang, Key::NameHasNul).to_string(),
        DraftError::NoChanges => t(lang, Key::NoChangesError).to_string(),
        DraftError::EmptyTarget { .. } => t(lang, Key::MacroEmptyTarget).to_string(),
        DraftError::EmptySteps => t(lang, Key::EmptyStepsError).to_string(),
        DraftError::TooManySteps { count } => {
            format(lang, Key::MacroStepLimitError, &[count.to_string()])
        }
        DraftError::ZeroDuration { index } => {
            format(lang, Key::ZeroStepError, &[(index + 1).to_string()])
        }
        DraftError::InvalidKeyTarget { .. } => t(lang, Key::InvalidTargetError).to_string(),
        DraftError::InvalidTrigger { .. } => t(lang, Key::InvalidTriggerError).to_string(),
        DraftError::InvalidPlan(_) => t(lang, Key::InvalidPlanError).to_string(),
        DraftError::NoSnapshot => t(lang, Key::NoSnapshotError).to_string(),
        DraftError::Recording => t(lang, Key::RecordingBlocksWrite).to_string(),
    }
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
            Key::OpenLicenses,
            Key::LicensesClose,
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
