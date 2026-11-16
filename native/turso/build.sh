#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
case "$(uname -s)" in
    Darwin) platform=darwin; library=libturso_sync_sdk_kit.dylib ;;
    Linux) platform=linux; library=libturso_sync_sdk_kit.so ;;
    MINGW*|MSYS*) platform=windows; library=turso_sync_sdk_kit.dll ;;
    *) printf '%s\n' 'Unsupported native build platform' >&2; exit 1 ;;
esac
case "$(uname -m)" in
    arm64|aarch64) architecture=arm64 ;;
    x86_64|amd64) architecture=amd64 ;;
    *) printf '%s\n' 'Unsupported native build architecture' >&2; exit 1 ;;
esac
output="$root/.runtime/native/$platform-$architecture"
mkdir -p "$output"
export CARGO_TARGET_DIR="$output/turso-cargo"
cd "$root/native/turso"
# Use the pinned rustup toolchain; an unrelated cargo earlier in PATH may
# change Cargo's output layout or ignore rust-toolchain.toml.
build=release
# Match the GNU libc floor used by the Host installer and the iroh library.
if [ "$platform" = linux ] && [ -n "${SAILRY_GLIBC-}" ]; then
    triple=$(rustc -vV | sed -n 's/^host: //p')
    cargo zigbuild --locked --release --lib --target "$triple.$SAILRY_GLIBC"
    build="$triple/release"
else
    cargo build --locked --release --lib
fi
cp "$CARGO_TARGET_DIR/$build/$library" "$output/$library"
