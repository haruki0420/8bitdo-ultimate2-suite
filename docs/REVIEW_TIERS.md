# 独立レビュー Tier

PR のリスクに応じてレビューの深さを変えます。目的は「docs に重すぎるレビューをしない」「危険な差分に軽すぎるレビューをしない」ことです。

## 0. 正本と適用範囲

- **Tier 判定・パス別トリガー・レビュー checklist の正本はこのファイル**です。`AGENTS.md` / `CONTRIBUTING.md` は要約、PR テンプレートは実チェックリストの写しです。矛盾した場合はこのファイルを優先してください。
- **ゲート実装の正本**は `.github/rulesets/main-protection.json`（live: `main-protection`, id `27785806`）、**status 投稿の正本**は `scripts/post-review-status.sh` です。`docs/BRANCH_PROTECTION.md` は説明と監査コマンドを持ち、実装を重複させません。
- ruleset が強制するのは **`review-gate` required check の成功（commit status または check run）** までです。Tier 判定・レビュー実施・故障リストの内容は手続きであり、write 権限者は迂回できます（§10）。

## 1. Tier 表

| Tier | 対象 | レビュー | 故障リスト |
| --- | --- | --- | --- |
| **1** | **Tier 3 パス以外**のコメント・整形、通常の docs（規範仕様・ゲート/プロセス文書を除く）、**純粋なテスト追加のみ**（write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するものを除く）。**振る舞いは変えない** | 1 名。diff + テスト + リンク/コマンド | 純粋なテスト追加はレビュアー側で 1〜2 件の故障注入を記録。**Tier 3 パス以外**の振る舞いを変えない docs/コメント/整形は不要。Tier 3 パスの整形は A1 の条件を満たす場合のみ降格、コメントは A2 で常に Tier 3 |
| **2** | 読み取り専用コード（セキュリティの振る舞いを除く）、読み取り系ビルダー、プロセス文書の散文、規範仕様の編集（誤字・リンク・表現、および安全-critical でない明確化）、依存・ツールチェーン（write-path / セキュリティに影響しないもの）・リポジトリ衛生、テストの変更/削除（Tier 3 パス、write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するものを除く）、リファクタ（write-path / Flash / OTA / backup / CRC / セキュリティの振る舞いに触れないもの） | 2 名（仕様検収 + 実装/回帰）。diff（不足時のみ変更ファイル全体・呼び出し元・契約・周辺） | コード 3 件以上（目安 3–5、見つかった故障はすべて記録）。docs/プロセスは検出方法 3 件以上（目安 3–5）。Tier 2 混合 PR はコード 3 件以上 / docs 3 件以上 |
| **3** | 書き込み経路 / Flash / OTA / backup-restore / CRC / offset / framing / マクロ step、セキュリティの振る舞い（authn/authz、crypto/signature、secrets、trust-boundary、permissions）、ゲート実装・チェックリスト、リリース、大規模な混合差分 | 2 名 + 別セッションの敵対的パス（計 3 セッション）。diff（不足時のみ変更ファイル全体・呼び出し元・契約・周辺） | コード 10 件以上。docs/ゲートは検出方法 5 件以上。Tier 3 混合 PR はコード 10 件以上 / docs 5 件以上 |

## 2. 判定ルール

- **混合 PR は最大 Tier**（code と docs の Tier の最大。例: code Tier 3 / docs Tier 2 → Tier 3、code Tier 2 / docs Tier 3 → Tier 3）。混合 PR の故障リストは、その最大 Tier の要件を **両方** に適用する: **Tier 2 混合 = コード 3 件以上 + docs 3 件以上、Tier 3 混合 = コード 10 件以上 + docs/ゲート 5 件以上**。（最大 Tier の件数を両側に適用するため、`code Tier 2 / docs Tier 3` の混合はコード 10 件以上が必要。意図的に厳しくしている）
- 著者が PR で Tier を提案し、**レビュアーは昇格できる。原則として降格できない。例外は §2 のダウングレード制限のみ。**
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
- `btleplug` / `hidapi` など BLE/USB 依存の更新（write timing・MTU/chunking・retry・error handling が変わるもの）
- build script / native dependency / feature flag（ビルドされる書き込みコードを変えるもの）
- `apps/ios/**` / `apps/android/**` の native write path
- 将来の `crates/eightbitdo-gui` など、デバイス書き込みを引き起こしうる新規 crate

読み取り専用のビルダー（`read_config` など）は Tier 2 のままでよいです。ただし、認証/認可、暗号/署名検証、secret の扱い、trust-boundary の入力検証、権限変更などのセキュリティの振る舞いを含む場合は Tier 3 です。

