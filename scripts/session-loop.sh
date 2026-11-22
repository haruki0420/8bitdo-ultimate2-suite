#!/usr/bin/env bash
# Resident `eightbitdo session` supervisor.
#
# One Profile long-press puts the controller into App Mode (`80ND`). This loop
# keeps a single `eightbitdo session` alive for that whole config-mode window.
#
# Exit contract (see docs/SESSION_OPERATIONS.md):
#
#   * exit 3 (keepalive degraded): STOP and wait for a physical Profile press.
#     A disconnected controller does not advertise `80ND`, so retrying cannot
#     succeed; the loop prints that and does not restart.
#   * exit 0 (`quit` / EOF / SIGINT inside the REPL): the BLE link is already
#     gone, so `80ND` may be gone too. Wait at least 2 seconds for the old link
#     to finish tearing down, then run `scan --check`; restart only when `80ND`
#     is still advertising (with backoff >= 2 s), otherwise print the press
#     guidance and stop. A failed scan (another BLE process holds the lock, or
#     the adapter is unavailable) also stops instead of restarting blindly.
#   * any other exit code: STOP. A failed connect usually means the controller
#     is not in App Mode, which also needs the physical press.
#
# The session owns the per-user single-owner BLE lock
# (`~/.cache/8bitdo/8bitdo-ble.lock` from the passwd home, or a per-user temp
# directory when no home is available) for its whole lifetime; this loop must
# not start a second BLE operation while it runs.
#
# The control channel (`.tmp/sess.cmd`) is read by one `tail -f` for the whole
# supervisor lifetime, so commands appended during a restart backoff are not
# lost and the next session receives them.
#
# Requirements: bash >= 4 (coproc), GNU coreutils `date +%s%N` for the
# millisecond restart-gap measurement in the test only (the loop itself uses
# integer seconds). macOS / BSD userland is not supported.
#
# Usage:
#   scripts/session-loop.sh [extra `session` arguments...]
#
# Environment:
#   EIGHTBITDO_BIN                 binary path (default target/release/eightbitdo)
#   SESSION_CMD_FILE               command source (default .tmp/sess.cmd)
#   SESSION_OUT / SESSION_ERR      logs (default .tmp/sess.out / .tmp/sess.err)
#   SESSION_LOOP_BACKOFF_INITIAL   first restart delay in seconds (default 2)
#   SESSION_LOOP_BACKOFF_MAX       maximum restart delay (default 15)
#   SESSION_LOOP_STABLE_SECONDS    runtime that resets the backoff (default 60)
#   SESSION_LOOP_SCAN_TIMEOUT      seconds for the post-exit `80ND` scan (default 5)
#   EXIT_DEGRADED                  exit code that means "degraded" (default 3)
#   EXIT_PRESS_REQUIRED            loop exit when a press is needed (default 3)

set -uo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
EIGHTBITDO_BIN=${EIGHTBITDO_BIN:-"$ROOT_DIR/target/release/eightbitdo"}
SESSION_CMD_FILE=${SESSION_CMD_FILE:-"$ROOT_DIR/.tmp/sess.cmd"}
SESSION_OUT=${SESSION_OUT:-"$ROOT_DIR/.tmp/sess.out"}
SESSION_ERR=${SESSION_ERR:-"$ROOT_DIR/.tmp/sess.err"}

EXIT_DEGRADED=${EXIT_DEGRADED:-3}
EXIT_PRESS_REQUIRED=${EXIT_PRESS_REQUIRED:-3}
BACKOFF_INITIAL=${SESSION_LOOP_BACKOFF_INITIAL:-2}
BACKOFF_MAX=${SESSION_LOOP_BACKOFF_MAX:-15}
STABLE_SECONDS=${SESSION_LOOP_STABLE_SECONDS:-60}
SCAN_TIMEOUT=${SESSION_LOOP_SCAN_TIMEOUT:-5}

# The minimum wait after a normal session exit. Exiting closes the BLE link; a
# reconnect before the controller finishes tearing it down is what produced the
# `BLE error: In Progress` incident.
MIN_RESTART_DELAY=2

log() {
    printf '%s session-loop: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"
}

if ! [[ "$BACKOFF_INITIAL" =~ ^[0-9]+$ ]] || ((BACKOFF_INITIAL < 1)); then
    log "SESSION_LOOP_BACKOFF_INITIAL must be a positive integer (got '${BACKOFF_INITIAL}')"
    exit 2
fi
if ! [[ "$BACKOFF_MAX" =~ ^[0-9]+$ ]] || ((BACKOFF_MAX < MIN_RESTART_DELAY)); then
    log "SESSION_LOOP_BACKOFF_MAX must be an integer >= ${MIN_RESTART_DELAY} (got '${BACKOFF_MAX}')"
    exit 2
fi
if ! [[ "$SCAN_TIMEOUT" =~ ^[0-9]+$ ]] || ((SCAN_TIMEOUT < 1)); then
    log "SESSION_LOOP_SCAN_TIMEOUT must be a positive integer (got '${SCAN_TIMEOUT}')"
    exit 2
fi
if ((BACKOFF_INITIAL < MIN_RESTART_DELAY)); then
    BACKOFF_INITIAL=$MIN_RESTART_DELAY
fi

