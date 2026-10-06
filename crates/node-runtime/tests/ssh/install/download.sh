#!/usr/bin/env bash
set -eu
download_url=
download_output=
while test "$#" -gt 0; do
  case "$1" in
    --output) download_output="$2"; shift 2 ;;
    https://github.com/sailry/sailry-harness/releases/download/v*) download_url="$1"; shift ;;
    --proto|--tlsv1.2) if test "$1" = --proto; then shift; fi; shift ;;
    --fail|--silent|--show-error|--location) shift ;;
    *) echo 'Unexpected fixture download argument' >&2; exit 1 ;;
  esac
done
test -n "$download_url"
test -n "$download_output"
cp "$HOME/.fixture/assets/${download_url##*/}" "$download_output"
