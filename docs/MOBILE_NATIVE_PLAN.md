# モバイルネイティブ 機能ギャップ監査・実装計画

- 監査日: 2026-11-23（対象 base: `81b0ac1`、PR #30 で main にマージ済み）
- 位置づけ: モバイルの現行方針は **Tauri v2 モバイル + ネイティブ BLE プラグイン**（[TAURI_PLAN.md](TAURI_PLAN.md) P8）。本書は、それまでの代替・先行実装である **UniFFI + ネイティブ UI（iOS SwiftUI / Android Jetpack Compose）**のギャップ監査と実装計画をまとめたもの。プロトコル・書き込み順序の正本は [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) と [STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md)。
- 表示名・識別子: **Padbench** / `io.github.haruki0420.padbench`（PR #32 で統一済み）。

## 1. 要約

| 項目 | 結論 |
| --- | --- |
| 現状（M0） | 読み取り専用（スキャン → 接続 → バージョン / 2768B 概要 / 12 マクロ一覧）。書き込み系は UniFFI に未露出（純関数 `prepare_config_write` のみ） |
| 主な不足 | ①設定全フィールド読取、②型付き編集（`ConfigEdit`）、③安全フロー付き設定書込（backup + TOCTOU + read-back）、④22 ボタンリマップ、⑤マクロ書込・復元、⑥プロファイル切替、⑦`request_input`（`0x0003`）と `0x0007` 副作用確認、⑧デスクトップ v1 互換バックアップ形式、⑨構造化エラー、⑩空マクロ新規作成（core が `EmptyMacroWrite` で拒否） |
| 画面数 | Android / iOS とも 16 項目（画面 14 + OTA + 共通基盤、うち既存は 1）。ナビゲーション未実装 |
| 工数概算 | 共有 Rust/UniFFI **19–32 人日** + Android **25–41 人日** + iOS **27–43 人日** = **71–116 人日**（OTA +9–14、ストア準備 +10–20、共有基盤・CI +3–7 は別枠） |
| ブロッカー | Android / iPhone 実機（ホストに無し）、iOS 署名・ストア提出（macOS + Xcode 必須）、OTA の wire 未確定、マクロ書込の実機証跡が S1 の M1/M2 のみ |

> ホスト環境（Linux x86_64）には BLE アダプタと Ultimate 2 実機があり、共有 Rust の読み書きは CLI で検証済み（[DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md)）。Android / iPhone 端末が無いためモバイルアプリ経由の実機検証のみ未実施。

## 2. 現状と機能ギャップ

### 2.1 実装済み（M0）

- **UniFFI**（`crates/eightbitdo-uniffi`、0.31.2 固定）: `MobileSession`（version / config / macro backup / `set_report_enable` / `read_input_once` / disconnect）、`NativeTransport` callback、bounded 受信キュー（64、drop-oldest）、`MIN_READ_MTU = 161`、`describe_config`、`prepare_config_write`、`config_write_chunk_len`。単体テスト 14 件。
- **Android**（`mobile/android`）: Compose 1 画面、`BluetoothGattTransport`（CCCD `0x2902` 必須、`requestMtu(200)` 必須・フォールバックなし、write-without-response / with-response 対応）、JVM テスト 8 件、JNI 同梱 debug APK ビルド成功（[MOBILE_BUILD_EVIDENCE.md](MOBILE_BUILD_EVIDENCE.md)）。UI は日本語ハードコード。
- **iOS**（`mobile/ios`）: SwiftUI 1 画面、`CoreBluetoothTransport`、`WriteAckTracker`（ホストテスト 5 チェック）、unsigned IPA ビルド成功。MTU は `maximumWriteValueLength(for: .withoutResponse) + 3` で推定。UI は英語ハードコード。

### 2.2 コア / デスクトップとの機能差分

