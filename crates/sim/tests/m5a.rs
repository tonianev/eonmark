//! M5a tests: attrition, supply, annexation and the match outcome
//! (`docs/design/attrition.md`, `docs/design/towns.md`).
//!
//! The rules-data slice (`rules_version` 18) adds the Watchtower's Harrying
//! I and II, side techs that count toward no age. Their tests are named
//! `tech_harrying_*`, so `cargo test -p sim tech` selects them with the M4a
//! tech tests (the filter matches test paths, not integration-test file
//! names).
//!
//! The attrition slice (`SIM_VERSION` 15, `rules_version` 19) adds the
//! `attrition_*` tests, which `cargo test -p sim attrition` selects
//! (acceptance line 1), and the seeded random stream `m5a_attrition_*`.
//! They run with `cmd_delay = 0` ([`M::delay0`]): a command issued in a
//! step applies in that step, so the tick an assertion names is the
//! literal `sim.tick()` after the step, and the field is set up in step 0.
//!
//! The tests run on `MatchSetup::scenario`, an empty start: with no Town
//! there is no income, so stockpiles are exact through `DebugGrant`, and
//! a Scriptorium's Lore is the only income where one is placed. Every
//! rejection is compared with a control sim that receives a rejected no-op
//! with the same `(player, seq)`, on every sub-hash (`rng` included) and
//! the whole hash, as in `m4a.rs`.

