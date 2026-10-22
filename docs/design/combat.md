# Combat

This document specifies combat: the eight unit kinds and their roles, the counter triangle, the Watchtower and Town shooters, deterministic target acquisition, the seeded fixtures the counter tests run on, and a summary of movement. Combat in Eonmark is meant to be readable: a small roster, hard counters, no random damage by default. Stat values (hit points, damage, range, speed, cooldown, cost, pop) are not fixed by the design; they are set in `data/rules/units.ron` and `data/rules/buildings.ron` at M4a and must satisfy the seeded counter tests below. For the design lineage see [prior-art.md](prior-art.md).

| Field | Value |
| --- | --- |
| Milestone | M4a (sim), M4b (health bars, unit looks, attack-move key) |
| Labels | `area:sim`, `area:rules`, `kind:balance` for multiplier tweaks |
| Rules files | `data/rules/units.ron` (8 kinds, classes, stats, trainer, counter table in permille), `data/rules/buildings.ron` (Watchtower and Town shooter stats, HP), `data/rules/rules.ron` (`damage_variance_permille`, acquisition settings) |
| Sim modules | `crates/sim/src/combat.rs`, `crates/sim/src/movement.rs` |
| Sub-hash | `units`, `buildings`, `rng` |

## Unit kinds

| Kind | Class | Age | Trainer | Role | Costs mainly | Pop |
| --- | --- | --- | --- | --- | --- | --- |
| Yeoman | civilian | Hearth | Town | Gathers Grain, Lumber, Ore; builds everything | 20 Grain, +1 per Yeoman | `units.ron` |
| Scribe | civilian | Hearth | Scriptorium | Gathers Lore (+5 per 30 s each, 3 per Scriptorium); immune to attrition | 30 Grain | 1 |
| Skirmisher | infantry | Hearth | Muster Hall | Light infantry; beats Bowmen | Grain | `units.ron` |
| Shieldbearer | infantry | Hearth | Muster Hall | Heavy infantry; beats Outriders | Ore | `units.ron` |
| Bowman | infantry (ranged) | Hearth | Muster Hall | Ranged; beats Shieldbearers | Lumber | `units.ron` |
| Outrider | cavalry | Masonry | Muster Hall | Fast raider; beats Bowmen, Skirmishers, Mangonels | Ore | `units.ron` |
| Mangonel | siege | Masonry | Engine Yard | Beats buildings; brings Towns to 0 HP | Lumber | `units.ron` |
| Supply Wain | support | Masonry | Engine Yard | No attack; cancels attrition within 14 tiles; immune to attrition | Lumber | `units.ron` |

