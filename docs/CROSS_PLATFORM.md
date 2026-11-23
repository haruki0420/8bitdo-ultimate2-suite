# クロスプラットフォーム・ビルド確認

- 確認日: 2026-11-23（base `main` `4004757`、Arch Linux x86_64、rustc/cargo 1.100.0、MSRV 1.85.0、実機なし）
- 実機動作確認済みは **Linux（BLE・MTU 200）のみ**。macOS / Windows / Android / iOS の実機確認は未実施。

## 1. 確認結果サマリ

| 対象 | コマンド・確認内容 | 結果 | 備考 |
| --- | --- | --- | --- |
| Linux x86_64 | `cargo +1.85.0 check --workspace --all-targets --all-features --locked` | **成功** | 宣言 MSRV 1.85.0 は Linux で成立 |
| Windows (`x86_64-pc-windows-gnu`) | `cargo check --workspace --all-targets [--all-features] --locked`（1.100.0） | **成功** | GUI（`eframe`）/ CLI / core（`winrtble`）/ uniffi 通過。lib test に warning 4 件 |
| Windows (`x86_64-pc-windows-gnu`) | `cargo clippy --workspace --all-features --locked -- -D warnings` | **成功** | `--all-targets` 付きは warning 4 件で失敗（exit 101） |
| Windows (`x86_64-pc-windows-gnu`) | `cargo build` / `cargo test`（リンク・実行） | **未実施** | MinGW リンカ（`x86_64-w64-mingw32-gcc`）不在のため不可 |
| Windows (`x86_64-pc-windows-gnu`) | MSRV 1.85.0 での `cargo +1.85.0 check` | **失敗** | `btleplug 0.13.5` の `winrtble` が let チェーン（Rust 1.88 安定化）を使用（9 箇所で `E0658`） |
| macOS (`*-apple-darwin`) | コンパイル確認 | **不能** | target 未導入・macOS SDK 不在（`macos-14` ランナーまたは実機が必要） |
| iOS (`aarch64-apple-ios`) | `cargo check -p eightbitdo-uniffi --lib --locked` | **成功** | `btleplug` を含まないため Linux で check 可（IPA 検証は [MOBILE_BUILD_EVIDENCE.md](MOBILE_BUILD_EVIDENCE.md)） |
| Android | JNI 同梱 APK | **別記録** | 本確認時は NDK 不在（後のビルド証跡は [MOBILE_BUILD_EVIDENCE.md](MOBILE_BUILD_EVIDENCE.md)） |

## 2. 切り分け結果

### 2.1 Windows `--all-targets` の warning 4 件

すべて **Unix 専用テストヘルパー**由来であり、本番コードの移植漏れではない（本番 `BleLock::acquire` は `fs2` + `symlink_metadata` で Windows コンパイル通過。ただし Windows での実排他と hard link 同一性検出は未検証・Unix 限定）。

- `crates/eightbitdo-core/src/ble.rs:98`: `acquire_ble_lock_with_default` 未使用
- `crates/eightbitdo-core/src/ble_lock.rs:142`: `acquire_enforcing` 未使用
- `crates/eightbitdo-core/src/ble_lock.rs:553`: `temp_parent_dir` 未使用 / `mode` 引数未使用

修正案（`eightbitdo-core` 変更のため別 PR・Tier 3）: 該当テストヘルパーへ `#[cfg(unix)]` または `#[cfg_attr(not(unix), allow(dead_code, unused_variables))]` を付与する。

### 2.2 Windows の実効 MSRV（1.88 以上）

`btleplug 0.13.5`（edition 2024）の `src/advertisement.rs` / `src/winrtble/` が Rust 1.88 安定化の let チェーンを使うため、1.85.0 では失敗する（1.88 ちょうどは未検証、1.100.0 で成功）。Linux は BlueZ バックエンドのため 1.85 で通る。対策候補は ①workspace MSRV を 1.88 へ引き上げ ②1.85 維持のまま「Windows ビルドは 1.88+ 必須」と明記 ③`btleplug` バージョン固定。

## 3. 未確認事項

- Windows: リンク・`.exe` 生成・`cargo test`・WinRT BLE 実挙動・ロック / `rename` 挙動・Rust 1.88/1.89 中間版。
- macOS: コンパイル・リンク・CoreBluetooth 実挙動。
- USB HID（`hidapi`）: 依存未追加のため Phase 2e 実装時に再検証。

## 4. CI 追加提案と再現コマンド

現行 [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) は Linux のみ。Ubuntu ランナーで低コストに追加できる `cross-check` ジョブ（`targets: x86_64-pc-windows-gnu, aarch64-apple-ios`）を最優先とする。

```bash
rustup target add x86_64-pc-windows-gnu aarch64-apple-ios
cargo check --workspace --all-targets --all-features --locked --target x86_64-pc-windows-gnu
cargo clippy --workspace --all-features --locked --target x86_64-pc-windows-gnu -- -D warnings
cargo check -p eightbitdo-uniffi --lib --locked --target aarch64-apple-ios
```

優先順: ①Ubuntu 上の `cross-check` ジョブ追加 ②warning 4 件修正（Tier 3）後に Windows clippy へ `--all-targets` 追加 ③MSRV 方針決定 ④`windows-latest` / `macos-14` 実ビルドジョブ検討。
