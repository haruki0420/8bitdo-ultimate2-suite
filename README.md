# Padbench

[![CI](https://github.com/haruki0420/8bitdo-ultimate2-suite/actions/workflows/ci.yml/badge.svg)](https://github.com/haruki0420/8bitdo-ultimate2-suite/actions/workflows/ci.yml)

**Padbench**（識別子: `io.github.haruki0420.padbench`、リポジトリ名: `8bitdo-ultimate2-suite`）は、**8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）向けの設定・マクロ編集・ファームウェア更新を行うオープンソースツール群です。

Nintendo Switch へ Bluetooth Classic または 2.4G アダプターで接続したまま、BLE GATT（アドバタイズ名 `"80ND"`）経由で設定やマクロを扱う構成を目指しています。**現時点の実機確認は Linux・BLE（MTU 200）のみ**で、OTA は core 実装のみ・未配線です。

> 非公式の独立したオープンソースプロジェクトであり、8BitDo（Shenzhen Bestodo Tech Co., Ltd.）とは提携・承認・後援関係にありません。公式バイナリ・ファームウェア・ロゴは含みません。商標の扱いは [TRADEMARKS.md](TRADEMARKS.md) を参照してください。

## 実装状況（main `ed61961` / 2026-11-23）

詳細は [AGENTS.md](AGENTS.md) §6 と [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) §5 を参照してください。

| 領域 | 状態 | 実機確認・備考 |
| --- | --- | --- |
| `eightbitdo-protocol`（Phase 1） | 完了 | 単体 100 件 + doc test 4 件 |
| `eightbitdo-core` / CLI 読取・REPL（Phase 2a） | 完了 | Linux BLE 実機確認済み（version / 2768B dump / backup / monitor / keepalive） |
| BLE マクロ読み書き（Phase 2b） | 完了 | slot S1 の M1/M2 で確認（M3/M4・S2/S3 は外挿、Switch 側の発動は未確認） |
| BLE 設定書き込み・リマップ（Phase 2d） | 完了 | full write / remap のみ確認（`config set` 各項目・`--force-raw`・MTU 200 以外は未確認） |
| プロファイル切替 `0x0015` | 完了 | 実機で ACK（`cmd=0x0004/subcmd=0x0015` 16B を 1 回観測）と `cur_slot` 6→1 を確認（永続性・実効・再現性は未確認） |
| BLE OTA（Phase 2c） | core のみ | Mock 52〜61 件、`ota-bench` API 追加済み。単一セクション `.dat` のみ、実機 btsnoop 未取得・未配線 |
| デスクトップ GUI（Tauri v2、正式） | P0 / P1 完了 | [`apps/desktop`](apps/desktop/README.md)。シェルと静的 2 画面まで。BLE 接続・書込・OTA は P2 以降で未着手 |
| デスクトップ GUI（egui 版、凍結） | 比較・撤退用 | [`crates/eightbitdo-gui`](crates/eightbitdo-gui/README.md)。読取 + 書込 UI + ライセンス画面まで。書込は実機未検証 |
| USB HID / USB OTA（Phase 4） | 保留 | USB Boot TX フレーミング未文書化 |
| モバイル（Phase 5、UniFFI + iOS / Android） | ビルド成功まで | Android JNI 同梱 APK / iOS unsigned IPA 生成済み（[docs/MOBILE_BUILD_EVIDENCE.md](docs/MOBILE_BUILD_EVIDENCE.md)）。実機未検証 |

**既知バグ（未修正）**: BLE 切断後も `session` の `status` が `connected: true` と keepalive カウンタを表示し続けます（送信時には `Not connected` が返る）。

**主な未確認範囲**: S2/S3・M3/M4、MTU 200 以外、`config set` 各項目、`--force-raw`、GUI 書込、REPL 書込、OTA 実機 wire、macOS / Windows / Android / iOS 実機。

## ビルドと実行

前提: Rust **1.85+**（workspace MSRV。Windows ビルドは `btleplug` 要件により **1.88+**）。Linux では `libdbus-1-dev`、`pkg-config`、BlueZ が必要です（クロスプラットフォーム確認状況は [docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md)）。

```bash
cargo build --release --workspace
./target/release/eightbitdo scan
./target/release/eightbitdo info
./target/release/eightbitdo config dump --raw config.bin --json config.json
./target/release/eightbitdo-gui   # 凍結中の egui 版。Tauri 版は apps/desktop/README.md を参照
```

CI と同じ検証コマンド:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo +1.85.0 check --workspace --all-targets --all-features --locked
```

> 書き込み前には必ず 2768B 設定とマクロ 3 slot × 4 をバックアップし、ファイル名と SHA-256 を記録してください。書込系コマンドは pre-write backup と read-back 検証を必須にしています。

## CLI / GUI / モバイル / OTA の概要

- **CLI（BLE のみ）**: `scan [--check]` / `info` / `backup` / `monitor` / `keepalive` / `config {dump,write,set,remap,map show,profile}` / `macro {backup,write,restore}` / `session` REPL。
  - BLE を使う全コマンドは per-user 単一所有者 flock（`~/.cache/8bitdo/8bitdo-ble.lock`、`EIGHTBITDO_BLE_LOCK` で上書き可）を取得します。`session` は keepalive の I/O エラー連続 3 回または `Disconnected` で終了コード 3 で終了し、自動再接続しません（詳細は [docs/SESSION_OPERATIONS.md](docs/SESSION_OPERATIONS.md)）。
  - `config write` / `set` / `remap` は予約バイト保持、CRC 再計算、pre-write backup、TOCTOU 再読込、read-back 検証を行います。実機観測の opaque flag（`0` / `1` / `0x20200911` / `0xFFFFFFFF`、gyro は `0x20190000` も）と完全消去 slot のみ受理し、それ以外は `--force-raw` が必要です（[docs/DEVICE_CONFIG_EVIDENCE.md](docs/DEVICE_CONFIG_EVIDENCE.md)）。
  - `config profile <0|1|2>`（REPL: `profile switch`）は `WriteSLOT`（`0x0015`）を Save なしで送信します。
- **GUI**: 正式は Tauri v2 版（[`apps/desktop/README.md`](apps/desktop/README.md)、計画は [docs/TAURI_PLAN.md](docs/TAURI_PLAN.md)、デザインは [docs/DESIGN_TAURI.md](docs/DESIGN_TAURI.md)）。egui 版（[`crates/eightbitdo-gui/README.md`](crates/eightbitdo-gui/README.md)）は凍結しています。
- **モバイル**: `mobile/` に iOS（SwiftUI）と Android（Jetpack Compose）の雛形があり、`eightbitdo-uniffi` で Rust コアを共有します（[mobile/README.md](mobile/README.md)）。書込・OTA は未公開です。
- **OTA**: `crates/eightbitdo-core/src/ota.rs` に状態機械と `ota-bench` API を実装済みですが、単一セクション `.dat` のみ対応で、Bootloader ACK の `offset` / `total_len` 照合も未追加です。証跡と採取計画は [docs/OTA_BLE_EVIDENCE.md](docs/OTA_BLE_EVIDENCE.md) と [docs/OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md) を参照してください。

## ドキュメント一覧

| カテゴリ | ドキュメント |
| --- | --- |
| プロトコル・構造体 | [PROTOCOL_SPEC.md](docs/PROTOCOL_SPEC.md) / [STRUCT_BT2_2768B.md](docs/STRUCT_BT2_2768B.md) / [DEVICE_CONFIG_EVIDENCE.md](docs/DEVICE_CONFIG_EVIDENCE.md) |
| OTA | [OTA_BLE_EVIDENCE.md](docs/OTA_BLE_EVIDENCE.md) / [OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md) |
| 計画・設計 | [IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) / [ROADMAP.md](docs/ROADMAP.md) / [FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md) / [DEVICE_MODEL_ABSTRACTION.md](docs/DEVICE_MODEL_ABSTRACTION.md) |
| GUI・モバイル | [TAURI_PLAN.md](docs/TAURI_PLAN.md) / [DESIGN_TAURI.md](docs/DESIGN_TAURI.md) / [ANTI_SLOP.md](docs/ANTI_SLOP.md) / [MOBILE_NATIVE_PLAN.md](docs/MOBILE_NATIVE_PLAN.md) / [MOBILE_BUILD_EVIDENCE.md](docs/MOBILE_BUILD_EVIDENCE.md) / [mobile/README.md](mobile/README.md) |
| 運用・品質・配布 | [SESSION_OPERATIONS.md](docs/SESSION_OPERATIONS.md) / [CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md) / [REVIEW_TIERS.md](docs/REVIEW_TIERS.md) / [BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md) / [DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md) |
| 引き継ぎ・規約 | [AGENTS.md](AGENTS.md) / [CONTRIBUTING.md](CONTRIBUTING.md) / [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) / [TRADEMARKS.md](TRADEMARKS.md) |

## ライセンスと免責

- **コード・ドキュメント**: 特記がない限り [MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE)（`MIT OR Apache-2.0`）。
- **第三者資産**: 同梱フォント（Noto Sans JP サブセットは OFL 1.1、egui 既定フォントは OFL 1.1 / UFL 1.0 / MIT）、Gradle wrapper（Apache-2.0）、依存クレート一覧は [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) を参照してください。配布・ストア申請の方針は [docs/DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md) にまとめています。
- **免責**: 本ソフトウェアは無保証（"AS IS"）で提供されます。設定・マクロ・ファームウェアの書き込みは文鎮化・保証喪失・データ損失のリスクを伴います。自己責任で使用してください。
