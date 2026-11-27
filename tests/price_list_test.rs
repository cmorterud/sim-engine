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
/// list prices them at, the engineers' 10.4 a point with the Combat Engineer's gun and radar, but
/// for any `[price_list.held]` holds (the T2 Engineer, for the design owner).
#[test]
fn builders_cost_their_build_power() {
    let db = BlueprintDb::standard();
    for key in ["t1_engineer", "t2_engineer", "t3_engineer", "t2_combat_engineer"] {
        let pricing = price_list::price(&db, key).unwrap_or_else(|| panic!("the list prices {key}"));
        assert!(pricing.build_power > 0.0, "{key} builds");
        if price_list::held_reason(key).is_none() {
            assert!(pricing.drift().abs() < 0.01, "{key} costs {:.1}, the list {:.1}", pricing.actual, pricing.list);
        }
    }
}

/// Cargo is a utility job (2026-11-27): the armed transports cost their cargo, at the share of a
/// full load the Light Air Transport pays, and their weapons, which do a quarter of their tier's
/// gunship's damage a second (the design owner's rule).
#[test]
fn transports_cost_their_cargo_and_weak_weapons() {
    let db = BlueprintDb::standard();
    let dps = |key: &str| {
        let unit = db.get(db.id(key));
        unit.weapon.iter().chain(&unit.extra_weapons).map(sim_engine::balance::damage_per_second).sum::<f64>()
    };
    for (key, gunship) in [("t2_transport", "t2_gunship"), ("t3_heavy_transport", "t3_heavy_gunship")] {
        let pricing = price_list::price(&db, key).unwrap_or_else(|| panic!("the list prices {key}"));
        assert!(pricing.cargo > 0.0, "{key} carries");
        assert!(pricing.drift().abs() < 0.01, "{key} costs {:.1}, the list {:.1}", pricing.actual, pricing.list);
        let share = dps(key) / dps(gunship);
        assert!((share - 0.25).abs() < 0.01, "{key}'s weapons do {share:.3} of the {gunship}'s damage a second, not a quarter");
    }
}

/// `[missiles] strategic_minutes` (2026-11-27): each game-ending weapon costs its minutes of a
/// standard T3 economy's income, at T3 prices, and each missile defense its share of the missile
/// it stops (`defense_over_missile`), within 1%.
#[test]
fn strategic_weapons_cost_their_minutes_of_a_t3_economy() {
    let targets: toml::Table = include_str!("../data/balance_targets.toml").parse().expect("valid targets");
    let missiles = &targets["missiles"];
    let number = |value: &toml::Value| value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number");
    let db = BlueprintDb::standard();
    let cost = |key: &str| sim_engine::balance::cost_at(db.get(db.id(key)), 3);
    let per_minute = number(&missiles["t3_economy_extractors"]) * db.get(db.id("mass_extractor_t3")).mass_income.to_num::<f64>() * sim_engine::sim::blueprints::TICKS_PER_SECOND as f64 * 60.0;
    for (key, minutes) in missiles["strategic_minutes"].as_table().expect("[missiles] strategic_minutes") {
        let wanted = number(minutes) * per_minute;
        assert!((cost(key) / wanted - 1.0).abs() < 0.01, "{key} costs {:.0}, {minutes} minutes of a T3 economy {wanted:.0}", cost(key));
    }
    for (key, missile) in [("strategic_interceptor", "strategic_missile"), ("tactical_missile_defense", "tactical_missile")] {
        let share = number(&missiles["defense_over_missile"][key]);
        let tier = db.get(db.id(key)).tier;
        let (own, stops) = (sim_engine::balance::cost_at(db.get(db.id(key)), tier), sim_engine::balance::cost_at(db.get(db.id(missile)), tier));
        assert!((own / stops / share - 1.0).abs() < 0.01, "{key} costs {:.2} of a {missile}, not {share}", own / stops);
    }
}

