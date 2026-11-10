set -eu
umask 077
command -v python3 >/dev/null
test -d "$install_stage"
test -f "$install_stage/sailry-host"
test ! -L "$install_stage/sailry-host"
test -f "$install_stage/$install_native"
test ! -L "$install_stage/$install_native"
if test "$(uname -s)" = Linux; then
  install_actual="$(sha256sum "$install_stage/sailry-host")"
  install_actual_native="$(sha256sum "$install_stage/$install_native")"
else
  install_actual="$(shasum -a 256 "$install_stage/sailry-host")"
  install_actual_native="$(shasum -a 256 "$install_stage/$install_native")"
fi
test "${install_actual%% *}" = "$install_digest"
test "${install_actual_native%% *}" = "$install_native_digest"
chmod 700 "$install_stage/sailry-host"
chmod 600 "$install_stage/$install_native"
test "$("$install_stage/sailry-host" --version)" = "Sailry Host $install_version"
install_parent="$(dirname "$install_root")"
install_backup="$install_parent/.sailry-previous"
install_unit_name="$(basename "$install_service_path")"
install_had_backup=0
if test "$install_update" = 0; then
  for install_path in "$install_root" "$install_data" "$install_service_path" "$install_command"; do
    if test -e "$install_path" || test -L "$install_path"; then
      echo 'Host files or data already exist; inspect the existing installation' >&2
      exit 1
    fi
  done
else
  test -d "$install_root"
  test ! -L "$install_root"
  test -f "$install_root/managed-go-host"
  test "$(cat "$install_root/managed-go-host")" = 1
  test -x "$install_root/sailry-host"
  test -f "$install_service_path"
  test ! -L "$install_service_path"
  test -L "$install_command"
  test "$(readlink "$install_command")" = "$install_root/sailry-host"
  if test -e "$install_backup" || test -L "$install_backup"; then
    test -d "$install_backup"
    test ! -L "$install_backup"
    test -f "$install_backup/managed-go-host"
    test "$(cat "$install_backup/managed-go-host")" = 1
    install_had_backup=1
  fi
fi
mkdir -p "$install_parent" "$(dirname "$install_command")"
install_publish="$(mktemp -d "$install_parent/.sailry-install-XXXXXX")"
install_changed=0
install_complete=0
install_cleanup() {
  install_status=$?
  trap - EXIT HUP INT TERM
  if test "$install_complete" = 0 && test "$install_changed" = 1; then
    if test "$install_update" = 1 && test -d "$install_backup"; then
      if test -d "$install_root"; then mv "$install_root" "$install_publish/failed" || exit 73; fi
      mv "$install_backup" "$install_root" || exit 73
    fi
    if test "$install_update" = 1 && test -d "$install_root"; then
      if test "$(uname -s)" = Linux; then
        if test "$(id -u)" = 0; then systemctl restart "$install_unit_name" || exit 73
        else systemctl --user restart "$install_unit_name" || exit 73; fi
      else
        launchctl kickstart -k "gui/$(id -u)/ai.sailry.host" || exit 73
      fi
    fi
    if test -d "$install_stage/previous-backup"; then
      mv "$install_stage/previous-backup" "$install_backup" || exit 73
    fi
    exit 73
  fi
  if test "$install_complete" = 0 && test -d "$install_stage/previous-backup"; then
    mv "$install_stage/previous-backup" "$install_backup" || exit 73
  fi
  if test -d "$install_publish"; then rm -rf -- "$install_publish"; fi
  exit "$install_status"
}
trap install_cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
cp "$install_stage/sailry-host" "$install_publish/sailry-host"
cp "$install_stage/$install_native" "$install_publish/$install_native"
printf '1\n' > "$install_publish/managed-go-host"
chmod 700 "$install_publish" "$install_publish/sailry-host"
chmod 600 "$install_publish/$install_native" "$install_publish/managed-go-host"
if test "$install_update" = 1; then
  if test "$install_had_backup" = 1; then
    mv "$install_backup" "$install_stage/previous-backup"
  fi
  install_changed=1
  if test "$(uname -s)" = Linux; then
    if test "$(id -u)" = 0; then systemctl stop "$install_unit_name"
    else export XDG_RUNTIME_DIR="/run/user/$(id -u)"; systemctl --user stop "$install_unit_name"; fi
  else
    launchctl kill SIGTERM "gui/$(id -u)/ai.sailry.host" || true
  fi
  mv "$install_root" "$install_backup"
fi
install_changed=1
mv "$install_publish" "$install_root"
install_publish="$(mktemp -d "$install_parent/.sailry-install-XXXXXX")"
if test "$install_update" = 0; then ln -s "$install_root/sailry-host" "$install_command"; fi
sh "$install_stage/service.sh" "$@" || exit 73
install_complete=1
