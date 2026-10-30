//! The New Game selector (M6 owner decision 6): the main menu chooses the
//! difficulty (Easy, Standard or Hard, each with its disclosed bonus
//! caption read from the rules), the opponent's style (Random, Balanced,
//! Rush, Boom or Tower; Random draws one of rush, boom and tower per seed
//! with `ai::resolve_personality` and stays "?" until the game-over
//! screen) and the seed (the readout, New seed, and digits typed while the
//! menu is open, Backspace to delete; the first digit typed after the seed
//! was set for the player replaces it, so a blind seed such as 112 can be
//! typed over the prefilled one). Defaults: Standard, Random, a fresh
//! wall-clock seed; the speed (x1) persists across Play Again.
//!
//! [`NewGameSettings`] holds the choice; New Game turns it into a
//! `match_scope::MatchRequest` ([`NewGameSettings::setup`]), and Play
//! Again reuses it with a fresh seed. `--difficulty`, `--personality` and
//! `--seed` prefill it, and `--spectate` with `--spectate-a/-b` records
//! its seats so Play Again spectates again.

use bevy::prelude::*;
use rules::Rules;
use sim::{MatchSetup, PlayerId};

/// The opponent's style on New Game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleChoice {
    /// Drawn per seed from the random pool (rush, boom, tower), hidden as
    /// "?" until the game-over screen.
    Random,
    /// A personality id (`standard` is shown as Balanced).
    Id(String),
}

/// The styles New Game offers, in order.
pub const STYLES: [&str; 5] = ["random", "standard", "rush", "boom", "tower"];
/// The difficulties New Game offers, in order.
pub const DIFFICULTIES: [&str; 3] = ["easy", "standard", "hard"];
/// The most digits the seed field takes.
pub const SEED_DIGITS_MAX: usize = 10;

/// What New Game and Play Again build.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct NewGameSettings {
    /// The bot's difficulty id (or `passive`).
    pub difficulty: String,
    /// The bot's style.
    pub style: StyleChoice,
    /// The seed New Game uses.
    pub seed: u64,
    /// `--spectate`: both seats as `(bot, personality)`; Play Again then
    /// spectates again on a fresh seed.
    pub spectate: Option<[(String, String); 2]>,
    /// Whether the player has typed into the seed field since the seed was
    /// last set for them (the launch, New seed, opening the menu): until
    /// then the first digit typed replaces the prefilled seed instead of
    /// extending it ([`NewGameSettings::type_seed_key`]).
    pub seed_typed: bool,
}

impl Default for NewGameSettings {
    fn default() -> Self {
        Self {
            difficulty: "standard".into(),
            style: StyleChoice::Random,
            seed: 1,
            spectate: None,
            seed_typed: false,
        }
    }
}

impl NewGameSettings {
    /// The settings a launch starts with: `--difficulty` (Standard),
    /// `--personality` (`random` or none: Random on a plain launch, owner
    /// decision 6; `--skip-menu` keeps M5b's `standard` when none is
    /// given), the match's seed, and `--spectate`'s seats.
    pub fn from_cli(cli: &crate::cli::Cli, seed: u64) -> Self {
        let style = match cli.personality.as_deref() {
            Some(ai::RANDOM_PERSONALITY) => StyleChoice::Random,
            Some(p) => StyleChoice::Id(p.to_string()),
            None if cli.starts_in_menu() => StyleChoice::Random,
            None => StyleChoice::Id(crate::app::STANDARD_PERSONALITY.to_string()),
        };
        let spectate = cli.spectate.then(|| {
            let d = || {
                (
                    ai::BOT_STANDARD.to_string(),
                    crate::app::STANDARD_PERSONALITY.to_string(),
                )
            };
            [
                cli.spectate_a.clone().unwrap_or_else(d),
                cli.spectate_b.clone().unwrap_or_else(d),
            ]
        });
        Self {
            difficulty: cli
                .difficulty
                .clone()
                .unwrap_or_else(|| ai::BOT_STANDARD.to_string()),
            style,
            seed,
            spectate,
            seed_typed: false,
        }
    }

    /// Show `seed` in the field as one set for the player (New seed, a
    /// fresh seed on opening the menu): the next digit typed replaces it.
    pub fn offer_seed(&mut self, seed: u64) {
        self.seed = seed;
        self.seed_typed = false;
    }

