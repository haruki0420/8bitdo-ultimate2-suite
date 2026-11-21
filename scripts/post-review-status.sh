#!/usr/bin/env bash
# Post and audit the review-gate commit status for a reviewed head SHA.
#
# Requires gh (verified with 2.105.0) and jq (verified with 1.8.2).
# gh 2.105.0 rejects `gh api --slurp` combined with `--jq`/`--template`, so
# every audit pipes the raw paginated pages to jq instead.
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

# jq is required for the audit. Fail before posting anything so a missing
# dependency cannot leave a success status without an audit.
if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required to audit review-gate statuses and check runs" >&2
  echo "install jq (verified with 1.8.2) and retry; no status was posted" >&2
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
    echo "residual risk: an earlier review-gate=success (commit status or check run) may still satisfy the gate" >&2
    echo "manual action: do not merge. Have the check-run owner update/re-run it as failure, or push a new commit after fixing (the new SHA has no gate status)." >&2
    return 1
  fi
}

current=$(gh pr view "$PR" --repo "$REPO" --json headRefOid --jq .headRefOid)
if [ "$current" != "$REVIEWED_SHA" ]; then
  echo "head moved: reviewed=$REVIEWED_SHA current=$current" >&2
  exit 1
fi

# Late CHANGES_REQUESTED guard (best effort). A review can arrive after this
# check, so the documented rule still requires a lead to post
# review-gate=failure immediately when a blocker appears on an already-approved
# SHA, then re-review the new SHA.
if [ "$STATE" = "success" ]; then
  if ! review_decision=$(
    gh pr view "$PR" --repo "$REPO" --json reviewDecision \
      | jq -r '.reviewDecision // ""'
  ); then
    echo "error: could not read PR reviewDecision; refusing to post review-gate=success" >&2
    post_audit_failure || true
    exit 1
  fi
  if [ "$review_decision" = "CHANGES_REQUESTED" ]; then
    echo "error: PR reviewDecision is CHANGES_REQUESTED; refusing to post review-gate=success" >&2
    echo "re-review after the requested changes are pushed; do not override a changes-requested review" >&2
    if ! post_status failure "Tier ${TIER} changes requested for ${REVIEWED_SHA:0:7}"; then
      echo "manual action: post review-gate=failure before merging" >&2
    fi
    exit 1
  fi
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

# The audit is an assertion, not a print. It reads all pages of both
# /statuses and /check-runs?filter=all, selects the latest review-gate check
# run by check-run `id` (queued runs have no timestamps), and compares the
# result against STATE:
#   success: no run, success, skipped, or neutral is acceptable
#            (skipped/neutral are suspicious); anything else is an anomaly.
#   failure: failure/cancelled/timed_out/action_required/stale/startup_failure,
#            a missing conclusion, or no run is acceptable because the gate is
#            blocked; success/skipped/neutral defeats the failure and is an
#            anomaly.
# A check run cannot be removed by this script. If a gate-satisfying check run
# exists alongside review-gate=failure, the run must be re-run/updated by its
# owner or the SHA must be superseded by a new commit.
expected_state=$STATE
status_state=""
check_conclusion=""
check_status=""
check_app=""
check_app_id=""
check_started=""
check_completed=""
check_id=""
audit_failed=0
audit_suspicious=0

# gh --slurp returns one JSON array containing every page. Pipe it to jq;
# gh 2.105.0 rejects --slurp together with --jq.
if ! status_state=$(gh api --paginate --slurp "repos/$REPO/commits/$REVIEWED_SHA/statuses" \
  | jq -r 'flatten | [.[] | select(.context == "review-gate")] | if length == 0 then "MISSING" else .[0].state end'); then
  echo "error: could not audit review-gate commit statuses" >&2
  audit_failed=1
elif [ "$status_state" != "$expected_state" ]; then
  echo "error: latest review-gate commit status is '$status_state', expected '$expected_state'" >&2
  audit_failed=1
fi

if ! check_line=$(gh api --paginate --slurp "repos/$REPO/commits/$REVIEWED_SHA/check-runs?filter=all" \
  | jq -r '([.[] | .check_runs[]] | [.[] | select(.name == "review-gate")] | if length == 0 then "NONE|||null|||none" else (max_by(.id) | [(.conclusion // "MISSING"), (.status // ""), (.app.slug // ""), ((.app.id // "") | tostring), (.started_at // ""), (.completed_at // ""), ((.id // "") | tostring)] | join("|")) end)'); then
  echo "error: could not audit review-gate check runs" >&2
  audit_failed=1
else
  IFS='|' read -r check_conclusion check_status check_app check_app_id check_started check_completed check_id <<< "$check_line"
  case "$STATE" in
    success)
      case "$check_conclusion" in
        NONE|success) ;;
        skipped|neutral)
          echo "warning: latest review-gate check run conclusion is '$check_conclusion' (id=${check_id:-none}); GitHub accepts it but it is suspicious" >&2
          audit_suspicious=1
          ;;
        MISSING)
          echo "error: latest review-gate check run has no conclusion (status='${check_status:-unknown}', id=${check_id:-none}); expected success, skipped, neutral, or no check run" >&2
          audit_failed=1
          ;;
        *)
          echo "error: latest review-gate check run conclusion is '$check_conclusion' (status='${check_status:-unknown}', id=${check_id:-none}), expected success, skipped, neutral, or no check run" >&2
          audit_failed=1
          ;;
      esac
      ;;
    failure)
      case "$check_conclusion" in
        success|skipped|neutral)
          echo "error: gate-satisfying review-gate check run exists (conclusion='$check_conclusion', status='${check_status:-unknown}', id=${check_id:-none}) despite review-gate=failure" >&2
          audit_failed=1
          ;;
        *)
          # failure / cancelled / timed_out / action_required / stale /
          # startup_failure / MISSING conclusion / no run: gate blocked.
          ;;
      esac
      ;;
  esac
fi

if [ "$audit_failed" -ne 0 ]; then
  if ! post_audit_failure; then
    echo "review-gate audit failed and the failure status could not be posted for $REVIEWED_SHA" >&2
    exit 1
  fi
  echo "review-gate audit failed for $REVIEWED_SHA; posted review-gate=failure" >&2
  if [ "$STATE" = "failure" ]; then
    echo "residual risk: this script cannot delete a gate-satisfying check run; the failure status alone may not block the gate" >&2
    echo "manual action: do not merge. Have the check-run owner update/re-run it as failure, or push a new commit after fixing." >&2
  fi
  exit 1
fi

echo "review-gate audit OK: status=$status_state check_run=$check_conclusion check_status=${check_status:-none} check_id=${check_id:-none} app=${check_app:-none} app_id=${check_app_id:-none} started=${check_started:-none} completed=${check_completed:-none} sha=$REVIEWED_SHA"
if [ "$audit_suspicious" -ne 0 ]; then
  echo "warning: review-gate check run conclusion is '$check_conclusion'; inspect app/source before merging" >&2
fi

# Regression tests: scripts/test-post-review-status.sh (mock gh, no network).
# Covers both states, latest-by-id selection (queued/in-progress), multi-page
# responses, skipped/neutral, cancelled/timed_out/action_required, missing
# conclusion, failure-post failure, late CHANGES_REQUESTED, missing jq, and
# rejection of the invalid --slurp + --jq combination.
