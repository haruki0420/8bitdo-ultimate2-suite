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
#   scripts/ota/capture-btsnoop.sh [-o DIR] [-i HCI]
#
#   -o, --out-dir DIR   capture directory (default: <repo>/.tmp/ota-capture)
#   -i, --index HCI     only monitor this adapter (passed to btmon -i;
#                       required when the host has multiple adapters)
#   -h, --help          show this help
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

usage() {
    cat <<'EOF'
Usage: scripts/ota/capture-btsnoop.sh [-o DIR] [-i HCI]

Start a btmon btsnoop capture, wait for the operator, then stop btmon.

Options:
  -o, --out-dir DIR   capture directory (default: <repo>/.tmp/ota-capture)
  -i, --index HCI     only monitor this adapter (btmon -i, e.g. hci0).
                      Required when the host has more than one adapter.
  -h, --help          show this help

Environment:
  CAPTURE_DIR     default output directory (overridden by -o)
  BTMON_BIN       btmon executable (default: btmon)
  STOP_TIMEOUT    seconds to wait for btmon to exit after SIGINT (default 10)

The capture file is btsnoop-YYYYmmddTHHMMSSZ.snoop (UTC), mode 0600.
See docs/OTA_VERIFICATION_PLAN.md for the procedure and safety rules.
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

command -v "$BTMON_BIN" >/dev/null 2>&1 \
    || die "btmon not found: $BTMON_BIN (install BlueZ, e.g. bluez / bluez-utils)"

# btmon subscribes to every adapter, so a multi-adapter host would mix other
# devices into the capture. Require an explicit -i hciX in that case.
adapters=(/sys/class/bluetooth/hci*)
if (( ${#adapters[@]} > 1 )); then
    adapter_names=''
    for adapter in "${adapters[@]}"; do
        adapter_names+=" ${adapter##*/}"
    done
    log "detected Bluetooth adapters:${adapter_names}"
    if [[ -z "$BTMON_INDEX" ]]; then
        die "multiple Bluetooth adapters detected; pass -i hciX (one of:${adapter_names})" \
            "so the capture contains only the controller's adapter"
    fi
fi
if [[ -n "$BTMON_INDEX" && ! -e "/sys/class/bluetooth/$BTMON_INDEX" ]]; then
    log "warning: /sys/class/bluetooth/$BTMON_INDEX does not exist;" \
        "check the adapter name passed to -i"
fi

if command -v pgrep >/dev/null 2>&1; then
    other_btmon=$(pgrep -x btmon 2>/dev/null || true)
    if [[ -n "$other_btmon" ]]; then
        log "warning: another btmon is already running (pid: ${other_btmon//$'\n'/ });" \
            "stop it first or the capture may be split between processes"
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
    kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

finish() {
    local status=$?
    local size

    trap - EXIT INT TERM HUP
    if [[ -n "$btmon_pid" ]]; then
        stop_btmon "$btmon_pid"
        btmon_pid=''
    fi

    if [[ -f "$out" ]]; then
        size=$(wc -c < "$out" | tr -d ' ')
        if [[ "$size" -gt 0 ]]; then
            log "capture saved: $out (${size} bytes)"
            if command -v sha256sum >/dev/null 2>&1; then
                log "sha256: $(sha256sum "$out" | awk '{print $1}')"
            fi
            log "read it back with: btmon -r '$out' -T -P"
            log "record size/SHA-256 and redacted excerpts in docs/OTA_BLE_EVIDENCE.md;" \
                "do not commit the capture"
        else
            log "warning: capture file is empty (no HCI traffic was recorded): $out"
        fi
    else
        log "warning: no capture file was created: $out"
    fi

    exit "$status"
}

trap finish EXIT
trap 'log "SIGINT received; stopping capture"; exit 130' INT
trap 'log "SIGTERM received; stopping capture"; exit 143' TERM
trap 'log "SIGHUP received; stopping capture"; exit 129' HUP

log "starting btmon (output: $out)"
if [[ -n "$BTMON_INDEX" ]]; then
    "$BTMON_BIN" -w "$out" -i "$BTMON_INDEX" &
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

cat <<'EOF'
--- BLE OTA capture is running ---------------------------------------------
Do this now (docs/OTA_VERIFICATION_PLAN.md §4):

  1. Connect the controller in App Mode ("80ND") if it is not connected.
  2. Send EnterBoot (0x000C) and wait for the "8bt" reconnect.
  3. Phase A: readPID (0x0008) -> readVersionInBoot (0x0005).
     Phase B: Erase / Write / WriteVersion / WriteFirmwareVersion / Jump.
  4. After Jump, wait for the "80ND" reconnect and read the version.

Press Enter after the App Mode reconnect is confirmed to stop the capture.
Ctrl-C also stops; this script always shuts btmon down.
---------------------------------------------------------------------------
EOF

if [[ -t 0 ]]; then
    printf 'Press Enter to stop the capture: '
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
