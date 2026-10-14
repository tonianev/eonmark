//! M3a acceptance tests: the territory field, and from the next M3a pull
//! requests the economy, Towns and construction. Test names start with
//! `territory_` or `economy_` so that `cargo test -p sim territory` and
//! `cargo test -p sim economy` select them (the filter matches test paths,
//! not integration-test file names).

use proptest::prelude::*;
use rules::Rules;
use sim::{BuildingId, PlayerId, Source, SourceOwner, TerritoryField, Tile};
use std::path::{Path, PathBuf};

fn load_rules() -> Rules {
    Rules::load(data_dir()).expect("data/ loads")
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// 32 cases locally, 256 under CI, `PROPTEST_CASES` overrides (as in m1.rs).
fn proptest_cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if std::env::var_os("CI").is_some() {
            256
        } else {
            32
        })
}

/// The Town radii from `rules.ron` (tier 1, tier 2).
fn town_radii(rules: &Rules) -> (u16, u16) {
    (
        u16::try_from(rules.town_radius_tiles).expect("radius fits u16"),
        u16::try_from(rules.town_radius_grown_tiles).expect("radius fits u16"),
    )
}

/// The Watchtower's border radius from `buildings.ron`.
fn watchtower_radius(rules: &Rules) -> u16 {
    let (_, tower) = rules
        .building_kind_by_id("watchtower")
        .expect("buildings.ron has a watchtower");
    u16::try_from(
        tower
            .border_radius_tiles
            .expect("the watchtower projects a border"),
    )
    .expect("radius fits u16")
}

/// Every tile's owner on a `w` x `h` field, checked against the oracle;
/// returns (P0 tiles, P1 tiles, neutral tiles).
fn counts_checked_against_oracle(f: &TerritoryField, sources: &[Source]) -> (u32, u32, u32) {
    let (mut p0, mut p1, mut neutral) = (0, 0, 0);
    for y in 0..f.height() {
        for x in 0..f.width() {
            let t = Tile::new(x, y);
            let owner = f.owner_at(t);
            assert_eq!(owner, oracle_owner(sources, 2, t), "{t:?}");
            match owner {
                Some(PlayerId(0)) => p0 += 1,
                Some(_) => p1 += 1,
                None => neutral += 1,
            }
        }
    }
    assert_eq!(p0, f.owned_tile_count(PlayerId(0)));
    assert_eq!(p1, f.owned_tile_count(PlayerId(1)));
    (p0, p1, neutral)
}

fn source(owner: SourceOwner, x: u16, y: u16, radius: u16) -> Source {
    Source {
        owner,
        centre: Tile::new(x, y),
        radius,
    }
}

fn player(p: u8) -> SourceOwner {
    SourceOwner::Player(PlayerId(p))
}

/// Independent oracle: each tile's owner computed straight from the source
/// list, without the field's layers or incremental bookkeeping.
fn oracle_owner(sources: &[Source], players: u8, t: Tile) -> Option<PlayerId> {
    let mut strength = vec![0u32; usize::from(players) + 1];
    for s in sources {
        let k = sim::territory::kernel(
            s.radius,
            i32::from(t.x) - i32::from(s.centre.x),
            i32::from(t.y) - i32::from(s.centre.y),
        );
        let layer = match s.owner {
            SourceOwner::Neutral => 0,
            SourceOwner::Player(p) => usize::from(p.0) + 1,
        };
        strength[layer] += u32::from(k);
    }
    let best = *strength.iter().max().expect("at least one layer");
    let winners: Vec<usize> = (0..strength.len())
        .filter(|&l| strength[l] == best)
        .collect();
    match winners.as_slice() {
        [l] if best > 0 && *l > 0 => Some(PlayerId(u8::try_from(*l - 1).unwrap())),
        _ => None,
    }
}

fn hash(f: &TerritoryField) -> u64 {
    xxhash_rust::xxh3::xxh3_64(&postcard::to_allocvec(f).expect("field serialises"))
}

