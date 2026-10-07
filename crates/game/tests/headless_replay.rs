//! Integration test for the headless replay path: a golden fixture is
//! re-simulated through the real `SimPlugin` on `MinimalPlugins`, with the
//! fixed clock stepped manually (`TimeUpdateStrategy::ManualDuration`, one
//! sim tick per `App::update`), and the `ReplayFinished` message the driver
//! writes must carry the fixture's recorded hash. This is the same path
//! `eonmark --headless-run <file.eonreplay>` and the CI hash-parity job
//! take, minus the process exit code.
//!
//! `move_500_short` (300 ticks) runs in the default `cargo test -p game`
//! suite; the 1200-tick `move_500` version is `#[ignore]` like the sim
//! crate's long golden and is covered by CI's `--headless-run` step.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use game::headless::hash_line;
use game::sim_driver::{
    DriverStats, InputSource, ReplayFinished, ReplayInput, SimHandle, SimPlugin,
};
use rules::Rules;
use sim::scenarios;

/// The first `ReplayFinished` the driver wrote, captured in `Last`.
#[derive(Resource, Default)]
struct Outcome(Option<ReplayFinished>);

fn capture_finished(mut finished: MessageReader<ReplayFinished>, mut outcome: ResMut<Outcome>) {
    for f in finished.read() {
        outcome.0.get_or_insert(*f);
    }
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../sim/tests/fixtures")
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// The sibling `.hash` file: `0x<16 hex>` and a newline.
fn recorded_hash(name: &str) -> u64 {
    let path = fixtures_dir().join(format!("{name}.hash"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    u64::from_str_radix(text.trim().trim_start_matches("0x"), 16)
        .unwrap_or_else(|e| panic!("{}: not a hash: {e}", path.display()))
}

/// What one replay through the app produced.
struct Run {
    finished: ReplayFinished,
    stats: DriverStats,
    updates: u32,
}

/// Build the sim from the fixture header, drive it with `SimPlugin` +
/// `ReplayInput` on `MinimalPlugins`, one fixed step per update, until the
/// driver reports the end of the replay.
fn replay_through_sim_plugin(name: &str) -> Run {
    let path = fixtures_dir().join(format!("{name}.eonreplay"));
    let rules = Rules::load(data_dir()).expect("data/ loads");
    let (setup, input) =
        ReplayInput::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let end = input.end_of_input().expect("a replay is a finite source");
    let handle = SimHandle::from_setup(setup, rules);
    let timestep = Duration::from_secs_f64(1.0 / f64::from(handle.tick_rate_hz()));

    let mut app = App::new();
    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_once()));
    // Advance the clocks by exactly one fixed timestep per update, so the
    // fixed schedule runs `step_sim` once per `app.update()` regardless of
    // wall time (no recorder: headless replay runs never record).
    app.insert_resource(TimeUpdateStrategy::ManualDuration(timestep));
    app.add_plugins(SimPlugin::new(handle, Box::new(input)));
    app.init_resource::<Outcome>();
    app.add_systems(Last, capture_finished);
    app.finish();
    app.cleanup();

    // The first update only primes `Time<Real>` (zero delta); then one tick
    // per update up to `final_tick`, and one more step for `Finished`.
    let max_updates = end.final_tick * 2 + 16;
    let mut updates = 0;
    while updates < max_updates {
        app.update();
        updates += 1;
        if let Some(finished) = app.world().resource::<Outcome>().0 {
            let stats = *app.world().resource::<DriverStats>();
            return Run {
                finished,
                stats,
                updates,
            };
        }
    }
    panic!(
        "{name}: no ReplayFinished after {max_updates} updates (final_tick {})",
        end.final_tick
    );
}

fn assert_replays_to_recorded_hash(name: &str, ticks: u32) {
    let expected = recorded_hash(name);
    let run = replay_through_sim_plugin(name);
    let f = run.finished;
    assert_eq!(f.final_tick, ticks, "{name}: {f:?}");
    assert_eq!(f.recorded_hash, Some(expected), "{name}: {f:?}");
    assert!(f.matches(), "{name}: DIVERGED {f:?}");
    assert_eq!(f.sim_hash, expected, "{name}: {f:?}");
    assert_eq!(
        hash_line(f.final_tick, f.sim_hash),
        format!("tick={ticks} hash={expected:#018x}"),
        "the CI hash-parity line"
    );
    assert_eq!(
        run.stats.stalled_ticks, 0,
        "a replay never stalls: {:?}",
        run.stats
    );
    assert_eq!(
        run.stats.dropped_ticks, 0,
        "one tick per update never hits the cap: {:?}",
        run.stats
    );
    assert!(
        run.updates <= ticks + 4,
        "{name}: {} updates for {ticks} ticks; the fixed clock should step once per update",
        run.updates
    );
}

#[test]
fn sim_plugin_replays_move_500_short_to_its_recorded_hash() {
    assert_replays_to_recorded_hash("move_500_short", scenarios::MOVE_500_SHORT_TICKS);
}

#[test]
#[ignore = "long golden (1200 ticks); CI runs it through `eonmark --headless-run` in the ci profile"]
fn sim_plugin_replays_move_500_to_its_recorded_hash() {
    assert_replays_to_recorded_hash("move_500", scenarios::MOVE_500_TICKS);
}
