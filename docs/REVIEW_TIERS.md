# 独立レビュー Tier

PR のリスクに応じてレビュー深度を切り替えるための正本です（`AGENTS.md` / `CONTRIBUTING.md` は要約）。ゲート実装の正本は `.github/rulesets/main-protection.json`（live: `main-protection`, id `27785806`）、status 投稿の正本は `scripts/post-review-status.sh` です。

## 1. Tier 表

| Tier | 対象 | レビュー体制 | 故障リスト最低数 |
| --- | --- | --- | --- |
| **1** | Tier 3 パス以外のコメント・整形、通常 docs（規範仕様・ゲート/プロセス・法務/ライセンス文書を除く）、純粋なテスト追加のみ（write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test / セキュリティ境界 / review-gate script を検証するものを除く）。**振る舞いを変えない** | 1 名（diff + テスト + リンク/コマンド） | 純粋テスト追加は 1〜2 件の故障注入を記録。振る舞いを変えない docs/コメント/整形は不要 |
| **2** | 読取専用コード（セキュリティ挙動除く）、読取系ビルダー、プロセス文書の散文、規範仕様の非安全-critical 編集、write-path/セキュリティ非影響の依存・衛生、非 Tier 3 テスト変更/削除、非 Tier 3 リファクタ | 2 名（仕様検収 + 実装/回帰） | コード 3 件以上 / docs・プロセス 3 件以上（混合 PR は両方） |
| **3** | 書き込み経路 / Flash / OTA / backup-restore / CRC / offset / framing / マクロ step、セキュリティ挙動、ゲート実装・チェックリスト、法務・ライセンス docs、リリース、大規模混合差分 | 2 名 + 別セッションの敵対的パス（計 3 セッション） | コード 10 件以上 / docs・ゲート 5 件以上（混合 PR は両方） |

## 2. 判定ルール

- **混合 PR は最大 Tier**とし、その最大 Tier の故障リスト要件を**コード・docs の両方**に適用する（例: code Tier 2 / docs Tier 3 → コード 10 件以上 + docs 5 件以上）。迷ったら 1 段上、行数では判断しない。
- **昇格可・原則降格不可**: レビュアーは昇格でき、降格は下記ダウングレード制限の例外のみ。revert / hotfix / security fix も触ったパスの最大 Tier を継承し、緊急降格はしない。push でリスク面が変わったら再判定・再レビューする。
- **Content-based override（内容優先で Tier 3）**: パスにかかわらず、①レビュアー確認内容（PR テンプレート項目・観点・故障リスト最低数・N/A 条件）②コントリビューター手順（`CONTRIBUTING.md` / backup・復元 / リリース）③required check / status コマンド / ruleset / workflow / ゲート手順 ④安全手順（Flash / OTA / backup / マクロ / ブリック復旧）の変更・削除・弱体化 ⑤**ライセンス・法務判断を含む docs**（`LICENSE*` / `NOTICE` / `THIRD_PARTY_LICENSES.md` / `TRADEMARKS.md`、ライセンス節・免責・商標を含む `README.md` / `docs/`。docs-only でも自動検証省略不可）。
- **Write-path rule（Tier 3）**: 書込バイト・offset・CRC・framing・cmd/subcmd・`total_len`・チャンク分割・順序・タイミング・タイムアウト・ACK・リトライ・エラー処理・復旧を変え得る変更、およびそれを検証するテスト・mock・golden vector・依存更新（`btleplug` / `hidapi` 等）・build script・モバイル書込経路・書込 UI。
- **Security behavior rule（Tier 3）**: 認証/認可、暗号/署名検証、secret、trust-boundary 入力検証、権限・sandbox・capability・CSP、およびこれらに影響する依存更新・テスト変更/削除。
- **自己修正ルール（Tier 3）**: Tier 定義・パス別トリガー・レビュアー数・quorum・故障リスト最低数・チェックリスト・required check・ruleset・status コマンド・ゲート手順の変更。

### ダウングレードの制限（Tier 3 パスの例外）

- **(A1) コード整形のみ**: `git diff -w` は使わない。base commit を `cargo fmt --all` した tree hash が `HEAD^{tree}` と完全一致することを一時 worktree で検証し、レビュアーが full diff 確認と証跡を記録した場合のみ降格可。
- **(A2) コードコメントのみ**: 例外なし（常に Tier 3）。
- **(A3) ドキュメント**: 非規範の散文のみ。HTML コメント・リンク参照定義・コードフェンス info string の変更は意味変更扱い。規範仕様・ゲート・プロセス文書は Tier 1 にしない（最低 Tier 2、安全-critical は Tier 3）。

## 3. パス別トリガー（基本 Tier。§2 が常に優先）

