#!/usr/bin/env bash
# Assembles the standalone Host archive of this machine's platform. Run from
# the repository root:
#
#   internal/node/host/deployment/archive.sh [-artifacts <dir>]
#
# -artifacts is the CI native job's directory holding the iroh library;
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

test -f go.mod && test -d internal/node/host/deployment/package || { echo 'run from the repository root' >&2; exit 1; }
artifacts=()
if test "${1:-}" = -artifacts; then
  test -d "${2:-}" || { echo '-artifacts needs a directory' >&2; exit 1; }
  artifacts=(-artifacts "$2")
  shift 2
fi
test $# -eq 0 || { echo 'usage: archive.sh [-artifacts <dir>]' >&2; exit 1; }

unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
case "$(uname -s)/$(uname -m)" in
  Linux/x86_64) platform=linux/amd64 triple=x86_64-unknown-linux-gnu ;;
  Linux/aarch64 | Linux/arm64) platform=linux/arm64 triple=aarch64-unknown-linux-gnu ;;
  Darwin/arm64) platform=darwin/arm64 triple=aarch64-apple-darwin ;;
  Darwin/x86_64) platform=darwin/amd64 triple=x86_64-apple-darwin ;;
  *) echo 'Host archives are built on Linux and macOS, amd64 or arm64' >&2; exit 1 ;;
esac
stage=.runtime/host/$triple
package=internal/node/host/deployment/package
rm -rf -- "$stage"
mkdir -p "$stage/host"

# The pinned runtimes from their official URLs and the iroh library; the Host
# ships no speech or capture library.
go run ./native/fetch -target "$platform" -host ${artifacts[@]+"${artifacts[@]}"} -bundle "$stage/host"

# A release build opens the release profile by default and loads the iroh
# library only from its package, against the seal written below.
telemetry=github.com/sailry/sailry-harness/internal/node/telemetry
flags="-w -X github.com/sailry/sailry-harness/internal/core.Default=~/.sailry"
revision=${SAILRY_REVISION:-$(git rev-parse HEAD)}
flags="$flags -X $telemetry.BuildRevision=$revision"
test -z "${SAILRY_SENTRY_DSN:-}" || flags="$flags -X $telemetry.BuildDSN=$SAILRY_SENTRY_DSN"
test -z "${SAILRY_TELEMETRY_KEY:-}" || flags="$flags -X $telemetry.BuildKey=$SAILRY_TELEMETRY_KEY"
go build -trimpath -ldflags "$flags" -o "$stage/host/sailry-host" ./cmd/sailry-host

# macOS: only Sailry's own code is signed. The pinned runtimes keep the bytes
# their locks record, which the Host verifies outside an application bundle.
if test "$(uname -s)" = Darwin; then
  signing=(--sign -)
  if test "${SAILRY_SIGNING:-ad-hoc}" = developer-id; then
    test -n "${APPLE_SIGNING_IDENTITY:-}" || { echo 'APPLE_SIGNING_IDENTITY is required' >&2; exit 1; }
    signing=(--sign "$APPLE_SIGNING_IDENTITY" --timestamp)
    test -z "${APPLE_SIGNING_KEYCHAIN:-}" || signing+=(--keychain "$APPLE_SIGNING_KEYCHAIN")
  fi
  codesign --force "${signing[@]}" "$stage/host/libsailry_iroh.dylib" "$stage/host/sailry-host"
  codesign --verify --strict "$stage/host/libsailry_iroh.dylib" "$stage/host/sailry-host"
fi
go run ./native/fetch -target "$platform" -host -seal "$stage/host"

# The installer accepts only an archive whose Host reports its release version.
version=$("$stage/host/sailry-host" --version)
version=${version#Sailry Host }
cp "$package/sailry" "$package/install-host.sh" "$package/service.sh" "$stage/host/"
chmod 755 "$stage/host/sailry"
bash internal/node/host/deployment/licenses.sh "$platform" "$triple" "$stage/host/licenses"
cp go.mod go.sum native/iroh/Cargo.lock "$stage/host/"
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