| 機能 | core / デスクトップ | UniFFI / モバイルの現状 | 必要 API |
| --- | --- | --- | --- |
| バージョン・2768B 概要・マクロ一覧 | あり | 実装済み（`read_config_json` 等は UI 未使用） | 既存 |
| 設定の全フィールド読取 | あり | 概要 5 項目のみ | API-1 |
| 22 ボタン割当読取・カタログ | あり | なし | API-2 |
| 型付き設定編集・ボタンリマップ | `ConfigEdit` 13 種（copy/delete は protocol ヘルパのみ） | raw 編集のみ | API-3 |
| 設定書込（backup + TOCTOU + read-back） | `Session::write_full_config` | なし | API-4 |
| マクロ読取（ステップ付き）・書込・復元・ヘッダ書込 | `write_macro` / `write_macro_slot` / `write_macro_header` | 概要のみ。空マクロ作成は core 未実装（`EmptyMacroWrite`） | API-5, 6 |
| プロファイル切替（`0x0015`） | `Session::switch_profile` | なし（永続性・実効は実機未確認） | API-7 |
| バックアップ / 復元（v1 形式） | `config.{bin,json}` / `macros.{json,toml}` / `slot{0..2}-macro{0..3}.bin` | 独自 JSON で非互換 | API-8 |
| ライブ入力・マクロ記録・バッテリー | `set_report_enable` + 20ms `request_input`（`0x0003`） | `request_input` 欠落、画面なし、バッテリー生値スケール未確認 | API-9 |
| 構造化エラー（stage / state / TOCTOU） | `ConfigWriteStage` 等 | 文字列に平坦化 | API-10 |
| 進捗・読取キャンセル | — | なし（マクロ backup は数十秒ブロック） | API-11 |
| OTA | core のみ（wire 未確定） | 意図的に未公開（ブロック） | API-12 |

## 3. 追加が必要な UniFFI API 契約案

既存 export を削除・改名せず追加のみで行う。バージョン判定は `mobile_api_version()`（API-13）で行う。

| ID | 概要・主な型と関数 | 設計・安全上の要点 | 見積 |
| --- | --- | --- | --- |
| **API-1** | `describe_config_v2(raw) -> ConfigView`（`ProfileView` × 3、Rumble / Joy / Trigger / Special / Gyro / LED 3 種 / Remap / MacroHeader） | 消去 slot の rumble は NaN を持つため `f32` を `u32` ビット（`left_bits` / `right_bits`）で返し、表示用は `Option<u8>` | S |
| **API-2** | `button_catalog() -> Vec<ButtonEntry>`（22 件）、`keymap_catalog() -> Vec<KeyMapEntry>` | 許可キー（`KeyMap::is_single_documented()`）の正本を Rust に固定 | S |
| **API-3** | `ConfigEdit` enum（13 種 + `CopySlot` / `DeleteSlot`）、`validate_config_edits`、`prepare_config_edits` | I/O なしの純関数。範囲・flag・whitelist・予約領域・no-op を検証。`Gyro` は core 名 `SixAxis` に統一。NaN 復元は raw 経路を使用 | M |
| **API-4** | `BackupSink` callback、`begin_config_write(expected_sha256, edits, sink) -> ConfigWritePreparation`、`commit_config_write(prep) -> ConfigWriteReport` | `BackupSink` を必須引数にし、backup なしの書込を型で禁止。TOCTOU 再読込・`0x0001` × N → Save → read-back を Rust 内で完結 | L |
| **API-5** | `MacroStep`、`MacroEntryView`、`MacroBackupData`（opaque）、`read_macro_backup_data()` | 設定と 12 マクロを 1 回の読取で保持。`MacroFile` と同一フィールド名、上限 `MAX_MACRO_STEPS = 200` | M |
| **API-6** | `prepare_macro_write` / `commit_macro_write`、`prepare_macro_restore` / `commit_macro_restore`、`write_macro_header_only` | slot 単位バッチ（Header → Save → Erase → Write → Save）を core で保証。空マクロは拒否。pre-write / pre-restore backup と TOCTOU 必須 | L |
| **API-7** | `switch_profile(slot: u8)`（1..=3） | `0x0015` を Save なしで送信。成功後に config を再読込して `cur_slot` を表示し、再接続後の確認を案内 | S |
| **API-8** | `export_backup_files(MacroBackupData) -> Vec<BackupFile>`、`import_backup_files`、`validate_backup_files` | デスクトップ v1 形式に統一。相対パス正規化で `..`・絶対パス・重複・許容集合外を拒否。ファイル I/O は Android SAF / iOS Files に委譲 | M |
| **API-9** | `request_input()`、`last_battery() -> Option<u8>`、`input_to_macro_step(snapshot, duration_ms)` | 公式相当の ~20ms `0x0003` ポーリングを可能にする。`0x0007` 有効化時のクライアント MTU `0x3F` 切替副作用は実機検証が必要 | S |
| **API-10** | `ConfigWriteStage/State`、`MacroWriteStage/State`、`MobileError` 拡張（`Backup` / `ChangeDetected` / `ConfigWrite` / `MacroWrite`） | Rust はコードと stage/state のみ返し、復旧文言はアプリ側の i18n JSON から引く | M |
| **API-11** | `OperationObserver`（進捗・読取限定キャンセル） | キャンセルは読取のフレーム合間のみ。flash 書込には付けない | S |
| **API-12** | `enter_boot` / `read_boot_pid` / `prepare_firmware` / `write_firmware`（**wire 確定後**） | `with_unverified_wire_allowed` は公開しない。署名検証・公式配布元・デバイス宛のみの 3 条件を満たすこと | — |
| **API-13** | `mobile_api_version`、`sha256_hex`、`slot_number` / `slot_index`、`resync` | slot 基底は新 API で `1..=3`（index は `0..=3`）に統一 | S |

