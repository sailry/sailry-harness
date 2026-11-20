#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
platform=${PLATFORM_NAME:?PLATFORM_NAME is required}
architectures=${ARCHS:?ARCHS is required}
native=${SAILRY_NATIVE_DIR:?SAILRY_NATIVE_DIR is required}
# Xcode consumes target-wide products, including a universal simulator binary
# when ARCHS contains both architectures, rather than a CURRENT_ARCH slice.
set --
for architecture in $architectures; do
    sh "$root/cmd/sailry-mobile/build.sh" ios "$platform" "$architecture"
    case "$platform/$architecture" in
        iphoneos/arm64) slice=ios-arm64 ;;
        iphonesimulator/arm64) slice=ios-simulator-arm64 ;;
        iphonesimulator/x86_64) slice=ios-simulator-amd64 ;;
        *) printf '%s\n' 'Unsupported iOS slice' >&2; exit 1 ;;
    esac
    set -- "$@" "$root/.runtime/native/$slice/libsailry_mobile.a"
done
mkdir -p "$native"
if [ "$#" -eq 1 ]; then
    cp "$1" "$native/libsailry_mobile.a"
else
    xcrun lipo -create "$@" -output "$native/libsailry_mobile.a"
fi
