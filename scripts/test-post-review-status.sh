#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TARGET="$ROOT_DIR/scripts/post-review-status.sh"
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/post-review-status-test.XXXXXX")
trap 'rm -rf "$TMP_DIR"' EXIT

command -v jq >/dev/null || { echo "jq is required for this test" >&2; exit 1; }
BASH_BIN=$(command -v bash)

MOCK_BIN="$TMP_DIR/bin"
mkdir -p "$MOCK_BIN"
cat > "$MOCK_BIN/gh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" = "pr" ] && [ "${2:-}" = "view" ]; then
  json_field=""
  jq_expr=""
  args=("$@")
  for ((i = 0; i < ${#args[@]}; i++)); do
    case "${args[$i]}" in
      --json) json_field="${args[$((i + 1))]:-}" ;;
      --jq) jq_expr="${args[$((i + 1))]:-}" ;;
    esac
  done

  case "$json_field" in
    headRefOid)
      payload=$(printf '{"headRefOid":"%s"}' "${MOCK_HEAD:-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}")
      ;;
    reviewDecision)
      [ "${MOCK_REVIEW_DECISION_API_FAIL:-0}" = "1" ] && exit 1
      if [ -n "${MOCK_REVIEW_DECISION:-}" ]; then
        payload=$(printf '{"reviewDecision":"%s"}' "$MOCK_REVIEW_DECISION")
      else
        payload='{"reviewDecision":null}'
      fi
      ;;
    *)
      echo "mock gh: unsupported pr view --json '${json_field}'" >&2
      exit 1
      ;;
  esac

  if [ -n "$jq_expr" ]; then
    printf '%s\n' "$payload" | jq -r "$jq_expr"
  else
    printf '%s\n' "$payload"
  fi
  exit 0
fi

