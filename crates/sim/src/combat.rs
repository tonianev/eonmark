//! Combat core (`docs/design/combat.md`, "M4a implementation notes"): unit
//! stats resolved through the Modifier rows, the explicit `Attack` order,
//! simultaneous hits, death and building destruction.
//!
//! Hashed state lives on the units and buildings: `Unit::damage_taken`,
//! `ready_at`, `target` and `intent`, and `Building::hp` and `ready_at`.
//! Nothing else carries over between ticks: `Stats` and `Plan` are
//! rebuilt inside every step, so a restore has nothing to rebuild.
//!
//! Each tick, inside step 5 of `Sim::step`:
//!
//! - 5a `think`, from the start-of-tick positions, for every unit with
//!   [`Intent::Attack`] in `UnitId` order: a target that is gone, the
//!   unit's own or a building with no hit points left ends the order. A
//!   target in weapon range is held ([`Engage::Hold`]). One within the
//!   unit's reach (the longer of `acquire_range_tiles` and its range) with
//!   a clear tile line, or a building whose footprint the unit stands
//!   beside, is chased directly ([`Engage::Chase`]), no path and no
//!   request. Otherwise the unit walks a path toward it (a path chase),
//!   which ends on the tile `chase_end` gives: the target's tile when the
//!   unit can walk there; else the tile of the unit's component nearest to
//!   it, as pathing corrects the goal, when the target is in weapon range
//!   from anywhere within the unit's arrive radius of that tile's centre;
//!   for a building, the reachable ring tile nearest to the unit. A target
//!   with no such tile cannot be reached and the order ends: at once, with
//!   no path, and on any later tick of the walk (a wall can close behind
//!   the target), so every attacker of a group gives up together. A new
//!   path chase is a move order to the target's tile (to the ring tile for
//!   a building). Once a unit target has drifted more than
//!   `chase_regoal_tiles` from the goal (or a building's ring tile is cut
//!   off), the order is re-goaled like a repath: the unit keeps its path
//!   until step 4 writes the new one, and while a request is outstanding it
//!   does not arrive at the end of its path (`Plan::awaiting`). A path
//!   chase that has arrived (no order, a post) walks again when its end
//!   tile is no longer the tile it arrived at (the target moved), and
//!   otherwise gives up. There is no leash.
//! - 5b `movement::step` takes the engagements: a holding unit stands and
//!   faces its target, a chasing one steers straight at it.
//! - 5c `resolve`: every attacker whose target is still valid and in
//!   weapon range from the post-movement positions, and whose cooldown has
//!   run out, hits, in `UnitId` order; the hits are summed per target and
//!   applied at once, so a unit killed this tick still lands its own and a
//!   lower id has no first strike. Then `reap` removes the dead and the
//!   destroyed.
//!
//! Range is measured edge to edge: a unit is in range of a unit target when
//! `dist_sq_i64(a, t) <= (range + r_a + r_t)^2`, and of a building when the
//! squared distance to the nearest point of the footprint rectangle (the
//! tiles' area, `[x0, x1 + 1] x [y0, y1 + 1]`) is at most
//! `(range + r_a)^2`, both in `dist_sq_i64`'s scale.
//!
//! A hit does `floor(floor(raw * counter / 1000) * variance / 1000)`, where
//! `raw` is the attacker's damage with its owner's `UnitDamage` rows,
//! `counter` the counter table's permille for the pair (1000 when not
//! listed) and `variance` 1000, or `1000 + rng_index(2v + 1) - v` when
//! `damage_variance_permille` `v` is not 0, the only draw combat makes.
//! Overkill is accepted: attackers do not spread their hits by the damage
//! already dealt this tick.

use crate::construction::{self, Rect, builder_stand, building_rect, order_move};
use crate::fx::{Fx, FxVec2};
use crate::ids::{BuildingId, PlayerId, TargetId, Tile, UnitId, UnitKindId};
use crate::map::Map;
use crate::pathing::PathRequest;
use crate::state::{Intent, RejectReason, SimEvent, State, Unit, fx_from_x100};
use crate::tech;
use crate::town;
use rand_core::Rng;
use rules::{Defender, Rules};
use std::collections::{BTreeMap, BTreeSet};

