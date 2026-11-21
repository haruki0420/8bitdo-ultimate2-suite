# 独立レビュー Tier

PR のリスクに応じてレビューの深さを変えます。目的は「docs に重すぎるレビューをしない」「危険な差分に軽すぎるレビューをしない」ことです。

## 0. 正本と適用範囲

- **Tier 判定・パス別トリガー・レビュー checklist の正本はこのファイル**です。`AGENTS.md` / `CONTRIBUTING.md` は要約、PR テンプレートは実チェックリストの写しです。矛盾した場合はこのファイルを優先してください。
- **ゲート実装の正本**は `.github/rulesets/main-protection.json`（live: `main-protection`, id `27785806`）、**status 投稿の正本**は `scripts/post-review-status.sh` です。`docs/BRANCH_PROTECTION.md` は説明と監査コマンドを持ち、実装を重複させません。
- ruleset が強制するのは **`review-gate` status の存在** までです。Tier 判定・レビュー実施・故障リストの内容は手続きであり、write 権限者は迂回できます（§10）。

## 1. Tier 表

| Tier | 対象 | レビュー | 故障リスト |
| --- | --- | --- | --- |
| **1** | コメント・整形、通常の docs（規範仕様・ゲート/プロセス文書を除く）、**純粋なテスト追加のみ**（write-path / Flash / OTA / backup を検証するものを除く）。**振る舞いは変えない** | 1 名。diff + テスト + リンク/コマンド | 純粋なテスト追加はレビュアー側で 1〜2 件の故障注入を記録。振る舞いを変えない docs/コメント/整形は不要 |
| **2** | 読み取り専用コード、読み取り系ビルダー、プロセス文書の散文、規範仕様の編集（安全-critical 以外）、依存・ツールチェーン・リポジトリ衛生、テストの変更/削除（write-path を検証するものを除く）、リファクタ | 2 名（仕様検収 + 実装/回帰）。変更ファイル + 直接の呼び出し側 + テスト | コード 3〜5 件。docs/プロセスは検出方法 3〜5 件。混合 PR は両方 |
| **3** | 書き込み経路 / Flash / OTA / backup-restore / CRC / offset / framing / マクロ step、セキュリティの振る舞い、ゲート実装・チェックリスト、リリース、大規模な混合差分 | 2 名 + 別セッションの敵対的パス（計 3 セッション）。関連ファイル全体 + 呼び出し側 + モック | コード 10 件以上。docs/ゲートは検出方法 5 件以上。混合 PR は両方 |

## 2. 判定ルール

- **混合 PR は最大 Tier**（docs + code なら code の Tier）。混合 PR の故障リストは、コード 10 件以上と docs/ゲート 5 件以上の **両方** を満たす。
- 著者が PR で Tier を提案し、**レビュアーは昇格できる。降格はできない。**
- 迷ったら 1 段上に倒す。
- 行数は目安にしない。10 行の CRC 変更は Tier 3、1000 行の README 整形は Tier 1。

### Content-based override（パスより内容を優先）

ファイル名やパスが Tier 1/2 の一覧にあっても、次のいずれかに該当する変更は **Tier 3** です。ファイル名や行数では下げられません。

- **レビュアーが確認する内容**を変える: PR テンプレートのチェックリスト項目、レビュー観点、故障リストの最低数、N/A の条件。
- **コントリビューターが実行する手順**を変える: `CONTRIBUTING.md` の手順、バックアップ/復元手順、リリース手順。
- **required check / status コマンド / ruleset / workflow / ゲート手順**を変える。
- **安全手順**（Flash / OTA / backup / restore / マクロ Flash / ブリック復旧）を変える。
- チェックリスト項目や安全手順を**削除・無効化**する、または適用条件を広げる。

例: PR テンプレートから OTA/バックアップのチェックボックスを消す、`CONTRIBUTING.md` のレビュー手順を短縮する、`docs/REVIEW_TIERS.md` の故障リスト最低数を下げる、`scripts/post-review-status.sh` の SHA pin を外す。これらはすべて Tier 3 です。

### Write-path rule

