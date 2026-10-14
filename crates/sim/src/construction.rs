//! Building footprints, `Build` validation, construction and site cancel
//! (`docs/design/towns.md`, "Founding"; `docs/design/economy.md`).
//!
//! A building's tile is the north-west corner of its footprint. Placement
//! blocks the footprint in the cost grid at once (one generation bump, so
//! the path cache sees one change), deducts the ramped cost and gives the
//! builder a [`Job::Build`] and a move to a stand tile on the ring around
//! the footprint. The site gains one tick of progress per tick its builder
//! is at work: no order, and its post (where it arrived) adjacent to the
//! footprint. A builder pushed off its post by separation walks back
//! without losing the post, so jostling never pauses construction.
//!
//! `validate_build` is pure: it reads the state and returns either a plan
//! or the first failing [`RejectReason`] in a documented order (the M3b HUD
//! shows that reason), so a rejected command changes nothing and draws no
//! randomness. `SimView::check_build` calls the same function.

use crate::economy::{self, building_kind};
use crate::fx::{Fx, FxVec2};
use crate::ids::{BuildingId, BuildingKindId, PlayerId, Tile, UnitId};
use crate::map::Map;
use crate::pathing::PathRequest;
use crate::state::{
    Building, Construction, Job, MoveOrder, RejectReason, SimEvent, State, TownState, Unit,
};
use crate::territory::{Source, SourceOwner};
use crate::town;
use rules::{BuildingKind, MapDef, Rules, Slots};

/// An inclusive tile rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// West column.
    pub x0: u16,
    /// North row.
    pub y0: u16,
    /// East column (inclusive).
    pub x1: u16,
    /// South row (inclusive).
    pub y1: u16,
}

impl Rect {
    /// Every tile, row by row from the north-west corner.
    pub fn tiles(self) -> impl Iterator<Item = Tile> {
        (self.y0..=self.y1).flat_map(move |y| (self.x0..=self.x1).map(move |x| Tile::new(x, y)))
    }

    /// Whether `t` lies inside.
    pub fn contains(self, t: Tile) -> bool {
        (self.x0..=self.x1).contains(&t.x) && (self.y0..=self.y1).contains(&t.y)
    }

    /// Chebyshev distance from `t` to the rectangle (0 inside).
    pub fn chebyshev(self, t: Tile) -> u16 {
        let dx = self.x0.saturating_sub(t.x).max(t.x.saturating_sub(self.x1));
        let dy = self.y0.saturating_sub(t.y).max(t.y.saturating_sub(self.y1));
        dx.max(dy)
    }
}

/// The footprint of `kind` with its north-west corner at `nw`, or `None`
/// when it leaves a `width` x `height` map.
pub fn footprint(kind: &BuildingKind, nw: Tile, width: u16, height: u16) -> Option<Rect> {
    let w = u16::try_from(kind.width_tiles).expect("validated <= 8");
    let h = u16::try_from(kind.height_tiles).expect("validated <= 8");
    let x1 = nw.x.checked_add(w - 1)?;
    let y1 = nw.y.checked_add(h - 1)?;
    (x1 < width && y1 < height).then_some(Rect {
        x0: nw.x,
        y0: nw.y,
        x1,
        y1,
    })
}

/// The footprint's centre tile, `nw + ((w - 1) / 2, (h - 1) / 2)`: the
/// border source's centre (border sources have odd sides).
pub fn centre_tile(kind: &BuildingKind, nw: Tile) -> Tile {
    let w = u16::try_from(kind.width_tiles).expect("validated <= 8");
    let h = u16::try_from(kind.height_tiles).expect("validated <= 8");
    Tile::new(nw.x + (w - 1) / 2, nw.y + (h - 1) / 2)
}

/// The footprint's exact centre point, `nw + (w / 2, h / 2)` in tiles.
pub fn centre_fx(kind: &BuildingKind, nw: Tile) -> FxVec2 {
    let w = i32::try_from(kind.width_tiles).expect("validated <= 8");
    let h = i32::try_from(kind.height_tiles).expect("validated <= 8");
    FxVec2::new(
        Fx::from_ratio(2 * i32::from(nw.x) + w, 2),
        Fx::from_ratio(2 * i32::from(nw.y) + h, 2),
    )
}

