set dotenv-path := ".runtime/dev.env"

desktop data_dir=env_var_or_default("SAILRY_DESKTOP_DATA_DIR", ""):
    if test -n {{quote(data_dir)}}; then bash scripts/run-desktop.sh --data-dir {{quote(data_dir)}}; else bash scripts/run-desktop.sh; fi

preview:
    bash scripts/run-desktop.sh --preview

[positional-arguments]
ci *args:
    bash scripts/python.sh scripts/ci.py local "$@"

check:
    cargo fmt --all -- --check
    cargo clippy --locked --workspace --all-targets -- -D warnings

test-ui:
    bash scripts/python.sh scripts/ci.py check desktop

test-source:
    bash scripts/python.sh -m unittest discover -s scripts/tests -v
    bash scripts/python.sh -m unittest discover -s apps/desktop/src/content/files/preview/web -v
    bash scripts/python.sh themes/scripts/catalog.py --check
    bash scripts/python.sh -m unittest discover -s themes/scripts/tests -v
    node --test crates/content/src/markdown/visuals/render.test.mjs

test-plugins:
    bash scripts/python.sh scripts/test-plugins.py

host data_dir="":
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
    mkdir -p .preview
    task_output="$(mktemp -d "$PWD/.preview/native-workload-XXXXXX")"
    cargo build --locked --release -p sailry-desktop --features workload-tests
    printf 'Workload output: %s\n' "$task_output"
    SAILRY_WORKLOAD_OUTPUT="$task_output" ZED_MEASUREMENTS=1 target/release/sailry-desktop > "$task_output/stdout.log" 2> "$task_output/stderr.log"
