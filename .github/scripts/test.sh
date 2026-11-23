#!/usr/bin/env bash
set -euo pipefail
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0

lane=${1:?A test lane is required}
case "$lane" in
  linux-0|linux-1|linux-2|linux-3|platform|integration) ;;
  *) echo "Unknown test lane: $lane" >&2; exit 1 ;;
esac

extract_tests() {
  local directory=$1
  shift
  local file
  for file in "$@"; do
    awk '/^func[[:space:]]+((Test|Example|Fuzz)([A-Z0-9_][[:alnum:]_]*)?)[[:space:]]*\(/ {
      name = $2
      sub(/\(.*/, "", name)
      if (name != "TestMain") print name
    }' "$directory/$file"
  done
}

module=$(go list -m)

if [ "$lane" = integration ]; then
  shift
  if [ "$#" -eq 0 ]; then
    echo 'At least one package is required for integration mode' >&2
    exit 1
  fi
  # Test lanes run only on Linux, so other systems run each package's whole suite.
  if [ "$(go env GOOS)" != linux ]; then
    go test -p 1 -timeout 20m -tags integration "$@"
    exit 0
  fi
  ran=0
  go list -tags integration -f '{{.ImportPath}}{{range .TestGoFiles}}{{"\t"}}{{.}}{{end}}{{range .XTestGoFiles}}{{"\t"}}{{.}}{{end}}' "$@" > "$RUNNER_TEMP/integration-files.txt"
  while IFS=$'\t' read -r -a files; do
    package=${files[0]}
    directory=${package#"$module"/}
    [ "$package" != "$module" ] || directory=.
    selected=()
    for file in "${files[@]:1}"; do
      if grep -Eq '^//go:build (.*[^[:alnum:]_])?integration([^[:alnum:]_]|$)' "$directory/$file"; then
        selected+=("$file")
      fi
    done
    if [ "${#selected[@]}" -eq 0 ]; then
      continue
    fi
    pattern=$(extract_tests "$directory" "${selected[@]}" | LC_ALL=C sort -u | awk '
      { printf "%s%s", separator, $0; separator = "|" }
      END { print "" }
    ')
    if [ -n "$pattern" ]; then
      go test -p 1 -timeout 20m -tags integration "$package" -run "^($pattern)$"
      ran=1
    fi
  done < "$RUNNER_TEMP/integration-files.txt"
  if [ "$ran" -eq 0 ]; then
    echo "No integration tests in: $*" >&2
    exit 1
  fi
  exit 0
fi

# Every event lists active package test files for the target OS via go list
# (zero compilation) instead of compiling test binaries just to list names.
go list -f '{{.ImportPath}}{{range .TestGoFiles}}{{"\t"}}{{.}}{{end}}{{range .XTestGoFiles}}{{"\t"}}{{.}}{{end}}' ./... > "$RUNNER_TEMP/package-files.txt"
awk -F '\t' '{ print $1 }' "$RUNNER_TEMP/package-files.txt" > "$RUNNER_TEMP/packages.txt"

if [ "$lane" = platform ]; then
  platform=$(go env GOOS)
  case "$platform" in darwin|windows) ;; *) echo "No platform suite for $platform" >&2; exit 1 ;; esac
  # Cross-platform vet already compiles every package; verify native command linking here.
  go build ./cmd/sailry-desktop ./cmd/sailry-host
  suite=$(cd "$(dirname "$0")" && pwd)/platform-tests.tsv
  # A renamed or removed package must not silently leave the curated suite.
  while IFS=$'\t' read -r directory _; do
    case "$directory" in ''|'#'*) continue ;; esac
    package=$module/$directory
    [ "$directory" != . ] || package=$module
    if ! grep -Fxq "$package" "$RUNNER_TEMP/packages.txt"; then
      echo "Platform suite names a missing package: $directory" >&2
      exit 1
    fi
  done < "$suite"
  all_packages=()
  while IFS=$'\t' read -r -a files; do
    package=${files[0]}
    directory=${package#"$module"/}
    [ "$package" != "$module" ] || directory=.
    rule=$(awk -F '\t' -v package="$directory" '$1 == package { print $2 }' "$suite")
    if [ "$rule" = all ]; then
      all_packages+=("$package")
      continue
    fi
    auto=$RUNNER_TEMP/platform-auto.txt
    : > "$auto"
    for file in "${files[@]:1}"; do
      scoped=false
      case "$file" in *_"$platform"_test.go|*_"$platform"_*_test.go) scoped=true ;; esac
      if [ "$scoped" = true ] || grep -Eq '^//go:build (.*[^[:alnum:]_])?(darwin|windows|linux|unix)([^[:alnum:]_]|$)' "$directory/$file"; then
        extract_tests "$directory" "$file" >> "$auto"
      fi
    done
    [ -n "$rule" ] || [ -s "$auto" ] || continue
    extract_tests "$directory" "${files[@]:1}" | LC_ALL=C sort -u > "$RUNNER_TEMP/platform-tests.txt"
    if [ -n "$rule" ] && ! grep -Eq "$rule" "$RUNNER_TEMP/platform-tests.txt"; then
      echo "Platform selector matches no tests: $package $rule" >&2
      exit 1
    fi
    pattern=$(awk -v rule="$rule" '
      FILENAME == ARGV[1] { names[$0] = 1; next }
      /^(Test|Example|Fuzz)/ && ($0 in names || (rule != "" && $0 ~ rule)) {
        printf "%s%s", separator, $0; separator = "|"
      }
      END { print "" }
    ' "$auto" "$RUNNER_TEMP/platform-tests.txt")
    if [ -n "$pattern" ]; then
      go test -p 1 -timeout 20m "$package" -run "^($pattern)$"
    fi
  done < "$RUNNER_TEMP/package-files.txt"
  if [ "${#all_packages[@]}" -gt 0 ]; then
    go test -p 1 -timeout 20m "${all_packages[@]}"
  fi
  exit 0
fi

units=$RUNNER_TEMP/test-units.tsv
: > "$units"
ui_packages=()
while IFS=$'\t' read -r -a files; do
  package=${files[0]}
  [ -n "$package" ] || continue
  directory=${package#"$module"/}
  [ "$package" != "$module" ] || directory=.
  case "$package" in
    */internal/desktop/live|*/internal/desktop/shell)
      ui_packages+=("$package")
      if [ "${#files[@]}" -gt 1 ]; then
        extract_tests "$directory" "${files[@]:1}" | LC_ALL=C sort -u | awk -v package="$package" '{ print package "\t" $0 }' >> "$units"
      fi
      ;;
    *) printf '%s\t*\n' "$package" >> "$units" ;;
  esac
