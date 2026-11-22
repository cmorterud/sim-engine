//! The balance harness (BALANCE.md): its duels are fair, repeatable, and measure what they say.

use std::sync::Arc;

use sim_engine::balance::{break_even, duel, mirrored, Side, Tactics};
use sim_engine::sim::blueprints::BlueprintDb;

fn db() -> Arc<BlueprintDb> {
    Arc::new(BlueprintDb::standard())
}

#[test]
fn a_duel_between_equal_groups_is_even_both_ways_round() {
    let db = db();
    let tank = Side { unit: db.id("t1_medium_tank"), count: 8, tactics: Tactics::AttackMove };
    let result = mirrored(&db, tank, tank, 1.0);
    assert!(result.finished, "one side should be destroyed");
    assert!(result.score.abs() < 0.05, "equal groups, played both ways round, come out even: {result:?}");
    assert_eq!(duel(&db, tank, tank, [1.0, 1.0]), duel(&db, tank, tank, [1.0, 1.0]), "and every duel is the same each time");
}

#[test]
fn more_health_wins_and_a_unit_breaks_even_with_itself() {
    let db = db();
    let tank = Side { unit: db.id("t1_medium_tank"), count: 8, tactics: Tactics::AttackMove };
    assert!(mirrored(&db, tank, tank, 1.25).score > 0.2, "a quarter more health, against a fifth less, wins clearly");
    assert!(mirrored(&db, tank, tank, 0.8).score < -0.2);
    let even = break_even(&db, tank.unit, tank.unit, 600.0, Tactics::AttackMove);
    assert!((even.efficiency - 1.0).abs() < 0.05, "a unit is as good a buy as itself: {even:?}");
    assert!(!even.at_limit);
}

#[test]
fn a_unit_that_cannot_hit_ground_is_reported_at_the_limit() {
    let db = db();
    let even = break_even(&db, db.id("t1_mobile_aa"), db.id("t1_medium_tank"), 600.0, Tactics::AttackMove);
    assert!(even.at_limit && even.health_factor > 1.0, "anti-air cannot win against tanks at any health: {even:?}");
}

#[test]
fn a_break_even_is_the_same_whichever_unit_is_measured_first() {
    let db = db();
    let (tank, artillery) = (db.id("t1_medium_tank"), db.id("t1_light_artillery"));
    let one = break_even(&db, tank, artillery, 600.0, Tactics::AttackMove);
    let other = break_even(&db, artillery, tank, 600.0, Tactics::AttackMove);
    let product = one.efficiency * other.efficiency;
    assert!((product - 1.0).abs() < 0.1, "the two answers should be reciprocal: {one:?} and {other:?}");
}

#[test]
fn a_micro_side_plays_differently_from_attack_moving() {
    let db = db();
    let tank = db.id("t1_medium_tank");
    let micro = Side { unit: tank, count: 6, tactics: Tactics::Micro };
    let plain = Side { unit: tank, count: 6, tactics: Tactics::AttackMove };
    assert_ne!(duel(&db, micro, plain, [1.0, 1.0]), duel(&db, plain, plain, [1.0, 1.0]), "micro changes how a duel plays out");
}

/// BALANCE.md: massed line units against massed artillery is meant to turn on micro.
#[test]
fn micro_is_worth_a_great_deal_against_massed_artillery() {
    let db = db();
    let (tank, artillery) = (db.id("t1_medium_tank"), db.id("t1_light_artillery"));
    let plain = break_even(&db, tank, artillery, 600.0, Tactics::AttackMove);
    let micro = break_even(&db, tank, artillery, 600.0, Tactics::Micro);
    let gain = micro.efficiency / plain.efficiency - 1.0;
    assert!(gain > 0.3, "spreading out and charging should be worth a lot against artillery, not {:.0}%", gain * 100.0);
}

/// BALANCE.md, T2 land: the tier's line unit is about as strong per materiel as T1's
/// (`[tiers] efficiency_multiplier`), so lower tiers stay useful.
#[test]
fn the_t2_line_unit_is_about_as_strong_per_materiel_as_the_t1_line_unit() {
    let db = db();
    let even = break_even(&db, db.id("t2_heavy_tank"), db.id("t1_medium_tank"), 1_440.0, Tactics::AttackMove);
    assert!((1.0..=1.15).contains(&even.efficiency), "the heavy tank is {:.2} times the medium tank per materiel", even.efficiency);
}

