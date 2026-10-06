//! Scripted scenarios shared by `record`, `bench` and `fuzz`.
//!
//! A scenario is a pure function of `(tick, &Sim)` to the commands issued
//! on that tick, so a recording and a test that replay the same scenario
//! agree command for command. The definitions mirror the helpers in
//! `crates/sim/tests/m1.rs` (`move_500_commands`, the snapshot/restore
//! test shape); the fixtures under `crates/sim/tests/fixtures/` are
//! recorded from here with `sim-cli record --scenario <name>`.
//!
//! Every scenario runs on [`MatchSetup::scenario`] (two human slots, debug
//! commands accepted) with [`ai::Passive`] in the AI seat.

use rand_core::{Rng, SeedableRng};
use sim::{Command, FxVec2, MatchSetup, PlayerCommand, PlayerId, Rules, Sim, UnitId, UnitKindId};

/// Player 0's start tile centre on `plains_1v1`.
pub const WEST: FxVec2 = FxVec2::from_ints(24, 64);
/// Player 1's start tile centre on `plains_1v1`.
pub const EAST: FxVec2 = FxVec2::from_ints(103, 64);
/// A point in the river (impassable) north of the central ford: the Move
/// target is corrected to the nearest passable tile and the spiral offsets
/// skip water tiles.
pub const RIVER: FxVec2 = FxVec2::from_ints(63, 40);

/// The Yeoman, the only unit kind in M1.
pub const YEOMAN: UnitKindId = UnitKindId(0);

/// The tick at which `snapshot_restore` snapshots and swaps sims.
pub const SNAPSHOT_TICK: u32 = 300;

/// A scripted command stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// 500 Yeomen at the west start on tick 0, Move to the east start on
    /// tick 5, Stop for every third unit on tick 400; 1200 ticks.
    Move500,
    /// `Move500` cut to 600 ticks (the `cargo test` golden).
    Move500Short,
    /// 64 Yeomen ordered into the river on tick 5 (goal correction and
    /// blocked-tile skipping in the spiral) and on to the east start on
    /// tick 300; 600 ticks.
    GroupSpiral,
    /// `Move500` with a snapshot/restore swap before tick 300 and 50 units
    /// spawned for player 1 at the east start on tick 300; 1200 ticks.
    /// The recorded hashes come from the restored sim, so `verify`
    /// (uninterrupted) passing proves the restore was exact.
    SnapshotRestore,
    /// Spawns, Stops and not-yet-implemented commands only: runs on the M1
    /// contract sim before pathing and movement land. 400 ticks.
    Selftest,
    /// `units` Yeomen crossing west to east with no Stop (the `bench` stream).
    Crossing {
        /// Units to spawn.
        units: u16,
    },
}

impl Scenario {
    /// Names accepted by `--scenario`.
    pub const NAMES: [&'static str; 5] = [
        "move_500",
        "move_500_short",
        "group_spiral",
        "snapshot_restore",
        "selftest",
    ];

    /// Parse a `--scenario` name.
    pub fn parse(name: &str) -> Option<Scenario> {
        Some(match name {
            "move_500" => Scenario::Move500,
            "move_500_short" => Scenario::Move500Short,
            "group_spiral" => Scenario::GroupSpiral,
            "snapshot_restore" => Scenario::SnapshotRestore,
            "selftest" => Scenario::Selftest,
            _ => return None,
        })
    }

    /// The tick count the fixture of this scenario is recorded with.
    pub fn default_ticks(self) -> u32 {
        match self {
            Scenario::Move500 | Scenario::SnapshotRestore | Scenario::Crossing { .. } => 1200,
            Scenario::Move500Short | Scenario::GroupSpiral => 600,
            Scenario::Selftest => 400,
        }
    }

    /// Commands issued during `tick`, given the sim as it stands before the
    /// step (unit ids are read from the view).
    pub fn commands(self, sim: &Sim, tick: u32) -> Vec<PlayerCommand> {
        match self {
            Scenario::Move500 | Scenario::Move500Short => move_500(sim, tick, 500),
            Scenario::SnapshotRestore => {
                if tick == SNAPSHOT_TICK {
                    vec![spawn(1, 0, EAST, 50)]
                } else {
                    move_500(sim, tick, 500)
                }
            }
            Scenario::GroupSpiral => match tick {
                0 => vec![spawn(0, 0, WEST, 64)],
                5 => vec![move_all(0, 1, sim, RIVER)],
                300 => vec![move_all(0, 2, sim, EAST)],
                _ => Vec::new(),
            },
            Scenario::Crossing { units } => match tick {
                0 => vec![spawn(0, 0, WEST, units)],
                5 => vec![move_all(0, 1, sim, EAST)],
                _ => Vec::new(),
            },
            Scenario::Selftest => selftest(tick),
        }
    }

