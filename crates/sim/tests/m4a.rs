//! M4a acceptance tests, tech slice: research, ages, the Modifier resolver
//! and what the resolved stats change. Test names start with `tech_` so
//! that `cargo test -p sim tech` selects them (the filter matches test
//! paths, not integration-test file names).
//!
//! Most tests run on `MatchSetup::scenario`, an empty start: with no
//! building there is no income, so stockpiles are exact through
//! `DebugGrant`. Where a test needs a Scriptorium (which yields Lore) it
//! compares with a control sim instead. Every rejection is compared with a
//! control sim that receives a rejected no-op with the same `(player,
//! seq)`, on every sub-hash (`rng` included) and the whole hash.

use rules::{ModOp, Modifier, Rules, StatPath, UnitRamp};
use sim::{
    BuildingId, BuildingKindId, Command, FxVec2, MatchSetup, PlayerCommand, PlayerId, QueueEntry,
    QueueItem, RejectReason, Sim, SimEvent, SimView, StartKind, TechId, Tile, UnitId, UnitKindId,
};
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

fn res_u8(rules: &Rules, id: &str) -> u8 {
    u8::try_from(res(rules, id)).unwrap()
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

    /// The scenario from the standard start: P0's Seat is `BuildingId(1)`,
    /// P1's `BuildingId(2)`, P0's Yeomen `UnitId(1..=5)`.
    fn standard(rules: Rules, seed: u64) -> M {
        let mut setup = MatchSetup::scenario(&rules, seed);
        setup.start = StartKind::Standard;
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

    fn rules(&self) -> &Rules {
        self.sim.rules()
    }
}

fn grant(resource: usize, amount: u32) -> Command {
    Command::DebugGrant {
        owner: P0,
        resource: u8::try_from(resource).unwrap(),
        amount,
    }
}

fn place(kind: BuildingKindId, x: u16, y: u16, complete: bool) -> Command {
    place_for(P0, kind, x, y, complete)
}

fn place_for(owner: PlayerId, kind: BuildingKindId, x: u16, y: u16, complete: bool) -> Command {
    Command::DebugPlaceBuilding {
        owner,
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

fn spawn(kind: UnitKindId, x: i32, y: i32, count: u16) -> Command {
    Command::DebugSpawn {
        owner: P0,
        kind,
        at: FxVec2::from_ints(x, y),
        count,
    }
}

fn train(building: u32, kind: UnitKindId) -> Command {
    Command::Train {
        building: BuildingId(building),
        kind,
    }
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

/// The Scriptoria, Muster Hall and Town positions in the all-grass west
/// rectangle of `plains_1v1` (x 0..43, y 24..41).
const SCRIPTORIUM_A: (u16, u16) = (4, 26);
const SCRIPTORIUM_B: (u16, u16) = (8, 26);
const MUSTER_HALL: (u16, u16) = (12, 26);
const TOWN: (u16, u16) = (20, 30);

/// An empty start with P0's complete Scriptorium as `BuildingId(1)` and
/// plenty of every resource.
fn lab(rules: &Rules, seed: u64) -> M {
    let mut m = M::new(rules.clone(), seed);
    let s = bkind(rules, "scriptorium");
    m.ok(vec![
        place(s, SCRIPTORIUM_A.0, SCRIPTORIUM_A.1, true),
        grant(res(rules, "grain"), 2000),
        grant(res(rules, "lumber"), 2000),
        grant(res(rules, "ore"), 2000),
        grant(res(rules, "lore"), 999),
    ]);
    m
}

/// `DebugResearch` Letters I and Trade I, grant the Masonry cost and
/// advance; asserts P0 is then in the Masonry Age.
fn reach_masonry(m: &mut M) {
    let rules = m.rules().clone();
    let mut cmds = vec![
        debug_research(tech(&rules, "letters_1")),
        debug_research(tech(&rules, "trade_1")),
    ];
    for (id, &amount) in &rules.ages[1].cost {
        cmds.push(grant(res(&rules, id), amount));
    }
    cmds.push(advance());
    m.ok(cmds);
    assert_eq!(m.view().age(P0), 1);
}

/// Acceptance 1, first clause: the Masonry Age needs two completed techs
/// (a queued one does not count) and exactly 250 Grain and 100 Lore.
#[test]
fn tech_masonry_age_unlocks_only_after_two_techs_and_250_grain_100_lore() {
    let rules = load_rules();
    let (grain, lore) = (res(&rules, "grain"), res(&rules, "lore"));
    let (letters, trade) = (tech(&rules, "letters_1"), tech(&rules, "trade_1"));
    let rich = |m: &mut M| {
        m.ok(vec![grant(grain, 1000), grant(lore, 999)]);
    };
    rejected_without_change(
        "no tech",
        &|| {
            let mut m = M::new(rules.clone(), 1);
            rich(&mut m);
            m
        },
        0,
        advance(),
        RejectReason::NotEnoughTechs,
    );
    rejected_without_change(
        "one tech",
        &|| {
            let mut m = M::new(rules.clone(), 1);
            rich(&mut m);
            m.ok(vec![debug_research(letters)]);
            m
        },
        0,
        advance(),
        RejectReason::NotEnoughTechs,
    );
    rejected_without_change(
        "one tech and one queued",
        &|| {
            let mut m = lab(&rules, 1);
            m.ok(vec![debug_research(letters)]);
            m.ok(vec![research(1, trade)]);
            let queue = &m.view().building(BuildingId(1)).unwrap().queue;
            assert!(matches!(queue[0].item, QueueItem::Research { tech, .. } if tech == trade));
            m
        },
        0,
        advance(),
        RejectReason::NotEnoughTechs,
    );
    let two_techs = |g: u32, l: u32| {
        let rules = rules.clone();
        move || {
            let mut m = M::new(rules.clone(), 1);
            m.ok(vec![
                debug_research(letters),
                debug_research(trade),
                grant(grain, g),
                grant(lore, l),
            ]);
            m
        }
    };
    rejected_without_change(
        "249 Grain",
        &two_techs(249, 100),
        0,
        advance(),
        RejectReason::CannotAfford {
            resource: res_u8(&rules, "grain"),
        },
    );
    rejected_without_change(
        "99 Lore",
        &two_techs(250, 99),
        0,
        advance(),
        RejectReason::CannotAfford {
            resource: res_u8(&rules, "lore"),
        },
    );
    let mut m = two_techs(250, 100)();
    let mut want = vec![0i64; 4];
    want[grain] = 250;
    want[lore] = 100;
    assert_eq!(m.stock(P0), want);
    assert_eq!(m.view().check_advance_age(P0), Ok(want.clone()));
    let events = m.ok(vec![advance()]);
    assert!(events.contains(&SimEvent::AgeAdvanced { player: P0, age: 1 }));
    assert_eq!(m.view().age(P0), 1);
    assert_eq!(
        m.stock(P0),
        vec![0; 4],
        "the stock fell by exactly the cost"
    );
    assert_eq!(m.view().age(P1), 0, "P1 stays in the Hearth Age");
    eprintln!("masonry: 2 techs + 250 Grain + 100 Lore -> age 1, stock {want:?} -> 0");
}

/// Acceptance 1, second clause: the Charter Age needs five completed techs
/// and exactly 500 Grain, 300 Lumber and 300 Lore; it is the last age, and
/// one `AdvanceAge` moves one age.
#[test]
fn tech_charter_age_unlocks_only_after_five_techs_and_500_grain_300_lumber_300_lore() {
    let rules = load_rules();
    let (grain, lumber, lore) = (
        res(&rules, "grain"),
        res(&rules, "lumber"),
        res(&rules, "lore"),
    );
    const FIVE: [&str; 5] = [
        "letters_1",
        "trade_1",
        "arms_1",
        "statecraft_1",
        "letters_2",
    ];
    let in_masonry = |n: usize, g: u32, lu: u32, lo: u32| {
        let rules = rules.clone();
        move || {
            let mut m = M::new(rules.clone(), 2);
            reach_masonry(&mut m);
            let more = FIVE[2..n].iter().map(|id| debug_research(tech(&rules, id)));
            m.ok(more.collect());
            assert_eq!(m.view().researched(P0).len(), n);
            assert_eq!(m.stock(P0), vec![0; 4]);
            m.ok(vec![grant(grain, g), grant(lumber, lu), grant(lore, lo)]);
            m
        }
    };
    rejected_without_change(
        "four techs",
        &in_masonry(4, 500, 300, 300),
        0,
        advance(),
        RejectReason::NotEnoughTechs,
    );
    for (short, name) in [(grain, "grain"), (lumber, "lumber"), (lore, "lore")] {
        let less = |r: usize, v: u32| if r == short { v - 1 } else { v };
        rejected_without_change(
            &format!("one {name} short"),
            &in_masonry(5, less(grain, 500), less(lumber, 300), less(lore, 300)),
            0,
            advance(),
            RejectReason::CannotAfford {
                resource: res_u8(&rules, name),
            },
        );
    }
    let mut m = in_masonry(5, 500, 300, 300)();
    let events = m.ok(vec![advance()]);
    assert!(events.contains(&SimEvent::AgeAdvanced { player: P0, age: 2 }));
    assert_eq!(m.view().age(P0), 2);
    assert_eq!(
        m.stock(P0),
        vec![0; 4],
        "the stock fell by exactly the cost"
    );
    rejected_without_change(
        "the last age",
        &|| {
            let mut m = in_masonry(5, 2000, 2000, 999)();
            m.ok(vec![advance()]);
            m
        },
        0,
        advance(),
        RejectReason::FinalAge,
    );
    // A Hearth player with five techs and plenty of everything moves one
    // age per command.
    let mut m = M::new(rules.clone(), 2);
    let mut cmds: Vec<Command> = FIVE
        .iter()
        .map(|id| debug_research(tech(&rules, id)))
        .collect();
    cmds.extend([grant(grain, 2000), grant(lumber, 2000), grant(lore, 999)]);
    m.ok(cmds);
    let events = m.ok(vec![advance()]);
    let ages: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            SimEvent::AgeAdvanced { age, .. } => Some(*age),
            _ => None,
        })
        .collect();
    assert_eq!(ages, [1], "only to Masonry");
    assert_eq!(m.view().age(P0), 1);
    m.ok(vec![advance()]);
    assert_eq!(m.view().age(P0), 2);
}

/// The gates in `ages.ron` are the design's numbers.
#[test]
fn tech_age_gates_match_the_design_numbers() {
    let rules = load_rules();
    let gate = |i: usize| {
        let a = &rules.ages[i];
        let cost: Vec<(&str, u32)> = a.cost.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        (a.id.as_str(), a.techs_required, cost)
    };
    assert_eq!(rules.ages.len(), 3);
    assert_eq!(gate(0), ("hearth", 0, vec![]));
    assert_eq!(gate(1), ("masonry", 2, vec![("grain", 250), ("lore", 100)]));
    assert_eq!(
        gate(2),
        (
            "charter",
            5,
            vec![("grain", 500), ("lore", 300), ("lumber", 300)]
        )
    );
}

/// Acceptance 1, third clause: each Letters level cuts the next research's
/// cost (and time) by 10%, floored after each row. The probe is Arms I,
/// whose 75 Lore makes the floor visible (67.5 -> 67); Trade I and Arms II
/// show that the floor follows each row rather than one floor over 0.729
/// at the end; a real `Research` at Letters III deducts exactly the quote
/// and queues its time.
#[test]
fn tech_each_letters_level_cuts_the_next_research_cost_by_ten_percent_floored() {
    let rules = load_rules();
    let (ore, lore) = (res(&rules, "ore"), res(&rules, "lore"));
    let letters = ["letters_1", "letters_2", "letters_3"];
    for id in letters {
        let (_, t) = rules.tech_by_id(id).unwrap();
        let rows: Vec<(&StatPath, ModOp, i32)> = t
            .modifiers
            .iter()
            .map(|m| (&m.target, m.op, m.value))
            .collect();
        assert_eq!(
            rows,
            [
                (&StatPath::ResearchCost, ModOp::Mul, 900),
                (&StatPath::ResearchTime, ModOp::Mul, 900)
            ],
            "{id}"
        );
    }
    let arms = tech(&rules, "arms_1");
    let probe = rules.tech(arms.0).unwrap();
    assert_ne!(probe.cost["lore"] % 10, 0, "the floor must be visible");
    // `research_quote` prices a tech whatever its age and predecessor.
    let others = [tech(&rules, "trade_1"), tech(&rules, "arms_2")];
    let mut m = M::new(rules.clone(), 4);
    let mut quotes = vec![m.view().research_quote(P0, arms)];
    let mut other_quotes = vec![others.map(|t| m.view().research_quote(P0, t))];
    for id in letters {
        m.ok(vec![debug_research(tech(&rules, id))]);
        quotes.push(m.view().research_quote(P0, arms));
        other_quotes.push(others.map(|t| m.view().research_quote(P0, t)));
    }
    let column = |r: usize| quotes.iter().map(|(c, _)| c[r]).collect::<Vec<i64>>();
    assert_eq!(column(lore), [75, 67, 60, 54]);
    assert_eq!(column(ore), [50, 45, 40, 36]);
    for w in quotes.windows(2) {
        for r in 0..4 {
            assert_eq!(w[1].0[r], w[0].0[r] * 900 / 1000, "floor(prev * 0.9)");
        }
    }
    let ticks: Vec<u32> = quotes.iter().map(|(_, t)| *t).collect();
    let want: Vec<u32> = [300, 270, 243, 218]
        .map(|ds| rules.ticks_from_ds(ds))
        .into();
    assert_eq!(ticks, want, "300 -> 270 -> 243 -> 218 ds");
    for (level, (cost, t)) in quotes.iter().enumerate() {
        eprintln!(
            "letters {level}: arms_1 costs {} Lore + {} Ore, {t} ticks",
            cost[lore], cost[ore]
        );
    }
    // Arms I cannot tell a floor after each row from one floor over the
    // product at the end: 75, 50 and 300 times 0.729 (54.675, 36.45,
    // 218.7) floor to 54, 36 and 218 as well. Trade I and Arms II can: one
    // floor over 0.729 would end at 51 Lore (70 * 0.729 = 51.03), 109 Lore
    // (150 * 0.729 = 109.35) and 328 ds (450 * 0.729 = 328.05).
    let base = |t: TechId, r: &str| i64::from(rules.tech(t.0).unwrap().cost[r]);
    assert_eq!(base(others[0], "lore") * 729 / 1000, 51, "one floor");
    assert_eq!(base(others[1], "lore") * 729 / 1000, 109, "one floor");
    let trade_lore: Vec<i64> = other_quotes.iter().map(|q| q[0].0[lore]).collect();
    assert_eq!(
        trade_lore,
        [70, 63, 56, 50],
        "Trade I, floored after each row"
    );
    let arms_2: Vec<(i64, i64, u32)> = other_quotes
        .iter()
        .map(|q| (q[1].0[lore], q[1].0[ore], q[1].1))
        .collect();
    let want: Vec<(i64, i64, u32)> = [
        (150, 100, 450),
        (135, 90, 405),
        (121, 81, 364),
        (108, 72, 327),
    ]
    .map(|(l, o, ds)| (l, o, rules.ticks_from_ds(ds)))
    .into();
    assert_eq!(arms_2, want, "Arms II: 450 -> 405 -> 364 -> 327 ds");
    for (level, (trade_lore, (lore_2, ore_2, t))) in trade_lore.iter().zip(&arms_2).enumerate() {
        eprintln!(
            "letters {level}: trade_1 costs {trade_lore} Lore; arms_2 costs {lore_2} Lore + {ore_2} Ore, {t} ticks"
        );
    }
    for w in other_quotes.windows(2) {
        for (prev, next) in w[0].iter().zip(&w[1]) {
            for r in 0..4 {
                assert_eq!(next.0[r], prev.0[r] * 900 / 1000, "floor(prev * 0.9)");
            }
        }
    }
    // A real Research at Letters III deducts exactly the quote.
    let setup = || {
        let mut m = lab(&rules, 4);
        m.ok(letters.map(|id| debug_research(tech(&rules, id))).into());
        m
    };
    let mut a = setup();
    let mut b = setup();
    let (cost, t) = a.view().research_quote(P0, arms);
    assert_eq!(
        a.view().check_research(P0, BuildingId(1), arms),
        Ok((cost.clone(), t))
    );
    a.ok(vec![research(1, arms)]);
    b.run(vec![noop()]);
    let paid: Vec<i64> = b
        .stock(P0)
        .iter()
        .zip(a.stock(P0))
        .map(|(b, a)| b - a)
        .collect();
    assert_eq!(paid, cost);
    assert_eq!(
        a.view().building(BuildingId(1)).unwrap().queue,
        [QueueEntry {
            item: QueueItem::Research {
                tech: arms,
                ticks: t
            },
            paid: cost,
            progress: 1,
        }]
    );
}

/// Acceptance 1, fourth clause: Set -> Add -> Mul with a floor after each
/// `Mul`, through `rules::modifiers::resolve`, then in the sim.
#[test]
fn tech_modifier_stacking_set_add_mul_produces_the_documented_values() {
    use rules::modifiers::resolve;
    let row = |op: ModOp, value: i32| Modifier {
        target: StatPath::YieldCap,
        op,
        value,
    };
    let (set, add, mul) = (
        |v| row(ModOp::Set, v),
        |v| row(ModOp::Add, v),
        |v| row(ModOp::Mul, v),
    );
    // The worked examples in tech-and-ages.md and economy.md.
    assert_eq!(resolve(70, &[mul(1250)]), 87);
    assert_eq!(resolve(70, &[set(100), mul(1250)]), 125);
    assert_eq!(resolve(25, &[add(25), add(25)]), 75);
    assert_eq!(resolve(100, &[mul(900)]), 90);
    assert_eq!(resolve(100, &[mul(900), mul(900)]), 81);
    assert_eq!(resolve(100, &[mul(900), mul(900), mul(900)]), 72);
    // A floor after each row: 150 -> 135 -> 121 -> 108; one floor over the
    // product would give 109 (150 * 0.729 = 109.35).
    assert_eq!(resolve(150, &[mul(900), mul(900), mul(900)]), 108);
    assert_eq!(resolve(2, &[add(1)]), 3);
    assert_eq!(resolve(70, &[add(10), mul(1250)]), 100);
    assert_eq!(resolve(70, &[set(100), add(10)]), 110);
    assert_eq!(resolve(100, &[mul(1100), mul(1100)]), 121);
    // Set applies first, then Add, then Mul, in any row order (one Set,
    // one Add, one Mul): (100 + 10) * 1.25 = 137.5 -> 137.
    let rows = [set(100), add(10), mul(1250)];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let permuted: Vec<&Modifier> = order.iter().map(|&i| &rows[i]).collect();
        assert_eq!(resolve(70, permuted.iter().copied()), 137, "{order:?}");
    }
    // Several Sets: the last in collection order wins.
    assert_eq!(resolve(70, &[set(100), set(150)]), 150);
    assert_eq!(resolve(70, &[set(150), set(100)]), 100);
    // The floor is div_euclid, not truncation: -1 * 1.5 = -1.5 -> -2.
    assert_eq!(resolve(1, &[add(-2), mul(1500)]), -2);
    // In the sim: a faction Mul 1250 and Trade I's Set 100 give 125.
    let mut rules = load_rules();
    rules.factions[0].modifiers.push(mul(1250));
    let grain = res(&rules, "grain");
    let mut m = M::new(rules.clone(), 5);
    assert_eq!(m.view().income(P0)[grain].cap, Some(87));
    m.ok(vec![debug_research(tech(&rules, "trade_1"))]);
    assert_eq!(m.view().income(P0)[grain].cap, Some(125));
    // Pop cap 25 + Arms I and II = 75; town limit 2 + Statecraft I = 3.
    let rules = load_rules();
    let mut m = M::new(rules.clone(), 5);
    m.ok(vec![
        debug_research(tech(&rules, "arms_1")),
        debug_research(tech(&rules, "arms_2")),
        debug_research(tech(&rules, "statecraft_1")),
    ]);
    assert_eq!(m.view().pop(P0).1, 75);
    assert_eq!(m.view().towns(P0).1, 3);
}

/// Acceptance 1, last clause: a faction row `YieldCap Mul 1250` (the M9
/// Tidewater League example, pushed into `factions[0]` in memory) gives a
/// Yield Cap of 87 at Trade level 0: with ten worked Crofts (raw 120) the
/// Grain stockpile grows by exactly 87 per income period.
#[test]
fn tech_faction_yield_cap_mul_1250_gives_cap_87_at_trade_level_0() {
    let mut rules = load_rules();
    rules.factions[0].modifiers.push(Modifier {
        target: StatPath::YieldCap,
        op: ModOp::Mul,
        value: 1250,
    });
    let grain = res(&rules, "grain");
    let (town, croft) = (bkind(&rules, "town"), bkind(&rules, "croft"));
    let period = rules.income_period_ticks();
    let mut m = M::new(rules.clone(), 6);
    assert_eq!(m.view().income(P0)[grain].cap, Some(87));
    assert_eq!(m.view().income(P1)[grain].cap, Some(87), "every player");
    // As in economy_ten_worked_crofts_flatline_at_the_yield_cap: two Towns
    // (BuildingId 1, 2), ten Crofts (3..=12), ten Yeomen (UnitId 1..=10).
    let mut cmds = vec![place(town, 23, 63, true), place(town, 40, 63, true)];
    let sites = [28u16, 31, 34, 37, 40, 43]
        .map(|x| (x, 58u16))
        .into_iter()
        .chain([28, 31, 34, 37].map(|x| (x, 60)));
    for (x, y) in sites {
        cmds.push(place(croft, x, y, true));
    }
    cmds.push(spawn(ukind(&rules, "yeoman"), 32, 67, 10));
    m.ok(cmds);
    let gathers = (0..10u32)
        .map(|i| Command::Gather {
            units: vec![UnitId(1 + i)],
            node: BuildingId(3 + i),
        })
        .collect();
    m.ok(gathers);
    let worked = |m: &M| -> u32 {
        (3..=12)
            .map(|b| u32::from(m.view().worked_slots(BuildingId(b)).unwrap().0))
            .sum()
    };
    let mut waited = 0;
    while worked(&m) < 10 {
        m.step(1);
        waited += 1;
        assert!(waited < 2000, "the Yeomen reach their Crofts");
    }
    let info = m.view().income(P0)[grain];
    assert!(info.raw > 87, "raw {} must exceed the cap", info.raw);
    assert_eq!((info.cap, info.credited), (Some(87), 87));
    for period_no in 0..3 {
        let before = m.stock(P0)[grain];
        m.step(period);
        let gained = m.stock(P0)[grain] - before;
        eprintln!("faction Mul 1250, Trade 0: period {period_no} +{gained} Grain");
        assert_eq!(gained, 87);
    }
}

/// A research entry sits in its host's queue and completes exactly its
/// fixed number of ticks after the step that queued it began it.
#[test]
fn tech_research_completes_at_the_host_after_its_time() {
    let rules = load_rules();
    let letters = tech(&rules, "letters_1");
    let mut m = lab(&rules, 7);
    let (_, ticks) = m.view().research_quote(P0, letters);
    assert_eq!(ticks, rules.ticks_from_ds(250));
    m.ok(vec![research(1, letters)]);
    // Applied (and given its first tick) in the last step of `ok`.
    let started = m.sim.tick() - 1;
    let head = &m.view().building(BuildingId(1)).unwrap().queue[0];
    assert_eq!(
        head.item,
        QueueItem::Research {
            tech: letters,
            ticks
        }
    );
    assert_eq!(head.progress, 1);
    let events = m.step(ticks - 2);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SimEvent::ResearchCompleted { .. }))
    );
    assert!(m.view().researched(P0).is_empty());
    assert_eq!(
        m.view().building(BuildingId(1)).unwrap().queue[0].progress,
        ticks - 1
    );
    let events = m.step(1);
    assert_eq!(m.sim.tick() - 1, started + ticks - 1, "the ticks-th step");
    assert!(events.contains(&SimEvent::ResearchCompleted {
        player: P0,
        tech: letters
    }));
    assert!(m.view().researched(P0).contains(&letters));
    assert!(m.view().building(BuildingId(1)).unwrap().queue.is_empty());
}