### Security behavior rule

認証/認可、暗号/署名検証、secret の扱い、trust-boundary の入力検証、権限変更、またはそれらに影響する依存更新は、パスにかかわらず **Tier 3** です。読み取り専用コード、通常の依存更新、docs に見えても、セキュリティの振る舞いを変える場合は Tier 3 とします。

例:

- 認証トークン / 署名検証 / secret の読み込み・保存
- TLS / crypto / auth に影響する依存更新
- 権限チェック、trust-boundary の入力検証、sandbox / capability の変更
- セキュリティ境界を検証するテスト / mock の変更・削除

### 自己修正ルール

Tier 定義、パス別トリガー、レビュアー数、quorum、故障リストの最低数、チェックリスト、required check、ruleset、status コマンド、ゲート手順を変更する PR は **Tier 3** です。Tier 2 のレビューでポリシーを弱められないようにします。

### ダウングレードの制限

Tier 3 のパスに対する変更は、原則として Tier 3 のままです。ダウングレードは次の **スクリーニング** を通過し、レビュアーが full diff を確認した場合に限ります。スクリーニングは証跡ではなく、証跡はレビュアーが記録します。

- **(A1) コードの整形のみ**: `git diff -w` は使わない（文字列・char・複数行文字列内の空白を見逃すため）。次の手順を再現可能な形で実行し、base commit を整形した tree と head の tree が **完全一致** することを確認する。一致しない場合は Tier 3 のまま。`cargo fmt --all -- --check` だけでは「head が整形済み」しか示さず、この証明にはならない。
  ```bash
  BASE=$(git merge-base origin/main HEAD)
  WORK=$(mktemp -d)
  git worktree add "$WORK" "$BASE"
  TREE=$(cd "$WORK" && cargo fmt --all && git add -A && git write-tree)
  test "$TREE" = "$(git rev-parse HEAD^{tree})"
  git worktree remove --force "$WORK"
  ```
  `--force` は `cargo fmt` で dirty になった **この一時 worktree の後片付け専用**。他の作業 worktree には使わない。

- **(A2) コードコメントのみ**: 例外は設けない。コメントのみの変更は Tier 3 のまま（token-aware な full diff 確認が必要で、`*` などの prefix heuristic は使わない）。A2 をダウングレード経路として扱わない。
- **(A3) ドキュメント**: 非規範の散文のみを対象とし、HTML コメント・リンク参照定義・コードフェンスの info string など **レンダリングで見えない内容を含まない** ものに限る。レンダリング結果の比較だけでは不十分なため、base..head の **ソースレベルの diff** を確認する。次のいずれかを含む変更は意味の変更として扱い、Tier 2/3 のまま（Tier 1 にしない）。
  - `<!-- ... -->` など HTML コメントの追加・削除・変更
  - `[label]: url` 形式のリンク参照定義
  - コードフェンスの info string（`rust` / `text` など）の変更
  - 規範仕様・ゲート・プロセス文書の本文
  規範仕様・ゲート・プロセス文書は Tier 1 にはしない（最低 Tier 2、安全-critical は Tier 3）。
- **(B) レビュアーの確認**: レビュアーは `-w` だけでなく full diff を読み、意味の変更がないことを確認し、証跡（base/tree hash、コマンド出力、diff URL）を PR に記録する。

それ以外は Tier 3 のままです。

### その他

- revert / hotfix / security fix は、触ったパスの最大 Tier を継承する。trust-boundary の振る舞いを変える security fix は Tier 3。緊急時の降格はしない。
- push によってリスク面が変わった場合（docs PR にコードが混ざった等）は、**再 Tier 判定と再レビュー**を行う。
- 「大規模」は行数ではなく、Tier 3 のリスク領域をまたぐかで判断する。

## 3. パス別トリガー

### Tier 1 の例

- `README.md` など通常の docs の散文・リンク、Tier 3 パス以外のコメント・整形、純粋なテスト追加（write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証しないもの）。§2 の content-based override に該当しないこと。

### Tier 2 以上（パス別の基本 Tier。内容による上書きは §2）

以下のパスは基本 Tier です。§2 の content-based override / write-path rule が常に優先します。

- `AGENTS.md` / `CLAUDE.md` / `CONTRIBUTING.md` / `docs/REVIEW_TIERS.md` / `docs/BRANCH_PROTECTION.md`
  - **散文・リンク・誤字のみ Tier 2。** チェックリスト項目、レビュアーが確認する内容、コントリビューターの手順、required check、status コマンド、ゲート手順、安全手順は **Tier 3**（§2）。
