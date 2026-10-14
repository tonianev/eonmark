//! M3a acceptance tests: the territory field, the standard start, Towns,
//! construction and base-yield income, and the economy fixture (gathering
//! and training join with the next M3a slices). Test names start with
//! `territory_` or `economy_` so that `cargo test -p sim territory` and
//! `cargo test -p sim economy` select them (the filter matches test paths,
//! not integration-test file names).

use proptest::prelude::*;
use rules::Rules;
use sim::{
    BuildingId, BuildingKindId, Command, Construction, FxVec2, Job, MatchSetup, PlayerCommand,
    PlayerId, RejectReason, Sim, SimEvent, SimView, Source, SourceOwner, StartKind, TerritoryField,
    Tile, UnitId, UnitKindId,
};
use std::path::{Path, PathBuf};

struct NoAi;

impl sim::AiController for NoAi {
    fn think(&mut self, _player: PlayerId, _view: &SimView<'_>) -> Vec<Command> {
        Vec::new()
    }
}

/// A two-player scenario (debug commands on) from the standard start.
fn standard(seed: u64) -> Sim {
    let rules = load_rules();
    let mut setup = MatchSetup::scenario(&rules, seed);
    setup.start = StartKind::Standard;
    Sim::new(setup, rules, Box::new(NoAi))
}

fn kind(rules: &Rules, id: &str) -> BuildingKindId {
    BuildingKindId(
        rules
            .building_kind_by_id(id)
            .expect("kind in buildings.ron")
            .0,
    )
}

fn resource(rules: &Rules, id: &str) -> usize {
    rules.resource_index(id).expect("resource in resources.ron")
}

/// Issue `cmds` this tick, step until they have been applied, and return
/// every event since.
fn apply(sim: &mut Sim, cmds: Vec<PlayerCommand>) -> Vec<SimEvent> {
    let delay = sim.setup().cmd_delay;
    sim.step(&cmds);
    for _ in 0..delay {
        sim.step(&[]);
    }
    sim.drain_events()
}