/// Every `Research` rejection in check order; each changes nothing.
#[test]
fn tech_research_rejections_change_nothing() {
    let rules = load_rules();
    let t = |id| tech(&rules, id);
    let scribe = ukind(&rules, "scribe");
    let base = || {
        // BuildingId(1) complete Scriptorium (lab), (2) a Scriptorium
        // site, (3) a complete Muster Hall, (4) P1's Scriptorium.
        let mut m = lab(&rules, 8);
        let s = bkind(&rules, "scriptorium");
        m.ok(vec![
            place(s, SCRIPTORIUM_B.0, SCRIPTORIUM_B.1, false),
            place(
                bkind(&rules, "muster_hall"),
                MUSTER_HALL.0,
                MUSTER_HALL.1,
                true,
            ),
            place_for(P1, s, 16, 26, true),
        ]);
        m
    };
    type Setup<'a> = Box<dyn Fn() -> M + 'a>;
    let cases: Vec<(&str, Setup, Command, RejectReason)> = vec![
        (
            "no such building",
            Box::new(base),
            research(99, t("letters_1")),
            RejectReason::UnknownBuilding,
        ),
        (
            "another player's Scriptorium",
            Box::new(base),
            research(4, t("letters_1")),
            RejectReason::UnknownBuilding,
        ),
        (
            "a site",
            Box::new(base),
            research(2, t("letters_1")),
            RejectReason::UnderConstruction,
        ),
        (
            "no such tech",
            Box::new(base),
            research(1, TechId(99)),
            RejectReason::UnknownTech,
        ),
        (
            "at a Muster Hall",
            Box::new(base),
            research(3, t("letters_1")),
            RejectReason::CannotResearchHere,
        ),
        (
            "level 2 without level 1, in the Hearth Age",
            Box::new(base),
            research(1, t("letters_2")),
            RejectReason::PrerequisiteMissing,
        ),
        (
            "level 2 in the Hearth Age",
            Box::new(|| {
                let mut m = base();
                m.ok(vec![debug_research(t("letters_1"))]);
                m
            }),
            research(1, t("letters_2")),
            RejectReason::AgeRequired,
        ),
        (
            "already researched",
            Box::new(|| {
                let mut m = base();
                m.ok(vec![debug_research(t("letters_1"))]);
                m
            }),
            research(1, t("letters_1")),
            RejectReason::AlreadyResearched,
        ),
        (
            "queued at the same Scriptorium",
            Box::new(|| {
                let mut m = base();
                m.ok(vec![research(1, t("letters_1"))]);
                m
            }),
            research(1, t("letters_1")),
            RejectReason::ResearchInProgress,
        ),
        (
            "queued at another own Scriptorium",
            Box::new(|| {
                let mut m = base();
                let s = bkind(&rules, "scriptorium");
                m.ok(vec![place(s, 4, 32, true)]);
                m.ok(vec![research(5, t("letters_1"))]);
                m
            }),
            research(1, t("letters_1")),
            RejectReason::ResearchInProgress,
        ),
        (
            "a full queue",
            Box::new(|| {
                let mut m = base();
                m.ok(vec![train(1, scribe); 5]);
                m
            }),
            research(1, t("letters_1")),
            RejectReason::QueueFull,
        ),
        (
            "no Lore",
            Box::new(|| {
                let mut m = M::new(rules.clone(), 8);
                let s = bkind(&rules, "scriptorium");
                m.ok(vec![place(s, SCRIPTORIUM_A.0, SCRIPTORIUM_A.1, true)]);
                m
            }),
            research(1, t("letters_1")),
            RejectReason::CannotAfford {
                resource: res_u8(&rules, "lore"),
            },
        ),
        (
            "no Ore (the first short resource)",
            Box::new(|| {
                let mut m = M::new(rules.clone(), 8);
                let s = bkind(&rules, "scriptorium");
                m.ok(vec![
                    place(s, SCRIPTORIUM_A.0, SCRIPTORIUM_A.1, true),
                    grant(res(&rules, "lore"), 999),
                ]);
                m
            }),
            research(1, t("arms_1")),
            RejectReason::CannotAfford {
                resource: res_u8(&rules, "ore"),
            },
        ),
    ];
    for (name, setup, cmd, want) in cases {
        rejected_without_change(name, &*setup, 0, cmd, want);
    }
}

