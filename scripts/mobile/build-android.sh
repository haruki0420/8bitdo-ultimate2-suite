#!/usr/bin/env bash
# Build the Android debug APK.
#
# This host has no Android SDK/NDK (see mobile/README.md); the script fails
# early with the exact setup commands rather than downloading anything.
#
# Prerequisites:
#   * Android SDK + NDK, with ANDROID_HOME (or ANDROID_SDK_ROOT) set
#   * rustup targets aarch64-linux-android, x86_64-linux-android
#   * cargo-ndk (`cargo install cargo-ndk --locked`)
#   * a Gradle wrapper (`gradle wrapper --gradle-version <x>` once in
#     mobile/android; the wrapper jar is generated, not committed)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ANDROID_DIR="$ROOT/mobile/android"

missing=()
command -v cargo >/dev/null 2>&1 || missing+=("cargo")
command -v cargo-ndk >/dev/null 2>&1 || missing+=("cargo-ndk")
command -v rustup >/dev/null 2>&1 || missing+=("rustup")
if [ -z "${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}" ]; then
  missing+=("ANDROID_HOME (Android SDK path)")
fi
if [ "${#missing[@]}" -gt 0 ]; then
  echo "missing prerequisites: ${missing[*]}" >&2
  echo "see mobile/README.md § Android for the setup commands" >&2
  exit 1
fi

if [ ! -x "$ANDROID_DIR/gradlew" ]; then
  echo "missing mobile/android/gradlew" >&2
  echo "generate it once with: (cd mobile/android && gradle wrapper --gradle-version 8.14.3)" >&2
  exit 1
fi

echo "==> UniFFI bindings"
"$ROOT/scripts/mobile/gen-bindings.sh"

echo "==> Rust shared libraries (cargo-ndk)"
for target in aarch64-linux-android x86_64-linux-android; do
  if ! rustup target list --installed | grep -qx "$target"; then
    echo "missing Rust target $target; run: rustup target add $target" >&2
    exit 1
  fi
done

mkdir -p "$ANDROID_DIR/app/src/main/jniLibs"
cd "$ROOT"
cargo ndk \
  -t arm64-v8a \
  -t x86_64 \
  -o "$ANDROID_DIR/app/src/main/jniLibs" \
  build -p eightbitdo-uniffi --release

echo "==> Gradle assembleDebug"
cd "$ANDROID_DIR"
./gradlew --no-daemon assembleDebug

APK="$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk"
if [ -f "$APK" ]; then
  echo
  echo "debug APK: $APK"
else
  echo "Gradle reported success but $APK is missing" >&2
  exit 1
fi
