//! What sets each stat of each unit (2026-11-28), as `price_list::cost_source` says what sets its
//! cost. Every number in data/units.toml other than cost is in one of ten families, and
//! data/stat_sources.toml names, for each unit and each family it has, the rule, the target or
//! the anchor that sets it, or says it is not derived yet and why. tests/stat_sources_test.rs
//! holds the table to the unit data and the targets, and counts what is still to derive: each
//! pass of the derivation plan turns some of it into rules.

use crate::math::fp::Fp;
use crate::sim::blueprints::{Blueprint, BlueprintDb, Res, UnitClass, WeaponDef};

/// The table, data/stat_sources.toml.
pub const SOURCES: &str = include_str!("../data/stat_sources.toml");

/// A group of a unit's stats that one rule usually sets together.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// Hit points.
    Health,
    /// The body: a unit's radius, which a blast catches, and a structure's footprint and its
    /// adjacency size class.
    Body,
    /// Every weapon's damage, a nuclear blast's inner ring too.
    Damage,
    /// Every weapon's reach, and its least range.
    Range,
    /// Top speed, on the ground, at sea or in the air.
    Speed,
    /// Turn rate and radius, and an aircraft's handling: its turn, break-off, fuel, least speed
    /// and height.
    Turning,
    /// Every weapon's reload, volley, shell or missile speed, blast, spread, turret swivel, firing
    /// arc, setup and cruise height.
    Timing,
    /// Vision, radar, sonar and deep scan.
    Sensors,
    /// Income, storage, build power and range, upkeep, energy a shot, and adjacency.
    Economy,
    /// Domes and their upkeep, missile and torpedo defense, transport room, airfield pads, stock
    /// limits, blasts and storms on death, the commander's Discharge, jamming, stealth and cloaking,
    /// stuns and crushes.
    Special,
}

impl Family {
    pub const ALL: [Family; 10] = [Family::Health, Family::Body, Family::Damage, Family::Range, Family::Speed, Family::Turning, Family::Timing, Family::Sensors, Family::Economy, Family::Special];

    /// Its key in data/stat_sources.toml.
    pub const fn key(self) -> &'static str {
        match self {
            Family::Health => "health",
            Family::Body => "body",
            Family::Damage => "damage",
            Family::Range => "range",
            Family::Speed => "speed",
            Family::Turning => "turning",
            Family::Timing => "timing",
            Family::Sensors => "sensors",
            Family::Economy => "economy",
            Family::Special => "special",
        }
    }

    /// Whether a unit has any stat in this family.
    /// A missile has none: its launcher's weapon carries its numbers.
    pub fn applies(self, unit: &Blueprint) -> bool {
        if unit.class == UnitClass::Ammo {
            return false;
        }
        let armed = !weapons(unit).is_empty();
        match self {
            Family::Health | Family::Body | Family::Sensors => true,
            Family::Damage | Family::Range | Family::Timing => armed,
            Family::Speed | Family::Turning => unit.speed > Fp::ZERO,
            Family::Economy => {
                unit.mass_income > Res::ZERO
                    || unit.energy_income > Res::ZERO
                    || unit.mass_storage > Res::ZERO
                    || unit.energy_storage > Res::ZERO
                    || unit.build_power > Fp::ZERO
                    || unit.upkeep > Res::ZERO
                    || unit.adjacency_giver.is_some()
                    || weapons(unit).iter().any(|weapon| weapon.energy_per_shot > Res::ZERO)
            }
            Family::Special => {
                unit.shield.is_some()
                    || unit.missile_defense.is_some()
                    || unit.torpedo_defense.is_some()
                    || unit.transport.is_some()
                    || unit.airfield.is_some()
                    || unit.max_stock > 0
                    || unit.death_explosion.is_some()
                    || unit.death_storm.is_some()
                    || unit.discharge.is_some()
                    || unit.jammer.is_some()
                    || unit.stealth_field > Fp::ZERO
                    || unit.cloak_upkeep.is_some()
                    || weapons(unit).iter().any(|weapon| weapon.stun_ticks > 0 || weapon.crush_ticks > 0)
            }
        }
    }
}

/// A unit's weapons, its main one first.
fn weapons(unit: &Blueprint) -> Vec<&WeaponDef> {
    unit.weapon.iter().chain(&unit.extra_weapons).collect()
}

/// How a family's stats are set.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// By a rule of the model, or a target the design owner set.
    Rule,
    /// The design owner's own number, which others are set from.
    Anchor,
    /// Not derived yet: it waits for a rule.
    NotDerived,
}

impl Kind {
    /// The words a source starts with.
    pub const fn prefix(self) -> &'static str {
        match self {
            Kind::Rule => "rule: ",
            Kind::Anchor => "anchor: ",
            Kind::NotDerived => "not derived yet: ",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Kind::Rule => "Rule",
            Kind::Anchor => "Anchor",
            Kind::NotDerived => "Not derived yet",
        }
    }

    /// The kind of a source, by the words it starts with.
    pub fn of(source: &str) -> Option<Kind> {
        [Kind::Rule, Kind::Anchor, Kind::NotDerived].into_iter().find(|kind| source.starts_with(kind.prefix()))
    }
}

