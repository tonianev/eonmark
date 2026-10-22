//! M5a tests: attrition, supply, annexation and the match outcome
//! (`docs/design/attrition.md`, `docs/design/towns.md`).
//!
//! The rules-data slice (`rules_version` 18) adds the Watchtower's Harrying
//! I and II, side techs that count toward no age. Their tests are named
//! `tech_harrying_*`, so `cargo test -p sim tech` selects them with the M4a
//! tech tests (the filter matches test paths, not integration-test file
//! names).
//!
//! The tests run on `MatchSetup::scenario`, an empty start: with no Town
//! there is no income, so stockpiles are exact through `DebugGrant`, and
//! a Scriptorium's Lore is the only income where one is placed. Every
//! rejection is compared with a control sim that receives a rejected no-op
//! with the same `(player, seq)`, on every sub-hash (`rng` included) and
//! the whole hash, as in `m4a.rs`.

use rules::Rules;
use sim::{
    BuildingId, BuildingKindId, Command, MatchSetup, PlayerCommand, PlayerId, RejectReason, Sim,
    SimEvent, SimView, TechId, Tile, UnitId,
};
use std::path::{Path, PathBuf};

const P0: PlayerId = PlayerId(0);

struct NoAi;

impl sim::AiController for NoAi {
    fn think(&mut self, _player: PlayerId, _view: &SimView<'_>) -> Vec<Command> {
        Vec::new()
    }
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn load_rules() -> Rules {
    Rules::load(data_dir()).expect("data/ loads")
}

fn tech(rules: &Rules, id: &str) -> TechId {
    TechId(rules.tech_by_id(id).expect("tech in techs.ron").0)
}

fn res(rules: &Rules, id: &str) -> usize {
    rules.resource_index(id).expect("resource in resources.ron")
}

fn bkind(rules: &Rules, id: &str) -> BuildingKindId {
    BuildingKindId(
        rules
            .building_kind_by_id(id)
            .expect("kind in buildings.ron")
            .0,
    )
}

fn rejections(events: &[SimEvent]) -> Vec<RejectReason> {
    events
        .iter()
        .filter_map(|e| match e {
            SimEvent::CommandRejected { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect()
}

/// Issue `cmds` this tick, step until they have been applied, and return
/// every event since the last drain.
fn apply(sim: &mut Sim, cmds: Vec<PlayerCommand>) -> Vec<SimEvent> {
    let delay = sim.setup().cmd_delay;
    sim.step(&cmds);
    for _ in 0..delay {
        sim.step(&[]);
    }
    sim.drain_events()
}

/// A sim plus the next sequence number per player, so two sims built by
/// the same steps stamp the same `(player, seq)` keys.
struct M {
    sim: Sim,
    seq: [u32; 2],
}

impl M {
    /// The empty-start scenario (debug commands on).
    fn new(rules: Rules, seed: u64) -> M {
        let setup = MatchSetup::scenario(&rules, seed);
        M {
            sim: Sim::new(setup, rules, Box::new(NoAi)),
            seq: [0; 2],
        }
    }

    fn stamp(&mut self, p: u8, cmd: Command) -> PlayerCommand {
        let i = usize::from(p.min(1));
        let pc = PlayerCommand::new(PlayerId(p), self.seq[i], cmd);
        self.seq[i] += 1;
        pc
    }

    /// Apply `cmds` from player `p` (in this order) and return the events.
    fn run_as(&mut self, p: u8, cmds: Vec<Command>) -> Vec<SimEvent> {
        let pcs = cmds.into_iter().map(|c| self.stamp(p, c)).collect();
        apply(&mut self.sim, pcs)
    }

    fn run(&mut self, cmds: Vec<Command>) -> Vec<SimEvent> {
        self.run_as(0, cmds)
    }

    /// `run`, asserting that nothing was rejected.
    fn ok(&mut self, cmds: Vec<Command>) -> Vec<SimEvent> {
        let events = self.run(cmds);
        assert!(rejections(&events).is_empty(), "{events:?}");
        events
    }

    /// Step `n` ticks with no commands; the events since the last drain.
    fn step(&mut self, n: u32) -> Vec<SimEvent> {
        for _ in 0..n {
            self.sim.step(&[]);
        }
        self.sim.drain_events()
    }

    fn view(&self) -> SimView<'_> {
        self.sim.view()
    }

    fn stock(&self, p: PlayerId) -> Vec<i64> {
        self.view().economy(p).stock.clone()
    }
}

fn grant(resource: usize, amount: u32) -> Command {
    Command::DebugGrant {
        owner: P0,
        resource: u8::try_from(resource).unwrap(),
        amount,
    }
}

/// Grants of exactly `cost`, a vector in `resources.ron` order.
fn grants(cost: &[i64]) -> Vec<Command> {
    cost.iter()
        .enumerate()
        .filter(|(_, c)| **c > 0)
        .map(|(r, c)| grant(r, u32::try_from(*c).unwrap()))
        .collect()
}

fn place(kind: BuildingKindId, x: u16, y: u16, complete: bool) -> Command {
    Command::DebugPlaceBuilding {
        owner: P0,
        kind,
        tile: Tile::new(x, y),
        complete,
    }
}

fn research(building: u32, tech: TechId) -> Command {
    Command::Research {
        building: BuildingId(building),
        tech,
    }
}

fn debug_research(tech: TechId) -> Command {
    Command::DebugResearch { owner: P0, tech }
}

fn advance() -> Command {
    Command::AdvanceAge { player: P0 }
}

/// A rejected no-op: `UnitId(999)` never exists.
fn noop() -> Command {
    Command::Stop {
        units: vec![UnitId(999)],
    }
}

/// Build two sims with `setup`, apply `cmd` from `p` to one and a rejected
/// no-op with the same key to the other: `cmd` is rejected with `want`
/// and every sub-hash and the whole hash match the control.
fn rejected_without_change(
    name: &str,
    setup: &dyn Fn() -> M,
    p: u8,
    cmd: Command,
    want: RejectReason,
) {
    let mut sim = setup();
    let mut control = setup();
    let events = sim.run_as(p, vec![cmd]);
    let control_events = control.run_as(p, vec![noop()]);
    assert_eq!(rejections(&events), [want], "{name}: {events:?}");
    assert_eq!(rejections(&control_events).len(), 1, "{name}: control");
    assert_eq!(
        sim.sim.sub_hashes(),
        control.sim.sub_hashes(),
        "{name}: no state change, rng included"
    );
    assert_eq!(sim.sim.hash(), control.sim.hash(), "{name}");
}

/// Harrying I and II count toward no age (M5a rules data): with Letters I
/// and both Harrying techs the Masonry Age still needs a second
/// Scriptorium tech, and with four Scriptorium techs and both Harrying
/// techs the Charter Age still needs a fifth. Each rejection changes
/// nothing, `SimView::age_tech_count` and `check_advance_age` agree with
/// apply, and the missing Scriptorium tech opens the age.
#[test]
fn tech_harrying_does_not_count_toward_ages() {
    let rules = load_rules();
    let t = |id: &str| tech(&rules, id);
    let harrying = ["harrying_1", "harrying_2"];
    // An empty start that has researched `ids` (`DebugResearch` skips the
    // age check) and holds exactly the next age's cost after `masonry`
    // AdvanceAges.
    let after = |masonry: bool, ids: &[&str]| {
        let rules = rules.clone();
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        move || {
            let mut m = M::new(rules.clone(), 3);
            if masonry {
                let mut cmds = vec![
                    debug_research(tech(&rules, "letters_1")),
                    debug_research(tech(&rules, "trade_1")),
                ];
                cmds.extend(grants(&rules.amounts(&rules.ages[1].cost)));
                cmds.push(advance());
                m.ok(cmds);
                assert_eq!(m.view().age(P0), 1);
            }
            let next = usize::from(masonry) + 1;
            let mut cmds: Vec<Command> = ids
                .iter()
                .map(|id| debug_research(tech(&rules, id)))
                .collect();
            cmds.extend(grants(&rules.amounts(&rules.ages[next].cost)));
            m.ok(cmds);
            m
        }
    };
    let cases: [(&str, bool, Vec<&str>, u32, &str); 2] = [
        (
            "masonry",
            false,
            [&["letters_1"][..], &harrying].concat(),
            1,
            "trade_1",
        ),
        (
            "charter",
            true,
            [&["arms_1", "statecraft_1"][..], &harrying].concat(),
            4,
            "letters_2",
        ),
    ];
    for (age, masonry, ids, counting, missing) in cases {
        let setup = after(masonry, &ids);
        let m = setup();
        let researched = m.view().researched(P0).len();
        let required = rules.ages[usize::from(masonry) + 1].techs_required;
        assert_eq!(m.view().age_tech_count(P0), counting, "{age}");
        assert_eq!(researched, usize::try_from(counting).unwrap() + 2, "{age}");
        assert!(
            u32::try_from(researched).unwrap() >= required && counting < required,
            "{age}: the Harrying techs alone would meet the gate"
        );
        assert_eq!(
            m.view().check_advance_age(P0),
            Err(RejectReason::NotEnoughTechs),
            "{age}"
        );
        println!(
            "{age}: {researched} techs researched, {counting} count toward an age, {required} required -> NotEnoughTechs"
        );
        rejected_without_change(
            &format!("{age} with both Harrying techs"),
            &setup,
            0,
            advance(),
            RejectReason::NotEnoughTechs,
        );
        let mut m = setup();
        m.ok(vec![debug_research(t(missing))]);
        assert_eq!(m.view().age_tech_count(P0), counting + 1);
        assert!(m.view().check_advance_age(P0).is_ok(), "{age}");
        let events = m.ok(vec![advance()]);
        let reached = u8::from(masonry) + 1;
        assert!(events.contains(&SimEvent::AgeAdvanced {
            player: P0,
            age: reached
        }));
        assert_eq!(m.stock(P0), vec![0; 4], "{age}: paid exactly the cost");
    }
}

/// Harrying I is researched at a Watchtower from the Masonry Age and
/// Harrying II from the Charter Age (M5a rules data): rejected with
/// `AgeRequired` before, with `CannotResearchHere` at a Scriptorium and
/// `PrerequisiteMissing` out of order, each changing nothing; accepted at
/// the Watchtower, it pays the quoted cost and completes in its quoted
/// time (800 and 1200 ticks with no Letters research).
#[test]
fn tech_harrying_is_researched_at_a_watchtower_from_masonry() {
    let rules = load_rules();
    let t = |id: &str| tech(&rules, id);
    let (h1, h2) = (t("harrying_1"), t("harrying_2"));
    // `BuildingId(1)` a complete Scriptorium, (2) a complete Watchtower,
    // plenty of every resource; then each AdvanceAge after the Scriptorium
    // techs it needs (no Letters, so every quote is the base cost).
    let setup = |age: u8, extra: &[&str]| {
        let rules = rules.clone();
        let extra: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
        move || {
            let mut m = M::new(rules.clone(), 4);
            let mut cmds = vec![
                place(bkind(&rules, "scriptorium"), 4, 26, true),
                place(bkind(&rules, "watchtower"), 2, 26, true),
                grant(res(&rules, "grain"), 2000),
                grant(res(&rules, "lumber"), 2000),
                grant(res(&rules, "ore"), 2000),
                grant(res(&rules, "lore"), 999),
            ];
            let age_techs = ["arms_1", "trade_1", "statecraft_1", "arms_2", "trade_2"];
            let needed = rules.ages[usize::from(age)].techs_required;
            cmds.extend(
                age_techs[..usize::try_from(needed).unwrap()]
                    .iter()
                    .map(|id| debug_research(tech(&rules, id))),
            );
            cmds.extend((0..age).map(|_| advance()));
            cmds.extend(extra.iter().map(|id| debug_research(tech(&rules, id))));
            m.ok(cmds);
            assert_eq!(m.view().age(P0), age);
            m
        }
    };
    let cases = [
        (
            "Harrying I in the Hearth Age",
            0,
            vec![],
            research(2, h1),
            RejectReason::AgeRequired,
        ),
        (
            "Harrying I at a Scriptorium",
            1,
            vec![],
            research(1, h1),
            RejectReason::CannotResearchHere,
        ),
        (
            "Arms II at the Watchtower",
            1,
            vec![],
            research(2, t("arms_2")),
            RejectReason::CannotResearchHere,
        ),
        (
            "Harrying II before Harrying I",
            2,
            vec![],
            research(2, h2),
            RejectReason::PrerequisiteMissing,
        ),
        (
            "Harrying II in the Masonry Age",
            1,
            vec!["harrying_1"],
            research(2, h2),
            RejectReason::AgeRequired,
        ),
    ];
    for (name, age, extra, cmd, want) in cases {
        rejected_without_change(name, &setup(age, &extra), 0, cmd, want);
    }
    // Accepted at the Watchtower: Harrying I in Masonry, then Harrying II
    // in Charter, each deducting its quote and completing in its time.
    let mut m = setup(1, &[])();
    for (level, id, next_age) in [(1, h1, 2u8), (2, h2, 0)] {
        let def = rules.tech(id.0).unwrap();
        let (cost, ticks) = m.view().research_quote(P0, id);
        assert_eq!(cost, rules.amounts(&def.cost), "{}: the base cost", def.id);
        assert_eq!(ticks, rules.ticks_from_ds(def.research_time_ds));
        assert_eq!(
            m.view().check_research(P0, BuildingId(2), id),
            Ok((cost.clone(), ticks))
        );
        let counting = m.view().age_tech_count(P0);
        let before = m.stock(P0);
        m.ok(vec![research(2, id)]);
        let paid: Vec<i64> = before.iter().zip(m.stock(P0)).map(|(b, a)| b - a).collect();
        assert_eq!(paid, cost, "{}", def.id);
        // Applied (and given its first tick) in the last step of `ok`.
        let started = m.sim.tick() - 1;
        let events = m.step(ticks - 2);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SimEvent::ResearchCompleted { .. })),
            "{}",
            def.id
        );
        let events = m.step(1);
        assert_eq!(m.sim.tick() - 1, started + ticks - 1, "the ticks-th step");
        assert!(events.contains(&SimEvent::ResearchCompleted {
            player: P0,
            tech: id
        }));
        assert!(m.view().researched(P0).contains(&id));
        assert_eq!(m.view().age_tech_count(P0), counting, "a side tech");
        println!(
            "{} (level {level}) at the Watchtower in age {}: paid {paid:?}, completed after {ticks} ticks",
            def.name,
            m.view().age(P0)
        );
        if next_age == 2 {
            // Three more Scriptorium techs, five in all, none of them
            // Letters (whose rows would lower the next quote); the start's
            // grants pay the Charter Age.
            m.ok(vec![
                debug_research(t("statecraft_1")),
                debug_research(t("arms_2")),
                debug_research(t("trade_2")),
                advance(),
            ]);
            assert_eq!(m.view().age(P0), next_age);
        }
    }
}
