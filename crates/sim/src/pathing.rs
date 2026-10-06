//! Budgeted, resumable in-house A* on the tile map, the per-tick request
//! queue and the transparent path cache. Specification:
//! `docs/design/pathing.md`.
//!
//! # What is hashed
//!
//! [`Pathing::queue`] and [`Pathing::active`] (the request being searched
//! and its full [`AStarSearch`]: open heap as a sequence, closed/g/parent
//! arrays, expansion counter) are hashed sim state and serialise with the
//! snapshot, so a snapshot taken mid-search restores the exact search.
//! `std::collections::BinaryHeap` serialises in internal array order and
//! deserialises by pushing in that order, which reproduces the identical
//! array; implementer A adds a test that a mid-search snapshot/restore is
//! byte-exact. [`Pathing::cache`] is derived, `#[serde(skip)]`, never hashed,
//! and cleared on every `cost_grid_generation` change and on restore.
//!
//! # Grid, costs, heuristic
//!
//! 8-connected; an orthogonal step costs [`COST_ORTHOGONAL`] (10) and a
//! diagonal [`COST_DIAGONAL`] (14). Neighbours come from
//! [`Map::neighbors8`], which already applies the no-corner-cutting rule.
//! The heuristic is [`octile`], admissible and consistent for these costs.
//!
//! # Open set order
//!
//! [`Node`]'s `Ord` is defined so that `BinaryHeap` (a max-heap) pops the
//! smallest `f` first, then the largest `g`, then the smallest tile index:
//! a total order, so every machine pops the same node. Stale entries (a tile
//! already closed) are skipped when popped and do not count as expansions.
//!
//! # Budget and `resume`
//!
//! [`AStarSearch::resume`] expands nodes until the goal is popped
//! (`Found`), the heap empties (`Exhausted`) or `*budget` reaches zero
//! (`Suspended`). It decrements `*budget` by the expansions it performed so
//! several searches share one per-tick budget
//! (`rules.path_budget_expansions`). Expansions, not pops, are charged.
//!
//! # Request queue
//!
//! Requests are kept sorted by `(requested_tick, UnitId)`. Each tick
//! [`Pathing::service`] walks the queue in that order: it corrects the goal
//! (blocked or in another component -> `Map::nearest_passable` within the
//! start's component), completes trivially when start == goal, then consults
//! the cache and finally runs or resumes the search. A `Suspended` result
//! ends the tick's pathing work; the request stays at the head as `active`.
//!
//! # The transparent cache rule (virtual budget)
//!
//! A cache entry stores the path *and* `expansions_used`, the total number of
//! expansions the original search spent. A hit must leave the hashed state at
//! the end of every tick identical to the state the same run reaches with the
//! cache disabled, so that `path_cache_is_transparent` can assert equal
//! hashes every tick. Therefore a hit is honoured only when
//! `expansions_used <= *budget` at the moment the request is reached: the
//! request then completes this tick with the cached path and `*budget` is
//! reduced by exactly `expansions_used`, which is precisely what the real
//! search would have done. When `expansions_used > *budget` the real search
//! would have suspended with a partially built heap in hashed state; no cache
//! replay can reproduce that, so the hit is ignored and a real search runs
//! (it suspends naturally and is cached when it completes). Timing, results
//! and hashes are thereby identical with the cache on or off; the cache only
//! saves work for searches that fit in the remaining budget.
//!
//! Cache key: `(start tile index, goal tile index, cost_grid_generation)`.
//! Eviction is deterministic: entries are stored in a `BTreeMap` keyed by an
//! insertion counter and the smallest counter is evicted at capacity. Whether
//! the cache evicts or hits never changes results, only work.

use crate::ids::{Tile, UnitId};
use crate::map::Map;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

/// Cost of an orthogonal step.
pub const COST_ORTHOGONAL: u32 = 10;
/// Cost of a diagonal step.
pub const COST_DIAGONAL: u32 = 14;
/// `g` value of an unvisited tile.
pub const G_UNVISITED: u32 = u32::MAX;
/// `parent` value of a tile without a predecessor (the start, or unvisited).
pub const NO_PARENT: u32 = u32::MAX;

/// Octile heuristic: `14 * min(dx, dy) + 10 * (max(dx, dy) - min(dx, dy))`.
pub fn octile(a: Tile, b: Tile) -> u32 {
    let dx = u32::from(a.x.abs_diff(b.x));
    let dy = u32::from(a.y.abs_diff(b.y));
    let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
    COST_DIAGONAL * lo + COST_ORTHOGONAL * (hi - lo)
}

