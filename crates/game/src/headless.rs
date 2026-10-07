//! `--headless-run <ticks | path.eonreplay>`: no window, `MinimalPlugins`.
//!
//! `<ticks>` (M0): step the default skirmish `ticks` times with no commands
//! and print `tick=<n> hash=0x<16 hex>`; exit 0. The CI smoke test on both
//! operating systems diffs that line.
//!
//! `<path.eonreplay>` (M2): build the sim from the replay header, drive it
//! with `sim_driver::ReplayInput` through `sim_driver::drive_tick` (no
//! recorder), and when `ReplayFinished` arrives print the same
//! `tick=<n> hash=0x<16 hex>` line as the LAST stdout line and exit 0 if
//! the recorded final hash matches, 1 otherwise. A `RULES CHANGED` or
//! `SIM VERSION MISMATCH` header is reported on stderr with exit 1 before
//! simulating, like `sim-cli verify`. Must finish `move_500` (1200 ticks)
//! in under 10 s on the dev Mac in the `ci` profile: step as many ticks per
//! `Update` as needed (never through `FixedUpdate`, which would be wall
//! clock bound).
//!
//! Ownership (M2 contract): agent A owns the replay mode; it is stubbed
//! here and prints `headless replay: not implemented` with exit 1.

use std::path::Path;
use std::time::Duration;

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;

use crate::app::load_rules;
use crate::cli::{Cli, HeadlessRun};
use crate::sim_driver::SimHandle;

#[derive(Resource)]
struct Target(u32);

/// Run the requested headless mode and return the exit status for `main`.
pub fn run(cli: &Cli, mode: &HeadlessRun) -> AppExit {
    match mode {
        HeadlessRun::Ticks(ticks) => run_ticks(cli, *ticks),
        HeadlessRun::Replay(path) => run_replay(cli, path),
    }
}

/// The exact stdout line both headless modes end with.
pub fn hash_line(tick: u32, hash: u64) -> String {
    format!("tick={tick} hash={hash:#018x}")
}

/// `--headless-run <ticks>`: one sim tick per app update, driven by the
/// runner, never by wall time, so the tick count is exact.
fn run_ticks(cli: &Cli, ticks: u32) -> AppExit {
    let Some(rules) = load_rules(cli) else {
        return AppExit::error();
    };
    let handle = SimHandle::skirmish(rules, cli.seed);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::ZERO)));
    app.world_mut().insert_non_send(handle);
    app.insert_resource(Target(ticks));
    app.add_systems(Update, step_and_report);
    app.run()
}

fn step_and_report(
    mut sim: NonSendMut<SimHandle>,
    target: Res<Target>,
    mut exit: MessageWriter<AppExit>,
) {
    if sim.tick() < target.0 {
        sim.step_once();
    }
    if sim.tick() >= target.0 {
        println!("{}", hash_line(sim.tick(), sim.hash()));
        exit.write(AppExit::Success);
    }
}

/// `--headless-run <path.eonreplay>`. See the module docs for the contract.
///
/// M2-A: body to implement with `ReplayInput::open`, `SimHandle::from_setup`,
/// `SimPlugin::new(handle, Box::new(input))` (not recording) or a direct
/// `drive_tick` loop in `Update`, and a `MessageReader<ReplayFinished>`
/// that prints `hash_line(final_tick, sim_hash)` last and exits 0/1.
fn run_replay(cli: &Cli, path: &Path) -> AppExit {
    let Some(_rules) = load_rules(cli) else {
        return AppExit::error();
    };
    eprintln!(
        "eonmark: headless replay: not implemented (M2-A); asked for {}",
        path.display()
    );
    AppExit::error()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_line_is_the_ci_format() {
        assert_eq!(hash_line(200, 0x1f), "tick=200 hash=0x000000000000001f");
    }
}
