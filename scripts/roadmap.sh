#!/usr/bin/env bash
# roadmap.sh: read docs/ROADMAP.md by piece. The file is 130 KB and a ticked
# acceptance line can carry 3 KB of pasted evidence, so a session reads it with
# this and not whole (docs/PLAYBOOK.md, P14).
#
#   scripts/roadmap.sh open [CUT]         every unticked acceptance line, one per line
#   scripts/roadmap.sh lines M7 [CUT]     the acceptance lines of one milestone
#   scripts/roadmap.sh section M7 [CUT]   the whole section of one milestone
#   scripts/roadmap.sh full N             print file line N uncut (what a cut line hides)
#
# CUT is the most characters shown per line (default 260 for open and lines,
# 700 for section; 0 shows everything). A cut line ends with
# `[+N chars: scripts/roadmap.sh full L]`. The milestone key is the start of
# its heading: M0 M1 M2 M3a M3b M4a M4b M5a M5b M6 M7 M8 M9, `Art` for the art
# pass, or any other heading prefix (`Risk`, `Open`, `Deferred`, `Closing`).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
exec python3 -I - "$root/docs/ROADMAP.md" "$@" <<'PY'
import re
import sys

args = sys.argv[1:]
path = args[0]
if len(args) < 2:
    sys.exit("usage: scripts/roadmap.sh open|lines|section|full ... (see the header of scripts/roadmap.sh)")
cmd, rest = args[1], args[2:]
lines = open(path, encoding="utf-8").read().split("\n")
acc = re.compile(r"^- \[( |x)\] ")


def cut(n, text, limit):
    if limit and len(text) > limit:
        return f"{text[:limit]} [+{len(text) - limit} chars: scripts/roadmap.sh full {n}]"
    return text


def sections():
    """(title, first line index, last line index exclusive) per `## ` heading."""
    heads = [i for i, l in enumerate(lines) if l.startswith("## ")]
    for k, i in enumerate(heads):
        end = heads[k + 1] if k + 1 < len(heads) else len(lines)
        yield lines[i][3:], i, end


def find(key):
    k = key.lower()
    for title, a, b in sections():
        t = title.lower()
        if t == k or t.startswith(k + ":") or t.startswith(k + " "):
            return title, a, b
    sys.exit(f"roadmap.sh: no section starting with {key!r}")


def arg(i, default):
    return int(rest[i]) if len(rest) > i and rest[i] != "" else default


if cmd == "open":
    limit = arg(0, 260)
    for title, a, b in sections():
        short = title.split(":")[0].split(" (")[0]
        for n in range(a, b):
            if lines[n].startswith("- [ ] "):
                print(f"L{n + 1} [{short}] {cut(n + 1, lines[n][6:], limit)}")
elif cmd == "lines":
    if not rest:
        sys.exit("usage: scripts/roadmap.sh lines M7 [CUT]")
    title, a, b = find(rest[0])
    limit = arg(1, 260)
    for n in range(a, b):
        m = acc.match(lines[n])
        if m:
            box = "x" if m.group(1) == "x" else " "
            print(f"L{n + 1} [{box}] {cut(n + 1, lines[n][6:], limit)}")
elif cmd == "section":
    if not rest:
        sys.exit("usage: scripts/roadmap.sh section M7 [CUT]")
    title, a, b = find(rest[0])
    limit = arg(1, 700)
    for n in range(a, b):
        print(cut(n + 1, lines[n], limit))
elif cmd == "full":
    if not rest:
        sys.exit("usage: scripts/roadmap.sh full LINE")
    n = int(rest[0])
    print(lines[n - 1])
else:
    sys.exit("usage: scripts/roadmap.sh open|lines|section|full ...")
PY
