//! Towns (`docs/design/towns.md`): the standard start, the town count and
//! limit, and attribution of other buildings to a Town.
//!
//! Tier-2 growth arrives with gathering in the next M3a slice; until then
//! every Town projects its tier-1 radius.

use crate::construction::{self, centre_fx};
use crate::economy::building_kind;
use crate::ids::{BuildingId, PlayerId, Tile, UnitKindId};
use crate::state::{Construction, State, TownState, Unit};
use crate::territory::{Source, SourceOwner};
use rules::{MapDef, Rules};

/// A player's Statecraft level. Research arrives in M4a; until then every
/// player is at level 0.
pub(crate) fn statecraft(_p: PlayerId) -> u32 {
    0
}

/// Town-kind buildings `p` owns, sites included (the town limit counts
/// both).
pub(crate) fn town_count(state: &State, rules: &Rules, p: PlayerId) -> u32 {
    let town = rules.town_kind();
    let n = state
        .buildings
        .values()
        .filter(|b| b.owner == p && b.kind.0 == town)
        .count();
    u32::try_from(n).expect("count fits u32")
}

/// The Town a new building centred at `centre` belongs to: the nearest own
/// complete Town by squared distance between footprint centres, ties by
/// `BuildingId`. `None` when `p` has no complete Town.
pub(crate) fn attribute(
    state: &State,
    rules: &Rules,
    p: PlayerId,
    centre: crate::fx::FxVec2,
) -> Option<BuildingId> {
    let town = rules.town_kind();
    state
        .buildings
        .values()
        .filter(|b| b.owner == p && b.kind.0 == town && b.construction == Construction::Complete)
        .min_by_key(|b| {
            let c = centre_fx(building_kind(rules, b.kind), b.tile);
            (c.dist_sq_i64(centre), b.id)
        })
        .map(|b| b.id)
}

/// The standard start (`rules.ron` `standard_start`), for every player in
/// id order: the Seat, a complete tier-1 Town centred on the player's map
/// start, projecting its border; then the start units on the nearest
/// passable tiles around it, in `UnitKindId` order; then the stockpile.
/// On a map whose starts are `(24, 64)` and `(103, 64)` that gives the
/// Seats `BuildingId(1)` and `BuildingId(2)`, P0's Yeomen `UnitId(1..=5)`
/// and P1's `UnitId(6..=10)`.
pub(crate) fn apply_standard_start(state: &mut State, rules: &Rules, def: &MapDef, hz: u32) {
    let town_kind = rules.town_kind();
    let kind = building_kind(rules, crate::ids::BuildingKindId(town_kind));
    let (w, h) = (
        u16::try_from(kind.width_tiles).expect("validated <= 8"),
        u16::try_from(kind.height_tiles).expect("validated <= 8"),
    );
    let players: Vec<PlayerId> = state.players.iter().map(|p| p.id).collect();
    for p in players {
        let start = def.starts[usize::from(p.0)];
        let nw = Tile::new(start.x - (w - 1) / 2, start.y - (h - 1) / 2);
        let seat = construction::place(
            state,
            rules,
            p,
            crate::ids::BuildingKindId(town_kind),
            nw,
            vec![0; rules.resources.resources.len()],
            0,
            None,
        );
        let b = state.buildings.get_mut(&seat).expect("just placed");
        b.construction = Construction::Complete;
        b.town_state = Some(TownState {
            seat: true,
            tier: 1,
        });
        let radius = rules.town_radius_for(1, statecraft(p));
        state.territory.add_source(
            seat,
            Source {
                owner: SourceOwner::Player(p),
                centre: Tile::new(start.x, start.y),
                radius: u16::try_from(radius).expect("validated radius fits u16"),
            },
        );
        for (k, unit_kind) in rules.units.iter().enumerate() {
            let Some(&count) = rules.standard_start.units.get(&unit_kind.id) else {
                continue;
            };
            let kind_id = UnitKindId(u16::try_from(k).expect("validated len"));
            let tiles: Vec<Tile> = state
                .map
                .spiral(Tile::new(start.x, start.y))
                .filter(|t| state.map.passable(*t))
                .take(usize::try_from(count).expect("validated <= 255"))
                .collect();
            for tile in tiles {
                let id = state.ids.unit();
                let u = Unit::spawn(id, p, kind_id, tile, unit_kind, &state.map, hz);
                state.units.insert(id, u);
            }
        }
        state.economy.player_mut(p).stock = rules.amounts(&rules.standard_start.stockpile);
    }
}
