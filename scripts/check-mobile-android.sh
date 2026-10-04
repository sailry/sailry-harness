#!/usr/bin/env bash
set -euo pipefail
# Compile the minimal platform adapter without scaffolding a Mobile product UI.
: "${ANDROID_SDK_ROOT:?Set ANDROID_SDK_ROOT to an installed Android SDK}"
: "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to an installed Android NDK}"
task_api="${SAILRY_ANDROID_API:-35}"
task_android_jar="$ANDROID_SDK_ROOT/platforms/android-$task_api/android.jar"
test -f "$task_android_jar"
command -v javac >/dev/null
command -v jar >/dev/null
bash scripts/check-mobile-boundary.sh aarch64-linux-android sailry-mobile-bridge

task_output=$(mktemp -d /tmp/sailry-android-adapter.XXXXXX)
mkdir "$task_output/classes"
javac --release 8 -classpath "$task_android_jar" -d "$task_output/classes" \
  crates/mobile-bridge/android/src/main/java/ai/sailry/bridge/Native.java
jar cf "$task_output/sailry-android-init.jar" -C "$task_output/classes" .
javap -p -s -classpath "$task_output/sailry-android-init.jar" ai.sailry.bridge.Native

case "$(uname -s)" in
  Darwin) task_ndk_host=darwin-x86_64 ;;
  Linux) task_ndk_host=linux-x86_64 ;;
  *) echo "Unsupported NDK host" >&2; exit 1 ;;
esac
"$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$task_ndk_host/bin/llvm-nm" \
  --dynamic --defined-only target/aarch64-linux-android/debug/libsailry_mobile_bridge.so \
  | rg 'Java_ai_sailry_bridge_Native_init$'

# Locate the companion through the lockfile-resolved dependency, never a private path.
task_companion=$(cargo metadata --locked --format-version 1 --filter-platform aarch64-linux-android \
  | node -e 'let data=""; process.stdin.on("data", chunk => data += chunk); process.stdin.on("end", () => { const pkg=JSON.parse(data).packages.find(pkg => pkg.name === "rustls-platform-verifier-android"); if (!pkg) process.exit(1); process.stdout.write(require("node:path").join(require("node:path").dirname(pkg.manifest_path), "maven")); });')
test -d "$task_companion"
printf 'Android initializer compiled: %s\nCompanion Maven repository: %s\n' "$task_output/sailry-android-init.jar" "$task_companion"
echo "No Android device or lifecycle acceptance is implied."
