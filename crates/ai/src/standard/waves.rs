//! Attack waves with a Supply Wain attached (docs/design/ai.md rule 5; M5b
//! owner decision 4).
//!
//! A wave's roster lives in the memory (`BotMemory::wave`), so it is never
//! confused with the home army, and goes through these phases:
//!
//! - **Launch.** No wave is active, the defend latch does not block
//!   launches ([`launch_blocked`]), the bot has reached
//!   `wave_requires_age`, the tick has reached `next_wave_at` and
//!   `first_wave_not_before_ds`, at least [`size`] home armed units stand
//!   within `assemble_radius_tiles` of the rally with at least `min_siege`
//!   siege among them, and a Supply Wain is home. The roster is every
//!   assembled armed unit, siege first then by id, at most
//!   `max_wave_units`, plus one Wain: the armed units attack-move and the
//!   Wain moves to the stage point S ([`stage_point`]), and the next
//!   launch waits `wave_interval_ds`.
//! - **Stage.** Units not on station are sent to S again. The assault
//!   starts once `stage_ready_permille` of the roster's armed units and
//!   every roster siege unit stand within `stage_radius_tiles` of S, or
//!   `stage_max_ds` after the stage began. In that one plan every armed
//!   unit attack-moves to the target Town's centre and the Wain moves to
//!   the Wain point W ([`wain_point`]), so the observer's
//!   `first_attack_tick` counts the whole wave and its Wain together.
//! - **Assault** (the target has hit points). Siege within
//!   `siege_focus_tiles` of the target gets an explicit `Attack` on it;
//!   units not on station attack-move to it again; the Wain is moved back
//!   to W when it is more than `wain_leash_tiles` from it (W moves up as
//!   enemy shooters fall).
//! - **Ring hold** (the target at 0 HP, still the enemy's). The passable
//!   ring tiles of its footprint go, nearest first, to up to `ring_units`
//!   roster units that can annex (units already on the ring count), each
//!   with a single-unit `Move` (M5a's hold rule keeps them there); every
//!   enemy non-civilian unit within `annexation_defender_radius_tiles + 2`
//!   of the Town's centre gets an `Attack` from up to
//!   `clear_defenders_units` of the other roster units; the rest stay on
//!   station at the Town.
//! - **Continue.** Once the target is the bot's, a wave of at least
//!   `continue_min_units` armed units takes the enemy Town nearest the
//!   rally as its next target, with a fresh stage point and timer;
//!   otherwise it comes home.
//! - **End.** A wave with no unit that can annex, fewer than
//!   `recall_below_units` armed units, or older than `wave_max_ds` while
//!   no annexation of the bot's runs at the target comes home: its units
//!   attack-move (the Wain moves) to the rally and the wave ends.
//!
//! **All-in** (from `all_in_after_ds`): every home armed unit joins the
//! active wave each plan and is ordered like the rest of it; a wave ends
//! only when none of its armed units is left, and is never recalled
//! (`defend::should_recall`); a launch needs one home armed unit anywhere,
//! no siege, no Wain and no wait for `next_wave_at`.
//!
//! "On station" (no new order) is [`on_station`] with a tolerance that
//! grows with the group: a group order gives each unit its own tile of a
//! spiral around the point, and around a Town the spiral starts on its
//! ring. A unit on an explicit `Attack` whose target still exists is left
//! alone. The wave issues `AttackMove` into enemy land only at the assault
//! (and the all-in's joiners then), so the first such tick is some wave's
//! assault order (a wave that ends before its assault issues none).

use super::army::home_armed;
use super::ctx::{Ctx, goal_or_post, jitter, on_station, salt};
use super::defend::{all_in, launch_blocked, recall_wave};
use super::facts::{kind_centre, kind_centre_tile, tiles_sq, visible_enemies};
use super::influence::isqrt;
use super::memory::{BotMemory, Wave, WaveKind, WavePhase};
use rules::{Rules, UnitClass, WainRule, WaveMode};
use sim::{Building, BuildingId, Command, Fx, FxVec2, Intent, TargetId, Tile, UnitId, UnitKindId};
use std::collections::BTreeSet;

/// A footprint as inclusive tile bounds `(x0, y0, x1, y1)`.
pub type Rect = (i32, i32, i32, i32);

/// The footprint of building `b`.
///
/// # Panics
///
/// If its kind is not in the rules.
pub fn footprint(rules: &Rules, b: &Building) -> Rect {
    let def = rules
        .building_kind(b.kind.0)
        .expect("a building's kind exists");
    let w = i32::try_from(def.width_tiles).expect("validated <= 8");
    let h = i32::try_from(def.height_tiles).expect("validated <= 8");
    let (x, y) = (i32::from(b.tile.x), i32::from(b.tile.y));
    (x, y, x + w - 1, y + h - 1)
}

/// Squared distance from `p` to the area of the footprint `r` (its tiles'
/// area, `[x0, x1 + 1] x [y0, y1 + 1]`), in [`FxVec2::dist_sq_i64`]'s
/// scale: what a shooter measures to a unit's centre.
pub fn dist_sq_to_rect(p: FxVec2, r: Rect) -> i64 {
    let x = p.x.clamp(Fx::from_int(r.0), Fx::from_int(r.2 + 1));
    let y = p.y.clamp(Fx::from_int(r.1), Fx::from_int(r.3 + 1));
    p.dist_sq_i64(FxVec2::new(x, y))
}

/// `v` hundredths of a tile, squared, in [`FxVec2::dist_sq_i64`]'s scale.
fn x100_sq(v: u32) -> i64 {
    ((i64::from(v) * i64::from(v)) << 32) / 10_000
}

/// Chebyshev distance from tile `t` to the footprint `r` (0 inside).
pub fn chebyshev(r: Rect, t: Tile) -> i32 {
    let (x, y) = (i32::from(t.x), i32::from(t.y));
    let dx = (r.0 - x).max(x - r.2).max(0);
    let dy = (r.1 - y).max(y - r.3).max(0);
    dx.max(dy)
}

/// The enemy's complete shooters with hit points left: footprint and
/// weapon range in hundredths of a tile.
pub fn shooters(ctx: &Ctx<'_>) -> Vec<(Rect, u32)> {
    let rules = ctx.view.rules();
    ctx.facts
        .enemy_shooters
        .iter()
        .filter_map(|id| {
            let b = ctx.view.building(*id)?;
            let a = rules.building_kind(b.kind.0)?.attack.as_ref()?;
            Some((footprint(rules, b), a.range_tiles_x100))
        })
        .collect()
}

/// Whether `p` lies more than `extra_tiles` beyond the range of every
/// shooter in `shooters`.
pub fn out_of_reach(shooters: &[(Rect, u32)], p: FxVec2, extra_tiles: u32) -> bool {
    shooters.iter().all(|(r, range)| {
        dist_sq_to_rect(p, *r) > x100_sq(range.saturating_add(extra_tiles.saturating_mul(100)))
    })
}

/// `num / den` rounded half away from zero (`den > 0`).
fn div_round(num: i32, den: i32) -> i32 {
    if num >= 0 {
        (2 * num + den) / (2 * den)
    } else {
        -((-2 * num + den) / (2 * den))
    }
}

/// The tiles of the integer segment from `a` to `b`, `a` first and `b`
/// last, one per step along the longer axis.
pub fn segment(a: Tile, b: Tile) -> Vec<Tile> {
    let (ax, ay) = (i32::from(a.x), i32::from(a.y));
    let (dx, dy) = (i32::from(b.x) - ax, i32::from(b.y) - ay);
    let n = dx.abs().max(dy.abs());
    if n == 0 {
        return vec![a];
    }
    (0..=n)
        .map(|k| {
            let x = ax + div_round(dx * k, n);
            let y = ay + div_round(dy * k, n);
            Tile::new(
                u16::try_from(x).expect("between two map tiles"),
                u16::try_from(y).expect("between two map tiles"),
            )
        })
        .collect()
}

/// How far a group order of `n` units spreads around its point: the
/// half-side of the square spiral that holds them, plus one.
pub fn spread(n: usize) -> u32 {
    u32::try_from(n.isqrt() / 2 + 1).unwrap_or(u32::MAX)
}

/// The armed units a wave needs to launch now: the `wave_sizes` row of
/// the tick, at most the home armed units plus the free pop (so a full pop
/// never deadlocks a launch), at least 1.
pub fn size(ctx: &Ctx<'_>, home: usize) -> u32 {
    let tick = ctx.tick();
    let table = ctx
        .knobs
        .wave_sizes
        .iter()
        .rev()
        .find(|w| ctx.ticks(w.from_ds) <= tick)
        .map(|w| w.armed)
        .unwrap_or(1);
    let (used, cap) = ctx.facts.pop;
    let home = u32::try_from(home).unwrap_or(u32::MAX);
    table
        .min(home.saturating_add(cap.saturating_sub(used)))
        .max(1)
}

/// Home armed units a launch or the all-in leaves at home: one while the
/// pop is full (nothing could replace it), else none.
pub fn home_guard(ctx: &Ctx<'_>) -> usize {
    let (used, cap) = ctx.facts.pop;
    usize::from(used >= cap)
}

/// Whole tiles between two points (the integer square root of the
/// squared distance).
pub fn dist_tiles(a: FxVec2, b: FxVec2) -> i64 {
    let d = u64::try_from(a.dist_sq_i64(b).max(0) >> 32).unwrap_or(u64::MAX);
    i64::try_from(isqrt(d)).unwrap_or(i64::MAX)
}

/// Whether Town `b` is its owner's Seat.
fn is_seat(b: &Building) -> bool {
    b.town_state.is_some_and(|t| t.seat)
}

/// The siege target score of the enemy Town `b` (lower is better, M6):
/// `dist_tiles x 1000 + target_strength_weight x isqrt(units + shooters)`,
/// the distance from the rally and the enemy units' and shooters'
/// strength over `influence_radius_cells` around the Town's cell.
pub fn target_score(ctx: &mut Ctx<'_>, b: BuildingId) -> Option<i64> {
    let rally = ctx.rally();
    let view = ctx.view;
    let rules = view.rules();
    let t = view.building(b)?;
    let centre = kind_centre(rules, t.kind, t.tile);
    let r = ctx.knobs.influence_radius_cells;
    let weight = i64::from(ctx.knobs.target_strength_weight);
    let inf = ctx.influence();
    let cell = inf.cell_at(centre);
    let strength = inf
        .strength(&inf.enemy_units, cell, r)
        .saturating_add(inf.strength(&inf.enemy_shooters, cell, r));
    let root = i64::try_from(isqrt(u64::try_from(strength.max(0)).unwrap_or(0))).unwrap_or(0);
    Some(
        dist_tiles(centre, rally)
            .saturating_mul(1000)
            .saturating_add(weight.saturating_mul(root)),
    )
}

/// The wave's target (M6): among the complete enemy Towns, the best
/// [`target_score`], ties to the lowest id. With `seat_last` the enemy
/// Seat is skipped while the enemy holds another complete Town that no
/// annexation timer runs on. When the runner-up scores within
/// `noise_mix_permille` of the best, the bot's noise (salted with the
/// launch count) picks between the two.
pub fn choose_target(ctx: &mut Ctx<'_>, launches: u16) -> Option<BuildingId> {
    let view = ctx.view;
    let towns: Vec<BuildingId> = ctx.facts.enemy_towns.clone();
    let seat_last = ctx.knobs.seat_last
        && towns.iter().any(|t| {
            view.building(*t).is_some_and(|b| !is_seat(b)) && view.annexation(*t).is_none()
        });
    let mut scored: Vec<(i64, BuildingId)> = Vec::new();
    for t in towns {
        if seat_last && view.building(t).is_some_and(is_seat) {
            continue;
        }
        if let Some(s) = target_score(ctx, t) {
            scored.push((s, t));
        }
    }
    scored.sort();
    let best = *scored.first()?;
    if let Some(second) = scored.get(1).copied() {
        let band = best
            .0
            .max(0)
            .saturating_mul(i64::from(ctx.knobs.noise_mix_permille))
            / 1000;
        if second.0.saturating_sub(best.0) <= band
            && ctx.noise(salt::TARGET + u64::from(launches)) % 2 == 1
        {
            return Some(second.1);
        }
    }
    Some(best.1)
}

