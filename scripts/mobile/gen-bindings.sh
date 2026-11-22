#!/usr/bin/env bash
# Generate the UniFFI bindings that the iOS and Android projects compile.
#
# Inputs:
#   crates/eightbitdo-uniffi            (Rust, `uniffi` 0.32.2)
#
# Outputs (all generated, none committed):
#   mobile/ios/RustCore/module.modulemap
#   mobile/ios/RustCore/EightBitdoFFI.h
#   mobile/ios/Sources/EightBitDo/Generated/EightBitdo.swift
#   mobile/android/app/src/main/java/dev/eightbitdo/mobile/core/eightbitdo_uniffi.kt
#
# The host cdylib built below is only a metadata input for the code generator;
# it is never shipped. Platform binaries are built by build-ios.sh /
# build-android.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CRATE="eightbitdo-uniffi"
IOS_DIR="$ROOT/mobile/ios"
ANDROID_DIR="$ROOT/mobile/android"
STAGING_DIR="$ROOT/target/mobile-bindings"

cd "$ROOT"

echo "==> building host $CRATE (UniFFI metadata only)"
cargo build -p "$CRATE"

case "$(uname -s)" in
  Linux) HOST_LIB="target/debug/libeightbitdo_uniffi.so" ;;
  Darwin) HOST_LIB="target/debug/libeightbitdo_uniffi.dylib" ;;
  *)
    echo "unsupported host: $(uname -s); run this on Linux or macOS" >&2
    exit 1
    ;;
esac

if [ ! -f "$HOST_LIB" ]; then
  echo "missing $HOST_LIB; the host cdylib was not produced" >&2
  exit 1
fi

rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR/swift"

echo "==> generating Swift bindings"
cargo run -q -p "$CRATE" --features cli --bin uniffi-bindgen-swift -- \
  "$HOST_LIB" \
  "$STAGING_DIR/swift" \
  --module-name EightBitdoFFI \
  --modulemap-filename module.modulemap \
  --swift-sources --headers --modulemap

mkdir -p "$IOS_DIR/RustCore" "$IOS_DIR/Sources/EightBitDo/Generated"
cp "$STAGING_DIR/swift/EightBitdoFFI.h" "$IOS_DIR/RustCore/EightBitdoFFI.h"
cp "$STAGING_DIR/swift/module.modulemap" "$IOS_DIR/RustCore/module.modulemap"
cp "$STAGING_DIR/swift/EightBitdo.swift" \
  "$IOS_DIR/Sources/EightBitDo/Generated/EightBitdo.swift"

echo "==> generating Kotlin bindings"
cargo run -q -p "$CRATE" --features cli --bin uniffi-bindgen -- \
  generate \
  --library "$HOST_LIB" \
  --language kotlin \
  --no-format \
  --out-dir "$ANDROID_DIR/app/src/main/java"

echo "done:"
echo "  $IOS_DIR/RustCore/"
echo "  $IOS_DIR/Sources/EightBitDo/Generated/EightBitdo.swift"
echo "  $ANDROID_DIR/app/src/main/java/dev/eightbitdo/mobile/core/"
