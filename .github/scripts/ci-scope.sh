#!/usr/bin/env bash
# Accept NUL-delimited changed paths, or a manual mobile build selection.
set -euo pipefail

go_checks=false
native=false
mobile=false
android=false
ios=false

case "${1:-paths}" in
  paths)
    while IFS= read -r -d '' path; do
      case "$path" in
        docs/* | third_party_licenses/* | assets/readme/* | apps/mobile/* | \
        .github/ISSUE_TEMPLATE/* | .github/pull_request_template.md) ;;
        *)
          if [[ ! $path =~ ^((README[^/]*|CONTRIBUTING|ARCHITECTURE|AGENTS|CHANGELOG|SECURITY|LICENSE[^/]*)\.md|LICENSE)$ ]]; then
            go_checks=true
          fi
          ;;
      esac

      case "$path" in
        native/turso/* | native/capture/* | native/windows/* | .github/actions/turso/* | .github/workflows/ci.yml)
          native=true ;;
      esac

      # Go test files do not participate in the mobile bridge build.
      case "$path" in
        *_test.go) ;;
        apps/mobile/android/* | .github/actions/android/*)
          android=true ;;
        apps/mobile/ios/* | cmd/sailry-mobile/clang-ios.sh)
          ios=true ;;
        # Both packages use the same FFI bridge, native build script, and dependency pins.
        cmd/sailry-mobile/* | internal/mobile/* | apps/mobile/tool/build_native.sh | \
        apps/mobile/packages/sailry_bridge/* | apps/mobile/.flutter-version | apps/mobile/pubspec.yaml | \
        apps/mobile/pubspec.lock | go.mod | go.sum | .github/actions/flutter/*)
          android=true
          ios=true ;;
        apps/mobile/* | assets/catalogs/* | internal/client/* | internal/link/* | internal/protocol/* | \
        internal/identity/* | .github/actions/go-cache/* | .github/workflows/ci.yml | .github/scripts/ci-scope.sh)
          mobile=true ;;
      esac
    done
    if [[ $android == true || $ios == true ]]; then
      mobile=true
    fi
    ;;
  none | android | ios | both)
    go_checks=true
    native=true
    mobile=true
    case "$1" in
      android) android=true ;;
      ios) ios=true ;;
      both) android=true; ios=true ;;
    esac
    ;;
  *)
    printf 'Unknown CI scope: %s\n' "$1" >&2
    exit 2
    ;;
esac

printf 'go=%s\nnative=%s\nmobile=%s\nandroid=%s\nios=%s\n' "$go_checks" "$native" "$mobile" "$android" "$ios"
