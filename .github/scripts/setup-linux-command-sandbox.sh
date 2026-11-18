#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install --yes bubblewrap netcat-openbsd apparmor "$@"
sysctl kernel.unprivileged_userns_clone user.max_user_namespaces kernel.apparmor_restrict_unprivileged_userns || true

# Keep the host's user namespace restriction enabled. Prefer the runner's
# packaged bubblewrap profile and supply one only where the package lacks it.
if [ "$(sysctl -n kernel.apparmor_restrict_unprivileged_userns 2>/dev/null || true)" = 1 ] &&
   [ "$(cat /sys/module/apparmor/parameters/enabled 2>/dev/null || true)" = Y ]; then
  profile=
  for candidate in /etc/apparmor.d/bwrap-userns-restrict /etc/apparmor.d/bwrap; do
    if [ -f "$candidate" ]; then profile=$candidate; break; fi
  done
  if [ -z "$profile" ]; then
    profile=/etc/apparmor.d/sailry-ci-bwrap
    sudo tee "$profile" >/dev/null <<'PROFILE'
abi <abi/4.0>,
/usr/bin/bwrap flags=(unconfined) {
  userns,
}
PROFILE
  fi
  sudo apparmor_parser -r "$profile"
fi

bwrap --die-with-parent --unshare-pid --unshare-net --new-session \
  --ro-bind / / --ro-bind-fd 3 /usr --proc /proc --dev /dev -- /bin/true 3</usr
