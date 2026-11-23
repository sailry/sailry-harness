#!/usr/bin/env bash
# Exercise the real lane script with synthetic package graphs and Go test files.
set -euo pipefail

source=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/sailry-ci-lanes-XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
export SAILRY_CI_FIXTURE=$fixture
mkdir "$fixture/bin"
mkdir "$fixture/source"
mkdir "$fixture/scripts"
cp "$source/test.sh" "$source/test-costs.tsv" "$source/platform-tests.tsv" "$fixture/scripts/"
script=$fixture/scripts/test.sh
export PATH="$fixture/bin:$PATH"

module=example.test/sailry
backend=$module/internal/client
i18n=$module/internal/desktop/i18n
live=$module/internal/desktop/live
shell=$module/internal/desktop/shell
printf '%s\n' "$module" > "$fixture/module"

write_test_file() {
  local rel=$1
  shift
  mkdir -p "$fixture/source/$(dirname "$rel")"
  {
    printf 'package fixture\n\n'
    for fn in "$@"; do
      printf 'func %s() {}\n' "$fn"
    done
  } > "$fixture/source/$rel"
}

printf '%s\n' TestLiveOne TestLiveTwo ExampleLive FuzzLive TestLiveThree TestLiveFour TestLiveFive > "$fixture/live.names"
printf '%s\n' ExampleShell TestShellOne FuzzShell TestShellTwo TestShellThree > "$fixture/shell.names"

write_test_file internal/client/client_test.go TestClientOne
write_test_file internal/desktop/i18n/i18n_test.go TestI18nOne
write_test_file internal/desktop/live/live_test.go TestLiveOne TestLiveTwo ExampleLive FuzzLive TestLiveThree TestLiveFour TestLiveFive BenchmarkIgnored TestMain
write_test_file internal/desktop/shell/shell_test.go ExampleShell TestShellOne FuzzShell TestShellTwo TestShellThree BenchmarkIgnored TestMain

{
  printf '%s\tclient_test.go\n' "$backend"
  printf '%s\ti18n_test.go\n' "$i18n"
  printf '%s\tlive_test.go\n' "$live"
  printf '%s\tshell_test.go\n' "$shell"
} > "$fixture/linux.metadata"

cat > "$fixture/bin/go" <<'GO'
#!/usr/bin/env bash
set -euo pipefail
case "$1 ${2:-}" in
  'list -m') cat "$SAILRY_CI_FIXTURE/module" ;;
  'env GOOS') printf '%s\n' "$SAILRY_CI_PLATFORM" ;;
  'build ./cmd/sailry-desktop')
    test "$#" -eq 3 && test "$3" = ./cmd/sailry-host
    test "${SAILRY_CI_FAILURE:-}" != build || exit 43
    printf '%s\n' "$SAILRY_CI_PLATFORM" >> "$SAILRY_CI_FIXTURE/builds"
    ;;
  'list -f')
    test "$#" -eq 4 && test "$4" = ./...
    test "${SAILRY_CI_FAILURE:-}" != list || exit 31
    cat "$SAILRY_CI_FIXTURE/${SAILRY_CI_PLATFORM:-linux}.metadata"
    ;;
  'list -tags')
    test "$3" = integration && test "$4" = -f
    cat "$SAILRY_CI_FIXTURE/integration.metadata"
    ;;
  'test -p')
    test "$3" = 1 -o "$3" = 2
    test "$4" = -timeout && test "$5" = 20m
    test "${SAILRY_CI_FAILURE:-}" != test || exit 37
    shift 5
    if test "${1:-}" = -tags; then
      test "$2" = integration
      shift 2
      test "$#" -eq 3 && test "$2" = -run
      printf '%s\t%s\t%s\n' "$SAILRY_CI_LANE" "$1" "$3" >> "$SAILRY_CI_FIXTURE/calls"
    elif test "$#" -eq 3 && test "$2" = -run; then
      printf '%s\t%s\t%s\n' "$SAILRY_CI_LANE" "$1" "$3" >> "$SAILRY_CI_FIXTURE/calls"
    else
      for package in "$@"; do printf '%s\t%s\tall\n' "$SAILRY_CI_LANE" "$package"; done >> "$SAILRY_CI_FIXTURE/calls"
    fi
    ;;
  'test -timeout')
    test "$3" = 20m
    test "${SAILRY_CI_FAILURE:-}" != test || exit 37
    shift 3
    for package in "$@"; do printf '%s\t%s\tall\n' "$SAILRY_CI_LANE" "$package"; done >> "$SAILRY_CI_FIXTURE/calls"
    ;;
  *) echo "Unexpected Go command: $*" >&2; exit 1 ;;