/// Every `AdvanceAge` rejection in check order; each changes nothing.
#[test]
fn tech_advance_age_rejections_change_nothing() {
    let rules = load_rules();
    let empty = || M::new(rules.clone(), 9);
    rejected_without_change(
        "naming another player (checked first)",
        &empty,
        0,
        Command::AdvanceAge { player: P1 },
        RejectReason::PlayerMismatch,
    );
    rejected_without_change(
        "naming an unknown player",
        &empty,
        0,
        Command::AdvanceAge {
            player: PlayerId(7),
        },
        RejectReason::PlayerMismatch,
    );
    rejected_without_change(
        "no tech and no stock (techs checked before cost)",
        &empty,
        0,
        advance(),
        RejectReason::NotEnoughTechs,
    );
    rejected_without_change(
        "two techs, no stock",
        &|| {
            let mut m = empty();
            m.ok(vec![
                debug_research(tech(&rules, "letters_1")),
                debug_research(tech(&rules, "trade_1")),
            ]);
            m
        },
        0,
        advance(),
        RejectReason::CannotAfford {
            resource: res_u8(&rules, "grain"),
        },
    );
    let at_charter = || {
        let mut m = empty();
        let mut cmds: Vec<Command> = rules.techs[..5]
            .iter()
            .map(|t| debug_research(tech(&rules, &t.id)))
            .collect();
        cmds.extend([
            grant(res(&rules, "grain"), 2000),
            grant(res(&rules, "lumber"), 2000),
            grant(res(&rules, "lore"), 999),
            advance(),
            advance(),
        ]);
        m.ok(cmds);
        assert_eq!(m.view().age(P0), 2);
        m
    };
    rejected_without_change(
        "the last age",
        &at_charter,
        0,
        advance(),
        RejectReason::FinalAge,
    );
    rejected_without_change(
        "the last age, naming another player",
        &at_charter,
        0,
        Command::AdvanceAge { player: P1 },
        RejectReason::PlayerMismatch,
    );
    rejected_without_change(
        "P1 has no tech",
        &at_charter,
        1,
        Command::AdvanceAge { player: P1 },
        RejectReason::NotEnoughTechs,
    );
}

