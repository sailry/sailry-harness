#!/usr/bin/env bash
# Assembles the standalone Host archive of this machine's platform. Run from
# the repository root:
#
#   internal/node/host/deployment/archive.sh [-artifacts <dir>]
#
# -artifacts is the CI native job's directory holding the Turso library;
# without it the library built in .runtime/native is used. The result is
# .runtime/host/<triple>/sailry-host-<triple>.tar.gz with SHA256SUMS-<triple>.
#
# Environment, with the names the release settings use:
#
#   SAILRY_REVISION, SAILRY_SENTRY_DSN, SAILRY_TELEMETRY_KEY: telemetry build
#     values, each optional
#   macOS: SAILRY_SIGNING=ad-hoc (default) or developer-id with
#     APPLE_SIGNING_IDENTITY and, for a temporary keychain,
#     APPLE_SIGNING_KEYCHAIN
set -euo pipefail

harness_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
package_root=$PWD
command=${SAILRY_HOST_COMMAND:-./cmd/sailry-host}
test -f go.mod && test -d "$command" || { echo 'run from the product repository root' >&2; exit 1; }
artifacts=()
if test "${1:-}" = -artifacts; then
  test -d "${2:-}" || { echo '-artifacts needs a directory' >&2; exit 1; }
  artifacts=(-artifacts "$(cd "$2" && pwd)")
  shift 2
fi
test $# -eq 0 || { echo 'usage: archive.sh [-artifacts <dir>]' >&2; exit 1; }
case "${SAILRY_RELEASE_REPOSITORY:-https://github.com/sailry/sailry-harness}" in
  https://github.com/sailry/sailry-harness | https://github.com/sailry/sailry-code) ;;
  *) echo 'Official Host packages only support Harness and Code release repositories; forks need an independent package identity' >&2; exit 1 ;;
esac

unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
case "$(uname -s)/$(uname -m)" in
  Linux/x86_64) platform=linux/amd64 triple=x86_64-unknown-linux-gnu ;;
  Linux/aarch64 | Linux/arm64) platform=linux/arm64 triple=aarch64-unknown-linux-gnu ;;
  Darwin/arm64) platform=darwin/arm64 triple=aarch64-apple-darwin ;;
  Darwin/x86_64) platform=darwin/amd64 triple=x86_64-apple-darwin ;;
  *) echo 'Host archives are built on Linux and macOS, amd64 or arm64' >&2; exit 1 ;;
esac
stage=$package_root/.runtime/host/$triple
package=$harness_root/internal/node/host/deployment/package
rm -rf -- "$stage"
mkdir -p "$stage/host"

# The pinned runtimes from their official URLs and the Turso library; the Host
# ships no speech or capture library.
go -C "$harness_root" run ./native/fetch -target "$platform" -host ${artifacts[@]+"${artifacts[@]}"} -bundle "$stage/host"

# A release build loads Turso only from its package, against the seal written
# below. The command's product identity selects its default profile.
flags="-w -X github.com/sailry/sailry-harness/internal/core.Default=~/.sailry"
flags+=" ${SAILRY_PACKAGE_LDFLAGS:-}"
revision=${SAILRY_REVISION:-$(git rev-parse HEAD)}
source "$harness_root/internal/node/telemetry/build-flags.sh"
telemetry_build_flags "$revision"
go build -trimpath -ldflags "$flags" -o "$stage/host/sailry-host" "$command"