use proptest::prelude::*;
use rand_core::{Rng, SeedableRng};
use rules::Rules;
use sim::{
    BuildingId, BuildingKindId, Command, Exposure, FxVec2, MatchSetup, PlayerCommand, PlayerId,
    RejectReason, Sim, SimEvent, SimView, TargetId, TechId, Tile, UnitId, UnitKindId,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

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

fn ukind(rules: &Rules, id: &str) -> UnitKindId {
    UnitKindId(
        u16::try_from(
            rules
                .units
                .iter()
                .position(|u| u.id == id)
                .expect("kind in units.ron"),
        )
        .unwrap(),
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

    /// [`M::new`] with `cmd_delay = 0`: a command issued in a step applies
    /// in that step (P0's before P1's), so `sim.tick()` after the step is
    /// the tick an acceptance line names.
    fn delay0(rules: Rules, seed: u64) -> M {
        let mut setup = MatchSetup::scenario(&rules, seed);
        setup.cmd_delay = 0;
        M {
            sim: Sim::new(setup, rules, Box::new(NoAi)),
            seq: [0; 2],
        }
    }

    /// Stamp `cmds` (issuing player, command) and step once; with delay 0
    /// they apply in this step. The step's events.
    fn step_with(&mut self, cmds: Vec<(u8, Command)>) -> Vec<SimEvent> {
        let pcs: Vec<PlayerCommand> = cmds.into_iter().map(|(p, c)| self.stamp(p, c)).collect();
        self.sim.step(&pcs);
        self.sim.drain_events()
    }

    /// A second sim restored from this one's snapshot, with the same
    /// sequence numbers: it continues exactly as this one would.
    fn clone_at_tick(&self) -> M {
        let mut m = M {
            sim: Sim::new(
                self.sim.setup().clone(),
                self.sim.rules().clone(),
                Box::new(NoAi),
            ),
            seq: self.seq,
        };
        m.sim.restore(&self.sim.snapshot()).expect("restores");
        m
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

// ---------------------------------------------------------------------------
// Attrition (slice 2): `cargo test -p sim attrition`.
// ---------------------------------------------------------------------------

/// P1's Town in the attrition field: complete, north-west tile (20,30), so
/// its centre tile is (21,31) and its tier-1 border (radius 20) owns every
/// tile with `dx² + dy² < 400` from there.
const FIELD_TOWN: (u16, u16) = (20, 30);

/// The intruder's tile: inside P1's border (dx 14), 12.15 tiles edge to
/// edge from the Town's footprint, beyond every unit's reach (6) and the
/// Town's range (7).
const INTRUDER: (i32, i32) = (35, 31);

fn spawn_for(owner: PlayerId, kind: UnitKindId, (x, y): (i32, i32)) -> Command {
    Command::DebugSpawn {
        owner,
        kind,
        at: FxVec2::from_ints(x, y),
        count: 1,
    }
}

fn place_for(owner: PlayerId, kind: BuildingKindId, (x, y): (u16, u16)) -> Command {
    Command::DebugPlaceBuilding {
        owner,
        kind,
        tile: Tile::new(x, y),
        complete: true,
    }
}

fn move_to(unit: UnitId, (x, y): (i32, i32)) -> Command {
    Command::Move {
        units: vec![unit],
        target: FxVec2::from_ints(x, y),
        queue: false,
    }
}

fn damage(target: TargetId, amount: u32) -> Command {
    Command::DebugDamage { target, amount }
}

/// The first `n` Harrying techs, as P1's `DebugResearch` commands.
fn harrying_for(rules: &Rules, owner: PlayerId, n: usize) -> Vec<(u8, Command)> {
    ["harrying_1", "harrying_2"][..n]
        .iter()
        .map(|id| {
            (
                owner.0,
                Command::DebugResearch {
                    owner,
                    tech: tech(rules, id),
                },
            )
        })
        .collect()
}

/// The attrition field, all in step 0 with delay 0: P0's Skirmisher at
/// [`INTRUDER`] (`UnitId(1)`), then `extra` in order (P0's commands apply
/// before P1's), then P1's complete Town at [`FIELD_TOWN`] and its first
/// `harrying` Harrying techs. Returns the sim after step 0 (`tick() == 1`)
/// and the step's events.
fn attrition_field(
    rules: &Rules,
    harrying: usize,
    extra: Vec<(u8, Command)>,
) -> (M, Vec<SimEvent>) {
    let mut m = M::delay0(rules.clone(), 5);
    let mut cmds = vec![(0, spawn_for(P0, ukind(rules, "skirmisher"), INTRUDER))];
    cmds.extend(extra);
    cmds.push((1, place_for(P1, bkind(rules, "town"), FIELD_TOWN)));
    cmds.extend(harrying_for(rules, P1, harrying));
    let events = m.step_with(cmds);
    assert!(rejections(&events).is_empty(), "{events:?}");
    assert_eq!(m.sim.tick(), 1);
    (m, events)
}

fn hp(m: &M, u: UnitId) -> i32 {
    m.view().unit_hp(u).expect("the unit is alive").0
}

fn exposed(m: &M, u: UnitId) -> u32 {
    m.view().unit(u).expect("the unit is alive").exposed_ticks
}

fn unit_tile(m: &M, u: UnitId) -> Tile {
    let view = m.view();
    view.map()
        .tile_of(view.unit(u).expect("the unit is alive").pos)
}

/// `(unit, damage)` of every `Attrition` event in `events`.
fn pulses(events: &[SimEvent]) -> Vec<(UnitId, i32)> {
    events
        .iter()
        .filter_map(|e| match e {
            SimEvent::Attrition { unit, damage } => Some((*unit, *damage)),
            _ => None,
        })
        .collect()
}

fn hits(events: &[SimEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, SimEvent::Hit { .. }))
        .count()
}

/// Ticks between pulses at Harrying I, read from the data: 64 with the
/// shipped 32 ds (the rules crate's `loads_repo_data` pins that). The
/// attrition tests derive their ticks, pulse counts and hit points from
/// it, so a data-only edit of `attrition_interval_ds`, the worked example
/// in `data/rules/README.md`, moves their numbers instead of failing them
/// (CONTRIBUTING.md: read a number the data controls from `Rules`).
fn interval1(rules: &Rules) -> u32 {
    rules.attrition_interval_ticks()
}

/// Ticks between pulses at Harrying II: half of [`interval1`], at least 1
/// (32 with the shipped data).
fn interval2(rules: &Rules) -> u32 {
    (interval1(rules) / 2).max(1)
}

/// How many pulses a unit exposed from step 0 takes by tick `t` at
/// `interval` (its counter is `t` then): `t / interval`, as an `i32`.
fn pulses_by(t: u32, interval: u32) -> i32 {
    i32::try_from(t / interval).unwrap()
}

/// Acceptance 1, first clause: an enemy unit in researched borders loses
/// exactly 1 HP at ticks 64, 128 and 192 (the multiples of [`interval1`]
/// with the shipped data). P0's Skirmisher stands inside P1's border from
/// step 0, P1 has Harrying I and both players are in the Hearth Age (gap
/// 0, 1 HP). Its hit points are 80 through tick 63, 79 at 64 to 127, 78 at
/// 128 to 191 and 77 at 192 to 200, with exactly three
/// `Attrition { damage: 1 }` events, in the steps that end at those ticks,
/// and no `Hit`.
#[test]
fn attrition_enemy_unit_in_researched_borders_loses_exactly_1_hp_at_ticks_64_128_192() {
    let rules = load_rules();
    let i = interval1(&rules);
    let end = 3 * i + 8;
    let u = UnitId(1);
    let (mut m, events) = attrition_field(&rules, 1, vec![]);
    let tile = unit_tile(&m, u);
    assert_eq!(m.view().territory().owner_at(tile), Some(P1), "{tile:?}");
    assert_eq!(m.view().harrying_level(P1), 1);
    assert_eq!(m.view().harrying_level(P0), 0);
    assert_eq!(
        m.view().attrition(u),
        Some(Exposure {
            owner: P1,
            interval_ticks: i,
            hp: 1
        })
    );
    assert!(pulses(&events).is_empty() && hits(&events) == 0);
    let mut hp_at = BTreeMap::from([(1, hp(&m, u))]);
    let mut landed = Vec::new();
    let mut hit_count = 0;
    while m.sim.tick() < end {
        let events = m.step(1);
        hit_count += hits(&events);
        for (unit, dmg) in pulses(&events) {
            landed.push((m.sim.tick(), unit, dmg));
        }
        hp_at.insert(m.sim.tick(), hp(&m, u));
    }
    for (&t, &h) in &hp_at {
        let want = 80 - i32::from(t >= i) - i32::from(t >= 2 * i) - i32::from(t >= 3 * i);
        assert_eq!(h, want, "hit points after tick {t}");
    }
    assert_eq!(landed, [(i, u, 1), (2 * i, u, 1), (3 * i, u, 1)]);
    assert_eq!(hit_count, 0, "nothing but attrition touched it");
    for t in [i - 1, i, 2 * i - 1, 2 * i, 3 * i - 1, 3 * i, end] {
        if let Some(h) = hp_at.get(&t) {
            println!("tick {t}: hp {h}");
        }
    }
    println!("Attrition events (tick, unit, damage): {landed:?}; Hit events: {hit_count}");
}

/// Acceptance 1, second clause: with a friendly Supply Wain within 14
/// tiles the unit loses 0 HP over 1200 ticks, and its counter never
/// leaves 0. The Wain stands 14 tiles south of the intruder's tile
/// (`dy² = 196`, inclusive); the control Wain 15 tiles south does not
/// cover it, and the intruder ends at 62 HP (18 pulses, `1200 /`
/// [`interval1`]). The Wain itself is immune.
#[test]
fn attrition_friendly_supply_wain_within_14_tiles_means_0_hp_lost_over_1200_ticks() {
    let rules = load_rules();
    let (u, w) = (UnitId(1), UnitId(2));
    let wain = ukind(&rules, "supply_wain");
    let n = pulses_by(1200, interval1(&rules));
    let cases = [
        ((35, 45), true, 80, 0),
        ((35, 46), false, 80 - n, usize::try_from(n).unwrap()),
    ];
    for (at, covered, want_hp, want_pulses) in cases {
        let (mut m, _) = attrition_field(&rules, 1, vec![(0, spawn_for(P0, wain, at))]);
        let at_tile = Tile::new(u16::try_from(at.0).unwrap(), u16::try_from(at.1).unwrap());
        assert_eq!(m.view().supply_radius_tiles(), 14);
        assert_eq!(m.view().supply_wains(P0), [at_tile]);
        assert_eq!(m.view().supplied(P0, unit_tile(&m, u)), covered, "{at:?}");
        assert_eq!(m.view().attrition(u).is_none(), covered);
        let mut landed = 0;
        while m.sim.tick() < 1200 {
            let events = m.step(1);
            landed += pulses(&events).len();
            if covered {
                assert_eq!((hp(&m, u), exposed(&m, u)), (80, 0), "{}", m.sim.tick());
            }
            assert_eq!(exposed(&m, w), 0, "the Wain is immune");
        }
        assert_eq!((hp(&m, u), landed), (want_hp, want_pulses), "{at:?}");
        assert_eq!(hp(&m, w), 150);
        println!(
            "Wain at {at:?} ({}): hp {} after {} ticks, {landed} pulses",
            if covered { "covers" } else { "does not cover" },
            hp(&m, u),
            m.sim.tick()
        );
    }
}

/// Acceptance 1, third clause: before Harrying I an enemy unit loses 0 HP:
/// with no Harrying tech P1's level is 0 and the intruder stays at 80 HP
/// with a counter of 0 for 1200 ticks, with no event. Harrying I
/// researched during tick 600 starts the exposure there: the first pulse
/// lands at tick 664 (600 + [`interval1`]).
#[test]
fn attrition_before_harrying_one_an_enemy_unit_loses_0_hp() {
    let rules = load_rules();
    let due = 600 + interval1(&rules);
    let u = UnitId(1);
    let (mut m, _) = attrition_field(&rules, 0, vec![]);
    assert_eq!(m.view().harrying_level(P1), 0);
    assert_eq!(m.view().attrition_pulse(P1, P0), None);
    assert_eq!(m.view().attrition(u), None);
    let mut quiet = m.clone_at_tick();
    while m.sim.tick() < 1200 {
        let events = m.step(1);
        assert!(pulses(&events).is_empty() && hits(&events) == 0);
        assert_eq!((hp(&m, u), exposed(&m, u)), (80, 0), "{}", m.sim.tick());
    }
    println!("no Harrying: hp {} after {} ticks", hp(&m, u), m.sim.tick());
    // The same field, researching Harrying I during tick 600.
    quiet.step(600 - quiet.sim.tick());
    assert_eq!(quiet.sim.tick(), 600);
    let events = quiet.step_with(harrying_for(&rules, P1, 1));
    assert!(rejections(&events).is_empty(), "{events:?}");
    assert_eq!(quiet.view().harrying_level(P1), 1);
    assert_eq!(exposed(&quiet, u), 1, "exposed from step 600");
    let mut first = None;
    while quiet.sim.tick() < due + 36 {
        let events = quiet.step(1);
        if first.is_none() && !pulses(&events).is_empty() {
            first = Some(quiet.sim.tick());
        }
        let t = quiet.sim.tick();
        assert_eq!(hp(&quiet, u), 80 - i32::from(t >= due), "{t}");
    }
    assert_eq!(first, Some(due));
    println!("Harrying I during tick 600: first pulse at tick {first:?}");
}

/// Acceptance 1, fourth clause: Harrying II doubles the rate. With both
/// Harrying techs the interval is 32 ticks instead of 64 ([`interval2`]
/// and [`interval1`]), and at every multiple of 64 up to 1152 the loss is
/// exactly twice Harrying I's; after 1200 ticks the intruder has 43 HP
/// against 62.
#[test]
fn attrition_harrying_two_doubles_the_rate() {
    let rules = load_rules();
    let (i1, i2) = (interval1(&rules), interval2(&rules));
    let u = UnitId(1);
    let (mut one, _) = attrition_field(&rules, 1, vec![]);
    let (mut two, _) = attrition_field(&rules, 2, vec![]);
    assert_eq!(
        (one.view().harrying_level(P1), two.view().harrying_level(P1)),
        (1, 2)
    );
    let interval = |m: &M| m.view().attrition(u).map(|e| e.interval_ticks);
    assert_eq!((interval(&one), interval(&two)), (Some(i1), Some(i2)));
    assert_eq!(two.view().attrition_pulse(P1, P0), Some((i2, 1)));
    let mut loss = [vec![0; 1201], vec![0; 1201]];
    while one.sim.tick() < 1200 {
        one.step(1);
        two.step(1);
        let t = usize::try_from(one.sim.tick()).unwrap();
        loss[0][t] = 80 - hp(&one, u);
        loss[1][t] = 80 - hp(&two, u);
    }
    let multiples: Vec<usize> = (1..=1200 / i1)
        .map(|k| usize::try_from(k * i1).unwrap())
        .collect();
    assert!(!multiples.is_empty());
    for (k, &t) in (1..).zip(&multiples) {
        assert_eq!(loss[0][t], k, "Harrying I at {t}");
        assert_eq!(loss[1][t], 2 * loss[0][t], "Harrying II at {t}");
    }
    assert_eq!(
        (hp(&one, u), hp(&two, u)),
        (80 - pulses_by(1200, i1), 80 - pulses_by(1200, i2))
    );
    println!(
        "after 1200 ticks: Harrying I hp {} (interval {i1}), Harrying II hp {} (interval {i2}); loss at {i1}k: {:?} vs {:?}",
        hp(&one, u),
        hp(&two, u),
        multiples.iter().map(|&t| loss[0][t]).collect::<Vec<_>>(),
        multiples.iter().map(|&t| loss[1][t]).collect::<Vec<_>>()
    );
}

/// The supply disc is 14 tiles, tile to tile and inclusive, and per
/// player: P0 Wains 14 tiles south or at (10, 9) cover the intruder
/// (`dx² + dy² = 196`, `181`), at 15 tiles south or (10, 10) (`225`,
/// `200`) they do not, and P1's Wain 9 tiles south never covers P0's unit.
/// After 200 ticks (three [`interval1`] pulses and 8 ticks) a covered
/// intruder has 80 HP and an uncovered one 77.
#[test]
fn attrition_supply_radius_is_14_tiles_inclusive_and_per_player() {
    let rules = load_rules();
    let end = 3 * interval1(&rules) + 8;
    let u = UnitId(1);
    let wain = ukind(&rules, "supply_wain");
    let cases = [
        (P0, (35, 45), true),
        (P0, (35, 46), false),
        (P0, (45, 40), true),
        (P0, (45, 41), false),
        (P1, (35, 40), false),
    ];
    for (owner, at, covers) in cases {
        let (mut m, _) = attrition_field(&rules, 1, vec![(owner.0, spawn_for(owner, wain, at))]);
        let t = unit_tile(&m, u);
        let (dx, dy) = (at.0 - i32::from(t.x), at.1 - i32::from(t.y));
        assert_eq!(dx * dx + dy * dy <= 196 && owner == P0, covers, "{at:?}");
        assert_eq!(m.view().supplied(P0, t), covers, "{owner:?} {at:?}");
        assert_eq!(m.view().supplied(P1, t), owner == P1);
        m.step(end - m.sim.tick());
        assert_eq!(hp(&m, u), if covers { 80 } else { 77 }, "{owner:?} {at:?}");
    }
}

/// Walking in and out leaves no hidden damage: the counter drops to 0 the
/// step the unit stands on its owner's land, on neutral land or under its
/// own Supply Wain, and restarts from 0 when it is exposed again. P1's
/// border and P0's touch on row 31 (P0's Town centre is 39 tiles east of
/// P1's), so the Skirmisher walks straight from P1's land onto P0's; it
/// then walks back, out to neutral land north of the border, and back
/// again, and a Wain walks in to cover it and away. Every step checks the
/// counter against the exposure the view reports after the step, and every
/// pulse against the counter, so the hit points lost are exactly the
/// pulses landed.
#[test]
fn attrition_counter_resets_when_supplied_friendly_or_neutral() {
    let rules = load_rules();
    let (u, w) = (UnitId(1), UnitId(2));
    let (mut m, _) = attrition_field(
        &rules,
        1,
        vec![
            (0, spawn_for(P0, ukind(&rules, "supply_wain"), (35, 62))),
            (0, place_for(P0, bkind(&rules, "town"), (59, 30))),
        ],
    );
    let script: BTreeMap<u32, Command> = BTreeMap::from([
        (100, move_to(u, (50, 31))),
        (300, move_to(u, INTRUDER)),
        (450, move_to(u, (35, 8))),
        (650, move_to(u, INTRUDER)),
        (850, move_to(w, (35, 44))),
        (1100, move_to(w, (35, 62))),
    ]);
    let mut counter = exposed(&m, u);
    let (mut lost, mut landed) = (0, 0);
    let mut resets: BTreeMap<&str, u32> = BTreeMap::new();
    while m.sim.tick() < 1300 {
        let cmds = script
            .get(&m.sim.tick())
            .map(|c| vec![(0, c.clone())])
            .unwrap_or_default();
        let events = m.step_with(cmds);
        assert!(rejections(&events).is_empty(), "{events:?}");
        let t = m.sim.tick();
        let tile = unit_tile(&m, u);
        let view = m.view();
        let e = view.attrition(u);
        let before = counter;
        counter = if e.is_some() { counter + 1 } else { 0 };
        assert_eq!(exposed(&m, u), counter, "tick {t} at {tile:?}");
        let want: Vec<(UnitId, i32)> = e
            .filter(|e| counter.is_multiple_of(e.interval_ticks))
            .map(|e| (u, e.hp))
            .into_iter()
            .collect();
        assert_eq!(pulses(&events), want, "tick {t}");
        lost += want.iter().map(|p| p.1).sum::<i32>();
        landed += want.len();
        if e.is_none() && before > 0 {
            let why = match view.territory().owner_at(tile) {
                None => "neutral",
                Some(P0) => "own",
                Some(_) if view.supplied(P0, tile) => "supplied",
                Some(_) => "other",
            };
            *resets.entry(why).or_default() += 1;
        }
    }
    assert_eq!(80 - hp(&m, u), lost, "no hidden damage");
    assert!(landed >= 6, "{landed}");
    assert_eq!(
        resets.keys().copied().collect::<Vec<_>>(),
        ["neutral", "own", "supplied"],
        "{resets:?}"
    );
    println!("resets {resets:?}, {landed} pulses, {lost} HP lost");
}

/// Only Scribes and Supply Wains are immune (`immune_to_attrition`):
/// over 1200 ticks a Yeoman goes from 40 to 22 HP and a Skirmisher from 80
/// to 62 (18 pulses, `1200 /` [`interval1`]), while a Scribe and a Supply
/// Wain (out of range of the others) keep every hit point and a counter of
/// 0.
#[test]
fn attrition_scribes_and_supply_wains_are_immune_yeomen_are_not() {
    let rules = load_rules();
    let n = pulses_by(1200, interval1(&rules));
    let k = |id: &str| ukind(&rules, id);
    let (mut m, _) = attrition_field(
        &rules,
        1,
        vec![
            (0, spawn_for(P0, k("yeoman"), (35, 33))),
            (0, spawn_for(P0, k("scribe"), (35, 29))),
            (0, spawn_for(P0, k("supply_wain"), (21, 49))),
        ],
    );
    let ids = [UnitId(1), UnitId(2), UnitId(3), UnitId(4)];
    for id in ids {
        assert_eq!(m.view().territory().owner_at(unit_tile(&m, id)), Some(P1));
    }
    assert!(!m.view().supplied(P0, unit_tile(&m, UnitId(2))));
    while m.sim.tick() < 1200 {
        m.step(1);
        assert_eq!((exposed(&m, UnitId(3)), exposed(&m, UnitId(4))), (0, 0));
    }
    let got: Vec<i32> = ids.iter().map(|&id| hp(&m, id)).collect();
    assert_eq!(
        got,
        [80 - n, 40 - n, 30, 150],
        "skirmisher, yeoman, scribe, wain"
    );
    assert_eq!(m.view().attrition(UnitId(3)), None);
    assert_eq!(m.view().attrition(UnitId(4)), None);
    println!("after 1200 ticks: skirmisher, yeoman, scribe, wain hp {got:?}");
}

/// The age-gap table and the late Harrying bonus: a Masonry owner deals 2
/// per pulse to a Hearth intruder (gap 1); a Hearth owner deals 1 to a
/// Masonry intruder (a negative gap reads the gap-0 row); with an in-memory
/// `harrying_bonus_hp` of 3 a Charter owner deals 5 to a Hearth intruder
/// (gap 2 clamps to the last row, 2, plus 3). After 200 ticks (3 pulses of
/// [`interval1`], and 8 ticks) the intruder has 74, 77 and 65 HP.
#[test]
fn attrition_age_gap_table_and_late_harrying_bonus() {
    let base = load_rules();
    let i = interval1(&base);
    let mut bonus = base.clone();
    bonus.harrying_bonus_hp = 3;
    // The commands that take `p` to `age` (DebugResearch skips the age
    // checks; DebugGrant pays exactly the ages' costs).
    let to_age = |rules: &Rules, p: PlayerId, age: usize| {
        let techs = [
            "letters_1",
            "trade_1",
            "arms_1",
            "statecraft_1",
            "letters_2",
        ];
        let needed = usize::try_from(rules.ages[age].techs_required).unwrap();
        let mut cmds: Vec<(u8, Command)> = techs[..needed]
            .iter()
            .map(|id| {
                (
                    p.0,
                    Command::DebugResearch {
                        owner: p,
                        tech: tech(rules, id),
                    },
                )
            })
            .collect();
        for a in 1..=age {
            for (r, amount) in rules.amounts(&rules.ages[a].cost).into_iter().enumerate() {
                if amount > 0 {
                    cmds.push((
                        p.0,
                        Command::DebugGrant {
                            owner: p,
                            resource: u8::try_from(r).unwrap(),
                            amount: u32::try_from(amount).unwrap(),
                        },
                    ));
                }
            }
            cmds.push((p.0, Command::AdvanceAge { player: p }));
        }
        cmds
    };
    let cases = [
        ("Masonry owner, Hearth intruder", &base, 1, 0, 2, 74),
        ("Hearth owner, Masonry intruder", &base, 0, 1, 1, 77),
        ("Charter owner with a bonus of 3", &bonus, 2, 0, 5, 65),
    ];
    for (name, rules, owner_age, intruder_age, per_pulse, want) in cases {
        let mut extra = to_age(rules, P0, intruder_age);
        extra.extend(to_age(rules, P1, owner_age));
        let (mut m, _) = attrition_field(rules, 1, extra);
        assert_eq!(
            (m.view().age(P1), m.view().age(P0)),
            (
                u8::try_from(owner_age).unwrap(),
                u8::try_from(intruder_age).unwrap()
            ),
            "{name}"
        );
        assert_eq!(
            m.view().attrition_pulse(P1, P0),
            Some((i, per_pulse)),
            "{name}"
        );
        let mut landed = Vec::new();
        while m.sim.tick() < 3 * i + 8 {
            landed.extend(pulses(&m.step(1)).into_iter().map(|p| p.1));
        }
        assert_eq!(landed, [per_pulse; 3], "{name}");
        assert_eq!(hp(&m, UnitId(1)), want, "{name}");
        println!(
            "{name}: {per_pulse} HP per pulse, hp {want} after {} ticks",
            m.sim.tick()
        );
    }
}

/// Attrition kills through the common death path: a Yeoman at 1 HP
/// (`DebugDamage` 39) walking inside P1's border dies to the pulse at tick
/// 64 ([`interval1`]): `UnitDied` with its owner and kind in that step, no
/// `Hit` in the whole run, and no path request left behind, although the
/// walk's periodic repath asked for one in the same step (the control,
/// without the damage, still has it pending after that step).
#[test]
fn attrition_kills_through_the_common_death_path() {
    let rules = load_rules();
    let yeoman = ukind(&rules, "yeoman");
    let u = UnitId(2);
    let i = interval1(&rules);
    let repath = rules.repath_interval_ticks();
    // The killing pulse lands at the first multiple of the interval that
    // leaves room for the walk to start in step 3 or later: tick 64 with
    // the shipped data. The pulses before it (none with the shipped data)
    // are paid for by a smaller `DebugDamage`.
    let kill_at = (repath + 4).div_ceil(i) * i;
    let earlier = pulses_by(kill_at - 1, i);
    // The walk's first periodic repath falls in step `kill_at - 1`, with
    // the pulse: a Move applied in step s repaths in step s + repath
    // (3 + 60 = 63 with the shipped data).
    let walk_from = kill_at - 1 - repath;
    for kill in [true, false] {
        let (mut m, _) = attrition_field(&rules, 1, vec![(0, spawn_for(P0, yeoman, (35, 33)))]);
        let mut all = Vec::new();
        if kill {
            let amount = u32::try_from(39 - earlier).unwrap();
            all.extend(m.step_with(vec![(0, damage(TargetId::Unit(u), amount))]));
            assert_eq!(hp(&m, u), 1 + earlier);
        } else {
            all.extend(m.step(1));
        }
        all.extend(m.step(walk_from - m.sim.tick()));
        assert_eq!(m.sim.tick(), walk_from);
        all.extend(m.step_with(vec![(0, move_to(u, (35, 20)))]));
        all.extend(m.step(kill_at - 1 - m.sim.tick()));
        assert_eq!(m.sim.tick(), kill_at - 1);
        assert_eq!(
            m.view().territory().owner_at(unit_tile(&m, u)),
            Some(P1),
            "still walking inside the border"
        );
        assert!(m.view().unit(u).unwrap().order.is_some());
        if kill {
            assert_eq!(hp(&m, u), 1, "one pulse from death");
        }
        let last = m.step(1);
        all.extend(last.iter().cloned());
        assert_eq!(m.sim.tick(), kill_at);
        assert!(pulses(&last).contains(&(u, 1)), "{last:?}");
        let died = SimEvent::UnitDied {
            unit: u,
            owner: P0,
            kind: yeoman,
        };
        if kill {
            assert!(last.contains(&died), "{last:?}");
            assert!(m.view().unit(u).is_none());
            assert!(!m.view().pathing().has_request(u), "the request went too");
        } else {
            assert!(!last.contains(&died));
            assert!(
                m.view().pathing().has_request(u),
                "the repath of step {}",
                kill_at - 1
            );
        }
        assert_eq!(hits(&all), 0);
    }
}

/// Attrition draws no randomness: with Harrying II the `rng` sub-hash
/// equals the no-Harrying control after every step of 600, while the
/// units differ (pulses landed, and killed a Yeoman left at 10 HP).
#[test]
fn attrition_draws_no_randomness() {
    let rules = load_rules();
    let extra = || vec![(0, spawn_for(P0, ukind(&rules, "yeoman"), (35, 33)))];
    let (mut a, _) = attrition_field(&rules, 2, extra());
    let (mut b, _) = attrition_field(&rules, 0, extra());
    let hurt = || vec![(0, damage(TargetId::Unit(UnitId(2)), 30))];
    let mut landed = pulses(&a.step_with(hurt())).len();
    b.step_with(hurt());
    let mut died = 0;
    for _ in 0..600 {
        let events = a.step(1);
        landed += pulses(&events).len();
        died += events
            .iter()
            .filter(|e| matches!(e, SimEvent::UnitDied { .. }))
            .count();
        b.step(1);
        assert_eq!(a.sim.sub_hashes().rng, b.sim.sub_hashes().rng);
    }
    assert_ne!(a.sim.sub_hashes().units, b.sim.sub_hashes().units);
    assert!(landed > 0 && died == 1, "{landed} {died}");
}

/// A restore mid-exposure continues exactly: the snapshot is taken when
/// the Skirmisher's counter is 37, while a Supply Wain walks toward it
/// (its tile changes after the restore, then it covers the Skirmisher) and
/// a Yeoman bleeds out of its range; Harrying II arrives later, the Wain
/// walks away again. Every sub-hash, the whole hash and the events agree
/// after every one of 600 steps, and again for 200 steps after 50 units
/// spawn inside P1's border.
#[test]
fn attrition_restore_mid_exposure_matches_uninterrupted() {
    let rules = load_rules();
    let (u, w) = (UnitId(1), UnitId(2));
    let (mut a, _) = attrition_field(
        &rules,
        1,
        vec![
            (0, spawn_for(P0, ukind(&rules, "supply_wain"), (35, 50))),
            (0, spawn_for(P0, ukind(&rules, "yeoman"), (30, 22))),
        ],
    );
    a.step_with(vec![(0, move_to(w, (35, 40)))]);
    while exposed(&a, u) < 37 {
        a.step(1);
    }
    assert_eq!((a.sim.tick(), exposed(&a, u)), (37, 37));
    let wain_tile = unit_tile(&a, w);
    let mut b = a.clone_at_tick();
    assert_eq!(a.sim.sub_hashes(), b.sim.sub_hashes());
    let script = |t: u32, rules: &Rules| -> Vec<(u8, Command)> {
        match t {
            100 => harrying_for(rules, P1, 2)[1..].to_vec(),
            300 => vec![(0, move_to(w, (35, 62)))],
            _ => Vec::new(),
        }
    };
    let (mut resets, mut landed, mut spawned) = (0, 0, 0);
    let mut wain_moved = false;
    for i in 0..800 {
        let t = a.sim.tick();
        let mut cmds = script(t, &rules);
        if i == 600 {
            cmds.push((
                0,
                Command::DebugSpawn {
                    owner: P0,
                    kind: ukind(&rules, "skirmisher"),
                    at: FxVec2::from_ints(30, 40),
                    count: 50,
                },
            ));
        }
        let before = a.view().unit(u).map_or(0, |x| x.exposed_ticks);
        let ea = a.step_with(cmds.clone());
        let eb = b.step_with(cmds);
        assert_eq!(ea, eb, "events after tick {t}");
        assert_eq!(a.sim.sub_hashes(), b.sim.sub_hashes(), "after tick {t}");
        assert_eq!(a.sim.hash(), b.sim.hash());
        landed += pulses(&ea).len();
        spawned += ea
            .iter()
            .filter(|e| matches!(e, SimEvent::UnitSpawned { .. }))
            .count();
        let after = a.view().unit(u).map_or(0, |x| x.exposed_ticks);
        resets += usize::from(before > 0 && after == 0);
        wain_moved |= unit_tile(&a, w) != wain_tile;
    }
    assert_eq!(spawned, 50);
    assert!(
        wain_moved && resets >= 1 && landed > 20,
        "{resets} {landed}"
    );
}

/// What `SimView::attrition` reports before a step is what the step
/// applies to a unit that stands still: a pulse of `hp` when the counter
/// reaches a multiple of `interval_ticks`, and no pulse and a counter of 0
/// when it reports `None`. P1 is in the Masonry Age with both Harrying
/// techs (32 ticks, [`interval2`], and 2 HP against Hearth intruders); the
/// units are an exposed Skirmisher and Yeoman, an immune Scribe, a
/// Skirmisher under its own Wain, one on neutral land, and P1 units on
/// their own and on neutral land.
#[test]
fn attrition_view_reports_the_pulse_the_step_applies() {
    let rules = load_rules();
    let k = |id: &str| ukind(&rules, id);
    let mut extra = vec![
        (0, spawn_for(P0, k("yeoman"), (35, 27))),
        (0, spawn_for(P0, k("scribe"), (35, 35))),
        (0, spawn_for(P0, k("skirmisher"), (21, 45))),
        (0, spawn_for(P0, k("supply_wain"), (21, 52))),
        (0, spawn_for(P0, k("skirmisher"), (45, 31))),
        (1, spawn_for(P1, k("bowman"), (5, 31))),
        (1, spawn_for(P1, k("bowman"), (5, 60))),
        (
            1,
            Command::DebugResearch {
                owner: P1,
                tech: tech(&rules, "letters_1"),
            },
        ),
        (
            1,
            Command::DebugResearch {
                owner: P1,
                tech: tech(&rules, "trade_1"),
            },
        ),
    ];
    for (r, amount) in rules.amounts(&rules.ages[1].cost).into_iter().enumerate() {
        if amount > 0 {
            extra.push((
                1,
                Command::DebugGrant {
                    owner: P1,
                    resource: u8::try_from(r).unwrap(),
                    amount: u32::try_from(amount).unwrap(),
                },
            ));
        }
    }
    extra.push((1, Command::AdvanceAge { player: P1 }));
    let (mut m, _) = attrition_field(&rules, 2, extra);
    let i2 = interval2(&rules);
    assert_eq!(m.view().attrition_pulse(P1, P0), Some((i2, 2)));
    assert_eq!(m.view().attrition_pulse(P0, P1), None);
    let ids: Vec<UnitId> = m.view().units().map(|u| u.id).collect();
    assert_eq!(ids.len(), 8);
    let (mut some, mut none) = (0, 0);
    for _ in 0..300 {
        let before: Vec<(UnitId, Option<Exposure>, u32, i32)> = ids
            .iter()
            .map(|&id| (id, m.view().attrition(id), exposed(&m, id), hp(&m, id)))
            .collect();
        let events = m.step(1);
        let landed: BTreeMap<UnitId, i32> = pulses(&events).into_iter().collect();
        for (id, e, n, h) in before {
            let next = if e.is_some() { n + 1 } else { 0 };
            let want = e
                .filter(|e| next.is_multiple_of(e.interval_ticks))
                .map(|e| e.hp);
            assert_eq!(landed.get(&id).copied(), want, "{id:?}");
            assert_eq!(exposed(&m, id), next, "{id:?}");
            assert_eq!(hp(&m, id), h - want.unwrap_or(0), "{id:?}");
            if e.is_some() {
                some += 1;
            } else {
                none += 1;
            }
        }
    }
    let exposed_units: Vec<UnitId> = ids
        .iter()
        .copied()
        .filter(|&id| m.view().attrition(id).is_some())
        .collect();
    assert_eq!(exposed_units, [UnitId(1), UnitId(2)]);
    assert_eq!(
        m.view().attrition(UnitId(1)),
        Some(Exposure {
            owner: P1,
            interval_ticks: i2,
            hp: 2
        })
    );
    assert!(some == 600 && none == 1800, "{some} {none}");
}

/// `DebugDamage` deals damage at once and leaves removal to step 5c: a
/// Skirmisher at 1 HP lives, one more point kills it (`UnitDied`); a
/// Watchtower at 1 HP stands, one more destroys it; a site is destroyed
/// like any building below 1 HP; a complete Town clamps at 0 with one
/// `TownAtZeroHp` and stays; a player may damage its own buildings; and
/// nothing draws from the RNG.
#[test]
fn attrition_debug_damage_kills_through_reap_and_clamps_towns() {
    let rules = load_rules();
    let mut m = M::delay0(rules.clone(), 6);
    let events = m.step_with(vec![
        (0, spawn_for(P0, ukind(&rules, "skirmisher"), (60, 60))),
        (0, place_for(P0, bkind(&rules, "croft"), (64, 64))),
        (1, place_for(P1, bkind(&rules, "watchtower"), (10, 10))),
        (1, place_for(P1, bkind(&rules, "town"), (20, 30))),
        (
            1,
            Command::DebugPlaceBuilding {
                owner: P1,
                kind: bkind(&rules, "croft"),
                tile: Tile::new(14, 14),
                complete: false,
            },
        ),
    ]);
    assert!(rejections(&events).is_empty(), "{events:?}");
    let rng = m.sim.sub_hashes().rng;
    let (u, croft, tower, town, site) = (
        UnitId(1),
        BuildingId(1),
        BuildingId(2),
        BuildingId(3),
        BuildingId(4),
    );
    let bhp = |m: &M, b| m.view().building_hp(b).map(|h| h.0);
    let unit = TargetId::Unit(u);
    let events = m.step_with(vec![
        (0, damage(unit, 79)),
        (0, damage(TargetId::Building(tower), 499)),
        (0, damage(TargetId::Building(town), u32::MAX)),
        (0, damage(TargetId::Building(croft), 100)),
        (0, damage(TargetId::Building(site), 300)),
    ]);
    assert!(rejections(&events).is_empty(), "{events:?}");
    assert_eq!(hp(&m, u), 1);
    assert_eq!(bhp(&m, tower), Some(1));
    assert_eq!(bhp(&m, town), Some(0));
    assert_eq!(bhp(&m, croft), Some(200), "an own building");
    assert_eq!(bhp(&m, site), None, "a site at 0 is destroyed");
    let zero = SimEvent::TownAtZeroHp { town };
    assert_eq!(events.iter().filter(|e| **e == zero).count(), 1);
    assert!(events.contains(&SimEvent::BuildingDestroyed {
        building: site,
        owner: P1,
        kind: bkind(&rules, "croft"),
    }));
    let events = m.step_with(vec![
        (1, damage(unit, 1)),
        (1, damage(TargetId::Building(tower), 1)),
        (1, damage(TargetId::Building(town), 5)),
    ]);
    assert!(rejections(&events).is_empty(), "{events:?}");
    assert!(events.contains(&SimEvent::UnitDied {
        unit: u,
        owner: P0,
        kind: ukind(&rules, "skirmisher"),
    }));
    assert!(events.contains(&SimEvent::BuildingDestroyed {
        building: tower,
        owner: P1,
        kind: bkind(&rules, "watchtower"),
    }));
    assert!(!events.contains(&zero), "only on the step that reaches 0");
    assert_eq!(bhp(&m, town), Some(0), "a complete Town stays at 0");
    assert!(m.view().unit(u).is_none() && bhp(&m, tower).is_none());
    assert_eq!(m.sim.sub_hashes().rng, rng, "no draw");
    assert_eq!(hits(&events), 0);
}

/// Rejected `DebugDamage` changes nothing but `commands_applied`: a unit
/// or building that does not exist, or a unit that has died, is
/// `InvalidTarget`; with `debug_commands` off it is
/// `DebugCommandsDisabled`, before the target is looked at. Each is
/// compared with a control on every sub-hash.
#[test]
fn attrition_debug_damage_rejections_change_nothing() {
    let rules = load_rules();
    let with_units = || {
        let mut m = M::new(rules.clone(), 7);
        m.ok(vec![
            spawn_for(P0, ukind(&rules, "skirmisher"), (60, 60)),
            spawn_for(P0, ukind(&rules, "yeoman"), (62, 60)),
        ]);
        m.ok(vec![damage(TargetId::Unit(UnitId(2)), 1000)]);
        assert!(m.view().unit(UnitId(2)).is_none());
        m
    };
    let no_debug = || {
        let mut setup = MatchSetup::scenario(&rules, 7);
        setup.debug_commands = false;
        M {
            sim: Sim::new(setup, rules.clone(), Box::new(NoAi)),
            seq: [0; 2],
        }
    };
    let cases: Vec<(&str, &dyn Fn() -> M, Command, RejectReason)> = vec![
        (
            "a unit that never existed",
            &with_units,
            damage(TargetId::Unit(UnitId(999)), 5),
            RejectReason::InvalidTarget,
        ),
        (
            "a unit that died",
            &with_units,
            damage(TargetId::Unit(UnitId(2)), 5),
            RejectReason::InvalidTarget,
        ),
        (
            "a building that does not exist",
            &with_units,
            damage(TargetId::Building(BuildingId(0)), 5),
            RejectReason::InvalidTarget,
        ),
        (
            "debug commands off",
            &no_debug,
            damage(TargetId::Unit(UnitId(999)), 5),
            RejectReason::DebugCommandsDisabled,
        ),
    ];
    for (name, setup, cmd, want) in cases {
        for p in [0, 1] {
            rejected_without_change(name, setup, p, cmd.clone(), want);
        }
    }
}

/// The rules of the M5a random streams: the shipped data with
/// `attrition_interval_ds: 4` (8 ticks, 4 at Harrying II), so a short
/// stream lands many pulses.
fn stream_rules() -> &'static Rules {
    static RULES: std::sync::OnceLock<Rules> = std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        let mut r = load_rules();
        r.attrition_interval_ds = 4;
        assert_eq!(r.attrition_interval_ticks(), 8);
        r
    })
}

