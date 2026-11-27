//! Shared constants (2026-11-27, `[constants]` in data/balance_targets.toml): the family rules
//! every unit of a family follows, and the mechanic rules the simulation keeps, written down as
//! the design owner's.

use sim_engine::balance::TARGETS;
use sim_engine::price_list::{domain_of, Domain};
use sim_engine::sim::blueprints::{Blueprint, BlueprintDb, Targets, WeaponDef, WeaponKind, TICKS_PER_SECOND};

fn constants() -> toml::Table {
    let mut table: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    table.remove("constants").and_then(|value| value.as_table().cloned()).expect("[constants]")
}

fn number(value: &toml::Value) -> f64 {
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number")
}

fn integers(value: &toml::Value) -> Vec<i64> {
    value.as_array().expect("a list").iter().map(|value| value.as_integer().expect("a whole number")).collect()
}

fn weapons(unit: &Blueprint) -> impl Iterator<Item = &WeaponDef> {
    unit.weapon.iter().chain(&unit.extra_weapons)
}

fn speed(weapon: &WeaponDef) -> f64 {
    weapon.projectile_speed.to_num()
}

/// Every ship's surface gun but the battleship's lobbed shells, and every torpedo, flies at its
/// family's speed.
#[test]
fn ship_guns_and_torpedoes_fly_at_their_familys_speed() {
    let (constants, db) = (constants(), BlueprintDb::standard());
    let (gun, torpedo) = (number(&constants["ship_gun_speed"]), number(&constants["torpedo_speed"]));
    let mut guns = 0;
    for unit in db.iter() {
        for weapon in weapons(unit) {
            if weapon.targets == Targets::Naval {
                assert_eq!(speed(weapon), torpedo, "{}'s torpedo", unit.key);
            } else if domain_of(unit) == Domain::Navy && weapon.kind == WeaponKind::Shell && weapon.targets != Targets::Air && !weapon.lobbed {
                assert_eq!(speed(weapon), gun, "{}'s gun", unit.key);
                guns += 1;
            }
        }
    }
    assert!(guns >= 4, "the family has its guns");
}

/// A tactical missile flies at its launcher's family's speed, and every strategic missile at one.
#[test]
fn missiles_fly_at_their_familys_speed() {
    let (constants, db) = (constants(), BlueprintDb::standard());
    let tactical = &constants["tactical_missile_speed"];
    let strategic = number(&constants["strategic_missile_speed"]);
    for unit in db.iter() {
        for weapon in weapons(unit) {
            let family = if unit.is_structure() {
                "structure"
            } else if domain_of(unit) == Domain::Navy {
                "ship"
            } else {
                "vehicle"
            };
            match weapon.kind {
                WeaponKind::TacticalMissile => assert_eq!(speed(weapon), number(&tactical[family]), "{}'s missile, a {family}'s", unit.key),
                WeaponKind::StrategicMissile => assert_eq!(speed(weapon), strategic, "{}'s strategic missile", unit.key),
                _ => {}
            }
        }
    }
}

/// The fighters' guns fly at one speed, and the fighters and tactical bombers turn on one
/// radius, each at its own speed.
#[test]
fn fighters_shoot_and_turn_as_their_family() {
    let (constants, db) = (constants(), BlueprintDb::standard());
    let (gun, radius) = (number(&constants["fighter_gun_speed"]), number(&constants["fighter_turn_radius"]));
    for key in ["t1_interceptor", "t3_air_superiority", "t2_fighter_bomber"] {
        let unit = db.get(db.id(key));
        let anti_air = weapons(unit).find(|weapon| weapon.targets == Targets::Air).expect("a fighter's gun");
        assert_eq!(speed(anti_air), gun, "{key}'s gun");
    }
    for key in ["t1_interceptor", "t1_bomber", "t2_fighter_bomber"] {
        let unit = db.get(db.id(key));
        let turn = f64::from(unit.flight.expect("an aircraft").turn_rate).to_radians();
        let turns_on = unit.speed.to_num::<f64>() * TICKS_PER_SECOND as f64 / turn;
        assert!((turns_on - radius).abs() < 1.0, "{key} turns on {turns_on:.1}, not {radius}");
    }
}

/// The veterancy and wreckage rules the simulation keeps are the ones written down.
#[test]
fn veterancy_and_wreckage_are_the_rules_written_down() {
    use sim_engine::sim::config::{WRECK_HEALTH_PERMILLE, WRECK_MASS_PERMILLE, WRECK_SUNK_PERMILLE, WRECK_TECH_PERMILLE};
    use sim_engine::sim::world::{COMMANDER_PROMOTION_HEAL_PERMILLE, COMMANDER_VET_MASS, MAX_RANK, PROMOTION_HEAL_PERMILLE, RANK_HEALTH_PERMILLE, VET_MASS_PERMILLE_BY_TIER, VET_MASS_PERMILLE_SUPPORT, VET_REGEN_ROWS};
    let constants = constants();
    let veterancy = &constants["veterancy"];
    assert_eq!(veterancy["max_rank"].as_integer(), Some(i64::from(MAX_RANK)));
    assert_eq!(integers(&veterancy["mass_permille_by_tier"]), VET_MASS_PERMILLE_BY_TIER);
    assert_eq!(veterancy["mass_permille_field_rig"].as_integer(), Some(VET_MASS_PERMILLE_SUPPORT));
    assert_eq!(integers(&veterancy["commander_mass"]), COMMANDER_VET_MASS);
    assert_eq!(veterancy["rank_health_permille"].as_integer(), Some(RANK_HEALTH_PERMILLE));
    assert_eq!(veterancy["promotion_heal_permille"].as_integer(), Some(PROMOTION_HEAL_PERMILLE));
    assert_eq!(integers(&veterancy["commander_promotion_heal_permille"]), COMMANDER_PROMOTION_HEAL_PERMILLE);
    assert_eq!(integers(&veterancy["regen_rows"]), VET_REGEN_ROWS);
    let wreckage = &constants["wreckage"];
    assert_eq!(wreckage["mass_permille"].as_integer(), Some(WRECK_MASS_PERMILLE));
    assert_eq!(integers(&wreckage["tech_permille"]), WRECK_TECH_PERMILLE);
    assert_eq!(wreckage["sunk_permille"].as_integer(), Some(WRECK_SUNK_PERMILLE));
    assert_eq!(wreckage["health_permille"].as_integer(), Some(WRECK_HEALTH_PERMILLE));
}
