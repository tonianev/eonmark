# data/rules/

This directory is where every gameplay number lives. `rules.ron` holds the
match-wide values, `resources.ron` describes the four resources and
`units.ron` lists the unit kinds. All are
loaded by `rules::Rules::load` and validated before the simulation starts;
their content is part of `rules_hash`, so editing them without bumping
`rules_version` makes `sim-cli verify` report `RULES CHANGED since recording`
on older replays. Changing a number here needs no Rust.

## Authoring convention: integers in deciseconds and tiles

The simulation runs at a fixed tick rate (`tick_rate_hz`, 20). Rules are
not authored in ticks because a tick rate change would then rewrite every
file. They are not authored as decimal seconds because the data crate
contains no floating point. The convention is:

| Quantity | Unit | Field suffix | Example |
|----------|------|--------------|---------|
| Time | deciseconds (tenths of a second) | `_ds` | `attrition_interval_ds: 32` is 3.2 s |
| Distance, radius | tiles | `_tiles` | `supply_radius_tiles: 14` |
| Counts, caps, versions | plain integers | none | `pop_cap_base: 25` |
| Tick-native values | ticks | `_ticks` | `cmd_delay_ticks: 2` (only this one) |
| Fractional tiles or rates | hundredths | `_x100` | `speed_tiles_per_s_x100: 180` is 1.8 tiles/s |

The single conversion lives in `Rules::ticks_from_ds`:

```text
ticks = round(ds * tick_rate_hz / 10)
```

At 20 Hz: 32 ds = 64 ticks, 600 ds = 1200 ticks, 1 ds = 2 ticks. Rounding is
to nearest with halves rounding up. Pick values whose tick count is whole
when you can; the validator does not require it.

## rules.ron

| Field | Meaning | Value | Used from |
|-------|---------|-------|-----------|
| `rules_version` | Bump whenever golden replays are regenerated | 3 | M0 |
| `tick_rate_hz` | Simulation ticks per second | 20 | M0 |
| `cmd_delay_ticks` | A command issued at tick N applies at N + this | 2 | M0 |
| `default_map` | Map for `MatchSetup::skirmish`; must exist under `data/maps/` | `"plains_1v1"` | M0 |
| `yield_cap_table` | Income ceiling per resource per 30 s, by Trade level; strictly increasing | `[70, 100, 150, 200]` | M3a |
| `pop_cap_base` | Population cap before Arms research | 25 | M3a |
| `pop_cap_per_arms_level` | Added per Arms level | 25 | M4a |
| `town_radius_tiles` | Border radius of a new Town | 20 | M3a |
| `town_radius_grown_tiles` | Radius once the Town has enough distinct building kinds | 24 | M3a |
| `town_growth_distinct_buildings` | Distinct kinds needed to grow | 5 | M3a |
| `town_limit_base` | Towns allowed before Statecraft research | 1 | M3a |
| `supply_radius_tiles` | A Supply Wain cancels attrition within this radius | 14 | M5a |
| `attrition_interval_ds` | Deciseconds between 1 HP attrition pulses at Harrying I (32 = 3.2 s = 64 ticks) | 32 | M5a |
| `annexation_ds` | Uncontested time before a Town at 0 HP flips | 600 | M5a |
| `vision.unit_tiles`, `vision.building_tiles`, `vision.town_tiles` | Placeholder vision radii | 8, 10, 14 | M3a, M7 |
| `path_budget_expansions` | A* expansions shared per tick | 4000 | M1 |
| `path_cache_entries` | Bounded path cache size | 1024 | M1 |
| `repath_interval_ds` | Deciseconds between periodic repaths while a unit is moving (30 = 3 s = 60 ticks) | 30 | M1 |
| `separation_push_moving_div` | The separation push on a moving unit is capped at `speed / this` so steering keeps the upper hand | 2 | M1 |
| `separation_push_idle_div` | The separation push on a unit without an order is capped at `speed / this`: parked units yield slowly to a passing crowd instead of jamming it | 8 | M1 |

