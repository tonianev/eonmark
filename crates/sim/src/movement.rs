//! Movement: arrival steering, spatial-grid separation, circle correction and
//! group-order target assignment. Specification: `docs/design/pathing.md`,
//! sections "Movement step", "Group orders: spiral offsets" and "BFS
//! nearest-passable fallback".
//!
//! Everything here is a pure function of the hashed state passed in. The
//! [`SpatialGrid`] is derived: rebuilt every tick by `Sim::step` and by
//! `Sim::restore`, never hashed.
//!
//! Per tick, for every unit with an order, in `UnitId` order:
//!
//! 1. Arrival steering toward `order.path[order.next]` at `unit.speed`
//!    (tiles per tick), scaled down linearly inside the kind's arrive radius
//!    around the final waypoint. A waypoint is consumed when the unit is
//!    within half a tile of it; the final one when within the arrive radius,
//!    which clears the order and emits [`SimEvent::UnitArrived`].
//! 2. Separation from neighbours read from the grid: for each neighbour
//!    closer than `radius_a + radius_b + separation`, accumulate a push-away
//!    vector. Compare with `dist_sq_i64`, never a square root.
//! 3. Integrate: `pos += clamp(desired + separation, speed)`.
//! 4. After every unit has moved, circle-vs-circle correction over
//!    overlapping pairs in `(UnitId, UnitId)` order, half the penetration
//!    each; a correction that would enter a blocked tile is dropped.
//!
//! Units never enter a blocked tile; one that would stops at the boundary
//! and the blocked-waypoint repath fires. Repath triggers (both re-issue a
//! `PathRequest` through `Pathing::request`): the next waypoint's tile is
//! blocked, or `tick >= order.repath_at` (every 60 ticks while moving).

use crate::fx::{Fx, FxVec2};
use crate::ids::UnitId;
use crate::map::Map;
use crate::state::{SimEvent, Unit};
use rules::Rules;
use std::collections::BTreeMap;

/// Side of one spatial-grid cell in tiles.
pub const CELL_TILES: u16 = 2;

/// Uniform grid of unit ids by position, 2x2 tiles per cell. Derived state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpatialGrid {
    cells_x: u16,
    cells_y: u16,
    /// `cells_x * cells_y` buckets, each in ascending `UnitId` order because
    /// `rebuild` walks the units map in id order.
    cells: Vec<Vec<UnitId>>,
}

impl SpatialGrid {
    /// An empty grid sized for `map`.
    pub fn new(map: &Map) -> SpatialGrid {
        let cells_x = map.width().div_ceil(CELL_TILES);
        let cells_y = map.height().div_ceil(CELL_TILES);
        SpatialGrid {
            cells_x,
            cells_y,
            cells: vec![Vec::new(); usize::from(cells_x) * usize::from(cells_y)],
        }
    }

    /// Grid dimensions in cells.
    pub fn dims(&self) -> (u16, u16) {
        (self.cells_x, self.cells_y)
    }

    /// Cell coordinates of a position, clamped into the grid.
    fn cell_of(&self, p: FxVec2) -> (u16, u16) {
        let clamp = |v: i32, max: u16| -> u16 {
            let hi = i32::from(max) - 1;
            u16::try_from(v.clamp(0, hi)).expect("clamped")
        };
        let cx = p.x.floor_to_int().div_euclid(i32::from(CELL_TILES));
        let cy = p.y.floor_to_int().div_euclid(i32::from(CELL_TILES));
        (clamp(cx, self.cells_x), clamp(cy, self.cells_y))
    }

    fn cell_index(&self, cx: u16, cy: u16) -> usize {
        usize::from(cy) * usize::from(self.cells_x) + usize::from(cx)
    }

    /// Clear and refill from the units map. Buckets end up in id order.
    pub fn rebuild(&mut self, units: &BTreeMap<UnitId, Unit>) {
        for c in &mut self.cells {
            c.clear();
        }
        for u in units.values() {
            let (cx, cy) = self.cell_of(u.pos);
            let i = self.cell_index(cx, cy);
            self.cells[i].push(u.id);
        }
    }