### UniFFI・ビルド上の制約

- **UniFFI 0.31.2 固定**: 0.32 系は推移依存が MSRV 1.85 を超える（`scripts/mobile/gen-bindings.sh` の冒頭コメント「0.32.2」は誤記）。
- **生成物は非コミット**: クリーンな worktree では `./scripts/mobile/gen-bindings.sh` を先に実行しないと Gradle / xtool がコンパイルできない。
- **大きなバッファ**: 2768B + 12 マクロ（最大 24KB）を境界で何度もコピーしないよう `MacroBackupData` は opaque Object で保持する。

## 4. 画面一覧とプラットフォーム固有事項

### 4.1 画面一覧（Android A1–A16 / iOS I1–I16 共通構成）

見積凡例: **S = 0.5–1 / M = 2–3 / L = 4–7 人日**（経験者 1 名、レビュー・実機待ち除く）。

| # | 画面・項目 | 主な内容 | 必要 API | 見積 |
| --- | --- | --- | --- | --- |
| 1 | 接続 / スキャン | 既存 + BT 有効化要求・権限恒久拒否時の設定導線・再試行 | 既存 | S |
| 2 | デバイス情報 | FW 版、接続状態、バッテリー生値（% 未確定注記） | API-9 | S |
| 3 | プロファイル一覧 / 切替 | 3 スロット表示、`0x0015` 切替と再読込確認 | API-1, 7 | S |
| 4 | 設定値ビュー | 全レコードの読み取り表示 | API-1 | S |
| 5 | 設定値編集 | 全セクション編集フォーム、範囲検証、差分プレビュー、破棄確認 | API-3 | L |
| 6 | ボタンリマップ | 22 ボタン × 3 スロット、カタログピッカー、既定値戻し | API-2, 3 | M |
| 7 | 書込確認 / 実行 / 結果 | 差分 → backup → 確認 → 進捗 → read-back / stage 別復旧案内 | API-4, 10 | M |
| 8 | バックアップ / 復元 | Android SAF / iOS Files で v1 ファイル群の入出力、SHA-256 表示 | API-8 | M |
| 9 | マクロ一覧 / 詳細 | 12 マクロ一覧 + ステップ表閲覧 | API-5 | S |
| 10 | マクロエディタ | ステップ追加・削除・並替・編集、JSON / TOML 入出力 | API-5, 6, 8 | L |
| 11 | マクロ記録（入力記録） | `request_input` ポーリング → `input_to_macro_step`（上限 200） | API-9 | M |
| 12 | マクロ書込 / 復元 | 1 本 / slot 一括書込・復元、確認、read-back | API-6 | M |
| 13 | ライブ入力 | スティック・トリガー・ボタン・バッテリー可視化 | API-9 | M |
| 14 | 設定 / ライセンス / 情報 | 言語切替（ja / en）、既定保存先、MIT / Apache / 第三者通知、非公式表記 | — | S (iOS: M) |
| 15 | OTA（**ブロック**） | wire 確定後に実装 | API-12 | L（別枠） |
| 16 | 共通基盤 | Navigation 分割、ViewModel / 状態管理、エラー表示共通化、i18n | — | M |

