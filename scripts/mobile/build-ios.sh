#!/usr/bin/env bash
# Build the unsigned iOS IPA on Linux with xtool.
#
# Prerequisites (see mobile/README.md):
#   * xtool 1.24.x with a Darwin SDK installed (`xtool sdk status`)
#   * rustup target aarch64-apple-ios (`rustup target add aarch64-apple-ios`)
#
# The result is an *unsigned* IPA: it proves the Swift app and the Rust static
# library compile and link, but it cannot be installed on a device. Signing
# needs `xtool dev build --sign` with a trusted device attached, and App Store
# upload needs macOS.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IOS_DIR="$ROOT/mobile/ios"
TARGET="aarch64-apple-ios"

command -v xtool >/dev/null 2>&1 || {
  echo "xtool not found; see mobile/README.md (Linux setup)" >&2
  exit 1
}
command -v cargo >/dev/null 2>&1 || {
  echo "cargo not found" >&2
  exit 1
}

if ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "missing Rust target $TARGET; run: rustup target add $TARGET" >&2
  exit 1
fi

echo "==> UniFFI bindings"
"$ROOT/scripts/mobile/gen-bindings.sh"

echo "==> Rust static library ($TARGET)"
cd "$ROOT"
# `--crate-type staticlib` stops cargo from also emitting a Mach-O cdylib,
# which the Linux host linker cannot produce.
cargo rustc -p eightbitdo-uniffi --lib --release --target "$TARGET" --crate-type staticlib

mkdir -p "$IOS_DIR/libs"
cp "target/$TARGET/release/libeightbitdo_uniffi.a" "$IOS_DIR/libs/"

echo "==> xtool dev build --ipa"
cd "$IOS_DIR"
xtool dev build -c release --ipa

IPA="$IOS_DIR/xtool/EightBitDo.ipa"
if [ -f "$IPA" ]; then
  echo
  echo "unsigned IPA: $IPA"
  echo "  (cannot be installed on a device; sign with xtool + a trusted device)"
else
  echo "xtool reported success but $IPA is missing" >&2
  exit 1
fi
