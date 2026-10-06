//! Headless command-line tooling for the Eonmark simulation.
//!
//! Runs on any OS without a GPU. M0 implemented `selftest`, `data-check` and
//! the scripted `hash-dump`; M1 adds `verify`, `bench`, `fuzz`, `record` and
//! the replay form of `hash-dump`; `play-bots` arrives in M5b.
//!
//! Wall-clock timing (`std::time::Instant`) is allowed here and banned in
//! `sim`, `rules` and `ai`: this crate measures the sim, it is not part of it.
#![forbid(unsafe_code)]

mod scenarios;

use clap::{Parser, Subcommand};
use scenarios::Scenario;
use sim::replay::DEFAULT_HASH_EVERY;
use sim::{
    AStarSearch, FxVec2, Map, MatchSetup, PlayerId, ReplayReader, ReplayWriter, Rules,
    SearchStatus, Sim, SimEvent, SubHashes, UnitId, VerifyOutcome,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

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
        /// Scenario name (`move_500`, `move_500_short`, `group_spiral`,
        /// `snapshot_restore`, `selftest`).
        #[arg(long)]
        scenario: String,
        /// Ticks to simulate (default: the scenario's fixture length).
        #[arg(long)]
        ticks: Option<u32>,
        /// Output file.
        #[arg(long)]
        out: PathBuf,
        /// Write a hash record every tick instead of every 20.
        #[arg(long)]
        hash_every_tick: bool,
        /// Match seed.
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Sleep this many milliseconds after every tick (to land a `kill -9`
        /// mid-recording when testing truncation tolerance).
        #[arg(long, default_value_t = 0)]
        slow_ms: u64,
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
            replay: Some(replay),
            every,
            ..
        } => hash_dump_replay(&cli.data, &replay, every),
        Cmd::Verify { replay } => verify(&cli.data, &replay),
        Cmd::Bench {
            units,
            ticks,
            astar,
            budget,
            seed,
        } => bench(&cli.data, units, ticks, astar, budget, seed),
        Cmd::Fuzz { ticks, seed, cases } => fuzz(&cli.data, ticks, seed, cases),
        Cmd::Record {
            scenario,
            ticks,
            out,
            hash_every_tick,
            seed,
            slow_ms,
        } => record(
            &cli.data,
            &scenario,
            ticks,
            &out,
            hash_every_tick,
            seed,
            slow_ms,
        ),
        Cmd::PlayBots => not_yet("play-bots", "M5b"),
    }
}

/// Exit status for a failure that is neither a verify outcome nor success:
/// unreadable files, unloadable rules, unknown scenario names.
const EXIT_ERROR: u8 = 2;

fn not_yet(name: &str, milestone: &str) -> ExitCode {
    eprintln!("{name}: not implemented until {milestone}");
    ExitCode::from(EXIT_ERROR)
}

