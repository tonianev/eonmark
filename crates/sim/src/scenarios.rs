//! Scripted M1 scenarios: pure, deterministic command streams shared by the
//! acceptance tests (`crates/sim/tests/m1.rs`), the criterion benches,
//! `sim-cli record` / `bench`, and the golden fixtures under
//! `crates/sim/tests/fixtures/`.
//!
//! A scenario is a [`MatchSetup`] (built with [`MatchSetup::scenario`], so
//! debug commands are accepted and both slots are human) plus a [`Stream`]:
//! `(tick, commands)` pairs sorted by tick, meaning "issue these commands
//! during that tick" (`Sim::step(&cmds)`), so they apply `cmd_delay` later.
//!
//! The streams are data, not functions of a running sim, so they name unit
//! ids directly. That relies on two documented invariants: a fresh `Sim`
//! hands out `UnitId(1)` first and ids are monotonic, so a `DebugSpawn` of
//! `n` units on an empty sim creates `UnitId(1..=n)`. Every stream is valid
//! only when fed to a fresh sim from tick 0.

use crate::ai_hook::{AiController, SimView};
use crate::command::{Command, PlayerCommand};
use crate::fx::FxVec2;
use crate::ids::{PlayerId, UnitId, UnitKindId};
use crate::state::{MatchSetup, Sim};
use rules::Rules;

/// A tick-stamped command stream, sorted by tick.
pub type Stream = Vec<(u32, Vec<PlayerCommand>)>;

/// Player 0's start on `plains_1v1`.
pub const WEST: FxVec2 = FxVec2::from_ints(24, 64);
/// Player 1's start on `plains_1v1`.
pub const EAST: FxVec2 = FxVec2::from_ints(103, 64);

/// Length of [`move_500`]: 500 Yeomen cross the map (80 tiles at 1.8
/// tiles/s is 890 ticks plus jams).
pub const MOVE_500_TICKS: u32 = 1200;
/// Length of [`move_500_short`]: the same stream cut for `cargo test`
/// (the Stop during tick 400 lies beyond it; the full [`move_500`] fixture
/// covers it in CI).
pub const MOVE_500_SHORT_TICKS: u32 = 300;
/// Length of [`group_spiral`]: the second order (during tick 300) is still
/// in progress at the end, which is enough to lock the target assignment.
pub const GROUP_SPIRAL_TICKS: u32 = 600;
/// Length of [`snapshot_restore`].
pub const SNAPSHOT_RESTORE_TICKS: u32 = 1200;
/// Tick at which [`snapshot_restore`] expects the snapshot to be taken
/// (before the extra spawn issued during tick [`SNAPSHOT_RESTORE_SPAWN_TICK`]).
pub const SNAPSHOT_RESTORE_SNAPSHOT_TICK: u32 = 300;
/// Tick during which [`snapshot_restore`] issues its second spawn.
pub const SNAPSHOT_RESTORE_SPAWN_TICK: u32 = 301;

/// Scenario names accepted by [`by_name`] and `sim-cli record --scenario`.
pub const NAMES: [&str; 4] = [
    "move_500",
    "move_500_short",
    "group_spiral",
    "snapshot_restore",
];

fn spawn(owner: u8, seq: u32, at: FxVec2, count: u16) -> PlayerCommand {
    PlayerCommand::new(
        PlayerId(owner),
        seq,
        Command::DebugSpawn {
            owner: PlayerId(owner),
            kind: UnitKindId(0),
            at,
            count,
        },
    )
}

fn ids(range: core::ops::RangeInclusive<u32>) -> Vec<UnitId> {
    range.map(UnitId).collect()
}

