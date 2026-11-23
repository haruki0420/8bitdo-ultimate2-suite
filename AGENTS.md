# AGENTS.md — AI エージェント向け引き継ぎガイド

このファイルは、どの AI エージェントや開発者が途中から参加しても、公式バイナリの再解析なしに即座に実装・検証へ進めるためのプロジェクト引き継ぎドキュメントです。

---

## 1. プロジェクト概要

- **目的**: **8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）を、Nintendo Switch と接続したまま **Linux / macOS / Windows / Android / iOS** から BLE GATT（および USB/2.4G HID）でフル設定・マクロ編集・ファームウェアアップデートできるクロスプラットフォーム OSS ツール群を構築する。
- **現在のフェーズ（2026-11-23、main `0af60a3`）**: **Phase 1・2a・2b（マクロ）・2d（設定書き込み / リマップ）・3a（読み取り専用 GUI）完了。PR #16 で GUI の Phase 3b（マクロエディタ）と Phase 3d（設定タブ / リマップ）を実装し main にマージ済みだが、GUI 書込は実機未検証・Tier 3。** Phase 2c は core 実装済みで、#20 で実機採取用の `ota-bench` 段階実行 API を追加（実機 wire 未確定・CLI / GUI / UniFFI 未配線）。Phase 3c（OTA 画面）は未着手。Phase 5 は UniFFI と iOS / Android の雛形に加え、#21 で **Android JNI 同梱 APK と iOS unsigned IPA のビルド成功まで記録**（実機未検証）。Phase 2e / 4（USB / 2.4G）は保留。**#17〜#26（実機 flag 対応 / クロスプラットフォーム確認 / OTA 採取計画 / `ota-bench` / モバイル証跡 / 配布方針 / 機能マトリクス / GUI ライセンス画面 #24 / 状態 docs 更新 #25 / プロファイル切替 #26）は main にマージ済み。GUI ライセンス画面（#24）はアプリの MIT / Apache-2.0 全文と同梱 5 フォント全文・著作権者行を表示し、プロファイル切替（`0x0015`）は core / CLI / REPL 配線（`Session::switch_profile` / CLI `config profile <N>` / REPL `profile switch <N>`、Save 送信なし）として実装済み。**`0x0015` は実機で ACK（`cmd=0x0004/subcmd=0x0015` の 16B 封筒を 1 回観測）と `cur_slot` 6→1 の更新を確認済み**（再接続/電源断を跨ぐ永続性・プロファイル実効・`crc16` とみられる 2B の意味・直接 ACK の条件・ACK 形状の再現性は未確認。`Save 不要` は断定しない。証跡は [docs/DEVICE_CONFIG_EVIDENCE.md](docs/DEVICE_CONFIG_EVIDENCE.md) §7）。** 詳細と残課題は §6 と [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) を参照。
- **ライセンス／公開方針**: パブリックリポジトリ。公式バイナリ（`.apk`, `.exe`, `.dll`, `.so` 等）や公式ファームウェアファイル自体は `.gitignore` で除外しており、**絶対にコミットしないこと**。リポジトリ公開・コードライセンス（`MIT OR Apache-2.0`）・商標・同梱フォント・ストア配布・マネタイズの決定は [docs/DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md)（決定日 2026-11-23）を正本とする。

---

## 2. 必ず先に読むべきドキュメント

実装を始める前に、以下のドキュメントを必ず参照してください。プロトコル・構造体の記述は、公式 Android APK v2.01 の `classes.dex`・`libadvancelib.so` および Windows v1.38 の `8BitDoAdvance.dll` 等の逆アセンブル／デコンパイルから直接検証したものです。実装状況・実機検証状況は各ドキュメントの該当節と §6 に明記しています。

1. **[docs/PROTOCOL_SPEC.md](docs/PROTOCOL_SPEC.md)**
   - VID/PID 一覧（`0x2DC8`, `0x600F`, `0x6011`, `0x6012`, `0x6013`, USB `0x310B`/`0x3208`）
   - BLE GATT UUID（App Mode: `"80ND"` / `0000FF10` / `0000FF11`, Bootloader: `"8bt"` / `0000FF20` / `0000FF21`）
   - 16 バイトカスタムヘッダ（`cumstom_header_t`）、BLE vs USB HID フレーミング、MTU とチャンク分割
   - 2 種類の CRC-16 アルゴリズム（構造体用 `0x8408`・パケット/OTA 用 `0xA001`）
   - App Mode 全コマンド、リアルタイム入力ストリーム（`0x0007` / `0x0003`）、BLE OTA Bootloader シーケンス、8BitDo ファームウェア API