/// Step cost between two 8-adjacent tiles.
pub fn step_cost(a: Tile, b: Tile) -> u32 {
    if a.x != b.x && a.y != b.y {
        COST_DIAGONAL
    } else {
        COST_ORTHOGONAL
    }
}

/// Total octile cost of a path given as consecutive tiles.
pub fn path_cost(path: &[Tile]) -> u32 {
    path.windows(2).map(|w| step_cost(w[0], w[1])).sum()
}

/// Outcome of one [`AStarSearch::resume`] call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchStatus {
    /// The goal was popped; the path runs from the start tile to the goal
    /// tile inclusive (a single tile when start == goal).
    Found(Vec<Tile>),
    /// The open set emptied: the goal is unreachable from the start.
    Exhausted,
    /// The budget ran out; call `resume` again next tick.
    Suspended,
}

/// An open-set entry. Ordered for a `BinaryHeap` so that the smallest `f`
/// pops first, then the largest `g`, then the smallest tile index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// `g + h`.
    pub f: u32,
    /// Cost from the start.
    pub g: u32,
    /// Row-major tile index.
    pub tile: u32,
}

impl Ord for Node {
    fn cmp(&self, o: &Node) -> Ordering {
        o.f.cmp(&self.f)
            .then(self.g.cmp(&o.g))
            .then(o.tile.cmp(&self.tile))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Node) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// A resumable A* search. Hashed state while it is `Pathing::active`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AStarSearch {
    /// Start tile.
    pub from: Tile,
    /// Goal tile.
    pub to: Tile,
    /// Open set.
    pub open: BinaryHeap<Node>,
    /// Expanded tiles, one flag per map tile.
    pub closed: Vec<bool>,
    /// Best known cost per tile; [`G_UNVISITED`] when untouched.
    pub g: Vec<u32>,
    /// Predecessor tile index per tile; [`NO_PARENT`] when none.
    pub parent: Vec<u32>,
    /// Expansions performed so far across every `resume` call.
    pub expansions: u32,
}

impl PartialEq for AStarSearch {
    fn eq(&self, o: &AStarSearch) -> bool {
        self.from == o.from
            && self.to == o.to
            && self.expansions == o.expansions
            && self.closed == o.closed
            && self.g == o.g
            && self.parent == o.parent
            && self.open.len() == o.open.len()
            && self.open.iter().zip(o.open.iter()).all(|(a, b)| a == b)
    }
}

impl Eq for AStarSearch {}

impl AStarSearch {
    /// Start a search from `from` to `to` on `map`: arrays sized to the map,
    /// the start pushed with `g = 0`, `f = octile(from, to)`.
    pub fn new(map: &Map, from: Tile, to: Tile) -> AStarSearch {
        // TEMP(movement): replaced by the pathing branch at integration.
        let n = map.len();
        let mut open = BinaryHeap::new();
        open.push(Node {
            f: octile(from, to),
            g: 0,
            tile: u32::try_from(map.idx(from)).expect("fits"),
        });
        let mut g = vec![G_UNVISITED; n];
        g[map.idx(from)] = 0;
        AStarSearch {
            from,
            to,
            open,
            closed: vec![false; n],
            g,
            parent: vec![NO_PARENT; n],
            expansions: 0,
        }
    }