デバイスへの書き込み・Flash・OTA・backup/restore について、次のいずれかを変えうる変更は、パスにかかわらず **Tier 3** です。

- 書き込むバイト、offset、CRC、フレーミング、コマンド/subcmd、`total_len`、チャンク分割。
- 書き込みの順序、タイミング、タイムアウト、ACK 待ち、リトライ、エラー処理、中断時の復旧。
- 上記を検証するテスト、mock transport、golden vector、CRC テストベクタ。

例:

- `config_bt2.rs` の setter（offset / CRC / マクロ以外でも、書き込みバイトを変えるもの）
- `keymap.rs` のビットマスク（書き込みデータに影響するもの）
- mock transport、golden vector、CRC テストベクタ
- build script / native dependency / feature flag（ビルドされる書き込みコードを変えるもの）
- `apps/ios/**` / `apps/android/**` の native write path
- 将来の `crates/eightbitdo-gui` など、デバイス書き込みを引き起こしうる新規 crate

読み取り専用のビルダー（`read_config` など）は Tier 2 のままでよいです。

### 自己修正ルール

Tier 定義、パス別トリガー、レビュアー数、quorum、故障リストの最低数、チェックリスト、required check、ruleset、status コマンド、ゲート手順を変更する PR は **Tier 3** です。Tier 2 のレビューでポリシーを弱められないようにします。

### ダウングレードの制限

Tier 3 のパスに対するコメント・整形のみの変更は、次の **両方** を満たす場合に限り Tier 1/2 にできます。

- `git diff -w --exit-code "$(git merge-base HEAD origin/main)"..HEAD` が空（空白無視で差分なし）、または `cargo fmt` のみのコミットである。コミット範囲と `git diff --stat` で確認する。
- レビュアーが「意味の変更なし」を確認し、その証跡（コマンド出力またはコミット URL）を PR に記録した。

それ以外は Tier 3 のままです。

### その他

- revert / hotfix / security fix は、触ったパスの最大 Tier を継承する。trust-boundary の振る舞いを変える security fix は Tier 3。緊急時の降格はしない。
- push によってリスク面が変わった場合（docs PR にコードが混ざった等）は、**再 Tier 判定と再レビュー**を行う。
- 「大規模」は行数ではなく、Tier 3 のリスク領域をまたぐかで判断する。

## 3. パス別トリガー

### Tier 1 の例

- `README.md` など通常の docs の散文・リンク、コメント、整形、純粋なテスト追加（write-path / Flash / OTA / backup を検証しないもの）。§2 の content-based override に該当しないこと。

### Tier 2 以上（パス別の基本 Tier。内容による上書きは §2）

以下のパスは基本 Tier です。§2 の content-based override / write-path rule が常に優先します。

- `AGENTS.md` / `CLAUDE.md` / `CONTRIBUTING.md` / `docs/REVIEW_TIERS.md` / `docs/BRANCH_PROTECTION.md`
  - **散文・リンク・誤字のみ Tier 2。** チェックリスト項目、レビュアーが確認する内容、コントリビューターの手順、required check、status コマンド、ゲート手順、安全手順は **Tier 3**（§2）。
- `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` / `docs/IMPLEMENTATION_PLAN.md`
  - **CRC / offset / flash / OTA / framing / 書き込みコマンド・subcmd / `total_len` / Save 手順 / チャンク分割の変更は Tier 3。** 誤字・リンク・表現の修正は Tier 2。
- `.github/pull_request_template.md`
  - **散文・リンクのみ Tier 2。** チェックリスト項目・手順・N/A 条件は Tier 3。
- `Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml`
  - 依存・ツールチェーンの更新のみ Tier 2。build script / native dependency / feature flag が書き込みコードに影響する場合は Tier 3。
- `.gitignore`（公式バイナリの除外が維持されているか確認する）
- `.github/dependabot.yml`

### Tier 3

