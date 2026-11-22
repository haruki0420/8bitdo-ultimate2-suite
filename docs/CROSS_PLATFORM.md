# クロスプラットフォーム・ビルド確認（Linux / Windows / macOS / モバイル）

本ドキュメントは、**ビルド・コンパイルの確認結果**をプラットフォーム別に記録するものです。
**実機での動作確認は Linux（BLE・MTU 200）のみ**で、macOS / Windows / Android / iOS の実機検証は未実施です（実機検証の現状は [README.md](../README.md) の「実装状況」と [AGENTS.md](../AGENTS.md) §6 を参照）。

- 確認日: **2026-11-23**
- 対象: ブランチ `chore/cross-platform`（base `main` `4004757`）
- ホスト: Arch Linux（x86_64, kernel 7.3.5-arch1-1, glibc 2.44）。**実機（コントローラ）なし**
- 変更内容: **docs のみ**（コード・ビルド設定は変更しない）

---

## 1. 結論（サマリ）

| 対象 | 確認したこと | 結果 | 備考 |
| --- | --- | --- | --- |
| Linux x86_64（ホスト） | `cargo +1.85.0 check --workspace --all-targets --all-features` | **成功**（exit 0） | 宣言 MSRV 1.85.0 で成立。CI の `msrv` job と同じ形 |
| Windows x86_64-pc-windows-gnu | `cargo check --workspace --all-targets [--all-features]`（stable 1.100.0） | **成功**（exit 0、warning 4 件） | GUI（`eframe`）/ CLI / core すべてコンパイル通過。§3.2 |
| Windows x86_64-pc-windows-gnu | `cargo clippy ... --all-targets ... -D warnings` | **失敗**（exit 101） | 原因は Unix 専用テストヘルパーの warning 4 件。§3.3・§4 |
| Windows x86_64-pc-windows-gnu | `cargo clippy ... -D warnings`（`--all-targets` なし） | **成功**（exit 0） | CI 追加時の推奨形。§6.1 |
| Windows x86_64-pc-windows-gnu | `cargo build` / `cargo test` / 実行（リンク） | **不可・未実施** | MinGW リンカ不在。§3.4 |
| Windows x86_64-pc-windows-gnu | MSRV **1.85.0** での check | **失敗**（exit 101） | `btleplug` の Windows バックエンドが let チェーン（**Rust 1.88** で安定化）を使用。§3.5 |
| macOS（`x86_64-apple-darwin` / `aarch64-apple-darwin`） | コンパイル確認 | **不能** | target 未導入 + macOS SDK 不在。§3.7 |
| iOS（`aarch64-apple-ios`） | `cargo check -p eightbitdo-uniffi --lib` | **成功**（exit 0） | Rust コア表面のみ。IPA 再生成・実機は未 |
| Android | Rust JNI（`libeightbitdo_uniffi.so`）同梱 | **未実施** | NDK + cargo-ndk 不在。Kotlin-only APK の状況は [mobile/README.md](../mobile/README.md) |
| 全プラットフォーム | 実機 BLE / GUI 起動 / 書き込み | **未実施** | 実機（コントローラ）なし。既存の実機証跡は Linux のみ |

**要約**: Windows は **コンパイル（`cargo check`）レベルで通る**。ただし (a) **リンク・実行・テストは未検証**、(b) **Windows の実効 MSRV は 1.88 以上**（宣言 1.85 では通らない）、(c) `-D warnings` の clippy はテストターゲットの warning 4 件で落ちる。macOS はこのホストでは**コンパイル確認すらできない**。

---

## 2. 確認環境

| 項目 | 値 |
| --- | --- |
| OS | Arch Linux（rolling, x86_64） |
| カーネル | 7.3.5-arch1-1 |
| glibc | 2.44 |
| rustup | 1.29.3 |
| cargo / rustc（stable） | 1.100.0（2026-11-05 / 2026-11-09） |
| MSRV ツールチェーン | 1.85.0 |
| 導入済み target（stable） | `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`, `aarch64-apple-ios` |
| 導入済み target（1.85.0） | `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`（今回 `rustup target add` で追加） |
| MinGW クロスツールチェーン | **なし**（`x86_64-w64-mingw32-gcc` / `-ld` / `-ar` 不在） |
| macOS SDK / osxcross | **なし** |
| Android NDK | **なし** |

