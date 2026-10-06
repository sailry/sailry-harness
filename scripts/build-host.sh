#!/usr/bin/env bash
set -euo pipefail

# Build one release target. Linux builders do not compile Desktop or other architectures.
task_profile="${1:-release}"
task_target="${2:?Usage: build-host.sh [debug|release] <target>}"
case "$task_profile" in debug) task_cargo_profile=dev ;; release) task_cargo_profile=release ;; *) echo 'Invalid build profile' >&2; exit 1 ;; esac
case "$task_target" in x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin) ;; *) echo 'Unsupported Host target' >&2; exit 1 ;; esac
task_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$task_root"
task_native="$(rustc -vV | awk '/^host: / {print $2}')"
if [[ "$task_target" == *linux-gnu ]]; then
  task_ar="$(command -v llvm-ar || true)"
  task_strip="$(command -v llvm-strip || true)"
  if { test -z "$task_ar" || test -z "$task_strip"; } && command -v brew >/dev/null; then
    task_llvm="$(brew --prefix llvm@21)"
    task_ar="$task_llvm/bin/llvm-ar"
    task_strip="$task_llvm/bin/llvm-strip"
  fi
  test -x "$task_ar"
  test -x "$task_strip"
  task_zigbuild="$task_root/target/tooling/bin/cargo-zigbuild"
  test -x "$task_zigbuild"
  test "$("$task_zigbuild" --version)" = 'cargo-zigbuild 0.23.4'
  task_sysroot="$task_root/target/host-sysroots/$task_target"
  python3 scripts/package/linux-sysroot.py --target "$task_target" --output "$task_sysroot" --ar "$task_ar"
  task_triplet="${task_target/-unknown/}"
  task_environment="${task_target//-/_}"
  export "AR_$task_environment=$task_ar"
  export "PKG_CONFIG_SYSROOT_DIR_$task_environment=$task_sysroot"
  export "PKG_CONFIG_LIBDIR_$task_environment=$task_sysroot/usr/lib/$task_triplet/pkgconfig:$task_sysroot/usr/share/pkgconfig"
  "$task_zigbuild" zigbuild --locked -p sailry-host --profile "$task_cargo_profile" \
    --target "$task_target.2.28" --target-dir target/host-cross
  task_binary="$task_root/target/host-cross/$task_target/$task_profile/sailry-host"
else
  test "$task_target" = "$task_native"
  cargo build --locked -p sailry-host --profile "$task_cargo_profile"
  task_binary="$task_root/target/$task_profile/sailry-host"
fi
task_output="$task_root/target/host-artifacts/$task_target"
mkdir -p "$task_output"
cp "$task_binary" "$task_output/sailry-host"
if [[ "$task_target" == *linux-gnu ]]; then
  "$task_strip" --strip-debug "$task_output/sailry-host"
else
  strip -S "$task_output/sailry-host"
  codesign --force --sign - "$task_output/sailry-host"
fi
chmod 755 "$task_output/sailry-host"
printf 'Host build: %s\n' "$task_output"
