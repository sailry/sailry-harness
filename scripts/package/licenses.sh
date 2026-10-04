#!/usr/bin/env bash
set -euo pipefail

# Collect declared licenses and source notices without choosing a product license.
task_package="${1:?Usage: licenses.sh <package> <target> <new-output-directory>}"
task_target="${2:?Missing target}"
task_output="${3:?Missing output directory}"
task_root="$(cd "$(dirname "$0")/../.." && pwd)"
test ! -e "$task_output"
mkdir -p "$task_output"
task_scratch="$(mktemp -d "$task_root/target/licenses-XXXXXX")"
cargo metadata --locked --format-version 1 --filter-platform "$task_target" \
  --manifest-path "$task_root/Cargo.toml" > "$task_scratch/metadata.json"
jq --arg package "$task_package" -f "$task_root/scripts/package/dependencies.jq" \
  "$task_scratch/metadata.json" > "$task_scratch/packages.json"
jq --arg root "$task_root/" 'map({name,version,license,repository,
  source:(.source // (.manifest_path | ltrimstr($root) | sub("/Cargo.toml$";"")))})' \
  "$task_scratch/packages.json" > "$task_output/dependencies.json"

while IFS=$'\t' read -r task_name task_version task_manifest task_declared; do
  task_directory="$(dirname "$task_manifest")"
  task_destination="$task_output/$task_name-$task_version"
  mkdir -p "$task_destination"
  if test -n "$task_declared"; then
    case "$task_declared" in
      /*) task_license="$task_declared" ;;
      *) task_license="$task_directory/$task_declared" ;;
    esac
    if test -f "$task_license"; then
      cp "$task_license" "$task_destination/$(basename "$task_license")"
    fi
  fi
  # Git workspaces commonly keep their shared license above an individual crate.
  task_parent="$task_directory"
  for task_depth in 0 1 2 3; do
    for task_license in "$task_parent"/LICENSE* "$task_parent"/LICENCE* \
      "$task_parent"/COPYING* "$task_parent"/NOTICE*; do
      if test -f "$task_license"; then
        cp "$task_license" "$task_destination/$(basename "$task_license")"
      fi
    done
    if test -n "$(ls -A "$task_destination")"; then break; fi
    if test "$task_parent" = "$task_root"; then break; fi
    if test -f "$task_parent/Cargo.toml" && rg -q '^\[workspace\]' "$task_parent/Cargo.toml"; then break; fi
    task_parent="$(dirname "$task_parent")"
    if test "$task_parent" = /; then break; fi
  done
  while IFS= read -r task_license; do
    task_relative="${task_license#"$task_directory/"}"
    mkdir -p "$(dirname "$task_destination/$task_relative")"
    cp "$task_license" "$task_destination/$task_relative"
  done < <(rg --files --hidden -g 'LICENSE*' -g 'LICENCE*' -g 'COPYING*' -g 'NOTICE*' "$task_directory")
done < <(jq -r '.[] | [.name,.version,.manifest_path,(.license_file // "")] | @tsv' "$task_scratch/packages.json")
cp -R "$task_root/third_party_licenses" "$task_output/source-notices"
jq '[.[] | select(.license == null)] | map({name,version,source})' \
  "$task_output/dependencies.json" > "$task_output/undeclared.json"
printf 'Collected %s dependency declarations for %s\n' \
  "$(jq length "$task_output/dependencies.json")" "$task_package"
