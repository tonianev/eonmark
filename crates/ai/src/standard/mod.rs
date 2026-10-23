//! The Standard bot (M5b): the single v0.1 opponent.
//!
//! Each call of [`StandardBot::think`] is either an off-cadence call, which
//! returns at once, or a plan: decode the memory, gather the [`Facts`], run
//! the rules over one [`Ctx`] in a fixed order, write the memory back if it
//! changed, and return the plan's commands. A bot plans on ticks where
//! `tick % every == player × every / players`, `every` being the
//! personality's `think_every_ds` in ticks (more than the command delay,
//! so a plan's commands apply before the next plan reads the view), so two
//! bots never plan on the same tick.
//!
//! M5b slice 1 is the skeleton: the cadence gate, the memory codec, the
//! facts, the plan context and the static map analysis, and no rule yet,
//! so a plan issues nothing. The pipeline (defend, build order, economy,
//! production, waves, rally upkeep) fills in from slice 2 on.

pub mod build_order;
pub mod ctx;
pub mod facts;
pub mod memory;
pub mod sites;

pub use ctx::{Ctx, Ledger};
pub use facts::Facts;
pub use memory::{BotMemory, BotStats, stats};
pub use sites::MapAnalysis;

use rules::{Personality, Rules};
use sim::{AiController, AiMemory, Command, MatchSetup, PlayerId, SimView};

/// The Standard bot of one seat. Its only state besides the hashed memory
/// is the [`MapAnalysis`], a pure function of immutable inputs, so a bot
/// rebuilt mid-match decides exactly as the one it replaces.
#[derive(Debug, Clone)]
pub struct StandardBot {
    player: PlayerId,
    personality: String,
    map: MapAnalysis,
}

/// The tick offset of `me`'s plans within the cadence: `me × every /
/// players`, so the seats of a match plan on different ticks.
pub fn phase(me: PlayerId, every: u32, setup: &MatchSetup) -> u32 {
    let players = u32::try_from(setup.players.len().max(1)).expect("at most 255 players");
    u32::from(me.0) * every / players
}

impl StandardBot {
    /// The bot for seat `player` with `personality` (a row of
    /// `rules.ai`).
    pub fn new(
        rules: &Rules,
        setup: &MatchSetup,
        player: PlayerId,
        personality: &Personality,
    ) -> StandardBot {
        StandardBot {
            player,
            personality: personality.id.clone(),
            map: MapAnalysis::new(rules, setup, player, personality),
        }
    }

    /// The seat this bot plays.
    pub fn player(&self) -> PlayerId {
        self.player
    }

    /// The bot's static map analysis.
    pub fn map(&self) -> &MapAnalysis {
        &self.map
    }

    /// The personality's knobs, looked up in the rules the sim runs on.
    ///
    /// # Panics
    ///
    /// If the rules have no such personality (`ai::controller_for` checks
    /// it before building the bot).
    pub fn knobs<'r>(&self, rules: &'r Rules) -> &'r Personality {
        rules
            .ai
            .personality(&self.personality)
            .expect("controller_for checked the personality")
    }

    /// One plan, whatever the tick: the pipeline over a fresh [`Ctx`],
    /// the memory written back when it changed, and the plan's commands.
    /// [`AiController::think`] calls it on the bot's cadence; tests call it
    /// directly.
    pub fn plan(&self, me: PlayerId, view: &SimView<'_>, memory: &mut AiMemory) -> Vec<Command> {
        let knobs = self.knobs(view.rules());
        let mem = BotMemory::decode(memory.bytes());
        let ctx = Ctx::new(*view, me, knobs, &self.map);
        // The rules run here in their fixed order from M5b slice 2 on:
        // defend, build order, economy, production, waves, rally upkeep.
        let bytes = mem.encode();
        if bytes != memory.bytes() {
            memory.set(bytes);
        }
        ctx.into_commands()
    }
}

impl AiController for StandardBot {
    fn think(&mut self, me: PlayerId, view: &SimView<'_>, memory: &mut AiMemory) -> Vec<Command> {
        if view.outcome().is_some() {
            return Vec::new();
        }
        let every = self
            .knobs(view.rules())
            .think_every_ticks(view.rules())
            .max(1);
        if view.tick % every != phase(me, every, view.setup()) {
            return Vec::new();
        }
        self.plan(me, view, memory)
    }
}