/// Whether a siege wave needs a Supply Wain now (M6, the personality's
/// Wain rule): always for `Always`; for `IfEnemyHarrying` once an enemy
/// has a Harrying level (attrition only exists then).
pub fn wain_needed(ctx: &Ctx<'_>) -> bool {
    match ctx.knobs.wain_rule {
        WainRule::Always => true,
        WainRule::IfEnemyHarrying => ctx.facts.enemy.harrying > 0,
    }
}

/// The extra wait of the first wave (M6): up to `noise_first_wave_ds`,
/// from the bot's noise, in ticks.
pub fn first_wave_extra(ctx: &Ctx<'_>) -> u32 {
    let max = ctx.knobs.noise_first_wave_ds;
    if max == 0 {
        return 0;
    }
    let ds = ctx.noise(salt::FIRST_WAVE) % (u64::from(max) + 1);
    ctx.ticks(u32::try_from(ds).expect("at most MAX_AI_DS"))
}

/// The wait after a launch (M6): `every_ds` in ticks jittered by up to
/// `noise_wave_interval_permille` with the noise of launch `n`.
pub fn interval_after(ctx: &Ctx<'_>, every_ds: u32, n: u16) -> u32 {
    jitter(
        ctx.ticks(every_ds),
        ctx.noise(salt::WAVE_INTERVAL + u64::from(n)),
        ctx.knobs.noise_wave_interval_permille,
    )
}

/// The stage point for a wave of `n` units against `target`: walking the
/// segment from the target's centre tile toward the rally, the first tile
/// with no other player's land within `stage_margin_tiles + spread(n)`
/// (Chebyshev) and more than that beyond every enemy shooter's range,
/// snapped to a passable tile; the rally's tile when none is.
pub fn stage_point(ctx: &mut Ctx<'_>, target: BuildingId, n: usize) -> Tile {
    let rally = ctx.rally();
    let view = ctx.view;
    let map = view.map();
    let rally_tile = map.tile_of(rally);
    let Some(b) = view.building(target) else {
        return rally_tile;
    };
    let t = kind_centre_tile(view.rules(), b.kind, b.tile);
    let margin = ctx.knobs.stage_margin_tiles.saturating_add(spread(n));
    let m = i32::try_from(margin).unwrap_or(i32::MAX / 4);
    let shooters = shooters(ctx);
    let territory = view.territory();
    let me = ctx.me;
    let clear = |tile: Tile| {
        let foreign = (-m..=m).any(|dy| {
            (-m..=m).any(|dx| {
                map.offset(tile, dx, dy)
                    .is_some_and(|o| territory.owner_at(o).is_some_and(|p| p != me))
            })
        });
        !foreign && out_of_reach(&shooters, map.center_of(tile), margin)
    };
    let found = segment(t, rally_tile)
        .into_iter()
        .find(|x| clear(*x))
        .unwrap_or(rally_tile);
    map.nearest_passable(found, None).unwrap_or(found)
}

/// The Supply Wain's point W for an assault on `target` from `stage`: the
/// passable tile of the segment from the target's centre tile to the
/// stage point nearest the target that lies more than
/// `wain_standoff_tiles` beyond every enemy shooter's range (the stage
/// point when none is). While the target still shoots, W keeps out of its
/// range; at 0 HP it moves up.
pub fn wain_point(ctx: &Ctx<'_>, target: BuildingId, stage: Tile) -> FxVec2 {
    let view = ctx.view;
    let map = view.map();
    let Some(b) = view.building(target) else {
        return map.center_of(stage);
    };
    let t = kind_centre_tile(view.rules(), b.kind, b.tile);
    let shooters = shooters(ctx);
    let standoff = ctx.knobs.wain_standoff_tiles;
    segment(t, stage)
        .into_iter()
        .find(|x| map.passable(*x) && out_of_reach(&shooters, map.center_of(*x), standoff))
        .map_or_else(|| map.center_of(stage), |x| map.center_of(x))
}

/// Whether unit kind `k` is siege.
fn is_siege(rules: &Rules, k: UnitKindId) -> bool {
    rules
        .unit_kind(k.0)
        .is_some_and(|d| d.class == UnitClass::Siege)
}

/// Whether unit kind `k` annexes (`annexation_attackers` selects it).
fn annexes(rules: &Rules, k: UnitKindId) -> bool {
    rules
        .unit_kind(k.0)
        .is_some_and(|d| rules.annexation_attackers.iter().any(|s| s.selects(d)))
}

/// Whether `u` is on an explicit `Attack` whose target still stands.
fn attacking(ctx: &Ctx<'_>, u: UnitId) -> bool {
    let Some(unit) = ctx.view.unit(u) else {
        return false;
    };
    unit.intent == Intent::Attack
        && unit.target.is_some_and(|t| match t {
            TargetId::Unit(id) => ctx.view.unit(id).is_some(),
            TargetId::Building(id) => ctx.view.building_hp(id).is_some_and(|(hp, _)| hp > 0),
        })
}

/// Run the rule: launch a wave, or run the active one.
pub fn step(ctx: &mut Ctx<'_>, mem: &mut BotMemory) {
    if mem.wave.is_some() {
        run(ctx, mem);
    } else {
        launch(ctx, mem);
    }
}

/// Whether a launch is due but for the defence and the units: no wave is
/// out, the bot has reached `wave_requires_age`, and the tick has reached
/// `next_wave_at` and `first_wave_not_before_ds` (always, during the
/// all-in). Past its cap the defence sizes its guard by it
/// ([`super::defend`]), so the assembled units are left to the wave.
pub fn launch_due(ctx: &Ctx<'_>, mem: &BotMemory) -> bool {
    if mem.wave.is_some() {
        return false;
    }
    if let Some(age) = &ctx.knobs.wave_requires_age
        && ctx
            .view
            .rules()
            .age_index(age)
            .is_none_or(|a| ctx.facts.age < a)
    {
        return false;
    }
    let all_in = all_in(ctx);
    // M6: the personality's tech gate (never during the all-in, so a lost
    // Watchtower cannot hold the waves back for ever).
    if !all_in
        && let Some(tech) = &ctx.knobs.wave_requires_tech
        && ctx
            .view
            .rules()
            .tech_by_id(tech)
            .is_none_or(|(t, _)| !ctx.view.researched(ctx.me).contains(&sim::TechId(t)))
    {
        return false;
    }
    let not_before = mem
        .next_wave_at
        .max(ctx.ticks(ctx.knobs.first_wave_not_before_ds));
    all_in || ctx.tick() >= not_before
}

/// Launch a wave when every condition holds (see the module docs): the
/// difficulty's deadline raid first (M6), then a raid while a
/// `RaidUntilSiege` personality has no siege assembled, else a siege wave.
fn launch(ctx: &mut Ctx<'_>, mem: &mut BotMemory) {
    let tick = ctx.tick();
    let rules = ctx.view.rules();
    let all_in = all_in(ctx);
    if launch_blocked(ctx, mem) || ctx.commands_left < 2 {
        return;
    }
    if deadline_raid(ctx, mem) || !launch_due(ctx, mem) {
        return;
    }
    let home = home_armed(ctx, mem);
    let rally = ctx.rally();
    let r2 = tiles_sq(ctx.knobs.assemble_radius_tiles);
    let view = ctx.view;
    let assembled: Vec<UnitId> = home
        .iter()
        .copied()
        .filter(|u| {
            all_in
                || view
                    .unit(*u)
                    .is_some_and(|x| x.pos.dist_sq_i64(rally) <= r2)
        })
        .collect();
    let keep = home_guard(ctx);
    let siege = |u: &UnitId| view.unit(*u).is_some_and(|x| is_siege(rules, x.kind));
    let sieges = assembled.iter().filter(|u| siege(u)).count();
    let sieges = u32::try_from(sieges).unwrap_or(u32::MAX);
    // M6: a raiding personality raids while it cannot field a siege wave.
    if !all_in
        && ctx.knobs.wave_mode == WaveMode::RaidUntilSiege
        && sieges < ctx.knobs.min_siege.max(1)
    {
        let raiders: Vec<UnitId> = assembled.iter().copied().filter(|u| !siege(u)).collect();
        let n = raiders.len().saturating_sub(keep);
        if u32::try_from(n).unwrap_or(u32::MAX) >= ctx.knobs.raid_min_armed {
            let max = usize::try_from(ctx.knobs.raid_max_armed).unwrap_or(usize::MAX);
            let mut units = raiders;
            units.truncate(n.min(max));
            if let Some((target, point)) = raid_target(ctx, mem.raids) {
                let every = ctx.knobs.raid_every_ds;
                launch_raid(ctx, mem, units, target, point, every);
            }
        }
        return;
    }
    let (need, min_siege) = if all_in {
        (1, 0)
    } else {
        // M6: an upward-only noise bonus per launch, within the cap.
        let bonus = ctx.noise(salt::WAVE_UNITS + u64::from(mem.waves_launched))
            % (u64::from(ctx.knobs.noise_wave_units) + 1);
        let bonus = u32::try_from(bonus).expect("at most MAX_WAVE_UNITS");
        let need = size(ctx, home.len().saturating_sub(keep))
            .saturating_add(bonus)
            .min(ctx.knobs.max_wave_units.max(1));
        (need, ctx.knobs.min_siege)
    };
    let going = assembled.len().saturating_sub(keep);
    if u32::try_from(going).unwrap_or(u32::MAX) < need || sieges < min_siege {
        return;
    }
    let wain = ctx
        .facts
        .supply
        .iter()
        .copied()
        .find(|w| !ctx.is_claimed(*w));
    if wain.is_none() && !all_in && wain_needed(ctx) {
        return;
    }
    // M6: the first wave waits a little longer, once, by the bot's noise.
    if !all_in && mem.waves_launched == 0 && mem.next_wave_at == 0 {
        let extra = first_wave_extra(ctx);
        if extra > 0 {
            mem.next_wave_at = tick.saturating_add(extra);
            return;
        }
    }
    let Some(target) = choose_target(ctx, mem.waves_launched) else {
        return;
    };
    let mut roster = assembled;
    roster.sort_by_key(|u| (!siege(u), *u));
    roster.truncate(going.min(usize::try_from(ctx.knobs.max_wave_units).unwrap_or(usize::MAX)));
    roster.sort();
    let stage = stage_point(ctx, target, roster.len() + usize::from(wain.is_some()));
    let s = ctx.view.map().center_of(stage);
    for u in roster.iter().chain(wain.iter()) {
        ctx.claim(*u);
    }
    ctx.order(roster.clone(), |units| Command::AttackMove {
        units,
        target: s,
    });
    if let Some(w) = wain {
        ctx.push(Command::Move {
            units: vec![w],
            target: s,
            queue: false,
        });
    }
    let launched_units = u16::try_from(roster.len()).unwrap_or(u16::MAX);
    mem.wave = Some(Wave {
        units: roster,
        wain,
        target,
        stage,
        phase: WavePhase::Stage,
        launched_at: tick,
        phase_at: tick,
        kind: WaveKind::Siege,
        launched_units,
        last_contact_at: tick,
    });
    mem.waves_launched = mem.waves_launched.saturating_add(1);
    mem.next_wave_at = tick.saturating_add(interval_after(
        ctx,
        ctx.knobs.wave_interval_ds,
        mem.waves_launched,
    ));
}

/// The difficulty's deadline raid (M6): once the tick reaches the first
/// raid's `by_ds` with no wave launched yet, and at least `min_armed`
/// non-siege armed units are home (anywhere, not only at the rally), up to
/// `max_armed` of them (by id, the home guard kept) raid at once. It
/// bypasses the age, tech, not-before, siege and Wain gates and does not
/// stage. Returns whether it launched.
fn deadline_raid(ctx: &mut Ctx<'_>, mem: &mut BotMemory) -> bool {
    let Some(raid) = ctx.first_raid else {
        return false;
    };
    if mem.raid_used || mem.waves_launched > 0 || ctx.tick() < ctx.ticks(raid.by_ds) {
        return false;
    }
    let view = ctx.view;
    let rules = view.rules();
    let keep = home_guard(ctx);
    let mut units: Vec<UnitId> = home_armed(ctx, mem)
        .into_iter()
        .filter(|u| view.unit(*u).is_some_and(|x| !is_siege(rules, x.kind)))
        .collect();
    let n = units.len().saturating_sub(keep);
    if u32::try_from(n).unwrap_or(u32::MAX) < raid.min_armed {
        return false;
    }
    units.truncate(n.min(usize::try_from(raid.max_armed).unwrap_or(usize::MAX)));
    let Some((target, point)) = raid_target(ctx, mem.raids) else {
        return false;
    };
    let every = ctx.knobs.raid_every_ds;
    launch_raid(ctx, mem, units, target, point, every);
    mem.raid_used = true;
    true
}