- `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` / `docs/IMPLEMENTATION_PLAN.md`
  - **CRC / offset / flash / OTA / framing / 書き込みコマンド・subcmd / `total_len` / Save 手順 / チャンク分割の変更は Tier 3。** 安全-critical な内容に触れない誤字・リンク・表現、および安全-critical でない明確化は Tier 2。安全手順・offset・CRC・OTA/Flash/backup の記述変更は Tier 3。
- `.github/pull_request_template.md`
  - **散文・リンクのみ Tier 2。** チェックリスト項目・手順・N/A 条件は Tier 3。
- `Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml`
  - 書き込み経路またはセキュリティに影響しうる依存・ツールチェーン更新（例: `btleplug` / `hidapi` の更新で BLE/USB の write timing・MTU/chunking・retry・error handling が変わるもの、TLS/crypto/auth に影響する更新）は Tier 3。影響しない更新のみ Tier 2。build script / native dependency / feature flag が書き込みコードに影響する場合も Tier 3。
- `.gitignore`（公式バイナリ `.apk` / `.exe` / `.dll` / `.so` / firmware の除外を削除・弱体化する変更は Tier 3。それ以外は Tier 2）
- `.github/dependabot.yml`
  - セキュリティ更新の方針（対象 ecosystem、無視・保留、自動マージ条件など）を変える変更は **Tier 3**。それ以外の設定（スケジュール、グループ化、ラベル等）は Tier 2。

### Tier 3

- `.github/workflows/**` / `.github/CODEOWNERS` / `.github/rulesets/**`
- `.gitignore` の公式バイナリ除外削除・弱体化
- `scripts/post-review-status.sh`、required check 名、ゲート手順の実装
- `crates/eightbitdo-protocol/src/crc.rs`
- `crates/eightbitdo-protocol/src/header.rs`（フレーミング）
- `crates/eightbitdo-protocol/src/macro_data.rs`
- `crates/eightbitdo-protocol/src/config_bt2.rs` の offset / CRC / マクロ Flash 関連、および書き込みバイトを変える setter
- `crates/eightbitdo-protocol/src/commands.rs` の設定書き込み / マクロ / OTA コマンド
- `crates/eightbitdo-protocol/src/keymap.rs` のビットマスク（書き込みデータに影響する変更）
- mock transport / golden vector / safety-critical test vector
- セキュリティ境界・review-gate script（stale-SHA refusal、failure overwrite、check-run audit など）を検証するテスト / mock の変更・削除
- `crates/eightbitdo-core/**` の書き込み・セッション・OTA・backup/restore
- `crates/eightbitdo-cli/**` の書き込みコマンド
- `crates/eightbitdo-uniffi/**` の書き込み / OTA 公開 API
- `apps/ios/**` / `apps/android/**` の native write path
- `packaging/**` / リリース設定 / 署名
- `unsafe` / FFI / ファームウェアのダウンロード・検証 / secrets / build script
- 認証 / 認可 / 暗号 / 署名検証 / secret の扱い / trust-boundary の入力検証 / 権限変更 / これらに影響する依存
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

- **Tier 1 は振る舞いを変えない変更のみ**です。**Tier 3 パス以外**のコメント・整形、通常の docs、**純粋なテスト追加**（write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するものを除く）が対象です。
- コードのリファクタは Tier 1 にしない（Tier 2 以上）。
- テストの変更・削除は Tier 1 にしない（Tier 2。Tier 3 パス、write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するものは Tier 3）。
- 規範仕様（`PROTOCOL_SPEC` / `STRUCT` / `IMPLEMENTATION_PLAN`）、ゲート文書、プロセス文書は Tier 1 にはしない（最低 Tier 2、安全-critical は Tier 3）。
- Tier 3 サブセクションは削除せず、該当しないものは `N/A + 理由` を 1 行記入する（全項目を埋める必要はない）。
- 振る舞いを変える docs は Tier 2/3 とし、故障リストを必須にする。
- レビュアー 1 名。diff、関連テスト、リンク、コマンド例を確認する。
- **Tier 3 パス以外**の振る舞いを変えない docs/コメント/整形は、故障注入なし。PR テンプレートの故障リストと故障注入記録は削除できる。
- Tier 3 パスの整形は §2 A1 を満たす場合のみ Tier 1/2 に降格できる。コメントは §2 A2 により常に Tier 3（A2 をダウングレード経路として扱わない）。
- 純粋なテスト追加（write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証しないもの）では、レビュアー側で 1〜2 件の故障を注入し「テストが落ちるか」を確認する。結果は **PR 本文またはレビューコメント** に「注入した故障 / 検出したテスト / 結果 / 証跡」を記録する。純粋なテスト追加の Tier 1 は、この記録セクションを削除できない。write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するテストは Tier 3（§2 / §3）。