esac
GO
chmod +x "$fixture/bin/go"

check_case() {
  local name=$1 directory package lane lanes
  shift
  directory=$fixture/$name
  mkdir "$directory"
  : > "$fixture/calls"
  : > "$fixture/builds"
  lanes=(linux-0 linux-1 linux-2 linux-3)
  [ -z "${SAILRY_CI_PLATFORM:-}" ] || lanes=(platform)
  for lane in "${lanes[@]}"; do
    mkdir "$directory/$lane"
    if ! (cd "$fixture/source" && RUNNER_TEMP=$directory/$lane SAILRY_CI_LANE=$lane bash "$script" "$lane") > "$directory/$lane/output" 2>&1; then
      cat "$directory/$lane/output" >&2
      exit 1
    fi
  done
  # Expand each actual anchored -run pattern into the names it selects.
  # Comparing multisets proves every expected parent runs exactly once.
  awk -F '\t' '
    $3 == "all" {
      if ($1 !~ /^linux-[0123]$/ && $1 != "platform") exit 1
      print $2 "\tall"; next
    }
    {
      if (($1 !~ /^linux-[0123]$/ && $1 != "platform") || substr($3, 1, 2) != "^(" || substr($3, length($3)-1) != ")$") exit 1
      count = split(substr($3, 3, length($3)-4), names, /\|/)
      for (i = 1; i <= count; i++) print $2 "\t" names[i]
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
  cp "$fixture/calls" "$directory/calls"
  if [ -n "${SAILRY_CI_PLATFORM:-}" ]; then
    test "$(cat "$fixture/builds")" = "$SAILRY_CI_PLATFORM"
  else
    test ! -s "$fixture/builds"
  fi
}

check_case full "$backend" "$i18n" "$live" "$shell"

# Duration placement must balance cost rather than an equal number of names.
# All other fixture parents use the 1s default, so the optimum is 12/12/11/11.
cat > "$fixture/scripts/test-costs.tsv" <<'COSTS'
internal/client	*	12
internal/desktop/i18n	*	8
internal/desktop/live	TestLiveOne	10
internal/desktop/shell	TestShellOne	6
COSTS
check_case balanced "$backend" "$i18n" "$live" "$shell"
routing_plan() {
  awk -F '\t' '
    $3 == "all" { print $2 "\t*\t" $1; next }
    {
      count = split(substr($3, 3, length($3)-4), names, /\|/)
      for (i = 1; i <= count; i++) print $2 "\t" names[i] "\t" $1
    }
  ' "$1" | sort
}
routing_plan "$fixture/balanced/calls" > "$fixture/balanced/plan"
awk -F '\t' -v module="$module" '
  FILENAME == ARGV[1] { costs[$1, $2] = $3; next }
  {
    package = substr($1, length(module) + 2)
    key = package SUBSEP $2
    cost = key in costs ? costs[key] : ($2 == "*" ? 2 : 1)
    loads[$3] += cost; counts[$3]++
  }
  END {
    low = high = loads["linux-0"]
    fewest = most = counts["linux-0"]
    for (i = 1; i < 4; i++) {
      lane = "linux-" i
      if (loads[lane] < low) low = loads[lane]
      if (loads[lane] > high) high = loads[lane]
      if (counts[lane] < fewest) fewest = counts[lane]
      if (counts[lane] > most) most = counts[lane]
    }
    if (low != 11 || high != 12 || most - fewest < 2) exit 1
  }
' "$fixture/scripts/test-costs.tsv" "$fixture/balanced/plan"

# Enumeration order must not change any unit's runner, including equal weights.
sort -r "$fixture/linux.metadata" -o "$fixture/linux.metadata"
check_case reordered "$backend" "$i18n" "$live" "$shell"
routing_plan "$fixture/reordered/calls" > "$fixture/reordered/plan"
diff -u "$fixture/balanced/plan" "$fixture/reordered/plan"

# Package and parent discovery is open: no duration hint is needed to run new units.
new_package=$module/internal/new-package
write_test_file internal/new-package/new_test.go TestNewPackage
printf '%s\tnew_test.go\n' "$new_package" >> "$fixture/linux.metadata"
printf '%s\n' TestNewParent ExampleNewParent FuzzNewParent >> "$fixture/live.names"
write_test_file internal/desktop/live/live_test.go $(cat "$fixture/live.names")
check_case new-units "$backend" "$i18n" "$live" "$shell" "$new_package"

# Fewer parents than lanes leave a lane empty, without an unfiltered test run.
printf '%s\n' TestOnlyParent > "$fixture/live.names"
write_test_file internal/desktop/live/live_test.go TestOnlyParent
check_case sparse "$backend" "$i18n" "$live" "$shell" "$new_package"

for failure in list test; do
  directory=$fixture/failure-$failure
  mkdir "$directory"
  status=0
  (cd "$fixture/source" && RUNNER_TEMP=$directory SAILRY_CI_LANE=linux-0 SAILRY_CI_FAILURE=$failure bash "$script" linux-0) > "$directory/output" 2>&1 || status=$?
  case "$failure" in list) expected=31 ;; test) expected=37 ;; esac
  test "$status" -eq "$expected" || { cat "$directory/output" >&2; echo "$failure failure returned $status, expected $expected" >&2; exit 1; }
