## 変更内容

<!-- 何を、なぜ変更したか。仕様上の根拠（docs の該当箇所）も書いてください。 -->

## Review Tier

該当する **最大 Tier のみ** チェックしてください。判定・パス別トリガー・故障リストの正本は `docs/REVIEW_TIERS.md` です（テンプレートファイルからは `../docs/REVIEW_TIERS.md`、PR 本文では `docs/REVIEW_TIERS.md`）。このテンプレートは実チェックリストの写しであり、矛盾した場合は REVIEW_TIERS を優先します。

- [ ] **Tier 1**（レビュアー 1 名。振る舞いを変えない変更のみ。Tier 3 パス以外のコメント・整形。Tier 3 パスの整形は A1 の条件を満たす場合のみ降格、コメントは A2 で常に Tier 3。write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するテストを除く）
- [ ] **Tier 2**（レビュアー 2 名: 仕様検収 + 実装/回帰）
- [ ] **Tier 3**（レビュアー 2 名 + 別セッションの敵対的パス）

> 規範仕様（`PROTOCOL_SPEC` / `STRUCT` / `IMPLEMENTATION_PLAN`）・ゲート文書・プロセス文書は Tier 1 にはしない（最低 Tier 2、安全-critical は Tier 3）。

判定理由:

<!-- 例: config_bt2.rs と commands.rs の書き込み経路を変更するため Tier 3 -->

> チェックリスト項目、コントリビューター手順、required check、status コマンド、安全手順を変える PR は、docs パスでも Tier 3。デバイス書き込みのバイト・順序・タイミング・エラー処理、write-path に影響する依存更新、またはそれを検証するテスト/mock/golden vector を変える PR も Tier 3。セキュリティの振る舞い（authn/authz、crypto/signature、secrets、trust-boundary、permissions）を変える PR も Tier 3。セキュリティ境界 / review-gate script を検証するテストの変更・削除も Tier 3。

## 故障リスト（Tier 1 の純粋なテスト追加 / Tier 2 / Tier 3）

- Tier 1 で **Tier 3 パス以外**の振る舞いを変えない docs/コメント/整形のみの場合に限り、このセクションを削除できます。Tier 3 パスの整形は A1 の条件を満たす場合のみ降格、コメントは A2 で常に Tier 3 です。
- Tier 1 の **純粋なテスト追加** は削除できません。下の「Tier 1 故障注入記録」を記入してください。
- **write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するテスト** は Tier 1 ではなく Tier 3 です。
- Tier 2/3 は削除できません。

件数: コード 3 件以上（Tier 2、目安 3–5、見つかった故障はすべて記録）/ 10 件以上（Tier 3）、docs/ゲートは検出方法 3 件以上（目安 3–5）/ 5 件以上。混合 PR は最大 Tier の要件を両方に適用します（**Tier 2 混合 = コード 3 件以上 / docs 3 件以上、Tier 3 混合 = コード 10 件以上 / docs 5 件以上**）。各項目に「故障 / 検出方法 / 結果 / 残存リスク / 証跡」を記録し、未検出は残存リスクと Issue/PR を書いてください。

> 記入例: 故障「CRC の多項式を取り違える」/ 検出方法「CRC テストベクタ」/ 結果「検出」/ 残存リスク「なし」/ 証跡「crc::tests::...」

| 故障 | 検出方法（テスト / コマンド / 目視） | 結果 | 残存リスク | 証跡 |
| --- | --- | --- | --- | --- |

### Tier 1 故障注入記録（純粋なテスト追加のみ）

Tier 1 の純粋なテスト追加では、レビュアーが 1〜2 件の故障を注入し、追加テストが落ちることを確認して記録します。この表は純粋なテスト追加では削除できません。振る舞いを変えない docs/コメント/整形のみの Tier 1 は、故障リストとこの記録を削除できます。Tier 2/3 では「N/A（Tier 1 ではない）」と記入してください。

