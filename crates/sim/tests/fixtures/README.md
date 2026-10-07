# Golden replay fixtures

Each fixture is `<name>.eonreplay` (written by `sim-cli record --scenario <name>`)
with a sibling `<name>.hash` holding the final state hash as `0x<16 hex>` and a
newline. `crates/sim/tests/m1.rs::golden(name)` verifies the replay and compares
the final hash; `sim-cli verify` does the same from the command line.

| Fixture | Ticks | Scenario (`sim::scenarios`) | Runs in |
| --- | --- | --- | --- |
| `move_500_short` | 300 | the first 300 ticks of `move_500` (seed 42) | `cargo test -p sim` |
| `move_500` | 1200 | 500 Yeomen west to east, Stop for every third id at tick 400 (seed 42) | `sim-cli verify --release` in CI on both OSes (hash-parity job); `cargo test -- --ignored` locally |
| `group_spiral` | 600 | 64 Yeomen to the west edge, then onto a mountain tile (seed 7) | `cargo test -p sim`, `sim-cli verify --release` in CI |
| `snapshot_restore` | 1200 | 200 Yeomen east, 50 more spawned for player 1 after a snapshot/restore swap at tick 300 (seed 7) | `cargo test -p sim`, `sim-cli verify --release` in CI |

The `snapshot_restore` recording snapshots the sim before tick 300, restores
the bytes into a fresh sim and continues recording on that sim; `verify`
re-simulates without the swap, so an `OK` proves the restore was exact.

Recorded 2026-10-06 at `rules_version` 4, `SIM_VERSION` 4 (M2: `visuals.ron`
joined `rules_hash`, and `Command::AttackMove` is applied like `Move` instead
of being rejected; no fixture contains an `AttackMove`, so every final hash is
unchanged from 3/3 and only the headers differ). Before that, the same day,
3/3 (return to post:
an arrived unit keeps a `Post` and walks back to it when pushed more than
`return_to_post_radius_tiles_x100` away; `Unit.post` joined the serialised
state, so every hash changed, including `move_500_short`, whose 300 ticks see
no arrival). Earlier recordings the same day: 1/1 (first creation) and 2/2
(movement constants moved into `rules.ron` and `units.ron`, replay clean-exit
trailer, `Pathing::service` no longer parks a zero-expansion search; final
hashes unchanged from 1/1). Command:
`cargo run -p sim-cli --release -- record --scenario <name> --out crates/sim/tests/fixtures/<name>.eonreplay`,
then the printed `final_hash` goes into `<name>.hash`.

Regeneration policy (docs/DETERMINISM.md, "Golden fixtures"): fixtures change
only in a commit that bumps `rules_version` in `data/rules/rules.ron` with a
one-line reason. A changed hash without that bump is a bug. CI runs
`scripts/check_fixture_policy.sh` against the PR base to enforce it.