/// Two Towns mirrored about the centre line of the 128-wide map, at the
/// tier-1 radius from `rules.ron`. The map is even-width and mirrors x to
/// `127 - x`, so no tile is exactly equidistant from both centres: ties come
/// only from the floored square root, and every one of them is neutral.
#[test]
fn territory_mirrored_towns_tie_only_by_isqrt_floor_and_ties_go_neutral() {
    let rules = load_rules();
    let (r1, _) = town_radii(&rules);
    let map = rules.map(&rules.default_map).expect("default map");
    let (w, h) = (map.width, map.height);
    let west = source(player(0), 48, 64, r1);
    let east = source(player(1), w - 1 - 48, 64, r1);
    let mut f = TerritoryField::new(w, h, 2);
    f.add_source(BuildingId(1), west);
    f.add_source(BuildingId(2), east);

    // The field agrees with the oracle on every tile.
    let sources = [west, east];
    for y in 0..h {
        for x in 0..w {
            let t = Tile::new(x, y);
            assert_eq!(f.owner_at(t), oracle_owner(&sources, 2, t), "{t:?}");
        }
    }
    // The owner map mirrors exactly: what one player owns at x, the other
    // owns at w - 1 - x.
    for y in 0..h {
        for x in 0..w {
            let a = f.owner_at(Tile::new(x, y));
            let b = f.owner_at(Tile::new(w - 1 - x, y));
            assert_eq!(a.map(|p| 1 - p.0), b.map(|p| p.0), "({x}, {y})");
        }
    }
    // Golden counts at R = 20 (regenerate from the oracle above if
    // `town_radius_tiles` moves).
    if r1 == 20 {
        assert_eq!(f.owned_tile_count(PlayerId(0)), 1167);
        assert_eq!(f.owned_tile_count(PlayerId(1)), 1167);
        let p0 = SourceOwner::Player(PlayerId(0));
        let p1 = SourceOwner::Player(PlayerId(1));
        let mut ties: Vec<Tile> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let t = Tile::new(x, y);
                let a = f.strength_at(p0, t);
                if a > 0 && a == f.strength_at(p1, t) {
                    ties.push(t);
                }
            }
        }
        let expected: Vec<Tile> = [(63, 54), (63, 56), (63, 72), (63, 74)]
            .into_iter()
            .flat_map(|(x, y)| [Tile::new(x, y), Tile::new(x + 1, y)])
            .collect();
        let mut sorted = ties.clone();
        sorted.sort();
        let mut exp = expected;
        exp.sort();
        assert_eq!(sorted, exp);
        for t in ties {
            assert_eq!(f.owner_at(t), None, "tie at {t:?} is neutral");
        }
    }
}

/// On an odd-width field the centre column is exactly equidistant from two
/// mirrored Towns, so every tile there that either kernel reaches is an
/// exact tie, and neutral.
#[test]
fn territory_odd_width_centre_column_is_neutral_wherever_reached() {
    let rules = load_rules();
    let (r1, _) = town_radii(&rules);
    let (w, h) = (127u16, 128u16);
    let mid = (w - 1) / 2;
    let mut f = TerritoryField::new(w, h, 2);
    f.add_source(BuildingId(1), source(player(0), 48, 64, r1));
    f.add_source(BuildingId(2), source(player(1), w - 1 - 48, 64, r1));
    let mut reached = 0;
    for y in 0..h {
        let t = Tile::new(mid, y);
        let a = f.strength_at(player(0), t);
        assert_eq!(a, f.strength_at(player(1), t));
        if a > 0 {
            reached += 1;
        }
        assert_eq!(f.owner_at(t), None, "centre column tile {t:?}");
    }
    assert!(reached > 0, "the kernels reach the centre column");
    if r1 == 20 {
        assert_eq!(reached, 27);
    }
}