2. **[docs/STRUCT_BT2_2768B.md](docs/STRUCT_BT2_2768B.md)**
   - 2768 バイト（`0xAD0`）`record_ultimatebt2` 設定構造体の全オフセット・型・デフォルト値・部分書き込みパラメータ
   - 32-bit `KeyMap` ビットマスク全表と 22 キーインデックス
   - 10 バイト `MacroDatas` ステップ構造（1ms 単位、8-bit スティック・トリガー）、Flash セクタ配置（`macro_idx << 12`）、Header (`total_len=0xD8`) → Save → Erase → Write → Save のコミット手順
3. **[docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)**
   - 全体アーキテクチャ、ワークスペース構成、Phase 1〜5 の実装ロードマップ、安全原則、現在の状態・残課題
4. **[docs/OTA_BLE_EVIDENCE.md](docs/OTA_BLE_EVIDENCE.md)**
   - BLE OTA の解析証跡（確定した順序・タイムアウト・`.dat` ヘッダ）と未確認事項の優先順。OTA を触る前に必ず読む。
5. **[docs/SESSION_OPERATIONS.md](docs/SESSION_OPERATIONS.md)**
   - 常駐 `session` の運用（per-user BLE 単一所有者ロック、degraded 終了コード 3、SIGINT 範囲、`scan --check` と `scripts/session-loop.sh`）
6. **[docs/ANTI_SLOP.md](docs/ANTI_SLOP.md)**
   - UI が量産的（AI slop）に見える原因と、回避手順・検出チェックリスト・自動テスト・レビュー観点。画面追加時に必ず参照
7. **[docs/OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md)**
   - BLE OTA の実機 btsnoop 採取（Linux `btmon` / Android HCI snoop）・確認項目と判定・EVIDENCE への追記方法、Phase A / Phase B の安全手順。OTA の実機検証前に必ず読む。

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
   - 必ず `Macro ヘッダ更新 (0x0001, total_len=0xD8)` → `Save (0x0006)` → `Erase (0x0104, total_len = chunk_len = 0x1000, ペイロードなし)` → `Write (0x0103, BLE 64B / USB HID 32B（32B アライン。それ以外は拒否。64B 超は 64B にクランプしてから判定）, 最終チャンクは 32B アライン長)` → `Save (0x0006)` の順序を守ること。`macro restore` の slot 単位バッチでは、ヘッダ + 最初の Save を slot に 1 回、最終 Save を全マクロの Erase/Write 後に 1 回だけ送る。マクロの `Erase` / `Write` / ヘッダは 2 秒、`Save` は 3 秒・リトライなし（BLE の正式値は未文書化。USB HID は 3 秒）。OTA の 10 秒 / 1 秒 / 64B 固定をマクロに適用しない。
   - 216B のマクロヘッダ部分書込は config CRC `+0x00C` を wire に載せない（`docs/PROTOCOL_SPEC.md` §6.5）。
4. **OTA（ファームウェア更新）の安全性**:
   - `enterBoot 0x000C` → BLE Boot は `"8bt"` へ再接続 → `readPID 0x0008` (5s)、USB Boot は **RX** `[0x00,0x05]` → `readPID cmd=0x0000/subcmd=0x0008` (5s) の前提を確認し、`readPID` の結果から `gamepad_type` を判定すること。`readVersionInBoot 0x0005` の offset `0x01017040` は type 1 用で、非 type 1 は推測せず `docs/PROTOCOL_SPEC.md` / Issue を確認して停止すること。
   - `EraseFlash 0x0004` の ACK を待ってから `WriteFirmware 0x0003` を送り、失敗時は再接続だけで済ませず、全セクタ消去・再書き込みで復旧すること。USB の復旧は再列挙/再接続であり、BLE の `"8bt"` ではない。
   - **USB Boot の TX フレーミングは未文書化**。`[0x00,0x05]` は RX 専用であり、`EraseFlash` / `WriteFirmware` / `WriteVersion` / `WriteAPPID` / `JumpApp` の USB 書込を実装する PR は、`docs/PROTOCOL_SPEC.md` に TX フレーミングの証跡を追加するまで停止し、`N/A + 理由（USB Boot TX 未文書化）` を記録すること。
   - 実機テスト前に必ずモックトランスポートで全パケット列・CRC・チャンク境界・リトライ動作をユニットテストで検証すること。