/// Strategic launching (2026-11-27, the design owner's pick): the Strategic Missile Submarine pays
/// half the Strategic Missile Launcher's cost by its share of the launcher's build power and half
/// by its share of its stock, its build power no builder's, and costs the list's price.
#[test]
fn the_missile_submarine_costs_its_share_of_the_launcher() {
    let db = BlueprintDb::standard();
    let (sub, launcher) = (db.get(db.id("t3_missile_sub")), db.get(db.id("strategic_missile_launcher")));
    let pricing = price_list::price(&db, "t3_missile_sub").expect("the list prices the Missile Submarine");
    let share = (sub.build_power.to_num::<f64>() / launcher.build_power.to_num::<f64>() + f64::from(sub.max_stock) / f64::from(launcher.max_stock)) / 2.0;
    let wanted = share * sim_engine::balance::cost_at(launcher, 3);
    assert!((pricing.launching / wanted - 1.0).abs() < 1e-9, "it pays {:.1} for launching, not {wanted:.1}", pricing.launching);
    assert_eq!(pricing.build_power, 0.0, "its build power is the launching job's");
    assert!(pricing.drift().abs() < 0.01, "it costs {:.1}, the list {:.1}", pricing.actual, pricing.list);
}

/// The Field Rig (2026-11-27, the design owner's plan): its gun, its build power, and its income,
/// what it makes a second paid back in T3's payback, energy at T3's rate, as an extractor's is.
#[test]
fn the_field_rig_costs_its_gun_its_build_power_and_its_income() {
    let db = BlueprintDb::standard();
    let table = targets();
    let rig = db.get(db.id("field_rig"));
    let pricing = price_list::price(&db, "field_rig").expect("the list prices the Field Rig");
    let per_second = |income: sim_engine::sim::blueprints::Res| income.to_num::<f64>() * sim_engine::sim::blueprints::TICKS_PER_SECOND as f64;
    let number = |value: &toml::Value| value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number");
    let (rate, payback) = (number(&table["economy"]["energy_per_materiel"][2]), number(&table["economy"]["payback_seconds"][2]));
    let land_share = 1.0 + number(&table["anchor"]["energy"]) / number(&table["anchor"]["materiel"]) / rate;
    let wanted = (per_second(rig.mass_income) + per_second(rig.energy_income) / rate) * payback / land_share;
    assert!((pricing.income / wanted - 1.0).abs() < 1e-9, "its income costs {:.1}, not {wanted:.1}", pricing.income);
    assert!(pricing.build_power > 0.0 && pricing.jobs.len() == 1, "it pays for its build power and its gun");
    assert!(pricing.drift().abs() < 0.01, "it costs {:.1}, the list {:.1}", pricing.actual, pricing.list);
    assert!(price_list::held_reason("field_rig").is_none(), "nothing holds it");
}

/// T4, for experimentals (2026-11-27): every per-tier table has a T4 entry, and a unit made T4 is
/// priced by them: its fights last the T4 time to kill, 1.45 of T1's, and it is measured against
/// T3's line, at T3's energy price and, in the air, T3's base.
#[test]
fn an_experimental_is_priced_by_the_t4_constants() {
    let targets: toml::Table = include_str!("../data/balance_targets.toml").parse().expect("valid targets");
    let tiers = usize::from(sim_engine::balance::MAX_TIER);
    for (section, key) in [("model", "time_to_kill_tier"), ("model", "closing_share_land"), ("price_list", "tier_efficiency"), ("economy", "energy_per_materiel"), ("economy", "payback_seconds")] {
        assert_eq!(targets[section][key].as_array().map(Vec::len), Some(tiers), "[{section}] {key} has an entry for each tier");
    }
    assert!((sim_engine::balance::model_time_to_kill(4, "land") / sim_engine::balance::model_time_to_kill(1, "land") - 1.45).abs() < 1e-9);
    let db = BlueprintDb::standard();
    for key in ["t3_battleship", "t3_heavy_walker", "t3_strategic_bomber"] {
        let id = db.id(key);
        let experimental = db.with_unit(id, |unit| unit.tier = 4);
        let (t3, t4) = (price_list::price(&db, key).expect("priced at T3"), price_list::price(&experimental, key).expect("priced at T4"));
        assert_eq!(t4.tier, 4);
        assert_eq!(t4.base, t3.base, "{key} at T4 keeps its domain's T3 base");
        assert!(t4.list.is_finite() && t4.list > 0.0, "{key} at T4 lists {}", t4.list);
    }
}
