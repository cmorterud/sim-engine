//! The Command Rig's gun and Discharge (`[commander]` in data/balance_targets.toml, 2026-11-27):
//! holding its ground, it comes out ahead of its count of Medium Tanks attack-moving into it, at the
//! least damage that does, and Discharge destroys any T1 land unit in one.

use std::sync::Arc;

use sim_engine::balance::{mirrored_armies, Side, Tactics};
use sim_engine::math::fp::Fp;
use sim_engine::price_list::{domain_of, Domain};
use sim_engine::sim::blueprints::{BlueprintDb, UnitClass};

fn commander() -> toml::Table {
    let mut table: toml::Table = sim_engine::balance::TARGETS.parse().expect("data/balance_targets.toml is valid");
    table.remove("commander").and_then(|value| value.as_table().cloned()).expect("[commander]")
}

/// The commander, holding, against `tanks` Medium Tanks attack-moving: above zero, it comes out ahead.
fn holds(db: &BlueprintDb, tanks: u32) -> f64 {
    let db = Arc::new(db.clone());
    let (rig, tank) = (db.id("command_rig"), db.id("t1_medium_tank"));
    mirrored_armies(&db, &[Side { unit: rig, count: 1, tactics: Tactics::Hold }], &[Side { unit: tank, count: tanks, tactics: Tactics::AttackMove }], 1.0).score
}

#[test]
fn the_commander_holds_against_its_count_of_medium_tanks_at_the_least_damage_that_does() {
    let db = BlueprintDb::standard();
    let tanks = commander()["holds_against_medium_tanks"].as_integer().expect("a count") as u32;
    let ahead = holds(&db, tanks);
    let behind = holds(&db, tanks + 2);
    assert!(ahead > 0.0, "it comes out {ahead:+.3} against {tanks}");
    assert!(behind < 0.0, "it comes out {behind:+.3} against {}", tanks + 2);
    // Its damage is the least that kills a tank in its count of shots: one shot more loses.
    let rig = db.id("command_rig");
    let damage = db.get(rig).weapon.expect("a gun").damage.to_num::<f64>();
    let health = db.get(db.id("t1_medium_tank")).health.to_num::<f64>();
    let shots = (health / damage).ceil();
    assert!((damage - health / shots).abs() < 1e-6, "{damage} is not the least that kills a tank in {shots} shots");
    let weaker = db.with_unit(rig, |unit| unit.weapon.as_mut().expect("a gun").damage = Fp::from_num(health / (shots + 1.0)));
    let lost = holds(&weaker, tanks);
    assert!(lost < 0.0, "at {:.1}, a tank in {} shots, it still comes out {lost:+.3}", health / (shots + 1.0), shots + 1.0);
}

#[test]
fn discharge_destroys_every_t1_land_unit_in_one() {
    assert_eq!(commander()["discharge_kills_every_t1_land_unit"].as_bool(), Some(true));
    let db = BlueprintDb::standard();
    let discharge = db.get(db.id("command_rig")).discharge.expect("the commander's Discharge");
    let least = discharge.min_damage.to_num::<f64>();
    let mut checked = 0;
    for unit in db.iter().filter(|unit| unit.tier == 1 && !unit.is_structure() && domain_of(unit) == Domain::Land && unit.class != UnitClass::Commander) {
        let toughness = unit.health.to_num::<f64>() + unit.shield.map_or(0.0, |shield| shield.health.to_num::<f64>());
        assert!(least >= toughness, "{} takes {toughness}, more than Discharge's least, {least}", unit.key);
        checked += 1;
    }
    assert!(checked >= 5, "only {checked} T1 land units");
}
