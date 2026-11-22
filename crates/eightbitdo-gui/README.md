# eightbitdo-gui（Phase 3a + 3b / 設定・マクロ書き込み対応）

8BitDo Ultimate 2 Bluetooth Controller（BLE App Mode `"80ND"`）を操作する
Pure Rust デスクトップ GUI です。egui / eframe を使い、Node.js・WebView・
Electron・Tauri は使用していません。

**書き込みは明示的な確認ダイアログの後だけ実行します。** 書き込みの直前に
設定 2768 バイト（`prewrite-config-*.bin` + JSON）またはマクロ 3 スロット ×
4 本（`macro-prewrite-*/`）のバックアップを必ず作成し、SHA-256 を表示します。
書き込み後はコントローラから read-back して照合し、成功・失敗・復旧手順
（バックアップのパスを含む）を表示します。接続が切れている間は書き込めません。
ファームウェア更新（OTA）とバックアップからのリストアは GUI にはまだありません。

UI は日本語が既定です（トップバーの `日本語 | English` で即時切り替え）。
日本語表示のため Noto Sans JP のサブセットを同梱しており、システムフォントに
依存しません。フォントの出所・ライセンス・再生成方法は
[`assets/fonts/README.md`](assets/fonts/README.md) を参照してください。

## できること

- **主導線**: 未接続時は中央を非対称 2 カラムにする。左に消灯したリング
  （径 176px、縦センタリング）と中心のオフホワイト主キー `80ND に接続`、右に
  「接続の手順」の 3 行と `スキャン`。下にデバイス一覧（行 44px、RSSI バー、
  App Mode バッジ）を置く。スキャン中はアンバーの円弧が回り、一覧はスケルトン行になる
- **接続管理**: BLE スキャン（`scan_devices`）、`80ND` への接続、切断、状態表示。
  状態は上部バーに 1 つだけ出し、左レールは「デバイス」として名前・アドレス・MTU を
  表示する。`切断` は接続中だけ有効
- **デバイス情報**: 名前、アドレス、MTU、使用中の GATT characteristic（`FF11` / `FF13`）
- **ファームウェア**: 接続直後に `ReadVersion` を実行し、raw 4 バイト・text・LE `u32` を表示
- **ライブ入力**: 接続直後に `0x0007` を有効化し、公式 20ms 周期の `0x0003` ポーリングで
  スティックを LED のリングと軌跡、トリガーを細いバー、ボタンを静かなバッジで表示する
- **設定値ビューア**: 2768B 設定の profile flag、`gamepad_mode`、`cur_slot`、
  プロファイル名、3 プロファイル × 4 マクロのヘッダ（name / max_steps / offset /
  key_map / cycles / interval など）を表示
- **設定の編集と書き込み**: プロファイル名、`gamepad_mode`、有効プロファイル、
  振動（左右モーター）、スティック / トリガーのデッドゾーン、特殊機能、
  6 軸ジャイロ、3 種類の LED テーマ、ボタン割り当てを編集する。変更は
  「変更を確認」→確認ダイアログ→書込→read-back 検証→結果表示の順でだけ反映する。
  `--force-raw` のような危険な上書き経路は GUI に置いていない
- **ボタン割り当て（リマップ）**: 22 個の物理ボタンそれぞれに単一の文書化済み
  キーを選ぶ。`227`（`MY_NULL`）、未知ビット、複合マスクは選択肢に出さない。
  `MapKeyRecord.flag` は `0` / `1` のみ書き込む
- **マクロ編集**: マクロを読み取り、ライブ入力（20 ms レポート）を録音して
  ステップ一覧（時間・キー・スティック・トリガー）を作る。ステップの削除、
  待ち時間の挿入、値の変更ができ、既存の非空マクロへ
  Header → Save → Erase → Write → Save の順で書き込む。空マクロの新規作成は
  拒否する。`total_len` は常に絶対値（セクタ基準 + 32 バイト整列）
- **バックアップ**: 2768B の raw と JSON サマリを保存し、保存先と SHA-256 を表示。
  書き込み前バックアップは `prewrite-config-*` と `macro-prewrite-*` に分離する
- **ログ**: タブ行の右端に `ログを消す`、行高を 28px に固定。空状態は
  「まだ記録がありません。接続や読み取りの結果がここに残ります。」を表示する
- **エラー表示**: 操作した場所の直上に、赤いヘッドライン + `詳細`（折りたたみ）+
  `閉じる` を出す。技術的な詳細は英語のまま分離し、GUI スレッドは停止しない
