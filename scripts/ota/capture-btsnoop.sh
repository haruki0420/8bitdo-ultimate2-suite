#!/usr/bin/env bash
# BLE OTA btsnoop capture helper (Linux BlueZ `btmon`).
#
# Starts `btmon -w <file>` for a real-hardware BLE OTA capture, waits for the
# operator to finish the OTA, then stops btmon with SIGINT -> SIGTERM ->
# SIGKILL so the btsnoop file is always closed. The capture procedure, the
# checklist and the safety rules are in docs/OTA_VERIFICATION_PLAN.md. This
# script only wraps btmon; it never talks to the controller itself.
#
# Usage:
#   scripts/ota/capture-btsnoop.sh [-o DIR] [-i HCI] [-p a|b] [--allow-concurrent]
#
#   -o, --out-dir DIR   capture directory (default: <repo>/.tmp/ota-capture)
#   -i, --index HCI     only monitor this adapter (0 or hci0; passed to btmon -i).
#                       Required when the host has more than one adapter.
#   -p, --phase a|b     print the stop instructions for Phase A (read-only) or
#                       Phase B (flash). Without it both are printed.
#       --allow-concurrent  proceed even if another btmon is already running
#                       (captures may be split between the two processes)
#   -h, --help          show this help
#
# Phase B needs two runs of this script, one per session: session 1 stops
# right after the first Erase ACK, session 2 starts before the "8bt"
# connection and stops after the "80ND" reconnect. Use separate output paths.
#
# Exit codes:
#   0   capture finished and looks valid (btsnoop magic present)
#   2   usage error / btmon could not start / another btmon without
#       --allow-concurrent
#   3   capture finished but is INVALID (empty file, bad btsnoop magic, or
#       btmon had to be killed with SIGKILL; completeness is not guaranteed)
#   130 / 143 / 129  interrupted by SIGINT / SIGTERM / SIGHUP
#
# Environment:
#   CAPTURE_DIR     default output directory (overridden by -o)
#   BTMON_BIN       btmon executable (default: btmon)
#   STOP_TIMEOUT    seconds to wait for btmon to exit after SIGINT (default 10)
#
# The capture is written as btsnoop-YYYYmmddTHHMMSSZ.snoop (UTC) with mode 0600
# because a btsnoop file contains BD_ADDR values. Do not commit a capture;
# record only its size/SHA-256 and redacted excerpts in docs/OTA_BLE_EVIDENCE.md.
#
# Requirements: bash >= 4, BlueZ btmon, and root or CAP_NET_ADMIN.

set -euo pipefail
shopt -s nullglob
umask 077

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
CAPTURE_DIR=${CAPTURE_DIR:-"$ROOT_DIR/.tmp/ota-capture"}
BTMON_BIN=${BTMON_BIN:-btmon}
STOP_TIMEOUT=${STOP_TIMEOUT:-10}
BTMON_INDEX=''
PHASE=''
ALLOW_CONCURRENT=0
KILLED_HARD=0

usage() {
    cat <<'EOF'
Usage: scripts/ota/capture-btsnoop.sh [-o DIR] [-i HCI] [-p a|b] [--allow-concurrent]

Start a btmon btsnoop capture, wait for the operator, then stop btmon.

Options:
  -o, --out-dir DIR   capture directory (default: <repo>/.tmp/ota-capture)
  -i, --index HCI     only monitor this adapter (0 or hci0; btmon -i).
                      Required when the host has more than one adapter.
  -p, --phase a|b     stop instructions for Phase A (read-only) or Phase B
                      (flash). Without it both variants are printed.
      --allow-concurrent  proceed even if another btmon is already running
                      (captures may be split between the two processes)
  -h, --help          show this help

Exit codes:
  0   capture finished and looks valid (btsnoop magic present)
  2   usage error / btmon could not start / another btmon without
      --allow-concurrent
  3   capture finished but is INVALID (empty file, bad btsnoop magic, or
      btmon had to be killed with SIGKILL; completeness is not guaranteed)
  130 / 143 / 129  interrupted by SIGINT / SIGTERM / SIGHUP

Environment:
  CAPTURE_DIR     default output directory (overridden by -o)
  BTMON_BIN       btmon executable (default: btmon)
  STOP_TIMEOUT    seconds to wait for btmon to exit after SIGINT (default 10)

The capture file is btsnoop-YYYYmmddTHHMMSSZ.snoop (UTC), mode 0600.
See docs/OTA_VERIFICATION_PLAN.md for the procedure and safety rules.
Phase B needs two runs of this script (one per session, separate output).
EOF
}

die() {
    printf 'capture-btsnoop: %s\n' "$*" >&2
    exit 2
}

