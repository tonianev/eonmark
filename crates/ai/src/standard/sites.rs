//! [`MapAnalysis`]: the Standard bot's static site tables, built once per
//! seat from immutable inputs (the map's terrain and starts, the rules and
//! the personality), never from the live view. A controller rebuilt
//! mid-match therefore holds exactly the same tables, which keeps the bot
//! restore-exact. The per-plan site choice that filters these candidates
//! against the live view (ownership, buildability, the free ring, units on
//! the footprint, `check_build`) is the build-order rule's (M5b slice 2).

use rules::{MapDef, Personality, Rules, Slots, Terrain};
use sim::{BuildingKindId, MatchSetup, PlayerId, Tile};
use std::collections::BTreeMap;

/// Expansion candidates kept, best first.
pub const EXPANSION_CANDIDATES: usize = 64;

/// The static site tables of one seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapAnalysis {
    /// The seat.
    pub me: PlayerId,
    /// The seat's map start (its Seat's centre).
    pub own_start: Tile,
    /// The nearest other player's map start (the enemy Seat's centre,
    /// known from the map, not scouted; docs/design/ai.md).
    pub enemy_start: Tile,
    /// `NearTown` offsets from a Town's centre to a footprint's north-west
    /// tile: Chebyshev rings `near_town_min_tiles..=near_town_max_tiles`,
    /// inner ring first, each ring clockwise from its north-west corner.
    pub near_town_offsets: Vec<(i32, i32)>,
    /// `Terrain` candidates per building kind with `Adjacent` slots:
    /// footprint north-west tiles on grass within `border_scan_tiles` of
    /// the own start with at least one adjacent tile of the kind's terrain,
    /// best first: most adjacent tiles (up to the kind's `max`), then
    /// nearest the own start, then the lowest tile index.
    pub terrain: BTreeMap<BuildingKindId, Vec<Tile>>,
    /// `Expansion` candidates: Town footprint north-west tiles on grass
    /// whose centre lies within `town_radius_grown_tiles + 3` of the own
    /// start, best first by score (resource tiles near the centre outside
    /// the Seat's first border, less a penalty for every tile gained
    /// toward the enemy), then the lowest tile index; at most
    /// [`EXPANSION_CANDIDATES`].
    pub expansion: Vec<Tile>,
    /// `BorderEdge` candidates: grass tiles within `border_scan_tiles` of
    /// the own start, nearest the enemy start first, then the lowest tile
    /// index.
    pub border_edge: Vec<Tile>,
    /// The rally seed: `rally_tiles_from_seat` from the own start toward
    /// the enemy start (the rally is the first passable owned tile of the
    /// spiral around it, chosen at plan time).
    pub rally_seed: Tile,
}

fn d2(a: Tile, b: Tile) -> i64 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    dx * dx + dy * dy
}

fn index(def: &MapDef, t: Tile) -> u32 {
    u32::from(t.y) * u32::from(def.width) + u32::from(t.x)
}

/// The tile `(x, y)` when it is on the map.
fn tile_at(def: &MapDef, x: i32, y: i32) -> Option<Tile> {
    let x = u16::try_from(x).ok()?;
    let y = u16::try_from(y).ok()?;
    (x < def.width && y < def.height).then_some(Tile::new(x, y))
}

/// Whether every tile of the `w` x `h` footprint at `nw` is on the map and
/// grass.
fn footprint_on_grass(def: &MapDef, nw: Tile, w: u16, h: u16) -> bool {
    (0..h).all(|dy| {
        (0..w).all(|dx| {
            tile_at(
                def,
                i32::from(nw.x) + i32::from(dx),
                i32::from(nw.y) + i32::from(dy),
            )
            .is_some_and(|t| def.tile(t.x, t.y) == Terrain::Grass)
        })
    })
}

/// Tiles of `terrain` sharing an edge or a corner with the footprint.
fn adjacent_count(def: &MapDef, nw: Tile, w: u16, h: u16, terrain: Terrain) -> u32 {
    let (x0, y0) = (i32::from(nw.x) - 1, i32::from(nw.y) - 1);
    let (x1, y1) = (
        i32::from(nw.x) + i32::from(w),
        i32::from(nw.y) + i32::from(h),
    );
    let mut n = 0;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let ring = x == x0 || x == x1 || y == y0 || y == y1;
            if ring && tile_at(def, x, y).is_some_and(|t| def.tile(t.x, t.y) == terrain) {
                n += 1;
            }
        }
    }
    n
}

