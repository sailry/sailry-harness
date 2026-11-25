telemetry_build_flags() {
  local package=github.com/sailry/sailry-harness/internal/node/telemetry
  local product=${SAILRY_PRODUCT-harness}
  if ! [[ $product =~ ^[a-z][a-z0-9]*(-[a-z0-9]+)*$ ]] || (( ${#product} > 64 )); then
    echo 'SAILRY_PRODUCT must be a lowercase product ID of at most 64 characters' >&2
    return 1
  fi
  flags+=" -X $package.BuildProduct=$product -X $package.BuildRevision=$1"
  if [[ -n ${SAILRY_SENTRY_DSN:-} ]]; then flags+=" -X $package.BuildDSN=$SAILRY_SENTRY_DSN"; fi
  if [[ -n ${SAILRY_TELEMETRY_KEY:-} ]]; then flags+=" -X $package.BuildKey=$SAILRY_TELEMETRY_KEY"; fi
}
