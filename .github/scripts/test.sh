#!/usr/bin/env bash
set -euo pipefail
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0

lane=${1:?A test lane is required}
case "$lane" in
  linux-0|linux-1|linux-2|linux-3|platform) ;;
  *) echo "Unknown test lane: $lane" >&2; exit 1 ;;
esac

# Every event tests every package. Tests also read schemas, lock files and
# source trees that the import graph does not record.
go list ./... > "$RUNNER_TEMP/packages.txt"

if [ "$lane" = platform ]; then
  platform=$(go env GOOS)
  case "$platform" in darwin|windows) ;; *) echo "No platform suite for $platform" >&2; exit 1 ;; esac
  go build ./...
  module=$(go list -m)
  # Go supplies only the files compiled for this OS, excluding integration
  # files. New OS-specific tests join the suite without a hand-maintained name.
  go list -f '{{.ImportPath}}{{range .TestGoFiles}}{{"\t"}}{{.}}{{end}}{{range .XTestGoFiles}}{{"\t"}}{{.}}{{end}}' ./... > "$RUNNER_TEMP/platform-files.txt"
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
  while IFS=$'\t' read -r -a files; do
    package=${files[0]}
    directory=${package#"$module"/}
    [ "$package" != "$module" ] || directory=.
    rule=$(awk -F '\t' -v package="$directory" '$1 == package { print $2 }' "$suite")
    if [ "$rule" = all ]; then
      go test -p 1 -timeout 20m "$package"
      continue
    fi
    auto=$RUNNER_TEMP/platform-auto.txt
    : > "$auto"
    for file in "${files[@]:1}"; do
      scoped=false
      case "$file" in *_"$platform"_test.go|*_"$platform"_*_test.go) scoped=true ;; esac
      if [ "$scoped" = true ] || grep -q '^//go:build ' "$directory/$file"; then
        awk '/^func[[:space:]]+(Test|Example|Fuzz)[[:alnum:]_]*[[:space:]]*\(/ { name=$2; sub(/\(.*/, "", name); if (name != "TestMain") print name }' "$directory/$file" >> "$auto"
      fi
    done
    [ -n "$rule" ] || [ -s "$auto" ] || continue
    go test -list . "$package" > "$RUNNER_TEMP/platform-tests.txt"
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
  done < "$RUNNER_TEMP/platform-files.txt"
  exit 0
fi

module=$(go list -m)
units=$RUNNER_TEMP/test-units.tsv
: > "$units"
ui_packages=()
while IFS= read -r package; do
  [ -n "$package" ] || continue
  case "$package" in
    */internal/desktop/live|*/internal/desktop/shell)
      ui_packages+=("$package")
      list="$RUNNER_TEMP/${package##*/}-tests.txt"
      go test -list . "$package" > "$list"
      awk -v package="$package" '/^(Test|Example|Fuzz)[^[:space:]]*$/ { print package "\t" $0 }' "$list" >> "$units"
      ;;
    *) printf '%s\t*\n' "$package" >> "$units" ;;
  esac
done < "$RUNNER_TEMP/packages.txt"

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
packages=()
while IFS= read -r package; do
  [ -z "$package" ] || packages+=("$package")
done < <(awk -F '\t' -v lane="$batch" '$1 == lane && $3 == "*" { print $2 }' "$RUNNER_TEMP/test-plan.tsv")

# Rasterizers already use each runner's CPUs. Lanes provide parallelism
# without making package binaries compete on one small runner.
if [ "${#packages[@]}" -gt 0 ]; then
  go test -p 1 -timeout 20m "${packages[@]}"
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
