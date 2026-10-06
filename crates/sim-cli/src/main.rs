//! Headless command-line tooling for the Eonmark simulation.
//!
//! Runs on any OS without a GPU. M0 implemented `selftest`, `data-check` and
//! the scripted `hash-dump`; M1 adds `verify`, `bench`, `fuzz`, `record` and
//! the replay form of `hash-dump` (skeletons exit 2 with "not implemented"
//! until implementer C fills them); `play-bots` arrives in M5b.
#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use sim::{MatchSetup, Rules, Sim};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "sim-cli",
    about = "Headless Eonmark simulation tooling",
    version
)]
struct Cli {
    /// Data directory (rules, maps).
    #[arg(long, global = true, default_value = "data")]
    data: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run scripted ticks twice on two threads and compare hashes.
    Selftest {
        /// Ticks to simulate in each run.
        #[arg(long, default_value_t = 1000)]
        ticks: u32,
        /// Match seed.
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Load and validate everything under a data directory.
    DataCheck {
        /// Path to the data directory (overrides --data).
        dir: Option<PathBuf>,
    },
    /// Print per-tick hashes: of a replay's re-simulation (with sub-hashes
    /// every --every ticks) when a replay path is given, otherwise of the
    /// scripted M0 match.
    HashDump {
        /// Replay file (.eonreplay) to re-simulate.
        replay: Option<PathBuf>,
        /// Print a line every N ticks (replay form).
        #[arg(long, default_value_t = 20)]
        every: u32,
        /// Ticks to simulate (scripted form).
        #[arg(long, default_value_t = 100)]
        ticks: u32,
        /// Match seed (scripted form).
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Re-simulate a replay and report OK / DIVERGED / SIM VERSION MISMATCH /
    /// RULES CHANGED (exit 0 only for OK).
    Verify {
        /// Replay file (.eonreplay).
        replay: PathBuf,
    },
    /// Measure step time: N units crossing the map, or one saturated A* budget.
    Bench {
        /// Units to spawn and move.
        #[arg(long, default_value_t = 500)]
        units: u16,
        /// Ticks to simulate.
        #[arg(long, default_value_t = 1200)]
        ticks: u32,
        /// Benchmark a saturated A* tick instead of the crossing.
        #[arg(long)]
        astar: bool,
        /// Expansion budget for --astar (defaults to rules.ron).
        #[arg(long)]
        budget: Option<u32>,
        /// Match seed.
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Feed seeded random command streams to the sim and compare two runs.
    Fuzz {
        /// Ticks per run.
        #[arg(long, default_value_t = 300)]
        ticks: u32,
        /// Seed of the first case; one case per seed.
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Number of cases.
        #[arg(long, default_value_t = 32)]
        cases: u32,
    },
    /// Record a scripted scenario to an .eonreplay file.
    Record {
        /// Scenario name (`move_500`, `group_spiral`, `snapshot_restore`, ...).
        #[arg(long)]
        scenario: String,
        /// Ticks to simulate.
        #[arg(long, default_value_t = 1200)]
        ticks: u32,
        /// Output file.
        #[arg(long)]
        out: PathBuf,
        /// Write a hash record every tick instead of every 20.
        #[arg(long)]
        hash_every_tick: bool,
        /// Match seed.
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Run seeded bot-vs-bot matches in parallel (M5b).
    PlayBots,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Selftest { ticks, seed } => selftest(&cli.data, ticks, seed),
        Cmd::DataCheck { dir } => data_check(dir.as_deref().unwrap_or(&cli.data)),
        Cmd::HashDump {
            replay: None,
            ticks,
            seed,
            ..
        } => hash_dump(&cli.data, ticks, seed),
        Cmd::HashDump {
            replay: Some(_), ..
        } => not_implemented("hash-dump <replay>"),
        Cmd::Verify { .. } => not_implemented("verify"),
        Cmd::Bench { .. } => not_implemented("bench"),
        Cmd::Fuzz { .. } => not_implemented("fuzz"),
        Cmd::Record { .. } => not_implemented("record"),
        Cmd::PlayBots => not_yet("play-bots", "M5b"),
    }
}

/// M1 subcommand skeleton: implementer C replaces the call site.
fn not_implemented(name: &str) -> ExitCode {
    eprintln!("{name}: not implemented");
    ExitCode::from(2)
}

fn not_yet(name: &str, milestone: &str) -> ExitCode {
    eprintln!("{name}: not implemented until {milestone}");
    ExitCode::from(2)
}

/// Build the scripted M0 match: human slot driven by
/// `ai::selftest_human_commands`, AI slot by `ai::Scripted::selftest`.
fn scripted_sim(data: &Path, seed: u64) -> Result<Sim, rules::Error> {
    let rules = Rules::load(data)?;
    let setup = MatchSetup::skirmish(&rules, seed);
    Ok(Sim::new(setup, rules, Box::new(ai::Scripted::selftest())))
}

fn run_scripted(data: &Path, ticks: u32, seed: u64) -> Result<u64, rules::Error> {
    let mut sim = scripted_sim(data, seed)?;
    for t in 0..ticks {
        sim.step(&ai::selftest_human_commands(t));
    }
    Ok(sim.hash())
}

fn selftest(data: &Path, ticks: u32, seed: u64) -> ExitCode {
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(|| run_scripted(data, ticks, seed));
        let b = s.spawn(|| run_scripted(data, ticks, seed));
        (a.join().expect("thread a"), b.join().expect("thread b"))
    });
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    println!("selftest ticks={ticks} seed={seed}");
    println!("thread A hash=0x{a:016x}");
    println!("thread B hash=0x{b:016x}");
    if a == b {
        println!("OK");
        ExitCode::SUCCESS
    } else {
        println!("MISMATCH");
        ExitCode::from(1)
    }
}

fn data_check(dir: &Path) -> ExitCode {
    match Rules::load(dir) {
        Ok(r) => {
            println!(
                "OK rules_version={} rules_hash=0x{:016x} resources={} maps={}",
                r.rules_version,
                r.rules_hash(),
                r.resources.resources.len(),
                r.maps.keys().cloned().collect::<Vec<_>>().join(",")
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn hash_dump(data: &Path, ticks: u32, seed: u64) -> ExitCode {
    let mut sim = match scripted_sim(data, seed) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    println!("tick hash");
    for t in 0..ticks {
        sim.step(&ai::selftest_human_commands(t));
        println!("{} 0x{:016x}", sim.tick(), sim.hash());
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn scripted_run_is_repeatable() {
        let a = run_scripted(&data(), 200, 42).unwrap();
        let b = run_scripted(&data(), 200, 42).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, run_scripted(&data(), 200, 43).unwrap());
    }

    #[test]
    fn cli_parses_stubs() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["sim-cli", "verify", "m.eonreplay"]).unwrap();
        assert!(matches!(cli.cmd, Cmd::Verify { replay } if replay == Path::new("m.eonreplay")));
        assert!(Cli::try_parse_from(["sim-cli", "verify"]).is_err());
        let cli =
            Cli::try_parse_from(["sim-cli", "--data", "x", "hash-dump", "--ticks", "3"]).unwrap();
        assert_eq!(cli.data, PathBuf::from("x"));
        assert!(matches!(
            cli.cmd,
            Cmd::HashDump {
                replay: None,
                ticks: 3,
                every: 20,
                ..
            }
        ));
        let cli =
            Cli::try_parse_from(["sim-cli", "hash-dump", "m.eonreplay", "--every", "1"]).unwrap();
        assert!(matches!(
            cli.cmd,
            Cmd::HashDump {
                replay: Some(_),
                every: 1,
                ..
            }
        ));
        let cli = Cli::try_parse_from([
            "sim-cli", "bench", "--units", "500", "--ticks", "1200", "--astar", "--budget", "4000",
        ])
        .unwrap();
        assert!(matches!(
            cli.cmd,
            Cmd::Bench {
                units: 500,
                ticks: 1200,
                astar: true,
                budget: Some(4000),
                ..
            }
        ));
        let cli = Cli::try_parse_from(["sim-cli", "fuzz", "--ticks", "10", "--seed", "7"]).unwrap();
        assert!(matches!(
            cli.cmd,
            Cmd::Fuzz {
                ticks: 10,
                seed: 7,
                ..
            }
        ));
        let cli = Cli::try_parse_from([
            "sim-cli",
            "record",
            "--scenario",
            "move_500",
            "--ticks",
            "1200",
            "--out",
            "x.eonreplay",
            "--hash-every-tick",
        ])
        .unwrap();
        assert!(matches!(
            cli.cmd,
            Cmd::Record {
                hash_every_tick: true,
                ticks: 1200,
                ..
            }
        ));
    }
}
