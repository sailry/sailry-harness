#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
case "$(uname -s)" in
    Darwin) platform=darwin; library=libsailry_iroh.dylib; archive=libsailry_iroh.a ;;
    Linux) platform=linux; library=libsailry_iroh.so; archive=libsailry_iroh.a ;;
    # Git Bash on Windows; the MSVC target names its outputs without a prefix.
    MINGW*|MSYS*) platform=windows; library=sailry_iroh.dll; archive=sailry_iroh.lib ;;
    *) printf '%s\n' 'Unsupported native build platform' >&2; exit 1 ;;
esac
case "$(uname -m)" in
    arm64|aarch64) architecture=arm64 ;;
    x86_64|amd64) architecture=amd64 ;;
    *) printf '%s\n' 'Unsupported native build architecture' >&2; exit 1 ;;
esac
output="$root/.runtime/native/$platform-$architecture"
mkdir -p "$output"
export CARGO_TARGET_DIR="$output/cargo"
cd "$root/native/iroh"
# SAILRY_GLIBC, on Linux, links the library against that GNU libc version
# through cargo-zigbuild instead of the build machine's own. Release builds
# set it to the floor the Host installer preflight admits.
build=release
if [ "$platform" = linux ] && [ -n "${SAILRY_GLIBC-}" ]; then
    triple=$(rustc -vV | sed -n 's/^host: //p')
    cargo zigbuild --locked --release --lib --target "$triple.$SAILRY_GLIBC"
    build="$triple/release"
else
    cargo build --locked --release --lib
fi
cp "$CARGO_TARGET_DIR/$build/$library" "$output/$library"
cp "$CARGO_TARGET_DIR/$build/$archive" "$output/$archive"
if [ "${1-}" = "--fixtures" ]; then
    cargo build --locked --release --features relay-fixture --bin relay-fixture
    fixture=relay-fixture
    if [ "$platform" = windows ]; then fixture=relay-fixture.exe; fi
    cp "$CARGO_TARGET_DIR/release/$fixture" "$output/$fixture"
fi
