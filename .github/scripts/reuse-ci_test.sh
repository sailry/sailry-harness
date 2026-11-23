#!/usr/bin/env bash
# Check exact-source reuse with a read-only GitHub API fixture.
set -euo pipefail

script=$(cd "$(dirname "$0")" && pwd)/reuse-ci.sh
fixture=$(mktemp -d "${TMPDIR:-/tmp}/sailry-ci-reuse-XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
mkdir "$fixture/bin" "$fixture/base"
export PATH="$fixture/bin:$PATH"
repo=example/sailry
sha=1111111111111111111111111111111111111111
jq -n '{id:7,path:".github/workflows/ci.yml"}' > "$fixture/base/workflow.json"
jq -n --arg sha "$sha" --arg repo "$repo" '{
  id:101, workflow_id:7, path:".github/workflows/ci.yml", head_sha:$sha,
  head_branch:"main", event:"push", head_repository:{full_name:$repo},
  status:"completed", conclusion:"success"
}' > "$fixture/base/run.json"
jq '{workflow_runs:[.]}' "$fixture/base/run.json" > "$fixture/base/runs.json"
jq -n '{jobs:([
  "lint",
  "race",
  "test (ubuntu-24.04, linux-0)",
  "test (ubuntu-24.04, linux-1)",
  "test (ubuntu-24.04, linux-2)",
  "test (ubuntu-24.04, linux-3)",
  "test (windows-2025, platform, windows-amd64)",
  "test (macos-15, platform, darwin-arm64)",
  "apps/mobile (Android)"
] | map({name:.,status:"completed",conclusion:"success"}))}' > "$fixture/base/jobs.json"

cat > "$fixture/bin/gh" <<'GH'
#!/usr/bin/env bash
set -euo pipefail
test "$1" = api
shift
if [ "${1:-}" = --paginate ]; then shift; fi
path=${1:?An API path is required}
shift
query=
if [ "$#" -gt 0 ]; then
  test "$1" = --jq && test "$#" -eq 2
  query=$2
fi
case "$path" in
  repos/example/sailry/actions/workflows/ci.yml) part=workflow ;;
  repos/example/sailry/actions/workflows/*/runs\?*)
    case "$path" in *'event=push&branch=main&head_sha=1111111111111111111111111111111111111111&status=completed&per_page=100') ;; *) exit 32 ;; esac
    part=runs
    ;;
  repos/example/sailry/actions/runs/*/jobs\?*) test "$query" = '.jobs[]'; part=jobs ;;
  repos/example/sailry/actions/runs/*)
    test "${path##*/}" = "$(jq -r .id "$SAILRY_REUSE_FIXTURE/data/run.json")"
    part=run
    ;;
  *) echo "Unexpected API path: $path" >&2; exit 33 ;;
esac
printf '%s\n' "$part" >> "$SAILRY_REUSE_FIXTURE/calls"
if [ "${SAILRY_REUSE_FAILURE:-}" = "$part" ]; then
  echo 'Fixture API unavailable' >&2
  exit 23
fi
if [ -n "$query" ]; then
  jq -c "$query" "$SAILRY_REUSE_FIXTURE/data/$part.json"
else
  cat "$SAILRY_REUSE_FIXTURE/data/$part.json"
fi
GH
chmod +x "$fixture/bin/gh"

prepare() {
  current=$fixture/$1
  mkdir -p "$current/data" "$current/tmp"
  cp "$fixture/base/"*.json "$current/data/"
}
change() {
  local file=$1 expression=$2
  jq "$expression" "$current/data/$file.json" > "$current/data/changed.json"
  mv "$current/data/changed.json" "$current/data/$file.json"
}
check() {
  local ci=$1 mobile=$2
  : > "$current/output"
  SAILRY_REUSE_FIXTURE=$current GH_REPO=$repo GITHUB_SHA=$sha DEFAULT_BRANCH=main \
    GITHUB_OUTPUT=$current/output RUNNER_TEMP=$current/tmp bash "$script" > "$current/log" 2>&1 || {
      cat "$current/log" >&2
      echo "Reuse discovery failed: $current" >&2
      exit 1
    }
  printf 'ci_run=%s\nmobile_checked=%s\n' "$ci" "$mobile" > "$current/expected"
  diff -u "$current/expected" "$current/output"
  if [ -z "$ci" ]; then grep -q '::notice::' "$current/log"; fi
  test "$(wc -l < "$current/calls")" -le 4
}

prepare complete
check 101 true

prepare no-runs
change runs '.workflow_runs=[]'
check '' false

# Every source identity field must match, not just the successful conclusion.
for mismatch in sha branch repo event manual workflow path status conclusion; do
  prepare "wrong-$mismatch"
  case "$mismatch" in
    sha) expression='.head_sha="2222222222222222222222222222222222222222"' ;;
    branch) expression='.head_branch="topic"' ;;
    repo) expression='.head_repository.full_name="foreign/sailry"' ;;
    event) expression='.event="pull_request"' ;;
    manual) expression='.event="workflow_dispatch"' ;;
    workflow) expression='.workflow_id=8' ;;
    path) expression='.path=".github/workflows/release.yml"' ;;
    status) expression='.status="in_progress"' ;;
    conclusion) expression='.conclusion="failure"' ;;
  esac
  change run "$expression"
  jq '{workflow_runs:[.]}' "$current/data/run.json" > "$current/data/runs.json"
  check '' false
done

prepare wrong-official-workflow
change workflow '.path=".github/workflows/release.yml"'
check '' false

prepare selected-run-changed
change run '.head_sha="2222222222222222222222222222222222222222"'
check '' false

# Discovery chooses the latest matching success, ignoring newer foreign runs.
prepare latest-matching
change run '.id=202'
jq '{workflow_runs:[(. + {id:101}),.,(. + {id:203,event:"pull_request"}),(. + {id:204,conclusion:"failure"})]}' \
  "$current/data/run.json" > "$current/data/runs.json"
check 202 true

prepare missing-linux-job
change jobs '.jobs |= map(select(.name != "test (ubuntu-24.04, linux-3)"))'
check '' false

prepare missing-platform-job
change jobs '.jobs |= map(select(.name != "test (windows-2025, platform, windows-amd64)"))'
check '' false

prepare failed-lint
change jobs '.jobs |= map(if .name == "lint" then .conclusion="failure" else . end)'
check '' false

prepare skipped-test
change jobs '.jobs |= map(if .name == "test (ubuntu-24.04, linux-1)" then .conclusion="skipped" else . end)'
check '' false

prepare duplicate-test-job
change jobs '.jobs += [.jobs[1]]'
check '' false

prepare skipped-mobile
change jobs '.jobs |= map(if .name == "apps/mobile (Android)" then .conclusion="skipped" else . end)'
check 101 false

for part in workflow runs run jobs; do
  prepare "api-failure-$part"
  SAILRY_REUSE_FAILURE=$part check '' false
done

for part in workflow runs run jobs; do
  prepare "invalid-json-$part"
  printf '{invalid' > "$current/data/$part.json"
  check '' false
done

echo 'Exact-source CI reuse fixtures passed'