> 記入例: 注入「境界値を 1 つずらす」/ 検出したテスト「追加した境界値テスト」/ 結果「検出」/ 証跡「テスト名 / コマンド出力」

| 注入した故障 | 検出したテスト | 結果 | 証跡 |
| --- | --- | --- | --- |

## 独立レビュー

各レビュアーは、他のレビュアーの判定を見る前に、同じ diff / head SHA に対して自分の行とレビューコメントを書きます。リードレビュアーは行を代筆せず、全員のコメント URL と SHA を確認して status を付けます。判定欄はレビュアーが記入するまで空のままにしてください。PR 本文を編集できないレビュアーは、役割 / セッション ID または氏名 / レビューした SHA / 判定を含む standalone review comment を投稿してください。リードレビュアーはその URL を表に記録しますが、判定を代筆しません。

> レビュー範囲は **PR の diff** を既定とします。diff だけでは判断できない場合に限り、変更ファイル全体、変更したシンボルの呼び出し元、API / スキーマ契約、セキュリティ・認証・データ・リリース workflow・破壊的変更の周辺まで広げます。write-path / Flash / OTA / backup / CRC / セキュリティ境界 / review-gate script の変更は、diff だけでは判断できない典型例として、変更したシンボルの呼び出し元・API / スキーマ契約・関連テストを必ず確認します（Tier 3 の範囲を弱めません）。全リポジトリ・全履歴・CI の再実行は求めません。レビュアーは CI を待たず判定を先に返し、CI はリード／親が確認します。follow-up は前回レビューした SHA からの差分だけを読みます。force-push / rebase で前回レビュー SHA が辿れない場合、または新規レビュアーが途中参加する場合は、前回差分ではなくフル diff を読みます。

| 役割 | レビュアー（セッション ID / 氏名） | レビューした SHA | レビューコメント URL | 判定（マージ可 / 要修正） |
| --- | --- | --- | --- | --- |
| 仕様検収（Tier 1/2/3） |  |  |  |  |
| 実装 / 回帰（Tier 2/3） |  |  |  |  |
| 敵対的パス（Tier 3） |  |  |  |  |
| adjudicator（Tier 3 で判定が割れた場合のみ） |  |  |  |  |

- [ ] レビュアーは著者と別セッションである
- [ ] Tier 2/3 のレビュアー同士も別セッションである
- [ ] 全レビュアーが **同じ head SHA** で「マージ可」を出した（Tier 3 で割れた場合は 4 名目の adjudicator を追加し、blocker の撤回または adjudication 理由を記録した）
- [ ] Tier 1 はレビュアー本人、Tier 2/3 はリードレビュアーが `scripts/post-review-status.sh` で `review-gate` の commit status を投稿した（check run でも gate は満たせるが、本手順は commit status。description に Tier と SHA）
- [ ] `success` 投稿後に blocker / `CHANGES_REQUESTED` が出た場合、同じ SHA に直ちに `review-gate=failure` を投稿し、修正後の新しい SHA で再レビューした（スクリプトの `reviewDecision` 確認は best-effort）
- [ ] Tier 3 パスを Tier 1/2 に下げた場合、A1 は base commit を別 worktree に checkout して `cargo fmt --all` → `git add -A` → `git write-tree` の結果が head の `git rev-parse HEAD^{tree}` と一致する証跡、A3 は非規範散文のソースレベル diff（HTML コメント・リンク参照定義・コードフェンス info string を含まない）を記録した（`git diff -w` だけ、コードコメントのみの例外、コメント prefix heuristic、レンダリング等価だけの A3 は不可。A2 は常に Tier 3。規範仕様・ゲート・プロセス文書は Tier 1 にはしない。最低 Tier 2、安全-critical は Tier 3）
- [ ] ダウングレード時、レビュアーが full diff を確認し、意味の変更がないことを記録した
- [ ] Tier 3 は A-E が PR 本文に存在し、該当しない項目は `N/A + 理由` を記入した
- [ ] 故障表・注入記録のデータ行に `例:` が残っていない（上の記入例 blockquote は対象外）
- [ ] レビューコメントをすべて解決した
- [ ] **この head SHA で Tier を再評価した**