    /// Pop and expand nodes until the goal is popped (`Found`), the heap is
    /// empty (`Exhausted`) or `*budget` reaches zero (`Suspended`).
    /// Decrements `*budget` by one per expansion. Skipped stale pops are free.
    /// The found path runs start -> goal inclusive, reconstructed through
    /// `parent` and reversed.
    pub fn resume(&mut self, map: &Map, budget: &mut u32) -> SearchStatus {
        // TEMP(movement): replaced by the pathing branch at integration.
        // Plain budgeted A* so movement can be exercised on real paths.
        let goal = u32::try_from(map.idx(self.to)).expect("fits");
        loop {
            if self.open.is_empty() {
                return SearchStatus::Exhausted;
            }
            if *budget == 0 {
                return SearchStatus::Suspended;
            }
            let Some(node) = self.open.pop() else {
                return SearchStatus::Exhausted;
            };
            let t = usize::try_from(node.tile).expect("fits");
            if self.closed[t] || node.g > self.g[t] {
                continue;
            }
            self.closed[t] = true;
            self.expansions += 1;
            *budget -= 1;
            if node.tile == goal {
                let mut path = Vec::new();
                let mut cur = goal;
                loop {
                    path.push(map.tile_at(usize::try_from(cur).expect("fits")));
                    let p = self.parent[usize::try_from(cur).expect("fits")];
                    if p == NO_PARENT {
                        break;
                    }
                    cur = p;
                }
                path.reverse();
                return SearchStatus::Found(path);
            }
            let tile = map.tile_at(t);
            for nb in map.neighbors8(tile) {
                let j = map.idx(nb);
                if self.closed[j] {
                    continue;
                }
                let ng = node.g + step_cost(tile, nb);
                if ng < self.g[j] {
                    self.g[j] = ng;
                    self.parent[j] = node.tile;
                    self.open.push(Node {
                        f: ng + octile(nb, self.to),
                        g: ng,
                        tile: u32::try_from(j).expect("fits"),
                    });
                }
            }
        }
    }
}

/// A unit's outstanding request for a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PathRequest {
    /// Tick the request was made; the primary sort key.
    pub requested_tick: u32,
    /// Requesting unit; the tiebreak. At most one request per unit is queued.
    pub unit: UnitId,
    /// Start tile (the unit's tile when the request was made).
    pub from: Tile,
    /// Goal tile (component-corrected again by `service`).
    pub to: Tile,
}

impl PathRequest {
    /// The total-order key the queue is sorted by.
    pub fn sort_key(&self) -> (u32, UnitId) {
        (self.requested_tick, self.unit)
    }
}

/// Cache key: `(start tile index, goal tile index, cost_grid_generation)`.
pub type CacheKey = (u32, u32, u32);

/// A cached search result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Path from start to goal inclusive.
    pub path: Vec<Tile>,
    /// Expansions the original search spent; charged on every hit.
    pub expansions_used: u32,
}

/// Bounded path cache with deterministic insertion-order eviction. Derived
/// state: never hashed, never serialised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathCache {
    /// Entries keyed by insertion counter (smallest is evicted first).
    pub entries: BTreeMap<u64, CacheEntry>,
    /// Lookup from cache key to insertion counter.
    pub by_key: BTreeMap<CacheKey, u64>,
    /// Maximum number of entries (`rules.path_cache_entries`).
    pub capacity: u32,
    /// `false` disables lookups and inserts (the transparency test flips it).
    pub enabled: bool,
    /// Next insertion counter.
    pub next_insert: u64,
}

impl Default for PathCache {
    fn default() -> PathCache {
        PathCache::new(0, false)
    }
}

impl PathCache {
    /// An empty cache.
    pub fn new(capacity: u32, enabled: bool) -> PathCache {
        PathCache {
            entries: BTreeMap::new(),
            by_key: BTreeMap::new(),
            capacity,
            enabled,
            next_insert: 0,
        }
    }

    /// Look up an entry. Always `None` when disabled.
    pub fn get(&self, key: CacheKey) -> Option<&CacheEntry> {
        if !self.enabled {
            return None;
        }
        self.by_key.get(&key).and_then(|i| self.entries.get(i))
    }

    /// Insert or replace an entry, evicting the oldest insertion at capacity.
    /// A no-op when disabled or when `capacity == 0`.
    pub fn insert(&mut self, key: CacheKey, entry: CacheEntry) {
        if !self.enabled || self.capacity == 0 {
            return;
        }
        if let Some(old) = self.by_key.remove(&key) {
            self.entries.remove(&old);
        }
        while self.entries.len() >= self.capacity as usize {
            let Some((&oldest, _)) = self.entries.iter().next() else {
                break;
            };
            self.entries.remove(&oldest);
            self.by_key.retain(|_, v| *v != oldest);
        }
        let i = self.next_insert;
        self.next_insert += 1;
        self.entries.insert(i, entry);
        self.by_key.insert(key, i);
    }

    /// Drop every entry; capacity and `enabled` are kept.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.by_key.clear();
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no entries are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The pathing subsystem: request queue, the active search and the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pathing {
    /// Outstanding requests sorted by `(requested_tick, UnitId)`, excluding
    /// the active one.
    pub queue: Vec<PathRequest>,
    /// The request whose search is in progress (suspended across ticks).
    pub active: Option<(PathRequest, AStarSearch)>,
    /// Derived, unhashed path cache.
    #[serde(skip)]
    pub cache: PathCache,
}

impl Pathing {
    /// Empty subsystem with a cache of `cache_entries` capacity.
    pub fn new(cache_entries: u32, cache_enabled: bool) -> Pathing {
        Pathing {
            queue: Vec::new(),
            active: None,
            cache: PathCache::new(cache_entries, cache_enabled),
        }
    }