## 5. Tier 2（標準）

- レビュアー 2 名。役割を分ける。
  - **仕様検収**: `docs/` との一致、オフセット、CRC、エンディアン、コマンド引数。
  - **実装/回帰**: API、エラー処理、境界値、呼び出し側、テストの抜け。
- 範囲は **PR の diff** を既定とする。diff だけでは判断できない場合に限り、変更ファイル全体、変更したシンボルの呼び出し元、API / スキーマ契約、セキュリティ・認証・データ・リリース workflow・破壊的変更の周辺まで広げる。**全リポジトリ・全履歴・CI の再実行は要求しない。** レビュアーは CI を待たず判定を先に返し、CI はリード／親が確認する。follow-up は前回レビューした SHA からの差分だけを読む。
- Tier 3 パス、write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するテストは Tier 3（§2 / §3）。
- Tier 3 サブセクションは削除せず、該当しないものは `N/A + 理由` を 1 行記入する（全項目を埋める必要はない）。
- 故障リストを 3 件以上（目安 3–5、見つかった故障はすべて記録）作り、それぞれ「検出方法」「結果」「証跡」を記録する。Tier 2 混合 PR はコード 3 件以上 / docs 3 件以上を両方満たす。
  - コード: テストで検出する。
  - docs/プロセス: リンクチェック、grep、コマンド実行、シナリオ通読で検出する（3 件以上、目安 3–5）。
- 証跡はテスト名、コマンド出力、URL、目視メモ、該当箇所の引用、file:line のいずれかで残す。

## 6. Tier 3（安全-critical）

- レビュアー 2 名 + **別セッションの敵対的パス**（合計 3 セッション）。
  - 敵対的パスは、正常系の確認ではなく「仕様の矛盾」「境界値」「失敗系」「中断・再試行」「予約バイト破壊」を探す。
  - コスト制約がある場合も、実装レビュアーとは別のセッションで敵対的パスを 1 パス追加する（2 名 + 1 パスを維持）。
- 範囲は **PR の diff** を既定とする。diff だけでは判断できない場合に限り、変更ファイル全体、変更したシンボルの呼び出し元、API / スキーマ契約、セキュリティ・認証・データ・リリース workflow・破壊的変更の周辺まで広げる。**全リポジトリ・全履歴・CI の再実行は要求しない。** レビュアーは CI を待たず判定を先に返し、CI はリード／親が確認する。follow-up は前回レビューした SHA からの差分だけを読む。
- write-path / Flash / OTA / backup / CRC / セキュリティ境界 / review-gate script の変更は、diff だけでは判断できない典型例として、変更したシンボルの呼び出し元・API / スキーマ契約・関連テストを必ず確認する（Tier 3 の範囲を弱めない）。
- 故障リストをコード 10 件以上、docs/ゲート 5 件以上作る。Tier 3 混合 PR はこの両方（Tier 2 混合はコード 3 件以上 / docs 3 件以上）。可能なら `cargo-mutants` などの自動変異も併用する。各項目に証跡を付ける。
- **PR テンプレートの Tier 3 A-E は PR 本文に存在しなければならない。省略は欠陥（要修正）。** 該当しない項目は `N/A + 理由` を記録する。
- 実機書き込みを含む場合は、次の **両方** をバックアップし、ファイル名（path）と SHA-256 を記録する。
  - 2768B 設定 (`record_ultimatebt2`)
  - マクロ 3 スロット × 4 マクロのステップデータ（2768B には含まれない。`slot 0..2` × `macro_idx 0..3` の 12 領域。`macro_idx << 12` の 4KB セクタ）
- 復元テスト、または文書化された復元手順を確認し、結果を記録する。復元後は全 slot の 216B マクロヘッダと非空マクロの raw を読み戻して比較する。2768B 全体の SHA-256 は device の CRC 再計算で変わり得るため、情報表示に留める。
- OTA を含む場合は、OTA イメージの SHA-256 と Bootloader 復旧結果を記録する。
- すべての故障に証跡（テスト名 / コマンド出力 / URL / 目視メモ / 該当箇所の引用 / file:line）を付け、**Tier をレビューした head SHA で再評価**する。

### 6.1 コード変更を含む場合

