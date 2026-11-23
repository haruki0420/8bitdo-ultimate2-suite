# `main` ブランチ保護（GitHub Rulesets）

`main` は GitHub Rulesets `main-protection`（id `27785806`、2026-11-21 有効化・検証済み）で保護しています。canonical copy は [`.github/rulesets/main-protection.json`](../.github/rulesets/main-protection.json) です。admin の out-of-band 変更は追加 PAT なしでは定期検知できないため、PR レビュー時の手動監査を必須とします。

## 1. 適用ルール

| 項目 | 設定 |
| --- | --- |
| Enforcement / Target | Active / `main`（PR 必須、直接 push 禁止） |
| Required approvals | `0`（ソロ運用。2 人目追加時の目標は `1`） |
| Required status checks | `fmt` / `clippy` / `test` / `msrv` / `review-gate`（up-to-date 必須） |
| Conversation / History | 会話解決必須、squash のみ、linear history 有効 |
| Force push / Deletion | 禁止（`non_fast_forward` / `deletion`） |
| Bypass actors | canonical: `[]` / live: `current_user_can_bypass: never` |

- **`Desktop` ワークフロー（`.github/workflows/desktop.yml`）**: `desktop-web` / `desktop-rust` は **advisory（非必須）**です。required 昇格は ruleset 変更の別 PR（Tier 3）で行います。マージ判断では手続き上 desktop ジョブの結果も確認します。
- **`review-gate`**: `integration_id` 未固定のため、head SHA に対する同名 context の commit status または check run 成功で満たされます。push で SHA が変わると自動で再ブロックされます。レビュー深度は [REVIEW_TIERS.md](REVIEW_TIERS.md) を参照してください。

## 2. canonical ruleset の更新と監査コマンド

canonical file の変更は **Tier 3** です。PR 内で GitHub API を呼んで live を変更せず、マージ後に別操作で反映します。

```bash
REPO=haruki0420/8bitdo-ultimate2-suite

# 構文検証と live との差分比較（空なら一致）
python3 -m json.tool .github/rulesets/main-protection.json >/dev/null
diff -u \
  <(jq -S '{name, target, enforcement, conditions, bypass_actors, rules}' .github/rulesets/main-protection.json) \
  <(gh api "repos/$REPO/rulesets/27785806" --jq '{name, target, enforcement, conditions, bypass_actors, rules}' | jq -S .)

# bypass 設定と PR ブロック状態の確認
gh api "repos/$REPO/rulesets/27785806" --jq '{current_user_can_bypass}'
gh pr view <PR> --repo "$REPO" --json mergeStateStatus,statusCheckRollup

# live 反映（canonical 更新・検証後の別操作）
gh api --method PUT "repos/$REPO/rulesets/27785806" --input .github/rulesets/main-protection.json
```

## 3. `review-gate` の投稿と監査

Tier 1 はレビュアー本人、Tier 2/3 は全レビュアーが同一 SHA で「マージ可」を出した後にリードレビュアーが **`scripts/post-review-status.sh`** で投稿します（1 名でも要修正なら `success` を付けない）。

```bash
scripts/post-review-status.sh <PR> <reviewed-sha> <1|2|3>          # success
scripts/post-review-status.sh <PR> <reviewed-sha> <1|2|3> failure  # failure
```

スクリプトは Tier 引数（1/2/3）を検証し、head SHA 固定、`reviewDecision` の `CHANGES_REQUESTED` 確認（best-effort）、投稿後の全 status と check run（`filter=all`、`jq` 必須）監査を行います。異常時は `review-gate=failure` を投稿して非 0 で終了します。

```bash
REPO=haruki0420/8bitdo-ultimate2-suite
SHA=<reviewed-sha>
gh api --paginate --slurp "repos/$REPO/commits/$SHA/statuses" \
  | jq 'flatten | [.[] | {context, state, description, creator: .creator.login, created_at}]'
gh api --paginate --slurp "repos/$REPO/commits/$SHA/check-runs?filter=all" \
  | jq '[.[] | .check_runs[] | {id, name, status, conclusion, app: .app.slug, app_id: .app.id, started_at, completed_at}]'
```

- 監査には最新 1 件のみの `/status` ではなく全履歴の `/statuses` と `/check-runs?filter=all` の両方を使い、最新 check run は最大 `id` で選びます。
- `STATE=success` では `skipped`/`neutral` を suspicious、`failure`/`cancelled`/`timed_out`/`action_required`/`stale`/`startup_failure`/conclusion なしを anomaly とします。`STATE=failure` では `success`/`skipped`/`neutral` が残っていれば anomaly です（既存 check run は削除できないため owner の再実行か新 SHA へ進む）。
- **late `CHANGES_REQUESTED`**: `success` 投稿後に blocker が出たら、直ちに同一 SHA へ `review-gate=failure` を投稿し、修正後の新 SHA で再レビューします。

## 4. セキュリティ上の限界

本ゲートは信頼できるメンテナ前提の**手続き的ゲート**であり、悪意ある write 権限者への暗号的保証ではありません（投稿者身元未強制、PR ワークフローによる `statuses: write` / `checks: write` / `ci.yml` 改変、最新 status 上書き、admin による out-of-band 変更）。強制力を高めるには ①2 人目アカウント + `required_approving_review_count: 1` ②信頼済み GitHub App の `integration_id` 固定 ③`.github/workflows/**` の CODEOWNERS 保護が必要です。
