# `main` ブランチ保護（GitHub Rulesets）

`main` は GitHub Rulesets の `main-protection`（id `27785806`）で保護しています。**2026-11-21 に有効化・検証済み**です。

ruleset の **canonical copy** は [`.github/rulesets/main-protection.json`](../.github/rulesets/main-protection.json) です。2026-11-21 時点の live ruleset と一致することを確認済みです。GitHub 上の live ruleset と差分がないか、PR レビュー時に監査してください。admin による out-of-band 変更は、追加の PAT なしでは定期検知できないため、レビュー時の手動監査を必須とします（[セキュリティ上の限界](#セキュリティ上の限界)）。

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
| Bypass actors | なし（`current_user_can_bypass: never`） |

`review-gate` は **commit status** です。PR の head SHA に `success` を付けると満たされます。新しいコミットを push すると SHA が変わり、status が無いため自動的にマージできなくなります。

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

- head SHA に `review-gate` status が無い
- 必須 CI が pending / failure
- ブランチが `main` より古い（up-to-date 必須）
- 未解決のレビュースレッドがある
- `require_extra_approval_for_unattributed_changes` が有効（現在は無効）

## `review-gate` status の付け方

レビューした SHA を固定し、投稿直前に head が動いていないことを確認します。Tier 1 はレビュアー本人、Tier 2/3 は **全レビュアーが同じ SHA で「マージ可」** を出したことを確認したリードレビュアーが付けます。1 名でも要修正なら `success` を付けません。

status 投稿の正本は **`scripts/post-review-status.sh`** です。インラインの `gh api` をコピーせず、このスクリプトを使ってください。

```bash
# success
scripts/post-review-status.sh <PR> <reviewed-sha> <1|2|3>

# failure（修正が必要な場合）
scripts/post-review-status.sh <PR> <reviewed-sha> <1|2|3> failure
```

スクリプトは Tier を検証し、投稿直前に head SHA を固定し、投稿後に全 status と check run を監査します。

> `review-gate` は **commit status** です。同じ名前の check run が成功していても、commit status の `success` が無ければゲートは満たされません。

## status / check run の監査

```bash
REPO=haruki0420/8bitdo-ultimate2-suite
SHA=<reviewed-sha>

# 全 commit status（新しい順）。context ごとの最新だけを見ない。
gh api "repos/$REPO/commits/$SHA/statuses" \
  --jq '[.[] | {context, state, description, creator: .creator.login, created_at}]'

# check run（commit status とは別）
gh api "repos/$REPO/commits/$SHA/check-runs" \
  --jq '[.check_runs[] | {name, status, conclusion, details_url}]'

# PR の rollup
gh pr view <PR> --repo "$REPO" --json statusCheckRollup
```

- `GET /commits/{sha}/status` は **context ごとの最新 status のみ** を返し、`creator` が `null` になることがあります。監査には `GET /commits/{sha}/statuses`（全 status、新しい順）を使ってください。
- `review-gate` の status は最新のものが有効です。過去の `failure` が `success` で上書きされていないか、`created_at` と `creator` を確認してください。
- check run は commit status とは別です。同じリポジトリの PR ワークフローが `checks: write` で check run を偽装できるため、`review-gate` の commit status と合わせて監査してください。

## セキュリティ上の限界

このゲートは **手続き的なゲート** であり、悪意ある write 権限者に対する暗号的な保証ではありません。

- commit status は SHA への紐づきを強制するが、投稿者の身元は強制しない。
- 同じリポジトリのブランチから実行される PR ワークフローは、`statuses: write` を要求して `review-gate` を偽装できる。PR 内で `ci.yml` を書き換えて `fmt` などを偽装することもできる。check run も `checks: write` で偽装できる。
- 最新 status が有効なので、`failure` を `success` で上書きできる。
- canonical ruleset はあくまで版管理されたコピーであり、admin が GitHub 上で out-of-band に変更できる。追加の PAT なしでは定期監査ジョブを置けないため、out-of-band 変更を常時検知できないことを受け入れる。
- このため、現状は **信頼できるコントリビューター（メンテナ）前提** の運用とする。

強制力を上げるには、次のいずれかが必要です。

1. 2 人目のレビュアーアカウントまたはマシンユーザーを追加し、`required_approving_review_count` を `1` にする。
2. 信頼できる GitHub App を用意し、`required_status_checks` の `integration_id` で expected source を固定する。
3. `.github/workflows/**` を CODEOWNERS で保護し、ワークフロー変更を別レビューにする（承認 1 が前提）。
