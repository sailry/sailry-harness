#!/usr/bin/env bash
set -euo pipefail

# Collect declared licenses and source notices without choosing a product license.
task_platform="${1:?Usage: licenses.sh <goos/goarch> <Rust target> <new-output-directory> [command package]}"
task_target="${2:?Missing Rust target}"
task_output="${3:?Missing output directory}"
task_command="${4:-./cmd/sailry-host}"
task_root="$(cd "$(dirname "$0")/../../../.." && pwd)"
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
    cp "$task_license" "$2/$task_relative"
  done < <(find "$1" -type f \( -name 'LICENSE*' -o -name 'LICENCE*' -o -name 'COPYING*' -o -name 'NOTICE*' \))
}

# Go modules linked into the command for this platform. Go module metadata
# declares no license, so a module without a notice file is listed instead.
GOOS="${task_platform%/*}" GOARCH="${task_platform#*/}" CGO_ENABLED=0 go -C "$task_root" list -deps \
  -f '{{with .Module}}{{if .Version}}{{.Path}}{{"\t"}}{{.Version}}{{"\t"}}{{.Dir}}{{end}}{{end}}' \
  "$task_command" | sort -u > "$task_scratch/modules.tsv"
while IFS=$'\t' read -r task_name task_version task_directory; do
  task_destination="$task_output/go/$task_name@$task_version"
  mkdir -p "$task_destination"
  task_notices "$task_directory" "$task_destination"
  task_found=true
  if test -z "$(ls -A "$task_destination")"; then task_found=false; fi
  printf '%s\t%s\t%s\n' "$task_name" "$task_version" "$task_found"
done < "$task_scratch/modules.tsv" > "$task_scratch/collected.tsv"
jq -R -s 'split("\n") | map(select(length > 0) | split("\t") | {name:.[0],version:.[1],notices:(.[2] == "true")})' \
  "$task_scratch/collected.tsv" > "$task_output/go-modules.json"

# Rust crates linked into the iroh library, read with its pinned toolchain.
(cd "$task_root/native/iroh" && cargo metadata --locked --format-version 1 --filter-platform "$task_target") \
  > "$task_scratch/metadata.json"
jq --arg package sailry-iroh -f "$task_root/internal/node/host/deployment/dependencies.jq" \
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
    if test -f "$task_parent/Cargo.toml" && grep -q '^\[workspace\]' "$task_parent/Cargo.toml"; then break; fi
    task_parent="$(dirname "$task_parent")"
    if test "$task_parent" = /; then break; fi
  done
  task_notices "$task_directory" "$task_destination"
done < <(jq -r '.[] | [.name,.version,.manifest_path,(.license_file // "")] | @tsv' "$task_scratch/packages.json")

# Notices and locks of the prebuilt runtimes shipped beside the executable.
mkdir "$task_output/source-notices"
cp "$task_root"/native/*-LICENSE "$task_root"/native/*.lock.json \
  "$task_root/internal/localmodels/decision/llama-runtime.lock.json" "$task_output/source-notices/"
jq '[.[] | select(.license == null)] | map({name,version,source})' \
  "$task_output/dependencies.json" > "$task_output/undeclared.json"
printf 'Collected %s crate and %s module declarations for %s\n' \
  "$(jq length "$task_output/dependencies.json")" "$(jq length "$task_output/go-modules.json")" "${task_command##*/}"
