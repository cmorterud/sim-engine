//! Kinds of AI (GAME_SPECIFICATION.md §17.1): every strategy in data/ai_strategies.toml is sound
//! data, and each plays as it says, against Balanced, in a match of its own. Balanced is the AI
//! as it always played, which the golden AI match (tests/determinism_test.rs) holds it to.

use std::collections::BTreeMap;
use std::sync::Arc;

use sim_engine::ai::play_match_with;
use sim_engine::ai_strategy::Strategy;
use sim_engine::sim::blueprints::BlueprintDb;
use sim_engine::sim::components::{Blueprinted, MoveClass, Team, UnitStats};
use sim_engine::sim::maps::bundled;
use sim_engine::sim::scenario::MapSource;
use sim_engine::sim::world::SimWorld;

#[test]
fn every_strategy_is_sound_data() {
    let db = BlueprintDb::standard();
    let all = Strategy::all();
    assert_eq!(all[0].id, "balanced", "Balanced comes first");
    let mut ids: Vec<&str> = all.iter().map(|strategy| strategy.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all.len(), "ids are unique");
    for strategy in all {
        for (tier, list) in [(1, &strategy.land_t1), (2, &strategy.land_t2), (3, &strategy.land_t3)] {
            assert!(!list.is_empty(), "{} builds nothing at T{tier}", strategy.id);
            for key in list {
                let unit = db.get(db.id(key));
                assert!(!unit.is_structure() && !unit.is_air() && matches!(unit.movement, MoveClass::Land | MoveClass::Hover | MoveClass::Seabed), "{}: {key} is not a land unit", strategy.id);
                assert!(unit.tier <= tier, "{}: {key} is above T{tier}", strategy.id);
            }
        }
        for (what, value) in [
            ("tech_income", strategy.tech_income),
            ("air_income", strategy.air_income),
            ("naval_income", strategy.naval_income),
            ("land_factories", strategy.land_factories),
            ("air_factories", strategy.air_factories),
            ("waves", strategy.waves),
            ("air_waves", strategy.air_waves),
            ("point_defenses", strategy.point_defenses),
        ] {
            assert!(value > 0.0 && value <= 10.0, "{}: {what} is {value}", strategy.id);
        }
        assert!(["economy", "air", "naval"].contains(&strategy.favours.as_str()), "{}: favours {}", strategy.id, strategy.favours);
        assert!(strategy.first_attack_minute >= 0.0 && strategy.first_upgrade_minute >= 0.0);
    }
    // "random" picks only what may be picked, and the same for the same seed and side.
    for seed in 0..50 {
        let pick = Strategy::random(seed, 1);
        assert!(pick.random, "random picked {}", pick.id);
        assert_eq!(pick.id, Strategy::random(seed, 1).id);
    }
    let picked: std::collections::BTreeSet<&str> = (0..200).map(|seed| Strategy::random(seed, 1).id.as_str()).collect();
    assert_eq!(picked.len(), all.iter().filter(|strategy| strategy.random).count(), "random reaches every strategy it may pick: {picked:?}");
}

/// When things first happened for Blue, by what they are, and its forces at 15 minutes, or when
/// the match ended, if sooner: Rush can win by then.
#[derive(Default)]
struct Milestones {
    first: BTreeMap<&'static str, u32>,
    at_15: BTreeMap<&'static str, u32>,
    attacked: bool,
}

