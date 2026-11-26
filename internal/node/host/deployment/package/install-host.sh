#!/usr/bin/env bash
set -euo pipefail

# Install this package's release assets. Existing profiles are never changed.
task_package=sailry
task_label=ai.sailry.host
task_repository=https://github.com/sailry/sailry-harness
task_default_version=""
task_binary="$task_package-host"
task_version=""
task_update=0
task_network=()
task_network_mode=""
while test "$#" -gt 0; do
  case "$1" in
    --version) test "$#" -ge 2; test -z "$task_version"; task_version="${2#v}"; shift 2 ;;
    --expect-package)
      if test "${2:-}" != "$task_package/$task_label"; then echo 'Host installer package does not match' >&2; exit 1; fi
      shift 2 ;;
    --update) task_update=1; shift ;;
    --internet) test -z "$task_network_mode"; task_network_mode=internet; task_network=(--internet); shift ;;
    --relay) test "$#" -ge 2; test "$task_network_mode" != internet; task_network_mode=custom; task_network+=(--relay "$2"); shift 2 ;;
    *) echo 'Usage: install-host.sh [--version <version>] [--update] [--internet | --relay <multiaddr> ...]' >&2; exit 1 ;;
  esac
done
if test "$task_update" = 1 && test -n "$task_network_mode"; then
  echo 'Network options apply to new installations; edit an existing Host network through its controller' >&2
  exit 1
fi
task_select_update=0
if test -z "$task_version" && test "$task_update" = 1 && test "$task_repository" = https://github.com/sailry/sailry-code; then
  task_select_update=1
else
  task_version="${task_version:-$task_default_version}"
  if test -z "$task_version" && test "$task_repository" = https://github.com/sailry/sailry-harness; then
    echo 'Harness releases are source-only. Build the Host from source or specify a historical binary version with --version.' >&2
    exit 1
  fi
  if test -z "$task_version"; then
    task_version="$(curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
      "https://api.github.com/repos/${task_repository#https://github.com/}/releases?per_page=1" | \
      sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' | head -n 1)"
  fi
  case "$task_version" in ''|*[!a-zA-Z0-9.+-]*) echo 'Invalid release version' >&2; exit 1 ;; esac
  if ! [[ "$task_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
    echo 'Invalid release version' >&2; exit 1
  fi
fi
case "$(uname -s)/$(uname -m)" in
  Linux/x86_64) task_target=x86_64-unknown-linux-gnu ;;
  Linux/aarch64|Linux/arm64) task_target=aarch64-unknown-linux-gnu ;;
  Darwin/x86_64) task_target=x86_64-apple-darwin ;;
  Darwin/arm64) task_target=aarch64-apple-darwin ;;
  *) echo 'Host supports Linux and macOS on amd64 or arm64' >&2; exit 1 ;;
esac
task_parent="$HOME/.local/lib"
task_root="$task_parent/$task_package"
task_data="$HOME/.$task_binary"
task_command="$HOME/.local/bin/$task_package"
if test "$(uname -s)" = Linux && test "$(id -u)" = 0; then
  task_parent=/usr/local/libexec
  task_root="$task_parent/$task_package"
  task_command="/usr/local/bin/$task_package"
fi
command -v curl >/dev/null
command -v tar >/dev/null
if test "$(uname -s)" = Linux; then
  command -v systemctl >/dev/null
  task_unit="$HOME/.config/systemd/user/$task_binary.service"
  if test "$(id -u)" = 0; then task_unit="/etc/systemd/system/$task_binary.service"; fi
else
  launchctl print "gui/$(id -u)" >/dev/null
  task_unit="$HOME/Library/LaunchAgents/$task_label.plist"
