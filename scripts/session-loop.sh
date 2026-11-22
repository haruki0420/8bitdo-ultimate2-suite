#!/usr/bin/env bash
# Resident `eightbitdo session` supervisor.
#
# One Profile long-press puts the controller into App Mode (`80ND`). This loop
# keeps a single `eightbitdo session` alive for that whole config-mode window:
#
#   * exit 0 (`quit` / EOF / SIGINT): restart, but never sooner than 2 seconds
#     (exponential up to 15) so the old BLE link and the controller's config
#     mode have time to settle before the next connect;
#   * exit 3 (keepalive degraded): STOP and wait for a physical Profile press.
#     A disconnected controller does not advertise `80ND`, so retrying cannot
#     succeed; the loop prints that and does not restart;
#   * any other exit code: STOP as well. A failed connect usually means the
#     controller is not in App Mode, which also needs the physical press.
#
# The session owns the single-owner BLE lock for its whole lifetime
# (`$XDG_RUNTIME_DIR/8bitdo-ble.lock`); this loop must not start a second BLE
# operation while it runs (the CLI refuses with "another process is using the
# BLE adapter" instead of fighting the link).
#
# Usage:
#   scripts/session-loop.sh [extra `session` arguments...]
#
# Environment:
#   EIGHTBITDO_BIN                 binary path (default target/release/eightbitdo)
#   SESSION_CMD_FILE               command pipe source (default .tmp/sess.cmd)
#   SESSION_OUT / SESSION_ERR      logs (default .tmp/sess.out / .tmp/sess.err)
#   SESSION_LOOP_BACKOFF_INITIAL   first restart delay in seconds (default 2)
#   SESSION_LOOP_BACKOFF_MAX       maximum restart delay (default 15)
#   SESSION_LOOP_STABLE_SECONDS    runtime that resets the backoff (default 60)
#   EXIT_DEGRADED                  exit code that means "Profile press needed" (default 3)

set -uo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
EIGHTBITDO_BIN=${EIGHTBITDO_BIN:-"$ROOT_DIR/target/release/eightbitdo"}
SESSION_CMD_FILE=${SESSION_CMD_FILE:-"$ROOT_DIR/.tmp/sess.cmd"}
SESSION_OUT=${SESSION_OUT:-"$ROOT_DIR/.tmp/sess.out"}
SESSION_ERR=${SESSION_ERR:-"$ROOT_DIR/.tmp/sess.err"}

EXIT_DEGRADED=${EXIT_DEGRADED:-3}
BACKOFF_INITIAL=${SESSION_LOOP_BACKOFF_INITIAL:-2}
BACKOFF_MAX=${SESSION_LOOP_BACKOFF_MAX:-15}
STABLE_SECONDS=${SESSION_LOOP_STABLE_SECONDS:-60}

# The minimum wait after any session exit. Exiting closes the BLE link; a
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

on_signal() {
    local code=$1
    log "stopping on signal; the BLE link will be closed and the controller leaves App Mode (Profile press needed to re-enter)"
    if [[ -n "$SESSION_PID" ]]; then
        kill "$SESSION_PID" 2>/dev/null || true
    fi
    stop_tail
    exit "$code"
}
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

backoff=$BACKOFF_INITIAL
while :; do
    # `tail -f` keeps the line reader alive across restarts. It is killed after
    # every session exit: a bare `tail -f | session` pipeline would leave the
    # shell waiting for tail forever once the session ended.
    coproc SESSION_INPUT { exec tail -n 0 -f -- "$SESSION_CMD_FILE"; }
    TAIL_PID=$SESSION_INPUT_PID
    input_fd=${SESSION_INPUT[0]}

    started=$(date +%s)
    # Duplicate the coproc descriptor to fd 3: bash cannot use a coproc's
    # high-numbered fd as the target of a background redirection.
    exec 3<&$input_fd
    "$EIGHTBITDO_BIN" session "$@" <&3 >>"$SESSION_OUT" 2>>"$SESSION_ERR" &
    SESSION_PID=$!
    wait "$SESSION_PID"
    code=$?
    SESSION_PID=""
    elapsed=$(( $(date +%s) - started ))

    stop_tail
    # Close the duplicated/coprocess descriptors. The brace groups keep the
    # `2>/dev/null` from becoming a permanent shell-wide stderr redirection,
    # which a bare `exec ... 2>/dev/null` would do.
    { exec 3<&-; } 2>/dev/null || true
    if [[ -n "${input_fd:-}" ]]; then
        { exec {input_fd}<&-; } 2>/dev/null || true
    fi

    if ((code == 0)); then
        # A long run is a healthy link: reset the backoff so a later restart is
        # fast instead of staying at the maximum from an old failure burst.
        if ((elapsed >= STABLE_SECONDS)); then
            backoff=$BACKOFF_INITIAL
        fi
        log "session exited normally after ${elapsed}s; restarting in ${backoff}s (minimum ${MIN_RESTART_DELAY}s for BLE teardown)"
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
        exit "$EXIT_DEGRADED"
    fi

    log "session exited with code ${code}; not restarting automatically."
    log "See the session error above: a lock conflict means another BLE process is still running (wait for it, then restart);"
    log "otherwise the controller probably left App Mode, so press Profile for about 3 seconds before starting this loop again."
    exit "$code"
done