/// Level k of a line needs level k-1 researched and age k (the data puts
/// level k in age k-1 counting from 0): every tech at every age, with all
/// four level-1 techs researched and with or without Letters II (so level
/// 2 with its predecessor, Letters III with and without its predecessor,
/// the other level-3 techs without it); then level 2 is rejected in the
/// Hearth Age and accepted in Masonry, and level 3 with its predecessor is
/// rejected in Masonry and accepted in Charter, by applying the command.
#[test]
fn tech_level_k_needs_level_k_minus_1_and_age_k() {
    let rules = load_rules();
    for (i, t) in rules.techs.iter().enumerate() {
        let i = u16::try_from(i).unwrap();
        assert_eq!(rules.age_of_tech(i), t.level - 1, "{}", t.id);
        assert_eq!(rules.tech_predecessor(i).is_some(), t.level > 1, "{}", t.id);
    }
    // A lab in each age, with Letters II or without it (`DebugResearch`
    // skips the age check); the Charter Age needs a fifth tech.
    let at_age = |age: u8, letters_2: bool| {
        let mut m = lab(&rules, 10);
        let mut ids = vec!["arms_1", "statecraft_1", "trade_1", "letters_1"];
        if letters_2 {
            ids.push("letters_2");
        }
        let mut cmds: Vec<Command> = ids
            .iter()
            .map(|id| debug_research(tech(&rules, id)))
            .collect();
        cmds.extend((0..age).map(|_| advance()));
        m.ok(cmds);
        m
    };
    let variants = [(0, false), (0, true), (1, false), (1, true), (2, true)];
    for (age, letters_2) in variants {
        let m = at_age(age, letters_2);
        assert_eq!(m.view().age(P0), age);
        for (i, t) in rules.techs.iter().enumerate() {
            let id = TechId(u16::try_from(i).unwrap());
            let researched = m.view().researched(P0).contains(&id);
            let pred_done = rules
                .tech_predecessor(id.0)
                .is_none_or(|p| m.view().researched(P0).contains(&TechId(p)));
            let got = m.view().check_research(P0, BuildingId(1), id).map(|_| ());
            let want = if !pred_done {
                Err(RejectReason::PrerequisiteMissing)
            } else if age < t.level - 1 {
                Err(RejectReason::AgeRequired)
            } else if researched {
                Err(RejectReason::AlreadyResearched)
            } else {
                Ok(())
            };
            assert_eq!(got, want, "{} at age {age}, {letters_2}", t.id);
        }
        // Letters III with its predecessor still waits for the Charter Age.
        let letters_3 = m
            .view()
            .check_research(P0, BuildingId(1), tech(&rules, "letters_3"));
        if letters_2 && age < 2 {
            assert_eq!(letters_3, Err(RejectReason::AgeRequired), "age {age}");
        }
    }
    // The checks are what apply does: level 2 is rejected in Hearth and
    // accepted once in Masonry; level 3 with its predecessor is rejected in
    // Masonry (changing nothing) and accepted in Charter.
    let (letters_2, letters_3) = (tech(&rules, "letters_2"), tech(&rules, "letters_3"));
    let mut m = at_age(0, false);
    let events = m.run(vec![research(1, letters_2)]);
    assert_eq!(rejections(&events), [RejectReason::AgeRequired]);
    let mut m = at_age(1, false);
    m.ok(vec![research(1, letters_2)]);
    rejected_without_change(
        "level 3 in the Masonry Age",
        &|| at_age(1, true),
        0,
        research(1, letters_3),
        RejectReason::AgeRequired,
    );
    let mut m = at_age(2, true);
    m.ok(vec![research(1, letters_3)]);
}

