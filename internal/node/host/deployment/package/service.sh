set -eu
umask 077
install_binary="$task_package-host"
install_service="$install_binary.service"
install_unit_created=0
install_activating=0
install_root="$HOME/.local/lib/$task_package"
if test "$(uname -s)" = Linux && test "$(id -u)" = 0; then
  install_root="/usr/local/libexec/$task_package"
fi
install_data="$HOME/.$install_binary"
mkdir -p "$install_root" "$install_data"
install_claim_unit() {
  (set -C; : > "$install_unit")
  install_unit_created=1
}
install_network=--internet
if test "$#" -gt 0; then
  # systemd expands dollar signs and specifiers even inside quoted arguments.
  install_network="$("$install_root/python/bin/python3.12" -I - "$@" <<'PY'
import sys
escapes = {'\\': '\\\\', '"': '\\"', '$': '$$', '%': '%%', '\n': '\\n', '\r': '\\r', '\t': '\\t'}
def quote(value):
    return '"' + ''.join(escapes.get(char, f'\\x{ord(char):02x}' if ord(char) < 32 else char) for char in value) + '"'
print(' '.join(quote(value) for value in sys.argv[1:]))
PY
)"
fi
if test "$(uname -s)" = Linux; then
  command -v systemctl >/dev/null
  if test "$(id -u)" = 0; then
    install_unit="/etc/systemd/system/$install_service"
    test ! -e "$install_unit"
    install_claim_unit
    cat > "$install_unit" <<UNIT
[Unit]
Description=Sailry Host
After=network-online.target
Wants=network-online.target
[Service]
ExecStart="$install_root/$install_binary" --data-dir "$install_data" $install_network --bootstrap
Restart=on-failure
RestartSec=3
UMask=0077
[Install]
WantedBy=multi-user.target
UNIT
    systemctl daemon-reload
    install_activating=1
    systemctl enable --now "$install_service"
    systemctl is-active --quiet "$install_service"
  else
    install_unit="$HOME/.config/systemd/user/$install_service"
    test ! -e "$install_unit"
    loginctl --no-ask-password enable-linger "$(id -un)"
    export XDG_RUNTIME_DIR="/run/user/$(id -u)"
    mkdir -p "$(dirname "$install_unit")"
    install_claim_unit
    cat > "$install_unit" <<UNIT
[Unit]
Description=Sailry Host
After=network-online.target
[Service]
ExecStart="%h/.local/lib/$task_package/$install_binary" --data-dir "%h/.$install_binary" $install_network --bootstrap
Restart=on-failure
RestartSec=3
UMask=0077
[Install]
WantedBy=default.target
UNIT
    systemctl --user daemon-reload
    install_activating=1
    systemctl --user enable --now "$install_service"
    systemctl --user is-active --quiet "$install_service"
  fi
else
  install_uid="$(id -u)"
  launchctl print "gui/$install_uid" >/dev/null
  install_unit="$HOME/Library/LaunchAgents/$task_label.plist"
  test ! -e "$install_unit"
  mkdir -p "$(dirname "$install_unit")"
  install_claim_unit
  printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' '<plist version="1.0"><dict/></plist>' > "$install_unit"
  /usr/libexec/PlistBuddy -c "Add :Label string $task_label" "$install_unit"
  /usr/libexec/PlistBuddy -c 'Add :ProgramArguments array' "$install_unit"
  /usr/libexec/PlistBuddy -c "Add :ProgramArguments: string $install_root/$install_binary" "$install_unit"
  /usr/libexec/PlistBuddy -c 'Add :ProgramArguments: string --data-dir' "$install_unit"
  /usr/libexec/PlistBuddy -c "Add :ProgramArguments: string $install_data" "$install_unit"
  if test "$#" -eq 0; then
    /usr/libexec/PlistBuddy -c 'Add :ProgramArguments: string --internet' "$install_unit"
  else
    # Pass values as argv instead of interpolating them into PlistBuddy commands.
    "$install_root/python/bin/python3.12" -I - "$install_unit" "$@" <<'PY'
import pathlib, plistlib, sys
path = pathlib.Path(sys.argv[1])
data = plistlib.loads(path.read_bytes())
data['ProgramArguments'].extend(sys.argv[2:])
path.write_bytes(plistlib.dumps(data))
PY
  fi
  /usr/libexec/PlistBuddy -c 'Add :ProgramArguments: string --bootstrap' "$install_unit"
  /usr/libexec/PlistBuddy -c 'Add :RunAtLoad bool true' "$install_unit"
  /usr/libexec/PlistBuddy -c 'Add :KeepAlive dict' "$install_unit"
  /usr/libexec/PlistBuddy -c 'Add :KeepAlive:SuccessfulExit bool false' "$install_unit"
  /usr/libexec/PlistBuddy -c 'Add :Umask integer 63' "$install_unit"
  /usr/libexec/PlistBuddy -c "Add :StandardOutPath string $install_data/host.stdout.log" "$install_unit"
  /usr/libexec/PlistBuddy -c "Add :StandardErrorPath string $install_data/host.stderr.log" "$install_unit"
  install_activating=1
  launchctl bootstrap "gui/$install_uid" "$install_unit"
fi