5. **クリーンなコミット管理**:
   - プロプライエタリな公式バイナリ（ローカル作業環境に置かれている `.apk` や `8BitDo_Ultimate_Software_V2_Windows_V1.38/`）を絶対に `git add` しないこと。

---

## 5. マージ前の独立レビュー（Tier 制）

`main` へマージする前に、**著者とも他のレビュアーとも別セッション**のレビュアーが PR の差分を読み、マージ可否を判定すること。「マージ可」が出るまでマージしない。修正を push したら新しい head SHA で再レビューする。

- **Tier 1**: レビュアー 1 名。**振る舞いを変えない変更のみ**（Tier 3 パス以外のコメント・整形、通常 docs（規範仕様・ゲート/プロセス文書を除く。これらは Tier 1 にはしない＝最低 Tier 2、安全-critical は Tier 3）、純粋なテスト追加）。純粋なテスト追加は 1〜2 件の故障注入を PR またはレビューコメントに記録する。Tier 3 パスの整形は A1 の条件を満たす場合のみ降格、コメントは A2 で常に Tier 3。write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するテストは Tier 3。
- **Tier 2**: レビュアー 2 名（仕様検収 + 実装/回帰）。
- **Tier 3**: レビュアー 2 名 + 別セッションの敵対的パス（計 3 セッション）。
- 判定・パス別トリガー・故障リストの正本は **[docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)**。**チェックリスト項目、コントリビューター手順、required check、status コマンド、安全手順を変える PR は、docs パスでも Tier 3**。**デバイス書き込みのバイト・順序・タイミング・エラー処理、write-path に影響する依存更新、またはそれを検証するテスト/mock/golden vector を変える PR も Tier 3**。**セキュリティ境界 / review-gate script を検証するテストの変更・削除も Tier 3**。**認証/認可、暗号/署名検証、secret の扱い、trust-boundary の入力検証、権限変更、およびそれらに影響する依存更新も Tier 3**。降格できるのは REVIEW_TIERS §2 のダウングレード制限の例外のみ。混合 PR は code と docs の **最大 Tier** で、故障リストは最大 Tier の要件を両方に適用する（Tier 2 混合 = コード 3 件以上 / docs 3 件以上、Tier 3 混合 = コード 10 件以上 / docs 5 件以上）。
- Tier 定義・パス別トリガー・レビュアー数・quorum・故障リスト最低数・チェックリスト・required check・ruleset・status コマンド・ゲート手順の変更は Tier 3（自己修正ルール）。
- コードのリファクタは Tier 1 にしない。実機書き込み・Flash・OTA・セキュリティに触れるコードは、差分が小さくても Tier 3。
- Tier 2/3 は全レビュアーが同じ SHA で「マージ可」を出した後、リードレビュアーが `scripts/post-review-status.sh` で `review-gate` の commit status を投稿する。Tier 1 はレビュアー本人が投稿する。check run でも gate は満たせるが、本手順は commit status を使う。
- 1 名でも blocker / 要修正なら `success` を付けない。Tier 3 で判定が割れた場合は 4 名目の独立 adjudicator を追加し、blocker の撤回または adjudication 理由を記録する。
- `success` 投稿後に blocker が出た場合（late `CHANGES_REQUESTED`）、リードレビュアーは **同じ SHA に直ちに `review-gate=failure`** を投稿し、修正 push 後の新しい SHA で再レビューする。`success` を維持したままマージしない。`scripts/post-review-status.sh` の `reviewDecision` 確認は best-effort であり、確認後に届くレビューは防げない。
- ruleset が強制するのは `review-gate` required check の成功（commit status または check run）まで。レビュー実施自体は手続きであり、write 権限者は迂回できる。canonical ruleset は **`.github/rulesets/main-protection.json`**（live: id `27785806`）。

