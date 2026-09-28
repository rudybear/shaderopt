#!/usr/bin/env bash
# Cross-compiles shaderlab-runner for Android arm64-v8a with the pinned NDK (lab/TOOLS.md).
# Output: lab/runner/build-android/shaderlab-runner (static libc++, runs from /data/local/tmp).
# Usage: build-android.sh [extra cmake args]   env: ANDROID_NDK, IGL_DIR, ANDROID_PLATFORM
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NDK="${ANDROID_NDK:-$HOME/Android/android-ndk-r27c}"
BUILD="$HERE/build-android"
PLATFORM="${ANDROID_PLATFORM:-android-26}"   # IGL's AHardwareBuffer path needs >= 26; Vulkan 1.1 loader >= 29 is not required (volk)
[ -f "$NDK/build/cmake/android.toolchain.cmake" ] || { echo "NDK not found at $NDK (set ANDROID_NDK)"; exit 1; }
cmake -G Ninja -S "$HERE" -B "$BUILD" \
  -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
  -DANDROID_ABI=arm64-v8a \
  -DANDROID_PLATFORM="$PLATFORM" \
  -DANDROID_STL=c++_static \
  -DCMAKE_BUILD_TYPE=Release \
  ${IGL_DIR:+-DIGL_DIR="$IGL_DIR"} \
  "$@"
ninja -C "$BUILD" shaderlab-runner
echo
echo "built: $BUILD/shaderlab-runner"
file "$BUILD/shaderlab-runner" || true
