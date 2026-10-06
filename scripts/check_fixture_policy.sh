#!/usr/bin/env bash
# check_fixture_policy.sh: golden replay fixtures change only together with a
# rules_version bump (docs/DETERMINISM.md, "Golden fixtures").
#
# Compares HEAD against a base commit. If any existing file under
# crates/sim/tests/fixtures/ (*.eonreplay, *.hash) was modified, data/rules/rules.ron
# must change its `rules_version` line to a larger number in the same range.
# A changed fixture hash without that bump is a bug to bisect, not a fixture
# update. Newly added fixtures are a first recording and need no bump. CI runs
# this on every pull request against the PR base; locally `just fixture-policy`
# uses the merge base with main.
#
# Usage: scripts/check_fixture_policy.sh <base-commit>
set -euo pipefail

base="${1:?usage: scripts/check_fixture_policy.sh <base-commit>}"
cd "$(dirname "$0")/.."

changed="$(git diff --name-only --diff-filter=M "$base" HEAD -- crates/sim/tests/fixtures/ \
  | grep -E '\.(eonreplay|hash)$' || true)"
if [ -z "$changed" ]; then
  echo "fixture policy: no existing golden fixture modified since ${base:0:12}"
  exit 0
fi

version_at() {
  git show "$1:data/rules/rules.ron" | sed -nE 's/^[[:space:]]*rules_version:[[:space:]]*([0-9]+),?.*/\1/p' | head -n 1
}
old="$(version_at "$base")"
new="$(version_at HEAD)"
if [ -n "$old" ] && [ -n "$new" ] && [ "$new" -gt "$old" ]; then
  echo "fixture policy: fixtures changed with rules_version $old -> $new:"
  echo "$changed" | sed 's/^/  /'
  exit 0
fi

echo "::error::golden fixtures changed without a rules_version bump in data/rules/rules.ron (was ${old:-?}, now ${new:-?}); see docs/DETERMINISM.md, Golden fixtures" >&2
echo "$changed" | sed 's/^/  /' >&2
exit 1
