#!/bin/bash
set -euo pipefail

case "${0##*/}" in
  uname) case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; *) exit 1 ;; esac ;;
  id) case "$1" in -u) echo 1001 ;; -un) echo fixture ;; *) exit 1 ;; esac ;;
  git) test "$*" = 'status --porcelain' ;;
  go)
    if test "$1" = -C; then shift 2; fi
    case "$1" in
      run)
        while test "$#" -gt 0; do
          case "$1" in
            -bundle)
              mkdir -p "$2/python/bin"
              cp "$SAILRY_INSTALL_FIXTURE/runtime-python" "$2/python/bin/python3.12"
              ;;
            -seal) echo '{}' > "$2/native-seal.json" ;;
          esac
          shift
        done
        ;;
      build)
        while test "$1" != -o; do shift; done
        cp "$SAILRY_INSTALL_FIXTURE/built-host" "$2"
        ;;
      list) ;;
      *) exit 1 ;;
    esac
    ;;
  jq) if test "$1" = length; then echo 0; else echo '[]'; fi ;;
  curl)
    destination=
    url=
    while test "$#" -gt 0; do
      case "$1" in
        --output) destination=$2; shift 2 ;;
        https://*) url=$1; shift ;;
        *) shift ;;
      esac
    done
    printf '%s\n' "$url" >> "$SAILRY_INSTALL_FIXTURE/downloads"
    case "$url" in
      https://api.github.com/repos/sailry/*/releases\?per_page=1)
        test -z "$destination"
        cat "$SAILRY_INSTALL_FIXTURE/releases.json"
        ;;
      https://api.github.com/repos/sailry/*/releases\?per_page=100\&page=*)
        test -z "$destination"
        cat "$SAILRY_INSTALL_FIXTURE/releases-${url##*=}.json"
        if test -f "$SAILRY_INSTALL_FIXTURE/mark-release-complete"; then
          : > "$SAILRY_INSTALL_FIXTURE/release-stream-complete"
        fi
        ;;
      https://github.com/sailry/*/releases/download/v*)
        test -n "$destination"
        cp "$SAILRY_INSTALL_FIXTURE/${url#https://github.com/}" "$destination"
        ;;
      *) exit 1 ;;
    esac
    ;;
  systemctl|loginctl) printf '%s %s\n' "${0##*/}" "$*" >> "$SAILRY_INSTALL_FIXTURE/services" ;;
  *) exit 1 ;;
esac
