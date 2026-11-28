//! What sets each stat (data/stat_sources.toml, 2026-11-28): every family of every unit names its
//! rule, its anchor, or why it is not derived yet; every citation resolves; the rules the table
//! claims that no other test checks are checked here; and what is still to derive can only fall.

use sim_engine::balance::{damage_per_second, model_time_to_kill, t1_land_roles, t2_land_roles, t3_land_roles, Anchor, Role};
use sim_engine::sim::blueprints::{Blueprint, BlueprintDb, Targets, WeaponDef, WeaponKind};
use sim_engine::stat_sources::{cited_code, cited_targets, derived, stat_source, Family, Kind, Reason, SOURCES};

const TARGETS: &str = include_str!("../data/balance_targets.toml");
const BALANCE: &str = include_str!("../src/balance.rs");
const PRICE_LIST: &str = include_str!("../src/price_list.rs");

/// Unit families not derived yet. A pass that derives some lowers this; nothing raises it.
const NOT_DERIVED_YET: usize = 461;

fn targets() -> toml::Table {
    TARGETS.parse().expect("data/balance_targets.toml is valid")
}

fn sources() -> toml::Table {
    SOURCES.parse().expect("data/stat_sources.toml is valid")
}

fn number(value: &toml::Value) -> f64 {
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number")
}

fn model(key: &str) -> f64 {
    number(&targets()["model"][key])
}

fn model_list(key: &str, tier: u8) -> f64 {
    number(&targets()["model"][key].as_array().unwrap_or_else(|| panic!("[model] {key} is a list"))[usize::from(tier) - 1])
}

#[test]
fn every_family_a_unit_has_names_what_sets_it() {
    let db = BlueprintDb::standard();
    let table = sources();
    let units = table["units"].as_table().expect("[units]");
    for unit in db.iter() {
        for family in Family::ALL {
            let source = stat_source(&unit.key, family);
            assert_eq!(source.is_some(), family.applies(unit), "{}'s {} {}", unit.key, family.key(), if family.applies(unit) { "is not named" } else { "is named, but it has none" });
        }
        if let Some(named) = units.get(&unit.key).and_then(toml::Value::as_table) {
            for key in named.keys() {
                assert!(Family::ALL.iter().any(|family| family.key() == key), "{} names a family {key} that does not exist", unit.key);
            }
        }
    }
    for key in units.keys() {
        assert!(db.find(key).is_some(), "data/stat_sources.toml names {key}, which is not a unit");
    }
}

#[test]
fn every_source_says_how_it_is_set_and_cites_what_sets_it() {
    let db = BlueprintDb::standard();
    for unit in db.iter() {
        for family in Family::ALL.into_iter().filter(|family| family.applies(unit)) {
            let source = stat_source(&unit.key, family).expect("named");
            let what = format!("{}'s {}: {source}", unit.key, family.key());
            let kind = Kind::of(&source).unwrap_or_else(|| panic!("{what} starts with none of rule, anchor, or not derived yet"));
            if kind == Kind::NotDerived {
                assert!(Reason::of(&source).is_some(), "{what} does not say why: held, the inspiration's, kept, today's value written down, or an identity knob");
            } else {
                assert!(!cited_targets(&source).is_empty() || !cited_code(&source).is_empty(), "{what} cites no target and no code");
            }
        }
    }
}

/// A `[section] key` in the targets, with a dotted section or key reaching into a table.
fn resolves(targets: &toml::Table, section: &str, key: &str) -> bool {
    let mut value = toml::Value::Table(targets.clone());
    for part in section.split('.').chain(key.split('.').filter(|part| !part.is_empty())) {
        match value.get(part) {
            Some(next) => value = next.clone(),
            None => return false,
        }
    }
    true
}

#[test]
fn every_citation_resolves() {
    let db = BlueprintDb::standard();
    let targets = targets();
    for unit in db.iter() {
        for family in Family::ALL.into_iter().filter(|family| family.applies(unit)) {
            let source = stat_source(&unit.key, family).expect("named");
            for (section, key) in cited_targets(&source) {
                assert!(resolves(&targets, &section, &key), "{}'s {} cites [{section}] {key}, which data/balance_targets.toml does not have", unit.key, family.key());
            }
            for (module, function) in cited_code(&source) {
                let code = match module.as_str() {
                    "balance" => BALANCE,
                    "price_list" => PRICE_LIST,
                    other => panic!("{}'s {} cites {other}::{function}: only balance and price_list are cited", unit.key, family.key()),
                };
                assert!(code.contains(&format!("fn {function}(")), "{}'s {} cites {module}::{function}, which does not exist", unit.key, family.key());
            }
        }
    }
}

#[test]
fn what_is_not_derived_yet_only_falls() {
    let db = BlueprintDb::standard();
    let left = db.iter().map(|unit| Family::ALL.into_iter().filter(|&family| family.applies(unit) && !derived(&unit.key, family)).count()).sum::<usize>();
    assert!(left <= NOT_DERIVED_YET, "{left} unit families are not derived yet, more than {NOT_DERIVED_YET}: a rule was lost");
    assert_eq!(left, NOT_DERIVED_YET, "{left} unit families are not derived yet: lower NOT_DERIVED_YET to it");
}