    /// One key typed while the menu is open: the first digit after the seed
    /// was set for the player replaces it (the owner types a blind seed
    /// such as 112 over a 9- or 10-digit wall-clock one), later digits
    /// append ([`edit_seed`]), Backspace deletes; any other key does
    /// nothing.
    pub fn type_seed_key(&mut self, key: KeyCode) {
        if key != KeyCode::Backspace && digit_of(key).is_none() {
            return;
        }
        let base = if self.seed_typed || key == KeyCode::Backspace {
            self.seed
        } else {
            0
        };
        self.seed = edit_seed(base, key);
        self.seed_typed = true;
    }

    /// The match these settings build at `seed`: the local player against
    /// the bot (`MatchSetup::vs_bot`, a Random style resolved for P1 at
    /// `seed`), or both seats spectated.
    ///
    /// # Errors
    ///
    /// Random with an empty pool.
    pub fn setup(&self, rules: &Rules, seed: u64) -> Result<MatchSetup, String> {
        if let Some([a, b]) = &self.spectate {
            return Ok(MatchSetup::bots(rules, seed, (&a.0, &a.1), (&b.0, &b.1)));
        }
        let personality = if self.difficulty == ai::BOT_PASSIVE {
            String::new()
        } else {
            match &self.style {
                StyleChoice::Random => ai::resolve_personality(rules, seed, PlayerId(1))?,
                StyleChoice::Id(p) => p.clone(),
            }
        };
        Ok(MatchSetup::vs_bot(
            rules,
            seed,
            &self.difficulty,
            &personality,
        ))
    }

    /// Whether the opponent's style is hidden until the outcome (Random).
    pub fn hides_style(&self) -> bool {
        self.spectate.is_none() && self.style == StyleChoice::Random
    }
}

/// A style's label: Random, Balanced (`standard`), or the personality's
/// name.
pub fn style_label(rules: &Rules, id: &str) -> String {
    match id {
        "random" => "Random".into(),
        "standard" => "Balanced".into(),
        other => rules
            .ai
            .personality(other)
            .map_or_else(|| crate::alert::title_case(other), |p| p.name.clone()),
    }
}

/// A difficulty's label: its display name.
pub fn difficulty_label(rules: &Rules, id: &str) -> String {
    rules
        .ai
        .difficulty(id)
        .map_or_else(|| crate::alert::title_case(id), |d| d.name.clone())
}

/// The disclosed bonus of difficulty `id`: "+20 Grain, +20 Lumber, +10 Ore
/// every 25 s from 3:00", or "No bonus income".
pub fn bonus_caption(rules: &Rules, id: &str) -> String {
    let Some(d) = rules.ai.difficulty(id) else {
        return "No bonus income".into();
    };
    if d.income_interval_ds == 0 || d.income.values().all(|v| *v == 0) {
        return "No bonus income".into();
    }
    let parts: Vec<String> = rules
        .resources
        .resources
        .iter()
        .filter_map(|r| {
            let n = *d.income.get(&r.id)?;
            (n > 0).then(|| format!("+{n} {}", r.name))
        })
        .collect();
    let from = d.income_from_ds / 10;
    format!(
        "{} every {} s from {}:{:02}",
        parts.join(", "),
        d.income_interval_ds / 10,
        from / 60,
        from % 60
    )
}

/// Apply one typed key to the seed field: a digit appends (at most
/// [`SEED_DIGITS_MAX`] digits), Backspace drops the last; anything else is
/// ignored.
pub fn edit_seed(seed: u64, key: KeyCode) -> u64 {
    match (key, digit_of(key)) {
        (KeyCode::Backspace, _) => seed / 10,
        (_, Some(d)) if seed.to_string().len() < SEED_DIGITS_MAX => {
            seed.saturating_mul(10).saturating_add(d)
        }
        _ => seed,
    }
}

