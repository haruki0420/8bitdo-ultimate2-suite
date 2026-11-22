#!/usr/bin/env bash
# Regression test for scripts/session-loop.sh using a stub `eightbitdo`.
#
# Covers:
#   1. exit 3 (degraded) stops the loop immediately, without a scan or retry;
#   2. exit 0 + `80ND` still advertising restarts after the 2 second delay;
#   3. exit 0 + `80ND` absent stops with the physical-press guidance;
#   4. exit 0 + scan failure (lock contention) stops without retrying;
#   5. any other non-zero exit stops the loop (no hot retry loop);
#   6. the exponential backoff grows after consecutive quick normal exits;
#   7. a long-enough session resets the backoff;
#   8. commands appended during backoff are delivered to the next session;
#   9. SIGTERM stops the supervisor and cleans up (exit 143).
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
LOOP="$ROOT_DIR/scripts/session-loop.sh"
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/session-loop-test.XXXXXX")
trap 'rm -rf "$TMP_DIR"' EXIT

STUB="$TMP_DIR/eightbitdo"
INVOCATIONS="$TMP_DIR/invocations"
SCAN_INVOCATIONS="$TMP_DIR/scans"
ARG_LOG="$TMP_DIR/args"
STDIN_LOG="$TMP_DIR/stdin"
COUNT_FILE="$TMP_DIR/count"

cat > "$STUB" <<'STUB_BODY'
#!/usr/bin/env bash
set -u
if [[ "${1:-}" == "scan" ]]; then
    printf '%s\n' "$*" >> "$LOOP_TEST_SCAN_INVOCATIONS"
    case "${LOOP_TEST_SCAN_MODE:-visible}" in
        visible)
            printf '%s\n' "NAME                 ADDRESS              RSSI  MODE"
            printf '%s\n' "80ND                 aa:bb:cc:dd:ee:ff      -40  App Mode"
            exit 0
            ;;
        absent)
            printf '%s\n' "no BLE devices found within 1 seconds"
            exit 0
            ;;
        error)
            printf '%s\n' "error: another process is using the BLE adapter (lock file /tmp/x)" >&2
            exit 1
            ;;
        *)
            printf '%s\n' "stub: unknown LOOP_TEST_SCAN_MODE" >&2
            exit 2
            ;;
    esac
fi

count=0
if [[ -f "$LOOP_TEST_COUNT_FILE" ]]; then
    count=$(cat "$LOOP_TEST_COUNT_FILE")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$LOOP_TEST_COUNT_FILE"
date +%s%N >> "$LOOP_TEST_INVOCATIONS"
printf '%s\n' "$*" >> "$LOOP_TEST_ARGS"

if [[ "${LOOP_TEST_READ_STDIN_1:-0}" == "1" || "${LOOP_TEST_READ_STDIN_2:-0}" == "1" ]]; then
    read_var="LOOP_TEST_READ_STDIN_${count}"
    if [[ "${!read_var:-0}" == "1" ]]; then
        if IFS= read -r line; then
            printf '%s\n' "$line" >> "$LOOP_TEST_STDIN_LOG"
        fi
    fi
fi

sleep_var="LOOP_TEST_SLEEP_${count}"
sleep_seconds=${!sleep_var:-0}
if [[ "$sleep_seconds" != "0" ]]; then
    sleep "$sleep_seconds"
fi

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

reset_state() {
    : > "$INVOCATIONS"
    : > "$SCAN_INVOCATIONS"
    : > "$ARG_LOG"
    : > "$STDIN_LOG"
    rm -f "$COUNT_FILE"
}

