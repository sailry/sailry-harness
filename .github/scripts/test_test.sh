#!/usr/bin/env bash
# Exercise the real lane script with synthetic package graphs and Go test lists.
set -euo pipefail

script=$(cd "$(dirname "$0")" && pwd)/test.sh
fixture=$(mktemp -d "${TMPDIR:-/tmp}/sailry-ci-lanes-XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
export SAILRY_CI_FIXTURE=$fixture
mkdir "$fixture/bin"
mkdir "$fixture/source"
export PATH="$fixture/bin:$PATH"

module=example.test/sailry
backend=$module/internal/client
i18n=$module/internal/desktop/i18n
live=$module/internal/desktop/live
shell=$module/internal/desktop/shell
printf '%s\n' "$module" > "$fixture/module"
printf '%s\n' "$backend" "$i18n" "$live" "$shell" > "$fixture/packages"
printf '%s.test %s\n' "$backend" "$backend" "$i18n" "$i18n" > "$fixture/deps"
printf '%s.test %s %s\n' "$live" "$live" "$backend" "$shell" "$shell" "$backend" >> "$fixture/deps"
printf '%s\n' TestLiveOne TestLiveTwo ExampleLive FuzzLive TestLiveThree TestLiveFour TestLiveFive > "$fixture/live.names"
printf '%s\n' ExampleShell TestShellOne FuzzShell TestShellTwo TestShellThree > "$fixture/shell.names"
for package in live shell; do
  cp "$fixture/$package.names" "$fixture/$package.list"
  printf 'BenchmarkIgnored\nok\t%s/internal/desktop/%s\t0.001s\n' "$module" "$package" >> "$fixture/$package.list"
done

cat > "$fixture/bin/go" <<'GO'
#!/usr/bin/env bash
set -euo pipefail
case "$1 ${2:-}" in
  'list ./...') cat "$SAILRY_CI_FIXTURE/packages" ;;
  'list -m') cat "$SAILRY_CI_FIXTURE/module" ;;
  'list -test') cat "$SAILRY_CI_FIXTURE/deps" ;;
  'env GOOS') printf '%s\n' "$SAILRY_CI_PLATFORM" ;;
  'build ./...')
    test "${SAILRY_CI_FAILURE:-}" != build || exit 43
    printf '%s\n' "$SAILRY_CI_PLATFORM" >> "$SAILRY_CI_FIXTURE/builds"
    ;;
  'list -f')
    for package in "${@:4}"; do
      awk -F '\t' -v package="$package" '$1 == package' "$SAILRY_CI_FIXTURE/$SAILRY_CI_PLATFORM.metadata"
    done
    ;;
  'test -list')
    test "$#" -eq 4 && test "$3" = .
    test "${SAILRY_CI_FAILURE:-}" != list || exit 31
    list=$SAILRY_CI_FIXTURE/${4##*/}.${SAILRY_CI_PLATFORM:-linux}.list
    test -f "$list" || list=$SAILRY_CI_FIXTURE/${4##*/}.list
    cat "$list"
    ;;
  'test -p')
    test "$3" = 1 && test "$4" = -timeout && test "$5" = 20m
    test "${SAILRY_CI_FAILURE:-}" != test || exit 37
    shift 5
    if test "$#" -eq 3 && test "$2" = -run; then
      printf '%s\t%s\t%s\n' "$SAILRY_CI_LANE" "$1" "$3" >> "$SAILRY_CI_FIXTURE/calls"
    else
      for package in "$@"; do printf '%s\t%s\tall\n' "$SAILRY_CI_LANE" "$package"; done >> "$SAILRY_CI_FIXTURE/calls"
    fi
    ;;
  *) echo "Unexpected Go command: $*" >&2; exit 1 ;;
esac
GO
cat > "$fixture/bin/git" <<'GIT'
#!/usr/bin/env bash
set -euo pipefail
test "$*" = 'diff --name-only HEAD^1 HEAD'
cat "$SAILRY_CI_FIXTURE/changed"
GIT
chmod +x "$fixture/bin/go" "$fixture/bin/git"