done

# Native lanes verify command linking, batch curated "all" packages, and select
# curated filters plus OS-constrained tests (ignoring feature-only build tags).
core=$module/internal/core
engine=$module/internal/node/agent/engine
node=$module/internal/node
shipped=$module/internal/shipped
codeindex=$module/modules/code-index/node
kit=$module/internal/desktop/kit
panel=$module/internal/desktop/panel

write_test_file internal/node/agent/engine/shared_test.go TestLargeInstructionsReachModel TestSharedEngineShouldNotRun
write_test_file internal/node/shared_test.go TestCopilotSignIn TestProfileLeftovers TestSharedNodeShouldNotRun
write_test_file internal/shipped/shared_test.go TestSeal TestSharedShippedShouldNotRun
write_test_file internal/desktop/kit/shared_test.go TestNativeConfirmation TestAlertDialogUsesNativeConfirmation TestDialogKeepsPressWhenEarlierOverlayAppears
write_test_file internal/desktop/panel/shared_test.go TestEditorTakesFocusBesideTree
write_test_file internal/desktop/live/shared_test.go TestAuthorizationFixedPortCallback TestAttachmentLocalIO TestUpdatePendingNativeInput TestSharedLiveShouldNotRun
write_test_file internal/desktop/shell/shared_test.go TestCloseShortcut TestNativeClosePrompt TestMenu TestSharedShellShouldNotRun
write_test_file internal/core/shared_test.go TestCoreOne
write_test_file internal/desktop/i18n/shared_test.go TestI18nOne
cat > "$fixture/source/internal/node/constrained_test.go" <<'SOURCE'
//go:build darwin || windows

package fixture

func TestConstrainedProfile() {}
func TestMain() {}
SOURCE
mkdir -p "$fixture/source/modules/code-index/node"
cat > "$fixture/source/modules/code-index/node/feature_test.go" <<'SOURCE'
//go:build !sailry_no_code_index

package fixture

func TestFeatureOnlyShouldNotRunOnPlatform() {}
SOURCE

for platform in darwin windows; do
  : > "$fixture/$platform.metadata"
  # Include every package named in platform-tests.tsv so the missing-package guard succeeds.
  awk -F '\t' -v module="$module" '!/^#/ && NF {
    pkg = module "/" $1
    if ($2 == "all" && $1 != "internal/core") print pkg
  }' "$fixture/scripts/platform-tests.tsv" > "$fixture/$platform.all.pkgs"
  while IFS= read -r pkg; do
    printf '%s\n' "$pkg" >> "$fixture/$platform.metadata"
  done < "$fixture/$platform.all.pkgs"
  for package in "$i18n" "$live" "$shell" "$core" "$engine" "$shipped" "$kit" "$panel"; do
    printf '%s\tshared_test.go\n' "$package" >> "$fixture/$platform.metadata"
  done
  printf '%s\tshared_test.go\tconstrained_test.go\n' "$node" >> "$fixture/$platform.metadata"
  printf '%s\tfeature_test.go\n' "$codeindex" >> "$fixture/$platform.metadata"
  printf '%s\tclient_test.go\tnew_%s_test.go\n' "$backend" "$platform" >> "$fixture/$platform.metadata"
  cat > "$fixture/source/internal/client/new_${platform}_test.go" <<SOURCE
package fixture

