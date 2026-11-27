//! Secondary reaches ([model] in data/balance_targets.toml, 2026-11-27): each weapon that is not a
//! unit's main gun reaches what a rule gives it, from the main guns the derived-stats pass set.

use sim_engine::balance::model_time_to_kill;
use sim_engine::sim::blueprints::{Blueprint, BlueprintDb, Targets, WeaponDef, WeaponKind};

const TARGETS: &str = include_str!("../data/balance_targets.toml");

fn model(key: &str) -> f64 {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let value = &table["model"][key];
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).unwrap_or_else(|| panic!("[model] {key} is a number"))
}

fn model_in(table_key: &str, key: &str) -> f64 {
    let table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let value = &table["model"][table_key][key];
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).unwrap_or_else(|| panic!("[model] {table_key}.{key} is a number"))
}

fn weapons(unit: &Blueprint) -> impl Iterator<Item = &WeaponDef> {
    unit.weapon.iter().chain(&unit.extra_weapons)
}

/// The reach of a unit's one weapon that meets `pick`.
fn reach(db: &BlueprintDb, key: &str, what: &str, pick: impl Fn(&WeaponDef) -> bool) -> f64 {
    let unit = db.get(db.id(key));
    let mut found = weapons(unit).filter(|weapon| pick(weapon));
    let weapon = found.next().unwrap_or_else(|| panic!("{key} has {what}"));
    assert!(found.all(|other| other.range == weapon.range), "{key}'s {what} all reach alike");
    weapon.range.to_num::<f64>()
}

fn main_gun(db: &BlueprintDb, key: &str) -> f64 {
    db.get(db.id(key)).weapon.unwrap_or_else(|| panic!("{key} has a weapon")).range.to_num::<f64>()
}

fn anti_air(db: &BlueprintDb, key: &str) -> f64 {
    reach(db, key, "anti-air", |weapon| weapon.targets == Targets::Air)
}

fn torpedo(db: &BlueprintDb, key: &str) -> f64 {
    reach(db, key, "a torpedo", |weapon| weapon.targets == Targets::Naval)
}

fn ground_gun(db: &BlueprintDb, key: &str) -> f64 {
    reach(db, key, "a gun at the ground", |weapon| weapon.targets == Targets::Ground && weapon.kind == WeaponKind::Shell)
}

/// Within 1%, the rounding a reach is written to.
fn assert_reach(what: &str, actual: f64, rule: f64) {
    assert!((actual / rule - 1.0).abs() <= 0.01, "{what} reaches {actual}, and its rule gives {rule:.1}");
}

#[test]
fn anti_air_reaches_follow_the_family() {
    let db = BlueprintDb::standard();
    let mobile_over_line = model("mobile_anti_air_over_line");
    let static_over_mobile = model("static_anti_air_over_mobile");
    let ship_over_static = model("ship_anti_air_over_static");
    let cruiser_over_static = model("cruiser_anti_air_over_static");

    let mobile = [anti_air(&db, "t1_mobile_aa"), anti_air(&db, "t2_mobile_flak")];
    assert_reach("The Mobile Anti-Air", mobile[0], mobile_over_line * main_gun(&db, "t1_medium_tank"));
    assert_reach("The Mobile Flak", mobile[1], mobile_over_line * main_gun(&db, "t2_heavy_tank"));
    // T3 has no mobile anti-air, so its static anti-air goes by the one it would have.
    let t3_mobile = mobile_over_line * main_gun(&db, "t3_heavy_walker");

    let fixed = [anti_air(&db, "t1_aa_turret"), anti_air(&db, "t2_flak"), anti_air(&db, "t3_sam")];
    assert_reach("The Anti-Air Turret", fixed[0], static_over_mobile * mobile[0]);
    assert_reach("The Flak Cannon", fixed[1], static_over_mobile * mobile[1]);
    assert_reach("The SAM", fixed[2], static_over_mobile * t3_mobile);

    assert_reach("The frigate's anti-air", anti_air(&db, "t1_frigate"), ship_over_static * fixed[0]);
    assert_reach("The destroyer's anti-air", anti_air(&db, "t2_destroyer"), ship_over_static * fixed[1]);
    assert_reach("The battleship's anti-air", anti_air(&db, "t3_battleship"), ship_over_static * fixed[2]);
    assert_reach("The cruiser's anti-air", anti_air(&db, "t2_cruiser"), cruiser_over_static * fixed[1]);
}

#[test]
fn aircraft_secondary_guns_reach_their_rules() {
    let db = BlueprintDb::standard();
    let at_fighter = model("aircraft_anti_air_at_fighter");
    let gun_over_anti_air = model("aircraft_gun_over_anti_air");
    let t3_fighter = anti_air(&db, "t3_air_superiority");
    for key in ["t3_heavy_gunship", "t3_heavy_transport"] {
        assert_reach(&format!("{key}'s anti-air"), anti_air(&db, key), at_fighter * t3_fighter);
    }
    // The T2 aircraft's guns stay inside the T1 Mobile Anti-Air, the T3 aircraft's inside the
    // Mobile Flak, the farthest-reaching mobile anti-air.
    for (key, under) in [("t2_gunship", "t1_mobile_aa"), ("t2_transport", "t1_mobile_aa"), ("t3_heavy_gunship", "t2_mobile_flak"), ("t3_heavy_transport", "t2_mobile_flak")] {
        assert_reach(&format!("{key}'s gun"), ground_gun(&db, key), gun_over_anti_air * anti_air(&db, under));
    }
}

#[test]
fn torpedo_and_support_gun_reaches_follow_their_rules() {
    let db = BlueprintDb::standard();
    let sub_hunter = |tier: u8| {
        model_in("closing_share_role", "sub_hunter") * model("speed_navy") * model_time_to_kill(tier, "navy") * model_in("role_time_to_kill", "sub_hunter")
    };
    assert_reach("The Torpedo Boat", torpedo(&db, "t2_torpedo_boat"), sub_hunter(2));
    assert_reach("The destroyer's torpedo", torpedo(&db, "t2_destroyer"), sub_hunter(2));
    assert_reach("The battlecruiser's torpedo", torpedo(&db, "t3_battlecruiser"), sub_hunter(3));

    let window = model("point_defense_over_direct_fire_min");
    assert_reach("The T1 Torpedo Launcher", torpedo(&db, "t1_torpedo_launcher"), window * main_gun(&db, "t1_frigate"));
    assert_reach("The T2 Torpedo Launcher", torpedo(&db, "t2_torpedo_launcher"), window * main_gun(&db, "t2_destroyer"));

    let cruiser_gun = reach(&db, "t2_cruiser", "a gun", |weapon| weapon.targets == Targets::Ground && weapon.kind == WeaponKind::Shell);
    assert_reach("The cruiser's gun", cruiser_gun, model("support_gun_at_line") * main_gun(&db, "t2_destroyer"));
}
