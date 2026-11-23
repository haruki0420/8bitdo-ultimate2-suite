# モバイルネイティブ フル機能ギャップ監査 / 実装計画

- 対象ブランチ: `feat/native-mobile`（監査時点の `origin/main` = `81b0ac1`）
- 監査日: 2026-11-23
- 種別: **調査ドキュメントのみ**（この文書の追加以外にコード差分はない）
- 目的: iOS（SwiftUI）と Android（Jetpack Compose）のネイティブアプリを、デスクトップ CLI / GUI と同等のフル機能まで仕上げるための不足（UniFFI API・画面・検証・工数・順序・リスク）を確定する

> 前提: モバイルは Tauri の Web UI ではなく、**各 OS のネイティブ UI + `eightbitdo-uniffi` 経由の Rust コア**で作る（[AGENTS.md](../AGENTS.md) §3）。プロトコル・構造体・書き込み順序の正本は [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) / [STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md) とし、この文書はそれらを再定義しない。
>
> デスクトップ側の移行: デスクトップ GUI は別ブランチ `feat/tauri-gui` で Tauri v2 へゼロベース移行中で、表示名は `Padbench` にする予定（ユーザー方針。本ブランチ `feat/native-mobile` には未反映）。この文書で「デスクトップ / GUI」と書くときは、比較基準として監査時点の base `81b0ac1` の egui GUI と CLI を指す。Tauri 版に移っても共有する正本は Rust コア（`eightbitdo-core` / `eightbitdo-protocol`）であり、**モバイル向け UniFFI の契約・不足 API・書き込み安全手順・用語集はそのまま適用できる**。モバイルの表示名も `Padbench` 系列に揃えるか別名にするかは §10 #1 で決める。

---

## 0. 結論（要約）

