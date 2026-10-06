//! `step_500_units`: step time while 500 Yeomen cross the plains map
//! (`sim::scenarios::move_500`, the same stream `sim-cli record` writes and
//! the `move_500` fixture verifies). Target (docs/design/pathing.md): mean
//! < 5 ms, p95 < 10 ms per step in release on the dev Mac.
//!
//! Two measurements:
//!
//! - `crossing_1200_steps`: one full 1200-tick run per iteration, with a
//!   throughput of 1200 elements, so criterion's `thrpt` line is steps per
//!   second and the mean per step is the reported time divided by 1200.
//! - `first_20_after_move`: the 20 ticks right after the Move order lands,
//!   when 500 path requests saturate the per-tick budget and the spawn
//!   cluster is at its densest; this is the tail the p95 target is about.
//!
//! The per-step p95 itself is reported by `sim-cli bench --units 500 --ticks
//! 1200`, which may use `std::time::Instant` (banned in this crate).
//! Owner: implementer B (movement) with A (pathing).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sim::scenarios::{self, Stream};
use sim::{MatchSetup, Rules, Sim};
use std::hint::black_box;
use std::path::Path;

fn rules() -> Rules {
    Rules::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).expect("data/ loads")
}

/// The `move_500` scenario run to `ticks` (0 = a fresh sim).
fn crossing(rules: &Rules, setup: &MatchSetup, stream: &Stream, ticks: u32) -> Sim {
    scenarios::run(setup.clone(), rules.clone(), stream, ticks)
}

fn bench_step(c: &mut Criterion) {
    let rules = rules();
    let (setup, stream) = scenarios::move_500(&rules);
    let ticks = scenarios::MOVE_500_TICKS;

    let mut group = c.benchmark_group("step_500_units");
    group.sample_size(10);

    group.throughput(Throughput::Elements(u64::from(ticks)));
    group.bench_function("crossing_1200_steps", |b| {
        b.iter_batched(
            || crossing(&rules, &setup, &stream, 0),
            |mut sim| {
                for t in 0..ticks {
                    sim.step(scenarios::commands_at(&stream, t));
                }
                black_box(sim.hash())
            },
            criterion::BatchSize::LargeInput,
        );
    });

    // The Move issued during tick 5 applies at tick 5 + cmd_delay; measure
    // the 20 steps starting there.
    let move_applied = 5 + setup.cmd_delay;
    group.throughput(Throughput::Elements(20));
    group.bench_function("first_20_after_move", |b| {
        b.iter_batched(
            || crossing(&rules, &setup, &stream, move_applied),
            |mut sim| {
                for t in move_applied..move_applied + 20 {
                    sim.step(scenarios::commands_at(&stream, t));
                }
                black_box(sim.hash())
            },
            criterion::BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(benches, bench_step);
criterion_main!(benches);
