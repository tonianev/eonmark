#!/usr/bin/env python3
"""check_doc_links.py: every relative Markdown link in the given files (default:
all tracked .md files) must point at a file or directory that exists.

External links (http, https, mailto) and pure anchors (#heading) are not
checked. A link's fragment is dropped before the file is looked up.

Usage: python3 -I scripts/check_doc_links.py [FILE.md ...]
"""
import os
import re
import subprocess
import sys

LINK = re.compile(r"\]\(([^)\s]+)\)")
CODE_FENCE = re.compile(r"^\s*```")


def links(path):
    """(line number, target) for every link outside code fences."""
    fenced = False
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            if CODE_FENCE.match(line):
                fenced = not fenced
                continue
            if fenced:
                continue
            # Inline code spans can show link syntax; drop them.
            line = re.sub(r"`[^`]*`", "", line)
            for m in LINK.finditer(line):
                yield n, m.group(1)


def main(argv):
    files = argv[1:]
    if not files:
        out = subprocess.run(["git", "ls-files", "*.md"], capture_output=True, text=True, check=True)
        files = out.stdout.split()
    bad = 0
    for path in files:
        base = os.path.dirname(path)
        for n, target in links(path):
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            file_part = target.split("#", 1)[0]
            if not file_part:
                continue
            resolved = os.path.normpath(os.path.join(base, file_part))
            if not os.path.exists(resolved):
                bad += 1
                print(f"{path}:{n}: broken link {target} (no {resolved})", file=sys.stderr)
    if bad:
        print(f"check_doc_links: {bad} broken link(s)", file=sys.stderr)
        return 1
    print(f"check_doc_links: ok ({len(files)} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