/// A player's attack for one unit kind, with the owner's Modifier rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackStats {
    /// Damage per hit with the `UnitDamage` rows, before counters and
    /// variance; at least 0.
    pub damage: i32,
    /// Weapon range, edge to edge, in tiles.
    pub range: Fx,
    /// How far the unit chases directly (and, from the combat behaviour
    /// slice, looks for enemies): the longer of `acquire_range_tiles` and
    /// the range, in tiles, edge to edge.
    pub reach: Fx,
    /// Ticks from one hit to the next (`cooldown_ds`, converted once).
    pub cooldown_ticks: u32,
}

/// A player's combat stats for one unit kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitStats {
    /// Hit points with the `UnitHp` rows; at least 1.
    pub max_hp: i32,
    /// `None` for a kind that does not attack.
    pub attack: Option<AttackStats>,
}

/// `p`'s combat stats for unit kind `kind`, resolved now.
///
/// # Panics
///
/// If `kind` is not in the rules.
pub(crate) fn unit_stats(state: &State, rules: &Rules, p: PlayerId, kind: UnitKindId) -> UnitStats {
    let def = rules.unit_kind(kind.0).expect("validated unit kind");
    let attack = def.attack.as_ref().map(|a| AttackStats {
        damage: tech::unit_damage(state, rules, p, kind).expect("the kind attacks"),
        range: fx_from_x100(a.range_tiles_x100),
        reach: fx_from_x100(rules.acquire_reach(a)),
        cooldown_ticks: rules.cooldown_ticks(a),
    });
    UnitStats {
        max_hp: tech::unit_max_hp(state, rules, p, kind),
        attack,
    }
}

/// Every player's [`UnitStats`] per kind and the counter table, built once
/// at the start of step 5 and read by `think` and `resolve`. Research
/// completes in step 6b, so the table holds for the whole of step 5.
pub(crate) struct Stats {
    kinds: usize,
    /// `[player * kinds + kind]`.
    units: Vec<UnitStats>,
    /// Permille, `[attacker * kinds + defender]`.
    counters: Vec<i64>,
    /// Permille against any building, `[attacker]`.
    vs_buildings: Vec<i64>,
    /// `damage_variance_permille`.
    variance: i64,
}

impl Stats {
    /// Resolve every player's stats for every kind.
    pub(crate) fn new(state: &State, rules: &Rules) -> Stats {
        let kinds = rules.units.len();
        let kind_id = |k: usize| UnitKindId(u16::try_from(k).expect("validated unit count"));
        let mut units = Vec::with_capacity(state.players.len() * kinds);
        for p in &state.players {
            for k in 0..kinds {
                units.push(unit_stats(state, rules, p.id, kind_id(k)));
            }
        }
        let mut counters = Vec::with_capacity(kinds * kinds);
        let mut vs_buildings = Vec::with_capacity(kinds);
        for a in &rules.units {
            for d in &rules.units {
                let permille = rules.counter_permille(&a.id, &Defender::Kind(d.id.clone()));
                counters.push(i64::from(permille));
            }
            vs_buildings.push(i64::from(
                rules.counter_permille(&a.id, &Defender::Buildings),
            ));
        }
        Stats {
            kinds,
            units,
            counters,
            vs_buildings,
            variance: i64::from(rules.damage_variance_permille),
        }
    }

    /// Player `p`'s stats for `kind`.
    pub(crate) fn unit(&self, p: PlayerId, kind: UnitKindId) -> &UnitStats {
        &self.units[usize::from(p.0) * self.kinds + usize::from(kind.0)]
    }

    fn counter(&self, attacker: UnitKindId, defender: UnitKindId) -> i64 {
        self.counters[usize::from(attacker.0) * self.kinds + usize::from(defender.0)]
    }
}

/// How a unit engages this tick; decided by `think`, carried out by
/// `movement::step`. Never stored across ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engage {
    /// In weapon range: stand (idle push cap, no path following) and face
    /// `face`.
    Hold {
        /// The target's position, or the nearest point of its footprint.
        face: FxVec2,
    },
    /// Steer straight at this point (the target's position, or the nearest
    /// point of its footprint) at full speed, with the moving push cap.
    Chase(FxVec2),
}

