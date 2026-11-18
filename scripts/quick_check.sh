#!/usr/bin/env bash
# quick_check.sh: run only the checks the current diff needs (docs/PLAYBOOK.md,
# P2 and P14). It is the inner loop; `just ci` stays the gate before a push.
#
# It looks at what changed against the base (default origin/main: committed,
# uncommitted and untracked files) and runs the matching checks. A passing check
# prints one line; a failing check prints the last 40 lines of its output (the
# whole output is kept under the scratch directory it names), so a run costs a
# few lines of conversation, not a few thousand.
#
#   scripts/quick_check.sh              the checks the diff needs
#   scripts/quick_check.sh --list       say which checks that would be, run none
#   scripts/quick_check.sh --all        every check, whatever changed
#   scripts/quick_check.sh --no-game    skip the Bevy-sized ones (clippy and tests of crates/game)
# Env: QUICK_BASE=<rev> compares against another base (default origin/main).
#
# Changed paths -> checks:
#   any .rs                       cargo fmt --check
#   always                        typos, trademark
#   sim|rules|ai|sim-cli|data|fixtures|Cargo   clippy and tests of those four crates, data-check
#   game, or sim|rules|ai src     clippy and tests of crates/game (the slow ones)
#   assets|tools|visuals.ron      check_assets (+ self-test), inspect_gltf check
#   fixtures|rules.ron|sim src    fixture policy, verify of every fixture
#   docs/ROADMAP.md               ROADMAP ticks guard
#   docs/PLAN.md|docs/wp|PLAYBOOK|CLAUDE.md   work-package reference lint
#   docs/**/*.md                  relative-link check
#   scripts/*.sh                  bash -n; .github/workflows  YAML parse
#   Cargo.lock|deny.toml          cargo deny; Cargo.toml  cargo machete
set -uo pipefail
cd "$(dirname "$0")/.."

all=0 list=0 heavy=1
for a in "$@"; do
  case "$a" in
    --all) all=1 ;;
    --list) list=1 ;;
    --no-game) heavy=0 ;;
    -h|--help) sed -n '2,/^set -uo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "quick_check: unknown option $a (see --help)" >&2; exit 2 ;;
  esac
done

base="${QUICK_BASE:-origin/main}"
git rev-parse -q --verify "$base" >/dev/null || base=main
mb="$(git merge-base "$base" HEAD 2>/dev/null || echo "$base")"
changed="$( { git diff --name-only "$mb" HEAD; git diff --name-only HEAD; git ls-files -o --exclude-standard; } 2>/dev/null | sort -u )"
touched() { [ "$all" = 1 ] || printf '%s\n' "$changed" | grep -Eq "$1"; }

log="$(mktemp -d "${TMPDIR:-/tmp}/quick_check.XXXXXX")"
results=()
failed=0

run() { # run <name> <command...>
  local name="$1"
  shift
  if [ "$list" = 1 ]; then results+=("would run  $name"); return; fi
  local t0 rc=0
  t0=$(date +%s)
  "$@" >"$log/$name.out" 2>&1 || rc=$?
  if [ "$rc" = 0 ]; then
    results+=("ok         $name ($(( $(date +%s) - t0 ))s)")
  else
    failed=1
    results+=("FAILED     $name ($(( $(date +%s) - t0 ))s)")
    echo "---- $name failed (exit $rc); last 40 lines (all of it: $log/$name.out)"
    tail -n 40 "$log/$name.out"
  fi
}
skip() { results+=("skipped    $1 ($2)"); }

# --- always
if touched '\.rs$'; then run fmt cargo fmt --all -- --check; else skip fmt "no .rs changed"; fi
if command -v typos >/dev/null 2>&1; then run typos typos; else skip typos "not installed: scripts/dev_setup.sh"; fi
run trademark scripts/check_trademark.sh

