#!/usr/bin/env python3
"""check_plan_refs.py: lint the work-package briefs (docs/wp/*.md) so a session
that follows one is not sent to a file or a symbol that does not exist.

Two checks:

1. Anchors. In each brief, the bullets under the `## Anchors` heading have the
   form  - `path/to/file.rs`: `symbol_a`, `symbol_b` (free text)  and are
   verified: the path must exist, and every other backticked token on the line
   must occur in that file (or, for a directory, somewhere under it). A path
   the work package creates is marked `(new)` and is not checked.
2. Paths. Any backticked token in a brief, in docs/PLAN.md, docs/PLAYBOOK.md or
   CLAUDE.md that starts with a repository directory (crates/, docs/, scripts/,
   data/, assets/, fixtures/, tools/, .github/) must exist, unless its line
   says `(new`, `new file`, `the new`, `creates` or `new:`, or the token holds a placeholder
   (<, >, *, {, ..., $).

Usage: python3 -I scripts/check_plan_refs.py
Exit 1 with one line per problem.
"""
import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
os.chdir(ROOT)

PREFIXES = ("crates/", "docs/", "scripts/", "data/", "assets/", "fixtures/", "tools/", ".github/")
TICKS = re.compile(r"`([^`]+)`")
PLACEHOLDER = re.compile(r"[<>*{}$]|\.\.\.")
NEW_MARK = re.compile(r"\(new\b|\bnew (file|directory|dir)\b|\bthe new\b|\bcreates?\b|\bnew:", re.I)


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


def tree_has(path, symbol):
    """Whether `symbol` occurs in the file, or in any file under the directory."""
    if os.path.isfile(path):
        return symbol in read(path)
    for dirpath, _, names in os.walk(path):
        for name in names:
            if name.endswith((".rs", ".ron", ".toml", ".md", ".sh", ".py", ".yml", ".wgsl")):
                if symbol in read(os.path.join(dirpath, name)):
                    return True
    return False


def check_anchors(path, errors):
    in_anchors = False
    for n, line in enumerate(read(path).split("\n"), 1):
        if line.startswith("## "):
            in_anchors = line.strip().lower().startswith("## anchors")
            continue
        if not in_anchors or not line.startswith("- `"):
            continue
        tokens = TICKS.findall(line)
        if not tokens:
            continue
        target, symbols = tokens[0], tokens[1:]
        if NEW_MARK.search(line):
            continue
        if not os.path.exists(target):
            errors.append(f"{path}:{n}: anchor path does not exist: {target}")
            continue
        for sym in symbols:
            if PLACEHOLDER.search(sym):
                continue
            if not tree_has(target, sym):
                errors.append(f"{path}:{n}: `{sym}` not found in {target}")


def check_paths(path, errors):
    fenced = False
    for n, line in enumerate(read(path).split("\n"), 1):
        if line.lstrip().startswith("```"):
            fenced = not fenced
            continue
        if fenced or NEW_MARK.search(line):
            continue
        for tok in TICKS.findall(line):
            tok = tok.strip()
            if not tok.startswith(PREFIXES) or " " in tok or PLACEHOLDER.search(tok):
                continue
            tok = tok.rstrip(".,;:)")
            tok = tok.split("#", 1)[0].split("::", 1)[0]
            if tok and not os.path.exists(tok):
                errors.append(f"{path}:{n}: path does not exist: {tok}")


def main():
    errors = []
    briefs = sorted(glob.glob("docs/wp/*.md"))
    for path in briefs:
        check_anchors(path, errors)
    for path in briefs + ["docs/PLAN.md", "docs/PLAYBOOK.md", "CLAUDE.md"]:
        if os.path.exists(path):
            check_paths(path, errors)
    for e in errors:
        print(e, file=sys.stderr)
    if errors:
        print(f"check_plan_refs: {len(errors)} problem(s)", file=sys.stderr)
        return 1
    print(f"check_plan_refs: ok ({len(briefs)} briefs)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
