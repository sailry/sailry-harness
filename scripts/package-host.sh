#!/usr/bin/env bash
set -euo pipefail

# Independent Linux Host archive for the execution Node and service controls.
task_profile="${1:-release}"
task_target="${2:?Usage: package-host.sh [debug|release] <Linux target>}"
case "$task_profile" in debug|release) ;; *) echo 'Invalid build profile' >&2; exit 1 ;; esac
case "$task_target" in x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) ;; *) echo 'Unsupported Linux Host target' >&2; exit 1 ;; esac
task_root="$(cd "$(dirname "$0")/.." && pwd)"
task_version="$(awk -F '"' '/^version = "/ {print $2; exit}' "$task_root/apps/host/Cargo.toml")"
test -x "$task_root/target/host-artifacts/$task_target/sailry-host"
mkdir -p "$task_root/dist"
task_output="$(mktemp -d "$task_root/dist/host-$task_target-$task_profile-XXXXXX")"
mkdir "$task_output/host"
cp "$task_root/target/host-artifacts/$task_target/sailry-host" "$task_output/host/sailry-host"
cp "$task_root/scripts/host-command.sh" "$task_output/host/sailry"
cp "$task_root/scripts/install-host.sh" "$task_output/host/install-host.sh"
cp "$task_root/crates/node-runtime/src/ssh/install/service.sh" "$task_output/host/service.sh"
chmod 755 "$task_output/host/sailry"
bash "$task_root/scripts/package/licenses.sh" sailry-host "$task_target" "$task_output/host/licenses"
cp "$task_root/Cargo.lock" "$task_output/host/Cargo.lock"
task_dirty=false
if test -n "$(git -C "$task_root" status --porcelain -- apps crates Cargo.toml Cargo.lock vendor scripts)"; then task_dirty=true; fi
jq -n --arg revision "$(git -C "$task_root" rev-parse HEAD)" --argjson dirty "$task_dirty" \
  --arg profile "$task_profile" --arg target "$task_target" --arg version "$task_version" \
  '{version:1,source_revision:$revision,source_dirty:$dirty,profile:$profile,target:$target,application_version:$version}' \
  > "$task_output/host/build.json"
tar -czf "$task_output/sailry-host-$task_version-$task_target.tar.gz" -C "$task_output/host" .
(cd "$task_output" && shasum -a 256 ./*.tar.gz > "SHA256SUMS-$task_target")
printf 'Host package: %s\n' "$task_output"