/// Trade I, II and III set the Yield Cap to 100, 150 and 200 for every
/// capped resource; Lore stays uncapped.
#[test]
fn tech_trade_sets_the_yield_cap_100_150_200() {
    let rules = load_rules();
    let mut m = M::new(rules.clone(), 11);
    let caps = |m: &M| -> Vec<Option<u32>> { m.view().income(P0).iter().map(|r| r.cap).collect() };
    let capped: Vec<bool> = rules
        .resources
        .resources
        .iter()
        .map(|r| r.yield_capped)
        .collect();
    let expect = |c: u32| -> Vec<Option<u32>> { capped.iter().map(|&y| y.then_some(c)).collect() };
    assert_eq!(caps(&m), expect(70));
    for (id, cap) in [("trade_1", 100), ("trade_2", 150), ("trade_3", 200)] {
        m.ok(vec![debug_research(tech(&rules, id))]);
        assert_eq!(caps(&m), expect(cap), "{id}");
        assert_eq!(m.view().income(P1)[0].cap, Some(70), "only P0's cap");
    }
    assert_eq!(caps(&m)[res(&rules, "lore")], None);
}

/// Statecraft I, II and III each add 1 to the town limit and 1 tile to the
/// radius of every own Town at once (a field change for each), not to
/// another player's; a Town completed afterwards projects the pushed
/// radius; the field always equals a full recompute.
#[test]
fn tech_statecraft_raises_the_town_limit_and_pushes_every_own_border() {
    let rules = load_rules();
    let town = bkind(&rules, "town");
    let r1 = u16::try_from(rules.town_radius_tiles).unwrap();
    let mut m = M::new(rules.clone(), 12);
    m.ok(vec![
        place(town, 6, 34, true),
        place(town, 30, 34, true),
        place_for(P1, town, 102, 63, true),
        grant(res(&rules, "lumber"), 1000),
        spawn(ukind(&rules, "yeoman"), 10, 38, 1),
    ]);
    let radius = |m: &M, b: u32| m.view().territory().sources()[&BuildingId(b)].radius;
    assert_eq!((radius(&m, 1), radius(&m, 2), radius(&m, 3)), (r1, r1, r1));
    assert_eq!(m.view().towns(P0), (2, 2));
    let third_town = |m: &M| m.view().check_build(P0, UnitId(1), town, Tile::new(14, 26));
    assert_eq!(third_town(&m), Err(RejectReason::TownLimitReached));
    assert_eq!(m.view().check_build_kind(P0, town), third_town(&m));
    let events = m.ok(vec![debug_research(tech(&rules, "statecraft_1"))]);
    assert_eq!(
        events
            .iter()
            .filter(|e| **e == SimEvent::TerritoryChanged)
            .count(),
        2,
        "one change per own Town"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SimEvent::TownTierChanged { .. }))
    );
    assert_eq!(radius(&m, 1), r1 + 1);
    assert_eq!(radius(&m, 2), r1 + 1);
    assert_eq!(radius(&m, 3), r1, "P1's Town is not pushed");
    assert_eq!(radius(&m, 1), 21);
    assert_eq!(m.view().towns(P0), (2, 3));
    assert!(third_town(&m).is_ok(), "{:?}", third_town(&m));
    assert_eq!(m.view().check_build_kind(P0, town), third_town(&m));
    assert_eq!(m.view().territory(), &m.view().territory().recomputed());
    m.ok(vec![place(town, 14, 38, true)]);
    assert_eq!(
        radius(&m, 4),
        r1 + 1,
        "a later Town projects the pushed radius"
    );
    m.ok(vec![debug_research(tech(&rules, "statecraft_2"))]);
    assert_eq!(
        (radius(&m, 1), radius(&m, 2), radius(&m, 4), radius(&m, 3)),
        (r1 + 2, r1 + 2, r1 + 2, r1)
    );
    assert_eq!(m.view().towns(P0), (3, 4));
    assert_eq!(m.view().territory(), &m.view().territory().recomputed());
    let events = m.ok(vec![debug_research(tech(&rules, "statecraft_3"))]);
    assert_eq!(
        events
            .iter()
            .filter(|e| **e == SimEvent::TerritoryChanged)
            .count(),
        3,
        "one change per own Town"
    );
    assert_eq!(
        (radius(&m, 1), radius(&m, 2), radius(&m, 4), radius(&m, 3)),
        (r1 + 3, r1 + 3, r1 + 3, r1)
    );
    assert_eq!(radius(&m, 1), 23);
    assert_eq!(m.view().towns(P0), (3, 5));
    assert_eq!(m.view().territory(), &m.view().territory().recomputed());
}

/// Each Arms level adds 25 to the pop cap, and `Train` honours it.
#[test]
fn tech_arms_raises_the_pop_cap() {
    let rules = load_rules();
    let yeoman = ukind(&rules, "yeoman");
    let mut m = M::new(rules.clone(), 13);
    assert_eq!(m.view().pop(P0), (0, 25));
    m.ok(vec![
        place(bkind(&rules, "town"), TOWN.0, TOWN.1, true),
        grant(res(&rules, "grain"), 5000),
        spawn(yeoman, 10, 36, 25),
    ]);
    assert_eq!(m.view().pop(P0), (25, 25));
    let events = m.run(vec![train(1, yeoman)]);
    assert_eq!(rejections(&events), [RejectReason::PopCapReached]);
    for (id, cap) in [("arms_1", 50), ("arms_2", 75), ("arms_3", 100)] {
        m.ok(vec![debug_research(tech(&rules, id))]);
        assert_eq!(m.view().pop(P0).1, cap, "{id}");
    }
    m.ok(vec![train(1, yeoman)]);
    assert_eq!(m.view().pop(P0), (26, 100));
    assert_eq!(m.view().pop(P1), (0, 25), "only P0's cap");
}

/// A unit kind's `requires_tech` (set in memory: the Outrider needs Arms I)
/// gates `Train` after the age check.
#[test]
fn tech_requires_tech_gates_training() {
    let mut rules = load_rules();
    let outrider = ukind(&rules, "outrider");
    rules.units[usize::from(outrider.0)].requires_tech = Some("arms_1".into());
    let setup = || {
        let mut m = M::new(rules.clone(), 14);
        m.ok(vec![
            place(
                bkind(&rules, "muster_hall"),
                MUSTER_HALL.0,
                MUSTER_HALL.1,
                true,
            ),
            grant(res(&rules, "grain"), 2000),
            grant(res(&rules, "ore"), 2000),
        ]);
        m
    };
    rejected_without_change(
        "Hearth Age: the age first",
        &setup,
        0,
        train(1, outrider),
        RejectReason::AgeRequired,
    );
    let masonry = || {
        let mut m = setup();
        reach_masonry(&mut m);
        m
    };
    rejected_without_change(
        "Masonry without Arms I",
        &masonry,
        0,
        train(1, outrider),
        RejectReason::PrerequisiteMissing,
    );
    let mut m = masonry();
    m.ok(vec![debug_research(tech(&rules, "arms_1"))]);
    m.ok(vec![train(1, outrider)]);
    // With the shipped data no kind names a tech.
    assert!(load_rules().units.iter().all(|u| u.requires_tech.is_none()));
}