/// The digit a key types (main row or numpad).
fn digit_of(key: KeyCode) -> Option<u64> {
    match key {
        KeyCode::Digit0 | KeyCode::Numpad0 => Some(0),
        KeyCode::Digit1 | KeyCode::Numpad1 => Some(1),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some(2),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some(3),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some(4),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some(5),
        KeyCode::Digit6 | KeyCode::Numpad6 => Some(6),
        KeyCode::Digit7 | KeyCode::Numpad7 => Some(7),
        KeyCode::Digit8 | KeyCode::Numpad8 => Some(8),
        KeyCode::Digit9 | KeyCode::Numpad9 => Some(9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn rules() -> Rules {
        Rules::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).unwrap()
    }

    /// New Game builds the chosen difficulty, style and seed.
    #[test]
    fn new_game_builds_the_selected_difficulty_style_and_seed() {
        let r = rules();
        let s = NewGameSettings {
            difficulty: "hard".into(),
            style: StyleChoice::Id("rush".into()),
            seed: 42,
            spectate: None,
            seed_typed: false,
        };
        let setup = s.setup(&r, 42).unwrap();
        assert_eq!(setup, MatchSetup::vs_bot(&r, 42, "hard", "rush"));
        assert!(!s.hides_style());
        let spec = NewGameSettings {
            spectate: Some([
                ("easy".into(), "boom".into()),
                ("hard".into(), "tower".into()),
            ]),
            ..s
        };
        assert_eq!(
            spec.setup(&r, 9).unwrap(),
            MatchSetup::bots(&r, 9, ("easy", "boom"), ("hard", "tower"))
        );
        assert_eq!(
            bonus_caption(&r, "standard"),
            "+20 Grain, +20 Lumber, +10 Ore every 25 s from 3:00"
        );
        assert_eq!(bonus_caption(&r, "passive"), "No bonus income");
        assert_eq!(style_label(&r, "standard"), "Balanced");
        assert_eq!(style_label(&r, "rush"), "Rush");
        assert_eq!(difficulty_label(&r, "hard"), "Hard");
    }

    /// Random draws one of rush, boom and tower from the seed, the one
    /// `ai::resolve_personality` names, and the header records it.
    #[test]
    fn random_style_is_resolved_from_the_seed_and_in_the_header() {
        let r = rules();
        let s = NewGameSettings::default();
        assert!(s.hides_style());
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..=30 {
            let setup = s.setup(&r, seed).unwrap();
            let p = setup.ai_seats[0].personality.clone();
            assert_eq!(p, ai::resolve_personality(&r, seed, PlayerId(1)).unwrap());
            assert!(["rush", "boom", "tower"].contains(&p.as_str()), "{p}");
            assert_eq!(setup.ai_seats[0].bot, "standard");
            seen.insert(p);
        }
        assert_eq!(seen.len(), 3, "every style comes up");
    }

    /// The seed field takes digits (main row and numpad) and Backspace
    /// only, at most ten digits.
    #[test]
    fn seed_field_accepts_digits_and_backspace_only() {
        let mut seed = 0;
        for k in [
            KeyCode::Digit4,
            KeyCode::Numpad2,
            KeyCode::KeyA,
            KeyCode::Space,
        ] {
            seed = edit_seed(seed, k);
        }
        assert_eq!(seed, 42);
        assert_eq!(edit_seed(seed, KeyCode::Backspace), 4);
        assert_eq!(edit_seed(0, KeyCode::Backspace), 0);
        let full = 1_234_567_890;
        assert_eq!(edit_seed(full, KeyCode::Digit9), full, "ten digits at most");
        // Typed over a seed set for the player, the first digit replaces it.
        for prefilled in [140_397_499, 2_707_532_408, 7] {
            let mut s = NewGameSettings::default();
            s.offer_seed(prefilled);
            for k in [
                KeyCode::Digit1,
                KeyCode::KeyA,
                KeyCode::Digit1,
                KeyCode::Digit2,
            ] {
                s.type_seed_key(k);
            }
            assert_eq!(s.seed, 112, "{prefilled}");
            s.type_seed_key(KeyCode::Backspace);
            assert_eq!(s.seed, 11);
        }
        let mut s = NewGameSettings::default();
        s.offer_seed(140_397_499);
        s.type_seed_key(KeyCode::Backspace);
        assert_eq!(s.seed, 14_039_749, "Backspace edits the prefilled seed");
        s.type_seed_key(KeyCode::Digit0);
        assert_eq!(s.seed, 140_397_490, "then digits append");
    }
}
