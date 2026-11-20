#!/usr/bin/env bash
set -euo pipefail

test -f go.mod && test -f cmd/sailry-libp2p-relay/main.go || { echo 'run from the repository root' >&2; exit 1; }
test $# -eq 1 && { test "$1" = amd64 || test "$1" = arm64; } || {
  echo 'usage: package-linux.sh <amd64|arm64>' >&2
  exit 1
}

unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0 GOOS=linux GOARCH="$1"
umask 022
target=linux-$GOARCH
output=.runtime/relay/$target
archive=sailry-relay-$target.tar.gz
checksum=SHA256SUMS-relay-$target
scratch=$(mktemp -d "${TMPDIR:-/tmp}/sailry-relay-XXXXXX")
trap 'rm -rf -- "$scratch"' EXIT
stage=$scratch/package
mkdir "$stage"

go build -trimpath -o "$stage/sailry-relay" ./cmd/sailry-libp2p-relay
chmod 755 "$stage/sailry-relay"
cp LICENSE NOTICE "$stage/"
bash internal/node/host/deployment/licenses.sh "$GOOS/$GOARCH" --go-only "$stage/licenses" ./cmd/sailry-libp2p-relay
case "$(tar --version)" in
  *'GNU tar'*) owner=(--owner=0 --group=0) ;;
  *bsdtar*) owner=(--uid 0 --gid 0) ;;
  *) echo 'packaging requires GNU tar or bsdtar' >&2; exit 1 ;;
esac
COPYFILE_DISABLE=1 tar "${owner[@]}" --numeric-owner --no-acls --no-xattrs -czf "$scratch/$archive" -C "$stage" sailry-relay LICENSE NOTICE licenses
(
  cd "$scratch"
  if command -v sha256sum >/dev/null; then
    sha256sum "$archive"
  else
    shasum -a 256 "$archive"
  fi > "$checksum"
)
mkdir -p "$output"
mv "$scratch/$archive" "$scratch/$checksum" "$output/"
cat "$output/$checksum"