/// BALANCE.md, T2 land: the skirmisher beats the line unit attack-moving into it, keeping
/// about half, and loses to it spread out and charging: a micro cell, as artillery is at T1.
#[test]
fn the_t2_skirmisher_beats_the_line_unit_unless_it_spreads_out_and_charges() {
    let db = db();
    let side = |key: &str, count: u32, tactics: Tactics| Side { unit: db.id(key), count, tactics };
    let plain = mirrored(&db, side("t2_grenadier_walker", 10, Tactics::AttackMove), side("t2_heavy_tank", 12, Tactics::AttackMove), 1.0);
    assert!((0.4..=0.7).contains(&plain.score), "the grenadiers keep {:.2} more than the tanks", plain.score);
    let micro = mirrored(&db, side("t2_heavy_tank", 12, Tactics::Micro), side("t2_grenadier_walker", 10, Tactics::AttackMove), 1.0);
    assert!(micro.score >= 0.3, "spread out and charging, the tanks keep {:.2} more", micro.score);
}

/// BALANCE.md, T3 land: the tier's line unit is about as strong per materiel as T2's.
#[test]
fn the_t3_line_unit_is_about_as_strong_per_materiel_as_the_t2_line_unit() {
    let db = db();
    let even = break_even(&db, db.id("t3_heavy_walker"), db.id("t2_heavy_tank"), 3_600.0, Tactics::AttackMove);
    assert!((1.0..=1.15).contains(&even.efficiency), "the heavy walker is {:.2} times the heavy tank per materiel", even.efficiency);
}

/// BALANCE.md, T3 land and [kills]: Mobile Heavy Artillery is a glass cannon with a wide blast.
/// It wrecks line units that attack-move into it bunched up; spread out and charging, they win,
/// and so do raiders, which its inaccuracy misses and its least range lets in.
#[test]
fn t3_artillery_wrecks_clumps_and_loses_to_spread_units_and_raiders() {
    let db = db();
    let side = |key: &str, tactics: Tactics| Side { unit: db.id(key), count: (3_600.0 / db.get(db.id(key)).mass.to_num::<f64>()).round() as u32, tactics };
    let artillery = side("t3_mobile_artillery", Tactics::AttackMove);
    let plain = mirrored(&db, artillery, side("t1_medium_tank", Tactics::AttackMove), 1.0);
    assert!(plain.score >= 0.4, "against Medium Tanks attack-moving, the artillery keeps {:.2} more", plain.score);
    let spread = mirrored(&db, side("t1_medium_tank", Tactics::Micro), artillery, 1.0);
    assert!(spread.score >= 0.3, "spread out and charging, the tanks keep {:.2} more", spread.score);
    let raid = mirrored(&db, side("t1_light_walker", Tactics::AttackMove), artillery, 1.0);
    assert!(raid.score >= 0.3, "raiders keep {:.2} more", raid.score);
}

/// BALANCE.md, [siege]: T3 artillery outranges the T3 heavy point defense by at least 15%, and an
/// equal materiel of it breaks a heavy shield over three heavy point defenses within a minute,
/// losing nothing, since nothing there can reach it.
#[test]
fn t3_artillery_outranges_t3_point_defense_and_breaks_a_shielded_position() {
    use sim_engine::balance::{position_materiel, siege, T3_POSITION};
    let db = db();
    let reach = |key: &str| db.get(db.id(key)).weapon.expect("it has a gun").range.to_num::<f64>();
    let edge = reach("t3_mobile_artillery") / reach("t3_heavy_point_defense");
    assert!(edge >= 1.15, "the artillery reaches only {edge:.2} times as far as the heavy point defense");
    let count = (position_materiel(&db, T3_POSITION) / db.get(db.id("t3_mobile_artillery")).mass.to_num::<f64>()).round() as u32;
    let result = siege(&db, db.id("t3_mobile_artillery"), count, T3_POSITION);
    assert!(result.broken && result.seconds <= 60.0, "{count} artillery broke the position: {}, in {:.0} s", result.broken, result.seconds);
    assert!(result.losses < 0.01, "and lost {:.0}%", result.losses * 100.0);
}

/// BALANCE.md, [missiles]: the missile launcher is priced as T1 artillery is, at the targets'
/// share of its strength per materiel, with no duel in it.
#[test]
fn the_missile_launcher_has_its_share_of_t1_artillerys_strength() {
    use sim_engine::balance::{strength_of, target_number, Anchor};
    let db = db();
    let anchor = Anchor::from_targets();
    let share = strength_of(&db, &anchor, "t2_mobile_missile_launcher") / strength_of(&db, &anchor, "t1_light_artillery");
    let wanted = target_number("missiles", "strength_over_artillery");
    assert!((share - wanted).abs() < 0.02, "the launcher has {share:.3} of T1 artillery's strength per materiel, for {wanted}");
}