/// Launch a raid of `units` (sorted) at `point` (raiding `target`): one
/// `AttackMove` straight there, no Wain, no stage; the next launch waits
/// `every_ds` (jittered).
fn launch_raid(
    ctx: &mut Ctx<'_>,
    mem: &mut BotMemory,
    mut units: Vec<UnitId>,
    target: BuildingId,
    point: Tile,
    every_ds: u32,
) {
    let tick = ctx.tick();
    units.sort();
    let goal = ctx.view.map().center_of(point);
    for u in &units {
        ctx.claim(*u);
    }
    ctx.order(units.clone(), |units| Command::AttackMove {
        units,
        target: goal,
    });
    let launched_units = u16::try_from(units.len()).unwrap_or(u16::MAX);
    mem.wave = Some(Wave {
        units,
        wain: None,
        target,
        stage: point,
        phase: WavePhase::Assault,
        launched_at: tick,
        phase_at: tick,
        kind: WaveKind::Raid,
        launched_units,
        last_contact_at: tick,
    });
    mem.waves_launched = mem.waves_launched.saturating_add(1);
    mem.raids = mem.raids.saturating_add(1);
    mem.next_wave_at = mem.next_wave_at.max(tick.saturating_add(interval_after(
        ctx,
        every_ds,
        mem.waves_launched,
    )));
}

/// The raid's target and point (M6): the enemy complete building with
/// workers on it (`Facts::enemy`'s worked buildings) whose centre lies
/// more than `stage_margin_tiles` beyond every enemy shooter's range, with
/// the best score `raid_worker_weight x workers - isqrt(enemy unit
/// strength near it) - dist_tiles x raid_distance_permille / 1000` (the
/// distance from the rally), ties to the lowest id, the bot's noise
/// picking between the top two; its point is the building's centre tile.
/// When none qualifies (Passive has only its shooting Seat), the
/// fallback is the passable enemy-owned tile nearest the rally beyond
/// every shooter's reach plus the margin (ties to the mirror-aware tile
/// index), raiding the enemy Town nearest it.
pub fn raid_target(ctx: &mut Ctx<'_>, n: u16) -> Option<(BuildingId, Tile)> {
    let rally = ctx.rally();
    let view = ctx.view;
    let rules = view.rules();
    let map = view.map();
    let shooters = shooters(ctx);
    let margin = ctx.knobs.stage_margin_tiles;
    let r = ctx.knobs.influence_radius_cells;
    let weight = i64::from(ctx.knobs.raid_worker_weight);
    let per_tile = i64::from(ctx.knobs.raid_distance_permille);
    let worked = ctx.facts.enemy.worked.clone();
    let mut scored: Vec<(i64, BuildingId, Tile)> = Vec::new();
    for (b, workers) in worked {
        let Some(bb) = view.building(b) else {
            continue;
        };
        let centre = kind_centre(rules, bb.kind, bb.tile);
        if !out_of_reach(&shooters, centre, margin) {
            continue;
        }
        let inf = ctx.influence();
        let near = inf.strength(&inf.enemy_units, inf.cell_at(centre), r);
        let near = i64::try_from(isqrt(u64::try_from(near.max(0)).unwrap_or(0))).unwrap_or(0);
        let score = weight
            .saturating_mul(i64::from(workers))
            .saturating_sub(near)
            .saturating_sub(dist_tiles(centre, rally).saturating_mul(per_tile) / 1000);
        scored.push((
            score.saturating_neg(),
            b,
            kind_centre_tile(rules, bb.kind, bb.tile),
        ));
    }
    scored.sort();
    if let Some(best) = scored.first().copied() {
        let pick = match scored.get(1) {
            Some(second) if ctx.noise(salt::TARGET + (1 << 20) + u64::from(n)) % 2 == 1 => *second,
            _ => best,
        };
        return Some((pick.1, pick.2));
    }
    let territory = view.territory();
    let me = ctx.me;
    let rally_tile = map.tile_of(rally);
    let mut best: Option<(i64, u32, Tile)> = None;
    for y in 0..map.height() {
        for x in 0..map.width() {
            let t = Tile::new(x, y);
            if territory.owner_at(t).is_none_or(|p| p == me) || !map.passable(t) {
                continue;
            }
            let dx = i64::from(x) - i64::from(rally_tile.x);
            let dy = i64::from(y) - i64::from(rally_tile.y);
            let key = (dx * dx + dy * dy, ctx.map.tie_index(t, 1, 1), t);
            if best.is_some_and(|b| (b.0, b.1) <= (key.0, key.1)) {
                continue;
            }
            if out_of_reach(&shooters, map.center_of(t), margin) {
                best = Some(key);
            }
        }
    }
    let point = best?.2;
    let at = map.center_of(point);
    let target = ctx
        .facts
        .enemy_towns
        .iter()
        .filter_map(|t| {
            let b = view.building(*t)?;
            Some((kind_centre(rules, b.kind, b.tile).dist_sq_i64(at), *t))
        })
        .min()?
        .1;
    Some((target, point))
}

/// End the wave: its units come home ([`recall_wave`]). Waits for a plan
/// with room for the two commands, so no unit is left behind. A wave that
/// ends with fewer than `recall_below_units` armed units counts as wiped
/// (M6).
fn end(ctx: &mut Ctx<'_>, mem: &mut BotMemory, wave: Wave) {
    let wiped = u32::try_from(wave.units.len()).unwrap_or(u32::MAX) < ctx.knobs.recall_below_units;
    mem.wave = Some(wave);
    if ctx.commands_left >= 2 {
        if wiped {
            mem.wiped = mem.wiped.saturating_add(1);
        }
        recall_wave(ctx, mem);
    }
}

/// Run the active wave (see the module docs).
fn run(ctx: &mut Ctx<'_>, mem: &mut BotMemory) {
    let Some(mut wave) = mem.wave.clone() else {
        return;
    };
    let view = ctx.view;
    let rules = view.rules();
    let me = ctx.me;
    let tick = ctx.tick();
    let mine = |u: &UnitId| view.unit(*u).is_some_and(|x| x.owner == me);
    wave.units.retain(mine);
    if wave.wain.is_some_and(|w| !mine(&w)) {
        wave.wain = None;
    }
    if wave.kind == WaveKind::Raid {
        run_raid(ctx, mem, wave);
        return;
    }
    let all_in = all_in(ctx);
    if all_in {
        let max = usize::try_from(ctx.knobs.max_wave_units).unwrap_or(usize::MAX);
        let room = max.saturating_sub(wave.units.len());
        let home = home_armed(ctx, mem);
        let free = home.len().saturating_sub(home_guard(ctx));
        let joiners: Vec<UnitId> = home.into_iter().take(room.min(free)).collect();
        wave.units.extend(joiners);
        wave.units.sort();
    }
    let annexers = wave
        .units
        .iter()
        .filter(|u| view.unit(**u).is_some_and(|x| annexes(rules, x.kind)))
        .count();
    let armed = u32::try_from(wave.units.len()).unwrap_or(u32::MAX);
    let Some(target) = view.building(wave.target) else {
        end(ctx, mem, wave);
        return;
    };
    if target.owner == me {
        if !all_in && armed < ctx.knobs.continue_min_units {
            end(ctx, mem, wave);
            return;
        }
        let Some(next) = choose_target(ctx, mem.waves_launched) else {
            end(ctx, mem, wave);
            return;
        };
        wave.target = next;
        wave.stage = stage_point(ctx, next, wave.units.len() + 1);
        wave.phase = WavePhase::Stage;
        wave.launched_at = tick;
        wave.phase_at = tick;
        stage_upkeep(ctx, &wave);
        mem.wave = Some(wave);
        return;
    }
    let annexing = view
        .annexation(wave.target)
        .is_some_and(|a| a.attacker == me);
    let over = if all_in {
        wave.units.is_empty()
    } else {
        annexers == 0
            || armed < ctx.knobs.recall_below_units
            || (tick.saturating_sub(wave.launched_at) >= ctx.ticks(ctx.knobs.wave_max_ds)
                && !annexing)
    };
    if over {
        end(ctx, mem, wave);
        return;
    }
    match wave.phase {
        WavePhase::Stage => {
            if staged(ctx, &wave) && ctx.commands_left >= 3 {
                start_assault(ctx, &mut wave);
            } else {
                stage_upkeep(ctx, &wave);
            }
        }
        WavePhase::Assault | WavePhase::Hold => {
            if view.building_hp(wave.target).is_some_and(|(hp, _)| hp > 0) {
                wave.phase = WavePhase::Assault;
                assault_upkeep(ctx, &wave);
            } else {
                wave.phase = WavePhase::Hold;
                hold(ctx, &wave);
            }
        }
    }
    mem.wave = Some(wave);
}

/// Run the active raid (M6). Its contact timer is refreshed while any
/// enemy unit stands within `raid_idle_tiles` of one of its units; it
/// ends (its units come home) with fewer than `recall_below_units` armed
/// units, after `raid_idle_ds` with no contact, or `raid_max_ds` after the
/// launch. When its target is gone (destroyed, or no longer the enemy's)
/// it takes a new one while at least `continue_min_units` remain, and
/// ends otherwise. Units off station attack-move to the raid point again.
fn run_raid(ctx: &mut Ctx<'_>, mem: &mut BotMemory, mut wave: Wave) {
    let view = ctx.view;
    let me = ctx.me;
    let tick = ctx.tick();
    let reach = tiles_sq(ctx.knobs.raid_idle_tiles);
    let contact = wave
        .units
        .iter()
        .filter_map(|u| view.unit(*u))
        .any(|u| visible_enemies(&view, me).any(|e| e.pos.dist_sq_i64(u.pos) <= reach));
    if contact {
        wave.last_contact_at = tick;
    }
    let armed = u32::try_from(wave.units.len()).unwrap_or(u32::MAX);
    if armed < ctx.knobs.recall_below_units
        || tick.saturating_sub(wave.last_contact_at) >= ctx.ticks(ctx.knobs.raid_idle_ds)
        || tick.saturating_sub(wave.launched_at) >= ctx.ticks(ctx.knobs.raid_max_ds)
    {
        end(ctx, mem, wave);
        return;
    }
    let gone = view.building(wave.target).is_none_or(|b| b.owner == me);
    if gone {
        if armed < ctx.knobs.continue_min_units {
            end(ctx, mem, wave);
            return;
        }
        let Some((target, point)) = raid_target(ctx, mem.raids) else {
            end(ctx, mem, wave);
            return;
        };
        wave.target = target;
        wave.stage = point;
    }
    let goal = ctx.view.map().center_of(wave.stage);
    let tol = station(ctx, wave.units.len());
    let moves = off_station(ctx, &wave.units, goal, tol);
    for u in &moves {
        ctx.claim(*u);
    }
    ctx.order(moves, |units| Command::AttackMove {
        units,
        target: goal,
    });
    mem.wave = Some(wave);
}

/// Whether the stage is over: `stage_ready_permille` of the armed units
/// and every siege unit within `stage_radius_tiles` of the stage point, or
/// `stage_max_ds` since the stage began.
fn staged(ctx: &Ctx<'_>, wave: &Wave) -> bool {
    if ctx.tick().saturating_sub(wave.phase_at) >= ctx.ticks(ctx.knobs.stage_max_ds) {
        return true;
    }
    let view = ctx.view;
    let rules = view.rules();
    let s = view.map().center_of(wave.stage);
    let r2 = tiles_sq(ctx.knobs.stage_radius_tiles);
    let mut near = 0u64;
    for u in &wave.units {
        let Some(x) = view.unit(*u) else { continue };
        let here = x.pos.dist_sq_i64(s) <= r2;
        if here {
            near += 1;
        } else if is_siege(rules, x.kind) {
            return false;
        }
    }
    let total = u64::try_from(wave.units.len()).unwrap_or(u64::MAX);
    near * 1000 >= total * u64::from(ctx.knobs.stage_ready_permille)
}

/// The on-station tolerance of a wave of `n` units around a point.
fn station(ctx: &Ctx<'_>, n: usize) -> u32 {
    ctx.knobs.goal_tolerance_tiles.saturating_add(spread(n))
}