fn build(p: u8, seq: u32, unit: u32, kind: BuildingKindId, x: u16, y: u16) -> PlayerCommand {
    PlayerCommand::new(
        PlayerId(p),
        seq,
        Command::Build {
            unit: UnitId(unit),
            kind,
            tile: Tile::new(x, y),
        },
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

/// Step with no commands until `done` holds, at most `max` ticks; returns
/// whether it held.
fn run_until(sim: &mut Sim, max: u32, mut done: impl FnMut(&Sim) -> bool) -> bool {
    for _ in 0..max {
        if done(sim) {
            return true;
        }
        sim.step(&[]);
    }
    done(sim)
}

fn load_rules() -> Rules {
    Rules::load(data_dir()).expect("data/ loads")
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Verify `fixtures/<name>.eonreplay` and compare its final hash with
/// `fixtures/<name>.hash` (as `m1.rs` does).
fn golden(name: &str) {
    let replay = fixtures_dir().join(format!("{name}.eonreplay"));
    let hash_file = fixtures_dir().join(format!("{name}.hash"));
    let expected = std::fs::read_to_string(&hash_file)
        .unwrap_or_else(|e| panic!("{}: {e}", hash_file.display()));
    let outcome = sim::verify(&replay, &data_dir(), Box::new(NoAi))
        .unwrap_or_else(|e| panic!("{}: {e}", replay.display()));
    match outcome {
        sim::VerifyOutcome::Ok { final_hash, .. } => {
            assert_eq!(format!("0x{final_hash:016x}"), expected.trim(), "{name}");
        }
        other => panic!("{name}: {other}"),
    }
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

// ---------------------------------------------------------------------------
// The Town loop: standard start, Build, construction, cancel, income
// ---------------------------------------------------------------------------

/// The standard start on `plains_1v1`: Seats `BuildingId(1)` and `(2)`
/// centred on the map starts, P0's Yeomen `UnitId(1..=5)` and P1's
/// `(6..=10)`, the stockpiles from `rules.ron`, both Seats' borders and one
/// cost-grid generation bump per Seat.
#[test]
fn economy_standard_start_layout() {
    let rules = load_rules();
    let sim = standard(1);
    let view = sim.view();
    let def = rules.map(&rules.default_map).unwrap();
    let town = kind(&rules, "town");
    for (i, start) in def.starts.iter().enumerate() {
        let p = PlayerId(u8::try_from(i).unwrap());
        let seat = view
            .building(BuildingId(u32::try_from(i).unwrap() + 1))
            .unwrap();
        assert_eq!((seat.owner, seat.kind), (p, town));
        assert_eq!(seat.tile, Tile::new(start.x - 1, start.y - 1));
        assert_eq!(seat.construction, Construction::Complete);
        let ts = seat.town_state.unwrap();
        assert!(ts.seat);
        assert_eq!(ts.tier, 1);
        assert!(!view.map().passable(Tile::new(start.x, start.y)));
        assert_eq!(
            view.economy(p).stock,
            rules.amounts(&rules.standard_start.stockpile)
        );
        assert_eq!(view.towns(p), (1, rules.town_limit_base));
    }
    let per_player = rules.standard_start.units["yeoman"];
    for u in view.units() {
        let p = u8::try_from((u.id.0 - 1) / per_player).unwrap();
        assert_eq!(
            (u.owner, u.kind),
            (PlayerId(p), UnitKindId(0)),
            "{:?}",
            u.id
        );
        assert!(u.job.is_none() && u.order.is_none());
    }
    assert_eq!(view.unit_count(), usize::try_from(2 * per_player).unwrap());
    assert_eq!(view.map().cost_grid_generation(), 2);
    if rules.town_radius_tiles == 20 {
        assert_eq!(view.territory().owned_tile_count(PlayerId(0)), 1245);
        assert_eq!(view.territory().owned_tile_count(PlayerId(1)), 1245);
    }
}

/// M3a territory acceptance at sim level: from the standard start, a P0
/// Watchtower completed at (43, 64) gives the golden counts and leaves the
/// mirrored midline (columns 63 and 64) neutral.
#[test]
fn territory_fixture_two_towns_and_a_watchtower_has_golden_owned_tiles_and_a_neutral_midline() {
    let rules = load_rules();
    let mut sim = standard(2);
    let events = apply(
        &mut sim,
        vec![PlayerCommand::new(
            PlayerId(0),
            0,
            Command::DebugPlaceBuilding {
                owner: PlayerId(0),
                kind: kind(&rules, "watchtower"),
                tile: Tile::new(43, 64),
                complete: true,
            },
        )],
    );
    assert!(events.contains(&SimEvent::TerritoryChanged), "{events:?}");
    let view = sim.view();
    let field = view.territory();
    for y in 0..field.height() {
        for x in [63, 64] {
            assert_eq!(field.owner_at(Tile::new(x, y)), None, "({x}, {y})");
        }
    }
    let (r1, _) = town_radii(&rules);
    if (r1, watchtower_radius(&rules)) == (20, 2) {
        assert_eq!(field.owned_tile_count(PlayerId(0)), 1248);
        assert_eq!(field.owned_tile_count(PlayerId(1)), 1245);
    }
}

/// A Build outside the player's borders is rejected with `OutsideBorders`
/// and changes nothing: compared with a control sim whose command at the
/// same (player, seq) is rejected for another reason, every hash and
/// sub-hash agrees on every following tick, and no randomness was drawn.
#[test]
fn territory_build_outside_own_borders_is_rejected_without_state_change() {
    let rules = load_rules();
    let croft = kind(&rules, "croft");
    let mut a = standard(3);
    let mut b = standard(3);
    let rng_before = a.sub_hashes().rng;
    let stock_before = a.view().economy(PlayerId(0)).stock.clone();
    let site = Tile::new(60, 64);
    assert_eq!(a.view().territory().owner_at(site), None);
    let ea = apply(&mut a, vec![build(0, 0, 1, croft, site.x, site.y)]);
    let eb = apply(
        &mut b,
        vec![PlayerCommand::new(
            PlayerId(0),
            0,
            Command::Stop {
                units: vec![UnitId(999)],
            },
        )],
    );
    assert_eq!(rejections(&ea), [RejectReason::OutsideBorders]);
    assert_eq!(rejections(&eb), [RejectReason::NoValidUnits]);
    for _ in 0..5 {
        assert_eq!(a.hash(), b.hash());
        assert_eq!(a.sub_hashes(), b.sub_hashes());
        a.step(&[]);
        b.step(&[]);
    }
    assert_eq!(a.sub_hashes().rng, rng_before);
    let view = a.view();
    assert_eq!(view.buildings().count(), 2);
    assert!(view.unit(UnitId(1)).unwrap().job.is_none());
    assert_eq!(
        view.economy(PlayerId(0)).stock[resource(&rules, "lumber")],
        stock_before[resource(&rules, "lumber")]
    );
}

/// The second Town costs exactly the `rules.ron` ramp over the first:
/// the quote, the rules function and the formula from the raw fields agree,
/// and the deduction (against a control sim) equals the quote.
#[test]
fn economy_second_town_costs_more_than_the_first_by_the_rules_formula() {
    let rules = load_rules();
    let town = kind(&rules, "town");
    let town_def = rules.building_kind(town.0).unwrap();
    let mut sim = standard(4);
    let mut control = standard(4);
    let quote = sim.view().building_cost(PlayerId(0), town);
    assert_eq!(quote, rules.building_cost(town_def, 1));
    let lumber = resource(&rules, "lumber");
    let base = i64::from(town_def.cost["lumber"]);
    let step = i64::from(rules.building_ramp_step_permille);
    assert_eq!(quote[lumber], base * (1000 + step) / 1000);
    assert!(quote[lumber] > rules.building_cost(town_def, 0)[lumber]);
    assert_eq!(
        sim.view()
            .check_build(PlayerId(0), UnitId(1), town, Tile::new(40, 63)),
        Ok(quote.clone())
    );

    let events = apply(&mut sim, vec![build(0, 0, 1, town, 40, 63)]);
    apply(&mut control, vec![]);
    assert!(rejections(&events).is_empty(), "{events:?}");
    let paid: Vec<i64> = control
        .view()
        .economy(PlayerId(0))
        .stock
        .iter()
        .zip(&sim.view().economy(PlayerId(0)).stock)
        .map(|(c, s)| c - s)
        .collect();
    assert_eq!(paid, quote);
    let site = sim.view().building(BuildingId(3)).unwrap();
    assert_eq!(site.paid, quote);
    assert_eq!(site.construction, Construction::Site { progress: 0 });
    assert_eq!(sim.view().towns(PlayerId(0)).0, 2, "a Town site counts");
    assert_eq!(
        sim.view().building_cost(PlayerId(0), town)[lumber],
        base * (1000 + 2 * step) / 1000,
        "the next copy ramps again"
    );
}

/// A founded Town extends its owner's borders when it completes, not when
/// it is placed: on completion the field equals a fresh field of the three
/// Town sources.
#[test]
fn territory_second_town_extends_borders_on_completion_not_at_placement() {
    let rules = load_rules();
    let town = kind(&rules, "town");
    let mut sim = standard(5);
    let before = sim.view().territory().clone();
    let events = apply(&mut sim, vec![build(0, 0, 1, town, 40, 63)]);
    assert!(events.contains(&SimEvent::BuildingPlaced {
        building: BuildingId(3)
    }));
    assert_eq!(sim.view().territory(), &before, "a site projects nothing");
    assert_eq!(
        sim.view().unit(UnitId(1)).unwrap().job,
        Some(Job::Build {
            site: BuildingId(3)
        })
    );
    let build_ticks = rules.build_time_ticks(rules.building_kind(town.0).unwrap());
    assert!(run_until(&mut sim, build_ticks + 600, |s| {
        s.view().building(BuildingId(3)).unwrap().construction == Construction::Complete
    }));
    let events = sim.drain_events();
    assert!(events.contains(&SimEvent::BuildingCompleted {
        building: BuildingId(3)
    }));
    assert!(events.contains(&SimEvent::TerritoryChanged));
    assert!(sim.view().unit(UnitId(1)).unwrap().job.is_none());
    let (r1, _) = town_radii(&rules);
    let def = rules.map(&rules.default_map).unwrap();
    let mut expected = TerritoryField::new(def.width, def.height, 2);
    for (i, s) in def.starts.iter().enumerate() {
        expected.add_source(
            BuildingId(u32::try_from(i).unwrap() + 1),
            source(player(u8::try_from(i).unwrap()), s.x, s.y, r1),
        );
    }
    expected.add_source(BuildingId(3), source(player(0), 41, 64, r1));
    assert_eq!(sim.view().territory(), &expected);
    assert!(
        expected.owned_tile_count(PlayerId(0)) > before.owned_tile_count(PlayerId(0)),
        "the second Town claims new tiles"
    );
    let t = sim
        .view()
        .building(BuildingId(3))
        .unwrap()
        .town_state
        .unwrap();
    assert!(!t.seat);
    assert_eq!(t.tier, 1);
}

/// A Town yields exactly its `base_yield` per income period from any phase,
/// and nothing else moves without workers.
#[test]
fn economy_town_base_yield_is_exact_per_income_period() {
    let rules = load_rules();
    let town = rules.building_kind(rules.town_kind()).unwrap();
    let period = rules.income_period_ticks();
    let start = rules.amounts(&rules.standard_start.stockpile);
    let yields = rules.amounts(&town.base_yield);
    let mut sim = standard(6);
    let mut at = Vec::new();
    for t in 0..=(2 * period + 300) {
        if t % 50 == 0 || t == period || t == 2 * period {
            at.push((t, sim.view().economy(PlayerId(0)).stock.clone()));
        }
        sim.step(&[]);
    }
    for (t, stock) in &at {
        for r in 0..start.len() {
            assert_eq!(
                stock[r],
                start[r] + yields[r] * i64::from(*t) / i64::from(period),
                "resource {r} at tick {t}"
            );
        }
    }
    let per = |t: u32| at.iter().find(|(x, _)| *x == t).unwrap().1.clone();
    for phase in [0, 50, 300] {
        let d: Vec<i64> = per(phase + period)
            .iter()
            .zip(per(phase))
            .map(|(a, b)| a - b)
            .collect();
        assert_eq!(d, yields, "one period from tick {phase}");
    }
}

/// Every `Build` rejection reason, each from a fresh standard start, and
/// each leaving the RNG, the stockpile and the building count unchanged.
#[test]
fn economy_build_rejections() {
    let rules = load_rules();
    let town = kind(&rules, "town");
    let croft = kind(&rules, "croft");
    let ore = u8::try_from(resource(&rules, "ore")).unwrap();
    let place = |seq: u32, k: BuildingKindId, x: u16, y: u16| {
        PlayerCommand::new(
            PlayerId(0),
            seq,
            Command::DebugPlaceBuilding {
                owner: PlayerId(0),
                kind: k,
                tile: Tile::new(x, y),
                complete: false,
            },
        )
    };
    type Setup = Box<dyn Fn(&mut Sim)>;
    let no_setup: Setup = Box::new(|_| {});
    let cases: Vec<(&str, Setup, PlayerCommand, RejectReason)> = vec![
        (
            "unknown unit",
            no_setup,
            build(0, 0, 999, croft, 28, 60),
            RejectReason::NoValidUnits,
        ),
        (
            "another player's unit",
            Box::new(|_| {}),
            build(0, 0, 6, croft, 28, 60),
            RejectReason::NoValidUnits,
        ),
        (
            "unknown kind",
            Box::new(|_| {}),
            build(0, 0, 1, BuildingKindId(999), 28, 60),
            RejectReason::UnknownBuildingKind,
        ),
        (
            "not a builder",
            Box::new(|s: &mut Sim| {
                apply(
                    s,
                    vec![PlayerCommand::new(
                        PlayerId(0),
                        100,
                        Command::DebugSpawn {
                            owner: PlayerId(0),
                            kind: UnitKindId(1),
                            at: FxVec2::from_ints(30, 70),
                            count: 1,
                        },
                    )],
                );
            }),
            build(0, 0, 11, croft, 28, 60),
            RejectReason::NotABuilder,
        ),
        (
            "off the map",
            Box::new(|_| {}),
            build(0, 0, 1, croft, 127, 127),
            RejectReason::SiteOutOfBounds,
        ),
        (
            "outside borders",
            Box::new(|_| {}),
            build(0, 0, 1, croft, 60, 64),
            RejectReason::OutsideBorders,
        ),
        (
            "on the Seat",
            Box::new(|_| {}),
            build(0, 0, 1, croft, 24, 64),
            RejectReason::SiteBlocked,
        ),
        (
            "on forest",
            Box::new(|_| {}),
            build(0, 0, 1, croft, 14, 50),
            RejectReason::SiteBlocked,
        ),
        (
            "on a Yeoman",
            Box::new(|_| {}),
            build(0, 0, 2, croft, 26, 64),
            RejectReason::SiteOccupied,
        ),
        (
            "third Town",
            Box::new(move |s: &mut Sim| {
                apply(s, vec![build(0, 100, 1, town, 40, 63)]);
            }),
            build(0, 0, 2, town, 30, 70),
            RejectReason::TownLimitReached,
        ),
        (
            "sixth Croft at the Seat",
            Box::new(move |s: &mut Sim| {
                let crofts = (0..5u16).map(|i| place(100 + u32::from(i), croft, 28 + 3 * i, 58));
                apply(s, crofts.collect());
            }),
            build(0, 0, 1, croft, 28, 60),
            RejectReason::PerTownLimitReached,
        ),
        (
            "Lumber Yard with no forest beside it",
            Box::new(|_| {}),
            build(0, 0, 1, kind(&rules, "lumber_yard"), 28, 60),
            RejectReason::NoAdjacentResource,
        ),
        (
            "Watchtower without Ore",
            Box::new(|_| {}),
            build(0, 0, 1, kind(&rules, "watchtower"), 28, 60),
            RejectReason::CannotAfford { resource: ore },
        ),
    ];
    for (name, setup, cmd, want) in cases {
        let mut sim = standard(7);
        setup(&mut sim);
        let rng = sim.sub_hashes().rng;
        let stock = sim.view().economy(PlayerId(0)).stock.clone();
        let buildings = sim.view().buildings().count();
        let events = apply(&mut sim, vec![cmd]);
        assert_eq!(rejections(&events), [want], "{name}");
        assert_eq!(sim.sub_hashes().rng, rng, "{name}: no RNG draw");
        assert_eq!(sim.view().buildings().count(), buildings, "{name}");
        // Two ticks of income may have arrived; nothing was deducted.
        let now = &sim.view().economy(PlayerId(0)).stock;
        assert!(now.iter().zip(&stock).all(|(n, s)| n >= s), "{name}");
    }
}

/// A Croft needs a complete Town to belong to: with an empty start and only
/// borders from two debug Watchtowers, `Build` is `PerTownLimitReached`.
#[test]
fn economy_croft_without_a_town_is_rejected() {
    let rules = load_rules();
    let mut setup = MatchSetup::scenario(&rules, 8);
    setup.start = StartKind::Empty;
    let mut sim = Sim::new(setup, load_rules(), Box::new(NoAi));
    let lumber = u8::try_from(resource(&rules, "lumber")).unwrap();
    apply(
        &mut sim,
        vec![
            PlayerCommand::new(
                PlayerId(0),
                0,
                Command::DebugPlaceBuilding {
                    owner: PlayerId(0),
                    kind: kind(&rules, "watchtower"),
                    tile: Tile::new(30, 64),
                    complete: true,
                },
            ),
            PlayerCommand::new(
                PlayerId(0),
                1,
                Command::DebugPlaceBuilding {
                    owner: PlayerId(0),
                    kind: kind(&rules, "watchtower"),
                    tile: Tile::new(33, 64),
                    complete: true,
                },
            ),
            PlayerCommand::new(
                PlayerId(0),
                2,
                Command::DebugSpawn {
                    owner: PlayerId(0),
                    kind: UnitKindId(0),
                    at: FxVec2::from_ints(33, 67),
                    count: 1,
                },
            ),
            PlayerCommand::new(
                PlayerId(0),
                3,
                Command::DebugGrant {
                    owner: PlayerId(0),
                    resource: lumber,
                    amount: 500,
                },
            ),
        ],
    );
    // The two 3x3 blocks touch: (31, 63)..(32, 64) is all P0's and free.
    for t in [(31, 63), (32, 63), (31, 64), (32, 64)] {
        assert!(
            sim.view()
                .territory()
                .is_buildable(PlayerId(0), Tile::new(t.0, t.1))
        );
    }
    let events = apply(
        &mut sim,
        vec![build(0, 4, 1, kind(&rules, "croft"), 31, 63)],
    );
    assert_eq!(rejections(&events), [RejectReason::PerTownLimitReached]);
}

/// Cancelling a site refunds exactly what was paid, unblocks the footprint
/// (one generation bump each way) and idles the builder; the economy is
/// then identical to a sim that never built.
#[test]
fn economy_site_cancel_refunds_and_unblocks() {
    let rules = load_rules();
    let croft = kind(&rules, "croft");
    let mut sim = standard(9);
    let mut control = standard(9);
    let g0 = sim.view().map().cost_grid_generation();
    apply(&mut sim, vec![build(0, 0, 1, croft, 28, 60)]);
    apply(&mut control, vec![]);
    let site = BuildingId(3);
    assert_eq!(sim.view().map().cost_grid_generation(), g0 + 1);
    assert!(!sim.view().map().passable(Tile::new(29, 61)));
    assert_ne!(sim.sub_hashes().economy, control.sub_hashes().economy);
    let events = apply(
        &mut sim,
        vec![PlayerCommand::new(
            PlayerId(0),
            1,
            Command::Cancel {
                building: site,
                slot: 0,
            },
        )],
    );
    apply(&mut control, vec![]);
    assert!(events.contains(&SimEvent::BuildingRemoved { building: site }));
    assert!(sim.view().building(site).is_none());
    assert_eq!(sim.view().map().cost_grid_generation(), g0 + 2);
    assert!(sim.view().map().passable(Tile::new(29, 61)));
    assert_eq!(sim.sub_hashes().economy, control.sub_hashes().economy);
    assert!(sim.view().unit(UnitId(1)).unwrap().job.is_none());

    // Slot 1 of a site, a complete building's empty queue and another
    // player's building are rejected.
    for (seq, building, slot, want) in [
        (2, BuildingId(1), 0, RejectReason::InvalidSlot),
        (3, BuildingId(2), 0, RejectReason::UnknownBuilding),
        (4, site, 0, RejectReason::UnknownBuilding),
    ] {
        let events = apply(
            &mut sim,
            vec![PlayerCommand::new(
                PlayerId(0),
                seq,
                Command::Cancel { building, slot },
            )],
        );
        assert_eq!(rejections(&events), [want], "{building:?} slot {slot}");
    }
    apply(&mut sim, vec![build(0, 5, 1, croft, 28, 60)]);
    let events = apply(
        &mut sim,
        vec![PlayerCommand::new(
            PlayerId(0),
            6,
            Command::Cancel {
                building: BuildingId(4),
                slot: 1,
            },
        )],
    );
    assert_eq!(rejections(&events), [RejectReason::InvalidSlot]);
}

/// A site whose builder was stopped is resumed by another Yeoman issuing
/// the same `Build`: nothing is charged twice, and the site completes.
#[test]
fn economy_builder_can_resume_a_site() {
    let rules = load_rules();
    let croft = kind(&rules, "croft");
    let mut sim = standard(10);
    apply(&mut sim, vec![build(0, 0, 1, croft, 28, 60)]);
    let site = BuildingId(3);
    apply(
        &mut sim,
        vec![PlayerCommand::new(
            PlayerId(0),
            1,
            Command::Stop {
                units: vec![UnitId(1)],
            },
        )],
    );
    assert!(sim.view().unit(UnitId(1)).unwrap().job.is_none());
    for _ in 0..200 {
        sim.step(&[]);
    }
    let Construction::Site { progress } = sim.view().building(site).unwrap().construction else {
        panic!("still a site");
    };
    assert_eq!(progress, 0, "no builder, no progress");
    let lumber = resource(&rules, "lumber");
    let before = sim.view().economy(PlayerId(0)).stock[lumber];
    let events = apply(&mut sim, vec![build(0, 2, 2, croft, 28, 60)]);
    assert!(rejections(&events).is_empty(), "{events:?}");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SimEvent::BuildingPlaced { .. }))
    );
    assert!(
        sim.view().economy(PlayerId(0)).stock[lumber] >= before,
        "no charge"
    );
    assert_eq!(
        sim.view().unit(UnitId(2)).unwrap().job,
        Some(Job::Build { site })
    );
    let ticks = rules.build_time_ticks(rules.building_kind(croft.0).unwrap());
    assert!(run_until(&mut sim, ticks + 400, |s| {
        s.view().building(site).unwrap().construction == Construction::Complete
    }));
}

/// Snapshot mid-construction, restore into a fresh sim, keep issuing
/// commands: every later tick hashes the same as the uninterrupted run.
#[test]
fn economy_restore_mid_construction_matches_uninterrupted() {
    let rules = load_rules();
    let town = kind(&rules, "town");
    let croft = kind(&rules, "croft");
    let mut a = standard(11);
    a.step(&[build(0, 0, 1, town, 40, 63)]);
    for _ in 1..600 {
        a.step(&[]);
    }
    assert!(matches!(
        a.view().building(BuildingId(3)).unwrap().construction,
        Construction::Site { progress } if progress > 0
    ));
    let mut b = standard(11);
    b.restore(&a.snapshot()).unwrap();
    assert_eq!(a.hash(), b.hash());
    for t in 600..2000u32 {
        let cmds = if t == 700 {
            vec![build(0, 1, 2, croft, 28, 60)]
        } else {
            Vec::new()
        };
        a.step(&cmds);
        b.step(&cmds);
        assert_eq!(a.hash(), b.hash(), "tick {t}");
    }
    assert_eq!(
        a.view().building(BuildingId(3)).unwrap().construction,
        Construction::Complete
    );
}

// ---------------------------------------------------------------------------
// The economy fixture
// ---------------------------------------------------------------------------

/// The M3a economy fixture: from the standard start, each player's fixed
/// build order founds a second Town that is complete, with its border, at
/// a tick below 3600; every command is accepted, and the owned tile counts
/// grow and stay mirrored.
#[test]
fn economy_second_town_completes_before_tick_3600() {
    let rules = load_rules();
    let (setup, stream) = sim::scenarios::economy_second_town(&rules);
    assert_eq!(setup.start, StartKind::Standard);
    let ticks = sim::scenarios::ECONOMY_SECOND_TOWN_TICKS;
    assert_eq!(ticks, 3600);
    let start_owned = 1245;
    let mut sim = Sim::new(setup, load_rules(), Box::new(NoAi));
    let mut founded = None;
    let mut events = Vec::new();
    for t in 0..ticks {
        sim.step(sim::scenarios::commands_at(&stream, t));
        events.extend(sim.drain_events());
        let both = [PlayerId(0), PlayerId(1)].iter().all(|&p| {
            sim.view()
                .buildings()
                .filter(|b| {
                    b.owner == p
                        && b.kind.0 == rules.town_kind()
                        && b.construction == Construction::Complete
                })
                .count()
                == 2
        });
        if both && founded.is_none() {
            founded = Some(sim.tick());
        }
    }
    let founded = founded.expect("both players found a second Town");
    assert!(founded < 3600, "second Towns complete at tick {founded}");
    assert!(rejections(&events).is_empty(), "{:?}", rejections(&events));
    let placed = events
        .iter()
        .filter(|e| matches!(e, SimEvent::BuildingPlaced { .. }))
        .count();
    assert_eq!(placed, 6, "two Towns and four Crofts");
    let field = sim.view().territory();
    let (p0, p1) = (
        field.owned_tile_count(PlayerId(0)),
        field.owned_tile_count(PlayerId(1)),
    );
    assert_eq!(p0, p1, "mirrored build orders own mirrored territory");
    if town_radii(&rules).0 == 20 {
        assert!(p0 > start_owned, "{p0}");
    }
    // The Crofts placed at tick 2400 belong to the new Towns.
    let new_town = |p: u8| {
        sim.view()
            .buildings()
            .find(|b| {
                b.owner == PlayerId(p)
                    && b.kind.0 == rules.town_kind()
                    && !b.town_state.unwrap().seat
            })
            .unwrap()
            .id
    };
    for p in [0u8, 1] {
        let late = sim
            .view()
            .buildings()
            .filter(|b| b.owner == PlayerId(p) && b.kind == kind(&rules, "croft"))
            .max_by_key(|b| b.id)
            .unwrap();
        assert_eq!(late.town, Some(new_town(p)), "player {p}");
    }
}

#[test]
fn golden_economy_second_town() {
    golden("economy_second_town");
}
