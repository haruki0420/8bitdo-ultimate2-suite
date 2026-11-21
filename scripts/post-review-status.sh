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

# Tier 1: the reviewer posts. Tier 2/3: the lead reviewer posts only after all
# reviewers have approved this exact SHA. This script does not verify quorum;
# that is a procedural check recorded in the PR.
gh api --method POST "repos/$REPO/statuses/$REVIEWED_SHA" \
  -f state="$STATE" \
  -f context=review-gate \
  -f description="$description" \
  -f target_url="https://github.com/$REPO/pull/$PR"

# Audit: list every status for this SHA (newest first), not only the latest.
gh api "repos/$REPO/commits/$REVIEWED_SHA/statuses" \
  --jq '[.[] | {context, state, description, creator: .creator.login, created_at}]' \
  || echo "warning: could not list commit statuses" >&2

# Check runs are separate from commit statuses; audit them too.
gh api "repos/$REPO/commits/$REVIEWED_SHA/check-runs" \
  --jq '[.check_runs[] | {name, status, conclusion, details_url}]' \
  || echo "warning: could not list check runs" >&2
