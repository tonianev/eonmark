//! Build-order satisfaction: when the view shows a step as done (never
//! when its command is issued). M5b slice 1 has only the satisfaction
//! table, which the play-bots `bo_done_tick_a` column reads; the window,
//! reservations, cooldowns and site keeper that act on it arrive with the
//! economy slice.

use super::facts::Facts;
use rules::{BuildOrder, Rules, Step};
use sim::{BuildingKindId, TechId, UnitKindId};

/// Whether the view (through `facts`) shows `step` as done:
///
/// | Step | Done when |
/// | --- | --- |
/// | `Workers` | workers alive plus queued reach the count |
/// | `Build` | own buildings of the kind, sites included, reach the count |
/// | `Train` | units of the kind alive plus queued reach the count |
/// | `Research` | the tech is researched or queued |
/// | `Advance` | the age is reached |
/// | `Military` | always |
/// | `Wait` | the tick reaches the time |
///
/// An id the rules do not know (which `Rules::load` rules out) is never
/// done.
pub fn satisfied(step: &Step, facts: &Facts, rules: &Rules) -> bool {
    match step {
        Step::Workers { count, .. } => facts
            .worker_kind
            .is_some_and(|w| facts.alive_and_queued(w) >= *count),
        Step::Build { kind, count, .. } => rules
            .building_kind_by_id(kind)
            .is_some_and(|(i, _)| facts.owned(BuildingKindId(i)) >= *count),
        Step::Train { kind, count, .. } => rules
            .units
            .iter()
            .position(|u| &u.id == kind)
            .and_then(|i| u16::try_from(i).ok())
            .is_some_and(|i| facts.alive_and_queued(UnitKindId(i)) >= *count),
        Step::Research { tech, .. } => rules
            .tech_by_id(tech)
            .is_some_and(|(i, _)| facts.researched_or_queued.contains(&TechId(i))),
        Step::Advance { age } => rules.age_index(age).is_some_and(|a| facts.age >= a),
        Step::Military => true,
        Step::Wait { until_ds } => facts.tick >= rules.ticks_from_ds(*until_ds),
    }
}

/// The index of the first step not done, skipping optional steps when
/// `required_only`; `None` when every (required) step is done.
pub fn first_unsatisfied(
    order: &BuildOrder,
    facts: &Facts,
    rules: &Rules,
    required_only: bool,
) -> Option<usize> {
    order
        .steps
        .iter()
        .position(|s| !(required_only && s.optional()) && !satisfied(s, facts, rules))
}