/// The in-bounds tiles sharing an edge or a corner with `rect`, clockwise
/// from the north-west corner: north edge east, east edge south, south edge
/// west, west edge north.
pub fn ring(rect: Rect, width: u16, height: u16) -> Vec<Tile> {
    let (x0, y0) = (i32::from(rect.x0) - 1, i32::from(rect.y0) - 1);
    let (x1, y1) = (i32::from(rect.x1) + 1, i32::from(rect.y1) + 1);
    let mut out = Vec::new();
    let mut push = |x: i32, y: i32| {
        if let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y))
            && x < width
            && y < height
        {
            out.push(Tile::new(x, y));
        }
    };
    for x in x0..x1 {
        push(x, y0);
    }
    for y in y0..y1 {
        push(x1, y);
    }
    for x in (x0 + 1..=x1).rev() {
        push(x, y1);
    }
    for y in (y0 + 1..=y1).rev() {
        push(x0, y);
    }
    out
}

/// A unit is at work on a building whose footprint is `rect` when it has no
/// order and its post (the tile it arrived at) is adjacent to the footprint.
/// The post, not the live position, so separation jostle never pauses work.
pub(crate) fn at_work(u: &Unit, map: &Map, rect: Rect) -> bool {
    u.order.is_none()
        && u.post
            .is_some_and(|p| rect.chebyshev(map.tile_of(p.goal)) == 1)
}

/// The footprint of an existing building.
pub(crate) fn building_rect(rules: &Rules, map: &Map, b: &Building) -> Rect {
    footprint(
        building_kind(rules, b.kind),
        b.tile,
        map.width(),
        map.height(),
    )
    .expect("a placed footprint is in bounds")
}

/// What an accepted `Build` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BuildPlan {
    /// An own site of the same kind already stands at the same tile: the
    /// unit takes over as its builder, at no cost.
    Resume {
        /// The site.
        site: BuildingId,
    },
    /// A new site.
    New {
        /// Ramped cost, `resources.ron` order.
        cost: Vec<i64>,
        /// Worker slots, fixed now.
        slots: u8,
        /// The Town the building is attributed to (`None` for a Town).
        town: Option<BuildingId>,
    },
}

