//! The hits rules (data/balance_targets.toml, [kills] and [splash]): what the snipe weapons and
//! heavy artillery destroy in how many hits, how wide each splash weapon's blast is, and how far
//! the installations scatter, all checked against the unit data the game plays with.

use sim_engine::sim::blueprints::{Blueprint, BlueprintDb};
use sim_engine::sim::components::MoveClass;

const TARGETS: &str = include_str!("../data/balance_targets.toml");

fn targets() -> toml::Table {
    TARGETS.parse().expect("data/balance_targets.toml is valid")
}

fn number(value: &toml::Value) -> f64 {
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number")
}

/// What it takes to destroy a unit at its base: its health, and a shield it carries.
fn toughness(unit: &Blueprint) -> f64 {
    unit.health.to_num::<f64>() + unit.shield.map_or(0.0, |shield| shield.health.to_num::<f64>())
}

fn damage(unit: &Blueprint) -> f64 {
    unit.weapon.unwrap_or_else(|| panic!("{} has a weapon", unit.key)).damage.to_num::<f64>()
}

/// Each weapon in [kills] destroys each unit listed in exactly that many direct hits.
#[test]
fn every_kills_rule_holds_exactly() {
    let db = BlueprintDb::standard();
    let table = targets();
    let kills = table["kills"].as_table().expect("[kills]");
    let mut checked = 0;
    for (weapon, rule) in kills {
        let Some(rule) = rule.as_table() else { continue };
        let shooter = db.get(db.id(weapon));
        for (target, hits) in rule {
            let unit = db.get(db.id(target));
            let wanted = hits.as_integer().expect("a count of hits");
            let took = (toughness(unit) / damage(shooter)).ceil() as i64;
            assert_eq!(took, wanted, "{weapon} ({} a hit) destroys {target} ({} with shields) in {took} hits, not {wanted}", damage(shooter), toughness(unit));
            checked += 1;
        }
    }
    assert!(checked >= 9, "only {checked} rules");
}

/// The T3 Artillery Installation destroys every T3 land unit with a direct hit, the Field Rig
/// aside; and the installations scatter as far as their rules say.
#[test]
fn the_installations_hit_as_designed() {
    let db = BlueprintDb::standard();
    let table = targets();
    let kills = table["kills"].as_table().expect("[kills]");
    assert_eq!(kills["t3_artillery_kills_every_t3_land_unit"].as_bool(), Some(true));
    let shell = damage(db.get(db.id("t3_artillery")));
    let land: Vec<&Blueprint> = db
        .iter()
        .filter(|unit| unit.tier == 3 && !unit.is_structure() && !unit.is_air() && unit.key != "field_rig")
        .filter(|unit| matches!(unit.movement, MoveClass::Land | MoveClass::Hover | MoveClass::Seabed))
        .collect();
    assert!(land.iter().any(|unit| unit.key == "t3_siege_walker"), "the siege walker is a T3 land unit");
    for unit in land {
        assert!(toughness(unit) <= shell, "a T3 artillery shell of {shell} does not destroy {} ({})", unit.key, toughness(unit));
    }
    for key in ["t3_artillery", "t2_artillery"] {
        let range = table["kills"][format!("{key}_spread")].as_array().expect("a spread range");
        let spread = db.get(db.id(key)).weapon.expect("artillery has a gun").spread.to_num::<f64>();
        assert!((number(&range[0])..=number(&range[1])).contains(&spread), "{key} scatters {spread}");
    }
}

/// Each splash weapon's blast reaches its size's share of a formation spacing of its own tier's
/// line unit: narrow about one, medium about one and a half, wide about three.
#[test]
fn every_splash_weapon_is_its_size() {
    let db = BlueprintDb::standard();
    let table = targets();
    let splash = table["splash"].as_table().expect("[splash]");
    let mut checked = 0;
    for (size, range) in [("narrow", "splash_reach_over_spacing"), ("medium", "medium_reach_over_spacing"), ("wide", "wide_reach_over_spacing")] {
        let bounds = splash[range].as_array().expect("a range");
        let (low, high) = (number(&bounds[0]), number(&bounds[1]));
        for key in splash[size].as_array().expect("a list of units") {
            let unit = db.get(db.id(key.as_str().expect("a unit key")));
            let radius = match unit.tier {
                1 => 9.0,
                2 => 11.0,
                _ => 13.0,
            };
            let blast = unit.weapon.expect("a splash weapon").splash_radius.to_num::<f64>();
            let reach = (blast + radius) / (2.5 * radius);
            assert!((low..=high).contains(&reach), "{} is {size}, but its blast of {blast} reaches {reach:.2} spacings, not {low}-{high}", unit.key);
            checked += 1;
        }
    }
    assert!(checked >= 8, "only {checked} weapons");
}
