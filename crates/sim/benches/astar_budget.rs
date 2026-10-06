//! `astar_budget`: cost of one tick that spends the full expansion budget.
//! Target (docs/design/pathing.md): a saturated 4000-expansion tick < 2 ms
//! in release on the dev Mac. Owner: implementer A.
//!
//! The plains map is too open to saturate a 4000-expansion budget (a
//! corner-to-corner search finishes in a few hundred expansions), so the
//! worst case is built explicitly: a 128x128 serpentine maze whose only
//! route winds through every lane, and an open map whose goal is walled off
//! so the search must exhaust the whole component. Both benches call one
//! `AStarSearch::new` + `resume` with budget 4000 and assert beforehand that
//! the budget really is spent. Criterion's throughput is set to the budget,
//! so its `thrpt` line reads as expansions per second (divide by 1000 for
//! expansions per millisecond).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sim::pathing::{AStarSearch, SearchStatus};
use sim::{Map, Tile};
use std::hint::black_box;

const SIDE: u16 = 128;
const BUDGET: u32 = 4000;

/// Every odd row but the last is a wall with a one-tile gap at alternating
/// ends; the route from the north-west corner to the south-east corner
/// visits every lane (about 8k tiles).
fn serpentine() -> Map {
    let mut m = Map::open(SIDE, SIDE);
    for y in (1..SIDE - 1).step_by(2) {
        let gap = if (y / 2) % 2 == 0 { SIDE - 1 } else { 0 };
        for x in 0..SIDE {
            if x != gap {
                m.set_blocked(Tile::new(x, y), true);
            }
        }
    }
    m
}

/// An open map with the goal sealed inside a 3x3 wall: the search can only
/// exhaust the start's component (16k tiles), the true worst case.
fn walled_goal() -> (Map, Tile) {
    let mut m = Map::open(SIDE, SIDE);
    let goal = Tile::new(100, 100);
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx != 0 || dy != 0 {
                let t = m.offset(goal, dx, dy).expect("in bounds");
                m.set_blocked(t, true);
            }
        }
    }
    (m, goal)
}

fn saturated(map: &Map, from: Tile, to: Tile) -> (SearchStatus, u32) {
    let mut search = AStarSearch::new(map, from, to);
    let mut remaining = BUDGET;
    let status = search.resume(map, &mut remaining);
    (status, remaining)
}

fn bench_budget(c: &mut Criterion) {
    let maze = serpentine();
    let far = Tile::new(SIDE - 1, SIDE - 1);
    let (status, remaining) = saturated(&maze, Tile::new(0, 0), far);
    assert_eq!(status, SearchStatus::Suspended, "maze must saturate");
    assert_eq!(remaining, 0);

    let (open, goal) = walled_goal();
    let (status, remaining) = saturated(&open, Tile::new(0, 0), goal);
    assert_eq!(status, SearchStatus::Suspended, "walled goal must saturate");
    assert_eq!(remaining, 0);

    let mut group = c.benchmark_group("astar_budget");
    group.throughput(Throughput::Elements(u64::from(BUDGET)));
    group.bench_function("serpentine_4000", |b| {
        b.iter(|| black_box(saturated(black_box(&maze), Tile::new(0, 0), far)));
    });
    group.bench_function("walled_goal_4000", |b| {
        b.iter(|| black_box(saturated(black_box(&open), Tile::new(0, 0), goal)));
    });
    group.finish();
}

criterion_group!(benches, bench_budget);
criterion_main!(benches);