fn mv(owner: u8, seq: u32, units: Vec<UnitId>, target: FxVec2) -> PlayerCommand {
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

fn stop(owner: u8, seq: u32, units: Vec<UnitId>) -> PlayerCommand {
    PlayerCommand::new(PlayerId(owner), seq, Command::Stop { units })
}

/// The fixed M1 crossing: 500 Yeomen spawned at the west start during tick
/// 0 (ids 1..=500), ordered to the east start during tick 5, a Stop for
/// every id divisible by 3 during tick 400, then idle until
/// [`MOVE_500_TICKS`]. Seed 42.
pub fn move_500(rules: &Rules) -> (MatchSetup, Stream) {
    let setup = MatchSetup::scenario(rules, 42);
    let stream = vec![
        (0, vec![spawn(0, 0, WEST, 500)]),
        (5, vec![mv(0, 1, ids(1..=500), EAST)]),
        (
            400,
            vec![stop(
                0,
                2,
                (1..=500u32)
                    .filter(|i| i.is_multiple_of(3))
                    .map(UnitId)
                    .collect(),
            )],
        ),
    ];
    (setup, stream)
}

/// [`move_500`] cut at [`MOVE_500_SHORT_TICKS`]: the same setup, and the
/// same stream without the commands issued at or after the cut.
pub fn move_500_short(rules: &Rules) -> (MatchSetup, Stream) {
    let (setup, mut stream) = move_500(rules);
    stream.retain(|(t, _)| *t < MOVE_500_SHORT_TICKS);
    (setup, stream)
}

/// Group-order targets: 64 Yeomen spawned at the west start (ids 1..=64)
/// are ordered during tick 5 to the west map edge (0, 64), where the square
/// spiral is cut by the edge (24 tiles, about 270 ticks); during tick 300
/// they are ordered onto a mountain tile (34, 53), so the click is corrected
/// by the BFS fallback and the spiral skips the 5x5 blocked blob (about 36
/// tiles, 400 ticks, so the fixture ends mid-walk). Seed 7. Length
/// [`GROUP_SPIRAL_TICKS`].
pub fn group_spiral(rules: &Rules) -> (MatchSetup, Stream) {
    let setup = MatchSetup::scenario(rules, 7);
    let stream = vec![
        (0, vec![spawn(0, 0, WEST, 64)]),
        (5, vec![mv(0, 1, ids(1..=64), FxVec2::from_ints(0, 64))]),
        (300, vec![mv(0, 2, ids(1..=64), FxVec2::from_ints(34, 53))]),
    ];
    (setup, stream)
}

/// Snapshot/restore stream: 200 Yeomen (ids 1..=200) spawned at the west
/// start during tick 0 and ordered east during tick 5; the test snapshots
/// at tick [`SNAPSHOT_RESTORE_SNAPSHOT_TICK`]; during tick
/// [`SNAPSHOT_RESTORE_SPAWN_TICK`] player 1 spawns 50 more (ids 201..=250)
/// at the east start and orders them west during tick 310; every id
/// divisible by 5 stops during tick 600. Seed 7. Length
/// [`SNAPSHOT_RESTORE_TICKS`].
pub fn snapshot_restore(rules: &Rules) -> (MatchSetup, Stream) {
    let setup = MatchSetup::scenario(rules, 7);
    let stream = vec![
        (0, vec![spawn(0, 0, WEST, 200)]),
        (5, vec![mv(0, 1, ids(1..=200), EAST)]),
        (SNAPSHOT_RESTORE_SPAWN_TICK, vec![spawn(1, 0, EAST, 50)]),
        (310, vec![mv(1, 1, ids(201..=250), WEST)]),
        (
            600,
            vec![stop(
                0,
                2,
                (1..=200u32)
                    .filter(|i| i.is_multiple_of(5))
                    .map(UnitId)
                    .collect(),
            )],
        ),
    ];
    (setup, stream)
}

/// Look a scenario up by name: `(setup, stream, ticks)`.
pub fn by_name(name: &str, rules: &Rules) -> Option<(MatchSetup, Stream, u32)> {
    let (setup, stream, ticks) = match name {
        "move_500" => {
            let (s, c) = move_500(rules);
            (s, c, MOVE_500_TICKS)
        }
        "move_500_short" => {
            let (s, c) = move_500_short(rules);
            (s, c, MOVE_500_SHORT_TICKS)
        }
        "group_spiral" => {
            let (s, c) = group_spiral(rules);
            (s, c, GROUP_SPIRAL_TICKS)
        }
        "snapshot_restore" => {
            let (s, c) = snapshot_restore(rules);
            (s, c, SNAPSHOT_RESTORE_TICKS)
        }
        _ => return None,
    };
    Some((setup, stream, ticks))
}

/// The commands a stream issues during `tick` (empty for most ticks).
pub fn commands_at(stream: &Stream, tick: u32) -> &[PlayerCommand] {
    match stream.binary_search_by_key(&tick, |(t, _)| *t) {
        Ok(i) => &stream[i].1,
        Err(_) => &[],
    }
}

/// A controller for slots no scenario has: every scenario setup is two
/// human slots, so it is never consulted.
struct NoSlots;

impl AiController for NoSlots {
    fn think(&mut self, _player: PlayerId, _view: &SimView<'_>) -> Vec<Command> {
        Vec::new()
    }
}

/// Build the sim for a scenario and drive it through ticks `0..ticks`,
/// feeding `stream` with [`commands_at`]. Panics when `setup` has an AI slot.
pub fn run(setup: MatchSetup, rules: Rules, stream: &Stream, ticks: u32) -> Sim {
    assert!(
        setup.players.iter().all(|p| !p.is_ai),
        "scenarios have no AI slots"
    );
    let mut sim = Sim::new(setup, rules, Box::new(NoSlots));
    for t in 0..ticks {
        sim.step(commands_at(stream, t));
    }
    sim
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn rules() -> Rules {
        Rules::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).unwrap()
    }

    #[test]
    fn streams_are_sorted_and_names_resolve() {
        let rules = rules();
        for name in NAMES {
            let (setup, stream, ticks) = by_name(name, &rules).unwrap_or_else(|| panic!("{name}"));
            assert!(setup.debug_commands, "{name}");
            assert!(stream.windows(2).all(|w| w[0].0 < w[1].0), "{name}");
            assert!(stream.last().is_some_and(|(t, _)| *t < ticks), "{name}");
            assert!(!commands_at(&stream, 0).is_empty(), "{name} spawns at 0");
            assert!(commands_at(&stream, 1).is_empty(), "{name}");
        }
        assert!(by_name("nope", &rules).is_none());
    }

    #[test]
    fn group_spiral_second_click_is_on_a_blocked_tile() {
        // The scenario exists to exercise the BFS click correction and the
        // spiral's blocked-tile skipping; keep it honest if the map changes.
        let rules = rules();
        let map = crate::map::Map::from_def(rules.map(&rules.default_map).unwrap());
        let (_, stream) = group_spiral(&rules);
        let Command::Move { target, .. } = &commands_at(&stream, 300)[0].cmd else {
            panic!("second order is a Move");
        };
        assert!(!map.passable(map.tile_of(*target)), "click must be blocked");
        let Command::Move { target, .. } = &commands_at(&stream, 5)[0].cmd else {
            panic!("first order is a Move");
        };
        assert_eq!(map.tile_of(*target).x, 0, "first click is on the west edge");
    }

    #[test]
    fn move_500_ids_match_a_fresh_sim() {
        let rules = rules();
        let (setup, stream) = move_500(&rules);
        let delay = setup.cmd_delay;
        let sim = run(setup, rules, &stream, delay + 1);
        let ids: Vec<u32> = sim.view().units().map(|u| u.id.0).collect();
        assert_eq!(ids, (1..=500).collect::<Vec<u32>>());
    }
}
