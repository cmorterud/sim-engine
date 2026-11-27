//! The battle line (2026-11-27): a force standing in fixed ranks, and aircraft coming at it from
//! the front, a flank and the rear. It sets the T3 air base and the navy base.

use sim_engine::balance::{aircraft_against_army, battle_order, battle_rank, fleet_against_aircraft, target_number, Side, Tactics, TARGETS};
use sim_engine::sim::blueprints::BlueprintDb;

/// Direct fire stands in front, anti-air and gunless shields in the middle, artillery and
/// lobbing guns behind.
#[test]
fn each_unit_takes_its_rank_by_rule() {
    let db = BlueprintDb::standard();
    let rank = |key: &str| battle_rank(db.get(db.id(key)));
    for key in ["t2_heavy_tank", "t3_heavy_walker", "t3_siege_walker", "t2_destroyer", "t3_battlecruiser"] {
        assert_eq!(rank(key), 0, "{key} stands in front");
    }
    for key in ["t2_mobile_flak", "t2_mobile_shield", "t2_cruiser", "t2_escort_corvette"] {
        assert_eq!(rank(key), 1, "{key} stands in the middle");
    }
    for key in ["t3_mobile_artillery", "t2_mobile_missile_launcher", "t3_battleship"] {
        assert_eq!(rank(key), 2, "{key} stands behind");
    }
    // In each rank, the shortest reach is nearest the enemy.
    let side = |key: &str| Side { unit: db.id(key), count: 1, tactics: Tactics::Hold };
    let order: Vec<String> = battle_order(&db, &[side("t3_battleship"), side("t3_battlecruiser"), side("t2_cruiser"), side("t2_destroyer")]).iter().map(|side| db.get(side.unit).key.clone()).collect();
    assert_eq!(order, ["t2_destroyer", "t3_battlecruiser", "t2_cruiser", "t3_battleship"]);
}

/// The T3 air base holds the T3 strike aircraft, against the standard T3 army, to the deal the
/// T2 strike aircraft get against the T2 army (`[battle_line] t3_air`).
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn t3_aircraft_are_as_good_a_buy_against_their_army_as_t2s() {
    let db = BlueprintDb::standard();
    let (t2, t3) = (aircraft_against_army(&db, 2, "t3_air_as"), aircraft_against_army(&db, 3, "t3_air"));
    assert!((t3 / t2 - 1.0).abs() < 0.015, "T3 strike aircraft {t3:.3} against the T3 army, T2's {t2:.3} against the T2 army");
}

/// The navy base holds the T2 fleet at its score against an equal cost of the T2 air force, and
/// a massed T3 fleet rules the water too (`[battle_line] navy`, `[domains.cells]`).
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn fleets_hold_their_scores_against_aircraft() {
    let db = BlueprintDb::standard();
    let target = target_number("battle_line", "navy_score");
    let t2 = fleet_against_aircraft(&db, 2, "navy");
    assert!((t2 - target).abs() < 0.02, "the T2 fleet scores {t2:+.3} against the T2 air force, for {target:+.2}");
    let t3 = fleet_against_aircraft(&db, 3, "navy_t3");
    let targets: toml::Table = TARGETS.parse().expect("data/balance_targets.toml is valid");
    let cell = targets["domains"]["cells"]["fleet_beats_air_force_t3"].as_float().expect("a score");
    assert!(t3 >= cell, "the T3 fleet scores {t3:+.3} against the T3 air force, under {cell:+.2}");
}
