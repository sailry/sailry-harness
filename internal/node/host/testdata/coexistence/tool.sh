#!/bin/sh
set -eu
case "${0##*/}" in
  id)
    case "$1" in -u) echo 1001 ;; -un) echo fixture ;; *) exit 1 ;; esac
    ;;
  curl)
    while test "$#" -gt 0; do
      case "$1" in
        https://github.com/*) url=$1; shift ;;
        --output) destination=$2; shift 2 ;;
        *) shift ;;
      esac
    done
    cp "$SAILRY_FIXTURE/${url#https://github.com/}" "$destination"
    printf '%s\n' "$url" >> "$SAILRY_FIXTURE/downloads"
    ;;
  systemctl)
    printf '%s\n' "$*" >> "$SAILRY_FIXTURE/service-log"
    if test "$1" = --user; then shift; fi
    case "$*" in
      'show --property=SystemState --value') echo running; exit ;;
      'show --property=Version --value') echo 255; exit ;;
      daemon-reload) exit ;;
    esac
    action=$1
    for unit do :; done
    case "$unit" in sailry-host.service|fixture-host.service) ;; *) exit 1 ;; esac
    state="$SAILRY_FIXTURE/$unit.active"
    service="$HOME/.config/systemd/user/$unit"
    case "$action" in
      cat) test -f "$service" ;;
      enable|start) test -f "$service"; : > "$state" ;;
      status|is-active) test -f "$state" ;;
      disable|stop) rm -f "$state" ;;
      *) exit 1 ;;
    esac
    ;;
  launchctl)
    printf '%s\n' "$*" >> "$SAILRY_FIXTURE/service-log"
    action=$1
    if test "$*" = 'print gui/1001'; then exit; fi
    if test "$action" = bootstrap; then
      test -f "$3"
      agent=${3##*/}; agent=${agent%.plist}
    else
      agent=${2##*/}
    fi
    case "$agent" in ai.sailry.host|org.fixture.host) ;; *) exit 1 ;; esac
    state="$SAILRY_FIXTURE/$agent.active"
    case "$action" in
      bootstrap) : > "$state" ;;
      print) test -f "$state" || exit 3 ;;
      bootout) rm -f "$state" ;;
      *) exit 1 ;;
    esac
    ;;
  loginctl) test "$*" = '--no-ask-password enable-linger fixture' ;;
  sha256sum)
    if test -x /usr/bin/sha256sum; then exec /usr/bin/sha256sum "$@"; fi
    exec /usr/bin/shasum -a 256 "$@"
    ;;
  getconf)
    test "$*" = GNU_LIBC_VERSION
    echo 'glibc 2.39'
    ;;
  cat)
    if test "$*" = /proc/1/comm; then echo systemd; else exec /bin/cat "$@"; fi
    ;;
  *) exit 1 ;;
esac
