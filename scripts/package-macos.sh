#!/usr/bin/env bash
set -euo pipefail

# Package existing builds into a fresh directory; never replace an installed app.
task_profile="${1:-release}"
case "$task_profile" in debug|release) ;; *) echo 'Usage: bash scripts/package-macos.sh [debug|release]' >&2; exit 1 ;; esac
test "$(uname -s)" = Darwin
command -v jq >/dev/null
task_root="$(cd "$(dirname "$0")/.." && pwd)"
task_build="$task_root/target/$task_profile"
task_target="$(rustc -vV | awk '/^host: / {print $2}')"
task_version="$(awk -F '"' '/^version = "/ {print $2; exit}' "$task_root/apps/desktop/Cargo.toml")"
test -n "$task_version"
test -x "$task_build/sailry-desktop"
test -x "$task_build/sailry-host"
mkdir -p "$task_root/dist"
task_output="$(mktemp -d "$task_root/dist/macos-$task_profile-XXXXXX")"
task_app="$task_output/Sailry.app"
mkdir -p "$task_app/Contents/MacOS" "$task_app/Contents/Resources" "$task_output/host"
for task_host in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu "$task_target"; do
  if ! test -x "$task_root/target/host-artifacts/$task_host/sailry-host"; then
    echo 'Build bundled hosts with scripts/build-hosts.sh before packaging' >&2
    exit 1
  fi
  test -f "$task_root/target/host-artifacts/$task_host/office-runtime.tar.gz"
done
tar -xzf "$task_root/target/host-artifacts/$task_target/office-runtime.tar.gz" -C "$task_app/Contents/Resources"
cp -R "$task_app/Contents/Resources/office-runtime" "$task_output/host/office-runtime"
cp -R "$task_root/target/host-artifacts" "$task_app/Contents/Resources/hosts"
cp "$task_build/sailry-desktop" "$task_app/Contents/MacOS/sailry-desktop"
cp "$task_build/sailry-host" "$task_output/host/sailry-host"
cp "$task_root/scripts/host-launchd.sh" "$task_output/host/host-launchd.sh"
cp "$task_root/apps/desktop/macos/Info.plist" "$task_app/Contents/Info.plist"
cp -R "$task_root/apps/desktop/macos/zh_CN.lproj" "$task_app/Contents/Resources/"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $task_version" "$task_app/Contents/Info.plist"
plutil -lint "$task_app/Contents/Info.plist"

# Development and release bundles use the same generated application icon.
cp "$task_root/assets/branding/Sailry.icns" "$task_app/Contents/Resources/Sailry.icns"
bash "$task_root/scripts/package/licenses.sh" sailry-desktop "$task_target" "$task_app/Contents/Resources/licenses"
bash "$task_root/scripts/package/licenses.sh" sailry-host "$task_target" "$task_output/host/licenses"
task_commit="$(git -C "$task_root" rev-parse HEAD)"
task_dirty=false
if test -n "$(git -C "$task_root" status --porcelain -- apps crates Cargo.toml Cargo.lock vendor)"; then task_dirty=true; fi
jq -n --arg revision "$task_commit" --argjson dirty "$task_dirty" \
  --arg profile "$task_profile" --arg target "$task_target" --arg version "$task_version" \
  --arg rustc "$(rustc --version)" \
  '{version:1,source_revision:$revision,source_dirty:$dirty,profile:$profile,target:$target,application_version:$version,rustc:$rustc}' \
  > "$task_app/Contents/Resources/build.json"
cp "$task_root/Cargo.lock" "$task_app/Contents/Resources/Cargo.lock"
cp "$task_app/Contents/Resources/build.json" "$task_output/host/build.json"
cp "$task_root/Cargo.lock" "$task_output/host/Cargo.lock"

# Ad hoc signatures support local acceptance. Developer ID and notarization are separate.
codesign --force --sign - "$task_output/host/sailry-host"
codesign --force --sign - "$task_app"
codesign --verify --deep --strict "$task_app"
codesign --verify --strict "$task_output/host/sailry-host"
"$task_app/Contents/MacOS/sailry-desktop" --help
"$task_output/host/sailry-host" --help
ditto -c -k --keepParent "$task_app" "$task_output/Sailry-$task_version-$task_target.zip"
tar -czf "$task_output/sailry-host-$task_version-$task_target.tar.gz" -C "$task_output/host" .
(cd "$task_output" && shasum -a 256 ./*.zip ./*.tar.gz > SHA256SUMS)
printf 'Local package: %s\n' "$task_output"
