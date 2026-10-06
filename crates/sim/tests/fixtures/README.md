# Golden replay fixtures

Each fixture is `<name>.eonreplay` (written by `sim-cli record --scenario <name>`)
with a sibling `<name>.hash` holding the final state hash as `0x<16 hex>` and a
newline. `crates/sim/tests/m1.rs::golden(name)` verifies the replay and compares
the final hash; `sim-cli verify` does the same from the command line.

| Fixture | Ticks | Runs in |
| --- | --- | --- |
| `move_500_short` | <= 5000 | `cargo test -p sim` |
| `move_500` | 1200 (full bench scenario) | `sim-cli verify --release` in CI (hash-parity job) |
| `group_spiral` | short | `cargo test -p sim` |
| `snapshot_restore` | short | `cargo test -p sim` |

Regeneration policy (docs/DETERMINISM.md, "Golden fixtures"): fixtures change
only in a commit that bumps `rules_version` in `data/rules/rules.ron` with a
one-line reason. A changed hash without that bump is a bug.
