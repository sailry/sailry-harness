#!/bin/sh
set -eu

root=$(CDPATH= cd -- "${SAILRY_HARNESS_ROOT:?SAILRY_HARNESS_ROOT is required}" && pwd -P)
native=${SAILRY_NATIVE_DIR:?SAILRY_NATIVE_DIR is required}
builder="$root/cmd/sailry-mobile/build.sh"
if [ ! -f "$builder" ]; then
    printf 'Sailry native builder is missing: %s\n' "$builder" >&2
    exit 1
fi
checkout=$(git --no-pager -C "$root" rev-parse --show-toplevel)
if [ "$root" != "$(CDPATH= cd -- "$checkout" && pwd -P)" ]; then
    printf 'Sailry Harness root is not a checkout root: %s\n' "$root" >&2
    exit 1
fi
revision=$(git --no-pager -C "$root" rev-parse --verify HEAD)
if [ "$revision" != "${SAILRY_HARNESS_REVISION:-$revision}" ]; then
    printf 'Harness revision mismatch in %s: expected %s, found %s\n' \
        "$root" "$SAILRY_HARNESS_REVISION" "$revision" >&2
    exit 1
fi
printf 'Building Sailry native from %s (%s) into %s\n' "$root" "$revision" "$native"

platform=${1:?A platform is required}
target=${2:?A target is required}
shift 2
case "$platform" in
    android)
        sh "$builder" android "$target"
        source="$root/.runtime/native/android-mobile-jni/$target/libsailry_mobile.so"
        product="$native/$target/libsailry_mobile.so"
        if [ ! "$source" -ef "$product" ]; then
            mkdir -p "$native/$target"
            cp "$source" "$product"
        fi
        ;;
    ios)
        [ "$#" -gt 0 ] || { printf '%s\n' 'An iOS architecture is required' >&2; exit 1; }
        # Xcode links a target-wide archive, with all requested simulator slices.
        for architecture do
            case "$target/$architecture" in
                iphoneos/arm64) slice=ios-arm64 ;;
                iphonesimulator/arm64) slice=ios-simulator-arm64 ;;
                iphonesimulator/x86_64) slice=ios-simulator-amd64 ;;
                *) printf '%s\n' 'Unsupported iOS slice' >&2; exit 1 ;;
            esac
            sh "$builder" ios "$target" "$architecture"
            shift
            set -- "$@" "$root/.runtime/native/$slice/libsailry_mobile.a"
        done
        mkdir -p "$native"
        if [ "$#" -eq 1 ]; then
            cp "$1" "$native/libsailry_mobile.a"
        else
            xcrun lipo -create "$@" -output "$native/libsailry_mobile.a"
        fi
        ;;
    *) printf '%s\n' 'Unsupported Sailry Mobile platform' >&2; exit 1 ;;
esac
