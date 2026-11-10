# Execution plan: M6 to v0.1.0

The live work board for the rest of the roadmap. It says what is done, what is ready, what waits on what, and what only the owner can do. The detail of each work package (WP) is in its own brief under [wp/](wp/README.md); scope and acceptance stay in [ROADMAP.md](ROADMAP.md), the binding rules in [IMPLEMENTER_NOTES.md](IMPLEMENTER_NOTES.md), the procedures in [PLAYBOOK.md](PLAYBOOK.md) and the session loop in [../CLAUDE.md](../CLAUDE.md).

Written 2026-11-08 at `main` 3095228 and finished 2026-11-10 for sessions on cost-efficient models: the design calls are made, a brief tells a session exactly what to touch, and anything a brief cannot settle is a "Stop if" that goes back to the owner.

**Merge policy: ask** (a session stops at a green PR and the owner merges). To save one round trip per PR, the owner may change this line to `auto-when-green`: a session may then squash-merge its own PR when every CI job is green, the WP's risk is not `high`, and the PR ticks no [owner] line. Nothing else in the repository depends on this line.

## How a session uses it

1. Read [../CLAUDE.md](../CLAUDE.md). Take the first `ready` row of your lane on the board below (or the first `ready` row from the top if no lane is named), and read only [wp/](wp/README.md)`<id>.md` and the PLAYBOOK sections it cites.
2. One WP per session and one PR per WP; an L-size WP may take two sessions on one draft PR (say so in the PR). Different lanes may run in parallel: they touch disjoint files, and each PR edits only its own board row.
3. The PR that finishes a WP sets its row to `done (#PR)` and updates the rows that waited on it, in the same PR.
4. If no row is `ready` in your lane, say what each waits on and stop; do not invent work.
5. Tools: `scripts/quick_check.sh` (the checks your diff needs), `scripts/roadmap.sh` (read the ROADMAP by piece), `scripts/fun_gate_all.sh` and `scripts/same_outcomes.sh` (the bot lines and the "no game changed" proof). PLAYBOOK P14 has the cost rules.

## Status at 2026-11-10