fi
task_check_paths() {
  task_existing="$task_root"
  task_migrate=0
  task_had_command=0
  if test "$task_update" = 1 && test "$task_root" = "/usr/local/libexec/$task_package" \
    && ! test -e "$task_root" && ! test -L "$task_root" \
    && test -L "$task_command" \
    && test "$(readlink "$task_command")" = "$HOME/.local/lib/$task_package/$task_package"; then
    task_existing="$HOME/.local/lib/$task_package"
    task_migrate=1
  fi
  if test -e "$task_command" || test -L "$task_command"; then
    if ! test -L "$task_command" || test "$(readlink "$task_command")" != "$task_existing/$task_package"; then
      printf 'The %s command path is already in use\n' "$task_package" >&2
      exit 1
    fi
    task_had_command=1
  fi
  if test "$task_update" = 0; then
    for task_path in "$task_root" "$task_data" "$task_unit"; do
      if test -e "$task_path" || test -L "$task_path"; then
        echo 'Host files or data already exist; inspect the existing installation' >&2
        exit 1
      fi
    done
  else
    test -x "$task_existing/$task_binary"
    test -x "$task_existing/$task_package"
  fi
}
task_check_paths
if test "$task_select_update" = 1; then
  task_current_version="$("$task_existing/$task_binary" --version)"
  SAILRY_HOST_EXPECT_CURRENT_VERSION="${SAILRY_HOST_EXPECT_CURRENT_VERSION:-$task_current_version}"
  task_version="$("$task_existing/python/bin/python3.12" -I "$task_existing/select-host-release.py" \
    "$task_repository" "$task_binary-$task_target.tar.gz" "SHA256SUMS-$task_target" "$task_current_version")"
  if test -z "$task_version"; then
    printf 'Host %s is already up to date\n' "${task_current_version#Sailry Host }"
    exit 0
  fi
