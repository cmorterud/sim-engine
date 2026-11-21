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

/// BALANCE.md, T3 land: artillery beats line units that attack-move into it, keeping about half,
/// and loses to them spread out and charging, as at T1.
#[test]
fn t3_artillery_beats_the_line_unit_unless_it_spreads_out_and_charges() {
    let db = db();
    let side = |key: &str, count: u32, tactics: Tactics| Side { unit: db.id(key), count, tactics };
    let plain = mirrored(&db, side("t3_mobile_artillery", 6, Tactics::AttackMove), side("t3_heavy_walker", 10, Tactics::AttackMove), 1.0);
    assert!((0.4..=0.7).contains(&plain.score), "the artillery keeps {:.2} more than the walkers", plain.score);
    let micro = mirrored(&db, side("t3_heavy_walker", 10, Tactics::Micro), side("t3_mobile_artillery", 6, Tactics::AttackMove), 1.0);
    assert!(micro.score >= 0.3, "spread out and charging, the walkers keep {:.2} more", micro.score);
}