fn load_rules(data: &Path) -> Result<Rules, ExitCode> {
    Rules::load(data).map_err(|e| {
        eprintln!("error: {e}");
        ExitCode::from(EXIT_ERROR)
    })
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

// ---------------------------------------------------------------------------
// M1: verify, hash-dump <replay>, record, bench, fuzz
// ---------------------------------------------------------------------------

/// The controller that drives AI slots during a re-simulation. M1 replays
/// are scenario recordings with two human slots; a header with an AI slot is
/// driven by `ai::Passive` with a warning until the header names a bot (M5b).
fn replay_controller(replay: &Path) -> Box<dyn sim::AiController> {
    if let Ok((setup, _)) = ReplayReader::open(replay)
        && setup.players.iter().any(|p| p.is_ai)
    {
        eprintln!(
            "warning: {} has AI slots; they are driven by ai::Passive (bot ids in the header arrive in M5b)",
            replay.display()
        );
    }
    Box::new(ai::Passive)
}

fn verify(data: &Path, replay: &Path) -> ExitCode {
    match sim::verify(replay, data, replay_controller(replay)) {
        Ok(outcome) => {
            println!("{outcome}");
            ExitCode::from(outcome.exit_code())
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

/// One `hash-dump` line: whole-state hash then every sub-hash in
/// `SubHashes::NAMES` order.
fn hash_dump_line(tick: u32, hash: u64, sub: &SubHashes) -> String {
    let mut line = format!("tick={tick} hash=0x{hash:016x}");
    for (name, value) in SubHashes::NAMES.iter().zip(sub.values()) {
        line.push_str(&format!(" {name}=0x{value:016x}"));
    }
    line
}

fn hash_dump_replay(data: &Path, replay: &Path, every: u32) -> ExitCode {
    use std::io::Write;
    let ai = replay_controller(replay);
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let result = sim::replay::hash_dump(replay, data, ai, every, |tick, hash, sub| {
        // `hash-dump ... | head` closes the pipe early; that is not an error.
        if writeln!(out, "{}", hash_dump_line(tick, hash, sub)).is_err() {
            std::process::exit(0);
        }
    });
    if out.flush().is_err() {
        return ExitCode::SUCCESS;
    }
    drop(out);
    match result {
        Ok(VerifyOutcome::Ok { .. }) => ExitCode::SUCCESS,
        Ok(outcome) => {
            eprintln!("{outcome}");
            ExitCode::from(outcome.exit_code())
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn record(
    data: &Path,
    name: &str,
    ticks: Option<u32>,
    out: &Path,
    hash_every_tick: bool,
    seed: u64,
    slow_ms: u64,
) -> ExitCode {
    let Some(scenario) = Scenario::parse(name) else {
        eprintln!(
            "record: unknown scenario {name:?}; one of {}",
            Scenario::NAMES.join(", ")
        );
        return ExitCode::from(EXIT_ERROR);
    };
    let ticks = ticks.unwrap_or_else(|| scenario.default_ticks());
    let rules = match load_rules(data) {
        Ok(r) => r,
        Err(code) => return code,
    };
    match record_scenario(&rules, scenario, ticks, out, hash_every_tick, seed, slow_ms) {
        Ok((final_hash, records)) => {
            println!(
                "recorded {} scenario={name} ticks={ticks} records={records} final_hash=0x{final_hash:016x}",
                out.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

/// Run `scenario` for `ticks` ticks writing `out`: a `Tick` record for every
/// tick with commands, a `Hash` record every `hash_every` ticks and at the
/// end, a `flush` every 20 ticks (no fsync) and `finish` at clean exit, so a
/// `kill -9` leaves a file that verifies up to its last complete record.
/// Returns the final hash and the number of records written.
fn record_scenario(
    rules: &Rules,
    scenario: Scenario,
    ticks: u32,
    out: &Path,
    hash_every_tick: bool,
    seed: u64,
    slow_ms: u64,
) -> Result<(u64, u64), sim::ReplayError> {
    let hash_every = if hash_every_tick {
        1
    } else {
        DEFAULT_HASH_EVERY
    };
    let mut sim = scenarios::build(rules, seed);
    let mut writer = ReplayWriter::create(out, sim.setup())?;
    let mut last_hashed = 0;
    for t in 0..ticks {
        if scenario.before_step(&mut sim, t, || scenarios::build(rules, seed)) {
            eprintln!(
                "snapshot/restore swap before tick {t}: hash 0x{:016x}",
                sim.hash()
            );
        }
        let cmds = scenario.commands(&sim, t);
        writer.tick(t, &cmds)?;
        sim.step(&cmds);
        if sim.tick().is_multiple_of(hash_every) {
            writer.hash(sim.tick(), sim.hash(), &sim.sub_hashes())?;
            last_hashed = sim.tick();
        }
        if sim.tick().is_multiple_of(DEFAULT_HASH_EVERY) {
            writer.flush()?;
        }
        if slow_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(slow_ms));
        }
    }
    if last_hashed != sim.tick() {
        writer.hash(sim.tick(), sim.hash(), &sim.sub_hashes())?;
    }
    let records = writer.records();
    writer.finish()?;
    Ok((sim.hash(), records))
}

/// Mean, 95th percentile and maximum of `samples` (milliseconds).
fn stats_ms(samples: &[f64]) -> (f64, f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let rank = ((sorted.len() as f64 * 0.95).ceil() as usize).clamp(1, sorted.len());
    (mean, sorted[rank - 1], sorted[sorted.len() - 1])
}

/// Squared distance, in `dist_sq_i64` scale, below which a unit counts as
/// arrived: 3 tiles.
const ARRIVED_DIST_SQ: i64 = (3 * 3) << 32;

fn bench(
    data: &Path,
    units: u16,
    ticks: u32,
    astar: bool,
    budget: Option<u32>,
    seed: u64,
) -> ExitCode {
    let rules = match load_rules(data) {
        Ok(r) => r,
        Err(code) => return code,
    };
    if astar {
        return bench_astar(&rules, budget.unwrap_or(rules.path_budget_expansions));
    }
    let scenario = Scenario::Crossing { units };
    let mut sim = scenarios::build(&rules, seed);
    // Each unit's goal, captured while its order is live (arrival clears it).
    let mut goals: BTreeMap<UnitId, FxVec2> = BTreeMap::new();
    let mut step_ms = Vec::with_capacity(ticks as usize);
    for t in 0..ticks {
        let cmds = scenario.commands(&sim, t);
        let start = Instant::now();
        sim.step(&cmds);
        step_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        for u in sim.view().units() {
            if let Some(order) = &u.order {
                goals.entry(u.id).or_insert(order.goal);
            }
        }
    }
    let (mean, p95, max) = stats_ms(&step_ms);
    let view = sim.view();
    let arrived = goals
        .iter()
        .filter(|(id, goal)| {
            view.unit(**id)
                .is_some_and(|u| u.pos.dist_sq_i64(**goal) <= ARRIVED_DIST_SQ)
        })
        .count();
    let pct = if goals.is_empty() {
        0.0
    } else {
        100.0 * arrived as f64 / goals.len() as f64
    };
    println!("bench units={units} ticks={ticks} seed={seed}");
    println!("step_ms mean={mean:.3} p95={p95:.3} max={max:.3}");
    println!(
        "arrived {pct:.1}% ({arrived}/{}) within 3 tiles of the goal",
        goals.len()
    );
    println!("final_hash=0x{:016x}", sim.hash());
    ExitCode::SUCCESS
}

/// Iterations of the saturated search in `bench --astar`.
const ASTAR_ITERATIONS: u32 = 100;

/// Time one `resume` with the full `budget` on a search that cannot finish
/// inside it: from the west start towards a river tile, which is blocked, so
/// the open set never reaches the goal and every expansion is spent.
fn bench_astar(rules: &Rules, budget: u32) -> ExitCode {
    let def = rules
        .map(&rules.default_map)
        .expect("default map is validated by Rules::load");
    let map = Map::from_def(def);
    let from = map.tile_of(scenarios::WEST);
    let to = map.tile_of(scenarios::RIVER);
    let mut tick_ms = Vec::with_capacity(ASTAR_ITERATIONS as usize);
    let mut found = 0u32;
    let mut exhausted = 0u32;
    let mut suspended = 0u32;
    let mut used = 0u32;
    for _ in 0..ASTAR_ITERATIONS {
        let mut search = AStarSearch::new(&map, from, to);
        let mut remaining = budget;
        let start = Instant::now();
        let status = search.resume(&map, &mut remaining);
        tick_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        used = budget - remaining;
        match status {
            SearchStatus::Found(_) => found += 1,
            SearchStatus::Exhausted => exhausted += 1,
            SearchStatus::Suspended => suspended += 1,
        }
    }
    let (mean, p95, max) = stats_ms(&tick_ms);
    println!(
        "bench astar budget={budget} iterations={ASTAR_ITERATIONS} from={from:?} to={to:?} (blocked goal)"
    );
    println!("tick_ms mean={mean:.3} p95={p95:.3} max={max:.3}");
    println!("expansions_used={used} found={found} exhausted={exhausted} suspended={suspended}");
    ExitCode::SUCCESS
}

/// What one fuzz case observed.
struct FuzzCase {
    commands: usize,
    rejected: usize,
}

/// Run one seeded random stream twice; the hashes must agree on every tick
/// and the rejected `(player, seq)` set must equal the generator's
/// expectation.
fn fuzz_case(rules: &Rules, seed: u64, ticks: u32, movement: bool) -> Result<FuzzCase, String> {
    let (stream, expected) = scenarios::random_commands(seed, ticks, movement);
    let run = || {
        let mut sim = scenarios::build(rules, seed);
        let mut hashes = Vec::with_capacity(stream.len());
        for cmds in &stream {
            sim.step(cmds);
            hashes.push(sim.hash());
        }
        // Let the last tick's commands pass through the delay queue.
        for _ in 0..=sim.setup().cmd_delay {
            sim.step(&[]);
        }
        let rejected: BTreeSet<(PlayerId, u32)> = sim
            .drain_events()
            .into_iter()
            .filter_map(|e| match e {
                SimEvent::CommandRejected { player, seq, .. } => Some((player, seq)),
                _ => None,
            })
            .collect();
        (hashes, rejected)
    };
    let (hashes_a, rejected_a) = run();
    let (hashes_b, rejected_b) = run();
    if hashes_a != hashes_b {
        let tick = hashes_a
            .iter()
            .zip(&hashes_b)
            .position(|(a, b)| a != b)
            .map_or(hashes_a.len(), |i| i + 1);
        return Err(format!("seed={seed}: two runs diverged at tick {tick}"));
    }
    if rejected_a != rejected_b {
        return Err(format!("seed={seed}: two runs rejected different commands"));
    }
    let expected: BTreeSet<(PlayerId, u32)> = expected.into_iter().collect();
    if rejected_a != expected {
        let missing: Vec<_> = expected.difference(&rejected_a).collect();
        let unexpected: Vec<_> = rejected_a.difference(&expected).collect();
        return Err(format!(
            "seed={seed}: rejection mismatch; not rejected={missing:?} unexpectedly rejected={unexpected:?}"
        ));
    }
    Ok(FuzzCase {
        commands: stream.iter().map(Vec::len).sum(),
        rejected: rejected_a.len(),
    })
}

fn fuzz(data: &Path, ticks: u32, seed: u64, cases: u32) -> ExitCode {
    let rules = match load_rules(data) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let mut commands = 0;
    let mut rejected = 0;
    for case in 0..u64::from(cases) {
        match fuzz_case(&rules, seed + case, ticks, true) {
            Ok(c) => {
                commands += c.commands;
                rejected += c.rejected;
            }
            Err(msg) => {
                println!("fuzz FAILED {msg}");
                return ExitCode::from(1);
            }
        }
    }
    println!(
        "fuzz cases={cases} ticks={ticks} seed={seed} commands={commands} rejected={rejected} OK"
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eonmark-sim-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn scripted_run_is_repeatable() {
        let a = run_scripted(&data(), 200, 42).unwrap();
        let b = run_scripted(&data(), 200, 42).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, run_scripted(&data(), 200, 43).unwrap());
    }

    #[test]
    fn cli_parses_m1_subcommands() {
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
            "--slow-ms",
            "5",
        ])
        .unwrap();
        assert!(matches!(
            cli.cmd,
            Cmd::Record {
                hash_every_tick: true,
                ticks: Some(1200),
                slow_ms: 5,
                ..
            }
        ));
    }

    #[test]
    fn record_then_verify_selftest_scenario() {
        let rules = Rules::load(data()).unwrap();
        let out = temp("selftest.eonreplay");
        let (final_hash, records) =
            record_scenario(&rules, Scenario::Selftest, 90, &out, false, 1, 0).unwrap();
        // Hash records at 20, 40, 60, 80 and the final one at 90, plus the
        // tick batches the selftest stream issues.
        assert!(records >= 5);
        let outcome = sim::verify(&out, &data(), Box::new(ai::Passive)).unwrap();
        assert_eq!(
            outcome,
            VerifyOutcome::Ok {
                final_hash,
                ticks: 90
            }
        );
        // --hash-every-tick: one hash record per tick, same final hash.
        let out2 = temp("selftest_every.eonreplay");
        let (final_hash2, _) =
            record_scenario(&rules, Scenario::Selftest, 90, &out2, true, 1, 0).unwrap();
        assert_eq!(final_hash2, final_hash);
        let (_, recs) = ReplayReader::open(&out2).unwrap();
        let hashes = recs
            .iter()
            .filter(|r| matches!(r, sim::replay::Record::Hash(_)))
            .count();
        assert_eq!(hashes, 90);
        // A different seed changes the final hash.
        let out3 = temp("selftest_seed2.eonreplay");
        let (final_hash3, _) =
            record_scenario(&rules, Scenario::Selftest, 90, &out3, false, 2, 0).unwrap();
        assert_ne!(final_hash3, final_hash);
    }

    #[test]
    fn hash_dump_line_has_every_subsystem() {
        let line = hash_dump_line(7, 1, &SubHashes::default());
        assert!(line.starts_with("tick=7 hash=0x0000000000000001 units=0x"));
        for name in SubHashes::NAMES {
            assert!(line.contains(&format!(" {name}=0x")), "{line}");
        }
    }

    #[test]
    fn fuzz_case_without_movement_matches_expected_rejections() {
        let rules = Rules::load(data()).unwrap();
        for seed in 1..=4 {
            let case = fuzz_case(&rules, seed, 150, false).unwrap();
            assert!(case.commands > 0);
            assert!(
                case.rejected > 0,
                "seed {seed} produced no invalid commands"
            );
        }
    }

    #[test]
    fn stats_ms_percentiles() {
        let s: Vec<f64> = (1..=100).map(f64::from).collect();
        let (mean, p95, max) = stats_ms(&s);
        assert!((mean - 50.5).abs() < 1e-9);
        assert!((p95 - 95.0).abs() < 1e-9);
        assert!((max - 100.0).abs() < 1e-9);
        assert_eq!(stats_ms(&[]), (0.0, 0.0, 0.0));
    }
}
