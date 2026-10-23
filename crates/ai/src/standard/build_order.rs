//! The build order: when the view shows a step as done (never when its
//! command is issued), the window of steps the bot works on, and what it
//! does for each (docs/design/ai.md rule 1).
//!
//! **The window** is the first `lookahead` steps that are not done and not
//! cooling down, in order; it stops at an unfinished `Wait`. Each window
//! step, in order, either issues its command (paid from the plan's
//! [`super::Ledger`]), saves up (its cost is reserved, so a later step or
//! production cannot spend it), waits (every trainer or host of the step
//! is busy or stands in a contested Town, a predecessor tech is still
//! queued, no worker is free, or the plan's `check_build` probes ran
//! out), or is
//! blocked (any other rejection, no trainer or host built yet, no site,
//! which includes a `NearTown` building with no uncontested own Town to
//! go beside): a blocked step is cooled down for
//! `blocked_retry_ds` in `BotMemory::blocked` and then retried; it never
//! becomes a permanent skip. An `Advance` is never cooled down: while the
//! techs it needs are still missing it waits and reserves nothing (the
//! army and the steps behind it spend the stock meanwhile), and once they
//! are done it saves up its cost until the stock fills, except while the
//! bot defends (`BotMemory::defend`): then it reserves nothing, so the
//! stock goes to the army first.
//!
//! **The `Military` latch** is set once every non-optional step before
//! the `Military` step is done, and never cleared.
//!
//! **The site keeper** gives one of the bot's construction sites with no
//! builder per plan a builder, with a `Build` on the site's own tile (a
//! free resume).
//!
//! **The remembered site** (`BotMemory::site`) is the last site chosen for
//! a `Build` step: tried first on the step's next plan, forgotten when a
//! check rejects it or the step leaves the window.

use super::ctx::Ctx;
use super::economy;
use super::facts::{Facts, kind_centre};
use super::memory::BotMemory;
use super::sites::{SiteSearch, site_for};
use rules::{BuildOrder, Rules, Step};
use sim::{BuildingId, BuildingKindId, Command, RejectReason, TechId, UnitId, UnitKindId};
use std::collections::BTreeSet;

/// Whether the view (through `facts`) shows `step` as done:
///
/// | Step | Done when |
/// | --- | --- |
/// | `Workers` | workers alive plus queued reach the count |
/// | `Build` | own buildings of the kind, sites included, reach the count |
/// | `Train` | units of the kind alive plus queued reach the count |
/// | `Research` | the tech is researched or queued |
/// | `Advance` | the age is reached |
/// | `Military` | always |
/// | `Wait` | the tick reaches the time |
///
/// An id the rules do not know (which `Rules::load` rules out) is never
/// done.
pub fn satisfied(step: &Step, facts: &Facts, rules: &Rules) -> bool {
    match step {
        Step::Workers { count, .. } => facts
            .worker_kind
            .is_some_and(|w| facts.alive_and_queued(w) >= *count),
        Step::Build { kind, count, .. } => rules
            .building_kind_by_id(kind)
            .is_some_and(|(i, _)| facts.owned(BuildingKindId(i)) >= *count),
        Step::Train { kind, count, .. } => {
            unit_kind(rules, kind).is_some_and(|k| facts.alive_and_queued(k) >= *count)
        }
        Step::Research { tech, .. } => rules
            .tech_by_id(tech)
            .is_some_and(|(i, _)| facts.researched_or_queued.contains(&TechId(i))),
        Step::Advance { age } => rules.age_index(age).is_some_and(|a| facts.age >= a),
        Step::Military => true,
        Step::Wait { until_ds } => facts.tick >= rules.ticks_from_ds(*until_ds),
    }
}

/// The index of the first step not done, skipping optional steps when
/// `required_only`; `None` when every (required) step is done.
pub fn first_unsatisfied(
    order: &BuildOrder,
    facts: &Facts,
    rules: &Rules,
    required_only: bool,
) -> Option<usize> {
    order
        .steps
        .iter()
        .position(|s| !(required_only && s.optional()) && !satisfied(s, facts, rules))
}

