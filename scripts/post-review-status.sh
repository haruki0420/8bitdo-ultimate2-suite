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

post_status() {
  local state=$1
  local description=$2
  gh api --method POST "repos/$REPO/statuses/$REVIEWED_SHA" \
    -f state="$state" \
    -f context=review-gate \
    -f description="$description" \
    -f target_url="https://github.com/$REPO/pull/$PR"
}

post_audit_failure() {
  local description="Tier ${TIER} audit failed for ${REVIEWED_SHA:0:7}"
  if ! post_status failure "$description"; then
    echo "error: could not post review-gate=failure for $REVIEWED_SHA" >&2
    echo "manual action required: post review-gate=failure before merging" >&2
    return 1
  fi
}

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
post_status "$STATE" "$description"

# The audit is an assertion, not a print. If the latest status/check run is
# missing or does not match the posted state, post review-gate=failure and
# exit non-zero so a stale success cannot satisfy the gate.
expected_state=$STATE
status_state=""
check_state=""
check_app=""
check_app_id=""
check_started=""
check_completed=""
audit_failed=0
audit_suspicious=0

if ! status_state=$(gh api --paginate --slurp "repos/$REPO/commits/$REVIEWED_SHA/statuses" \
  --jq 'flatten | [.[] | select(.context == "review-gate")] | if length == 0 then "MISSING" else .[0].state end'); then
  echo "error: could not audit review-gate commit statuses" >&2
  audit_failed=1
elif [ "$status_state" != "$expected_state" ]; then
  echo "error: latest review-gate commit status is '$status_state', expected '$expected_state'" >&2
  audit_failed=1
fi

if ! check_line=$(gh api --paginate --slurp "repos/$REPO/commits/$REVIEWED_SHA/check-runs?filter=all" \
  --jq '([.[] | .check_runs[]] | [.[] | select(.name == "review-gate")] | if length == 0 then "NONE||||" else (sort_by(.started_at // .completed_at // "") | last | [(.conclusion // "MISSING"), (.app.slug // ""), ((.app.id // "") | tostring), (.started_at // ""), (.completed_at // "")] | join("|")) end)'); then
  echo "error: could not audit review-gate check runs" >&2
  audit_failed=1
else
  IFS='|' read -r check_state check_app check_app_id check_started check_completed <<< "$check_line"
  case "$check_state" in
    NONE|success) ;;
    skipped|neutral)
      echo "warning: latest review-gate check run conclusion is '$check_state'; GitHub accepts it but it is suspicious" >&2
      audit_suspicious=1
      ;;
    *)
      echo "error: latest review-gate check run conclusion is '$check_state', expected success, skipped, neutral, or no check run" >&2
      audit_failed=1
      ;;
  esac
fi

if [ "$audit_failed" -ne 0 ]; then
  if ! post_audit_failure; then
    exit 1
  fi
  echo "review-gate audit failed for $REVIEWED_SHA; posted review-gate=failure" >&2
  exit 1
fi

echo "review-gate audit OK: status=$status_state check_run=$check_state app=${check_app:-none} app_id=${check_app_id:-none} started=${check_started:-none} completed=${check_completed:-none} sha=$REVIEWED_SHA"
if [ "$audit_suspicious" -ne 0 ]; then
  echo "warning: review-gate check run conclusion is '$check_state'; inspect app/source before merging" >&2
fi

# Regression tests: scripts/test-post-review-status.sh (mock gh, no network).
# Covers success + empty check-runs, empty statuses, latest status failure,
# check-run failure, head mismatch, invalid tier, status API failure, skipped.
