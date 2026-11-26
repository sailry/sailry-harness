#!/usr/bin/env bash
# Packages the Windows x64 Desktop: installer and portable archive. The Go
# application cross-compiles, so this runs on Linux or macOS with NSIS
# (makensis), jq and zip installed. Run from the repository root:
#
#   cmd/sailry-desktop/windows/package.sh -artifacts <dir>
#
# -artifacts is the CI native job's directory of the Windows Turso, capture
# and app-local Microsoft C++ runtime, including checksums and notices.
#
# .runtime/package/out/windows-amd64 receives the application directory, the
# installer Sailry-x86_64-pc-windows-msvc-setup.exe and the portable
# Sailry-x86_64-pc-windows-msvc.zip, whose top folder is Sailry. Nothing is
# code signed.
#
# SAILRY_REVISION, SAILRY_SENTRY_DSN, SAILRY_TELEMETRY_KEY: telemetry build
# values, each optional.
set -euo pipefail

harness_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
package_root=$PWD
desktop=${SAILRY_DESKTOP_DIR:-cmd/sailry-desktop}
test -f go.mod && test -f "$desktop/mygo.json" || { echo 'run from the product repository root' >&2; exit 1; }
test "${1:-}" = -artifacts && test -d "${2:-}" && test $# -eq 2 || { echo 'usage: package.sh -artifacts <dir>' >&2; exit 1; }
command -v makensis >/dev/null || { echo 'makensis (NSIS) is required' >&2; exit 1; }
artifacts=$(cd "$2" && pwd)

unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0
platform=windows/amd64
triple=x86_64-pc-windows-msvc
out=$package_root/.runtime/package/out/windows-amd64
resources=$package_root/$desktop/resources/windows-amd64
installer=$out/Sailry-$triple-setup.exe
portable=$out/Sailry-$triple.zip

rm -f -- "$resources/sailry_iroh.dll"
go -C "$harness_root" run ./native/fetch -target "$platform" -artifacts "$artifacts" -bundle "$resources"
GOOS=windows GOARCH=amd64 go -C "$harness_root" build -trimpath -ldflags="-s -w -H=windowsgui" -o "$resources/sailry-update.exe" ./internal/desktop/updater/cmd/windows-install
go build -o .runtime/package/bin/mygo github.com/egoist/mygo/cmd/mygo

# Everything the package ships is staged before the build, because the build
# also writes the installer: the dependency notices, the build record and the
# seal the release build requires of its native libraries.
revision=${SAILRY_REVISION:-$(git rev-parse HEAD)}
version=$(jq -r .version "$desktop/mygo.json")
rm -rf -- "$resources/licenses"
bash "$harness_root/internal/node/host/deployment/licenses.sh" "$platform" "$triple" "$resources/licenses" "./$desktop" "$package_root"
dirty=false
if test -n "$(git status --porcelain)"; then dirty=true; fi
jq -n --arg revision "$revision" --argjson dirty "$dirty" --arg target "$triple" --arg version "$version" \
  '{version:1,source_revision:$revision,source_dirty:$dirty,profile:"release",target:$target,application_version:$version}' \
  > "$resources/build.json"
go -C "$harness_root" run ./native/fetch -target "$platform" -seal "$resources"

# The installer lists top-level entries. Include individual package files so
# future upgrades can also preserve local additions inside resource folders.
python3 - "$resources" <<'PYTHON'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
path = root / "build.json"
build = json.loads(path.read_text())
build["files"] = sorted({".mygo-files", "Sailry.exe"} | {
    entry.relative_to(root).as_posix() for entry in root.rglob("*") if entry.is_file()
})
path.write_text(json.dumps(build, separators=(",", ":")) + "\n")
PYTHON

flags="-X github.com/sailry/sailry-harness/internal/core.Default=~/.sailry"
flags+=" ${SAILRY_PACKAGE_LDFLAGS:-}"
source "$harness_root/internal/node/telemetry/build-flags.sh"
telemetry_build_flags "$revision"

rm -rf -- "$out"
MYGO_LDFLAGS=$flags .runtime/package/bin/mygo build -platform "$platform" "$desktop"
for name in Sailry.exe sailry-update.exe native-seal.json build.json; do
  test -f "$out/$name" || { echo "the package has no $name" >&2; exit 1; }
done
mv "$out/sailry-setup-$version-amd64.exe" "$installer"

# The portable archive is the application directory under the name Sailry.
stage=$(mktemp -d "${TMPDIR:-/tmp}/sailry-portable-XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
mkdir "$stage/Sailry"
(cd "$out" && find . -mindepth 1 -maxdepth 1 ! -name '*.zip' ! -name "Sailry-$triple-setup.exe" -exec cp -R {} "$stage/Sailry/" \;)
portable=$(cd "$out" && pwd)/Sailry-$triple.zip
(cd "$stage" && zip -q -r -X "$portable" Sailry)

if command -v sha256sum >/dev/null; then
  sha256sum "$installer" "$portable"
else
  shasum -a 256 "$installer" "$portable"
fi
