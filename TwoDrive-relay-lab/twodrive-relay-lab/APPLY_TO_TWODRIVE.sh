#!/usr/bin/env bash
set -euo pipefail
repo="${1:-$HOME/software/twodrive}"
self="$(cd -- "$(dirname -- "$0")" && pwd)"
dst="$repo/experiments/twodrive-relay-lab"
if [[ ! -d "$repo/.git" ]]; then
  echo "Not a git repository: $repo" >&2
  exit 1
fi
if [[ -e "$dst" ]]; then
  echo "Refusing to overwrite existing $dst" >&2
  exit 1
fi
mkdir -p "$(dirname -- "$dst")"
cp -a "$self" "$dst"
rm -f "$dst/APPLY_TO_TWODRIVE.sh"
echo "Copied experiment to: $dst"
echo "Main TwoDrive workspace files were not modified."
echo
printf 'Suggested next steps:\n  cd %q\n  cargo test\n  cargo build --release\n' "$dst"