check_case() {
  local name=$1 event=$2 changed=$3 directory package lane lanes
  shift 3
  directory=$fixture/$name
  mkdir "$directory"
  printf '%s\n' "$changed" > "$fixture/changed"
  : > "$fixture/calls"
  : > "$fixture/builds"
  lanes=(packages shell-0 shell-1 shell-2)
  [ -z "${SAILRY_CI_PLATFORM:-}" ] || lanes=(platform)
  for lane in "${lanes[@]}"; do
    mkdir "$directory/$lane"
    if ! (cd "$fixture/source" && RUNNER_TEMP=$directory/$lane GITHUB_EVENT_NAME=$event SAILRY_CI_LANE=$lane bash "$script" "$lane") > "$directory/$lane/output" 2>&1; then
      cat "$directory/$lane/output" >&2
      exit 1
    fi
  done
  # Expand each actual anchored -run pattern into the names it selects.
  # Comparing multisets proves every expected parent runs exactly once.
  awk -F '\t' '
    $3 == "all" {
      if ($1 != "packages" && $1 != "platform") exit 1
      print $2 "\tall"; next
    }
    {
      if (($1 !~ /^shell-[012]$/ && $1 != "platform") || substr($3, 1, 2) != "^(" || substr($3, length($3)-1) != ")$") exit 1
      count = split(substr($3, 3, length($3)-4), names, /\|/)
      if ($1 != "platform") { packages[$2] = 1; counts[$2, $1] += count }
      for (i = 1; i <= count; i++) print $2 "\t" names[i]
    }
    END {
      for (package in packages) {
        low = high = counts[package, "shell-0"]
        for (i = 1; i < 3; i++) {
          n = counts[package, "shell-" i]
          if (n < low) low = n
          if (n > high) high = n
        }
        if (high - low > 1) exit 1
      }
    }
  ' "$fixture/calls" | sort > "$directory/actual"
  : > "$directory/expected"
  for package in "$@"; do
    if [ -n "${SAILRY_CI_PLATFORM:-}" ]; then
      awk -v package="$package" '{ print package "\t" $0 }' "$fixture/${package##*/}.$SAILRY_CI_PLATFORM.expected"
      continue
    fi
    case "$package" in
      "$live"|"$shell")
        awk -v package="$package" '{ print package "\t" $0 }' "$fixture/${package##*/}.names"
        ;;
      *) printf '%s\tall\n' "$package" ;;
    esac
  done | sort > "$directory/expected"
  diff -u "$directory/expected" "$directory/actual"
  if [ -n "${SAILRY_CI_PLATFORM:-}" ]; then
    test "$(cat "$fixture/builds")" = "$SAILRY_CI_PLATFORM"
  else
    test ! -s "$fixture/builds"
  fi
}

check_case full push '' "$backend" "$i18n" "$live" "$shell"
check_case live-only pull_request internal/desktop/live/testdata/input.json "$live"
check_case shell-only pull_request internal/desktop/shell/header.go "$i18n" "$shell"
check_case dependency pull_request internal/client/commands.go "$backend" "$i18n" "$live" "$shell"
check_case workflow pull_request .github/scripts/test.sh "$backend" "$i18n" "$live" "$shell"
check_case unrelated pull_request README.md

# Fewer parents than lanes leave a lane empty, without an unfiltered test run.
printf '%s\n' TestOnlyParent > "$fixture/live.names"
cp "$fixture/live.names" "$fixture/live.list"
check_case sparse pull_request internal/desktop/live/testdata/input.json "$live"

for failure in list test; do
  directory=$fixture/failure-$failure
  mkdir "$directory"
  status=0
  RUNNER_TEMP=$directory GITHUB_EVENT_NAME=push SAILRY_CI_FAILURE=$failure bash "$script" shell-0 > "$directory/output" 2>&1 || status=$?
  case "$failure" in list) expected=31 ;; test) expected=37 ;; esac
  test "$status" -eq "$expected" || { cat "$directory/output" >&2; echo "$failure failure returned $status, expected $expected" >&2; exit 1; }
done

# Native lanes build all production packages, then run only the platform suite
# and new OS-file tests from the affected package graph. Source files stay here.
core=$module/internal/core
engine=$module/internal/node/agent/engine
node=$module/internal/node
printf '%s\n' "$core" "$engine" "$node" >> "$fixture/packages"
printf '%s.test %s\n' "$core" "$core" "$engine" "$engine" "$node" "$node" >> "$fixture/deps"
for package in "$backend" "$i18n" "$live" "$shell" "$core" "$engine" "$node"; do
  mkdir -p "$fixture/source/${package#"$module"/}"
  printf 'package fixture\n' > "$fixture/source/${package#"$module"/}/shared_test.go"
