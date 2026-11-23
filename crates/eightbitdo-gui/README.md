# eightbitdo-gui（egui 版・凍結）

> **凍結（2026-11-23）**: 正式なデスクトップ GUI は Tauri v2 版（[`apps/desktop`](../../apps/desktop/README.md)）です。本 crate は比較・撤退用として凍結しており、既知バグ修正と実機検証の記録のみ行います（[docs/TAURI_PLAN.md](../../docs/TAURI_PLAN.md) §4）。

8BitDo Ultimate 2 Bluetooth Controller（BLE App Mode `"80ND"`）を操作する egui / eframe 製の Pure Rust GUI です（Node.js・WebView 不使用）。UI は日本語既定（トップバーで英語へ即時切替可）で、Noto Sans JP サブセットを同梱しています（[`assets/fonts/README.md`](assets/fonts/README.md)）。

## 実装済み機能（凍結時点）

- **接続・デバイス情報**: BLE スキャン、`"80ND"` 接続・切断、名前 / アドレス / MTU / GATT characteristic（`FF11` / `FF13`）、`ReadVersion` 表示。
- **ライブ入力**: 接続直後に `0x0007` を有効化し、20ms 周期の `0x0003` ポーリング（keepalive 兼用）でスティック軌跡・トリガー・ボタンを表示。連続 3 回の失敗または `Disconnected` でリンクを閉じ、自動再接続しません。
- **設定表示と編集・書込**: 2768B 設定（プロファイル名・`gamepad_mode`・`cur_slot`・振動・スティック / トリガー・特殊機能・6 軸ジャイロ・LED 3 テーマ・22 ボタンリマップ）の表示と編集。リマップは文書化済み単一キーのみ選択可（`227` / 未知ビット / 複合マスク不可）、`MapKeyRecord.flag` は `0` / `1` のみ。`--force-raw` 相当の経路は置きません。
- **マクロ編集・書込**: 20ms ライブ入力の録音、ステップ編集（削除・待ち時間挿入・値変更）、既存非空マクロへの `Header → Save → Erase → Write → Save` 書込（`total_len` は絶対値、空マクロ新規作成は拒否、録音中は書込不可）。
- **安全機構**: 書込は必ず確認ダイアログ後に実行し、直前に pre-write backup（SHA-256 表示）と TOCTOU 再読込、書込後に read-back 検証を行います。書込中は 20ms ポーリングを停止し完了後に再開します。
- **ログ・エラー・ライセンス**: 翻訳キー + 英語技術詳細の構造化ログ（28px 行高）、操作位置直上のエラーバナー、左レール下部の「ライセンス」画面（MIT / Apache-2.0 全文 + 同梱 5 フォントの全文と著作権表示を `include_str!` で表示）。
- **未実装**: OTA 画面、GUI からのバックアップ復元、マクロの JSON/TOML I/O UI。

## 起動とバックアップ先

```bash
cargo run -p eightbitdo-gui   # 初期 1120x800 / 最小 900x620
```

Linux では X11 または Wayland、OpenGL、BlueZ / D-Bus、`libdbus-1-dev`、`pkg-config` が必要です。

バックアップは `dirs::data_dir()` 配下の `8bitdo-backups/`（Linux: `$XDG_DATA_HOME` または `~/.local/share`、macOS: `~/Library/Application Support`、Windows: `%APPDATA%`）に保存します。

- 手動: `config-YYYYMMDD-HHMMSS.{bin,json}` / 書込前設定: `prewrite-config-YYYYMMDD-HHMMSS.{bin,json}` / 書込前マクロ: `macro-prewrite-*/`（CLI `eightbitdo macro restore --dir <パス>` と互換）。
- 同名時は `-1`, `-2` を付けて既存ファイルを上書きしません。Unix ではディレクトリ `0700`・ファイル `0600` で作成し、JSON 作成失敗時は raw を削除します。

## 構成とデザイン

```text
crates/eightbitdo-gui/
  assets/{fonts,licenses}/   Noto Sans JP サブセット（Regular 1.48MiB / Medium・Bold 各 0.11MiB）とライセンス全文
  src/
    main.rs / app.rs         eframe エントリ・UI（UI スレッドで .await しない）
    theme.rs / i18n.rs       トークン・フォント登録・日英 i18n（enum Key で訳抜けをコンパイル検出）
    worker.rs / driver.rs    tokio 専用スレッド・Session 単一保持（single-flight）
    model.rs / backup.rs / write.rs / licenses.rs
```

テーマは「燻しスレート」（`BG #0B0D10` / `SURFACE_1..3` / `LED #6FE7D8` / 影なし、角丸 12/8/6/4、本文 4.5:1・輪郭 3:1 以上を単体テストで固定）。

## 検証と残課題

```bash
cargo check -p eightbitdo-gui
cargo test -p eightbitdo-gui   # GUI 環境不要の単体テスト 88 件
cargo clippy -p eightbitdo-gui --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

単体テストでは mock transport による読取・書込・TOCTOU 中止・keepalive 停止再開・i18n・theme コントラスト・ヘッドレス描画を検証していますが、**実機での接続・設定/マクロ書込・read-back は未検証（Tier 3）**です。

残課題: 実機書込検証、マクロ JSON/TOML UI、GUI リストア、OTA 画面、言語選択の永続化、`motion_scale` 設定 UI、JIS X 0208 外漢字の補完（必要時は `assets/fonts/build_fonts.py` で再生成）。
