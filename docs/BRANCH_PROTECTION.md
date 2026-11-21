# `main` ブランチ保護（GitHub Rulesets）

`main` は GitHub Rulesets の `main-protection`（id `27785806`）で保護しています。**2026-11-21 に有効化・検証済み**です。

ruleset の **canonical copy** は [`.github/rulesets/main-protection.json`](../.github/rulesets/main-protection.json) です。2026-11-21 時点の live ruleset と canonical フィールドが一致することを確認済みです。`current_user_can_bypass` は live API にのみ存在するため、canonical との比較対象外とし、live 側で別途確認します。GitHub 上の live ruleset と差分がないか、PR レビュー時に監査してください。admin による out-of-band 変更は、追加の PAT なしでは定期検知できないため、レビュー時の手動監査を必須とします（[セキュリティ上の限界](#セキュリティ上の限界)）。

## 適用するルール

| 項目 | 設定 |
| --- | --- |
| Enforcement | Active |
| Target | `main`（default branch） |
| Pull request | 必須（直接 push 禁止） |
| Required approvals | `0`（ソロ運用。2 人目のプリンシパルがいる場合の目標は `1`） |
| Extra approval for unattributed changes | 無効（`require_extra_approval_for_unattributed_changes: false`） |
| Required status checks | `fmt` / `clippy` / `test` / `msrv` / `review-gate` |
| Require branches up to date | 有効（`strict_required_status_checks_policy: true`） |
| Conversation resolution | 有効 |
| Merge methods | squash のみ（`allowed_merge_methods: ["squash"]`） |
| Linear history | 有効 |
| Force push | 禁止（`non_fast_forward`） |
| Branch deletion | 禁止（`deletion`） |
| Bypass actors | canonical: `bypass_actors: []` / live: `current_user_can_bypass: never` |

`review-gate` は **required status check** です。PR の head SHA に context `review-gate` の **commit status** または **check run** の成功があれば満たされます（ruleset に `integration_id` が無いため）。新しいコミットを push すると SHA が変わり、どちらも無い新しいコミットは自動的にマージできなくなります。

レビューの深度（Tier 1〜3）と故障リストの要件は **[docs/REVIEW_TIERS.md](REVIEW_TIERS.md)** を参照してください。

## canonical ruleset の更新

- canonical: `.github/rulesets/main-protection.json`
- live: `main-protection`, id `27785806`

canonical file を変更する PR は **Tier 3** です。live ruleset への反映は canonical file を更新した後の別操作とし、このリポジトリの PR 内で GitHub API を呼んで live を変更しないでください。

```bash
REPO=haruki0420/8bitdo-ultimate2-suite

# live へ反映する場合（canonical file を更新し、構文検証を通した後）
gh api --method PUT "repos/$REPO/rulesets/27785806" \
  --input .github/rulesets/main-protection.json
```

## 確認コマンド

```bash
REPO=haruki0420/8bitdo-ultimate2-suite

# canonical JSON の構文検証
python3 -m json.tool .github/rulesets/main-protection.json >/dev/null

# live と canonical の主要フィールドを比較（diff が空なら一致）
diff -u \
  <(jq -S '{name, target, enforcement, conditions, bypass_actors, rules}' \
      .github/rulesets/main-protection.json) \
  <(gh api "repos/$REPO/rulesets/27785806" \
      --jq '{name, target, enforcement, conditions, bypass_actors, rules}' | jq -S .)

# bypass 設定
gh api "repos/$REPO/rulesets/27785806" --jq '{current_user_can_bypass}'

# PR が保護でブロックされているか
gh pr view <PR> --repo "$REPO" --json mergeStateStatus,statusCheckRollup
```

`mergeStateStatus: BLOCKED` の主な原因:

- head SHA に成功した `review-gate` の commit status / check run が無い
- 必須 CI が pending / failure
- ブランチが `main` より古い（up-to-date 必須）
- 未解決のレビュースレッドがある
- `require_extra_approval_for_unattributed_changes` が有効（現在は無効）

## `review-gate` を満たす方法

レビューした SHA を固定し、投稿直前に head が動いていないことを確認します。Tier 1 はレビュアー本人、Tier 2/3 は **全レビュアーが同じ SHA で「マージ可」** を出したことを確認したリードレビュアーが commit status を投稿します。1 名でも要修正なら `success` を付けません。

本手順では **`scripts/post-review-status.sh`** で commit status を投稿します。インラインの `gh api` をコピーせず、このスクリプトを使ってください。check run でも gate は満たせますが、本リポジトリの標準手順は commit status です。

```bash
# success
scripts/post-review-status.sh <PR> <reviewed-sha> <1|2|3>

# failure（修正が必要な場合）
scripts/post-review-status.sh <PR> <reviewed-sha> <1|2|3> failure
```

スクリプトは Tier 引数（1/2/3）を検証します。PR の宣言 Tier と quorum は検証しないため（手続き）、呼び出し側が PR の宣言 Tier を渡してください。投稿直前に head SHA を固定し、`success` の投稿前には `gh pr view --json reviewDecision` で `CHANGES_REQUESTED` を確認します（best-effort）。投稿後に `gh api --paginate --slurp ... | jq` で全 status と check run（`filter=all`）を監査します（**jq 必須**。gh 2.105.0 は `--slurp` と `--jq` / `--template` を併用できません）。`STATE=success` は最新 check run（`id` で選択）が `success` / `skipped` / `neutral` / なし、`STATE=failure` は gate-satisfying な check run が残っていないことを要求します。最新 status / check run が期待状態でない、または監査 API が失敗した場合、スクリプトは `review-gate=failure` を投稿して非 0 で終了します。

> `review-gate` は **required status check** です。commit status（`scripts/post-review-status.sh`）または同じ context の check run で満たされます。ruleset に `integration_id` が無いため、どちらも gate-satisfying です。

## status / check run の監査

```bash
REPO=haruki0420/8bitdo-ultimate2-suite
SHA=<reviewed-sha>

# jq が必須（gh 2.105.0 は --slurp と --jq / --template を併用できない）。
command -v jq >/dev/null

# 全 commit status（新しい順）。context ごとの最新だけを見ない。
gh api --paginate --slurp "repos/$REPO/commits/$SHA/statuses" \
  | jq 'flatten | [.[] | {context, state, description, creator: .creator.login, created_at}]'

# check run（gate-satisfying。filter=all で latest 以外も監査）
gh api --paginate --slurp "repos/$REPO/commits/$SHA/check-runs?filter=all" \
  | jq '[.[] | .check_runs[] | {id, name, status, conclusion, app: .app.slug, app_id: .app.id, started_at, completed_at, details_url}]'

# PR の rollup
gh pr view <PR> --repo "$REPO" --json statusCheckRollup
```

- `GET /commits/{sha}/status` は **context ごとの最新 status のみ** を返し、`creator` が `null` になることがあります。監査には `GET /commits/{sha}/statuses`（全 status、新しい順）を使ってください。
- `review-gate` の commit status は最新のものが有効です。過去の `failure` が `success` で上書きされていないか、`created_at` と `creator` を確認してください。
- check run も `review-gate` を満たす gate-satisfying な signal です。`commits/$SHA/statuses` と `commits/$SHA/check-runs?filter=all` の **両方** を監査し、context `review-gate` の最新状態を確認してください。最新 check run は **`id` の最大** で選びます（queued / in-progress は `started_at` / `completed_at` が null のため、時刻ソートでは正しく選べません）。GitHub の required check は `success` / `skipped` / `neutral` を受け入れます。
  - `STATE=success`: `success` / `skipped` / `neutral` / check run なしを許容します。`skipped` / `neutral` は suspicious として `app.slug` / `app.id` / `started_at` / `completed_at` / `id` を確認してください。`failure` / `cancelled` / `timed_out` / `action_required` / `stale` / `startup_failure` / conclusion なしは anomaly です。
  - `STATE=failure`: `failure` / `cancelled` / `timed_out` / `action_required` / `stale` / `startup_failure` / conclusion なし / check run なしは gate blocked として許容します。`success` / `skipped` / `neutral` は failure を打ち消すため anomaly です。
  - `GET /commits/{sha}/check-runs` は既定で `filter=latest` なので、failure-overwrite を監査するときは `?filter=all` を使います。同じリポジトリの PR ワークフローが `checks: write` で passing な `review-gate` check run を作れるため、これは `integration_id` を trusted app に固定するまで documented bypass です。
- `review-gate=failure` を投稿しても、既存の gate-satisfying な check run は削除できません。その場合は check-run の owner が update / re-run するか、新しい SHA に進んでください。failure の投稿自体に失敗した場合は、以前の `success` が残りうるため、手動で `failure` を投稿するまでマージしないでください。
- **late `CHANGES_REQUESTED`**: `success` 投稿後に blocker が出た場合、リードレビュアーは **同じ SHA に直ちに `review-gate=failure`** を投稿し、修正 push 後の新しい SHA で再レビューします。スクリプトの `reviewDecision` 確認は best-effort であり、確認後に届くレビューは防げません。

## セキュリティ上の限界

このゲートは **手続き的なゲート** であり、悪意ある write 権限者に対する暗号的な保証ではありません。

- commit status は SHA への紐づきを強制するが、投稿者の身元は強制しない。
- 同じリポジトリのブランチから実行される PR ワークフローは、`statuses: write` で commit status を、`checks: write` で check run を偽装できる。required check `review-gate` は `integration_id` が無いため、どちらでも満たせる。PR 内で `ci.yml` を書き換えて `fmt` などを偽装することもできる。
- 最新 status が有効なので、`failure` を `success` で上書きできる。
- canonical ruleset はあくまで版管理されたコピーであり、admin が GitHub 上で out-of-band に変更できる。追加の PAT なしでは定期監査ジョブを置けないため、out-of-band 変更を常時検知できないことを受け入れる。
- このため、現状は **信頼できるコントリビューター（メンテナ）前提** の運用とする。

強制力を上げるには、次のいずれかが必要です。

1. 2 人目のレビュアーアカウントまたはマシンユーザーを追加し、`required_approving_review_count` を `1` にする。
2. 信頼できる GitHub App を用意し、`required_status_checks` の `integration_id` で expected source を固定する。
3. `.github/workflows/**` を CODEOWNERS で保護し、ワークフロー変更を別レビューにする（承認 1 が前提）。
