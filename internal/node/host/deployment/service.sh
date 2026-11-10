set -eu
umask 077
install_unit="$install_service_path"
install_unit_name="$(basename "$install_unit")"
if test "$install_update" = 1; then
  if test "$(uname -s)" = Linux; then
    if test "$(id -u)" = 0; then
      systemctl restart "$install_unit_name"
      systemctl is-active --quiet "$install_unit_name"
    else
      export XDG_RUNTIME_DIR="/run/user/$(id -u)"
      systemctl --user restart "$install_unit_name"
      systemctl --user is-active --quiet "$install_unit_name"
    fi
  else
    launchctl kickstart -k "gui/$(id -u)/ai.sailry.host"
  fi
  exit 0
fi
test ! -e "$install_unit"
test ! -L "$install_unit"
mkdir -p "$install_data" "$(dirname "$install_unit")"
chmod 700 "$install_data"
if test "$#" -eq 0; then set -- --internet; fi
if test "$(uname -s)" = Linux; then
  command -v systemctl >/dev/null
  install_network="$(python3 -I - "$@" <<'PY'
import sys
escapes = {'\\': '\\\\', '"': '\\"', '$': '$$', '%': '%%', '\n': '\\n', '\r': '\\r', '\t': '\\t'}
def quote(value):
    return '"' + ''.join(escapes.get(char, f'\\x{ord(char):02x}' if ord(char) < 32 else char) for char in value) + '"'
print(' '.join(quote(value) for value in sys.argv[1:]))
PY
)"
  install_program="$(python3 -I - "$install_root/sailry-host" "$install_data" <<'PY'
import sys
escapes = {'\\': '\\\\', '"': '\\"', '$': '$$', '%': '%%', '\n': '\\n', '\r': '\\r', '\t': '\\t'}
def quote(value):
    return '"' + ''.join(escapes.get(char, f'\\x{ord(char):02x}' if ord(char) < 32 else char) for char in value) + '"'
print(quote(sys.argv[1]) + ' --data-dir ' + quote(sys.argv[2]))
PY
)"
  install_target=default.target
  if test "$(id -u)" = 0; then
    install_target=multi-user.target
  else
    loginctl --no-ask-password enable-linger "$(id -un)"
    export XDG_RUNTIME_DIR="/run/user/$(id -u)"
  fi
  (set -C; cat > "$install_unit" <<UNIT
[Unit]
Description=Sailry Host
After=network-online.target
Wants=network-online.target
[Service]
ExecStart=$install_program $install_network --bootstrap
Restart=on-failure
RestartSec=3
UMask=0077
[Install]
WantedBy=$install_target
UNIT
  )
  if test "$(id -u)" = 0; then
    systemctl daemon-reload
    systemctl enable --now "$install_unit"
    systemctl is-active --quiet "$install_unit_name"
  else
    systemctl --user daemon-reload
    systemctl --user enable --now "$install_unit"
    systemctl --user is-active --quiet "$install_unit_name"
  fi
else
  install_uid="$(id -u)"
  launchctl print "gui/$install_uid" >/dev/null
  python3 -I - "$install_unit" "$install_root/sailry-host" "$install_data" "$@" <<'PY'
import os
import plistlib
import sys
path, binary, profile = sys.argv[1:4]
value = {
    'Label': 'ai.sailry.host',
    'ProgramArguments': [binary, '--data-dir', profile] + sys.argv[4:] + ['--bootstrap'],
    'RunAtLoad': True,
    'KeepAlive': {'SuccessfulExit': False},
    'Umask': 63,
    'StandardOutPath': os.path.join(profile, 'host.stdout.log'),
    'StandardErrorPath': os.path.join(profile, 'host.stderr.log'),
}
with open(path, 'xb') as output:
    output.write(plistlib.dumps(value))
PY
  launchctl bootstrap "gui/$install_uid" "$install_unit"
fi