/// Validate `Build { unit, kind, tile: nw }` for `p` without changing
/// anything. Checks, first failure wins:
///
/// 1. the unit exists and is `p`'s (`NoValidUnits`);
/// 2. the kind exists (`UnknownBuildingKind`);
/// 3. the unit's kind can build (`NotABuilder`);
/// 4. an own site of this kind at `nw` is resumed ([`BuildPlan::Resume`]);
/// 5. the footprint is on the map (`SiteOutOfBounds`);
/// 6. every footprint tile is `p`'s territory (`OutsideBorders`);
/// 7. every footprint tile is passable (`SiteBlocked`);
/// 8. no unit of any player stands on it (`SiteOccupied`);
/// 9. a Town keeps `p` within the town limit, sites included
///    (`TownLimitReached`);
/// 10. a kind with `max_per_town` has a Town to belong to and room under
///     its limit there, sites included (`PerTownLimitReached`);
/// 11. an `Adjacent` gatherer has at least one resource tile beside it
///     (`NoAdjacentResource`);
/// 12. at least one ring tile is passable for the builder (`SiteBlocked`);
/// 13. `p` can pay the ramped cost (`CannotAfford`, first short resource).
pub(crate) fn validate_build(
    state: &State,
    rules: &Rules,
    def: &MapDef,
    p: PlayerId,
    unit: UnitId,
    kind_id: BuildingKindId,
    nw: Tile,
) -> Result<BuildPlan, RejectReason> {
    let map = &state.map;
    let u = state
        .units
        .get(&unit)
        .filter(|u| u.owner == p)
        .ok_or(RejectReason::NoValidUnits)?;
    let kind = rules
        .building_kind(kind_id.0)
        .ok_or(RejectReason::UnknownBuildingKind)?;
    let unit_kind = rules
        .unit_kind(u.kind.0)
        .expect("unit kind in state exists in the rules");
    if !unit_kind.builder {
        return Err(RejectReason::NotABuilder);
    }
    if let Some(site) = state.buildings.values().find(|b| {
        b.owner == p
            && b.kind == kind_id
            && b.tile == nw
            && matches!(b.construction, Construction::Site { .. })
    }) {
        return Ok(BuildPlan::Resume { site: site.id });
    }
    let rect =
        footprint(kind, nw, map.width(), map.height()).ok_or(RejectReason::SiteOutOfBounds)?;
    if !rect.tiles().all(|t| state.territory.is_buildable(p, t)) {
        return Err(RejectReason::OutsideBorders);
    }
    if !rect.tiles().all(|t| map.passable(t)) {
        return Err(RejectReason::SiteBlocked);
    }
    if state
        .units
        .values()
        .any(|o| rect.contains(map.tile_of(o.pos)))
    {
        return Err(RejectReason::SiteOccupied);
    }
    let town = if kind.town {
        let towns = town::town_count(state, rules, p);
        if towns >= rules.town_limit(town::statecraft(p)) {
            return Err(RejectReason::TownLimitReached);
        }
        None
    } else {
        town::attribute(state, rules, p, centre_fx(kind, nw))
    };
    if let Some(limit) = kind.max_per_town {
        let Some(t) = town else {
            return Err(RejectReason::PerTownLimitReached);
        };
        let here = state
            .buildings
            .values()
            .filter(|b| b.owner == p && b.kind == kind_id && b.town == Some(t))
            .count();
        if u32::try_from(here).expect("count fits u32") >= limit {
            return Err(RejectReason::PerTownLimitReached);
        }
    }
    let slots = slots_for(kind, rect, def)?;
    if !ring(rect, map.width(), map.height())
        .iter()
        .any(|t| map.passable(*t))
    {
        return Err(RejectReason::SiteBlocked);
    }
    let cost = economy::building_cost_for(state, rules, p, kind_id);
    if let Some(resource) = economy::first_short(&state.economy.player(p).stock, &cost) {
        return Err(RejectReason::CannotAfford { resource });
    }
    Ok(BuildPlan::New { cost, slots, town })
}

/// Worker slots a building of `kind` gets at `rect`: `Fixed(n)` is `n`;
/// `Adjacent` counts the ring tiles of its terrain on the map definition,
/// up to `max`, and rejects zero with `NoAdjacentResource`.
fn slots_for(kind: &BuildingKind, rect: Rect, def: &MapDef) -> Result<u8, RejectReason> {
    let n = match kind.gather.as_ref().map(|g| &g.slots) {
        None => 0,
        Some(Slots::Fixed(n)) => *n,
        Some(Slots::Adjacent { terrain, max }) => {
            let n = ring(rect, def.width, def.height)
                .iter()
                .filter(|t| def.tile(t.x, t.y) == *terrain)
                .count();
            let n = u32::try_from(n).expect("ring fits u32").min(*max);
            if n == 0 {
                return Err(RejectReason::NoAdjacentResource);
            }
            n
        }
    };
    Ok(u8::try_from(n).expect("validated <= 255"))
}

/// Validate `DebugPlaceBuilding` once the debug gate, the owner and the
/// kind have passed: the footprint must be on the map, passable and free of
/// units, and an `Adjacent` gatherer needs a resource tile. Borders, limits
/// and cost are skipped. Returns the slots and the attributed Town.
pub(crate) fn validate_debug_place(
    state: &State,
    rules: &Rules,
    def: &MapDef,
    p: PlayerId,
    kind_id: BuildingKindId,
    nw: Tile,
) -> Result<(u8, Option<BuildingId>), RejectReason> {
    let map = &state.map;
    let kind = building_kind(rules, kind_id);
    let rect =
        footprint(kind, nw, map.width(), map.height()).ok_or(RejectReason::SiteOutOfBounds)?;
    if !rect.tiles().all(|t| map.passable(t)) {
        return Err(RejectReason::SiteBlocked);
    }
    if state
        .units
        .values()
        .any(|o| rect.contains(map.tile_of(o.pos)))
    {
        return Err(RejectReason::SiteOccupied);
    }
    let slots = slots_for(kind, rect, def)?;
    let town = if kind.town {
        None
    } else {
        town::attribute(state, rules, p, centre_fx(kind, nw))
    };
    Ok((slots, town))
}