/// The Masonry Age gates the Engine Yard, the Masonry units and Town tier
/// 2 (`town_growth_age`): in the Hearth Age a Town with five distinct
/// attributed kinds stays at tier 1, and grows at the `AdvanceAge`.
#[test]
fn tech_masonry_gates_engine_yard_outrider_and_town_tier_two() {
    let rules = load_rules();
    let (yard, hall) = (bkind(&rules, "engine_yard"), bkind(&rules, "muster_hall"));
    let outrider = ukind(&rules, "outrider");
    let (r1, r2) = (
        u16::try_from(rules.town_radius_tiles).unwrap(),
        u16::try_from(rules.town_radius_grown_tiles).unwrap(),
    );
    // Five distinct kinds at the Seat, the Engine Yard by debug placement
    // (which ignores ages): BuildingId 3..=7, the Muster Hall is 5.
    let setup = || {
        let mut m = M::standard(rules.clone(), 15);
        let kinds = [
            ("croft", 28u16, 58u16),
            ("scriptorium", 28, 67),
            ("muster_hall", 32, 67),
            ("engine_yard", 36, 67),
            ("watchtower", 31, 58),
        ];
        let events = m.ok(kinds
            .iter()
            .map(|(id, x, y)| place(bkind(&rules, id), *x, *y, true))
            .collect());
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SimEvent::TownTierChanged { .. }))
        );
        m.ok(vec![
            grant(res(&rules, "grain"), 1000),
            grant(res(&rules, "ore"), 1000),
            grant(res(&rules, "lumber"), 1000),
        ]);
        assert_eq!(
            m.view().building(BuildingId(5)).unwrap().kind,
            hall,
            "the Muster Hall"
        );
        m
    };
    let seat_tier = |m: &M| {
        m.view()
            .building(BuildingId(1))
            .unwrap()
            .town_state
            .unwrap()
            .tier
    };
    let seat_radius = |m: &M| m.view().territory().sources()[&BuildingId(1)].radius;
    let m = setup();
    assert_eq!((seat_tier(&m), seat_radius(&m)), (1, r1), "Hearth: tier 1");
    let yard_at = |m: &M| m.view().check_build(P0, UnitId(1), yard, Tile::new(19, 67));
    assert_eq!(yard_at(&m), Err(RejectReason::AgeRequired));
    assert_eq!(m.view().check_build_kind(P0, yard), yard_at(&m));
    rejected_without_change(
        "an Engine Yard in the Hearth Age",
        &setup,
        0,
        Command::Build {
            unit: UnitId(1),
            kind: yard,
            tile: Tile::new(19, 67),
        },
        RejectReason::AgeRequired,
    );
    rejected_without_change(
        "an Outrider in the Hearth Age",
        &setup,
        0,
        train(5, outrider),
        RejectReason::AgeRequired,
    );
    // A site placed earlier is resumed whatever the age.
    let mut m = setup();
    m.ok(vec![place(yard, 14, 58, false)]);
    m.ok(vec![Command::Build {
        unit: UnitId(2),
        kind: yard,
        tile: Tile::new(14, 58),
    }]);
    // Masonry: the Seat grows at the advance, and the gates open.
    let mut m = setup();
    m.ok(vec![
        debug_research(tech(&rules, "letters_1")),
        debug_research(tech(&rules, "arms_1")),
        grant(res(&rules, "lore"), 100),
    ]);
    let events = m.ok(vec![advance()]);
    assert!(events.contains(&SimEvent::AgeAdvanced { player: P0, age: 1 }));
    assert!(events.contains(&SimEvent::TownTierChanged {
        town: BuildingId(1),
        tier: 2
    }));
    assert!(events.contains(&SimEvent::TerritoryChanged));
    assert_eq!((seat_tier(&m), seat_radius(&m)), (2, r2));
    assert_eq!(m.view().territory(), &m.view().territory().recomputed());
    assert!(yard_at(&m).is_ok(), "{:?}", yard_at(&m));
    assert_eq!(m.view().check_build_kind(P0, yard), yard_at(&m));
    m.ok(vec![
        Command::Build {
            unit: UnitId(1),
            kind: yard,
            tile: Tile::new(19, 67),
        },
        train(5, outrider),
    ]);
    // With no age gate in rules.ron the Seat grows in the Hearth Age.
    let mut ungated = rules.clone();
    ungated.town_growth_age = None;
    let mut m = M::standard(ungated, 15);
    m.ok([
        ("croft", 28u16, 58u16),
        ("scriptorium", 28, 67),
        ("muster_hall", 32, 67),
        ("engine_yard", 36, 67),
        ("watchtower", 31, 58),
    ]
    .iter()
    .map(|(id, x, y)| place(bkind(&rules, id), *x, *y, true))
    .collect());
    assert_eq!((seat_tier(&m), seat_radius(&m)), (2, r2));
}

/// Cancelling research entries, the head with progress and one behind it,
/// refunds exactly what each paid when it was queued, not the cheaper
/// quote after a later Letters level, with Lore under its cap so nothing
/// is clamped: the state ends equal to a control that never queued them.
/// At the Lore cap a refund leaves the stockpile at the cap, as for a unit.
#[test]
fn tech_research_cancel_refunds_exactly_what_it_paid() {
    let rules = load_rules();
    let (arms, trade) = (tech(&rules, "arms_1"), tech(&rules, "trade_1"));
    let lore = res(&rules, "lore");
    let cap = i64::from(rules.resources.resources[lore].stockpile_cap.unwrap());
    let setup = || {
        let mut m = M::new(rules.clone(), 16);
        m.ok(vec![
            place(
                bkind(&rules, "scriptorium"),
                SCRIPTORIUM_A.0,
                SCRIPTORIUM_A.1,
                true,
            ),
            grant(res(&rules, "ore"), 2000),
            // Well under Lore's stockpile_cap (999): the cap must not be
            // able to hide a wrong Lore refund.
            grant(lore, 400),
        ]);
        m
    };
    let mut a = setup();
    let mut b = setup();
    let quotes = (
        a.view().research_quote(P0, arms),
        a.view().research_quote(P0, trade),
    );
    let before = a.stock(P0);
    a.ok(vec![research(1, arms), research(1, trade)]);
    b.run(vec![noop(), noop()]);
    let queue = &a.view().building(BuildingId(1)).unwrap().queue;
    let paid = [queue[0].paid.clone(), queue[1].paid.clone()];
    assert_eq!(paid, [quotes.0.0.clone(), quotes.1.0.clone()]);
    let spent: Vec<i64> = b
        .stock(P0)
        .iter()
        .zip(a.stock(P0))
        .map(|(b, a)| b - a)
        .collect();
    let both: Vec<i64> = paid[0].iter().zip(&paid[1]).map(|(x, y)| x + y).collect();
    assert_eq!(spent, both);
    assert!(before.iter().zip(&both).all(|(s, c)| s >= c));
    // Letters I after queueing: the quotes drop (75 Lore + 50 Ore to 67 +
    // 45, 70 Lore to 63); what the entries paid must not.
    let letters = tech(&rules, "letters_1");
    a.ok(vec![debug_research(letters)]);
    b.ok(vec![debug_research(letters)]);
    assert_ne!(a.view().research_quote(P0, arms).0, paid[0]);
    assert_ne!(a.view().research_quote(P0, trade).0, paid[1]);
    a.step(100);
    b.step(100);
    assert!(a.view().building(BuildingId(1)).unwrap().queue[0].progress > 100);
    let cancel = |slot| Command::Cancel {
        building: BuildingId(1),
        slot,
    };
    let (a_pre, b_pre) = (a.stock(P0), b.stock(P0));
    let events = a.ok(vec![cancel(1), cancel(0)]);
    b.run(vec![noop(), noop()]);
    assert!(events.contains(&SimEvent::TrainingCancelled {
        building: BuildingId(1),
        slot: 1
    }));
    assert!(events.contains(&SimEvent::TrainingCancelled {
        building: BuildingId(1),
        slot: 0
    }));
    assert!(a.view().building(BuildingId(1)).unwrap().queue.is_empty());
    assert!(
        a.stock(P0)[lore] < cap && b.stock(P0)[lore] < cap,
        "the cap would hide a wrong Lore refund"
    );
    // What came back is exactly what both entries paid, plus the income
    // both sims earned in the cancel's ticks.
    let delta = |now: Vec<i64>, pre: &[i64]| -> Vec<i64> {
        now.iter().zip(pre).map(|(x, y)| x - y).collect()
    };
    let income = delta(b.stock(P0), &b_pre);
    let want: Vec<i64> = both.iter().zip(&income).map(|(p, i)| p + i).collect();
    assert_eq!(
        delta(a.stock(P0), &a_pre),
        want,
        "exactly the queue-time paid amounts back"
    );
    assert_eq!(a.stock(P0), b.stock(P0));
    assert_eq!(a.sim.sub_hashes(), b.sim.sub_hashes());
    assert_eq!(
        a.view().researched(P0).len(),
        1,
        "only the debug Letters I: nothing completed"
    );
    // At the cap: the stockpile stays at the cap, not cap + paid.
    let mut a = lab(&rules, 16);
    let mut b = lab(&rules, 16);
    a.ok(vec![research(1, trade)]);
    b.run(vec![noop()]);
    let paid = a.view().building(BuildingId(1)).unwrap().queue[0].paid[lore];
    a.ok(vec![grant(lore, u32::try_from(paid).unwrap())]);
    b.ok(vec![grant(lore, u32::try_from(paid).unwrap())]);
    assert_eq!((a.stock(P0)[lore], b.stock(P0)[lore]), (cap, cap));
    let events = a.ok(vec![cancel(0)]);
    b.run(vec![noop()]);
    assert!(events.contains(&SimEvent::TrainingCancelled {
        building: BuildingId(1),
        slot: 0
    }));
    assert_eq!(a.stock(P0)[lore], cap, "clamped, not {}", cap + paid);
    assert_eq!(a.stock(P0), b.stock(P0));
    assert_eq!(a.sim.sub_hashes(), b.sim.sub_hashes());
}