### 4.2 プラットフォーム固有の制約と不足タスク

- **Android（`mobile/android`）**:
  - 前景専用設計: 読取・書込中は `FLAG_KEEP_SCREEN_ON`、バックグラウンド移行時は中断して復旧案内。自動再接続は二重送信防止のため入れない。
  - 権限・BT 無効: `ACTION_REQUEST_ENABLE` と恒久拒否時の設定リンクを追加。release 時は keystore と AAB、Play Console のクローズドテスト（12 テスター × 14 日）が必要。
  - USB OTG は別マイルストーン。
- **iOS（`mobile/ios`）**:
  - Linux + `xtool` で unsigned IPA 生成と条件付き実機署名（USB + Apple ID）は可能だが、**シミュレータ・TestFlight・App Store 提出は macOS + Xcode 必須**。`xtool devices` は端末未接続時にハングするため必ず `timeout` を付ける。
  - MTU 推定リスク: with-response 専用特性で `maximumWriteValueLength(for: .withoutResponse)` が 0 になる可能性があるため、実機で特性と値を実測し必要ならフォールバックを検討する。
  - 申請前に `PrivacyInfo.xcprivacy`、`ITSAppUsesNonExemptEncryption`、非公式・非提携の Review Note、日英 `Localizable.xcstrings`、`isIdleTimerDisabled` が必要。

## 5. 3 実装間の共通化方針

デスクトップ（Tauri v2）・Android・iOS でロジック・文言・形式を重複させない。

- **正本の配置**: ロジック・型・安全フローは `crates/eightbitdo-uniffi/`、UI / エラー文言の正本は `mobile/shared/i18n/{ja,en}.json`（`gen-strings.sh` で `strings.xml` / `Localizable.xcstrings` を生成）、共有テストベクタは `mobile/shared/test-vectors/`（config / macros / frames / errors / backup_v1）に置く。
- **用語統一**: デスクトップ Tauri 版の用語集（[DESIGN_TAURI.md](DESIGN_TAURI.md)）に合わせ、**設定値 / ボタン割り当て / 記録（入力記録）/ 読み戻し / ファームウェア更新 / バッテリー残量（生値）**を採用する。
- **安全フローの強制**: `begin_*`（TOCTOU 再読込 + 差分 + `BackupSink` 永続化）→ 明示確認（既定は中止）→ `commit_*`（書込 + Save + read-back、自動リトライなし）を Rust 側で完結させ、プラットフォーム側にパケット列や CRC 解釈を書かせない。
- **バックアップ形式 v1**: `config.bin` / `config.json` / `macros.json`（`format: 1`）/ `macros.toml` / `slot{0..2}-macro{0..3}.bin`（ファイル名の slot / macro は 0 始まり、manifest の `profile` のみ 1–3）。1 ファイル化が必要なら v1 ディレクトリの zip とする。

## 6. 検証マトリクスと実装マイルストーン

### 6.1 検証環境（2026-11-23 実測）

