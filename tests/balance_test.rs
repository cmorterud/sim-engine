//! The balance harness (BALANCE.md): its duels are fair, repeatable, and measure what they say.

use std::sync::Arc;

use sim_engine::balance::{
    anti_air_strength, break_even, break_even_forces, defenses_against_line, domain_force, duel, force_materiel, mirrored, mirrored_on, position_materiel_at, purpose_strength, siege_on,
    squadrons, Field, Ground, Side, Tactics,
    T1_TORPEDO_POSITION, T2_POSITION, T3_POSITION, TORPEDO_POSITION,
};
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

/// BALANCE.md, T3 land and [kills]: Mobile Heavy Artillery is a glass cannon with a wide blast,
/// and a siege piece: it loses to what reaches it. Units spread out and charging beat it, and so
/// do raiders, which its inaccuracy misses and its least range lets in. Until 2026-11-24 it also
/// had to beat Medium Tanks that attack-moved into it bunched up; since it fires half as often,
/// to sit under the battleship's turrets, that is no longer asked of it.
#[test]
fn t3_artillery_loses_to_spread_units_and_raiders() {
    let db = db();
    let side = |key: &str, tactics: Tactics| Side { unit: db.id(key), count: (3_600.0 / db.get(db.id(key)).mass.to_num::<f64>()).round() as u32, tactics };
    let artillery = side("t3_mobile_artillery", Tactics::AttackMove);
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

/// The T3 fighter against the T1 interceptor ([air.t3_fighter]): more damage a second,
/// one shot to destroy one, more range and speed, a quicker turn, and the better buy in a fight
/// at T3 prices, so that spamming interceptors against it loses.
#[test]
fn the_t3_fighter_outclasses_the_t1_interceptor() {
    use sim_engine::balance::{bought_at, cost_at};
    let targets: toml::Table = include_str!("../data/balance_targets.toml").parse().expect("valid targets");
    let rules = targets["air"]["t3_fighter"].as_table().expect("[air.t3_fighter]");
    let number = |key: &str| rules[key].as_float().unwrap_or_else(|| panic!("{key} is a number"));
    let range = |key: &str| {
        let bounds = rules[key].as_array().unwrap_or_else(|| panic!("{key} is a range"));
        let value = |at: usize| bounds[at].as_float().or_else(|| bounds[at].as_integer().map(|value| value as f64)).expect("a number");
        (value(0), value(1))
    };
    let db = db();
    let (fighter, interceptor) = (db.get(db.id("t3_air_superiority")), db.get(db.id("t1_interceptor")));
    let (gun, small) = (fighter.weapon.expect("a gun"), interceptor.weapon.expect("a gun"));
    let dps = |weapon: sim_engine::sim::blueprints::WeaponDef| weapon.damage.to_num::<f64>() * 10.0 / f64::from(weapon.reload_ticks.max(1));
    assert!(dps(gun) > dps(small), "{} times the damage a second", dps(gun) / dps(small));
    let bomber = db.get(db.id("t3_strategic_bomber"));
    let over = fighter.speed.to_num::<f64>() / bomber.speed.to_num::<f64>();
    assert!((over - number("speed_over_strategic_bomber")).abs() < 0.02, "{over} times the Strategic Bomber's speed");
    assert!(gun.damage >= interceptor.health, "a shot of {} against {} health", gun.damage, interceptor.health);
    let (low, high) = range("range_over_interceptor");
    let reach = gun.range.to_num::<f64>() / small.range.to_num::<f64>();
    assert!((low..=high).contains(&reach), "{reach} times the range");
    assert!(fighter.speed > interceptor.speed, "faster");
    let turn = |unit: &sim_engine::sim::blueprints::Blueprint| f64::from(unit.flight.expect("flies").turn_rate);
    assert!(turn(fighter) >= turn(interceptor) * number("turn_over_interceptor"), "turns at {}", turn(fighter));
    assert!(gun.arc.is_none(), "its guns fire off its nose");
    let materiel = 6.0 * cost_at(fighter, 3);
    let fight = mirrored(&db, bought_at(&db, "t3_air_superiority", materiel, 3), bought_at(&db, "t1_interceptor", materiel, 3), 1.0);
    assert!(fight.score >= number("beats_interceptors_by"), "six fighters score {} against as much in interceptors", fight.score);
}

/// The sea grounds (2026-11-24, for fights between domains) are as fair as the land: equal fleets
/// come out even both ways round, every duel at sea is the same each time, and a fleet breaks even
/// with itself.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn duels_at_sea_are_fair_and_repeatable() {
    let db = db();
    let field = Field::new(Ground::Water, 2);
    // Destroyers: cruisers' missile defenses stop each other's every missile, and a fleet of them
    // never finishes.
    let fleet = [Side { unit: db.id("t2_destroyer"), count: 3, tactics: Tactics::AttackMove }];
    let result = mirrored_on(&db, field, &fleet, &fleet, 1.0);
    assert!(result.finished && result.seconds > 5.0, "the fleets fight until one is sunk: {result:?}");
    assert!(result.score.abs() < 0.05, "equal fleets, played both ways round, come out even: {result:?}");
    assert!(mirrored_on(&db, field, &fleet, &fleet, 1.25).score > 0.2, "a quarter more health, against a fifth less, wins clearly at sea");
    assert!(mirrored_on(&db, field, &fleet, &fleet, 0.8).score < -0.2);
    assert_eq!(mirrored_on(&db, field, &fleet, &fleet, 1.0), result, "and every duel at sea is the same each time");
    let even = break_even_forces(&db, field, &fleet, &fleet);
    assert!((even.efficiency - 1.0).abs() < 0.05 && !even.at_limit, "a fleet is as good a buy as itself: {even:?}");
}

