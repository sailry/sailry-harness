set -eu
umask 077
command -v curl >/dev/null
command -v bash >/dev/null
install_stage="$(mktemp -d "${TMPDIR:-/tmp}/sailry-bootstrap-XXXXXX")"
trap 'rm -rf -- "$install_stage"' EXIT
install_release="https://github.com/sailry/sailry-harness/releases/download/v$install_version"
for install_asset in install-host.sh SHA256SUMS-installer; do
  curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
    "$install_release/$install_asset" --output "$install_stage/$install_asset"
done
install_expected="$(awk '$2 == "install-host.sh" || $2 == "./install-host.sh" {print $1}' "$install_stage/SHA256SUMS-installer")"
test "${#install_expected}" = 64
case "$install_expected" in *[!0-9a-f]*) echo 'Invalid installer checksum' >&2; exit 1 ;; esac
if test "$(uname -s)" = Linux; then
  install_actual="$(sha256sum "$install_stage/install-host.sh")"
else
  install_actual="$(shasum -a 256 "$install_stage/install-host.sh")"
fi
if test "${install_actual%% *}" != "$install_expected"; then echo 'Installer checksum does not match' >&2; exit 1; fi
bash "$install_stage/install-host.sh" --version "$install_version"
