#!/usr/bin/env bash
set -euo pipefail
repo="${1:-$HOME/software/twodrive}"
branch="${TWODRIVE_RELAY_BRANCH:-exp/encrypted-relays}"
self="$(cd -- "$(dirname -- "$0")" && pwd)"

if [[ ! -d "$repo/.git" ]]; then
  echo "Not a git repository: $repo" >&2
  exit 1
fi

# Keep the user's current TwoDrive checkout untouched by doing all work in a temporary worktree.
git -C "$repo" fetch origin main
workroot="$(mktemp -d -t twodrive-relay-publish.XXXXXX)"
cleanup() {
  git -C "$repo" worktree remove --force "$workroot/worktree" >/dev/null 2>&1 || true
  rm -rf -- "$workroot"
}
trap cleanup EXIT

if git -C "$repo" show-ref --verify --quiet "refs/heads/$branch"; then
  echo "Local branch already exists: $branch" >&2
  echo "Refusing to overwrite it. Set TWODRIVE_RELAY_BRANCH to a new experimental branch name." >&2
  exit 1
fi
if git -C "$repo" ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
  echo "Remote branch already exists: origin/$branch" >&2
  echo "Refusing to overwrite it. Set TWODRIVE_RELAY_BRANCH to a new experimental branch name." >&2
  exit 1
fi

git -C "$repo" worktree add -b "$branch" "$workroot/worktree" origin/main
mkdir -p "$workroot/worktree/experiments"
cp -a "$self" "$workroot/worktree/experiments/twodrive-relay-lab"
rm -f \
  "$workroot/worktree/experiments/twodrive-relay-lab/APPLY_TO_TWODRIVE.sh" \
  "$workroot/worktree/experiments/twodrive-relay-lab/PUBLISH_EXPERIMENT.sh"
mkdir -p "$workroot/worktree/.github/workflows"
cp "$self/ci/relay-lab.yml" "$workroot/worktree/.github/workflows/relay-lab.yml"
# The workflow template has been installed at repository level; don't duplicate it in the experiment.
rm -rf "$workroot/worktree/experiments/twodrive-relay-lab/ci"

git -C "$workroot/worktree" add experiments/twodrive-relay-lab .github/workflows/relay-lab.yml
git -C "$workroot/worktree" commit -m "experiment: add encrypted OneDrive and VPS relay lab"
git -C "$workroot/worktree" push -u origin "$branch"

printf '\nPublished experimental branch: %s\n' "$branch"
printf 'Current working tree left untouched: %s\n' "$repo"
printf 'GitHub Actions will build/test the relay lab on Windows and Ubuntu.\n'