/// On the shore ground the sea is Blue's side and the land Red's: ships start at sea and shell a
/// position built ashore, and nothing on land walks out to them.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn ships_on_the_shore_ground_shell_a_position_ashore() {
    let db = db();
    let cruisers = [Side { unit: db.id("t2_cruiser"), count: 4, tactics: Tactics::AttackMove }];
    let result = siege_on(&db, Field::new(Ground::Shore, 2), &cruisers, T2_POSITION);
    assert!(result.seconds > 0.0, "the cruisers reach the position from the sea: {result:?}");
}

const TARGETS: &str = include_str!("../data/balance_targets.toml");

/// A value in `[domains]`, or in one of its tables.
fn domains(path: &[&str]) -> toml::Value {
    let mut value: toml::Value = TARGETS.parse::<toml::Table>().expect("data/balance_targets.toml is valid").remove("domains").expect("[domains]");
    for key in path {
        value = value.get(key).unwrap_or_else(|| panic!("[domains] has {path:?}")).clone();
    }
    value
}

fn number(value: &toml::Value) -> f64 {
    value.as_float().or_else(|| value.as_integer().map(|value| value as f64)).expect("a number")
}

/// `first`, so many squadrons of it, against an equal cost of `second`, on a field.
fn fight(db: &Arc<BlueprintDb>, field: Field, first: &str, how_many: f64, second: &str) -> f64 {
    let squadron = domain_force(db, first);
    let one = force_materiel(db, &squadrons(db, &squadron, 0.0, field.tier, Tactics::AttackMove), field.tier);
    let a = squadrons(db, &squadron, one * how_many, field.tier, Tactics::AttackMove);
    let b = squadrons(db, &domain_force(db, second), force_materiel(db, &a, field.tier), field.tier, Tactics::AttackMove);
    mirrored_on(db, field, &a, &b, 1.0).score
}

