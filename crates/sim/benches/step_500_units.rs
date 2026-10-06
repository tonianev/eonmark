//! `step_500_units`: mean step time while 500 Yeomen cross the plains map.
//! Target (docs/design/pathing.md): mean < 5 ms, p95 < 10 ms in release on
//! the dev Mac. Owner: implementer B (movement) with A (pathing).

use criterion::{Criterion, criterion_group, criterion_main};
use sim::{Command, FxVec2, MatchSetup, PlayerCommand, PlayerId, Rules, Sim, UnitKindId};
use std::hint::black_box;
use std::path::Path;

struct NoAi;

impl sim::AiController for NoAi {
    fn think(&mut self, _player: PlayerId, _view: &sim::SimView<'_>) -> Vec<Command> {
        Vec::new()
    }
}

fn rules() -> Rules {
    Rules::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).expect("data/ loads")
}

/// A scenario sim with `n` units spawned near the west start and ordered to
/// the east start. The spawn and move commands are applied before the
/// returned sim is handed to the measured loop.
fn crossing(n: u16) -> Sim {
    let rules = rules();
    let setup = MatchSetup::scenario(&rules, 1);
    let delay = setup.cmd_delay;
    let mut sim = Sim::new(setup, rules, Box::new(NoAi));
    let p = PlayerId(0);
    sim.step(&[PlayerCommand::new(
        p,
        0,
        Command::DebugSpawn {
            owner: p,
            kind: UnitKindId(0),
            at: FxVec2::from_ints(24, 64),
            count: n,
        },
    )]);
    for _ in 0..delay {
        sim.step(&[]);
    }
    let units = sim.view().units().map(|u| u.id).collect();
    sim.step(&[PlayerCommand::new(
        p,
        1,
        Command::Move {
            units,
            target: FxVec2::from_ints(103, 64),
            queue: false,
        },
    )]);
    for _ in 0..delay {
        sim.step(&[]);
    }
    sim
}

fn bench_step(c: &mut Criterion) {
    c.bench_function("step_500_units", |b| {
        b.iter_batched(
            || crossing(500),
            |mut sim| {
                for _ in 0..20 {
                    sim.step(&[]);
                }
                black_box(sim.hash())
            },
            criterion::BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, bench_step);
criterion_main!(benches);