`x86_64-pc-windows-gnu` は stable に導入済みだったため `rustup target add` は不要でした（MSRV 1.85.0 側にのみ追加）。

---

## 3. 実行コマンドと結果

### 3.1 Linux ホスト（ベースライン）

```bash
cargo +1.85.0 check --workspace --all-targets --all-features --locked
```

```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 02s
```

exit 0。ワークスペースの宣言 MSRV（`rust-version = "1.85"`）は **Linux では成立**します（CI の `msrv` job と同条件）。

### 3.2 Windows GNU クロスチェック（成功）

```bash
rustup target list --installed
# aarch64-apple-ios
# x86_64-pc-windows-gnu
# x86_64-unknown-linux-gnu

cargo check --workspace --all-targets --locked --target x86_64-pc-windows-gnu
# Finished `dev` profile ... in 28.35s  (exit 0)

cargo check --workspace --all-targets --all-features --locked --target x86_64-pc-windows-gnu
# Finished `dev` profile ... in 11.70s  (exit 0)
```

通過したクレート:

- `eightbitdo-gui`: **`eframe` 0.29.1 / `egui-winit` / `winit` / `accesskit_windows` / `glow` 系が Windows target でコンパイル通過**（リンクは未検証）
- `eightbitdo-cli`: `clap` / `tokio` / `tokio-util` / `serde` / `toml` 通過
- `eightbitdo-core`: `btleplug` 0.13.5 の **WinRT バックエンド（`winrtble`）**、`fs2` 通過
- `eightbitdo-uniffi`: `uniffi` 0.31.2 通過
- `nix` / `rustix` は `[target.'cfg(unix)'.dependencies]` に置かれているため **Windows では解決されない**（`ble` feature が `dep:nix` / `dep:rustix` を有効化しても、対象外 target では依存ユニットが生成されない）
- `hidapi` は **まだ依存に存在しない**（`Cargo.lock` に `hidapi` / `hidapi-sys` なし）。USB HID（Phase 2e）実装時に改めて Windows 検証が必要

`--all-targets` 付きでは、次の **warning 4 件**（`eightbitdo-core` の lib test）が出ます:

```
warning: unused variable: `mode`
   --> crates/eightbitdo-core/src/ble_lock.rs:553:37
warning: function `acquire_ble_lock_with_default` is never used
  --> crates/eightbitdo-core/src/ble.rs:98:4
warning: associated function `acquire_enforcing` is never used
   --> crates/eightbitdo-core/src/ble_lock.rs:142:19
warning: function `temp_parent_dir` is never used
   --> crates/eightbitdo-core/src/ble_lock.rs:553:8
warning: `eightbitdo-core` (lib test) generated 4 warnings
```

`--all-targets` を付けなければ（`cargo check --workspace --all-features --target x86_64-pc-windows-gnu`）**warning は 0 件**です（exit 0）。

### 3.3 clippy（Windows target、`-D warnings`）

```bash
# (a) --all-targets あり: 失敗
cargo clippy --workspace --all-targets --all-features --locked \
  --target x86_64-pc-windows-gnu -- -D warnings
# error: unused variable: `mode`
# error: function `acquire_ble_lock_with_default` is never used
# error: associated function `acquire_enforcing` is never used
# error: function `temp_parent_dir` is never used
# error: could not compile `eightbitdo-core` (lib test) due to 4 previous errors
# exit 101

# (b) --all-targets なし: 成功
cargo clippy --workspace --all-features --locked \
  --target x86_64-pc-windows-gnu -- -D warnings
# Finished `dev` profile ... in 3.32s  (exit 0)
```

### 3.4 リンク可否（Windows バイナリは作れない）

```bash
rustc --target x86_64-pc-windows-gnu /tmp/xplat_link_test.rs -o /tmp/xplat_link_test.exe
# error: linker `x86_64-w64-mingw32-gcc` not found
#   = note: No such file or directory (os error 2)
```