/// What `think` decided for this tick.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    /// Engaged units; every other unit moves as it would without combat.
    pub engage: BTreeMap<UnitId, Engage>,
    /// Path chasers with a path request outstanding (a re-goal or a
    /// repath). They keep following their path but do not arrive at its
    /// end: arriving would end the order, step 4 would then drop the new
    /// path, and the arrived chase would give up or ask again from the back
    /// of the queue. They wait at the end until the new path lands.
    pub awaiting: BTreeSet<UnitId>,
}

/// A target as seen by an attacker.
#[derive(Debug, Clone, Copy)]
enum Aim {
    Unit {
        pos: FxVec2,
        radius: Fx,
        kind: UnitKindId,
    },
    Building {
        rect: Rect,
    },
}

impl Aim {
    /// The point an attacker at `from` steers at and faces.
    fn point(&self, from: FxVec2) -> FxVec2 {
        match *self {
            Aim::Unit { pos, .. } => pos,
            Aim::Building { rect } => nearest_point(from, rect),
        }
    }
}

/// `t` as a target for a unit of `p`: it exists, belongs to another player
/// and, for a building, has hit points left (a complete Town at 0 HP and a
/// building about to be destroyed are not targets). `None` otherwise.
fn locate(state: &State, rules: &Rules, p: PlayerId, t: TargetId) -> Option<Aim> {
    match t {
        TargetId::Unit(id) => state
            .units
            .get(&id)
            .filter(|u| u.owner != p)
            .map(|u| Aim::Unit {
                pos: u.pos,
                radius: u.radius,
                kind: u.kind,
            }),
        TargetId::Building(id) => state
            .buildings
            .get(&id)
            .filter(|b| b.owner != p && b.hp > 0)
            .map(|b| Aim::Building {
                rect: building_rect(rules, &state.map, b),
            }),
    }
}

/// `r` squared, in `dist_sq_i64`'s scale.
pub(crate) fn reach_sq(r: Fx) -> i64 {
    (r * r).to_bits()
}

/// The nearest point to `p` of the area the footprint `rect` covers,
/// `[x0, x1 + 1] x [y0, y1 + 1]` in tiles; `p` itself when inside.
pub(crate) fn nearest_point(p: FxVec2, rect: Rect) -> FxVec2 {
    let span = |lo: u16, hi: u16, v: Fx| {
        v.clamp(Fx::from_int(i32::from(lo)), Fx::from_int(i32::from(hi) + 1))
    };
    FxVec2::new(span(rect.x0, rect.x1, p.x), span(rect.y0, rect.y1, p.y))
}

/// Squared distance from `p` to the footprint `rect` (0 inside), in
/// `dist_sq_i64`'s scale.
pub(crate) fn dist_sq_to_rect(p: FxVec2, rect: Rect) -> i64 {
    p.dist_sq_i64(nearest_point(p, rect))
}

/// Whether `aim` lies within `extra` of a unit at `pos` with `radius`,
/// edge to edge.
fn within(pos: FxVec2, radius: Fx, extra: Fx, aim: &Aim) -> bool {
    match *aim {
        Aim::Unit {
            pos: t, radius: rt, ..
        } => pos.dist_sq_i64(t) <= reach_sq(extra + radius + rt),
        Aim::Building { rect } => dist_sq_to_rect(pos, rect) <= reach_sq(extra + radius),
    }
}