/// The on-station tolerance around the target Town's centre: a group's
/// spiral starts on the Town's ring, outside its footprint.
fn town_station(ctx: &Ctx<'_>, wave: &Wave) -> u32 {
    let half = ctx
        .view
        .building(wave.target)
        .map(|b| footprint(ctx.view.rules(), b))
        .map_or(0, |r| {
            u32::try_from((r.2 - r.0).max(r.3 - r.1) / 2 + 1).unwrap_or(0)
        });
    station(ctx, wave.units.len()).saturating_add(half)
}

/// Units of `units` not claimed, not on an explicit attack and not on
/// station within `tol` tiles of `goal`.
fn off_station(ctx: &Ctx<'_>, units: &[UnitId], goal: FxVec2, tol: u32) -> Vec<UnitId> {
    units
        .iter()
        .copied()
        .filter(|u| !ctx.is_claimed(*u) && !attacking(ctx, *u))
        .filter(|u| ctx.view.unit(*u).is_some_and(|x| !on_station(x, goal, tol)))
        .collect()
}

/// Keep the Wain at `at`: a `Move` when its goal is more than `leash`
/// tiles away.
fn keep_wain(ctx: &mut Ctx<'_>, wave: &Wave, at: FxVec2, leash: u32) {
    let Some(w) = wave.wain else { return };
    if ctx.is_claimed(w) || ctx.view.unit(w).is_none_or(|x| on_station(x, at, leash)) {
        return;
    }
    ctx.claim(w);
    ctx.push(Command::Move {
        units: vec![w],
        target: at,
        queue: false,
    });
}

/// Staging: units off station attack-move to the stage point again, the
/// Wain moves there.
fn stage_upkeep(ctx: &mut Ctx<'_>, wave: &Wave) {
    let s = ctx.view.map().center_of(wave.stage);
    let tol = station(ctx, wave.units.len());
    let moves = off_station(ctx, &wave.units, s, tol);
    for u in &moves {
        ctx.claim(*u);
    }
    ctx.order(moves, |units| Command::AttackMove { units, target: s });
    keep_wain(ctx, wave, s, tol);
}

/// The assault's first plan: every armed unit attack-moves to the target
/// Town (siege as its own group) and the Wain moves to W.
fn start_assault(ctx: &mut Ctx<'_>, wave: &mut Wave) {
    let view = ctx.view;
    let rules = view.rules();
    let Some(b) = view.building(wave.target) else {
        return;
    };
    let centre = kind_centre(rules, b.kind, b.tile);
    let (siege, other): (Vec<UnitId>, Vec<UnitId>) = wave
        .units
        .iter()
        .copied()
        .filter(|u| !ctx.is_claimed(*u))
        .partition(|u| view.unit(*u).is_some_and(|x| is_siege(rules, x.kind)));
    for u in siege.iter().chain(other.iter()) {
        ctx.claim(*u);
    }
    ctx.order(other, |units| Command::AttackMove {
        units,
        target: centre,
    });
    ctx.order(siege, |units| Command::AttackMove {
        units,
        target: centre,
    });
    let w = wain_point(ctx, wave.target, wave.stage);
    if let Some(wain) = wave.wain
        && !ctx.is_claimed(wain)
    {
        ctx.claim(wain);
        ctx.push(Command::Move {
            units: vec![wain],
            target: w,
            queue: false,
        });
    }
    wave.phase = WavePhase::Assault;
    wave.phase_at = ctx.tick();
}

/// The assault while the target has hit points: siege focus, units back
/// on station, the Wain on its leash.
fn assault_upkeep(ctx: &mut Ctx<'_>, wave: &Wave) {
    let view = ctx.view;
    let rules = view.rules();
    let Some(b) = view.building(wave.target) else {
        return;
    };
    let centre = kind_centre(rules, b.kind, b.tile);
    let focus_r2 = tiles_sq(ctx.knobs.siege_focus_tiles);
    let goal = TargetId::Building(wave.target);
    let focus: Vec<UnitId> = wave
        .units
        .iter()
        .copied()
        .filter(|u| !ctx.is_claimed(*u))
        .filter(|u| {
            view.unit(*u).is_some_and(|x| {
                is_siege(rules, x.kind)
                    && x.pos.dist_sq_i64(centre) <= focus_r2
                    && !(x.intent == Intent::Attack && x.target == Some(goal))
            })
        })
        .collect();
    if !focus.is_empty()
        && view
            .check_attack(ctx.me, &focus, goal)
            .is_ok_and(|v| !v.is_empty())
    {
        for u in &focus {
            ctx.claim(*u);
        }
        ctx.push(Command::Attack {
            units: focus,
            target: goal,
        });
    }
    let tol = town_station(ctx, wave);
    let moves = off_station(ctx, &wave.units, centre, tol);
    for u in &moves {
        ctx.claim(*u);
    }
    ctx.order(moves, |units| Command::AttackMove {
        units,
        target: centre,
    });
    let w = wain_point(ctx, wave.target, wave.stage);
    keep_wain(ctx, wave, w, ctx.knobs.wain_leash_tiles);
}

