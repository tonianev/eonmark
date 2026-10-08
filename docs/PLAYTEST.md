# Playtest

This document holds two things: the controls checklist that closes M2, and the match log that the M6 fun gate requires. Both are filled by hand by the owner or a tester, with a date, after playing a build from `main`. The checklist states what each control must do; the log records what a full human-versus-Standard match felt like. Empty cells mean not yet tested. The fun-gate thresholds that these logs are checked against are listed at the end.

## Controls checklist (M2)

Run `cargo run -p game --features dev -- --scenario units200` from `main` (200 Yeomen at the west start, nothing scripted: you drive). Mark each row Pass or Fail with the date, the commit short sha and your initials in the Signed column; a row without initials is not signed off. The Proxy column names the automated check that already covers the row (see [Automated proxies](#automated-proxies-m2)); `owner` means only a human can check it.

| Control | Expected | Proxy | Pass/Fail | Date | Commit | Signed | Notes |
|---|---|---|---|---|---|---|---|
| Two-finger trackpad scroll | Pans the camera; arrives as `MouseScrollUnit::Pixel` | owner | | | | | |
| Pinch on trackpad | Zooms between 15 m and 60 m with smoothing | owner | | | | | |
| Mouse wheel | Zooms; arrives as `MouseScrollUnit::Line` | owner | | | | | |
| W, A, S, D | Pans the camera. Note: with units selected, S also issues Stop and A also arms attack-move (`camera.rs` and `orders.rs` both read the key); the key overlap is an open question in [design/ui.md](design/ui.md) | owner | | | | | |
| Arrow keys | Pans the camera | owner | | | | | |
| Edge scroll | Cursor at a screen edge pans in that direction | owner | | | | | |
| Camera travel at 30 fps and 120 fps | Holding D for 2 s moves the camera the same distance under `--max-fps 30` and `--max-fps 120` | `scripted_moves` 30 vs 120 hash equality (sim side); camera distance is owner | | | | | |
| Left click on a unit | Selects it; a flat ring appears under it | owner | | | | | |
| Shift-click on a unit | Adds to or removes from the selection | owner | | | | | |
| Drag box | Selects every own unit whose position projects inside the box | owner | | | | | |
| Double-click a unit | Selects every unit of the same kind visible on screen | owner | | | | | |
| Ctrl+1 | Assigns the selection to group 1 | unit tests in `selection.rs` | | | | | |
| 1 | Recalls group 1 | unit tests in `selection.rs` | | | | | |
| Right-click on ground | Issues Move; a short-lived marker ring appears at the click point; units arrive | `units200_auto` (scripted Move, frame stats) | | | | | |
| Shift+right-click | Queues a Move after the current one | owner | | | | | |
| S | Stop for the selection | `scripted_moves` (Stop during tick 400) | | | | | |
| A, then left click on ground | Attack-move placeholder: the units move exactly as with Move (the sim treats `AttackMove` like `Move` until M4a); Esc or a right click before the click cancels the mode | `scripted_moves` (AttackMove during tick 450) | | | | | |
| Esc | Clears the selection and leaves attack-move mode (the pause menu arrives in M6) | owner | | | | | |
| Right-click with the pointer over the bottom bar | No Move is issued, no marker appears | `hud_click` | | | | | |
| Click the Stop button | The selection is unchanged; the units stop | `hud_click` | | | | | |
| Close via the red window button | Process exits with code 0 (`echo $?`); the replay has its trailer | `--close-window-after-seconds` | | | | | |
| Cmd-Q | Same as above through the `Quit Eonmark` menu item; see [Cmd-Q](#cmd-q-m2) | `--quit-via-menu-after-seconds` | | | | | |
| `kill -9` the process mid-match, then `sim-cli verify` on its replay | `warning:` about the missing trailer, then `OK`; at most one second of commands lost | `scripts/m2_checks.sh` step 4 | | | | | |
| Window resize | HUD stays anchored; world picking still correct | owner | | | | | |

Every windowed session records a replay. The directory is `~/Library/Application Support/com.tonianev.Eonmark/replays/` unless `--replay-dir <dir>` was passed, and the file is `<yyyymmdd-hhmmss>-<seed>.eonreplay` (UTC wall clock, then the match seed). The exact path is the `replay: <path>` line on stdout at startup, so a run started from a terminal tells you which file to verify.

## Cmd-Q (M2)

What the menu item does. `crates/game/src/macos_menu.rs` replaces winit's default application menu with a muda menu whose Quit item is a custom `MenuItem` (id `quit`, label `Quit Eonmark`, accelerator Cmd+Q). Activating it only emits a `MenuEvent`; the `drain_menu_events` system reads it on the AppKit main thread and writes `AppExit::Success`. On that same frame `finish_recorder_on_exit` (in `Last`) records a final hash checkpoint, sends `Finish` to the replay writer thread and joins it, so the file ends with the `EONRDONE` trailer and `sim-cli verify` prints `OK` without a `warning:`. The red close button takes the same `AppExit` route (`bevy_window::exit_on_all_closed`, also in `Last`).

Why not `PredefinedMenuItem::quit`. muda's predefined Quit, like winit's default Quit, calls `NSApp terminate:` directly: Bevy's runner returns without running any schedule, no `AppExit` is written, the recorder never gets `Finish`, and the replay is left without its trailer. It would still verify, but as a truncated recording, and the acceptance line asks for a clean exit. Details in [design/macos-packaging.md](design/macos-packaging.md).

The owner check (ROADMAP M2 acceptance, "play 2 minutes, press Cmd-Q"):

```bash
cargo run -p game --features dev -- --scenario units200       # note the `replay: <path>` line
# play for two minutes: select, move, stop, attack-move, then press Cmd-Q
echo $?                                                        # must print 0
replay=$(ls -t ~/Library/Application\ Support/com.tonianev.Eonmark/replays/*.eonreplay | head -n 1)
cargo run -p sim-cli --release -- verify "$replay"             # one line: OK final_hash=0x... ticks=N
```

Pass when `echo $?` prints `0`, `verify` prints exactly one `OK ...` line, there is no truncation warning on stderr (`warning: <file> is a truncated recording ...` means the trailer is missing, which is the `terminate:` bug; the unrelated `warning: <file> has AI slots; they are driven by ai::Passive` line appears for every skirmish replay until M5b and is fine), and `ticks` is about 2400 for two minutes at 20 Hz. Also confirm with the menu open that the application menu reads About, Services, Hide, Hide Others, Show All and `Quit Eonmark` with the Cmd+Q glyph, and that no second Quit item exists. The predefined items carry the process name, so under `cargo run` they read `About eonmark` and `Hide eonmark`; in the bundled app (`CFBundleName` Eonmark, M8) they read `About Eonmark`. Only the custom item is spelled `Quit Eonmark` in both. The same list can be read without clicking:

```bash
osascript -e 'tell application "System Events" to tell (first process whose name is "eonmark") to get name of every menu item of menu 1 of menu bar item 2 of menu bar 1'
# About eonmark, missing value, Services, missing value, Hide eonmark, Hide Others, Show All, missing value, Quit Eonmark
```

## Automated proxies (M2)

The implementer cannot press keys or click, so each proxy drives the same code path from a flag or a scenario. Run them from the repo root; paste the output into the PR.

Background mode. Every automated or agent-run windowed check runs in background mode, so the window never pops up in front of the owner's work: pass `--background` or export `EONMARK_BACKGROUND=1` (`scripts/m2_checks.sh` and `just m2-checks` export it; `1`, `true` and `yes` turn it on). The window then opens unfocused, at `WindowLevel::AlwaysOnBottom` (below every normal window) in the top-left corner of the primary monitor; on macOS the game switches itself to the `Accessory` activation policy (no Dock icon, no menu bar) and, during its first three seconds, hands activation back to the app that was frontmost when it started (winit activates the app before any Bevy system runs, and bevy_winit 0.19.1 does not expose the winit options that would prevent it). It also runs `WinitSettings::continuous()`: Bevy's default throttles an unfocused window to 60 updates a second, which would make the frame-time proxy measure the throttle (see the note under the table). Rendering, `--screenshot`, `hud_click` and the frame-rate check work the same as in the foreground, as long as part of the window stays visible: macOS presents nothing for a fully covered window, so a covered corner means no screenshot and meaningless frame times (the game logs `background: the window is fully covered at <s> s`, and `frame_stats` reports `occluded=<n>/<frames>`). Measured on the dev Mac on 2026-10-07 with `osascript -e 'tell application "System Events" to get name of first application process whose frontmost is true'` polled every 0.6 s before, during and after a `--background --exit-after-seconds 6` run: the frontmost app stayed the same (Claude) at every poll; the game window held focus for about 20 to 30 ms at launch (`frame_stats` events `focused@0.00s,unfocused@0.02s`) before the hand-back. The `hud_click` check also leaves the mouse pointer alone now: it used to move the window cursor with `Window::set_cursor_position`, which bevy_winit turns into an OS pointer warp, so every run left the owner's pointer on the Stop button; it now overrides the window cursor for one frame at a time and restores it before bevy_winit looks (`hud::RealCursor`; `NSEvent.mouseLocation` polled every 50 ms through a run did not move). Manual playtests (this checklist, the Cmd-Q check, the screenshots for the PR when taken by hand) do not use background mode.

| Proxy | Command | Covers | Pass when |
|---|---|---|---|
| Synthetic Cmd-Q | `cargo run -p game --features dev -- --quit-via-menu-after-seconds 3 --exit-after-seconds 30 --replay-dir /tmp/eonmark-replays` then `cargo run -p sim-cli --release -- verify /tmp/eonmark-replays/<newest>.eonreplay` | the Cmd-Q row except the key press itself: after 3 s the game builds a `MenuEvent` with the `quit` id and hands it to the same handler the real menu item reaches (muda's event channel is `pub(crate)`, so the event cannot be injected one step earlier); `AppExit`, recorder finish, trailer and exit code are the real ones. `--exit-after-seconds 30` is only a safety net and must not fire. | stdout has `quit-via-menu: firing "quit" after 3.0x s`, the process exits 0 well before 30 s (4 s wall on the dev Mac), `tail -c 8 <file>` is `EONRDONE`, `verify` prints `OK` with no truncation warning (the `has AI slots` warning is expected for a skirmish) |
| Red close button | `cargo run -p game --features dev -- --close-window-after-seconds 3 --exit-after-seconds 30 --replay-dir /tmp/eonmark-replays`, then `verify` the newest file | everything after the click: the flag writes `WindowCloseRequested` for the primary window, which is what winit sends for the button; `bevy_window` despawns the window and `exit_on_all_closed` writes `AppExit::Success` in `Last`, then the recorder finishes. This proxy found a real bug on 2026-10-06: the recorder system ran before `exit_on_all_closed` in `Last` and the winit runner exits without another frame, so the close button left the replay without its trailer (exit code still 0). Fixed by ordering `finish_recorder_on_exit` after `bevy::window::ExitSystems` | stdout has `close-window: requesting close after 3.0x s` and `No windows are open, exiting`, exit 0 within a few seconds, `tail -c 8` is `EONRDONE`, `verify` OK with no truncation warning |
| `kill -9` | `scripts/m2_checks.sh` step 4, or by hand: start windowed with `--replay-dir`, `kill -9` the pid after ~5 s, `verify` the file | truncated-recording tolerance of the reader and the 20-tick flush cadence | `warning:` on stderr naming the dropped tail, then `OK`; the lost span is under 20 ticks |
| Frame-rate independence | `--scenario scripted_moves --max-fps 30 --exit-after-seconds 35 --replay-dir A` and the same with `--max-fps 120 --replay-dir B`; compare the `final_hash` of both `verify` lines (or `hash-dump --every 20` up to tick 600) | the sim never sees the frame rate: identical hashes at 30 and 120 fps (`scripts/m2_checks.sh` step 2) | both `OK` lines carry the same `final_hash` |
| HUD click isolation | `cargo run -p game --features dev -- --scenario hud_click --exit-after-seconds 10` (add `--exit-after-seconds 3 --screenshot <file>.png` for the picture: the check holds its verdict until the PNG has landed) | the two HUD rows: first a positive control (a synthetic right click on open ground at the window centre must queue exactly one Move, so the "no Move" assertions below cannot pass vacuously), then synthetic `PointerInput` press and release over the bottom bar, then over the Stop button; no Move queued, selection unchanged | stdout ends with `hud_click: ok`, exit 0 (`hud_click: FAIL <why>` and exit 1 otherwise) |
| 200 units, frame time | `cargo run -p game --features dev -- --background --scenario units200_auto --exit-after-seconds 70 --screenshot <file>.png` | the automated half of the first acceptance line: 200 Yeomen ordered east during tick 20, `frame_stats:` lines (mean, p95, max frame ms, the three worst frames with their time, focused and covered frame counts, dropped ticks) on exit, a screenshot for the PR | p95 under 16.7 ms, `occluded=0`, dropped ticks 0, the screenshot shows the group under way (`scripts/m2_checks.sh` step 6 gates the first two) |
| Display sleep | `cargo run -p game --features dev -- --background --scenario units200_auto --exit-after-seconds 40 --replay-dir /tmp/eonmark-replays`, 8 s in run `pmset displaysleepnow` in another terminal (it sleeps the display; ask first if someone is using the Mac), wake it before the deadline, then `verify` the newest file | the `monitor_loss` fix: macOS stops listing the sleeping display, `bevy_winit` despawns its `Monitor`, and Bevy 0.19.1 would despawn the window with it (`HasWindows` is `linked_spawn`) and exit | the log shows `Monitor removed <entity>` then `monitor <entity> removed; keeping window <entity> open`, never `No windows are open, exiting`; `exit-after-seconds reached` at 40 s; then `OK` |
| Headless replay | `cargo run -p game --profile ci -- --headless-run crates/sim/tests/fixtures/move_500.eonreplay` | the driver's replay source and the CI hash-parity line | last stdout line `tick=1200 hash=0x...`, exit 0, under 10 s |

Reading frame times. Two things outside the game's own frame cost showed up in the M2 measurements; both were measured on 2026-10-07 in background mode on the dev Mac (dev build, 120 Hz display):

- An unfocused window is throttled to 60 updates a second. Bevy's default `WinitSettings::game()` runs `Continuous` while the window has focus and `reactive_low_power(1/60 s)` without it, so a window you clicked away from settles at mean 16.7 ms, p95 17.6 ms, whatever it draws: `units200_auto` with that default and the window unfocused gave mean 16.67 ms, p95 17.61 ms, 60.0 fps, the exact numbers of the 2026-10-06 acceptance runs. The same scene with `continuous()` and vsync off (`--max-fps 1000`) costs mean 3.65 ms, p95 6.73 ms (274 fps), and the render thread, not game code, is most of it (`sample` profile: the game's own systems are under 1 % of samples). Read the FPS overlay with the game window focused; the background mode sets `continuous()` for this reason.
- A one-off frame of about one second when the window becomes fully covered (another app's window, Mission Control, hiding the window). AppKit updates `NSWindow.occlusionState` a few milliseconds after the window is covered; wgpu-hal 29.0.4 checks that state before `-[CAMetalLayer nextDrawable]` to skip covered windows, so a frame that acquires inside that gap waits in `nextDrawable` until CoreAnimation gives up, 1 s later. Reproduced by hiding the window repeatedly (2 of 20 hides in one run gave 1018.70 and 1015.23 ms frames, each 1 s after the hide; no stall on any of the 29 un-hides); the `sample` stack during the stall is `bevy_render::view::window::prepare_windows -> wgpu Surface::get_current_texture -> wgpu_hal::metal acquire_texture -> -[CAMetalLayer nextDrawable] -> _dispatch_semaphore_wait_slow`, with the main thread parked waiting for the render thread. The window is covered when it happens, so nothing visible freezes; the 250 ms `max_delta` clamp turns it into about 15 skipped ticks (deterministic: the replay simply has fewer ticks). It is the `max=1003..1016` ms frame of the earlier acceptance runs, whose window came to the front at launch and was covered by the owner's next click. Not a pipeline compile: the first selection ring spawns at frame 15 (0.26 s, `RUST_LOG=info,game=debug` prints it) and the longest frame there is the startup frame 3 (about 100 ms). The race is in wgpu-hal; the game cannot close it without patching wgpu.

Rows marked `owner` in the checklist (trackpad, pinch, wheel, pan keys, edge scroll, drag box, double-click, shift-click, resize, and the by-eye camera distance) have no proxy: the screenshot from `units200_auto` and the `scripted_moves` hash equality stand in for them in the PR until the owner signs the table.

## Screenshots (M2)

Captured on 2026-10-06 from the M2 integration build with `--screenshot` (dev build, 1280x800 logical window on a Retina display, downscaled to 1600 px wide for the repository). They are the `ci_testing`-style owner proxies the ROADMAP asks for: the selection state and the HUD click check. Regenerate them with the commands in the table whenever the look changes; the first run after a build spends about three seconds compiling shaders, so the tick in the Sim panel is lower on a cold shader cache.

| File | Command | What to look for |
|---|---|---|
| [screenshots/m2-units200.png](screenshots/m2-units200.png) | `cargo run -p game --features dev -- --scenario units200 --exit-after-seconds 6 --screenshot m2-units200.png` | 200 matte slate-blue capsules at the west start, nothing selected; dark top bar, bottom bar with the Stop button; no glow, no bloom |
| [screenshots/m2-selection.png](screenshots/m2-selection.png) | `cargo run -p game --features dev -- --scenario units200_auto --exit-after-seconds 6 --screenshot m2-selection.png` | `selection: 200` on stdout; every unit carries a thin flat ring on the ground (separate entities, see the inspector); the group is under way east after the tick-20 scripted Move |
| [screenshots/m2-hud.png](screenshots/m2-hud.png) | `cargo run -p game --features dev -- --scenario hud_click --exit-after-seconds 3 --screenshot m2-hud.png` | `hud_click: ok` on stdout; 20 units still selected after the synthetic clicks on the bottom bar and on the Stop button; the HUD bars anchored top and bottom |

## Match log (M6)

Play against the Standard bot from New Game to the game-over screen. One row per match. Keep the replay file; its name is the seed and timestamp.

| Date | Commit | Seed | Difficulty | Duration (min) | Outcome | AI reached my borders | Town below 50% HP | Cap readout amber | Fun (1-5) | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| | | | Standard | | decisive / tiebreak | y / n | y / n | y / n | | |
| | | | Standard | | decisive / tiebreak | y / n | y / n | y / n | | |
| | | | Standard | | decisive / tiebreak | y / n | y / n | y / n | | |

Column meanings:

| Column | Meaning |
|---|---|
| Outcome | `decisive` if the match ended by capital capture or all Towns lost; `tiebreak` if the 45-minute territory tiebreak ended it. Add `win` or `loss`. |
| AI reached my borders | The bot's army crossed into tiles I owned at least once |
| Town below 50% HP | Any of my Towns dropped below half hit points, or was annexed |
| Cap readout amber | The Yield Cap indicator in the resource bar turned amber at least once |
| Fun | Owner's rating, 1 to 5, honest |
| Notes | What was tense, what was dull, which number felt wrong and in which RON file |

## Fun gate thresholds (M6)

M6 cannot close until all of these hold. Bot-side numbers come from `sim-cli play-bots`; human-side numbers come from the log above.

| Measure | Threshold |
|---|---|
| Seeded Standard-vs-Standard bot games (distinct personalities) | 20 of 20 end with a declared winner; territory-tiebreak wins are valid |
| Decisive (capital capture) before tick 48000 (40 min) | at least 10 of 20 |
| Median bot game length | at most 45 min |
| Difficulty ordering (Hard beats Standard beats Easy) | at least 15 of 20 |
| Logged human-vs-Standard matches | 3, each 20 to 40 min |
| AI army reaches the player's borders and annexes or reduces a Town below 50% HP | in at least 2 of 3 |
| Yield Cap readout turns amber | at least once per match |
| Owner fun rating | at least 3 of 5 |

Two to three tuning iterations of `data/rules/rules.ron` are budgeted. The late-game pressure knobs (Charter Age Harrying bonus, optional Yield Cap decay after minute 30) exist for this purpose. Record each iteration as a new set of three rows above with the commit that changed the numbers.