/// Insert a building of `kind` at `nw` for `p` and block its footprint. A
/// site starts at progress 0; `complete` buildings are finished at once
/// (debug placement and the Seat). Returns the new id.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place(
    state: &mut State,
    rules: &Rules,
    p: PlayerId,
    kind_id: BuildingKindId,
    nw: Tile,
    paid: Vec<i64>,
    slots: u8,
    town: Option<BuildingId>,
) -> BuildingId {
    let kind = building_kind(rules, kind_id);
    let rect =
        footprint(kind, nw, state.map.width(), state.map.height()).expect("validated footprint");
    let id = state.ids.building();
    state.buildings.insert(
        id,
        Building {
            id,
            owner: p,
            kind: kind_id,
            tile: nw,
            hp: i32::try_from(kind.hp).expect("validated hp fits i32"),
            construction: Construction::Site { progress: 0 },
            paid,
            town,
            town_state: None,
            slots,
            queue: Vec::new(),
            rally: None,
        },
    );
    let tiles: Vec<Tile> = rect.tiles().collect();
    state.map.set_blocked_many(&tiles, true);
    id
}

/// Make `unit` the builder of `site`: any previous builder of the site goes
/// idle, and the unit walks to the ring tile nearest to it (ties by tile).
pub(crate) fn assign_builder(
    state: &mut State,
    rules: &Rules,
    unit: UnitId,
    site: BuildingId,
    tick: u32,
) {
    for u in state.units.values_mut() {
        if u.job == Some(Job::Build { site }) {
            u.job = None;
        }
    }
    let rect = building_rect(rules, &state.map, &state.buildings[&site]);
    let map = &state.map;
    let pos = state.units[&unit].pos;
    let stand = ring(rect, map.width(), map.height())
        .into_iter()
        .filter(|t| map.passable(*t))
        .min_by_key(|t| (pos.dist_sq_i64(map.center_of(*t)), *t))
        .expect("validated: a passable ring tile exists");
    let goal = map.center_of(stand);
    let u = state.units.get_mut(&unit).expect("validated unit");
    u.job = Some(Job::Build { site });
    u.seek_at = None;
    order_move(state, rules, unit, goal, tick);
}

/// Give `unit` a fresh move order to `goal` with a path request, as `Move`
/// does for one unit: the post is cleared.
pub(crate) fn order_move(state: &mut State, rules: &Rules, unit: UnitId, goal: FxVec2, tick: u32) {
    let u = state.units.get_mut(&unit).expect("unit exists");
    u.order = Some(MoveOrder {
        goal,
        path: Vec::new(),
        next: 0,
        repath_at: tick + rules.repath_interval_ticks(),
    });
    u.post = None;
    let from = state.map.tile_of(u.pos);
    let to = state.map.tile_of(goal);
    state.pathing.request(PathRequest {
        requested_tick: tick,
        unit,
        from,
        to,
    });
}

/// Step 6a: every site whose builder is at work gains one tick of progress,
/// in id order; a site reaching its build time completes.
pub(crate) fn step(state: &mut State, rules: &Rules, events: &mut Vec<SimEvent>) {
    let builders: std::collections::BTreeMap<BuildingId, UnitId> = state
        .units
        .values()
        .filter_map(|u| match u.job {
            Some(Job::Build { site }) => Some((site, u.id)),
            _ => None,
        })
        .collect();
    let sites: Vec<BuildingId> = state
        .buildings
        .values()
        .filter(|b| matches!(b.construction, Construction::Site { .. }))
        .map(|b| b.id)
        .collect();
    for id in sites {
        let Some(&builder) = builders.get(&id) else {
            continue;
        };
        let b = &state.buildings[&id];
        let rect = building_rect(rules, &state.map, b);
        if !at_work(&state.units[&builder], &state.map, rect) {
            continue;
        }
        let done = rules.build_time_ticks(building_kind(rules, b.kind));
        let b = state.buildings.get_mut(&id).expect("site exists");
        let Construction::Site { progress } = &mut b.construction else {
            unreachable!("filtered to sites");
        };
        *progress += 1;
        if *progress >= done {
            complete(state, rules, id, events);
            if let Some(u) = state.units.get_mut(&builder) {
                u.job = None;
            }
        }
    }
}