- **状態とアクセシビリティ**: 消灯リング / アンバー円弧 / アンバー点 / LED + グローで
  未接続・スキャン中・接続中・接続済みを表す。状態表示は上部バーの 1 か所だけ。
  無効時は枠を消して `TEXT_DISABLED` にし、理由を 1 行上に出す。
  フォーカスリングは LED 2px + 外側 BG 1px。主キー・副キー・タブ・行は 32px 以上の
  ヒット領域を持つ

## 起動

```bash
cargo run -p eightbitdo-gui
```

初期ウィンドウは 1120×800、最小は 900×620 です。Linux では X11 または Wayland、
OpenGL、BlueZ / D-Bus が必要です。
`btleplug` は BlueZ を使用します。CI と同様に、Debian/Ubuntu では
`libdbus-1-dev` と `pkg-config` を入れてください。Wayland は `wayland-dlopen`
付きの winit を eframe の既定 feature で使用します。

## バックアップの保存先

`dirs::data_dir()` 配下の `8bitdo-backups/` です。書き込み前バックアップも
同じ場所に作ります。マクロの書き込み前バックアップは CLI の `macro backup` と
同じ `config.bin` / `config.json` / `macros.json` / `slotS-macroM.bin` 構成なので、
`eightbitdo macro restore --dir <パス>` で復元できます。

- Linux: `$XDG_DATA_HOME/8bitdo-backups`、未設定なら
  `~/.local/share/8bitdo-backups`
- macOS: `~/Library/Application Support/8bitdo-backups`
- Windows: `%APPDATA%\8bitdo-backups`

既存 CLI の `backup` は出力パスを明示する設計で、既定ディレクトリは持っていません。
GUI 側の既定値として上記を採用しています。

手動バックアップのファイル名は `config-YYYYMMDD-HHMMSS.bin` / `.json`、書き込み前の
設定バックアップは `prewrite-config-YYYYMMDD-HHMMSS.bin` / `.json` です。同名ファイルがある場合は
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
  assets/fonts/            Noto Sans JP のサブセット（Regular/Medium/Bold）+ OFL.txt
  src/
    main.rs    eframe の起動だけを行う薄いエントリポイント（窓タイトルは日本語）
    app.rs     egui の画面。UI スレッドで .await しない
    theme.rs   色・余白・角丸・タイポグラフィのトークンとフォント登録
    i18n.rs    全 UI 文言（日本語既定、英語フォールバック）と表示言語
    worker.rs  BLE ワーカースレッド。tokio ランタイムと Command/Event チャネル
    driver.rs  Session を 1 本だけ保持する読み取りドライバ
    model.rs   2768B / 入力レポートの表示用モデルと整形
    backup.rs  raw + JSON の安全な新規作成と SHA-256、CLI 互換のマクロバックアップ
    write.rs   設定 / リマップ / マクロ編集の純粋な状態機械と確認モデル
```

- UI スレッドは `Command` を送り、`Event` を毎フレーム drain するだけです。
- マクロ録音中は書き込みを確定できません（`can_write` が録音中を拒否し、確認
  ダイアログの確定キーも無効）。確認ダイアログは非モーダルなので、背景で録音を
  開始した場合も確定時に再検証します。録音中にマクロを読み込んだ場合は録音を
  停止し、ログに通知します。確認ダイアログを開いた後に設定値が更新された場合、
  読み込み済みマクロのヘッダが一致しなければ編集状態を破棄して読み直しを促します。
- 書き込みは worker が 1 セッションを保持したまま実行します。フラッシュ操作中は
  20 ms の `0x0003` ポーリング（keepalive）を `LiveState` で止め、書き込み後に
  再開します。CLI の operation lock と同じ目的で、書き込みの ACK とポーリングを
  競合させません。ライブ入力が停止している場合、書き込みは keepalive なしで
  実行します（CLI の polling 無効時と同じ。書込中に `0x0007` を有効化しません）。
  書き込み前に pre-write バックアップ、書き込み後に read-back 照合を必ず行います。
  失敗時は `Event::WriteFailed` がバックアップのパス・SHA-256・復旧手順を運びます
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

方向性は **「燻しスレート — 実機の素材と、ひとつの光」**。燻した半透明の筐体、
刻印されたラベル、暗い部屋で一点だけ点く LED を画面の語彙にする。色を足さず、
光の意味を「点いている＝つながっている」の 1 つに固定する。

- **面**: `BG #0B0D10` / `SURFACE_1 #111419` / `SURFACE_2 #171B21` /
  `SURFACE_3 #1F242B`。影は使わず、面の明度差と上端 1px の `EDGE_LIGHT`
  （`#333A44`）で前後を作る。構造線は `HAIRLINE #2B3139`、押せるものの輪郭だけ
  `CONTROL_LINE #666F7B` に分ける
