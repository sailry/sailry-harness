#!/usr/bin/env bash
# Exercise the publisher without a repository credential, commit or network.
set -euo pipefail
script=$(cd "$(dirname "$0")" && pwd)/sentry-draft.sh
fixture=$(mktemp -d "${TMPDIR:-/tmp}/sailry-draft-XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
export PATH="$fixture:$PATH" GITHUB_STEP_SUMMARY="$fixture/summary" effects="$fixture/effects"
export BASE=main GITHUB_REPOSITORY=example/harness GITHUB_REPOSITORY_OWNER=example
export GITHUB_SERVER_URL=https://example.invalid GITHUB_RUN_ID=1
export GITHUB_SHA=1111111111111111111111111111111111111111 VERIFIED_TREE=2222222222222222222222222222222222222222
export TARGET=3333333333333333333333333333333333333333 FINGERPRINT=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
cat > "$fixture/gh" <<'SH'
#!/usr/bin/env bash
set -eu
case "$1 $2" in
'api --method')
  [[ $3 == GET && "$*" == *'state=all'* && "$*" == *"head=example:sentry-repair/$FINGERPRINT"* ]] || exit 93
  case "$scenario" in
    open) echo 81;;
    closed) echo 82;;
    unavailable) exit 52;;
  esac
  ;;
'pr create')
  [[ " $* " == *' --draft '* && " $* " == *' --base main '* && " $* " == *" --head sentry-repair/$FINGERPRINT "* ]] || exit 94
  echo draft >> "$effects"; echo 'https://example.invalid/1';;
*) exit 91;;
esac
SH
cat > "$fixture/git" <<'SH'
#!/usr/bin/env bash
set -eu
case "$1" in
ls-remote)
  if [[ ${!#} == refs/heads/main ]]; then
    if [[ $scenario == stale ]]; then echo old; else echo "$GITHUB_SHA"; fi
  elif [[ $scenario == recover || $scenario == occupied ]]; then
    echo existing
  fi;;
apply) [[ $2 == --cached ]];;
write-tree) if [[ $scenario == mismatch ]]; then echo wrong; else echo "$VERIFIED_TREE"; fi;;
rev-parse) echo base-tree;;
fetch) [[ $2 == --quiet ]];;
show) if [[ $scenario == occupied ]]; then echo unrelated; else echo "$GITHUB_SHA $VERIFIED_TREE"; fi;;
-c) [[ " $* " == *" commit-tree $VERIFIED_TREE -p $GITHUB_SHA "* ]] || exit 95; echo tip;;
push) [[ $2 == origin && $3 == "tip:refs/heads/sentry-repair/$FINGERPRINT" && $# == 3 ]] || exit 96; echo push >> "$effects";;
*) exit 92;;
esac
SH
chmod +x "$fixture/git" "$fixture/gh"
export scenario
for scenario in open closed unavailable stale fresh recover occupied mismatch; do
  : > "$effects"
  status=0
  bash "$script" > /dev/null || status=$?
  case "$scenario" in
    unavailable|occupied|mismatch) [[ $status != 0 && ! -s $effects ]] || exit 1;;
    open|closed|stale) [[ $status == 0 && ! -s $effects ]] || exit 1;;
    recover) [[ $status == 0 && $(cat "$effects") == draft ]] || exit 1;;
    fresh) [[ $status == 0 && $(cat "$effects") == $'push\ndraft' ]] || exit 1;;
  esac
done
