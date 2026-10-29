#!/usr/bin/env bash
# check_difficulty_flip.sh: ROADMAP M6 acceptance 4, second clause
# (docs/design/ai.md, "Difficulty table"): changing hard.ron's
# income_interval_ds from 200 to 300 (the design text's income_interval_s
# 20 to 30) must flip the Hard-versus-Easy result.
#
# It copies the data directory to a scratch directory, edits the copy's
# ai/difficulty/hard.ron `income_interval_ds: 200` to 300 and asserts that
# exactly one line changed, then plays `--a hard --b easy` and
# `--a easy --b hard` over seeds 1..20 at --jobs 4 on the original and on
# the edited copy (four play-bots runs of 20 games) and prints the four
# Hard win counts. PASS: Hard wins >= 15/20 at 200 and <= 9/20 at 300 in
# both seat orders (at 300 both sides receive the same bonus, so the
# flipped run is the equal-income control).
#
# Usage: scripts/check_difficulty_flip.sh [--data D] [--seeds A..B] [--jobs N] [--out DIR]
# SIM_CLI names the sim-cli binary (default target/release/sim-cli, built
# with `cargo build --release -p sim-cli` when missing). The CSVs and the
# stderr files stay under --out (default a fresh scratch directory).
# Exit 0 on PASS, 1 on FAIL or a failed run, 2 on usage errors.
set -euo pipefail

data=data
seeds=1..20
jobs=4
out=
while [ $# -gt 0 ]; do
  case "$1" in
    --data) [ $# -ge 2 ] || { echo "check_difficulty_flip: --data needs a directory" >&2; exit 2; }; data="$2"; shift 2 ;;
    --seeds) [ $# -ge 2 ] || { echo "check_difficulty_flip: --seeds needs A..B" >&2; exit 2; }; seeds="$2"; shift 2 ;;
    --jobs) [ $# -ge 2 ] || { echo "check_difficulty_flip: --jobs needs N" >&2; exit 2; }; jobs="$2"; shift 2 ;;
    --out) [ $# -ge 2 ] || { echo "check_difficulty_flip: --out needs a directory" >&2; exit 2; }; out="$2"; shift 2 ;;
    *) echo "usage: scripts/check_difficulty_flip.sh [--data D] [--seeds A..B] [--jobs N] [--out DIR]" >&2; exit 2 ;;
  esac
done
hard="$data/ai/difficulty/hard.ron"
[ -r "$hard" ] || { echo "check_difficulty_flip: no $hard" >&2; exit 2; }
sim_cli="${SIM_CLI:-target/release/sim-cli}"
if [ ! -x "$sim_cli" ]; then
  cargo build --release --locked -p sim-cli
fi
[ -n "$out" ] || out=$(mktemp -d "${TMPDIR:-/tmp}/difficulty_flip.XXXXXX")
mkdir -p "$out"
flipped="$out/data-flipped"
rm -rf "$flipped"
cp -R "$data" "$flipped"
sed 's/^\([[:space:]]*income_interval_ds:[[:space:]]*\)200,/\1300,/' "$hard" > "$flipped/ai/difficulty/hard.ron"
changed=$(diff "$hard" "$flipped/ai/difficulty/hard.ron" | grep -c '^>' || true)
if [ "$changed" != 1 ]; then
  echo "check_difficulty_flip: expected exactly one changed line in hard.ron (income_interval_ds: 200 -> 300), got $changed" >&2
  exit 2
fi
echo "check_difficulty_flip: the edit (diff of hard.ron):"
diff "$hard" "$flipped/ai/difficulty/hard.ron" || true

# hard_wins <csv> <side>: rows whose winner is <side>.
hard_wins() { awk -F, -v s="$2" 'NR > 1 && $2 == s { n++ } END { print n + 0 }' "$1"; }

run() { # run <data> <a> <b> <name>
  local start end
  start=$(date +%s)
  if ! "$sim_cli" --data "$1" play-bots --seeds "$seeds" --a "$2" --b "$3" --jobs "$jobs" --allow-undecided > "$out/$4.csv" 2> "$out/$4.err"; then
    echo "check_difficulty_flip: play-bots $4 failed:" >&2
    tail -n 5 "$out/$4.err" >&2
    exit 1
  fi
  end=$(date +%s)
  echo "check_difficulty_flip: $4: $(tail -n 1 "$out/$4.err") (shell ${start:+$((end - start))} s)"
}

run "$data" hard easy base_hard_a
run "$data" easy hard base_hard_b
run "$flipped" hard easy flip_hard_a
run "$flipped" easy hard flip_hard_b

games=$(($(wc -l < "$out/base_hard_a.csv") - 1))
b_a=$(hard_wins "$out/base_hard_a.csv" a)
b_b=$(hard_wins "$out/base_hard_b.csv" b)
f_a=$(hard_wins "$out/flip_hard_a.csv" a)
f_b=$(hard_wins "$out/flip_hard_b.csv" b)
echo "check_difficulty_flip: hard interval 200: hard as a $b_a/$games, hard as b $b_b/$games"
echo "check_difficulty_flip: hard interval 300: hard as a $f_a/$games, hard as b $f_b/$games"
# The bars of the literal 20 games, scaled to another seed count: 15/20
# (three quarters, rounded up) and 9/20 (under half).
hi=$(((games * 3 + 3) / 4))
lo=$(((games - 1) / 2))
if [ "$b_a" -ge "$hi" ] && [ "$b_b" -ge "$hi" ] && [ "$f_a" -le "$lo" ] && [ "$f_b" -le "$lo" ]; then
  echo "check_difficulty_flip: PASS (baseline >= $hi and flipped <= $lo in both seat orders; out $out)"
else
  echo "check_difficulty_flip: FAIL (need baseline >= $hi and flipped <= $lo in both seat orders; out $out)"
  exit 1
fi
