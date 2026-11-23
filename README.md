# 8BitDo Ultimate 2 Cross-Platform Suite

[![CI](https://github.com/haruki0420/8bitdo-ultimate2-suite/actions/workflows/ci.yml/badge.svg)](https://github.com/haruki0420/8bitdo-ultimate2-suite/actions/workflows/ci.yml)

**8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）向けのオープンソース・クロスプラットフォーム設定／マクロ編集／ファームウェアアップデートスイートです。

Nintendo Switch へ Bluetooth Classic または 2.4G アダプターで接続したまま、**Linux / macOS / Windows / Android / iOS** から **BLE GATT（アドバタイズ名 `"80ND"`）** 経由で同時接続し、リアルタイムにマクロやプロファイルを編集・適用することを目指しています。**現時点の実機検証は Linux・BLE のみ**で、ファームウェア更新（OTA）は core 実装のみ・未配線です（[実装状況](#実装状況)）。

> **非公式プロジェクト**: 本プロジェクトは 8BitDo（Shenzhen Bestodo Tech Co., Ltd.）と提携・承認・後援関係にない、独立した非公式オープンソースプロジェクトです。公式のバイナリ・ファームウェア・ロゴは含みません。商標と非提携の扱いは [TRADEMARKS.md](TRADEMARKS.md) を参照してください。

---

## 主な特徴

### 実装済み

- **BLE 設定モード（`80ND`）への接続と読み取り**: Switch に接続したまま Profile ボタン約 3 秒で設定モードに入り、version / 2768B 設定 / マクロ 3 slot × 4 / リアルタイム入力 / keepalive を取得。20ms `0x0003` keepalive、per-user BLE 単一所有ロック、degraded 時の終了コード 3。
- **設定の書き込みとボタンリマップ（BLE）**: `config write` / `config set` / `config remap` / `config map show`。予約バイト保持、CRC 再計算、必須 pre-write backup、前後 SHA-256、TOCTOU 再読込、read-back 検証。**実機確認済み**（full write / remap のみ。`config set` 各フィールドは未確認）。
- **マクロのバックアップ・書き込み・復元（BLE）**: `macro backup` / `macro write` / `macro restore`。公式順序（Header `0xD8` → Save → Erase → Write → Save）と Write `total_len` の**絶対終端**。**実機確認済み**（slot S1 の M1/M2。**Switch 側の発動は未確認**）。
- **GUI**: egui / Pure Rust。日本語 UI、燻しスレートのテーマ、Noto Sans JP 同梱。読み取り 4 タブ（デバイス / ライブ入力 / 設定値 / ログ）に加え、PR #16 で設定・ボタンリマップ・マクロの書き込み UI を追加（確認ダイアログ、pre-write backup、read-back 検証）。**GUI 書込は実機未検証**。
- **セッション REPL**: 1 接続で `info` / `config dump` / `backup` / `monitor` / `macro backup` / `config map show` などを実行。書込系は `--allow-macro-write` / `--allow-config-write` と確認が必須。
- **マクロ・設定の JSON / TOML I/O**: `MacroFile` のインポート / エクスポート。1ms 解像度・8-bit スティック / トリガー値を保持。
- **モバイル雛形**: `eightbitdo-uniffi` 経由の読み取り API、iOS SwiftUI + CoreBluetooth、Android Compose + BluetoothGatt。iOS は unsigned IPA、Android は JNI 同梱 APK（arm64-v8a / x86_64）を生成可能。

### 計画中・未提供

- **GUI からの書き込み**: PR #16 で設定 2768B・リマップ・マクロを実装し main にマージ済み。確認ダイアログ、pre-write backup、read-back 検証、TOCTOU 検出付き。**実機未検証・Tier 3**。
- **GUI ライセンス画面**: PR #24 でレビュー中・未マージ。現 main の GUI にはライセンス表示画面がありません。
- **プロファイル切替 API**: PR #26 で core `Session::switch_profile` + CLI `config profile <N>` + REPL `profile switch <N>` を実装し、レビュー中・未マージ。[docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md) の残作業「`0x0015` 切替のセッション API 化」に対応。
- **ファームウェア更新（BLE OTA）**: core の状態機械と `ota-bench` 段階実行 API（#20）、実機 btsnoop 採取計画（#19）まで用意済み。実機 wire 未確定・CLI / GUI / UniFFI 未配線。USB OTA は USB Boot TX フレーミング未文書化のため保留。
- **モバイルの書き込み・OTA**: バックアップ UI と Tier 3 レビュー待ちで未公開。Android の JNI 同梱 APK と iOS unsigned IPA はビルド成功（#21、実機未検証）、iOS / Android 実機確認、App Store 配布は未着手。
- **USB HID / 2.4G transport**: 未実装・保留。

---

## 実装状況

main は `0582e9a`（2026-11-23）時点。フェーズの詳細・残課題は [AGENTS.md](AGENTS.md) §6 と [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) §5。

2026-11-23 に main へマージ済みの追加分: **#17** raw config の実機 flag 対応（`slot_flags` allowlist、gyro key_map の erasure sentinel 例外を削除しつつ flag の `0x20190000` は raw allowlist で受理を継続、実 dump を使う `#[ignore]` E2E、docs 整合）、**#18** クロスプラットフォーム・ビルド確認（[docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md)）、**#19** OTA 実機 btsnoop 採取計画（[docs/OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md)）、**#20** `ota-bench` 段階実行 API、**#21** モバイルビルド証跡（[docs/MOBILE_BUILD_EVIDENCE.md](docs/MOBILE_BUILD_EVIDENCE.md)）、**#22** 配布・ライセンス・マネタイズ方針（[docs/DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md)）、**#23** 公式機能カバレッジ・マトリクス（[docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md)）。**GUI ライセンス画面は PR #24、プロファイル切替（`0x0015`）の core / CLI 配線は PR #26 として、いずれもレビュー中で未マージ（#24 は main へ未マージ）**。

| Phase | 内容 | 状態 |
| --- | --- | --- |
| 1 | `eightbitdo-protocol`（CRC・ヘッダ・2768B・KeyMap・マクロ・入力） | 完了（単体 100 + doc test 4） |
| 2a | 読み取り専用 core / CLI / `session` REPL | 完了・実機確認済み |
| 2b | BLE マクロ backup / write / restore | 完了・実機確認済み（S1 の M1/M2。Switch 側の発動は未確認） |
| 2c | BLE OTA 状態機械 | **core のみ**。Mock 52（既定）/ 55（`mobile-boot`）/ 58（`ota-bench`）/ 61（`--all-features`）テスト。実機未検証・CLI/GUI/UniFFI 未配線。`ota-bench` 段階実行 API（#20）と採取計画（#19）は用意済み |
| 2d | 設定 write / set / remap（BLE） | 完了・実機確認済み（full write / remap のみ。`config set` 各フィールドは未確認）。#17 で実機 flag を opaque 値として扱う検証に修正 |
| 3a | 読み取り専用 GUI | 完了（PR #9） |
| 3b | GUI マクロエディタ（録音・ステップ編集・既存マクロ書込） | 実装・main にマージ（PR #16）。**実機未検証・Tier 3** |
| 3c | OTA 画面 | 未着手（Phase 2c の wire 確定後） |
| 3d | GUI 設定タブ / 22 ボタンリマップ | 実装・main にマージ（PR #16）。**実機未検証・Tier 3** |
| 4 | USB OTA | 保留（USB Boot TX 未文書化） |
| 5 | UniFFI + iOS / Android | **JNI 同梱 APK / unsigned IPA ビルド成功、実機未検証**（#21） |
| — | GUI ライセンス画面 | **PR #24（レビュー中・未マージ）**。実装予定 |
| — | プロファイル切替 API（`0x0015`） | **PR #26（レビュー中・未マージ）**。core `Session::switch_profile` / CLI `config profile <N>` / REPL `profile switch <N>`。実機未検証（ACK 形状・Save 要否・永続性は未確認） |

**実機で確認済み（Linux・BLE・MTU 200、2026-11-21〜23）**: version / 2768B dump / マクロ backup、マクロ書込（絶対終端 `total_len`）と復元、設定 full write（38 write + 1 Save = 合計 39 packets・74B chunk・read-back SHA 一致）、remap A→B→A。2026-11-23 に再接続後に再確認し、version `0xC8`、config full write → read-back、`config remap A B` → read-back で `A A` の往復（read-back SHA `fae1de6…` / `7a9838e…`）、`macro restore`（slot 0 の M1/M2、read-back SHA `7ebbfde…` / `c6f2419…`、22 packets）を確認した。**実機 config の SHA は 2026-11-22 時点（`1dca47fa…`）と異なる**（プロファイル名 `za` / `Profile2` を設定したためと考えられ、現在は `95b87bb0…`）ので、固定値の一致ではなく、**各操作後の read-back が期待状態と一致し、復帰後は書込前の read-back と一致する**ことで検証している。**設定の実機確認範囲は full write / remap のみ**（`config set` 各フィールド・`--force-raw`・MTU 200 以外は未確認）。

**既知バグ（未修正、2026-11-23 発見）**: BLE リンク切断後も `session` の `status` が `connected: true` と keepalive カウンタを表示し続けることを観察した。その状態での送信時に `Not connected` エラーが返ることは確認済み。`status` が実リンク状態に追従しないため、`status` からはリンク断を検知できない。

**未確認**: slot S2/S3・`macro_idx=2/3 (M3/M4)`、MTU 200 以外、USB HID の config / macro write、`config set` の各フィールド実機確認、`--force-raw` / `--accept-reserved-drift`、GUI 書込（PR #16）と REPL からの書込、OTA の wire / ACK / アドレス / MTU、macOS / Windows / Android / iOS 実機、Android / iOS の JNI ロードと実機起動。

---

## ビルドと実行

**前提**: Rust **1.85 以上**（workspace の MSRV）。Linux では btleplug 用に `libdbus-1-dev` と `pkg-config`（CI と同じ）、BLE には BlueZ が必要です。**Windows をビルドする場合は Rust 1.88 以上**が必要です（`btleplug` の Windows バックエンドが let チェーンを使うため、1.85〜1.87 では `E0658` で失敗します。詳細と確認済みの範囲は [docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md)）。macOS / Windows の実機検証は未実施です。

```bash
# ワークスペースのビルド
cargo build --release --workspace

# CLI（target/release/eightbitdo）と GUI（target/release/eightbitdo-gui）
./target/release/eightbitdo --help
./target/release/eightbitdo scan            # 80ND が見えるか確認
./target/release/eightbitdo info
./target/release/eightbitdo config dump --raw config.bin --json config.json
./target/release/eightbitdo-gui
```

```bash
# 検証（CI と同じ）
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo +1.85.0 check --workspace --all-targets --all-features --locked
```

モバイルは [mobile/README.md](mobile/README.md) を参照してください（`scripts/mobile/build-ios.sh` / `build-android.sh`）。iOS は xtool + Darwin Swift SDK、Android は Android SDK（Rust JNI を含む完全ビルドには NDK + cargo-ndk）が必要です。

**書き込み前の注意**: 実機では読み取り専用コマンドから確認し、最初の書き込み前に 2768B 設定とマクロ 3 slot × 4 をバックアップしてファイル名と SHA-256 を記録してください。書込系は pre-write backup と read-back 検証を必須にしています。

---

## CLI（BLE のみ。USB HID は未実装）

主なサブコマンド:

- `scan [--check]` / `info` / `backup [--raw FILE] [--json FILE] [--hold N]` / `monitor [--count N]` / `keepalive [--interval-ms 20] [--duration 120]`
- `config dump` / `config write` / `config set` / `config remap` / `config map show` / `config profile`
- `macro backup --dir DIR` / `macro write --slot N --index N --file FILE [--trigger KEY]` / `macro restore --dir DIR [--force]`
- `session [--interval-ms 20] [--allow-macro-write] [--allow-config-write] [--no-report-enable]` — 1 接続の REPL

`eightbitdo config` は full 2768B 設定の書込みを扱います（USB HID config write は未実装）。

- `config dump [--raw FILE] [--json FILE]` — 読取のみ。
- `config write --file <2768B>` — `config dump --raw` の編集ファイルを書込み。予約バイトの改変は拒否し、復旧時のみ `--accept-reserved-drift` で device の現在値に合わせる。
- `config set <field> <value>` — プロファイル名、cur_slot、gamepad_mode、rumble / joy / trigger / gyro / LED、`map_key.flag`（typed は 0/1 のみ。raw dump の実機値は opaque で `0x20200911` 等）。範囲外は I/O 前に拒否。joy / trigger はレコード flag=1、LED color はテーマ flag=1 の副作用あり（help に明記）。
- `config remap <physical> <target>` — 1 ボタンのみ（4B）変更。`config map show` で 22 index を表示。
- `config profile <0|1|2>` — 公式の `WriteSLOT`（`0x0015`、payload なし、`subcmd=slot`）でアクティブプロファイルを切り替える。**Save は送らない**（`0x0015` に Save が必要かは実機未確認）。ACK 形状・永続性も未確認のため、read-back での検証は行わず、その旨を出力する。REPL では `profile switch <N> [--yes]`。
- 安全策: **pre-write backup 必須**（`config.bin` + `config.json`）、`--yes` なしの確認、前後 SHA-256、確認後の TOCTOU 再読込、read-back 検証、マクロヘッダ変更警告。書込 chunk は既定 `min(MTU, 74)`（RE: 公式 74B）で、`--write-chunk` は接続前上限 **180**（観測 MTU 200 のペイロード上限）まで指定可能。
- raw `config write` の検証: 22 個の `button_mappings`、`gyro.key_map`（documented single のみ。`0xFFFFFFFF` は map_key の消去 slot 専用）、マクロ `max_steps`（0/0xFFFF/1..=200）・`total_cnt`、マクロ `key_map`（`macro write --trigger` と同じ任意 KeyMap）を確認。**各レコードの flag は実機観測値集合（`0` / `1` / `0x20200911` / `0xFFFFFFFF`、gyro は `0x20190000` も）のみ受理**し、`slot_flags` も同じ観測値集合のみ受理する。それ以外は `--force-raw` が必要。数値範囲（joy/gyro/hotwheel/rumble）は **flag の値に関係なく常時検証**するが、実機の完全消去 slot（`slot_flags` が `0xFFFFFFFF` + flag `0xFFFFFFFF` + joy 全 255 + rumble NaN + 22 key 全 `0xFFFFFFFF`）は **slot 全体が揃った場合のみ**その実機状態として受理する（joy だけ・rumble だけ・map_key だけの partial state は拒否）。`0xFFFFFFFF` が flag 値として全レコードで許可されるのは「flag 値のみの許可」で、payload の消去扱いにはならない。typed `config set` の flag のみ 0/1。不明な `cur_slot`（実機観測 6 など）は警告して保持。`--force-raw` は未文書値の復旧用（予約領域ガードは維持）。実バイト証跡は [docs/DEVICE_CONFIG_EVIDENCE.md](docs/DEVICE_CONFIG_EVIDENCE.md)。
- `session --allow-config-write` の REPL からも `config set` / `config remap` / `config write --file` / `profile switch <N> [--yes]` を実行できる（`config map show` と `config dump` はフラグ不要の read-only）。`profile switch` は単発の `0x0015` で pre-write backup を取らない（それ以外の書込は backup 必須）。書込は `--yes` で対話確認なし、無い場合は `yes` 入力の確認、pre-write backup、前後 SHA-256、read-back 検証付き。確認待ちの間は operation lock を解放して 20ms keepalive を継続し、確認プロンプトは Ctrl-C で中止できる（pre-write backup は残る）。実書込（最初の `0x0001` 以降）だけが operation lock 下の Ctrl-C 非キャンセル区間で、書込後は keepalive が再開する。
- 既知の差: 公式 BLE Save は 2000ms / 最大 2 再送（静的解析）だが、本実装は 3 秒 / リトライなし。USB config write は未実装。
- BLE 競合・degraded 対策: `scan` / `connect` / `session` と `BleTransport::connect` を使う全 CLI コマンド（info / config / macro / backup / monitor / keepalive）は、BLE を触る前に **per-user** 単一所有者 flock を取得する。パスは環境非依存で、`EIGHTBITDO_BLE_LOCK`（テスト/ラッパー用の信頼された例外）→ NSS（`getpwuid_r`）の euid ホーム由来の `~/.cache/8bitdo/8bitdo-ble.lock` → ホーム不明時のみ temp の per-user dir。**XDG_RUNTIME_DIR / XDG_CACHE_HOME / `$HOME` は参照しない**ので、デスクトップ/SSH/cron/systemd で同じユーザーなら同じロックになる。`O_NOFOLLOW` と fd メタデータ検証で symlink / 非通常ファイル / 他ユーザー所有を拒否し、既定パスでは既存 parent が euid 所有かつ group/other write なし・**symlink でない**ことを検証し、Unix は dirfd（`openat`）で TOCTOU も閉じる。取得できなければ「別プロセスが BLE を使用中」として失敗する（`session` は接続保持中ロックを保持し、teardown 成功時のみ解放）。別ユーザー同士は排他しない。`session` の keepalive が **I/O エラー連続 3 回**（無応答は failure に数えない）/ `Disconnected` で degraded になると終了コード **3** で終了し、自動再接続しない（コマンドが観測した `Disconnected` は exit 1）。切断と同時に App Mode を抜けて `80ND` が広告されなくなるため、再開には Profile 約 3 秒押しが必要である旨を表示する。SIGINT は範囲に応じて警告する（REPL 終了 0 / connect 前・info/config/macro 130 / backup・monitor・keepalive は全体を 130 / flash は prepare（ファイル読込・backup・確認プロンプト）が**キャンセル可**で、最初の flash フレーム以降だけ**遅延**して完了後に復旧案内 + 130）。`scan --check` は 1 行出力し、exit 0 = App Mode あり / 3 = なし / その他 = エラーで、人間向け出力の grep は不要。常駐運用は `scripts/session-loop.sh`（exit 0 では最低 2 秒待ってから `scan --check` し、見えれば指数 backoff で再起動、見えなければ押下案内して停止。bash 4 + GNU date 必須、macOS 非対応）。押下を依頼する前に `scan` で `80ND` を確認する手順は [docs/SESSION_OPERATIONS.md](docs/SESSION_OPERATIONS.md) を参照。
- 互換性（pre-1.0）: `eightbitdo-core::BleOptions` に `lock_path: Option<PathBuf>` を追加（構造体リテラル構築側は `lock_path: None` が必要）。既定ロックパスは `/tmp` 直下から per-user ディレクトリに変更。
- REPL の引数は空白区切り。スペースを含むプロファイル名は top-level `config set profile_name "..."`（シェル引用）を使う。

---

## GUI

`eightbitdo-gui` は egui / eframe のネイティブ GUI です。Node.js / WebView / Electron / Tauri は使いません。

- **6 タブ**: デバイス（接続・情報）、ライブ入力、設定値（2768B の要約）、設定（書き込み）、マクロ（録音・編集・書き込み）、ログ。
- 日本語 UI と英語表示の切り替え、燻しスレートのテーマ、Noto Sans JP（OFL）を同梱。
- **書き込み**: 設定 2768B（プロファイル・リマップ・振動・スティック / トリガー・ジャイロ・LED）と既存マクロの書き込みに対応。必ず確認ダイアログを経て、直前に設定 / マクロの pre-write backup を作成し、書込後に read-back 検証します。録音中は書込不可。確認後にデバイス設定が変わっていた場合は expected SHA-256 不一致で中止します。**実機未検証（Tier 3）**。バックアップ復元とファームウェア更新は未実装です。
- 起動: `cargo run --release -p eightbitdo-gui`（または `./target/release/eightbitdo-gui`）。

## モバイル（iOS / Android）

`mobile/` に iOS（SwiftUI + CoreBluetooth）と Android（Jetpack Compose + BluetoothGatt）の雛形があります。プロトコル・設定・マクロはすべて Rust コアを `eightbitdo-uniffi` 経由で共有します。

- **到達点**: Rust 側の読み取り API（version / config / macro backup / input / disconnect / `describeConfig` / `prepareConfigWrite`）、iOS unsigned IPA、Android JNI 同梱 APK（arm64-v8a / x86_64、JVM unit test 8 件）。
- **未達**: iOS / Android 実機での BLE 読み取り、App Store / TestFlight 配布（macOS + Xcode 必須）、Android の JNI ロード実機確認。書き込み・OTA はバックアップ UI と Tier 3 レビュー待ちで未公開。
- 手順・制約の詳細は [mobile/README.md](mobile/README.md)。

## ファームウェア更新（OTA）の現状

- **BLE OTA**: `crates/eightbitdo-core/src/ota.rs` に状態機械を実装済み（Mock 52（既定）/ 55（`mobile-boot`）/ 58（`ota-bench`）/ 61（`--all-features`）テスト）。実機採取用の `ota-bench` 段階実行 API は #20、btsnoop 採取手順と判定基準は [docs/OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md)（#19）で用意済み。ただし **実機 btsnoop 未取得・実機書込未検証・CLI / GUI / UniFFI 未配線**です。未確認 wire の実行時ゲートは既定 OFF。
- **公式 `.dat` の制約**: 現実装は **単一セクション `.dat` のみ**対応。公式ファームがマルチセクションの場合は更新を拒否するため、これは公式ファーム対応の **blocker** です。
- **ACK 照合**: 現状は cmd/subcmd のみで、Bootloader ACK の `offset` / `total_len` を照合していません。遅延・重複 ACK の誤帰属を防ぐため、**CLI 配線前に照合を追加するのが必須**です。
- **USB OTA**: USB Boot の TX フレーミングが未文書化のため `N/A + 理由（USB Boot TX 未文書化）`。BLE の `"8bt"` / prefix `0x05` を流用しません。
- 未検証事項の一覧は [docs/PROTOCOL_SPEC.md](docs/PROTOCOL_SPEC.md) §8.5、解析証跡は [docs/OTA_BLE_EVIDENCE.md](docs/OTA_BLE_EVIDENCE.md) を参照してください。

---

## ドキュメント一覧

公式バイナリの静的解析から検証した完全仕様と実装計画を `docs/` にまとめています。

| ドキュメント | 内容 |
| --- | --- |
| **[docs/PROTOCOL_SPEC.md](docs/PROTOCOL_SPEC.md)** | デバイス VID/PID、BLE GATT UUID・MTU、16B カスタムヘッダ、BLE/USB HID フレーミング、2 種の CRC-16、全コマンド、リアルタイム入力ストリーム、BLE OTA ブートローダー＆ファームウェア API 仕様 |
| **[docs/STRUCT_BT2_2768B.md](docs/STRUCT_BT2_2768B.md)** | 2768 バイト（`0xAD0`）設定構造体 (`record_ultimatebt2`) の全バイトオフセット、32-bit `KeyMap`、10 バイト `MacroDatas` ステップ構造、Flash 書き込みシーケンス |
| **[docs/DEVICE_CONFIG_EVIDENCE.md](docs/DEVICE_CONFIG_EVIDENCE.md)** | 実機 2768B ダンプの実バイト証跡（2026-11-22 時点の dump は SHA-256 `1dca47fa…`。現在の実機 config はプロファイル名の変更により `95b87bb0…` と異なると考えられる）: `slot_flags` / `map_key.flag` の非 0/1 値、`config remap` の差分、raw 検証エラーと再現コマンド |
| **[docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md)** | 公式 Windows / Android ソフトの機能ごとのカバレッジ: RE 証拠ラベル、本リポジトリの実装状況（core / CLI / GUI）、実機検証状況（Linux BLE のみ）、残作業と追加 RE 項目 |
| **[docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)** | Rust + SwiftUI + Jetpack Compose のアーキテクチャ設計、ワークスペース構成、Phase 1〜5 ロードマップ、現在の状態・残課題、安全検証手順 |
| **[docs/OTA_BLE_EVIDENCE.md](docs/OTA_BLE_EVIDENCE.md)** | BLE OTA の解析証跡（確定した順序・タイムアウト・`.dat` ヘッダ）と未確認事項の優先順 |
| **[docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md)** | クロスプラットフォームのビルド確認状況（Windows クロス `check` の結果と警告 4 件、MSRV 1.85 が Windows で通らない理由、iOS target check、macOS 未確認の理由、CI 追加提案） |
| **[docs/OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md)** | BLE OTA 実機検証の準備: btsnoop 採取（Linux `btmon` / Android HCI snoop）、確認項目と判定、EVIDENCE への追記、Phase A / B の安全手順 |
| **[mobile/README.md](mobile/README.md)** | iOS / Android のビルド手順、UniFFI 公開 API、到達点と制約（unsigned IPA / JNI 同梱 APK / 未公開の書込） |
| **[docs/SESSION_OPERATIONS.md](docs/SESSION_OPERATIONS.md)** | 常駐 `session` の運用（per-user BLE 単一所有者ロック、degraded 終了コード 3、SIGINT 範囲、押下依頼前の `scan` 確認手順、`scripts/session-loop.sh` の scan 契約と backoff） |
| **[AGENTS.md](AGENTS.md)** | AI エージェント／開発者向け引き継ぎガイド（[CLAUDE.md](CLAUDE.md) からも参照） |
| **[CONTRIBUTING.md](CONTRIBUTING.md)** | ブランチ運用、必須 CI、独立レビュー（Tier 1〜3 / SHA 紐づきの `review-gate` 成功）の手順 |
| **[docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md)** | `main` の canonical ruleset、`review-gate` の付け方・監査とゲートの限界 |
| **[docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)** | リスクに応じた独立レビューの Tier 1〜3、レビュアーの役割、故障リストの書き方 |
| **[LICENSE-MIT](LICENSE-MIT)** / **[LICENSE-APACHE](LICENSE-APACHE)** | コードのライセンス全文（MIT / Apache-2.0） |
| **[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)** | 依存ライブラリのライセンス一覧、帰属表示、同梱フォント（OFL / UFL）・Gradle wrapper（Apache-2.0）の扱い |
| **[TRADEMARKS.md](TRADEMARKS.md)** | 第三者の商標、非公式・非提携、ロゴ不使用の方針 |
| **[docs/DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md)** | リポジトリ公開・コードライセンス・商標・同梱フォント・App Store / Google Play 配布・マネタイズの決定事項（決定日 2026-11-23）と未決定事項 |

---

## 現在のステータス

- `main` は **PR + CI + 独立レビュー（SHA 紐づきの `review-gate` 成功）** で保護済みです（`main-protection`, id `27785806`, 2026-11-21 検証）。レビュー深度は [docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)、詳細は [CONTRIBUTING.md](CONTRIBUTING.md) と [docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md) を参照してください。
- [x] 公式アプリ（Android v2.01 / Windows v1.38）の通信プロトコル・バイナリ構造・OTA シーケンスの解析完了
- [x] 完全仕様書・実装計画書・AI 引き継ぎドキュメント作成
- [x] Phase 1: `eightbitdo-protocol`（Rust プロトコル・構造体・CRC・マクロ、単体テスト 100 件 + doc test 4 件）
- [ ] Phase 2: `eightbitdo-core` + `eightbitdo-cli`（BLE 実機通信＆検証。**Phase 2 一部完了（2c 未完了）**。2a / 2b / 2d は実機確認済み、2c BLE OTA は core 実装のみ）
  - [x] Phase 2a: 読み取り専用 core / CLI / `session`（`eightbitdo session [--interval-ms 20] [--allow-macro-write] [--allow-config-write] [--no-report-enable]` の REPL は `info` / `config dump` / `backup` / `monitor` / `monitor stop` / `status` / `help` / `quit`。REPL には `macro backup <DIR>`（読み取りのみ、常に利用可）/ `macro write <SLOT> <INDEX> <FILE> [--trigger <KEY>] [--chunk-size <32|64>]` / `macro restore <DIR> [--force]` もあり、書込系は `--allow-macro-write`（既定 OFF）が必要。`session` は既定で起動時に `0x0007 enable(1)` を送る（公式相当）ため REPL の `macro write` は enable 状態で動き（トップレベル `macro write` の既定とは条件が異なる）、`--no-report-enable` 指定時は起動時の enable(1) を送らず `0x0003` keepalive のみを継続する（終了時の disable(0) は best-effort。トップレベル既定と同じ書込条件で A/B するため）。`--allow-macro-write` 時の `macro write` / `macro restore` は、ファイル読込・pre-write/pre-restore backup を含むコマンド全体で Ctrl-C を遅延する（非通常ファイルは読取前に拒否し、デバイス I/O は timeout で有界。SIGKILL・電源断は防げない）。REPL の `macro write` は `--backup-dir`（既定名 `macro-backup-<unix-nanos>-<pid>-<sequence>`）、`macro restore` は `--chunk-size` を指定できず、REPL 経由の実機書込は未検証。設定バックアップは `--raw` / `--json` を同一ディレクトリの一時ファイル経由でアトミックに書き、同一ファイルを指す 2 つの出力パスは拒否する。ただし hard link の同一性検出は Unix のみで、Windows は file identity 未実装のため hard link alias を事前拒否せず、raw 書き込み後の再検証と `rename` semantics で保護する。この再検証は偶発的な alias 用であり、出力ディレクトリに write 権限を持つ攻撃者による TOCTOU / 並行 rename は非保証である）
  - [x] Phase 2b: マクロ読み書き（実機確認済み。BLE・slot S1 の M1/M2。M3/M4・S2/S3 は外挿）
  - [ ] Phase 2c: BLE OTA（Linux。core 実装済み・Mock 52 テスト（既定）/ 55（`mobile-boot`）/ 58（`ota-bench`）/ 61（`--all-features`）。**単一セクション `.dat` のみ対応**で公式 `.dat` がマルチセクションの場合は未対応（公式ファーム対応の blocker）、未確認 wire ゲート既定 OFF、boot 専用 `BleBootTransport` のみ許可（App Session を切断してから `8bt` に接続。同一 BLE ロック）、type 1 application ウィンドウを暫定検証、CLI/GUI 未配線、実機書込は保留。実機採取用の `ota-bench` 段階実行 API（既定 OFF、`write_firmware_with_checkpoint` / `OtaCheckpoint::AfterFirstErase` 等）は #20 でマージ済み、採取手順は [docs/OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md)（#19）。マルチセクション対応と実機 wire 確定は次 PR の必須項目）
  - [x] Phase 2d: 設定書き込み（BLE のみ実機確認済み。`config write` / `set` / `remap` / `map show`、pre-write backup、確認、前後 SHA-256、read-back 検証。USB config write は未実装）。#17 で実機 flag を opaque 値として扱う raw 検証に修正（`slot_flags` allowlist、gyro key_map の erasure sentinel 例外を削除しつつ flag の `0x20190000` は raw allowlist で受理を継続、実 dump を使う `#[ignore]` E2E）
- [x] Phase 3a: `eightbitdo-gui` 読み取り専用 GUI（日本語・燻しスレート。PR #9）
- [x] Phase 3b: GUI マクロエディタ（20ms 録音・ステップ編集・既存非空マクロへの書込。PR #16。**実機未検証・Tier 3**）
- [ ] Phase 3c: GUI OTA 画面（Phase 2c の実機 wire 確定後）
- [x] Phase 3d: GUI 設定タブ / 22 ボタンリマップ（PR #16。**実機未検証・Tier 3**）
- [ ] Phase 4: USB OTA（保留）
- [ ] Phase 5: `eightbitdo-uniffi` + iOS (SwiftUI) / Android (Jetpack Compose) アプリ（雛形完了・PR #12。JNI 同梱 APK / unsigned IPA はビルド成功（#21、[docs/MOBILE_BUILD_EVIDENCE.md](docs/MOBILE_BUILD_EVIDENCE.md)）。実機確認・App Store 配布は未着手）
- [ ] GUI ライセンス画面（PR #24、レビュー中・未マージ。実装予定）
- [ ] プロファイル切替 API（PR #26、レビュー中・未マージ。core `Session::switch_profile` + CLI `config profile <N>` + REPL `profile switch <N>`。FEATURE_MATRIX の残作業「`0x0015` 切替のセッション API 化」に対応）
- [x] クロスプラットフォーム・ビルド確認（[docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md)、#18。実機は Linux のみ。Windows は `cargo check` のみ、macOS は未確認）
- [x] 配布・ライセンス・マネタイズ方針（[docs/DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md)、#22）
- [x] 公式機能カバレッジ・マトリクス（[docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md)、#23）
- [ ] 既知バグ: BLE 切断後も `session` の `status` が `connected: true` と keepalive カウンタを保持する。送信時に `Not connected` エラーが返ることは確認済み（2026-11-23 発見・未修正）

---

## 注意事項

- 本プロジェクトは非公式のオープンソースプロジェクトであり、8BitDo とは一切関係ありません。
- 著作権保護された公式ソフトウェアのバイナリ・ファームウェアファイル自体は本リポジトリに含まれていません。

---

## ライセンス

- **コード**（`crates/` 以下）: [MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のいずれかを選択できます（`MIT OR Apache-2.0`）。
- **ドキュメント**（`docs/` 以下と `README.md` など）: 特記がない限り、コードと同じ `MIT OR Apache-2.0` です。将来 CC BY 4.0 などの別ライセンスへ変更する場合は、変更時に本文へ明記します。ただし、第三者に由来する部分（同梱フォントの OFL、第三者コードの BSD / Unicode / MPL 表示、引用・出典）は、それぞれの権利者のライセンス条件に従います。
- **同梱フォント**（GUI に同梱済み）: Noto Sans JP のサブセットは SIL Open Font License 1.1 です。`crates/eightbitdo-gui/assets/fonts/OFL.txt` を同梱しています。egui 既定フォント（Ubuntu-Light / NotoEmoji / Hack / emoji-icon-font）は OFL 1.1・Ubuntu Font Licence 1.0・MIT で、帰属は [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) §3.6.1 に整理しています。
- **Gradle wrapper**（`mobile/android/gradle/wrapper/gradle-wrapper.jar` など、ビルド再現性のためコミット済み）: Apache-2.0（jar 内 `META-INF/LICENSE`、manifest の `SPDX-License-Identifier`）。帰属は [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) §3.6.3 を参照してください。
- 依存ソフトウェアのライセンスは [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) を参照してください。**THIRD_PARTY_LICENSES.md の基準 SHA は `6b39e23` のまま**です（同ファイルは GUI / モバイル依存を含む形で `6b39e23` 時点の `cargo metadata` を集計しており、#17〜#23 では dependency の追加・削除はありません。今回の main `0582e9a` の `Cargo.lock` も 502 パッケージで `6b39e23` と同数、第三者クレート **497**（全 502 パッケージ − workspace 5）です。`Cargo.lock` の差分は workspace 内クレート `eightbitdo-cli` への `serde` 依存追加 1 行のみ）。大半は MIT または Apache-2.0 で、代表的な依存は `eframe`/`egui`（MIT OR Apache-2.0）、`uniffi`（MPL-2.0、ファイル単位コピーレフト）、`btleplug`（MIT/Apache-2.0/BSD-3-Clause）です。Cargo 管理外では Noto Sans JP（OFL 1.1）と Gradle wrapper（Apache-2.0）を同梱しています。同ファイルは要約と参照であり、全クレートの著作権表示・ライセンス本文を網羅しません。バイナリ配布時は `cargo-about` 等で完全な第三者通知を生成して同梱してください。配布・ストア申請前の再確認要否は [docs/DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md) §5 を参照してください。
- 商標・非公式に関する方針は [TRADEMARKS.md](TRADEMARKS.md) を参照してください。
- リポジトリ公開・コードライセンス・商標・同梱フォント・ストア配布・マネタイズの方針は、[docs/DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md) に決定事項としてまとめています（決定日 2026-11-23）。未決定の項目は同ファイル §7 に「要判断」として列挙しています。

## 免責

本プロジェクトは**非公式**の独立したオープンソースプロジェクトであり、8BitDo（Shenzhen Bestodo Tech Co., Ltd.）とは提携・承認・後援関係にありません。"8BitDo" は同社の登録商標です。"Ultimate 2" は同社の製品名です（単独の商標登録の有無は未確認）。本リポジトリは公式のバイナリ・ファームウェア・ロゴ等を含みません。

本ソフトウェアは無保証（"AS IS"）で提供されます。コントローラへの設定・ファームウェア書き込み・マクロ書き込みは、文鎮化・保証喪失・データ損失のリスクを伴います。自己責任で使用してください。
