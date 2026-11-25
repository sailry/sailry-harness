#!/usr/bin/env bash
set -euo pipefail

# Collect declared licenses and source notices without choosing a product license.
task_platform="${1:?Usage: licenses.sh <goos/goarch> <Rust target|--go-only> <new-output-directory> [command package]}"
task_target="${2:?Missing Rust target or --go-only}"
task_output="${3:?Missing output directory}"
task_command="${4:-./cmd/sailry-host}"
task_root="$(cd "$(dirname "$0")/../../../.." && pwd)"
task_command_root="${5:-$task_root}"
task_command_root="$(cd "$task_command_root" && pwd)"
test ! -e "$task_output"
mkdir -p "$task_output"
task_output="$(cd "$task_output" && pwd)"
task_scratch="$(mktemp -d "${TMPDIR:-/tmp}/sailry-licenses-XXXXXX")"
trap 'rm -rf -- "$task_scratch"' EXIT

# Copies every license and notice file below a source directory, keeping its relative path.
task_notices() {
  while IFS= read -r task_license; do
    task_relative="${task_license#"$1/"}"
    mkdir -p "$(dirname "$2/$task_relative")"
    cp -f "$task_license" "$2/$task_relative"
  done < <(find "$1" -type d \( -name .git -o -name .runtime \) -prune -o -type f \( -name 'LICENSE*' -o -name 'LICENCE*' -o -name 'COPYING*' -o -name 'NOTICE*' -o -name 'THIRD_PARTY_NOTICES*' \) -print)
}

# Go modules linked into the command for this platform. Go module metadata
# declares no license, so a module without a notice file is listed instead.
GOOS="${task_platform%/*}" GOARCH="${task_platform#*/}" CGO_ENABLED=0 go -C "$task_command_root" list -deps \
  -f '{{with .Module}}{{.Path}}{{"\t"}}{{or .Version "workspace"}}{{"\t"}}{{.Dir}}{{end}}' \
  "$task_command" | sort -u > "$task_scratch/modules.tsv"
while IFS=$'\t' read -r task_name task_version task_directory; do
  test -n "$task_name" && test "$task_directory" != "$task_command_root" || continue
  task_destination="$task_output/go/$task_name@$task_version"
  mkdir -p "$task_destination"
  task_notices "$task_directory" "$task_destination"
  task_found=true
  if test -z "$(ls -A "$task_destination")"; then task_found=false; fi
  printf '%s\t%s\t%s\n' "$task_name" "$task_version" "$task_found"
done < "$task_scratch/modules.tsv" > "$task_scratch/collected.tsv"
jq -R -s 'split("\n") | map(select(length > 0) | split("\t") | {name:.[0],version:.[1],notices:(.[2] == "true")})' \
  "$task_scratch/collected.tsv" > "$task_output/go-modules.json"

if test "$task_target" = --go-only; then
  task_goroot="$(go -C "$task_root" env GOROOT)"
  cp "$task_goroot/LICENSE" "$task_output/go-LICENSE"
  printf 'Collected %s module declarations for %s\n' \
    "$(jq length "$task_output/go-modules.json")" "${task_command##*/}"
  exit 0
fi

# Rust crates linked into Turso, read with its pinned toolchain unless a
# pre-collected cache directory for this Cargo.lock and target is supplied.
task_rust_cache="${SAILRY_RUST_LICENSES_CACHE:-}"
if test -n "$task_rust_cache" && test -f "$task_rust_cache/dependencies.json"; then
  cp -R "$task_rust_cache"/. "$task_output/"
else
  (cd "$task_root/native/turso" && cargo metadata --locked --format-version 1 --filter-platform "$task_target") \
    > "$task_scratch/turso-metadata.json"
  jq --arg package sailry-turso -f "$task_root/internal/node/host/deployment/dependencies.jq" \
    "$task_scratch/turso-metadata.json" > "$task_scratch/packages.json"
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
        cp -f "$task_license" "$task_destination/$(basename "$task_license")"
      fi
    fi
    # Git workspaces commonly keep their shared license above an individual crate.
    task_parent="$task_directory"
    for task_depth in 0 1 2 3; do
      for task_license in "$task_parent"/LICENSE* "$task_parent"/LICENCE* \
        "$task_parent"/COPYING* "$task_parent"/NOTICE*; do
        if test -f "$task_license"; then
          cp -f "$task_license" "$task_destination/$(basename "$task_license")"
        fi
      done
      if test -n "$(ls -A "$task_destination")"; then break; fi
      if test "$task_parent" = "$task_root"; then break; fi
      if test -f "$task_parent/Cargo.toml" && grep -q '^\[workspace\]' "$task_parent/Cargo.toml"; then break; fi
      task_parent="$(dirname "$task_parent")"
      if test "$task_parent" = /; then break; fi
    done
    task_notices "$task_directory" "$task_destination"
  done < <(jq -r '.[] | [.name,.version,.manifest_path,(.license_file // "")] | @tsv' "$task_scratch/packages.json")
  jq '[.[] | select(.license == null)] | map({name,version,source})' \
    "$task_output/dependencies.json" > "$task_output/undeclared.json"
  if test -n "$task_rust_cache"; then
    rm -rf -- "$task_rust_cache"
    mkdir -p "$task_rust_cache"
    for task_entry in "$task_output"/*; do
      task_base="$(basename "$task_entry")"
      case "$task_base" in go | go-modules.json) continue ;; esac
      cp -R "$task_entry" "$task_rust_cache/$task_base"
    done
  fi
fi

# Notices and locks of the prebuilt runtimes shipped beside the executable.
mkdir -p "$task_output/source-notices"
cp "$task_root/internal/node/providers/data/LICENSE.models.dev" "$task_output/source-notices/"
cp "$task_root"/native/*-LICENSE "$task_root"/native/*.lock.json \
  "$task_root/internal/localmodels/decision/llama-runtime.lock.json" "$task_output/source-notices/"
cp "$task_root/native/turso/LICENSE-Turso" "$task_output/source-notices/turso-LICENSE"
cp "$task_root/native/turso/SOURCE" "$task_output/source-notices/turso-SOURCE"
cp "$task_root/native/turso/Cargo.lock" "$task_output/source-notices/turso-Cargo.lock"
printf 'Collected %s crate and %s module declarations for %s\n' \
  "$(jq length "$task_output/dependencies.json")" "$(jq length "$task_output/go-modules.json")" "${task_command##*/}"
