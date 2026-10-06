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

Recorded 2026-10-06 at `rules_version` 2, `SIM_VERSION` 2 (first creation was
at 1/1 the same day; the bump moved the movement constants into `rules.ron` and
`units.ron`, added the replay clean-exit trailer and stopped `Pathing::service`
from parking a zero-expansion search; every final hash is unchanged from the
first recording, only the intermediate `pathing` sub-hashes of saturated ticks
and the header differ):
`cargo run -p sim-cli --release -- record --scenario <name> --out crates/sim/tests/fixtures/<name>.eonreplay`.

Regeneration policy (docs/DETERMINISM.md, "Golden fixtures"): fixtures change
only in a commit that bumps `rules_version` in `data/rules/rules.ron` with a
one-line reason. A changed hash without that bump is a bug. CI runs
`scripts/check_fixture_policy.sh` against the PR base to enforce it.
