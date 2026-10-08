#!/usr/bin/env bash
# Use the same supported Python range locally and in CI without changing PATH.
set -euo pipefail

task_candidates=("${SAILRY_PYTHON:-python3}" python3.14 python3.13 python3.12)
if test -n "${SAILRY_PYTHON:-}"; then
  task_candidates=("$SAILRY_PYTHON")
fi
for task_python in "${task_candidates[@]}"; do
  if command -v "$task_python" >/dev/null 2>&1 &&
      "$task_python" -c 'import sys; sys.exit(sys.version_info < (3, 12))'; then
    exec "$task_python" "$@"
  fi
done
printf '%s\n' 'Python 3.12 or newer is required; set SAILRY_PYTHON to its executable' >&2
exit 1
