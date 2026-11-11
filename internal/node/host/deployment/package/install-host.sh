#!/usr/bin/env bash
set -euo pipefail

# Install only official release assets. Existing profiles are never changed.
task_version=""
task_update=0
task_network=()
task_network_mode=""
while test "$#" -gt 0; do
  case "$1" in
    --version) test "$#" -ge 2; test -z "$task_version"; task_version="${2#v}"; shift 2 ;;
    --update) task_update=1; shift ;;
    --internet) test -z "$task_network_mode"; task_network_mode=iroh; task_network=(--internet); shift ;;
    --relay) test "$#" -ge 2; test "$task_network_mode" != iroh; task_network_mode=custom; task_network+=(--relay "$2"); shift 2 ;;
    *) echo 'Usage: install-host.sh [--version <version>] [--update] [--internet | --relay <HTTPS URL> ...]' >&2; exit 1 ;;
  esac
done
if test "$task_update" = 1 && test -n "$task_network_mode"; then
  echo 'Network options apply to new installations; edit an existing Host network through its controller' >&2
  exit 1
fi
task_repository=https://github.com/sailry/sailry-harness
if test -z "$task_version"; then
  task_version="$(curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
    https://api.github.com/repos/sailry/sailry-harness/releases?per_page=1 | \
    sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' | head -n 1)"
fi
case "$task_version" in ''|*[!a-zA-Z0-9.+-]*) echo 'Invalid release version' >&2; exit 1 ;; esac
if ! [[ "$task_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo 'Invalid release version' >&2; exit 1
fi
case "$(uname -s)/$(uname -m)" in
  Linux/x86_64) task_target=x86_64-unknown-linux-gnu ;;
  Linux/aarch64|Linux/arm64) task_target=aarch64-unknown-linux-gnu ;;
  Darwin/x86_64) task_target=x86_64-apple-darwin ;;
  Darwin/arm64) task_target=aarch64-apple-darwin ;;
  *) echo 'Host supports Linux and macOS on amd64 or arm64' >&2; exit 1 ;;
esac
task_parent="$HOME/.local/lib"
task_root="$task_parent/sailry"
task_data="$HOME/.sailry-host"
task_command="$HOME/.local/bin/sailry"
if test "$(uname -s)" = Linux && test "$(id -u)" = 0; then
  task_parent=/usr/local/libexec
  task_root="$task_parent/sailry"
  task_command=/usr/local/bin/sailry
fi
task_existing="$task_root"
task_migrate=0
task_had_command=0
if test "$task_update" = 1 && test "$task_root" = /usr/local/libexec/sailry \
  && ! test -e "$task_root" && ! test -L "$task_root" \
  && test -L "$task_command" \
  && test "$(readlink "$task_command")" = "$HOME/.local/lib/sailry/sailry"; then
  task_existing="$HOME/.local/lib/sailry"
  task_migrate=1
fi
if test -e "$task_command" || test -L "$task_command"; then
  if ! test -L "$task_command" || test "$(readlink "$task_command")" != "$task_existing/sailry"; then
    echo 'The sailry command path is already in use' >&2
    exit 1
  fi
  task_had_command=1
fi
if test "$task_update" = 0; then
  if test -e "$task_root" || test -e "$task_data"; then
    echo 'Host files or data already exist; inspect the existing installation' >&2
    exit 1
  fi
else
  test -x "$task_existing/sailry-host"
  test -x "$task_existing/sailry"
fi
command -v curl >/dev/null
command -v tar >/dev/null
if test "$(uname -s)" = Linux; then
  command -v systemctl >/dev/null
  task_unit="$HOME/.config/systemd/user/sailry-host.service"
  if test "$(id -u)" = 0; then task_unit=/etc/systemd/system/sailry-host.service; fi
else
  launchctl print "gui/$(id -u)" >/dev/null
  task_unit="$HOME/Library/LaunchAgents/ai.sailry.host.plist"