/// Blue playing `strategy` against Balanced for 15 minutes.
fn milestones(map: &MapSource, strategy: &'static Strategy) -> Milestones {
    let mut seen = Milestones::default();
    play_match_with(map, 1, 9_001, [strategy, Strategy::balanced()], |world: &SimWorld| {
        if !world.current_tick.is_multiple_of(50) {
            return;
        }
        let (blue, red) = (world.starts[0], world.starts[1]);
        let mut count: BTreeMap<&'static str, u32> = BTreeMap::new();
        let mut forward = 0;
        for &(id, entity) in &world.units {
            let Ok(stats) = world.ecs.get::<&UnitStats>(entity) else { continue };
            if stats.team != Team::Blue {
                continue;
            }
            let unit = world.blueprints.get(world.ecs.get::<&Blueprinted>(entity).expect("a unit").0);
            let kind = if unit.key.starts_with("land_factory_t2") || unit.key.starts_with("land_factory_t3") {
                "upgraded land factory"
            } else if unit.key.starts_with("air_factory") {
                "air factory"
            } else if unit.key.starts_with("naval_factory") {
                "naval factory"
            } else if unit.is_air() {
                "aircraft"
            } else if !stats.is_structure && matches!(unit.movement, MoveClass::Land | MoveClass::Hover | MoveClass::Seabed) && unit.weapon.is_some() && unit.builds.is_empty() {
                let at = world.unit_position(id).expect("alive").xy();
                if at.distance_squared(&red) < at.distance_squared(&blue) {
                    forward += 1;
                }
                "land unit"
            } else {
                continue;
            };
            *count.entry(kind).or_default() += 1;
            seen.first.entry(kind).or_insert(world.current_tick);
        }
        if forward >= 3 {
            seen.attacked = true;
            seen.first.entry("set out").or_insert(world.current_tick);
        }
        // The last count before 15 minutes stands, if the match ends first.
        if world.current_tick <= 9_000 && world.match_result.is_none() {
            seen.at_15 = count;
        }
    });
    seen
}

/// Each strategy plays as data/ai_strategies.toml says, against Balanced on the same map: Rush
/// sets out sooner with more land units, Tech upgrades a land factory sooner, Air builds air
/// sooner and fields more of it, Naval builds a naval factory sooner on a map with water, and
/// Turtle never sets out.
#[test]
#[cfg_attr(debug_assertions, ignore = "release only")]
fn each_strategy_plays_as_it_says() {
    let named = |id: &str| Strategy::named(id).unwrap_or_else(|| panic!("data/ai_strategies.toml has {id}"));
    let land = MapSource::Skirmish;
    let water = MapSource::Map(Arc::new(bundled("twin_passes").expect("a bundled map")));
    let base = milestones(&land, Strategy::balanced());
    let first = |seen: &Milestones, what: &str| seen.first.get(what).copied().unwrap_or(u32::MAX);
    let at_15 = |seen: &Milestones, what: &str| seen.at_15.get(what).copied().unwrap_or(0);

    let rush = milestones(&land, named("rush"));
    assert!(first(&rush, "set out") < first(&base, "set out"), "Rush sets out at {}, Balanced at {}", first(&rush, "set out"), first(&base, "set out"));
    assert!(at_15(&rush, "land unit") > at_15(&base, "land unit"), "Rush has {} land units, Balanced {}", at_15(&rush, "land unit"), at_15(&base, "land unit"));

    let tech = milestones(&land, named("tech"));
    assert!(first(&tech, "upgraded land factory") < first(&base, "upgraded land factory"), "Tech upgrades at {}, Balanced at {}", first(&tech, "upgraded land factory"), first(&base, "upgraded land factory"));

    let air = milestones(&land, named("air"));
    assert!(first(&air, "air factory") < first(&base, "air factory"), "Air builds air at {}, Balanced at {}", first(&air, "air factory"), first(&base, "air factory"));
    assert!(at_15(&air, "aircraft") > at_15(&base, "aircraft") * 2, "Air has {} aircraft, Balanced {}", at_15(&air, "aircraft"), at_15(&base, "aircraft"));

    let water_base = milestones(&water, Strategy::balanced());
    let naval = milestones(&water, named("naval"));
    assert!(first(&naval, "naval factory") < first(&water_base, "naval factory"), "Naval builds at {}, Balanced at {}", first(&naval, "naval factory"), first(&water_base, "naval factory"));

    let turtle = milestones(&land, named("turtle"));
    assert!(!turtle.attacked, "Turtle set out");
}
