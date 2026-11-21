# コントリビューションガイド

このリポジトリでは、`main` の履歴と品質を守るために **すべての変更を Pull Request 経由** で行います。AI エージェントも人間も同じルールです。

## 1. `main` の保護

`main` には [docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md) の `main-protection` ruleset（id `27785806`）を適用済みです（2026-11-21 検証）。

- `main` への直接 push 禁止（Pull Request 必須）
- 必須 CI: `fmt` / `clippy` / `test` / `msrv`
- 必須レビューゲート: **レビュー済みコミット SHA に紐づく `review-gate` の成功（commit status または check run）**
- Required approvals は現在 `0`（ソロ運用。2 人目のプリンシパルがいる場合の目標は `1`）
- force push・ブランチ削除禁止
- linear history（squash merge のみ）
- 未解決のレビューコメントがある限りマージ不可
- ブランチが `main` より古い場合はマージ不可（up-to-date 必須）

> `review-gate` はラベルではなく **required status check** です。commit status または check run で満たされます。新しいコミットを push すると SHA が変わり、どちらも無い新しいコミットは自動的にマージできなくなります。これにより「レビュー後にコミットを差し替えて通す」迂回を防ぎます。

## 2. 変更の流れ

1. Issue または PR の説明に、変更目的と仕様上の根拠を書く。
2. `main` から作業ブランチを切る。
   - `feat/...` / `fix/...` / `docs/...` / `chore/...`
