#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
platform=${1:?A platform is required}
target=${2:?A target is required}
cargo=$(rustup which --toolchain 1.98.0 cargo)
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0

case "$platform" in
    android)
        ndk=${ANDROID_NDK_HOME:?ANDROID_NDK_HOME is required}
        api=${SAILRY_ANDROID_API:-24}
        case "$(uname -s)" in
            Darwin) ndk_host=darwin-x86_64 ;;
            Linux) ndk_host=linux-x86_64 ;;
            *) printf '%s\n' 'Unsupported Android build host' >&2; exit 1 ;;
        esac
        case "$target" in
            arm64-v8a) architecture=arm64; rust_target=aarch64-linux-android; clang_target=aarch64-linux-android ;;
            armeabi-v7a) architecture=arm; rust_target=armv7-linux-androideabi; clang_target=armv7a-linux-androideabi ;;
            x86_64) architecture=amd64; rust_target=x86_64-linux-android; clang_target=x86_64-linux-android ;;
            *) printf '%s\n' 'Unsupported Android ABI' >&2; exit 1 ;;
        esac
        output="$root/.runtime/native/android-$architecture"
        jni="$root/.runtime/native/android-jni/$target"
        mkdir -p "$output" "$jni"
        compiler="$ndk/toolchains/llvm/prebuilt/$ndk_host/bin/$clang_target$api-clang"
        export CC="$compiler" AR="$ndk/toolchains/llvm/prebuilt/$ndk_host/bin/llvm-ar"
        export CARGO_TARGET_DIR="$output/cargo"
        export RUSTFLAGS="-C linker=$compiler -C link-arg=-Wl,-z,max-page-size=16384"
        "$cargo" build --locked --release --lib --manifest-path "$root/native/iroh/Cargo.toml" --target "$rust_target"
        cp "$CARGO_TARGET_DIR/$rust_target/release/libsailry_iroh.so" "$jni/libsailry_iroh.so"
        cd "$root"
        CGO_ENABLED=1 GOOS=android GOARCH="$architecture" GOARM=7 CGO_LDFLAGS="-Wl,-z,max-page-size=16384" \
            go build -trimpath -buildmode=c-shared -o "$output/libsailry_mobile.so" ./cmd/sailry-mobile
        cp "$output/libsailry_mobile.so" "$jni/libsailry_mobile.so"
        ;;
    ios)
        architecture=${3:?An Apple architecture is required}
        case "$target/$architecture" in
            iphoneos/arm64) go_arch=arm64; rust_target=aarch64-apple-ios; slice=ios-arm64 ;;
            iphonesimulator/arm64) go_arch=arm64; rust_target=aarch64-apple-ios-sim; slice=ios-simulator-arm64 ;;
            iphonesimulator/x86_64) go_arch=amd64; rust_target=x86_64-apple-ios; slice=ios-simulator-amd64 ;;
            *) printf '%s\n' 'Unsupported iOS slice' >&2; exit 1 ;;
        esac
        output="$root/.runtime/native/$slice"
        framework="$output/SailryIroh.framework"
        mkdir -p "$output" "$framework"
        export CARGO_TARGET_DIR="$output/cargo"
        export SDKROOT="$(xcrun --sdk "$target" --show-sdk-path)"
        export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}"
        "$cargo" build --locked --release --lib --manifest-path "$root/native/iroh/Cargo.toml" --target "$rust_target"
        cp "$CARGO_TARGET_DIR/$rust_target/release/libsailry_iroh.dylib" "$framework/SailryIroh"
        install_name_tool -id '@rpath/SailryIroh.framework/SailryIroh' "$framework/SailryIroh"
        cp "$root/cmd/sailry-mobile/SailryIroh-Info.plist" "$framework/Info.plist"
        export SAILRY_IOS_SDK="$target" SAILRY_IOS_ARCH="$architecture"
        export CC="$root/cmd/sailry-mobile/clang-ios.sh"
        cd "$root"
        CGO_ENABLED=1 GOOS=ios GOARCH="$go_arch" \
            go build -trimpath -buildmode=c-archive -o "$output/libsailry_mobile.a" ./cmd/sailry-mobile
        ;;
    *)
        printf '%s\n' 'Unsupported Sailry Mobile platform' >&2
        exit 1
        ;;
esac