- **Tier 1 例**: 通常 docs（`README.md` 等、ライセンス/免責節除く）、非 Tier 3 パスのコメント・整形・純粋テスト追加。
- **Tier 2 以上**: `AGENTS.md` / `CLAUDE.md` / `CONTRIBUTING.md` / `docs/REVIEW_TIERS.md` / `docs/BRANCH_PROTECTION.md` / `.github/pull_request_template.md`（散文・誤字のみ Tier 2、手順・チェックリストは Tier 3）、`docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` / `docs/IMPLEMENTATION_PLAN.md`（非安全-critical のみ Tier 2、offset/CRC/Flash/OTA/framing は Tier 3）、`Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml`（write-path・セキュリティ影響は Tier 3）、`.gitignore`（公式バイナリ除外弱体化は Tier 3）、`apps/desktop/src/**`（表示のみ Tier 2、書込確認・安全文言は Tier 3）、`.github/dependabot.yml`（セキュリティ更新方針変更は Tier 3）。
- **Tier 3**: `.github/{workflows,CODEOWNERS,rulesets}/**`、`scripts/post-review-status.sh`、`eightbitdo-protocol/src/{crc,header,macro_data}.rs`・書込関連の `config_bt2.rs`/`commands.rs`/`keymap.rs`、`eightbitdo-core/**`（書込・session・OTA・backup）、`eightbitdo-cli/**`（書込）、`eightbitdo-uniffi/**`（書込・OTA・公開境界）、`mobile/{ios,android}/**`（native write path）、`apps/desktop/src-tauri/**`（IPC/capability/CSP）、`packaging/**`、`unsafe`/FFI/secrets/build script、mock/golden/セキュリティ境界テスト、法務・ライセンス docs。

## 4. レビュー手順とチェックリスト

### 共通ルール

- **範囲**: 既定は PR diff のみ（不足時のみ変更ファイル全体・呼び出し元・API/スキーマ契約・セキュリティ周辺を読む。follow-up は前回レビュー SHA からの差分のみ、force-push/新規参加時はフル diff）。レビュアーは CI 完了を待たず判定を先に返し、CI は親/リードが確認する。
- **独立性と合議**: 著者とも他レビュアーとも別セッションで独立判定し、各自がセッション ID・SHA・判定・コメント URL を記録（代筆禁止）。1 名でも blocker なら `success` を付けない。Tier 3 で判定が割れたら 4 名目の独立 adjudicator を追加する。
- **late `CHANGES_REQUESTED`**: `success` 投稿後に blocker が出たら、リードは直ちに同一 SHA へ `review-gate=failure` を投稿し、修正後の新 SHA で再レビューする。
- **Tier 3 PR テンプレート**: A–E の存在が必須（該当しない項目は `N/A + 理由`）。docs-only PR は突合・リンクチェック・grep・シナリオ通読で検証し、mock/実機項目は `N/A + 理由` とする。

### Tier 3 固有チェック項目

- **実機書き込みバックアップ**: 2768B 設定 + マクロ 3 slot × 4（12 領域）のファイル名と SHA-256 を記録し復元確認する。例外として `config write/set/remap`（`0x0103`/`0x0104` 非送信）は 2768B + `config.json` を必須とし、マクロヘッダ領域（`0x68C..0x914`）変更時は明示警告する。
- **OTA（§6.3）**:
  - `enterBoot 0x000C`（`subcmd=0x0067`）→ BLE `"8bt"`（`FF20`/`FF21`）→ `readPID 0x0008`（5s）→ `gamepad_type` 判定 → `readVersionInBoot 0x0005`（type 1: `0x01017040`、非 type 1 は推測せず停止）。
  - `EraseFlash 0x0004`（4KB 境界、10s、ACK 待ち）→ `WriteFirmware 0x0003`（64B 固定チャンク・最終含む、CRC-16/MODBUS、1s、各 ACK 待ち）→ `WriteVersion 0x00C4`（24B、3s）→ `WriteFirmwareVersion 0x0002`（type 1: `0x01017040` / 他: `0x08014034`）→ `WriteAPPID 0x0002`（非 type 1 のみ `0x0801401C`）→ `JumpApp 0x0006`（type 1: `0x01018000`）。
  - **USB OTA 書込停止規則**: `[0x00,0x05]` は RX 専用、TX 未文書化のため `PROTOCOL_SPEC.md` に証跡追加まで停止し `N/A + 理由（USB Boot TX 未文書化）` とする。失敗復旧は全セクタ消去・再書込。
- **マクロ Flash（§6.4）**:
  - 順序: `Header 0x0001`（`offset=0x68C+slot*0xD8`, `total_len=0xD8`, `+0x00C` 非送信）→ `Save 0x0006`（BLE `0x0067` / USB `0x0123`）→ `Erase 0x0104`（`subcmd=(mode<<8)|slot`, `offset=macro_idx<<12`, `total_len=chunk_len=0x1000`, `crc16=0xFFFF`, payload 0B）→ `Write 0x0103`（`total_len=(macro_idx<<12)+align32(steps*10)` 絶対終端、32B/64B・32B アライン、CRC-16/MODBUS）→ `Save 0x0006`。slot バッチ restore は Header+Save 1 回 → 全 Erase/Write → 最終 Save 1 回。
  - タイムアウト: Header / Erase / Write は 2 秒、Save は 3 秒・リトライなし（OTA の 10s/1s/64B 固定と混同しない）。書込中は `0x0003` を送らない（`--poll-interval-ms 0`）。
- **ゲート / セキュリティ / リリース**: canonical ruleset と live `27785806` の一致確認、`scripts/post-review-status.sh <PR> <reviewed-sha> <1|2|3> [success|failure]` の実行・監査（詳細は [BRANCH_PROTECTION.md](BRANCH_PROTECTION.md)）。

## 5. 故障リストと強制範囲

故障リストは「故障 / 検出方法 / 結果 / 残存リスク / 証跡（テスト名・コマンド出力・URL・file:line）」を記録する。検出できないものはテスト追加・Tier 昇格・残存リスク明記で対処する。

ruleset が強制するのは `review-gate` required check の成功までであり、Tier 判定・レビュー実施・故障リストは手続きである（限界と強化策は [BRANCH_PROTECTION.md](BRANCH_PROTECTION.md) §4 を参照）。