    /// Ids of every unit whose cell intersects the square
    /// `[pos - radius, pos + radius]`, in ascending `UnitId` order. The
    /// caller filters by exact distance; `pos`'s own unit is included.
    pub fn neighbors(&self, pos: FxVec2, radius: Fx) -> impl Iterator<Item = UnitId> + '_ {
        let (x0, y0) = self.cell_of(FxVec2::new(pos.x - radius, pos.y - radius));
        let (x1, y1) = self.cell_of(FxVec2::new(pos.x + radius, pos.y + radius));
        let mut out: Vec<UnitId> = Vec::new();
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                out.extend_from_slice(&self.cells[self.cell_index(cx, cy)]);
            }
        }
        out.sort_unstable();
        out.into_iter()
    }

    /// Total units indexed.
    pub fn len(&self) -> usize {
        self.cells.iter().map(Vec::len).sum()
    }

    /// `true` when no units are indexed.
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(Vec::is_empty)
    }
}

/// Advance every unit with an order by one tick (see the module docs).
/// `grid` was rebuilt from `units` at the start of this tick. Emits
/// [`SimEvent::UnitArrived`] when an order completes. Repath requests are
/// not issued here; the function returns the ids of units that need one
/// (blocked next waypoint, or `tick >= repath_at`), in id order, and
/// `Sim::step` pushes the `PathRequest`s.
pub fn step(
    units: &mut BTreeMap<UnitId, Unit>,
    map: &Map,
    grid: &SpatialGrid,
    rules: &Rules,
    tick: u32,
    events: &mut Vec<SimEvent>,
) -> Vec<UnitId> {
    let _ = (units, map, grid, rules, tick, events);
    todo!("M1 movement: step (implementer B)")
}

/// Targets for `n` units ordered to `click`, one per unit, in the order the
/// caller assigns them (callers sort the units by `UnitId`). The click tile
/// is corrected first with `Map::nearest_passable(click_tile, None)`; then a
/// square spiral from the corrected tile yields tile centres, skipping
/// blocked tiles and tiles outside the corrected tile's component. If the
/// spiral is exhausted the remaining entries repeat the last assigned
/// target; if the map has no passable tile at all the result repeats
/// `click`. Length is always `n`.
pub fn group_targets(map: &Map, click: FxVec2, n: usize) -> Vec<FxVec2> {
    let _ = (map, click, n);
    todo!("M1 movement: group_targets (implementer B)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{PlayerId, UnitKindId};

    fn unit(id: u32, x: i32, y: i32) -> Unit {
        Unit {
            id: UnitId(id),
            owner: PlayerId(0),
            kind: UnitKindId(0),
            pos: FxVec2::from_ints(x, y),
            facing: FxVec2::from_ints(1, 0),
            radius: Fx::from_ratio(35, 100),
            speed: Fx::from_ratio(9, 100),
            order: None,
        }
    }

    #[test]
    fn grid_buckets_by_2x2_cells_and_returns_id_order() {
        let map = Map::open(8, 8);
        let mut units = BTreeMap::new();
        for (id, x, y) in [(5, 0, 0), (2, 1, 1), (9, 3, 3), (1, 7, 7), (4, 2, 0)] {
            units.insert(UnitId(id), unit(id, x, y));
        }
        let mut grid = SpatialGrid::new(&map);
        assert_eq!(grid.dims(), (4, 4));
        grid.rebuild(&units);
        assert_eq!(grid.len(), 5);
        // Cell (0,0) holds ids 2 and 5; radius 0 at (0,0) sees only that cell.
        let n: Vec<UnitId> = grid.neighbors(FxVec2::from_ints(0, 0), Fx::ZERO).collect();
        assert_eq!(n, vec![UnitId(2), UnitId(5)]);
        // Radius 1 around (1.5, 1.5) spans cells (0..1, 0..1): ids 2, 4, 5, 9.
        let n: Vec<UnitId> = grid
            .neighbors(
                FxVec2::new(Fx::from_ratio(3, 2), Fx::from_ratio(3, 2)),
                Fx::ONE,
            )
            .collect();
        assert_eq!(n, vec![UnitId(2), UnitId(4), UnitId(5), UnitId(9)]);
        // Out-of-map positions clamp into the grid.
        let n: Vec<UnitId> = grid
            .neighbors(FxVec2::from_ints(50, 50), Fx::ZERO)
            .collect();
        assert_eq!(n, vec![UnitId(1)]);
        grid.rebuild(&BTreeMap::new());
        assert!(grid.is_empty());
    }
}