done
printf '%s\n' TestLargeInstructionsReachModel TestSharedEngineShouldNotRun > "$fixture/engine.list"
printf '%s\n' TestConstrainedProfile TestSharedNodeShouldNotRun > "$fixture/node.list"
printf '%s\n' TestAuthorizationFixedPortCallback TestAttachmentLocalIO TestUpdatePendingNativeInput TestSSHWindowsPairingGuidance TestSharedLiveShouldNotRun > "$fixture/live.list"
printf '%s\n' TestCloseShortcut TestNativeClosePrompt TestMenu TestSharedShellShouldNotRun > "$fixture/shell.list"
cat > "$fixture/source/internal/node/constrained_test.go" <<'SOURCE'
//go:build darwin || windows

package fixture

func TestConstrainedProfile() {}
func TestMain() {}
SOURCE
for platform in darwin windows; do
  : > "$fixture/$platform.metadata"
  for package in "$i18n" "$live" "$shell" "$core" "$engine"; do
    printf '%s\tshared_test.go\n' "$package" >> "$fixture/$platform.metadata"
  done
  printf '%s\tshared_test.go\tconstrained_test.go\n' "$node" >> "$fixture/$platform.metadata"
  printf '%s\tshared_test.go\tnew_%s_test.go\n' "$backend" "$platform" >> "$fixture/$platform.metadata"
  cat > "$fixture/source/internal/client/new_${platform}_test.go" <<SOURCE
package fixture

func TestNewPlatform_${platform}() {}
func ExampleNewPlatform_${platform}() {}
func FuzzNewPlatform_${platform}() {}
SOURCE
  printf '%s\n' "TestNewPlatform_$platform" "ExampleNewPlatform_$platform" "FuzzNewPlatform_$platform" TestSharedClientShouldNotRun > "$fixture/client.$platform.list"
  printf '%s\n' "TestNewPlatform_$platform" "ExampleNewPlatform_$platform" "FuzzNewPlatform_$platform" > "$fixture/client.$platform.expected"
  printf '%s\n' all > "$fixture/core.$platform.expected"
  printf '%s\n' TestLargeInstructionsReachModel > "$fixture/engine.$platform.expected"
  printf '%s\n' TestConstrainedProfile > "$fixture/node.$platform.expected"
  printf '%s\n' TestAuthorizationFixedPortCallback TestAttachmentLocalIO TestUpdatePendingNativeInput TestSSHWindowsPairingGuidance > "$fixture/live.$platform.expected"
  printf '%s\n' TestCloseShortcut TestNativeClosePrompt TestMenu > "$fixture/shell.$platform.expected"
  SAILRY_CI_PLATFORM=$platform check_case "$platform-full" push '' "$core" "$engine" "$node" "$backend" "$live" "$shell"
  SAILRY_CI_PLATFORM=$platform check_case "$platform-pr" pull_request internal/client/commands.go "$backend" "$live" "$shell"
  SAILRY_CI_PLATFORM=$platform check_case "$platform-unrelated" pull_request README.md
done

for failure in build list test; do
  directory=$fixture/platform-failure-$failure
  mkdir "$directory"
  status=0
  (cd "$fixture/source" && RUNNER_TEMP=$directory GITHUB_EVENT_NAME=push SAILRY_CI_PLATFORM=windows SAILRY_CI_LANE=platform SAILRY_CI_FAILURE=$failure bash "$script" platform) > "$directory/output" 2>&1 || status=$?
  case "$failure" in build) expected=43 ;; list) expected=31 ;; test) expected=37 ;; esac
  test "$status" -eq "$expected" || { cat "$directory/output" >&2; echo "$failure failure returned $status, expected $expected" >&2; exit 1; }
done

# A stale curated selector is an error, never an empty -run or a broader run.
printf '%s\n' TestSharedEngineShouldNotRun > "$fixture/engine.list"
directory=$fixture/empty-selector
mkdir "$directory"
status=0
(cd "$fixture/source" && RUNNER_TEMP=$directory GITHUB_EVENT_NAME=push SAILRY_CI_PLATFORM=windows SAILRY_CI_LANE=platform bash "$script" platform) > "$directory/output" 2>&1 || status=$?
test "$status" -eq 1 && grep -q 'Platform selector matches no tests:' "$directory/output"

echo 'Test lane routing fixtures passed'
