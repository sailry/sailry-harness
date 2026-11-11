#!/usr/bin/env bash
# Packages the macOS Desktop for release: build, sign, seal, notarize, disk
# image and update archive. Run from the repository root on the target
# architecture:
#
#   cmd/sailry-desktop/macos/package.sh [-artifacts <dir>]
#
# -artifacts is the CI native job's directory of iroh and capture libraries;
# without it the libraries already built in .runtime/native are used.
#
# .runtime/b14/out/darwin-<arch> receives Sailry.app, the disk image
# Sailry-<triple>.dmg and Sailry-<triple>-unsigned.zip, the archive of the
# finished application that the update feed command signs.
#
# The environment selects signing, with the names the release settings use:
#
#   SAILRY_SIGNING=ad-hoc (default) or developer-id
#   developer-id: APPLE_SIGNING_IDENTITY, APPLE_TEAM_ID and, for a temporary
#     keychain, APPLE_SIGNING_KEYCHAIN
#   SAILRY_NOTARIZE=1 (developer-id only): APPLE_API_KEY_PATH,
#     APPLE_API_KEY_ID, APPLE_API_ISSUER_ID
#   SAILRY_REVISION, SAILRY_SENTRY_DSN, SAILRY_TELEMETRY_KEY: telemetry build
#     values, each optional
#
# The package opens the release profile ~/.sailry. Nothing here runs it.
set -euo pipefail

test "$(uname -s)" = Darwin || { echo 'package.sh needs macOS' >&2; exit 1; }
test -f go.mod && test -f cmd/sailry-desktop/mygo.json || { echo 'run from the repository root' >&2; exit 1; }

artifacts=()
if test "${1:-}" = -artifacts; then
  test -d "${2:-}" || { echo '-artifacts needs a directory' >&2; exit 1; }
  artifacts=(-artifacts "$2")
  shift 2
fi
test $# -eq 0 || { echo 'usage: package.sh [-artifacts <dir>]' >&2; exit 1; }

signing=${SAILRY_SIGNING:-ad-hoc}
notarize=${SAILRY_NOTARIZE:-0}
case "$signing" in
  ad-hoc)
    test "$notarize" != 1 || { echo 'SAILRY_NOTARIZE needs SAILRY_SIGNING=developer-id' >&2; exit 1; }
    identity=-
    ;;
  developer-id)
    test -n "${APPLE_SIGNING_IDENTITY:-}" && test -n "${APPLE_TEAM_ID:-}" ||
      { echo 'APPLE_SIGNING_IDENTITY and APPLE_TEAM_ID are required' >&2; exit 1; }
    identity=$APPLE_SIGNING_IDENTITY
    ;;
  *) echo 'SAILRY_SIGNING must be ad-hoc or developer-id' >&2; exit 1 ;;
esac
credentials=()
if test "$notarize" = 1; then
  test -f "${APPLE_API_KEY_PATH:-}" && test -n "${APPLE_API_KEY_ID:-}" && test -n "${APPLE_API_ISSUER_ID:-}" ||
    { echo 'APPLE_API_KEY_PATH, APPLE_API_KEY_ID and APPLE_API_ISSUER_ID are required' >&2; exit 1; }
  credentials=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID")
fi

unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
case "$(uname -m)" in
  arm64) arch=arm64 triple=aarch64-apple-darwin ;;
  x86_64) arch=amd64 triple=x86_64-apple-darwin ;;
  *) echo 'unsupported architecture' >&2; exit 1 ;;
esac
platform=darwin/$arch
out=.runtime/b14/out/darwin-$arch
app=$out/Sailry.app
image=$out/Sailry-$triple.dmg
update=$out/Sailry-$triple-unsigned.zip
resources=cmd/sailry-desktop/resources/darwin-$arch

# codesign finds the identity of a temporary keychain through the search
# list, which is restored when the script ends.
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

go run ./native/fetch -target "$platform" ${artifacts[@]+"${artifacts[@]}"} -bundle "$resources"
go build -o .runtime/b14/bin/mygo github.com/egoist/mygo/cmd/mygo

