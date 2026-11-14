#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
platform=${PLATFORM_NAME:?PLATFORM_NAME is required}
architectures=${ARCHS:?ARCHS is required}
native=${SAILRY_NATIVE_DIR:?SAILRY_NATIVE_DIR is required}
for architecture in $architectures; do
    sh "$root/cmd/sailry-mobile/build.sh" ios "$platform" "$architecture"
done
# Xcode consumes target-wide products, including a universal simulator binary
# when ARCHS contains both architectures, rather than a CURRENT_ARCH slice.
for artifact in libsailry_mobile.a SailryIroh.framework/SailryIroh; do
    set --
    for architecture in $architectures; do
        case "$platform/$architecture" in
            iphoneos/arm64) slice=ios-arm64 ;;
            iphonesimulator/arm64) slice=ios-simulator-arm64 ;;
            iphonesimulator/x86_64) slice=ios-simulator-amd64 ;;
            *) printf '%s\n' 'Unsupported iOS slice' >&2; exit 1 ;;
        esac
        set -- "$@" "$root/.runtime/native/$slice/$artifact"
    done
    mkdir -p "$(dirname -- "$native/$artifact")"
    if [ "$#" -eq 1 ]; then
        cp "$1" "$native/$artifact"
    else
        xcrun lipo -create "$@" -output "$native/$artifact"
    fi
done
cp "$(dirname -- "$1")/Info.plist" "$native/SailryIroh.framework/Info.plist"
destination="$TARGET_BUILD_DIR/$FRAMEWORKS_FOLDER_PATH/SailryIroh.framework"
mkdir -p "$destination"
cp "$native/SailryIroh.framework/Info.plist" "$destination/Info.plist"
cp "$native/SailryIroh.framework/SailryIroh" "$destination/SailryIroh"
if [ "${CODE_SIGNING_ALLOWED:-YES}" != NO ] && [ -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]; then
    codesign --force --sign "$EXPANDED_CODE_SIGN_IDENTITY" "$destination"
fi