/// BALANCE.md, T1 navy (2026-11-24): the frigate, the attack submarine, and the T1 torpedo
/// launcher are as strong per cost against the anchor as the inspiration has its own against its
/// T1 tank, within `[domains] strength_band`.
#[test]
fn t1_navy_is_as_strong_per_cost_as_the_inspirations() {
    let db = db();
    let band = domains(&["strength_band"]).as_array().expect("a band").iter().map(number).collect::<Vec<f64>>();
    for key in ["t1_frigate", "t1_attack_sub", "t1_torpedo_launcher"] {
        let ratio = purpose_strength(&db, key) / number(&domains(&["strength_over_anchor", key]));
        assert!((band[0]..=band[1]).contains(&ratio), "{key} is {ratio:.2} of the inspiration's strength per cost");
    }
}

/// `[domains]`: an equal cost of frigates closing on T1 torpedo launchers does not break them.
/// Frigates against T1 bombers are set by `[kill_times]` (tests/kill_times_test.rs).
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn frigates_lose_to_t1_torpedo_launchers() {
    let db = db();
    let frigates = squadrons(&db, &[(db.id("t1_frigate"), 1)], position_materiel_at(&db, T1_TORPEDO_POSITION, 1), 1, Tactics::AttackMove);
    let siege = siege_on(&db, Field::new(Ground::Shore, 1), &frigates, T1_TORPEDO_POSITION);
    assert!(!siege.broken, "an equal cost of frigates broke the T1 torpedo launchers: {siege:?}");
}

/// `[domains] torpedo_boats_beat_submarines`: the sub hunter is a hard counter, beating an equal
/// cost of submarines (2026-11-26).
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn torpedo_boats_beat_an_equal_cost_of_submarines() {
    let db = db();
    let score = fight(&db, Field::new(Ground::Water, 2), "t2_torpedo_boats", 4.0, "t1_submarines");
    let least = number(&domains(&["cells", "torpedo_boats_beat_submarines"]));
    assert!(score >= least, "Torpedo Boats score {score:+.2} against an equal cost of submarines, under {least:+.2}");
}

/// `[domains]`, cells met before any naval pass that must stay met: skipping the counter loses (a
/// fleet with no anti-air to torpedo bombers, one with no anti-submarine weapon to submarines),
/// destroyers do not break T2 torpedo launchers, and cruisers and battleships outrange the point
/// defenses of their tier.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn skipping_the_counter_still_loses_and_ships_still_outrange_the_coast() {
    let db = db();
    let water = Field::new(Ground::Water, 2);
    let bare = fight(&db, water, "t2_fleet_without_anti_air", 4.0, "t2_torpedo_bombers");
    assert!(bare <= number(&domains(&["cells", "fleet_without_anti_air_loses_to_torpedo_bombers"])), "a fleet with no anti-air scores {bare:+.2} against torpedo bombers");
    let submarines = fight(&db, water, "t1_submarines", 24.0, "t2_fleet_without_anti_submarine");
    assert!(submarines >= number(&domains(&["cells", "submarines_beat_fleet_without_anti_submarine"])), "submarines score {submarines:+.2} against a fleet with no anti-submarine weapon");
    let destroyers = squadrons(&db, &[(db.id("t2_destroyer"), 1)], position_materiel_at(&db, TORPEDO_POSITION, 2), 2, Tactics::AttackMove);
    assert!(!siege_on(&db, Field::new(Ground::Shore, 2), &destroyers, TORPEDO_POSITION).broken, "an equal cost of destroyers broke the T2 torpedo launchers");
    let edge = sim_engine::balance::target_number("siege", "outrange_defense_min");
    for (ship, defense) in domains(&["cells", "coastal_ships"]).as_table().expect("a table") {
        let reach = |key: &str| sim_engine::balance::purpose_weapons(db.get(db.id(key)), sim_engine::balance::Purpose::Surface).1;
        let over = reach(ship) / reach(defense.as_str().expect("a key")) - 1.0;
        assert!(over >= edge, "{ship} outranges {defense} by only {:.0}%", over * 100.0);
    }
}