/// A seeded random stream over both players for the delay-0 sim with
/// [`stream_rules`]. Step 0: P1's Town at [`FIELD_TOWN`] with Harrying I
/// (and II on odd seeds), P0's Town at (59,30) (the borders touch on row
/// 31) with Harrying I on seeds divisible by 3, and units of both sides on
/// each other's land: armed units, a Yeoman and a Scribe each, and a
/// Supply Wain each out of their range.
/// Then each tick may move a few units of one side to a random point of
/// the strip between the Towns, stop one, damage a random unit id (dead
/// or never spawned ones are rejected), research Harrying II for a random
/// player (rejected once researched) or spawn a unit.
fn random_attrition_commands(seed: u64, ticks: u32) -> Vec<Vec<PlayerCommand>> {
    let rules = stream_rules();
    let k = |id: &str| ukind(rules, id);
    let mut rng = rand_pcg::Pcg32::seed_from_u64(seed);
    let mut seq = [0u32; 2];
    let mut ids: [Vec<u32>; 2] = [Vec::new(), Vec::new()];
    let mut next_id = 1u32;
    let mut out = Vec::with_capacity(usize::try_from(ticks).unwrap());
    let kinds = ["skirmisher", "bowman", "yeoman", "scribe", "supply_wain"];
    let point = |rng: &mut rand_pcg::Pcg32| {
        (
            10 + i32::try_from(rng.next_u32() % 60).unwrap(),
            14 + i32::try_from(rng.next_u32() % 36).unwrap(),
        )
    };
    for t in 0..ticks {
        let mut cmds: Vec<PlayerCommand> = Vec::new();
        let mut push = |p: usize, c: Command| {
            cmds.push(PlayerCommand::new(
                PlayerId(u8::try_from(p).unwrap()),
                seq[p],
                c,
            ));
            seq[p] += 1;
        };
        if t == 0 {
            for (p, at) in [(0usize, (35, 31)), (1, (47, 31))] {
                let owner = PlayerId(u8::try_from(p).unwrap());
                for (i, kind) in kinds.iter().enumerate() {
                    // Two tiles apart; the Wain (last) 20 tiles south of
                    // the others, out of their supply range.
                    let dy = if *kind == "supply_wain" {
                        20
                    } else {
                        2 * i32::try_from(i).unwrap() - 4
                    };
                    push(p, spawn_for(owner, k(kind), (at.0, at.1 + dy)));
                    ids[p].push(next_id);
                    next_id += 1;
                }
            }
            push(0, place_for(P0, bkind(rules, "town"), (59, 30)));
            if seed.is_multiple_of(3) {
                push(0, harrying_for(rules, P0, 1)[0].1.clone());
            }
            push(1, place_for(P1, bkind(rules, "town"), FIELD_TOWN));
            for (_, c) in harrying_for(rules, P1, 1 + usize::from(seed % 2 == 1)) {
                push(1, c);
            }
        } else {
            let roll = rng.next_u32() % 100;
            let p = usize::try_from(rng.next_u32() % 2).unwrap();
            if roll < 14 {
                let n = 1 + rng.next_u32() % 3;
                let units = (0..n)
                    .map(|_| {
                        UnitId(ids[p][usize::try_from(rng.next_u32()).unwrap() % ids[p].len()])
                    })
                    .collect();
                let (x, y) = point(&mut rng);
                push(
                    p,
                    Command::Move {
                        units,
                        target: FxVec2::from_ints(x, y),
                        queue: false,
                    },
                );
            } else if roll < 17 {
                let u = ids[p][usize::try_from(rng.next_u32()).unwrap() % ids[p].len()];
                push(
                    p,
                    Command::Stop {
                        units: vec![UnitId(u)],
                    },
                );
            } else if roll < 22 {
                let target = UnitId(rng.next_u32() % (next_id + 2));
                let amount = 1 + rng.next_u32() % 30;
                push(p, damage(TargetId::Unit(target), amount));
            } else if roll < 23 {
                let owner = PlayerId(u8::try_from(p).unwrap());
                push(p, harrying_for(rules, owner, 2)[1].1.clone());
            } else if roll < 25 {
                let owner = PlayerId(u8::try_from(p).unwrap());
                let kind = kinds[usize::try_from(rng.next_u32()).unwrap() % kinds.len()];
                push(p, spawn_for(owner, k(kind), point(&mut rng)));
                ids[p].push(next_id);
                next_id += 1;
            }
        }
        out.push(cmds);
    }
    out
}

