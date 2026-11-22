#!/usr/bin/env bash
# Run the pure-logic Swift checks for WriteAckTracker on the host.
#
# There is no XCTest harness for the iOS target on Linux (CoreBluetooth is not
# available), so the write-ack state machine is kept free of CoreBluetooth and
# exercised here with a plain Swift executable.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TRACKER="$ROOT/mobile/ios/Sources/EightBitDo/BLE/WriteAckTracker.swift"
TESTS="$ROOT/mobile/ios/tests/WriteAckTrackerTests.swift"

command -v swiftc >/dev/null 2>&1 || {
  echo "swiftc not found; install Swift (xtool's Swift toolchain works)" >&2
  exit 1
}

OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

swiftc -O "$TRACKER" "$TESTS" -o "$OUT/write-ack-tracker-tests"
"$OUT/write-ack-tracker-tests"