/// Why a family is not derived yet, by the words after "not derived yet: ".
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// Today's value, which no rule sets.
    Held,
    /// The inspiration's number or ratio.
    Inspiration,
    /// Kept inside a band, or past a floor, that does not pin it.
    Kept,
    /// Today's value, written down as a target.
    WrittenDown,
    /// One of the identity knobs `[constants]` names, at today's value.
    IdentityKnob,
}

impl Reason {
    pub const ALL: [Reason; 5] = [Reason::Held, Reason::Inspiration, Reason::Kept, Reason::WrittenDown, Reason::IdentityKnob];

    pub const fn words(self) -> &'static str {
        match self {
            Reason::Held => "held",
            Reason::Inspiration => "the inspiration's",
            Reason::Kept => "kept",
            Reason::WrittenDown => "today's value written down",
            Reason::IdentityKnob => "an identity knob",
        }
    }

    /// Why a source that is not derived yet is not, by its words.
    pub fn of(source: &str) -> Option<Reason> {
        let why = source.strip_prefix(Kind::NotDerived.prefix())?;
        Reason::ALL.into_iter().find(|reason| why.starts_with(reason.words()))
    }
}

fn table() -> toml::Table {
    SOURCES.parse().expect("data/stat_sources.toml is valid")
}

/// What sets one family of a unit's stats, as data/stat_sources.toml has it. None for a family the
/// table leaves out, which tests/stat_sources_test.rs forbids for one the unit has.
pub fn stat_source(key: &str, family: Family) -> Option<String> {
    table().get("units")?.get(key)?.get(family.key())?.as_str().map(str::to_string)
}

/// Whether a family of a unit's stats is set by a rule or an anchor.
pub fn derived(key: &str, family: Family) -> bool {
    stat_source(key, family).and_then(|source| Kind::of(&source)).is_some_and(|kind| kind != Kind::NotDerived)
}

/// The `[section] key` targets a source cites, in data/balance_targets.toml: a section may be
/// dotted, as `[air.t3_fighter]`, and a key may be dotted into an inline table, as
/// `[kills] t1_bomber.t1_medium_tank`.
pub fn cited_targets(source: &str) -> Vec<(String, String)> {
    let mut cited = Vec::new();
    let mut rest = source;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else { break };
        let section = &after[..close];
        let tail = after[close + 1..].trim_start();
        let key: String = tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.').collect();
        let key = key.trim_end_matches('.');
        if !section.is_empty() && section.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.') {
            cited.push((section.to_string(), key.to_string()));
        }
        rest = &after[close + 1..];
    }
    cited
}

/// The `module::function` code a source cites, in src/balance.rs or src/price_list.rs.
pub fn cited_code(source: &str) -> Vec<(String, String)> {
    let mut cited = Vec::new();
    for word in source.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':')) {
        if let Some((module, function)) = word.split_once("::") {
            if !module.is_empty() && !function.is_empty() && !function.contains(':') {
                cited.push((module.to_string(), function.to_string()));
            }
        }
    }
    cited
}

/// The `--balance-derive stat-sources` report: each unit's families by kind, then each family's
/// count still to derive, and the sources in full for one family if asked (`stat-sources range`).
pub fn sources_report(db: &BlueprintDb, family: Option<&str>) {
    let short = |key: &str, family: Family| -> &'static str {
        match stat_source(key, family).as_deref().and_then(Kind::of) {
            Some(Kind::Rule) => "Rule",
            Some(Kind::Anchor) => "Anchor",
            Some(Kind::NotDerived) => "·",
            None => "**none**",
        }
    };
    if let Some(name) = family {
        let Some(family) = Family::ALL.into_iter().find(|family| family.key() == name) else {
            println!("No family {name}: the families are {}.", Family::ALL.map(Family::key).join(", "));
            return;
        };
        println!("| Unit | Tier | What sets its {name} |");
        println!("| :-- | --: | :-- |");
        for unit in db.iter().filter(|unit| family.applies(unit)) {
            println!("| {} | T{} | {} |", unit.key, unit.tier, stat_source(&unit.key, family).unwrap_or_else(|| "**nothing**".to_string()));
        }
        return;
    }
    println!("| Unit | Tier | {} |", Family::ALL.map(Family::key).join(" | "));
    println!("| :-- | --: |{}", " :-: |".repeat(Family::ALL.len()));
    for unit in db.iter() {
        let cells: Vec<&str> = Family::ALL.iter().map(|&family| if family.applies(unit) { short(&unit.key, family) } else { "" }).collect();
        println!("| {} | T{} | {} |", unit.key, unit.tier, cells.join(" | "));
    }
    println!();
    let (mut all, mut waiting) = (0, 0);
    for family in Family::ALL {
        let has: Vec<&Blueprint> = db.iter().filter(|unit| family.applies(unit)).collect();
        let left = has.iter().filter(|unit| !derived(&unit.key, family)).count();
        println!("{}: {} of {} not derived yet", family.key(), left, has.len());
        all += has.len();
        waiting += left;
    }
    println!("\n{waiting} of {all} unit families not derived yet ({} derived)", all - waiting);
    for reason in Reason::ALL {
        let count = db
            .iter()
            .flat_map(|unit| Family::ALL.into_iter().filter(|family| family.applies(unit)).filter_map(|family| stat_source(&unit.key, family)))
            .filter(|source| Reason::of(source) == Some(reason))
            .count();
        println!("- {}: {count}", reason.words());
    }
}
