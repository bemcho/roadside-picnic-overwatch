#!/bin/bash
# Commit working-tree changes and merge them into main through a PR.
# Solo sync flow: branch -> commit -> push -> PR -> merge -> back on main.
# Useful when the "main branch protection" ruleset requires PRs again, or
# when you want a reviewable sync. Without the pull_request rule a plain
# `git push` on main is enough.
#
# Usage: tools/pr_sync.sh [commit message]
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
message="${1:-sync working tree}"
branch="sync-$(date +%Y%m%d-%H%M%S)"

git checkout -q -b "$branch"
if git diff --quiet --cached && git diff --quiet &&
   [ -z "$(git ls-files --others --exclude-standard)" ]; then
    echo "nothing to commit; back on $(git rev-parse --abbrev-ref HEAD)"
    git checkout -q main
    exit 0
fi
git add -A
git commit -m "$message"
git push -u origin "$branch"
gh pr create --fill --base main --head "$branch"
for _ in 1 2 3 4 5; do
    if gh pr merge "$branch" --merge --delete-branch; then
        git checkout -q main
        git pull -q --ff-only
        git branch -d "$branch"
        echo "merged and back on main"
        exit 0
    fi
    sleep 3
done
echo "PR created but not mergeable yet; merge manually: gh pr merge '$branch' --merge" >&2
exit 1