/// Run `stream` on the delay-0 sim with [`stream_rules`] and `seed`,
/// restoring into a fresh sim after step `restore_after` (never when it is
/// past the end): the hash after every step, and the events.
fn run_attrition_stream(
    seed: u64,
    stream: &[Vec<PlayerCommand>],
    restore_after: Option<usize>,
) -> (Vec<u64>, Vec<SimEvent>) {
    let fresh = || {
        let mut setup = MatchSetup::scenario(stream_rules(), seed);
        setup.cmd_delay = 0;
        Sim::new(setup, stream_rules().clone(), Box::new(NoAi))
    };
    let mut sim = fresh();
    let mut hashes = Vec::with_capacity(stream.len());
    let mut events = Vec::new();
    for (i, cmds) in stream.iter().enumerate() {
        sim.step(cmds);
        hashes.push(sim.hash());
        events.extend(sim.drain_events());
        if restore_after == Some(i) {
            let mut restored = fresh();
            restored.restore(&sim.snapshot()).expect("restores");
            sim = restored;
        }
    }
    (hashes, events)
}

/// The random attrition stream really exercises attrition: over 8 seeds
/// of 200 ticks, pulses land on both players' units (P0's Harrying comes on
/// seeds divisible by 3), some at Harrying II's interval, units die,
/// counters reset, and `DebugDamage` is both accepted and rejected.
#[test]
fn m5a_attrition_stream_exercises_attrition() {
    let (mut on, mut died, mut rejected) = ([0usize; 2], 0, 0);
    for seed in 0..8 {
        let stream = random_attrition_commands(seed, 200);
        let (_, events) = run_attrition_stream(seed, &stream, None);
        let owner: BTreeMap<UnitId, PlayerId> = {
            let mut next = 0u32;
            let mut map = BTreeMap::new();
            for pc in stream.iter().flatten() {
                if let Command::DebugSpawn { owner, .. } = pc.cmd {
                    next += 1;
                    map.insert(UnitId(next), owner);
                }
            }
            map
        };
        for e in &events {
            match e {
                SimEvent::Attrition { unit, .. } => on[usize::from(owner[unit].0)] += 1,
                SimEvent::UnitDied { .. } => died += 1,
                SimEvent::CommandRejected {
                    reason: RejectReason::InvalidTarget,
                    ..
                } => rejected += 1,
                _ => {}
            }
        }
    }
    eprintln!(
        "m5a attrition stream, 8 seeds: pulses on P0, P1 units {on:?}, died {died}, rejected DebugDamage {rejected}"
    );
    assert!(on[0] > 0 && on[1] > 0, "{on:?}");
    assert!(died > 0 && rejected > 0, "{died} {rejected}");
}

/// Proptest case count: `PROPTEST_CASES` when set, else 256 in CI and 8
/// locally (the `cargo test -p sim` budget), as in `m4a.rs`.
fn proptest_cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if std::env::var_os("CI").is_some() {
            256
        } else {
            8
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: proptest_cases(),
        .. ProptestConfig::default()
    })]

    /// A random attrition stream gives the same hash after every step on
    /// a second run that restores from a snapshot at a random step.
    #[test]
    fn m5a_attrition_stream_is_deterministic_with_restore(
        seed in any::<u64>(),
        ticks in 60u32..240,
        cut in 0u32..1000,
    ) {
        let stream = random_attrition_commands(seed, ticks);
        let (a, _) = run_attrition_stream(seed, &stream, None);
        let at = usize::try_from(cut % ticks).unwrap();
        let (b, _) = run_attrition_stream(seed, &stream, Some(at));
        prop_assert_eq!(a, b, "restored after step {}", at);
    }
}
