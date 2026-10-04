#!/usr/bin/env bash
set -euo pipefail
# Run from the repository root. This exercises native Dart FFI against a real
# isolated Node; it does not launch Flutter pages or prove device lifecycle.
command -v dart >/dev/null
(cd tests/mobile-contract && dart pub get --enforce-lockfile && dart analyze)
cargo build --locked -p sailry-mobile-bridge
cargo test --locked -p sailry-mobile-bridge --test dart -- --ignored --skip mcp_peer::stdio_peer --nocapture
