//! Training queues: `Train`, queue `Cancel`, `SetRally` and the training
//! step (`docs/design/economy.md`, "Ramping costs" and "Population cap").
//!
//! A complete building whose kind is a unit kind's `trainer` queues that
//! kind, up to `train_queue_max` entries. The cost is deducted and the pop
//! charged when an entry is queued; `Cancel` refunds exactly what the entry
//! paid. Only the head progresses, one tick per step, and a finished unit
//! appears beside the building, on the side it can leave from (see
//! `spawn_tile`). It walks to the building's rally point when one is set,
//! or else takes a free slot at its own building when its kind works it (a
//! Scribe at a Scriptorium); otherwise it starts idle, and a worker kind
//! then seeks work like any idle worker while any other kind stands.

use crate::construction::{building_rect, centre_tile, order_move, ring};
use crate::economy::{self, building_kind, first_short};
use crate::fx::FxVec2;
use crate::ids::{BuildingId, PlayerId, Tile, UnitKindId};
use crate::movement;
use crate::state::{
    Building, Construction, QueueEntry, QueueItem, RejectReason, SimEvent, State, Unit,
};
use rules::Rules;
use std::collections::{BTreeMap, BTreeSet};

/// Pop `p` uses: every own unit alive plus every entry queued at an own
/// building, each at its kind's `pop`.
pub(crate) fn pop_used(state: &State, rules: &Rules, p: PlayerId) -> u32 {
    let pop = |k: UnitKindId| rules.unit_kind(k.0).expect("kind in state").pop;
    let alive: u32 = state
        .units
        .values()
        .filter(|u| u.owner == p)
        .map(|u| pop(u.kind))
        .sum();
    let queued: u32 = state
        .buildings
        .values()
        .filter(|b| b.owner == p)
        .flat_map(|b| &b.queue)
        .map(|e| match e.item {
            QueueItem::Unit(k) => pop(k),
        })
        .sum();
    alive.checked_add(queued).expect("pop fits u32")
}

/// `p`'s population cap. Arms research arrives in M4a; until then every
/// player is at level 0.
pub(crate) fn pop_cap(rules: &Rules, _p: PlayerId) -> u32 {
    rules.pop_cap(0)
}

/// What one more unit of `kind` costs `p` now. `n` counts own units of the
/// kind alive or queued (the standard start's Yeomen included); `k` is 1
/// plus the entries queued at own buildings of the kind's trainer kind, of
/// any unit kind, so every Muster Hall shares one military ramp.
pub(crate) fn unit_cost_for(
    state: &State,
    rules: &Rules,
    p: PlayerId,
    kind: UnitKindId,
) -> Vec<i64> {
    let def = rules.unit_kind(kind.0).expect("validated unit kind");
    let (trainer, _) = rules
        .building_kind_by_id(&def.trainer)
        .expect("validated trainer");
    let alive = state
        .units
        .values()
        .filter(|u| u.owner == p && u.kind == kind)
        .count();
    let mut queued_same = 0usize;
    let mut queued_class = 0usize;
    for b in state.buildings.values().filter(|b| b.owner == p) {
        for e in &b.queue {
            let QueueItem::Unit(k) = e.item;
            if k == kind {
                queued_same += 1;
            }
            if b.kind.0 == trainer {
                queued_class += 1;
            }
        }
    }
    let n = u32::try_from(alive + queued_same).expect("count fits u32");
    let k = u32::try_from(1 + queued_class).expect("count fits u32");
    rules.unit_cost(def, n, k)
}

/// Validate `Train { building, kind }` for `p` without changing anything.
/// Checks, first failure wins: the building is `p`'s (`UnknownBuilding`)
/// and complete (`UnderConstruction`); the unit kind exists
/// (`UnknownUnitKind`) and is trained by this building's kind
/// (`CannotTrainHere`); the queue has room (`QueueFull`); the unit fits
/// under the pop cap, queued units included (`PopCapReached`); `p` can pay
/// the ramped cost (`CannotAfford`, first short resource). Returns the cost.
pub(crate) fn validate_train(
    state: &State,
    rules: &Rules,
    p: PlayerId,
    building: BuildingId,
    kind: UnitKindId,
) -> Result<Vec<i64>, RejectReason> {
    let b = state
        .buildings
        .get(&building)
        .filter(|b| b.owner == p)
        .ok_or(RejectReason::UnknownBuilding)?;
    if b.construction != Construction::Complete {
        return Err(RejectReason::UnderConstruction);
    }
    let def = rules
        .unit_kind(kind.0)
        .ok_or(RejectReason::UnknownUnitKind)?;
    if building_kind(rules, b.kind).id != def.trainer {
        return Err(RejectReason::CannotTrainHere);
    }
    let max = usize::try_from(rules.train_queue_max).expect("validated <= 255");
    if b.queue.len() >= max {
        return Err(RejectReason::QueueFull);
    }
    let used = pop_used(state, rules, p);
    if used.checked_add(def.pop).expect("pop fits u32") > pop_cap(rules, p) {
        return Err(RejectReason::PopCapReached);
    }
    let cost = unit_cost_for(state, rules, p, kind);
    if let Some(resource) = first_short(&state.economy.player(p).stock, &cost) {
        return Err(RejectReason::CannotAfford { resource });
    }
    Ok(cost)
}