/// Finish building `id`: a Town gets its town state and projects its
/// tier-1 border, a kind with a border radius projects that. Emits
/// `BuildingCompleted` (and `TerritoryChanged` when a border moved).
pub(crate) fn complete(
    state: &mut State,
    rules: &Rules,
    id: BuildingId,
    events: &mut Vec<SimEvent>,
) {
    let b = state.buildings.get_mut(&id).expect("building exists");
    b.construction = Construction::Complete;
    let kind = building_kind(rules, b.kind);
    let owner = b.owner;
    let centre = centre_tile(kind, b.tile);
    let radius = if kind.town {
        b.town_state.get_or_insert(TownState {
            seat: false,
            tier: 1,
        });
        Some(rules.town_radius_for(1, town::statecraft(owner)))
    } else {
        kind.border_radius_tiles
    };
    events.push(SimEvent::BuildingCompleted { building: id });
    if let Some(r) = radius {
        state.territory.add_source(
            id,
            Source {
                owner: SourceOwner::Player(owner),
                centre,
                radius: u16::try_from(r).expect("validated radius fits u16"),
            },
        );
        events.push(SimEvent::TerritoryChanged);
    }
}

/// Cancel a site: refund what was paid (clamped to stockpile caps), unblock
/// the footprint, remove the site and idle its builder.
pub(crate) fn cancel_site(
    state: &mut State,
    rules: &Rules,
    id: BuildingId,
    events: &mut Vec<SimEvent>,
) {
    let b = state.buildings.remove(&id).expect("site exists");
    let rect = footprint(
        building_kind(rules, b.kind),
        b.tile,
        state.map.width(),
        state.map.height(),
    )
    .expect("placed footprint");
    let tiles: Vec<Tile> = rect.tiles().collect();
    state.map.set_blocked_many(&tiles, false);
    economy::refund(rules, &mut state.economy.player_mut(b.owner).stock, &b.paid);
    for u in state.units.values_mut() {
        if u.job == Some(Job::Build { site: id }) {
            u.job = None;
        }
    }
    events.push(SimEvent::BuildingRemoved { building: id });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: u16, y0: u16, x1: u16, y1: u16) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    #[test]
    fn ring_runs_clockwise_from_the_north_west_corner_and_clips() {
        let r = ring(rect(1, 1, 2, 2), 10, 10);
        assert_eq!(
            r,
            [
                (0, 0),
                (1, 0),
                (2, 0),
                (3, 0),
                (3, 1),
                (3, 2),
                (3, 3),
                (2, 3),
                (1, 3),
                (0, 3),
                (0, 2),
                (0, 1)
            ]
            .map(|(x, y)| Tile::new(x, y))
        );
        let clipped = ring(rect(0, 0, 0, 0), 10, 10);
        assert_eq!(
            clipped,
            [(1, 0), (1, 1), (0, 1)].map(|(x, y)| Tile::new(x, y))
        );
        for t in ring(rect(4, 4, 6, 6), 10, 10) {
            assert_eq!(rect(4, 4, 6, 6).chebyshev(t), 1, "{t:?}");
        }
        assert_eq!(ring(rect(4, 4, 6, 6), 10, 10).len(), 16);
    }

    #[test]
    fn chebyshev_to_a_rect() {
        let r = rect(4, 4, 6, 5);
        assert_eq!(r.chebyshev(Tile::new(5, 5)), 0);
        assert_eq!(r.chebyshev(Tile::new(3, 3)), 1);
        assert_eq!(r.chebyshev(Tile::new(9, 5)), 3);
        assert_eq!(r.chebyshev(Tile::new(5, 0)), 4);
        assert!(r.contains(Tile::new(6, 5)) && !r.contains(Tile::new(7, 5)));
        assert_eq!(r.tiles().count(), 6);
    }
}