# Shipped beside the runtimes: the dependency notices and the build record
# the updater requires of every package it installs.
revision=${SAILRY_REVISION:-$(git rev-parse HEAD)}
version=$(jq -r .version cmd/sailry-desktop/mygo.json)
rm -rf -- "$resources/licenses"
bash internal/node/host/deployment/licenses.sh "$platform" "$triple" "$resources/licenses" ./cmd/sailry-desktop
dirty=false
if test -n "$(git status --porcelain)"; then dirty=true; fi
jq -n --arg revision "$revision" --argjson dirty "$dirty" --arg target "$triple" --arg version "$version" \
  '{version:1,source_revision:$revision,source_dirty:$dirty,profile:"release",target:$target,application_version:$version}' \
  > "$resources/build.json"

telemetry=github.com/sailry/sailry-harness/internal/node/telemetry
flags="-X github.com/sailry/sailry-harness/internal/core.Default=~/.sailry"
flags="$flags -X $telemetry.BuildRevision=$revision"
test -z "${SAILRY_SENTRY_DSN:-}" || flags="$flags -X $telemetry.BuildDSN=$SAILRY_SENTRY_DSN"
test -z "${SAILRY_TELEMETRY_KEY:-}" || flags="$flags -X $telemetry.BuildKey=$SAILRY_TELEMETRY_KEY"

rm -rf -- "$app" "$image" "$update"
# mygo signs nested code and the bundle. Notarization and the disk image
# follow the seal, which changes the bundle after mygo has signed it.
MYGO_LDFLAGS=$flags .runtime/b14/bin/mygo build -platform "$platform" -sign "$identity" -skip-notarize -skip-dmg cmd/sailry-desktop
test -d "$app"

# mygo names the executable after the product. The updater, like every
# release before this one, identifies the application by sailry-desktop.
mv "$app/Contents/MacOS/Sailry" "$app/Contents/MacOS/sailry-desktop"
/usr/libexec/PlistBuddy -c 'Set :CFBundleExecutable sailry-desktop' "$app/Contents/Info.plist"

# Signing rewrote the native libraries and the rename changed the bundle:
# record the libraries as shipped and sign the bundle again over the records.
go run ./native/fetch -target "$platform" -seal "$app" -sign "$identity"

codesign --verify --deep --strict "$app"
if test "$signing" = developer-id; then
  find "$app" -type f \( -perm -u+x -o -name '*.dylib' -o -name '*.so' \) -print0 |
    while IFS= read -r -d '' file; do
      file -b "$file" | grep -q 'Mach-O' || continue
      details=$(codesign --display --verbose=4 "$file" 2>&1)
      case "$details" in
        *"TeamIdentifier=$APPLE_TEAM_ID"*) ;;
        *) echo "unexpected signing team: $file" >&2; exit 1 ;;
      esac
      case "$details" in
        *'(runtime)'*Timestamp=* | *Timestamp=*'(runtime)'*) ;;
        *) echo "missing hardened runtime or secure timestamp: $file" >&2; exit 1 ;;
      esac
    done
fi

submit() {
  result=$(xcrun notarytool submit "$1" "${credentials[@]}" --wait --timeout 45m --output-format json)
  case "$result" in
    *'"status":"Accepted"'* | *'"status": "Accepted"'*) ;;
    *) echo "notarization did not succeed: $result" >&2; exit 1 ;;
  esac
}

if test "$notarize" = 1; then
  archive=$out/Sailry-notarize.zip
  rm -f -- "$archive"
  ditto -c -k --keepParent "$app" "$archive"
  submit "$archive"
  rm -f -- "$archive"
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  codesign --verify --deep --strict "$app"
  spctl --assess --type execute --verbose=2 "$app"
fi

stage=$(mktemp -d "$out/.dmg-XXXXXX")
cp -R "$app" "$stage/Sailry.app"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname Sailry -srcfolder "$stage" -fs HFS+ -format UDZO "$image"
rm -rf -- "$stage"
if test "$signing" = developer-id; then
  codesign --sign "$identity" --timestamp --identifier ai.sailry.disk-image "$image"
  codesign --verify --strict "$image"
fi
hdiutil verify -quiet "$image"
if test "$notarize" = 1; then
  submit "$image"
  xcrun stapler staple "$image"
  xcrun stapler validate "$image"
  spctl --assess --type open --context context:primary-signature "$image"
fi

# The update archive holds the application exactly as the disk image does.
ditto -c -k --norsrc --keepParent "$app" "$update"

shasum -a 256 "$image" "$update"