func TestNewPlatform_${platform}() {}
func ExampleNewPlatform_${platform}() {}
func FuzzNewPlatform_${platform}() {}
SOURCE
  printf '%s\n' "TestNewPlatform_$platform" "ExampleNewPlatform_$platform" "FuzzNewPlatform_$platform" > "$fixture/client.$platform.expected"
  printf '%s\n' all > "$fixture/core.$platform.expected"
  printf '%s\n' TestLargeInstructionsReachModel > "$fixture/engine.$platform.expected"
  printf '%s\n' TestConstrainedProfile TestCopilotSignIn TestProfileLeftovers > "$fixture/node.$platform.expected"
  printf '%s\n' TestSeal > "$fixture/shipped.$platform.expected"
  printf '%s\n' TestNativeConfirmation TestAlertDialogUsesNativeConfirmation TestDialogKeepsPressWhenEarlierOverlayAppears > "$fixture/kit.$platform.expected"
  printf '%s\n' TestEditorTakesFocusBesideTree > "$fixture/panel.$platform.expected"
  printf '%s\n' TestAuthorizationFixedPortCallback TestAttachmentLocalIO TestUpdatePendingNativeInput > "$fixture/live.$platform.expected"
  printf '%s\n' TestCloseShortcut TestNativeClosePrompt TestMenu > "$fixture/shell.$platform.expected"
  all_extra=()
  while IFS= read -r pkg; do
    all_extra+=("$pkg")
    printf '%s\n' all > "$fixture/${pkg##*/}.$platform.expected"
  done < "$fixture/$platform.all.pkgs"
  SAILRY_CI_PLATFORM=$platform check_case "$platform-full" "$core" "$engine" "$node" "$shipped" "$kit" "$panel" "$backend" "$live" "$shell" "${all_extra[@]}"
done

for failure in build list test; do
  directory=$fixture/platform-failure-$failure
  mkdir "$directory"
  status=0
  (cd "$fixture/source" && RUNNER_TEMP=$directory SAILRY_CI_PLATFORM=windows SAILRY_CI_LANE=platform SAILRY_CI_FAILURE=$failure bash "$script" platform) > "$directory/output" 2>&1 || status=$?
  case "$failure" in build) expected=43 ;; list) expected=31 ;; test) expected=37 ;; esac
  test "$status" -eq "$expected" || { cat "$directory/output" >&2; echo "$failure failure returned $status, expected $expected" >&2; exit 1; }
done

# A stale curated selector is an error, never an empty -run or a broader run.
write_test_file internal/node/agent/engine/shared_test.go TestSharedEngineShouldNotRun
directory=$fixture/empty-selector
mkdir "$directory"
status=0
(cd "$fixture/source" && RUNNER_TEMP=$directory SAILRY_CI_PLATFORM=windows SAILRY_CI_LANE=platform bash "$script" platform) > "$directory/output" 2>&1 || status=$?
test "$status" -eq 1 && grep -q 'Platform selector matches no tests:' "$directory/output"

# A suite entry whose package no longer exists is an error too.
printf 'internal/removed\tall\n' >> "$fixture/scripts/platform-tests.tsv"
directory=$fixture/missing-package
mkdir "$directory"
status=0
(cd "$fixture/source" && RUNNER_TEMP=$directory SAILRY_CI_PLATFORM=windows SAILRY_CI_LANE=platform bash "$script" platform) > "$directory/output" 2>&1 || status=$?
test "$status" -eq 1 && grep -q 'Platform suite names a missing package: internal/removed' "$directory/output"

# Integration mode runs only tests declared in //go:build integration files.
mkdir -p "$fixture/source/internal/node/store"
cat > "$fixture/source/internal/node/store/unit_test.go" <<'SOURCE'
package store

func TestDefaultUnitShouldNotRerun() {}
SOURCE
cat > "$fixture/source/internal/node/store/load_integration_test.go" <<'SOURCE'
//go:build integration

package store

func TestStorageLoad() {}
func TestHotPaths() {}
SOURCE
cat > "$fixture/integration.metadata" <<META
$module/internal/node/store	unit_test.go	load_integration_test.go
$module/internal/core	shared_test.go
META
directory=$fixture/integration-mode
mkdir "$directory"
: > "$fixture/calls"
(cd "$fixture/source" && RUNNER_TEMP=$directory SAILRY_CI_LANE=integration bash "$script" integration ./internal/node/store ./internal/core) > "$directory/output" 2>&1
diff -u <(printf 'integration\t%s/internal/node/store\t^(TestHotPaths|TestStorageLoad)$\n' "$module") "$fixture/calls"

echo 'Test lane routing fixtures passed'
