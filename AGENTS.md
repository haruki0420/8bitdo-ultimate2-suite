# AGENTS.md — AI エージェント向け引き継ぎガイド

このファイルは、どの AI エージェントや開発者が途中から参加しても、公式バイナリの再解析なしに即座に実装・検証へ進めるためのプロジェクト引き継ぎドキュメントです。

---

## 1. プロジェクト概要

- **目的**: **8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）を、Nintendo Switch と接続したまま **Linux / macOS / Windows / Android / iOS** から BLE GATT（および USB/2.4G HID）でフル設定・マクロ編集・ファームウェアアップデートできるクロスプラットフォーム OSS ツール群を構築する。
- **現在のフェーズ**: **Phase 1 完了（`eightbitdo-protocol` 実装・テスト済み）。Phase 2 実装着手待ち**。
- **ライセンス／公開方針**: パブリックリポジトリ。公式バイナリ（`.apk`, `.exe`, `.dll`, `.so` 等）や公式ファームウェアファイル自体は `.gitignore` で除外しており、**絶対にコミットしないこと**。

---

## 2. 必ず先に読むべきドキュメント

実装を始める前に、以下の 3 つのドキュメントを必ず参照してください（すべて公式 Android APK v2.01 の `classes.dex`・`libadvancelib.so` および Windows v1.38 の `8BitDoAdvance.dll` 等の逆アセンブル／デコンパイルから直接検証済みです）。

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
   - 全体アーキテクチャ、ワークスペース構成、Phase 1〜5 の実装ロードマップ、安全原則

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

**Phase 1 は完了済み**です（`crates/eightbitdo-protocol`）。

- 実装済み: `crc.rs` / `header.rs` / `commands.rs` / `config_bt2.rs` / `keymap.rs` / `macro_data.rs` / `input_report.rs`
- 検証コマンド:
  - `cargo test -p eightbitdo-protocol` → 単体テスト 74 件 + doc test 4 件
  - `cargo clippy -p eightbitdo-protocol --all-targets` → 警告なし
  - `cargo check -p eightbitdo-protocol --no-default-features` → `no_std` + `alloc` でビルド可
  - `cargo test -p eightbitdo-protocol --features serde` → JSON/TOML 用 `MacroFile` を含めてビルド可

次は **[docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)** の **Phase 2** に進んでください。

1. `crates/eightbitdo-core` に `Transport` トレイト（BLE / USB HID / Mock）とセッション管理を実装する。
2. `crates/eightbitdo-cli` に `scan` / `info` / `config dump` / `monitor` を実装する。
3. 実機では **読み取り専用コマンドから**検証し、最初の書き込み前に必ず 2768B 設定とマクロ 3 スロット × 4 マクロをバックアップし、ファイル名と SHA-256 を記録する。
4. 書き込み系の前に、モックトランスポートで `Header 0x0001 (total_len=0xD8) → Save 0x0006 → Erase 0x0104 → Write 0x0103 → Save 0x0006` のパケット列と CRC を固定する。
