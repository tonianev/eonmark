# Execution plan: M6 to M9

This file is the live work queue for the rest of the roadmap. It breaks every remaining ROADMAP item into work packages (WPs) sized for one agent session each, lists what only the owner can do, and records the decisions the plan rests on. Scope and acceptance stay in [ROADMAP.md](ROADMAP.md), the binding rules in [IMPLEMENTER_NOTES.md](IMPLEMENTER_NOTES.md), the procedures in [PLAYBOOK.md](PLAYBOOK.md) and the session loop in [CLAUDE.md](../CLAUDE.md).

Written on 2026-11-08 at `main` 3095228. From here on the work runs in cost-efficient sessions, so every WP below is written to be executed, not designed: the design calls are made here, and anything a WP cannot settle is a "Stop if" that goes back to the owner.

## How to use this plan

- Take the WP the board (section 4) marks **next**. If none is marked, take the first `ready` row in board order. An ingest WP (M6-1, M7-7's owner step, M8-5) goes first as soon as its owner session is posted.
- One WP per session and one PR per WP. A WP sized L may take two sessions on one branch and one draft PR; say so in the PR.
- The PR that finishes a WP sets its row to `done` with the PR number and marks the next row **next**, in the same PR. Nothing else in this file changes without a reason given in the PR.
- Every WP lists Goal, Serves (the acceptance lines or deliverables), Touches, Steps, Verify, Done when and Stop if. Stop if means: stop, write the Blocked section ([PLAYBOOK.md](PLAYBOOK.md), P13) and leave the call to the owner.
- Sizes: S is under half a session, M one session, L two. Risk is the chance that the WP needs a judgement call or a second attempt.

## 1. Status at 2026-11-08

| Item | State |
|---|---|
| `main` | 3095228, PR #57 (2026-11-05) |
| `SIM_VERSION` / `rules_version` | 18 / 31 |
| Golden fixtures | 12, in `crates/sim/tests/fixtures` (M8-0 moves them to `fixtures/`) |
| CI jobs | check (macos-26), headless (ubuntu-24.04), game compiles on Linux, cargo-deny, hash parity (macOS vs Linux), fun-gate (ubuntu-24.04) |
| Merged | M0 to M5b; the art pass (#51 to #54); M6 PR 1 (#56, lines 1 to 4) and M6 slice 3 (#57, line 5) |
| Open agent lines | M7 (all but the two the art pass ticked), M8, M9 |
| Open owner lines | M0's window line; M2's Cmd-Q and controls lines; M3b's fps line (Mac) and border screenshot; M4b's fps line (Mac); M5b's New Game match; the art pass's AP4 (Mac `mode=` re-read), AP5, AP6 (by hand) and AP8; M6's fun gate (line 6) |

Bot strength and balance at `rules_version` 31 are in [design/ai.md](design/ai.md), "Difficulty table (as retuned)". The owner's decisions are numbered in the ROADMAP's Notes (M5b, art pass, M6 1 to 18) and in section 6 below.

## 2. Critical path

```text
Now, in parallel (D1):
  agent: M7-1 pause menu -> H2 -> M7-2 assets -> M7-3 audio -> M7-4 panels -> H1
  owner: O1 three streamed matches (any time; the agent ingests them first)

Then, in order:
  M6-1 ingest O1 -> M6-2 tuning (0 to 3 rounds, each one replayed by O1) -> M6-3 close M6
  M7-5 fog in the sim + factions (M7's one bump) -> M7-6 fog in the game -> M7-7 close M7 -> owner O4
  owner O2 Mac batch (before M8 starts)
  M8-0 M9 readiness -> M8-1 CI hygiene -> M8-2 smoke -> M8-3 icon and bundle -> M8-4 docs and launch kit
  owner O5 tag and launch -> M8-5 release and close
  M9-1 Tidewater, data only (the first change after the tag) -> M9-2 Bevy 0.20 gate -> M9-3 v0.2 plan and close
```

The owner's three matches (O1) are the only thing between the game-only M7 work and the rest of the roadmap. O2 must be done before M8 starts, because every older [owner] line has to pass before the release milestone (IMPLEMENTER_NOTES rule 1, extended by the batching decisions of M4b and M5b).

## 3. Owner queue

Each item is one sitting. Post the results as one issue per sitting titled `Owner session YYYY-MM-DD: O<n>`, with one heading per step and its output pasted, and write `pass` or `fail` against every [owner] line you judged. Attach replays as a zip, or push them to a branch `owner/<date>`. The next agent session ingests the issue (PLAYBOOK P11).

### O1. Three streamed fun-gate matches (about 2 to 2.5 hours, one sitting)

Unblocks: M6 line 6 (the fun gate) and M5b's [owner] line, which the same matches satisfy (New Game against Standard, played to the game-over screen in one sitting, replay verified).

1. [STREAMING.md](STREAMING.md)'s pre-flight.
2. [PLAYTEST.md](PLAYTEST.md), "Owner's Mac checklist (M6)": seeds 112, 107 and 110, Difficulty Standard, Opponent style Random, x1, in one sitting. One `match_log:` row per match; add Fun (1 to 5) and Notes (your style guess before the reveal, the bonus, speed and paused times, iteration 0).
3. `just match-log <replay>` for each replay (the Verify cell must read `OK 0x<the screen's final hash>`).
4. Off air, step 7 of the same checklist: x4 throughput on Metal.
5. In the issue: the three rows, the replays, your ratings, and for M5b's line a pass or fail plus M5b decision 4's note (did the bot feel winnable and losable).

If `data/ai/personalities.ron` or the resolver changes before you play, the agent re-derives the seeds first (PLAYBOOK P7).

### O2. The Mac batch (about 60 to 90 minutes, can be split)

Run from `main` on the dev Mac, in this order (what unblocks the most comes first):

1. [PLAYTEST.md](PLAYTEST.md), "Owner's Mac checklist (art pass)", steps 1 to 8: AP5's fps and headroom runs, AP7 on Metal, AP8's sign-off of the four screenshots on the P3 display and as sRGB, AP6 by hand, AP4's `mode=` re-read, and M3b's and M4b's fps lines with the models.
2. "Owner's Mac checklist (M5b)", steps 4 and 5: M5b line 1's wall time on the Mac and `just m5b-checks` on Metal (steps 1 to 3 are covered by O1).
3. "Owner's Mac checklist (M4b)", steps 1 to 10, with M5a's line 3 and its proxy.
4. The controls checklists of M2, M3b, M4b and M5a, "Cmd-Q (M2)", M0's window line (pinch, wheel, WASD, the red button and `echo $?`), and M3b's border screenshot review.

### O3. Asset drop (15 minutes, only if M7-2 asks)

Only if this cloud environment cannot reach kenney.nl and you decline adding that host: download the zips M7-2 lists from kenney.nl, unchanged, and push them to a branch `owner/kenney-inbox` under `inbox/`.

### O4. M7 on the Mac (about 20 minutes, after M7-7)

The commands M7-7 writes into PLAYTEST, "Owner's Mac checklist (M7)": M7 line 5's fps at 1440p with fog (and draw calls), and line 6's sign-off of `docs/screenshots/m7_town.png` against the ART_STYLE checklist. (M7's lines in order: 1 `cargo test -p sim fog`, 2 fog in the game, 3 `check_assets` and 4 `data-check`, both ticked by the art pass, 5 fps at 1440p, 6 the owner's screenshot.)

### O5. Release and launch (about 2 hours, after M8-4)

1. With CI green on `main`, push the tag (`git tag v0.1.0 && git push origin v0.1.0`), or tell the agent to.
2. M8 line 3: download the zip with Safari on a second Mac or a fresh user account, follow the README's "Open Anyway" steps, play a match to the end, then `sim-cli verify` its replay with sim-cli built from the tag. M8 line 2: `shasum -a 256 -c SHA256SUMS` on the download.
3. M8 line 4: a fresh clone on a second Mac with only the Xcode command line tools and rustup, `cargo run -p game --features dev` on the first try; or the line's proxy on the dev Mac.
4. M8 line 1's Mac clauses: `open dist/Eonmark.app` shows the Dock name Eonmark and the icon.
5. M8 line 5: create the Discussions categories (Welcome, Ideas, Show and tell) in the repository settings, open the bevy-assets PR from M8-4's kit, post the two showcase drafts, and put the links in the issue.

## 4. Board

| WP | Title | Size | Risk | Waits on | Status |
|---|---|---|---|---|---|
| E1 | Dev tools in every container | S | low | | done (this plan's PR) |
| M7-1 | Esc pause menu and settings | M | low | | **next** |
| H2 | One command for the whole fun gate | S | low | | ready |
| M7-2 | Vendor the Kenney UI, icons and audio | S | medium | kenney.nl or O3 | ready |
| M7-3 | Audio | M | low | M7-1, M7-2 | waiting |
| M7-4 | Kenney panels and icons | M | medium | M7-2 | waiting |
| H1 | File the unfiled good first issues | S | low | | ready |
| M6-1 | Ingest the fun-gate matches | S | low | O1 | waiting (goes first once O1 is posted) |
| M6-2 | Tuning round (0 to 3 of them) | M | high | M6-1 finds a failure, H2 | conditional |
| M6-3 | Close M6 | S | low | M6-1 (all pass), H1 | waiting |
| M7-5 | Fog in the sim and the faction choice (the M7 bump) | L | medium | M6-3, H2 | waiting |
| M7-6 | Fog in the game | L | medium | M7-5 | waiting |
| M7-7 | Close M7 | S | low | M7-6 | waiting |
| M8-0 | M9 readiness: data-only factions after the tag | M | medium | M7-7, O2 | waiting |
| M8-1 | CI hygiene: pinned actions, Dependabot | S | low | M8-0 | waiting |
| M8-2 | The smoke fixture | S | low | M8-0 | waiting |
| M8-3 | App icon and the bundle checks | M | medium | M8-1 | waiting |
| M8-4 | Final docs and the launch kit | M | low | M8-3 | waiting |
| M8-5 | Release and close M8 | S | low | O5 | waiting |
| M9-1 | Tidewater League as pure data | M | medium | M8-5 | waiting |
| M9-2 | Bevy 0.20 gate | S (L if open) | high if open | M9-1 | waiting |
| M9-3 | The v0.2 plan and the M9 close | S | low | M9-2 | waiting |
| H3 | Render watchdog on cold CI runners | S | low | a second stall | conditional |

## 5. Work packages

### E1. Dev tools in every container (done)

`scripts/dev_setup.sh` installs what the checks need on Ubuntu 24.04: the game crate's build libraries, Xvfb with Mesa's lavapipe, xdotool, the pinned toolchain, and `just`, `typos`, `cargo-machete` and `cargo-deny` (cargo-nextest is CI-only; `cargo test --workspace --profile ci` stands in locally). It is idempotent. The cloud environment runs it at container start when the owner has approved that setup script; otherwise CLAUDE.md tells a session to run it when `just` is missing. A fresh container has no `target/`: the first `cargo build -p game --profile ci` takes 15 to 25 minutes on 4 vCPUs, so start it in the background while you read (PLAYBOOK P1).

### H1. File the unfiled good first issues (S, low)

- **Goal:** the art pass's three and M6's two "Good first issues" exist on GitHub, so both milestones can close (ROADMAP "Closing a milestone", rule 5) and M8's count stays above 15.
- **Steps:**
  1. Read the five texts: ROADMAP "Art pass (pre-stream)" and M6, each under "Good first issues".
  2. Search open and closed issues for each title, so nothing is filed twice.
  3. File each through the GitHub MCP tools: an imperative title; the ROADMAP text; a "How to verify" section with exact commands (the art pass texts carry theirs; for M6's two, write them from the code: a `sim-cli` test for `--summary`, and `data-check` plus `cargo test -p rules` for the difficulty copy); labels `good first issue` and the area, plus `help wanted` and `platform:macos` where the text needs a window.
  4. Append ` (#NN)` to each bullet in the ROADMAP's lists. The bullets are not acceptance lines.
- **Verify:** `gh issue list --repo tonianev/eonmark --label "good first issue" --state open --json number --jq length` (20 before, 25 after).
- **Done when:** the five issues exist and are linked from the ROADMAP.

### H2. One command for the whole fun gate (S, low)

- **Goal:** `just fun-gate-all [OUT]` runs exactly what CI's `fun-gate` job runs (lines 1, 2 and both clauses of 4, then the ignored `m6_` tests); CI's steps call the same script, so the two cannot drift. Add `scripts/same_outcomes.sh a.csv b.csv`, which drops the `final_hash` column and diffs two play-bots CSVs (M7-5 needs it).
- **Touches:** `scripts/fun_gate_all.sh` (new), `scripts/same_outcomes.sh` (new), `justfile`, `.github/workflows/ci.yml` (job `fun-gate`), [design/ai.md](design/ai.md) ("The fun gate in CI"), PLAYBOOK P5.
- **Steps:** move each step's `run:` body into a function of the script named `line1`, `line2`, `line4a`, `line4b` and `tests`; `all` runs every section, keeps going after a failure, prints `elapsed_s` per section and exits 1 if any section failed; each CI step becomes `scripts/fun_gate_all.sh <section>` with the same names and `if: ${{ !cancelled() }}`. The printed lines (`play-bots:`, `fun_gate:`, `check_ordering:`, `check_attack_before:`, `check_difficulty_flip:`) must not change.
- **Verify:** `bash -n` on both scripts; `just fun-gate-all /tmp/fg` locally (about 10 minutes on 4 vCPUs); `scripts/same_outcomes.sh /tmp/fg/l1.csv /tmp/fg/l1.csv` prints nothing and exits 0; CI's `fun-gate` job prints the same summary lines as its last run on `main`.
- **Stop if:** any printed number differs from `main`'s run for the same data: the move changed a command.

### H3. Render watchdog on cold CI runners (S, conditional)

Only if `game compiles on Linux` fails again with `render_watchdog: render pipeline ... not ready after 20 s` and a single `worst_ms` near 20000 (seen once, on PR #57, and green on its re-run). Then make the threshold configurable (`EONMARK_RENDER_WATCHDOG_S`, default 20), set 60 in CI's render smoke only, and test that the default stays 20. Never remove the watchdog or its ERROR gate.

### M6-1. Ingest the fun-gate matches (S, low; waits on O1)

- **Goal:** the three matches are logged in PLAYTEST with verified replays, every threshold is evaluated, and the [owner] lines O1 covers are ready for the owner's tick.
- **Serves:** M6 line 6 (the owner ticks it) and M5b's [owner] line.
- **Steps:**
  1. Read the owner's session issue. Put the three `match_log` rows into PLAYTEST "Match log (M6)" as iteration 0, keeping the owner's Fun and Notes verbatim.
  2. If the replays were attached, commit them under `docs/playtest/replays/` (about 40 KB each) and run `cargo run -p sim-cli --release --locked -- verify <file>`: each must print `OK final_hash=<the row's hash>`. If they were not, the owner's `just match-log` output with `OK 0x...` is the evidence.
  3. Evaluate every row of PLAYTEST "Fun gate thresholds (M6)": pass or fail, naming the cell that decides it.
  4. All pass: tick M5b's [owner] line if the owner's issue marks it `pass` (D6). Write the exact tick text for M6 line 6 into the PR description (PLAYBOOK P3) and ask the owner to apply it: an agent never ticks the fun gate. Then M6-3 is next.
  5. Any fail: list the failed criteria and the owner's notes in the PR, and mark M6-2 **next**.
- **Done when:** the rows and their verification are in PLAYTEST and CI is green.

### M6-2. Tuning round (M, high; conditional; at most 3 rounds)

- **Goal:** fix only the criteria the matches failed, with data, keep every fun-gate line green, and give the owner new blind seeds for the replayed matches.
- **Serves:** M6 line 6, whose own text sets the rule: every failed criterion gets a `rules.ron` tuning commit (`data/ai` may ride along, M6 owner decision 11) and the three matches are played again, at most 3 rounds before an ADR on the ruleset. M6 owner decision 18 limits the scope to what the matches exposed.
- **Steps:** PLAYBOOK P6, using the levers below in order (read `data/ai/README.md` for each field before you edit it); one `rules_version` bump (P4); the fun gate (P5); new seeds (P7) into PLAYTEST's seed table; a PR; then O1 again with `iteration 1` in the Notes.

  | Failed criterion | First lever | Second lever |
  |---|---|---|
  | The AI's army never reached your borders, or no Town fell below 50% HP, in 2 of 3 | `standard.ron` `wave_interval_permille` 1000 to 850 (more waves) | `pressure.commit_ds` 300 to 450 (a wave presses longer) |
  | A match shorter than 20 minutes because the bot won | `standard.ron` `first_raid.by_ds` 9000 to 10800 | `wave_size_permille` 1000 to 900 |
  | A match shorter than 20 minutes because you won | `pressure.reinforce_min_units` 6 to 4 | `wave_size_permille` 1000 to 1150 |
  | A match longer than 40 minutes | `rules.ron` `yield_cap_decay_step` 0 to 1 (decay from 30:00) | `harrying_bonus_hp` 0 to 2 |
  | The Yield Cap readout never turned amber | `rules.ron` `yield_cap_base` 70 to 62 | |
  | Fun below 3 | the lever the owner's notes point to | none: ask the owner which row to try |
  | The failed match was against one style | that style's gap from ai.md "Deviations" items 3 and 4, in `data/ai/personalities.ron` or `data/ai/build_orders/<style>.ron` | |

  A lever that moves a fun-gate line (line 2's first clause rides on Standard's launch gate) is measured against all of them before it ships.
- **Verify:** P6's table in the PR; `just fun-gate-all` (or CI's job) green; `just verify`; CI green.
- **Stop if:** a third round still fails a criterion: open a `needs-design` issue proposing the ADR on the ruleset that the line requires. Also stop if no variant within P6's budget passes both the criterion's proxy and every fun-gate line: post the table and the two best options for the owner.

### M6-3. Close M6 (S, low)

- **Steps:** H1 is done (M6's issues filed); [design/ai.md](design/ai.md) gets "As built" for any tuning rounds and marks its open deviations answered; BUILD_TIMES gets any new numbers; HANDOFF's state is updated; the board here too. Ask the owner in the PR whether to close the tracking issue #11.
- **Done when:** all six M6 lines are ticked on `main` (line 6 by the owner).

### M7-1. Esc pause menu and settings (M, low)

- **Goal:** [design/ui.md](design/ui.md)'s pause menu. Esc in a live match opens a menu with Resume, UI scale, Volume, Borderless fullscreen and Quit to menu. The sim does not tick while it is open, and UI scale and volume persist.
- **Serves:** M7's "pause menu" deliverable. D1 allows it before M6 closes.
- **Spec:**
  - Esc in normal mode opens the menu and clears the selection; in attack-move or placement mode Esc only leaves the mode, as today (ui.md, controls table). Esc again, or Resume, closes it.
  - Opening sets `PauseReason::Menu` through `speed::GameSpeed`, which already gates world input (`world_input_allowed`, `the_menu_pause_gates_world_input_and_the_key_pause_does_not`). Closing restores the previous pause state, so a Space pause stays on.
  - It is not available in the main menu or after the outcome (the game-over screen owns that state). It is available while spectating.
  - UI scale: 75, 100, 125 and 150% through Bevy's `UiScale`. Volume: 0 to 100 in steps of 10, stored now and wired to audio by M7-3. Borderless fullscreen switches `WindowMode::BorderlessFullscreen(MonitorSelection::Current)` and back to `Windowed`; exclusive fullscreen is never used.
  - Quit to menu ends the match through `match_scope`, like the game-over screen's Main Menu, and the replay is finished cleanly: it verifies, ending at the current tick with no outcome.
  - `settings.ron` (UI scale and volume, as integer percents) lives in the config directory of the same `directories` project the replays use. It is read at startup and written on change; a missing or unreadable file gives the defaults and one `warn!`.
  - `--pause-menu-at-tick <n>` opens it at an exact tick, for tests and the screenshot, like `--pause-at-tick`.
- **Touches:** `crates/game/src/menu/pause_menu.rs` (new), `menu/mod.rs`, `menu/widgets.rs` (reuse it), `speed.rs`, `selection.rs`, `cli.rs`, `app.rs`, `settings.rs` (new), `data/rules/strings/en.ron` (and `HUD_KEYS` in `economy_hud.rs` if the keys are registered there), [design/ui.md](design/ui.md), PLAYTEST (a new "Automated proxies (M7)" section), `docs/screenshots/m7-pause-menu.png`.
- **Tests (`cargo test -p game`):** `esc_opens_the_menu_pauses_the_sim_and_clears_the_selection`, `esc_in_attack_move_only_leaves_the_mode`, `resume_restores_a_space_pause`, `no_tick_while_the_menu_is_open`, `quit_to_menu_finishes_a_replay_that_verifies`, `ui_scale_and_volume_round_trip_through_settings_ron` (in a temp dir), `an_unreadable_settings_file_gives_the_defaults`, `the_pause_menu_fits_1280x720` (the game-over panel's fit test is the pattern).
- **Verify:** `cargo test -p game --profile ci`; `cargo clippy -p game --all-targets --locked --profile ci -- -D warnings`; one windowed run under Xvfb (PLAYBOOK P8) with `--skip-menu --seed 1 --pause-menu-at-tick 200 --screenshot-at 210:docs/screenshots/m7-pause-menu.png --background --exit-after-seconds 60`, then read the PNG; `just ci`.
- **Done when:** CI is green, the screenshot is read and committed, and ui.md describes the menu as built.
- **Stop if:** UI scale 125% or 150% breaks a HUD fit test and the fix is not local. Ship 75% and 100% only and file an issue for the rest.

### M7-2. Vendor the Kenney UI, icons and audio (S to M, medium)

- **Goal:** the CC0 files M7-3 and M7-4 need are vendored with sha256 pins and `ATTRIBUTION.md` rows, and `scripts/check_assets.sh` (with `--self-test`) passes.
- **Needs:** kenney.nl, which this environment blocks by default ([ART_STYLE.md](ART_STYLE.md), "Where the art pass's files come from"). Check with `curl -sS -o /dev/null -w '%{http_code}\n' https://kenney.nl/assets/ui-pack`. If it fails, follow PLAYBOOK P10: ask for that one host through the environment tools; if the owner declines, use O3's inbox branch.
- **Packs:** all CC0 on kenney.nl. Read the licence file in each zip and require "CC0" before using a file.
  - UI Pack, or its RPG Expansion, whose brown and beige panels sit closer to the palette: the panels, buttons and frames M7-4 uses (about 10 PNG files).
  - Game Icons: grain, lumber, ore, lore, move, attack and stop (about 8 PNG files).
  - Interface Sounds, Impact Sounds and Music Jingles (or RPG Audio): the 11 cues of M7-3, as OGG.
- **Steps:**
  1. Extend `tools/assets/sources.tsv` and `scripts/fetch_assets.sh` with zip-member rows (zip URL, zip sha256, member path, file sha256). Read the script first and make the smallest change that fits it.
  2. Vendor into `assets/ui/kenney_<pack>/`, `assets/icons/kenney_game_icons/` and `assets/audio/kenney_<pack>/`, each pack's licence text with its own row.
  3. Write the `ATTRIBUTION.md` rows: asset, author (Kenney), source page URL, CC0-1.0, modifications `none`, download date.
  4. Nothing AI-generated ([AI_CONTRIBUTIONS.md](../AI_CONTRIBUTIONS.md)).
- **Verify:** `scripts/check_assets.sh` and `scripts/check_assets.sh --self-test`; `cargo test -p inspect_gltf` (`attribution_rows_match_the_sources`); no file over 2 MB; `just ci`.
- **Stop if:** a pack has no CC0 licence text, or the host stays blocked and O3 has not happened.

### M7-3. Audio (M, low)

- **Goal:** about ten CC0 cues through one facade, with the volume from M7-1's setting, and silent in every automated run.
- **Spec:**
  - `crates/game/src/audio.rs` holds `enum Cue { UiClick, OrderMove, OrderAttack, BuildPlaced, BuildingComplete, UnitTrained, ResearchComplete, AgeAdvanced, UnderAttack, Victory, Defeat }` and one table from cue to `assets/audio/...ogg`.
  - `SfxPlugin` plays a cue on:
    - a widget button press;
    - an accepted human order;
    - the local player's own sim events: trained, completed, researched, advanced (take the event names from `SimEvent` in `crates/sim/src/state.rs`);
    - the existing under-attack alert (`alert.rs`);
    - the outcome for the local player (spectating plays Victory).
  - Each cue has a minimum interval of 200 ms; UnderAttack follows the alert's own throttle.
  - `GlobalVolume` comes from the volume setting.
  - `--background` and a new `--mute` silence everything, so agent runs on the owner's Mac stay quiet.
  - Nothing in the sim reads audio.
- **Tests:** `every_cue_file_exists_and_is_ogg`, `local_events_map_to_cues_and_enemy_events_do_not`, `a_cue_is_throttled`, `background_mode_is_silent`, `the_volume_setting_reaches_global_volume`.
- **Verify:** `cargo test -p game --profile ci`; `cargo tree -i cpal --depth 0` prints one version (M7 line 5's clause: paste it); a 60 s windowed run logs no ERROR (`No audio device found` is a WARN under Xvfb); `just ci`.
- **Docs:** ui.md "Audio"; ART_STYLE's asset rows; one line in STREAMING.md that OBS should capture the game's audio.

### M7-4. Kenney panels and icons (M, medium)

- **Goal:** menus, the HUD and the game-over screen use Kenney 9-slice panels (`NodeImageMode::Sliced`) tinted to the existing palette, plus resource and order icons, without breaking any fit test or the ART_STYLE checklist.
- **Rules:** every text and every palette colour stays. Panels replace flat rectangles only, and icons sit beside labels, never instead of them. Start in `menu/widgets.rs`, then the top bar (`economy_hud.rs`), the bottom bar and command card, the selection panel, the minimap frame and the game-over screen. Load the images once (a `UiSkin` resource) and tint the slices with the palette's panel colours, keeping the HUD matte (ART_STYLE).
- **Steps:** retake the screenshots that show the HUD (the `m6-*.png` set; `art-match-minimap.png` only if it changes, and then say in the PR that AP8's sign-off covers the new look). Read each one against the ART_STYLE checklist and write the checklist ticks into the PR.
- **Tests:** every existing fit test passes unchanged; add `panels_use_the_skin_and_keep_palette_tints` and `icons_do_not_change_label_text`.
- **Verify:** `cargo test -p game --profile ci`; `just render-smoke` and `scripts/check_draws.sh` (9-slice panels add draw calls: keep `art_bench` under 450 for headroom); `just ci`.
- **Stop if:** a fit test fails at 1280x720 and the fix is not local. Ship the panels on the menus only and file the HUD part as an issue.

### M7-5. Fog in the sim and the faction choice (L, medium; waits on M6-3)

- **Goal:** the milestone's one bump (`SIM_VERSION` 18 to 19, `rules_version` 31 to 32). It adds the hashed visibility grid ([design/fog.md](design/fog.md)), `SimView`'s filtered queries, the `fog_walkthrough` scenario and golden, and each player's faction in the match setup, with no change to any game's outcome: the bot still reads the unfiltered view ([ADR 0004](adr/0004-bot-reads-the-unfiltered-view.md)).
- **Serves:** M7 line 1 (ticked here); lines 2 and 5 (prepared); M9 lines 1 and 2, because this plumbing must ship in v0.1.0 for M9's faction to be data only.

**Session 1, fog:**

1. Branch from `main`. Bump `SIM_VERSION` (`crates/sim/src/state.rs`) and `rules_version` (`data/rules/rules.ron`) with the one-line reason "M7: hashed visibility grid (fog of war) and each player's faction in the match setup". This is M7's single bump: every later commit of this WP re-records without bumping again, and the PR stays a draft until session 2 ends.
2. Write `crates/sim/src/visibility.rs` as fog.md "The visibility grid" specifies:
   - 128 x 128 `u8` per player, with states 0, 1 and 2;
   - each tick, decay and then stamp, using the integer disc test;
   - units stamp from the floor of their position;
   - buildings stamp from their footprint's centre tile (`kind_centre_tile`), and construction sites and annexed Towns stamp for their current owner;
   - the field goes last in `State`, is serialised in the snapshot, and has nothing to rebuild in `restore()`.
3. Run it in `Sim::step` after movement, combat deaths and construction, before the outcome check, and add it with its number to [DETERMINISM.md](DETERMINISM.md)'s step list.
4. Add `visibility` to `SubHashes::NAMES` (fog.md's open question, answered yes), and fix every test that counts the names.
5. Add to `SimView`: `visibility(p) -> &[u8]`, `tile_state(p, tile) -> u8`, `units_visible_to(p)` (own units, and others' units on state 2 tiles) and `buildings_known_to(p)` (own buildings, and others' whose centre tile is state 1 or 2). In the bot's seam (`visible_enemies` and `Enemies` in `crates/ai/src/standard/facts.rs`), change only the doc comments, to cite ADR 0004.
6. Write `crates/sim/tests/m7.rs`. Every name starts with `fog_`, so `cargo test -p sim fog` runs them all:
   - `fog_grid_matches_vision_radii_for_a_fixture`: step a fixture tick by tick, keep an independent expected grid from the disc rule and the union of every earlier visible set, and compare every 50 ticks;
   - `fog_an_enemy_unit_outside_all_vision_is_hidden_in_simview`;
   - `fog_explored_never_returns_to_unexplored`;
   - `fog_restore_mid_match_matches_the_uninterrupted_run`;
   - `fog_two_runs_are_identical`.
7. `just rerecord` (every `.hash` changes, since the grid is hashed state); `just verify`; `just fixture-policy origin/main`.
8. Prove that nothing else changed (PLAYBOOK P5, "Same outcomes"). Run M6 line 1's and line 2's play-bots commands on `main` and on the branch: `scripts/same_outcomes.sh` must print nothing for all three CSVs. `m5_first_match` and `scripted_match` must end on the same tick with the same outcome as before.
9. Check the budget:
   - warm `cargo test -p sim --profile ci`, three runs, must stay under 10 s;
   - `sim-cli bench --units 400 --ticks 1200` and `--combat`, before and after, with the rows added to BUILD_TIMES;
   - if the suite goes over 10 s, stamp every N ticks through `vision_stamp_every_ticks` in `rules.ron` (default 4), inside this same bump, with fog.md and the tests following.

**Session 2, factions and the scenario:**

10. `MatchSetup` gains each player's faction index: a header field appended last, documented in DETERMINISM's header table, and validated against `factions.ron` by `Sim::new`. Every place that resolves Modifier rows uses the player's own faction instead of `factions[0]`.
11. `sim-cli play-bots --faction-a <id> --faction-b <id>` (default `freeholders`), shown in the summary line. `match-log` prints each faction.
12. Test with a second faction written into a temp copy of `data/`, never into `data/` itself: Tidewater is M9's data-only commit. Cover the Yield Cap at Trade level 0 (`Mul 1250` on `yield_cap_base` 70 gives 87, [design/economy.md](design/economy.md)), `TownBorderRadius Add -1`, the header round trip, and the play-bots flags. (The gather-rate and Watchtower targets of M9's row are M8-0's.)
13. Add `fog_walkthrough` to `sim::scenarios::NAMES`, after the `economy_walkthrough` pattern: P0 sends an Outrider east into unexplored land, sees a P1 group, loses sight of it, and explores P1's Seat, over 1200 to 1800 ticks. Record its golden; a new fixture needs no extra bump. Add it everywhere CI lists fixtures: `grep -n m5_first_match .github/workflows/ci.yml` finds the macOS verify, the Linux verify and the hash-parity compares.
14. Update the docs:
    - fog.md: "As built (M7)";
    - DETERMINISM: "What M7 adds" (the grid, the sub-hash, the step, the header field, the versions);
    - ARCHITECTURE's code map;
    - DATA_FORMAT: the faction in the header;
    - the fixtures README;
    - BUILD_TIMES.

- **Verify:** `cargo test -p sim --profile ci fog`; `cargo test --workspace --profile ci`; P5's same-outcomes check; `just verify` (13 OK lines); `just fixture-policy origin/main`; `just ci`; CI green on every job, the unchanged fun-gate lines and hash parity included.
- **Tick:** M7 line 1, after CI is green (PLAYBOOK P3), with the `fog_` test lines and `just verify`'s output.
- **Stop if:** the same-outcomes check prints anything (the grid leaked into behaviour: find the read; never re-tune to hide it), or the sim suite stays above 10 s even at a stamp cadence of 4.

### M7-6. Fog in the game (L, medium; waits on M7-5)

- **Goal:** the human sees fog: unexplored land is black, explored land darkened (0.45), visible land lit. Enemy units vanish without friendly vision, and the minimap darkens and filters its dots in the same frame. Spectating shows no fog. New Game gains the faction row and the bot-vision disclosure.
- **Serves:** M7 line 2 (ticked here), line 5's lavapipe proxy and line 6's proxy screenshot.
- **Steps:**
  1. `field.rs`: write the local player's grid into the field texture's B channel (fog.md "Rendering"; the byte encoding is your choice, and the shader maps it to 0.0, 0.45 and 1.0). Widen `sync_field`'s change test to the grid by comparing its 16 KiB with the last written copy. `--spectate` writes "visible" everywhere.
  2. `field.wgsl`: multiply the albedo after the territory tint and isoline. Judge the edge softness on the screenshot (fog.md's open question) and record the choice.
  3. `present.rs`, and the bars, rings and banners keyed by `UnitId`:
     - enemy units are `Visibility::Hidden` unless their tile is state 2;
     - enemy buildings and sites are hidden while their centre tile is state 0;
     - hit effects are drawn only when the source or the target is visible.
  4. `minimap.rs`: a cached fog layer rebuilt when the grid changes. Enemy unit dots appear only on state 2, enemy buildings on state 1 or 2. Reuse the "repaints in the update of its tick" test pattern.
  5. Orders: a hidden enemy cannot be selected or attacked by a click, and a right click on fogged ground is a move.
  6. New Game: a faction row listing every faction in `factions.ron` (one today, Freeholders), the choice reaching the replay header. Under the difficulty row, and on the game-over screen beside the bonus, add the disclosure that the bot sees the whole map (a new string; ADR 0004).
  7. Screenshots at 1280x720 from `--scenario fog_walkthrough` with `--screenshot-at`:
     - `docs/screenshots/m7_town.png`: P0's Town with explored and unexplored land in view;
     - `m7-fog-minimap.png`;
     - `m7-new-game.png`.

     Read each one against the ART_STYLE checklist.
  8. CI: add `fog_walkthrough` to game-linux's render smoke, with the draw-call gate.
- **Tests:**
  - `the_field_texture_carries_the_local_players_fog_in_b`
  - `spectating_writes_no_fog`
  - `an_enemy_unit_hides_in_the_frame_its_tile_leaves_vision`
  - `an_enemy_building_stays_shown_on_explored_tiles`
  - `the_minimap_fog_layer_repaints_in_the_update_of_its_tick`
  - `a_hidden_enemy_is_not_pickable`
  - `new_game_lists_every_faction_and_the_choice_reaches_the_header`
  - `the_bot_vision_disclosure_is_shown`
- **Verify:** `cargo test -p game --profile ci`; `just render-smoke` on `fog_walkthrough` with `scripts/check_draws.sh` under 500; `just ci`; CI green.
- **Tick:** M7 line 2, with the tests, the scenario's replay verification and the screenshots (the M3b and M5a precedent: lavapipe screenshots stand in for `ci_testing`).
- **Stop if:** render smoke logs any pipeline ERROR and the fix is not in `field.wgsl`.

### M7-7. Close M7 (S, low)

- **Steps:**
  1. Finalise ART_STYLE: the checklist, the import rules, the palette marked final, and the Kenney rows.
  2. Re-run `scripts/check_assets.sh` and paste its output (AP1's note asks for it at M7's close).
  3. Update fog.md and ui.md to "as built", plus BUILD_TIMES and HANDOFF.
  4. File two or three good first issues, for example a fog edge-softness setting or a settings reset button.
  5. Add PLAYTEST "Owner's Mac checklist (M7)" with O4's exact commands, and ask the owner for O4.
- **Done when:** every M7 line without [owner] or a Mac clause is ticked, and O4 is requested.

### M8-0. M9 readiness: data-only factions after the tag (M, medium; waits on M7-7 and O2)

- **Why:** M9 line 1 needs `git diff v0.1.0 -- crates/ | grep -c .` to print 0 for the Tidewater commit. Three things in today's tree make that impossible:
  - A new faction changes `rules_hash`, so every golden fixture's header is re-recorded (ADR 0003's header-only pattern), and the fixtures live under `crates/sim/tests/fixtures`.
  - Two tests assert one faction (`crates/rules/src/lib.rs` and `crates/sim-cli/tests/data_check.rs`, `factions=1`).
  - The Tidewater row's "lumber gather Mul 1100" and "Watchtower border push Add -1" ([design/tech-and-ages.md](design/tech-and-ages.md)) have no `StatPath` target.
- **Steps:**
  1. `git mv crates/sim/tests/fixtures fixtures`, the top-level directory M8's line 7 already names (`fixtures/smoke.eonreplay`). Update every reference: `grep -rln "tests/fixtures" --include=*.rs --include=*.sh --include=*.yml --include=*.md --include=justfile .` finds about 25 files. The tests join `CARGO_MANIFEST_DIR`, the scripts set `dir=`, and ci.yml lists paths.
  2. Make crates/ tests independent of the faction count and of `rules_hash` values: require Freeholders first instead of exactly one faction, match `factions=\d+`, and grep the tests for `rules_hash=0x` literals.
  3. Add `StatPath::GatherRate(<resource id>)` and `StatPath::BorderRadius(<building kind id>)` at the end of the enum, so existing rows serialise as before and `rules_hash` does not move. Apply them in the gather step and the territory sources, with validators and tests that use a temp data copy. With no rows in `data/`, nothing changes: every `.hash` stays byte-identical and no bump is needed.
  4. Dry run, not committed: on a scratch branch add Tidewater's row (`YieldCap Mul 1250`, `GatherRate("lumber") Mul 1100`, `BorderRadius("watchtower") Add -1`) and its strings to `data/`, bump `rules_version`, run `just rerecord` and `cargo test --workspace --profile ci`, then `git diff main -- crates/ | grep -c .` must print 0, and `sim-cli play-bots --faction-a tidewater --faction-b freeholders --seeds 1..2` must run. Paste the outputs into the PR and delete the scratch branch.
- **Verify:** `just ci`; `just verify`; CI green, every job finding the moved fixtures.
- **Done when:** the dry run prints 0 and CI is green.

### M8-1. CI hygiene (S, low)

- **Serves:** M8 line 6.
- **Steps:**
  1. Pin every action to a full commit SHA, keeping the version as a comment (`uses: actions/checkout@<sha> # v7`). Resolve each with `git ls-remote --tags https://github.com/<owner>/<repo> '<tag>*'`, taking the peeled `^{}` SHA of an annotated tag. Dependabot's github-actions updates keep the pins current.
  2. Find a Dependabot grouped PR that passed CI; if none exists, say in the PR when the next weekly run is due and ask the owner to merge it once it is green.
  3. Confirm `cargo deny check`, `cargo machete`, `typos`, `scripts/check_trademark.sh` and `cargo doc --workspace --no-deps` run in CI, and link the run.
- **Verify:** CI green; `grep -n "uses:" .github/workflows/*.yml` shows only SHAs.

### M8-2. The smoke fixture (S, low)

- **Serves:** M8 line 7.
- **Steps:**
  1. Add a short `smoke` scenario to `sim::scenarios::NAMES`: economy, training and one skirmish, about 600 ticks.
  2. Record `fixtures/smoke.eonreplay` and its hash; a new fixture needs no bump.
  3. CI's headless job runs `--headless-run fixtures/smoke.eonreplay` against `fixtures/smoke.hash`.
  4. `release-check.yml` runs the same with the release binary after bundling and prints its cold wall time; BUILD_TIMES records it (not gated).
- **Verify:** CI green; a `workflow_dispatch` run of release-check on the branch (GitHub MCP `actions_run_trigger`), with its smoke step and wall time pasted.

### M8-3. App icon and the bundle checks (M, medium)

- **Serves:** M8 line 1's checks that can be automated: `plutil`, `codesign --verify`, the bevy_dylib check and `otool` minos 13.0. The Dock name and icon are the owner's, in O5.
- **Steps:**
  1. Make `assets/icon.iconset` from an Eonmark render of a CC0 KayKit Town with its team banner on a plain background (a game run with `--window-size 1024x1024`, a `--camera` framing and `--screenshot-at`), scaled to the ten iconset sizes by a script under `tools/`. Nothing AI-generated.
  2. Add the `ATTRIBUTION.md` rows: derived from KayKit Medieval Hexagon (CC0), rendered by Eonmark and scaled.
  3. Run release-check by `workflow_dispatch` on the branch and paste the `plutil`, `codesign`, arm64, bevy_dylib and `otool -l` (`LC_BUILD_VERSION`) output.
- **Stop if:** release-check fails in a way only a Mac can debug. Write it up for the owner.

### M8-4. Final docs and the launch kit (M, low)

- **Serves:** M8's deliverables (README, CONTRIBUTING and RELEASING final, DEPENDENCIES current, the deferred list), line 4's cold-build quote and line 5's preparation.
- **Steps:**
  1. README: what Eonmark is, a screenshot, the download with the Gatekeeper "Open Anyway" steps and the `xattr` alternative, building from source with the cold build time from BUILD_TIMES, the controls, and the bot's disclosed bonus and vision.
  2. CONTRIBUTING: the fixture move and the release flow.
  3. RELEASING: the tag procedure as `release.yml` runs it.
  4. DEPENDENCIES: re-verify each version where crates.io is reachable and record any drift.
  5. Check ROADMAP's deferred list against what shipped.
  6. In `docs/launch/`:
     - the bevy-assets TOML entry: Apps, a description under 100 characters, and a 16:9 image under 2 MB and at most 600 px wide, taken from the game;
     - the Bevy Discord #showcase and r/rust_gamedev drafts.
- **Verify:** `typos`; `scripts/check_trademark.sh`; every link resolves; `just ci`.

### M8-5. Release and close M8 (S, low; waits on O5)

- **Steps:**
  1. Check the release with GitHub MCP `get_latest_release`: the zip, the dSYM zip and `SHA256SUMS`, with notes naming `sim_version`, `rules_version` and `rules_hash`.
  2. Fill "Launch links (M8)" from the owner's issue.
  3. Tick M8's lines with the evidence: CI and release-check for the agent's lines, the owner's issue for the rest.
  4. File M8's good first issues and update HANDOFF.
- **Rule:** from the tag until M9-1 merges, no PR may change anything under `crates/`, because M9 line 1 measures that tree.

### M9-1. Tidewater League as pure data (M, medium; waits on M8-5)

- **Steps:**
  1. From `main` (equal to `v0.1.0` under `crates/`), add the faction to `data/rules/factions.ron`: `YieldCap Mul 1250`, `GatherRate("lumber") Mul 1100`, `BorderRadius("watchtower") Add -1`.
  2. Add its strings, bump `rules_version` with the reason, and run `just rerecord`. Headers only: every `.hash` stays byte-identical.
  3. Tune the modifiers' values (data only, PLAYBOOK P6) until the line's command, `play-bots --a standard --b standard --faction-a tidewater --faction-b freeholders --seeds 1..20 --jobs 5`, gives Tidewater 7 to 13 of 20. Report the swapped seats too.
  4. Write `docs/design/factions.md` with the play-bots table.
  5. Take a screenshot of New Game with Tidewater selected and the HUD showing Yield Cap 87.
- **Verify:** `git diff v0.1.0 -- crates/ | grep -c .` prints 0 (it exits 1 when it prints 0, so read the number); `sim-cli data-check data`; `just verify`; CI green.
- **Tick:** M9 lines 1 and 2.
- **Stop if:** 7 to 13 cannot be reached with these three rows. Post the table, and the owner picks a value or a fourth row.

### M9-2. Bevy 0.20 gate (S to check; L, high, if the gate opens)

- **Steps:** check the trigger exactly as M9 line 3 words it: a stable 0.20.x in `cargo info bevy`, and releases of bevy_egui and bevy-inspector-egui on bevy ^0.20 that share one egui version.
  - Closed: record the date and the unmet conditions in [ADR 0002](adr/0002-bevy-0-20-migration.md), and close with the faction work, as the line allows.
  - Open: do not start the migration in a routine session. Put the migration plan into a PR description (ADR 0002's steps, plus IMPLEMENTER_NOTES's traps: the draw_stats internals, `monitor_loss.rs`, the WESL shader, the picking API) and ask the owner how to run it. It is the riskiest change left.

### M9-3. The v0.2 plan and the M9 close (S, low)

- **Steps:** write the v0.2 plan section in ROADMAP from the candidates table, each item naming its ADR; file M9's good first issues; update HANDOFF and BUILD_TIMES; tick M9 line 4 with its two printed counts.

## 6. Decisions this plan rests on

| Id | Decision | Source |
|---|---|---|
| D1 | M7's game-only WPs (M7-1 to M7-4) may run before M6 line 6 closes. M7-5, which changes the sim, waits for M6 to close. This also covers starting them before M5b's [owner] line passes (O1 covers it). | Owner, 2026-11-08 (ROADMAP, "Order of work") |
| D2 | The v0.1 bot reads the unfiltered view; fog is the human's, disclosed beside the bonus; a fog-obeying bot is a v0.2 candidate. | Owner, 2026-11-08, [ADR 0004](adr/0004-bot-reads-the-unfiltered-view.md) |
| D3 | The decision-17 balance gaps are tuned in M6-2 only where the three matches expose them; the rest become `kind:balance` issues for after v0.1. | M6 owner decision 18, 2026-11-08 |
| D4 | M6 owner decisions 15 to 17 (Easy's 33:00 all-in, Hard never retreats, the gaps to slice 4) are recorded in the ROADMAP's M6 Notes. | Owner, 2026-10-30 |
| D5 | One WP per session and one PR per WP. No multi-agent workflows; at most one narrow subagent call for a search or a log. The owner merges. | This plan |
| D6 | [owner] lines are ticked by the owner, or by the agent when the owner's session issue marks that line `pass` with its output. M6 line 6 is ticked only by the owner. | This plan, keeping the owner's rule for line 6 |
| D7 | The golden fixtures move to `fixtures/` in M8-0. M9 line 1 needs it, and M8 line 7 already names the path. | This plan |
| D8 | The faction choice ships in M7-5, inside M7's single bump; the gather-rate and per-building border targets ship in M8-0 with no bump. | This plan |
| D9 | Spectating shows the whole map (streams and bot-vs-bot viewing). | This plan |
| D10 | Only UI scale and volume persist, in `settings.ron` (ROADMAP's deferred list keeps every other setting out). | This plan |
| D11 | Audio comes from Kenney's CC0 packs rather than Freesound: one source, one licence file per pack, the same host as the UI. | This plan (ART_STYLE allows both) |

The owner may overturn any row marked "This plan" by saying so in an issue or a PR; the next session updates this table and the WPs it touches.

## 7. Risks

| Risk | Handling |
|---|---|
| The owner's sittings (O1, O2) are the critical path | D1 keeps game-only M7 work moving; O1 and O2 are written to be done in one sitting each and posted as an issue, with no agent waiting on them |
| Tuning loops (M6-2, M9-1) burn sessions | PLAYBOOK P6's budget of six measured variants per session, the line's own three-round limit, and stopping to ask the owner |
| M7-5's hash churn hides a behaviour change | The same-outcomes check (P5) proves no game changed; one bump on one draft PR across both sessions |
| The grid breaks the 10 s `cargo test -p sim` budget | Measured in M7-5; stamp cadence as a data field in the same bump |
| Blocked asset hosts | One environment approval for kenney.nl, or O3 |
| A cheaper agent judging looks (M7-4, M7-6) | The ART_STYLE checklist written into the PR for every screenshot; the owner signs AP8 and M7 line 6 |
| Mac-only failures in M8 | release-check on macOS runners by `workflow_dispatch`; Mac clauses go to O5 |
| M9's data-only line | M8-0's fixture move, data-agnostic tests, new modifier targets and dry run |
| A data change silently moves the fun gate | CI's `fun-gate` job on every PR; H2's one local command |
| A rare CI stall (the render watchdog on a cold runner) | One re-run; H3 if it happens twice |

## 8. Budget

| Track | Agent sessions | Owner |
|---|---|---|
| M6 close | 2 (plus 0 to 3 tuning rounds) | O1 (2 to 2.5 h, plus 2 h per extra round) |
| M7 | about 9 | O3 if needed (15 min), O4 (20 min) |
| M8 | about 6 | O2 (1 to 1.5 h, before M8), O5 (about 2 h) |
| M9 | about 3 (more if the Bevy gate opens) | none |
| Housekeeping | 1 to 2 | |
| Total | about 21 to 25 | about 6 to 8 hours |

## Found while working

Sessions append one line per problem found outside their WP, with the file and why it matters, instead of fixing it in their PR. Triage turns each line into an issue or a WP.

- (none yet)
