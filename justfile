# Local development recipes. Commands and environment follow CONTRIBUTING.md.
# Run `just --list` for an overview.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Every Go command runs with the pinned toolchain, without cgo and without an
# inherited GOROOT. The two documented cgo exceptions override it per command.
export GOTOOLCHAIN := "go1.27.3"
export CGO_ENABLED := "0"
unexport GOROOT

goos := if os() == "macos" { "darwin" } else { os() }
goarch := if arch() == "aarch64" { "arm64" } else if arch() == "x86_64" { "amd64" } else { arch() }
native := justfile_directory() / ".runtime/native" / goos + "-" + goarch
iroh := if os() == "macos" { "libsailry_iroh.dylib" } else if os() == "windows" { "sailry_iroh.dll" } else { "libsailry_iroh.so" }
profiles := justfile_directory() / ".runtime/profiles"

# List the recipes.
default:
    @just --list

# Show the isolated preview UI; it opens no profile and starts no Node.
preview:
    go run ./cmd/sailry-desktop --preview

# Run Desktop live against the fixed profile .runtime/profiles/<name>.
desktop name="desktop": native
    go run ./cmd/sailry-desktop --data-dir {{ quote(profiles / name) }}

# Build and run Host against the fixed profile .runtime/profiles/<name>; extra arguments are Host options such as --internet.
host name="host" *options: native
    go build -o .runtime/sailry-host ./cmd/sailry-host
    .runtime/sailry-host --data-dir {{ quote(profiles / name) }} {{ options }}

# Build the local iroh library with the pinned Rust toolchain when it is missing.
native:
    test -f {{ quote(native / iroh) }} || sh native/iroh/build.sh

# Rebuild the local iroh library after its source, C ABI, pins or target changed.
native-build:
    sh native/iroh/build.sh

# Stage this machine's pinned native runtimes in .runtime/native after checking them against the lock files.
native-fetch:
    go run ./native/fetch

# Print the local tools beside the versions this repository pins.
tools:
    @echo "Go: go1.27.3 pinned"; go version
    @echo "Rust: 1.98.0 pinned in native/iroh/rust-toolchain.toml"; cd native/iroh && (rustc --version || echo "rustc is not installed")
    @echo "Flutter: $(cat apps/mobile/.flutter-version) pinned in apps/mobile/.flutter-version"; flutter --version || echo "flutter is not installed"
    @echo "Node.js: 22.13.0 or newer for services/pairing-relay and plugin tests"; node --version || echo "node is not installed"
    @just --version

# Generate the desktop catalogs from the source catalogs in locales/.
i18n:
    go generate ./internal/desktop/i18n

# Run the complete gate required before every code commit.
check:
    test -z "$(gofmt -l .)"
    go build ./...
    go vet ./...
    go test ./...

# Run every default test.
test:
    go test ./...

# Run the Desktop UI and composition tests.
test-desktop:
    go test ./internal/desktop/... ./cmd/sailry-desktop/...

# Run the tests of every package outside Desktop.
test-backend:
    go test $(go list ./... | grep -v -e '/internal/desktop' -e '/cmd/sailry-desktop')

# Run integration-tagged tests of the named packages, for example ./internal/link.
test-integration +packages:
    go test -tags integration {{ packages }}

# Run the named packages under the race detector, once per phase for Node, Client and Link changes.
test-race +packages:
    CGO_ENABLED=1 go test -race {{ packages }}

# Run the transcript frame benchmarks.
bench:
    go test ./internal/desktop/conversation -run '^$' -bench .

# Run every plugin package's deterministic tests from the plugins submodule.
[working-directory('plugins')]
test-plugins:
    node --experimental-vm-modules scripts/check-packages.mjs
    python3 scripts/test.py

# Typecheck and test the pairing service.
[working-directory('services/pairing-relay')]
test-pairing-relay:
    pnpm install --frozen-lockfile --ignore-scripts
    pnpm typecheck
    pnpm test

# List phones and running simulators recognized by Flutter.
mobile-devices:
    flutter devices

# Fetch the pinned Flutter dependencies.
[working-directory('apps/mobile')]
mobile-deps:
    flutter pub get --enforce-lockfile

# Run the mobile app; the Flutter build runs the native adapter build itself.
[working-directory('apps/mobile')]
mobile device="": mobile-deps
    flutter run --no-pub --target lib/main.dart {{ if device == "" { "" } else { "-d " + quote(device) } }}

# Vet the mobile C ABI with cgo, then analyze and test the Flutter application.
check-mobile: mobile-deps
    CGO_ENABLED=1 go vet ./cmd/sailry-mobile
    cd apps/mobile && flutter analyze
    cd apps/mobile && flutter test

# Build the Android arm64 debug APK.
[working-directory('apps/mobile')]
mobile-apk: mobile-deps
    flutter build apk --debug --no-pub --target lib/main.dart --target-platform android-arm64

# Build the iOS Simulator application.
[working-directory('apps/mobile')]
mobile-ios: mobile-deps
    flutter build ios --simulator --debug --no-pub --target lib/main.dart