done < "$RUNNER_TEMP/package-files.txt"

# Duration hints affect placement, never discovery. Whole packages remain
# indivisible; UI parents carry all their subtests. New units use defaults.
costs=$(cd "$(dirname "$0")" && pwd)/test-costs.tsv
awk -F '\t' -v module="$module" '
  FILENAME == ARGV[1] {
    if ($0 !~ /^#/ && NF == 3) costs[$1, $2] = $3
    next
  }
  {
    path = $1 == module ? "." : substr($1, length(module) + 2)
    key = path SUBSEP $2
    cost = key in costs ? costs[key] : ($2 == "*" ? 2 : 1)
    print cost "\t" $1 "\t" $2
  }
' "$costs" "$units" | LC_ALL=C sort -t $'\t' -k1,1nr -k2,2 -k3,3 > "$RUNNER_TEMP/weighted-tests.tsv"

# Stable longest-first placement keeps a slow package from sharing a runner
# with the slowest UI parents. Equal loads choose the lowest lane number.
awk -F '\t' '
  {
    lane = 0
    for (i = 1; i < 4; i++) if (load[i] + 0 < load[lane] + 0) lane = i
    print lane "\t" $2 "\t" $3 "\t" $1
    load[lane] += $1
  }
' "$RUNNER_TEMP/weighted-tests.tsv" > "$RUNNER_TEMP/test-plan.tsv"

batch=${lane#linux-}
awk -F '\t' -v lane="$batch" '
  $1 == lane { count++; cost += $4 }
  END { printf "Linux lane %d: %d units, estimated %.1fs\n", lane, count, cost }
' "$RUNNER_TEMP/test-plan.tsv"
backend_packages=()
desktop_packages=()
while IFS= read -r package; do
  [ -z "$package" ] || case "$package" in
    */internal/desktop/*|*/modules/*/desktop) desktop_packages+=("$package") ;;
    *) backend_packages+=("$package") ;;
  esac
done < <(awk -F '\t' -v lane="$batch" '$1 == lane && $3 == "*" { print $2 }' "$RUNNER_TEMP/test-plan.tsv")

# Non-UI backend packages compile and execute concurrently; UI rasterizers
# already fan out across runner cores and stay at -p 1.
if [ "${#backend_packages[@]}" -gt 0 ]; then
  go test -p 2 -timeout 20m "${backend_packages[@]}"
fi
if [ "${#desktop_packages[@]}" -gt 0 ]; then
  go test -p 1 -timeout 20m "${desktop_packages[@]}"
fi
if [ "${#ui_packages[@]}" -gt 0 ]; then
  for package in "${ui_packages[@]}"; do
    pattern=$(awk -F '\t' -v lane="$batch" -v package="$package" '
      $1 == lane && $2 == package { printf "%s%s", separator, $3; separator = "|" }
      END { print "" }
    ' "$RUNNER_TEMP/test-plan.tsv")
    if [ -n "$pattern" ]; then
      go test -p 1 -timeout 20m "$package" -run "^($pattern)$"
    fi
  done
fi
