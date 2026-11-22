# 8BitDo Ultimate 2 Cross-Platform Suite

[![CI](https://github.com/haruki0420/8bitdo-ultimate2-suite/actions/workflows/ci.yml/badge.svg)](https://github.com/haruki0420/8bitdo-ultimate2-suite/actions/workflows/ci.yml)

**8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）向けのオープンソース・クロスプラットフォーム設定／マクロ編集／ファームウェアアップデートスイート（設計・仕様リポジトリ）。

Nintendo Switch へ Bluetooth Classic または 2.4G アダプターで接続したまま、**Linux / macOS / Windows / Android / iOS** から **BLE GATT（アドバタイズ名 `"80ND"`）** 経由で同時接続し、リアルタイムにマクロやプロファイルを編集・適用することを目指しています。

> **非公式プロジェクト**: 本プロジェクトは 8BitDo（Shenzhen Bestodo Tech Co., Ltd.）と提携・承認・後援関係にない、独立した非公式オープンソースプロジェクトです。公式のバイナリ・ファームウェア・ロゴは含みません。商標と非提携の扱いは [TRADEMARKS.md](TRADEMARKS.md) を参照してください。

---

## 主な特徴（設計目標）

- **Switch 同時接続でのリアルタイム設定**: コントローラを Switch に繋いだまま Profile ボタンを 3 秒長押しし、PC やスマホから BLE GATT で接続・編集可能（USB / 2.4G HID 接続にも対応）。
- **高精度マクロエディタ**:
  - コントローラ実機入力からのリアルタイム録音（`cmd 0x0007` / `0x0003`）
  - **1ms 単位**のステップ時間編集、フル **8-bit アナログスティック／トリガー値**指定
  - JSON / TOML 形式でのマクロ・プロファイル インポート／エクスポート
- **2768 バイト (`0xAD0`) 設定構造体のフルサポート**:
  - プロファイルスロット 1〜3、ボタンリマップ（22 キー + 拡張 32-bit `KeyMap`）、スティック＆トリガーデッドゾーン、振動、6軸ジャイロ、RGB ライティング（Dynamic / HotWheel / Single）、マクロ
- **PC & モバイル対応ファームウェアアップデート**:
  - PC（BLE / USB）だけでなく、**Android / iOS からの BLE Bootloader OTA（`"8bt"`, `0000FF20`/`0000FF21`）** および Android USB OTG 更新に対応
- **クロスプラットフォーム・ネイティブ構成**:
  - 共通プロトコル＆コア: **Rust** (`eightbitdo-protocol`, `eightbitdo-core`, `eightbitdo-uniffi`)
  - PC (Linux / macOS / Windows): Pure Rust CLI + ネイティブ GUI（`egui`、`btleplug` + `hidapi`）
  - モバイル: **iOS (SwiftUI + CoreBluetooth)** / **Android (Jetpack Compose + Android BLE / USB OTG)**

---

## CLI（実装済みの設定書き込み・BLE のみ）

`eightbitdo config` は full 2768B 設定の書込みを扱います（USB HID config write は未実装）。

- `config dump [--raw FILE] [--json FILE]` — 読取のみ。
- `config write --file <2768B>` — `config dump --raw` の編集ファイルを書込み。予約バイトの改変は拒否し、復旧時のみ `--accept-reserved-drift` で device の現在値に合わせる。
- `config set <field> <value>` — プロファイル名、cur_slot、gamepad_mode、rumble / joy / trigger / gyro / LED、`map_key.flag`。範囲外は I/O 前に拒否。joy / trigger はレコード flag=1、LED color はテーマ flag=1 の副作用あり（help に明記）。
- `config remap <physical> <target>` — 1 ボタンのみ（4B）変更。`config map show` で 22 index を表示。
- 安全策: **pre-write backup 必須**（`config.bin` + `config.json`）、`--yes` なしの確認、前後 SHA-256、確認後の TOCTOU 再読込、read-back 検証、マクロヘッダ変更警告。書込 chunk は既定 `min(MTU, 74)`（RE: 公式 74B）で、`--write-chunk` は接続前上限 **180**（観測 MTU 200 のペイロード上限）まで指定可能。
- raw `config write` の検証: 22 個の `button_mappings`、`gyro.key_map`、全レコードの flag 0/1、マクロ `max_steps`（0/0xFFFF/1..=200）・`total_cnt`、マクロ `key_map`（`macro write --trigger` と同じ任意 KeyMap）を確認。不明な `cur_slot`（実機観測 6 など）は警告して保持。`--force-raw` は未文書値の復旧用（予約領域ガードは維持）。
- `session --allow-config-write` の REPL からも `config set` / `config remap` / `config write --file` を実行できる（`config map show` と `config dump` はフラグ不要の read-only）。書込は `--yes` で対話確認なし、無い場合は `yes` 入力の確認、pre-write backup、前後 SHA-256、read-back 検証付き。確認待ちの間は operation lock を解放して 20ms keepalive を継続し、確認プロンプトは Ctrl-C で中止できる（pre-write backup は残る）。実書込（最初の `0x0001` 以降）だけが operation lock 下の Ctrl-C 非キャンセル区間で、書込後は keepalive が再開する。
- 既知の差: 公式 BLE Save は 2000ms / 最大 2 再送（静的解析）だが、本実装は 3 秒 / リトライなし。USB config write は未実装。
- BLE 競合・degraded 対策: `scan` / `connect` / `session` と `BleTransport::connect` を使う全 CLI コマンド（info / config / macro / backup / monitor / keepalive）は、BLE を触る前に **per-user** 単一所有者 flock を取得する。パスは環境非依存で、`EIGHTBITDO_BLE_LOCK`（テスト/ラッパー用の信頼された例外）→ NSS（`getpwuid_r`）の euid ホーム由来の `~/.cache/8bitdo/8bitdo-ble.lock` → ホーム不明時のみ temp の per-user dir。**XDG_RUNTIME_DIR / XDG_CACHE_HOME / `$HOME` は参照しない**ので、デスクトップ/SSH/cron/systemd で同じユーザーなら同じロックになる。`O_NOFOLLOW` と fd メタデータ検証で symlink / 非通常ファイル / 他ユーザー所有を拒否し、既定パスでは既存 parent が euid 所有かつ group/other write なし・**symlink でない**ことを検証し、Unix は dirfd（`openat`）で TOCTOU も閉じる。取得できなければ「別プロセスが BLE を使用中」として失敗する（`session` は接続保持中ロックを保持し、teardown 成功時のみ解放）。別ユーザー同士は排他しない。`session` の keepalive が **I/O エラー連続 3 回**（無応答は failure に数えない）/ `Disconnected` で degraded になると終了コード **3** で終了し、自動再接続しない（コマンドが観測した `Disconnected` は exit 1）。切断と同時に App Mode を抜けて `80ND` が広告されなくなるため、再開には Profile 約 3 秒押しが必要である旨を表示する。SIGINT は範囲に応じて警告する（REPL 終了 0 / connect 前・info/config/macro 130 / backup・monitor・keepalive は全体を 130 / flash は prepare（ファイル読込・backup・確認プロンプト）が**キャンセル可**で、最初の flash フレーム以降だけ**遅延**して完了後に復旧案内 + 130）。`scan --check` は 1 行出力し、exit 0 = App Mode あり / 3 = なし / その他 = エラーで、人間向け出力の grep は不要。常駐運用は `scripts/session-loop.sh`（exit 0 では最低 2 秒待ってから `scan --check` し、見えれば指数 backoff で再起動、見えなければ押下案内して停止。bash 4 + GNU date 必須、macOS 非対応）。押下を依頼する前に `scan` で `80ND` を確認する手順は [docs/SESSION_OPERATIONS.md](docs/SESSION_OPERATIONS.md) を参照。
- 互換性（pre-1.0）: `eightbitdo-core::BleOptions` に `lock_path: Option<PathBuf>` を追加（構造体リテラル構築側は `lock_path: None` が必要）。既定ロックパスは `/tmp` 直下から per-user ディレクトリに変更。
- REPL の引数は空白区切り。スペースを含むプロファイル名は top-level `config set profile_name "..."`（シェル引用）を使う。

---

## ドキュメント一覧

公式バイナリの静的解析から検証した完全仕様と実装計画を `docs/` にまとめています。

| ドキュメント | 内容 |
| --- | --- |
| **[docs/PROTOCOL_SPEC.md](docs/PROTOCOL_SPEC.md)** | デバイス VID/PID、BLE GATT UUID・MTU、16B カスタムヘッダ、BLE/USB HID フレーミング、2 種の CRC-16、全コマンド、リアルタイム入力ストリーム、BLE OTA ブートローダー＆ファームウェア API 仕様 |
| **[docs/STRUCT_BT2_2768B.md](docs/STRUCT_BT2_2768B.md)** | 2768 バイト（`0xAD0`）設定構造体 (`record_ultimatebt2`) の全バイトオフセット、32-bit `KeyMap`、10 バイト `MacroDatas` ステップ構造、Flash 書き込みシーケンス |
| **[docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)** | Rust + SwiftUI + Jetpack Compose のアーキテクチャ設計、ワークスペース構成、Phase 1〜5 ロードマップ、安全検証手順 |
| **[docs/SESSION_OPERATIONS.md](docs/SESSION_OPERATIONS.md)** | 常駐 `session` の運用（per-user BLE 単一所有者ロック、degraded 終了コード 3、SIGINT 範囲、押下依頼前の `scan` 確認手順、`scripts/session-loop.sh` の scan 契約と backoff） |
| **[AGENTS.md](AGENTS.md)** | AI エージェント／開発者向け引き継ぎガイド（[CLAUDE.md](CLAUDE.md) からも参照） |
| **[CONTRIBUTING.md](CONTRIBUTING.md)** | ブランチ運用、必須 CI、独立レビュー（Tier 1〜3 / SHA 紐づきの `review-gate` 成功）の手順 |
| **[docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md)** | `main` の canonical ruleset、`review-gate` の付け方・監査とゲートの限界 |
| **[docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)** | リスクに応じた独立レビューの Tier 1〜3、レビュアーの役割、故障リストの書き方 |
| **[LICENSE-MIT](LICENSE-MIT)** / **[LICENSE-APACHE](LICENSE-APACHE)** | コードのライセンス全文（MIT / Apache-2.0） |
| **[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)** | 依存ライブラリのライセンス一覧、帰属表示、同梱フォント（OFL）の扱い |
| **[TRADEMARKS.md](TRADEMARKS.md)** | 第三者の商標、非公式・非提携、ロゴ不使用の方針 |

---

## 現在のステータス

- `main` は **PR + CI + 独立レビュー（SHA 紐づきの `review-gate` 成功）** で保護済みです（`main-protection`, id `27785806`, 2026-11-21 検証）。レビュー深度は [docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)、詳細は [CONTRIBUTING.md](CONTRIBUTING.md) と [docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md) を参照してください。
- [x] 公式アプリ（Android v2.01 / Windows v1.38）の通信プロトコル・バイナリ構造・OTA シーケンスの解析完了
- [x] 完全仕様書・実装計画書・AI 引き継ぎドキュメント作成
- [x] Phase 1: `eightbitdo-protocol`（Rust プロトコル・構造体・CRC・マクロ、単体テスト 74 件 + doc test 4 件）
- [ ] Phase 2: `eightbitdo-core` + `eightbitdo-cli`（BLE 実機通信＆検証）
  - [ ] Phase 2a: 読み取り専用 core / CLI / `session`（`eightbitdo session [--interval-ms 20] [--allow-macro-write] [--no-report-enable]` の REPL は `info` / `config dump` / `backup` / `monitor` / `monitor stop` / `status` / `help` / `quit`。REPL には `macro backup <DIR>`（読み取りのみ、常に利用可）/ `macro write <SLOT> <INDEX> <FILE> [--trigger <KEY>] [--chunk-size <32|64>]` / `macro restore <DIR> [--force]` もあり、書込系は `--allow-macro-write`（既定 OFF）が必要。`session` は既定で起動時に `0x0007 enable(1)` を送る（公式相当）ため REPL の `macro write` は enable 状態で動き（トップレベル `macro write` の既定とは条件が異なる）、`--no-report-enable` 指定時は起動時の enable(1) を送らず `0x0003` keepalive のみを継続する（終了時の disable(0) は best-effort。トップレベル既定と同じ書込条件で A/B するため）。`--allow-macro-write` 時の `macro write` / `macro restore` は、ファイル読込・pre-write/pre-restore backup を含むコマンド全体で Ctrl-C を遅延する（非通常ファイルは読取前に拒否し、デバイス I/O は timeout で有界。SIGKILL・電源断は防げない）。REPL の `macro write` は `--backup-dir`（既定名 `macro-backup-<unix-nanos>-<pid>-<sequence>`）、`macro restore` は `--chunk-size` を指定できず、REPL 経由の実機書込は未検証。設定バックアップは `--raw` / `--json` を同一ディレクトリの一時ファイル経由でアトミックに書き、同一ファイルを指す 2 つの出力パスは拒否する。ただし hard link の同一性検出は Unix のみで、Windows は file identity 未実装のため hard link alias を事前拒否せず、raw 書き込み後の再検証と `rename` semantics で保護する。この再検証は偶発的な alias 用であり、出力ディレクトリに write 権限を持つ攻撃者による TOCTOU / 並行 rename は非保証である）
  - [ ] Phase 2b: マクロ読み書き（実装完了・実機待ち）
  - [ ] Phase 2c: BLE OTA（Linux。core 実装済み・Mock 48 テスト。**単一セクション `.dat` のみ対応**で公式 `.dat` がマルチセクションの場合は未対応（公式ファーム対応の blocker）、未確認 wire ゲート既定 OFF、boot 専用 `BleBootTransport` のみ許可、type 1 application ウィンドウを暫定検証、CLI/GUI 未配線、実機書込は保留。マルチセクション対応と実機 wire 確定は次 PR の必須項目）
  - [ ] Phase 2d: 設定書き込み（BLE のみ実装済み・Mock 検証。`config write` / `set` / `remap` / `map show`、pre-write backup、確認、前後 SHA-256、read-back 検証。USB config write は未実装）
- [ ] Phase 3: `eightbitdo-gui`（デスクトップネイティブ GUI）
- [ ] Phase 4: USB OTA（保留）
- [ ] Phase 5: `eightbitdo-uniffi` + iOS (SwiftUI) / Android (Jetpack Compose) アプリ

---

## 注意事項

- 本プロジェクトは非公式のオープンソースプロジェクトであり、8BitDo とは一切関係ありません。
- 著作権保護された公式ソフトウェアのバイナリ・ファームウェアファイル自体は本リポジトリに含まれていません。

---

## ライセンス

- **コード**（`crates/` 以下）: [MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のいずれかを選択できます（`MIT OR Apache-2.0`）。
- **ドキュメント**（`docs/` 以下と `README.md` など）: 特記がない限り、コードと同じ `MIT OR Apache-2.0` です。将来 CC BY 4.0 などの別ライセンスへ変更する場合は、変更時に本文へ明記します。ただし、第三者に由来する部分（同梱フォントの OFL、第三者コードの BSD / Unicode 表示、引用・出典）は、それぞれの権利者のライセンス条件に従います。
- **同梱フォント**（GUI を取り込む時点で同梱）: SIL Open Font License 1.1。`crates/eightbitdo-gui/assets/fonts/OFL.txt` を同梱し、GUI の About > Open Source Licenses などのライセンス画面から全文を参照できるようにします。
- 依存ソフトウェアのライセンスは [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) を参照してください。同ファイルは要約と参照であり、全クレートの著作権表示・ライセンス本文を網羅しません。バイナリ配布時は `cargo-about` 等で完全な第三者通知を生成して同梱してください。
- 商標・非公式に関する方針は [TRADEMARKS.md](TRADEMARKS.md) を参照してください。

## 免責

本プロジェクトは**非公式**の独立したオープンソースプロジェクトであり、8BitDo（Shenzhen Bestodo Tech Co., Ltd.）とは提携・承認・後援関係にありません。"8BitDo" は同社の登録商標です。"Ultimate 2" は同社の製品名です（単独の商標登録の有無は未確認）。本リポジトリは公式のバイナリ・ファームウェア・ロゴ等を含みません。

本ソフトウェアは無保証（"AS IS"）で提供されます。コントローラへの設定・ファームウェア書き込み・マクロ書き込みは、文鎮化・保証喪失・データ損失のリスクを伴います。自己責任で使用してください。