---

## 6. 現在の状態と次に着手すべき作業

最終更新: **2026-11-23（main `0af60a3`、PR #16 と #17〜#26 反映）**。フェーズの詳細・レビュー Tier・残課題は [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) §5 を正本とする。

### 6.1 機能の実装状況

| Phase | 内容 | 状態 |
| --- | --- | --- |
| 1 | `eightbitdo-protocol`（CRC / ヘッダ / コマンド / 2768B 構造体 / KeyMap / マクロ / 入力レポート） | 完了。`cargo test -p eightbitdo-protocol` は **単体 100 件 + doc test 4 件** |
| 2a | `eightbitdo-core` + CLI 読み取り専用 + `session` REPL（per-user BLE 単一所有、keepalive、degraded 終了コード 3） | 完了。実機で version / config dump / backup / monitor / keepalive を確認 |
| 2b | BLE マクロ読み書き（3 slot × 4 macro、backup / write / restore） | 完了。実機で slot S1 の M1/M2 の書込・読み戻し・復元を確認（M3/M4・S2/S3 は外挿）。**Switch 側でのマクロ発動は未確認** |
| 2c | BLE OTA 状態機械（`crates/eightbitdo-core/src/ota.rs`） | **core 実装済み・Mock 52 テスト（既定）/ 55（`mobile-boot`）/ 58（`ota-bench`）/ 61（`--all-features`）。#20 で実機採取用の `ota-bench` 段階実行 API（既定 OFF、`write_firmware_with_checkpoint` / `OtaCheckpoint::AfterFirstErase` 等、`OtaBenchOutcome.write_state` / `phase`）を追加。実機 btsnoop の採取計画は [docs/OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md)（#19）。CLI / GUI / UniFFI 未配線、実機 btsnoop 未取得・実機書込未検証** |
| 2d | 2768B 設定書き込み / `config set` / `config remap`（BLE のみ） | 完了。実機（BLE・MTU 200・74B chunk）で full write → Save → read-back 一致、remap A→B→A を確認。**実機確認は full write / remap のみ（`config set` 各フィールド・`--force-raw`・MTU 200 以外は未確認）**。#17 で実機 flag を opaque 値として扱う検証に修正。`slot_flags` の観測値 allowlist を追加し、gyro key_map の erasure sentinel 例外を削除したうえで flag の `0x20190000` は raw allowlist で受理を継続し、実 dump を使う `#[ignore]` E2E を追加して docs を整合させた |
| 3a | 読み取り専用 GUI（egui、日本語、燻しスレート、デバイス / ライブ入力 / 設定値 / ログ） | 完了（PR #9）。読み取り専用の 4 タブ |
| 3b / 3d | GUI 書込 UI（設定タブ / 22 ボタンリマップ / マクロエディタ・録音） | 実装済み・main にマージ（PR #16）。確認ダイアログ、pre-write backup、read-back、TOCTOU 付き。**実機未検証・Tier 3** |
| 3c | GUI OTA 画面 | 未着手（Phase 2c の実機 wire 確定後） |
| 4 | USB OTA | **保留**（USB Boot TX フレーミング未文書化） |
| 5 | `eightbitdo-uniffi` + iOS SwiftUI / Android Compose | **JNI 同梱 APK / unsigned IPA のビルド成功まで（#21、実機未検証）**。Android JNI 同梱 APK 14,551,011 bytes（`libeightbitdo_uniffi.so` を arm64-v8a / x86_64 に同梱）、iOS unsigned IPA 1,550,522 bytes。実機 BLE・JNI ロード・インストール・App Store / TestFlight 配布は未着手。証跡は [docs/MOBILE_BUILD_EVIDENCE.md](docs/MOBILE_BUILD_EVIDENCE.md) |
| — | ライセンス文書（LICENSE-MIT / LICENSE-APACHE / NOTICE / THIRD_PARTY_LICENSES / TRADEMARKS） | 完了（PR #13）。リポジトリ公開・コードライセンス・商標・同梱フォント・ストア配布・マネタイズの決定は [docs/DISTRIBUTION_POLICY.md](docs/DISTRIBUTION_POLICY.md)（#22、決定日 2026-11-23）が正本 |
| — | クロスプラットフォーム・ビルド確認 | [docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md)（#18）。Windows は `cargo check` 成功（リンク・実行・テストは未確認、実効 MSRV 1.88+）、macOS は本ホストで確認不能、iOS target の `cargo check` 成功。**実機は Linux のみ** |
| — | 公式機能カバレッジ・マトリクス | [docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md)（#23）。公式 Windows / Android ソフトとの機能比較、RE / 実装 / 実機検証のラベル定義 |
| — | GUI ライセンス画面（#24、main にマージ済み） | 左レール下部「ライセンス」ボタンから開く `App::licenses_dialog`。アプリの MIT / Apache-2.0 全文と、同梱 5 フォント（Noto Sans JP (subset) の OFL、NotoEmoji の OFL、Ubuntu-Light の UFL、Hack の MIT + Bitstream Vera + DejaVu、emoji-icon-font の MIT）の全文、各著作権者行を表示。Esc / × / 「閉じる」で閉じられ、書込確認中（write ダイアログ表示中）はボタンを無効化。ヘッドレステストで開閉・帰属行・本文見出しの描画を検証 |
| — | プロファイル切替 API（#26、main にマージ済み） | core `Session::switch_profile` + CLI `config profile <N>` + REPL `profile switch <N>`。`0x0015` を送り **Save は送らない**。**実機で ACK（`cmd=0x0004/subcmd=0x0015` の 16B 封筒を 1 回観測）と `cur_slot` 6→1 の更新を確認**（再接続/電源断を跨ぐ永続性・プロファイル実効・`crc16` とみられる 2B の意味・直接 ACK の条件・ACK 形状の再現性は未確認。`Save 不要` は断定しない。証跡は [docs/DEVICE_CONFIG_EVIDENCE.md](docs/DEVICE_CONFIG_EVIDENCE.md) §7。[docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md) の残作業「`0x0015` 切替のセッション API 化」に対応） |

