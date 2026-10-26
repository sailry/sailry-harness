set dotenv-path := ".runtime/dev.env"

desktop data_dir=env_var_or_default("SAILRY_DESKTOP_DATA_DIR", ""):
    if test -n {{quote(data_dir)}}; then bash scripts/run-desktop.sh --data-dir {{quote(data_dir)}}; else bash scripts/run-desktop.sh; fi

preview:
    bash scripts/run-desktop.sh --preview

[positional-arguments]
ci *args:
    bash scripts/python.sh scripts/ci.py local "$@"

check:
    bash scripts/python.sh scripts/ci.py check format
    bash scripts/python.sh scripts/ci.py check lint

test-ui:
    bash scripts/python.sh scripts/ci.py check desktop

test-source:
    bash scripts/python.sh scripts/ci.py check source
    bash scripts/python.sh scripts/ci.py check desktop-source
    bash scripts/python.sh scripts/ci.py check host-source
    bash scripts/python.sh scripts/ci.py check mobile-source

test-plugins:
    bash scripts/python.sh scripts/test-plugins.py

host data_dir="":
    #!/usr/bin/env bash
    set -euo pipefail
    task_triple="$(rustc -vV | awk '/^host: / {print $2}')"
    task_parsers=(--target "$task_triple" --output "${CARGO_TARGET_DIR:-target}/debug/code-parsers")
    if [[ "$task_triple" == *apple-darwin ]]; then task_parsers+=(--sign -); fi
    TSLP_SOURCE_BUNDLE_URL="$(bash scripts/python.sh scripts/package/code-parsers.py "${task_parsers[@]}")"
    export TSLP_SOURCE_BUNDLE_URL
    unset TSLP_OFFLINE TSLP_LANGUAGES
    if test -n {{quote(data_dir)}}; then cargo run --locked -p sailry-host -- --data-dir {{quote(data_dir)}}; else cargo run --locked -p sailry-host; fi

test-backend:
    bash scripts/python.sh scripts/ci.py check backend

# List phones and running simulators recognized by Flutter.
mobile-devices:
    flutter devices

# Fetch the pinned Flutter dependencies, including the shared Rust bridge package.
mobile-deps:
    cd apps/mobile && flutter pub get --enforce-lockfile

# Run the mobile app; native assets build the shared Rust libraries automatically.
mobile device="": mobile-deps
    #!/usr/bin/env bash
    set -euo pipefail
    cd apps/mobile
    if test -n {{quote(device)}}; then
        exec flutter run --no-pub --target lib/main.dart -d {{quote(device)}}
    else
        exec flutter run --no-pub --target lib/main.dart
    fi

# Analyze and run Mobile logic tests without starting a Node.
check-mobile: mobile-deps
    bash scripts/python.sh scripts/ci.py check mobile-format
    bash scripts/python.sh scripts/ci.py check mobile-check

# Run the existing Dart/Rust API contract suite against isolated Nodes.
test-mobile-contract:
    CARGO_TARGET_DIR={{quote(justfile_directory() / "target")}} bash scripts/check-mobile-contract.sh

# Build the Android arm64 debug APK with the normal app entry point.
mobile-apk: mobile-deps
    cd apps/mobile && flutter build apk --debug --no-pub --target lib/main.dart --target-platform android-arm64

# Build the iOS Simulator app with the normal app entry point.
mobile-ios: mobile-deps
    cd apps/mobile && flutter build ios --simulator --debug --no-pub --target lib/main.dart

# Use ordinary GPUI frame scheduling; test-support forces synchronous redraws.
test-workload:
    #!/usr/bin/env bash
    set -euo pipefail
    task_output="$PWD/.runtime/native-workload"
    mkdir -p "$task_output"
    task_triple="$(rustc -vV | awk '/^host: / {print $2}')"
    task_parsers=(--target "$task_triple" --output "${CARGO_TARGET_DIR:-target}/release/code-parsers")
    if [[ "$task_triple" == *apple-darwin ]]; then task_parsers+=(--sign -); fi
    TSLP_SOURCE_BUNDLE_URL="$(bash scripts/python.sh scripts/package/code-parsers.py "${task_parsers[@]}")"
    export TSLP_SOURCE_BUNDLE_URL
    unset TSLP_OFFLINE TSLP_LANGUAGES
    cargo build --locked --release -p sailry-desktop --features workload-tests
    printf 'Workload output: %s\n' "$task_output"
    SAILRY_WORKLOAD_OUTPUT="$task_output" ZED_MEASUREMENTS=1 target/release/sailry-desktop > "$task_output/stdout.log" 2> "$task_output/stderr.log"
