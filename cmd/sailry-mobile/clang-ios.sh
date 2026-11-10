#!/bin/sh
set -eu

sdk=${SAILRY_IOS_SDK:?SAILRY_IOS_SDK is required}
architecture=${SAILRY_IOS_ARCH:?SAILRY_IOS_ARCH is required}
version=${IPHONEOS_DEPLOYMENT_TARGET:-15.0}
case "$sdk" in
    iphoneos) target="$architecture-apple-ios$version" ;;
    iphonesimulator) target="$architecture-apple-ios$version-simulator" ;;
    *) printf '%s\n' 'Unsupported Apple SDK' >&2; exit 1 ;;
esac
exec xcrun --sdk "$sdk" clang -target "$target" -isysroot "$(xcrun --sdk "$sdk" --show-sdk-path)" "$@"
