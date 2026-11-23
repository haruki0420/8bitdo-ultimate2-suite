# 実装計画書（Padbench — 8BitDo Ultimate 2 対応）

- 対象: **8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）。Nintendo Switch へ接続したまま Linux / macOS / Windows / Android / iOS から BLE GATT（将来 USB/2.4G HID）で設定・マクロ・OTA を扱う OSS ツール群を構築する。
- 基準: **2026-11-23（main `ed61961`）**。

## 1. アーキテクチャとワークスペース構成

プロトコル処理・2768B 構造体・マクロ・CRC・OTA 状態機械を Rust コアに集約し、デスクトップは **Tauri v2（正式）** / CLI、モバイルは Tauri v2 モバイル（計画）または **UniFFI + ネイティブ UI（代替維持）** で共有する（Cargo パッケージ名は数字開始不可のため `eightbitdo-*` を維持）。

| レイヤー | 技術・配置 | 役割・状態 |
| --- | --- | --- |
| プロトコル | `crates/eightbitdo-protocol`（Rust、`no_std` 互換） | CRC 2 種、16B ヘッダ、コマンド、2768B `Bt2Config`、KeyMap、10B マクロ、入力レポート（単体 100 + doc 4） |
| コア | `crates/eightbitdo-core`（Rust + `btleplug`） | `Transport`、BLE 単一所有ロック、`Session`、`config_ops`、`macro_ops`、`ota`、`MockTransport`（`hid.rs` / `firmware_api.rs` 未作成） |
| CLI | `crates/eightbitdo-cli`（`clap` + `serde`） | 読取・設定/マクロ書込・プロファイル切替・`session` REPL |
| デスクトップ GUI（正式） | `apps/desktop`（Tauri v2 + React / TS / Vite / Tailwind、pnpm） | 独立 `src-tauri` クレート。P0 / P1 完了、P2 以降未着手（[TAURI_PLAN.md](TAURI_PLAN.md)） |
| デスクトップ GUI（凍結） | `crates/eightbitdo-gui`（`eframe` / egui） | 比較・撤退用。3a 読取 + 3b/3d 書込 UI + ライセンス画面まで。新機能は Tauri 版のみ |
| モバイル | `crates/eightbitdo-uniffi` + `mobile/{ios,android}` | SwiftUI + CoreBluetooth / Compose + BluetoothGatt。読取 API + APK / unsigned IPA ビルド成功まで |

## 2. 機能スコープ

- **マクロ**: リアルタイム入力記録（`0x0007` + `0x0003`）、1ms 単位・8-bit スティック/トリガー編集、最大 200 ステップ × 4 マクロ × 3 スロット、JSON / TOML 入出力。
- **2768B 全設定**: 3 スロット名（UTF-16BE 32B）・`cur_slot`・`0x0015` 切替、22 ボタンリマップ、スティック/トリガー範囲、特殊機能、左右振動、6 軸ジャイロ、RGB 3 テーマ（24 LED）。
- **OTA**: 公式 API / ローカル `.dat` 検証、`0x000C` → `"8bt"` 再接続 → 消去・書込・バージョン書込・`JumpApp`、バッテリー警告と `"8bt"` 復旧モード。

## 3. フェーズ別進捗（main `ed61961`）

