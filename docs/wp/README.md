# Work-package briefs

One file per work package (WP) on the board in [../PLAN.md](../PLAN.md). A session reads CLAUDE.md, its own brief and the PLAYBOOK sections the brief cites, nothing more.

A brief has these sections:

| Section | What it holds |
|---|---|
| header line | size (S under half a session, M one, L two), risk (the chance of a judgement call or a second attempt), lane, what it waits on |
| Goal, Serves | what changes, and which ROADMAP lines or deliverables it closes |
| Anchors | `- `path`: `symbol`, `symbol`` bullets naming the code the work touches. Verified: `python3 -I scripts/check_plan_refs.py` fails if a path or a symbol does not exist. A thing the WP creates is marked `(new)` and is not checked |
| Steps | numbered, in order; names files and functions |
| Tests | the test names to write and what each asserts |
| Verify | exact commands and what they print |
| Done when, Stop if | the exit test; and the conditions under which to stop and write "Blocked" ([../PLAYBOOK.md](../PLAYBOOK.md), P13) instead of improvising |

Rules for editing a brief: change it only to correct a fact or to record what the WP learned for the next one, and keep Anchors true (run `scripts/check_plan_refs.py`). A brief never changes a ROADMAP acceptance line.
