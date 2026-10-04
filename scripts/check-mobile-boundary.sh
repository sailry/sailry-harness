#!/usr/bin/env bash
set -euo pipefail
command -v rg >/dev/null

# Run from the repository root. This checks target compilation, not device behavior.
task_target="${1:?Usage: bash scripts/check-mobile-boundary.sh <aarch64-apple-ios|aarch64-apple-ios-sim|aarch64-linux-android>}"
task_package="${2:-sailry-client}"
case "$task_package" in
  sailry-client|sailry-mobile-bridge) ;;
  *) echo "Unsupported mobile package" >&2; exit 1 ;;
esac
case "$task_target" in
  aarch64-apple-ios|aarch64-apple-ios-sim)
    # Keep Rust and native crypto objects on the same deployment target.
    export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-13.0}"
    ;;
  aarch64-linux-android)
    : "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to an installed Android NDK}"
    case "$(uname -s)" in
      Darwin) task_ndk_host=darwin-x86_64 ;;
      Linux) task_ndk_host=linux-x86_64 ;;
      *) echo "Unsupported NDK build host" >&2; exit 1 ;;
    esac
    task_bin="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$task_ndk_host/bin"
    export CC_aarch64_linux_android="$task_bin/aarch64-linux-android24-clang"
    export CXX_aarch64_linux_android="$task_bin/aarch64-linux-android24-clang++"
    export AR_aarch64_linux_android="$task_bin/llvm-ar"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$task_bin/aarch64-linux-android24-clang"
    test -x "$CC_aarch64_linux_android"
    ;;
  *) echo "Unsupported mobile target" >&2; exit 1 ;;
esac

task_dependencies="$(cargo tree --locked -p "$task_package" --target "$task_target" --edges normal --prefix none --format '{p}')"
if printf '%s\n' "$task_dependencies" | rg '^(sailry-node-runtime|sysinfo|russh|adk[-_][^ ]*|gpui[^ ]*|git2|libgit2-sys|portable-pty|process-wrap|libghostty[^ ]*|ghostty[^ ]*) v'; then
  echo "Execution or desktop dependency leaked into the mobile client" >&2
  exit 1
fi
if test "$task_package" = sailry-mobile-bridge; then
  cargo build --locked -p "$task_package" --lib --target "$task_target"
else
  cargo check --locked -p "$task_package" --target "$task_target"
fi