fn weapons(unit: &Blueprint) -> impl Iterator<Item = &WeaponDef> {
    unit.weapon.iter().chain(&unit.extra_weapons)
}

fn unit<'a>(db: &'a BlueprintDb, key: &str) -> &'a Blueprint {
    db.get(db.id(key))
}

fn range(db: &BlueprintDb, key: &str) -> f64 {
    unit(db, key).weapon.unwrap_or_else(|| panic!("{key} has a weapon")).range.to_num::<f64>()
}

/// Within this share, the rounding a stat is written to.
fn assert_near(what: &str, actual: f64, rule: f64, share: f64) {
    assert!((actual / rule - 1.0).abs() <= share, "{what} is {actual}, and its rule gives {rule:.2}");
}

/// Whether the table says this template sets this family of this unit.
fn set_by(key: &str, family: Family, template: &str) -> bool {
    stat_source(key, family).is_some_and(|source| Kind::of(&source) == Some(Kind::Rule) && source.contains(&format!("balance::{template}")))
}

/// The land role templates (balance::t1_land_roles and on) set what the table says they do: each
/// unit has its role's numbers, its strength whatever its duel target solved.
#[test]
fn each_land_template_sets_what_the_table_says() {
    let db = BlueprintDb::standard();
    let anchor = Anchor::from_targets();
    let templates: [(&str, Vec<Role>); 3] = [("t1_land_roles", t1_land_roles()), ("t2_land_roles", t2_land_roles()), ("t3_land_roles", t3_land_roles())];
    for (template, roles) in templates {
        for role in roles {
            let key = role.unit;
            let actual = unit(&db, key);
            let gun = actual.weapon.unwrap_or_else(|| panic!("{key} has a gun"));
            let strength = (actual.health.to_num::<f64>() * damage_per_second(&gun)).sqrt() / (anchor.efficiency() * role.cost);
            let applied = sim_engine::balance::apply(&db, &anchor, &role, strength);
            let wanted = unit(&applied, key);
            let wanted_gun = wanted.weapon.expect("a gun");
            let set = |family: Family| set_by(key, family, template);
            if set(Family::Health) || set(Family::Damage) {
                // Within half a point of health and a twentieth of a point of damage, as each is
                // rounded, or half a percent.
                let health = actual.health.to_num::<f64>();
                let rounding = (0.5 / health + 0.05 / gun.damage.to_num::<f64>()).max(0.005);
                assert_near(&format!("{key}'s time to kill"), health / damage_per_second(&gun), anchor.time_to_kill() * role.time_to_kill, rounding);
            }
            if set(Family::Body) {
                assert_eq!(actual.radius, wanted.radius, "{key}'s radius");
            }
            if set(Family::Range) {
                assert_eq!((gun.range, gun.min_range), (wanted_gun.range, wanted_gun.min_range), "{key}'s reach and least range");
            }
            if set(Family::Speed) {
                assert_eq!(actual.speed, wanted.speed, "{key}'s speed");
            }
            if set(Family::Sensors) {
                assert_eq!(actual.vision, wanted.vision, "{key}'s vision");
            }
            // The template's timing, which the table counts with the turret it does not set.
            let timing = stat_source(key, Family::Timing).unwrap_or_default();
            if timing.contains(&format!("balance::{template}")) {
                assert_eq!(gun.reload_ticks, wanted_gun.reload_ticks, "{key}'s reload");
                assert_eq!(gun.splash_radius, wanted_gun.splash_radius, "{key}'s blast");
                assert_eq!(gun.spread, wanted_gun.spread, "{key}'s spread");
                assert_eq!(gun.projectile_speed, wanted_gun.projectile_speed, "{key}'s shell speed");
            }
        }
    }
}

/// The time to kill a role at a tier, in a domain: the template role's factor over its tier's
/// line, on the [model] base.
fn role_time_to_kill(roles: &[Role], name: &str) -> f64 {
    let line = roles.iter().find(|role| role.role == "line").expect("a line role").time_to_kill;
    roles.iter().find(|role| role.role == name).unwrap_or_else(|| panic!("a {name} role")).time_to_kill / line
}