/// Research time is fixed when the entry is queued: Letters researched
/// while an entry waits speeds only entries queued afterwards.
#[test]
fn tech_research_time_is_fixed_when_queued() {
    let rules = load_rules();
    let (arms, trade) = (tech(&rules, "arms_1"), tech(&rules, "trade_1"));
    let mut m = lab(&rules, 17);
    let slow = m.view().research_quote(P0, arms).1;
    assert_eq!(slow, rules.ticks_from_ds(300));
    m.ok(vec![research(1, arms)]);
    let started = m.sim.tick() - 1;
    m.ok(vec![debug_research(tech(&rules, "letters_1"))]);
    let fast = m.view().research_quote(P0, trade).1;
    assert_eq!(fast, rules.ticks_from_ds(270));
    m.ok(vec![research(1, trade)]);
    let queue = &m.view().building(BuildingId(1)).unwrap().queue;
    assert_eq!(
        queue.iter().map(|e| e.item).collect::<Vec<_>>(),
        [
            QueueItem::Research {
                tech: arms,
                ticks: slow
            },
            QueueItem::Research {
                tech: trade,
                ticks: fast
            }
        ]
    );
    // Arms I completes in the slow-th step from its first; Trade I starts
    // in the next step and completes fast steps later.
    let mut done = Vec::new();
    while done.len() < 2 {
        for e in m.step(1) {
            if let SimEvent::ResearchCompleted { tech, .. } = e {
                done.push((tech, m.sim.tick() - 1));
            }
        }
        assert!(m.sim.tick() < started + 3000);
    }
    assert_eq!(
        done,
        [
            (arms, started + slow - 1),
            (trade, started + slow + fast - 1)
        ]
    );
}

/// Research entries take no pop and count toward no unit ramp, even for a
/// kind (here the Scribe, made Military in memory) trained at the host.
#[test]
fn tech_queued_research_takes_no_pop_and_no_military_ramp() {
    let mut rules = load_rules();
    let scribe = ukind(&rules, "scribe");
    rules.units[usize::from(scribe.0)].ramp = UnitRamp::Military;
    let setup = || {
        let mut m = lab(&rules, 18);
        let s = bkind(&rules, "scriptorium");
        m.ok(vec![place(s, SCRIPTORIUM_B.0, SCRIPTORIUM_B.1, true)]);
        m.ok(vec![train(2, scribe)]);
        m
    };
    let mut a = setup();
    let mut b = setup();
    a.ok(vec![
        research(1, tech(&rules, "letters_1")),
        research(1, tech(&rules, "trade_1")),
        research(2, tech(&rules, "arms_1")),
    ]);
    b.run(vec![noop(), noop(), noop()]);
    assert_eq!(a.view().building(BuildingId(1)).unwrap().queue.len(), 2);
    assert_eq!(a.view().pop(P0), b.view().pop(P0));
    assert_eq!(a.view().pop(P0).0, 1, "the queued Scribe only");
    for (k, u) in rules.units.iter().enumerate() {
        let k = UnitKindId(u16::try_from(k).unwrap());
        assert_eq!(
            a.view().unit_cost(P0, k),
            b.view().unit_cost(P0, k),
            "{}",
            u.id
        );
    }
    // The queued Scribe does raise the (in-memory) ramp: the check works.
    let mut c = setup();
    let before = c.view().unit_cost(P0, scribe);
    c.ok(vec![train(2, scribe)]);
    assert_ne!(c.view().unit_cost(P0, scribe), before);
}

/// `DebugResearch` completes a tech at once, but never one that is
/// researched or queued, and only when debug commands are on.
#[test]
fn tech_debug_research_rejects_a_queued_tech() {
    let rules = load_rules();
    let letters = tech(&rules, "letters_1");
    let queued = || {
        let mut m = lab(&rules, 19);
        m.ok(vec![research(1, letters)]);
        m
    };
    rejected_without_change(
        "queued",
        &queued,
        0,
        debug_research(letters),
        RejectReason::ResearchInProgress,
    );
    rejected_without_change(
        "researched",
        &|| {
            let mut m = M::new(rules.clone(), 19);
            m.ok(vec![debug_research(letters)]);
            m
        },
        0,
        debug_research(letters),
        RejectReason::AlreadyResearched,
    );
    rejected_without_change(
        "unknown tech",
        &|| M::new(rules.clone(), 19),
        0,
        debug_research(TechId(99)),
        RejectReason::UnknownTech,
    );
    rejected_without_change(
        "unknown owner",
        &|| M::new(rules.clone(), 19),
        0,
        Command::DebugResearch {
            owner: PlayerId(7),
            tech: letters,
        },
        RejectReason::UnknownPlayer,
    );
    // Debug commands off (a skirmish): rejected, changing nothing.
    let skirmish = || M {
        sim: Sim::new(
            MatchSetup::skirmish(&rules, 19),
            rules.clone(),
            Box::new(NoAi),
        ),
        seq: [0; 2],
    };
    rejected_without_change(
        "debug commands off",
        &skirmish,
        0,
        debug_research(letters),
        RejectReason::DebugCommandsDisabled,
    );
    let mut m = skirmish();
    m.run(vec![debug_research(letters)]);
    assert!(m.view().researched(P0).is_empty());
    // The queued one completes exactly once, at its host.
    let mut m = queued();
    let (_, ticks) = m.view().research_quote(P0, letters);
    let events = m.step(ticks);
    let completions = events
        .iter()
        .filter(|e| matches!(e, SimEvent::ResearchCompleted { .. }))
        .count();
    assert_eq!(completions, 1);
    assert!(m.view().researched(P0).contains(&letters));
    // DebugResearch skips host, cost, prerequisite and age.
    let mut m = M::new(rules.clone(), 19);
    let events = m.ok(vec![debug_research(tech(&rules, "trade_3"))]);
    assert!(events.contains(&SimEvent::ResearchCompleted {
        player: P0,
        tech: tech(&rules, "trade_3")
    }));
    assert_eq!(m.stock(P0), vec![0; 4]);
}