fi
if test "$task_update" = 0; then test ! -e "$task_unit"; fi
umask 077
mkdir -p "$task_parent" "$(dirname "$task_command")"
task_stage="$(mktemp -d "$task_parent/.sailry-install-XXXXXX")"
task_backup="$task_parent/.sailry-previous"
task_changing=0
task_starting=0
task_complete=0
task_active=0
task_stop() {
  local task_output task_status=0
  task_output="$("$1/sailry" stop 2>&1)" || task_status=$?
  # launchctl reports ESRCH when this managed service target is not loaded.
  if test "$task_status" = 3 && test "$(uname -s)" = Darwin; then return 0; fi
  if test -n "$task_output"; then printf '%s\n' "$task_output" >&2; fi
  return "$task_status"
}
task_rollback() {
  if test "$task_starting" = 1; then
    task_stop "$task_backup" || return
  fi
  if test -d "$task_backup"; then
    if test -e "$task_root" || test -L "$task_root"; then
      mv "$task_root" "$task_stage/failed" || return
    fi
    mv "$task_backup" "$task_existing" || return
  fi
  if test "$task_migrate" = 1; then
    cp -p "$task_stage/previous-service" "$task_unit" || return
    systemctl daemon-reload || return
  fi
  if test "$task_had_command" = 1; then
    ln -sfn "$task_existing/sailry" "$task_command" || return
  elif test -L "$task_command" && test "$(readlink "$task_command")" = "$task_root/sailry"; then
    rm -- "$task_command" || return
  fi
  if test "$task_active" = 1; then
    "$task_existing/sailry" start || return
  fi
}
task_cleanup() {
  task_status=$?
  trap - EXIT HUP INT TERM
  if test "$task_changing" = 1 && test "$task_complete" = 0; then
    if ! task_rollback; then
      printf 'Host recovery failed; inspect program files at %s and %s\n' "$task_existing" "$task_backup" >&2
      printf 'Recovery files remain at %s\n' "$task_stage" >&2
      exit 1
    fi
    echo 'Host update failed; the previous installation was restored' >&2
  fi
  if test "$task_complete" = 0 && test -d "$task_stage/previous-backup"; then
    if test -e "$task_backup" || test -L "$task_backup" \
      || ! mv "$task_stage/previous-backup" "$task_backup"; then
      printf 'Host backup recovery failed; files remain at %s\n' "$task_stage" >&2
      exit 1
    fi
  fi
  find "$task_stage" -type d -exec chmod u+w {} +
  rm -rf -- "$task_stage"
  exit "$task_status"
}
trap task_cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
task_archive="sailry-host-$task_target.tar.gz"
task_release="$task_repository/releases/download/v$task_version"
for task_asset in "$task_archive" "SHA256SUMS-$task_target"; do
  if ! curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
    "$task_release/$task_asset" --output "$task_stage/$task_asset"; then
    echo "Host $task_version is not available for $task_target" >&2
    exit 1
  fi
done
task_expected="$(awk -v name="$task_archive" '$2 == name || $2 == "./" name {print $1}' "$task_stage/SHA256SUMS-$task_target")"
test "${#task_expected}" = 64
case "$task_expected" in *[!0-9a-f]*) echo 'Invalid release checksum' >&2; exit 1 ;; esac
if test "$(uname -s)" = Linux; then
  task_actual="$(sha256sum "$task_stage/$task_archive")"
else
  task_actual="$(shasum -a 256 "$task_stage/$task_archive")"
fi
if test "${task_actual%% *}" != "$task_expected"; then echo 'Host checksum does not match' >&2; exit 1; fi
mkdir "$task_stage/package"
tar --no-same-owner -xzf "$task_stage/$task_archive" -C "$task_stage/package"
test -x "$task_stage/package/sailry-host"
test -f "$task_stage/package/sailry"
test -f "$task_stage/package/service.sh"
chmod 700 "$task_stage/package/sailry" "$task_stage/package/sailry-host"
test "$("$task_stage/package/sailry-host" --version)" = "Sailry Host $task_version"
if test "$task_migrate" = 1; then
  cp -p "$task_unit" "$task_stage/previous-service"
  "$task_stage/package/python/bin/python3.12" -I - "$task_unit" "$task_existing" "$task_root" "$task_stage/service" <<'PY'
from pathlib import Path
import sys
unit, previous, current, output = sys.argv[1:]
lines = Path(unit).read_text().splitlines(keepends=True)
prefix = f'ExecStart="{previous}/sailry-host" '
if sum(line.startswith(prefix) for line in lines) != 1:
    raise RuntimeError("Existing Host service does not match its managed program path")
Path(output).write_text(''.join(
    f'ExecStart="{current}/sailry-host" ' + line[len(prefix):]
    if line.startswith(prefix) else line for line in lines))
PY
fi
if test "$task_update" = 1; then
  if test -e "$task_backup" || test -L "$task_backup"; then
    if test -L "$task_backup" || ! test -d "$task_backup" \
      || ! test -x "$task_backup/sailry-host" || ! test -x "$task_backup/sailry"; then
      echo 'Host backup path is not a managed program directory' >&2
      exit 1
    fi
    mv "$task_backup" "$task_stage/previous-backup"
  fi
  if "$task_existing/sailry" status >/dev/null 2>&1; then task_active=1; fi
  task_changing=1
  task_stop "$task_existing"
  mv "$task_existing" "$task_backup"
fi
mv "$task_stage/package" "$task_root"
if test "$(uname -s)" = Linux && test "$(id -u)" = 0 && command -v restorecon >/dev/null; then
  restorecon -RF "$task_root"
fi
if test "$task_migrate" = 1; then
  cat "$task_stage/service" > "$task_unit"
  systemctl daemon-reload
fi
ln -sfn "$task_root/sailry" "$task_command"
if test "$task_update" = 1; then
  task_starting=1
  "$task_root/sailry" start
  task_complete=1
  printf 'Host updated; previous files: %s\n' "$task_backup"
else
  if test -n "$task_network_mode"; then
    bash "$task_root/service.sh" "${task_network[@]}"
  else
    bash "$task_root/service.sh"
  fi
  printf 'Host installed; run %s share to pair\n' "$task_command"
fi