**実機検証済み（2026-11-21〜23、Linux・BLE・MTU 200）**:

- 読み取り: ReadVersion **200 (0xC8)**、2768B config dump、マクロ backup、20ms keepalive **128.9 秒**
- マクロ書込: slot S1・`macro_idx=0/1 (M1/M2)`。Write `0x0103` の `total_len` は **絶対終端** `(macro_idx << 12) + align32(steps * 10)` が必須（相対値は ACK のみで永続化されない）
- マクロ復元: `macro restore` 後に config を read-back し、**復元後の read-back が書込前の read-back と SHA-256 一致することを都度確認**した。**Switch 側でのマクロ発動は未確認**
- 設定書込: full 2768B write（**38 write + 1 Save = 合計 39 packets**、74B chunk）→ read-back SHA 一致。device は送信した CRC を保持（`stored` 一致、`in_place` 解釈とは不一致）。`config remap` の A→B→A を同一接続で確認。**実機確認範囲は full write / remap のみ**
- **2026-11-23 の追加確認（再接続後の再確認）**: Linux BLE・MTU 200 で再接続し、version **`0xC8`**、config の full write → read-back、`config remap A B` → read-back で `A A` の往復（remap 書込 read-back SHA `fae1de6…` / 復帰 read-back SHA `7a9838e…`）、`macro restore`（slot 0 の M1/M2、read-back SHA `7ebbfde…` / `c6f2419…`、22 packets）を確認した。**config の SHA-256 は 2026-11-22 時点の `1dca47fa…` とは異なる**（ユーザーがプロファイル名 `za` / `Profile2` を設定したためと考えられ、現在は `95b87bb0…`）。したがって「元の SHA に一致」のような固定値の断定はせず、**各操作後の read-back が期待状態と一致し、復帰操作の後は書込前の read-back と一致することを確認した**と記録する
- **プロファイル切替（`0x0015`）の実機確認（2026-11-23、Linux BLE・MTU 200）**: `config profile 1` を実行し、要求 **17B**（`04 15 00 01 …`、`cmd=0x0015/subcmd=0x0001`、`total_len=0`）を送信。ACK は **`cmd=0x0004/subcmd=0x0015` の 16B 封筒**（`04 00 15 00 … 81 …`）を **1 回**観測し、直接 `cmd=0x0015` の応答は観測されなかった。`Save` を送らなくても直後の `config dump` で `cur_slot`（`+0x12`）が **6→1** に変化（read-back SHA `7a9838e8…`→`e3a84da2…`）。復元は `config dump --raw` の `+0x12` を 6 に戻して `config write --file … --yes`（39 packets、read-back `cur_slot=6`、SHA `af897618…`）。**未確認**: 再接続/電源断を跨ぐ永続性、プロファイル実効、`crc16` とみられる 2B（`0x8100` / `0x0081` の解釈）、直接 `cmd=0x0015` ACK の条件、ACK 形状の再現性（`Save 不要` は断定しない）。詳細は [docs/DEVICE_CONFIG_EVIDENCE.md](docs/DEVICE_CONFIG_EVIDENCE.md) §7
- **実機 flag の扱い（修正済み）**: 実機の各レコード flag は `0x20200911` / `0xFFFFFFFF` / gyro slot0 `0x20190000` で意味は未確定（opaque）。raw 検証は観測値集合（`0` / `1` / `0x20200911` / `0xFFFFFFFF`、gyro は `0x20190000` も）のみ受理し、実機 dump は `--force-raw` なしで書き戻せる（未観測値は `--force-raw` 必須）。`slot_flags` も同じ観測値集合のみ受理。数値範囲は flag の値に関係なく常時検証（実機の完全消去 slot のみ例外）。消去例外は slot 全体（`slot_flags[slot]` が `0xFFFFFFFF`、joy 全 255 + rumble NaN + map_key 全 22 `0xFFFFFFFF`、各 flag が `0xFFFFFFFF`）でのみ成立し、record 単体の partial state は拒否する。`0xFFFFFFFF` の flag 値許可は全レコード共通だが payload の消去扱いにはならない。typed `config set` の flag のみ 0/1。実バイト証跡は [docs/DEVICE_CONFIG_EVIDENCE.md](docs/DEVICE_CONFIG_EVIDENCE.md)
- **既知バグ（未修正、2026-11-23 発見）**: BLE リンクが実際に切断された後も `session` の `status` が `connected: true` と keepalive カウンタ（sent / received）を保持し続けることを観察した。その状態でコマンドを送ると `Not connected` エラーが返ることは確認済み。`status` の表示が実リンク状態に追従していないため、**リンク断を `status` から検知できない**（keepalive が degraded 判定に落ちるかは未確認）。未修正の既知バグとして扱い、修正は別 PR（`crates/eightbitdo-core/**` の session / keepalive に触れるため **Tier 3**。§6.3）
- **未検証**: slot S2/S3 と `macro_idx=2/3 (M3/M4)`、MTU 200 以外、USB HID の config / macro write、REPL 内からの書き込み、GUI 書き込み（PR #16）、OTA の wire / ACK / アドレス / MTU、`0x0015` の再接続/電源断を跨ぐ永続性とプロファイル実効、ACK 形状の再現性・直接 ACK の条件、macOS / Windows / Android / iOS 実機