/// BALANCE.md, T2 air and navy (2026-11-24): each is as strong per cost against the anchor as the
/// inspiration has its own against its T1 tank, within `[domains] strength_band`.
#[test]
fn t2_air_and_navy_are_as_strong_per_cost_as_the_inspirations() {
    let db = db();
    let band = domains(&["strength_band"]).as_array().expect("a band").iter().map(number).collect::<Vec<f64>>();
    for key in ["t2_destroyer", "t2_cruiser", "t2_torpedo_boat", "t2_torpedo_launcher", "t2_gunship", "t2_torpedo_bomber", "t2_fighter_bomber"] {
        let ratio = purpose_strength(&db, key) / number(&domains(&["strength_over_anchor", key]));
        assert!((band[0]..=band[1]).contains(&ratio), "{key} is {ratio:.2} of the inspiration's strength per cost");
    }
}

/// `[domains]`: a composed T2 fleet rules the water. It beats an equal cost of the T2 air force,
/// torpedo bombers need over twice its cost to break it, and hovercraft lose to it. Its
/// destroyers alone lose to an equal cost of submarines (2026-11-25).
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn a_t2_fleet_rules_the_water() {
    let db = db();
    let water = Field::new(Ground::Water, 2);
    let cell = |key: &str| number(&domains(&["cells", key]));
    let air = fight(&db, water, "t2_fleet", 2.0, "t2_air_force");
    assert!(air >= cell("fleet_beats_air_force_t2"), "the T2 fleet scores {air:+.2} against an equal cost of the T2 air force");
    let fleet = squadrons(&db, &domain_force(&db, "t2_fleet"), 0.0, 2, Tactics::AttackMove);
    let fleet = squadrons(&db, &domain_force(&db, "t2_fleet"), force_materiel(&db, &fleet, 2) * 2.0, 2, Tactics::AttackMove);
    let bombers = squadrons(&db, &domain_force(&db, "t2_torpedo_bombers"), force_materiel(&db, &fleet, 2), 2, Tactics::AttackMove);
    let multiple = break_even_forces(&db, water, &fleet, &bombers).efficiency;
    let band = domains(&["cells", "torpedo_bombers_to_break_fleet_t2"]).as_array().expect("a band").iter().map(number).collect::<Vec<f64>>();
    assert!((band[0]..=band[1]).contains(&multiple), "torpedo bombers break the T2 fleet at {multiple:.2} times its cost");
    let hunters = fight(&db, water, "t2_destroyers", 4.0, "t1_submarines");
    assert!(-hunters >= cell("submarines_beat_destroyers"), "submarines score {:+.2} against an equal cost of destroyers", -hunters);
    let hovercraft = fight(&db, water, "t2_fleet", 2.0, "t2_hovercraft");
    assert!(-hovercraft <= cell("hovercraft_loses_to_fleet"), "hovercraft score {:+.2} against the T2 fleet", -hovercraft);
}

/// `[domains]`: at T2, the AI's air force comes out even with a land army a fifth flak, and two
/// cruisers break a coastal position of T2 point defenses under a dome, from beyond their reach.
/// One cannot, by design: the dome heals as fast as its missiles strike (2026-11-25).
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn t2_air_meets_a_land_army_evenly_and_cruisers_break_a_coastal_position() {
    let db = db();
    let even = fight(&db, Field::new(Ground::Land, 2), "t2_land_army", 4.0, "t2_air_force");
    let margin = sim_engine::balance::target_number("counters", "even_margin");
    assert!(even.abs() <= margin, "a T2 land army a fifth flak scores {even:+.2} against the T2 air force");
    let count = number(&domains(&["cells", "coastal_ship_count"])) as u32;
    let cruisers = [Side { unit: db.id("t2_cruiser"), count, tactics: Tactics::AttackMove }];
    let siege = siege_on(&db, Field::new(Ground::Shore, 2), &cruisers, T2_POSITION);
    let (seconds, losses) = (number(&domains(&["cells", "coastal_seconds_max"])), number(&domains(&["cells", "coastal_losses_max"])));
    assert!(siege.broken && siege.seconds <= seconds && siege.losses <= losses, "{count} cruisers against a coastal T2 position: {siege:?}");
    let one = siege_on(&db, Field::new(Ground::Shore, 2), &[Side { count: 1, ..cruisers[0] }], T2_POSITION);
    assert!(!one.broken, "one cruiser broke the coastal T2 position through its dome: {one:?}");
}