/// A unit kind's id by its string id.
fn unit_kind(rules: &Rules, id: &str) -> Option<UnitKindId> {
    let i = rules.units.iter().position(|u| u.id == id)?;
    Some(UnitKindId(u16::try_from(i).ok()?))
}

/// Whether step `i` is cooling down at `tick`.
pub fn cooled(mem: &BotMemory, i: usize, tick: u32) -> bool {
    mem.blocked
        .iter()
        .any(|(s, at)| usize::from(*s) == i && tick < *at)
}

/// The plan's window: the first `lookahead` steps that are not done and
/// not cooling down, in order, stopping at an unfinished `Wait`.
pub fn window(ctx: &Ctx<'_>, mem: &BotMemory) -> Vec<usize> {
    let lookahead = usize::try_from(ctx.knobs.lookahead).unwrap_or(1);
    window_of(ctx.order, &ctx.facts, ctx.view.rules(), mem, lookahead)
}

/// [`window`] from its inputs (`facts.tick` is the plan's tick).
pub fn window_of(
    order: &BuildOrder,
    facts: &Facts,
    rules: &Rules,
    mem: &BotMemory,
    lookahead: usize,
) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, step) in order.steps.iter().enumerate() {
        if out.len() >= lookahead {
            break;
        }
        if satisfied(step, facts, rules) || cooled(mem, i, facts.tick) {
            continue;
        }
        if matches!(step, Step::Wait { .. }) {
            break;
        }
        out.push(i);
    }
    out
}

/// Whether every non-optional step before the build order's first
/// `Military` step is done (`false` when it has none).
pub fn military_crossed(order: &BuildOrder, facts: &Facts, rules: &Rules) -> bool {
    let Some(m) = order.steps.iter().position(|s| matches!(s, Step::Military)) else {
        return false;
    };
    order.steps[..m]
        .iter()
        .all(|s| s.optional() || satisfied(s, facts, rules))
}

/// A short label for a step (`ai::describe`).
pub fn step_label(step: &Step) -> String {
    match step {
        Step::Workers { count, .. } => format!("workers {count}"),
        Step::Build { kind, count, .. } => format!("build {kind} {count}"),
        Step::Train { kind, count, .. } => format!("train {kind} {count}"),
        Step::Research { tech, .. } => format!("research {tech}"),
        Step::Advance { age } => format!("advance {age}"),
        Step::Military => "military".to_string(),
        Step::Wait { until_ds } => format!("wait {until_ds}"),
    }
}

/// What a window step did this plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// Its command was issued.
    Issued,
    /// It saves up for this cost.
    Saving(Vec<i64>),
    /// It waits for something already under way.
    Waiting,
    /// It is blocked and cools down.
    Blocked,
}

/// The workers a `Build` may take, nearest first by the caller: idle,
/// unclaimed workers; when there are none, the gatherers of the resource
/// most over its share (ties to the lower resource index). Never a unit
/// that builds.
pub fn builder_pool(ctx: &Ctx<'_>, mem: &BotMemory) -> Vec<UnitId> {
    let idle: Vec<UnitId> = ctx
        .facts
        .idle_workers
        .iter()
        .copied()
        .filter(|u| !ctx.is_claimed(*u))
        .collect();
    if !idle.is_empty() {
        return idle;
    }
    let s = economy::shares(ctx, mem);
    let Some(r) = (0..s.current.len())
        .filter(|r| s.current[*r] > 0)
        .max_by_key(|r| {
            (
                i64::from(s.current[*r]) - i64::from(s.desired[*r]),
                core::cmp::Reverse(*r),
            )
        })
    else {
        return Vec::new();
    };
    ctx.facts.gatherers[r]
        .iter()
        .copied()
        .filter(|u| s.pool.contains(u))
        .collect()
}