/// Whether the tile line from `a` to `b` is clear: every tile a straight
/// segment between the two tile centres passes through is passable (tiles
/// of `ignore`, the target's own footprint, count as passable), and where
/// the segment passes exactly through a corner, both tiles beside the
/// corner are, as for a diagonal step in `Map::neighbors8`.
pub(crate) fn line_clear(map: &Map, a: Tile, b: Tile, ignore: Option<Rect>) -> bool {
    let open = |x: i32, y: i32| {
        let t = Tile::new(
            u16::try_from(x).expect("between two map tiles"),
            u16::try_from(y).expect("between two map tiles"),
        );
        map.passable(t) || ignore.is_some_and(|r| r.contains(t))
    };
    let (dx, dy) = (
        i32::from(b.x) - i32::from(a.x),
        i32::from(b.y) - i32::from(a.y),
    );
    let (nx, ny) = (dx.abs(), dy.abs());
    let (sx, sy) = (dx.signum(), dy.signum());
    let (mut x, mut y) = (i32::from(a.x), i32::from(a.y));
    if !open(x, y) {
        return false;
    }
    let (mut done_x, mut done_y) = (0, 0);
    while done_x < nx || done_y < ny {
        let decision = (1 + 2 * done_x) * ny - (1 + 2 * done_y) * nx;
        if decision == 0 {
            if !open(x + sx, y) || !open(x, y + sy) {
                return false;
            }
            x += sx;
            y += sy;
            done_x += 1;
            done_y += 1;
        } else if decision < 0 {
            x += sx;
            done_x += 1;
        } else {
            y += sy;
            done_y += 1;
        }
        if !open(x, y) {
            return false;
        }
    }
    true
}

/// A uniform index in `0..n` from one draw of the sim's RNG:
/// `(next_u32 * n) >> 32`. Stable across `rand` versions; `n > 0`.
pub(crate) fn rng_index(rng: &mut rand_pcg::Pcg32, n: u32) -> u32 {
    u32::try_from((u64::from(rng.next_u32()) * u64::from(n)) >> 32).expect("below n")
}

/// Chebyshev distance between two tiles.
fn chebyshev(a: Tile, b: Tile) -> u16 {
    a.x.abs_diff(b.x).max(a.y.abs_diff(b.y))
}

/// Whether `id`'s unit kind attacks.
fn armed(rules: &Rules, kind: UnitKindId) -> bool {
    rules.unit_kind(kind.0).is_some_and(|k| k.attack.is_some())
}

/// Validate `Attack { units, target }` for `p` without changing anything.
/// Checks, first failure wins: the target exists, is not `p`'s and, for a
/// building, has hit points left (`InvalidTarget`); at least one listed
/// unit is `p`'s and of a kind that attacks (`NoValidUnits`). Returns those
/// units, deduplicated, in `UnitId` order.
pub(crate) fn validate_attack(
    state: &State,
    rules: &Rules,
    p: PlayerId,
    units: &[UnitId],
    target: TargetId,
) -> Result<Vec<UnitId>, RejectReason> {
    if locate(state, rules, p, target).is_none() {
        return Err(RejectReason::InvalidTarget);
    }
    let attackers: BTreeSet<UnitId> = units
        .iter()
        .copied()
        .filter(|id| {
            state
                .units
                .get(id)
                .is_some_and(|u| u.owner == p && armed(rules, u.kind))
        })
        .collect();
    if attackers.is_empty() {
        return Err(RejectReason::NoValidUnits);
    }
    Ok(attackers.into_iter().collect())
}

/// Apply a validated `Attack`: each unit, in `UnitId` order, drops its
/// order, post, job and path request and takes `target` with
/// [`Intent::Attack`]. The chase starts in step 5a of the same tick.
pub(crate) fn apply_attack(state: &mut State, attackers: &[UnitId], target: TargetId) {
    for &id in attackers {
        let u = state.units.get_mut(&id).expect("validated unit");
        u.order = None;
        u.post = None;
        u.job = None;
        u.seek_at = None;
        u.target = Some(target);
        u.intent = Intent::Attack;
        state.pathing.cancel(id);
    }
}

/// End `id`'s combat order: no target, [`Intent::Plain`], no order and no
/// path request.
fn give_up(state: &mut State, id: UnitId) {
    let u = state.units.get_mut(&id).expect("unit exists");
    u.disengage();
    u.order = None;
    state.pathing.cancel(id);
}

/// An explicit attacker engages directly: any path-chase order is dropped
/// with its request first (step 4 writes completed paths into whatever
/// order the unit has), and the post a path chase left is spent.
fn engage_directly(state: &mut State, id: UnitId) {
    let u = state.units.get_mut(&id).expect("unit exists");
    u.post = None;
    if u.order.take().is_some() {
        state.pathing.cancel(id);
    }
}

