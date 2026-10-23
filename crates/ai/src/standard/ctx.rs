//! [`Ctx`]: one plan's working state, and the [`Ledger`] that keeps the
//! plan's rules from spending the same stock twice.

use super::facts::Facts;
use super::sites::MapAnalysis;
use rules::{BuildOrder, Personality};
use sim::{Command, PlayerId, SimView, UnitId};
use std::collections::BTreeSet;

/// The stock one plan may still spend: what the stockpile holds, less
/// what was spent or reserved earlier in the same plan. All arithmetic
/// saturates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    /// The stockpile at the plan's start, less what this plan spent.
    pub stock: Vec<i64>,
    /// What earlier steps of this plan are saving up for.
    pub reserved: Vec<i64>,
}

impl Ledger {
    /// A ledger over `stock` with nothing reserved.
    pub fn new(stock: Vec<i64>) -> Ledger {
        let reserved = vec![0; stock.len()];
        Ledger { stock, reserved }
    }

    /// What resource `r` can still pay for: stock less reservations, at
    /// least 0.
    pub fn available(&self, r: usize) -> i64 {
        let stock = self.stock.get(r).copied().unwrap_or(0);
        let reserved = self.reserved.get(r).copied().unwrap_or(0);
        stock.saturating_sub(reserved).max(0)
    }

    /// Whether `cost` fits what is available.
    pub fn affords(&self, cost: &[i64]) -> bool {
        cost.iter()
            .enumerate()
            .all(|(r, c)| *c <= self.available(r))
    }

    /// Spend `cost` if it fits what is available; `false` (and nothing
    /// spent) otherwise.
    pub fn try_spend(&mut self, cost: &[i64]) -> bool {
        if !self.affords(cost) {
            return false;
        }
        for (r, c) in cost.iter().enumerate() {
            if let Some(s) = self.stock.get_mut(r) {
                *s = s.saturating_sub(*c);
            }
        }
        true
    }

    /// Hold `cost` back from later spending in this plan (a step saving
    /// up).
    pub fn reserve(&mut self, cost: &[i64]) {
        for (r, c) in cost.iter().enumerate() {
            if let Some(v) = self.reserved.get_mut(r) {
                *v = v.saturating_add(*c);
            }
        }
    }
}

/// One plan's working state. The rules run in a fixed order over the same
/// `Ctx`: each reads the view and the facts, claims the units it uses so a
/// later rule leaves them alone, spends or reserves through the ledger,
/// and pushes commands within the plan's caps.
pub struct Ctx<'a> {
    /// The view of the tick being planned.
    pub view: SimView<'a>,
    /// The bot's player.
    pub me: PlayerId,
    /// The personality's knobs.
    pub knobs: &'a Personality,
    /// The personality's build order.
    pub order: &'a BuildOrder,
    /// The plan's facts.
    pub facts: Facts,
    /// The static map analysis of the bot's seat.
    pub map: &'a MapAnalysis,
    /// The plan's stock.
    pub ledger: Ledger,
    /// Units an earlier rule of this plan already gave an order.
    pub claimed: BTreeSet<UnitId>,
    /// `check_build` probes (a full-map search each) left in this plan.
    pub probes_left: u32,
    /// Commands left in this plan.
    pub commands_left: u32,
    out: Vec<Command>,
}

impl<'a> Ctx<'a> {
    /// A fresh plan context: the facts gathered, the ledger at the stock,
    /// nothing claimed, the caps from the knobs.
    ///
    /// # Panics
    ///
    /// If the knobs name a build order the rules do not have (`Rules::load`
    /// rules it out).
    pub fn new(
        view: SimView<'a>,
        me: PlayerId,
        knobs: &'a Personality,
        map: &'a MapAnalysis,
    ) -> Ctx<'a> {
        let order = view
            .rules()
            .ai
            .build_order(&knobs.build_order)
            .expect("Rules::load checks every personality's build order");
        let facts = Facts::gather(&view, me);
        let ledger = Ledger::new(facts.stock.clone());
        Ctx {
            view,
            me,
            knobs,
            order,
            facts,
            map,
            ledger,
            claimed: BTreeSet::new(),
            probes_left: knobs.site_checks_per_think,
            commands_left: knobs.max_commands_per_think,
            out: Vec::new(),
        }
    }

    /// Queue `cmd` if the plan has a command left; `false` when the cap is
    /// reached (and nothing was queued).
    pub fn push(&mut self, cmd: Command) -> bool {
        if self.commands_left == 0 {
            return false;
        }
        self.commands_left -= 1;
        self.out.push(cmd);
        true
    }

    /// Claim `unit` for the rule that is ordering it; `false` when an
    /// earlier rule of this plan already claimed it.
    pub fn claim(&mut self, unit: UnitId) -> bool {
        self.claimed.insert(unit)
    }

    /// Whether an earlier rule of this plan claimed `unit`.
    pub fn is_claimed(&self, unit: UnitId) -> bool {
        self.claimed.contains(&unit)
    }

    /// The commands this plan issues, in the order the rules pushed them.
    pub fn into_commands(self) -> Vec<Command> {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_spends_only_what_is_not_reserved_and_saturates() {
        let mut l = Ledger::new(vec![150, 150, 0, 0]);
        assert!(l.try_spend(&[30, 0, 0, 0]));
        assert_eq!(l.stock, [120, 150, 0, 0]);
        l.reserve(&[100, 0, 0, 0]);
        assert_eq!(l.available(0), 20);
        assert!(!l.try_spend(&[40, 0, 0, 0]), "the reservation holds");
        assert_eq!(l.stock, [120, 150, 0, 0], "a refused spend changes nothing");
        assert!(l.try_spend(&[20, 50, 0, 0]));
        assert_eq!(l.available(0), 0);
        l.reserve(&[i64::MAX, 0, 0, 0]);
        assert_eq!(l.available(0), 0, "saturates, never negative");
        assert!(!l.affords(&[0, 0, 1, 0]));
        assert!(l.affords(&[0, 0, 0, 0]));
        assert_eq!(l.available(9), 0, "an unknown resource has nothing");
    }
}