- `.github/workflows/**` / `.github/CODEOWNERS` / `.github/rulesets/**`
- `scripts/post-review-status.sh`、required check 名、ゲート手順の実装
- `crates/eightbitdo-protocol/src/crc.rs`
- `crates/eightbitdo-protocol/src/header.rs`（フレーミング）
- `crates/eightbitdo-protocol/src/macro_data.rs`
- `crates/eightbitdo-protocol/src/config_bt2.rs` の offset / CRC / マクロ Flash 関連、および書き込みバイトを変える setter
- `crates/eightbitdo-protocol/src/commands.rs` の設定書き込み / マクロ / OTA コマンド
- `crates/eightbitdo-protocol/src/keymap.rs` のビットマスク（書き込みデータに影響する変更）
- mock transport / golden vector / safety-critical test vector
- `crates/eightbitdo-core/**` の書き込み・セッション・OTA・backup/restore
- `crates/eightbitdo-cli/**` の書き込みコマンド
- `crates/eightbitdo-uniffi/**` の書き込み / OTA 公開 API
- `apps/ios/**` / `apps/android/**` の native write path
- `packaging/**` / リリース設定 / 署名
- `unsafe` / FFI / ファームウェアのダウンロード・検証 / secrets / build script
- `AGENTS.md` / `CONTRIBUTING.md` / `docs/REVIEW_TIERS.md` / `docs/BRANCH_PROTECTION.md` / PR テンプレートの **安全-critical なチェックリスト項目・手順**（Flash 順序、backup/restore、OTA 復旧、マクロ Flash、ゲート手順）
- 上記の領域に**新規ファイルを追加**する場合、または新規 crate / workspace member が書き込み・Flash・OTA・backup/restore・ゲート変更を引き起こせる場合

### 判定例

| 変更 | Tier | 理由 |
| --- | --- | --- |
| PR テンプレートの誤字・リンク修正 | 2 | 散文・リンクのみ |
| PR テンプレートから OTA/バックアップのチェック項目を削除 | 3 | content-based override（レビュアーが確認する内容の削除） |
| `CONTRIBUTING.md` のレビュー手順を短縮 | 3 | content-based override（コントリビューターの手順） |
| `docs/REVIEW_TIERS.md` の故障リスト最低数を変更 | 3 | 自己修正ルール |
| `README.md` の文言修正 | 1 | 通常の docs |
| `docs/PROTOCOL_SPEC.md` の誤字修正 | 2 | 規範仕様の非安全-critical な編集 |
| `docs/PROTOCOL_SPEC.md` の offset 変更 | 3 | 安全-critical な仕様変更 |
| `config_bt2.rs` の読み取り専用 builder | 2 | 読み取り専用コード |
| mock transport の変更 | 3 | write-path rule |

## 4. Tier 1（軽量・振る舞いを変えない）

- **Tier 1 は振る舞いを変えない変更のみ**です。コメント、整形、通常の docs、**純粋なテスト追加**（write-path / Flash / OTA / backup を検証するものを除く）が対象です。
- コードのリファクタは Tier 1 にしない（Tier 2 以上）。
- テストの変更・削除は Tier 1 にしない（Tier 2。write-path を検証するものは Tier 3）。
- 規範仕様（`PROTOCOL_SPEC` / `STRUCT` / `IMPLEMENTATION_PLAN`）、ゲート文書、プロセス文書は Tier 1 にしない。
- 振る舞いを変える docs は Tier 2/3 とし、故障リストを必須にする。
- レビュアー 1 名。diff、関連テスト、リンク、コマンド例を確認する。
- 振る舞いを変えない docs/コメント/整形は、故障注入なし。PR テンプレートの故障リストと故障注入記録は削除できる。
- 純粋なテスト追加（write-path / Flash / OTA / backup を検証しないもの）では、レビュアー側で 1〜2 件の故障を注入し「テストが落ちるか」を確認する。結果は **PR 本文またはレビューコメント** に「注入した故障 / 検出したテスト / 結果 / 証跡」を記録する。純粋なテスト追加の Tier 1 は、この記録セクションを削除できない。write-path / Flash / OTA / backup を検証するテストは Tier 3（§2 Write-path rule）。

## 5. Tier 2（標準）