/// BALANCE.md, T3 air and navy (2026-11-24): the battleship, battlecruiser, and heavy gunship are
/// as strong per cost against the anchor as the inspiration has its own against its T1 tank,
/// within `[domains] strength_band`.
#[test]
fn t3_air_and_navy_are_as_strong_per_cost_as_the_inspirations() {
    let db = db();
    let band = domains(&["strength_band"]).as_array().expect("a band").iter().map(number).collect::<Vec<f64>>();
    for key in ["t3_battleship", "t3_battlecruiser", "t3_heavy_gunship"] {
        let ratio = purpose_strength(&db, key) / number(&domains(&["strength_over_anchor", key]));
        assert!((band[0]..=band[1]).contains(&ratio), "{key} is {ratio:.2} of the inspiration's strength per cost");
    }
}

/// `[domains]`: a massed T3 fleet, escorts two-thirds of its cost, beats an equal cost of the T3
/// air force, and torpedo bombers need over twice its cost to break it; without its escorts, a
/// battleship-heavy fleet breaks to its own cost of them.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn a_massed_t3_fleet_is_as_safe_from_the_air_as_it_is_escorted() {
    let db = db();
    let water = Field::new(Ground::Water, 3);
    let air = fight(&db, water, "t3_fleet", 1.0, "t3_air_force");
    assert!(air >= number(&domains(&["cells", "fleet_beats_air_force_t3"])), "the massed T3 fleet scores {air:+.2} against an equal cost of the T3 air force");
    let multiple = |name: &str| {
        let squadron = domain_force(&db, name);
        let one = force_materiel(&db, &squadrons(&db, &squadron, 0.0, 3, Tactics::AttackMove), 3);
        let fleet = squadrons(&db, &squadron, one * 2.0, 3, Tactics::AttackMove);
        let bombers = squadrons(&db, &domain_force(&db, "t2_torpedo_bombers"), force_materiel(&db, &fleet, 3), 3, Tactics::AttackMove);
        break_even_forces(&db, water, &fleet, &bombers).efficiency
    };
    let band = domains(&["cells", "torpedo_bombers_to_break_fleet_t3"]).as_array().expect("a band").iter().map(number).collect::<Vec<f64>>();
    let escorted = multiple("t3_fleet");
    assert!((band[0]..=band[1]).contains(&escorted), "torpedo bombers break the massed T3 fleet at {escorted:.2} times its cost");
    let heavy = multiple("t3_battleship_heavy_fleet");
    let most = number(&domains(&["cells", "battleship_heavy_fleet_breaks_to_torpedo_bombers_at_most"]));
    assert!(heavy <= most, "torpedo bombers break a battleship-heavy fleet at {heavy:.2} times its cost");
}

/// `[domains]`: two battleships break a coastal T3 position from beyond its reach, and so does
/// one, more slowly: its broadsides wear the heavy dome down faster than it heals (2026-11-25).
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn battleships_break_a_coastal_t3_position() {
    let db = db();
    let count = number(&domains(&["cells", "coastal_ship_count"])) as u32;
    let battleships = [Side { unit: db.id("t3_battleship"), count, tactics: Tactics::AttackMove }];
    let siege = siege_on(&db, Field::new(Ground::Shore, 3), &battleships, T3_POSITION);
    let (seconds, losses) = (number(&domains(&["cells", "coastal_seconds_max"])), number(&domains(&["cells", "coastal_losses_max"])));
    assert!(siege.broken && siege.seconds <= seconds && siege.losses <= losses, "{count} battleships against a coastal T3 position: {siege:?}");
    let one = siege_on(&db, Field::new(Ground::Shore, 3), &[Side { count: 1, ..battleships[0] }], T3_POSITION);
    assert!(one.broken && one.losses <= losses && one.seconds > siege.seconds, "one battleship against a coastal T3 position: {one:?}, against two's {siege:?}");
}