if [ "${1:-}" = "api" ]; then
  printf '%s\n' "$*" >> "${MOCK_INVOCATION_LOG:-/dev/null}"
  has_slurp=0
  jq_expr=""
  args=("$@")
  for ((i = 0; i < ${#args[@]}; i++)); do
    case "${args[$i]}" in
      --slurp) has_slurp=1 ;;
      --jq) jq_expr="${args[$((i + 1))]:-}" ;;
    esac
  done

  # gh 2.105.0 rejects this combination. The mock must reject it too, so the
  # regression test fails if the script reverts to `gh api --slurp ... --jq`.
  if [ "$has_slurp" = "1" ] && [ -n "$jq_expr" ]; then
    echo "mock gh: the --slurp option is not supported with --jq or --template" >&2
    exit 1
  fi

  if [[ "$*" == *"--method POST"* ]]; then
    if [ "${MOCK_FAILURE_POST_FAIL:-0}" = "1" ] && [[ "$*" == *"-f state=failure"* ]]; then
      exit 1
    fi
    printf '%s\n' "$*" >> "${MOCK_POST_LOG:?}"
    exit 0
  fi

  if [[ "$*" == *"/statuses"* ]]; then
    [ "${MOCK_STATUS_API_FAIL:-0}" = "1" ] && exit 1
    pages=${MOCK_STATUS_PAGES:-}
    [ -n "$pages" ] || pages="[[${MOCK_STATUSES:-[]}]]"
  elif [[ "$*" == *"/check-runs"* ]]; then
    [ "${MOCK_CHECK_API_FAIL:-0}" = "1" ] && exit 1
    pages=${MOCK_CHECK_PAGES:-}
    if [ -z "$pages" ]; then
      runs=${MOCK_CHECK_RUNS:-}
      [ -n "$runs" ] || runs='{"check_runs":[]}'
      pages="[${runs}]"
    fi
  else
    echo "mock gh: unexpected api endpoint: $*" >&2
    exit 1
  fi

  if [ "$has_slurp" = "1" ]; then
    printf '%s\n' "$pages"
  elif [ -n "$jq_expr" ]; then
    printf '%s\n' "$pages" | jq -c '.[]' | while IFS= read -r page; do
      printf '%s\n' "$page" | jq -r "$jq_expr"
    done
  else
    printf '%s\n' "$pages" | jq -c '.[]'
  fi
  exit 0
fi

echo "mock gh: unexpected invocation: $*" >&2
exit 1
MOCK
chmod +x "$MOCK_BIN/gh"

export PATH="$MOCK_BIN:$PATH"
export REVIEW_REPO="test/repo"
export MOCK_POST_LOG="$TMP_DIR/post.log"
export MOCK_INVOCATION_LOG="$TMP_DIR/invocations.log"
SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

reset_case() {
  : > "$MOCK_POST_LOG"
  export MOCK_HEAD="$SHA"
  unset MOCK_STATUS_API_FAIL MOCK_CHECK_API_FAIL MOCK_STATUSES MOCK_STATUS_PAGES \
    MOCK_CHECK_RUNS MOCK_CHECK_PAGES MOCK_REVIEW_DECISION \
    MOCK_REVIEW_DECISION_API_FAIL MOCK_FAILURE_POST_FAIL 2>/dev/null || true
}

run_target() {
  set +e
  bash "$TARGET" "$@" >/dev/null 2>"$TMP_DIR/target.err"
  RUN_CODE=$?
  set -e
  return 0
}

run_target_stderr() {
  set +e
  bash "$TARGET" "$@" >/dev/null 2>"$TMP_DIR/stderr.log"
  RUN_CODE=$?
  set -e
  return 0
}

assert_stderr() {
  grep -q "$1" "$TMP_DIR/target.err" || fail "$2: missing '$1' in stderr"
}

assert_log_stderr() {
  grep -q "$1" "$TMP_DIR/stderr.log" || fail "$2: missing '$1' in stderr"
}

assert_exit() {
  local expected=$1 actual=$2 name=$3
  if [ "$actual" -ne "$expected" ]; then
    [ -s "$TMP_DIR/target.err" ] && cat "$TMP_DIR/target.err" >&2
    fail "$name: expected exit $expected, got $actual"
  fi
}

assert_posted() {
  grep -q -- "$1" "$MOCK_POST_LOG" || fail "$2: missing '$1' in POST log"
}

refute_posted() {
  if grep -q -- "$1" "$MOCK_POST_LOG"; then fail "$2: unexpected '$1' in POST log"; fi
}

# 1. success + empty check-runs -> exit 0, no failure post.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success","description":"Tier 1 review","created_at":"2026-11-21T00:00:00Z"}]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
run_target 1 "$SHA" 1
assert_exit 0 "$RUN_CODE" "success + empty check-runs"
assert_posted "-f state=success" "success + empty check-runs"
refute_posted "-f state=failure" "success + empty check-runs"

# 2. empty /statuses after posting -> exit 1 + failure post.
reset_case
export MOCK_STATUSES='[]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "empty /statuses"
assert_posted "-f state=failure" "empty /statuses"

# 3. latest /statuses is failure after success -> exit 1 + failure post.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"failure"},{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "latest /statuses failure"
assert_posted "-f state=failure" "latest /statuses failure"

# 4. latest check-run by id is failure -> exit 1 + failure post.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":1,"name":"review-gate","status":"completed","conclusion":"success","app":{"slug":"github-actions","id":1},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"},{"id":2,"name":"review-gate","status":"completed","conclusion":"failure","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T01:00:00Z","completed_at":"2026-11-21T01:00:01Z"}]}'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "check-run failure"
assert_posted "-f state=failure" "check-run failure"

# 5. head mismatch -> exit 1, no status posted.
reset_case
export MOCK_HEAD=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "head mismatch"
[ ! -s "$MOCK_POST_LOG" ] || fail "head mismatch: POST log should be empty"

# 6. invalid tier -> exit 2, no status posted.
reset_case
run_target 1 "$SHA" 9
assert_exit 2 "$RUN_CODE" "invalid tier"
[ ! -s "$MOCK_POST_LOG" ] || fail "invalid tier: POST log should be empty"

# 7. status API failure -> exit 1 + failure post.
reset_case
export MOCK_STATUS_API_FAIL=1
export MOCK_STATUSES='[]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "status API failure"
assert_posted "-f state=failure" "status API failure"

# 8. check API failure -> exit 1 + failure post.
reset_case
export MOCK_CHECK_API_FAIL=1
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "check API failure"
assert_posted "-f state=failure" "check API failure"

# 9. skipped check-run in success state -> exit 0 + warning, no failure post.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":1,"name":"review-gate","status":"completed","conclusion":"skipped","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"}]}'
run_target_stderr 1 "$SHA" 1
assert_exit 0 "$RUN_CODE" "skipped check-run"
assert_log_stderr "suspicious" "skipped check-run"
assert_posted "-f state=success" "skipped check-run"
refute_posted "-f state=failure" "skipped check-run"

# 10. neutral check-run in success state -> exit 0 + warning.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":1,"name":"review-gate","status":"completed","conclusion":"neutral","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"}]}'
run_target_stderr 1 "$SHA" 1
assert_exit 0 "$RUN_CODE" "neutral check-run"
assert_log_stderr "suspicious" "neutral check-run"
assert_posted "-f state=success" "neutral check-run"
refute_posted "-f state=failure" "neutral check-run"

# 11. latest selection uses id, not timestamps: queued run has no started_at.
# The old sort_by(.started_at) would select the completed success run.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":2,"name":"review-gate","status":"queued","conclusion":null,"app":{"slug":"test-app","id":2},"started_at":null,"completed_at":null},{"id":1,"name":"review-gate","status":"completed","conclusion":"success","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T01:00:00Z","completed_at":"2026-11-21T01:00:01Z"}]}'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "queued latest by id"
assert_posted "-f state=failure" "queued latest by id"

# 12. in-progress run with a missing conclusion is an anomaly in success state.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":3,"name":"review-gate","status":"in_progress","conclusion":null,"app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":null}]}'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "in-progress missing conclusion"
assert_posted "-f state=failure" "in-progress missing conclusion"

# 13. multi-page statuses: page 1 is newest -> success.
reset_case
export MOCK_STATUS_PAGES='[[{"context":"review-gate","state":"success","created_at":"2026-11-21T02:00:00Z"}],[{"context":"review-gate","state":"failure","created_at":"2026-11-21T01:00:00Z"}]]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
run_target 1 "$SHA" 1
assert_exit 0 "$RUN_CODE" "multi-page statuses"
assert_posted "-f state=success" "multi-page statuses"
refute_posted "-f state=failure" "multi-page statuses"

# 14. multi-page check runs: page 2 holds the latest id (failure) -> exit 1.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_PAGES='[{"check_runs":[{"id":5,"name":"review-gate","status":"completed","conclusion":"success","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"}]},{"check_runs":[{"id":10,"name":"review-gate","status":"completed","conclusion":"failure","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T01:00:00Z","completed_at":"2026-11-21T01:00:01Z"}]}]'
run_target 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "multi-page check runs latest"
assert_posted "-f state=failure" "multi-page check runs latest"

# 15. multi-page check runs: page 2 holds an older failure -> success.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_PAGES='[{"check_runs":[{"id":10,"name":"review-gate","status":"completed","conclusion":"success","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T01:00:00Z","completed_at":"2026-11-21T01:00:01Z"}]},{"check_runs":[{"id":5,"name":"review-gate","status":"completed","conclusion":"failure","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"}]}]'
run_target 1 "$SHA" 1
assert_exit 0 "$RUN_CODE" "multi-page check runs older failure"
assert_posted "-f state=success" "multi-page check runs older failure"
refute_posted "-f state=failure" "multi-page check runs older failure"

# 16. STATE=failure + a success check run defeats the failure -> anomaly.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"failure"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":1,"name":"review-gate","status":"completed","conclusion":"success","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"}]}'
run_target 1 "$SHA" 1 failure
assert_exit 1 "$RUN_CODE" "failure + success check-run"
assert_posted "-f state=failure" "failure + success check-run"

# 17. STATE=failure + skipped defeats the failure -> anomaly.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"failure"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":1,"name":"review-gate","status":"completed","conclusion":"skipped","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"}]}'
run_target 1 "$SHA" 1 failure
assert_exit 1 "$RUN_CODE" "failure + skipped check-run"
assert_posted "-f state=failure" "failure + skipped check-run"

# 18. STATE=failure + neutral defeats the failure -> anomaly.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"failure"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":1,"name":"review-gate","status":"completed","conclusion":"neutral","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"}]}'
run_target 1 "$SHA" 1 failure
assert_exit 1 "$RUN_CODE" "failure + neutral check-run"
assert_posted "-f state=failure" "failure + neutral check-run"

# 19-22. STATE=failure with non-gate-satisfying conclusions -> exit 0.
for conclusion in failure cancelled timed_out action_required; do
  reset_case
  export MOCK_STATUSES='[{"context":"review-gate","state":"failure"}]'
  export MOCK_CHECK_RUNS="{\"check_runs\":[{\"id\":1,\"name\":\"review-gate\",\"status\":\"completed\",\"conclusion\":\"$conclusion\",\"app\":{\"slug\":\"test-app\",\"id\":2},\"started_at\":\"2026-11-21T00:00:00Z\",\"completed_at\":\"2026-11-21T00:00:01Z\"}]}"
  run_target 1 "$SHA" 1 failure
  assert_exit 0 "$RUN_CODE" "failure + $conclusion check-run"
  assert_posted "-f state=failure" "failure + $conclusion check-run"
done

# 23. STATE=failure + no check run -> exit 0.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"failure"}]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
run_target 1 "$SHA" 1 failure
assert_exit 0 "$RUN_CODE" "failure + no check-run"
assert_posted "-f state=failure" "failure + no check-run"

# 24. STATE=failure + queued run (missing conclusion) -> exit 0.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"failure"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"id":1,"name":"review-gate","status":"queued","conclusion":null,"app":{"slug":"test-app","id":2},"started_at":null,"completed_at":null}]}'
run_target 1 "$SHA" 1 failure
assert_exit 0 "$RUN_CODE" "failure + queued check-run"
assert_posted "-f state=failure" "failure + queued check-run"

# 25. late CHANGES_REQUESTED blocks success before it is posted.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"failure"}]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
export MOCK_REVIEW_DECISION=CHANGES_REQUESTED
run_target 1 "$SHA" 3
assert_exit 1 "$RUN_CODE" "late CHANGES_REQUESTED"
refute_posted "-f state=success" "late CHANGES_REQUESTED"
assert_posted "-f state=failure" "late CHANGES_REQUESTED"

# 26. reviewDecision API failure blocks success.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"failure"}]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
export MOCK_REVIEW_DECISION_API_FAIL=1
run_target 1 "$SHA" 3
assert_exit 1 "$RUN_CODE" "reviewDecision API failure"
refute_posted "-f state=success" "reviewDecision API failure"
assert_posted "-f state=failure" "reviewDecision API failure"

# 27. failure-post failure leaves a residual success -> exit 1 + manual action.
reset_case
export MOCK_STATUSES='[]'
export MOCK_CHECK_RUNS='{"check_runs":[]}'
export MOCK_FAILURE_POST_FAIL=1
run_target_stderr 1 "$SHA" 1
assert_exit 1 "$RUN_CODE" "failure-post failure"
assert_log_stderr "residual risk" "failure-post failure"
assert_log_stderr "manual action" "failure-post failure"

# 28. missing jq exits 2 before any gh call.
reset_case
mkdir -p "$TMP_DIR/no-jq"
set +e
PATH="$TMP_DIR/no-jq" "$BASH_BIN" "$TARGET" 1 "$SHA" 1 >/dev/null 2>"$TMP_DIR/stderr.log"
JQ_CODE=$?
set -e
if [ "$JQ_CODE" -ne 2 ]; then fail "missing jq: expected exit 2, got $JQ_CODE"; fi
grep -q "jq is required" "$TMP_DIR/stderr.log" || fail "missing jq: missing precondition message"
[ ! -s "$MOCK_POST_LOG" ] || fail "missing jq: POST log should be empty"

# 29. the target never used --slurp together with --jq (static regression).
if grep -Eq -- '--slurp.*--jq|--jq.*--slurp' "$MOCK_INVOCATION_LOG"; then
  fail "target used the unsupported --slurp + --jq combination: $(grep -E -- '--slurp.*--jq|--jq.*--slurp' "$MOCK_INVOCATION_LOG" | tail -1)"
fi

# 30. the mock rejects the invalid --slurp + --jq combination itself.
set +e
gh api --paginate --slurp "repos/test/repo/commits/$SHA/statuses" --jq '.' \
  >/dev/null 2>"$TMP_DIR/slurp.err"
SLURP_CODE=$?
set -e
if [ "$SLURP_CODE" -eq 0 ]; then fail "--slurp + --jq: mock gh accepted the invalid combination"; fi
grep -q "not supported" "$TMP_DIR/slurp.err" || fail "--slurp + --jq: missing rejection message"

echo "PASS: post-review-status.sh regression matrix"