## 検証

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo clippy -p eightbitdo-protocol --no-default-features --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `cargo test --workspace --all-features --locked`
- [ ] `cargo test -p eightbitdo-protocol --no-default-features --locked`
- [ ] `cargo check -p eightbitdo-protocol --no-default-features --all-targets --locked`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked`
- [ ] `rustup toolchain install 1.85.0`
- [ ] `cargo +1.85.0 check --workspace --all-targets --all-features --locked`
- [ ] `cargo +1.85.0 check -p eightbitdo-protocol --no-default-features --all-targets --locked`
- [ ] `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` との整合性を確認した
- [ ] 予約領域・未知バイトを破壊しないことを確認した
- [ ] 実機で未検証の項目は、その旨を明記した

### A-E 証跡チェック（Tier 1/2/3 共通）

Tier 1/2 もセクションを削除せず、該当しないサブセクションには「N/A + 理由」を 1 行記入します（全項目を埋める必要はありません）。

Tier 3 では A-E の全セクションが PR 本文に存在しなければならず、**省略は欠陥（要修正）** です。Tier 3 では全項目を確認し、該当しないものだけ `N/A + 理由` とします。

#### A. 共通証跡

- [ ] 故障リストの各項目に証跡（テスト名 / コマンド出力 / URL / 目視メモ / 該当箇所の引用 / file:line）を記録した
- [ ] 実機書き込みを含む場合: 2768B 設定のバックアップ path と SHA-256 を記録した: `path=... sha256=...`
- [ ] 実機書き込みを含む場合: マクロ 3 スロット × 4 マクロ（`slot 0..2` × `macro_idx 0..3`）のバックアップ path と SHA-256 を記録した: `path=... sha256=...`
- [ ] 復元テストまたは文書化された復元手順を確認し、結果を記録した: `result=...`
- [ ] 復元後、全 slot の 216B マクロヘッダと非空マクロの raw を読み戻して比較した（2768B 全体の SHA-256 は device の CRC 再計算で変わり得るため情報表示のみ）
- [ ] OTA を含む場合: OTA イメージの SHA-256 を記録した: `sha256=...`
- [ ] OTA を含む場合: Bootloader 復旧結果（全セクタ消去 → 再書き込み → 起動確認）を記録した: `result=...`
- [ ] **この head SHA で Tier を再評価した**

#### B. コード変更を含む場合（プロトコル / Flash / OTA / マクロ）

コード変更がない場合は「N/A（docs/チェックリストのみ）」と理由を記入してください。OTA の詳細・正本は `docs/REVIEW_TIERS.md` §6.3 です。

- [ ] モックトランスポートでパケット列・CRC・チャンク境界・リトライを検証した
- [ ] OTA / Flash（Bootloader）: 4KB セクタ境界、10 秒 erase タイムアウト、erase ACK 待ち、64B 固定チャンク（最終チャンクを含む）を確認した（マクロ Flash は下記の 2 秒 / 32B アラインを使い、OTA の 10 秒 / 1 秒 / 64B 固定を適用しない）
- [ ] OTA 前提 (BLE Boot): `enterBoot 0x000C`（subcmd `0x0067`, `total_len 4`, `offset 4`）→ `"8bt"` FF20/FF21 再接続 → `readPID 0x0008`（subcmd `0x0067`, `total_len 2`, 5s）→ `readVersionInBoot 0x0005`（subcmd `0x0067`, `total_len 4`, offset `0x01017040` [gamepad_type == 1], 5s timeout）を確認した
- [ ] OTA 前提 (USB Boot — 2026-11-29 確定): Switch (`0x057E:0x2009`) → `0x2DC8:0x600F` 再列挙後、TX `[0x81,0x05]` / RX `[0x02,0x05]`（64B HID、`<=46B [0x2E]`）で `Section 1 (0x600F, 相対 offset)` → `0x00C1 (0x8000)` → `Section 0 (0x6010, .dat 0x40004000 → wire 0x40000000)` → `WriteVersion 0x00C4 (8B)` → `Reset 0x0007 (8B)` の順（`0x0002`・`0x0008` は非送信）、および Section 0 512B 境界 ACK 例外（`clen=512, off = req.off & !0x1FF`）と 2.4G (`0x310B`) OTA 禁止を確認した。該当しない transport は `N/A + 理由` を記録した
- [ ] OTA 書込: BLE Boot（`allow_unverified_wire` 既定 false・btsnoop 待ち）または USB Boot（確定済み 2 セクション・46B 上限・`off/tlen/clen` ACK 照合）の境界に従ったことを記録した
- [ ] OTA 書込: `EraseFlash 0x0004`（`offset=load_base_addr+sector_off`, `total_len=0x1000`, 10s / ACK 待ち）→ `WriteFirmware 0x0003`（`offset=load_base_addr+sector_off+chunk_off`, `total_len=header[8]`, 64B 固定、CRC-16/MODBUS 付き、1s / 各 64B ACK 待ち）→ `WriteVersion 0x00C4`（`total_len=header[8]`, 24B ヘッダ / 3s）→ `WriteFirmwareVersion 0x0002`（type 1: `0x01017040` / それ以外: `0x08014034`, 4B + CRC16）→ `gamepad_type != 1` のときのみ `WriteAPPID`（`0x0801401C`, 12B + CRC16, 5s）→ `JumpApp 0x0006`（`0x01018000` [gamepad_type == 1], 5s）の順序を確認した
- [ ] OTA `WriteFirmware` は各 64B チャンク送信ごとに `offset` を 64B 進め、チャンク ACK を待った（1 秒タイムアウト、リトライ方針を記録）
- [ ] 非 gamepad_type == 1 の場合、`readVersionInBoot` / `JumpApp` の offset を推測せず、`docs/PROTOCOL_SPEC.md` / Issue を確認して停止した
- [ ] OTA 失敗時: BLE は `"8bt"` へ再接続 → `readPID` → 全セクタ消去 → 再書き込みで復旧できることを確認した。USB は再列挙 / 再接続で Bootloader に入り直す（BLE の `"8bt"` を流用しない）。再接続だけでは復旧とみなさない
- [ ] 設定書き込み: `cmd 0x0001` の CRC / offset / chunking（`total_len` は部分書込でも `0x0AD0`、部分長は `chunk_len`。マクロヘッダ `0xD8` を除く）、`Save 0x0006` の ACK（BLE `0x0067` / USB `0x0123`）を確認した
- [ ] マクロ: ヘッダ `0x0001`（`subcmd 0x0000`, `offset=0x68C + slot*0xD8`, `len=total_len=0xD8`）→ `Save 0x0006`（BLE `0x0067`）→ `Erase 0x0104`（`offset=macro_idx<<12`, `total_len=chunk_len=0x1000`, payload なし）→ `Write 0x0103`（`offset=(macro_idx<<12)+chunk_index*chunk_size`, `chunk_size=32/64B（32B アライン）`, `total_len=align32(max_steps*10)`, 各チャンク CRC16, 最終チャンクは 32B アライン長）→ 最終 `Save 0x0006` の公式順序を確認した。`macro restore` は slot 単位でヘッダ + Save を 1 回、全マクロの後に最終 Save を 1 回だけ送る
- [ ] マクロヘッダは **`total_len=0xD8`** で送り、**Header → Save → Erase → Write → Save** の公式順序を確認した（旧 Erase → Write → Header(total_len=0x0AD0) → Save ではない）
- [ ] マクロ Erase は `total_len = chunk_len = 0x1000`（wire header `+0x04` は公式実装の設定値）、ペイロードなし（`len=0`）。`total_len=0x1000` は消去サイズであり、ペイロード長ではない
- [ ] マクロ ACK: `Erase` / `Write` / ヘッダは 2s、`Save` は **3s・リトライなし**（BLE の正式値は未文書化。USB HID は 3s）。OTA の 10s erase / 1s chunk / 64B 固定をマクロに適用していない
- [ ] マクロ `Write 0x0103` の `chunk_size` が BLE `0x40` / USB HID `0x20` を超える場合、拒否または上限へクランプし、上限超過をテストした
- [ ] マクロ 3 スロット × 4 マクロのバックアップと復元を確認した。`max_steps` は `0x68C + slot * 0xD8 + 0x08 + macro_idx * 0x34 + 0x22`（`u16`）から読んだ

#### C. docs / チェックリストのみの場合

コード変更がない場合はこちらを記入してください。mock/実機の項目は `N/A + 理由` とし、検証したことにしないでください。

- [ ] `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` と突合した
- [ ] 相対リンクチェックを実行した（0 broken）
- [ ] 変更した用語・コマンド・offset を grep で横断確認した
- [ ] 変更後の手順をシナリオとして通読した（正常系・失敗系・中断/再試行）
- [ ] mock/実機の検証項目は `N/A + 理由` を記録した（例: docs-only のため mock 不要）

#### D. ゲート / セキュリティ / リリース

- [ ] `bash -n scripts/post-review-status.sh`
- [ ] `bash -n scripts/test-post-review-status.sh`
- [ ] `bash scripts/test-post-review-status.sh`
- [ ] `python3 -m json.tool .github/rulesets/main-protection.json`
- [ ] `command -v jq`（jq 必須。gh 2.105.0 は `--slurp` と `--jq` / `--template` を併用できない）
- [ ] `.github/rulesets/main-protection.json`（canonical）と live ruleset `27785806` の `conditions` / `bypass_actors` / `required_status_checks` を監査した。`current_user_can_bypass` は live にのみ存在するため live 側で確認した
- [ ] `required_approving_review_count` を確認した（現状 0。2 人目のプリンシパルがいる場合の目標 1）
- [ ] `gh pr view --json statusCheckRollup` と `gh api --paginate --slurp .../statuses | jq ...` / `.../check-runs?filter=all | jq ...` で status と check run（`id` / `app.slug` / `app.id` / `started_at` / `completed_at`）を監査した
- [ ] 最新 check run は `id` の最大で選んだ（`started_at` / `completed_at` は queued / in-progress で null）
- [ ] `STATE=success` は `success` / `skipped` / `neutral` / なし、`STATE=failure` は gate-satisfying な conclusion が残っていないことを確認した（`skipped` / `neutral` は suspicious として app/source を確認）
- [ ] `success` 投稿後に late `CHANGES_REQUESTED` が出た場合、同じ SHA に直ちに `review-gate=failure` を投稿するルールを確認した（スクリプトの `reviewDecision` 確認は best-effort）
- [ ] セキュリティ境界 / review-gate script を検証するテスト（stale-SHA refusal、failure overwrite、state-aware check-run audit、late CHANGES_REQUESTED、`--slurp` + `--jq` 拒否など）の変更・削除を確認した
- [ ] workflow の権限、status / check-run 偽装の可否、secret の扱いを確認した
- [ ] action の pin、署名、配布物、ロールバック手順を確認した
- [ ] リリースを含む場合、バージョン（Cargo.toml / ファームウェアバージョン）を確認した

#### E. セキュリティコード

- [ ] authn/authz、暗号/署名検証、secret の扱い、trust-boundary の入力検証、権限変更を確認した
- [ ] TLS / crypto / auth に影響する依存更新を確認した
- [ ] 該当しない場合は `N/A + 理由` を記録した
