# AGENTS.md — AI エージェント向け引き継ぎガイド

このファイルは、どの AI エージェントや開発者が途中から参加しても、公式バイナリの再解析なしに即座に実装・検証へ進めるためのプロジェクト引き継ぎドキュメントです。

---

## 1. プロジェクト概要

- **目的**: **8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）を、Nintendo Switch と接続したまま **Linux / macOS / Windows / Android / iOS** から BLE GATT（および USB/2.4G HID）でフル設定・マクロ編集・ファームウェアアップデートできるクロスプラットフォーム OSS ツール群を構築する。
- **現在のフェーズ**: **Phase 1 完了（`eightbitdo-protocol` 実装・テスト済み）。Phase 2 実装着手待ち**。
- **ライセンス／公開方針**: パブリックリポジトリ。公式バイナリ（`.apk`, `.exe`, `.dll`, `.so` 等）や公式ファームウェアファイル自体は `.gitignore` で除外しており、**絶対にコミットしないこと**。

---

## 2. 必ず先に読むべきドキュメント

実装を始める前に、以下の 3 つのドキュメントを必ず参照してください（すべて公式 Android APK v2.01 の `classes.dex`・`libadvancelib.so` および Windows v1.38 の `8BitDoAdvance.dll` 等の逆アセンブル／デコンパイルから直接検証済みです）。

1. **[docs/PROTOCOL_SPEC.md](docs/PROTOCOL_SPEC.md)**
   - VID/PID 一覧（`0x2DC8`, `0x600F`, `0x6011`, `0x6012`, `0x6013`, USB `0x310B`/`0x3208`）
   - BLE GATT UUID（App Mode: `"80ND"` / `0000FF10` / `0000FF11`, Bootloader: `"8bt"` / `0000FF20` / `0000FF21`）
   - 16 バイトカスタムヘッダ（`cumstom_header_t`）、BLE vs USB HID フレーミング、MTU とチャンク分割
   - 2 種類の CRC-16 アルゴリズム（構造体用 `0x8408`・パケット/OTA 用 `0xA001`）
   - App Mode 全コマンド、リアルタイム入力ストリーム（`0x0007` / `0x0003`）、BLE OTA Bootloader シーケンス、8BitDo ファームウェア API
2. **[docs/STRUCT_BT2_2768B.md](docs/STRUCT_BT2_2768B.md)**
   - 2768 バイト（`0xAD0`）`record_ultimatebt2` 設定構造体の全オフセット・型・デフォルト値・部分書き込みパラメータ
   - 32-bit `KeyMap` ビットマスク全表と 22 キーインデックス
   - 10 バイト `MacroDatas` ステップ構造（1ms 単位、8-bit スティック・トリガー）、Flash セクタ配置（`macro_idx << 12`）、Erase → Write → Header → Save のコミット手順
3. **[docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)**
   - 全体アーキテクチャ、ワークスペース構成、Phase 1〜5 の実装ロードマップ、安全原則

---

## 3. 確定済みの技術スタック・設計方針

以下の意思決定はユーザーと合意済みのため、理由なく変更しないでください。

- **共通コア (`eightbitdo-protocol`, `eightbitdo-core`)**: **Rust**
  - `eightbitdo-protocol` は I/O 非依存の純粋なバイナリプロトコル／構造体クレートとして分離し、充実した単体テストを書く。
  - `eightbitdo-core` は `Transport` トレイト経由で BLE / USB HID / モックを差し替え可能にする。
  - **クレート名の注意**: Cargo のパッケージ名は数字で開始できないため、`8bitdo-*` ではなく **`eightbitdo-*`** を正式名とする（ディレクトリ名も同様）。
- **デスクトップ (Linux / macOS / Windows)**:
  - **Pure Rust** で実装する（Electron / Tauri / Node.js / WebView は使わない）。
  - BLE: `btleplug`、USB/2.4G HID: `hidapi`、CLI: `clap`、GUI: **`egui` (`eframe`)**（または `iced`）。
