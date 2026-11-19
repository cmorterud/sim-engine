//! The playtest sandbox (README, Playtest): Blue has 5 of every unit and no commander, Red two
//! coastal bases, which a turtling AI holds without ever setting out.

use sim_engine::ai::{AiBrain, Difficulty};
use sim_engine::math::fp::fp_int;
use sim_engine::sim::blueprints::UnitClass;
use sim_engine::sim::components::{Blueprinted, Builder, Team, UnitStats};
use sim_engine::sim::playtest::playtest;

#[test]
fn blue_has_five_of_every_unit_and_no_commander_and_red_two_bases() {
    let world = playtest(1).world;
    let count = |team: Team, key: &str| {
        world.units.iter().filter(|&&(_, entity)| {
            world.ecs.get::<&UnitStats>(entity).unwrap().team == team && world.blueprints.get(world.ecs.get::<&Blueprinted>(entity).unwrap().0).key == key
        }).count()
    };
    for def in world.blueprints.iter().filter(|def| !def.is_structure() && def.class != UnitClass::Ammo && def.speed > fp_int(0)) {
        let expected = if def.is_commander() { 0 } else { 5 };
        assert_eq!(count(Team::Blue, &def.key), expected, "Blue's {}", def.key);
    }
    assert!(world.commanders[Team::Blue.index()].is_none(), "Blue has no commander, so it cannot lose");
    assert!(world.commanders[Team::Red.index()].is_some());
    for (key, bases) in [("land_factory_t2", 2), ("shield_generator", 2), ("t1_torpedo_launcher", 4), ("naval_factory", 1)] {
        assert_eq!(count(Team::Red, key), bases, "Red's {key}s");
    }
    // And its anti-air range, a row of each tech level, beside what its bases have, and two
    // cruisers offshore.
    for (key, all) in [("t1_aa_turret", 4 + 4), ("t2_flak", 4 + 2), ("t3_sam", 4), ("t2_cruiser", 2)] {
        assert_eq!(count(Team::Red, key), all, "Red's {key}s");
    }
}

/// Blue can afford a strategic missile launcher and nuclear missiles for it.
#[test]
fn blue_can_afford_a_strategic_launcher_and_nuclear_missiles() {
    let mut world = playtest(1).world;
    world.advance_tick(&[]);
    let launcher = world.blueprints.get(world.blueprints.id("strategic_missile_launcher")).clone();
    let missile = world.blueprints.get(world.blueprints.id("strategic_missile")).clone();
    let blue = &world.economies[Team::Blue.index()];
    assert!(blue.mass >= launcher.mass + missile.mass * 3, "materiel {} for the launcher and three missiles", blue.mass);
    assert!(blue.energy >= launcher.energy + missile.energy * 3, "energy {} for the launcher and three missiles", blue.energy);
    let t3_builder = world.units.iter().any(|&(_, entity)| {
        let def = world.blueprints.get(world.ecs.get::<&Blueprinted>(entity).unwrap().0);
        world.ecs.get::<&UnitStats>(entity).unwrap().team == Team::Blue && def.builds.contains(&world.blueprints.id("strategic_missile_launcher"))
    });
    assert!(t3_builder, "and has engineers that can build the launcher");
}

/// Left alone for ten minutes, the turtle builds and keeps everything that fights at home, and
/// both sides' shields stay up.
#[test]
fn the_turtle_builds_up_and_never_sets_out() {
    let scenario = playtest(1);
    let mut world = scenario.world;
    let mut red = AiBrain::with_difficulty(Team::Red, 1, world.starts, Difficulty::Turtle);
    let structures = |world: &sim_engine::sim::world::SimWorld| world.units.iter().filter(|&&(_, entity)| {
        let stats = world.ecs.get::<&UnitStats>(entity).unwrap();
        stats.team == Team::Red && stats.is_structure
    }).count();
    let before = structures(&world);
    let home = world.starts[Team::Red.index()];
    let outpost = home + sim_engine::math::fp::Vec2Fp::new(fp_int(0), fp_int(5_200));
    for _ in 0..6_000 {
        let commands = red.think(&world);
        world.advance_tick(&commands);
        for &(_, entity) in &world.units {
            let stats = *world.ecs.get::<&UnitStats>(entity).unwrap();
            let builds = world.ecs.get::<&Builder>(entity).is_ok();
            if stats.team != Team::Red || stats.is_structure || builds {
                continue;
            }
            let at = world.ecs.get::<&sim_engine::sim::components::Position>(entity).unwrap().0.xy();
            let near = |base: sim_engine::math::fp::Vec2Fp| (at.x - base.x).to_num::<f64>().hypot((at.y - base.y).to_num::<f64>()) < 3_000.0;
            assert!(near(home) || near(outpost), "a Red unit set out, to {at:?}");
        }
    }
    assert!(world.match_result.is_none(), "with nothing to lose, the match goes on");
    // Blue's power keeps every one of its shields up, and Red's its.
    for &(_, entity) in &world.units {
        if let Ok(shield) = world.ecs.get::<&sim_engine::sim::components::Shield>(entity) {
            let team = world.ecs.get::<&UnitStats>(entity).unwrap().team;
            assert!(shield.is_up(), "a {team:?} shield went down for want of energy");
        }
    }
    assert!(structures(&world) > before, "it should have built more");
}
