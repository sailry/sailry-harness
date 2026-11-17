#!/usr/bin/env bash
# Discover checks and native artifacts already verified for this exact commit.
set -euo pipefail

: "${GH_REPO:?A repository is required}"
: "${GITHUB_SHA:?A source commit is required}"
: "${GITHUB_OUTPUT:?An output file is required}"
: "${RUNNER_TEMP:?A temporary directory is required}"
branch=${DEFAULT_BRANCH:-main}
ci_run=
native_run=
mobile_checked=false
outputs() {
  printf 'ci_run=%s\nmobile_checked=%s\nnative_run=%s\n' "$ci_run" "$mobile_checked" "$native_run" >> "$GITHUB_OUTPUT"
}
trap outputs EXIT
fallback() {
  printf '::notice::%s; running full CI.\n' "$1"
  exit 0
}

workflow=$RUNNER_TEMP/prior-ci-workflow.json
runs=$RUNNER_TEMP/prior-ci-runs.json
run=$RUNNER_TEMP/prior-ci-run.json
jobs=$RUNNER_TEMP/prior-ci-jobs.json
if ! gh api "repos/$GH_REPO/actions/workflows/ci.yml" > "$workflow"; then
  fallback 'Unable to read the CI workflow'
fi
workflow_id=$(jq -er 'select(.path == ".github/workflows/ci.yml") | .id | select(type == "number" and . > 0)' "$workflow") || fallback 'Unable to verify the CI workflow'
if ! gh api "repos/$GH_REPO/actions/workflows/$workflow_id/runs?event=push&branch=$branch&head_sha=$GITHUB_SHA&status=completed&per_page=100" > "$runs"; then
  fallback 'Unable to discover prior CI runs'
fi
identity='
  .workflow_id == $workflow and .path == ".github/workflows/ci.yml" and
  .head_sha == $sha and .head_branch == $branch and .event == "push" and
  .head_repository.full_name == $repo and .status == "completed" and .conclusion == "success"
'
candidate=$(jq -r --argjson workflow "$workflow_id" --arg sha "$GITHUB_SHA" --arg branch "$branch" --arg repo "$GH_REPO" \
  "[.workflow_runs[] | select($identity) | select(.id | type == \"number\" and . > 0)] | max_by(.id) | .id // empty" "$runs") || fallback 'Unable to verify prior CI runs'
[ -n "$candidate" ] || fallback 'No successful CI push run matches this commit'

# Recheck the selected run after discovery; a new attempt may have started.
if ! gh api "repos/$GH_REPO/actions/runs/$candidate" > "$run"; then
  fallback 'Unable to read the selected CI run'
fi
if ! jq -e --argjson workflow "$workflow_id" --argjson id "$candidate" --arg sha "$GITHUB_SHA" --arg branch "$branch" --arg repo "$GH_REPO" \
  ".id == \$id and ($identity)" "$run" >/dev/null; then
  fallback 'The selected CI run no longer matches successful checks'
fi
if ! gh api --paginate "repos/$GH_REPO/actions/runs/$candidate/jobs?filter=latest&per_page=100" --jq '.jobs[]' > "$jobs.lines"; then
  fallback 'Unable to read completed CI jobs'
fi
jq -s . "$jobs.lines" > "$jobs" || fallback 'Unable to verify completed CI jobs'
required='[
  "lint",
  "test (ubuntu-24.04, linux-0)",
  "test (ubuntu-24.04, linux-1)",
  "test (ubuntu-24.04, linux-2)",
  "test (ubuntu-24.04, linux-3)",
  "test (windows-2025, platform, windows-amd64)",
  "test (macos-15, platform, darwin-arm64)"
]'
complete='
  . as $jobs | all($required[]; . as $name |
    [$jobs[] | select(.name == $name)] as $matched |
    ($matched | length) == 1 and $matched[0].status == "completed" and $matched[0].conclusion == "success")
'
if ! jq -e --argjson required "$required" "$complete" "$jobs" >/dev/null; then
  fallback 'Required CI checks are missing or unsuccessful'
fi
checked_mobile=false
if jq -e --argjson required '["apps/mobile (Android)"]' "$complete" "$jobs" >/dev/null; then
  checked_mobile=true
fi

checked_native=
required_native='[
  "native (ubuntu-24.04, linux-amd64, x86_64-unknown-linux-gnu)",
  "native (ubuntu-24.04-arm, linux-arm64, aarch64-unknown-linux-gnu)",
  "native (macos-15, darwin-arm64, aarch64-apple-darwin)",
  "native (macos-15-intel, darwin-amd64, x86_64-apple-darwin)",
  "native (windows-2025, windows-amd64, x86_64-pc-windows-msvc)"
]'
if jq -e --argjson required "$required_native" "$complete" "$jobs" >/dev/null; then
  artifacts=$RUNNER_TEMP/prior-ci-artifacts.json
  if ! gh api --paginate "repos/$GH_REPO/actions/runs/$candidate/artifacts?per_page=100" --jq '.artifacts[]' > "$artifacts.lines"; then
    fallback 'Unable to read prior native artifacts'
  fi
  jq -s . "$artifacts.lines" > "$artifacts" || fallback 'Unable to verify prior native artifacts'
  if jq -e '
    . as $artifacts |
    all(["linux-amd64", "linux-arm64", "darwin-arm64", "darwin-amd64", "windows-amd64"][];
      . as $target | [$artifacts[] | select(.name == "native-" + $target)] as $matched |
      ($matched | length) == 1 and $matched[0].expired == false and
      ($matched[0].size_in_bytes | type) == "number" and $matched[0].size_in_bytes > 0)
  ' "$artifacts" >/dev/null; then
    checked_native=$candidate
  else
    echo '::notice::Prior native artifacts are missing, duplicated or expired; rebuilding native targets.'
  fi
else
  echo '::notice::Prior native jobs were not all successful; rebuilding native targets.'
fi

ci_run=$candidate
mobile_checked=$checked_mobile
native_run=$checked_native
printf 'Reusing verified CI run %s for %s.\n' "$ci_run" "$GITHUB_SHA"