/// Whether a unit standing on `a` can walk to `b`: `a` is passable and
/// `b` is in its component (blocked tiles are in none).
fn walkable(map: &Map, a: Tile, b: Tile) -> bool {
    map.passable(a) && map.component_of(a) == map.component_of(b)
}

/// Whether `u`, with weapon `range`, has the unit target at `pos` with
/// radius `rt` in weapon range from anywhere within `u`'s arrive radius of
/// the centre of `stand`: so a unit that walks to `stand` and arrives
/// there can hit.
fn hittable_from(
    rules: &Rules,
    map: &Map,
    u: &Unit,
    range: Fx,
    pos: FxVec2,
    rt: Fx,
    stand: Tile,
) -> bool {
    let kind = rules.unit_kind(u.kind.0).expect("validated unit kind");
    let edge = range + u.radius + rt - fx_from_x100(kind.arrive_radius_tiles_x100);
    edge > Fx::ZERO && map.center_of(stand).dist_sq_i64(pos) <= reach_sq(edge)
}

/// Where a path chase by `u` (weapon `range`) on `aim` ends, or `None` when
/// the target cannot be reached:
///
/// - a building: the reachable ring tile nearest to the unit, as a
///   builder's (`builder_stand`);
/// - a unit on a tile the attacker can walk to: that tile;
/// - a unit anywhere else (walled off, another component): the tile of the
///   attacker's component nearest to it, the tile pathing corrects the goal
///   to (`Map::nearest_passable`), provided the target is in weapon range
///   from anywhere within the attacker's arrive radius of that tile's
///   centre (`hittable_from`). A melee attacker never hits a target
///   walled off by a whole tile; a Bowman shoots over the wall.
///
/// It depends only on the map, the two positions and the stats, so every
/// attacker of a group decides alike, and an unreachable target costs no
/// path.
fn chase_end(map: &Map, rules: &Rules, u: &Unit, range: Fx, aim: &Aim) -> Option<Tile> {
    match *aim {
        Aim::Building { rect } => builder_stand(map, rect, u.pos).ok(),
        Aim::Unit {
            pos, radius: rt, ..
        } => {
            let (here, there) = (map.tile_of(u.pos), map.tile_of(pos));
            if !map.passable(here) || walkable(map, here, there) {
                // Units never stand on a blocked tile; should one, pathing
                // corrects its start, so nothing is judged from there.
                return Some(there);
            }
            let stand = map.nearest_passable(there, Some(map.component_of(here)))?;
            hittable_from(rules, map, u, range, pos, rt, stand).then_some(stand)
        }
    }
}

/// Re-goal `id`'s path chase to `goal` like a repath: its path and next
/// waypoint are kept until step 4 writes the new path, so a saturated
/// budget delays the chaser instead of parking it, and the request joins
/// the queue with `requested_tick = tick`. The periodic timer restarts.
fn regoal(state: &mut State, rules: &Rules, id: UnitId, goal: FxVec2, tick: u32) {
    let u = state.units.get_mut(&id).expect("unit exists");
    let from = state.map.tile_of(u.pos);
    let order = u.order.as_mut().expect("a path chase has an order");
    order.goal = goal;
    order.repath_at = tick + rules.repath_interval_ticks();
    state.pathing.request(PathRequest {
        requested_tick: tick,
        unit: id,
        from,
        to: state.map.tile_of(goal),
    });
}

