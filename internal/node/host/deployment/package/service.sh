set -eu
umask 077
install_root="$HOME/.local/lib/sailry"
if test "$(uname -s)" = Linux && test "$(id -u)" = 0; then
  install_root=/usr/local/libexec/sailry
fi
install_data="$HOME/.sailry-host"
mkdir -p "$install_root" "$install_data"
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
    install_unit=/etc/systemd/system/sailry-host.service
    test ! -e "$install_unit"
    cat > "$install_unit" <<UNIT
[Unit]
Description=Sailry Host
After=network-online.target
Wants=network-online.target
[Service]
ExecStart="$install_root/sailry-host" --data-dir "$install_data" $install_network --bootstrap
Restart=on-failure
RestartSec=3
UMask=0077
[Install]
WantedBy=multi-user.target
UNIT
    systemctl daemon-reload
    systemctl enable --now sailry-host.service
    systemctl is-active --quiet sailry-host.service
  else
    install_unit="$HOME/.config/systemd/user/sailry-host.service"
    test ! -e "$install_unit"
    loginctl --no-ask-password enable-linger "$(id -un)"
    export XDG_RUNTIME_DIR="/run/user/$(id -u)"
    mkdir -p "$(dirname "$install_unit")"
    cat > "$install_unit" <<UNIT
[Unit]
Description=Sailry Host
After=network-online.target
[Service]
ExecStart="%h/.local/lib/sailry/sailry-host" --data-dir "%h/.sailry-host" $install_network --bootstrap
Restart=on-failure
RestartSec=3
UMask=0077
[Install]
WantedBy=default.target
UNIT
    systemctl --user daemon-reload
    systemctl --user enable --now sailry-host.service
    systemctl --user is-active --quiet sailry-host.service
  fi
else
  install_uid="$(id -u)"
  launchctl print "gui/$install_uid" >/dev/null
  install_unit="$HOME/Library/LaunchAgents/ai.sailry.host.plist"
  test ! -e "$install_unit"
  mkdir -p "$(dirname "$install_unit")"
  /usr/libexec/PlistBuddy -c 'Clear dict' "$install_unit"
  /usr/libexec/PlistBuddy -c 'Add :Label string ai.sailry.host' "$install_unit"
  /usr/libexec/PlistBuddy -c 'Add :ProgramArguments array' "$install_unit"
  /usr/libexec/PlistBuddy -c "Add :ProgramArguments: string $install_root/sailry-host" "$install_unit"
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
  launchctl bootstrap "gui/$install_uid" "$install_unit"
fi