/// The [model] rules the table claims that no other test checks: main guns' reach (the defense
/// window, the navy line, the brawler, the battleship over T2 artillery), the navy's times to kill
/// and speeds, and the battlecruiser's targets.
#[test]
fn the_model_rules_the_table_claims_hold() {
    let db = BlueprintDb::standard();
    let reach_within = 0.01;

    // The defense window: artillery reaches its multiple of its tier's line, and a point defense
    // the artillery's over point_defense_under_artillery.
    let artillery = [("t1_light_artillery", "t1_medium_tank", 1), ("t2_mobile_missile_launcher", "t2_heavy_tank", 2), ("t3_mobile_artillery", "t3_heavy_walker", 3)];
    for (key, line, tier) in artillery {
        assert_near(&format!("{key}'s reach"), range(&db, key), model_list("artillery_over_line", tier) * range(&db, line), reach_within);
    }
    let under = model("point_defense_under_artillery");
    for (key, over) in [("t1_point_defense", "t1_light_artillery"), ("t2_point_defense", "t2_mobile_missile_launcher"), ("t3_heavy_point_defense", "t3_mobile_artillery")] {
        assert_near(&format!("{key}'s reach"), range(&db, key), range(&db, over) / under, reach_within);
    }

    // The navy: reach is the closing share of its speed over its time to kill.
    let speed = |key: &str| unit(&db, key).speed.to_num::<f64>() * 10.0;
    let navy_share = |tier: u8| model_list("closing_share_land", tier) * model("closing_share_navy_over_land");
    let brawler = number(&targets()["model"]["closing_share_role"]["brawler"]);
    assert_near("The frigate's reach", range(&db, "t1_frigate"), brawler * speed("t1_frigate") * model_time_to_kill(1, "navy"), reach_within);
    assert_near("The destroyer's reach", range(&db, "t2_destroyer"), navy_share(2) * speed("t2_destroyer") * model_time_to_kill(2, "navy"), reach_within);
    assert_near("The battlecruiser's reach", range(&db, "t3_battlecruiser"), navy_share(3) * speed("t3_battlecruiser") * model_time_to_kill(3, "navy"), reach_within);
    let over_artillery = targets()["domains"]["cells"]["battleship_over_t2_artillery_reach"].as_array().expect("a band").iter().map(number).collect::<Vec<_>>();
    let battleship_over = range(&db, "t3_battleship") / range(&db, "t2_artillery");
    assert!(battleship_over >= over_artillery[0] && battleship_over <= over_artillery[1], "The battleship reaches {battleship_over:.3} times T2 artillery's");

    // Times to kill, health over the damage a second of the guns the role is measured on.
    let guns = |key: &str, pick: &dyn Fn(&WeaponDef) -> bool| -> f64 { weapons(unit(&db, key)).filter(|weapon| pick(weapon)).map(damage_per_second).sum() };
    let at_surface = |weapon: &WeaponDef| weapon.targets == Targets::Ground && weapon.kind != WeaponKind::TacticalMissile;
    let health = |key: &str| unit(&db, key).health.to_num::<f64>();
    let sub_hunter = number(&targets()["model"]["role_time_to_kill"]["sub_hunter"]);
    let heavy = role_time_to_kill(&t3_land_roles(), "heavy");
    assert_near("The frigate's time to kill", health("t1_frigate") / guns("t1_frigate", &at_surface), model_time_to_kill(1, "navy"), 0.01);
    assert_near("The destroyer's time to kill", health("t2_destroyer") / guns("t2_destroyer", &at_surface), model_time_to_kill(2, "navy"), 0.01);
    assert_near("The Torpedo Boat's time to kill", health("t2_torpedo_boat") / guns("t2_torpedo_boat", &|weapon| weapon.targets == Targets::Naval), model_time_to_kill(2, "navy") * sub_hunter, 0.01);
    assert_near("The battleship's time to kill", health("t3_battleship") / guns("t3_battleship", &at_surface), model_time_to_kill(3, "navy") * heavy, 0.01);

    // Speeds: the navy's base at the T2 line's share and the T3 heavy role's, rounded to a tenth
    // a tick, and the battlecruiser half again the battleship's.
    let share = |roles: &[Role], name: &str| roles.iter().find(|role| role.role == name).unwrap_or_else(|| panic!("a {name} role")).speed;
    let navy = model("speed_navy");
    assert_near("The destroyer's speed", speed("t2_destroyer"), navy * share(&t2_land_roles(), "line"), 0.01);
    assert_near("The battleship's speed", speed("t3_battleship"), navy * share(&t3_land_roles(), "heavy"), 0.01);
    assert_near("The battlecruiser's speed", speed("t3_battlecruiser"), model("battlecruiser_speed_over_battleship") * speed("t3_battleship"), 0.005);

    // Each laser at least twice a battleship turret's damage a second.
    let turret = damage_per_second(&unit(&db, "t3_battleship").weapon.expect("turrets"));
    for laser in weapons(unit(&db, "t3_battlecruiser")).filter(|weapon| weapon.kind == WeaponKind::Laser) {
        assert!(damage_per_second(laser) >= model("battlecruiser_laser_over_turret_min") * turret, "a laser does {:.0} a second, a turret {turret:.0}", damage_per_second(laser));
    }

    // The Torpedo Boat's decoys three times as often as the destroyer's.
    let decoys = |key: &str| f64::from(unit(&db, key).torpedo_defense.unwrap_or_else(|| panic!("{key} has decoys")).reload_ticks);
    assert_near("The Torpedo Boat's decoys", decoys("t2_destroyer") / decoys("t2_torpedo_boat"), 3.0, 0.02);
}
