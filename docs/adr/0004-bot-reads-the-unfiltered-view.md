# 0004: The v0.1 bot reads the unfiltered view; fog of war is the human's

Status: Accepted
Date: 2026-11-08

This record decides what the scripted bot knows once M7 adds fog of war. It is the owner's decision of 2026-11-08, taken when the execution plan for M6 to M9 was written ([PLAN.md](../PLAN.md), decision D2). It changes one clause of M7's deliverables, so it goes through this record and the ROADMAP's M7 Notes.

## Context

- **The design.** [fog.md](../design/fog.md) specifies a hashed per-player visibility grid in the sim, a `SimView` that filters other players' units and buildings by the viewer's grid, and a bot that reads only that filtered view ("What the AI is allowed to know"). ROADMAP M7's deliverable says `SimView` "exposes it so the AI respects fog". Its first acceptance line asks for a sim test in which "an enemy unit outside all vision cells is hidden in `SimView`", and for every earlier golden fixture to verify after the milestone's single `rules_version` bump.
- **The bot depends on what fog would hide.** Since M6 the Standard bot launches a siege wave only at 0.8 times the enemy army at home (`pressure.launch_ratio_permille` 800 in `data/ai/difficulty/standard.ron`), Hard assaults only at 1.3 times it, and the counters, retreat, lure and influence maps all read enemy units ([ai.md](../design/ai.md), "Difficulty table (as retuned)"). Under fog the enemy army at home is unseen until a wave arrives, so these gates would open blind. The M6 retune measured that the launch gate alone decides ROADMAP M6 line 2's first clause (without it Standard beats Easy 11 of 40). A fog-obeying bot therefore needs a last-seen memory in its hashed `ai_memory` and a re-tune of M6 lines 1, 2 and 4, which CI's `fun-gate` job checks on every pull request.
- **Cost.** The rest of the roadmap runs on cost-efficient agent sessions ([PLAN.md](../PLAN.md)). The fog-obeying bot is estimated at 2 to 4 extra sessions, mostly bounded tuning, with a real risk of a blocked M7.
- **Precedent.** The bot's income bonus is a disclosed advantage (M6 owner decision 2), stated on New Game and on the game-over screen.

## Decision

1. M7 ships the visibility grid as fog.md designs it: hashed sim state, stamped in `Sim::step`, with its own sub-hash, serialised in the snapshot, and `SimView` queries that filter by it (`visibility`, `tile_state`, `units_visible_to`, `buildings_known_to`). M7's first acceptance line tests those queries.
2. The bot keeps reading the unfiltered view. The seam (`visible_enemies` in `crates/ai/src/standard/facts.rs`) and the building reads stay as they are; only their doc comments change, citing this record. No bot decision changes in M7.
3. Fog is drawn for the human player only: the ground, enemy units and buildings, and the minimap. Spectating (`--spectate`) shows the whole map.
4. The advantage is disclosed beside the bonus: New Game's difficulty row and the game-over screen say the bot sees the whole map.
5. A fog-obeying bot becomes a v0.2 candidate (ROADMAP, "v0.2 candidates (M9)"), re-entered with its own ADR after v0.1.0 ships.

## Alternatives considered

| Alternative | Why not |
|---|---|
| The bot obeys fog in M7 (fog.md as written) | It needs a last-seen memory for the launch and assault gates, the counters and retreat, then a re-tune of the CI-gated M6 lines 1, 2 and 4 under fog: 2 to 4 more sessions, and M7 can stall on the fun gate. The human-facing value of fog (hidden armies, surprise) does not depend on it. |
| Only Hard sees the whole map | Easy and Standard would still need the fog-obeying bot, with its memory and its re-tune, so nothing is saved. |
| No sim grid: fog computed in the game from the view, unhashed | M7's first line asks for a sim test and the single `rules_version` bump. Lockstep peers and a later fog-obeying bot need the grid in hashed state, and an explored history cannot be rebuilt from the current state, so it could not be a derived cache ([DETERMINISM.md](../DETERMINISM.md), "Derived caches"). |

## Consequences

- M7's sim change alters every state hash (the grid is hashed) and no game's outcome. With the `final_hash` column removed, the play-bots CSVs of M6 lines 1 and 2 are identical before and after the bump; that comparison is the proof the bump's pull request carries ([PLAYBOOK.md](../PLAYBOOK.md), P5).
- The M6 fun gate is untouched by fog. The human's experience changes, since they see less while the bot does not; the M7 owner sign-off and later streams judge it.
- fog.md's table "What the AI is allowed to know" stays as the design of the v0.2 bot. fog.md, ai.md and the ROADMAP's M7 Notes cite this record.
- The grid costs simulation time every tick. M7 measures it against the `cargo test -p sim` budget of 10 s and may stamp every few ticks (a data field in the same bump) instead of every tick.