# --- the engine-free crates: seconds to minutes, no Bevy
if touched '^(crates/(sim|rules|ai|sim-cli)/|data/|fixtures/|Cargo\.(toml|lock)|clippy\.toml)'; then
  run clippy_engine_free cargo clippy -p sim -p rules -p ai -p sim-cli --all-targets --locked --profile ci -- -D warnings
  run test_engine_free cargo test -p sim -p rules -p ai -p sim-cli --locked --profile ci
  run data_check cargo run -q -p sim-cli --locked --profile ci -- data-check data
else
  skip engine-free "no sim, rules, ai, sim-cli, data or fixtures change"
fi

# --- the game crate: Bevy-sized
if [ "$heavy" = 1 ] && touched '^(crates/game/|crates/(sim|rules|ai)/src/|Cargo\.(toml|lock))'; then
  run clippy_game cargo clippy -p game --all-targets --locked --profile ci -- -D warnings
  run test_game cargo test -p game --locked --profile ci
else
  skip game "no game or engine-free src change, or --no-game"
fi

# --- assets
if touched '^(assets/|tools/|data/visuals\.ron|scripts/check_assets\.sh)'; then
  run assets scripts/check_assets.sh
  run assets_selftest scripts/check_assets.sh --self-test
  run inspect_gltf cargo run -q -p inspect_gltf --locked --release -- check data assets
else
  skip assets "no assets, tools or visuals.ron change"
fi

# --- golden fixtures
if touched '^(fixtures/|data/rules/rules\.ron|crates/(sim|rules|ai)/src/|scripts/check_fixture_policy\.sh)'; then
  run fixture_policy scripts/check_fixture_policy.sh "$mb"
  run fixture_policy_selftest scripts/check_fixture_policy.sh --self-test
  run fixtures_verify bash -c 'for f in fixtures/*.eonreplay; do cargo run -q -p sim-cli --locked --release -- verify "$f" || exit 1; done'
else
  skip fixtures "no fixture, rules.ron or engine-free src change"
fi

# --- docs
if touched '^docs/ROADMAP\.md$'; then run roadmap_ticks python3 -I scripts/check_roadmap_ticks.py "$mb"; else skip roadmap_ticks "ROADMAP.md unchanged"; fi
if touched '^(docs/PLAN\.md|docs/wp/|docs/PLAYBOOK\.md|CLAUDE\.md)'; then run plan_refs python3 -I scripts/check_plan_refs.py; else skip plan_refs "plan docs unchanged"; fi
if touched '\.md$'; then
  run doc_links python3 -I scripts/check_doc_links.py $(printf '%s\n' "$changed" | grep -E '\.md$' | while read -r f; do [ -f "$f" ] && echo "$f"; done)
else
  skip doc_links "no .md changed"
fi

# --- scripts, workflows, dependencies
if touched '^scripts/.*\.sh$'; then run script_syntax bash -c 'for f in scripts/*.sh; do bash -n "$f" || exit 1; done'; else skip script_syntax "no script changed"; fi
if touched '^\.github/workflows/'; then
  if python3 -c 'import yaml' 2>/dev/null; then
    run workflow_yaml python3 -I -c 'import glob, yaml; [yaml.safe_load(open(f)) for f in glob.glob(".github/workflows/*.yml")]'
  else
    skip workflow_yaml "PyYAML not installed"
  fi
else
  skip workflow_yaml "no workflow changed"
fi
if touched '^(Cargo\.lock|deny\.toml)'; then
  if command -v cargo-deny >/dev/null 2>&1; then run deny cargo deny check; else skip deny "not installed: scripts/dev_setup.sh"; fi
fi
if touched 'Cargo\.toml$'; then
  if command -v cargo-machete >/dev/null 2>&1; then run machete cargo machete; else skip machete "not installed: scripts/dev_setup.sh"; fi
fi

echo
echo "quick_check against ${base} ($(printf '%s\n' "$changed" | grep -c .) changed files):"
printf '  %s\n' "${results[@]}"
if [ "$list" = 0 ]; then
  if [ "$failed" = 0 ]; then echo "quick_check: ok. Run \`just ci\` once before the push."; else echo "quick_check: FAILED"; fi
fi
exit "$failed"