| Phase | 内容 | 状態 | 実機確認・要点（Tier） |
| --- | --- | --- | --- |
| **1** | `eightbitdo-protocol` | 完了 | 単体 100 + doc 4。実機観測の ReadVersion 4B / ReadConfig 142B 切り詰め等を補正済み |
| **2a** | 読取専用 core / CLI / `session` | 完了 | 実機確認済み（version `0xC8`、2768B dump、20ms keepalive 128.9s、per-user flock、degraded exit 3）。REPL の macro/config 書込は実機未検証（Tier 3） |
| **2b** | BLE マクロ読み書き | 完了 | 実機確認済み（S1 の M1/M2 で書込・read-back・復元。Write `total_len` は絶対終端必須、空マクロ拒否）。**Switch 側発動・M3/M4・S2/S3 は未確認**（Tier 3） |
| **2c** | BLE OTA（core） | **core のみ** | Mock 52〜61 件、`ota-bench` 段階停止 API（#20）、`BleBootTransport` 結合、type 1 暫定ウィンドウ `0x0101_0000..0x0102_0000`、`OTA_MIN_MTU=84`、`allow_unverified_wire=false`。**単一セクション `.dat` のみ、ACK `offset`/`total_len` 未照合、実機 btsnoop 未取得・未配線**（Tier 3） |
| **2d** | BLE 設定書き込み・リマップ | 完了 | 実機確認済み（full 2768B 38 write + 1 Save = 39 packets、74B chunk、`remap A B`→`A A` 往復）。PR #17 で実機 opaque flag と完全消去 slot に対応。**`config set` 各項目・`--force-raw`・MTU 200 以外は未確認**（Tier 3） |
| — | プロファイル切替 `0x0015` | 完了（#26） | core / CLI / REPL 配線（Save なし）。実機で 16B 封筒 ACK（1 回）と `cur_slot` 6→1 を確認（永続性・実効・再現性は未確認。[DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md)） |
| **2e / 4** | USB / 2.4G・USB OTA | **保留** | USB App Mode 実機証跡と USB Boot TX フレーミング未文書化（Tier 3） |
| **3a–3d** | egui 版 GUI（**凍結**） | 3a/3b/3d 完了、3c 未着手 | 3a 読取 + ライセンス画面（#24）+ 3b/3d 書込 UI（#16、**実機未検証・Tier 3**） |
| **Tauri P0–P1** | Tauri v2 GUI（正式） | **完了（#32）** | 足場・独立 `src-tauri`・CSP・CI `desktop.yml`・Hyperstudio トークン・静的 2 画面。**P2 以降（BLE 接続・書込・OTA）未着手** |
| **5** | UniFFI + iOS / Android | ビルド成功まで（#21） | Android JNI 同梱 APK（14.0 MiB）/ iOS unsigned IPA（1.48 MiB）生成済み（[MOBILE_BUILD_EVIDENCE.md](MOBILE_BUILD_EVIDENCE.md)）。実機・署名・書込公開は未着手（Tier 3） |

> 2026-11-23 の再確認以降、実機 config の SHA はプロファイル名設定により初期ダンプ（`1dca47fa…`）から変化したため、検証は固定値ではなく「操作後の read-back 一致と復帰後の一致」で行う。

## 4. 残課題（優先順）

| # | 課題 | 影響 | Tier |
| --- | --- | --- | --- |
| 1 | BLE OTA の実機 btsnoop 採取と wire 確定（[OTA_VERIFICATION_PLAN.md](OTA_VERIFICATION_PLAN.md)） | 実機 OTA 不可 | 3 |
| 2 | OTA ACK の `offset` / `total_len` 照合追加（CLI 配線前の必須条件） | 遅延 ACK 誤帰属リスク | 3 |
| 3 | マルチセクション `.dat` 対応 | 公式 FW 更新の blocker | 3 |
| 4 | OTA の CLI / GUI / UniFFI 配線、FW 取得 API、バッテリー・ダウングレード判定 | OTA 未提供 | 3 |
| 5 | Tauri v2 GUI の P2〜P7 実装と GUI 書込の実機確認 | 正式 GUI 未完成 | 2–3 |
| 6 | モバイル実機での BLE・JNI ロード確認、署名・ストア配布（macOS 必須） | モバイル未実証 | 3 |
| 7 | macOS / Windows の実機 BLE 確認（[CROSS_PLATFORM.md](CROSS_PLATFORM.md)） | Linux 以外未実証 | 2–3 |
| 8 | M3/M4・S2/S3、`config set` 各項目、MTU 200 以外、REPL 書込、Switch 側実効の確認 | 外挿・未検証が残る | 3 |
| 9 | **既知バグ**: BLE 切断後も `session` の `status` が `connected: true` と keepalive カウンタを保持（送信時は `Not connected`） | リンク断を `status` で検知不可 | 3 |
| 10 | `0x0015` の永続性・実効・ACK 再現性確認 | 切替保証が不完全 | 3 |
| 11 | USB / 2.4G transport・USB OTA（TX 証跡取得後） | Phase 2e / 4 保留 | 3 |

## 5. 安全原則

1. 実機書き込み前に必ず 2768B 設定原本と全 12 マクロをバックアップし SHA-256 を記録する。
2. 予約・未編集領域は `read → modify → CRC 再計算 → write` でバイト単位保全する。
3. OTA はモック検証と実機 btsnoop による wire 確定を経てから実機適用する。
4. 公式 `.apk` / `.exe` / `.dll` / `.so` やファームウェアバイナリは絶対にコミットしない。
