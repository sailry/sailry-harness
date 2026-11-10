#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
case "$(uname -s)" in
    Darwin) platform=darwin; suffix=dylib ;;
    Linux) platform=linux; suffix=so ;;
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
cargo build --locked --release --lib
cp "$CARGO_TARGET_DIR/release/libsailry_iroh.$suffix" "$output/libsailry_iroh.$suffix"
cp "$CARGO_TARGET_DIR/release/libsailry_iroh.a" "$output/libsailry_iroh.a"
if [ "${1-}" = "--fixtures" ]; then
    cargo build --locked --release --features relay-fixture --bin relay-fixture
    cp "$CARGO_TARGET_DIR/release/relay-fixture" "$output/relay-fixture"
fi