- **光**: `LED #6FE7D8` は接続・選択・フォーカス・スティック位置だけ。
  `LED_DIM` は消灯リング、`AMBER` はスキャン中と古いデータ、`RED` はエラー。
  主キーは光らせず、オフホワイトのキー（`#E9ECEF` 地 + `#0B0D10` 文字）
- **タイポグラフィ**: 22 Bold（ページタイトル）、12 Medium + 字間 0.5px
  （セクション）、14 Regular + 行高 24px（本文）、14 Medium（操作）、
  12 Mono + 字間 0.25px（数値・アドレス）、12 Regular（補足）。`theme::text_job()`
  で `TextFormat` の `extra_letter_spacing` と `line_height` を統一する
- **構図**: 左レール 264px、上部バー 56px、タブ行 40px。本文は最大 700px の
  1 列にし、右端に縦ヘアラインを引いて 80pt 以上の余白に境界を与える。未接続・
  空状態は左リング + 右「接続の手順／次の一手」の非対称 2 カラム、カードは
  1 画面 1 枚まで。内容列の下端には 12px のメタ行を置き、高窓でも余白が
  「未実装」に見えないようにする
- **操作は 3 種類**: 主キー（オフホワイト、高 40）、副キー（地は透明で hover 時だけ
  置かれた面から `SURFACE_2` へフェード + 輪郭 3:1、高 40）、静かな行（枠なし、高 36、
  hover で `SURFACE_2`、選択は `SELECT_TINT` + 左 2px LED）。無効時は輪郭を消し、
  文字を `TEXT_DISABLED` にして理由を 1 行上に出す。角丸は
  12（カード）/ 8（キー）/ 6（タブ）/ 4（バッジ）の 4 種のみ
- **動き**: hover 100ms、タブ下線 180ms、点灯グロー 300ms、スキャン中の円弧は
  1.5 秒で 1 周。`motion_scale` で 0 にできる（UI 設定は未実装）
- **コントラスト**: 本文・補足・無効文字は全サーフェスで AA 4.5:1 以上。
  押せるものの輪郭とフォーカスリングは SC 1.4.11 の 3:1 以上。`theme.rs` の
  純関数テストで固定している。brief の `CONTROL_LINE #616B77` はカード上で
  2.88:1、`TEXT_DISABLED #7C8490` はカード上で 4.13:1 になるため、それぞれ
  1 段明るい値に調整した
- **i18n**: `enum Key` の全キーを 1 か所で定義し、日本語と英語の `match` を
  同じ定義から生成するため、訳抜けはコンパイルエラーになります。プレースホルダ
  数が両言語で一致すること、英語フォールバックに日本語が混ざらないこと、
  ラベルが 12 文字以内で句点・全角コロンを持たないことをテストしている
- **フォント**: Noto Sans CJK JP（OFL 1.1）を JIS X 0208 第 1・2 水準 +
  UI 文言へサブセット化。Regular 1.48 MiB / Medium 0.11 MiB / Bold 0.11 MiB。
  同梱物と生成手順は [`assets/fonts/README.md`](assets/fonts/README.md)、
  ライセンス本文は [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt)。

## 検証

