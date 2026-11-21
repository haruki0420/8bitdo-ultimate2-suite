#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 <pr-number> <reviewed-sha> <tier:1|2|3> [state:success|failure]" >&2
}

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
  usage
  exit 2
fi

REPO="${REVIEW_REPO:-haruki0420/8bitdo-ultimate2-suite}"
PR=$1
REVIEWED_SHA=$2
TIER=$3
STATE=${4:-success}

case "$TIER" in
  1|2|3) ;;
  *)
    echo "TIER must be 1, 2 or 3" >&2
    exit 2
    ;;
esac

case "$STATE" in
  success|failure) ;;
  *)
    echo "STATE must be success or failure" >&2
    exit 2
    ;;
esac

if ! [[ "$REVIEWED_SHA" =~ ^[0-9a-f]{40}$ ]]; then
  echo "reviewed-sha must be a 40-character lowercase hex SHA" >&2
  exit 2
fi

current=$(gh pr view "$PR" --repo "$REPO" --json headRefOid --jq .headRefOid)
if [ "$current" != "$REVIEWED_SHA" ]; then
  echo "head moved: reviewed=$REVIEWED_SHA current=$current" >&2
  exit 1
fi

description="Tier ${TIER} review of ${REVIEWED_SHA:0:7}"
if [ "$STATE" = "failure" ]; then
  description="$description (changes requested)"
fi

# The tier argument must be 1/2/3. This script does not verify the PR's
# declared Tier or quorum; those are procedural checks recorded in the PR.
# Tier 1: the reviewer posts. Tier 2/3: the lead reviewer posts only after all
# reviewers have approved this exact SHA.
gh api --method POST "repos/$REPO/statuses/$REVIEWED_SHA" \
  -f state="$STATE" \
  -f context=review-gate \
  -f description="$description" \
  -f target_url="https://github.com/$REPO/pull/$PR"

# Audit is mandatory. If it cannot run, fail closed after posting rather than
# reporting success without evidence.
if ! gh api "repos/$REPO/commits/$REVIEWED_SHA/statuses" \
  --jq '[.[] | {context, state, description, creator: .creator.login, created_at}]'; then
  echo "error: post-audit of commit statuses failed; verify manually before merging" >&2
  exit 1
fi

# A review-gate check run is gate-satisfying because the ruleset has no
# integration_id. Audit all check runs (filter=all), not only the latest.
if ! gh api "repos/$REPO/commits/$REVIEWED_SHA/check-runs?filter=all" \
  --jq '[.check_runs[] | {name, status, conclusion, details_url}]'; then
  echo "error: post-audit of check runs failed; verify manually before merging" >&2
  exit 1
fi

# Manual regression matrix (temporary mock `gh` on PATH, no network;
# verified 2026-11-21):
#   scripts/post-review-status.sh 1 <40hex> 9       -> 2 (invalid tier)
#   scripts/post-review-status.sh 1 bad 1           -> 2 (invalid SHA)
#   MOCK_HEAD=<other-sha> scripts/... 1 <40hex> 1   -> 1 (head mismatch)
#   scripts/post-review-status.sh 1 <40hex> 1       -> 0 (success)
#   scripts/post-review-status.sh 1 <40hex> 1 failure -> 0 (failure state)
#   MOCK_AUDIT_FAIL=1 scripts/... 1 <40hex> 1       -> 1 (fail closed)
