#!/usr/bin/env bash
# same_outcomes.sh: prove that a change altered no game. Compares two
# `sim-cli play-bots` CSVs row by row on every column both files have, except
# `final_hash`, so a change that must keep every winner, tick and statistic
# (the M7 fog bump hashes a new grid, so every final_hash moves; PLAN.md,
# M7-5) is checked against its base. Columns are matched by name, so a column
# the change adds or removes cannot fake a difference.
#
# Usage: scripts/same_outcomes.sh BEFORE.csv AFTER.csv
#        scripts/same_outcomes.sh --self-test
# Prints nothing and exits 0 when the games are identical; otherwise prints the
# first differences and exits 1. Exit 2 on a usage error.
set -euo pipefail

exec python3 -I - "$@" <<'PY'
import csv
import io
import sys

IGNORED = {"final_hash"}


def load(text):
    return list(csv.DictReader(io.StringIO(text)))


def compare(a_rows, b_rows):
    """(compared columns, ignored columns, list of difference strings)."""
    if not a_rows or not b_rows:
        return [], [], ["one of the CSVs has no rows"]
    a_cols, b_cols = list(a_rows[0]), list(b_rows[0])
    common = [c for c in a_cols if c in b_cols and c not in IGNORED]
    ignored = [c for c in a_cols + b_cols if c not in common]
    diffs = []
    if len(a_rows) != len(b_rows):
        diffs.append(f"row counts differ: {len(a_rows)} against {len(b_rows)}")
    for i, (x, y) in enumerate(zip(a_rows, b_rows), 1):
        bad = [c for c in common if x[c] != y[c]]
        if bad:
            seed = x.get("seed", "?")
            shown = ", ".join(f"{c}: {x[c]} -> {y[c]}" for c in bad[:4])
            diffs.append(f"row {i} (seed {seed}): {shown}")
    return common, sorted(set(ignored)), diffs


def self_test():
    head = "seed,winner,tick,final_hash,bonus_a\n"
    a = head + "1,a,100,0xaa,5\n2,b,200,0xbb,6\n"
    cases = [
        ("identical", a, a, 0),
        ("only the hash moved", a, a.replace("0xaa", "0x11").replace("0xbb", "0x22"), 0),
        ("a winner moved", a, a.replace("2,b,200", "2,a,200"), 1),
        ("a tick moved", a, a.replace("1,a,100", "1,a,101"), 1),
        ("a column was added", a, "seed,winner,tick,final_hash,bonus_a,faction_a\n1,a,100,0x1,5,x\n2,b,200,0x2,6,x\n", 0),
        ("a row is missing", a, head + "1,a,100,0xaa,5\n", 1),
    ]
    bad = 0
    for name, x, y, want in cases:
        _, _, diffs = compare(load(x), load(y))
        ok = (1 if diffs else 0) == want
        print(f"  {'ok' if ok else 'FAILED'}: {name}")
        bad += 0 if ok else 1
    if bad:
        print("same_outcomes: self-test FAILED", file=sys.stderr)
        return 1
    print("same_outcomes: self-test ok")
    return 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if len(argv) != 2:
        print("usage: scripts/same_outcomes.sh BEFORE.csv AFTER.csv | --self-test", file=sys.stderr)
        return 2
    common, ignored, diffs = compare(load(open(argv[0]).read()), load(open(argv[1]).read()))
    if diffs:
        print(f"same_outcomes: the games differ ({len(common)} columns compared; ignored: {', '.join(ignored) or 'none'})", file=sys.stderr)
        for d in diffs[:10]:
            print(f"  {d}", file=sys.stderr)
        if len(diffs) > 10:
            print(f"  ... and {len(diffs) - 10} more", file=sys.stderr)
        return 1
    return 0


sys.exit(main(sys.argv[1:]))
PY
