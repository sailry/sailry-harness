#!/usr/bin/env bash
set -euo pipefail
# Run from the repository root. This exercises native Dart FFI against a real
# isolated Node; it does not launch Flutter pages or prove device lifecycle.
command -v dart >/dev/null
task_triple="$(rustc -vV | awk '/^host: / {print $2}')"
task_parsers=(--target "$task_triple" --output "${CARGO_TARGET_DIR:-target}/debug/code-parsers")
if [[ "$task_triple" == *apple-darwin ]]; then task_parsers+=(--sign -); fi
TSLP_SOURCE_BUNDLE_URL="$(bash scripts/python.sh scripts/package/code-parsers.py "${task_parsers[@]}")"
export TSLP_SOURCE_BUNDLE_URL
unset TSLP_OFFLINE TSLP_LANGUAGES
(cd tests/mobile-contract && dart pub get --enforce-lockfile && dart analyze)
cargo build --locked -p sailry-mobile-bridge
cargo test --locked -p sailry-mobile-bridge --test dart -- --ignored --skip mcp_peer::stdio_peer --nocapture
