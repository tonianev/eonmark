#!/usr/bin/env bash
# check_fixture_policy.sh: golden replay fixtures change only together with a
# rules_version bump (docs/DETERMINISM.md, "Golden fixtures").
#
# Compares HEAD against a base commit. If any golden fixture
# (*.eonreplay, *.hash) that exists in the base has different content in HEAD,
# data/rules/rules.ron must change its `rules_version` line to a larger number
# in the same range. A changed fixture hash without that bump is a bug to
# bisect, not a fixture update. Newly added fixtures are a first recording and
# need no bump. CI runs this on every pull request against the PR base; locally
# `just fixture-policy` uses the merge base with main.
#
# The fixtures lived in crates/sim/tests/fixtures/ until the M8 prep moved them
# to fixtures/ (docs/PLAN.md, X1), so a base from before the move works too.
# Files are matched by name, not by git's rename detection (which would see a
# small .hash file that moved and changed as a delete plus an add).
#
# Usage: scripts/check_fixture_policy.sh <base-commit>
#        scripts/check_fixture_policy.sh --self-test
set -euo pipefail

if [ "${1:-}" = "--self-test" ]; then
  self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  cd "$tmp"
  git init -q -b main .
  git config user.email selftest@example.invalid
  git config user.name selftest
  mkdir -p scripts data/rules crates/sim/tests/fixtures
  cp "$self" scripts/check_fixture_policy.sh
  printf '(\n  rules_version: 5,\n)\n' > data/rules/rules.ron
  printf '0x0123456789abcdef\n' > crates/sim/tests/fixtures/a.hash
  head -c 4000 /dev/zero | tr '\0' 'A' > crates/sim/tests/fixtures/a.eonreplay
  git add -A && git commit -qm base
  base="$(git rev-parse HEAD)"
  expect() { # expect <exit code> <description>
    local want="$1" what="$2" got=0
    scripts/check_fixture_policy.sh "$base" >/dev/null 2>&1 || got=$?
    if [ "$got" != "$want" ]; then
      echo "self-test FAILED: $what (exit $got, wanted $want)" >&2
      exit 1
    fi
    echo "  ok: $what"
  }
  echo "fixture policy self-test:"
  git mv crates/sim/tests/fixtures fixtures && git commit -qm "move"
  expect 0 "a pure move is not a modification"
  printf '0xfedcba9876543210\n' > fixtures/a.hash && git commit -qam "change hash"
  expect 1 "a moved and changed .hash without a bump fails"
  sed -i 's/rules_version: 5/rules_version: 6/' data/rules/rules.ron && git commit -qam "bump"
  expect 0 "the same change with a rules_version bump passes"
  git reset -q --hard "$base"
  printf '0xfedcba9876543210\n' > crates/sim/tests/fixtures/a.hash && git commit -qam "change in place"
  expect 1 "an in-place change without a bump fails"
  git reset -q --hard "$base"
  printf '0x1111111111111111\n' > crates/sim/tests/fixtures/b.hash && git add -A && git commit -qm "new fixture"
  expect 0 "a new fixture needs no bump"
  echo "check_fixture_policy: self-test ok"
  exit 0
fi

base="${1:?usage: scripts/check_fixture_policy.sh <base-commit>}"
cd "$(dirname "$0")/.."

is_golden() { case "$1" in *.eonreplay|*.hash) return 0 ;; *) return 1 ;; esac; }

changed=""
# Every golden fixture present in HEAD, in either location.
while IFS= read -r f; do
  is_golden "$f" || continue
  name="$(basename "$f")"
  for dir in fixtures crates/sim/tests/fixtures; do
    old="$(git rev-parse -q --verify "$base:$dir/$name" 2>/dev/null || true)"
    if [ -n "$old" ]; then
      if [ "$old" != "$(git rev-parse "HEAD:$f")" ]; then changed="$changed$f"$'\n'; fi
      break
    fi
  done
done < <(git ls-tree -r --name-only HEAD -- fixtures/ crates/sim/tests/fixtures/)
changed="${changed%$'\n'}"

# A fixture that vanished is not a policy violation, but nobody should lose one
# unnoticed.
while IFS= read -r f; do
  is_golden "$f" || continue
  name="$(basename "$f")"
  if ! git cat-file -e "HEAD:fixtures/$name" 2>/dev/null \
    && ! git cat-file -e "HEAD:crates/sim/tests/fixtures/$name" 2>/dev/null; then
    echo "::warning::golden fixture $name exists in the base and not in HEAD" >&2
  fi
done < <(git ls-tree -r --name-only "$base" -- fixtures/ crates/sim/tests/fixtures/)

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
