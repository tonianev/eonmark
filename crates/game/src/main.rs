//! The `eonmark` binary: the windowed game, or a headless simulation run.
//!
//! Module map (M2):
//! - [`cli`]: hand-rolled argument parsing and data directory resolution.
//! - [`app`]: the windowed Bevy app and plugin wiring.
//! - [`sim_driver`]: `SimHandle`, the `FixedUpdate` driver, `InputSource`
//!   (local, replay, scenario), `PendingCommands`, the replay recorder thread.
//! - [`present`]: `UnitId -> Entity` mirror, interpolation, visuals.
//! - [`selection`]: picking, click/box/group selection, rings and markers.
//! - [`orders`]: ground ray-plane hit to `Move` / `Stop` / `AttackMove`.
//! - [`hud`]: top and bottom bars, `PointerOverUi`, the `hud_click` check.
//! - [`camera`]: the yaw-locked RTS camera.
//! - [`ground`] and [`palette`]: the flat 128 x 128 m ground mesh and colours.
//! - [`headless`]: `--headless-run <ticks | replay>` on `MinimalPlugins`.
//! - `macos_menu`: muda menu with the custom Cmd-Q item (macOS only).
//! - `dev_tools`: FPS overlay, egui inspector and sim panel (`dev` only).
#![forbid(unsafe_code)]

mod app;
mod camera;
mod cli;
#[cfg(feature = "dev")]
mod dev_tools;
mod ground;
mod headless;
mod hud;
#[cfg(target_os = "macos")]
mod macos_menu;
mod orders;
mod palette;
mod present;
mod selection;
mod sim_driver;

use std::process::ExitCode;

use bevy::app::AppExit;

fn main() -> ExitCode {
    let cli = match cli::Cli::parse(std::env::args().skip(1)) {
        Ok(cli::Parsed::Run(cli)) => cli,
        Ok(cli::Parsed::Help) => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("eonmark: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };

    let exit = match &cli.headless_run {
        Some(mode) => headless::run(&cli, mode),
        None => app::run(&cli),
    };

    match exit {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}