impl MapAnalysis {
    /// Build the tables for seat `me` of `setup` under `rules` and the
    /// personality `knobs`.
    ///
    /// # Panics
    ///
    /// If the setup's map is not in the rules or has no start for `me`
    /// (`Sim::new` and `Rules::load` rule both out).
    pub fn new(
        rules: &Rules,
        setup: &MatchSetup,
        me: PlayerId,
        knobs: &Personality,
    ) -> MapAnalysis {
        let def = rules
            .map(&setup.map)
            .expect("the setup's map is in the rules");
        let start = |p: usize| {
            let s = def.starts[p];
            Tile::new(s.x, s.y)
        };
        let own_start = start(usize::from(me.0));
        let enemy_start = setup
            .players
            .iter()
            .filter(|p| p.id != me)
            .map(|p| start(usize::from(p.id.0)))
            .min_by_key(|t| (d2(*t, own_start), index(def, *t)))
            .unwrap_or(own_start);

        let mut near_town_offsets = Vec::new();
        let min = i32::try_from(knobs.near_town_min_tiles).expect("validated <= 64");
        let max = i32::try_from(knobs.near_town_max_tiles).expect("validated <= 64");
        for r in min..=max {
            for x in -r..r {
                near_town_offsets.push((x, -r));
            }
            for y in -r..r {
                near_town_offsets.push((r, y));
            }
            for x in (-r + 1..=r).rev() {
                near_town_offsets.push((x, r));
            }
            for y in (-r + 1..=r).rev() {
                near_town_offsets.push((-r, y));
            }
        }

        let scan = i64::from(knobs.border_scan_tiles);
        let tiles: Vec<Tile> = (0..def.height)
            .flat_map(|y| (0..def.width).map(move |x| Tile::new(x, y)))
            .collect();

        let mut terrain = BTreeMap::new();
        for (i, kind) in rules.buildings.iter().enumerate() {
            let Some(Slots::Adjacent { terrain: t, max }) = kind.gather.as_ref().map(|g| &g.slots)
            else {
                continue;
            };
            let w = u16::try_from(kind.width_tiles).expect("validated <= 8");
            let h = u16::try_from(kind.height_tiles).expect("validated <= 8");
            let mut found: Vec<(u32, i64, u32, Tile)> = tiles
                .iter()
                .filter(|nw| d2(**nw, own_start) <= scan * scan)
                .filter(|nw| footprint_on_grass(def, **nw, w, h))
                .filter_map(|nw| {
                    let adj = adjacent_count(def, *nw, w, h, *t).min(*max);
                    (adj > 0).then(|| (adj, d2(*nw, own_start), index(def, *nw), *nw))
                })
                .collect();
            found.sort_by_key(|(adj, d, i, _)| (core::cmp::Reverse(*adj), *d, *i));
            terrain.insert(
                BuildingKindId(u16::try_from(i).expect("validated len")),
                found.into_iter().map(|c| c.3).collect(),
            );
        }

        let town = rules
            .building_kind(rules.town_kind())
            .expect("rules have a town kind");
        let (tw, th) = (
            u16::try_from(town.width_tiles).expect("validated <= 8"),
            u16::try_from(town.height_tiles).expect("validated <= 8"),
        );
        let reach = i64::from(rules.town_radius_grown_tiles) + 3;
        let seat_r = i64::from(rules.town_radius_tiles);
        let res_r = i32::try_from(knobs.resource_radius_tiles).expect("validated <= 64");
        let penalty = i64::from(knobs.toward_enemy_penalty);
        let home_to_enemy = d2(own_start, enemy_start).isqrt();
        let mut expansion: Vec<(i64, u32, Tile)> = Vec::new();
        for nw in &tiles {
            let Some(centre) = tile_at(
                def,
                i32::from(nw.x) + i32::from((tw - 1) / 2),
                i32::from(nw.y) + i32::from((th - 1) / 2),
            ) else {
                continue;
            };
            if d2(centre, own_start) > reach * reach || !footprint_on_grass(def, *nw, tw, th) {
                continue;
            }
            let mut resources: i64 = 0;
            for dy in -res_r..=res_r {
                for dx in -res_r..=res_r {
                    if dx * dx + dy * dy > res_r * res_r {
                        continue;
                    }
                    let Some(t) = tile_at(def, i32::from(centre.x) + dx, i32::from(centre.y) + dy)
                    else {
                        continue;
                    };
                    let is_resource =
                        matches!(def.tile(t.x, t.y), Terrain::Forest | Terrain::Mountain);
                    if is_resource && d2(t, own_start) > seat_r * seat_r {
                        resources += 1;
                    }
                }
            }
            let gain = (home_to_enemy - d2(centre, enemy_start).isqrt()).max(0);
            expansion.push((resources - penalty * gain, index(def, *nw), *nw));
        }
        expansion.sort_by_key(|(score, i, _)| (core::cmp::Reverse(*score), *i));
        expansion.truncate(EXPANSION_CANDIDATES);

        let mut border: Vec<(i64, u32, Tile)> = tiles
            .iter()
            .filter(|t| d2(**t, own_start) <= scan * scan && def.tile(t.x, t.y) == Terrain::Grass)
            .map(|t| (d2(*t, enemy_start), index(def, *t), *t))
            .collect();
        border.sort_by_key(|(d, i, _)| (*d, *i));

        let dx = i64::from(enemy_start.x) - i64::from(own_start.x);
        let dy = i64::from(enemy_start.y) - i64::from(own_start.y);
        let len = d2(own_start, enemy_start).isqrt().max(1);
        let r = i64::from(knobs.rally_tiles_from_seat);
        let clamp =
            |v: i64, hi: u16| u16::try_from(v.clamp(0, i64::from(hi) - 1)).expect("clamped");
        let rally_seed = Tile::new(
            clamp(i64::from(own_start.x) + dx * r / len, def.width),
            clamp(i64::from(own_start.y) + dy * r / len, def.height),
        );

        MapAnalysis {
            me,
            own_start,
            enemy_start,
            near_town_offsets,
            terrain,
            expansion: expansion.into_iter().map(|c| c.2).collect(),
            border_edge: border.into_iter().map(|c| c.2).collect(),
            rally_seed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn rules() -> Rules {
        Rules::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).unwrap()
    }

    /// The tables are a pure function of the rules, the setup's map and
    /// starts and the knobs: rebuilt, they are equal, and the seed of the
    /// match does not enter them.
    #[test]
    fn map_analysis_is_a_pure_function_of_the_rules() {
        let r = rules();
        let k = r.ai.personality("standard").unwrap();
        let a = MatchSetup::bots(&r, 1, ("standard", "standard"), ("standard", "standard"));
        let b = MatchSetup::bots(&r, 99, ("standard", "standard"), ("standard", "standard"));
        for p in [PlayerId(0), PlayerId(1)] {
            let one = MapAnalysis::new(&r, &a, p, k);
            assert_eq!(one, MapAnalysis::new(&r, &a, p, k));
            assert_eq!(one, MapAnalysis::new(&r, &b, p, k));
        }
    }

    /// The shapes the site rules rely on, on `plains_1v1`: the starts, the
    /// rings, candidates on grass near the own start, and a rally toward
    /// the enemy.
    #[test]
    fn map_analysis_tables_have_the_documented_shape() {
        let r = rules();
        let k = r.ai.personality("standard").unwrap();
        let setup = MatchSetup::vs_bot(&r, 1, "standard", "standard");
        let def = r.map(&setup.map).unwrap();
        let p0 = MapAnalysis::new(&r, &setup, PlayerId(0), k);
        let p1 = MapAnalysis::new(&r, &setup, PlayerId(1), k);
        assert_eq!(
            (p0.own_start, p0.enemy_start),
            (Tile::new(24, 64), Tile::new(103, 64))
        );
        assert_eq!(
            (p1.own_start, p1.enemy_start),
            (Tile::new(103, 64), Tile::new(24, 64))
        );
        // Rings 4..=12: 8r offsets each, inner first, starting at the
        // north-west corner.
        let rings: usize = (4..=12).map(|r| 8 * r).sum();
        assert_eq!(p0.near_town_offsets.len(), rings);
        assert_eq!(p0.near_town_offsets[0], (-4, -4));
        assert_eq!(*p0.near_town_offsets.last().unwrap(), (-12, -11));
        for (kind, list) in &p0.terrain {
            let def_kind = r.building_kind(kind.0).unwrap();
            assert!(matches!(
                def_kind.gather.as_ref().unwrap().slots,
                Slots::Adjacent { .. }
            ));
            assert!(!list.is_empty(), "{}", def_kind.id);
            for t in list {
                assert!(d2(*t, p0.own_start) <= 40 * 40);
            }
        }
        assert_eq!(p0.terrain.len(), 2, "the Lumber Yard and the Ore Pit");
        assert!(!p0.expansion.is_empty() && p0.expansion.len() <= EXPANSION_CANDIDATES);
        for nw in &p0.expansion {
            assert!(footprint_on_grass(def, *nw, 3, 3));
        }
        assert_eq!(
            def.tile(p0.border_edge[0].x, p0.border_edge[0].y),
            Terrain::Grass
        );
        assert!(
            p0.border_edge[0].x > p0.own_start.x,
            "nearest the enemy first"
        );
        assert!(
            d2(p0.border_edge[0], p0.enemy_start)
                <= d2(*p0.border_edge.last().unwrap(), p0.enemy_start)
        );
        assert_eq!(p0.rally_seed, Tile::new(36, 64));
        assert_eq!(p1.rally_seed, Tile::new(91, 64));
    }
}
