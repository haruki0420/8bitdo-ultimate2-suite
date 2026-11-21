# `main` ブランチ保護（GitHub Rulesets）

`main` は GitHub Rulesets で保護します。このドキュメントの設定は **PR #1 のマージ前に有効化し、検証します**。

## 適用するルール

| 項目 | 設定 |
| --- | --- |
| Enforcement | Active |
| Target | `main`（default branch） |
| Pull request | 必須（直接 push 禁止） |
| Required approvals | `0`（ソロ運用。自分自身は approve できないため） |
| Required status checks | `fmt` / `clippy` / `test` / `msrv` / `review-gate` |
| Require branches up to date | 有効（`strict_required_status_checks_policy: true`） |
| Conversation resolution | 有効 |
| Linear history | 有効（squash merge のみ） |
| Force push | 禁止（`non_fast_forward`） |
| Branch deletion | 禁止（`deletion`） |
| Bypass actors | なし（管理者も直接 push 不可） |

`review-gate` は **commit status** です。PR の head SHA に `success` を付けると満たされます。新しいコミットを push すると SHA が変わり、status が無いため自動的にマージできなくなります。

## 作成コマンド

```sh
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
gh api --method POST "repos/$REPO/rulesets" --input /tmp/8bitdo-main-ruleset.json
```

## 確認コマンド

```sh
REPO=haruki0420/8bitdo-ultimate2-suite

# 一覧
gh api "repos/$REPO/rulesets" --jq '.[] | {id, name, enforcement, target}'

# 詳細（id は一覧の値）
gh api "repos/$REPO/rulesets/<id>" --jq '{name, enforcement, rules}'

# PR が保護でブロックされているか
gh pr view <PR> --repo "$REPO" --json mergeStateStatus,statusCheckRollup
```

`mergeStateStatus` が `BLOCKED` で、`review-gate` が不足していることを確認してから status を付けます。

## `review-gate` status の付け方

レビューした SHA を固定し、投稿直前に head が動いていないことを確認します。

```sh
REPO=haruki0420/8bitdo-ultimate2-suite
PR=1
REVIEWED_SHA=<レビューした head SHA>

current=$(gh pr view "$PR" --repo "$REPO" --json headRefOid --jq .headRefOid)
if [ "$current" != "$REVIEWED_SHA" ]; then
  echo "head moved: reviewed=$REVIEWED_SHA current=$current" >&2
  exit 1
fi

gh api --method POST "repos/$REPO/statuses/$REVIEWED_SHA" \
  -f state=success \
  -f context=review-gate \
  -f description="Independent review of ${REVIEWED_SHA:0:7}" \
  -f target_url="https://github.com/$REPO/pull/$PR"

# 確認
gh api "repos/$REPO/commits/$REVIEWED_SHA/status" --jq '{state, statuses: [.statuses[] | {context, state, creator}]}'
```

修正が必要な場合は `state=failure` で同じ context に status を付けます。同じ SHA への最新 status が有効になるため、**レビュアーは自分の status が上書きされていないか確認してください**。

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