/// Train one unit of `kind` at the free trainer with the shortest queue
/// (the caller's order breaks ties).
fn train(ctx: &mut Ctx<'_>, kind: UnitKindId, mut trainers: Vec<BuildingId>) -> StepOutcome {
    trainers.sort_by_key(|b| ctx.view.building(*b).map_or(usize::MAX, |b| b.queue.len()));
    let Some(b) = trainers.first().copied() else {
        return StepOutcome::Waiting;
    };
    match ctx.view.check_train(ctx.me, b, kind) {
        Ok(cost) if ctx.ledger.try_spend(&cost) => {
            ctx.push(Command::Train { building: b, kind });
            StepOutcome::Issued
        }
        Ok(cost) => StepOutcome::Saving(cost),
        Err(RejectReason::CannotAfford { .. }) => {
            StepOutcome::Saving(ctx.view.unit_cost(ctx.me, kind))
        }
        Err(RejectReason::QueueFull) => StepOutcome::Waiting,
        Err(_) => StepOutcome::Blocked,
    }
}

/// Free trainers of `kind` with the shortest queue first (Seat first for
/// the worker kind, then by id).
fn trainers_for(ctx: &Ctx<'_>, kind: UnitKindId) -> (bool, Vec<BuildingId>) {
    let rules = ctx.view.rules();
    let Some(trainer) = rules
        .unit_kind(kind.0)
        .and_then(|d| rules.building_kind_by_id(&d.trainer))
    else {
        return (false, Vec::new());
    };
    let kind_id = BuildingKindId(trainer.0);
    let all: Vec<BuildingId> = if kind_id.0 == rules.town_kind() {
        ctx.facts.towns.clone()
    } else {
        ctx.facts
            .complete
            .get(&kind_id)
            .cloned()
            .unwrap_or_default()
    };
    let max = usize::try_from(ctx.knobs.max_queue_per_building).unwrap_or(1);
    let free = all
        .iter()
        .copied()
        .filter(|b| !ctx.facts.contested.contains(b))
        .filter(|b| ctx.view.building(*b).is_some_and(|b| b.queue.len() < max))
        .collect();
    (!all.is_empty(), free)
}