fi
umask 077
# Shared parents such as /usr/local/libexec stay usable by other software;
# the program directory itself is published from the private stage.
(umask 022 && mkdir -p "$task_parent" "$(dirname "$task_command")")
task_stage="$(mktemp -d "$task_parent/.$task_package-install-XXXXXX")"
task_backup="$task_parent/.$task_package-previous"
task_changing=0
task_starting=0
task_complete=0
task_active=0
task_published=0
task_created_data=0
task_created_command=0
install_unit_created=0
install_activating=0
task_stop() {
  local task_output task_status=0
  task_output="$("$1/$task_package" stop 2>&1)" || task_status=$?
  # launchctl reports ESRCH when this managed service target is not loaded.
  if test "$task_status" = 3 && test "$(uname -s)" = Darwin; then return 0; fi
  if test -n "$task_output"; then printf '%s\n' "$task_output" >&2; fi
  return "$task_status"
}
task_rollback() {
  if test "$task_update" = 0; then
    if test "$install_unit_created" = 1; then
      if test "$install_activating" = 1; then
        if test "$(uname -s)" = Linux; then
          if test "$(id -u)" = 0; then systemctl disable --now "$(basename "$task_unit")" || return
          else systemctl --user disable --now "$(basename "$task_unit")" || return; fi
        else
          local task_status=0
          launchctl bootout "gui/$(id -u)/$task_label" || task_status=$?
          if test "$task_status" != 0 && test "$task_status" != 3; then return "$task_status"; fi
        fi
      fi
      rm -- "$task_unit" || return
      if test "$(uname -s)" = Linux && test "$install_activating" = 1; then
        if test "$(id -u)" = 0; then systemctl daemon-reload || return
        else systemctl --user daemon-reload || return; fi
      fi
    fi
    if test "$task_created_command" = 1; then
      test -L "$task_command" && test "$(readlink "$task_command")" = "$task_root/$task_package" || return
      rm -- "$task_command" || return
    fi
    if test "$task_published" = 1; then mv "$task_root" "$task_stage/failed" || return; fi
    if test "$task_created_data" = 1 && ! rmdir -- "$task_data" 2>/dev/null; then
      printf 'Host data remains at %s; inspect it before retrying the installation\n' "$task_data" >&2
    fi
    return 0
  fi
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
    ln -sfn "$task_existing/$task_package" "$task_command" || return
  elif test -L "$task_command" && test "$(readlink "$task_command")" = "$task_root/$task_package"; then
    rm -- "$task_command" || return
  fi
  if test "$task_active" = 1; then
    "$task_existing/$task_package" start || return
  fi
}
task_cleanup() {
  task_status=$?
  trap - EXIT HUP INT TERM
  if test "$task_changing" = 1 && test "$task_complete" = 0; then
    if ! task_rollback; then
      printf 'Host recovery failed; inspect program files at %s and %s\n' "$task_existing" "$task_backup" >&2
      printf 'Recovery files remain at %s\n' "$task_stage" >&2
      exit 73
    fi
    if test "$task_update" = 1; then
      echo 'Host update failed; the previous installation was restored' >&2
    else
      echo 'Host installation failed; its program and service files were removed' >&2
    fi
  fi
  if test "$task_complete" = 0 && test -d "$task_stage/previous-backup"; then
    if test -e "$task_backup" || test -L "$task_backup" \
      || ! mv "$task_stage/previous-backup" "$task_backup"; then
      printf 'Host backup recovery failed; files remain at %s\n' "$task_stage" >&2
      exit 73
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
task_archive="$task_binary-$task_target.tar.gz"
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
test -x "$task_stage/package/$task_binary"
test -f "$task_stage/package/$task_package"
test -f "$task_stage/package/service.sh"
chmod 700 "$task_stage/package/$task_package" "$task_stage/package/$task_binary"
test "$("$task_stage/package/$task_binary" --version)" = "Sailry Host $task_version"
# Share this lock with SSH deployment. Python locks the descriptor owned by
# this shell, which keeps it through publication, service startup and rollback.
exec 9<> "$task_parent/.$task_package-install.lock"
"$task_stage/package/python/bin/python3.12" -I - <<'PY'
import fcntl
import sys
try:
    fcntl.flock(9, fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    sys.exit("Another Host installation or update is in progress")
PY
task_check_paths
# SSH verifies a paired Host before downloading the release. Compare that
# version under the shared lock before changing the installation.
if test "$task_update" = 1 && test -n "${SAILRY_HOST_EXPECT_CURRENT_VERSION:-}"; then
  if ! task_current_version="$("$task_existing/$task_binary" --version)" || test "$task_current_version" != "$SAILRY_HOST_EXPECT_CURRENT_VERSION"; then
    echo 'Host version changed after verification; inspect it before retrying' >&2
    exit 1
  fi
fi
if test "$task_migrate" = 1; then
  cp -p "$task_unit" "$task_stage/previous-service"
  "$task_stage/package/python/bin/python3.12" -I - "$task_unit" "$task_existing" "$task_root" "$task_stage/service" "$task_binary" <<'PY'
from pathlib import Path
import sys
unit, previous, current, output, binary = sys.argv[1:]
lines = Path(unit).read_text().splitlines(keepends=True)
prefix = f'ExecStart="{previous}/{binary}" '
if sum(line.startswith(prefix) for line in lines) != 1:
    raise RuntimeError("Existing Host service does not match its managed program path")
Path(output).write_text(''.join(
    f'ExecStart="{current}/{binary}" ' + line[len(prefix):]
    if line.startswith(prefix) else line for line in lines))
PY
fi
if test "$task_update" = 1; then
  if test -e "$task_backup" || test -L "$task_backup"; then
    if test -L "$task_backup" || ! test -d "$task_backup" \
      || ! test -x "$task_backup/$task_binary" || ! test -x "$task_backup/$task_package"; then
      echo 'Host backup path is not a managed program directory' >&2
      exit 1
    fi
    mv "$task_backup" "$task_stage/previous-backup"
  fi
  if "$task_existing/$task_package" status >/dev/null 2>&1; then task_active=1; fi
  task_changing=1
  task_stop "$task_existing"
  mv "$task_existing" "$task_backup"
fi
task_changing=1
mv "$task_stage/package" "$task_root"
task_published=1
if test "$(uname -s)" = Linux && test "$(id -u)" = 0 && command -v restorecon >/dev/null; then
  restorecon -RF "$task_root"
fi
if test "$task_migrate" = 1; then
  cat "$task_stage/service" > "$task_unit"
  systemctl daemon-reload
fi
if test "$task_update" = 1; then
  ln -sfn "$task_root/$task_package" "$task_command"
  task_starting=1
  "$task_root/$task_package" start
  task_complete=1
  printf 'Host updated; previous files: %s\n' "$task_backup"
else
  mkdir "$task_data"
  task_created_data=1
  if test "$task_had_command" = 0; then
    ln -s "$task_root/$task_package" "$task_command"
    task_created_command=1
  fi
  # The installer owns rollback; sourcing reports which service mutations
  # succeeded even when a later registration step fails.
  if test -n "$task_network_mode"; then
    . "$task_root/service.sh" "${task_network[@]}"
  else
    . "$task_root/service.sh"
  fi
  task_complete=1
  printf 'Host installed; run %s share to pair\n' "$task_command"
fi
