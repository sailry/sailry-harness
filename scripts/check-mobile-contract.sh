#!/usr/bin/env bash
set -euo pipefail
# Exercise native Dart FFI against isolated Nodes, without Flutter or device lifecycle.
exec bash scripts/python.sh scripts/ci.py check mobile-contract
