//! The `eonmark` binary: the windowed game, or a headless simulation run.
//! The modules live in the `game` library crate (`src/lib.rs`) so the
//! integration tests under `tests/` can drive the real plugins.
#![forbid(unsafe_code)]

use std::process::ExitCode;

use bevy::app::AppExit;
use game::{app, cli, headless};

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