log() {
    printf '%s capture-btsnoop: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"
}

while (( $# > 0 )); do
    case "$1" in
        -o|--out-dir)
            if (( $# < 2 )); then die "$1 requires a directory"; fi
            CAPTURE_DIR=$2
            shift 2
            ;;
        -i|--index)
            if (( $# < 2 )); then die "$1 requires an adapter name (e.g. hci0)"; fi
            BTMON_INDEX=$2
            shift 2
            ;;
        -p|--phase)
            if (( $# < 2 )); then die "$1 requires 'a' or 'b'"; fi
            PHASE=$2
            shift 2
            ;;
        --allow-concurrent)
            ALLOW_CONCURRENT=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            die "unknown option: $1 (try --help)"
            ;;
        *)
            die "unexpected argument: $1 (try --help)"
            ;;
    esac
done

if ! [[ "$STOP_TIMEOUT" =~ ^[0-9]+$ ]] || (( STOP_TIMEOUT < 1 || STOP_TIMEOUT > 60 )); then
    die "STOP_TIMEOUT must be an integer between 1 and 60 (got '$STOP_TIMEOUT')"
fi
if [[ -n "$PHASE" && "$PHASE" != "a" && "$PHASE" != "b" ]]; then
    die "--phase must be 'a' or 'b' (got '$PHASE')"
fi

command -v "$BTMON_BIN" >/dev/null 2>&1 \
    || die "btmon not found: $BTMON_BIN (install BlueZ, e.g. bluez / bluez-utils)"

# btmon takes a numeric index (-i 0) but hci0 is easier to type; accept both.
BTMON_INDEX_NUM=''
BTMON_INDEX_NAME=''
if [[ -n "$BTMON_INDEX" ]]; then
    if [[ "$BTMON_INDEX" =~ ^[0-9]+$ ]]; then
        BTMON_INDEX_NUM=$BTMON_INDEX
    elif [[ "$BTMON_INDEX" =~ ^hci([0-9]+)$ ]]; then
        BTMON_INDEX_NUM=${BASH_REMATCH[1]}
    else
        die "invalid adapter '$BTMON_INDEX'; pass 0 or hci0"
    fi
    BTMON_INDEX_NAME="hci$BTMON_INDEX_NUM"
fi

# btmon subscribes to every adapter, so a multi-adapter host would mix other
# devices into the capture. Require an explicit -i in that case.
adapters=(/sys/class/bluetooth/hci*)
if (( ${#adapters[@]} == 0 )); then
    log "warning: no adapter detected under /sys/class/bluetooth;" \
        "if this host has multiple adapters, pass -i hciX explicitly"
else
    adapter_names=''
    for adapter in "${adapters[@]}"; do
        adapter_names+=" ${adapter##*/}"
    done
    if (( ${#adapters[@]} > 1 )); then
        log "detected Bluetooth adapters:${adapter_names}"
        if [[ -z "$BTMON_INDEX_NUM" ]]; then
            die "multiple Bluetooth adapters detected; pass -i hciX (one of:${adapter_names})" \
                "so the capture contains only the controller's adapter"
        fi
    fi
    if [[ -n "$BTMON_INDEX_NAME" ]]; then
        found=0
        for adapter in "${adapters[@]}"; do
            if [[ "${adapter##*/}" == "$BTMON_INDEX_NAME" ]]; then
                found=1
            fi
        done
        if (( found == 0 )); then
            log "warning: $BTMON_INDEX_NAME not found under /sys/class/bluetooth;" \
                "check the adapter passed to -i"
        fi
    fi
fi

if command -v pgrep >/dev/null 2>&1; then
    other_btmon=$(pgrep -x btmon 2>/dev/null || true)
    if [[ -n "$other_btmon" ]]; then
        if (( ALLOW_CONCURRENT == 0 )); then
            die "another btmon is already running (pid: ${other_btmon//$'\n'/ });" \
                "stop it first, or pass --allow-concurrent to proceed anyway"
        fi
        log "warning: another btmon is running (pid: ${other_btmon//$'\n'/ });" \
            "--allow-concurrent given, captures may be split between processes"
    fi
fi

if [[ -L "$CAPTURE_DIR" ]]; then
    die "capture directory must not be a symlink: $CAPTURE_DIR"
fi
if [[ -e "$CAPTURE_DIR" && ! -d "$CAPTURE_DIR" ]]; then
    die "capture path exists but is not a directory: $CAPTURE_DIR"
fi
if [[ -d "$CAPTURE_DIR" ]]; then
    dir_mode=$(stat -c '%a' -- "$CAPTURE_DIR" 2>/dev/null || true)
    if [[ -n "$dir_mode" && "$dir_mode" != "700" ]]; then
        log "tightening capture directory mode from $dir_mode to 700: $CAPTURE_DIR"
        chmod 700 -- "$CAPTURE_DIR" 2>/dev/null \
            || die "cannot set mode 700 on the capture directory: $CAPTURE_DIR"
    fi
fi
if ! mkdir -p -- "$CAPTURE_DIR"; then
    die "cannot create capture directory: $CAPTURE_DIR"
fi
if [[ ! -w "$CAPTURE_DIR" ]]; then
    die "capture directory is not writable: $CAPTURE_DIR"
fi

stamp=$(date -u +%Y%m%dT%H%M%SZ)
out="$CAPTURE_DIR/btsnoop-$stamp.snoop"
if [[ -e "$out" ]]; then
    die "refusing to overwrite an existing capture: $out"
fi

if (( EUID != 0 )); then
    log "warning: btmon usually needs root or CAP_NET_ADMIN;" \
        "re-run with sudo if btmon cannot start"
fi

btmon_pid=''

# Stop btmon and make sure it is gone. SIGINT lets btmon flush and close the
# btsnoop file; SIGTERM/SIGKILL are only fallbacks so the operator never
# leaves a monitor process behind.
stop_btmon() {
    local pid=$1
    local i

    if ! kill -0 "$pid" 2>/dev/null; then
        wait "$pid" 2>/dev/null || true
        return 0
    fi

    log "stopping btmon (pid $pid) with SIGINT"
    kill -INT "$pid" 2>/dev/null || true
    for i in $(seq 1 $((STOP_TIMEOUT * 10))); do
        if ! kill -0 "$pid" 2>/dev/null; then
            wait "$pid" 2>/dev/null || true
            return 0
        fi
        sleep 0.1
    done

    log "btmon did not exit within ${STOP_TIMEOUT}s; sending SIGTERM"
    kill -TERM "$pid" 2>/dev/null || true
    for _ in $(seq 1 30); do
        if ! kill -0 "$pid" 2>/dev/null; then
            wait "$pid" 2>/dev/null || true
            return 0
        fi
        sleep 0.1
    done

    log "btmon still running; sending SIGKILL"
    KILLED_HARD=1
    kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

finish() {
    local status=$?
    local size
    local magic=''
    local invalid=0

    trap - EXIT INT TERM HUP
    if [[ -n "$btmon_pid" ]]; then
        stop_btmon "$btmon_pid"
        btmon_pid=''
    fi

    if [[ -f "$out" ]]; then
        size=$(wc -c < "$out" | tr -d ' ')
        if [[ "$size" -gt 0 ]]; then
            log "capture saved: $out (${size} bytes)"
            if command -v od >/dev/null 2>&1; then
                magic=$(od -An -tx1 -N8 -- "$out" 2>/dev/null | tr -d ' \n')
                if [[ -z "$magic" || "$magic" != "6274736e6f6f7000" ]]; then
                    invalid=1
                    log "INVALID: capture does not start with the btsnoop magic: $out"
                fi
            else
                log "warning: od not found; btsnoop magic was not verified"
            fi
            if command -v sha256sum >/dev/null 2>&1; then
                log "sha256: $(sha256sum "$out" | awk '{print $1}')"
            fi
            log "read it back with: btmon -r '$out' -T -P"
            log "verify completeness (normal exit, final frame, expected frame counts) per" \
                "docs/OTA_VERIFICATION_PLAN.md §6.4"
            log "record size/SHA-256 and redacted excerpts in docs/OTA_BLE_EVIDENCE.md;" \
                "do not commit the capture"
        else
            invalid=1
            log "INVALID: capture file is empty (no HCI traffic was recorded): $out"
        fi
    else
        invalid=1
        log "INVALID: no capture file was created: $out"
    fi

    if (( KILLED_HARD == 1 )); then
        invalid=1
        log "INVALID: btmon had to be killed with SIGKILL;" \
            "capture completeness is not guaranteed (the last frames may be missing)"
    fi

    if (( invalid == 1 )); then
        log "capture is INVALID; do not use it as evidence instead of a clean run" \
            "(docs/OTA_VERIFICATION_PLAN.md §6.4)"
        if (( status == 0 )); then
            status=3
        fi
    fi

    exit "$status"
}

trap finish EXIT
trap 'log "SIGINT received; stopping capture"; exit 130' INT
trap 'log "SIGTERM received; stopping capture"; exit 143' TERM
trap 'log "SIGHUP received; stopping capture"; exit 129' HUP

log "starting btmon (output: $out)"
if [[ -n "$BTMON_INDEX_NUM" ]]; then
    "$BTMON_BIN" -w "$out" -i "$BTMON_INDEX_NUM" &
else
    "$BTMON_BIN" -w "$out" &
fi
btmon_pid=$!

# Give btmon a moment to open the monitor channel. An immediate exit is almost
# always a privilege problem or a monitor channel already owned by another
# process, so fail with a hint instead of waiting for a capture that is dead.
sleep 0.5
if ! kill -0 "$btmon_pid" 2>/dev/null; then
    wait "$btmon_pid" 2>/dev/null || true
    btmon_pid=''
    die "btmon exited immediately; check root/CAP_NET_ADMIN and that no other" \
        "btmon owns the monitor channel"
fi

log "capture started at $(date -u +%Y-%m-%dT%H:%M:%SZ) (UTC)"

print_instructions() {
    case "$PHASE" in
        a)
            cat <<'EOF'
--- BLE OTA capture (Phase A: read-only) -----------------------------------
Do this now (docs/OTA_VERIFICATION_PLAN.md §4):

  1. Connect the controller in App Mode ("80ND").
  2. Send EnterBoot (0x000C) and wait for the "8bt" reconnect.
  3. readPID (0x0008) -> readVersionInBoot (0x0005).
  4. Wait 1-2 seconds after the readVersionInBoot response, then stop.

Phase A does NOT return to App Mode. Do not wait for "80ND": stop here, then
recover with a power cycle and `eightbitdo scan --check` (plan §8.2).
Ctrl-C also stops; this script always shuts btmon down.
---------------------------------------------------------------------------
EOF
            ;;
        b)
            cat <<'EOF'
--- BLE OTA capture (Phase B: flash) ---------------------------------------
Do this now (docs/OTA_VERIFICATION_PLAN.md §4 and §8.3). Phase B has TWO
sessions and requires TWO runs of this script, one per session, each with its
own stop point (use a separate output directory or file per run):

  RUN 1 - Session 1 (AfterFirstErase): connect App Mode ("80ND") ->
  EnterBoot (0x000C) -> "8bt" reconnect -> readPID -> checkpoint. STOP right
  after the FIRST Erase ACK (accepted shapes: cmd=0x0004 envelope with
  subcmd=0x0004, or a direct cmd=0x0004 echo with subcmd=0), then verify
  offset / total_len / ACK shape before deciding whether to continue (plan
  §8.3).

  RUN 2 - Session 2 (full run): scan for "8bt" (do NOT use `scan --check`; it
  is for "80ND"; do NOT send EnterBoot) -> new OtaUpdate -> prepare_boot ->
  write_firmware -> all writes, WriteVersion, WriteFirmwareVersion, Jump.
  STOP after the "80ND" reconnect and version read (plan §8.3). Once started
  this run cannot be aborted; it either completes or fails.

Ctrl-C also stops; this script always shuts btmon down.
---------------------------------------------------------------------------
EOF
            ;;
        *)
            cat <<'EOF'
--- BLE OTA capture is running ---------------------------------------------
Do this now (docs/OTA_VERIFICATION_PLAN.md §4):

  Phase A (read-only): EnterBoot -> readPID -> readVersionInBoot, then wait
  1-2 seconds and stop. Phase A does NOT return to App Mode; do not wait for
  "80ND" (recover with a power cycle, plan §8.2).

  Phase B (flash) requires TWO runs of this script, one per session
  (separate output per run). Session 1 stops right after the FIRST Erase ACK
  (human checkpoint, plan §8.3). Session 2 connects to "8bt" directly (no
  EnterBoot), completes the full run, then stops after the "80ND" reconnect
  and version read. Session 2 cannot be aborted once started.

Pass -p a or -p b for phase-specific instructions.
Ctrl-C also stops; this script always shuts btmon down.
---------------------------------------------------------------------------
EOF
            ;;
    esac
}

print_instructions

if [[ -t 0 ]]; then
    case "$PHASE" in
        a) printf 'Press Enter to stop the capture (1-2 s after readVersionInBoot): ' ;;
        b) printf 'Press Enter to stop (Phase B: run this once per session; session 1 after Erase ACK / session 2 after 80ND reconnect): ' ;;
        *) printf 'Press Enter to stop the capture: ' ;;
    esac
    read -r _ || true
else
    log "stdin is not a terminal; waiting for SIGINT/SIGTERM to stop" \
        "(or for btmon to exit on its own)"
    while kill -0 "$btmon_pid" 2>/dev/null; do
        sleep 1
    done
    log "btmon exited on its own; finishing"
fi

if [[ -n "$btmon_pid" ]]; then
    stop_btmon "$btmon_pid"
    btmon_pid=''
fi
log "capture stopped at $(date -u +%Y-%m-%dT%H:%M:%SZ) (UTC)"
exit 0