/// The M3a territory acceptance fixture at field level: both Seats at the
/// map starts with the tier-1 radius, plus a P0 Watchtower at (43, 64) with
/// its radius from `buildings.ron`. Nothing reaches columns 63 and 64, so
/// the mirrored midline is neutral (the sim-level fixture from the standard
/// start lands with the Town loop).
#[test]
fn territory_fixture_two_towns_and_a_watchtower_golden_counts_field_level() {
    let rules = load_rules();
    let (r1, _) = town_radii(&rules);
    let tower = watchtower_radius(&rules);
    let map = rules.map(&rules.default_map).expect("default map");
    let mut f = TerritoryField::new(map.width, map.height, 2);
    let mut sources = Vec::new();
    for (p, s) in map.starts.iter().enumerate() {
        let src = source(player(u8::try_from(p).unwrap()), s.x, s.y, r1);
        f.add_source(BuildingId(u32::try_from(p).unwrap() + 1), src);
        sources.push(src);
    }
    let watch = source(player(0), 43, 64, tower);
    f.add_source(BuildingId(3), watch);
    sources.push(watch);

    let (p0, p1, neutral) = counts_checked_against_oracle(&f, &sources);
    for y in 0..map.height {
        for x in [map.width / 2 - 1, map.width / 2] {
            assert_eq!(f.owner_at(Tile::new(x, y)), None, "midline ({x}, {y})");
        }
    }
    if (r1, tower, map.width, map.height) == (20, 2, 128, 128) {
        assert_eq!((p0, p1, neutral), (1248, 1245, 13891));
    }
}

/// The contested configuration (mirrored Towns at (48, 64) and (79, 64))
/// plus a P0 Watchtower on the ford at (63, 64): P0 is stronger on its own
/// column, and the three tiles east of it become exact ties, neutral.
#[test]
fn territory_contested_watchtower_on_the_ford_creates_neutral_ties() {
    let rules = load_rules();
    let (r1, _) = town_radii(&rules);
    let tower = watchtower_radius(&rules);
    let map = rules.map(&rules.default_map).expect("default map");
    let (w, h) = (map.width, map.height);
    let sources = [
        source(player(0), 48, 64, r1),
        source(player(1), w - 1 - 48, 64, r1),
        source(player(0), 63, 64, tower),
    ];
    let mut f = TerritoryField::new(w, h, 2);
    for (i, s) in sources.iter().enumerate() {
        f.add_source(BuildingId(u32::try_from(i).unwrap() + 1), *s);
    }
    let counts = counts_checked_against_oracle(&f, &sources);
    if (r1, tower, w, h) == (20, 2, 128, 128) {
        assert_eq!(counts, (1167, 1164, 14053));
        assert_eq!(f.owner_at(Tile::new(63, 64)), Some(PlayerId(0)));
        for y in 63..=65 {
            let t = Tile::new(64, y);
            assert!(f.strength_at(player(0), t) > 0, "{t:?} reached");
            assert_eq!(
                f.strength_at(player(0), t),
                f.strength_at(player(1), t),
                "{t:?} tie"
            );
            assert_eq!(f.owner_at(t), None, "{t:?} neutral");
        }
    }
}

/// One random edit of the source set.
#[derive(Debug, Clone)]
enum Edit {
    /// Add a source under a new id; `mirror` also adds its mirror image for
    /// the other player, which puts ties on the centre line.
    Add {
        owner: u8,
        x: u16,
        y: u16,
        r: usize,
        mirror: bool,
    },
    /// Remove the n-th live source (modulo the count).
    Remove { n: usize },
    /// Change the n-th live source's owner, centre or radius.
    Change {
        n: usize,
        owner: u8,
        x: u16,
        y: u16,
        r: usize,
    },
}

/// Owners 0 and 1 are players, 2 is the neutral layer.
fn owner_of(code: u8) -> SourceOwner {
    match code % 3 {
        2 => SourceOwner::Neutral,
        p => player(p),
    }
}

/// Coordinates biased toward the edges and the centre line, where clipping
/// and ties live.
fn coord(size: u16) -> impl Strategy<Value = u16> {
    let max = size - 1;
    prop_oneof![
        0..size,
        0u16..4,
        (max - 3)..=max,
        (size / 2 - 3)..(size / 2 + 3),
    ]
}