| 検証項目 | Linux ホスト | Android / iPhone 実機 | macOS + Xcode |
| --- | --- | --- | --- |
| Rust / UniFFI（14 件）・Android JVM（8 件）・Swift ホスト（5 件） | 可（すべて通過） | — | — |
| Android JNI 同梱 APK / iOS unsigned IPA ビルド | 可（実績 #21） | — | — |
| Android / iOS アプリ経由の BLE 読取・書込・JNI ロード | 不可（エミュレータは BLE 非対応かつ実行権限なし） | **必須** | — |
| iOS シミュレータ・TestFlight・App Store 提出 | 不可 | — | **必須** |
| 共有 Rust の BLE 読取・書込・`0x0015` 永続性・`btmon` 採取 | 可（CLI で検証） | — | — |

### 6.2 マイルストーン

書き込み系は **実機検証 + Tier 3 レビュー + バックアップ UI** が揃うまで公開しない（[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §5.1）。

| M | 内容 | 対象 | 完了条件（exit） |
| --- | --- | --- | --- |
| **M0（完了）** | 読取専用 + ビルド証跡 | 既存 | APK / IPA ビルド成功（#21） |
| **M1** | 読取完成 + 基盤 | API-1, 2, 9(読取), 10, 13 / 画面 1–4, 9, 13, 14, 16 | 3 実装で同一 `ConfigView`、`request_input` ライブ入力、全テスト・ビルド green |
| **M2** | 書込基盤（debug 限定） | API-3, 4, 7, 10 / 画面 3, 5–7 | Linux 参考検証 + Tier 3 レビュー完了、stage 別復旧案内 |
| **M3** | 書込公開 | 画面 5–7 | Android / iOS 実機で書込 → read-back・リマップ実効・切断復旧を確認 |
| **M4** | マクロ | API-5, 6, 9(記録), 11 / 画面 9–12 | 編集・記録・書込・復元 + **S2/S3・M3/M4 の実機検証** |
| **M5** | ファイル互換 | API-8 / 画面 8, 10 | モバイル ⇔ デスクトップ CLI の v1 バックアップ相互復元 |
| **M6（ブロック）** | OTA | API-12 / 画面 15 | Phase 2c 完了（btsnoop・ACK 照合・マルチセクション `.dat`・MTU）+ Tier 3 |
| **M7** | ストア配布 | — | iOS TestFlight / 審査（macOS）、Android クローズドテスト（12 人 × 14 日）→ 本番 |

## 7. 主なリスクと未決事項

### リスク

- **実機・環境制約（R1, R9, R10, R17）**: Android / iPhone 実機と macOS が無いと M3 / M7 が進まない。CI にモバイルビルドが無い点も nightly 等で補う必要がある。
- **BLE 実装差（R2, R3, R4, R15, R16）**: iOS の MTU 推定（with-response 専用特性で 0 になる懸念）、Android OEM 差（MTU 200 / CCCD）、write-without-response 溢れ（74B cap + fail-closed 維持）、バックグラウンド移行による書込中断（前景専用 + keep-screen-on）。
- **プロトコル未確定・未検証（R5, R6, R7, R19, R20）**: `0x0015` の永続性・実効、マクロ書込の S2/S3・M3/M4、OTA wire、空マクロ新規作成（step-data clear 未実装）、`0x0007` の MTU `0x3F` 副作用。
- **安全・互換性（R11–R14, R18）**: UniFFI 0.31.2 固定、境界コピー削減、v1 形式統一、rumble NaN の `u32` ビット扱い、未検証書込の先行公開防止。

### 未決事項

1. 書込公開に必要な実機検証の最低台数・OS バージョン基準。
2. マクロ記録で `0x0007` と `0x0003` をどう組み合わせるか（MTU `0x3F` 副作用と ~20ms 記録・1ms 保持の扱い）。
3. バックアップ既定保存先（SAF 永続 URI / Files）と暗号化不要方針の確認。
4. `CopySlot` / `DeleteSlot` をデスクトップと同時に露出するか。
5. OTA をストア版に含めるか（[DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md) §5.4）。
6. 対象 OS（iOS 17+ / Android minSdk 26）、日英既定言語、`MobileError` 拡張タイミング、CI モバイルビルドの範囲。
7. 空マクロ新規作成（step-data clear）を core に実装するか（実装時は Tier 3 + 実機 btsnoop 必須）。
