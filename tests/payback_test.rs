//! What income is worth (`[economy]` in data/balance_targets.toml, 2026-11-27): every extractor and
//! generator costs its tier's payback of what it makes, an upgrade of what it adds over what it
//! upgrades from, energy at the tier's rate; those in `payback_held`, their own payback. And
//! fabricators turn energy into materiel at their tier's rate.

use sim_engine::sim::blueprints::{Blueprint, BlueprintDb, TICKS_PER_SECOND};

fn economy() -> toml::Table {
    let mut table: toml::Table = sim_engine::balance::TARGETS.parse().expect("data/balance_targets.toml is valid");
    table.remove("economy").and_then(|value| value.as_table().cloned()).expect("[economy]")
}

fn number(value: &toml::Value) -> f64 {
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number")
}

fn by_tier(economy: &toml::Table, key: &str, tier: u8) -> f64 {
    number(&economy[key][usize::from(tier) - 1])
}

/// Materiel a second, its energy at `rate`.
fn income(unit: &Blueprint, rate: f64) -> f64 {
    let per_second = |income: sim_engine::sim::blueprints::Res| income.to_num::<f64>() * TICKS_PER_SECOND as f64;
    per_second(unit.mass_income) + per_second(unit.energy_income) / rate
}

/// A structure that spends far more energy than the materiel it makes: it converts one into the
/// other.
fn is_fabricator(unit: &Blueprint) -> bool {
    unit.is_structure() && unit.mass_income.to_num::<f64>() > 0.0 && unit.upkeep > unit.mass_income * 10
}

#[test]
fn every_extractor_and_generator_pays_back_in_its_tiers_payback() {
    let (economy, db) = (economy(), BlueprintDb::standard());
    let held = economy["payback_held"].as_table().expect("[economy] payback_held");
    let mut checked = 0;
    for unit in db.iter().filter(|unit| unit.is_structure() && !is_fabricator(unit)) {
        let rate = by_tier(&economy, "energy_per_materiel", unit.tier);
        let from = db.iter().find(|other| other.upgrades_to.is_some_and(|to| db.get(to).key == unit.key));
        let adds = income(unit, rate) - from.map_or(0.0, |from| income(from, rate));
        if adds <= 0.0 {
            continue;
        }
        let cost = unit.mass.to_num::<f64>() + unit.energy.to_num::<f64>() / rate;
        let wanted = held.get(&unit.key).map_or_else(|| by_tier(&economy, "payback_seconds", unit.tier), number);
        let pays_back = cost / adds;
        assert!((pays_back / wanted - 1.0).abs() < 0.01, "{} pays back in {pays_back:.1} s, not {wanted}", unit.key);
        checked += 1;
    }
    assert!(checked >= 7, "only {checked} extractors and generators");
    for key in held.keys() {
        assert!(db.find(key).is_some(), "{key} in payback_held is a unit");
    }
}

#[test]
fn fabricators_turn_energy_into_materiel_at_their_tiers_rate() {
    let (economy, db) = (economy(), BlueprintDb::standard());
    let mut checked = 0;
    for unit in db.iter().filter(|unit| is_fabricator(unit)) {
        let rate = by_tier(&economy, "energy_per_materiel", unit.tier);
        let converts = unit.upkeep.to_num::<f64>() / unit.mass_income.to_num::<f64>();
        assert!((converts / rate - 1.0).abs() < 1e-4, "{} turns {converts} energy into a materiel, not {rate}", unit.key);
        checked += 1;
    }
    assert_eq!(checked, 2, "the two fabricators");
}
