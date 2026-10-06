//! `astar_budget`: cost of one tick that spends the full expansion budget.
//! Target (docs/design/pathing.md): a saturated 4000-expansion tick < 2 ms
//! in release on the dev Mac. Owner: implementer A.

use criterion::{Criterion, criterion_group, criterion_main};
use sim::pathing::AStarSearch;
use sim::{Map, Rules, Tile};
use std::hint::black_box;
use std::path::Path;

fn plains() -> Map {
    let rules =
        Rules::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).expect("data/ loads");
    Map::from_def(rules.map("plains_1v1").expect("plains_1v1"))
}

fn bench_budget(c: &mut Criterion) {
    let map = plains();
    let budget = 4000u32;
    c.bench_function("astar_budget_4000", |b| {
        b.iter(|| {
            // A corner-to-corner search cannot finish inside one budget, so
            // every iteration spends exactly `budget` expansions.
            let mut search = AStarSearch::new(&map, Tile::new(0, 0), Tile::new(127, 127));
            let mut remaining = budget;
            let status = search.resume(&map, &mut remaining);
            black_box((status, remaining))
        });
    });
}

criterion_group!(benches, bench_budget);
criterion_main!(benches);