3. 実装・テストを追加する。
4. ローカルで以下を通す。

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo clippy -p eightbitdo-protocol --no-default-features --all-targets --locked -- -D warnings
   cargo test --workspace --locked
   cargo test --workspace --all-features --locked
   cargo test -p eightbitdo-protocol --no-default-features --locked
   cargo check -p eightbitdo-protocol --no-default-features --all-targets --locked
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
   rustup toolchain install 1.85.0
   cargo +1.85.0 check --workspace --all-targets --all-features --locked
   cargo +1.85.0 check -p eightbitdo-protocol --no-default-features --all-targets --locked
   ```

5. PR を作成する。PR テンプレートで **Review Tier** を提案する（混合 PR は最大 Tier）。
6. CI がすべて通ることを確認する。
7. **著者とも他のレビュアーとも別セッション**のレビュアー（AI エージェント、または人間）が、Tier に応じた深度で差分をレビューする。各レビュアーは他のレビュアーの判定を見る前に、自分の行とレビューコメントを書く。
8. Tier 1 はレビュアー本人、Tier 2/3 はリードレビュアーが、全員の「マージ可」を確認してから、**レビューした head SHA** に `review-gate` を満たす commit status を投稿する（check run でも gate は満たせるが、本手順は `scripts/post-review-status.sh` を使う）。

   ```bash
   scripts/post-review-status.sh <PR> <レビューした head SHA> <1|2|3>
   ```

   スクリプトは Tier 引数（1/2/3）を検証します。PR の宣言 Tier と quorum は検証しないため（手続き）、呼び出し側が PR の宣言 Tier を渡してください。投稿直前に head SHA を固定し、`success` の投稿前には `gh pr view --json reviewDecision` で `CHANGES_REQUESTED` を確認します（best-effort）。投稿後に `gh api --paginate --slurp ... | jq` で全 status と check run（`filter=all`）を監査します（**jq 必須**。gh 2.105.0 は `--slurp` と `--jq` / `--template` を併用できません）。`STATE=success` は最新 check run（`id` で選択）が `success` / `skipped` / `neutral` / なし、`STATE=failure` は gate-satisfying な check run が残っていないことを要求します。最新 status / check run が期待状態でない、または監査 API が失敗した場合、スクリプトは `review-gate=failure` を投稿して非 0 で終了します。修正が必要な場合は `failure` を付けてください。

   ```bash
   scripts/post-review-status.sh <PR> <レビューした head SHA> <1|2|3> failure
   ```

   修正後は新しい SHA へ `success` を付ける。[docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md) に監査方法と限界があります。

9. すべてのレビューコメントを解決し、squash merge する。

> `reviewed` ラベルは任意の目視用マーカーであり、マージ条件ではありません（ラベル説明も "Optional visual marker; not a merge gate"）。必須なのは SHA に紐づく `review-gate` の成功（commit status または check run）です。

## 3. レビュー Tier と観点

Tier の判定・パス別トリガー・故障リストの詳細は **[docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)** を参照してください。ここでは要約だけを示します。

- **Tier 1**: レビュアー 1 名。**振る舞いを変えない変更のみ**（Tier 3 パス以外のコメント・整形、通常 docs（規範仕様・ゲート/プロセス文書を除く。これらは Tier 1 にはしない＝最低 Tier 2、安全-critical は Tier 3）、純粋なテスト追加）。純粋なテスト追加は 1〜2 件の故障注入を記録する。Tier 3 パスの整形は A1 の条件を満たす場合のみ降格、コメントは A2 で常に Tier 3。write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するテストは Tier 3。
- **Tier 2**: レビュアー 2 名（仕様検収 + 実装/回帰）。
- **Tier 3**: レビュアー 2 名 + 別セッションの敵対的パス（計 3 セッション）。
- 混合 PR は最大 Tier。著者が提案し、レビュアーは昇格できる。原則として降格できない（例外は REVIEW_TIERS §2 のダウングレード制限のみ。A1 は整形のみ、A2 は常に Tier 3、A3 はソースレベル diff が必要。規範仕様・ゲート・プロセス文書は Tier 1 にはしない。最低 Tier 2、安全-critical は Tier 3）。
- **Content-based override**: チェックリスト項目、コントリビューター手順、required check、status コマンド、安全手順を変える PR は、docs パスでも Tier 3。
- **Write-path rule**: デバイス書き込みのバイト・順序・タイミング・エラー処理、write-path に影響する依存更新、またはそれを検証するテスト/mock/golden vector を変える PR は Tier 3。読み取り専用 builder は Tier 2（security behavior を除く）。
- **Security behavior rule**: 認証/認可、暗号/署名検証、secret の扱い、trust-boundary の入力検証、権限変更、またはそれらに影響する依存更新は、パスにかかわらず Tier 3。
- **自己修正ルール**: Tier 定義・パス別トリガー・レビュアー数・quorum・故障リスト最低数・チェックリスト・required check・ruleset・status コマンド・ゲート手順の変更は Tier 3。
- コードのリファクタは Tier 1 にしない。テストの変更・削除は Tier 2（Tier 3 パス、write-path / Flash / OTA / backup / CRC / golden vector / mock transport / safety-critical test vector / セキュリティ境界 / review-gate script を検証するものは Tier 3）。実機書き込み・Flash・OTA・セキュリティは小さくても Tier 3。
- レビュアーは **著者とも他のレビュアーとも別セッション**。Tier 2/3 は **全レビュアーが同じ head SHA で「マージ可」** を出すまでマージしない。1 名でも要修正なら status を付けない。
- **Tier 1 はレビュアー本人、Tier 2/3 はリードレビュアー**が、全員の判定を確認してから `scripts/post-review-status.sh` で `review-gate` を満たす commit status を投稿する。Tier 3 で判定が割れたら 4 名目の独立 adjudicator を追加し、blocker の撤回または adjudication 理由を記録する。
- Tier 2/3 は「故障 → 検出方法 → 結果 → 残存リスク → 証跡」を PR に残す。証跡はテスト名 / コマンド出力 / URL / 目視メモ / 該当箇所の引用 / file:line のいずれか。件数はコードの下限であり、Tier 2 は 3 件以上（目安 3–5、見つかった故障はすべて記録）、Tier 3 は 10 件以上。docs は検出方法で数え、3 件以上（目安 3–5）/ 5 件以上。混合 PR は最大 Tier の要件を両方に適用する（Tier 2 混合 = コード 3 件以上 / docs 3 件以上、Tier 3 混合 = コード 10 件以上 / docs 5 件以上）。docs/gate-only は mock/実機項目を `N/A + 理由` とする。
- **「マージ可」が出るまでマージしない。** 修正を push したら新しい SHA で再レビューし、Tier を再評価する。
- **late `CHANGES_REQUESTED`**: `review-gate=success` の投稿後に blocker が出た場合、リードレビュアーは **同じ SHA に直ちに `review-gate=failure`** を投稿し、修正 push 後の新しい SHA で再レビューする。`success` を維持したままマージしない。スクリプトの `reviewDecision` 確認は best-effort であり、確認後に届くレビューは防げない。

レビュアーは「作者の意図」ではなく、**仕様書と実装の一致** を確認します。

- [ ] `docs/PROTOCOL_SPEC.md` のコマンド / フレーミング / CRC と一致しているか
- [ ] `docs/STRUCT_BT2_2768B.md` のオフセット・サイズ・エンディアンと一致しているか
- [ ] 予約領域 `0x1F4..0x68B` / `0x914..0x92B` を破壊していないか
- [ ] 名前が UTF-16**BE**、数値が Little-Endian になっているか
- [ ] マクロの `align32`、BLE 64B / USB HID 32B チャンク（または交渉済みサイズ、上限超過は拒否またはクランプ）、4KB セクタ、Erase → Write → Header → Save の順序。`Erase 0x0104` は `total_len = chunk_len = 0x1000`、ペイロードなし
- [ ] OTA の前提（BLE Boot: `enterBoot` → `"8bt"` → `readPID 0x0008` 5s、USB Boot: **RX** `[0x00,0x05]` → `readPID cmd=0x0000/subcmd=0x0008` 5s、`readVersionInBoot 0x0005`）と書込順序、失敗時の全セクタ消去・再書き込み。`readVersionInBoot` の offset `0x01017040` は type 1 用で、非 type 1 は停止して仕様/Issue を確認する。USB Boot の **TX** フレーミングは未文書化のため、USB OTA 書込は停止し、`PROTOCOL_SPEC` に証跡を追加するか `N/A + 理由（USB Boot TX 未文書化）` を記録する。USB の復旧は再列挙/再接続
- [ ] 実機書き込み前に 2768B 設定とマクロ 4×3 をバックアップし、SHA-256 と復元結果を記録しているか。`max_steps` は `0x68C + slot * 0xD8 + 0x08 + macro_idx * 0x34 + 0x22`（`u16`）
- [ ] BLE 受信にプレフィックスを要求していないか（`0x600F`）
- [ ] USB HID の送信 `[0x81, 0x04]` / 受信 `[0x02, 0x04]` を混同していないか
- [ ] 失敗系（長さ不足、未知の値、予約バイト）のテストがあるか
- [ ] ドキュメントと実装の記述が矛盾していないか

## 4. レビュー status の限界

`review-gate` は context `review-gate` の commit status / check run に **SHA への紐づき** を強制しますが、**投稿者の身元** や **レビュー内容** までは強制しません。write 権限を持つ者はどちらも付けられます。commit status は同じ SHA の最新が有効なので `failure` を `success` で上書きできます。check run も成功した run を追加できます。canonical ruleset は `.github/rulesets/main-protection.json`（live: id `27785806`）です。

- 現状はソロ運用のため required approvals は `0`。2 人目のプリンシパルがいる場合の目標は `1`。独立レビューは手続きとして運用する。
- 監査には `gh api --paginate --slurp ... | jq ...` で `commits/$SHA/statuses`（全 status、新しい順）と `commits/$SHA/check-runs?filter=all`（最新以外も含む）の **両方** を使う。gh 2.105.0 は `--slurp` と `--jq` を併用できないため、必ず jq に pipe する。`commits/$SHA/status` は context ごとの最新のみを返し、`creator` が `null` になるため監査に使わない。
- `review-gate` を満たす signal は commit status と check run の両方。GitHub の required check は `success` / `skipped` / `neutral` を受け入れるため、check run の `skipped` / `neutral` は suspicious として `app.slug` / `app.id` / `started_at` / `completed_at` を確認する。`gh pr view --json statusCheckRollup` でも確認する。同じリポジトリの PR ワークフローは `statuses: write` や `checks: write` を要求してどちらも偽装できる。PR 内で `ci.yml` を書き換えることもできる。**CI と status / check run は悪意ある write 権限者に対する security boundary ではない。**
- 身元まで強制する場合は、2 人目のレビュアーアカウントまたはマシンユーザーを追加し、required approvals を 1 に上げる。
- または、trusted GitHub App の `integration_id` で `review-gate` の expected source を固定する。それまでは、same-repo PR ワークフローが `checks: write` で passing な check run を作れることを documented bypass として受け入れる。
- GitHub Actions の bot 承認は required approvals にカウントされない。
- ruleset は admin が out-of-band に変更できる。追加の PAT なしでは定期監査ジョブを置けないため、PR レビュー時の live 監査を必須とし、out-of-band 変更を常時検知できないことを受け入れる。
- **fork PR**: status の投稿には base リポジトリの `statuses: write` が必要なため、外部レビュアーは付けられません。メンテナが代行してください。fork の workflow token は read-only ですが、fork PR は CI ワークフローの挙動を変更できるため、CI も security boundary ではありません。

## 5. コミットメッセージ

Conventional Commits に近い形式を使います。

```text
feat(protocol): add macro write packet builder
fix(protocol): correct config write subcmd
docs: update review process
chore(ci): add review gate
```

## 6. 禁止事項

- 公式の `.apk` / `.exe` / `.dll` / `.so` / ファームウェア本体をコミットしない（`.gitignore` で除外済み）。
- 実機の 2768B 設定やマクロを、バックアップなしで上書きしない。
- `main` へ直接 push しない。
- レビューを受けずに `review-gate` の commit status / check run を自分で付けない。

## 7. AI エージェント向け

- 変更は必ずブランチ → PR で行う。
- レビューは **著者とも他のレビュアーとも別セッションのエージェント** に依頼し、著者自身のセッションでレビューを完結させない。
- レビュー結果は PR のレビューコメントとして残す。各レビュアーは自分の行を書き、リードは代筆しない。
- レビューで blocker / major が見つかった場合は、修正して CI を再実行し、新しい SHA で再度レビューを受ける。
- status を付ける前に、レビュー対象の `headRefOid` と自分のレビューした SHA が一致していることを確認する。投稿は `scripts/post-review-status.sh` を使う。
- Tier 3 のサブセクションは削除せず、該当しない項目は `N/A + 理由` を記録する。