    /// Side effects on the recording sim before the step at `tick`. Only
    /// `snapshot_restore` has one: at [`SNAPSHOT_TICK`] it snapshots, builds
    /// a fresh sim with `fresh`, restores into it and continues on that sim.
    /// Returns `true` when a swap happened.
    pub fn before_step(self, sim: &mut Sim, tick: u32, fresh: impl FnOnce() -> Sim) -> bool {
        if self != Scenario::SnapshotRestore || tick != SNAPSHOT_TICK {
            return false;
        }
        let snap = sim.snapshot();
        let mut restored = fresh();
        restored
            .restore(&snap)
            .expect("a snapshot of the same setup restores");
        assert_eq!(
            restored.hash(),
            sim.hash(),
            "restore must reproduce the hash at tick {tick}"
        );
        *sim = restored;
        true
    }
}

/// A sim for a scenario: [`MatchSetup::scenario`] with a passive AI seat.
pub fn build(rules: &Rules, seed: u64) -> Sim {
    Sim::new(
        MatchSetup::scenario(rules, seed),
        rules.clone(),
        Box::new(ai::Passive),
    )
}

/// `DebugSpawn` of `count` Yeomen for `owner` around `at`.
pub fn spawn(owner: u8, seq: u32, at: FxVec2, count: u16) -> PlayerCommand {
    PlayerCommand::new(
        PlayerId(owner),
        seq,
        Command::DebugSpawn {
            owner: PlayerId(owner),
            kind: YEOMAN,
            at,
            count,
        },
    )
}

/// Move every unit `owner` has to `target`.
pub fn move_all(owner: u8, seq: u32, sim: &Sim, target: FxVec2) -> PlayerCommand {
    let units = sim
        .view()
        .units()
        .filter(|u| u.owner == PlayerId(owner))
        .map(|u| u.id)
        .collect();
    PlayerCommand::new(
        PlayerId(owner),
        seq,
        Command::Move {
            units,
            target,
            queue: false,
        },
    )
}

/// The `move_500_commands` stream of `crates/sim/tests/m1.rs`, sized to
/// `units`: spawn at the west start on tick 0, Move to the east start on
/// tick 5, Stop every third unit on tick 400.
fn move_500(sim: &Sim, tick: u32, units: u16) -> Vec<PlayerCommand> {
    match tick {
        0 => vec![spawn(0, 0, WEST, units)],
        5 => vec![move_all(0, 1, sim, EAST)],
        400 => {
            let units = sim
                .view()
                .units()
                .filter(|u| u.id.0.is_multiple_of(3))
                .map(|u| u.id)
                .collect();
            vec![PlayerCommand::new(PlayerId(0), 2, Command::Stop { units })]
        }
        _ => Vec::new(),
    }
}

/// Spawns for both players on tick 0, a not-yet-implemented command every 7
/// ticks (counted and rejected, advancing the rng), a Stop every 50 ticks,
/// an unknown unit kind on tick 100 and an unknown player on tick 200.
/// No Move, so the stream runs before pathing and movement exist.
fn selftest(tick: u32) -> Vec<PlayerCommand> {
    let mut out = Vec::new();
    match tick {
        0 => {
            out.push(spawn(0, 0, WEST, 8));
            out.push(spawn(1, 0, EAST, 8));
        }
        100 => out.push(PlayerCommand::new(
            PlayerId(0),
            100,
            Command::DebugSpawn {
                owner: PlayerId(0),
                kind: UnitKindId(99),
                at: WEST,
                count: 1,
            },
        )),
        200 => out.push(PlayerCommand::new(
            PlayerId(7),
            0,
            Command::Stop {
                units: vec![UnitId(1)],
            },
        )),
        _ => {}
    }
    if tick % 7 == 3 {
        let player = u8::try_from(tick % 2).expect("0 or 1");
        out.push(PlayerCommand::new(
            PlayerId(player),
            tick,
            Command::AttackMove {
                units: vec![UnitId(1 + tick % 16)],
                target: FxVec2::from_ints(64, 64),
            },
        ));
    }
    if tick % 50 == 25 {
        out.push(PlayerCommand::new(
            PlayerId(0),
            tick,
            Command::Stop {
                units: vec![UnitId(1 + tick % 8)],
            },
        ));
    }
    out
}

