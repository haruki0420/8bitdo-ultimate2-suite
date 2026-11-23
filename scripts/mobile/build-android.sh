#!/usr/bin/env bash
# Build the Android debug APK.
#
# Requires an Android SDK + NDK and cargo-ndk for the full build (Rust JNI
# library included). `--kotlin-only` skips the Rust/NDK steps and verifies the
# Kotlin/Compose project + JVM unit tests instead; the resulting APK does not
# contain libeightbitdo_uniffi.so.
#
# Prerequisites:
#   * Android SDK + NDK, with ANDROID_HOME (or ANDROID_SDK_ROOT) set
#   * rustup targets aarch64-linux-android, x86_64-linux-android
#   * cargo-ndk (`cargo install cargo-ndk --locked`)
#   * JDK 17+ (verified with JDK 21)
# The Gradle wrapper is committed (Gradle 9.8.2); no global Gradle is needed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ANDROID_DIR="$ROOT/mobile/android"
KOTLIN_ONLY=0
if [ "${1:-}" = "--kotlin-only" ]; then
  KOTLIN_ONLY=1
fi

command -v cargo >/dev/null 2>&1 || {
  echo "cargo not found" >&2
  exit 1
}
if [ -z "${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}" ]; then
  echo "ANDROID_HOME is not set; see mobile/README.md § Android" >&2
  exit 1
fi
if [ ! -x "$ANDROID_DIR/gradlew" ]; then
  echo "missing mobile/android/gradlew (the Gradle wrapper should be committed)" >&2
  exit 1
fi

echo "==> UniFFI bindings"
"$ROOT/scripts/mobile/gen-bindings.sh"

if [ "$KOTLIN_ONLY" -eq 0 ]; then
  command -v cargo-ndk >/dev/null 2>&1 || {
    echo "cargo-ndk not found; install it (or add \$HOME/.cargo/bin to PATH) or run with --kotlin-only" >&2
    exit 1
  }
  if [ -z "${ANDROID_NDK_HOME:-}" ] && [ ! -d "${ANDROID_HOME}/ndk" ]; then
    echo "Android NDK not found under \$ANDROID_HOME/ndk; install it or run with --kotlin-only" >&2
    exit 1
  fi
  for target in aarch64-linux-android x86_64-linux-android; do
    if ! rustup target list --installed | grep -qx "$target"; then
      echo "missing Rust target $target; run: rustup target add $target" >&2
      exit 1
    fi
  done

  echo "==> Rust shared libraries (cargo-ndk)"
  mkdir -p "$ANDROID_DIR/app/src/main/jniLibs"
  cd "$ROOT"
  cargo ndk \
    -t arm64-v8a \
    -t x86_64 \
    -o "$ANDROID_DIR/app/src/main/jniLibs" \
    build -p eightbitdo-uniffi --release
fi

echo "==> Gradle unit tests + assembleDebug"
cd "$ANDROID_DIR"
./gradlew --no-daemon testDebugUnitTest assembleDebug

APK="$ANDROID_DIR/app/build/outputs/apk/debug/app-debug.apk"
if [ -f "$APK" ]; then
  echo
  echo "debug APK: $APK"
  if [ "$KOTLIN_ONLY" -eq 1 ]; then
    echo "  (--kotlin-only: the Rust JNI library is NOT packaged; use the full"
    echo "   build with an NDK to run the Rust core on a device)"
  fi
else
  echo "Gradle reported success but $APK is missing" >&2
  exit 1
fi
