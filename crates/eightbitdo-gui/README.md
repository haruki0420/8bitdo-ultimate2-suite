# eightbitdo-gui（Phase 3a / 読み取り専用）

8BitDo Ultimate 2 Bluetooth Controller（BLE App Mode `"80ND"`）を操作する
Pure Rust デスクトップ GUI です。egui / eframe を使い、Node.js・WebView・
Electron・Tauri は使用していません。

**このビルドは読み取り専用です。** 設定の書き込み、マクロの Erase / Write /
Save、バックアップからのリストア、ファームウェア更新は実装していません。

## できること

- **接続パネル**: BLE スキャン（`scan_devices`）、`80ND` への接続、切断、状態表示
- **デバイス情報**: 名前、アドレス、MTU、使用中の GATT characteristic（`FF11` / `FF13`）
- **ファームウェア**: 接続直後に `ReadVersion` を実行し、raw 4 バイト・text・LE `u32` を表示
- **ライブ入力**: 接続直後に `0x0007` を有効化し、16ms 周期の `0x0003` ポーリングで
  スティック、トリガー、ボタン、バッテリー raw 値を約 60fps で表示（Stop も可能）
- **設定ビューア**: 2768B 設定の profile flag、`gamepad_mode`、`cur_slot`、
  プロファイル名、3 プロファイル × 4 マクロのヘッダ（name / max_steps / offset /
  key_map / cycles / interval など）を読み取り専用で表示
- **バックアップ**: 2768B の raw と JSON サマリを保存し、保存先と SHA-256 を表示
- **エラー表示**: 失敗は上部のエラーバナーとログタブに表示。GUI スレッドは停止しない

## 起動

```bash
cargo run -p eightbitdo-gui
```

Linux では X11 または Wayland、OpenGL、BlueZ / D-Bus が必要です。
`btleplug` は BlueZ を使用します。CI と同様に、Debian/Ubuntu では
`libdbus-1-dev` と `pkg-config` を入れてください。Wayland は `wayland-dlopen`
付きの winit を eframe の既定 feature で使用します。

## バックアップの保存先

`dirs::data_dir()` 配下の `8bitdo-backups/` です。

- Linux: `$XDG_DATA_HOME/8bitdo-backups`、未設定なら
  `~/.local/share/8bitdo-backups`
- macOS: `~/Library/Application Support/8bitdo-backups`
- Windows: `%APPDATA%\8bitdo-backups`

既存 CLI の `backup` は出力パスを明示する設計で、既定ディレクトリは持っていません。
GUI 側の既定値として上記を採用しています。

ファイル名は `config-YYYYMMDD-HHMMSS.bin` / `.json` です。同名ファイルがある場合は
`-1`, `-2`, ... と連番を付け、**既存ファイルを置き換えません**。Unix では
ディレクトリを `0700`、ファイルを `0600` で作成します。raw を書いた後に JSON の
作成が失敗した場合は、中途半端な raw を削除します。

JSON は CLI のサマリと互換のキー（`size`, `sha256`, `cur_slot`, `gamepad_mode`,
`slot_flags`, `profile_names`, `crc_stored`, `crc_in_place`）に加えて、
マクロヘッダと `format` / `created_utc` を含みます。設定の全バイトやマクロ名を
ログへ出力することはありません。

## 構成

```text
crates/eightbitdo-gui/src/
  main.rs    eframe の起動だけを行う薄いエントリポイント
  app.rs     egui の画面。UI スレッドで .await しない
  worker.rs  BLE ワーカースレッド。tokio ランタイムと Command/Event チャネル
  driver.rs  Session を 1 本だけ保持する読み取りドライバ
  model.rs   2768B / 入力レポートの表示用モデルと整形
  backup.rs  raw + JSON の安全な新規作成と SHA-256
```

- UI スレッドは `Command` を送り、`Event` を毎フレーム drain するだけです。
- BLE ワーカーは専用スレッドの tokio ランタイム上で動き、`Session` の
  single-flight 契約を守るため、読み取りと入力ポーリングを同時に実行しません。
- ライブ入力中の 16ms `0x0003` ポーリングが keepalive を兼ねます。接続直後の
  version / config 読み取りが終わると自動で開始します。現時点の
  `eightbitdo-core` に session 所有の keepalive はないため、CLI の
  `run_keepalive` と同じ方式を GUI ワーカー側に置いています。Stop すると
  ポーリングも止まるため、設定モードが失効する可能性があります。
- 連続 3 回の入力失敗、または `Disconnected` でリンクを閉じ、理由をログに残します。
  自動再接続はせず、ユーザーが Connect で再接続します。

## 検証

```bash
cargo check -p eightbitdo-gui
cargo test -p eightbitdo-gui
cargo clippy -p eightbitdo-gui --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

単体テストは GUI 表示環境を必要としません。モックトランスポートで以下を検証しています。

- `ReadVersion` の応答デコードと `0x000B` 要求
- ReadConfig の 20 チャンク組み立て、SHA-256、`0x0002` 要求
- `0x0003` 入力レポートのデコードと送信要求
- 切断時の fail-closed エラー
- バックアップの raw / JSON / SHA-256 / 0600 / 上書き禁止
- 2768B 設定からマクロヘッダへの変換、未知 KeyMap ビットの表示

この worktree では **実機接続を行っていません**。スキャン、接続、ライブ入力、
実機からのバックアップはハードウェア未検証です。

## Phase 3b への申し送り

- マクロ 3 スロット × 4 本の Flash バックアップは、core にマクロ読み出し API が
  入ってから追加する（PR #6 / Phase 2b の後）。
- マクロ編集 UI、録音、JSON/TOML 入出力は Phase 3b。書き込み系を追加する際は
  Erase → Write → Header → Save の順序と、書き込み前バックアップを必須にする。
- 設定書き込み（Phase 3d）と OTA 画面（Phase 3c）は、それぞれ core の write path
  と OTA API が安定してから着手する。
- 既定フォントは Latin のみのため、GUI は日本語を表示しない。デバイス由来の
  非 ASCII 名は `\u{XXXX}` 形式でエスケープ表示している。日本語 UI が必要なら
  Noto Sans JP などの同梱フォントを追加する。
- workspace は `resolver = "3"` に変更し、`rust-version = 1.85` を守る範囲で
  eframe の依存を解決している。GUI を追加しても CI の `msrv` は 1.85 のまま。