run_loop() {
    reset_state
    timeout 60 env \
        EIGHTBITDO_BIN="$STUB" \
        SESSION_CMD_FILE="$TMP_DIR/sess.cmd" \
        SESSION_OUT="$TMP_DIR/sess.out" \
        SESSION_ERR="$TMP_DIR/sess.err" \
        SESSION_LOOP_BACKOFF_INITIAL=2 \
        SESSION_LOOP_BACKOFF_MAX=15 \
        SESSION_LOOP_STABLE_SECONDS=60 \
        SESSION_LOOP_SCAN_TIMEOUT=1 \
        LOOP_TEST_COUNT_FILE="$COUNT_FILE" \
        LOOP_TEST_INVOCATIONS="$INVOCATIONS" \
        LOOP_TEST_SCAN_INVOCATIONS="$SCAN_INVOCATIONS" \
        LOOP_TEST_ARGS="$ARG_LOG" \
        LOOP_TEST_STDIN_LOG="$STDIN_LOG" \
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

scan_invocations() {
    if [[ -f "$SCAN_INVOCATIONS" ]]; then
        wc -l < "$SCAN_INVOCATIONS" | tr -d ' '
    else
        echo 0
    fi
}

gap_ms() {
    local first second
    first=$(sed -n "${1}p" "$INVOCATIONS")
    second=$(sed -n "${2}p" "$INVOCATIONS")
    echo $(( (second - first) / 1000000 ))
}

# 1. Degraded exit: stop at once, no scan, no restart, no 2 second wait.
start=$(date +%s)
set +e
run_loop LOOP_TEST_EXIT_1=3
code=$?
set -e
elapsed=$(( $(date +%s) - start ))
[[ "$code" -eq 3 ]] || fail "degraded exit must propagate 3, got $code"
[[ "$(invocations)" -eq 1 ]] || fail "degraded exit must not retry, saw $(invocations) invocations"
[[ "$(scan_invocations)" -eq 0 ]] || fail "degraded exit must not scan"
((elapsed < 2)) || fail "degraded exit must not sleep before stopping (took ${elapsed}s)"
grep -q "WAITING FOR PHYSICAL ACTION" "$TMP_DIR/loop.out" \
    || fail "degraded log must tell the operator to press Profile"
grep -q "NOT retried automatically" "$TMP_DIR/loop.out" \
    || fail "degraded log must say the loop does not retry"

# 2. Normal exit + 80ND visible: exactly one 2 second+ restart gap.
set +e
run_loop LOOP_TEST_EXIT_1=0 LOOP_TEST_EXIT_2=3
code=$?
set -e
[[ "$code" -eq 3 ]] || fail "second (degraded) exit must propagate 3, got $code"
[[ "$(invocations)" -eq 2 ]] || fail "expected exactly two invocations, saw $(invocations)"
[[ "$(scan_invocations)" -eq 1 ]] || fail "expected one post-exit scan, saw $(scan_invocations)"
gap=$(gap_ms 1 2)
((gap >= 1900)) || fail "restart gap must be at least 2s, was ${gap}ms"
grep -q "80ND' is still advertising" "$TMP_DIR/loop.out" \
    || fail "a visible 80ND must be logged before restarting"

# 3. Normal exit + 80ND absent: press guidance, no restart.
set +e
run_loop LOOP_TEST_EXIT_1=0 LOOP_TEST_SCAN_MODE=absent
code=$?
set -e
[[ "$code" -eq 3 ]] || fail "missing 80ND must exit with the press-required code, got $code"
[[ "$(invocations)" -eq 1 ]] || fail "missing 80ND must not restart, saw $(invocations) invocations"
[[ "$(scan_invocations)" -eq 1 ]] || fail "expected one scan, saw $(scan_invocations)"
grep -q "'80ND' is not advertising" "$TMP_DIR/loop.out" \
    || fail "missing 80ND must be logged"
grep -q "WAITING FOR PHYSICAL ACTION" "$TMP_DIR/loop.out" \
    || fail "missing 80ND must ask for the physical press"

# 4. Normal exit + scan failure (lock contention): stop, no blind restart.
set +e
run_loop LOOP_TEST_EXIT_1=0 LOOP_TEST_SCAN_MODE=error
code=$?
set -e
[[ "$code" -eq 1 ]] || fail "a failed scan must stop with 1, got $code"
[[ "$(invocations)" -eq 1 ]] || fail "a failed scan must not restart, saw $(invocations) invocations"
grep -q "post-exit scan failed" "$TMP_DIR/loop.out" \
    || fail "a failed scan must be explained in the log"

# 5. Any other non-zero exit stops instead of hot-looping.
set +e
run_loop LOOP_TEST_EXIT_1=7
code=$?
set -e
[[ "$code" -eq 7 ]] || fail "ordinary failure exit must propagate 7, got $code"
[[ "$(invocations)" -eq 1 ]] || fail "ordinary failure must not retry, saw $(invocations) invocations"
[[ "$(scan_invocations)" -eq 0 ]] || fail "ordinary failure must not scan"
grep -q "not restarting automatically" "$TMP_DIR/loop.out" \
    || fail "ordinary failure must log that it does not restart"

# 6. Backoff grows after consecutive quick normal exits (2s then 4s).
set +e
run_loop LOOP_TEST_EXIT_1=0 LOOP_TEST_EXIT_2=0 LOOP_TEST_EXIT_N=3
code=$?
set -e
[[ "$code" -eq 3 ]] || fail "backoff run must end with 3, got $code"
[[ "$(invocations)" -eq 3 ]] || fail "expected three invocations, saw $(invocations)"
first_gap=$(gap_ms 1 2)
second_gap=$(gap_ms 2 3)
((first_gap >= 1900)) || fail "first backoff must be >= 2s, was ${first_gap}ms"
((second_gap >= 3900)) || fail "second backoff must grow to >= 4s, was ${second_gap}ms"

# 7. A long-enough session resets the backoff to the initial 2s.
set +e
run_loop \
    SESSION_LOOP_STABLE_SECONDS=1 \
    LOOP_TEST_EXIT_1=0 LOOP_TEST_EXIT_2=0 LOOP_TEST_EXIT_N=3 \
    LOOP_TEST_SLEEP_1=1.2 LOOP_TEST_SLEEP_2=1.2
code=$?
set -e
[[ "$code" -eq 3 ]] || fail "reset run must end with 3, got $code"
first_gap=$(gap_ms 1 2)
second_gap=$(gap_ms 2 3)
((second_gap < 3900)) || fail "a stable session must reset the backoff, gap was ${second_gap}ms"
((first_gap >= 1900)) || fail "first backoff must stay >= 2s, was ${first_gap}ms"

# 8. Commands appended during backoff reach the next session.
reset_state
set +e
timeout 60 env \
    EIGHTBITDO_BIN="$STUB" \
    SESSION_CMD_FILE="$TMP_DIR/sess.cmd" \
    SESSION_OUT="$TMP_DIR/sess.out" \
    SESSION_ERR="$TMP_DIR/sess.err" \
    SESSION_LOOP_BACKOFF_INITIAL=2 \
    SESSION_LOOP_BACKOFF_MAX=2 \
    SESSION_LOOP_STABLE_SECONDS=60 \
    SESSION_LOOP_SCAN_TIMEOUT=1 \
    LOOP_TEST_COUNT_FILE="$COUNT_FILE" \
    LOOP_TEST_INVOCATIONS="$INVOCATIONS" \
    LOOP_TEST_SCAN_INVOCATIONS="$SCAN_INVOCATIONS" \
    LOOP_TEST_ARGS="$ARG_LOG" \
    LOOP_TEST_STDIN_LOG="$STDIN_LOG" \
    LOOP_TEST_READ_STDIN_2=1 \
    LOOP_TEST_EXIT_1=0 LOOP_TEST_EXIT_2=0 LOOP_TEST_EXIT_N=3 \
    bash "$LOOP" >"$TMP_DIR/loop.out" 2>&1 &
loop_pid=$!
for _ in $(seq 1 500); do
    [[ -f "$COUNT_FILE" ]] && [[ "$(cat "$COUNT_FILE")" -eq 1 ]] && break
    sleep 0.01
done
# The first session has exited; the loop is now in its 2 second backoff.
printf 'status\n' >> "$TMP_DIR/sess.cmd"
for _ in $(seq 1 1000); do
    [[ -f "$COUNT_FILE" ]] && [[ "$(cat "$COUNT_FILE")" -ge 2 ]] && break
    sleep 0.01
done
wait "$loop_pid"
code=$?
set -e
[[ "$code" -eq 3 ]] || fail "buffered-command run must end with 3, got $code"
grep -qx 'status' "$STDIN_LOG" \
    || fail "a command appended during backoff was lost: $(cat "$STDIN_LOG" 2>/dev/null)"

# 9. SIGTERM stops the supervisor, kills the child and exits 143.
reset_state
set +e
timeout 60 env \
    EIGHTBITDO_BIN="$STUB" \
    SESSION_CMD_FILE="$TMP_DIR/sess.cmd" \
    SESSION_OUT="$TMP_DIR/sess.out" \
    SESSION_ERR="$TMP_DIR/sess.err" \
    SESSION_LOOP_BACKOFF_INITIAL=2 \
    SESSION_LOOP_BACKOFF_MAX=2 \
    SESSION_LOOP_STABLE_SECONDS=60 \
    SESSION_LOOP_SCAN_TIMEOUT=1 \
    LOOP_TEST_COUNT_FILE="$COUNT_FILE" \
    LOOP_TEST_INVOCATIONS="$INVOCATIONS" \
    LOOP_TEST_SCAN_INVOCATIONS="$SCAN_INVOCATIONS" \
    LOOP_TEST_ARGS="$ARG_LOG" \
    LOOP_TEST_STDIN_LOG="$STDIN_LOG" \
    LOOP_TEST_EXIT_1=0 LOOP_TEST_SLEEP_1=30 \
    bash "$LOOP" >"$TMP_DIR/loop.out" 2>&1 &
loop_pid=$!
for _ in $(seq 1 500); do
    [[ -s "$INVOCATIONS" ]] && break
    sleep 0.01
done
kill -TERM "$loop_pid"
wait "$loop_pid"
code=$?
set -e
[[ "$code" -eq 143 ]] || fail "SIGTERM must exit 143, got $code"
grep -q "stopping on signal" "$TMP_DIR/loop.out" \
    || fail "SIGTERM must be logged"

echo "session-loop tests passed"
