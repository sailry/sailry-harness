set -eu
wsl_fail() { printf '%s\n' "$1" >&2; exit 1; }
test "$(uname -s)" = Linux || wsl_fail 'WSL Host requires Linux'
case "$(uname -m)" in x86_64|aarch64|arm64) ;; *) wsl_fail 'WSL Host requires x64 or arm64' ;; esac
wsl_libc="$(getconf GNU_LIBC_VERSION 2>/dev/null)" || wsl_fail 'WSL Host requires GNU libc 2.34 or later'
case "$wsl_libc" in 'glibc '*) ;; *) wsl_fail 'WSL Host requires GNU libc 2.34 or later' ;; esac
wsl_libc="${wsl_libc#glibc }"
wsl_major="${wsl_libc%%.*}"
wsl_minor="${wsl_libc#*.}"
wsl_minor="${wsl_minor%%.*}"
case "$wsl_major:$wsl_minor" in *[!0-9:]*|:*) wsl_fail 'GNU libc version could not be verified' ;; esac
if ! { test "$wsl_major" -gt 2 || { test "$wsl_major" -eq 2 && test "$wsl_minor" -ge 34; }; }; then
  wsl_fail 'WSL Host requires GNU libc 2.34 or later'
fi
test "$(cat /proc/1/comm)" = systemd || wsl_fail 'Enable systemd in this WSL2 distribution before installing Host'
command -v systemctl >/dev/null || wsl_fail 'systemctl is unavailable'
case "$(systemctl show --property=SystemState --value)" in running|degraded) ;; *) wsl_fail 'The systemd manager is not running' ;; esac
for wsl_tool in curl bash tar sha256sum awk; do
  command -v "$wsl_tool" >/dev/null || wsl_fail "Required command is unavailable: $wsl_tool"
done
wsl_root="$HOME/.local/lib/sailry"
wsl_command="$HOME/.local/bin/sailry"
wsl_unit="$HOME/.config/systemd/user/sailry-host.service"
if test "$(id -u)" = 0; then
  wsl_root=/usr/local/libexec/sailry
  wsl_command=/usr/local/bin/sailry
  wsl_unit=/etc/systemd/system/sailry-host.service
  if systemctl cat sailry-host.service >/dev/null 2>&1; then wsl_fail 'A Host service already exists; pair the existing Host instead'; fi
else
  export XDG_RUNTIME_DIR="/run/user/$(id -u)"
  systemctl --user show --property=Version --value >/dev/null || wsl_fail 'A working systemd user manager is required'
  if systemctl --user cat sailry-host.service >/dev/null 2>&1; then wsl_fail 'A Host service already exists; pair the existing Host instead'; fi
  command -v loginctl >/dev/null || wsl_fail 'loginctl is unavailable'
fi
for wsl_path in "$wsl_root" "$HOME/.sailry-host" "$wsl_command" "$wsl_unit"; do
  if test -e "$wsl_path" || test -L "$wsl_path"; then
    wsl_fail 'Host files or data already exist; pair the existing Host instead'
  fi
done