fn edit(w: u16, h: u16) -> impl Strategy<Value = Edit> {
    prop_oneof![
        3 => (0u8..3, coord(w), coord(h), any::<usize>(), any::<bool>())
            .prop_map(|(owner, x, y, r, mirror)| Edit::Add { owner, x, y, r, mirror }),
        1 => any::<usize>().prop_map(|n| Edit::Remove { n }),
        2 => (any::<usize>(), 0u8..3, coord(w), coord(h), any::<usize>())
            .prop_map(|(n, owner, x, y, r)| Edit::Change { n, owner, x, y, r }),
    ]
}

/// Apply `edits` to a field incrementally and assert, every 10 edits and at
/// the end, that it equals a full recompute of its sources: as a value and
/// by hash of its serialised bytes.
fn check_incremental_equals_full(
    w: u16,
    h: u16,
    radii: &[u16],
    edits: &[Edit],
) -> Result<(), TestCaseError> {
    let mut f = TerritoryField::new(w, h, 2);
    let mut next = 1u32;
    for (i, e) in edits.iter().enumerate() {
        let live: Vec<BuildingId> = f.sources().keys().copied().collect();
        match *e {
            Edit::Add {
                owner,
                x,
                y,
                r,
                mirror,
            } => {
                let radius = radii[r % radii.len()];
                f.add_source(BuildingId(next), source(owner_of(owner), x, y, radius));
                next += 1;
                if mirror {
                    let other = match owner_of(owner) {
                        SourceOwner::Player(p) => player(1 - p.0),
                        SourceOwner::Neutral => SourceOwner::Neutral,
                    };
                    f.add_source(BuildingId(next), source(other, w - 1 - x, y, radius));
                    next += 1;
                }
            }
            Edit::Remove { n } if !live.is_empty() => {
                f.remove_source(live[n % live.len()]);
            }
            Edit::Change { n, owner, x, y, r } if !live.is_empty() => {
                let radius = radii[r % radii.len()];
                f.change_source(live[n % live.len()], source(owner_of(owner), x, y, radius));
            }
            Edit::Remove { .. } | Edit::Change { .. } => {}
        }
        if i % 10 == 9 || i + 1 == edits.len() {
            let full = f.recomputed();
            prop_assert_eq!(&f, &full, "incremental != full after edit {}", i);
            prop_assert_eq!(hash(&f), hash(&full));
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: proptest_cases(),
        .. ProptestConfig::default()
    })]

    /// 200 random source edits on the default map's field (adds, removes,
    /// moves, radius changes, owners moving to and from the neutral layer,
    /// mirrored pairs): incremental recompute equals a full recompute, by
    /// value and by hash. Radii are the Town tiers from `rules.ron`, each
    /// plus 0 to 3 for Statecraft (M4a), the Watchtower's from
    /// `buildings.ron` and small ones.
    #[test]
    fn territory_incremental_recompute_equals_full_recompute(
        edits in prop::collection::vec(edit(128, 128), 200),
    ) {
        let rules = load_rules();
        let map = rules.map(&rules.default_map).expect("default map");
        let (r1, r2) = town_radii(&rules);
        let mut radii: Vec<u16> = (0..=3).flat_map(|s| [r1 + s, r2 + s]).collect();
        radii.extend([1, 2, 3, watchtower_radius(&rules)]);
        check_incremental_equals_full(map.width, map.height, &radii, &edits)?;
    }

    /// The same on an odd-width, non-square field, where mirrored pairs
    /// produce exact ties on the centre column.
    #[test]
    fn territory_incremental_recompute_equals_full_on_an_odd_width_field(
        edits in prop::collection::vec(edit(47, 33), 200),
    ) {
        let rules = load_rules();
        let (r1, r2) = town_radii(&rules);
        let tower = watchtower_radius(&rules);
        check_incremental_equals_full(47, 33, &[1, 2, 5, tower, r1, r2], &edits)?;
    }
}
