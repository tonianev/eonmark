#!/usr/bin/env python3
"""check_roadmap_ticks.py: an acceptance line of docs/ROADMAP.md is only ever
ticked and given evidence, never reworded (CLAUDE.md rule 5, ROADMAP "Closing
a milestone", IMPLEMENTER_NOTES "When blocked" 5).

Compares the acceptance lines (`- [ ] ...` and `- [x] ...`) of the working
tree's docs/ROADMAP.md with those of a base commit:

* every base line's contract text must still open some head line. The contract
  is the line without its checkbox and without the evidence suffix that starts
  at " (Observed on PR #" or " (Owner-confirmed ". Appending a note or evidence
  is fine; changing, shortening or deleting the contract is not;
* a line that goes from [ ] to [x] must carry an evidence suffix;
* a head line that matches no base line is only a warning (an owner-directed
  scope change adds lines through an ADR; the owner can accept the warning).

Ticking an [owner] line is the owner's (docs/PLAN.md, D6); this script cannot
tell who ticked, so the PR description must say.

Usage: python3 -I scripts/check_roadmap_ticks.py <base-commit>
       python3 -I scripts/check_roadmap_ticks.py --self-test
"""
import re
import subprocess
import sys

ACC = re.compile(r"^- \[( |x)\] (.*)$")
MARKERS = (" (Observed on PR #", " (Owner-confirmed ")


def parse(text):
    """[(checked, text)] for every acceptance line, in file order."""
    out = []
    for line in text.split("\n"):
        m = ACC.match(line)
        if m:
            out.append((m.group(1) == "x", m.group(2)))
    return out


def split_evidence(text):
    """(contract, evidence) at the earliest evidence marker."""
    cuts = [i for i in (text.find(m) for m in MARKERS) if i >= 0]
    if not cuts:
        return text, ""
    i = min(cuts)
    return text[:i], text[i:]


def check(base_text, head_text):
    """(errors, warnings) for a base and a head ROADMAP text."""
    base, head = parse(base_text), parse(head_text)
    errors, warnings = [], []
    matched_heads = set()
    for checked, text in base:
        contract, _ = split_evidence(text)
        found = [k for k, (_, h) in enumerate(head) if h.startswith(contract)]
        if not found:
            errors.append(
                "an acceptance line was reworded, shortened or removed: "
                f"{contract[:110]!r}"
            )
            continue
        k = found[0]
        matched_heads.add(k)
        h_checked, h_text = head[k]
        if not checked and h_checked and not any(m in h_text for m in MARKERS):
            errors.append(
                "ticked without evidence (append ' (Observed on PR #N, date: "
                f"output)'): {contract[:110]!r}"
            )
    if errors:
        # A reworded line also looks like a new one; report only the cause.
        return errors, warnings
    for k, (_, h) in enumerate(head):
        if k not in matched_heads and not any(h.startswith(split_evidence(t)[0]) for _, t in base):
            warnings.append(f"a new acceptance line (an owner-directed scope change?): {h[:110]!r}")
    return errors, warnings


def self_test():
    base = "\n".join(
        [
            "# Roadmap",
            "- [ ] `cargo test -p sim fog` passes.",
            "- [x] Done already. (Observed on PR #5, 2026-10-01: ok)",
            "- [ ] Open line with a note. (Note 2026-10-28: pending the Mac)",
            "",
        ]
    )
    cases = [
        ("unchanged", base, 0, 0),
        (
            "tick with evidence",
            base.replace(
                "- [ ] `cargo test -p sim fog` passes.",
                "- [x] `cargo test -p sim fog` passes. (Observed on PR #9, 2026-11-09: 5 passed)",
            ),
            0,
            0,
        ),
        (
            "tick without evidence",
            base.replace("- [ ] `cargo test -p sim fog` passes.", "- [x] `cargo test -p sim fog` passes."),
            1,
            0,
        ),
        (
            "reworded contract",
            base.replace("passes.", "mostly passes."),
            1,
            0,
        ),
        (
            "appended note",
            base.replace("passes.", "passes. (Note 2026-11-09: split in two)", 1),
            0,
            0,
        ),
        ("removed line", base.replace("- [x] Done already. (Observed on PR #5, 2026-10-01: ok)\n", ""), 1, 0),
        (
            "evidence of a ticked line corrected",
            base.replace("2026-10-01: ok", "2026-10-02: ok, rerun"),
            0,
            0,
        ),
        ("untick keeps the contract", base.replace("- [x] Done", "- [ ] Done"), 0, 0),
        ("new line is a warning", base + "- [ ] A brand new line.\n", 0, 1),
        (
            "owner confirmation counts as evidence",
            base.replace(
                "- [ ] `cargo test -p sim fog` passes.",
                "- [x] `cargo test -p sim fog` passes. (Owner-confirmed 2026-11-09)",
            ),
            0,
            0,
        ),
    ]
    bad = 0
    for name, head, want_errors, want_warnings in cases:
        errors, warnings = check(base, head)
        ok = len(errors) == want_errors and len(warnings) == want_warnings
        print(f"  {'ok' if ok else 'FAILED'}: {name}")
        if not ok:
            bad += 1
            print(f"    errors={errors} warnings={warnings}")
    if bad:
        print(f"check_roadmap_ticks: self-test FAILED ({bad})", file=sys.stderr)
        return 1
    print("check_roadmap_ticks: self-test ok")
    return 0


def main(argv):
    if argv[1:] == ["--self-test"]:
        return self_test()
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    base = argv[1]
    shown = subprocess.run(
        ["git", "show", f"{base}:docs/ROADMAP.md"], capture_output=True, text=True
    )
    if shown.returncode != 0:
        print(f"check_roadmap_ticks: no docs/ROADMAP.md at {base}: {shown.stderr.strip()}", file=sys.stderr)
        return 2
    with open("docs/ROADMAP.md", encoding="utf-8") as f:
        head = f.read()
    errors, warnings = check(shown.stdout, head)
    for w in warnings:
        print(f"::warning::check_roadmap_ticks: {w}")
    for e in errors:
        print(f"::error::check_roadmap_ticks: {e}", file=sys.stderr)
    if errors:
        print(
            "check_roadmap_ticks: acceptance lines may only be ticked and given evidence "
            "(CLAUDE.md rule 5); an owner-directed change goes through an ADR",
            file=sys.stderr,
        )
        return 1
    print(f"check_roadmap_ticks: ok ({len(parse(head))} acceptance lines, {len(warnings)} new)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