fn premiums(key: &str) -> toml::Value {
    TARGETS.parse::<toml::Table>().expect("data/balance_targets.toml is valid")["premiums"][key].clone()
}

/// `[premiums]` (2026-11-25): what cannot move is worth more for its cost, so static anti-air is
/// half again as strong per cost against aircraft as the mobile anti-air of its tier.
#[test]
fn static_anti_air_is_worth_more_for_its_cost_than_mobile() {
    let db = db();
    let band = premiums("static_anti_air_over_mobile").as_array().expect("a band").iter().map(number).collect::<Vec<f64>>();
    for (tier, fixed, mobile) in [(1, "t1_aa_turret", "t1_mobile_aa"), (2, "t2_flak", "t2_mobile_flak")] {
        let ratio = anti_air_strength(&db, fixed, tier) / anti_air_strength(&db, mobile, tier);
        assert!((band[0]..=band[1]).contains(&ratio), "{fixed} is {ratio:.2} times as strong per cost against aircraft as {mobile}");
    }
}

/// `[premiums]`: three point defenses of each tier hold against half again their cost in the line
/// unit of their tier attack-moving into them; and the hovercraft, which crosses water, is weaker
/// per materiel than the Heavy Tank on land.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn point_defenses_are_worth_more_and_hovercraft_less_for_their_cost() {
    let db = db();
    let times = number(&premiums("point_defense_holds_against_line_units"));
    for tier in 1..=3 {
        let siege = defenses_against_line(&db, tier, times);
        assert!(!siege.broken, "three T{tier} point defenses fell to {times} times their cost in the line unit: {siege:?}");
    }
    let hover = squadrons(&db, &domain_force(&db, "t2_hovercraft"), 2_400.0, 2, Tactics::AttackMove);
    let tanks = squadrons(&db, &domain_force(&db, "t2_heavy_tanks"), force_materiel(&db, &hover, 2), 2, Tactics::AttackMove);
    let efficiency = break_even_forces(&db, Field::new(Ground::Land, 2), &hover, &tanks).efficiency;
    let band = premiums("hovercraft_over_heavy_tank_on_land").as_array().expect("a band").iter().map(number).collect::<Vec<f64>>();
    assert!((band[0]..=band[1]).contains(&efficiency), "the hovercraft is {efficiency:.2} as strong per materiel as the Heavy Tank on land");
}

/// `[domains] strength_over_anchor` (2026-11-25): the fighters are as strong per cost against
/// aircraft, against the anchor, as the inspiration has its own, within the band, with the damage
/// `[air]` fixes for them (an interceptor kills a bomber in about ten seconds; the T3 fighter has
/// nine to eleven times its damage and destroys one with a shot).
#[test]
fn fighters_are_as_strong_per_cost_as_the_inspirations() {
    let db = db();
    let band = domains(&["strength_band"]).as_array().expect("a band").iter().map(number).collect::<Vec<f64>>();
    for key in ["t1_interceptor", "t3_air_superiority"] {
        let ratio = purpose_strength(&db, key) / number(&domains(&["strength_over_anchor", key]));
        assert!((band[0]..=band[1]).contains(&ratio), "{key} is {ratio:.2} of the inspiration's strength per cost");
    }
    let interceptor = db.get(db.id("t1_interceptor"));
    let fighter = db.get(db.id("t3_air_superiority"));
    assert!(fighter.weapon.expect("a gun").damage >= interceptor.health, "the T3 fighter destroys an interceptor with a shot");
}
