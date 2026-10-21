#!/usr/bin/env bash
set -euo pipefail

# Keep the update publisher seed out of every unrelated packaging child.
task_update_signing_key="${SAILRY_UPDATE_SIGNING_KEY:-}"
unset SAILRY_UPDATE_SIGNING_KEY

# Package existing builds into a fresh directory; never replace an installed app.
task_profile="${1:-release}"
case "$task_profile" in debug|release) ;; *) echo 'Usage: bash scripts/package-macos.sh [debug|release] [desktop|host|all]' >&2; exit 1 ;; esac
task_component="${2:-all}"
task_desktop=1
task_host=1
case "$task_component" in
  desktop) task_host=0 ;;
  host) task_desktop=0; task_update_signing_key=; unset task_update_signing_key ;;
  all) ;;
  *) echo 'Component must be desktop, host or all' >&2; exit 1 ;;
esac
test "$(uname -s)" = Darwin
command -v jq >/dev/null
task_root="$(cd "$(dirname "$0")/.." && pwd)"
task_build="$task_root/target/$task_profile"
task_target="$(rustc -vV | awk '/^host: / {print $2}')"
task_manifest=desktop
if test "$task_desktop" = 0; then task_manifest=host; fi
task_version="$(awk -F '"' '/^version = "/ {print $2; exit}' "$task_root/apps/$task_manifest/Cargo.toml")"
task_signing="${SAILRY_SIGNING:-ad-hoc}"
case "$task_signing" in
  ad-hoc)
    test "${SAILRY_NOTARIZE:-0}" != 1
    task_update_signing_key=
    unset task_update_signing_key
    ;;
  developer-id)
    test -n "${APPLE_SIGNING_IDENTITY:-}"
    test -n "${APPLE_TEAM_ID:-}"
    test "${SAILRY_NOTARIZE:-0}" = 1
    if test "$task_desktop" = 1; then
      test -n "$task_update_signing_key"
      test -x "${SAILRY_RELEASE_TOOL:-$task_root/target/release/sailry-release}"
    fi
    ;;
  *) echo 'SAILRY_SIGNING must be ad-hoc or developer-id' >&2; exit 1 ;;
esac
test -n "$task_version"
if test "$task_desktop" = 1; then test -x "$task_build/sailry-desktop"; fi
if test "$task_host" = 1; then test -x "$task_build/sailry-host"; fi
mkdir -p "$task_root/dist"
task_output="$(mktemp -d "$task_root/dist/macos-$task_profile-XXXXXX")"
task_app="$task_output/Sailry.app"
task_package_args=()
task_python_outputs=()
task_metadata=()
# Developer ID signing covers the runtime later; other bundles sign it while staging.
task_ocu=(--target "$task_target" --cache "$task_root/target/ocu-helper-cache")
if test "$task_signing" != developer-id; then task_ocu+=(--sign -); fi
task_llama=(--target "$task_target" --cache "$task_root/target/llama-prebuilt" --source "$task_build")
if test "$task_signing" != developer-id; then task_llama+=(--sign -); fi
if test "$task_desktop" = 1; then
  mkdir -p "$task_app/Contents/MacOS" "$task_app/Contents/Resources"
  cp "$task_build/sailry-desktop" "$task_app/Contents/MacOS/sailry-desktop"
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/onnx-runtime.py" --target "$task_target" \
    --output "$task_app/Contents/MacOS" --source "$task_build" --cache "$task_root/target/sherpa-onnx-prebuilt" --speech
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/llama-runtime.py" "${task_llama[@]}" \
    --output "$task_app/Contents/MacOS"
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/ocu-helper.py" "${task_ocu[@]}" \
    --output "$task_app/Contents/Helpers"
  cp "$task_root/apps/desktop/macos/Info.plist" "$task_app/Contents/Info.plist"
  cp -R "$task_root/apps/desktop/macos/zh_CN.lproj" "$task_app/Contents/Resources/"
  task_apple_version="$(bash "$task_root/scripts/python.sh" "$task_root/scripts/package/versions.py" --apple "$task_version")"
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $task_apple_version" "$task_app/Contents/Info.plist"
  plutil -lint "$task_app/Contents/Info.plist"
  # Development and release bundles use the same generated application icon.
  cp "$task_root/assets/branding/Sailry.icns" "$task_app/Contents/Resources/Sailry.icns"
  bash "$task_root/scripts/package/licenses.sh" sailry-desktop "$task_target" "$task_app/Contents/Resources/licenses"
  task_package_args+=(--app "$task_app")
  task_python_outputs+=("$task_app/Contents/MacOS")
  task_metadata+=("$task_app/Contents/Resources")
fi
if test "$task_host" = 1; then
  mkdir -p "$task_output/host"
  cp "$task_build/sailry-host" "$task_output/host/sailry-host"
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/onnx-runtime.py" --target "$task_target" \
    --output "$task_output/host" --source "$task_build" --cache "$task_root/target/sherpa-onnx-prebuilt"
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/llama-runtime.py" "${task_llama[@]}" \
    --output "$task_output/host"
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/ocu-helper.py" "${task_ocu[@]}" \
    --output "$task_output/host/helpers"
  cp "$task_root/scripts/host-launchd.sh" "$task_output/host/host-launchd.sh"
  cp "$task_root/scripts/host-command.sh" "$task_output/host/sailry"
  cp "$task_root/scripts/install-host.sh" "$task_output/host/install-host.sh"
  cp "$task_root/crates/node-runtime/src/ssh/install/service.sh" "$task_output/host/service.sh"
  chmod 755 "$task_output/host/sailry"
  bash "$task_root/scripts/package/licenses.sh" sailry-host "$task_target" "$task_output/host/licenses"
  task_package_args+=(--host "$task_output/host")
  task_python_outputs+=("$task_output/host")
  task_metadata+=("$task_output/host")
