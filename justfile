set dotenv-path := ".runtime/dev.env"

desktop data_dir=env_var_or_default("SAILRY_DESKTOP_DATA_DIR", justfile_directory() / ".runtime/desktop"):
    bash scripts/run-desktop.sh --data-dir {{quote(data_dir)}}

preview:
    bash scripts/run-desktop.sh --preview

check:
    cargo fmt --all -- --check
    cargo clippy --locked --workspace --all-targets -- -D warnings

test-ui:
    cargo test --locked -p sailry-desktop -- --test-threads=1

test-source:
    python3 -m unittest discover -s scripts/tests -v

test-plugins:
    python3 scripts/test-plugins.py

host data_dir="":
    if test -n {{quote(data_dir)}}; then cargo run --locked -p sailry-host -- --data-dir {{quote(data_dir)}}; else cargo run --locked -p sailry-host; fi

test-backend:
    cargo test --locked -p sailry-protocol -p sailry-link -p sailry-client -p sailry-node-runtime -p sailry-host -- --test-threads=4

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

# Analyze and run Flutter unit/interaction tests without starting a Node.
check-mobile: mobile-deps
    cd apps/mobile && flutter analyze --no-pub
    cd apps/mobile && flutter test --no-pub

# Run the existing Dart/Rust API contract suite against isolated Nodes.
test-mobile-contract:
    CARGO_TARGET_DIR={{quote(justfile_directory() / "target")}} bash scripts/check-mobile-contract.sh

# Test Flutter against an isolated real Node, on the host or a running simulator.
test-mobile-node device="": mobile-deps
    #!/usr/bin/env bash
    set -euo pipefail
    export CARGO_TARGET_DIR={{quote(justfile_directory() / "target")}}
    if test -n {{quote(device)}}; then
        export SAILRY_FLUTTER_DEVICE={{quote(device)}}
    else
        unset SAILRY_FLUTTER_DEVICE
        cargo build --locked -p sailry-mobile-bridge --lib
    fi
    cargo test --locked -p sailry-mobile-bridge --test flutter controls_remote_node -- --ignored --exact --nocapture

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
    mkdir -p .preview
    task_output="$(mktemp -d "$PWD/.preview/native-workload-XXXXXX")"
    cargo build --locked --release -p sailry-desktop --features workload-tests
    printf 'Workload output: %s\n' "$task_output"
    SAILRY_WORKLOAD_OUTPUT="$task_output" ZED_MEASUREMENTS=1 target/release/sailry-desktop > "$task_output/stdout.log" 2> "$task_output/stderr.log"