- レビュアー 2 名。役割を分ける。
  - **仕様検収**: `docs/` との一致、オフセット、CRC、エンディアン、コマンド引数。
  - **実装/回帰**: API、エラー処理、境界値、呼び出し側、テストの抜け。
- 範囲は「変更ファイル + 直接の呼び出し側 + テスト」。変更ファイルだけを読んで終わらせない。
- 故障リストを 3〜5 件作り、それぞれ「検出方法」「結果」「証跡」を記録する。
  - コード: テストで検出する。
  - docs/プロセス: リンクチェック、grep、コマンド実行、シナリオ通読で検出する（3〜5 件）。
- 証跡はテスト名、コマンド出力、URL のいずれかで残す。

## 6. Tier 3（安全-critical）

- レビュアー 2 名 + **別セッションの敵対的パス**（合計 3 セッション）。
  - 敵対的パスは、正常系の確認ではなく「仕様の矛盾」「境界値」「失敗系」「中断・再試行」「予約バイト破壊」を探す。
  - コスト制約がある場合も、実装レビュアーとは別のセッションで敵対的パスを 1 パス追加する（2 名 + 1 パスを維持）。
- 範囲は関連ファイル全体 + 呼び出し側 + モックトランスポート。
- 故障リストをコード 10 件以上、docs/ゲート 5 件以上作る。可能なら `cargo-mutants` などの自動変異も併用する。各項目に証跡を付ける。
- **Tier 3 のサブセクションは削除しない。** 該当しない項目は `N/A + 理由` を記録する。
- 実機書き込みを含む場合は、次の **両方** をバックアップし、ファイル名（path）と SHA-256 を記録する。
  - 2768B 設定 (`record_ultimatebt2`)
  - マクロ 4×3 スロットのステップデータ（2768B には含まれない。`slot 0..2` × `macro_idx 0..3` の 12 領域。`macro_idx << 12` の 4KB セクタ）
- 復元テスト、または文書化された復元手順を確認し、結果を記録する。復元後は 2768B と各マクロを読み戻し、SHA-256 を比較する。
- OTA を含む場合は、OTA イメージの SHA-256 と Bootloader 復旧結果を記録する。
- すべての故障に証跡（テスト名 / コマンド出力 / URL）を付け、**Tier をレビューした head SHA で再評価**する。

### 6.1 コード変更を含む場合

- モックトランスポートでパケット列・CRC・チャンク境界・リトライ動作を検証する。
- OTA / Flash は 4KB セクタ境界、10 秒 erase タイムアウト、erase ACK 待ち、64B 固定チャンク（最終チャンクを含む）を確認する。
- 実機書き込みは §6 のバックアップと復元を必須にする。
- 可能なら `cargo-mutants` などの自動変異も併用する。

### 6.2 docs / チェックリストのみの場合

コード変更がない docs-only の PR は、次の方法で検証する。**mock/実機の項目は `N/A + 理由` とし、検証したことにしない。**

- `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` との突合。
- 相対リンクチェック（0 broken）。
- 変更した用語・コマンド・offset の grep 横断確認。
- 正常系・失敗系・中断/再試行を含むシナリオ通読。

リリース・ゲート・セキュリティのみの変更は、ruleset 監査・権限監査・秘密情報チェックで代替する。

### 6.3 OTA チェックリスト

**前提（App Mode → Bootloader）**

```text
enterBoot 0x000C (subcmd 0x0067, total_len 4, offset 4)
→ BLE 切断 → "8bt" (FF20/FF21) へ再接続（CCCD + MTU 設定）
→ readPID 0x0008 (subcmd 0x0067, total_len 2)
→ readVersionInBoot 0x0005 (subcmd 0x0067, total_len 4,
   offset は gamepad_type に応じる。type 1 は 0x01017040、5s)
```

`readPID` の結果で `gamepad_type` を判定し、`WriteAPPID` の要否を決める。

**書き込み**