    /// Queue a request, replacing any earlier request or active search for
    /// the same unit, and keep the queue sorted.
    pub fn request(&mut self, req: PathRequest) {
        self.cancel(req.unit);
        let pos = self
            .queue
            .binary_search_by_key(&req.sort_key(), PathRequest::sort_key)
            .unwrap_or_else(|p| p);
        self.queue.insert(pos, req);
    }

    /// Drop the unit's queued request and its active search, if any.
    pub fn cancel(&mut self, unit: UnitId) {
        self.queue.retain(|r| r.unit != unit);
        if self.active.as_ref().is_some_and(|(r, _)| r.unit == unit) {
            self.active = None;
        }
    }

    /// `true` when nothing is queued or in progress.
    pub fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.active.is_none()
    }

    /// Requests outstanding (queued plus active).
    pub fn outstanding(&self) -> usize {
        self.queue.len() + usize::from(self.active.is_some())
    }

    /// `true` when the unit has a queued or active request.
    pub fn has_request(&self, unit: UnitId) -> bool {
        self.queue.iter().any(|r| r.unit == unit)
            || self.active.as_ref().is_some_and(|(r, _)| r.unit == unit)
    }

    /// Spend up to `budget` expansions on the queue in order (see the module
    /// docs for goal correction, the cache rule and suspension). Returns the
    /// requests completed this tick in completion order: `Some(path)` with
    /// the start -> goal tiles (empty when start == corrected goal) or
    /// `None` when the goal is unreachable (defensive; the caller clears the
    /// unit's order and emits an event). Clears the cache first when
    /// `map.cost_grid_generation()` differs from the generation the cache
    /// was filled under (compare against the stored keys' third element).
    pub fn service(
        &mut self,
        map: &Map,
        budget: u32,
        tick: u32,
    ) -> Vec<(UnitId, Option<Vec<Tile>>)> {
        // TEMP(movement): replaced by the pathing branch at integration.
        // Minimal queue driver: goal correction, no cache, suspension.
        let _ = tick;
        let mut budget = budget;
        let mut done = Vec::new();
        loop {
            if budget == 0 {
                break;
            }
            let (req, mut search) = match self.active.take() {
                Some(a) => a,
                None => {
                    if self.queue.is_empty() {
                        break;
                    }
                    let req = self.queue.remove(0);
                    let from = if map.passable(req.from) {
                        req.from
                    } else {
                        match map.nearest_passable(req.from, None) {
                            Some(t) => t,
                            None => {
                                done.push((req.unit, None));
                                continue;
                            }
                        }
                    };
                    let Some(goal) = map.nearest_passable(req.to, Some(map.component_of(from)))
                    else {
                        done.push((req.unit, None));
                        continue;
                    };
                    if from == goal {
                        done.push((req.unit, Some(Vec::new())));
                        continue;
                    }
                    (req, AStarSearch::new(map, from, goal))
                }
            };
            match search.resume(map, &mut budget) {
                SearchStatus::Found(path) => done.push((req.unit, Some(path))),
                SearchStatus::Exhausted => done.push((req.unit, None)),
                SearchStatus::Suspended => {
                    self.active = Some((req, search));
                    break;
                }
            }
        }
        done
    }