Validation: every count except `pop_cap_per_arms_level` and `rules_version` is > 0 (tightening those two is a listed good first issue), `yield_cap_table` is non-empty and strictly
increasing, `town_radius_grown_tiles >= town_radius_tiles`, `default_map`
resolves to a file, the three movement fields are > 0. Fields the design names for later milestones (ramping
parameters, Harrying doubling, age-gap table, late-game pressure knobs,
the 45-minute cap) are added when those milestones land, each with a
validator rule.

## resources.ron

A list of resources in display order. Each entry:

| Field | Meaning |
|-------|---------|
| `id` | Lowercase identifier other files use (`grain`, `lumber`, `ore`, `lore`) |
| `name` | Display name |
| `gathered_from` | Building ids that produce it; cross-checked against `buildings.ron` from M3a |
| `role` | One sentence on what it pays for |
| `yield_capped` | `true` if income flatlines at `yield_cap_table` |
| `stockpile_cap` | `Some(n)` hard ceiling; required when `yield_capped` is `false` |

Lore is the only uncapped resource; it has `stockpile_cap: Some(999)` and is
not tradeable.

## units.ron

A list of unit kinds; the list index is the sim's `UnitKindId`. M1 ships
one kind, `yeoman` (index 0). Combat stats, costs and trainer buildings are
added in M3a / M4a, each with a validator rule. How the movement fields are
used is specified in `docs/design/pathing.md` ("Movement step"): speed is
converted once at spawn to tiles per tick (`x100 / (100 * tick_rate_hz)`),
the radius to fixed-point tiles; the arrive, separation and return-to-post
radii are read per tick.

| Field | Meaning | Yeoman |
|-------|---------|--------|
| `id` | Lowercase identifier other files use | `"yeoman"` |
| `name` | Display name | `"Yeoman"` |
| `speed_tiles_per_s_x100` | Movement speed in tiles per second, times 100; converted once to tiles per tick at spawn | 180 (1.8 tiles/s, 0.09 tiles/tick at 20 Hz) |
| `radius_tiles_x100` | Collision radius in tiles, times 100 | 35 |
| `arrive_radius_tiles_x100` | The unit has arrived (order cleared, `UnitArrived` emitted) within this radius of its final waypoint | 25 |
| `arrive_slowdown_radius_tiles_x100` | Arrival steering slows the unit linearly inside this radius of its final waypoint; at least the arrive radius | 50 |
| `waypoint_radius_tiles_x100` | A non-final waypoint counts as reached within this radius (50 = half a tile) | 50 |
| `separation_tiles_x100` | Margin added to the sum of two radii before separation pushes neighbours apart | 10 |
| `return_to_post_radius_tiles_x100` | An arrived unit pushed farther than this from the tile centre it arrived at (its post) walks straight back to it and is parked again once within the arrive radius; at least the arrive radius; `100` (one tile) when the field is absent | 100 |

Validation: at least one kind, ids unique and lowercase, `speed_tiles_per_s_x100 > 0`,
`radius_tiles_x100 > 0`, `waypoint_radius_tiles_x100 > 0`,
`arrive_slowdown_radius_tiles_x100 >= arrive_radius_tiles_x100`,
`return_to_post_radius_tiles_x100 > 0` and `>= arrive_radius_tiles_x100`, every `_x100`
field at most 100000. Errors name the file and `units[i].<field>`.

## Worked example: slower attrition

To make attrition tick every 4 seconds instead of 3.2:

```text
attrition_interval_ds: 40,
```

Then bump `rules_version` (replays recorded under the old value will report
`RULES CHANGED since recording` from M1) and run:

```bash
cargo run -p sim-cli -- data-check data
```

Expected output starts with `OK rules_version=4` (the repository is at 3). A typo such as
`attrition_interval_s` fails with the file path, the span and the field:

```text
error: data/rules/rules.ron: 24:3-24:23: Unexpected field named `attrition_interval_s` in `Rules`, expected one of `rules_version`, ... instead
```
