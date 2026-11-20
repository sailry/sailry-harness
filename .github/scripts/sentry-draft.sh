#!/usr/bin/env bash
set -euo pipefail

[[ $FINGERPRINT =~ ^[0-9a-f]{64}$ && $VERIFIED_TREE =~ ^[0-9a-f]{40}$ && $TARGET =~ ^[0-9a-f]{40}$ ]] || exit 1
branch="sentry-repair/$FINGERPRINT"
ref="refs/heads/$branch"
report() { printf '%s\n' "$1" | tee -a "$GITHUB_STEP_SUMMARY"; }

# Closed (including merged) drafts count too: a repeated Sentry event must
# not undo a maintainer's decision. API errors must not mean "no PR".
prior=$(gh api --method GET "repos/$GITHUB_REPOSITORY/pulls" -f state=all \
  -f "head=$GITHUB_REPOSITORY_OWNER:$branch" -F per_page=1 --jq '.[0].number // empty')
if [[ -n $prior ]]; then
  report "Already handled by PR #$prior; no new draft."
  exit 0
fi
base_sha=$(git ls-remote --exit-code origin "refs/heads/$BASE" | cut -f1)
if [[ $base_sha != "$GITHUB_SHA" ]]; then
  report 'Default branch changed after verification. Rerun against its new tip; no draft.'
  exit 0
fi

git apply --cached .github/scripts/sentry-candidates/queue-stopping.patch
tree=$(git write-tree)
base_tree=$(git rev-parse 'HEAD^{tree}')
if [[ $tree != "$VERIFIED_TREE" || $tree == "$base_tree" ]]; then
  report 'Reconstructed tree does not match a verified nonempty change; no draft.'
  exit 1
fi
remote=$(git ls-remote origin "$ref")
if [[ -n $remote ]]; then
  # Recover only an interrupted create with the same verified parent and tree.
  git fetch --quiet --depth=2 origin "$ref"
  [[ $(git show -s --format='%P %T' FETCH_HEAD) == "$GITHUB_SHA $tree" ]] || {
    report 'Fingerprint branch contains different work. Inspect it manually; no draft.'
    exit 1
  }
else
  commit=$(git -c user.name='github-actions[bot]' -c user.email='41898282+github-actions[bot]@users.noreply.github.com' \
    commit-tree "$tree" -p "$GITHUB_SHA" -m 'Fix stopping queue guidance')
  git push origin "$commit:$ref"
fi
body="Fingerprint: \`$FINGERPRINT\`

The fixed queue-stopping probe matched its assertion on \`$TARGET\` and the default branch \`$GITHUB_SHA\`. The same probe passed on candidate tree \`$tree\`.

Verification: $GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID"
url=$(gh pr create --repo "$GITHUB_REPOSITORY" --draft --base "$BASE" --head "$branch" \
  --title 'Fix stopping queue guidance' --body "$body")
report "Created draft: $url"
