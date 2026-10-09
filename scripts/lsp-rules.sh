#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 2 ]]; then
  printf 'Usage: scripts/lsp-rules.sh <helix-commit> <mason-release-tag>\n' >&2
  exit 2
fi
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
bash "$root/scripts/python.sh" "$root/scripts/lsp-rules.py" "$1" "$2"
