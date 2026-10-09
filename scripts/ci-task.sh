#!/usr/bin/env bash
set -euo pipefail

: "${SAILRY_REQUEST_ID:?Set a stable request ID for this task and retain it across retries}"
task_arguments=(submit --request-id "$SAILRY_REQUEST_ID" --timeout "${SAILRY_TIMEOUT:-600}")
if test -n "${SAILRY_SESSION:-}" && test -z "${SAILRY_WORKTREE:-}"; then
  task_arguments+=(--session "$SAILRY_SESSION")
elif test -n "${SAILRY_WORKTREE:-}" && test -z "${SAILRY_SESSION:-}"; then
  task_arguments+=(--worktree "$SAILRY_WORKTREE")
else
  printf '%s\n' 'Set exactly one of SAILRY_SESSION or SAILRY_WORKTREE' >&2
  exit 1
fi
if test -n "${SAILRY_EXPECTED_REVISION:-}"; then
  task_arguments+=(--expected-revision "$SAILRY_EXPECTED_REVISION")
fi
if test -n "${SAILRY_CONFIG:-}"; then task_arguments+=(--config "$SAILRY_CONFIG"); fi
if test -n "${SAILRY_PROFILE:-}"; then task_arguments+=(--data-dir "$SAILRY_PROFILE"); fi
if test -n "${SAILRY_PROMPT_FILE:-}"; then
  task_arguments+=(--prompt-file "$SAILRY_PROMPT_FILE")
else
  task_arguments+=(--stdin)
fi
exec "${SAILRY_COMMAND:-sailry}" "${task_arguments[@]}" "$@"