/// Step 5a: decide how every unit with an explicit `Attack` engages this
/// tick (see the module docs), from the start-of-tick positions. Issues and
/// cancels path-chase requests; returns the direct engagements and the
/// path chasers waiting for a path for `movement::step`. Acquisition by
/// idle and attack-moving units arrives in the M4a combat behaviour slice.
pub(crate) fn think(state: &mut State, rules: &Rules, stats: &Stats, tick: u32) -> Plan {
    let mut plan = Plan::default();
    let attackers: Vec<UnitId> = state
        .units
        .values()
        .filter(|u| u.intent == Intent::Attack)
        .map(|u| u.id)
        .collect();
    for id in attackers {
        let u = &state.units[&id];
        let attack = stats.unit(u.owner, u.kind).attack;
        let aim = u.target.and_then(|t| locate(state, rules, u.owner, t));
        let (Some(attack), Some(aim)) = (attack, aim) else {
            give_up(state, id);
            continue;
        };
        let point = aim.point(u.pos);
        if within(u.pos, u.radius, attack.range, &aim) {
            engage_directly(state, id);
            plan.engage.insert(id, Engage::Hold { face: point });
            continue;
        }
        let map = &state.map;
        let here = map.tile_of(u.pos);
        // On a tile beside a footprint the nearest point of the footprint
        // lies on that tile's own edge or corner, so the straight steer
        // never leaves the tile, whatever blocks the tiles around it. The
        // tile line cannot see that: on the east and south faces the point
        // floors to a tile outside the footprint, and a corner crossing
        // needs both tiles beside the corner. A building's path chase ends
        // on such a tile, out of melee range when it is a corner one.
        let clear = match aim {
            Aim::Building { rect } => {
                rect.chebyshev(here) <= 1 || line_clear(map, here, map.tile_of(point), Some(rect))
            }
            Aim::Unit { .. } => line_clear(map, here, map.tile_of(point), None),
        };
        if within(u.pos, u.radius, attack.reach, &aim) && clear {
            engage_directly(state, id);
            plan.engage.insert(id, Engage::Chase(point));
            continue;
        }
        // A path chase. Its order's goal is the target's tile (pathing
        // corrects it to the end tile), or a building's ring tile.
        let goal = |end: Tile| match aim {
            Aim::Unit { pos, .. } => map.center_of(map.tile_of(pos)),
            Aim::Building { .. } => map.center_of(end),
        };
        match (&u.order, u.post) {
            // Walking. Whether the target can still be reached is checked
            // every tick, so a wall closing round it ends the chase at once
            // and no group is left jostling for a corrected goal tile it
            // cannot all reach. The cheap checks come first: a unit target
            // the unit can walk to, or one it can hit from the end of its
            // path, or a building whose ring tile it can still walk to;
            // `chase_end` searches only when they fail.
            (Some(order), _) => {
                let goal_tile = map.tile_of(order.goal);
                let to = match aim {
                    Aim::Unit {
                        pos, radius: rt, ..
                    } => {
                        let there = map.tile_of(pos);
                        let reachable = walkable(map, here, there)
                            || order.path.last().is_some_and(|&end| {
                                walkable(map, here, end)
                                    && hittable_from(rules, map, u, attack.range, pos, rt, end)
                            })
                            || chase_end(map, rules, u, attack.range, &aim).is_some();
                        if !reachable {
                            give_up(state, id);
                            continue;
                        }
                        let drift = chebyshev(goal_tile, there);
                        (u32::from(drift) > rules.chase_regoal_tiles).then(|| map.center_of(there))
                    }
                    Aim::Building { .. } if walkable(map, here, goal_tile) => None,
                    Aim::Building { .. } => match chase_end(map, rules, u, attack.range, &aim) {
                        Some(end) => Some(goal(end)),
                        None => {
                            give_up(state, id);
                            continue;
                        }
                    },
                };
                if let Some(to) = to
                    && !state.pathing.has_request(id)
                {
                    regoal(state, rules, id, to, tick);
                }
                if state.pathing.has_request(id) {
                    plan.awaiting.insert(id);
                }
            }
            (None, None) => match chase_end(map, rules, u, attack.range, &aim) {
                Some(end) => {
                    let to = goal(end);
                    order_move(state, rules, id, to, tick);
                }
                None => give_up(state, id),
            },
            // The path chase arrived and the target is still out of reach.
            // When the end tile is no longer the tile it arrived at (a unit
            // target walked on, by up to `chase_regoal_tiles`, round an
            // obstacle), walk again: each walk needs the target to have
            // moved, so this is no request storm. Otherwise it cannot be
            // reached from here: give up.
            (None, Some(post)) => match chase_end(map, rules, u, attack.range, &aim) {
                Some(end) if end != map.tile_of(post.goal) => {
                    let to = goal(end);
                    order_move(state, rules, id, to, tick);
                }
                _ => give_up(state, id),
            },
        }
    }
    plan
}

