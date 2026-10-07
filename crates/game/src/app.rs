//! The windowed Bevy app: window, plugins, the sim driver with its input
//! source and recorder, and the `--exit-after-seconds` / `--max-fps` /
//! `--screenshot` smoke-test helpers.
//!
//! Which input source runs (M2 decision 2): `--scenario <name>` builds the
//! sim from `MatchSetup::scenario` (debug commands on) and feeds a
//! `ScenarioInput` (scripted stream + the player's commands); otherwise a
//! skirmish with `LocalInput`. Every windowed session records a replay
//! (decision 3) to `--replay-dir` or the platform data directory.

use std::time::Duration;

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use rules::Rules;
use sim::scenarios;

use crate::cli::Cli;
use crate::hud::HudPlugin;
use crate::orders::OrdersPlugin;
use crate::present::PresentPlugin;
use crate::selection::SelectionPlugin;
use crate::sim_driver::{
    InputSource, LocalInput, ScenarioInput, SimHandle, SimPlugin, new_replay_path, replay_dir,
};
use crate::{camera, ground, palette};

/// Default logical window size.
pub const WINDOW_SIZE: (u32, u32) = (1280, 800);

/// Load `data/` for `cli`, or print why it failed.
pub fn load_rules(cli: &Cli) -> Option<Rules> {
    let dir = cli.resolve_data_dir();
    match Rules::load(&dir) {
        Ok(rules) => Some(rules),
        Err(err) => {
            eprintln!("eonmark: cannot load rules from {}: {err}", dir.display());
            None
        }
    }
}

/// The sim and the input source a windowed session starts with.
pub fn build_match(cli: &Cli, rules: Rules) -> Result<(SimHandle, Box<dyn InputSource>), String> {
    match &cli.scenario {
        None => Ok((SimHandle::skirmish(rules, cli.seed), Box::new(LocalInput))),
        Some(name) => {
            let Some((mut setup, stream, _ticks)) = scenarios::by_name(name, &rules) else {
                return Err(format!(
                    "unknown scenario {name:?}; one of {}",
                    scenarios::NAMES.join(", ")
                ));
            };
            // `--seed` overrides the scenario's own seed only when given
            // explicitly (the default 1 would otherwise clobber it).
            if cli.seed != Cli::default().seed {
                setup.seed = cli.seed;
            }
            Ok((
                SimHandle::from_setup(setup, rules),
                Box::new(ScenarioInput::new(stream)),
            ))
        }
    }
}

/// `--exit-after-seconds`: wall-clock deadline for smoke tests.
#[derive(Resource)]
struct ExitAfter(Duration);

/// `--max-fps <n>`: minimum frame duration enforced by a sleep in `Last`.
///
/// M2-A: the limiter system is a stub; implement `limit_frame_rate` (sleep
/// until `last_frame + 1/n`, spin for the final sub-millisecond) so camera
/// travel and the `scripted_moves` 30 vs 120 fps hash check can be run.
#[derive(Resource, Debug, Clone, Copy)]
#[allow(dead_code)] // M2-A: remove once limit_frame_rate reads max_fps.
pub struct FrameLimiter {
    /// Target frames per second.
    pub max_fps: u32,
}

/// `--screenshot <path>`: capture the primary window about one second
/// before the exit deadline (or at the deadline when it is under a second).
///
/// M2-B: the capture system is a stub; implement with
/// `commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path))`
/// once `Time<Real>::elapsed() >= deadline - 1 s`, exactly once.
#[derive(Resource, Debug, Clone)]
#[allow(dead_code)] // M2-B: remove once take_screenshot reads these.
pub struct ScreenshotRequest {
    /// Output PNG path.
    pub path: std::path::PathBuf,
    /// When to capture, in wall-clock seconds since start.
    pub at_secs: f64,
    /// Set once the capture was requested.
    pub taken: bool,
}

/// Run the windowed game. Returns the exit status for `main`.
pub fn run(cli: &Cli) -> AppExit {
    let Some(rules) = load_rules(cli) else {
        return AppExit::error();
    };
    let (handle, input) = match build_match(cli, rules) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("eonmark: {message}");
            return AppExit::from_code(2);
        }
    };
    let replay_path = new_replay_path(&replay_dir(cli.replay_dir.as_deref()), handle.seed());
    let hud_click_check = cli.scenario.as_deref() == Some("hud_click");

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            // The title stays "Eonmark" (ROADMAP M0 acceptance); the seed,
            // tick and hash readout lives in the `dev` Sim panel.
            title: "Eonmark".into(),
            // Logical size: winit reads this as a LogicalSize at creation.
            resolution: WindowResolution::new(WINDOW_SIZE.0, WINDOW_SIZE.1),
            present_mode: PresentMode::AutoVsync,
            ..default()
        }),
        ..default()
    }));
    app.insert_resource(ClearColor(palette::SKY));
    app.add_plugins(
        SimPlugin::new(handle, input)
            .recording(Some(replay_path))
            .hash_every(cli.hash_every()),
    );
    app.add_plugins((
        camera::CameraPlugin,
        ground::GroundPlugin,
        PresentPlugin,
        SelectionPlugin,
        OrdersPlugin,
        HudPlugin { hud_click_check },
    ));

    #[cfg(target_os = "macos")]
    app.add_plugins(crate::macos_menu::MacosMenuPlugin);

    #[cfg(feature = "dev")]
    app.add_plugins(crate::dev_tools::DevToolsPlugin);

    if let Some(max_fps) = cli.max_fps {
        app.insert_resource(FrameLimiter { max_fps })
            .add_systems(Last, limit_frame_rate);
    }

    if let Some(secs) = cli.exit_after_seconds {
        app.insert_resource(ExitAfter(Duration::from_secs_f64(secs)))
            .add_systems(Update, exit_after);
        if let Some(path) = &cli.screenshot {
            app.insert_resource(ScreenshotRequest {
                path: path.clone(),
                at_secs: (secs - 1.0).max(0.0),
                taken: false,
            })
            .add_systems(Update, take_screenshot);
        }
    } else if cli.screenshot.is_some() {
        eprintln!("eonmark: --screenshot needs --exit-after-seconds to know when to capture");
    }

    app.run()
}

fn exit_after(
    time: Res<Time<Real>>,
    deadline: Res<ExitAfter>,
    sim: NonSend<SimHandle>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed() >= deadline.0 {
        info!(
            "exit-after-seconds reached: seed={} tick={} hash={:#018x}",
            sim.seed(),
            sim.tick(),
            sim.hash()
        );
        exit.write(AppExit::Success);
    }
}

/// Sleep in `Last` so the frame takes at least `1 / max_fps` seconds.
///
/// M2-A: body to implement (track the previous frame's `Instant` in a
/// `Local`; the sim is unaffected because `FixedUpdate` catches up).
fn limit_frame_rate(_limiter: Res<FrameLimiter>, _time: Res<Time<Real>>) {
    // M2-A: see the doc comment.
}

/// Capture the primary window once `at_secs` has passed.
///
/// M2-B: body to implement (`bevy::render::view::screenshot::{Screenshot,
/// save_to_disk}`; dev builds only in practice, the code compiles everywhere).
fn take_screenshot(
    _time: Res<Time<Real>>,
    _request: ResMut<ScreenshotRequest>,
    _commands: Commands,
) {
    // M2-B: see the doc comment.
}
