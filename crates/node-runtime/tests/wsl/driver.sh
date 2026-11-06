#!/bin/sh
set -eu
test "$#" = 7
test "$1" = --distribution
test "$2" = 'Ubuntu 资料'
test "$3" = --cd
test "$4" = '~'
test "$5" = --exec
test "$6" = /bin/sh
test "$7" = -s
HOME="$(cd -P -- "${0%/*}" && pwd)"
export HOME
PATH="$HOME/.fixture/bin:/usr/bin:/bin"
TMPDIR="$HOME/tmp"
export PATH TMPDIR
source="$(cat)"
case "$source" in
  *install_version=*)
    # The WSL transport is a fixture; release verification and Link pairing remain real.
    script="$(printf '%s\n' "$source" | sed -n '/^install_version=/,$p')"
    exec /bin/sh -c "$script"
    ;;
  *bootstrap.ticket*) exec /bin/sh -c "$source" ;;
  *)
    printf detecting > "$HOME/detecting"
    if test "$(cat "$HOME/mode")" = wait-detect; then sleep 30; fi
    if test -e "$HOME/.sailry-host"; then echo 'Host data already exist' >&2; exit 1; fi
    ;;
esac
