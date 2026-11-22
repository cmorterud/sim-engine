//! Kinds of AI (GAME_SPECIFICATION.md §17.1): a strategy is what an AI plays, apart from its
//! difficulty, which is how well. Strategies are data, in data/ai_strategies.toml, so that new
//! ones are written and tuned without changing the AI. A strategy scales its difficulty's own
//! numbers, and whatever it leaves out is Balanced's, which is the AI as it always played.

use std::sync::OnceLock;

use serde::Deserialize;

const DATA: &str = include_str!("../data/ai_strategies.toml");

/// One strategy, as data/ai_strategies.toml describes it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    /// What it plays, in a sentence, for menus.
    pub plays: String,
    /// What its land factories build at each tier, in turn, by unit key. Balanced's if left out.
    #[serde(default)]
    pub land_t1: Vec<String>,
    #[serde(default)]
    pub land_t2: Vec<String>,
    #[serde(default)]
    pub land_t3: Vec<String>,
    #[serde(default = "one")]
    pub tech_income: f64,
    #[serde(default = "one")]
    pub air_income: f64,
    #[serde(default = "one")]
    pub naval_income: f64,
    #[serde(default = "one")]
    pub land_factories: f64,
    #[serde(default = "one")]
    pub air_factories: f64,
    #[serde(default = "one")]
    pub waves: f64,
    #[serde(default = "one")]
    pub air_waves: f64,
    #[serde(default = "one")]
    pub point_defenses: f64,
    #[serde(default)]
    pub first_attack_minute: f64,
    /// Which factories it builds before taking more deposits: "economy" (none, the default),
    /// "air", or "naval".
    #[serde(default = "economy")]
    pub favours: String,
    /// Whether a factory may upgrade with no other factory still making units.
    #[serde(default)]
    pub upgrade_alone: bool,
    /// No factory upgrades before this minute.
    #[serde(default)]
    pub first_upgrade_minute: f64,
    #[serde(default = "yes")]
    pub attacks: bool,
    #[serde(default = "yes")]
    pub missiles: bool,
    #[serde(default = "yes")]
    pub transports: bool,
    #[serde(default = "yes")]
    pub commander_joins: bool,
    #[serde(default = "yes")]
    pub random: bool,
}

const fn one() -> f64 {
    1.0
}

const fn yes() -> bool {
    true
}

fn economy() -> String {
    "economy".to_string()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    strategy: Vec<Strategy>,
}

impl Strategy {
    /// Every strategy, in the order data/ai_strategies.toml gives them, Balanced first, with
    /// what each leaves out filled in from Balanced.
    pub fn all() -> &'static [Strategy] {
        static ALL: OnceLock<Vec<Strategy>> = OnceLock::new();
        ALL.get_or_init(|| {
            let mut strategies = toml::from_str::<File>(DATA).expect("data/ai_strategies.toml is valid").strategy;
            let balanced = strategies.first().filter(|first| first.id == "balanced").expect("Balanced comes first").clone();
            for strategy in &mut strategies {
                for (mine, base) in [(&mut strategy.land_t1, &balanced.land_t1), (&mut strategy.land_t2, &balanced.land_t2), (&mut strategy.land_t3, &balanced.land_t3)] {
                    if mine.is_empty() {
                        mine.clone_from(base);
                    }
                }
            }
            strategies
        })
    }

    /// The AI as it always played.
    pub fn balanced() -> &'static Strategy {
        &Self::all()[0]
    }

    /// By its id, as given on the command line or in the page address.
    pub fn named(id: &str) -> Option<&'static Strategy> {
        Self::all().iter().find(|strategy| strategy.id == id)
    }

    /// The one "random" picks for this seed and side: any that may be picked, by the seed, so
    /// that a match replays the same. `salt` tells sides apart.
    pub fn random(seed: u64, salt: u64) -> &'static Strategy {
        let choices: Vec<&Strategy> = Self::all().iter().filter(|strategy| strategy.random).collect();
        let mixed = (seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        choices[(mixed >> 33) as usize % choices.len()]
    }

    /// The tick from which it may set out, from `first_attack_minute`.
    pub fn first_attack_ticks(&self) -> u32 {
        (self.first_attack_minute * 600.0).round() as u32
    }

    /// The tick from which its factories may upgrade, from `first_upgrade_minute`.
    pub fn first_upgrade_ticks(&self) -> u32 {
        (self.first_upgrade_minute * 600.0).round() as u32
    }

    /// A multiplier as a percentage, rounded, which the AI works in: whole numbers only, so that
    /// its choices are the same on every machine.
    pub fn percent(multiplier: f64) -> i64 {
        (multiplier * 100.0).round() as i64
    }
}
