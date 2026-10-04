#!/usr/bin/env bash
set -euo pipefail

# Build the exact workspace revision shipped with Desktop. No remote installer is executed.
task_profile="${1:-debug}"
case "$task_profile" in debug) task_cargo_profile=dev ;; release) task_cargo_profile=release ;; *) echo 'Usage: scripts/build-hosts.sh [debug|release]' >&2; exit 1 ;; esac
task_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$task_root"
task_native="$(rustc -vV | awk '/^host: / {print $2}')"
task_llvm_ar="$(command -v llvm-ar || true)"
task_llvm_strip="$(command -v llvm-strip || true)"
if { test -z "$task_llvm_ar" || test -z "$task_llvm_strip"; } && command -v brew >/dev/null; then
  for task_llvm in llvm llvm@21 llvm@20; do
    task_prefix="$(brew --prefix "$task_llvm" 2>/dev/null || true)"
    if test -x "$task_prefix/bin/llvm-ar"; then
      task_llvm_ar="$task_prefix/bin/llvm-ar"
      task_llvm_strip="$task_prefix/bin/llvm-strip"
      break
    fi
  done
fi
if test -z "$task_llvm_ar" || test -z "$task_llvm_strip"; then
  echo 'LLVM ar and strip are required to package Linux host binaries' >&2
  exit 1
fi
# Rust 1.98's ARM64 linker arguments require cargo-zigbuild 0.23.4.
task_zigbuild="$task_root/target/tooling/bin/cargo-zigbuild"
if ! test -x "$task_zigbuild"; then task_zigbuild="$(command -v cargo-zigbuild || true)"; fi
if test -z "$task_zigbuild" || [[ "$("$task_zigbuild" --version)" != 'cargo-zigbuild 0.23.4' ]]; then
  echo 'Install the pinned tool: cargo install cargo-zigbuild --version 0.23.4 --locked --root target/tooling' >&2
  exit 1
fi
# Ghostty needs Zig 0.16. LLVM ar avoids its macOS archive creation regression.
export AR_aarch64_unknown_linux_gnu="$task_llvm_ar"
export AR_x86_64_unknown_linux_gnu="$task_llvm_ar"
"$task_zigbuild" zigbuild --locked -p sailry-host --profile "$task_cargo_profile" \
  --target x86_64-unknown-linux-gnu.2.28 --target aarch64-unknown-linux-gnu.2.28 \
  --target-dir target/host-cross
cargo build --locked -p sailry-host --profile "$task_cargo_profile"
for task_target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu "$task_native"; do
  task_output="$task_root/target/host-artifacts/$task_target"
  mkdir -p "$task_output"
  if test "$task_target" = "$task_native"; then
    cp "target/$task_profile/sailry-host" "$task_output/sailry-host"
  else
    cp "target/host-cross/$task_target/$task_profile/sailry-host" "$task_output/sailry-host"
  fi
  "$task_llvm_strip" --strip-debug "$task_output/sailry-host"
  if [[ "$task_target" == *apple-darwin ]]; then codesign --force --sign - "$task_output/sailry-host"; fi
  chmod 755 "$task_output/sailry-host"
  python3 "$task_root/scripts/prepare-office-runtime.py" --target "$task_target" \
    --output "$task_root/target/office-runtimes/$task_target/office-runtime" \
    --archive "$task_output/office-runtime.tar.gz"
done