fi
for task_python_output in "${task_python_outputs[@]}"; do
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/python-runtime.py" --target "$task_target" \
    --output "$task_python_output" --cache "$task_root/target/python-cache"
done
task_commit="$(git -C "$task_root" rev-parse HEAD)"
task_dirty=false
if test -n "$(git -C "$task_root" status --porcelain -- apps crates Cargo.toml Cargo.lock vendor scripts)"; then task_dirty=true; fi
for task_directory in "${task_metadata[@]}"; do
  jq -n --arg revision "$task_commit" --argjson dirty "$task_dirty" \
    --arg profile "$task_profile" --arg target "$task_target" --arg version "$task_version" \
    --arg rustc "$(rustc --version)" \
    '{version:1,source_revision:$revision,source_dirty:$dirty,profile:$profile,target:$target,application_version:$version,rustc:$rustc}' \
    > "$task_directory/build.json"
  cp "$task_root/Cargo.lock" "$task_directory/Cargo.lock"
done

# Sign inside out for distribution; ad hoc bundles remain available for development.
bash "$task_root/scripts/python.sh" "$task_root/scripts/package/sign-macos.py" \
  "${task_package_args[@]}" --target "$task_target" --verify-dependencies
if test "$task_signing" = developer-id; then
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/sign-macos.py" "${task_package_args[@]}" --target "$task_target"
else
  for task_python_output in "${task_python_outputs[@]}"; do
    bash "$task_root/scripts/python.sh" "$task_root/scripts/package/python-runtime.py" --target "$task_target" \
      --output "$task_python_output" --sign -
  done
  if test "$task_desktop" = 1; then
    for task_library in libonnxruntime.dylib libsherpa-onnx-c-api.dylib; do
      codesign --force --sign - "$task_app/Contents/MacOS/$task_library"
    done
    bash "$task_root/scripts/python.sh" "$task_root/scripts/package/onnx-runtime.py" --target "$task_target" \
      --output "$task_app/Contents/MacOS" --speech --finalize
    codesign --force --sign - "$task_app"
  fi
  if test "$task_host" = 1; then
    codesign --force --sign - "$task_output/host/libonnxruntime.dylib"
    codesign --force --sign - "$task_output/host/sailry-host"
    bash "$task_root/scripts/python.sh" "$task_root/scripts/package/onnx-runtime.py" --target "$task_target" \
      --output "$task_output/host" --finalize
  fi
fi
if test "$task_desktop" = 1; then
  codesign --verify --deep --strict "$task_app"
  "$task_app/Contents/MacOS/sailry-desktop" --help
fi
if test "$task_host" = 1; then
  codesign --verify --strict "$task_output/host/sailry-host"
  "$task_output/host/sailry-host" --help
fi
if test "${SAILRY_NOTARIZE:-0}" = 1; then
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/notarize-macos.py" "${task_package_args[@]}"
fi
if test "$task_desktop" = 1; then
  task_dmg="$task_output/Sailry-$task_target.dmg"
  bash "$task_root/scripts/python.sh" "$task_root/scripts/package/dmg.py" --app "$task_app" --output "$task_dmg" --signing "$task_signing"
  if test "${SAILRY_NOTARIZE:-0}" = 1; then
    bash "$task_root/scripts/python.sh" "$task_root/scripts/package/notarize-macos.py" --dmg "$task_dmg"
  fi
  # Automatic updates use the final notarized app ZIP; DMG remains the manual installer.
  task_zip_name="Sailry-$task_target.zip"
  task_zip="$task_output/$task_zip_name"
  task_unsigned="$task_output/.unsigned-$task_zip_name"
  ditto -c -k --norsrc --keepParent "$task_app" "$task_unsigned"
  if test "$task_signing" = developer-id; then
    task_release_tool="${SAILRY_RELEASE_TOOL:-$task_root/target/release/sailry-release}"
    test -x "$task_release_tool"
    test -n "${SAILRY_RELEASE_REPOSITORY:-}"
    test -n "${SAILRY_RELEASE_TAG:-}"
    SAILRY_UPDATE_SIGNING_KEY="$task_update_signing_key" \
      "$task_release_tool" sign-archive --input "$task_unsigned" --output "$task_zip" \
      --name "$task_zip_name" --keys "$task_root/release/update-public-keys.txt"
    task_update_signing_key=
    unset task_update_signing_key
    "$task_release_tool" describe --archive "$task_zip" --version "$task_version" \
      --target "$task_target" --minimum-system 13.0 \
      --repository "$SAILRY_RELEASE_REPOSITORY" --tag "$SAILRY_RELEASE_TAG" \
      --keys "$task_root/release/update-public-keys.txt" --output "$task_zip.update.json"
    python3 - "$task_unsigned" <<'PY'
import pathlib, sys
pathlib.Path(sys.argv[1]).unlink()
PY
  else
    mv "$task_unsigned" "$task_zip"
  fi
fi
if test "$task_host" = 1; then
  tar -czf "$task_output/sailry-host-$task_target.tar.gz" -C "$task_output/host" .
fi
(
  cd "$task_output"
  task_checksums=()
  if test "$task_desktop" = 1; then task_checksums+=(./*.dmg ./*.zip); fi
  if test "$task_host" = 1; then task_checksums+=(./*.tar.gz); fi
  if compgen -G '*.zip.update.json' >/dev/null; then
    task_checksums+=(./*.zip.update.json)
  fi
  shasum -a 256 "${task_checksums[@]}" > SHA256SUMS
)
printf 'Local package: %s\n' "$task_output"
