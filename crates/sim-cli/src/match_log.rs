//! `sim-cli match-log <replay>` (M6): re-simulate a replay with the bots
//! its header names (exactly as `verify` does, every recorded hash
//! checked), feed `sim::MatchStats` after every step, and print the
//! `docs/PLAYTEST.md` match-log row, a statistics table and the verify
//! line. A logged match can so be re-derived from its replay at any time.
//! The row and the table are `ai::match_log::MatchReport`'s, which the
//! game's `match_log:` line shares.

pub use ai::match_log::MatchReport;
use sim::{ReplayError, VerifyOutcome};
use std::path::Path;

/// What one re-simulation of a replay gathered: the report and the verify
/// outcome. Derefs to the [`MatchReport`].
pub struct MatchLog {
    /// The statistics and the header.
    pub report: MatchReport,
    /// The verify outcome.
    pub outcome: VerifyOutcome,
}

impl std::ops::Deref for MatchLog {
    type Target = MatchReport;
    fn deref(&self) -> &MatchReport {
        &self.report
    }
}

impl MatchLog {
    /// The row with the verify outcome in its Verify cell (`OK 0x...`).
    pub fn row(&self, date: &str, commit: &str, replay_name: &str) -> String {
        self.report
            .row(date, commit, replay_name, &verify_cell(&self.outcome))
    }
}

/// The Verify cell for `outcome`: `OK 0x<hash>`, else the outcome's line.
pub fn verify_cell(outcome: &VerifyOutcome) -> String {
    match outcome {
        VerifyOutcome::Ok { final_hash, .. } => format!("OK 0x{final_hash:016x}"),
        other => other.to_string(),
    }
}

/// Re-simulate `replay` against `data` (`ai::match_log::collect`).
/// `Ok(Err(outcome))` is a version or rules mismatch, where nothing was
/// simulated.
pub fn collect(replay: &Path, data: &Path) -> Result<Result<MatchLog, VerifyOutcome>, ReplayError> {
    Ok(ai::match_log::collect(replay, data)?.map(|(report, outcome)| MatchLog { report, outcome }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::harness::BotMatch;
    use sim::{MatchSetup, MatchStats, PlayerId, Rules};
    use std::path::PathBuf;

    fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    /// `match-log` re-derives exactly the statistics the play-bots
    /// harness observed for the same match (4400 ticks of Hard against
    /// Easy, written as a replay with no commands and one final hash), and
    /// verifies the replay.
    #[test]
    fn match_log_stats_equal_the_harness() {
        let rules = Rules::load(data()).unwrap();
        let setup = MatchSetup::bots(&rules, 3, ("hard", "standard"), ("easy", "standard"));
        let mut m = BotMatch::with_bots(setup.clone(), rules.clone()).unwrap();
        m.run(4400);
        let dir = std::env::temp_dir().join(format!("eonmark-match-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hard_easy.eonreplay");
        let mut w = sim::ReplayWriter::create(&path, &setup).unwrap();
        w.hash(m.sim.tick(), m.sim.hash(), &m.sim.sub_hashes())
            .unwrap();
        w.finish().unwrap();
        let log = collect(&path, &data()).unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            log.outcome,
            VerifyOutcome::Ok {
                final_hash: m.sim.hash(),
                ticks: 4400
            }
        );
        assert_eq!(&log.stats, m.observer.stats());
        assert!(log.stats.player(PlayerId(0)).bonus_total() > 0);
        assert_eq!(log.me(), PlayerId(0), "no human slot: P0");
        let row = log.row("2026-10-28", "abc1234", "hard_easy.eonreplay");
        assert!(
            row.starts_with("| 2026-10-28 | abc1234 | 3 | Easy | standard | "),
            "{row}"
        );
        assert!(row.contains(" | 3.6 | undecided | n | n 100% | "), "{row}");
        assert!(
            row.ends_with(&format!(
                " | | | hard_easy.eonreplay | OK 0x{:016x} |",
                m.sim.hash()
            )),
            "{row}"
        );
        assert_eq!(row.matches('|').count(), 16);
        let table = log.table();
        assert!(table[0].starts_with("| Stat | You (P0) | Easy standard (P1) |"));
        assert_eq!(table.len(), 2 + 11 + 3);
    }

    /// The replay of a human-against-bot match carries the human's
    /// commands as `Tick` records, and verify steps those ticks through its
    /// own branch. `match-log` must observe those steps too: the statistics
    /// it re-derives equal a `MatchStats` fed after every step of the
    /// recorded run, past first contact and the human's first losses, with
    /// commands on ticks whose next step is a 20-tick sample.
    #[test]
    fn match_log_stats_equal_the_recorded_run_with_human_commands() {
        let rules = Rules::load(data()).unwrap();
        let setup = MatchSetup::vs_bot(&rules, 5, "hard", "standard");
        let (worker, _) = rules::ai::worker_kind(&rules).unwrap();
        let mut sim = sim::Sim::new(
            setup.clone(),
            rules.clone(),
            ai::controller_for(&setup, &rules).unwrap(),
        );
        let p0 = PlayerId(0);
        let seat = sim::BuildingId(1);
        let enemy_seat = {
            let view = sim.view();
            let tile = view
                .buildings()
                .find(|b| b.owner == PlayerId(1))
                .map(|b| b.tile)
                .unwrap();
            view.map().center_of(tile)
        };
        let dir = std::env::temp_dir().join(format!("eonmark-match-log-h-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("human_hard.eonreplay");
        let mut w = sim::ReplayWriter::create(&path, &setup).unwrap();
        let mut stats = MatchStats::new(&setup, &rules);
        stats.observe(&sim.view(), &[]);
        let mut seq = 0u32;
        let mut command_ticks = Vec::new();
        for _ in 0..4000 {
            let t = sim.tick();
            let mut cmds = Vec::new();
            if t == 300 {
                // Two Yeomen walk to the enemy Seat: its shots are first
                // contact, and they die.
                cmds.push(sim::Command::Move {
                    units: vec![sim::UnitId(4), sim::UnitId(5)],
                    target: enemy_seat,
                    queue: false,
                });
            }
            if t % 1000 == 19 || t == 0 {
                cmds.push(sim::Command::Train {
                    building: seat,
                    kind: sim::UnitKindId(worker),
                });
            }
            let cmds: Vec<sim::PlayerCommand> = cmds
                .into_iter()
                .map(|c| {
                    seq += 1;
                    sim::PlayerCommand::new(p0, seq, c)
                })
                .collect();
            if !cmds.is_empty() {
                command_ticks.push(t);
            }
            w.tick(t, &cmds).unwrap();
            sim.step(&cmds);
            let events = sim.drain_events();
            stats.observe(&sim.view(), &events);
        }
        w.hash(sim.tick(), sim.hash(), &sim.sub_hashes()).unwrap();
        w.finish().unwrap();
        let log = collect(&path, &data()).unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            log.outcome,
            VerifyOutcome::Ok {
                final_hash: sim.hash(),
                ticks: 4000
            }
        );
        assert!(command_ticks.iter().filter(|t| *t % 20 == 19).count() >= 3);
        assert!(stats.first_contact_tick().is_some(), "contact");
        assert!(stats.player(p0).units_lost_total() > 0, "the Yeomen fell");
        assert!(stats.player(p0).units_trained_total() > 0);
        assert_eq!(log.me(), p0);
        assert_eq!(log.stats, stats);
    }
}