/// Step 5c: collect every hit from the post-movement positions, apply them
/// summed per target, then `reap` (see the module docs).
pub(crate) fn resolve(
    state: &mut State,
    rules: &Rules,
    stats: &Stats,
    tick: u32,
    events: &mut Vec<SimEvent>,
) {
    let mut hits: Vec<(UnitId, TargetId, Aim)> = Vec::new();
    for u in state.units.values() {
        let Some(t) = u.target else { continue };
        if tick < u.ready_at {
            continue;
        }
        let Some(attack) = stats.unit(u.owner, u.kind).attack else {
            continue;
        };
        let Some(aim) = locate(state, rules, u.owner, t) else {
            continue;
        };
        if within(u.pos, u.radius, attack.range, &aim) {
            hits.push((u.id, t, aim));
        }
    }
    if hits.is_empty() {
        return reap(state, rules, stats, events);
    }
    let mut dealt: BTreeMap<TargetId, i64> = BTreeMap::new();
    for (id, target, aim) in hits {
        let u: &Unit = &state.units[&id];
        let (owner, kind) = (u.owner, u.kind);
        let attack = stats.unit(owner, kind).attack.expect("collected armed");
        let counter = match aim {
            Aim::Unit { kind: defender, .. } => stats.counter(kind, defender),
            Aim::Building { .. } => stats.vs_buildings[usize::from(kind.0)],
        };
        let variance = if stats.variance == 0 {
            1000
        } else {
            let span = u32::try_from(2 * stats.variance + 1).expect("variance <= 500");
            1000 + i64::from(rng_index(&mut state.rng, span)) - stats.variance
        };
        let damage = i64::from(attack.damage)
            .checked_mul(counter)
            .expect("damage times counter fits i64")
            / 1000
            * variance
            / 1000;
        let u = state.units.get_mut(&id).expect("collected");
        u.ready_at = tick
            .checked_add(attack.cooldown_ticks)
            .expect("tick fits u32");
        events.push(SimEvent::Hit {
            attacker: TargetId::Unit(id),
            target,
            damage: i32::try_from(damage).expect("data-check bounds a hit"),
        });
        let sum = dealt.entry(target).or_insert(0);
        *sum = sum.checked_add(damage).expect("damage sum fits i64");
    }
    let clamp_i32 =
        |v: i64| i32::try_from(v.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).expect("clamped");
    for (target, damage) in dealt {
        match target {
            TargetId::Unit(id) => {
                let u = state.units.get_mut(&id).expect("validated target");
                u.damage_taken = clamp_i32(i64::from(u.damage_taken) + damage);
            }
            TargetId::Building(id) => {
                let b = state.buildings.get_mut(&id).expect("validated target");
                let hp = i64::from(b.hp) - damage;
                if b.town_state.is_some() {
                    // A complete Town is never destroyed: it stays at 0.
                    let before = b.hp;
                    b.hp = clamp_i32(hp.max(0));
                    if before > 0 && b.hp == 0 {
                        events.push(SimEvent::TownAtZeroHp { town: id });
                    }
                } else {
                    b.hp = clamp_i32(hp);
                }
            }
        }
    }
    reap(state, rules, stats, events);
}

