//! M5b bot tests that run whole matches (`cargo test -p ai`). Long ones
//! (more than 5000 ticks) are `#[ignore]`d and run in CI with
//! `cargo test -p ai --locked -- --ignored m5b_`.

mod common;

use ai::harness::BotMatch;
use ai::standard::{BotMemory, phase};
use common::Arena;
use sim::{AiController, AiMemory, Command, MatchSetup, PlayerId, SimView};
use std::sync::{Arc, Mutex};

/// A controller that logs the ticks at which the Standard bot inside it
/// returns from a plan (it writes nothing in slice 1, so the log is taken
/// from a wrapper around `plan`).
struct Ticks {
    inner: Box<dyn AiController>,
    calls: Arc<Mutex<Vec<(PlayerId, u32, usize)>>>,
}

impl AiController for Ticks {
    fn think(&mut self, p: PlayerId, view: &SimView<'_>, memory: &mut AiMemory) -> Vec<Command> {
        let out = self.inner.think(p, view, memory);
        self.calls.lock().unwrap().push((p, view.tick, out.len()));
        out
    }
}

/// The skeleton Standard bot plans every 10 ticks, P0 on ticks 0, 10, ...
/// and P1 on ticks 5, 15, ..., issues nothing yet and leaves its memory
/// empty (the default memory encodes to no bytes); a plan of the arena
/// matches.
#[test]
fn standard_skeleton_keeps_its_cadence_and_issues_nothing() {
    let r = common::rules();
    let setup = MatchSetup::bots(&r, 1, ("standard", "standard"), ("standard", "standard"));
    let every = r.ai.personality("standard").unwrap().think_every_ticks(&r);
    assert_eq!(every, 10);
    assert_eq!(
        (
            phase(PlayerId(0), every, &setup),
            phase(PlayerId(1), every, &setup)
        ),
        (0, 5)
    );
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = ai::controller_for(&setup, &r).unwrap();
    let mut m = BotMatch::new(
        setup.clone(),
        r.clone(),
        Box::new(Ticks {
            inner,
            calls: calls.clone(),
        }),
    );
    for _ in 0..40 {
        m.step();
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 80, "the sim calls both seats every tick");
    assert!(calls.iter().all(|c| c.2 == 0), "no orders yet");
    assert_eq!(m.sim.view().commands_applied(), 0);
    for p in [PlayerId(0), PlayerId(1)] {
        assert!(m.sim.view().ai_memory(p).is_empty());
    }
    let arena = Arena::new(setup, Vec::new());
    let (cmds, memory) = arena.plan(PlayerId(1));
    assert!(cmds.is_empty() && memory.is_empty());
    assert_eq!(BotMemory::decode(memory.bytes()), BotMemory::default());
    let ctx = arena.ctx(PlayerId(0));
    assert_eq!(ctx.facts.workers.len(), 5, "the standard start's Yeomen");
    assert_eq!(ctx.facts.towns.len(), 1, "the Seat");
    assert_eq!(ctx.facts.enemy_towns.len(), 1);
    assert_eq!(ctx.ledger.stock, ctx.facts.stock);
    assert_eq!(ctx.commands_left, 16);
    assert_eq!(ctx.probes_left, 4);
}

/// A Standard-vs-Standard match is the same match on two threads, and a
/// header naming the Standard bot rebuilds it (`controller_for`) so a
/// fresh run reproduces the hash.
#[test]
fn standard_vs_standard_is_deterministic_across_threads() {
    let r = common::rules();
    let setup = MatchSetup::bots(&r, 3, ("standard", "standard"), ("standard", "standard"));
    let run = || {
        let mut m = BotMatch::with_bots(setup.clone(), r.clone()).unwrap();
        m.run(600);
        m.sim.hash()
    };
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(run);
        let b = s.spawn(run);
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(a, b);
    assert_eq!(a, run());
}