/// Do step `i` of the window.
fn execute(ctx: &mut Ctx<'_>, mem: &mut BotMemory, i: usize) -> StepOutcome {
    let rules = ctx.view.rules();
    let step = ctx.order.steps[i].clone();
    match step {
        Step::Workers { .. } => {
            let Some(w) = ctx.facts.worker_kind else {
                return StepOutcome::Blocked;
            };
            match trainers_for(ctx, w) {
                (false, _) => StepOutcome::Blocked,
                (true, free) => train(ctx, w, free),
            }
        }
        Step::Train { kind, .. } => {
            let Some(k) = unit_kind(rules, &kind) else {
                return StepOutcome::Blocked;
            };
            match trainers_for(ctx, k) {
                (false, _) => StepOutcome::Blocked,
                (true, free) => train(ctx, k, free),
            }
        }
        Step::Research { tech, .. } => {
            let Some((t, def)) = rules.tech_by_id(&tech) else {
                return StepOutcome::Blocked;
            };
            let tech = TechId(t);
            let Some((host, _)) = rules.building_kind_by_id(&def.host) else {
                return StepOutcome::Blocked;
            };
            let all = ctx
                .facts
                .complete
                .get(&BuildingKindId(host))
                .cloned()
                .unwrap_or_default();
            if all.is_empty() {
                return StepOutcome::Blocked;
            }
            let max = usize::try_from(ctx.knobs.max_queue_per_building).unwrap_or(1);
            let mut free: Vec<BuildingId> = all
                .into_iter()
                .filter(|b| !ctx.facts.contested.contains(b))
                .filter(|b| ctx.view.building(*b).is_some_and(|b| b.queue.len() < max))
                .collect();
            free.sort_by_key(|b| ctx.view.building(*b).map_or(usize::MAX, |b| b.queue.len()));
            let Some(b) = free.first().copied() else {
                return StepOutcome::Waiting;
            };
            match ctx.view.check_research(ctx.me, b, tech) {
                Ok((cost, _)) if ctx.ledger.try_spend(&cost) => {
                    ctx.push(Command::Research { building: b, tech });
                    StepOutcome::Issued
                }
                Ok((cost, _)) => StepOutcome::Saving(cost),
                Err(RejectReason::CannotAfford { .. }) => {
                    StepOutcome::Saving(ctx.view.research_quote(ctx.me, tech).0)
                }
                Err(RejectReason::QueueFull) => StepOutcome::Waiting,
                Err(RejectReason::PrerequisiteMissing)
                    if rules
                        .tech_predecessor(t)
                        .is_some_and(|p| ctx.facts.researched_or_queued.contains(&TechId(p))) =>
                {
                    StepOutcome::Waiting
                }
                Err(_) => StepOutcome::Blocked,
            }
        }
        Step::Advance { .. } => {
            let next = rules.ages.get(usize::from(ctx.facts.age) + 1);
            let cost = next.map(|a| rules.amounts(&a.cost)).unwrap_or_default();
            match ctx.view.check_advance_age(ctx.me) {
                Ok(cost) if ctx.ledger.try_spend(&cost) => {
                    ctx.push(Command::AdvanceAge { player: ctx.me });
                    StepOutcome::Issued
                }
                Ok(cost) => StepOutcome::Saving(cost),
                Err(RejectReason::CannotAfford { .. }) => StepOutcome::Saving(cost),
                Err(_) => StepOutcome::Waiting,
            }
        }
        Step::Build { kind, at, .. } => {
            let Some((k, _)) = rules.building_kind_by_id(&kind) else {
                return StepOutcome::Blocked;
            };
            let k = BuildingKindId(k);
            let cost = match ctx.view.check_build_kind(ctx.me, k) {
                Ok(cost) => cost,
                Err(RejectReason::CannotAfford { .. }) => {
                    return StepOutcome::Saving(ctx.view.building_cost(ctx.me, k));
                }
                Err(_) => return StepOutcome::Blocked,
            };
            if !ctx.ledger.affords(&cost) {
                return StepOutcome::Saving(cost);
            }
            let pool = builder_pool(ctx, mem);
            let step_index = u16::try_from(i).expect("at most 64 steps");
            match site_for(ctx, mem, step_index, k, at, &pool) {
                SiteSearch::Found { builder, tile } => {
                    if ctx.ledger.try_spend(&cost) {
                        ctx.claim(builder);
                        if ctx.push(Command::Build {
                            unit: builder,
                            kind: k,
                            tile,
                        }) && let Some(d) = rules.building_kind(k.0)
                        {
                            let (x, y) = (i32::from(tile.x), i32::from(tile.y));
                            let w = i32::try_from(d.width_tiles).expect("validated <= 8");
                            let h = i32::try_from(d.height_tiles).expect("validated <= 8");
                            ctx.planned.push((x, y, x + w - 1, y + h - 1));
                        }
                        StepOutcome::Issued
                    } else {
                        StepOutcome::Saving(cost)
                    }
                }
                SiteSearch::NoBuilder | SiteSearch::OutOfProbes => StepOutcome::Waiting,
                SiteSearch::NoSite => StepOutcome::Blocked,
            }
        }
        Step::Military | Step::Wait { .. } => StepOutcome::Waiting,
    }
}

/// Run the rule: latch `Military`, work the window, keep one abandoned
/// site going.
pub fn step(ctx: &mut Ctx<'_>, mem: &mut BotMemory) {
    let tick = ctx.tick();
    mem.blocked.retain(|(_, at)| tick < *at);
    if !mem.military && military_crossed(ctx.order, &ctx.facts, ctx.view.rules()) {
        mem.military = true;
    }
    let retry = ctx.ticks(ctx.knobs.blocked_retry_ds);
    let steps = window(ctx, mem);
    // A remembered site is only kept for a step still being worked on.
    if mem
        .site
        .is_some_and(|(s, _)| !steps.contains(&usize::from(s)))
    {
        mem.site = None;
    }
    for i in steps {
        if ctx.commands_left == 0 {
            break;
        }
        match execute(ctx, mem, i) {
            StepOutcome::Issued | StepOutcome::Waiting => {}
            // A defence outranks saving for an age: the `Advance` keeps
            // its place but leaves the stock to production.
            StepOutcome::Saving(_)
                if mem.defend.is_some() && matches!(ctx.order.steps[i], Step::Advance { .. }) => {}
            StepOutcome::Saving(cost) => ctx.ledger.reserve(&cost),
            StepOutcome::Blocked => {
                if !matches!(ctx.order.steps[i], Step::Advance { .. }) {
                    let s = u16::try_from(i).expect("at most 64 steps");
                    mem.blocked.push((s, tick.saturating_add(retry)));
                    if mem.site.is_some_and(|(step, _)| step == s) {
                        mem.site = None;
                    }
                }
            }
        }
    }
    site_keeper(ctx, mem);
}