/// The ring hold while the target is at 0 HP and still the enemy's: ring
/// tiles to annexing units, defenders attacked, the rest on station.
fn hold(ctx: &mut Ctx<'_>, wave: &Wave) {
    let view = ctx.view;
    let rules = view.rules();
    let map = view.map();
    let Some(b) = view.building(wave.target) else {
        return;
    };
    let rect = footprint(rules, b);
    let centre = kind_centre(rules, b.kind, b.tile);
    let ring: Vec<Tile> = (rect.1 - 1..=rect.3 + 1)
        .flat_map(|y| (rect.0 - 1..=rect.2 + 1).map(move |x| (x, y)))
        .filter_map(|(x, y)| Some(Tile::new(u16::try_from(x).ok()?, u16::try_from(y).ok()?)))
        .filter(|t| map.passable(*t) && chebyshev(rect, *t) == 1)
        .collect();
    let ring_set: BTreeSet<Tile> = ring.iter().copied().collect();
    // A unit is on the ring when it stands on a ring tile or is heading
    // for (or holds) one.
    let ring_tile_of = |u: UnitId| -> Option<Tile> {
        let x = view.unit(u)?;
        let here = map.tile_of(x.pos);
        if ring_set.contains(&here) {
            return Some(here);
        }
        goal_or_post(x)
            .map(|g| map.tile_of(g))
            .filter(|t| ring_set.contains(t))
    };
    let mut on_ring: BTreeSet<UnitId> = BTreeSet::new();
    let mut taken: BTreeSet<Tile> = BTreeSet::new();
    for u in &wave.units {
        if view.unit(*u).is_some_and(|x| annexes(rules, x.kind))
            && let Some(t) = ring_tile_of(*u)
        {
            on_ring.insert(*u);
            taken.insert(t);
        }
    }
    let want = usize::try_from(ctx.knobs.ring_units).unwrap_or(usize::MAX);
    let mut free: Vec<Tile> = ring
        .iter()
        .copied()
        .filter(|t| !taken.contains(t))
        .collect();
    let mut candidates: Vec<UnitId> = wave
        .units
        .iter()
        .copied()
        .filter(|u| !on_ring.contains(u) && !ctx.is_claimed(*u))
        .filter(|u| view.unit(*u).is_some_and(|x| annexes(rules, x.kind)))
        .collect();
    while on_ring.len() < want && !free.is_empty() && !candidates.is_empty() {
        let Some((_, u, t)) = candidates
            .iter()
            .flat_map(|u| free.iter().map(move |t| (*u, *t)))
            .filter_map(|(u, t)| {
                let p = view.unit(u)?.pos;
                Some((p.dist_sq_i64(map.center_of(t)), u, t))
            })
            .min()
        else {
            break;
        };
        candidates.retain(|c| *c != u);
        free.retain(|f| *f != t);
        on_ring.insert(u);
        ctx.claim(u);
        if !ctx.push(Command::Move {
            units: vec![u],
            target: map.center_of(t),
            queue: false,
        }) {
            return;
        }
    }
    // Defenders: every enemy non-civilian unit near the Town's centre, read
    // through the fog seam.
    let reach = tiles_sq(rules.annexation_defender_radius_tiles.saturating_add(2));
    let defenders: Vec<UnitId> = visible_enemies(&view, ctx.me)
        .filter(|x| x.pos.dist_sq_i64(centre) <= reach)
        .filter(|x| {
            rules
                .unit_kind(x.kind.0)
                .is_some_and(|k| k.class != UnitClass::Civilian)
        })
        .map(|x| x.id)
        .collect();
    let per = usize::try_from(ctx.knobs.clear_defenders_units).unwrap_or(usize::MAX);
    let mut pool: Vec<UnitId> = wave
        .units
        .iter()
        .copied()
        .filter(|u| !on_ring.contains(u) && !ctx.is_claimed(*u))
        .collect();
    for d in defenders {
        let Some(dp) = view.unit(d).map(|x| x.pos) else {
            continue;
        };
        let already: Vec<UnitId> = pool
            .iter()
            .copied()
            .filter(|u| {
                view.unit(*u).is_some_and(|x| {
                    x.intent == Intent::Attack && x.target == Some(TargetId::Unit(d))
                })
            })
            .collect();
        pool.retain(|u| !already.contains(u));
        let mut near: Vec<(i64, UnitId)> = pool
            .iter()
            .filter_map(|u| Some((view.unit(*u)?.pos.dist_sq_i64(dp), *u)))
            .collect();
        near.sort();
        let send: Vec<UnitId> = near
            .into_iter()
            .take(per.saturating_sub(already.len()))
            .map(|(_, u)| u)
            .collect();
        if send.is_empty() {
            continue;
        }
        pool.retain(|u| !send.contains(u));
        if view
            .check_attack(ctx.me, &send, TargetId::Unit(d))
            .is_ok_and(|v| !v.is_empty())
        {
            for u in &send {
                ctx.claim(*u);
            }
            if !ctx.push(Command::Attack {
                units: send,
                target: TargetId::Unit(d),
            }) {
                return;
            }
        }
    }
    let rest: Vec<UnitId> = wave
        .units
        .iter()
        .copied()
        .filter(|u| !on_ring.contains(u))
        .collect();
    let tol = town_station(ctx, wave);
    let moves = off_station(ctx, &rest, centre, tol);
    for u in &moves {
        ctx.claim(*u);
    }
    ctx.order(moves, |units| Command::AttackMove {
        units,
        target: centre,
    });
    let w = wain_point(ctx, wave.target, wave.stage);
    keep_wain(ctx, wave, w, ctx.knobs.wain_leash_tiles);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standard::defend;
    use crate::standard::{Ctx, ctx::jitter};
    use crate::testkit::{Arena, P0, P1, place, spawn, standard_vs_passive};
    use rules::Personality;
    use sim::MatchSetup;

    /// P0's rally in these arenas: the rally seed (36,64) is passable own
    /// land.
    const RALLY: (i32, i32) = (36, 64);

    fn wave(units: Vec<UnitId>, wain: Option<UnitId>, target: BuildingId, stage: Tile) -> Wave {
        let launched_units = u16::try_from(units.len()).unwrap();
        Wave {
            units,
            wain,
            target,
            stage,
            phase: WavePhase::Stage,
            launched_at: 0,
            phase_at: 0,
            kind: WaveKind::Siege,
            launched_units,
            last_contact_at: 0,
        }
    }

    /// The setup of a launch at P0's rally: Masonry (or not), `skirm`
    /// Skirmishers, `mang` Mangonels and `wains` Supply Wains.
    fn at_rally(r: &Rules, age: bool, skirm: u16, mang: u16, wains: u16) -> Vec<Command> {
        let (x, y) = RALLY;
        let mut v = Vec::new();
        if age {
            v.push(Command::DebugAdvanceAge { owner: P0 });
        }
        for (kind, n) in [
            ("skirmisher", skirm),
            ("mangonel", mang),
            ("supply_wain", wains),
        ] {
            if n > 0 {
                v.push(spawn(r, P0, kind, x, y, n));
            }
        }
        v
    }

    fn knobs(r: &Rules, edit: impl FnOnce(&mut Personality)) -> Personality {
        let mut k = r.ai.personality("standard").unwrap().clone();
        edit(&mut k);
        k
    }

    /// A launch needs the age, the size, the siege, a Wain and the timer;
    /// then every assembled armed unit attack-moves and the Wain moves to
    /// the stage point, which lies off the enemy's land.
    #[test]
    fn launch_needs_age_size_siege_wain_and_timer() {
        let r = crate::testkit::rules();
        // M6: noise off (the M5b timing; noise has its own tests).
        let quiet_k = quiet(&r, |_| {});
        let a = Arena::frozen(standard_vs_passive(&r), at_rally(&r, true, 7, 1, 1));
        let mut armed = a.units_of(P0, "skirmisher");
        armed.extend(a.units_of(P0, "mangonel"));
        armed.sort();
        let wain = a.units_of(P0, "supply_wain")[0];
        let mut ctx = Ctx::new(a.sim.view(), P0, &quiet_k, a.map(P0));
        assert_eq!(size(&ctx, 8), 8, "the table's first row");
        let mut mem = BotMemory::default();
        step(&mut ctx, &mut mem);
        let tick = a.sim.tick();
        let w = mem.wave.clone().expect("a wave launches");
        assert_eq!(w.units, armed);
        assert_eq!(
            (w.wain, w.target, w.phase),
            (Some(wain), a.seat(P1), WavePhase::Stage)
        );
        assert_eq!((w.launched_at, w.phase_at), (tick, tick));
        assert_eq!(mem.waves_launched, 1);
        assert_eq!(
            mem.next_wave_at,
            tick + ctx.ticks(ctx.knobs.wave_interval_ds)
        );
        let view = a.sim.view();
        let s = view.map().center_of(w.stage);
        assert_eq!(
            ctx.into_commands(),
            [
                Command::AttackMove {
                    units: armed,
                    target: s
                },
                Command::Move {
                    units: vec![wain],
                    target: s,
                    queue: false
                }
            ]
        );
        assert_ne!(view.territory().owner_at(w.stage), Some(P1));

        // Each condition missing in turn: no launch.
        for (why, setup) in [
            ("no Masonry", at_rally(&r, false, 7, 1, 1)),
            ("7 armed of 8", at_rally(&r, true, 6, 1, 1)),
            ("no siege", at_rally(&r, true, 8, 0, 1)),
            ("no Wain", at_rally(&r, true, 7, 1, 0)),
        ] {
            let a = Arena::frozen(standard_vs_passive(&r), setup);
            let mut ctx = Ctx::new(a.sim.view(), P0, &quiet_k, a.map(P0));
            let mut mem = BotMemory::default();
            step(&mut ctx, &mut mem);
            assert!(mem.wave.is_none(), "{why}");
            assert!(ctx.into_commands().is_empty(), "{why}");
        }
        let mut ctx = Ctx::new(a.sim.view(), P0, &quiet_k, a.map(P0));
        let mut mem = BotMemory {
            next_wave_at: a.sim.tick() + 1,
            ..BotMemory::default()
        };
        step(&mut ctx, &mut mem);
        assert!(mem.wave.is_none(), "the wave interval");
        // Away from the rally, the units are not assembled.
        let (x, y) = RALLY;
        let far = Arena::frozen(
            standard_vs_passive(&r),
            [
                at_rally(&r, true, 0, 1, 1),
                vec![spawn(&r, P0, "skirmisher", x - 14, y, 7)],
            ]
            .concat(),
        );
        let mut ctx = Ctx::new(far.sim.view(), P0, &quiet_k, far.map(P0));
        let mut mem = BotMemory::default();
        step(&mut ctx, &mut mem);
        assert!(mem.wave.is_none(), "not assembled");
    }

    /// The wave size is the table's row, at most the home armed units plus
    /// the free pop, never below 1; at a full pop one armed unit stays
    /// home.
    #[test]
    fn size_respects_free_pop() {
        let r = crate::testkit::rules();
        // M6: noise off (the M5b timing; noise has its own tests).
        let quiet_k = quiet(&r, |_| {});
        let (x, y) = RALLY;
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![spawn(&r, P0, "skirmisher", x, y, 5)],
        );
        let ctx = Ctx::new(a.sim.view(), P0, &quiet_k, a.map(P0));
        assert_eq!(ctx.facts.pop, (10, 25));
        assert_eq!((size(&ctx, 5), home_guard(&ctx)), (8, 0));
        // A full pop: 5 + 13 Yeomen, 2 Skirmishers, a Mangonel and a Wain.
        let full = Arena::frozen(
            standard_vs_passive(&r),
            [
                at_rally(&r, true, 2, 1, 1),
                vec![spawn(&r, P0, "yeoman", 20, 70, 13)],
            ]
            .concat(),
        );
        let mut ctx = Ctx::new(full.sim.view(), P0, &quiet_k, full.map(P0));
        assert_eq!(ctx.facts.pop, (25, 25));
        assert_eq!((size(&ctx, 3), size(&ctx, 0)), (3, 1));
        assert_eq!(home_guard(&ctx), 1);
        let mut mem = BotMemory::default();
        step(&mut ctx, &mut mem);
        let w = mem.wave.expect("a wave of two launches");
        let mangonel = full.units_of(P0, "mangonel")[0];
        let skirm = full.units_of(P0, "skirmisher");
        let mut want = vec![mangonel, skirm[0]];
        want.sort();
        assert_eq!(w.units, want, "siege first, one Skirmisher stays home");
    }

    /// The assault waits for every siege unit at the stage point (or for
    /// `stage_max_ds`), then sends the armed units at the target's centre,
    /// siege as its own group, and the Wain to its point.
    #[test]
    fn stage_waits_for_siege() {
        let r = crate::testkit::rules();
        let probe = Arena::frozen(standard_vs_passive(&r), Vec::new());
        let target = probe.seat(P1);
        let stage = stage_point(&mut probe.ctx(P0), target, 12);
        let (sx, sy) = (i32::from(stage.x), i32::from(stage.y));
        let (rx, ry) = RALLY;
        let seat_centre = {
            let view = probe.sim.view();
            let b = view.building(target).unwrap();
            kind_centre(&r, b.kind, b.tile)
        };
        for (mangonel_at, timed_out, assault) in [
            ((rx, ry), false, false),
            ((rx, ry), true, true),
            ((sx, sy), false, true),
        ] {
            let mut a = Arena::frozen(
                standard_vs_passive(&r),
                vec![
                    spawn(&r, P0, "skirmisher", sx, sy, 8),
                    spawn(&r, P0, "mangonel", mangonel_at.0, mangonel_at.1, 1),
                    spawn(&r, P0, "supply_wain", sx, sy, 1),
                ],
            );
            let stage_max = a.ctx(P0).ticks(a.ctx(P0).knobs.stage_max_ds);
            a.run(stage_max);
            let skirm = a.units_of(P0, "skirmisher");
            let mangonel = a.units_of(P0, "mangonel")[0];
            let wain = a.units_of(P0, "supply_wain")[0];
            let mut units = skirm.clone();
            units.push(mangonel);
            units.sort();
            let tick = a.sim.tick();
            let mut w = wave(units, Some(wain), target, stage);
            w.launched_at = tick;
            w.phase_at = if timed_out { tick - stage_max } else { tick };
            let mut mem = BotMemory {
                wave: Some(w),
                ..BotMemory::default()
            };
            let mut ctx = a.ctx(P0);
            step(&mut ctx, &mut mem);
            let phase = mem.wave.as_ref().unwrap().phase;
            let w_point = wain_point(&ctx, target, stage);
            let cmds = ctx.into_commands();
            let at_town = |units: &[UnitId]| {
                cmds.contains(&Command::AttackMove {
                    units: units.to_vec(),
                    target: seat_centre,
                })
            };
            assert_eq!(phase == WavePhase::Assault, assault, "{cmds:?}");
            assert_eq!(at_town(&skirm), assault, "{cmds:?}");
            assert_eq!(at_town(&[mangonel]), assault, "{cmds:?}");
            assert_eq!(
                cmds.contains(&Command::Move {
                    units: vec![wain],
                    target: w_point,
                    queue: false
                }),
                assault,
                "{cmds:?}"
            );
            if !assault {
                let s = a.sim.view().map().center_of(stage);
                assert!(
                    cmds.iter().any(|c| matches!(c, Command::AttackMove { units, target } if units.contains(&mangonel) && *target == s)),
                    "the Mangonel is sent to the stage point: {cmds:?}"
                );
            }
        }
    }

    /// W keeps `wain_standoff_tiles` beyond the range of every enemy
    /// shooter (the target Town and a Watchtower), as near the target as
    /// that allows on the segment to the stage point; with both at 0 HP it
    /// moves up beside the Town.
    #[test]
    fn wain_point_is_out_of_every_shooters_range() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            vec![place(&r, P1, "watchtower", 92, 66)],
        );
        let target = a.seat(P1);
        let tower = a.buildings_of(P1, "watchtower")[0];
        let mut ctx = a.ctx(P0);
        let stage = stage_point(&mut ctx, target, 10);
        let standoff = ctx.knobs.wain_standoff_tiles;
        let both = shooters(&ctx);
        assert_eq!(both.len(), 2, "the Seat and the tower");
        let w = wain_point(&ctx, target, stage);
        assert!(out_of_reach(&both, w, standoff));
        // The tile before W on the segment from the target is not.
        let view = a.sim.view();
        let b = view.building(target).unwrap();
        let line = segment(kind_centre_tile(&r, b.kind, b.tile), stage);
        let at = line
            .iter()
            .position(|t| view.map().center_of(*t) == w)
            .expect("W lies on the segment");
        assert!(!out_of_reach(
            &both,
            view.map().center_of(line[at - 1]),
            standoff
        ));
        // The tower alone pushes W back: without it W would be nearer.
        let seat_only: Vec<(Rect, u32)> = both
            .iter()
            .copied()
            .filter(|(rect, _)| *rect == footprint(&r, b))
            .collect();
        let nearer = line
            .iter()
            .position(|t| out_of_reach(&seat_only, view.map().center_of(*t), standoff))
            .unwrap();
        assert!(nearer < at, "{nearer} {at}");
        a.issue(
            P0,
            vec![
                Command::DebugDamage {
                    target: TargetId::Building(target),
                    amount: 5000,
                },
                Command::DebugDamage {
                    target: TargetId::Building(tower),
                    amount: 400,
                },
            ],
        );
        let ctx = a.ctx(P0);
        assert_eq!(shooters(&ctx).len(), 1, "the tower still shoots at 100 HP");
        a.issue(
            P0,
            vec![Command::DebugDamage {
                target: TargetId::Building(tower),
                amount: 400,
            }],
        );
        let ctx = a.ctx(P0);
        assert!(shooters(&ctx).is_empty());
        let w = wain_point(&ctx, target, stage);
        let view = a.sim.view();
        let rect = footprint(&r, view.building(target).unwrap());
        assert_eq!(chebyshev(rect, view.map().tile_of(w)), 1, "beside the Town");
    }

    /// At 0 HP the Town's ring tiles go to distinct annexing units, the
    /// nearest first, one `Move` each; units already on the ring count,
    /// and the annexation starts once they stand there.
    #[test]
    fn ring_hold_assigns_distinct_ring_tiles() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            vec![spawn(&r, P0, "skirmisher", 96, 64, 6)],
        );
        let target = a.seat(P1);
        a.issue(
            P0,
            vec![Command::DebugDamage {
                target: TargetId::Building(target),
                amount: 5000,
            }],
        );
        let units = a.units_of(P0, "skirmisher");
        let mut w = wave(units.clone(), None, target, Tile::new(70, 64));
        w.phase = WavePhase::Assault;
        let mut mem = BotMemory {
            wave: Some(w),
            ..BotMemory::default()
        };
        a.set_mem(P0, &mem);
        let mut ctx = a.ctx(P0);
        step(&mut ctx, &mut mem);
        assert_eq!(mem.wave.as_ref().unwrap().phase, WavePhase::Hold);
        let cmds = ctx.into_commands();
        let view = a.sim.view();
        let rect = footprint(&r, view.building(target).unwrap());
        let moves: Vec<(UnitId, Tile)> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::Move { units, target, .. } if units.len() == 1 => {
                    Some((units[0], view.map().tile_of(*target)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(moves.len(), 4, "ring_units: {cmds:?}");
        let tiles: BTreeSet<Tile> = moves.iter().map(|(_, t)| *t).collect();
        let movers: BTreeSet<UnitId> = moves.iter().map(|(u, _)| *u).collect();
        assert_eq!((tiles.len(), movers.len()), (4, 4), "distinct");
        for t in &tiles {
            assert_eq!(chebyshev(rect, *t), 1);
            assert!(view.map().passable(*t));
        }
        // Run the plans: the ring holds and the annexation starts.
        for _ in 0..40 {
            a.act(P0);
            a.run(7);
        }
        let annex = a.sim.view().annexation(target).expect("annexing");
        assert_eq!(annex.attacker, P0);
        let (cmds, _) = a.plan(P0);
        assert!(
            cmds.iter()
                .all(|c| !matches!(c, Command::Move { units, .. } if units.len() == 1)),
            "the ring is held: {cmds:?}"
        );
    }

    /// Enemy military near a Town at 0 HP is attacked by the roster units
    /// not on the ring, up to `clear_defenders_units` each.
    #[test]
    fn clears_defenders_with_attack() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            vec![spawn(&r, P0, "skirmisher", 95, 64, 10)],
        );
        let target = a.seat(P1);
        a.issue(
            P1,
            vec![
                Command::DebugDamage {
                    target: TargetId::Building(target),
                    amount: 5000,
                },
                spawn(&r, P1, "shieldbearer", 104, 70, 1),
                spawn(&r, P1, "shieldbearer", 104, 58, 1),
            ],
        );
        let defenders = a.units_of(P1, "shieldbearer");
        assert_eq!(defenders.len(), 2);
        let units = a.units_of(P0, "skirmisher");
        let mut w = wave(units, None, target, Tile::new(70, 64));
        w.phase = WavePhase::Hold;
        let mut mem = BotMemory {
            wave: Some(w),
            ..BotMemory::default()
        };
        let mut ctx = a.ctx(P0);
        step(&mut ctx, &mut mem);
        let cmds = ctx.into_commands();
        let ring: BTreeSet<UnitId> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::Move { units, .. } if units.len() == 1 => Some(units[0]),
                _ => None,
            })
            .collect();
        assert_eq!(ring.len(), 4);
        let attacks: Vec<(TargetId, Vec<UnitId>)> = cmds
            .iter()
            .filter_map(|c| match c {
                Command::Attack { units, target } => Some((*target, units.clone())),
                _ => None,
            })
            .collect();
        let sizes: Vec<(TargetId, usize)> = attacks.iter().map(|(t, u)| (*t, u.len())).collect();
        assert_eq!(
            sizes,
            [
                (TargetId::Unit(defenders[0]), 4),
                (TargetId::Unit(defenders[1]), 2)
            ],
            "{cmds:?}"
        );
        for (_, units) in &attacks {
            assert!(units.iter().all(|u| !ring.contains(u)), "ring units hold");
        }
    }

    /// A wave ends (its units come home) with no unit that can annex, with
    /// fewer than `recall_below_units` armed units, or after
    /// `wave_max_ds`. The siege-only wave has `recall_below_units`
    /// Mangonels, so only the annexer rule can end it.
    #[test]
    fn wave_ends_without_annexers() {
        let r = crate::testkit::rules();
        let below = r.ai.personality("standard").unwrap().recall_below_units;
        assert!(3 >= below, "the siege-only case needs {below} Mangonels");
        let (x, y) = (80, 64);
        for (why, kind, n, age, ends) in [
            ("siege only", "mangonel", 3, 0, true),
            ("two armed", "skirmisher", 2, 0, true),
            ("three armed", "skirmisher", 3, 0, false),
            ("too old", "skirmisher", 3, 7200, true),
        ] {
            let mut a = Arena::frozen(
                standard_vs_passive(&r),
                vec![
                    Command::DebugAdvanceAge { owner: P0 },
                    spawn(&r, P0, kind, x, y, n),
                ],
            );
            a.run(age);
            let units = a.units_of(P0, kind);
            let tick = a.sim.tick();
            let mut w = wave(units.clone(), None, a.seat(P1), Tile::new(72, 64));
            w.launched_at = tick - age;
            w.phase_at = tick;
            let mut mem = BotMemory {
                wave: Some(w),
                ..BotMemory::default()
            };
            let mut ctx = a.ctx(P0);
            let rally = ctx.rally();
            step(&mut ctx, &mut mem);
            assert_eq!(mem.wave.is_none(), ends, "{why}");
            // M6: only a wave that ends with fewer than `recall_below_units`
            // armed units counts as wiped; one that ends by the annexer
            // rule or by age with 3 does not, nor one that goes on.
            assert_eq!(mem.wiped, u16::from(why == "two armed"), "{why}: wiped");
            let home = ctx.into_commands().contains(&Command::AttackMove {
                units,
                target: rally,
            });
            assert_eq!(home, ends, "{why}");
        }
    }

    /// Once the target is the bot's, a wave of at least
    /// `continue_min_units` armed units retargets to the enemy Town nearest
    /// the rally with a fresh stage point; a smaller one comes home. (M6:
    /// without `seat_last` and noise, which have their own tests.)
    #[test]
    fn retargets_after_capture() {
        let r = crate::testkit::rules();
        let quiet_k = quiet(&r, |k| k.seat_last = false);
        for (n, goes_on) in [(4, true), (3, false)] {
            let a = Arena::frozen(
                standard_vs_passive(&r),
                vec![
                    place(&r, P1, "town", 106, 81),
                    spawn(&r, P0, "skirmisher", 60, 64, n),
                ],
            );
            let units = a.units_of(P0, "skirmisher");
            // The wave's target is the bot's own Town now.
            let mut w = wave(units.clone(), None, a.seat(P0), Tile::new(40, 64));
            w.phase = WavePhase::Hold;
            let mut mem = BotMemory {
                wave: Some(w),
                ..BotMemory::default()
            };
            let mut ctx = Ctx::new(a.sim.view(), P0, &quiet_k, a.map(P0));
            let rally = ctx.rally();
            let next = a.seat(P1);
            let stage = stage_point(&mut ctx, next, n as usize + 1);
            step(&mut ctx, &mut mem);
            let cmds = ctx.into_commands();
            if goes_on {
                let w = mem.wave.expect("goes on");
                assert_eq!(
                    (w.target, w.stage, w.phase),
                    (next, stage, WavePhase::Stage)
                );
                assert_eq!((w.launched_at, w.phase_at), (a.sim.tick(), a.sim.tick()));
                let s = a.sim.view().map().center_of(stage);
                assert!(
                    cmds.contains(&Command::AttackMove { units, target: s }),
                    "{cmds:?}"
                );
            } else {
                assert!(mem.wave.is_none());
                assert!(cmds.contains(&Command::AttackMove {
                    units,
                    target: rally
                }));
            }
        }
    }

    /// From `all_in_after_ds` every home armed unit joins the active wave
    /// and is sent with it, the wave goes on, and a threat at home recalls
    /// nothing.
    #[test]
    fn all_in_joins_every_home_unit_and_is_never_recalled() {
        let r = crate::testkit::rules();
        let (x, y) = RALLY;
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                Command::DebugAdvanceAge { owner: P0 },
                spawn(&r, P0, "mangonel", 80, 64, 1),
                spawn(&r, P0, "skirmisher", x, y, 3),
            ],
        );
        let k = knobs(&r, |k| k.all_in_after_ds = 0);
        let siege = a.units_of(P0, "mangonel");
        let home = a.units_of(P0, "skirmisher");
        let target = a.seat(P1);
        let mut w = wave(siege.clone(), None, target, Tile::new(72, 64));
        w.phase = WavePhase::Stage;
        w.phase_at = a.sim.tick();
        let mut mem = BotMemory {
            wave: Some(w),
            ..BotMemory::default()
        };
        let mut ctx = a.ctx(P0);
        ctx.knobs = &k;
        step(&mut ctx, &mut mem);
        let w = mem.wave.clone().expect("the wave goes on");
        let mut all = siege.clone();
        all.extend(home.iter().copied());
        all.sort();
        assert_eq!(w.units, all, "the home units joined");
        let s = a.sim.view().map().center_of(w.stage);
        let cmds = ctx.into_commands();
        assert!(
            cmds.iter().any(|c| matches!(c, Command::AttackMove { units, target } if units.contains(&home[0]) && *target == s)),
            "{cmds:?}"
        );
        // A raider at home: no recall during the all-in.
        let raided = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                spawn(&r, P0, "skirmisher", 80, 64, 3),
                spawn(&r, P1, "skirmisher", 24, 51, 1),
            ],
        );
        let units = raided.units_of(P0, "skirmisher");
        let mut mem = BotMemory {
            wave: Some(wave(units, None, target, Tile::new(72, 64))),
            ..BotMemory::default()
        };
        let mut ctx = raided.ctx(P0);
        ctx.knobs = &k;
        defend::step(&mut ctx, &mut mem);
        assert!(mem.defend.is_some(), "the raid is seen");
        assert!(
            mem.wave.is_some(),
            "a staging wave stays out during the all-in"
        );
    }

    /// During the all-in a wave ends only when none of its armed units is
    /// left: two Mangonels alone, with no home unit to join them (no
    /// annexer and fewer than `recall_below_units`, which ends the same
    /// wave before the all-in), stay out.
    #[test]
    fn all_in_keeps_a_wave_of_siege_alone_out() {
        let r = crate::testkit::rules();
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                Command::DebugAdvanceAge { owner: P0 },
                spawn(&r, P0, "mangonel", 80, 64, 2),
            ],
        );
        let siege = a.units_of(P0, "mangonel");
        let target = a.seat(P1);
        let standard = r.ai.personality("standard").unwrap().all_in_after_ds;
        for (all_in_after_ds, stays) in [(standard, false), (0, true)] {
            let k = knobs(&r, |k| k.all_in_after_ds = all_in_after_ds);
            let mut w = wave(siege.clone(), None, target, Tile::new(72, 64));
            w.phase_at = a.sim.tick();
            let mut mem = BotMemory {
                wave: Some(w),
                ..BotMemory::default()
            };
            let mut ctx = a.ctx(P0);
            ctx.knobs = &k;
            let rally = ctx.rally();
            step(&mut ctx, &mut mem);
            assert_eq!(mem.wave.is_some(), stays, "all-in from {all_in_after_ds}");
            let cmds = ctx.into_commands();
            let home = cmds
                .iter()
                .any(|c| matches!(c, Command::AttackMove { target, .. } if *target == rally));
            assert_eq!(home, !stays, "all-in from {all_in_after_ds}: {cmds:?}");
            if stays {
                assert_eq!(mem.wave.unwrap().units, siege, "no unit joined");
            }
        }
    }

    /// The all-in launches with one home armed unit anywhere: no size,
    /// siege or Wain, not assembled at the rally, and no wait for
    /// `next_wave_at` (the age gate still holds); before the all-in the
    /// same army launches nothing.
    #[test]
    fn all_in_launches_one_unit_without_siege_wain_or_timer() {
        let r = crate::testkit::rules();
        let (x, y) = RALLY;
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                Command::DebugAdvanceAge { owner: P0 },
                spawn(&r, P0, "skirmisher", x - 14, y, 1),
            ],
        );
        let skirm = a.units_of(P0, "skirmisher");
        let standard = r.ai.personality("standard").unwrap().all_in_after_ds;
        for (all_in_after_ds, launches) in [(standard, false), (0, true)] {
            let k = knobs(&r, |k| k.all_in_after_ds = all_in_after_ds);
            let mut mem = BotMemory {
                next_wave_at: a.sim.tick() + 1000,
                ..BotMemory::default()
            };
            let mut ctx = a.ctx(P0);
            ctx.knobs = &k;
            step(&mut ctx, &mut mem);
            let cmds = ctx.into_commands();
            assert_eq!(
                mem.wave.is_some(),
                launches,
                "all-in from {all_in_after_ds}: {cmds:?}"
            );
            if launches {
                let w = mem.wave.unwrap();
                assert_eq!(
                    (&w.units, w.wain, w.phase),
                    (&skirm, None, WavePhase::Stage)
                );
                let s = a.sim.view().map().center_of(w.stage);
                assert_eq!(
                    cmds,
                    [Command::AttackMove {
                        units: skirm.clone(),
                        target: s
                    }]
                );
            } else {
                assert!(cmds.is_empty(), "{cmds:?}");
            }
        }
    }

    #[test]
    fn segments_and_rects() {
        let s = segment(Tile::new(10, 10), Tile::new(14, 12));
        assert_eq!(
            s,
            [
                Tile::new(10, 10),
                Tile::new(11, 11),
                Tile::new(12, 11),
                Tile::new(13, 12),
                Tile::new(14, 12)
            ]
        );
        assert_eq!(segment(Tile::new(3, 3), Tile::new(3, 3)), [Tile::new(3, 3)]);
        let back = segment(Tile::new(14, 12), Tile::new(10, 10));
        assert_eq!(back.first(), Some(&Tile::new(14, 12)));
        assert_eq!(back.last(), Some(&Tile::new(10, 10)));
        let r = (10, 10, 12, 12);
        assert_eq!(chebyshev(r, Tile::new(11, 11)), 0);
        assert_eq!(chebyshev(r, Tile::new(9, 13)), 1);
        assert_eq!(chebyshev(r, Tile::new(15, 11)), 3);
        assert_eq!(dist_sq_to_rect(FxVec2::from_ints(11, 11), r), 0);
        assert_eq!(dist_sq_to_rect(FxVec2::from_ints(16, 11), r), tiles_sq(3));
        assert_eq!((spread(1), spread(9), spread(24)), (1, 2, 3));
    }

    // ---- M6 ----

    /// The `standard` knobs with every noise knob off (the M5b timing).
    fn quiet(r: &Rules, edit: impl FnOnce(&mut Personality)) -> Personality {
        knobs(r, |k| {
            k.noise_wave_interval_permille = 0;
            k.noise_wave_units = 0;
            k.noise_first_wave_ds = 0;
            k.noise_mix_permille = 0;
            k.noise_site_top_k = 1;
            edit(k);
        })
    }

    /// A raid of `units` at `point` against `target`, launched at
    /// `launched_at`, last in contact at `contact`.
    fn raid(
        units: Vec<UnitId>,
        target: BuildingId,
        point: Tile,
        launched_at: u32,
        contact: u32,
    ) -> Wave {
        let launched_units = u16::try_from(units.len()).unwrap();
        Wave {
            units,
            wain: None,
            target,
            stage: point,
            phase: WavePhase::Assault,
            launched_at,
            phase_at: launched_at,
            kind: WaveKind::Raid,
            launched_units,
            last_contact_at: contact,
        }
    }

    /// P1's gathering buildings, each worked by the units sent to it.
    fn worked_p1(cmds: Vec<Command>, jobs: &[(&str, &str, u16)]) -> Arena {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(standard_vs_passive(&r), cmds);
        let scribes = a.units_of(P1, "scribe");
        let yeomen = a.units_of(P1, "yeoman");
        let (mut s, mut y) = (scribes.into_iter(), yeomen.into_iter());
        let mut orders = Vec::new();
        for (kind, worker, n) in jobs {
            let node = *a
                .buildings_of(P1, kind)
                .iter()
                .find(|b| {
                    !orders
                        .iter()
                        .any(|c| matches!(c, Command::Gather { node, .. } if node == *b))
                })
                .expect("a node of the kind");
            let units: Vec<UnitId> = (0..*n)
                .filter_map(|_| {
                    if *worker == "scribe" {
                        s.next()
                    } else {
                        y.next()
                    }
                })
                .collect();
            orders.push(Command::Gather { units, node });
        }
        a.issue(P1, orders);
        a
    }

    /// M6: the raid goes for the worked enemy building outside every
    /// shooter's reach (plus the stage margin) with the best score; a
    /// building inside the reach of the Seat or a Watchtower is never
    /// picked however many work it, and between the top two the bot's
    /// noise picks.
    #[test]
    fn raid_targets_the_most_worked_building_outside_tower_reach() {
        let r = crate::testkit::rules();
        let base = vec![
            // A: out of every reach, three Scribes.
            place(&r, P1, "scriptorium", 86, 57),
            // B: beside the Seat, three Scribes.
            place(&r, P1, "scriptorium", 96, 61),
            // C: a Croft under a Watchtower.
            place(&r, P1, "croft", 90, 74),
            place(&r, P1, "watchtower", 92, 77),
            spawn(&r, P1, "scribe", 92, 58, 6),
        ];
        let a = worked_p1(
            base.clone(),
            &[
                ("scriptorium", "scribe", 3),
                ("scriptorium", "scribe", 3),
                ("croft", "yeoman", 1),
            ],
        );
        let scriptoria = a.buildings_of(P1, "scriptorium");
        let croft = a.buildings_of(P1, "croft")[0];
        let mut ctx = a.ctx(P0);
        let worked: Vec<BuildingId> = ctx.facts.enemy.worked.iter().map(|(b, _)| *b).collect();
        assert_eq!(worked.len(), 3, "{:?}", ctx.facts.enemy.worked);
        let (target, point) = raid_target(&mut ctx, 0).expect("a target");
        assert_eq!(target, scriptoria[0], "the one out of reach");
        let view = a.sim.view();
        let b = view.building(target).unwrap();
        assert_eq!(point, kind_centre_tile(&r, b.kind, b.tile));
        assert!(target != scriptoria[1] && target != croft);

        // A second Croft out of reach with one Yeoman: the noise picks
        // between the two, the Scriptorium (three workers) scoring best.
        let mut more = base;
        more.push(place(&r, P1, "croft", 86, 64));
        let a = worked_p1(
            more,
            &[
                ("scriptorium", "scribe", 3),
                ("scriptorium", "scribe", 3),
                ("croft", "yeoman", 1),
                ("croft", "yeoman", 1),
            ],
        );
        let crofts = a.buildings_of(P1, "croft");
        let scriptorium = a.buildings_of(P1, "scriptorium")[0];
        let mut ctx = a.ctx(P0);
        let second = ctx.noise(salt::TARGET + (1 << 20)) % 2 == 1;
        let (target, _) = raid_target(&mut ctx, 0).unwrap();
        assert_eq!(target, if second { crofts[1] } else { scriptorium });
    }

    /// M6: against Passive (only its shooting Seat, nothing worked) the
    /// raid's point is the passable tile of P1's land nearest the rally
    /// that lies beyond every shooter's reach plus the margin.
    #[test]
    fn raid_falls_back_to_the_nearest_enemy_tile_against_passive() {
        let r = crate::testkit::rules();
        let a = Arena::frozen(standard_vs_passive(&r), Vec::new());
        let mut ctx = a.ctx(P0);
        assert!(ctx.facts.enemy.worked.is_empty());
        let (target, point) = raid_target(&mut ctx, 0).expect("a fallback");
        assert_eq!(target, a.seat(P1));
        let view = a.sim.view();
        let map = view.map();
        let territory = view.territory();
        assert_eq!(territory.owner_at(point), Some(P1));
        assert!(map.passable(point));
        let shooters = shooters(&ctx);
        let margin = ctx.knobs.stage_margin_tiles;
        assert!(out_of_reach(&shooters, map.center_of(point), margin));
        let rally = map.tile_of(ctx.rally());
        let d = |t: Tile| {
            let (dx, dy) = (
                i64::from(t.x) - i64::from(rally.x),
                i64::from(t.y) - i64::from(rally.y),
            );
            dx * dx + dy * dy
        };
        for y in 0..map.height() {
            for x in 0..map.width() {
                let t = Tile::new(x, y);
                if d(t) < d(point) && territory.owner_at(t) == Some(P1) && map.passable(t) {
                    assert!(
                        !out_of_reach(&shooters, map.center_of(t), margin),
                        "{t:?} is nearer and out of reach"
                    );
                }
            }
        }
        // It is on P1's side of the map, short of the Seat's reach.
        assert!(point.x > 64 && point.x < 100, "{point:?}");
    }

    /// M6: a raid comes home after `raid_idle_ds` with no enemy near, or
    /// `raid_max_ds` after the launch; an enemy near keeps it going; when
    /// its target is gone it takes a new one while `continue_min_units`
    /// remain and comes home otherwise.
    #[test]
    fn raid_ends_when_idle_gone_or_old() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                spawn(&r, P0, "skirmisher", 70, 40, 6),
                place(&r, P1, "croft", 90, 74),
            ],
        );
        a.run(2500);
        let tick = a.sim.tick();
        let units = a.units_of(P0, "skirmisher");
        let seat = a.seat(P1);
        let point = Tile::new(70, 40);
        let k = quiet(&r, |_| {});
        let idle = k.raid_idle_ds * 2;
        let run_one = |a: &Arena, w: Wave| -> (BotMemory, Vec<Command>) {
            let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
            let mut mem = BotMemory {
                wave: Some(w),
                ..BotMemory::default()
            };
            step(&mut ctx, &mut mem);
            (mem, ctx.into_commands())
        };
        // Idle: launched a moment ago, no contact for long.
        let (mem, cmds) = run_one(&a, raid(units.clone(), seat, point, tick - 10, tick - idle));
        assert!(mem.wave.is_none(), "idle");
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Command::AttackMove { units: u, .. } if *u == units)),
            "{cmds:?}"
        );
        // Old: in contact, but launched past raid_max_ds.
        let (mem, _) = run_one(&a, raid(units.clone(), seat, point, 0, tick));
        assert!(mem.wave.is_none(), "old");
        // A recent, fresh raid goes on.
        let (mem, _) = run_one(&a, raid(units.clone(), seat, point, tick - 10, tick - 10));
        assert!(mem.wave.is_some(), "fresh");
        // Contact refreshes the timer.
        let mut near = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                spawn(&r, P0, "skirmisher", 70, 40, 6),
                // Out of the Skirmishers' acquire range (6), inside the
                // raid's contact radius (10).
                spawn(&r, P1, "yeoman", 78, 40, 1),
            ],
        );
        near.run(2500);
        let tick = near.sim.tick();
        let (mem, _) = run_one(
            &near,
            raid(
                near.units_of(P0, "skirmisher"),
                near.seat(P1),
                point,
                tick - 10,
                tick - idle,
            ),
        );
        assert_eq!(mem.wave.map(|w| w.last_contact_at), Some(tick), "contact");
        // Target gone: six left retarget, three come home.
        let croft = a.buildings_of(P1, "croft")[0];
        a.issue(
            P0,
            vec![Command::DebugDamage {
                target: TargetId::Building(croft),
                amount: u32::MAX,
            }],
        );
        assert!(a.sim.view().building(croft).is_none(), "destroyed");
        let tick = a.sim.tick();
        let (mem, _) = run_one(&a, raid(units.clone(), croft, point, tick - 10, tick - 10));
        let w = mem.wave.expect("retargeted");
        assert_ne!(w.target, croft);
        let (mem, _) = run_one(
            &a,
            raid(units[..3].to_vec(), croft, point, tick - 10, tick - 10),
        );
        assert!(mem.wave.is_none(), "too few to go on");
    }

    /// M6: under `IfEnemyHarrying` a siege wave needs no Wain until the
    /// enemy has a Harrying level; then it does.
    #[test]
    fn wain_needed_only_after_enemy_harrying() {
        let r = crate::testkit::rules();
        let k = quiet(&r, |k| k.wain_rule = rules::WainRule::IfEnemyHarrying);
        let a = Arena::frozen(standard_vs_passive(&r), at_rally(&r, true, 7, 1, 0));
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        assert!(!wain_needed(&ctx));
        let mut mem = BotMemory::default();
        step(&mut ctx, &mut mem);
        let w = mem.wave.expect("a siege wave without a Wain");
        assert_eq!((w.kind, w.wain), (WaveKind::Siege, None));
        let mut a = Arena::frozen(standard_vs_passive(&r), at_rally(&r, true, 7, 1, 0));
        a.issue(P0, vec![crate::testkit::research(&r, P1, "harrying_1")]);
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        assert!(wain_needed(&ctx), "Harrying I");
        let mut mem = BotMemory::default();
        step(&mut ctx, &mut mem);
        assert!(mem.wave.is_none(), "now it waits for a Wain");
    }

    /// M6: the `standard` personality keeps the M5b Wain rule: no Wain, no
    /// wave, whatever the enemy has researched.
    #[test]
    fn standard_still_needs_a_wain() {
        let r = crate::testkit::rules();
        let k = quiet(&r, |_| {});
        assert_eq!(k.wain_rule, rules::WainRule::Always);
        let a = Arena::frozen(standard_vs_passive(&r), at_rally(&r, true, 7, 1, 0));
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        assert!(wain_needed(&ctx));
        let mut mem = BotMemory::default();
        step(&mut ctx, &mut mem);
        assert!(mem.wave.is_none());
    }

    /// M6: Hard's deadline raid launches once its tick is reached with no
    /// wave out yet: without Masonry, without siege or a Wain, from units
    /// away from the rally, at most `max_armed` of them, with one
    /// `AttackMove` straight onto the enemy's land (no stage); not with
    /// fewer than `min_armed`, not twice.
    #[test]
    fn hard_deadline_raid_launches_by_12000_without_masonry_and_does_not_stage() {
        let r = crate::testkit::rules();
        let hard = r.ai.difficulty("hard").unwrap();
        let first = hard.first_raid.expect("Hard has a deadline raid");
        assert_eq!(r.ticks_from_ds(first.by_ds), 12_000);
        let k = quiet(&r, |_| {});
        let raid_now = rules::FirstRaid { by_ds: 0, ..first };
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![spawn(&r, P0, "skirmisher", 30, 50, 12)],
        );
        let units = a.units_of(P0, "skirmisher");
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        ctx.first_raid = Some(raid_now);
        let mut mem = BotMemory::default();
        step(&mut ctx, &mut mem);
        let w = mem.wave.clone().expect("the deadline raid");
        let max = usize::try_from(first.max_armed).unwrap();
        assert_eq!(w.units, units[..max].to_vec());
        assert_eq!(
            (w.kind, w.phase, w.wain),
            (WaveKind::Raid, WavePhase::Assault, None)
        );
        assert!(mem.raid_used && mem.raids == 1 && mem.waves_launched == 1);
        let view = a.sim.view();
        assert_eq!(
            view.territory().owner_at(w.stage),
            Some(P1),
            "onto P1's land"
        );
        assert_eq!(
            ctx.into_commands(),
            [Command::AttackMove {
                units: units[..max].to_vec(),
                target: view.map().center_of(w.stage)
            }],
            "one order, no stage"
        );
        // Before the tick, short of min_armed, or once used: nothing.
        let few = Arena::frozen(
            standard_vs_passive(&r),
            vec![spawn(
                &r,
                P0,
                "skirmisher",
                30,
                50,
                u16::try_from(first.min_armed - 1).unwrap(),
            )],
        );
        let mut ctx = Ctx::new(few.sim.view(), P0, &k, few.map(P0));
        ctx.first_raid = Some(raid_now);
        let mut m = BotMemory::default();
        step(&mut ctx, &mut m);
        assert!(m.wave.is_none(), "short of min_armed");
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        ctx.first_raid = Some(first);
        let mut m = BotMemory::default();
        step(&mut ctx, &mut m);
        assert!(m.wave.is_none(), "before 12000");
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        ctx.first_raid = Some(raid_now);
        let mut m = BotMemory {
            raid_used: true,
            ..BotMemory::default()
        };
        step(&mut ctx, &mut m);
        assert!(m.wave.is_none(), "once only");
    }

    /// M6: Easy plays its personality with the Masonry gate and every wave
    /// at the largest size, capped at 14.
    #[test]
    fn easy_waits_for_masonry_and_sends_largest_capped_waves() {
        let r = crate::testkit::rules();
        let mut setup = MatchSetup::bots(&r, 1, ("easy", "standard"), ("passive", ""));
        setup.debug_commands = true;
        let a = Arena::frozen(setup.clone(), at_rally(&r, false, 20, 1, 1));
        let ctx = a.ctx(P0);
        let easy = r.ai.difficulty("easy").unwrap();
        let cap = easy.max_wave_units.unwrap();
        assert_eq!(ctx.knobs.max_wave_units, cap);
        assert!(ctx.knobs.wave_sizes.iter().all(|w| w.armed == cap));
        assert_eq!(size(&ctx, 40), cap, "the largest row, capped, from tick 0");
        assert_eq!(ctx.knobs.wave_requires_age.as_deref(), Some("masonry"));
        let mut mem = BotMemory {
            waves_launched: 1,
            ..BotMemory::default()
        };
        let mut ctx = a.ctx(P0);
        step(&mut ctx, &mut mem);
        assert!(mem.wave.is_none(), "no Masonry");
        let a = Arena::frozen(setup.clone(), at_rally(&r, true, 20, 1, 1));
        let mut ctx = a.ctx(P0);
        let mut mem = BotMemory {
            waves_launched: 1,
            ..BotMemory::default()
        };
        step(&mut ctx, &mut mem);
        let w = mem.wave.expect("a wave");
        assert_eq!(w.units.len(), usize::try_from(cap).unwrap(), "capped");
        let short = u16::try_from(cap - 2).unwrap();
        let a = Arena::frozen(setup, at_rally(&r, true, short, 1, 1));
        let mut ctx = a.ctx(P0);
        let mut mem = BotMemory {
            waves_launched: 1,
            ..BotMemory::default()
        };
        step(&mut ctx, &mut mem);
        assert!(mem.wave.is_none(), "{} armed of {cap}", cap - 1);
    }

    /// M6: with `seat_last` the wave skips the enemy Seat while the enemy
    /// holds another complete Town that no annexation runs on, even when
    /// the Seat scores better; without it the best score wins.
    #[test]
    fn seat_last_skips_the_seat_while_another_town_stands() {
        let r = crate::testkit::rules();
        // A second P1 Town to the south-east: farther from P0's rally
        // than the Seat.
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![place(&r, P1, "town", 104, 84)],
        );
        let seat = a.seat(P1);
        let other = a
            .buildings_of(P1, "town")
            .into_iter()
            .find(|b| *b != seat)
            .unwrap();
        let k = quiet(&r, |_| {});
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        assert!(target_score(&mut ctx, seat) < target_score(&mut ctx, other));
        assert_eq!(choose_target(&mut ctx, 0), Some(other));
        let free = quiet(&r, |k| k.seat_last = false);
        let mut ctx = Ctx::new(a.sim.view(), P0, &free, a.map(P0));
        assert_eq!(choose_target(&mut ctx, 0), Some(seat));
        // Only the Seat left: it is the target.
        let alone = Arena::frozen(standard_vs_passive(&r), Vec::new());
        let mut ctx = Ctx::new(alone.sim.view(), P0, &k, alone.map(P0));
        assert_eq!(choose_target(&mut ctx, 0), Some(alone.seat(P1)));
    }

    /// M6: `seat_last` skips the Seat only while another enemy Town is
    /// uncontested: once the bot's own annexation timer runs on the only
    /// other Town (0 HP, P0's Skirmishers on its ring, no P1 defender), the
    /// Seat is a target again and, scoring better, is the one chosen.
    #[test]
    fn seat_last_takes_the_seat_when_the_other_town_is_contested() {
        let r = crate::testkit::rules();
        let mut a = Arena::frozen(
            standard_vs_passive(&r),
            vec![place(&r, P1, "town", 104, 84)],
        );
        let seat = a.seat(P1);
        let other = a
            .buildings_of(P1, "town")
            .into_iter()
            .find(|b| *b != seat)
            .unwrap();
        let k = quiet(&r, |_| {});
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        assert_eq!(choose_target(&mut ctx, 0), Some(other), "uncontested");
        a.issue(
            P0,
            vec![
                Command::DebugDamage {
                    target: TargetId::Building(other),
                    amount: 5000,
                },
                spawn(&r, P0, "skirmisher", 103, 86, 2),
            ],
        );
        a.run(5);
        let view = a.sim.view();
        assert_eq!(view.annexation(other).map(|x| x.attacker), Some(P0));
        assert_eq!(view.building(other).map(|b| b.owner), Some(P1));
        let mut ctx = Ctx::new(view, P0, &k, a.map(P0));
        assert!(
            ctx.facts.enemy_towns.contains(&other),
            "still an enemy Town"
        );
        assert!(target_score(&mut ctx, seat) < target_score(&mut ctx, other));
        assert_eq!(choose_target(&mut ctx, 0), Some(seat), "contested");
    }

    /// M6: the target score adds the enemy's strength at a Town: an
    /// army parked at the nearer Town sends the wave to the other.
    #[test]
    fn influence_scored_target_avoids_a_defended_town() {
        let r = crate::testkit::rules();
        let a = Arena::frozen(
            standard_vs_passive(&r),
            vec![
                place(&r, P1, "town", 104, 84),
                spawn(&r, P1, "shieldbearer", 98, 64, 30),
            ],
        );
        let seat = a.seat(P1);
        let other = a
            .buildings_of(P1, "town")
            .into_iter()
            .find(|b| *b != seat)
            .unwrap();
        let k = quiet(&r, |k| {
            k.seat_last = false;
            k.target_strength_weight = 1000;
        });
        let mut ctx = Ctx::new(a.sim.view(), P0, &k, a.map(P0));
        assert_eq!(choose_target(&mut ctx, 0), Some(other));
        let blind = quiet(&r, |k| {
            k.seat_last = false;
            k.target_strength_weight = 0;
        });
        let mut ctx = Ctx::new(a.sim.view(), P0, &blind, a.map(P0));
        let nearest = [seat, other]
            .into_iter()
            .min_by_key(|t| target_score(&mut ctx, *t))
            .unwrap();
        assert_eq!(choose_target(&mut ctx, 0), Some(nearest));
    }

    /// M6: `jitter` stays within `permille` either way, the noise is the
    /// same at every tick for one seed and salt, and differs by seed.
    #[test]
    fn noise_jitter_is_bounded_stable_across_plans_and_differs_by_seed() {
        for noise in [0u64, 1, 150, 300, 301, 12_345, u64::MAX] {
            let v = jitter(1000, noise, 150);
            assert!((850..=1150).contains(&v), "{noise}: {v}");
        }
        assert_eq!(jitter(1000, 7, 0), 1000);
        assert_eq!(jitter(0, 7, 150), 0);
        let r = crate::testkit::rules();
        let at = |seed: u64, ticks: u32| {
            let mut s = MatchSetup::bots(&r, seed, ("standard", "standard"), ("passive", ""));
            s.debug_commands = true;
            let mut a = Arena::frozen(s, Vec::new());
            a.run(ticks);
            let ctx = a.ctx(P0);
            (
                ctx.noise(salt::WAVE_INTERVAL),
                interval_after(&ctx, 1800, 0),
                first_wave_extra(&ctx),
            )
        };
        assert_eq!(at(3, 0), at(3, 777), "tick-free");
        let values: std::collections::BTreeSet<_> = (1..=6).map(|s| at(s, 0)).collect();
        assert!(values.len() >= 5, "seeds differ: {values:?}");
        let k = r.ai.personality("standard").unwrap();
        for (_, interval, extra) in values {
            let base = r.ticks_from_ds(1800);
            let span = base * k.noise_wave_interval_permille / 1000;
            assert!(
                (base - span..=base + span).contains(&interval),
                "{interval}"
            );
            assert!(extra <= r.ticks_from_ds(k.noise_first_wave_ds));
        }
    }

    /// M6: the wave-size noise only ever adds: across seeds no wave
    /// launches below the table's size, and every one launches with the
    /// size plus `noise_wave_units` at the rally.
    #[test]
    fn wave_size_noise_never_lowers_a_wave() {
        let r = crate::testkit::rules();
        let k = r.ai.personality("standard").unwrap();
        let bonus = u16::try_from(k.noise_wave_units).unwrap();
        assert!(bonus > 0);
        let mut launched_at_size = 0;
        for seed in 1..=6 {
            // With the Mangonel: 7 armed (below the size of 8), 8 + the
            // largest bonus (always), and exactly 8 (only with no bonus).
            for (skirm, must) in [(6u16, false), (7 + bonus, true), (7, false)] {
                let mut s = MatchSetup::bots(&r, seed, ("standard", "standard"), ("passive", ""));
                s.debug_commands = true;
                let a = Arena::frozen(s, at_rally(&r, true, skirm, 1, 1));
                let mut ctx = a.ctx(P0);
                let mut mem = BotMemory {
                    waves_launched: 1,
                    ..BotMemory::default()
                };
                step(&mut ctx, &mut mem);
                if skirm == 6 {
                    assert!(mem.wave.is_none(), "seed {seed}: below the size");
                } else if must {
                    assert!(mem.wave.is_some(), "seed {seed}: size plus the bonus");
                } else {
                    launched_at_size += usize::from(mem.wave.is_some());
                }
            }
        }
        assert!(launched_at_size < 6, "the bonus holds some waves back");
    }
}
