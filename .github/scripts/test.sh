#!/usr/bin/env bash
set -euo pipefail
unset GOROOT
export GOTOOLCHAIN=go1.27.3 CGO_ENABLED=0

lane=${1:?A test lane is required}
case "$lane" in
  packages|shell-0|shell-1|shell-2) ;;
  *) echo "Unknown test lane: $lane" >&2; exit 1 ;;
esac

go list ./... > "$RUNNER_TEMP/packages.txt"
cp "$RUNNER_TEMP/packages.txt" "$RUNNER_TEMP/affected.txt"
if [ "$GITHUB_EVENT_NAME" = pull_request ]; then
  changed=$(git diff --name-only HEAD^1 HEAD)
  if ! grep -Eq '^(go\.(mod|sum)$|\.github/(workflows/ci\.yml$|scripts/))' <<< "$changed"; then
    module=$(go list -m)
    go list -test -f '{{.ImportPath}}{{range .Deps}} {{.}}{{end}}' ./... > "$RUNNER_TEMP/deps.txt"
    # Embedded resources and testdata belong to their nearest Go package.
    # The root package owns its Go files and the pinned package submodules.
    printf '%s\n' "$changed" | while IFS= read -r file; do
      [ -n "$file" ] || continue
      directory=$(dirname "$file")
      while [ "$directory" != . ] && ! grep -Fxq "$module/$directory" "$RUNNER_TEMP/packages.txt"; do
        directory=$(dirname "$directory")
      done
      if [ "$directory" != . ]; then
        printf '%s\n' "$module/$directory"
        continue
      fi
      case "$file" in
        plugins|plugins/*|themes|themes/*) printf '%s\n' "$module" ;;
        */*) ;;
        *.go) printf '%s\n' "$module" ;;
      esac
    done | sort -u > "$RUNNER_TEMP/changed.txt"
    awk '
      FILENAME == ARGV[1] { changed[$0] = 1; next }
      $1 ~ /\.test$/ {
        name = substr($1, 1, length($1) - 5)
        for (i = 2; i <= NF; i++) {
          if ($i in changed) { print name; next }
        }
      }
    ' "$RUNNER_TEMP/changed.txt" "$RUNNER_TEMP/deps.txt" > "$RUNNER_TEMP/affected.txt"
    # SourceKeys reads these source trees without importing their packages.
    # Select that guard itself, without treating every i18n consumer as changed.
    if grep -Eq '^(internal|cmd)/.*\.go$' <<< "$changed"; then
      printf '%s/internal/desktop/i18n\n' "$module" >> "$RUNNER_TEMP/affected.txt"
    fi
    sort -u "$RUNNER_TEMP/affected.txt" -o "$RUNNER_TEMP/affected.txt"
  fi
fi

packages=()
shell_package=
while IFS= read -r package; do
  [ -n "$package" ] || continue
  case "$package" in
    */internal/desktop/shell) shell_package=$package ;;
    *) packages+=("$package") ;;
  esac
done < "$RUNNER_TEMP/affected.txt"

# Rasterizers already use each runner's CPUs. Independent lanes provide
# parallelism without making package binaries compete on one small runner.
if [ "$lane" = packages ]; then
  if [ "${#packages[@]}" -gt 0 ]; then
    go test -p 1 -timeout 20m "${packages[@]}"
  else
    echo 'No affected Go packages in this lane'
  fi
elif [ -n "$shell_package" ]; then
  go test -list . "$shell_package" > "$RUNNER_TEMP/shell-tests.txt"
  pattern=$(awk -v batch="${lane#shell-}" '
    /^(Test|Example|Fuzz)/ {
      if (count++ % 3 == batch) { printf "%s%s", separator, $0; separator = "|" }
    }
    END { print "" }
  ' "$RUNNER_TEMP/shell-tests.txt")
  if [ -n "$pattern" ]; then
    go test -p 1 -timeout 20m "$shell_package" -run "^($pattern)$"
  fi
else
  echo 'No affected shell tests'
fi
