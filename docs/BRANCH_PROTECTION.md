# `main` ブランチ保護（GitHub Rulesets）

`main` は GitHub Rulesets の `main-protection`（id `27785806`）で保護しています。**2026-11-21 に有効化・検証済み**です。

## 適用するルール

| 項目 | 設定 |
| --- | --- |
| Enforcement | Active |
| Target | `main`（default branch） |
| Pull request | 必須（直接 push 禁止） |
| Required approvals | `0`（ソロ運用。自分自身は approve できないため） |
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

## 作成・更新コマンド

新規作成は `POST`、既存 ruleset の更新は `PUT` を使います。以下は id `27785806` を更新する例です。

```bash
cat > /tmp/8bitdo-main-ruleset.json <<'JSON'
{
  "name": "main-protection",
  "target": "branch",
  "enforcement": "active",
  "conditions": {
    "ref_name": {
      "include": ["~DEFAULT_BRANCH"],
      "exclude": []
    }
  },
  "bypass_actors": [],
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "required_linear_history" },
    {
      "type": "pull_request",
      "parameters": {
        "required_approving_review_count": 0,
        "dismiss_stale_reviews_on_push": true,
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "require_extra_approval_for_unattributed_changes": false,
        "allowed_merge_methods": ["squash"],
        "required_review_thread_resolution": true
      }
    },
    {
      "type": "required_status_checks",
      "parameters": {
        "strict_required_status_checks_policy": true,
        "required_status_checks": [
          { "context": "fmt" },
          { "context": "clippy" },
          { "context": "test" },
          { "context": "msrv" },
          { "context": "review-gate" }
        ]
      }
    }
  ]
}
JSON

REPO=haruki0420/8bitdo-ultimate2-suite
gh api --method PUT "repos/$REPO/rulesets/27785806" --input /tmp/8bitdo-main-ruleset.json
```

## 確認コマンド

```bash
REPO=haruki0420/8bitdo-ultimate2-suite

# 一覧
gh api "repos/$REPO/rulesets" --jq '.[] | {id, name, enforcement, target}'

# 詳細（conditions / bypass / 実ルールまで確認）
gh api "repos/$REPO/rulesets/27785806" \
  --jq '{name, enforcement, target, conditions, bypass_actors, current_user_can_bypass, rules}'

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

レビューした SHA を固定し、投稿直前に head が動いていないことを確認します。

```bash
set -euo pipefail

REPO=haruki0420/8bitdo-ultimate2-suite
PR="${1:?usage: post-review-status.sh <pr-number> <reviewed-sha>}"
REVIEWED_SHA="${2:?usage: post-review-status.sh <pr-number> <reviewed-sha>}"

current=$(gh pr view "$PR" --repo "$REPO" --json headRefOid --jq .headRefOid)
if [ "$current" != "$REVIEWED_SHA" ]; then
  echo "head moved: reviewed=$REVIEWED_SHA current=$current" >&2
  exit 1
fi

gh api --method POST "repos/$REPO/statuses/$REVIEWED_SHA" \
  -f state=success \
  -f context=review-gate \
  -f description="Independent review of $(printf '%s' "$REVIEWED_SHA" | cut -c1-7)" \
  -f target_url="https://github.com/$REPO/pull/$PR"

# 監査: 最新 status と投稿者を確認
gh api "repos/$REPO/commits/$REVIEWED_SHA/status" \
  --jq '{state, statuses: [.statuses[] | {context, state, creator}]}'
```

修正が必要な場合は `state=failure` で同じ context に status を付けます。同じ SHA への最新 status が有効になるため、**レビュアーは自分の status が上書きされていないか確認してください**。

> **fork PR**: status の投稿には base リポジトリの `statuses: write` が必要なため、外部レビュアーは付けられません。メンテナが代行してください。fork の workflow token は read-only ですが、fork PR は CI ワークフローの挙動を変更できるため、CI も security boundary ではありません。

## セキュリティ上の限界

このゲートは **手続き的なゲート** であり、悪意ある write 権限者に対する暗号的な保証ではありません。

- commit status は SHA への紐づきを強制するが、投稿者の身元は強制しない。
- 同じリポジトリのブランチから実行される PR ワークフローは、`statuses: write` を要求して `review-gate` を偽装できる。PR 内で `ci.yml` を書き換えて `fmt` などを偽装することもできる。
- 最新 status が有効なので、`failure` を `success` で上書きできる。
- このため、現状は **信頼できるコントリビューター（メンテナ）前提** の運用とする。

強制力を上げるには、次のいずれかが必要です。

1. 2 人目のレビュアーアカウントまたはマシンユーザーを追加し、`required_approving_review_count` を `1` にする。
2. 信頼できる GitHub App を用意し、`required_status_checks` の `integration_id` で expected source を固定する。
3. `.github/workflows/**` を CODEOWNERS で保護し、ワークフロー変更を別レビューにする（承認 1 が前提）。