if [[ ! -x "$EIGHTBITDO_BIN" ]]; then
    log "session binary is missing or not executable: $EIGHTBITDO_BIN"
    exit 1
fi

mkdir -p -- "$(dirname -- "$SESSION_CMD_FILE")" \
    "$(dirname -- "$SESSION_OUT")" \
    "$(dirname -- "$SESSION_ERR")"
touch -- "$SESSION_CMD_FILE" "$SESSION_OUT" "$SESSION_ERR"

TAIL_PID=""
SESSION_PID=""

stop_tail() {
    if [[ -n "$TAIL_PID" ]]; then
        kill "$TAIL_PID" 2>/dev/null || true
        wait "$TAIL_PID" 2>/dev/null || true
        TAIL_PID=""
    fi
}

finish() {
    stop_tail
    exit "$1"
}

on_signal() {
    local code=$1
    log "stopping on signal; the BLE link will be closed and the controller leaves App Mode (Profile press needed to re-enter)"
    if [[ -n "$SESSION_PID" ]]; then
        kill "$SESSION_PID" 2>/dev/null || true
    fi
    finish "$code"
}
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

# Start the line reader once. The shell keeps the read end open, so tail never
# gets SIGPIPE between sessions and commands appended during backoff stay
# queued for the next session.
coproc SESSION_INPUT { exec tail -n 0 -f -- "$SESSION_CMD_FILE"; }
TAIL_PID=$SESSION_INPUT_PID
input_fd=${SESSION_INPUT[0]}

# Return 0 when `scan --check` sees an App Mode device, 1 when it succeeded but
# saw none (exit 3), and 2 when the scan itself failed (usually another process
# holds the BLE lock, or the adapter is unavailable).
scan_sees_app_mode() {
    "$EIGHTBITDO_BIN" scan --check --scan-timeout "$SCAN_TIMEOUT" \
        >>"$SESSION_OUT" 2>>"$SESSION_ERR"
    local status=$?
    case "$status" in
        0) return 0 ;;
        3) return 1 ;;
        *) return 2 ;;
    esac
}

backoff=$BACKOFF_INITIAL
while :; do
    # Duplicate the coproc descriptor to fd 3: bash cannot use a coproc's
    # high-numbered fd as the target of a background redirection.
    exec 3<&$input_fd
    started=$(date +%s)
    "$EIGHTBITDO_BIN" session "$@" <&3 >>"$SESSION_OUT" 2>>"$SESSION_ERR" &
    SESSION_PID=$!
    wait "$SESSION_PID"
    code=$?
    SESSION_PID=""
    elapsed=$(( $(date +%s) - started ))

    # The brace groups keep `2>/dev/null` from becoming a permanent shell-wide
    # stderr redirection (a bare `exec ... 2>/dev/null` would do that).
    { exec 3<&-; } 2>/dev/null || true

    if ((code == 0)); then
        # A normal exit still dropped the BLE link. Give the old link time to
        # finish tearing down before probing: a scan immediately after exit can
        # race the teardown and produce a false "80ND absent" result.
        log "session exited normally after ${elapsed}s; waiting ${MIN_RESTART_DELAY}s for BLE teardown before checking for '80ND'"
        sleep "$MIN_RESTART_DELAY"
        # Verify that the controller is still in App Mode before reconnecting;
        # if it is not, no retry can succeed and only a physical Profile press
        # can restore `80ND`.
        scan_sees_app_mode
        scan_status=$?
        if ((scan_status == 2)); then
            log "the post-exit scan failed (another BLE process may hold the lock, or the adapter is unavailable); not restarting"
            finish 1
        elif ((scan_status != 0)); then
            log "session exited normally after ${elapsed}s, but '80ND' is not advertising: the controller left App Mode when the link closed."
            log "WAITING FOR PHYSICAL ACTION: press Profile on the controller for about 3 seconds, then start this loop again."
            log "It is NOT retried automatically: no reconnect can succeed before that press."
            finish "$EXIT_PRESS_REQUIRED"
        fi

        # `80ND` is still visible: this was a clean session end with the
        # controller still in App Mode, so a restart can reconnect. A long run
        # is a healthy link, so reset the backoff.
        if ((elapsed >= STABLE_SECONDS)); then
            backoff=$BACKOFF_INITIAL
        fi
        log "session exited normally after ${elapsed}s and '80ND' is still advertising; restarting in ${backoff}s (minimum ${MIN_RESTART_DELAY}s for BLE teardown)"
        sleep "$backoff"
        if ((backoff * 2 > BACKOFF_MAX)); then
            backoff=$BACKOFF_MAX
        else
            backoff=$((backoff * 2))
        fi
        continue
    fi

    if ((code == EXIT_DEGRADED)); then
        log "session exited degraded (code ${EXIT_DEGRADED}): the BLE link is gone and '80ND' is not advertising."
        log "WAITING FOR PHYSICAL ACTION: press Profile on the controller for about 3 seconds to re-enter App Mode,"
        log "then start this loop again. It is NOT retried automatically: no reconnect can succeed before that press."
        finish "$EXIT_DEGRADED"
    fi

    log "session exited with code ${code}; not restarting automatically."
    log "See the session error above: a lock conflict means another BLE process is still running (wait for it, then restart);"
    log "otherwise the controller probably left App Mode, so press Profile for about 3 seconds before starting this loop again."
    finish "$code"
done