`cargo check` はリンクしないため通りますが、`cargo build` / `cargo run` / `cargo test`（テストバイナリのリンクと実行が必要）は **このホストでは実行できません**。リンクまで見るには MinGW を導入する（Arch: `mingw-w64-gcc`、Debian/Ubuntu: `gcc-mingw-w64-x86-64`）か、Windows ランナーで実行する必要があります。**今回の確認は「コンパイルが通る」までで、`.exe` の生成・起動は未確認**です。

### 3.5 MSRV 1.85.0 での Windows check（失敗）

```bash
rustup target add --toolchain 1.85.0 x86_64-pc-windows-gnu   # 1.85.0 には target 未導入だったため追加
cargo +1.85.0 check --workspace --all-targets --all-features --locked \
  --target x86_64-pc-windows-gnu
# exit 101
# error[E0658]: `let` expressions in this position are unstable   (9 箇所)
# error: could not compile `btleplug` (lib) due to 9 previous errors
```

該当箇所（`btleplug 0.13.5`、Windows 専用 `winrtble` バックエンド）:

```
src/advertisement.rs:34
src/winrtble/ble/watcher.rs:143,144
src/winrtble/peripheral.rs:181,230,231,251,484,501
```

**原因**: `btleplug 0.13.5` は `edition = "2024"` で、Windows バックエンドに let チェーン（`if data_type == X && let Some(v) = ...`）を使用しています。let チェーンは **Rust 1.88.0 で（edition 2024 限定で）安定化**された機能です（[Announcing Rust 1.88.0](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0/)）。Linux では別バックエンド（BlueZ / zbus 系）を通るため 1.85 でも通ります。

**含意**:

- workspace の `rust-version = "1.85"` は **Linux では成立**するが、**Windows ビルドの実効 MSRV は 1.88 以上**（`btleplug` の Windows バックエンド由来。自前コードの失敗ではない）。
- **1.88 ちょうどでの成功は未検証**。今回確認できたのは「1.85 では失敗」「stable 1.100.0 では成功」の 2 点。
- 対策の選択肢（いずれもビルド設定/コード変更なので本 PR の対象外・要判断）:
  1. workspace の MSRV を 1.88 に引き上げ、CI の `msrv` job も 1.88 にする
  2. MSRV は 1.85 のまま、README / 本ドキュメントで「**Windows ビルドは 1.88 以上が必要**」と明記する
  3. `btleplug` の Windows バックエンドが 1.85 互換になる版まで固定する（セキュリティ/機能面のトレードオフあり）

### 3.6 iOS target（コンパイル確認のみ成功）

```bash
cargo check -p eightbitdo-uniffi --lib --locked --target aarch64-apple-ios
# Finished `dev` profile ... in 11.63s  (exit 0)
```

`eightbitdo-uniffi` は `eightbitdo-core` を `default-features = false` で参照するため `btleplug` を含まず、**Linux ホスト上で iOS target の check が通ります**（モバイル Rust コア表面の回帰検出に使えます）。署名なし IPA の生成は `scripts/mobile/build-ios.sh`（xtool + Darwin Swift SDK）の領域で、今回は再実行していません。

### 3.7 macOS（コンパイル確認不能）

- `aarch64-apple-darwin` / `x86_64-apple-darwin` は **未導入**。`aarch64-apple-ios` のみ導入済み（§3.6 用）。
- 仮に `rustup target add x86_64-apple-darwin` で std を追加しても、**macOS SDK とリンカが無いため `cargo build` は不可能**（`cargo check` が通るかは未検証。今回は指示に従い target を追加していない）。
- macOS のビルド確認は **GitHub Actions の `macos-14` ランナー**（または実機の macOS）で行うのが確実です。

---

## 4. 切り分け結果

### 4.1 warning 4 件（Windows、`--all-targets`）

すべて **Unix 専用テストヘルパー**に由来し、**プロダクションコードの Windows 移植漏れではありません**。