/// BALANCE.md, [missiles]: artillery is the better unit, and what keeps moving beats launchers;
/// but an equal materiel of them breaks a shield over T2 point defenses, losing nothing.
#[test]
fn missile_launchers_lose_to_artillery_and_the_line_but_break_a_t2_position() {
    use sim_engine::balance::{position_materiel, siege, target_number, T2_POSITION};
    let db = db();
    let side = |key: &str| Side { unit: db.id(key), count: (1_440.0 / db.get(db.id(key)).mass.to_num::<f64>()).round() as u32, tactics: Tactics::AttackMove };
    let artillery = mirrored(&db, side("t1_light_artillery"), side("t2_mobile_missile_launcher"), 1.0);
    assert!(artillery.score > 0.0, "T1 artillery keeps {:.2} more than the launchers", artillery.score);
    let line = mirrored(&db, side("t2_heavy_tank"), side("t2_mobile_missile_launcher"), 1.0);
    assert!(line.score >= 0.3, "heavy tanks walking in keep {:.2} more than the launchers", line.score);
    let launcher = db.id("t2_mobile_missile_launcher");
    let count = (position_materiel(&db, T2_POSITION) / db.get(launcher).mass.to_num::<f64>()).round() as u32;
    let result = siege(&db, launcher, count, T2_POSITION);
    assert!(result.broken && result.seconds <= target_number("missiles", "siege_seconds_max"), "{count} launchers broke the position: {}, in {:.0} s", result.broken, result.seconds);
    assert!(result.losses < 0.01, "and lost {:.0}%", result.losses * 100.0);
}

/// T1 air is what its rules derive ([air] and [kills]): the derivation, run on the unit data,
/// changes nothing.
#[test]
fn t1_air_is_what_its_rules_derive() {
    let standard = BlueprintDb::standard();
    let derived = sim_engine::balance::derive_t1_air(&standard);
    for key in ["t1_bomber", "t1_interceptor", "t1_mobile_aa"] {
        let (now, rule) = (standard.get(standard.id(key)), derived.get(derived.id(key)));
        assert_eq!((now.mass, now.energy, now.build_time, now.health), (rule.mass, rule.energy, rule.build_time, rule.health), "{key}");
        assert_eq!(now.weapon.map(|weapon| weapon.damage), rule.weapon.map(|weapon| weapon.damage), "{key}'s damage");
    }
}

/// The T1 air rules' fights: five bombers on a commander holding its ground, an interceptor on a
/// bomber's tail, an army a fifth anti-air against bombers, and anti-air alone against them.
#[test]
fn t1_air_meets_its_rules_in_the_duels() {
    use sim_engine::balance::{bought, escorted_army, mirrored_armies, one, strike_seconds, target_number, target_number_range};
    let db = db();
    let raid = mirrored(&db, Side { count: 5, ..one(&db, "t1_bomber") }, Side { tactics: Tactics::Hold, ..one(&db, "command_rig") }, 1.0);
    let (low, high) = target_number_range("air", "commander_seconds_five_bombers_t1");
    assert!(raid.finished && (low..=high).contains(&raid.seconds), "five bombers take {} s on a commander", raid.seconds);
    let caught = strike_seconds(&db, one(&db, "t1_interceptor"), one(&db, "t1_bomber"));
    let wanted = target_number("air", "interceptor_seconds_per_bomber_t1");
    assert!((caught - wanted).abs() <= wanted * 0.2, "an interceptor takes {caught} s on a bomber");
    let materiel = target_number("air", "check_materiel");
    let escorted = mirrored_armies(&db, &escorted_army(&db, materiel, target_number("air", "anti_air_escort_share")), &[bought(&db, "t1_bomber", materiel)], 1.0);
    assert!(escorted.score.abs() <= target_number("counters", "even_margin"), "an escorted army scores {} against bombers", escorted.score);
    let pure = mirrored(&db, bought(&db, "t1_mobile_aa", materiel), bought(&db, "t1_bomber", materiel), 1.0);
    assert!(pure.score >= target_number("air", "anti_air_keeps_min"), "anti-air alone scores {} against bombers", pure.score);
}

/// Mobile Anti-Air is no cheaper a shield than the anchor, and fighters out-turn the T1 bomber.
#[test]
fn anti_air_is_no_better_a_shield_and_fighters_out_turn_bombers() {
    use sim_engine::balance::target_number;
    let db = db();
    let per_mass = |key: &str| db.get(db.id(key)).health.to_num::<f64>() / db.get(db.id(key)).mass.to_num::<f64>();
    assert!(per_mass("t1_mobile_aa") <= per_mass("t1_medium_tank") * 1.01, "anti-air has {} health a mass", per_mass("t1_mobile_aa"));
    let targets: toml::Table = include_str!("../data/balance_targets.toml").parse().expect("valid targets");
    let turn = |key: &str| f64::from(db.get(db.id(key)).flight.unwrap_or_else(|| panic!("{key} flies")).turn_rate);
    let fighters = targets["air"]["fighters"].as_array().expect("a list of fighters");
    assert!(fighters.len() >= 3);
    for fighter in fighters {
        let key = fighter.as_str().expect("a unit key");
        assert!(turn(key) >= turn("t1_bomber") * target_number("air", "fighter_turn_over_t1_bomber"), "{key} turns at {}", turn(key));
    }
}
