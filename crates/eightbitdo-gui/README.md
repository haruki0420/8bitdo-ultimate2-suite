# eightbitdo-gui（Phase 3a / 読み取り専用）

8BitDo Ultimate 2 Bluetooth Controller（BLE App Mode `"80ND"`）を操作する
Pure Rust デスクトップ GUI です。egui / eframe を使い、Node.js・WebView・
Electron・Tauri は使用していません。

**このビルドは読み取り専用です。** 設定の書き込み、マクロの Erase / Write /
Save、バックアップからのリストア、ファームウェア更新は実装していません。

UI は日本語が既定です（トップバーの `日本語 | English` で即時切り替え）。
日本語表示のため Noto Sans JP のサブセットを同梱しており、システムフォントに
依存しません。フォントの出所・ライセンス・再生成方法は
[`assets/fonts/README.md`](assets/fonts/README.md) を参照してください。

## できること

- **主導線**: 未接続時は中央パネルがオンボーディング画面になる。状態カード +
  `接続`（主 CTA）/ `スキャン`（副）+ 3 ステップ（USB を抜く → Profile を 3 秒
  長押し → 80ND を選んで接続）+ スキャン結果のデバイス行リストを表示する
- **接続管理**: BLE スキャン（`scan_devices`）、`80ND` への接続、切断、状態表示。
  `切断` は接続中だけ有効。左レールの状態とデバイス情報も同期する
- **デバイス情報**: 名前、アドレス、MTU、使用中の GATT characteristic（`FF11` / `FF13`）
- **ファームウェア**: 接続直後に `ReadVersion` を実行し、raw 4 バイト・text・LE `u32` を表示
- **ライブ入力**: 接続直後に `0x0007` を有効化し、公式 20ms 周期の `0x0003` ポーリングで
  スティック、トリガー、ボタン、バッテリー raw 値を表示（Stop も可能）
- **設定ビューア**: 2768B 設定の profile flag、`gamepad_mode`、`cur_slot`、
  プロファイル名、3 プロファイル × 4 マクロのヘッダ（name / max_steps / offset /
  key_map / cycles / interval など）を読み取り専用で表示
- **バックアップ**: 2768B の raw と JSON サマリを保存し、保存先と SHA-256 を表示
- **エラー表示**: 失敗は上部の 44px バナー（左に赤バー + `閉じる`）とログタブに表示。
  技術的な詳細はバナー/ログ行の hover に分離し、GUI スレッドは停止しない
- **状態とアクセシビリティ**: 状態ピル（未接続/スキャン中/接続中/接続済み）、
  無効化理由の表示、無効時は背景なし + 枠線のボタン、フォーカスリング、
  全ボタン・行・タブへの AccessKit ラベル

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
crates/eightbitdo-gui/
  assets/fonts/            Noto Sans JP のサブセット（Regular/Bold）+ OFL.txt
  src/
    main.rs    eframe の起動だけを行う薄いエントリポイント（窓タイトルは日本語）
    app.rs     egui の画面。UI スレッドで .await しない
    theme.rs   色・余白・角丸・タイポグラフィのトークンとフォント登録
    i18n.rs    全 UI 文言（日本語既定、英語フォールバック）と表示言語
    worker.rs  BLE ワーカースレッド。tokio ランタイムと Command/Event チャネル
    driver.rs  Session を 1 本だけ保持する読み取りドライバ
    model.rs   2768B / 入力レポートの表示用モデルと整形
    backup.rs  raw + JSON の安全な新規作成と SHA-256
```

- UI スレッドは `Command` を送り、`Event` を毎フレーム drain するだけです。
- `worker.rs` の `Event::Error` / `Event::Log` / 切断理由は「翻訳キー + 技術詳細」
  に構造化されています。表示言語を切り替えても、過去のログが現在の言語で
  再描画されます。技術詳細（`eightbitdo-core` 由来の英語）は翻訳せず、
  バナー/ログの hover とログ行にそのまま残します。
- BLE ワーカーは専用スレッドの tokio ランタイム上で動き、`Session` の
  single-flight 契約を守るため、読み取りと入力ポーリングを同時に実行しません。
- ライブ入力中の 20ms `0x0003` ポーリングが keepalive を兼ねます。接続直後の
  version / config 読み取りが終わると自動で開始します。現時点の
  `eightbitdo-core` に session 所有の keepalive はないため、CLI の
  `run_keepalive` と同じ方式を GUI ワーカー側に置いています。Stop すると
  ポーリングも止まるため、設定モードが失効する可能性があります。
- 連続 3 回の入力失敗、または `Disconnected` でリンクを閉じ、理由をログに残します。
  自動再接続はせず、ユーザーが Connect で再接続します。

## デザインと日本語化

- **テーマ**: 背景 `#121212` / パネル `#1C1C1C` / カード `#262626`、
  境界線 `#454545`、アクセント `#0067C0`、角丸 6px。本文・補助・無効文字は
  パネル上で WCAG AA（4.5:1）以上。コントラストは `theme.rs` の純関数テストで
  固定しています。
