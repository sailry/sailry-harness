#!/usr/bin/env bash
set -euo pipefail

task_measure=0
if test "$#" = 1 && test "$1" = --measure; then
  task_measure=1
elif test "$#" != 0; then
  echo 'Usage: check-decision-native.sh [--measure]' >&2
  exit 1
fi
task_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$task_root"
task_target="$(rustc -vV | awk '$1 == "host:" {print $2}')"
task_build="${CARGO_TARGET_DIR:-target}"
task_libraries="$task_build/debug/deps"
cargo test --locked -p sailry-decision --lib --no-run
task_stage() {
  local task_output="$1"
  local task_llama=(--target "$task_target" --output "$task_output" --cache "$task_build/llama-prebuilt")
  case "$task_target" in *-apple-darwin) task_llama+=(--sign -);; esac
  bash scripts/python.sh scripts/package/llama-runtime.py "${task_llama[@]}"
  bash scripts/python.sh scripts/package/onnx-runtime.py --target "$task_target" \
    --output "$task_output" --cache "$task_build/sherpa-onnx-prebuilt"
  case "$task_target" in
    *-apple-darwin)
      codesign --force --sign - "$task_output/libonnxruntime.dylib"
      bash scripts/python.sh scripts/package/onnx-runtime.py --target "$task_target" \
        --output "$task_output" --finalize
      ;;
  esac
}
task_stage "$task_libraries"
bash scripts/python.sh crates/decision/tests/golden.py
cargo test --locked -p sailry-decision --lib
cargo test --locked -p sailry-decision --lib runtime::tests::loads_scored_cpu_backend -- --ignored --exact --nocapture
cargo test --locked -p sailry-decision --lib runtime::tests::caches_verified_package -- --ignored --exact --nocapture
cargo test --locked -p sailry-decision --lib engine::tests::native:: -- --ignored --test-threads=1 --nocapture
cargo test --locked -p sailry-decision --lib engine::tests::coexist::onnx_first -- --ignored --exact --nocapture
cargo test --locked -p sailry-decision --lib engine::tests::coexist::gguf_first -- --ignored --exact --nocapture
if test "$task_measure" = 1; then
  cargo build --locked -p sailry-decision --example qualify
  task_stage "$task_build/debug/examples"
  task_fixture="$task_root/.runtime/local-model-host/models/julia"
  task_report="$task_root/.runtime/local-model-host/qualification/native-report.json"
  mkdir -p "$(dirname "$task_report")"
  "$task_build/debug/examples/qualify" "$task_fixture/definition.json" "$task_fixture" > "$task_report"
  cat "$task_report"
fi