```text
EraseFlash 0x0004 (4KB セクタ単位、10 秒タイムアウト、ACK を待つ)
→ WriteFirmware 0x0003
   (total_len = header[8]（イメージ長）、chunk_len = 64B 固定、CRC-16/MODBUS 付き、1 秒)
→ WriteVersion 0x00C4
   (24B ヘッダ、chunk_len = 0x18、total_len = header[8]、3 秒)
→ WriteFirmwareVersion 0x0002
   (gamepad_type == 1: offset 0x01017040 /
    それ以外: offset 0x08014034、4B + CRC16)
→ WriteAPPID 0x0002
   (gamepad_type != 1 のみ、offset 0x0801401C、12B + CRC16、5 秒)
→ JumpApp 0x0006 (offset 0x01018000、5 秒)
```

- 4KB セクタ境界、64B チャンク、**最終チャンクの 64B 固定**、CRC の範囲を確認する。
- `EraseFlash` の ACK 前に `WriteFirmware` を送らない。
- 失敗時は `"8bt"` へ再接続し、`readPID` → 全セクタ消去 → 再書き込みで復旧する。「再接続だけ」で復旧したとみなさない。
- `WriteFirmwareVersion` 完了前に電源断しても Bootloader に留まるため再試行できることを確認する。
- マクロの `Erase 0x0104 → Write 0x0103 → ヘッダ 0x0001 → Save 0x0006` と混同しない。

### 6.4 マクロ Flash チェックリスト

```text
Erase 0x0104
  (subcmd = (mode << 8) | slot, offset = macro_idx << 12,
   total_len = 0x1000, ペイロードなし / chunk_len 0)
→ Write 0x0103
  (subcmd = (mode << 8) | slot, offset = macro_idx << 12,
   total_len = align32(max_steps * 10), 64B チャンク,
   32B アライン、各チャンク CRC-16/MODBUS)
→ マクロヘッダ更新 0x0001
  (subcmd 0x0000, offset = 0x68C + slot * 0xD8, len 0xD8,
   total_len 0xAD0、ANSI_CRC_16_sh を再計算)
→ Save 0x0006
  (BLE: subcmd 0x0067 / USB: subcmd 0x0123)
```

- `align32(x) = (x + 31) & ~31 = ((x + 31) / 32) * 32`、`offset = macro_idx << 12`、4KB セクタを確認する。
- バックアップは 2768B 設定から `max_steps` を読み、各 `(slot, macro_idx)` を `Read 0x0102`（`subcmd=(mode<<8)|slot`, `offset=macro_idx<<12`, `total_len=max_steps*10`）で取得する。
- マクロ 4×3（`slot 0..2` × `macro_idx 0..3`）のバックアップと復元を確認する。
- モックでパケット列・CRC・チャンク境界を検証する。
- BLE と USB で Save の subcmd が異なる（`0x0067` / `0x0123`）ことを確認する。

### 6.5 ゲート / セキュリティ / リリース

- **ゲート**: `.github/rulesets/main-protection.json`（canonical）と live ruleset `27785806` の `conditions` / `bypass_actors` / `current_user_can_bypass` / `required_status_checks` / `required_approving_review_count`（現在 0。2 人目のプリンシパルがいる場合の目標 1）を監査する。`gh pr view --json statusCheckRollup`、`commits/$SHA/statuses`、`commits/$SHA/check-runs` で status と check run を確認する。
- **セキュリティ**: workflow 権限、status / check-run 偽装の可否、secret の扱い、action の pin、署名検証。
- **リリース**: バージョン（Cargo.toml / ファームウェアバージョン）、署名、配布物、ロールバック。

## 7. レビュアーの独立性と合議

- レビュアーは **著者とも他のレビュアーとも別セッション** でレビューする。互いの指摘を見る前に、同じ diff / SHA に対して独立に判定する。
- 各レビュアーは **自分の行/コメント** を書き、セッション ID、レビューした SHA、判定、レビューコメントの URL を残す。リードレビュアーは他のレビュアーの行を代筆せず、全員のコメント URL と SHA を確認する。
- Tier 2/3 は **全レビュアーが同じ head SHA で「マージ可」** を出すまでマージしない。
- 1 名でも blocker / 要修正を出したら `review-gate` の `success` を付けない。必要なら `failure` を付ける。
- **リードレビュアー**（仕様検収レビュアー）が、全員の判定とコメント URL を確認してから status を付ける。Tier 1 はレビュアー本人が付ける。
- 判定が割れた場合は、Tier を 1 段上げるか、追加のレビューを入れる。Tier 3 では **4 名目の独立 adjudicator** を追加する。blocker は、blocking reviewer が撤回するか、adjudication の理由が PR に記録されるまで残る。
- 同じ SHA の `failure` を、新しいレビューなしに `success` で上書きしない。誤りだった場合は理由を PR に残す。
- レビュアーは著者であってはならず、Tier 2/3 のレビュアー同士も別セッションでなければならない。