| Item | State |
|---|---|
| `main` | 3095228 (PR #57, 2026-11-05); this plan is PR "plan" on `claude/confident-hopper-ho6dz1` |
| `SIM_VERSION` / `rules_version` | 18 / 31 (M7-5 is the only planned bump: 19 / 32) |
| Golden fixtures | 12 in the top-level `fixtures/` (moved there from crates/sim/tests/fixtures by this plan's PR) |
| CI jobs | check (macos-26), headless (ubuntu-24.04), game compiles on Linux, cargo-deny, hash parity (macOS vs Linux), fun-gate (ubuntu-24.04) |
| Merged | M0 to M5b; the art pass; M6 lines 1 to 5 |
| Open agent lines | M7 lines 1, 2, 5; M8; M9 (`scripts/roadmap.sh open`) |
| Open owner lines | M0 window; M2 Cmd-Q and controls; M3b fps and border screenshot; M4b fps; M5b New Game match; art pass AP4 (Mac), AP5, AP6 by hand, AP8; M6 line 6 (the fun gate) |
| Version v0.1.0 is | M8-5. M9 is post-release and off the critical path |

## Board

Lanes: **G** game and UI (`crates/game`), **S** sim, bot and data, **A** assets, **R** repository, CI, release and docs.

| WP | Lane | Title | Size | Risk | Status |
|---|---|---|---|---|---|
| E1 | R | Dev tools in every container (`scripts/dev_setup.sh`) | S | low | done (plan PR) |
| X1 | S | Fixtures to `fixtures/`, faction-agnostic tests, rename-aware fixture policy | S | low | done (plan PR) |
| T1 | R | Guard and speed tools (`roadmap.sh`, `check_roadmap_ticks.py`, `quick_check.sh`, `check_plan_refs.py`, `check_doc_links.py`) | S | low | done (plan PR) |
| H2 | S | The fun gate as one command (`fun_gate_all.sh`, `same_outcomes.sh`) | S | low | done (plan PR) |
| [M7-1](wp/M7-1.md) | G | Esc pause menu and settings | M | low | ready |
| [M7-2](wp/M7-2.md) | A | Vendor the Kenney UI, icons and audio | S to M | medium | ready (needs kenney.nl, else O3) |
| [H1](wp/H1.md) | R | File the unfiled good first issues | S | low | ready |
| [M8-1](wp/M8-1.md) | R | CI hygiene: pinned actions, Dependabot | S | low | ready |
| [M6-1](wp/M6-1.md) | S | Ingest the fun-gate matches | S | low | waiting (O1) |
| [M6-2](wp/M6-2.md) | S | Tuning round, 0 to 3 of them | M | high | conditional (M6-1 finds a failure) |
| [M6-3](wp/M6-3.md) | S | Close M6 | S | low | waiting (M6-1, H1) |
| [M7-3](wp/M7-3.md) | G | Audio | M | low | waiting (M7-1, M7-2) |
| [M7-4](wp/M7-4.md) | G | Kenney panels and icons | M | medium | waiting (M7-2) |
| [M7-5](wp/M7-5.md) | S | Fog in the sim and the faction choice (the one bump) | L | medium | waiting (M6-3) |
| [M7-6](wp/M7-6.md) | G | Fog in the game | L | medium | waiting (M7-5, M7-4) |
| [M7-7](wp/M7-7.md) | R | Close M7 | S | low | waiting (M7-6) |
| [M8-0](wp/M8-0.md) | S | Two modifier targets and the data-only dry run | M | medium | waiting (M7-5) |
| [M8-2](wp/M8-2.md) | R | The smoke fixture | S | low | waiting (M7-5) |
| [M8-3](wp/M8-3.md) | R | App icon and the bundle checks | M | medium | waiting (M8-1) |
| [M8-4](wp/M8-4.md) | R | Final docs and the launch kit | M | low | waiting (M8-3, M7-7) |
| [M8-5](wp/M8-5.md) | R | Release and close M8 | S | low | waiting (O5; O2 and O4 done) |
| [M9-1](wp/M9-1.md) | S | Tidewater League as pure data | M | medium | waiting (M8-5, M8-0) |
| [M9-2](wp/M9-2.md) | R | Bevy 0.20 gate | S (L if open) | high if open | waiting (M9-1) |
| [M9-3](wp/M9-3.md) | R | The v0.2 plan and the M9 close | S | low | waiting (M9-2) |
| [H3](wp/H3.md) | R | Render watchdog on cold CI runners | S | low | conditional (a second stall) |

## Critical path

```text
Now, in parallel:
  lane G: M7-1 pause menu ---------------------------------------> M7-3 audio -> M7-4 panels
  lane A: M7-2 assets (needs kenney.nl or O3) ------------------------^
  lane R: H1 issues, M8-1 CI hygiene
  owner : O1 three streamed matches (any time; nothing else waits on it except M6 and, behind it, fog)

Then:
  M6-1 ingest O1 -> [M6-2 tuning, 0 to 3 rounds, each replayed by O1] -> M6-3 close M6
  M6-3 -> M7-5 fog in the sim + factions (the one bump) -> M7-6 fog in the game -> M7-7 close M7 -> owner O4
  M7-5 -> M8-0 and M8-2 ;  M8-1 -> M8-3 -> M8-4 ;  owner O2 (Mac batch) any time before M8-5
  O5 (tag and launch) -> M8-5  =  v0.1.0
  After the tag: M9-1 Tidewater (data only) -> M9-2 Bevy gate -> M9-3
```

The fun gate is the only thing between the game-only M7 work and fog: M7-5 changes the sim, and the owner's replays of O1 must verify with the build that recorded them (ADR 0003's timing trap), so the bump lands after M6 closes. Everything else runs now.

## Owner queue

Each item is one sitting. Post the results as one issue per sitting titled `Owner session YYYY-MM-DD: O<n>`, with one heading per step, its output pasted, and `pass` or `fail` against every [owner] line judged. Attach replays as a zip or on a branch `owner/<date>`. The next agent session ingests the issue (PLAYBOOK P11). **Start with O1: it is the longest pole and needs nothing from the agents.**

### O1. Three streamed fun-gate matches (about 2 to 2.5 hours, one sitting)
Unblocks M6 line 6 and M5b's [owner] line (the same matches satisfy it).
1. [STREAMING.md](STREAMING.md)'s pre-flight.
2. [PLAYTEST.md](PLAYTEST.md), "Owner's Mac checklist (M6)": seeds 112, 107 and 110, Difficulty Standard, Opponent style Random, x1, one sitting. One `match_log:` row per match; add Fun (1 to 5) and Notes (your style guess before the reveal, the bonus, speed and paused times, `iteration 0`).
3. `just match-log <replay>` for each replay (the Verify cell must read `OK 0x<the screen's final hash>`).
4. Off air, step 7 of that checklist: x4 throughput on Metal.
5. In the issue: the three rows, the replays, your ratings; for M5b's line a pass or fail and whether the bot felt winnable and losable (M5b decision 4).

Play on any build of `main` at `rules_version` 31 (game-only PRs do not change it). If `data/ai/personalities.ron` or the resolver changes first, the seeds are re-derived (PLAYBOOK P7).

### O2. The Mac batch (60 to 90 minutes, can be split; before the tag)
From `main` on the dev Mac, in this order: (1) PLAYTEST "Owner's Mac checklist (art pass)" steps 1 to 8: AP5's fps and headroom runs, AP7 on Metal, AP8's sign-off of the four screenshots on the P3 display and as sRGB, AP6 by hand, AP4's `mode=` re-read, and M3b's and M4b's fps lines with the models; (2) "Owner's Mac checklist (M5b)" steps 4 and 5; (3) "Owner's Mac checklist (M4b)" steps 1 to 10 with M5a's line 3; (4) the controls checklists (M2, M3b, M4b, M5a), "Cmd-Q (M2)", M0's window line and M3b's border screenshot.

### O3. Asset drop (15 minutes, only if M7-2 asks)
Only if this environment cannot reach kenney.nl and you decline adding the host: download the zips M7-2 lists, unchanged, and push them to a branch `owner/kenney-inbox` under `inbox/`.

### O4. M7 on the Mac (about 20 minutes, after M7-7)
The commands M7-7 writes into PLAYTEST "Owner's Mac checklist (M7)": M7 line 5's fps at 1440p with fog and draw calls, and line 6's sign-off of the new `docs/screenshots/m7_town.png`. (M7's lines in order: 1 `cargo test -p sim fog`, 2 fog in the game, 3 `check_assets` and 4 `data-check` (both ticked by the art pass), 5 fps at 1440p, 6 the screenshot.)

### O5. Release and launch (about 2 hours, after M8-4)
1. With CI green on `main`, push the tag (`git tag v0.1.0 && git push origin v0.1.0`), or tell the agent to.
2. M8 line 3: download the zip with Safari on a second Mac or a fresh user account, follow the README's "Open Anyway" steps, play a match to the end, `sim-cli verify` its replay with sim-cli built from the tag. M8 line 2: `shasum -a 256 -c SHA256SUMS` on the download.
3. M8 line 4: a fresh clone on a second Mac with only the Xcode command line tools and rustup, `cargo run -p game --features dev` on the first try (or the line's proxy on the dev Mac).
4. M8 line 1's Mac clauses: `open dist/Eonmark.app` shows the Dock name Eonmark and the icon.
5. M8 line 5: create the Discussions categories (Welcome, Ideas, Show and tell), open the bevy-assets PR from `docs/launch/`, post the showcase drafts, put the links in the issue.

## Decisions this plan rests on

| Id | Decision | Source |
|---|---|---|
| D1 | M7's game-only work (M7-1 to M7-4) may start before M6's fun-gate line closes and before M5b's [owner] line passes (O1 covers it). Sim-changing work waits for M6 to close. | Owner, 2026-11-08 (ROADMAP "Order of work") |
| D2 | The v0.1 bot reads the unfiltered view; fog is the human's, disclosed beside the bonus; a fog-obeying bot is a v0.2 candidate. | Owner, 2026-11-08, [ADR 0004](adr/0004-bot-reads-the-unfiltered-view.md) |
| D3 | The decision-17 balance gaps are tuned only where the three matches expose them; the rest become `kind:balance` issues. | M6 owner decision 18, 2026-11-08 |
| D4 | M6 owner decisions 15 to 17 (Easy's 33:00 all-in, Hard never retreats, the gaps to slice 4) are recorded in the ROADMAP's M6 Notes. | Owner, 2026-10-30 |
| D5 | One WP per session, one PR per WP. No multi-agent workflows; at most one narrow subagent call for a search or a log. The owner merges unless the Merge policy line says otherwise. | This plan |
| D6 | [owner] lines are ticked by the owner, or by the agent when the owner's session issue marks that line `pass` with its output. M6 line 6 is ticked only by the owner. | This plan |
| D7 | The golden fixtures live in the top-level `fixtures/` (done): M9 line 1 needs a faction commit with an empty `crates/` diff, and a faction changes `rules_hash`, which every fixture header embeds. | This plan |
| D8 | The faction choice ships in M7-5 inside M7's single bump; the gather-rate and per-building border targets ship in M8-0 with no bump. | This plan |
| D9 | Spectating shows the whole map (streams, bot-vs-bot). | This plan |
| D10 | Only UI scale and volume persist, in `settings.ron`. | This plan |
| D11 | Audio comes from Kenney's CC0 packs rather than Freesound: one host, one licence file per pack. | This plan |
| D12 | The ROADMAP-reading and acceptance-line rules are enforced by `scripts/roadmap.sh` and, in CI, by `scripts/check_roadmap_ticks.py`. | This plan |
| D13 | M8-1 to M8-4 may start before O2 and O4; the tag (M8-5) waits for O2, O4 and O5. | This plan (IMPLEMENTER_NOTES rule 1 is the exception the owner already granted for M4b and M5b) |

The owner may overturn any row marked "This plan" in an issue or a PR; the next session updates this table and the briefs it touches.

## Scope levers

The roadmap's scope is unchanged (no further cuts: ROADMAP open question "Session budget"). If the owner wants v0.1.0 sooner, these are the cheapest deliverable-level cuts, none of which touches an acceptance line; each is a ROADMAP edit with an ADR, the owner's call.

| Lever | Saves | Cost to the game |
|---|---|---|
| Ship without audio (drop M7-3 and its assets) | about 1.5 sessions | silent game; `cpal` stays unused |
| Ship with the flat matte UI (drop M7-4; M7-2 shrinks to audio) | about 1.5 sessions and O3 | the HUD keeps its current flat look, which ART_STYLE already calls matte |
| Pause menu without UI scale and borderless (M7-1 reduced to Resume, Volume, Quit to menu) | about 0.3 session | no UI scale, no fullscreen toggle |
| Skip the second-Mac sittings of O5 (M8 lines 3 and 4 become the dev-Mac proxies) | about 1 hour of the owner's time | a quarantined-download and clean-clone check on the dev Mac only |

M9 is already post-release, so it is not a lever.

## Risks

| Risk | Handling |
|---|---|
| The owner's sittings (O1, O2) are the critical path | D1 keeps game-only work moving; each O-item is one sitting posted as one issue |
| Tuning loops (M6-2, M9-1) burn sessions | the six-variant budget (P6), `fun_gate_all.sh` with a `DATA` override, the line's three-round limit, and stopping to ask |
| M7-5's hash churn hides a behaviour change | `same_outcomes.sh` proves no game changed; one bump on one draft PR across both sessions |
| The grid breaks the 10 s `cargo test -p sim` budget | measured in M7-5; stamp cadence as a data field inside the same bump |
| Blocked asset hosts | one environment approval for kenney.nl, or O3 |
| A cheaper agent judging looks (M7-4, M7-6) | the ART_STYLE checklist written into the PR for every screenshot; the owner signs AP8 and M7 line 6 |
| Mac-only failures in M8 | release-check on macOS runners by `workflow_dispatch`; Mac clauses go to O5 |
| A reworded acceptance line | CI fails the PR (`check_roadmap_ticks.py`) |
| A stale reference in a brief | `check_plan_refs.py` fails `quick_check.sh` when a brief's path or symbol does not exist; a brief is corrected, never worked around |
| A data change silently moves the fun gate | CI's `fun-gate` job on every PR; `fun_gate_all.sh` locally |
| A rare CI stall (the render watchdog on a cold runner) | one re-run; H3 if it happens twice |

## Budget

About 21 to 25 agent sessions to v0.1.0 (M6 close 2 plus 0 to 3 tuning rounds; M7 about 9; M8 about 6; housekeeping 1 to 2; M9 about 3 after the tag) and about 6 to 8 hours of the owner's time (O1 2.5 h plus 2 h per extra round, O2 1 to 1.5 h, O4 20 min, O5 2 h).

## Found while working

Sessions append one line per problem found outside their WP, with the file and why it matters, instead of fixing it in their PR. Triage turns each line into an issue or a WP.

- (none yet)
