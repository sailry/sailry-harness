#!/bin/sh
set -eu

tool=$(dirname -- "$0")/../tool/build_native.sh
set -f
exec sh "$tool" ios "${PLATFORM_NAME:?PLATFORM_NAME is required}" ${ARCHS:?ARCHS is required}