Classes drive other systems: infantry and cavalry adjacent to a Town at 0 HP start annexation, and every non-civilian kind counts as "defender military" (see [towns.md](towns.md)). The Scribe's trainer is the Scriptorium in the M3a data; the Outrider's is M4a starting data and an open question. The Skirmisher is already in `units.ron` from M3a by owner decision (index 2, infantry, 40 Grain, `ramp: Military`, trained at the Muster Hall; the M3a military ramp test queues Skirmishers). It moved at Yeoman speed in M3a; M4a gives it its own speed (`speed_tiles_per_s_x100: 220`, against the Yeoman's 180) and its combat stats. The 8 kinds (Yeoman, Scribe and six military kinds) and 8 building kinds are counted by `sim-cli data-check`.

## Counter triangle

Damage from attacker to defender is scaled by a permille multiplier from the counter table in `units.ron`. Entries not listed are 1000 (x1.0). The design fixes which pairs are counters, not the multiplier values.

| Attacker | Bonus against |
| --- | --- |
| Skirmisher | Bowman |
| Bowman | Shieldbearer |
| Shieldbearer | Outrider |
| Outrider | Bowman, Skirmisher, Mangonel |
| Mangonel | buildings |

The multiplier values must make each 10 vs 10 matchup below win at least 18 of 20 seeds. Proposing tweaks with a `cargo test -p sim counters` table is a good first issue.

## Damage

```text
raw      = attacker.damage (after Arms Mul 1100 per level, see tech-and-ages.md)
counter  = counter_table[attacker.kind][defender.kind]        // permille, default 1000
variance = 1000 + rng.range(-damage_variance_permille ..= damage_variance_permille)
damage   = floor(floor(raw * counter / 1000) * variance / 1000)
defender.hp -= damage
```

`damage_variance_permille` defaults to 0 in `rules.ron`, so combat is exactly reproducible from stats alone and stays readable. Setting it above 0 draws from the single `Pcg32` in the sim, which is hashed, so replays still verify. Hits land immediately in the sim; projectiles are render-side only. Cooldowns are authored in deciseconds (`_ds` fields) and converted once by `Rules::ticks_from_ds`.

## Shooters

Watchtowers and Towns attack on their own. Their range, damage and cooldown are in `buildings.ron`.

| Shooter | Notes |
| --- | --- |
| Watchtower | Outranges Skirmishers (acceptance test). Also a border source of radius 2 and the host of Harrying research. Destroyed at 0 HP. Costs Ore. |
| Town | Shoots back. Indestructible: at 0 HP it stays and becomes annexable (see [towns.md](towns.md)). |

## Buildings in combat

Every building has HP. A building at 0 HP is removed, except Towns. Removal frees its footprint in the cost grid, which bumps `cost_grid_generation` and clears the path cache (see [pathing.md](pathing.md)); a removed Watchtower is also removed from the territory field. Mangonels carry the building counter and are the intended answer to Watchtowers and Towns.

## Target acquisition

Units acquire targets deterministically.

```text
each tick, for each unit that is idle, attack-moving or holding a dead target (UnitId order):
    candidates = enemy units and buildings within acquisition range,
                 found through the spatial grid (2 x 2 tile cells)
    sort candidates by (dist_sq_i64, id)
    tied = candidates sharing the smallest dist_sq_i64
    target = if tied.len() == 1 { tied[0] } else { tied[rng.range(0..tied.len())] }
```

The seeded tie-break is the only randomness in default combat. A unit keeps its target until the target dies, leaves range or the unit receives a new order. `Attack { units, target }` chases a specific target; `AttackMove { units, target }` moves toward a point and engages anything acquired on the way, then resumes. `Stop` clears the target. The attack-move key is `A` in the game (M4b).

## Fixtures and the 20-seed tests

Counter tests spawn 10 vs 10 of two kinds on an open field with a seeded +/- 1 tile spawn jitter per unit. The seed perturbs jitter and acquisition tie-breaks, so results differ across seeds, and the test asserts that they do. Each matchup runs 20 seeds.

| Matchup | Expectation |
| --- | --- |
| 10 Skirmishers vs 10 Bowmen | Skirmishers win at least 18 of 20, with at least 4 survivors |
| 10 Bowmen vs 10 Shieldbearers | Bowmen win at least 18 of 20 |
| 10 Shieldbearers vs 10 Outriders | Shieldbearers win at least 18 of 20 |
| 10 Outriders vs 10 Bowmen | Outriders win at least 18 of 20 |
| Watchtower vs Skirmishers | the Watchtower outranges them |

## Performance budget

| Bench | Budget |
| --- | --- |
| `sim-cli bench --units 400 --ticks 1200 --combat` (200 vs 200 fighting) | mean step < 5 ms in release on the dev Mac |
| `--scenario enemy_outpost` with 200 vs 200 (M4b) | at least 60 fps; health bars are separate entities, never children of units |

Numbers are appended to `docs/BUILD_TIMES.md` when the bench passes.

## Movement summary

Movement is specified in [pathing.md](pathing.md); the parts combat depends on:

| Step | Rule |
| --- | --- |
| Pathing | Budgeted in-house A* on the 128 x 128 cost grid; repath every 60 ticks while moving or when the next waypoint becomes blocked. |
| Arrival steering | Fixed-point steering toward the next waypoint with slow-down at the goal; no trig, direction vectors only. |
| Separation | Push away from neighbours found in the 2 x 2 tile spatial grid, rebuilt each tick. |
| Circle correction | Circle-vs-circle overlap resolved in `UnitId` order so the result is independent of iteration luck. |
| Group orders | Square-spiral offsets assigned in `UnitId` order, skipping blocked tiles and tiles outside the click's connected component. |

Units without a path yet wait in place; nothing in combat depends on wall-clock time.

## M4a implementation notes

The combat core slice (`SIM_VERSION` 13, `rules_version` 15) implements hit points, the explicit `Attack`, simultaneous hits, death and building destruction in `crates/sim/src/combat.rs`, on the stats of the M4a rules data (`rules_version` 13). Target acquisition by idle and attack-moving units, a real `AttackMove` (applied like `Move` until then), the Watchtower and Town shooters, seeded spawn jitter and the 20-seed counter tests follow in the combat behaviour slice. Where it is more precise than the sections above:

| Topic | As implemented |
| --- | --- |
| State | `Unit` gains `damage_taken: i32`, `ready_at: u32` (the first tick it may hit again), `target: Option<TargetId>` and `intent: Intent` (`Plain`, `AttackMove { goal }` for the next slice, `Attack`), hashed in `units`; `Building` gains `ready_at` for the shooters of the next slice, hashed in `buildings`, and its `hp` (hashed since M3a) now takes damage. A unit's hit points are not stored: its maximum is resolved every tick from the kind's `hp` and the owner's `UnitHp` rows (at least 1), and it has `max - damage_taken`, so an Arms level raises a wounded unit's hit points as much as its maximum. Its damage is the attack's `damage` with the owner's `UnitDamage` rows (at least 0). `SimView::unit_stats(p, kind)` (hit points, damage, range, reach, cooldown) and `unit_hp(id)` (current and maximum) call the same functions as the step. |
| Tick order | Step 5 rebuilds the spatial grid, then 5a `combat::think` (every unit with an explicit `Attack`, in `UnitId` order, from the start-of-tick positions), 5b `movement::step` with the engagements 5a decided, 5c `combat::resolve` (hits, then the dead and the destroyed); see [ARCHITECTURE.md](../ARCHITECTURE.md). Nothing combat decides is kept across ticks except the fields above, so a restore has nothing new to rebuild. |
| Range metric | Edge to edge, compared in `dist_sq_i64`'s scale with no square root: a unit is in weapon range of a unit target when `dist_sq(a, t) <= (range + r_a + r_t)^2`, and of a building or site when the squared distance to the nearest point of its footprint rectangle, the tiles' area `[x0, x1 + 1] x [y0, y1 + 1]`, is at most `(range + r_a)^2`. With the starting data a melee attacker (range 0.25 tiles) and a target of radius 0.35 each are in range at 0.95 tiles centre to centre. |
| `Attack` | Check order, the first failure wins: (1) the target exists, is not the player's and, for a building, has hit points left, so a complete Town at 0 HP is not a target (`InvalidTarget`); (2) at least one listed unit is the player's and of a kind with an attack (`NoValidUnits`). An accepted `Attack` gives each such unit, in `UnitId` order, the target and `Intent::Attack` and drops its order, post, job and path request; the chase starts in step 5a of the same tick. `Move`, `AttackMove`, `Stop`, `Gather` and `Build` clear the target. An `Attack` draws nothing from the RNG, accepted or rejected (until this slice it was rejected with `NotImplemented` and drew once). `SimView::check_attack` makes the same checks after the player checks. |
| Chase and give-up | Each tick in 5a, from the start-of-tick positions. A target that is gone, the player's own or a building at 0 HP ends the order: no target, `Plain`, no order, no path request. In weapon range the unit holds: no desired velocity, the idle separation push cap, facing the target. Within its reach (the longer of `acquire_range_tiles`, 6, and its weapon range, edge to edge) and with a clear tile line (the tiles a straight segment between the two tile centres crosses are passable, a corner crossing needs both tiles beside the corner, and the target's own footprint does not count), or, for a building, from any tile beside its footprint (the nearest point of the footprint then lies on that tile's own edge or corner, so the straight steer never leaves the tile; a building's path chase ends on such a tile, and from a corner one the tile line can be blocked by a neighbour flush against the footprint), it chases directly: it steers straight at the target, or at the nearest point of the footprint, at full speed with the moving push cap, with no path and no request; a path-chase order is dropped with its request first, because step 4 writes a completed path into whatever order a unit has. Otherwise it walks a path, which ends on: the target's tile when the unit can walk there (the same connected component); else the tile of the unit's component nearest to the target (the tile pathing corrects the goal to, `Map::nearest_passable`) provided the target is in weapon range from anywhere within the unit's arrive radius of that tile's centre, so a Bowman walks to the near side of a wall or river and shoots across while a melee unit never hits a target walled off by a whole tile; for a building, the reachable ring tile nearest to the unit, as a builder's. With no such end tile the target cannot be reached and the order ends: at once, with no path, and on any later tick of the walk (a wall can close round the target), the cheap checks first (a target the unit can walk to, one it can hit from the end of its current path, a ring tile still in its component) and the search only when they fail. It is judged from the map, the two positions and the stats alone, so every attacker of a group gives up in the same tick; on a path each, a group would all walk to the one corrected goal tile, which not all of them can come within the arrive radius of, and the rest would jostle there for ever. With no order (and no post) the unit walks to the target's tile (a building's ring tile). With an order it re-goals to the target's tile once that is more than `chase_regoal_tiles` (2, Chebyshev) from the goal (to a new ring tile when a building's is cut off) and no request is outstanding. A re-goal is a repath: the unit keeps walking its current path until step 4 writes the new one, and while any request is outstanding it does not arrive at the end of its path but waits there (arriving would end the order, step 4 would drop the new path and the chase would start over from the back of the queue). An explicit `Attack` has no leash. A unit whose path chase has arrived (no order, a post) while its target is still neither in range nor directly chaseable walks again when the end tile is no longer the tile it arrived at (a target that stopped within `chase_regoal_tiles` of the goal round an obstacle, or walked on), and otherwise gives up; each such walk needs the target to have moved. A path step 4 finds unreachable (`PathUnreachable`) ends the order too. So an unreachable target costs no path, and one walled off during the chase no more than the path being walked, never a request every tick. |
| Hits | Step 5c collects, in `UnitId` order, every unit whose target is still valid and in weapon range from the post-movement positions and whose `ready_at` has come. Each hit does `floor(floor(raw * counter / 1000) * variance / 1000)` in `i64` (`raw` with the Arms rows; `counter` the listed row for the pair, `Buildings` for any building or site, else 1000; `variance` 1000 with no draw at `damage_variance_permille` 0, else one `rng_index(2v + 1)` draw per hit in collection order), sets the attacker's `ready_at` to the tick plus its cooldown and emits `Hit { attacker, target, damage }`. The hits are summed per target and applied together: they are simultaneous, so a unit killed this tick still lands its own and a lower id has no first strike. Overkill is accepted: no attacker skips a target already dealt enough this tick. |
| Death | Every unit whose `damage_taken` has reached its maximum is removed, in `UnitId` order, with its path request (`UnitDied { unit }`). The slot it worked, the site it built and the pop it used are derived, so they are free at once. |
| Destruction | Then every building or site at 0 HP or below other than a complete Town is removed, in `BuildingId` order (`BuildingDestroyed { building }`): no refund, its queue is lost (the pop it charged is free again, and a tech it was researching can be queued again), its builder loses its job and a complete Watchtower's border source leaves the territory field (one `TerritoryChanged`; see [territory.md](territory.md)). Sites of every kind are destroyed, Town sites included (owner decision 2026-10-21). Every footprint destroyed in a tick is unblocked by one `Map::set_blocked_many`, one cost-grid generation bump, so the path cache is cleared once and a suspended search is requeued on the new grid. Each Town a destroyed building was attributed to is then re-derived (`town::refresh`, in id order) and may fall back to tier 1. |
| Towns at 0 HP | A complete Town is clamped at 0 HP and stays, and is no longer a target (owner decision 2026-10-21): it keeps its footprint, border and queue, emits `TownAtZeroHp { town }` once, on the hit that brings it to 0, and its attackers stand down the next tick. It does not regenerate (the M4a plan's choice, see [towns.md](towns.md) open question 2). `Train` there is still accepted until M5a's annexation. Annexation needs infantry or cavalry next to such a Town; from the next slice an idle soldier acquires the nearest enemy, which may draw it away from the Town, and M5a has to allow for that (see [towns.md](towns.md)). |
| RNG | Combat draws only for damage variance (and, from the next slice, for exact ties when acquiring); at the shipped `damage_variance_permille: 0` a fight draws nothing. `rng_index(rng, n) = (next_u32 * n) >> 32`, which does not depend on the `rand` version. |

Tests (`crates/sim/tests/m4a.rs`, `cargo test -p sim combat`): `combat_damage_is_raw_times_counter_floored_every_cooldown` (27 for a Skirmisher at Arms I on a Bowman, 2 on a Croft, 13 for a Shieldbearer on a Yeoman, one hit per cooldown), `combat_hits_resolve_simultaneously_without_id_bias`, `combat_zero_variance_draws_no_randomness`, `combat_variance_is_seeded_and_reproducible` (variance 100 set in memory), `combat_attack_rejections_change_nothing` (every rejection against a control sim on every sub-hash, `rng` included, and `check_attack`), `combat_attack_chases_an_ordered_target` (direct and path chase, a re-goal keeps the old path), `combat_attack_paths_across_the_ford_when_the_line_is_blocked`, `combat_attack_on_an_unreachable_target_gives_up_without_a_request_storm` (no path at all), `combat_group_attack_on_an_unreachable_target_gives_up` (six attackers, walled off from the start and walled off while walking), `combat_ranged_attack_on_a_walled_off_target_shoots_over_the_wall`, `combat_attack_on_a_building_closes_in_from_a_corner_tile` (NE, SW, NW and SE corners beside a second building), `combat_attack_walks_round_a_corner_to_a_target_near_its_goal`, `combat_path_chase_keeps_walking_while_pathing_is_saturated` (a budget of 2 expansions a tick; `movement::tests::awaiting_units_hold_the_end_of_their_path_instead_of_arriving` covers the wait at the path's end), `combat_stop_clears_the_target`, `combat_watchtower_destroyed_frees_footprint_border_and_tier`, `combat_destroyed_building_loses_its_queue_and_frees_pop`, `combat_site_at_zero_hp_is_destroyed_town_sites_included`, `combat_town_at_zero_hp_stays_and_is_not_targeted`, `combat_dead_worker_frees_its_slot_and_path_request`, `combat_destruction_during_a_suspended_search_is_cache_transparent` (with path-cache keys that repeat within a generation and across the destruction, so the cache-on run really hits) and `combat_restore_mid_fight_matches_uninterrupted`. The Arms rows are covered by `tech_arms_raises_military_hp_and_damage_including_living_units` and `tech_every_arms_level_strictly_raises_every_military_stat` (`cargo test -p sim tech`), and `m4a_proptest_streams_are_deterministic` and `m4a_path_cache_is_transparent` run seeded random streams of spawns (at most 40 a side, `m4a_random_stream_spawns_at_most_40_a_side`), attacks, moves, stops and research with deaths and destruction.

## Interactions

| System | Interaction |
| --- | --- |
| [economy.md](economy.md) | Military ramp is triangular per training-building class; Ore gates Shieldbearers, Outriders and Watchtowers; pop cap bounds army size. |
| [tech-and-ages.md](tech-and-ages.md) | Arms gives +10% stats per level; Masonry unlocks Outrider, Mangonel, Supply Wain and the Engine Yard. |
| [attrition.md](attrition.md) | Unsupplied armies bleed inside enemy borders; Supply Wains and Scribes are immune. |
| [towns.md](towns.md) | Towns shoot and cannot be destroyed; infantry and cavalry annex them. |
| [territory.md](territory.md) | Watchtowers are border sources. |
| [ai.md](ai.md) | Influence maps (4 x 4 tile cells, every 10 ticks) and counter-weighted composition arrive in M6; retreat-when-losing. |
| [fog.md](fog.md) | From M7 acquisition respects the visibility grid through `SimView`. |
| [ui.md](ui.md) | Health bars, five primitive silhouettes, attack-move key, selection panel (M4b). |

## Acceptance checklist (from M4a)

- [ ] `cargo test -p sim counters` over 20 seeds each (seed perturbs spawn jitter and acquisition tie-breaks, so results differ across seeds and the test asserts they do): 10 Skirmishers vs 10 Bowmen -> Skirmishers win >= 18/20 with >= 4 survivors; 10 Bowmen vs 10 Shieldbearers -> Bowmen >= 18/20; 10 Shieldbearers vs 10 Outriders -> Shieldbearers >= 18/20; 10 Outriders vs 10 Bowmen -> Outriders >= 18/20; a Watchtower outranges Skirmishers.
- [ ] `cargo run -p sim-cli --release -- bench --units 400 --ticks 1200 --combat` keeps mean step < 5 ms with 200 vs 200 fighting.

Good first issue from M4a:

- [ ] Propose counter multiplier tweaks with a `cargo test -p sim counters` table (any OS).

```bash
cargo test -p sim counters
cargo run -p sim-cli --release -- bench --units 400 --ticks 1200 --combat
```

## Open tuning questions

1. Counter multiplier values: the design fixes the pairs and the 18/20 bar, not the numbers. Decided 2026-10-21 (owner accepted the M4a plan defaults): the starting table in `units.ron` is Skirmisher on Bowman 2500, Bowman on Shieldbearer 3000, Shieldbearer on Outrider 3000, Outrider on Bowman 2500, on Skirmisher 1500 and on Mangonel 3000, Mangonel on buildings 4000, and the Skirmisher, Shieldbearer, Bowman and Outrider on buildings 250 each. It is tuned in the M4a counters slice against the 18/20 bar.
2. Trainer for Outrider (Muster Hall or Engine Yard) and for Scribe (Scriptorium or Town). The M3a data trains the Scribe at the Scriptorium. Decided 2026-10-21 by the M4a plan (an implementation choice, not one of the nine owner decisions in ROADMAP.md; the owner may revisit): the Outrider trains at the Muster Hall, the Mangonel and the Supply Wain at the Engine Yard, and the Scribe stays at the Scriptorium.
3. Is Bowman an infantry class for annexation purposes? This doc says yes. Decided 2026-10-21 by the M4a plan (an implementation choice, not one of the nine owner decisions in ROADMAP.md; the owner may revisit): yes, the Bowman's class is `Infantry` in `units.ron`.
4. Acquisition range versus weapon range: equal in the starting data, or a slightly larger acquisition range so units close in. Decided 2026-10-21 (owner accepted the M4a plan defaults): one global `acquire_range_tiles: 6` in `rules.ron`. By the M4a plan, a unit's reach is the longer of that and its weapon range, so the Bowman (5 tiles) reaches 6 and the Mangonel (8) 8; the combat core uses the reach for direct chases, and the behaviour slice for acquisition.
5. Chase leash for `Attack` on a fleeing target. Decided 2026-10-21 by the M4a plan (an implementation choice, not one of the nine owner decisions in ROADMAP.md; the owner may revisit): no leash for an explicit `Attack`; the order ends only when the target dies, is destroyed or becomes invalid, on a new order, or when the target cannot be reached (see "M4a implementation notes"). Targets acquired in the behaviour slice are dropped beyond the reach.
6. Whether `damage_variance_permille` ever leaves 0 in a shipped ruleset. Decided 2026-10-21 by the M4a plan (an implementation choice, not one of the nine owner decisions in ROADMAP.md; the owner may revisit): it ships at 0, validated to 0..=500, so default combat draws nothing; a ruleset may raise it, and every hit then draws once from the sim's RNG.
