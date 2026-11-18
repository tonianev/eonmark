#!/usr/bin/env bash
# fun_gate_all.sh: CI's `fun-gate` job as one command (ROADMAP M6 lines 1, 2
# and both clauses of 4, then the ignored `m6_` bot tests; docs/design/ai.md,
# "The fun gate in CI"). CI's steps call the sections of this script, so the
# job and `just fun-gate-all` cannot drift. The bodies are the former CI step
# bodies, character for character, so the printed lines are unchanged.
#
# Usage: scripts/fun_gate_all.sh [all|line1|line2|line4a|line4b|tests] [OUT]
#   all     build sim-cli, run every section (a failure does not stop the
#           rest), print one summary line, exit 1 if any failed (about 10
#           minutes on 4 vCPUs)
#   OUT     where the CSVs and stderr files go: default the current directory
#           for one section (CI), a fresh scratch directory for `all`
# Env:
#   DATA     the data directory to play with (default `data`). Tuning runs
#            pass a scratch copy: `DATA=/tmp/td scripts/fun_gate_all.sh all`
#            (docs/PLAYBOOK.md, P6). The `tests` section reads the repository's
#            own data and is skipped when DATA is set.
#   SIM_CLI  the sim-cli binary (default target/release/sim-cli)
set -uo pipefail
cd "$(dirname "$0")/.."

data="${DATA:-data}"
sim="${SIM_CLI:-target/release/sim-cli}"
export SIM_CLI="$sim"
dflag=(--data "$data")

line1() {
  # The literal command; scripts/fun_gate.sh checks every clause by the M6
  # owner decision 1 readings (a winner is a or b; decisive is seat_captured
  # before tick 48000; past 24000 is tick > 24000). The line requires exit 0,
  # so play-bots' exit fails the step, but only after the summary and
  # fun_gate.sh's clauses are printed.
  local start rc=0
  start=$(date +%s)
  "$sim" "${dflag[@]}" play-bots --seeds 1..20 --a standard --b standard --personality-a rush,boom,tower --personality-b rush,boom,tower --max-ticks 72000 --jobs 5 > "$out/l1.csv" 2> "$out/l1.err" || { echo "play-bots exit=$?"; rc=1; }
  tail -n 1 "$out/l1.err"
  scripts/fun_gate.sh "$out/l1.csv" "$out/l1.err" || rc=1
  echo "elapsed_s=$(( $(date +%s) - start ))"
  return "$rc"
}

line2() {
  # The literal commands at --jobs 4 and with --allow-undecided: the CSV is
  # the same at any --jobs and with or without the flag, which only stops a
  # draw from making play-bots exit 1 (the line counts wins, not draws; a
  # panic or a usage error still exits non-zero and fails the step). Only
  # `winner == b` counts; check_ordering.sh also prints the cap games that
  # ended untouched. Both pairings run and report whatever the first one gave.
  local start rc=0
  start=$(date +%s)
  "$sim" "${dflag[@]}" play-bots --a easy --b standard --seeds 1..20 --jobs 4 --allow-undecided > "$out/e_s.csv" 2> "$out/e_s.err" || { echo "play-bots exit=$?"; rc=1; }
  tail -n 1 "$out/e_s.err"
  scripts/check_ordering.sh "$out/e_s.csv" b 15 || rc=1
  "$sim" "${dflag[@]}" play-bots --a standard --b hard --seeds 1..20 --jobs 4 --allow-undecided > "$out/s_h.csv" 2> "$out/s_h.err" || { echo "play-bots exit=$?"; rc=1; }
  tail -n 1 "$out/s_h.err"
  scripts/check_ordering.sh "$out/s_h.csv" b 15 || rc=1
  echo "elapsed_s=$(( $(date +%s) - start ))"
  return "$rc"
}

line4a() {
  # Hard against Passive in seeds 1..3, as the New Game seat (b), with every
  # Random style, and from seat a; check_attack_before.sh also requires an
  # armed unit on the enemy's land before tick 14400.
  local rc=0
  "$sim" "${dflag[@]}" play-bots --seeds 1..3 --a passive --b hard --max-ticks 14400 --allow-undecided > "$out/h.csv" || rc=1
  scripts/check_attack_before.sh --side b --before 14400 "$out/h.csv" || rc=1
  "$sim" "${dflag[@]}" play-bots --seeds 1..3 --a passive --b hard --personality-b rush,boom,tower --max-ticks 14400 --allow-undecided > "$out/hp.csv" || rc=1
  scripts/check_attack_before.sh --side b --before 14400 "$out/hp.csv" || rc=1
  "$sim" "${dflag[@]}" play-bots --seeds 1..3 --a hard --b passive --max-ticks 14400 --allow-undecided > "$out/ha.csv" || rc=1
  scripts/check_attack_before.sh --side a --before 14400 "$out/ha.csv" || rc=1
  return "$rc"
}

line4b() {
  local start rc=0
  start=$(date +%s)
  scripts/check_difficulty_flip.sh --data "$data" || rc=1
  echo "elapsed_s=$(( $(date +%s) - start ))"
  return "$rc"
}

tests() {
  if [ "$data" != data ]; then
    echo "fun_gate_all: tests skipped (they read the repository's own data, not DATA=$data)"
    return 0
  fi
  cargo test -p ai --locked -- --ignored m6_
}

section="${1:-all}"
out="${2:-${OUT:-}}"
if [ -z "$out" ]; then
  if [ "$section" = all ]; then out="$(mktemp -d "${TMPDIR:-/tmp}/fun_gate_all.XXXXXX")"; else out=.; fi
fi
mkdir -p "$out"

case "$section" in
  line1|line2|line4a|line4b|tests)
    "$section"
    ;;
  all)
    if [ "$sim" = target/release/sim-cli ]; then
      cargo build -p sim-cli --locked --release || exit 1
    fi
    begin=$(date +%s)
    failed=()
    summary=()
    for s in line1 line2 line4a line4b tests; do
      echo "==> fun_gate_all: $s"
      if "$s"; then summary+=("$s=ok"); else summary+=("$s=FAIL"); failed+=("$s"); fi
    done
    echo "fun_gate_all: ${summary[*]} (data=$data out=$out elapsed_s=$(( $(date +%s) - begin )))"
    [ "${#failed[@]}" -eq 0 ]
    ;;
  *)
    echo "usage: scripts/fun_gate_all.sh [all|line1|line2|line4a|line4b|tests] [OUT]" >&2
    exit 2
    ;;
esac
