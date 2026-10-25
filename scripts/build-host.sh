#!/usr/bin/env bash
set -euo pipefail

# Build one release target. Linux builders do not compile Desktop or other architectures.
task_profile="${1:-release}"
task_target="${2:?Usage: build-host.sh [debug|release] <target>}"
case "$task_profile" in debug) task_cargo_profile=dev ;; release) task_cargo_profile=release ;; *) echo 'Invalid build profile' >&2; exit 1 ;; esac
case "$task_target" in x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin) ;; *) echo 'Unsupported Host target' >&2; exit 1 ;; esac
task_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$task_root"
task_build="${CARGO_TARGET_DIR:-$task_root/target}"
export CARGO_TARGET_DIR="$task_build"
task_native="$(rustc -vV | awk '/^host: / {print $2}')"
task_parser_output="$task_build/$task_profile/code-parsers"
if [[ "$task_target" == *linux-gnu ]]; then task_parser_output="$task_build/$task_target/$task_profile/code-parsers"; fi
task_parsers=(--target "$task_target" --output "$task_parser_output" --cache "$task_root/.runtime/code-index/resources")
if [[ "$task_target" == *apple-darwin ]]; then task_parsers+=(--sign -); fi
TSLP_SOURCE_BUNDLE_URL="$(bash "$task_root/scripts/python.sh" "$task_root/scripts/package/code-parsers.py" "${task_parsers[@]}")"
export TSLP_SOURCE_BUNDLE_URL
unset TSLP_OFFLINE TSLP_LANGUAGES
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
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/linux-sysroot.py" --target "$task_target" --output "$task_sysroot" --ar "$task_ar"
  task_triplet="${task_target/-unknown/}"
  task_environment="${task_target//-/_}"
  export "AR_$task_environment=$task_ar"
  export "PKG_CONFIG_SYSROOT_DIR_$task_environment=$task_sysroot"
  export "PKG_CONFIG_LIBDIR_$task_environment=$task_sysroot/usr/lib/$task_triplet/pkgconfig:$task_sysroot/usr/share/pkgconfig"
  "$task_zigbuild" zigbuild --locked -p sailry-host --profile "$task_cargo_profile" \
    --target "$task_target.2.34" --target-dir "$task_build"
  task_binary="$task_build/$task_target/$task_profile/sailry-host"
else
  test "$task_target" = "$task_native"
  cargo build --locked -p sailry-host --profile "$task_cargo_profile"
  task_binary="$task_build/$task_profile/sailry-host"
fi
task_output="$task_root/target/host-artifacts/$task_target"
mkdir -p "$task_output"
cp "$task_binary" "$task_output/sailry-host"
cp -R "$task_parser_output" "$task_output/"
bash "$task_root/scripts/python.sh" "$task_root/scripts/package/onnx-runtime.py" --target "$task_target" \
  --output "$task_output" --cache "$task_root/target/sherpa-onnx-prebuilt"
task_llama=(--target "$task_target" --output "$task_output" --cache "$task_root/target/llama-prebuilt")
if [[ "$task_target" == *apple-darwin ]]; then task_llama+=(--sign -); fi
bash "$task_root/scripts/python.sh" "$task_root/scripts/package/llama-runtime.py" "${task_llama[@]}"
bash "$task_root/scripts/python.sh" "$task_root/scripts/package/python-runtime.py" --target "$task_target" \
  --output "$task_output" --cache "$task_root/target/python-cache"
if [[ "$task_target" == *linux-gnu ]]; then
  "$task_strip" --strip-debug "$task_output/sailry-host"
else
  task_ocu=(--target "$task_target" --output "$task_output/helpers" --cache "$task_root/target/ocu-helper-cache" --sign -)
  if test "$task_profile" = debug; then task_ocu+=(--development); fi
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/ocu-helper.py" "${task_ocu[@]}"
  strip -S "$task_output/sailry-host"
  codesign --force --sign - "$task_output/libonnxruntime.dylib"
  codesign --force --sign - "$task_output/sailry-host"
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/python-runtime.py" --target "$task_target" \
    --output "$task_output" --sign -
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/onnx-runtime.py" --target "$task_target" \
    --output "$task_output" --finalize
fi
chmod 755 "$task_output/sailry-host"
printf 'Host build: %s\n' "$task_output"