- **タイポグラフィ**: 14 / 16 / 20 / 24 px の 4 段。見出しとセクションラベルは
  同梱 Bold、本文は Regular、hex/数値は Hack + JP フォールバックの等幅。
- **i18n**: `enum Key` の全キーを 1 か所で定義し、日本語と英語の `match` を
  同じ定義から生成するため、訳抜けはコンパイルエラーになります。プレースホルダ
  数が両言語で一致すること、英語フォールバックに日本語が混ざらないことを
  テストしています。
- **フォント**: Noto Sans CJK JP（OFL 1.1）を JIS X 0208 第 1・2 水準 +
  UI 文言へサブセット化。Regular 1.48 MiB / Bold 0.11 MiB。同梱物と生成手順は
  [`assets/fonts/README.md`](assets/fonts/README.md)、ライセンス本文は
  [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt)。

## 検証

```bash
cargo check -p eightbitdo-gui
cargo test -p eightbitdo-gui
cargo clippy -p eightbitdo-gui --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

単体テストは GUI 表示環境を必要としません（49 件）。モックトランスポートに加えて、
UI 層を以下のように検証しています。

- `ReadVersion` の応答デコードと `0x000B` 要求
- ReadConfig の 20 チャンク組み立て、SHA-256、`0x0002` 要求
- `0x0003` 入力レポートのデコードと送信要求
- 切断時の fail-closed エラー
- バックアップの raw / JSON / SHA-256 / 0600 / 上書き禁止
- 2768B 設定からマクロヘッダへの変換、日本語プロファイル名/マクロ名の非エスケープ
- i18n: 全キーの日英非空・プレースホルダ数一致、既定日本語、英語フォールバック、
  keymap/モード/プロファイル表記のローカライズ
- theme: 4 段タイポグラフィ、WCAG AA コントラスト（通常・補助・無効・アクセント）、
  境界線の可視性、8px グリッド、同梱フォントの日本語グリフ被覆
- app: セッション警告条件、オンボーディング表示条件、RSSI バー量子化、
  接続対象の App Mode 判定、レイアウト数値、ヘッドレス egui での全タブ描画
  （未接続 / 古いデータ / 接続済み、エラーバナー、スキャン行を含む）

この worktree では **実機接続を行っていません**。スキャン、接続、ライブ入力、
実機からのバックアップはハードウェア未検証です。GUI の起動確認とスクリーンショット
取得もこの worktree では実施していません。

## Phase 3b への申し送り

- マクロ 3 スロット × 4 本の Flash バックアップは、core にマクロ読み出し API が
  入ってから追加する（PR #6 / Phase 2b の後）。
- マクロ編集 UI、録音、JSON/TOML 入出力は Phase 3b。書き込み系を追加する際は
  Header (`total_len=0xD8`) → Save → Erase → Write → Save の公式順序と、
  書き込み前バックアップを必須にする。
- 設定書き込み（Phase 3d）と OTA 画面（Phase 3c）は、それぞれ core の write path
  と OTA API が安定してから着手する。
- 日本語表示は同梱フォントで完結する。JIS X 0208 外の漢字（人名の異体字など）は
  tofu になるため、必要になったら `assets/fonts/build_fonts.py` の文字集合を
  拡張して再生成する。
- 言語選択はメモリ上のみ（既定は日本語）。永続化する場合は eframe の
  `persistence` か `dirs::config_dir()` の設定ファイルを使う。デバイスへの
  書き込みではないため読み取り専用制約には反しない。
- ライトテーマ / OS テーマ追従、アプリアイコン、言語切替の永続化、
  バックアップ履歴、ログのレベルフィルタは未実装（DESIGN_REVIEW の Could）。
- workspace は `resolver = "3"` に変更し、`rust-version = 1.85` を守る範囲で
  eframe の依存を解決している。GUI を追加しても CI の `msrv` は 1.85 のまま。
  この変更で `Cargo.lock` の既存解決も一部変わる（`hashbrown` 0.14.5 → 0.17.2、
  `redox_syscall` 0.5.18 → 0.9.5）。全 workspace の test / clippy / MSRV 1.85 で
  確認済み。