    /// Drop every cache entry (grid change, restore, or the transparency test).
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Enable or disable the cache (tests). Disabling clears it.
    pub fn set_cache_enabled(&mut self, enabled: bool) {
        self.cache.enabled = enabled;
        if !enabled {
            self.cache.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octile_matches_definition() {
        assert_eq!(octile(Tile::new(0, 0), Tile::new(0, 0)), 0);
        assert_eq!(octile(Tile::new(0, 0), Tile::new(3, 0)), 30);
        assert_eq!(octile(Tile::new(0, 0), Tile::new(3, 3)), 42);
        assert_eq!(octile(Tile::new(5, 1), Tile::new(0, 3)), 2 * 14 + 3 * 10);
        assert_eq!(step_cost(Tile::new(1, 1), Tile::new(2, 2)), COST_DIAGONAL);
        assert_eq!(step_cost(Tile::new(1, 1), Tile::new(1, 2)), COST_ORTHOGONAL);
        assert_eq!(
            path_cost(&[Tile::new(0, 0), Tile::new(1, 1), Tile::new(1, 2)]),
            24
        );
    }

    #[test]
    fn node_order_pops_min_f_then_max_g_then_min_tile() {
        let mut heap = BinaryHeap::new();
        heap.push(Node {
            f: 20,
            g: 5,
            tile: 3,
        });
        heap.push(Node {
            f: 10,
            g: 2,
            tile: 9,
        });
        heap.push(Node {
            f: 10,
            g: 7,
            tile: 8,
        });
        heap.push(Node {
            f: 10,
            g: 7,
            tile: 1,
        });
        let popped: Vec<Node> = std::iter::from_fn(|| heap.pop()).collect();
        assert_eq!(
            popped,
            vec![
                Node {
                    f: 10,
                    g: 7,
                    tile: 1
                },
                Node {
                    f: 10,
                    g: 7,
                    tile: 8
                },
                Node {
                    f: 10,
                    g: 2,
                    tile: 9
                },
                Node {
                    f: 20,
                    g: 5,
                    tile: 3
                },
            ]
        );
    }

    #[test]
    fn binary_heap_serde_round_trip_preserves_array_order() {
        let mut heap = BinaryHeap::new();
        for i in 0..64u32 {
            heap.push(Node {
                f: (i * 7919) % 23,
                g: (i * 31) % 11,
                tile: i,
            });
        }
        heap.pop();
        heap.pop();
        let before: Vec<Node> = heap.iter().copied().collect();
        let bytes = postcard::to_allocvec(&heap).unwrap();
        let back: BinaryHeap<Node> = postcard::from_bytes(&bytes).unwrap();
        let after: Vec<Node> = back.iter().copied().collect();
        assert_eq!(before, after, "heap array order survives serde");
        assert_eq!(postcard::to_allocvec(&back).unwrap(), bytes);
    }

    #[test]
    fn queue_is_sorted_and_one_request_per_unit() {
        let mut p = Pathing::new(8, true);
        let mk = |t: u32, u: u32| PathRequest {
            requested_tick: t,
            unit: UnitId(u),
            from: Tile::new(0, 0),
            to: Tile::new(1, 1),
        };
        p.request(mk(5, 2));
        p.request(mk(3, 9));
        p.request(mk(5, 1));
        p.request(mk(3, 9)); // replaces itself
        let keys: Vec<_> = p.queue.iter().map(PathRequest::sort_key).collect();
        assert_eq!(keys, vec![(3, UnitId(9)), (5, UnitId(1)), (5, UnitId(2))]);
        assert!(p.has_request(UnitId(2)));
        p.cancel(UnitId(2));
        assert!(!p.has_request(UnitId(2)));
        assert_eq!(p.outstanding(), 2);
        assert!(!p.is_idle());
    }

    #[test]
    fn cache_is_bounded_with_oldest_first_eviction_and_skipped_by_serde() {
        let mut c = PathCache::new(2, true);
        let e = |n: u32| CacheEntry {
            path: vec![],
            expansions_used: n,
        };
        c.insert((0, 1, 0), e(1));
        c.insert((0, 2, 0), e(2));
        c.insert((0, 3, 0), e(3));
        assert_eq!(c.len(), 2);
        assert!(c.get((0, 1, 0)).is_none(), "oldest evicted");
        assert_eq!(c.get((0, 3, 0)).unwrap().expansions_used, 3);
        c.insert((0, 2, 0), e(22));
        assert_eq!(c.get((0, 2, 0)).unwrap().expansions_used, 22);
        assert_eq!(c.len(), 2);
        c.clear();
        assert!(c.is_empty());
        let mut d = PathCache::new(2, false);
        d.insert((0, 1, 0), e(1));
        assert!(d.get((0, 1, 0)).is_none(), "disabled cache stores nothing");

        let mut p = Pathing::new(4, true);
        p.cache.insert((1, 2, 3), e(9));
        let bytes = postcard::to_allocvec(&p).unwrap();
        let back: Pathing = postcard::from_bytes(&bytes).unwrap();
        assert!(back.cache.is_empty(), "cache is not serialised");
        assert_eq!(back.queue, p.queue);
    }
}