- **モバイル (iOS / Android)**:
  - 各 OS の**ネイティブ UI**（**iOS: SwiftUI**, **Android: Jetpack Compose**）から Rust コアを **`UniFFI` (`eightbitdo-uniffi`)** 経由で呼び出す。
  - モバイルの BLE / USB OTG トランスポートは OS ネイティブ API（CoreBluetooth / Android BluetoothGatt & UsbManager）で実装し、UniFFI の Callback Interface で `eightbitdo-core` に注入する。
- **機能要件**:
  - マクロエディタは **リアルタイム入力録音 + GUI ステップ編集 + JSON/TOML インポート・エクスポート** に対応し、1ms 解像度とフル 8-bit アナログスティック／トリガー値を開放する。
  - コントローラ設定は 2768 バイト構造体の全機能（プロファイル、リマップ、スティック/トリガー、振動、6軸ジャイロ、RGB、マクロ）を公式同等にサポートする。
  - ファームウェアアップデートは **PC だけでなく Android / iOS でも BLE OTA（および Android USB OTG）で実行可能**にする。

---

## 4. 実装時の厳守事項

1. **プロトコル仕様の厳守**:
   - BLE 受信時の `0x600F` における先頭バイト有無、USB HID 送信 (`[0x81, 0x04]`) と受信 (`[0x02, 0x04]`) の違い、Save サブコマンドの差異（BLE `0x0067` vs USB `0x0123`）、2 種類の CRC-16 の使い分けを混同しないこと。
   - 設定の読み出しは **`cmd=0x0002` / `subcmd=0x0000`**、書き込みは **`cmd=0x0001` / `subcmd=0x0000`**（x86_64 `libadvancelib.so` で再確認済み。旧仕様書の `subcmd=0x0101/0x0102` は誤記）。
2. **構造体の未知・予約バイト保全**:
   - 2768 バイト構造体を編集する際は必ず元のバイトバッファを保持した上で変更箇所だけ上書きし、`ANSI_CRC_16_sh` を再計算してから書き込むこと。
3. **マクロ Flash 書き込み順序**:
   - 必ず `Erase (0x0104)` → `Write (0x0103, 64B チャンク, 32B アライン長)` → `Macro ヘッダ更新 (0x0001)` → `Save (0x0006)` の順序を守ること。
4. **OTA（ファームウェア更新）の安全性**:
   - 実機テスト前に必ずモックトランスポートで全パケット列・CRC・チャンク境界・リトライ動作をユニットテストで検証すること。
5. **クリーンなコミット管理**:
   - プロプライエタリな公式バイナリ（ローカル作業環境に置かれている `.apk` や `8BitDo_Ultimate_Software_V2_Windows_V1.38/`）を絶対に `git add` しないこと。

---

## 5. 現在の状態と次に着手すべき作業

**Phase 1 は完了済み**です（`crates/eightbitdo-protocol`）。

- 実装済み: `crc.rs` / `header.rs` / `commands.rs` / `config_bt2.rs` / `keymap.rs` / `macro_data.rs` / `input_report.rs`
- 検証コマンド:
  - `cargo test -p eightbitdo-protocol` → 単体テスト 74 件 + doc test 4 件
  - `cargo clippy -p eightbitdo-protocol --all-targets` → 警告なし
  - `cargo check -p eightbitdo-protocol --no-default-features` → `no_std` + `alloc` でビルド可
  - `cargo test -p eightbitdo-protocol --features serde` → JSON/TOML 用 `MacroFile` を含めてビルド可

次は **[docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)** の **Phase 2** に進んでください。

1. `crates/eightbitdo-core` に `Transport` トレイト（BLE / USB HID / Mock）とセッション管理を実装する。
2. `crates/eightbitdo-cli` に `scan` / `info` / `config dump` / `monitor` を実装する。
3. 実機では **読み取り専用コマンドから**検証し、最初の書き込み前に必ず 2768B 設定とマクロ 4×3 スロットをバックアップする。
4. 書き込み系の前に、モックトランスポートで `Erase 0x0104 → Write 0x0103 → Header 0x0001 → Save 0x0006` のパケット列と CRC を固定する。