```bash
cargo check -p eightbitdo-gui
cargo test -p eightbitdo-gui
cargo clippy -p eightbitdo-gui --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

単体テストは GUI 表示環境を必要としません（88 件）。モックトランスポートに加えて、
UI 層を以下のように検証しています。

- `ReadVersion` の応答デコードと `0x000B` 要求
- ReadConfig の 20 チャンク組み立て、SHA-256、`0x0002` 要求
- `0x0003` 入力レポートのデコードと送信要求
- 切断時の fail-closed エラー
- 設定書き込み: 事前読み取り → pre-write バックアップ（raw / JSON / SHA-256）→
  `0x0001` チャンク + Save → read-back 照合。no-op はバックアップ前に拒否、
  バックアップ失敗時は書き込みフレームを 1 つも送らない、切断時は backup 参照付きで
  fail-closed になることを固定
- マクロ書き込み: 設定 + 全 12 マクロの読み取り → CLI 互換バックアップ →
  Header → Save → Erase → Write → Save → read-back。空マクロはバックアップ前に拒否
- TOCTOU: 確認ダイアログを開いた時点の設定 SHA-256（マクロは対象ステップの
  SHA-256 も）を保持し、書込直前の fresh read と不一致なら 1 フレームも送らず
  `ConfigChanged` で中止する。バックアップも作成しない
- worker: 書き込み中の keepalive 停止 / 再開、書き込み前バックアップイベントが
  結果より先に出ること、失敗フェーズごとの見出しキー
- write: 設定変更の差分（名前・モード・振動・リマップ）、名前の 16 UTF-16 制限、
  227 / 未知ビット / 複合の選択禁止、flag 0/1、マクロ録音の 20 ms 結合と 200 ステップ
  上限、空マクロ・0 ms ステップの拒否、削除・待ち時間挿入、確認モデル
- 実機パターンの回帰: 未使用 slot の全 key `0xFFFFFFFF` を未変更のまま保持して設定 plan を
  作れること、未変更の複合 / 227 マクロ trigger を保持して書けること、確認後の
  設定 / マクロ変更を `ConfigChanged` で中止すること
- app: 書き込み結果 / 失敗イベントの状態遷移、確認ダイアログを開く条件、
  切断中の書込不可、録音中の書込不可（確認キーも無効）、録音中のマクロ読み込みで
  停止通知が出ること、設定更新後の stale なマクロ baseline の破棄、マクロ読み込み後の
  編集状態、ライブ入力停止時の録音停止
- 2768B 設定からマクロヘッダへの変換、日本語プロファイル名/マクロ名の非エスケープ
- i18n: 全キーの日英非空・プレースホルダ数一致、既定日本語、英語フォールバック、
  ラベルの長さと句読点、keymap/モード/プロファイル表記のローカライズ
- theme: 3 ウェイトの書体階層、`text_job` の行高・字間、全サーフェスの AA、
  輪郭とフォーカスリングの 3:1、面の明度順、4 種の角丸、8px グリッド、
  同梱フォントの全 UI 文字被覆
- app: セッション警告条件、オンボーディング表示条件、RSSI バー量子化、
  接続対象の App Mode 判定、スティック軌跡の 8 点上限、レイアウト数値、
  ヘッドレス egui での全タブ描画（未接続 / スキャン中 / 古いデータ / 接続済み、
  エラーバナー、スキャン行、ログ行を含む）。描画後のテキスト形状から、次を検査する:
  タブが 1 行に並ぶこと、`スキャン` キーと 3 手順がカード内に収まること、
  手順番号が `TEXT_MUTED` で `LED_DIM` の文字が存在しないこと、状態名が
  上部バーに 1 回だけ出ること、ログ空状態と `ログを消す` の位置、下端メタ行、
  本文が 700px の内容列に収まること

この worktree では **実機接続を行っていません**。スキャン、接続、ライブ入力、
実機からのバックアップ、設定/マクロの書き込みと read-back はハードウェア未検証です。
GUI の起動確認とスクリーンショット取得もこの worktree では実施していません。

## 残課題

- 実機での書き込み検証（特に BLE の Save ACK、read-back、マクロの絶対
  `total_len`）。モックテストはパケット列・CRC・順序を固定するが、実機の
  ACK タイミングやデバイス側の正規化は未確認。
- マクロの JSON/TOML インポート・エクスポート UI は未実装（core の `MacroFile` は
  利用可能）。
- バックアップからの GUI リストアは未実装。復旧は CLI の
  `eightbitdo macro restore --dir <パス>` と設定 raw の書き込みを使う。
- OTA（ファームウェア更新）画面は未実装。
- 日本語表示は同梱フォントで完結する。JIS X 0208 外の漢字（人名の異体字など）は
  tofu になるため、必要になったら `assets/fonts/build_fonts.py` の文字集合を
  拡張して再生成する。
- 言語選択はメモリ上のみ（既定は日本語）。永続化する場合は eframe の
  `persistence` か `dirs::config_dir()` の設定ファイルを使う。
- ライトテーマ / OS テーマ追従、アプリアイコン、言語切替の永続化、
  バックアップ履歴、ログのレベルフィルタは未実装（DESIGN_REVIEW の Could）。
- reduced motion は `App::motion_scale` で 0 にできるが、設定 UI は未実装。
- エラーバナーの「もう一度読み取る」は、エラー種別ごとに対象操作が異なるため
  今回は置いていない。再試行は左レールの各操作から行う。
- 設定値タブの 2 列グリッド、コントローラの 1 枚絵、操作箇所ごとのエラー表示は
  Could のため未実装。
- workspace は `resolver = "3"` に変更し、`rust-version = 1.85` を守る範囲で
  eframe の依存を解決している。GUI を追加しても CI の `msrv` は 1.85 のまま。
  この変更で `Cargo.lock` の既存解決も一部変わる（`hashbrown` 0.14.5 → 0.17.2、
  `redox_syscall` 0.5.18 → 0.9.5）。全 workspace の test / clippy / MSRV 1.85 で
  確認済み。