/// A seeded random command stream for `fuzz`: valid spawns, Moves and
/// Stops for player 0, and invalid commands of every rejectable kind
/// (unknown player, no valid units, unknown unit kind, not implemented,
/// and everything player 1 issues after it surrenders). Returns the
/// per-tick commands and the `(player, seq)` keys the sim must reject.
/// `movement` enables Move commands (which need the pathing and movement
/// bodies; the M1 contract sim panics on them).
pub fn random_commands(
    seed: u64,
    ticks: u32,
    movement: bool,
) -> (Vec<Vec<PlayerCommand>>, Vec<(PlayerId, u32)>) {
    let mut rng = rand_pcg::Pcg32::seed_from_u64(seed);
    let mut stream = Vec::with_capacity(ticks as usize);
    let mut rejected = Vec::new();
    let mut seq = [0u32; 2];
    let mut spawned: u32 = 0;
    let mut surrendered = false;
    let point = |rng: &mut rand_pcg::Pcg32| {
        let x = i32::try_from(rng.next_u32() % 128).expect("fits");
        let y = i32::try_from(rng.next_u32() % 128).expect("fits");
        FxVec2::from_ints(x, y)
    };
    for _ in 0..ticks {
        let mut cmds = Vec::new();
        let roll = rng.next_u32() % 100;
        // Player 0: valid commands.
        if roll < 10 && spawned < 300 {
            let count = u16::try_from(1 + rng.next_u32() % 20).expect("fits");
            spawned += u32::from(count);
            let at = point(&mut rng);
            cmds.push(spawn(0, seq[0], at, count));
            seq[0] += 1;
        } else if roll < 40 && spawned > 0 && movement {
            let n = 1 + rng.next_u32() % 12;
            let units = (0..n)
                .map(|_| UnitId(1 + rng.next_u32() % spawned))
                .collect();
            let target = point(&mut rng);
            cmds.push(PlayerCommand::new(
                PlayerId(0),
                seq[0],
                Command::Move {
                    units,
                    target,
                    queue: false,
                },
            ));
            seq[0] += 1;
        } else if roll < 45 && spawned > 0 {
            let units = vec![UnitId(1 + rng.next_u32() % spawned)];
            cmds.push(PlayerCommand::new(
                PlayerId(0),
                seq[0],
                Command::Stop { units },
            ));
            seq[0] += 1;
        }
        // Invalid commands, each rejected for a known reason.
        let bad = rng.next_u32() % 100;
        if bad < 8 {
            // UnknownPlayer.
            cmds.push(PlayerCommand::new(
                PlayerId(7),
                0,
                Command::Stop {
                    units: vec![UnitId(1)],
                },
            ));
            rejected.push((PlayerId(7), 0));
        } else if bad < 16 {
            // NoValidUnits: UnitId(0) is never allocated.
            cmds.push(PlayerCommand::new(
                PlayerId(0),
                seq[0],
                Command::Stop {
                    units: vec![UnitId(0)],
                },
            ));
            rejected.push((PlayerId(0), seq[0]));
            seq[0] += 1;
        } else if bad < 24 {
            // UnknownUnitKind.
            let at = point(&mut rng);
            cmds.push(PlayerCommand::new(
                PlayerId(0),
                seq[0],
                Command::DebugSpawn {
                    owner: PlayerId(0),
                    kind: UnitKindId(99),
                    at,
                    count: 1,
                },
            ));
            rejected.push((PlayerId(0), seq[0]));
            seq[0] += 1;
        } else if bad < 36 {
            // NotImplemented (counted, advances the rng).
            let target = point(&mut rng);
            cmds.push(PlayerCommand::new(
                PlayerId(0),
                seq[0],
                Command::AttackMove {
                    units: vec![UnitId(1)],
                    target,
                },
            ));
            rejected.push((PlayerId(0), seq[0]));
            seq[0] += 1;
        } else if bad < 40 {
            // Player 1 owns nothing: NoValidUnits, or Surrendered afterwards.
            cmds.push(PlayerCommand::new(
                PlayerId(1),
                seq[1],
                Command::Stop {
                    units: vec![UnitId(1)],
                },
            ));
            rejected.push((PlayerId(1), seq[1]));
            seq[1] += 1;
        } else if bad < 41 && !surrendered {
            // The surrender itself is applied; everything after it is not.
            cmds.push(PlayerCommand::new(PlayerId(1), seq[1], Command::Surrender));
            seq[1] += 1;
            surrendered = true;
        }
        stream.push(cmds);
    }
    (stream, rejected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_parse_and_default_ticks_match_the_roadmap() {
        for name in Scenario::NAMES {
            let s = Scenario::parse(name).unwrap();
            assert!(s.default_ticks() > 0);
        }
        assert_eq!(Scenario::parse("nope"), None);
        assert_eq!(Scenario::Move500.default_ticks(), 1200);
        assert!(Scenario::Move500Short.default_ticks() <= 5000);
    }

    #[test]
    fn selftest_stream_has_no_move() {
        for t in 0..400 {
            for pc in selftest(t) {
                assert!(!matches!(pc.cmd, Command::Move { .. }), "tick {t}");
            }
        }
        assert_eq!(selftest(0).len(), 2);
        assert!(selftest(100).iter().any(|c| matches!(
            c.cmd,
            Command::DebugSpawn {
                kind: UnitKindId(99),
                ..
            }
        )));
    }

    #[test]
    fn random_commands_are_repeatable_and_track_rejections() {
        let (a, ra) = random_commands(5, 200, false);
        let (b, rb) = random_commands(5, 200, false);
        assert_eq!(a, b);
        assert_eq!(ra, rb);
        assert!(!ra.is_empty());
        assert!(
            a.iter()
                .flatten()
                .any(|c| matches!(c.cmd, Command::DebugSpawn { .. }))
        );
        let (c, _) = random_commands(6, 200, false);
        assert_ne!(a, c);
    }
}
