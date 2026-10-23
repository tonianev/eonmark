//! M5b bot tests that run whole matches (`cargo test -p ai`). The costly
//! ones (several games, or games longer than 5000 ticks) are `#[ignore]`d
//! with an `m5b_` prefix and run in CI with
//! `cargo test -p ai --locked -- --ignored m5b_`. One planned behaviour
//! test stays in the default run although its game is longer:
//! `no_unit_gets_the_same_order_twice_in_three_plans` (11000 ticks, about
//! 0.4 s).

mod common;

use ai::harness::{BotMatch, Side};
use ai::standard::{BotMemory, phase};
use common::{Arena, P0, P1};
use rules::{Rules, UnitClass};
use sim::{
    AiController, AiMemory, BuildingId, Command, FxVec2, MatchSetup, PlayerId, Sim, SimEvent,
    SimView, TargetId, UnitId,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// A controller that logs, per call, the player, the tick and how many
/// commands the bot inside it returned.
struct Ticks {
    inner: Box<dyn AiController>,
    calls: Arc<Mutex<Vec<(PlayerId, u32, usize)>>>,
}

impl AiController for Ticks {
    fn think(&mut self, p: PlayerId, view: &SimView<'_>, memory: &mut AiMemory) -> Vec<Command> {
        let out = self.inner.think(p, view, memory);
        self.calls.lock().unwrap().push((p, view.tick, out.len()));
        out
    }
}

/// The Standard bot plans every 10 ticks, P0 on ticks 0, 10, ... and P1
/// on ticks 5, 15, ...: commands come only from plan ticks, both seats
/// act from their first plan (a Yeoman, builders, gatherers), and a plan
/// of the arena starts from the documented context.
#[test]
fn standard_keeps_its_cadence() {
    let r = common::rules();
    let setup = MatchSetup::bots(&r, 1, ("standard", "standard"), ("standard", "standard"));
    let every = r.ai.personality("standard").unwrap().think_every_ticks(&r);
    assert_eq!(every, 10);
    assert_eq!(
        (
            phase(PlayerId(0), every, &setup),
            phase(PlayerId(1), every, &setup)
        ),
        (0, 5)
    );
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = ai::controller_for(&setup, &r).unwrap();
    let mut m = BotMatch::new(
        setup.clone(),
        r.clone(),
        Box::new(Ticks {
            inner,
            calls: calls.clone(),
        }),
    );
    for _ in 0..40 {
        m.step();
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 80, "the sim calls both seats every tick");
    for (p, tick, n) in calls.iter() {
        if *n > 0 {
            assert_eq!(*tick % every, phase(*p, every, &setup), "{p:?} at {tick}");
        }
    }
    for p in [PlayerId(0), PlayerId(1)] {
        assert!(
            calls
                .iter()
                .any(|(q, t, n)| *q == p && *t == phase(p, every, &setup) && *n > 0),
            "{p:?} acts at its first plan"
        );
    }
    assert!(m.sim.view().commands_applied() > 0);
    let arena = Arena::new(setup, Vec::new());
    let ctx = arena.ctx(PlayerId(0));
    assert_eq!(ctx.facts.workers.len(), 5, "the standard start's Yeomen");
    assert_eq!(ctx.facts.towns.len(), 1, "the Seat");
    assert_eq!(ctx.facts.enemy_towns.len(), 1);
    assert_eq!(ctx.ledger.stock, ctx.facts.stock);
    assert_eq!(ctx.commands_left, 16);
    assert_eq!(ctx.probes_left, 4);
    let (cmds, _) = arena.plan(PlayerId(1));
    assert!(!cmds.is_empty() && cmds.len() <= 16);
}

/// A Standard-vs-Standard match is the same match on two threads, and a
/// header naming the Standard bot rebuilds it (`controller_for`) so a
/// fresh run reproduces the hash.
#[test]
fn standard_vs_standard_is_deterministic_across_threads() {
    let r = common::rules();
    let setup = MatchSetup::bots(&r, 3, ("standard", "standard"), ("standard", "standard"));
    let run = || {
        let mut m = BotMatch::with_bots(setup.clone(), r.clone()).unwrap();
        m.run(600);
        m.sim.hash()
    };
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(run);
        let b = s.spawn(run);
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(a, b);
    assert_eq!(a, run());
}

/// Every plan's commands, as `(tick, player, commands)`.
type Log = Arc<Mutex<Vec<(u32, PlayerId, Vec<Command>)>>>;

/// A controller that logs every non-empty batch of the controller inside
/// it, and on `raid_at` makes `raider` debug-spawn `raid` (with
/// `debug_commands` on).
struct Logged {
    inner: Box<dyn AiController>,
    log: Log,
    raid: Option<(u32, PlayerId, Command)>,
}

impl AiController for Logged {
    fn think(&mut self, p: PlayerId, view: &SimView<'_>, memory: &mut AiMemory) -> Vec<Command> {
        let mut out = self.inner.think(p, view, memory);
        if !out.is_empty() {
            self.log.lock().unwrap().push((view.tick, p, out.clone()));
        }
        if let Some((at, raider, cmd)) = &self.raid
            && view.tick == *at
            && p == *raider
        {
            out.push(cmd.clone());
        }
        out
    }
}

fn logged(
    setup: &MatchSetup,
    r: &Rules,
    raid: Option<(u32, PlayerId, Command)>,
) -> (BotMatch, Log) {
    let log: Log = Arc::default();
    let inner = ai::controller_for(setup, r).unwrap();
    let m = BotMatch::new(
        setup.clone(),
        r.clone(),
        Box::new(Logged {
            inner,
            log: log.clone(),
            raid,
        }),
    );
    (m, log)
}

/// What an order asks of one unit, comparable across plans.
fn unit_orders(cmd: &Command) -> Vec<(UnitId, String)> {
    let each = |units: &[UnitId], what: String| {
        units.iter().map(|u| (*u, what.clone())).collect::<Vec<_>>()
    };
    match cmd {
        Command::Move { units, target, .. } => each(units, format!("move {target:?}")),
        Command::AttackMove { units, target } => each(units, format!("amove {target:?}")),
        Command::Attack { units, target } => each(units, format!("attack {target:?}")),
        Command::Gather { units, node } => each(units, format!("gather {node:?}")),
        Command::Build { unit, kind, tile } => vec![(*unit, format!("build {kind:?} {tile:?}"))],
        _ => Vec::new(),
    }
}

/// No unit gets the same order again within three plans of its seat
/// (Critique A #3: the on-station rule and the claims stop re-issue
/// storms), through a raid that engages the defence: Standard (P0)
/// against Passive (P1), which debug-spawns three Skirmishers 12 tiles
/// north of P0's Seat at tick 9000, when P0 has an army.
#[test]
fn no_unit_gets_the_same_order_twice_in_three_plans() {
    let r = common::rules();
    let setup = common::standard_vs_passive(&r);
    let raid_at = 9000;
    let raid = common::spawn(&r, P1, "skirmisher", 24, 52, 3);
    let (mut m, log) = logged(&setup, &r, Some((raid_at, P1, raid)));
    m.run(11_000);
    let every = r.ai.personality("standard").unwrap().think_every_ticks(&r);
    let log = log.lock().unwrap();
    let mut last: BTreeMap<(UnitId, String), u32> = BTreeMap::new();
    let mut orders = 0;
    for (tick, p, cmds) in log.iter() {
        assert_eq!(*p, P0, "only the Standard seat plans");
        for c in cmds {
            for key in unit_orders(c) {
                orders += 1;
                if let Some(prev) = last.get(&key) {
                    assert!(
                        tick - prev >= 3 * every,
                        "{key:?} at {prev} and again at {tick}"
                    );
                }
                last.insert(key, *tick);
            }
        }
    }
    assert!(orders > 25, "{orders} unit orders");
    assert!(
        log.iter()
            .any(|(t, _, c)| *t > raid_at
                && c.iter().any(|c| matches!(c, Command::AttackMove { .. }))),
        "the raid was answered"
    );
}

/// A Standard bot built afresh mid-match (`controller_for` on the
/// header, at tick 1500) issues exactly the commands the running one
/// issues, and leaves the same memory, on every tick of both seats for
/// the next 3000 ticks: the bot holds nothing but its hashed memory and
/// tables computed from immutable inputs.
#[test]
fn controller_rebuilt_mid_match_issues_the_same_commands() {
    let r = common::rules();
    let setup = MatchSetup::bots(&r, 2, ("standard", "standard"), ("standard", "standard"));
    let (mut m, log) = logged(&setup, &r, None);
    while m.sim.tick() < 1500 {
        m.step();
    }
    let mut fresh = ai::controller_for(&setup, &r).unwrap();
    let mut compared = 0;
    while m.sim.tick() < 4500 {
        let at = m.sim.tick();
        let view = m.sim.view();
        let mut expected = Vec::new();
        for p in [P0, P1] {
            let mut memory = AiMemory::default();
            memory.set(view.ai_memory(p).to_vec());
            let cmds = fresh.think(p, &view, &mut memory);
            expected.push((p, cmds, memory));
        }
        m.step();
        let log = log.lock().unwrap();
        for (p, cmds, memory) in expected {
            let issued: Vec<Command> = log
                .iter()
                .filter(|(t, q, _)| *t == at && *q == p)
                .flat_map(|(_, _, c)| c.clone())
                .collect();
            assert_eq!(cmds, issued, "{p:?} at {at}");
            assert_eq!(memory.bytes(), m.sim.view().ai_memory(p), "{p:?} at {at}");
            compared += usize::from(!cmds.is_empty());
        }
    }
    assert!(compared >= 20, "{compared} non-empty plans compared");
}

/// `ai::describe` names the window, the latches and the wave.
#[test]
fn describe_reports_the_window_and_latches() {
    let r = common::rules();
    let setup = MatchSetup::bots(&r, 1, ("standard", "standard"), ("passive", ""));
    let mut m = BotMatch::with_bots(setup, r).unwrap();
    m.run(20);
    let view = m.sim.view();
    let line = ai::describe(&view, P0);
    assert!(
        line.starts_with("standard standard window=[0 workers 7, "),
        "{line}"
    );
    assert!(
        line.ends_with("military=no wave=none next_wave=0 waves=0 defend=none defends=0"),
        "{line}"
    );
    assert_eq!(ai::describe(&view, P1), "passive");
    let human = MatchSetup::skirmish(m.sim.rules(), 1);
    let sim = Sim::new(human, m.sim.rules().clone(), Box::new(sim::NoAi));
    assert_eq!(ai::describe(&sim.view(), P0), "human");
}

/// Runs `games` in parallel, one thread each.
fn par<T: Send>(games: Vec<Box<dyn FnOnce() -> T + Send>>) -> Vec<T> {
    std::thread::scope(|s| {
        let handles: Vec<_> = games.into_iter().map(|g| s.spawn(g)).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

/// Standard against Passive with the Standard bot in `seat`, to
/// `ticks` or the outcome; returns the observed facts of the Standard
/// side.
fn vs_passive(seed: u64, seat: PlayerId, ticks: u32) -> Side {
    let r = common::rules();
    let (a, b) = if seat == P0 {
        (("standard", "standard"), ("passive", ""))
    } else {
        (("passive", ""), ("standard", "standard"))
    };
    let setup = MatchSetup::bots(&r, seed, a, b);
    let mut m = BotMatch::with_bots(setup, r).unwrap();
    m.run(ticks);
    m.observer.sides()[usize::from(seat.0)]
}

/// The build order never stalls: every non-optional step is done before
/// tick 30000 in five seeds, from either seat.
#[test]
#[ignore = "long: five 30000-tick games a side; CI runs --ignored m5b_"]
fn m5b_build_order_completes_vs_passive_by_30000() {
    for seat in [P0, P1] {
        let games: Vec<Box<dyn FnOnce() -> Side + Send>> = (1..=5)
            .map(|seed| Box::new(move || vs_passive(seed, seat, 30_000)) as Box<_>)
            .collect();
        for (i, side) in par(games).into_iter().enumerate() {
            let done = side.bo_done_tick;
            assert!(
                done.is_some_and(|t| t < 30_000),
                "{seat:?} seed {}: {done:?}",
                i + 1
            );
        }
    }
}

/// The bot checks its commands before it issues them: fewer than 20 are
/// rejected in 36000 ticks against Passive, from either seat.
#[test]
#[ignore = "long: 36000-tick games; CI runs --ignored m5b_"]
fn m5b_rejections_stay_under_20_per_36000_ticks_vs_passive() {
    let games: Vec<Box<dyn FnOnce() -> Side + Send>> = [P0, P1]
        .into_iter()
        .map(|seat| Box::new(move || vs_passive(1, seat, 36_000)) as Box<_>)
        .collect();
    for (seat, side) in [P0, P1].into_iter().zip(par(games)) {
        assert!(side.rejected < 20, "{seat:?}: {} rejected", side.rejected);
    }
}

/// Own armed units of `me`: non-civilian kinds with an attack.
fn armed(r: &Rules, view: &SimView<'_>, me: PlayerId) -> usize {
    view.units()
        .filter(|u| u.owner == me)
        .filter(|u| {
            r.unit_kind(u.kind.0)
                .is_some_and(|k| k.class != UnitClass::Civilian && k.attack.is_some())
        })
        .count()
}

/// Steps `m` to tick `until`, checking after every step from Masonry on
/// that a full pop never comes with no armed unit at home (the units of
/// the wave in memory are away). Returns the ticks with a full pop.
fn full_pop_ticks(r: &Rules, m: &mut Sim, until: u32) -> u32 {
    let masonry = r.age_index("masonry").unwrap();
    let mut full = 0;
    while m.tick() < until {
        m.step(&[]);
        let view = m.view();
        if view.age(P0) >= masonry {
            let (used, cap) = view.pop(P0);
            let mem = BotMemory::decode(view.ai_memory(P0));
            let away = mem.wave.map_or(0, |w| w.units.len());
            assert!(
                used < cap || armed(r, &view, P0) > away,
                "tick {}: pop {used}/{cap} and no armed unit at home",
                view.tick
            );
            full += u32::from(used >= cap);
        }
    }
    full
}

/// The pop cap never locks the army out: at least 8 armed units by tick
/// 18000, and after Masonry no tick sees a full pop with no armed unit at
/// home. The natural game (Standard P0 against Passive) never fills its
/// pop before 18000 (52/75 at most), so a second game fills it: as soon
/// as P0 reaches Masonry, P0 is given Yeomen up to two below its cap; the
/// full-pop branch must then be met at least once, and the bot must still
/// field at least 8 armed units by 18000. Until the waves of
/// M5b slice 3 exist no unit is ever away, so the check reads "a full pop
/// holds at least one armed unit"; slice 3 gives the wave a roster here.
#[test]
#[ignore = "long: two 18000-tick games; CI runs --ignored m5b_"]
fn m5b_pop_never_deadlocks() {
    let games: Vec<Box<dyn FnOnce() -> (u32, usize) + Send>> = [false, true]
        .into_iter()
        .map(|fill| {
            Box::new(move || {
                let r = common::rules();
                let mut a = Arena::new(common::standard_vs_passive(&r), Vec::new());
                if fill {
                    let masonry = r.age_index("masonry").unwrap();
                    while a.sim.view().age(P0) < masonry {
                        a.run(1);
                    }
                    let (used, cap) = a.sim.view().pop(P0);
                    let n = u16::try_from(cap.saturating_sub(used + 2)).unwrap();
                    assert!(n > 0, "pop {used}/{cap} at Masonry");
                    a.issue(P0, vec![common::spawn(&r, P0, "yeoman", 20, 70, n)]);
                }
                let full = full_pop_ticks(&r, &mut a.sim, 18_000);
                (full, armed(&r, &a.sim.view(), P0))
            }) as Box<_>
        })
        .collect();
    let [(_, natural), (full, filled)] = par(games)[..] else {
        unreachable!()
    };
    assert!(natural >= 8, "{natural} armed units at 18000");
    assert!(full > 0, "the filled game never had a full pop");
    assert!(
        filled >= 8,
        "{filled} armed units at 18000 in the filled game"
    );
}

/// The bot never walls itself in: after 20000 ticks every one of its
/// buildings has a ring tile in the walkable component of the Seat's
/// ring.
#[test]
#[ignore = "long: a 20000-tick game; CI runs --ignored m5b_"]
fn m5b_sites_keep_connectivity() {
    let r = common::rules();
    let setup = common::standard_vs_passive(&r);
    let mut m = BotMatch::with_bots(setup, r.clone()).unwrap();
    m.run(20_000);
    let view = m.sim.view();
    let map = view.map();
    let ring = |b: &sim::Building| {
        let d = r.building_kind(b.kind.0).unwrap();
        let (x0, y0) = (i32::from(b.tile.x) - 1, i32::from(b.tile.y) - 1);
        let (w, h) = (
            i32::try_from(d.width_tiles).unwrap(),
            i32::try_from(d.height_tiles).unwrap(),
        );
        let mut out = Vec::new();
        for y in y0..=y0 + h + 1 {
            for x in x0..=x0 + w + 1 {
                let edge = x == x0 || y == y0 || x == x0 + w + 1 || y == y0 + h + 1;
                if edge
                    && let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y))
                    && map.passable(sim::Tile::new(x, y))
                {
                    out.push(map.component_of(sim::Tile::new(x, y)));
                }
            }
        }
        out
    };
    let seat = view
        .buildings()
        .find(|b| b.owner == P0 && b.town_state.is_some_and(|t| t.seat))
        .unwrap();
    let home = ring(seat);
    assert!(!home.is_empty());
    let mut checked = 0;
    for b in view.buildings().filter(|b| b.owner == P0) {
        let comps = ring(b);
        assert!(
            comps.iter().any(|c| home.contains(c)),
            "{} at {:?} is walled off",
            r.building_kind(b.kind.0).unwrap().id,
            b.tile
        );
        checked += 1;
    }
    assert!(checked >= 15, "{checked} buildings");
}

/// Whether `cmd` trains a non-civilian unit.
fn trains_army(r: &Rules, cmd: &Command) -> bool {
    matches!(cmd, Command::Train { kind, .. } if r.unit_kind(kind.0).is_some_and(|k| k.class != UnitClass::Civilian))
}

/// The bot answers a raid at minute 5: Passive debug-spawns six
/// Skirmishers 10 tiles north of the Standard bot's Seat (P1) at tick
/// 6000. In this game P1's `Military` step is already latched (tick 4906,
/// its Muster Hall complete at 5729), so production is running before the
/// raid; emergency production before the latch is covered by
/// `standard::defend::tests::emergency_production_before_the_military_latch`.
/// Within 200 ticks the defend latch is set and a guard engages: P1
/// issues an `AttackMove` within `defend_regoal_tiles` of a living raider
/// (or an `Attack` on one). Army production goes on through the raid; the
/// bot then kills the raiders, releases the latch, and keeps every Town.
#[test]
#[ignore = "long: a 12000-tick game; CI runs --ignored m5b_"]
fn m5b_defend_answers_a_raid_at_minute_5() {
    let r = common::rules();
    let mut setup = MatchSetup::bots(&r, 1, ("passive", ""), ("standard", "standard"));
    setup.debug_commands = true;
    let raid_at = 6000;
    let raid = common::spawn(&r, P0, "skirmisher", 103, 54, 6);
    let (mut m, log) = logged(&setup, &r, Some((raid_at, P0, raid)));
    while m.sim.tick() < raid_at {
        m.step();
    }
    assert!(
        BotMemory::decode(m.sim.view().ai_memory(P1)).military,
        "the setup's premise: P1's Military step is latched before the raid"
    );
    while m.sim.tick() < raid_at + 3 {
        m.step();
    }
    let raiders: Vec<UnitId> = m
        .sim
        .view()
        .units()
        .filter(|u| u.owner == P0 && u.kind == common::unit_kind(&r, "skirmisher"))
        .map(|u| u.id)
        .collect();
    assert_eq!(raiders.len(), 6);
    let towns_before: Vec<BuildingId> = m
        .sim
        .view()
        .buildings()
        .filter(|b| b.owner == P1 && b.town_state.is_some())
        .map(|b| b.id)
        .collect();
    // Where the living raiders stand at each tick a plan may read.
    let mut seen: BTreeMap<u32, Vec<FxVec2>> = BTreeMap::new();
    while m.sim.tick() < raid_at + 200 {
        let view = m.sim.view();
        seen.insert(
            view.tick,
            raiders
                .iter()
                .filter_map(|u| view.unit(*u).map(|u| u.pos))
                .collect(),
        );
        m.step();
    }
    let mem = BotMemory::decode(m.sim.view().ai_memory(P1));
    assert!(mem.defend.is_some(), "the defend latch is set");
    {
        let regoal = r.ai.personality("standard").unwrap().defend_regoal_tiles;
        let r2 = ai::standard::facts::tiles_sq(regoal);
        let log = log.lock().unwrap();
        let engaged = log.iter().any(|(t, p, cmds)| {
            *p == P1
                && *t > raid_at
                && cmds.iter().any(|c| match c {
                    Command::AttackMove { target, .. } => seen
                        .get(t)
                        .is_some_and(|at| at.iter().any(|x| x.dist_sq_i64(*target) <= r2)),
                    Command::Attack {
                        target: TargetId::Unit(u),
                        ..
                    } => raiders.contains(u),
                    _ => false,
                })
        });
        assert!(engaged, "a guard engages a raider within 200 ticks");
        let trained = log.iter().any(|(t, p, cmds)| {
            *p == P1 && *t > raid_at && cmds.iter().any(|c| trains_army(&r, c))
        });
        assert!(trained, "army production goes on through the raid");
    }
    let mut cleared = None;
    while m.sim.tick() < 12_000 {
        m.step();
        let view = m.sim.view();
        if cleared.is_none() && raiders.iter().all(|u| view.unit(*u).is_none()) {
            cleared = Some(view.tick);
        }
    }
    let view = m.sim.view();
    let cleared = cleared.expect("the raiders die");
    let mem = BotMemory::decode(view.ai_memory(P1));
    assert!(
        mem.defend.is_none() && mem.defends >= 1,
        "{mem:?}, cleared at {cleared}"
    );
    for t in towns_before {
        assert_eq!(view.building(t).map(|b| b.owner), Some(P1), "no flip");
    }
    let _ = SimEvent::TerritoryChanged;
}

/// A defence outranks saving for an age: Standard (P0) against Passive
/// (P1), which debug-spawns three Skirmishers 13 tiles north of P0's Seat
/// at tick 6200. P0's Muster Hall is up and idle and its `Military` step
/// latched, but the Masonry `Advance` in its window is saving up its 250
/// Grain while its techs finish. While the bot defends, that saving no
/// longer holds the stock back from production, so P0 queues an armed
/// unit within 200 ticks of the raid.
#[test]
#[ignore = "long: a 6400-tick game; CI runs --ignored m5b_"]
fn m5b_defend_outranks_saving_for_an_age() {
    let r = common::rules();
    let setup = common::standard_vs_passive(&r);
    let raid_at = 6200;
    let raid = common::spawn(&r, P1, "skirmisher", 24, 51, 3);
    let (mut m, log) = logged(&setup, &r, Some((raid_at, P1, raid)));
    while m.sim.tick() < raid_at {
        m.step();
    }
    let view = m.sim.view();
    let mem = BotMemory::decode(view.ai_memory(P0));
    let line = ai::describe(&view, P0);
    assert!(
        mem.military && line.contains("advance masonry"),
        "the setup's premise: production latched, Masonry saved for: {line}"
    );
    while m.sim.tick() <= raid_at + 200 {
        m.step();
    }
    assert!(
        BotMemory::decode(m.sim.view().ai_memory(P0))
            .defend
            .is_some(),
        "the defend latch is set"
    );
    let log = log.lock().unwrap();
    let first = log
        .iter()
        .filter(|(t, p, _)| *p == P0 && *t > raid_at)
        .find(|(_, _, cmds)| cmds.iter().any(|c| trains_army(&r, c)))
        .map(|(t, _, _)| *t);
    assert!(
        first.is_some_and(|t| t <= raid_at + 200),
        "the first armed unit queued after the raid at {raid_at}: {first:?}"
    );
}

/// Hidden state: restoring a Standard-vs-Standard match at tick 3003 or
/// 3007 (one in each seat's planning phase) into a fresh `Sim` with
/// fresh bots from the header matches the uninterrupted run, every
/// sub-hash, every tick, for the next 3000 ticks.
#[test]
#[ignore = "long: 6000-tick games; CI runs --ignored m5b_"]
fn m5b_restore_at_3003_and_3007_matches_uninterrupted() {
    let r = common::rules();
    let setup = MatchSetup::bots(&r, 5, ("standard", "standard"), ("standard", "standard"));
    let games: Vec<Box<dyn FnOnce() -> u32 + Send>> = [3003u32, 3007]
        .into_iter()
        .map(|at| {
            let (setup, r) = (setup.clone(), r.clone());
            Box::new(move || {
                let fresh = || {
                    Sim::new(
                        setup.clone(),
                        r.clone(),
                        ai::controller_for(&setup, &r).unwrap(),
                    )
                };
                let mut a = fresh();
                while a.tick() < at {
                    a.step(&[]);
                }
                let mut b = fresh();
                b.restore(&a.snapshot()).unwrap();
                for _ in 0..3000 {
                    a.step(&[]);
                    b.step(&[]);
                    let (ha, hb) = (a.sub_hashes(), b.sub_hashes());
                    assert_eq!(
                        ha.first_difference(&hb),
                        None,
                        "restored at {at}, tick {}",
                        a.tick()
                    );
                }
                assert!(!a.view().ai_memory(P0).is_empty() && !a.view().ai_memory(P1).is_empty());
                a.tick()
            }) as Box<_>
        })
        .collect();
    assert_eq!(par(games), [6003, 6007]);
}