| 警告 | 由来 |
| --- | --- |
| `ble.rs:98 acquire_ble_lock_with_default` が未使用 | `#[cfg(test)]` のテスト用 seam。呼び出し元が `ble.rs:903` の `#[cfg(unix)] #[test] production_lock_path_enforces_the_parent_policy` のみ |
| `ble_lock.rs:142 acquire_enforcing` が未使用 | 上記 seam からのみ利用（`#[cfg(test)]`） |
| `ble_lock.rs:553 temp_parent_dir` が未使用 | `#[cfg(unix)]` テストからのみ利用 |
| `ble_lock.rs:553` の `mode` 引数が未使用 | 引数を使う `set_permissions` が `#[cfg(unix)]` ブロック内にあるため、Windows では未使用 |

プロダクションのロック実装（`BleLock::acquire`）は `fs2` の `FileExt::try_lock_exclusive` + `symlink_metadata` 事前検証で **Windows でもコンパイルされます**。ただし **Windows で実際に排他が機能するかは未検証**で、hard link の同一性検出が Unix のみという既知の制約があります（[README.md](../README.md) の設定バックアップ節・[AGENTS.md](../AGENTS.md) §6.1 を参照）。

修正案（**コード変更なので別 PR**。`crates/eightbitdo-core/**` に触れるため [AGENTS.md](../AGENTS.md) §6.3 により **Tier 3**）:

- seam と helper に `#[cfg(unix)]` を付ける（このテストは Unix 専用の親ディレクトリ権限ポリシーを見るもの）、または
- `#[cfg_attr(not(unix), allow(dead_code, unused_variables))]` を付ける

修正が入るまで、Windows target の clippy は `--all-targets` を付けずに回します（§6.1）。`cargo check --all-targets` 自体は通り続けます（warning のみ）。

### 4.2 MSRV 1.85 の Windows 失敗

原因は **依存クレート `btleplug 0.13.5` の Windows バックエンドの let チェーン**のみ（§3.5）。自前コードの変更では解消できません（MSRV 引き上げ or 依存の変更が必要）。

---

## 5. 未確認・このホストでは確認できないこと

| 項目 | 理由 |
| --- | --- |
| Windows: リンク・`.exe` 生成・起動・`cargo test` | MinGW リンカ不在（§3.4） |
| Windows: BLE（WinRT）の実挙動、GUI ウィンドウ表示、パス/ロック挙動、`rename` semantics | 実機・実 OS 不在 |
| Windows: 1.88 / 1.89 などの中間バージョンでの成功 | 未検証（1.85 NG / 1.100 OK のみ確認） |
| macOS: コンパイル・リンク・起動・BLE（CoreBluetooth） | target・SDK 不在（§3.7） |
| iOS: 署名なし IPA の再生成、実機インストール | 今回は未実行（`scripts/mobile/build-ios.sh` の領域） |
| Android: Rust JNI 同梱 APK | NDK + cargo-ndk 不在 |
| 全プラットフォーム: 実機 BLE / 設定・マクロ書き込み / OTA | 実機（コントローラ）なし。既存の実機証跡は Linux のみ |

---

## 6. CI で追加できる check の提案

現状の [.github/workflows/ci.yml](../.github/workflows/ci.yml) は **Ubuntu × Linux target のみ**で、クロスプラットフォームのコンパイル回帰は検出できません。追加候補を費用対効果順に示します。

### 6.1 すぐ追加できる（Ubuntu ランナー、低コスト）

Windows target の **コンパイル回帰検出**だけなら Linux ランナーで足ります（`cargo check` はリンクしないので MinGW 不要）。

```yaml
  cross-check:
    name: cross-check (windows + ios targets)
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@<pin>
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@<pin>
        with:
          toolchain: stable
          targets: x86_64-pc-windows-gnu, aarch64-apple-ios
      - uses: Swatinem/rust-cache@<pin>
      - name: Windows target (GUI / CLI / core / uniffi)
        run: cargo check --workspace --all-targets --all-features --locked --target x86_64-pc-windows-gnu
      - name: Windows target clippy
        run: cargo clippy --workspace --all-features --locked --target x86_64-pc-windows-gnu -- -D warnings
      - name: iOS target (Rust core surface)
        run: cargo check -p eightbitdo-uniffi --lib --locked --target aarch64-apple-ios
```

注意点:

- このジョブは **Linux ランナーのまま**なので追加コストは小さい（実測: `check` 28 秒 + clippy 3 秒、target 追加のダウンロード込みでも数分）。
- **`cargo check --target ... --all-targets` は現状 warning 4 件を出すが exit 0**。CI の `RUSTFLAGS: -D warnings` をグローバルに付けている場合は落ちるので、このジョブでは付けない（または §4.1 の修正後に対応）。
- **clippy は `--all-targets` を付けない**（§3.3）。§4.1 の修正 PR の後で `--all-targets` を有効化する。
- **MSRV job に Windows target を足さない**（1.85 では必ず失敗する。§3.5）。MSRV の扱い（§3.5 の選択肢 1〜3）を決めてから。
- `rust-cache` は target ごとにキーが分かれるため、キャッシュヒット率は Linux 単体より下がる（許容範囲）。

### 6.2 実ビルド・実テストまで検証する場合（追加ランナー）

| ランナー | 内容 | 得られる保証 | コスト |
| --- | --- | --- | --- |
| `windows-latest` | `cargo build --workspace --locked` → `cargo test --workspace --locked` → GUI の起動スモーク（不可なら `cargo check` のみ） | **リンク + Windows 上でのユニットテスト実行**。`.exe` の生成 | Windows 分単価（macOS より安い） |
| `macos-14`（Apple Silicon） | `cargo check --workspace --all-targets --all-features --locked` + `cargo test -p eightbitdo-core --lib`（BLE 実機なしの範囲） | **macOS target のコンパイル + CoreBluetooth 以外のロジック** | macOS は分単価が高い |
| `macos-13`（Intel） | 上と同じ | `x86_64-apple-darwin` 側 | 同上 |

- BLE の実挙動は CI にコントローラが無いため**どのランナーでも検証できません**（`cargo test` は mock transport ベース）。実機確認は手元の macOS / Windows で行う必要があります。
- 費用を抑えるなら `on.pull_request.paths` で `crates/**` / `Cargo.toml` / `Cargo.lock` / `.github/workflows/ci.yml` に限定し、`main` push 時のみ全実行する形が現実的です。

### 6.3 優先順位（提案）

1. **§6.1 の `cross-check` ジョブ**（Windows `check` + iOS `check`）— 追加コストが小さく、Windows で壊れる変更を PR 時点で検出できる
2. **§4.1 の warning 4 件修正**（Tier 3）→ Windows clippy を `--all-targets` で有効化
3. **MSRV の方針決定**（§3.5。1.88 へ引き上げ or Windows は 1.88+ と明記）
4. **`windows-latest` / `macos-14` の実ビルド job**（予算と相談。まずは週次 or `main` push のみでも可）

---

## 7. 再現手順（要約）

```bash
# 前提: rustup で target を追加（stable）
rustup target add x86_64-pc-windows-gnu aarch64-apple-ios

# Windows: コンパイル確認（リンクなし・MinGW 不要）
cargo check --workspace --all-targets --all-features --locked --target x86_64-pc-windows-gnu
cargo clippy --workspace --all-features --locked --target x86_64-pc-windows-gnu -- -D warnings

# iOS: Rust コア表面のコンパイル確認
cargo check -p eightbitdo-uniffi --lib --locked --target aarch64-apple-ios

# 参考: MSRV ツールチェーンでは Windows target が落ちる（§3.5）
rustup target add --toolchain 1.85.0 x86_64-pc-windows-gnu
cargo +1.85.0 check --workspace --all-targets --all-features --locked --target x86_64-pc-windows-gnu

# macOS: このホストでは不可。macOS ランナー / 実機で
#   cargo check --workspace --all-targets --all-features --locked
```

---

## 8. 関連ドキュメント

- [README.md](../README.md): ビルドと実行、実装状況、実機検証の現状
- [AGENTS.md](../AGENTS.md) §6: 現在の状態・残課題（クロスプラットフォーム実機確認は §6.2 の項目 5）
- [docs/IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md): Phase 3（デスクトップ GUI）の完了条件「Linux / macOS / Windows でビルド・起動」、§5.4 残課題 #8
- [mobile/README.md](../mobile/README.md): iOS / Android のビルド手順と到達点
- [.github/workflows/ci.yml](../.github/workflows/ci.yml): 現行 CI（Ubuntu × Linux target のみ）
