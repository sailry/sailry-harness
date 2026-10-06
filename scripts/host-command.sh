#!/usr/bin/env bash
set -euo pipefail

task_root="$HOME/.local/lib/sailry"
task_data="$HOME/.sailry-host"
task_command="${1:-help}"
if test "$#" -gt 0; then shift; fi
case "$task_command" in
  version) test "$#" = 0; exec "$task_root/sailry-host" --version ;;
  share) exec "$task_root/sailry-host" share --data-dir "$task_data" "$@" ;;
  update)
    # Use the installed, release-verified installer, not mutable remote code.
    bash "$task_root/install-host.sh" --update "$@"
    ;;
  start|stop|restart|status)
    test "$#" = 0
    if test "$(uname -s)" = Linux; then
      if test "$(id -u)" = 0; then
        exec systemctl "$task_command" sailry-host.service
      else
        export XDG_RUNTIME_DIR="/run/user/$(id -u)"
        exec systemctl --user "$task_command" sailry-host.service
      fi
    else
      task_domain="gui/$(id -u)"
      task_service="$task_domain/ai.sailry.host"
      task_unit="$HOME/Library/LaunchAgents/ai.sailry.host.plist"
      case "$task_command" in
        start) launchctl print "$task_service" >/dev/null 2>&1 || launchctl bootstrap "$task_domain" "$task_unit" ;;
        stop) launchctl bootout "$task_service" ;;
        restart) launchctl print "$task_service" >/dev/null 2>&1 || launchctl bootstrap "$task_domain" "$task_unit"; launchctl kickstart -k "$task_service" ;;
        status) launchctl print "$task_service" ;;
      esac
    fi
    ;;
  help|--help|-h) printf 'Usage: sailry <version|start|stop|restart|status|share|update>\n' ;;
  *) echo 'Unknown command; use sailry help' >&2; exit 1 ;;
esac
