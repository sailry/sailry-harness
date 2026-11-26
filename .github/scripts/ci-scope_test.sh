#!/usr/bin/env bash
# Exercise the CI path classifier without GitHub Actions or build tools.
set -euo pipefail

scope=$(cd "$(dirname "$0")" && pwd)/ci-scope.sh

expect_scope() {
  local label=$1 lanes=" $2 " actual expected lane
  shift 2
  actual=$(bash "$scope" "$@")
  expected=$(
    for lane in go native mobile android ios; do
      if [[ $lanes == *" $lane "* ]]; then
        printf '%s=true\n' "$lane"
      else
        printf '%s=false\n' "$lane"
      fi
    done
  )
  if [[ $actual != "$expected" ]]; then
    printf '%s:\nExpected:\n%s\nActual:\n%s\n' "$label" "$expected" "$actual" >&2
    return 1
  fi
}

paths() {
  local label=$1 lanes=$2
  shift 2
  { if (( $# )); then printf '%s\0' "$@"; fi; } | expect_scope "$label" "$lanes"
}

paths 'empty diff' ''
paths 'README translations and images' '' README.md README.zh-CN.md README.ja.md assets/readme/architecture.svg
paths 'literal newline in a README asset path' '' $'assets/readme/example\ninternal/protocol/wire.go'
paths 'other assets are not README assets' 'go' assets/branding/sailry.png
paths 'mixed documentation and code' 'go' README.zh-CN.md internal/node/node.go
paths 'desktop native dependencies' 'go native' native/capture/sailry_capture.c
paths 'workflow changes do not build apps' 'go native mobile' .github/workflows/ci.yml

for path in internal/client/client.go internal/link/link.go internal/protocol/message.go internal/identity/identity.go; do
  paths "$path" 'go mobile' "$path"
  paths "${path%.go}_test.go" 'go' "${path%.go}_test.go"
done
paths 'mobile-only Go tests' 'go' internal/mobile/runtime_test.go cmd/sailry-mobile/exports_test.go
paths 'Flutter features' 'mobile' apps/mobile/lib/main.dart
paths 'Flutter tests' 'mobile' apps/mobile/test/attention_test.dart
paths 'catalogs' 'go mobile' assets/catalogs/models.json

for path in cmd/sailry-mobile/exports.go cmd/sailry-mobile/sailry_mobile.h internal/mobile/runtime.go go.mod go.sum; do
  paths "$path" 'go mobile android ios' "$path"
done
for path in apps/mobile/tool/build_native.sh apps/mobile/packages/sailry_bridge/lib/src/native.dart \
  apps/mobile/pubspec.yaml apps/mobile/pubspec.lock apps/mobile/.flutter-version; do
  paths "$path" 'mobile android ios' "$path"
done
paths 'Android project and shared Go' 'go mobile android' apps/mobile/android/app/build.gradle.kts internal/link/link.go
paths 'Android toolchain' 'go mobile android' .github/actions/android/action.yml
paths 'iOS project' 'mobile ios' apps/mobile/ios/Runner.xcodeproj/project.pbxproj
paths 'iOS compiler wrapper' 'go mobile ios' cmd/sailry-mobile/clang-ios.sh

expect_scope 'manual checks and missing-diff fallback' 'go native mobile' none
expect_scope 'manual Android' 'go native mobile android' android
expect_scope 'manual iOS' 'go native mobile ios' ios
expect_scope 'manual both packages' 'go native mobile android ios' both