### 6.2 次に着手すべき作業（優先順）

1. **BLE OTA の実機 wire 確定（Tier 3・最優先 blocker）**: 採取手順と判定基準は [docs/OTA_VERIFICATION_PLAN.md](docs/OTA_VERIFICATION_PLAN.md)（#19、Phase A / Phase B）、段階実行 API は `ota-bench` feature（#20）で用意済み。実機 btsnoop を取得し、[docs/PROTOCOL_SPEC.md](docs/PROTOCOL_SPEC.md) §8.5「未検証事項」のうち wire 関連の 1〜13 を確定する（14 は trust boundary、15 はバッテリー / 取得検証で別課題）。特に (a) **ACK の `offset` / `total_len` 照合の追加**（現状は cmd/subcmd のみで遅延・重複 ACK を区別しない）、(b) **マルチセクション `.dat` 対応**（現実装は単一セクションのみで、公式 `.dat` がマルチセクションなら更新を拒否する）、(c) `readPID` / ready ACK / Erase・Write のアドレス規約 / `WriteVersion` payload / `WriteFirmware` フレーム長 / Bootloader MTU。確定まで CLI / GUI / UniFFI に OTA を配線しない。
2. **OTA の配線と安全機構**: 確定後、CLI / GUI / UniFFI に OTA を追加し、バッテリー警告、バージョン / ダウングレード判定、ファームウェア取得・SHA-256 検証、成功時の `"80ND"` 再接続 + バージョン確認を実装する。Tier 3。
3. **GUI 書き込み UI**（PR #16）: config write / remap / マクロエディタは main にマージ済み。残るのは実機確認（Tier 3、pre-write backup + read-back 前提）と、backup 復元 UI / JSON・TOML 入出力。**GUI ライセンス画面（#24）とプロファイル切替（`0x0015`）の core / CLI / REPL 配線（#26）は main にマージ済み**。`0x0015` は実機で ACK と `cur_slot` 6→1 の更新を確認済み（Save 送信なし。再接続/電源断を跨ぐ永続性・プロファイル実効・`crc16` とみられる 2B の意味・直接 ACK の条件・ACK 形状の再現性は未確認）。
4. **モバイルの仕上げ**: **ビルド証跡は [docs/MOBILE_BUILD_EVIDENCE.md](docs/MOBILE_BUILD_EVIDENCE.md)（#21）に記録済み**（Android JNI 同梱 APK 14,551,011 bytes / iOS unsigned IPA 1,550,522 bytes。いずれもビルド・パッケージ確認のみ）。残るのは iOS / Android 実機での BLE 読み取りと JNI ロード、エミュレータ起動確認、署名・App Store / TestFlight 配布（macOS + Xcode が必要）。書き込み・OTA の公開は Tier 3 レビューとバックアップ UI が揃うまで行わない（[mobile/README.md](mobile/README.md)）。
5. **クロスプラットフォーム実機確認**: macOS / Windows の BLE（btleplug）と Linux BlueZ の回帰、Android / iOS。現状の実機証跡は Linux のみ。**ビルド/コンパイルの確認状況は [docs/CROSS_PLATFORM.md](docs/CROSS_PLATFORM.md)（#18）**: Windows は `cargo check`（リンク・実行は未確認、MinGW 必要）のみ確認済みで、**Windows ビルドは Rust 1.88 以上が必要**（`btleplug` の Windows バックエンドの let チェーン由来。宣言 MSRV 1.85 では `E0658`）、macOS は target / SDK 不在のため Linux からは確認不能。
6. **`status` のリンク断検知バグの修正**: 上記の既知バグ（切断後も `connected: true` と keepalive カウンタが残る）を直し、`status` を実リンク状態に追従させる。session / keepalive に触れるため **Tier 3**。テストで固定してから直す。
7. **USB HID / 2.4G（Phase 2e / 4）**: USB App Mode の実機証跡と USB Boot の TX フレーミングが未文書化のため保留。`docs/PROTOCOL_SPEC.md` に証跡が追加されるまで実装しない。

### 6.3 レビュー Tier（変更時）

- 2b / 2c / 2d、GUI の書き込み UI、モバイルの書き込み公開は **Tier 3**（write-path / Flash / OTA / CRC / mock transport / 安全手順）。
- 3a の表示のみの変更は **Tier 2**。`crates/eightbitdo-core/**`（session / transport / mock）に触れる場合は **Tier 3**。
- OTA の ACK 照合・`.dat` パーサ・フラッシュ順序、モバイルの UniFFI 公開 API・transport 契約の変更は **Tier 3**。
- 判定の正本は [docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)。

### 6.4 検証コマンド

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo test -p eightbitdo-core --features ota-bench --locked
cargo test -p eightbitdo-protocol --no-default-features --locked
cargo +1.85.0 check --workspace --all-targets --all-features --locked
```

実機では **読み取り専用から**検証し、書き込み前に必ず 2768B 設定とマクロ 3 スロット × 4 マクロをバックアップしてファイル名と SHA-256 を記録する（§4・[docs/SESSION_OPERATIONS.md](docs/SESSION_OPERATIONS.md)）。OTA は実機 btsnoop と ACK 照合が揃うまで実機書込しない。