- モックトランスポートでパケット列・CRC・チャンク境界・リトライ動作を検証する。
- 設定書き込みは `cmd 0x0001` の CRC / offset / chunking と `Save 0x0006` の ACK（BLE `0x0067` / USB `0x0123`）を確認する。
- セキュリティの振る舞い（authn/authz、暗号/署名検証、secret、trust-boundary、permissions）を確認する。
- OTA / Flash（Bootloader）は 4KB セクタ境界、10 秒 erase タイムアウト、erase ACK 待ち、64B 固定チャンク（最終チャンクを含む）を確認する。これは OTA 専用で、マクロ Flash は §6.4（2 秒・32B アライン）に従う。
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
→ BLE Boot: "8bt" (FF20/FF21) へ再接続（CCCD + MTU 設定）
   → readPID 0x0008 (subcmd 0x0067, total_len 2, 5s)
→ USB Boot: [0x00, 0x05] は受信 (RX) レポートの先頭（TX フレーミングは未文書化）
   → readPID cmd=0x0000, subcmd=0x0008 (total_len 2, 5s)
→ readVersionInBoot 0x0005 (subcmd 0x0067, total_len 4,
   offset 0x01017040 [gamepad_type == 1]、5s)
```

`readPID` の結果で `gamepad_type` を判定し、`WriteAPPID` の要否を決める。BLE Boot は `"8bt"` (FF20/FF21)、USB Boot は受信 `[0x00,0x05]` と `readPID cmd=0x0000/subcmd=0x0008` を確認する。該当しない transport は `N/A + 理由` を記録する。

**USB OTA 書込の制限**: `[0x00,0x05]` は **RX のみ** であり、USB Boot の **TX** フレーミング（`EraseFlash` / `WriteFirmware` / `WriteVersion` / `WriteAPPID` / `JumpApp`）はまだ文書化されていない。USB OTA 書込を実装・検証する PR は、`docs/PROTOCOL_SPEC.md` に TX フレーミングの証跡を追加するまで **停止** し、該当項目を `N/A + 理由（USB Boot TX 未文書化）` とする。BLE の `"8bt"` を USB に流用しない。USB の復旧は再列挙（re-enumeration）/再接続であり、BLE の `"8bt"` 再接続ではない。

**書き込み**

```text
EraseFlash 0x0004
  (offset = load_base_addr + sector_off, 4KB セクタ単位、
   10 秒タイムアウト、ACK を待つ)
→ WriteFirmware 0x0003
   (offset = load_base_addr + sector_off + chunk_off,
    total_len = header[8]（イメージ長）、chunk_len = 64B 固定、
    CRC-16/MODBUS 付き、1 秒、各 64B チャンクの ACK を待つ)
→ WriteVersion 0x00C4
   (24B ヘッダ、chunk_len = 0x18、total_len = header[8]、3 秒)
→ WriteFirmwareVersion 0x0002
   (gamepad_type == 1: offset 0x01017040 /
    それ以外: offset 0x08014034、4B + CRC16)
→ WriteAPPID 0x0002
   (gamepad_type != 1 のみ、offset 0x0801401C、12B + CRC16、5 秒)
→ JumpApp 0x0006 (offset 0x01018000 [gamepad_type == 1]、5 秒)
```

- `readVersionInBoot` / `JumpApp` の documented offset は `gamepad_type == 1` 用。非 type 1 では値を推測せず、`docs/PROTOCOL_SPEC.md` / Issue を確認して停止する。
- `WriteFirmwareVersion` は仕様書に両分岐（type 1: `0x01017040` / それ以外: `0x08014034`）がある。
- `EraseFlash` の offset は `load_base_addr + sector_off`、`WriteFirmware` の offset は `load_base_addr + sector_off + chunk_off`。`sector_off` は 0x1000 ずつ、`chunk_off` は 0x40 ずつ進める。
- 4KB セクタ境界、64B チャンク、**最終チャンクの 64B 固定**、CRC の範囲を確認する（OTA 専用。マクロは 32B アライン）。
- `EraseFlash` の ACK 前に `WriteFirmware` を送らない。各 64B `WriteFirmware` チャンクの ACK を待ち、ACK 前に次のチャンクを送らない。タイムアウトは 1 秒、リトライ方針を記録する。
- 失敗時は BLE なら `"8bt"` へ再接続し、`readPID` → 全セクタ消去 → 再書き込みで復旧する。USB は再列挙 / 再接続で Bootloader に入り直す（USB Boot の TX フレーミングは未文書化のため、USB OTA 書込は上記の停止規則に従う）。「再接続だけ」で復旧したとみなさない。
- `WriteFirmwareVersion` 完了前に電源断しても Bootloader に留まるため再試行できることを確認する。
- マクロの `ヘッダ 0x0001 (total_len=0xD8) → Save 0x0006 → Erase 0x0104 → Write 0x0103 → Save 0x0006` と混同しない。

### 6.4 マクロ Flash チェックリスト

```text
マクロヘッダ更新 0x0001
  (subcmd 0x0000, offset = 0x68C + slot * 0xD8, len = total_len = 0xD8,
   216B を MTU に応じて分割。ANSI_CRC_16_sh は全体で計算するが +0x00C は wire に含めない)