/// Snapshot, restore into a fresh sim and keep going: every later tick
/// hashes the same as the uninterrupted run, every sub-hash included. The
/// first restore has research half done and no tech complete, and goes
/// through an `AdvanceAge`, further research and a queue `Cancel`. The
/// second comes after the `AdvanceAge`, with Letters I and Trade I
/// researched and Arms I in flight, before the `Research` of Letters II,
/// which needs the restored age, the restored Letters I and its price.
#[test]
fn tech_restore_mid_research_matches_uninterrupted() {
    let rules = load_rules();
    let t = |id| tech(&rules, id);
    let s = bkind(&rules, "scriptorium");
    let cmd = |seq: u32, c: Command| PlayerCommand::new(P0, seq, c);
    let stream: Vec<(u32, Vec<PlayerCommand>)> = vec![
        (
            0,
            vec![
                cmd(0, place(s, SCRIPTORIUM_A.0, SCRIPTORIUM_A.1, true)),
                cmd(1, place(s, SCRIPTORIUM_B.0, SCRIPTORIUM_B.1, true)),
                cmd(2, grant(res(&rules, "grain"), 2000)),
                cmd(3, grant(res(&rules, "lumber"), 2000)),
                cmd(4, grant(res(&rules, "ore"), 2000)),
                cmd(5, grant(res(&rules, "lore"), 999)),
            ],
        ),
        (
            5,
            vec![
                cmd(6, research(1, t("letters_1"))),
                cmd(7, research(2, t("trade_1"))),
                cmd(8, research(1, t("arms_1"))),
            ],
        ),
        (650, vec![cmd(9, advance())]),
        (
            700,
            vec![
                cmd(10, research(2, t("letters_2"))),
                cmd(11, research(2, t("statecraft_1"))),
            ],
        ),
        (
            800,
            vec![cmd(
                12,
                Command::Cancel {
                    building: BuildingId(2),
                    slot: 1,
                },
            )],
        ),
    ];
    let at = |tick: u32| -> Vec<PlayerCommand> {
        stream
            .iter()
            .find(|(t, _)| *t == tick)
            .map(|(_, c)| c.clone())
            .unwrap_or_default()
    };
    let letters_2 = t("letters_2");
    for snap in [300u32, 680] {
        let mut a = M::new(rules.clone(), 20).sim;
        for tick in 0..snap {
            a.step(&at(tick));
        }
        a.drain_events();
        let q = &a.view().building(BuildingId(1)).unwrap().queue;
        if snap == 300 {
            assert!(q[0].progress > 100 && q.len() == 2, "research half done");
            assert!(a.view().researched(P0).is_empty());
        } else {
            // The AdvanceAge applied during tick 652.
            assert!(q[0].progress > 100 && q.len() == 1, "Arms I in flight");
            assert_eq!(a.view().age(P0), 1);
            assert_eq!(
                a.view().researched(P0),
                &std::collections::BTreeSet::from([t("letters_1"), t("trade_1")])
            );
            assert!(
                a.view()
                    .check_research(P0, BuildingId(2), letters_2)
                    .is_ok()
            );
        }
        let mut b = M::new(rules.clone(), 20).sim;
        b.restore(&a.snapshot()).unwrap();
        assert_eq!(a.hash(), b.hash());
        assert_eq!(a.sub_hashes(), b.sub_hashes());
        for p in [P0, P1] {
            assert_eq!(b.view().age(p), a.view().age(p), "restore at {snap}");
            assert_eq!(
                b.view().researched(p),
                a.view().researched(p),
                "restore at {snap}"
            );
            assert_eq!(
                b.view().research_quote(p, letters_2),
                a.view().research_quote(p, letters_2),
                "restore at {snap}: the Letters price"
            );
            assert_eq!(
                b.view().check_research(p, BuildingId(2), letters_2),
                a.view().check_research(p, BuildingId(2), letters_2),
                "restore at {snap}"
            );
        }
        let mut advanced = false;
        for tick in snap..1500 {
            let cmds = at(tick);
            a.step(&cmds);
            b.step(&cmds);
            assert_eq!(a.hash(), b.hash(), "restore at {snap}, tick {tick}");
            assert_eq!(
                a.sub_hashes(),
                b.sub_hashes(),
                "restore at {snap}, tick {tick}"
            );
            advanced |= a
                .drain_events()
                .contains(&SimEvent::AgeAdvanced { player: P0, age: 1 });
            b.drain_events();
        }
        assert_eq!(
            advanced,
            snap == 300,
            "the stream advances an age after the first restore only"
        );
        let done: Vec<TechId> = a.view().researched(P0).iter().copied().collect();
        assert_eq!(
            done,
            [t("arms_1"), t("trade_1"), t("letters_1"), letters_2],
            "statecraft_1 was cancelled"
        );
        assert_eq!(b.view().researched(P0), a.view().researched(P0));
    }
}

/// `SimView::check_research` and `check_advance_age` return exactly what
/// applying the command does: the rejection reason, or the quote it
/// deducts (and for research the ticks it queues).
#[test]
fn tech_view_checks_agree_with_apply() {
    let rules = load_rules();
    let t = |id| tech(&rules, id);
    type Setup<'a> = Box<dyn Fn() -> M + 'a>;
    let lab_ = || lab(&rules, 21);
    let lettered: Setup = Box::new(|| {
        let mut m = lab(&rules, 21);
        m.ok(vec![
            debug_research(t("letters_1")),
            debug_research(t("trade_1")),
        ]);
        m
    });
    let surrendered: Setup = Box::new(|| {
        let mut m = lab(&rules, 21);
        m.run_as(1, vec![Command::Surrender]);
        m
    });
    let research_cases: Vec<(&str, &Setup, u8, u32, TechId)> = vec![
        ("fresh", &lettered, 0, 1, t("arms_1")),
        ("already", &lettered, 0, 1, t("letters_1")),
        ("age", &lettered, 0, 1, t("letters_2")),
        ("no building", &lettered, 0, 9, t("arms_1")),
        ("unknown tech", &lettered, 0, 1, TechId(400)),
        ("unknown player", &lettered, 7, 1, t("arms_1")),
        ("surrendered", &surrendered, 1, 1, t("arms_1")),
    ];
    for (name, setup, p, b, tech) in research_cases {
        let mut m = setup();
        let mut control = setup();
        let check = m.view().check_research(PlayerId(p), BuildingId(b), tech);
        let events = m.run_as(p, vec![research(b, tech)]);
        control.run_as(p, vec![noop()]);
        match check {
            Err(reason) => assert_eq!(rejections(&events), [reason], "{name}"),
            Ok((cost, ticks)) => {
                assert!(rejections(&events).is_empty(), "{name}: {events:?}");
                let pid = PlayerId(p);
                let paid: Vec<i64> = control
                    .stock(pid)
                    .iter()
                    .zip(m.stock(pid))
                    .map(|(c, s)| c - s)
                    .collect();
                assert_eq!(paid, cost, "{name}");
                let entry = m
                    .view()
                    .building(BuildingId(b))
                    .unwrap()
                    .queue
                    .last()
                    .cloned();
                assert_eq!(
                    entry.map(|e| e.item),
                    Some(QueueItem::Research { tech, ticks }),
                    "{name}"
                );
            }
        }
    }
    let rich_two: Setup = Box::new(|| {
        let mut m = lettered();
        m.ok(vec![grant(res(&rules, "grain"), 300)]);
        m
    });
    let plain_lab: Setup = Box::new(lab_);
    let advance_cases: Vec<(&str, &Setup, u8)> = vec![
        ("affordable", &rich_two, 0),
        ("P1 without techs", &rich_two, 1),
        ("no techs", &plain_lab, 0),
        ("unknown player", &rich_two, 7),
        ("surrendered", &surrendered, 1),
    ];
    for (name, setup, p) in advance_cases {
        let mut m = setup();
        let mut control = setup();
        let pid = PlayerId(p);
        let check = m.view().check_advance_age(pid);
        let events = m.run_as(p, vec![Command::AdvanceAge { player: pid }]);
        control.run_as(p, vec![noop()]);
        match check {
            Err(reason) => assert_eq!(rejections(&events), [reason], "{name}"),
            Ok(cost) => {
                assert!(rejections(&events).is_empty(), "{name}: {events:?}");
                let paid: Vec<i64> = control
                    .stock(pid)
                    .iter()
                    .zip(m.stock(pid))
                    .map(|(c, s)| c - s)
                    .collect();
                assert_eq!(paid, cost, "{name}");
                assert_eq!(m.view().age(pid), control.view().age(pid) + 1, "{name}");
            }
        }
    }
    // A site is UnderConstruction in both.
    let mut m = lab_();
    m.ok(vec![place(
        bkind(&rules, "scriptorium"),
        SCRIPTORIUM_B.0,
        SCRIPTORIUM_B.1,
        false,
    )]);
    assert_eq!(
        m.view().check_research(P0, BuildingId(2), t("letters_1")),
        Err(RejectReason::UnderConstruction)
    );
    let events = m.run(vec![research(2, t("letters_1"))]);
    assert_eq!(rejections(&events), [RejectReason::UnderConstruction]);
}
