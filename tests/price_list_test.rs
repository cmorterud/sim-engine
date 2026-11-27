//! The ability price list as code (`sim_engine::price_list`, `[price_list]` in
//! data/balance_targets.toml): every unit with a fighting weapon costs the list's price, or names
//! what else sets its cost (`[price_list.held]`).

use sim_engine::price_list;
use sim_engine::sim::blueprints::BlueprintDb;

const TARGETS: &str = include_str!("../data/balance_targets.toml");

fn targets() -> toml::Table {
    TARGETS.parse().expect("data/balance_targets.toml is valid")
}

#[test]
fn every_unit_the_list_prices_costs_its_price_or_names_what_holds_it() {
    let db = BlueprintDb::standard();
    let table = targets();
    let drift_max = table["price_list"]["drift_max"].as_float().expect("[price_list] drift_max");
    let mut off = Vec::new();
    for unit in db.iter() {
        let Some(pricing) = price_list::price(&db, &unit.key) else { continue };
        let held = price_list::held_reason(&unit.key);
        println!("{:28} list {:8.1}, costs {:8.1} ({:+5.1}%){}", unit.key, pricing.list, pricing.actual, pricing.drift() * 100.0, held.as_deref().map_or(String::new(), |reason| format!(", held: {reason}")));
        if held.is_none() && pricing.drift().abs() > drift_max {
            off.push(format!("{} costs {:.1}, {:+.1}% off the list's {:.1}, and nothing in [price_list.held] holds it", unit.key, pricing.actual, pricing.drift() * 100.0, pricing.list));
        }
    }
    assert!(off.is_empty(), "{off:#?}");
}

#[test]
fn every_held_unit_is_one_the_list_prices() {
    let db = BlueprintDb::standard();
    let table = targets();
    let held = table["price_list"]["held"].as_table().expect("[price_list.held]");
    let stale: Vec<&String> = held.keys().filter(|key| price_list::price(&db, key).is_none()).collect();
    assert!(stale.is_empty(), "[price_list.held] names units the list does not price: {stale:?}");
}

/// The list reproduces the aircraft each tier's air base is solved from or priced at, within
/// rounding: the T1 Bomber, whose cost the escort rule sets and the T1 base prices, and the
/// Interceptor at it; the Gunship, the Heavy Gunship and the T3 fighter at T2's (2026-11-26 and
/// 27).
#[test]
fn the_list_reproduces_the_aircraft_it_priced() {
    let db = BlueprintDb::standard();
    for key in ["t1_bomber", "t1_interceptor", "t2_gunship", "t3_heavy_gunship", "t3_air_superiority"] {
        let pricing = price_list::price(&db, key).expect("an aircraft with a gun");
        assert!(pricing.drift().abs() < 0.01, "{key} costs {:.1}, the list {:.1}", pricing.actual, pricing.list);
    }
}

/// Every dome costs the strength it adds to the group it covers (`[price_list.shield_groups]`,
/// 2026-11-27), within rounding.
#[test]
fn every_dome_costs_the_strength_it_adds() {
    let targets: toml::Table = include_str!("../data/balance_targets.toml").parse().expect("valid targets");
    let groups = targets["price_list"]["shield_groups"].as_table().expect("[price_list.shield_groups]");
    assert!(groups.len() >= 4);
    let db = BlueprintDb::standard();
    for key in groups.keys() {
        let pricing = price_list::shield_price(&db, key).unwrap_or_else(|| panic!("{key} has a dome"));
        assert!(pricing.drift().abs() < 0.01, "{key} costs {:.1}, the strength its dome adds {:.1}", pricing.actual, pricing.list);
    }
}

/// Build power is a utility job (2026-11-27): the engineers and the Combat Engineer cost what the
/// list prices them at, the engineers' 10.4 a point with the Combat Engineer's gun and radar.
#[test]
fn builders_cost_their_build_power() {
    let db = BlueprintDb::standard();
    for key in ["t1_engineer", "t2_engineer", "t3_engineer", "t2_combat_engineer"] {
        let pricing = price_list::price(&db, key).unwrap_or_else(|| panic!("the list prices {key}"));
        assert!(pricing.build_power > 0.0, "{key} builds");
        assert!(pricing.drift().abs() < 0.01, "{key} costs {:.1}, the list {:.1}", pricing.actual, pricing.list);
    }
}
