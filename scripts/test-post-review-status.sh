#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TARGET="$ROOT_DIR/scripts/post-review-status.sh"
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/post-review-status-test.XXXXXX")
trap 'rm -rf "$TMP_DIR"' EXIT

command -v jq >/dev/null || { echo "jq is required for this test" >&2; exit 1; }

MOCK_BIN="$TMP_DIR/bin"
mkdir -p "$MOCK_BIN"
cat > "$MOCK_BIN/gh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" = "pr" ] && [ "${2:-}" = "view" ]; then
  echo "${MOCK_HEAD:-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}"
  exit 0
fi

if [ "${1:-}" = "api" ]; then
  if [[ "$*" == *"--method POST"* ]]; then
    printf '%s\n' "$*" >> "${MOCK_POST_LOG:?}"
    exit 0
  fi

  jq_expr=""
  args=("$@")
  for ((i = 0; i < ${#args[@]}; i++)); do
    if [ "${args[$i]}" = "--jq" ]; then
      jq_expr="${args[$((i + 1))]}"
    fi
  done
  [ -n "$jq_expr" ] || { echo "mock gh: missing --jq" >&2; exit 1; }

  if [[ "$*" == *"/statuses"* ]]; then
    [ "${MOCK_STATUS_API_FAIL:-0}" = "1" ] && exit 1
    if [[ "$*" == *"--slurp"* ]]; then
      printf '%s\n' "[${MOCK_STATUSES:-[]}]" | jq -r "$jq_expr"
    else
      printf '%s\n' "${MOCK_STATUSES:-[]}" | jq -r "$jq_expr"
    fi
    exit 0
  fi

  if [[ "$*" == *"/check-runs"* ]]; then
    [ "${MOCK_CHECK_API_FAIL:-0}" = "1" ] && exit 1
    check_runs=${MOCK_CHECK_RUNS:-}
    [ -n "$check_runs" ] || check_runs='{"check_runs":[]}'
    if [[ "$*" == *"--slurp"* ]]; then
      printf '%s\n' "[$check_runs]" | jq -r "$jq_expr"
    else
      printf '%s\n' "$check_runs" | jq -r "$jq_expr"
    fi
    exit 0
  fi
fi

echo "mock gh: unexpected invocation: $*" >&2
exit 1
MOCK
chmod +x "$MOCK_BIN/gh"

export PATH="$MOCK_BIN:$PATH"
export REVIEW_REPO="test/repo"
export MOCK_POST_LOG="$TMP_DIR/post.log"
SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

reset_case() {
  : > "$MOCK_POST_LOG"
  export MOCK_HEAD="$SHA"
  unset MOCK_STATUS_API_FAIL MOCK_CHECK_API_FAIL || true
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

# 4. latest check-run conclusion is failure -> exit 1 + failure post.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"name":"review-gate","status":"completed","conclusion":"success","app":{"slug":"github-actions","id":1},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"},{"name":"review-gate","status":"completed","conclusion":"failure","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T01:00:00Z","completed_at":"2026-11-21T01:00:01Z"}]}'
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

# 9. skipped check-run -> exit 0 + warning, no failure post.
reset_case
export MOCK_STATUSES='[{"context":"review-gate","state":"success"}]'
export MOCK_CHECK_RUNS='{"check_runs":[{"name":"review-gate","status":"completed","conclusion":"skipped","app":{"slug":"test-app","id":2},"started_at":"2026-11-21T00:00:00Z","completed_at":"2026-11-21T00:00:01Z"}]}'
run_target_stderr 1 "$SHA" 1
assert_exit 0 "$RUN_CODE" "skipped check-run"
assert_stderr "suspicious" "skipped check-run"
assert_posted "-f state=success" "skipped check-run"
refute_posted "-f state=failure" "skipped check-run"

echo "PASS: post-review-status.sh regression matrix"
