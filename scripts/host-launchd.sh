#!/usr/bin/env bash
set -euo pipefail

# Generate a per-user service definition; installation and network choices stay explicit.
if test "$#" -lt 2; then
  echo 'Usage: bash scripts/host-launchd.sh <absolute-host-binary> <existing-private-profile> [host-options...]' >&2
  exit 1
fi
task_binary="$1"
task_profile="$2"
shift 2
test "$(uname -s)" = Darwin
case "$task_binary" in /*) ;; *) echo 'Host binary must be absolute' >&2; exit 1 ;; esac
case "$task_profile" in /*) ;; *) echo 'Host profile must be absolute' >&2; exit 1 ;; esac
test -x "$task_binary"
test -d "$task_profile"
task_output="$(mktemp -d "${TMPDIR:-/tmp}/sailry-host-service-XXXXXX")"
umask 077
jq -n --arg binary "$task_binary" --arg profile "$task_profile" --args '
  {Label:"ai.sailry.host",ProgramArguments:([$binary,"--data-dir",$profile]+$ARGS.positional),
   RunAtLoad:true,KeepAlive:{SuccessfulExit:false},ExitTimeOut:30,Umask:63,
   StandardOutPath:($profile+"/host.stdout.log"),StandardErrorPath:($profile+"/host.stderr.log")}
' -- "$@" | plutil -convert xml1 -o "$task_output/ai.sailry.host.plist" -
plutil -lint "$task_output/ai.sailry.host.plist"
printf 'Service definition: %s\n' "$task_output/ai.sailry.host.plist"
