#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
platform=${PLATFORM_NAME:?PLATFORM_NAME is required}
architectures=${ARCHS:?ARCHS is required}
first=
set --
for architecture in $architectures; do
    sh "$root/cmd/sailry-mobile/build.sh" ios "$platform" "$architecture"
    case "$platform/$architecture" in
        iphoneos/arm64) slice=ios-arm64 ;;
        iphonesimulator/arm64) slice=ios-simulator-arm64 ;;
        iphonesimulator/x86_64) slice=ios-simulator-amd64 ;;
        *) printf '%s\n' 'Unsupported iOS slice' >&2; exit 1 ;;
    esac
    framework="$root/.runtime/native/$slice/SailryIroh.framework"
    if [ -z "$first" ]; then
        first="$framework"
    fi
    set -- "$@" "$framework/SailryIroh"
done
destination="$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH/SailryIroh.framework"
mkdir -p "$destination"
cp "$first/Info.plist" "$destination/Info.plist"
if [ "$#" -eq 1 ]; then
    cp "$first/SailryIroh" "$destination/SailryIroh"
else
    xcrun lipo -create "$@" -output "$destination/SailryIroh"
fi
if [ "${CODE_SIGNING_ALLOWED:-YES}" != NO ] && [ -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]; then
    codesign --force --sign "$EXPANDED_CODE_SIGN_IDENTITY" "$destination"
fi