# macOS: only Sailry's own code is signed. The pinned runtimes keep the bytes
# their locks record, which the Host verifies outside an application bundle.
if test "$(uname -s)" = Darwin; then
  # --keychain limits identity lookup; its certificate chain still needs the
  # user search list. Keep the standalone Host signer self-contained.
  if test -n "${APPLE_SIGNING_KEYCHAIN:-}"; then
    previous=()
    while IFS= read -r line; do
      line=${line#"${line%%[![:space:]]*}"}
      line=${line#\"}
      previous+=("${line%\"}")
    done < <(security list-keychains -d user)
    restore() { security list-keychains -d user -s ${previous[@]+"${previous[@]}"}; }
    trap restore EXIT
    security list-keychains -d user -s "$APPLE_SIGNING_KEYCHAIN" ${previous[@]+"${previous[@]}"}
  fi
  signing=(--sign -)
  if test "${SAILRY_SIGNING:-ad-hoc}" = developer-id; then
    test -n "${APPLE_SIGNING_IDENTITY:-}" || { echo 'APPLE_SIGNING_IDENTITY is required' >&2; exit 1; }
    signing=(--sign "$APPLE_SIGNING_IDENTITY" --timestamp)
    test -z "${APPLE_SIGNING_KEYCHAIN:-}" || signing+=(--keychain "$APPLE_SIGNING_KEYCHAIN")
  fi
  codesign --force "${signing[@]}" "$stage/host/libturso_sync_sdk_kit.dylib" "$stage/host/sailry-host"
  codesign --verify --strict "$stage/host/libturso_sync_sdk_kit.dylib" "$stage/host/sailry-host"
fi
go -C "$harness_root" run ./native/fetch -target "$platform" -host -seal "$stage/host"

# The installer accepts only an archive whose Host reports its release version.
version=$("$stage/host/sailry-host" --version)
version=${version#Sailry Host }
cp "$package/sailry" "$package/install-host.sh" "$package/service.sh" "$package/select-host-release.py" "$stage/host/"
if test -n "${SAILRY_RELEASE_REPOSITORY:-}"; then
  SAILRY_RELEASE_REPOSITORY=$SAILRY_RELEASE_REPOSITORY python3 - "$stage/host/install-host.sh" "$version" <<'PY'
import os
from pathlib import Path
import shlex
import sys

path = Path(sys.argv[1])
source = path.read_text()
repository = os.environ["SAILRY_RELEASE_REPOSITORY"]
previous = "task_repository=https://github.com/sailry/sailry-harness\n"
if source.count(previous) != 1:
    raise SystemExit("Host installer repository declaration is missing or ambiguous")
source = source.replace(previous, f"task_repository={repository}\n", 1)
if repository == "https://github.com/sailry/sailry-code":
    previous = 'task_default_version=""\n'
    if source.count(previous) != 1:
        raise SystemExit("Host installer default version declaration is missing or ambiguous")
    source = source.replace(previous, f"task_default_version={shlex.quote(sys.argv[2])}\n", 1)
path.write_text(source)
PY
fi
chmod 755 "$stage/host/sailry"
bash "$harness_root/internal/node/host/deployment/licenses.sh" "$platform" "$triple" "$stage/host/licenses" "$command" "$package_root"
cp go.mod go.sum "$stage/host/"
dirty=false
if test -n "$(git status --porcelain)"; then dirty=true; fi
jq -n --arg revision "$revision" --argjson dirty "$dirty" --arg target "$triple" --arg version "$version" \
  '{version:1,source_revision:$revision,source_dirty:$dirty,profile:"release",target:$target,application_version:$version}' \
  > "$stage/host/build.json"

# Directory entries are written last so tar restores read-only modes after contents.
(cd "$stage/host" && find . -depth -print0 | COPYFILE_DISABLE=1 tar --no-acls --no-xattrs --no-recursion --null \
  -czf "../sailry-host-$triple.tar.gz" -T -)
if command -v sha256sum >/dev/null; then
  (cd "$stage" && sha256sum "./sailry-host-$triple.tar.gz" > "SHA256SUMS-$triple")
else
  (cd "$stage" && shasum -a 256 "./sailry-host-$triple.tar.gz" > "SHA256SUMS-$triple")
fi
cat "$stage/SHA256SUMS-$triple"
printf 'Sailry Host %s: %s\n' "$version" "$stage/sailry-host-$triple.tar.gz"