→ Save 0x0006 (BLE: subcmd 0x0067)
→ Erase 0x0104
  (cmd = 0x0104, subcmd = (mode << 8) | slot,
   offset = macro_idx << 12, total_len = 0x1000,
   chunk_len = 0x1000（wire header +0x04 は公式実装のチャンク設定値。
   ペイロードなし len = 0）)
→ Write 0x0103
  (subcmd = (mode << 8) | slot,
   offset = (macro_idx << 12) + chunk_index * chunk_size,
   total_len = align32(max_steps * 10),
   chunk_size = 32/64B（32B アライン。--chunk-size 32 で 32B 均一）,
   各チャンク CRC-16/MODBUS)
→ Save 0x0006 (BLE: subcmd 0x0067)
```

- `macro restore` は slot 単位のバッチで、**ヘッダ × N → Save 1 回 → 各マクロの Erase/Write → 最終 Save 1 回** の順に送る。マクロごとにヘッダ + Save を繰り返さない。
- ACK は `cmd` / `subcmd` で判定し、`chunk_len` / `total_len` のエコーを許容する。`0x600F` の BLE App チャネルでは受信フレームに prefix は付かないため、prefix なしで解析する。先頭 2 バイトが `04 01` の frame は `cmd=0x0104`（Erase の直接 ACK）として扱う。payload 付き envelope ACK が Header / Erase / Write / Save の各 stage で受理されることを確認する。
- `Erase 0x0104` はペイロードを持たない。公式実装（`getBT2_EraseMacro`）は `total_len` と `chunk_len` をともに `0x1000` に設定し、`Generate_CustomDataCmd` が wire header `+0x04` にその値を書く。実ペイロードは `len = 0`。`total_len = 0x1000` は消去サイズであり、ペイロード長ではない。`chunk_len = 0` と混同しない。Rust 実装の修正は本 PR に含まれる。
- `Write 0x0103` の `offset` は `(macro_idx << 12) + chunk_index * chunk_size`。`chunk_size` は BLE 64B / USB HID 32B（32B アライン。それ以外は拒否）。各チャンク送信ごとに `chunk_index` を進める。
- マクロのチャンクは BLE 最大 64B / USB HID 最大 32B（32B アライン。それ以外は拒否。64B 超は 64B にクランプしてから判定）。**最終チャンクは 32B アライン長まで** とし、OTA の「最終チャンク 64B 固定」とは混同しない。46B などの交渉済みサイズは将来拡張で、現在の API は `UnsupportedChunkSize` を返す。
- マクロの ACK タイムアウトは OTA と別に扱う。`Erase 0x0104` / `Write 0x0103` / ヘッダ `0x0001` は 2 秒（実装定数 `0x44fa0000`）、マクロ `Save 0x0006` は **3 秒・リトライなし**（BLE の正式値は未文書化。USB HID は 3 秒）。OTA の 10 秒 erase / 1 秒 chunk / 64B 固定をマクロに適用しない。`Save 0x0006` の ACK を受けるまで成功とみなさない。
- `align32(x) = (x + 31) & ~31 = ((x + 31) / 32) * 32`、`Erase 0x0104` の `offset = macro_idx << 12`、4KB セクタを確認する。
- バックアップは 2768B 設定の `max_steps` を読み、各 `(slot, macro_idx)` を `Read 0x0102`（`subcmd=(mode<<8)|slot`, `offset=macro_idx<<12`, `total_len=max_steps*10`）で取得する。`max_steps` は `u16` で、絶対 offset は `0x68C + slot * 0xD8 + 0x08 + macro_idx * 0x34 + 0x22`（= `0x6B6 + slot * 0xD8 + macro_idx * 0x34`）。`+0x08` はスロット内の `records[4]` 先頭、`+0x22` は `MacroFunPro3Record` 内の `max_steps`。
- マクロ 3 スロット × 4 マクロ（`slot 0..2` × `macro_idx 0..3`）のバックアップと復元を確認する。
- モックでパケット列・CRC・チャンク境界を検証する。
- BLE と USB で Save の subcmd が異なる（`0x0067` / `0x0123`）ことを確認する。

### 6.5 ゲート / セキュリティ / リリース

- **ゲート**: `.github/rulesets/main-protection.json`（canonical）と live ruleset `27785806` を監査する。canonical と比較するのは `conditions` / `bypass_actors` / `required_status_checks` / `required_approving_review_count`（現在 0。2 人目のプリンシパルがいる場合の目標 1）。`current_user_can_bypass` は live API にのみ存在するため、live 側で別途確認する。`gh pr view --json statusCheckRollup`、`gh api --paginate --slurp "repos/:owner/:repo/commits/$SHA/statuses" | jq ...`、`gh api --paginate --slurp "repos/:owner/:repo/commits/$SHA/check-runs?filter=all" | jq ...` で status と check run を確認する（**jq 必須**。gh 2.105.0 は `--slurp` と `--jq` / `--template` を併用できない）。check run は `id` で最新を選ぶ。`STATE=success` では `success` / `skipped` / `neutral`（suspicious） / check run なしを許容し、`STATE=failure` では gate-satisfying な conclusion が残っていないことを確認する。
- **セキュリティ**: workflow 権限、status / check-run 偽装の可否、secret の扱い、action の pin、署名検証、セキュリティ境界 / review-gate script のテスト（stale-SHA refusal、failure overwrite、check-run audit など）。
- **リリース**: バージョン（Cargo.toml / ファームウェアバージョン）、署名、配布物、ロールバック。

## 7. レビュアーの独立性と合議

- レビュアーは **著者とも他のレビュアーとも別セッション** でレビューする。互いの指摘を見る前に、同じ diff / SHA に対して独立に判定する。
- 各レビュアーは **自分の行/コメント** を書き、セッション ID、レビューした SHA、判定、レビューコメントの URL を残す。リードレビュアーは他のレビュアーの行を代筆せず、全員のコメント URL と SHA を確認する。
- PR 本文を編集できないレビュアーは、役割 / セッション ID または氏名 / レビューした SHA / 判定を含む standalone review comment を投稿する。リードレビュアーはその URL を記録するが、判定を代筆しない。
- Tier 2/3 は **全レビュアーが同じ head SHA で「マージ可」** を出すまでマージしない。
- **follow-up の再レビュー** は、前回レビューした SHA からの差分だけを読む（Tier 3 でも全ファイルを読み直さない）。新しい変更が diff だけでは判断できない場合に限り、変更ファイル全体、変更したシンボルの呼び出し元・API / スキーマ契約・セキュリティ／認証／データ／リリース workflow／破壊的変更の周辺を追加で読む。force-push / rebase で前回レビュー SHA が辿れない場合、または新規レビュアーが途中参加する場合は、前回差分ではなくフル diff を読む。
- 1 名でも blocker / 要修正を出したら `review-gate` の `success` を付けない。必要なら `failure` を付ける。
- **リードレビュアー**（仕様検収レビュアー）が、全員の判定とコメント URL を確認してから status を付ける。Tier 1 はレビュアー本人が付ける。
- 判定が割れた場合は、Tier を 1 段上げるか、追加のレビューを入れる。Tier 3 では **4 名目の独立 adjudicator** を追加する。blocker は、blocking reviewer が撤回するか、adjudication の理由が PR に記録されるまで残る。
- 同じ SHA の `failure` を、新しいレビューなしに `success` で上書きしない。誤りだった場合は理由を PR に残す。
- **late `CHANGES_REQUESTED`**: `success` 投稿後に blocker が出た場合、リードレビュアーは **同じ SHA に直ちに `review-gate=failure`** を投稿し、修正 push 後の新しい SHA で再レビューする。`scripts/post-review-status.sh` の `reviewDecision` 確認は best-effort であり、確認後に届くレビューは防げない。`success` を維持したままマージしない。
- レビュアーは著者であってはならず、Tier 2/3 のレビュアー同士も別セッションでなければならない。

## 8. 故障リスト

件数を満たすことではなく、**検出したい故障を先に列挙**します。カウントはコードの下限です。docs/ゲートはテストの有無ではなく、検出方法（リンクチェック、grep、コマンド実行、目視）で 3 件以上（目安 3–5）/ 5 件以上を記録します。混合 PR は最大 Tier の要件を両方に適用します（**Tier 2 混合 = コード 3 件以上 / docs 3 件以上、Tier 3 混合 = コード 10 件以上 / docs/ゲート 5 件以上**）。各項目には証跡を付けます。

**以下の表は記入例です。`実例（未解決）` と付いた行だけは、このリポジトリの既知の残存リスクです。** PR では自分の故障を記入してください。

| 故障 | 検出方法（テスト / コマンド / 目視） | 結果 | 残存リスク | 証跡 |
| --- | --- | --- | --- | --- |
| CRC の多項式を `0x8408` ↔ `0xA001` で取り違える | CRC テストベクタ | 例: 検出 | なし | テスト名 |
| `0x92C + slot * 0x0C` を `+ slot * 4` にする | SixAxis オフセットテスト | 例: 検出 | なし | テスト名 |
| `align32` を `align16` にする | マクロ長テスト | 例: 検出 | なし | テスト名 |
| RT/LT を入れ替える | マクロステップのバイト配置テスト | 例: 検出 | なし | テスト名 |
| BLE RX に `0x04` プレフィックスを要求する | 受信フレーミングテスト | 例: 検出 | なし | テスト名 |
| `cmd 0x0103` の 64B クランプを外す | チャンク分割テスト | 例: 検出 | なし | テスト名 |
| OTA の最終チャンクを 64B 未満で送る | ファームウェア分割テスト | 検出・修正済み（本 PR） | 実機 OTA 未実施。パディング方針は実機確認待ち（現状 fail-closed） | `boot_firmware_rejects_empty_short_and_mismatched_images` |
| docs の相対リンクを壊す | リンクチェック | 例: 検出 | なし | コマンド出力 |
| Tier 表の記述が 2 箇所でずれる | grep / 目視 | 例: 検出 | なし | grep 出力 |
| セキュリティトリガーが抜ける | チェックリスト照合 | 例: 検出 | なし | 照合メモ |

検出できない場合は、テストを追加するか、Tier を上げるか、残存リスクとして PR に明記します。

## 9. レビュー記録

- レビュー結果は PR のレビューコメントに残す。各レビュアーは自分の行を書き、リードは代筆しない。
- **`scripts/post-review-status.sh <PR> <reviewed-sha> <TIER> [success|failure]`** で、レビューした head SHA に `review-gate` の commit status を投稿する（check run でも gate は満たせるが、本手順は commit status を使う）。スクリプトは Tier 引数（1/2/3）を検証する。PR の宣言 Tier と quorum は検証しない（手続き）。呼び出し側は PR の宣言 Tier を渡すこと。投稿直前に head SHA を固定し、`success` の投稿前には `gh pr view --json reviewDecision` で `CHANGES_REQUESTED` を確認する（best-effort）。投稿後に `gh api --paginate --slurp ... | jq` で全 status と check run（`filter=all`）を監査する（**jq 必須**。gh 2.105.0 は `--slurp` と `--jq` / `--template` を併用できない）。
  - `STATE=success`: 最新 `review-gate` status が `success` と一致し、最新 check run（`id` 昇順の最大）が `success` / `skipped` / `neutral` / なしであること。`failure` / `cancelled` / `timed_out` / `action_required` / conclusion なしは anomaly（`skipped` / `neutral` は suspicious として警告）。
  - `STATE=failure`: 最新 status が `failure` と一致し、gate-satisfying な check run（`success` / `skipped` / `neutral`）が残っていないこと。残っている場合は anomaly。
  - API 失敗、または anomaly の場合は `review-gate=failure` を投稿して非 0 で終了する。スクリプトは check run を削除できないため、gate-satisfying な check run が残る場合はその owner が更新するか、新しい SHA に進む。failure の投稿自体に失敗した場合は、以前の `success` が残りうるため、手動で `failure` を投稿するまでマージしない。
- status の description に Tier と SHA を含める（例: `Tier 3 review of abc1234`）。
- 修正を push したら新しい SHA で再レビューする。前の SHA の status は引き継がれない。
- Tier、故障リスト、レビュアー表は PR 本文に残す。Tier は各 push 後に再評価する。

## 10. 強制範囲

`main-protection` ruleset が強制するのは **`review-gate` required check の成功（commit status または check run）** までです。Tier 判定・レビュー実施・故障リストの内容は手続きであり、write 権限者は迂回できます。同じリポジトリの PR ワークフローは `statuses: write` や `checks: write` で偽装できる可能性があり、`review-gate` は `integration_id` が無いためどちらでも満たせます。`.github/rulesets/main-protection.json` は canonical copy ですが、admin による out-of-band 変更は追加の PAT なしでは定期検知できません。check run はスクリプトから削除できないため、`failure` 投稿後も gate-satisfying な check run が残る場合は、その owner が更新するか新しい SHA に進む必要があります。強制力を上げる方法と監査上の限界は [docs/BRANCH_PROTECTION.md](BRANCH_PROTECTION.md) の「セキュリティ上の限界」を参照してください。
