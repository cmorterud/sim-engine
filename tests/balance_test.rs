//! The balance harness (BALANCE.md): its duels are fair, repeatable, and measure what they say.

use std::sync::Arc;

use sim_engine::balance::{break_even, duel, mirrored, Side};
use sim_engine::sim::blueprints::BlueprintDb;

fn db() -> Arc<BlueprintDb> {
    Arc::new(BlueprintDb::standard())
}

#[test]
fn a_duel_between_equal_groups_is_even_both_ways_round() {
    let db = db();
    let tank = Side { unit: db.id("t1_medium_tank"), count: 8 };
    let result = mirrored(&db, tank, tank, 1.0);
    assert!(result.finished, "one side should be destroyed");
    assert!(result.score.abs() < 0.05, "equal groups, played both ways round, come out even: {result:?}");
    assert_eq!(duel(&db, tank, tank, [1.0, 1.0]), duel(&db, tank, tank, [1.0, 1.0]), "and every duel is the same each time");
}

#[test]
fn more_health_wins_and_a_unit_breaks_even_with_itself() {
    let db = db();
    let tank = Side { unit: db.id("t1_medium_tank"), count: 8 };
    assert!(mirrored(&db, tank, tank, 1.5).score > 0.2, "half again the health wins clearly");
    assert!(mirrored(&db, tank, tank, 0.67).score < -0.2);
    let even = break_even(&db, tank.unit, tank.unit, 600.0);
    assert!((even.efficiency - 1.0).abs() < 0.05, "a unit is as good a buy as itself: {even:?}");
    assert!(!even.at_limit);
}

#[test]
fn a_unit_that_cannot_hit_ground_is_reported_at_the_limit() {
    let db = db();
    let even = break_even(&db, db.id("t1_mobile_aa"), db.id("t1_medium_tank"), 600.0);
    assert!(even.at_limit && even.health_factor > 1.0, "anti-air cannot win against tanks at any health: {even:?}");
}