| 項目 | 結論 |
| --- | --- |
| 現状 | モバイルは **M0 = 読み取り専用**。スキャン → 接続 → バージョン / 2768B 設定概要 / マクロ一覧の表示まで。書き込み系は **UniFFI に一切公開されていない**（意図的。`prepare_config_write` という純関数の「準備」だけがある） |
| 最大の不足 | ①設定の**全フィールド読み取り**、②型付き**設定編集**（`ConfigEdit` 相当）、③**設定書き込み**（pre-write backup + TOCTOU + read-back）、④**リマップ**、⑤**マクロ書き込み / 復元**、⑥**プロファイル切替**、⑦**`request_input`（0x0003）と `0x0007` の副作用**、⑧**バックアップ / 復元のファイル形式**、⑨**構造化エラー**、⑩**空スロットへのマクロ新規作成**（core が `EmptyMacroWrite` で拒否） |
| 画面数 | Android / iOS とも **16 項目**（A1–A16 / I1–I16。画面 14 + OTA + 共通基盤。うち既存は 1）。Android はナビゲーション自体が無く、iOS は `NavigationStack` の器だけあって画面遷移が無い |
| 工数（概算） | 共有 Rust / UniFFI **19–32 人日**、Android **25–41 人日**、iOS **27–43 人日**、合計 **71–116 人日**（OTA とストア準備を除く。S/M/L の素点は §8.1）。1 人なら約 3.5–6 か月、2 人並行で約 2.5–4 か月。OTA は wire 確定後に **+9–14 人日** |
| 必須ブロッカー | **BLE 実機**（このホストに無い）、**iOS の署名 / ストアは macOS + Xcode 必須**（unsigned IPA までは Linux で可）、**OTA は core の wire 未確定**、**プロファイル切替の永続性・実効が未確認**、**マクロ書込の実機証跡が S1 / M1・M2 のみ**、**表示名が商標方針に不適合**（[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §3.3） |
| 公開順序 | 読み取り完成 → 書き込み基盤（安全フロー）→ 実機検証 → 書き込み公開（Tier 3）→ マクロ → ファイル互換 → OTA → ストア配布 |

---

## 1. 現状サマリ

### 1.1 実装済み

| 領域 | 実装内容 | 根拠 |
| --- | --- | --- |
| UniFFI | 同期 `MobileSession`（version / config / macro backup / input / disconnect）、callback `NativeTransport`、`describe_config`、`prepare_config_write`（ローカル検証のみ）、bounded 受信キュー（64、drop-oldest）、`MIN_READ_MTU = 161` | [crates/eightbitdo-uniffi/src/lib.rs](../crates/eightbitdo-uniffi/src/lib.rs) |
| Android | Compose 1 画面。スキャン / 接続 / バージョン / 設定概要 / 12 マクロ一覧。GATT transport（CCCD 必須、MTU 200 要求・失敗時フォールバックなし、write-without-response / with-response 両対応、`FrameQueue`、再接続時に旧セッション破棄） | [DeviceViewModel.kt](../mobile/android/app/src/main/java/dev/eightbitdo/mobile/DeviceViewModel.kt)、[HomeScreen.kt](../mobile/android/app/src/main/java/dev/eightbitdo/mobile/ui/HomeScreen.kt)、[BluetoothGattTransport.kt](../mobile/android/app/src/main/java/dev/eightbitdo/mobile/ble/BluetoothGattTransport.kt) |
| iOS | SwiftUI 1 画面。CoreBluetooth で同じ読み取り。`WriteAckTracker`（with-response の遅延 ACK 誤帰属防止） | [AppModel.swift](../mobile/ios/Sources/EightBitDo/AppModel.swift)、[ContentView.swift](../mobile/ios/Sources/EightBitDo/Views/ContentView.swift)、[CoreBluetoothTransport.swift](../mobile/ios/Sources/EightBitDo/BLE/CoreBluetoothTransport.swift) |
| ビルド | JNI 同梱 debug APK 14,551,011 bytes、unsigned IPA 1,550,522 bytes | [MOBILE_BUILD_EVIDENCE.md](MOBILE_BUILD_EVIDENCE.md) |
| テスト | Rust 14 件（`#[test]`）、Android JVM 8 件、Swift ホスト純ロジック 5 チェック | コード件数と、本監査で再実行した結果（§6.2。14 / 8 / 5 すべて成功） |
| 原稿 | `mobile/README.md` に API・MTU・書き込みモード・署名制約が整理済み | [mobile/README.md](../mobile/README.md) |

### 1.2 不足（機能単位）

「コア / CLI / GUI にあるのに、UniFFI に無い」ものを正とする。`—` は未実装・未露出。

| # | 機能 | core | CLI | GUI | UniFFI | モバイル |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | ファームウェア版読み取り | あり | `info` | デバイスタブ | **あり** | 実装済み |
| 2 | 2768B 設定読み取り | あり | `config dump` | 設定タブ | **あり** | 実装済み（概要のみ表示） |
| 3 | 設定の**全フィールド**読み取り | あり（型） | `config dump` | 設定タブ | **概要 5 項目のみ** | 不足 |
| 4 | 設定書き込み | あり（`Session::write_full_config`） | `config write` / `config set` | 設定タブ | **なし** | 不足 |
| 5 | 型付き設定編集 | あり（`ConfigEdit` 13 種） | `config set` | 設定タブ | **なし**（raw バッファ編集のみ） | 不足 |
| 6 | 22 ボタン割り当ての読み取り | あり | `config map show` | リマップ UI | **なし** | 不足 |
| 7 | ボタンリマップ書き込み | あり（`ConfigEdit::ButtonRemap`） | `config remap` | リマップ UI | **なし** | 不足 |
| 8 | プロファイル切替 | あり（`switch_profile`） | `config profile` | 専用切替画面なし（`cur_slot` 編集は GUI にあり） | **なし** | 不足 |
| 9 | プロファイル copy / delete | protocol ヘルパのみ | — | — | **なし** | 不足 |
| 10 | マクロ読み取り（backup） | あり | `macro backup` | マクロタブ | **あり（表示用の概要のみ）** | 実装済み |
| 11 | マクロ書き込み | あり（`write_macro` / `write_macro_slot`） | `macro write` | マクロタブ | **なし** | 不足 |
| 12 | マクロ復元 | あり（CLI がオーケストレーション） | `macro restore` | 画面なし | **なし** | 不足 |
| 13 | マクロヘッダのみ書き込み | あり（`write_macro_header`） | — | — | **なし** | 不足 |
| 14 | 入力レポートストリーム | あり（`set_report_enable` + `request_input` + `try_next_input`） | `monitor` | ライブ入力タブ | **`set_report_enable` / `read_input_once` のみ。`request_input` が欠落** | 不足（画面なし・API 未使用。`0x0007` の通知だけで足りるかは未検証） |
| 15 | バッテリー | 入力レポート内（生値） | `monitor` 表示 | 入力タブ | 入力レポート内のみ | 不足（画面なし） |
| 16 | keepalive | あり | `keepalive` | — | **なし**（`request_input` を公開すれば 20ms `0x0003` で代替可。現状は `0x0003` を送れない） | 不足（軽微） |
| 17 | OTA | core のみ（wire 未確定） | — | — | **なし（意図的）** | ブロック |
| 18 | バックアップ（ファイル出力） | — | `backup` / `macro backup`（v1 形式） | あり | **JSON 文字列のみ・形式が不一致** | 不足 |
| 19 | 復元（ファイル入力） | — | `macro restore` | 画面なし | **なし** | 不足 |
| 20 | 構造化エラー | あり（stage / state 付き） | 表示 | stage / 復旧文言 | **文字列に平坦化** | 不足 |

補足:

- 3〜9・11〜13・17〜19 は「デスクトップにあり、モバイルに無い」もの。2・10・14・15 は API の一部はあるが画面・配線が無い。
- `read_config_json` / `read_macro_backup_json` / `describe_config` / `prepare_config_write` / `config_write_chunk_len` は **Android / iOS から一度も呼ばれていない**（Rust のテストのみ）。「次に作る読み取り UI」の下準備だけが存在する状態。
- `read_macro_backup` は内部で毎回 config を読み直す（`read_backup` は private）。設定とマクロを同時に取得する公開 API が無い。
- **空スロットへのマクロ新規作成は core で未実装**: `Session::write_macro` / `write_macro_slot` は対象マクロが空（`max_steps == 0` / `0xFFFF`）だと `Error::EmptyMacroWrite`（step-data clear 未実装）で拒否する。モバイルのマクロエディタは当面「既存の非空マクロの編集・記録による上書き」に限定される（§9 R19）。
- **`0x0007` には副作用がある**: `set_report_enable(true)` は公式実装でクライアント側 `MAX_ATT_MTU` を `0x3F`（63）へ切り替える（[PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §7）。有効化中の通知だけで公式相当の 20ms レポートが来るかは未検証で、**公式同等のライブ入力・記録には `request_input`（0x0003）の公開と周期制御が必要**（§2.3 API-9）。

### 1.3 未検証（重要）

| 項目 | 状態 |
| --- | --- |
| Android / iOS 実機での BLE スキャン・接続・読み取り | **未実施**（実機なし） |
| JNI ロード（実機 / エミュレータ） | **未実施**（ホストの emulator 実行ファイルが実行不可。§6.2） |
| 書き込み・リマップ・マクロ書き込み | **モバイルでは未公開**。デスクトップでも GUI 書込は実機未検証（AGENTS.md §6） |
| iOS 署名・実機インストール・TestFlight / App Store | **未実施**（実機と macOS が必要） |
| プロファイル切替の再接続 / 電源断を跨ぐ永続性・実効 | **未確認**（[DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md) §7） |
| マクロ書き込みの実機証跡 | **BLE・S1 の M1 / M2 のみ**。S2 / S3・M3 / M4 は外挿 |
| OTA | core 実装済み・**wire 未確定で未配線** |
| バッテリーのスケール / 充電状態 | **未確認**（生値のみ。0–100 か 0–255 か不明） |

---

## 2. UniFFI API 監査

### 2.1 現在の公開 API 一覧

出典: [crates/eightbitdo-uniffi/src/lib.rs](../crates/eightbitdo-uniffi/src/lib.rs)（`uniffi::setup_scaffolding!` 以降の export のみ）。

**Callback interface（ネイティブ側が実装）**

| 名前 | シグネチャ | 備考 |
| --- | --- | --- |
| `NativeTransport` | `send(Vec<u8>) -> Result<(), NativeTransportError>` / `recv(u32) -> Result<Option<Vec<u8>>, NativeTransportError>` / `mtu() -> u16` / `disconnect()` | `recv` は prefix 無しの 16B ヘッダ + payload。MTU は交渉後の実値 |

**Errors**

| 名前 | バリアント |
| --- | --- |
| `NativeTransportError` | `Failure { detail }` |
| `MobileError` | `Transport` / `Protocol` / `Timeout` / `Disconnected` / `Unsupported` / `InvalidRequest` / `Internal`（すべて `detail: String`） |

**Records**

| 名前 | フィールド |
| --- | --- |
| `DeviceInfo` | `raw: Vec<u8>`, `text: String` |
| `DeviceConfig` | `raw: Vec<u8>`, `stored_crc: u16`, `computed_crc: u16`, `crc_valid: bool`, `summary: ConfigSummary` |
| `ConfigSummary` | `gamepad_mode_raw: u16`, `gamepad_mode: String`, `current_slot: Option<u8>`, `slot_flags: Vec<u32>`, `profile_names: Vec<String>` |
| `MacroEntryInfo` | `slot: u8`（**1..=3**）, `index: u8`（**0..=3**）, `name`, `step_count`, `raw_len`, `is_empty` |
| `MacroBackupView` | `entries: Vec<MacroEntryInfo>`（12 件、slot-major） |
| `InputSnapshot` | `keys: u32`, `left_x/left_y/right_x/right_y: i16`, `trigger_left/right: u8`, `battery: u8` |
| `ConfigRegionInfo` | `offset: u32`, `len: u32` |
| `ConfigWriteDraft` | `expected: Vec<u8>`, `expected_crc: u16`, `changed_regions: Vec<ConfigRegionInfo>`, `is_noop: bool` |

**Functions / Object**

| 名前 | シグネチャ | 備考 |
| --- | --- | --- |
| `minimum_read_mtu()` | `-> u16` | 161 |
| `describe_config(raw)` | `-> Result<ConfigSummary, MobileError>` | I/O なし |
| `prepare_config_write(snapshot, edited)` | `-> Result<ConfigWriteDraft, MobileError>` | **送信しない**。CRC 再計算と差分のみ |
| `config_write_chunk_len(mtu)` | `-> Result<u16, MobileError>` | 1 パケットの payload 長 |
| `MobileSession::new(transport)` | `-> Result<Arc<MobileSession>, MobileError>` | MTU < 161 を拒否 |
| `read_device_info()` | `-> Result<DeviceInfo, MobileError>` | |
| `read_config()` / `read_config_json()` | `-> Result<DeviceConfig, MobileError>` / `String` | |
| `read_macro_backup()` / `read_macro_backup_json()` | `-> Result<MacroBackupView, MobileError>` / `String` | 設定読み直し + 12 マクロ読み |
| `set_report_enable(enabled)` | `-> Result<(), MobileError>` | `0x0007` |
| `read_input_once(timeout_ms)` | `-> Result<Option<InputSnapshot>, MobileError>` | **受信を待つだけ**。`request_input`（0x0003）が無いため公式の 20ms 周期ポーリングは再現できない。`set_report_enable(true)` の通知に依存する（周期 / 静止時挙動 / MTU 0x3F 副作用は未検証） |
| `disconnect()` | `-> ()` | 受信スレッドを join（最大 `recv` タイムアウト分ブロック） |

### 2.2 デスクトップ / CLI との差分（機能→API の対応）

| デスクトップ側の機能 | 実体 | モバイルに必要な API |
| --- | --- | --- |
| `config write` / GUI 設定タブ | `ConfigWritePlan` + `Session::write_full_config`（pre-write backup / TOCTOU / Save / read-back は呼び出し側） | API-4 |
| `config set` / GUI 設定タブ | `ConfigEdit` 13 種 + `prepare` | API-3 |
| `config remap` / GUI リマップ | `ConfigEdit::ButtonRemap` + `button_mappings` | API-2 / API-3 |
| `config map show` | `button_mappings` / `button_mapping` | API-2 |
| `config profile` | `Session::switch_profile` | API-7 |
| `macro write` / GUI マクロタブ | `MacroWritePlan` + `Session::write_macro` / `write_macro_slot` | API-6 |
| `macro restore` | CLI が backup を読み、per-macro plan を組み立てて書く | API-6 / API-8 |
| `macro backup` / GUI | `read_macro_backup` + backup v1 ファイル群 | API-8 |
| `monitor` | `set_report_enable(true)` → `request_input()` → `try_next_input` のループ | API-9 |
| `backup` | `read_config_snapshot` + `config.bin` + `config.json`（SHA-256 付き） | API-8 |
| GUI ライセンス画面 | 同梱フォント / MIT / Apache 全文 | 画面のみ（API 不要。文言アセット） |
| OTA | `OtaUpdate`（`mobile-boot` feature で `MobileBootLink` あり） | API-12（wire 確定後） |

### 2.3 追加が必要な API（関数シグネチャ案）

以下はすべて **追加のみ**（既存 export の削除・改名をしない）方針。API の版番号は `mobile_api_version()` で持つ（API-13）。

#### API-1: 設定の全フィールド読み取りビュー

`ConfigSummary` は 5 項目しか無く、設定画面を作れない。`Bt2Config` の全レコードを Record に写す。**f32 は生ビット（`u32`）で返す**（消去 slot の rumble は NaN を保持するため。§2.4）。

```rust
#[derive(uniffi::Record)]
pub struct ConfigView {
    pub raw: Vec<u8>,                 // 2768B
    pub stored_crc: u16,
    pub computed_crc: u16,
    pub crc_valid: bool,
    pub gamepad_mode_raw: u16,
    pub gamepad_mode: String,
    pub current_slot: Option<u8>,     // 1..=3
    pub slot_flags: Vec<u32>,         // 3
    pub profiles: Vec<ProfileView>,   // 3（S1..S3）
}

#[derive(uniffi::Record)]
pub struct ProfileView {
    pub slot: u8,                     // 1..=3
    pub name: String,
    pub rumble: RumbleView,
    pub joy: JoyView,
    pub trigger: TriggerView,
    pub special: SpecialView,
    pub gyro: GyroView,
    pub led: LedView,
    pub remap: RemapView,
    pub macros: MacroHeaderView,
}

#[derive(uniffi::Record)]
pub struct RumbleView {
    pub flag: u32,
    pub left_level: Option<u8>,       // 0..=5（LEVELS に一致しない値は None）
    pub right_level: Option<u8>,
    pub left_bits: u32,               // f32::to_bits（NaN / 非正規値も保全）
    pub right_bits: u32,
}

#[derive(uniffi::Record)]
pub struct JoyView {
    pub flag: u32,
    pub left_start: u8,  pub left_end: u8,
    pub right_start: u8, pub right_end: u8,
}

#[derive(uniffi::Record)]
pub struct TriggerView {
    pub flag: u32,
    pub left_start: u8,  pub left_end: u8,
    pub right_start: u8, pub right_end: u8,
}

#[derive(uniffi::Record)]
pub struct SpecialView { pub flag: u32, pub features: u32 }

#[derive(uniffi::Record)]
pub struct GyroView {
    pub flag: u32,
    pub key_map: u32,
    pub trigger_mode: u8,      // 0=hold, 1=toggle
    pub sensitivity: u8,       // 1..=15
    pub dead_compensate: u8,   // 0..=255
    pub mapping_type: u8,      // 0=left, 1=right, 2=mouse
}

#[derive(uniffi::Record)]
pub struct LedView {
    pub dynamic: DynamicThemeView,
    pub hotwheel: HotWheelThemeView,
    pub single: SingleThemeView,
}

#[derive(uniffi::Record)]
pub struct DynamicThemeView { pub flag: u32, pub color: u32, pub bg_color: u32 }

#[derive(uniffi::Record)]
pub struct HotWheelThemeView {
    pub flag: u32, pub color: u32, pub bg_color: u32, pub speed: u8, // 0..=15
}

#[derive(uniffi::Record)]
pub struct SingleThemeView { pub flag: u32, pub colors: Vec<u32> } // 24 個（ARGB）

#[derive(uniffi::Record)]
pub struct RemapView { pub flag: u32, pub buttons: Vec<ButtonMappingView> } // 22

#[derive(uniffi::Record)]
pub struct ButtonMappingView {
    pub button: u8,          // 0..=21
    pub button_name: String, // "A" / "P1"...
    pub to: u32,             // KeyMap 生値
    pub to_name: String,     // "B" / "M1" / "KeyMap(0x...)"
}

#[derive(uniffi::Record)]
pub struct MacroHeaderView { pub flag: u32, pub total_cnt: u8, pub records: Vec<MacroHeaderRecordView> } // 4

#[derive(uniffi::Record)]
pub struct MacroHeaderRecordView {
    pub name: String,
    pub gamepad_mode: u8,
    pub special_flag: u8,
    pub max_steps: u16,
    pub offset: u16,
    pub key_map: u32,
    pub cycles_num: u32,
    pub interval_ms: u32,
}

#[uniffi::export]
pub fn describe_config_v2(raw: Vec<u8>) -> Result<ConfigView, MobileError>;
```

- 実装は `Bt2Config` の getter をそのまま写すだけ（プロトコル知識を Swift / Kotlin に漏らさない）。
- 既存 `describe_config` / `ConfigSummary` は残す（後方互換）。新規画面は `describe_config_v2` を使う。

#### API-2: ボタン / KeyMap カタログ

22 ボタンの並びと `KeyMap` の許可値は Swift / Kotlin に複製させない（リマップ UI の選択肢・検証の正本を Rust に置く）。

```rust
#[derive(uniffi::Record)]
pub struct ButtonEntry {
    pub index: u8,             // 0..=21
    pub name: String,          // "A" / "L1" / "P1" / "P4"...
    pub default_target: u32,   // ButtonIndex::default_key()
    pub default_target_name: String,
}

#[derive(uniffi::Record)]
pub struct KeyMapEntry {
    pub bits: u32,
    pub name: String,
    pub is_macro: bool,        // M1..M4 / SHARE / LOGO 等の非ビットマスク
}

#[uniffi::export]
pub fn button_catalog() -> Vec<ButtonEntry>;   // 22、wire 順
#[uniffi::export]
pub fn keymap_catalog() -> Vec<KeyMapEntry>;   // KeyMap::is_single_documented() を満たす値
```

#### API-3: 型付き設定編集（`ConfigEdit` 相当）

raw 2768B をプラットフォーム側で編集させると、オフセット・UTF-16BE・CRC・予約領域の知識が 3 実装に重複する。core の `ConfigEdit` と同じ粒度で列挙する（13 種 + copy/delete）。

```rust
#[derive(uniffi::Enum)]
pub enum ConfigEdit {
    GamepadMode { mode: u16 },
    CurSlot { slot: Option<u8> },                                  // 1..=3, None=未選択
    ProfileName { slot: u8, name: String },                        // UTF-16BE 16 単位まで
    Rumble { slot: u8, flag: u32, left_level: u8, right_level: u8 },
    Joy { slot: u8, flag: u32, left_start: u8, left_end: u8, right_start: u8, right_end: u8 },
    Trigger { slot: u8, flag: u32, left_start: u8, left_end: u8, right_start: u8, right_end: u8 },
    Special { slot: u8, flag: u32, features: u32 },
    Gyro {
        slot: u8, flag: u32, key_map: u32,
        trigger_mode: u8, sensitivity: u8, dead_compensate: u8, mapping_type: u8,
    },
    DynamicTheme { slot: u8, flag: u32, color: u32, bg_color: u32 },
    HotWheelTheme { slot: u8, flag: u32, color: u32, bg_color: u32, speed: u8 },
    SingleTheme { slot: u8, flag: u32, colors: Vec<u32> },         // 24
    ButtonRemap { slot: u8, button: u8, to: u32 },                 // API-4 のリマップ
    MapKeyFlag { slot: u8, flag: u32 },
    CopySlot { dst: u8, src: u8 },                                 // Bt2Config::copy_slot
    DeleteSlot { slot: u8 },                                       // Bt2Config::delete_slot
}

#[uniffi::export]
pub fn validate_config_edits(edits: Vec<ConfigEdit>) -> Result<(), MobileError>;

#[uniffi::export]
pub fn prepare_config_edits(
    snapshot: Vec<u8>,
    edits: Vec<ConfigEdit>,
) -> Result<ConfigWriteDraft, MobileError>;
```

- `prepare_config_edits` は入力の検証（範囲・flag・KeyMap whitelist・予約領域不変・no-op 拒否）と CRC 再計算だけを行う純関数。**I/O もフレーム送信もしない**。
- 既存 `prepare_config_write(snapshot, edited)` は raw 編集用に残す。
- copy / delete は core に helper があるが CLI / GUI 未露出。モバイルで出すなら **同時にデスクトップ側の `ConfigEdit` へも追加**して 3 実装の意味を揃える（本監査の範囲外だが、片側だけ出すと用語・挙動がずれる）。
- rumble の消去 slot は `f32` が NaN のため、typed edit（`left_level: u8`）では復元できない。**NaN を含む raw をそのまま戻す場合は raw 経路**（`prepare_config_write(snapshot, edited)`）を使う。typed 編集は「現在値が有効な範囲のとき」に限定し、UI でその旨を示す。
- 列挙名の `Gyro` はこの文書の便宜名で、**core の `ConfigEdit` では `SixAxis`**（`SixAxisRecord`、`+0x92C + slot * 0x0C`）。3 実装で名称を揃えるときは core 側の `SixAxis` に合わせ、UniFFI の enum だけ別名にしない。

#### API-4: 設定書き込み（二段階 + 構造的バックアップ + TOCTOU）

デスクトップ GUI の `driver.rs` と同じ順序を FFI に持ち上げる。**プラットフォーム側で「read → backup → write → read-back」を再実装させない**のが目的。

1. prepare: 設定を**読み直す** → 呼び出し元が持つ SHA-256 と比較（TOCTOU）→ edit 適用 → no-op 拒否 → 必須 pre-write backup を `BackupSink` で永続化
2. commit: `write_full_config`（0x0001 × N → Save → 待機 → 0x0002 read-back、core 内で実施）→ 結果を返す

```rust
#[derive(uniffi::Record)]
pub struct BackupRefView {
    pub directory: String,          // プラットフォームが付けたバックアップ名
    pub config_sha256: String,
    pub config_bin_sha256: String,
    pub config_json_sha256: Option<String>,
}

/// バックアップ永続化の失敗（transport エラーとは分ける）。
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BackupError {
    #[error("{detail}")]
    Failure { detail: String },
}

#[uniffi::export(with_foreign)]
pub trait BackupSink: Send + Sync {
    /// `name`（相対パス）と `bytes` を永続化し、**flush / close 完了後**に Ok を返す。
    /// Err を返した場合、書き込みは開始されない（fail closed）。
    fn save(&self, name: String, bytes: Vec<u8>) -> Result<(), BackupError>;
}

#[derive(uniffi::Object)]
pub struct ConfigWritePreparation;   // snapshot / PreparedConfigWrite / BackupRefView

impl MobileSession {
    /// 設定を読み直して TOCTOU を検証し、必須バックアップを書いてから準備を返す。
    /// 既存の純関数 `prepare_config_write(snapshot, edited)` と生成名が衝突しないよう
    /// デバイス I/O を伴う方は `begin_config_write` にする。
    pub fn begin_config_write(
        &self,
        expected_config_sha256: String,
        edits: Vec<ConfigEdit>,
        sink: Arc<dyn BackupSink>,
    ) -> Result<Arc<ConfigWritePreparation>, MobileError>;

    /// 実際の flash 書き込み + Save + read-back。preparation は begin_config_write の戻り値のみ。
    pub fn commit_config_write(
        &self,
        preparation: Arc<ConfigWritePreparation>,
    ) -> Result<ConfigWriteReport, MobileError>;
}

#[uniffi::export]
impl ConfigWritePreparation {
    pub fn draft(&self) -> ConfigWriteDraft;      // 差分確認 UI 用
    pub fn backup(&self) -> BackupRefView;
    pub fn config_sha256(&self) -> String;
}

#[derive(uniffi::Record)]
pub struct ConfigWriteReport {
    pub backup: BackupRefView,
    pub packets_sent: u32,
    pub changed_regions: Vec<ConfigRegionInfo>,
    pub read_back: ConfigView,          // 書き込み後に読み戻した全フィールド
    pub read_back_matches: bool,
    pub crc_expected: u16,
    pub crc_stored: u16,
    pub crc_recomputed: u16,
    pub crc_interpretation: String,     // "matches_sent" / "in_place" / "from_sent"
}
```

- **read-back は Rust 側で実施済み**（`write_full_config` の doc コメント手順 4）。プラットフォームが再実装しない。
- `BackupSink` を必須引数にすることで「バックアップ無しで書き込む API」が存在しなくなる（Tier 3 レビューの前提）。
- `expected_config_sha256` は `sha256_hex(snapshot)` で得る（API-13）。GUI の driver と同じ TOCTOU 検査（再読みした設定の SHA-256 と比較）。
- 失敗時は API-10 の `ConfigWrite { stage, state }` を返し、UI は stage に応じた復旧文言を出す（core の `ConfigWriteStage` / `ConfigWriteState` をそのまま写す）。

#### API-5: マクロ読み取り（ステップ付き）

現状の `MacroEntryInfo` は概要だけで、ステップを編集できない。`MacroStep` を Record 化する。

```rust
#[derive(uniffi::Record)]
pub struct MacroStep {
    pub ms_times: u16,  // 1ms 単位（core と同じ名前）
    pub keys: u16,      // 下位 16bit のボタンマスク
    pub rt_value: u8,
    pub lt_value: u8,
    pub left_x: u8,  pub left_y: u8,
    pub right_x: u8, pub right_y: u8,
}

#[derive(uniffi::Record)]
pub struct MacroEntryView {
    pub slot: u8,          // 1..=3
    pub index: u8,         // 0..=3
    pub name: String,
    pub steps: Vec<MacroStep>,
    pub raw_sha256: String,
    pub header: MacroHeaderRecordView,
    pub is_empty: bool,
}

#[derive(uniffi::Object)]
pub struct MacroBackupData;   // core の MacroBackup を保持（12 件 + 設定スナップショット）

impl MobileSession {
    /// 設定を 1 回だけ読み、12 マクロをまとめて読む（read_backup の公開版）。
    pub fn read_macro_backup_data(&self) -> Result<Arc<MacroBackupData>, MobileError>;
}

#[uniffi::export]
impl MacroBackupData {
    pub fn config_sha256(&self) -> String;
    pub fn entries(&self) -> Vec<MacroEntryView>;   // 12
    pub fn to_json(&self) -> Result<String, MobileError>;
}
```

- core の `MacroStep`（`macro_data.rs`）は serde 対応で、CLI の `MacroFile` も同じ 10 バイトを JSON / TOML 化している。FFI でも**同じフィールド名**を使い、`MacroFile` と相互変換できる形にする（§5.5）。1 マクロの上限は `MAX_MACRO_STEPS = 200`。

#### API-6: マクロ書き込み / 復元（二段階 + バックアップ）

core の `write_macro` / `write_macro_slot` / `write_macro_header` は「backup（12 件）を引数に取る」ので、`MacroBackupData` をそのまま渡せる形にする。

```rust
#[derive(uniffi::Record)]
pub struct MacroWriteOptions {
    pub name: Option<String>,
    pub key_map: Option<u32>,
    pub cycles_num: Option<u32>,
    pub interval_ms: Option<u32>,
    pub flag: Option<u32>,        // スロットのマクロ有効化フラグ（None=現状維持）
    pub total_cnt: Option<u8>,    // 0..=4（None=現状維持）
    pub chunk_size: Option<u16>,  // Some(32) / Some(64) / None=MTU から選択
}

#[derive(uniffi::Object)]
pub struct MacroWritePreparation;

#[derive(uniffi::Record)]
pub struct MacroWriteReport {
    pub backup: BackupRefView,
    pub packets_sent: u32,
    pub read_back: MacroEntryView,
    pub read_back_matches: bool,
}

impl MobileSession {
    /// 対象マクロが空なら拒否（空マクロ作成は core 未実装）。
    /// 現在の config / 対象マクロ raw の SHA-256 で TOCTOU を検証し、
    /// macro backup（config.bin / config.json / macros.json / 各 .bin）を sink に書く。
    pub fn prepare_macro_write(
        &self,
        slot: u8,
        index: u8,
        expected_config_sha256: String,
        expected_macro_raw_sha256: String,
        steps: Vec<MacroStep>,
        options: MacroWriteOptions,
        sink: Arc<dyn BackupSink>,
    ) -> Result<Arc<MacroWritePreparation>, MobileError>;

    pub fn commit_macro_write(
        &self,
        preparation: Arc<MacroWritePreparation>,
    ) -> Result<MacroWriteReport, MobileError>;

    /// backup ディレクトリ（MacroBackupData）から非空マクロを全て戻す。
    /// 事前バックアップは必須（sink）。force は「読めない現マクロを飛ばす」等。
    pub fn prepare_macro_restore(
        &self,
        backup: Arc<MacroBackupData>,
        force: bool,
        sink: Arc<dyn BackupSink>,
    ) -> Result<Arc<MacroRestorePreparation>, MobileError>;

    pub fn commit_macro_restore(
        &self,
        preparation: Arc<MacroRestorePreparation>,
    ) -> Result<MacroRestoreReport, MobileError>;

    /// 空スロットのヘッダだけを戻す（core の write_macro_header）。
    /// Save を伴う書き込みなので、他と同じく TOCTOU + 必須バックアップを通す。
    pub fn write_macro_header_only(
        &self,
        backup: Arc<MacroBackupData>,
        slot: u8,
        expected_config_sha256: String,
        sink: Arc<dyn BackupSink>,
    ) -> Result<MacroHeaderReport, MobileError>;
}
```

- 復元の slot 単位バッチ（ヘッダ 1 回 + 最初の Save 1 回 + 最後の Save 1 回）は core の `write_macro_slot` が守る。**モバイル側でパケット列を組まない**。
- 書き込みも slot 一括 API（`write_macro_slot`）を公開する。同一 slot の複数マクロは 1 ヘッダ + 2 Save で書くのが公式順序で、モバイルから 1 本ずつ呼ぶと Save が増える。
- 対象マクロが空だと core が `Error::EmptyMacroWrite` で拒否する。**空スロットへの新規作成は当面不可**（§9 R19）。
- `MacroWriteOptions` は `None` のフィールドを既存ヘッダ値のまま保持する（core の `MacroWritePlan` と同じ意味）。

#### API-7: プロファイル切替

```rust
impl MobileSession {
    /// 0x0015 を 1 回送る。Save は送らない（core と同じ）。
    /// ACK 形状・永続性は未確認のため、UI は成功後に config を読み直して
    /// cur_slot を表示し、再接続後の再確認を促すこと。
    pub fn switch_profile(&self, slot: u8) -> Result<(), MobileError>;   // 1..=3
}
```

#### API-8: バックアップ / 復元のファイル形式（デスクトップ v1 と同一）

デスクトップの backup v1 は `config.bin` / `config.json` / `macros.json`（`format: 1`）/ `macros.toml` / `slot0-macro0.bin`〜`slot2-macro3.bin` で、各ファイルの SHA-256 を manifest に持つ。モバイルの現行 JSON は**別形式**（`raw_hex` を 1 ファイルに埋めるだけ）で相互運用できない。形式を 1 本化する。

```rust
#[derive(uniffi::Record)]
pub struct BackupFile { pub path: String, pub bytes: Vec<u8>, pub sha256: String }

#[uniffi::export]
pub fn export_backup_files(
    backup: Arc<MacroBackupData>,   // 設定スナップショット + 12 マクロを 1 つで持つ
) -> Result<Vec<BackupFile>, MobileError>;          // v1 の全ファイル（パス + バイト + SHA-256）

#[uniffi::export]
pub fn import_backup_files(
    files: Vec<BackupFile>,
) -> Result<Arc<MacroBackupData>, MobileError>;     // SHA-256 照合・サイズ検証・manifest 検証

#[uniffi::export]
pub fn validate_backup_files(files: Vec<BackupFile>) -> Result<(), MobileError>;
```

- 保存はプラットフォームのファイルピッカー（Android SAF、iOS `fileImporter` / `fileExporter`）で行う。Rust はバイト列と名前・ハッシュだけ返す（ファイル I/O を持たない）。
- `export_backup_files` の入力は `MacroBackupData` 1 つにする。設定とマクロを別引数で渡すと、両者の SHA-256 が食い違う余地ができるため。
- 1 ファイルにまとめたい場合の拡張は「v1 ディレクトリを zip した 1 ファイル」。中身のパス構成は変えない（デスクトップで展開すればそのまま読める）。
- マクロ 1 本の入出力は core / CLI の `MacroFile`（`name` / `gamepad_mode` / `key_map` / `cycles_num` / `interval_ms` / `steps`）に合わせる。
- `path` は**正規化してから検証**する: `..` を含む親参照、絶対パス（先頭 `/`・`\`、Windows のドライブ表記）、重複パス（同一パスが複数 entry）、v1 の許容集合（`config.bin` / `config.json` / `macros.json` / `macros.toml` / `slot{0..2}-macro{0..3}.bin`）外を拒否する。`import_backup_files` / `validate_backup_files` はアーカイブ内の相対パスとしてのみ扱い、実ファイルの書き込み先決定はプラットフォーム側に任せる。

#### API-9: 入力レポート（`request_input`）と記録ヘルパ

**現状の欠落**: core には `Session::request_input()`（`0x0003` の投げっぱなし送信）があるが、UniFFI に公開されていない。`read_input_once` は受信を待つだけで、公式アプリの約 20ms 周期 `0x0003` ポーリングを再現できない。`set_report_enable(true)`（0x0007）の通知だけでレポートが来る可能性はあるが、周期・静止時挙動・クライアント MTU 0x3F への切替（§1.2、[PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §7）が未検証なので、公式同等のライブ入力・マクロ記録には `request_input` の公開と周期制御が要る。

```rust
impl MobileSession {
    /// 0x0003 を 1 回送る（fire-and-forget）。read_input_once と組にして
    /// ~20ms 周期で呼ぶ。
    pub fn request_input(&self) -> Result<(), MobileError>;

    /// 直近に受信した入力レポートのバッテリー生値。未受信なら None。
    /// スケール（0-100 / 0-255）と充電状態は未確認なので % 表示にしない。
    pub fn last_battery(&self) -> Option<u8>;
}

/// 入力 1 フレームを「duration_ms 継続する 1 ステップ」に変換する純関数。
/// スティック -128..=128 → 0..=255（+128）、トリガーはそのまま、keys は下位 16bit、
/// ms_times = duration_ms。変換規則を 3 実装に重複させない。
#[uniffi::export]
pub fn input_to_macro_step(snapshot: InputSnapshot, duration_ms: u16) -> MacroStep;
```

- 記録の分解能はホストのポーリング周期（公式 ≈20ms。`0x0007` の通知駆動なら端末依存）で決まる。1ms 解像度は「保持」できるが「記録」はできない。この制約を UI に明記する。
- `0x0007`（ReportEnable）と `0x0003`（ReadKeysAndJoy）のどちらを記録に使うか、`0x0007` の MTU 0x3F 副作用をどう扱うかは実機で確定する（§10 #3、§9 R20）。現状の mobile API は `0x0007` 側しか無い。
- ステップ上限は `MAX_MACRO_STEPS = 200`。記録が上限に達したら自動停止する（GUI と同じ挙動）。

#### API-10: 構造化エラー（stage / state / TOCTOU）

現状は core の `Error` を `detail: String` に平坦化しているため、UI が「Save 前か後か」「復旧手順は何か」を判断できない。デスクトップ GUI が出している情報（`ConfigWriteStage` / `ConfigWriteState` / 復旧文言）を型で渡す。

```rust
#[derive(uniffi::Enum)]
pub enum ConfigWriteStage { Write, Save, Verify }
#[derive(uniffi::Enum)]
pub enum ConfigWriteState { NotSaved, Saved }
#[derive(uniffi::Enum)]
pub enum MacroWriteStage { Header, HeaderSave, Erase, Write, FinalSave, Verify }
#[derive(uniffi::Enum)]
pub enum MacroWriteState { NotSaved, HeaderSaved, DataSaved }   // core と同一

#[derive(uniffi::Error)]
pub enum MobileError {
    Transport { detail: String },
    Protocol { detail: String },
    Timeout { detail: String },
    Disconnected { detail: String },
    Unsupported { detail: String },
    InvalidRequest { detail: String },
    Backup { detail: String },
    ChangeDetected { expected_sha256: String, actual_sha256: String }, // TOCTOU
    ConfigWrite { stage: ConfigWriteStage, state: ConfigWriteState, detail: String },
    MacroWrite { stage: MacroWriteStage, state: MacroWriteState, detail: String },
    Internal { detail: String },
}
```

- 文言そのものは Rust に持たせない（日本語 / 英語の正本は §5.2 の共有リソース）。Rust は**コードと stage / state だけ**返し、UI が文言を選ぶ。
- core の `Error::ConfigWrite` / `Error::MacroWrite` は `recovery: &'static str`（英語の復旧案内）も持つが、FFI では**その文字列を出さない**。stage / state から §5.2 の文言を選ぶ（英語の復旧文が日本語 UI に混ざるのを防ぐ）。
- `MobileError` のバリアント追加は生成バインディングの `when` / `switch` に影響する。`mobile_api_version()`（API-13）で世代を判定できるようにする。

#### API-11: 進捗とキャンセル（任意・推奨）

`read_macro_backup` は数十秒かかり、現状キャンセルできない（`disconnect` でリンクごと落とすしかない）。進捗は「設定読み → マクロ 12 本」の段階だけでも価値がある。

```rust
#[uniffi::export(with_foreign)]
pub trait OperationObserver: Send + Sync {
    fn on_progress(&self, phase: String, done: u32, total: u32);
    fn is_cancelled(&self) -> bool;
}

impl MobileSession {
    pub fn read_macro_backup_data_with(
        &self,
        observer: Arc<dyn OperationObserver>,
    ) -> Result<Arc<MacroBackupData>, MobileError>;
}
```

- キャンセルは「フレーム送信の合間」でのみ反映する（flash 書き込み中の中断は安全でないため、書き込み系には付けない）。読み取り専用操作に限定する。

#### API-12: OTA（未配線の確認と配線条件）

**現状: UniFFI に OTA のシンボルは 1 つも無い。** core には `OtaUpdate` / `FirmwareImage` / `MobileBootLink`（`mobile-boot` feature、`eightbitdo-uniffi` が有効化）があるが、export されていない。CLI / GUI も未配線で、[FEATURE_MATRIX.md](FEATURE_MATRIX.md) のとおり **ACK の offset / total_len 照合・マルチセクション `.dat`・Bootloader MTU が未確定**。したがってモバイル OTA は「実装しない」が正しい現状判断。

wire 確定後に必要な API の形（参考）:

```rust
impl MobileSession {
    /// 0x000C EnterBoot を送り、リンク終了を待つ。UI は "8bt" へ再接続する。
    pub fn enter_boot(&self) -> Result<(), MobileError>;
}

#[uniffi::export]
pub fn read_boot_pid(transport: Arc<dyn NativeTransport>) -> Result<u16, MobileError>;

#[uniffi::export]
pub fn prepare_firmware(dat: Vec<u8>, expected_load_base: u32) -> Result<Arc<FirmwareImageView>, MobileError>;

#[uniffi::export]
pub fn write_firmware(
    transport: Arc<dyn NativeTransport>,      // "8bt" (FF20/FF21) の接続済みリンク
    image: Arc<FirmwareImageView>,
    observer: Arc<dyn OperationObserver>,
) -> Result<OtaReport, MobileError>;
```

- `OtaConfig::with_unverified_wire_allowed` は**公開 API に出さない**（既定 false のゲートを維持）。
- ファームウェアの取得・SHA-256 / 署名検証・ダウングレード判定はアプリ側に置かない（[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §5.5 の「署名検証・公式配布元・デバイス宛のみ」の 3 条件を満たす設計にする）。

#### API-13: 補助

```rust
#[uniffi::export] pub fn mobile_api_version() -> u32;          // 破壊的変更の検出
#[uniffi::export] pub fn sha256_hex(bytes: Vec<u8>) -> String; // backup / TOCTOU 用（3 実装に暗号実装を置かない）
#[uniffi::export] pub fn slot_number(slot_index: u8) -> Result<u8, MobileError>;   // 0..=2 → 1..=3
#[uniffi::export] pub fn slot_index(slot_number: u8) -> Result<u8, MobileError>;   // 1..=3 → 0..=2
impl MobileSession { pub fn resync(&self) -> Result<(), MobileError>; }            // 連続失敗後の再同期
```

### 2.4 互換性と実装上の注意

| 項目 | 注意 |
| --- | --- |
| UniFFI のバージョン | **0.31.2 固定**（0.32 系は推移依存の MSRV が 1.85 を超える。[mobile/README.md](../mobile/README.md) §MSRV）。rich enum / callback interface は 0.31.2 の範囲で設計する。bindgen と crate の不一致は実行時クラッシュ |
| `f32` | rumble は `f32` で、**消去 slot は NaN** を持つ（[STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md)）。Record に `f32` をそのまま載せると JSON 化・等価比較・Kotlin/Swift の橋渡しで問題になるため、**ビット（`u32`）で公開**し、表示用 level は `Option<u8>` にする |
| `Vec<u8>` のコピー | 2768B 設定 + 12 マクロ（最大 24KB）を JNI / Swift 境界で何度もコピーしない。`ConfigView.raw` は必要時だけ、バックアップは `MacroBackupData`（opaque）で保持 |
| slot の基底 | 既存 API は混在している（`MacroEntryInfo.slot` は **1..=3**、`MacroEntryInfo.index` は **0..=3**、CLI の `--slot` は **0..=2**）。新 API は Record を **1..=3 / 0..=3** に統一し、CLI 互換が必要な箇所だけ `slot_index` / `slot_number` を通す。UI 表示は「プロファイル 1/2/3」 |
| Index / 0-based | `MacroIndex` は 0..=3。`MacroEntryView.index` を 0..=3 で返し、UI は `+1` して表示する（既存 Android / iOS と同じ） |
| JSON 形式 | デスクトップ `config.json` とモバイル `read_config_json` は**別形式**（§5.5）。v1 に統一するまで相互運用しない |
| `MobileError` の拡張 | 生成バインディングの網羅 `switch` が壊れる。`mobile_api_version()` を上げ、アプリ側も同時更新 |
| `read_macro_backup` の再読み | 既存 API は毎回 config を読み直す。`read_macro_backup_data`（API-5）を追加し、既存は互換のため残す |
| `0x0007` の副作用 | 公式実装は `set_report_enable(true)` の時点でクライアント側 `MAX_ATT_MTU` を `0x3F`（63）へ切り替える（[PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §7）。既存の 161 バイト読み取りと干渉しないか、MTU 交渉後のセッションで `0x0007` を有効化して実機確認する |
| `uniffi` の生成物 | `mobile/ios/RustCore/`・`Generated/`・`mobile/android/.../core/` は **生成物でコミットしない**。`scripts/mobile/gen-bindings.sh` で再生成 |
| 生成物が無い worktree | **クリーンな worktree では `gen-bindings.sh` を先に実行しないと Gradle / xtool がコンパイルできない**（Kotlin は `Unresolved reference 'core'` / `stepCount`、iOS は `EightBitdoFFI` 未解決）。本監査でも再現した（§3.4・§6.2） |
| `scripts/mobile/gen-bindings.sh` のコメント | 冒頭コメントが「`uniffi` 0.32.2」のまま（実際は 0.31.2 固定）。ドキュメント齟齬として別途修正候補（本監査では変更しない） |

---

## 3. Android（`mobile/android`）

### 3.1 現在の実装範囲

- **画面**: [`HomeScreen`](../mobile/android/app/src/main/java/dev/eightbitdo/mobile/ui/HomeScreen.kt) の 1 画面のみ。スキャン → デバイス一覧 → 接続 → バージョン / CRC / 設定概要 / プロファイル名 / 12 マクロ一覧。ナビゲーション・設定・リマップ・マクロ編集・ライブ入力は無い。
- **状態管理**: `DeviceViewModel` + `StateFlow<UiState>`。Rust 呼び出しは `Dispatchers.IO`、切断は別スレッドで join（ANR 回避）。再接続前に旧セッションと transport を破棄。
- **BLE**: [`BluetoothGattTransport`](../mobile/android/app/src/main/java/dev/eightbitdo/mobile/ble/BluetoothGattTransport.kt) が `NativeTransport` を実装。GATT 操作を単一 executor で直列化、`FrameQueue`（64、drop-oldest）、CCCD `0x2902` 必須、`requestMtu(200)` 必須（失敗時フォールバックなし）、`minimumReadMtu()`（161）未満を拒否。
- **書き込みモード**: `PROPERTY_WRITE_NO_RESPONSE` 優先、無ければ `PROPERTY_WRITE`（`onCharacteristicWrite` 待ち）。タイムアウト後は再接続まで書き込み拒否（遅延 ACK の誤帰属防止）。
- **権限**: `BLUETOOTH_SCAN`（`neverForLocation`）/ `BLUETOOTH_CONNECT`（Android 12+）、`BLUETOOTH` / `BLUETOOTH_ADMIN` / `ACCESS_FINE_LOCATION`（API ≤ 30）。`MainActivity` 起動時に要求し、許可後に自動スキャン。
- **JNI 検出**: `RustCore.availability()` が `UnsatisfiedLinkError` を捕捉し、Rust 非同梱 APK でも落ちない（UI に NDK ビルド版が必要と表示）。
- **Rust ライブラリ**: `scripts/mobile/build-android.sh` が `gen-bindings.sh` → `cargo ndk`（arm64-v8a / x86_64）→ `testDebugUnitTest` / `assembleDebug`。
- **未配線**: `set_report_enable` / `read_input_once` は画面から呼ばれていない。`request_input`（0x0003）が無いため公式の 20ms ポーリングは未実装（§2.3 API-9）。`prepareConfigWrite` / `readConfigJson` などの準備 API も未使用。

### 3.2 画面一覧と工数（Android）

凡例: **S = 0.5–1 人日 / M = 2–3 人日 / L = 4–7 人日**（プラットフォーム経験者 1 名、レビュー待ち・実機待ちを除く）。

| # | 画面 | 内容 | 必要 API | 見積 |
| --- | --- | --- | --- | --- |
| A1 | 接続 / スキャン（既存 + 導線） | Bluetooth 無効時の有効化要求、権限拒否時（恒久拒否）の説明と設定リンク、接続失敗の再試行 | 既存 | S |
| A2 | デバイス情報 | ファームウェア版、接続状態、バッテリー（生値 + 「% 未確定」注記） | API-9 | S |
| A3 | プロファイル一覧 / 切替 | 3 スロット、名前、アクティブ表示、切替（0x0015）。切替後に config を読み直して `cur_slot` を確認 | API-7 / API-1 | S |
| A4 | 設定値ビュー（読み取り） | 全レコードをセクション表示（プロファイル / 振動 / スティック / トリガー / 特殊 / ジャイロ / LED 3 種 / リマップ概要 / マクロヘッダ） | API-1 | S（API 追加後は表示のみ） |
| A5 | 設定値編集 | 上記の編集フォーム、範囲検証、差分プレビュー、破棄確認 | API-3 | L |
| A6 | ボタンリマップ | 22 ボタン × 3 スロット、割当先ピッカー（カタログから選択）、既定値に戻す | API-2 / API-3 | M |
| A7 | 書き込み確認 / 実行 / 結果 | 差分表示 → バックアップ必須チェック → 確認 → 進捗 → read-back 結果 / stage 別の復旧案内 | API-4 / API-10 | M |
| A8 | バックアップ / 復元 | SAF でフォルダ選択、v1 ファイル群の書き出し / 読み込み、SHA-256 表示 | API-8 | M |
| A9 | マクロ一覧 / 詳細 | 既存の一覧 + ステップ表の閲覧 | API-5 | S |
| A10 | マクロエディタ | ステップの追加 / 削除 / 並べ替え / 値編集、JSON / TOML 入出力 | API-5 / API-6 / API-8 | L |
| A11 | マクロ記録（入力記録） | `request_input` ポーリング → `input_to_macro_step` → ステップ追加、開始 / 停止 / 破棄 | API-9 | M |
| A12 | マクロ書き込み / 復元 | 1 本 / slot 一括、確認、read-back、復元（pre-restore backup 必須） | API-6 | M |
| A13 | ライブ入力 | ボタン / スティック / トリガー / バッテリーの可視化、ポーリング制御 | API-9 | M |
| A14 | 設定 / ライセンス / アプリ情報 | 言語（ja / en）、バックアップ既定先、MIT / Apache / 第三者通知、非公式表記 | （文言） | S |
| A15 | OTA（ブロック） | ファームウェア選択・更新。**wire 確定まで作らない**（骨組みのみなら L、実装は API-12 後） | API-12 | L |
| A16 | 共通基盤 | Navigation、ViewModel 分割、エラー表示共通化、i18n、テーマ | — | M |

合計（A15 を除く）: S×6（A1–A4, A9, A14）+ M×7（A6–A8, A11–A13, A16）+ L×2（A5, A10）= 3–6 + 14–21 + 8–14 = **25–41 人日**（S 0.5–1 / M 2–3 / L 4–7 の素点。集計方法は §8.1）。

### 3.3 権限・MTU・切断復帰・前景制約

| 項目 | 現状 | 必要な対応 |
| --- | --- | --- |
| 権限 | `BLUETOOTH_SCAN` / `BLUETOOTH_CONNECT` 要求。拒否時はスキャンしないだけで説明なし | 恒久拒否時の説明と「設定を開く」導線。`neverForLocation` の意味を UI に書かない（内部実装） |
| Bluetooth 無効 | `BleScanner` が「Bluetooth is off or unavailable」で閉じる | `ACTION_REQUEST_ENABLE` の要求、復帰後の再スキャン |
| MTU | 200 を要求、失敗・無応答・`onMtuChanged` 異常はすべてエラー。23 に戻さない（正しい） | 機種差の記録（MTU 200 が通らない端末）。必要なら 161–200 の再試行方針を決める（**現状は厳格のままが安全**） |
| 切断復帰 | `onConnectionStateChange(DISCONNECTED)` で GATT / executor を解放。切断後の自動再接続は無い | UI に「再接続」を明示。自動再接続は書き込み中の二重送信リスクがあるため入れない |
| 入力ストリーム | 未使用（`set_report_enable` / `read_input_once` を呼ぶ画面が無い） | API-9 公開後、`0x0007` 有効化 → 20ms `0x0003` ポーリングでライブ入力。`0x0007` だけの通知に依存しない。MTU 0x3F 副作用を実機確認 |
| 前景制約 | 画面なし | 読み取り・書き込み中は `FLAG_KEEP_SCREEN_ON`。バックグラウンド移行で操作を中断し、復旧案内を出す。書き込みは前景専用（設計方針） |
| 長時間操作 | `readMacroBackup` は数十秒。キャンセル不可 | API-11 の進捗 + 読み取り限定キャンセル。書き込み中のキャンセルは出さない |
| R8 / ProGuard | `release` は `isMinifyEnabled = false` | リリースで minify するなら JNA / UniFFI の keep ルールを追加（現状は不要） |
| 署名 | debug 署名のみ | release keystore（リポジトリ外）と `signingConfig`。AAB 生成 |
| USB OTG | 未実装（M3 以降） | BLE を主経路にし、OTG は別マイルストーン |

### 3.4 ビルド・テスト手順とこのホストでの検証可否

```bash
export ANDROID_HOME=/opt/android-sdk
export ANDROID_NDK_HOME=/opt/android-sdk/ndk/29.0.14206865
./scripts/mobile/gen-bindings.sh                    # 生成物（必須。クリーンな worktree では特に）
./scripts/mobile/build-android.sh                   # Rust JNI 込み
./scripts/mobile/build-android.sh --kotlin-only     # NDK 不要のコンパイル検証
```

- 前提はこのホストに揃っている（§6.2）。`--kotlin-only` は NDK 不要。
- **クリーンな checkout / 新しい worktree では `gen-bindings.sh` を先に実行しないと Gradle が通らない**。生成物（Kotlin `core/` と Swift `Generated/`）は `.gitignore` 済みで、無いまま `./gradlew testDebugUnitTest` を実行すると `Unresolved reference 'core'` と `stepCount` 未解決で失敗する（本監査で再現: 1 回目失敗 → `gen-bindings.sh` 後は 8 テスト成功）。`build-android.sh` は内部で生成する。
- 実機 BLE は不可。エミュレータは **BLE 非対応**で、かつホストの emulator 実行ファイルが実行不可（§6.2）。JNI ロードと UI 起動だけならエミュレータで確認できる可能性があるが、**未検証**。

### 3.5 Android の不足タスク（コード外の作業を含む）

1. ナビゲーションと画面分割（現状 1 ファイル 1 画面）。
2. ファイル入出力（SAF: `ACTION_CREATE_DOCUMENT` / `ACTION_OPEN_DOCUMENT_TREE`、永続 URI 権限）。
3. 文言のリソース化（現状は Kotlin に日本語ハードコード。`strings.xml` + `values-en`）。
4. エラー表示の共通化（現状 `error.message` をそのまま表示。英語の Rust 文言が日本語 UI に混ざる）。
5. Bluetooth 有効化 / 権限恒久拒否の導線。
6. 画面維持とバックグラウンド中断の扱い。
7. release 署名・AAB・Play Console 設定（データセーフティ、プライバシーポリシー、クローズドテスト）。
8. CI への Android ビルド追加（現状 CI は Rust のみ。§6.2）。`gen-bindings.sh` を含めること。
9. 入力ストリームの配線（`0x0007` → `0x0003` 20ms）と `0x0007` の MTU 0x3F 副作用の実機確認（§2.3 API-9）。
10. 表示名の変更（`app_name = "Ultimate2 Mobile"` → 自前名称）。R8 / §10 #1。

---

## 4. iOS（`mobile/ios`）

### 4.1 現在の実装範囲

- **画面**: [`ContentView`](../mobile/ios/Sources/EightBitDo/Views/ContentView.swift) の 1 画面。接続 / ファームウェア / 設定概要 / マクロ一覧。**UI 文言は英語のみ**（Android は日本語）。`NavigationStack` の器はあるが遷移先は無い。
- **状態管理**: `AppModel`（`ObservableObject` + `@Published`）。`CBCentralManager` のライフサイクルを所有し、`CoreBluetoothTransport` は接続済み 1 リンクの状態だけを持つ。
- **BLE**: [`CoreBluetoothTransport`](../mobile/ios/Sources/EightBitDo/BLE/CoreBluetoothTransport.swift)。bounded キュー（64、drop-oldest）、`WriteAckTracker`（with-response の世代管理）、`peripheralIsReady` による write-without-response のフロー制御。
- **MTU**: `peripheral.maximumWriteValueLength(for: .withoutResponse) + 3` を ATT MTU とみなし、161 未満はリンクを閉じる。**iOS は MTU を直接公開しない**ための推定。
- **ビルド**: `scripts/mobile/build-ios.sh`（`gen-bindings.sh` → `cargo rustc --target aarch64-apple-ios --crate-type staticlib` → `xtool dev build -c release --ipa`）。
- **テスト**: CoreBluetooth を使わない `WriteAckTracker` のみ、`scripts/mobile/test-write-ack-tracker.sh` でホスト実行（`swiftc`）。

### 4.2 画面一覧と工数（iOS）

画面は Android と同構成。iOS 固有コスト（ローカライズ・Files 連携・プライバシーマニフェスト）を上乗せする。

| # | 画面 | 必要 API | 見積 |
| --- | --- | --- | --- |
| I1 | 接続 / スキャン（既存 + 導線） | 既存 | S |
| I2 | デバイス情報（バッテリー含む） | API-9 | S |
| I3 | プロファイル一覧 / 切替 | API-7 / API-1 | S |
| I4 | 設定値ビュー | API-1 | S |
| I5 | 設定値編集 | API-3 | L |
| I6 | ボタンリマップ | API-2 / API-3 | M |
| I7 | 書き込み確認 / 実行 / 結果 | API-4 / API-10 | M |
| I8 | バックアップ / 復元（Files / `.fileImporter` / `.fileExporter`） | API-8 | M |
| I9 | マクロ一覧 / 詳細 | API-5 | S |
| I10 | マクロエディタ（JSON / TOML 入出力含む） | API-5 / API-6 / API-8 | L |
| I11 | マクロ記録（入力記録） | API-9 | M |
| I12 | マクロ書き込み / 復元 | API-6 | M |
| I13 | ライブ入力 | API-9 | M |
| I14 | 設定 / ライセンス / アプリ情報（ja / en ローカライズ） | （文言） | M |
| I15 | OTA（ブロック） | API-12 | L |
| I16 | 共通基盤（NavigationStack 分割、i18n、エラー共通化） | — | M |

合計（I15 を除く）: S×5（I1–I4, I9）+ M×8（I6–I8, I11–I14, I16）+ L×2（I5, I10）= 2.5–5 + 16–24 + 8–14 = **26.5–43 人日**（約 27–43 人日。S 0.5–1 / M 2–3 / L 4–7 の素点）。

### 4.3 ビルド・署名・配布の制約（macOS 必須の範囲）

| やりたいこと | Linux + xtool で可能か | 必要なもの |
| --- | --- | --- |
| Rust staticlib + SwiftUI のコンパイル確認 | **可能** | xtool + Darwin SDK + `aarch64-apple-ios` |
| 署名なし IPA の生成 | **可能**（実績あり） | — |
| 実機インストール | **条件付きで可能** | USB 接続した iPhone + `xtool dev build --sign --ipa` + Apple ID（このホストは `xtool auth status` がログイン済み） |
| iOS シミュレータでの起動 / デバッグ | **不可** | macOS + Xcode |
| TestFlight / App Store 提出 | **不可** | macOS + Xcode / Transporter（または macOS CI） |
| App のメタデータ・スクリーンショット作成 | Linux でも作成可 | ただし提出は macOS |

- **MTU 推定のリスク**: 常に `.withoutResponse` の最大書込長から MTU を推定している。FF11/FF13 の中には with-response しか広告しない実装があり、その場合 `maximumWriteValueLength(for: .withoutResponse)` が 0 になる可能性がある（0 なら MTU 3 → エラーでリンク切断）。**実機で properties と値を記録し、必要なら with-response 側の値も見るフォールバックを検討**（Tier 3 ではなく transport 契約の変更なのでレビュー対象）。
- `Info.plist` は `NSBluetoothAlwaysUsageDescription` / `UIBackgroundModes = bluetooth-central` 設定済み。書き込みは前景専用のため、バックグラウンドで実行させない UI 制御が必要。
- App Store 提出前に必要: プライバシーマニフェスト（`PrivacyInfo.xcprivacy`）、暗号輸出コンプライアンス（`ITSAppUsesNonExemptEncryption`）、非公式・非提携の Review Note、**アプリ表示名の変更**（[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §3.3）。

### 4.4 iOS の不足タスク

1. 画面分割とナビゲーション。
2. 日本語 / 英語のローカライズ（現状は英語ハードコード。`Localizable.xcstrings` へ）。
3. Files / ドキュメントピッカーによるバックアップ入出力。
4. エラー文言の共通化（現状は `error.localizedDescription` / `String(describing:)`）。
5. 画面維持（`UIApplication.shared.isIdleTimerDisabled`）とバックグラウンド移行時の中断。
6. プライバシーマニフェスト・輸出コンプライアンス・TestFlight 設定。
7. 実機テスト（BLE 読み取り → 書き込み）と MTU 推定の実測（`maximumWriteValueLength` が with-response 専用機種で 0 になる可能性。§4.3）。
8. CI（Linux では xtool ビルドのみ。macOS ランナーが無いと署名はできない）。`gen-bindings.sh` を含めること。
9. 入力ストリームの配線（0x0007 → 0x0003 20ms）と `0x0007` の MTU 0x3F 副作用の実機確認（§2.3 API-9）。
10. 表示名の変更（`CFBundleDisplayName = "Ultimate2 Mobile"` → 自前名称）。R8 / §10 #1。

---

## 5. 共通化（3 実装で重複しない構造）

3 実装 = **現行デスクトップ GUI（egui）/ Android / iOS**。デスクトップは別ブランチで Tauri v2 + Web UI へ移行中（§0 の前提）で、移行後は「Web フロント + Rust コマンド」になるが、**共有する正本は Rust コアと `mobile/shared/` のデータ**であり、本節の共通化方針は変わらない（Web 側の文言は Web の i18n に載せ、同じキー体系を使う）。現状もロジックは Rust にあるが、UI に漏れている知識（文言・エラー分類・書き込み手順・ファイル形式・スロット基底・入力→マクロ変換）が重複の温床。以下を共通化の単位とする。

### 5.1 日本語文言（用語集）

用語の正本を 1 つ決め、UI 文言はキー化する。デスクトップ GUI は `crates/eightbitdo-gui/src/i18n.rs` に日英の対応表を持っているので、**そこから用語を抽出して同じ語を使う**。

| 概念 | 日本語（UI） | English | 備考 |
| --- | --- | --- | --- |
| configuration | 設定値 | Configuration | 「設定」はアプリ設定と紛らわしいので「設定値」 |
| profile | プロファイル | Profile | スロット番号は 1–3 |
| active profile | アクティブプロファイル | Active profile | |
| slot | スロット | Slot | プロファイル / マクロの格納単位 |
| button remapping | ボタン割り当て | Button remapping | 「リマップ」は補助表記に留める |
| macro | マクロ | Macro | M1–M4 |
| macro header | マクロヘッダー | Macro header | |
| step | ステップ | Step | |
| recording（macro input） | 記録（入力記録） | Recording | 音声の「録音」ではない。GUI の現行 i18n は「録音」なので追従して直す（下記） |
| rumble | 振動 | Rumble | |
| stick / joystick | スティック | Stick | |
| dead zone | デッドゾーン | Dead zone | |
| trigger | トリガー | Trigger | |
| gyro / six-axis | ジャイロ | Gyro | |
| LED / lighting | LED | LED | |
| write | 書き込み | Write | |
| read-back | 読み戻し | Read-back | 「検証」ではなく読み戻しと書く |
| backup | バックアップ | Backup | |
| restore | 復元 | Restore | |
| firmware | ファームウェア | Firmware | |
| firmware update / OTA | ファームウェア更新 | Firmware update | 「OTA」は技術文書のみ |
| battery level | バッテリー残量 | Battery level | 生値のみ（% 未確定） |
| unofficial | 非公式 | Unofficial | ストア説明でも維持 |
| recovery | 復旧 | Recovery | 書き込み失敗時 |
| diff | 差分 | Diff | |
| discard changes | 変更を破棄 | Discard changes | |
| retry | 再試行 | Retry | |

- Android / iOS の現状: Android は日本語ハードコード、iOS は英語ハードコード。**同じ画面で言語が違う**ため、早い段階でリソース化する。
- **現状の揺れ（本監査で確認）**: `crates/eightbitdo-gui/src/i18n.rs` は `MacroStartRecording => "録音を開始"` など「録音」を使い、日本語文中に `read-back` をそのまま埋めている（`ConfirmNoteReadback` / `SafetyText`）。「リマップ」も `RemapEnabled` / `ChangeRemapFlag` で使う。共有用語集では **記録 / 読み戻し / ボタン割り当て** を採用語とし、GUI 側は文言変更時に合わせる（Android / iOS は新規なので最初から採用語で書く）。採用語の変更は docs-only ではなく UI 文言の変更なので、Tier 2 以上で扱う。
- 配置案: `mobile/shared/i18n/ja.json` / `en.json`（`{"common.ok": "OK", ...}`）を正本にし、`scripts/mobile/gen-strings.sh` で Android `strings.xml` / `values-en/strings.xml` と iOS `Localizable.xcstrings` を生成。GUI は将来同じキーを `i18n.rs` に対応付ける（無理に生成しない）。

### 5.2 エラー文言とコード

現状は Rust の英語文字列をそのまま表示している（Android の日本語 UI に英語エラーが出る）。`MobileError` のコード / stage / state（API-10）から、共有文言表で日本語・英語を引く。

| コード | 日本語 | English | 復旧 |
| --- | --- | --- | --- |
| `transport.disconnected` | コントローラとの接続が切れました。 | The controller disconnected. | 再接続 |
| `transport.failure` | 通信に失敗しました。 | The Bluetooth link failed. | 再接続 |
| `timeout.read` | 応答がありませんでした。 | The controller did not respond. | 再試行（1 回） |
| `protocol.invalid` | 応答を解釈できませんでした。 | The controller sent an unexpected response. | 再接続して再読み込み |
| `mtu.tooSmall` | 通信設定（MTU）が不足しています。 | The negotiated MTU is too small. | 再接続（OS 設定の変更不可） |
| `backup.failed` | バックアップを保存できませんでした。 | Could not save the backup. | 書き込みを中止 |
| `write.changed` | 接続中に設定値が変わりました。 | The configuration changed while connected. | 読み直してやり直す |
| `write.stage.write` | 書き込みの途中で失敗しました（未保存）。 | The write failed before Save. | 読み直して再試行 |
| `write.stage.save` | 保存の確認が取れませんでした。 | Save was sent but not acknowledged. | 読み直して比較 |
| `write.stage.verify` | 保存後の読み戻しが一致しませんでした。 | The read-back did not match. | バックアップから復元 |
| `macro.stage.*` | マクロ書き込みの各段階 | same | stage ごとの案内 |
| `unsupported` | この操作は未対応です。 | This operation is not supported. | — |

- Rust はコードだけ返し、**文言はアプリ側**が持つ（i18n と同じ JSON に置く）。
- stage / state は core の enum をそのまま写すため、core の復旧文言（`RECOVERY_CONFIG_*` など。GUI が表示している）と意味が一致する。

### 5.3 書き込み安全手順（三重実装しない）

デスクトップの手順をそのまま FFI の二段階 API（API-4 / API-6）で強制する。

```
[読み取り画面]
  ↓ 設定/マクロを読む（sha256 を保持）
[編集]
  ↓ ConfigEdit を適用（ローカル検証: 範囲・KeyMap whitelist・予約領域・no-op）
[begin]
  ↓ 1. 設定を読み直す（TOCTOU: sha256 比較）
  ↓ 2. 差分を返す（UI が表示）
  ↓ 3. 必須バックアップを BackupSink で永続化（失敗したらここで中止）
[確認]
  ↓ 明示的な確認（既定は「中止」。チェックボックス等）
[commit]
  ↓ 0x0001 × N → Save → 待機 → 0x0002 read-back（Rust 内）
[結果]
  ↓ 一致: 完了 / 不一致: stage + state + 復旧案内（自動リトライしない）
  ↓ 失敗: バックアップ参照を表示し、復元手順を案内
```

- **プラットフォームが実装するのは**: ファイル保存（`BackupSink`）、差分表示、確認 UI、結果表示、復旧案内。
- **プラットフォームが実装してはいけないもの**: フレーム列、Save のタイミング、read-back 比較、CRC 解釈。
- 一貫性の担保: `commit_config_write` は `begin_config_write` が返した opaque オブジェクトしか受け付けない。`BackupSink` を必須引数にすることで「バックアップ無しの書き込み」を型で不可能にする。
- 手順の文章は `docs/`（この文書 §5.3）と UI 内ヘルプで共有し、Android / iOS で同じ文面を使う。

### 5.4 テストベクタ

Rust のテストに閉じている現状のベクタを、3 実装が読める共有データにする。

| ファイル | 内容 | 利用者 |
| --- | --- | --- |
| `mobile/shared/test-vectors/config_v1.json` | 2768B raw（hex）、期待 `ConfigView`、CRC 状態、消去 slot（NaN rumble 含む） | Rust / Kotlin JVM / Swift |
| `mobile/shared/test-vectors/macros_v1.json` | 12 マクロ、ステップ、raw SHA-256、空 / 非空 | 同上 |
| `mobile/shared/test-vectors/frames_v1.json` | `read_version` / `read_config` / `write` / `Save` / `switch_profile` の期待フレーム bytes | Rust / Kotlin / Swift |
| `mobile/shared/test-vectors/errors_v1.json` | エラーコード → ja / en 文言 → 復旧アクション | Android / iOS / GUI |
| `mobile/shared/test-vectors/backup_v1/` | v1 バックアップの実ファイル一式（config.bin 等） | 3 実装の round-trip |

- 正本は共有ディレクトリ 1 か所。Rust テストは `CARGO_MANIFEST_DIR` から読み、Kotlin は `src/test/resources`、Swift はテストターゲットのリソースに **コピー**（ビルドスクリプトで）して読む。
- 「3 実装が同じ期待値を持つ」ことを CI（Rust）とローカル（Gradle / swiftc）で検証する。

### 5.5 バックアップファイル形式 v1

デスクトップ CLI / GUI が既に使っている形式を正本にし、モバイルを合わせる。

```
<backup-dir>/
  config.bin              # 2768B raw
  config.json             # size / sha256 / cur_slot / gamepad_mode / slot_flags /
                          # profile_names / crc_stored / crc_in_place
  macros.json             # format: 1, config_file, config_sha256, gamepad_mode,
                          # slot_count, macros_per_slot, macros[]
  macros.toml             # 同じ内容の TOML（manifest は最後に書く）
  slot0-macro0.bin ...    # 各マクロの raw（10B × steps）。ファイル名の slot/macro は 0 始まり
```

- `macros.json` / `macros.toml` の entry は `slot`（0–2）/ `profile`（1–3）/ `index`（0–3）/ `id`（M1–M4）/ `name` / `skipped` / `skip_reason` / `max_steps` / `key_map` / `key_map_name` / `cycles_num` / `interval_ms` / `raw_file` / `raw_sha256` / `response_offsets` / `steps` を持つ。
- **ファイル名の 0 始まりに注意**: CLI は `format!("slot{}-macro{}.bin", slot.index(), index.index())` で書くため `slot0-macro0.bin`〜`slot2-macro3.bin`。manifest の `profile` だけが 1–3。モバイルで同じ形式を作るときはここを混同しない。

- 現行モバイルの `read_config_json` / `read_macro_backup_json` は**この形式ではない**（`raw_hex` を 1 ファイルに埋める）。v1 に寄せるまで「モバイルで保存したファイルをデスクトップで復元」はできない。
- 保存先: Android は SAF のフォルダ、iOS は Files（`.fileImporter` / `.fileExporter`）。Rust は `Vec<BackupFile>`（path + bytes + sha256）を返すだけ。
- 1 ファイル化が必要なら「v1 ディレクトリの zip」。中身のパス構成は変えない。

### 5.6 配置案（重複を作らない構成）

```
crates/eightbitdo-uniffi/        # ロジック・型・安全フロー・バックアップ形式（API-1〜13）
mobile/shared/
  i18n/{ja,en}.json              # UI 文言・エラー文言の正本
  test-vectors/                  # 3 実装共有のベクタ
  README.md                      # 生成手順とキー規約
scripts/mobile/
  gen-bindings.sh                # 既存
  gen-strings.sh                 # 新規: i18n JSON → strings.xml / xcstrings
  build-android.sh / build-ios.sh
mobile/android/                  # 画面・transport・SAF・権限
mobile/ios/                      # 画面・transport・Files・署名
```

- **正本は Rust と `mobile/shared/` のみ**。Android / iOS は表示と OS 連携（権限・ファイル・ライフサイクル）に限定する。

---

## 6. 検証マトリクス

### 6.1 何をどこで検証できるか

| 検証項目 | このホスト（Linux） | 実機（Android / iPhone） | macOS + Xcode | 備考 |
| --- | --- | --- | --- | --- |
| Rust ユニット / ホストテスト（UniFFI 14 件含む） | **可能** | — | — | `cargo test -p eightbitdo-uniffi` |
| Android JVM ユニットテスト（8 件） | **可能**（`gen-bindings.sh` 後） | — | — | SDK 36 / JDK 21 / wrapper。生成物が無いとコンパイルできない（§3.4） |
| Android debug APK（JNI 同梱）ビルド | **可能** | — | — | NDK 29 + cargo-ndk 4.1.2（実績 #21） |
| Android APK の起動 / JNI ロード | **現状不可**（emulator 実行ファイルが root 所有で `o+x` 無し） | **可能** | — | KVM と AVD（android-34 x86_64）は存在。権限修正で起動確認の余地あり |
| Android BLE スキャン / 接続 / 読み書き | 不可 | **必須** | — | エミュレータは BLE 非対応 |
| iOS unsigned IPA ビルド | **可能** | — | — | xtool + Darwin SDK（実績 #21） |
| Swift 純ロジックテスト（`WriteAckTracker`） | **可能** | — | — | `scripts/mobile/test-write-ack-tracker.sh` |
| iOS シミュレータ起動 / デバッグ | 不可 | — | **必須** | Xcode |
| iOS 実機インストール | 条件付きで可能（USB + Apple ID + `xtool --sign`） | **必須** | （Xcode でも可） | このホストは `xtool auth status` ログイン済み。端末未接続 |
| TestFlight / App Store 提出 | 不可 | — | **必須** | macOS + Xcode / Transporter |
| Android ストア提出 | 可能（AAB + keystore + Play Console） | — | — | Google Play は Web から申請 |
| 実機 BLE 書き込み（設定 / リマップ / マクロ） | 不可 | **必須** | — | Tier 3 + pre-write backup + read-back |
| プロファイル切替の永続性 / 実効 | 不可 | **必須** | — | 再接続・電源断を跨ぐ確認 |
| OTA | 不可 | **必須**（wire 確定後） | — | btsnoop 取得が前提 |
| ストア審査（メタデータ・Review Note） | 不可 | — | **必須**（iOS） | Android は Web |

### 6.2 このホストの実測状態（2026-11-23）

| 項目 | 実測 |
| --- | --- |
| OS / Rust | Arch Linux x86_64 / rustc・cargo 1.100.0（`mobile/README.md` と [MOBILE_BUILD_EVIDENCE.md](MOBILE_BUILD_EVIDENCE.md) の記録と一致） |
| Android SDK | `/opt/android-sdk`（`ANDROID_HOME` / `ANDROID_SDK_ROOT` 設定済み）。platforms 34 / 36、build-tools 34 / 36 / 37、NDK `29.0.14206865` |
| cargo-ndk | 4.1.2（`cargo ndk --version`） |
| Rust targets | `aarch64-linux-android` / `x86_64-linux-android` / `aarch64-apple-ios` 導入済み |
| JDK | OpenJDK 21.0.13 |
| xtool | 導入済み（`/usr/bin/xtool`）。`xtool sdk status` = Darwin SDK installed。`xtool auth status` = ログイン済み（Apple ID / Team ID 表示） |
| `xtool devices` | 応答なし（本監査では `timeout 12` でも出力なし。端末未接続と推測。**ハングするので必ず timeout を付ける**） |
| Android エミュレータ | `/opt/android-sdk/emulator/emulator` は `-rwxr--r-- root root` で一般ユーザー実行不可。`/dev/kvm` あり。AVD `ut2`（android-34 / x86_64）あり。system-image `android-34 / google_apis` あり。**BLE はエミュレータで不可** |
| xcodebuild / gradle（グローバル） | 無し（Gradle は wrapper 同梱） |
| Swift ホストテスト | `scripts/mobile/test-write-ack-tracker.sh` = `WriteAckTrackerTests: all checks passed`（本監査で再実行） |
| 本監査での再実行（2026-11-23） | `cargo test -p eightbitdo-uniffi --locked` = **14 passed**、`gen-bindings.sh` = **uniffi 0.31.2（Kotlin / Swift bindgen 一致）**、`./gradlew --no-daemon testDebugUnitTest` = **BUILD SUCCESSFUL（8 テスト）**、`test-write-ack-tracker.sh` = **all checks passed**。IPA / JNI APK の再ビルドはしていない（[MOBILE_BUILD_EVIDENCE.md](MOBILE_BUILD_EVIDENCE.md) の #21 を参照） |

### 6.3 検証コマンド

```bash
# Rust / UniFFI
cargo test -p eightbitdo-uniffi
cargo test --workspace --locked
cargo +1.85.0 check --workspace --all-targets --all-features --locked

# Android（このホストで可能。初回はバインディング生成が必須）
export ANDROID_HOME=/opt/android-sdk
export ANDROID_NDK_HOME=/opt/android-sdk/ndk/29.0.14206865
./scripts/mobile/gen-bindings.sh
(cd mobile/android && ./gradlew --no-daemon testDebugUnitTest)
./scripts/mobile/build-android.sh                 # Rust JNI 込み APK
./scripts/mobile/build-android.sh --kotlin-only   # NDK 不要

# iOS（このホストで可能）
./scripts/mobile/build-ios.sh                     # 内部で gen-bindings + staticlib + xtool
./scripts/mobile/test-write-ack-tracker.sh

# iOS 署名 / 実機（実機 + Apple ID が必要）
cd mobile/ios && xtool dev build --sign --ipa   # 未検証

# ドキュメント検証
git diff --check
# 相対リンクの実在チェック（本監査で 60 リンクを確認）
python3 - <<'EOF'
import re, pathlib
doc = pathlib.Path("docs/MOBILE_NATIVE_PLAN.md")
for m in re.finditer(r"\[[^\]]*\]\(([^)#]+)", doc.read_text()):
    target = m.group(1)
    if target.startswith(("http://", "https://", "mailto:")):
        continue
    assert (doc.parent / target).resolve().exists(), f"missing link: {target}"
print("all relative links exist")
EOF
```

---

## 7. 実装順序（マイルストーン）

前提: 書き込み系は **実機検証 + Tier 3 レビュー + バックアップ UI** が揃うまで公開しない（[mobile/README.md](../mobile/README.md)、[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §5.1）。

| M | 内容 | 含む API / 画面 | 完了条件（exit） | 依存 |
| --- | --- | --- | --- | --- |
| **M0（完了）** | 読み取り専用 + ビルド証跡 | 既存 | APK / IPA のビルド成功（#21） | — |
| **M1 読み取り完成 + 基盤** | 全設定ビュー、ボタンカタログ、ライブ入力、ナビゲーション、i18n、エラー共通化 | API-1 / API-2 / API-9（読み取り）/ API-10（分類）/ API-13。A1–A4, A9, A13, A14, A16 / I1–I4, I9, I13, I14, I16 | 3 実装（GUI / Android / iOS）が同じ `ConfigView` を表示。`request_input` でライブ入力が動く。Rust / Android JVM / Swift ホストテスト green、APK / IPA ビルド green | なし |
| **M2 書き込み基盤（非公開）** | 編集・差分・TOCTOU・BackupSink・二段階書き込み・復旧 UI。**debug フラグ限定** | API-3 / API-4 / API-7 / API-10。A5–A7, A3 / I5–I7, I3 | Linux 実機（参考）で設定書き込み → read-back 一致。Tier 3 レビュー完了。失敗 stage ごとの復旧文言が表示される | M1 |
| **M3 書き込み公開** | 設定編集・リマップ・プロファイル切替を一般公開 | A5–A7 / I5–I7 | **Android / iOS 実機**で書き込み → read-back 一致、リマップの実効（押下→入力）確認、切断時の復旧確認。公開 | M2 + 実機 |
| **M4 マクロ** | ステップ付き読み取り、エディタ、記録、書き込み / 復元 | API-5 / API-6 / API-9（記録）/ API-11。A9–A12 / I9–I12 | 読み取り → 編集 → 書き込み → read-back 一致、復元で元に戻る。**S2 / S3・M3 / M4 の実機検証**（現状は S1 の M1 / M2 のみ） | M3 |
| **M5 ファイル互換** | v1 バックアップ / 復元、JSON / TOML 入出力、デスクトップとの相互運用 | API-8。A8, A10 / I8, I10 | モバイルで保存 → デスクトップ CLI `macro restore` で復元、逆方向も。SHA-256 照合とラウンドトリップテスト | M4 |
| **M6 OTA（ブロック）** | ファームウェア更新 | API-12。A15 / I15 | **前提**: 実機 btsnoop で wire 確定、ACK の offset / total_len 照合、マルチセクション `.dat`、Bootloader MTU。Tier 3。ストア公開可否は別判断（[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §5.4） | core の Phase 2c 完了 |
| **M7 ストア配布** | 表示名変更、署名、プライバシーマニフェスト、TestFlight / クローズドテスト、Review Note | — | iOS: TestFlight 配布 / 審査提出（macOS）。Android: クローズドテスト 12 テスター × 14 日 → production | M3（読み取り + 書き込み）+ M5 |

補足:

- 画面の実装は M1〜M5 で「読み取り → 書き込み基盤 → 公開 → マクロ → ファイル」の順に積む。**Android と iOS は同じマイルストーンを並行**に進められる（共有 API が先）。
- **実機検証は M2 の後半から必須**。このホストだけでは M3 以降の完了条件を満たせない。
- ストア配布は M3 の完了（書き込み公開）を待たず、M1 完了時点の「読み取りのみ版」でクローズドテストを始める選択肢もある（Google Play の 14 日要件を早く満たすため）。

---

## 8. 工数サマリ

### 8.1 画面・プラットフォーム別

| 対象 | S | M | L | 素点（S 0.5–1 / M 2–3 / L 4–7） |
| --- | --- | --- | --- | --- |
| 共有 Rust / UniFFI（API-1〜11, 13） | 6 | 4 | 2 | 3–6 + 8–12 + 8–14 = **19–32** |
| Android 画面（A1–A14, A16） | 6 | 7 | 2 | 3–6 + 14–21 + 8–14 = **25–41** |
| iOS 画面（I1–I14, I16） | 5 | 8 | 2 | 2.5–5 + 16–24 + 8–14 = **26.5–43**（約 27–43） |
| **小計（OTA 除く）** | — | — | — | **71–116** |
| OTA（M6、wire 確定後） | — | — | — | **+9–14** |
| ストア準備・実機検証・QA | — | — | — | **+10–20** |

共有 Rust / UniFFI の内訳:

| API | 内容 | 見積 |
| --- | --- | --- |
| API-1 | 設定の全フィールドビュー（`ConfigView`） | S |
| API-2 | ボタン / KeyMap カタログ | S |
| API-3 | 型付き編集 + 検証 + prepare（I/O なし） | M |
| API-4 | 設定書き込みの二段階 + BackupSink + TOCTOU + report | L |
| API-5 | マクロ読み取り（ステップ付き、`MacroBackupData`） | M |
| API-6 | マクロ書き込み / 復元の二段階 | L |
| API-7 | プロファイル切替（0x0015） | S |
| API-8 | バックアップ v1 の入出力 + manifest 検証 | M |
| API-9 | `request_input` + `input_to_macro_step` + バッテリー | S |
| API-10 | 構造化エラー（stage / state / TOCTOU） | M |
| API-11 | 進捗 + 読み取り限定キャンセル | S |
| API-13 | `mobile_api_version` / `sha256_hex` / slot 変換 / resync | S |

- 1 人で進める場合: **約 3.5–6 か月**（71–116 人日、OTA・ストア除く）。2 人（共有 Rust → Android / iOS 並行）で **約 2.5–4 か月**（共有 19–32 人日の後に、27–43 人日のプラットフォーム側を並行）。
- 見積りは「経験者が連続作業した場合」の粗い値。Tier 3 レビュー待ち・実機待ち・ストア審査待ちは含まない。
- 最も重いのは **設定編集画面（L）とマクロエディタ（L）**。どちらもプラットフォームごとに UI を作り直す必要がある（ロジックは Rust）。共有側の L は **API-4（設定書き込み）と API-6（マクロ書き込み）**で、ここが安全手順・TOCTOU・バックアップを含むため Tier 3 の中心になる。

### 8.2 クリティカルパス

1. API-1〜4（設定ビュー・編集・書き込み安全フロー）→ M2 → 実機検証 → M3
2. API-5〜6（マクロ）→ M4 → 実機検証（S2/S3・M3/M4）
3. 実機の調達（このホストに無い）
4. iOS の macOS 環境（ストア配布）

### 8.3 ブロッカー（工数に含めない外部依存）

| ブロッカー | 影響する作業 | 必要なもの / 現状 |
| --- | --- | --- |
| **BLE 実機**（Android 端末 + 8BitDo Ultimate 2） | M2 後半〜M6 の検証、書き込み公開（M3） | このホストに無い。読み取り → 書き込みの順で 1 台から |
| **iPhone 実機 + Apple ID** | iOS の実機インストール / BLE 検証 | このホストは `xtool auth status` ログイン済みだが端末未接続。無料 Apple ID では配布証明書なし |
| **macOS + Xcode** | iOS シミュレータ、TestFlight / App Store 提出、プライバシーマニフェスト検証 | 無し（Linux では unsigned IPA まで） |
| **ファームウェアイメージ + btsnoop 採取環境** | OTA（M6）の wire 確定 | Android の HCI snoop または Linux `btmon`。公式 `.dat` はリポジトリに置かない |
| **表示名の決定** | ストア申請（M7） | `Padbench` 系列に揃えるか別名か（§10 #1）。`applicationId` / `bundleID` の再確認も |
| **ストアアカウント** | Android 本番公開 / iOS 配布 | Google Play のクローズドテスト（12 テスター × 14 日）、Apple Developer Program（有料） |

---

## 9. リスク

| # | リスク | 影響 | 対策 | 検出方法 |
| --- | --- | --- | --- | --- |
| R1 | **実機 BLE が無い**（このホストにデバイスが無い） | 読み取り以降の全検証が止まる | 実機を調達し、読み取り → 書き込みの順で検証。M3 まで書き込みを公開しない | 実機テスト |
| R2 | iOS の MTU 推定が with-response 専用機種で誤る（`.withoutResponse` の最大書込長が 0 になる可能性） | 接続失敗（リンク切断） | 実機で properties と `maximumWriteValueLength` を記録。必要なら with-response 側の値も見るフォールバック | 実機ログ / btsnoop |
| R3 | write-without-response のフロー制御・キュー溢れ | フレーム欠落 → 読み取り失敗 / 書き込み失敗 | 既存の fail-closed（部分成功にしない）を維持。74B cap を守る | 実機 + Rust テスト |
| R4 | Android OEM 差（MTU 200 要求失敗、CCCD 失敗、GATT 切断） | 接続不可 / 読み取り不可 | エラー文言の整備、機種別の記録、再試行 1 回 | 実機マトリクス |
| R5 | プロファイル切替（0x0015）の永続性・実効・ACK 形状が未確認 | 切替 UI が誤った成功表示をする | 切替後に config を読み直して `cur_slot` を表示。再接続後の再確認を案内。Save 不要と断定しない | 実機 |
| R6 | マクロ書込の実機証跡が **S1 の M1 / M2 のみ**（S2/S3・M3/M4 は外挿） | 一部スロットで永続化しない可能性 | M4 の exit 条件に実機検証を含める。UI に検証範囲を明記 | 実機 |
| R7 | OTA の wire 未確定（ACK 照合・マルチセクション `.dat`・Bootloader MTU） | OTA 実装不能 | **配線しない**。core の Phase 2c を先に完了させる | btsnoop |
| R8 | 表示名が商標方針に不適合（"Ultimate2 Mobile"） | ストア却下 | 自前名称へ変更（[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §3.3）。識別子も再確認 | 申請前チェック |
| R9 | iOS の署名 / ストアが macOS 必須 | リリース不能 | macOS + Xcode 環境（または macOS CI）を用意。Linux は unsigned IPA / 実機署名まで | ビルド / 提出 |
| R10 | Google Play 個人アカウントのクローズドテスト（12 テスター × 14 日） | 公開遅延 | 読み取りのみ版で早期にクローズドテスト開始 | Play Console |
| R11 | UniFFI 0.31.2 固定（0.32 系は MSRV 超過） | API 追加時に bindgen を上げられない | 0.31.2 の範囲で設計。必要なら bindgen を workspace 外へ分離 | CI `msrv` |
| R12 | 大きな `Vec<u8>` の境界コピー（2768B + マクロ 24KB） | 遅延・メモリ | opaque な `MacroBackupData` で保持。JSON は必要時のみ | プロファイリング |
| R13 | バックアップ形式の不一致（デスクトップ v1 vs モバイル JSON） | 相互運用不可・移行コスト | v1 に統一（API-8）。ラウンドトリップテスト | M5 |
| R14 | rumble の NaN（消去 slot）を `f32` で公開してしまう | JSON 化失敗・等価比較の誤り | ビット（`u32`）で公開、level は `Option<u8>` | 消去 slot のベクタ |
| R15 | 長時間操作（マクロ backup 数十秒）がキャンセル不可 | UX / ANR | IO ディスパッチャ + 画面維持 + 進捗。中断は読み取り限定で設計 | 実機 |
| R16 | 前景専用の制約（バックグラウンド suspend でタイマー破綻） | 書き込み失敗 | 書き込み中は keep-screen-on。バックグラウンド移行で中断・復旧案内 | 実機 |
| R17 | CI にモバイルビルドが無い | 回帰を検出できない | Android / iOS ビルドを CI に追加（重いので nightly 等） | CI |
| R18 | 書き込み API の公開が先行して実機未検証のまま出る | デバイス破損（設定 / マクロ） | Tier 3 + pre-write backup + read-back + debug フラグで段階公開 | レビュー / 実機 |
| R19 | **空スロットへのマクロ新規作成が core で未実装**（`Error::EmptyMacroWrite`、step-data clear なし） | マクロエディタで新規作成できない（既存の非空マクロの編集・記録上書きのみ） | core に step-data clear を実装するまで UI に「新規作成」を出さない。実装するなら Tier 3 + 実機検証 | 実装 / レビュー |
| R20 | **`0x0007` 有効化の副作用（クライアント MTU 0x3F）と通知 cadence が未検証** | ライブ入力・記録のレートが公式と異なる / 読み取り・書き込みと干渉する | 実機で `0x0007` 有無 × `0x0003` 周期を A/B。`request_input` を公開して周期をホスト側で制御し、MTU は交渉値を別に保持する | 実機 / btsnoop |

---

## 10. 未決事項（オープンクエスチョン）

1. **アプリの最終名称**: デスクトップは別ブランチで `Padbench` へ改名予定（§0 前提）。モバイルも同じ系列名にするか、別名にするか。[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §3.3 の「商標・製品名を使わない」決定に合わせ、`applicationId` / `bundleID` も同時に確認する。
2. **書き込み公開の条件**: どの機種 / OS バージョンで実機検証できたら公開するか（Android 12+ / iPhone の最低 2 機種など）。
3. **記録の仕様**: `0x0007`（ReportEnable）と `0x0003`（ReadKeysAndJoy）のどちらを記録に使うか。`0x0007` の MTU 0x3F 副作用をどう扱うか。ポーリング周期（≈20ms）と 1ms 分解能の扱い（丸め / 保持）。
4. **バックアップの既定保存先**: Android SAF の永続 URI、iOS Files のどちらを既定にするか。暗号化はしない方針でよいか（設定値は機微情報ではないが端末外に出る）。
5. **プロファイル copy / delete をモバイルに出すか**（core の helper はあるが CLI / GUI 未露出。3 実装で同時に出すか）。
6. **OTA をストア版に含めるか**（[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §5.4。含めない場合はデスクトップ限定）。
7. **UI 言語**: 日本語を既定、英語を追加でよいか（iOS は現状英語、Android は日本語）。
8. **対象 OS**: iOS 17+ / Android 8.0+（minSdk 26）のままでよいか。
9. **`MobileError` のバリアント追加をどのタイミングで行うか**（アプリと同時更新が必要。`mobile_api_version()` の運用）。
10. **CI のモバイルビルド**: どこまで回すか（Rust テストは既存 CI、APK / IPA は nightly か手動か）。
11. **空マクロ作成を core に実装するか**: step-data clear（フラッシュの 0 埋め書き込み）の wire 手順が未確定。実装するなら OTA と同じく Tier 3 + 実機 btsnoop が必要（§9 R19）。

---

## 付録 A: 画面 ⇔ API 対応

| 画面 | 主な API | 既存 / 追加 |
| --- | --- | --- |
| 接続 / スキャン | `MobileSession::new` / `disconnect` / `NativeTransport` | 既存 |
| デバイス情報 | `read_device_info` / `last_battery` | 既存 + API-9 |
| プロファイル一覧 / 切替 | `read_config`（`ConfigView`）/ `switch_profile` | API-1 / API-7 |
| 設定値ビュー | `read_config` → `describe_config_v2` | API-1 |
| 設定値編集 | `button_catalog` / `keymap_catalog` / `validate_config_edits` / `prepare_config_edits` | API-2 / API-3 |
| リマップ | `RemapView` / `ConfigEdit::ButtonRemap` | API-1 / API-3 |
| 書き込み確認 / 実行 | `begin_config_write` / `commit_config_write` / `ConfigWriteReport` / `MobileError::ConfigWrite` | API-4 / API-10 |
| バックアップ / 復元 | `export_backup_files` / `import_backup_files` / `validate_backup_files` | API-8 |
| マクロ一覧 / 詳細 | `read_macro_backup_data` / `MacroEntryView` | API-5 |
| マクロエディタ | `MacroStep` / `prepare_macro_write` / `commit_macro_write` | API-5 / API-6 |
| マクロ記録（入力記録） | `request_input` / `read_input_once` / `input_to_macro_step` | API-9 |
| マクロ書き込み / 復元 | `prepare_macro_write` / `commit_macro_write` / `prepare_macro_restore` / `commit_macro_restore` | API-6 |
| ライブ入力 | `set_report_enable` / `request_input` / `read_input_once` | 既存 + API-9 |
| 設定 / ライセンス | （Rust 不要。文言と同梱テキスト） | — |
| OTA | API-12 | 追加（wire 確定後） |

---

## 付録 B: 参照ファイル

- 仕様・証跡: [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md)、[STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md)、[DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md)、[OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md)、[OTA_VERIFICATION_PLAN.md](OTA_VERIFICATION_PLAN.md)
- 計画・状態: [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md)、[FEATURE_MATRIX.md](FEATURE_MATRIX.md)、[CROSS_PLATFORM.md](CROSS_PLATFORM.md)、[SESSION_OPERATIONS.md](SESSION_OPERATIONS.md)、[REVIEW_TIERS.md](REVIEW_TIERS.md)
- 方針: [DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md)、[../TRADEMARKS.md](../TRADEMARKS.md)、[../AGENTS.md](../AGENTS.md)
- モバイル: [../mobile/README.md](../mobile/README.md)、[../mobile/CHANGELOG.md](../mobile/CHANGELOG.md)、[MOBILE_BUILD_EVIDENCE.md](MOBILE_BUILD_EVIDENCE.md)
- コード: [../crates/eightbitdo-uniffi/src/lib.rs](../crates/eightbitdo-uniffi/src/lib.rs)、[../crates/eightbitdo-core/src/config_ops.rs](../crates/eightbitdo-core/src/config_ops.rs)、[../crates/eightbitdo-core/src/macro_ops.rs](../crates/eightbitdo-core/src/macro_ops.rs)、[../crates/eightbitdo-core/src/profile_ops.rs](../crates/eightbitdo-core/src/profile_ops.rs)、[../crates/eightbitdo-core/src/session.rs](../crates/eightbitdo-core/src/session.rs)、[../crates/eightbitdo-core/src/error.rs](../crates/eightbitdo-core/src/error.rs)、[../crates/eightbitdo-gui/src/driver.rs](../crates/eightbitdo-gui/src/driver.rs)、[../crates/eightbitdo-gui/src/i18n.rs](../crates/eightbitdo-gui/src/i18n.rs)、[../crates/eightbitdo-cli/src/main.rs](../crates/eightbitdo-cli/src/main.rs)

---

## 付録 C: 本監査で確認したこと（再現手順）

```bash
# 1. UniFFI 公開 API（export・record・error の列挙）
grep -n "uniffi::export\|uniffi::Record\|uniffi::Error\|uniffi::Object\|with_foreign" \
  crates/eightbitdo-uniffi/src/lib.rs

# 2. デスクトップ側の機能（CLI サブコマンド）
grep -n "enum Command\|enum ConfigCommand\|enum MacroCommand\|enum MapCommand" \
  crates/eightbitdo-cli/src/main.rs

# 3. モバイルの画面・呼び出し
grep -rn "MobileSession\|readConfig\|readMacroBackup\|setReportEnable\|readInputOnce" \
  mobile/android mobile/ios --include=*.kt --include=*.swift

# 4. 共有できる形式（CLI の config.json / macros.json）
grep -n "json_summary\|struct MacroBackupDocument\|format: 1" crates/eightbitdo-cli/src/main.rs

# 5. ホストのツールチェーン（§6.2）
echo "$ANDROID_HOME"; ls /opt/android-sdk/ndk /opt/android-sdk/platforms
rustup target list --installed; cargo ndk --version; java -version
xtool sdk status; xtool auth status
ls -l /opt/android-sdk/emulator/emulator; ls /dev/kvm

# 6. テスト（Android は gen-bindings 後）
cargo test -p eightbitdo-uniffi --locked
./scripts/mobile/gen-bindings.sh
(cd mobile/android && ./gradlew --no-daemon testDebugUnitTest)
./scripts/mobile/test-write-ack-tracker.sh

# 7. 入力ストリームの実体（request_input が FFI に無いことの確認）
grep -n "request_input" crates/eightbitdo-core/src/session.rs
grep -c "request_input" crates/eightbitdo-uniffi/src/lib.rs   # 0

# 8. ドキュメント検証（相対リンク + 空白エラー）
git diff --check
```

> 注: `xtool devices` はこのホストで応答が無い（`timeout 12` でも出力なし）。実行する場合は必ず `timeout` を付けること。`xtool auth status` はログイン済み（Apple ID / Team ID を表示）だが、実機は接続されていない。