/// Remove every unit whose damage has reached its hit points, in `UnitId`
/// order (its path request is cancelled; its job, slot and pop go with
/// it), then destroy every building or site at 0 HP or below other than a
/// complete Town, in `BuildingId` order: no refund, its queue is lost, its
/// border source is removed, and every destroyed footprint is unblocked by
/// one `set_blocked_many` (one cost-grid generation bump). Each Town a
/// destroyed building was attributed to is then re-derived, in id order,
/// and may fall back to tier 1. Emits `UnitDied`, `BuildingDestroyed` and
/// one `TerritoryChanged` when a border source went.
pub(crate) fn reap(state: &mut State, rules: &Rules, stats: &Stats, events: &mut Vec<SimEvent>) {
    let dead: Vec<UnitId> = state
        .units
        .values()
        .filter(|u| u.damage_taken > 0 && u.damage_taken >= stats.unit(u.owner, u.kind).max_hp)
        .map(|u| u.id)
        .collect();
    for id in dead {
        state.units.remove(&id);
        state.pathing.cancel(id);
        events.push(SimEvent::UnitDied { unit: id });
    }
    let wrecked: Vec<BuildingId> = state
        .buildings
        .values()
        .filter(|b| b.hp <= 0 && b.town_state.is_none())
        .map(|b| b.id)
        .collect();
    if wrecked.is_empty() {
        return;
    }
    let mut tiles = Vec::new();
    let mut source_removed = false;
    let mut towns = BTreeSet::new();
    for id in wrecked {
        let removed = construction::remove_building(state, rules, id);
        tiles.extend(removed.tiles);
        source_removed |= removed.source;
        towns.extend(removed.building.town);
        events.push(SimEvent::BuildingDestroyed { building: id });
    }
    state.map.set_blocked_many(&tiles, false);
    if source_removed {
        events.push(SimEvent::TerritoryChanged);
    }
    for town in towns {
        town::refresh(state, rules, town, events);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: u16, y0: u16, x1: u16, y1: u16) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    #[test]
    fn distance_to_a_footprint_is_to_its_nearest_point() {
        let r = rect(4, 4, 6, 5);
        // Inside the area (tiles 4..=6 cover x in [4, 7]): distance 0.
        assert_eq!(dist_sq_to_rect(FxVec2::from_ints(5, 5), r), 0);
        assert_eq!(dist_sq_to_rect(FxVec2::from_ints(7, 6), r), 0);
        // West of it by 1.5 tiles on a row it spans.
        let p = FxVec2::new(Fx::from_ratio(5, 2), Fx::from_int(5));
        assert_eq!(dist_sq_to_rect(p, r), reach_sq(Fx::from_ratio(3, 2)));
        assert_eq!(nearest_point(p, r), FxVec2::from_ints(4, 5));
        // Off the south-east corner (7, 6) by (3, 4): distance 5.
        assert_eq!(
            dist_sq_to_rect(FxVec2::from_ints(10, 10), r),
            reach_sq(Fx::from_int(5))
        );
    }

    #[test]
    fn tile_lines_respect_blocked_tiles_and_corners() {
        let mut map = Map::open(10, 10);
        assert!(line_clear(&map, Tile::new(0, 0), Tile::new(9, 3), None));
        map.set_blocked(Tile::new(4, 2), true);
        // Row 2 from x 0 to 9 crosses (4, 2).
        assert!(!line_clear(&map, Tile::new(0, 2), Tile::new(9, 2), None));
        // A blocked target footprint does not block its own line.
        let footprint = rect(4, 2, 4, 2);
        assert!(line_clear(
            &map,
            Tile::new(0, 2),
            Tile::new(4, 2),
            Some(footprint)
        ));
        assert!(!line_clear(
            &map,
            Tile::new(0, 2),
            Tile::new(9, 2),
            Some(rect(5, 2, 5, 2))
        ));
        // An exact diagonal through a corner needs both tiles beside it.
        let mut d = Map::open(4, 4);
        assert!(line_clear(&d, Tile::new(0, 0), Tile::new(3, 3), None));
        d.set_blocked(Tile::new(1, 0), true);
        assert!(!line_clear(&d, Tile::new(0, 0), Tile::new(3, 3), None));
        assert!(!line_clear(&d, Tile::new(3, 3), Tile::new(0, 0), None));
        // A steep line passes tiles a diagonal-only walk would skip.
        let mut s = Map::open(4, 8);
        s.set_blocked(Tile::new(1, 3), true);
        assert!(!line_clear(&s, Tile::new(0, 0), Tile::new(2, 7), None));
        assert!(line_clear(&s, Tile::new(0, 0), Tile::new(0, 7), None));
        assert!(line_clear(&s, Tile::new(2, 2), Tile::new(2, 2), None));
    }

    #[test]
    fn rng_index_spans_its_range() {
        use rand_core::SeedableRng;
        let mut rng = rand_pcg::Pcg32::seed_from_u64(7);
        let mut seen = [0u32; 5];
        for _ in 0..1000 {
            seen[usize::try_from(rng_index(&mut rng, 5)).unwrap()] += 1;
        }
        assert!(seen.iter().all(|&n| n > 150), "{seen:?}");
        assert_eq!(rng_index(&mut rng, 1), 0);
    }
}