## 8. 故障リスト

件数を満たすことではなく、**検出したい故障を先に列挙**します。カウントはコードの下限です。docs/ゲートはテストの有無ではなく、検出方法（リンクチェック、grep、コマンド実行、目視）で 3〜5 / 5+ 件を記録します。混合 PR はコード 10 件以上と docs/ゲート 5 件以上の両方を満たします。各項目には証跡を付けます。

**以下の表は例であり、実際のテスト結果ではありません。** PR では自分の故障を記入してください。

| 故障 | 検出方法（テスト / コマンド / 目視） | 結果 | 残存リスク | 証跡 |
| --- | --- | --- | --- | --- |
| CRC の多項式を `0x8408` ↔ `0xA001` で取り違える | CRC テストベクタ | 例: 検出 | なし | テスト名 |
| `0x92C + slot * 0x0C` を `+ slot * 4` にする | SixAxis オフセットテスト | 例: 検出 | なし | テスト名 |
| `align32` を `align16` にする | マクロ長テスト | 例: 検出 | なし | テスト名 |
| RT/LT を入れ替える | マクロステップのバイト配置テスト | 例: 検出 | なし | テスト名 |
| BLE RX に `0x04` プレフィックスを要求する | 受信フレーミングテスト | 例: 検出 | なし | テスト名 |
| `cmd 0x0103` の 64B クランプを外す | チャンク分割テスト | 例: 検出 | なし | テスト名 |
| OTA の最終チャンクを 64B 未満で送る | ファームウェア分割テスト | **未検出** | 最終チャンクの 64B パディング未実装。Issue #2 に記録済み（修正 PR 未作成） | Issue #2 |
| docs の相対リンクを壊す | リンクチェック | 例: 検出 | なし | コマンド出力 |
| Tier 表の記述が 2 箇所でずれる | grep / 目視 | 例: 検出 | なし | grep 出力 |
| セキュリティトリガーが抜ける | チェックリスト照合 | 例: 検出 | なし | 照合メモ |

検出できない場合は、テストを追加するか、Tier を上げるか、残存リスクとして PR に明記します。

## 9. レビュー記録

- レビュー結果は PR のレビューコメントに残す。各レビュアーは自分の行を書き、リードは代筆しない。
- **`scripts/post-review-status.sh <PR> <reviewed-sha> <TIER> [success|failure]`** で、レビューした head SHA に `review-gate` status を付ける。スクリプトは Tier を検証し、投稿直前に head SHA を固定し、投稿後に全 status と check run を監査する。
- status の description に Tier と SHA を含める（例: `Tier 3 review of abc1234`）。
- 修正を push したら新しい SHA で再レビューする。前の SHA の status は引き継がれない。
- Tier、故障リスト、レビュアー表は PR 本文に残す。Tier は各 push 後に再評価する。

## 10. 強制範囲

`main-protection` ruleset が強制するのは **`review-gate` status の存在** までです。Tier 判定・レビュー実施・故障リストの内容は手続きであり、write 権限者は迂回できます。同じリポジトリの PR ワークフローは `statuses: write` や `checks: write` で偽装できる可能性があり、check run は commit status とは別に監査する必要があります。`.github/rulesets/main-protection.json` は canonical copy ですが、admin による out-of-band 変更は追加の PAT なしでは定期検知できません。強制力を上げる方法と監査上の限界は [docs/BRANCH_PROTECTION.md](BRANCH_PROTECTION.md) の「セキュリティ上の限界」を参照してください。