/// Apply a validated `Train`: deduct and queue.
pub(crate) fn enqueue(
    state: &mut State,
    p: PlayerId,
    building: BuildingId,
    kind: UnitKindId,
    cost: Vec<i64>,
) {
    economy::deduct(&mut state.economy.player_mut(p).stock, &cost);
    state
        .buildings
        .get_mut(&building)
        .expect("validated building")
        .queue
        .push(QueueEntry {
            item: QueueItem::Unit(kind),
            paid: cost,
            progress: 0,
        });
}

/// Cancel queue entry `slot` of `building` (the caller checked it exists):
/// refund exactly what it paid, clamped to stockpile caps. The head's
/// progress is lost with it.
pub(crate) fn cancel_slot(
    state: &mut State,
    rules: &Rules,
    building: BuildingId,
    slot: u8,
    events: &mut Vec<SimEvent>,
) {
    let b = state
        .buildings
        .get_mut(&building)
        .expect("validated building");
    let entry = b.queue.remove(usize::from(slot));
    let owner = b.owner;
    economy::refund(
        rules,
        &mut state.economy.player_mut(owner).stock,
        &entry.paid,
    );
    events.push(SimEvent::TrainingCancelled { building, slot });
}

/// Step 6b: every complete building's queue head gains one tick, in id
/// order; a head that reaches its kind's training time becomes a unit. The
/// entry behind it becomes the head and gains its first tick in the next
/// step, so a busy building turns out one unit every `train_time` steps.
pub(crate) fn step(state: &mut State, rules: &Rules, tick: u32, events: &mut Vec<SimEvent>) {
    let ids: Vec<BuildingId> = state
        .buildings
        .values()
        .filter(|b| b.construction == Construction::Complete && !b.queue.is_empty())
        .map(|b| b.id)
        .collect();
    for id in ids {
        let done = {
            let head = &mut state.buildings.get_mut(&id).expect("listed").queue[0];
            let QueueItem::Unit(kind) = head.item;
            let def = rules.unit_kind(kind.0).expect("validated unit kind");
            head.progress = (head.progress + 1).min(rules.train_time_ticks(def));
            head.progress == rules.train_time_ticks(def)
        };
        if !done {
            continue;
        }
        let b = &state.buildings[&id];
        let owner = b.owner;
        let goal = b
            .rally
            .map(|target| movement::group_targets(&state.map, target, 1)[0]);
        // A finished head waits, complete, while no tile around can take
        // the unit (never on a real map; the spiral covers every tile).
        let Some(tile) = spawn_tile(state, rules, b, goal.map(|g| state.map.tile_of(g))) else {
            continue;
        };
        let b = state.buildings.get_mut(&id).expect("listed");
        let QueueItem::Unit(kind) = b.queue.remove(0).item;
        let def = rules.unit_kind(kind.0).expect("validated unit kind");
        let unit = state.ids.unit();
        let u = Unit::spawn(unit, owner, kind, tile, def, &state.map, rules.tick_rate_hz);
        state.units.insert(unit, u);
        events.push(SimEvent::UnitSpawned { unit });
        match goal {
            Some(goal) => order_move(state, rules, unit, goal, tick),
            None => {
                crate::gather::unit_takes_slot(state, rules, unit, id, tick);
            }
        }
    }
}

/// Where a unit trained at `b` appears: the first passable tile of the
/// spiral around the footprint's centre tile that lies in the spawn
/// component. With a rally goal on a component that touches the building's
/// ring, that is the goal's component; otherwise it is the largest
/// component, by tile count and then the lower id, among the passable ring
/// tiles, so a pocket that neighbouring buildings seal against this one
/// never takes the unit. With no passable ring tile, any passable tile.
/// The tile may hold a unit already; separation pushes them apart.
fn spawn_tile(state: &State, rules: &Rules, b: &Building, goal: Option<Tile>) -> Option<Tile> {
    let map = &state.map;
    let rect = building_rect(rules, map, b);
    let touching: BTreeSet<u16> = ring(rect, map.width(), map.height())
        .into_iter()
        .filter(|t| map.passable(*t))
        .map(|t| map.component_of(t))
        .collect();
    let want = goal
        .map(|g| map.component_of(g))
        .filter(|c| touching.contains(c))
        .or_else(|| {
            if touching.len() <= 1 {
                return touching.first().copied();
            }
            let mut size: BTreeMap<u16, u32> = touching.iter().map(|&c| (c, 0)).collect();
            for c in map.components() {
                if let Some(n) = size.get_mut(c) {
                    *n += 1;
                }
            }
            size.into_iter()
                .max_by_key(|&(c, n)| (n, std::cmp::Reverse(c)))
                .map(|(c, _)| c)
        });
    let centre = centre_tile(building_kind(rules, b.kind), b.tile);
    map.spiral(centre)
        .find(|t| map.passable(*t) && want.is_none_or(|w| map.component_of(*t) == w))
}

/// Validate `SetRally { building, target }`: the building (complete or a
/// site) must be `p`'s.
pub(crate) fn validate_rally(
    state: &State,
    p: PlayerId,
    building: BuildingId,
) -> Result<(), RejectReason> {
    state
        .buildings
        .get(&building)
        .filter(|b| b.owner == p)
        .map(|_| ())
        .ok_or(RejectReason::UnknownBuilding)
}

/// Apply a validated `SetRally`.
pub(crate) fn set_rally(state: &mut State, building: BuildingId, target: FxVec2) {
    state
        .buildings
        .get_mut(&building)
        .expect("validated building")
        .rally = Some(target);
}