/// One own site with no builder gets the nearest worker of the builder
/// pool, with a `Build` on its own tile (a free resume).
pub fn site_keeper(ctx: &mut Ctx<'_>, mem: &BotMemory) {
    if ctx.commands_left == 0 || ctx.probes_left == 0 {
        return;
    }
    let held: BTreeSet<BuildingId> = ctx.facts.builders.keys().copied().collect();
    let Some(site) = ctx.facts.sites.iter().copied().find(|s| !held.contains(s)) else {
        return;
    };
    let Some(b) = ctx.view.building(site) else {
        return;
    };
    let (kind, tile) = (b.kind, b.tile);
    let centre = kind_centre(ctx.view.rules(), kind, tile);
    let pool = builder_pool(ctx, mem);
    let Some(builder) = pool
        .iter()
        .filter_map(|id| ctx.view.unit(*id))
        .min_by_key(|u| (u.pos.dist_sq_i64(centre), u.id))
        .map(|u| u.id)
    else {
        return;
    };
    ctx.probes_left -= 1;
    if ctx.view.check_build(ctx.me, builder, kind, tile).is_ok() {
        ctx.claim(builder);
        ctx.push(Command::Build {
            unit: builder,
            kind,
            tile,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{Arena, P0, P1, grant_all, place, standard_vs_passive, unit_kind};
    use rules::Placement;
    use sim::{Command, SimEvent, TargetId};

    fn order(steps: Vec<Step>) -> BuildOrder {
        BuildOrder {
            id: "test".into(),
            steps,
        }
    }

    fn build(kind: &str, count: u32, at: Placement) -> Step {
        Step::Build {
            kind: kind.into(),
            count,
            at,
            optional: false,
        }
    }

    /// The window is the first three steps not done; a site counts as a
    /// building and a queued unit as a unit.
    #[test]
    fn window_is_first_unsatisfied_steps_counting_sites_and_queued_entries() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(standard_vs_passive(&r), Vec::new());
        let mem = BotMemory::default();
        assert_eq!(window(&a.ctx(P0), &mem), [0, 1, 2]);
        let croft = crate::testkit::building_kind(&r, "croft");
        let site = |x, y| Command::DebugPlaceBuilding {
            owner: P0,
            kind: croft,
            tile: sim::Tile::new(x, y),
            complete: false,
        };
        a.issue(P0, vec![site(18, 58), site(18, 70)]);
        assert_eq!(a.buildings_of(P0, "croft").len(), 2);
        assert_eq!(window(&a.ctx(P0), &mem), [0, 2, 3], "two croft sites");
        let seat = a.seat(P0);
        let yeoman = unit_kind(&r, "yeoman");
        let train = || Command::Train {
            building: seat,
            kind: yeoman,
        };
        a.issue(P0, vec![train(), train()]);
        assert_eq!(a.sim.view().building(seat).unwrap().queue.len(), 2);
        assert_eq!(
            window(&a.ctx(P0), &mem),
            [2, 3, 4],
            "five Yeomen and two queued make seven"
        );
    }

    /// A step the stock cannot pay for reserves its cost, so a later step
    /// cannot spend it; a step blocked for any other reason reserves
    /// nothing, cools down for `blocked_retry_ds` and is retried after.
    #[test]
    fn cannot_afford_reserves_other_rejections_cool_down_and_retry() {
        let r = crate::testkit::rules();
        let a = Arena::frozen(standard_vs_passive(&r), Vec::new());
        let o = order(vec![
            Step::Research {
                tech: "arms_1".into(),
                optional: true,
            },
            Step::Train {
                kind: "scribe".into(),
                count: 1,
                optional: true,
            },
            build("croft", 1, Placement::NearTown),
            build("muster_hall", 1, Placement::NearTown),
        ]);
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        let mut mem = BotMemory::default();
        assert_eq!(window(&ctx, &mem), [0, 1, 2]);
        step(&mut ctx, &mut mem);
        let tick = a.sim.tick();
        let retry = r.ticks_from_ds(ctx.knobs.blocked_retry_ds);
        assert_eq!(
            mem.blocked,
            [(0, tick + retry), (1, tick + retry)],
            "no Scriptorium hosts arms_1 or trains Scribes"
        );
        assert_eq!(ctx.ledger.reserved, [0, 0, 0, 0]);
        let cmds = ctx.into_commands();
        assert!(
            matches!(cmds[..], [Command::Build { .. }]),
            "the Croft only: {cmds:?}"
        );

        // The cooled steps leave the window; the Muster Hall enters it and
        // cannot be paid for once the Croft is: it saves up.
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        ctx.ledger.stock[1] = 120;
        assert_eq!(window(&ctx, &mem), [2, 3]);
        step(&mut ctx, &mut mem);
        assert_eq!(
            ctx.ledger.reserved,
            [0, 100, 0, 0],
            "the Muster Hall's 100 Lumber"
        );
        assert_eq!(ctx.ledger.available(1), 0);

        // With a Scriptorium and no Ore or Lore, the sim's own check says
        // CannotAfford: arms_1 saves up its quote, no cooldown.
        let a2 = Arena::frozen(
            standard_vs_passive(&r),
            vec![place(&r, P0, "scriptorium", 18, 58)],
        );
        let mut ctx = a2.ctx(P0);
        ctx.order = &o;
        let mut fresh = BotMemory::default();
        step(&mut ctx, &mut fresh);
        assert_eq!(ctx.ledger.reserved, [0, 0, 50, 75]);
        assert!(!fresh.blocked.iter().any(|(s, _)| *s == 0));

        // After the cooldown, with a Scriptorium and stock, both are done.
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            [
                vec![place(&r, P0, "scriptorium", 18, 58)],
                grant_all(&r, P0, 500),
            ]
            .concat(),
        );
        a.run(retry);
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        assert_eq!(window(&ctx, &mem), [0, 1, 2], "the cooldowns ran out");
        step(&mut ctx, &mut mem);
        assert!(mem.blocked.is_empty(), "{:?}", mem.blocked);
        let cmds = ctx.into_commands();
        assert!(cmds.iter().any(|c| matches!(c, Command::Research { .. })));
        assert!(cmds.iter().any(|c| matches!(c, Command::Train { .. })));
    }

    /// An `Advance` that cannot be made yet is never cooled down. While the
    /// techs it needs are missing it reserves nothing, so the steps behind
    /// it and production may spend the stock; once they are researched it
    /// saves up its cost, and the Yeoman behind it saves up too.
    #[test]
    fn advance_is_never_cooled_down() {
        let r = crate::testkit::rules();
        let o = order(vec![
            Step::Advance {
                age: "masonry".into(),
            },
            Step::Workers {
                count: 9,
                split: [("grain".to_string(), 1)].into_iter().collect(),
            },
        ]);
        let yeoman_kind = unit_kind(&r, "yeoman");
        let a = Arena::frozen(standard_vs_passive(&r), Vec::new());
        let yeoman = a.sim.view().unit_cost(P0, yeoman_kind)[0];
        let mut mem = BotMemory::default();
        for _ in 0..3 {
            let mut ctx = a.ctx(P0);
            ctx.order = &o;
            step(&mut ctx, &mut mem);
            assert!(mem.blocked.is_empty(), "never cooled down");
            assert_eq!(
                ctx.ledger.reserved,
                [0, 0, 0, 0],
                "Masonry's techs are missing: nothing is reserved"
            );
            let cmds = ctx.into_commands();
            assert!(
                cmds.iter()
                    .any(|c| matches!(c, Command::Train { kind, .. } if *kind == yeoman_kind)),
                "the Yeoman spends the Grain: {cmds:?}"
            );
        }
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                crate::testkit::research(&r, P0, "arms_1"),
                crate::testkit::research(&r, P0, "trade_1"),
            ],
        );
        assert!(matches!(
            a.sim.view().check_advance_age(P0),
            Err(RejectReason::CannotAfford { .. })
        ));
        for _ in 0..3 {
            let mut ctx = a.ctx(P0);
            ctx.order = &o;
            step(&mut ctx, &mut mem);
            assert!(mem.blocked.is_empty(), "never cooled down");
            assert_eq!(
                ctx.ledger.reserved,
                [250 + yeoman, 0, 0, 100],
                "the techs are done: Masonry's cost is saved, and the Yeoman saves up behind it"
            );
            let cmds = ctx.into_commands();
            assert!(
                cmds.is_empty(),
                "the Yeoman cannot use the reserved Grain: {cmds:?}"
            );
        }
    }

    /// While the bot defends, an `Advance` that cannot be made yet keeps
    /// its place in the window but reserves nothing, so production can
    /// spend the stock; it is still never cooled down.
    #[test]
    fn an_advance_reserves_nothing_while_the_bot_defends() {
        let r = crate::testkit::rules();
        let a = Arena::frozen(standard_vs_passive(&r), Vec::new());
        let o = order(vec![Step::Advance {
            age: "masonry".into(),
        }]);
        let tick = a.sim.tick();
        let mut mem = BotMemory {
            defend: Some(crate::standard::memory::Defend {
                town: a.seat(P0),
                last_threat: tick,
                since: tick,
            }),
            ..BotMemory::default()
        };
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        assert_eq!(window(&ctx, &mem), [0]);
        step(&mut ctx, &mut mem);
        assert_eq!(ctx.ledger.reserved, [0, 0, 0, 0]);
        assert!(mem.blocked.is_empty(), "never cooled down");
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        assert_eq!(window(&ctx, &mem), [0], "it keeps its place");
    }

    /// The `Military` latch is set once the steps before it are done and
    /// stays set when a lost building reopens one of them.
    #[test]
    fn lost_building_reopens_its_step_without_unlatching_military() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            vec![place(&r, P0, "croft", 18, 58)],
        );
        let o = order(vec![
            build("croft", 1, Placement::NearTown),
            Step::Military,
            Step::Wait { until_ds: 72_000 },
        ]);
        let mut mem = BotMemory::default();
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        assert!(window(&ctx, &mem).is_empty());
        step(&mut ctx, &mut mem);
        assert!(mem.military);
        let croft = a.buildings_of(P0, "croft")[0];
        a.issue(
            P0,
            vec![Command::DebugDamage {
                target: TargetId::Building(croft),
                amount: 10_000,
            }],
        );
        assert!(a.buildings_of(P0, "croft").is_empty());
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        assert_eq!(window(&ctx, &mem), [0], "the Croft step reopens");
        step(&mut ctx, &mut mem);
        assert!(mem.military, "the latch holds");
        assert!(
            ctx.into_commands()
                .iter()
                .any(|c| matches!(c, Command::Build { .. })),
            "and the Croft is rebuilt"
        );
    }

    /// A site with no builder gets the nearest worker with a `Build` on
    /// its own tile, which resumes it.
    #[test]
    fn site_keeper_resumes_an_abandoned_site() {
        let r = crate::testkit::rules();
        let croft = crate::testkit::building_kind(&r, "croft");
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            vec![Command::DebugPlaceBuilding {
                owner: P0,
                kind: croft,
                tile: sim::Tile::new(29, 66),
                complete: false,
            }],
        );
        let site = a.buildings_of(P0, "croft")[0];
        let o = order(vec![Step::Wait { until_ds: 72_000 }]);
        let mut mem = BotMemory::default();
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        step(&mut ctx, &mut mem);
        let cmds = ctx.into_commands();
        let [Command::Build { unit, kind, tile }] = cmds[..] else {
            panic!("one resume: {cmds:?}");
        };
        assert_eq!((kind, tile), (croft, sim::Tile::new(29, 66)));
        assert_eq!(unit, sim::UnitId(3), "the Yeoman nearest the site");
        a.issue(P0, cmds);
        assert_eq!(
            a.sim.view().unit(unit).unwrap().job,
            Some(sim::Job::Build { site }),
            "it builds the same site"
        );
        assert_eq!(a.buildings_of(P0, "croft"), [site], "no second site");
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        step(&mut ctx, &mut mem);
        assert!(ctx.into_commands().is_empty(), "the site has its builder");
        let _ = P1;
    }

    /// The site chosen for a `Build` step is remembered while the step is
    /// worked on and forgotten once the building stands.
    #[test]
    fn a_remembered_site_is_forgotten_once_its_step_is_done() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(standard_vs_passive(&r), Vec::new());
        let o = order(vec![
            build("croft", 1, Placement::NearTown),
            Step::Wait { until_ds: 72_000 },
        ]);
        let mut mem = BotMemory::default();
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        step(&mut ctx, &mut mem);
        let cmds = ctx.into_commands();
        let [Command::Build { tile, .. }] = cmds[..] else {
            panic!("{cmds:?}");
        };
        assert_eq!(mem.site, Some((0, tile)));
        a.issue(P0, cmds);
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        assert!(window(&ctx, &mem).is_empty(), "the site counts");
        step(&mut ctx, &mut mem);
        assert_eq!(mem.site, None);
        assert_eq!(mem, BotMemory::default(), "nothing left to remember");
    }

    /// Two `NearTown` steps worked in the same plan never pick footprints
    /// closer than `building_gap_tiles`: the second site keeps clear of
    /// the first, which is not placed yet, so neither `Build` is rejected
    /// when they apply.
    #[test]
    fn two_near_town_sites_in_one_plan_keep_their_gap() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            [
                vec![place(&r, P0, "croft", 20, 60)],
                grant_all(&r, P0, 1000),
            ]
            .concat(),
        );
        let o = order(vec![
            build("muster_hall", 1, Placement::NearTown),
            build("scriptorium", 1, Placement::NearTown),
        ]);
        let mut ctx = a.ctx(P0);
        ctx.order = &o;
        let mut mem = BotMemory::default();
        step(&mut ctx, &mut mem);
        let gap = i32::try_from(ctx.knobs.building_gap_tiles).unwrap();
        let cmds = ctx.into_commands();
        let rects: Vec<(i32, i32, i32, i32)> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::Build { kind, tile, .. } => {
                    let d = r.building_kind(kind.0).unwrap();
                    let (x, y) = (i32::from(tile.x), i32::from(tile.y));
                    let w = i32::try_from(d.width_tiles).unwrap();
                    let h = i32::try_from(d.height_tiles).unwrap();
                    Some((x, y, x + w - 1, y + h - 1))
                }
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2, "both steps build: {cmds:?}");
        let (p, q) = (rects[0], rects[1]);
        let apart = p.2 + gap < q.0 || q.2 + gap < p.0 || p.3 + gap < q.1 || q.3 + gap < p.1;
        assert!(apart, "footprints {p:?} and {q:?} within {gap} tiles");
        a.sim.drain_events();
        a.issue(P0, cmds);
        let rejected: Vec<SimEvent> = a
            .sim
            .drain_events()
            .into_iter()
            .filter(|e| matches!(e, SimEvent::CommandRejected { .. }))
            .collect();
        assert!(rejected.is_empty(), "{rejected:?}");
        assert_eq!(a.buildings_of(P0, "muster_hall").len(), 1);
        assert_eq!(a.buildings_of(P0, "scriptorium").len(), 1);
    }

    /// At the first plan the builders the build order takes are never also
    /// sent to gather.
    #[test]
    fn claimed_builder_is_never_sent_to_gather() {
        let r = crate::testkit::rules();
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                place(&r, P0, "croft", 18, 58),
                place(&r, P0, "lumber_yard", 16, 56),
            ],
        );
        assert_eq!(a.sim.view().idle_workers(P0).len(), 5);
        let (cmds, _) = a.plan(P0);
        let builders: Vec<sim::UnitId> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::Build { unit, .. } => Some(*unit),
                _ => None,
            })
            .collect();
        let gatherers: Vec<sim::UnitId> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::Gather { units, .. } => Some(units.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(builders.len(), 2, "a Croft and the Scriptorium: {cmds:?}");
        assert_eq!(gatherers.len(), 3, "the other three gather: {cmds:?}");
        assert!(builders.iter().all(|b| !gatherers.contains(b)));
    }
}
