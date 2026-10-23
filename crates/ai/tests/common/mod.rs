//! The bot testkit (M5b): an [`Arena`] holds a real `Sim` with chosen AI
//! seats and debug setup commands, and hands out a live plan context or a
//! plan of the Standard bot without stepping. Shared by the `crates/ai`
//! integration tests (`mod common;`).
#![allow(dead_code)]

use ai::standard::{Ctx, MapAnalysis, StandardBot};
use rules::Rules;
use sim::{AiMemory, Command, MatchSetup, PlayerCommand, PlayerId, Sim};
use std::path::{Path, PathBuf};

/// The repository's `data/`.
pub fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// The repository's rules.
pub fn rules() -> Rules {
    Rules::load(data_dir()).expect("data/ loads")
}

/// A match under test.
pub struct Arena {
    /// The sim, with the header's own bots (`ai::controller_for`).
    pub sim: Sim,
    /// A Standard bot per seat whose header names one (for `ctx` and
    /// `plan`), `None` for other seats.
    pub bots: Vec<Option<StandardBot>>,
}

impl Arena {
    /// A match of `setup` with its header's bots. `setup_commands` are
    /// applied on tick 0 (issued by `PlayerId(0)` with sequence numbers
    /// from `1 << 30`, so they never meet a bot's own), which needs
    /// `setup.debug_commands` for debug commands.
    pub fn new(setup: MatchSetup, setup_commands: Vec<Command>) -> Arena {
        let rules = rules();
        let bots = setup
            .players
            .iter()
            .map(|slot| {
                let seat = setup.ai_seats.iter().find(|s| s.player == slot.id)?;
                (seat.bot == ai::BOT_STANDARD).then(|| {
                    let knobs = rules
                        .ai
                        .personality(&seat.personality)
                        .expect("personality");
                    StandardBot::new(&rules, &setup, slot.id, knobs)
                })
            })
            .collect();
        let controller = ai::controller_for(&setup, &rules).expect("the header's bots build");
        let mut sim = Sim::new(setup, rules, controller);
        let stamped: Vec<PlayerCommand> = setup_commands
            .into_iter()
            .enumerate()
            .map(|(i, c)| PlayerCommand::new(PlayerId(0), (1 << 30) + u32::try_from(i).unwrap(), c))
            .collect();
        sim.step(&stamped);
        Arena { sim, bots }
    }

    fn bot(&self, me: PlayerId) -> &StandardBot {
        self.bots[usize::from(me.0)]
            .as_ref()
            .expect("the seat is a standard bot")
    }

    /// The static map analysis of `me`'s Standard bot.
    pub fn map(&self, me: PlayerId) -> &MapAnalysis {
        self.bot(me).map()
    }

    /// A live plan context for `me` at the current tick.
    pub fn ctx(&self, me: PlayerId) -> Ctx<'_> {
        let view = self.sim.view();
        let knobs = self.bot(me).knobs(view.rules());
        Ctx::new(view, me, knobs, self.bot(me).map())
    }

    /// What `me`'s Standard bot would plan now, whatever its cadence, and
    /// the memory it would leave; nothing is stepped.
    pub fn plan(&self, me: PlayerId) -> (Vec<Command>, AiMemory) {
        let view = self.sim.view();
        let mut memory = AiMemory::default();
        memory.set(view.ai_memory(me).to_vec());
        let cmds = self.bot(me).plan(me, &view, &mut memory);
        (cmds, memory)
    }

    /// Step `n` ticks with no human input.
    pub fn run(&mut self, n: u32) {
        for _ in 0..n {
            self.sim.step(&[]);
        }
    }
}
