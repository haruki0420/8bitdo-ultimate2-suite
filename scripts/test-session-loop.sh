#!/usr/bin/env bash
# Regression test for scripts/session-loop.sh using a stub `eightbitdo`.
#
# Covers:
#   1. exit 3 (degraded) stops the loop immediately and does not retry;
#   2. exit 0 restarts only after the 2 second BLE-teardown delay;
#   3. any other non-zero exit stops the loop (no hot retry loop);
#   4. the stub sees the configured session arguments.
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
LOOP="$ROOT_DIR/scripts/session-loop.sh"
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/session-loop-test.XXXXXX")
trap 'rm -rf "$TMP_DIR"' EXIT

STUB="$TMP_DIR/eightbitdo"
INVOCATIONS="$TMP_DIR/invocations"
ARG_LOG="$TMP_DIR/args"
COUNT_FILE="$TMP_DIR/count"

cat > "$STUB" <<'STUB_BODY'
#!/usr/bin/env bash
set -u
count=0
if [[ -f "$LOOP_TEST_COUNT_FILE" ]]; then
    count=$(cat "$LOOP_TEST_COUNT_FILE")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$LOOP_TEST_COUNT_FILE"
date +%s%N >> "$LOOP_TEST_INVOCATIONS"
printf '%s\n' "$*" >> "$LOOP_TEST_ARGS"
case "$count" in
    1) exit "${LOOP_TEST_EXIT_1:-0}" ;;
    2) exit "${LOOP_TEST_EXIT_2:-3}" ;;
    *) exit "${LOOP_TEST_EXIT_N:-3}" ;;
esac
STUB_BODY
chmod +x "$STUB"

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

run_loop() {
    : > "$INVOCATIONS"
    : > "$ARG_LOG"
    rm -f "$COUNT_FILE"
    timeout 30 env \
        EIGHTBITDO_BIN="$STUB" \
        SESSION_CMD_FILE="$TMP_DIR/sess.cmd" \
        SESSION_OUT="$TMP_DIR/sess.out" \
        SESSION_ERR="$TMP_DIR/sess.err" \
        SESSION_LOOP_BACKOFF_INITIAL=2 \
        SESSION_LOOP_BACKOFF_MAX=2 \
        LOOP_TEST_COUNT_FILE="$COUNT_FILE" \
        LOOP_TEST_INVOCATIONS="$INVOCATIONS" \
        LOOP_TEST_ARGS="$ARG_LOG" \
        "$@" \
        bash "$LOOP" --allow-macro-write --scan-timeout 30 \
        >"$TMP_DIR/loop.out" 2>&1
}

invocations() {
    if [[ -f "$INVOCATIONS" ]]; then
        wc -l < "$INVOCATIONS" | tr -d ' '
    else
        echo 0
    fi
}

# 1. Degraded exit: stop at once, no restart, no 2 second wait.
start=$(date +%s)
set +e
run_loop LOOP_TEST_EXIT_1=3
code=$?
set -e
elapsed=$(( $(date +%s) - start ))
[[ "$code" -eq 3 ]] || fail "degraded exit must propagate 3, got $code"
[[ "$(invocations)" -eq 1 ]] || fail "degraded exit must not retry, saw $(invocations) invocations"
((elapsed < 2)) || fail "degraded exit must not sleep before stopping (took ${elapsed}s)"
grep -q "WAITING FOR PHYSICAL ACTION" "$TMP_DIR/loop.out" \
    || fail "degraded log must tell the operator to press Profile"
grep -q "NOT retried automatically" "$TMP_DIR/loop.out" \
    || fail "degraded log must say the loop does not retry"

# 2. Normal exit then degraded: exactly one 2 second+ restart gap.
set +e
run_loop LOOP_TEST_EXIT_1=0 LOOP_TEST_EXIT_2=3
code=$?
set -e
[[ "$code" -eq 3 ]] || fail "second (degraded) exit must propagate 3, got $code"
[[ "$(invocations)" -eq 2 ]] || fail "expected exactly two invocations, saw $(invocations)"
first=$(sed -n '1p' "$INVOCATIONS")
second=$(sed -n '2p' "$INVOCATIONS")
gap_ms=$(( (second - first) / 1000000 ))
((gap_ms >= 1900)) || fail "restart gap must be at least 2s, was ${gap_ms}ms"

# 3. Any other non-zero exit stops instead of hot-looping.
set +e
run_loop LOOP_TEST_EXIT_1=7
code=$?
set -e
[[ "$code" -eq 7 ]] || fail "ordinary failure exit must propagate 7, got $code"
[[ "$(invocations)" -eq 1 ]] || fail "ordinary failure must not retry, saw $(invocations) invocations"

# 4. The loop forwards session arguments unchanged.
grep -q -- "--allow-macro-write --scan-timeout 30" "$ARG_LOG" \
    || fail "session arguments were not forwarded"

echo "session-loop tests passed"
